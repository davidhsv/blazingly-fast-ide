use super::proofs_merge_fixtures::*;
use super::*;
use crate::vcs_diff::merge_gutters::{MergePane, MergeSide};

/// In the result, Select All, Copy, Cut and Paste run as actions: Copy and Cut are disabled and do nothing
/// without a selection; Select All selects the whole result, Copy puts it on the clipboard, Cut takes it out and
/// Paste puts the clipboard's text back at the caret.
#[gpui_kit::test]
async fn select_all_copy_cut_and_paste_edit_the_result(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    let len = LETTERS_BASE.len();
    assert!(!context_is_on(cx, "merge.copy"), "nothing is selected");
    assert!(!context_is_on(cx, "merge.cut"));
    assert!(context_is_on(cx, "merge.paste"));
    assert!(context_is_on(cx, "merge.selectAll"));
    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before\n".to_owned())));
    assert!(!try_action(cx, "merge.cut", None));
    assert!(!try_action(cx, "merge.copy", None));
    assert_eq!(result(&view, cx), LETTERS_BASE);
    assert_eq!(clipboard(cx).as_deref(), Some("before\n"));

    run_action(cx, "merge.selectAll", None);
    assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 0..len);
    assert!(context_is_on(cx, "merge.copy"));
    assert!(context_is_on(cx, "merge.cut"));
    run_action(cx, "merge.copy", None);
    assert_eq!(clipboard(cx).as_deref(), Some(LETTERS_BASE));
    assert_eq!(result(&view, cx), LETTERS_BASE, "Copy leaves the result");

    run_action(cx, "merge.cut", None);
    assert_eq!(result(&view, cx), "");
    assert_eq!(clipboard(cx).as_deref(), Some(LETTERS_BASE));
    assert!(!context_is_on(cx, "merge.cut"), "nothing is selected any more");

    cx.update(|_, cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("pasted\n".to_owned())));
    run_action(cx, "merge.paste", None);
    assert_eq!(result(&view, cx), "pasted\n");
    assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 7..7, "the caret follows the text");
}

/// In a side pane, Select All selects that version and Copy copies it; Cut and Paste are disabled there, as the
/// side panes are read-only, and do nothing.
#[gpui_kit::test]
async fn a_side_pane_copies_its_version_but_is_not_cut_or_pasted_into(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    let left = MergePane::Side(MergeSide::Left);
    click_line(cx, "merge-pane-left", 1);
    assert_eq!(view.read_with(cx, |view, _| view.active_pane()), left);
    assert!(!context_is_on(cx, "merge.copy"));
    assert!(!context_is_on(cx, "merge.paste"), "Yours is read-only");
    assert!(!try_action(cx, "merge.paste", None));

    run_action(cx, "merge.selectAll", None);
    assert_eq!(view.read_with(cx, |view, _| view.selection(left)), 0..LETTERS_OURS.len());
    assert!(context_is_on(cx, "merge.copy"));
    assert!(!context_is_on(cx, "merge.cut"), "Yours is read-only");
    assert!(!try_action(cx, "merge.cut", None));
    run_action(cx, "merge.copy", None);
    assert_eq!(clipboard(cx).as_deref(), Some(LETTERS_OURS));
    assert_eq!(result(&view, cx), LETTERS_BASE);
}
