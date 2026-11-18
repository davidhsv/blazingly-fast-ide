//! Save As of an untitled buffer: File › Save All asks where each one goes and turns its tab into the
//! file's, the dialog cancelled keeps the buffer, a refusal is told, and closing the window or the
//! project counts the buffer and stays while its Save As is cancelled. The save dialog is answered
//! with the test context's stand-in for the system's chooser, inside the temporary project.

use super::proofs48_untitled::{
    active_input, answer_save_dialog, close_backend, close_decided, listed, new_untitled, type_untitled,
};
use super::*;
use crate::editor::tabs::EditorInput;
use crate::editor_model::untitled::untitled_input;

fn file_input(path: &std::path::Path) -> EditorInput {
    let path = path.to_string_lossy().into_owned();
    EditorInput { kind: "file".to_owned(), key: path.clone(), path: Some(path), data: None }
}

fn editor_is_dirty(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> bool {
    workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).is_dirty()))
}

/// The stored values of the UI state, for what the session holds.
fn stored(cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| cx.global::<UiStorage>().entries().map(|(_, value)| value.to_owned()).collect())
}

// src/editor/core/untitled.test.ts › untitled editor input › autosave never writes an untitled buffer;
// explicit save asks for a path and retargets tab and model
/// Leaving the window with autosave on writes nothing for an untitled buffer and asks nothing. File ›
/// Save All asks where it goes; the file chosen, in a folder that did not exist, holds the text with
/// the final line break the save adds; the tab and its editor are that file's now, saved, in the
/// language of its extension, with the typing still undoable. The session holds the file and never an
/// untitled buffer.
#[gpui_kit::test]
async fn autosave_never_writes_an_untitled_buffer_explicit_save_asks_for_a_path_and_retargets_tab_and_model(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

    cx.deactivate_window();
    cx.run_until_parked();
    assert!(cx.pending_prompt().is_none());
    // Autosave runs the save cleanup on the buffer (the final line break) but has nowhere to write it.
    settle(cx, |cx| editor.read_with(cx, |editor, _| editor.text() == "draft\n"));
    assert!(editor.read_with(cx, |editor, _| editor.is_dirty()), "autosave leaves it unsaved");
    assert_eq!(listed(&root), ["main.rs"]);
    cx.update(|window, _| window.activate_window());

    run_action(cx, "file.save", None);
    let hello = root.join("new/dir/hello.rs");
    answer_save_dialog(cx, &root, Some(hello.clone()));
    // The backend reports the document clean before its answer reaches the window: the wait covers
    // the tab taking the file too.
    wait_long(cx, "the buffer to be written as hello.rs and its tab to be the file's", |cx| {
        std::fs::read_to_string(&hello).is_ok_and(|text| text == "draft\n")
            && !editor.read_with(cx, |editor, _| editor.is_dirty())
            && active_input(&workbench, cx).is_some_and(|input| input.kind == "file")
    });
    assert_eq!(active_input(&workbench, cx), Some(file_input(&hello)));
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.path(), hello.as_path());
        assert!(!editor.is_untitled());
        assert!(editor.can_undo(), "the buffer's history stays");
    });
    settle(cx, |cx| editor.read_with(cx, |editor, _| editor.language().as_str() == "rust"));
    let titles: Vec<String> = workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|t| t.name).collect());
    assert_eq!(titles, ["hello.rs"]);

    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "second", cx);
    workbench.update(cx, |w, cx| w.save_session(cx));
    let session = stored(cx);
    let hello = hello.to_string_lossy().into_owned();
    assert!(session.iter().any(|value| value.contains(&hello)), "{session:?}");
    assert!(session.iter().all(|value| !value.contains("untitled:")), "{session:?}");
    close_backend(cx);
}

// src/editor/core/untitled.test.ts › untitled editor input › keeps the buffer open and dirty when the
// save dialog is cancelled
/// Save All with the save dialog cancelled: the tab is still the untitled buffer's, with its text and
/// unsaved, and nothing is written; the next Save All asks again.
#[gpui_kit::test]
async fn keeps_the_buffer_open_and_dirty_when_the_save_dialog_is_cancelled(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    run_action(cx, "file.save", None);
    answer_save_dialog(cx, &root, None);
    cx.run_until_parked();
    assert_eq!(active_input(&workbench, cx), Some(untitled_input("untitled:Untitled-1")));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    // The save cleanup ran before the dialog opened, as before any save.
    assert!(editor.read_with(cx, |editor, _| editor.is_dirty() && editor.text() == "draft\n"));
    assert_eq!(listed(&root), ["main.rs"]);
    assert!(notification_titles(&workbench, cx).is_empty(), "cancelling is not a failure");

    // The buffer is still untitled: the next save asks again, and only the file chosen then is written.
    run_action(cx, "file.save", None);
    let kept = root.join("kept.txt");
    answer_save_dialog(cx, &root, Some(kept.clone()));
    wait_long(cx, "the buffer to be written as kept.txt and its tab to be the file's", |cx| {
        std::fs::read_to_string(&kept).is_ok_and(|text| text == "draft\n")
            && active_input(&workbench, cx).is_some_and(|input| input.kind == "file")
    });
    assert_eq!(listed(&root), ["kept.txt", "main.rs"], "the cancelled dialog wrote nothing");
    close_backend(cx);
}

/// Save As onto a file another tab has open is refused: "Cannot save file" says why, the buffer stays
/// untitled with its text, and the file is unchanged.
#[gpui_kit::test]
async fn save_as_onto_a_file_open_in_another_tab_is_refused_and_keeps_the_buffer(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    let notes = root.join("notes.txt");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(notes.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == notes)));
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    run_action(cx, "file.save", None);
    answer_save_dialog(cx, &root, Some(notes.clone()));
    wait_for_notice(&workbench, "Cannot save file", cx);
    let body = workbench.read_with(cx, |w, _| {
        w.notifications().items().iter().find(|n| n.title == "Cannot save file").and_then(|n| n.body.clone())
    });
    assert!(body.is_some_and(|body| body.contains("already open")), "the notice says why");
    assert_eq!(active_input(&workbench, cx), Some(untitled_input("untitled:Untitled-1")));
    // The save's cleanup (the final line break) runs before the backend answers, as before any save.
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("draft\n"));
    assert!(editor_is_dirty(&workbench, cx), "the buffer keeps its unsaved text");
    assert_eq!(std::fs::read_to_string(&notes).expect("readable"), "notes\n");
    close_backend(cx);
}

// src/features/app/__tests__/windowClose.test.ts › stays open when an untitled buffer's Save As is
// cancelled
/// Closing the window counts the untitled buffer among the unsaved files; Save All with its Save As
/// cancelled keeps the window and its project, quietly. Save All with a file chosen writes it, and
/// the window goes to the Welcome screen.
#[gpui_kit::test]
async fn stays_open_when_an_untitled_buffers_save_as_is_cancelled(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    let (title, body) = cx.pending_prompt().expect("the question is asked");
    assert_eq!(title, "Unsaved Changes");
    let project = root.file_name().expect("a name").to_string_lossy().into_owned();
    assert_eq!(body, format!("1 file has unsaved changes in {project}. Save before closing?"));
    cx.simulate_prompt_answer("Save All");
    answer_save_dialog(cx, &root, None);
    close_decided(&workbench, cx);
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the window stays");
    assert!(notification_titles(&workbench, cx).is_empty(), "{:?}", notification_titles(&workbench, cx));
    // Cleaned up before the dialog, as before any save; still unsaved.
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("draft\n"));
    assert!(editor_is_dirty(&workbench, cx));

    assert!(!cx.simulate_close());
    cx.run_until_parked();
    cx.simulate_prompt_answer("Save All");
    let kept = root.join("kept.txt");
    answer_save_dialog(cx, &root, Some(kept.clone()));
    wait_long(cx, "the buffer to be written and the window to leave the project", |cx| {
        std::fs::read_to_string(&kept).is_ok_and(|text| text == "draft\n")
            && workbench.read_with(cx, |w, _| w.project_tree().is_none() && w.welcome().is_some())
    });
}

/// Close Project counts the untitled buffer and asks; Save All with its Save As cancelled keeps the
/// project; Don't Save leaves for the Welcome screen and writes nothing.
#[gpui_kit::test]
async fn close_project_counts_an_untitled_buffer_and_stays_when_its_save_as_is_cancelled(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    run_action(cx, "file.closeProject", None);
    let (title, _) = cx.pending_prompt().expect("leaving asks about the buffer");
    assert_eq!(title, "Unsaved Changes");
    cx.simulate_prompt_answer("Save All");
    answer_save_dialog(cx, &root, None);
    close_decided(&workbench, cx);
    assert!(workbench.read_with(cx, |w, _| w.project_tree().is_some()), "the project stays");

    run_action(cx, "file.closeProject", None);
    cx.simulate_prompt_answer("Don't Save");
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_none() && w.welcome().is_some()));
    assert_eq!(listed(&root), ["main.rs"]);
}
