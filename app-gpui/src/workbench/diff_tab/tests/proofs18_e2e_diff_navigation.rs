use super::*;
use crate::vcs_diff::toolbar_view::counter_text;

fn counter(viewer: &Entity<DiffView>, cx: &mut VisualTestContext) -> String {
    viewer.read_with(cx, |viewer, cx| counter_text(&viewer.summary(), cx))
}

fn viewer_width(cx: &mut VisualTestContext) -> f32 {
    cx.run_until_parked();
    f32::from(cx.debug_bounds("diff-view").expect("the diff is drawn").size.width)
}

fn click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// A changed file's diff in its tab beside the Project view: Maximize Editor from the toolbar hides the
/// Project view and draws the diff wider, still at "1 difference"; Restore Size brings the Project view back
/// and draws the diff at its width again.
#[gpui_kit::test]
async fn maximizes_a_diff_preview_tab_and_restores_its_previous_layout(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = non_modal_project(cx);
    let viewer = open_diff_tab(&workbench, &root, cx);
    let project_shown =
        |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.is_tool_window_visible("project"));
    assert!(project_shown(cx), "the Project view is shown");
    assert_eq!(counter(&viewer, cx), "1 difference");
    let before = viewer_width(cx);

    click(cx, "diff-maximize");
    assert!(!project_shown(cx), "the Project view is hidden");
    assert_eq!(maximize_button(&viewer, cx), Some(("Restore Size", true)));
    assert!(viewer_width(cx) > before + 100.0, "the diff is drawn wider");
    assert_eq!(counter(&viewer, cx), "1 difference");

    click(cx, "diff-maximize");
    assert!(project_shown(cx), "the Project view is back");
    assert!((viewer_width(cx) - before).abs() < 2.0, "the diff is drawn at its width again");
    assert_eq!(counter(&viewer, cx), "1 difference");
}
