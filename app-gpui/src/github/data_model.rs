//! Rules of the pull request data, after IntelliJ: the list search value and its query
//! (`GHPRListSearchValue`, `GHPRSearchQuery`), reviewer states (`GHReviewersUtils`), the unread
//! rule (`GHPRPersistentInteractionState`), the merge box state (`GHPRMergeabilityStateBuilder`),
//! permissions (`GHPRSecurityService`), merge commit messages and repository names.

use super::time::timestamp_ms;
use super::types::{
    CheckConclusion, ChecksState, CiJob, CiJobState, Coordinates, GhActor, GhPullRequestShort, GhRepository, GhReview,
    GhReviewer, GhUser, MergeStateStatus, MergeabilityData, MergeabilityState, Mergeable, Nodes, Permission, PrState,
    ReviewRequest, ReviewState, ReviewerState, StatusContextState, ThreadStatus,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// List search (`GHPRListSearchValue`)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchState {
    Open,
    Closed,
    Merged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchReviewState {
    NoReview,
    Required,
    Approved,
    ChangesRequested,
    ReviewedByMe,
    NotReviewedByMe,
    AwaitingReview,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SearchSort {
    Newest,
    Oldest,
    MostCommented,
    LeastCommented,
    RecentlyUpdated,
    LeastRecentlyUpdated,
}

/// The list filter: free text plus the chips (author / assignee are logins). Unset = `None`.
/// Equality is IntelliJ's "same search": every field equal.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchValue {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<SearchState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub review_state: Option<SearchReviewState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<SearchSort>,
}

impl SearchValue {
    /// `GHPRListSearchValue.DEFAULT`: the Quick Filters' "Open" (`EMPTY` is the default value).
    pub fn default_search() -> Self {
        Self { state: Some(SearchState::Open), ..Self::default() }
    }
}

fn state_term(s: SearchState) -> &'static str {
    match s {
        SearchState::Open => "is:open",
        SearchState::Closed => "is:closed",
        SearchState::Merged => "is:merged",
    }
}

fn review_term(s: SearchReviewState) -> &'static str {
    match s {
        SearchReviewState::NoReview => "review:none",
        SearchReviewState::Required => "review:required",
        SearchReviewState::Approved => "review:approved",
        SearchReviewState::ChangesRequested => "review:changes-requested",
        SearchReviewState::ReviewedByMe => "reviewed-by:@me",
        SearchReviewState::NotReviewedByMe => "-reviewed-by:@me",
        SearchReviewState::AwaitingReview => "review-requested:@me",
    }
}

fn sort_term(s: SearchSort) -> &'static str {
    match s {
        SearchSort::Newest => "sort:created-desc",
        SearchSort::Oldest => "sort:created-asc",
        SearchSort::MostCommented => "sort:comments-desc",
        SearchSort::LeastCommented => "sort:comments-asc",
        SearchSort::RecentlyUpdated => "sort:updated-desc",
        SearchSort::LeastRecentlyUpdated => "sort:updated-asc",
    }
}

/// How many filters are set (the text counts as one): the Quick Filters badge and "Clear {n} Filters".
pub fn filter_count(v: &SearchValue) -> usize {
    [
        v.search_query.is_some(),
        v.state.is_some(),
        v.assignee.is_some(),
        v.review_state.is_some(),
        v.author.is_some(),
        v.label.is_some(),
        v.sort.is_some(),
    ]
    .into_iter()
    .filter(|set| *set)
    .count()
}

/// A search value with only known, non-empty fields (from storage); `None` for anything but an object.
pub fn normalize_search(v: &Value) -> Option<SearchValue> {
    let o = v.as_object()?;
    fn text(o: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
        o.get(key).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_owned)
    }
    fn one_of<T: for<'de> Deserialize<'de>>(o: &serde_json::Map<String, Value>, key: &str) -> Option<T> {
        o.get(key).filter(|x| x.is_string()).and_then(|x| serde_json::from_value(x.clone()).ok())
    }
    Some(SearchValue {
        search_query: text(o, "searchQuery"),
        state: one_of(o, "state"),
        assignee: text(o, "assignee"),
        review_state: one_of(o, "reviewState"),
        author: text(o, "author"),
        label: text(o, "label"),
        sort: one_of(o, "sort"),
    })
}

/// The filter's search terms (`GHPRListSearchValue.toQuery`), or `None` when nothing is set. The
/// typed text is grouped in parentheses: GitHub's search gives `OR` the lowest precedence, so
/// `a OR b` would otherwise escape the repository and the other filters.
pub fn search_terms(v: &SearchValue) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();
    if let Some(text) = v.search_query.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        terms.push(format!("({text})"));
    }
    if let Some(s) = v.state {
        terms.push(state_term(s).to_owned());
    }
    if let Some(a) = &v.assignee {
        terms.push(format!("assignee:{a}"));
    }
    if let Some(r) = v.review_state {
        terms.push(review_term(r).to_owned());
    }
    if let Some(a) = &v.author {
        terms.push(format!("author:{a}"));
    }
    if let Some(l) = &v.label {
        terms.push(format!("label:\"{l}\""));
    }
    if let Some(s) = v.sort {
        terms.push(sort_term(s).to_owned());
    }
    (!terms.is_empty()).then(|| terms.join(" "))
}

/// The `issueSearch` query of the list: `type:pr repo:owner/name` and the filter's terms.
pub fn list_search_query(repo: &Coordinates, v: &SearchValue) -> String {
    let base = format!("type:pr repo:{}/{}", repo.owner, repo.name);
    match search_terms(v) {
        Some(terms) => format!("{base} {terms}"),
        None => base,
    }
}

// ---------------------------------------------------------------------------
// Reviewers (`GHReviewersUtils`)

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewerWithState {
    pub reviewer: GhReviewer,
    pub state: ReviewerState,
}

/// A reviewer's effective state from their reviews (newest opinionated review wins, a dismissed
/// one counts as a comment); `None` without a submitted review.
pub fn reviewer_state_of(reviews: &[&GhReview]) -> Option<ReviewerState> {
    let mut newest_first: Vec<&GhReview> = reviews.to_vec();
    newest_first.sort_by_key(|r| std::cmp::Reverse(timestamp_ms(&r.created_at)));
    let mut commented = false;
    for review in newest_first {
        match review.state {
            ReviewState::Approved => return Some(ReviewerState::Approved),
            ReviewState::ChangesRequested => return Some(ReviewerState::ChangesRequested),
            ReviewState::Dismissed => return Some(ReviewerState::Commented),
            ReviewState::Commented => commented = true,
            ReviewState::Pending => {}
        }
    }
    commented.then_some(ReviewerState::Commented)
}

/// Placeholder author of reviews whose user was deleted (IntelliJ's ghost user).
pub fn ghost_user() -> GhUser {
    GhActor {
        typename: "User".into(),
        id: String::new(),
        login: "ghost".into(),
        url: "https://github.com/ghost".into(),
        avatar_url: "https://avatars.githubusercontent.com/u/10137?v=4".into(),
        name: Some("Deleted user".into()),
    }
}

/// What [`reviewers_states`] reads of a pull request.
#[derive(Clone, Copy, Debug)]
pub struct ReviewersInput<'a> {
    pub author: Option<&'a GhActor>,
    pub reviews: &'a [GhReview],
    pub review_requests: &'a [ReviewRequest],
}

impl<'a> From<&'a GhPullRequestShort> for ReviewersInput<'a> {
    fn from(pr: &'a GhPullRequestShort) -> Self {
        Self { author: pr.author.as_ref(), reviews: &pr.reviews.nodes, review_requests: &pr.review_requests.nodes }
    }
}

/// Every reviewer with their state, as GitHub shows them: reviewers by their reviews (the author's
/// own excluded), then the pending requests, which make a reviewer who already reviewed
/// RE_REQUESTED and one who did not NEEDS_REVIEW. Order: first review or request.
pub fn reviewers_states(pr: ReviewersInput<'_>) -> Vec<ReviewerWithState> {
    let mut by_reviewer: Vec<(GhReviewer, Vec<&GhReview>)> = Vec::new();
    for review in pr.reviews {
        let reviewer = GhReviewer::Actor(review.author.clone().unwrap_or_else(ghost_user));
        match by_reviewer.iter_mut().find(|(r, _)| r.key() == reviewer.key()) {
            Some((_, reviews)) => reviews.push(review),
            None => by_reviewer.push((reviewer, vec![review])),
        }
    }
    let author_key = pr.author.map(|a| a.login.as_str());
    let mut result: Vec<ReviewerWithState> = Vec::new();
    for (reviewer, reviews) in by_reviewer {
        if Some(reviewer.key()) == author_key {
            continue;
        }
        if let Some(state) = reviewer_state_of(&reviews) {
            result.push(ReviewerWithState { reviewer, state });
        }
    }
    for requested in pr.review_requests.iter().filter_map(|r| r.requested_reviewer.as_ref()) {
        match result.iter_mut().find(|r| r.reviewer.key() == requested.key()) {
            Some(existing) => {
                existing.reviewer = requested.clone();
                existing.state = ReviewerState::ReRequested;
            }
            None => result.push(ReviewerWithState { reviewer: requested.clone(), state: ReviewerState::NeedsReview }),
        }
    }
    result
}

fn reviewer_order(s: ReviewerState) -> u8 {
    match s {
        ReviewerState::Approved => 0,
        ReviewerState::ChangesRequested => 1,
        ReviewerState::Commented => 2,
        ReviewerState::ReRequested => 3,
        ReviewerState::NeedsReview => 4,
    }
}

/// [`reviewers_states`] ordered like the list row's avatars: approved → changes requested →
/// commented → re-requested → needs review.
pub fn sorted_reviewers_states(pr: ReviewersInput<'_>) -> Vec<ReviewerWithState> {
    let mut states = reviewers_states(pr);
    states.sort_by_key(|s| reviewer_order(s.state));
    states
}

/// Review threads that are neither resolved nor outdated (the list row's comment counter).
pub fn unresolved_threads_count(threads: &Nodes<ThreadStatus>) -> usize {
    threads.nodes.iter().filter(|t| !t.is_resolved && !t.is_outdated).count()
}

// ---------------------------------------------------------------------------
// Unread (`GHPRPersistentInteractionState`)

/// Registry `github.last.seen.state.margin.millis`.
pub const LAST_SEEN_MARGIN_MS: i64 = 5000;
/// Registry `github.clear.last.seen.state.days`.
pub const CLEAR_LAST_SEEN_AFTER_DAYS: i64 = 14;

/// Whether `pr` counts as seen by the user `user_id`: always, unless they wrote it or their review
/// is requested; then when it was last seen (Unix ms, stored locally) no earlier than its update.
pub fn is_seen(pr: &GhPullRequestShort, user_id: &str, last_seen: Option<i64>) -> bool {
    let involved = pr.author.as_ref().is_some_and(|a| a.id == user_id)
        || pr.review_requests.nodes.iter().any(|r| r.requested_reviewer.as_ref().is_some_and(|r| r.id() == user_id));
    if !involved {
        return true;
    }
    match (last_seen, timestamp_ms(&pr.updated_at)) {
        (Some(seen), Some(updated)) => seen + LAST_SEEN_MARGIN_MS >= updated,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Pull request state

/// `GHUIUtil.getPullRequestStateIcon`.
pub fn pull_request_state_icon(state: PrState, is_draft: bool) -> &'static str {
    match state {
        PrState::Merged => "github-pr-merged",
        PrState::Closed => "github-pr-closed",
        PrState::Open if is_draft => "github-pr-draft",
        PrState::Open => "github-pr-open",
    }
}

/// The state tag's text, or `None` for an open pull request (draft wins over open).
pub fn state_tag_text(state: PrState, is_draft: bool) -> Option<&'static str> {
    match state {
        PrState::Merged => Some("MERGED"),
        PrState::Closed => Some("CLOSED"),
        PrState::Open => is_draft.then_some("DRAFT"),
    }
}

// ---------------------------------------------------------------------------
// Mergeability (`GHPRMergeabilityStateBuilder`)

fn context_job_state(s: StatusContextState) -> CiJobState {
    match s {
        StatusContextState::Pending => CiJobState::Pending,
        StatusContextState::Success => CiJobState::Success,
        _ => CiJobState::Failed,
    }
}

fn check_run_job_state(c: Option<CheckConclusion>) -> CiJobState {
    match c {
        None => CiJobState::Pending,
        Some(CheckConclusion::Skipped) => CiJobState::Skipped,
        Some(CheckConclusion::Success) => CiJobState::Success,
        Some(_) => CiJobState::Failed,
    }
}

pub fn build_mergeability_state(data: &MergeabilityData, current_user_is_admin: bool) -> MergeabilityState {
    let rule = data.base_ref.as_ref().and_then(|b| b.ref_update_rule.as_ref());
    let can_override_as_admin = rule.is_some() && current_user_is_admin;
    let required_contexts: Vec<&str> = rule
        .map(|r| r.required_status_check_contexts.iter().flatten().map(String::as_str).collect())
        .unwrap_or_default();
    let is_restricted = rule.is_some_and(|r| !r.viewer_can_push);
    let required_reviews = rule.and_then(|r| r.required_approving_review_count).unwrap_or(0);

    let has_conflicts = match data.mergeable {
        Mergeable::Mergeable => Some(false),
        Mergeable::Conflicting => Some(true),
        Mergeable::Unknown => None,
    };
    let last_commit = data.commits.nodes.last().map(|n| &n.commit);
    let contexts = last_commit.and_then(|c| c.status.as_ref()).map(|s| s.contexts.as_slice()).unwrap_or_default();
    let mut ci_jobs: Vec<CiJob> = contexts
        .iter()
        .map(|c| CiJob {
            name: c.context.clone(),
            status: context_job_state(c.state),
            is_required: c.is_required,
            details_url: c.target_url.clone(),
        })
        .collect();
    if let Some(suites) = last_commit.and_then(|c| c.check_suites.as_ref()) {
        for run in suites.nodes.iter().filter_map(|s| s.check_runs.as_ref()).flat_map(|r| &r.nodes) {
            ci_jobs.push(CiJob {
                name: run.name.clone(),
                status: check_run_job_state(run.conclusion),
                is_required: run.is_required,
                details_url: Some(run.details_url.clone().unwrap_or_else(|| run.url.clone())),
            });
        }
    }
    let status = data.merge_state_status;
    let can_merge = matches!(status, MergeStateStatus::Clean | MergeStateStatus::HasHooks | MergeStateStatus::Unstable);
    let admin_can_merge = matches!(status, MergeStateStatus::Blocked | MergeStateStatus::Behind);
    let can_be_merged = can_merge || (admin_can_merge && can_override_as_admin);
    let checks_state = match status {
        MergeStateStatus::Behind => Some(ChecksState::BlockingBehind),
        MergeStateStatus::Blocked => {
            let passed =
                |name: &str| contexts.iter().any(|c| c.state == StatusContextState::Success && c.context == name);
            (!required_contexts.is_empty() && !required_contexts.iter().all(|c| passed(c)))
                .then_some(ChecksState::BlockingFailing)
        }
        _ => None,
    };
    let required_approving_reviews_count = if status == MergeStateStatus::Blocked
        && !is_restricted
        && checks_state != Some(ChecksState::BlockingFailing)
    {
        required_reviews
    } else {
        0
    };
    MergeabilityState {
        has_conflicts,
        ci_jobs,
        can_be_merged,
        can_be_rebased: data.can_be_rebased,
        is_restricted,
        required_approving_reviews_count,
        checks_state,
        mergeable: data.mergeable,
        merge_state_status: status,
    }
}

// ---------------------------------------------------------------------------
// Permissions (`GHPRSecurityService`)

/// Whether `permission` is at least `level` (an unknown permission has none).
pub fn has_permission(permission: Option<Permission>, level: Permission) -> bool {
    permission.is_some_and(|p| p >= level)
}

/// The repository settings the merge actions depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepoMergeSettings {
    pub viewer_permission: Option<Permission>,
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
}

impl From<&GhRepository> for RepoMergeSettings {
    fn from(r: &GhRepository) -> Self {
        Self {
            viewer_permission: r.viewer_permission,
            merge_commit_allowed: r.merge_commit_allowed,
            squash_merge_allowed: r.squash_merge_allowed,
            rebase_merge_allowed: r.rebase_merge_allowed,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrPermissions {
    pub can_manage: bool,
    pub can_merge: bool,
    pub can_squash_merge: bool,
    pub can_rebase_merge: bool,
}

/// Which pull request actions the user may run: managing (request reviewers, close, reopen, post a
/// draft) needs triage access or authorship; merging needs write access, a method the repository
/// allows, a mergeable state, an unrestricted base branch, and `merge_forbidden` (the project
/// setting) off.
pub fn pull_request_permissions(
    viewer_did_author: bool,
    state: PrState,
    repo: RepoMergeSettings,
    mergeability: Option<&MergeabilityState>,
    merge_forbidden: bool,
) -> PrPermissions {
    let can_manage = has_permission(repo.viewer_permission, Permission::Triage) || viewer_did_author;
    let can_write = has_permission(repo.viewer_permission, Permission::Write) && !merge_forbidden;
    let open = state == PrState::Open;
    let mergeable = mergeability.filter(|m| open && can_write && !m.is_restricted);
    PrPermissions {
        can_manage,
        can_merge: mergeable.is_some_and(|m| repo.merge_commit_allowed && m.can_be_merged),
        can_squash_merge: mergeable.is_some_and(|m| repo.squash_merge_allowed && m.can_be_merged),
        can_rebase_merge: mergeable.is_some_and(|m| repo.rebase_merge_allowed && m.can_be_rebased),
    }
}

// ---------------------------------------------------------------------------
// Merge commit messages

/// The merge dialog's message: subject `Merge pull request #n`, body the title, or `* headline`
/// per commit when squashing.
pub fn merge_commit_message(number: u32, title: &str, squash_commit_headlines: Option<&[String]>) -> String {
    let body = match squash_commit_headlines {
        Some(headlines) => format!("* {}", headlines.join("\n\n* ")),
        None => title.to_owned(),
    };
    format!("Merge pull request #{number}\n\n{body}")
}

/// A commit message's subject (first line) and body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitMessage {
    pub title: String,
    pub body: String,
}

/// Splits a commit message into its subject (first line) and body.
pub fn split_commit_message(message: &str) -> CommitMessage {
    match message.split_once('\n') {
        Some((title, body)) => CommitMessage { title: title.trim().to_owned(), body: body.trim().to_owned() },
        None => CommitMessage { title: message.trim().to_owned(), body: String::new() },
    }
}

// ---------------------------------------------------------------------------
// Repositories

/// `GHUIUtil.getRepositoryDisplayName`: `name`, or `owner/name` when the owners differ (or always).
pub fn repository_display_name(all: &[Coordinates], repo: &Coordinates, always_show_owner: bool) -> String {
    let show_owner = always_show_owner || (all.len() > 1 && all.iter().any(|r| r.owner != all[0].owner));
    if show_owner { repo.name_with_owner() } else { repo.name.clone() }
}

/// `https?://[user@]`, `ssh://[user@]`, `git://` or scp-like `[user@]`, github.com, an optional
/// port (only before `/`), then `owner/name[.git][/]`.
static GITHUB_REMOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^(?:https?://(?:[^@/]+@)?|ssh://(?:[^@/]+@)?|git://|(?:[^@/]+@)?)",
        r"(?:www\.)?github\.com(?::[0-9]+/[:/]*|[:/]+)([^/]+)/([^/]+?)(?:\.git)?/?$",
    ))
    .expect("valid regex")
});

/// `owner/name` of a github.com remote URL (https, ssh or scp-like), else `None`.
pub fn parse_github_remote(url: &str) -> Option<Coordinates> {
    let m = GITHUB_REMOTE.captures(url.trim())?;
    Some(Coordinates::new(&m[1], &m[2]))
}

/// The remote of `repo` among `remotes`, by the URL's path: gh also resolves a remote behind an
/// SSH host alias (`git@work-github:o/r.git`), which [`parse_github_remote`] can't tell is GitHub.
pub fn remote_of_repository<'a>(remotes: &'a [ide_vcs::Remote], repo: &Coordinates) -> Option<&'a ide_vcs::Remote> {
    let path = repo.name_with_owner().to_lowercase();
    remotes.iter().find(|r| {
        let url = r.url.trim().to_lowercase();
        let url = url.strip_suffix('/').unwrap_or(&url);
        let url = url.strip_suffix(".git").unwrap_or(url);
        url.ends_with(&format!("/{path}")) || url.ends_with(&format!(":{path}"))
    })
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::super::types::*;

    pub fn user(login: &str) -> GhUser {
        user_named(login, None)
    }

    pub fn user_named(login: &str, name: Option<&str>) -> GhUser {
        GhActor {
            typename: "User".into(),
            id: format!("id-{login}"),
            login: login.into(),
            url: format!("https://github.com/{login}"),
            avatar_url: String::new(),
            name: name.map(str::to_owned),
        }
    }

    pub fn team() -> GhTeam {
        GhTeam {
            id: "T1".into(),
            slug: "core".into(),
            url: String::new(),
            team_avatar_url: None,
            team_name: "Core".into(),
            combined_slug: "org/core".into(),
        }
    }

    pub fn short_pr() -> GhPullRequestShort {
        GhPullRequestShort {
            id: "PR1".into(),
            url: "https://github.com/o/r/pull/1".into(),
            number: 1,
            title: "Fix".into(),
            state: PrState::Open,
            author: Some(user("ann")),
            created_at: "2026-10-01T09:00:00Z".into(),
            updated_at: "2026-10-01T12:00:00Z".into(),
            mergeable: Mergeable::Mergeable,
            viewer_can_react: true,
            ..GhPullRequestShort::default()
        }
    }

    pub fn requested(r: GhReviewer) -> ReviewRequest {
        ReviewRequest { requested_reviewer: Some(r) }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::github::types::*;
    use std::cell::Cell;

    thread_local! { static SEQ: Cell<u32> = const { Cell::new(0) }; }

    fn review(author: Option<GhActor>, state: ReviewState, minute: u32) -> GhReview {
        let n = SEQ.with(|s| {
            s.set(s.get() + 1);
            s.get()
        });
        GhReview {
            id: format!("R{n}"),
            author,
            state,
            created_at: format!("2026-10-01T10:{minute:02}:00Z"),
            ..GhReview::default()
        }
    }

    fn state_of(reviews: &[GhReview]) -> Option<ReviewerState> {
        reviewer_state_of(&reviews.iter().collect::<Vec<_>>())
    }

    #[test]
    fn builds_intellijs_terms_in_order() {
        let v = SearchValue {
            search_query: Some("foo bar".into()),
            state: Some(SearchState::Merged),
            assignee: Some("me".into()),
            review_state: Some(SearchReviewState::NotReviewedByMe),
            author: Some("ann".into()),
            label: Some("good first issue".into()),
            sort: Some(SearchSort::LeastRecentlyUpdated),
        };
        assert_eq!(
            search_terms(&v).as_deref(),
            Some(
                r#"(foo bar) is:merged assignee:me -reviewed-by:@me author:ann label:"good first issue" sort:updated-asc"#
            )
        );
        let awaiting = SearchValue { review_state: Some(SearchReviewState::AwaitingReview), ..SearchValue::default() };
        assert_eq!(search_terms(&awaiting).as_deref(), Some("review-requested:@me"));
        assert_eq!(search_terms(&SearchValue::default()), None);
    }

    #[test]
    fn prefixes_the_repository_for_issue_search() {
        let repo = Coordinates::new("o", "r");
        assert_eq!(list_search_query(&repo, &SearchValue::default_search()), "type:pr repo:o/r is:open");
        assert_eq!(list_search_query(&repo, &SearchValue::default()), "type:pr repo:o/r");
    }

    #[test]
    fn keeps_githubs_boolean_syntax_inside_the_repository() {
        let q = "is:pr is:open (review-requested:ann OR assignee:@ann)";
        let v = SearchValue { search_query: Some(q.into()), ..SearchValue::default() };
        assert_eq!(list_search_query(&Coordinates::new("o", "r"), &v), format!("type:pr repo:o/r ({q})"));
        let blank = SearchValue { search_query: Some("   ".into()), ..SearchValue::default() };
        assert_eq!(search_terms(&blank), None);
    }

    #[test]
    fn counts_filters_and_normalizes_stored_values() {
        let v = SearchValue {
            search_query: Some("x".into()),
            state: Some(SearchState::Open),
            sort: Some(SearchSort::Newest),
            ..SearchValue::default()
        };
        assert_eq!(filter_count(&v), 3);
        let stored =
            serde_json::json!({ "state": "OPEN", "sort": "SIDEWAYS", "label": "", "author": "ann", "extra": 1 });
        assert_eq!(
            normalize_search(&stored),
            Some(SearchValue { state: Some(SearchState::Open), author: Some("ann".into()), ..SearchValue::default() })
        );
        assert_eq!(normalize_search(&serde_json::json!("nope")), None);
    }

    #[test]
    fn lets_the_newest_opinionated_review_win_over_later_comments() {
        let bo = || Some(user("bo"));
        assert_eq!(
            state_of(&[review(bo(), ReviewState::Approved, 1), review(bo(), ReviewState::Commented, 2)]),
            Some(ReviewerState::Approved)
        );
        assert_eq!(
            state_of(&[review(bo(), ReviewState::Approved, 1), review(bo(), ReviewState::ChangesRequested, 2)]),
            Some(ReviewerState::ChangesRequested)
        );
    }

    #[test]
    fn surfaces_a_dismissed_latest_review_as_a_comment() {
        let bo = || Some(user("bo"));
        assert_eq!(
            state_of(&[review(bo(), ReviewState::Approved, 1), review(bo(), ReviewState::Dismissed, 2)]),
            Some(ReviewerState::Commented)
        );
    }

    #[test]
    fn skips_reviewers_with_only_pending_reviews() {
        let bo = || Some(user("bo"));
        assert_eq!(state_of(&[review(bo(), ReviewState::Pending, 1)]), None);
        assert_eq!(
            state_of(&[review(bo(), ReviewState::Pending, 2), review(bo(), ReviewState::Commented, 1)]),
            Some(ReviewerState::Commented)
        );
    }

    #[test]
    fn excludes_the_author_and_marks_requests_as_re_requested_or_needing_review() {
        let pr = GhPullRequestShort {
            reviews: Nodes::of(vec![
                review(Some(user("ann")), ReviewState::Commented, 1),
                review(Some(user("bo")), ReviewState::Approved, 2),
                review(Some(user("cy")), ReviewState::Commented, 3),
            ]),
            review_requests: Nodes::of(vec![
                requested(GhReviewer::Actor(user("cy"))),
                requested(GhReviewer::Team(team())),
                ReviewRequest { requested_reviewer: None },
            ]),
            ..short_pr()
        };
        let states = reviewers_states((&pr).into());
        let pairs: Vec<(&str, ReviewerState)> =
            states.iter().map(|s| (if s.reviewer.is_team() { "team" } else { s.reviewer.key() }, s.state)).collect();
        assert_eq!(
            pairs,
            [("bo", ReviewerState::Approved), ("cy", ReviewerState::ReRequested), ("team", ReviewerState::NeedsReview)]
        );
    }

    #[test]
    fn counts_reviews_of_deleted_users_under_the_ghost_user() {
        let pr = GhPullRequestShort { reviews: Nodes::of(vec![review(None, ReviewState::Approved, 1)]), ..short_pr() };
        let states = reviewers_states((&pr).into());
        assert_eq!(states.len(), 1);
        assert_eq!(states[0].reviewer.key(), "ghost");
    }

    #[test]
    fn orders_the_rows_avatars_approved_changes_commented_re_requested_needs_review() {
        let pr = GhPullRequestShort {
            reviews: Nodes::of(vec![
                review(Some(user("c")), ReviewState::Commented, 1),
                review(Some(user("x")), ReviewState::ChangesRequested, 2),
                review(Some(user("a")), ReviewState::Approved, 3),
            ]),
            review_requests: Nodes::of(vec![requested(GhReviewer::Actor(user("n")))]),
            ..short_pr()
        };
        let states: Vec<ReviewerState> = sorted_reviewers_states((&pr).into()).iter().map(|s| s.state).collect();
        assert_eq!(
            states,
            [
                ReviewerState::Approved,
                ReviewerState::ChangesRequested,
                ReviewerState::Commented,
                ReviewerState::NeedsReview
            ]
        );
    }

    #[test]
    fn counts_unresolved_current_threads() {
        let threads = Nodes::of(vec![
            ThreadStatus { is_resolved: false, is_outdated: false },
            ThreadStatus { is_resolved: true, is_outdated: false },
            ThreadStatus { is_resolved: false, is_outdated: true },
        ]);
        assert_eq!(unresolved_threads_count(&threads), 1);
    }

    fn updated() -> i64 {
        timestamp_ms("2026-10-01T12:00:00Z").expect("timestamp")
    }

    #[test]
    fn only_applies_to_pull_requests_the_user_wrote_or_is_asked_to_review() {
        assert!(is_seen(&short_pr(), "id-me", None));
        assert!(!is_seen(&GhPullRequestShort { author: Some(user("me")), ..short_pr() }, "id-me", None));
        let asked = GhPullRequestShort {
            review_requests: Nodes::of(vec![requested(GhReviewer::Actor(user("me")))]),
            ..short_pr()
        };
        assert!(!is_seen(&asked, "id-me", None));
    }

    #[test]
    fn compares_the_last_seen_time_with_a_5_s_margin_with_the_update() {
        let mine = GhPullRequestShort { author: Some(user("me")), ..short_pr() };
        assert!(is_seen(&mine, "id-me", Some(updated() - 4000)));
        assert!(!is_seen(&mine, "id-me", Some(updated() - 6000)));
        assert!(is_seen(&mine, "id-me", Some(updated() + 1)));
    }

    fn data() -> MergeabilityData {
        let context = |name: &str, state, required, url: Option<&str>| StatusContext {
            context: name.into(),
            state,
            is_required: required,
            target_url: url.map(str::to_owned),
        };
        let run = |name: &str, conclusion, required, details: Option<&str>, url: &str| CheckRun {
            name: name.into(),
            conclusion,
            is_required: required,
            details_url: details.map(str::to_owned),
            url: url.into(),
        };
        MergeabilityData {
            mergeable: Mergeable::Mergeable,
            can_be_rebased: true,
            merge_state_status: MergeStateStatus::Clean,
            base_ref: None,
            commits: Connection {
                page_info: PageInfo::default(),
                nodes: vec![MergeabilityCommit {
                    id: "c".into(),
                    commit: CommitWithChecks {
                        id: "c".into(),
                        oid: "abc".into(),
                        abbreviated_oid: "abc".into(),
                        status: Some(CommitStatus {
                            contexts: vec![
                                context("ci/a", StatusContextState::Success, true, Some("u1")),
                                context("ci/b", StatusContextState::Error, false, None),
                            ],
                        }),
                        check_suites: Some(Connection {
                            page_info: PageInfo::default(),
                            nodes: vec![CheckSuite {
                                check_runs: Some(Connection {
                                    page_info: PageInfo::default(),
                                    nodes: vec![
                                        run("build", None, false, None, "run"),
                                        run("lint", Some(CheckConclusion::Skipped), false, Some("d"), "run2"),
                                        run("test", Some(CheckConclusion::TimedOut), true, None, "run3"),
                                    ],
                                }),
                            }],
                        }),
                    },
                }],
            },
        }
    }

    fn rule() -> RefUpdateRule {
        RefUpdateRule {
            pattern: "main".into(),
            required_approving_review_count: Some(2),
            required_status_check_contexts: vec![Some("ci/a".into())],
            viewer_can_push: true,
            ..RefUpdateRule::default()
        }
    }

    fn with(status: MergeStateStatus, rule: Option<RefUpdateRule>) -> MergeabilityData {
        MergeabilityData {
            merge_state_status: status,
            base_ref: rule.map(|r| BaseRefRule { ref_update_rule: Some(r) }),
            ..data()
        }
    }

    #[test]
    fn maps_statuses_and_check_runs_to_ci_jobs() {
        let s = build_mergeability_state(&data(), false);
        let jobs: Vec<(&str, CiJobState, Option<&str>)> =
            s.ci_jobs.iter().map(|j| (j.name.as_str(), j.status, j.details_url.as_deref())).collect();
        assert_eq!(
            jobs,
            [
                ("ci/a", CiJobState::Success, Some("u1")),
                ("ci/b", CiJobState::Failed, None),
                ("build", CiJobState::Pending, Some("run")),
                ("lint", CiJobState::Skipped, Some("d")),
                ("test", CiJobState::Failed, Some("run3")),
            ]
        );
        assert_eq!(s.has_conflicts, Some(false));
        assert!(s.can_be_merged);
        assert_eq!(s.checks_state, None);
        assert_eq!(s.required_approving_reviews_count, 0);
    }

    #[test]
    fn follows_the_merge_state_and_branch_protection() {
        let unknown = MergeabilityData { mergeable: Mergeable::Unknown, ..data() };
        assert_eq!(build_mergeability_state(&unknown, false).has_conflicts, None);
        let dirty = MergeabilityData {
            mergeable: Mergeable::Conflicting,
            merge_state_status: MergeStateStatus::Dirty,
            ..data()
        };
        let s = build_mergeability_state(&dirty, false);
        assert_eq!((s.has_conflicts, s.can_be_merged), (Some(true), false));

        let blocked = with(MergeStateStatus::Blocked, Some(rule()));
        let s = build_mergeability_state(&blocked, false);
        assert_eq!((s.can_be_merged, s.checks_state, s.required_approving_reviews_count), (false, None, 2));
        assert!(build_mergeability_state(&blocked, true).can_be_merged);

        let failing = with(
            MergeStateStatus::Blocked,
            Some(RefUpdateRule { required_status_check_contexts: vec![Some("ci/b".into())], ..rule() }),
        );
        let s = build_mergeability_state(&failing, false);
        assert_eq!((s.checks_state, s.required_approving_reviews_count), (Some(ChecksState::BlockingFailing), 0));
        assert_eq!(
            build_mergeability_state(&with(MergeStateStatus::Behind, None), false).checks_state,
            Some(ChecksState::BlockingBehind)
        );
        let restricted = with(MergeStateStatus::Blocked, Some(RefUpdateRule { viewer_can_push: false, ..rule() }));
        let s = build_mergeability_state(&restricted, false);
        assert_eq!((s.is_restricted, s.required_approving_reviews_count), (true, 0));
    }

    #[test]
    fn decides_the_merge_actions_from_permission_settings_and_state() {
        let repo = RepoMergeSettings {
            viewer_permission: Some(Permission::Write),
            merge_commit_allowed: true,
            squash_merge_allowed: false,
            rebase_merge_allowed: true,
        };
        let state = build_mergeability_state(&data(), false);
        assert_eq!(
            pull_request_permissions(false, PrState::Open, repo, Some(&state), false),
            PrPermissions { can_manage: true, can_merge: true, can_squash_merge: false, can_rebase_merge: true }
        );
        assert!(!pull_request_permissions(false, PrState::Open, repo, Some(&state), true).can_merge);
        let reader = RepoMergeSettings { viewer_permission: Some(Permission::Read), ..repo };
        let p = pull_request_permissions(true, PrState::Open, reader, Some(&state), false);
        assert_eq!((p.can_manage, p.can_merge), (true, false));
        assert!(!pull_request_permissions(false, PrState::Open, reader, Some(&state), false).can_manage);
        assert!(has_permission(Some(Permission::Maintain), Permission::Write));
        assert!(!has_permission(None, Permission::Read));
    }

    #[test]
    fn names_states_like_intellij() {
        assert_eq!(state_tag_text(PrState::Open, false), None);
        assert_eq!(state_tag_text(PrState::Open, true), Some("DRAFT"));
        assert_eq!(state_tag_text(PrState::Merged, true), Some("MERGED"));
        assert_eq!(pull_request_state_icon(PrState::Open, true), "github-pr-draft");
        assert_eq!(pull_request_state_icon(PrState::Closed, false), "github-pr-closed");
    }

    #[test]
    fn prefills_and_splits_merge_commit_messages() {
        assert_eq!(merge_commit_message(7, "Fix it", None), "Merge pull request #7\n\nFix it");
        assert_eq!(
            merge_commit_message(7, "x", Some(&["a".to_owned(), "b".to_owned()])),
            "Merge pull request #7\n\n* a\n\n* b"
        );
        assert_eq!(
            split_commit_message("Subject\n\nBody\nmore"),
            CommitMessage { title: "Subject".into(), body: "Body\nmore".into() }
        );
        assert_eq!(split_commit_message("Only"), CommitMessage { title: "Only".into(), body: String::new() });
    }

    #[test]
    fn shows_the_owner_only_when_owners_differ() {
        let a = Coordinates::new("me", "app");
        let b = Coordinates::new("org", "app");
        assert_eq!(repository_display_name(std::slice::from_ref(&a), &a, false), "app");
        assert_eq!(repository_display_name(&[a.clone(), b], &a, false), "me/app");
        assert_eq!(repository_display_name(&[a.clone(), Coordinates::new("me", "lib")], &a, false), "app");
        assert_eq!(repository_display_name(std::slice::from_ref(&a), &a, true), "me/app");
    }

    #[test]
    fn finds_the_remote_behind_an_ssh_host_alias_by_the_urls_path() {
        let repo = Coordinates::new("Org", "app");
        let remote = |name: &str, url: &str| ide_vcs::Remote { name: name.into(), url: url.into() };
        let remotes =
            [remote("other", "git@work-github:org/app2.git"), remote("origin", "git@work-github:org/app.git")];
        assert_eq!(remote_of_repository(&remotes, &repo), Some(&remotes[1]));
        let slash = [remote("x", "ssh://work-github/org/app/")];
        assert_eq!(remote_of_repository(&slash, &repo).map(|r| r.name.as_str()), Some("x"));
        assert_eq!(remote_of_repository(&[remote("x", "git@work-github:notorg/app.git")], &repo), None);
    }
}
