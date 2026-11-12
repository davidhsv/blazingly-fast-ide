//! Dragging over the line numbers selects whole lines.

use super::{Edit, EditState};

/// The lines from `start` to `end` (0-based, either order), the head at the far line boundary:
/// past the line break of the last line, or at the start of the first line when dragging up.
pub fn line_range_selection(state: &EditState, start: u32, end: u32) -> Edit {
    let after = |number: u32| state.next_line_start(&state.line(number)).unwrap_or_else(|| state.doc_len());
    let range = if end >= start { (state.line(start).from, after(end)) } else { (after(start), state.line(end).from) };
    Edit::select(vec![range], 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_whole_lines_in_either_direction() {
        let state = EditState::new("a\nb\nc\nd");
        let main = |edit: Edit| edit.selections[edit.main];
        assert_eq!(main(line_range_selection(&state, 1, 2)), (2, 6));
        assert_eq!(main(line_range_selection(&state, 2, 0)), (6, 0));
        assert_eq!(main(line_range_selection(&state, 3, 3)), (6, 7));
        // Windows line breaks: the head goes past both characters of the break.
        let crlf = EditState::new("a\r\nb\r\nc");
        assert_eq!(main(line_range_selection(&crlf, 0, 1)), (0, 6));
    }
}
