//! Line hunks of a file against HEAD: the edit that rolls one back, the hunk under a line, moving between
//! hunks and their identity across edits.
//!
//! A hunk covers the current lines `[start, start + lines)` (0-based). A deletion has no lines and
//! starts at the line now following the removed block, which is the line count when the block was
//! removed at the end. `old_text` is the original text of the hunk with its line terminators. Offsets
//! are UTF-16 code units.

use super::doc::{base36, fnv1a_utf16, utf16_len};
use ide_core::TextChange;
use ide_vcs::LineHunk;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::ops::Range;

/// Offsets where each line starts, plus the length of the text as a last, sentinel line start.
pub fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    let mut offset = 0;
    for c in text.chars() {
        offset += c.len_utf16();
        if c == '\n' {
            starts.push(offset);
        }
    }
    if starts.last() != Some(&offset) {
        starts.push(offset);
    }
    starts
}

fn start_of_line(starts: &[usize], line: usize) -> usize {
    starts[line.min(starts.len() - 1)]
}

/// The text between two UTF-16 offsets that are line starts of `text`.
fn slice_utf16(text: &str, from: usize, to: usize) -> String {
    let units: Vec<u16> = text.encode_utf16().collect();
    let to = to.min(units.len());
    String::from_utf16_lossy(&units[from.min(to)..to])
}

/// The edit that restores the hunk's original text. Hunks are computed over lines with their
/// terminators, so a plain replacement restores the original exactly, a missing or present final
/// newline included.
pub fn rollback_edit(text: &str, hunk: &LineHunk) -> TextChange {
    let starts = line_starts(text);
    let from = start_of_line(&starts, hunk.start as usize);
    let to = if hunk.lines == 0 { from } else { start_of_line(&starts, (hunk.start + hunk.lines) as usize) };
    TextChange { from: from as u32, to: to as u32, insert: hunk.old_text.clone() }
}

/// `text` with the hunk rolled back.
pub fn apply_rollback(text: &str, hunk: &LineHunk) -> String {
    let edit = rollback_edit(text, hunk);
    let end = utf16_len(text);
    let (from, to) = (edit.from as usize, edit.to as usize);
    format!("{}{}{}", slice_utf16(text, 0, from), edit.insert, slice_utf16(text, to, end))
}

fn hunk_of<H: Borrow<LineHunk>>(hunk: &H) -> &LineHunk {
    Borrow::<LineHunk>::borrow(hunk)
}

/// The hunk whose marker is on `line`. A deletion marks the line following it, and the last line
/// when it is at the end of a text of `line_count` lines.
pub fn hunk_at_line<H: Borrow<LineHunk>>(hunks: &[H], line: u32, line_count: Option<u32>) -> Option<&H> {
    hunks.iter().find(|h| {
        let h = hunk_of(*h);
        if h.lines == 0 {
            let marker = line_count.map_or(h.start, |count| h.start.min(count.saturating_sub(1)));
            return marker == line;
        }
        line >= h.start && line < h.start + h.lines
    })
}

/// The next hunk starting after `line`, wrapping around to the first one.
pub fn next_hunk<H: Borrow<LineHunk>>(hunks: &[H], line: u32) -> Option<&H> {
    hunks.iter().find(|h| hunk_of(*h).start > line).or_else(|| hunks.first())
}

/// The previous hunk ending before `line`, wrapping around to the last one.
pub fn prev_hunk<H: Borrow<LineHunk>>(hunks: &[H], line: u32) -> Option<&H> {
    hunks
        .iter()
        .rev()
        .find(|h| {
            let h = hunk_of(*h);
            h.start + h.lines.max(1) - 1 < line
        })
        .or_else(|| hunks.last())
}

/// The current text of the hunk's lines.
pub fn current_hunk_text(text: &str, hunk: &LineHunk) -> String {
    if hunk.lines == 0 {
        return String::new();
    }
    let starts = line_starts(text);
    let from = start_of_line(&starts, hunk.start as usize);
    slice_utf16(text, from, start_of_line(&starts, (hunk.start + hunk.lines) as usize))
}

/// A chunk of a diff as 0-based line ranges: `new` in the current text, `old` in the original one,
/// or `None` when the old side is unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineChunk {
    pub old: Option<Range<u32>>,
    pub new: Range<u32>,
}

/// Whether two line ranges overlap; an empty range overlaps the range it lies in or touches.
fn overlaps(s1: u32, e1: u32, s2: u32, e2: u32) -> bool {
    match (s1 == e1, s2 == e2) {
        (true, true) => s1 == s2,
        (true, false) => s2 <= s1 && s1 <= e2,
        (false, true) => s1 <= s2 && s2 <= e1,
        (false, false) => s1 < e2 && s2 < e1,
    }
}

/// Indices of the hunks overlapping `chunk` on either side.
pub fn hunks_overlapping<H: Borrow<LineHunk>>(hunks: &[H], chunk: &LineChunk) -> Vec<usize> {
    hunks
        .iter()
        .enumerate()
        .filter(|(_, h)| {
            let h = hunk_of(*h);
            let new = overlaps(h.start, h.start + h.lines, chunk.new.start, chunk.new.end);
            let old = chunk
                .old
                .as_ref()
                .is_some_and(|old| overlaps(h.old_start, h.old_start + h.old_lines, old.start, old.end));
            old || new
        })
        .map(|(i, _)| i)
        .collect()
}

/// Identity of each changed block that survives edits elsewhere in the file: a hash of its old and
/// new text with their length, and how often the same block occurs before it.
pub fn hunk_keys<H: Borrow<LineHunk>>(text: &str, hunks: &[H]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    hunks
        .iter()
        .map(|h| {
            let h = hunk_of(h);
            let body = format!("{}\0{}", h.old_text, current_hunk_text(text, h));
            let base = format!("{}:{}", base36(fnv1a_utf16(body.encode_utf16())), utf16_len(&body));
            let count = seen.entry(base.clone()).or_insert(0);
            let key = format!("{base}:{count}");
            *count += 1;
            key
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_vcs::HunkKind;

    fn h(kind: HunkKind, start: u32, lines: u32, old_start: u32, old_lines: u32, old_text: &str) -> LineHunk {
        LineHunk { kind, start, lines, old_start, old_lines, old_text: old_text.to_owned() }
    }

    fn starts_of(hunk: Option<&LineHunk>) -> Option<u32> {
        hunk.map(|h| h.start)
    }

    #[test]
    fn restores_modified_lines() {
        let cur = "a\nB\nc\n";
        let hunk = h(HunkKind::Modified, 1, 1, 1, 1, "b\n");
        assert_eq!(rollback_edit(cur, &hunk), TextChange { from: 2, to: 4, insert: "b\n".to_owned() });
        assert_eq!(apply_rollback(cur, &hunk), "a\nb\nc\n");
    }

    #[test]
    fn removes_added_lines() {
        assert_eq!(apply_rollback("a\nb\nx\nc\n", &h(HunkKind::Added, 1, 2, 1, 0, "")), "a\nc\n");
    }

    #[test]
    fn re_inserts_deleted_lines_before_the_following_line() {
        assert_eq!(apply_rollback("a\nc\n", &h(HunkKind::Deleted, 1, 0, 1, 1, "b\n")), "a\nb\nc\n");
    }

    #[test]
    fn re_inserts_lines_deleted_at_the_end_of_the_file() {
        assert_eq!(apply_rollback("a\n", &h(HunkKind::Deleted, 1, 0, 1, 1, "b\n")), "a\nb\n");
        assert_eq!(apply_rollback("", &h(HunkKind::Deleted, 0, 0, 0, 1, "x\n")), "x\n");
    }

    #[test]
    fn keeps_the_original_final_newline_state() {
        // Last line without terminator, changed.
        assert_eq!(apply_rollback("a\nB", &h(HunkKind::Modified, 1, 1, 1, 1, "b")), "a\nb");
        // A trailing newline was added to the last line.
        assert_eq!(apply_rollback("a\nb\n", &h(HunkKind::Modified, 1, 1, 1, 1, "b")), "a\nb");
    }

    #[test]
    fn restores_multi_line_replacements_of_different_size() {
        let cur = "1\nX\nY\nZ\n5\n";
        assert_eq!(apply_rollback(cur, &h(HunkKind::Modified, 1, 3, 1, 2, "2\n3\n")), "1\n2\n3\n5\n");
    }

    fn navigation_hunks() -> Vec<LineHunk> {
        vec![
            h(HunkKind::Modified, 2, 2, 2, 2, ""),
            h(HunkKind::Deleted, 6, 0, 6, 1, ""),
            h(HunkKind::Added, 9, 1, 8, 0, ""),
        ]
    }

    #[test]
    fn finds_the_hunk_under_a_line_deletions_mark_the_following_line() {
        let hunks = navigation_hunks();
        assert_eq!(starts_of(hunk_at_line(&hunks, 3, None)), Some(2));
        assert_eq!(starts_of(hunk_at_line(&hunks, 6, None)), Some(6));
        assert!(hunk_at_line(&hunks, 5, None).is_none());
        // Deletion at the very end marks the last line.
        assert!(hunk_at_line(&[h(HunkKind::Deleted, 4, 0, 4, 1, "")], 3, Some(4)).is_some());
    }

    #[test]
    fn wraps_next_and_previous() {
        let hunks = navigation_hunks();
        assert_eq!(starts_of(next_hunk(&hunks, 2)), Some(6));
        assert_eq!(starts_of(next_hunk(&hunks, 9)), Some(2));
        assert_eq!(starts_of(prev_hunk(&hunks, 9)), Some(6));
        assert_eq!(starts_of(prev_hunk(&hunks, 2)), Some(9));
    }

    #[test]
    fn maps_a_diff_chunk_to_backend_hunk_indices() {
        let hunks = [
            h(HunkKind::Modified, 1, 1, 1, 1, "x\n"),
            h(HunkKind::Added, 4, 2, 4, 0, ""),
            h(HunkKind::Deleted, 8, 0, 6, 2, ""),
        ];
        let chunk = |old: Option<Range<u32>>, new: Range<u32>| LineChunk { old, new };
        assert_eq!(hunks_overlapping(&hunks, &chunk(Some(1..2), 1..2)), [0]);
        assert_eq!(hunks_overlapping(&hunks, &chunk(Some(4..4), 4..6)), [1]);
        assert_eq!(hunks_overlapping(&hunks, &chunk(Some(6..8), 8..8)), [2]);
        // Old side unknown (a diff against HEAD): match on the new side only.
        assert_eq!(hunks_overlapping(&hunks, &chunk(None, 4..5)), [1]);
        assert_eq!(hunks_overlapping(&hunks, &chunk(None, 2..3)), [0usize; 0]);
    }

    #[test]
    fn identifies_blocks_by_content_not_position() {
        let a = hunk_keys("x\nB\nc\n", &[h(HunkKind::Modified, 1, 1, 1, 1, "b\n")]);
        // The same block two lines further down (lines inserted above it) keeps its key.
        let b = hunk_keys(
            "new\nnew\nx\nB\nc\n",
            &[h(HunkKind::Added, 0, 2, 0, 0, ""), h(HunkKind::Modified, 3, 1, 1, 1, "b\n")],
        );
        assert_eq!(b[1], a[0]);
        assert_ne!(b[0], b[1]);
        // Identical blocks get distinct keys.
        let twice =
            hunk_keys("B\nB\n", &[h(HunkKind::Modified, 0, 1, 0, 1, "b\n"), h(HunkKind::Modified, 1, 1, 1, 1, "b\n")]);
        assert_ne!(twice[0], twice[1]);
    }

    // Not from a spec: offsets count UTF-16 code units, so a rollback after an astral character lands
    // on the line start.
    #[test]
    fn line_starts_are_utf16_offsets() {
        assert_eq!(line_starts("😀\nb"), [0, 3, 4]);
        assert_eq!(line_starts("a\n"), [0, 2]);
        assert_eq!(line_starts(""), [0]);
        assert_eq!(apply_rollback("😀\nB\n", &h(HunkKind::Modified, 1, 1, 1, 1, "b\n")), "😀\nb\n");
    }
}
