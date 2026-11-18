use super::*;

fn balloons(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    workbench.read_with(cx, |w, cx| w.balloons.read(cx).shown().iter().map(|entry| entry.title.clone()).collect())
}

/// With balloons turned off the result of a Git command the user ran still shows as a balloon, as the answer to
/// what they asked; a notice that is not a Git command's result goes to the history only.
#[gpui_kit::test]
async fn a_git_command_s_result_shows_as_a_balloon_while_balloons_are_off(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, _parent) = origin_project(cx);
    cx.update(|_, cx| settings::set("w1", SettingsScope::Global, "notifications.balloons", &json!(false), cx))
        .expect("the setting is written");

    run_action(cx, "vcs.rollback", None);
    wait_for_notice(&workbench, "No changes to roll back", cx);
    assert!(!balloons(&workbench, cx).iter().any(|title| title == "No changes to roll back"));

    run_action(cx, "vcs.fetch", None);
    let done = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| {
            w.notifications()
                .items()
                .iter()
                .find(|item| item.key.as_deref() == Some("vcs.remote") && !item.title.ends_with('…'))
                .map(|item| item.title.clone())
        })
    };
    wait_long(cx, "the fetch is told", |cx| done(cx).is_some());
    let title = done(cx).expect("the fetch's result");
    assert!(balloons(&workbench, cx).contains(&title), "{title} in {:?}", balloons(&workbench, cx));
}
