use super::*;
use crate::plugin_models::plugins_page::{GearCommand, ItemKind};
use ide_extension::package::{self, Index, IndexEntry, KeyEntry, Publisher, PublisherKey};

/// Whether the page lists the marketplace plugin `id` now (on its tab).
fn offers(page: &Entity<PluginsPage>, id: &str, cx: &mut VisualTestContext) -> bool {
    page.update(cx, |page, cx| page.rows(cx))
        .into_iter()
        .any(|row| matches!(row, ListRow::Item(item) if item.kind == ItemKind::Market && item.id == id))
}

fn consent_dialog(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<Entity<ConsentDialog>> {
    workbench.read_with(cx, |w, _| w.plugins_consent_dialog().cloned())
}

/// A marketplace in `market`: fixture plugin `hello` as `acme.hello` 1.0.0, packed and signed by the
/// publisher `acme`, and the index that lists it. Returns the index's path.
fn signed_market(home: &tempfile::TempDir, host: &PluginHost, market: &Path) -> String {
    std::fs::create_dir_all(market).expect("the market folder is made");
    let source = plugin_copy(home.path(), "hello", |text| {
        text.replace("id = \"hello\"", "id = \"acme.hello\"").replace("version = \"0.3.0\"", "version = \"1.0.0\"")
    });
    let key = PublisherKey::from_seed([7; 32]);
    let files = host.package_files_of(&source).expect("the package's files");
    let digest = package::package_digest(&source, &files).expect("the package's digest");
    let signature = key.sign("acme", "acme.hello", "1.0.0", &digest);
    let archive = market.join("acme.hello-1.0.0.oiplugin");
    package::pack(&source, &files, &archive, Some(&signature)).expect("the package is packed");
    let entry = IndexEntry {
        id: "acme.hello".to_owned(),
        name: "Hello".to_owned(),
        version: "1.0.0".to_owned(),
        description: String::new(),
        authors: Vec::new(),
        publisher: "acme".to_owned(),
        download: "acme.hello-1.0.0.oiplugin".to_owned(),
        digest,
        capabilities: Vec::new(),
        engine: "^1.0".to_owned(),
        categories: vec!["tools".to_owned()],
        icon: None,
    };
    let publisher =
        Publisher { id: "acme".to_owned(), name: "ACME".to_owned(), keys: vec![KeyEntry::new(key.public_base64())] };
    let index =
        Index { format: package::INDEX_FORMAT, publishers: vec![publisher], plugins: vec![entry], ..Index::default() };
    let path = market.join("index.json");
    std::fs::write(&path, serde_json::to_string(&index).expect("the index serialises")).expect("the index is written");
    path_text(&path)
}

/// With a marketplace index configured, Marketplace lists its plugins; Install downloads the chosen
/// one, checks its signature and installs it, signed by its publisher.
#[gpui_kit::test]
async fn the_marketplace_lists_the_indexs_plugins_and_install_downloads_and_installs_one(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let url = signed_market(&home, &host, &home.path().join("market"));
    host.set_market_url(Some(url));
    let (workbench, page, cx) = plugins_window(cx, &host);
    click_on(cx, "plugins-tab-marketplace");
    wait_long(cx, "the index's plugin is listed", |cx| offers(&page, "acme.hello", cx));
    assert!(installed(&host, "acme.hello").is_none());

    click_on(cx, "plugins-market-row-acme.hello");
    click_on(cx, "plugins-details-install");
    wait_long(cx, "the install page, or the plugin installed", |cx| {
        consent_dialog(&workbench, cx).is_some() || installed(&host, "acme.hello").is_some()
    });
    if consent_dialog(&workbench, cx).is_some() {
        click_on(cx, "consent-button-ok");
    }
    wait_long(cx, "the downloaded plugin is installed", |_| installed(&host, "acme.hello").is_some());
    let info = installed(&host, "acme.hello").expect("installed");
    assert_eq!((info.version.as_str(), info.publisher.as_deref()), ("1.0.0", Some("acme")));
}

/// Plugin Repository… in the gear menu shows the marketplace URL the host has stored; a new URL
/// saved there is the host's marketplace from then on, also for a host started again on the same
/// folder, and the page lists what that index offers.
#[gpui_kit::test]
async fn plugin_repository_shows_the_stored_url_and_saves_a_new_one_the_host_keeps(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let first = signed_market(&home, &host, &home.path().join("market"));
    let second = signed_market(&home, &host, &home.path().join("mirror"));
    host.set_market_url(Some(first.clone()));
    let (workbench, page, cx) = plugins_window(cx, &host);
    click_on(cx, "plugins-tab-marketplace");
    wait_long(cx, "the marketplace is read", |cx| offers(&page, "acme.hello", cx));

    click_on(cx, "plugins-gear");
    let gear = page.update(cx, |page, cx| page.gear_items(cx));
    let index = gear
        .iter()
        .position(|item| item.command == Some(GearCommand::EditRepository))
        .expect("Plugin Repository… is in the gear menu");
    click_on(cx, Box::leak(format!("plugins-gear-item-{index}").into_boxed_str()));
    let prompt = workbench.read_with(cx, |w, _| w.plugins_repository_prompt().cloned()).expect("the URL is asked");
    let field = prompt.read_with(cx, |p, _| p.field().cloned()).expect("a one-line field");
    assert_eq!(field.read_with(cx, |field, _| field.text().to_owned()), first, "the stored URL is shown");

    // Typed, not set: the dialog keeps what the field announces as changed.
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-a");
    cx.simulate_input(&second);
    cx.simulate_keystrokes("enter");
    wait_long(cx, "the new URL is stored", |_| host.market_url().as_deref() == Some(second.as_str()));
    assert!(workbench.read_with(cx, |w, _| w.plugins_repository_prompt().is_none()), "the dialog closes");
    wait_long(cx, "the new index is read", |cx| offers(&page, "acme.hello", cx));
    let sink = |_: Option<&str>, _: PluginEvent| {};
    let again = PluginHost::new(dir.path().join("plugins"), Arc::new(sink), ide_plugin::Limits::default())
        .expect("the plugin host starts again");
    assert_eq!(again.market_url().as_deref(), Some(second.as_str()), "the URL survives a restart");
}
