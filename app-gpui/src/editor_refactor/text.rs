//! Text helpers the refactorings share: words, lines and UTF-16 offsets of plain strings.

use crate::editor::buffer;
use ropey::Rope;

/// A letter, digit or `_`: what a word is made of.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The word touching `offset` (on either side), or `None` when no word character is next to it.
pub fn word_at(rope: &Rope, offset: u32) -> Option<(u32, u32)> {
    let (line, col) = buffer::line_col(rope, offset);
    let range = buffer::line_range(rope, line);
    let text = buffer::slice(rope, range.start, range.end);
    let at = buffer::byte_of_col(&text, col);
    let start = text[..at].char_indices().rev().take_while(|(_, c)| is_word_char(*c)).last().map_or(at, |(i, _)| i);
    let end = at + text[at..].char_indices().find(|(_, c)| !is_word_char(*c)).map_or(text.len() - at, |(i, _)| i);
    (start < end)
        .then(|| (range.start + buffer::col_of_byte(&text, start), range.start + buffer::col_of_byte(&text, end)))
}

/// Length of `text` in UTF-16 code units.
pub fn utf16_len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// The UTF-16 offset of byte `byte` of `text`.
pub fn utf16_at(text: &str, byte: usize) -> u32 {
    utf16_len(&text[..byte])
}

/// The byte of UTF-16 offset `offset` of `text` (the end when past it).
pub fn byte_at(text: &str, offset: u32) -> usize {
    buffer::byte_of_col(text, offset)
}

/// The text between two UTF-16 offsets of `text`.
pub fn slice16(text: &str, from: u32, to: u32) -> &str {
    let start = byte_at(text, from);
    &text[start..byte_at(text, to).max(start)]
}

/// The file name of a path, `/p/other.rs` is `other.rs`.
pub fn basename(path: &str) -> &str {
    path.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_touch_the_offset_from_either_side() {
        let rope = Rope::from_str("fn ab_c() {}\n\n");
        assert_eq!(word_at(&rope, 3), Some((3, 7)));
        assert_eq!(word_at(&rope, 7), Some((3, 7)));
        assert_eq!(word_at(&rope, 13), None);
        assert_eq!(basename("/p/other.rs"), "other.rs");
        assert_eq!(slice16("😀ab", 2, 3), "a");
    }
}
