//! New Workspace… and Manage Workspace…: a name, a location and the linked project folders.
//! Folders picked with Add Projects… are searched for projects; the ones found are offered as a
//! checklist, the picked folders that are projects themselves (or hold none) are linked at once.

use super::windows::{WorkspaceMembers, restricted_text};
use crate::platform::i18n::I18n;
use crate::platform::virtual_docs::dirname;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::ipc_path::to_ipc;
use ide_workspace::project_discovery::ProjectDiscovery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogMode {
    Create,
    Manage,
}

/// A project the search found below a picked folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub path: String,
    /// The picked folder it was found in.
    pub root: String,
    pub relative_path: String,
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DialogStep {
    /// Search the folders (`workspace_discover_projects` with this request id, run locally even in
    /// a remote window), then report [`WorkspaceDialog::discovered`].
    Discover { request: u64, folders: Vec<String> },
    /// Cancel the search locally (`cancel_request` of this window, not of its remote).
    CancelSearch(u64),
    /// `workspace_create`, then report [`WorkspaceDialog::created`].
    Create { path: String, name: String, projects: Vec<String> },
    /// Confirm the new member is trusted, then report [`WorkspaceDialog::trust_confirmed`].
    ConfirmTrust(String),
    /// Read whether it is trusted now, then report [`WorkspaceDialog::trust_read`].
    ReadTrust(String),
    /// `workspace_update`, show the project it answers, then report [`WorkspaceDialog::updated`].
    Update { name: String, projects: Vec<String> },
    /// Open the created workspace following "Open project in", and close the dialog.
    OpenWithPolicy(String),
    /// Close the dialog.
    Close,
    /// Nothing more for now: the dialog stays as it is.
    Stay,
}

/// The project the window shows when the dialog opens.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DialogProject {
    pub root: String,
    pub workspace: Option<WorkspaceMembers>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceDialog {
    pub mode: DialogMode,
    pub name: String,
    pub path: String,
    /// The Project folder path field.
    pub adding: String,
    projects: Vec<String>,
    members: Vec<String>,
    busy: bool,
    searching: Option<u64>,
    discovery: Option<Vec<Candidate>>,
    selected: Vec<String>,
    truncated: bool,
    error: Option<String>,
    /// Members still to confirm as trusted before Manage applies, and the list it applies.
    pending_trust: Vec<String>,
    included: Vec<String>,
}

fn push_new(list: &mut Vec<String>, items: impl IntoIterator<Item = String>) {
    for item in items {
        if !list.contains(&item) {
            list.push(item);
        }
    }
}

impl WorkspaceDialog {
    /// Create proposes `<parent>/workspace` with the open local project linked; Manage edits the
    /// window's workspace.
    pub fn new(mode: DialogMode, project: Option<&DialogProject>, remote: bool) -> Self {
        let workspace = project.and_then(|p| p.workspace.as_ref()).filter(|_| mode == DialogMode::Manage);
        let local = project.filter(|_| !remote);
        let (name, path, projects) = match (workspace, project) {
            (Some(workspace), Some(project)) => {
                (workspace.name.clone(), project.root.clone(), workspace.projects.clone())
            }
            _ => {
                let path = local.map(|p| format!("{}/workspace", dirname(&p.root))).unwrap_or_default();
                let linked = local.filter(|p| p.workspace.is_none()).map(|p| vec![p.root.clone()]).unwrap_or_default();
                ("Workspace".to_owned(), path, linked)
            }
        };
        let members = workspace.map(|w| w.projects.clone()).unwrap_or_default();
        Self {
            mode,
            name,
            path,
            adding: String::new(),
            projects,
            members,
            busy: false,
            searching: None,
            discovery: None,
            selected: Vec::new(),
            truncated: false,
            error: None,
            pending_trust: Vec::new(),
            included: Vec::new(),
        }
    }

    pub fn projects(&self) -> &[String] {
        &self.projects
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn is_searching(&self) -> bool {
        self.searching.is_some()
    }

    /// "Search stopped at the entry limit…" is shown.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// The inputs and Add Projects… are disabled while saving or searching.
    pub fn disabled(&self) -> bool {
        self.busy || self.searching.is_some()
    }

    /// Create / Apply is enabled.
    pub fn can_save(&self) -> bool {
        !self.disabled() && !self.name.trim().is_empty() && !self.path.trim().is_empty()
    }

    /// The checklist of found projects, while it is shown.
    pub fn candidates(&self) -> Option<&[Candidate]> {
        self.discovery.as_deref()
    }

    /// A found project already linked: shown disabled and unchecked ("Already linked").
    pub fn is_linked(&self, path: &str) -> bool {
        self.projects.iter().any(|p| p == path)
    }

    pub fn is_selected(&self, path: &str) -> bool {
        self.selected.iter().any(|p| p == path)
    }

    /// Add Selected is enabled.
    pub fn can_add_selected(&self) -> bool {
        !self.selected.is_empty()
    }

    /// Add (or Enter in the path field): links the folder once.
    pub fn add(&mut self) {
        let path = self.adding.trim().to_owned();
        if !path.is_empty() {
            push_new(&mut self.projects, [path]);
        }
        self.adding.clear();
    }

    pub fn remove(&mut self, path: &str) {
        self.projects.retain(|p| p != path);
    }

    /// Add Projects… picked `folders` (none: the picker was cancelled, nothing is searched).
    pub fn picked(&mut self, folders: Vec<String>, request: u64) -> DialogStep {
        if folders.is_empty() {
            return DialogStep::Stay;
        }
        self.searching = Some(request);
        self.error = None;
        self.truncated = false;
        DialogStep::Discover { request, folders }
    }

    /// The search `request` answered; an answer for a search that was cancelled is dropped.
    pub fn discovered(&mut self, request: u64, found: Result<ProjectDiscovery, IdeError>) {
        if self.searching != Some(request) {
            return;
        }
        self.searching = None;
        let found = match found {
            Ok(found) => found,
            Err(error) => {
                if error.kind != ErrorKind::Cancelled {
                    self.error = Some(error.message);
                }
                return;
            }
        };
        let before = self.projects.clone();
        let direct: Vec<String> = found.direct.iter().map(|path| to_ipc(path)).collect();
        push_new(&mut self.projects, direct.iter().cloned());
        self.truncated = found.truncated;
        if found.projects.is_empty() {
            return;
        }
        let candidates: Vec<Candidate> = found
            .projects
            .iter()
            .map(|p| Candidate { path: to_ipc(&p.path), root: to_ipc(&p.root), relative_path: p.relative_path.clone() })
            .collect();
        self.selected = candidates
            .iter()
            .map(|c| c.path.clone())
            .filter(|path| !before.contains(path) && !direct.contains(path))
            .collect();
        self.discovery = Some(candidates);
    }

    pub fn toggle(&mut self, path: &str, checked: bool) {
        self.selected.retain(|p| p != path);
        if checked {
            self.selected.push(path.to_owned());
        }
    }

    pub fn select_all(&mut self) {
        let all: Vec<String> = self.discovery.iter().flatten().map(|c| c.path.clone()).collect();
        self.selected = all.into_iter().filter(|path| !self.projects.contains(path)).collect();
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// Back: the checklist closes, nothing is added.
    pub fn back(&mut self) {
        self.discovery = None;
    }

    pub fn add_selected(&mut self) {
        let selected = std::mem::take(&mut self.selected);
        push_new(&mut self.projects, selected);
        self.discovery = None;
    }

    /// Cancel or Escape. While saving the dialog stays; a running search is cancelled.
    pub fn close(&mut self) -> Vec<DialogStep> {
        if self.busy {
            return vec![DialogStep::Stay];
        }
        let mut steps: Vec<DialogStep> = self.searching.take().map(DialogStep::CancelSearch).into_iter().collect();
        steps.push(DialogStep::Close);
        steps
    }

    /// Create / Apply, including a path typed in the field and not added yet.
    pub fn save(&mut self) -> DialogStep {
        if self.disabled() || self.discovery.is_some() {
            return DialogStep::Stay;
        }
        self.busy = true;
        self.error = None;
        let mut included = self.projects.clone();
        let adding = self.adding.trim();
        if !adding.is_empty() {
            push_new(&mut included, [adding.to_owned()]);
        }
        match self.mode {
            DialogMode::Create => DialogStep::Create {
                path: self.path.trim().to_owned(),
                name: self.name.trim().to_owned(),
                projects: included,
            },
            DialogMode::Manage => {
                self.pending_trust = included.iter().filter(|p| !self.members.contains(p)).cloned().collect();
                self.included = included;
                self.next_trust()
            }
        }
    }

    fn next_trust(&mut self) -> DialogStep {
        match self.pending_trust.first() {
            Some(path) => DialogStep::ConfirmTrust(path.clone()),
            None => {
                DialogStep::Update { name: self.name.trim().to_owned(), projects: std::mem::take(&mut self.included) }
            }
        }
    }

    fn failed(&mut self, error: IdeError) -> DialogStep {
        self.busy = false;
        if error.kind != ErrorKind::Cancelled {
            self.error = Some(error.message);
        }
        DialogStep::Stay
    }

    /// The workspace was created at the answered root, or creating it failed (the form stays).
    pub fn created(&mut self, root: Result<String, IdeError>) -> DialogStep {
        match root {
            Ok(root) => {
                self.busy = false;
                DialogStep::OpenWithPolicy(root)
            }
            Err(error) => self.failed(error),
        }
    }

    pub fn trust_confirmed(&mut self, result: Result<(), IdeError>) -> DialogStep {
        if let Err(error) = result {
            return self.failed(error);
        }
        match self.pending_trust.first().cloned() {
            Some(path) => DialogStep::ReadTrust(path),
            None => self.next_trust(),
        }
    }

    /// Whether the member is trusted now; a restricted one is not attached and the form stays.
    pub fn trust_read(&mut self, trusted: Result<bool, IdeError>, i18n: &mut I18n) -> DialogStep {
        match trusted {
            Ok(true) => {
                if !self.pending_trust.is_empty() {
                    self.pending_trust.remove(0);
                }
                self.next_trust()
            }
            Ok(false) => {
                let path = self.pending_trust.first().cloned().unwrap_or_default();
                self.busy = false;
                self.error = Some(restricted_text(&path, i18n));
                DialogStep::Stay
            }
            Err(error) => self.failed(error),
        }
    }

    pub fn updated(&mut self, result: Result<(), IdeError>) -> DialogStep {
        match result {
            Ok(()) => {
                self.busy = false;
                DialogStep::Close
            }
            Err(error) => self.failed(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_workspace::project_discovery::DiscoveredProject;

    fn s(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|&p| p.to_owned()).collect()
    }

    fn found(direct: &[&str], projects: &[(&str, &str)], truncated: bool) -> ProjectDiscovery {
        ProjectDiscovery {
            direct: direct.iter().map(|&p| p.into()).collect(),
            projects: projects
                .iter()
                .map(|&(path, root)| DiscoveredProject {
                    path: path.into(),
                    root: root.into(),
                    relative_path: path[root.len() + 1..].to_owned(),
                })
                .collect(),
            truncated,
        }
    }

    // merges folder selections and adds only checked projects in %s mode
    #[test]
    fn merges_folder_selections_and_adds_only_checked_projects() {
        for mode in [DialogMode::Create, DialogMode::Manage] {
            let manage = mode == DialogMode::Manage;
            let project = DialogProject {
                root: if manage { "/work/workspace" } else { "/work/one/linked" }.to_owned(),
                workspace: manage
                    .then(|| WorkspaceMembers { name: "Workspace".into(), projects: s(&["/work/one/linked"]) }),
            };
            let mut dialog = WorkspaceDialog::new(mode, Some(&project), false);
            let folders = s(&["/work/one", "/work/two", "/work/empty", "/work/one/direct"]);
            assert_eq!(dialog.picked(folders.clone(), 1), DialogStep::Discover { request: 1, folders });
            let answer = found(
                &["/work/empty", "/work/one/direct"],
                &[
                    ("/work/one/linked", "/work/one"),
                    ("/work/one/direct", "/work/one"),
                    ("/work/one/frontend", "/work/one"),
                    ("/work/two/group/backend", "/work/two"),
                ],
                false,
            );
            dialog.discovered(1, Ok(answer));
            for linked in ["/work/one/linked", "/work/one/direct"] {
                assert!(dialog.is_linked(linked) && !dialog.is_selected(linked), "{linked}");
            }
            assert!(dialog.is_selected("/work/one/frontend") && dialog.is_selected("/work/two/group/backend"));
            dialog.select_none();
            assert!(!dialog.can_add_selected());
            dialog.select_all();
            assert!(!dialog.is_selected("/work/one/linked"));
            dialog.toggle("/work/two/group/backend", false);
            dialog.add_selected();
            assert_eq!(dialog.projects().len(), 4);
            let included = s(&["/work/one/linked", "/work/empty", "/work/one/direct", "/work/one/frontend"]);
            let mut step = dialog.save();
            if manage {
                // The new members are confirmed as trusted first.
                for _ in 0..3 {
                    let DialogStep::ConfirmTrust(_) = step else { panic!("confirms trust: {step:?}") };
                    dialog.trust_confirmed(Ok(()));
                    step = dialog.trust_read(Ok(true), &mut I18n::default());
                }
                assert_eq!(step, DialogStep::Update { name: "Workspace".into(), projects: included });
                assert_eq!(dialog.updated(Ok(())), DialogStep::Close);
            } else {
                assert_eq!(
                    step,
                    DialogStep::Create {
                        path: "/work/one/workspace".into(),
                        name: "Workspace".into(),
                        projects: included
                    }
                );
                assert_eq!(
                    dialog.created(Ok("/work/workspace".into())),
                    DialogStep::OpenWithPolicy("/work/workspace".into())
                );
            }
        }
    }

    // adds direct and empty folders without a checklist and retains an entry-limit warning
    #[test]
    fn adds_direct_and_empty_folders_without_a_checklist_and_retains_an_entry_limit_warning() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        dialog.picked(s(&["/work/direct", "/work/empty"]), 1);
        dialog.discovered(1, Ok(found(&["/work/direct", "/work/empty"], &[], true)));
        assert_eq!(dialog.projects(), ["/work/direct", "/work/empty"]);
        assert!(dialog.candidates().is_none());
        assert!(dialog.truncated());
    }

    // returns from the checklist without adding unchecked projects
    #[test]
    fn returns_from_the_checklist_without_adding_unchecked_projects() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        dialog.picked(s(&["/work"]), 1);
        dialog.discovered(1, Ok(found(&[], &[("/work/app", "/work")], false)));
        assert!(dialog.candidates().is_some());
        dialog.back();
        assert!(dialog.projects().is_empty());
    }

    // cancels scanning locally on close and ignores late results
    #[test]
    fn cancels_scanning_locally_on_close_and_ignores_late_results() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        dialog.picked(s(&["/work"]), 7);
        assert!(dialog.is_searching() && dialog.disabled() && !dialog.can_save());
        assert_eq!(dialog.close(), [DialogStep::CancelSearch(7), DialogStep::Close]);
        dialog.discovered(7, Ok(found(&[], &[("/work/app", "/work")], false)));
        assert!(dialog.candidates().is_none());
    }

    // does not scan when the native picker is cancelled
    #[test]
    fn does_not_scan_when_the_native_picker_is_cancelled() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        assert_eq!(dialog.picked(Vec::new(), 1), DialogStep::Stay);
        assert!(!dialog.is_searching());
    }

    // creates a workspace from the pending project field without an extra Add click
    #[test]
    fn creates_a_workspace_from_the_pending_project_field_without_an_extra_add_click() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        dialog.path = "/work/full".into();
        dialog.adding = "/work/frontend".into();
        assert_eq!(
            dialog.save(),
            DialogStep::Create {
                path: "/work/full".into(),
                name: "Workspace".into(),
                projects: s(&["/work/frontend"])
            }
        );
        assert_eq!(dialog.created(Ok("/work/full".into())), DialogStep::OpenWithPolicy("/work/full".into()));
    }

    // uses the shared opening policy after creation from an already open project
    #[test]
    fn uses_the_shared_opening_policy_after_creation_from_an_already_open_project() {
        let project = DialogProject { root: "/work/frontend".into(), workspace: None };
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, Some(&project), false);
        dialog.path = "/work/full".into();
        assert!(matches!(dialog.save(), DialogStep::Create { .. }));
        assert_eq!(dialog.created(Ok("/work/full".into())), DialogStep::OpenWithPolicy("/work/full".into()));
    }

    // retains the form and linked folders when saving fails
    #[test]
    fn retains_the_form_and_linked_folders_when_saving_fails() {
        let mut dialog = WorkspaceDialog::new(DialogMode::Create, None, false);
        dialog.path = "/work/full".into();
        dialog.adding = "/work/missing".into();
        dialog.save();
        let failed = IdeError::new(ErrorKind::InvalidInput, "Folder does not exist");
        assert_eq!(dialog.created(Err(failed)), DialogStep::Stay);
        assert_eq!(dialog.error(), Some("Folder does not exist"));
        assert_eq!(dialog.adding, "/work/missing");
        assert_eq!(dialog.close(), [DialogStep::Close]);
    }

    // retains Manage Workspace when project trust is %s
    #[test]
    fn retains_manage_workspace_when_project_trust_is_restricted_or_cancelled() {
        for cancelled in [false, true] {
            let project = DialogProject {
                root: "/work/primary".into(),
                workspace: Some(WorkspaceMembers { name: "primary".into(), projects: s(&["/work/primary"]) }),
            };
            let mut dialog = WorkspaceDialog::new(DialogMode::Manage, Some(&project), false);
            dialog.adding = "/work/restricted".into();
            assert_eq!(dialog.save(), DialogStep::ConfirmTrust("/work/restricted".into()));
            let step = if cancelled {
                dialog.trust_confirmed(Err(IdeError::cancelled()))
            } else {
                assert_eq!(dialog.trust_confirmed(Ok(())), DialogStep::ReadTrust("/work/restricted".into()));
                dialog.trust_read(Ok(false), &mut I18n::default())
            };
            assert_eq!(step, DialogStep::Stay);
            assert!(dialog.can_save());
            let restricted = "Cannot attach /work/restricted because it is restricted. Open it separately and choose \
                              Trust Project, then attach it again.";
            assert_eq!(dialog.error(), (!cancelled).then_some(restricted));
            assert_eq!(dialog.adding, "/work/restricted");
        }
    }
}
