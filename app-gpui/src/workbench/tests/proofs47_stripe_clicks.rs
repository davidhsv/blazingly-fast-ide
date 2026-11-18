use super::*;
use crate::code_insight::PROBLEMS_TOOL_WINDOW;

/// A click on a tool window's stripe button hides the window while it has the keyboard and shows it
/// again on the next click: the Project window on the left, Problems at the bottom.
#[gpui_kit::test]
async fn a_click_on_a_stripe_button_hides_and_shows_its_tool_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let folder = tempfile::tempdir().expect("a temporary folder");
    std::fs::write(folder.path().join("notes.txt"), "notes\n").expect("the file is written");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(folder.path().to_path_buf(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let shown = |cx: &mut VisualTestContext, id: &str| workbench.read_with(cx, |w, _| w.is_tool_window_visible(id));
    assert!(shown(cx, PROJECT_TOOL_WINDOW), "a project opens with its Project window");

    click_on(cx, "stripe:project");
    assert!(!shown(cx, PROJECT_TOOL_WINDOW), "the click hides the focused Project window");
    click_on(cx, "stripe:project");
    assert!(shown(cx, PROJECT_TOOL_WINDOW), "the next click shows it again");

    assert!(!shown(cx, PROBLEMS_TOOL_WINDOW));
    click_on(cx, "stripe:problems");
    assert!(shown(cx, PROBLEMS_TOOL_WINDOW), "the click shows Problems");
    click_on(cx, "stripe:problems");
    assert!(!shown(cx, PROBLEMS_TOOL_WINDOW), "the next click hides it");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.close_project("w1");
}
