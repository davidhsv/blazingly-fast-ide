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
