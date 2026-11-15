use super::*;

/// A plugin's notification shows as a balloon with its three buttons; a click on the first runs the
/// publishing plugin's command with that button's argument, line break included, and takes the
/// balloon away.
#[cfg(unix)]
#[gpui_kit::test]
async fn plugin_notification_actions_run_their_command_and_dismiss_the_balloon(cx: &mut TestAppContext) {
    let home = start(cx);
    let bin = home.path().join("bin");
    program(&bin, "jobs", "#!/bin/sh\nexit 0\n");
    programs_in(&bin, cx);
    let folder = plugin_copy(home.path(), "jobs", str::to_owned);
    let id = link(cx, &folder, &["project.read"], "notify-actions");
    let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    run_action(cx, &plugin_action_id(&id, "notify-actions"), None);
    let balloon = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| {
            w.balloons.read(cx).shown().iter().find(|entry| entry.title == "Choose an action").cloned()
        })
    };
    wait(cx, "the balloon", |cx| balloon(cx).is_some());
    let entry = balloon(cx).expect("the balloon");
    let labels: Vec<String> = entry
        .actions
        .iter()
        .filter_map(|action| match action {
            EntryAction::Own(own) => Some(own.label.clone()),
            EntryAction::DontShowAgain { .. } => None,
        })
        .collect();
    assert_eq!(labels, ["Action 1", "Action 2", "Action 3"]);
    let first = entry
        .actions
        .iter()
        .position(|action| matches!(action, EntryAction::Own(own) if own.label == "Action 1"))
        .expect("the first button");
    cx.run_until_parked();
    for index in 0..entry.actions.len() {
        let selector: &'static str = Box::leak(format!("balloon-{}-action-{index}", entry.id).into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
    }

    let selector: &'static str = Box::leak(format!("balloon-{}-action-{first}", entry.id).into_boxed_str());
    let at = cx.debug_bounds(selector).expect("the button is drawn").center();
    cx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
    assert!(balloon(cx).is_none(), "the balloon is dismissed");
    let shown: &'static str = Box::leak(format!("balloon-{}", entry.id).into_boxed_str());
    assert!(cx.debug_bounds(shown).is_none(), "the balloon is no longer drawn");
    wait(cx, "the plugin ran the command with the argument", |cx| {
        context("plugin.jobs.notification-action", cx) == Some(ContextValue::from("1\nvalue"))
    });
}
