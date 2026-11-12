//! New review comments being written in diffs and editors (IntelliJ's
//! `GHPRThreadsViewModels.newComments` with `GHPRReviewNewCommentEditorViewModel`): kept per pull
//! request, file and diff (the cumulative one or a commit's) while the diff tabs come and go, and
//! submitted with
//!
//! - "Start Review" / "Add Single Comment" without a pending review: `createReview` with the
//!   comment as its thread (pending, or posted at once with `event: COMMENT`); the choice becomes
//!   the preferred button;
//! - "Add Review Comment" with one: `addPullRequestReviewThread` on the cumulative diff, a commit
//!   comment at the line's diff position (merge base ↔ commit) on a commit's diff.
//!
//! Submitting does no I/O: [`begin_submit`] hands out the request to send (a commit comment first
//! asks for the commit's patch), [`NewComments::submit_answered`] takes the answer and closes the
//! comment.

use super::api::{graphql_data, graphql_request, parse_rest, repo_path, rest_request};
use super::diff::{CommentLocation, Side, SubmitAction, diff_position, draft_thread};
use super::graphql::query;
use super::types::{Coordinates, GhReviewThread, PendingReview};
use crate::platform::i18n::I18n;
use ide_github::GhApiRequest;
use serde::Deserialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewComment {
    pub id: String,
    pub number: u32,
    /// Repository-relative path.
    pub path: String,
    /// The commit whose diff it is on; `None` for the pull request's changes.
    pub commit: Option<String>,
    pub location: CommentLocation,
    pub text: String,
    pub created_at: String,
    /// Bumped to move the focus into its editor.
    pub focus: u32,
}

impl NewComment {
    fn on(&self, number: u32, path: &str, commit: Option<&str>) -> bool {
        self.number == number && self.path == path && self.commit.as_deref() == commit
    }
}

/// What closing a new comment asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discard {
    /// It was empty and is closed.
    Removed,
    /// It has text: ask "Are you sure you want to discard the unsaved comment?" ("Discard Comment",
    /// Yes / No), then [`NewComments::remove`] on Yes.
    Confirm,
    /// No such comment.
    Missing,
}

/// The "Discard Comment" question: its title and text.
pub fn discard_question(i18n: &I18n) -> (String, String) {
    (i18n.tx("Discard Comment"), i18n.tx("Are you sure you want to discard the unsaved comment?"))
}

/// The new comments of every diff.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewComments {
    pub comments: Vec<NewComment>,
    seq: u64,
}

impl NewComments {
    /// The new comments of a diff.
    pub fn of(&self, number: u32, path: &str, commit: Option<&str>) -> Vec<&NewComment> {
        self.comments.iter().filter(|c| c.on(number, path, commit)).collect()
    }

    pub fn get(&self, id: &str) -> Option<&NewComment> {
        self.comments.iter().find(|c| c.id == id)
    }

    fn get_mut(&mut self, id: &str) -> Option<&mut NewComment> {
        self.comments.iter_mut().find(|c| c.id == id)
    }

    /// `requestNewComment`: opens a new comment at `location`, or focuses the one already there;
    /// `now` is the creation time (ISO 8601). Returns its id.
    pub fn request(
        &mut self,
        number: u32,
        path: &str,
        commit: Option<&str>,
        location: CommentLocation,
        now: &str,
    ) -> String {
        let existing = self.comments.iter_mut().find(|c| {
            c.on(number, path, commit)
                && c.location.side == location.side
                && c.location.line == location.line
                && c.location.start_line == location.start_line
        });
        if let Some(c) = existing {
            c.focus += 1;
            return c.id.clone();
        }
        self.seq += 1;
        let id = format!("new-{}", self.seq);
        self.comments.push(NewComment {
            id: id.clone(),
            number,
            path: path.to_owned(),
            commit: commit.map(str::to_owned),
            location,
            text: String::new(),
            created_at: now.to_owned(),
            focus: 1,
        });
        id
    }

    pub fn set_text(&mut self, id: &str, text: &str) {
        if let Some(c) = self.get_mut(id) {
            c.text = text.to_owned();
        }
    }

    /// The range was dragged.
    pub fn set_location(&mut self, id: &str, location: CommentLocation) {
        if let Some(c) = self.get_mut(id) {
            c.location = location;
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.comments.retain(|c| c.id != id);
    }

    /// Escape or the gutter's cancel: an empty comment closes at once.
    pub fn discard(&mut self, id: &str) -> Discard {
        match self.get(id) {
            None => Discard::Missing,
            Some(c) if c.text.trim().is_empty() => {
                self.remove(id);
                Discard::Removed
            }
            Some(_) => Discard::Confirm,
        }
    }

    /// Forgets every new comment (another repository).
    pub fn clear(&mut self) {
        self.comments.clear();
    }

    /// Takes the answer of a submit request; on success the comment is closed.
    pub fn submit_answered(
        &mut self,
        ticket: SubmitTicket,
        answer: Result<String, String>,
    ) -> Result<Submitted, String> {
        let text = answer?;
        let submitted = match ticket.kind {
            SubmitKind::Review { prefer_review } => {
                graphql_data::<Value>(&text).map_err(|e| e.message)?;
                Submitted { review_comments_preferred: Some(prefer_review), thread: None }
            }
            SubmitKind::Thread => {
                #[derive(Deserialize)]
                struct Thread {
                    thread: GhReviewThread,
                }
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase")]
                struct Data {
                    add_pull_request_review_thread: Thread,
                }
                let data = graphql_data::<Data>(&text).map_err(|e| e.message)?;
                Submitted { review_comments_preferred: None, thread: Some(data.add_pull_request_review_thread.thread) }
            }
            SubmitKind::CommitComment => {
                graphql_data::<Value>(&text).map_err(|e| e.message)?;
                Submitted { review_comments_preferred: None, thread: None }
            }
        };
        self.remove(&ticket.comment_id);
        Ok(submitted)
    }
}

// ---------------------------------------------------------------------------
// Submitting

/// What the submit needs to know of the pull request.
#[derive(Clone, Copy, Debug)]
pub struct SubmitContext<'a> {
    pub repo: &'a Coordinates,
    /// The pull request's node id.
    pub pull_request_id: &'a str,
    pub head_ref_oid: &'a str,
    pub base_ref_oid: &'a str,
    pub pending_review: Option<&'a PendingReview>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SubmitKind {
    /// `createReview`; "Start Review" is preferred afterwards when `prefer_review`.
    Review { prefer_review: bool },
    /// `addPullRequestReviewThread`.
    Thread,
    /// `addPullRequestReviewComment` at a diff position.
    CommitComment,
}

/// A submit request in flight, for [`NewComments::submit_answered`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitTicket {
    pub comment_id: String,
    kind: SubmitKind,
}

/// The commit's changed files are needed first (`compare/{base}...{commit}`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareTicket {
    comment_id: String,
    review_id: String,
    body: String,
    commit: String,
    path: String,
    side: Side,
    line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SubmitStep {
    /// Send `request`, then call [`NewComments::submit_answered`].
    Send { ticket: SubmitTicket, request: GhApiRequest },
    /// Send `request` (the commit's files), then call [`compare_answered`].
    Compare { ticket: CompareTicket, request: GhApiRequest },
}

/// What a successful submit changed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Submitted {
    /// Store this as the review-in-diff settings' `reviewCommentsPreferred` (a review was created).
    pub review_comments_preferred: Option<bool>,
    /// The created thread (put it into the pull request's threads; then reload the pending review
    /// and the timeline). Without one, reload the pending review and the threads.
    pub thread: Option<GhReviewThread>,
}

/// The `gh_api` request of the GraphQL document `name`.
pub(crate) fn document_request(name: &str, variables: &[(&str, Option<Value>)]) -> Result<GhApiRequest, String> {
    let doc = query(name).ok_or_else(|| format!("unknown GraphQL document {name}"))?;
    graphql_request(doc, variables)
}

fn text(s: &str) -> Option<Value> {
    Some(Value::from(s))
}

/// Starts sending `body` with `action` for the new comment `c`.
pub fn begin_submit(
    c: &NewComment,
    action: SubmitAction,
    body: &str,
    ctx: SubmitContext<'_>,
) -> Result<SubmitStep, String> {
    let draft = draft_thread(&c.path, c.location, body);
    if action == SubmitAction::ReviewComment {
        let pending = ctx.pending_review.ok_or("No pending review")?;
        let Some(commit) = &c.commit else {
            let null_or = |v: Option<Value>| Some(v.unwrap_or(Value::Null));
            let request = document_request(
                "addPullRequestReviewThread",
                &[
                    ("pullRequestReviewId", text(&pending.id)),
                    ("body", text(body)),
                    ("path", text(&c.path)),
                    ("line", Some(Value::from(draft.line))),
                    ("side", serde_json::to_value(draft.side).ok()),
                    ("startLine", null_or(draft.start_line.map(Value::from))),
                    ("startSide", null_or(draft.start_side.and_then(|s| serde_json::to_value(s).ok()))),
                ],
            )?;
            return Ok(SubmitStep::Send {
                ticket: SubmitTicket { comment_id: c.id.clone(), kind: SubmitKind::Thread },
                request,
            });
        };
        let endpoint = format!("{}/compare/{}...{commit}", repo_path(ctx.repo), ctx.base_ref_oid);
        return Ok(SubmitStep::Compare {
            ticket: CompareTicket {
                comment_id: c.id.clone(),
                review_id: pending.id.clone(),
                body: body.to_owned(),
                commit: commit.clone(),
                path: c.path.clone(),
                side: c.location.side,
                line: c.location.line,
            },
            request: rest_request("GET", &endpoint, None, false),
        });
    }
    let single = action == SubmitAction::SingleComment;
    let threads = serde_json::to_value([draft]).map_err(|e| e.to_string())?;
    let request = document_request(
        "createReview",
        &[
            ("pullRequestId", text(ctx.pull_request_id)),
            ("event", single.then(|| Value::from("COMMENT"))),
            ("commitOid", text(c.commit.as_deref().unwrap_or(ctx.head_ref_oid))),
            ("threads", Some(threads)),
        ],
    )?;
    Ok(SubmitStep::Send {
        ticket: SubmitTicket { comment_id: c.id.clone(), kind: SubmitKind::Review { prefer_review: !single } },
        request,
    })
}

/// Takes the commit's changed files (REST `compare`) and comments at the line's diff position.
pub fn compare_answered(ticket: CompareTicket, answer: Result<String, String>) -> Result<SubmitStep, String> {
    let files = parse_rest(&answer?, false).map_err(|e| e.to_string())?;
    let patch = files
        .as_ref()
        .and_then(|f| f.get("files"))
        .and_then(Value::as_array)
        .and_then(|files| files.iter().find(|f| f.get("filename").and_then(Value::as_str) == Some(&ticket.path)))
        .and_then(|f| f.get("patch"))
        .and_then(Value::as_str);
    let position = diff_position(patch, ticket.side, ticket.line).ok_or("Can't map file line to diff")?;
    let request = document_request(
        "addReviewComment",
        &[
            ("reviewId", text(&ticket.review_id)),
            ("body", text(&ticket.body)),
            ("commit", text(&ticket.commit)),
            ("file", text(&ticket.path)),
            ("position", Some(Value::from(position))),
        ],
    )?;
    Ok(SubmitStep::Send {
        ticket: SubmitTicket { comment_id: ticket.comment_id, kind: SubmitKind::CommitComment },
        request,
    })
}

#[cfg(test)]
pub(crate) mod fixtures {
    use super::*;

    /// The GraphQL variables of a request.
    pub fn variables(request: &GhApiRequest) -> Value {
        let body: Value = serde_json::from_str(request.body.as_deref().unwrap_or("{}")).expect("JSON body");
        body["variables"].clone()
    }

    /// The document a GraphQL request sends.
    pub fn document(request: &GhApiRequest) -> String {
        let body: Value = serde_json::from_str(request.body.as_deref().unwrap_or("{}")).expect("JSON body");
        body["query"].as_str().unwrap_or_default().to_owned()
    }

    pub const REVIEW_CREATED: &str = r#"{"data":{"addPullRequestReview":{"pullRequestReview":{"id":"PRR1","state":"PENDING","comments":{"totalCount":1}}}}}"#;
    pub const THREAD_ADDED: &str =
        r#"{"data":{"addPullRequestReviewThread":{"thread":{"id":"T-new","path":"src/a.ts","line":4}}}}"#;
    pub const COMMENT_ADDED: &str = r#"{"data":{"addPullRequestReviewComment":{"comment":{"id":"C9","body":"c"}}}}"#;
}

#[cfg(test)]
mod tests {
    use super::fixtures::*;
    use super::*;
    use crate::github::diff::new_comment_location;
    use serde_json::json;

    // Not from a spec: a request at a place that has a comment focuses it, Escape closes empty ones.
    #[test]
    fn opens_one_comment_per_place_and_discards_empty_ones() {
        let mut c = NewComments::default();
        let a = c.request(7, "a.ts", None, new_comment_location(Side::Right, 1, 1), "t");
        let again = c.request(7, "a.ts", None, new_comment_location(Side::Right, 1, 1), "t");
        assert_eq!(a, again);
        assert_eq!(c.get(&a).map(|x| x.focus), Some(2));
        let other = c.request(7, "a.ts", Some("c1"), new_comment_location(Side::Right, 1, 1), "t");
        assert_ne!(a, other);
        assert_eq!(c.of(7, "a.ts", None).len(), 1);
        c.set_text(&a, "x");
        assert_eq!(c.discard(&a), Discard::Confirm);
        assert_eq!(c.discard(&other), Discard::Removed);
        assert_eq!(c.comments.len(), 1);
        let i18n = I18n::default();
        assert_eq!(discard_question(&i18n).0, "Discard Comment");
    }

    // Not from a spec: what each action sends (the specs of the review diff and review mode check
    // the same through their flows).
    #[test]
    fn sends_start_review_without_an_event_and_closes_the_comment() {
        let repo = Coordinates::new("o", "r");
        let ctx = SubmitContext {
            repo: &repo,
            pull_request_id: "PR1",
            head_ref_oid: "head",
            base_ref_oid: "base",
            pending_review: None,
        };
        let mut c = NewComments::default();
        let id = c.request(7, "a.ts", None, new_comment_location(Side::Right, 0, 0), "t");
        let comment = c.get(&id).cloned().expect("opened");
        assert_eq!(begin_submit(&comment, SubmitAction::ReviewComment, "x", ctx), Err("No pending review".into()));
        let SubmitStep::Send { ticket, request } =
            begin_submit(&comment, SubmitAction::StartReview, "x", ctx).expect("sent")
        else {
            panic!("a review is created at once")
        };
        assert!(document(&request).contains("addPullRequestReview("));
        assert_eq!(
            variables(&request),
            json!({ "pullRequestId": "PR1", "commitOid": "head",
                    "threads": [{ "body": "x", "path": "a.ts", "line": 1, "side": "RIGHT", "startLine": null, "startSide": null }] })
        );
        let done = c.submit_answered(ticket, Ok(REVIEW_CREATED.into())).expect("created");
        assert_eq!(done.review_comments_preferred, Some(true));
        assert!(c.comments.is_empty());
    }
}
