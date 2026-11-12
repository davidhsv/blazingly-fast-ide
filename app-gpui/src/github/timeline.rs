//! Rules of the timeline, after IntelliJ: merging the raw timeline items
//! (`GHPRTimelineMergingModel`, `GHPRTimelineMergedSimpleEvents`, `…StateEvents`), review statuses
//! (`GHPRTimelineItemComponentFactory.createReviewStatus`), a thread's diff hunk with its anchor
//! (`GHPRTimelineThreadViewModel.calcDiffWithAnchor`, `PatchHunkUtil`), suggested changes
//! (`GHPRReviewCommentBodyViewModel`, `GHSuggestedChange`), reactions (`GHReactionsViewModel`)
//! and the @mention completion of the comment editor.
//!
//! Lines are 0-based unless the name says otherwise (GitHub's are 1-based); carets and text
//! offsets are UTF-16 code units.

use super::time::timestamp_ms;
use super::types::{
    DiffSide, GhActor, GhIssueComment, GhLabel, GhReaction, GhReview, GhReviewThread, GhReviewer, GhUser, PrState,
    ReactionContent, ReviewState, TimelineCommit, TimelineItem,
};
use crate::everywhere::open::{byte_at_utf16, utf16_len};
use crate::platform::collate;
use crate::platform::i18n::I18n;
use regex::Regex;
use std::sync::LazyLock;

// ---------------------------------------------------------------------------
// Merging (`GHPRTimelineMergingModel`)

/// Events from one actor within this time are shown as one item.
pub const MERGE_THRESHOLD_MS: i64 = 2 * 60_000;

/// Merged simple events (one event is a group of one): what changed overall.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimpleGroup {
    pub key: String,
    pub actor: Option<GhActor>,
    pub created_at: String,
    /// Raw events in the group.
    pub count: usize,
    pub added_labels: Vec<GhLabel>,
    pub removed_labels: Vec<GhLabel>,
    pub assigned: Vec<GhUser>,
    pub unassigned: Vec<GhUser>,
    pub added_reviewers: Vec<Option<GhReviewer>>,
    pub removed_reviewers: Vec<Option<GhReviewer>>,
    /// `(from, to)`; with several events only shown when the title really changed ([`effective_rename`]).
    pub rename: Option<(String, String)>,
}

/// Merged state events: the resulting state and the event that set it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateGroup {
    pub key: String,
    pub actor: Option<GhActor>,
    pub created_at: String,
    pub count: usize,
    pub new_state: PrState,
    /// The event that set `new_state` (a merge stays the last state).
    pub last: TimelineItem,
    pub inferred_original_state: PrState,
}

/// An item as the timeline shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TimelineEntry {
    Commits {
        key: String,
        commits: Vec<TimelineCommit>,
    },
    Comment {
        key: String,
        comment: GhIssueComment,
    },
    Review {
        key: String,
        review: GhReview,
    },
    /// A branch or complex event (never merged).
    Event {
        key: String,
        event: TimelineItem,
    },
    Simple(SimpleGroup),
    State(StateGroup),
    Unknown {
        key: String,
        type_name: String,
    },
}

impl TimelineEntry {
    pub fn key(&self) -> &str {
        match self {
            Self::Commits { key, .. }
            | Self::Comment { key, .. }
            | Self::Review { key, .. }
            | Self::Event { key, .. }
            | Self::Unknown { key, .. } => key,
            Self::Simple(g) => &g.key,
            Self::State(g) => &g.key,
        }
    }
}

fn is_simple(item: &TimelineItem) -> bool {
    matches!(
        item,
        TimelineItem::LabeledEvent(_)
            | TimelineItem::UnlabeledEvent(_)
            | TimelineItem::AssignedEvent(_)
            | TimelineItem::UnassignedEvent(_)
            | TimelineItem::ReviewRequestedEvent(_)
            | TimelineItem::ReviewRequestRemovedEvent(_)
            | TimelineItem::RenamedTitleEvent(_)
    )
}

/// The state a state event sets; `None` for other items.
pub fn state_of(item: &TimelineItem) -> Option<PrState> {
    match item {
        TimelineItem::ClosedEvent(_) => Some(PrState::Closed),
        TimelineItem::MergedEvent(_) => Some(PrState::Merged),
        TimelineItem::ReopenedEvent(_) => Some(PrState::Open),
        _ => None,
    }
}

fn actor_key(a: Option<&GhActor>) -> Option<&str> {
    a.map(|a| a.login.as_str())
}

fn label_key(l: &GhLabel) -> String {
    if l.id.is_empty() { l.name.clone() } else { l.id.clone() }
}

fn user_key(u: &GhUser) -> String {
    if u.id.is_empty() { u.login.clone() } else { u.id.clone() }
}

fn reviewer_key(r: &Option<GhReviewer>) -> String {
    r.as_ref().map_or_else(|| "ghost".to_owned(), |r| r.key().to_owned())
}

/// Adds `item` unless the opposite list holds it (which then loses it): a label added and removed is no change.
fn toggle_in<T: Clone>(add: &mut Vec<T>, remove: &mut Vec<T>, item: &T, key: fn(&T) -> String) {
    let k = key(item);
    if let Some(i) = remove.iter().position(|r| key(r) == k) {
        remove.remove(i);
    } else if !add.iter().any(|a| key(a) == k) {
        add.push(item.clone());
    }
}

fn add_simple(g: &mut SimpleGroup, item: &TimelineItem) {
    match item {
        TimelineItem::LabeledEvent(e) => {
            toggle_in(&mut g.added_labels, &mut g.removed_labels, &e.data.label, label_key)
        }
        TimelineItem::UnlabeledEvent(e) => {
            toggle_in(&mut g.removed_labels, &mut g.added_labels, &e.data.label, label_key)
        }
        TimelineItem::AssignedEvent(e) => {
            if let Some(user) = &e.data.user {
                toggle_in(&mut g.assigned, &mut g.unassigned, user, user_key);
            }
        }
        TimelineItem::UnassignedEvent(e) => {
            if let Some(user) = &e.data.user {
                toggle_in(&mut g.unassigned, &mut g.assigned, user, user_key);
            }
        }
        TimelineItem::ReviewRequestedEvent(e) => {
            toggle_in(&mut g.added_reviewers, &mut g.removed_reviewers, &e.data.requested_reviewer, reviewer_key)
        }
        TimelineItem::ReviewRequestRemovedEvent(e) => {
            toggle_in(&mut g.removed_reviewers, &mut g.added_reviewers, &e.data.requested_reviewer, reviewer_key)
        }
        TimelineItem::RenamedTitleEvent(e) => {
            let from = g.rename.take().map_or_else(|| e.data.previous_title.clone(), |(from, _)| from);
            g.rename = Some((from, e.data.current_title.clone()));
        }
        _ => {}
    }
}

/// `GHPRTimelineMergedSimpleEvents.hasAnyChanges`.
pub fn simple_has_changes(g: &SimpleGroup) -> bool {
    !g.added_labels.is_empty()
        || !g.removed_labels.is_empty()
        || !g.assigned.is_empty()
        || !g.unassigned.is_empty()
        || !g.added_reviewers.is_empty()
        || !g.removed_reviewers.is_empty()
        || effective_rename(g).is_some()
}

/// The rename to show: a single event's always, a merged one's only when the title changed.
pub fn effective_rename(g: &SimpleGroup) -> Option<&(String, String)> {
    g.rename.as_ref().filter(|(from, to)| g.count == 1 || from != to)
}

/// Merged groups that change nothing (assigned and unassigned again, closed and reopened) are dropped.
fn is_collapsed_merge(e: &TimelineEntry) -> bool {
    match e {
        TimelineEntry::Simple(g) => g.count > 1 && !simple_has_changes(g),
        TimelineEntry::State(g) => g.count > 1 && g.new_state == g.inferred_original_state,
        _ => false,
    }
}

fn within_threshold(previous: &str, next: &str) -> bool {
    matches!((timestamp_ms(previous), timestamp_ms(next)), (Some(a), Some(b)) if b - a <= MERGE_THRESHOLD_MS)
}

fn later(a: &str, b: &str) -> String {
    match (timestamp_ms(a), timestamp_ms(b)) {
        (Some(x), Some(y)) if y > x => b.to_owned(),
        _ => a.to_owned(),
    }
}

/// The items as the timeline shows them: consecutive commits grouped, simple and state events of one
/// actor within [`MERGE_THRESHOLD_MS`] of the previous one merged, merges that change nothing
/// dropped. Unknown types are kept only with `show_unknown` (IntelliJ shows them in internal / EAP
/// builds); [`crate::github::api::timeline_items`] already drops the ignored types.
pub fn merge_timeline(items: &[TimelineItem], show_unknown: bool) -> Vec<TimelineEntry> {
    let mut out: Vec<TimelineEntry> = Vec::new();
    let mut last: Option<TimelineEntry> = None;
    fn push(out: &mut Vec<TimelineEntry>, e: Option<TimelineEntry>) {
        if let Some(e) = e.filter(|e| !is_collapsed_merge(e)) {
            out.push(e);
        }
    }
    for (index, item) in items.iter().enumerate() {
        if let TimelineItem::Unknown { type_name } = item {
            if show_unknown {
                push(&mut out, last.take());
                last = Some(TimelineEntry::Unknown { key: format!("unknown:{index}"), type_name: type_name.clone() });
            }
            continue;
        }
        if let TimelineItem::PullRequestCommit(commit) = item {
            if let Some(TimelineEntry::Commits { commits, .. }) = &mut last {
                commits.push(commit.clone());
            } else {
                push(&mut out, last.take());
                let key = format!("commits:{}", commit.id);
                last = Some(TimelineEntry::Commits { key, commits: vec![commit.clone()] });
            }
            continue;
        }
        let info = item.event_info();
        if let Some(info) = info.filter(|_| is_simple(item)) {
            if let Some(TimelineEntry::Simple(g)) = &mut last
                && actor_key(g.actor.as_ref()) == actor_key(info.actor.as_ref())
                && within_threshold(&g.created_at, &info.created_at)
            {
                g.count += 1;
                g.actor.clone_from(&info.actor);
                g.created_at = later(&g.created_at, &info.created_at);
                add_simple(g, item);
            } else {
                push(&mut out, last.take());
                let mut g = SimpleGroup {
                    key: format!("simple:{index}"),
                    actor: info.actor.clone(),
                    created_at: info.created_at.clone(),
                    count: 1,
                    added_labels: Vec::new(),
                    removed_labels: Vec::new(),
                    assigned: Vec::new(),
                    unassigned: Vec::new(),
                    added_reviewers: Vec::new(),
                    removed_reviewers: Vec::new(),
                    rename: None,
                };
                add_simple(&mut g, item);
                last = Some(TimelineEntry::Simple(g));
            }
            continue;
        }
        if let (Some(info), Some(state)) = (info, state_of(item)) {
            if let Some(TimelineEntry::State(g)) = &mut last
                && actor_key(g.actor.as_ref()) == actor_key(info.actor.as_ref())
                && within_threshold(&g.created_at, &info.created_at)
            {
                g.count += 1;
                g.actor.clone_from(&info.actor);
                g.created_at = later(&g.created_at, &info.created_at);
                if g.new_state != PrState::Merged {
                    g.new_state = state;
                    g.last = item.clone();
                }
            } else {
                push(&mut out, last.take());
                last = Some(TimelineEntry::State(StateGroup {
                    key: format!("state:{index}"),
                    actor: info.actor.clone(),
                    created_at: info.created_at.clone(),
                    count: 1,
                    new_state: state,
                    last: item.clone(),
                    inferred_original_state: if state == PrState::Open { PrState::Closed } else { PrState::Open },
                }));
            }
            continue;
        }
        push(&mut out, last.take());
        last = Some(match item {
            TimelineItem::IssueComment(c) => {
                TimelineEntry::Comment { key: format!("comment:{}", c.id), comment: c.clone() }
            }
            TimelineItem::PullRequestReview(r) => {
                TimelineEntry::Review { key: format!("review:{}", r.id), review: r.clone() }
            }
            other => TimelineEntry::Event { key: format!("event:{index}"), event: other.clone() },
        });
    }
    push(&mut out, last);
    out
}

/// The actor a group of commits is shown with: the only commit's author's user, else the pull
/// request's author.
pub fn commits_actor<'a>(commits: &'a [TimelineCommit], pr_author: Option<&'a GhActor>) -> Option<&'a GhActor> {
    match commits {
        [only] => only.commit.author.as_ref().and_then(|a| a.user.as_ref()).or(pr_author),
        _ => pr_author,
    }
}

// ---------------------------------------------------------------------------
// Reviews

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusType {
    Info,
    SecondaryInfo,
    Success,
    Error,
    Warning,
}

/// A review's status line: its text and colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewStatus {
    pub text: String,
    pub kind: StatusType,
}

pub fn review_status(state: ReviewState, i18n: &I18n) -> ReviewStatus {
    let (text, kind) = match state {
        ReviewState::Approved => ("approved the changes", StatusType::Success),
        ReviewState::ChangesRequested => ("requested changes", StatusType::Error),
        ReviewState::Pending => ("started a review", StatusType::SecondaryInfo),
        ReviewState::Commented | ReviewState::Dismissed => ("reviewed the changes", StatusType::Info),
    };
    ReviewStatus { text: i18n.tx(text), kind }
}

/// The review a thread belongs to (its first comment's review).
pub fn thread_review_id(t: &GhReviewThread) -> Option<&str> {
    t.comments.nodes.first().and_then(|c| c.pull_request_review.as_ref()).map(|r| r.id.as_str())
}

/// Whether a review can be edited: GitHub fails to edit reviews created with an empty body.
pub fn can_edit_review(r: &GhReview) -> bool {
    r.viewer_can_update && !r.body.is_empty()
}

/// `getPresentableName` of an actor: the name, else the login (`ghost` without an actor).
pub fn presentable_actor_name(a: Option<&GhActor>) -> &str {
    match a {
        None => "ghost",
        Some(a) => a.name.as_deref().filter(|n| !n.is_empty()).unwrap_or(&a.login),
    }
}

/// `getPresentableName` of a reviewer: users' name, else the login; teams' name, else the slug.
pub fn presentable_reviewer_name(r: Option<&GhReviewer>) -> &str {
    match r {
        Some(GhReviewer::Team(t)) => {
            if t.team_name.is_empty() {
                t.slug.as_str()
            } else {
                t.team_name.as_str()
            }
        }
        Some(GhReviewer::Actor(a)) => presentable_actor_name(Some(a)),
        None => "ghost",
    }
}

// ---------------------------------------------------------------------------
// Diff hunks (`PatchHunk`)

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkLineType {
    Context,
    Add,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkLine {
    pub kind: HunkLineType,
    pub text: String,
}

/// A unified diff hunk; start lines are 0-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
    pub start_line_before: u32,
    pub start_line_after: u32,
    pub lines: Vec<HunkLine>,
}

static HUNK_HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@@ -([0-9]+)(?:,[0-9]+)? \+([0-9]+)(?:,[0-9]+)? @@").expect("valid regex"));

fn normalize_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Parses GitHub's `diffHunk` (`@@ -a,b +c,d @@ …` then the lines); `None` when it has none.
pub fn parse_patch_hunk(diff_hunk: &str) -> Option<PatchHunk> {
    if diff_hunk.trim().is_empty() {
        return None;
    }
    let text = normalize_newlines(diff_hunk);
    let mut rows: Vec<&str> = text.split('\n').collect();
    if rows.len() > 1 && rows.last() == Some(&"") {
        rows.pop();
    }
    let header = HUNK_HEADER.captures(rows.first().copied().unwrap_or_default())?;
    let number = |i: usize| header[i].parse::<u32>().ok().map_or(0, |n| n.saturating_sub(1));
    let mut lines = Vec::new();
    for row in &rows[1..] {
        if row.starts_with('\\') {
            continue;
        }
        let (kind, rest) = if let Some(rest) = row.strip_prefix('+') {
            (HunkLineType::Add, rest)
        } else if let Some(rest) = row.strip_prefix('-') {
            (HunkLineType::Remove, rest)
        } else if let Some(rest) = row.strip_prefix(' ') {
            (HunkLineType::Context, rest)
        } else if row.is_empty() {
            (HunkLineType::Context, "")
        } else {
            continue;
        };
        lines.push(HunkLine { kind, text: rest.to_owned() });
    }
    Some(PatchHunk { start_line_before: number(1), start_line_after: number(2), lines })
}

/// `PatchHunkUtil.truncateHunkBefore`: drops the lines before index `start`, moving the start lines.
pub fn truncate_hunk_before(hunk: &PatchHunk, start: i64) -> PatchHunk {
    let Ok(start) = usize::try_from(start) else { return hunk.clone() };
    let start = start.min(hunk.lines.len());
    let dropped = &hunk.lines[..start];
    let count = |skip: HunkLineType| dropped.iter().filter(|l| l.kind != skip).count() as u32;
    PatchHunk {
        start_line_before: hunk.start_line_before + count(HunkLineType::Add),
        start_line_after: hunk.start_line_after + count(HunkLineType::Remove),
        lines: hunk.lines[start..].to_vec(),
    }
}

/// Lines of the timeline's thread diffs around the anchor (`TimelineDiffComponentFactory.DIFF_CONTEXT_SIZE`).
pub const DIFF_CONTEXT_SIZE: i64 = 3;

/// What [`thread_diff_with_anchor`] reads of a thread.
#[derive(Clone, Copy, Debug)]
pub struct ThreadAnchor<'a> {
    pub diff_hunk: &'a str,
    pub diff_side: DiffSide,
    pub original_line: Option<u32>,
    pub start_diff_side: Option<DiffSide>,
    pub original_start_line: Option<u32>,
}

impl<'a> From<&'a GhReviewThread> for ThreadAnchor<'a> {
    fn from(t: &'a GhReviewThread) -> Self {
        Self {
            diff_hunk: thread_diff_hunk(t),
            diff_side: t.diff_side,
            original_line: t.original_line,
            start_diff_side: t.start_diff_side,
            original_start_line: t.original_start_line,
        }
    }
}

/// A thread's diff and the anchor line range in it (`start..end` indexes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadDiff {
    pub hunk: PatchHunk,
    pub anchor: (usize, usize),
}

/// The thread's diff (the end of its hunk, where GitHub anchors the comment) and the anchor, as
/// `calcDiffWithAnchor`; `None` when the hunk is empty (file comments, some outdated threads).
pub fn thread_diff_with_anchor(thread: ThreadAnchor<'_>) -> Option<ThreadDiff> {
    let hunk = parse_patch_hunk(thread.diff_hunk).filter(|h| !h.lines.is_empty())?;
    let anchor = thread.original_line.map(|l| (thread.diff_side, i64::from(l) - 1));
    let start_anchor = thread.start_diff_side.zip(thread.original_start_line).map(|(side, l)| {
        let floor = if side == DiffSide::Left { hunk.start_line_before } else { hunk.start_line_after };
        (side, (i64::from(l) - 1).max(i64::from(floor)))
    });
    let anchor_length = if start_anchor.map(|a| a.0) == anchor.map(|a| a.0) {
        (anchor.map_or(0, |a| a.1) - start_anchor.map_or(0, |a| a.1)).max(0)
    } else {
        0
    };
    let truncated = truncate_hunk_before(&hunk, hunk.lines.len() as i64 - 1 - (anchor_length + DIFF_CONTEXT_SIZE));
    let len = truncated.lines.len() as i64;
    let start = usize::try_from(len - 1 - anchor_length).unwrap_or(0);
    Some(ThreadDiff { anchor: (start, truncated.lines.len()), hunk: truncated })
}

/// The diff hunk of a thread (its first comment's).
pub fn thread_diff_hunk(t: &GhReviewThread) -> &str {
    t.comments.nodes.first().map_or("", |c| c.diff_hunk.as_str())
}

/// Line numbers (1-based) of each hunk line on both sides; `None` where the line does not exist on that side.
pub fn hunk_line_numbers(hunk: &PatchHunk) -> Vec<(Option<u32>, Option<u32>)> {
    let mut before = hunk.start_line_before;
    let mut after = hunk.start_line_after;
    hunk.lines
        .iter()
        .map(|l| match l.kind {
            HunkLineType::Add => {
                after += 1;
                (None, Some(after))
            }
            HunkLineType::Remove => {
                before += 1;
                (Some(before), None)
            }
            HunkLineType::Context => {
                before += 1;
                after += 1;
                (Some(before), Some(after))
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Suggested changes

pub const SUGGESTION_BLOCK_START: &str = "```suggestion";
const SUGGESTION_BLOCK_END: &str = "```";

/// Whether a comment holds a suggested change (its range cannot be adjusted in the diff).
pub fn contains_suggested_change(markdown: &str) -> bool {
    markdown.split('\n').any(|l| l.strip_suffix('\r').unwrap_or(l).starts_with(SUGGESTION_BLOCK_START))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyBlock {
    Markdown(String),
    Suggestion(Vec<String>),
}

/// A comment body split into Markdown and ```` ```suggestion ```` blocks (`getSuggestions`: a
/// block starts with a line starting with the fence and ends at the next line ending with
/// ```` ``` ````). An unterminated block stays Markdown.
pub fn split_suggestions(body: &str) -> Vec<BodyBlock> {
    let text = normalize_newlines(body);
    let mut out = Vec::new();
    let mut markdown: Vec<&str> = Vec::new();
    let mut block: Option<Vec<&str>> = None;
    let mut opening = "";
    fn flush(out: &mut Vec<BodyBlock>, markdown: &mut Vec<&str>) {
        let joined = markdown.join("\n");
        if !joined.trim().is_empty() {
            out.push(BodyBlock::Markdown(joined));
        }
        markdown.clear();
    }
    for row in text.split('\n') {
        match block.take() {
            Some(lines) if row.ends_with(SUGGESTION_BLOCK_END) => {
                flush(&mut out, &mut markdown);
                out.push(BodyBlock::Suggestion(lines.into_iter().map(str::to_owned).collect()));
            }
            Some(mut lines) => {
                lines.push(row);
                block = Some(lines);
            }
            None if row.starts_with(SUGGESTION_BLOCK_START) => {
                block = Some(Vec::new());
                opening = row;
            }
            None => markdown.push(row),
        }
    }
    if let Some(lines) = block {
        markdown.push(opening);
        markdown.extend(lines);
    }
    flush(&mut out, &mut markdown);
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionApplicability {
    Applicable,
    Resolved,
    Outdated,
}

/// Where a suggestion applies: 0-based line indexes of the current and original code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionTarget {
    pub path: String,
    pub applicability: SuggestionApplicability,
    /// The current lines (`line` / `startLine`), `None` when outdated.
    pub start_line_index: Option<u32>,
    pub end_line_index: Option<u32>,
    /// The commented code as the diff hunk shows it (its RIGHT side).
    pub original_lines: Vec<String>,
}

/// What [`suggestion_target`] reads of a thread.
#[derive(Clone, Copy, Debug)]
pub struct SuggestionThread<'a> {
    pub is_resolved: bool,
    pub path: &'a str,
    pub line: Option<u32>,
    pub start_line: Option<u32>,
    pub original_line: Option<u32>,
    pub original_start_line: Option<u32>,
    pub diff_hunk: &'a str,
}

impl<'a> From<&'a GhReviewThread> for SuggestionThread<'a> {
    fn from(t: &'a GhReviewThread) -> Self {
        Self {
            is_resolved: t.is_resolved,
            path: &t.path,
            line: t.line,
            start_line: t.start_line,
            original_line: t.original_line,
            original_start_line: t.original_start_line,
            diff_hunk: thread_diff_hunk(t),
        }
    }
}

/// The lines the thread comments on and whether a suggestion in it can be applied.
pub fn suggestion_target(thread: SuggestionThread<'_>) -> SuggestionTarget {
    let original_end = thread.original_line.map(|l| l.saturating_sub(1));
    let original_start = thread.original_start_line.map(|l| l.saturating_sub(1)).or(original_end);
    let end = thread.line.map(|l| l.saturating_sub(1));
    let start = thread.start_line.map(|l| l.saturating_sub(1)).or(end);
    let original_lines = match parse_patch_hunk(thread.diff_hunk) {
        None => Vec::new(),
        Some(hunk) => match original_start.zip(original_end) {
            Some((from, to)) => hunk
                .lines
                .iter()
                .zip(hunk_line_numbers(&hunk))
                .filter(|(l, (_, n))| {
                    l.kind != HunkLineType::Remove && n.is_some_and(|n| (from..=to).contains(&(n - 1)))
                })
                .map(|(l, _)| l.text.clone())
                .collect(),
            None => hunk
                .lines
                .iter()
                .rev()
                .find(|l| l.kind != HunkLineType::Remove)
                .map(|l| l.text.clone())
                .into_iter()
                .collect(),
        },
    };
    let applicability = if thread.is_resolved {
        SuggestionApplicability::Resolved
    } else if original_start.is_none() || original_end.is_none() || start.is_none() || end.is_none() {
        SuggestionApplicability::Outdated
    } else {
        SuggestionApplicability::Applicable
    };
    SuggestionTarget {
        path: thread.path.to_owned(),
        applicability,
        start_line_index: start,
        end_line_index: end,
        original_lines,
    }
}

/// The disabled tooltip of "Apply Locally" (`None`: it can be applied).
pub fn suggestion_disabled_reason(
    applicability: SuggestionApplicability,
    on_review_branch: bool,
    i18n: &I18n,
) -> Option<String> {
    let reason = if !on_review_branch {
        "Unable to apply the suggested change from a pull request in a different branch"
    } else {
        match applicability {
            SuggestionApplicability::Resolved => "This suggestion has been resolved",
            SuggestionApplicability::Outdated => "Outdated suggestions cannot be applied",
            SuggestionApplicability::Applicable => return None,
        }
    };
    Some(i18n.tx(reason))
}

/// A git patch applying a suggestion, and where the new lines are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionPatch {
    pub patch: String,
    /// 0-based first line of the replaced code.
    pub start: usize,
    /// Number of new lines.
    pub lines: usize,
}

/// A git patch replacing the target lines of `text` (the local file) with `suggestion`, with up to
/// 3 lines of context. The lines are looked up at the thread's line first, then at the nearest
/// place holding the same code; an error when the file no longer has them.
pub fn build_suggestion_patch(
    text: &str,
    target: &SuggestionTarget,
    suggestion: &[String],
) -> Result<SuggestionPatch, String> {
    let Some(wanted) = target.start_line_index else {
        return Err("Outdated suggestions cannot be applied".to_owned());
    };
    let wanted = wanted as i64;
    let crlf = text.contains("\r\n");
    // Lines keep their "\r" so that the patch matches CRLF files.
    let mut lines: Vec<&str> = text.split('\n').collect();
    let trailing_newline = lines.last() == Some(&"");
    if trailing_newline {
        lines.pop();
    }
    let bare = |l: &str| l.strip_suffix('\r').unwrap_or(l).to_owned();
    let original = &target.original_lines;
    let matches_at = |at: i64| {
        at >= 0
            && (at as usize) + original.len() <= lines.len()
            && original.iter().enumerate().all(|(i, l)| bare(lines[at as usize + i]) == *l)
    };
    let mut at: Option<usize> = None;
    let mut d = 0i64;
    while !original.is_empty() && d <= lines.len() as i64 + wanted && at.is_none() {
        if matches_at(wanted - d) {
            at = Some((wanted - d) as usize);
        } else if d > 0 && matches_at(wanted + d) {
            at = Some((wanted + d) as usize);
        }
        d += 1;
    }
    let Some(at) = at else {
        return Err(format!("{}: the suggested change does not apply", target.path));
    };
    let ctx_start = at.saturating_sub(3);
    let ctx_end = (at + original.len() + 3).min(lines.len());
    let before = &lines[ctx_start..at];
    let removed = &lines[at..at + original.len()];
    let after = &lines[at + original.len()..ctx_end];
    let old_count = before.len() + removed.len() + after.len();
    let new_count = before.len() + suggestion.len() + after.len();
    let cr = if crlf { "\r" } else { "" };
    let mut body: Vec<String> = Vec::new();
    body.extend(before.iter().map(|l| format!(" {l}")));
    body.extend(removed.iter().map(|l| format!("-{l}")));
    body.extend(suggestion.iter().map(|l| format!("+{l}{cr}")));
    body.extend(after.iter().map(|l| format!(" {l}")));
    if ctx_end == lines.len() && !trailing_newline {
        body.push("\\ No newline at end of file".to_owned());
    }
    let range = |n: usize| format!("{},{n}", if n > 0 { ctx_start + 1 } else { ctx_start });
    let mut patch =
        format!("--- a/{path}\n+++ b/{path}\n@@ -{} +{} @@\n", range(old_count), range(new_count), path = target.path);
    for row in body {
        patch.push_str(&row);
        patch.push('\n');
    }
    Ok(SuggestionPatch { patch, start: at, lines: suggestion.len() })
}

// ---------------------------------------------------------------------------
// Reactions

/// A reaction as the picker shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReactionKind {
    pub content: ReactionContent,
    pub emoji: &'static str,
    pub name: &'static str,
}

/// GitHub's reactions, in its order, with the emoji and name (`presentableName`).
pub const REACTIONS: [ReactionKind; 8] = [
    ReactionKind { content: ReactionContent::ThumbsUp, emoji: "\u{1F44D}", name: "thumbs up" },
    ReactionKind { content: ReactionContent::ThumbsDown, emoji: "\u{1F44E}", name: "thumbs down" },
    ReactionKind { content: ReactionContent::Laugh, emoji: "\u{1F604}", name: "laugh" },
    ReactionKind { content: ReactionContent::Hooray, emoji: "\u{1F389}", name: "hooray" },
    ReactionKind { content: ReactionContent::Confused, emoji: "\u{1F615}", name: "confused" },
    ReactionKind { content: ReactionContent::Heart, emoji: "\u{2764}\u{FE0F}", name: "heart" },
    ReactionKind { content: ReactionContent::Rocket, emoji: "\u{1F680}", name: "rocket" },
    ReactionKind { content: ReactionContent::Eyes, emoji: "\u{1F440}", name: "eyes" },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReactionInfo {
    pub content: ReactionContent,
    pub users: Vec<GhUser>,
    pub is_reacted_by_current_user: bool,
}

/// `reactionsWithInfo`: reactions grouped by content, in order of first appearance.
pub fn group_reactions(reactions: &[GhReaction], current_user_id: Option<&str>) -> Vec<ReactionInfo> {
    let mut out: Vec<ReactionInfo> = Vec::new();
    for r in reactions {
        let i = match out.iter().position(|i| i.content == r.content) {
            Some(i) => i,
            None => {
                out.push(ReactionInfo { content: r.content, users: Vec::new(), is_reacted_by_current_user: false });
                out.len() - 1
            }
        };
        if let Some(user) = &r.user {
            out[i].users.push(user.clone());
            if current_user_id.is_some_and(|me| !me.is_empty() && me == user.id) {
                out[i].is_reacted_by_current_user = true;
            }
        }
    }
    out
}

/// The viewer's reaction toggled locally (the optimistic update before the reload).
pub fn toggle_reaction_locally(
    reactions: &[GhReaction],
    content: ReactionContent,
    me: &GhUser,
    add: bool,
) -> Vec<GhReaction> {
    if add {
        let mut out = reactions.to_vec();
        out.push(GhReaction { content, user: Some(me.clone()) });
        out
    } else {
        reactions
            .iter()
            .filter(|r| !(r.content == content && r.user.as_ref().is_some_and(|u| u.id == me.id)))
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// @mentions

/// A mention being typed: where its `@` is (UTF-16) and the text after it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionQuery {
    pub start: usize,
    pub query: String,
}

fn is_login_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// The `@query` being typed before the caret (UTF-16), with where it starts; `None` when not in a
/// mention. The `@` must start the text or follow a character that is neither a word character
/// nor a backtick (an e-mail address is no mention).
pub fn mention_query(text: &str, caret: usize) -> Option<MentionQuery> {
    let before = &text[..byte_at_utf16(text, caret)];
    let query_start = before.trim_end_matches(is_login_char).len();
    let head = before[..query_start].strip_suffix('@')?;
    if let Some(c) = head.chars().next_back()
        && (c.is_ascii_alphanumeric() || c == '_' || c == '`')
    {
        return None;
    }
    let query = &before[query_start..];
    Some(MentionQuery { start: utf16_len(before) - query.len() - 1, query: query.to_owned() })
}

/// Users matching `query` (login or name; login prefixes first, then name word prefixes, then
/// anywhere), at most `limit`.
pub fn filter_mentions<'a>(users: &'a [GhUser], query: &str, limit: usize) -> Vec<&'a GhUser> {
    let q = query.to_lowercase();
    let mut scored: Vec<(&GhUser, u8)> = users
        .iter()
        .filter_map(|u| {
            let login = u.login.to_lowercase();
            let name = u.name.as_deref().unwrap_or_default().to_lowercase();
            let score = if q.is_empty() {
                2
            } else if login.starts_with(&q) {
                0
            } else if name.split_whitespace().any(|w| w.starts_with(&q)) {
                1
            } else if login.contains(&q) || name.contains(&q) {
                2
            } else {
                return None;
            };
            Some((u, score))
        })
        .collect();
    scored.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| collate::compare(&a.0.login, &b.0.login)));
    scored.into_iter().take(limit).map(|(u, _)| u).collect()
}

/// The text after a mention is chosen, and the caret (UTF-16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MentionInsert {
    pub text: String,
    pub caret: usize,
}

/// `text` with the mention at `start..caret` (UTF-16) replaced by `@login `; the new caret.
pub fn insert_mention(text: &str, start: usize, caret: usize, login: &str) -> MentionInsert {
    let insert = format!("@{login} ");
    let (a, b) = (byte_at_utf16(text, start), byte_at_utf16(text, caret).max(byte_at_utf16(text, start)));
    MentionInsert { text: format!("{}{insert}{}", &text[..a], &text[b..]), caret: start + utf16_len(&insert) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::{user, user_named};
    use crate::github::types::*;

    fn label(name: &str) -> GhLabel {
        GhLabel { id: format!("l-{name}"), url: String::new(), name: name.into(), color: "ff0000".into() }
    }

    fn at(minute: u32) -> String {
        format!("2024-01-01T12:{minute:02}:00.000Z")
    }

    fn info(actor: &GhActor, minute: u32) -> EventInfo {
        EventInfo { actor: Some(actor.clone()), created_at: at(minute) }
    }

    fn labeled(actor: &GhActor, minute: u32, name: &str) -> TimelineItem {
        TimelineItem::LabeledEvent(Event { info: info(actor, minute), data: LabelChange { label: label(name) } })
    }

    fn assigned(actor: &GhActor, minute: u32, who: &GhUser) -> TimelineItem {
        TimelineItem::AssignedEvent(Event { info: info(actor, minute), data: UserChange { user: Some(who.clone()) } })
    }

    fn plain(actor: &GhActor, minute: u32) -> Event<NoFields> {
        Event { info: info(actor, minute), data: NoFields {} }
    }

    fn commit(id: &str) -> TimelineItem {
        TimelineItem::PullRequestCommit(TimelineCommit {
            id: id.into(),
            url: String::new(),
            commit: GhCommitShort { id: id.into(), oid: id.into(), message_headline: id.into(), ..Default::default() },
        })
    }

    fn kinds(entries: &[TimelineEntry]) -> Vec<&'static str> {
        entries
            .iter()
            .map(|e| match e {
                TimelineEntry::Commits { .. } => "commits",
                TimelineEntry::Comment { .. } => "comment",
                TimelineEntry::Review { .. } => "review",
                TimelineEntry::Event { .. } => "event",
                TimelineEntry::Simple(_) => "simple",
                TimelineEntry::State(_) => "state",
                TimelineEntry::Unknown { .. } => "unknown",
            })
            .collect()
    }

    fn simple(e: &TimelineEntry) -> &SimpleGroup {
        match e {
            TimelineEntry::Simple(g) => g,
            other => panic!("not a simple group: {other:?}"),
        }
    }

    #[test]
    fn groups_consecutive_commits() {
        let alice = user("alice");
        let out = merge_timeline(
            &[commit("a"), commit("b"), TimelineItem::ClosedEvent(plain(&alice, 0)), commit("c")],
            false,
        );
        assert_eq!(kinds(&out), ["commits", "state", "commits"]);
        let TimelineEntry::Commits { commits, .. } = &out[0] else { panic!("commits first") };
        assert_eq!(commits.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn merges_simple_events_of_one_actor_within_2_minutes() {
        let (alice, bob) = (user("alice"), user("bob"));
        let out = merge_timeline(
            &[labeled(&alice, 0, "bug"), assigned(&alice, 1, &bob), labeled(&alice, 4, "ui"), labeled(&bob, 4, "x")],
            false,
        );
        assert_eq!(out.len(), 3);
        let first = simple(&out[0]);
        assert_eq!(first.count, 2);
        assert_eq!(first.added_labels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["bug"]);
        assert_eq!(first.assigned.iter().map(|u| u.login.as_str()).collect::<Vec<_>>(), ["bob"]);
        assert_eq!(simple(&out[1]).added_labels.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["ui"]);
    }

    #[test]
    fn drops_merged_events_that_cancel_out() {
        let (alice, bob) = (user("alice"), user("bob"));
        let unlabeled =
            TimelineItem::UnlabeledEvent(Event { info: info(&alice, 1), data: LabelChange { label: label("bug") } });
        let out = merge_timeline(
            &[
                labeled(&alice, 0, "bug"),
                unlabeled,
                TimelineItem::ClosedEvent(plain(&bob, 5)),
                TimelineItem::ReopenedEvent(plain(&bob, 5)),
            ],
            false,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn keeps_a_single_event_even_without_changes_and_tracks_renames() {
        let alice = user("alice");
        let rename = |minute, from: &str, to: &str| {
            TimelineItem::RenamedTitleEvent(Event {
                info: info(&alice, minute),
                data: Renamed { previous_title: from.into(), current_title: to.into() },
            })
        };
        let out = merge_timeline(&[rename(0, "a", "b"), rename(1, "b", "c")], false);
        assert_eq!(simple(&out[0]).rename, Some(("a".to_owned(), "c".to_owned())));
    }

    #[test]
    fn keeps_merged_as_the_state_of_merged_state_events() {
        let alice = user("alice");
        let merged = TimelineItem::MergedEvent(Event {
            info: info(&alice, 0),
            data: Merged { merged_event_commit: None, merge_ref_name: "main".into() },
        });
        let out = merge_timeline(&[merged.clone(), TimelineItem::ClosedEvent(plain(&alice, 1))], false);
        let TimelineEntry::State(g) = &out[0] else { panic!("a state group") };
        assert_eq!(g.new_state, PrState::Merged);
        assert_eq!(g.last, merged);
    }

    #[test]
    fn hides_unknown_items_unless_asked() {
        let items = [TimelineItem::Unknown { type_name: "PinnedEvent".into() }];
        assert!(merge_timeline(&items, false).is_empty());
        let out = merge_timeline(&items, true);
        assert!(matches!(&out[..], [TimelineEntry::Unknown { type_name, .. }] if type_name == "PinnedEvent"));
    }

    #[test]
    fn does_not_merge_across_comments() {
        let (alice, bob) = (user("alice"), user("bob"));
        let comment = TimelineItem::IssueComment(GhIssueComment { id: "c1".into(), ..Default::default() });
        let out = merge_timeline(&[assigned(&alice, 0, &bob), comment, assigned(&alice, 0, &alice)], false);
        assert_eq!(kinds(&out), ["simple", "comment", "simple"]);
    }

    #[test]
    fn maps_states_to_intellijs_texts() {
        let i18n = I18n::default();
        assert_eq!(
            review_status(ReviewState::Approved, &i18n),
            ReviewStatus { text: "approved the changes".into(), kind: StatusType::Success }
        );
        assert_eq!(review_status(ReviewState::ChangesRequested, &i18n).kind, StatusType::Error);
        assert_eq!(review_status(ReviewState::Pending, &i18n).text, "started a review");
        assert_eq!(review_status(ReviewState::Dismissed, &i18n).text, "reviewed the changes");
    }

    const HUNK: &str = "@@ -10,4 +10,5 @@ class A\n a\n-b\n+B\n+B2\n c\n d";

    #[test]
    fn parses_and_numbers_a_hunk() {
        let h = parse_patch_hunk(HUNK).expect("a hunk");
        assert_eq!(h.start_line_before, 9);
        use HunkLineType::*;
        assert_eq!(h.lines.iter().map(|l| l.kind).collect::<Vec<_>>(), [Context, Remove, Add, Add, Context, Context]);
        assert_eq!(
            hunk_line_numbers(&h),
            [
                (Some(10), Some(10)),
                (Some(11), None),
                (None, Some(11)),
                (None, Some(12)),
                (Some(12), Some(13)),
                (Some(13), Some(14)),
            ]
        );
    }

    #[test]
    fn truncates_before_an_index() {
        let h = truncate_hunk_before(&parse_patch_hunk(HUNK).expect("a hunk"), 2);
        assert_eq!((h.start_line_before, h.start_line_after, h.lines.len()), (11, 10, 4));
    }

    fn anchor(original_line: u32, start: Option<(DiffSide, u32)>) -> ThreadAnchor<'static> {
        ThreadAnchor {
            diff_hunk: HUNK,
            diff_side: DiffSide::Right,
            original_line: Some(original_line),
            start_diff_side: start.map(|s| s.0),
            original_start_line: start.map(|s| s.1),
        }
    }

    #[test]
    fn keeps_the_anchor_and_3_lines_of_context_before_it() {
        let d = thread_diff_with_anchor(anchor(14, None)).expect("a diff");
        assert_eq!(d.hunk.lines.len(), 4);
        assert_eq!(d.anchor, (3, 4));
    }

    #[test]
    fn anchors_a_multi_line_range() {
        let d = thread_diff_with_anchor(anchor(14, Some((DiffSide::Right, 13)))).expect("a diff");
        assert_eq!(d.anchor, (3, 5));
        assert_eq!(d.hunk.lines.len(), 5);
    }

    #[test]
    fn has_no_diff_for_an_empty_hunk() {
        assert_eq!(thread_diff_with_anchor(ThreadAnchor { diff_hunk: "", ..anchor(1, None) }), None);
    }

    fn thread() -> SuggestionThread<'static> {
        SuggestionThread {
            is_resolved: false,
            path: "src/a.ts",
            line: Some(12),
            start_line: Some(11),
            original_line: Some(12),
            original_start_line: Some(11),
            diff_hunk: HUNK,
        }
    }

    fn x() -> Vec<String> {
        vec!["X".to_owned()]
    }

    #[test]
    fn splits_a_body_into_markdown_and_suggestion_blocks() {
        assert_eq!(
            split_suggestions("Use this:\n```suggestion\nB\nC\n```\nthanks"),
            [
                BodyBlock::Markdown("Use this:".into()),
                BodyBlock::Suggestion(vec!["B".into(), "C".into()]),
                BodyBlock::Markdown("thanks".into()),
            ]
        );
        assert_eq!(split_suggestions("```suggestion\nx"), [BodyBlock::Markdown("```suggestion\nx".into())]);
    }

    #[test]
    fn finds_the_commented_code_and_the_applicability() {
        let t = suggestion_target(thread());
        assert_eq!(t.applicability, SuggestionApplicability::Applicable);
        assert_eq!((t.start_line_index, t.end_line_index), (Some(10), Some(11)));
        assert_eq!(t.original_lines, ["B", "B2"]);
        assert_eq!(
            suggestion_target(SuggestionThread { line: None, ..thread() }).applicability,
            SuggestionApplicability::Outdated
        );
        assert_eq!(
            suggestion_target(SuggestionThread { is_resolved: true, ..thread() }).applicability,
            SuggestionApplicability::Resolved
        );
        let i18n = I18n::default();
        let reason = suggestion_disabled_reason(SuggestionApplicability::Applicable, false, &i18n);
        assert!(reason.is_some_and(|r| r.contains("different branch")));
        assert_eq!(suggestion_disabled_reason(SuggestionApplicability::Applicable, true, &i18n), None);
    }

    #[test]
    fn builds_a_patch_with_context_against_the_local_file() {
        let text = "1\n2\n3\n4\n5\n6\n7\n8\n9\na\nB\nB2\nc\nd\n";
        let r = build_suggestion_patch(text, &suggestion_target(thread()), &x()).expect("applies");
        assert_eq!(r.start, 10);
        assert_eq!(r.patch, "--- a/src/a.ts\n+++ b/src/a.ts\n@@ -8,7 +8,6 @@\n 8\n 9\n a\n-B\n-B2\n+X\n c\n d\n");
    }

    #[test]
    fn finds_moved_code_and_fails_when_it_is_gone() {
        let r = build_suggestion_patch("0\na\nB\nB2\nc", &suggestion_target(thread()), &x()).expect("applies");
        assert_eq!(r.start, 2);
        assert!(r.patch.contains("\\ No newline at end of file"));
        let gone = build_suggestion_patch("nothing\n", &suggestion_target(thread()), &x());
        assert!(gone.is_err_and(|e| e.contains("does not apply")));
    }

    #[test]
    fn keeps_crlf_line_ends() {
        let r = build_suggestion_patch("a\r\nB\r\nB2\r\n", &suggestion_target(thread()), &x()).expect("applies");
        assert!(r.patch.contains("-B\r\n"));
        assert!(r.patch.contains("+X\r\n"));
    }

    #[test]
    fn groups_by_content_and_marks_the_viewers() {
        let (alice, bob) = (user("alice"), user("bob"));
        let rs = [
            GhReaction { content: ReactionContent::Heart, user: Some(alice) },
            GhReaction { content: ReactionContent::Eyes, user: Some(bob.clone()) },
            GhReaction { content: ReactionContent::Heart, user: Some(bob.clone()) },
        ];
        let g = group_reactions(&rs, Some(bob.id.as_str()));
        let summary: Vec<(ReactionContent, usize, bool)> =
            g.iter().map(|r| (r.content, r.users.len(), r.is_reacted_by_current_user)).collect();
        assert_eq!(summary, [(ReactionContent::Heart, 2, true), (ReactionContent::Eyes, 1, true)]);
        let toggled = toggle_reaction_locally(&rs, ReactionContent::Heart, &bob, false);
        assert_eq!(group_reactions(&toggled, Some(bob.id.as_str()))[0].users.len(), 1);
    }

    #[test]
    fn finds_the_query_before_the_caret() {
        assert_eq!(mention_query("hi @al", 6), Some(MentionQuery { start: 3, query: "al".into() }));
        assert_eq!(mention_query("@", 1), Some(MentionQuery { start: 0, query: String::new() }));
        assert_eq!(mention_query("mail@x", 6), None);
        assert_eq!(mention_query("hi @al ", 7), None);
    }

    #[test]
    fn filters_and_inserts() {
        let users = [user_named("bob", Some("Alice Bobson")), user("alice"), user("malik")];
        let found: Vec<&str> = filter_mentions(&users, "al", 10).iter().map(|u| u.login.as_str()).collect();
        assert_eq!(found, ["alice", "bob", "malik"]);
        assert_eq!(insert_mention("hi @al!", 3, 6, "alice"), MentionInsert { text: "hi @alice !".into(), caret: 10 });
    }
}
