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

/// Another project opened in the window: the files edited in the one before are not listed by Show edited
/// only, there nor once the first project is opened again.
#[gpui_kit::test]
async fn the_edited_files_of_the_project_before_are_not_listed_after_another_is_opened(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = search_project(cx);
    cx.update(|window, _| window.activate_window());
    let showing = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).path().to_path_buf()))
    };
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/beta.rs"), window, cx));
    settle(cx, |cx| showing(cx) == Some(root.join("src/beta.rs")));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("x");
    settle(cx, |cx| editor.read_with(cx, |editor, _| editor.text().starts_with('x')));
    run_action(cx, "navigate.recentFiles", None);
    run_action(cx, "navigate.recentFiles", None);
    let popup = workbench.read_with(cx, |w, _| w.recent_files()).expect("Recent Files is open");
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["src/beta.rs"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    drop(popup);

    let other = tempfile::tempdir().expect("a temporary folder");
    let other_root = other.path().canonicalize().expect("the folder exists");
    std::fs::write(other_root.join("gamma.rs"), "pub fn gamma() {}\n").expect("the file is written");
    workbench.update_in(cx, |w, window, cx| w.open_project(other_root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(other_root.as_path())));
    run_action(cx, "navigate.recentFiles", None);
    run_action(cx, "navigate.recentFiles", None);
    let popup = workbench.read_with(cx, |w, _| w.recent_files()).expect("Recent Files is open");
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
    assert!(popup.read_with(cx, |popup, _| popup.edited_only()));
    assert!(popup.read_with(cx, |popup, _| popup.lines()).is_empty(), "nothing was edited in this project");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    drop(popup);

    // Back in the first project, its file only opened this time is not listed as edited.
    workbench.update_in(cx, |w, window, cx| w.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.project_root(cx).as_deref() == Some(root.as_path())));
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/beta.rs"), window, cx));
    settle(cx, |cx| showing(cx) == Some(root.join("src/beta.rs")));
    run_action(cx, "navigate.recentFiles", None);
    let popup = workbench.read_with(cx, |w, _| w.recent_files()).expect("Recent Files is open");
    settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_loading()));
    assert!(popup.read_with(cx, |popup, _| popup.lines()).contains(&"src/beta.rs".to_owned()), "it is recent");
    run_action(cx, "navigate.recentFiles", None);
    assert!(popup.read_with(cx, |popup, _| popup.edited_only()));
    assert!(popup.read_with(cx, |popup, _| popup.lines()).is_empty(), "the edit was made before the project was left");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    close_backend_project(cx);
}
