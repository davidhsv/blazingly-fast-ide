//! The Sync with Deployed window (`deployment-<parent>`): one per project window, sharing that
//! project's documents. It opens the comparison a launch asks for and, when closed, keeps unsaved
//! local edits unless they were saved.
//!
//! Launches are handled one at a time: the caller takes the next launch only after the previous
//! one ended.

use super::store::DeploymentStore;
use crate::platform::i18n::{I18n, Param};
use ide_core::settings::{DeploymentServer, DeploymentSettings};

/// The event a comparison window sends its parent when it stays open after a close request.
pub const AUX_CLOSED: &str = "openidea://auxiliary/closed";

/// What a launch of the window asks to compare (`window_take_launch().deployment`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchRequest {
    pub server: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LaunchStep {
    /// Nothing to do.
    Idle,
    /// Show this message box; the launch ends.
    ShowError { title: String, message: String },
    /// Read the project (`project_current`), make it this window's project, then call
    /// [`ComparisonWindow::project_loaded`] with its deployment settings.
    LoadProject,
    /// Set the window title, open the comparison
    /// ([`DeploymentStore::open_sync`]) and call [`ComparisonWindow::sync_opened`].
    Open { title: String, server: DeploymentServer, paths: Vec<String> },
    /// The launch failed: the error is in the comparison, or [`ComparisonWindow::startup_error`].
    Failed,
}

/// What the parent window is told: this window stays open (`closed: false`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxClosed {
    pub parent: String,
    pub label: String,
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CloseStep {
    /// A close is in progress already.
    Ignore,
    /// Ask whether to save, with one primary button `save_label`; answer with
    /// [`ComparisonWindow::close_answer`].
    Ask { title: String, body: String, save_label: String },
    /// Save every edited document; answer with [`ComparisonWindow::saved`].
    Save,
    /// Emit [`AUX_CLOSED`] to the parent and keep the window.
    StayOpen(AuxClosed),
    /// [`CloseStep::StayOpen`], and show the error.
    Failed { stay_open: AuxClosed, title: String, message: String },
    /// Close the comparison ([`DeploymentStore::close_sync`]), answer an open SSH prompt with
    /// nothing, dispose the documents and destroy the window, in that order; then call
    /// [`ComparisonWindow::torn_down`].
    Teardown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonWindow {
    parent: String,
    label: String,
    closing: bool,
    disposed: bool,
    pending: Option<LaunchRequest>,
    /// The error shown instead of a comparison that could not open.
    pub startup_error: Option<String>,
}

/// "Diff Between 'local files' and 'files on Staging'".
pub fn comparison_title(server_name: &str, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Diff Between 'local files' and 'files on ", "'"], &[Param::from(server_name)])
}

impl ComparisonWindow {
    /// `label` is this window's, `parent` the project window's.
    pub fn new(parent: &str, label: &str) -> Self {
        Self {
            parent: parent.to_owned(),
            label: label.to_owned(),
            closing: false,
            disposed: false,
            pending: None,
            startup_error: None,
        }
    }

    pub fn dispose(&mut self) {
        self.disposed = true;
    }

    /// Whether to take the next launch (`window_take_launch`).
    pub fn can_launch(&self) -> bool {
        !self.disposed && !self.closing
    }

    /// An error of the launch: shown in the comparison when one is open, else instead of it.
    pub fn fail(&mut self, message: String, store: &mut DeploymentStore) -> LaunchStep {
        match store.sync.as_mut() {
            Some(sync) => sync.error = Some(message),
            None => self.startup_error = Some(message),
        }
        LaunchStep::Failed
    }

    /// The answer of `window_take_launch`.
    pub fn launch_taken(&mut self, launch: Option<LaunchRequest>, store: &DeploymentStore, i18n: &I18n) -> LaunchStep {
        let Some(launch) = launch.filter(|_| self.can_launch()) else { return LaunchStep::Idle };
        if store.sync.as_ref().is_some_and(|sync| sync.running) {
            return LaunchStep::ShowError {
                title: i18n.tx("Deployment comparison"),
                message: i18n.tx("Wait for synchronization to finish before opening another comparison."),
            };
        }
        self.pending = Some(launch);
        LaunchStep::LoadProject
    }

    /// The project was read (`None` when it is no longer open): opens the launched server.
    pub fn project_loaded(
        &mut self,
        deployment: Option<&DeploymentSettings>,
        store: &mut DeploymentStore,
        i18n: &mut I18n,
    ) -> LaunchStep {
        let Some(launch) = self.pending.take() else { return LaunchStep::Idle };
        let Some(deployment) = deployment else {
            return self.fail(i18n.tx("The project is no longer open."), store);
        };
        let Some(server) = deployment.servers.iter().find(|server| server.id == launch.server) else {
            return self.fail(i18n.tx("The deployment server is no longer available."), store);
        };
        let name = if server.name.is_empty() { &server.id } else { &server.name };
        LaunchStep::Open { title: comparison_title(name, i18n), server: server.clone(), paths: launch.paths }
    }

    /// The comparison was opened (or not, when nothing selected is mapped to the server).
    pub fn sync_opened(&mut self, store: &mut DeploymentStore, i18n: &I18n) -> LaunchStep {
        if store.sync.is_some() || self.closing {
            return LaunchStep::Idle;
        }
        self.fail(i18n.tx("No selected files are mapped to this deployment server."), store)
    }

    /// The comparison closed by itself (Cancel, Synchronize All): close the window too.
    pub fn should_close(&self, had_sync: bool, has_sync: bool) -> bool {
        had_sync && !has_sync && !self.closing
    }

    fn stay_open(&self) -> AuxClosed {
        AuxClosed { parent: self.parent.clone(), label: self.label.clone(), closed: false }
    }

    /// The window is asked to close. `dirty`: local files were edited; `auto_save_off`: the
    /// project's `files.autoSave` is off, so closing asks before saving them.
    pub fn close_requested(&mut self, dirty: bool, auto_save_off: bool, i18n: &I18n) -> CloseStep {
        if self.closing {
            return CloseStep::Ignore;
        }
        self.closing = true;
        if dirty && auto_save_off {
            return CloseStep::Ask {
                title: i18n.tx("Save local changes?"),
                body: i18n.tx("Save the local files edited in this comparison before closing."),
                save_label: i18n.tx("Save and Close"),
            };
        }
        CloseStep::Save
    }

    /// The answer of [`CloseStep::Ask`]: anything but Save and Close keeps the window.
    pub fn close_answer(&mut self, save: bool) -> CloseStep {
        if save {
            return CloseStep::Save;
        }
        self.closing = false;
        CloseStep::StayOpen(self.stay_open())
    }

    fn failed(&mut self, message: String, i18n: &I18n) -> CloseStep {
        self.closing = false;
        CloseStep::Failed { stay_open: self.stay_open(), title: i18n.tx("Cannot close deployment comparison"), message }
    }

    /// The answer of [`CloseStep::Save`]: a failed save keeps the window and the edits.
    pub fn saved(&mut self, result: Result<(), String>, i18n: &I18n) -> CloseStep {
        match result {
            Ok(()) => CloseStep::Teardown,
            Err(message) => self.failed(message, i18n),
        }
    }

    /// The answer of [`CloseStep::Teardown`].
    pub fn torn_down(&mut self, result: Result<(), String>, i18n: &I18n) -> CloseStep {
        match result {
            Ok(()) => {
                self.closing = false;
                CloseStep::Ignore
            }
            Err(message) => self.failed(message, i18n),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::DeploymentCoverage;
    use super::*;
    use ide_core::RequestId;

    // loads the parent project before comparing the launched server and local selection
    #[test]
    fn loads_the_parent_project_before_comparing_the_launched_server_and_local_selection() {
        let mut i18n = I18n::default();
        let mut store = DeploymentStore::default();
        let mut window = ComparisonWindow::new("project-2", "deployment-project-2");
        let launch = LaunchRequest { server: "s2".to_owned(), paths: vec!["/second/a.php".to_owned()] };
        assert_eq!(window.launch_taken(Some(launch), &store, &i18n), LaunchStep::LoadProject);
        let server = DeploymentServer { id: "s2".to_owned(), name: "Second project".to_owned(), ..Default::default() };
        let deployment = DeploymentSettings { servers: vec![server.clone()], ..Default::default() };
        let step = window.project_loaded(Some(&deployment), &mut store, &mut i18n);
        let LaunchStep::Open { title, server: opened, paths } = step else { panic!("the comparison opens") };
        assert_eq!(title, "Diff Between 'local files' and 'files on Second project'");
        assert_eq!((opened.id.as_str(), paths.as_slice()), ("s2", ["/second/a.php".to_owned()].as_slice()));
        let coverage = [DeploymentCoverage { server: "s2".to_owned(), paths: paths.clone(), skipped: Vec::new() }];
        store.open_sync(&opened, paths, &coverage, RequestId(1), &mut i18n);
        assert_eq!(window.sync_opened(&mut store, &i18n), LaunchStep::Idle);
        assert_eq!(window.startup_error, None);

        let mut empty = DeploymentStore::default();
        window.launch_taken(Some(LaunchRequest { server: "gone".to_owned(), paths: Vec::new() }), &empty, &i18n);
        assert_eq!(window.project_loaded(Some(&deployment), &mut empty, &mut i18n), LaunchStep::Failed);
        assert_eq!(window.startup_error.as_deref(), Some("The deployment server is no longer available."));
    }

    // keeps the window and unsaved local edits when closing is cancelled or saving fails
    #[test]
    fn keeps_the_window_and_unsaved_local_edits_when_closing_is_cancelled_or_saving_fails() {
        let i18n = I18n::default();
        let mut window = ComparisonWindow::new("project-2", "deployment-project-2");
        let stay_open =
            AuxClosed { parent: "project-2".to_owned(), label: "deployment-project-2".to_owned(), closed: false };
        assert!(matches!(window.close_requested(true, true, &i18n), CloseStep::Ask { .. }));
        assert_eq!(window.close_requested(true, true, &i18n), CloseStep::Ignore);
        // The project window waiting to close stops.
        assert_eq!(window.close_answer(false), CloseStep::StayOpen(stay_open.clone()));

        assert!(matches!(window.close_requested(true, true, &i18n), CloseStep::Ask { .. }));
        assert_eq!(window.close_answer(true), CloseStep::Save);
        let failed = CloseStep::Failed {
            stay_open,
            title: "Cannot close deployment comparison".to_owned(),
            message: "Disk full".to_owned(),
        };
        assert_eq!(window.saved(Err("Disk full".to_owned()), &i18n), failed);

        assert!(matches!(window.close_requested(true, true, &i18n), CloseStep::Ask { .. }));
        assert_eq!(window.close_answer(true), CloseStep::Save);
        assert_eq!(window.saved(Ok(()), &i18n), CloseStep::Teardown);
        assert_eq!(window.torn_down(Ok(()), &i18n), CloseStep::Ignore);
        assert_eq!(window.close_requested(false, true, &i18n), CloseStep::Save);
    }
}
