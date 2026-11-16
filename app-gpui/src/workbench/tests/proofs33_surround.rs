//! Surround With and Surround with Live Template in a project window's editor, through the registry
//! and their shortcuts: what the chooser lists for a selection and for a caret, what a mnemonic picks,
//! the statement the backend finds at the caret, Configure Live Templates…, and the condition the
//! template actions share (an editable text editor).
//!
//! The files are small ones in a temporary project, in languages whose grammar is bundled; no
//! language server runs and the UI storage is the test's own, in memory.

use super::proofs18_e2e_fixtures::primary;
use super::*;
use crate::platform::actions::execute_action_or_throw;
use crate::templates::chooser_view::TemplateChooser;
use std::path::Path;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

fn surround_window<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> Opened<'a> {
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

/// The chooser, once it is open.
fn open_chooser(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TemplateChooser> {
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.template_chooser().is_some()));
    workbench.read_with(cx, |w, _| w.template_chooser()).expect("the chooser is open")
}

fn lines(chooser: &Entity<TemplateChooser>, cx: &mut VisualTestContext) -> Vec<String> {
    chooser.read_with(cx, |c, _| c.lines())
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.text())
}

fn selected_text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |e, _| e.selected_text())
}

fn at(text: &str, needle: &str) -> u32 {
    text.find(needle).map_or(0, |byte| text[..byte].encode_utf16().count() as u32)
}

/// A shortcut with ⌥ and the platform's primary modifier.
fn primary_alt(key: &str) -> String {
    if cfg!(target_os = "macos") { format!("cmd-alt-{key}") } else { format!("ctrl-alt-{key}") }
}

const PYTHON: &str = "def f():\n    a()\n    b()\n";
const JAVA: &str = "class A {\n  void f() {\n    a();\n  }\n}\n";

// surround with › wraps the selected lines: Surround With through the registry lists Python's
// surrounders, its mnemonic `1` picks `if`, and the caret is in the condition.
#[gpui_kit::test]
async fn wraps_the_selected_lines(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/f.py", PYTHON)]);
    let editor = edit(&workbench, &root.join("src/f.py"), at(PYTHON, "a()"), at(PYTHON, "b()") + 3, cx);
    run_action(cx, "code.surroundWith", None);
    let chooser = open_chooser(&workbench, cx);
    assert_eq!(chooser.read_with(cx, |c, cx| c.title(cx)), "Surround With");
    assert_eq!(lines(&chooser, cx).first().map(String::as_str), Some("1. if"));
    cx.simulate_keystrokes("1");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    assert_eq!(text(&editor, cx), "def f():\n    if :\n        a()\n        b()\n    \n");
    let caret = "def f():\n    if ".len() as u32;
    assert_eq!(editor.read_with(cx, |e, _| e.selections().to_vec()), [(caret, caret)]);
    close_backend_project(cx);
}

// surround with › lists the language's surrounders with numeric mnemonics; a mnemonic key picks: with a
// caret on a one-line statement and the shortcut, the chooser opens once the backend has answered,
// expression surrounders are not offered, and `9` wraps the line in `synchronized`.
#[gpui_kit::test]
async fn lists_the_languages_surrounders_with_numeric_mnemonics_and_a_mnemonic_key_picks(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/A.java", JAVA)]);
    let caret = at(JAVA, "a();");
    let editor = edit(&workbench, &root.join("src/A.java"), caret, caret, cx);
    cx.simulate_keystrokes(&primary_alt("t"));
    let chooser = open_chooser(&workbench, cx);
    let shown = lines(&chooser, cx);
    assert_eq!(shown[..3], ["1. if", "2. if / else", "3. while"]);
    assert!(!shown.iter().any(|line| line.contains("(expr)")));
    assert!(shown.contains(&"-- Live templates".to_owned()));
    assert_eq!(shown.last().map(String::as_str), Some("Configure Live Templates…"));
    cx.simulate_keystrokes("9");
    cx.run_until_parked();
    assert!(text(&editor, cx).contains("synchronized (this) {"), "{}", text(&editor, cx));
    assert_eq!(selected_text(&editor, cx), "this");
    close_backend_project(cx);
}

// The backend's statement at the caret: a statement over several whole lines is what Surround With
// wraps, its lines indented one level inside.
#[gpui_kit::test]
async fn a_caret_on_a_multi_line_statement_surrounds_the_whole_statement(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "class A {\n  void f() {\n    if (x) {\n      a();\n    }\n  }\n}\n";
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/A.java", source)]);
    let caret = at(source, "if (x)");
    let editor = edit(&workbench, &root.join("src/A.java"), caret, caret, cx);
    run_action(cx, "code.surroundWith", None);
    let chooser = open_chooser(&workbench, cx);
    assert_eq!(lines(&chooser, cx).first().map(String::as_str), Some("1. if"));
    cx.simulate_keystrokes("1");
    cx.run_until_parked();
    assert_eq!(
        text(&editor, cx),
        "class A {\n  void f() {\n    if () {\n        if (x) {\n          a();\n        }\n    }\n  }\n}\n"
    );
    close_backend_project(cx);
}

// surround with › offers expression surrounders for a selection inside a line
#[gpui_kit::test]
async fn offers_expression_surrounders_for_a_selection_inside_a_line(cx: &mut TestAppContext) {
    let _home = start(cx);
    let source = "fn main() {\n    let x = a + b;\n}\n";
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/main.rs", source)]);
    let from = at(source, "a + b");
    let editor = edit(&workbench, &root.join("src/main.rs"), from, from + 5, cx);
    run_action(cx, "code.surroundWith", None);
    let chooser = open_chooser(&workbench, cx);
    assert_eq!(lines(&chooser, cx), ["1. (expr)", "2. !(expr)"]);
    cx.simulate_keystrokes("2");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "fn main() {\n    let x = !(a + b);\n}\n");
    close_backend_project(cx);
}

// Surround with Live Template, by its shortcut: only the `$SELECTION$` templates and Configure Live
// Templates…, which opens Settings on the Live Templates page; Escape before closes the chooser and
// leaves the text.
#[gpui_kit::test]
async fn surround_with_live_template_lists_the_selection_templates_and_configure_opens_the_settings(
    cx: &mut TestAppContext,
) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/A.java", JAVA)]);
    let from = at(JAVA, "a();");
    let editor = edit(&workbench, &root.join("src/A.java"), from, from + 4, cx);
    cx.simulate_keystrokes(&primary_alt("j"));
    let chooser = open_chooser(&workbench, cx);
    assert_eq!(chooser.read_with(cx, |c, cx| c.title(cx)), "Surround with Live Template");
    let shown = lines(&chooser, cx);
    assert!(!shown.contains(&"1. if".to_owned()), "no surrounders: {shown:?}");
    assert!(!shown.contains(&"-- Live templates".to_owned()));
    assert_eq!(shown.last().map(String::as_str), Some("Configure Live Templates…"));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    assert_eq!(text(&editor, cx), JAVA);

    // A `$SELECTION$` template the Live Templates page stored, picked by the mnemonic its abbreviation
    // gives, wraps the selection, and the caret goes to its end.
    let wrap = r#"{"templates":{"user.qq":{"id":"user.qq","group":"User","abbrev":"qq","description":"Wrap","body":"wrap($SELECTION$)$END$","contexts":["java"]}},"deleted":[]}"#;
    cx.update(|_, cx| {
        cx.global_mut::<crate::platform::storage::UiStorage>()
            .set(crate::templates::live_store::LIVE_TEMPLATES_KEY, wrap.to_owned())
    });
    cx.simulate_keystrokes(&primary_alt("j"));
    let chooser = open_chooser(&workbench, cx);
    assert!(lines(&chooser, cx).iter().any(|line| line.starts_with("qq")), "{:?}", lines(&chooser, cx));
    cx.simulate_keystrokes("q");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    let wrapped = JAVA.replace("a();", "wrap(a();)");
    assert_eq!(text(&editor, cx), wrapped);
    let end = at(&wrapped, "wrap(a();)") + "wrap(a();)".len() as u32;
    assert_eq!(editor.read_with(cx, |e, _| e.selections().to_vec()), [(end, end)]);
    cx.simulate_keystrokes(&primary("z"));
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), JAVA);
    editor.update(cx, |e, cx| e.select(from, from + 4, cx));

    run_action(cx, "code.surroundWithLiveTemplate", None);
    open_chooser(&workbench, cx);
    cx.simulate_keystrokes("end enter");
    settle(cx, |cx| settings_dialog_of(&workbench, cx).is_some());
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("editor.liveTemplates"));
    assert_eq!(text(&editor, cx), JAVA);
    close_backend_project(cx);
}

// The template actions want an editable text editor: in a read-only file they are not available,
// through the registry or their shortcuts, and Tab expands nothing.
#[gpui_kit::test]
async fn the_template_actions_are_unavailable_in_a_read_only_editor(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/main.ts", "log\n")]);
    let path = root.join("src/main.ts");
    let mut permissions = std::fs::metadata(&path).expect("the file exists").permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).expect("the file is made read-only");
    let editor = edit(&workbench, &path, 3, 3, cx);
    settle(cx, |cx| context_is_on(cx, "editorReadOnly"));
    for id in ["code.insertLiveTemplate", "code.surroundWith", "code.surroundWithLiveTemplate"] {
        let ran = cx.update(|window, cx| execute_action_or_throw(id, None, window, cx));
        assert!(ran.is_err(), "{id} is not available in a read-only editor");
    }
    cx.simulate_keystrokes(&primary_alt("t"));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()));
    assert_eq!(text(&editor, cx), "log\n");
    close_backend_project(cx);
}

const IF_STATEMENT: &str = "class A {\n  void f() {\n    if (x) {\n      a();\n    }\n  }\n}\n";

// The statement at the caret is asked of the structure service; typed text that reaches the
// editor before the answer makes the answer stale: it is dropped and Surround With wraps the caret's
// line alone, as with no statement.
#[gpui_kit::test]
async fn a_statement_answer_for_text_that_changed_meanwhile_is_dropped(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = surround_window(cx, &[("src/A.java", IF_STATEMENT)]);
    let caret = at(IF_STATEMENT, "if (x)");
    let editor = edit(&workbench, &root.join("src/A.java"), caret, caret, cx);
    cx.update(|window, cx| {
        execute_action_or_throw("code.surroundWith", None, window, cx).expect("Surround With runs");
        editor.update(cx, |e, cx| e.insert(None, "y", cx));
    });
    let chooser = open_chooser(&workbench, cx);
    assert_eq!(lines(&chooser, cx).first().map(String::as_str), Some("1. if"));
    cx.simulate_keystrokes("1");
    cx.run_until_parked();
    assert_eq!(
        text(&editor, cx),
        "class A {\n  void f() {\n    if () {\n        yif (x) {\n    }\n      a();\n    }\n  }\n}\n",
        "only the caret's line is wrapped"
    );
    close_backend_project(cx);
}

// Surround With waits for the structure service only while its editor stays the active one: another
// tab activated before the answer leaves both texts as they were and opens no chooser.
#[gpui_kit::test]
async fn surround_with_does_nothing_once_another_editor_is_active(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("src/A.java", IF_STATEMENT), ("src/B.java", JAVA)];
    let (_folder, root, workbench, cx) = surround_window(cx, &files);
    edit(&workbench, &root.join("src/B.java"), 0, 0, cx);
    let caret = at(IF_STATEMENT, "if (x)");
    let editor = edit(&workbench, &root.join("src/A.java"), caret, caret, cx);
    let (doc, version) = editor.read_with(cx, |e, _| (e.doc(), e.version()));
    cx.update(|window, cx| {
        execute_action_or_throw("code.surroundWith", None, window, cx).expect("Surround With runs");
        execute_action_or_throw("editor.nextTab", None, window, cx).expect("the next tab is selected");
    });
    let other = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    assert_ne!(other, editor, "another tab is active");
    let statement = ide_core::Pos::new(2, 0);
    let answered = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let service = cx.default_global::<crate::editor::structure::StructureService>();
            service.cache(doc).ranges(version, statement) != crate::editor::structure::Lookup::Ask
        })
    };
    settle(cx, answered);
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.template_chooser().is_none()), "no chooser opens");
    assert_eq!(text(&editor, cx), IF_STATEMENT);
    assert_eq!(text(&other, cx), JAVA);
    close_backend_project(cx);
}
