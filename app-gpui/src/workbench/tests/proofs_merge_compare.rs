use super::proofs_merge_fixtures::*;
use super::*;
use crate::vcs_diff::diff_input::DiffSide;
use crate::vcs_more::merge_actions::MergeCommand;

/// The rows of the open menu of merge commands: label, disabled, checked.
fn merge_menu_rows(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool, bool)> {
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<MergeCommand>()).expect("a merge menu is open");
    menu.read_with(cx, |menu, _| {
        menu.rows().into_iter().map(|row| (row.label.clone(), row.disabled, row.checked)).collect()
    })
}

/// Each comparison of Compare Contents, run as an action, shows its two versions in the diff window with their
/// titles, Yours or Theirs or the base on the left; Escape closes it and gives the keyboard back to the merge
/// editor.
#[gpui_kit::test]
async fn each_comparison_shows_its_two_versions_in_the_diff_window(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    run_action(cx, "merge.applyAll", None);
    let merged = "a\nb\nc\nD\ne\nf\nG\n";
    assert_eq!(result(&view, cx), merged);
    let titles = view.read_with(cx, |view, _| view.titles().cloned()).expect("the panes are titled");
    let yours = DiffSide::Text { text: LETTERS_OURS.to_owned(), label: titles.yours };
    let theirs = DiffSide::Text { text: LETTERS_THEIRS.to_owned(), label: titles.theirs };
    let base = DiffSide::Text { text: LETTERS_BASE.to_owned(), label: "Base".to_owned() };
    let result_side = DiffSide::Text { text: merged.to_owned(), label: "Result".to_owned() };
    let comparisons = [
        ("merge.compareYoursResult", &yours, &result_side),
        ("merge.compareResultTheirs", &result_side, &theirs),
        ("merge.compareYoursTheirs", &yours, &theirs),
        ("merge.compareBaseYours", &base, &yours),
        ("merge.compareBaseResult", &base, &result_side),
        ("merge.compareBaseTheirs", &base, &theirs),
    ];
    for (id, left, right) in comparisons {
        assert!(context_is_on(cx, id), "{id}");
        run_action(cx, id, None);
        let data = workbench.read_with(cx, |w, cx| w.diff_dialog().map(|dialog| dialog.read(cx).data().clone()));
        let data = data.unwrap_or_else(|| panic!("{id} shows the diff window"));
        assert_eq!((&data.left, &data.right), (left, right), "{id}");
        assert!(data.path.ends_with("a.txt"), "{id}: {}", data.path);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()), "{id}: Escape closes the comparison");
        assert!(context_is_on(cx, "mergeEditorFocus"), "{id}: the merge editor has the keyboard again");
    }
}

/// Merge Settings shows Automatically Apply Non-Conflicting Changes, unchecked. Switched on as an action it is
/// stored, checked in the menu, and applies the non-conflicting changes at once; the next merge editor starts
/// with them applied, as its starting point and not a change. Switched off it is stored off and unchecked.
#[gpui_kit::test]
async fn automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    let title = "Automatically Apply Non-Conflicting Changes".to_owned();
    let stored = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| cx.global::<UiStorage>().get(crate::workbench::conflicts::AUTO_APPLY_KEY).map(str::to_owned))
    };
    let toggled = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            let action = cx.global::<crate::platform::actions::ActionRegistry>().get("merge.autoApply");
            action.and_then(|action| crate::platform::actions::action_toggled(&action, window, cx))
        })
    };
    run_action(cx, "merge.settings", None);
    assert_eq!(merge_menu_rows(&workbench, cx), [(title.clone(), false, false)]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(toggled(cx), Some(false));

    run_action(cx, "merge.autoApply", None);
    assert_eq!(stored(cx).as_deref(), Some("true"));
    assert_eq!(toggled(cx), Some(true));
    assert_eq!(result(&view, cx), "a\nb\nc\nD\ne\nf\nG\n", "the non-conflicting changes are applied at once");
    assert_eq!(status(&view, cx), "1 change left, 1 conflict");
    run_action(cx, "merge.settings", None);
    assert_eq!(merge_menu_rows(&workbench, cx), [(title.clone(), false, true)]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // Applying them changed the result: Cancel asks.
    run_action(cx, "merge.cancel", None);
    wait_for_prompt(&workbench, cx);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
    assert_eq!(git_in(&root, &["ls-files", "-u"]).lines().count(), 3, "the file is still unmerged");

    run_action(cx, "vcs.resolveConflicts", None);
    let view = open_merge_from_dialog(&workbench, cx);
    assert_eq!(result(&view, cx), "a\nb\nc\nD\ne\nf\nG\n");
    assert!(!view.read_with(cx, |view, _| view.is_modified()), "the starting point is not a change");

    run_action(cx, "merge.autoApply", None);
    assert_eq!(stored(cx).as_deref(), Some("false"));
    assert_eq!(toggled(cx), Some(false));
    run_action(cx, "merge.settings", None);
    assert_eq!(merge_menu_rows(&workbench, cx), [(title, false, false)]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    // Nothing was decided in this editor: Cancel closes it at once.
    run_action(cx, "merge.cancel", None);
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "nothing is asked");
    wait_long(cx, "the merge editor closes", |cx| workbench.read_with(cx, |w, _| w.merge_view().is_none()));
}

/// Merge… in the Conflicts dialog already open: the merge editor once it shows the versions.
fn open_merge_from_dialog(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<crate::vcs::merge_view::MergeView> {
    wait_long(cx, "the Conflicts dialog", |cx| workbench.read_with(cx, |w, _| w.conflicts_dialog().is_some()));
    let dialog = workbench.read_with(cx, |w, _| w.conflicts_dialog()).expect("the dialog is open");
    dialog.update(cx, |dialog, cx| dialog.press(crate::vcs::conflicts_dialog::MERGE, cx));
    merge_view_loaded(workbench, cx)
}

/// Reload from Disk, run as an action, reads the file's versions again: a version changed since shows, and what
/// was decided goes, the result starting again from the base.
#[gpui_kit::test]
async fn reload_from_disk_reads_the_versions_again(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    run_action(cx, "merge.applyAll", None);
    assert!(view.read_with(cx, |view, _| view.is_modified()));
    assert_eq!(view.read_with(cx, |view, _| view.changes()[0].right.clone()), "R\n");

    set_stage(&root, 3, "a.txt", "a\nR2\nc\nD\ne\nf\nG\n");
    assert!(context_is_on(cx, "merge.reload"));
    run_action(cx, "merge.reload", None);
    wait_long(cx, "the versions are read again", |cx| {
        view.read_with(cx, |view, _| view.changes().first().is_some_and(|change| change.right == "R2\n"))
    });
    assert_eq!(result(&view, cx), LETTERS_BASE);
    assert_eq!(status(&view, cx), "3 changes left, 1 conflict");
    assert!(!view.read_with(cx, |view, _| view.is_modified()));
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_some_and(|shown| shown == view)), "the same editor");
}
