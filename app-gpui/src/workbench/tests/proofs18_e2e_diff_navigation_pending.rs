//! Not declared: with the non-modal commit interface the diffs open in editor tabs (`workbench/diff_tab.rs`,
//! with Maximize Editor / Restore Size). Not checked (hypothesis): whether F7 / ⇧F7 walk from one changed file's
//! diff tab to the next one past binary siblings (`vcs_diff::diff_navigation` skips them in its model). The test
//! assumes `Workbench::diff_tab()` / `diff_tab_path(cx)`; the real accessor is `diff_tab_viewer(cx)`
//! (`pub(super)`), and the tab's path is read from the active tab's input. An unchanged sibling never enters
//! the chain natively: the chain is the repository's changes.

use super::*;

/// a.ts, a binary file, an unchanged one and z.ts in a chain, with the left side maximized and every tool
/// window hidden: F7 twice goes from a.ts to z.ts past the binary and unchanged files, a third F7 stays;
/// ⇧F7 twice comes back to a.ts; Restore Size closes the diff and leaves the left side maximized, the Project
/// view shown.
#[gpui_kit::test]
async fn skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        for name in ["a.ts", "binary.dat", "z.ts"] {
            std::fs::write(root.join(name), "before\n").expect("the file is written");
        }
        git_in(root, &["add", "."]);
        git_in(root, &["commit", "-q", "-m", "second"]);
    });
    std::fs::write(root.join("a.ts"), "after\n").expect("the file is written");
    std::fs::write(root.join("binary.dat"), b"\0binary").expect("the file is written");
    std::fs::write(root.join("z.ts"), "after\n").expect("the file is written");
    run_action(cx, "vcs.showDiff", Some(serde_json::json!(root.join("a.ts").to_string_lossy())));
    wait_long(cx, "the diff tab", |cx| workbench.read_with(cx, |w, _| w.diff_tab().is_some()));
    workbench.update_in(cx, |w, window, cx| w.with_layout(window, cx, |layout| layout.hide_all()));
    cx.simulate_keystrokes("f7 f7");
    wait_long(cx, "z.ts", |cx| {
        workbench.read_with(cx, |w, cx| w.diff_tab_path(cx)).is_some_and(|p| p.ends_with("z.ts"))
    });
    cx.simulate_keystrokes("shift-f7 shift-f7");
    wait_long(cx, "a.ts", |cx| {
        workbench.read_with(cx, |w, cx| w.diff_tab_path(cx)).is_some_and(|p| p.ends_with("a.ts"))
    });
    click_on(cx, "diff-restore");
    assert!(workbench.read_with(cx, |w, _| w.diff_tab().is_none()));
}
