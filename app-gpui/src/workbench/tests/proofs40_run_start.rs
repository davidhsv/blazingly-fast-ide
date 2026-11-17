//! Stop while a run is starting. Each run's program is a stand-in shell script in the test's project
//! folder: it records its start in `<script>.started`, records the interrupt or termination Stop sends it
//! in `<script>.stopped` and exits, and until then waits. Its start is held by
//! [`StartGate`](crate::run::proofs40_start_gate::StartGate): before the process host is asked (as a run
//! target's validation keeps a start waiting), or with the host's answer once the process started (an
//! answer that comes late). Nothing else is started.
#![cfg(unix)]

use super::*;
use crate::run::configs::blank_config;
use crate::run::proofs40_start_gate::{GatePoint, StartGate};
use crate::run::store::{RunSession, SessionStatus};
use std::path::Path;

/// The last session of the configuration `name`.
fn session_of(store: &Entity<RunStore>, name: &str, cx: &mut VisualTestContext) -> Option<RunSession> {
    store.read_with(cx, |store, _| store.sessions.iter().rev().find(|s| s.config.name == name).cloned())
}

/// The lines the Run tool window shows for its current tab.
fn run_lines(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
        .and_then(|view| view.downcast::<RunView>().ok())
        .expect("the Run tool window holds its view");
    view.read_with(cx, |view, cx| view.lines(cx))
}

/// Writes the stand-in program `name` into `root` and returns its path.
fn stand_in(root: &Path, name: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let path = root.join(name);
    let script = "#!/bin/sh\ntrap 'echo stopped > \"$0.stopped\"; exit 0' INT TERM\necho started > \"$0.started\"\n\
                  echo running\nwhile :; do sleep 0.1; done\n";
    std::fs::write(&path, script).expect("the stand-in is written");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the stand-in can run");
    path
}

/// The record `what` (`started`, `stopped`) of the stand-in at `program`.
fn record(program: &Path, what: &str) -> PathBuf {
    PathBuf::from(format!("{}.{what}", program.to_string_lossy()))
}

/// Run Configuration's argument for running `program` as a temporary configuration named `name`.
fn given(name: &str, program: &Path) -> serde_json::Value {
    let config = ide_run::RunConfig {
        command: program.to_string_lossy().into_owned(),
        args: Vec::new(),
        ..blank_config(name, name, "program")
    };
    serde_json::json!({ "config": serde_json::to_value(&config).expect("a configuration is JSON"), "temporary": true })
}

/// The cancellable request the session's launch waits on.
fn request_of(store: &Entity<RunStore>, session: &str, cx: &mut VisualTestContext) -> Option<ide_core::RequestId> {
    store.read_with(cx, |store, _| store.resolve_request(session))
}

/// Whether a notification or a console line tells of a failed start or stop.
fn told_a_failure(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    let failed = |text: &str| text.starts_with("Error running") || text == "Cannot stop process";
    notification_titles(workbench, cx).iter().any(|title| failed(title))
        || run_lines(workbench, cx).iter().any(|line| failed(line))
}

/// Stop during a run's start cancels the start's request: the tab ends as stopped, without a process,
/// and once the start's late answer (cancelled) arrives nothing is started, told or printed, and the
/// request is gone.
#[gpui_kit::test]
async fn cancels_startup_and_handles_a_late_cancelled_response(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, store) = run_project(cx, Vec::new());
    cx.update(|window, _| window.activate_window());
    let program = stand_in(&root, "waiter");
    let mut gate = StartGate::hold(&program.to_string_lossy(), GatePoint::BeforeStart);

    run_action(cx, "run.runConfig", Some(given("waiter", &program)));
    wait_long(cx, "the start to wait for the process host", |_| gate.reached());
    let starting = session_of(&store, "waiter", cx).expect("the run has its tab");
    assert_eq!((starting.status, starting.run_id), (SessionStatus::Starting, None));
    assert!(request_of(&store, &starting.id, cx).is_some(), "the start goes out with a request Stop can cancel");

    click_on(cx, "run-stop");
    let stopped = session_of(&store, "waiter", cx).expect("the tab stays");
    assert_eq!((stopped.status, stopped.stopped, stopped.run_id), (SessionStatus::Finished, true, None));

    gate.release();
    wait_long(cx, "the late answer to be handled", |cx| {
        gate.answered() && request_of(&store, &starting.id, cx).is_none()
    });
    let ended = session_of(&store, "waiter", cx).expect("the tab stays");
    assert_eq!((ended.status, ended.stopped, ended.run_id), (SessionStatus::Finished, true, None));
    assert!(!record(&program, "started").exists(), "the cancelled start started no process");
    assert!(!told_a_failure(&workbench, cx), "{:?}", notification_titles(&workbench, cx));
    close_backend_project(cx);
}

/// Stop while the process host's answer is still on its way: the process that started anyway is
/// stopped once the answer (a success) arrives, the tab stays stopped without a process, nothing is
/// told, and a run of another configuration keeps running.
#[gpui_kit::test]
async fn cancels_startup_and_handles_a_late_success_response(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, store) = run_project(cx, Vec::new());
    cx.update(|window, _| window.activate_window());
    let bystander = stand_in(&root, "bystander");
    run_action(cx, "run.runConfig", Some(given("bystander", &bystander)));
    wait_long(cx, "the other run to start", |cx| {
        session_of(&store, "bystander", cx).is_some_and(|s| s.status == SessionStatus::Running)
    });

    let program = stand_in(&root, "late");
    let mut gate = StartGate::hold(&program.to_string_lossy(), GatePoint::AfterStart);
    run_action(cx, "run.runConfig", Some(given("late", &program)));
    wait_long(cx, "the process to start and its answer to wait", |_| {
        gate.reached() && record(&program, "started").exists()
    });
    let starting = session_of(&store, "late", cx).expect("the run has its tab");
    assert_eq!((starting.status, starting.run_id), (SessionStatus::Starting, None));

    click_on(cx, "run-stop");
    let stopped = session_of(&store, "late", cx).expect("the tab stays");
    assert_eq!((stopped.status, stopped.stopped), (SessionStatus::Finished, true));

    gate.release();
    wait_long(cx, "the process that started anyway to be stopped", |_| record(&program, "stopped").exists());
    wait_long(cx, "the answer to be handled", |cx| request_of(&store, &starting.id, cx).is_none());
    let ended = session_of(&store, "late", cx).expect("the tab stays");
    assert_eq!((ended.status, ended.stopped, ended.run_id), (SessionStatus::Finished, true, None));
    assert!(!told_a_failure(&workbench, cx), "{:?}", notification_titles(&workbench, cx));

    let other = session_of(&store, "bystander", cx).expect("the other run has its tab");
    assert_eq!((other.status, other.stopped), (SessionStatus::Running, false));
    assert!(!record(&bystander, "stopped").exists(), "a run nobody stopped keeps running");
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    driver.update(cx, |driver, cx| driver.stop(&other.id, cx));
    wait_long(cx, "the other run to end", |cx| session_of(&store, "bystander", cx).is_some_and(|s| !s.is_live()));
    close_backend_project(cx);
}

/// A start held and let go before anyone stops it runs its program: the tab shows the process running
/// and its output, and the start's request is gone once the process host answered.
#[gpui_kit::test]
async fn a_start_let_go_before_stop_runs_its_program(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, store) = run_project(cx, Vec::new());
    cx.update(|window, _| window.activate_window());
    let program = stand_in(&root, "plain");
    let mut gate = StartGate::hold(&program.to_string_lossy(), GatePoint::BeforeStart);
    run_action(cx, "run.runConfig", Some(given("plain", &program)));
    wait_long(cx, "the start to wait for the process host", |_| gate.reached());
    let starting = session_of(&store, "plain", cx).expect("the run has its tab");
    assert!(request_of(&store, &starting.id, cx).is_some(), "every process start goes out with a request");
    gate.release();
    wait_long(cx, "the process to run", |cx| {
        session_of(&store, "plain", cx).is_some_and(|s| s.status == SessionStatus::Running && s.run_id.is_some())
    });
    let running = session_of(&store, "plain", cx).expect("the run has its tab");
    assert_eq!(request_of(&store, &running.id, cx), None);
    wait_long(cx, "its output", |cx| run_lines(&workbench, cx).iter().any(|line| line == "running"));

    click_on(cx, "run-stop");
    wait_long(cx, "the program to be stopped", |_| record(&program, "stopped").exists());
    wait_long(cx, "its end", |cx| session_of(&store, "plain", cx).is_some_and(|s| !s.is_live()));
    assert_eq!(session_of(&store, "plain", cx).map(|s| s.stopped), Some(true));
    close_backend_project(cx);
}
