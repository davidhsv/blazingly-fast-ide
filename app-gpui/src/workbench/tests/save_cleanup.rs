//! What a save writes: Editor › General › Save Files (trailing spaces on modified or all lines, the
//! caret line kept, trailing blank lines, the final line break) and the file's `.editorconfig`, applied
//! by File › Save All and by autosave to the documents the user edited, never to the others; and
//! System Settings › Save files if the IDE is idle.

use super::*;
use ide_workspace::settings::SettingsScope;
use serde_json::Value;
use std::path::Path;
use std::time::SystemTime;

/// Three lines, the first with trailing spaces, the last without a line break.
const THREE_LINES: &str = "keep   \nline\nlast";
/// Where `line` ends in [`THREE_LINES`].
const LINE_END: u32 = 12;
/// What the user types there.
const TYPED: &str = " x   ";
/// The end of [`THREE_LINES`] once [`TYPED`] is in.
const TEXT_END: u32 = 22;

/// A trusted project of `files` with autosave off, so that only the save a test runs writes.
fn project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    autosave(false, cx);
    (folder, root, workbench, cx)
}

fn autosave(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &on.into(), cx))
        .expect("the setting is stored");
}

/// File › Settings… on `page`: `clicks` pressed in order, then Apply and OK.
fn apply_in_settings(workbench: &Entity<Workbench>, page: &str, clicks: &[&'static str], cx: &mut VisualTestContext) {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    for selector in clicks {
        click_in_view(cx, selector);
    }
    assert!(dialog.read_with(cx, |view, cx| view.can_apply(cx)), "the changes wait for Apply");
    click_on(cx, "settings-apply");
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(workbench, cx).is_none());
}

/// Clicks the Settings control `selector`, after scrolling the page with the wheel until the control
/// is above the dialog's buttons: the save options are at the bottom of their pages.
fn click_in_view(cx: &mut VisualTestContext, selector: &'static str) {
    use gpui_kit::{ScrollDelta, ScrollEvent, point, px};
    cx.run_until_parked();
    for _ in 0..30 {
        let control = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        let buttons = cx.debug_bounds("settings-apply").expect("the dialog's buttons are drawn");
        if control.bottom() < buttons.top() - px(12.0) {
            break;
        }
        let position = point(control.center().x, buttons.top() - px(60.0));
        cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
            ..Default::default()
        });
        cx.run_until_parked();
    }
    click_on(cx, selector);
}

/// Opens `file` in a tab with the keyboard in its editor.
fn open(workbench: &Entity<Workbench>, file: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = file.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    focus(&editor, cx);
    editor
}

fn focus(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
}

/// Types `text` at `offset` of `editor`, which keeps the keyboard.
fn type_at(editor: &Entity<Editor>, offset: u32, text: &str, cx: &mut VisualTestContext) {
    focus(editor, cx);
    editor.update(cx, |editor, cx| editor.select(offset, offset, cx));
    cx.simulate_input(text);
    cx.run_until_parked();
}

fn caret_to(editor: &Entity<Editor>, offset: u32, cx: &mut VisualTestContext) {
    editor.update(cx, |editor, cx| editor.select(offset, offset, cx));
}

/// File › Save All, once every tab is saved.
fn save_all(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    run_action(cx, "file.save", None);
    wait_long(cx, "every tab to be saved", |cx| workbench.read_with(cx, |w, cx| w.unsaved_count(cx)) == 0);
}

fn bytes(path: &Path) -> String {
    std::fs::read_to_string(path).expect("the file is readable")
}

/// Dates `path` back, so that a write shows in its modification time; returns that time.
fn date_back(path: &Path) -> SystemTime {
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::options().write(true).open(path).and_then(|file| file.set_modified(old)).expect("the time is set");
    old
}

fn modified_at(path: &Path) -> SystemTime {
    std::fs::metadata(path).and_then(|meta| meta.modified()).expect("the file has a modification time")
}

/// With the settings of a new home, a save trims the trailing spaces of the lines the user edited
/// (not of the others, nor of a file only opened), adds the missing final line break, and leaves the
/// caret where it was; one Undo takes the cleanup back, a second the typing.
#[gpui_kit::test]
async fn trims_modified_lines_and_adds_the_final_newline_on_save(cx: &mut TestAppContext) {
    let home = start(cx);
    let untouched = "untouched   \n\n\n\nend  ";
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES), ("b.txt", untouched)]);
    let other = root.join("b.txt");
    let was = date_back(&other);
    open(&workbench, &other, cx);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    assert!(!global_settings_file(&home).contains("railing"), "{}", global_settings_file(&home));

    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position()), (2, 4), "the caret stays before the line break");
    assert_eq!(bytes(&other), untouched, "a file only opened is not cleaned");
    assert_eq!(modified_at(&other), was, "nor written");

    run_action(cx, "edit.undo", None);
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "keep   \nline x   \nlast");
    assert!(editor.read_with(cx, |e, _| e.is_dirty()));
    run_action(cx, "edit.undo", None);
    assert_eq!(editor.read_with(cx, |e, _| e.text()), THREE_LINES);
}

/// With Remove trailing spaces and Ensure a line break unticked in Settings, a save writes the user's
/// edit and nothing else: the typed trailing spaces stay and the file still ends without a line break.
#[gpui_kit::test]
async fn with_both_options_off_a_save_writes_only_the_users_edit(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    apply_in_settings(
        &workbench,
        "editor.general",
        &["settings-toggle-files.trimTrailingWhitespace", "settings-toggle-files.ensureFinalNewline"],
        cx,
    );
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \nline x   \nlast");
}

/// Remove trailing spaces on All lines: the lines the user did not touch lose theirs as well.
#[gpui_kit::test]
async fn all_lines_scope_strips_the_trailing_spaces_of_every_line(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    apply_in_settings(
        &workbench,
        "editor.general",
        &["settings-select-editor.general.stripTrailingScope", "settings-choice-editor.general.stripTrailingScope-all"],
        cx,
    );
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep\nline x\nlast\n");
}

/// Keep trailing spaces on caret line: the line under the caret keeps the spaces typed at its end; with
/// the option unticked they go too, and the caret moves back to where the line now ends.
#[gpui_kit::test]
async fn the_caret_line_keeps_its_trailing_spaces_unless_the_option_is_off(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \nline x   \nlast\n");

    apply_in_settings(&workbench, "editor.general", &["settings-toggle-editor.general.keepTrailingOnCaretLine"], cx);
    type_at(&editor, LINE_END + TYPED.len() as u32, "  ", cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position()), (1, 6));
}

/// Remove trailing blank lines: off, the blank lines at the end of an edited file stay; on, exactly
/// one line break is left after the last line that has text.
#[gpui_kit::test]
async fn trailing_blank_lines_go_only_when_the_option_is_on(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n\n  \n\n")]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, 0, "b", cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "ba\n\n  \n\n");

    apply_in_settings(&workbench, "editor.general", &["settings-toggle-editor.general.removeTrailingBlankLines"], cx);
    type_at(&editor, 0, "c", cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "cba\n");
}

/// A file's `.editorconfig` saying not to trim nor add a final line break wins over the settings, while
/// EditorConfig support is on; with the support off the settings apply again.
#[gpui_kit::test]
async fn editorconfig_overrides_the_save_options_while_its_support_is_on(cx: &mut TestAppContext) {
    let _home = start(cx);
    let editorconfig = "root = true\n\n[*.txt]\ntrim_trailing_whitespace = false\ninsert_final_newline = false\n";
    let (_folder, root, workbench, cx) = project(cx, &[(".editorconfig", editorconfig), ("a.txt", THREE_LINES)]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \nline x   \nlast");

    cx.update(|_, cx| {
        crate::platform::settings::set("w1", SettingsScope::Global, "editor.codeStyle.editorconfig", &false.into(), cx)
    })
    .expect("the setting is stored");
    type_at(&editor, 0, "z", cx);
    caret_to(&editor, TEXT_END + 1, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "zkeep\nline x   \nlast\n");
}

/// A file with Windows line breaks keeps them: the trimmed line and the added final line break end in
/// CR LF like the others.
#[gpui_kit::test]
async fn a_crlf_file_keeps_its_line_breaks_when_cleaned_up(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "keep   \r\nline\r\nlast")]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "keep   \r\nline x\r\nlast\r\n");
}

/// Autosave cleans up as Save All does: leaving the edited tab for another writes it trimmed and with
/// its final line break, and the tab only looked at is not written.
#[gpui_kit::test]
async fn autosave_cleans_up_the_tab_it_saves(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES), ("b.txt", "other  ")]);
    autosave(true, cx);
    let other = root.join("b.txt");
    let was = date_back(&other);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    open(&workbench, &other, cx);
    wait_long(cx, "the edited tab to be saved", |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
    assert_eq!((bytes(&other), modified_at(&other)), ("other  ".to_owned(), was));
}

/// A second editor of the same document: the lines edited in it are trimmed by a Save All of the tab,
/// and the line under its caret keeps its trailing spaces.
#[gpui_kit::test]
async fn edits_and_carets_of_a_second_editor_of_the_document_count(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "one  \ntwo\nthree\n")]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    let opened = cx.update(|_, cx| {
        let path = file.clone();
        cx.global::<Backend>().with_host(move |host| async move { ide_host::ops::doc::open(&host, "w1", path).await })
    });
    let info = opened.await.expect("the task completes").expect("the document opens again");
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let sibling = cx.new(|cx| Editor::new(host, "w1", &info, cx).expect("a second editor"));
    // In the second editor: spaces after "two" (trimmed: no caret there) and after "three", where its
    // caret stays.
    sibling.update(cx, |e, cx| e.insert(Some(9..9), "  ", cx));
    sibling.update(cx, |e, cx| e.insert(Some(17..17), "   ", cx));
    caret_to(&editor, 0, cx);
    // The tab learns that the document is unsaved from the backend's report.
    settle(cx, |cx| editor.read_with(cx, |e, _| e.is_dirty()));
    save_all(&workbench, cx);
    assert_eq!(bytes(&file), "one  \ntwo\nthree   \n");
    drop(sibling);
}

/// System Settings › Save files if the IDE is idle for, ticked or unticked in Settings, then OK.
fn set_idle_saving(workbench: &Entity<Workbench>, seconds: Option<&str>, cx: &mut VisualTestContext) {
    run_action(cx, "file.settings", Some(Value::from("general")));
    click_in_view(cx, "settings-toggle-general.saveOnIdle");
    if let Some(seconds) = seconds {
        click_in_view(cx, "settings-number-general.idleSeconds");
        cx.simulate_keystrokes("cmd-a");
        cx.simulate_input(seconds);
    }
    click_on(cx, "settings-apply");
    click_on(cx, "settings-ok");
    settle(cx, |cx| settings_dialog_of(workbench, cx).is_none());
}

fn idle_save_pending(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, _| w.idle_save_pending())
}

/// Save files if the IDE is idle for 5 seconds, ticked in Settings: no countdown runs while nothing is
/// unsaved; an edit starts one and is saved, cleaned up, once five seconds pass without input, not
/// before, after which no countdown is left. Unticked again, the running countdown stops and nothing
/// is saved however long the window stays idle.
#[gpui_kit::test]
async fn idle_saving_saves_the_edited_files_after_the_seconds_without_input(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    set_idle_saving(&workbench, Some("5"), cx);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    assert!(!idle_save_pending(&workbench, cx), "nothing unsaved, no countdown");
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, TEXT_END, cx);
    assert!(idle_save_pending(&workbench, cx), "the edit starts the countdown");

    cx.executor().advance_clock(Duration::from_millis(4900));
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_dirty()), "not saved before the five seconds");
    assert_eq!(bytes(&file), THREE_LINES);
    cx.executor().advance_clock(Duration::from_millis(200));
    settle(cx, |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
    assert!(!idle_save_pending(&workbench, cx), "saved, no countdown is left");

    type_at(&editor, 0, "z", cx);
    assert!(idle_save_pending(&workbench, cx));
    set_idle_saving(&workbench, None, cx);
    assert!(!idle_save_pending(&workbench, cx), "unticked, the countdown stops");
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_dirty()), "nothing saves it");
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
}

/// With idle saving off, as a new home has it, an edit starts no countdown and stays unsaved however
/// long the window is idle.
#[gpui_kit::test]
async fn without_idle_saving_an_edit_starts_no_countdown(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    assert!(!idle_save_pending(&workbench, cx));
    cx.executor().advance_clock(Duration::from_secs(120));
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&file), THREE_LINES);
}

/// An idle save that fails is told once as "Could not save", the tab stays unsaved, and no countdown
/// is left to try again until the next input.
#[cfg(unix)]
#[gpui_kit::test]
async fn a_failed_idle_save_is_told_once_and_not_retried_without_input(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(home.path().join("config")).expect("the config folder is made");
    std::fs::write(home.path().join("config/settings.toml"), "[general]\nsaveOnIdle = true\nidleSeconds = 2\n")
        .expect("the settings are written");
    let _home = start_in(home, cx);
    let (_folder, root, workbench, cx) = project(cx, &[("locked/a.txt", "a\n")]);
    let file = root.join("locked/a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, 0, "b", cx);
    let lock = |mode: u32, path: &Path| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permissions change")
    };
    lock(0o444, &file);
    lock(0o555, &root.join("locked"));
    assert!(idle_save_pending(&workbench, cx));

    cx.executor().advance_clock(Duration::from_millis(2100));
    wait_for_notice(&workbench, "Could not save", cx);
    assert!(!idle_save_pending(&workbench, cx), "no countdown is left after the failure");
    cx.executor().advance_clock(Duration::from_secs(60));
    cx.run_until_parked();
    let told = notification_titles(&workbench, cx).iter().filter(|title| *title == "Could not save").count();
    assert_eq!(told, 1, "{:?}", notification_titles(&workbench, cx));
    assert!(editor.read_with(cx, |e, _| e.is_dirty()), "the tab stays unsaved");
    assert_eq!(bytes(&file), "a\n");
    lock(0o755, &root.join("locked"));
    lock(0o644, &file);
}

/// A home whose settings file turns idle saving on after `seconds`.
fn home_with_idle_saving(seconds: u32) -> tempfile::TempDir {
    let home = tempfile::tempdir().expect("a temporary home");
    std::fs::create_dir_all(home.path().join("config")).expect("the config folder is made");
    let settings = format!("[general]\nsaveOnIdle = true\nidleSeconds = {seconds}\n");
    std::fs::write(home.path().join("config/settings.toml"), settings).expect("the settings are written");
    home
}

/// A key pressed while the countdown runs starts it over: the edit is saved five seconds after the
/// key, not after the edit.
#[gpui_kit::test]
async fn a_key_pressed_while_idle_saving_counts_down_starts_the_countdown_over(cx: &mut TestAppContext) {
    let _home = start_in(home_with_idle_saving(5), cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    let file = root.join("a.txt");
    let editor = open(&workbench, &file, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    cx.simulate_keystrokes("down");
    assert_eq!(editor.read_with(cx, |e, _| e.caret_position()), (2, 4), "the key reached the editor");
    cx.executor().advance_clock(Duration::from_secs(4));
    cx.run_until_parked();
    assert!(editor.read_with(cx, |e, _| e.is_dirty()), "eight seconds after the edit, four after the key");
    cx.executor().advance_clock(Duration::from_millis(1100));
    settle(cx, |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&file), "keep   \nline x\nlast\n");
}

/// File › Save All while the countdown runs: once nothing is unsaved, no countdown is left.
#[gpui_kit::test]
async fn saving_everything_stops_the_idle_countdown(cx: &mut TestAppContext) {
    let _home = start_in(home_with_idle_saving(30), cx);
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", THREE_LINES)]);
    let editor = open(&workbench, &root.join("a.txt"), cx);
    type_at(&editor, LINE_END, TYPED, cx);
    assert!(idle_save_pending(&workbench, cx));
    save_all(&workbench, cx);
    settle(cx, |cx| !idle_save_pending(&workbench, cx));
}

/// A file with mixed line breaks is written with the ones most of its lines have, as any save of it
/// is; the cleanup adds nothing to that: with the options off only the edit differs.
#[gpui_kit::test]
async fn a_file_with_mixed_line_breaks_gets_only_the_cleanup_the_options_ask_for(cx: &mut TestAppContext) {
    let _home = start(cx);
    let mixed = "keep   \r\nline\r\nlast\nend";
    let (_folder, root, workbench, cx) = project(cx, &[("a.txt", mixed), ("b.txt", mixed)]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let editor = open(&workbench, &a, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, 0, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&a), "keep   \r\nline x\r\nlast\r\nend\r\n");

    apply_in_settings(
        &workbench,
        "editor.general",
        &["settings-toggle-files.trimTrailingWhitespace", "settings-toggle-files.ensureFinalNewline"],
        cx,
    );
    let editor = open(&workbench, &b, cx);
    type_at(&editor, LINE_END, TYPED, cx);
    caret_to(&editor, 0, cx);
    save_all(&workbench, cx);
    assert_eq!(bytes(&b), "keep   \r\nline x   \r\nlast\r\nend");
}

/// Find in Files on `query` with its preview on the first result of `file`: the preview's editor.
fn previewed(workbench: &Entity<Workbench>, query: &str, file: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    use crate::find::popup_view::SEARCH_DEBOUNCE;
    use crate::find::session::FindStatus;
    run_action(cx, "find.inPath", Some(serde_json::json!({ "query": query })));
    let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
    wait_long(cx, "the search to finish", |cx| {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale())
    });
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    let path = text_of(file);
    wait_long(cx, "the preview of the file", |cx| {
        preview.read_with(cx, |preview, _| preview.path() == Some(path.as_str()) && preview.editor().is_some())
    });
    preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("the preview has an editor")
}

fn command(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
}

/// ⌘S in the preview of Find in Files writes the edit without the cleanup while no tab shows the file;
/// with a tab of it open, the save is cleaned up through the tab.
#[gpui_kit::test]
async fn a_preview_save_is_cleaned_up_only_through_a_tab_of_the_file(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        project(cx, &[("a.rs", "fn order() {}\nlast"), ("b.rs", "fn other() {}\nlast")]);
    let (a, b) = (root.join("a.rs"), root.join("b.rs"));
    let editor = previewed(&workbench, "order", &a, cx);
    focus(&editor, cx);
    cx.simulate_input("x ");
    cx.simulate_keystrokes(&command("s"));
    wait_long(cx, "the preview's save", |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&a), "x fn order() {}\nlast", "no tab: written as edited");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    let tab = open(&workbench, &b, cx);
    caret_to(&tab, 0, cx);
    let editor = previewed(&workbench, "other", &b, cx);
    focus(&editor, cx);
    cx.simulate_input("x ");
    cx.simulate_keystrokes(&command("s"));
    wait_long(cx, "the preview's save", |cx| !editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(bytes(&b), "x fn other() {}\nlast\n", "cleaned up through the tab");
}
