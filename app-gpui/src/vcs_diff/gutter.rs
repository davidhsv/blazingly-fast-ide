//! Change markers of a text editor against HEAD: coloured bars beside added and modified lines and a
//! notch where lines were deleted, with the hunks' extents kept in step with the buffer as it is
//! edited, so that a rollback stays exact between two answers of the backend.
//!
//! Offsets are UTF-16 code units; a fold is the range of offsets it hides.

use super::doc::{Direction, Doc};
use super::marks::{MarkKind, ScrollbarMark};
use crate::editor::buffer::{Assoc, map_offset};
use ide_core::TextChange;
use ide_vcs::{HunkKind, LineHunk};
use std::borrow::Borrow;
use std::collections::HashSet;
use std::ops::Range;

/// A hunk with its current extent in the document.
#[derive(Clone, Debug)]
pub struct EditorHunk {
    pub hunk: LineHunk,
    pub from: usize,
    pub to: usize,
}

impl Borrow<LineHunk> for EditorHunk {
    fn borrow(&self) -> &LineHunk {
        &self.hunk
    }
}

impl EditorHunk {
    /// Whether the hunk has no text in the document: a deletion, or lines that were removed since.
    fn is_point(&self) -> bool {
        self.hunk.kind == HunkKind::Deleted || self.to <= self.from
    }

    /// Start of the line the hunk's marker is on, for a hunk without text.
    fn marker_line(&self, doc: &Doc) -> usize {
        doc.line_at(self.from.min(doc.len())).from
    }
}

/// Name of the theme token colouring the markers of `kind`.
pub fn color_token(kind: HunkKind) -> &'static str {
    match kind {
        HunkKind::Added => "gutter-added",
        HunkKind::Modified => "gutter-modified",
        HunkKind::Deleted => "gutter-deleted",
    }
}

/// English tooltip of the overview mark of `kind`.
pub fn tooltip(kind: HunkKind) -> &'static str {
    match kind {
        HunkKind::Added => "Added lines",
        HunkKind::Modified => "Modified lines",
        HunkKind::Deleted => "Deleted lines",
    }
}

/// English title of a marker of `kind` in the change lane.
pub fn title(kind: HunkKind) -> &'static str {
    match kind {
        HunkKind::Added => "Added lines (click to show)",
        HunkKind::Modified => "Modified lines (click to show)",
        HunkKind::Deleted => "Deleted lines (click to show)",
    }
}

/// How a marker is drawn in the change lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneShape {
    /// A bar the height of the line.
    Bar,
    /// A notch at the top of the line: lines were removed above it.
    Deleted,
}

/// A marker in the change lane, on the line starting at `from`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LaneMarker {
    pub from: usize,
    /// What the marker stands for; gives its colour ([`color_token`]) and title ([`title`]).
    pub kind: HunkKind,
    pub shape: LaneShape,
}

fn build_markers(hunks: &[EditorHunk], doc: &Doc) -> Vec<LaneMarker> {
    let mut markers = Vec::new();
    for h in hunks {
        if h.is_point() {
            markers.push(LaneMarker { from: h.marker_line(doc), kind: HunkKind::Deleted, shape: LaneShape::Deleted });
            continue;
        }
        let first = doc.line_at(h.from).number;
        let last = doc.line_at(h.from.max(h.to - 1)).number;
        let kind = if h.hunk.kind == HunkKind::Added { HunkKind::Added } else { HunkKind::Modified };
        markers.extend((first..=last).map(|n| LaneMarker { from: doc.line(n).from, kind, shape: LaneShape::Bar }));
    }
    markers.sort_by_key(|marker| marker.from);
    markers
}

/// Whether a fold touches `[from, to]`.
fn touching(folds: &[Range<usize>], from: usize, to: usize) -> impl Iterator<Item = &Range<usize>> {
    folds.iter().filter(move |fold| fold.start <= to && fold.end >= from)
}

/// The hunks of one editor and their markers.
#[derive(Clone, Debug, Default)]
pub struct HunkState {
    hunks: Vec<EditorHunk>,
    markers: Vec<LaneMarker>,
}

impl HunkState {
    /// Takes the backend's hunks for the text of `doc`.
    pub fn set_hunks(&mut self, hunks: &[LineHunk], doc: &Doc) {
        let at = |line: u32| if (line as usize) < doc.lines() { doc.line(line as usize + 1).from } else { doc.len() };
        self.hunks = hunks
            .iter()
            .map(|h| EditorHunk {
                hunk: h.clone(),
                from: at(h.start),
                to: if h.lines == 0 { at(h.start) } else { at(h.start + h.lines) },
            })
            .collect();
        self.markers = build_markers(&self.hunks, doc);
    }

    /// Moves the hunks through an edit; `doc` is the document after `changes`.
    pub fn map_through(&mut self, changes: &[TextChange], doc: &Doc) {
        if changes.is_empty() || self.hunks.is_empty() {
            return;
        }
        let map = |offset: usize, assoc: Assoc| map_offset(offset as u32, changes, assoc) as usize;
        for h in &mut self.hunks {
            let from = map(h.from, Assoc::Before);
            let to = map(h.to, if h.to == h.from { Assoc::Before } else { Assoc::After });
            h.from = from;
            h.to = to.max(from);
        }
        self.markers = build_markers(&self.hunks, doc);
    }

    pub fn hunks(&self) -> &[EditorHunk] {
        &self.hunks
    }

    /// The markers by line, hidden lines included.
    pub fn markers(&self) -> &[LaneMarker] {
        &self.markers
    }

    /// The hunk shown on the line starting at `line_from`; a folded line shows the hunks it hides.
    pub fn hunk_at_pos(&self, doc: &Doc, folds: &[Range<usize>], line_from: usize) -> Option<&EditorHunk> {
        let line = doc.line_at(line_from);
        let direct = self.hunks.iter().find(|h| {
            if h.is_point() { h.marker_line(doc) == line.from } else { h.from <= line.from && line.from < h.to }
        });
        if direct.is_some() {
            return direct;
        }
        let end = touching(folds, line.from, line.to).fold(line.to, |end, fold| end.max(fold.end));
        if end > line.to { self.hunks.iter().find(|h| h.from > line.to && h.from <= end) } else { None }
    }

    /// The markers as the lane draws them: those of lines hidden in a fold move to the folded line,
    /// which shows one marker.
    pub fn visible_markers(&self, doc: &Doc, folds: &[Range<usize>]) -> Vec<LaneMarker> {
        if folds.is_empty() || self.markers.is_empty() {
            return self.markers.clone();
        }
        let mut seen = HashSet::new();
        let mut moved = Vec::new();
        for marker in &self.markers {
            let from = marker.from;
            // Folds come in document order; the last one hiding the line decides.
            let hiding = folds.iter().rev().find(|fold| fold.start < from && from <= fold.end);
            let at = hiding.map_or(from, |fold| doc.line_at(fold.start).from);
            if seen.insert(at) {
                moved.push(LaneMarker { from: at, ..*marker });
            }
        }
        moved.sort_by_key(|marker| marker.from);
        moved
    }

    /// The hunks on the overview lane.
    pub fn scroll_marks(&self) -> Vec<ScrollbarMark> {
        self.hunks
            .iter()
            .map(|h| ScrollbarMark {
                from: h.from,
                to: Some(if h.to > h.from { h.to - 1 } else { h.from }),
                color: color_token(h.hunk.kind),
                kind: MarkKind::Line,
                tooltip: Some(tooltip(h.hunk.kind)),
                priority: 0,
            })
            .collect()
    }

    /// The change to move the caret to from `head`, wrapping around, with the caret's new offset.
    pub fn goto_change(&self, doc: &Doc, head: usize, direction: Direction) -> Option<(usize, &EditorHunk)> {
        let line_from = doc.line_at(head).from;
        let mut sorted: Vec<&EditorHunk> = self.hunks.iter().collect();
        sorted.sort_by_key(|h| h.from);
        let target = match direction {
            Direction::Next => sorted
                .iter()
                .find(|h| h.from > line_from && doc.line_at(h.from).from > line_from)
                .or_else(|| sorted.first()),
            Direction::Previous => sorted
                .iter()
                .rev()
                .find(|h| h.marker_line(doc) < line_from && (head < h.from || head >= h.to))
                .or_else(|| sorted.last()),
        };
        target.map(|h| (h.from.min(doc.len()), *h))
    }

    /// The hunks touched by the selection `ranges`; a deletion counts on the line its marker is on.
    pub fn hunks_in_selection(&self, doc: &Doc, ranges: &[Range<usize>]) -> Vec<&EditorHunk> {
        self.hunks
            .iter()
            .filter(|h| {
                ranges.iter().any(|range| {
                    let first = doc.line_at(range.start).from;
                    let last = doc.line_at(range.end).to;
                    if h.is_point() {
                        let marker = h.marker_line(doc);
                        return first <= marker && marker <= last;
                    }
                    h.from <= last && first < h.to
                })
            })
            .collect()
    }

    /// The edit restoring the hunks touched by the selection; empty when it touches none.
    pub fn rollback_selected_lines(&self, doc: &Doc, ranges: &[Range<usize>]) -> Vec<TextChange> {
        self.hunks_in_selection(doc, ranges)
            .into_iter()
            .map(|h| TextChange { from: h.from as u32, to: h.to as u32, insert: h.hunk.old_text.clone() })
            .collect()
    }
}

/// A press of a mouse button over an editor, in window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneClick {
    /// The primary button, with no modifier key held.
    pub plain_primary: bool,
    pub x: f64,
    pub y: f64,
}

/// Where the change lane is in the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneRect {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

/// The hunk whose popup a press on (or right next to) the change lane opens. `line_from` is the start
/// of the line drawn at the height of the press.
pub fn lane_mouse_down<'a>(
    state: &'a HunkState,
    doc: &Doc,
    folds: &[Range<usize>],
    click: &LaneClick,
    lane: Option<LaneRect>,
    line_from: usize,
) -> Option<&'a EditorHunk> {
    let lane = lane.filter(|_| click.plain_primary)?;
    if click.x < lane.left - 1.0 || click.x > lane.right + 3.0 || click.y < lane.top || click.y > lane.bottom {
        return None;
    }
    state.hunk_at_pos(doc, folds, line_from)
}

/// What to do when the timer of a [`HunkFetcher`] fires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchStep {
    Idle,
    /// Version control is not available: drop the hunks shown.
    Clear,
    /// Ask the backend for the file's hunks and hand its answer to [`HunkFetcher::accepts`] with
    /// this ticket.
    Request {
        ticket: u64,
    },
}

/// When an editor asks the backend for its hunks: shortly after it opens and after version control
/// changes, and once the buffer has been left alone for a moment. An answer computed for an older
/// buffer is dropped.
#[derive(Clone, Debug, Default)]
pub struct HunkFetcher {
    seq: u64,
    dirty_since: u64,
    destroyed: bool,
}

impl HunkFetcher {
    /// Delay of the first request, in milliseconds.
    pub const INITIAL_DELAY_MS: u64 = 30;
    /// Delay after the repository's state changed.
    pub const VCS_DELAY_MS: u64 = 60;
    /// Delay after an edit.
    pub const EDIT_DELAY_MS: u64 = 300;

    /// The buffer changed: an open hunk popup is stale and the timer restarts with the returned delay.
    pub fn doc_changed(&mut self) -> u64 {
        self.seq += 1;
        self.dirty_since = self.seq;
        Self::EDIT_DELAY_MS
    }

    /// The timer fired. `available`: version control is; `has_hunks`: the editor shows hunks.
    pub fn timer_fired(&mut self, available: bool, has_hunks: bool) -> FetchStep {
        if self.destroyed {
            return FetchStep::Idle;
        }
        if !available {
            return if has_hunks { FetchStep::Clear } else { FetchStep::Idle };
        }
        self.seq += 1;
        FetchStep::Request { ticket: self.seq }
    }

    /// Whether the answer to the request `ticket` is still for the buffer as it is.
    pub fn accepts(&self, ticket: u64) -> bool {
        !self.destroyed && self.dirty_since <= ticket
    }

    /// The editor closed: nothing more is requested or accepted.
    pub fn destroy(&mut self) {
        self.destroyed = true;
    }
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a single selection or fold range is the case under test")]
mod tests {
    use super::*;

    fn h(kind: HunkKind, start: u32, lines: u32, old_start: u32, old_lines: u32, old_text: &str) -> LineHunk {
        LineHunk { kind, start, lines, old_start, old_lines, old_text: old_text.to_owned() }
    }

    fn with_hunks(text: &str, hunks: &[LineHunk]) -> (Doc, HunkState) {
        let doc = Doc::new(text);
        let mut state = HunkState::default();
        state.set_hunks(hunks, &doc);
        (doc, state)
    }

    #[test]
    fn tracks_hunk_extents_through_edits_so_rollback_stays_exact() {
        // HEAD: "a\nb\nc\n"; the buffer modified line 2.
        let (doc, mut state) = with_hunks("a\nB\nc\n", &[h(HunkKind::Modified, 1, 1, 1, 1, "b\n")]);
        // Insert a line above the hunk.
        let doc = doc.replace(0, 0, "new\n");
        state.map_through(&[TextChange { from: 0, to: 0, insert: "new\n".to_owned() }], &doc);
        let hunk = &state.hunks()[0];
        assert_eq!(doc.slice_string(hunk.from, hunk.to), "B\n");
        let rolled = doc.replace(hunk.from, hunk.to, &hunk.hunk.old_text);
        assert_eq!(rolled.to_string(), "new\na\nb\nc\n");
    }

    #[test]
    fn locates_markers_by_line() {
        let (doc, state) =
            with_hunks("a\nc\nd\n", &[h(HunkKind::Deleted, 1, 0, 1, 1, "b\n"), h(HunkKind::Added, 2, 1, 3, 0, "")]);
        let kind_at = |line: usize| state.hunk_at_pos(&doc, &[], doc.line(line).from).map(|h| h.hunk.kind);
        assert_eq!(kind_at(2), Some(HunkKind::Deleted));
        assert_eq!(kind_at(3), Some(HunkKind::Added));
        assert_eq!(kind_at(1), None);
    }

    #[test]
    fn opens_the_hunk_popup_from_a_mousedown_on_the_lane() {
        let (doc, state) = with_hunks("a\nB\nc\n", &[h(HunkKind::Modified, 0, 1, 0, 1, "b\n")]);
        let lane = LaneRect { left: 0.0, right: 0.0, top: 0.0, bottom: 0.0 };
        let click = LaneClick { plain_primary: true, x: 0.0, y: 0.0 };
        let opened = lane_mouse_down(&state, &doc, &[], &click, Some(lane), 0);
        assert_eq!(opened.map(|h| h.hunk.kind), Some(HunkKind::Modified));
        // Not from a spec: other buttons, modified presses and presses away from the lane do nothing.
        let modified = LaneClick { plain_primary: false, ..click };
        assert!(lane_mouse_down(&state, &doc, &[], &modified, Some(lane), 0).is_none());
        let away = LaneClick { x: 4.0, ..click };
        assert!(lane_mouse_down(&state, &doc, &[], &away, Some(lane), 0).is_none());
        assert!(lane_mouse_down(&state, &doc, &[], &click, None, 0).is_none());
    }

    #[test]
    fn shows_hidden_hunks_on_the_folded_line_and_opens_them_from_there() {
        // Line 4 ("d", 0-based 3) lost a line below it.
        let (doc, state) = with_hunks("a\nb\nc\nd\ne\n", &[h(HunkKind::Deleted, 3, 0, 3, 1, "x\n")]);
        let at = |folds: &[Range<usize>]| -> Vec<usize> {
            state.visible_markers(&doc, folds).iter().map(|marker| marker.from).collect()
        };
        assert_eq!(at(&[]), [doc.line(4).from]);
        // Fold lines 2 to 4 onto line 1.
        let folds = [doc.line(1).to..doc.line(4).to];
        assert_eq!(at(&folds), [doc.line(1).from]);
        let hunk = state.hunk_at_pos(&doc, &folds, doc.line(1).from);
        assert_eq!(hunk.map(|h| h.hunk.kind), Some(HunkKind::Deleted));
    }

    #[test]
    fn takes_the_hunks_on_the_carets_line_or_touched_by_the_selection() {
        let hunks = [
            h(HunkKind::Modified, 1, 1, 1, 1, "b\n"),
            h(HunkKind::Deleted, 3, 0, 3, 1, "d\n"),
            h(HunkKind::Added, 4, 1, 5, 0, ""),
        ];
        let (doc, state) = with_hunks("a\nB\nc\ne\nX\n", &hunks);
        let at = |from: usize, to: usize| -> Vec<u32> {
            state.hunks_in_selection(&doc, &[from..to]).iter().map(|h| h.hunk.start).collect()
        };
        assert_eq!(at(0, 0), [0u32; 0]);
        assert_eq!(at(3, 3), [1]);
        // The deletion's marker is on the line after the removed block.
        assert_eq!(at(7, 7), [3]);
        assert_eq!(at(2, 9), [1, 3, 4]);
    }

    // Not from a spec: the rollback of a selection replaces each touched hunk by its original text.
    #[test]
    fn rolls_back_the_selected_lines() {
        let (doc, state) = with_hunks("a\nB\nc\n", &[h(HunkKind::Modified, 1, 1, 1, 1, "b\n")]);
        assert!(state.rollback_selected_lines(&doc, &[0..0]).is_empty());
        let edits = state.rollback_selected_lines(&doc, &[2..2]);
        assert_eq!(edits, [TextChange { from: 2, to: 4, insert: "b\n".to_owned() }]);
    }

    // Not from a spec: Next / Previous Change wrap around and skip the hunk the caret is in.
    #[test]
    fn moves_the_caret_between_changes() {
        let hunks = [h(HunkKind::Modified, 1, 1, 1, 1, "b\n"), h(HunkKind::Added, 4, 1, 5, 0, "")];
        let (doc, state) = with_hunks("a\nB\nc\ne\nX\n", &hunks);
        let go = |head: usize, direction: Direction| state.goto_change(&doc, head, direction).map(|(pos, _)| pos);
        assert_eq!(go(0, Direction::Next), Some(2));
        assert_eq!(go(2, Direction::Next), Some(8));
        assert_eq!(go(8, Direction::Next), Some(2));
        assert_eq!(go(8, Direction::Previous), Some(2));
        assert_eq!(go(0, Direction::Previous), Some(8));
        assert_eq!(HunkState::default().goto_change(&doc, 0, Direction::Next).map(|(pos, _)| pos), None);
    }

    // Not from a spec: an answer computed before an edit is dropped, and nothing is asked without
    // version control.
    #[test]
    fn drops_answers_for_an_older_buffer() {
        let mut fetcher = HunkFetcher::default();
        let FetchStep::Request { ticket } = fetcher.timer_fired(true, false) else { panic!("a request is due") };
        assert_eq!(fetcher.doc_changed(), HunkFetcher::EDIT_DELAY_MS);
        assert!(!fetcher.accepts(ticket));
        let FetchStep::Request { ticket } = fetcher.timer_fired(true, false) else { panic!("a request is due") };
        assert!(fetcher.accepts(ticket));
        assert_eq!(fetcher.timer_fired(false, true), FetchStep::Clear);
        assert_eq!(fetcher.timer_fired(false, false), FetchStep::Idle);
        fetcher.destroy();
        assert_eq!(fetcher.timer_fired(true, false), FetchStep::Idle);
        assert!(!fetcher.accepts(ticket));
    }
}
