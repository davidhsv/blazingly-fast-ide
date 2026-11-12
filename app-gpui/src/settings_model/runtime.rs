//! Applying the settings no other feature owns: the keymap preset with its overrides, the
//! Appearance options and the document language. (Theme and accent are applied by the workbench,
//! editor options by the editor.)
//!
//! Settings written by any window reach every window as a settings-changed event, after which
//! the effective settings are read again: once for a burst of events, since one Apply writes
//! several keys.

use super::appearance_runtime::{AppearanceOptions, appearance_options};
use super::assign_shortcut::KeymapSettings;
use crate::platform::i18n::LOCALES;
use ide_core::settings::Settings;

/// How long after a settings-changed event the settings are read again (milliseconds).
pub const RELOAD_DELAY_MS: f64 = 30.0;

/// What a window applies of the settings.
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeSettings {
    /// The selected keymap with the overrides and user keymaps to apply it with.
    pub keymap: KeymapSettings,
    pub appearance: AppearanceOptions,
    /// The BCP 47 tag of the UI language.
    pub language_tag: &'static str,
    /// Tool window buttons and editor tabs are only dragged with Alt pressed.
    pub drag_with_alt_only: bool,
    /// Save the files after this many seconds without input; `None` when idle saving is off.
    pub save_after_idle_seconds: Option<u32>,
}

/// What `settings` ask a window to apply.
pub fn runtime_settings(settings: &Settings) -> RuntimeSettings {
    let locale = LOCALES.iter().find(|locale| locale.id == settings.appearance.locale).unwrap_or(&LOCALES[0]);
    RuntimeSettings {
        keymap: KeymapSettings {
            keymap: settings.keymap.clone(),
            keymap_overrides: settings.keymap_overrides.clone(),
            custom_keymaps: settings.custom_keymaps.clone(),
        },
        appearance: appearance_options(&settings.appearance),
        language_tag: locale.tag,
        drag_with_alt_only: settings.appearance.dnd_with_alt_only,
        save_after_idle_seconds: settings.general.save_on_idle.then_some(settings.general.idle_seconds),
    }
}

/// The pending re-read of the settings after settings-changed events.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SettingsReload {
    due_at: Option<f64>,
}

impl SettingsReload {
    /// A settings-changed event arrived at `now_ms`: the re-read is (re)scheduled. Returns when
    /// it is due.
    pub fn settings_changed(&mut self, now_ms: f64) -> f64 {
        let due = now_ms + RELOAD_DELAY_MS;
        self.due_at = Some(due);
        due
    }

    /// Whether the settings are to be read again at `now_ms`; true once per burst of events.
    pub fn take_due(&mut self, now_ms: f64) -> bool {
        let due = self.due_at.is_some_and(|due| now_ms >= due);
        if due {
            self.due_at = None;
        }
        due
    }

    /// The window is going away: nothing is read any more.
    pub fn cancel(&mut self) {
        self.due_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::{default_settings, parse_settings, set_at};
    use serde_json::json;

    // reloads independent font preferences after standalone settings-window Apply events
    #[test]
    fn reloads_independent_font_preferences_after_standalone_settings_window_apply_events() {
        let mut reload = SettingsReload::default();
        // One Apply of the Settings window: several keys, several events.
        reload.settings_changed(0.0);
        reload.settings_changed(0.0);
        let reads = [10.0, 40.0, 80.0].into_iter().filter(|&now| reload.take_due(now)).count();
        assert_eq!(reads, 1);
        // What the read brings is applied: each font preference on its own.
        let mut next = set_at(&default_settings(), "editor.fontFamily", json!("Maple Mono NF"));
        next = set_at(&next, "appearance.useCustomFont", json!(true));
        next = set_at(&next, "appearance.uiFontFamily", json!("思源黑体"));
        next = set_at(&next, "terminal.fontFamily", json!("Independent Terminal"));
        let settings = parse_settings(&next).expect("the backend takes the settings");
        assert_eq!(settings.editor.font_family, "Maple Mono NF");
        assert_eq!(settings.terminal.font_family, "Independent Terminal");
        let applied = runtime_settings(&settings);
        assert!(applied.appearance.font_ui.is_some_and(|font| font.contains("思源黑体")));
    }

    // Not from a spec: the language tag follows the locale, idle saving is off by default, and an
    // event after a re-read schedules the next one.
    #[test]
    fn applies_the_locale_and_schedules_one_read_per_burst() {
        let defaults = runtime_settings(&Settings::default());
        assert_eq!(defaults.language_tag, LOCALES[0].tag);
        assert_eq!((defaults.drag_with_alt_only, defaults.save_after_idle_seconds), (false, None));
        let stored = json!({ "general": { "saveOnIdle": true, "idleSeconds": 20 }, "appearance": { "locale": "zh" } });
        let custom = runtime_settings(&parse_settings(&stored).expect("the backend takes the settings"));
        assert_eq!(custom.save_after_idle_seconds, Some(20));
        assert_eq!(Some(custom.language_tag), LOCALES.iter().find(|locale| locale.id == "zh").map(|locale| locale.tag));
        let mut reload = SettingsReload::default();
        assert!(!reload.take_due(100.0));
        assert_eq!(reload.settings_changed(100.0), 130.0);
        assert_eq!(reload.settings_changed(120.0), 150.0);
        assert!(!reload.take_due(140.0));
        assert!(reload.take_due(150.0));
        reload.settings_changed(200.0);
        reload.cancel();
        assert!(!reload.take_due(500.0));
    }
}
