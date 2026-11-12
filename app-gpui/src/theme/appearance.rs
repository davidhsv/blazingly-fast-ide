//! Applying the `appearance` settings: which theme the windows draw with.
//!
//! The theme is the chosen one, or with "Sync with OS" the preferred light or dark theme for
//! the mode the OS is in. It is re-resolved when a setting changes and when the OS switches
//! mode. A theme picked while the settings cannot be saved still applies, for this session.

use super::themes::{
    Appearance, ContributedThemes, KINDS_KEY, PLUGIN_THEME_ATTRIBUTE, PLUGIN_THEME_KIND_ATTRIBUTE, ThemeChoice,
    ThemeChoiceId, ThemeKind, ThemeSetting, apply_theme, current_theme, current_theme_choice, theme_setting_writes,
    toggled_theme,
};
use super::tokens::Tokens;
use super::{ActiveTheme, tokens::ThemeAttributes};
use crate::backend::Backend;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use gpui_kit::{App, Global, WindowAppearance};
use ide_core::IdeEvent;
use ide_core::settings::AppearanceSettings;
use std::collections::BTreeMap;
use std::rc::Rc;

/// A plugin's theme as it is applied: its tokens over the built-in base of its kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTheme {
    /// `<plugin id>/<theme id>`.
    pub reference: String,
    pub kind: ThemeKind,
    /// Token names with their `--`, to their values.
    pub tokens: BTreeMap<String, String>,
}

/// Something to do after the theme was applied.
type Follower = Rc<dyn Fn(&mut App)>;

/// What the theme is resolved from besides the settings.
pub struct ThemeState {
    /// A theme chosen while the settings could not be saved.
    session_override: Option<ThemeSetting>,
    /// The themes plugins contribute.
    pub contributed: ContributedThemes,
    /// The plugin theme the plugin host has selected.
    plugin_theme: Option<PluginTheme>,
    /// Whether the OS is in light mode.
    system_light: bool,
    /// Called after every [`apply`]: what follows the theme choice (the plugin host's selection).
    after_apply: Vec<Follower>,
}

impl Global for ThemeState {}

impl Default for ThemeState {
    /// No plugin themes, the OS in light mode.
    fn default() -> Self {
        Self {
            session_override: None,
            contributed: ContributedThemes::new(None),
            plugin_theme: None,
            system_light: true,
            after_apply: Vec::new(),
        }
    }
}

fn is_light(appearance: WindowAppearance) -> bool {
    matches!(appearance, WindowAppearance::Light | WindowAppearance::VibrantLight)
}

/// The theme-selecting part of the settings, values as written.
fn selection(settings: &AppearanceSettings) -> Appearance {
    Appearance {
        theme: Some(settings.theme.clone()),
        sync_with_os: settings.sync_with_os,
        preferred_light_theme: Some(settings.preferred_light_theme.clone()),
        preferred_dark_theme: Some(settings.preferred_dark_theme.clone()),
        accent: Some(settings.accent.clone()),
    }
}

/// Starts following the settings and applies the theme they name. Call once the backend runs.
pub fn init(cx: &mut App) {
    let stored_kinds = cx.try_global::<UiStorage>().and_then(|storage| storage.get(KINDS_KEY)).map(str::to_owned);
    let state = ThemeState {
        contributed: ContributedThemes::new(stored_kinds.as_deref()),
        system_light: is_light(cx.window_appearance()),
        ..ThemeState::default()
    };
    cx.set_global(state);
    if cx.has_global::<Backend>() {
        cx.global_mut::<Backend>().listen(|event, cx| {
            if let IdeEvent::SettingsChanged { key } = &event.event
                && (key.is_empty() || key == "appearance" || key.starts_with("appearance."))
            {
                apply(cx);
            }
        });
    }
    apply(cx);
}

/// Resolves the theme from the settings as they stand and switches every window to it.
pub fn apply(cx: &mut App) {
    let appearance = selection(&settings::global(cx).appearance);
    let state = cx.default_global::<ThemeState>();
    // The stylesheet is cascaded afresh from the base attributes, so nothing of the previous
    // theme (its palette, contrast or accent) carries over.
    let root = match &state.plugin_theme {
        Some(plugin) => ThemeAttributes::light()
            .with(PLUGIN_THEME_ATTRIBUTE, &plugin.reference)
            .with(PLUGIN_THEME_KIND_ATTRIBUTE, plugin.kind.as_str()),
        None => ThemeAttributes::light(),
    };
    let applied =
        apply_theme(state.session_override.as_ref(), Some(&appearance), state.system_light, &state.contributed, root);
    if let Some(applied) = applied {
        let mut theme = Tokens::builtin().theme(&applied.attributes);
        // A plugin theme's tokens outrank every built-in theme block; a custom accent outranks
        // them in turn.
        for (name, value) in state.plugin_theme.iter().flat_map(|plugin| &plugin.tokens) {
            if is_token_declaration(name, value) {
                theme.set(name, value);
            }
        }
        for (name, value) in &applied.overrides {
            theme.set(name, value);
        }
        cx.set_global(ActiveTheme { theme });
        cx.refresh_windows();
    }
    let followers = cx.default_global::<ThemeState>().after_apply.clone();
    for follower in followers {
        follower(cx);
    }
}

/// Whether a plugin theme's entry is a token and a plain value: a name of lowercase letters,
/// digits and dashes after `--`, and a value that cannot end the declaration or open a block.
fn is_token_declaration(name: &str, value: &str) -> bool {
    let named = name.strip_prefix("--").is_some_and(|rest| {
        !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
    });
    named && !value.contains([';', '{', '}', '<', '>', '\\'])
}

/// Calls `follower` after every [`apply`], from now on.
pub fn after_apply(follower: impl Fn(&mut App) + 'static, cx: &mut App) {
    cx.default_global::<ThemeState>().after_apply.push(Rc::new(follower));
}

/// The theme the settings select, as selected: a built-in theme or a plugin's.
pub fn current_choice(cx: &App) -> ThemeChoiceId {
    let appearance = selection(&settings::global(cx).appearance);
    match cx.try_global::<ThemeState>() {
        Some(state) => current_theme_choice(state.session_override.as_ref(), Some(&appearance), state.system_light),
        None => current_theme_choice(None, Some(&appearance), true),
    }
}

/// The plugin theme in effect, as `<plugin id>/<theme id>`.
pub fn plugin_theme(cx: &App) -> Option<&str> {
    cx.try_global::<ThemeState>()?.plugin_theme.as_ref().map(|plugin| plugin.reference.as_str())
}

/// The plugin host selected a plugin theme, or none.
pub fn set_plugin_theme(theme: Option<PluginTheme>, cx: &mut App) {
    let state = cx.default_global::<ThemeState>();
    if state.plugin_theme != theme {
        state.plugin_theme = theme;
        apply(cx);
    }
}

/// The themes live plugins offer are now exactly `themes`. The kind each has is remembered, so
/// that a theme whose plugin is off later still shows the built-in base of its kind.
pub fn set_contributed(themes: Vec<ThemeChoice>, cx: &mut App) {
    let state = cx.default_global::<ThemeState>();
    if state.contributed.themes() == themes.as_slice() {
        return;
    }
    for gone in state.contributed.themes().iter().filter(|theme| !themes.contains(theme)).cloned().collect::<Vec<_>>() {
        state.contributed.remove(&gone);
    }
    let mut kinds = None;
    for theme in themes {
        kinds = state.contributed.add(theme).or(kinds);
    }
    if let Some(kinds) = kinds.filter(|_| cx.has_global::<UiStorage>()) {
        cx.global_mut::<UiStorage>().set(KINDS_KEY, kinds);
    }
    apply(cx);
}

/// The OS switched between light and dark mode.
pub fn system_appearance_changed(appearance: WindowAppearance, cx: &mut App) {
    let light = is_light(appearance);
    let state = cx.default_global::<ThemeState>();
    if state.system_light != light {
        state.system_light = light;
        apply(cx);
    }
}

/// The light / dark base of the theme in effect.
pub fn current_kind(cx: &App) -> ThemeKind {
    let appearance = selection(&settings::global(cx).appearance);
    match cx.try_global::<ThemeState>() {
        Some(state) => {
            current_theme(state.session_override.as_ref(), Some(&appearance), state.system_light, &state.contributed)
        }
        None => current_theme(None, Some(&appearance), true, &ContributedThemes::new(None)),
    }
}

/// Selects theme `next` for good: an explicit choice also ends "Sync with OS". Should the
/// settings not take it, the theme still shows until the app quits, and the error is returned.
pub fn set_theme(window: &str, next: ThemeSetting, cx: &mut App) -> anyhow::Result<()> {
    cx.default_global::<ThemeState>().session_override = Some(next.clone());
    let sync = settings::global(cx).appearance.sync_with_os;
    // The first write alone decides whether the choice was saved: it is the theme itself.
    let writes = theme_setting_writes(&next, sync);
    let result = settings::set_global(window, &writes, cx);
    if result.is_ok() {
        cx.default_global::<ThemeState>().session_override = None;
    }
    apply(cx);
    result.map(|_| ()).map_err(|error| anyhow::anyhow!("the theme is not saved: {}", error.message))
}

/// View › Toggle Light/Dark Theme.
pub fn toggle_theme(window: &str, cx: &mut App) -> anyhow::Result<()> {
    let next = toggled_theme(current_kind(cx));
    set_theme(window, ThemeSetting::Choice(ThemeChoiceId::Builtin(next)), cx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::settings::SettingsWrite;
    use crate::theme::themes::{COLOR_THEME_ATTRIBUTE, CONTRAST_ATTRIBUTE};
    use gpui_kit::TestAppContext;
    use serde_json::json;

    fn settle(cx: &mut TestAppContext, done: impl Fn(&mut App) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if cx.update(|cx| done(cx)) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the theme did not follow within three seconds");
    }

    fn dark(cx: &App) -> bool {
        cx.global::<ActiveTheme>().is_dark()
    }

    fn write(key: &'static str, value: serde_json::Value, cx: &mut App) {
        settings::set_global("w1", &[SettingsWrite { key, value }], cx).expect("the write succeeds");
    }

    /// The theme follows the settings whoever writes them: built-in themes, palette themes
    /// over their base, high contrast, and a custom accent that goes away again.
    #[gpui_kit::test]
    async fn the_windows_draw_with_the_theme_the_settings_name(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            init(cx);
            assert!(!dark(cx), "the default theme is light");
            write("appearance.theme", json!("dark"), cx);
        });
        settle(cx, |cx| dark(cx));
        cx.update(|cx| {
            assert_eq!(current_kind(cx), ThemeKind::Dark);
            write("appearance.theme", json!("dark-hc"), cx);
        });
        settle(cx, |cx| cx.global::<ActiveTheme>().theme.attributes().get(CONTRAST_ATTRIBUTE) == Some("high"));

        // A palette theme is its own attribute over a dark or light base.
        let palette = crate::theme::themes::BUILTIN_THEME_IDS
            .into_iter()
            .find(|id| !matches!(id.as_str(), "light" | "dark" | "dark-hc"))
            .expect("a palette theme");
        cx.update(|cx| write("appearance.theme", json!(palette.as_str()), cx));
        settle(cx, |cx| {
            let attributes = cx.global::<ActiveTheme>().theme.attributes().clone();
            attributes.get(COLOR_THEME_ATTRIBUTE) == Some(palette.as_str())
                && attributes.get(CONTRAST_ATTRIBUTE).is_none()
                && attributes.is_light() == (palette.kind() == ThemeKind::Light)
        });

        let accent = |cx: &App| cx.global::<ActiveTheme>().theme.color("accent").map(|c| c.to_hex());
        let own = cx.update(|cx| accent(cx));
        cx.update(|cx| write("appearance.accent", json!("#ff0000"), cx));
        settle(cx, |cx| accent(cx).is_some_and(|hex| hex.eq_ignore_ascii_case("#ff0000")));
        cx.update(|cx| write("appearance.accent", json!(crate::theme::themes::DEFAULT_ACCENT), cx));
        settle(cx, |cx| accent(cx) == own);
    }

    /// With Sync with OS the preferred light or dark theme shows for the OS mode, and the
    /// toggle picks the opposite theme and ends the sync.
    #[gpui_kit::test]
    async fn follows_the_os_mode_until_a_theme_is_chosen(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            init(cx);
            write("appearance.syncWithOs", json!(true), cx);
            system_appearance_changed(WindowAppearance::Dark, cx);
            assert!(dark(cx));
            system_appearance_changed(WindowAppearance::VibrantLight, cx);
            assert!(!dark(cx));
            system_appearance_changed(WindowAppearance::VibrantDark, cx);
            assert!(dark(cx) && current_kind(cx) == ThemeKind::Dark);

            // Dark now: the toggle chooses light, for good.
            toggle_theme("w1", cx).expect("the choice is saved");
            assert!(!dark(cx));
            let appearance = settings::global(cx).appearance;
            assert_eq!(appearance.theme, "light");
            assert!(!appearance.sync_with_os);
            system_appearance_changed(WindowAppearance::Light, cx);
            system_appearance_changed(WindowAppearance::Dark, cx);
            assert!(!dark(cx), "an explicit choice no longer follows the OS");
            toggle_theme("w1", cx).expect("the choice is saved");
            assert!(dark(cx));
        });
    }

    /// A theme chosen while the settings cannot be saved shows for the session and the failure
    /// is reported.
    #[gpui_kit::test]
    async fn a_theme_that_cannot_be_saved_still_shows(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            init(cx);
            let chosen = ThemeSetting::parse("dark").expect("a theme");
            let error = set_theme("w1", chosen, cx).expect_err("there is no backend to save to");
            assert!(error.to_string().contains("not saved"), "{error}");
            assert!(dark(cx));
            assert_eq!(current_kind(cx), ThemeKind::Dark);
        });
    }
}
