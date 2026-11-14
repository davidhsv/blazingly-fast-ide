use super::*;
use crate::platform::actions::ActionRegistry;
use crate::platform::settings;
use crate::platform::storage::UiStorage;
use crate::vcs::commit_view::CommitPanel;
use crate::vcs_diff::dialog_view::DiffDialog;
use crate::vcs_diff::diff_options::{DIFF_OPTIONS_KEY, DiffMode, Side, load_diff_options};
use crate::vcs_diff::toolbar_view::ToolbarItem;
use crate::vcs_diff::view::DiffView;
use crate::workbench::shelf::ChangeCommand;
use gpui_kit::{Focusable, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, point, px};
use ide_workspace::settings::SettingsScope;
use serde_json::json;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

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

/// The viewer of the diff window, once its texts are loaded and differ.
fn loaded_viewer(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<DiffView> {
    let dialog = diff_dialog(workbench, cx).expect("the diff is shown");
    let viewer = dialog.read_with(cx, |dialog, _| dialog.viewer().clone());
    wait_long(cx, "the diff to be loaded", |cx| viewer.read_with(cx, |viewer, _| viewer.difference_count() > 0));
    viewer
}

fn offers_jump_to_source(viewer: &Entity<DiffView>, cx: &mut VisualTestContext) -> bool {
    viewer.read_with(cx, |viewer, cx| {
        viewer.toolbar(cx).iter().filter_map(ToolbarItem::button).any(|button| button.id == "diff-jump-to-source")
    })
}

/// The file the active editor shows and the 0-based line of its caret.
fn edited_at(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<(PathBuf, u32)> {
    workbench.read_with(cx, |w, cx| {
        w.editor().map(|editor| {
            let editor = editor.read(cx);
            (editor.path().to_path_buf(), editor.caret_position().0)
        })
    })
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

/// A repository whose `a.txt` gains lines above the old ones at each step: in the parent commit, in the last
/// commit and in the working copy.
fn revision_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    git_project(cx, |root| {
        init_repo(root);
        for (text, subject) in [("a\nb\nc\nd\n", "parent"), ("a\nX\nY\nb\nc\nd\n", "commit")] {
            std::fs::write(root.join("a.txt"), text).expect("the file is written");
            git_in(root, &["commit", "-q", "-am", subject]);
        }
        std::fs::write(root.join("a.txt"), "new\na\nX\nY\nb\nc\nd\n").expect("the file is written");
    })
}

/// The diff of `a.txt` between the parent commit and the last one, as Git Log opens a commit's file.
fn open_revision_diff(workbench: &Entity<Workbench>, root: &Path, cx: &mut VisualTestContext) -> Entity<DiffView> {
    let data = json!({
        "path": text_of(&root.join("a.txt")),
        "left": { "type": "rev", "rev": "HEAD~1" },
        "right": { "type": "rev", "rev": "HEAD" },
    });
    run_action(cx, "vcs.openDiff", Some(data));
    loaded_viewer(workbench, cx)
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

/// The actions the keyboard runs while `keys` are pressed.
fn actions_run_by(keys: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let ran: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = ran.clone();
    let listener = cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
    });
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
    ran.take()
}

// src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › opens the diff in a front
// dialog, not in an editor tab
/// With the modal commit interface, activating a changed file in the Commit window shows its diff in the diff
/// window in front, and opens no editor tab.
#[gpui_kit::test]
async fn opens_the_diff_in_a_front_dialog_not_in_an_editor_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    set_non_modal(&workbench, false, cx);
    commit_changes(&workbench, cx);
    double_click(cx, "commit-row-a.txt");
    let dialog = diff_dialog(&workbench, cx).expect("the diff is shown in front");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.data().path.clone()), text_of(&root.join("a.txt")));
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title()), "a.txt (HEAD ↔ Your version)");
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no editor tab is opened");
}

// src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › opens the context menu's Show
// Diff in a front dialog, not in an editor tab
/// Show Diff from the changes' context menu shows the file's diff in the diff window in front, and opens no
/// editor tab.
#[gpui_kit::test]
async fn opens_the_context_menus_show_diff_in_a_front_dialog_not_in_an_editor_tab(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    set_non_modal(&workbench, false, cx);
    commit_changes(&workbench, cx);
    click_on(cx, "commit-row-a.txt");
    let at = cx.debug_bounds("commit-row-a.txt").expect("the row is drawn").center();
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<ChangeCommand>()).expect("the changes menu is open");
    let is_selected = |cx: &mut VisualTestContext| {
        let lines = menu.read_with(cx, |menu, _| menu.lines());
        lines.first().is_some_and(|rows| rows.contains(&"> Show Diff".to_owned()))
    };
    for _ in 0..40 {
        if is_selected(cx) {
            break;
        }
        cx.simulate_keystrokes("down");
    }
    assert!(is_selected(cx), "Show Diff in {:?}", menu.read_with(cx, |menu, _| menu.lines()));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let dialog = diff_dialog(&workbench, cx).expect("the diff is shown in front");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.data().path.clone()), text_of(&root.join("a.txt")));
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no editor tab is opened");
}

// src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › remembers the resized size for
// the next opening (#2328)
/// The diff window has a resize grip; the size it is dragged to is the size the next diff window opens at.
#[gpui_kit::test]
async fn remembers_the_resized_size_for_the_next_opening(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    let path = text_of(&root.join("a.txt"));
    run_action(cx, "vcs.showDiff", Some(json!(path)));
    loaded_viewer(&workbench, cx);
    let before = cx.debug_bounds("diff-dialog").expect("the diff window is drawn").size;
    let grip = cx.debug_bounds("diff-window-resize").expect("the diff window has a resize grip").center();
    let to = grip - point(px(100.0), px(80.0));
    cx.simulate_mouse_down(grip, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let resized = cx.debug_bounds("diff-dialog").expect("the diff window is drawn").size;
    assert_ne!(resized, before, "the drag resizes the window");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(diff_dialog(&workbench, cx).is_none());
    run_action(cx, "vcs.showDiff", Some(json!(path)));
    loaded_viewer(&workbench, cx);
    assert_eq!(cx.debug_bounds("diff-dialog").expect("the diff window is drawn").size, resized);
}

// src/features/vcs/__tests__/diffDialog.test.tsx › Commit dialog diff window (#1863) › owns the close shortcut with
// %s focus
/// The close shortcut, which also closes editor tabs, closes the diff window whether its viewer or the window
/// itself has the keyboard; the editor tab behind it stays open.
#[gpui_kit::test]
async fn owns_the_close_shortcut_with_body_or_titlebar_focus(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    let file = root.join("a.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.clone(), window, cx));
    wait_long(cx, "the file to be open", |cx| edited_at(&workbench, cx).is_some_and(|(path, _)| path == file));
    let close = if cfg!(target_os = "macos") { "cmd-w" } else { "ctrl-f4" };
    for body in [true, false] {
        run_action(cx, "vcs.showDiff", Some(json!(text_of(&file))));
        let viewer = loaded_viewer(&workbench, cx);
        let dialog = diff_dialog(&workbench, cx).expect("the diff is shown");
        cx.update(|window, cx| {
            let focus = if body { viewer.focus_handle(cx) } else { dialog.focus_handle(cx) };
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        cx.simulate_keystrokes(close);
        cx.run_until_parked();
        assert!(diff_dialog(&workbench, cx).is_none(), "the diff window closes (viewer focused: {body})");
        let open = edited_at(&workbench, cx).map(|(path, _)| path);
        assert_eq!(open.as_deref(), Some(file.as_path()), "the editor tab stays (viewer focused: {body})");
    }
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

// src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › can start an embedded comparison side by side
// without overwriting saved viewer preferences
/// A diff that asks for the side-by-side viewer opens side by side under a stored unified preference, and the
/// stored preference is left as it was.
#[gpui_kit::test]
async fn can_start_an_embedded_comparison_side_by_side_without_overwriting_saved_viewer_preferences(
    cx: &mut TestAppContext,
) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(DIFF_OPTIONS_KEY, r#"{"mode":"unified"}"#.to_owned()));
    let data = json!({
        "path": text_of(&root.join("a.txt")),
        "left": { "type": "rev", "rev": "HEAD" },
        "right": { "type": "worktree" },
        "mode": "split",
    });
    run_action(cx, "vcs.openDiff", Some(data));
    let viewer = loaded_viewer(&workbench, cx);
    assert_eq!(viewer.read_with(cx, |viewer, _| viewer.options().mode), DiffMode::Split);
    cx.run_until_parked();
    assert!(cx.debug_bounds("diff-pane-left").is_some() && cx.debug_bounds("diff-pane-right").is_some());
    let stored = cx.update(|_, cx| load_diff_options(cx.global::<UiStorage>().get(DIFF_OPTIONS_KEY)).mode);
    assert_eq!(stored, DiffMode::Unified, "the stored preference is unchanged");
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › jumps to the working copy of a file shown at a
// revision
/// A diff of two revisions of a file that is in the working tree offers Jump to Source.
#[gpui_kit::test]
async fn jumps_to_the_working_copy_of_a_file_shown_at_a_revision(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = revision_project(cx);
    let viewer = open_revision_diff(&workbench, &root, cx);
    assert!(offers_jump_to_source(&viewer, cx), "the working copy is a file to jump to");
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › does not offer editor maximization in an embedded diff
/// The diff window's toolbar has no Maximize Editor button.
#[gpui_kit::test]
async fn does_not_offer_editor_maximization_in_an_embedded_diff(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = changed_project(cx);
    run_action(cx, "vcs.showDiff", Some(json!(text_of(&root.join("a.txt")))));
    let viewer = loaded_viewer(&workbench, cx);
    let labels: Vec<&'static str> = viewer.read_with(cx, |viewer, cx| {
        viewer.toolbar(cx).iter().filter_map(ToolbarItem::button).map(|button| button.label).collect()
    });
    assert!(labels.contains(&"Next Difference"), "{labels:?}");
    assert!(!labels.iter().any(|label| *label == "Maximize Editor" || *label == "Restore Size"), "{labels:?}");
}

/// Puts the caret on `line` of `side` and runs Jump to Source; where the editor then is.
fn jump_from(
    workbench: &Entity<Workbench>,
    viewer: &Entity<DiffView>,
    side: Side,
    line: usize,
    cx: &mut VisualTestContext,
) -> (PathBuf, u32) {
    viewer.update(cx, |viewer, cx| viewer.reveal_line(side, line, cx));
    run_action(cx, "diff.jumpToSource", None);
    wait_long(cx, "the working copy to be opened", |cx| {
        diff_dialog(workbench, cx).is_none() && edited_at(workbench, cx).is_some()
    });
    edited_at(workbench, cx).expect("an editor is open")
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Jump to Source of a revision diff › maps a line of the commit to
// the working copy
/// Jump to Source from a line of the commit's side opens the working copy at that line as it is there now.
#[gpui_kit::test]
async fn maps_a_line_of_the_commit_to_the_working_copy(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = revision_project(cx);
    let viewer = open_revision_diff(&workbench, &root, cx);
    assert!(offers_jump_to_source(&viewer, cx), "the working copy is a file to jump to");
    // Line 2 of the commit, "Y", is line 3 of the working copy.
    assert_eq!(jump_from(&workbench, &viewer, Side::B, 2, cx), (root.join("a.txt"), 3));
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Jump to Source of a revision diff › maps a line of the parent
// through the diff to the working copy
/// Jump to Source from a line of the parent's side follows it through the diff into the commit, then into the
/// working copy.
#[gpui_kit::test]
async fn maps_a_line_of_the_parent_through_the_diff_to_the_working_copy(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root) = revision_project(cx);
    let viewer = open_revision_diff(&workbench, &root, cx);
    assert!(offers_jump_to_source(&viewer, cx), "the working copy is a file to jump to");
    // Line 2 of the parent, "c", is line 4 of the commit and line 5 of the working copy.
    assert_eq!(jump_from(&workbench, &viewer, Side::A, 2, cx), (root.join("a.txt"), 5));
}

// src/features/vcs/__tests__/diffToolbar.test.tsx › Jump to Source of a revision diff › jumps on ⌘↓ in the IntelliJ
// macOS keymap only while a diff pane has focus
/// In the macOS keymap ⌘↓ is Jump to Source while the diff window has the keyboard, and only then.
#[gpui_kit::test]
async fn jumps_on_cmd_down_in_the_intellij_macos_keymap_only_while_a_diff_pane_has_focus(cx: &mut TestAppContext) {
    if !cfg!(target_os = "macos") {
        return;
    }
    let (workbench, cx, _folders, root) = revision_project(cx);
    let viewer = open_revision_diff(&workbench, &root, cx);
    assert!(offers_jump_to_source(&viewer, cx), "the working copy is a file to jump to");
    viewer.update(cx, |viewer, cx| viewer.reveal_line(Side::B, 2, cx));
    assert!(actions_run_by("cmd-down", cx).contains(&"diff.jumpToSource".to_owned()));
    wait_long(cx, "the working copy to be opened", |cx| {
        edited_at(&workbench, cx).is_some_and(|at| at == (root.join("a.txt"), 3))
    });
    assert!(diff_dialog(&workbench, cx).is_none());
    // In the editor the key is not Jump to Source.
    assert!(!actions_run_by("cmd-down", cx).contains(&"diff.jumpToSource".to_owned()));
}
