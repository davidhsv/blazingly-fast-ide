//! Tools › Install / Uninstall 'openidea' Command: what the user is told afterwards.

use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::IdeError;
use ide_core::error::ErrorKind;

/// The key both notifications replace each other under.
pub const CLI_KEY: &str = "app.cli";

/// Where the command was installed, and the folder to add to `PATH` when it is not on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInstallation {
    pub path: String,
    pub path_directory: Option<String>,
}

fn keyed(level: NotificationLevel, title: String, body: Option<String>) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.body = body;
    request.key = Some(CLI_KEY.to_owned());
    request
}

/// The answer to Install: where the command is and what to do before using it, or why it could
/// not be installed. A cancelled administrator prompt says nothing.
pub fn installed_notice(result: Result<CliInstallation, IdeError>, i18n: &mut I18n) -> Option<NotificationRequest> {
    match result {
        Ok(installed) => {
            let hint = match &installed.path_directory {
                Some(dir) => i18n.tx_built(
                    &["Add ", " to the beginning of PATH, then open a new terminal."],
                    &[Param::Text(dir.clone())],
                ),
                None => i18n.tx("Open a new terminal to use the openidea command."),
            };
            let title = i18n.tx("The openidea command is installed");
            Some(keyed(NotificationLevel::Info, title, Some(format!("{}\n{hint}", installed.path))))
        }
        Err(error) if error.kind == ErrorKind::Cancelled => None,
        Err(error) => {
            let title = i18n.tx("Cannot install the openidea command");
            Some(keyed(NotificationLevel::Error, title, Some(error.message)))
        }
    }
}

/// The answer to Uninstall: the links removed, or that there was nothing to remove.
pub fn uninstalled_notice(result: Result<Vec<String>, IdeError>, i18n: &I18n) -> Option<NotificationRequest> {
    match result {
        Ok(links) if links.is_empty() => {
            Some(keyed(NotificationLevel::Info, i18n.tx("The openidea command is not installed"), None))
        }
        Ok(links) => {
            let title = i18n.tx("The openidea command is uninstalled");
            Some(keyed(NotificationLevel::Info, title, Some(links.join("\n"))))
        }
        Err(error) if error.kind == ErrorKind::Cancelled => None,
        Err(error) => {
            let title = i18n.tx("Cannot uninstall the openidea command");
            Some(keyed(NotificationLevel::Error, title, Some(error.message)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // explains the Linux PATH step when the user bin is missing from PATH
    #[test]
    fn explains_the_linux_path_step_when_the_user_bin_is_missing_from_path() {
        let installed = CliInstallation {
            path: "/home/user/.local/bin/openidea".into(),
            path_directory: Some("/home/user/.local/bin".into()),
        };
        let notice = installed_notice(Ok(installed), &mut I18n::default()).expect("a notice");
        assert_eq!(notice.level, NotificationLevel::Info);
        assert!(notice.body.is_some_and(|body| body.contains("Add /home/user/.local/bin to the beginning of PATH")));
    }

    // tells users to open a new terminal after installation
    #[test]
    fn tells_users_to_open_a_new_terminal_after_installation() {
        let installed =
            CliInstallation { path: "C:/Users/user/OpenIDEA/bin/openidea.cmd".into(), path_directory: None };
        let notice = installed_notice(Ok(installed), &mut I18n::default()).expect("a notice");
        assert!(notice.body.is_some_and(|body| body.contains("Open a new terminal")));
    }

    // reports an absent command on repeated uninstall
    #[test]
    fn reports_an_absent_command_on_repeated_uninstall() {
        let notice = uninstalled_notice(Ok(Vec::new()), &I18n::default()).expect("a notice");
        assert_eq!(notice.title, "The openidea command is not installed");
    }

    // reports an install failure and stays quiet when an administrator prompt is cancelled
    #[test]
    fn reports_an_install_failure_and_stays_quiet_when_an_administrator_prompt_is_cancelled() {
        let mut i18n = I18n::default();
        let failed = installed_notice(Err(IdeError::new(ErrorKind::Internal, "foreign command")), &mut i18n);
        let failed = failed.expect("a notice");
        assert_eq!((failed.level, failed.body.as_deref()), (NotificationLevel::Error, Some("foreign command")));
        assert_eq!(installed_notice(Err(IdeError::cancelled()), &mut i18n), None);
    }
}
