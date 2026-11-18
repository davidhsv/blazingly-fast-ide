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

/// Split and Move Down with a second tab in the group moves the active tab into a new group drawn below, under a
/// header of its own: it holds that tab alone, selected, without the window's title or its Options button, which
/// stay in the upper group's header with the tab left behind.
#[gpui_kit::test]
async fn split_and_move_down_gives_the_moved_tab_a_group_below_with_its_own_header(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "")]);
    plain_shells(home.path(), cx);
    run_action(cx, "terminal.new", None);
    let panel = panel_of(&workbench, cx);
    run_action(cx, "terminal.new", None);
    let moved = panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned)).expect("a tab");
    let names = titles(&panel, cx);
    assert_eq!(names.len(), 2, "{names:?}");
    let (stays, goes) = (names[0].0.clone(), names[1].0.clone());

    run_action(cx, "terminal.splitAndMoveDown", None);
    let headers = panel.read_with(cx, |panel, _| panel.headers());
    assert_eq!(headers.len(), 2, "{headers:?}");
    let (upper, lower) = (&headers[0], &headers[1]);
    assert_eq!(upper.tabs, [(stays, true)], "{headers:?}");
    assert_eq!(lower.tabs, [(goes, true)], "{headers:?}");
    assert!(upper.title && upper.actions, "{headers:?}");
    assert!(!lower.title && !lower.actions, "{headers:?}");
    let bounds = |selector: String, cx: &mut VisualTestContext| {
        cx.run_until_parked();
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    };
    let upper_header = bounds(format!("terminal-header:{}", upper.group), cx);
    let lower_header = bounds(format!("terminal-header:{}", lower.group), cx);
    assert!(lower_header.origin.y > upper_header.origin.y, "{upper_header:?} {lower_header:?}");
    assert_eq!(lower_header.origin.x, upper_header.origin.x, "{upper_header:?} {lower_header:?}");
    let tab = bounds(format!("terminal-tab:{moved}"), cx).center();
    assert!(lower_header.contains(&tab) && !upper_header.contains(&tab), "the moved tab is in the lower header");
    let options = bounds("more:terminal".to_owned(), cx).center();
    assert!(upper_header.contains(&options) && !lower_header.contains(&options), "Options stays above");
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
