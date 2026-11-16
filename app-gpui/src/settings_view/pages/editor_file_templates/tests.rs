use super::super::test_support::{Seen, click, dialog_model, open, type_over};
use super::*;
use crate::settings_model::staging::Scope;
use crate::templates::files_builtins::builtin_code_templates;
use crate::templates::files_model::{FileTemplate, plan_file};
use crate::templates::files_store::{FILE_TEMPLATES_KEY, project_key};
use gpui_kit::{Focusable, TestAppContext, VisualTestContext};
use serde_json::{Value, json};

fn page<'a>(
    cx: &'a mut TestAppContext,
    root: Option<&str>,
) -> (Entity<FileTemplatesPageView>, Seen, &'a mut VisualTestContext) {
    cx.update(|cx| cx.set_global(UiStorage::default()));
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let root = root.map(str::to_owned);
    open(cx, move |_, cx| FileTemplatesPageView::new(Binding::fixed(model, PAGE, Scope::Global), root, cx))
}

fn stored_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

fn lines(view: &Entity<FileTemplatesPageView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, cx| view.lines(cx))
}

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

fn selected_file(view: &Entity<FileTemplatesPageView>, cx: &mut VisualTestContext) -> FileTemplate {
    view.read_with(cx, |view, _| {
        let id = view.selected_file_template()?;
        view.store().templates().iter().find(|t| t.id == id).cloned()
    })
    .expect("a template is selected")
}

fn file_field(view: &Entity<FileTemplatesPageView>, field: FileField, cx: &mut VisualTestContext) -> Entity<TextField> {
    view.read_with(cx, |view, _| view.file_field(field)).unwrap_or_else(|| panic!("the {field:?} field is shown"))
}

fn include_body(view: &Entity<FileTemplatesPageView>, name: &str, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.store().includes().iter().find(|f| f.name == name).map(|f| f.body.clone()))
}

// src/features/templates/__tests__/views.test.tsx › settings pages › renders file templates
#[gpui_kit::test]
async fn renders_file_templates(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, None);
    let lines = lines(&view, cx);
    assert_eq!(lines[0], "Scheme: Default ▾ (disabled)");
    assert_eq!(lines[1], "[Files] | Includes | Code");
    assert!(lines.iter().any(|line| line.ends_with("[x] Kotlin Class/File")), "{lines:?}");
}

// src/features/templates/__tests__/views.test.tsx › settings pages › edits includes and code templates
// in their tabs
#[gpui_kit::test]
async fn edits_includes_and_code_templates_in_their_tabs(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, None);
    click(cx, "settings-filetemplates-tab-includes");
    type_area(view.read_with(cx, |view, _| view.include_body()), "// header", cx);
    assert_eq!(include_body(&view, "File Header", cx).as_deref(), Some("// header"));
    assert_eq!(stored_json(FILE_TEMPLATES_KEY, cx)["includes"]["File Header"]["body"], json!("// header"));
    click(cx, "settings-includes-restore");
    assert_eq!(include_body(&view, "File Header", cx).as_deref(), Some(""));
    click(cx, "settings-filetemplates-tab-code");
    assert!(lines(&view, cx).contains(&"> JUnit5 Test Method".to_owned()));
    type_area(view.read_with(cx, |view, _| view.code_body()), "@Test\nvoid t() {}\n", cx);
    assert_eq!(view.read_with(cx, |view, _| view.store().code_templates()[0].body.clone()), "@Test\nvoid t() {}\n");
    assert!(lines(&view, cx).contains(&"Used by Java Class › JUnit 5 Test".to_owned()));
    click(cx, "settings-code-restore");
    assert_eq!(view.read_with(cx, |view, _| view.store().code_templates().to_vec()), builtin_code_templates());
}

// src/features/templates/__tests__/views.test.tsx › settings pages › keeps a project scheme seeded from
// Default
#[gpui_kit::test]
async fn keeps_a_project_scheme_seeded_from_default(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, Some("/proj"));
    click(cx, "settings-filetemplates-tab-includes");
    type_area(view.read_with(cx, |view, _| view.include_body()), "// default", cx);
    click(cx, "settings-filetemplates-scheme");
    click(cx, "settings-filetemplates-scheme-project");
    assert_eq!(lines(&view, cx)[0], "Scheme: Project ▾ Templates of this project only");
    assert_eq!(include_body(&view, "File Header", cx).as_deref(), Some("// default"));
    assert_eq!(stored_json(&project_key("/proj"), cx)["includes"]["File Header"]["body"], json!("// default"));
    type_area(view.read_with(cx, |view, _| view.include_body()), "// project", cx);
    click(cx, "settings-filetemplates-scheme");
    click(cx, "settings-filetemplates-scheme-default");
    assert_eq!(include_body(&view, "File Header", cx).as_deref(), Some("// default"));
    click(cx, "settings-filetemplates-scheme");
    click(cx, "settings-filetemplates-scheme-project");
    assert_eq!(include_body(&view, "File Header", cx).as_deref(), Some("// project"));
    assert_eq!(view.read_with(cx, |view, _| view.tab()), TemplateTab::Includes);
}

// Not from a spec: Add Template makes a selected "New Template"; its name, extension (stored without
// the leading dot), file name style, a kind added with its label and text, Reformat according to
// style and its checkbox are stored with it; Remove Kind keeps one kind, and Remove Template takes
// the template away.
#[gpui_kit::test]
async fn each_control_of_the_files_tab_is_stored_with_the_template(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, None);
    click(cx, "settings-filetemplates-add");
    let added = selected_file(&view, cx);
    assert!(added.id.starts_with("user."), "{}", added.id);
    assert_eq!((added.name.as_str(), added.extension.as_str(), added.kinds.len()), ("New Template", "txt", 1));

    type_over(&file_field(&view, FileField::Name, cx), "Notes", cx);
    let extension = file_field(&view, FileField::Extension, cx);
    type_over(&extension, ".md ", cx);
    assert_eq!(extension.read_with(cx, |field, _| field.text().to_owned()), "md");
    click(cx, "settings-filetemplates-style");
    click(cx, "settings-filetemplates-style-kebab");
    click(cx, "settings-filetemplates-add-kind");
    type_over(&file_field(&view, FileField::KindLabel, cx), "Daily", cx);
    type_area(view.read_with(cx, |view, _| view.file_body()), "# $NAME$\n$END$\n", cx);
    click(cx, "settings-filetemplates-reformat");
    let template = selected_file(&view, cx);
    assert_eq!((template.name.as_str(), template.extension.as_str()), ("Notes", "md"));
    assert_eq!(template.name_style, NameStyle::Kebab);
    assert_eq!(template.kinds.len(), 2);
    assert_eq!((template.kinds[1].label.as_str(), template.kinds[1].body.as_str()), ("Daily", "# $NAME$\n$END$\n"));
    assert_eq!(template.reformat, Some(false));
    let stored = stored_json(FILE_TEMPLATES_KEY, cx)["templates"][template.id.as_str()].clone();
    assert_eq!((stored["nameStyle"].clone(), stored["reformat"].clone()), (json!("kebab"), json!(false)));
    assert!(lines(&view, cx).contains(&"Kinds: Default | [Daily]".to_owned()));

    click(cx, "settings-filetemplates-remove-kind");
    assert_eq!(selected_file(&view, cx).kinds.len(), 1);
    click(cx, "settings-filetemplates-remove-kind");
    assert_eq!(selected_file(&view, cx).kinds.len(), 1, "a template keeps one kind");
    let enabled: &'static str = Box::leak(format!("settings-filetemplates-enabled-{}", template.id).into_boxed_str());
    click(cx, enabled);
    assert_eq!(selected_file(&view, cx).enabled, Some(false));
    click(cx, "settings-filetemplates-remove");
    assert_eq!(view.read_with(cx, |view, _| view.selected_file_template().map(str::to_owned)), None);
    assert_eq!(stored_json(FILE_TEMPLATES_KEY, cx)["templates"], json!({}));
}

// Not from a spec: a template named `../escape` on the page is a label only: New refuses the name
// typed as `../escape` and creates a name typed plainly in the folder it is used in.
#[gpui_kit::test]
async fn a_template_named_with_dot_dot_slash_creates_nothing_outside_the_folder(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, None);
    click(cx, "settings-filetemplates-add");
    type_over(&file_field(&view, FileField::Name, cx), "../escape", cx);
    let template = selected_file(&view, cx);
    assert_eq!(template.name, "../escape");
    assert_eq!(plan_file(&template, "/p/src", "../escape"), None);
    assert_eq!(plan_file(&template, "/p/src", "a/../../b"), None);
    let planned = plan_file(&template, "/p/src", "notes").expect("a plain name is planned");
    assert_eq!(planned.path, "/p/src/notes.txt");
}

// Not from a spec: Add Include makes "New Include"; a name typed and committed with Enter renames it
// (and is the dialog's OK), a taken name is put back when the field is left, a built-in include's
// name cannot be changed, and Remove Include takes it away.
#[gpui_kit::test]
async fn includes_are_added_renamed_and_removed(cx: &mut TestAppContext) {
    let (view, seen, cx) = page(cx, None);
    click(cx, "settings-filetemplates-tab-includes");
    let builtin = view.read_with(cx, |view, _| view.include_name_field()).expect("the name field");
    assert!(builtin.read_with(cx, |field, _| field.is_read_only()), "File Header keeps its name");
    click(cx, "settings-includes-add");
    let selected = view.read_with(cx, |view, _| view.selected_include_name().map(str::to_owned));
    assert_eq!(selected.as_deref(), Some("New Include"));
    let name = view.read_with(cx, |view, _| view.include_name_field()).expect("the name field");
    type_over(&name, "Copyright", cx);
    assert!(include_body(&view, "Copyright", cx).is_none(), "the name is committed when left or on Enter");
    cx.simulate_keystrokes("enter");
    assert_eq!(include_body(&view, "Copyright", cx).as_deref(), Some(""));
    assert!(include_body(&view, "New Include", cx).is_none());
    assert_eq!(seen.borrow().last().map(String::as_str), Some("submit"));
    let name = view.read_with(cx, |view, _| view.include_name_field()).expect("the name field");
    type_over(&name, "File Header", cx);
    let body = view.read_with(cx, |view, _| view.include_body()).expect("the body");
    cx.update(|window, cx| window.focus(&body.focus_handle(cx), cx));
    cx.run_until_parked();
    assert_eq!(name.read_with(cx, |field, _| field.text().to_owned()), "Copyright", "a taken name is put back");
    click(cx, "settings-includes-remove");
    assert!(include_body(&view, "Copyright", cx).is_none());
    assert_eq!(stored_json(FILE_TEMPLATES_KEY, cx)["includes"], json!({}));
}

// Not from a spec: what another window stores is shown at once.
#[gpui_kit::test]
async fn templates_stored_elsewhere_are_shown_at_once(cx: &mut TestAppContext) {
    let (view, _, cx) = page(cx, None);
    click(cx, "settings-filetemplates-tab-includes");
    let stored = json!({ "includes": { "File Header": { "name": "File Header", "body": "// elsewhere" } } });
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(FILE_TEMPLATES_KEY, stored.to_string()));
    cx.run_until_parked();
    let body = view.read_with(cx, |view, _| view.include_body()).expect("the body");
    assert_eq!(body.read_with(cx, |area, _| area.text().to_owned()), "// elsewhere");
}
