use super::super::test_support::{click, dialog_model, open, type_over};
use super::*;
use crate::settings_model::staging::Scope;
use crate::templates::files_model::{FileTemplate, FileTemplateKind, plan_file, with_kind};
use crate::templates::files_store::FILE_TEMPLATES_KEY;
use gpui_kit::{TestAppContext, VisualTestContext};

// An extension typed with slashes on the page is not stored: the field says why and the template
// keeps its extension, so New creates nothing outside the folder it is used in; neither does a
// template or a kind carrying such an extension from elsewhere.
#[gpui_kit::test]
async fn an_extension_typed_with_slashes_creates_nothing_outside_the_folder(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(UiStorage::default()));
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let (view, _, cx) =
        open(cx, move |_, cx| FileTemplatesPageView::new(Binding::fixed(model, PAGE, Scope::Global), None, cx));
    click(cx, "settings-filetemplates-add");
    let stored_before = cx.update(|_, cx| cx.global::<UiStorage>().get(FILE_TEMPLATES_KEY).map(str::to_owned));
    let extension = view.read_with(cx, |view, _| view.file_field(FileField::Extension)).expect("the field");
    type_over(&extension, "/../../x", cx);
    assert!(view.read_with(cx, |view, _| view.extension_is_invalid()));
    assert!(
        view.read_with(cx, |view, cx| view.lines(cx)).contains(&"Not a file extension: it is not saved".to_owned())
    );
    let stored_after = cx.update(|_, cx| cx.global::<UiStorage>().get(FILE_TEMPLATES_KEY).map(str::to_owned));
    assert_eq!(stored_after, stored_before, "nothing is stored");
    let template = view
        .read_with(cx, |view, _| {
            let id = view.selected_file_template()?;
            view.store().templates().iter().find(|t| t.id == id).cloned()
        })
        .expect("the template");
    assert_eq!(template.extension, "txt");
    assert_eq!(plan_file(&template, "/p/src", "notes").map(|plan| plan.path).as_deref(), Some("/p/src/notes.txt"));

    type_over(&extension, ".md", cx);
    assert!(!view.read_with(cx, |view, _| view.extension_is_invalid()));
    assert_eq!(extension.read_with(cx, |field, _| field.text().to_owned()), "md");

    let escaping = FileTemplate { extension: "/../../x".to_owned(), ..template.clone() };
    assert_eq!(plan_file(&escaping, "/p/src", "notes"), None);
    let kind = FileTemplateKind { extension: Some("../../y".to_owned()), ..template.kinds[0].clone() };
    assert_eq!(plan_file(&with_kind(&template, Some(&kind)), "/p/src", "notes"), None);
}

// A template stored elsewhere with an extension that is not plain, or a kind with one, shows why
// where it is edited and takes no change; removing it is saved, and so is removing the kind of a
// template whose own extension is plain.
#[gpui_kit::test]
async fn a_template_stored_with_an_unplain_extension_says_why_its_changes_are_not_saved(cx: &mut TestAppContext) {
    let kinds = serde_json::json!([
        { "id": "a", "label": "A", "body": "$END$" },
        { "id": "b", "label": "B", "body": "$END$", "extension": "../../y" }
    ]);
    let stored = serde_json::json!({ "templates": {
        "user.bad": { "id": "user.bad", "name": "Bad", "icon": "file", "extension": "../x", "kinds": kinds },
        "user.kind": { "id": "user.kind", "name": "Kind", "icon": "file", "extension": "txt", "kinds": kinds }
    } });
    let mut storage = UiStorage::default();
    storage.set(FILE_TEMPLATES_KEY, stored.to_string());
    cx.update(|cx| cx.set_global(storage));
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let (view, _, cx) =
        open(cx, move |_, cx| FileTemplatesPageView::new(Binding::fixed(model, PAGE, Scope::Global), None, cx));
    let stored = |cx: &mut VisualTestContext| {
        let text = cx.update(|_, cx| cx.global::<UiStorage>().get(FILE_TEMPLATES_KEY).map(str::to_owned));
        serde_json::from_str::<serde_json::Value>(&text.expect("stored")).expect("JSON")
    };
    let kind_refused = "A kind's extension is not a file extension: changes to this template are not saved";

    view.update(cx, |view, cx| view.select_template("user.bad", cx));
    cx.run_until_parked();
    let lines = view.read_with(cx, |view, cx| view.lines(cx));
    assert!(lines.contains(&"Not a file extension: it is not saved".to_owned()), "{lines:?}");
    assert!(lines.contains(&kind_refused.to_owned()), "{lines:?}");
    type_over(&extension_field(&view, cx), "md", cx);
    assert_eq!(stored(cx)["templates"]["user.bad"]["extension"], "../x", "no change is saved");
    click(cx, "settings-filetemplates-remove");
    assert!(stored(cx)["templates"].get("user.bad").is_none(), "removing it is saved");

    view.update(cx, |view, cx| view.select_template("user.kind", cx));
    cx.run_until_parked();
    assert!(view.read_with(cx, |view, cx| view.lines(cx)).contains(&kind_refused.to_owned()));
    view.update(cx, |view, cx| view.select_kind(1, cx));
    click(cx, "settings-filetemplates-remove-kind");
    assert_eq!(stored(cx)["templates"]["user.kind"]["kinds"].as_array().map(Vec::len), Some(1));
    assert!(!view.read_with(cx, |view, cx| view.lines(cx)).contains(&kind_refused.to_owned()));
}

fn extension_field(view: &Entity<FileTemplatesPageView>, cx: &mut VisualTestContext) -> Entity<TextField> {
    view.read_with(cx, |view, _| view.file_field(FileField::Extension)).expect("the field")
}
