use super::*;
use crate::vcs::log_details::LogDetailsEvent;
use crate::vcs::log_filters::FilterKind;
use crate::vcs::log_view::GitLog;

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

/// Writes `file` and commits it as `message`, authored and committed at `date`.
fn commit_dated(root: &std::path::Path, file: &str, message: &str, date: &str) {
    std::fs::write(root.join(file), message).expect("the file is written");
    git_in(root, &["add", "."]);
    let output = std::process::Command::new("git")
        .args(["commit", "-q", "-m", message])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_DATE", date)
        .env("GIT_COMMITTER_DATE", date)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git commit: {}", String::from_utf8_lossy(&output.stderr));
}

/// A selected merge commit lists the files it changed against every parent: a file both sides
/// changed and the merge resolved anew, and not the files that came from one side only. With Show
/// Changes from Parents its changes against each parent are listed, the other side's files among
/// them.
#[gpui_kit::test]
async fn a_selected_merge_commit_lists_its_changes_against_its_parents(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("f.txt"), "from feature\n").expect("the file is written");
        std::fs::write(root.join("a.txt"), "feature\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "on feature"]);
        git_in(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("m.txt"), "from main\n").expect("the file is written");
        std::fs::write(root.join("a.txt"), "main\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "on main"]);
        git_in(root, &["merge", "--no-ff", "--no-commit", "-X", "ours", "feature"]);
        std::fs::write(root.join("a.txt"), "merged\n").expect("the file is written");
        git_in(root, &["add", "a.txt"]);
        git_in(root, &["commit", "-q", "-m", "Merge feature"]);
    });
    let log = git_log(&workbench, cx);
    wait_long(cx, "the log is read", |cx| subjects(&log, cx).len() == 4);
    assert_eq!(subjects(&log, cx).first().map(String::as_str), Some("Merge feature"));
    let pane = workbench.read_with(cx, |w, _| w.log_details().cloned()).expect("the window has the details");
    cx.update(|window, cx| {
        let focus = log.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("down");
    wait_long(cx, "the merge's changed files", |cx| {
        pane.read_with(cx, |pane, _| !pane.is_loading() && pane.lines() == ["a.txt"])
    });
    let message = pane.update(cx, |pane, cx| pane.message_lines(cx));
    assert_eq!(message.first().map(String::as_str), Some("Merge feature"), "{message:?}");

    let mut options = log.read_with(cx, |log, _| log.options().clone());
    options.changes_from_parents = true;
    pane.update(cx, |_, cx| cx.emit(LogDetailsEvent::SetOptions(Box::new(options))));
    let has = |lines: &[String], name: &str| lines.iter().any(|line| line.trim() == name);
    wait_long(cx, "the changes against each parent", |cx| {
        let lines = pane.read_with(cx, |pane, _| pane.lines());
        !pane.read_with(cx, |pane, _| pane.is_loading()) && has(&lines, "f.txt") && has(&lines, "m.txt")
    });
    let lines = pane.read_with(cx, |pane, _| pane.lines());
    assert_eq!(lines.iter().filter(|line| line.trim() == "a.txt").count(), 2, "a.txt against each parent: {lines:?}");
}

/// The log's Date filter with Last 7 Days shows only the commits of the last week, on every branch.
#[gpui_kit::test]
async fn the_log_date_filter_keeps_the_commits_of_the_chosen_period(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["checkout", "-q", "-b", "archive"]);
        commit_dated(root, "old.txt", "old change", "2020-01-01T12:00:00Z");
        git_in(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("recent.txt"), "recent\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "recent change"]);
    });
    let log = git_log(&workbench, cx);
    wait_long(cx, "the log is read", |cx| subjects(&log, cx).len() == 3);
    assert!(subjects(&log, cx).contains(&"old change".to_owned()));

    let bar = log.read_with(cx, |log, _| log.filter_bar().cloned()).expect("the log has the filter bar");
    bar.update_in(cx, |bar, window, cx| bar.open(FilterKind::Date, window, cx));
    let labels: Vec<String> =
        bar.update(cx, |bar, cx| bar.menu_lines(cx)).unwrap_or_default().into_iter().map(|(label, _)| label).collect();
    let week = labels.iter().position(|label| label == "Last 7 Days");
    let week = week.unwrap_or_else(|| panic!("Last 7 Days in {labels:?}"));
    bar.update_in(cx, |bar, window, cx| bar.choose(week, window, cx));
    wait_long(cx, "the last week's commits", |cx| {
        let shown = subjects(&log, cx);
        shown.len() == 2 && !shown.contains(&"old change".to_owned())
    });
    let shown = subjects(&log, cx);
    assert!(shown.contains(&"recent change".to_owned()) && shown.contains(&"first".to_owned()), "{shown:?}");
    let date = log.read_with(cx, |log, _| log.filters().date.clone()).expect("a date filter is set");
    assert_eq!(date.label, "Last 7 Days");
    assert!(date.after.is_some() && date.before.is_none());
}

/// Add to .gitignore writes the rules for a file and a folder and then reads the status again: the
/// window's changes no longer list them as unversioned.
#[gpui_kit::test]
async fn routes_all_paths_to_gitignore_then_refreshes_status_and_tree(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, init_repo);
    std::fs::write(root.join("shared.log"), "s\n").expect("the file is written");
    std::fs::create_dir_all(root.join("out")).expect("the folder is made");
    std::fs::write(root.join("out/build.bin"), "b\n").expect("the file is written");
    let listed = |cx: &mut VisualTestContext, rel: &str| {
        workbench.read_with(cx, |w, _| w.vcs_state().is_some_and(|s| s.status.iter().any(|f| f.rel_path == rel)))
    };
    wait_long(cx, "the new file and folder are listed", |cx| listed(cx, "shared.log") && listed(cx, "out/"));
    let paths = [root.join("shared.log"), root.join("out")].map(|path| path.to_string_lossy().into_owned());
    run_action(cx, "vcs.ignoreShared", Some(serde_json::json!(paths)));
    wait_long(cx, "the status to be read again", |cx| !listed(cx, "shared.log") && !listed(cx, "out/"));
    let rules = std::fs::read_to_string(root.join(".gitignore")).expect("the rules are written");
    assert!(rules.contains("shared.log") && rules.contains("out"), "{rules}");
}
