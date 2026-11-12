//! Theme / colour scheme / keymap / view mode switches shared by the main toolbar's gear menu and
//! View › Quick Switch Scheme… (⌃`).
//!
//! The menus are data: a [`MenuItem`] names what becoming active ([`Hover`]) and being chosen
//! ([`Command`]) do, and the menu host carries that out. [`ThemePreview`] is the instant theme
//! switch of Quick Switch; its delay is a deadline the host checks with [`ThemePreview::expire`].

use crate::platform::keymap::PRESET_TITLES;
use crate::platform::settings::SettingsWrite;
use serde_json::Value;

/// How long a theme stays under the pointer or keyboard before Quick Switch shows it (IDEA's delay).
pub const THEME_PREVIEW_DELAY: f64 = 500.0;

/// The global settings key of the keymap in effect.
pub const KEYMAP_KEY: &str = "keymap";
/// The global settings key of the editor colour scheme.
pub const COLOR_SCHEME_KEY: &str = "editor.colorScheme";
/// The colour scheme that follows the theme.
pub const THEME_DEFAULT_SCHEME: &str = "theme-default";

fn set_global(key: &'static str, value: &str) -> SettingsWrite {
    SettingsWrite { key, value: Value::String(value.to_owned()) }
}

/// The write that switches to keymap `id`.
pub fn select_keymap(id: &str) -> SettingsWrite {
    set_global(KEYMAP_KEY, id)
}

/// What the theme has to do for a [`ThemePreview`] transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThemeEffect {
    /// Show this theme in this window without selecting it; `None` shows the selected theme again.
    /// Nothing is saved and other windows keep theirs.
    Preview(Option<String>),
    /// Select the theme (it is saved, for every window).
    Select(String),
}

/// IDEA's instant theme switch (`ide.instant.theme.switch`): in Quick Switch the active theme
/// shows after a moment, in this window only; choosing one selects it, and closing the menu
/// without a choice brings the selected theme back.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemePreview {
    /// The theme waiting to be shown, and when it is due.
    pending: Option<(f64, String)>,
    shown: Option<String>,
    chosen: bool,
}

impl ThemePreview {
    /// The theme became the active item: it shows once it stayed active for
    /// [`THEME_PREVIEW_DELAY`]. Moving on restarts the delay.
    pub fn preview(&mut self, id: &str, now_ms: f64) {
        self.pending = Some((now_ms + THEME_PREVIEW_DELAY, id.to_owned()));
    }

    /// When [`Self::expire`] has something to do.
    pub fn deadline(&self) -> Option<f64> {
        self.pending.as_ref().map(|(due, _)| *due)
    }

    /// Shows the waiting theme once its delay has passed.
    pub fn expire(&mut self, now_ms: f64) -> Option<ThemeEffect> {
        if self.deadline()? > now_ms {
            return None;
        }
        let (_, id) = self.pending.take()?;
        self.shown = Some(id.clone());
        Some(ThemeEffect::Preview(Some(id)))
    }

    /// Back to the selected theme (another Quick Switch entry became active).
    pub fn restore(&mut self) -> Option<ThemeEffect> {
        self.pending = None;
        let previewed = self.shown.take().is_some();
        (previewed && !self.chosen).then_some(ThemeEffect::Preview(None))
    }

    /// The theme was chosen: the effects to carry out, in order.
    pub fn choose(&mut self, id: &str) -> Vec<ThemeEffect> {
        self.pending = None;
        self.chosen = true;
        let mut effects = Vec::new();
        // Shown right away; selecting it then changes nothing visible.
        if self.shown.as_deref() != Some(id) {
            effects.push(ThemeEffect::Preview(Some(id.to_owned())));
        }
        effects.push(ThemeEffect::Select(id.to_owned()));
        effects
    }

    /// The menu closed. A chosen item runs right after its menu closes: let it choose first.
    pub fn end(&mut self) -> Option<ThemeEffect> {
        self.restore()
    }

    /// A menu item became the active one.
    pub fn activate(&mut self, hover: &Hover, now_ms: f64) -> Option<ThemeEffect> {
        match hover {
            Hover::Preview(id) => {
                self.preview(id, now_ms);
                None
            }
            Hover::Restore => self.restore(),
        }
    }
}

/// What a menu item does when the pointer or the keyboard makes it the active item (not on
/// opening the menu).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hover {
    /// [`ThemePreview::preview`] this theme.
    Preview(String),
    /// [`ThemePreview::restore`].
    Restore,
}

/// What choosing a menu item does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Select the theme (no preview is running).
    SetTheme(String),
    /// [`ThemePreview::choose`] the theme.
    ChooseTheme(String),
    /// Write a global setting.
    SetGlobal(SettingsWrite),
}

/// An entry of a Quick Switch menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    Separator,
    /// A registered action, shown with its own title, shortcut and state.
    Action(&'static str),
    Item(MenuEntry),
}

/// A menu row with its own label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuEntry {
    pub id: String,
    pub label: String,
    /// Text from outside the IDE (a plugin's theme, a user's keymap): shown as written.
    pub verbatim: bool,
    /// A checkmark row, checked or not; `None` for a plain row.
    pub checked: Option<bool>,
    pub on_active: Option<Hover>,
    pub run: Option<Command>,
    pub submenu: Vec<MenuItem>,
}

impl MenuItem {
    /// The row, unless this is a separator or an action.
    pub fn entry(&self) -> Option<&MenuEntry> {
        match self {
            Self::Item(entry) => Some(entry),
            Self::Separator | Self::Action(_) => None,
        }
    }
}

/// A theme that can be selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThemeChoice {
    /// A built-in theme id, or a plugin's theme: `plugin:<plugin-id>/<theme-id>`.
    pub id: String,
    pub label: String,
    /// Who contributes it (a plugin's name); built-in themes have none.
    pub origin: Option<String>,
}

/// The themes of each group (the built-in ones, then each plugin's), a separator between the
/// groups (as IDEA's Theme list). `theme_id` is the theme in effect. With `preview`, the items
/// drive a [`ThemePreview`].
pub fn theme_items(groups: &[Vec<ThemeChoice>], theme_id: &str, preview: bool) -> Vec<MenuItem> {
    let item = |choice: &ThemeChoice| {
        MenuItem::Item(MenuEntry {
            id: format!("theme.{}", choice.id),
            label: choice.label.clone(),
            // Plugin themes keep their own names.
            verbatim: choice.origin.as_deref().is_some_and(|origin| !origin.is_empty()),
            checked: Some(choice.id == theme_id),
            on_active: preview.then(|| Hover::Preview(choice.id.clone())),
            run: Some(if preview {
                Command::ChooseTheme(choice.id.clone())
            } else {
                Command::SetTheme(choice.id.clone())
            }),
            submenu: Vec::new(),
        })
    };
    groups
        .iter()
        .enumerate()
        .flat_map(|(index, group)| {
            (index > 0).then_some(MenuItem::Separator).into_iter().chain(group.iter().map(&item))
        })
        .collect()
}

/// The preset keymaps, then the user's own; `keymap_id` is the one in effect.
pub fn keymap_items(keymap_id: &str, custom_keymaps: &[String]) -> Vec<MenuItem> {
    let preset_title = |id: &str| PRESET_TITLES.iter().find(|(preset, _)| *preset == id).map(|(_, title)| *title);
    PRESET_TITLES
        .iter()
        .map(|(id, _)| *id)
        .chain(custom_keymaps.iter().map(String::as_str))
        .map(|id| {
            let title = preset_title(id);
            MenuItem::Item(MenuEntry {
                id: format!("keymap.{id}"),
                label: title.unwrap_or(id).to_owned(),
                verbatim: title.is_none(),
                checked: Some(id == keymap_id),
                run: Some(Command::SetGlobal(select_keymap(id))),
                ..MenuEntry::default()
            })
        })
        .collect()
}

/// An editor colour scheme that can be selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ColorScheme {
    pub id: String,
    pub name: String,
}

/// "Theme default", then `schemes` (the built-in and the user's own); `current` is the stored
/// scheme, taken as "Theme default" when it names none of them.
pub fn color_scheme_items(current: Option<&str>, schemes: &[ColorScheme]) -> Vec<MenuItem> {
    let value = current.filter(|id| schemes.iter().any(|scheme| scheme.id == *id)).unwrap_or(THEME_DEFAULT_SCHEME);
    let item = |id: &str, label: &str| {
        MenuItem::Item(MenuEntry {
            id: format!("scheme.{id}"),
            label: label.to_owned(),
            checked: Some(id == value),
            run: Some(Command::SetGlobal(set_global(COLOR_SCHEME_KEY, id))),
            ..MenuEntry::default()
        })
    };
    std::iter::once(item(THEME_DEFAULT_SCHEME, "Theme default"))
        .chain(schemes.iter().map(|scheme| item(&scheme.id, &scheme.name)))
        .collect()
}

/// The existing view modes: Distraction Free Mode and Full Screen.
pub const VIEW_MODE_ITEMS: [MenuItem; 2] =
    [MenuItem::Action("editor.toggleZen"), MenuItem::Action("window.toggleFullScreen")];

/// The keymap in effect and the user's custom keymaps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeymapState {
    pub keymap_id: String,
    pub customs: Vec<String>,
}

/// Reads [`KeymapState`] from the effective settings (when loaded); `preset` is the keymap in
/// effect while the settings name none.
pub fn keymap_state(settings: Option<&Value>, preset: &str) -> KeymapState {
    let customs: Vec<String> = match settings.and_then(|settings| settings.get("customKeymaps")) {
        Some(Value::Object(keymaps)) => keymaps.keys().cloned().collect(),
        Some(Value::Array(keymaps)) => (0..keymaps.len()).map(|index| index.to_string()).collect(),
        _ => Vec::new(),
    };
    let keymap_id = settings.and_then(|settings| settings.get(KEYMAP_KEY)).and_then(Value::as_str).unwrap_or(preset);
    KeymapState { keymap_id: keymap_id.to_owned(), customs }
}

/// What the Quick Switch menu is built from.
#[derive(Debug, Clone, Copy)]
pub struct QuickSwitchInput<'a> {
    /// The stored editor colour scheme (`editor.colorScheme`).
    pub color_scheme: Option<&'a str>,
    pub schemes: &'a [ColorScheme],
    pub keymap: &'a KeymapState,
    pub theme_groups: &'a [Vec<ThemeChoice>],
    /// The theme in effect.
    pub theme_id: &'a str,
}

/// View › Quick Switch Scheme…: IDEA's popup of the switchable schemes. With `preview`, its
/// themes are previewed through a [`ThemePreview`], which the host ends when the menu closes.
pub fn quick_switch_items(input: &QuickSwitchInput<'_>, preview: bool) -> Vec<MenuItem> {
    let restore = preview.then_some(Hover::Restore);
    let entry = |id: &str, label: &str, on_active: Option<Hover>, submenu: Vec<MenuItem>| {
        MenuItem::Item(MenuEntry {
            id: id.to_owned(),
            label: label.to_owned(),
            on_active,
            submenu,
            ..MenuEntry::default()
        })
    };
    vec![
        entry(
            "qs.scheme",
            "Editor Color Scheme",
            restore.clone(),
            color_scheme_items(input.color_scheme, input.schemes),
        ),
        entry("qs.keymap", "Keymap", restore.clone(), keymap_items(&input.keymap.keymap_id, &input.keymap.customs)),
        entry("qs.viewMode", "View Mode", restore, VIEW_MODE_ITEMS.to_vec()),
        entry("qs.theme", "Theme", None, theme_items(input.theme_groups, input.theme_id, preview)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn builtin_themes() -> Vec<Vec<ThemeChoice>> {
        let themes = [
            ("light", "Islands Light"),
            ("dark", "Islands Dark"),
            ("dark-hc", "High Contrast"),
            ("solarized-light", "Solarized Light"),
            ("solarized-dark", "Solarized Dark"),
            ("one-light", "One Light"),
            ("one-dark", "One Dark"),
        ];
        vec![
            themes
                .iter()
                .map(|(id, label)| ThemeChoice { id: (*id).to_owned(), label: (*label).to_owned(), origin: None })
                .collect(),
        ]
    }

    /// The Quick Switch menu of a window showing Islands Light, with the theme preview on.
    fn quick_switch() -> Vec<MenuItem> {
        let groups = builtin_themes();
        let schemes: Vec<ColorScheme> =
            groups[0].iter().map(|theme| ColorScheme { id: theme.id.clone(), name: theme.label.clone() }).collect();
        let keymap = keymap_state(None, "intellij-macos");
        quick_switch_items(
            &QuickSwitchInput {
                color_scheme: None,
                schemes: &schemes,
                keymap: &keymap,
                theme_groups: &groups,
                theme_id: "light",
            },
            true,
        )
    }

    fn entry<'a>(items: &'a [MenuItem], label: &str) -> &'a MenuEntry {
        items.iter().filter_map(MenuItem::entry).find(|entry| entry.label == label).unwrap()
    }

    /// Runs a chosen item the way the menu host does.
    fn run(preview: &mut ThemePreview, entry: &MenuEntry) -> Vec<ThemeEffect> {
        match entry.run.as_ref().unwrap() {
            Command::ChooseTheme(id) => preview.choose(id),
            Command::SetTheme(id) => vec![ThemeEffect::Select(id.clone())],
            Command::SetGlobal(_) => Vec::new(),
        }
    }

    fn preview_of(id: &str) -> Option<ThemeEffect> {
        Some(ThemeEffect::Preview(Some(id.to_owned())))
    }

    // src/workbench/quickSwitch.test.tsx › Quick Switch theme preview › shows the active theme after a
    // moment, in this window only, and takes it back
    #[test]
    fn shows_the_active_theme_after_a_moment_and_takes_it_back() {
        let mut preview = ThemePreview::default();
        let mut now = 0.0;
        preview.preview("solarized-dark", now);
        now += THEME_PREVIEW_DELAY - 1.0;
        assert_eq!(preview.expire(now), None);
        // Moving on restarts the delay: only the theme the user stays on shows.
        preview.preview("one-light", now);
        now += THEME_PREVIEW_DELAY - 1.0;
        assert_eq!(preview.expire(now), None);
        now += 1.0;
        // Shown, not selected: no `Select` effect, so nothing is saved.
        assert_eq!(preview.expire(now), preview_of("one-light"));
        assert_eq!(preview.expire(now), None);

        assert_eq!(preview.restore(), Some(ThemeEffect::Preview(None)));
        assert_eq!(preview.restore(), None);
        preview.preview("solarized-dark", now);
        now += THEME_PREVIEW_DELAY;
        assert_eq!(preview.expire(now), preview_of("solarized-dark"));
        assert_eq!(preview.end(), Some(ThemeEffect::Preview(None)));
    }

    // src/workbench/quickSwitch.test.tsx › Quick Switch theme preview › selects the chosen theme and keeps
    // it when the menu closes
    #[test]
    fn selects_the_chosen_theme_and_keeps_it_when_the_menu_closes() {
        let mut preview = ThemePreview::default();
        preview.preview("solarized-dark", 0.0);
        assert_eq!(preview.expire(THEME_PREVIEW_DELAY), preview_of("solarized-dark"));
        assert_eq!(
            preview.choose("one-dark"),
            [ThemeEffect::Preview(Some("one-dark".to_owned())), ThemeEffect::Select("one-dark".to_owned())]
        );
        assert_eq!(preview.end(), None);
    }

    // src/workbench/quickSwitch.test.tsx › Quick Switch theme preview › previews from the Theme submenu
    // and takes the preview back when the menu closes
    #[test]
    fn previews_from_the_theme_submenu_and_takes_the_preview_back_when_the_menu_closes() {
        let menu = quick_switch();
        let mut preview = ThemePreview::default();
        let theme = entry(&menu, "Theme");
        let solarized_light = entry(&theme.submenu, "Solarized Light");
        assert_eq!(solarized_light.checked, Some(false));
        let hover = |preview: &mut ThemePreview, entry: &MenuEntry, now: f64| {
            entry.on_active.as_ref().and_then(|hover| preview.activate(hover, now))
        };

        let mut now = 200.0;
        assert_eq!(hover(&mut preview, theme, 0.0), None);
        assert_eq!(hover(&mut preview, solarized_light, now), None);
        now += THEME_PREVIEW_DELAY;
        assert_eq!(preview.expire(now), preview_of("solarized-light"));

        // Back on another entry: the selected theme again.
        assert_eq!(hover(&mut preview, entry(&menu, "Keymap"), now), Some(ThemeEffect::Preview(None)));

        assert_eq!(hover(&mut preview, theme, now), None);
        assert_eq!(hover(&mut preview, solarized_light, now), None);
        now += THEME_PREVIEW_DELAY;
        assert_eq!(preview.expire(now), preview_of("solarized-light"));
        // The menu closes without a choice: the preview goes, and nothing was selected.
        assert_eq!(preview.end(), Some(ThemeEffect::Preview(None)));
    }

    // src/workbench/quickSwitch.test.tsx › Quick Switch theme preview › selects a theme clicked in the
    // Theme submenu
    #[test]
    fn selects_a_theme_clicked_in_the_theme_submenu() {
        let menu = quick_switch();
        let mut preview = ThemePreview::default();
        let solarized_dark = entry(&entry(&menu, "Theme").submenu, "Solarized Dark");
        assert_eq!(
            run(&mut preview, solarized_dark),
            [ThemeEffect::Preview(Some("solarized-dark".to_owned())), ThemeEffect::Select("solarized-dark".to_owned())]
        );
        assert_eq!(preview.end(), None);
    }

    #[test]
    fn lists_the_switchable_schemes_with_the_current_ones_checked() {
        let plugin = ThemeChoice {
            id: "plugin:acme/night".to_owned(),
            label: "Acme Night".to_owned(),
            origin: Some("Acme".to_owned()),
        };
        let mut groups = builtin_themes();
        groups.push(vec![plugin]);
        let themes = theme_items(&groups, "plugin:acme/night", false);
        assert_eq!(themes.len(), 9);
        assert_eq!(themes[7], MenuItem::Separator);
        let night = entry(&themes, "Acme Night");
        assert_eq!((night.id.as_str(), night.verbatim, night.checked), ("theme.plugin:acme/night", true, Some(true)));
        assert_eq!(night.on_active, None);
        assert_eq!(run(&mut ThemePreview::default(), night), [ThemeEffect::Select("plugin:acme/night".to_owned())]);
        assert!(!entry(&themes, "Islands Light").verbatim);

        let settings = json!({ "keymap": "mine", "customKeymaps": { "mine": {} } });
        let state = keymap_state(Some(&settings), "intellij-macos");
        assert_eq!(state, KeymapState { keymap_id: "mine".to_owned(), customs: vec!["mine".to_owned()] });
        assert_eq!(keymap_state(None, "vscode-macos").keymap_id, "vscode-macos");
        let keymaps = keymap_items(&state.keymap_id, &state.customs);
        assert_eq!(keymaps.len(), PRESET_TITLES.len() + 1);
        let mine = entry(&keymaps, "mine");
        assert_eq!((mine.id.as_str(), mine.verbatim, mine.checked), ("keymap.mine", true, Some(true)));
        assert_eq!(mine.run, Some(Command::SetGlobal(SettingsWrite { key: "keymap", value: json!("mine") })));
        assert_eq!(entry(&keymaps, "VS Code (macOS)").checked, Some(false));

        let schemes = [ColorScheme { id: "dark".to_owned(), name: "Islands Dark".to_owned() }];
        let stale = color_scheme_items(Some("custom-gone"), &schemes);
        assert_eq!(entry(&stale, "Theme default").checked, Some(true));
        let dark = color_scheme_items(Some("dark"), &schemes);
        assert_eq!(entry(&dark, "Theme default").checked, Some(false));
        assert_eq!(
            entry(&dark, "Islands Dark").run,
            Some(Command::SetGlobal(SettingsWrite { key: "editor.colorScheme", value: json!("dark") }))
        );
    }
}
