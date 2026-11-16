//! Not declared: the test below fails until `templates::files_model::plan_file` refuses an extension
//! that names another folder. The page stores the Extension field as typed (without a leading dot and
//! the blanks around it), and `plan_file` appends it to the file name unchecked, so an extension
//! `/../../x` plans `<folder>/notes./../../x`, two folders up. Once the guard lands, declare this
//! module in `editor_file_templates.rs` with `#[cfg(test)] mod extension_guard_pending;` (renamed
//! without `_pending`).

use super::super::test_support::{click, dialog_model, open, type_over};
use super::*;
use crate::settings_model::staging::Scope;
use crate::templates::files_model::{FileTemplateKind, plan_file, with_kind};
use gpui_kit::TestAppContext;

// An extension typed with slashes on the page creates no file outside the folder New is used in,
// neither for the template nor for a kind that carries its own extension.
#[gpui_kit::test]
async fn an_extension_typed_with_slashes_creates_nothing_outside_the_folder(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_global(UiStorage::default()));
    let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
    let (view, _, cx) =
        open(cx, move |_, cx| FileTemplatesPageView::new(Binding::fixed(model, PAGE, Scope::Global), None, cx));
    click(cx, "settings-filetemplates-add");
    let extension = view.read_with(cx, |view, _| view.file_field(FileField::Extension)).expect("the field");
    type_over(&extension, "/../../x", cx);
    let template = view
        .read_with(cx, |view, _| {
            let id = view.selected_file_template()?;
            view.store().templates().iter().find(|t| t.id == id).cloned()
        })
        .expect("the template");
    assert_eq!(template.extension, "/../../x");
    assert_eq!(plan_file(&template, "/p/src", "notes"), None);
    let kind = FileTemplateKind { extension: Some("../../y".to_owned()), ..template.kinds[0].clone() };
    assert_eq!(plan_file(&with_kind(&template, Some(&kind)), "/p/src", "notes"), None);
}
