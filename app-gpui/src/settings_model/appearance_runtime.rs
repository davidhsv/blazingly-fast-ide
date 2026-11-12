//! The runtime side of Settings › Appearance that no other feature owns: what the options turn
//! into for the window (compact mode, contrast scrollbars, the red-green palette, antialiasing,
//! menu icons, the UI font), the IDE zoom with its actions, and the View › Appearance toggles.

use crate::platform::settings::SettingsWrite;
use crate::theme::fonts::ui_font_family;
use ide_core::settings::AppearanceSettings;
use serde_json::json;

/// The per-level indent trees use when "Use smaller indents in trees" is on (IDEA parity).
pub const SMALL_TREE_INDENT: &str = "12px";
/// The UI font size of the theme, which a custom font replaces.
pub const DEFAULT_UI_FONT_SIZE: f32 = 13.0;
pub const ZOOM_MIN: u32 = 50;
pub const ZOOM_MAX: u32 = 300;

/// What the Appearance options ask of a window.
#[derive(Debug, Clone, PartialEq)]
pub struct AppearanceOptions {
    /// The sizes of UI, small and tiny text with "Use custom font"; `None` keeps the theme's.
    pub font_sizes: Option<[f32; 3]>,
    /// The UI font stack with "Use custom font"; `None` keeps the theme's.
    pub font_ui: Option<String>,
    pub compact: bool,
    pub contrast_scrollbars: bool,
    pub red_green: bool,
    pub different_tool_window_background: bool,
    /// Project colours are left out of the toolbar.
    pub project_colors_off: bool,
    pub menu_icons_off: bool,
    /// The per-level indent of trees; `None` keeps the default.
    pub tree_indent: Option<&'static str>,
    pub ide_antialiasing: String,
    pub editor_antialiasing: String,
    /// The IDE zoom in percent.
    pub zoom: u32,
}

/// The window attributes and variables of the Appearance options.
pub fn appearance_options(appearance: &AppearanceSettings) -> AppearanceOptions {
    let custom = appearance.use_custom_font;
    let size = if custom { appearance.ui_font_size } else { DEFAULT_UI_FONT_SIZE };
    AppearanceOptions {
        font_sizes: (size != DEFAULT_UI_FONT_SIZE).then_some([size, size - 1.0, size - 2.0]),
        font_ui: custom.then(|| ui_font_family(&appearance.ui_font_family, &appearance.ui_font_fallback)),
        compact: appearance.compact_mode,
        contrast_scrollbars: appearance.contrast_scrollbars,
        red_green: appearance.red_green_vision,
        different_tool_window_background: appearance.different_tool_window_background,
        project_colors_off: !appearance.project_colors_in_toolbar,
        menu_icons_off: !appearance.menu_icons,
        tree_indent: appearance.small_tree_indents.then_some(SMALL_TREE_INDENT),
        ide_antialiasing: appearance.ide_antialiasing.clone(),
        editor_antialiasing: appearance.editor_antialiasing.clone(),
        zoom: appearance.zoom.clamp(ZOOM_MIN, ZOOM_MAX),
    }
}

// ---------------------------------------------------------------------------
// Zoom actions (IDEA: View › Appearance › Zoom IDE)

const ZOOM_STEPS: [u32; 13] = [50, 67, 75, 80, 90, 100, 110, 125, 150, 175, 200, 250, 300];

/// The next zoom step above (`up`) or below `current`; `current` itself at either end.
pub fn next_zoom(current: u32, up: bool) -> u32 {
    let next = if up {
        ZOOM_STEPS.iter().copied().find(|&step| step > current)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|&step| step < current)
    };
    next.unwrap_or(current).clamp(ZOOM_MIN, ZOOM_MAX)
}

/// A zoom action of the View menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomAction {
    In,
    Out,
    Reset,
}

impl ZoomAction {
    pub const ALL: [Self; 3] = [Self::In, Self::Out, Self::Reset];

    /// The action's id and title.
    pub fn action(self) -> (&'static str, &'static str) {
        match self {
            Self::In => ("view.zoomIn", "Zoom In IDE"),
            Self::Out => ("view.zoomOut", "Zoom Out IDE"),
            Self::Reset => ("view.resetZoom", "Reset IDE Zoom"),
        }
    }

    /// The global settings write of the action, at the zoom `current`.
    pub fn write(self, current: u32) -> SettingsWrite {
        let percent = match self {
            Self::In => next_zoom(current, true),
            Self::Out => next_zoom(current, false),
            Self::Reset => 100,
        };
        SettingsWrite { key: "appearance.zoom", value: json!(percent) }
    }
}

// ---------------------------------------------------------------------------
// View › Appearance toggles over boolean appearance settings (IDEA's ToggleActions)

/// A View › Appearance toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppearanceToggle {
    CompactMode,
    Toolbar,
    ToolWindowBars,
    StatusBar,
}

impl AppearanceToggle {
    pub const ALL: [Self; 4] = [Self::CompactMode, Self::Toolbar, Self::ToolWindowBars, Self::StatusBar];

    /// The action's id.
    pub fn id(self) -> &'static str {
        match self {
            Self::CompactMode => "view.toggleCompactMode",
            Self::Toolbar => "view.toggleToolbar",
            Self::ToolWindowBars => "view.toggleToolWindowBars",
            Self::StatusBar => "view.toggleStatusBar",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::CompactMode => "Compact Mode",
            Self::Toolbar => "Toolbar",
            Self::ToolWindowBars => "Tool Window Bars",
            Self::StatusBar => "Status Bar",
        }
    }

    /// Extra search terms of the action, after "appearance".
    pub fn keywords(self) -> &'static [&'static str] {
        match self {
            Self::CompactMode => &["density", "smaller"],
            Self::Toolbar => &["main toolbar", "header", "hide"],
            Self::ToolWindowBars => &["stripes", "tool window buttons", "hide"],
            Self::StatusBar => &["hide"],
        }
    }

    /// The setting the toggle flips.
    pub fn key(self) -> &'static str {
        match self {
            Self::CompactMode => "appearance.compactMode",
            Self::Toolbar => "appearance.showMainToolbar",
            Self::ToolWindowBars => "appearance.showToolWindowBars",
            Self::StatusBar => "appearance.showStatusBar",
        }
    }

    /// Whether the toggle is on.
    pub fn toggled(self, appearance: &AppearanceSettings) -> bool {
        match self {
            Self::CompactMode => appearance.compact_mode,
            Self::Toolbar => appearance.show_main_toolbar,
            Self::ToolWindowBars => appearance.show_tool_window_bars,
            Self::StatusBar => appearance.show_status_bar,
        }
    }

    /// The global settings write that flips the toggle.
    pub fn write(self, appearance: &AppearanceSettings) -> SettingsWrite {
        SettingsWrite { key: self.key(), value: json!(!self.toggled(appearance)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::{parse_settings, set_at, settings_value};
    use ide_core::settings::Settings;

    fn appearance(overrides: serde_json::Value) -> AppearanceSettings {
        parse_settings(&json!({ "appearance": overrides })).expect("the backend takes the value").appearance
    }

    // appearance & behavior settings › steps zoom within 50–300%
    #[test]
    fn steps_zoom_within_50_to_300_percent() {
        assert_eq!(next_zoom(100, true), 110);
        assert_eq!(next_zoom(100, false), 90);
        assert_eq!(next_zoom(300, true), 300);
        assert_eq!(next_zoom(50, false), 50);
    }

    // appearance & behavior settings › defaults Use smaller indents in trees to off and applies the indent variable
    #[test]
    fn defaults_use_smaller_indents_in_trees_to_off_and_applies_the_indent_variable() {
        assert!(!appearance(json!({})).small_tree_indents);
        assert!(appearance(json!({ "smallTreeIndents": true })).small_tree_indents);
        assert!(parse_settings(&json!({ "appearance": { "smallTreeIndents": "on" } })).is_err());
        let small = appearance_options(&appearance(json!({ "smallTreeIndents": true })));
        assert_eq!(small.tree_indent, Some(SMALL_TREE_INDENT));
        // Off restores the default indents.
        assert_eq!(appearance_options(&appearance(json!({}))).tree_indent, None);
        assert_eq!(appearance_options(&appearance(json!({ "smallTreeIndents": false }))).tree_indent, None);
    }

    // appearance & behavior settings › applies and clears the UI fallback, and removes custom fonts when disabled
    #[test]
    fn applies_and_clears_the_ui_fallback_and_removes_custom_fonts_when_disabled() {
        let mut custom =
            appearance(json!({ "useCustomFont": true, "uiFontFamily": "Menlo", "uiFontFallback": "Songti SC" }));
        let font = |appearance: &AppearanceSettings| appearance_options(appearance).font_ui;
        assert_eq!(font(&custom).as_deref(), Some("\"Menlo\", \"Songti SC\", system-ui, sans-serif"));
        custom.ui_font_fallback.clear();
        assert_eq!(font(&custom).as_deref(), Some("\"Menlo\", system-ui, sans-serif"));
        custom.use_custom_font = false;
        assert_eq!(font(&custom), None);
    }

    // View › Appearance toggles › %s flips appearance.%s in the global settings
    #[test]
    fn each_toggle_flips_its_appearance_setting_in_the_global_settings() {
        let cases = [
            ("view.toggleCompactMode", "compactMode", false),
            ("view.toggleToolbar", "showMainToolbar", true),
            ("view.toggleToolWindowBars", "showToolWindowBars", true),
            ("view.toggleStatusBar", "showStatusBar", true),
        ];
        for (id, key, initial) in cases {
            let toggle = AppearanceToggle::ALL.into_iter().find(|toggle| toggle.id() == id).expect("the toggle");
            let settings = Settings::default();
            assert_eq!(toggle.toggled(&settings.appearance), initial, "{id}");
            let write = toggle.write(&settings.appearance);
            assert_eq!(write.key, format!("appearance.{key}"));
            assert_eq!(write.value.to_string(), (!initial).to_string());
            // The write applied to the settings flips what the toggle shows.
            let written = set_at(&settings_value(&settings), write.key, write.value);
            let after = parse_settings(&written).expect("the backend takes the write");
            assert_eq!(toggle.toggled(&after.appearance), !initial, "{id}");
        }
    }

    // Not from a spec: a custom font also sizes the text, the zoom actions write the next step,
    // and the remaining options map one to one.
    #[test]
    fn custom_fonts_size_the_text_and_zoom_actions_write_the_next_step() {
        let defaults = appearance_options(&AppearanceSettings::default());
        assert_eq!((defaults.font_sizes, defaults.font_ui.as_deref(), defaults.zoom), (None, None, 100));
        assert!(!defaults.project_colors_off && !defaults.menu_icons_off && !defaults.compact);
        let custom = appearance(json!({ "useCustomFont": true, "uiFontSize": 15, "menuIcons": false, "zoom": 1000 }));
        let options = appearance_options(&custom);
        assert_eq!(options.font_sizes, Some([15.0, 14.0, 13.0]));
        assert!(options.menu_icons_off);
        assert_eq!(options.zoom, ZOOM_MAX);
        assert_eq!(ZoomAction::In.write(100), SettingsWrite { key: "appearance.zoom", value: json!(110) });
        assert_eq!(ZoomAction::Out.write(110).value, json!(100));
        assert_eq!(ZoomAction::Reset.write(250).value, json!(100));
        assert_eq!(next_zoom(101, true), 110);
        assert_eq!(ZoomAction::ALL.map(|action| action.action().0), ["view.zoomIn", "view.zoomOut", "view.resetZoom"]);
        assert_eq!(AppearanceToggle::Toolbar.keywords().first(), Some(&"main toolbar"));
        assert_eq!(AppearanceToggle::StatusBar.title(), "Status Bar");
    }
}
