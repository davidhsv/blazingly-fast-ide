//! Review in the diff (IntelliJ's `GHPRDiffReviewViewModel`, `GHPRDiffViewModel`,
//! `GHPRThreadsViewModels`, `GHPRReviewNewCommentEditorViewModel`, `PatchHunkUtil`):
//!
//! - a file's patch (GitHub's `patch` of the changed file) parsed into hunks, the commentable ranges
//!   of each side (the hunks' lines), line mapping across a patch, and the "position" of a line
//!   (what commit comments are addressed by);
//! - where a review thread sits in the cumulative diff or in a single commit's diff, and whether the
//!   "Show Review Threads" option shows it;
//! - the new comment's GitHub input (`line`, `side`, `startLine`, `startSide`) and its submit actions;
//! - Previous / Next Comment: the threads in unified file / line order;
//! - the review-in-diff settings.
//!
//! Lines are 0-based unless the name says otherwise (GitHub's are 1-based).

use super::types::{DiffSide, DraftReviewThread, GhReviewThread, SubjectType};
use crate::platform::collate;
use crate::platform::i18n::I18n;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cmp::Ordering;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn to_github(self) -> DiffSide {
        match self {
            Self::Left => DiffSide::Left,
            Self::Right => DiffSide::Right,
        }
    }

    pub fn from_github(side: DiffSide) -> Self {
        match side {
            DiffSide::Left => Self::Left,
            DiffSide::Right => Self::Right,
        }
    }
}

// ---------------------------------------------------------------------------
// Patches

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
    /// 0-based first line of each side.
    pub left_start: usize,
    pub left_count: usize,
    pub right_start: usize,
    pub right_count: usize,
    /// The rows after the header, with their first character (` `, `+`, `-`, `\`).
    pub rows: Vec<String>,
}

static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^@@ -([0-9]+)(?:,([0-9]+))? \+([0-9]+)(?:,([0-9]+))? @@").expect("valid regex"));

/// The numbers of a hunk header: left start, left count, right start, right count (counts default to 1).
fn header(row: &str) -> Option<(usize, usize, usize, usize)> {
    let h = HEADER.captures(row)?;
    let number = |i: usize, default: usize| h.get(i).map_or(Some(default), |m| m.as_str().parse().ok());
    Some((number(1, 0)?, number(2, 1)?, number(3, 0)?, number(4, 1)?))
}

fn patch_rows(patch: &str) -> Vec<String> {
    patch.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_owned).collect()
}

/// The hunks of a file's patch (GitHub's `patch`: hunks without the file header).
pub fn parse_patch(patch: Option<&str>) -> Vec<PatchHunk> {
    let mut rows = patch_rows(patch.unwrap_or_default());
    while rows.last().is_some_and(String::is_empty) {
        rows.pop();
    }
    let mut hunks: Vec<PatchHunk> = Vec::new();
    for row in rows {
        if let Some((left, left_count, right, right_count)) = header(&row) {
            // An empty side's start is the line before it (`-0,0`, `+10,0`).
            hunks.push(PatchHunk {
                left_start: if left_count > 0 { left.saturating_sub(1) } else { left },
                left_count,
                right_start: if right_count > 0 { right.saturating_sub(1) } else { right },
                right_count,
                rows: Vec::new(),
            });
        } else if let Some(h) = hunks.last_mut() {
            h.rows.push(if row.is_empty() { " ".to_owned() } else { row });
        }
    }
    hunks
}

/// A half-open line range `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// The commentable lines of each side.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommentableRanges {
    pub left: Vec<Span>,
    pub right: Vec<Span>,
}

/// The commentable lines of each side (`patch.ranges`): the lines of every hunk, context included.
pub fn commentable_ranges(hunks: &[PatchHunk]) -> CommentableRanges {
    let mut out = CommentableRanges::default();
    for h in hunks {
        if h.left_count > 0 {
            out.left.push(Span { start: h.left_start, end: h.left_start + h.left_count });
        }
        if h.right_count > 0 {
            out.right.push(Span { start: h.right_start, end: h.right_start + h.right_count });
        }
    }
    out
}

/// The line of the other side matching `line` of `from` (`forcefullyMapLine`): outside the hunks
/// lines shift by what the hunks before added or removed; inside, context lines map exactly and
/// changed lines to the other side's line at that point.
pub fn map_across_patch(hunks: &[PatchHunk], from: Side, line: usize) -> usize {
    let (mine, theirs) = if from == Side::Left { ('-', '+') } else { ('+', '-') };
    let mut shift: i64 = 0;
    for h in hunks {
        let (start, count, other_start, other_count) = match from {
            Side::Left => (h.left_start, h.left_count, h.right_start, h.right_count),
            Side::Right => (h.right_start, h.right_count, h.left_start, h.left_count),
        };
        if line < start {
            break;
        }
        if line < start + count {
            let (mut here, mut there) = (start, other_start);
            for row in &h.rows {
                let c = row.chars().next().unwrap_or(' ');
                if c == '\\' {
                    continue;
                }
                if c == theirs {
                    there += 1;
                } else {
                    if here == line {
                        return there;
                    }
                    here += 1;
                    if c != mine {
                        there += 1;
                    }
                }
            }
            return there;
        }
        shift = (other_start + other_count) as i64 - (start + count) as i64;
    }
    usize::try_from(line as i64 + shift).unwrap_or(0)
}

/// GitHub's diff "position" of `line` of `side` (`PatchHunkUtil.findDiffFileLineIndex`): the row's
/// index counted from the first hunk header (the row under it is 1, later headers count too);
/// `None` when the line is not in the patch.
pub fn diff_position(patch: Option<&str>, side: Side, line: usize) -> Option<usize> {
    let mut position = 0;
    let mut started = false;
    let (mut left, mut right) = (0usize, 0usize);
    for row in patch_rows(patch.unwrap_or_default()) {
        if let Some((l, _, r, _)) = header(&row) {
            if started {
                position += 1;
            }
            started = true;
            left = l.saturating_sub(1);
            right = r.saturating_sub(1);
            continue;
        }
        if !started {
            continue;
        }
        position += 1;
        let c = row.chars().next().unwrap_or(' ');
        if c == '\\' {
            continue;
        }
        if c != '+' {
            if side == Side::Left && left == line {
                return Some(position);
            }
            left += 1;
        }
        if c != '-' {
            if side == Side::Right && right == line {
                return Some(position);
            }
            right += 1;
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Threads in a diff

/// A comment's place: the last line (where its inlay goes) and the first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommentLocation {
    pub side: Side,
    pub line: usize,
    pub start_side: Side,
    pub start_line: usize,
}

/// Where `thread` sits in a diff of the pull request (`mapThreadToChange`): `commit` `None` is the
/// cumulative diff, which places current threads at their `line` / `startLine`; a single commit's
/// diff places right-side threads made on that commit at their original lines (and those whose
/// current commit it is at their lines). Outdated and file threads have no place.
pub fn thread_location(thread: &GhReviewThread, commit: Option<&str>) -> Option<CommentLocation> {
    if thread.subject_type == SubjectType::File {
        return None;
    }
    let root = thread.comments.nodes.first();
    let side = Side::from_github(thread.diff_side);
    let at = |line: Option<u32>, start: Option<u32>| {
        let line = line? as usize;
        Some(CommentLocation {
            side,
            line: line.saturating_sub(1),
            start_side: if start.is_none() {
                side
            } else {
                Side::from_github(thread.start_diff_side.unwrap_or(thread.diff_side))
            },
            start_line: start.map_or(line, |s| s as usize).saturating_sub(1),
        })
    };
    let Some(commit) = commit else { return at(thread.line, thread.start_line) };
    if side != Side::Right {
        return None;
    }
    let oid = |c: Option<&super::types::GhCommitHash>| c.is_some_and(|c| c.oid == commit);
    if root.is_some_and(|r| oid(r.original_commit.as_ref())) {
        at(thread.original_line, thread.original_start_line)
    } else if root.is_some_and(|r| oid(r.commit.as_ref())) {
        at(thread.line, thread.start_line)
    } else {
        None
    }
}

/// "Show Review Threads" (`DiscussionsViewOption`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ThreadsView {
    #[serde(rename = "all")]
    All,
    #[serde(rename = "unresolved")]
    Unresolved,
    #[serde(rename = "none")]
    Hidden,
}

impl ThreadsView {
    pub const ALL: [ThreadsView; 3] = [Self::All, Self::Unresolved, Self::Hidden];

    pub fn label(self, i18n: &I18n) -> String {
        i18n.tx(match self {
            Self::All => "All",
            Self::Unresolved => "Unresolved Only",
            Self::Hidden => "Do Not Show",
        })
    }
}

pub fn is_thread_visible(is_resolved: bool, option: ThreadsView) -> bool {
    option == ThreadsView::All || (option == ThreadsView::Unresolved && !is_resolved)
}

// ---------------------------------------------------------------------------
// New comments

/// `createThreadDTO`: the GitHub input of a new comment on `path` at `loc`.
pub fn draft_thread(path: &str, loc: CommentLocation, body: &str) -> DraftReviewThread {
    let single = loc.start_side == loc.side && loc.start_line == loc.line;
    DraftReviewThread {
        body: body.to_owned(),
        path: path.to_owned(),
        line: loc.line as u32 + 1,
        side: loc.side.to_github(),
        start_line: (!single).then_some(loc.start_line as u32 + 1),
        start_side: (!single).then_some(loc.start_side.to_github()),
    }
}

/// The location of a new comment on the 0-based lines `start`..=`end` of one side.
pub fn new_comment_location(side: Side, start: usize, end: usize) -> CommentLocation {
    CommentLocation { side, line: start.max(end), start_side: side, start_line: start.min(end) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubmitAction {
    StartReview,
    SingleComment,
    ReviewComment,
}

impl SubmitAction {
    pub fn text(self, i18n: &I18n) -> String {
        i18n.tx(match self {
            Self::StartReview => "Start Review",
            Self::SingleComment => "Add Single Comment",
            Self::ReviewComment => "Add Review Comment",
        })
    }
}

/// The new comment's buttons, the primary one first: with a pending review "Add Review Comment";
/// else "Start Review" and "Add Single Comment", the one used last first (`reviewCommentsPreferred`).
pub fn submit_actions(has_pending_review: bool, review_preferred: bool) -> Vec<SubmitAction> {
    use SubmitAction::*;
    if has_pending_review {
        vec![ReviewComment]
    } else if review_preferred {
        vec![StartReview, SingleComment]
    } else {
        vec![SingleComment, StartReview]
    }
}

// ---------------------------------------------------------------------------
// Settings

/// Review-in-diff settings: "Show Review Threads" (`editorReviewViewOption`), the new comment's
/// preferred button (`reviewCommentsPreferred`) and "Automatically mark opened files as viewed"
/// (`GithubSettings.isAutomaticallyMarkAsViewed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiffSettings {
    pub threads_view: ThreadsView,
    /// "Start Review" is the primary button of a new comment (else "Add Single Comment").
    pub review_comments_preferred: bool,
    pub auto_mark_viewed: bool,
}

/// Storage key of [`ReviewDiffSettings`].
pub const REVIEW_DIFF_SETTINGS_KEY: &str = "openidea.github.reviewDiff.v1";

impl Default for ReviewDiffSettings {
    /// IntelliJ's defaults.
    fn default() -> Self {
        Self { threads_view: ThreadsView::Unresolved, review_comments_preferred: true, auto_mark_viewed: false }
    }
}

/// The stored settings, each invalid or missing value replaced by its default.
pub fn parse_review_diff_settings(raw: Option<&Value>) -> ReviewDiffSettings {
    let d = ReviewDiffSettings::default();
    let get = |key: &str| raw.and_then(|r| r.get(key));
    ReviewDiffSettings {
        threads_view: get("threadsView")
            .filter(|v| v.is_string())
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(d.threads_view),
        review_comments_preferred: get("reviewCommentsPreferred")
            .and_then(Value::as_bool)
            .unwrap_or(d.review_comments_preferred),
        auto_mark_viewed: get("autoMarkViewed").and_then(Value::as_bool).unwrap_or(d.auto_mark_viewed),
    }
}

/// The settings as one window holds them, kept in step with storage: another window's save (or a
/// cleared storage) reaches it through [`ReviewDiffSettingsStore::storage_changed`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReviewDiffSettingsStore {
    pub settings: ReviewDiffSettings,
}

impl ReviewDiffSettingsStore {
    /// The settings stored as `raw` (JSON text; `None` when nothing is stored).
    pub fn load(raw: Option<&str>) -> Self {
        let value = raw.and_then(|r| serde_json::from_str::<Value>(r).ok());
        Self { settings: parse_review_diff_settings(value.as_ref()) }
    }

    /// A storage change: `key` `None` means storage was cleared. Changes of other keys are ignored;
    /// unreadable text gives the defaults. Returns whether the settings were read again.
    pub fn storage_changed(&mut self, key: Option<&str>, raw: Option<&str>) -> bool {
        if key.is_some_and(|k| k != REVIEW_DIFF_SETTINGS_KEY) {
            return false;
        }
        *self = Self::load(raw);
        true
    }

    /// Changes the settings; returns the JSON text to store under [`REVIEW_DIFF_SETTINGS_KEY`].
    pub fn set(&mut self, change: impl FnOnce(&mut ReviewDiffSettings)) -> String {
        change(&mut self.settings);
        serde_json::to_string(&self.settings).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Previous / Next Comment

/// A thread's place in the unified order (`UnifiedCodeReviewItemPosition`): file, then right, then left line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadPosition {
    pub id: String,
    /// Index of the file in the pull request's changes.
    pub file: usize,
    pub path: String,
    pub left: usize,
    pub right: usize,
    pub created_at: String,
    /// Where navigation reveals it.
    pub side: Side,
    pub line: usize,
}

/// A place in the unified order: a file and a line on each side (the caret's, or a thread's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnifiedPosition {
    pub file: usize,
    pub left: usize,
    pub right: usize,
}

impl ThreadPosition {
    pub fn position(&self) -> UnifiedPosition {
        UnifiedPosition { file: self.file, left: self.left, right: self.right }
    }
}

pub fn compare_positions(a: UnifiedPosition, b: UnifiedPosition) -> Ordering {
    a.file.cmp(&b.file).then(a.right.cmp(&b.right)).then(a.left.cmp(&b.left))
}

/// A changed file of the cumulative diff, with its patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub hunks: Vec<PatchHunk>,
}

/// The threads of the cumulative diff in navigation order (`createSortedThreadPositionsAndIds`): each
/// thread's line is mapped to both sides with its file's patch.
pub fn thread_order(files: &[DiffFile], threads: &[GhReviewThread]) -> Vec<ThreadPosition> {
    let mut out: Vec<ThreadPosition> = Vec::new();
    for t in threads {
        let Some(file) = files.iter().position(|f| f.path == t.path) else { continue };
        let Some(loc) = thread_location(t, None) else { continue };
        let other = map_across_patch(&files[file].hunks, loc.side, loc.line);
        out.push(ThreadPosition {
            id: t.id.clone(),
            file,
            path: t.path.clone(),
            left: if loc.side == Side::Left { loc.line } else { other },
            right: if loc.side == Side::Right { loc.line } else { other },
            created_at: t.comments.nodes.first().map(|c| c.created_at.clone()).unwrap_or_default(),
            side: loc.side,
            line: loc.line,
        });
    }
    out.sort_by(|a, b| {
        compare_positions(a.position(), b.position()).then_with(|| collate::compare(&a.created_at, &b.created_at))
    });
    out
}

/// Where Previous / Next Comment starts from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationStart<'a> {
    /// The thread with this id.
    Thread(&'a str),
    /// The caret: the thread on its line counts as next and as previous.
    Caret(UnifiedPosition),
}

/// The visible thread after (`forward`) or before the start.
pub fn adjacent_thread<'a>(
    order: &'a [ThreadPosition],
    from: NavigationStart<'_>,
    forward: bool,
    visible: impl Fn(&str) -> bool,
) -> Option<&'a ThreadPosition> {
    let mut candidates: Box<dyn Iterator<Item = &'a ThreadPosition> + 'a> = match from {
        NavigationStart::Thread(id) => {
            let at = order.iter().position(|p| p.id == id)?;
            if forward { Box::new(order[at + 1..].iter()) } else { Box::new(order[..at].iter().rev()) }
        }
        NavigationStart::Caret(caret) if forward => {
            Box::new(order.iter().skip_while(move |p| compare_positions(p.position(), caret) == Ordering::Less))
        }
        NavigationStart::Caret(caret) => Box::new(
            order.iter().rev().skip_while(move |p| compare_positions(p.position(), caret) == Ordering::Greater),
        ),
    };
    candidates.find(|p| visible(&p.id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::types::*;

    // a.ts: line 2 changed, line 4 added; 20 lines later one removed.
    const PATCH: &str = "@@ -1,4 +1,5 @@\n one\n-two\n+TWO\n three\n+three and a half\n four\n\
                         @@ -20,3 +21,2 @@ function x() {\n twenty\n-twenty-one\n twenty-two";

    fn comment() -> GhReviewComment {
        GhReviewComment {
            id: "C1".into(),
            created_at: "2026-10-01T10:00:00Z".into(),
            commit: Some(GhCommitHash { oid: "head".into(), abbreviated_oid: "head".into(), ..Default::default() }),
            original_commit: Some(GhCommitHash {
                oid: "c1".into(),
                abbreviated_oid: "c1".into(),
                ..Default::default()
            }),
            ..GhReviewComment::default()
        }
    }

    fn thread() -> GhReviewThread {
        GhReviewThread {
            id: "T1".into(),
            path: "a.ts".into(),
            diff_side: DiffSide::Right,
            line: Some(3),
            original_line: Some(2),
            comments: Nodes::of(vec![comment()]),
            viewer_can_reply: true,
            viewer_can_resolve: true,
            ..GhReviewThread::default()
        }
    }

    fn loc(side: Side, line: usize, start_side: Side, start_line: usize) -> Option<CommentLocation> {
        Some(CommentLocation { side, line, start_side, start_line })
    }

    #[test]
    fn parses_hunks_with_0_based_starts() {
        let hunks = parse_patch(Some(&format!("{PATCH}\n")));
        let heads: Vec<(usize, usize, usize, usize)> =
            hunks.iter().map(|h| (h.left_start, h.left_count, h.right_start, h.right_count)).collect();
        assert_eq!(heads, [(0, 4, 0, 5), (19, 3, 20, 2)]);
        assert_eq!(hunks[1].rows, [" twenty", "-twenty-one", " twenty-two"]);
        assert!(parse_patch(None).is_empty());
    }

    #[test]
    fn takes_every_hunk_line_of_a_side_as_commentable_patch_ranges() {
        assert_eq!(
            commentable_ranges(&parse_patch(Some(PATCH))),
            CommentableRanges {
                left: vec![Span { start: 0, end: 4 }, Span { start: 19, end: 22 }],
                right: vec![Span { start: 0, end: 5 }, Span { start: 20, end: 22 }],
            }
        );
        // A new file has no left side.
        assert!(commentable_ranges(&parse_patch(Some("@@ -0,0 +1,2 @@\n+a\n+b"))).left.is_empty());
    }

    #[test]
    fn maps_lines_across_the_patch() {
        let hunks = parse_patch(Some(PATCH));
        assert_eq!(map_across_patch(&hunks, Side::Left, 0), 0);
        assert_eq!(map_across_patch(&hunks, Side::Left, 2), 2, "three");
        assert_eq!(map_across_patch(&hunks, Side::Left, 3), 4, "four");
        assert_eq!(map_across_patch(&hunks, Side::Right, 3), 3, "the added line: before four");
        assert_eq!(map_across_patch(&hunks, Side::Left, 10), 11, "between the hunks: +1");
        assert_eq!(map_across_patch(&hunks, Side::Left, 21), 21, "twenty-two");
        assert_eq!(map_across_patch(&hunks, Side::Left, 30), 30, "after: +1 -1");
        assert_eq!(map_across_patch(&hunks, Side::Right, 30), 30);
    }

    #[test]
    fn finds_githubs_diff_position_of_a_line() {
        let p = Some(PATCH);
        assert_eq!(diff_position(p, Side::Right, 0), Some(1));
        assert_eq!(diff_position(p, Side::Left, 1), Some(2));
        assert_eq!(diff_position(p, Side::Right, 1), Some(3));
        assert_eq!(diff_position(p, Side::Right, 3), Some(5));
        // The second header counts as a row.
        assert_eq!(diff_position(p, Side::Left, 20), Some(9));
        assert_eq!(diff_position(p, Side::Right, 21), Some(10));
        assert_eq!(diff_position(p, Side::Right, 10), None);
    }

    #[test]
    fn places_current_threads_of_the_changes_at_their_lines() {
        assert_eq!(thread_location(&thread(), None), loc(Side::Right, 2, Side::Right, 2));
        let left = GhReviewThread {
            diff_side: DiffSide::Left,
            line: Some(5),
            start_line: Some(3),
            start_diff_side: Some(DiffSide::Left),
            ..thread()
        };
        assert_eq!(thread_location(&left, None), loc(Side::Left, 4, Side::Left, 2));
        // Outdated and file threads have no line.
        assert_eq!(thread_location(&GhReviewThread { line: None, is_outdated: true, ..thread() }, None), None);
        assert_eq!(thread_location(&GhReviewThread { subject_type: SubjectType::File, ..thread() }, None), None);
    }

    #[test]
    fn places_right_side_threads_of_a_commits_diff_at_their_original_lines() {
        assert_eq!(thread_location(&thread(), Some("c1")), loc(Side::Right, 1, Side::Right, 1));
        assert_eq!(thread_location(&thread(), Some("head")).map(|l| l.line), Some(2));
        assert_eq!(thread_location(&thread(), Some("other")), None);
        assert_eq!(thread_location(&GhReviewThread { diff_side: DiffSide::Left, ..thread() }, Some("c1")), None);
    }

    #[test]
    fn filters_by_show_review_threads() {
        assert!(is_thread_visible(true, ThreadsView::All));
        assert!(!is_thread_visible(true, ThreadsView::Unresolved));
        assert!(is_thread_visible(false, ThreadsView::Unresolved));
        assert!(!is_thread_visible(false, ThreadsView::Hidden));
    }

    #[test]
    fn builds_the_github_input_of_single_and_multi_line_comments() {
        assert_eq!(
            draft_thread("a.ts", new_comment_location(Side::Right, 4, 4), "hi"),
            DraftReviewThread {
                body: "hi".into(),
                path: "a.ts".into(),
                line: 5,
                side: DiffSide::Right,
                start_line: None,
                start_side: None,
            }
        );
        let multi = draft_thread("a.ts", new_comment_location(Side::Left, 6, 2), "x");
        assert_eq!(
            (multi.line, multi.side, multi.start_line, multi.start_side),
            (7, DiffSide::Left, Some(3), Some(DiffSide::Left))
        );
    }

    #[test]
    fn offers_add_review_comment_with_a_pending_review_else_the_preferred_of_start_review_add_single_comment() {
        use SubmitAction::*;
        assert_eq!(submit_actions(true, false), [ReviewComment]);
        assert_eq!(submit_actions(false, true), [StartReview, SingleComment]);
        assert_eq!(submit_actions(false, false), [SingleComment, StartReview]);
    }

    #[test]
    fn parses_the_stored_settings_with_intellijs_defaults() {
        assert_eq!(
            parse_review_diff_settings(None),
            ReviewDiffSettings {
                threads_view: ThreadsView::Unresolved,
                review_comments_preferred: true,
                auto_mark_viewed: false
            }
        );
        let stored = serde_json::json!({ "threadsView": "none", "autoMarkViewed": true, "reviewCommentsPreferred": 1 });
        assert_eq!(
            parse_review_diff_settings(Some(&stored)),
            ReviewDiffSettings {
                threads_view: ThreadsView::Hidden,
                review_comments_preferred: true,
                auto_mark_viewed: true
            }
        );
    }

    fn order() -> Vec<ThreadPosition> {
        let files = [
            DiffFile { path: "a.ts".into(), hunks: parse_patch(Some(PATCH)) },
            DiffFile { path: "b.ts".into(), hunks: Vec::new() },
        ];
        let t = |id: &str, path: &str, side: DiffSide, line: Option<u32>| GhReviewThread {
            id: id.into(),
            path: path.into(),
            diff_side: side,
            line,
            ..thread()
        };
        let threads = [
            t("b1", "b.ts", DiffSide::Right, Some(1)),
            t("a-right", "a.ts", DiffSide::Right, Some(5)),
            t("a-left", "a.ts", DiffSide::Left, Some(2)),
            t("a-first", "a.ts", DiffSide::Right, Some(1)),
            t("outdated", "a.ts", DiffSide::Right, None),
            t("elsewhere", "c.ts", DiffSide::Right, Some(3)),
        ];
        thread_order(&files, &threads)
    }

    #[test]
    fn orders_threads_by_file_right_line_left_line() {
        let order = order();
        assert_eq!(order.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["a-first", "a-left", "a-right", "b1"]);
        let p = &order[1];
        assert_eq!((p.side, p.line, p.left, p.right), (Side::Left, 1, 1, 1));
    }

    #[test]
    fn moves_from_a_thread_or_from_the_caret_skipping_hidden_threads() {
        let order = order();
        let id = |p: Option<&ThreadPosition>| p.map(|p| p.id.clone());
        let all = |_: &str| true;
        let caret = |file, left, right| NavigationStart::Caret(UnifiedPosition { file, left, right });
        assert_eq!(
            id(adjacent_thread(&order, NavigationStart::Thread("a-first"), true, all)).as_deref(),
            Some("a-left")
        );
        assert_eq!(id(adjacent_thread(&order, NavigationStart::Thread("a-first"), false, all)), None);
        let hidden = |id: &str| id != "a-right";
        assert_eq!(id(adjacent_thread(&order, NavigationStart::Thread("a-left"), true, hidden)).as_deref(), Some("b1"));
        // The caret's line counts: the thread on it is the next and the previous one.
        assert_eq!(id(adjacent_thread(&order, caret(0, 3, 4), true, all)).as_deref(), Some("a-right"));
        assert_eq!(id(adjacent_thread(&order, caret(0, 3, 4), false, all)).as_deref(), Some("a-right"));
        assert_eq!(id(adjacent_thread(&order, caret(0, 3, 3), false, all)).as_deref(), Some("a-left"));
        assert_eq!(id(adjacent_thread(&order, caret(1, 9, 9), true, all)), None);
    }

    #[test]
    fn updates_an_open_page_when_another_window_saves_or_clears_review_settings() {
        let mut store = ReviewDiffSettingsStore::load(None);
        assert!(!store.settings.auto_mark_viewed);
        let saved = r#"{"autoMarkViewed":true,"threadsView":"all"}"#;
        assert!(store.storage_changed(Some(REVIEW_DIFF_SETTINGS_KEY), Some(saved)));
        assert!(store.settings.auto_mark_viewed);
        assert_eq!(store.settings.threads_view, ThreadsView::All);
        store.storage_changed(None, None);
        assert!(!store.settings.auto_mark_viewed);
        let stored = store.set(|s| s.auto_mark_viewed = !s.auto_mark_viewed);
        let stored: Value = serde_json::from_str(&stored).expect("JSON");
        assert_eq!(stored["autoMarkViewed"], Value::Bool(true));
    }

    #[test]
    fn ignores_unrelated_storage_changes_and_recovers_from_invalid_saved_settings() {
        let mut store = ReviewDiffSettingsStore::default();
        store.set(|s| s.auto_mark_viewed = true);
        assert!(!store.storage_changed(Some("other"), None));
        assert!(store.settings.auto_mark_viewed);
        store.storage_changed(Some(REVIEW_DIFF_SETTINGS_KEY), Some("invalid JSON"));
        assert!(!store.settings.auto_mark_viewed);
    }
}
