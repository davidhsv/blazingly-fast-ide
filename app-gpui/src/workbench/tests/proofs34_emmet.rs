//! Update Tag with Emmet in a project window: the prompt for the tag at the caret, the tag pair
//! edited in the editor, what the window says when there is nothing to update, and the editor
//! menu's row for markup files only.

use super::*;
use crate::platform::actions::execute_action_or_throw;
use crate::run_producers::context_run::{ContextMenu, MenuGroupContext};

const DOC: &str = "<div class=\"a\" id=\"x\">\n  <p>hi</p>\n</div>\n";

/// Opens `file` in the editor, gives it the keyboard in the active window and waits until the
/// context keys say so.
fn focus_file(workbench: &Entity<Workbench>, file: &std::path::Path, cx: &mut VisualTestContext) -> Entity<Editor> {
    let path = file.to_path_buf();
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path, window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == file)));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("the editor");
    cx.update(|window, cx| {
        window.activate_window();
        let focus = editor.focus_handle(cx);
        window.focus(&focus, cx);
    });
    wait_long(cx, "the editor has the keyboard", |cx| context_is_on(cx, "editorFocus"));
    editor
}

fn text(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.text())
}

/// The rows of the editor menu's groups for `file`.
fn editor_group_actions(
    workbench: &Entity<Workbench>,
    file: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Vec<String> {
    let paths = [text_of(file)];
    let rows = workbench.update_in(cx, |w, window, cx| {
        let ctx = MenuGroupContext { paths: &paths, dirs: &[false], text: Some(""), line: Some(0) };
        w.group_rows(ContextMenu::Editor, &ctx, None, window, cx)
    });
    rows.iter().filter_map(|row| row.action_id().map(str::to_owned)).collect()
}

/// The abbreviation renames the element around the caret (both its tags), adds its class and sets
/// its id and attribute; the prompt names the tag and closes.
#[gpui_kit::test]
async fn update_tag_with_emmet_renames_the_element_and_sets_its_attributes(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("index.html", DOC)]);
    let editor = focus_file(&workbench, &root.join("index.html"), cx);
    wait_long(cx, "the editor is an HTML editor", |cx| context_text(cx, "editorLangId").as_deref() == Some("html"));
    editor.update(cx, |editor, cx| editor.select(3, 3, cx));
    run_action(cx, "emmet.updateTag", None);
    let dialog = workbench.read_with(cx, |w, _| w.update_tag_dialog()).expect("the prompt is open");
    assert!(dialog.update(cx, |dialog, cx| dialog.label(cx)).starts_with("Abbreviation for <div>"));
    cx.simulate_input("section.b#y[data-k=v]");
    cx.simulate_keystrokes("enter");
    let expected = "<section class=\"a b\" id=\"y\" data-k=\"v\">\n  <p>hi</p>\n</section>\n";
    wait_long(cx, "the tag pair is updated", |cx| text(&editor, cx) == expected);
    assert!(workbench.read_with(cx, |w, _| w.update_tag_dialog().is_none()), "the prompt closed");
    assert!(context_is_on(cx, "editorFocus"), "the editor has the keyboard again");
}

/// Outside a markup editor the action is off; in one, a caret outside every tag is told so and no
/// prompt opens.
#[gpui_kit::test]
async fn update_tag_is_off_outside_markup_and_a_caret_outside_any_tag_is_told(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files = [("src/main.rs", "fn main() {}\n"), ("plain.html", "just text\n<p>x</p>\n")];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    focus_file(&workbench, &root.join("src/main.rs"), cx);
    let ran = cx.update(|window, cx| execute_action_or_throw("emmet.updateTag", None, window, cx));
    assert!(ran.is_err(), "not in a Rust file");
    let editor = focus_file(&workbench, &root.join("plain.html"), cx);
    wait_long(cx, "the editor is an HTML editor", |cx| context_text(cx, "editorLangId").as_deref() == Some("html"));
    editor.update(cx, |editor, cx| editor.select(2, 2, cx));
    run_action(cx, "emmet.updateTag", None);
    wait_for_notice(&workbench, "No tag at the caret", cx);
    assert!(workbench.read_with(cx, |w, _| w.update_tag_dialog().is_none()));
}

/// An abbreviation of several elements leaves the tag as it was and says why.
#[gpui_kit::test]
async fn an_abbreviation_of_several_elements_leaves_the_tag_and_warns(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("index.html", DOC)]);
    let editor = focus_file(&workbench, &root.join("index.html"), cx);
    wait_long(cx, "the editor is an HTML editor", |cx| context_text(cx, "editorLangId").as_deref() == Some("html"));
    editor.update(cx, |editor, cx| editor.select(3, 3, cx));
    run_action(cx, "emmet.updateTag", None);
    cx.simulate_input("ul>li");
    cx.simulate_keystrokes("enter");
    wait_for_notice(&workbench, "Cannot update the tag", cx);
    assert_eq!(text(&editor, cx), DOC);
}

/// Escape closes the prompt: nothing changes and the editor has the keyboard again.
#[gpui_kit::test]
async fn escape_closes_the_update_tag_prompt_without_editing(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("index.html", DOC)]);
    let editor = focus_file(&workbench, &root.join("index.html"), cx);
    wait_long(cx, "the editor is an HTML editor", |cx| context_text(cx, "editorLangId").as_deref() == Some("html"));
    let inside = DOC.find("hi").expect("the paragraph's text") as u32;
    editor.update(cx, |editor, cx| editor.select(inside, inside, cx));
    run_action(cx, "emmet.updateTag", None);
    let dialog = workbench.read_with(cx, |w, _| w.update_tag_dialog()).expect("the prompt is open");
    assert!(dialog.update(cx, |dialog, cx| dialog.label(cx)).starts_with("Abbreviation for <p>"));
    cx.simulate_input("span");
    cx.simulate_keystrokes("escape");
    assert!(workbench.read_with(cx, |w, _| w.update_tag_dialog().is_none()));
    assert_eq!(text(&editor, cx), DOC);
    wait_long(cx, "the editor has the keyboard", |cx| context_is_on(cx, "editorFocus"));
}

/// The editor menu's Emmet group offers Update Tag with Emmet for HTML-like files only.
#[gpui_kit::test]
async fn the_editor_menu_offers_update_tag_for_markup_files_only(cx: &mut TestAppContext) {
    let _home = start(cx);
    let files =
        [("index.html", DOC), ("page.vue", "<template><p>x</p></template>\n"), ("src/main.rs", "fn main() {}\n")];
    let (_folder, root, workbench, cx) = trusted_project(cx, &files);
    for markup in ["index.html", "page.vue"] {
        let actions = editor_group_actions(&workbench, &root.join(markup), cx);
        assert!(actions.contains(&"emmet.updateTag".to_owned()), "{markup}: {actions:?}");
    }
    let actions = editor_group_actions(&workbench, &root.join("src/main.rs"), cx);
    assert!(!actions.contains(&"emmet.updateTag".to_owned()), "{actions:?}");
}
