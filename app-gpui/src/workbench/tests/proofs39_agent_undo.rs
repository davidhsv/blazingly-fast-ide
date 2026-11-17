//! An edit an agent applies through the IDE's MCP tools, in a project window: the backend applies it
//! as it applies a refactoring, the window does not announce it, and Edit › Undo and Edit › Redo take
//! it back and put it again over every file it changed, also when it was made before any editor was
//! open. An Undo the backend refuses (a file changed after the edit) leaves every file as it is and
//! says why.
//!
//! The edit is the backend operation the MCP tools call (`apply_agent_workspace_edit`), started by the
//! test; no agent runs. The files are small ones in a temporary project and no language server runs.

use super::*;
use ide_core::{FileEdit, Pos, TextEdit, WorkspaceEdit};
use std::path::Path;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

const REFACTORING_APPLIED: &str = "Refactoring applied";
const REFACTORING_UNDONE: &str = "Refactoring undone";
const CANNOT_UNDO: &str = "Cannot undo refactoring";

fn agent_window<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Opened<'a> {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// Opens `path`, waits for its editor and gives it the keyboard with the caret at `caret`.
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

fn on_disk(path: &Path) -> Vec<u8> {
    std::fs::read(path).expect("the file is read")
}

/// Replaces the first three characters of each file of `files` with its text, as an agent's tool
/// call does: through the backend operation the MCP tools call, in the window's project.
fn apply_agent_edit(files: &[(&Path, &str)], cx: &mut VisualTestContext) {
    let first_three = ide_core::Range::new(Pos::new(0, 0), Pos::new(0, 3));
    let files = files
        .iter()
        .map(|&(path, new_text)| FileEdit::Edit {
            path: path.to_path_buf().into(),
            edits: vec![TextEdit { range: first_three, new_text: new_text.to_owned() }],
            version: None,
        })
        .collect();
    apply_agent_files(files, cx);
}

/// Applies `files` as one agent's edit, through the backend operation the MCP tools call.
fn apply_agent_files(files: Vec<FileEdit>, cx: &mut VisualTestContext) {
    let edit = WorkspaceEdit { files, preconditions: Vec::new() };
    cx.update(|_, cx| {
        let backend = cx.global::<Backend>();
        backend.block_on(ide_host::ops::lsp::apply_agent_workspace_edit(&backend.host, "w1", edit))
    })
    .expect("the agent's edit applies");
    cx.run_until_parked();
}

/// The notifications of the window, as `(title, body)`.
fn notices(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, Option<String>)> {
    workbench
        .read_with(cx, |w, _| w.notifications().items().iter().map(|n| (n.title.clone(), n.body.clone())).collect())
}

fn announced(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) -> bool {
    notification_titles(workbench, cx).iter().any(|shown| shown == title)
}

// does not announce an agent's edit as a refactoring: the edit is applied and remembered (Edit › Undo
// takes it back), and neither the edit nor its Undo raises a refactoring notification.
#[gpui_kit::test]
async fn does_not_announce_an_agents_edit_as_a_refactoring(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n")]);
    let a = root.join("a.txt");
    let editor = edit_at(&workbench, &a, 0, cx);
    apply_agent_edit(&[(&a, "new")], cx);
    settle(cx, |cx| text(&editor, cx) == "new a\n");
    assert_eq!(on_disk(&a), b"new a\n");

    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor, cx) == "old a\n");
    assert_eq!(on_disk(&a), b"old a\n");
    // The Undo found the edit, so its event was handled; it raised nothing.
    assert!(!announced(&workbench, REFACTORING_APPLIED, cx));
    assert!(!announced(&workbench, REFACTORING_UNDONE, cx));
    close_backend_project(cx);
}

/// An agent's edit over two open files: Edit › Undo in one of them puts the bytes of both back, on
/// disk and in both editors, and Edit › Redo applies the edit to both again.
#[gpui_kit::test]
async fn edit_undo_takes_back_an_agents_edit_over_two_files_and_redo_applies_it_again(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let editor_a = edit_at(&workbench, &a, 0, cx);
    let editor_b = edit_at(&workbench, &b, 0, cx);
    apply_agent_edit(&[(&a, "new"), (&b, "new")], cx);
    settle(cx, |cx| text(&editor_a, cx) == "new a\n" && text(&editor_b, cx) == "new b\n");
    assert_eq!((on_disk(&a), on_disk(&b)), (b"new a\n".to_vec(), b"new b\n".to_vec()));

    // Edit › Undo in the editor of b, which has nothing of its own to undo.
    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor_a, cx) == "old a\n" && text(&editor_b, cx) == "old b\n");
    settle(cx, |_| on_disk(&a) == b"old a\n" && on_disk(&b) == b"old b\n");

    // Redo applies the edit the window knows as undone, which it learns from the backend's answer.
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.undone_edits()) == 1);
    run_action(cx, "edit.redo", None);
    settle(cx, |cx| text(&editor_a, cx) == "new a\n" && text(&editor_b, cx) == "new b\n");
    settle(cx, |_| on_disk(&a) == b"new a\n" && on_disk(&b) == b"new b\n");
    assert!(!announced(&workbench, CANNOT_UNDO, cx), "{:?}", notices(&workbench, cx));
    close_backend_project(cx);
}

/// reports a refused undo instead of discarding newer work: the agent edits the open a and the closed
/// b, then b is written by another program. Edit › Undo in a is refused by the backend, which names
/// b, and neither file loses anything. With b back as the agent left it, the Undo goes through.
#[gpui_kit::test]
async fn reports_a_refused_undo_instead_of_discarding_newer_work(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let editor_a = edit_at(&workbench, &a, 0, cx);
    apply_agent_edit(&[(&a, "new"), (&b, "new")], cx);
    settle(cx, |cx| text(&editor_a, cx) == "new a\n");
    assert_eq!(on_disk(&b), b"new b\n");
    std::fs::write(&b, "mine b\n").expect("b is written");

    run_action(cx, "edit.undo", None);
    wait_for_notice(&workbench, CANNOT_UNDO, cx);
    let refused = notices(&workbench, cx).into_iter().find(|(title, _)| title == CANNOT_UNDO);
    let body = refused.and_then(|(_, body)| body).expect("the refusal says why");
    assert!(body.contains("b.txt") && body.ends_with("changed; request the refactoring again"), "{body}");
    assert_eq!(text(&editor_a, cx), "new a\n");
    assert_eq!((on_disk(&a), on_disk(&b)), (b"new a\n".to_vec(), b"mine b\n".to_vec()));

    std::fs::write(&b, "new b\n").expect("b is written back");
    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor_a, cx) == "old a\n");
    settle(cx, |_| on_disk(&a) == b"old a\n" && on_disk(&b) == b"old b\n");
    close_backend_project(cx);
}

/// An agent's edit made while no editor is open is remembered all the same: once a file it changed
/// is open, Edit › Undo takes the edit back from every file.
#[gpui_kit::test]
async fn an_agents_edit_made_before_any_editor_opened_is_undone_from_the_editor_opened_later(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    assert!(workbench.read_with(cx, |w, _| w.editor().is_none()), "no editor is open");
    apply_agent_edit(&[(&a, "new"), (&b, "new")], cx);
    settle(cx, |_| on_disk(&a) == b"new a\n" && on_disk(&b) == b"new b\n");

    let editor_a = edit_at(&workbench, &a, 0, cx);
    assert_eq!(text(&editor_a, cx), "new a\n");
    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor_a, cx) == "old a\n");
    settle(cx, |_| on_disk(&a) == b"old a\n" && on_disk(&b) == b"old b\n");
    close_backend_project(cx);
}

/// Every file and folder under `dir` with its bytes (`None` for a folder), by path relative to `dir`.
fn tree(dir: &Path) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, found: &mut std::collections::BTreeMap<String, Option<Vec<u8>>>) {
        for entry in std::fs::read_dir(dir).expect("the folder is read") {
            let path = entry.expect("an entry").path();
            let relative = path.strip_prefix(root).expect("under the root").to_string_lossy().into_owned();
            if path.is_dir() {
                found.insert(relative, None);
                walk(root, &path, found);
            } else {
                found.insert(relative, Some(on_disk(&path)));
            }
        }
    }
    let mut found = std::collections::BTreeMap::new();
    walk(dir, dir, &mut found);
    found
}

/// The tree `before` with `changes` applied (`None` removes the path).
fn changed(
    before: &std::collections::BTreeMap<String, Option<Vec<u8>>>,
    changes: &[(&str, Option<&[u8]>)],
) -> std::collections::BTreeMap<String, Option<Vec<u8>>> {
    let mut after = before.clone();
    for &(path, bytes) in changes {
        match bytes {
            Some(bytes) => after.insert(path.to_owned(), Some(bytes.to_vec())),
            None => after.remove(path),
        };
    }
    after
}

/// An agent's edit that changes a, deletes b and creates c: Undo changes back a, puts b back and
/// removes only c, leaving every other file of the project and a file outside it as they are; Redo
/// does the three again. The project's whole tree and the outside folder are compared after each
/// step.
#[gpui_kit::test]
async fn undoing_an_agents_edit_touches_only_the_files_it_changed_created_or_deleted(cx: &mut TestAppContext) {
    let _home = start(cx);
    let outside = tempfile::tempdir().expect("a folder outside the project");
    std::fs::write(outside.path().join("keep.txt"), "outside\n").expect("the outside file is written");
    let outside_before = tree(outside.path());
    let files = [("a.txt", "old a\n"), ("b.txt", "old b\n"), ("src/deep/d.txt", "old d\n")];
    let (_folder, root, workbench, cx) = agent_window(cx, &files);
    let (a, b, c) = (root.join("a.txt"), root.join("b.txt"), root.join("c.txt"));
    let editor_a = edit_at(&workbench, &a, 0, cx);
    let before = tree(&root);

    let first_three = ide_core::Range::new(Pos::new(0, 0), Pos::new(0, 3));
    apply_agent_files(
        vec![
            FileEdit::Edit {
                path: a.clone().into(),
                edits: vec![TextEdit { range: first_three, new_text: "new".to_owned() }],
                version: None,
            },
            FileEdit::Delete { path: b.into(), recursive: false },
            FileEdit::Create { path: c.into(), overwrite: false },
        ],
        cx,
    );
    settle(cx, |cx| text(&editor_a, cx) == "new a\n");
    let after = changed(&before, &[("a.txt", Some(b"new a\n")), ("b.txt", None), ("c.txt", Some(b""))]);
    settle(cx, |_| tree(&root) == after);

    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor_a, cx) == "old a\n");
    settle(cx, |_| tree(&root) == before);
    assert_eq!(tree(&root), before);
    assert_eq!(tree(outside.path()), outside_before);

    // Redo applies the edit the window knows as undone, which it learns from the backend's answer.
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.undone_edits()) == 1);
    run_action(cx, "edit.redo", None);
    settle(cx, |cx| text(&editor_a, cx) == "new a\n");
    settle(cx, |_| tree(&root) == after);
    assert_eq!(tree(&root), after);
    assert_eq!(tree(outside.path()), outside_before);
    assert!(!announced(&workbench, CANNOT_UNDO, cx), "{:?}", notices(&workbench, cx));
    close_backend_project(cx);
}

/// The window remembers each agent's edit once, whether it came before any editor opened or after
/// one did: opening an editor does not listen to the edits a second time.
#[gpui_kit::test]
async fn each_agents_edit_is_remembered_once_before_and_after_an_editor_opens(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    apply_agent_edit(&[(&a, "new")], cx);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.remembered_edits()) == 1);

    let editor_b = edit_at(&workbench, &b, 0, cx);
    apply_agent_edit(&[(&b, "new")], cx);
    settle(cx, |cx| text(&editor_b, cx) == "new b\n" && workbench.read_with(cx, |w, _| w.remembered_edits()) >= 2);
    cx.run_until_parked();
    assert_eq!(workbench.read_with(cx, |w, _| w.remembered_edits()), 2);
    close_backend_project(cx);
}

/// Typing made in an editor before a workspace edit of its file: the edit starts the editor's own
/// history over, so after Edit › Undo has taken the edit back, the earlier typing stays in the text
/// and Edit › Undo has nothing more to take back in that editor.
#[gpui_kit::test]
async fn typing_made_before_an_agents_edit_is_kept_but_no_longer_undoable(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n")]);
    let a = root.join("a.txt");
    let editor = edit_at(&workbench, &a, 5, cx);
    cx.simulate_input("!");
    settle(cx, |cx| text(&editor, cx) == "old a!\n");
    assert!(editor.read_with(cx, |e, _| e.can_undo()), "the typing is undoable before the edit");

    apply_agent_edit(&[(&a, "new")], cx);
    settle(cx, |cx| text(&editor, cx) == "new a!\n");
    assert!(!editor.read_with(cx, |e, _| e.can_undo()), "the edit starts the editor's history over");

    run_action(cx, "edit.undo", None);
    settle(cx, |cx| text(&editor, cx) == "old a!\n");
    run_action(cx, "edit.undo", None);
    assert_eq!(text(&editor, cx), "old a!\n");
    assert!(!editor.read_with(cx, |e, _| e.can_undo()));
    close_backend_project(cx);
}

/// An agent's edit over a and b, then unsaved typing in b: Edit › Undo in a is refused by the
/// backend, which names b, and both editors and both files keep what they have.
#[gpui_kit::test]
async fn an_undo_from_another_editor_keeps_newer_unsaved_typing_in_an_editor_of_the_edit(cx: &mut TestAppContext) {
    let _home = start(cx);
    // Autosave would save b when its tab is left; the typing stays unsaved.
    cx.update(|cx| {
        crate::platform::settings::set(
            "w1",
            ide_workspace::settings::SettingsScope::Global,
            "files.autoSave",
            &false.into(),
            cx,
        )
        .expect("the setting is stored");
    });
    let (_folder, root, workbench, cx) = agent_window(cx, &[("a.txt", "old a\n"), ("b.txt", "old b\n")]);
    let (a, b) = (root.join("a.txt"), root.join("b.txt"));
    let editor_a = edit_at(&workbench, &a, 0, cx);
    let editor_b = edit_at(&workbench, &b, 0, cx);
    apply_agent_edit(&[(&a, "new"), (&b, "new")], cx);
    settle(cx, |cx| text(&editor_a, cx) == "new a\n" && text(&editor_b, cx) == "new b\n");

    edit_at(&workbench, &b, 5, cx);
    cx.simulate_input("!");
    settle(cx, |cx| text(&editor_b, cx) == "new b!\n");
    edit_at(&workbench, &a, 0, cx);
    run_action(cx, "edit.undo", None);
    wait_for_notice(&workbench, CANNOT_UNDO, cx);
    let refused = notices(&workbench, cx).into_iter().find(|(title, _)| title == CANNOT_UNDO);
    let body = refused.and_then(|(_, body)| body).expect("the refusal says why");
    assert!(body.contains("b.txt") && body.ends_with("changed; request the refactoring again"), "{body}");
    assert_eq!((text(&editor_a, cx), text(&editor_b, cx)), ("new a\n".to_owned(), "new b!\n".to_owned()));
    assert_eq!((on_disk(&a), on_disk(&b)), (b"new a\n".to_vec(), b"new b\n".to_vec()));
    close_backend_project(cx);
}
