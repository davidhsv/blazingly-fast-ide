//! Tab, smart Backspace, Tab out of the brackets and quotes the editor inserted, and stepping
//! over the closing quote of a string literal.
//!
//! The pairs the editor inserted are a plain value ([`PairScope`]s): the caller keeps them,
//! passes every edit through [`track_pair_scopes`] and hands them to [`insert_tab`].

use super::tree::{SyntaxTree, pascal_name};
use super::visible_column::visible_column;
use super::{
    Edit, EditState, Indenter, RangeEdit, SelectionExt, caret, change, change_by_range, indentation, insertion,
    leading_blank, map_after, map_before, utf16_len,
};
use crate::editor::buffer::byte_of_col;
use ide_core::TextChange;

// --- Tab -------------------------------------------------------------------------------

/// Tab: steps out of a pair the editor inserted when every caret is at the end of one; with a
/// selection holding more than whitespace, indents the selected lines one unit; otherwise
/// replaces each range with whitespace up to the next indent stop (a tab character when the
/// indent unit is a tab).
///
/// Answers the edit and the pair scopes after it.
pub fn insert_tab(state: &EditState, scopes: &[PairScope]) -> (Edit, Vec<PairScope>) {
    if let Some(out) = tab_out_of_scope(state, scopes) {
        return out;
    }
    let selects_text = state
        .selections
        .iter()
        .any(|range| !range.is_caret() && !state.slice(range.start(), range.end()).trim().is_empty());
    let edit = if selects_text { indent_lines(state) } else { insert_whitespace(state) };
    let scopes = track_pair_scopes(scopes, state, &edit, false);
    (edit, scopes)
}

/// One indent unit at the start of every line a range touches; a line a range only ends at the
/// start of is left alone, and a line two ranges share is indented once.
fn indent_lines(state: &EditState) -> Edit {
    let mut at_line: Option<u32> = None;
    change_by_range(state, |_, range| {
        let mut changes = Vec::new();
        let mut pos = range.start();
        loop {
            let line = state.line_at(pos);
            if at_line.is_none_or(|done| line.number > done) && (range.is_caret() || range.end() > line.from) {
                changes.push(insertion(line.from, state.indent_unit.clone()));
                at_line = Some(line.number);
            }
            match state.next_line_start(&line) {
                Some(next) if next <= range.end() => pos = next,
                _ => break,
            }
        }
        RangeEdit { range: (map_after(range.anchor(), &changes), map_after(range.head(), &changes)), changes }
    })
}

/// Each range replaced by whitespace up to the next indent stop, counted in visible columns.
fn insert_whitespace(state: &EditState) -> Edit {
    let use_tab = state.indent_unit == "\t";
    let size = state.indent_unit_columns().max(1);
    change_by_range(state, |_, range| {
        let from = range.start();
        let line = state.line_at(from);
        let column = visible_column(&line.text, from.saturating_sub(line.from), state.tab_size);
        let insert = if use_tab { "\t".to_owned() } else { " ".repeat((size - column % size) as usize) };
        let end = from + utf16_len(&insert);
        RangeEdit { changes: vec![change(from, range.end(), insert)], range: caret(end) }
    })
}

// --- Backspace -------------------------------------------------------------------------------

/// Languages whose layout is indentation: Backspace keeps going to the nearest indent position.
const NEAREST_INDENT_LANGUAGES: [&str; 4] = ["python", "gdscript", "yaml", "shellscript"];

/// Smart Backspace runs in the "to proper indent position" mode (`autoindent`), except in
/// languages whose layout is indentation.
pub fn smart_backspace_applies(mode: &str, language: &str) -> bool {
    mode == "autoindent" && !NEAREST_INDENT_LANGUAGES.contains(&language)
}

/// The change one Backspace makes at `pos` in "to proper indent position" mode; `None` to delete
/// normally (text before the caret on its line, or no indentation known for the line).
///
/// Deeper than the proper indentation, the whitespace goes back to it. At it, the line joins the
/// line above: after a blank line the text moves up at its indentation, else it follows the text
/// above with one space (none after an opening bracket or before a closing one or punctuation).
pub fn auto_indent_backspace(state: &EditState, indenter: Option<Indenter<'_>>, pos: u32) -> Option<TextChange> {
    let line = state.line_at(pos);
    let split = byte_of_col(&line.text, pos.saturating_sub(line.from));
    let (before, rest) = line.text.split_at(split);
    if before.contains(|c: char| c != ' ' && c != '\t') {
        return None;
    }
    let target = indentation(state, indenter, line.from)?;
    let blank = leading_blank(rest);
    let end = pos + utf16_len(blank);
    let current = state.count_column(&line.text[..split + blank.len()]);
    if current > target {
        return Some(change(line.from, end, state.indent_string(target)));
    }
    if line.number == 0 {
        return (end > 0).then(|| change(0, end, ""));
    }
    let previous = state.line(line.number - 1);
    let text_above = previous.text.trim_end();
    let previous_end = previous.from + utf16_len(text_above);
    if previous_end == previous.from && previous_end != 0 {
        return Some(change(previous_end, end, state.indent_string(target)));
    }
    let after = &rest[blank.len()..];
    let tight = after.is_empty()
        || text_above.is_empty()
        || text_above.ends_with(['(', '[', '{'])
        || after.starts_with([')', ']', '}', '.', ',', ';']);
    Some(change(previous_end, end, if tight { "" } else { " " }))
}

/// Backspace in leading whitespace at every caret: to the proper indentation, or joined with the
/// line above when already there. `None` with a selection, or when one caret deletes normally.
pub fn smart_backspace(state: &EditState, indenter: Option<Indenter<'_>>) -> Option<Edit> {
    if state.selections.iter().any(|range| !range.is_caret()) {
        return None;
    }
    let plans: Vec<TextChange> = state
        .selections
        .iter()
        .map(|range| auto_indent_backspace(state, indenter, range.head()))
        .collect::<Option<_>>()?;
    Some(change_by_range(state, |index, _| {
        let plan = plans[index].clone();
        let at = plan.from + utf16_len(&plan.insert);
        RangeEdit { changes: vec![plan], range: caret(at) }
    }))
}

// --- Tab out of brackets / quotes ------------------------------------------------------------

/// The inside of a bracket or quote pair the editor inserted itself; Tab with the caret at `to`
/// steps over the closing character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairScope {
    pub from: u32,
    pub to: u32,
}

/// The closing character of a pair the editor completes.
fn closer(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' | '\'' | '`' => Some(open),
        _ => None,
    }
}

/// The pair scopes after `edit` (applied to `before`). A scope survives a change outside it only
/// while the caret was inside it and the change does not touch it; the survivors follow the
/// changes, growing with text typed at their end. When `typed` (the user typed, and the editor
/// may have completed a pair), every pair inserted as one two-character change with a caret
/// between its characters opens a new scope.
pub fn track_pair_scopes(scopes: &[PairScope], before: &EditState, edit: &Edit, typed: bool) -> Vec<PairScope> {
    if edit.changes.is_empty() {
        return scopes.to_vec();
    }
    let caret_at = before.main().head();
    let survives = |scope: &PairScope| {
        edit.changes.iter().all(|c| {
            let outside = c.from < scope.from || c.to > scope.to;
            let caret_outside = caret_at < scope.from || caret_at > scope.to;
            !(outside && (caret_outside || (c.to >= scope.from && c.from <= scope.to)))
        })
    };
    let mut next: Vec<PairScope> = scopes
        .iter()
        .filter(|scope| survives(scope))
        .map(|scope| PairScope { from: map_before(scope.from, &edit.changes), to: map_after(scope.to, &edit.changes) })
        .collect();
    if typed {
        next.extend(inserted_pairs(edit));
    }
    next
}

/// Empty scopes between the pairs `edit` inserted with a caret between their two characters.
fn inserted_pairs(edit: &Edit) -> Vec<PairScope> {
    let mut out = Vec::new();
    let mut delta: i64 = 0;
    for c in &edit.changes {
        let from_after = i64::from(c.from) + delta;
        delta += i64::from(utf16_len(&c.insert)) - i64::from(c.to - c.from);
        let mut chars = c.insert.chars();
        let (Some(open), Some(close), None) = (chars.next(), chars.next(), chars.next()) else {
            continue;
        };
        if utf16_len(&c.insert) != 2 || closer(open) != Some(close) {
            continue;
        }
        let inside = (from_after + 1).max(0) as u32;
        if edit.selections.iter().any(|range| range.is_caret() && range.head() == inside) {
            out.push(PairScope { from: inside, to: inside });
        }
    }
    out
}

/// Tab with every caret at the end of a pair scope: each caret steps over the closing
/// character, and the scopes it left are dropped. `None` when a range is a selection or a caret
/// is not at a scope's end. Answers the selection change and the remaining scopes.
pub fn tab_out_of_scope(state: &EditState, scopes: &[PairScope]) -> Option<(Edit, Vec<PairScope>)> {
    let heads: Vec<u32> = state.selections.iter().map(SelectionExt::head).collect();
    let all_at_ends =
        state.selections.iter().all(|range| range.is_caret() && scopes.iter().any(|scope| scope.to == range.head()));
    if scopes.is_empty() || !all_at_ends {
        return None;
    }
    let len = state.doc_len();
    let ranges = heads.iter().map(|head| caret((head + 1).min(len))).collect();
    let remaining = scopes.iter().filter(|scope| !heads.contains(&scope.to)).copied().collect();
    Some((Edit::select(ranges, state.main), remaining))
}

// --- closing quote ---------------------------------------------------------------------------

/// The node names of string-like literals: strings, characters, templates, Go runes.
fn is_string_like(name: &str) -> bool {
    let name = pascal_name(name);
    ["String", "Char", "Template", "Rune"].iter().any(|word| name.contains(word))
}

/// `quote` at `pos` closes the string literal around the caret (a literal that ends right after
/// it and starts before `pos`).
pub fn closes_string_at(state: &EditState, tree: &SyntaxTree, pos: u32, quote: char) -> bool {
    if !state.slice(pos, pos.saturating_add(1)).chars().eq(std::iter::once(quote)) {
        return false;
    }
    let Some(inner) = tree.resolve_inner(pos, 1) else {
        return false;
    };
    for index in tree.ancestors(inner) {
        let node = tree.node(index);
        if node.to > pos + 1 {
            break;
        }
        if node.to == pos + 1 && node.from < pos && is_string_like(&node.name) {
            return true;
        }
    }
    false
}

/// Typing a quote right before the closing quote of a string literal steps over it: every
/// caret moves past the quote. `None` unless `text` is one quote and every range is a caret
/// before the closing quote of its literal.
pub fn skip_closing_quote(state: &EditState, tree: &SyntaxTree, text: &str) -> Option<Edit> {
    let mut chars = text.chars();
    let (Some(quote), None) = (chars.next(), chars.next()) else {
        return None;
    };
    if !matches!(quote, '"' | '\'' | '`') {
        return None;
    }
    let closes =
        state.selections.iter().all(|range| range.is_caret() && closes_string_at(state, tree, range.head(), quote));
    closes.then(|| Edit::select(state.selections.iter().map(|range| caret(range.head() + 1)).collect(), state.main))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::buffer::Assoc;
    use crate::editor_commands::testing::{show, state, styled};
    use crate::editor_commands::tree::TreeNode;
    use crate::editor_commands::{IndentContext, apply};

    /// One unit (2) deeper after a non-blank line ending in `{`, one less on a line starting with
    /// `}`.
    fn braces(cx: &IndentContext<'_>) -> Option<u32> {
        let (_, mut from) = cx.line_at(cx.pos, -1);
        let above = loop {
            if from == 0 {
                return Some(0);
            }
            let (text, start) = cx.line_at(from - 1, -1);
            from = start;
            if !text.trim().is_empty() {
                break text;
            }
        };
        let base = (above.len() - above.trim_start_matches(' ').len()) as i64;
        let own = cx.state.line_at(cx.pos).text;
        let opens = if above.trim_end().ends_with('{') { 2 } else { 0 };
        let closes = if own.trim_start().starts_with('}') { 2 } else { 0 };
        Some((base + opens - closes).max(0) as u32)
    }

    fn tab(marked: &str, unit: &str) -> String {
        let before = styled(marked, 4, unit);
        let (edit, _) = insert_tab(&before, &[]);
        show(&apply(&before, &edit))
    }

    fn backspace(marked: &str) -> Option<String> {
        let before = state(marked);
        let indenter: Indenter<'_> = &braces;
        smart_backspace(&before, Some(indenter)).map(|edit| show(&apply(&before, &edit)))
    }

    /// Types `text` at every range as the editor does with bracket completion on: an opening
    /// bracket at a caret before nothing or whitespace inserts the pair, anything else replaces
    /// the selection.
    fn type_text(before: &EditState, scopes: &[PairScope], text: &str) -> (EditState, Vec<PairScope>) {
        let pair = text.chars().next().filter(|open| text.len() == 1 && "([{".contains(*open)).and_then(closer);
        let edit = change_by_range(before, |_, range| match pair {
            Some(close) if range.is_caret() && before.slice(range.head(), range.head() + 1).trim().is_empty() => {
                RangeEdit {
                    changes: vec![insertion(range.head(), format!("{text}{close}"))],
                    range: caret(range.head() + 1),
                }
            }
            _ => RangeEdit {
                changes: vec![change(range.start(), range.end(), text)],
                range: caret(range.start() + utf16_len(text)),
            },
        });
        let scopes = track_pair_scopes(scopes, before, &edit, true);
        (apply(before, &edit), scopes)
    }

    /// Runs `edit` as an edit that is not typing.
    fn dispatch(before: &EditState, scopes: &[PairScope], edit: Edit) -> (EditState, Vec<PairScope>) {
        let scopes = track_pair_scopes(scopes, before, &edit, false);
        (apply(before, &edit), scopes)
    }

    #[test]
    fn inserts_whitespace_up_to_the_next_indent_stop_at_each_caret() {
        assert_eq!(tab("ab|c\n|x", "  "), "ab  |c\n  |x");
    }

    #[test]
    fn counts_wide_characters_as_two_columns_towards_the_indent_stop() {
        assert_eq!(tab("a张|x\n张|y", "  "), "a张 |x\n张  |y");
    }

    #[test]
    fn inserts_a_tab_character_when_the_code_style_uses_tabs() {
        assert_eq!(tab("a|b", "\t"), "a\t|b");
    }

    #[test]
    fn indents_the_lines_of_a_selection() {
        let before = state("[a\nb]");
        let (edit, _) = insert_tab(&before, &[]);
        assert_eq!(apply(&before, &edit).text(), "  a\n  b");
    }

    #[test]
    fn replaces_a_whitespace_only_selection_like_a_caret() {
        assert_eq!(tab("a[  ]b", "  "), "a |b");
    }

    #[test]
    fn applies_in_proper_indent_mode_except_in_indentation_based_languages() {
        assert!(smart_backspace_applies("autoindent", "java"));
        assert!(!smart_backspace_applies("autoindent", "python"));
        assert!(!smart_backspace_applies("autoindent", "gdscript"));
        assert!(!smart_backspace_applies("nearest", "java"));
    }

    #[test]
    fn goes_back_to_the_proper_indent_from_deeper_whitespace() {
        assert_eq!(backspace("f {\n        |x\n}").as_deref(), Some("f {\n  |x\n}"));
    }

    #[test]
    fn joins_with_the_line_above_at_the_proper_indent() {
        assert_eq!(backspace("f {\n  a\n  |b\n}").as_deref(), Some("f {\n  a |b\n}"));
    }

    #[test]
    fn moves_text_up_over_a_blank_line_at_its_proper_indent() {
        assert_eq!(backspace("f {\n  a\n\n  |b\n}").as_deref(), Some("f {\n  a\n  |b\n}"));
    }

    #[test]
    fn joins_an_empty_line_without_leaving_a_space() {
        assert_eq!(backspace("f {\n  a\n  |\n}").as_deref(), Some("f {\n  a|\n}"));
    }

    #[test]
    fn leaves_backspace_after_text_to_the_default() {
        let before = state("f {\n  a|b\n}");
        let indenter: Indenter<'_> = &braces;
        assert_eq!(auto_indent_backspace(&before, Some(indenter), before.main().head()), None);
        assert_eq!(backspace("f {\n  a|b\n}"), None);
    }

    #[test]
    fn steps_over_a_closing_bracket_the_editor_inserted() {
        let (typed, scopes) = type_text(&state("f|"), &[], "(");
        let (typed, scopes) = type_text(&typed, &scopes, "x");
        assert_eq!(show(&typed), "f(x|)");
        assert!(tab_out_of_scope(&typed, &scopes).is_some());
        let (edit, scopes) = insert_tab(&typed, &scopes);
        let out = apply(&typed, &edit);
        assert_eq!(show(&out), "f(x)|");
        // Once out, Tab inserts whitespace again.
        let (edit, _) = insert_tab(&out, &scopes);
        assert_eq!(show(&apply(&out, &edit)), "f(x)  |");
    }

    #[test]
    fn ignores_closers_the_user_typed() {
        assert_eq!(tab_out_of_scope(&state("f(x|)"), &[]), None);
    }

    #[test]
    fn keeps_the_pair_through_edits_elsewhere_while_the_caret_is_inside_else_forgets_it() {
        let (typed, scopes) = type_text(&state("a f|"), &[], "(");
        let replace = Edit::following(&typed, vec![change(0, 1, "bb")], Assoc::Before);
        let (edited, scopes) = dispatch(&typed, &scopes, replace);
        assert_eq!(show(&edited), "bb f(|)");
        let (moved, scopes) = dispatch(&edited, &scopes, Edit::select(vec![caret(0)], 0));
        let (edited, scopes) = dispatch(&moved, &scopes, Edit::new(vec![insertion(0, "c")], vec![caret(6)], 0));
        assert_eq!(tab_out_of_scope(&edited, &scopes), None);
    }

    #[test]
    fn steps_over_the_closing_quote_of_an_existing_string_literal() {
        // f("ab");
        let tree = SyntaxTree::new(vec![
            TreeNode::new("Script", 0, 8),
            TreeNode::new("ExpressionStatement", 0, 8),
            TreeNode::new("CallExpression", 0, 7),
            TreeNode::new("VariableName", 0, 1),
            TreeNode::new("ArgList", 1, 7),
            TreeNode::new("(", 1, 2),
            TreeNode::new("String", 2, 6),
            TreeNode::new(")", 6, 7),
        ]);
        let before = state("f(\"ab|\");");
        assert!(closes_string_at(&before, &tree, before.main().head(), '"'));
        assert!(!closes_string_at(&before, &tree, 2, '"'));
        let edit = skip_closing_quote(&before, &tree, "\"");
        assert!(edit.is_some());
        assert_eq!(show(&crate::editor_commands::testing::run(&before, edit)), "f(\"ab\"|);");
    }
}
