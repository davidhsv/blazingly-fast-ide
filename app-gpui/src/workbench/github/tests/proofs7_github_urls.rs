use super::*;

/// The page GitHub gives for pull request #7, which is neither open in a tab nor in the list: its repository was
/// renamed, so the page is not the one the connected repository's name would build.
const MOVED: &str = "https://github.com/org/app-renamed/pull/7";

/// The answers of [`github_answers`], with gh's details of a pull request being those of #7 at [`MOVED`].
fn moved_pull_request_answers(head: &str) -> Vec<(&'static str, String)> {
    let mut answers = github_answers(head);
    for (name, text) in &mut answers {
        if *name == "pull-request.json" {
            let mut document: serde_json::Value = serde_json::from_str(text).expect("the answer is JSON");
            let pull_request = &mut document["data"]["repository"]["pullRequest"];
            pull_request["number"] = serde_json::json!(7);
            pull_request["url"] = serde_json::json!(MOVED);
            *text = document.to_string();
        }
    }
    answers
}

/// Open on GitHub with no pull request in context (the list shown, no pull request's tab open) opens the
/// repository's page.
#[gpui_kit::test]
async fn open_on_github_without_a_pull_request_in_context_opens_the_repositorys_page(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    assert_eq!(window.read_with(cx, |window, _| window.context_pull_request()), None, "no tab and no row chosen");
    run_action(cx, "github.openInBrowser", None);
    wait_long(cx, "the repository's page", |cx| cx.opened_url().as_deref() == Some("https://github.com/org/app"));
}

/// Copy Pull Request URL and View Pull Request in Browser for a pull request that is neither open nor listed use
/// the page GitHub gives for it, not one built from the repository's name.
#[gpui_kit::test]
async fn the_url_of_a_pull_request_neither_open_nor_listed_is_the_one_github_gives(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, moved_pull_request_answers);
    show_list(&workbench, 2, cx);
    run_action(cx, "github.pullRequest.copyUrl", Some(serde_json::json!(7)));
    wait_long(cx, "the URL on the clipboard", |cx| {
        cx.read_from_clipboard().and_then(|item| item.text()).as_deref() == Some(MOVED)
    });
    run_action(cx, "github.pullRequest.openInBrowser", Some(serde_json::json!(7)));
    wait_long(cx, "the pull request's page", |cx| cx.opened_url().as_deref() == Some(MOVED));
}
