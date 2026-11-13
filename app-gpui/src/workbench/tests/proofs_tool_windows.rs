use super::*;

/// Hide Active Tool Window hides the tool window that has the keyboard, and the keyboard leaves it.
#[gpui_kit::test]
async fn hide_active_tool_window_hides_the_focused_project_view(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.zig", "const a = 1;\n")]);
    cx.update(|window, _| window.activate_window());
    let shown = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.tool_windows().expect("a layout").is_visible("project"))
    };
    // A project opens with its Project view shown and focused.
    assert!(shown(cx) && context_is_on(cx, "toolWindowFocus"));
    run_action(cx, "toolwindow.hideActive", None);
    assert!(!shown(cx), "the Project view is hidden");
    assert!(!context_is_on(cx, "toolWindowFocus"), "no tool window has the keyboard");
    assert!(!context_is_on(cx, "toolWindow.project.visible"));
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}

/// With a bottom tool window focused, Resize Up raises its inner edge by a step and Resize Down brings it back.
#[gpui_kit::test]
async fn resize_up_and_down_move_a_bottom_tool_windows_edge(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.zig", "const a = 1;\n")]);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "toolwindow.run", None);
    assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(RUN_TOOL_WINDOW)));
    assert!(context_is_on(cx, "toolWindowFocus"), "the Run tool window has the keyboard");
    let height = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| w.tool_windows().expect("a project is open").sizes().bottom)
    };
    let before = height(cx);
    run_action(cx, "toolwindow.resizeUp", None);
    assert!(height(cx) > before, "{:?} > {before:?}", height(cx));
    run_action(cx, "toolwindow.resizeDown", None);
    assert_eq!(height(cx), before);
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
