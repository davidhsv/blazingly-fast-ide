use super::proofs_merge_fixtures::*;
use super::*;
use crate::vcs_diff::merge_gutters::MergePane;

/// Apply Non-Conflicting Changes from the Left Side, run as an action, takes Yours' change alone and leaves
/// Theirs' and the conflict, with the caret on the first change left; it is then disabled and does nothing.
/// From the Right Side it takes Theirs' change, and the caret goes to the conflict on line 5.
#[gpui_kit::test]
async fn applies_those_of_the_chosen_side_only(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, numbers);
    let view = open_merge(&workbench, cx);
    assert_eq!(status(&view, cx), "3 changes left, 1 conflict");
    assert!(context_is_on(cx, "merge.applyLeft"));
    assert!(context_is_on(cx, "merge.applyRight"));

    run_action(cx, "merge.applyLeft", None);
    assert_eq!(result(&view, cx), "1\n2\n3\n4\n5\n6\n7\n8\nnine\n");
    assert_eq!(status(&view, cx), "2 changes left, 1 conflict");
    assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), 0..0, "on Theirs' change, line 1");
    assert!(!context_is_on(cx, "merge.applyLeft"), "Yours has nothing non-conflicting left");
    assert!(!try_action(cx, "merge.applyLeft", None));
    assert_eq!(result(&view, cx), "1\n2\n3\n4\n5\n6\n7\n8\nnine\n");
    assert!(context_is_on(cx, "merge.applyRight"));

    run_action(cx, "merge.applyRight", None);
    assert_eq!(result(&view, cx), "one\n2\n3\n4\n5\n6\n7\n8\nnine\n");
    assert_eq!(status(&view, cx), "1 change left, 1 conflict");
    let conflict = view.read_with(cx, |view, _| view.changes()[1].from);
    assert_eq!(conflict, 10, "the conflict starts line 5");
    assert_eq!(view.read_with(cx, |view, _| view.selection(MergePane::Result)), conflict..conflict);
    assert!(!context_is_on(cx, "merge.applyRight"));
    assert!(!context_is_on(cx, "merge.applyAll"), "only the conflict is left");
    assert!(!try_action(cx, "merge.applyRight", None));
}

/// Resolve Simple Conflicts, run as an action, takes every change that needs no decision and leaves the conflict
/// of the same word; Revert Conflict Resolution then sends the settled changes back to the base, and with nothing
/// settled it is disabled and does nothing.
#[gpui_kit::test]
async fn resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, numbers);
    let view = open_merge(&workbench, cx);
    assert!(context_is_on(cx, "merge.resolveSimple"));
    assert!(!context_is_on(cx, "merge.revert"), "nothing is settled yet");
    assert!(!try_action(cx, "merge.revert", None));

    run_action(cx, "merge.resolveSimple", None);
    assert_eq!(result(&view, cx), "one\n2\n3\n4\n5\n6\n7\n8\nnine\n");
    assert_eq!(status(&view, cx), "1 change left, 1 conflict");
    assert!(!context_is_on(cx, "merge.resolveSimple"), "the conflict of the same word needs a decision");
    assert!(!try_action(cx, "merge.resolveSimple", None));
    assert!(context_is_on(cx, "merge.revert"));

    // sends the settled changes back to the base and leaves the pending ones
    run_action(cx, "merge.revert", None);
    assert_eq!(result(&view, cx), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    assert_eq!(status(&view, cx), "3 changes left, 1 conflict");
    assert!(!context_is_on(cx, "merge.revert"));
    assert!(!try_action(cx, "merge.revert", None));
    assert_eq!(result(&view, cx), "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
}

/// Resolve Simple Conflicts settles a conflict whose sides changed different words with both words; Apply Merge
/// Result then writes it without asking, stages it, closes the merge editor, and the repository has nothing
/// unmerged left.
#[gpui_kit::test]
async fn resolve_simple_conflicts_then_apply_writes_both_sides_words_and_stages_the_file(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, words);
    let view = open_merge(&workbench, cx);
    run_action(cx, "merge.resolveSimple", None);
    assert_eq!(result(&view, cx), "let x = 1 + 2;\n");
    assert_eq!(status(&view, cx), "All changes processed");

    run_action(cx, "merge.apply", None);
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "nothing unresolved is asked about");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).expect("the file is read"), "let x = 1 + 2;\n");
    assert_eq!(git_in(&root, &["ls-files", "-u"]), "", "nothing is left unmerged");
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "M  a.txt");
    wait_for_notice(&workbench, "a.txt: conflicts resolved", cx);
}

/// Apply Merge Result with a change unresolved asks first: No keeps the merge editor and the file unmerged; Mark
/// Resolved writes the result, which keeps the base there, and stages it. Once the editor is closed, the merge
/// commands and `mergeEditorFocus` are off and Apply does nothing.
#[gpui_kit::test]
async fn apply_merge_result_asks_about_unresolved_changes_and_writes_the_result(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, conflicting_branches);
    let view = open_merge(&workbench, cx);
    assert_eq!(status(&view, cx), "1 change left, 1 conflict");
    assert!(context_is_on(cx, "merge.apply"));

    run_action(cx, "merge.apply", None);
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Unresolved Changes");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_some()), "No keeps the merge editor");
    assert_eq!(git_in(&root, &["ls-files", "-u"]).lines().count(), 3, "the file is still unmerged");

    run_action(cx, "merge.apply", None);
    wait_for_prompt(&workbench, cx);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).expect("the file is read"), "one\n");
    assert_eq!(git_in(&root, &["ls-files", "-u"]), "");
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "M  a.txt");
    wait_for_notice(&workbench, "a.txt: conflicts resolved", cx);

    assert!(!context_is_on(cx, "mergeEditorFocus"));
    assert!(!context_is_on(cx, "merge.apply"));
    assert!(!try_action(cx, "merge.apply", None));
}

/// Accept Theirs, run as an action after the result was changed, asks first: No keeps the merge editor and its
/// result; Discard Changes writes Theirs as it is, stages it and closes the editor.
#[gpui_kit::test]
async fn accept_theirs_asks_once_the_result_was_changed_and_writes_their_version(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, conflicting_branches);
    let view = open_merge(&workbench, cx);
    assert!(context_is_on(cx, "merge.acceptTheirs"));
    // The caret is on the conflict: resolving it with Yours changes the result.
    run_action(cx, "merge.resolveUsingYours", None);
    assert_eq!(result(&view, cx), "main\n");

    run_action(cx, "merge.acceptTheirs", None);
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Accept Theirs");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_some()), "No keeps the merge editor");
    assert_eq!(result(&view, cx), "main\n");

    run_action(cx, "merge.acceptTheirs", None);
    wait_for_prompt(&workbench, cx);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(std::fs::read_to_string(root.join("a.txt")).expect("the file is read"), "feature\n");
    assert_eq!(git_in(&root, &["ls-files", "-u"]), "");
    assert_eq!(git_in(&root, &["status", "--porcelain"]), "M  a.txt");
    assert!(root.join(".git/MERGE_HEAD").exists(), "the merge is still to be committed");
    assert!(!try_action(cx, "merge.acceptTheirs", None));
}
