use super::*;

/// What the debug console of the window's active session holds, as one text.
fn debug_console(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    let Some(driver) = workbench.read_with(cx, |w, _| w.debug_driver().cloned()) else { return String::new() };
    driver.read_with(cx, |driver, cx| {
        let state = &driver.debugger().read(cx).state;
        let lines = state.active().map(|id| state.console(id)).unwrap_or_default();
        lines.iter().map(|line| line.text.as_str()).collect()
    })
}

/// Debug Plugin Configuration has the plugin host prepare the configuration before the session starts: the
/// detector names its program `execution-tool`, the preparation puts the program the plugin was granted in its
/// place, and the adapter is launched with that program, as it says in the debug console.
#[cfg(unix)]
#[gpui_kit::test]
async fn the_debug_preparation_changes_the_program_and_the_session_launches_the_changed_one(cx: &mut TestAppContext) {
    let home = start_with_fake_debugger(cx);
    // The fake adapter, saying first which program it was launched with.
    let launch = "    elif command == 'launch':\n        launch = request\n";
    let telling = "    elif command == 'launch':\n        launch = request\n        \
                   event('output', {'category': 'stdout', 'output': 'launched ' + arguments['program'] + '\\n'})\n";
    let adapter = super::super::super::debug::tests::FAKE_ADAPTER.replacen(launch, telling, 1);
    assert!(adapter.contains("'launched '"), "{adapter}");
    std::fs::write(home.path().join("fake_dap.py"), adapter).expect("the adapter script is written");
    let runs_main_fake = |text: &str| text.replace("args = [\"test\"]", "args = [\"main.fake\"]");
    let (_folder, _root, workbench, cx) =
        execution_project(cx, home.path(), runs_main_fake, &[("main.fake", "start\nend\n")]);

    run_action(cx, &plugin_action_id("execution", "debug"), Some(Value::from("detected")));
    wait_long(cx, "the adapter says what it launched", |cx| debug_console(&workbench, cx).contains("launched "));
    let console = debug_console(&workbench, cx);
    let launched = console
        .lines()
        .find_map(|line| line.strip_prefix("launched "))
        .unwrap_or_else(|| panic!("the launch line in {console}"))
        .to_owned();
    assert_ne!(launched, "execution-tool", "the detector's own program is not what runs");
    assert!(
        launched.starts_with('/') && launched.ends_with("/bin/execution-tool"),
        "the program granted to the plugin runs: {launched}"
    );
}
