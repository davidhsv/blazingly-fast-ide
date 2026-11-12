//! Where committing happens: the Commit tool window (non-modal commit interface) or the Commit
//! dialog with the local changes in the Git tool window (modal one, Settings › Version Control ›
//! Commit). Also the push that follows Commit and Push….

use super::commit_store::{CommitState, CommitTab};
use super::git_tabs::stash_tab_of;
use super::remote_ui::is_protected_branch;
use ide_core::settings::{CommitSettings, GitSettings, VcsSettings};
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::BranchInfo;

/// The Commit dialog is used instead of the Commit tool window.
pub fn is_modal_commit(commit: &CommitSettings) -> bool {
    !commit.non_modal
}

/// Where the local changes, Shelf or Stash tab was shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalChangesWindow {
    /// The Git tool window: select its content tab `tab` and show the window.
    Git { tab: CommitTab },
    /// The Commit tool window (its tab is selected already): show the window.
    Commit,
}

/// Shows the local changes, Shelf or Stash tab wherever the commit interface puts them.
pub fn show_local_changes(state: &mut CommitState, tab: CommitTab, vcs: &VcsSettings) -> LocalChangesWindow {
    let target = stash_tab_of(tab, vcs.git.combine_stash_and_shelf);
    if is_modal_commit(&vcs.commit) {
        return LocalChangesWindow::Git { tab: target };
    }
    state.tab = target;
    if tab == CommitTab::Changes {
        state.focus_tick += 1;
    }
    LocalChangesWindow::Commit
}

/// What Commit… does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitEntry {
    /// The message field of the Commit tool window was focused: show that window.
    Window(LocalChangesWindow),
    /// Open the Commit dialog.
    Dialog,
}

/// Commit…: focuses the commit message of the Commit window, or opens the Commit dialog.
pub fn open_commit(state: &mut CommitState, vcs: &VcsSettings) -> CommitEntry {
    if is_modal_commit(&vcs.commit) {
        CommitEntry::Dialog
    } else {
        CommitEntry::Window(show_local_changes(state, CommitTab::Changes, vcs))
    }
}

/// Commit and Push…: whether the Push dialog comes up after the commit (Settings › Git › "Show Push
/// dialog for Commit and Push", optionally only for protected branches) or the branch is pushed
/// right away. `target` is the remote branch pushed to (`None`: none can be told, e.g. a detached
/// HEAD, which always needs the dialog).
pub fn push_dialog_needed(git: &GitSettings, target: Option<&str>) -> bool {
    let Some(target) = target.filter(|target| !target.is_empty()) else { return true };
    git.show_push_dialog && (!git.push_dialog_only_protected || is_protected_branch(target, &git.protected_branches))
}

/// What follows the commit of Commit and Push….
#[derive(Clone)]
pub enum PushAfterCommit {
    /// The Push dialog.
    Dialog,
    /// A push of the current branch; `detail` says where to.
    Push { op: RemoteOp, detail: String },
}

/// After Commit and Push…, with the status refreshed: the Push dialog, or a push of the current
/// branch. `repositories`: how many the project has.
pub fn push_after_commit(branch: Option<&BranchInfo>, repositories: usize, git: &GitSettings) -> PushAfterCommit {
    fn given(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|value| !value.is_empty())
    }
    let head = branch.and_then(|branch| given(&branch.head));
    let upstream = branch.and_then(|branch| given(&branch.upstream));
    let target = upstream.map(|upstream| upstream.split_once('/').map_or(upstream, |(_, name)| name)).or(head);
    // Several repositories: which of them to push is chosen in the dialog.
    if repositories > 1 || push_dialog_needed(git, target) {
        return PushAfterCommit::Dialog;
    }
    let head = head.unwrap_or_default();
    let detail = match upstream {
        Some(upstream) => format!("{head} → {upstream}"),
        None => format!("{head} → new remote branch"),
    };
    let op = RemoteOp::Push {
        force_with_lease: false,
        set_upstream: upstream.is_none(),
        tags: None,
        force: false,
        no_verify: false,
    };
    PushAfterCommit::Push { op, detail }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git() -> GitSettings {
        GitSettings {
            show_push_dialog: true,
            push_dialog_only_protected: false,
            protected_branches: "master;main".to_owned(),
            ..GitSettings::default()
        }
    }

    // src/features/vcs/__tests__/commitMode.test.ts › Commit and Push… push dialog › shows the dialog unless turned off, or only for protected branches
    #[test]
    fn shows_the_dialog_unless_turned_off_or_only_for_protected_branches() {
        assert!(push_dialog_needed(&git(), Some("feature")));
        let off = GitSettings { show_push_dialog: false, ..git() };
        assert!(!push_dialog_needed(&off, Some("main")));
        let protected = GitSettings { push_dialog_only_protected: true, ..git() };
        assert!(!push_dialog_needed(&protected, Some("feature")));
        assert!(push_dialog_needed(&protected, Some("main")));
        // Nothing to tell the target from (detached HEAD): always the dialog.
        assert!(push_dialog_needed(&off, None));
    }

    // Not from a spec: with the dialog turned off, the current branch is pushed to its upstream.
    #[test]
    fn pushes_right_away_when_the_dialog_is_turned_off() {
        let off = GitSettings { show_push_dialog: false, ..git() };
        let detached = super::super::test_support::branch_info();
        let fresh = BranchInfo { head: Some("topic".to_owned()), ..detached };
        let tracked =
            BranchInfo { head: Some("main".to_owned()), upstream: Some("origin/main".to_owned()), ..fresh.clone() };
        let PushAfterCommit::Push { op, detail } = push_after_commit(Some(&tracked), 1, &off) else {
            panic!("expected a push");
        };
        assert_eq!(detail, "main → origin/main");
        assert!(matches!(op, RemoteOp::Push { set_upstream: false, .. }));
        assert!(matches!(push_after_commit(Some(&tracked), 2, &off), PushAfterCommit::Dialog));
        let PushAfterCommit::Push { op, detail } = push_after_commit(Some(&fresh), 1, &off) else {
            panic!("expected a push");
        };
        assert_eq!(detail, "topic → new remote branch");
        assert!(matches!(op, RemoteOp::Push { set_upstream: true, .. }));
        assert!(matches!(push_after_commit(None, 1, &off), PushAfterCommit::Dialog));
    }

    // Not from a spec: the commit interface decides which window shows the local changes.
    #[test]
    fn shows_the_local_changes_where_the_commit_interface_puts_them() {
        let mut state = CommitState::default();
        let mut vcs = VcsSettings::default();
        assert_eq!(open_commit(&mut state, &vcs), CommitEntry::Window(LocalChangesWindow::Commit));
        assert_eq!(state.focus_tick, 1);
        assert_eq!(show_local_changes(&mut state, CommitTab::Shelf, &vcs), LocalChangesWindow::Commit);
        assert_eq!((state.tab, state.focus_tick), (CommitTab::Shelf, 1));
        vcs.commit.non_modal = false;
        vcs.git.combine_stash_and_shelf = true;
        assert_eq!(open_commit(&mut state, &vcs), CommitEntry::Dialog);
        let shown = show_local_changes(&mut state, CommitTab::Shelf, &vcs);
        assert_eq!(shown, LocalChangesWindow::Git { tab: CommitTab::Stashes });
    }
}
