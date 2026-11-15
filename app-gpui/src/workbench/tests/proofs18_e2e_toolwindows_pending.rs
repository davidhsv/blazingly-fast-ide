//! Not declared: the flows below need steps the application does not have, or selectors its views do not draw.
//!
//! - Maximized Terminal: the header's Options menu (`more:<id>`, with Maximize / Restore Size) and Hide button
//!   (`hide:<id>`) exist; a double click on the header's title toggling the maximized state does not (no
//!   double-click handler on the tool window header in `workbench/mod.rs`, selector `tool-window-title:<id>`
//!   assumed). Window resizing (`setViewportSize`) is not reproduced.
//! - Split Right: each tab group of the Terminal has no header of its own (hypothesis from reading
//!   `terminal/panel.rs`: one header is drawn for the window; check with `grep -n "header" terminal/panel.rs`);
//!   its tabs (`terminal-tab:<id>`) are element ids without a `debug_selector`.
//! - Closing the last tab of a group: the tab's Close Tab button (`terminal-close:<id>`) is an element id without
//!   a `debug_selector`.

use super::proofs18_e2e_fixtures::{click_at, click_menu_row, double_click_at, right_click_at};
use super::*;
use crate::terminal::panel::TerminalPanel;

fn maximized_terminal_fills_the_workbench_and_restores_the_layout(cx: &mut TestAppContext, anchor: Anchor) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("README.md"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    workbench.update_in(cx, |w, window, cx| {
        w.with_layout(window, cx, |layout| {
            layout.move_window(TERMINAL_TOOL_WINDOW, Slot::new(anchor, false), None);
            layout.set_size(anchor, 650.0);
            layout.show(TERMINAL_TOOL_WINDOW, true)
        })
    });
    let side =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.tool_windows().and_then(|l| l.maximized()));
    cx.simulate_keystrokes("cmd-shift-'");
    cx.run_until_parked();
    assert_eq!(side(cx), Some(anchor));
    click_at(cx, "more:terminal".to_owned());
    click_menu_row::<ToolWindowCommand>(&workbench, 0, "Restore Size", cx);
    assert_eq!(side(cx), None);
    click_at(cx, "more:terminal".to_owned());
    click_menu_row::<ToolWindowCommand>(&workbench, 0, "Maximize", cx);
    assert_eq!(side(cx), Some(anchor));
    // The missing step: a double click on the header's title restores, a second one maximizes again.
    double_click_at(cx, "tool-window-title:terminal".to_owned());
    assert_eq!(side(cx), None);
    double_click_at(cx, "tool-window-title:terminal".to_owned());
    assert_eq!(side(cx), Some(anchor));
    cx.simulate_keystrokes("cmd-shift-'");
    cx.run_until_parked();
    assert_eq!(side(cx), None);
    cx.simulate_keystrokes("cmd-shift-'");
    cx.run_until_parked();
    click_at(cx, "hide:terminal".to_owned());
    assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
    assert!(context_is_on(cx, "editorFocus"), "the editor has the keyboard again");
}

#[gpui_kit::test]
async fn maximized_left_terminal_fills_the_workbench_and_restores_the_layout(cx: &mut TestAppContext) {
    maximized_terminal_fills_the_workbench_and_restores_the_layout(cx, Anchor::Left);
}

#[gpui_kit::test]
async fn maximized_right_terminal_fills_the_workbench_and_restores_the_layout(cx: &mut TestAppContext) {
    maximized_terminal_fills_the_workbench_and_restores_the_layout(cx, Anchor::Right);
}

#[gpui_kit::test]
async fn maximized_bottom_terminal_fills_the_workbench_and_restores_the_layout(cx: &mut TestAppContext) {
    maximized_terminal_fills_the_workbench_and_restores_the_layout(cx, Anchor::Bottom);
}

/// Split Right from the tab's menu gives the new group `Local (2)` its own header, the window's buttons on the
/// last one; Unsplit merges the tabs back under one header.
#[gpui_kit::test]
async fn split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes("alt-f12");
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())
        .expect("the Terminal window holds its tabs");
    let first = panel.read_with(cx, |p, _| p.terminals().active_tab().map(str::to_owned)).expect("a tab");
    right_click_at(cx, format!("terminal-tab:{first}"));
    click_menu_row::<crate::terminal::panel::TabCommand>(&workbench, 0, "Split Right", cx);
    assert_eq!(panel.read_with(cx, |p, _| p.panes().len()), 2);
    assert_eq!(cx.debug_bounds("terminal-header:1").is_some(), true, "the new group has its own header");
}

/// Close Tab of the second group's only tab removes the group and gives the first group's terminal the
/// keyboard; ⌘T then opens a second tab.
#[gpui_kit::test]
async fn closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "# acme\n")]);
    cx.update(|window, _| window.activate_window());
    cx.simulate_keystrokes("alt-f12");
    run_action(cx, "terminal.splitRight", None);
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
        .and_then(|view| view.downcast::<TerminalPanel>().ok())
        .expect("the Terminal window holds its tabs");
    let second = panel.read_with(cx, |p, _| p.terminals().active_tab().map(str::to_owned)).expect("a tab");
    click_at(cx, format!("terminal-close:{second}"));
    assert_eq!(panel.read_with(cx, |p, _| p.panes().len()), 1);
    let left = panel.read_with(cx, |p, _| p.active_view().cloned()).expect("the neighbour");
    assert!(cx.update(|window, cx| left.focus_handle(cx).is_focused(window)));
    cx.simulate_keystrokes("cmd-t");
    cx.run_until_parked();
    assert_eq!(panel.read_with(cx, |p, _| p.tab_titles().len()), 2);
}
