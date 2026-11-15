use super::*;
use crate::run::store::SessionStatus;

/// Run's key: ⌃R on macOS, Shift+F10 elsewhere.
const RUN_KEYS: &str = if cfg!(target_os = "macos") { "ctrl-r" } else { "shift-f10" };
/// Stop's key: ⌘F2 on macOS, Ctrl+F2 elsewhere.
const STOP_KEYS: &str = if cfg!(target_os = "macos") { "cmd-f2" } else { "ctrl-f2" };

/// A `dev` configuration running `script` with the shell.
fn dev(script: &str) -> ide_run::RunConfig {
    ide_run::RunConfig {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), script.to_owned()],
        ..crate::run::configs::blank_config("dev", "dev", "program")
    }
}

fn run_view(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<RunView> {
    workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view")
}

fn tooltips(view: &Entity<RunView>, cx: &mut VisualTestContext) -> Vec<Option<String>> {
    view.update(cx, |view, cx| view.tab_tooltips(cx))
}

/// ⌃R runs the selected `dev`: the Run tool window shows its standard output and error lines; once it exits,
/// Stop is off and the `dev` tab says it ended with exit code 0.
#[gpui_kit::test]
async fn ctrl_r_runs_the_selected_configuration_and_shows_its_output_and_exit(cx: &mut TestAppContext) {
    let script = "echo '> acme-api@1.0.0 dev'; echo 'Server listening on http://localhost:8080'; \
                  echo 'warning: config/dev.json not found, using defaults' >&2";
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![dev(script)]);
    store.update(cx, |store, _| store.select_config(Some("dev")));
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes(RUN_KEYS);
    let session = |cx: &mut VisualTestContext| store.read_with(cx, |store, _| store.sessions.first().cloned());
    wait_long(cx, "the exit of dev", |cx| session(cx).is_some_and(|s| !s.is_live()));
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW)), "Run shows");
    let view = run_view(&workbench, cx);
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.iter().any(|line| line == "Server listening on http://localhost:8080"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "warning: config/dev.json not found, using defaults"), "{lines:?}");
    assert!(!view.read_with(cx, |view, cx| view.can_stop(cx)), "Stop is off");
    assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["dev"]);
    let tooltip = tooltips(&view, cx).into_iter().next().flatten().unwrap_or_default();
    assert!(tooltip.ends_with("exit code 0"), "{tooltip}");
    let started = session(cx).expect("the session");
    assert_eq!((started.config.command.as_str(), started.exit_code), ("/bin/sh", Some(0)));
}

/// ⌃R runs a `dev` that keeps running: its output shows and Stop is on; ⌘F2 stops it, Stop is off and the
/// tab says it ended with the exit code of an interrupted process, 130.
#[gpui_kit::test]
async fn cmd_f2_stops_a_running_process(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, store) =
        run_project(cx, vec![dev("echo 'Watching for changes…'; exec sleep 30")]);
    store.update(cx, |store, _| store.select_config(Some("dev")));
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes(RUN_KEYS);
    let session = |cx: &mut VisualTestContext| store.read_with(cx, |store, _| store.sessions.first().cloned());
    wait_long(cx, "the output of dev", |cx| {
        if workbench.read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW)).is_none() {
            return false;
        }
        let lines = run_view(&workbench, cx).read_with(cx, |view, cx| view.lines(cx));
        lines.iter().any(|line| line == "Watching for changes…")
    });
    assert!(session(cx).is_some_and(|s| s.status == SessionStatus::Running));
    let view = run_view(&workbench, cx);
    assert!(view.read_with(cx, |view, cx| view.can_stop(cx)), "Stop is on");

    cx.simulate_keystrokes(STOP_KEYS);
    wait_long(cx, "the end of dev", |cx| session(cx).is_some_and(|s| !s.is_live()));
    assert!(session(cx).is_some_and(|s| s.stopped), "the process was stopped");
    assert!(!view.read_with(cx, |view, cx| view.can_stop(cx)), "Stop is off");
    let tooltip = tooltips(&view, cx).into_iter().next().flatten().unwrap_or_default();
    // Stop interrupts the process group first (SIGINT): the shell's `sleep` ends with 128 + 2.
    assert!(tooltip.ends_with("exit code 130"), "{tooltip}");
}
