//! Coordinates of language-server answers in a document.
//!
//! The backend speaks 0-based lines and UTF-16 columns; the editor's offsets are UTF-16 code units
//! from the start of the document, so a column is an index into the line's text.

use crate::editor::buffer;
use ide_core::{Pos, Range, TextChange, TextEdit};
use ropey::Rope;
use std::cmp::Ordering;

/// The offset of `pos`, clamped to its line and to the document.
pub fn pos_to_offset(doc: &Rope, pos: Pos) -> u32 {
    if pos.line >= buffer::line_count(doc) {
        return buffer::len_utf16(doc);
    }
    let line = buffer::line_range(doc, pos.line);
    line.start + pos.col.min(line.end - line.start)
}

/// The position of `offset`, clamped to the document.
pub fn offset_to_pos(doc: &Rope, offset: u32) -> Pos {
    let (line, col) = buffer::line_col(doc, offset.min(buffer::len_utf16(doc)));
    Pos::new(line, col)
}

/// The offsets of `range`, in order whichever end comes first.
pub fn range_to_offsets(doc: &Rope, range: Range) -> std::ops::Range<u32> {
    let a = pos_to_offset(doc, range.start);
    let b = pos_to_offset(doc, range.end);
    a.min(b)..a.max(b)
}

pub fn offsets_to_range(doc: &Rope, from: u32, to: u32) -> Range {
    Range::new(offset_to_pos(doc, from), offset_to_pos(doc, to))
}

pub fn compare_pos(a: Pos, b: Pos) -> Ordering {
    a.line.cmp(&b.line).then(a.col.cmp(&b.col))
}

/// Whether `pos` lies inside `range`, its end included (a caret at the end of a word is in it).
pub fn range_contains(range: Range, pos: Pos) -> bool {
    compare_pos(range.start, pos) != Ordering::Greater && compare_pos(pos, range.end) != Ordering::Greater
}

/// Text edits of one document version as sorted, non-overlapping changes. An edit overlapping an
/// earlier one is dropped rather than producing a corrupt change list, and so is one that
/// replaces text with itself.
pub fn text_edits_to_changes(doc: &Rope, edits: &[TextEdit]) -> Vec<TextChange> {
    let mut changes: Vec<(std::ops::Range<u32>, usize)> =
        edits.iter().enumerate().map(|(i, edit)| (range_to_offsets(doc, edit.range), i)).collect();
    changes.sort_by(|(a, i), (b, j)| a.start.cmp(&b.start).then(a.end.cmp(&b.end)).then(i.cmp(j)));
    let mut out = Vec::new();
    let mut end: Option<u32> = None;
    for (range, i) in changes {
        if end.is_some_and(|end| range.start < end) {
            continue;
        }
        let insert = &edits[i].new_text;
        if buffer::slice(doc, range.start, range.end) == *insert {
            continue;
        }
        out.push(TextChange { from: range.start, to: range.end, insert: insert.clone() });
        end = Some(range.end);
    }
    out
}

/// `doc` after `changes` (sorted, non-overlapping, in the coordinates of `doc`).
pub fn apply_changes(doc: &Rope, changes: &[TextChange]) -> Rope {
    let mut out = doc.clone();
    for change in changes.iter().rev() {
        let start = out.utf16_cu_to_char((change.from as usize).min(out.len_utf16_cu()));
        let end = out.utf16_cu_to_char((change.to as usize).min(out.len_utf16_cu()));
        out.remove(start..end.max(start));
        out.insert(start, &change.insert);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Rope {
        Rope::from_str("let a = 1;\n// 😀 émoji\n\nend")
    }

    fn edit(line: u32, from: u32, to: u32, new_text: &str) -> TextEdit {
        TextEdit { range: Range::new(Pos::new(line, from), Pos::new(line, to)), new_text: new_text.to_owned() }
    }

    #[test]
    fn maps_0_based_line_and_utf_16_column_to_offsets_and_back() {
        let doc = doc();
        assert_eq!(pos_to_offset(&doc, Pos::new(0, 4)), 4);
        // Line 1 starts at 11; the emoji is two UTF-16 units.
        assert_eq!(pos_to_offset(&doc, Pos::new(1, 3)), 14);
        assert_eq!(pos_to_offset(&doc, Pos::new(1, 6)), 17);
        assert_eq!(offset_to_pos(&doc, 17), Pos::new(1, 6));
        assert_eq!(offset_to_pos(&doc, buffer::len_utf16(&doc)), Pos::new(3, 3));
    }

    #[test]
    fn clamps_out_of_range_positions() {
        let doc = doc();
        assert_eq!(pos_to_offset(&doc, Pos::new(0, 99)), 10);
        assert_eq!(pos_to_offset(&doc, Pos::new(99, 0)), buffer::len_utf16(&doc));
        assert_eq!(offset_to_pos(&doc, u32::MAX), Pos::new(3, 3));
        assert_eq!(offset_to_pos(&doc, 0), Pos::new(0, 0));
    }

    #[test]
    fn converts_ranges_in_both_directions_and_normalises_order() {
        let doc = doc();
        let range = offsets_to_range(&doc, 4, 14);
        assert_eq!(range, Range::new(Pos::new(0, 4), Pos::new(1, 3)));
        assert_eq!(range_to_offsets(&doc, Range::new(range.end, range.start)), 4..14);
    }

    #[test]
    fn checks_containment_with_inclusive_ends() {
        let range = Range::new(Pos::new(1, 2), Pos::new(1, 5));
        assert!(range_contains(range, Pos::new(1, 5)));
        assert!(!range_contains(range, Pos::new(1, 6)));
        assert!(!range_contains(range, Pos::new(0, 9)));
    }

    #[test]
    fn turns_text_edits_into_sorted_non_overlapping_changes() {
        let doc = doc();
        let len = buffer::len_utf16(&doc);
        let changes = text_edits_to_changes(
            &doc,
            &[edit(3, 0, 3, "END"), edit(0, 0, 3, "const"), edit(0, 1, 2, "overlap"), edit(0, 4, 5, "a")],
        );
        assert_eq!(
            changes,
            [
                TextChange { from: 0, to: 3, insert: "const".into() },
                TextChange { from: len - 3, to: len, insert: "END".into() },
            ]
        );
        assert_eq!(apply_changes(&doc, &changes).to_string(), "const a = 1;\n// 😀 émoji\n\nEND");
    }
}
