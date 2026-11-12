//! Edit › Sort Lines / Reverse Lines: reorders the lines each selection covers, or the whole file
//! when nothing is selected.

use super::{Edit, EditState, SelectionExt, change, map_after, map_before};
use crate::editor::buffer::Assoc;
use crate::platform::collate;
use ide_core::TextChange;

/// The line blocks the command reorders, as `(from, to)` offsets of whole lines: one per
/// non-empty selection range (touching blocks merged), or the document without a selection.
fn blocks(state: &EditState) -> Vec<(u32, u32)> {
    let ranges: Vec<_> = state.selections.iter().filter(|range| !range.is_caret()).collect();
    if ranges.is_empty() {
        // The empty line after a final line break stays last.
        let count = state.line_count();
        let last = state.line(count.saturating_sub(1));
        let to = if last.length() == 0 && count > 1 { state.line(count - 2).to } else { state.doc_len() };
        return vec![(0, to)];
    }
    let mut out: Vec<(u32, u32)> = Vec::new();
    for range in ranges {
        let first = state.line_at(range.start());
        let end_line = state.line_at(range.end());
        // A selection ending at a line start does not include that line.
        let last = if range.end() > range.start() && end_line.from == range.end() {
            state.line(end_line.number.saturating_sub(1))
        } else {
            end_line
        };
        match out.last_mut() {
            Some(previous) if first.from <= previous.1 => previous.1 = previous.1.max(last.to),
            _ => out.push((first.from, last.to)),
        }
    }
    out
}

fn rewrite(state: &EditState, transform: impl Fn(&mut [&str])) -> Option<Edit> {
    let mut changes: Vec<TextChange> = Vec::new();
    for (from, to) in blocks(state) {
        let text = state.slice(from, to);
        let mut lines: Vec<&str> = text.split(state.line_break.as_str()).collect();
        if lines.len() < 2 {
            continue;
        }
        transform(&mut lines);
        let next = lines.join(state.line_break.as_str());
        if next != text {
            changes.push(change(from, to, next));
        }
    }
    if changes.is_empty() {
        return None;
    }
    if state.selections.iter().all(SelectionExt::is_caret) {
        return Some(Edit::following(state, changes, Assoc::Before));
    }
    let ranges = state
        .selections
        .iter()
        .map(|range| (map_before(range.anchor(), &changes), map_after(range.head(), &changes)))
        .collect();
    Some(Edit::new(changes, ranges, state.main))
}

/// Sort Lines: the lines in the default collation order (case and accents secondary), stable.
/// `None` when every block is one line or already sorted.
pub fn sort_lines(state: &EditState) -> Option<Edit> {
    rewrite(state, |lines| lines.sort_by(|a, b| collate::compare(a, b)))
}

/// Reverse Lines: the lines in reverse order. `None` when every block is one line.
pub fn reverse_lines(state: &EditState) -> Option<Edit> {
    rewrite(state, |lines| lines.reverse())
}

#[cfg(test)]
mod tests {
    use super::super::testing::{run, state};
    use super::*;

    fn text_after(marked: &str, command: fn(&EditState) -> Option<Edit>) -> Option<String> {
        let before = state(marked);
        let edit = command(&before)?;
        Some(run(&before, Some(edit)).text())
    }

    #[test]
    fn sorts_the_whole_file_without_a_selection() {
        assert_eq!(text_after("c\na\nb", sort_lines).as_deref(), Some("a\nb\nc"));
        assert_eq!(text_after("c\na\nb", reverse_lines).as_deref(), Some("b\na\nc"));
        assert_eq!(text_after("c\nb\na\n", sort_lines).as_deref(), Some("a\nb\nc\n"));
        assert_eq!(text_after("c\nb\na\n", reverse_lines).as_deref(), Some("a\nb\nc\n"));
        // Windows line breaks are kept as they are.
        assert_eq!(text_after("c\r\nb\r\na\r\n", sort_lines).as_deref(), Some("a\r\nb\r\nc\r\n"));
    }

    #[test]
    fn sorts_only_the_selected_lines_a_selection_ending_at_a_line_start_excludes_that_line() {
        assert_eq!(text_after("z\n[c\nb\na\n]y", sort_lines).as_deref(), Some("z\na\nb\nc\ny"));
    }

    #[test]
    fn handles_several_selections_independently() {
        assert_eq!(text_after("[b\na]\n--\n[d\nc]", sort_lines).as_deref(), Some("a\nb\n--\nc\nd"));
    }

    #[test]
    fn does_nothing_for_a_single_line_or_an_already_sorted_block() {
        assert_eq!(text_after("only", sort_lines), None);
        assert_eq!(text_after("a\nb", sort_lines), None);
    }
}
