//! What a network operation reports once it succeeded: a push names how many commits went to which
//! upstream.

use crate::vcs_ops::ops::counts_outgoing;
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::BranchInfo;

/// What a push about to run in a repository carries, for its summary: the upstream and how many
/// commits the branch is ahead of it (pushing does not move the local head, so this is read before
/// the push). `None` when nothing is ahead, and for a push to another remote or target, which
/// receives other commits than those.
pub fn outgoing_push(op: &RemoteOp, branch: Option<&BranchInfo>) -> Option<(String, usize)> {
    if !counts_outgoing(op) {
        return None;
    }
    let branch = branch?;
    let upstream = branch.upstream.as_deref().filter(|upstream| !upstream.is_empty())?;
    (branch.ahead > 0).then(|| (upstream.to_owned(), branch.ahead as usize))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::vcs_ops::ops::{remote_success_title, updated_title};

    fn push() -> RemoteOp {
        RemoteOp::Push { force_with_lease: false, set_upstream: false, tags: None, force: false, no_verify: false }
    }

    fn branch(upstream: &str, ahead: u32) -> BranchInfo {
        BranchInfo {
            head: Some("main".to_owned()),
            head_commit: None,
            upstream: Some(upstream.to_owned()),
            ahead,
            behind: 0,
            operation: None,
        }
    }

    /// Title of the notification of a push that succeeded in one repository on `branch`.
    fn push_title(op: &RemoteOp, branch: &BranchInfo, i18n: &mut I18n) -> String {
        let pushed: Vec<(String, usize)> = outgoing_push(op, Some(branch)).into_iter().collect();
        remote_success_title(op, &pushed, i18n)
    }

    fn chinese() -> I18n {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        i18n
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › push result notification › says how many commits went to which upstream
    #[test]
    fn says_how_many_commits_went_to_which_upstream() {
        let mut i18n = I18n::default();
        assert_eq!(push_title(&push(), &branch("origin/main", 3), &mut i18n), "Pushed 3 commits to origin/main");
        assert_eq!(push_title(&push(), &branch("origin/main", 1), &mut i18n), "Pushed 1 commit to origin/main");
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › push result notification › stays generic when nothing was ahead
    #[test]
    fn stays_generic_when_nothing_was_ahead() {
        let title = push_title(&push(), &branch("origin/main", 0), &mut I18n::default());
        assert!(title.ends_with("successful"), "{title}");
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › push result notification › does not name the upstream when the push went to another remote
    #[test]
    fn does_not_name_the_upstream_when_the_push_went_to_another_remote() {
        // `ahead` counts against the upstream, so it says nothing about what another remote received.
        let op = RemoteOp::PushBranch {
            branch: "main".to_owned(),
            revision: None,
            force_with_lease: false,
            target: None,
            remote: Some("fork".to_owned()),
            force: false,
            set_upstream: false,
            no_verify: false,
        };
        let title = push_title(&op, &branch("origin/main", 1), &mut I18n::default());
        assert!(title.ends_with("successful"), "{title}");
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › push result notification › is translated as a whole, count included
    #[test]
    fn is_translated_as_a_whole_count_included() {
        let title = push_title(&push(), &branch("origin/main", 3), &mut chinese());
        assert_eq!(title, "已推送 3 个提交到 origin/main");
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › Update Project result notification › counts the files and commits that came in
    #[test]
    fn counts_the_files_and_commits_that_came_in() {
        let mut i18n = I18n::default();
        assert_eq!(updated_title(1, 3, 2, &mut i18n), "3 files updated in 2 commits");
        assert_eq!(updated_title(1, 1, 1, &mut i18n), "1 file updated in 1 commit");
    }

    // src/features/vcs/__tests__/remoteResults.test.ts › Update Project result notification › translates the counts inside the sentence
    #[test]
    fn translates_the_counts_inside_the_sentence() {
        let mut i18n = chinese();
        // Notification titles are translated as they are shown.
        let title = updated_title(1, 3, 2, &mut i18n);
        assert_eq!(i18n.tx_template(&title), "已在 2 个提交中更新 3 个文件");
    }
}
