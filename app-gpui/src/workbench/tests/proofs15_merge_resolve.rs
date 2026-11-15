//! Resolve Conflicts… on the title bar's widget of a stopped merge.

use super::*;

/// With a merge stopped on conflicts and its Conflicts dialog closed, Resolve Conflicts… on the
/// title bar opens the dialog again on the conflicted file; the merge stays stopped.
#[gpui_kit::test]
async fn resolve_conflicts_on_the_title_bar_opens_the_conflicts_dialog_again(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        git_in(root, &["checkout", "-q", "-b", "feature"]);
        std::fs::write(root.join("a.txt"), "feature\n").expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "feature side"]);
        git_in(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), "main\n").expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "main side"]);
    });
    choose_in_branch_menu(&workbench, "feature", "Merge 'feature' into 'main'", cx);
    wait_long(cx, "the merge stops", |cx| workbench.read_with(cx, |w, cx| w.merge_rebase_widget(cx).is_some()));
    wait_long(cx, "the Conflicts dialog", |cx| workbench.read_with(cx, |w, _| w.conflicts_dialog().is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.conflicts_dialog().is_none()), "Escape closes the dialog");
    let merge = workbench.read_with(cx, |w, cx| w.merge_rebase_widget(cx).cloned()).expect("the buttons show");
    assert_eq!(merge.read_with(cx, |merge, cx| merge.labels(cx))[0], "Resolve Conflicts…");

    click_on(cx, "merge-rebase-0");
    wait_long(cx, "the Conflicts dialog again", |cx| workbench.read_with(cx, |w, _| w.conflicts_dialog().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.conflicts_dialog()).expect("the dialog is open");
    let names: Vec<String> = dialog.read_with(cx, |dialog, _| dialog.rows().into_iter().map(|row| row.name).collect());
    assert_eq!(names, ["a.txt"]);
    assert!(root.join(".git/MERGE_HEAD").exists(), "the merge is still stopped");
    assert_eq!(branch_label(&workbench, cx), "Merging main");
}
