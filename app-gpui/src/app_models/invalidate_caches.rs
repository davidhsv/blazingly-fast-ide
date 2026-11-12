//! File › Invalidate Caches…: the symbol index is always rebuilt; Local History, language server
//! workspaces and build tool caches optionally. Caches in use are deleted at the next start, so
//! the choice is scheduled and the IDE restarts. Restart IDE is Just Restart.

use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_workspace::caches::InvalidateCachesOptions;

/// What the caller carries out, in order; a failure stops the rest ([`schedule_failed`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachesEffect {
    /// `caches_schedule_invalidation`: these caches, or none (a pending invalidation is dropped).
    Schedule(Option<InvalidateCachesOptions>),
    Restart,
}

/// The dialog's choices; nothing optional is chosen at first.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InvalidateCachesDialog {
    pub options: InvalidateCachesOptions,
}

impl InvalidateCachesDialog {
    /// "Local History of all projects will be deleted." is shown.
    pub fn warns_about_local_history(&self) -> bool {
        self.options.local_history
    }

    /// Invalidate and Restart: the dialog closes, the chosen caches are scheduled, the IDE restarts.
    pub fn invalidate_and_restart(&self) -> [CachesEffect; 2] {
        [CachesEffect::Schedule(Some(self.options)), CachesEffect::Restart]
    }
}

/// Just Restart (and Restart IDE): no invalidation, and one scheduled before is cancelled.
pub fn just_restart() -> [CachesEffect; 2] {
    [CachesEffect::Schedule(None), CachesEffect::Restart]
}

/// Scheduling or restarting failed.
pub fn schedule_failed(error: &str, i18n: &I18n) -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Error, i18n.tx("Could not invalidate caches"));
    request.body = Some(error.to_owned());
    request.key = Some("app.invalidateCaches".to_owned());
    request
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invalidate Caches › schedules the chosen caches and restarts
    #[test]
    fn schedules_the_chosen_caches_and_restarts() {
        let mut dialog = InvalidateCachesDialog::default();
        dialog.options.local_history = true;
        assert!(dialog.warns_about_local_history());
        dialog.options.language_servers = true;
        let chosen = InvalidateCachesOptions { local_history: true, language_servers: true, build_caches: false };
        assert_eq!(dialog.invalidate_and_restart(), [CachesEffect::Schedule(Some(chosen)), CachesEffect::Restart]);
        assert_eq!(schedule_failed("busy", &I18n::default()).body.as_deref(), Some("busy"));
    }

    // Invalidate Caches › Just Restart cancels a pending invalidation; Cancel does nothing
    #[test]
    fn just_restart_cancels_a_pending_invalidation_cancel_does_nothing() {
        assert_eq!(just_restart(), [CachesEffect::Schedule(None), CachesEffect::Restart]);
        // Cancel only closes the dialog: there is nothing to carry out.
        assert!(!InvalidateCachesDialog::default().warns_about_local_history());
    }
}
