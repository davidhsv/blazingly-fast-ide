//! The lines Generate, Override Methods and Apply Quick Fix insert are indented to the file's style; the editor's
//! language actions are the editor's; ⌘N is Generate only where it applies; the rename preview says what it changes.

use super::*;
use std::cell::RefCell;
use std::rc::Rc;

fn focus_tree(f: &Fixture, cx: &mut VisualTestContext) {
    let tree = f.workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a project tree");
    cx.update(|window, cx| {
        let focus = tree.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
}

fn focus_editor(f: &Fixture, cx: &mut VisualTestContext) {
    let editor = f.active(cx);
    cx.update(|window, cx| {
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
}

/// With the Project view's keyboard the action `id` is not available and the file stays as it is.
fn refused_from_the_tree(f: &Fixture, id: &str, cx: &mut VisualTestContext) {
    let before = f.text(cx);
    focus_tree(f, cx);
    let refused = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
    assert!(refused.is_err(), "{id} is the editor's");
    assert_eq!(f.text(cx), before);
    focus_editor(f, cx);
}

/// The class whose members the actions insert, and what they insert at column 0 under its `{`.
const CLASS: &str = "class A {\n}\n";
const INDENTED: &str = "class A {\n    run() {\n        go()\n    }\n}\n";
const AT_COLUMN_0: &str = "run() {\n    go()\n}\n";

/// The action the chooser of Generate offers is applied and the lines the server wrote at column 0 under the
/// class's `{` move one indentation level in, keeping their own nesting. Generate is not available while the
/// Project view has the keyboard.
#[gpui_kit::test]
async fn generate_indents_the_inserted_members_under_the_brace(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.mock", CLASS)], "a.mock", Some(json!({})), &[]);
    let a = f.root.join("a.mock");
    let action = |title: &str, kind: &str, text: &str| {
        let edit = edit(&a, vec![text_edit(range(1, 0, 0), text)]);
        json!({ "title": title, "kind": kind, "edit": edit })
    };
    f.answer(json!({ "textDocument/codeAction": [
        action("Generate constructor", "source.generate.constructor", "A() {\n}\n"),
        action("Generate run()", "source.generate.run", AT_COLUMN_0),
    ] }));
    refused_from_the_tree(&f, "code.generate", cx);
    f.put_caret(cx, 6);
    f.run(cx, "code.generate");
    let list = f.list(cx);
    let lines = list.read_with(cx, |l, _| l.lines());
    let chosen = lines.iter().position(|line| line.contains("Generate run()")).expect("listed");
    for _ in 0..chosen {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    settle(cx, "the editor's text", |cx| f.text(cx) == INDENTED);
    f.close(cx);
}

/// Override Methods applies the only override the server offers, and its lines written at column 0 under the
/// class's `{` move one indentation level in. It is not available while the Project view has the keyboard.
#[gpui_kit::test]
async fn override_methods_indents_the_inserted_method_under_the_brace(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.mock", CLASS)], "a.mock", Some(json!({})), &[]);
    let a = f.root.join("a.mock");
    let edit = edit(&a, vec![text_edit(range(1, 0, 0), AT_COLUMN_0)]);
    f.answer(json!({ "textDocument/codeAction": [{ "title": "Override run()", "kind": "quickfix", "edit": edit }] }));
    refused_from_the_tree(&f, "code.overrideMethods", cx);
    f.put_caret(cx, 6);
    f.run(cx, "code.overrideMethods");
    settle(cx, "the editor's text", |cx| f.text(cx) == INDENTED);
    f.close(cx);
}

/// Apply Quick Fix applies the fix of the problem at the caret, and the lines the fix wrote at column 0 under the
/// class's `{` move one indentation level in. It is not available while the Project view has the keyboard.
#[gpui_kit::test]
async fn apply_quick_fix_indents_the_inserted_lines_under_the_brace(cx: &mut TestAppContext) {
    let problem = json!({ "range": range(0, 6, 7), "message": "A lacks run", "severity": 1, "source": "mock" });
    let answers = json!({ "diagnostics": [problem.clone()] });
    let (f, cx) = open(cx, &[("a.mock", CLASS)], "a.mock", Some(answers), &[]);
    let a = f.root.join("a.mock");
    let editor = f.active(cx);
    settle(cx, "the problems", |cx| cx.update(|_, cx| !problems(&editor, cx).is_empty()));
    let edit = edit(&a, vec![text_edit(range(1, 0, 0), AT_COLUMN_0)]);
    let fix = json!({ "title": "Implement run()", "kind": "quickfix", "isPreferred": true, "edit": edit });
    f.answer(json!({ "diagnostics": [problem], "textDocument/codeAction": [fix] }));
    refused_from_the_tree(&f, "code.applyQuickFix", cx);
    f.put_caret(cx, 6);
    f.run(cx, "code.applyQuickFix");
    settle(cx, "the editor's text", |cx| f.text(cx) == INDENTED);
    f.close(cx);
}

// src/platform/keymaps/newElement.test.ts › lets Generate win the New key only where it applies
/// ⌘N, the key Generate shares with New, runs Generate in a Java file; in a Python file, where Generate does not
/// apply, it opens the New popup instead.
#[gpui_kit::test]
async fn lets_generate_win_the_new_key_only_where_it_applies(cx: &mut TestAppContext) {
    let files = [("src/A.java", CLASS), ("tool.py", "x = 1\n")];
    let (f, cx) = open(cx, &files, "src/A.java", None, &[]);
    let heard: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = heard.clone();
    let listener = cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
    });
    let new_popup = |cx: &mut VisualTestContext| f.workbench.read_with(cx, |w, _| w.new_popup().is_some());

    cx.simulate_keystrokes("cmd-n");
    settle(cx, "Generate to run", |_| heard.borrow().contains(&"code.generate".to_owned()));
    assert!(!new_popup(cx), "no New popup in a Java file");
    assert_eq!(f.text(cx), CLASS);

    let python = f.root.join("tool.py");
    f.workbench.update_in(cx, |w, window, cx| w.open_project_file(python.clone(), window, cx));
    settle(cx, "the Python file has the keyboard", |cx| {
        if !f.editing(cx, &python) {
            return false;
        }
        let editor = f.active(cx);
        cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
    });
    heard.borrow_mut().clear();
    cx.simulate_keystrokes("cmd-n");
    settle(cx, "the New popup", new_popup);
    assert!(!heard.borrow().contains(&"code.generate".to_owned()), "{:?}", heard.borrow());
    assert_eq!(f.text(cx), "x = 1\n");
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
    f.close(cx);
}

/// The preview of a rename that changes two files says what it changes: how many occurrences in how many files,
/// then a line per file. Cancel applies nothing.
#[gpui_kit::test]
async fn the_rename_preview_says_how_many_occurrences_in_how_many_files_it_changes(cx: &mut TestAppContext) {
    let files = [("a.mock", "let foo = 1\nfoo + foo\n"), ("b.mock", "use foo\n")];
    let capabilities = json!({ "renameProvider": { "prepareProvider": true } });
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
    let (a, b) = (f.root.join("a.mock"), f.root.join("b.mock"));
    let a_edits =
        vec![text_edit(range(0, 4, 7), "bar"), text_edit(range(1, 0, 3), "bar"), text_edit(range(1, 6, 9), "bar")];
    let both = json!({ "changes": { uri(&a): a_edits, uri(&b): [text_edit(range(0, 4, 7), "bar")] } });
    f.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareRename": range(0, 4, 7),
        "textDocument/rename": both,
    }));
    f.put_caret(cx, 5);
    f.run(cx, "refactor.rename");
    let dialog = f.prompt(cx);
    assert_eq!(retype(&dialog, "bar", cx), "foo");
    cx.simulate_keystrokes("enter");
    settle(cx, "the preview", |cx| {
        f.workbench.read_with(cx, |w, cx| w.vcs_prompt().is_some_and(|d| d.read(cx).title() != "Rename 'foo'"))
    });
    let preview = f.prompt(cx);
    assert_eq!(preview.read_with(cx, |d, _| d.title().to_owned()), "Rename 'foo' to 'bar'");
    let message = preview.read_with(cx, |d, _| d.message_text().map(str::to_owned)).expect("the preview has a body");
    let mut lines: Vec<&str> = message.lines().collect();
    assert_eq!(lines.first().copied(), Some("4 occurrences in 2 files"), "{message}");
    let mut changed = lines.split_off(1);
    changed.sort_unstable();
    assert_eq!(changed, ["a.mock  a.mock", "b.mock  b.mock"], "{message}");

    cx.simulate_keystrokes("escape");
    settle(cx, "the preview to close", |cx| f.workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
    assert_eq!(f.text(cx), "let foo = 1\nfoo + foo\n", "Cancel renames nothing");
    assert_eq!(f.project_text(cx, &b), "use foo\n");
    f.close(cx);
}
