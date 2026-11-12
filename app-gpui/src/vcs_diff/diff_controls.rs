//! Per-change controls of the diff viewer: the ">>" and "<<" arrows in the divider that copy a change
//! toward an editable side, and the Revert button shown while the pointer is over a change of an
//! editable pane.

use super::diff_options::{Chunk, Side, transfer_chunk};
use super::doc::Doc;
use ide_core::TextChange;

/// The edit that copies chunk `index` from `source` to the other side, with the side it is applied
/// to (as one undoable user edit).
pub fn apply_chunk(chunks: &[Chunk], index: usize, source: Side, a: &Doc, b: &Doc) -> Option<(Side, TextChange)> {
    let chunk = chunks.get(index)?;
    let (src, dst) = match source {
        Side::A => (a, b),
        Side::B => (b, a),
    };
    Some((source.other(), transfer_chunk(chunk, source, src, dst)))
}

/// The edit reverting the change at `pos` of a unified view to the `original` text, with the same
/// handling of line boundaries as the side-by-side viewer.
pub fn revert_unified_chunk(chunks: &[Chunk], pos: usize, original: &Doc, doc: &Doc) -> Option<TextChange> {
    let chunk = chunks.iter().find(|c| c.from_b <= pos && c.end(Side::B) >= pos)?;
    Some(transfer_chunk(chunk, Side::A, original, doc))
}

/// An arrow of the divider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArrowDir {
    /// ">>": the left side's text replaces the change on the right.
    AToB,
    /// "<<": the right side's text replaces the change on the left.
    BToA,
}

impl ArrowDir {
    /// English label and tooltip of the arrow.
    pub fn label(self) -> &'static str {
        match self {
            Self::AToB => "Apply change to the right (Replace ›››)",
            Self::BToA => "Apply change to the left (‹‹‹ Replace)",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::AToB => "chevron-right",
            Self::BToA => "chevron-left",
        }
    }

    /// The side the arrow copies from.
    pub fn source(self) -> Side {
        match self {
            Self::AToB => Side::A,
            Self::BToA => Side::B,
        }
    }
}

/// The sides a diff lets the arrows write to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArrowSides {
    /// The left side is editable ("<<").
    pub to_a: bool,
    /// The right side is editable (">>").
    pub to_b: bool,
}

/// The arrows shown beside each chunk, in the order they are laid out.
pub fn arrow_controls(sides: ArrowSides) -> Vec<ArrowDir> {
    let arrows = [(sides.to_b, ArrowDir::AToB), (sides.to_a, ArrowDir::BToA)];
    arrows.into_iter().filter(|(shown, _)| *shown).map(|(_, arrow)| arrow).collect()
}

/// Index of the chunk of pane `side` holding `pos`; an empty side holds the point it sits on.
pub fn chunk_at(chunks: &[Chunk], side: Side, pos: usize, doc_length: usize) -> Option<usize> {
    chunks.iter().position(|c| {
        let (from, to) = (c.from(side), c.to(side));
        if from == to { pos == from } else { pos >= from && pos < to.max(from + 1) && pos <= doc_length }
    })
}

/// English label of the hover button.
pub const REVERT_LABEL: &str = "Revert this change";
/// English text of the hover button.
pub const REVERT_TEXT: &str = "Revert";

/// Where the Revert button of the hovered change is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverRevertMark {
    /// The chunk the button reverts.
    pub index: usize,
    /// Start of the change's first line, which is highlighted.
    pub line_from: usize,
    /// End of that line, where the button sits.
    pub button_at: usize,
}

/// The change under the pointer in an editable pane of a side-by-side diff.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoverRevert {
    index: Option<usize>,
}

impl HoverRevert {
    pub fn index(&self) -> Option<usize> {
        self.index
    }

    /// The pointer moved over the line starting at `line_from`. Returns whether the hovered change
    /// is another one now.
    pub fn pointer_moved(&mut self, chunks: &[Chunk], side: Side, line_from: usize, doc_length: usize) -> bool {
        self.set(chunk_at(chunks, side, line_from, doc_length))
    }

    /// The pointer left the pane. Returns whether a change was hovered.
    pub fn pointer_left(&mut self) -> bool {
        self.set(None)
    }

    /// The chunks were recomputed: a change that is gone is hovered no more.
    pub fn chunks_changed(&mut self, chunk_count: usize) {
        self.index = self.index.filter(|index| *index < chunk_count);
    }

    fn set(&mut self, index: Option<usize>) -> bool {
        let changed = self.index != index;
        self.index = index;
        changed
    }

    /// What to draw for the hovered change in pane `side`.
    pub fn mark(&self, chunks: &[Chunk], side: Side, doc: &Doc) -> Option<HoverRevertMark> {
        let index = self.index?;
        let line = doc.line_at(chunks.get(index)?.from(side).min(doc.len()));
        Some(HoverRevertMark { index, line_from: line.from, button_at: line.to })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_options::{HighlightPolicy, IgnorePolicy, idea_chunks};

    fn chunks(a: &Doc, b: &Doc) -> Vec<Chunk> {
        idea_chunks(a, b, IgnorePolicy::None, HighlightPolicy::Words).chunks
    }

    #[test]
    fn applies_chunks_in_a_merge_view_through_the_arrow_controls() {
        let mut a = Doc::new("1\nleft\n3\n");
        let mut b = Doc::new("1\nright\n3\n");
        let arrows = arrow_controls(ArrowSides { to_a: true, to_b: true });
        assert_eq!(arrows, [ArrowDir::AToB, ArrowDir::BToA]);
        assert!(arrows[0].label().contains("right") && arrows[1].label().contains("left"));
        // "<<": the right side's text replaces the left chunk.
        let (target, edit) = apply_chunk(&chunks(&a, &b), 0, arrows[1].source(), &a, &b).expect("the chunk exists");
        assert_eq!(target, Side::A);
        a = a.apply(&edit);
        assert_eq!(a.to_string(), "1\nright\n3\n");
        assert!(chunks(&a, &b).is_empty());
        a = a.replace(2, 7, "LEFT");
        let (target, edit) = apply_chunk(&chunks(&a, &b), 0, Side::A, &a, &b).expect("the chunk exists");
        assert_eq!(target, Side::B);
        b = b.apply(&edit);
        assert_eq!(b.to_string(), "1\nLEFT\n3\n");
        assert!(apply_chunk(&chunks(&a, &b), 0, Side::A, &a, &b).is_none());
    }

    // Not from a spec: only the editable sides get an arrow.
    #[test]
    fn shows_arrows_toward_editable_sides_only() {
        assert_eq!(arrow_controls(ArrowSides { to_a: false, to_b: true }), [ArrowDir::AToB]);
        assert!(arrow_controls(ArrowSides { to_a: false, to_b: false }).is_empty());
    }

    // Not from a spec: the hovered change follows the pointer's line, an empty side included, and the
    // unified view reverts the change at the caret.
    #[test]
    fn tracks_the_hovered_change_and_reverts_it() {
        let a = Doc::new("keep\nold\nend\n");
        let b = Doc::new("keep\nnew\nend\nadded\n");
        let found = chunks(&a, &b);
        assert_eq!(found.len(), 2);
        assert_eq!(chunk_at(&found, Side::B, b.line(2).from, b.len()), Some(0));
        assert_eq!(chunk_at(&found, Side::B, b.line(1).from, b.len()), None);
        // The added line has no lines on the left: the point where they would be.
        assert_eq!(chunk_at(&found, Side::A, found[1].from_a, a.len()), Some(1));
        let mut hover = HoverRevert::default();
        assert!(hover.pointer_moved(&found, Side::B, b.line(2).from, b.len()));
        assert!(!hover.pointer_moved(&found, Side::B, b.line(2).from, b.len()));
        let mark = hover.mark(&found, Side::B, &b).expect("a change is hovered");
        assert_eq!((mark.index, mark.line_from, mark.button_at), (0, 5, 8));
        hover.chunks_changed(0);
        assert_eq!(hover.index(), None);
        assert!(!hover.pointer_left());
        let revert = revert_unified_chunk(&found, 6, &a, &b).expect("the caret is in a change");
        assert_eq!(b.apply(&revert).to_string(), "keep\nold\nend\nadded\n");
        assert!(revert_unified_chunk(&found, 0, &a, &b).is_none());
    }
}
