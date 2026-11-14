use super::*;
use crate::plugin_models::plugins_page::{ItemKind, ListItem};
use ide_extension::package;

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

fn consent_dialog(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<ConsentDialog>> {
    workbench.read_with(cx, |w, _| w.plugins_consent_dialog().cloned())
}

/// Waits until the consent page shows or `done` holds; accepts the page when it shows.
fn accept_if_asked(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
    done: impl Fn(&mut VisualTestContext) -> bool,
) {
    wait_long(cx, "the consent page, or the operation done", |cx| consent_dialog(workbench, cx).is_some() || done(cx));
    if consent_dialog(workbench, cx).is_some() {
        click_on(cx, "consent-button-ok");
    }
}

/// Marketplace lists the official plugins of the local catalog, read from the folder it is
/// shipped in, beside the installed ones.
#[gpui_kit::test]
async fn the_marketplace_tab_lists_the_plugins_of_the_local_catalog(cx: &mut TestAppContext) {
    let _home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let (_workbench, page, cx) = plugins_window(cx, &host);
    click_on(cx, "plugins-tab-marketplace");
    wait_long(cx, "the catalog is listed", |cx| {
        items(&page, cx).iter().any(|item| item.kind == ItemKind::Catalog && item.name == "String Tools")
    });
    assert!(installed(&host, STRING_TOOLS).is_none(), "listing the catalog installs nothing");
}

/// Install Plugin from Disk… given a package file unpacks it and, accepted, installs the plugin it
/// holds into the host's own folder.
#[gpui_kit::test]
async fn installing_a_package_file_from_disk_unpacks_it_and_installs_its_plugin(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let source = fixture(&home, "hello");
    let files = host.package_files_of(&source).expect("the package's files");
    let archive = home.path().join("hello-0.3.0.oiplugin");
    package::pack(&source, &files, &archive, None).expect("the package is packed");
    let (workbench, page, cx) = plugins_window(cx, &host);

    run_action(cx, "plugins.installFromDisk", Some(json!(path_text(&archive))));
    wait_long(cx, "the install page shows", |cx| {
        consent_dialog(&workbench, cx).is_some() || installed(&host, "hello").is_some()
    });
    if let Some(asked) = consent_dialog(&workbench, cx) {
        let id = asked.read_with(cx, |d, _| d.request().plugin_id.clone());
        assert_eq!(id, "hello", "the install page shows the package's manifest");
        click_on(cx, "consent-button-ok");
    }
    wait_long(cx, "the plugin is installed", |_| installed(&host, "hello").is_some());
    let info = installed(&host, "hello").expect("installed");
    let path = info.path.into_path_buf().canonicalize().expect("the installed copy exists");
    let plugins = dir.path().canonicalize().expect("the plugins folder exists");
    assert!(path.starts_with(plugins), "installed into the host's folder, not run from the archive: {path:?}");
    wait_long(cx, "and listed", |cx| listed(&page, "hello", cx));
}

/// Roll Back on an updated plugin goes back to the version the update replaced: the host has it
/// installed again and the window says so.
#[gpui_kit::test]
async fn roll_back_returns_an_updated_plugin_to_its_previous_version(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let version =
        |to: &'static str| move |text: &str| text.replace("version = \"0.1.0\"", &format!("version = \"{to}\""));
    let older = plugin_copy(home.path(), "string-tools", version("9.0.0"));
    host.install_from_dir(&older, &Consent::all()).expect("the first version installs");
    let newer = plugin_copy(home.path(), "string-tools", version("9.1.0"));
    let updated = host.install_from_dir(&newer, &Consent::all()).expect("the update installs");
    assert_eq!((updated.version.as_str(), updated.previous_version.as_deref()), ("9.1.0", Some("9.0.0")));
    let (workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugin is listed", |cx| listed(&page, STRING_TOOLS, cx));

    click_on(cx, "plugins-details-roll-back");
    wait_long(cx, "the previous version is back", |_| {
        installed(&host, STRING_TOOLS).is_some_and(|info| info.version == "9.0.0")
    });
    wait_long(cx, "the window says so", |cx| {
        notification_titles(&workbench, cx).iter().any(|title| title.starts_with("Rolled back"))
    });
    wait_long(cx, "the page shows it", |cx| {
        page.update(cx, |page, cx| page.details(cx)).is_some_and(|details| details.version.contains("9.0.0"))
    });
}

/// A permission's row on the plugin's page revokes the granted optional permission on the host, and
/// allows it again.
#[gpui_kit::test]
async fn a_permissions_row_revokes_and_allows_the_permission_on_the_host(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    host.install_from_dir(&fixture(&home, "ai-completion"), &Consent::all()).expect("the plugin installs");
    let granted = |host: &PluginHost| {
        installed(host, AI)
            .and_then(|info| info.capabilities.into_iter().find(|c| c.id == "net:localhost"))
            .map(|capability| capability.granted)
    };
    assert_eq!(granted(&host), Some(true));
    let (_workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugin is listed", |cx| listed(&page, AI, cx));

    click_on(cx, "plugins-capability-net:localhost");
    wait_long(cx, "the permission is revoked", |_| granted(&host) == Some(false));
    wait_long(cx, "the page is done with it", |cx| {
        page.update(cx, |page, cx| page.details(cx)).is_some_and(|details| !details.busy)
    });
    click_on(cx, "plugins-capability-net:localhost");
    wait_long(cx, "the permission is allowed again", |_| granted(&host) == Some(true));
}

/// Review Permissions of a plugin waiting for consent asks for its permissions; accepted, the host
/// records the consent and the plugin no longer waits.
#[gpui_kit::test]
async fn review_permissions_grants_a_plugin_waiting_for_consent(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    host.link(&fixture(&home, "hello")).expect("the plugin links");
    let status = |host: &PluginHost| installed(host, "hello").map(|info| info.status);
    assert_eq!(status(&host), Some(PluginStatus::NeedsConsent), "a linked plugin waits for consent");
    let (workbench, page, cx) = plugins_window(cx, &host);
    wait_long(cx, "the plugin is listed", |cx| listed(&page, "hello", cx));

    click_on(cx, "plugins-details-review");
    let consented =
        |_: &mut VisualTestContext| status(&host).is_some_and(|status| status != PluginStatus::NeedsConsent);
    accept_if_asked(&workbench, cx, consented);
    wait_long(cx, "the consent is recorded", consented);
}
