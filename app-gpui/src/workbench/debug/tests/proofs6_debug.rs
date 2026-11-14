use super::*;
use crate::debugger::breakpoints::model::BreakpointStatus;
use crate::debugger::breakpoints::special::SpecialBreakpoint;
use crate::debugger::driver::JavaDebugServerPort;
use crate::vcs_ops::menu::MenuItem;
use crate::workbench::notification_balloons::EntryAction;
use crate::workbench::notifications::IdeNotification;

/// What the fake adapter says of the breakpoint it unbinds once the program is configured.
const UNBOUND: &str = "No code at this line";

/// The requests [`capable_adapter`] answers beyond the fixture's: Reset Frame takes the program back to its first
/// line; method breakpoints, field watchpoints and exception filters are answered verified and printed to the debug
/// console as `<kind>: <names>`; a variable is watched under the data id `<name>@main`.
const CAPABLE_REQUESTS: &str = r#"    elif command == 'restartFrame':
        respond(request)
        current = 1
        stop('restart')
    elif command == 'setFunctionBreakpoints':
        names = [b['name'] for b in arguments.get('breakpoints', [])]
        respond(request, {'breakpoints': [{'verified': True} for _ in names]})
        event('output', {'category': 'stdout', 'output': 'function breakpoints: ' + ','.join(names) + '\n'})
    elif command == 'setDataBreakpoints':
        ids = [b['dataId'] for b in arguments.get('breakpoints', [])]
        respond(request, {'breakpoints': [{'verified': True} for _ in ids]})
        event('output', {'category': 'stdout', 'output': 'data breakpoints: ' + ','.join(ids) + '\n'})
    elif command == 'dataBreakpointInfo':
        name = arguments['name']
        respond(request, {'dataId': name + '@main', 'description': name, 'accessTypes': ['write']})
    elif command == 'setExceptionBreakpoints':
        filters = arguments.get('filters', [])
        respond(request, {'breakpoints': []})
        event('output', {'category': 'stdout', 'output': 'exception filters: ' + ','.join(filters) + '\n'})
    elif command == 'pause':"#;

/// The fake adapter, also able to restart frames and to set method breakpoints, field watchpoints and exception
/// breakpoints (the filter `raised`, "Raised Exceptions", off by default), with a second thread `worker`.
fn capable_adapter() -> String {
    let caps = "'supportsSetVariable': True, 'supportsRestartFrame': True, 'supportsFunctionBreakpoints': True, \
                'supportsDataBreakpoints': True, 'exceptionBreakpointFilters': \
                [{'filter': 'raised', 'label': 'Raised Exceptions', 'default': False}]}";
    let one_thread = "{'threads': [{'id': 1, 'name': 'main'}]}";
    let two_threads = "{'threads': [{'id': 1, 'name': 'main'}, {'id': 2, 'name': 'worker'}]}";
    let script = FAKE_ADAPTER
        .replace("'supportsSetVariable': True}", caps)
        .replace(one_thread, two_threads)
        .replace("    elif command == 'pause':", CAPABLE_REQUESTS);
    assert!(script.contains("'supportsDataBreakpoints': True") && script.contains("'worker'"), "{script}");
    assert!(script.contains("'dataBreakpointInfo'"), "{script}");
    script
}

/// The fake adapter, which reports the first breakpoint unbound once the program is configured, before it runs.
fn unverifying_adapter() -> String {
    let changed = format!(
        "            launch = None\n        event('breakpoint', {{'reason': 'changed', 'breakpoint': \
         {{'id': 1, 'verified': False, 'message': '{UNBOUND}'}}}})\n        run(1)"
    );
    let script = FAKE_ADAPTER.replace("            launch = None\n        run(1)", &changed);
    assert!(script.contains("'reason': 'changed'"), "{script}");
    script
}

/// Replaces the adapter script; a session started from now on runs `script`.
fn use_adapter(fixture: &Fixture, script: &str) {
    std::fs::write(fixture._folders.0.path().join("fake_dap.py"), script).expect("the adapter is written");
}

/// Opens `name` of the fixture's project, written with `text`, in the editor.
fn open_file(fixture: &Fixture, name: &str, text: &str, cx: &mut VisualTestContext) {
    let path = fixture.root.join(name);
    std::fs::write(&path, text).expect("the file is written");
    fixture.workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
    let shown = |cx: &mut VisualTestContext| {
        let editor = fixture.workbench.read_with(cx, |workbench, _| workbench.editor().cloned());
        editor.is_some_and(|editor| editor.read_with(cx, |editor, _| editor.path() == path.as_path()))
    };
    long(cx, "the file in the editor", &shown);
}

/// Stops at the breakpoint on the second line of the fixture's program.
fn stop_at_the_breakpoint(fixture: &Fixture, cx: &mut VisualTestContext) {
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    fixture.select_main(cx);
    run(cx, "run.debug");
    wait_execution_at(fixture, 1, "the stop at the breakpoint", cx);
}

fn stop_session(fixture: &Fixture, cx: &mut VisualTestContext) {
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
}

/// The Breakpoints dialog, opened when it is not.
fn breakpoints_dialog(fixture: &Fixture, cx: &mut VisualTestContext) -> Entity<BreakpointsDialog> {
    let open = |cx: &mut VisualTestContext| fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().is_some());
    if !open(cx) {
        run(cx, "debug.viewBreakpoints");
        long(cx, "the Breakpoints dialog", &open);
    }
    fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().cloned()).expect("the dialog")
}

fn close_dialog(fixture: &Fixture, dialog: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) {
    dialog.update(cx, |_, cx| cx.emit(BreakpointsDialogEvent::Close));
    long(cx, "the dialog to close", &|cx| fixture.workbench.read_with(cx, |w, _| w.breakpoints_dialog().is_none()));
}

fn rows(dialog: &Entity<BreakpointsDialog>, cx: &mut VisualTestContext) -> Vec<DialogRow> {
    dialog.read_with(cx, |dialog, _| dialog.rows().to_vec())
}

/// The labels of a menu's own rows, with whether each is disabled.
fn menu_rows<C>(items: &[MenuItem<C>]) -> Vec<(String, bool)> {
    items
        .iter()
        .filter_map(|item| match item {
            MenuItem::Item(entry) => Some((entry.label.clone(), entry.disabled)),
            _ => None,
        })
        .collect()
}

fn frames_view(fixture: &Fixture, cx: &mut VisualTestContext) -> Entity<FramesView> {
    let view = fixture.view(cx).expect("the Debug tool window");
    view.read_with(cx, |view, _| view.frames().clone())
}

/// Whether the frame menu offers Reset Frame enabled.
fn reset_frame_enabled(fixture: &Fixture, cx: &mut VisualTestContext) -> Option<bool> {
    let frames = frames_view(fixture, cx);
    let items = frames.read_with(cx, |frames, cx| frames.frame_menu(cx));
    menu_rows(&items).into_iter().find(|(label, _)| label == "Reset Frame").map(|(_, disabled)| !disabled)
}

fn capabilities_known(fixture: &Fixture, cx: &mut VisualTestContext) -> bool {
    let driver = fixture.driver(cx);
    driver.read_with(cx, |driver, cx| {
        driver.debugger().read(cx).state.active_session().is_some_and(|session| session.caps.is_some())
    })
}

// Not from a spec: the frame menu offers Reset Frame only when the adapter's capabilities say it can restart
// frames: with the fixture's adapter it is disabled once the capabilities are read, and a session of an adapter
// that can restart frames offers it.
#[gpui_kit::test]
async fn the_frame_menu_offers_reset_frame_only_when_the_adapter_can_restart_frames(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    stop_at_the_breakpoint(&fixture, cx);
    long(cx, "the adapter's capabilities", &|cx| capabilities_known(&fixture, cx));
    assert_eq!(reset_frame_enabled(&fixture, cx), Some(false), "the fixture's adapter cannot restart frames");
    stop_session(&fixture, cx);

    use_adapter(&fixture, &capable_adapter());
    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the second stop at the breakpoint", &|cx| {
        context(cx, "debugSessionPaused") && fixture.frame_lines(cx) == ["main, main.fake:2"]
    });
    long(cx, "Reset Frame offered", &|cx| reset_frame_enabled(&fixture, cx) == Some(true));
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

// Not from a spec: stopped at a breakpoint, the thread chooser lists every thread the adapter reports, the
// selected one first named.
#[gpui_kit::test]
async fn the_thread_chooser_lists_every_thread_the_adapter_reports(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &capable_adapter());
    stop_at_the_breakpoint(&fixture, cx);
    let frames = frames_view(&fixture, cx);
    let threads = |cx: &mut VisualTestContext| {
        let items = frames.read_with(cx, |frames, cx| frames.thread_menu(cx));
        menu_rows(&items).into_iter().map(|(label, _)| label).collect::<Vec<_>>()
    };
    long(cx, "the threads", &|cx| threads(cx) == ["main", "worker"]);
    assert_eq!(frames.read_with(cx, |frames, cx| frames.thread_label(cx)).as_deref(), Some("main"));
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

// Not from a spec: a breakpoint the adapter reports unbound after the program was configured shows as not
// verified, with the adapter's message, in the Breakpoints dialog.
#[gpui_kit::test]
async fn a_breakpoint_the_adapter_reports_unbound_shows_its_message_in_the_breakpoints_dialog(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &unverifying_adapter());
    stop_at_the_breakpoint(&fixture, cx);
    let dialog = breakpoints_dialog(&fixture, cx);
    let unbound = |cx: &mut VisualTestContext| {
        rows(&dialog, cx).iter().any(|row| match row {
            DialogRow::Line { entry, status: Some(BreakpointStatus { verified, message, .. }), .. } => {
                entry.line == 1 && !verified && message.as_deref() == Some(UNBOUND)
            }
            _ => false,
        })
    };
    long(cx, "the unbound breakpoint", &unbound);
    close_dialog(&fixture, &dialog, cx);
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

// Not from a spec: a breakpoint the adapter reports unbound is drawn as such in the editor: its line keeps the
// breakpoint tint and the gutter shows a grey dot, not the red one of a bound breakpoint.
#[gpui_kit::test]
async fn a_breakpoint_the_adapter_reports_unbound_shows_a_grey_dot_in_the_gutter(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &unverifying_adapter());
    stop_at_the_breakpoint(&fixture, cx);
    let unbound = fixture.line_mark(1, MarkKind::UnverifiedBreakpoint, cx);
    let editor = fixture.editor(cx);
    let painted = |kind: MarkKind, cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.painted_marks(kind));
    long(cx, "the unbound breakpoint's mark", &|cx| {
        fixture.marks(MarkLayer::Breakpoints, cx) == [unbound.clone()]
            && !painted(MarkKind::UnverifiedBreakpoint, cx).is_empty()
    });
    assert!(painted(MarkKind::BreakpointLine, cx).is_empty(), "no red dot for the unbound breakpoint");
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

/// The exception filters the Breakpoints dialog lists for `adapter`: (label, on).
fn exception_filters(
    dialog: &Entity<BreakpointsDialog>,
    adapter: &str,
    cx: &mut VisualTestContext,
) -> Vec<(String, bool)> {
    dialog.read_with(cx, |dialog, _| {
        let group = dialog.exception_groups().iter().find(|group| group.adapter == adapter);
        let filters = group.map(|group| group.filters.as_slice()).unwrap_or_default();
        filters.iter().map(|filter| (filter.filter.label.clone(), filter.enabled)).collect()
    })
}

// Not from a spec: the Breakpoints dialog lists the exception filters of the running session's adapter, which it
// did not know before the session; ticking one sends it to the adapter, which reports it.
#[gpui_kit::test]
async fn the_breakpoints_dialog_lists_the_adapters_exception_filters_and_a_ticked_one_reaches_it(
    cx: &mut TestAppContext,
) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &capable_adapter());
    let dialog = breakpoints_dialog(&fixture, cx);
    assert!(exception_filters(&dialog, "fake-dap", cx).is_empty(), "no session told of its filters yet");
    close_dialog(&fixture, &dialog, cx);

    stop_at_the_breakpoint(&fixture, cx);
    let dialog = breakpoints_dialog(&fixture, cx);
    let raised = |on: bool| vec![("Raised Exceptions".to_owned(), on)];
    long(cx, "the adapter's filters", &|cx| exception_filters(&dialog, "fake-dap", cx) == raised(false));
    dialog.update(cx, |_, cx| {
        let (adapter, filter) = ("fake-dap".to_owned(), "raised".to_owned());
        cx.emit(BreakpointsDialogEvent::SetExceptionFilter { adapter, filter, enabled: true });
    });
    long(cx, "the adapter to report the filter", &|cx| fixture.console(cx).contains("exception filters: raised"));
    assert_eq!(exception_filters(&dialog, "fake-dap", cx), raised(true));
    close_dialog(&fixture, &dialog, cx);
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

// Not from a spec: Toggle Method Breakpoint on a Python function while a session runs sends the method breakpoint
// to the adapter, which reports it, and the Breakpoints dialog lists it as the adapter verified it.
#[gpui_kit::test]
async fn a_method_breakpoint_set_during_a_session_reaches_the_adapter_and_shows_it_verified(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &capable_adapter());
    stop_at_the_breakpoint(&fixture, cx);
    open_file(&fixture, "calc.py", "x = 1\ndef run():\n    pass\n", cx);
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleMethodBreakpoint");
    long(cx, "the adapter to report the method", &|cx| fixture.console(cx).contains("function breakpoints: run"));
    let dialog = breakpoints_dialog(&fixture, cx);
    let verified = |cx: &mut VisualTestContext| {
        rows(&dialog, cx).iter().any(|row| {
            matches!(row, DialogRow::Special { bp: SpecialBreakpoint::Method(method), status: Some(status) }
                if method.name == "run" && status.verified)
        })
    };
    long(cx, "the verified method breakpoint", &verified);
    close_dialog(&fixture, &dialog, cx);
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

// Not from a spec: Add Field Watchpoint on a local asks the adapter what to watch; the watchpoint it names is
// listed in the Breakpoints dialog under the local's name, sent to the adapter, which reports it, and verified.
#[gpui_kit::test]
async fn add_field_watchpoint_on_a_local_sets_the_watchpoint_the_adapter_names(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    use_adapter(&fixture, &capable_adapter());
    stop_at_the_breakpoint(&fixture, cx);
    long(cx, "the locals", &|cx| fixture.shows_line_value("2", cx));
    long(cx, "the adapter's capabilities", &|cx| capabilities_known(&fixture, cx));
    let view = fixture.view(cx).expect("the Debug tool window");
    let variables = view.read_with(cx, |view, _| view.variables().clone());
    variables.update_in(cx, |variables, window, cx| variables.open_menu_on("line", window, cx));
    let offered = variables.read_with(cx, |variables, cx| menu_rows(&variables.variable_menu(cx)));
    assert!(offered.iter().any(|(label, disabled)| label == "Add Field Watchpoint" && !disabled), "{offered:?}");
    variables.update_in(cx, |variables, window, cx| {
        variables.run_variable_command(VariableCommand::AddFieldWatchpoint, window, cx);
    });
    long(cx, "the adapter to report the watchpoint", &|cx| fixture.console(cx).contains("data breakpoints: line@main"));
    if fixture.workbench.read_with(cx, |w, _| w.popup.is_some()) {
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
    }
    let dialog = breakpoints_dialog(&fixture, cx);
    let watched = |cx: &mut VisualTestContext| {
        rows(&dialog, cx).iter().any(|row| {
            matches!(row, DialogRow::Special { bp: SpecialBreakpoint::Field(field), status: Some(status) }
                if field.data_id == "line@main" && field.label == "line" && status.verified)
        })
    };
    long(cx, "the verified watchpoint", &watched);
    close_dialog(&fixture, &dialog, cx);
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}

/// A loopback port nothing listens on.
fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("the port's address").port()
}

/// Whether something of this process or another holds `port` open for connections.
fn port_taken(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_err()
}

/// Connects to `port` of the loopback address once something listens there, as a remote JVM started with
/// `server=n` does.
fn connect_once_listened(port: u16, cx: &mut VisualTestContext) -> std::net::TcpStream {
    for _ in 0..1500 {
        if let Ok(stream) = std::net::TcpStream::connect(("127.0.0.1", port)) {
            return stream;
        }
        cx.executor().advance_clock(Duration::from_millis(20));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("nothing listened on port {port} within thirty seconds");
}

/// Debugs a Remote JVM Debug configuration `Remote` in listen mode on `port` of the loopback address. The Java
/// debugger it would attach afterwards is answered with a port nothing listens on.
fn listen_for_a_remote_jvm(port: u16, cx: &mut VisualTestContext) {
    cx.update(|_, cx| cx.set_global(JavaDebugServerPort(free_port())));
    let mut config = blank_config("remote", "Remote", "remote-jvm");
    let port = port.to_string();
    for (key, value) in [("mode", "listen"), ("host", "127.0.0.1"), ("port", port.as_str())] {
        config.extras.params.insert(key.to_owned(), value.to_owned());
    }
    let arg = serde_json::json!({ "config": serde_json::to_value(&config).expect("the configuration as JSON") });
    let ran = cx.update(|window, cx| execute_action_or_throw("debug.startConfig", Some(&arg), window, cx));
    ran.expect("debug.startConfig runs");
}

/// The balloon that says the IDE listens for the remote JVM on `port`.
fn listening_notice(fixture: &Fixture, port: u16, cx: &mut VisualTestContext) -> Option<IdeNotification> {
    let title = format!("'Remote': listening for the remote JVM on port {port}");
    fixture.workbench.read_with(cx, |w, _| {
        w.notifications().balloons(false).into_iter().find(|notice| notice.title == title).cloned()
    })
}

// Not from a spec: a Remote JVM Debug configuration in listen mode listens on its port and says so until the
// remote JVM connects there; then the notice goes.
#[gpui_kit::test]
async fn listening_for_a_remote_jvm_waits_on_its_port_until_the_jvm_connects(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    let port = free_port();
    listen_for_a_remote_jvm(port, cx);
    long(cx, "the listening notice", &|cx| listening_notice(&fixture, port, cx).is_some());
    let jvm = connect_once_listened(port, cx);
    long(cx, "the notice to go", &|cx| listening_notice(&fixture, port, cx).is_none());
    drop(jvm);
    close_project(&fixture, cx);
}

// Not from a spec: Cancel on the listening notice stops listening for the remote JVM: the notice goes, the port is
// free again, and nothing failed.
#[gpui_kit::test]
async fn cancel_on_the_listening_notice_stops_listening_for_the_remote_jvm(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    let port = free_port();
    listen_for_a_remote_jvm(port, cx);
    long(cx, "the listening notice", &|cx| listening_notice(&fixture, port, cx).is_some());
    long(cx, "the port to be listened on", &|_| port_taken(port));
    let notice = listening_notice(&fixture, port, cx).expect("the notice");
    let cancel = notice.actions.iter().find(|action| action.command.starts_with("debug.cancelListen:"));
    let cancel = EntryAction::Own(cancel.cloned().expect("the notice offers Cancel"));
    fixture.workbench.update_in(cx, |w, window, cx| w.run_notification_action(cancel, window, cx));
    long(cx, "the notice to go", &|cx| listening_notice(&fixture, port, cx).is_none());
    long(cx, "the port to be free", &|_| !port_taken(port));
    assert!(!notification_titles(&fixture, cx).iter().any(|title| title == "Cannot start debugging"));
    close_project(&fixture, cx);
}

/// Writes an executable shell script at `path`.
#[cfg(unix)]
fn write_script(path: &std::path::Path, text: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(path, text).expect("the script is written");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("the script can run");
}

/// A user definition of the language `id`, of files `.extension`, whose debugger `adapter` is the fake adapter; it
/// replaces a bundled language of the same id.
#[cfg(unix)]
struct FakeLanguage<'a> {
    id: &'a str,
    extension: &'a str,
    adapter: &'a str,
}

/// As the fixture's `open`, with the fake adapter also declared by `languages`, the project's `file` holding
/// `program`, and the configurations `configs` makes (given the home and the project root) saved for the project;
/// `file` is open in the editor.
#[cfg(unix)]
fn open_with<'a>(
    cx: &'a mut TestAppContext,
    languages: &[FakeLanguage<'_>],
    file: &str,
    program: &str,
    configs: impl FnOnce(&std::path::Path, &std::path::Path) -> Vec<ide_run::RunConfig>,
) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let script = home.path().join("fake_dap.py");
    std::fs::write(&script, FAKE_ADAPTER).expect("the adapter script is written");
    let folder_of_languages = dirs.config.join("languages");
    std::fs::create_dir_all(&folder_of_languages).expect("the languages folder is made");
    for language in languages {
        let toml = format!(
            "id = {:?}\nname = {:?}\nextensions = [{:?}]\nline-comment = \"//\"\n\n\
             [debugger]\nadapter = {:?}\ncommand = \"python3\"\nargs = [{:?}]\n",
            language.id,
            language.id,
            language.extension,
            language.adapter,
            script.to_string_lossy(),
        );
        let path = folder_of_languages.join(format!("{}.toml", language.id));
        std::fs::write(path, toml).expect("the language is written");
    }
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join(file), program).expect("the program is written");
    let configs = configs(home.path(), &root);
    ide_run::save_user_configs(&root, &configs).expect("the configurations are saved");
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
    let path = root.join(file);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path, window, cx));
    long(cx, "the editor", &|cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    (Fixture { workbench, root, _folders: (home, folder) }, cx)
}

// Not from a spec: debugging a PHP configuration starts the PHP chosen in Settings; when that PHP loads no Xdebug,
// a warning names it and its version and offers its settings. The PHP is a script that prints PHP 8.3.0 and no
// Xdebug version, as `php -r` does for a PHP without Xdebug.
#[cfg(unix)]
#[gpui_kit::test]
async fn a_php_debug_start_warns_when_the_php_chosen_in_settings_loads_no_xdebug(cx: &mut TestAppContext) {
    let php = FakeLanguage { id: "php", extension: "php", adapter: "php-debug" };
    let (fixture, cx) = open_with(cx, &[php], "main.php", "start\nloop\n", |_, _| {
        vec![ide_run::RunConfig {
            command: "php".to_owned(),
            args: vec!["main.php".to_owned()],
            debug_adapter: Some("php-debug".to_owned()),
            ..blank_config("main", "main", "program")
        }]
    });
    let interpreter = fixture._folders.0.path().join("bin/php");
    write_script(&interpreter, "#!/bin/sh\necho 8.3.0\n");
    let interpreter = interpreter.to_string_lossy().into_owned();
    let write = crate::platform::settings::SettingsWrite {
        key: "languages.php.interpreter",
        value: serde_json::Value::from(interpreter.clone()),
    };
    cx.update(|_, cx| crate::platform::settings::set_global("w1", &[write], cx)).expect("the PHP is chosen");
    fixture.select_main(cx);
    run(cx, "run.debug");
    let title = "Xdebug is not loaded in PHP 8.3.0";
    let warning = |cx: &mut VisualTestContext| {
        fixture.workbench.read_with(cx, |w, _| w.notifications().items().iter().find(|n| n.title == title).cloned())
    };
    long(cx, "the Xdebug warning", &|cx| warning(cx).is_some());
    let warning = warning(cx).expect("the warning");
    let body = warning.body.unwrap_or_default();
    assert!(body.starts_with(&interpreter), "{body}");
    let labels: Vec<&str> = warning.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(labels, ["Open PHP Settings"]);
    if fixture.status(cx).is_some_and(|status| status != SessionStatus::Terminated) {
        stop_session(&fixture, cx);
    }
    close_project(&fixture, cx);
}

// Not from a spec: debugging a .NET project configuration asks the project's `dotnet` for the build output
// (`msbuild -getProperty:TargetPath`) and debugs that file: the program stops at its breakpoint. The `dotnet` is a
// script that names the program `main.cs` as the output.
#[cfg(unix)]
#[gpui_kit::test]
async fn a_dotnet_debug_start_debugs_the_build_output_the_project_names(cx: &mut TestAppContext) {
    // F# declares `netcoredbg` too: both are replaced, so the fake adapter is the only `netcoredbg`.
    let languages = [
        FakeLanguage { id: "csharp", extension: "cs", adapter: "netcoredbg" },
        FakeLanguage { id: "fsharp", extension: "fs", adapter: "netcoredbg" },
    ];
    let (fixture, cx) = open_with(cx, &languages, "main.cs", PROGRAM, |home, root| {
        let output = root.join("main.cs");
        let dotnet = home.join("bin/dotnet");
        let text = format!(
            "#!/bin/sh\ncase \"$*\" in *-getProperty:TargetPath*) echo {:?} ;; *) exit 1 ;; esac\n",
            output.to_string_lossy()
        );
        write_script(&dotnet, &text);
        std::fs::write(root.join("App.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\" />\n").expect("the project");
        let mut config = blank_config("main", "main", "dotnet");
        config.command = dotnet.to_string_lossy().into_owned();
        config.debug_adapter = Some("netcoredbg".to_owned());
        config.extras.params.insert("project".to_owned(), "App.csproj".to_owned());
        vec![config]
    });
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    fixture.select_main(cx);
    run(cx, "run.debug");
    wait_execution_at(&fixture, 1, "the stop in the build output", cx);
    long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.cs:2"]);
    stop_session(&fixture, cx);
    close_project(&fixture, cx);
}
