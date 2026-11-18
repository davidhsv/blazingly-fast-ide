//! File › New › Untitled File in a window: the buffers it opens, their tabs and names, the keys of the
//! VS Code keymaps, a buffer shared by two tabs, the session that never holds one, and the question
//! closing the last tab of one with text asks. Every window runs on the real backend over a
//! temporary home and project.

use super::*;
use crate::editor::tabs::EditorInput;
use crate::editor_model::untitled::{UNTITLED_KIND, untitled_input};

/// The input of the active tab of the active pane.
pub(super) fn active_input(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<EditorInput> {
    workbench.read_with(cx, |w, _| {
        let tabs = w.tabs()?;
        Some(tabs.pane(&tabs.active_pane)?.active_tab()?.input.clone())
    })
}

/// Every tab input of every pane, pane by pane.
pub(super) fn all_inputs(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<EditorInput> {
    workbench.read_with(cx, |w, _| {
        w.tabs()
            .map(|tabs| tabs.panes.iter().flat_map(|pane| pane.tabs.iter().map(|tab| tab.input.clone())).collect())
            .unwrap_or_default()
    })
}

/// Runs File › New › Untitled File and waits for the buffer `key` to be the active tab, with its
/// editor holding the keyboard.
pub(super) fn new_untitled(workbench: &Entity<Workbench>, key: &str, cx: &mut VisualTestContext) {
    run_action(cx, "file.newUntitled", None);
    settle(cx, |cx| {
        active_input(workbench, cx).is_some_and(|input| input.key == key)
            && workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path().to_string_lossy() == key))
    });
}

/// Types `typed` into the window's editor at its start and waits for the backend to report the
/// document unsaved.
pub(super) fn type_untitled(workbench: &Entity<Workbench>, typed: &str, cx: &mut VisualTestContext) {
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    cx.update(|window, cx| {
        editor.update(cx, |editor, cx| editor.select(0, 0, cx));
        window.focus(&editor.focus_handle(cx), cx);
    });
    cx.simulate_input(typed);
    settle(cx, |cx| editor.read_with(cx, |editor, _| editor.is_dirty()));
}

/// The untitled buffers the backend holds for the window: key, text, unsaved.
pub(super) fn backend_buffers(cx: &mut VisualTestContext) -> Vec<(String, String, bool)> {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.project("w1").map(|project| project.documents.untitled_buffers()).unwrap_or_default()
}

/// The names of the files and folders directly in `dir`, sorted.
pub(super) fn listed(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the folder is listed")
        .map(|entry| entry.expect("an entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

pub(super) fn close_backend(cx: &mut VisualTestContext) {
    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    host.close_project("w1");
}

// src/editor/core/untitled.test.ts › untitled editor input › opens editable, pathless buffers with
// incrementing names
/// File › New › Untitled File opens an empty, editable plain-text buffer named Untitled-1 in a tab
/// of its own kind, unsaved only once something is typed; the next one is Untitled-2. The tab and
/// the window title show the name, not the key.
#[gpui_kit::test]
async fn opens_editable_pathless_buffers_with_incrementing_names(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);

    assert_eq!(active_input(&workbench, cx), Some(untitled_input("untitled:Untitled-1")));
    let input = active_input(&workbench, cx).expect("a tab");
    assert_eq!((input.kind.as_str(), input.path.as_deref()), (UNTITLED_KIND, Some("untitled:Untitled-1")));
    let titles: Vec<String> = workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|t| t.name).collect());
    assert_eq!(titles, ["Untitled-1"]);
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.read_with(cx, |editor, _| {
        assert_eq!(editor.language().as_str(), "plaintext");
        assert!(!editor.is_read_only() && !editor.is_dirty() && editor.is_untitled());
        assert_eq!(editor.text(), "");
    });
    let title = workbench.read_with(cx, |w, cx| w.title(cx));
    assert!(title.ends_with("Untitled-1") && !title.contains("untitled:"), "{title}");

    type_untitled(&workbench, "draft", cx);
    assert_eq!(editor.read_with(cx, |editor, _| editor.text()), "draft");
    assert_eq!(listed(&root), ["main.rs"], "typing writes nothing");

    new_untitled(&workbench, "untitled:Untitled-2", cx);
    let titles: Vec<String> = workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|t| t.name).collect());
    assert_eq!(titles, ["Untitled-1", "Untitled-2"]);
    assert_eq!(untitled_input("untitled:Untitled-9").key, "untitled:Untitled-9");
    close_backend(cx);
}

/// In the VS Code keymaps ⌘N (macOS) and Ctrl+N (Windows) open an untitled buffer, wherever the
/// keyboard is in the window.
#[gpui_kit::test]
async fn the_vs_code_keymaps_open_an_untitled_buffer_with_cmd_n_and_ctrl_n(cx: &mut TestAppContext) {
    use crate::platform::keymap::KeymapState;
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("notes.txt", "notes\n")]);
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));

    for (preset, keys, key) in
        [("vscode-macos", "cmd-n", "untitled:Untitled-1"), ("vscode-windows", "ctrl-n", "untitled:Untitled-2")]
    {
        cx.update(|_, cx| cx.global_mut::<input::Keymap>().0 = KeymapState::new(preset));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.simulate_keystrokes(keys);
        settle(cx, |cx| active_input(&workbench, cx).is_some_and(|input| input.key == key));
        assert_eq!(active_input(&workbench, cx), Some(untitled_input(key)), "{preset}");
    }
    close_backend(cx);
}

// src/editor/core/untitled.test.ts › untitled editor input › reuses the buffer for a re-opened untitled
// name and shares it across tabs
/// Opening an untitled buffer's key again gives the same document, and Split Right shows the buffer
/// in a second tab: both editors are of one document, and what is typed in one is in the other.
#[gpui_kit::test]
async fn reuses_the_buffer_for_a_re_opened_untitled_name_and_shares_it_across_tabs(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    let first = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

    let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
    let documents = host.project("w1").expect("the project is open").documents.clone();
    let again = documents.open_untitled(Some("untitled:Untitled-1")).expect("the buffer opens again");
    assert_eq!(again.id, first.read_with(cx, |editor, _| editor.doc()));
    documents.close(again.id);

    run_action(cx, "editor.splitRight", None);
    settle(cx, |cx| {
        workbench.read_with(cx, |w, _| w.editors().len()) == 2
            && workbench.read_with(cx, |w, _| w.editor().is_some_and(|editor| *editor != first))
    });
    let inputs = all_inputs(&workbench, cx);
    assert_eq!(inputs, [untitled_input("untitled:Untitled-1"), untitled_input("untitled:Untitled-1")]);
    let second = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the second editor");
    assert_eq!(second.read_with(cx, |e, _| e.doc()), first.read_with(cx, |e, _| e.doc()));

    type_untitled(&workbench, "shared", cx);
    settle(cx, |cx| first.read_with(cx, |editor, _| editor.text()) == "shared");
    assert_eq!(backend_buffers(cx), [("untitled:Untitled-1".to_owned(), "shared".to_owned(), true)]);
    close_backend(cx);
}

/// The stored session keeps the project's file tabs and where they are, never an untitled buffer:
/// neither its tab nor its caret.
#[gpui_kit::test]
async fn the_session_never_holds_an_untitled_buffer(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    let main = root.join("main.rs");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(main.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == main)));
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    workbench.update(cx, |w, cx| w.save_session(cx));
    let stored: Vec<String> =
        cx.update(|_, cx| cx.global::<UiStorage>().entries().map(|(_, value)| value.to_owned()).collect());
    let main = main.to_string_lossy().into_owned();
    assert!(stored.iter().any(|value| value.contains(&main)), "the file tab is stored");
    assert!(stored.iter().all(|value| !value.contains("untitled:")), "{stored:?}");
    close_backend(cx);
}

// src/editor/core/untitled.test.ts › untitled editor input › asks about a dirty untitled buffer when its
// last tab closes
/// Closing the last tab of an untitled buffer with text asks whatever autosave says (on by default):
/// Cancel brings the tab back with its text; Save asks where to save it and the file holds the text
/// with the final line break the save adds, and the tab is gone.
#[gpui_kit::test]
async fn asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    run_action(cx, "editor.closeTab", None);
    let (title, body) = cx.pending_prompt().expect("closing asks");
    assert_eq!(title, "Unsaved Changes");
    assert_eq!(body, "Untitled-1 has unsaved changes. Save it before closing?");
    cx.simulate_prompt_answer("Cancel");
    settle(cx, |cx| active_input(&workbench, cx) == Some(untitled_input("untitled:Untitled-1")));
    assert_eq!(editor_text(&workbench, cx).as_deref(), Some("draft"));

    run_action(cx, "editor.closeTab", None);
    cx.simulate_prompt_answer("Save");
    cx.run_until_parked();
    let kept = root.join("kept.txt");
    let target = kept.clone();
    cx.simulate_new_path_selection(move |_| Some(target));
    wait_long(cx, "the buffer to be written and its tab closed", |cx| {
        std::fs::read_to_string(&kept).is_ok_and(|text| text == "draft\n") && all_inputs(&workbench, cx).is_empty()
    });
    settle(cx, |cx| backend_buffers(cx).is_empty());
    close_backend(cx);
}

// src/editor/core/untitled.test.ts › untitled editor input › closes an untitled buffer without saving
// when the user declines
/// Don't Save closes the buffer: its tab is gone, nothing is written and the backend holds no
/// untitled buffer any more.
#[gpui_kit::test]
async fn closes_an_untitled_buffer_without_saving_when_the_user_declines(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);

    run_action(cx, "editor.closeTab", None);
    cx.simulate_prompt_answer("Don't Save");
    settle(cx, |cx| all_inputs(&workbench, cx).is_empty() && backend_buffers(cx).is_empty());
    assert_eq!(listed(&root), ["main.rs"]);
    close_backend(cx);
}

/// Closing one of two tabs of the same untitled buffer asks nothing: the other tab keeps the text.
/// A buffer with nothing typed closes without a question.
#[gpui_kit::test]
async fn closing_one_of_two_tabs_of_an_untitled_buffer_asks_nothing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = trusted_project(cx, &[("main.rs", "fn main() {}\n")]);
    cx.update(|window, _| window.activate_window());
    new_untitled(&workbench, "untitled:Untitled-1", cx);
    type_untitled(&workbench, "draft", cx);
    run_action(cx, "editor.splitRight", None);
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editors().len()) == 2);

    run_action(cx, "editor.closeTab", None);
    assert!(cx.pending_prompt().is_none(), "another tab shows the buffer");
    settle(cx, |cx| all_inputs(&workbench, cx).len() == 1);
    let texts: Vec<String> = workbench.read_with(cx, |w, cx| w.editors().iter().map(|e| e.read(cx).text()).collect());
    assert_eq!(texts, ["draft"]);

    new_untitled(&workbench, "untitled:Untitled-2", cx);
    run_action(cx, "editor.closeTab", None);
    assert!(cx.pending_prompt().is_none(), "an empty buffer closes quietly");
    settle(cx, |cx| backend_buffers(cx).len() == 1);
    close_backend(cx);
}
