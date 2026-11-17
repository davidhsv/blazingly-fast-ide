//! Java's `.var` stands for Introduce Variable: Tab gives the action with where the expression
//! ends, the edit before the action takes the key away and ends the statement, and the template's
//! text replaces the expression when the action does not run.

use super::editor::{TemplateAction, TemplateExpansion};
use super::editor_context::{CommentTokens, TreeNode};
use super::expand::{EditorSnapshot, TabPress, TemplateEdit, TemplateStores, tab_press};
use super::postfix_action::{PostfixAction, action_edit, without_action};
use super::postfix_model::postfix_templates_for;
use super::snippet::testing::replace;
use crate::editor::buffer;
use ide_core::TextChange;
use ropey::Rope;
use std::collections::BTreeSet;

const INTRODUCE_VARIABLE: &str = "refactor.extractVariable";

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// A Java editor on `text` with its caret at `caret`.
fn java(text: &str, caret: u32) -> EditorSnapshot {
    EditorSnapshot {
        doc: Rope::from_str(text),
        selections: vec![(caret, caret)],
        main: 0,
        path: "/p/src/A.java".to_owned(),
        language: "java".to_owned(),
        ancestors: vec![TreeNode::new("Document", 0, len(text))],
        comment_tokens: CommentTokens { line: Some("//".to_owned()), block: None },
        project_root: "/p".to_owned(),
        indent_unit: "  ".to_owned(),
    }
}

/// `doc` with `changes` applied (in the coordinates of `doc`, applied from the last).
fn edited(doc: &Rope, changes: &[TextChange]) -> Rope {
    let mut doc = doc.clone();
    for change in changes.iter().rev() {
        replace(&mut doc, change.from, change.to, &change.insert);
    }
    doc
}

/// The expansion Tab makes in `snapshot`, which must be one.
fn tab_expansion(snapshot: &EditorSnapshot) -> TemplateExpansion {
    match tab_press(snapshot, None, false, false, false, &TemplateStores::default()) {
        TabPress::Template(action) => match *action {
            TemplateAction::Expand(expansion) => expansion,
            other => panic!("expected an expansion, got {other:?}"),
        },
        other => panic!("expected a template, got {other:?}"),
    }
}

// Java .var carries Introduce Variable and the description of the declaration it makes; the `.var`
// of the other languages inserts its text.
#[test]
fn java_var_stands_for_introduce_variable() {
    let var = |language: &str| {
        let all = postfix_templates_for(language, &BTreeSet::new());
        all.into_iter().find(|t| t.key == "var").unwrap_or_else(|| panic!("{language} has .var"))
    };
    assert_eq!(var("java").action, Some(INTRODUCE_VARIABLE));
    assert_eq!(var("java").description, "T name = expr;");
    assert_eq!(var("java").body, "var $NAME$ = $EXPR$;$END$");
    for language in ["typescript", "python", "go", "kotlin"] {
        assert_eq!(var(language).action, None, "{language}");
    }
}

// Java .var › runs Introduce Variable on the expression, ended as a statement: the edit before the
// action takes the key away, ends the statement with `;` and selects the expression.
#[test]
fn the_var_key_becomes_a_semicolon_and_the_expression_is_selected() {
    let line = "class A { void f() {\n  list.get(0).var";
    let before = java(&format!("{line}\n}} }}"), len(line));
    let expansion = tab_expansion(&before);
    let dot = len("class A { void f() {\n  list.get(0)");
    let run = expansion.postfix_action.expect("the template runs an action");
    assert_eq!(run, PostfixAction { action: INTRODUCE_VARIABLE, dot, statement: true });
    assert_eq!(expansion.from, len("class A { void f() {\n  "));
    let edit = action_edit(&before.doc, expansion.from, &run, expansion.to);
    let after = edited(&before.doc, &edit.changes);
    assert_eq!(after.to_string(), "class A { void f() {\n  list.get(0);\n} }");
    let (anchor, head) = edit.selection;
    assert_eq!(buffer::slice(&after, anchor, head), "list.get(0)");
    assert_eq!(head, dot, "the caret ends the expression");
    let terminator = edit.terminator.as_ref().expect("a `;` was added");
    let without = edited(&after, std::slice::from_ref(terminator));
    assert_eq!(without.to_string(), "class A { void f() {\n  list.get(0)\n} }");

    // A `;` already there is not doubled; an expression that is not a statement gets none.
    let line = "  value.var";
    let ended = Rope::from_str(&format!("{line} ;\n"));
    let at = PostfixAction { action: INTRODUCE_VARIABLE, dot: len("  value"), statement: true };
    let edit = action_edit(&ended, 2, &at, len(line));
    assert_eq!(edited(&ended, &edit.changes).to_string(), "  value ;\n");
    assert_eq!(edit.terminator, None);
    let inside = PostfixAction { statement: false, ..at };
    let open = Rope::from_str(line);
    assert_eq!(edited(&open, &action_edit(&open, 2, &inside, len(line)).changes).to_string(), "  value");
}

// Java .var › falls back to the var template without Introduce Variable: once the key and the `;`
// are gone, the template's text replaces the expression.
#[test]
fn the_var_template_replaces_the_expression_when_the_action_does_not_run() {
    let line = "class A { void f() {\n  name().var";
    let before = java(&format!("{line}\n}} }}"), len(line));
    let expansion = tab_expansion(&before);
    let run = expansion.postfix_action.expect("the template runs an action");
    let edit = action_edit(&before.doc, expansion.from, &run, expansion.to);
    let prepared = edited(&before.doc, &edit.changes);
    let terminator = edit.terminator.as_ref().expect("a `;` was added");
    let restored = edited(&prepared, std::slice::from_ref(terminator));
    let (anchor, head) = edit.selection;
    let state = EditorSnapshot { doc: restored, selections: vec![(anchor, head)], ..before.clone() };
    let fallback = without_action(&expansion, run.dot);
    assert_eq!((fallback.from, fallback.to, fallback.postfix_action), (anchor, run.dot, None));
    let applied = TemplateEdit::new(fallback.apply(&state.template_editor(), None));
    let after = edited(&state.doc, &applied.changes);
    assert_eq!(after.to_string(), "class A { void f() {\n  var name = name();\n} }");
}
