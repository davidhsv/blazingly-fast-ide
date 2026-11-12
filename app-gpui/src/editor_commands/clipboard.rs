//! Copy, cut and paste of editor text.
//!
//! Copy without a selection takes the caret lines (a line-wise copy), and pasting the text of a
//! line-wise copy at carets puts it above their lines. Whether the clipboard holds a line-wise
//! copy is the caller's to remember (the clipboard history's latest entry): [`paste`] takes it
//! as an argument.
//!
//! A paste is the edit [`paste`] answers, then the string literal escaping and the
//! reformatting on paste in that order, each taking the edit before it.

use super::{Edit, EditState, RangeEdit, SelectionExt, caret, change, change_by_range, insertion, utf16_len};
use crate::editor::buffer::Assoc;

/// What a copy or a cut puts on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    pub text: String,
    /// Nothing was selected: the text is the caret lines, without their line breaks.
    pub linewise: bool,
}

/// The copied text and the ranges a cut removes: the selected ranges, or, when nothing is
/// selected, each caret line once with the line break after it.
fn copied_ranges(state: &EditState) -> (Copied, Vec<(u32, u32)>) {
    let selected: Vec<(u32, u32)> =
        state.selections.iter().filter(|range| !range.is_caret()).map(|range| (range.start(), range.end())).collect();
    if !selected.is_empty() {
        let parts: Vec<String> = selected.iter().map(|(from, to)| state.slice(*from, *to)).collect();
        return (Copied { text: parts.join(state.line_break.as_str()), linewise: false }, selected);
    }
    let mut lines = Vec::new();
    let mut ranges = Vec::new();
    let mut upto: Option<u32> = None;
    for range in &state.selections {
        let line = state.line_at(range.start());
        if upto.is_none_or(|done| line.number > done) {
            ranges.push((line.from, state.next_line_start(&line).unwrap_or(line.to)));
            upto = Some(line.number);
            lines.push(line.text);
        }
    }
    (Copied { text: lines.join(state.line_break.as_str()), linewise: true }, ranges)
}

/// What Copy puts on the clipboard: the selected ranges joined by line breaks, or the caret
/// lines (each once) when nothing is selected.
pub fn copy(state: &EditState) -> Copied {
    copied_ranges(state).0
}

/// What Cut puts on the clipboard (as [`copy`]) and the edit removing it: the selected ranges,
/// or the caret lines with their line breaks.
pub fn cut(state: &EditState) -> (Copied, Edit) {
    let (copied, ranges) = copied_ranges(state);
    let changes = ranges.into_iter().map(|(from, to)| change(from, to, "")).collect();
    (copied, Edit::following(state, changes, Assoc::Before))
}

/// `text` with its line breaks (`\r\n`, `\r`, `\n`) made `line_break`.
fn with_line_breaks(text: &str, line_break: &str) -> String {
    let unix = text.replace("\r\n", "\n").replace('\r', "\n");
    if line_break == "\n" { unix } else { unix.replace('\n', line_break) }
}

/// The insert of each of `caret_count` carets for `text`: line *i* for caret *i*, empty inserts
/// for the carets past the last line and the lines past the last caret dropped. A single line (a
/// lone trailing line break is an artifact of copying) repeats at every caret without its line
/// break. `None` with one caret.
fn lines_per_caret(text: &str, caret_count: usize) -> Option<Vec<String>> {
    if caret_count <= 1 {
        return None;
    }
    let unix = with_line_breaks(text, "\n");
    let mut lines: Vec<&str> = unix.split('\n').collect();
    if lines.len() == 2 && lines[1].is_empty() {
        lines.pop();
    }
    if let [line] = lines.as_slice() {
        return Some(vec![(*line).to_owned(); caret_count]);
    }
    Some((0..caret_count).map(|index| lines.get(index).copied().unwrap_or_default().to_owned()).collect())
}

/// Paste `text`; `linewise` says the clipboard holds a line-wise copy of exactly this text.
///
/// A line-wise copy at carets goes above each distinct caret line, the carets staying on their
/// text. Otherwise, with several ranges, the lines of `text` spread over them, line *i* to range
/// *i* (a single line repeating at each); with one, `text` replaces the selection. Line breaks in `text` become
/// the document's. `None` for empty text that is not a line-wise copy.
pub fn paste(state: &EditState, text: &str, linewise: bool) -> Option<Edit> {
    if text.is_empty() && !linewise {
        return None;
    }
    if linewise && state.selections.iter().all(SelectionExt::is_caret) {
        let insert = with_line_breaks(text, &state.line_break) + &state.line_break;
        let mut starts: Vec<u32> = state.selections.iter().map(|range| state.line_at(range.head()).from).collect();
        starts.dedup();
        let changes = starts.into_iter().map(|from| insertion(from, insert.clone())).collect();
        return Some(Edit::following(state, changes, Assoc::After));
    }
    let inserts = lines_per_caret(text, state.selections.len())
        .unwrap_or_else(|| vec![with_line_breaks(text, &state.line_break); state.selections.len()]);
    Some(change_by_range(state, |index, range| {
        let insert = inserts.get(index).cloned().unwrap_or_default();
        let end = range.start() + utf16_len(&insert);
        RangeEdit { changes: vec![change(range.start(), range.end(), insert)], range: caret(end) }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::paste::{PasteReindent, reindent_paste};
    use crate::editor_commands::{IndentContext, Indenter, apply};

    fn carets(doc: &str, positions: &[u32]) -> EditState {
        EditState::new(doc).with_selections(positions.iter().map(|pos| caret(*pos)).collect(), 0)
    }

    /// The line-wise marker of the clipboard after `copied`: set while it holds that copy.
    fn marker(copied: &Copied, text: &str) -> bool {
        copied.linewise && copied.text == text
    }

    fn pasted(state: &EditState, text: &str, linewise: bool) -> EditState {
        apply(state, &paste(state, text, linewise).expect("the paste applies"))
    }

    fn heads(state: &EditState) -> Vec<u32> {
        state.selections.iter().map(SelectionExt::head).collect()
    }

    #[test]
    fn pastes_a_copied_caret_line_above_the_line_through_the_context_menu() {
        let copied = copy(&carets("one\ntarget", &[1]));
        assert_eq!(copied.text, "one");
        let target = carets("one\ntarget", &[7]);
        let after = pasted(&target, &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "one\none\ntarget");
        assert_eq!(after.main().head(), 11);
    }

    #[test]
    fn tracks_a_line_wise_cut_before_the_line_is_removed() {
        let before = carets("one\ntarget", &[1]);
        let (copied, edit) = cut(&before);
        let after = apply(&before, &edit);
        assert_eq!(after.text(), "target");
        assert_eq!(copied, Copied { text: "one".to_owned(), linewise: true });
        let restored = pasted(&carets("target", &[3]), &copied.text, marker(&copied, &copied.text));
        assert_eq!(restored.text(), "one\ntarget");
        assert_eq!(restored.main().head(), 7);
    }

    #[test]
    fn replaces_a_non_empty_selection_even_when_the_copy_was_line_wise() {
        let copied = copy(&carets("one\ntarget", &[1]));
        let selected = EditState::new("one\ntarget").with_selections(vec![(5, 8)], 0);
        let after = pasted(&selected, &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "one\ntoneet");
        assert_eq!(after.main().head(), 8);
    }

    #[test]
    fn pastes_unmatched_text_at_the_caret() {
        let copied = copy(&carets("one\ntarget", &[1]));
        let after = pasted(&carets("one\ntarget", &[7]), "other\n", marker(&copied, "other\n"));
        assert_eq!(after.text(), "one\ntarother\nget");
    }

    #[test]
    fn does_not_treat_a_later_selection_copy_as_line_wise() {
        assert!(copy(&carets("one\ntarget", &[1])).linewise);
        let copied = copy(&EditState::new("one\ntarget").with_selections(vec![(5, 8)], 0));
        assert_eq!(copied, Copied { text: "arg".to_owned(), linewise: false });
        let after = pasted(&carets("one\ntarget", &[7]), &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "one\ntarargget");
    }

    #[test]
    fn copies_distinct_caret_lines_and_pastes_the_block_once_per_distinct_target_line() {
        let copied = copy(&carets("one\ntwo", &[1, 2, 5]));
        assert_eq!(copied, Copied { text: "one\ntwo".to_owned(), linewise: true });
        let target = EditState::new("one\ntwo").with_selections([1, 2, 4, 7].map(caret).to_vec(), 2);
        let after = pasted(&target, &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "one\ntwo\none\none\ntwo\ntwo");
        assert_eq!(heads(&after), [9, 10, 20, 23]);
        assert_eq!(after.main, 2);
    }

    #[test]
    fn keeps_mixed_empty_and_non_empty_selections_on_the_usual_per_caret_split_path() {
        let copied = copy(&carets("one\ntwo", &[1]));
        let mixed = EditState::new("one\ntwo").with_selections(vec![(0, 2), caret(5)], 0);
        assert_eq!(pasted(&mixed, "one", marker(&copied, "one")).text(), "onee\ntonewo");
    }

    #[test]
    fn moves_a_caret_at_column_n_down_with_its_original_line() {
        for pos in [0, 3] {
            let before = carets("one", &[pos]);
            let copied = copy(&before);
            let after = pasted(&before, &copied.text, marker(&copied, &copied.text));
            assert_eq!(after.text(), "one\none");
            assert_eq!(after.main().head(), pos + 4);
        }
    }

    #[test]
    fn copies_and_pastes_an_empty_caret_line() {
        let before = carets("", &[0]);
        let copied = copy(&before);
        let after = pasted(&before, &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "\n");
        assert_eq!(after.main().head(), 1);
    }

    #[test]
    fn uses_document_line_breaks_without_counting_crlf_as_two_document_positions() {
        // The document holds its CRLF breaks, so they are two code units here: the carets on
        // both lines are at 1 and 6, and the caret moved past the pasted block ends at 12.
        let copied = copy(&carets("one\r\ntwo", &[1, 6]));
        assert_eq!(copied.text, "one\r\ntwo");
        let after = pasted(&carets("one\r\ntwo", &[2]), &copied.text, marker(&copied, &copied.text));
        assert_eq!(after.text(), "one\r\ntwo\r\none\r\ntwo");
        assert_eq!(after.main().head(), 12);
    }

    #[test]
    fn keeps_the_original_caret_when_reformatting_with_mode() {
        let two = |cx: &IndentContext<'_>| -> Option<u32> { Some(if cx.pos == 0 { 0 } else { 2 }) };
        let indenter: Indenter<'_> = &two;
        let text = "      a();\n          b();";
        for (mode, inserted) in
            [(PasteReindent::IndentBlock, "  a();\n      b();\n"), (PasteReindent::IndentEachLine, "  a();\n  b();\n")]
        {
            let before = carets("if {\n  target\n}", &[10]);
            let raw = paste(&before, text, true).expect("the paste applies");
            let after = apply(&before, &reindent_paste(&before, &raw, mode, Some(indenter)));
            assert_eq!(after.text(), format!("if {{\n{inserted}  target\n}}"));
            assert_eq!(after.main().head(), 10 + utf16_len(inserted));
            assert_eq!(after.line_at(after.main().head()).text, "  target");
        }
    }

    /// Seven lines with a caret inside each pair of quotes.
    fn quoted_lines() -> EditState {
        let line = "icjsdiocjosid \"\" cdscsd";
        let width = utf16_len(line) + 1;
        let positions: Vec<u32> = (0..7).map(|index| index * width + 15).collect();
        carets(&[line; 7].join("\n"), &positions)
    }

    fn quoted(fill: &str) -> String {
        let mut letters = fill.chars();
        (0..7)
            .map(|_| format!("icjsdiocjosid \"{}\" cdscsd", letters.next().map(String::from).unwrap_or_default()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn pastes_one_line_per_caret_when_the_counts_match() {
        assert_eq!(pasted(&quoted_lines(), "s\nd\nd\ns\nd\nd\ne", false).text(), quoted("sddsdde"));
    }

    #[test]
    fn keeps_a_trailing_newline_as_the_last_carets_empty_line() {
        assert_eq!(pasted(&quoted_lines(), "s\nd\nd\ns\nd\nd\n", false).text(), quoted("sddsdd"));
    }

    #[test]
    fn drops_the_extra_lines_when_there_are_more_lines_than_carets() {
        assert_eq!(pasted(&carets("a b", &[1, 3]), "x\ny\nz", false).text(), "ax by");
    }

    #[test]
    fn pastes_empty_lines_into_the_extra_carets_when_there_are_more_carets_than_lines() {
        assert_eq!(pasted(&carets("a b c", &[1, 3, 5]), "x\ny", false).text(), "ax by c");
    }

    #[test]
    fn repeats_a_single_line_at_every_caret() {
        assert_eq!(pasted(&carets("a b", &[1, 3]), "xyz", false).text(), "axyz bxyz");
    }

    #[test]
    fn treats_a_single_copied_line_with_a_trailing_newline_as_one_line() {
        assert_eq!(pasted(&carets("a b", &[1, 3]), "x\n", false).text(), "ax bx");
    }

    #[test]
    fn pastes_the_whole_text_at_a_single_caret() {
        assert_eq!(pasted(&carets("a", &[1]), "x\ny", false).text(), "ax\ny");
    }
}
