//! The template being filled in, in a project window's editor, against everything else that changes
//! the editor: ⌘Z and ⇧⌘Z, several carets, the Find bar's Escape, an edit made by another editor of
//! the document, the file reloaded from disk and the tab closed. The fields follow or the template
//! ends; no field is left drawn on text it no longer covers. A Find in Files preview of a file never
//! expands a template.
//!
//! The files are small ones in a temporary project; no language server runs and the UI storage is the
//! test's own, in memory.

use super::proofs18_e2e_fixtures::primary;
use super::*;
use crate::editor::marks::MarkLayer;
use crate::find::popup_view::SEARCH_DEBOUNCE;
use crate::find::session::FindStatus;
use serde_json::json;
use std::path::Path;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

fn session_window<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Opened<'a> {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

fn edit_at(workbench: &Entity<Workbench>, path: &Path, caret: u32, cx: &mut VisualTestContext) -> Entity<Editor> {
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.to_path_buf(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |e, cx| e.select(caret, caret, cx));
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    cx.run_until_parked();
    editor
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.text())
}

fn selections(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<(u32, u32)> {
    editor.read_with(cx, |e, _| e.selections().to_vec())
}

fn selected_texts(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| {
        let rope = e.rope();
        e.selections()
            .iter()
            .map(|&(anchor, head)| crate::editor::buffer::slice(rope, anchor.min(head), anchor.max(head)))
            .collect()
    })
}

fn active_field(workbench: &Entity<Workbench>, editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<usize> {
    workbench.read_with(cx, |w, _| w.template_session(editor).map(|session| session.active_field()))
}

/// The field ranges the editor draws, as the texts they cover.
fn drawn_fields(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| {
        let rope = e.rope();
        e.marks(MarkLayer::Snippet)
            .iter()
            .map(|mark| crate::editor::buffer::slice(rope, mark.range.start, mark.range.end))
            .collect()
    })
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

const FORI: &str = "for (let i = 0; i < .length; i++) {\n  \n}\n";

/// Types `fori` at the caret and expands it with Tab: the three `i` are selected.
fn expand_fori(workbench: &Entity<Workbench>, editor: &Entity<Editor>, cx: &mut VisualTestContext) {
    cx.simulate_input("fori");
    press("tab", cx);
    assert_eq!(selected_texts(editor, cx), ["i", "i", "i"]);
    assert_eq!(active_field(workbench, editor, cx), Some(0));
}

/// A second editor of `path`'s document, as another tab or split of it would be.
fn sibling_editor(path: &Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = path.to_path_buf();
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let opened = cx.update(|_, cx| {
        cx.global::<Backend>().with_host(move |host| async move { ide_host::ops::doc::open(&host, "w1", path).await })
    });
    let info = futures::executor::block_on(opened).expect("the task completes").expect("the document opens again");
    cx.new(|cx| Editor::new(host, "w1", &info, cx).expect("a second editor"))
}

// One ⌘Z gives the abbreviation back, which ends the template; ⇧⌘Z puts the expansion back as text.
#[gpui_kit::test]
async fn undo_gives_the_abbreviation_back_and_ends_the_template_and_redo_puts_the_expansion_back(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/loop.ts", "\n")]);
    let editor = edit_at(&workbench, &root.join("src/loop.ts"), 0, cx);
    expand_fori(&workbench, &editor, cx);
    press(&primary("z"), cx);
    assert_eq!(text(&editor, cx), "fori\n");
    assert_eq!(active_field(&workbench, &editor, cx), None);
    assert!(drawn_fields(&editor, cx).is_empty());
    press(&primary("shift-z"), cx);
    assert_eq!(text(&editor, cx), FORI);
    assert_eq!(active_field(&workbench, &editor, cx), None, "the redone text is no template to fill in");
    close_backend_project(cx);
}

// With several carets Tab expands the abbreviation at the main caret alone, which keeps the
// keyboard; the other caret's word stays as typed.
#[gpui_kit::test]
async fn several_carets_expand_the_template_at_the_main_caret_only(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "function f() {\n  log\n  log\n}\n";
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/main.ts", source)]);
    let (first, second) = (len("function f() {\n  log"), len("function f() {\n  log\n  log"));
    let editor = edit_at(&workbench, &root.join("src/main.ts"), first, cx);
    editor.update(cx, |e, cx| e.set_selections(vec![(first, first), (second, second)], 1, cx));
    press("tab", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  log\n  console.log();\n}\n");
    let caret = len("function f() {\n  log\n  console.log(");
    assert_eq!(selections(&editor, cx), [(caret, caret)]);
    close_backend_project(cx);
}

// Inside a template the tab's Find bar takes Escape first, as outside one: the first Escape closes the
// bar and the template goes on, the next drops the field's selection, and the last leaves the template.
#[gpui_kit::test]
async fn escape_in_a_template_closes_the_find_bar_first(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/loop.ts", "\n")]);
    let editor = edit_at(&workbench, &root.join("src/loop.ts"), 0, cx);
    workbench.update_in(cx, |w, window, cx| w.show_find_bar(false, window, cx));
    cx.run_until_parked();
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    expand_fori(&workbench, &editor, cx);
    let bar_open = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.find_bar().is_some());
    press("escape", cx);
    assert!(!bar_open(cx), "the Find bar closes first");
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    assert_eq!(selected_texts(&editor, cx), ["i", "i", "i"], "the field's selection stays");
    press("escape", cx);
    assert!(selections(&editor, cx).iter().all(|(anchor, head)| anchor == head), "the selection is dropped");
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    press("escape", cx);
    assert_eq!(active_field(&workbench, &editor, cx), None);
    assert!(drawn_fields(&editor, cx).is_empty());
    assert_eq!(text(&editor, cx), FORI);
    close_backend_project(cx);
}

// Another editor of the document edits it: an edit before the fields moves them, typing still renames
// every occurrence, and an edit across a field ends the template with no field left drawn.
#[gpui_kit::test]
async fn an_edit_from_another_editor_moves_the_fields_or_ends_the_template(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/loop.ts", "\n")]);
    let path = root.join("src/loop.ts");
    let editor = edit_at(&workbench, &path, 0, cx);
    expand_fori(&workbench, &editor, cx);
    let sibling = sibling_editor(&path, cx);
    let comment = ide_core::TextChange { from: 0, to: 0, insert: "// x\n".to_owned() };
    assert!(sibling.update(cx, |e, cx| e.apply_edit(&[comment], vec![(0, 0)], 0, cx)));
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), format!("// x\n{FORI}"));
    assert_eq!(active_field(&workbench, &editor, cx), Some(0), "the template goes on");
    assert_eq!(drawn_fields(&editor, cx).first().map(String::as_str), Some("i"));
    cx.simulate_input("j");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "// x\nfor (let j = 0; j < .length; j++) {\n  \n}\n");

    let across = len("// x\nfor (le");
    let cut = ide_core::TextChange { from: across, to: across + 4, insert: String::new() };
    assert!(sibling.update(cx, |e, cx| e.apply_edit(&[cut], vec![(across, across)], 0, cx)));
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "// x\nfor (le= 0; j < .length; j++) {\n  \n}\n");
    assert_eq!(active_field(&workbench, &editor, cx), None);
    assert!(drawn_fields(&editor, cx).is_empty());
    press("tab", cx);
    assert!(text(&editor, cx).starts_with("// x\nfor (le"), "Tab is the editor's again");
    close_backend_project(cx);
}

// The file reloaded from disk while a template is filled in: the new text replaces the fields, the
// template ends and nothing is drawn on the new text.
#[gpui_kit::test]
async fn reloading_the_file_from_disk_ends_the_template(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/loop.ts", "\n")]);
    let path = root.join("src/loop.ts");
    let editor = edit_at(&workbench, &path, 0, cx);
    expand_fori(&workbench, &editor, cx);
    run_action(cx, "file.save", None);
    // The save trims the edited body line, which no caret is on.
    let saved_text = "for (let i = 0; i < .length; i++) {\n\n}\n";
    settle(cx, |_| std::fs::read_to_string(&path).is_ok_and(|saved| saved == saved_text));
    assert_eq!(active_field(&workbench, &editor, cx), Some(0), "saving keeps the template");
    std::fs::write(&path, "let other = 1;\n").expect("the file is written");
    run_action(cx, "file.reloadFromDisk", None);
    settle(cx, |cx| text(&editor, cx) == "let other = 1;\n");
    assert_eq!(active_field(&workbench, &editor, cx), None);
    assert!(drawn_fields(&editor, cx).is_empty());
    close_backend_project(cx);
}

// Closing the tab of an editor filling in a template leaves nothing of it behind.
#[gpui_kit::test]
async fn closing_the_tab_forgets_its_template(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = session_window(cx, &[("src/loop.ts", "\n")]);
    let editor = edit_at(&workbench, &root.join("src/loop.ts"), 0, cx);
    expand_fori(&workbench, &editor, cx);
    assert_eq!(workbench.read_with(cx, |w, _| w.template_sessions()), 1);
    run_action(cx, "file.save", None);
    drop(editor);
    run_action(cx, "editor.closeTab", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_none()));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.template_sessions() == 0));
    close_backend_project(cx);
}

/// Waits for the Find in Files search to finish.
fn searched(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<crate::find::popup_view::FindPopup> {
    let popup = workbench.read_with(cx, |w, _| w.find_popup()).expect("Find in Files is open");
    for _ in 0..3000 {
        cx.executor().advance_clock(SEARCH_DEBOUNCE);
        cx.run_until_parked();
        if popup.read_with(cx, |popup, _| popup.status() == FindStatus::Done && !popup.is_stale()) {
            return popup;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the search did not finish within thirty seconds");
}

// The editor of a Find in Files preview is no editor of the window's tabs: Tab after an abbreviation
// there is the editor's own and expands nothing.
#[gpui_kit::test]
async fn a_find_in_files_preview_never_expands_a_template(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = session_window(cx, &[("src/a.ts", "fori\n")]);
    run_action(cx, "find.inPath", Some(json!({ "query": "fori" })));
    let popup = searched(&workbench, cx);
    let preview = popup.read_with(cx, |popup, _| popup.code_preview().clone());
    wait_long(cx, "the preview of the file", |cx| preview.read_with(cx, |preview, _| preview.editor().is_some()));
    let editor = preview.read_with(cx, |preview, _| preview.editor().cloned()).expect("the preview's editor");
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&editor.focus_handle(cx), cx);
    });
    editor.update(cx, |e, cx| e.select(4, 4, cx));
    press("tab", cx);
    let shown = text(&editor, cx);
    assert!(shown.starts_with("fori") && !shown.contains("for ("), "{shown:?}");
    assert_eq!(workbench.read_with(cx, |w, _| w.template_sessions()), 0);
    close_backend_project(cx);
}
