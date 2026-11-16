//! Editor › File and Code Templates, opened through File › Settings… in a project window and
//! changed with the mouse and the keyboard: the tabs, the scheme, an include's text, Add Template with
//! its name, extension, file name style, a kind added and Reformat according to style. Every change
//! is read back from the UI storage, in the JSON the other app reads, under the key of the scheme in
//! use; Apply stays off and Reset hidden, Cancel keeps what was stored and Settings opened again
//! shows it, the Project scheme included.
//!
//! Nothing here reaches the network or anything under the user's home: the backend's folders are
//! under a temporary home and the UI storage is the test's own, in memory.

use super::proofs32_settings_templates_fixtures::{
    nothing_staged, open_template_page, stored_template_json, template_project, type_into,
};
use super::*;
use crate::platform::paths::ipc_path;
use crate::settings_view::pages::editor_file_templates::{self, FileField, FileTemplatesPageView};
use crate::templates::files_store::{FILE_TEMPLATES_KEY, project_key, scheme_key};
use crate::ui::text_area::TextArea;
use gpui_kit::Focusable;
use serde_json::json;

const PAGE: &str = editor_file_templates::PAGE;

/// Types `text` over the text of `area`.
fn type_area(area: Option<Entity<TextArea>>, text: &str, cx: &mut VisualTestContext) {
    let area = area.expect("the text is shown");
    cx.update(|window, cx| {
        area.update(cx, |area, cx| area.select_all(cx));
        window.focus(&area.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.simulate_input(text);
    cx.run_until_parked();
}

// An include typed in the Default scheme, the Project scheme chosen (seeded from Default) and the
// include typed again are stored at once under the project's keys; nothing is staged, Cancel keeps
// them, and Settings opened again is on the Project scheme with the project's include.
#[gpui_kit::test]
async fn the_project_scheme_and_an_include_are_stored_at_once_and_cancel_keeps_them(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, root, workbench, cx) = template_project(cx, &[("src/Main.kt", "fun main() {}\n")]);
    let root = ipc_path(&root.to_string_lossy());
    let (dialog, page) = open_template_page::<FileTemplatesPageView>(&workbench, PAGE, cx);
    assert_eq!(page.read_with(cx, |page, cx| page.lines(cx))[0], "Scheme: Default ▾");

    click_on(cx, "settings-filetemplates-tab-includes");
    type_area(page.read_with(cx, |page, _| page.include_body()), "// default header", cx);
    let default = stored_template_json(FILE_TEMPLATES_KEY, cx);
    assert_eq!(default["includes"]["File Header"]["body"], json!("// default header"));
    click_on(cx, "settings-filetemplates-scheme");
    click_on(cx, "settings-filetemplates-scheme-project");
    assert_eq!(stored_template_json(&scheme_key(&root), cx), json!({ "scheme": "project" }));
    assert_eq!(stored_template_json(&project_key(&root), cx), default, "the Project scheme starts as Default");
    type_area(page.read_with(cx, |page, _| page.include_body()), "// project header", cx);
    let project = stored_template_json(&project_key(&root), cx);
    assert_eq!(project["includes"]["File Header"]["body"], json!("// project header"));
    assert_eq!(stored_template_json(FILE_TEMPLATES_KEY, cx), default, "Default is left as it was");
    nothing_staged(&dialog, cx);

    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    let (_, again) = open_template_page::<FileTemplatesPageView>(&workbench, PAGE, cx);
    assert_eq!(again.read_with(cx, |page, cx| page.lines(cx))[0], "Scheme: Project ▾ Templates of this project only");
    click_on(cx, "settings-filetemplates-tab-includes");
    let body = again.read_with(cx, |page, _| page.include_body()).expect("the include's text");
    assert_eq!(body.read_with(cx, |area, _| area.text().to_owned()), "// project header");
}

// A template added on the Files tab, its name and extension typed, its file name style chosen, a kind
// added with its text and Reformat according to style cleared, is stored at once; OK closes the
// dialog at once, as nothing is staged.
#[gpui_kit::test]
async fn a_file_template_added_and_edited_on_the_page_is_stored_at_once(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (_folder, _root, workbench, cx) = template_project(cx, &[("README.md", "# p\n")]);
    let (dialog, page) = open_template_page::<FileTemplatesPageView>(&workbench, PAGE, cx);
    click_on(cx, "settings-filetemplates-add");
    let id = page.read_with(cx, |page, _| page.selected_file_template().map(str::to_owned)).expect("selected");
    let field = |which: FileField, cx: &mut VisualTestContext| {
        page.read_with(cx, |page, _| page.file_field(which)).expect("the field is shown")
    };
    type_into(&field(FileField::Name, cx), "Changelog Entry", cx);
    type_into(&field(FileField::Extension, cx), ".md", cx);
    click_on(cx, "settings-filetemplates-style");
    click_on(cx, "settings-filetemplates-style-snake");
    click_on(cx, "settings-filetemplates-add-kind");
    type_into(&field(FileField::KindLabel, cx), "Release", cx);
    type_area(page.read_with(cx, |page, _| page.file_body()), "## $NAME$\n$END$\n", cx);
    click_on(cx, "settings-filetemplates-reformat");

    let stored = stored_template_json(FILE_TEMPLATES_KEY, cx)["templates"][id.as_str()].clone();
    assert_eq!(stored["name"], json!("Changelog Entry"));
    assert_eq!(stored["extension"], json!("md"));
    assert_eq!(stored["nameStyle"], json!("snake"));
    assert_eq!(stored["reformat"], json!(false));
    assert_eq!(stored["kinds"][1]["label"], json!("Release"));
    assert_eq!(stored["kinds"][1]["body"], json!("## $NAME$\n$END$\n"));
    nothing_staged(&dialog, cx);
    click_on(cx, "settings-ok");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    let kept = stored_template_json(FILE_TEMPLATES_KEY, cx)["templates"][id.as_str()]["name"].clone();
    assert_eq!(kept, json!("Changelog Entry"));
}
