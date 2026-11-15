//! Not declared: the flows below need steps the merge editor does not have.
//!
//! - Per-change controls from the keyboard: the dividers' Accept arrows and Ignore crosses
//!   (`merge-accept-<side>-<i>`, `merge-ignore-<side>-<i>` in `vcs/merge_view.rs`) take clicks only; they need a
//!   focus handle each (Tab order) and Enter / Space to press the focused one.
//! - Two merge editors: a window shows one merge editor (`Workbench::merge_view`); a second conflicted file opens
//!   in its place. Two editors side by side, each with its own commands following its focus, need the merge
//!   editor to be an editor tab (a `Content` of its own per file) rather than the window's single one.

use super::proofs_merge_fixtures::*;
use super::*;

const BASE: &str = "const total = price + tax;\np1\np2\nc\np1\np2\nsame\np1\np2\nend\n";
const OURS: &str = "const total = cost + tax;\np1\np2\nc\np1\np2\nmine\np1\np2\nend\n";
const THEIRS: &str = "const total = price + fee;\np1\np2\nC\np1\np2\nyours\np1\np2\nend\n";

/// Enter on Yours' second Accept arrow, focused, takes `mine`; Space on Theirs' third Ignore cross settles it.
#[gpui_kit::test]
async fn per_change_controls_also_work_from_the_keyboard(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| diverged(root, BASE, OURS, THEIRS));
    let view = open_merge(&workbench, cx);
    view.update_in(cx, |view, window, cx| view.focus_control("merge-accept-left-1", window, cx));
    cx.simulate_keystrokes("enter");
    assert!(result(&view, cx).contains("\nmine\n"));
    view.update_in(cx, |view, window, cx| view.focus_control("merge-ignore-right-2", window, cx));
    cx.simulate_keystrokes("space");
    assert_eq!(status(&view, cx), "2 changes left, 1 conflict");
}

/// With two merge editors, Resolve Simple Conflicts in the first changes only the first; Ignore from the second's
/// context menu changes only the second.
#[gpui_kit::test]
async fn merge_actions_follow_the_focused_instance_and_survive_a_context_menu(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, |root| diverged(root, BASE, OURS, THEIRS));
    let first = open_merge(&workbench, cx);
    let second = workbench.read_with(cx, |w, _| w.merge_views().get(1).cloned()).expect("a second merge editor");
    run_action(cx, "merge.resolveSimple", None);
    assert_eq!(status(&first, cx), "1 change left, 1 conflict");
    assert_eq!(status(&second, cx), "3 changes left, 2 conflicts");
}
