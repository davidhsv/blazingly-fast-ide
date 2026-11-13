use super::proofs_merge_fixtures::*;
use super::*;

const LEFT: &str = "merge-pane-left";
const RIGHT: &str = "merge-pane-right";

/// Ignore, run as an action with the caret on Theirs' side of the conflict, settles that side and leaves the
/// result's text; Accept, with the caret on Yours' side, then takes Yours' text and settles the conflict. On an
/// unchanged line of a side, and in the result, Accept is disabled and does nothing.
#[gpui_kit::test]
async fn settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    assert!(!context_is_on(cx, "merge.accept"), "the caret is in the result");
    assert!(!try_action(cx, "merge.accept", None));

    click_line(cx, RIGHT, 1);
    assert!(context_is_on(cx, "merge.ignore"));
    run_action(cx, "merge.ignore", None);
    assert_eq!(result(&view, cx), LETTERS_BASE, "ignoring keeps the result's text");
    assert_eq!(status(&view, cx), "3 changes left, 1 conflict", "the conflict waits for Yours");
    assert!(!context_is_on(cx, "merge.ignore"), "Theirs' side of the conflict is settled");
    assert!(!try_action(cx, "merge.ignore", None));

    click_line(cx, LEFT, 0);
    assert!(!context_is_on(cx, "merge.accept"), "the first line is unchanged");
    assert!(!try_action(cx, "merge.accept", None));
    click_line(cx, LEFT, 1);
    assert!(context_is_on(cx, "merge.accept"));
    run_action(cx, "merge.accept", None);
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "2 changes left");
    assert!(view.read_with(cx, |view, _| view.changes()[0].is_resolved()));
    assert!(!context_is_on(cx, "merge.accept"));

    // A change of Theirs only, from its line in Theirs.
    click_line(cx, RIGHT, 3);
    run_action(cx, "merge.accept", None);
    assert_eq!(result(&view, cx), "a\nL\nc\nD\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "1 change left");
}

/// Accept on Yours' side of a conflict takes its text and leaves the conflict open for Theirs.
#[gpui_kit::test]
async fn takes_the_text_and_leaves_a_conflict_open_for_the_other_side(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    click_line(cx, LEFT, 1);
    run_action(cx, "merge.accept", None);
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "3 changes left, 1 conflict");
    click_line(cx, RIGHT, 1);
    assert!(context_is_on(cx, "merge.accept"), "Theirs' side of the conflict is still open");
    run_action(cx, "merge.accept", None);
    assert_eq!(result(&view, cx), "a\nL\nR\nc\nd\ne\nf\ng\n", "Theirs follows Yours");
    assert_eq!(status(&view, cx), "2 changes left");
}

/// Resolve Using Yours, run as an action from Yours' side of the conflict, takes Yours and settles the conflict
/// at once; both resolutions are then disabled there. After Revert, Resolve Using Theirs from Theirs' side
/// settles it with Theirs.
#[gpui_kit::test]
async fn settles_the_change_at_once_when_resolving_using_a_side(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    click_line(cx, LEFT, 1);
    assert!(context_is_on(cx, "merge.resolveUsingYours"));
    run_action(cx, "merge.resolveUsingYours", None);
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "2 changes left");
    assert!(!context_is_on(cx, "merge.resolveUsingYours"));
    assert!(!context_is_on(cx, "merge.resolveUsingTheirs"));
    assert!(!try_action(cx, "merge.resolveUsingTheirs", None));
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n");

    run_action(cx, "merge.revert", None);
    assert_eq!(result(&view, cx), LETTERS_BASE);
    click_line(cx, RIGHT, 1);
    assert!(context_is_on(cx, "merge.resolveUsingTheirs"));
    run_action(cx, "merge.resolveUsingTheirs", None);
    assert_eq!(result(&view, cx), "a\nR\nc\nd\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "2 changes left");
}

/// Resolve Automatically, run as an action with the caret on the conflict in the result, merges the words both
/// sides changed, and is then disabled. After Revert, Ignore in the result settles the conflict with the base,
/// which Apply Merge Result writes and stages.
#[gpui_kit::test]
async fn merges_a_conflict_whose_sides_changed_different_words(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, words);
    let view = open_merge(&workbench, cx);
    assert!(context_is_on(cx, "merge.resolveAutomatically"));
    run_action(cx, "merge.resolveAutomatically", None);
    assert_eq!(result(&view, cx), "let x = 1 + 2;\n");
    assert_eq!(status(&view, cx), "All changes processed");
    assert!(!context_is_on(cx, "merge.resolveAutomatically"));
    assert!(!try_action(cx, "merge.resolveAutomatically", None));

    // settles a conflict with the base when ignoring both sides
    run_action(cx, "merge.revert", None);
    assert_eq!(result(&view, cx), WORDS_BASE);
    assert!(context_is_on(cx, "merge.ignore"));
    run_action(cx, "merge.ignore", None);
    assert_eq!(result(&view, cx), WORDS_BASE);
    assert_eq!(status(&view, cx), "All changes processed");
    assert!(!context_is_on(cx, "merge.ignore"));
    assert!(!context_is_on(cx, "merge.resolveAutomatically"), "a settled conflict is not merged again");

    run_action(cx, "merge.apply", None);
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).expect("the file is read"), WORDS_BASE);
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "M  a.txt");
}

/// Resolve Merge Change, run as an action with the change and what to do with it, does that to the change: an
/// arrow's Accept resolving with Yours, an Ignore of both sides, a wand's automatic resolution. Run again on a
/// settled change it changes nothing.
#[gpui_kit::test]
async fn resolve_merge_change_does_what_its_argument_says_to_the_change(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    assert!(context_is_on(cx, "merge.change"));
    let accept = serde_json::json!({ "indexes": [0], "action": { "type": "accept", "side": "left", "resolve": true } });
    run_action(cx, "merge.change", Some(accept.clone()));
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n");
    assert_eq!(status(&view, cx), "2 changes left");

    let ignore = serde_json::json!({ "indexes": [1], "action": { "type": "ignore", "side": null } });
    run_action(cx, "merge.change", Some(ignore));
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\ng\n", "the base stays");
    assert_eq!(status(&view, cx), "1 change left");

    let auto = serde_json::json!({ "indexes": [2], "action": { "type": "auto", "side": "base" } });
    run_action(cx, "merge.change", Some(auto));
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\nG\n");
    assert_eq!(status(&view, cx), "All changes processed");

    run_action(cx, "merge.change", Some(accept));
    assert_eq!(result(&view, cx), "a\nL\nc\nd\ne\nf\nG\n", "a settled change takes nothing more");
}

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
