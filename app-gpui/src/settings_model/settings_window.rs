//! The start-up of the Settings window: the dialog opens at once, over the settings known so far,
//! while its pages register and the project's settings load. A page that fails to register is
//! logged and does not keep the window from opening.

use super::staging::SettingsDialog;
use serde_json::Value;

/// The window starts: the dialog is open and its pages are loading.
pub fn start(dialog: &mut SettingsDialog, settings: Value) {
    dialog.pages_loading = true;
    dialog.open(None, false, settings);
}

/// The built-in pages registered, or failed to with `error`. Returns the line to log for a
/// failure; the window stays open either way.
pub fn pages_registered(result: Result<(), String>) -> Option<String> {
    result.err().map(|error| format!("cannot register settings pages: {error}"))
}

/// The project's settings arrived: the dialog compares edits with them from now on.
pub fn settings_loaded(dialog: &mut SettingsDialog, settings: Value) {
    dialog.rebase(settings);
}

/// Pages and settings are in, or failed: the window stops showing that it is loading.
pub fn finished(dialog: &mut SettingsDialog) {
    dialog.pages_loading = false;
}

/// What the window does when the dialog's state changed from `before` to `after`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEffect {
    /// The dialog closed: the window goes with it.
    Close,
    SetTitle(&'static str),
}

pub fn window_effects(before: &SettingsDialog, after: &SettingsDialog) -> Vec<WindowEffect> {
    let mut effects = Vec::new();
    if before.open && !after.open {
        effects.push(WindowEffect::Close);
    }
    if before.new_projects != after.new_projects {
        effects.push(WindowEffect::SetTitle(super::staging::settings_title(after.new_projects)));
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings_model::schema::{default_settings, set_at};
    use serde_json::json;

    // the native Settings window › opens even when a page fails to register, and logs why
    #[test]
    fn opens_even_when_a_page_fails_to_register_and_logs_why() {
        let mut dialog = SettingsDialog::default();
        start(&mut dialog, default_settings());
        assert!(dialog.open);
        assert!(dialog.pages_loading);
        let logged = pages_registered(Err("Error: page failed".to_owned()));
        assert!(logged.is_some_and(|line| line.contains("cannot register settings pages: Error: page failed")));
        finished(&mut dialog);
        assert!(dialog.open);
        assert!(!dialog.pages_loading);
    }

    // Not from a spec: the project's settings replace the base the dialog opened over, closing
    // the dialog closes the window, and Settings for New Projects retitles it.
    #[test]
    fn loads_the_projects_settings_and_follows_the_dialog() {
        let mut dialog = SettingsDialog::default();
        start(&mut dialog, default_settings());
        assert_eq!(pages_registered(Ok(())), None);
        settings_loaded(&mut dialog, set_at(&default_settings(), "editor.tabSize", json!(8)));
        assert_eq!(dialog.base["editor"]["tabSize"], 8);
        let before = dialog.clone();
        dialog.new_projects = true;
        assert_eq!(window_effects(&before, &dialog), [WindowEffect::SetTitle("Settings for New Projects")]);
        dialog.close();
        assert!(window_effects(&before, &dialog).contains(&WindowEffect::Close));
    }
}
