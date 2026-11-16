//! Editor › Vim opened through File › Settings… (and Tools › Vim Settings…) on its page and changed
//! with the mouse. Enable Vim emulation and each key's handler are stored at once in the UI storage
//! under the Vim settings' key, as the page says: nothing is staged, so Apply stays off, no Reset is
//! offered, the page is not marked modified, and Cancel leaves the changes as they were made. The
//! conflicts table lists the IDE actions the keymap in effect binds to each key Vim uses. The rc file
//! is read from the test's own home folder, with the files it sources, once, and again on Reload.
//!
//! Nothing here reads or writes anything under the user's home: the backend's folders and the home
//! the rc file is looked for in are a temporary folder.

use super::*;
use crate::features_a::vim::conflicts::{VIM_KEYS, conflict_rows};
use crate::features_a::vim::settings::VIM_SETTINGS_KEY;
use crate::platform::actions::ActionRegistry;
use crate::platform::keymap::{format_binding, is_mac_host};
use crate::settings_view::dialog::SettingsDialogView;
use crate::settings_view::pages::editor_vim::{self, VimRcStatus, VimSettingsView};
use gpui_kit::AnyView;
use serde_json::{Value, json};
use std::path::Path;

fn window(cx: &mut TestAppContext) -> (Entity<Workbench>, &mut VisualTestContext) {
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    (workbench, cx)
}

/// A window with a trusted project of one file open in it.
fn project_window(cx: &mut TestAppContext) -> (tempfile::TempDir, Entity<Workbench>, &mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    write(&root.join("a.txt"), "a\n");
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = window(cx);
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, workbench, cx)
}

/// The Vim page's view, drawn for its whole page in the open dialog.
fn vim_view(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) -> Entity<VimSettingsView> {
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(editor_vim::PAGE));
    assert_eq!(dialog.read_with(cx, |view, cx| view.control_lines(cx)), ["<page view>"]);
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(editor_vim::PAGE).cloned());
    view.and_then(|view| view.downcast().ok()).expect("the Vim page has its view")
}

/// File › Settings… on the Vim page, once the rc file is read.
fn open_vim_page(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<VimSettingsView>) {
    run_action(cx, "file.settings", Some(Value::from(editor_vim::PAGE)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    let page = vim_view(&dialog, cx);
    wait_long(cx, "the rc file is read", |cx| page.read_with(cx, |page, cx| page.rc_status(cx).loaded));
    (dialog, page)
}

/// What the dialog's buttons and tree say of the page after a change: nothing to apply, no Reset,
/// no page marked modified, as nothing was staged.
fn assert_nothing_staged(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) {
    let (apply, reset, modified) = dialog.read_with(cx, |view, cx| {
        (view.can_apply(cx), view.reset_visible(cx), view.modified_pages(cx).into_iter().collect::<Vec<_>>())
    });
    assert!(!apply, "Apply stays off");
    assert!(!reset, "no Reset is offered");
    assert!(modified.is_empty(), "no page is marked modified: {modified:?}");
}

/// The text stored under `key` as JSON; `Null` while nothing is.
fn stored_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

fn lines(page: &Entity<VimSettingsView>, cx: &mut VisualTestContext) -> Vec<String> {
    page.read_with(cx, |page, cx| page.lines(cx))
}

/// The row of `stroke` in the conflicts table.
fn index_of(stroke: &str) -> usize {
    VIM_KEYS.iter().position(|key| key.stroke == stroke).expect("a Vim key")
}

/// Clicks the page's element `selector`, which must be in the part of the page the dialog shows: a
/// click on an element scrolled out of view fails here instead of landing on whatever is drawn there.
fn click_shown(cx: &mut VisualTestContext, selector: &'static str) {
    cx.run_until_parked();
    let page = cx.debug_bounds("settings-vim-page").expect("the page is drawn");
    let target = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    let at = target.center();
    assert!(page.contains(&at), "{selector} at {at:?} is outside the page the dialog shows ({page:?})");
    click_on(cx, selector);
}

fn selector(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

/// Opens the handler choices of `stroke` and picks `handler` (`vim`, `ide`).
fn choose_handler(stroke: &str, handler: &str, cx: &mut VisualTestContext) {
    let row = index_of(stroke);
    click_shown(cx, selector(format!("settings-vim-handler-{row}")));
    click_shown(cx, selector(format!("settings-vim-handler-{row}-{handler}")));
}

/// Turns the wheel over the page, a step at a time, until the element `at` is wholly inside the part
/// of the page the dialog shows.
fn scroll_into_view(cx: &mut VisualTestContext, at: &'static str) {
    use gpui_kit::{Modifiers, ScrollDelta, ScrollEvent, TouchPhase, point, px};
    for _ in 0..60 {
        cx.run_until_parked();
        let page = cx.debug_bounds("settings-vim-page").expect("the page is drawn");
        let target = cx.debug_bounds(at).unwrap_or_else(|| panic!("{at} is drawn"));
        let step = if target.bottom() > page.bottom() {
            -60.0
        } else if target.top() < page.top() {
            60.0
        } else {
            return;
        };
        cx.simulate_event(ScrollEvent {
            position: point(page.center().x, page.top() + px(24.0)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(step))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: gpui_kit::ScrollSource::Pointer,
        });
    }
    panic!("{at} cannot be scrolled into view");
}

/// A registered action's title as the page shows it.
fn title_of(id: &str, cx: &mut VisualTestContext) -> String {
    cx.update(|_, cx| {
        let action = cx.global::<ActionRegistry>().get(id).expect("the action is registered");
        match cx.try_global::<crate::platform::i18n::Localization>() {
            Some(localization) => action.meta.localized(&localization.0).title,
            None => action.meta.title.clone(),
        }
    })
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).expect("the file is written");
}

// Settings › Editor › Vim: Enable Vim emulation and a handler chosen for a key are stored at once in
// the JSON the page reads back; choosing a key's default handler again removes its entry, Reset to
// Defaults removes every one and is then off, nothing is staged, and Cancel keeps the changes.
#[gpui_kit::test]
async fn vim_emulation_and_each_handler_are_stored_at_once_and_cancel_keeps_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_project, workbench, cx) = project_window(cx);
    let (dialog, page) = open_vim_page(&workbench, cx);
    assert_eq!(lines(&page, cx)[0], "[ ] Enable Vim emulation");
    assert_eq!(lines(&page, cx).last().map(String::as_str), Some("[Reset to Defaults] (disabled)"));

    click_shown(cx, "settings-vim-enabled");
    assert_eq!(stored_json(VIM_SETTINGS_KEY, cx), json!({ "enabled": true, "handlers": {} }));
    assert_eq!(lines(&page, cx)[0], "[x] Enable Vim emulation");

    choose_handler("Ctrl+A", "ide", cx);
    choose_handler("Escape", "vim", cx);
    let both = json!({ "enabled": true, "handlers": { "Ctrl+A": "ide", "Escape": "vim" } });
    assert_eq!(stored_json(VIM_SETTINGS_KEY, cx), both);
    let shown = lines(&page, cx);
    let row = |stroke: &str| shown.iter().find(|line| line.starts_with(&format_binding(stroke, is_mac_host())));
    assert!(row("Ctrl+A").is_some_and(|line| line.ends_with(" | Increment number | IDE")), "{shown:?}");
    assert!(row("Escape").is_some_and(|line| line.ends_with(" | Exit to Normal mode | Vim")), "{shown:?}");
    assert_nothing_staged(&dialog, cx);

    // Vim is Ctrl+A's default handler: choosing it again is no override.
    choose_handler("Ctrl+A", "vim", cx);
    let escape_only = json!({ "enabled": true, "handlers": { "Escape": "vim" } });
    assert_eq!(stored_json(VIM_SETTINGS_KEY, cx), escape_only);

    // Reset to Defaults is under the table, below the fold of the page.
    scroll_into_view(cx, "settings-vim-reset");
    click_shown(cx, "settings-vim-reset");
    assert_eq!(stored_json(VIM_SETTINGS_KEY, cx), json!({ "enabled": true, "handlers": {} }));
    assert_eq!(lines(&page, cx).last().map(String::as_str), Some("[Reset to Defaults] (disabled)"));
    assert_nothing_staged(&dialog, cx);

    scroll_into_view(cx, "settings-vim-enabled");
    click_shown(cx, "settings-vim-enabled");
    choose_handler("Ctrl+B", "ide", cx);
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    let kept = json!({ "enabled": false, "handlers": { "Ctrl+B": "ide" } });
    assert_eq!(stored_json(VIM_SETTINGS_KEY, cx), kept);
    let (_, again) = open_vim_page(&workbench, cx);
    assert_eq!(lines(&again, cx)[0], "[ ] Enable Vim emulation");
    assert_eq!(lines(&again, cx).last().map(String::as_str), Some("[Reset to Defaults]"));
}

// The conflicts table lists, for each key Vim uses, the titles of the IDE actions the keymap in
// effect binds to it (a chord starting with it included), `—` for a key no action is bound to, and
// follows a change of the keymap while it is open.
#[gpui_kit::test]
async fn the_conflicts_table_lists_the_ide_actions_the_keymap_binds_to_each_vim_key(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let overrides = json!({ "file.settings": ["Ctrl+R"], "file.newProjectsSettings": ["Ctrl+Y Ctrl+Y"] });
    cx.update(|_, cx| cx.global_mut::<input::Keymap>().0.apply_settings(None, Some(&overrides), None));
    let (_, page) = open_vim_page(&workbench, cx);
    let settings_title = title_of("file.settings", cx);
    let new_projects_title = title_of("file.newProjectsSettings", cx);
    let conflicts = page.read_with(cx, |page, cx| page.conflicts(cx));
    assert_eq!(conflicts.len(), VIM_KEYS.len());
    let line = |stroke: &str| conflicts.iter().find(|line| line.stroke == stroke).expect("a row").clone();
    assert_eq!(line("Ctrl+R").shortcut, format_binding("Ctrl+R", is_mac_host()));
    assert!(line("Ctrl+R").actions.split(", ").any(|title| title == settings_title), "{:?}", line("Ctrl+R"));
    assert!(line("Ctrl+Y").actions.split(", ").any(|title| title == new_projects_title), "{:?}", line("Ctrl+Y"));
    // Every row names the actions the effective keymap binds, `—` when there are none.
    let bindings = cx.update(|_, cx| cx.global::<input::Keymap>().0.effective_bindings().to_vec());
    for (row, shown) in conflict_rows(&bindings, &Default::default()).iter().zip(&conflicts) {
        assert_eq!(row.actions.is_empty(), shown.actions == "—", "{shown:?}");
    }

    let moved = json!({ "file.newProjectsSettings": ["Ctrl+R"] });
    cx.update(|_, cx| cx.global_mut::<input::Keymap>().0.apply_settings(None, Some(&moved), None));
    cx.run_until_parked();
    let after = page.read_with(cx, |page, cx| page.conflicts(cx));
    let ctrl_r = &after[index_of("Ctrl+R")];
    assert!(ctrl_r.actions.split(", ").any(|title| title == new_projects_title), "{ctrl_r:?}");
    assert!(!ctrl_r.actions.split(", ").any(|title| title == settings_title), "{ctrl_r:?}");
}

// The rc file is `.ideavimrc` in the home folder, read with the file its `source` line reaches
// (`~` is the home folder): the page shows its path, the mappings in it (`set` lines are not
// mappings) and each line left out, a sourced file's under its name as written.
#[gpui_kit::test]
async fn the_rc_file_in_the_home_folder_is_read_with_the_files_it_sources(cx: &mut TestAppContext) {
    let home = start(cx);
    let rc = home.path().join(".ideavimrc");
    write(&rc, "set tw=80\nnnoremap j gj\nsource ~/extra.vim\nautocmd BufRead * echo\n");
    write(&home.path().join("extra.vim"), "nmap gd <Action>(GotoDeclaration)\nset hlsearch\n");
    let (workbench, cx) = window(cx);
    let (_, page) = open_vim_page(&workbench, cx);
    let status = page.read_with(cx, |page, cx| page.rc_status(cx));
    assert_eq!(status.path, Some(rc.display().to_string()));
    assert_eq!(status.mappings, 2);
    let shown = lines(&page, cx);
    assert_eq!(shown[2], format!("{} · 2 mappings", rc.display()));
    assert_eq!(shown[4], "  ~/extra.vim:2: set hlsearch — option not supported: hlsearch");
    assert_eq!(shown[5], "  4: autocmd BufRead * echo — unsupported command: autocmd");
    assert_eq!(shown[6], "Shortcut conflicts");
}

// `.openideavimrc` is read in place of `.ideavimrc` when both are there; a sourced file that cannot be
// read is a line left out.
#[gpui_kit::test]
async fn openideavimrc_is_read_in_place_of_ideavimrc(cx: &mut TestAppContext) {
    let home = start(cx);
    let preferred = home.path().join(".openideavimrc");
    write(&preferred, "nnoremap k gk\nsource ~/missing.vim\n");
    write(&home.path().join(".ideavimrc"), "nnoremap j gj\nnnoremap h gh\n");
    let (workbench, cx) = window(cx);
    let (_, page) = open_vim_page(&workbench, cx);
    let shown = lines(&page, cx);
    assert_eq!(shown[2], format!("{} · 1 mappings", preferred.display()));
    assert_eq!(shown[4], "  2: source ~/missing.vim — cannot read file");
}

// The rc file is read when the page is first shown in the app; showing it again shows what was read,
// and Reload reads the file again: one written meanwhile is found, one removed is no longer.
#[gpui_kit::test]
async fn reload_reads_the_rc_file_again_and_showing_the_page_again_does_not(cx: &mut TestAppContext) {
    let home = start(cx);
    let (workbench, cx) = window(cx);
    let (_, page) = open_vim_page(&workbench, cx);
    assert_eq!(lines(&page, cx)[2], "No ~/.openideavimrc or ~/.ideavimrc found.");
    let rc = home.path().join(".ideavimrc");
    write(&rc, "nnoremap j gj\n");
    click_on(cx, "settings-cancel");
    let (_, again) = open_vim_page(&workbench, cx);
    assert_eq!(lines(&again, cx)[2], "No ~/.openideavimrc or ~/.ideavimrc found.", "not read again");

    click_shown(cx, "settings-vim-reload");
    let found = format!("{} · 1 mappings", rc.display());
    wait_long(cx, "the rc file is found", |cx| lines(&again, cx)[2] == found);
    std::fs::remove_file(&rc).expect("the file is removed");
    click_shown(cx, "settings-vim-reload");
    wait_long(cx, "the rc file is gone", |cx| lines(&again, cx)[2] == "No ~/.openideavimrc or ~/.ideavimrc found.");
}

// In a window with a project open, the rc file outside the project is read the same way, and Settings
// opened on another page reads no rc file until it comes to show the Vim page.
#[gpui_kit::test]
async fn a_project_window_reads_the_rc_file_once_the_vim_page_is_shown(cx: &mut TestAppContext) {
    let home = start(cx);
    let rc = home.path().join(".ideavimrc");
    write(&rc, "nnoremap j gj\nnnoremap k gk\n");
    let (_project, workbench, cx) = project_window(cx);

    run_action(cx, "file.settings", Some(Value::from("appearance")));
    cx.run_until_parked();
    let read = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.try_global::<VimRcStatus>().cloned());
    assert_eq!(read(cx), None, "no rc file is read while another page is shown");
    run_action(cx, "file.settings", Some(Value::from(editor_vim::PAGE)));
    wait_long(cx, "the rc file is read", |cx| read(cx).is_some_and(|status| status.loaded));
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings is open");
    let page = vim_view(&dialog, cx);
    assert_eq!(lines(&page, cx)[2], format!("{} · 2 mappings", rc.display()));
}

// Under a home of its own a `source` line reaches only files inside it: a file elsewhere, named by
// its path or climbing out with `..`, is a line left out and none of its mappings count.
#[gpui_kit::test]
async fn a_source_line_reads_nothing_outside_the_home_of_its_own(cx: &mut TestAppContext) {
    let home = start(cx);
    let elsewhere = tempfile::tempdir().expect("a temporary folder");
    let outside = elsewhere.path().canonicalize().expect("the folder exists").join("outside.vim");
    write(&outside, "nnoremap x dd\nnnoremap y yy\n");
    let climbing = format!("~/../{}", outside.strip_prefix("/").expect("an absolute path").display());
    let rc = home.path().join(".ideavimrc");
    write(&rc, &format!("nnoremap j gj\nsource {}\nsource {climbing}\nsource ~/inside.vim\n", outside.display()));
    write(&home.path().join("inside.vim"), "nnoremap k gk\n");
    let (workbench, cx) = window(cx);
    let (_, page) = open_vim_page(&workbench, cx);
    let shown = lines(&page, cx);
    assert_eq!(shown[2], format!("{} · 2 mappings", rc.display()), "only the home's two files count");
    assert_eq!(shown[4], format!("  2: source {} — cannot read file", outside.display()));
    assert_eq!(shown[5], format!("  3: source {climbing} — cannot read file"));
    assert_eq!(shown[6], "Shortcut conflicts");
}

// The rc file is looked for in the test's own home folder, never the user's: the home of a backend
// under a home of its own is that home.
#[gpui_kit::test]
async fn the_rc_file_is_looked_for_in_the_home_of_its_own(cx: &mut TestAppContext) {
    let home = start(cx);
    let dirs = cx.update(|cx| cx.global::<Backend>().dirs.clone());
    assert!(dirs.isolated);
    assert_eq!(crate::workbench::settings_vim_page::rc_home(&dirs).as_deref(), Some(home.path()));
}

// Settings saved by another window show on the open page at once.
#[gpui_kit::test]
async fn the_open_vim_page_shows_settings_stored_by_another_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    let (_, page) = open_vim_page(&workbench, cx);
    let stored = r#"{"enabled":true,"handlers":{"Ctrl+D":"ide"}}"#;
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(VIM_SETTINGS_KEY, stored.to_owned()));
    cx.run_until_parked();
    let shown = lines(&page, cx);
    assert_eq!(shown[0], "[x] Enable Vim emulation");
    let ctrl_d = &page.read_with(cx, |page, cx| page.conflicts(cx))[index_of("Ctrl+D")];
    assert_eq!(ctrl_d.handler, crate::features_a::vim::settings::Handler::Ide);
    assert_eq!(shown.last().map(String::as_str), Some("[Reset to Defaults]"));
}

// Tools › Vim Settings… opens Settings on the Vim page.
#[gpui_kit::test]
async fn vim_settings_opens_settings_on_the_vim_page(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    run_action(cx, "vim.settings", None);
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    let _page = vim_view(&dialog, cx);
}

// The Settings search finds the Vim page by labels drawn on it that are none of its keywords: the page
// stays in the filtered tree.
#[gpui_kit::test]
async fn the_settings_search_finds_the_vim_page_by_a_label_on_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = window(cx);
    for label in ["Configuration file", "IDE action", "Reset to Defaults"] {
        run_action(cx, "file.settings", Some(Value::from("appearance")));
        let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
        let field = dialog.read_with(cx, |view, _| view.search_field().clone());
        cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(label.to_owned()));
        cx.simulate_keystrokes("cmd-v");
        cx.run_until_parked();
        let rows = dialog.read_with(cx, |view, cx| view.pages(cx));
        assert!(rows.iter().any(|row| row.trim() == "Vim"), "{label} finds Vim: {rows:?}");
        click_on(cx, "settings-cancel");
    }
}
