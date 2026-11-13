use super::*;
use crate::platform::actions::execute_action_or_throw;

/// Maximize Tool Window draws the focused Project view across the window, over where the editor was, and ⌘⇧'
/// draws it back at its width. With the editor focused no tool window has the keyboard and the action does not run.
#[gpui_kit::test]
async fn maximize_tool_window_draws_the_focused_side_across_the_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.zig", "const a = 1;\n")]);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("main.zig"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    cx.run_until_parked();
    assert!(context_is_on(cx, "editorFocus") && !context_is_on(cx, "toolWindowFocus"));
    let unavailable = cx.update(|window, cx| execute_action_or_throw("toolwindow.maximize", None, window, cx));
    assert!(unavailable.is_err(), "Maximize needs a focused tool window");

    run_action(cx, "toolwindow.project", None);
    assert!(context_is_on(cx, "toolWindowFocus"));
    let width = cx.update(|window, _| f32::from(window.viewport_size().width));
    let header_end = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        let bounds = cx.debug_bounds("hide:project").expect("the Project view's header is drawn");
        f32::from(bounds.right())
    };
    let side = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.tool_windows().and_then(|layout| layout.maximized()))
    };
    let docked = header_end(cx);
    assert!(docked < width / 2.0, "docked on the left: {docked} of {width}");

    run_action(cx, "toolwindow.maximize", None);
    assert_eq!(side(cx), Some(Anchor::Left));
    let maximized = header_end(cx);
    assert!(maximized > width * 0.75, "drawn across the window: {maximized} of {width}");

    cx.simulate_keystrokes("cmd-shift-'");
    assert_eq!(side(cx), None);
    assert_eq!(header_end(cx), docked, "drawn at its width again");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// Background Tasks opens the tasks popup, and run again while it is open it stays open: the action only shows it.
#[gpui_kit::test]
async fn background_tasks_opens_the_popup_and_running_it_again_keeps_it_open(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.zig", "const a = 1;\n")]);
    let open = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.background_tasks_popup().is_some());
    assert!(!open(cx));
    run_action(cx, "view.backgroundTasks", None);
    assert!(open(cx), "the popup opens");
    run_action(cx, "view.backgroundTasks", None);
    assert!(open(cx), "asked again, the popup stays open");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
