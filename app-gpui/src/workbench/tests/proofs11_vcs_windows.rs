use super::*;

/// Whether the Project view lists `path` and draws it as ignored.
fn tree_marks_ignored(tree: &Entity<ProjectTree>, path: &std::path::Path, cx: &mut VisualTestContext) -> Option<bool> {
    let at = text_of(path);
    tree.read_with(cx, |tree, _| tree.rows().iter().find(|row| row.node.path == at).map(|row| row.node.ignored))
}

/// Whether the window's changes list `rel` as a change.
fn status_lists(workbench: &Entity<Workbench>, rel: &str, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|s| s.status.iter().any(|f| f.rel_path == rel)))
}

/// A repository with two unversioned files at its top, `shared.log` and `local.log`, both listed in the window.
fn two_unversioned(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    let (workbench, cx, folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("shared.log"), "s\n").expect("the file is written");
        std::fs::write(root.join("local.log"), "l\n").expect("the file is written");
    });
    wait_long(cx, "the new files are listed", |cx| {
        status_lists(&workbench, "shared.log", cx) && status_lists(&workbench, "local.log", cx)
    });
    (workbench, cx, folders, root)
}

/// Add to .gitignore reads the project again after writing the rule: the Project view draws the file as ignored
/// and the changes no longer list it.
#[gpui_kit::test]
async fn add_to_gitignore_reloads_the_project_tree_with_the_file_ignored(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = two_unversioned(cx);
    let tree = project_tree_of(&workbench, cx);
    let file = root.join("shared.log");
    wait_long(cx, "the tree lists the file", |cx| tree_marks_ignored(&tree, &file, cx) == Some(false));

    run_action(cx, "vcs.ignoreShared", Some(serde_json::json!([text_of(&file)])));
    wait_long(cx, "the status to be read again", |cx| !status_lists(&workbench, "shared.log", cx));
    wait_long(cx, "the tree to be read again", |cx| tree_marks_ignored(&tree, &file, cx) == Some(true));
    assert_eq!(tree_marks_ignored(&tree, &root.join("local.log"), cx), Some(false), "the other file is not ignored");
}

/// Add to .git/info/exclude reads the project again after writing the local rule, though that file is outside
/// the project's folders: the changes no longer list the file and the Project view draws it as ignored.
#[gpui_kit::test]
async fn add_to_local_exclude_refreshes_the_status_and_the_project_tree(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = two_unversioned(cx);
    let tree = project_tree_of(&workbench, cx);
    let file = root.join("local.log");
    wait_long(cx, "the tree lists the file", |cx| tree_marks_ignored(&tree, &file, cx) == Some(false));

    run_action(cx, "vcs.ignoreLocal", Some(serde_json::json!([text_of(&file)])));
    wait_long(cx, "the rule is written", |_| {
        std::fs::read_to_string(root.join(".git/info/exclude")).is_ok_and(|rules| rules.contains("local.log"))
    });
    wait_long(cx, "the status to be read again", |cx| !status_lists(&workbench, "local.log", cx));
    wait_long(cx, "the tree to be read again", |cx| tree_marks_ignored(&tree, &file, cx) == Some(true));
    assert!(!root.join(".gitignore").exists(), "the shared rules are left alone");
}

/// A write to .git/info/exclude that saved its rules but then failed, because a .gitignore rule keeps one file
/// visible, is reported as failed and still reads the project again: the file whose rule took effect leaves the
/// changes and shows as ignored in the Project view, the other stays listed.
#[gpui_kit::test]
async fn refreshes_after_a_failed_or_partially_completed_multi_repository_write(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join(".gitignore"), "!/kept.log\n").expect("the rules are written");
        git_in(root, &["add", ".gitignore"]);
        git_in(root, &["commit", "-q", "-m", "rules"]);
        std::fs::write(root.join("other.log"), "o\n").expect("the file is written");
        std::fs::write(root.join("kept.log"), "k\n").expect("the file is written");
    });
    wait_long(cx, "the new files are listed", |cx| {
        status_lists(&workbench, "other.log", cx) && status_lists(&workbench, "kept.log", cx)
    });
    let tree = project_tree_of(&workbench, cx);
    let other = root.join("other.log");
    wait_long(cx, "the tree lists the file", |cx| tree_marks_ignored(&tree, &other, cx) == Some(false));

    let paths = [text_of(&other), text_of(&root.join("kept.log"))];
    run_action(cx, "vcs.ignoreLocal", Some(serde_json::json!(paths)));
    wait_for_notice(&workbench, "Add to .git/info/exclude failed", cx);
    let exclude = std::fs::read_to_string(root.join(".git/info/exclude")).expect("the exclude file is read");
    assert!(exclude.contains("other.log") && exclude.contains("kept.log"), "the rules were saved: {exclude}");
    wait_long(cx, "the status to be read again", |cx| !status_lists(&workbench, "other.log", cx));
    assert!(status_lists(&workbench, "kept.log", cx), "the file .gitignore keeps visible stays listed");
    wait_long(cx, "the tree to be read again", |cx| tree_marks_ignored(&tree, &other, cx) == Some(true));
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "a failed write opens nothing");
}
