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

/// Hide Active Tool Window on a side maximized for a window that has gone (its plugin was
/// disabled) gives the editor and the other tool windows back, instead of hiding nothing.
#[gpui_kit::test]
async fn hide_active_tool_window_clears_a_maximized_side_whose_window_went(cx: &mut TestAppContext) {
    use crate::workbench::tool_windows::{Anchor, ToolWindow};
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.zig", "const a = 1;\n")]);
    cx.update(|window, _| window.activate_window());
    let id = "plugin.k8s.view:kubernetes";
    let registration = workbench.update_in(cx, |w, window, cx| {
        let mut registration = None;
        w.with_layout(window, cx, |layout| {
            let kubernetes = ToolWindow::new(id, "Kubernetes", "cloud", Anchor::Right);
            registration = Some(layout.register(ToolWindow { secondary: true, ..kubernetes }).0);
            let _ = layout.show(id, true);
            layout.set_focused(Some(id));
            layout.toggle_maximize(id)
        });
        registration.expect("the window registers")
    });
    cx.run_until_parked();
    let layout = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| {
            let layout = w.tool_windows().expect("a layout");
            (layout.maximized(), layout.is_visible("project"))
        })
    };
    assert_eq!(layout(cx), (Some(Anchor::Right), false));
    workbench.update_in(cx, |w, window, cx| w.with_layout(window, cx, |layout| layout.unregister(registration)));
    cx.run_until_parked();

    run_action(cx, "toolwindow.hideActive", None);
    assert_eq!(layout(cx), (None, true), "nothing is maximized and the Project view shows again");
    cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
}
