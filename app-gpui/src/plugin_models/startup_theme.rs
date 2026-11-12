//! The selected plugin theme across start-ups. The last selection is kept in UI storage, so the next
//! window draws with it before the plugin host has listed anything; it stays until a listing
//! arrives (a failed one does not drop it) and then follows the host. The theme's resolved frame
//! colour, which new windows open in, is reported once per selection.
//!
//! Nothing here reads storage or the host: [`PluginThemeTracker::listed`] says what to apply and
//! what to store, [`restore`] reads back what was stored.

use crate::theme::themes::ThemeKind;
use ide_host::plugins::ActiveTheme;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// UI storage key of the selected plugin theme.
pub const PLUGIN_THEME_CACHE_KEY: &str = "openidea.pluginTheme";
/// UI storage key of the built-in base and the splash colours.
pub const STARTUP_THEME_KEY: &str = "openidea.theme";

/// The selected plugin theme as stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedPluginTheme {
    /// `<pluginId>/<themeId>`.
    pub id: String,
    /// `light` or `dark`.
    pub kind: String,
    pub tokens: BTreeMap<String, String>,
}

impl CachedPluginTheme {
    pub fn of(theme: &ActiveTheme) -> Self {
        let tokens = theme
            .theme
            .tokens
            .iter()
            .filter(|(name, value)| safe_token(name, value))
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let id = format!("{}/{}", theme.plugin_id, theme.theme.id);
        Self { id, kind: kind_of(theme).as_str().to_owned(), tokens }
    }

    pub fn kind(&self) -> ThemeKind {
        if self.kind == "light" { ThemeKind::Light } else { ThemeKind::Dark }
    }
}

fn kind_of(theme: &ActiveTheme) -> ThemeKind {
    match theme.theme.kind {
        ide_host::plugins::ThemeKind::Light => ThemeKind::Light,
        ide_host::plugins::ThemeKind::Dark => ThemeKind::Dark,
    }
}

static TOKEN_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("^--[a-z0-9-]+$").expect("the token name pattern is valid"));
static HEX: LazyLock<Regex> = LazyLock::new(|| Regex::new("^#[0-9a-fA-F]{3,8}$").expect("the hex pattern is valid"));

/// A token that cannot add declarations or load anything.
fn safe_token(name: &str, value: &str) -> bool {
    TOKEN_NAME.is_match(name)
        && !value.contains([';', '{', '}', '<', '>', '\\'])
        && !value.to_ascii_lowercase().contains("url(")
}

/// The splash colours of the next start-up.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupTheme {
    /// The built-in base (`light`, `dark`, …).
    #[serde(default)]
    pub theme: Option<String>,
    #[serde(default)]
    pub frame: Option<String>,
    #[serde(default)]
    pub fg: Option<String>,
    #[serde(default)]
    pub accent: Option<String>,
}

/// What a window starts with before anything is listed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BootTheme {
    /// The built-in base: the plugin theme's kind when one is restored.
    pub theme: Option<String>,
    pub plugin: Option<CachedPluginTheme>,
    /// `--boot-frame`, `--boot-fg`, `--boot-accent` with their hex colours, for those stored validly.
    pub colors: Vec<(&'static str, String)>,
}

/// Reads back the stored plugin theme and splash colours; anything malformed or able to load a
/// resource is ignored.
pub fn restore(plugin_theme: Option<&str>, startup: Option<&str>) -> BootTheme {
    let last: StartupTheme = startup.and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
    let plugin = plugin_theme
        .and_then(|text| serde_json::from_str::<CachedPluginTheme>(text).ok())
        .filter(|cached| cached.tokens.iter().all(|(name, value)| safe_token(name, value)));
    let theme = plugin.as_ref().map(|cached| cached.kind().as_str().to_owned()).or(last.theme);
    let colors = [("--boot-frame", last.frame), ("--boot-fg", last.fg), ("--boot-accent", last.accent)]
        .into_iter()
        .filter_map(|(name, value)| value.filter(|v| HEX.is_match(v)).map(|v| (name, v)))
        .collect();
    BootTheme { theme, plugin, colors }
}

/// A change of the stored plugin theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CacheWrite {
    /// Store this JSON under [`PLUGIN_THEME_CACHE_KEY`].
    Set(String),
    Remove,
}

/// What a listing changes.
#[derive(Clone, Debug, PartialEq)]
pub struct ThemeSync {
    /// The plugin theme to draw with (`None`: the built-in one).
    pub apply: Option<ActiveTheme>,
    pub cache: CacheWrite,
}

/// The selected plugin theme as the listings report it.
#[derive(Debug, Default)]
pub struct PluginThemeTracker {
    /// `<theme>|<frame>` last reported.
    reported_frame: Option<String>,
}

impl PluginThemeTracker {
    /// A listing state: `loaded` once one arrived, `error` when it failed. Before the first listing,
    /// and after a failed one without a theme, the restored theme stays (`None`).
    pub fn listed(&mut self, loaded: bool, error: bool, active: Option<&ActiveTheme>) -> Option<ThemeSync> {
        if active.is_none() && (!loaded || error) {
            return None;
        }
        let cache = match active {
            Some(theme) => CacheWrite::Set(serde_json::to_string(&CachedPluginTheme::of(theme)).unwrap_or_default()),
            None => {
                self.reported_frame = None;
                CacheWrite::Remove
            }
        };
        Some(ThemeSync { apply: active.cloned(), cache })
    }

    /// The frame colour to report for `active` (`set_theme_frame`), once per selection: the host
    /// forgets it when the selection changes.
    pub fn frame_to_report(&mut self, active: Option<&ActiveTheme>, frame: Option<&str>) -> Option<String> {
        let (Some(theme), Some(frame)) = (active, frame) else {
            self.reported_frame = None;
            return None;
        };
        let key = format!("{}/{}|{frame}", theme.plugin_id, theme.theme.id);
        if self.reported_frame.as_deref() == Some(key.as_str()) {
            return None;
        }
        self.reported_frame = Some(key);
        Some(frame.to_owned())
    }

    /// The report failed: the next one is sent again.
    pub fn frame_report_failed(&mut self) {
        self.reported_frame = None;
    }
}

/// The splash colours to store for the next start-up.
pub fn startup_theme(theme: ThemeKind, frame: Option<&str>, fg: Option<&str>, accent: Option<&str>) -> String {
    let stored = StartupTheme {
        theme: Some(theme.as_str().to_owned()),
        frame: frame.map(str::to_owned),
        fg: fg.map(str::to_owned),
        accent: accent.map(str::to_owned),
    };
    serde_json::to_string(&stored).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn one_dark() -> ActiveTheme {
        serde_json::from_value(json!({
            "pluginId": "theme-one",
            "theme": {
                "id": "one-dark",
                "name": "One Dark",
                "kind": "dark",
                "tokens": {
                    "--bg-frame": "hsl(220, 13%, 15%)",
                    "--fg-muted": "hsl(220, 9%, 55%)",
                    "--accent": "hsl(220, 100%, 66%)",
                },
            },
        }))
        .expect("a theme")
    }

    fn solarized_light() -> ActiveTheme {
        serde_json::from_value(json!({
            "pluginId": "theme-solarized",
            "theme": {
                "id": "solarized-light",
                "name": "Solarized Light",
                "kind": "light",
                "tokens": { "--bg-frame": "#eee8d5" },
            },
        }))
        .expect("a theme")
    }

    fn stored(sync: &ThemeSync) -> &str {
        match &sync.cache {
            CacheWrite::Set(json) => json,
            CacheWrite::Remove => panic!("removed"),
        }
    }

    #[test]
    fn restores_the_last_plugin_theme_and_splash_colours_before_the_app_loads() {
        let mut tracker = PluginThemeTracker::default();
        let sync = tracker.listed(true, false, Some(&one_dark())).expect("applied");
        let startup = startup_theme(ThemeKind::Dark, Some("#21252b"), Some("#828997"), Some("#5c8aff"));
        let boot = restore(Some(stored(&sync)), Some(&startup));
        let plugin = boot.plugin.expect("the plugin theme is restored");
        assert_eq!((plugin.id.as_str(), plugin.kind()), ("theme-one/one-dark", ThemeKind::Dark));
        assert_eq!(boot.theme.as_deref(), Some("dark"));
        assert_eq!(plugin.tokens.get("--bg-frame").map(String::as_str), Some("hsl(220, 13%, 15%)"));
        assert_eq!(
            boot.colors,
            vec![
                ("--boot-frame", "#21252b".to_owned()),
                ("--boot-fg", "#828997".to_owned()),
                ("--boot-accent", "#5c8aff".to_owned())
            ]
        );
    }

    #[test]
    fn keeps_the_restored_theme_until_the_plugin_list_arrives_then_follows_it() {
        let mut tracker = PluginThemeTracker::default();
        assert_eq!(tracker.listed(false, false, None), None, "nothing listed yet");
        assert_eq!(tracker.listed(true, true, None), None, "a failed listing doesn't drop it");
        let sync = tracker.listed(true, false, Some(&solarized_light())).expect("followed");
        assert_eq!(sync.apply.as_ref().map(|t| t.theme.id.as_str()), Some("solarized-light"));
        let cached: CachedPluginTheme = serde_json::from_str(stored(&sync)).expect("JSON");
        assert_eq!((cached.id.as_str(), cached.kind.as_str()), ("theme-solarized/solarized-light", "light"));
        let gone = tracker.listed(true, false, None).expect("followed");
        assert_eq!((gone.apply, gone.cache), (None, CacheWrite::Remove));
    }

    #[test]
    fn ignores_a_cached_stylesheet_that_could_load_resources() {
        let plugin = json!({ "id": "p/t", "kind": "dark", "tokens": { "--bg-app": "url(https://example.com/x)" } });
        let startup = json!({ "theme": "light", "frame": "red; color: blue" });
        let boot = restore(Some(&plugin.to_string()), Some(&startup.to_string()));
        assert_eq!(boot.plugin, None);
        assert_eq!(boot.theme.as_deref(), Some("light"));
        assert!(boot.colors.is_empty());
    }

    #[test]
    fn reports_the_resolved_frame_colour_once_per_selected_theme() {
        let mut tracker = PluginThemeTracker::default();
        let theme = one_dark();
        assert_eq!(tracker.frame_to_report(Some(&theme), Some("#21252b")).as_deref(), Some("#21252b"));
        assert_eq!(tracker.frame_to_report(Some(&theme), Some("#21252b")), None);
        tracker.listed(true, false, None);
        assert_eq!(tracker.frame_to_report(None, None), None);
        assert_eq!(tracker.frame_to_report(Some(&theme), Some("#21252b")).as_deref(), Some("#21252b"));
    }
}
