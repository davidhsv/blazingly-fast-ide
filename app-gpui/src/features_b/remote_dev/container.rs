//! Reopen in Container: the local project's dev container, opened as a remote project.

use crate::platform::i18n::I18n;
use crate::platform_more::remote::{RemoteTarget, RemoteWindow};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::IdeError;
use ide_core::error::ErrorKind;

/// A remote project to open: `path` on `target`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteOpen {
    pub target: RemoteTarget,
    pub path: String,
}

/// The dev container (its default `devcontainer.json`) of this window's local project, to open
/// with the project's folder as its path. `None` in a remote window or without a project. The
/// folder must be trusted: building the container runs the project's commands.
pub fn reopen_in_container(project_root: Option<&str>, window: &RemoteWindow) -> Option<RemoteOpen> {
    let root = project_root.filter(|root| !root.is_empty() && !window.is_remote())?;
    Some(RemoteOpen {
        target: RemoteTarget::DevContainer { folder: root.to_owned(), config: String::new() },
        path: root.to_owned(),
    })
}

/// The notification for opening the container failing; `None` when the user cancelled it.
pub fn reopen_failed(error: &IdeError, i18n: &I18n) -> Option<NotificationRequest> {
    if error.kind == ErrorKind::Cancelled {
        return None;
    }
    let mut request =
        NotificationRequest::new(NotificationLevel::Error, i18n.tx("Could not reopen the project in a container"));
    request.body = Some(error.message.clone());
    request.group = Some("remote".to_owned());
    Some(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_more::remote::RemoteSession;

    // opens the local project's dev container
    #[test]
    fn opens_the_local_projects_dev_container() {
        let open = reopen_in_container(Some("/Users/me/app"), &RemoteWindow::new());
        let target = RemoteTarget::DevContainer { folder: "/Users/me/app".to_owned(), config: String::new() };
        assert_eq!(open, Some(RemoteOpen { target, path: "/Users/me/app".to_owned() }));
    }

    // does nothing in a remote window or without a project
    #[test]
    fn does_nothing_in_a_remote_window_or_without_a_project() {
        let mut window = RemoteWindow::new();
        window.enable(RemoteSession {
            target: RemoteTarget::Ssh { config: "a".to_owned() },
            root: "/srv/app".to_owned(),
            host: "a".to_owned(),
            os: "linux".to_owned(),
            commands: Vec::new(),
        });
        assert_eq!(reopen_in_container(Some("/Users/me/app"), &window), None);
        assert_eq!(reopen_in_container(None, &RemoteWindow::new()), None);
    }

    // reports why the container could not be opened, but not a cancel
    #[test]
    fn reports_why_the_container_could_not_be_opened_but_not_a_cancel() {
        let i18n = I18n::default();
        let missing = IdeError::new(ErrorKind::Unavailable, "The Dev Containers CLI is not installed.");
        let note = reopen_failed(&missing, &i18n).expect("a failure is reported");
        assert_eq!(note.level, NotificationLevel::Error);
        assert_eq!(note.body.as_deref(), Some("The Dev Containers CLI is not installed."));
        let cancelled = IdeError::new(ErrorKind::Cancelled, "Project opening cancelled");
        assert_eq!(reopen_failed(&cancelled, &i18n), None);
    }
}
