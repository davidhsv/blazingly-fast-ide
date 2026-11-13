use super::*;

/// Update Project on a branch with a commit of its own counts what came in from where the branch met its upstream,
/// not from the branch's own tip: one file in one commit, though the local commit changed another file.
#[gpui_kit::test]
async fn update_project_tells_what_came_in_from_where_the_branch_met_its_upstream(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, parent) = origin_project(cx);
    push_from_another_clone(&parent, "c.txt", "third");
    std::fs::write(root.join("l.txt"), "local\n").expect("the file is written");
    git_in(&root, &["add", "."]);
    git_in(&root, &["commit", "-q", "-m", "local"]);

    run_action(cx, "vcs.update", None);
    let dialog = wait_for_form(&workbench, "Update Project", cx);
    dialog.update(cx, |dialog, cx| dialog.choose("type", "rebase", cx));
    press(&dialog, OK, cx);
    wait_long(cx, "the update", |_| git_in(&root, &["log", "-2", "--format=%s"]) == "local\nthird");
    // Counted from the merge base, only c.txt came in; from the local tip l.txt would count too.
    wait_for_notice(&workbench, "1 file updated in 1 commit", cx);
    assert!(!notification_titles(&workbench, cx).iter().any(|title| title == "2 files updated in 1 commit"));
}
