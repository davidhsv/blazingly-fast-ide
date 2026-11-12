//! Change list actions of the Commit view: Commit… on a list or on files, moving files between
//! lists and deleting a list.

use super::changelists::{active_list, checks_for_list, move_files};
use super::commit_store::{CommitState, CommitTab};
use super::dialogs::ConfirmRequest;
use super::ops::vcs_notice;
use super::store::VcsState;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::{ChangelistState, CheckState, GroupId, group_of, rel_to, strip_slash};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use std::collections::{HashMap, HashSet};

fn include(commit: &mut CommitState, checks: HashMap<String, CheckState>) {
    commit.checks = checks;
    commit.tab = CommitTab::Changes;
    commit.focus_tick += 1;
}

/// Commit… on a change list: includes exactly its changes and focuses the message. Under the modal
/// commit interface (`modal`) the message is in the Commit dialog: returns whether to open it.
pub fn commit_changelist(
    commit: &mut CommitState,
    vcs: &VcsState,
    lists: &ChangelistState,
    id: &str,
    modal: bool,
) -> bool {
    let root = vcs.root.as_deref().unwrap_or_default();
    include(commit, checks_for_list(&vcs.status, root, lists, id));
    modal
}

/// Commit File…: includes exactly these changes (absolute paths) and focuses the message. Under the
/// modal commit interface (`modal`) the message is in the Commit dialog: returns whether to open it.
pub fn commit_files<S: AsRef<str>>(commit: &mut CommitState, vcs: &VcsState, paths: &[S], modal: bool) -> bool {
    let chosen: HashSet<&str> = paths.iter().map(|path| strip_slash(path.as_ref())).collect();
    let checks = vcs
        .status
        .iter()
        .filter(|file| group_of(file) == Some(GroupId::Changes))
        .map(|file| {
            let check = if chosen.contains(strip_slash(&file.path)) { CheckState::All } else { CheckState::None };
            (file.path.clone(), check)
        })
        .collect();
    include(commit, checks);
    modal
}

/// Moves files (absolute paths) of the project at `root` to list `id`.
pub fn move_to_changelist<S: AsRef<str>>(
    lists: &ChangelistState,
    paths: &[S],
    root: Option<&str>,
    id: &str,
) -> ChangelistState {
    let root = root.unwrap_or_default();
    let rels: Vec<String> = paths.iter().map(|path| rel_to(root, strip_slash(path.as_ref())).to_owned()).collect();
    move_files(lists, &rels, id)
}

/// What Delete Changelist does first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeleteList {
    /// There is no such list.
    Missing,
    /// The active list cannot be deleted.
    Refuse(NotificationRequest),
    /// The list holds changes: ask, then delete it.
    Confirm(ConfirmRequest),
    /// Delete it ([`super::changelists::remove_list`]).
    Delete,
}

pub fn delete_changelist(lists: &ChangelistState, id: &str, i18n: &mut I18n) -> DeleteList {
    let Some(list) = lists.lists.iter().find(|list| list.id == id) else { return DeleteList::Missing };
    if id == lists.active {
        let mut notice = vcs_notice(NotificationLevel::Info, "The active changelist cannot be deleted");
        notice.body = Some("Make another changelist active first.".to_owned());
        return DeleteList::Refuse(notice);
    }
    let count = lists.files.values().filter(|list| list.as_str() == id).count();
    if count == 0 {
        return DeleteList::Delete;
    }
    let active = active_list(lists).map_or("", |list| list.name.as_str());
    let (contains, moved) = if count == 1 {
        (" change. It will be moved to '", "'. Delete the changelist?")
    } else {
        (" changes. They will be moved to '", "'. Delete the changelist?")
    };
    let values = [Param::from(list.name.as_str()), Param::from(count), Param::from(active)];
    let message = i18n.tx_built(&["Changelist '", "' contains ", contains, moved], &values);
    DeleteList::Confirm(ConfirmRequest::danger("Delete Changelist", message, "Delete"))
}

#[cfg(test)]
mod tests {
    use super::super::changelists::{add_list, initial_changelists, set_active_list};
    use super::*;
    use crate::vcs::changes::{ChangeKind, FileStatus};

    fn status() -> Vec<FileStatus> {
        let modified = |rel: &str| FileStatus {
            path: format!("/p/{rel}"),
            rel_path: rel.to_owned(),
            staged: None,
            unstaged: Some(ChangeKind::Modified),
            renamed_from: None,
        };
        vec![modified("src/a.ts"), modified("src/b.ts")]
    }

    fn vcs() -> VcsState {
        VcsState {
            available: true,
            root: Some("/p".to_owned()),
            repo: Some("/p".to_owned()),
            roots: vec!["/p".to_owned()],
            status: status(),
            ..VcsState::default()
        }
    }

    fn assert_only_a_included(commit: &CommitState) {
        assert_eq!(commit.checks.get("/p/src/a.ts"), Some(&CheckState::All));
        assert_eq!(commit.checks.get("/p/src/b.ts"), Some(&CheckState::None));
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Commit File… and the modal commit interface › opens the Commit dialog with just the file included
    #[test]
    fn opens_the_commit_dialog_with_just_the_file_included() {
        let mut commit = CommitState::default();
        assert!(commit_files(&mut commit, &vcs(), &["/p/src/a.ts"], true));
        assert_only_a_included(&commit);
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Commit File… and the modal commit interface › keeps the message in the Commit tool window without the modal interface
    #[test]
    fn keeps_the_message_in_the_commit_tool_window_without_the_modal_interface() {
        let mut commit = CommitState::default();
        let tick = commit.focus_tick;
        assert!(!commit_files(&mut commit, &vcs(), &["/p/src/a.ts"], false));
        assert_eq!(commit.focus_tick, tick + 1);
        assert_only_a_included(&commit);
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Commit File… and the modal commit interface › opens the Commit dialog for Commit… on a change list too
    #[test]
    fn opens_the_commit_dialog_for_commit_on_a_change_list_too() {
        let lists = add_list(&initial_changelists(), "other", "Other", "", false);
        let lists = move_to_changelist(&lists, &["/p/src/a.ts"], Some("/p"), "other");
        let mut commit = CommitState::default();
        assert!(commit_changelist(&mut commit, &vcs(), &lists, "other", true));
        assert_only_a_included(&commit);
    }

    // Not from a spec: a list with changes is asked about before it is deleted; the active one stays.
    #[test]
    fn asks_before_deleting_a_list_with_changes() {
        let mut i18n = I18n::default();
        let lists = add_list(&initial_changelists(), "other", "Other", "", false);
        assert_eq!(delete_changelist(&lists, "other", &mut i18n), DeleteList::Delete);
        assert_eq!(delete_changelist(&lists, "nope", &mut i18n), DeleteList::Missing);
        let holding = move_to_changelist(&lists, &["/p/src/a.ts"], Some("/p"), "other");
        let DeleteList::Confirm(question) = delete_changelist(&holding, "other", &mut i18n) else {
            panic!("expected a question");
        };
        assert_eq!(
            question.message,
            "Changelist 'Other' contains 1 change. It will be moved to 'Changes'. Delete the changelist?"
        );
        let active = set_active_list(&holding, "other");
        assert!(matches!(delete_changelist(&active, "other", &mut i18n), DeleteList::Refuse(_)));
    }
}
