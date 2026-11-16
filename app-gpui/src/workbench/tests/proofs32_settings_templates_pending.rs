//! Not declared: the two flows below need features the application does not have yet.
//!
//! - Live templates in the editor: nothing in the workbench or the editor reads the live templates
//!   (`templates::live_store::LiveTemplates`) or expands an abbreviation with Tab; `templates::editor`
//!   has the expansion as a model only. The test needs Tab in a project editor to expand the
//!   abbreviation of a template stored under `LIVE_TEMPLATES_KEY`, reading the UI storage the page
//!   writes (and following it when it changes, as the page does with `observe_global::<UiStorage>`).
//! - File templates in New ▸: the New popup lists template rows only as contributed `templates.new.*`
//!   actions, and none is registered (`project_view/new_popup_model.rs`, `workbench/context_menus.rs`).
//!   The test needs the popup to list the file templates of the scheme in use
//!   (`templates::files_store::FileTemplates::load(root, read)`) and to create the file from the
//!   chosen one through `templates::files_create::plan_creation`.
//!
//! Declare it in `workbench/mod.rs`'s `mod tests` once both exist.

use super::proofs32_settings_templates_fixtures::{open_template_page, template_project, type_into};
use super::*;
use crate::settings_view::pages::editor_file_templates::{self, FileField, FileTemplatesPageView};
use crate::settings_view::pages::editor_live_templates::{self, FieldKind, LiveTemplatesPageView};
use gpui_kit::Focusable;

// A live template added on the page expands in an editor of the same window: its abbreviation typed
// and Tab give its text with the caret at `$END$`.
#[gpui_kit::test]
async fn a_live_template_added_on_the_page_expands_in_an_editor_of_the_window(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_project(cx, &[("src/main.rs", "fn main() {\n    \n}\n")]);
    let (_, page) = open_template_page::<LiveTemplatesPageView>(&workbench, editor_live_templates::PAGE, cx);
    click_on(cx, "settings-live-add");
    let field = |kind: FieldKind, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| page.editor_field(&kind)).expect("the field is shown")
    };
    type_into(&field(FieldKind::Abbreviation, cx), "logd", cx);
    type_into(&field(FieldKind::Contexts, cx), "rust", cx);
    let body = page.read_with(cx, |page, _| page.body_area()).expect("the text");
    cx.update(|window, cx| {
        body.update(cx, |area, cx| area.select_all(cx));
        window.focus(&body.focus_handle(cx), cx);
    });
    cx.simulate_input("dbg!($END$);");
    cx.run_until_parked();
    click_on(cx, "settings-ok");

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("src/main.rs"), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let at = "fn main() {\n    ".len() as u32;
    editor.update(cx, |e, cx| e.select(at, at, cx));
    cx.simulate_input("logd");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |e, _| e.text()), "fn main() {\n    dbg!();\n}\n");
}

// A file template added on the page is offered by New ▸ in the Project view of the same window.
#[gpui_kit::test]
async fn a_file_template_added_on_the_page_is_offered_by_new(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("README.md", "# p\n")]);
    let (_, page) = open_template_page::<FileTemplatesPageView>(&workbench, editor_file_templates::PAGE, cx);
    click_on(cx, "settings-filetemplates-add");
    let name = page.read_with(cx, |page, _| page.file_field(FileField::Name)).expect("the name field");
    type_into(&name, "Changelog Entry", cx);
    click_on(cx, "settings-ok");
    run_action(cx, "project.newInThisDirectory", None);
    let popup = workbench.read_with(cx, |w, _| w.new_popup()).expect("the New popup is open");
    assert!(popup.read_with(cx, |popup, _| popup.rows()).iter().any(|row| row.contains("Changelog Entry")));
}
