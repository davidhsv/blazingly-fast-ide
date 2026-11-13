use super::*;

/// A class to generate into, with the fake server answering `actions` for code actions; the caret
/// inside it.
fn class_with_actions<'a>(
    cx: &'a mut TestAppContext,
    actions: &[(&str, &str, &str)],
) -> (Fixture, &'a mut VisualTestContext) {
    let (f, cx) = open(cx, &[("a.mock", "class A {\n}\n")], "a.mock", Some(json!({})), &[]);
    let a = f.root.join("a.mock");
    let answered: Vec<Value> = actions
        .iter()
        .map(|(title, kind, text)| {
            json!({ "title": title, "kind": kind, "edit": edit(&a, vec![text_edit(range(1, 0, 0), text)]) })
        })
        .collect();
    f.answer(json!({ "textDocument/codeAction": answered }));
    f.put_caret(cx, 6);
    (f, cx)
}

/// ⌃I in the editor is Implement Methods: the only implement action the server offers is applied.
#[gpui_kit::test]
async fn ctrl_i_in_the_editor_implements_the_missing_members(cx: &mut TestAppContext) {
    let actions = [
        ("Implement missing members", "quickfix", "  run() {}\n"),
        ("Generate constructor", "source.generate.constructor", "  A() {}\n"),
    ];
    let (f, cx) = class_with_actions(cx, &actions);
    cx.simulate_keystrokes("ctrl-i");
    settle(cx, |cx| f.text(cx) == "class A {\n  run() {}\n}\n");
    f.close(cx);
}

/// ⌃O in the editor is Override Methods: the only override action the server offers is applied.
#[gpui_kit::test]
async fn ctrl_o_in_the_editor_overrides_the_offered_method(cx: &mut TestAppContext) {
    let actions = [
        ("Override toString()", "quickfix", "  toString() {}\n"),
        ("Implement missing members", "quickfix", "  run() {}\n"),
    ];
    let (f, cx) = class_with_actions(cx, &actions);
    cx.simulate_keystrokes("ctrl-o");
    settle(cx, |cx| f.text(cx) == "class A {\n  toString() {}\n}\n");
    f.close(cx);
}

/// `keys` in the editor show the Quick Definition popup of the definition at the caret.
fn quick_definition_by_key(cx: &mut TestAppContext, keys: &str) {
    let files = [("a.mock", "call target here\n"), ("b.mock", "zero\none\n  target\nthree\n")];
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
    let b = f.root.join("b.mock");
    let at = json!({ "line": 2, "character": 2 });
    f.answer(json!({ "textDocument/definition": [{ "uri": uri(&b), "range": { "start": at, "end": at } }] }));
    f.put_caret(cx, 7);
    cx.simulate_keystrokes(keys);
    settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some()));
    let popup = f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>()).expect("the popup");
    assert_eq!(popup.read_with(cx, |p, _| p.header()), ["b.mock", "b.mock:3"], "{keys}");
    assert_eq!(popup.read_with(cx, |p, _| p.lines()), ["2  one", "3    target", "4  three"], "{keys}");
    assert_eq!(f.text(cx), "call target here\n", "{keys} typed nothing");
    f.close(cx);
}

/// ⌥Space in the editor is Quick Definition: the popup shows the definition's lines.
#[gpui_kit::test]
async fn alt_space_in_the_editor_shows_the_quick_definition(cx: &mut TestAppContext) {
    quick_definition_by_key(cx, "alt-space");
}

/// ⌘Y in the editor is Quick Definition too.
#[gpui_kit::test]
async fn cmd_y_in_the_editor_shows_the_quick_definition(cx: &mut TestAppContext) {
    quick_definition_by_key(cx, "cmd-y");
}

/// ⇧⌘T in the editor is Go to Test: from the file to its test, and back to the file it tests.
#[gpui_kit::test]
async fn cmd_shift_t_in_the_editor_goes_to_the_test_and_back(cx: &mut TestAppContext) {
    let files =
        [("src/calc.py", "def add(a, b):\n    return a + b\n"), ("tests/test_calc.py", "def test_add():\n    pass\n")];
    let (f, cx) = open(cx, &files, "src/calc.py", None, &[]);
    let (subject, test) = (f.root.join("src/calc.py"), f.root.join("tests/test_calc.py"));
    let indexed = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let host = cx.global::<Backend>().host.clone();
            let found = ide_host::ops::nav::find_files(&host, "w1", "test_calc.py".to_owned(), 50);
            found.is_ok_and(|found| !found.is_empty())
        })
    };
    settle(cx, indexed);
    cx.simulate_keystrokes("cmd-shift-t");
    settle(cx, |cx| f.editing(cx, &test));
    // The opened test has the keyboard: the same key goes back.
    settle(cx, |cx| {
        let editor = f.active(cx);
        cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
    });
    cx.simulate_keystrokes("cmd-shift-t");
    settle(cx, |cx| f.editing(cx, &subject));
    f.close(cx);
}

/// ⇧F6 in the editor is Rename: it asks the new name of the element at the caret and applies the
/// server's edit of this file.
#[gpui_kit::test]
async fn shift_f6_in_the_editor_renames_the_element_at_the_caret(cx: &mut TestAppContext) {
    let capabilities = json!({ "renameProvider": { "prepareProvider": true } });
    let text = "let foo = 1\nfoo + foo\n";
    let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
    let a = f.root.join("a.mock");
    let edits: Vec<Value> =
        [range(0, 4, 7), range(1, 0, 3), range(1, 6, 9)].into_iter().map(|at| text_edit(at, "bar")).collect();
    f.answer(json!({
        "capabilities": capabilities,
        "textDocument/prepareRename": range(0, 4, 7),
        "textDocument/rename": { "changes": { uri(&a): edits } },
    }));
    f.put_caret(cx, 5);
    cx.simulate_keystrokes("shift-f6");
    let dialog = f.prompt(cx);
    assert_eq!(dialog.read_with(cx, |d, _| d.title().to_owned()), "Rename 'foo'");
    assert_eq!(retype(&dialog, "bar", cx), "foo");
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| f.text(cx) == "let bar = 1\nbar + bar\n");
    f.close(cx);
}

/// ⇧F2 in the editor is Previous Highlighted Error: from before the first error it goes round to
/// the last one and shows its message, then back to the one before, skipping the warning.
#[gpui_kit::test]
async fn shift_f2_in_the_editor_goes_to_the_previous_error(cx: &mut TestAppContext) {
    let problem = |line: u32, from: u32, to: u32, severity: u32, message: &str| json!({ "range": range(line, from, to), "message": message, "severity": severity, "source": "mock" });
    let diagnostics = json!([
        problem(0, 4, 7, 1, "unknown foo"),
        problem(1, 0, 3, 2, "unused bar"),
        problem(2, 2, 5, 1, "unknown baz"),
    ]);
    let text = "let foo = 1\nbar\n  baz\n";
    let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "diagnostics": diagnostics })), &[]);
    let editor = f.active(cx);
    settle(cx, |cx| cx.update(|_, cx| problems(&editor, cx).len()) == 3);
    f.put_caret(cx, 0);
    cx.simulate_keystrokes("shift-f2");
    settle(cx, |cx| f.caret(cx) == 18);
    f.wait_hint(cx, "unknown baz");
    cx.simulate_keystrokes("shift-f2");
    settle(cx, |cx| f.caret(cx) == 4);
    f.wait_hint(cx, "unknown foo");
    f.close(cx);
}

/// ⌃⇧↑ in the editor is Previous Method: the caret goes to the method before it in the outline.
#[gpui_kit::test]
async fn ctrl_shift_up_in_the_editor_goes_to_the_previous_method(cx: &mut TestAppContext) {
    let capabilities = json!({ "documentSymbolProvider": true });
    let text = "fn one() {\n}\n\nfn two() {\n}\n";
    let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
    let symbol = |name: &str, line: u32| {
        let whole = json!({ "start": { "line": line, "character": 0 }, "end": { "line": line + 1, "character": 1 } });
        json!({ "name": name, "kind": 12, "range": whole, "selectionRange": range(line, 3, 6) })
    };
    let symbols = [symbol("one", 0), symbol("two", 3)];
    f.answer(json!({ "capabilities": capabilities, "textDocument/documentSymbol": symbols }));
    // On `two`'s name.
    f.put_caret(cx, 17);
    cx.simulate_keystrokes("ctrl-shift-up");
    settle(cx, |cx| f.caret(cx) == 3);
    assert_eq!(f.text(cx), text, "no line moved");
    f.close(cx);
}

/// ⌃⌥↑ in the editor is Previous Highlighted Usage: from the first highlighted usage it goes round
/// to the last, then to the one before it.
#[gpui_kit::test]
async fn ctrl_alt_up_in_the_editor_goes_to_the_previous_highlighted_usage(cx: &mut TestAppContext) {
    let capabilities = json!({ "documentHighlightProvider": true });
    let text = "let foo = 1\nfoo + foo\n";
    let (f, cx) = open(cx, &[("a.mock", text)], "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
    let highlights = json!([
        { "range": range(0, 4, 7), "kind": 3 },
        { "range": range(1, 0, 3), "kind": 2 },
        { "range": range(1, 6, 9), "kind": 2 },
    ]);
    f.answer(json!({ "capabilities": capabilities, "textDocument/documentHighlight": highlights }));
    let editor = f.active(cx);
    f.put_caret(cx, 5);
    f.run(cx, "find.highlightUsages");
    settle(cx, |cx| editor.read_with(cx, |e, _| e.marks(MarkLayer::Usages).len() == 3));
    cx.simulate_keystrokes("ctrl-alt-up");
    assert_eq!(f.caret(cx), 19, "it goes round to the last usage");
    cx.simulate_keystrokes("ctrl-alt-up");
    assert_eq!(f.caret(cx), 13);
    assert_eq!(f.text(cx), text);
    f.close(cx);
}
