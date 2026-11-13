//! Undo / Redo of a refactoring that changed several files: after the backend applied a workspace
//! edit (`workspaceEditApplied`), a notification offers to undo the whole edit as one step. It follows
//! the same history as the editor's Undo / Redo, so it offers Redo once the edit is undone.
//!
//! The undo waits for the editors' pending typing to reach the backend first, and is refused once
//! the project it was made in is no longer open.

use crate::workbench::notifications::NotificationLevel;

/// The notification's key: a newer one replaces it.
pub const NOTICE_KEY: &str = "refactor.workspaceEdit";

/// What the notification's button undoes or redoes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoAction {
    pub label: &'static str,
    entry: Entry,
}

/// A notification to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoNotice {
    pub level: NotificationLevel,
    pub title: &'static str,
    pub body: Option<String>,
    pub key: Option<&'static str>,
    pub action: Option<UndoAction>,
}

/// What running the button needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoStep {
    /// Wait until every open editor's edits reached the backend, then call [`WorkspaceUndo::synced`].
    WaitForSync,
    /// `undo_workspace_edit(id)`.
    Undo(u32),
    /// `redo_workspace_edit(id)`.
    Redo(u32),
    /// Show this notification instead.
    Refused(UndoNotice),
    /// The notification is outdated: a newer edit or history step took its place.
    Nothing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: u32,
    /// The project root the edit was made in.
    project: Option<String>,
    undone: bool,
}

/// The last workspace edit and whether it is undone.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceUndo {
    current: Option<Entry>,
}

fn notice(entry: &Entry) -> UndoNotice {
    UndoNotice {
        level: NotificationLevel::Info,
        title: if entry.undone { "Refactoring undone" } else { "Refactoring applied" },
        body: None,
        key: Some(NOTICE_KEY),
        action: Some(UndoAction { label: if entry.undone { "Redo" } else { "Undo" }, entry: entry.clone() }),
    }
}

/// The error shown when an undo or redo failed.
pub fn failed_notice(message: &str) -> UndoNotice {
    UndoNotice {
        level: NotificationLevel::Error,
        title: "Cannot undo refactoring",
        body: Some(message.to_owned()),
        key: None,
        action: None,
    }
}

impl WorkspaceUndo {
    /// The backend applied workspace edit `id` in `project`.
    pub fn applied(&mut self, id: u32, project: Option<&str>) -> UndoNotice {
        let entry = Entry { id, project: project.map(str::to_owned), undone: false };
        let shown = notice(&entry);
        self.current = Some(entry);
        shown
    }

    /// The editor's history undid (`undone`) or redid edit `id`: the notification follows.
    pub fn history(&mut self, id: u32, undone: bool, project: Option<&str>) -> Option<UndoNotice> {
        let current = self.current.as_mut().filter(|entry| entry.id == id && entry.project.as_deref() == project)?;
        current.undone = undone;
        Some(notice(current))
    }

    /// The button was pressed: pending typing goes first.
    pub fn run(&self, _action: &UndoAction) -> UndoStep {
        UndoStep::WaitForSync
    }

    /// The editors are in sync; `project` is the window's project now.
    pub fn synced(&self, action: &UndoAction, project: Option<&str>) -> UndoStep {
        if action.entry.project.as_deref() != project {
            return UndoStep::Refused(failed_notice("The project for this refactoring is no longer open"));
        }
        if self.current.as_ref() != Some(&action.entry) {
            return UndoStep::Nothing;
        }
        if action.entry.undone { UndoStep::Redo(action.entry.id) } else { UndoStep::Undo(action.entry.id) }
    }

    /// The window switched projects or closed: no notification acts any more.
    pub fn clear(&mut self) {
        self.current = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(notice: &UndoNotice) -> UndoAction {
        notice.action.clone().expect("the notice has a button")
    }

    #[test]
    fn waits_for_pending_typing_before_undoing_the_entire_backend_transaction() {
        let mut undo = WorkspaceUndo::default();
        let notice = undo.applied(17, Some("/p"));
        let button = action(&notice);
        assert_eq!(button.label, "Undo");
        assert_eq!(undo.run(&button), UndoStep::WaitForSync);
        assert_eq!(undo.synced(&button, Some("/p")), UndoStep::Undo(17));
    }

    #[test]
    fn reports_a_refused_undo_instead_of_discarding_newer_work() {
        let notice = failed_notice("a.txt changed");
        assert_eq!(
            (notice.level, notice.title, notice.body.as_deref()),
            (NotificationLevel::Error, "Cannot undo refactoring", Some("a.txt changed"))
        );
    }

    #[test]
    fn does_not_apply_an_old_notifications_id_to_a_different_project() {
        let mut undo = WorkspaceUndo::default();
        let button = action(&undo.applied(19, Some("/p")));
        let UndoStep::Refused(refused) = undo.synced(&button, Some("/different-project")) else {
            panic!("the undo is refused");
        };
        assert_eq!(refused.level, NotificationLevel::Error);
        assert_eq!(refused.body.as_deref(), Some("The project for this refactoring is no longer open"));
    }

    // Not from a spec: the notification follows the editor's history, and an outdated one does nothing.
    #[test]
    fn follows_the_editor_history_and_ignores_outdated_buttons() {
        let mut undo = WorkspaceUndo::default();
        let old = action(&undo.applied(20, Some("/p")));
        let undone = undo.history(20, true, Some("/p")).expect("the notice follows");
        assert_eq!((undone.title, undone.key), ("Refactoring undone", Some(NOTICE_KEY)));
        let redo = action(&undone);
        assert_eq!(redo.label, "Redo");
        assert_eq!(undo.synced(&redo, Some("/p")), UndoStep::Redo(20));
        assert_eq!(undo.synced(&old, Some("/p")), UndoStep::Nothing);
        assert_eq!(undo.history(21, false, Some("/p")), None);
        undo.clear();
        assert_eq!(undo.synced(&redo, Some("/p")), UndoStep::Nothing);
    }
}
