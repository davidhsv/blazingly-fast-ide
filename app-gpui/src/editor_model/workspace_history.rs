//! Refactoring (workspace edit) history in the editors' undo stacks. A workspace edit puts one
//! marker step into the history of every editor showing an affected file, never its text
//! patches; undoing or redoing that step in any of them (or from the refactoring notification)
//! undoes or redoes the whole edit in the backend and moves the marker in every other editor.
//! An editor with newer typing on top of the marker refuses until that typing is undone.
//!
//! Each editor's history is modelled as its done and undone steps; the caller mirrors them in
//! the editor's real history. The backend request is handed out as a ticket
//! ([`WorkspaceHistory::run`]) and reported back ([`WorkspaceHistory::finished`]).

use std::collections::BTreeMap;

/// How many workspace edits stay undoable.
const MAX_RECORDS: usize = 20;

/// One step of an editor's history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The editor's own change.
    Local,
    /// A workspace edit.
    Marker(u64),
}

/// Why a refactoring history operation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HistoryError {
    InProgress,
    ProjectClosed,
    NotUndoable,
    NewerChanges,
    /// The backend refused (out of sync, external write).
    Backend(String),
}

impl HistoryError {
    pub fn message(&self) -> String {
        match self {
            HistoryError::InProgress => "A refactoring history operation is in progress".to_owned(),
            HistoryError::ProjectClosed => "The project for this refactoring is no longer open".to_owned(),
            HistoryError::NotUndoable => "The refactoring can no longer be undone".to_owned(),
            HistoryError::NewerChanges => "Undo newer editor changes before undoing this refactoring".to_owned(),
            HistoryError::Backend(message) => message.clone(),
        }
    }
}

/// The backend request of a running operation: `undoWorkspaceEdit(id)` or, `forward`,
/// `redoWorkspaceEdit(id)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRequest {
    pub id: u64,
    pub forward: bool,
}

/// What an editor's Undo or Redo did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorStep {
    /// Nothing to undo or redo.
    Nothing,
    /// The editor's own change moved.
    Local,
    /// The editor's marker moved and the whole refactoring goes with it.
    Workspace(HistoryRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Record {
    paths: Vec<String>,
    undone: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    path: String,
    done: Vec<Step>,
    undone: Vec<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Running {
    request: HistoryRequest,
    source: Option<u64>,
    targets: Vec<u64>,
}

/// The refactoring history of one window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkspaceHistory {
    records: Vec<(u64, Record)>,
    targets: BTreeMap<u64, Target>,
    next_target: u64,
    project: Option<String>,
    running: Option<Running>,
}

fn affects(paths: &[String], path: &str) -> bool {
    paths
        .iter()
        .any(|root| path.strip_prefix(root.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('/')))
}

/// The side a step leaves and the side it goes to.
fn sides(target: &mut Target, forward: bool) -> (&mut Vec<Step>, &mut Vec<Step>) {
    if forward { (&mut target.undone, &mut target.done) } else { (&mut target.done, &mut target.undone) }
}

impl WorkspaceHistory {
    pub fn new(project: Option<String>) -> Self {
        Self { project, ..Self::default() }
    }

    /// Whether an operation runs: the editors refuse Undo, Redo and typing meanwhile.
    pub fn is_busy(&self) -> bool {
        self.running.is_some()
    }

    /// How many steps editor `target` can undo and redo.
    pub fn depths(&self, target: u64) -> (usize, usize) {
        self.targets.get(&target).map_or((0, 0), |target| (target.done.len(), target.undone.len()))
    }

    /// Whether workspace edit `id` is undone, for the notification's Undo / Redo action.
    pub fn is_undone(&self, id: u64) -> Option<bool> {
        self.record(id).map(|record| record.undone)
    }

    fn record(&self, id: u64) -> Option<&Record> {
        self.records.iter().find(|(key, _)| *key == id).map(|(_, record)| record)
    }

    fn mark(target: &mut Target, id: u64) {
        target.done.push(Step::Marker(id));
        target.undone.clear();
    }

    /// The backend applied workspace edit `id` to `paths` in `project`. It invalidates redo for
    /// any undone edit it overlaps, and marks every editor of an affected file.
    pub fn applied(&mut self, id: u64, paths: Vec<String>, project: Option<&str>) {
        if self.project.as_deref() != project {
            self.records.clear();
            self.project = project.map(str::to_owned);
        }
        self.records.retain(|(_, record)| {
            !(record.undone
                && record.paths.iter().any(|path| {
                    affects(&paths, path) || paths.iter().any(|root| root.starts_with(&format!("{path}/")))
                }))
        });
        for target in self.targets.values_mut().filter(|target| affects(&paths, &target.path)) {
            Self::mark(target, id);
        }
        self.records.push((id, Record { paths, undone: false }));
        let excess = self.records.len().saturating_sub(MAX_RECORDS);
        self.records.drain(..excess);
    }

    /// An editor of `path` opened. With `restore`, it gets the same marker steps as the editors
    /// already open (undone ones on its redo side), without replaying anything.
    pub fn attach(&mut self, path: &str, restore: bool) -> u64 {
        self.next_target += 1;
        let mut target = Target { path: path.to_owned(), done: Vec::new(), undone: Vec::new() };
        if restore {
            let records: Vec<&(u64, Record)> =
                self.records.iter().filter(|(_, record)| affects(&record.paths, path)).collect();
            for (id, _) in &records {
                Self::mark(&mut target, *id);
            }
            for (id, record) in records.iter().rev() {
                if record.undone && target.done.last() == Some(&Step::Marker(*id)) {
                    target.done.pop();
                    target.undone.push(Step::Marker(*id));
                }
            }
        }
        self.targets.insert(self.next_target, target);
        self.next_target
    }

    pub fn detach(&mut self, target: u64) {
        self.targets.remove(&target);
    }

    /// The editor's own change: it goes on top of its history and clears its redo side.
    pub fn local_edit(&mut self, target: u64) {
        if let Some(target) = self.targets.get_mut(&target) {
            target.done.push(Step::Local);
            target.undone.clear();
        }
    }

    fn step(target: &mut Target, id: u64, forward: bool) -> bool {
        let (from, to) = sides(target, forward);
        if from.last() != Some(&Step::Marker(id)) {
            return false;
        }
        from.pop();
        to.push(Step::Marker(id));
        true
    }

    /// Undo (`forward == false`) or Redo in editor `target`. On a marker, the editor's step moves
    /// now and the refactoring runs; while one runs, nothing moves.
    pub fn editor_step(&mut self, target: u64, forward: bool) -> Result<EditorStep, HistoryError> {
        if self.is_busy() {
            return Ok(EditorStep::Nothing);
        }
        let Some(editor) = self.targets.get_mut(&target) else { return Ok(EditorStep::Nothing) };
        let top = if forward { editor.undone.last() } else { editor.done.last() };
        match top.copied() {
            None => Ok(EditorStep::Nothing),
            Some(Step::Local) => {
                let (from, to) = sides(editor, forward);
                if let Some(step) = from.pop() {
                    to.push(step);
                }
                Ok(EditorStep::Local)
            }
            Some(Step::Marker(id)) => {
                Self::step(editor, id, forward);
                self.run(id, forward, Some(target)).map(EditorStep::Workspace)
            }
        }
    }

    /// Undoes (or, `forward`, redoes) workspace edit `id` from `source` (an editor whose step has
    /// moved already) or from the notification. Every other affected editor must have the marker
    /// on top; a refusal puts the source's step back.
    pub fn run(&mut self, id: u64, forward: bool, source: Option<u64>) -> Result<HistoryRequest, HistoryError> {
        if self.is_busy() {
            return Err(HistoryError::InProgress);
        }
        let checked = self.check(id, forward, source);
        match checked {
            Ok(targets) => {
                let request = HistoryRequest { id, forward };
                self.running = Some(Running { request, source, targets });
                Ok(request)
            }
            Err(error) => {
                self.restore_source(source, id, forward);
                Err(error)
            }
        }
    }

    fn check(&self, id: u64, forward: bool, source: Option<u64>) -> Result<Vec<u64>, HistoryError> {
        // Undo needs an edit that is applied, Redo one that is undone.
        let record = self.record(id).filter(|record| record.undone == forward).ok_or(HistoryError::NotUndoable)?;
        let targets: Vec<u64> = self
            .targets
            .iter()
            .filter(|(key, target)| Some(**key) != source && affects(&record.paths, &target.path))
            .map(|(key, _)| *key)
            .collect();
        for key in &targets {
            let target = &self.targets[key];
            let top = if forward { target.undone.last() } else { target.done.last() };
            if top != Some(&Step::Marker(id)) {
                return Err(HistoryError::NewerChanges);
            }
        }
        Ok(targets)
    }

    fn restore_source(&mut self, source: Option<u64>, id: u64, forward: bool) {
        if let Some(target) = source.and_then(|source| self.targets.get_mut(&source)) {
            Self::step(target, id, !forward);
        }
    }

    /// The project of the window changed: running operations of the old one are refused.
    pub fn project_changed(&mut self, project: Option<&str>) {
        if self.project.as_deref() != project {
            self.records.clear();
            self.project = project.map(str::to_owned);
        }
    }

    /// The backend request of the running operation finished. On success every other affected
    /// editor's marker moves and the edit's new state is returned; on failure the source's step
    /// is put back.
    pub fn finished(&mut self, result: Result<(), String>) -> Result<(u64, bool), HistoryError> {
        let Some(running) = self.running.take() else { return Err(HistoryError::NotUndoable) };
        let HistoryRequest { id, forward } = running.request;
        if let Err(error) = result {
            self.restore_source(running.source, id, forward);
            return Err(HistoryError::Backend(error));
        }
        for key in &running.targets {
            if let Some(target) = self.targets.get_mut(key) {
                Self::step(target, id, forward);
            }
        }
        let Some((_, record)) = self.records.iter_mut().find(|(key, _)| *key == id) else {
            return Err(HistoryError::ProjectClosed);
        };
        record.undone = !forward;
        Ok((id, record.undone))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rename(history: &mut WorkspaceHistory) {
        let paths = ["/p/A.java", "/p/B.java", "/p/closed.java"].map(str::to_owned).to_vec();
        history.applied(9, paths, Some("/p"));
    }

    fn history() -> WorkspaceHistory {
        WorkspaceHistory::new(Some("/p".into()))
    }

    #[test]
    fn editor_undo_and_redo_restore_the_entire_java_rename_with_one_step_in_every_split_and_background_tab() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        let split = h.attach("/p/A.java", true);
        let b = h.attach("/p/B.java", true);
        rename(&mut h);
        for target in [a, split, b] {
            assert_eq!(h.depths(target).0, 1);
        }
        let request = h.editor_step(split, false).expect("undoes");
        assert_eq!(request, EditorStep::Workspace(HistoryRequest { id: 9, forward: false }));
        assert_eq!(h.finished(Ok(())), Ok((9, true)));
        for target in [a, split, b] {
            assert_eq!(h.depths(target), (0, 1));
        }
        let request = h.editor_step(b, true).expect("redoes");
        assert_eq!(request, EditorStep::Workspace(HistoryRequest { id: 9, forward: true }));
        assert_eq!(h.finished(Ok(())), Ok((9, false)));
        assert_eq!(h.depths(a), (1, 0));
    }

    #[test]
    fn notification_undo_consumes_the_same_marker_and_never_makes_cmd_z_reapply_a_single_file() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        rename(&mut h);
        h.run(9, false, None).expect("undoes");
        h.finished(Ok(())).expect("undone");
        assert_eq!(h.editor_step(a, false), Ok(EditorStep::Nothing));
        assert_eq!(h.depths(a).1, 1);
    }

    #[test]
    fn preserves_earlier_local_history_and_refuses_the_notification_until_later_typing_is_undone() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        h.local_edit(a);
        rename(&mut h);
        h.local_edit(a);
        assert_eq!(h.run(9, false, None), Err(HistoryError::NewerChanges));
        assert_eq!(h.editor_step(a, false), Ok(EditorStep::Local));
        h.run(9, false, None).expect("undoes");
        h.finished(Ok(())).expect("undone");
        assert_eq!(h.depths(a).0, 1);
    }

    #[test]
    fn restores_the_editor_marker_when_backend_guards_refuse_undo() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        rename(&mut h);
        h.editor_step(a, false).expect("undoes");
        let refused = h.finished(Err("external write".into()));
        assert_eq!(refused, Err(HistoryError::Backend("external write".into())));
        assert_eq!(h.depths(a), (1, 0));
        assert!(!h.is_busy());
    }

    #[test]
    fn supports_direct_undo_from_an_attached_diff_view() {
        let mut h = history();
        let view = h.attach("/p/A.java", true);
        rename(&mut h);
        assert!(matches!(h.editor_step(view, false), Ok(EditorStep::Workspace(_))));
        h.finished(Ok(())).expect("undone");
        assert_eq!(h.depths(view).0, 0);
    }

    #[test]
    fn can_redo_after_a_file_opens_following_undo() {
        for path in ["/p/B.java", "/p/A.java"] {
            let mut h = history();
            let a = h.attach("/p/A.java", true);
            rename(&mut h);
            h.run(9, false, None).expect("undoes");
            h.finished(Ok(())).expect("undone");
            let next = h.attach(path, true);
            assert_eq!(h.depths(next), (0, 1), "{path}");
            let redo = h.editor_step(a, true);
            assert_eq!(redo, Ok(EditorStep::Workspace(HistoryRequest { id: 9, forward: true })), "{path}");
            h.finished(Ok(())).expect("redone");
            assert_eq!(h.depths(next).0, 1, "{path}");
        }
    }

    #[test]
    fn keeps_the_refactoring_notification_in_sync_with_editor_undo_and_redo() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        rename(&mut h);
        assert_eq!(h.is_undone(9), Some(false));
        h.editor_step(a, false).expect("undoes");
        assert_eq!(h.finished(Ok(())), Ok((9, true)));
        h.editor_step(a, true).expect("redoes");
        assert_eq!(h.finished(Ok(())), Ok((9, false)));
        h.run(9, false, None).expect("the notification undoes");
        assert_eq!(h.finished(Ok(())), Ok((9, true)));
        h.run(9, true, None).expect("the notification redoes");
        assert_eq!(h.finished(Ok(())), Ok((9, false)));
        assert_eq!(h.depths(a), (1, 0));
    }

    #[test]
    fn rebuilds_multiple_redo_markers_in_order_when_a_closed_file_opens() {
        let mut h = history();
        let a = h.attach("/p/A.java", true);
        rename(&mut h);
        h.applied(10, vec!["/p/A.java".into(), "/p/B.java".into()], Some("/p"));
        for id in [10, 9] {
            h.run(id, false, None).expect("undoes");
            h.finished(Ok(())).expect("undone");
        }
        let b = h.attach("/p/B.java", true);
        assert_eq!(h.depths(b).1, 2);
        let mut redone = Vec::new();
        for target in [b, a] {
            let Ok(EditorStep::Workspace(request)) = h.editor_step(target, true) else { panic!("redoes") };
            redone.push(request.id);
            h.finished(Ok(())).expect("redone");
        }
        assert_eq!(redone, [9, 10]);
    }

    #[test]
    fn refuses_redo_when_a_newly_opened_affected_editor_has_newer_typing() {
        let mut h = history();
        h.attach("/p/A.java", true);
        rename(&mut h);
        h.run(9, false, None).expect("undoes");
        h.finished(Ok(())).expect("undone");
        let b = h.attach("/p/B.java", true);
        h.local_edit(b);
        assert_eq!(h.run(9, true, None), Err(HistoryError::NewerChanges));
        assert_eq!(HistoryError::NewerChanges.message(), "Undo newer editor changes before undoing this refactoring");
    }

    #[test]
    fn keeps_one_refactoring_history_step_when_a_secondary_view_hands_its_history_to_a_tab() {
        let mut h = history();
        // A view sharing its history keeps no steps of its own: the tab's history holds them.
        let view = h.attach("/p/A.java", false);
        rename(&mut h);
        h.detach(view);
        let tab = h.attach("/p/A.java", true);
        assert_eq!(h.depths(tab).0, 1);
        assert!(matches!(h.editor_step(tab, false), Ok(EditorStep::Workspace(_))));
        h.finished(Ok(())).expect("undone");
        assert_eq!(h.depths(tab).0, 0);
    }

    #[test]
    fn keeps_refactoring_markers_current_while_a_secondary_view_is_unmounted() {
        let mut h = history();
        let view = h.attach("/p/A.java", false);
        h.detach(view);
        rename(&mut h);
        let tab = h.attach("/p/A.java", true);
        assert_eq!(h.depths(tab).0, 1);
        let Ok(EditorStep::Workspace(request)) = h.editor_step(tab, false) else { panic!("undoes") };
        assert_eq!(request, HistoryRequest { id: 9, forward: false });
        h.finished(Ok(())).expect("undone");
    }
}
