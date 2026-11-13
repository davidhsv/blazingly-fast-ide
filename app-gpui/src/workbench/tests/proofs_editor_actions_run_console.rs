use super::*;

/// The console find bar's counter as (current, total): `2/3` is (2, 3).
fn counter(view: &Entity<RunView>, cx: &mut VisualTestContext) -> (usize, usize) {
    let text = view.read_with(cx, |view, cx| view.find_counter(cx));
    let parsed = text.split_once('/').and_then(|(at, of)| Some((at.parse().ok()?, of.parse().ok()?)));
    parsed.unwrap_or_else(|| panic!("the counter reads n/m: {text:?}"))
}

/// While the keyboard is in the Run console's find bar, its keys are the bar's and not the IDE
/// keymap's: F3 and ⌘G go to the next match (F3 does not toggle a bookmark in the open editor), a
/// shortcut the bar does not use runs nothing, and Escape closes the bar (it does not send the
/// keyboard to the editor).
#[gpui_kit::test]
async fn the_run_consoles_find_bar_keeps_its_keys_from_the_ide_keymap(cx: &mut TestAppContext) {
    let (workbench, cx, _dirs, root, store) =
        run_project(cx, vec![program("hello", "/usr/bin/printf", "hello\\nhello\\nhello\\n")]);
    let notes = root.join("notes.txt");
    std::fs::write(&notes, "nothing to find\n").expect("the file is written");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(notes.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == notes)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

    store.update(cx, |store, _| store.select_config(Some("hello")));
    run_action(cx, "run.run", None);
    wait_long(cx, "the exit of printf", |cx| {
        store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.config.name == "hello" && !s.is_live()))
    });
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view");
    wait_long(cx, "the output", |cx| {
        view.read_with(cx, |view, cx| view.lines(cx).iter().filter(|line| *line == "hello").count() == 3)
    });

    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd-f");
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, _| view.find_open()));
    let field = view.read_with(cx, |view, _| view.find_field().clone());
    assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)), "the bar's field has the keyboard");
    cx.simulate_input("hello");
    cx.run_until_parked();
    let (first, total) = counter(&view, cx);
    assert!(total >= 3, "every printed line matches: {total}");
    let next = |at: usize| at % total + 1;

    cx.simulate_keystrokes("f3");
    cx.run_until_parked();
    assert_eq!(counter(&view, cx), (next(first), total), "F3 goes to the next match");
    assert!(workbench.read_with(cx, |w, _| w.bookmarks().data().items.is_empty()), "F3 toggled no bookmark");
    cx.simulate_keystrokes("cmd-g");
    cx.run_until_parked();
    assert_eq!(counter(&view, cx), (next(next(first)), total), "⌘G goes to the next match");
    // A key the bar has no use for does not reach the keymap either: ⌘E (Recent Files) opens nothing.
    cx.simulate_keystrokes("cmd-e");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.recent_files().is_none()), "⌘E reached the IDE keymap");
    assert!(view.read_with(cx, |view, _| view.find_open()));

    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.find_open()), "Escape closes the bar");
    assert!(!cx.update(|window, cx| editor.focus_handle(cx).is_focused(window)), "the editor did not take it");
    assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)), "the console has the keyboard back");
    close_backend_project(cx);
}
