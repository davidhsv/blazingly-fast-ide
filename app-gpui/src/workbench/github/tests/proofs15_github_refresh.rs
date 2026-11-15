//! Refresh Pull Request shows what gh answers now.

use super::*;

/// Rewrites the fake gh's answer `name` in the repository at `root` with `change`.
fn change_answer(root: &Path, name: &str, change: impl FnOnce(&mut Value)) {
    let file = root.join(".git").join("fake-gh").join(name);
    let mut answer: Value =
        serde_json::from_str(&std::fs::read_to_string(&file).expect("the answer is read")).expect("the answer parses");
    change(&mut answer);
    std::fs::write(&file, answer.to_string()).expect("the answer is written");
}

/// After the pull request was renamed and its required check failed on GitHub, Refresh Pull Request shows
/// the new title and the failed required check in the `#1` tab in place of what it showed before.
#[gpui_kit::test]
async fn refresh_pull_request_replaces_what_the_tab_shows_by_the_new_answer(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
    show_list(&workbench, 2, cx);
    let panel = open_first(&workbench, cx);
    let title = |cx: &mut VisualTestContext| {
        panel.read_with(cx, |panel, _| panel.details().map(|details| details.short.title.clone()))
    };
    let says = |cx: &mut VisualTestContext, text: &str| {
        let lines = panel.update(cx, |panel, cx| panel.status_lines(cx));
        lines.iter().any(|line| line == text)
    };
    assert_eq!(title(cx).as_deref(), Some("Fix the crash on start"));
    assert!(says(cx, "All required checks have passed"));

    change_answer(&root, "pull-request.json", |answer| {
        answer["data"]["repository"]["pullRequest"]["title"] = json!("Fix the crash on start and on exit");
    });
    change_answer(&root, "mergeability.json", |answer| {
        let commit = &mut answer["data"]["repository"]["pullRequest"]["commits"]["nodes"][0]["commit"];
        let check = &mut commit["checkSuites"]["nodes"][0]["checkRuns"]["nodes"][0];
        check["conclusion"] = json!("FAILURE");
        check["isRequired"] = json!(true);
    });
    // Nothing asks gh on its own meanwhile: the tab still shows the first answer.
    cx.run_until_parked();
    assert_eq!(title(cx).as_deref(), Some("Fix the crash on start"));

    run_action(cx, "github.pullRequest.refresh", None);
    wait_long(cx, "the new title is shown", |cx| title(cx).as_deref() == Some("Fix the crash on start and on exit"));
    wait_long(cx, "the failed check is shown", |cx| says(cx, "Checks failed"));
    assert!(!says(cx, "All required checks have passed"));
}
