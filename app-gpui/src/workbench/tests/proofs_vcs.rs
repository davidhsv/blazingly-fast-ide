use super::*;
use crate::vcs::log_view::GitLog;

fn changed(lines: Vec<crate::vcs_diff::view::PaneLine>) -> Vec<String> {
    lines.into_iter().filter(|line| line.kind.is_some()).map(|line| line.text).collect()
}

/// The Git tool window's log, shown.
fn git_log(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<GitLog> {
    run_action(cx, "toolwindow.git", None);
    workbench
        .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<GitLog>().ok())
        .expect("the Git window holds the log")
}

fn subjects(log: &Entity<GitLog>, cx: &mut VisualTestContext) -> Vec<String> {
    log.read_with(cx, |log, _| log.commits().iter().map(|commit| commit.summary.clone()).collect())
}

/// Compare Previous File in the diff window goes back to the group's file before, and the window's settings button
/// opens its settings menu over the window, which Escape closes.
#[gpui_kit::test]
async fn compare_previous_file_walks_back_and_the_diff_settings_menu_opens(cx: &mut TestAppContext) {
    use crate::vcs::commit_view::CommitPanel;
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), "one\ntwo\nthree\n").expect("the file is written");
        std::fs::write(root.join("b.txt"), "x\ny\nz\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
    });
    std::fs::write(root.join("a.txt"), "one\nTWO\nthree\n").expect("the file is written");
    std::fs::write(root.join("b.txt"), "x\nY\nz\n").expect("the file is written");
    run_action(cx, "toolwindow.commit", None);
    let changes = workbench
        .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<CommitPanel>().ok())
        .expect("the Commit window holds the changes");
    wait_long(cx, "the changes", |cx| changes.read_with(cx, |changes, _| changes.lines().len() == 3));
    cx.simulate_keystrokes("down down");
    run_action(cx, "vcs.showDiff", None);
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff is shown");
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    wait_long(cx, "a.txt's diff", |cx| viewer.read_with(cx, |viewer, _| viewer.difference_count() == 1));
    assert!(dialog.read_with(cx, |dialog, _| dialog.data().path.ends_with("a.txt")));

    run_action(cx, "diff.nextFile", None);
    wait_long(cx, "b.txt's diff", |cx| {
        dialog.read_with(cx, |dialog, _| dialog.data().path.ends_with("b.txt"))
            && changed(viewer.read_with(cx, |viewer, _| viewer.right_lines())) == ["Y"]
    });
    run_action(cx, "diff.prevFile", None);
    wait_long(cx, "a.txt's diff again", |cx| {
        dialog.read_with(cx, |dialog, _| dialog.data().path.ends_with("a.txt"))
            && changed(viewer.read_with(cx, |viewer, _| viewer.right_lines())) == ["TWO"]
    });
    assert_eq!(changed(viewer.read_with(cx, |viewer, _| viewer.left_lines())), ["two"]);

    assert!(workbench.read_with(cx, |w, _| w.popup.is_none()));
    run_action(cx, "diff.showSettings", None);
    assert!(workbench.read_with(cx, |w, _| w.popup.is_some()), "the settings menu is open");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.popup.is_none()), "Escape closes the menu");
    assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_some()), "the diff stays");
}

/// The User filter of the Git log lists the repository's authors, read from the repository; choosing one leaves
/// only that author's commits in the log.
#[gpui_kit::test]
async fn the_log_user_filter_lists_the_authors_and_keeps_the_chosen_ones_commits(cx: &mut TestAppContext) {
    use crate::vcs::log_filters::FilterKind;
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("b.txt"), "bob\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["-c", "user.name=Bob", "-c", "user.email=bob@example.test", "commit", "-q", "-m", "by bob"]);
    });
    let log = git_log(&workbench, cx);
    wait_long(cx, "the log is read", |cx| subjects(&log, cx).len() == 2);
    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    bar.update_in(cx, |bar, window, cx| bar.open(FilterKind::User, window, cx));
    let labels = |cx: &mut VisualTestContext| -> Vec<String> {
        let lines = bar.update(cx, |bar, cx| bar.menu_lines(cx)).unwrap_or_default();
        lines.into_iter().map(|(label, _)| label).collect()
    };
    let (ada, bob) = ("Ada <ada@example.test>", "Bob <bob@example.test>");
    wait_long(cx, "the authors", |cx| {
        let shown = labels(cx);
        shown.iter().any(|label| label == ada) && shown.iter().any(|label| label == bob)
    });
    let index = labels(cx).iter().position(|label| label == bob).expect("Bob is listed");
    bar.update_in(cx, |bar, window, cx| bar.choose(index, window, cx));
    wait_long(cx, "Bob's commits", |cx| subjects(&log, cx) == ["by bob"]);
    assert_eq!(log.read_with(cx, |log, _| log.filters().authors.clone()), [bob]);
}

/// With line counts turned on in the details' view options, a changed file of the selected commit shows the lines
/// the commit added and removed.
#[gpui_kit::test]
async fn the_commit_details_show_the_added_and_removed_lines_of_a_file(cx: &mut TestAppContext) {
    use crate::vcs::log_details::LogDetailsEvent;
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), "two\nthree\nfour\n").expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "three for one"]);
    });
    let log = git_log(&workbench, cx);
    wait_long(cx, "the log is read", |cx| subjects(&log, cx).len() == 2);
    let pane = workbench.read_with(cx, |w, _| w.log_details().cloned()).expect("the window has the details");
    cx.update(|window, cx| {
        let focus = log.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("down");
    wait_long(cx, "the changed file", |cx| {
        pane.read_with(cx, |pane, _| !pane.is_loading() && pane.lines() == ["a.txt"])
    });

    // The view options' Show Line Counts.
    let mut options = log.read_with(cx, |log, _| log.options().clone());
    options.show_line_stats = true;
    pane.update(cx, |_, cx| cx.emit(LogDetailsEvent::SetOptions(Box::new(options))));
    wait_long(cx, "the line counts", |cx| pane.read_with(cx, |pane, _| pane.lines() == ["a.txt +3 \u{2212}1"]));
}

/// Deleting a branch that another worktree has checked out asks first; Delete removes the worktree, its folder
/// and then the branch.
#[gpui_kit::test]
async fn deleting_a_branch_checked_out_in_a_worktree_removes_the_worktree_once_confirmed(cx: &mut TestAppContext) {
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let worktree = elsewhere.path().canonicalize().expect("the folder exists").join("wt");
    let worktree_path = worktree.to_string_lossy().into_owned();
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["branch", "topic"]);
        git_in(root, &["worktree", "add", "-q", &worktree_path, "topic"]);
    });
    assert!(git_in(&root, &["worktree", "list"]).contains(&worktree_path));

    choose_in_branch_menu(&workbench, "topic", "Delete", cx);
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Delete Branch");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the branch to go", |_| !git_branches(&root).contains(&"topic".to_owned()));
    assert!(!git_in(&root, &["worktree", "list"]).contains(&worktree_path), "the worktree is removed");
    assert!(!worktree.exists(), "its folder is deleted");
    wait_for_notice(&workbench, "Deleted branch topic", cx);
}
