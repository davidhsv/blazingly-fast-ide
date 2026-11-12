//! The New Pull Request tab's rules (IntelliJ's `GHPRCreateViewModel` and
//! `GHPRCreateMergeDirectionModel`): the merge direction (base and head repository and branch) and
//! how it is shown, the default branches, the branches check, the generated title, the push target
//! of a local head, and when Create is enabled.

use super::repository::GhRemote;
use super::types::{Coordinates, GhReviewer, GhUser};
use crate::platform::collate;
use crate::platform::git_ref_name::clean_up_branch_name_on_typing;
use ide_vcs::Branch;
use regex::Regex;
use std::sync::LazyLock;

/// A branch of a remote (`origin/main` is `{ remote: "origin", name: "main" }`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RemoteBranchRef {
    pub remote: String,
    pub name: String,
}

impl RemoteBranchRef {
    pub fn new(remote: impl Into<String>, name: impl Into<String>) -> Self {
        Self { remote: remote.into(), name: name.into() }
    }

    /// `remote/name`.
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.remote, self.name)
    }
}

/// The head: a local branch or a remote one (`GitLocalBranch` / `GitRemoteBranch`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HeadBranch {
    Local(String),
    Remote(RemoteBranchRef),
}

impl HeadBranch {
    /// What git calls the head (`feature`, `origin/feature`).
    pub fn git_name(&self) -> String {
        match self {
            Self::Local(name) => name.clone(),
            Self::Remote(r) => r.full_name(),
        }
    }
}

/// `BranchesState`: the base is always a branch of the connected repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Direction {
    pub base_repo: GhRemote,
    pub base_branch: Option<RemoteBranchRef>,
    pub head_repo: Option<GhRemote>,
    pub head_branch: Option<HeadBranch>,
}

/// `origin/main` split by the longest matching remote name.
pub fn split_remote_branch(full: &str, remotes: &[&str]) -> Option<RemoteBranchRef> {
    let remote = remotes
        .iter()
        .filter(|r| !r.is_empty() && full.strip_prefix(**r).is_some_and(|rest| rest.starts_with('/')))
        .max_by_key(|r| r.len())?;
    Some(RemoteBranchRef::new(*remote, &full[remote.len() + 1..]))
}

/// The branches of `remote` (its `HEAD` pointer left out), sorted by name.
pub fn remote_branches_of(branches: &[Branch], remote: &str) -> Vec<RemoteBranchRef> {
    let mut out: Vec<RemoteBranchRef> = branches
        .iter()
        .filter(|b| b.remote)
        .filter_map(|b| split_remote_branch(&b.name, &[remote]))
        .filter(|r| r.name != "HEAD")
        .collect();
    out.sort_by(|a, b| collate::compare(&a.name, &b.name));
    out
}

/// Local branches, sorted by name.
pub fn local_branches_of(branches: &[Branch]) -> Vec<HeadBranch> {
    let mut names: Vec<&str> = branches.iter().filter(|b| !b.remote).map(|b| b.name.as_str()).collect();
    names.sort_by(|a, b| collate::compare(a, b));
    names.into_iter().map(|n| HeadBranch::Local(n.to_owned())).collect()
}

/// `getDefaultRemoteBranch`: the repository's default branch on the base remote, else `master`, else `main`.
pub fn default_base_branch(branches: &[Branch], remote: &str, default_branch: Option<&str>) -> Option<RemoteBranchRef> {
    let names: Vec<String> = remote_branches_of(branches, remote).into_iter().map(|b| b.name).collect();
    [default_branch, Some("master"), Some("main")]
        .into_iter()
        .flatten()
        .find(|n| !n.is_empty() && names.iter().any(|x| x == n))
        .map(|n| RemoteBranchRef::new(remote, n))
}

/// The remote branch a local branch tracks (`upstream`), split by the known remotes.
pub fn upstream_of(branches: &[Branch], local: &str, remotes: &[&str]) -> Option<RemoteBranchRef> {
    let upstream = branches.iter().find(|b| !b.remote && b.name == local)?.upstream.as_deref()?;
    split_remote_branch(upstream, remotes)
}

/// The remote branch the head is or tracks (none for an untracked local branch).
pub fn tracked_head(head: &HeadBranch, branches: &[Branch], remotes: &[&str]) -> Option<RemoteBranchRef> {
    match head {
        HeadBranch::Remote(r) => Some(r.clone()),
        HeadBranch::Local(name) => upstream_of(branches, name, remotes),
    }
}

/// The default direction: base the default branch of the connected repository's remote, head the
/// current branch in the repository of its tracked remote (else the base one).
pub fn default_direction(
    base_repo: &GhRemote,
    remotes: &[GhRemote],
    branches: &[Branch],
    current_branch: Option<&str>,
    default_branch: Option<&str>,
) -> Direction {
    let names: Vec<&str> = remotes.iter().map(|r| r.remote.as_str()).collect();
    let tracked = current_branch.and_then(|c| upstream_of(branches, c, &names));
    let head_repo = tracked.and_then(|t| remotes.iter().find(|r| r.remote == t.remote)).unwrap_or(base_repo).clone();
    Direction {
        base_repo: base_repo.clone(),
        base_branch: default_base_branch(branches, &base_repo.remote, default_branch),
        head_repo: Some(head_repo),
        head_branch: current_branch.map(|c| HeadBranch::Local(c.to_owned())),
    }
}

fn repo_text(r: &GhRemote, show_owner: bool) -> String {
    if show_owner { format!("{}/{}", r.owner, r.name) } else { r.name.clone() }
}

/// The base as the direction selector shows it: `repo:branch`, `owner/repo:branch` when the head's
/// repository differs.
pub fn base_text(d: &Direction) -> Option<String> {
    let branch = d.base_branch.as_ref()?;
    let show_owner = d.head_repo.as_ref().is_some_and(|h| !h.coordinates().same(&d.base_repo.coordinates()));
    Some(format!("{}:{}", repo_text(&d.base_repo, show_owner), branch.name))
}

/// The head as the direction selector shows it: `repo:branch` (`origin/branch` for a remote branch).
pub fn head_text(d: &Direction) -> Option<String> {
    let (branch, repo) = (d.head_branch.as_ref()?, d.head_repo.as_ref()?);
    let show_owner = !repo.coordinates().same(&d.base_repo.coordinates());
    Some(format!("{}:{}", repo_text(repo, show_owner), branch.git_name()))
}

/// `git log base..head`.
pub fn commits_range(base: &RemoteBranchRef, head: &HeadBranch) -> String {
    format!("{}..{}", base.full_name(), head.git_name())
}

/// `generateTitle`: the subject of the only commit, else the branch name.
pub fn generate_title(commit_summaries: &[&str], head: &HeadBranch) -> String {
    match commit_summaries {
        [only] => (*only).to_owned(),
        _ => head.git_name(),
    }
}

/// `headRefName` of `createPullRequest`: `owner:branch` for a branch of another repository (a fork).
pub fn head_ref_name(base_repo: &Coordinates, head_repo: &Coordinates, branch: &str) -> String {
    if base_repo.same(head_repo) { branch.to_owned() } else { format!("{}:{branch}", head_repo.owner) }
}

/// `head` of the REST pull request search: always `owner:branch`.
pub fn search_head(head_repo: &Coordinates, branch: &str) -> String {
    format!("{}:{branch}", head_repo.owner)
}

/// `GitPushUtil.findPushTarget`: the tracked branch when it is on the head's remote.
pub fn push_target(branches: &[Branch], local: &str, head_remote: &str, remotes: &[&str]) -> Option<RemoteBranchRef> {
    upstream_of(branches, local, remotes).filter(|u| u.remote == head_remote)
}

/// Whether the local branch differs from its push target (or the target does not exist yet).
pub fn needs_push(branches: &[Branch], local: &str, target: &RemoteBranchRef) -> bool {
    let tip = |remote: bool, name: &str| {
        branches
            .iter()
            .find(|b| b.remote == remote && b.name == name)
            .map(|b| b.commit.as_str())
            .filter(|c| !c.is_empty())
    };
    match (tip(false, local), tip(true, &target.full_name())) {
        (Some(local), Some(remote)) => local != remote,
        _ => true,
    }
}

/// Characters and sequences git refuses in a branch name.
static ILLEGAL_REF_CHARS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\x00-\x20\x7F~^:?*\[\\]|\.\.|@\{").expect("valid regex"));
static EDGE_DOTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[./-]+|[./]+$").expect("valid regex"));

/// `GitRefNameValidator.cleanUpBranchName`: characters git refuses become `_`.
pub fn clean_up_branch_name(name: &str) -> String {
    let cleaned = ILLEGAL_REF_CHARS.replace_all(name.trim(), "_");
    let cleaned = EDGE_DOTS.replace_all(&cleaned, "");
    match cleaned.strip_suffix(".lock") {
        Some(stem) => format!("{stem}_lock"),
        None => cleaned.into_owned(),
    }
}

/// `BranchesCheckResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BranchesCheck {
    NoChanges { base: String, head: String },
    AlreadyExists { number: u32 },
    Ok,
}

/// `checkBranches`: no changes when the head tracks the base or has no commits over it, else an
/// open pull request of the head, else OK.
pub fn check_branches(
    base: &RemoteBranchRef,
    head: &HeadBranch,
    tracked: Option<&RemoteBranchRef>,
    commits: usize,
    existing: Option<u32>,
) -> BranchesCheck {
    if tracked == Some(base) || commits == 0 {
        return BranchesCheck::NoChanges { base: base.name.clone(), head: head.git_name() };
    }
    match existing {
        Some(number) => BranchesCheck::AlreadyExists { number },
        None => BranchesCheck::Ok,
    }
}

/// The description part of a commit message (its second paragraph).
pub fn commit_description(message: &str) -> String {
    message.split("\n\n").nth(1).map(|p| p.trim().to_owned()).unwrap_or_default()
}

/// A value being loaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loadable<T> {
    Loading,
    Failed(String),
    Ok(T),
}

/// `CreationState`: where creating the pull request is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreationState {
    Collecting,
    Pushing,
    Creating,
    Metadata,
    Created,
    Error(String),
}

/// Create is enabled for an OK check while nothing runs, or after a failure.
pub fn can_create(check: Option<&Loadable<BranchesCheck>>, creation: Option<&CreationState>) -> bool {
    matches!(check, Some(Loadable::Ok(BranchesCheck::Ok))) && matches!(creation, None | Some(CreationState::Error(_)))
}

/// Reviewers the current user may pick: everyone but themselves.
pub fn selectable_reviewers(items: &[GhReviewer], me: Option<&GhUser>) -> Vec<GhReviewer> {
    items.iter().filter(|r| r.is_team() || me.is_none_or(|me| r.id() != me.id)).cloned().collect()
}

/// "New Remote Branch for Pull Request": the name to push a local head without a push target to.
/// What is typed is cleaned as a branch name; OK is enabled for a non-blank name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RemoteBranchDialog {
    pub name: String,
}

impl RemoteBranchDialog {
    /// The dialog suggesting `suggested`.
    pub fn new(suggested: &str) -> Self {
        Self { name: suggested.to_owned() }
    }

    /// The field's new text, as typed.
    pub fn set_text(&mut self, text: &str) {
        self.name = clean_up_branch_name_on_typing(text);
    }

    pub fn can_accept(&self) -> bool {
        !self.name.trim().is_empty()
    }

    /// OK (or Enter): the name to push to; `None` while it is blank.
    pub fn accept(&self) -> Option<String> {
        self.can_accept().then(|| self.name.trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::team;
    use crate::github::types::{GhActor, GhTeam};

    fn branch(name: &str, upstream: Option<&str>, commit: &str, current: bool) -> Branch {
        Branch {
            name: name.into(),
            remote: false,
            current,
            upstream: upstream.map(str::to_owned),
            ahead: 0,
            behind: 0,
            last_commit_ms: 0.0,
            commit: commit.into(),
        }
    }

    fn remote(name: &str, commit: &str) -> Branch {
        Branch { remote: true, ..branch(name, None, commit, false) }
    }

    fn gh(owner: &str, r: &str) -> GhRemote {
        GhRemote { owner: owner.into(), name: "openidea".into(), remote: r.into(), url: String::new(), root: None }
    }

    fn origin() -> GhRemote {
        gh("l0ng-ai", "origin")
    }

    fn fork() -> GhRemote {
        gh("me", "fork")
    }

    fn branches() -> Vec<Branch> {
        vec![
            branch("feature", Some("fork/feature"), "a1", true),
            branch("local-only", None, "b1", false),
            branch("tracking-main", Some("origin/main"), "c1", false),
            remote("origin/HEAD", ""),
            remote("origin/main", "c1"),
            remote("origin/master", "m1"),
            remote("fork/feature", "a0"),
        ]
    }

    fn local(name: &str) -> HeadBranch {
        HeadBranch::Local(name.into())
    }

    #[test]
    fn splits_remote_branches_by_the_longest_remote_name() {
        assert_eq!(split_remote_branch("origin/feat/x", &["origin"]), Some(RemoteBranchRef::new("origin", "feat/x")));
        assert_eq!(split_remote_branch("my/fork/x", &["my", "my/fork"]), Some(RemoteBranchRef::new("my/fork", "x")));
        assert_eq!(split_remote_branch("upstream/x", &["origin"]), None);
    }

    #[test]
    fn lists_a_remotes_branches_without_head() {
        let names: Vec<String> = remote_branches_of(&branches(), "origin").into_iter().map(|b| b.name).collect();
        assert_eq!(names, ["main", "master"]);
    }

    #[test]
    fn defaults_the_base_to_the_repository_default_then_master_then_main() {
        let main = Some(RemoteBranchRef::new("origin", "main"));
        assert_eq!(default_base_branch(&branches(), "origin", Some("main")), main);
        assert_eq!(
            default_base_branch(&branches(), "origin", Some("develop")),
            Some(RemoteBranchRef::new("origin", "master"))
        );
        assert_eq!(default_base_branch(&[remote("origin/main", "")], "origin", None), main);
        assert_eq!(default_base_branch(&[], "origin", Some("main")), None);
    }

    #[test]
    fn defaults_the_head_to_the_current_branch_in_its_tracked_remotes_repository() {
        let remotes = [origin(), fork()];
        let d = default_direction(&origin(), &remotes, &branches(), Some("feature"), Some("main"));
        assert_eq!(d.head_repo, Some(fork()));
        assert_eq!(d.head_branch, Some(local("feature")));
        assert_eq!(d.base_branch, Some(RemoteBranchRef::new("origin", "main")));
        let untracked = default_direction(&origin(), &remotes, &branches(), Some("local-only"), Some("main"));
        assert_eq!(untracked.head_repo, Some(origin()));
    }

    #[test]
    fn shows_the_owner_only_when_the_repositories_differ() {
        let same = Direction {
            base_repo: origin(),
            base_branch: Some(RemoteBranchRef::new("origin", "main")),
            head_repo: Some(origin()),
            head_branch: Some(local("feature")),
        };
        assert_eq!(base_text(&same).as_deref(), Some("openidea:main"));
        assert_eq!(head_text(&same).as_deref(), Some("openidea:feature"));
        let forked = Direction {
            head_repo: Some(fork()),
            head_branch: Some(HeadBranch::Remote(RemoteBranchRef::new("fork", "x"))),
            ..same.clone()
        };
        assert_eq!(base_text(&forked).as_deref(), Some("l0ng-ai/openidea:main"));
        assert_eq!(head_text(&forked).as_deref(), Some("me/openidea:fork/x"));
        assert_eq!(base_text(&Direction { base_branch: None, ..same.clone() }), None);
        assert_eq!(head_text(&Direction { head_repo: None, ..same }), None);
    }

    #[test]
    fn builds_the_log_range_and_the_api_head_names() {
        let main = RemoteBranchRef::new("origin", "main");
        assert_eq!(commits_range(&main, &local("f")), "origin/main..f");
        assert_eq!(commits_range(&main, &HeadBranch::Remote(RemoteBranchRef::new("fork", "f"))), "origin/main..fork/f");
        let (o, f) = (origin().coordinates(), fork().coordinates());
        assert_eq!(head_ref_name(&o, &o, "f"), "f");
        assert_eq!(head_ref_name(&o, &f, "f"), "me:f");
        assert_eq!(search_head(&o, "f"), "l0ng-ai:f");
    }

    #[test]
    fn finds_the_tracked_branch_of_the_head() {
        let base = RemoteBranchRef::new("origin", "main");
        assert_eq!(tracked_head(&local("tracking-main"), &branches(), &["origin", "fork"]), Some(base));
        assert_eq!(tracked_head(&local("local-only"), &branches(), &["origin"]), None);
    }

    #[test]
    fn reports_no_changes_an_existing_pull_request_or_ok() {
        let base = RemoteBranchRef::new("origin", "main");
        let head = local("tracking-main");
        assert_eq!(
            check_branches(&base, &head, Some(&base), 1, None),
            BranchesCheck::NoChanges { base: "main".into(), head: "tracking-main".into() }
        );
        assert!(matches!(check_branches(&base, &head, None, 0, Some(3)), BranchesCheck::NoChanges { .. }));
        assert_eq!(check_branches(&base, &head, None, 1, Some(3)), BranchesCheck::AlreadyExists { number: 3 });
        assert_eq!(check_branches(&base, &head, None, 1, None), BranchesCheck::Ok);
    }

    #[test]
    fn enables_create_only_for_ok_while_nothing_runs_or_after_a_failure() {
        let ok = Loadable::Ok(BranchesCheck::Ok);
        assert!(can_create(Some(&ok), None));
        assert!(can_create(Some(&ok), Some(&CreationState::Error("x".into()))));
        assert!(!can_create(Some(&ok), Some(&CreationState::Pushing)));
        assert!(!can_create(Some(&Loadable::Loading), None));
        assert!(!can_create(Some(&Loadable::Ok(BranchesCheck::AlreadyExists { number: 1 })), None));
    }

    #[test]
    fn is_the_tracked_branch_on_the_heads_remote() {
        let remotes = ["origin", "fork"];
        assert_eq!(
            push_target(&branches(), "feature", "fork", &remotes),
            Some(RemoteBranchRef::new("fork", "feature"))
        );
        assert_eq!(push_target(&branches(), "feature", "origin", &remotes), None);
        assert_eq!(push_target(&branches(), "local-only", "origin", &remotes), None);
    }

    #[test]
    fn needs_a_push_when_the_tips_differ_or_the_remote_branch_is_missing() {
        assert!(needs_push(&branches(), "feature", &RemoteBranchRef::new("fork", "feature")));
        assert!(!needs_push(&branches(), "tracking-main", &RemoteBranchRef::new("origin", "main")));
        assert!(needs_push(&branches(), "local-only", &RemoteBranchRef::new("origin", "local-only")));
    }

    #[test]
    fn cleans_up_branch_names_like_git_ref_name_validator() {
        assert_eq!(clean_up_branch_name(" my feature: v2? "), "my_feature__v2_");
        assert_eq!(clean_up_branch_name("a..b~c^d"), "a_b_c_d");
        assert_eq!(clean_up_branch_name("/x.lock"), "x_lock");
    }

    #[test]
    fn titles_with_the_only_commits_subject_else_the_branch() {
        assert_eq!(generate_title(&["Fix it"], &local("feature")), "Fix it");
        assert_eq!(generate_title(&["a", "b"], &local("feature")), "feature");
        assert_eq!(generate_title(&[], &HeadBranch::Remote(RemoteBranchRef::new("fork", "x"))), "fork/x");
    }

    #[test]
    fn takes_the_description_paragraph_of_a_commit_message() {
        assert_eq!(commit_description("Subject\n\nBody line\nmore\n\nTrailer: x"), "Body line\nmore");
        assert_eq!(commit_description("Subject"), "");
    }

    #[test]
    fn leaves_the_current_user_out_of_the_reviewers() {
        let me = GhActor { typename: "User".into(), id: "1".into(), login: "me".into(), ..GhActor::default() };
        let other = GhActor { id: "2".into(), login: "other".into(), ..me.clone() };
        let t = GhReviewer::Team(GhTeam { id: "1".into(), ..team() });
        let items = [GhReviewer::Actor(me.clone()), GhReviewer::Actor(other.clone()), t.clone()];
        assert_eq!(selectable_reviewers(&items, Some(&me)), [GhReviewer::Actor(other), t]);
    }

    #[test]
    fn turns_spaces_typed_in_the_remote_branch_name_into_separators() {
        let mut dialog = RemoteBranchDialog::new("");
        assert!(!dialog.can_accept());
        for c in "my feature".chars() {
            let typed = format!("{}{c}", dialog.name);
            dialog.set_text(&typed);
        }
        assert_eq!(dialog.name, "my-feature");
        assert_eq!(dialog.accept().as_deref(), Some("my-feature"));
    }
}
