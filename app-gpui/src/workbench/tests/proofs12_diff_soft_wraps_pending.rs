use super::*;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, load_diff_options};
use crate::vcs_diff::toolbar_view::{DiffCommand, settings_menu};
use crate::vcs_diff::view::DiffView;
use crate::vcs_ops::menu::MenuItem;
use ide_workspace::settings::SettingsScope;
use serde_json::json;
use std::path::Path;

/// The editor's own Soft-wrap files patterns.
const DEFAULT_SOFT_WRAP_FILES: &str = "*.md; *.txt; *.rst; *.adoc";

/// A repository whose `a.ts` gained a line after its commit.
fn wraps_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.ts"), "a\nb\n").expect("the file is written");
        git_in(root, &["add", "a.ts"]);
        git_in(root, &["commit", "-q", "-m", "a.ts"]);
        std::fs::write(root.join("a.ts"), "a\nnew\nb\n").expect("the file is written");
    })
}

fn set_setting(workbench: &Entity<Workbench>, key: &str, value: serde_json::Value, cx: &mut VisualTestContext) {
    let label = workbench.read_with(cx, |w, _| w.label.clone());
    cx.update(|_, cx| settings::set(&label, SettingsScope::Global, key, &value, cx)).expect("the setting is written");
    cx.run_until_parked();
}

/// Settings › Editor › Soft Wraps: Soft-wrap files, and its file patterns.
fn editor_soft_wraps(workbench: &Entity<Workbench>, on: bool, files: &str, cx: &mut VisualTestContext) {
    set_setting(workbench, "editor.softWrap", json!(on), cx);
    set_setting(workbench, "editor.general.softWrapFiles", json!(files), cx);
}

fn store_diff_options(stored: &str, cx: &mut VisualTestContext) {
    let stored = stored.to_owned();
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(DIFF_OPTIONS_KEY, stored));
}

fn stored_soft_wrap(cx: &mut VisualTestContext) -> Option<bool> {
    cx.update(|_, cx| load_diff_options(cx.global::<UiStorage>().get(DIFF_OPTIONS_KEY)).soft_wrap)
}

/// Show Diff of `a.ts`, once its texts are loaded.
fn open_diff_of_a(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Entity<DiffView> {
    run_action(cx, "vcs.showDiff", Some(json!(text_of(&root.join("a.ts")))));
    let dialog = workbench.read_with(cx, |w, _| w.diff_dialog().cloned()).expect("the diff is shown");
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    wait_long(cx, "the diff to be loaded", |cx| viewer.read_with(cx, |viewer, _| viewer.difference_count() > 0));
    viewer
}

fn close_diff(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.diff_dialog().is_none()));
}

fn wraps(viewer: &Entity<DiffView>, cx: &mut VisualTestContext) -> bool {
    viewer.read_with(cx, |viewer, _| viewer.soft_wraps())
}

/// Use Soft Wraps, from the viewer's settings menu.
fn toggle_soft_wraps(viewer: &Entity<DiffView>, cx: &mut VisualTestContext) {
    let items = settings_menu(&viewer.read_with(cx, |viewer, _| viewer.options()));
    let command: Option<DiffCommand> =
        items.iter().find(|item| item.label() == Some("Use Soft Wraps")).and_then(MenuItem::command).copied();
    let command = command.expect("the settings menu offers Use Soft Wraps");
    viewer.update(cx, |viewer, cx| viewer.run_command(command, cx));
    cx.run_until_parked();
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › soft wraps › follow the editor's setting when
// $name
/// Without a choice of its own the diff wraps its lines as the editor would wrap the file: when Soft-wrap files is
/// on and its patterns match the file, side by side and unified.
#[gpui_kit::test]
async fn soft_wraps_follow_the_editors_setting(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = wraps_project(cx);
    for (name, on, files, mode, expected) in [
        ("the file matches Soft-wrap files", true, "*.ts", "split", true),
        ("the file matches, unified viewer", true, "*.ts", "unified", true),
        ("the file is not in Soft-wrap files", true, DEFAULT_SOFT_WRAP_FILES, "split", false),
        ("soft wraps are off in the editor", false, "*.ts", "split", false),
    ] {
        editor_soft_wraps(&workbench, on, files, cx);
        store_diff_options(&format!(r#"{{"mode":"{mode}"}}"#), cx);
        let viewer = open_diff_of_a(&workbench, &root, cx);
        assert_eq!(wraps(&viewer, cx), expected, "{name}");
        close_diff(&workbench, cx);
    }
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › soft wraps › reconfigures $mode (aligned:
// $alignChanges) without losing selection
/// The diff follows changes of the editor's setting while it is open, until Use Soft Wraps is chosen in its own
/// menu: that choice is stored, outlasts the editor's setting and the next diff, and none of it rebuilds the
/// viewer or moves its caret.
#[gpui_kit::test]
async fn soft_wraps_reconfigure_without_losing_the_selection(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = wraps_project(cx);
    for (mode, align) in [("split", false), ("split", true), ("unified", false)] {
        let case = format!("{mode}, aligned: {align}");
        editor_soft_wraps(&workbench, true, "*.ts", cx);
        store_diff_options(&format!(r#"{{"mode":"{mode}","alignChanges":{align}}}"#), cx);
        let viewer = open_diff_of_a(&workbench, &root, cx);
        viewer.update(cx, |viewer, cx| viewer.reveal_line(crate::vcs_diff::diff_options::Side::B, 1, cx));
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0), "{case}");
        assert!(wraps(&viewer, cx), "{case}");
        editor_soft_wraps(&workbench, false, "*.ts", cx);
        wait_long(cx, "the diff to stop wrapping", |cx| !wraps(&viewer, cx));
        editor_soft_wraps(&workbench, true, "*.ts", cx);
        wait_long(cx, "the diff to wrap again", |cx| wraps(&viewer, cx));

        toggle_soft_wraps(&viewer, cx);
        assert!(!wraps(&viewer, cx), "{case}: Use Soft Wraps turns them off");
        assert_eq!(stored_soft_wrap(cx), Some(false), "{case}");
        editor_soft_wraps(&workbench, true, "*", cx);
        assert!(!wraps(&viewer, cx), "{case}: the viewer's own choice outlasts the editor's setting");
        let shown = workbench.read_with(cx, |w, cx| w.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone()));
        assert_eq!(shown.as_ref(), Some(&viewer), "{case}: the viewer is not rebuilt");
        assert_eq!(viewer.read_with(cx, |viewer, _| viewer.current_difference()), Some(0), "{case}");
        close_diff(&workbench, cx);

        let again = open_diff_of_a(&workbench, &root, cx);
        assert!(!wraps(&again, cx), "{case}: the next diff keeps the choice");
        close_diff(&workbench, cx);
    }
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › soft wraps › turn on from the settings menu and
// stay on
/// Use Soft Wraps in the viewer's settings menu wraps a file the editor would not wrap, stores the choice, and the
/// next diff opens wrapped.
#[gpui_kit::test]
async fn soft_wraps_turn_on_from_the_settings_menu_and_stay_on(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = wraps_project(cx);
    editor_soft_wraps(&workbench, true, DEFAULT_SOFT_WRAP_FILES, cx);
    let viewer = open_diff_of_a(&workbench, &root, cx);
    assert!(!wraps(&viewer, cx), "the editor would not wrap a.ts");
    toggle_soft_wraps(&viewer, cx);
    assert!(wraps(&viewer, cx));
    assert_eq!(stored_soft_wrap(cx), Some(true));
    close_diff(&workbench, cx);
    let again = open_diff_of_a(&workbench, &root, cx);
    assert!(wraps(&again, cx), "the next diff opens wrapped");
    close_diff(&workbench, cx);
}
