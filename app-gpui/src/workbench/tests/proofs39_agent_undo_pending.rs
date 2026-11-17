//! Waits on the per-editor refactoring history (`crate::editor_model::workspace_history`) being wired
//! into the editors: an Undo started in one editor of a workspace edit is refused by the window
//! itself, before the backend is asked, while another open editor of the edit has newer typing on
//! top of it. Today the backend refuses it with its own message (the typed file "changed; request
//! the refactoring again"), so the assertion on the message fails. Declare it in
//! `workbench/mod.rs`'s `mod tests` as `mod proofs39_agent_undo_editor_history;` (renamed without
//! `_pending`) once the marker model is wired.

use super::*;
use ide_core::{FileEdit, Pos, TextEdit, WorkspaceEdit};
use ide_workspace::settings::SettingsScope;
use std::path::Path;

const CANNOT_UNDO: &str = "Cannot undo refactoring";
const NEWER_CHANGES: &str = "Undo newer editor changes before undoing this refactoring";

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

/// An agent's edit over a and b, then typing in b: Edit › Undo in a is refused with "Undo newer
/// editor changes before undoing this refactoring", and both editors and both files keep their text.
#[gpui_kit::test]
async fn an_undo_from_another_editor_is_refused_while_an_editor_of_the_edit_has_newer_typing(cx: &mut TestAppContext) {
    let _home = start(cx);
    // Autosave would save b when its tab is left.
    cx.update(|cx| {
        crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
            .expect("the setting is stored");
    });
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    cx.update(|window, _| window.activate_window());
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let editor_a = edit_at(&workbench, &a, 0, cx);
    let editor_b = edit_at(&workbench, &b, 0, cx);
    let first_three = ide_core::Range::new(Pos::new(0, 0), Pos::new(0, 3));
    let files = [&a, &b]
        .into_iter()
        .map(|path| FileEdit::Edit {
            path: path.clone().into(),
            edits: vec![TextEdit { range: first_three, new_text: "new".to_owned() }],
            version: None,
        })
        .collect();
    let edit = WorkspaceEdit { files, preconditions: Vec::new() };
    cx.update(|_, cx| {
        let backend = cx.global::<Backend>();
        backend.block_on(ide_host::ops::lsp::apply_agent_workspace_edit(&backend.host, "w1", edit))
    })
    .expect("the agent's edit applies");
    settle(cx, |cx| text(&editor_a, cx) == "new a\n" && text(&editor_b, cx) == "new b\n");

    edit_at(&workbench, &b, 5, cx);
    cx.simulate_input("!");
    cx.run_until_parked();
    edit_at(&workbench, &a, 0, cx);
    run_action(cx, "edit.undo", None);
    wait_for_notice(&workbench, CANNOT_UNDO, cx);
    let bodies: Vec<Option<String>> = workbench.read_with(cx, |w, _| {
        w.notifications().items().iter().filter(|n| n.title == CANNOT_UNDO).map(|n| n.body.clone()).collect()
    });
    assert_eq!(bodies.last().cloned().flatten().as_deref(), Some(NEWER_CHANGES));
    assert_eq!((text(&editor_a, cx), text(&editor_b, cx)), ("new a\n".to_owned(), "new b!\n".to_owned()));
    assert_eq!(std::fs::read(&a).expect("a is read"), b"new a\n");
    assert_eq!(std::fs::read(&b).expect("b is read"), b"new b\n");
    close_backend_project(cx);
}
