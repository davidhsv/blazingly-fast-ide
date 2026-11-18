use super::*;

/// The TODO tool window's preview of a file that can be written is an editor that edits it; choosing
/// another item of the same file moves that editor's caret to the item instead of making a new one.
#[gpui_kit::test]
async fn the_todo_preview_edits_the_file_and_moves_its_caret_to_another_item_of_the_same_file(cx: &mut TestAppContext) {
    use crate::features_a::todo_view::TodoView;
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::create_dir(root.join("src")).expect("a folder");
    std::fs::write(root.join("src/a.rs"), "// TODO first\nfn a() {}\n// FIXME second\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    run_action(cx, "toolwindow.todo", None);
    let view: Entity<TodoView> =
        workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO tool window is shown");
    settle(cx, |cx| !view.update(cx, |view, cx| view.lines(cx)).is_empty());
    let preview = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.preview().cloned()).and_then(|preview| preview.downcast::<Editor>().ok())
    };

    // Row 0 is the file, rows 1 and 2 its two items.
    click_on(cx, "todo-row-1");
    settle(cx, |cx| preview(cx).is_some_and(|e| e.read_with(cx, |e, _| e.caret_position()) == (0, 3)));
    let first = preview(cx).expect("the preview is an editor");
    assert!(!first.read_with(cx, |editor, _| editor.status().read_only), "a file that can be written is edited");

    click_on(cx, "todo-row-2");
    settle(cx, |cx| preview(cx).is_some_and(|e| e.read_with(cx, |e, _| e.caret_position()) == (2, 3)));
    let second = preview(cx).expect("the preview is an editor");
    assert_eq!(second.entity_id(), first.entity_id(), "the same editor shows the other item");
}

/// An edit typed in the TODO preview stays when the preview moves to another file: Save All writes it
/// to disk, and previewing the file again shows the edited editor.
#[gpui_kit::test]
async fn an_edit_in_the_todo_preview_survives_another_file_and_save_all_writes_it(cx: &mut TestAppContext) {
    use crate::features_a::todo_view::TodoView;
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::create_dir(root.join("src")).expect("a folder");
    std::fs::write(root.join("src/a.rs"), "// TODO first\nfn a() {}\n").expect("the file is written");
    std::fs::write(root.join("src/b.rs"), "// TODO second\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    run_action(cx, "toolwindow.todo", None);
    let view: Entity<TodoView> =
        workbench.read_with(cx, |w, _| w.todo_window_view()).expect("the TODO tool window is shown");
    settle(cx, |cx| view.update(cx, |view, cx| view.lines(cx)).len() == 4);
    let preview = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.preview().cloned()).and_then(|preview| preview.downcast::<Editor>().ok())
    };
    let previews = |name: &str, cx: &mut VisualTestContext| {
        preview(cx).is_some_and(|e| e.read_with(cx, |e, _| e.path().ends_with(name)))
    };

    // Rows: a.rs, its item, b.rs, its item.
    click_on(cx, "todo-row-1");
    settle(cx, |cx| previews("a.rs", cx));
    let edited = preview(cx).expect("the preview is an editor");
    cx.update(|window, cx| window.focus(&edited.focus_handle(cx), cx));
    cx.simulate_input("x");
    assert_eq!(edited.read_with(cx, |e, _| e.text()), "// xTODO first\nfn a() {}\n");

    click_on(cx, "todo-row-3");
    settle(cx, |cx| previews("b.rs", cx));
    run_action(cx, "file.save", None);
    wait_long(cx, "a.rs to be saved", |_| {
        std::fs::read_to_string(root.join("src/a.rs")).is_ok_and(|text| text == "// xTODO first\nfn a() {}\n")
    });

    click_on(cx, "todo-row-1");
    settle(cx, |cx| previews("a.rs", cx));
    assert_eq!(preview(cx).expect("an editor").read_with(cx, |e, _| e.text()), "// xTODO first\nfn a() {}\n");
}
