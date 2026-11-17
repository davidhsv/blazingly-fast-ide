//! A postfix template that stands for an editor action (Java's `.var` is Introduce Variable, which
//! lets the language server declare the expression's type). Before the action runs, the `.key`
//! goes, a statement is ended with `;` so that the server sees complete code, and the expression is
//! selected for the action to work on. When the action does not run, the `;` goes again and the
//! template's own text replaces the expression.

use super::editor::TemplateExpansion;
use crate::editor::buffer;
use ide_core::TextChange;
use ropey::Rope;

/// How far after the key the text is read for a `;` that already ends the statement (UTF-16 units).
const LOOK_AHEAD: u32 = 80;

/// The editor action a postfix template runs on its expression, and where the expression ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostfixAction {
    /// The action's id.
    pub action: &'static str,
    /// Offset of the `.` before the key: the expression ends there.
    pub dot: u32,
    /// The template is offered where a statement starts: it is ended with `;` first.
    pub statement: bool,
}

/// The edit that prepares the action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionEdit {
    /// The `.key` replaced by `;`, or by nothing.
    pub changes: Vec<TextChange>,
    /// The selection afterwards, `(anchor, head)`: the expression, its end the head.
    pub selection: (u32, u32),
    /// Takes the added `;` out again, in the text after [`ActionEdit::changes`].
    pub terminator: Option<TextChange>,
}

/// The edit that removes the `.key` from `run.dot` to `end` (the caret) and selects the expression
/// from `from` to the dot. A statement gets a `;` in the key's place unless one already follows on
/// its line, blanks aside.
pub fn action_edit(doc: &Rope, from: u32, run: &PostfixAction, end: u32) -> ActionEdit {
    let len = buffer::len_utf16(doc);
    let end = end.min(len);
    let after = buffer::slice(doc, end, end.saturating_add(LOOK_AHEAD).min(len));
    let ended = after.trim_start_matches([' ', '\t']).starts_with(';');
    let insert = if run.statement && !ended { ";" } else { "" };
    let terminator = if insert.is_empty() {
        None
    } else {
        Some(TextChange { from: run.dot, to: run.dot + 1, insert: String::new() })
    };
    ActionEdit {
        changes: vec![TextChange { from: run.dot, to: end, insert: insert.to_owned() }],
        selection: (from, run.dot),
        terminator,
    }
}

/// `expansion` as the template's own text over the expression alone: the key is already gone.
pub fn without_action(expansion: &TemplateExpansion, dot: u32) -> TemplateExpansion {
    TemplateExpansion { to: dot, postfix_action: None, ..expansion.clone() }
}
