//! What a review shows in an editor pane (the diff's panes and review mode's editor): the inlays
//! under lines (review threads and new comments) and the review gutter's state (comment icons, the
//! "+" on commentable lines, the ranges of new comments being written).
//!
//! Lines are the pane's, 0-based; ranges are half-open.

use super::diff::Span;
use crate::platform::i18n::I18n;
use std::collections::BTreeSet;

/// What an inlay shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlayContent {
    /// The review thread with this id (the timeline's compact thread view).
    Thread(String),
    /// The new comment with this id ([`super::new_comments::NewComment`]).
    NewComment(String),
}

/// An inlay under `line`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inlay {
    pub line: usize,
    pub content: InlayContent,
}

impl Inlay {
    pub fn key(&self) -> &str {
        match &self.content {
            InlayContent::Thread(id) | InlayContent::NewComment(id) => id,
        }
    }
}

/// Why a new comment's range can't be dragged (`AdjustmentDisabledReason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutlineLock {
    /// The comment is on a single commit's diff.
    Commit,
    /// Its text holds a suggested change.
    Suggestion,
}

/// The tooltip of a new comment's range handles: how to adjust it, or why it can't be.
pub fn outline_tooltip(lock: Option<OutlineLock>, i18n: &I18n) -> String {
    i18n.tx(match lock {
        Some(OutlineLock::Commit) => "Comment range cannot be adjusted while reviewing a single commit",
        Some(OutlineLock::Suggestion) => "The range of this comment is not adjustable as it contains suggestion",
        None => "Drag to select several lines for commenting",
    })
}

/// The range of a comment being written, lines `start..=end`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outline {
    pub key: String,
    pub start: usize,
    pub end: usize,
    /// Its ends can be dragged.
    pub adjustable: bool,
    pub tooltip: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GutterState {
    pub lines_with_comments: BTreeSet<usize>,
    pub lines_with_new_comments: BTreeSet<usize>,
    /// Lines whose inlays are shown (there is room for a second icon under the line).
    pub lines_with_inlays: BTreeSet<usize>,
    /// Commentable lines.
    pub commentable: Vec<Span>,
    pub outlines: Vec<Outline>,
}

impl GutterState {
    pub fn is_line_commentable(&self, line: usize) -> bool {
        self.commentable.iter().any(|r| line >= r.start && line < r.end)
    }

    /// Both ends of `start..=end` are commentable (`canCreateComment`); a pane shows one side.
    pub fn can_create_comment(&self, start: usize, end: usize) -> bool {
        self.is_line_commentable(start) && self.is_line_commentable(end)
    }

    /// The lines "Add Review Comment" (the caret's line) or "Add Multiline Review Comment" (the
    /// selected lines `selection`, at least two) would comment on, when they can.
    pub fn comment_range(&self, multiline: bool, selection: (usize, usize), caret: usize) -> Option<(usize, usize)> {
        if multiline {
            let (start, end) = selection;
            (start < end && self.can_create_comment(start, end)).then_some((start, end))
        } else {
            self.is_line_commentable(caret).then_some((caret, caret))
        }
    }
}

/// The inlays and gutter state of a pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaneReview {
    pub inlays: Vec<Inlay>,
    pub gutter: GutterState,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the commentable lines decide what the comment actions offer.
    #[test]
    fn offers_comments_on_commentable_lines_only() {
        let gutter = GutterState { commentable: vec![Span { start: 2, end: 5 }], ..GutterState::default() };
        assert_eq!(gutter.comment_range(false, (0, 0), 4), Some((4, 4)));
        assert_eq!(gutter.comment_range(false, (0, 0), 5), None);
        assert_eq!(gutter.comment_range(true, (2, 4), 4), Some((2, 4)));
        assert_eq!(gutter.comment_range(true, (3, 3), 3), None);
        assert_eq!(gutter.comment_range(true, (1, 4), 4), None);
    }
}
