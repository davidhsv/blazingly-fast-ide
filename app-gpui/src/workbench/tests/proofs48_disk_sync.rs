//! Synchronize files on frame activation through a window: a file outside the project's folder (no
//! watcher sees it) changes on disk while the window is in the background, and coming back to the
//! window reads it again, unless the setting is off; a document with unsaved edits keeps them and a
//! warning says the file changed; an unchanged document keeps its caret and scroll position, and a
//! window that left its project checks nothing.

use super::proofs48_untitled::close_backend;
use super::*;
use ide_workspace::settings::SettingsScope;

/// The project's folder, the folder beside it, the file there, the window, the file's editor.
type OutsideFile<'a> =
    (tempfile::TempDir, tempfile::TempDir, PathBuf, Entity<Workbench>, Entity<Editor>, &'a mut VisualTestContext);

/// A project window and, beside the project, a folder of its own holding `outside.txt` with `text`,
/// open in the window's editor.
fn outside_file<'a>(cx: &'a mut TestAppContext, text: &str) -> OutsideFile<'a> {
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let file = elsewhere.path().canonicalize().expect("the folder exists").join("outside.txt");
    std::fs::write(&file, text).expect("the file is written");
    let path = file.clone();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    (folder, elsewhere, file, workbench, editor, cx)
}

/// The window goes to the background and comes back.
fn leave_and_come_back(cx: &mut VisualTestContext) {
    cx.deactivate_window();
    cx.run_until_parked();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
}

fn finished(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> u64 {
    workbench.read_with(cx, |w, _| w.disk_syncs_finished())
}

/// A file outside the project changed on disk while the window was in the background: once the window
/// is active again its editor shows the new text. With Synchronize files on frame activation off,
/// coming back reads nothing.
#[gpui_kit::test]
async fn a_file_changed_outside_the_project_is_read_again_when_the_window_is_active_again(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _elsewhere, file, workbench, editor, cx) = outside_file(cx, "one\n");
    cx.deactivate_window();
    cx.run_until_parked();
    std::fs::write(&file, "one two\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    wait_long(cx, "the new text in the editor", |cx| editor.read_with(cx, |e, _| e.text()) == "one two\n");
    assert!(!editor.read_with(cx, |e, _| e.is_dirty()));

    let off = serde_json::Value::Bool(false);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.syncOnFocus", &off, cx))
        .expect("the setting is stored");
    let before = finished(&workbench, cx);
    cx.deactivate_window();
    cx.run_until_parked();
    std::fs::write(&file, "one two three\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    for _ in 0..20 {
        cx.executor().advance_clock(std::time::Duration::from_millis(50));
        cx.run_until_parked();
    }
    assert_eq!(finished(&workbench, cx), before, "no check runs");
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "one two\n");
    close_backend(cx);
}

/// A document with unsaved edits whose file changed on disk keeps the edits, and a warning says that
/// the file changed on disk.
#[gpui_kit::test]
async fn a_changed_file_with_unsaved_edits_keeps_them_and_a_warning_says_so(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _elsewhere, file, workbench, editor, cx) = outside_file(cx, "one\n");
    let off = serde_json::Value::Bool(false);
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &off, cx))
        .expect("the setting is stored");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_input("my ");
    settle(cx, |cx| editor.read_with(cx, |e, _| e.is_dirty()));
    cx.deactivate_window();
    cx.run_until_parked();
    std::fs::write(&file, "changed elsewhere\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    let warning = "outside.txt changed on disk".to_owned();
    wait_long(cx, "the warning", |cx| notification_titles(&workbench, cx).contains(&warning));
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "my one\n");
    assert!(editor.read_with(cx, |e, _| e.is_dirty()));
    close_backend(cx);
}

// src/editor/core/diskSync.test.ts › window disk synchronization › controller preserves the unchanged
// document's selection and scroll, then stops checking
/// Coming back to the window checks the open document; unchanged on disk, it keeps its text, caret
/// and scroll position. Once the window has left its project, coming back checks nothing.
#[gpui_kit::test]
async fn controller_preserves_the_unchanged_documents_selection_and_scroll_then_stops_checking(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, _elsewhere, _file, workbench, editor, cx) = outside_file(cx, &"line\n".repeat(100));
    editor.update(cx, |editor, cx| {
        editor.select(150, 150, cx);
        editor.scroll_to_line(20, cx);
    });
    cx.run_until_parked();
    let (text, top) = editor.read_with(cx, |e, cx| (e.text(), e.top_line(cx)));
    let before = finished(&workbench, cx);
    leave_and_come_back(cx);
    wait_long(cx, "the check", |cx| finished(&workbench, cx) == before + 1);
    editor.read_with(cx, |e, cx| {
        assert_eq!(e.text(), text);
        assert_eq!(e.caret(), 150);
        assert_eq!(e.top_line(cx), top);
    });

    run_action(cx, "file.closeProject", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_none() && w.welcome().is_some()));
    let after = finished(&workbench, cx);
    leave_and_come_back(cx);
    for _ in 0..20 {
        cx.executor().advance_clock(std::time::Duration::from_millis(50));
        cx.run_until_parked();
    }
    assert_eq!(finished(&workbench, cx), after, "a window without a project checks nothing");
}
