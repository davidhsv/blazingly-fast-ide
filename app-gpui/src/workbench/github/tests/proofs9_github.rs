use super::*;

/// Commits a change to `src/main.rs` on `fix-crash` and then moves `main` past the point it branched from:
/// the fork point holds `a`, the branch `b` and `c`, `main` now `z`. Returns (main, fork point, branch head).
fn diverged_branches(root: &Path) -> (String, String, String) {
    let commit = |text: &str, message: &str| {
        std::fs::create_dir_all(root.join("src")).expect("the folder is made");
        std::fs::write(root.join("src/main.rs"), text).expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", message]);
        git_in(root, &["rev-parse", "HEAD"])
    };
    let fork = commit("a\n", "Add main");
    git_in(root, &["checkout", "-q", "-b", "fix-crash"]);
    let head = commit("b\nc\n", "Fix the crash");
    git_in(root, &["checkout", "-q", "main"]);
    let main = commit("z\n", "Move main on");
    (main, fork, head)
}

/// Makes gh's details of #1 name `base` and `head` as the pull request's base and head commits.
fn set_pull_request_commits(root: &Path, base: &str, head: &str) {
    let file = root.join(".git").join("fake-gh").join("pull-request.json");
    let text = std::fs::read_to_string(&file).expect("the details answer is read");
    let mut document: serde_json::Value = serde_json::from_str(&text).expect("the answer is JSON");
    let pull_request = &mut document["data"]["repository"]["pullRequest"];
    pull_request["baseRefOid"] = serde_json::json!(base);
    pull_request["headRefOid"] = serde_json::json!(head);
    std::fs::write(&file, document.to_string()).expect("the details answer is written");
}

/// A double click on a changed file of a pull request shows its diff from the point the pull request's branch
/// left the base branch to the branch's head: the base branch's later change to the file is not on the left.
#[gpui_kit::test]
async fn a_double_click_on_a_changed_file_diffs_it_from_the_merge_base_to_the_pull_requests_head(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, root) = github_project(cx, GITHUB_REMOTE, github_answers);
    let (main, fork, head) = diverged_branches(&root);
    assert_ne!(main, fork, "the base branch moved on after the fork");
    set_pull_request_commits(&root, &main, &head);
    show_list(&workbench, 2, cx);
    let panel = open_first(&workbench, cx);
    let paths: Vec<Option<String>> =
        panel.read_with(cx, |panel, _| panel.tree_rows().into_iter().map(|row| row.path).collect());
    let index = paths
        .iter()
        .position(|path| path.as_deref() == Some("src/main.rs"))
        .unwrap_or_else(|| panic!("src/main.rs is listed among {paths:?}"));
    let selector: &'static str = Box::leak(format!("gh-change-{index}").into_boxed_str());
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    let (modifiers, button) = (gpui_kit::Modifiers::default(), gpui_kit::MouseButton::Left);
    cx.simulate_event(gpui_kit::MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(gpui_kit::MouseUpEvent { position, modifiers, button, click_count: 2 });

    wait_long(cx, "the pull request's diff", |cx| workbench.read_with(cx, |w, _| w.diff_dialog().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff is shown");
    assert!(dialog.read_with(cx, |dialog, _| dialog.data().path.ends_with("src/main.rs")));
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    let changed = |lines: Vec<crate::vcs_diff::view::PaneLine>| -> Vec<String> {
        lines.into_iter().filter(|line| line.kind.is_some()).map(|line| line.text).collect()
    };
    wait_long(cx, "both sides of the diff", |cx| viewer.read_with(cx, |viewer, _| viewer.difference_count() == 1));
    assert_eq!(changed(viewer.read_with(cx, |viewer, _| viewer.left_lines())), ["a"], "the merge base, not main's z");
    assert_eq!(changed(viewer.read_with(cx, |viewer, _| viewer.right_lines())), ["b", "c"]);
    assert!(
        !notification_titles(&workbench, cx).iter().any(|title| title == "Cannot show the pull request diff"),
        "{:?}",
        notification_titles(&workbench, cx)
    );
}
