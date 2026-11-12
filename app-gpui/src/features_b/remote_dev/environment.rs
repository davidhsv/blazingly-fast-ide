//! Reload Environment: new terminals, runs, language servers and tools get the login shell's
//! current environment (and, on a remote host, the project's `devcontainer.json` `remoteEnv`)
//! without restarting the backend. What is already running keeps its own.
//!
//! The caller runs `env_reload` and hands the answer to [`reload_notification`]. Its "Restart
//! Language Servers" button carries [`RESTART_LANGUAGE_SERVERS`]: the caller then lists the
//! language servers (`lsp_servers`), hands the answer to [`servers_listed`] and restarts each
//! server it names (`lsp_restart`).

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_core::{IdeError, ServerId};
use ide_host::environment::EnvReloaded;

/// The key of the reload's notification: a new reload replaces the previous one's.
pub const RELOAD_KEY: &str = "remote.reloadEnvironment";
/// The command of the notification's "Restart Language Servers" button.
pub const RESTART_LANGUAGE_SERVERS: &str = "remote.restartLanguageServers";
/// Variable names listed in the notification before "and N more".
const LISTED: usize = 8;

/// The notification text for a reload result.
pub fn reload_summary(reloaded: &EnvReloaded, i18n: &mut I18n) -> String {
    let names = reloaded.changed.iter().take(LISTED).map(String::as_str).collect::<Vec<_>>().join(", ");
    let changed = match reloaded.changed.len() {
        0 => i18n.tx("No variables changed."),
        count if count > LISTED => {
            i18n.tx_built(&["Changed: ", " and ", " more."], &[Param::from(names), Param::from(count - LISTED)])
        }
        _ => i18n.tx_built(&["Changed: ", "."], &[Param::from(names)]),
    };
    let keep = i18n.tx("Terminals and processes already running keep their old environment; new ones get the new one.");
    format!("{changed} {keep}")
}

/// The notification for the answer of `env_reload`. When variables changed it offers to restart
/// the language servers, which read the environment only when they start.
pub fn reload_notification(result: &Result<EnvReloaded, IdeError>, i18n: &mut I18n) -> NotificationRequest {
    let mut request = match result {
        Ok(reloaded) => {
            let mut request = NotificationRequest::new(NotificationLevel::Info, "Environment reloaded");
            request.body = Some(reload_summary(reloaded, i18n));
            if !reloaded.changed.is_empty() {
                request.actions.push(NotificationAction {
                    label: "Restart Language Servers".to_owned(),
                    command: RESTART_LANGUAGE_SERVERS.to_owned(),
                });
            }
            request
        }
        Err(error) => {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Could not reload the environment");
            request.body = Some(error.message.clone());
            request
        }
    };
    request.key = Some(RELOAD_KEY.to_owned());
    request
}

/// What to do once the language servers are listed for a restart.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerRestart {
    /// Restart these, in order (`lsp_restart` each); a failure goes to [`restart_failed`].
    Restart(Vec<ServerId>),
    /// The servers could not be listed.
    Failed(NotificationRequest),
}

/// The restarts for the answer of `lsp_servers` (the ids of the servers it listed).
pub fn servers_listed(result: Result<Vec<ServerId>, IdeError>) -> ServerRestart {
    match result {
        Ok(servers) => ServerRestart::Restart(servers),
        Err(error) => ServerRestart::Failed(restart_failed(&error)),
    }
}

/// The notification for a restart (or the listing before it) that failed.
pub fn restart_failed(error: &IdeError) -> NotificationRequest {
    let mut request = NotificationRequest::new(NotificationLevel::Error, "Failed to restart language server");
    request.body = Some(error.message.clone());
    request.group = Some("lsp".to_owned());
    request
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::error::ErrorKind;

    fn reloaded(changed: &[&str]) -> EnvReloaded {
        EnvReloaded {
            changed: changed.iter().map(|&name| name.to_owned()).collect(),
            devcontainer: None,
            shell_read: true,
        }
    }

    // Reload Environment › says what changed and that running processes keep their environment
    #[test]
    fn says_what_changed_and_that_running_processes_keep_their_environment() {
        let mut i18n = I18n::default();
        assert_eq!(
            reload_summary(&reloaded(&[]), &mut i18n),
            "No variables changed. Terminals and processes already running keep their old environment; new ones get \
             the new one.",
        );
        let two = reload_summary(&reloaded(&["JAVA_HOME", "PATH"]), &mut i18n);
        assert!(two.starts_with("Changed: JAVA_HOME, PATH. Terminals"), "{two}");
        let many: Vec<String> = (0..10).map(|i| format!("V{i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let ten = reload_summary(&reloaded(&many), &mut i18n);
        assert!(ten.starts_with("Changed: V0, V1, V2, V3, V4, V5, V6, V7 and 2 more."), "{ten}");
    }

    // Reload Environment › offers to restart the language servers when something changed
    #[test]
    fn offers_to_restart_the_language_servers_when_something_changed() {
        let mut i18n = I18n::default();
        let note = reload_notification(&Ok(reloaded(&["PATH"])), &mut i18n);
        assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Info, "Environment reloaded"));
        assert_eq!(note.key.as_deref(), Some(RELOAD_KEY));
        let restart = note.actions.iter().find(|action| action.label == "Restart Language Servers");
        assert_eq!(restart.map(|action| action.command.as_str()), Some(RESTART_LANGUAGE_SERVERS));
        let servers = vec![ServerId(1), ServerId(2)];
        assert_eq!(servers_listed(Ok(servers.clone())), ServerRestart::Restart(servers));
        let failed = restart_failed(&IdeError::new(ErrorKind::Unavailable, "gone"));
        assert_eq!((failed.level, failed.group.as_deref()), (NotificationLevel::Error, Some("lsp")));
    }

    // Reload Environment › has nothing to restart when nothing changed
    #[test]
    fn has_nothing_to_restart_when_nothing_changed() {
        let mut i18n = I18n::default();
        assert!(reload_notification(&Ok(reloaded(&[])), &mut i18n).actions.is_empty());
    }

    // Reload Environment › reports a failure as an error
    #[test]
    fn reports_a_failure_as_an_error() {
        let mut i18n = I18n::default();
        let error =
            IdeError::new(ErrorKind::Unavailable, "could not read the environment of the login shell (/bin/zsh)");
        let note = reload_notification(&Err(error), &mut i18n);
        assert_eq!((note.level, note.title.as_str()), (NotificationLevel::Error, "Could not reload the environment"));
        assert!(note.body.is_some_and(|body| body.contains("login shell")));
    }
}
