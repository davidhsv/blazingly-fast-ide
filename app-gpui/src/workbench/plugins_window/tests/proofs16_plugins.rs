//! The Plugins page and the permission page carried out on a running plugin host.

use super::*;
use crate::plugin_models::plugins_page::GearCommand;

/// The app's plugin host, whichever page drives it.
fn app_host(cx: &mut VisualTestContext) -> Arc<PluginHost> {
    cx.update(|_, cx| cx.global::<Plugins>().host.clone())
}

fn registered(id: &str, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| cx.global::<ActionRegistry>().contains(id))
}

/// Clicks the gear menu's item that runs `command`.
fn run_gear_item(page: &Entity<PluginsPage>, command: GearCommand, cx: &mut VisualTestContext) {
    click_on(cx, "plugins-gear");
    let gear = page.update(cx, |page, cx| page.gear_items(cx));
    let index = gear.iter().position(|item| item.command == Some(command)).expect("the item is in the gear menu");
    click_on(cx, Box::leak(format!("plugins-gear-item-{index}").into_boxed_str()));
}

/// Apply on a plugin theme of the Plugins page makes the window draw with it; Use Default Theme
/// then takes it away again.
#[gpui_kit::test]
async fn selects_and_clears_a_plugin_theme(cx: &mut TestAppContext) {
    const PLUGIN: &str = "openidea.harbor-night";
    let home = start_with_plugins(cx);
    let folder = fixture(&home, "harbor-night");
    cx.update(|cx| {
        let host = cx.global::<Plugins>().host.clone();
        let info = host.link(&folder).expect("the plugin links");
        host.grant(&info.id, &Consent::default()).expect("consent is recorded");
    });
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    let (_workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the theme plugin is listed", |cx| {
        page.update(cx, |page, cx| page.rows(cx))
            .into_iter()
            .any(|row| matches!(row, ListRow::Item(item) if item.id == PLUGIN))
    });
    click_on(cx, Box::leak(format!("plugins-row-{PLUGIN}").into_boxed_str()));
    wait_long(cx, "its details show", |cx| listed(&page, PLUGIN, cx));
    let active = |cx: &mut VisualTestContext| app_host(cx).active_theme().map(|theme| theme.theme.id);

    assert_eq!(active(cx), None);
    click_on(cx, "plugins-apply-theme-harbor-night");
    wait_long(cx, "the plugin's theme applies", |cx| active(cx).as_deref() == Some("harbor-night"));
    assert_eq!(cx.update(|_, cx| global(cx).appearance.theme), format!("plugin:{PLUGIN}/harbor-night"));

    wait_long(cx, "the page offers the default theme", |cx| {
        cx.debug_bounds("plugins-default-theme-harbor-night").is_some()
    });
    click_on(cx, "plugins-default-theme-harbor-night");
    wait_long(cx, "the plugin's theme is taken away", |cx| active(cx).is_none());
    assert_eq!(cx.update(|_, cx| global(cx).appearance.theme), "dark", "the default of the theme's kind");
}

/// Disable All Downloaded Plugins in the gear menu turns off every installed plugin and leaves a
/// linked development plugin on; Enable All turns them on again.
#[gpui_kit::test]
async fn disables_and_re_enables_all_downloaded_plugins_leaving_dev_plugins_alone(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    host.install_from_dir(&fixture(&home, "string-tools"), &Consent::all()).expect("the plugin installs");
    host.install_from_dir(&fixture(&home, "hello"), &Consent::all()).expect("the plugin installs");
    let linked = host.link(&fixture(&home, "language-toy")).expect("the development plugin links");
    host.grant(&linked.id, &Consent::default()).expect("consent is recorded");
    let (_workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugins are listed", |cx| {
        let listed = page.update(cx, |page, cx| page.rows(cx));
        [STRING_TOOLS, "hello", linked.id.as_str()]
            .iter()
            .all(|id| listed.iter().any(|row| matches!(row, ListRow::Item(item) if item.id == *id)))
    });
    let enabled = |id: &str| installed(&host, id).is_some_and(|plugin| plugin.enabled);

    run_gear_item(&page, GearCommand::DisableAll, cx);
    wait_long(cx, "the downloaded plugins are off", |_| !enabled(STRING_TOOLS) && !enabled("hello"));
    assert!(enabled(&linked.id), "the development plugin stays on");

    wait_long(cx, "the page is done with them", |cx| {
        page.update(cx, |page, cx| page.gear_items(cx))
            .iter()
            .any(|item| item.command == Some(GearCommand::EnableAll) && item.enabled)
    });
    run_gear_item(&page, GearCommand::EnableAll, cx);
    wait_long(cx, "the downloaded plugins are on again", |_| enabled(STRING_TOOLS) && enabled("hello"));
    assert!(enabled(&linked.id));
}

/// The checkbox of a plugin's row turns it off on the app's host and its commands stop being
/// actions; checked again, they are actions again.
#[gpui_kit::test]
async fn turning_a_plugin_off_takes_its_commands_out_of_the_actions_and_on_brings_them_back(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let id = link(cx, &fixture(&home, "hello"), &[], "insert-date");
    let action = plugin_action_id(&id, "insert-date");
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    let (_workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugin is listed", |cx| listed(&page, &id, cx));
    assert!(registered(&action, cx));

    click_on(cx, Box::leak(format!("plugins-check-{id}").into_boxed_str()));
    wait_long(cx, "its command is no action", |cx| !registered(&action, cx));
    assert!(installed(&host, &id).is_some_and(|plugin| !plugin.enabled));

    wait_long(cx, "the page is done with it", |cx| {
        page.update(cx, |page, cx| page.details(cx)).is_some_and(|details| !details.busy)
    });
    click_on(cx, Box::leak(format!("plugins-check-{id}").into_boxed_str()));
    wait_long(cx, "its command is an action again", |cx| registered(&action, cx));
}

/// A plugin's commands are actions named `plugin.<plugin>.<command>` with the declared title and,
/// without a category of their own, the plugin's name as their category; Reload Plugins after the
/// manifest changed gives the action the new title.
#[gpui_kit::test]
async fn projects_command_contributions_to_plugin_actions_and_follows_a_changed_declaration(cx: &mut TestAppContext) {
    let home = start_with_plugins(cx);
    let folder = plugin_copy(home.path(), "hello", |text| {
        text.replace("name = \"Hello\"", "name = \"Greeter\"").replace("category = \"Hello\"\n", "")
    });
    let id = link(cx, &folder, &[], "insert-date");
    let action = plugin_action_id(&id, "insert-date");
    assert_eq!(action, "plugin.hello.insert-date");
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    let (_workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugin is listed", |cx| listed(&page, &id, cx));
    let meta = |cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            let action = cx.global::<ActionRegistry>().get(&action).expect("registered");
            (action.meta.title.clone(), action.meta.category.clone())
        })
    };
    assert_eq!(meta(cx), ("Hello: Insert Date".to_owned(), Some("Greeter".to_owned())));

    let manifest = folder.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest is readable");
    let renamed = text.replace("title = \"Hello: Insert Date\"", "title = \"Greeter: Insert Today\"");
    std::fs::write(&manifest, renamed).expect("the manifest is written");
    run_action(cx, "plugins.reloadAll", None);
    wait_long(cx, "the action has the new title", |cx| meta(cx).0 == "Greeter: Insert Today");
    assert_eq!(meta(cx).1.as_deref(), Some("Greeter"));
}

/// The Hello fixture's manifest as a plugin made for plugin API 0.1 declares it: installed by a
/// version without the grant ledger, it is given its low-risk permissions only.
fn hello_for_api_0_1(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace("engines.openidea = \"^1.0\"", "openidea-api = \"0.1\"")
        .replace("activation = [\"on-command:*\", \"on-view:greetings\"]\n", "")
        .replace(
            "[capabilities]\nrequired = [\"editor.read\", \"editor.write\"]",
            "permissions = [\"project-read\", \"editor-read\", \"editor-write\"]",
        )
}

/// A running plugin that asks for a permission it was never given shows the permission page in
/// its window, naming that permission: Cancel grants nothing; OK, the next time it asks, grants
/// it on the app's host.
#[gpui_kit::test]
async fn a_running_plugins_permission_request_shows_the_permission_page_and_ok_grants_it(cx: &mut TestAppContext) {
    use super::super::test_support::{PluginHome, copy_dir, repo, settle_app};
    let hosts = crate::plugins::lock_test_hosts();
    let home = start(cx);
    // Installed before the host starts, as an older version of the app left it.
    let installed_at = home.path().join("data/plugins/hello");
    copy_dir(&repo().join("crates/ide-plugin/tests/fixtures/hello"), &installed_at);
    let manifest = installed_at.join("plugin.toml");
    let text = std::fs::read_to_string(&manifest).expect("the manifest is readable");
    std::fs::write(&manifest, hello_for_api_0_1(&text)).expect("the manifest is written");
    cx.update(|cx| {
        crate::theme::appearance::init(cx);
        Plugins::init(cx).expect("the plugin host starts");
    });
    let _home = PluginHome::new(home, hosts);
    settle_app(cx, "the installed plugin is loaded", |cx| cx.global::<Plugins>().host.info_of("hello").is_ok());
    let write_granted = |cx: &mut VisualTestContext| {
        app_host(cx).info_of("hello").ok().and_then(|info| {
            info.capabilities.iter().find(|capability| capability.id == "editor.write").map(|c| c.granted)
        })
    };
    let (_folder, _root, workbench, cx) = trusted_project(cx, "w1", &[("a.txt", "a\n")]);
    assert_eq!(write_granted(cx), Some(false), "migrated with its low-risk permissions only");
    let consent = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugins_consent_dialog().cloned());
    let action = plugin_action_id("hello", "insert-date");
    wait_long(cx, "the plugin's command is an action", |cx| registered(&action, cx));

    run_action(cx, &action, None);
    wait_long(cx, "the permission page shows", |cx| consent(cx).is_some());
    let dialog = consent(cx).expect("the permission page");
    let (asked, install) = dialog.read_with(cx, |d, _| (d.request().plugin_id.clone(), d.request().install));
    assert_eq!((asked.as_str(), install), ("hello", false));
    let shown: Vec<String> = dialog.read_with(cx, |d, _| d.capabilities().iter().map(|c| c.id.clone()).collect());
    assert!(shown.iter().any(|id| id == "editor.write"), "{shown:?}");
    click_on(cx, "consent-button-cancel");
    assert!(consent(cx).is_none(), "Cancel closes the page");
    cx.run_until_parked();
    assert_eq!(write_granted(cx), Some(false), "Cancel grants nothing");

    run_action(cx, &action, None);
    wait_long(cx, "the permission page shows again", |cx| consent(cx).is_some());
    click_on(cx, "consent-button-ok");
    wait_long(cx, "the permission is granted", |cx| write_granted(cx) == Some(true));
    assert!(consent(cx).is_none(), "OK closes the page");
}
