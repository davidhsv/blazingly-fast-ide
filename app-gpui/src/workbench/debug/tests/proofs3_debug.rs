use super::*;
use crate::debugger::breakpoints::special::SpecialBreakpoint;

/// Opens `name` of the fixture's project, written with `text`, in the editor.
fn open_file(fixture: &Fixture, name: &str, text: &str, cx: &mut VisualTestContext) {
    let path = fixture.root.join(name);
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(&path, text).expect("the file is written");
    fixture.workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
    let shown =
        |cx: &mut VisualTestContext| fixture.editor(cx).read_with(cx, |editor, _| editor.path() == path.as_path());
    long(cx, "the file in the editor", &shown);
}

/// The method breakpoints and field watchpoints View Breakpoints lists, as `method name:line` and
/// `field label:line` (0-based lines); the dialog is closed again.
fn listed_special(fixture: &Fixture, cx: &mut VisualTestContext) -> Vec<String> {
    run(cx, "debug.viewBreakpoints");
    let open = |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().is_some());
    long(cx, "the Breakpoints dialog", &open);
    let dialog = fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().cloned()).expect("the dialog");
    let listed = dialog.read_with(cx, |dialog, _| {
        dialog
            .rows()
            .iter()
            .filter_map(|row| match row {
                DialogRow::Special { bp: SpecialBreakpoint::Method(method), .. } => {
                    Some(format!("method {}:{}", method.name, method.line.map_or(-1, i64::from)))
                }
                DialogRow::Special { bp: SpecialBreakpoint::Field(field), .. } => {
                    Some(format!("field {}:{}", field.data_id, field.line.map_or(-1, i64::from)))
                }
                _ => None,
            })
            .collect()
    });
    dialog.update(cx, |_, cx| cx.emit(BreakpointsDialogEvent::Close));
    long(cx, "the dialog to close", &|cx| !open(cx));
    listed
}

/// The fake adapter, also answering Flutter's `hotReload` and `hotRestart` requests: each prints
/// `<request> done` to the debug console.
fn hot_reloading_adapter() -> String {
    let script = FAKE_ADAPTER.replace(
        "    elif command == 'pause':",
        "    elif command in ('hotReload', 'hotRestart'):\n        respond(request)\n        \
         event('output', {'category': 'stdout', 'output': command + ' done\\n'})\n    elif command == 'pause':",
    );
    assert!(script.contains("('hotReload', 'hotRestart')"), "{script}");
    script
}

/// As the fixture's `open`, with the hot-reloading fake adapter as the `dart` adapter: a user
/// definition of the language `dart` replaces the bundled one, so its sessions are Dart sessions.
/// `program` is `lib/main.dart`, debugged through the configuration `main`.
fn open_dart<'a>(cx: &'a mut TestAppContext, program: &str) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let script = home.path().join("fake_dap.py");
    std::fs::write(&script, hot_reloading_adapter()).expect("the adapter script is written");
    let languages = dirs.config.join("languages");
    std::fs::create_dir_all(&languages).expect("the languages folder is made");
    let toml = format!(
        "id = \"dart\"\nname = \"Dart\"\nextensions = [\"dart\"]\nline-comment = \"//\"\n\n\
         [debugger]\nadapter = \"dart\"\ncommand = \"python3\"\nargs = [{:?}]\n",
        script.to_string_lossy(),
    );
    std::fs::write(languages.join("dart.toml"), toml).expect("the language is written");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::create_dir_all(root.join("lib")).expect("the folder is made");
    std::fs::write(root.join("lib/main.dart"), program).expect("the program is written");
    let config = ide_run::RunConfig {
        command: "dart".to_owned(),
        args: vec!["run".to_owned(), "lib/main.dart".to_owned()],
        debug_adapter: Some("dart".to_owned()),
        ..blank_config("main", "main", "program")
    };
    ide_run::save_user_configs(&root, &[config]).expect("the configuration is saved");
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        super::super::super::register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        super::super::super::menu::init(cx);
        cx.global::<Backend>().host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    long(cx, "the project to open", &|cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let file = root.join("lib/main.dart");
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
    long(cx, "the editor", &|cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    (Fixture { workbench, root, _folders: (home, folder) }, cx)
}

// Not from a spec: Hot Reload and Hot Restart need a running Dart session; on one they reach its
// adapter, whose answer shows in the debug console.
#[gpui_kit::test]
async fn hot_reload_and_hot_restart_reach_the_adapter_of_a_running_dart_session(cx: &mut TestAppContext) {
    let (fixture, cx) = open_dart(cx, "start\nloop\n");
    assert!(!context(cx, "debugSessionDart"));
    let ran = cx.update(|window, cx| execute_action_or_throw("debug.hotReload", None, window, cx));
    assert!(ran.is_err(), "Hot Reload is not available without a Dart session");

    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the program to run into its loop", &|cx| context(cx, "debugSessionRunning"));
    assert!(context(cx, "debugSessionDart"));
    run(cx, "debug.hotReload");
    long(cx, "the adapter's answer to Hot Reload", &|cx| fixture.console(cx).contains("hotReload done"));
    run(cx, "debug.hotRestart");
    long(cx, "the adapter's answer to Hot Restart", &|cx| fixture.console(cx).contains("hotRestart done"));
    assert!(!notification_titles(&fixture, cx).iter().any(|t| t.starts_with("Hot ")), "nothing failed");

    run(cx, "debug.stop");
    long(cx, "the stop", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}

/// The title of the question the window shows.
fn dialog_title(fixture: &Fixture, cx: &mut VisualTestContext) -> Option<String> {
    fixture.workbench.read_with(cx, |w, cx| w.app_dialog().map(|dialog| dialog.read(cx).title().to_owned()))
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();
}

// Not from a spec: Quit with a program still running under the debugger confirms the exit, then
// asks to terminate it, naming it; Cancel keeps the window and the session running. Closing the
// project window asks the same; Terminate ends the session and the window shows the Welcome screen.
#[gpui_kit::test]
async fn quit_with_a_running_debug_session_asks_to_terminate_it(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "start\nloop\n");
    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the program to run into its loop", &|cx| context(cx, "debugSessionRunning"));

    run(cx, "app.quit");
    long(cx, "the exit question", &|cx| dialog_title(&fixture, cx).as_deref() == Some("Confirm Exit"));
    click(cx, "prompt-button-exit");
    long(cx, "the question about the running program", &|cx| {
        dialog_title(&fixture, cx).as_deref() == Some("Process 'main' Is Running")
    });
    click(cx, "prompt-button-cancel");
    assert_eq!(cx.windows().len(), 1, "Cancel keeps the window");
    assert_eq!(fixture.status(cx), Some(SessionStatus::Running), "and the program running");

    assert!(!cx.simulate_close());
    long(cx, "the question on closing the window", &|cx| {
        dialog_title(&fixture, cx).as_deref() == Some("Process 'main' Is Running")
    });
    click(cx, "prompt-button-terminate");
    long(cx, "the end of the debugged program", &|cx| fixture.status(cx) != Some(SessionStatus::Running));
    long(cx, "the Welcome screen", &|cx| fixture.workbench.read_with(cx, |w, _| w.welcome().is_some()));
    assert_eq!(cx.windows().len(), 1, "closing the project keeps the window");
}

// Not from a spec: Toggle Method Breakpoint on a Python function's declaration sets a method
// breakpoint named after it, listed in View Breakpoints; the action again on that line removes it.
#[gpui_kit::test]
async fn toggle_method_breakpoint_on_a_declaration_sets_and_removes_a_method_breakpoint(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    open_file(&fixture, "calc.py", "x = 1\ndef run():\n    pass\n", cx);
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleMethodBreakpoint");
    cx.run_until_parked();
    assert_eq!(listed_special(&fixture, cx), ["method run:1"]);
    assert!(!notification_titles(&fixture, cx).iter().any(|t| t == "No method declaration on this line"));

    run(cx, "debug.toggleMethodBreakpoint");
    cx.run_until_parked();
    assert!(listed_special(&fixture, cx).is_empty(), "the second toggle removes it");
    close_project(&fixture, cx);
}

// Not from a spec: Toggle Field Watchpoint on a Java field declaration sets a watchpoint on the
// field, identified by its class, listed in View Breakpoints; the action again removes it.
#[gpui_kit::test]
async fn toggle_field_watchpoint_on_a_java_field_sets_and_removes_a_watchpoint(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    open_file(&fixture, "src/a/A.java", "package a;\nclass A {\n  private int count;\n}\n", cx);
    fixture.caret_to(2, cx);
    run(cx, "debug.toggleFieldWatchpoint");
    cx.run_until_parked();
    assert_eq!(listed_special(&fixture, cx), ["field a.A#count:2"]);
    assert!(!notification_titles(&fixture, cx).iter().any(|t| t == "No field declaration on this line"));

    run(cx, "debug.toggleFieldWatchpoint");
    cx.run_until_parked();
    assert!(listed_special(&fixture, cx).is_empty(), "the second toggle removes it");
    close_project(&fixture, cx);
}
