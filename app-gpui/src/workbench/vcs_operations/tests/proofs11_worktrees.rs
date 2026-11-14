use super::*;

/// The detail of the row `key` of the Worktrees dialog.
fn detail(dialog: &Entity<GitFormDialog>, key: &str, cx: &mut VisualTestContext) -> String {
    let rows = list(dialog, cx);
    let row = rows.iter().find(|row| row.key == key);
    row.map(|row| row.detail.clone()).unwrap_or_else(|| panic!("{key} in {rows:?}"))
}

/// Whether the dialog's button `id` is enabled, and its tooltip.
fn button(dialog: &Entity<GitFormDialog>, id: &str, cx: &mut VisualTestContext) -> (bool, Option<String>) {
    let spec = dialog.update(cx, |dialog, cx| dialog.spec(cx));
    let button = spec.button(id).unwrap_or_else(|| panic!("{id} is offered"));
    (button.enabled, button.tooltip.clone())
}

/// A project opened in a worktree linked to a repository in another folder, on branch `feature`. Also the
/// folder of that repository's main worktree.
fn linked_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 3], PathBuf, PathBuf) {
    let place = tempfile::tempdir().expect("a temporary folder");
    let main = place.path().canonicalize().expect("the folder exists").join("app");
    std::fs::create_dir_all(&main).expect("the folder is made");
    init_repo(&main);
    let (workbench, cx, [home, folder], root) = git_project(cx, |root| {
        git_in(&main, &["worktree", "add", "-q", "-b", "feature", &root.to_string_lossy()]);
    });
    (workbench, cx, [home, folder, place], root, main)
}

/// With the project in a linked worktree, Worktrees… marks it current and its Remove is disabled, saying it
/// contains the open project.
#[gpui_kit::test]
async fn disables_removal_of_the_worktree_containing_project(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _main) = linked_project(cx);
    run_action(cx, "vcs.worktrees", None);
    let dialog = wait_for_form(&workbench, "Worktrees", cx);
    let key = format!("w:{}", root.to_string_lossy());
    wait_long(cx, "the worktrees are listed", |cx| list(&dialog, cx).iter().any(|row| row.key == key));
    assert!(detail(&dialog, &key, cx).starts_with("current"), "{}", detail(&dialog, &key, cx));
    dialog.update(cx, |dialog, cx| dialog.choose("worktrees", &key, cx));
    cx.run_until_parked();
    let (enabled, tooltip) = button(&dialog, "remove", cx);
    assert!(!enabled, "the project's own worktree cannot be removed");
    assert_eq!(tooltip.as_deref(), Some("Cannot remove a worktree containing the open project."));
    press(&dialog, "remove", cx);
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "Remove asks nothing");
    assert!(root.exists(), "the worktree stays");
}

/// The main worktree cannot be removed from the Worktrees dialog, though it can be opened.
#[gpui_kit::test]
async fn keeps_the_main_worktree_protected(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root, main) = linked_project(cx);
    run_action(cx, "vcs.worktrees", None);
    let dialog = wait_for_form(&workbench, "Worktrees", cx);
    let key = format!("w:{}", main.to_string_lossy());
    wait_long(cx, "the main worktree is listed", |cx| list(&dialog, cx).iter().any(|row| row.key == key));
    assert!(detail(&dialog, &key, cx).starts_with("main"), "{}", detail(&dialog, &key, cx));
    dialog.update(cx, |dialog, cx| dialog.choose("worktrees", &key, cx));
    cx.run_until_parked();
    assert!(!button(&dialog, "remove", cx).0, "the main worktree is protected");
    assert!(button(&dialog, "open", cx).0, "it can still be opened");
    press(&dialog, "remove", cx);
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "Remove asks nothing");
    assert!(main.join(".git").is_dir(), "the main worktree stays");
}

/// A project holding a repository and a worktree linked to it opens both as roots: Worktrees… shows the
/// repository once, with both worktrees under it, each marked current.
#[gpui_kit::test]
async fn marks_both_opened_roots_current_while_grouping_their_repository_once(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        let main = root.join("app");
        std::fs::create_dir_all(&main).expect("the folder is made");
        init_repo(&main);
        let linked = root.join("app-feature");
        git_in(&main, &["worktree", "add", "-q", "-b", "feature", &linked.to_string_lossy()]);
    });
    let (main, linked) = (root.join("app"), root.join("app-feature"));
    wait_long(cx, "both roots are read", |cx| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|state| state.roots.len() == 2))
    });
    run_action(cx, "vcs.worktrees", None);
    let dialog = wait_for_form(&workbench, "Worktrees", cx);
    let keys = [main, linked].map(|path| format!("w:{}", path.to_string_lossy()));
    wait_long(cx, "the worktrees are listed", |cx| {
        let rows = list(&dialog, cx);
        keys.iter().all(|key| rows.iter().any(|row| &row.key == key))
    });
    for key in &keys {
        assert!(detail(&dialog, key, cx).starts_with("current"), "{key}: {}", detail(&dialog, key, cx));
    }
    let rows = list(&dialog, cx);
    assert_eq!(rows.len(), 3, "one repository row and its two worktrees: {rows:?}");
    assert_eq!(rows.iter().filter(|row| row.depth == 1).count(), 2, "{rows:?}");
}
