use super::*;

/// Debugging a configuration with Run › Debug puts it first among the run chooser's recent
/// configurations, as a run does.
#[gpui_kit::test]
async fn marks_a_configuration_recent_when_a_debug_session_starts(cx: &mut TestAppContext) {
    let (fixture, cx) = open(cx, "start\nloop\nend\n");
    fixture.select_main(cx);
    let driver = fixture.workbench.read_with(cx, |workbench, _| workbench.run_driver().cloned()).expect("runs");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    assert!(store.read_with(cx, |store, _| store.recent.is_empty()));
    run(cx, "run.debug");
    long(cx, "the running session", &|cx| context(cx, "debugSessionRunning"));
    assert_eq!(store.read_with(cx, |store, _| store.recent.clone()), ["main"]);
    run(cx, "debug.stop");
    long(cx, "the end of the session", &|cx| fixture.status(cx) == Some(SessionStatus::Terminated));
    close_project(&fixture, cx);
}
