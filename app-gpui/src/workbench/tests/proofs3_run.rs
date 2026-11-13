use super::*;
use crate::run::configs::blank_config;
use crate::run::store::{RunSession, SessionStatus};
use crate::services::model::NodeCommand;
use crate::services::view::ServicesView;

/// The last session of the configuration `name`.
fn session_of(store: &Entity<RunStore>, name: &str, cx: &mut VisualTestContext) -> Option<RunSession> {
    store.read_with(cx, |store, _| store.sessions.iter().rev().find(|s| s.config.name == name).cloned())
}

/// The view of the Run tool window.
fn run_view_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunView> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view")
}

/// Whether the Run console shows the line `text`.
fn shows(view: &Entity<RunView>, text: &str, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, cx| view.lines(cx).iter().any(|line| line == text))
}

/// The view of the Services tool window.
fn services_view_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<ServicesView> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
        .and_then(|view| view.downcast::<ServicesView>().ok())
        .expect("the Services tool window holds its view")
}

/// A program configuration that runs `/bin/sh -c script`.
fn shell(id: &str, script: &str) -> ide_run::RunConfig {
    ide_run::RunConfig {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), script.to_owned()],
        ..blank_config(id, id, "program")
    }
}

/// Run Configuration runs the configuration its argument names by id, and one given whole with
/// `temporary` as a temporary configuration of the project; their output shows in the Run tool
/// window. Stop with a process's run id stops that process: its session ends as stopped.
#[gpui_kit::test]
async fn run_configuration_runs_the_named_or_given_configuration_and_stop_process_ends_the_process(
    cx: &mut TestAppContext,
) {
    let configs = vec![program("hello", "/bin/echo", "hello"), program("nap", "/bin/sleep", "30")];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);

    run_action(cx, "run.runConfig", Some(serde_json::Value::from("hello")));
    wait_long(cx, "the exit of echo", |cx| session_of(&store, "hello", cx).is_some_and(|s| !s.is_live()));
    let hello = session_of(&store, "hello", cx).expect("hello ran");
    assert_eq!((hello.status, hello.exit_code, hello.stopped), (SessionStatus::Finished, Some(0), false));
    let view = run_view_of(&workbench, cx);
    wait_long(cx, "the output of echo", |cx| shows(&view, "hello", cx));

    let greet = ide_run::RunConfig {
        command: "/bin/echo".to_owned(),
        args: vec!["greetings".to_owned()],
        ..blank_config("greet", "greet", "program")
    };
    let given = serde_json::json!({
        "config": serde_json::to_value(&greet).expect("a configuration is JSON"),
        "temporary": true,
    });
    run_action(cx, "run.runConfig", Some(given));
    wait_long(cx, "the exit of the given configuration", |cx| {
        session_of(&store, "greet", cx).is_some_and(|s| !s.is_live())
    });
    assert_eq!(session_of(&store, "greet", cx).and_then(|s| s.exit_code), Some(0));
    let kept = store.read_with(cx, |store, _| {
        store.temporary.iter().filter_map(|id| store.config(id)).any(|config| config.name == "greet")
    });
    assert!(kept, "the configuration given whole is kept as a temporary one");
    wait_long(cx, "its output", |cx| shows(&view, "greetings", cx));

    run_action(cx, "run.runConfig", Some(serde_json::Value::from("nap")));
    wait_long(cx, "the start of sleep", |cx| {
        session_of(&store, "nap", cx).is_some_and(|s| s.status == SessionStatus::Running && s.run_id.is_some())
    });
    let run = session_of(&store, "nap", cx).and_then(|s| s.run_id).expect("sleep runs as a process");
    run_action(cx, "run.stopProcess", Some(serde_json::Value::from(run.0)));
    wait_long(cx, "the end of sleep", |cx| session_of(&store, "nap", cx).is_some_and(|s| !s.is_live()));
    let nap = session_of(&store, "nap", cx).expect("nap ran");
    assert_eq!((nap.status, nap.stopped), (SessionStatus::Finished, true));
    close_backend_project(cx);
}

/// Reload Run Configurations lists again what the project has saved: a configuration saved after
/// the project opened is listed once the action has run.
#[gpui_kit::test]
async fn reload_run_configurations_lists_a_configuration_saved_since_the_project_opened(cx: &mut TestAppContext) {
    let hello = program("hello", "/bin/echo", "hello");
    let (_workbench, cx, _folders, root, store) = run_project(cx, vec![hello.clone()]);
    let names = |cx: &mut VisualTestContext| -> Vec<String> {
        store.read_with(cx, |store, _| store.configs().iter().map(|config| config.name.clone()).collect())
    };
    assert_eq!(names(cx), ["hello"]);

    ide_run::save_user_configs(&root, &[hello, program("later", "/bin/echo", "later")])
        .expect("the configurations are saved");
    cx.run_until_parked();
    assert_eq!(names(cx), ["hello"], "nothing lists the new one by itself");
    run_action(cx, "run.reloadConfigs", None);
    wait_long(cx, "the configurations to be listed again", |cx| names(cx) == ["hello", "later"]);
    close_backend_project(cx);
}

/// A line typed into the Run console of a process that reads its input reaches the process, which
/// prints what it read; ⌃D closes the input of a process reading to its end, which then exits on
/// its own.
#[gpui_kit::test]
async fn a_line_typed_in_the_run_console_reaches_the_process_and_ctrl_d_ends_its_input(cx: &mut TestAppContext) {
    let configs =
        vec![shell("reader", "read line; echo \"got $line\""), shell("drain", "cat >/dev/null; echo drained")];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);
    cx.update(|window, _| window.activate_window());

    run_action(cx, "run.runConfig", Some(serde_json::Value::from("reader")));
    wait_long(cx, "the reader to take input", |cx| session_of(&store, "reader", cx).is_some_and(|s| s.accepts_input()));
    let view = run_view_of(&workbench, cx);
    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("h e l l o");
    assert_eq!(view.read_with(cx, |view, cx| view.pending_input(cx)).as_deref(), Some("hello"));
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the reader's answer", |cx| shows(&view, "got hello", cx));
    wait_long(cx, "the reader to exit", |cx| session_of(&store, "reader", cx).is_some_and(|s| !s.is_live()));
    assert_eq!(session_of(&store, "reader", cx).and_then(|s| s.exit_code), Some(0));

    run_action(cx, "run.runConfig", Some(serde_json::Value::from("drain")));
    wait_long(cx, "the drain to take input", |cx| session_of(&store, "drain", cx).is_some_and(|s| s.accepts_input()));
    let view = run_view_of(&workbench, cx);
    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("ctrl-d");
    wait_long(cx, "the drain to reach the end of its input", |cx| {
        session_of(&store, "drain", cx).is_some_and(|s| !s.is_live())
    });
    let drain = session_of(&store, "drain", cx).expect("drain ran");
    assert_eq!((drain.status, drain.exit_code, drain.stopped), (SessionStatus::Finished, Some(0), false));
    wait_long(cx, "the drain's last line", |cx| shows(&view, "drained", cx));
    close_backend_project(cx);
}

/// On a running configuration's row in Services the Run button reads Rerun and starts the
/// configuration again in a later session; Stop ends it, the row reads Stopped and the button
/// reads Run again.
#[gpui_kit::test]
async fn run_on_a_running_services_row_reruns_it_and_stop_ends_it(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![program("nap", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = services_view_of(&workbench, cx);
    let detail = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            view.rows().into_iter().find(|row| row.key == "run:config:nap").and_then(|row| row.detail)
        })
    };
    let run_title = |cx: &mut VisualTestContext| {
        let actions = view.read_with(cx, |view, cx| view.toolbar_actions(cx));
        actions.into_iter().find(|action| action.id == "run").map(|action| action.title)
    };

    click_on(cx, "services-row:run:config:nap");
    assert_eq!(run_title(cx).as_deref(), Some("Run"));
    click_on(cx, "services-action:run");
    wait_long(cx, "the start of sleep", |cx| {
        session_of(&store, "nap", cx).is_some_and(|s| s.status == SessionStatus::Running)
    });
    wait_long(cx, "the row to say so", |cx| detail(cx).as_deref() == Some("Running"));
    let first = session_of(&store, "nap", cx).expect("nap runs").started_at;
    assert_eq!(run_title(cx).as_deref(), Some("Rerun"));

    click_on(cx, "services-action:run");
    wait_long(cx, "the rerun of sleep", |cx| {
        session_of(&store, "nap", cx).is_some_and(|s| s.status == SessionStatus::Running && s.started_at > first)
    });
    let live = store.read_with(cx, |store, _| store.sessions.iter().filter(|s| s.is_live()).count());
    assert_eq!(live, 1, "the rerun replaces the running process");

    click_on(cx, "services-action:stop");
    wait_long(cx, "the end of sleep", |cx| {
        store.read_with(cx, |store, _| store.sessions.iter().all(|session| !session.is_live()))
    });
    wait_long(cx, "the row to say so", |cx| detail(cx).as_deref() == Some("Stopped"));
    assert_eq!(session_of(&store, "nap", cx).map(|s| s.stopped), Some(true));
    assert_eq!(run_title(cx).as_deref(), Some("Run"));
    close_backend_project(cx);
}

/// Add Service shows the Services tool window and opens its Add Service menu; Run Configuration
/// Type lists the project's category first, and choosing Program there shows the Program group
/// with the project's configuration of that type.
#[gpui_kit::test]
async fn add_service_opens_its_menu_and_a_chosen_type_shows_its_configurations(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, _store) = run_project(cx, vec![program("nap", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "services.addService", None);
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(SERVICES_TOOL_WINDOW)));
    let view = services_view_of(&workbench, cx);
    assert!(view.read_with(cx, |view, _| view.rows().iter().all(|row| row.key != "run:type:program")));

    let menu = workbench.read_with(cx, |w, _| w.open_menu::<Vec<NodeCommand>>()).expect("the Add Service menu is open");
    let levels = |cx: &mut VisualTestContext| -> Vec<Vec<String>> {
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        let unselected = |level: Vec<String>| -> Vec<String> {
            level.into_iter().map(|line| line.trim_start_matches("> ").to_owned()).collect()
        };
        lines.into_iter().map(unselected).collect()
    };
    let click_row = |cx: &mut VisualTestContext, depth: usize, index: usize| {
        let selector: &'static str = Box::leak(format!("menu-row-{depth}-{index}").into_boxed_str());
        click_on(cx, selector);
    };
    let shown = levels(cx);
    let types = shown[0].iter().position(|line| line == "Run Configuration Type");
    let types = types.unwrap_or_else(|| panic!("Run Configuration Type in {shown:?}"));
    click_row(cx, 0, types);
    // The categories of the project's own configurations come first.
    let shown = levels(cx);
    let category = shown[1].iter().position(|line| line != "-").unwrap_or_else(|| panic!("a category in {shown:?}"));
    click_row(cx, 1, category);
    let shown = levels(cx);
    let program_row = shown[2].iter().position(|line| line == "Program");
    let program_row = program_row.unwrap_or_else(|| panic!("Program in {shown:?}"));
    click_row(cx, 2, program_row);

    assert!(workbench.read_with(cx, |w, _| w.open_menu::<Vec<NodeCommand>>().is_none()), "the menu closes");
    let row = |cx: &mut VisualTestContext, key: &str| {
        view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.key == key).map(|row| row.label))
    };
    wait_long(cx, "the Program group", |cx| row(cx, "run:type:program").as_deref() == Some("Program"));
    assert_eq!(row(cx, "run:config:nap").as_deref(), Some("nap"));
    close_backend_project(cx);
}
