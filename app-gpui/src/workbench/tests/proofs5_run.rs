use super::*;
use crate::services::view::ServicesView;

/// Open in Browser on a configuration type's group opens nothing while no server of it runs, and the page of the
/// port its started server reports once one does. Remove from Services takes a configuration's row out of the
/// Services tree and leaves the others.
#[gpui_kit::test]
async fn open_in_browser_opens_the_started_servers_page_and_remove_from_services_hides_a_row(cx: &mut TestAppContext) {
    let web = ide_run::RunConfig {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), "echo 'Tomcat started on port 8123 (http)'; sleep 30".to_owned()],
        ..crate::run::configs::blank_config("web", "web", "program")
    };
    let (workbench, cx, _folders, _root, store) = run_project(cx, vec![web, program("nap", "/bin/sleep", "30")]);
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
        .and_then(|view| view.downcast::<ServicesView>().ok())
        .expect("the Services tool window holds its view");
    let row = |cx: &mut VisualTestContext, key: &str| {
        view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.key == key))
    };
    let group = Some(serde_json::Value::from("run:type:program"));
    assert!(row(cx, "run:config:nap").is_some() && row(cx, "run:config:web").is_some());

    run_action(cx, "run.services.openBrowser", group.clone());
    assert_eq!(cx.opened_url(), None, "no server runs, so there is no page to open");

    run_action(cx, "run.services.runAll", group.clone());
    wait_long(cx, "the server's port", |cx| {
        row(cx, "run:config:web").and_then(|row| row.detail).as_deref() == Some(":8123")
    });
    run_action(cx, "run.services.openBrowser", group.clone());
    wait_long(cx, "the page opens", |cx| cx.opened_url().is_some());
    assert_eq!(cx.opened_url().as_deref(), Some("http://localhost:8123"));

    run_action(cx, "run.services.stopAll", group);
    wait_long(cx, "the processes end", |cx| {
        store.read_with(cx, |store, _| store.sessions.iter().all(|session| !session.is_live()))
    });
    run_action(cx, "run.services.hideConfig", Some(serde_json::Value::from("nap")));
    wait_long(cx, "the row goes", |cx| row(cx, "run:config:nap").is_none());
    assert!(row(cx, "run:config:web").is_some(), "the other configuration stays in Services");
    assert!(row(cx, "run:type:program").is_some());
}
