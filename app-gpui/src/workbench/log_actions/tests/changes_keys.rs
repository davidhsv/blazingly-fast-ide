use super::super::super::tests::context_is_on;
use super::*;

/// The keyboard in a commit's changed files turns `vcsLogChangesFocus` on: Show Diff's key there shows the selected
/// file's diff of the commit in the preview tab, and F4 opens the file; the keyboard elsewhere turns it off.
#[gpui_kit::test]
async fn in_the_changed_files_show_diff_s_key_shows_the_commit_s_diff_and_f4_jumps_to_source(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        commit_file(root, "a.txt", "two\n", "Second change");
    });
    let log = show_log(&workbench, 2, cx);
    log.update(cx, |log, cx| log.select(0, cx));
    let pane = workbench.read_with(cx, |w, _| w.log_details().cloned()).expect("the window has the details");
    wait_long(cx, "the changed files", |cx| pane.read_with(cx, |pane, _| pane.all_changes().len() == 1));
    let head = git_in(&root, &["rev-parse", "HEAD"]);

    cx.update(|window, cx| window.focus(&log.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(!context_is_on(cx, "vcsLogChangesFocus"), "the keyboard is in the commit list");
    click_on(cx, "log-change:f:a.txt");
    assert!(context_is_on(cx, "vcsLogChangesFocus"), "the keyboard is in the changed files");

    cx.simulate_keystrokes("cmd-d");
    cx.run_until_parked();
    let shown = preview_diff(&workbench, cx).expect("the diff is in the preview tab");
    assert!(shown.path.ends_with("a.txt"));
    assert!(matches!(&shown.right, DiffSide::Rev { rev, .. } if *rev == head));

    cx.update(|window, cx| window.focus(&pane.focus_handle(cx), cx));
    cx.simulate_keystrokes("f4");
    wait_long(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("a.txt")))
    });
    assert!(!context_is_on(cx, "vcsLogChangesFocus"), "the keyboard went to the editor");
}

/// ⌘↓ in a commit's changed files jumps to the selected file's source, as F4 does: the file opens in the editor.
#[gpui_kit::test]
async fn in_the_changed_files_cmd_down_jumps_to_the_selected_file_s_source(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        commit_file(root, "a.txt", "two\n", "Second change");
    });
    let log = show_log(&workbench, 2, cx);
    log.update(cx, |log, cx| log.select(0, cx));
    let pane = workbench.read_with(cx, |w, _| w.log_details().cloned()).expect("the window has the details");
    wait_long(cx, "the changed files", |cx| pane.read_with(cx, |pane, _| pane.all_changes().len() == 1));
    click_on(cx, "log-change:f:a.txt");
    assert!(context_is_on(cx, "vcsLogChangesFocus"), "the keyboard is in the changed files");
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no file is open yet");

    cx.simulate_keystrokes("cmd-down");
    wait_long(cx, "the file opens", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("a.txt")))
    });
    assert!(!context_is_on(cx, "vcsLogChangesFocus"), "the keyboard went to the editor");
}
