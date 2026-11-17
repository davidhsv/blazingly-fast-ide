use super::*;

/// The run store of the project `root` once it is open again in the window and its configurations are listed.
fn reopen(workbench: &Entity<Workbench>, root: &std::path::Path, cx: &mut VisualTestContext) -> Entity<RunStore> {
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    cx.run_until_parked();
    let root = root.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    wait_long(cx, "listing the configurations", |cx| store.read_with(cx, |store, _| store.loaded));
    store
}

/// The rows the chooser lists right after its section `section`, up to the next section, separator or the Edit
/// row.
fn rows_in_section(lines: &[String], section: &str) -> Vec<String> {
    let Some(start) = lines.iter().position(|line| line == section) else { return Vec::new() };
    let end = |line: &&String| line.starts_with('[') || *line == "---" || *line == "edit";
    lines[start + 1..].iter().take_while(|line| !end(line)).cloned().collect()
}

/// Pins or unpins the chooser's row `name` from its actions menu, opened with → (the chooser is open), and checks
/// that the menu offered `label` and that the chooser is still open afterwards.
fn pin_in_chooser(workbench: &Entity<Workbench>, name: &str, label: &str, cx: &mut VisualTestContext) {
    select_in_chooser(workbench, name, cx);
    cx.simulate_keystrokes("right");
    let chooser = workbench.read_with(cx, |w, _| w.config_chooser()).expect("the chooser is open");
    let menu = chooser.read_with(cx, |chooser, cx| chooser.menu_lines(cx)).expect("the actions menu is open");
    assert_eq!(menu.get(4).map(String::as_str), Some(label), "{menu:?}");
    click_on(cx, "menu-row-0-4");
    let open = workbench.read_with(cx, |w, _| w.config_chooser().is_some());
    assert!(open, "pinning keeps the chooser open");
    let store = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    assert!(store.read_with(cx, |driver, cx| driver.store().read(cx).chooser_open), "the store says it is open");
}

/// Pins made in the chooser's row menus come back when the project is opened again, in the order they were made; an unpin is
/// kept too, and another project has pins of its own (none).
#[gpui_kit::test]
async fn keeps_pins_per_project_across_reloads(cx: &mut TestAppContext) {
    let configs = vec![program("build", "/bin/echo", "b"), program("serve", "/bin/echo", "s")];
    let (workbench, cx, _folders, root, _store) = run_project(cx, configs.clone());
    run_action(cx, "run.chooseConfig", None);
    pin_in_chooser(&workbench, "serve", "Pin", cx);
    pin_in_chooser(&workbench, "build", "Pin", cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let store = reopen(&workbench, &root, cx);
    assert_eq!(store.read_with(cx, |store, _| store.pinned.clone()), ["serve", "build"]);
    run_action(cx, "run.chooseConfig", None);
    let lines = chooser_lines(&workbench, cx).expect("the chooser is open");
    assert_eq!(rows_in_section(&lines, "[Pinned]"), ["serve", "build"], "{lines:?}");
    // Unpinned from the same menu, then read back once more.
    pin_in_chooser(&workbench, "serve", "Unpin", cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    let store = reopen(&workbench, &root, cx);
    assert_eq!(store.read_with(cx, |store, _| store.pinned.clone()), ["build"]);

    let other = tempfile::tempdir().expect("a temporary folder");
    let other_root = other.path().canonicalize().expect("the folder exists");
    ide_run::save_user_configs(&other_root, &configs).expect("the configurations are saved");
    cx.update(|_, cx| cx.global::<Backend>().host.trust.decide(&other_root, true, false))
        .expect("the project is trusted");
    let store = reopen(&workbench, &other_root, cx);
    assert!(store.read_with(cx, |store, _| store.pinned.is_empty()), "the other project has no pins");
    run_action(cx, "run.chooseConfig", None);
    let lines = chooser_lines(&workbench, cx).expect("the chooser is open");
    assert!(!lines.contains(&"[Pinned]".to_owned()), "{lines:?}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
}

/// A configuration run from the chooser is listed under Recent the next time the chooser opens, and the one run
/// last comes first.
#[gpui_kit::test]
async fn marks_a_configuration_recent_when_a_run_session_starts(cx: &mut TestAppContext) {
    let configs = vec![program("hello", "/bin/echo", "hello"), program("bye", "/bin/echo", "bye")];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);
    run_action(cx, "run.chooseConfig", None);
    let lines = chooser_lines(&workbench, cx).expect("the chooser is open");
    assert!(rows_in_section(&lines, "[Recent]").is_empty(), "nothing ran yet: {lines:?}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let ended = |cx: &mut VisualTestContext, name: &str| {
        store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.config.name == name && !s.is_live()))
    };
    for name in ["hello", "bye"] {
        run_action(cx, "run.chooseConfig", None);
        select_in_chooser(&workbench, name, cx);
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the run of echo ends", |cx| ended(cx, name));
    }
    run_action(cx, "run.chooseConfig", None);
    let lines = chooser_lines(&workbench, cx).expect("the chooser is open");
    assert_eq!(rows_in_section(&lines, "[Recent]"), ["bye", "hello"], "{lines:?}");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
}
