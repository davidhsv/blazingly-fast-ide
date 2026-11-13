use super::*;

/// The fake adapter, also able to restart a frame: `restartFrame` takes the program back to its first line and
/// stops there.
fn restarting_adapter() -> String {
    let script = FAKE_ADAPTER
        .replace("'supportsSetVariable': True}", "'supportsSetVariable': True, 'supportsRestartFrame': True}")
        .replace(
            "    elif command == 'pause':",
            "    elif command == 'restartFrame':\n        respond(request)\n        current = 1\n        \
             stop('restart')\n    elif command == 'pause':",
        );
    assert!(script.contains("'supportsRestartFrame': True") && script.contains("'restartFrame'"), "{script}");
    script
}

/// The four session keys, in the order (paused, running, active, can rerun).
fn session_keys(cx: &mut VisualTestContext) -> [bool; 4] {
    ["debugSessionPaused", "debugSessionRunning", "debugSessionActive", "debugCanRerun"].map(|key| context(cx, key))
}

// Not from a spec: the debugger's session keys follow the session. Before any session none is on; stopped at a
// breakpoint the session is paused, active and can be rerun; resumed into a loop it runs and is no longer paused;
// stopped, it is neither active, running nor paused.
#[gpui_kit::test]
async fn the_session_keys_follow_a_session_from_paused_to_running_to_ended(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "start\nmiddle\nloop\nend\n");
    assert_eq!(session_keys(cx), [false, false, false, false], "no session yet");
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    fixture.select_main(cx);
    run(cx, "run.debug");
    long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
    assert_eq!(session_keys(cx), [true, false, true, true], "paused at the breakpoint");

    run(cx, "debug.resume");
    long(cx, "the program to run into its loop", &|cx| context(cx, "debugSessionRunning"));
    assert_eq!(session_keys(cx), [false, true, true, true], "running");

    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    let [paused, running, active, _] = session_keys(cx);
    assert!(!paused && !running && !active, "ended");
    close_project(&fixture, cx);
}

// Not from a spec: Debug… without a configuration starts the project's only configuration that has a debug
// adapter, and the program stops at its breakpoint with its frame listed.
#[gpui_kit::test]
async fn debug_without_a_configuration_starts_the_only_debuggable_one(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    let marked = fixture.line_mark(1, MarkKind::BreakpointLine, cx);
    long(cx, "the breakpoint mark", &|cx| fixture.marks(MarkLayer::Breakpoints, cx) == [marked.clone()]);
    run(cx, "debug.startConfig");
    long(cx, "the stop at the breakpoint", &|cx| context(cx, "debugSessionPaused"));
    long(cx, "the frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:2"]);
    let view = fixture.view(cx).expect("the Debug tool window");
    assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["main"]);
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}

// Not from a spec: Force Run to Cursor runs to the caret past a breakpoint on the way, which plain Run to Cursor
// stops at; Force Step Into steps one line; Reset Frame, with an adapter that can restart frames, takes the
// program back to the start of the frame.
#[gpui_kit::test]
async fn force_run_to_cursor_skips_breakpoints_force_step_into_steps_and_reset_frame_restarts(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "a\nb\nc\nd\ne\nf\nloop\n");
    // The adapter is started by the first debug session: the script is replaced before it.
    let script = fixture._folders.0.path().join("fake_dap.py");
    std::fs::write(script, restarting_adapter()).expect("the adapter is written");
    for line in [1, 2] {
        fixture.caret_to(line, cx);
        run(cx, "debug.toggleBreakpoint");
    }
    long(cx, "the breakpoint marks", &|cx| fixture.marks(MarkLayer::Breakpoints, cx).len() == 2);
    fixture.select_main(cx);
    run(cx, "run.debug");
    wait_execution_at(&fixture, 1, "the stop at the first breakpoint", cx);

    fixture.caret_to(4, cx);
    run(cx, "debug.forceRunToCursor");
    wait_execution_at(&fixture, 4, "the forced run to the cursor", cx);
    // The breakpoint on the way is still there, and plain Run to Cursor would have stopped at it.
    assert_eq!(fixture.marks(MarkLayer::Breakpoints, cx).len(), 2);

    run(cx, "debug.forceStepInto");
    wait_execution_at(&fixture, 5, "the forced step into", cx);

    run(cx, "debug.resetFrame");
    wait_execution_at(&fixture, 0, "the frame's restart", cx);
    long(cx, "the restarted frame", &|cx| fixture.frame_lines(cx) == ["main, main.fake:1"]);
    assert!(!notification_titles(&fixture, cx).iter().any(|title| title.starts_with("Reset Frame")));

    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}

// Not from a spec: with an adapter that cannot restart frames, Reset Frame says so and leaves the program where it
// stopped.
#[gpui_kit::test]
async fn reset_frame_tells_that_an_adapter_cannot_restart_frames(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, PROGRAM);
    fixture.caret_to(1, cx);
    run(cx, "debug.toggleBreakpoint");
    fixture.select_main(cx);
    run(cx, "run.debug");
    wait_execution_at(&fixture, 1, "the stop at the breakpoint", cx);
    run(cx, "debug.resetFrame");
    long(cx, "the notice", &|cx| {
        notification_titles(&fixture, cx).iter().any(|title| title == "Reset Frame is not supported by fake-dap")
    });
    assert_eq!(fixture.status(cx), Some(SessionStatus::Paused));
    assert_eq!(fixture.frame_lines(cx), ["main, main.fake:2"]);
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}
