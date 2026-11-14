use super::proofs13_agents_fixtures::*;
use super::*;
use ide_agent::Isolation;

/// Waits for the dialog titled `title`, past any other one.
fn wait_for_titled_prompt(
    workbench: &Entity<Workbench>,
    title: &str,
    cx: &mut VisualTestContext,
) -> Entity<crate::vcs::prompt_dialogs::PromptDialog> {
    wait_long(cx, title, |cx| {
        workbench.read_with(cx, |w, cx| w.vcs_prompt().is_some_and(|dialog| dialog.read(cx).title() == title))
    });
    workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the dialog is open")
}

/// Deleting a worktree task and a local task, with the project no longer a repository, fails for the worktree task
/// (its folder cannot be removed through git) and still deletes the local one; the failure is then reported once,
/// naming the task that failed, which stays listed with its folder. Once the repository is back, deleting it works.
#[gpui_kit::test]
async fn continues_after_an_error_and_reports_the_failed_task(cx: &mut TestAppContext) {
    let home = start(cx);
    install_fake_agent(home.path(), cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    init_repo(&root);
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    let replied =
        |task: &str, cx: &mut VisualTestContext| !agent_working(&store, task, cx) && shows(&workbench, "agent: ", cx);
    let local = start_with(&workbench, &store, "Local greeting", Vec::new(), Vec::new(), cx);
    wait_long(cx, "the local reply", |cx| replied(&local, cx));
    store.update(cx, |store, cx| {
        store.draft.isolation = Isolation::Worktree;
        store.draft.isolation_chosen = true;
        cx.notify();
    });
    let worktree = start_with(&workbench, &store, "Worktree greeting", Vec::new(), Vec::new(), cx);
    wait_long(cx, "the worktree reply", |cx| replied(&worktree, cx));
    let folder_of = |task: &str, cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| store.task(task).map(|t| (t.isolation, t.cwd.0.clone())))
    };
    let (isolation, worktree_folder) = folder_of(&worktree, cx).expect("the worktree task");
    assert_eq!(isolation, Isolation::Worktree);
    assert!(worktree_folder.is_dir() && worktree_folder != root);

    // Without its repository, the project's worktrees cannot be removed through git.
    let git = root.join(".git");
    let away = root.join(".git-away");
    std::fs::rename(&git, &away).expect("the repository is moved away");
    run_action(cx, "agents.deleteTasks", Some(serde_json::json!([worktree, local])));
    let dialog = wait_for_titled_prompt(&workbench, "Delete Agent Tasks", cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(true), "the worktree folder goes too");
    click_on(cx, "prompt-button-ok");
    let report = wait_for_titled_prompt(&workbench, "Cannot delete agent tasks", cx);
    let body = report.read_with(cx, |dialog, _| dialog.message_text().map(str::to_owned)).unwrap_or_default();
    assert!(body.starts_with("Worktree greeting: "), "the failed task is named: {body}");
    assert!(!body.contains('\n') && !body.contains("Local greeting"), "only the failed task is reported: {body}");
    wait_long(cx, "the local task to be deleted after the failure", |cx| {
        task_ids(&store, cx) == std::slice::from_ref(&worktree)
    });
    assert!(worktree_folder.is_dir(), "the failed task keeps its folder");
    let rows = workbench.read_with(cx, |w, _| w.agents_list()).expect("the tool window has its view");
    let rows = rows.read_with(cx, |list, cx| list.rows(cx));
    assert!(rows.iter().any(|row| row.contains(" | Worktree greeting | ")), "{rows:?}");
    assert!(!rows.iter().any(|row| row.contains(" | Local greeting | ")), "{rows:?}");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the report to close", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));

    std::fs::rename(&away, &git).expect("the repository is back");
    run_action(cx, "agents.deleteTasks", Some(serde_json::json!([worktree])));
    wait_for_titled_prompt(&workbench, "Delete Agent Task", cx);
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the task to be deleted", |cx| task_ids(&store, cx).is_empty());
    wait_long(cx, "the worktree folder to go", |_| !worktree_folder.exists());
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
