//! The UTF-8 byte order mark from the File Encoding item's menu, and Reload with Encoding for a
//! document that cannot be read again: the rows the menu shows, what a save writes, the item's label.

use super::proofs48_untitled::{close_backend, new_untitled};
use super::*;
use ide_workspace::settings::SettingsScope;

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];

fn open_and_wait(workbench: &Entity<Workbench>, path: &std::path::Path, cx: &mut VisualTestContext) {
    let path = path.to_path_buf();
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
}

/// Clicks the File Encoding item and returns the rows of its menu: label and whether disabled.
fn open_encoding_menu(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
    cx.run_until_parked();
    let at = cx.debug_bounds("status:editor.encoding").expect("the status bar item is drawn").center();
    cx.simulate_click(at, gpui_kit::Modifiers::default());
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<StatusCommand>()).expect("the item's menu is open");
    menu.read_with(cx, |menu, _| menu.rows().iter().map(|row| (row.label.clone(), row.disabled)).collect())
}

fn encoding_item(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    workbench.read_with(cx, |w, cx| w.status_items(cx)).get(2).cloned().unwrap_or_default()
}

fn autosave(on: bool, cx: &mut VisualTestContext) {
    cx.update(|_, cx| crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &on.into(), cx))
        .expect("the setting is stored");
}

/// A UTF-8 file's encoding menu offers Add UTF-8 BOM; chosen, the file is unsaved and the item says
/// UTF-8 BOM, and Save All writes the mark before the text. The menu then offers Remove UTF-8 BOM,
/// after which the save writes the text alone.
#[gpui_kit::test]
async fn the_encoding_menu_adds_and_removes_the_utf8_bom_and_the_save_writes_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "hi\n")]);
    cx.update(|window, _| window.activate_window());
    autosave(false, cx);
    let notes = root.join("notes.txt");
    open_and_wait(&workbench, &notes, cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    assert_eq!(encoding_item(&workbench, cx), "UTF-8");

    let rows = open_encoding_menu(&workbench, cx);
    assert!(rows.contains(&("Add UTF-8 BOM".to_owned(), false)), "{rows:?}");
    assert!(!rows.iter().any(|(label, _)| label == "Remove UTF-8 BOM"));
    // A typed letter selects the row it starts.
    cx.simulate_keystrokes("a enter");
    settle(cx, |cx| encoding_item(&workbench, cx) == "UTF-8 BOM" && editor.read_with(cx, |e, _| e.is_dirty()));
    assert_eq!(std::fs::read(&notes).expect("readable"), b"hi\n", "nothing is written before the save");
    run_action(cx, "file.save", None);
    settle(cx, |cx| {
        std::fs::read(&notes).is_ok_and(|bytes| bytes == [BOM, b"hi\n"].concat())
            && !editor.read_with(cx, |e, _| e.is_dirty())
    });

    let rows = open_encoding_menu(&workbench, cx);
    assert!(rows.contains(&("Remove UTF-8 BOM".to_owned(), false)), "{rows:?}");
    cx.simulate_keystrokes("r enter");
    settle(cx, |cx| encoding_item(&workbench, cx) == "UTF-8");
    run_action(cx, "file.save", None);
    settle(cx, |cx| std::fs::read(&notes).is_ok_and(|bytes| bytes == b"hi\n"));
    close_backend(cx);
}

/// With autosave on, adding the mark writes it at once.
#[gpui_kit::test]
async fn with_autosave_on_adding_the_bom_writes_the_file_at_once(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "hi\n")]);
    cx.update(|window, _| window.activate_window());
    autosave(true, cx);
    let notes = root.join("notes.txt");
    open_and_wait(&workbench, &notes, cx);
    open_encoding_menu(&workbench, cx);
    cx.simulate_keystrokes("a enter");
    settle(cx, |cx| std::fs::read(&notes).is_ok_and(|bytes| bytes == [BOM, b"hi\n"].concat()));
    assert_eq!(encoding_item(&workbench, cx), "UTF-8 BOM");
    close_backend(cx);
}

/// A file in another encoding offers no byte order mark row; a read-only UTF-8 file shows the row
/// disabled.
#[cfg(unix)]
#[gpui_kit::test]
async fn the_bom_row_is_only_for_utf8_and_disabled_for_a_read_only_file(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt;
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("utf16.txt", ""), ("locked.txt", "locked\n")]);
    cx.update(|window, _| window.activate_window());
    let utf16 = root.join("utf16.txt");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend("hi\n".encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&utf16, bytes).expect("the file is written");
    let locked = root.join("locked.txt");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o444)).expect("permissions change");

    open_and_wait(&workbench, &utf16, cx);
    settle(cx, |cx| encoding_item(&workbench, cx).starts_with("UTF-16"));
    let rows = open_encoding_menu(&workbench, cx);
    assert!(!rows.iter().any(|(label, _)| label.contains("BOM")), "{rows:?}");
    cx.simulate_keystrokes("escape");

    open_and_wait(&workbench, &locked, cx);
    let rows = open_encoding_menu(&workbench, cx);
    assert!(rows.contains(&("Add UTF-8 BOM".to_owned(), true)), "{rows:?}");
    cx.simulate_keystrokes("escape");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).expect("permissions change");
    close_backend(cx);
}

/// An untitled buffer has no source to read again: Reload with Encoding is disabled for it although
/// nothing is unsaved; Convert to Encoding stays offered.
#[gpui_kit::test]
async fn reload_with_encoding_is_disabled_for_a_document_that_cannot_be_read_again(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    let rows = open_encoding_menu(&workbench, cx);
    assert_eq!(rows[..2], [("Reload with Encoding".to_owned(), true), ("Convert to Encoding".to_owned(), false)]);
    cx.simulate_keystrokes("escape");
    close_backend(cx);
}
