//! Editor › Live Templates, opened through File › Settings… in a project window and changed with
//! the mouse and the keyboard: an option's checkbox, Add Template, the abbreviation and the template
//! text typed, a template's checkbox, the filter, a built-in template's description and Remove
//! Template. Every change is read back from the UI storage, in the JSON the other app reads, while
//! Apply stays off and Reset hidden; Cancel keeps what was stored and Settings opened again shows it.
//!
//! Nothing here reaches the network or anything under the user's home: the backend's folders are
//! under a temporary home and the UI storage is the test's own, in memory.

use super::proofs32_settings_templates_fixtures::{
    click_template, nothing_staged, open_template_page, stored_template_json, template_project, type_into,
};
use super::*;
use crate::settings_view::pages::editor_live_templates::{self, FieldKind, LiveTemplatesPageView};
use crate::templates::live_store::{LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY};
use crate::ui::text_field::TextField;
use gpui_kit::Focusable;
use serde_json::json;

const PAGE: &str = editor_live_templates::PAGE;

fn editor_field(
    page: &Entity<LiveTemplatesPageView>,
    kind: FieldKind,
    cx: &mut VisualTestContext,
) -> Entity<TextField> {
    let field = page.read_with(cx, |page, _| page.editor_field(&kind));
    field.unwrap_or_else(|| panic!("the {kind:?} field is shown"))
}

// A template added on the page, its abbreviation and text typed and its checkbox cleared, is stored
// at once with the option changed; nothing is staged, Cancel keeps it all, and Settings opened again
// lists it under User, filtered by its abbreviation.
#[gpui_kit::test]
async fn a_live_template_added_and_typed_on_the_page_is_stored_at_once_and_cancel_keeps_it(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("src/main.rs", "fn main() {}\n")]);
    let (dialog, page) = open_template_page::<LiveTemplatesPageView>(&workbench, PAGE, cx);

    click_on(cx, "settings-live-showInCompletion");
    let options = json!({ "expandWithTab": true, "showInCompletion": false });
    assert_eq!(stored_template_json(LIVE_TEMPLATE_OPTIONS_KEY, cx), options);
    click_on(cx, "settings-live-add");
    let id = page.read_with(cx, |page, _| page.selected().map(str::to_owned)).expect("the new template is selected");
    type_into(&editor_field(&page, FieldKind::Abbreviation, cx), "logd", cx);
    let body = page.read_with(cx, |page, _| page.body_area()).expect("the template text is shown");
    cx.update(|window, cx| {
        body.update(cx, |area, cx| area.select_all(cx));
        window.focus(&body.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.simulate_input("dbg!($VALUE$);");
    cx.simulate_keystrokes("tab");
    cx.simulate_input("$END$");
    cx.run_until_parked();
    type_into(&editor_field(&page, FieldKind::Expression("VALUE".to_owned()), cx), "clipboard()", cx);
    click_on(cx, "settings-live-skip-VALUE");
    // The new template is listed last: the filter brings it into view before its checkbox is clicked.
    let filter = page.read_with(cx, |page, _| page.filter_field().clone());
    type_into(&filter, "logd", cx);
    click_template(cx, format!("settings-live-enabled-{id}"));

    let stored = stored_template_json(LIVE_TEMPLATES_KEY, cx)["templates"][id.as_str()].clone();
    assert_eq!(stored["abbrev"], json!("logd"));
    assert_eq!(stored["body"], json!("dbg!($VALUE$);\t$END$"));
    assert_eq!(stored["group"], json!("User"));
    assert_eq!(stored["enabled"], json!(false));
    let variable = json!({ "name": "VALUE", "expression": "clipboard()", "skipIfDefined": true });
    assert_eq!(stored["variables"], json!([variable]));
    nothing_staged(&dialog, cx);

    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    assert_eq!(stored_template_json(LIVE_TEMPLATES_KEY, cx)["templates"][id.as_str()]["abbrev"], json!("logd"));
    let (_, again) = open_template_page::<LiveTemplatesPageView>(&workbench, PAGE, cx);
    let filter = again.read_with(cx, |page, _| page.filter_field().clone());
    type_into(&filter, "logd", cx);
    assert_eq!(again.read_with(cx, |page, cx| page.tree_lines(cx)), ["User", "  [ ] logd | "]);
    let options = again.read_with(cx, |page, cx| page.editor_lines(cx));
    assert_eq!(options[1], "[ ] Show live templates in code completion");
}

// A built-in template's description typed on the page is stored as the user's edit of it and marks
// it changed, and Restore Default drops the edit; its contexts, group and scope are stored as edits
// too; Remove Template hides it, which is stored as well, and OK closes the dialog at once.
#[gpui_kit::test]
async fn a_built_in_live_template_edited_and_removed_on_the_page_is_stored_at_once(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("Main.java", "class Main {}\n")]);
    let (dialog, page) = open_template_page::<LiveTemplatesPageView>(&workbench, PAGE, cx);
    let filter = page.read_with(cx, |page, _| page.filter_field().clone());
    type_into(&filter, "psvm", cx);
    click_on(cx, "settings-live-item-java.psvm");
    type_into(&editor_field(&page, FieldKind::Description, cx), "Main method", cx);
    let stored = stored_template_json(LIVE_TEMPLATES_KEY, cx);
    assert_eq!(stored["templates"]["java.psvm"]["description"], json!("Main method"));
    assert_eq!(page.read_with(cx, |page, cx| page.tree_lines(cx))[1], "> [x] psvm * | Main method");
    nothing_staged(&dialog, cx);
    click_on(cx, "settings-live-restore");
    assert_eq!(stored_template_json(LIVE_TEMPLATES_KEY, cx), json!({ "templates": {}, "deleted": [] }));
    type_into(&editor_field(&page, FieldKind::Contexts, cx), "java, kotlin", cx);
    type_into(&editor_field(&page, FieldKind::Group, cx), "Mine", cx);
    click_on(cx, "settings-live-scope");
    let edit = stored_template_json(LIVE_TEMPLATES_KEY, cx)["templates"]["java.psvm"].clone();
    assert_eq!((edit["contexts"].clone(), edit["group"].clone()), (json!(["java", "kotlin"]), json!("Mine")));
    assert_eq!(edit["scope"], json!("any"));
    nothing_staged(&dialog, cx);
    click_on(cx, "settings-live-remove");
    let hidden = json!({ "templates": {}, "deleted": ["java.psvm"] });
    assert_eq!(stored_template_json(LIVE_TEMPLATES_KEY, cx), hidden);
    assert!(page.read_with(cx, |page, cx| page.tree_lines(cx)).is_empty());
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none());
}
