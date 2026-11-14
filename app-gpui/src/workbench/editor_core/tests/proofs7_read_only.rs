use super::*;

/// Clicks the status bar's lock of the active editor's file.
fn click_lock(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds("status:editor.readOnly").expect("the lock is drawn");
    cx.simulate_click(bounds.center(), gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// The title of the question the window asks, while it asks one.
fn question(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let dialog = workbench.read_with(cx, |w, _| w.vcs_prompt())?;
    Some(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()))
}

/// A click on the status bar's lock asks to make the file read-only; confirmed, the file is read-only on disk and
/// its editor refuses typing. A click again asks to make it writable, and confirmed both are writable again.
#[gpui_kit::test]
async fn a_click_on_the_status_bars_lock_asks_then_toggles_the_files_read_only_attribute(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
    let file = root.join("notes.txt");
    let editor = open(&workbench, &file, cx);

    click_lock(cx);
    assert_eq!(question(&workbench, cx).as_deref(), Some("Make File Read-only"));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| editor.read_with(cx, |editor, _| editor.is_read_only()));
    assert!(std::fs::metadata(&file).expect("the file").permissions().readonly());
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.simulate_input("x");
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "one\n");

    click_lock(cx);
    assert_eq!(question(&workbench, cx).as_deref(), Some("Make File Writable"));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.is_read_only()));
    assert!(!std::fs::metadata(&file).expect("the file").permissions().readonly());
    close(cx);
}

/// When the attribute cannot be set (the file is gone from the disk), confirming says so in an error
/// notification named after the file, and the editor stays writable.
#[gpui_kit::test]
async fn a_read_only_toggle_that_fails_says_it_cannot_make_the_file_read_only(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
    let file = root.join("notes.txt");
    let editor = open(&workbench, &file, cx);
    std::fs::remove_file(&file).expect("the file is removed");
    run(cx, "file.toggleReadOnly", None);
    assert_eq!(question(&workbench, cx).as_deref(), Some("Make File Read-only"));
    cx.simulate_keystrokes("enter");
    settle(cx, |cx| titles(&workbench, cx).iter().any(|title| title == "Cannot make notes.txt read-only"));
    let level = workbench.read_with(cx, |w, _| {
        w.notifications().items().iter().find(|n| n.title == "Cannot make notes.txt read-only").map(|n| n.level)
    });
    assert_eq!(level, Some(NotificationLevel::Error));
    assert!(!editor.read_with(cx, |editor, _| editor.is_read_only()), "the editor stays writable");
    close(cx);
}
