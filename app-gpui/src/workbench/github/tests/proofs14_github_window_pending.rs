use super::*;

/// Every action the Pull Requests window names is registered, so that a key, a menu row or the window's own
/// header buttons can run it.
#[gpui_kit::test]
async fn registers_the_window_the_timeline_editor_and_the_actions(cx: &mut TestAppContext) {
    let (_workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let ids = [
        "github.pullRequests",
        "github.createPullRequest",
        "github.checkoutPullRequest",
        "github.openInBrowser",
        "github.refresh",
        "github.pullRequest.show",
        "github.pullRequest.showTimeline",
        "github.pullRequest.refresh",
        "github.pullRequest.timeline.refresh",
        "github.pullRequest.openInBrowser",
        "github.pullRequest.copyUrl",
        "github.pullRequest.checkoutBranch",
        "github.pullRequest.openInWorktree",
        "github.pullRequest.submitReview",
        "github.pullRequest.toggleViewed",
        "github.selectOpenedPullRequest",
        "github.changeRepository",
    ];
    let missing: Vec<&str> = cx.update(|_, cx| {
        let registry = cx.global::<ActionRegistry>();
        ids.into_iter().filter(|id| !registry.contains(id)).collect()
    });
    assert!(missing.is_empty(), "not registered: {missing:?}");
}

/// Create Pull Request… opens the New Pull Request tab once; closing it selects the tab before it.
#[gpui_kit::test]
async fn opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    run_action(cx, "github.pullRequest.show", Some(json!(5)));
    run_action(cx, "github.pullRequest.show", Some(json!(7)));
    run_action(cx, "github.createPullRequest", None);
    click_on(cx, "gh-create-pull-request");
    assert_eq!(window.update(cx, |window, cx| window.tab_titles(cx)), ["app", "#5", "#7", "New Pull Request at app"]);
    assert_eq!(context_text(cx, "githubPrTab").as_deref(), Some("create"));
    assert!(cx.debug_bounds("gh-tab-loading").is_none(), "the tab shows the New Pull Request view");
    click_on(cx, "gh-tab-close-3");
    assert_eq!(window.read_with(cx, |window, _| window.tabs().active), GhTab::PullRequest(7));
}
