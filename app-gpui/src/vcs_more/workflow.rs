//! The commits a repository's push would send, as the Push dialog of several repositories lists
//! them.

use crate::vcs_ops::remote_ui::{OutgoingQuery, outgoing_query};
use ide_vcs::BranchInfo;

/// The log query for the outgoing commits of a repository on `branch`; `None` for a detached HEAD,
/// which has none to list. When the query fails, [`recent_history_query`] takes its place.
pub fn push_commits_query(branch: &BranchInfo) -> Option<OutgoingQuery> {
    branch.head.as_deref().filter(|head| !head.is_empty())?;
    Some(outgoing_query(branch.upstream.as_deref()))
}

/// The query to fall back to when the upstream range does not resolve. That happens once the
/// upstream branch was deleted on the remote while the tracking configuration remains: the
/// repository then stays in the list, pushable, with its recent history.
pub fn recent_history_query() -> OutgoingQuery {
    outgoing_query(None)
}

#[cfg(test)]
mod tests {
    use super::super::log_loading::test_support::commit;
    use super::*;
    use crate::platform::i18n::I18n;
    use crate::vcs::changes::{ChangeKind, FileStatus};
    use crate::vcs_diff::model::status_decorations;
    use crate::vcs_ops::commit_view::{message_problems, wrap_at_margin};
    use crate::vcs_ops::remote_ui::{
        RepoPush, default_push_checked, is_lease_rejected, is_protected_branch, is_push_rejected, push_candidates,
    };
    use crate::vcs_ops::store::attention_repo;
    use ide_core::IpcPath;
    use ide_core::workflow_settings::CommitSettings;
    use ide_host::ops::vcs::VcsRoot;
    use std::collections::{BTreeMap, HashSet};
    use std::path::PathBuf;

    fn limits() -> CommitSettings {
        CommitSettings {
            blank_line_after_subject: true,
            limit_subject: true,
            subject_limit: 10,
            limit_body: true,
            body_limit: 8,
            ..CommitSettings::default()
        }
    }

    fn branch(head: Option<&str>, upstream: Option<&str>) -> BranchInfo {
        BranchInfo {
            head: head.map(str::to_owned),
            head_commit: Some("c1".to_owned()),
            upstream: upstream.map(str::to_owned),
            ahead: 0,
            behind: 0,
            operation: None,
        }
    }

    // src/features/vcs/__tests__/workflow.test.ts › commit message inspections › reports subject length, missing blank line and long body lines
    #[test]
    fn reports_subject_length_missing_blank_line_and_long_body_lines() {
        let cfg = limits();
        assert!(message_problems("short", &cfg).is_empty());
        assert_eq!(
            message_problems("a subject that is long\nbody", &cfg),
            ["Subject line is longer than 10 characters", "Missing blank line between subject and body"]
        );
        assert_eq!(message_problems("ok\n\nthis body line is long", &cfg), ["Body line 3 is longer than 8 characters"]);
        let unlimited = CommitSettings { limit_subject: false, ..cfg };
        assert!(message_problems(&"x".repeat(20), &unlimited).is_empty());
    }

    // src/features/vcs/__tests__/workflow.test.ts › commit message inspections › wraps body lines at the last space before the margin
    #[test]
    fn wraps_body_lines_at_the_last_space_before_the_margin() {
        let text = "subject\n\nhello world again";
        assert_eq!(wrap_at_margin(text, text.len(), 12).as_deref(), Some("subject\n\nhello world\nagain"));
        assert_eq!(wrap_at_margin("a very long subject line", 5, 5), None);
    }

    // src/features/vcs/__tests__/workflow.test.ts › git settings helpers › matches protected branch globs
    #[test]
    fn matches_protected_branch_globs() {
        assert!(is_protected_branch("main", "master; main"));
        assert!(is_protected_branch("release/1.2", "release/*"));
        assert!(!is_protected_branch("feature", "master, main"));
    }

    // src/features/vcs/__tests__/workflow.test.ts › git settings helpers › recognises rejected pushes
    #[test]
    fn recognises_rejected_pushes() {
        assert!(is_push_rejected(" ! [rejected]        main -> main (fetch first)"));
        assert!(!is_push_rejected("Permission denied (publickey)"));
        assert!(is_push_rejected(" ! [rejected]        main -> main (non-fast-forward)"));
        // A tag that exists on the remote, a hook or the server refusing: updating does not help.
        let tag_exists = " ! [rejected]        v1 -> v1 (already exists)\n\
                          hint: Updates were rejected because the tag already exists in the remote.";
        assert!(!is_push_rejected(tag_exists));
        assert!(!is_push_rejected(" ! [remote rejected] main -> main (pre-receive hook declined)"));
        assert!(!is_push_rejected(" ! [rejected]        main -> main (stale info)"));
        assert!(is_lease_rejected(" ! [rejected]        main -> main (stale info)"));
    }

    // src/features/vcs/__tests__/workflow.test.ts › git settings helpers › propagates folder colours only when enabled
    #[test]
    fn propagates_folder_colours_only_when_enabled() {
        let status = [FileStatus {
            path: "/p/src/a.ts".to_owned(),
            rel_path: "src/a.ts".to_owned(),
            staged: None,
            unstaged: Some(ChangeKind::Modified),
            renamed_from: None,
        }];
        let mut i18n = I18n::default();
        let propagated = |propagate: bool, i18n: &mut I18n| {
            let decorations = status_decorations(&status, propagate, i18n);
            let file = decorations.into_iter().find(|(path, _)| path == "/p/src/a.ts");
            file.expect("the file is decorated").1.propagate
        };
        assert_eq!(propagated(true, &mut i18n), Some(true));
        assert_eq!(propagated(false, &mut i18n), None);
    }

    // src/features/vcs/__tests__/workflow.test.ts › several repositories › selects a repository that just stopped in an operation
    #[test]
    fn selects_a_repository_that_just_stopped_in_an_operation() {
        let roots = |stopped: &[(&str, &str)]| -> Vec<VcsRoot> {
            stopped
                .iter()
                .map(|(root, operation)| VcsRoot {
                    root: IpcPath(PathBuf::from(root)),
                    operation: Some((*operation).to_owned()),
                    head: None,
                    head_commit: None,
                })
                .collect()
        };
        let none = BTreeMap::new();
        let merging = BTreeMap::from([("/p/lib".to_owned(), "merge".to_owned())]);
        assert_eq!(attention_repo(Some("/p"), &none, &roots(&[("/p/lib", "merge")])).as_deref(), Some("/p/lib"));
        assert_eq!(attention_repo(Some("/p"), &merging, &roots(&[("/p/lib", "merge")])), None);
        assert_eq!(attention_repo(Some("/p"), &none, &roots(&[("/p", "rebase"), ("/p/lib", "merge")])), None);
        assert_eq!(attention_repo(Some("/p"), &none, &roots(&[])), None);
    }

    // src/features/vcs/__tests__/workflow.test.ts › several repositories › lists repositories with something to push
    #[test]
    fn lists_repositories_with_something_to_push() {
        let outgoing = || vec![commit("c1", &[], &[])];
        let repo = |root: &str, branch: BranchInfo, commits| RepoPush { root: root.to_owned(), branch, commits };
        let repos = [
            repo("/p", branch(Some("main"), Some("origin/main")), outgoing()),
            repo("/p/a", branch(Some("main"), Some("origin/main")), Vec::new()),
            repo("/p/b", branch(Some("feature"), None), outgoing()),
            repo("/p/c", branch(None, None), Vec::new()),
        ];
        let candidates: Vec<RepoPush> = push_candidates(&repos).into_iter().cloned().collect();
        let listed: Vec<&str> = candidates.iter().map(|repo| repo.root.as_str()).collect();
        assert_eq!(listed, ["/p", "/p/b"]);
        assert_eq!(default_push_checked(&candidates, Some("/p")), HashSet::from(["/p"]));
        assert_eq!(default_push_checked(&candidates, Some("/p/b")), HashSet::from(["/p", "/p/b"]));
    }

    // src/features/vcs/__tests__/workflow.test.ts › pushRepoCommits › falls back to recent history when the upstream no longer resolves
    #[test]
    fn falls_back_to_recent_history_when_the_upstream_no_longer_resolves() {
        let first = push_commits_query(&branch(Some("feature"), Some("origin/feature")));
        assert_eq!(first.and_then(|query| query.range), Some("@{u}..HEAD"));
        // The range does not resolve: the recent history of HEAD is listed instead.
        assert_eq!(recent_history_query(), OutgoingQuery { branch: "HEAD", range: None, limit: 50 });
    }

    // src/features/vcs/__tests__/workflow.test.ts › pushRepoCommits › passes the upstream range through when it resolves
    #[test]
    fn passes_the_upstream_range_through_when_it_resolves() {
        let query = push_commits_query(&branch(Some("main"), Some("origin/main")));
        assert_eq!(query, Some(OutgoingQuery { branch: "HEAD", range: Some("@{u}..HEAD"), limit: 1000 }));
    }

    // src/features/vcs/__tests__/workflow.test.ts › pushRepoCommits › lists no commits for a detached HEAD
    #[test]
    fn lists_no_commits_for_a_detached_head() {
        assert_eq!(push_commits_query(&branch(None, None)), None);
    }
}
