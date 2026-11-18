use super::*;

/// A project whose repository holds another one, `inner`, on branch `dev` with a branch `side` that adds `b.txt`.
fn nested_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    let (workbench, cx, dirs, root) = git_project(cx, |root| {
        init_repo(root);
        let inner = root.join("inner");
        std::fs::create_dir(&inner).expect("the folder is made");
        init_repo(&inner);
        git_in(&inner, &["checkout", "-q", "-b", "dev"]);
        git_in(&inner, &["checkout", "-q", "-b", "side"]);
        std::fs::write(inner.join("b.txt"), "side\n").expect("the file is written");
        git_in(&inner, &["add", "."]);
        git_in(&inner, &["commit", "-q", "-m", "side work"]);
        git_in(&inner, &["checkout", "-q", "dev"]);
    });
    wait_long(cx, "both repositories are found", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.roots.len() == 2))
    });
    (workbench, cx, dirs, root)
}

// src/features/vcs/__tests__/operationRepo.test.ts › operationRepo › starts on the repository of the action's files
// (context menus)
/// Merge… on a file of the nested repository asks to merge into that repository's branch, and merges there.
#[gpui_kit::test]
async fn merge_starts_on_the_repository_of_the_action_s_file(cx: &mut TestAppContext) {
    let (workbench, cx, _dirs, root) = nested_project(cx);
    let inner = root.join("inner");
    let file = inner.join("a.txt").to_string_lossy().into_owned();

    run_action(cx, "vcs.merge", Some(json!([file])));
    let dialog = wait_for_form(&workbench, "Merge into dev", cx);
    dialog.update(cx, |dialog, cx| dialog.choose("branch", "side", cx));
    press(&dialog, OK, cx);
    wait_for_notice(&workbench, "Merged side into dev", cx);
    assert_eq!(read(&inner.join("b.txt")), "side\n", "the nested repository has the merged file");
    assert!(!root.join("b.txt").exists(), "the outer repository is left alone");
    assert_eq!(git_head(&root), "main");
}

/// Rebase… with `{ repo }` starts on that repository: its branch is the one rebased.
#[gpui_kit::test]
async fn rebase_starts_on_the_repository_the_action_names(cx: &mut TestAppContext) {
    let (workbench, cx, _dirs, root) = nested_project(cx);
    let inner = root.join("inner").to_string_lossy().into_owned();

    run_action(cx, "vcs.rebase", Some(json!({ "repo": inner })));
    let dialog = wait_for_form(&workbench, "Rebase", cx);
    wait_long(cx, "the nested repository's branch is named", |cx| {
        rows(&dialog, cx).contains(&FormRow::Label("Rebase dev onto:".to_owned()))
    });
}

/// Update with `{ repo }` (the branch menus' Update) pulls that repository only: the nested one takes the commit its
/// remote has, though the outer one has no remote to pull from.
#[gpui_kit::test]
async fn update_with_a_repository_pulls_that_repository_only(cx: &mut TestAppContext) {
    let remote = tempfile::tempdir().expect("a temporary folder");
    let parent = remote.path().canonicalize().expect("the folder exists");
    let bare = parent.join("origin.git").to_string_lossy().into_owned();
    git_in(&parent, &["init", "-q", "--bare", "-b", "main", &bare]);
    let (workbench, cx, _dirs, root) = git_project(cx, |root| {
        init_repo(root);
        let inner = root.join("inner");
        std::fs::create_dir(&inner).expect("the folder is made");
        init_repo(&inner);
        git_in(&inner, &["remote", "add", "origin", &bare]);
        git_in(&inner, &["push", "-q", "-u", "origin", "main"]);
    });
    wait_long(cx, "both repositories are found", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.roots.len() == 2))
    });
    push_from_another_clone(&parent, "c.txt", "third");
    let inner = root.join("inner");
    let before = git_in(&root, &["rev-parse", "HEAD"]);

    run_action(cx, "vcs.update", Some(json!({ "repo": inner.to_string_lossy() })));
    let dialog = wait_for_form(&workbench, "Update Project", cx);
    press(&dialog, OK, cx);
    wait_long(cx, "the update", |_| git_in(&inner, &["log", "-1", "--format=%s"]) == "third");
    wait_for_notice(&workbench, "1 file updated in 1 commit", cx);
    assert_eq!(git_in(&root, &["rev-parse", "HEAD"]), before, "the outer repository is not pulled");
}

// src/features/vcs/__tests__/operationRepo.test.ts › operationRepo › from the keyboard, uses the focused Project
// view's selection, else the active editor's file
/// Without an argument Merge… and Pull… start on the repository of the active editor's file.
#[gpui_kit::test]
async fn merge_and_pull_without_an_argument_start_on_the_active_editor_s_repository(cx: &mut TestAppContext) {
    let (workbench, cx, _dirs, root) = nested_project(cx);
    let file = root.join("inner").join("a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file))
    });

    run_action(cx, "vcs.merge", None);
    let dialog = wait_for_form(&workbench, "Merge into dev", cx);
    press(&dialog, "cancel", cx);
    assert!(no_form(&workbench, cx));

    run_action(cx, "vcs.pull", None);
    wait_for_form(&workbench, "Pull to dev", cx);
}
