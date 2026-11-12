//! The project open in a window, and what the window shows while another one is being opened:
//! the current project stays until the new one is there, the target is shown, and actions wait
//! (the `projectOpening` context key).
//!
//! Path helpers (`relative_dir`, `basename`…) are [`crate::platform::virtual_docs`]'s; the
//! content root of a file is [`crate::platform::misc::copy_path::content_root`].

/// What the session needs to know of a project.
pub trait OpenedProject {
    type Settings: Clone;

    fn root(&self) -> &str;
    /// The settings in effect for the project.
    fn settings(&self) -> &Self::Settings;
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenStep {
    /// Ask whether the folder is trusted, then report [`ProjectSession::trust_confirmed`] or
    /// [`ProjectSession::trust_cancelled`].
    ConfirmTrust {
        path: String,
    },
    /// Have the backend open the folder, then report [`ProjectSession::opened`] or
    /// [`ProjectSession::open_failed`].
    Open {
        path: String,
    },
    /// Ask the backend which project the window has now, then report
    /// [`ProjectSession::reconciled`].
    ReadCurrentProject,
    Done,
}

/// The project state of one window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSession<P: OpenedProject> {
    project: Option<P>,
    /// Effective settings (project-layered when a project is open).
    settings: Option<P::Settings>,
    opening: bool,
    opening_path: Option<String>,
    /// The window edits loose files without a project.
    light_edit: bool,
}

impl<P: OpenedProject> Default for ProjectSession<P> {
    fn default() -> Self {
        Self { project: None, settings: None, opening: false, opening_path: None, light_edit: false }
    }
}

impl<P: OpenedProject> ProjectSession<P> {
    pub fn new() -> Self {
        Self::default()
    }

    /// A window that already shows `project`.
    pub fn with_project(project: P) -> Self {
        let settings = Some(project.settings().clone());
        Self { project: Some(project), settings, ..Self::default() }
    }

    pub fn project(&self) -> Option<&P> {
        self.project.as_ref()
    }

    pub fn settings(&self) -> Option<&P::Settings> {
        self.settings.as_ref()
    }

    pub fn is_opening(&self) -> bool {
        self.opening
    }

    /// The folder being opened, shown while it opens.
    pub fn opening_path(&self) -> Option<&str> {
        self.opening_path.as_deref()
    }

    pub fn is_light_edit(&self) -> bool {
        self.light_edit
    }

    /// The `projectOpening` context key: while it holds, only window and quit actions run.
    pub fn project_opening(&self) -> bool {
        self.opening_path.is_some()
    }

    /// Whether leaving what the window shows needs the unsaved-changes check first.
    pub fn has_something_to_leave(&self) -> bool {
        self.project.is_some() || self.light_edit
    }

    /// Open Project was asked for `path`: trust comes first, and nothing changes until then.
    pub fn open_project(&self, path: &str) -> OpenStep {
        OpenStep::ConfirmTrust { path: path.to_owned() }
    }

    /// The folder is trusted: the window shows that `path` is opening.
    pub fn trust_confirmed(&mut self, path: &str) -> OpenStep {
        self.opening = true;
        self.opening_path = Some(path.to_owned());
        OpenStep::Open { path: path.to_owned() }
    }

    /// The trust prompt was dismissed: the current project stays as it is.
    pub fn trust_cancelled(&self) -> OpenStep {
        OpenStep::Done
    }

    fn opening_over(&mut self) {
        self.opening = false;
        self.opening_path = None;
    }

    /// The backend opened `project`.
    pub fn opened(&mut self, project: P) -> OpenStep {
        self.settings = Some(project.settings().clone());
        self.project = Some(project);
        self.light_edit = false;
        self.opening_over();
        OpenStep::Done
    }

    /// The backend could not open the folder. It may have closed the old project already, so a
    /// window that showed one asks which project it has now.
    pub fn open_failed(&mut self) -> OpenStep {
        if self.project.is_some() {
            return OpenStep::ReadCurrentProject;
        }
        self.opening_over();
        OpenStep::Done
    }

    /// After a failed open the backend has `current` (`None`: no project, or it could not say).
    /// The window follows it when it is another project than the one shown.
    pub fn reconciled(&mut self, current: Option<P>) -> OpenStep {
        let shown = self.project.as_ref().map(OpenedProject::root);
        if current.as_ref().map(OpenedProject::root) != shown {
            if let Some(current) = &current {
                self.settings = Some(current.settings().clone());
            }
            self.project = current;
            self.light_edit = false;
        }
        self.opening_over();
        OpenStep::Done
    }

    /// The window switched to editing the loose file it was given, with these settings.
    pub fn light_edit_opened(&mut self, settings: P::Settings) {
        self.project = None;
        self.settings = Some(settings);
        self.light_edit = true;
    }

    pub fn project_closed(&mut self) {
        self.project = None;
        self.light_edit = false;
    }

    /// The effective settings were read again.
    pub fn settings_loaded(&mut self, settings: P::Settings) {
        self.settings = Some(settings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::{ActionMeta, is_action_enabled, is_action_key_enabled};
    use crate::platform::context::ContextKeys;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Project {
        root: &'static str,
        settings: &'static str,
    }

    impl OpenedProject for Project {
        type Settings = &'static str;

        fn root(&self) -> &str {
            self.root
        }

        fn settings(&self) -> &&'static str {
            &self.settings
        }
    }

    const OLD: Project = Project { root: "/old", settings: "old settings" };
    const NEXT: Project = Project { root: "/new", settings: "new settings" };

    fn context(session: &ProjectSession<Project>) -> ContextKeys {
        let mut ctx = ContextKeys::new();
        ctx.set("projectOpening", session.project_opening());
        ctx
    }

    // shows the target while opening and blocks actions from both menus and shortcuts
    #[test]
    fn shows_the_target_while_opening_and_blocks_actions_from_both_menus_and_shortcuts() {
        let mut session = ProjectSession::with_project(OLD);
        assert_eq!(session.open_project(NEXT.root), OpenStep::ConfirmTrust { path: "/new".to_owned() });
        assert_eq!(session.trust_confirmed(NEXT.root), OpenStep::Open { path: "/new".to_owned() });
        assert_eq!((session.project(), session.is_opening(), session.opening_path()), (Some(&OLD), true, Some("/new")));
        let action = ActionMeta::new("test.projectMutation", "test");
        let opening = context(&session);
        assert!(!is_action_key_enabled(&action, &opening));
        assert!(!is_action_enabled(&action, &opening));
        assert!(is_action_key_enabled(&ActionMeta::new("app.quit", "test"), &opening));
        assert_eq!(session.opened(NEXT), OpenStep::Done);
        assert_eq!((session.project(), session.is_opening(), session.opening_path()), (Some(&NEXT), false, None));
        assert_eq!(session.settings(), Some(&"new settings"));
        assert!(!session.project_opening());
        assert!(is_action_enabled(&action, &context(&session)));
    }

    // reconciles a failed open with the backend's remaining project: %s
    #[test]
    fn reconciles_a_failed_open_with_the_backends_remaining_project() {
        for current in [Some(OLD), None, Some(NEXT)] {
            let mut session = ProjectSession::with_project(OLD);
            assert_eq!(session.trust_confirmed(NEXT.root), OpenStep::Open { path: "/new".to_owned() });
            assert_eq!(session.open_failed(), OpenStep::ReadCurrentProject);
            assert_eq!(session.reconciled(current.clone()), OpenStep::Done);
            assert_eq!(session.project(), current.as_ref());
            assert_eq!((session.is_opening(), session.opening_path()), (false, None));
            // The settings follow a project that took the old one's place, and stay otherwise.
            let expected = if current == Some(NEXT) { "new settings" } else { "old settings" };
            assert_eq!(session.settings(), Some(&expected));
        }
    }

    // leaves the current project alone when trust is cancelled
    #[test]
    fn leaves_the_current_project_alone_when_trust_is_cancelled() {
        let mut session = ProjectSession::with_project(OLD);
        let before = session.clone();
        assert_eq!(session.open_project(NEXT.root), OpenStep::ConfirmTrust { path: "/new".to_owned() });
        // Nothing is asked of the backend.
        assert_eq!(session.trust_cancelled(), OpenStep::Done);
        assert_eq!(session, before);
        assert_eq!((session.project(), session.is_opening(), session.opening_path()), (Some(&OLD), false, None));
        session.project_closed();
        assert!(!session.has_something_to_leave());
    }

    // Not from a spec: a window without a project has nothing to reconcile after a failed open,
    // and a loose file is edited without a project.
    #[test]
    fn opens_from_an_empty_window_and_edits_loose_files() {
        let mut session: ProjectSession<Project> = ProjectSession::new();
        assert!(!session.has_something_to_leave());
        assert_eq!(session.trust_confirmed("/new"), OpenStep::Open { path: "/new".to_owned() });
        assert_eq!(session.open_failed(), OpenStep::Done);
        assert!(!session.project_opening());
        session.light_edit_opened("loose settings");
        assert!(session.is_light_edit() && session.has_something_to_leave() && session.project().is_none());
        session.settings_loaded("reloaded");
        assert_eq!(session.settings(), Some(&"reloaded"));
        assert_eq!(session.opened(NEXT), OpenStep::Done);
        assert!(!session.is_light_edit());
    }
}
