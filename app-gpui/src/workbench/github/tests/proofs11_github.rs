use super::*;
use crate::github::list::Chip;

/// A `&'static str` selector built at run time.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// A page of the list's search: `nodes`, and whether a next page follows from `cursor`.
fn search_page(nodes: Vec<Value>, cursor: Option<&str>) -> String {
    let page_info = json!({ "hasNextPage": cursor.is_some(), "endCursor": cursor });
    json!({ "data": { "search": { "pageInfo": page_info, "nodes": nodes } } }).to_string()
}

/// [`github_answers`] with a list of two pages: #1 and #2, then from cursor `c1` the draft #3.
fn paged_answers(head: &str) -> Vec<(&'static str, String)> {
    let mut answers: Vec<(&'static str, String)> =
        github_answers(head).into_iter().filter(|(name, _)| *name != "search.json").collect();
    let first = vec![
        pull_request(1, "Fix the crash on start", "alice", false, "bug"),
        pull_request(2, "Document the setup", "bob", true, "docs"),
    ];
    answers.push(("search.json", search_page(first, Some("c1"))));
    let next = vec![pull_request(3, "Speed up the start", "carol", true, "perf")];
    answers.push(("search-next.json", search_page(next, None)));
    answers
}

/// Makes the fake gh answer a search from cursor `c1` with `search-next.json`.
fn answer_the_next_page(root: &Path) {
    let issue_search = "      *\"query issueSearch\"*)\n";
    let next_page = "        case \"$body\" in *'\"cursor\":\"c1\"'*) answer search-next.json ;; esac\n";
    let script = FAKE_GH.replace(issue_search, &format!("{issue_search}{next_page}"));
    assert_ne!(script, FAKE_GH, "the fake gh has its search case");
    std::fs::write(root.join(".git").join("fake-gh").join("gh"), script).expect("the fake gh is written");
}

/// View Pull Requests shows the Pull Requests tool window and gives it the keyboard.
#[gpui_kit::test]
async fn view_pull_requests_shows_the_tool_window_and_gives_it_the_keyboard(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = github_project(cx, GITHUB_REMOTE, github_answers);
    cx.update(|window, _| window.activate_window());
    assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW)));
    show_list(&workbench, 2, cx);
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(PULL_REQUESTS_TOOL_WINDOW)));
    assert_eq!(context_text(cx, "activeToolWindow").as_deref(), Some(PULL_REQUESTS_TOOL_WINDOW));
}

/// A first page that says more follow is followed by its next page, asked for from its cursor, and the list
/// shows the pull requests of both.
#[gpui_kit::test]
async fn the_list_asks_gh_for_its_next_page_and_appends_it(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, paged_answers);
    answer_the_next_page(&root);
    run_action(cx, "github.pullRequests", None);
    wait_long(cx, "both pages are listed", |cx| listed(&workbench, cx) == [1, 2, 3]);
    assert_eq!(calls_with(&root, "\"cursor\":\"c1\""), 1, "{:?}", gh_calls(&root));
}

/// The State chip's chooser in the window runs a new search with the chosen state; the Author chip's chooser
/// asks gh for the repository's users.
#[gpui_kit::test]
async fn the_chip_choosers_run_a_new_search_and_load_their_users_through_gh(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let window = show_list(&workbench, 2, cx);
    let list = window.read_with(cx, |window, _| window.list().clone());
    click_on(cx, "gh-chip-state");
    let (chip, rows) = list.read_with(cx, |list, _| list.chooser_rows()).expect("the State chooser is open");
    assert_eq!(chip, Chip::State);
    let merged = rows.iter().position(|row| row == "Merged").unwrap_or_else(|| panic!("Merged in {rows:?}"));
    click_on(cx, leak(format!("gh-overlay-{merged}")));
    wait_long(cx, "the merged search", |_| {
        gh_calls(&root).iter().any(|call| call.contains("query issueSearch") && call.contains("is:merged"))
    });
    assert_eq!(list.update(cx, |list, cx| list.chips(cx))[0], ("Merged".to_owned(), true));

    let users = |root: &Path| ["assignees", "collaborators", "contributors"].map(|kind| calls_with(root, kind));
    let before: usize = users(&root).iter().sum();
    click_on(cx, "gh-chip-author");
    assert_eq!(list.read_with(cx, |list, _| list.chooser_rows()).map(|(chip, _)| chip), Some(Chip::Author));
    wait_long(cx, "the users to be asked for", |_| users(&root).iter().sum::<usize>() > before);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
}
