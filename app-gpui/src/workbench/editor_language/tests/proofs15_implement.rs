//! Implement Methods with several implement actions at the caret.

use super::*;

/// With two implement actions at the caret Implement Methods offers both under Implement Members,
/// leaving out the other actions, and applies the one chosen; ⌃I runs it, and Escape closes the
/// list without changing the file.
#[gpui_kit::test]
async fn implement_methods_offers_every_implement_action_and_applies_the_chosen_one(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("a.mock", "class A {\n}\n")], "a.mock", Some(json!({})), &[]);
    let a = f.root.join("a.mock");
    let action = |title: &str, kind: &str, text: &str| {
        let edit = edit(&a, vec![text_edit(range(1, 0, 0), text)]);
        json!({ "title": title, "kind": kind, "edit": edit })
    };
    f.answer(json!({ "textDocument/codeAction": [
        action("Implement missing members", "quickfix", "  run() {}\n"),
        action("Generate constructor", "source.generate.constructor", "  A() {}\n"),
        action("Add missing trait items", "quickfix", "  walk() {}\n"),
    ] }));
    f.put_caret(cx, 6);
    f.run(cx, "code.implementMethods");
    let list = f.list(cx);
    assert_eq!(list.read_with(cx, |l, _| l.title().to_owned()), "Implement Members");
    let lines = list.read_with(cx, |l, _| l.lines());
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("Generate constructor")), "{lines:?}");
    cx.simulate_keystrokes("escape");
    settle(cx, |cx| f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()));
    assert_eq!(f.text(cx), "class A {\n}\n", "Escape applies nothing");

    // ⌃I in the editor.
    let editor = f.active(cx);
    cx.update(|window, cx| {
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("ctrl-i");
    let list = f.list(cx);
    let lines = list.read_with(cx, |l, _| l.lines());
    let chosen = lines.iter().position(|line| line.contains("Add missing trait items")).expect("listed");
    for _ in 0..chosen {
        cx.simulate_keystrokes("down");
    }
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| f.text(cx) == "class A {\n  walk() {}\n}\n");
    assert!(f.workbench.read_with(cx, |w, _| w.language_popup::<ListPopup>().is_none()), "the list closes");
    f.close(cx);
}
