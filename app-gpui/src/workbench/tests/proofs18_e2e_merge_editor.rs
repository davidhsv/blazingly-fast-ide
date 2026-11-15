use super::proofs_merge_fixtures::*;
use super::proofs18_e2e_fixtures::{click_menu_row, menu_level, primary};
use super::*;
use crate::vcs_more::merge_actions::MergeCommand;

/// `a.txt` before the merge: a line whose sides change different words, a line Theirs alone changes, and a line
/// both sides change, apart.
const BASE: &str = "const total = price + tax;\np1\np2\nc\np1\np2\nsame\np1\np2\nend\n";
const OURS: &str = "const total = cost + tax;\np1\np2\nc\np1\np2\nmine\np1\np2\nend\n";
const THEIRS: &str = "const total = price + fee;\np1\np2\nC\np1\np2\nyours\np1\np2\nend\n";

fn three_changes(root: &std::path::Path) {
    diverged(root, BASE, OURS, THEIRS);
}

fn press_at(cx: &mut VisualTestContext, selector: &'static str, modifiers: gpui_kit::Modifiers) {
    cx.run_until_parked();
    let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    cx.simulate_click(at, modifiers);
    cx.run_until_parked();
}

/// Right-clicks line `line` (0-based) of the merge editor's pane `pane`, at its start.
fn right_click_line(cx: &mut VisualTestContext, pane: &'static str, line: usize) {
    cx.run_until_parked();
    let bounds = cx.debug_bounds(pane).unwrap_or_else(|| panic!("{pane} is drawn"));
    let height = cx.update(|_, cx| {
        let height = crate::theme::length(cx, "lh-code");
        if height > gpui_kit::px(0.0) { height } else { gpui_kit::px(20.0) }
    });
    let at = gpui_kit::point(bounds.left() + gpui_kit::px(2.0), bounds.top() + height * (line as f32 + 0.5));
    cx.simulate_mouse_down(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// A file with CRLF line breaks and characters outside the Basic Multilingual Plane: the platform-modified
/// click on Yours' Accept arrow resolves the conflict with Yours, leaving nothing to process; Apply writes
/// Yours with its CRLF line breaks and stages the file.
#[gpui_kit::test]
async fn crlf_resolutions_use_utf16_editor_positions_and_preserve_line_endings_on_apply(cx: &mut TestAppContext) {
    let base = "标题😀\r\nunchanged\r\nbase\r\ntail";
    let ours = base.replace("base", "左侧🚀");
    let theirs = base.replace("base", "右侧🧪");
    let (workbench, cx, _folders, root) = git_project(cx, |root| diverged(root, base, &ours, &theirs));
    let view = open_merge(&workbench, cx);
    let modifiers = cx.update(|_, cx| gpui_kit::Modifiers::secondary_key(cx));
    press_at(cx, "merge-accept-left-0", modifiers);
    assert_eq!(status(&view, cx), "All changes processed");
    assert_eq!(result(&view, cx).trim_end(), ours.replace("\r\n", "\n"));

    press_at(cx, "merge-footer-apply", gpui_kit::Modifiers::default());
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "nothing unresolved is asked about");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).expect("the file is read"), ours, "CRLF is kept");
    assert_eq!(git_in(&root, &["ls-files", "-u"]), "", "nothing is left unmerged");
    // Yours is the text HEAD has: staged, the resolved file differs from HEAD in nothing.
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "", "the file is staged");
}

/// Theirs deleted the file Yours changed: Accept Theirs deletes it and stages the deletion.
#[gpui_kit::test]
async fn accepting_a_deleted_version_removes_the_file_before_staging(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), BASE).expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "base"]);
        git_in(root, &["checkout", "-q", "-b", "feature"]);
        git_in(root, &["rm", "-q", "a.txt"]);
        git_in(root, &["commit", "-q", "-m", "feature deletes"]);
        git_in(root, &["checkout", "-q", "main"]);
        std::fs::write(root.join("a.txt"), OURS).expect("the file is written");
        git_in(root, &["commit", "-q", "-am", "main side"]);
    });
    let _view = open_merge(&workbench, cx);
    press_at(cx, "merge-footer-acceptTheirs", gpui_kit::Modifiers::default());
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert!(!root.join("a.txt").exists(), "the file is deleted");
    assert_eq!(git_in(&root, &["ls-files", "-u"]), "", "nothing is left unmerged");
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "D  a.txt", "the deletion is staged");
}

/// Nothing in the file is a change of Yours alone: the left side's toolbar button is disabled and does nothing.
/// The right side's applies Theirs' change, leaving the two conflicts, and nothing non-conflicting is left.
#[gpui_kit::test]
async fn the_toolbar_applies_the_non_conflicting_changes_of_one_side(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, three_changes);
    let view = open_merge(&workbench, cx);
    assert_eq!(status(&view, cx), "3 changes left, 2 conflicts");
    assert!(!view.read_with(cx, |view, _| view.enabled(MergeCommand::ApplyLeft)), "the left side's is disabled");
    press_at(cx, "merge-tool-applyLeft", gpui_kit::Modifiers::default());
    assert_eq!(status(&view, cx), "3 changes left, 2 conflicts", "a disabled button does nothing");
    press_at(cx, "merge-tool-applyRight", gpui_kit::Modifiers::default());
    assert_eq!(status(&view, cx), "2 changes left, 2 conflicts");
    assert!(result(&view, cx).contains("\nC\n"));
    assert!(!view.read_with(cx, |view, _| view.enabled(MergeCommand::ApplyAll)), "nothing non-conflicting is left");
}

/// A right click on `same` in the result offers Resolve Automatically disabled there; Ignore settles that
/// conflict with the base. A right click on `C` in Theirs and Resolve Using Theirs takes it.
#[gpui_kit::test]
async fn the_context_menus_decide_the_changes_under_the_caret(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, three_changes);
    let view = open_merge(&workbench, cx);
    right_click_line(cx, "merge-pane-result", 6);
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<MergeCommand>()).expect("the result's menu");
    let rows: Vec<(String, bool)> =
        menu.read_with(cx, |menu, _| menu.rows().into_iter().map(|row| (row.label.clone(), row.disabled)).collect());
    assert!(rows.contains(&("Resolve Automatically".to_owned(), true)), "{rows:?}");
    click_menu_row::<MergeCommand>(&workbench, 0, "Ignore", cx);
    assert_eq!(status(&view, cx), "2 changes left, 1 conflict");
    assert!(result(&view, cx).contains("\nsame\n"));

    right_click_line(cx, "merge-pane-right", 3);
    assert!(menu_level::<MergeCommand>(&workbench, 0, cx).contains(&"Resolve Using Theirs".to_owned()));
    click_menu_row::<MergeCommand>(&workbench, 0, "Resolve Using Theirs", cx);
    assert_eq!(status(&view, cx), "1 change left, 1 conflict");
    assert!(result(&view, cx).contains("\nC\n"));
}

/// With the result's keyboard, Find Action runs Resolve Simple Conflicts and closes, then Revert Conflict
/// Resolution, which brings the base back. With the keyboard outside the merge editor the command does not run.
#[gpui_kit::test]
async fn find_action_runs_merge_commands_with_the_editor_context_and_clears_it_on_external_focus(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, _root) = git_project(cx, three_changes);
    let view = open_merge(&workbench, cx);
    click_line(cx, "merge-pane-result", 0);
    let steps = [
        ("Resolve Simple Conflicts", "1 change left, 1 conflict"),
        ("Revert Conflict Resolution", "3 changes left, 2 conflicts"),
    ];
    for (title, after) in steps {
        cx.simulate_keystrokes(&primary("shift-a"));
        cx.run_until_parked();
        let everywhere = workbench.read_with(cx, |w, _| w.search_everywhere()).expect("Find Action opens");
        type_query(cx, title);
        settle(cx, |cx| everywhere.read_with(cx, |popup, _| popup.lines().first().map(String::as_str) == Some(title)));
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| status(&view, cx) == after);
        assert!(workbench.read_with(cx, |w, _| w.search_everywhere().is_none()), "the popup closed");
    }
    assert_eq!(result(&view, cx), BASE);

    // The keyboard outside the merge editor: the command is off and does not run.
    let tree = project_tree_of(&workbench, cx);
    cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(!try_action(cx, "merge.resolveSimple", None));
    assert_eq!(status(&view, cx), "3 changes left, 2 conflicts");
}
