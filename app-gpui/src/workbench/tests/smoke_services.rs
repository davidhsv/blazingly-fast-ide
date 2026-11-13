use super::*;
use crate::run::store::SessionStatus;
use crate::services::view::ServicesView;

/// Run All, Restart All and Stop All on the Program group of Services, run through the registry on two
/// plain local processes: both rows say Running, a restart stops each and starts it again in its own
/// session (a later start), and Stop All leaves both rows Stopped.
#[gpui_kit::test]
async fn run_all_restart_all_and_stop_all_drive_every_configuration_of_a_type(cx: &mut TestAppContext) {
    let configs = vec![program("nap1", "/bin/sleep", "30"), program("nap2", "/bin/sleep", "30")];
    let (workbench, cx, _folders, _root, store) = run_project(cx, configs);
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
        .and_then(|view| view.downcast::<ServicesView>().ok())
        .expect("the Services tool window holds its view");
    let detail = |cx: &mut VisualTestContext, key: &str| {
        view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.key == key).and_then(|row| row.detail))
    };
    let rows_say = |cx: &mut VisualTestContext, text: &str| {
        ["run:config:nap1", "run:config:nap2"].iter().all(|key| detail(cx, key).as_deref() == Some(text))
    };
    let live = |cx: &mut VisualTestContext| -> Vec<(String, String, f64)> {
        store.read_with(cx, |store, _| {
            let mut live: Vec<(String, String, f64)> = store
                .sessions
                .iter()
                .filter(|s| s.status == SessionStatus::Running)
                .map(|s| (s.config.name.clone(), s.id.clone(), s.started_at))
                .collect();
            live.sort_by(|a, b| a.0.cmp(&b.0));
            live
        })
    };
    let group = Some(serde_json::Value::from("run:type:program"));

    run_action(cx, "run.services.runAll", group.clone());
    wait_long(cx, "both to run", |cx| live(cx).len() == 2 && rows_say(cx, "Running"));
    let first = live(cx);
    assert_eq!(first.iter().map(|(name, _, _)| name.as_str()).collect::<Vec<_>>(), ["nap1", "nap2"]);

    run_action(cx, "run.services.restartAll", group.clone());
    wait_long(cx, "both to run again", |cx| {
        let now = live(cx);
        now.len() == 2 && now.iter().zip(&first).all(|(new, old)| new.0 == old.0 && new.2 > old.2)
    });
    let sessions = store.read_with(cx, |store, _| store.sessions.len());
    assert_eq!(sessions, 2, "each configuration runs again in its own session");
    assert!(rows_say(cx, "Running"));

    run_action(cx, "run.services.stopAll", group);
    wait_long(cx, "both to end", |cx| {
        store.read_with(cx, |store, _| store.sessions.iter().all(|session| !session.is_live()))
    });
    wait_long(cx, "the rows to say so", |cx| rows_say(cx, "Stopped"));
}
