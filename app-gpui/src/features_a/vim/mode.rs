//! Vim mode on and off across the app, and the mode the status bar widget shows.
//!
//! [`VimMode`] turns the setting into effects for the caller: context keys, the stored setting,
//! loading the engine (with the rc file) and installing or removing the Vim editor contribution.
//! The engine loads asynchronously; an answer for an older toggle is dropped.

use super::settings::{VIM_SETTINGS_KEY, VimSettings, parse_vim_settings, vim_settings_json};
use crate::platform::context::ContextValue;
use crate::platform::i18n::I18n;

/// Visual sub-mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisualSub {
    Linewise,
    Blockwise,
}

/// The mode of the focused (or last focused) Vim editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VimModeState {
    /// `normal`, `insert`, `visual`, `replace`.
    pub mode: String,
    pub sub: Option<VisualSub>,
}

impl Default for VimModeState {
    fn default() -> Self {
        Self { mode: "normal".to_owned(), sub: None }
    }
}

impl VimModeState {
    /// The state a mode change reports.
    pub fn changed(mode: &str, sub_mode: Option<&str>) -> Self {
        let sub = match sub_mode {
            Some("linewise") => Some(VisualSub::Linewise),
            Some("blockwise") => Some(VisualSub::Blockwise),
            _ => None,
        };
        Self { mode: mode.to_owned(), sub }
    }

    /// Whether typed text goes into the document (Insert or Replace).
    pub fn inserting(&self) -> bool {
        self.mode == "insert" || self.mode == "replace"
    }

    /// The widget text: NORMAL, INSERT, VISUAL, V-LINE, V-BLOCK, REPLACE.
    pub fn label(&self) -> String {
        if self.mode == "visual" {
            return match self.sub {
                Some(VisualSub::Linewise) => "V-LINE",
                Some(VisualSub::Blockwise) => "V-BLOCK",
                None => "VISUAL",
            }
            .to_owned();
        }
        if self.mode.is_empty() { "NORMAL".to_owned() } else { self.mode.to_uppercase() }
    }
}

/// A key press, as far as Escape handling cares.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Escape goes to the editor's own Escape bindings first (an open popup, lookup, find bar or
/// template closes); Vim only gets an Escape nothing else wanted. In Visual mode Escape always
/// leaves Visual mode. `key` is the key as GPUI names it (`escape`).
pub fn escape_to_editor_first(key: &str, mods: Modifiers, visual_mode: bool) -> bool {
    key == "escape" && mods == Modifiers::default() && !visual_mode
}

/// What the caller does for Vim mode.
#[derive(Clone, Debug, PartialEq)]
pub enum VimEffect {
    SetContext(&'static str, ContextValue),
    /// Store the settings under [`VIM_SETTINGS_KEY`].
    Store(String),
    /// Load the engine (and, when `rc` is set, read and apply the rc file quietly), then call
    /// [`VimMode::engine_ready`] with this generation.
    LoadEngine {
        generation: u64,
        rc: bool,
    },
    InstallContribution,
    RemoveContribution,
}

/// Vim mode across the app.
#[derive(Clone, Debug, Default)]
pub struct VimMode {
    settings: VimSettings,
    generation: u64,
    rc_applied: bool,
    contribution: bool,
}

impl VimMode {
    /// Starts with the stored settings and applies them.
    pub fn new(stored: Option<&str>) -> (Self, Vec<VimEffect>) {
        let mut vim = Self { settings: parse_vim_settings(stored), ..Self::default() };
        let effects = vim.apply();
        (vim, effects)
    }

    pub fn settings(&self) -> &VimSettings {
        &self.settings
    }

    fn apply(&mut self) -> Vec<VimEffect> {
        self.generation += 1;
        let enabled = self.settings.enabled;
        let mut effects = vec![VimEffect::SetContext("vimMode", enabled.into())];
        if !enabled {
            effects.push(VimEffect::SetContext("vim.mode", "".into()));
            if std::mem::take(&mut self.contribution) {
                effects.push(VimEffect::RemoveContribution);
            }
            return effects;
        }
        effects.push(VimEffect::LoadEngine { generation: self.generation, rc: !self.rc_applied });
        effects
    }

    /// Settings › Editor › Vim or Tools › Vim changed the settings here.
    pub fn update(&mut self, settings: VimSettings) -> Vec<VimEffect> {
        let toggled = settings.enabled != self.settings.enabled;
        self.settings = settings;
        let mut effects = vec![VimEffect::Store(vim_settings_json(&self.settings))];
        if toggled {
            effects.extend(self.apply());
        }
        effects
    }

    /// Tools › Vim.
    pub fn toggle(&mut self) -> Vec<VimEffect> {
        let settings = VimSettings { enabled: !self.settings.enabled, ..self.settings.clone() };
        self.update(settings)
    }

    /// Another window stored settings: `raw` is the stored value of `key`.
    pub fn stored_elsewhere(&mut self, key: &str, raw: Option<&str>) -> Vec<VimEffect> {
        if key != VIM_SETTINGS_KEY {
            return Vec::new();
        }
        let settings = parse_vim_settings(raw);
        let toggled = settings.enabled != self.settings.enabled;
        self.settings = settings;
        if toggled { self.apply() } else { Vec::new() }
    }

    /// The engine (and the rc file it was asked for) is loaded for `generation`.
    pub fn engine_ready(&mut self, generation: u64, rc_applied: bool) -> Vec<VimEffect> {
        self.rc_applied |= rc_applied;
        if generation != self.generation || self.contribution || !self.settings.enabled {
            return Vec::new();
        }
        self.contribution = true;
        vec![VimEffect::InstallContribution]
    }

    /// Reload .ideavimrc: the status text once it is applied (`commands` read from it).
    pub fn rc_reloaded(&mut self, commands: usize, i18n: &I18n) -> String {
        self.rc_applied = true;
        i18n.tx(if commands > 0 { "Vim: rc file reloaded" } else { "Vim: no ~/.openideavimrc or ~/.ideavimrc" })
    }

    /// The editor mode changed in the focused Vim editor: the `vim.mode` context value.
    pub fn mode_context(state: &VimModeState) -> VimEffect {
        VimEffect::SetContext("vim.mode", state.mode.as_str().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(effects: &[VimEffect]) -> Option<VimSettings> {
        effects.iter().find_map(|e| match e {
            VimEffect::Store(raw) => Some(parse_vim_settings(Some(raw))),
            _ => None,
        })
    }

    #[test]
    fn vim_toggle_turns_vim_on_and_off_for_every_editor_live_and_persists_it() {
        let (mut vim, start) = VimMode::new(None);
        assert!(!start.contains(&VimEffect::InstallContribution));
        let on = vim.toggle();
        assert!(vim.settings().enabled);
        assert!(stored(&on).is_some_and(|s| s.enabled));
        assert!(on.contains(&VimEffect::SetContext("vimMode", true.into())));
        let load = on.iter().find(|e| matches!(e, VimEffect::LoadEngine { .. }));
        let Some(&VimEffect::LoadEngine { generation, rc }) = load else {
            panic!("the engine is loaded");
        };
        assert!(rc);
        assert_eq!(vim.engine_ready(generation, true), [VimEffect::InstallContribution]);
        let off = vim.toggle();
        assert!(!vim.settings().enabled);
        assert!(stored(&off).is_some_and(|s| !s.enabled));
        assert!(off.contains(&VimEffect::RemoveContribution));
        assert!(off.contains(&VimEffect::SetContext("vim.mode", "".into())));
        // An engine answer for the toggle before installs nothing.
        assert!(vim.engine_ready(generation, true).is_empty());
    }

    #[test]
    fn follows_the_setting_changed_from_another_window() {
        let (mut vim, _) = VimMode::new(None);
        let Some(VimEffect::LoadEngine { generation, .. }) =
            vim.update(VimSettings { enabled: true, ..VimSettings::default() }).pop()
        else {
            panic!("the engine is loaded");
        };
        assert_eq!(vim.engine_ready(generation, true), [VimEffect::InstallContribution]);
        let effects = vim.stored_elsewhere(VIM_SETTINGS_KEY, Some(r#"{"enabled":false,"handlers":{}}"#));
        assert!(!vim.settings().enabled);
        assert!(effects.contains(&VimEffect::RemoveContribution));
        assert!(vim.stored_elsewhere("other.key", Some(r#"{"enabled":true}"#)).is_empty());
    }

    #[test]
    fn publishes_the_mode_and_lets_the_editors_escape_bindings_go_first() {
        assert!(escape_to_editor_first("escape", Modifiers::default(), false));
        assert!(!escape_to_editor_first("escape", Modifiers::default(), true));
        assert!(!escape_to_editor_first("escape", Modifiers { shift: true, ..Modifiers::default() }, false));
        let visual = VimModeState::changed("visual", None);
        assert_eq!(VimMode::mode_context(&visual), VimEffect::SetContext("vim.mode", "visual".into()));
        assert!(VimModeState::changed("insert", None).inserting());
        assert!(VimModeState::changed("replace", None).inserting());
        assert!(!visual.inserting());
    }

    #[test]
    fn labels_the_modes_as_the_status_widget_shows_them() {
        assert_eq!(VimModeState::default().label(), "NORMAL");
        assert_eq!(VimModeState::changed("insert", None).label(), "INSERT");
        assert_eq!(VimModeState::changed("visual", Some("linewise")).label(), "V-LINE");
        assert_eq!(VimModeState::changed("visual", Some("blockwise")).label(), "V-BLOCK");
        assert_eq!(VimModeState::changed("visual", Some("characterwise")).label(), "VISUAL");
        assert_eq!(VimModeState::changed("", None).label(), "NORMAL");
    }

    #[test]
    fn reports_whether_the_rc_file_was_found_on_reload() {
        let (mut vim, _) = VimMode::new(None);
        let i18n = I18n::default();
        assert_eq!(vim.rc_reloaded(3, &i18n), "Vim: rc file reloaded");
        assert_eq!(vim.rc_reloaded(0, &i18n), "Vim: no ~/.openideavimrc or ~/.ideavimrc");
    }
}
