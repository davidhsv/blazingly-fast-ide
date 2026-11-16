//! Cases of the code preview that wait on an undo history kept by the document rather than by each
//! editor: the editors of one document (a tab and a preview) undo each other's edits.

use super::*;
use crate::editor::marks::{MarkKind, MarkLayer};
use crate::find::popup_view::SEARCH_DEBOUNCE;
use crate::find::session::FindStatus;
use crate::ui::code_preview::CodePreview;
use serde_json::json;

const ORIGINAL: &str = "fn a() {}\nfn order() {}\nfn b() { order() }\n";

fn command(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-{key}") } else { format!("ctrl-{key}") }
}

fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) {
    for _ in 0..3000 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within thirty seconds");
}

/// Find in Files for `order`, previewing `path`: the popup and the preview's editor.
fn preview_of(
    workbench: &Entity<Workbench>,
    path: &str,
    cx: &mut VisualTestContext,
) -> (Entity<CodePreview>, Entity<Editor>) {
    run_action(cx, "find.inPath", Some(json!({ "query": "order" })));
    let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
    searched(&popup, cx);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "the preview", |cx| preview.read_with(cx, |preview, _| preview.path() == Some(path)));
    let editor = preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("the preview's editor");
    (preview, editor)
}

fn focus(editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.run_until_parked();
}

/// Opens `file` in a tab and returns the tab's editor.
fn tab_of(workbench: &Entity<Workbench>, file: &std::path::Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.to_path_buf(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the tab's editor")
}

/// An edit typed in the preview is the tab's too and modifies the document; the tab's Undo takes it back
/// in the preview at once, Redo in the preview brings it back, and the save shortcut in the preview saves
/// the document. The tab is the one shown, or one behind another tab.
#[gpui_kit::test]
async fn shares_edits_document_undo_and_save_with_a_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) =
        trusted_project(cx, &[("src/a.rs", ORIGINAL), ("src/other.rs", "fn other() {}\n")]);
    let file = root.join("src/a.rs");
    for mounted in [false, true] {
        std::fs::write(&file, ORIGINAL).expect("the file is written");
        let tab = tab_of(&workbench, &file, cx);
        if !mounted {
            tab_of(&workbench, &root.join("src/other.rs"), cx);
        }
        let (_preview, editor) = preview_of(&workbench, &text_of(&file), cx);
        focus(&editor, cx);
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        cx.simulate_input("// 😀");
        cx.simulate_keystrokes("enter");
        let edited = format!("// 😀\n{ORIGINAL}");
        wait_long(cx, "the edit in the tab", |cx| tab.read_with(cx, |tab, _| tab.text()) == edited);
        assert!(tab.read_with(cx, |tab, _| tab.is_dirty()), "mounted: {mounted}");
        let current = editor.read_with(cx, |editor, _| {
            let marks = editor.marks(MarkLayer::Find);
            let current = marks.iter().find(|mark| mark.kind == MarkKind::CurrentFindOccurrence).cloned();
            current.map(|mark| crate::editor::buffer::line_col(editor.rope(), mark.range.start).0)
        });
        assert_eq!(current, Some(2), "the mark moved down with its line");

        tab.update(cx, |tab, cx| {
            tab.undo(cx);
        });
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), ORIGINAL, "the tab's Undo takes the edit back");
        focus(&editor, cx);
        cx.simulate_keystrokes(&command("shift-z"));
        assert_eq!(editor.read_with(cx, |editor, _| editor.text()), edited);
        cx.simulate_keystrokes(&command("s"));
        wait_long(cx, "the save", |_| std::fs::read_to_string(&file).is_ok_and(|text| text == edited));
        wait_long(cx, "the document saved", |cx| !tab.read_with(cx, |tab, _| tab.is_dirty()));
        cx.simulate_keystrokes("escape");
    }
    close_backend_project(cx);
}

/// Typing in the preview, then opening a tab of the file: the tab's Undo and Redo take over the preview's
/// edit, and once the tab closes the preview undoes it again.
#[gpui_kit::test]
async fn hands_undo_history_to_a_newly_opened_tab_and_back_when_the_tab_closes(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(|cx| {
        let off = serde_json::Value::from(false);
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &off,
            cx,
        )
        .expect("the setting is stored");
    });
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let file = root.join("src/a.rs");
    let (_preview, editor) = preview_of(&workbench, &text_of(&file), cx);
    focus(&editor, cx);
    editor.update(cx, |editor, cx| editor.select(0, 0, cx));
    cx.simulate_input("preview");
    cx.simulate_keystrokes("enter");
    let tab = tab_of(&workbench, &file, cx);
    tab.update(cx, |tab, cx| {
        tab.undo(cx);
    });
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), ORIGINAL);
    tab.update(cx, |tab, cx| {
        tab.redo(cx);
    });
    run_action(cx, "editor.closeTab", None);
    focus(&editor, cx);
    cx.simulate_keystrokes(&command("z"));
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), ORIGINAL);
    close_backend_project(cx);
}

/// A preview with unsaved edits goes away; a tab of the file types too and closes; the preview shown
/// again has both edits and undoes both.
#[gpui_kit::test]
async fn tracks_tab_edits_and_history_while_the_dirty_preview_is_unmounted(cx: &mut TestAppContext) {
    let _home = start(cx);
    cx.update(|cx| {
        let off = serde_json::Value::from(false);
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &off,
            cx,
        )
        .expect("the setting is stored");
    });
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("src/a.rs", ORIGINAL)]);
    let file = root.join("src/a.rs");
    let path = text_of(&file);
    let (_preview, editor) = preview_of(&workbench, &path, cx);
    focus(&editor, cx);
    editor.update(cx, |editor, cx| editor.select(0, 0, cx));
    cx.simulate_input("preview");
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("escape");
    let tab = tab_of(&workbench, &file, cx);
    focus(&tab, cx);
    tab.update(cx, |tab, cx| tab.select(0, 0, cx));
    cx.simulate_input("tab");
    cx.simulate_keystrokes("enter");
    run_action(cx, "editor.closeTab", None);
    let (_preview, restored) = preview_of(&workbench, &path, cx);
    assert_eq!(restored.read_with(cx, |editor, _| editor.text()), format!("tab\npreview\n{ORIGINAL}"));
    focus(&restored, cx);
    cx.simulate_keystrokes(&command("z"));
    cx.simulate_keystrokes(&command("z"));
    assert_eq!(restored.read_with(cx, |editor, _| editor.text()), ORIGINAL);
    close_backend_project(cx);
}
