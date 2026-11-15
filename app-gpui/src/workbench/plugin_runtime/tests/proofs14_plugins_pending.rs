use super::proofs14_plugins::{hello_as, market_offering};
use super::*;
use ide_host::plugins::Consent;

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
