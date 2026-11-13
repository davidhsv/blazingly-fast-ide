use super::*;

/// Quit with unsaved changes asks about them first; Cancel keeps the window and the typed text.
#[gpui_kit::test]
async fn quit_with_unsaved_changes_asks_and_cancel_keeps_the_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.simulate_input("my ");
    run_action(cx, "app.quit", None);
    let (title, _) = cx.pending_prompt().expect("the question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert_eq!(cx.windows().len(), 1, "the window stays");
    let text = workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text()));
    assert_eq!(text.as_deref(), Some("my notes\n"));
    assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
