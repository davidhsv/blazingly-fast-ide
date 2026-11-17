//! Debug of a configuration that runs on a run target (another machine over SSH, a container) is
//! refused: the window says where it runs and offers Run, and the debug adapter is never started. The
//! targets are made-up settings of the test project; no connection is ever opened.

use super::*;
use ide_workspace::settings::SettingsScope;
use std::path::Path;

const ON_TARGETS: &str = "Debugging on run targets is not supported";
const IN_CONTAINERS: &str = "Only Python can be debugged in containers";

/// The fixture's adapter, appending a line to `<script>.started` each time it starts.
fn counting_adapter(fixture: &Fixture) -> PathBuf {
    let script = fixture._folders.0.path().join("fake_dap.py");
    let started = script.with_extension("started");
    let counting = format!(
        "with open({:?}, 'a') as started:\n    started.write('started\\n')\n{FAKE_ADAPTER}",
        started.to_string_lossy().into_owned()
    );
    std::fs::write(&script, counting).expect("the adapter is written");
    started
}

/// How many times the adapter started.
fn starts(started: &Path) -> usize {
    std::fs::read_to_string(started).map_or(0, |text| text.lines().count())
}

/// Puts an SSH target `box` and a Docker target `img` in the project's settings, and makes `default`
/// the target of every configuration that names none (empty: this machine).
fn set_targets(default: &str, cx: &mut VisualTestContext) {
    let targets = json!([
        { "kind": "ssh", "id": "box", "name": "Build box", "sshConfig": "made-up", "projectRoot": "/srv/app" },
        { "kind": "docker", "id": "img", "name": "App image", "image": "made-up:latest" },
    ]);
    cx.update(|_, cx| {
        crate::platform::settings::set("w1", SettingsScope::Project, "runTargets", &targets, cx)
            .expect("the targets are stored");
        crate::platform::settings::set("w1", SettingsScope::Project, "run.defaultTarget", &json!(default), cx)
            .expect("the default target is stored");
    });
    cx.run_until_parked();
}

/// The refusal with `title`: its body and its buttons.
fn refusal(fixture: &Fixture, title: &str, cx: &mut VisualTestContext) -> (Option<String>, Vec<String>) {
    long(cx, title, &|cx| notification_titles(fixture, cx).iter().any(|shown| shown == title));
    fixture.workbench.read_with(cx, |w, _| {
        let notice = w.notifications().items().iter().find(|n| n.title == title).expect("the notice");
        (notice.body.clone(), notice.actions.iter().map(|action| action.label.clone()).collect())
    })
}

// Not from a spec: Run › Debug of the selected configuration, which runs on an SSH target, tells where it
// runs and offers Run and Edit Configuration…; no session is created and the debug adapter never starts.
#[gpui_kit::test]
async fn debugging_a_configuration_on_an_ssh_target_is_refused_and_starts_no_adapter(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    let started = counting_adapter(&fixture);
    set_targets("box", cx);
    fixture.select_main(cx);
    run(cx, "run.debug");
    let (body, buttons) = refusal(&fixture, ON_TARGETS, cx);
    assert_eq!(
        body.as_deref(),
        Some("'main' runs on Build box. Run it, or choose “Local machine” under “Run on” to debug it.")
    );
    assert_eq!(buttons, ["Run", "Edit Configuration…"]);
    cx.run_until_parked();
    assert_eq!(session_count(&fixture, cx), 0);
    assert!(!fixture.workbench.read_with(cx, |w, _| w.is_tool_window_visible(DEBUG_TOOL_WINDOW)));
    assert_eq!(starts(&started), 0, "the adapter is not started");
    close_project(&fixture, cx);
}

// Not from a spec: Debug Configuration (what the gutter's, the chooser's, the Services' and the Run tool
// window's Debug run) of a configuration in a Docker container says only Python is debugged there, and
// starts nothing; its Edit Configuration… button opens the configurations' editor on it.
#[gpui_kit::test]
async fn debug_configuration_in_a_container_is_refused_and_its_button_edits_the_configuration(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    let started = counting_adapter(&fixture);
    set_targets("img", cx);
    let ran = cx.update(|window, cx| execute_action_or_throw("run.debugConfig", Some(&json!("main")), window, cx));
    ran.expect("Debug Configuration runs");
    let (body, _) = refusal(&fixture, IN_CONTAINERS, cx);
    assert_eq!(
        body.as_deref(),
        Some("'main' runs on App image. Run it, or choose “Local machine” under “Run on” to debug it.")
    );
    assert_eq!(session_count(&fixture, cx), 0);
    assert_eq!(starts(&started), 0, "the adapter is not started");

    let edit = fixture.workbench.read_with(cx, |w, _| {
        let notice = w.notifications().items().iter().find(|n| n.title == IN_CONTAINERS).expect("the notice");
        notice.actions.iter().find(|action| action.label == "Edit Configuration…").cloned().expect("the button")
    });
    let press = crate::workbench::notification_balloons::EntryAction::Own(edit);
    fixture.workbench.update_in(cx, |w, window, cx| w.run_notification_action(press, window, cx));
    long(cx, "Edit Configurations", &|cx| fixture.workbench.read_with(cx, |w, _| w.edit_configs_dialog().is_some()));
    let dialog = fixture.workbench.read_with(cx, |w, _| w.edit_configs_dialog().cloned()).expect("the dialog");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.selected()).as_deref(), Some("main"));
    close_project(&fixture, cx);
}

// Not from a spec: Rerun of a debug session whose configuration now runs on a run target is refused like a
// first Debug: the session it replaces ends and no second one starts.
#[gpui_kit::test]
async fn rerun_in_the_debugger_of_a_configuration_now_on_a_run_target_is_refused(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "start\nloop\nend\n");
    let started = counting_adapter(&fixture);
    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the running session", &|cx| context(cx, "debugSessionRunning"));
    long(cx, "the adapter to start", &|_| starts(&started) == 1);
    set_targets("box", cx);
    run(cx, "debug.rerun");
    refusal(&fixture, ON_TARGETS, cx);
    long(cx, "the first session to end", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    assert_eq!(session_count(&fixture, cx), 1);
    assert_eq!(starts(&started), 1, "no second adapter is started");
    close_project(&fixture, cx);
}

// Not from a spec: with the target back on this machine the same configuration is debugged.
#[gpui_kit::test]
async fn a_configuration_on_this_machine_is_still_debugged(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "start\nloop\nend\n");
    let started = counting_adapter(&fixture);
    set_targets("", cx);
    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the running session", &|cx| context(cx, "debugSessionRunning"));
    long(cx, "the adapter to start", &|_| starts(&started) == 1);
    assert!(!notification_titles(&fixture, cx).iter().any(|title| title == ON_TARGETS));
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}

// Not from a spec: the run widget's Debug button on the selected configuration, which runs on an SSH
// target, is refused the same way and starts nothing.
#[gpui_kit::test]
async fn the_run_widgets_debug_button_on_a_run_target_is_refused(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    let started = counting_adapter(&fixture);
    set_targets("box", cx);
    fixture.select_main(cx);
    cx.run_until_parked();
    let bounds = cx.debug_bounds("run-widget-debug").expect("the widget offers Debug");
    cx.simulate_click(bounds.center(), Modifiers::default());
    refusal(&fixture, ON_TARGETS, cx);
    cx.run_until_parked();
    assert_eq!(session_count(&fixture, cx), 0);
    assert_eq!(starts(&started), 0, "the adapter is not started");
    close_project(&fixture, cx);
}
