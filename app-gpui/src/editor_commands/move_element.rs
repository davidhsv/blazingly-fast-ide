//! Move Element Left / Right: swaps the element at the caret with its neighbour in the enclosing
//! comma-separated list (call arguments, parameters, array and object literal entries).

use super::tree::{SyntaxTree, pascal_name};
use super::{Edit, EditState, caret, change};

const OPEN: [u16; 3] = [b'(' as u16, b'[' as u16, b'{' as u16];
const CLOSE: [u16; 3] = [b')' as u16, b']' as u16, b'}' as u16];
/// Lists longer than this are not scanned.
const MAX_SCAN: u32 = 20_000;
/// Node names whose text is not code: a bracket or a comma there does not count.
const NOT_CODE: [&str; 5] = ["String", "Comment", "Regexp", "Char", "Template"];

/// `name` in PascalCase, a one-word name capitalized too (`string` reads `String`).
fn capitalized(name: &str) -> String {
    let pascal = pascal_name(name);
    let mut chars = pascal.chars();
    chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The text at `pos` is code: the tree is not parsed, or the innermost node starting at or
/// around `pos` is not a string, comment, regular expression, character or template literal.
pub fn is_code(tree: &SyntaxTree, pos: u32) -> bool {
    let Some(node) = tree.resolve_inner(pos, 1) else {
        return true;
    };
    let name = capitalized(tree.name(node));
    !NOT_CODE.iter().any(|word| name.contains(*word))
}

fn position_of(set: &[u16; 3], unit: u16) -> Option<usize> {
    set.iter().position(|candidate| *candidate == unit)
}

/// The offset of the bracket closing the `open_unit` at `open`, skipping nested pairs of the
/// same kind and text that is not code.
fn matching_close(units: &[u16], tree: &SyntaxTree, open: u32, open_unit: u16) -> Option<u32> {
    let want = CLOSE[position_of(&OPEN, open_unit)?];
    let max = (units.len() as u32).min(open.saturating_add(MAX_SCAN));
    let mut depth = 0u32;
    for p in open + 1..max {
        let unit = units[p as usize];
        if unit == open_unit && is_code(tree, p) {
            depth += 1;
        } else if unit == want && is_code(tree, p) {
            if depth == 0 {
                return Some(p);
            }
            depth -= 1;
        }
    }
    None
}

/// The `(open, close)` offsets of the innermost bracket pair around `pos`.
fn enclosing_list(units: &[u16], tree: &SyntaxTree, pos: u32) -> Option<(u32, u32)> {
    let min = pos.saturating_sub(MAX_SCAN);
    let mut stack: Vec<u16> = Vec::new();
    for p in (min..pos).rev() {
        let unit = units[p as usize];
        let close = position_of(&CLOSE, unit);
        let open = position_of(&OPEN, unit);
        if (close.is_none() && open.is_none()) || !is_code(tree, p) {
            continue;
        }
        if let Some(close) = close {
            stack.push(OPEN[close]);
        } else if let Some(expected) = stack.pop() {
            if expected != unit {
                return None;
            }
        } else if let Some(end) = matching_close(units, tree, p, unit)
            && end >= pos
        {
            return Some((p, end));
        }
    }
    None
}

fn is_space(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
}

/// The top-level elements between `from` and `to`, trimmed of whitespace, as `(from, to)`
/// offsets: commas inside quotes and nested brackets do not separate elements.
pub fn list_elements(state: &EditState, from: u32, to: u32) -> Vec<(u32, u32)> {
    let units = state.units();
    list_elements_in(&units, from, to)
}

fn list_elements_in(units: &[u16], from: u32, to: u32) -> Vec<(u32, u32)> {
    let to = to.min(units.len() as u32);
    let text = &units[from.min(to) as usize..to as usize];
    let mut out = Vec::new();
    let mut push = |start: usize, end: usize| {
        let raw = &text[start..end];
        let lead = raw.iter().take_while(|unit| is_space(**unit)).count();
        let trail = raw[lead..].iter().rev().take_while(|unit| is_space(**unit)).count();
        if lead + trail < raw.len() {
            out.push((from + (start + lead) as u32, from + (end - trail) as u32));
        }
    };
    let mut depth = 0i32;
    let mut quote: Option<u16> = None;
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let unit = text[i];
        if let Some(open_quote) = quote {
            if unit == u16::from(b'\\') {
                i += 1;
            } else if unit == open_quote {
                quote = None;
            }
        } else if b"\"'`".map(u16::from).contains(&unit) {
            quote = Some(unit);
        } else if OPEN.contains(&unit) {
            depth += 1;
        } else if CLOSE.contains(&unit) {
            depth -= 1;
        } else if unit == u16::from(b',') && depth == 0 {
            push(start, i);
            start = i + 1;
        }
        i += 1;
    }
    push(start, text.len());
    out
}

/// What Move Element swaps: the element at the caret, its neighbour, and where the caret goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElementSwap {
    /// The element at the caret.
    pub current: (u32, u32),
    /// The neighbour it trades places with.
    pub other: (u32, u32),
    /// The caret in the text after the swap, at the same place in the moved element.
    pub caret: u32,
}

/// The swap Move Element makes at `pos` (`forward` moves right), or `None` when the element has
/// no neighbour on that side or the caret is in no list.
pub fn element_swap(state: &EditState, tree: &SyntaxTree, pos: u32, forward: bool) -> Option<ElementSwap> {
    let units = state.units();
    // The innermost list with more than one element: in `f(g(x), y)` at `x`, `g(x)` moves.
    let mut items: Vec<(u32, u32)> = Vec::new();
    let mut at = pos;
    let mut depth = 0;
    while items.len() < 2 {
        if depth >= 16 {
            return None;
        }
        let list = enclosing_list(&units, tree, at)?;
        items = list_elements_in(&units, list.0 + 1, list.1);
        at = list.0;
        depth += 1;
    }
    // The last element starting at or before the caret (the one before a comma it follows).
    let i = items.iter().rposition(|item| item.0 <= pos).unwrap_or(0);
    let j = if forward { i + 1 } else { i.checked_sub(1)? };
    let current = items[i];
    let other = *items.get(j)?;
    let length = current.1 - current.0;
    let offset = pos.saturating_sub(current.0).min(length);
    let start = if forward { other.1 - length } else { other.0 };
    Some(ElementSwap { current, other, caret: start + offset })
}

fn move_element(state: &EditState, tree: &SyntaxTree, forward: bool) -> Option<Edit> {
    if state.selections.len() > 1 {
        return None;
    }
    let swap = element_swap(state, tree, state.main().1, forward)?;
    let (a, b) = (swap.current, swap.other);
    let mut changes = vec![change(a.0, a.1, state.slice(b.0, b.1)), change(b.0, b.1, state.slice(a.0, a.1))];
    changes.sort_by_key(|change| change.from);
    Some(Edit::new(changes, vec![caret(swap.caret)], 0))
}

/// Move Element Left: the element at the caret trades places with the one before it.
pub fn move_element_left(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_element(state, tree, false)
}

/// Move Element Right: the element at the caret trades places with the one after it.
pub fn move_element_right(state: &EditState, tree: &SyntaxTree) -> Option<Edit> {
    move_element(state, tree, true)
}

#[cfg(test)]
mod tests {
    use super::super::testing::{shown, state_carets};
    use super::*;

    fn moved(marked: &str, command: fn(&EditState, &SyntaxTree) -> Option<Edit>) -> String {
        let before = state_carets(marked);
        shown(&before, command(&before, &SyntaxTree::default()))
    }

    #[test]
    fn swaps_call_arguments_keeping_the_caret_in_the_moved_one() {
        assert_eq!(moved("f(a|lpha, b)", move_element_right), "f(b, a|lpha)");
        assert_eq!(moved("f(alpha, b|eta)", move_element_left), "f(b|eta, alpha)");
    }

    #[test]
    fn moves_a_nested_call_within_the_enclosing_list() {
        assert_eq!(moved("f(g(|x), y)", move_element_right), "f(y, g(|x))");
    }

    #[test]
    fn ignores_commas_in_strings_and_nested_brackets() {
        assert_eq!(moved("f(\"a, b\", [1, 2], |c)", move_element_left), "f(\"a, b\", |c, [1, 2])");
    }

    #[test]
    fn does_nothing_at_the_end_of_the_list() {
        assert_eq!(moved("f(a, |b)", move_element_right), "f(a, |b)");
    }

    #[test]
    fn works_across_lines() {
        assert_eq!(moved("[\n  one,\n  |two,\n]", move_element_left), "[\n  |two,\n  one,\n]");
    }
}
