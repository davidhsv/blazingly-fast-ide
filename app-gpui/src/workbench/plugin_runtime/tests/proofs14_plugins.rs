use super::*;
use ide_extension::package::{self, Index, IndexEntry, KeyEntry, Publisher, PublisherKey};
use ide_host::plugins::Consent;

/// The Hello fixture as plugin `id` named `name` at `version`, copied into the home `home`.
pub(super) fn hello_as(home: &Path, id: &str, name: &str, version: &str) -> PathBuf {
    let (id, name, version) = (id.to_owned(), name.to_owned(), version.to_owned());
    plugin_copy(home, "hello", move |text| {
        text.replace("id = \"hello\"", &format!("id = \"{id}\""))
            .replace("name = \"Hello\"", &format!("name = \"{name}\""))
            .replace("version = \"0.3.0\"", &format!("version = \"{version}\""))
    })
}

/// A marketplace in `market` offering each `(id, name, version)` as the Hello fixture packed and signed by the
/// publisher `acme`. Returns the index's path.
pub(super) fn market_offering(home: &Path, host: &PluginHost, market: &Path, offers: &[(&str, &str, &str)]) -> String {
    std::fs::create_dir_all(market).expect("the market folder is made");
    let key = PublisherKey::from_seed([7; 32]);
    let entries: Vec<IndexEntry> = offers
        .iter()
        .map(|&(id, name, version)| {
            let source = hello_as(home, id, name, version);
            let files = host.package_files_of(&source).expect("the package's files");
            let digest = package::package_digest(&source, &files).expect("the package's digest");
            let signature = key.sign("acme", id, version, &digest);
            let download = format!("{id}-{version}.oiplugin");
            package::pack(&source, &files, &market.join(&download), Some(&signature)).expect("the package is packed");
            IndexEntry {
                id: id.to_owned(),
                name: name.to_owned(),
                version: version.to_owned(),
                description: String::new(),
                authors: Vec::new(),
                publisher: "acme".to_owned(),
                download,
                digest,
                capabilities: Vec::new(),
                engine: "^1.0".to_owned(),
                categories: Vec::new(),
                icon: None,
            }
        })
        .collect();
    let publisher =
        Publisher { id: "acme".to_owned(), name: "ACME".to_owned(), keys: vec![KeyEntry::new(key.public_base64())] };
    let publishers = vec![publisher];
    let index = Index { format: package::INDEX_FORMAT, publishers, plugins: entries, ..Index::default() };
    let path = market.join("index.json");
    std::fs::write(&path, serde_json::to_string(&index).expect("the index serialises")).expect("the index is written");
    path.to_string_lossy().into_owned()
}

/// A plugin command that fails is reported in a notification named after the command; a command of a plugin
/// the host stopped (it failed and is disabled, and the host said why) is not reported again.
#[gpui_kit::test]
async fn reports_command_failures_but_not_twice_for_plugins_that_trapped(cx: &mut TestAppContext) {
    let home = start(cx);
    let hello = link(cx, &plugin_copy(home.path(), "hello", str::to_owned), &[], "word-count");
    // A build whose activation fails: the host stops the plugin for the session on its first use.
    let broken =
        plugin_copy(home.path(), "misbehave", |text| text.replace("version = \"0.0.2\"", "version = \"0.0.2-broken\""));
    let misbehave = link(cx, &broken, &[], "panic");
    let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    let answered = |cx: &mut VisualTestContext| cx.update(|_, cx| cx.global::<Plugins>().commands_answered());

    // No editor is open: Word Count of Active File fails in the plugin.
    let before = answered(cx);
    run_action(cx, &plugin_action_id(&hello, "word-count"), None);
    wait(cx, "the failure is reported", |cx| {
        notification_titles(&workbench, cx).iter().any(|title| title == "Hello: Word Count of Active File failed")
    });
    assert_eq!(answered(cx), before + 1);
    let body = workbench.read_with(cx, |w, _| {
        let notice = w.notifications().items().iter().find(|n| n.title.ends_with("Word Count of Active File failed"));
        notice.and_then(|notice| notice.body.clone())
    });
    assert!(body.is_some_and(|body| body.contains("no file is open")), "the notification says why");

    let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
    run_action(cx, &plugin_action_id(&misbehave, "panic"), None);
    wait(cx, "the command's answer", |cx| answered(cx) == before + 2);
    assert_eq!(host.info_of(&misbehave).map(|info| info.status).ok(), Some(PluginStatus::Failed));
    wait(cx, "the host says why", |cx| {
        notification_titles(&workbench, cx).iter().any(|title| title == "Plugin \u{201c}Misbehave\u{201d} was disabled")
    });
    let titles = notification_titles(&workbench, cx);
    assert!(!titles.iter().any(|title| title == "panic failed"), "the failure is not reported twice: {titles:?}");
}

/// Newer versions of two installed plugins in the marketplace are announced in one notification naming both,
/// with Update All and Show Plugins; Show Plugins opens Settings › Plugins.
#[gpui_kit::test]
async fn announces_updates_with_actions_to_install_them_or_show_the_plugins(cx: &mut TestAppContext) {
    let home = start(cx);
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    for (id, name) in [("acme.a", "A"), ("acme.b", "B")] {
        host.install_from_dir(&hello_as(home.path(), id, name, "1.0.0"), &Consent::all()).expect("the plugin installs");
    }
    let url = market_offering(
        home.path(),
        &host,
        &home.path().join("market"),
        &[("acme.a", "A", "1.1.0"), ("acme.b", "B", "2.0.0")],
    );
    let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    host.set_market_url(Some(url));
    host.refresh_market().expect("the marketplace is read");

    let notice = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| {
            w.notifications().items().iter().find(|n| n.title == "2 plugin updates available").cloned()
        })
    };
    wait(cx, "the updates are announced", |cx| notice(cx).is_some());
    let notice = notice(cx).expect("the notification");
    assert_eq!(notice.body.as_deref(), Some("A 1.1.0, B 2.0.0"));
    let labels: Vec<&str> = notice.actions.iter().map(|action| action.label.as_str()).collect();
    assert_eq!(labels, ["Update All", "Show Plugins"]);

    let show = EntryAction::Own(notice.actions[1].clone());
    workbench.update_in(cx, |w, window, cx| w.run_notification_action(show, window, cx));
    wait(cx, "Settings › Plugins opens", |cx| workbench.read_with(cx, |w, _| w.settings_plugins_page().is_some()));
}

/// Update All on the announcement of two updates installs both and says so.
#[gpui_kit::test]
async fn update_all_on_the_announcement_installs_the_updates(cx: &mut TestAppContext) {
    let home = start(cx);
    let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
    for (id, name) in [("acme.a", "A"), ("acme.b", "B")] {
        host.install_from_dir(&hello_as(home.path(), id, name, "1.0.0"), &Consent::all()).expect("the plugin installs");
    }
    let url = market_offering(
        home.path(),
        &host,
        &home.path().join("market"),
        &[("acme.a", "A", "1.1.0"), ("acme.b", "B", "2.0.0")],
    );
    let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
    host.set_market_url(Some(url));
    host.refresh_market().expect("the marketplace is read");
    let notice = |cx: &mut VisualTestContext| {
        workbench.read_with(cx, |w, _| {
            w.notifications().items().iter().find(|n| n.title == "2 plugin updates available").cloned()
        })
    };
    wait(cx, "the updates are announced", |cx| notice(cx).is_some());
    let update_all = EntryAction::Own(notice(cx).expect("the notification").actions[0].clone());
    workbench.update_in(cx, |w, window, cx| w.run_notification_action(update_all, window, cx));
    wait(cx, "the updates are installed", |_| {
        let version = |id: &str| host.info_of(id).map(|info| info.version).unwrap_or_default();
        version("acme.a") == "1.1.0" && version("acme.b") == "2.0.0"
    });
    wait(cx, "the window says so", |cx| {
        notification_titles(&workbench, cx).iter().any(|title| title == "Updated 2 plugins")
    });
}
