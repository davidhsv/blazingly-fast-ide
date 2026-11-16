//! Tab, Shift+Tab, Enter and Escape in a project window's editor with no template being filled in:
//! what each key does to the text, the selection and the Find bar, pressed as a user presses it, with
//! a word before the caret that no template has and with one a template has but the key does not
//! expand.
//!
//! The files are small ones in a temporary project; no language server runs (the default mode starts
//! none) and the UI storage is the test's own, in memory.

use super::*;
use std::path::Path;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

fn keys_window<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Opened<'a> {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// Opens `path` with `from..to` selected and the keyboard in its editor.
fn edit(workbench: &Entity<Workbench>, path: &Path, from: u32, to: u32, cx: &mut VisualTestContext) -> Entity<Editor> {
    workbench.update_in(cx, |w, window, cx| w.open_project_file(path.to_path_buf(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    editor.update(cx, |e, cx| e.select(from, to, cx));
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

fn put(editor: &Entity<Editor>, ranges: Vec<(u32, u32)>, cx: &mut VisualTestContext) {
    editor.update(cx, |e, cx| e.set_selections(ranges, 0, cx));
    cx.run_until_parked();
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
}

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

const FUNCTION: &str = "function f() {\n  zzq\n}\n";

// Tab with no template at the caret: whitespace to the next indent stop after a word no template
// has, the selected lines indented, and every caret indented.
#[gpui_kit::test]
async fn tab_outside_a_template_indents_the_caret_the_lines_and_every_caret(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = keys_window(cx, &[("src/main.ts", FUNCTION)]);
    let word_end = len("function f() {\n  zzq");
    let editor = edit(&workbench, &root.join("src/main.ts"), word_end, word_end, cx);
    press("tab", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  zzq \n}\n");
    assert_eq!(selections(&editor, cx), [(word_end + 1, word_end + 1)]);

    // The selected lines move one indent level right.
    put(&editor, vec![(0, len("function f() {\n  zzq"))], cx);
    press("tab", cx);
    assert_eq!(text(&editor, cx), "  function f() {\n    zzq \n}\n");

    // Every caret gets its whitespace.
    let (first, second) = (len("  function"), len("  function f() {\n    zzq"));
    put(&editor, vec![(first, first), (second, second)], cx);
    press("tab", cx);
    assert_eq!(text(&editor, cx), "  function   f() {\n    zzq  \n}\n");
    assert_eq!(selections(&editor, cx).len(), 2);

    close_backend_project(cx);
}

// Shift+Tab with no template being filled in is not a command of the editor: it reaches the text
// input as a tab character, typed at the caret after a template's abbreviation and in place of a
// selection alike.
#[gpui_kit::test]
async fn shift_tab_outside_a_template_types_a_tab_character(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "function f() {\n  log\n}\n";
    let (_folder, root, workbench, cx) = keys_window(cx, &[("src/main.ts", source)]);
    let word_end = len("function f() {\n  log");
    let editor = edit(&workbench, &root.join("src/main.ts"), word_end, word_end, cx);
    press("shift-tab", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  log\t\n}\n");
    assert_eq!(selections(&editor, cx), [(word_end + 1, word_end + 1)]);
    put(&editor, vec![(0, word_end)], cx);
    press("shift-tab", cx);
    assert_eq!(text(&editor, cx), "\t\t\n}\n");
    assert_eq!(selections(&editor, cx), [(1, 1)]);
    close_backend_project(cx);
}

// Enter with no template being filled in breaks the line with the language's indentation, after a
// template's abbreviation too, and replaces a selection.
#[gpui_kit::test]
async fn enter_outside_a_template_breaks_the_line_with_its_indentation(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "function f() {\n  log\n}\n";
    let (_folder, root, workbench, cx) = keys_window(cx, &[("src/main.ts", source)]);
    let word_end = len("function f() {\n  log");
    let editor = edit(&workbench, &root.join("src/main.ts"), word_end, word_end, cx);
    press("enter", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  log\n  \n}\n");
    let caret = len("function f() {\n  log\n  ");
    assert_eq!(selections(&editor, cx), [(caret, caret)]);

    let brace = len("function f() {");
    put(&editor, vec![(brace, brace)], cx);
    press("enter", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  \n  log\n  \n}\n");
    let word = len("function f() {\n  \n  ");
    put(&editor, vec![(word, word + 3)], cx);
    press("enter", cx);
    assert_eq!(text(&editor, cx), "function f() {\n  \n\n  \n  \n}\n");
    close_backend_project(cx);
}

// Escape with no template being filled in: the Find bar closes first, then several carets become
// the main one, then a selection becomes its caret; a template's abbreviation before the caret
// changes none of that, and the text stays.
#[gpui_kit::test]
async fn escape_outside_a_template_closes_the_find_bar_then_the_carets_then_the_selection(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "function f() {\n  log\n}\n";
    let (_folder, root, workbench, cx) = keys_window(cx, &[("src/main.ts", source)]);
    let word_end = len("function f() {\n  log");
    let editor = edit(&workbench, &root.join("src/main.ts"), word_end, word_end, cx);
    workbench.update_in(cx, |w, window, cx| w.show_find_bar(false, window, cx));
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.find_bar().is_some()));
    cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
    put(&editor, vec![(1, 4), (word_end, word_end)], cx);
    press("escape", cx);
    assert!(workbench.read_with(cx, |w, _| w.find_bar().is_none()), "the Find bar closes first");
    assert_eq!(selections(&editor, cx), [(1, 4), (word_end, word_end)]);
    press("escape", cx);
    assert_eq!(selections(&editor, cx), [(1, 4)], "the main range stays");
    press("escape", cx);
    assert_eq!(selections(&editor, cx), [(4, 4)], "the selection becomes its caret");
    press("escape", cx);
    assert_eq!(selections(&editor, cx), [(4, 4)]);
    assert_eq!(text(&editor, cx), source);
    close_backend_project(cx);
}
