//! Reload Plugins reporting a plugin it could not reload.

use super::*;

/// An installed plugin whose uninstall was left unfinished cannot be reloaded: Reload Plugins says
/// so in a "Could not reload Hello" error notification with the reason, and the plugin stays
/// listed.
#[gpui_kit::test]
async fn reload_plugins_reports_the_plugin_it_could_not_reload(cx: &mut TestAppContext) {
    let home = start(cx);
    let dir = tempfile::tempdir().expect("a folder for the plugins");
    let (host, _) = test_plugin_host(dir.path());
    let folder = path_text(&fixture(&home, "hello"));
    let (workbench, page, cx) = plugins_window(cx, &host);
    let consent = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugins_consent_dialog().cloned());
    run_action(cx, "plugins.installFromDisk", Some(json!(folder)));
    wait_long(cx, "the install page shows", |cx| consent(cx).is_some());
    click_on(cx, "consent-button-ok");
    wait_long(cx, "the plugin is installed", |_| installed(&host, "hello").is_some());
    wait_long(cx, "and listed", |cx| listed(&page, "hello", cx));

    // An uninstall that stopped before its cleanup leaves its record in the plugins folder.
    let pending = dir.path().join("plugins").join(".uninstall");
    std::fs::create_dir_all(&pending).expect("the folder is made");
    std::fs::write(pending.join("hello.json"), "{}").expect("the record is written");
    run_action(cx, "plugins.reloadAll", None);
    wait_long(cx, "the failure is reported", |cx| {
        notification_titles(&workbench, cx).iter().any(|title| title == "Could not reload Hello")
    });
    let body = workbench.read_with(cx, |w, _| {
        let items = w.notifications().items();
        items.iter().find(|n| n.title == "Could not reload Hello").and_then(|n| n.body.clone())
    });
    assert!(body.is_some_and(|body| !body.is_empty()), "the reason is shown");
    assert!(installed(&host, "hello").is_some(), "the plugin is still installed");
}
