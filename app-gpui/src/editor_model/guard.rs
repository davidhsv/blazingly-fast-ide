//! Who may open and close editor tabs, and when: the editor's generations (project, close
//! admission, open sequence, activation), its close decision for the project or window, the
//! custom documents frozen while a close is decided, and the serial queue that every tab removal
//! and close preparation runs through.
//!
//! The guard never awaits anything. An open takes an [`OpenRequest`] ticket when it starts and
//! hands it back at every later step ([`EditorGuard::check_opening`], [`EditorGuard::admit`]); a
//! ticket that is no longer current is refused or admitted without taking the keyboard. Every
//! document a decision stops freezing is queued for its `cancel close` in
//! [`EditorGuard::take_resumed`], which the caller drains after each call.

use super::documents::{DocumentKinds, custom_documents};
use super::removal::{Removal, RemovalEnd};
use crate::editor::tabs::{EditorInput, OpenTabOptions, SplitDir, TabsState};
use crate::platform::virtual_docs::basename;
use ide_core::editor_settings::EditorTabsSettings;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Why an editor operation was refused. The messages are the ones logged and reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorError {
    /// The project was replaced after the open started.
    ProjectChanged,
    /// A close decision is pending or being prepared.
    Closing,
    /// Another close decision is still pending.
    AlreadyPending,
    /// The close session was settled, cancelled or outlived by its project.
    NotCurrent,
    /// The project was replaced while the documents were prepared.
    ProjectChangedDuringClose,
    /// The close decision was cancelled while the documents were prepared.
    Cancelled,
    /// A document operation failed.
    Document(String),
}

impl EditorError {
    pub fn message(&self) -> String {
        match self {
            EditorError::ProjectChanged => "The project changed while opening the editor".to_owned(),
            EditorError::Closing => "The project is closing".to_owned(),
            EditorError::AlreadyPending => "An editor close decision is already pending".to_owned(),
            EditorError::NotCurrent => "The editor close decision is no longer current".to_owned(),
            EditorError::ProjectChangedDuringClose => "The project changed during close".to_owned(),
            EditorError::Cancelled => "The editor close decision was cancelled".to_owned(),
            EditorError::Document(message) => message.clone(),
        }
    }
}

/// How an input is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenOptions {
    /// Open in the pane's preview slot (when previews are enabled).
    pub preview: bool,
    /// Open in the pane beside the target pane, splitting when there is none.
    pub side: bool,
    /// The pane to open in; the active pane when unset or gone.
    pub pane: Option<String>,
    /// The open places the caret ([`EditorGuard::selection_applies`]).
    pub selection: bool,
    /// The editor takes the keyboard once it is ready ([`EditorGuard::can_focus`]).
    pub focus: bool,
}

impl Default for OpenOptions {
    fn default() -> Self {
        Self { preview: false, side: false, pane: None, selection: false, focus: true }
    }
}

/// The ticket of one open, taken when it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    project: u64,
    admission: u64,
    pub sequence: u64,
    activation: u64,
    /// The pane the open targets: the one asked for, else the pane active when it started.
    pub pane: String,
}

/// What [`EditorGuard::admit`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// The input's document is still being released: admit again after [`EditorGuard::released`].
    WaitForRelease,
    /// A superseded preview, or a pane that is gone: nothing opened. A provider's resolution is
    /// rolled back, never committed.
    Dropped,
    /// The tab is open. `activated`: the open was still the newest and took the active tab.
    /// `keep` is the tab the pane's tab limit spares ([`Removal::tab_limit`]).
    Opened { pane: String, tab: String, activated: bool, keep: String },
}

/// The start of a close decision: the prepared documents of a session ready to be decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloseSession {
    pub id: u64,
    /// How many of the prepared documents have unsaved changes.
    pub dirty: usize,
}

/// One preparation of every open custom document for a close decision.
#[derive(Debug, Clone, PartialEq)]
pub struct Preparation {
    id: u64,
    generation: u64,
    documents: Vec<EditorInput>,
    next: usize,
}

/// What the caller does next while preparing a close.
#[derive(Debug, Clone, PartialEq)]
pub enum PrepareStep {
    /// Prepare the document (stop editing and executing, flush pending changes), then report
    /// [`EditorGuard::document_prepared`].
    Prepare(EditorInput),
    /// Every document is prepared; the session is pending until committed or cancelled.
    Ready(CloseSession),
    /// The preparation failed; the documents it froze are in [`EditorGuard::take_resumed`].
    Failed(EditorError),
}

/// A job of the serial queue.
#[derive(Debug, Clone, PartialEq)]
pub enum Job {
    Removal(Box<Removal>),
    /// Start [`EditorGuard::start_preparation`].
    PrepareClose,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SessionState {
    pub(super) id: u64,
    generation: u64,
    committed: bool,
    documents: Vec<EditorInput>,
    prepared: Vec<EditorInput>,
}

/// The editor's open/close bookkeeping for one window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EditorGuard {
    pub(super) project: u64,
    admission: u64,
    sequence: u64,
    activation: u64,
    close_requests: usize,
    pub(super) session: Option<SessionState>,
    next_session: u64,
    /// Keys of the documents frozen by a removal or a close session.
    pub(super) prepared: BTreeSet<String>,
    /// Keys of the documents being released, with how many releases are pending for each.
    releasing: BTreeMap<String, usize>,
    opening_project: bool,
    restoring: bool,
    last_active: Option<(String, Option<String>)>,
    /// The newest open that asked for a selection, per tab id.
    selection_requests: BTreeMap<String, u64>,
    pub(super) resumed: Vec<EditorInput>,
    running: bool,
    queue: VecDeque<Job>,
}

fn active_of(tabs: &TabsState) -> (String, Option<String>) {
    (tabs.active_pane.clone(), tabs.pane(&tabs.active_pane).and_then(|pane| pane.active.clone()))
}

impl EditorGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// The documents whose close was decided against: the caller cancels their close (they
    /// resume editing and executing).
    pub fn take_resumed(&mut self) -> Vec<EditorInput> {
        std::mem::take(&mut self.resumed)
    }

    /// Whether `key`'s document is frozen for a close decision (no autosave touches it).
    pub fn is_prepared(&self, key: &str) -> bool {
        self.prepared.contains(key)
    }

    /// Whether a close decision is pending: autosave and disk synchronization wait.
    pub fn close_pending(&self) -> bool {
        self.session.is_some()
    }

    // --- serial queue -------------------------------------------------------------------------

    /// Queues `job`; returns it when the queue is idle and the caller runs it now.
    pub fn enqueue(&mut self, job: Job) -> Option<Job> {
        if self.running {
            self.queue.push_back(job);
            return None;
        }
        self.running = true;
        Some(job)
    }

    /// The running job finished; returns the next one to run.
    pub fn job_finished(&mut self) -> Option<Job> {
        let next = self.queue.pop_front();
        self.running = next.is_some();
        next
    }

    /// A removal ended and the caller applied its tabs and ran its tab-change reaction (which
    /// must not autosave a prepared document): the frozen markers go, the released documents are
    /// noted until [`EditorGuard::released`], and the next job is returned.
    pub fn finish_removal(&mut self, end: &RemovalEnd) -> Option<Job> {
        for key in &end.prepared {
            self.prepared.remove(key);
        }
        for input in &end.release {
            *self.releasing.entry(input.key.clone()).or_default() += 1;
        }
        self.job_finished()
    }

    /// One release of `key`'s document finished (or failed).
    pub fn released(&mut self, key: &str) {
        if let Some(count) = self.releasing.get_mut(key) {
            *count -= 1;
            if *count == 0 {
                self.releasing.remove(key);
            }
        }
    }

    // --- opening ------------------------------------------------------------------------------

    /// Starts an open, taking its ticket; refused while the project is closing.
    pub fn begin_open(&mut self, tabs: &TabsState, opts: &OpenOptions) -> Result<OpenRequest, EditorError> {
        self.sequence += 1;
        let pane = match &opts.pane {
            Some(pane) if tabs.pane(pane).is_some() => pane.clone(),
            _ => tabs.active_pane.clone(),
        };
        let request = OpenRequest {
            project: self.project,
            admission: self.admission,
            sequence: self.sequence,
            activation: self.activation,
            pane,
        };
        self.check_opening(&request)?;
        Ok(request)
    }

    /// Whether the open may go on after an await: the project is the same and no close came.
    /// A decision to leave that was authorized, but whose project change never came (the window
    /// stayed with its project), is released here, so the window opens files and can be left
    /// again.
    pub fn check_opening(&mut self, request: &OpenRequest) -> Result<(), EditorError> {
        if request.project != self.project {
            return Err(EditorError::ProjectChanged);
        }
        self.release_stale_close();
        if request.admission != self.admission || self.session.is_some() || self.close_requests > 0 {
            return Err(EditorError::Closing);
        }
        Ok(())
    }

    /// Whether `request` is still the newest open and nothing was activated since it started.
    pub fn is_current(&self, request: &OpenRequest) -> bool {
        request.project == self.project
            && request.admission == self.admission
            && request.sequence == self.sequence
            && request.activation == self.activation
    }

    /// Opens `input` in `tabs` for `request`. A newer open or a manual activation since it
    /// started keeps the tab it made active: this one then opens in the background, or not at
    /// all when it is a preview. Custom documents never open as previews.
    pub fn admit(
        &mut self,
        request: &OpenRequest,
        input: EditorInput,
        opts: &OpenOptions,
        tabs: &mut TabsState,
        prefs: &EditorTabsSettings,
        kinds: &DocumentKinds,
    ) -> Result<Admission, EditorError> {
        self.check_opening(request)?;
        if self.releasing.contains_key(&input.key) {
            return Ok(Admission::WaitForRelease);
        }
        let activate = self.is_current(request);
        let lifecycle = kinds.has_lifecycle(&input);
        let preview = opts.preview && prefs.preview_tab && !lifecycle;
        if !activate && preview {
            return Ok(Admission::Dropped);
        }
        let mut pane = if tabs.pane(&request.pane).is_some() {
            request.pane.clone()
        } else if activate {
            tabs.active_pane.clone()
        } else {
            return Ok(Admission::Dropped);
        };
        let mut next = tabs.clone();
        if opts.side {
            if let Some(right) = next.neighbour_pane(&pane) {
                pane = right.to_owned();
            } else if let Some(split) = next.split_pane(&pane, SplitDir::Row) {
                // The new pane starts with only the requested input.
                if let Some(created) = next.panes.iter_mut().find(|candidate| candidate.id == split) {
                    created.tabs.clear();
                    created.active = None;
                    created.mru.clear();
                }
                if !activate {
                    next.active_pane.clone_from(&tabs.active_pane);
                }
                pane = split;
            }
        }
        let found = next.find_tab(&input.key, Some(&pane)).map(|(at, tab)| (at.to_owned(), tab.clone()));
        let tab_id = match found {
            Some((at, tab)) if !(opts.side || opts.pane.is_some()) || at == pane => {
                pane = at;
                if activate {
                    next.activate_tab(&pane, &tab.id);
                }
                if !opts.preview || lifecycle {
                    next.promote_tab(&pane, &tab.id);
                }
                if activate && input.data.is_some() && tab.input.data != input.data {
                    let reopen = OpenTabOptions { preview: opts.preview && !lifecycle, activate, ..Default::default() };
                    next.open_tab(&pane, input, reopen);
                }
                tab.id
            }
            _ => {
                let index = prefs.open_at_end.then(|| next.pane(&pane).map_or(0, |at| at.tabs.len()));
                let open = OpenTabOptions { preview, activate, preserve_preview: !activate, index, pinned: false };
                let Some(id) = next.open_tab(&pane, input, open) else { return Ok(Admission::Dropped) };
                if prefs.sort_alphabetically {
                    next.sort_tabs(&pane, |tab| {
                        basename(tab.input.path.as_deref().unwrap_or(&tab.input.key)).into_owned()
                    });
                }
                id
            }
        };
        if opts.selection {
            let newest = self.selection_requests.entry(tab_id.clone()).or_default();
            *newest = (*newest).max(request.sequence);
        }
        *tabs = next;
        self.last_active = Some(active_of(tabs));
        let keep = if activate {
            tab_id.clone()
        } else {
            tabs.pane(&pane).and_then(|at| at.active.clone()).unwrap_or_else(|| tab_id.clone())
        };
        Ok(Admission::Opened { pane, tab: tab_id, activated: activate, keep })
    }

    /// Whether the selection `request` asked for is still the one to place in `tab`: no newer
    /// open asked for another and nothing was activated since.
    pub fn selection_applies(&self, request: &OpenRequest, tab: &str) -> bool {
        self.selection_requests.get(tab) == Some(&request.sequence) && request.activation == self.activation
    }

    /// Whether `tab`'s editor, now ready, takes the keyboard for `request`: it is the newest open
    /// and its tab is still the active one.
    pub fn can_focus(&self, request: &OpenRequest, tab: &str, tabs: &TabsState) -> bool {
        self.is_current(request) && tabs.pane(&tabs.active_pane).and_then(|pane| pane.active.as_deref()) == Some(tab)
    }

    /// The user activated a tab: opens still pending no longer take the active tab.
    pub fn activated(&mut self) {
        self.activation += 1;
    }

    /// The tabs changed (after any transition): a change of the active tab that no admission or
    /// session restore made counts as an activation; requests of closed tabs are forgotten.
    pub fn note_tabs(&mut self, tabs: &TabsState) {
        let active = active_of(tabs);
        if !self.restoring && self.last_active.as_ref() != Some(&active) {
            self.activation += 1;
        }
        self.last_active = Some(active);
        let live: BTreeSet<&str> = tabs.panes.iter().flat_map(|pane| &pane.tabs).map(|tab| tab.id.as_str()).collect();
        self.selection_requests.retain(|id, _| live.contains(id.as_str()));
    }

    /// The project's saved session is being merged into the tabs: that is no user activation.
    pub fn set_restoring(&mut self, restoring: bool) {
        self.restoring = restoring;
    }

    // --- the project ----------------------------------------------------------------------------

    /// The window started or finished opening a project. A failed open returns to the same
    /// project: its authorized close decision is released.
    pub fn set_opening_project(&mut self, opening: bool) {
        let failed = self.opening_project && !opening;
        self.opening_project = opening;
        if failed && self.session.as_ref().is_some_and(|session| session.committed) {
            self.cancel_close();
        }
    }

    /// The window's project was replaced (another root, LightEdit, or a reopened project): the
    /// caller resets the tabs and releases the returned documents without prompting or
    /// autosaving, then reports each with [`EditorGuard::released`]. Pending opens and decisions
    /// of the old project are refused from now on.
    pub fn project_replaced(&mut self, tabs: &TabsState, kinds: &DocumentKinds) -> Vec<EditorInput> {
        self.project += 1;
        self.session = None;
        self.prepared.clear();
        self.selection_requests.clear();
        self.last_active = None;
        let documents = custom_documents(tabs, kinds);
        for input in &documents {
            *self.releasing.entry(input.key.clone()).or_default() += 1;
        }
        documents
    }

    // --- the close decision ---------------------------------------------------------------------

    /// Asks for a close decision: opens are refused from now on, and the returned job (when the
    /// queue is idle) starts [`EditorGuard::start_preparation`].
    pub fn request_close(&mut self) -> Option<Job> {
        self.admission += 1;
        self.close_requests += 1;
        self.enqueue(Job::PrepareClose)
    }

    fn release_stale_close(&mut self) {
        if !self.opening_project && self.session.as_ref().is_some_and(|session| session.committed) {
            self.cancel_close();
        }
    }

    /// Holds every open custom document still until the project or window decision.
    pub fn start_preparation(
        &mut self,
        tabs: &TabsState,
        kinds: &DocumentKinds,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> (Option<Preparation>, PrepareStep) {
        self.release_stale_close();
        if self.session.is_some() {
            self.close_requests -= 1;
            return (None, PrepareStep::Failed(EditorError::AlreadyPending));
        }
        self.next_session += 1;
        let documents = custom_documents(tabs, kinds);
        self.session = Some(SessionState {
            id: self.next_session,
            generation: self.project,
            committed: false,
            documents: documents.clone(),
            prepared: Vec::new(),
        });
        let mut preparation = Preparation { id: self.next_session, generation: self.project, documents, next: 0 };
        let step = self.prepare_next(&mut preparation, dirty);
        (Some(preparation), step)
    }

    fn prepare_next(&mut self, preparation: &mut Preparation, dirty: &dyn Fn(&EditorInput) -> bool) -> PrepareStep {
        let Some(input) = preparation.documents.get(preparation.next).cloned() else {
            self.close_requests -= 1;
            let dirty = preparation.documents.iter().filter(|input| dirty(input)).count();
            return PrepareStep::Ready(CloseSession { id: preparation.id, dirty });
        };
        preparation.next += 1;
        if let Some(session) = self.session.as_mut() {
            session.prepared.push(input.clone());
        }
        self.prepared.insert(input.key.clone());
        PrepareStep::Prepare(input)
    }

    /// The document of the last [`PrepareStep::Prepare`] was prepared, or preparing it failed.
    pub fn document_prepared(
        &mut self,
        preparation: &mut Preparation,
        result: Result<(), String>,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> PrepareStep {
        let error = if let Err(message) = result {
            Some(EditorError::Document(message))
        } else if preparation.generation != self.project {
            Some(EditorError::ProjectChangedDuringClose)
        } else if self.session.as_ref().map(|session| session.id) != Some(preparation.id) {
            if let Some(input) = preparation.next.checked_sub(1).and_then(|at| preparation.documents.get(at)) {
                self.resumed.push(input.clone());
            }
            Some(EditorError::Cancelled)
        } else {
            None
        };
        match error {
            Some(error) => {
                self.cancel_session(preparation.id);
                self.close_requests -= 1;
                PrepareStep::Failed(error)
            }
            None => self.prepare_next(preparation, dirty),
        }
    }

    /// Cancels session `id` when it is the current one: its documents resume.
    pub fn cancel_session(&mut self, id: u64) {
        if self.session.as_ref().map(|session| session.id) != Some(id) {
            return;
        }
        if let Some(session) = self.session.take() {
            for input in session.prepared {
                self.prepared.remove(&input.key);
                self.resumed.push(input);
            }
        }
    }

    /// Cancels whatever close decision is pending.
    pub fn cancel_close(&mut self) {
        if let Some(id) = self.session.as_ref().map(|session| session.id) {
            self.cancel_session(id);
        }
    }

    fn current_session(&self, id: u64) -> Result<&SessionState, EditorError> {
        self.session
            .as_ref()
            .filter(|session| session.id == id && session.generation == self.project)
            .ok_or(EditorError::NotCurrent)
    }

    /// The close goes ahead: the documents stay frozen until the project changes (or the window
    /// stays with it, [`EditorGuard::check_opening`]).
    pub fn commit_session(&mut self, id: u64) -> Result<(), EditorError> {
        self.current_session(id)?;
        if let Some(session) = self.session.as_mut() {
            session.committed = true;
        }
        Ok(())
    }

    /// The documents of session `id` whose unsaved changes the caller discards.
    pub fn discard_session(
        &self,
        id: u64,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> Result<Vec<EditorInput>, EditorError> {
        let session = self.current_session(id)?;
        Ok(session.documents.iter().filter(|input| dirty(input)).cloned().collect())
    }

    // --- saving ---------------------------------------------------------------------------------

    /// Save All: every custom document with unsaved changes, once however many tabs show it. The
    /// caller saves each and reports the failures together after trying all of them.
    pub fn save_targets(
        &self,
        tabs: &TabsState,
        kinds: &DocumentKinds,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> Vec<EditorInput> {
        custom_documents(tabs, kinds).into_iter().filter(|input| dirty(input)).collect()
    }

    /// Autosave when the window or the application loses focus: nothing while a close decision is
    /// pending, and never a document frozen for one.
    pub fn blur_autosave(
        &self,
        tabs: &TabsState,
        kinds: &DocumentKinds,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> Vec<EditorInput> {
        if self.session.is_some() {
            return Vec::new();
        }
        let mut targets = self.save_targets(tabs, kinds, dirty);
        targets.retain(|input| !self.prepared.contains(&input.key));
        targets
    }

    /// Autosave of the custom documents whose tab stopped being the active one of its pane:
    /// only a document that remains open (a closed or discarded one is never saved by the tab
    /// removal that follows its decision), not frozen, and with unsaved changes.
    pub fn switch_autosave(
        &self,
        before: &TabsState,
        after: &TabsState,
        kinds: &DocumentKinds,
        dirty: &dyn Fn(&EditorInput) -> bool,
    ) -> Vec<EditorInput> {
        if self.session.is_some() {
            return Vec::new();
        }
        let open = after.open_keys();
        let mut saved = BTreeSet::new();
        let mut out = Vec::new();
        for pane in &before.panes {
            let Some(active) = &pane.active else { continue };
            let Some(now) = after.pane(&pane.id) else { continue };
            if now.active.as_ref() == Some(active) {
                continue;
            }
            let Some(tab) = pane.tab(active) else { continue };
            let input = &tab.input;
            if open.contains(input.key.as_str())
                && !self.prepared.contains(&input.key)
                && kinds.has_lifecycle(input)
                && saved.insert(input.key.clone())
                && dirty(input)
            {
                out.push(input.clone());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::tabs::{LayoutNode, Pane, Tab};
    use crate::editor_model::removal::tests::Ide;

    fn file(path: &str) -> EditorInput {
        EditorInput { kind: "file".into(), key: path.into(), path: Some(path.into()), data: None }
    }

    fn background() -> OpenOptions {
        OpenOptions { focus: false, ..OpenOptions::default() }
    }

    fn preview() -> OpenOptions {
        OpenOptions { preview: true, ..background() }
    }

    fn open(ide: &mut Ide, path: &str, opts: OpenOptions) -> OpenRequest {
        let request = ide.begin(&opts);
        ide.admit_request(&request, file(path), &opts).expect("admitted");
        request
    }

    fn tab_of<'a>(ide: &'a Ide, path: &str) -> &'a Tab {
        ide.tabs.find_tab(path, None).map(|(_, tab)| tab).expect("an open tab")
    }

    fn sorted(mut paths: Vec<String>) -> Vec<String> {
        paths.sort();
        paths
    }

    #[test]
    fn keeps_the_caret_and_activation_of_a_file_opened_while_the_projects_session_restores() {
        let mut ide = Ide::new();
        let selection = OpenOptions { selection: true, ..background() };
        let request = open(&mut ide, "/p/a.ts", selection);
        let saved = Tab { id: "saved-b".into(), input: file("/p/b.ts"), preview: false, pinned: false };
        let restored = TabsState::from_parts(
            LayoutNode::Pane { pane_id: "saved-pane".into() },
            vec![Pane {
                id: "saved-pane".into(),
                tabs: vec![saved],
                active: Some("saved-b".into()),
                mru: vec!["saved-b".into()],
            }],
            "saved-pane".into(),
        );
        ide.guard.set_restoring(true);
        ide.tabs = TabsState::merge_session_tabs(&ide.tabs, restored);
        ide.guard.note_tabs(&ide.tabs);
        ide.guard.set_restoring(false);
        assert_eq!(sorted(ide.open_paths()), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(ide.active_path().as_deref(), Some("/p/a.ts"));
        let tab = tab_of(&ide, "/p/a.ts").id.clone();
        assert!(ide.guard.selection_applies(&request, &tab));
    }

    #[test]
    fn keeps_the_newer_file_active_after_an_earlier_probe() {
        for limit in [1, 30] {
            let mut ide = Ide::new();
            ide.prefs.tab_limit = limit;
            let first = ide.begin(&background());
            open(&mut ide, "/p/b.ts", background());
            assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
            let admitted = ide.admit_request(&first, file("/p/a.ts"), &background()).expect("admitted");
            assert!(matches!(admitted, Admission::Opened { activated: false, .. }));
            let expected: &[&str] = if limit == 1 { &["/p/b.ts"] } else { &["/p/a.ts", "/p/b.ts"] };
            assert_eq!(sorted(ide.open_paths()), expected, "limit {limit}");
            assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"), "limit {limit}");
        }
    }

    #[test]
    fn activates_the_newer_file_after_an_earlier_probe_finishes_while_both_are_pending() {
        let mut ide = Ide::new();
        let first = ide.begin(&background());
        let second = ide.begin(&background());
        ide.admit_request(&first, file("/p/a.ts"), &background()).expect("admitted");
        ide.admit_request(&second, file("/p/b.ts"), &background()).expect("admitted");
        assert_eq!(sorted(ide.open_paths()), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
    }

    #[test]
    fn discards_an_earlier_preview_without_replacing_the_newer_preview() {
        let mut ide = Ide::new();
        let first = ide.begin(&preview());
        open(&mut ide, "/p/b.ts", preview());
        let admitted = ide.admit_request(&first, file("/p/a.ts"), &preview());
        assert_eq!(admitted, Ok(Admission::Dropped));
        assert_eq!(ide.open_paths(), ["/p/b.ts"]);
        assert!(tab_of(&ide, "/p/b.ts").preview);
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
    }

    #[test]
    fn keeps_an_earlier_regular_file_without_removing_the_newer_preview() {
        let mut ide = Ide::new();
        let first = ide.begin(&background());
        open(&mut ide, "/p/b.ts", preview());
        ide.admit_request(&first, file("/p/a.ts"), &background()).expect("admitted");
        assert_eq!(sorted(ide.open_paths()), ["/p/a.ts", "/p/b.ts"]);
        assert!(!tab_of(&ide, "/p/a.ts").preview);
        assert!(tab_of(&ide, "/p/b.ts").preview);
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
    }

    #[test]
    fn keeps_manual_tab_activation_while_an_earlier_probe_is_pending() {
        let mut ide = Ide::new();
        open(&mut ide, "/p/b.ts", background());
        open(&mut ide, "/p/c.ts", background());
        let first = ide.begin(&background());
        let pane = ide.active_pane();
        let b = tab_of(&ide, "/p/b.ts").id.clone();
        ide.tabs.activate_tab(&pane, &b);
        ide.guard.activated();
        ide.admit_request(&first, file("/p/a.ts"), &background()).expect("admitted");
        assert_eq!(sorted(ide.open_paths()), ["/p/a.ts", "/p/b.ts", "/p/c.ts"]);
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
    }

    #[test]
    fn keeps_a_newer_explicit_open_regular_when_the_same_files_preview_resolves_late() {
        let mut ide = Ide::new();
        let late = ide.begin(&preview());
        open(&mut ide, "/p/a.ts", background());
        assert_eq!(ide.admit_request(&late, file("/p/a.ts"), &preview()), Ok(Admission::Dropped));
        assert_eq!(ide.open_paths(), ["/p/a.ts"]);
        assert!(!tab_of(&ide, "/p/a.ts").preview);
        assert_eq!(ide.active_path().as_deref(), Some("/p/a.ts"));
    }

    #[test]
    fn promotes_an_admitted_preview_when_the_same_file_is_opened_explicitly() {
        let mut ide = Ide::new();
        open(&mut ide, "/p/a.ts", preview());
        let id = tab_of(&ide, "/p/a.ts").id.clone();
        assert!(tab_of(&ide, "/p/a.ts").preview);
        open(&mut ide, "/p/a.ts", background());
        assert_eq!(ide.open_paths(), ["/p/a.ts"]);
        let tab = tab_of(&ide, "/p/a.ts");
        assert_eq!((tab.id.as_str(), tab.preview), (id.as_str(), false));
        assert_eq!(ide.active_path().as_deref(), Some("/p/a.ts"));
    }

    #[test]
    fn keeps_the_newer_selection_when_an_earlier_open_of_the_same_file_resolves_late() {
        let mut ide = Ide::new();
        let selection = OpenOptions { selection: true, ..background() };
        let first = ide.begin(&selection);
        let second = open(&mut ide, "/p/a.ts", selection.clone());
        ide.admit_request(&first, file("/p/a.ts"), &selection).expect("admitted");
        let tab = tab_of(&ide, "/p/a.ts").id.clone();
        assert!(ide.guard.selection_applies(&second, &tab));
        assert!(!ide.guard.selection_applies(&first, &tab));
    }

    #[test]
    fn keeps_a_pending_navigation_selection_after_a_plain_open() {
        for same in [false, true] {
            let mut ide = Ide::new();
            let navigation = open(&mut ide, "/p/a.ts", OpenOptions { selection: true, ..OpenOptions::default() });
            open(&mut ide, if same { "/p/a.ts" } else { "/p/b.ts" }, background());
            // The navigation's document finished loading only now.
            let tab = tab_of(&ide, "/p/a.ts").id.clone();
            assert!(ide.guard.selection_applies(&navigation, &tab), "same file: {same}");
            assert!(!ide.guard.can_focus(&navigation, &tab, &ide.tabs), "same file: {same}");
            let active = if same { "/p/a.ts" } else { "/p/b.ts" };
            assert_eq!(ide.active_path().as_deref(), Some(active), "same file: {same}");
        }
    }

    #[test]
    fn keeps_a_delayed_open_in_its_origin_pane() {
        for explicit in [true, false] {
            let mut ide = Ide::new();
            open(&mut ide, "/p/b.ts", background());
            let origin = ide.active_pane();
            let other = ide.split(&origin);
            assert_ne!(other, origin);
            let b = ide.tab_id("/p/b.ts", Some(origin.as_str()));
            ide.tabs.activate_tab(&origin, &b);
            ide.guard.activated();
            let opts = OpenOptions { pane: explicit.then(|| origin.clone()), ..background() };
            let first = ide.begin(&opts);
            open(&mut ide, "/p/c.ts", OpenOptions { pane: Some(other.clone()), ..background() });
            ide.admit_request(&first, file("/p/a.ts"), &opts).expect("admitted");
            let holds = |pane: &str| {
                ide.tabs.pane(pane).is_some_and(|pane| pane.tabs.iter().any(|tab| tab.input.key == "/p/a.ts"))
            };
            assert!(holds(&origin), "explicit: {explicit}");
            assert!(!holds(&other), "explicit: {explicit}");
            assert_eq!(ide.active_path().as_deref(), Some("/p/c.ts"), "explicit: {explicit}");
        }
    }

    #[test]
    fn does_not_request_focus_when_an_earlier_document_finishes_loading_late() {
        let mut ide = Ide::new();
        let first = open(&mut ide, "/p/a.ts", OpenOptions::default());
        open(&mut ide, "/p/b.ts", background());
        assert_eq!(sorted(ide.open_paths()), ["/p/a.ts", "/p/b.ts"]);
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
        let tab = tab_of(&ide, "/p/a.ts").id.clone();
        assert!(!ide.guard.can_focus(&first, &tab, &ide.tabs));
    }

    #[test]
    fn rolls_back_a_superseded_custom_preview_before_provider_admission() {
        let mut ide = Ide::new();
        let first = ide.begin(&preview());
        open(&mut ide, "/p/b.ts", preview());
        let image =
            EditorInput { kind: "image".into(), key: "image:/p/a.ts".into(), path: Some("/p/a.ts".into()), data: None };
        // Dropped: the provider's resolution is rolled back and never committed.
        assert_eq!(ide.admit_request(&first, image, &preview()), Ok(Admission::Dropped));
        assert_eq!(ide.active_path().as_deref(), Some("/p/b.ts"));
        assert!(ide.tabs.find_tab("image:/p/a.ts", None).is_none());
    }
}
