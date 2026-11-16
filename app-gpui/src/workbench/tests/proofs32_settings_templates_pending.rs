//! Not declared: the flow below needs a feature the application does not have yet.
//!
//! - File templates in New ▸: the New popup lists template rows only as contributed `templates.new.*`
//!   actions, and none is registered (`project_view/new_popup_model.rs`, `workbench/context_menus.rs`).
//!   The test needs the popup to list the file templates of the scheme in use
//!   (`templates::files_store::FileTemplates::load(root, read)`) and to create the file from the
//!   chosen one through `templates::files_create::plan_creation`.
//!
//! Declare it in `workbench/mod.rs`'s `mod tests` once it exists.

use super::proofs32_settings_templates_fixtures::{open_template_page, template_project, type_into};
use super::*;
use crate::settings_view::pages::editor_file_templates::{self, FileField, FileTemplatesPageView};

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
