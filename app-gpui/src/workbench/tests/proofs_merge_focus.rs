use super::proofs_merge_fixtures::*;
use super::*;
use crate::vcs_more::merge_actions::{MergeCommand, merge_actions};

/// Runs every merge command through the registry: none is enabled, so none runs.
fn assert_no_merge_command_runs(cx: &mut VisualTestContext) {
    for meta in merge_actions() {
        assert!(!context_is_on(cx, &meta.id), "{} is off", meta.id);
        assert!(!try_action(cx, &meta.id, None), "{} does nothing", meta.id);
    }
}

/// The merge commands are registered but off until a merge editor has the keyboard, and off again while the
/// editor behind it has the keyboard: `mergeEditorFocus` reads on and off with it, and none of the commands runs
/// while it is off. Once the merge editor has the keyboard back they are on again.
#[gpui_kit::test]
async fn keeps_merge_commands_registered_and_disabled_outside_a_merge_editor(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, letters);
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("a.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let registered = cx.update(|_, cx| {
        let registry = cx.global::<crate::platform::actions::ActionRegistry>();
        registry.ids().filter(|id| id.starts_with("merge.")).count()
    });
    assert_eq!(registered, 30);
    assert!(!context_is_on(cx, "mergeEditorFocus"));
    assert_no_merge_command_runs(cx);

    let view = open_merge(&workbench, cx);
    assert!(context_is_on(cx, "mergeEditorFocus"));
    assert!(context_is_on(cx, "diffEditorFocus"), "the merge editor counts as a diff viewer");
    let always = [
        "merge.applyAll",
        "merge.acceptYours",
        "merge.acceptTheirs",
        "merge.apply",
        "merge.cancel",
        "merge.reload",
        "merge.compare",
        "merge.settings",
        "merge.autoApply",
        "merge.compareYoursTheirs",
        "merge.selectAll",
        "merge.paste",
    ];
    for id in always {
        assert!(context_is_on(cx, id), "{id} is on while the merge editor has the keyboard");
    }

    // The keyboard goes to the file's editor behind the merge editor.
    cx.update(|window, cx| {
        let editor = workbench.read(cx).editor().map(|editor| editor.focus_handle(cx));
        window.focus(&editor.expect("the file's editor is open"), cx);
    });
    workbench.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(!cx.update(|window, cx| workbench.read(cx).merge_editor_focused(window, cx)));
    assert!(!context_is_on(cx, "mergeEditorFocus"));
    assert!(!context_is_on(cx, "diffEditorFocus"));
    assert_no_merge_command_runs(cx);
    assert_eq!(result(&view, cx), LETTERS_BASE, "nothing was applied");
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_some()), "nothing closed the merge editor");
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "nothing was asked");

    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    workbench.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    assert!(context_is_on(cx, "mergeEditorFocus"));
    assert!(context_is_on(cx, "merge.applyAll"));
    run_action(cx, "merge.applyAll", None);
    assert_eq!(result(&view, cx), "a\nb\nc\nD\ne\nf\nG\n");
}

/// With the merge editor's keyboard, the diff viewer's Diff Settings opens the merge settings, and Jump to
/// Source opens the conflicted file in its editor with the keyboard.
#[gpui_kit::test]
async fn diff_settings_and_jump_to_source_act_on_the_merge_editor_that_has_the_keyboard(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, letters);
    let _merge = open_merge(&workbench, cx);

    run_action(cx, "diff.showSettings", None);
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<MergeCommand>()).expect("the merge settings are open");
    let labels: Vec<String> =
        menu.read_with(cx, |menu, _| menu.rows().into_iter().map(|row| row.label.clone()).collect());
    assert_eq!(labels, ["Automatically Apply Non-Conflicting Changes"]);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.popup.is_none()), "Escape closes the menu");

    run_action(cx, "diff.jumpToSource", None);
    wait_long(cx, "the file's editor", |cx| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == root.join("a.txt")))
    });
    let focused = cx.update(|window, cx| {
        workbench.read(cx).editor().is_some_and(|editor| editor.focus_handle(cx).is_focused(window))
    });
    assert!(focused, "the file's editor has the keyboard");
}

/// Jump to Source after the result was changed asks as Cancel does, so merge work is never dropped
/// silently: No keeps the merge editor with its work and the keyboard; Exit closes it, leaves the file
/// unmerged and opens it in its editor.
#[gpui_kit::test]
async fn jump_to_source_from_a_changed_merge_asks_first_and_no_keeps_the_work(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = git_project(cx, letters);
    let view = open_merge(&workbench, cx);
    run_action(cx, "merge.applyAll", None);
    let applied = result(&view, cx);
    assert_eq!(applied, "a\nb\nc\nD\ne\nf\nG\n");
    assert!(view.read_with(cx, |view, _| view.is_modified()));
    let file_open = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == root.join("a.txt")))
    };

    run_action(cx, "diff.jumpToSource", None);
    let question = wait_for_prompt(&workbench, cx);
    assert_eq!(question.read_with(cx, |dialog, _| dialog.title().to_owned()), "Cancel Merge");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_some()), "No keeps the merge editor");
    assert_eq!(result(&view, cx), applied, "and its work");
    assert!(!file_open(cx), "nothing is opened");
    assert!(cx.update(|window, cx| workbench.read(cx).merge_editor_focused(window, cx)));

    run_action(cx, "diff.jumpToSource", None);
    wait_for_prompt(&workbench, cx);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the file's editor", file_open);
    assert!(workbench.read_with(cx, |w, _| w.merge_view().is_none()), "Exit closes the merge editor");
    assert_eq!(git_in(&root, &["ls-files", "-u"]).lines().count(), 3, "the file is still unmerged");
}
