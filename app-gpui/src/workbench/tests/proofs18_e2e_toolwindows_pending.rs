//! Not declared: the flows below need steps the application does not have, or selectors its views do not draw.
//!
//! - Maximized Terminal: the header's Options menu (`more:<id>`, with Maximize / Restore Size) and Hide button
//!   (`hide:<id>`) exist; a double click on the header's title toggling the maximized state does not (no
//!   double-click handler on the tool window header in `workbench/mod.rs`, selector `tool-window-title:<id>`
//!   assumed). Window resizing (`setViewportSize`) is not reproduced.

use super::proofs18_e2e_fixtures::{click_at, click_menu_row, double_click_at};
use super::*;

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
