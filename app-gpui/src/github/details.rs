//! Rules of a pull request's details tab, after IntelliJ: the review flow
//! (`GHPRReviewFlowViewModelImpl`, `GHPRDetailsActionsComponentFactory`), the status section
//! (`GHPRStatusChecksComponentFactory`, `CodeReviewDetailsStatusComponentFactory`,
//! `GHPRReviewerStateUIUtil`), the commit picker (`CodeReviewDetailsCommitsComponentFactory`), the
//! branch widget (`GHPRBranchesViewModel`), the changes tree's progress state
//! (`CodeReviewProgressTreeModel`, `GHPRChangeListViewModel`) and the viewed toggle
//! (`GHPRViewedStateToggleAction`).

use super::data_model::{ReviewerWithState, has_permission};
use super::repository::GhRemote;
use super::types::{
    CiJob, CiJobState, CommitFile, CommitFileStatus, FileViewedState, GhPullRequest, GhReviewThread, GhReviewer,
    Permission, PrState, ReviewDecision, ReviewerState,
};
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::changes::EffectiveKind;
use crate::vcs_diff::file_tree::{FileTreeNode, build_file_tree};
use ide_vcs::BranchInfo;
use std::collections::HashSet;

// ---------------------------------------------------------------------------
// Review flow

/// `ReviewRequestState`: a draft is DRAFT whatever its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewRequestState {
    Opened,
    Closed,
    Merged,
    Draft,
}

/// `ReviewRole`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewRole {
    Author,
    Reviewer,
    Guest,
}

/// `ReviewState`: the overall review state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverallReviewState {
    Accepted,
    WaitForUpdates,
    NeedReview,
}

pub fn review_request_state(state: PrState, is_draft: bool) -> ReviewRequestState {
    match state {
        _ if is_draft => ReviewRequestState::Draft,
        PrState::Closed => ReviewRequestState::Closed,
        PrState::Merged => ReviewRequestState::Merged,
        PrState::Open => ReviewRequestState::Opened,
    }
}

/// AUTHOR when the user `me_id` wrote the pull request, REVIEWER when requested or when they
/// reviewed, else GUEST.
pub fn review_role(pr: &GhPullRequest, me_id: &str) -> ReviewRole {
    let pr = &pr.short;
    if pr.author.as_ref().is_some_and(|a| a.id == me_id) {
        return ReviewRole::Author;
    }
    let requested =
        pr.review_requests.nodes.iter().any(|r| r.requested_reviewer.as_ref().is_some_and(|r| r.id() == me_id));
    if requested || pr.reviews.nodes.iter().any(|r| r.author.as_ref().is_some_and(|a| a.id == me_id)) {
        ReviewRole::Reviewer
    } else {
        ReviewRole::Guest
    }
}

/// ACCEPTED when the approvals reach the required count (one when none is required), else from
/// GitHub's review decision.
pub fn overall_review_state(
    review_decision: Option<ReviewDecision>,
    reviewers: &[ReviewerWithState],
    required_approving_reviews_count: u32,
) -> OverallReviewState {
    let approved = reviewers.iter().filter(|r| r.state == ReviewerState::Approved).count() as u32;
    let is_approved =
        if required_approving_reviews_count == 0 { approved > 0 } else { approved >= required_approving_reviews_count };
    match review_decision {
        _ if is_approved => OverallReviewState::Accepted,
        Some(ReviewDecision::Approved) => OverallReviewState::Accepted,
        Some(ReviewDecision::ChangesRequested) => OverallReviewState::WaitForUpdates,
        _ => OverallReviewState::NeedReview,
    }
}

/// `GHPRReviewerState.isAwaitingReview`.
pub fn is_awaiting_review(s: Option<ReviewerState>) -> bool {
    matches!(s, Some(ReviewerState::NeedsReview | ReviewerState::ReRequested))
}

/// Entries of the More (⋯) menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoreItem {
    RequestReview,
    MergeGroup,
    Close,
}

/// Which review-flow controls an open pull request shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenedActions {
    pub request_review: bool,
    pub re_request_review: bool,
    pub submit_review: bool,
    pub set_myself_as_reviewer: bool,
    pub merge: bool,
    pub more: Vec<MoreItem>,
}

/// The open pull request's buttons and More menu for `role` (`GHPRDetailsActionsComponentFactory`).
pub fn opened_actions(
    role: ReviewRole,
    state: OverallReviewState,
    requested_reviewers: usize,
    my_state: Option<ReviewerState>,
) -> OpenedActions {
    use MoreItem::*;
    let accepted = state == OverallReviewState::Accepted;
    match role {
        ReviewRole::Author => OpenedActions {
            request_review: state == OverallReviewState::NeedReview
                || (state == OverallReviewState::WaitForUpdates && requested_reviewers > 0),
            re_request_review: state == OverallReviewState::WaitForUpdates && requested_reviewers == 0,
            merge: accepted,
            more: if accepted { vec![RequestReview, Close] } else { vec![MergeGroup, Close] },
            ..OpenedActions::default()
        },
        ReviewRole::Reviewer => OpenedActions {
            submit_review: my_state != Some(ReviewerState::Approved),
            merge: accepted && !is_awaiting_review(my_state),
            more: if accepted { vec![RequestReview, Close] } else { vec![RequestReview, MergeGroup, Close] },
            ..OpenedActions::default()
        },
        ReviewRole::Guest => OpenedActions {
            set_myself_as_reviewer: true,
            more: vec![RequestReview, MergeGroup, Close],
            ..OpenedActions::default()
        },
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}

impl MergeMethod {
    /// The button's text, translated.
    pub fn label(self, i18n: &I18n) -> String {
        i18n.tx(match self {
            Self::Merge => "Merge…",
            Self::Squash => "Squash and Merge…",
            Self::Rebase => "Rebase",
        })
    }
}

/// The merge split button's main method and the others in its menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeButton {
    pub main: MergeMethod,
    pub others: Vec<MergeMethod>,
}

/// The merge split button's main method (the first the repository allows) and the others.
pub fn merge_button_methods(merge_commit_allowed: bool, squash_merge_allowed: bool) -> MergeButton {
    let main = if merge_commit_allowed {
        MergeMethod::Merge
    } else if squash_merge_allowed {
        MergeMethod::Squash
    } else {
        MergeMethod::Rebase
    };
    let others = [MergeMethod::Merge, MergeMethod::Squash, MergeMethod::Rebase].into_iter().filter(|m| *m != main);
    MergeButton { main, others: others.collect() }
}

/// Reviewers without repeats (by login / team slug).
pub fn dedupe(reviewers: &[GhReviewer]) -> Vec<GhReviewer> {
    let mut seen: HashSet<&str> = HashSet::new();
    reviewers.iter().filter(|r| seen.insert(r.key())).cloned().collect()
}

/// Reviewers to add and remove to go from `current` to `chosen` (`CollectionDelta`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewersDelta {
    pub added: Vec<GhReviewer>,
    pub removed: Vec<GhReviewer>,
}

pub fn reviewers_delta(current: &[GhReviewer], chosen: &[GhReviewer]) -> ReviewersDelta {
    let before: HashSet<&str> = current.iter().map(GhReviewer::key).collect();
    let after: HashSet<&str> = chosen.iter().map(GhReviewer::key).collect();
    ReviewersDelta {
        added: dedupe(chosen).into_iter().filter(|r| !before.contains(r.key())).collect(),
        removed: dedupe(current).into_iter().filter(|r| !after.contains(r.key())).collect(),
    }
}

/// `getPresentableName`: the name, else the login (teams: the name, else `org/slug`).
pub fn presentable_name(r: &GhReviewer) -> &str {
    match r {
        GhReviewer::Team(t) => {
            if t.team_name.is_empty() {
                t.combined_slug.as_str()
            } else {
                t.team_name.as_str()
            }
        }
        GhReviewer::Actor(a) => a.name.as_deref().filter(|n| !n.is_empty()).unwrap_or(a.login.as_str()),
    }
}

// ---------------------------------------------------------------------------
// Status section

/// A status row's icon (`CIBuildStatusIcons` / `ReviewDetailsUIUtil`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusIcon {
    Success,
    Failed,
    Pending,
    FailedInProgress,
    Skipped,
    Warning,
    Error,
}

impl StatusIcon {
    /// The icon's name in the icon set.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Failed => "ci-failed",
            Self::Pending => "ci-pending",
            Self::FailedInProgress => "ci-failed-in-progress",
            Self::Skipped => "ci-skipped",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }

    pub fn of_job(s: CiJobState) -> Self {
        match s {
            CiJobState::Failed => Self::Failed,
            CiJobState::Pending => Self::Pending,
            CiJobState::Skipped => Self::Skipped,
            CiJobState::Success => Self::Success,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusLine {
    pub icon: StatusIcon,
    pub text: String,
}

/// The CI line (`calcPipelineIcon` / `calcPipelineText`); `None` without jobs.
pub fn ci_summary(jobs: &[CiJob], i18n: &I18n) -> Option<StatusLine> {
    if jobs.is_empty() {
        return None;
    }
    let failed = jobs.iter().any(|j| j.status == CiJobState::Failed);
    let pending = jobs.iter().any(|j| j.status == CiJobState::Pending);
    let (icon, text) = if jobs.iter().filter(|j| j.is_required).all(|j| j.status == CiJobState::Success) {
        (StatusIcon::Success, "All required checks have passed")
    } else if pending && failed {
        (StatusIcon::FailedInProgress, "Checks in progress, and some already failed…")
    } else if pending {
        (StatusIcon::Pending, "Checks in progress…")
    } else {
        (StatusIcon::Failed, "Checks failed")
    };
    Some(StatusLine { icon, text: i18n.tx(text) })
}

/// The access line (`createAccessDeniedLabel`); `None` when the user may do everything.
pub fn access_denied_text(
    permission: Option<Permission>,
    viewer_did_author: bool,
    is_draft: bool,
    merge_forbidden: bool,
    i18n: &I18n,
) -> Option<String> {
    let can_merge = has_permission(permission, Permission::Write);
    let can_close = has_permission(permission, Permission::Triage) || viewer_did_author;
    let can_mark_ready_for_review = can_merge || viewer_did_author;
    let text = if !can_close {
        "Repository access required to manage pull requests"
    } else if (!can_mark_ready_for_review && is_draft) || (!can_merge && !is_draft) {
        "Repository write access required to merge pull requests"
    } else if merge_forbidden && !is_draft {
        "Merging is disabled for this project"
    } else {
        return None;
    };
    Some(i18n.tx(text))
}

/// A reviewer row's text (`GHPRReviewerStateUIUtil.getText`).
pub fn reviewer_status_text(state: ReviewerState, name: &str, i18n: &mut I18n) -> String {
    let name = [Param::from(name)];
    match state {
        ReviewerState::Approved => i18n.tx_built(&["", " approved review"], &name),
        ReviewerState::ChangesRequested => i18n.tx_built(&["", " is waiting for updates"], &name),
        ReviewerState::NeedsReview => i18n.tx_built(&["Waiting for review from ", ""], &name),
        ReviewerState::ReRequested => i18n.tx_built(&["Re-requested review from ", ""], &name),
        ReviewerState::Commented => i18n.tx_built(&["", " left comments"], &name),
    }
}

/// A reviewer row's status icon: approved, changes requested, awaiting (warning), commented.
pub fn reviewer_status_icon(state: ReviewerState) -> &'static str {
    match state {
        ReviewerState::Approved => "success",
        ReviewerState::ChangesRequested => "error",
        ReviewerState::Commented => "review-comment",
        ReviewerState::NeedsReview | ReviewerState::ReRequested => "warning",
    }
}

/// The remotes Resolve merges between (`GHPRResolveConflictsLocallyViewModel.requestOrError`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveRemotes {
    pub head_remote: String,
    pub base_remote: String,
    pub head_ref_name: String,
    pub base_ref_name: String,
}

/// The local git remote of `name_with_owner` among the project's GitHub remotes.
pub fn remote_of<'a>(remotes: &'a [GhRemote], name_with_owner: &str) -> Option<&'a GhRemote> {
    if name_with_owner.is_empty() {
        return None;
    }
    let key = name_with_owner.to_lowercase();
    remotes.iter().find(|r| format!("{}/{}", r.owner, r.name).to_lowercase() == key)
}

/// What Resolve needs, or why it cannot run (`GHPRResolveConflictsLocallyError`). `merge_in_progress`:
/// a merge or rebase runs in the repository; `already_resolved_locally`: the current branch tracks
/// the head branch and contains the base commit.
pub fn resolve_conflicts(
    details: Option<&GhPullRequest>,
    remotes: &[GhRemote],
    merge_in_progress: bool,
    already_resolved_locally: bool,
    i18n: &mut I18n,
) -> Result<ResolveRemotes, String> {
    if merge_in_progress {
        return Err(i18n.tx("Merge is already in progress"));
    }
    let Some(pr) = details else { return Err(i18n.tx("Details have not finished loading")) };
    let Some(head_repo) = &pr.head_repository else {
        return Err(i18n.tx_built(&["", " repository not found"], &[Param::from("Source")]));
    };
    let Some(base_repo) = &pr.base_repository else {
        return Err(i18n.tx_built(&["", " repository not found"], &[Param::from("Target")]));
    };
    let Some(head) = remote_of(remotes, &head_repo.name_with_owner) else {
        let values = [Param::from("Source"), Param::from(head_repo.url.as_str())];
        return Err(i18n.tx_built(&["", " remote not found: ", ""], &values));
    };
    let Some(base) = remote_of(remotes, &base_repo.name_with_owner) else {
        let values = [Param::from("Target"), Param::from(base_repo.url.as_str())];
        return Err(i18n.tx_built(&["", " remote not found: ", ""], &values));
    };
    if already_resolved_locally {
        return Err(i18n.tx("Already resolved locally, push your changes to resolve"));
    }
    Ok(ResolveRemotes {
        head_remote: head.remote.clone(),
        base_remote: base.remote.clone(),
        head_ref_name: pr.head_ref_name.clone(),
        base_ref_name: pr.base_ref_name.clone(),
    })
}

// ---------------------------------------------------------------------------
// Branches

/// The head branch as the widget shows it: `owner:branch` for forks and when head and base names are equal.
pub fn head_branch_label(pr: &GhPullRequest) -> String {
    match &pr.head_repository {
        Some(repo) if repo.is_fork || pr.base_ref_name == pr.head_ref_name => {
            format!("{}:{}", repo.owner.login, pr.head_ref_name)
        }
        _ => pr.head_ref_name.clone(),
    }
}

/// The GitHub repository of a branch's upstream (`origin/feature`, `None` for an unknown remote),
/// with its branch name.
fn upstream_of<'a>(upstream: &'a str, remotes: &[GhRemote]) -> Option<(Option<String>, &'a str)> {
    let named = remotes
        .iter()
        .filter(|r| upstream.strip_prefix(r.remote.as_str()).is_some_and(|rest| rest.starts_with('/')))
        .max_by_key(|r| r.remote.len());
    if let Some(r) = named {
        return Some((Some(format!("{}/{}", r.owner, r.name)), &upstream[r.remote.len() + 1..]));
    }
    match upstream.find('/') {
        Some(slash) if slash > 0 => Some((None, &upstream[slash + 1..])),
        _ => None,
    }
}

/// Whether the pull request's head branch is checked out (`testRemoteBranchCheckedOut`): the current
/// branch tracks the head repository's branch, or (gh's fork checkouts, which track a URL) it is
/// named like the head branch and sits on the head commit.
pub fn is_head_checked_out(branch: Option<&BranchInfo>, remotes: &[GhRemote], pr: &GhPullRequest) -> bool {
    let Some(branch) = branch else { return false };
    let Some(head) = branch.head.as_deref() else { return false };
    let up = branch.upstream.as_deref().and_then(|u| upstream_of(u, remotes));
    if let (Some((Some(repo), up_branch)), Some(head_repo)) = (&up, &pr.head_repository) {
        return *up_branch == pr.head_ref_name && repo.eq_ignore_ascii_case(&head_repo.name_with_owner);
    }
    let name = head.rsplit('/').next().unwrap_or(head);
    (name == pr.head_ref_name || head == pr.head_ref_name)
        && branch.head_commit.as_deref() == Some(pr.head_ref_oid.as_str())
}

// ---------------------------------------------------------------------------
// Commit picker

/// The bold label before the dropdown: "Changes" for one commit, else "Changes from".
pub fn commits_title(count: usize, i18n: &I18n) -> String {
    i18n.tx(if count == 1 { "Changes" } else { "Changes from" })
}

/// Index of the next (`forward`) / previous commit, or `None` at either end or with all commits
/// shown (`selected` `None`).
pub fn step_commit(selected: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    let selected = selected?;
    let next = if forward { selected.checked_add(1)? } else { selected.checked_sub(1)? };
    (next < count).then_some(next)
}

// ---------------------------------------------------------------------------
// Changes tree

/// `CodeReviewProgressTreeModel`'s node state: read (viewed) and the unresolved discussions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressState {
    pub is_read: bool,
    pub discussions: usize,
}

/// Unresolved review threads per path (`unresolvedThreadsCountByChange`), in order of first thread.
pub fn unresolved_threads_by_path(threads: &[GhReviewThread]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for t in threads.iter().filter(|t| !t.is_resolved) {
        match out.iter_mut().find(|(path, _)| *path == t.path) {
            Some((_, n)) => *n += 1,
            None => out.push((t.path.clone(), 1)),
        }
    }
    out
}

/// Only VIEWED counts as viewed (DISMISSED: changed since); a path without state counts as read.
pub fn is_viewed(state: Option<FileViewedState>) -> bool {
    matches!(state, None | Some(FileViewedState::Viewed))
}

/// A tree node's progress: a file's own, a folder's the sum of its files (read when all are).
pub fn progress_of(node: &FileTreeNode<CommitFile>, leaf: &dyn Fn(&CommitFile) -> ProgressState) -> ProgressState {
    if let Some(file) = &node.file {
        return leaf(file);
    }
    node.children.iter().fold(ProgressState { is_read: true, discussions: 0 }, |acc, c| {
        let s = progress_of(c, leaf);
        ProgressState { is_read: acc.is_read && s.is_read, discussions: acc.discussions + s.discussions }
    })
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The tree's roots: grouped by directory, or a flat list by file name.
pub fn changes_tree(files: &[CommitFile], group_by_directory: bool) -> Vec<FileTreeNode<CommitFile>> {
    if group_by_directory {
        return build_file_tree(files.iter().cloned(), |f| f.filename.clone(), |_| false);
    }
    let mut sorted: Vec<&CommitFile> = files.iter().collect();
    sorted.sort_by(|a, b| {
        collate::compare(file_name(&a.filename), file_name(&b.filename))
            .then_with(|| collate::compare(&a.filename, &b.filename))
    });
    sorted
        .into_iter()
        .map(|f| FileTreeNode {
            key: format!("f:{}", f.filename),
            name: file_name(&f.filename).to_owned(),
            file: Some(f.clone()),
            directory: None,
            children: Vec::new(),
            count: 1,
        })
        .collect()
}

/// Files of the selected nodes (by key), folders contributing all theirs (`SELECTED_CHANGES`), in tree order.
pub fn selected_files<'a>(roots: &'a [FileTreeNode<CommitFile>], keys: &HashSet<String>) -> Vec<&'a CommitFile> {
    fn add<'a>(n: &'a FileTreeNode<CommitFile>, out: &mut Vec<&'a CommitFile>) {
        match &n.file {
            Some(f) => {
                if !out.iter().any(|o| o.filename == f.filename) {
                    out.push(f);
                }
            }
            None => n.children.iter().for_each(|c| add(c, out)),
        }
    }
    fn walk<'a>(nodes: &'a [FileTreeNode<CommitFile>], keys: &HashSet<String>, out: &mut Vec<&'a CommitFile>) {
        for n in nodes {
            if keys.contains(&n.key) {
                add(n, out);
            } else {
                walk(&n.children, keys, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(roots, keys, &mut out);
    out
}

/// The file's VCS status colour kind (`Change.getFileStatus`).
pub fn change_kind(status: CommitFileStatus) -> EffectiveKind {
    match status {
        CommitFileStatus::Added | CommitFileStatus::Copied => EffectiveKind::Added,
        CommitFileStatus::Removed => EffectiveKind::Deleted,
        _ => EffectiveKind::Modified,
    }
}

/// The viewed toggle's text (`GHPRViewedStateToggleAction`).
pub fn viewed_toggle_text(count: usize, all_viewed: bool, i18n: &I18n) -> String {
    i18n.tx(match (count == 1, all_viewed) {
        (true, false) => "Mark as Viewed",
        (true, true) => "Mark as Not Viewed",
        (false, false) => "Mark All as Viewed",
        (false, true) => "Mark All as Not Viewed",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::{requested, team, user, user_named};
    use crate::github::types::*;

    fn repo(name_with_owner: &str, is_fork: bool) -> GhRepository {
        GhRepository {
            id: name_with_owner.into(),
            owner: RepositoryOwner {
                typename: "User".into(),
                id: "o".into(),
                login: name_with_owner.split('/').next().unwrap_or_default().into(),
            },
            name_with_owner: name_with_owner.into(),
            viewer_permission: Some(Permission::Write),
            merge_commit_allowed: true,
            squash_merge_allowed: true,
            rebase_merge_allowed: true,
            default_branch_ref: Some(RefName { name: "main".into() }),
            is_fork,
            url: format!("https://github.com/{name_with_owner}"),
            ssh_url: String::new(),
        }
    }

    fn file(filename: &str, status: CommitFileStatus) -> CommitFile {
        CommitFile { filename: filename.into(), status, additions: 1, changes: 1, ..CommitFile::default() }
    }

    fn remote(owner: &str, name: &str, r: &str) -> GhRemote {
        GhRemote { owner: owner.into(), name: name.into(), remote: r.into(), url: String::new(), root: None }
    }

    fn pr() -> GhPullRequest {
        GhPullRequest {
            short: GhPullRequestShort {
                id: "PR1".into(),
                url: "https://github.com/o/r/pull/7".into(),
                number: 7,
                title: "Title".into(),
                author: Some(user("alice")),
                is_read_by_viewer: true,
                mergeable: Mergeable::Mergeable,
                viewer_can_update: true,
                viewer_can_react: true,
                ..GhPullRequestShort::default()
            },
            base_ref_name: "main".into(),
            base_ref_oid: "base".into(),
            base_repository: Some(repo("o/r", false)),
            head_ref: Some(NodeId { id: "ref".into() }),
            head_ref_name: "feature".into(),
            head_ref_oid: "head".into(),
            head_repository: Some(repo("o/r", false)),
            ..GhPullRequest::default()
        }
    }

    fn approved(login: &str) -> ReviewerWithState {
        ReviewerWithState { reviewer: GhReviewer::Actor(user(login)), state: ReviewerState::Approved }
    }

    #[test]
    fn derives_the_request_state_a_draft_first() {
        assert_eq!(review_request_state(PrState::Open, false), ReviewRequestState::Opened);
        assert_eq!(review_request_state(PrState::Merged, false), ReviewRequestState::Merged);
        assert_eq!(review_request_state(PrState::Closed, false), ReviewRequestState::Closed);
        assert_eq!(review_request_state(PrState::Closed, true), ReviewRequestState::Draft);
    }

    #[test]
    fn decides_the_role_from_authorship_requests_and_reviews() {
        assert_eq!(review_role(&pr(), "id-alice"), ReviewRole::Author);
        let mut asked = pr();
        asked.short.review_requests = Nodes::of(vec![requested(GhReviewer::Actor(user("bob")))]);
        assert_eq!(review_role(&asked, "id-bob"), ReviewRole::Reviewer);
        let mut reviewed = pr();
        reviewed.short.reviews = Nodes::of(vec![GhReview { author: Some(user("bob")), ..GhReview::default() }]);
        assert_eq!(review_role(&reviewed, "id-bob"), ReviewRole::Reviewer);
        assert_eq!(review_role(&pr(), "id-bob"), ReviewRole::Guest);
    }

    #[test]
    fn accepts_on_enough_approvals_else_follows_the_review_decision() {
        let one = [approved("b")];
        assert_eq!(overall_review_state(None, &one, 0), OverallReviewState::Accepted);
        assert_eq!(overall_review_state(None, &one, 2), OverallReviewState::NeedReview);
        assert_eq!(overall_review_state(Some(ReviewDecision::Approved), &[], 2), OverallReviewState::Accepted);
        assert_eq!(
            overall_review_state(Some(ReviewDecision::ChangesRequested), &[], 0),
            OverallReviewState::WaitForUpdates
        );
        assert_eq!(overall_review_state(Some(ReviewDecision::ReviewRequired), &[], 0), OverallReviewState::NeedReview);
    }

    #[test]
    fn shows_the_authors_buttons_per_review_state() {
        use MoreItem::*;
        let a = opened_actions(ReviewRole::Author, OverallReviewState::NeedReview, 0, None);
        assert_eq!((a.request_review, a.re_request_review, a.merge), (true, false, false));
        assert_eq!(a.more, [MergeGroup, Close]);
        let a = opened_actions(ReviewRole::Author, OverallReviewState::WaitForUpdates, 1, None);
        assert_eq!((a.request_review, a.re_request_review), (true, false));
        let a = opened_actions(ReviewRole::Author, OverallReviewState::WaitForUpdates, 0, None);
        assert_eq!((a.request_review, a.re_request_review), (false, true));
        let a = opened_actions(ReviewRole::Author, OverallReviewState::Accepted, 0, None);
        assert_eq!((a.request_review, a.merge), (false, true));
        assert_eq!(a.more, [RequestReview, Close]);
    }

    #[test]
    fn shows_the_reviewers_and_the_guests_buttons() {
        use MoreItem::*;
        let a =
            opened_actions(ReviewRole::Reviewer, OverallReviewState::NeedReview, 1, Some(ReviewerState::NeedsReview));
        assert_eq!((a.submit_review, a.merge), (true, false));
        assert_eq!(a.more, [RequestReview, MergeGroup, Close]);
        let a = opened_actions(ReviewRole::Reviewer, OverallReviewState::Accepted, 0, Some(ReviewerState::Approved));
        assert_eq!((a.submit_review, a.merge), (false, true));
        let a = opened_actions(ReviewRole::Reviewer, OverallReviewState::Accepted, 1, Some(ReviewerState::ReRequested));
        assert!(!a.merge);
        let a = opened_actions(ReviewRole::Guest, OverallReviewState::Accepted, 0, None);
        assert_eq!((a.set_myself_as_reviewer, a.merge), (true, false));
        assert_eq!(a.more, [RequestReview, MergeGroup, Close]);
    }

    #[test]
    fn picks_the_merge_buttons_main_method_from_the_repository_settings() {
        use MergeMethod::*;
        assert_eq!(merge_button_methods(true, true), MergeButton { main: Merge, others: vec![Squash, Rebase] });
        assert_eq!(merge_button_methods(false, true).main, Squash);
        assert_eq!(merge_button_methods(false, false), MergeButton { main: Rebase, others: vec![Merge, Squash] });
    }

    #[test]
    fn computes_the_reviewer_delta_by_login_and_team_slug() {
        let a = GhReviewer::Actor(user("a"));
        let b = GhReviewer::Actor(user("b"));
        let t = GhReviewer::Team(team());
        let delta = reviewers_delta(&[a.clone(), t.clone()], &[b.clone(), t, b.clone()]);
        assert_eq!(delta, ReviewersDelta { added: vec![b], removed: vec![a] });
    }

    #[test]
    fn presents_reviewers_by_name_else_login_or_slug() {
        assert_eq!(presentable_name(&GhReviewer::Actor(user_named("a", Some("Ann")))), "Ann");
        assert_eq!(presentable_name(&GhReviewer::Actor(user("a"))), "a");
        assert_eq!(presentable_name(&GhReviewer::Team(team())), "Core");
        assert_eq!(presentable_name(&GhReviewer::Team(GhTeam { team_name: String::new(), ..team() })), "org/core");
    }

    fn job(status: CiJobState, is_required: bool) -> CiJob {
        CiJob { name: format!("{status:?}"), status, is_required, details_url: None }
    }

    #[test]
    fn summarises_the_ci_jobs() {
        use CiJobState::*;
        let i18n = I18n::default();
        let line = |icon, text: &str| Some(StatusLine { icon, text: text.into() });
        assert_eq!(ci_summary(&[], &i18n), None);
        assert_eq!(
            ci_summary(&[job(Success, true), job(Failed, false)], &i18n),
            line(StatusIcon::Success, "All required checks have passed")
        );
        assert_eq!(
            ci_summary(&[job(Pending, true), job(Failed, true)], &i18n).map(|l| l.icon),
            Some(StatusIcon::FailedInProgress)
        );
        assert_eq!(ci_summary(&[job(Pending, true)], &i18n), line(StatusIcon::Pending, "Checks in progress…"));
        assert_eq!(
            ci_summary(&[job(Failed, true), job(Skipped, false)], &i18n),
            line(StatusIcon::Failed, "Checks failed")
        );
    }

    #[test]
    fn explains_missing_access() {
        let i18n = I18n::default();
        let text = |p, author, draft, forbidden| access_denied_text(Some(p), author, draft, forbidden, &i18n);
        let manage = Some("Repository access required to manage pull requests".to_owned());
        let write = Some("Repository write access required to merge pull requests".to_owned());
        assert_eq!(text(Permission::Read, false, false, false), manage);
        assert_eq!(text(Permission::Read, true, false, false), write);
        assert_eq!(text(Permission::Read, true, true, false), None);
        assert_eq!(text(Permission::Triage, false, true, false), write);
        assert_eq!(text(Permission::Write, false, false, false), None);
        assert_eq!(
            text(Permission::Write, false, false, true),
            Some("Merging is disabled for this project".to_owned())
        );
    }

    #[test]
    fn words_and_marks_reviewer_states_like_github() {
        let mut i18n = I18n::default();
        assert_eq!(reviewer_status_text(ReviewerState::Approved, "Ann", &mut i18n), "Ann approved review");
        assert_eq!(
            reviewer_status_text(ReviewerState::ChangesRequested, "Ann", &mut i18n),
            "Ann is waiting for updates"
        );
        assert_eq!(reviewer_status_text(ReviewerState::NeedsReview, "Ann", &mut i18n), "Waiting for review from Ann");
        assert_eq!(reviewer_status_text(ReviewerState::ReRequested, "Ann", &mut i18n), "Re-requested review from Ann");
        assert_eq!(reviewer_status_text(ReviewerState::Commented, "Ann", &mut i18n), "Ann left comments");
        assert_eq!(reviewer_status_icon(ReviewerState::Approved), "success");
        assert_eq!(reviewer_status_icon(ReviewerState::ChangesRequested), "error");
        assert_eq!(reviewer_status_icon(ReviewerState::ReRequested), "warning");
        assert_eq!(reviewer_status_icon(ReviewerState::Commented), "review-comment");
    }

    #[test]
    fn decides_whether_resolve_can_run() {
        let mut i18n = I18n::default();
        let remotes = [remote("o", "r", "origin")];
        let details = pr();
        assert_eq!(
            resolve_conflicts(Some(&details), &remotes, false, false, &mut i18n),
            Ok(ResolveRemotes {
                head_remote: "origin".into(),
                base_remote: "origin".into(),
                head_ref_name: "feature".into(),
                base_ref_name: "main".into(),
            })
        );
        let err = |r: Result<ResolveRemotes, String>| r.err().unwrap_or_default();
        assert_eq!(
            err(resolve_conflicts(None, &remotes, false, false, &mut i18n)),
            "Details have not finished loading"
        );
        assert_eq!(
            err(resolve_conflicts(Some(&details), &remotes, true, false, &mut i18n)),
            "Merge is already in progress"
        );
        let no_head = GhPullRequest { head_repository: None, ..pr() };
        assert_eq!(
            err(resolve_conflicts(Some(&no_head), &remotes, false, false, &mut i18n)),
            "Source repository not found"
        );
        let fork = GhPullRequest { head_repository: Some(repo("fork/r", true)), ..pr() };
        assert_eq!(
            err(resolve_conflicts(Some(&fork), &remotes, false, false, &mut i18n)),
            "Source remote not found: https://github.com/fork/r"
        );
        assert_eq!(
            err(resolve_conflicts(Some(&details), &remotes, false, true, &mut i18n)),
            "Already resolved locally, push your changes to resolve"
        );
    }

    #[test]
    fn shows_owner_branch_for_forks_and_equal_names() {
        assert_eq!(head_branch_label(&pr()), "feature");
        assert_eq!(
            head_branch_label(&GhPullRequest { head_repository: Some(repo("fork/r", true)), ..pr() }),
            "fork:feature"
        );
        assert_eq!(head_branch_label(&GhPullRequest { head_ref_name: "main".into(), ..pr() }), "o:main");
    }

    #[test]
    fn knows_when_the_head_branch_is_checked_out() {
        let remotes = [remote("o", "r", "origin"), remote("fork", "r", "fork")];
        let head = |h: Option<&str>, upstream: Option<&str>, commit: &str| BranchInfo {
            head: h.map(str::to_owned),
            head_commit: Some(commit.to_owned()),
            upstream: upstream.map(str::to_owned),
            ahead: 0,
            behind: 0,
            operation: None,
        };
        let check = |b: BranchInfo, pr: &GhPullRequest| is_head_checked_out(Some(&b), &remotes, pr);
        assert!(check(head(Some("feature"), Some("origin/feature"), "x"), &pr()));
        assert!(!check(head(Some("feature"), Some("fork/feature"), "x"), &pr()));
        assert!(!check(head(Some("other"), Some("origin/other"), "x"), &pr()));
        assert!(!check(head(None, None, "x"), &pr()));
        // gh's fork checkouts track a URL: the name and the head commit decide.
        let fork = GhPullRequest { head_repository: Some(repo("someone/r", true)), ..pr() };
        assert!(check(head(Some("someone/feature"), None, "head"), &fork));
        assert!(!check(head(Some("feature"), None, "older"), &fork));
    }

    #[test]
    fn labels_and_steps_through_the_commits() {
        let i18n = I18n::default();
        assert_eq!(commits_title(1, &i18n), "Changes");
        assert_eq!(commits_title(3, &i18n), "Changes from");
        assert_eq!(step_commit(None, 3, true), None);
        assert_eq!(step_commit(Some(0), 3, true), Some(1));
        assert_eq!(step_commit(Some(2), 3, true), None);
        assert_eq!(step_commit(Some(0), 3, false), None);
        assert_eq!(step_commit(Some(2), 3, false), Some(1));
    }

    fn files() -> Vec<CommitFile> {
        vec![
            file("src/a.ts", CommitFileStatus::Modified),
            file("src/b.ts", CommitFileStatus::Added),
            file("README.md", CommitFileStatus::Removed),
        ]
    }

    #[test]
    fn groups_by_directory_or_lists_flat() {
        let names = |nodes: Vec<FileTreeNode<CommitFile>>| nodes.into_iter().map(|n| n.name).collect::<Vec<_>>();
        assert_eq!(names(changes_tree(&files(), true)), ["src", "README.md"]);
        assert_eq!(names(changes_tree(&files(), false)), ["a.ts", "b.ts", "README.md"]);
    }

    #[test]
    fn counts_unresolved_threads_and_the_viewed_state_folders_summing_their_files() {
        let thread = |path: &str, is_resolved| GhReviewThread { path: path.into(), is_resolved, ..Default::default() };
        let threads = unresolved_threads_by_path(&[
            thread("src/a.ts", false),
            thread("src/a.ts", false),
            thread("src/b.ts", true),
        ]);
        assert_eq!(threads, [("src/a.ts".to_owned(), 2)]);
        let viewed = |path: &str| match path {
            "src/a.ts" => Some(FileViewedState::Viewed),
            "src/b.ts" => Some(FileViewedState::Dismissed),
            _ => None,
        };
        let leaf = |f: &CommitFile| ProgressState {
            is_read: is_viewed(viewed(&f.filename)),
            discussions: threads.iter().find(|(p, _)| *p == f.filename).map_or(0, |(_, n)| *n),
        };
        let roots = changes_tree(&files(), true);
        assert_eq!(progress_of(&roots[0], &leaf), ProgressState { is_read: false, discussions: 2 });
        assert_eq!(progress_of(&roots[0].children[0], &leaf), ProgressState { is_read: true, discussions: 2 });
        assert_eq!(progress_of(&roots[1], &leaf), ProgressState { is_read: true, discussions: 0 });
    }

    #[test]
    fn collects_the_files_under_the_selection() {
        let roots = changes_tree(&files(), true);
        let keys = |ks: &[&str]| ks.iter().map(|k| (*k).to_owned()).collect::<HashSet<_>>();
        let names = |fs: Vec<&CommitFile>| fs.into_iter().map(|f| f.filename.clone()).collect::<Vec<_>>();
        assert_eq!(names(selected_files(&roots, &keys(&["d:src", "f:src/a.ts"]))), ["src/a.ts", "src/b.ts"]);
        assert_eq!(names(selected_files(&roots, &keys(&["f:README.md"]))), ["README.md"]);
    }

    #[test]
    fn colours_by_vcs_status_and_words_the_viewed_toggle() {
        let i18n = I18n::default();
        assert_eq!(change_kind(CommitFileStatus::Added), EffectiveKind::Added);
        assert_eq!(change_kind(CommitFileStatus::Removed), EffectiveKind::Deleted);
        assert_eq!(change_kind(CommitFileStatus::Renamed), EffectiveKind::Modified);
        assert_eq!(viewed_toggle_text(1, false, &i18n), "Mark as Viewed");
        assert_eq!(viewed_toggle_text(1, true, &i18n), "Mark as Not Viewed");
        assert_eq!(viewed_toggle_text(2, false, &i18n), "Mark All as Viewed");
        assert_eq!(viewed_toggle_text(2, true, &i18n), "Mark All as Not Viewed");
    }
}
