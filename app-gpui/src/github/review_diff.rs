//! Review in a pull request's diff (IntelliJ's `GHPRReviewDiffExtension` with its
//! `DiffEditorModel`), for diffs whose context is a pull request's file. For each pane it computes
//!
//! - the inlays: the visible review threads under their lines (on their side; the unified viewer
//!   maps left lines to the right side) and the new comments being written;
//! - the gutter: comment icons, the "+" on commentable lines (the patch's hunks of that side, read
//!   access needed), new comments' range outlines (adjustable on the pull request's changes unless
//!   the text has a suggestion).
//!
//! It also gives the toolbar's Submit Review text, "Show Review Threads", the editor menu (Add
//! Review Comment, Add Multiline Review Comment, Mark File as Viewed), the automatic "viewed" mark,
//! and the thread inlays' operations (the compact view, Reply, Resolve). Requests are handed out;
//! the caller sends them and reloads the pull request's data.

use super::diff::{
    CommentableRanges, Side, ThreadsView, commentable_ranges, is_thread_visible, new_comment_location, parse_patch,
    thread_location,
};
use super::gutter::{GutterState, Inlay, InlayContent, Outline, OutlineLock, PaneReview, outline_tooltip};
use super::new_comments::{Discard, NewComments, document_request};
use super::timeline::contains_suggested_change;
use super::types::{CommitFile, FileViewedState, GhReviewComment, GhReviewThread, PendingReview};
use crate::platform::i18n::{I18n, Param};
use ide_github::GhApiRequest;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;

/// `context.kind` of a diff of a pull request's file.
pub const PULL_REQUEST_DIFF: &str = "github.pullRequest";

/// The pull request file a diff shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewDiffContext {
    pub number: u32,
    /// Repository-relative path.
    pub path: String,
    /// The commit whose diff this is; `None` for the pull request's changes.
    pub commit: Option<String>,
}

/// The pull request file of a diff, from its `context`; `None` for other diffs.
pub fn review_diff_context(context: Option<&Value>) -> Option<ReviewDiffContext> {
    let c = context?;
    if c.get("kind").and_then(Value::as_str) != Some(PULL_REQUEST_DIFF) {
        return None;
    }
    Some(ReviewDiffContext {
        number: c.get("number").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())?,
        path: c.get("path").and_then(Value::as_str)?.to_owned(),
        commit: c.get("commit").and_then(Value::as_str).map(str::to_owned),
    })
}

/// A pane of the diff viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffPane {
    /// The side it shows (the unified viewer's single pane: `Right`).
    pub side: Side,
    pub unified: bool,
}

/// What the review shows of the pull request.
#[derive(Clone, Copy, Debug)]
pub struct DiffData<'a> {
    /// The pull request's threads (every file).
    pub threads: &'a [GhReviewThread],
    pub threads_view: ThreadsView,
    /// The pull request's changed files, once loaded.
    pub changes: Option<&'a [CommitFile]>,
    /// The user has read access.
    pub can_comment: bool,
}

/// The review of one pull request diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewDiff {
    pub ctx: ReviewDiffContext,
    /// Threads hidden with the gutter's comment icon (`Hideable`).
    hidden: HashSet<String>,
    /// A commit's patch of the file once its files are known (`Some(None)`: no patch).
    commit_patch: Option<Option<String>>,
}

/// Loads a review diff asks for when it opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiffLoad {
    /// The pull request's details, threads, pending review, changes and viewed state.
    PullRequestData(u32),
    /// The commit's changed files (their patches give the commentable lines), then
    /// [`ReviewDiff::commit_files_loaded`].
    CommitFiles(String),
}

impl ReviewDiff {
    pub fn new(ctx: ReviewDiffContext) -> (Self, Vec<DiffLoad>) {
        let mut loads = vec![DiffLoad::PullRequestData(ctx.number)];
        loads.extend(ctx.commit.clone().map(DiffLoad::CommitFiles));
        (Self { ctx, hidden: HashSet::new(), commit_patch: None }, loads)
    }

    /// Previous / Next Comment work on the pull request's changes only (`canNavigate`).
    pub fn can_navigate(&self) -> bool {
        self.ctx.commit.is_none()
    }

    pub fn commit_files_loaded(&mut self, files: &[CommitFile]) {
        self.commit_patch = Some(files.iter().find(|f| f.filename == self.ctx.path).and_then(|f| f.patch.clone()));
    }

    fn threads<'a>(&self, data: &DiffData<'a>) -> Vec<&'a GhReviewThread> {
        data.threads.iter().filter(|t| t.path == self.ctx.path).collect()
    }

    /// The commentable lines of each side, once the patch is known.
    fn commentable(&self, data: &DiffData<'_>) -> Option<CommentableRanges> {
        if !data.can_comment {
            return None;
        }
        let patch = if self.ctx.commit.is_some() {
            self.commit_patch.clone()?
        } else {
            data.changes?.iter().find(|f| f.filename == self.ctx.path).and_then(|f| f.patch.clone())
        };
        Some(commentable_ranges(&parse_patch(patch.as_deref())))
    }

    /// The inlays and gutter of `pane`; `locate` gives a line of a side in this pane (`None`: it
    /// shows elsewhere).
    pub fn pane(
        &self,
        pane: DiffPane,
        locate: &dyn Fn(Side, usize) -> Option<usize>,
        data: &DiffData<'_>,
        comments: &NewComments,
        i18n: &I18n,
    ) -> PaneReview {
        let commit = self.ctx.commit.as_deref();
        let mut out = PaneReview::default();
        for t in self.threads(data) {
            let loc = is_thread_visible(t.is_resolved, data.threads_view).then(|| thread_location(t, commit)).flatten();
            let Some(line) = loc.and_then(|l| locate(l.side, l.line)) else { continue };
            out.gutter.lines_with_comments.insert(line);
            if self.hidden.contains(&t.id) {
                continue;
            }
            out.gutter.lines_with_inlays.insert(line);
            out.inlays.push(Inlay { line, content: InlayContent::Thread(t.id.clone()) });
        }
        for c in comments.of(self.ctx.number, &self.ctx.path, commit) {
            let Some(line) = locate(c.location.side, c.location.line) else { continue };
            out.gutter.lines_with_new_comments.insert(line);
            out.gutter.lines_with_inlays.insert(line);
            out.inlays.push(Inlay { line, content: InlayContent::NewComment(c.id.clone()) });
            if let Some(start) = locate(c.location.start_side, c.location.start_line) {
                let lock = if commit.is_some() {
                    Some(OutlineLock::Commit)
                } else {
                    contains_suggested_change(&c.text).then_some(OutlineLock::Suggestion)
                };
                out.gutter.outlines.push(Outline {
                    key: c.id.clone(),
                    start: start.min(line),
                    end: start.max(line),
                    adjustable: lock.is_none(),
                    tooltip: outline_tooltip(lock, i18n),
                });
            }
        }
        if let Some(ranges) = self.commentable(data) {
            // The unified viewer's lines are the right side's.
            out.gutter.commentable = if pane.unified || pane.side == Side::Right { ranges.right } else { ranges.left };
        }
        out
    }

    /// Opens a new comment on lines `start..=end` of `side` (the gutter's "+" or the actions).
    pub fn request(&self, comments: &mut NewComments, side: Side, start: usize, end: usize, now: &str) -> String {
        let location = new_comment_location(side, start, end);
        comments.request(self.ctx.number, &self.ctx.path, self.ctx.commit.as_deref(), location, now)
    }

    /// The gutter's cancel: closes the new comment ending on `line` of `side`.
    pub fn cancel_at(&self, comments: &mut NewComments, side: Side, line: usize) -> Discard {
        let id = comments
            .of(self.ctx.number, &self.ctx.path, self.ctx.commit.as_deref())
            .into_iter()
            .find(|c| c.location.side == side && c.location.line == line)
            .map(|c| c.id.clone());
        id.map_or(Discard::Missing, |id| comments.discard(&id))
    }

    /// The gutter's comment icon on `line` of the pane `locate` belongs to: shows the line's
    /// threads, or hides them when any is shown (`syncOrToggleAll`).
    pub fn toggle_at(&mut self, line: usize, locate: &dyn Fn(Side, usize) -> Option<usize>, data: &DiffData<'_>) {
        let commit = self.ctx.commit.as_deref();
        let ids: Vec<String> = self
            .threads(data)
            .into_iter()
            .filter(|t| thread_location(t, commit).and_then(|l| locate(l.side, l.line)) == Some(line))
            .map(|t| t.id.clone())
            .collect();
        let any_shown = ids.iter().any(|id| !self.hidden.contains(id));
        for id in ids {
            if any_shown {
                self.hidden.insert(id);
            } else {
                self.hidden.remove(&id);
            }
        }
    }

    /// Shows a thread again (Previous / Next Comment moved to it).
    pub fn reveal(&mut self, thread_id: &str) {
        self.hidden.remove(thread_id);
    }

    /// "Mark File as Viewed" / "Mark File as Not Viewed": the current state; `None` on a commit's
    /// diff or before the viewed state loaded.
    pub fn viewed(&self, viewed_state: Option<FileViewedState>) -> Option<bool> {
        if self.ctx.commit.is_some() {
            return None;
        }
        viewed_state.map(|s| s == FileViewedState::Viewed)
    }

    /// The request marking the file viewed (`viewed`) or not; set the cached state at once, and
    /// reload it when the request fails.
    pub fn set_viewed_request(&self, pull_request_id: &str, viewed: bool) -> Result<GhApiRequest, String> {
        let name = if viewed { "markFileAsViewed" } else { "unmarkFileAsViewed" };
        document_request(
            name,
            &[
                ("pullRequestId", Some(Value::from(pull_request_id))),
                ("path", Some(Value::from(self.ctx.path.as_str()))),
            ],
        )
    }

    /// Opening the diff marks the file viewed when "Automatically mark opened files as viewed" is on
    /// (never on a commit's diff); `viewed_state` is the file's loaded state.
    pub fn auto_mark_viewed(
        &self,
        auto_mark_viewed: bool,
        viewed_state: Option<FileViewedState>,
        pull_request_id: &str,
    ) -> Option<GhApiRequest> {
        if !auto_mark_viewed || self.viewed(viewed_state)? {
            return None;
        }
        self.set_viewed_request(pull_request_id, true).ok()
    }

    /// The editor menu of a pane line: Add Review Comment (`caret`'s line), Add Multiline Review
    /// Comment (the selected lines), and Mark File as (Not) Viewed when the state is known.
    pub fn context_menu(
        &self,
        gutter: &GutterState,
        caret: usize,
        selection: (usize, usize),
        viewed_state: Option<FileViewedState>,
    ) -> Vec<DiffMenuItem> {
        let mut items = vec![
            DiffMenuItem::AddComment { enabled: gutter.comment_range(false, selection, caret).is_some() },
            DiffMenuItem::AddMultilineComment { enabled: gutter.comment_range(true, selection, caret).is_some() },
        ];
        items.extend(self.viewed(viewed_state).map(|viewed| DiffMenuItem::ToggleViewed { viewed }));
        items
    }
}

/// An item of the review diff's editor menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMenuItem {
    AddComment {
        enabled: bool,
    },
    AddMultilineComment {
        enabled: bool,
    },
    /// "Mark File as Not Viewed" when `viewed`, else "Mark File as Viewed".
    ToggleViewed {
        viewed: bool,
    },
}

impl DiffMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            Self::AddComment { .. } => "Add Review Comment",
            Self::AddMultilineComment { .. } => "Add Multiline Review Comment",
            Self::ToggleViewed { viewed: true } => "Mark File as Not Viewed",
            Self::ToggleViewed { viewed: false } => "Mark File as Viewed",
        }
    }
}

/// The toolbar's button: `{0, choice, 0#Submit Review…|1#Submit Review ({0})…}`.
pub fn submit_review_text(pending_comments: u32, i18n: &mut I18n) -> String {
    if pending_comments > 0 {
        i18n.tx_built(&["Submit Review (", ")…"], &[Param::Number(f64::from(pending_comments))])
    } else {
        i18n.tx("Submit Review…")
    }
}

// ---------------------------------------------------------------------------
// Thread inlays

/// The compact thread view: more than 3 comments show the first, "… {n} more replies" and the last.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompactThread<'a> {
    pub shown: Vec<&'a GhReviewComment>,
    /// The "{n} more replies" link after the first, while folded.
    pub more: Option<String>,
}

pub fn compact_thread<'a>(thread: &'a GhReviewThread, expanded: bool, i18n: &mut I18n) -> CompactThread<'a> {
    let comments = &thread.comments.nodes;
    match (comments.first(), comments.last()) {
        (Some(first), Some(last)) if !expanded && comments.len() > 3 => CompactThread {
            shown: vec![first, last],
            more: Some(i18n.tx_built(&["", " more replies"], &[Param::from(comments.len() - 2)])),
        },
        _ => CompactThread { shown: comments.iter().collect(), more: None },
    }
}

/// "Resolve" / "Unresolve" of a thread, when the user may (`getResolveToggleActionText`).
pub fn resolve_action(thread: &GhReviewThread) -> Option<&'static str> {
    let can = if thread.is_resolved { thread.viewer_can_unresolve } else { thread.viewer_can_resolve };
    can.then_some(if thread.is_resolved { "Unresolve" } else { "Resolve" })
}

/// The request of [`resolve_action`]; its answer is the changed thread.
pub fn resolve_request(thread: &GhReviewThread) -> Result<GhApiRequest, String> {
    let name = if thread.is_resolved { "unresolveReviewThread" } else { "resolveReviewThread" };
    document_request(name, &[("threadId", Some(Value::from(thread.id.as_str())))])
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplyStage {
    CreateReview,
    AddComment { review_id: String, submit: bool },
    Submit,
}

/// A reply in flight, for [`reply_answered`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyTicket {
    stage: ReplyStage,
    in_reply_to: String,
    body: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReplyStep {
    /// Send `request`, then call [`reply_answered`].
    Send { ticket: ReplyTicket, request: GhApiRequest },
    /// The reply is posted: reload the pending review and the threads.
    Done,
}

fn add_comment(review_id: &str, in_reply_to: &str, body: &str) -> Result<GhApiRequest, String> {
    document_request(
        "addReviewComment",
        &[
            ("reviewId", Some(Value::from(review_id))),
            ("body", Some(Value::from(body))),
            ("inReplyTo", Some(Value::from(in_reply_to))),
        ],
    )
}

/// "Reply" to the thread whose first comment is `in_reply_to`: in the pending review; without one a
/// review is created and submitted at once (`COMMENT`), like GitHub's "Reply".
pub fn begin_reply(
    pending: Option<&PendingReview>,
    pull_request_id: &str,
    in_reply_to: &str,
    body: &str,
) -> Result<ReplyStep, String> {
    let ticket = |stage| ReplyTicket { stage, in_reply_to: in_reply_to.to_owned(), body: body.to_owned() };
    Ok(match pending {
        Some(p) => ReplyStep::Send {
            ticket: ticket(ReplyStage::AddComment { review_id: p.id.clone(), submit: false }),
            request: add_comment(&p.id, in_reply_to, body)?,
        },
        None => ReplyStep::Send {
            ticket: ticket(ReplyStage::CreateReview),
            request: document_request("createReview", &[("pullRequestId", Some(Value::from(pull_request_id)))])?,
        },
    })
}

/// Takes the answer of a reply step.
pub fn reply_answered(ticket: ReplyTicket, answer: Result<String, String>) -> Result<ReplyStep, String> {
    let text = answer?;
    match ticket.stage {
        ReplyStage::CreateReview => {
            #[derive(Deserialize)]
            struct Review {
                id: String,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Created {
                pull_request_review: Review,
            }
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct Data {
                add_pull_request_review: Created,
            }
            let id = super::api::graphql_data::<Data>(&text)
                .map_err(|e| e.message)?
                .add_pull_request_review
                .pull_request_review
                .id;
            let request = add_comment(&id, &ticket.in_reply_to, &ticket.body)?;
            Ok(ReplyStep::Send {
                ticket: ReplyTicket { stage: ReplyStage::AddComment { review_id: id, submit: true }, ..ticket },
                request,
            })
        }
        ReplyStage::AddComment { review_id, submit } => {
            super::api::graphql_data::<Value>(&text).map_err(|e| e.message)?;
            if !submit {
                return Ok(ReplyStep::Done);
            }
            let request = document_request(
                "submitReview",
                &[
                    ("reviewId", Some(Value::from(review_id))),
                    ("event", Some(Value::from("COMMENT"))),
                    ("body", Some(Value::Null)),
                ],
            )?;
            Ok(ReplyStep::Send { ticket: ReplyTicket { stage: ReplyStage::Submit, ..ticket }, request })
        }
        ReplyStage::Submit => {
            super::api::graphql_data::<Value>(&text).map_err(|e| e.message)?;
            Ok(ReplyStep::Done)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::diff::{ReviewDiffSettingsStore, SubmitAction, submit_actions};
    use crate::github::new_comments::fixtures::{COMMENT_ADDED, REVIEW_CREATED, THREAD_ADDED, document, variables};
    use crate::github::new_comments::{SubmitContext, SubmitStep, begin_submit, compare_answered};
    use crate::github::review_mode::fixtures::thread;
    use crate::github::types::{Coordinates, GhReviewComment, Nodes};
    use serde_json::json;

    const PATH: &str = "src/a.ts";
    const PATCH: &str = "@@ -1,6 +1,6 @@\n line 1\n line 2\n-line 3\n+line three\n line 4\n line 5\n line 6";

    fn file() -> CommitFile {
        CommitFile { filename: PATH.into(), patch: Some(PATCH.into()), ..CommitFile::default() }
    }

    fn threads() -> Vec<GhReviewThread> {
        let comment = |i: usize, body: &str| GhReviewComment {
            id: format!("C{i}"),
            body: body.into(),
            ..thread("x", 1).comments.nodes[0].clone()
        };
        let t1 = GhReviewThread {
            comments: Nodes::of(
                ["first", "second", "third", "fourth"].iter().enumerate().map(|(i, b)| comment(i, b)).collect(),
            ),
            ..thread("T1", 3)
        };
        vec![t1, GhReviewThread { is_resolved: true, ..thread("T2", 5) }]
    }

    fn diff(commit: Option<&str>) -> ReviewDiff {
        let context = json!({ "kind": PULL_REQUEST_DIFF, "number": 7, "path": PATH, "commit": commit });
        let (d, loads) = ReviewDiff::new(review_diff_context(Some(&context)).expect("a pull request diff"));
        assert_eq!(loads.len(), if commit.is_some() { 2 } else { 1 });
        d
    }

    /// The split viewer's pane of `side`: its own side's lines.
    fn split(side: Side) -> (DiffPane, impl Fn(Side, usize) -> Option<usize>) {
        (DiffPane { side, unified: false }, move |s: Side, l: usize| (s == side).then_some(l))
    }

    fn data<'a>(threads: &'a [GhReviewThread], files: &'a [CommitFile], view: ThreadsView) -> DiffData<'a> {
        DiffData { threads, threads_view: view, changes: Some(files), can_comment: true }
    }

    fn thread_inlays(p: &PaneReview) -> Vec<(&str, usize)> {
        p.inlays.iter().filter(|i| matches!(i.content, InlayContent::Thread(_))).map(|i| (i.key(), i.line)).collect()
    }

    fn ctx<'a>(repo: &'a Coordinates, pending: Option<&'a PendingReview>) -> SubmitContext<'a> {
        SubmitContext {
            repo,
            pull_request_id: "PR1",
            head_ref_oid: "head",
            base_ref_oid: "base",
            pending_review: pending,
        }
    }

    #[test]
    fn shows_threads_as_compact_inlays_under_their_lines_filtered_by_show_review_threads() {
        let (mut i18n, d, ts, files, nc) =
            (I18n::default(), diff(None), threads(), vec![file()], NewComments::default());
        let (pane, locate) = split(Side::Right);
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::All), &nc, &i18n);
        // Under line 3 ("line three") of the right side.
        assert_eq!(thread_inlays(&p), [("T1", 2), ("T2", 4)]);
        assert_eq!(p.gutter.lines_with_comments.len(), 2);
        let compact = compact_thread(&ts[0], false, &mut i18n);
        let bodies: Vec<&str> = compact.shown.iter().map(|c| c.body.as_str()).collect();
        assert_eq!((bodies, compact.more.as_deref()), (vec!["first", "fourth"], Some("2 more replies")));
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::Unresolved), &nc, &i18n);
        assert_eq!(thread_inlays(&p), [("T1", 2)]);
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::Hidden), &nc, &i18n);
        assert!(p.inlays.is_empty());
    }

    #[test]
    fn follows_the_threads_when_the_data_reloads() {
        let (i18n, d, files, nc) = (I18n::default(), diff(None), vec![file()], NewComments::default());
        let (pane, locate) = split(Side::Right);
        let fresh = vec![thread("T9", 1)];
        assert_eq!(
            thread_inlays(&d.pane(pane, &locate, &data(&fresh, &files, ThreadsView::All), &nc, &i18n)),
            [("T9", 0)]
        );
    }

    #[test]
    fn replies_to_and_resolves_a_thread() {
        let ts = threads();
        let pending = PendingReview { id: "PRR0".into(), ..PendingReview::default() };
        let ReplyStep::Send { ticket, request } =
            begin_reply(Some(&pending), "PR1", &ts[0].comments.nodes[0].id, "thanks").expect("sent")
        else {
            panic!("the reply is sent")
        };
        assert_eq!(variables(&request), json!({ "reviewId": "PRR0", "body": "thanks", "inReplyTo": "C0" }));
        assert_eq!(reply_answered(ticket, Ok(COMMENT_ADDED.into())), Ok(ReplyStep::Done));
        assert_eq!(resolve_action(&ts[0]), Some("Resolve"));
        let request = resolve_request(&ts[0]).expect("built");
        assert!(document(&request).contains("resolveReviewThread("));
        assert_eq!(variables(&request), json!({ "threadId": "T1" }));
    }

    #[test]
    fn start_review_creates_a_pending_review_with_the_comments_thread_and_becomes_the_preferred_action() {
        let (i18n, d, ts, files) = (I18n::default(), diff(None), threads(), vec![file()]);
        let mut nc = NewComments::default();
        let id = d.request(&mut nc, Side::Right, 1, 3, "now");
        // Without a pending review: Start Review (preferred, primary) and Add Single Comment.
        assert_eq!(submit_actions(false, true), [SubmitAction::StartReview, SubmitAction::SingleComment]);
        let (pane, locate) = split(Side::Right);
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::All), &nc, &i18n);
        let outline = &p.gutter.outlines[0];
        assert_eq!((outline.start, outline.end, outline.adjustable), (1, 3, true));
        assert_eq!(outline.tooltip, "Drag to select several lines for commenting");
        let repo = Coordinates::new("o", "r");
        let c = nc.get(&id).cloned().expect("open");
        let SubmitStep::Send { ticket, request } =
            begin_submit(&c, SubmitAction::StartReview, "Looks odd", ctx(&repo, None)).expect("sent")
        else {
            panic!("a review is created")
        };
        assert_eq!(
            variables(&request),
            json!({ "pullRequestId": "PR1", "commitOid": "head",
                    "threads": [{ "body": "Looks odd", "path": PATH, "line": 4, "side": "RIGHT", "startLine": 2, "startSide": "RIGHT" }] })
        );
        let done = nc.submit_answered(ticket, Ok(REVIEW_CREATED.into())).expect("created");
        assert!(nc.comments.is_empty());
        let mut settings = ReviewDiffSettingsStore::default();
        settings.set(|s| {
            s.review_comments_preferred = done.review_comments_preferred.unwrap_or(s.review_comments_preferred)
        });
        assert!(settings.settings.review_comments_preferred);
    }

    #[test]
    fn add_single_comment_posts_the_comment_at_once_and_becomes_the_primary_button() {
        let (i18n, d, ts, files) = (I18n::default(), diff(None), threads(), vec![file()]);
        let mut nc = NewComments::default();
        let id = d.request(&mut nc, Side::Left, 4, 4, "now");
        // A left-side comment goes into the left pane.
        let (left, locate_left) = split(Side::Left);
        let p = d.pane(left, &locate_left, &data(&ts, &files, ThreadsView::All), &nc, &i18n);
        assert!(p.inlays.iter().any(|i| i.key() == id));
        let (right, locate_right) = split(Side::Right);
        assert!(
            !d.pane(right, &locate_right, &data(&ts, &files, ThreadsView::All), &nc, &i18n)
                .inlays
                .iter()
                .any(|i| i.key() == id)
        );
        let repo = Coordinates::new("o", "r");
        let c = nc.get(&id).cloned().expect("open");
        let SubmitStep::Send { ticket, request } =
            begin_submit(&c, SubmitAction::SingleComment, "hm", ctx(&repo, None)).expect("sent")
        else {
            panic!("a review is created")
        };
        let vars = variables(&request);
        assert_eq!(vars["event"], "COMMENT");
        assert_eq!(
            vars["threads"],
            json!([{ "body": "hm", "path": PATH, "line": 5, "side": "LEFT", "startLine": null, "startSide": null }])
        );
        let done = nc.submit_answered(ticket, Ok(REVIEW_CREATED.into())).expect("created");
        assert_eq!(done.review_comments_preferred, Some(false));
        assert_eq!(submit_actions(false, false)[0], SubmitAction::SingleComment);
    }

    #[test]
    fn add_review_comment_adds_a_thread_to_the_pending_review() {
        let mut i18n = I18n::default();
        let pending = PendingReview { id: "PRR0".into(), comments_count: 2, ..PendingReview::default() };
        assert_eq!(submit_review_text(pending.comments_count, &mut i18n), "Submit Review (2)…");
        let d = diff(None);
        let mut nc = NewComments::default();
        let id = d.request(&mut nc, Side::Right, 0, 0, "now");
        assert_eq!(submit_actions(true, true), [SubmitAction::ReviewComment]);
        let repo = Coordinates::new("o", "r");
        let c = nc.get(&id).cloned().expect("open");
        let SubmitStep::Send { ticket, request } =
            begin_submit(&c, SubmitAction::ReviewComment, "nit", ctx(&repo, Some(&pending))).expect("sent")
        else {
            panic!("a thread is added")
        };
        assert_eq!(
            variables(&request),
            json!({ "pullRequestReviewId": "PRR0", "body": "nit", "path": PATH, "line": 1, "side": "RIGHT", "startLine": null, "startSide": null })
        );
        nc.submit_answered(ticket, Ok(THREAD_ADDED.into())).expect("added");
    }

    #[test]
    fn comments_on_a_single_commits_diff_at_the_lines_diff_position_with_a_fixed_range() {
        let (i18n, ts, files) = (I18n::default(), threads(), vec![file()]);
        let mut d = diff(Some("c1"));
        d.commit_files_loaded(&files);
        let mut nc = NewComments::default();
        let (pane, locate) = split(Side::Right);
        // The threads were made on another commit.
        assert!(thread_inlays(&d.pane(pane, &locate, &data(&ts, &files, ThreadsView::All), &nc, &i18n)).is_empty());
        let id = d.request(&mut nc, Side::Right, 2, 2, "now");
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::All), &nc, &i18n);
        assert_eq!(p.gutter.outlines[0].tooltip, "Comment range cannot be adjusted while reviewing a single commit");
        assert!(!p.gutter.outlines[0].adjustable);
        let pending = PendingReview { id: "PRR0".into(), ..PendingReview::default() };
        let repo = Coordinates::new("o", "r");
        let c = nc.get(&id).cloned().expect("open");
        let SubmitStep::Compare { ticket, request } =
            begin_submit(&c, SubmitAction::ReviewComment, "here", ctx(&repo, Some(&pending))).expect("sent")
        else {
            panic!("the commit's files are asked for first")
        };
        assert_eq!(request.endpoint, "repos/o/r/compare/base...c1");
        let answer = json!({ "files": [{ "filename": PATH, "patch": PATCH }] }).to_string();
        let SubmitStep::Send { ticket, request } = compare_answered(ticket, Ok(answer)).expect("mapped") else {
            panic!("the comment is sent")
        };
        assert_eq!(
            variables(&request),
            json!({ "reviewId": "PRR0", "body": "here", "commit": "c1", "file": PATH, "position": 4 })
        );
        nc.submit_answered(ticket, Ok(COMMENT_ADDED.into())).expect("added");
        assert!(nc.comments.is_empty());
    }

    #[test]
    fn discards_an_empty_new_comment_on_escape() {
        let d = diff(None);
        let mut nc = NewComments::default();
        let id = d.request(&mut nc, Side::Right, 1, 1, "now");
        assert_eq!(nc.discard(&id), Discard::Removed);
        assert!(nc.comments.is_empty());
    }

    #[test]
    fn marks_the_file_viewed_from_the_editor_menu() {
        let (i18n, d, ts, files, nc) = (I18n::default(), diff(None), threads(), vec![file()], NewComments::default());
        let (pane, locate) = split(Side::Right);
        let p = d.pane(pane, &locate, &data(&ts, &files, ThreadsView::All), &nc, &i18n);
        // The caret on "line 2".
        let menu = d.context_menu(&p.gutter, 1, (1, 1), Some(FileViewedState::Unviewed));
        let labels: Vec<&str> = menu.iter().map(|i| i.label()).collect();
        assert_eq!(labels, ["Add Review Comment", "Add Multiline Review Comment", "Mark File as Viewed"]);
        assert_eq!(menu[0], DiffMenuItem::AddComment { enabled: true });
        let request = d.set_viewed_request("PR1", true).expect("built");
        assert!(document(&request).contains("markFileAsViewed("));
        assert_eq!(variables(&request), json!({ "pullRequestId": "PR1", "path": PATH }));
    }

    #[test]
    fn marks_opened_files_viewed_when_automatically_mark_opened_files_as_viewed_is_on() {
        let d = diff(None);
        let request = d.auto_mark_viewed(true, Some(FileViewedState::Unviewed), "PR1").expect("marked");
        assert_eq!(variables(&request), json!({ "pullRequestId": "PR1", "path": PATH }));
        assert_eq!(d.auto_mark_viewed(false, Some(FileViewedState::Unviewed), "PR1"), None);
        assert_eq!(d.auto_mark_viewed(true, Some(FileViewedState::Viewed), "PR1"), None);
    }

    #[test]
    fn does_not_mark_a_single_commits_file_viewed() {
        let mut i18n = I18n::default();
        let d = diff(Some("c1"));
        assert_eq!(submit_review_text(0, &mut i18n), "Submit Review…");
        assert_eq!(d.auto_mark_viewed(true, Some(FileViewedState::Unviewed), "PR1"), None);
        assert!(!d.can_navigate());
    }
}
