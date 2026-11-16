//! What the end-to-end tests of the template pages share: a project window, Settings opened on a
//! page drawn by its own view, the UI storage read back as JSON, and the dialog's state when nothing
//! is staged.

use super::*;
use crate::platform::storage::UiStorage;
use crate::settings_view::dialog::SettingsDialogView;
use crate::ui::text_field::TextField;
use gpui_kit::{AnyView, Focusable};
use serde_json::Value;
use std::path::PathBuf;

/// A trusted project with `files`, open in a window.
pub(super) fn template_project<'a>(
    cx: &'a mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let root = folder.path().canonicalize().expect("the folder exists");
    for (path, text) in files {
        let file = root.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("the folder is made");
        }
        std::fs::write(file, text).expect("the file is written");
    }
    cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    (folder, root, workbench, cx)
}

/// File › Settings… on `page`: the dialog shows it, drawn by the view of its own `V`.
pub(super) fn open_template_page<V: 'static>(
    workbench: &Entity<Workbench>,
    page: &str,
    cx: &mut VisualTestContext,
) -> (Entity<SettingsDialogView>, Entity<V>) {
    run_action(cx, "file.settings", Some(Value::from(page)));
    let dialog = settings_dialog_of(workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
    assert_eq!(dialog.read_with(cx, |view, cx| view.control_lines(cx)), ["<page view>"]);
    let view: Option<AnyView> = dialog.read_with(cx, |view, _| view.page_view(page).cloned());
    let view = view.and_then(|view| view.downcast().ok()).unwrap_or_else(|| panic!("{page} has its view"));
    (dialog, view)
}

/// The text stored under `key` as JSON; `Null` while nothing is.
pub(super) fn stored_template_json(key: &str, cx: &mut VisualTestContext) -> Value {
    let text = cx.update(|_, cx| cx.global::<UiStorage>().get(key).map(str::to_owned));
    text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null)
}

/// Nothing is staged: Apply cannot be used and Reset is not offered.
pub(super) fn nothing_staged(dialog: &Entity<SettingsDialogView>, cx: &mut VisualTestContext) {
    assert!(!dialog.read_with(cx, |view, cx| view.can_apply(cx)), "Apply is off");
    assert!(!dialog.read_with(cx, |view, cx| view.reset_visible(cx)), "Reset is hidden");
}

/// Types `text` over the text of `field`, with the keyboard in it.
pub(super) fn type_into(field: &Entity<TextField>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        field.update(cx, |field, cx| field.select_all(cx));
        window.focus(&field.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// A selector built at run time, as `debug_bounds` wants it.
pub(super) fn click_template(cx: &mut VisualTestContext, selector: String) {
    let selector: &'static str = Box::leak(selector.into_boxed_str());
    click_on(cx, selector);
}
