use super::*;
use crate::workbench::windows::{self, Target};
use ide_workspace::settings::SettingsScope;

/// The message under the title of the close question shown over the workbench's window, while one is.
fn dialog_body(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |w, cx| w.app_dialog().and_then(|dialog| dialog.read(cx).body().map(str::to_owned)))
}

/// Writes the global setting `key` as `value`.
fn set_setting(cx: &mut VisualTestContext, key: &str, value: bool) {
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, key, &value.into(), cx))
        .expect("the setting is stored");
}

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

/// Whether the window shows the Welcome screen instead of a project.
fn welcomes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.project_tree().is_none() && w.welcome().is_some())
}

/// Whether the run configuration `name` of `store` has a live process.
fn is_running(store: &Entity<RunStore>, name: &str, cx: &mut VisualTestContext) -> bool {
    store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.config.name == name && s.is_live()))
}

/// Closing the last window with `general.confirmExit` on asks to confirm the exit; Exit without
/// "Don't ask again" closes the window and leaves the setting on.
#[gpui_kit::test]
async fn confirms_exit_for_the_last_window_when_enabled(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Confirm Exit"));
    assert_eq!(dialog_body(&workbench, cx).as_deref(), Some("Are you sure you want to exit OpenIDEA?"));
    click_on(cx, "prompt-button-exit");
    settle(cx, |cx| cx.windows().is_empty());
    assert!(cx.read(|cx| crate::platform::settings::global(cx).general.confirm_exit), "the setting stays on");
}

/// With two windows open, closing one of them closes it at once without confirming the exit; a
/// quit confirms the exit once and then closes every window without asking each one again.
#[gpui_kit::test]
async fn does_not_confirm_exit_for_one_of_several_windows_or_during_quit(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(gpui_kit::init);
    assert!(cx.update(|cx| crate::platform::settings::global(cx).general.confirm_exit));
    let main = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    let second = cx.update(|cx| windows::open_window(Target::Empty, cx)).expect("a window opens");
    cx.run_until_parked();
    assert_eq!(cx.update(|cx| windows::windows(cx).len()), 2);

    let mut second_cx = VisualTestContext::from_window(second, cx);
    let second_workbench = second_cx.update(|window, cx| for_window(window, cx)).expect("a workbench");
    assert!(second_cx.simulate_close(), "one of two windows closes at once");
    second_cx.run_until_parked();
    assert!(app_dialog_title(&second_workbench, &mut second_cx).is_none(), "nothing is asked");
    assert!(!second_cx.has_pending_prompt());

    // Quit from the first window: one confirmation, then every window closes.
    let mut main_cx = VisualTestContext::from_window(main, cx);
    let main_workbench = main_cx.update(|window, cx| for_window(window, cx)).expect("a workbench");
    main_cx.update(|window, _| window.activate_window());
    run_action(&mut main_cx, "app.quit", None);
    assert_eq!(app_dialog_title(&main_workbench, &mut main_cx).as_deref(), Some("Confirm Exit"));
    click_on(&mut main_cx, "prompt-button-exit");
    wait_long(&mut main_cx, "every window to close", |cx| cx.windows().is_empty());
}

/// Closing with one process running asks before terminating it, naming it in the title and the
/// message.
#[gpui_kit::test]
async fn asks_before_terminating_a_running_process(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("app", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Process 'app' Is Running"));
    assert_eq!(dialog_body(&workbench, cx).as_deref(), Some("Do you want to terminate the process 'app'?"));
    click_on(cx, "prompt-button-terminate");
    wait_long(cx, "the end of app", |cx| !is_running(&store, "app", cx));
    wait_long(cx, "the Welcome screen", |cx| welcomes(&workbench, cx));
}

/// Closing with two processes running names both in one question; Cancel keeps both running.
#[gpui_kit::test]
async fn names_every_running_process_in_the_prompt(cx: &mut TestAppContext) {
    let configs = vec![program("app", "/bin/sleep", "30"), program("worker", "/bin/sleep", "30")];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("worker")));
    wait_long(cx, "the start of worker", |cx| is_running(&store, "worker", cx));

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Processes Are Running"));
    assert_eq!(
        dialog_body(&workbench, cx).as_deref(),
        Some("Do you want to terminate these 2 processes: app, worker?")
    );
    click_on(cx, "prompt-button-cancel");
    assert!(is_running(&store, "app", cx) && is_running(&store, "worker", cx), "Cancel stops nothing");
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    click_on(cx, "prompt-button-terminate");
    wait_long(cx, "the end of both", |cx| !is_running(&store, "app", cx) && !is_running(&store, "worker", cx));
    wait_long(cx, "the Welcome screen", |cx| welcomes(&workbench, cx));
}

/// With a process running and a file unsaved, Cancel on the process question ends the close there:
/// the unsaved question is never asked, nothing is saved, the process keeps running.
#[gpui_kit::test]
async fn asks_about_processes_before_discarding_unsaved_changes(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, store) = run_project(cx, vec![program("app", "/bin/sleep", "30")]);
    std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    type_into(&workbench, root.join("notes.txt"), "my ", cx);

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Process 'app' Is Running"));
    click_on(cx, "prompt-button-cancel");
    assert!(cx.pending_prompt().is_none(), "the unsaved question is not asked");
    assert!(app_dialog_title(&workbench, cx).is_none());
    assert!(is_running(&store, "app", cx));
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("my notes\n"));
    assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// "Don't ask again" on the process question turns `run.confirmTerminate` off; Terminate stops the
/// process and the window goes back to the Welcome screen.
#[gpui_kit::test]
async fn dont_ask_again_turns_process_confirmation_off(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("app", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    assert!(cx.update(|_, cx| crate::platform::settings::global(cx).run.confirm_terminate));
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Process 'app' Is Running"));
    click_on(cx, "prompt-checkbox");
    click_on(cx, "prompt-button-terminate");
    wait_long(cx, "the end of app", |cx| !is_running(&store, "app", cx));
    wait_long(cx, "the Welcome screen", |cx| welcomes(&workbench, cx));
    assert!(!cx.update(|_, cx| crate::platform::settings::global(cx).run.confirm_terminate), "the setting is off");
}

/// With `run.confirmTerminate` off, closing a window whose process runs asks nothing about it.
#[gpui_kit::test]
async fn does_not_ask_about_processes_when_the_setting_is_off(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("app", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    set_setting(cx, "run.confirmTerminate", false);
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    assert!(app_dialog_title(&workbench, cx).is_none(), "no process question");
    assert!(cx.pending_prompt().is_none());
    wait_long(cx, "the Welcome screen", |cx| welcomes(&workbench, cx));
}

/// Quit with an unsaved file: after the exit is confirmed, Don't Save closes the window and the file
/// keeps what it had on disk.
#[gpui_kit::test]
async fn discards_without_saving(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    type_into(&workbench, root.join("notes.txt"), "my ", cx);
    run_action(cx, "app.quit", None);
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Confirm Exit"));
    click_on(cx, "prompt-button-exit");
    let (title, _) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Don't Save");
    wait_long(cx, "the window to close", |cx| cx.windows().is_empty());
    assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");
}

/// Save All on the unsaved question of a close, when the file cannot be written: the close is called
/// off, the project and the typed text stay, and the failure is reported.
#[cfg(unix)]
#[gpui_kit::test]
async fn keeps_the_window_when_saving_fails_or_the_user_cancels(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("locked/notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    set_setting(cx, "files.autoSave", false);
    let file = root.join("locked/notes.txt");
    type_into(&workbench, file.clone(), "my ", cx);
    let lock = |mode: u32, path: &std::path::Path| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permissions change")
    };
    lock(0o444, &file);
    lock(0o555, &root.join("locked"));

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    let (title, _) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Save All");
    wait_for_notice(&workbench, "Could not save all files", cx);
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("my notes\n"));
    assert_eq!(std::fs::read_to_string(&file).expect("readable"), "notes\n");

    // Cancel keeps it too.
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    lock(0o755, &root.join("locked"));
    lock(0o644, &file);
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Every unsaved document of the window is counted in the close question: two edited files are
/// "2 files".
#[gpui_kit::test]
async fn includes_pending_local_edits_and_restored_dirty_buffers_before_backend_dirty_events(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "a\n"), ("b.txt", "b\n")]);
    cx.update(|window, _| window.activate_window());
    set_setting(cx, "files.autoSave", false);
    type_into(&workbench, root.join("a.txt"), "my ", cx);
    type_into(&workbench, root.join("b.txt"), "my ", cx);
    assert!(!cx.simulate_close());
    cx.run_until_parked();
    let (title, body) = cx.pending_prompt().expect("the unsaved question is asked");
    assert_eq!(title, "Unsaved Changes");
    let project = root.file_name().expect("a name").to_string_lossy().into_owned();
    assert_eq!(body, format!("2 files have unsaved changes in {project}. Save before closing?"));
    cx.simulate_prompt_answer("Cancel");
    cx.run_until_parked();
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Close Project (leaving the project, not the window) with a process running asks about the
/// process first; Cancel keeps the project open and the process running.
#[gpui_kit::test]
async fn confirm_leave_asks_about_running_processes_before_closing(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("app", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "run.runConfig", Some(serde_json::Value::from("app")));
    wait_long(cx, "the start of app", |cx| is_running(&store, "app", cx));
    run_action(cx, "file.closeProject", None);
    assert_eq!(app_dialog_title(&workbench, cx).as_deref(), Some("Process 'app' Is Running"));
    assert_eq!(dialog_body(&workbench, cx).as_deref(), Some("Do you want to terminate the process 'app'?"));
    click_on(cx, "prompt-button-cancel");
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");
    assert!(is_running(&store, "app", cx));
    run_action(cx, "file.closeProject", None);
    click_on(cx, "prompt-button-terminate");
    wait_long(cx, "the end of app", |cx| !is_running(&store, "app", cx));
    wait_long(cx, "the Welcome screen", |cx| welcomes(&workbench, cx));
}
