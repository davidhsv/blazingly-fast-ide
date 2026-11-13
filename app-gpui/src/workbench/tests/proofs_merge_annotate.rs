use super::proofs_merge_fixtures::*;
use super::*;

const LEFT: &str = "merge-pane-left";
const RIGHT: &str = "merge-pane-right";

/// Annotate with Git Blame is enabled while the caret is in a side pane, and disabled in the result, where it
/// does nothing.
#[gpui_kit::test]
async fn annotate_with_git_blame_is_offered_in_the_side_panes_only(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    assert!(!context_is_on(cx, "merge.annotate"), "the caret is in the result");
    assert!(!try_action(cx, "merge.annotate", None));
    click_line(cx, LEFT, 1);
    assert!(context_is_on(cx, "merge.annotate"), "Yours' pane can be annotated");
    click_line(cx, RIGHT, 1);
    assert!(context_is_on(cx, "merge.annotate"), "Theirs' pane can be annotated");
    click_line(cx, "merge-pane-result", 1);
    assert!(!context_is_on(cx, "merge.annotate"));
    assert_eq!(result(&view, cx), LETTERS_BASE);
}
