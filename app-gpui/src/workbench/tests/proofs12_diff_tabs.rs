use super::*;
use crate::platform::settings;
use crate::vcs::commit_view::CommitPanel;
use crate::vcs_diff::dialog_view::DiffDialog;
use gpui_kit::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent};
use ide_workspace::settings::SettingsScope;
use serde_json::json;

fn window_label(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |w, _| w.label.clone())
}

fn set_setting(workbench: &Entity<Workbench>, key: &str, value: serde_json::Value, cx: &mut VisualTestContext) {
    let label = window_label(workbench, cx);
    cx.update(|_, cx| settings::set(&label, SettingsScope::Global, key, &value, cx)).expect("the setting is written");
    cx.run_until_parked();
}

/// Settings › Version Control › Commit › Use non-modal commit interface.
fn set_non_modal(workbench: &Entity<Workbench>, on: bool, cx: &mut VisualTestContext) {
    set_setting(workbench, "vcs.commit.nonModal", json!(on), cx);
    let label = window_label(workbench, cx);
    wait_long(cx, "the commit interface setting", |cx| {
        cx.update(|_, cx| settings::effective(&label, cx).vcs.commit.non_modal) == on
    });
}

fn diff_dialog(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<DiffDialog>> {
    workbench.read_with(cx, |w, _| w.diff_dialog().cloned())
}

/// A repository whose `a.txt` was changed after its first commit.
fn changed_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
    })
}

/// The Commit tool window, once it lists the changed `a.txt`.
fn commit_changes(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<CommitPanel> {
    run_action(cx, "toolwindow.commit", None);
    let panel = workbench
        .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
        .and_then(|view| view.downcast::<CommitPanel>().ok())
        .expect("the Commit window holds the changes");
    wait_long(cx, "a.txt to be listed", |cx| {
        panel.read_with(cx, |panel, _| panel.lines().iter().any(|line| line.trim() == "a.txt"))
    });
    panel
}

fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    cx.run_until_parked();
}

// src/features/vcs/__tests__/diffDialog.test.tsx › opens an editor tab instead once the non-modal interface is chosen
/// With the non-modal commit interface, activating a changed file in the Commit window opens its diff in an
/// editor tab, not in the diff window.
#[gpui_kit::test]
async fn opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, _root) = changed_project(cx);
    set_non_modal(&workbench, true, cx);
    commit_changes(&workbench, cx);
    double_click(cx, "commit-row-a.txt");
    assert!(diff_dialog(&workbench, cx).is_none(), "no diff window is shown");
    let diff_tab = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| {
            let tabs = w.tabs().map(|tabs| tabs.panes.iter().flat_map(|pane| &pane.tabs));
            tabs.is_some_and(|mut tabs| tabs.any(|tab| tab.input.kind == "diff"))
        })
    };
    wait_long(cx, "the diff's editor tab", diff_tab);
}
