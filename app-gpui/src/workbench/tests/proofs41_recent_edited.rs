use super::*;

/// Recent Files asked for again shows only the files edited in this window: a file typed in is listed,
/// those only opened are not; asked for a third time it lists them all again.
#[gpui_kit::test]
async fn recent_files_asked_for_again_lists_only_the_files_edited_in_this_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    cx.update(|window, _| window.activate_window());
    let showing = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
    };
    for name in ["src/alpha.rs", "src/beta.rs", "notes.txt"] {
        workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join(name), window, cx));
        settle(cx, |cx| showing(cx) == Some(root.join(name)));
        if name == "src/beta.rs" {
            let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
            cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
            cx.simulate_input("x");
            settle(cx, |cx| editor.read_with(cx, |editor, _| editor.text().starts_with('x')));
        }
    }

    run_action(cx, "navigate.recentFiles", None);
    let popup = workbench.read_with(cx, |w, _| w.recent_files()).expect("Recent Files is open");
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["notes.txt", "src/beta.rs", "src/alpha.rs"]);
    run_action(cx, "navigate.recentFiles", None);
    assert!(popup.read_with(cx, |popup, _| popup.edited_only()));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["src/beta.rs"]);
    run_action(cx, "navigate.recentFiles", None);
    assert!(!popup.read_with(cx, |popup, _| popup.edited_only()));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()).len(), 3);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    close_backend_project(cx);
}
