//! Split and Move Right, and closing a shell that runs a command in the foreground.

use super::*;

/// Split and Move Right does nothing to a group's only tab; with a second tab in the group it moves
/// the active tab into a new group drawn to the right of the first, on the same row, without
/// starting a shell or closing a tab.
#[gpui_kit::test]
async fn split_and_move_right_moves_the_tab_into_a_new_group_beside_its_own(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
    plain_shells(home.path(), cx);
    run_action(cx, "terminal.new", None);
    let panel = panel_of(&workbench, cx);
    let alone = groups(&panel, cx);
    assert_eq!(alone.len(), 1);
    run_action(cx, "terminal.splitAndMoveRight", None);
    assert_eq!(groups(&panel, cx), alone, "the group's only tab stays");

    run_action(cx, "terminal.new", None);
    let moved = panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned)).expect("a tab");
    assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 2);
    run_action(cx, "terminal.splitAndMoveRight", None);
    let split = groups(&panel, cx);
    assert_eq!(split.len(), 2, "{split:?}");
    assert_eq!(split[0], alone[0], "the first group stays where it was");
    let group_of = panel.read_with(cx, |panel, _| panel.terminals().tab(&moved).map(|tab| tab.group.clone()));
    assert_eq!(group_of.as_deref(), Some(split[1].as_str()), "the active tab is in the new group");
    let (left, right) = (pane_bounds(&split[0], cx), pane_bounds(&split[1], cx));
    assert!(right.origin.x > left.origin.x && right.origin.y == left.origin.y, "{left:?} {right:?}");
    assert_eq!(panel.read_with(cx, |panel, _| panel.started()), 2, "no shell is started");
    assert_eq!(titles(&panel, cx).len(), 2, "no tab is closed");
    close_project(cx);
}

/// Close Terminal Session on a shell running a command in the foreground asks the backend, which
/// says the shell is busy: the question names the tab, Cancel keeps it and Terminate closes it.
#[gpui_kit::test]
async fn close_session_asks_before_ending_a_shell_whose_foreground_runs_a_command(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
    plain_shells(home.path(), cx);
    run_action(cx, "terminal.new", None);
    let panel = panel_of(&workbench, cx);
    let shell = active_view(&panel, cx);
    wait_long(cx, "the shell starts", |cx| shell.read_with(cx, |view, _| view.id().is_some()));
    let id = shell.read_with(cx, |view, _| view.id()).expect("the shell runs");
    let busy = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| ide_host::ops::term::is_busy(&cx.global::<Backend>().host, id)).unwrap_or(false)
    };
    assert!(!busy(cx), "a shell waiting for input is idle");
    // Job control gives the command the terminal, as an interactive shell does.
    type_line("set -m", cx);
    type_line("echo started-$((2*4)); sleep 30", cx);
    wait_long(cx, "the command runs", |cx| screen(&shell, cx).contains("started-8") && busy(cx));

    run_action(cx, "terminal.closeSession", None);
    wait_long(cx, "the question", |cx| prompt_shows(&workbench, cx));
    let dialog = workbench.read_with(cx, |w, _| w.vcs_dialog_view::<PromptDialog>()).expect("the question");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Process 'Local' Is Running");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the question to close", |cx| !prompt_shows(&workbench, cx));
    assert_eq!(titles(&panel, cx).len(), 1, "Cancel keeps the tab");
    assert!(busy(cx), "the command still runs");

    cx.update(|window, cx| {
        let focus = shell.focus_handle(cx);
        window.focus(&focus, cx);
    });
    run_action(cx, "terminal.closeSession", None);
    wait_long(cx, "the question again", |cx| prompt_shows(&workbench, cx));
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the tab to close", |cx| titles(&panel, cx).is_empty());
    close_project(cx);
}
