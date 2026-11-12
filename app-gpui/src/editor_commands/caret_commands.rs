//! Caret commands of the IDE's editor: Split Line, code block navigation and Move Caret to
//! Matching Brace.

use super::move_element::is_code;
use super::tree::SyntaxTree;
use super::{
    Edit, EditState, IndentContext, Indenter, RangeEdit, SelectionExt, caret, change, change_by_range, leading_space,
    map_before, utf16_len,
};

const OPEN_BRACE: u16 = b'{' as u16;
const CLOSE_BRACE: u16 = b'}' as u16;
/// Bracket pairs, an opening bracket at an even index and its closing one after it.
const BRACKETS: [u16; 6] = [b'(' as u16, b')' as u16, b'[' as u16, b']' as u16, b'{' as u16, b'}' as u16];
/// How far bracket matching looks for the other bracket.
const MAX_SCAN_DISTANCE: u32 = 10_000;

fn is_space_unit(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
}

/// Both sides of `pos` are an empty bracket pair: Enter there opens it onto three lines.
fn between_brackets(units: &[u16], pos: u32) -> bool {
    let Some(before) = pos.checked_sub(1).and_then(|at| units.get(at as usize)) else {
        return false;
    };
    let Some(after) = units.get(pos as usize) else {
        return false;
    };
    BRACKETS.chunks(2).any(|pair| pair[0] == *before && pair[1] == *after)
}

/// Enter with indentation at every selection range: the range is replaced by a line break and
/// the new line indented as `indenter` says (else as the current line), spaces after the caret
/// dropped; between an empty bracket pair the closing bracket goes to a line of its own.
fn newline_and_indent(state: &EditState, indenter: Option<Indenter<'_>>) -> Edit {
    let units = state.units();
    let line_break = state.line_break.as_str();
    change_by_range(state, |_, range| {
        let (mut from, mut to) = (range.start(), range.end());
        let line = state.line_at(from);
        let explode = from == to && between_brackets(&units, from);
        let context =
            IndentContext { simulated_break: Some(from), double_break: explode, ..IndentContext::new(state, from) };
        let indent = indenter
            .and_then(|indenter| indenter(&context))
            .unwrap_or_else(|| state.count_column(leading_space(&line.text)));
        while to < line.to && units.get(to as usize).is_some_and(|unit| is_space_unit(*unit)) {
            to += 1;
        }
        if explode {
            to = from;
        } else if from > line.from && from < line.from + 100 {
            // Blank text before the caret: the new line replaces it. The test reads the line's
            // first `from` code units (the caret's document offset, not its column), so past the
            // start of a document it also asks for blank text after the caret.
            let prefix_end = (line.from + from).min(line.to) as usize;
            if units[line.from as usize..prefix_end].iter().all(|unit| is_space_unit(*unit)) {
                from = line.from;
            }
        }
        let indentation = state.indent_string(indent);
        let mut insert = format!("{line_break}{indentation}");
        if explode {
            insert.push_str(line_break);
            insert.push_str(&state.indent_string(context.line_indent(line.from, -1)));
        }
        let head = from + utf16_len(line_break) + utf16_len(&indentation);
        RangeEdit { changes: vec![change(from, to, insert)], range: caret(head) }
    })
}

/// Split Line: breaks the line at each caret, indented as Enter does, and leaves the caret before
/// the break.
pub fn split_line(state: &EditState, indenter: Option<Indenter<'_>>) -> Option<Edit> {
    let changes = newline_and_indent(state, indenter).changes;
    let ranges = state.selections.iter().map(|range| caret(map_before(range.start(), &changes))).collect();
    Some(Edit::new(changes, ranges, state.main))
}

/// The `{` or `}` at `pos` when it is code (not inside a string or a comment, as far as the syntax
/// tree knows).
fn brace_at(units: &[u16], tree: &SyntaxTree, pos: u32) -> Option<u16> {
    let unit = *units.get(pos as usize)?;
    ((unit == OPEN_BRACE || unit == CLOSE_BRACE) && is_code(tree, pos)).then_some(unit)
}

fn block_start_in(units: &[u16], tree: &SyntaxTree, pos: u32) -> Option<u32> {
    let from = pos.checked_sub(1)?;
    let after_close = brace_at(units, tree, from) == Some(CLOSE_BRACE);
    let mut depth: i32 = if after_close { -1 } else { 0 };
    for p in (0..=from).rev() {
        match brace_at(units, tree, p) {
            Some(OPEN_BRACE) if p != from => {
                if depth == 0 {
                    return Some(if after_close { p } else { p + 1 });
                }
                depth -= 1;
            }
            Some(CLOSE_BRACE) => depth += 1,
            _ => {}
        }
    }
    None
}

fn block_end_in(units: &[u16], tree: &SyntaxTree, pos: u32) -> Option<u32> {
    let before_open = brace_at(units, tree, pos) == Some(OPEN_BRACE);
    let mut depth: i32 = if before_open { -1 } else { 0 };
    for p in pos..units.len() as u32 {
        match brace_at(units, tree, p) {
            Some(CLOSE_BRACE) if p != pos => {
                if depth == 0 {
                    return Some(if before_open { p + 1 } else { p });
                }
                depth -= 1;
            }
            Some(OPEN_BRACE) => depth += 1,
            _ => {}
        }
    }
    None
}

/// Where Move Caret to Code Block Start goes from `pos`: just after the enclosing `{`, or before
/// the `{` matching a `}` the caret follows. `None` outside any block.
pub fn code_block_start(state: &EditState, tree: &SyntaxTree, pos: u32) -> Option<u32> {
    block_start_in(&state.units(), tree, pos)
}

/// Where Move Caret to Code Block End goes from `pos`: before the enclosing `}`, or after the `}`
/// matching a `{` the caret precedes. `None` outside any block.
pub fn code_block_end(state: &EditState, tree: &SyntaxTree, pos: u32) -> Option<u32> {
    block_end_in(&state.units(), tree, pos)
}

/// The inside of the `{…}` block around `pos` (between the braces), for Collapse Block; after a
/// `}` the block it closes. `None` outside any block.
pub fn enclosing_code_block(state: &EditState, tree: &SyntaxTree, pos: u32) -> Option<(u32, u32)> {
    let units = state.units();
    let start = block_start_in(&units, tree, pos)?;
    let open = if brace_at(&units, tree, start) == Some(OPEN_BRACE) { start } else { start.checked_sub(1)? };
    let end = block_end_in(&units, tree, open)?;
    Some((open + 1, end.saturating_sub(1)))
}

/// One caret at the target of `find` from the main head (or a selection from the main anchor to
/// it when `extend`).
fn move_to_block(
    state: &EditState,
    tree: &SyntaxTree,
    extend: bool,
    find: fn(&EditState, &SyntaxTree, u32) -> Option<u32>,
) -> Option<Edit> {
    let main = state.main();
    let target = find(state, tree, main.head())?;
    Some(Edit::select(vec![if extend { (main.anchor(), target) } else { caret(target) }], 0))
}

/// Move Caret to Code Block Start.
pub fn cursor_code_block_start(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_to_block(state, tree, false, code_block_start)
}

/// Move Caret to Code Block End.
pub fn cursor_code_block_end(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_to_block(state, tree, false, code_block_end)
}

/// Move Caret to Code Block Start with Selection.
pub fn select_code_block_start(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_to_block(state, tree, true, code_block_start)
}

/// Move Caret to Code Block End with Selection.
pub fn select_code_block_end(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_to_block(state, tree, true, code_block_end)
}

/// A bracket found next to a position and the bracket it pairs with, if the scan reached one.
struct BracketMatch {
    start: (u32, u32),
    end: Option<(u32, u32)>,
}

/// The name of the innermost node at `pos` (`None` in an unparsed text): brackets pair only with
/// brackets of the same token type.
fn token_at(tree: &SyntaxTree, pos: u32, side: i8) -> Option<&str> {
    tree.resolve_inner(pos, side).map(|node| tree.name(node))
}

/// The bracket before (`forward` false) or after `pos` and the one it pairs with, scanning over
/// nested pairs. `None` when there is no bracket facing that way, or the scan gives up.
fn match_brackets(units: &[u16], tree: &SyntaxTree, pos: u32, forward: bool) -> Option<BracketMatch> {
    let len = units.len() as u32;
    let start_at = if forward { (pos < len).then_some(pos)? } else { pos.checked_sub(1)? };
    let bracket = BRACKETS.iter().position(|unit| *unit == units[start_at as usize])?;
    if (bracket % 2 == 0) != forward {
        return None;
    }
    let start = (start_at, start_at + 1);
    let token = token_at(tree, pos, if forward { 1 } else { -1 });
    let mut depth: i32 = 0;
    let scanned: Box<dyn Iterator<Item = u32>> = if forward { Box::new(pos..len) } else { Box::new((0..pos).rev()) };
    for (distance, p) in scanned.enumerate() {
        if distance as u32 > MAX_SCAN_DISTANCE {
            return None;
        }
        let Some(found) = BRACKETS.iter().position(|unit| *unit == units[p as usize]) else {
            continue;
        };
        if token_at(tree, p, 1) != token {
            continue;
        }
        if (found % 2 == 0) == forward {
            depth += 1;
        } else if depth == 1 {
            return Some(BracketMatch { start, end: Some((p, p + 1)) });
        } else {
            depth -= 1;
        }
    }
    Some(BracketMatch { start, end: None })
}

/// Where Move Caret to Matching Brace goes from `pos`: from a bracket next to the caret to the
/// one it pairs with; elsewhere to the unclosed opening bracket before the caret.
pub fn match_brace_target(state: &EditState, tree: &SyntaxTree, pos: u32) -> Option<u32> {
    let units = state.units();
    let len = units.len() as u32;
    let found = match_brackets(&units, tree, pos, false)
        .or_else(|| match_brackets(&units, tree, pos, true))
        .or_else(|| pos.checked_sub(1).and_then(|before| match_brackets(&units, tree, before, true)))
        .or_else(|| (pos < len).then(|| match_brackets(&units, tree, pos + 1, false)).flatten());
    if let Some(BracketMatch { start, end: Some(end) }) = found {
        return Some(if start.0 == pos { end.1 } else { end.0 });
    }
    let mut depth = 0u32;
    for p in (0..pos.min(len)).rev() {
        let Some(found) = BRACKETS.iter().position(|unit| *unit == units[p as usize]) else {
            continue;
        };
        if !is_code(tree, p) {
            continue;
        }
        if found % 2 == 1 {
            depth += 1;
        } else if depth == 0 {
            return Some(p);
        } else {
            depth -= 1;
        }
    }
    None
}

/// Move Caret to Matching Brace, for every caret; `None` when no caret moves.
pub fn cursor_match_brace(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    let mut moved = false;
    let ranges: Vec<_> = state
        .selections
        .iter()
        .map(|range| match match_brace_target(state, tree, range.head()) {
            Some(target) => {
                moved = true;
                caret(target)
            }
            None => *range,
        })
        .collect();
    moved.then(|| Edit::select(ranges, state.main))
}

#[cfg(test)]
mod tests {
    use super::super::testing::{shown, state, state_carets};
    use super::super::tree::TreeNode;
    use super::*;
    use crate::editor::buffer::byte_of_col;

    /// One unit deeper after a line ending in `{`.
    fn brace_indent(context: &IndentContext<'_>) -> Option<u32> {
        let (text, from) = context.line_at(context.pos, -1);
        let base = (text.len() - text.trim_start_matches(' ').len()) as u32;
        let before = &text[..byte_of_col(&text, context.pos.saturating_sub(from))];
        Some(if before.trim_end().ends_with('{') { base + 2 } else { base })
    }

    fn split(marked: &str) -> String {
        let before = state(marked);
        let indenter: Indenter<'_> = &brace_indent;
        shown(&before, split_line(&before, Some(indenter)))
    }

    #[test]
    fn breaks_the_line_at_each_caret_and_leaves_the_caret_before_the_break() {
        assert_eq!(split("if (a) {|return 1; }"), "if (a) {|\n  return 1; }");
        assert_eq!(split("a|b\nc|d"), "a|\nb\nc|\nd");
    }

    /// The text without its caret mark, and the caret's offset.
    fn at(marked: &str) -> (EditState, u32) {
        let parsed = state_carets(marked);
        let pos = parsed.main().head();
        (parsed, pos)
    }

    #[test]
    fn goes_to_just_inside_the_enclosing_braces() {
        let (state, pos) = at("f() { if (x) { a|(); } b(); }");
        let tree = SyntaxTree::default();
        assert_eq!(code_block_start(&state, &tree, pos), Some(utf16_len("f() { if (x) {")));
        assert_eq!(code_block_end(&state, &tree, pos), Some(utf16_len("f() { if (x) { a(); ")));
    }

    #[test]
    fn jumps_across_a_brace_at_the_caret_to_its_match_and_outward_from_inside_one() {
        let text = "f() { if (x) { a(); } b(); }";
        let state = EditState::new(text);
        let tree = SyntaxTree::default();
        let open = text[6..].find('{').map(|at| at as u32 + 6).expect("an inner block");
        let close = text.find('}').expect("a closing brace") as u32;
        let outer_open = text.find('{').expect("an outer block") as u32;
        let outer_close = text.rfind('}').expect("a last brace") as u32;
        // |{ … } goes to { … }|, and back.
        assert_eq!(code_block_end(&state, &tree, open), Some(close + 1));
        assert_eq!(code_block_start(&state, &tree, close + 1), Some(open));
        // {| and |} go to the outer block.
        assert_eq!(code_block_start(&state, &tree, open + 1), Some(outer_open + 1));
        assert_eq!(code_block_end(&state, &tree, close), Some(outer_close));
    }

    #[test]
    fn skips_braces_in_strings_and_comments() {
        let (state, pos) = at("f() { const s = \"{\"; /* } */ a|(); }");
        let text = state.text();
        let string = text.find("\"{\"").expect("a string") as u32;
        let comment = text.find("/* } */").expect("a comment") as u32;
        let tree = SyntaxTree::new(vec![
            TreeNode::new("Script", 0, utf16_len(&text)),
            TreeNode::new("String", string, string + 3),
            TreeNode::new("BlockComment", comment, comment + 7),
        ]);
        assert_eq!(code_block_start(&state, &tree, pos), Some(utf16_len("f() {")));
        assert_eq!(code_block_end(&state, &tree, pos), Some(utf16_len("f() { const s = \"{\"; /* } */ a(); ")));
    }

    #[test]
    fn has_nowhere_to_go_outside_any_block() {
        let (state, pos) = at("a|();");
        let tree = SyntaxTree::default();
        assert_eq!(code_block_start(&state, &tree, pos), None);
        assert_eq!(code_block_end(&state, &tree, pos), None);
    }

    #[test]
    fn jumps_between_braces_else_to_the_unclosed_opening_brace() {
        let state = EditState::new("f(a, [b], c)");
        let tree = SyntaxTree::default();
        assert_eq!(match_brace_target(&state, &tree, 1), Some(12));
        assert_eq!(match_brace_target(&state, &tree, 12), Some(1));
        assert_eq!(match_brace_target(&state, &tree, 3), Some(1));
        let carets = state_carets("f(a, |[b], c)|");
        assert_eq!(shown(&carets, cursor_match_brace(&carets, &tree)), "f|(a, [b]|, c)");
    }
}
