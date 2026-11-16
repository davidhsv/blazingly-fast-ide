//! The tool window's groups as they are drawn and used: a header with a tab strip per group, side by
//! side or stacked, the line between two groups dragged, files dropped on one group's terminal, and
//! ⌘W in a terminal.

use super::*;
use crate::terminal::panel::groups::GroupHeader;
use crate::terminal::store::{GroupLayout, SplitOrientation};
use gpui_kit::{Bounds, ExternalPaths, FileDropEvent, Modifiers, MouseButton, Pixels, point, px};
use std::cell::RefCell;
use std::rc::Rc;

/// A panel on a trusted temporary project whose terminals run the plain test shell, in an active
/// window; the home and the project live as long as the returned guards.
fn panel_window(cx: &mut TestAppContext) -> (Entity<TerminalPanel>, &mut VisualTestContext, [tempfile::TempDir; 2]) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let root = folder.path().canonicalize().expect("the folder exists");
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        cx.set_global(UiStorage::default());
        crate::platform::input::init(cx);
        Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        crate::terminal::test_support::isolate_shells(home.path(), cx);
        crate::terminal::test_support::trusted_folder("w1", &root, cx);
    });
    let (panel, cx) = cx.add_window_view(|_, cx| TerminalPanel::new("w1", root, cx));
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (panel, cx, [home, folder])
}

fn active_tab(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> String {
    panel.read_with(cx, |panel, _| panel.terminals().active_tab().map(str::to_owned)).expect("a tab is active")
}

fn view_of(panel: &Entity<TerminalPanel>, tab: &str, cx: &mut VisualTestContext) -> Entity<TerminalView> {
    panel.read_with(cx, |panel, _| panel.view_of_tab(tab).cloned()).expect("the tab's shell started")
}

fn screen(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.emulator().lines().join("\n"))
}

fn focused(view: &Entity<TerminalView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|window, cx| view.focus_handle(cx).is_focused(window))
}

/// Where the element `selector` is drawn.
fn bounds_of(selector: String, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    cx.run_until_parked();
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
}

fn click(selector: String, cx: &mut VisualTestContext) {
    let at = bounds_of(selector, cx).center();
    cx.simulate_click(at, Modifiers::default());
    cx.run_until_parked();
}

/// The width of the terminal of `group`.
fn width_of(group: &str, cx: &mut VisualTestContext) -> f32 {
    f32::from(bounds_of(format!("terminal-group:{group}"), cx).size.width)
}

/// Split Right of the active tab: the left group, the right group and the split's id.
fn split_right(panel: &Entity<TerminalPanel>, cx: &mut VisualTestContext) -> (String, String, String) {
    let tab = active_tab(panel, cx);
    panel.update(cx, |panel, cx| {
        panel.change(cx, |terminals| terminals.split_tab(&tab, SplitOrientation::Horizontal, false, "Local"))
    });
    cx.run_until_parked();
    let GroupLayout::Split(split) = panel.read_with(cx, |panel, _| panel.terminals().layout().clone()) else {
        panic!("the window is split")
    };
    let groups = panel.read_with(cx, |panel, _| panel.panes().into_iter().map(|(group, _)| group).collect::<Vec<_>>());
    assert_eq!(groups.len(), 2, "{groups:?}");
    (groups[0].clone(), groups[1].clone(), split.id)
}

fn labels(tabs: &[(&str, bool)]) -> Vec<(String, bool)> {
    tabs.iter().map(|(label, selected)| ((*label).to_owned(), *selected)).collect()
}

/// Split, each group draws a header of its own: the window's title in the first, its Options and
/// Hide buttons in the top right one. A group's New Tab opens the tab in that group with the keyboard
/// in it, a click on a group's tab selects it there, and a press in a group's terminal makes its tab
/// the active one.
#[gpui_kit::test]
async fn each_group_has_a_header_whose_tabs_and_new_tab_button_act_on_that_group(cx: &mut TestAppContext) {
    let (panel, cx, _dirs) = panel_window(cx);
    let first = active_tab(&panel, cx);
    let (left, right, _) = split_right(&panel, cx);
    let second = active_tab(&panel, cx);
    let headers = |cx: &mut VisualTestContext| panel.read_with(cx, |panel, _| panel.headers());
    assert_eq!(
        headers(cx),
        [
            GroupHeader { group: left.clone(), title: true, actions: false, tabs: labels(&[("Local", true)]) },
            GroupHeader { group: right.clone(), title: false, actions: true, tabs: labels(&[("Local (2)", true)]) },
        ]
    );
    let (left_header, right_header) =
        (bounds_of(format!("terminal-header:{left}"), cx), bounds_of(format!("terminal-header:{right}"), cx));
    assert!(right_header.origin.x > left_header.origin.x && right_header.origin.y == left_header.origin.y);
    let options = bounds_of("more:terminal".to_owned(), cx);
    assert!(right_header.contains(&options.center()), "the window's buttons are in the top right header");
    assert!(right_header.contains(&bounds_of("hide:terminal".to_owned(), cx).center()));
    assert!(left_header.contains(&bounds_of(format!("terminal-tab:{first}"), cx).center()));
    assert!(right_header.contains(&bounds_of(format!("terminal-tab:{second}"), cx).center()));

    click(format!("terminal-new-tab:{left}"), cx);
    let third = active_tab(&panel, cx);
    let tabs: Vec<Vec<(String, bool)>> = headers(cx).into_iter().map(|header| header.tabs).collect();
    assert_eq!(tabs, [labels(&[("Local", false), ("Local (3)", true)]), labels(&[("Local (2)", true)])]);
    assert!(focused(&view_of(&panel, &third, cx), cx), "the new tab has the keyboard");

    click(format!("terminal-tab:{first}"), cx);
    assert_eq!(headers(cx)[0].tabs, labels(&[("Local", true), ("Local (3)", false)]));
    assert_eq!(active_tab(&panel, cx), first);
    assert!(focused(&view_of(&panel, &first, cx), cx), "the selected tab has the keyboard");

    let pane = bounds_of(format!("terminal-group:{right}"), cx);
    cx.simulate_click(pane.center(), Modifiers::default());
    cx.run_until_parked();
    assert_eq!(active_tab(&panel, cx), second, "a press in a group's terminal makes its tab the active one");
    assert!(focused(&view_of(&panel, &second, cx), cx));
}

/// The line between two groups follows the pointer while it is dragged, the two groups trading
/// room, and the group it moves into keeps 60 pixels; the line stays where the button was released.
#[gpui_kit::test]
async fn dragging_the_line_between_two_groups_resizes_them_down_to_sixty_pixels(cx: &mut TestAppContext) {
    let (panel, cx, _dirs) = panel_window(cx);
    let (left, right, split) = split_right(&panel, cx);
    let (left_before, right_before) = (width_of(&left, cx), width_of(&right, cx));
    assert!((left_before - right_before).abs() <= 1.0, "{left_before} {right_before}: equal shares at first");

    let drag = |by: f32, cx: &mut VisualTestContext| {
        let start = bounds_of(format!("terminal-divider:{split}:0"), cx).center();
        let end = point(start.x + px(by), start.y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
    };
    drag(100.0, cx);
    let (left_after, right_after) = (width_of(&left, cx), width_of(&right, cx));
    assert!((left_after - left_before - 100.0).abs() <= 1.5, "{left_before} → {left_after}");
    assert!((right_before - right_after - 100.0).abs() <= 1.5, "{right_before} → {right_after}");

    drag(5000.0, cx);
    let right_least = width_of(&right, cx);
    assert!((right_least - 60.0).abs() <= 1.5, "the right group keeps 60 pixels: {right_least}");
    assert!((width_of(&left, cx) + right_least - left_before - right_before).abs() <= 1.5);

    // Released, the line no longer follows the pointer.
    let at = bounds_of(format!("terminal-divider:{split}:0"), cx).center();
    cx.simulate_mouse_move(point(at.x - px(300.0), at.y), None, Modifiers::default());
    cx.run_until_parked();
    assert!((width_of(&right, cx) - right_least).abs() <= 0.5);

    // The line can be grabbed a few pixels to either side, over the terminals beside it: a press
    // there drags the line and selects no text in them.
    let tabs = panel.read_with(cx, |panel, _| panel.panes().into_iter().filter_map(|(_, tab)| tab).collect::<Vec<_>>());
    let views: Vec<Entity<TerminalView>> = tabs.iter().map(|tab| view_of(&panel, tab, cx)).collect();
    // Rows full of text on both sides of the line, so that a press that reached a terminal would
    // select some.
    for view in &views {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_input("printf '%01000d\\n' 0");
        cx.simulate_keystrokes("enter");
        settle(cx, "the rows fill", |cx| {
            view.read_with(cx, |view, _| view.emulator().lines().concat().contains(&"0".repeat(40)))
        });
    }
    let row = bounds_of(format!("terminal-group:{right}"), cx).top() + px(40.0);
    for (offset, by) in [(2.0, -200.0), (-2.0, -100.0)] {
        let line = bounds_of(format!("terminal-divider:{split}:0"), cx).center();
        let (start, before) = (point(line.x + px(offset), row), width_of(&right, cx));
        let end = point(start.x + px(by), start.y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
        assert!((width_of(&right, cx) - before + by).abs() <= 1.5, "a press {offset}px off the line drags it");
        for view in &views {
            assert_eq!(view.read_with(cx, |view, _| view.emulator().selected_text()), None, "no text is selected");
        }
    }
    // The same press and drag away from the line selects text in the terminal under it.
    let left_pane = bounds_of(format!("terminal-group:{left}"), cx);
    let start = point(left_pane.left() + px(40.0), row);
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(point(start.x + px(100.0), row), MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(point(start.x + px(100.0), row), MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let selected = views[0].read_with(cx, |view, _| view.emulator().selected_text());
    assert!(selected.is_some_and(|text| text.contains('0')), "a drag in a terminal selects its text");
}

/// Split Down stacks a new group under the first, each with a header of its own, the window's title
/// and buttons in the top one; the line between them is dragged up and down, each group keeping 60
/// pixels.
#[gpui_kit::test]
async fn split_down_stacks_a_group_with_its_own_header_under_a_line_dragged_up_and_down(cx: &mut TestAppContext) {
    let (panel, cx, _dirs) = panel_window(cx);
    let tab = active_tab(&panel, cx);
    panel.update(cx, |panel, cx| {
        panel.change(cx, |terminals| terminals.split_tab(&tab, SplitOrientation::Vertical, false, "Local"))
    });
    cx.run_until_parked();
    let GroupLayout::Split(split) = panel.read_with(cx, |panel, _| panel.terminals().layout().clone()) else {
        panic!("the window is split")
    };
    let headers = panel.read_with(cx, |panel, _| panel.headers());
    assert_eq!(headers.len(), 2, "{headers:?}");
    assert!(headers[0].title && headers[0].actions && !headers[1].title && !headers[1].actions, "{headers:?}");
    let (top, bottom) = (headers[0].group.clone(), headers[1].group.clone());
    let (top_header, bottom_header) =
        (bounds_of(format!("terminal-header:{top}"), cx), bounds_of(format!("terminal-header:{bottom}"), cx));
    assert!(bottom_header.origin.y > top_header.origin.y && bottom_header.origin.x == top_header.origin.x);
    assert!(top_header.contains(&bounds_of("more:terminal".to_owned(), cx).center()));

    let height = |group: &str, cx: &mut VisualTestContext| {
        f32::from(bounds_of(format!("terminal-header:{group}"), cx).size.height)
            + f32::from(bounds_of(format!("terminal-group:{group}"), cx).size.height)
    };
    let drag = |by: f32, cx: &mut VisualTestContext| {
        let start = bounds_of(format!("terminal-divider:{}:0", split.id), cx).center();
        let end = point(start.x, start.y + px(by));
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::default());
        cx.run_until_parked();
    };
    let (top_before, bottom_before) = (height(&top, cx), height(&bottom, cx));
    drag(100.0, cx);
    assert!((height(&top, cx) - top_before - 100.0).abs() <= 1.5, "the top group grows by 100");
    assert!((bottom_before - height(&bottom, cx) - 100.0).abs() <= 1.5, "the bottom group gives them up");
    drag(-5000.0, cx);
    assert!((height(&top, cx) - 60.0).abs() <= 1.5, "the top group keeps 60 pixels: {}", height(&top, cx));
}

// src/features/terminal/drop.test.ts › terminal drop › pastes only into the host under the cursor without executing,
// regardless of registration order
/// Files dropped on a group's terminal are typed at its prompt, quoted and separated by spaces, and
/// run only once Enter is pressed; the other group's terminal gets nothing.
#[gpui_kit::test]
async fn pastes_only_into_the_terminal_under_the_cursor_without_executing(cx: &mut TestAppContext) {
    let (panel, cx, _dirs) = panel_window(cx);
    let first = active_tab(&panel, cx);
    let (left, _right, _) = split_right(&panel, cx);
    let second = active_tab(&panel, cx);
    let (left_view, right_view) = (view_of(&panel, &first, cx), view_of(&panel, &second, cx));
    settle(cx, "both shells run", |cx| {
        left_view.read_with(cx, |view, _| view.id().is_some())
            && right_view.read_with(cx, |view, _| view.id().is_some())
    });
    cx.update(|window, cx| {
        let focus = left_view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_input("printf '<%s>\\n' ");
    let at = bounds_of(format!("terminal-group:{left}"), cx).center();
    let paths = ExternalPaths(vec![std::path::PathBuf::from("/a b"), std::path::PathBuf::from("/plain")].into());
    cx.simulate_event(FileDropEvent::Entered { position: at, paths });
    cx.simulate_event(FileDropEvent::Submit { position: at });
    settle(cx, "the paths are typed", |cx| screen(&left_view, cx).contains("printf '<%s>\\n' '/a b' /plain"));
    assert!(!screen(&left_view, cx).contains("</a b>"), "nothing runs before Enter");

    cx.simulate_keystrokes("enter");
    settle(cx, "the command runs", |cx| {
        let shown = screen(&left_view, cx);
        shown.contains("</a b>") && shown.contains("</plain>")
    });
    assert!(!screen(&right_view, cx).contains("/plain"), "the other group's terminal gets nothing");
}

/// ⌘W in a terminal asks the window to close that terminal's tab, which the window does after asking
/// about a command that runs; the panel itself closes nothing.
#[gpui_kit::test]
async fn cmd_w_in_a_terminal_asks_the_window_to_close_its_tab(cx: &mut TestAppContext) {
    let (panel, cx, _dirs) = panel_window(cx);
    panel.update(cx, |panel, cx| panel.open_tab(cx));
    let tab = active_tab(&panel, cx);
    let events: Rc<RefCell<Vec<TerminalPanelEvent>>> = Rc::default();
    let log = events.clone();
    let _subscription = cx.update(|_, cx| {
        cx.subscribe(&panel, move |_, event: &TerminalPanelEvent, _| log.borrow_mut().push(event.clone()))
    });
    let view = view_of(&panel, &tab, cx);
    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.simulate_keystrokes("cmd-w");
    cx.run_until_parked();
    assert!(events.borrow().contains(&TerminalPanelEvent::CloseTabs(vec![tab])), "{:?}", events.borrow());
    assert_eq!(panel.read_with(cx, |panel, _| panel.tab_titles().len()), 2, "the window decides");
}
