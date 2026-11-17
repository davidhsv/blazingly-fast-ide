use super::*;

/// template fields › leaves Enter to the editor in a completion's snippet: a server's tag item
/// `<dependency>$1</dependency>$0` accepted from the lookup leaves the caret between the tags; Enter
/// there breaks the line as the editor does and does not jump to the snippet's end, and Tab still
/// does (the line break went into the empty field, which grows with what is typed at it).
#[gpui_kit::test]
async fn leaves_enter_to_the_editor_in_a_completions_snippet(cx: &mut TestAppContext) {
    let tag = json!({
        "label": "dependency",
        "kind": 15,
        "insertText": "<dependency>${1}</dependency>${0}",
        "insertTextFormat": 2,
    });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", json!({ "textDocument/completion": [tag] }));
    let editor = f.editor(cx).expect("an editor");
    let caret = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.caret());
    // The item replaces the word typed, as the server's `call` item replaces `ca`.
    cx.simulate_input("dep");
    f.run(cx, "code.completion");
    settle(cx, "the lookup's rows", |cx| !f.rows(cx).is_empty());
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "<dependency></dependency>");
    let inside = "<dependency>".encode_utf16().count() as u32;
    assert_eq!(caret(cx), inside);
    assert!(f.workbench.read_with(cx, |w, _| w.template_session(&editor).is_some()), "the snippet's fields are active");

    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let text = f.text(cx);
    assert!(text.starts_with("<dependency>\n") && text.trim_end().ends_with("</dependency>"), "{text:?}");
    assert_ne!(caret(cx), text.encode_utf16().count() as u32, "Enter did not move to the snippet's end");
    assert!(caret(cx) > inside, "the caret is on the new line");

    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let text = f.text(cx);
    assert_eq!(caret(cx), text.encode_utf16().count() as u32, "Tab moves to the snippet's end");
    assert!(f.workbench.read_with(cx, |w, _| w.template_session(&editor).is_none()), "the last field ends it");
    f.close(cx);
}
