use super::*;
use crate::plugin_views::settings_page::{FieldKey, PluginSettingsPage, declared_page_id};
use crate::settings_view::dialog::SettingsDialogView;
use crate::ui::text_field::TextField;
use gpui_kit::Focusable as _;

const PLUGIN: &str = "settings-pages";

fn leaked(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn open_settings(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<SettingsDialogView> {
    cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-," } else { "ctrl-," });
    cx.run_until_parked();
    workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("⌘, opens Settings")
}

/// Puts the keyboard in `field`, selects what it holds and types `text` over it.
fn type_over(field: &Entity<TextField>, text: &str, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    field.update(cx, |field, cx| field.select_all(cx));
    cx.simulate_input(text);
    cx.run_until_parked();
}

/// Finds `page` by typing `title` into the settings search, clicks it in the tree and clears the search.
fn find_page(dialog: &Entity<SettingsDialogView>, title: &str, page: &str, cx: &mut VisualTestContext) {
    let search = dialog.read_with(cx, |view, _| view.search_field().clone());
    type_over(&search, title, cx);
    click_on(cx, leaked(format!("settings-page-{page}")));
    let search = dialog.read_with(cx, |view, _| view.search_field().clone());
    cx.update(|window, cx| window.focus(&search.focus_handle(cx), cx));
    search.update(cx, |field, cx| field.select_all(cx));
    cx.simulate_keystrokes("backspace");
    cx.run_until_parked();
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(page));
}

/// The plugin's page `page` as the dialog draws it.
fn page_of(dialog: &Entity<SettingsDialogView>, page: &str, cx: &mut VisualTestContext) -> Entity<PluginSettingsPage> {
    dialog
        .read_with(cx, |view, _| view.page_view(page).cloned())
        .and_then(|view| view.downcast::<PluginSettingsPage>().ok())
        .unwrap_or_else(|| panic!("{page} has its view"))
}

fn field_of(page: &Entity<PluginSettingsPage>, key: &FieldKey, cx: &mut VisualTestContext) -> Entity<TextField> {
    page.read_with(cx, |page, _| page.field(key).cloned()).unwrap_or_else(|| panic!("{key:?} has a field"))
}

fn text_of_field(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
    field.read_with(cx, |field, _| field.text().to_owned())
}

fn stored(pointer: &str, cx: &mut VisualTestContext) -> Option<Value> {
    use crate::settings_model::schema::settings_value;
    let settings = cx.update(|_, cx| settings_value(&crate::platform::settings::global(cx)));
    settings.pointer(pointer).cloned()
}

/// A plugin's declared settings pages, found by the search and in the tree under Plugins › Model
/// Profiles, stage their edits: Test on LLM Settings applies the added profile named Cloud before its
/// command reads it, leaving nothing to apply. An edit after that is gone with Cancel: opened again,
/// the page shows Cloud. An edit on Commit Template's Allowed commit types survives Reset on LLM
/// Settings, which takes back only that page's edit, and Apply writes it.
#[gpui_kit::test]
async fn plugin_settings_pages_stage_edits_apply_before_actions_and_discard_on_cancel(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), PLUGIN, str::to_owned);
    link(cx, &folder, &[], "test");
    let (_folder, _root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());
    let (llm, templates) = (declared_page_id(PLUGIN, "llm"), declared_page_id(PLUGIN, "templates"));
    let plugin_page = crate::plugin_views::settings_page::settings_page_id(PLUGIN);
    let name = FieldKey::cell("profiles", 0, Some("name"));
    let first_type = FieldKey::cell("types", 0, None);

    let dialog = open_settings(&workbench, cx);
    find_page(&dialog, "LLM Settings", &llm, cx);
    // The same page through the tree: Plugins, Right, Model Profiles, Right, LLM Settings.
    click_on(cx, "settings-page-plugins");
    cx.simulate_keystrokes("right");
    click_on(cx, leaked(format!("settings-page-{plugin_page}")));
    cx.simulate_keystrokes("right");
    click_on(cx, leaked(format!("settings-page-{llm}")));
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(llm.as_str()));

    let page = page_of(&dialog, &llm, cx);
    click_on(cx, "plugin-setting-add:profiles");
    type_over(&field_of(&page, &name, cx), "Cloud", cx);
    assert!(dialog.read_with(cx, |view, cx| view.can_apply(cx)), "the row is staged");
    click_on(cx, "plugin-settings-action:0");
    wait_long(cx, "the command read the applied row", |cx| {
        notice_titled(&workbench, "Model Profiles", cx)
            .is_some_and(|notice| notice.body.as_deref() == Some("Read 1 saved profile(s)"))
    });
    assert!(!dialog.read_with(cx, |view, cx| view.can_apply(cx)), "Apply is disabled: the row was applied");
    assert_eq!(stored("/plugins/settings-pages/profiles/0/name", cx), Some(Value::from("Cloud")));

    type_over(&field_of(&page, &name, cx), "Discard me", cx);
    click_on(cx, "settings-cancel");
    assert!(workbench.read_with(cx, |w, _| w.settings_dialog().is_none()), "Cancel closes Settings");
    assert_eq!(stored("/plugins/settings-pages/profiles/0/name", cx), Some(Value::from("Cloud")));
    let dialog = open_settings(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some(llm.as_str()));
    let page = page_of(&dialog, &llm, cx);
    assert_eq!(text_of_field(&field_of(&page, &name, cx), cx), "Cloud");

    find_page(&dialog, "Commit Template", &templates, cx);
    let template_page = page_of(&dialog, &templates, cx);
    let lines = template_page.read_with(cx, |page, cx| page.lines(cx));
    assert!(lines.iter().any(|line| line.starts_with("Allowed commit types")), "{lines:?}");
    type_over(&field_of(&template_page, &first_type, cx), "docs", cx);

    find_page(&dialog, "LLM Settings", &llm, cx);
    let page = page_of(&dialog, &llm, cx);
    type_over(&field_of(&page, &name, cx), "Reset only this page", cx);
    click_on(cx, "settings-reset");
    assert_eq!(text_of_field(&field_of(&page, &name, cx), cx), "Cloud");

    find_page(&dialog, "Commit Template", &templates, cx);
    let template_page = page_of(&dialog, &templates, cx);
    assert_eq!(text_of_field(&field_of(&template_page, &first_type, cx), cx), "docs", "the other page keeps its edit");
    click_on(cx, "settings-apply");
    assert!(!dialog.read_with(cx, |view, cx| view.can_apply(cx)), "nothing is left to apply");
    assert_eq!(stored("/plugins/settings-pages/types/0", cx), Some(Value::from("docs")));
}
