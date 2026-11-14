use super::*;

/// With the keyboard nowhere when a popup opens, closing the popup (Escape) gives the keyboard to
/// the editor: `editorFocus` is off while the popup has it and on again once it is closed.
#[gpui_kit::test]
async fn returns_focus_to_the_editor_when_the_last_popup_closes_and_focus_was_left_nowhere(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let editor_focused =
        |cx: &mut VisualTestContext| cx.update(|window, cx| editor.focus_handle(cx).is_focused(window));

    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert!(!editor_focused(cx), "the keyboard is nowhere");
    run_action(cx, "view.backgroundTasks", None);
    assert!(workbench.read_with(cx, |w, _| w.background_tasks_popup().is_some()), "the popup is open");
    assert!(!editor_focused(cx) && !context_is_on(cx, "editorFocus"));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.background_tasks_popup().is_none()), "Escape closes it");
    assert!(editor_focused(cx), "the editor has the keyboard");
    assert!(context_is_on(cx, "editorFocus"));
    // Typing goes to the editor without a click.
    cx.simulate_input("my ");
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("my notes\n"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
