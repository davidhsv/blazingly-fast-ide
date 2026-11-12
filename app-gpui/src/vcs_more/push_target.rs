//! Which push a Push dialog sends for one repository, from the remote and target picked in it.

use ide_host::ops::vcs::RemoteOp;
use ide_vcs::PushTags;

/// What a Push dialog holds for one repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PushChoice {
    /// Local branch being pushed.
    pub head: String,
    /// Its upstream as `remote/branch`, if any.
    pub tracked: Option<String>,
    /// Remote picked in the dialog; `None` keeps the tracked one.
    pub remote: Option<String>,
    /// Target branch field as typed; empty keeps the default.
    pub target: String,
    /// Another branch than the checked-out one: always pushed by name.
    pub explicit: bool,
    /// Push only up to this commit.
    pub revision: Option<String>,
    pub force_with_lease: bool,
    pub set_upstream: bool,
    /// Dropped once the push is retargeted: pushing a branch by name carries no tags.
    pub tags: Option<PushTags>,
    pub no_verify: bool,
}

/// `remote/branch` split at the first slash, as (remote, branch).
pub fn split_tracked(tracked: &str) -> (&str, &str) {
    tracked.split_once('/').unwrap_or(("", tracked))
}

/// The plain push while remote and target are those of the upstream, which keeps tags and the
/// update-and-retry of a rejected push; otherwise the push of the branch by name (`git push <remote>
/// <src>:<dst>`), naming only what differs from the upstream.
pub fn push_operation(c: &PushChoice) -> RemoteOp {
    let tracked = c.tracked.as_deref().filter(|tracked| !tracked.is_empty());
    let upstream = tracked.map(split_tracked);
    let default_target = upstream.map_or(c.head.as_str(), |(_, branch)| branch);
    let edited = c.target.trim();
    let retarget = !edited.is_empty() && edited != default_target;
    let remote = c.remote.as_deref().filter(|remote| !remote.is_empty());
    let remote_changed = remote.is_some() && remote != upstream.map(|(remote, _)| remote);
    let revision = c.revision.clone().filter(|revision| !revision.is_empty());
    if c.explicit || revision.is_some() || retarget || remote_changed {
        return RemoteOp::PushBranch {
            branch: c.head.clone(),
            set_upstream: c.set_upstream && revision.is_none(),
            revision,
            force_with_lease: c.force_with_lease,
            target: retarget.then(|| edited.to_owned()),
            remote: remote.filter(|_| remote_changed).map(str::to_owned),
            force: false,
            no_verify: c.no_verify,
        };
    }
    RemoteOp::Push {
        force_with_lease: c.force_with_lease,
        set_upstream: c.set_upstream || tracked.is_none(),
        tags: c.tags,
        force: false,
        no_verify: c.no_verify,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracked() -> PushChoice {
        PushChoice {
            head: "feat-x".to_owned(),
            tracked: Some("origin/feat-x".to_owned()),
            remote: Some("origin".to_owned()),
            target: "feat-x".to_owned(),
            ..PushChoice::default()
        }
    }

    fn is_plain(op: &RemoteOp) -> bool {
        matches!(op, RemoteOp::Push { .. })
    }

    /// The target and the remote a push by name goes to.
    fn destination(op: &RemoteOp) -> Option<(Option<&str>, Option<&str>)> {
        match op {
            RemoteOp::PushBranch { target, remote, .. } => Some((target.as_deref(), remote.as_deref())),
            _ => None,
        }
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › is the plain push of the tracked branch while remote and target are unchanged
    #[test]
    fn is_the_plain_push_of_the_tracked_branch_while_remote_and_target_are_unchanged() {
        let op = push_operation(&PushChoice { tags: Some(PushTags::All), ..tracked() });
        let RemoteOp::Push { force_with_lease, set_upstream, tags, force, no_verify } = op else {
            panic!("the tracked branch is pushed plainly");
        };
        assert_eq!((force_with_lease, set_upstream, force, no_verify), (false, false, false, false));
        assert_eq!(tags, Some(PushTags::All));
        // No remote picked yet, or the default name with stray spaces, or a cleared field.
        assert!(is_plain(&push_operation(&PushChoice { remote: None, ..tracked() })));
        assert!(is_plain(&push_operation(&PushChoice { target: "  feat-x ".to_owned(), ..tracked() })));
        assert!(is_plain(&push_operation(&PushChoice { target: String::new(), ..tracked() })));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › pushes to another branch of the tracked remote when the target changes
    #[test]
    fn pushes_to_another_branch_of_the_tracked_remote_when_the_target_changes() {
        let op = push_operation(&PushChoice { target: " release-2.1 ".to_owned(), ..tracked() });
        let RemoteOp::PushBranch { branch, revision, target, remote, force_with_lease, set_upstream, .. } = op else {
            panic!("the branch is pushed by name");
        };
        assert_eq!((branch.as_str(), revision, target.as_deref(), remote), ("feat-x", None, Some("release-2.1"), None));
        assert_eq!((force_with_lease, set_upstream), (false, false));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › pushes the tracked branch name to another remote when the remote changes
    #[test]
    fn pushes_the_tracked_branch_name_to_another_remote_when_the_remote_changes() {
        let op = push_operation(&PushChoice { remote: Some("fork".to_owned()), ..tracked() });
        assert!(matches!(&op, RemoteOp::PushBranch { branch, .. } if branch == "feat-x"));
        assert_eq!(destination(&op), Some((None, Some("fork"))));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › sets the upstream of a branch without one on the plain push
    #[test]
    fn sets_the_upstream_of_a_branch_without_one_on_the_plain_push() {
        let untracked = || PushChoice { tracked: None, remote: None, ..tracked() };
        assert!(matches!(push_operation(&untracked()), RemoteOp::Push { set_upstream: true, .. }));
        let renamed = push_operation(&PushChoice { target: "feat-y".to_owned(), ..untracked() });
        assert_eq!(destination(&renamed), Some((Some("feat-y"), None)));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › keeps force-with-lease and the hook choice on both kinds
    #[test]
    fn keeps_force_with_lease_and_the_hook_choice_on_both_kinds() {
        let force = || PushChoice { force_with_lease: true, no_verify: true, ..tracked() };
        assert!(matches!(push_operation(&force()), RemoteOp::Push { force_with_lease: true, no_verify: true, .. }));
        let retargeted = push_operation(&PushChoice { target: "release-2.1".to_owned(), ..force() });
        assert!(matches!(retargeted, RemoteOp::PushBranch { force_with_lease: true, no_verify: true, .. }));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › drops tags once the target changes: pushing a branch carries none
    #[test]
    fn drops_tags_once_the_target_changes_pushing_a_branch_carries_none() {
        let choice = PushChoice { tags: Some(PushTags::All), target: "release-2.1".to_owned(), ..tracked() };
        // A push by name has no tags to carry.
        assert!(matches!(push_operation(&choice), RemoteOp::PushBranch { .. }));
    }

    // src/features/vcs/__tests__/pushTarget.test.ts › the operation a push dialog sends › pushes another branch or a revision explicitly, without moving the upstream of a partial push
    #[test]
    fn pushes_another_branch_or_a_revision_explicitly_without_moving_the_upstream_of_a_partial_push() {
        let explicit = push_operation(&PushChoice { explicit: true, ..tracked() });
        assert_eq!(destination(&explicit), Some((None, None)));
        let revision = Some("abc123".to_owned());
        let partial = PushChoice { explicit: true, revision, set_upstream: true, ..tracked() };
        let RemoteOp::PushBranch { revision, set_upstream, .. } = push_operation(&partial) else {
            panic!("a revision is pushed by name");
        };
        assert_eq!((revision.as_deref(), set_upstream), (Some("abc123"), false));
    }
}
