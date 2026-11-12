//! Quick Evaluate Expression: the selection (or the expression at the caret, or at a ⌥-click)
//! evaluated in a value hint, without a dialog.
//!
//! Positions are UTF-16 offsets in the document.

use super::value_tooltip::{HINT_TIMEOUT_MS, HINT_VALUE_MAX, ValueHint, expression_at, value_hint};
use crate::editor::buffer;
use ide_dap::Variable;
use ropey::Rope;

/// The action a mouse binding of Quick Evaluate dispatches.
pub const QUICK_EVALUATE_ACTION: &str = "debug.quickEvaluate";

/// What the pointer is over when the button goes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerOver {
    /// The text of the editor.
    Content,
    /// A tooltip or an inlay hint, which keep their own clicks.
    Overlay,
    /// The gutters, a scrollbar: not the text.
    Elsewhere,
}

/// Whether a mouse press can be a Quick Evaluate click at all: the primary button, on the text,
/// at a position of the document. Whether it is one is the keymap's decision (the mouse binding
/// of [`QUICK_EVALUATE_ACTION`] and the action being enabled).
pub fn click_position(primary_button: bool, over: PointerOver, pos: Option<u32>) -> Option<u32> {
    pos.filter(|_| primary_button && over == PointerOver::Content)
}

/// The main selection after a Quick Evaluate click at `pos`: a selection the click lands in is
/// kept (it is what gets evaluated); otherwise the caret moves to the click.
pub fn selection_after_click(selection: (u32, u32), pos: u32) -> (u32, u32) {
    let (from, to) = selection;
    if from == to || pos < from || pos > to { (pos, pos) } else { selection }
}

/// Evaluate's initial text, as in IDEA: the selection, else the expression at the caret (at `pos`
/// for a click). `selection`: the main selection's start and end.
pub fn evaluate_seed(doc: &Rope, selection: (u32, u32), pos: Option<u32>) -> String {
    let (from, to) = selection;
    if from != to && pos.is_none_or(|pos| from <= pos && pos <= to) {
        return buffer::slice(doc, from, to);
    }
    expression_at(doc, selection, pos.unwrap_or(from)).map(|expression| expression.text).unwrap_or_default()
}

/// What Quick Evaluate does first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuickEvaluate {
    /// Nothing to evaluate: the editor shows this message as a hint at the caret.
    Nothing(&'static str),
    /// Evaluate `expression` for a hover in the active session; the hint goes to `pos`.
    Evaluate { expression: String, pos: u32 },
}

/// Quick Evaluate in an editor; `click`: the position of the ⌥-click that asked, if one did.
pub fn quick_evaluate(doc: &Rope, selection: (u32, u32), click: Option<u32>) -> QuickEvaluate {
    let seed = evaluate_seed(doc, selection, click);
    let expression = seed.trim();
    if expression.is_empty() {
        return QuickEvaluate::Nothing("Nothing to evaluate at the caret");
    }
    QuickEvaluate::Evaluate { expression: expression.to_owned(), pos: click.unwrap_or(selection.0) }
}

/// The hint that answers a Quick Evaluate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluateHint {
    /// The value, shown until dismissed or `timeout_ms` pass.
    Value { hint: ValueHint, pos: u32, timeout_ms: u32 },
    /// Why it could not be evaluated, in the error tone.
    Error { message: String, pos: u32 },
}

/// The hint for the evaluation's `result` (the value, or the failure's message).
pub fn evaluated(expression: &str, result: Result<&Variable, &str>, pos: u32) -> EvaluateHint {
    match result {
        Ok(value) => EvaluateHint::Value {
            hint: value_hint(expression, value, Some(HINT_VALUE_MAX)),
            pos,
            timeout_ms: HINT_TIMEOUT_MS,
        },
        Err(message) => EvaluateHint::Error { message: message.to_owned(), pos },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "total + order.items";

    fn result() -> Variable {
        Variable {
            name: "result".to_owned(),
            value: "42".to_owned(),
            type_name: Some("int".to_owned()),
            variables_reference: 0,
            evaluate_name: None,
        }
    }

    // src/features/debugger/__tests__/quickEvaluate.test.ts › evaluates the clicked member through %s
    #[test]
    fn evaluates_the_clicked_member() {
        let doc = Rope::from_str(DOC);
        let click = click_position(true, PointerOver::Content, Some(16)).expect("a click on the text");
        // The click is outside the selection: the caret goes there.
        assert_eq!(selection_after_click((0, 5), click), (16, 16));
        let step = quick_evaluate(&doc, (16, 16), Some(click));
        assert_eq!(step, QuickEvaluate::Evaluate { expression: "order.items".to_owned(), pos: 16 });
        let EvaluateHint::Value { hint, pos, timeout_ms } = evaluated("order.items", Ok(&result()), 16) else {
            panic!("a value hint");
        };
        assert!(hint.text().contains("order.items = 42"));
        assert_eq!((pos, timeout_ms), (16, 60_000));
    }

    // src/features/debugger/__tests__/quickEvaluate.test.ts › preserves a clicked selection with %s
    #[test]
    fn preserves_a_clicked_selection() {
        let doc = Rope::from_str(DOC);
        let all = (0, DOC.len() as u32);
        assert_eq!(selection_after_click(all, 16), all);
        let whole = QuickEvaluate::Evaluate { expression: DOC.to_owned(), pos: 16 };
        assert_eq!(quick_evaluate(&doc, all, Some(16)), whole);
    }

    // src/features/debugger/__tests__/quickEvaluate.test.ts
    #[test]
    fn uses_a_selected_expression_spanning_several_lines() {
        let text = "(total +\n order.items)";
        let doc = Rope::from_str(text);
        let all = (0, text.len() as u32);
        assert_eq!(
            quick_evaluate(&doc, all, Some(16)),
            QuickEvaluate::Evaluate { expression: text.to_owned(), pos: 16 },
        );
    }

    // Not from a spec: presses that are not on the text are none of Quick Evaluate's, the keyboard
    // shortcut evaluates at the caret, and a failure shows as an error hint.
    #[test]
    fn evaluates_at_the_caret_and_reports_failures() {
        let doc = Rope::from_str(DOC);
        assert_eq!(click_position(false, PointerOver::Content, Some(16)), None);
        assert_eq!(click_position(true, PointerOver::Overlay, Some(16)), None);
        assert_eq!(click_position(true, PointerOver::Elsewhere, Some(16)), None);
        assert_eq!(click_position(true, PointerOver::Content, None), None);
        let at_caret = QuickEvaluate::Evaluate { expression: "total".to_owned(), pos: 2 };
        assert_eq!(quick_evaluate(&doc, (2, 2), None), at_caret);
        assert_eq!(quick_evaluate(&doc, (6, 6), None), QuickEvaluate::Nothing("Nothing to evaluate at the caret"));
        assert_eq!(evaluate_seed(&doc, (0, 5), Some(16)), "order.items");
        assert_eq!(
            evaluated("x", Err("No active debug session"), 3),
            EvaluateHint::Error { message: "No active debug session".to_owned(), pos: 3 },
        );
    }
}
