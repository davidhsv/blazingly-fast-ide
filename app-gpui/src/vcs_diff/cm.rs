//! What the panes of the version control viewers (diff, merge, local history) draw for the chunks of
//! a comparison: line backgrounds by the type of their change, the changed words inside them, the
//! tint carried into the gutters and the marks on the overview lane.
//!
//! Offsets are UTF-16 code units; a viewport is the range of offsets in view.

use super::diff_options::{Chunk, DiffChunks, Side};
use super::doc::{Doc, Line};
use super::marks::{MarkKind, ScrollbarMark};
use std::ops::Range;

/// The type of a change: both sides have text, only the new side, only the old side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffKind {
    Modified,
    Inserted,
    Deleted,
}

impl DiffKind {
    pub const ALL: [DiffKind; 3] = [DiffKind::Modified, DiffKind::Inserted, DiffKind::Deleted];

    fn of(has_a: bool, has_b: bool) -> Self {
        match (has_a, has_b) {
            (true, true) => Self::Modified,
            (true, false) => Self::Deleted,
            _ => Self::Inserted,
        }
    }

    /// Name of the theme token holding the background of changes of this type.
    pub fn color_token(self) -> &'static str {
        match self {
            Self::Modified => "diff-modified",
            Self::Inserted => "diff-inserted",
            Self::Deleted => "diff-deleted",
        }
    }

    /// Name of the theme token holding the colour of this type on the overview lane.
    pub fn stripe_token(self) -> &'static str {
        match self {
            Self::Modified => "diff-modified-stripe",
            Self::Inserted => "diff-inserted-stripe",
            Self::Deleted => "diff-deleted-stripe",
        }
    }
}

/// How much of a type's colour the backgrounds take, in percent. Text backgrounds stay translucent
/// so the selection, painted below the text, shows through; gutters have no selection and keep the
/// full colour.
pub const LINE_TINT: u8 = 70;
/// A changed word.
pub const WORD_TINT: u8 = 85;
/// A line of a chunk whose words were compared: lighter, its words in the full word tint.
pub const FINE_LINE_TINT: u8 = 28;
/// The gutter of such a line, mixed into the editor's background.
pub const FINE_GUTTER_TINT: u8 = 40;

/// A chunk's type, by the sides it has lines on.
pub fn chunk_kind(chunk: &Chunk) -> DiffKind {
    DiffKind::of(chunk.has_lines(Side::A), chunk.has_lines(Side::B))
}

/// A changed line of a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangedLine {
    pub line: Line,
    pub kind: DiffKind,
    /// The words of the line's chunk were compared: it takes the lighter background.
    pub fine: bool,
}

/// The changed lines of pane `side` within `viewport`, each with the type of its chunk: a chunk with
/// lines on both sides is modified as a whole, however its words changed.
pub fn changed_lines(chunks: &[Chunk], side: Side, doc: &Doc, viewport: Range<usize>) -> Vec<ChangedLine> {
    let mut out = Vec::new();
    for chunk in chunks {
        let (from, to) = (chunk.from(side), chunk.to(side));
        if from > viewport.end {
            break;
        }
        if from == to || to < viewport.start {
            continue;
        }
        let (kind, fine) = (chunk_kind(chunk), chunk.has_inner_fragments());
        let mut line = doc.line_at(from.max(viewport.start));
        // A line starting at the end of the viewport is still in view (the empty last line).
        while line.from < to && line.from <= viewport.end {
            out.push(ChangedLine { line, kind, fine });
            if line.number == doc.lines() {
                break;
            }
            line = doc.line(line.number + 1);
        }
    }
    out
}

/// The line backgrounds of a pane within `viewport`. The unified view shows every new line as
/// inserted, below the old ones as deleted; pass it the new side.
pub fn line_marks(chunks: &[Chunk], side: Side, doc: &Doc, viewport: Range<usize>, unified: bool) -> Vec<ChangedLine> {
    let mut lines = changed_lines(chunks, side, doc, viewport);
    if unified {
        for line in &mut lines {
            line.kind = DiffKind::Inserted;
        }
    }
    lines
}

/// The lines whose background is carried into the gutters (line numbers, inclusion boxes).
pub fn changed_gutter_lines(chunks: &[Chunk], side: Side, doc: &Doc) -> Vec<ChangedLine> {
    changed_lines(chunks, side, doc, 0..doc.len())
}

/// A changed word range of a pane. An empty range is a word the other side has and this one lacks:
/// a thin bar where it would be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WordMark {
    pub from: usize,
    pub to: usize,
    pub kind: DiffKind,
}

impl WordMark {
    pub fn is_empty(&self) -> bool {
        self.from == self.to
    }
}

/// "Highlight words" for a side-by-side pane: the inner fragments of the chunks whose words were
/// compared, each in the colour of its own type. Moved blocks are left out, having their own
/// comparison against where they went.
pub fn word_marks(diff: &DiffChunks, side: Side, viewport: Range<usize>) -> Vec<WordMark> {
    let mut moved: Vec<Range<usize>> = diff
        .moves
        .iter()
        .map(|m| match side {
            Side::A => m.from_a..m.to_a,
            Side::B => m.from_b..m.to_b,
        })
        .collect();
    moved.sort_by_key(|range| range.start);
    let mut move_index = 0;
    let mut out = Vec::new();
    for chunk in &diff.chunks {
        let base = chunk.from(side);
        if base > viewport.end {
            break;
        }
        if chunk.to(side) < viewport.start || !chunk.has_inner_fragments() {
            continue;
        }
        for c in &chunk.changes {
            let (from, to) = match side {
                Side::A => (c.from_a, c.to_a),
                Side::B => (c.from_b, c.to_b),
            };
            let kind = DiffKind::of(c.from_a != c.to_a, c.from_b != c.to_b);
            let mut cursor = base + from;
            let end = base + to;
            while moved.get(move_index).is_some_and(|range| range.end <= cursor) {
                move_index += 1;
            }
            if from == to {
                if moved.get(move_index).is_none_or(|range| range.start > cursor) {
                    out.push(WordMark { from: cursor, to: cursor, kind });
                }
                continue;
            }
            for range in moved[move_index..].iter().take_while(|range| range.start < end) {
                if range.start > cursor {
                    out.push(WordMark { from: cursor, to: range.start.min(end), kind });
                }
                cursor = cursor.max(range.end);
            }
            if cursor < end {
                out.push(WordMark { from: cursor, to: end, kind });
            }
        }
    }
    out
}

/// The diff viewer's layouts, as the overview lane sees them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffLayout {
    /// Side by side, each pane scrolling on its own.
    Split,
    /// Side by side, padded into alignment and scrolling together.
    Aligned,
    Unified,
}

/// The changes on a pane's overview lane, as marks across its width. A side-by-side pane (`side`
/// is its side) marks each chunk's lines in the chunk's type, an empty side a point where the lines
/// would be. The unified view has no modified type: the new lines are marked inserted and the old
/// ones, which it shows above them, deleted, as a point on top of the inserted lines.
pub fn diff_scrollbar_marks(chunks: &[Chunk], side: Side, unified: bool) -> Vec<ScrollbarMark> {
    let mark = |from: usize, to: Option<usize>, kind: DiffKind, priority: u32| ScrollbarMark {
        from,
        to,
        color: kind.stripe_token(),
        kind: MarkKind::Full,
        tooltip: None,
        priority,
    };
    if !unified {
        return chunks.iter().map(|c| mark(c.from(side), Some(c.end(side)), chunk_kind(c), 0)).collect();
    }
    let mut marks = Vec::new();
    for c in chunks {
        if c.has_lines(Side::B) {
            marks.push(mark(c.from_b, Some(c.end(Side::B)), DiffKind::Inserted, 0));
        }
        if c.has_lines(Side::A) {
            marks.push(mark(c.from_b, None, DiffKind::Deleted, 1));
        }
    }
    marks
}

/// Which scrollbar a pane's overview lane lies over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverviewLane {
    /// The pane's own scrollbar.
    Pane,
    /// The scrollbar the panes of an aligned side-by-side diff share; the right pane's lane takes it.
    SharedScroller,
}

/// The overview lane of a pane of the diff viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffOverview {
    /// The marks are those of the unified view ([`diff_scrollbar_marks`]).
    pub unified: bool,
    pub lane: OverviewLane,
}

pub fn diff_overview(layout: DiffLayout) -> DiffOverview {
    DiffOverview {
        unified: layout == DiffLayout::Unified,
        lane: if layout == DiffLayout::Aligned { OverviewLane::SharedScroller } else { OverviewLane::Pane },
    }
}

/// How a viewer pane is set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewerOptions {
    pub read_only: bool,
    pub line_numbers: bool,
    /// The viewer shows the editor's find bar instead of a search panel of its own.
    pub find_bar: bool,
}

impl ViewerOptions {
    pub fn new(read_only: bool) -> Self {
        Self { read_only, line_numbers: true, find_bar: false }
    }
}

/// What a viewer pane lets the user do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewerBase {
    pub read_only: bool,
    /// The text takes input.
    pub editable: bool,
    /// The pane is a tab stop of its own: a pane that takes no input still needs the keyboard for
    /// selection and copy. An editable pane is focusable as any editor.
    pub tab_stop: bool,
    pub line_numbers: bool,
    pub find_bar: bool,
}

pub fn viewer_base(options: ViewerOptions) -> ViewerBase {
    ViewerBase {
        read_only: options.read_only,
        editable: !options.read_only,
        tab_stop: options.read_only,
        line_numbers: options.line_numbers,
        find_bar: options.find_bar,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_options::{HighlightPolicy, IgnorePolicy, idea_chunks};

    struct Panes {
        a: Doc,
        b: Doc,
        diff: DiffChunks,
    }

    fn panes(a: &str, b: &str) -> Panes {
        let (a, b) = (Doc::new(a), Doc::new(b));
        let diff = idea_chunks(&a, &b, IgnorePolicy::None, HighlightPolicy::Words);
        Panes { a, b, diff }
    }

    impl Panes {
        fn doc(&self, side: Side) -> &Doc {
            match side {
                Side::A => &self.a,
                Side::B => &self.b,
            }
        }

        fn changed(&self, side: Side) -> Vec<ChangedLine> {
            let doc = self.doc(side);
            line_marks(&self.diff.chunks, side, doc, 0..doc.len(), false)
        }

        /// Text of the lines of `side` with the background of `kind`.
        fn lines(&self, side: Side, kind: DiffKind) -> Vec<String> {
            let doc = self.doc(side);
            self.changed(side).iter().filter(|l| l.kind == kind).map(|l| doc.line_text(l.line)).collect()
        }

        /// Text of the words of `side` highlighted as `kind`, split at line breaks as they are drawn.
        fn words(&self, side: Side, kind: DiffKind) -> Vec<String> {
            let doc = self.doc(side);
            word_marks(&self.diff, side, 0..doc.len())
                .iter()
                .filter(|mark| mark.kind == kind && !mark.is_empty())
                .flat_map(|mark| {
                    let text = doc.slice_string(mark.from, mark.to);
                    text.split('\n').filter(|part| !part.is_empty()).map(str::to_owned).collect::<Vec<_>>()
                })
                .collect()
        }

        /// 1-based numbers of the lines of `side` whose gutter is tinted as `kind`.
        fn gutter_numbers(&self, side: Side, kind: DiffKind) -> Vec<usize> {
            changed_gutter_lines(&self.diff.chunks, side, self.doc(side))
                .iter()
                .filter(|l| l.kind == kind)
                .map(|l| l.line.number)
                .collect()
        }
    }

    const NONE: [&str; 0] = [];

    #[test]
    fn colours_lines_by_their_chunks_type() {
        let p =
            panes("keep\nremoved one\nremoved two\nmid\nvalue = old\nend\n", "keep\nmid\nvalue = new\nadded\nend\n");
        assert_eq!(p.lines(Side::A, DiffKind::Deleted), ["removed one", "removed two"]);
        // A block with lines on both sides is modified as a whole, on both sides.
        assert_eq!(p.lines(Side::A, DiffKind::Modified), ["value = old"]);
        assert_eq!(p.lines(Side::B, DiffKind::Modified), ["value = new", "added"]);
        assert_eq!(p.lines(Side::B, DiffKind::Inserted), NONE);
        assert_eq!(chunk_kind(&p.diff.chunks[1]), DiffKind::Modified);
    }

    #[test]
    fn highlights_the_words_of_a_modified_block_by_their_own_type() {
        let p = panes("x\n  syncScroll: boolean;\ny\n", "x\n  /** Docs. */\n  syncScrolling: boolean;\ny\n");
        // The edited identifier is highlighted whole, joined by the comment added before it.
        assert_eq!(p.words(Side::A, DiffKind::Modified), ["syncScroll"]);
        assert_eq!(p.words(Side::B, DiffKind::Modified).concat(), "/** Docs. */  syncScrolling");
    }

    #[test]
    fn marks_inserted_and_deleted_words_inside_a_modified_line() {
        let p = panes("call(first, extra, last)\n", "call(first, last, more)\n");
        assert!(p.words(Side::A, DiffKind::Deleted).concat().contains("extra"));
        assert!(p.words(Side::B, DiffKind::Inserted).concat().contains("more"));
        assert_eq!(p.words(Side::A, DiffKind::Inserted), NONE);
        assert_eq!(p.words(Side::B, DiffKind::Deleted), NONE);
    }

    #[test]
    fn keeps_a_line_with_only_words_added_modified_on_both_sides() {
        let p = panes("f() {\n  return a;\n}\n", "f() {\n  return a + b;\n}\n");
        assert_eq!(p.lines(Side::A, DiffKind::Modified), ["  return a;"]);
        assert_eq!(p.lines(Side::B, DiffKind::Modified), ["  return a + b;"]);
        // The space before an inserted word stays in the fragment.
        assert_eq!(p.words(Side::B, DiffKind::Inserted).concat(), " + b");
        // and a thin bar marks where it would be on the other side
        let bars: Vec<WordMark> = word_marks(&p.diff, Side::A, 0..p.a.len())
            .into_iter()
            .filter(|mark| mark.is_empty() && mark.kind == DiffKind::Inserted)
            .collect();
        assert_eq!(bars.len(), 1);
    }

    #[test]
    fn carries_the_change_background_into_the_line_number_gutter() {
        let p = panes("keep\nlet x = 1;\nend\n", "keep\nlet x = 2;\nadded\nend\nnew\n");
        assert_eq!(p.gutter_numbers(Side::A, DiffKind::Modified), [2]);
        assert_eq!(p.gutter_numbers(Side::B, DiffKind::Modified), [2, 3]);
        assert_eq!(p.gutter_numbers(Side::B, DiffKind::Inserted), [5]);
    }

    #[test]
    fn shows_a_final_line_break_added_as_an_inserted_empty_line_gutter_included() {
        let p = panes("alpha\nbeta", "alpha\nbeta\n");
        let flags = |lines: Vec<ChangedLine>| -> Vec<bool> {
            (1..=p.b.lines()).map(|number| lines.iter().any(|l| l.line.number == number)).collect()
        };
        // Lines are compared without their breaks: "beta" is unchanged, the empty last line inserted.
        let lines = flags(p.changed(Side::B));
        assert_eq!(lines, [false, false, true]);
        assert_eq!(flags(changed_gutter_lines(&p.diff.chunks, Side::B, &p.b)), lines);
        assert_eq!(p.lines(Side::B, DiffKind::Inserted).len(), 1);
    }

    #[test]
    fn shows_a_last_line_removed_as_a_deleted_line_only() {
        let p = panes("keep\ngone", "keep");
        // "keep" loses its line break, which does not count as a change.
        let changed: Vec<String> = p.changed(Side::A).iter().map(|l| p.a.line_text(l.line)).collect();
        assert_eq!(changed, ["gone"]);
        assert_eq!(p.lines(Side::A, DiffKind::Deleted).len(), 1);
    }

    #[test]
    fn keeps_word_highlighting_for_a_single_line_replacement() {
        let p = panes("before\nvalue = old\nafter\n", "before\nvalue = new\nafter\n");
        assert_eq!(p.words(Side::A, DiffKind::Modified), ["old"]);
        assert_eq!(p.words(Side::B, DiffKind::Modified), ["new"]);
    }

    #[test]
    fn gives_a_read_only_side_a_tab_stop_and_leaves_an_editable_side_alone() {
        let read_only = viewer_base(ViewerOptions::new(true));
        assert!(!read_only.editable && read_only.tab_stop);
        let editable = viewer_base(ViewerOptions::new(false));
        assert!(editable.editable && !editable.tab_stop);
    }

    // One modified line, one inserted line, one deleted line.
    const A: &str = "one\ntwo\nthree\nfour\nfive\nsix";
    const B: &str = "one\nTWO\nthree\nnew\nfour\nsix";

    /// Offset of the start of the 1-based line `n` of `text`.
    fn line(text: &str, n: usize) -> usize {
        Doc::new(text).line(n).from
    }

    fn colors(marks: &[ScrollbarMark]) -> Vec<(usize, &'static str)> {
        marks.iter().map(|mark| (mark.from, mark.color)).collect()
    }

    #[test]
    fn marks_every_chunk_of_a_side_by_side_pane_in_the_chunks_type() {
        let p = panes(A, B);
        // The insertion has no lines on the left: a point where they would be; likewise the deletion
        // on the right.
        for (side, text, lines) in [(Side::A, A, [2, 4, 5]), (Side::B, B, [2, 4, 6])] {
            let marks = diff_scrollbar_marks(&p.diff.chunks, side, false);
            let expected = [
                (line(text, lines[0]), "diff-modified-stripe"),
                (line(text, lines[1]), "diff-inserted-stripe"),
                (line(text, lines[2]), "diff-deleted-stripe"),
            ];
            assert_eq!(colors(&marks), expected);
            assert!(marks.iter().all(|mark| mark.kind == MarkKind::Full));
        }
    }

    #[test]
    fn marks_the_unified_views_old_lines_deleted_and_new_lines_inserted_with_no_modified_type() {
        let p = panes(A, B);
        let marks = diff_scrollbar_marks(&p.diff.chunks, Side::B, true);
        let expected = [
            (line(B, 2), "diff-inserted-stripe"),
            (line(B, 2), "diff-deleted-stripe"),
            (line(B, 4), "diff-inserted-stripe"),
            (line(B, 6), "diff-deleted-stripe"),
        ];
        assert_eq!(colors(&marks), expected);
        // The old lines' point sits on top of the new lines that replaced them.
        assert!(marks[1].priority > marks[0].priority);
    }

    #[test]
    fn follows_the_chunks_when_the_other_side_changes_without_a_document_change_of_its_own() {
        // The left side takes the right side's text: the right pane has nothing left to mark.
        let p = panes(B, B);
        assert!(diff_scrollbar_marks(&p.diff.chunks, Side::B, false).is_empty());
    }

    #[test]
    fn draws_the_aligned_views_lane_in_the_right_pane_over_the_shared_scroller() {
        let aligned = diff_overview(DiffLayout::Aligned);
        assert_eq!(aligned, DiffOverview { unified: false, lane: OverviewLane::SharedScroller });
        assert_eq!(diff_overview(DiffLayout::Split).lane, OverviewLane::Pane);
        let unified = diff_overview(DiffLayout::Unified);
        assert_eq!(unified, DiffOverview { unified: true, lane: OverviewLane::Pane });
    }
}
