//! Marked strings for the tests of the editing commands: `|` is a caret and a pair of marks a
//! selection, so a case reads as the text before and the text after.

use super::{Edit, EditState, Selection, SelectionExt, apply, caret};

fn parse(marked: &str, open: char, close: char) -> EditState {
    let mut text = String::new();
    let mut units = 0u32;
    let mut ranges: Vec<Selection> = Vec::new();
    let mut anchor = 0;
    for c in marked.chars() {
        if c == '|' {
            ranges.push(caret(units));
        } else if c == open {
            anchor = units;
        } else if c == close {
            ranges.push((anchor, units));
        } else {
            text.push(c);
            units += c.len_utf16() as u32;
        }
    }
    if ranges.is_empty() {
        ranges.push(caret(0));
    }
    EditState::new(&text).with_selections(ranges, 0)
}

/// A state from a marked string: `|` is a caret, `[`…`]` a selection. The first range is the
/// main one; without a mark the caret is at the start.
pub(crate) fn state(marked: &str) -> EditState {
    parse(marked, '[', ']')
}

/// As [`state`], with `«`…`»` for selections: for texts that contain brackets.
pub(crate) fn state_guillemets(marked: &str) -> EditState {
    parse(marked, '«', '»')
}

/// As [`state`], with carets only: for texts that contain brackets.
pub(crate) fn state_carets(marked: &str) -> EditState {
    parse(marked, '\0', '\0')
}

/// [`state`] with a tab size and an indent unit.
pub(crate) fn styled(marked: &str, tab_size: u32, indent_unit: &str) -> EditState {
    EditState { tab_size, indent_unit: indent_unit.to_owned(), ..state(marked) }
}

fn insert(units: &mut Vec<u16>, at: u32, mark: char) {
    let at = (at as usize).min(units.len());
    units.splice(at..at, mark.encode_utf16(&mut [0; 2]).iter().copied());
}

fn render(state: &EditState, open: char, close: char) -> String {
    let mut units = state.units();
    let mut ranges = state.selections.clone();
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start()));
    for range in ranges {
        if range.is_caret() {
            insert(&mut units, range.head(), '|');
        } else {
            insert(&mut units, range.end(), close);
            insert(&mut units, range.start(), open);
        }
    }
    String::from_utf16_lossy(&units)
}

/// The text with its carets and selections marked as [`state`] reads them.
pub(crate) fn show(state: &EditState) -> String {
    render(state, '[', ']')
}

/// The text with its carets marked and selections collapsed to their heads.
pub(crate) fn show_carets(state: &EditState) -> String {
    let heads = state.selections.iter().map(|range| caret(range.head())).collect();
    render(&EditState { selections: heads, ..state.clone() }, '[', ']')
}

/// The state after `edit`; the same state when the command did not apply.
pub(crate) fn run(state: &EditState, edit: Option<Edit>) -> EditState {
    edit.map_or_else(|| state.clone(), |edit| apply(state, &edit))
}

/// The marked text after `edit` (the text before it when the command did not apply).
pub(crate) fn shown(state: &EditState, edit: Option<Edit>) -> String {
    show(&run(state, edit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marked_strings_round_trip_in_utf16_offsets() {
        let parsed = state("a😀|b [cd] e|");
        assert_eq!(parsed.text(), "a😀b cd e");
        assert_eq!(parsed.selections, [(3, 3), (5, 7), (9, 9)]);
        assert_eq!(show(&parsed), "a😀|b [cd] e|");
        assert_eq!(state_guillemets("f(«x»)[0]").selections, [(2, 3)]);
        assert_eq!(state_carets("a[|]").selections, [(2, 2)]);
        assert_eq!(state("plain").selections, [(0, 0)]);
        assert_eq!(show_carets(&state("[ab]c")), "ab|c");
    }
}
