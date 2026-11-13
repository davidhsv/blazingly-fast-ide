use super::*;
use crate::vcs::conflicts_dialog::MERGE;
use crate::vcs::merge_view::{MergeView, status_text};
use std::io::Write as _;

/// `a.txt` before the merge.
pub(super) const LETTERS_BASE: &str = "a\nb\nc\nd\ne\nf\ng\n";
/// `a.txt` on `main` (Yours): the second line conflicts, the last is changed as on `feature`.
pub(super) const LETTERS_OURS: &str = "a\nL\nc\nd\ne\nf\nG\n";
/// `a.txt` on `feature` (Theirs): the second line conflicts, the fourth is changed on this side only.
pub(super) const LETTERS_THEIRS: &str = "a\nR\nc\nD\ne\nf\nG\n";

/// `a.txt` before the merge, and on each side: one line both sides changed in different words.
pub(super) const WORDS_BASE: &str = "let a = 1 + b;\n";
pub(super) const WORDS_OURS: &str = "let x = 1 + b;\n";
pub(super) const WORDS_THEIRS: &str = "let a = 1 + 2;\n";

/// A repository whose `a.txt` reads `base` before `main` and `feature` changed it to `ours` and `theirs`;
/// `main` is checked out, and merging `feature` stops on a conflict.
pub(super) fn diverged(root: &std::path::Path, base: &str, ours: &str, theirs: &str) {
    init_repo(root);
    std::fs::write(root.join("a.txt"), base).expect("the file is written");
    git_in(root, &["commit", "-q", "-am", "base"]);
    git_in(root, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(root.join("a.txt"), theirs).expect("the file is written");
    git_in(root, &["commit", "-q", "-am", "feature side"]);
    git_in(root, &["checkout", "-q", "main"]);
    std::fs::write(root.join("a.txt"), ours).expect("the file is written");
    git_in(root, &["commit", "-q", "-am", "main side"]);
}

/// Changes of `a.txt`: 0 a conflict on line 2, 1 on Theirs only on line 4, 2 the same on both sides on line 7.
pub(super) fn letters(root: &std::path::Path) {
    diverged(root, LETTERS_BASE, LETTERS_OURS, LETTERS_THEIRS);
}

/// Changes of `a.txt`: 0 on Theirs only on line 1, 1 a conflict on line 5, 2 on Yours only on line 9.
pub(super) fn numbers(root: &std::path::Path) {
    diverged(
        root,
        "1\n2\n3\n4\n5\n6\n7\n8\n9\n",
        "1\n2\n3\n4\nfive main\n6\n7\n8\nnine\n",
        "one\n2\n3\n4\nfive feature\n6\n7\n8\n9\n",
    );
}

/// One change of `a.txt`: a conflict whose sides changed different words.
pub(super) fn words(root: &std::path::Path) {
    diverged(root, WORDS_BASE, WORDS_OURS, WORDS_THEIRS);
}

/// Merges `feature` into `main`, which stops on the conflict of `a.txt`, and opens the merge editor on it from
/// the Conflicts dialog; the editor has the keyboard once it shows the versions.
pub(super) fn open_merge(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<MergeView> {
    let dialog = merge_into_conflicts(workbench, cx);
    dialog.update(cx, |dialog, cx| dialog.press(MERGE, cx));
    merge_view_loaded(workbench, cx)
}

/// The merge editor's result.
pub(super) fn result(view: &Entity<MergeView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.text()).expect("the versions are shown")
}

/// What the merge editor's toolbar says is left: "3 changes left, 1 conflict".
pub(super) fn status(view: &Entity<MergeView>, cx: &mut VisualTestContext) -> String {
    let summary = view.read_with(cx, |view, _| view.summary());
    status_text(&summary, &mut crate::platform::i18n::I18n::default())
}

/// Runs the action `id` through the registry as a key or a menu would: whether it was enabled and ran.
pub(super) fn try_action(cx: &mut VisualTestContext, id: &str, arg: Option<serde_json::Value>) -> bool {
    let ran = cx.update(|window, cx| crate::platform::actions::execute_action(id, arg.as_ref(), window, cx));
    cx.run_until_parked();
    ran
}

/// Clicks line `line` (0-based) of the merge editor's pane `pane` at its start: the caret goes there and the
/// pane is the one the commands act in.
pub(super) fn click_line(cx: &mut VisualTestContext, pane: &'static str, line: usize) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(pane).unwrap_or_else(|| panic!("{pane} is drawn"));
    let height = cx.update(|_, cx| {
        let height = crate::theme::length(cx, "lh-code");
        if height > gpui_kit::px(0.0) { height } else { gpui_kit::px(20.0) }
    });
    let at = gpui_kit::point(bounds.left() + gpui_kit::px(2.0), bounds.top() + height * (line as f32 + 0.5));
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// The text the clipboard holds.
pub(super) fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.update(|_, cx| cx.read_from_clipboard()).and_then(|item| item.text())
}

/// Replaces stage `stage` (1 base, 2 ours, 3 theirs) of the conflicted `path` in the index with `text`, as a
/// version read again from disk would differ.
pub(super) fn set_stage(root: &std::path::Path, stage: u8, path: &str, text: &str) {
    std::fs::write(root.join(".git/stage-blob"), text).expect("the blob's text is written");
    let blob = git_in(root, &["hash-object", "-w", ".git/stage-blob"]);
    let entry = format!("100644 {blob} {stage}\t{path}\n");
    let run = || {
        let mut child = std::process::Command::new("git")
            .args(["update-index", "--index-info"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .stdin(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("git runs");
        child.stdin.take().expect("git reads its input").write_all(entry.as_bytes()).expect("the entry is sent");
        child.wait_with_output().expect("git ends")
    };
    let mut output = run();
    // The open window reads the repository too, which holds its index for a moment.
    for _ in 0..100 {
        if output.status.success() || !String::from_utf8_lossy(&output.stderr).contains("index.lock") {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
        output = run();
    }
    assert!(output.status.success(), "update-index: {}", String::from_utf8_lossy(&output.stderr));
}
