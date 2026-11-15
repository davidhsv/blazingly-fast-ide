use super::proofs18_e2e_fixtures::{open_from_tree, primary};
use super::*;

/// ⌘K with an edited file open shows the Commit window with the keyboard in the message: the modified
/// tracked file is included and the unversioned one left out. Commit without a message commits nothing
/// and says so at the field; with a typed message it commits the included file only, the unversioned
/// file stays unversioned and unstaged, the commit is reported, and the committed file's checkbox is
/// gone from the tree.
#[gpui_kit::test]
async fn cmd_k_commits_the_checked_changes_with_the_typed_message(cx: &mut TestAppContext) {
    const MESSAGE: &str = "Return order DTOs from create";
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("build.gradle.kts"), "plugins {}\n").expect("the file is written");
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "build"]);
        std::fs::write(root.join("build.gradle.kts"), "plugins { java }\n").expect("the file is written");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    });
    open_from_tree(&workbench, &root.join("build.gradle.kts"), cx);

    cx.simulate_keystrokes(&primary("k"));
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(COMMIT_TOOL_WINDOW)), "⌘K shows Commit");
    let (panel, commit_box) = commit_parts(&workbench, cx);
    wait_long(cx, "the changes are listed", |cx| {
        panel.read_with(cx, |panel, _| panel.lines())
            == ["Changes (1)", "  build.gradle.kts", "Unversioned Files (1)", "  notes.txt"]
    });
    // Tracked changes start included; unversioned files start left out.
    assert_eq!(panel.read_with(cx, |panel, cx| panel.included_files(cx)), ["build.gradle.kts"]);
    assert!(cx.debug_bounds("commit-include-build.gradle.kts").is_some(), "the file's checkbox is drawn");
    assert!(cx.debug_bounds("commit-include-Unversioned Files").is_some(), "the group's checkbox is drawn");
    wait_long(cx, "the box counts the included file", |cx| commit_box.read_with(cx, |b, _| b.input().included == 1));
    assert!(
        cx.update(|window, cx| commit_box.read(cx).message_area().focus_handle(cx).is_focused(window)),
        "the message has the keyboard"
    );

    // Committing without a message is refused.
    click_on(cx, "commit-commit");
    assert!(commit_box.read_with(cx, |commit_box, _| commit_box.empty_error()), "Specify commit message");
    assert_eq!(git_in(&root, &["rev-list", "--count", "HEAD"]), "2", "nothing is committed");

    cx.simulate_input(MESSAGE);
    click_on(cx, "commit-commit");
    wait_long(cx, "the commit is reported", |cx| {
        notification_titles(&workbench, cx).contains(&format!("1 file committed: {MESSAGE}"))
    });
    assert_eq!(git_in(&root, &["log", "-1", "--format=%s"]), MESSAGE);
    // The commit takes the checked file itself; the unversioned file is neither committed nor staged.
    assert_eq!(git_in(&root, &["show", "--name-only", "--format=", "HEAD"]), "build.gradle.kts");
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "?? notes.txt");
    wait_long(cx, "the tree follows", |cx| {
        panel.read_with(cx, |panel, _| panel.lines()) == ["Unversioned Files (1)", "  notes.txt"]
    });
    assert!(cx.debug_bounds("commit-include-build.gradle.kts").is_none(), "the committed file's checkbox is gone");
}
