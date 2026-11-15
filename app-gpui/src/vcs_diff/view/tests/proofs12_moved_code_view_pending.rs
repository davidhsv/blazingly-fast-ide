use super::*;

const FIRST: &str = "function first() {\n  return 'first';\n}\n";
const SECOND: &str = "function second() {\n  return 'second';\n}\n";
const THIRD: &str = "function third() {\n  return 'third';\n}\n";

/// The 1-based line of the caret, with its side.
fn caret_line(view: &Entity<DiffView>, cx: &mut VisualTestContext) -> Option<(Side, usize)> {
    view.read_with(cx, |view, _| view.caret.map(|(side, pos)| (side, view.docs[ix(side)].line_at(pos).number)))
}

/// The first function moved below the other two, its `'first'` edited to `'changed'`.
fn edited_move() -> (String, String) {
    (format!("{FIRST}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{}", FIRST.replace("'first'", "'changed'")))
}

// src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › labels edited moves and highlights
// only their internal word edits
/// A function moved with an edit is counted as a moved block; its link to the moved code says it was edited, the
/// edited words are marked on both sides, and the link puts the caret on the moved code's first line.
#[gpui_kit::test]
async fn labels_edited_moves_and_highlights_only_their_internal_word_edits(cx: &mut TestAppContext) {
    let (before, after) = edited_move();
    let (view, _, cx) = open(cx, &before, &after, split());
    assert!(counter(&view, cx).contains("1 moved block"), "{}", counter(&view, cx));
    assert!(cx.debug_bounds("diff-moved-to-0").is_some(), "a link goes to the moved code at line 7");
    assert!(cx.debug_bounds("diff-moved-edited-0").is_some(), "the link says the moved code was edited");
    assert!(cx.debug_bounds("diff-move-edit-left-0").is_some(), "\"first\" is marked on the left");
    assert!(cx.debug_bounds("diff-move-edit-right-0").is_some(), "\"changed\" is marked on the right");
    click(cx, "diff-moved-to-0");
    assert_eq!(caret_line(&view, cx), Some((Side::B, 7)));
}

// src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › omits inner edits in Highlight
// lines mode
/// Under Highlight lines the link of an edited move still says it was edited, and no word inside it is marked.
#[gpui_kit::test]
async fn omits_inner_edits_in_highlight_lines_mode(cx: &mut TestAppContext) {
    let (before, after) = edited_move();
    let options = DiffViewOptions { highlight: HighlightPolicy::Lines, ..split() };
    let (view, _, cx) = open(cx, &before, &after, options);
    assert!(cx.debug_bounds("diff-moved-edited-0").is_some(), "the link says the moved code was edited");
    assert!(cx.debug_bounds("diff-move-edit-left-0").is_none() && cx.debug_bounds("diff-move-edit-right-0").is_none());
    assert!(view.read_with(cx, |view, _| view.inner_highlights(Side::B).is_empty()));
}

// src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › shows native move links, follows
// them in both directions and updates after editing
/// A moved function is counted and marked as moved; its links go to the moved code and back to the original, and
/// once the right side no longer moves it the count, the marks and the links are gone.
#[gpui_kit::test]
async fn shows_native_move_links_follows_them_in_both_directions_and_updates_after_editing(cx: &mut TestAppContext) {
    let before = format!("{FIRST}{SECOND}{THIRD}");
    let after = format!("{SECOND}{THIRD}{FIRST}");
    let (view, _, cx) = open(cx, &before, &after, split());
    assert!(counter(&view, cx).contains("1 moved block"), "{}", counter(&view, cx));
    assert!(cx.debug_bounds("diff-moved-left-0").is_some(), "the moved lines are marked");
    click(cx, "diff-moved-to-0");
    assert_eq!(caret_line(&view, cx), Some((Side::B, 7)));
    click(cx, "diff-moved-from-0");
    assert_eq!(caret_line(&view, cx), Some((Side::A, 1)));

    view.update(cx, |view, cx| view.set_texts(&before, &before, cx));
    cx.run_until_parked();
    assert!(!counter(&view, cx).contains("moved block"));
    assert!(cx.debug_bounds("diff-moved-left-0").is_none());
    assert!(cx.debug_bounds("diff-moved-to-0").is_none());
}

// src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › respects Do not highlight
/// Under Do not highlight a moved function is still counted, but neither marked nor linked, and no line is
/// coloured.
#[gpui_kit::test]
async fn respects_do_not_highlight(cx: &mut TestAppContext) {
    let (before, after) = edited_move();
    let options = DiffViewOptions { highlight: HighlightPolicy::None, ..split() };
    let (view, _, cx) = open(cx, &before, &after, options);
    assert!(counter(&view, cx).contains("1 moved block"), "{}", counter(&view, cx));
    assert!(cx.debug_bounds("diff-moved-left-0").is_none() && cx.debug_bounds("diff-moved-right-0").is_none());
    assert!(cx.debug_bounds("diff-move-edit-left-0").is_none());
    assert!(cx.debug_bounds("diff-moved-to-0").is_none() && cx.debug_bounds("diff-moved-from-0").is_none());
    assert!(kinds(&view.read_with(cx, |view, _| view.right_lines())).iter().all(Option::is_none));
    assert!(view.read_with(cx, |view, _| view.inner_highlights(Side::A).is_empty()));
}
