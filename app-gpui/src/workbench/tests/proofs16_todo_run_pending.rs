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

/// The Run tool window's stripe button shows how many processes run: one while a long program runs, none
/// once it is stopped.
///
/// Depends on `Workbench::stripe_badge(&self, id: &str) -> Option<usize>`, the badge the stripe button
/// of tool window `id` draws.
#[gpui_kit::test]
async fn the_run_stripe_button_shows_the_number_of_running_processes(cx: &mut TestAppContext) {
    use crate::platform::actions::execute_action_or_throw;
    use crate::run::configs::blank_config;
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let nap = ide_run::RunConfig {
        command: "/bin/sleep".to_owned(),
        args: vec!["30".to_owned()],
        ..blank_config("nap", "nap", "program")
    };
    ide_run::save_user_configs(&root, &[nap]).expect("the configuration is saved");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    let long = |cx: &mut VisualTestContext, what: &str, done: &dyn Fn(&mut VisualTestContext) -> bool| {
        for _ in 0..3000 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what} did not happen within thirty seconds");
    };
    long(cx, "listing the configurations", &|cx| store.read_with(cx, |store, _| store.loaded));
    store.update(cx, |store, _| store.select_config(Some("nap")));
    cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx)).expect("it runs");
    long(cx, "the program runs", &|cx| store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.is_live())));
    assert_eq!(workbench.read_with(cx, |w, _| w.stripe_badge(RUN_TOOL_WINDOW)), Some(1));
    cx.update(|window, cx| execute_action_or_throw("run.stop", None, window, cx)).expect("it stops");
    long(cx, "the program ends", &|cx| store.read_with(cx, |store, _| !store.sessions.iter().any(|s| s.is_live())));
    assert_eq!(workbench.read_with(cx, |w, _| w.stripe_badge(RUN_TOOL_WINDOW)), None);
}
