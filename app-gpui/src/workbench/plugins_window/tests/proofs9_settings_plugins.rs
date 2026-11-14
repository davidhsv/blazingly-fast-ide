use super::*;
use crate::plugin_models::plugins_page::ListItem;

/// The plugin items the page lists now (of its tab).
fn items(page: &Entity<PluginsPage>, cx: &mut VisualTestContext) -> Vec<ListItem> {
    page.update(cx, |page, cx| page.rows(cx))
        .into_iter()
        .filter_map(|row| match row {
            ListRow::Item(item) => Some(item),
            _ => None,
        })
        .collect()
}

/// Settings › Plugins in a window with a plugin host is the Plugins page of that host: it lists the installed
/// plugin, enabled, and its checkbox there disables the plugin on the host.
#[gpui_kit::test]
async fn settings_plugins_lists_the_hosts_plugins_and_its_checkbox_disables_one(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", str::to_owned);
    link(cx, &folder, &[], "word-count");
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
    cx.update(|window, _| window.activate_window());

    run_action(cx, "plugins.settings", None);
    let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("plugins"));
    let page = workbench.read_with(cx, |w, _| w.settings_plugins_page().cloned()).expect("the page has the host");
    wait_long(cx, "the installed plugin is listed", |cx| items(&page, cx).iter().any(|item| item.id == "hello"));
    let hello = items(&page, cx).into_iter().find(|item| item.id == "hello").expect("listed");
    assert_eq!(hello.name, "Hello");
    assert_eq!(hello.enabled, Some((true, true)), "enabled, and the checkbox can be clicked");

    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    assert!(installed(&host, "hello").is_some_and(|plugin| plugin.enabled));
    click_on(cx, "plugins-check-hello");
    wait_long(cx, "the host disables the plugin", |_| installed(&host, "hello").is_some_and(|plugin| !plugin.enabled));
    wait_long(cx, "the page follows", |cx| {
        items(&page, cx).iter().any(|item| item.id == "hello" && item.enabled.is_some_and(|(on, _)| !on))
    });
}
