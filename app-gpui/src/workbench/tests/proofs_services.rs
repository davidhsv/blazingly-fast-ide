use super::*;
use crate::run::store::SessionStatus;
use crate::services::view::ServicesView;

/// Run All on a configuration type's group in Services starts every configuration of the type; a server among
/// them that says which port it started on shows the port and can be opened in the browser while it runs; Stop
/// All ends them all, and the browser button goes with the port.
#[gpui_kit::test]
async fn run_all_and_stop_all_drive_a_type_group_and_a_started_servers_port_shows(cx: &mut TestAppContext) {
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
    let detail = |cx: &mut VisualTestContext, key: &str| {
        view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.key == key).and_then(|row| row.detail))
    };
    let status = |cx: &mut VisualTestContext, name: &str| {
        store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == name).map(|s| s.status))
    };
    let browser_disabled = |cx: &mut VisualTestContext| {
        let actions = view.read_with(cx, |view, cx| view.toolbar_actions(cx));
        actions.iter().find(|action| action.id == "open-browser").map(|action| action.disabled)
    };
    let group = Some(serde_json::Value::from("run:type:program"));

    run_action(cx, "run.services.runAll", group.clone());
    wait_long(cx, "both to run", |cx| {
        status(cx, "web") == Some(SessionStatus::Running) && status(cx, "nap") == Some(SessionStatus::Running)
    });
    wait_long(cx, "the server's port", |cx| detail(cx, "run:config:web").as_deref() == Some(":8123"));
    assert_eq!(detail(cx, "run:config:nap").as_deref(), Some("Running"));
    click_on(cx, "services-row:run:config:web");
    assert_eq!(browser_disabled(cx), Some(false), "a running server can be opened in the browser");

    run_action(cx, "run.services.stopAll", group);
    wait_long(cx, "both to end", |cx| {
        store.read_with(cx, |store, _| store.sessions.iter().all(|session| !session.is_live()))
    });
    wait_long(cx, "the rows to say so", |cx| detail(cx, "run:config:web").as_deref() == Some("Stopped"));
    assert_eq!(detail(cx, "run:config:nap").as_deref(), Some("Stopped"));
    assert_eq!(browser_disabled(cx), Some(true), "a stopped server has no page to open");
}
