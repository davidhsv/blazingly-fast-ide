//! Editor › General › Editor Tabs, changed through File › Settings… and Apply: the tab strip of a
//! project window follows each setting as the user sees it (titles, marks, icons, tooltips, rows,
//! order, the tabs the limit closes).

use super::proofs19_settings_misc::bring_into_view;
use super::*;
use ide_workspace::settings::SettingsScope;
use serde_json::Value;
use std::path::Path;

const PAGE: &str = "editor.general.editorTabs";

/// A trusted project with the files the tab strip is read on, open in a window.
fn tabs_project(cx: &mut TestAppContext) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &mut VisualTestContext) {
    let files: Vec<(String, String)> = ["a", "b", "c", "d", "e"]
        .iter()
        .map(|name| (format!("{name}.txt"), format!("{name}\n")))
        .chain([("x/mod.rs".to_owned(), "// x\n".to_owned()), ("y/mod.rs".to_owned(), "// y\n".to_owned())])
        .collect();
    let files: Vec<(&str, &str)> = files.iter().map(|(path, text)| (path.as_str(), text.as_str())).collect();
    let (folder, root, workbench, cx) = trusted_project(cx, &files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

fn open(workbench: &Entity<Workbench>, path: &Path, cx: &mut VisualTestContext) {
    let path = path.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
}

/// What the strip of the active pane draws, tab by tab.
fn labels(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<TabLabel> {
    workbench.read_with(cx, |w, cx| {
        let Some(tabs) = w.tabs() else { return Vec::new() };
        w.tab_labels_of(&tabs.active_pane, cx)
    })
}

/// The titles the strip of the active pane draws, with the hint after an em dash.
fn names(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    labels(workbench, cx)
        .into_iter()
        .map(|label| match label.hint {
            Some(hint) => format!("{} — {hint}", label.name),
            None => label.name,
        })
        .collect()
}

fn active(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    labels(workbench, cx).into_iter().find(|label| label.active).map(|label| label.name)
}

/// The files whose documents the backend holds open for the window, by name, sorted.
fn open_documents(cx: &mut VisualTestContext) -> Vec<String> {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let Ok(project) = host.project("w1") else { return Vec::new() };
    let mut names: Vec<String> = project
        .documents
        .open_documents()
        .into_iter()
        .filter_map(|doc| project.documents.identity(doc).ok())
        .filter_map(|(path, _)| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// File › Settings… on Editor Tabs, `change` there, then Apply and OK.
fn change_tab_settings(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    change: impl FnOnce(&mut VisualTestContext),
) {
    run_action(cx, "file.settings", Some(Value::from(PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(PAGE));
    change(cx);
    click_on(cx, "settings-apply");
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(workbench, cx).is_none());
}

fn toggle(cx: &mut VisualTestContext, at: &'static str) {
    bring_into_view(cx, at);
    click_on(cx, at);
}

fn set_tab_limit(cx: &mut VisualTestContext, limit: &str) {
    let at = "settings-number-editor.tabs.tabLimit";
    bring_into_view(cx, at);
    click_on(cx, at);
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(limit);
    cx.run_until_parked();
}

fn setting(cx: &mut VisualTestContext, key: &str) -> Option<Value> {
    cx.update(|_, cx| setting_of(&crate::platform::settings::effective("w1", cx), key))
}

/// Types `typed` at the start of the active editor: its document is modified and stays so.
fn modify_active(workbench: &Entity<Workbench>, typed: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let editor = workbench.read(cx).editor().cloned().expect("an editor");
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_input(typed);
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).is_dirty())));
}

fn autosave_off(cx: &mut VisualTestContext) {
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx))
        .expect("the setting is stored");
}

/// With the tab limit set to 3, opening five files keeps the three most recently used: the two
/// least recently used close, their documents are released, and Reopen Closed Tab brings back the
/// last one closed.
#[gpui_kit::test]
async fn a_tab_limit_of_three_closes_the_least_recently_used_tabs_and_reopen_brings_them_back(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    change_tab_settings(&workbench, cx, |cx| set_tab_limit(cx, "3"));
    assert_eq!(setting(cx, "editor.tabs.tabLimit"), Some(Value::from(3)));

    for name in ["a", "b", "c", "d", "e"] {
        open(&workbench, &root.join(format!("{name}.txt")), cx);
    }
    assert_eq!(names(&workbench, cx), ["c.txt", "d.txt", "e.txt"]);
    assert_eq!(active(&workbench, cx).as_deref(), Some("e.txt"));
    settle(cx, |cx| open_documents(cx) == ["c.txt", "d.txt", "e.txt"]);

    run_action(cx, "editor.reopenClosedTab", None);
    settle(cx, |cx| active(&workbench, cx).as_deref() == Some("b.txt"));
    assert!(names(&workbench, cx).contains(&"b.txt".to_owned()));
    close_backend_project(cx);
}

/// "Close non-modified files first": past the limit, an unmodified tab closes before an older
/// modified one, whose edit stays in its tab.
#[gpui_kit::test]
async fn past_the_limit_close_non_modified_files_first_keeps_the_modified_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    autosave_off(cx);
    change_tab_settings(&workbench, cx, |cx| {
        set_tab_limit(cx, "2");
        toggle(cx, "settings-radio-editor.tabs.closingPolicy-unchanged");
    });
    assert_eq!(setting(cx, "editor.tabs.closingPolicy"), Some(Value::from("unchanged")));

    open(&workbench, &root.join("a.txt"), cx);
    modify_active(&workbench, "edited ", cx);
    open(&workbench, &root.join("b.txt"), cx);
    open(&workbench, &root.join("c.txt"), cx);
    assert_eq!(names(&workbench, cx), ["a.txt", "c.txt"]);
    assert!(workbench.read_with(cx, |w, _| w.app_dialog().is_none()), "nothing asks about the modified file");
    let kept = std::fs::read_to_string(root.join("a.txt")).expect("the file reads");
    assert_eq!(kept, "a\n", "the modified file is not written");
    close_backend_project(cx);
}

/// A new tab opens after the active one; with "Open new tabs at the end" it opens last.
#[gpui_kit::test]
async fn open_new_tabs_at_the_end_puts_a_new_tab_last_instead_of_after_the_active_one(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    for name in ["a", "b", "c"] {
        open(&workbench, &root.join(format!("{name}.txt")), cx);
    }
    open(&workbench, &root.join("a.txt"), cx);
    open(&workbench, &root.join("d.txt"), cx);
    assert_eq!(names(&workbench, cx), ["a.txt", "d.txt", "b.txt", "c.txt"], "after the active tab by default");

    change_tab_settings(&workbench, cx, |cx| toggle(cx, "settings-toggle-editor.tabs.openAtEnd"));
    assert_eq!(setting(cx, "editor.tabs.openAtEnd"), Some(Value::from(true)));
    open(&workbench, &root.join("a.txt"), cx);
    open(&workbench, &root.join("e.txt"), cx);
    assert_eq!(names(&workbench, cx), ["a.txt", "d.txt", "b.txt", "c.txt", "e.txt"]);
    close_backend_project(cx);
}

/// The titles of open tabs follow "Show file extension", "Show directory for non-unique file
/// names" and "Mark modified (*)" as soon as they are applied.
#[gpui_kit::test]
async fn the_titles_of_open_tabs_follow_the_extension_directory_and_modified_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    autosave_off(cx);
    open(&workbench, &root.join("x/mod.rs"), cx);
    open(&workbench, &root.join("a.txt"), cx);
    modify_active(&workbench, "edited ", cx);
    open(&workbench, &root.join("y/mod.rs"), cx);
    assert_eq!(names(&workbench, cx), ["mod.rs — x", "a.txt", "mod.rs — y"], "the defaults");

    change_tab_settings(&workbench, cx, |cx| {
        toggle(cx, "settings-toggle-editor.tabs.showExtension");
        toggle(cx, "settings-toggle-editor.tabs.showDirectoryForNonUnique");
        toggle(cx, "settings-toggle-editor.tabs.markModified");
    });
    assert_eq!(names(&workbench, cx), ["mod", "*a", "mod"]);
    close_backend_project(cx);
}

/// "Show file icon" off draws tabs without their icons; "Show full path on mouse hover" shows the
/// file's path while the pointer rests on a tab, and nothing once it is off.
#[gpui_kit::test]
async fn tabs_follow_the_file_icon_and_full_path_on_hover_settings(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    let path = root.join("x/mod.rs");
    open(&workbench, &path, cx);
    let label = labels(&workbench, cx).pop().expect("a tab");
    assert!(label.icon.is_some(), "an icon by default");
    assert_eq!(label.tooltip.as_deref(), Some(&*path.to_string_lossy()), "the full path by default");

    change_tab_settings(&workbench, cx, |cx| {
        toggle(cx, "settings-toggle-editor.tabs.showFileIcon");
        toggle(cx, "settings-toggle-editor.tabs.showFullPathOnHover");
    });
    let label = labels(&workbench, cx).pop().expect("a tab");
    assert_eq!((label.icon, label.tooltip), (None, None));
    close_backend_project(cx);
}

/// "Show pinned tabs in a separate row": the pinned tabs take a row above the others while the pane
/// has both; one row again once every tab is pinned or the setting is off.
#[gpui_kit::test]
async fn pinned_tabs_take_a_row_of_their_own_when_the_setting_asks_for_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    open(&workbench, &root.join("a.txt"), cx);
    run_action(cx, "editor.pinTab", None);
    open(&workbench, &root.join("b.txt"), cx);
    open(&workbench, &root.join("c.txt"), cx);
    let rows = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, cx| {
            let tabs = w.tabs().expect("a project is open");
            w.tab_rows_of(&tabs.active_pane, cx)
        })
    };
    assert_eq!(rows(cx), [3], "one row by default");

    change_tab_settings(&workbench, cx, |cx| toggle(cx, "settings-toggle-editor.tabs.pinnedSeparateRow"));
    assert_eq!(rows(cx), [1, 2]);
    run_action(cx, "editor.pinTab", None);
    assert_eq!(rows(cx), [2, 1]);
    open(&workbench, &root.join("b.txt"), cx);
    run_action(cx, "editor.pinTab", None);
    assert_eq!(rows(cx), [3], "every tab pinned: one row");
    close_backend_project(cx);
}

/// The text of the active editor.
fn active_text(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    workbench.read_with(cx, |w, cx| w.editor().map(|editor| editor.read(cx).text()))
}

/// With the default saving settings, a modified text tab the limit closes (least recently used) is
/// written as it closes: nothing asks, the tab goes, and reopening the file shows the edit.
#[gpui_kit::test]
async fn a_modified_tab_the_limit_closes_is_written_with_autosave_on(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    change_tab_settings(&workbench, cx, |cx| set_tab_limit(cx, "1"));
    assert_eq!(setting(cx, "files.autoSave"), Some(Value::from(true)), "autosave is on by default");
    assert_eq!(setting(cx, "editor.tabs.closingPolicy"), Some(Value::from("lru")));

    open(&workbench, &root.join("a.txt"), cx);
    modify_active(&workbench, "edited ", cx);
    open(&workbench, &root.join("b.txt"), cx);
    assert_eq!(names(&workbench, cx), ["b.txt"], "the limit closed the modified tab");
    let disk = |root: &Path| std::fs::read_to_string(root.join("a.txt")).expect("the file reads");
    settle(cx, |_| disk(&root) == "edited a\n");
    assert!(cx.pending_prompt().is_none(), "nothing asks");
    assert!(workbench.read_with(cx, |w, _| w.app_dialog().is_none()), "nothing asks");

    open(&workbench, &root.join("a.txt"), cx);
    assert_eq!(active_text(&workbench, cx).as_deref(), Some("edited a\n"), "reopening shows the edit");
    close_backend_project(cx);
}

/// With "Save files when switching to a different application or a built-in terminal" off (the
/// setting that writes a closing tab), a modified text tab the limit closes asks Save, Don't Save or
/// Cancel; Cancel brings the tab back with its edit, and the file on disk is untouched.
#[gpui_kit::test]
async fn a_modified_tab_the_limit_closes_with_autosave_off_asks_and_cancel_keeps_its_edit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = tabs_project(cx);
    run_action(cx, "file.settings", Some(Value::from("general")));
    settle(cx, |cx| settings_dialog_of(&workbench, cx).is_some());
    toggle(cx, "settings-toggle-files.autoSave");
    click_on(cx, "settings-apply");
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(setting(cx, "files.autoSave"), Some(Value::from(false)));
    change_tab_settings(&workbench, cx, |cx| set_tab_limit(cx, "1"));

    open(&workbench, &root.join("a.txt"), cx);
    modify_active(&workbench, "edited ", cx);
    let b = root.join("b.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(b, window, cx));
    cx.run_until_parked();
    let (title, _) = cx.pending_prompt().expect("closing the modified tab asks");
    assert_eq!(title, "Unsaved Changes");
    assert_eq!(names(&workbench, cx), ["b.txt"], "the tab is closed while the question waits");
    cx.simulate_prompt_answer("Cancel");
    settle(cx, |cx| names(&workbench, cx).contains(&"a.txt".to_owned()));
    let disk = std::fs::read_to_string(root.join("a.txt")).expect("the file reads");
    assert_eq!(disk, "a\n", "nothing is written");

    open(&workbench, &root.join("a.txt"), cx);
    assert_eq!(active_text(&workbench, cx).as_deref(), Some("edited a\n"), "the tab keeps the edit");
    close_backend_project(cx);
}
