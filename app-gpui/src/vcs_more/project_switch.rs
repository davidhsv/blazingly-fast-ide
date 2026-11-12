//! The window switched to another project: nothing of the previous one's Git log carries over.
//! Repository selection inside a project does not change the log or its history tabs.

use super::log_loading::LogLoads;
use crate::vcs_ops::log_store::{GitLog, MAIN_LOG};
use ide_core::RequestId;

/// Forgets the commits, filters, selection and log tabs of the previous project. The Git tool
/// window stays on its content tab, a log tab giving way to the main Log. Returns the page requests
/// to cancel.
pub fn reset_git_log(log: &mut GitLog, loads: &mut LogLoads) -> Vec<RequestId> {
    let tab = if log.is_log_tab(&log.tab) { MAIN_LOG.to_owned() } else { log.tab.clone() };
    *log = GitLog::default();
    log.tab = tab;
    loads.reset()
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::test_support::commit;
    use super::*;
    use crate::vcs_ops::commit_store::{CommitState, MessageContext};
    use crate::vcs_ops::log_store::{ME, RepoCommit};
    use crate::vcs_ops::ops::{ConsoleKind, VcsConsole};
    use crate::vcs_ops::store::VcsState;

    // src/features/vcs/__tests__/projectSwitch.test.ts › switching the window to another repository › drops the commit message, Amend and the log of the previous one
    #[test]
    fn drops_the_commit_message_amend_and_the_log_of_the_previous_one() {
        let mut vcs = VcsState::default();
        vcs.reset_for_project(Some("/a"), true);
        let mut commits = CommitState::default();
        commits.message = "wip".to_owned();
        commits.amend = true;
        commits.selection = vec!["/a/x".to_owned()];
        let mut log = GitLog::default();
        let mut loads = LogLoads::default();
        log.view.commits = vec![RepoCommit::from(commit("c1", &[], &[]))];
        log.select_commit(Some("c1"), false);
        log.view.filters.authors = vec![ME.to_owned()];
        let mut console = VcsConsole::default();
        console.log(ConsoleKind::Cmd, "git status", 0.0);

        assert!(vcs.reset_for_project(Some("/b"), true));
        let previous = MessageContext { root: Some("/a"), operation: None };
        let _load = commits.vcs_changed(previous, MessageContext { root: vcs.root.as_deref(), operation: None });
        assert!(reset_git_log(&mut log, &mut loads).is_empty());
        console.clear();

        assert_eq!((commits.message.as_str(), commits.amend), ("", false));
        assert!(commits.selection.is_empty());
        assert!(log.view.commits.is_empty());
        assert_eq!((log.view.selected.as_deref(), log.view.selection.len()), (None, 0));
        assert!(log.view.filters.authors.is_empty());
        assert!(console.lines().is_empty());
    }
}
