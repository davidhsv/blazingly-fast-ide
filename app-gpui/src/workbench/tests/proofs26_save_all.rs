use super::*;
use ide_workspace::settings::SettingsScope;

/// Opens `file` of the project in the window's editor and types `typed` at its start.
fn type_into(workbench: &Entity<Workbench>, file: PathBuf, typed: &str, cx: &mut VisualTestContext) {
    let path = file.clone();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
    cx.update(|window, cx| {
        let editor = workbench.read(cx).editor().cloned().expect("an editor");
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_input(typed);
}

/// Whether the tab of `path` holds changes that are not on disk.
fn is_unsaved(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, cx| w.editors().iter().any(|e| e.read(cx).path() == path && e.read(cx).is_dirty()))
}

/// A project with autosave off, so that only Save All writes what is typed.
fn project_without_autosave<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx))
        .expect("the setting is stored");
    (folder, root, workbench, cx)
}

/// File › Save All writes every unsaved tab, not only the one in front: both files hold the typed
/// text and neither tab is unsaved any more.
#[gpui_kit::test]
async fn save_all_writes_every_unsaved_tab_not_only_the_active_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project_without_autosave(cx, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    type_into(&workbench, root.join("a.txt"), "my ", cx);
    type_into(&workbench, root.join("b.txt"), "my ", cx);
    assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 2);

    run_action(cx, "file.save", None);
    wait_long(cx, "both files to be written", |cx| {
        workbench.read_with(cx, |w, cx| w.unsaved_count(cx)) == 0
            && std::fs::read_to_string(root.join("a.txt")).is_ok_and(|text| text == "my a\n")
            && std::fs::read_to_string(root.join("b.txt")).is_ok_and(|text| text == "my b\n")
    });
    assert!(!is_unsaved(&workbench, &root.join("a.txt"), cx));
    assert!(!is_unsaved(&workbench, &root.join("b.txt"), cx));
    assert!(notification_titles(&workbench, cx).is_empty(), "{:?}", notification_titles(&workbench, cx));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// File › Save All with a file that cannot be written: the user is told "Could not save" with the
/// reason, that tab stays unsaved with its text, and the other tab is written all the same.
#[cfg(unix)]
#[gpui_kit::test]
async fn save_all_tells_the_user_when_a_file_cannot_be_saved_and_keeps_that_tab_unsaved(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        project_without_autosave(cx, &[("a.txt", "a\n"), ("locked/notes.txt", "notes\n")]);
    let locked = root.join("locked/notes.txt");
    type_into(&workbench, locked.clone(), "my ", cx);
    type_into(&workbench, root.join("a.txt"), "my ", cx);
    let lock = |mode: u32, path: &std::path::Path| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permissions change")
    };
    lock(0o444, &locked);
    lock(0o555, &root.join("locked"));

    run_action(cx, "file.save", None);
    wait_for_notice(&workbench, "Could not save", cx);
    wait_long(cx, "the writable file to be written", |cx| {
        !is_unsaved(&workbench, &root.join("a.txt"), cx)
            && std::fs::read_to_string(root.join("a.txt")).is_ok_and(|text| text == "my a\n")
    });
    let body = workbench.read_with(cx, |w, _| {
        w.notifications().items().iter().find(|n| n.title == "Could not save").and_then(|n| n.body.clone())
    });
    assert!(body.is_some_and(|body| !body.is_empty()), "the notice says why");
    assert!(is_unsaved(&workbench, &locked, cx), "the tab that failed stays unsaved");
    assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);
    let kept = workbench
        .read_with(cx, |w, cx| w.editors().iter().find(|e| e.read(cx).path() == locked).map(|e| e.read(cx).text()));
    assert_eq!(kept.as_deref(), Some("my notes\n"));
    assert_eq!(std::fs::read_to_string(&locked).expect("readable"), "notes\n");
    lock(0o755, &root.join("locked"));
    lock(0o644, &locked);
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
