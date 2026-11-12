//! Overwrite mode (the Insert key): typed characters replace the ones after the caret, never the
//! line break. Whether the mode is on is a plain value the caller keeps.

use super::{Edit, EditState, RangeEdit, SelectionExt, caret, change, change_by_range, utf16_len};

/// Typing `text` in overwrite mode: at a caret it replaces as many characters after the caret
/// (up to the line's end), a selection it replaces. `None` for text with a line break, which is
/// typed as usual.
pub fn type_overwriting(state: &EditState, text: &str) -> Option<Edit> {
    if text.contains(['\r', '\n']) {
        return None;
    }
    let length = utf16_len(text);
    Some(change_by_range(state, |_, range| {
        let to = if range.is_caret() { state.line_at(range.head()).to.min(range.head() + length) } else { range.end() };
        RangeEdit { changes: vec![change(range.start(), to, text)], range: caret(range.start() + length) }
    }))
}

#[cfg(test)]
mod tests {
    use super::super::testing::{shown, state};
    use super::*;

    #[test]
    fn replaces_the_characters_after_the_caret_never_the_line_break() {
        let before = state("ab|cd\nx");
        assert_eq!(shown(&before, type_overwriting(&before, "XYZ")), "abXYZ|\nx");
        let selected = state("a[bc]d|e");
        assert_eq!(shown(&selected, type_overwriting(&selected, "Q")), "aQ|dQ|");
        assert_eq!(type_overwriting(&before, "\n"), None);
    }
}
