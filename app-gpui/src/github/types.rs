//! Shapes of the GitHub data the GraphQL documents and REST calls answer with, named after
//! IntelliJ's DTOs (`GHPullRequestShort`, `GHPullRequest`, …). The backend hands these over as
//! the JSON text of `gh api`; [`crate::github::api::graphql_data`] reads them. GraphQL
//! connections keep their `nodes`; [`crate::github::data_model`] derives what the UI shows.

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Common

/// `pageInfo` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PageInfo {
    pub start_cursor: Option<String>,
    pub has_previous_page: bool,
    pub end_cursor: Option<String>,
    pub has_next_page: bool,
}

/// A GraphQL connection with its page info.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection<T> {
    #[serde(default)]
    pub page_info: PageInfo,
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
}

impl<T> Default for Connection<T> {
    fn default() -> Self {
        Self { page_info: PageInfo::default(), nodes: Vec::new() }
    }
}

/// A GraphQL connection read without its page info.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nodes<T> {
    #[serde(default = "Vec::new")]
    pub nodes: Vec<T>,
}

impl<T> Default for Nodes<T> {
    fn default() -> Self {
        Self { nodes: Vec::new() }
    }
}

impl<T> Nodes<T> {
    pub fn of(nodes: Vec<T>) -> Self {
        Self { nodes }
    }
}

/// `actorInfo` / `user`: what every actor has. `name` comes with users, organizations and
/// enterprise accounts. `typename` is `User`, `Bot`, `Organization`, `EnterpriseUserAccount`,
/// `Mannequin`, …
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhActor {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: String,
    pub login: String,
    pub url: String,
    pub avatar_url: String,
    pub name: Option<String>,
}

/// A user (`user` fragment); the same shape as any actor.
pub type GhUser = GhActor;

/// `teamInfo` fragment (aliased `teamAvatarUrl`, `teamName`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhTeam {
    pub id: String,
    pub slug: String,
    pub url: String,
    pub team_avatar_url: Option<String>,
    pub team_name: String,
    /// `org/slug`.
    pub combined_slug: String,
}

/// `pullRequestReviewer`: a team, or a user (GitHub Apps: another actor).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum GhReviewer {
    Team(GhTeam),
    Actor(GhActor),
}

impl<'de> Deserialize<'de> for GhReviewer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("__typename").and_then(serde_json::Value::as_str) == Some("Team") {
            serde_json::from_value(value).map(Self::Team).map_err(D::Error::custom)
        } else {
            serde_json::from_value(value).map(Self::Actor).map_err(D::Error::custom)
        }
    }
}

impl GhReviewer {
    pub fn is_team(&self) -> bool {
        matches!(self, Self::Team(_))
    }

    /// The node id of the user or team.
    pub fn id(&self) -> &str {
        match self {
            Self::Team(team) => &team.id,
            Self::Actor(actor) => &actor.id,
        }
    }

    /// Login of a user / actor, `org/slug` of a team.
    pub fn key(&self) -> &str {
        match self {
            Self::Team(team) => &team.combined_slug,
            Self::Actor(actor) => &actor.login,
        }
    }
}

/// `labelInfo` fragment. `color` is hex without `#`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GhLabel {
    pub id: String,
    pub url: String,
    pub name: String,
    pub color: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReactionContent {
    ThumbsUp,
    ThumbsDown,
    Laugh,
    Hooray,
    Confused,
    Heart,
    Rocket,
    Eyes,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GhReaction {
    pub content: ReactionContent,
    #[serde(default)]
    pub user: Option<GhUser>,
}

/// `gitActorInfo`: commit author / committer.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhGitActor {
    pub name: Option<String>,
    pub email: Option<String>,
    pub avatar_url: String,
    pub date: Option<String>,
    pub user: Option<GhUser>,
}

/// `commitHash` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhCommitHash {
    pub id: String,
    pub oid: String,
    pub abbreviated_oid: String,
}

/// `commitShort` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhCommitShort {
    pub id: String,
    pub oid: String,
    pub abbreviated_oid: String,
    pub url: String,
    pub message_headline: String,
    pub author: Option<GhGitActor>,
}

// ---------------------------------------------------------------------------
// Repository

/// `RepositoryPermission`, lowest first (the order [`crate::github::data_model::has_permission`] compares).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Permission {
    Read,
    Triage,
    Write,
    Maintain,
    Admin,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RepositoryOwner {
    #[serde(rename = "__typename")]
    pub typename: String,
    pub id: String,
    pub login: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RefName {
    pub name: String,
}

/// `repository` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhRepository {
    pub id: String,
    pub owner: RepositoryOwner,
    pub name_with_owner: String,
    pub viewer_permission: Option<Permission>,
    pub merge_commit_allowed: bool,
    pub squash_merge_allowed: bool,
    pub rebase_merge_allowed: bool,
    pub default_branch_ref: Option<RefName>,
    pub is_fork: bool,
    pub url: String,
    pub ssh_url: String,
}

/// Where a request goes: `owner/name` on github.com.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Coordinates {
    pub owner: String,
    pub name: String,
}

impl Coordinates {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Self {
        Self { owner: owner.into(), name: name.into() }
    }

    /// `owner/name`.
    pub fn name_with_owner(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    /// The same repository, ignoring case (GitHub's names are case-insensitive).
    pub fn same(&self, other: &Coordinates) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner) && self.name.eq_ignore_ascii_case(&other.name)
    }

    /// `owner/name` parsed; `None` unless it is exactly two non-empty parts without spaces.
    pub fn parse(name_with_owner: &str) -> Option<Self> {
        let (owner, name) = name_with_owner.split_once('/')?;
        let valid = |part: &str| !part.is_empty() && !part.contains('/') && !part.contains(char::is_whitespace);
        (valid(owner) && valid(name)).then(|| Self::new(owner, name))
    }
}

// ---------------------------------------------------------------------------
// Pull requests

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrState {
    #[default]
    Open,
    Closed,
    Merged,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Mergeable {
    Mergeable,
    Conflicting,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewDecision {
    Approved,
    ChangesRequested,
    ReviewRequired,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    #[default]
    Pending,
    Commented,
    Approved,
    ChangesRequested,
    Dismissed,
}

/// `PullRequestReviewEvent` of `submitReview`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
    Dismiss,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeStateStatus {
    Behind,
    Blocked,
    Clean,
    Dirty,
    Draft,
    HasHooks,
    #[default]
    Unknown,
    Unstable,
}

/// `pullRequestReview` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhReview {
    pub id: String,
    pub url: String,
    pub author: Option<GhActor>,
    pub body: String,
    pub state: ReviewState,
    pub created_at: String,
    pub viewer_can_update: bool,
}

/// A pending review request.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReviewRequest {
    pub requested_reviewer: Option<GhReviewer>,
}

/// The list row's view of a review thread.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ThreadStatus {
    pub is_resolved: bool,
    pub is_outdated: bool,
}

/// `pullRequestInfoShort` fragment: a list row (`issueSearch`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhPullRequestShort {
    pub id: String,
    pub url: String,
    pub number: u32,
    pub title: String,
    pub state: PrState,
    pub is_draft: bool,
    pub author: Option<GhActor>,
    pub created_at: String,
    pub updated_at: String,
    pub is_read_by_viewer: bool,
    pub assignees: Nodes<GhUser>,
    pub labels: Nodes<GhLabel>,
    pub review_requests: Nodes<ReviewRequest>,
    pub review_threads: Nodes<ThreadStatus>,
    pub reviews: Nodes<GhReview>,
    pub reactions: Connection<GhReaction>,
    pub mergeable: Mergeable,
    pub viewer_can_update: bool,
    pub viewer_can_react: bool,
    pub viewer_did_author: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeId {
    pub id: String,
}

/// `pullRequestInfo` fragment: the details (`findPullRequest`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhPullRequest {
    #[serde(flatten)]
    pub short: GhPullRequestShort,
    pub body: String,
    pub review_decision: Option<ReviewDecision>,
    pub base_ref_name: String,
    pub base_ref_oid: String,
    pub base_repository: Option<GhRepository>,
    /// `None` once the head branch is deleted.
    pub head_ref: Option<NodeId>,
    pub head_ref_name: String,
    pub head_ref_oid: String,
    pub head_repository: Option<GhRepository>,
    pub viewer_can_delete_head_ref: bool,
}

// ---------------------------------------------------------------------------
// Mergeability (`findPullRequestMergeability`)

/// `refUpdateRule` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RefUpdateRule {
    pub allows_deletions: bool,
    pub allows_force_pushes: bool,
    pub pattern: String,
    pub required_approving_review_count: Option<u32>,
    pub required_status_check_contexts: Vec<Option<String>>,
    pub requires_linear_history: bool,
    pub requires_signatures: bool,
    pub viewer_can_push: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StatusContextState {
    Error,
    Expected,
    Failure,
    #[default]
    Pending,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckConclusion {
    ActionRequired,
    Cancelled,
    Failure,
    Neutral,
    Skipped,
    Stale,
    StartupFailure,
    Success,
    TimedOut,
}

/// `commitStatusContext` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StatusContext {
    pub context: String,
    pub state: StatusContextState,
    pub is_required: bool,
    pub target_url: Option<String>,
}

/// `checkRun` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CheckRun {
    pub name: String,
    pub conclusion: Option<CheckConclusion>,
    pub is_required: bool,
    pub details_url: Option<String>,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitStatus {
    pub contexts: Vec<StatusContext>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CheckSuite {
    pub check_runs: Option<Connection<CheckRun>>,
}

/// `commitWithCheckStatuses` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CommitWithChecks {
    pub id: String,
    pub oid: String,
    pub abbreviated_oid: String,
    pub status: Option<CommitStatus>,
    pub check_suites: Option<Connection<CheckSuite>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BaseRefRule {
    pub ref_update_rule: Option<RefUpdateRule>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MergeabilityCommit {
    pub id: String,
    pub commit: CommitWithChecks,
}

/// `pullRequestMergeability` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MergeabilityData {
    pub mergeable: Mergeable,
    pub can_be_rebased: bool,
    pub merge_state_status: MergeStateStatus,
    pub base_ref: Option<BaseRefRule>,
    pub commits: Connection<MergeabilityCommit>,
}

// ---------------------------------------------------------------------------
// Files, reviews, threads

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileViewedState {
    Viewed,
    #[default]
    Unviewed,
    Dismissed,
}

/// `pullRequestFiles` node.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChangedFile {
    pub path: String,
    pub viewer_viewed_state: FileViewedState,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommitFileStatus {
    Added,
    Removed,
    #[default]
    Modified,
    Renamed,
    Copied,
    Changed,
    Unchanged,
}

/// REST `pulls/{n}/files` / `commits/{sha}` entry (IntelliJ's `GHCommitFile`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CommitFile {
    pub sha: Option<String>,
    pub filename: String,
    pub status: CommitFileStatus,
    pub additions: u32,
    pub deletions: u32,
    pub changes: u32,
    /// Absent for binary and very large files.
    pub patch: Option<String>,
    pub previous_filename: Option<String>,
}

/// `pullRequestPendingReview`, flattened like IntelliJ's `GHPullRequestPendingReview`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PendingReview {
    pub id: String,
    pub state: ReviewState,
    pub comments_count: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewCommentState {
    Pending,
    #[default]
    Submitted,
}

/// `pullRequestReviewComment` fragment.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhReviewComment {
    pub id: String,
    pub author: Option<GhActor>,
    pub body: String,
    pub created_at: String,
    pub reactions: Connection<GhReaction>,
    pub url: String,
    pub state: ReviewCommentState,
    pub commit: Option<GhCommitHash>,
    pub original_commit: Option<GhCommitHash>,
    pub diff_hunk: String,
    pub pull_request_review: Option<NodeId>,
    pub viewer_can_delete: bool,
    pub viewer_can_update: bool,
    pub viewer_can_react: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DiffSide {
    Left,
    #[default]
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SubjectType {
    #[default]
    Line,
    File,
}

/// `pullRequestReviewThread` fragment. Lines are 1-based; `line` is `None` when outdated.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhReviewThread {
    pub id: String,
    pub is_resolved: bool,
    pub is_outdated: bool,
    pub path: String,
    pub subject_type: SubjectType,
    pub diff_side: DiffSide,
    pub line: Option<u32>,
    pub original_line: Option<u32>,
    pub start_diff_side: Option<DiffSide>,
    pub start_line: Option<u32>,
    pub original_start_line: Option<u32>,
    pub comments: Nodes<GhReviewComment>,
    pub viewer_can_reply: bool,
    pub viewer_can_resolve: bool,
    pub viewer_can_unresolve: bool,
}

/// `DraftPullRequestReviewThread` input of `createReview`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftReviewThread {
    pub body: String,
    pub path: String,
    pub line: u32,
    pub side: DiffSide,
    pub start_line: Option<u32>,
    pub start_side: Option<DiffSide>,
}

// ---------------------------------------------------------------------------
// Timeline (`pullRequestTimeline`)

/// `issueCommentInfo`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GhIssueComment {
    pub id: String,
    pub author: Option<GhActor>,
    pub body: String,
    pub created_at: String,
    pub reactions: Connection<GhReaction>,
    pub viewer_can_update: bool,
    pub viewer_can_delete: bool,
    pub viewer_can_react: bool,
}

/// A commit of the timeline.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TimelineCommit {
    pub id: String,
    pub commit: GhCommitShort,
    pub url: String,
}

/// `referenceSubject`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "__typename")]
pub enum ReferencedSubject {
    #[serde(rename_all = "camelCase")]
    PullRequest { title: String, number: u32, pr_state: PrState, is_draft: bool, url: String },
    #[serde(rename_all = "camelCase")]
    Issue { title: String, number: u32, issue_state: String, url: String },
}

/// What every event has: who and when.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventInfo {
    pub actor: Option<GhActor>,
    pub created_at: String,
}

/// An event with its own fields `T`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event<T> {
    #[serde(flatten)]
    pub info: EventInfo,
    #[serde(flatten)]
    pub data: T,
}

/// An event without fields of its own.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoFields {}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Renamed {
    pub previous_title: String,
    pub current_title: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Merged {
    pub merged_event_commit: Option<GhCommitShort>,
    pub merge_ref_name: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserChange {
    pub user: Option<GhUser>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LabelChange {
    pub label: GhLabel,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReviewerChange {
    pub requested_reviewer: Option<GhReviewer>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DismissedReview {
    pub author: Option<GhActor>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReviewDismissed {
    #[serde(rename = "dismissalMessageHTML")]
    pub dismissal_message_html: Option<String>,
    pub review: Option<DismissedReview>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ForcePushed {
    #[serde(rename = "ref")]
    pub git_ref: Option<RefName>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeadForcePushed {
    #[serde(rename = "ref")]
    pub git_ref: Option<RefName>,
    pub before_commit: Option<GhCommitHash>,
    pub after_commit: Option<GhCommitHash>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeadRefDeleted {
    pub head_ref_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrossReferenced {
    pub source: ReferencedSubject,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubjectChange {
    pub subject: ReferencedSubject,
}

/// A timeline item, by its `__typename`. [`crate::github::api::timeline_items`] reads them,
/// dropping the types IntelliJ ignores and keeping other types as [`TimelineItem::Unknown`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "__typename")]
pub enum TimelineItem {
    PullRequestCommit(TimelineCommit),
    PullRequestReview(GhReview),
    IssueComment(GhIssueComment),
    RenamedTitleEvent(Event<Renamed>),
    ClosedEvent(Event<NoFields>),
    ReopenedEvent(Event<NoFields>),
    MergedEvent(Event<Merged>),
    AssignedEvent(Event<UserChange>),
    UnassignedEvent(Event<UserChange>),
    LabeledEvent(Event<LabelChange>),
    UnlabeledEvent(Event<LabelChange>),
    ReviewRequestedEvent(Event<ReviewerChange>),
    ReviewRequestRemovedEvent(Event<ReviewerChange>),
    ReviewDismissedEvent(Event<ReviewDismissed>),
    ReadyForReviewEvent(Event<NoFields>),
    ConvertToDraftEvent(Event<NoFields>),
    BaseRefChangedEvent(Event<NoFields>),
    BaseRefForcePushedEvent(Event<ForcePushed>),
    HeadRefForcePushedEvent(Event<HeadForcePushed>),
    HeadRefDeletedEvent(Event<HeadRefDeleted>),
    HeadRefRestoredEvent(Event<NoFields>),
    CrossReferencedEvent(Event<CrossReferenced>),
    ConnectedEvent(Event<SubjectChange>),
    DisconnectedEvent(Event<SubjectChange>),
    /// A type the timeline document selects no fields for (`PinnedEvent`, `AutoMergeEnabledEvent`, …).
    #[serde(skip)]
    Unknown {
        type_name: String,
    },
}

impl TimelineItem {
    /// The `__typename` of the item (`Unknown` for unknown types).
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::PullRequestCommit(_) => "PullRequestCommit",
            Self::PullRequestReview(_) => "PullRequestReview",
            Self::IssueComment(_) => "IssueComment",
            Self::RenamedTitleEvent(_) => "RenamedTitleEvent",
            Self::ClosedEvent(_) => "ClosedEvent",
            Self::ReopenedEvent(_) => "ReopenedEvent",
            Self::MergedEvent(_) => "MergedEvent",
            Self::AssignedEvent(_) => "AssignedEvent",
            Self::UnassignedEvent(_) => "UnassignedEvent",
            Self::LabeledEvent(_) => "LabeledEvent",
            Self::UnlabeledEvent(_) => "UnlabeledEvent",
            Self::ReviewRequestedEvent(_) => "ReviewRequestedEvent",
            Self::ReviewRequestRemovedEvent(_) => "ReviewRequestRemovedEvent",
            Self::ReviewDismissedEvent(_) => "ReviewDismissedEvent",
            Self::ReadyForReviewEvent(_) => "ReadyForReviewEvent",
            Self::ConvertToDraftEvent(_) => "ConvertToDraftEvent",
            Self::BaseRefChangedEvent(_) => "BaseRefChangedEvent",
            Self::BaseRefForcePushedEvent(_) => "BaseRefForcePushedEvent",
            Self::HeadRefForcePushedEvent(_) => "HeadRefForcePushedEvent",
            Self::HeadRefDeletedEvent(_) => "HeadRefDeletedEvent",
            Self::HeadRefRestoredEvent(_) => "HeadRefRestoredEvent",
            Self::CrossReferencedEvent(_) => "CrossReferencedEvent",
            Self::ConnectedEvent(_) => "ConnectedEvent",
            Self::DisconnectedEvent(_) => "DisconnectedEvent",
            Self::Unknown { .. } => "Unknown",
        }
    }

    /// Who and when, for events (commits, reviews and comments have their own).
    pub fn event_info(&self) -> Option<&EventInfo> {
        Some(match self {
            Self::RenamedTitleEvent(e) => &e.info,
            Self::ClosedEvent(e)
            | Self::ReopenedEvent(e)
            | Self::ReadyForReviewEvent(e)
            | Self::ConvertToDraftEvent(e)
            | Self::BaseRefChangedEvent(e)
            | Self::HeadRefRestoredEvent(e) => &e.info,
            Self::MergedEvent(e) => &e.info,
            Self::AssignedEvent(e) | Self::UnassignedEvent(e) => &e.info,
            Self::LabeledEvent(e) | Self::UnlabeledEvent(e) => &e.info,
            Self::ReviewRequestedEvent(e) | Self::ReviewRequestRemovedEvent(e) => &e.info,
            Self::ReviewDismissedEvent(e) => &e.info,
            Self::BaseRefForcePushedEvent(e) => &e.info,
            Self::HeadRefForcePushedEvent(e) => &e.info,
            Self::HeadRefDeletedEvent(e) => &e.info,
            Self::CrossReferencedEvent(e) => &e.info,
            Self::ConnectedEvent(e) | Self::DisconnectedEvent(e) => &e.info,
            Self::PullRequestCommit(_) | Self::PullRequestReview(_) | Self::IssueComment(_) | Self::Unknown { .. } => {
                return None;
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Derived state

/// IntelliJ's `GHPRReviewerState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReviewerState {
    Approved,
    ChangesRequested,
    Commented,
    NeedsReview,
    ReRequested,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CiJobState {
    Failed,
    Pending,
    Skipped,
    Success,
}

/// IntelliJ's `CodeReviewCIJob`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiJob {
    pub name: String,
    pub status: CiJobState,
    pub is_required: bool,
    pub details_url: Option<String>,
}

/// What blocks the merge box on checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksState {
    BlockingBehind,
    BlockingFailing,
}

/// IntelliJ's `GHPRMergeabilityState`, plus the raw GitHub values it was built from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeabilityState {
    /// `None` while GitHub is still checking (mergeable UNKNOWN).
    pub has_conflicts: Option<bool>,
    pub ci_jobs: Vec<CiJob>,
    pub can_be_merged: bool,
    pub can_be_rebased: bool,
    /// The viewer may not push to the base branch.
    pub is_restricted: bool,
    /// Approvals still required by branch protection (0 when nothing blocks on reviews).
    pub required_approving_reviews_count: u32,
    pub checks_state: Option<ChecksState>,
    pub mergeable: Mergeable,
    pub merge_state_status: MergeStateStatus,
}
