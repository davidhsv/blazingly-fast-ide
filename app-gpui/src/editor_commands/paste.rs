//! Reformat on paste: a pasted block moves to the paste column (Indent Block), then every pasted
//! line takes the language's indentation where it can compute one (Indent Each Line). A paste
//! after nothing but whitespace starts at the line's proper indentation.

use super::{
    Edit, EditState, Indenter, Selection, apply, apply_changes, change, indentation, leading_space, map_selection,
    utf16_len,
};
use crate::editor::buffer::Assoc;
use ide_core::TextChange;

/// Settings › Smart Keys › Reformat on paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasteReindent {
    /// The text goes in as it was copied.
    None,
    /// The block shifts to the paste column, keeping its inner indentation.
    IndentBlock,
    /// The block shifts, then every pasted line gets the language's indentation.
    IndentEachLine,
}

/// The columns of the spaces and tabs `text` starts with.
fn leading_columns(text: &str, tab_size: u32) -> u32 {
    let tab = tab_size.max(1);
    let mut column = 0;
    for c in text.chars() {
        match c {
            ' ' => column += 1,
            '\t' => column += tab - column % tab,
            _ => break,
        }
    }
    column
}

/// Whitespace `columns` wide: tabs first when the code style uses them, then spaces.
fn padding(columns: u32, use_tabs: bool, tab_size: u32) -> String {
    let tab = tab_size.max(1);
    if use_tabs {
        "\t".repeat((columns / tab) as usize) + &" ".repeat((columns % tab) as usize)
    } else {
        " ".repeat(columns as usize)
    }
}

/// Indent Block: `text` re-indented so its block starts at column `target`, lines joined with
/// `line_break`. A first line copied with its indentation sets the block's base; one copied
/// from mid-line does not, so the lines after it do, and a block opener copied from mid-line
/// (`if x:`, `fn f() {`) sat one level (`unit_columns`) above its body. Blank lines lose their
/// whitespace; a single line comes back as it is.
pub fn indent_pasted_text(
    text: &str,
    target: u32,
    use_tabs: bool,
    tab_size: u32,
    unit_columns: u32,
    line_break: &str,
) -> String {
    let lines: Vec<&str> = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)).collect();
    let Some((first, rest)) = lines.split_first().filter(|(_, rest)| !rest.is_empty()) else {
        return text.to_owned();
    };
    let filled: Vec<&str> = rest.iter().copied().filter(|line| !line.trim().is_empty()).collect();
    if filled.is_empty() {
        return text.to_owned();
    }
    let first_indented = first.starts_with([' ', '\t']);
    let measured = filled.iter().chain(first_indented.then_some(first));
    let mut base = measured.map(|line| leading_columns(line, tab_size)).min().unwrap_or(0);
    if !first_indented && base > 0 && first.trim_end().ends_with([':', '{', '(', '[']) {
        base = base.saturating_sub(unit_columns);
    }
    let delta = i64::from(target) - i64::from(base);
    let mut out = first.trim_start().to_owned();
    for line in rest {
        out.push_str(line_break);
        if line.trim().is_empty() {
            continue;
        }
        let columns = (i64::from(leading_columns(line, tab_size)) + delta).max(0) as u32;
        out.push_str(&padding(columns, use_tabs, tab_size));
        out.push_str(line.trim_start());
    }
    out
}

/// A selection range of the text after the change `pasted` (made to `before`), through the
/// inverse of `pasted` back to `before` and then through the change `formatted`, carets moving
/// after text inserted at them.
fn remap(range: Selection, pasted: &TextChange, before: &EditState, formatted: &TextChange) -> Selection {
    let inverse = change(pasted.from, pasted.from + utf16_len(&pasted.insert), before.slice(pasted.from, pasted.to));
    let original = map_selection(range, std::slice::from_ref(&inverse), Assoc::After);
    map_selection(original, std::slice::from_ref(formatted), Assoc::After)
}

/// The paste `paste` (one change and the selection after it, applied to `state`) reformatted as
/// `mode` says, as one edit of `state`. The paste comes back unchanged when it is not one change,
/// inserts only whitespace, or needs no reformatting.
///
/// Carets keep their place on the text around the paste: a line-wise paste above the caret's
/// line leaves the caret on its line.
pub fn reindent_paste(state: &EditState, paste: &Edit, mode: PasteReindent, indenter: Option<Indenter<'_>>) -> Edit {
    let [pasted] = paste.changes.as_slice() else {
        return paste.clone();
    };
    if mode == PasteReindent::None || pasted.insert.trim().is_empty() {
        return paste.clone();
    }
    let line = state.line_at(pasted.from);
    let before = state.slice(line.from, pasted.from);
    let at_indent = before.trim().is_empty();
    let tab_size = state.tab_size;
    let unit_columns = match state.indent_unit_columns() {
        0 => tab_size,
        columns => columns,
    };
    let mut target = leading_columns(if at_indent { &before } else { &line.text }, tab_size);
    let mut prefix = String::new();
    let mut from = pasted.from;
    if at_indent {
        let proper = indentation(state, indenter, line.from).filter(|proper| *proper != target);
        if let Some(proper) = proper {
            target = proper;
            prefix = state.indent_string(proper);
            from = line.from;
        }
    }
    let block =
        indent_pasted_text(&pasted.insert, target, state.uses_tabs(), tab_size, unit_columns, &state.line_break);
    let out = if prefix.is_empty() { block } else { prefix + block.trim_start() };
    let end = from + utf16_len(&out);
    let unchanged = out == pasted.insert && from == pasted.from;
    let formatted = change(from, pasted.to, out);
    let ranges: Vec<Selection> =
        paste.selections.iter().map(|range| remap(*range, pasted, state, &formatted)).collect();
    if mode != PasteReindent::IndentEachLine {
        return if unchanged { paste.clone() } else { Edit::new(vec![formatted], ranges, paste.main) };
    }
    let first = Edit::new(vec![formatted], ranges, paste.main);
    reindent_pasted_lines(state, &first, from, end, at_indent, indenter).unwrap_or(first)
}

/// Indent Each Line after `first` (the block pasted at `from..end` of the text after it): every
/// line whose start lies in the pasted text, plus the paste line when the paste was at its
/// indentation, gets the language's indentation (blank lines none), each line seeing the lines
/// above as already reindented. Answers `first` and the reindentation as one replacement of
/// the lines of the paste; `None` when no line changes.
fn reindent_pasted_lines(
    state: &EditState,
    first: &Edit,
    from: u32,
    end: u32,
    at_indent: bool,
    indenter: Option<Indenter<'_>>,
) -> Option<Edit> {
    let pasted = apply(state, first);
    let first_line = pasted.line_at(from);
    let line_start = if at_indent { first_line.from } else { pasted.next_line_start(&first_line)? };
    let last_line = pasted.line_at(end);
    let line_end =
        if last_line.from == end && end > from { pasted.previous_line_end(&last_line).unwrap_or(end) } else { end };
    if line_start > line_end {
        return None;
    }
    let mut working = pasted.clone();
    let mut ranges = first.selections.clone();
    let mut changed = false;
    for number in pasted.line_at(line_start).number..=pasted.line_at(line_end).number {
        let line = working.line(number);
        let Some(indent) = indentation(&working, indenter, line.from) else {
            continue;
        };
        let indent = if line.text.trim().is_empty() { 0 } else { indent };
        let current = leading_space(&line.text);
        let normal = working.indent_string(indent);
        if current == normal {
            continue;
        }
        let reindent = change(line.from, line.from + utf16_len(current), normal);
        let changes = std::slice::from_ref(&reindent);
        ranges = ranges.iter().map(|range| map_selection(*range, changes, Assoc::After)).collect();
        working.rope = apply_changes(&working.rope, changes);
        changed = true;
    }
    if !changed {
        return None;
    }
    // Only the lines of the paste changed: from the start of the paste line to the end of the
    // line the paste ended in.
    let span_from = state.line_at(first.changes[0].from).from;
    let span_to = state.line_at(first.changes[0].to).to;
    let growth = i64::from(working.doc_len()) - i64::from(state.doc_len());
    let new_to = (i64::from(span_to) + growth).max(i64::from(span_from)) as u32;
    let replacement = change(span_from, span_to, working.slice(span_from, new_to));
    Some(Edit::new(vec![replacement], ranges, first.main))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::testing::state;
    use crate::editor_commands::{IndentContext, SelectionExt};

    /// One unit deeper after a line ending in `{`.
    fn brace_indent(cx: &IndentContext<'_>) -> Option<u32> {
        let line = cx.state.line_at(cx.pos);
        if line.number == 0 {
            return Some(0);
        }
        let previous = cx.state.line(line.number - 1);
        let base = cx.line_indent(previous.from, 1);
        Some(if previous.text.trim_end().ends_with('{') { base + 2 } else { base })
    }

    /// The text after pasting `text` over the main selection of `marked` with `mode`.
    fn paste(marked: &str, text: &str, mode: PasteReindent) -> String {
        let before = state(marked);
        let main = before.main();
        let raw = Edit::following(&before, vec![change(main.start(), main.end(), text)], Assoc::Before);
        let indenter: Indenter<'_> = &brace_indent;
        apply(&before, &reindent_paste(&before, &raw, mode, Some(indenter))).text()
    }

    #[test]
    fn indents_each_pasted_line_ideas_default() {
        assert_eq!(
            paste("if {\n|\n}", "a();\n      b();\n  c();", PasteReindent::IndentEachLine),
            "if {\n  a();\n  b();\n  c();\n}"
        );
    }

    #[test]
    fn starts_a_block_pasted_at_a_lines_indentation_at_the_proper_indent() {
        assert_eq!(
            paste("if {\n|\n}", "a();\n    b();\n  c();", PasteReindent::IndentBlock),
            "if {\n  a();\n    b();\n  c();\n}"
        );
    }

    #[test]
    fn re_indents_a_pasted_block_to_the_caret_column() {
        let block = |text: &str, target: u32| indent_pasted_text(text, target, false, 4, 4, "\n");
        assert_eq!(block("if x:\n    a()\n    b()", 4), "if x:\n        a()\n        b()");
        assert_eq!(block("    a()\n    b()", 0), "a()\nb()");
        assert_eq!(block("one line", 8), "one line");
    }
}
