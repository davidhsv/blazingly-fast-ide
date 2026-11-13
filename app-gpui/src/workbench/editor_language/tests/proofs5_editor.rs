use super::*;

/// The titles of the window's notifications.
fn notice_titles(f: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    f.notices(cx).into_iter().map(|(title, _)| title).collect()
}

/// Code Cleanup in File with `optimizeImports` and `reformat` turned off applies only the server's fix-all
/// action: the imports keep their order and the file is not reformatted.
#[gpui_kit::test]
async fn code_cleanup_in_file_leaves_out_the_steps_its_argument_turns_off(cx: &mut TestAppContext) {
    let files = [("a.mock", "a\n"), ("c.mock", "import d\nimport c\nlet x = 1;;\n")];
    let capabilities = json!({ "documentFormattingProvider": true });
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({ "capabilities": capabilities.clone() })), &[]);
    let c = f.root.join("c.mock");
    let whole = json!({ "start": { "line": 0, "character": 0 }, "end": { "line": 1, "character": 8 } });
    let sort = json!({
        "title": "Organize imports",
        "kind": "source.organizeImports",
        "edit": edit(&c, vec![text_edit(whole, "import c\nimport d")]),
    });
    let fix_all = json!({
        "title": "Fix all",
        "kind": "source.fixAll",
        "edit": edit(&c, vec![text_edit(range(2, 9, 11), ";")]),
    });
    f.answer(json!({
        "capabilities": capabilities,
        "textDocument/codeAction": [sort, fix_all],
        "textDocument/formatting": [text_edit(range(2, 0, 3), "var")],
    }));
    let arg = json!({ "path": c.to_string_lossy(), "optimizeImports": false, "reformat": false });
    f.run_with(cx, "code.cleanupFile", Some(arg));
    settle(cx, |cx| f.editing(cx, &c));
    settle(cx, |cx| f.text(cx) == "import d\nimport c\nlet x = 1;\n");
    let asked = f.requests("textDocument/codeAction");
    assert_eq!(asked.len(), 1, "only the fix-all step asks the server: {asked:?}");
    assert_eq!(asked[0]["params"]["context"]["only"], json!(["source.fixAll"]));
    assert!(f.requests("textDocument/formatting").is_empty(), "the reformat step is left out");
    assert_eq!(f.text(cx), "import d\nimport c\nlet x = 1;\n");
    f.close(cx);
}

/// Quick Type Definition shows the lines of the type definition the server finds for the name at the caret in a
/// popup with the keyboard, and F4 opens it with the caret on it.
#[gpui_kit::test]
async fn quick_type_definition_shows_the_type_definitions_lines_and_f4_opens_it(cx: &mut TestAppContext) {
    let files = [("a.mock", "let value = make()\n"), ("t.mock", "zero\none\n  Shape\nthree\n")];
    let (f, cx) = open(cx, &files, "a.mock", Some(json!({})), &[]);
    let t = f.root.join("t.mock");
    let at = json!({ "line": 2, "character": 2 });
    f.answer(json!({ "textDocument/typeDefinition": [{ "uri": uri(&t), "range": { "start": at, "end": at } }] }));
    f.put_caret(cx, 6);
    f.run(cx, "code.quickTypeDefinition");
    settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_some()));
    let popup = f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>()).expect("the popup");
    assert_eq!(popup.read_with(cx, |p, _| p.header()), ["t.mock", "t.mock:3"]);
    assert_eq!(popup.read_with(cx, |p, _| p.lines()), ["2  one", "3    Shape", "4  three"]);
    assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the popup has the keyboard");
    cx.simulate_keystrokes("f4");
    settle(cx, |cx| f.editing(cx, &t));
    let opened = f.active(cx);
    settle(cx, |cx| opened.read_with(cx, |e, _| e.caret_position()) == (2, 2));
    assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<DefinitionPopup>().is_none()));
    assert!(f.requests("textDocument/definition").is_empty(), "it asks for the type definition, not the definition");
    f.close(cx);
}

/// Show Editor Hint shows its text as the hint at the caret of the active editor, not as a notification, and the
/// next hint replaces it.
#[gpui_kit::test]
async fn show_editor_hint_shows_the_text_at_the_caret_and_the_next_hint_replaces_it(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "alpha\n")], "a.txt", None, &[]);
    f.run_with(cx, "editor.showHint", Some(json!({ "text": "A hint" })));
    f.wait_hint(cx, "A hint");
    f.run_with(cx, "editor.showHint", Some(json!({ "text": "Careful", "tone": "warning" })));
    f.wait_hint(cx, "Careful");
    let titles = notice_titles(&f, cx);
    assert!(!titles.iter().any(|title| title == "A hint" || title == "Careful"), "{titles:?}");
    f.close(cx);
}

/// Copy Path and Copy Path with Line Number say what they copied in the hint at the caret.
#[gpui_kit::test]
async fn the_copy_path_actions_say_what_they_copied_at_the_caret(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("src/a.txt", "alpha\nbeta\n")], "src/a.txt", None, &[]);
    let absolute = f.root.join("src/a.txt").to_string_lossy().into_owned();
    f.run(cx, "editor.copyPath");
    f.wait_hint(cx, &format!("Copied {absolute}"));
    f.put_caret(cx, 7);
    f.run(cx, "editor.copyPathWithLineNumber");
    f.wait_hint(cx, "Copied src/a.txt:2");
    assert!(!notice_titles(&f, cx).iter().any(|title| title.starts_with("Copied")));
    f.close(cx);
}

/// Paste from History with an empty clipboard history says so in the hint at the caret and opens no dialog.
#[gpui_kit::test]
async fn paste_from_history_with_an_empty_history_says_so_at_the_caret(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "alpha\n")], "a.txt", None, &[]);
    cx.simulate_keystrokes("cmd-shift-v");
    f.wait_hint(cx, "Nothing to paste: the clipboard history is empty");
    assert!(f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
    let titles = notice_titles(&f, cx);
    assert!(!titles.iter().any(|title| title.starts_with("Nothing to paste")), "not a notification: {titles:?}");
    f.close(cx);
}

/// ⌃G with the last occurrence selected says it found none and adds nothing; ⌃G again searches from the top and
/// adds the first occurrence.
#[gpui_kit::test]
async fn ctrl_g_past_the_last_occurrence_says_not_found_then_searches_from_the_top(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "foo x foo\n")], "a.txt", None, &[]);
    let editor = f.active(cx);
    editor.update(cx, |editor, cx| editor.select(6, 9, cx));
    cx.run_until_parked();
    let selections = |cx: &mut VisualTestContext| editor.read_with(cx, |editor, _| editor.selections().to_vec());
    cx.simulate_keystrokes("ctrl-g");
    f.wait_hint(cx, "Not found, press again to search from the top");
    assert_eq!(selections(cx), [(6, 9)]);
    cx.simulate_keystrokes("ctrl-g");
    assert_eq!(selections(cx), [(0, 3), (6, 9)]);
    f.close(cx);
}

/// ⌥/ on a word nothing else completes says so in the hint at the caret and leaves the text as it is; Cyclic Expand
/// Word Backward with no word before the caret says that.
#[gpui_kit::test]
async fn cyclic_expand_word_with_nothing_to_expand_says_so_at_the_caret(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.txt", "zzz q\n")], "a.txt", None, &[]);
    f.put_caret(cx, 5);
    cx.simulate_keystrokes("alt-/");
    f.wait_hint(cx, "No suitable completions found");
    assert_eq!(f.text(cx), "zzz q\n");
    f.put_caret(cx, 4);
    f.run(cx, "code.cyclicExpandWordBackward");
    f.wait_hint(cx, "No word to expand at the caret");
    f.close(cx);
}
