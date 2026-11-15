use super::*;

/// The Run tool window's stripe button shows how many processes run: one while a long program runs, none
/// once it is stopped.
#[gpui_kit::test]
async fn the_run_stripe_button_shows_the_number_of_running_processes(cx: &mut TestAppContext) {
    use crate::platform::actions::execute_action_or_throw;
    use crate::run::configs::blank_config;
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    let nap = ide_run::RunConfig {
        command: "/bin/sleep".to_owned(),
        args: vec!["30".to_owned()],
        ..blank_config("nap", "nap", "program")
    };
    ide_run::save_user_configs(&root, &[nap]).expect("the configuration is saved");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
    let store = driver.read_with(cx, |driver, _| driver.store().clone());
    wait_long(cx, "listing the configurations", |cx| store.read_with(cx, |store, _| store.loaded));
    let badge = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.stripe_badge(RUN_TOOL_WINDOW, cx));
    assert_eq!(badge(cx), None, "nothing runs yet");

    store.update(cx, |store, _| store.select_config(Some("nap")));
    cx.update(|window, cx| execute_action_or_throw("run.run", None, window, cx)).expect("it runs");
    wait_long(cx, "the program runs", |cx| store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.is_live())));
    assert_eq!(badge(cx), Some(1));
    wait_long(cx, "the badge is drawn", |cx| cx.debug_bounds("stripe-badge").is_some());
    let button = cx.debug_bounds("stripe:run").expect("the Run stripe button is drawn");
    let drawn = cx.debug_bounds("stripe-badge").expect("the badge is drawn");
    assert!(button.contains(&drawn.origin) || drawn.intersects(&button), "the badge is on the Run button");

    cx.update(|window, cx| execute_action_or_throw("run.stop", None, window, cx)).expect("it stops");
    wait_long(cx, "the program ends", |cx| store.read_with(cx, |store, _| !store.sessions.iter().any(|s| s.is_live())));
    assert_eq!(badge(cx), None);
    wait_long(cx, "the badge is gone", |cx| cx.debug_bounds("stripe-badge").is_none());
}
