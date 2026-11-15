//! Paste from History's Paste and Delete answer the keys the keymap binds to them.

use super::*;
use crate::editor::view::EditorClipboard;
use ide_workspace::settings::SettingsScope;

/// With Paste bound to Tab and Delete to F6 in the keymap, in the open dialog F6 deletes the
/// chosen entry and Tab pastes it, while Delete and Enter, no longer theirs, do neither.
#[gpui_kit::test]
async fn the_paste_history_keys_are_the_ones_the_keymap_binds(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("notes.txt", "alpha beta gamma\n")], "notes.txt", None);
    let overrides = json!({ "editor.pasteHistory.paste": ["Tab"], "editor.pasteHistory.remove": ["F6"] });
    cx.update(|_, cx| settings::set("w1", SettingsScope::Global, "keymapOverrides", &overrides, cx))
        .expect("the keymap is stored");
    let bound = |cx: &mut VisualTestContext, id: &str, key: &str| {
        cx.update(|_, cx| cx.global::<input::Keymap>().0.bindings_for(id).iter().any(|binding| binding == key))
    };
    settle(cx, |cx| bound(cx, "editor.pasteHistory.paste", "Tab") && bound(cx, "editor.pasteHistory.remove", "F6"));
    let editor = f.active(cx);
    for (from, to) in [(0, 5), (6, 10), (11, 16)] {
        editor.update(cx, |e, cx| e.select(from, to, cx));
        f.run(cx, "edit.copy");
    }
    let history = |cx: &mut VisualTestContext| -> Vec<String> {
        cx.update(|_, cx| {
            cx.default_global::<EditorClipboard>().0.entries().iter().map(|entry| entry.text.clone()).collect()
        })
    };
    let open_dialog = |cx: &mut VisualTestContext| f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_some());
    editor.update(cx, |e, cx| e.select(17, 17, cx));
    f.run(cx, "code.pasteFromHistory");
    assert!(open_dialog(cx));

    cx.simulate_keystrokes("delete");
    assert_eq!(history(cx), ["gamma", "beta", "alpha"], "Delete is no longer bound to Delete");
    cx.simulate_keystrokes("f6");
    assert_eq!(history(cx), ["beta", "alpha"], "F6 deletes the chosen entry");
    cx.simulate_keystrokes("enter");
    assert!(open_dialog(cx), "Enter is no longer bound to Paste");
    assert_eq!(f.text(cx), "alpha beta gamma\n");
    cx.simulate_keystrokes("tab");
    assert!(!open_dialog(cx), "Tab pastes and closes the dialog");
    assert_eq!(f.text(cx), "alpha beta gamma\nbeta");
    f.close(cx);
}
