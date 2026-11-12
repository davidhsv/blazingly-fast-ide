//! Text measured as the backend and the editor measure it: lengths, columns and slices in UTF-16 code units.

use crate::editor::buffer::byte_of_col;

/// Length of `text` in UTF-16 code units.
pub fn len(text: &str) -> u32 {
    text.chars().map(|c| c.len_utf16() as u32).sum()
}

/// The part of `text` between the UTF-16 columns `start` and `end`, both clamped to the text; a column inside
/// a surrogate pair rounds down. Empty when `end` is not after `start`.
pub fn slice(text: &str, start: u32, end: u32) -> &str {
    text.get(byte_of_col(text, start)..byte_of_col(text, end)).unwrap_or("")
}

/// The part of `text` from the UTF-16 column `start` on.
pub fn slice_from(text: &str, start: u32) -> &str {
    &text[byte_of_col(text, start)..]
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: lengths and slices count UTF-16 code units, and out-of-range columns are clamped.
    #[test]
    fn lengths_and_slices_count_utf16_code_units() {
        let text = "a😀é";
        assert_eq!(len(text), 4);
        assert_eq!(slice(text, 1, 3), "😀");
        assert_eq!(slice(text, 3, 9), "é");
        assert_eq!(slice(text, 3, 1), "");
        assert_eq!(slice_from(text, 3), "é");
        assert_eq!(slice_from(text, 9), "");
    }
}
