//! Paste from History with the mouse, the keyboard going back to the editor, the dialog starting
//! afresh each time it opens, and its Paste and Delete answering the keys the keymap binds to them.

use super::*;
use crate::editor::view::EditorClipboard;
use ide_workspace::settings::SettingsScope;

/// Where row `index` of the open Paste from History dialog is drawn.
fn row_at(index: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    cx.run_until_parked();
    let selector: &'static str = Box::leak(format!("paste-history-row-{index}").into_boxed_str());
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("row {index} is drawn")).center()
}

/// The editor of the fixture with three copies in the clipboard history, newest first gamma,
/// beta, alpha, and the caret at the end of the text.
fn with_three_copies(f: &Fixture, cx: &mut VisualTestContext) -> Entity<Editor> {
    let editor = f.active(cx);
    for (from, to) in [(0, 5), (6, 10), (11, 16)] {
        editor.update(cx, |e, cx| e.select(from, to, cx));
        f.run(cx, "edit.copy");
    }
    editor.update(cx, |e, cx| e.select(17, 17, cx));
    editor
}

fn editor_has_the_keyboard(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| editor.focus_handle(cx).is_focused(window))
}

/// A click chooses a row, a click with the secondary modifier adds one, a click with Shift chooses
/// the rows down to it, and a double click pastes the row clicked alone and gives the keyboard back
/// to the editor.
#[gpui_kit::test]
async fn paste_from_history_answers_clicks_and_a_double_click_pastes_the_row(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("notes.txt", "alpha beta gamma\n")], "notes.txt", None);
    let editor = with_three_copies(&f, cx);
    f.run(cx, "code.pasteFromHistory");
    let dialog = f.workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open");
    let chosen = |cx: &mut VisualTestContext| -> Vec<String> {
        dialog.read_with(cx, |d, _| d.chosen().into_iter().map(str::to_owned).collect())
    };
    assert_eq!(chosen(cx), ["gamma"]);
    let mac = cfg!(target_os = "macos");
    let secondary = Modifiers { platform: mac, control: !mac, ..Modifiers::default() };
    let (first, second, third) = (row_at(0, cx), row_at(1, cx), row_at(2, cx));
    cx.simulate_click(second, Modifiers::default());
    assert_eq!(chosen(cx), ["beta"]);
    cx.simulate_click(third, secondary);
    assert_eq!(chosen(cx), ["beta", "alpha"]);
    cx.simulate_click(first, Modifiers::default());
    cx.simulate_click(third, Modifiers::shift());
    assert_eq!(chosen(cx), ["gamma", "beta", "alpha"]);

    let position = row_at(1, cx);
    let (button, modifiers) = (gpui_kit::MouseButton::Left, Modifiers::default());
    cx.simulate_event(gpui_kit::MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
    assert!(f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()), "the dialog closes");
    assert_eq!(f.text(cx), "alpha beta gamma\nbeta", "only the double-clicked row is pasted");
    assert!(editor_has_the_keyboard(&editor, cx));
    f.close(cx);
}

/// Closed with Escape the dialog gives the keyboard back to the editor; opened again it starts with
/// an empty filter, every copy listed and the newest chosen.
#[gpui_kit::test]
async fn paste_from_history_opens_afresh_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, &[("notes.txt", "alpha beta gamma\n")], "notes.txt", None);
    let editor = with_three_copies(&f, cx);
    f.run(cx, "code.pasteFromHistory");
    cx.simulate_input("al");
    cx.simulate_keystrokes("shift-down");
    let dialog = f.workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open");
    assert_eq!(dialog.read_with(cx, |d, _| d.rows().len()), 1, "the filter keeps alpha");
    assert!(!editor_has_the_keyboard(&editor, cx), "the dialog has the keyboard");
    cx.simulate_keystrokes("escape");
    assert!(f.workbench.read_with(cx, |w, _| w.paste_history_dialog().is_none()));
    assert!(editor_has_the_keyboard(&editor, cx), "Escape gives the keyboard back");
    assert_eq!(f.text(cx), "alpha beta gamma\n", "nothing is pasted");

    f.run(cx, "code.pasteFromHistory");
    let dialog = f.workbench.read_with(cx, |w, _| w.paste_history_dialog()).expect("the dialog is open again");
    assert_eq!(dialog.read_with(cx, |d, cx| d.field().read(cx).text().to_owned()), "");
    let rows: Vec<String> = dialog.read_with(cx, |d, _| d.rows().iter().map(|row| row.text.clone()).collect());
    assert_eq!(rows, ["gamma", "beta", "alpha"]);
    assert_eq!(dialog.read_with(cx, |d, _| d.chosen().into_iter().map(str::to_owned).collect::<Vec<_>>()), ["gamma"]);
    cx.simulate_keystrokes("enter");
    assert_eq!(f.text(cx), "alpha beta gamma\ngamma");
    assert!(editor_has_the_keyboard(&editor, cx), "pasting gives the keyboard back");
    f.close(cx);
}

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
    settle(cx, "the paste history's keys", |cx| {
        bound(cx, "editor.pasteHistory.paste", "Tab") && bound(cx, "editor.pasteHistory.remove", "F6")
    });
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
