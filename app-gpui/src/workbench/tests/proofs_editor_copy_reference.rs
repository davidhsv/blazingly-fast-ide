use super::*;
use crate::project_view::copy_path_popup::REFERENCE;

const JAVA: &str = "package com.acme;\n\npublic class Orders {\n    public void place(int count) {\n    }\n}\n";
const RUST: &str = "pub fn place() {}\n";

/// Opens `file` in the editor with the caret at the first `token` plus `past` characters.
fn edit_at(workbench: &Entity<Workbench>, file: &std::path::Path, token: &str, past: u32, cx: &mut VisualTestContext) {
    workbench.update_in(cx, |w, window, cx| w.open_project_file(file.to_owned(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let at = editor.read_with(cx, |e, _| e.text().find(token).expect("the token is in the file") as u32) + past;
    editor.update(cx, |e, cx| e.select(at, at, cx));
    cx.run_until_parked();
}

/// Opens Copy Path/Reference… and waits until its Reference row is resolved; returns that row.
fn resolved_reference(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> String {
    run_action(cx, "editor.copyPathReference", None);
    let popup = workbench.read_with(cx, |w, _| w.copy_path_popup()).expect("the Copy Path popup");
    let row = |cx: &mut VisualTestContext| {
        let lines = popup.read_with(cx, |popup, _| popup.lines());
        lines.into_iter().find(|line| line.starts_with("Reference | ")).expect("the Reference row")
    };
    settle(cx, |cx| !row(cx).contains("Resolving reference"));
    row(cx)
}

/// Chooses the Reference row of the open Copy Path popup with the keyboard.
fn choose_reference(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let popup = workbench.read_with(cx, |w, _| w.copy_path_popup()).expect("the Copy Path popup");
    for _ in 0..8 {
        if popup.read_with(cx, |popup, _| popup.selected()) == Some(REFERENCE) {
            break;
        }
        cx.simulate_keystrokes("down");
    }
    assert_eq!(popup.read_with(cx, |popup, _| popup.selected()), Some(REFERENCE));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
}

/// With the caret on a declaration's name, Copy Reference copies its fully qualified name from the file's
/// outline and the language's conventions (`com.acme.Orders#place` for a Java method, `crate::orders::place`
/// for a Rust function) and says so; off a name it copies the project-relative `path:line`.
#[gpui_kit::test]
async fn copy_reference_copies_the_qualified_name_of_the_declaration_at_the_caret(cx: &mut TestAppContext) {
    let _home = start(cx);
    let java = "src/main/java/com/acme/Orders.java";
    let (_folder, root, workbench, cx) = trusted_project(cx, &[(java, JAVA), ("src/orders.rs", RUST)]);
    cx.update(|window, _| window.activate_window());

    edit_at(&workbench, &root.join(java), "place", 2, cx);
    assert!(resolved_reference(&workbench, cx).starts_with("Reference | com.acme.Orders#place | "));
    choose_reference(&workbench, cx);
    assert!(workbench.read_with(cx, |w, _| w.copy_path_popup()).is_none());
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("com.acme.Orders#place"));
    assert_eq!(editor_hint(&workbench, cx).as_deref(), Some("Copied reference: com.acme.Orders#place"));

    // In the method's leading whitespace the reference is the caret's place.
    edit_at(&workbench, &root.join(java), "    public void", 1, cx);
    let place = format!("{java}:4");
    assert!(resolved_reference(&workbench, cx).starts_with(&format!("Reference | {place} | ")));
    choose_reference(&workbench, cx);
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()), Some(place));

    edit_at(&workbench, &root.join("src/orders.rs"), "place", 0, cx);
    assert!(resolved_reference(&workbench, cx).starts_with("Reference | crate::orders::place | "));
    choose_reference(&workbench, cx);
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("crate::orders::place"));
    close_backend_project(cx);
}

/// Edit › Copy Reference is an action of its own (⌥⇧⌘C): run with the caret on a declaration's name, it puts
/// the qualified name on the clipboard without a popup.
#[gpui_kit::test]
async fn the_copy_reference_action_copies_the_qualified_name_without_a_popup(cx: &mut TestAppContext) {
    let _home = start(cx);
    let java = "src/main/java/com/acme/Orders.java";
    let (_folder, root, workbench, cx) = trusted_project(cx, &[(java, JAVA)]);
    cx.update(|window, _| window.activate_window());
    edit_at(&workbench, &root.join(java), "place", 2, cx);
    run_action(cx, "editor.copyReference", None);
    let copied = |cx: &mut VisualTestContext| cx.read_from_clipboard().and_then(|item| item.text());
    settle(cx, |cx| copied(cx).is_some());
    assert_eq!(copied(cx).as_deref(), Some("com.acme.Orders#place"));
    assert!(workbench.read_with(cx, |w, _| w.copy_path_popup()).is_none());
    close_backend_project(cx);
}
