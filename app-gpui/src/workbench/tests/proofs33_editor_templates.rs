//! Live and postfix templates in a project window's editor, driven as a user drives them: an
//! abbreviation typed and Tab, the fields filled in with typing, Tab, Enter and Escape, ⌘Z after,
//! Insert Live Template through the registry and its shortcut, and the Choose Template chooser. The
//! templates and options in effect are what the UI storage holds under the Settings pages' keys,
//! written here directly.
//!
//! The files are small ones in a temporary project; no language server runs (the default mode
//! starts none) and the UI storage is the test's own, in memory.

use super::proofs18_e2e_fixtures::primary;
use super::*;
use crate::editor::intelligence::EditorIntelligence;
use crate::editor::marks::MarkLayer;
use crate::platform::storage::UiStorage;
use crate::templates::live_store::{LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY};
use crate::templates::postfix_store::POSTFIX_KEY;
use gpui_kit::ClipboardItem;
use std::path::Path;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

/// A trusted project with `files`, in an active window.
fn template_window<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Opened<'a> {
    let (folder, root, workbench, cx) = trusted_project(cx, files);
    cx.update(|window, _| window.activate_window());
    (folder, root, workbench, cx)
}

/// Opens `path` and gives its editor the keyboard with the caret at `caret`.
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

/// The selected texts, in document order.
fn selected_texts(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    editor.read_with(cx, |e, _| {
        let rope = e.rope();
        e.selections()
            .iter()
            .map(|&(anchor, head)| crate::editor::buffer::slice(rope, anchor.min(head), anchor.max(head)))
            .collect()
    })
}

/// The field the caret is in, while a template is filled in.
fn active_field(workbench: &Entity<Workbench>, editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Option<usize> {
    workbench.read_with(cx, |w, _| w.template_session(editor).map(|session| session.active_field()))
}

/// How many field ranges the editor draws.
fn drawn_fields(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> usize {
    editor.read_with(cx, |e, _| e.marks(MarkLayer::Snippet).len())
}

fn store(key: &str, value: &str, cx: &mut VisualTestContext) {
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(key, value.to_owned()));
}

fn undo(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(&primary("z"));
    cx.run_until_parked();
}

/// Puts the caret at the end of the 0-based `line`.
fn caret_at_line_end(editor: &Entity<Editor>, line: u32, cx: &mut VisualTestContext) {
    editor.update(cx, |e, cx| {
        let end = crate::editor::buffer::line_range(e.rope(), line).end;
        e.select(end, end, cx);
    });
}

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// The rows the completion lookup of `editor` shows.
fn lookup_rows(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> Vec<String> {
    let intelligence = cx.update(|_, cx| EditorIntelligence::of(editor, cx)).expect("the editor's intelligence");
    intelligence.read_with(cx, |intelligence, cx| intelligence.lookup().read(cx).rows())
}

const FUNCTION: &str = "function f() {\n  \n}\n";

// Tab expansion › expands a live template abbreviation; one ⌘Z gives the abbreviation back, and Tab
// after a word no template has is the editor's own.
#[gpui_kit::test]
async fn expands_a_live_template_abbreviation(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    cx.simulate_input("log");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  console.log();\n}\n");
    let caret = len("function f() {\n  console.log(");
    assert_eq!(selections(&editor, cx), [(caret, caret)]);
    // A template with one stop leaves nothing to fill in.
    assert_eq!(active_field(&workbench, &editor, cx), None);
    undo(cx);
    assert_eq!(text(&editor, cx), "function f() {\n  log\n}\n");

    // Tab expansion › falls through when nothing matches
    let end = len("function f() {\n  log");
    editor.update(cx, |e, cx| e.select(end, end, cx));
    cx.simulate_input("zzz");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(text(&editor, cx).starts_with("function f() {\n  logzzz "), "{}", text(&editor, cx));
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    close_backend_project(cx);
}

// template fields › Enter moves to the next field like Tab: the linked occurrences of the first field
// take what is typed, Enter and Tab walk on, the last field ends the template, and ⌘Z takes back one
// field's typing at a time before the expansion.
#[gpui_kit::test]
async fn enter_moves_to_the_next_field_like_tab(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/loop.ts", "\n")]);
    let editor = edit_at(&workbench, &root.join("src/loop.ts"), 0, cx);
    cx.simulate_input("fori");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "for (let i = 0; i < .length; i++) {\n  \n}\n");
    assert_eq!(selected_texts(&editor, cx), ["i", "i", "i"]);
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    assert!(drawn_fields(&editor, cx) > 0, "the fields are drawn");

    cx.simulate_input("j");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "for (let j = 0; j < .length; j++) {\n  \n}\n");
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    let arr = len("for (let j = 0; j < ");
    assert_eq!(selections(&editor, cx), [(arr, arr)]);
    assert_eq!(active_field(&workbench, &editor, cx), Some(1));
    // Enter moved the caret and typed no line break.
    assert_eq!(text(&editor, cx), "for (let j = 0; j < .length; j++) {\n  \n}\n");
    cx.simulate_input("xs");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let end = len("for (let j = 0; j < xs.length; j++) {\n  ");
    assert_eq!(selections(&editor, cx), [(end, end)]);
    assert_eq!(active_field(&workbench, &editor, cx), None, "the last field ends the template");
    assert_eq!(drawn_fields(&editor, cx), 0);

    undo(cx);
    assert_eq!(text(&editor, cx), "for (let j = 0; j < .length; j++) {\n  \n}\n");
    undo(cx);
    assert_eq!(text(&editor, cx), "for (let i = 0; i < .length; i++) {\n  \n}\n");
    undo(cx);
    assert_eq!(text(&editor, cx), "fori\n");
    close_backend_project(cx);
}

// template fields › Esc first drops the field's selection, then leaves the template; Shift+Tab goes
// back a field, and the caret leaving the field ends the template.
#[gpui_kit::test]
async fn esc_first_drops_the_fields_selection_then_leaves_the_template(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/loop.ts", "\n")]);
    let editor = edit_at(&workbench, &root.join("src/loop.ts"), 0, cx);
    cx.simulate_input("fori");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(selections(&editor, cx).iter().all(|(anchor, head)| anchor == head), "the selection is dropped");
    assert_eq!(active_field(&workbench, &editor, cx), Some(0), "the template goes on");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(active_field(&workbench, &editor, cx), None);
    assert_eq!(drawn_fields(&editor, cx), 0);
    // Outside a template Escape is the editor's: the text stays.
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&editor, cx), "for (let i = 0; i < .length; i++) {\n  \n}\n");

    // Shift+Tab goes back; the caret put elsewhere ends the template.
    let line = len("for (let i = 0; i < .length; i++) {\n  \n}\n");
    editor.update(cx, |e, cx| e.select(line, line, cx));
    cx.simulate_input("fori");
    cx.simulate_keystrokes("tab enter");
    cx.run_until_parked();
    assert_eq!(active_field(&workbench, &editor, cx), Some(1));
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    assert_eq!(selected_texts(&editor, cx), ["i", "i", "i"]);
    editor.update(cx, |e, cx| e.select(0, 0, cx));
    cx.run_until_parked();
    assert_eq!(active_field(&workbench, &editor, cx), None);
    close_backend_project(cx);
}

// Tab expansion › does not expand code templates inside comments
#[gpui_kit::test]
async fn does_not_expand_code_templates_inside_comments(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/c.ts", "// log\n")]);
    let editor = edit_at(&workbench, &root.join("src/c.ts"), len("// log"), cx);
    // The highlights come after a pause the executor's clock has to pass.
    wait_long(cx, "the highlights", |cx| editor.read_with(cx, |e, _| !e.highlights().is_empty()));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(!text(&editor, cx).contains("console.log"), "{}", text(&editor, cx));
    assert_eq!(active_field(&workbench, &editor, cx), None);
    close_backend_project(cx);
}

// Tab expansion › expands todo with the language's comment start
#[gpui_kit::test]
async fn expands_todo_with_the_languages_comment_start(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/a.py", "x = 1\n")]);
    let editor = edit_at(&workbench, &root.join("src/a.py"), len("x = 1\n"), cx);
    cx.simulate_input("todo");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "x = 1\n# TODO: ");
    assert_eq!(selections(&editor, cx), [(len("x = 1\n# TODO: "), len("x = 1\n# TODO: "))]);
    close_backend_project(cx);
}

const USER_TEMPLATES: &str = r#"{"templates":{"user.zz":{"id":"user.zz","group":"User","abbrev":"zz","description":"Zebra","body":"zebra($END$);","contexts":["typescript"]}},"deleted":["ts.log"]}"#;

// What the Live Templates page stores is what Tab expands: a template added there expands, a built-in
// deleted there does not, and with "Expand abbreviations with Tab" off nothing does.
#[gpui_kit::test]
async fn tab_expands_the_live_templates_the_settings_stored(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    store(LIVE_TEMPLATES_KEY, USER_TEMPLATES, cx);
    cx.simulate_input("zz");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  zebra();\n}\n");

    caret_at_line_end(&editor, 1, cx);
    cx.simulate_input(" log");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(!text(&editor, cx).contains("console.log"), "the deleted built-in stays text: {}", text(&editor, cx));

    store(LIVE_TEMPLATE_OPTIONS_KEY, r#"{"expandWithTab":false,"showInCompletion":true}"#, cx);
    caret_at_line_end(&editor, 1, cx);
    cx.simulate_input(" zz");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let line = text(&editor, cx).lines().nth(1).map(str::to_owned).unwrap_or_default();
    assert!(line.contains(" zz") && line.matches("zebra(").count() == 1, "the option keeps zz as text: {line}");
    close_backend_project(cx);
}

const EDITED_TEMPLATES: &str = r#"{"templates":{"ts.log":{"id":"ts.log","group":"JavaScript / TypeScript","abbrev":"log","description":"console.log","body":"edited($END$);","contexts":["typescript"]},"user.zq":{"id":"user.zq","group":"User","abbrev":"zq","body":"zq($END$);","contexts":["typescript"],"enabled":false}},"deleted":[]}"#;

// A built-in edited on the Live Templates page expands as edited, and a template switched off there
// does not expand.
#[gpui_kit::test]
async fn an_edited_built_in_expands_as_edited_and_a_disabled_template_does_not(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    store(LIVE_TEMPLATES_KEY, EDITED_TEMPLATES, cx);
    cx.simulate_input("log");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  edited();\n}\n");
    caret_at_line_end(&editor, 1, cx);
    cx.simulate_input(" zq");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let line = text(&editor, cx).lines().nth(1).map(str::to_owned).unwrap_or_default();
    assert!(line.starts_with("  edited(); zq") && !line.contains("zq("), "the disabled template stays text: {line}");
    close_backend_project(cx);
}

// `clipboard()` is the one template function that reads outside the editor: its value is the
// clipboard's text, selected as the field it fills.
#[gpui_kit::test]
async fn a_clipboard_variable_takes_the_clipboard_text(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    let template = r#"{"templates":{"user.cl":{"id":"user.cl","group":"User","abbrev":"cl","body":"paste($CLIP$);$END$","contexts":["typescript"],"variables":[{"name":"CLIP","expression":"clipboard()"}]}},"deleted":[]}"#;
    store(LIVE_TEMPLATES_KEY, template, cx);
    cx.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_string("hello".to_owned())));
    cx.simulate_input("cl");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  paste(hello);\n}\n");
    assert_eq!(selected_texts(&editor, cx), ["hello"]);
    close_backend_project(cx);
}

// Tab expansion › expands postfix templates replacing the expression; a postfix template disabled on
// the Postfix Completion page, or postfix completion turned off there, leaves `.for` as text.
#[gpui_kit::test]
async fn expands_postfix_templates_replacing_the_expression(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.rs", "fn main() {\n    \n}\n")]);
    let editor = edit_at(&workbench, &root.join("src/main.rs"), len("fn main() {\n    "), cx);
    cx.simulate_input("items.iter().for");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "fn main() {\n    for item in items.iter() {\n        \n    }\n}\n");
    assert_eq!(selected_texts(&editor, cx), ["item"]);
    assert_eq!(active_field(&workbench, &editor, cx), Some(0));
    undo(cx);
    assert_eq!(text(&editor, cx), "fn main() {\n    items.iter().for\n}\n");

    let typed = len("fn main() {\n    items.iter().for");
    let off = r#"{"enabled":true,"expandWithTab":true,"showInCompletion":true,"disabled":["rust.for"]}"#;
    store(POSTFIX_KEY, off, cx);
    editor.update(cx, |e, cx| e.select(typed, typed, cx));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(!text(&editor, cx).contains("for item in"), "{}", text(&editor, cx));
    undo(cx);

    store(POSTFIX_KEY, r#"{"enabled":false,"expandWithTab":true,"showInCompletion":true,"disabled":[]}"#, cx);
    editor.update(cx, |e, cx| e.select(typed, typed, cx));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(!text(&editor, cx).contains("for item in"), "{}", text(&editor, cx));
    close_backend_project(cx);
}

// Two templates named alike: Tab opens Choose Template under the caret with both, the arrows and Enter
// pick one, and the keyboard is back in the editor.
#[gpui_kit::test]
async fn several_templates_of_one_abbreviation_open_choose_template_at_the_caret(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    let second = r#"{"templates":{"user.log":{"id":"user.log","group":"User","abbrev":"log","description":"Debug log","body":"debug($END$);","contexts":["typescript"]}},"deleted":[]}"#;
    store(LIVE_TEMPLATES_KEY, second, cx);
    cx.simulate_input("log");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    let chooser = workbench.read_with(cx, |w, _| w.template_chooser()).expect("Choose Template opens");
    assert_eq!(chooser.read_with(cx, |c, cx| c.title(cx)), "Choose Template");
    assert_eq!(chooser.read_with(cx, |c, _| c.lines()), ["log · console.log", "log · Debug log"]);
    assert!(cx.debug_bounds("popup-anchored").is_some(), "the chooser is anchored at the caret");
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    assert_eq!(text(&editor, cx), "function f() {\n  debug();\n}\n");
    cx.simulate_input("x");
    assert_eq!(text(&editor, cx), "function f() {\n  debug(x);\n}\n", "the editor has the keyboard again");

    // Escape closes the chooser and leaves the abbreviation.
    let end = len("function f() {\n  debug(x);");
    editor.update(cx, |e, cx| e.select(end, end, cx));
    cx.simulate_input(" log");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_some()));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    assert_eq!(text(&editor, cx), "function f() {\n  debug(x); log\n}\n");
    close_backend_project(cx);
}

// ⌘J Insert Live Template › opens a lookup restricted to live templates and ends with it: through the
// registry it lists the templates for the word before the caret, typing narrows them and Enter
// expands the selected one; through its shortcut without a word it lists every applicable template,
// and Escape ends it with the text unchanged.
#[gpui_kit::test]
async fn opens_a_lookup_restricted_to_live_templates_and_ends_with_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    cx.simulate_input("lo");
    run_action(cx, "code.insertLiveTemplate", None);
    assert!(lookup_rows(&editor, cx).contains(&"log — console.log".to_owned()), "{:?}", lookup_rows(&editor, cx));
    cx.simulate_input("g");
    cx.run_until_parked();
    assert_eq!(lookup_rows(&editor, cx).first().map(String::as_str), Some("log — console.log"));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  console.log();\n}\n");
    let caret = len("function f() {\n  console.log(");
    assert_eq!(selections(&editor, cx), [(caret, caret)]);
    assert!(lookup_rows(&editor, cx).is_empty(), "the lookup ended");

    let end = len("function f() {\n  console.log();");
    editor.update(cx, |e, cx| e.select(end, end, cx));
    cx.simulate_input("\n  ");
    cx.simulate_keystrokes(&primary("j"));
    cx.run_until_parked();
    let rows = lookup_rows(&editor, cx);
    assert!(
        rows.iter().any(|row| row.starts_with("fori")) && rows.iter().any(|row| row.starts_with("log")),
        "{rows:?}"
    );
    let before = text(&editor, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(lookup_rows(&editor, cx).is_empty());
    assert_eq!(text(&editor, cx), before);
    close_backend_project(cx);
}

// Insert Live Template's lookup takes no commit character, moved over with the arrows or not: a dot
// typed after ↓ is typed, ends the lookup and expands nothing.
#[gpui_kit::test]
async fn a_dot_typed_in_the_template_lookup_ends_it_without_expanding(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_window(cx, &[("src/main.ts", FUNCTION)]);
    let editor = edit_at(&workbench, &root.join("src/main.ts"), len("function f() {\n  "), cx);
    cx.simulate_input("lo");
    run_action(cx, "code.insertLiveTemplate", None);
    assert!(lookup_rows(&editor, cx).len() > 1, "{:?}", lookup_rows(&editor, cx));
    cx.simulate_keystrokes("down");
    cx.simulate_input(".");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "function f() {\n  lo.\n}\n");
    assert!(lookup_rows(&editor, cx).is_empty(), "the lookup ended");
    assert_eq!(active_field(&workbench, &editor, cx), None);
    close_backend_project(cx);
}
