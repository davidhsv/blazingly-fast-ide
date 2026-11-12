//! Value hints in the editor: while suspended, hovering an expression (or the selection under the
//! mouse) or selecting one evaluates it in the selected frame and shows `expression = value {type}`.
//! Settings › Debugger › Data Views › "Show value tooltip on mouse hover" / "on code selection".
//!
//! Positions are UTF-16 offsets in the document.

use crate::debugger::values::inline_value;
use crate::editor::buffer;
use crate::find::utf16;
use ide_dap::Variable;
use ropey::Rope;

/// The hover delay every tooltip has, in milliseconds; the configured delay adds to it.
pub const BASE_HOVER_MS: u32 = 100;
/// How long a selection has to rest before its value shows, in milliseconds.
pub const SELECTION_DELAY_MS: u32 = 400;
/// The longest selection that is evaluated, in UTF-16 code units.
const MAX_SELECTION: u32 = 200;
/// How long a value hint stays, in milliseconds.
pub const HINT_TIMEOUT_MS: u32 = 60_000;
/// Default length of the value in a hint, in UTF-16 code units.
pub const HINT_VALUE_MAX: u32 = 400;

/// Whether `c` can be part of an identifier.
pub(crate) fn is_identifier_char(c: char) -> bool {
    c.is_alphabetic() || c.is_numeric() || c == '_' || c == '$'
}

/// Whether a UTF-16 code unit is an identifier character on its own.
fn is_identifier_unit(unit: Option<&u16>) -> bool {
    unit.and_then(|unit| char::from_u32(u32::from(*unit))).is_some_and(is_identifier_char)
}

/// An expression in the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expression {
    pub from: u32,
    pub to: u32,
    pub text: String,
}

/// The identifier (with `a.b.c` member access) around `pos`, or the selection containing it.
/// `selection`: the main selection's start and end.
pub fn expression_at(doc: &Rope, selection: (u32, u32), pos: u32) -> Option<Expression> {
    let (from, to) = selection;
    if from != to && pos >= from && pos <= to {
        let selected = buffer::slice(doc, from, to);
        let text = selected.trim();
        return (!text.is_empty() && !text.contains('\n')).then(|| Expression { from, to, text: text.to_owned() });
    }
    let (line, column) = buffer::line_col(doc, pos);
    let line_from = buffer::line_range(doc, line).start;
    let text = buffer::line_text(doc, line);
    let units: Vec<u16> = text.encode_utf16().collect();
    let (mut start, mut end) = (column as usize, column as usize);
    let before = |at: usize, back: usize| at.checked_sub(back).and_then(|index| units.get(index));
    if !is_identifier_unit(units.get(end)) && !is_identifier_unit(before(start, 1)) {
        return None;
    }
    while is_identifier_unit(units.get(end)) {
        end += 1;
    }
    let dot = u16::from(b'.');
    while start > 0
        && (is_identifier_unit(before(start, 1))
            || (before(start, 1) == Some(&dot) && is_identifier_unit(before(start, 2))))
    {
        start -= 1;
    }
    let found = utf16::slice(&text, start as u32, end as u32);
    if found.is_empty() || found.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(Expression { from: line_from + start as u32, to: line_from + end as u32, text: found.to_owned() })
}

/// The selected text when it can be evaluated: one selection of one non-blank line, not too long.
/// `ranges`: the start and end of every selection range. Returns where it starts and the text.
pub fn selection_expression(doc: &Rope, ranges: &[(u32, u32)]) -> Option<(u32, String)> {
    let [(from, to)] = *ranges else { return None };
    if from == to {
        return None;
    }
    let selected = buffer::slice(doc, from, to);
    let text = selected.trim();
    if text.is_empty() || utf16::len(text) > MAX_SELECTION || text.contains('\n') {
        return None;
    }
    Some((from, text.to_owned()))
}

/// What a value hint shows: `expression = value {type}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueHint {
    /// `expression = `.
    pub name: String,
    pub value: String,
    /// `  {type}`, when the adapter names the type.
    pub type_suffix: Option<String>,
    /// The value has children: the hint expands into a variables tree in place.
    pub expandable: bool,
}

impl ValueHint {
    /// The hint as one line of text.
    pub fn text(&self) -> String {
        format!("{}{}{}", self.name, self.value, self.type_suffix.as_deref().unwrap_or_default())
    }
}

/// The hint for `expression` evaluated to `value`; `max`: the longest value shown, `None` for all
/// of it.
pub fn value_hint(expression: &str, value: &Variable, max: Option<u32>) -> ValueHint {
    ValueHint {
        name: format!("{expression} = "),
        value: max.map_or_else(|| value.value.clone(), |max| inline_value(&value.value, max)),
        type_suffix: value.type_name.as_deref().filter(|name| !name.is_empty()).map(|name| format!("  {{{name}}}")),
        expandable: value.variables_reference > 0,
    }
}

/// Whether hovering evaluates: the setting is on, no navigation modifier is held (⌘/Ctrl-hover
/// shows the navigation info instead) and the active session is suspended.
pub fn hover_tooltip_applies(setting: bool, navigation_modifier_held: bool, session_paused: bool) -> bool {
    setting && !navigation_modifier_held && session_paused
}

/// How long to wait beyond the base hover delay for the configured `delay_ms`.
pub fn extra_hover_delay(delay_ms: u32) -> u32 {
    delay_ms.saturating_sub(BASE_HOVER_MS)
}

/// Whether a settled selection evaluates: the setting is on and the active session is suspended.
pub fn selection_tooltip_applies(setting: bool, session_paused: bool) -> bool {
    setting && session_paused
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(doc: &str, from: u32, to: u32) -> Option<(u32, String)> {
        selection_expression(&Rope::from_str(doc), &[(from, to)])
    }

    // src/features/debugger/__tests__/values.test.ts › selectionExpression
    #[test]
    fn takes_a_one_line_selection() {
        assert_eq!(at("total += item.price", 9, 19), Some((9, "item.price".to_owned())));
    }

    // src/features/debugger/__tests__/values.test.ts › selectionExpression
    #[test]
    fn ignores_empty_blank_multi_line_and_multiple_selections() {
        assert_eq!(at("a + b", 2, 2), None);
        assert_eq!(at("a   b", 1, 4), None);
        assert_eq!(at("a\nb", 0, 3), None);
        assert_eq!(selection_expression(&Rope::from_str("a + b"), &[(0, 1), (4, 5)]), None);
    }

    // Not from a spec: the expression under a position is the identifier with its member access,
    // or the selection when the position is inside one.
    #[test]
    fn finds_the_expression_around_a_position() {
        let doc = Rope::from_str("total = order.items\nfor i in x:\n  pass");
        let expression = |from, to, text: &str| Some(Expression { from, to, text: text.to_owned() });
        assert_eq!(expression_at(&doc, (15, 15), 15), expression(8, 19, "order.items"));
        assert_eq!(expression_at(&doc, (9, 9), 9), expression(8, 13, "order"));
        assert_eq!(expression_at(&doc, (19, 19), 19), expression(8, 19, "order.items"));
        assert_eq!(expression_at(&doc, (6, 6), 6), None);
        // A selection of several lines is no expression to hover.
        assert_eq!(expression_at(&doc, (0, 31), 4), None);
        assert_eq!(expression_at(&doc, (8, 19), 10), expression(8, 19, "order.items"));
        assert_eq!(expression_at(&Rope::from_str("x = 42"), (5, 5), 5), None);
    }

    // Not from a spec: the hint names the expression, shortens long values and marks expandable ones.
    #[test]
    fn describes_a_value_as_a_hint() {
        let value = Variable {
            name: "result".to_owned(),
            value: "42".to_owned(),
            type_name: Some("int".to_owned()),
            variables_reference: 0,
            evaluate_name: None,
        };
        let hint = value_hint("order.items", &value, Some(HINT_VALUE_MAX));
        assert_eq!(hint.text(), "order.items = 42  {int}");
        assert!(!hint.expandable);
        let long = Variable { value: "x".repeat(500), variables_reference: 7, type_name: None, ..value };
        assert_eq!(utf16::len(&value_hint("s", &long, Some(HINT_VALUE_MAX)).value), HINT_VALUE_MAX);
        let full = value_hint("s", &long, None);
        assert!(full.expandable && full.value.len() == 500 && full.type_suffix.is_none());
        assert!(hover_tooltip_applies(true, false, true) && !hover_tooltip_applies(true, true, true));
        assert_eq!((extra_hover_delay(700), extra_hover_delay(50)), (600, 0));
        assert!(selection_tooltip_applies(true, true) && !selection_tooltip_applies(false, true));
    }
}
