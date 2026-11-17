//! Java's `.var` postfix template in a project window's editor: Tab on `expr.var` runs Introduce
//! Variable on the expression, ended as a statement, and inserts the `var` declaration when no
//! Introduce Variable is there to run. The action's handler is the test's own, which records what
//! it was run on; no language server runs (the default mode starts none).

use super::*;
use crate::platform::actions::ActionRegistry;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

type Opened<'a> = (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext);

const INTRODUCE_VARIABLE: &str = "refactor.extractVariable";

fn java_window<'a>(cx: &'a mut TestAppContext, text: &str) -> Opened<'a> {
    let (folder, root, workbench, cx) = trusted_project(cx, &[("src/A.java", text)]);
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

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

/// What Introduce Variable saw when it ran: the editor's text and its selected text.
type Seen = Rc<RefCell<Option<(String, String)>>>;

/// Puts a handler for Introduce Variable in place of the window's, recording what it runs on.
fn record_introduce_variable(cx: &mut VisualTestContext) -> Seen {
    let seen: Seen = Rc::default();
    let record = seen.clone();
    cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().handle(INTRODUCE_VARIABLE, move |_, window, cx| {
            let editor = active_editor(window, cx).ok_or_else(|| anyhow::anyhow!("no editor is active"))?;
            let read = editor.read(cx);
            *record.borrow_mut() = Some((read.text(), read.selected_text()));
            Ok(())
        });
    });
    seen
}

// Java .var › runs Introduce Variable on the expression, ended as a statement
#[gpui_kit::test]
async fn runs_introduce_variable_on_the_expression_ended_as_a_statement(cx: &mut TestAppContext) {
    let _home = start(cx);
    let line = "class A { void f() {\n  list.get(0).var";
    let (_folder, root, workbench, cx) = java_window(cx, &format!("{line}\n}} }}\n"));
    let editor = edit_at(&workbench, &root.join("src/A.java"), len(line), cx);
    let seen = record_introduce_variable(cx);
    cx.simulate_keystrokes("tab");
    // The action runs once the key's own update is over.
    settle(cx, |_| seen.borrow().is_some());
    let ran = seen.borrow().clone();
    assert_eq!(
        ran,
        Some(("class A { void f() {\n  list.get(0);\n} }\n".to_owned(), "list.get(0)".to_owned())),
        "Introduce Variable runs on the selected expression of the ended statement"
    );
    assert_eq!(text(&editor, cx), "class A { void f() {\n  list.get(0);\n} }\n");
    assert!(workbench.read_with(cx, |w, _| w.template_session(&editor).is_none()), "no template is filled in");
    close_backend_project(cx);
}

// Java .var › falls back to the var template without Introduce Variable
#[gpui_kit::test]
async fn falls_back_to_the_var_template_without_introduce_variable(cx: &mut TestAppContext) {
    let _home = start(cx);
    let line = "class A { void f() {\n  name().var";
    let (_folder, root, workbench, cx) = java_window(cx, &format!("{line}\n}} }}\n"));
    let editor = edit_at(&workbench, &root.join("src/A.java"), len(line), cx);
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().unregister(INTRODUCE_VARIABLE));
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "class A { void f() {\n  var name = name();\n} }\n");
    // The name is the template's first field, selected.
    assert_eq!(editor.read_with(cx, |e, _| e.selected_text()), "name");
    close_backend_project(cx);
}

/// Not from a spec: an Introduce Variable that is registered but whose condition does not hold
/// declines: the `;` the statement got goes again and the `var` declaration replaces the expression.
#[gpui_kit::test]
async fn a_declined_introduce_variable_leaves_the_var_declaration(cx: &mut TestAppContext) {
    let _home = start(cx);
    let line = "class A { void f() {\n  name().var";
    let (_folder, root, workbench, cx) = java_window(cx, &format!("{line}\n}} }}\n"));
    let editor = edit_at(&workbench, &root.join("src/A.java"), len(line), cx);
    // A context key nothing sets: the action stays registered and never runs.
    cx.update(|_, cx| {
        let registry = cx.global_mut::<ActionRegistry>();
        let mut meta = registry.get(INTRODUCE_VARIABLE).expect("Introduce Variable is registered").meta.clone();
        meta.when = Some("proofs39NeverSet".to_owned());
        registry.register(crate::platform::actions::Action::new(meta, |_, _, _| Ok(())));
    });
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(text(&editor, cx), "class A { void f() {\n  var name = name();\n} }\n");
    close_backend_project(cx);
}
