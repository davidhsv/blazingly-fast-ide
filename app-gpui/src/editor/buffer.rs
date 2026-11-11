//! Coordinates in a document's rope.
//!
//! Everything the backend and the platform's text input speak is UTF-16: offsets from the start
//! of the document, and columns within a line. Shaped lines are indexed in UTF-8 bytes. These
//! helpers convert between them and map positions through edits.

use ide_core::TextChange;
use ropey::Rope;

/// Length of the document in UTF-16 code units.
pub fn len_utf16(rope: &Rope) -> u32 {
    rope.len_utf16_cu() as u32
}

fn char_of(rope: &Rope, offset: u32) -> usize {
    rope.utf16_cu_to_char((offset as usize).min(rope.len_utf16_cu()))
}

/// Number of lines; a document always has one, and a trailing newline starts another.
pub fn line_count(rope: &Rope) -> u32 {
    rope.len_lines() as u32
}

/// The line holding `offset` and the UTF-16 column within it.
pub fn line_col(rope: &Rope, offset: u32) -> (u32, u32) {
    let char = char_of(rope, offset);
    let line = rope.char_to_line(char);
    let start = rope.char_to_utf16_cu(rope.line_to_char(line));
    (line as u32, (rope.char_to_utf16_cu(char) - start) as u32)
}

/// UTF-16 offsets of a line's text, without its line break.
pub fn line_range(rope: &Rope, line: u32) -> std::ops::Range<u32> {
    let line = (line as usize).min(rope.len_lines().saturating_sub(1));
    let start = rope.line_to_char(line);
    let text = rope.line(line);
    let mut end = start + text.len_chars();
    // Drop `\n`, `\r\n` (and a lone `\r`).
    for _ in 0..2 {
        if end > start && matches!(rope.char(end - 1), '\n' | '\r') {
            end -= 1;
        }
    }
    rope.char_to_utf16_cu(start) as u32..rope.char_to_utf16_cu(end) as u32
}

/// The offset of UTF-16 column `col` in `line`, clamped to the line's end.
pub fn offset_at(rope: &Rope, line: u32, col: u32) -> u32 {
    let range = line_range(rope, line);
    (range.start + col).min(range.end)
}

/// A line's text, without its line break.
pub fn line_text(rope: &Rope, line: u32) -> String {
    let range = line_range(rope, line);
    slice(rope, range.start, range.end)
}

/// The text between two UTF-16 offsets.
pub fn slice(rope: &Rope, from: u32, to: u32) -> String {
    let (from, to) = (char_of(rope, from.min(to)), char_of(rope, to.max(from)));
    rope.slice(from..to).to_string()
}

/// UTF-8 byte index of UTF-16 column `col` in `text` (clamped; inside a surrogate pair rounds down).
pub fn byte_of_col(text: &str, col: u32) -> usize {
    let mut units = 0u32;
    for (byte, c) in text.char_indices() {
        if units + c.len_utf16() as u32 > col {
            return byte;
        }
        units += c.len_utf16() as u32;
    }
    text.len()
}

/// UTF-16 column of UTF-8 byte index `byte` in `text` (clamped; inside a character rounds down).
pub fn col_of_byte(text: &str, byte: usize) -> u32 {
    text.char_indices().take_while(|(i, c)| i + c.len_utf8() <= byte).map(|(_, c)| c.len_utf16() as u32).sum()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Word,
    Space,
    Other,
}

fn category(c: char) -> Category {
    if c.is_alphanumeric() || c == '_' {
        Category::Word
    } else if c.is_whitespace() {
        Category::Space
    } else {
        Category::Other
    }
}

/// Where a move by word from `offset` lands: over leading spaces, then to the end of the run of
/// characters of one kind (word, punctuation). A line boundary is one stop of its own.
pub fn group_boundary(rope: &Rope, offset: u32, forward: bool) -> u32 {
    let (line, col) = line_col(rope, offset);
    let range = line_range(rope, line);
    let text = slice(rope, range.start, range.end);
    let at = byte_of_col(&text, col);
    if forward {
        if at == text.len() {
            return if line + 1 < line_count(rope) { line_range(rope, line + 1).start } else { offset };
        }
        let mut kind = None;
        let mut end = text.len();
        for (i, c) in text[at..].char_indices() {
            let next = category(c);
            match kind {
                None | Some(Category::Space) => kind = Some(next),
                Some(current) if current != next => {
                    end = at + i;
                    break;
                }
                Some(_) => {}
            }
        }
        range.start + col_of_byte(&text, end)
    } else {
        if at == 0 {
            return if line > 0 { line_range(rope, line - 1).end } else { offset };
        }
        let mut kind = None;
        let mut start = 0;
        for (i, c) in text[..at].char_indices().rev() {
            let next = category(c);
            match kind {
                None | Some(Category::Space) => kind = Some(next),
                Some(current) if current != next => {
                    start = i + c.len_utf8();
                    break;
                }
                Some(_) => {}
            }
        }
        range.start + col_of_byte(&text, start)
    }
}

/// The run of characters of one kind around `offset` (what a double click selects): the word
/// under it, else the punctuation or spaces. Empty on an empty line.
pub fn word_at(rope: &Rope, offset: u32) -> std::ops::Range<u32> {
    let (line, col) = line_col(rope, offset);
    let range = line_range(rope, line);
    let text = slice(rope, range.start, range.end);
    let at = byte_of_col(&text, col);
    // At the end of a line the character before the position decides.
    let Some(kind) = text[at..].chars().next().or_else(|| text[..at].chars().next_back()).map(category) else {
        return offset..offset;
    };
    let start = text[..at].char_indices().rev().take_while(|(_, c)| category(*c) == kind).last().map_or(at, |(i, _)| i);
    let end = at
        + text[at..]
            .char_indices()
            .take_while(|(_, c)| category(*c) == kind)
            .last()
            .map_or(0, |(i, c)| i + c.len_utf8());
    range.start + col_of_byte(&text, start)..range.start + col_of_byte(&text, end)
}

/// Which side of an insertion a position at that exact place moves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Assoc {
    /// Stays before text inserted at the position.
    Before,
    /// Moves after text inserted at the position (a caret that is typing).
    After,
}

/// `offset` after `changes` (CodeMirror change-set semantics: ordered, non-overlapping, all in
/// the coordinates of the document before the changes).
pub fn map_offset(offset: u32, changes: &[TextChange], assoc: Assoc) -> u32 {
    let mut delta: i64 = 0;
    for change in changes {
        let inserted = change.insert.encode_utf16().count() as i64;
        if change.to < offset || (change.to == offset && (change.from < change.to || assoc == Assoc::After)) {
            delta += inserted - i64::from(change.to - change.from);
        } else if change.from < offset || (change.from == offset && change.from == change.to && assoc == Assoc::After) {
            // Inside a replaced range: the position collapses to the replacement's end or start.
            let at = if assoc == Assoc::After { i64::from(change.from) + inserted } else { i64::from(change.from) };
            return (at + delta).max(0) as u32;
        } else {
            break;
        }
    }
    (i64::from(offset) + delta).max(0) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    #[test]
    fn lines_and_columns_are_utf16() {
        // "a😀b" is 4 UTF-16 units; the emoji is a surrogate pair.
        let rope = Rope::from_str("a😀b\r\n中文\n\nlast");
        assert_eq!(line_count(&rope), 4);
        assert_eq!(len_utf16(&rope), 4 + 2 + 2 + 1 + 1 + 4);
        assert_eq!(line_range(&rope, 0), 0..4);
        assert_eq!(line_range(&rope, 1), 6..8);
        assert_eq!(line_range(&rope, 2), 9..9);
        assert_eq!(line_range(&rope, 3), 10..14);
        assert_eq!(line_text(&rope, 0), "a😀b");
        assert_eq!(line_text(&rope, 1), "中文");
        assert_eq!(line_col(&rope, 3), (0, 3));
        assert_eq!(line_col(&rope, 7), (1, 1));
        assert_eq!(line_col(&rope, 9), (2, 0));
        assert_eq!(line_col(&rope, 999), (3, 4));
        assert_eq!(offset_at(&rope, 1, 1), 7);
        assert_eq!(offset_at(&rope, 0, 99), 4);
        assert_eq!(slice(&rope, 1, 3), "😀");
        assert_eq!(slice(&rope, 8, 6), "中文");
        // A trailing newline starts an empty last line.
        assert_eq!(line_count(&Rope::from_str("x\n")), 2);
        assert_eq!(line_range(&Rope::from_str("x\n"), 1), 2..2);
        assert_eq!(line_range(&Rope::from_str(""), 0), 0..0);
    }

    #[test]
    fn converts_between_utf16_columns_and_utf8_bytes() {
        let text = "a😀中b";
        assert_eq!([0, 1, 3, 4, 5].map(|col| byte_of_col(text, col)), [0, 1, 5, 8, 9]);
        // Inside the surrogate pair rounds down to the emoji's start; past the end clamps.
        assert_eq!(byte_of_col(text, 2), 1);
        assert_eq!(byte_of_col(text, 99), text.len());
        assert_eq!([0, 1, 5, 8, 9].map(|byte| col_of_byte(text, byte)), [0, 1, 3, 4, 5]);
        assert_eq!(col_of_byte(text, 3), 1);
        assert_eq!(col_of_byte(text, 99), 5);
    }

    // src/editor/core/cm/caretCommands.test.ts (movement by word, as CodeMirror's cursorGroup commands)
    #[test]
    fn moves_by_word_and_finds_the_word_at_a_position() {
        let rope = Rope::from_str("let foo_bar = a.b(😀);\n\n  next");
        let forward: Vec<u32> =
            std::iter::successors(Some(0), |&o| Some(group_boundary(&rope, o, true)).filter(|n| *n != o)).collect();
        // let| foo_bar| =| a|.|b|(😀);| ⏎ (empty line) ⏎ next|: an emoji is punctuation, not a word.
        assert_eq!(forward, [0, 3, 11, 13, 15, 16, 17, 22, 23, 24, 30]);
        let backward: Vec<u32> =
            std::iter::successors(Some(30), |&o| Some(group_boundary(&rope, o, false)).filter(|n| *n != o)).collect();
        assert_eq!(backward, [30, 26, 24, 23, 22, 17, 16, 15, 14, 12, 4, 0]);
        assert_eq!(word_at(&rope, 5), 4..11);
        assert_eq!(word_at(&rope, 4), 4..11);
        assert_eq!(word_at(&rope, 11), 11..12);
        assert_eq!(word_at(&rope, 15), 15..16);
        assert_eq!(word_at(&rope, 18), 17..22);
        // At a line's end the last word is taken; an empty line has none.
        assert_eq!(word_at(&rope, 30), 26..30);
        assert_eq!(word_at(&rope, 23), 23..23);
    }

    #[test]
    fn maps_positions_through_changes() {
        // Insertion before, at and after a position.
        assert_eq!(map_offset(5, &[change(2, 2, "xx")], Assoc::Before), 7);
        assert_eq!(map_offset(5, &[change(5, 5, "xx")], Assoc::Before), 5);
        assert_eq!(map_offset(5, &[change(5, 5, "xx")], Assoc::After), 7);
        assert_eq!(map_offset(5, &[change(9, 9, "xx")], Assoc::After), 5);
        // Deletion before and around a position.
        assert_eq!(map_offset(5, &[change(1, 3, "")], Assoc::Before), 3);
        assert_eq!(map_offset(5, &[change(3, 8, "")], Assoc::Before), 3);
        assert_eq!(map_offset(5, &[change(3, 8, "ab")], Assoc::After), 5);
        assert_eq!(map_offset(5, &[change(3, 5, "")], Assoc::After), 3);
        // Several changes, all in original coordinates; surrogate pairs count two units.
        assert_eq!(map_offset(10, &[change(0, 0, "😀"), change(4, 6, ""), change(20, 20, "z")], Assoc::Before), 10);
        assert_eq!(map_offset(0, &[change(0, 0, "ab")], Assoc::Before), 0);
    }
}
