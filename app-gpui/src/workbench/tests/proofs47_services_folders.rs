use super::*;
use crate::run::store::SessionStatus;
use crate::services::model::NodeCommand;
use crate::services::view::ServicesView;
use std::cell::RefCell;
use std::rc::Rc;

/// The folder node the tests act on: it holds `a1` and `a2`.
const INFRA: &str = "run:folder:program:Infra";
const INFRA_ROW: &str = "services-row:run:folder:program:Infra";
const GROUP: &str = "run:type:program";

/// How a folder's button is pressed: on the Services toolbar with the folder's row selected, or as
/// the registered action with the folder's key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Through {
    Toolbar,
    Registry,
}

fn in_folder(mut config: ide_run::RunConfig, folder: &str) -> ide_run::RunConfig {
    config.extras.folder = Some(folder.to_owned());
    config
}

/// A program configuration that says it started a web server on `port`, then waits.
fn server(id: &str, port: u16) -> ide_run::RunConfig {
    ide_run::RunConfig {
        command: "/bin/sh".to_owned(),
        args: vec!["-c".to_owned(), format!("echo 'Tomcat started on port {port} (http)'; sleep 300")],
        ..crate::run::configs::blank_config(id, id, "program")
    }
}

/// A project whose Program configurations are in Services: `edge`, a server on port 8124 in the
/// folder Edge and first in the type's order, then the folder Infra with `a1`, which only waits, and
/// `a2`, a server on port 8123.
fn folder_project(
    cx: &mut TestAppContext,
) -> (Entity<ServicesView>, &mut VisualTestContext, [tempfile::TempDir; 2], Entity<RunStore>) {
    let configs = vec![
        in_folder(server("edge", 8124), "Edge"),
        in_folder(program("a1", "/bin/sleep", "300"), "Infra"),
        in_folder(server("a2", 8123), "Infra"),
    ];
    let (workbench, cx, folders, _root, store) = run_project(cx, configs);
    cx.update(|window, _| window.activate_window());
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
        .and_then(|view| view.downcast::<ServicesView>().ok())
        .expect("the Services tool window holds its view");
    (view, cx, folders, store)
}

/// The running session of the configuration `id`: its id and when it started.
fn live(store: &Entity<RunStore>, id: &str, cx: &mut VisualTestContext) -> Option<(String, f64)> {
    store.read_with(cx, |store, _| {
        let running = store.sessions.iter().find(|s| s.config.id == id && s.status == SessionStatus::Running);
        running.map(|session| (session.id.clone(), session.started_at))
    })
}

fn sessions_of(store: &Entity<RunStore>, id: &str, cx: &mut VisualTestContext) -> usize {
    store.read_with(cx, |store, _| store.sessions.iter().filter(|s| s.config.id == id).count())
}

fn detail(view: &Entity<ServicesView>, key: &str, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.key == key).and_then(|row| row.detail))
}

/// Whether the toolbar button `id` of the selection is enabled; `None` when it is not offered.
fn toolbar_enabled(view: &Entity<ServicesView>, id: &str, cx: &mut VisualTestContext) -> Option<bool> {
    let actions = view.read_with(cx, |view, cx| view.toolbar_actions(cx));
    actions.iter().find(|action| action.id == id).map(|action| !action.disabled)
}

/// Starts every Program configuration from the type's group and waits until the three run and the
/// servers' ports show.
fn run_everything(view: &Entity<ServicesView>, store: &Entity<RunStore>, cx: &mut VisualTestContext) {
    run_action(cx, "run.services.runAll", Some(serde_json::Value::from(GROUP)));
    wait_long(cx, "the three configurations to run with their ports", |cx| {
        ["edge", "a1", "a2"].iter().all(|id| live(store, id, cx).is_some())
            && detail(view, "run:config:a2", cx).as_deref() == Some(":8123")
            && detail(view, "run:config:edge", cx).as_deref() == Some(":8124")
    });
}

/// Presses the Infra folder's button `button` through `through`: `action` is its registered id.
fn press(view: &Entity<ServicesView>, through: Through, button: &str, action: &str, cx: &mut VisualTestContext) {
    match through {
        Through::Toolbar => {
            wait_long(cx, "the Infra folder's row", |cx| {
                view.read_with(cx, |view, _| view.rows().iter().any(|row| row.key == INFRA))
            });
            click_on(cx, INFRA_ROW);
            assert_eq!(view.read_with(cx, |view, _| view.selected().map(str::to_owned)).as_deref(), Some(INFRA));
            assert_eq!(toolbar_enabled(view, button, cx), Some(true), "{button} is offered for the folder");
            let selector: &'static str = match button {
                "run-all" => "services-action:run-all",
                "restart-all" => "services-action:restart-all",
                "stop-all" => "services-action:stop-all",
                "open-browser" => "services-action:open-browser",
                other => panic!("no folder button {other}"),
            };
            click_on(cx, selector);
        }
        Through::Registry => run_action(cx, action, Some(serde_json::Value::from(INFRA))),
    }
}

/// Run All on a folder starts the folder's configurations only: the one in another folder, first in
/// the type's order, is not started, and the folder's and the type's rows count two running.
fn run_all_starts_the_folders_configurations(through: Through, cx: &mut TestAppContext) {
    let (view, cx, _folders, store) = folder_project(cx);
    press(&view, through, "run-all", "run.services.runAll", cx);
    wait_long(cx, "the folder's two configurations to run", |cx| {
        live(&store, "a1", cx).is_some()
            && live(&store, "a2", cx).is_some()
            && detail(&view, INFRA, cx).as_deref() == Some("2 running")
    });
    assert_eq!(sessions_of(&store, "edge", cx), 0, "the configuration outside the folder is not started");
    assert_eq!(detail(&view, GROUP, cx).as_deref(), Some("2 running"));
    if through == Through::Toolbar {
        assert_eq!(toolbar_enabled(&view, "run-all", cx), Some(false), "every configuration of the folder runs");
    }
    close_backend_project(cx);
}

/// Stop All on a folder stops the folder's running configurations and leaves the one outside it
/// running, never asked to stop.
fn stop_all_stops_the_folders_configurations(through: Through, cx: &mut TestAppContext) {
    let (view, cx, _folders, store) = folder_project(cx);
    run_everything(&view, &store, cx);
    let edge = live(&store, "edge", cx);
    press(&view, through, "stop-all", "run.services.stopAll", cx);
    wait_long(cx, "the folder's configurations to end", |cx| {
        store.read_with(cx, |store, _| {
            let mut folder = store.sessions.iter().filter(|s| s.config.id == "a1" || s.config.id == "a2");
            folder.all(|s| s.stopped && !s.is_live())
        })
    });
    assert_eq!(live(&store, "edge", cx), edge, "the configuration outside the folder still runs");
    let asked = store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.config.id == "edge" && s.stopped));
    assert!(!asked, "the configuration outside the folder was not asked to stop");
    if through == Through::Toolbar {
        assert_eq!(toolbar_enabled(&view, "stop-all", cx), Some(false), "nothing of the folder runs");
    }
    close_backend_project(cx);
}

/// Restart All on a folder starts each of the folder's configurations again, later, and leaves the
/// run of the one outside it as it was.
fn restart_all_restarts_the_folders_configurations(through: Through, cx: &mut TestAppContext) {
    let (view, cx, _folders, store) = folder_project(cx);
    run_everything(&view, &store, cx);
    let started = |id: &str, cx: &mut VisualTestContext| live(&store, id, cx).map(|(_, at)| at);
    let (edge, a1, a2) = (live(&store, "edge", cx), started("a1", cx), started("a2", cx));
    press(&view, through, "restart-all", "run.services.restartAll", cx);
    wait_long(cx, "the folder's configurations to run again", |cx| {
        started("a1", cx).zip(a1).is_some_and(|(now, before)| now > before)
            && started("a2", cx).zip(a2).is_some_and(|(now, before)| now > before)
    });
    assert_eq!(live(&store, "edge", cx), edge, "the configuration outside the folder keeps its run");
    assert_eq!(sessions_of(&store, "edge", cx), 1);
    close_backend_project(cx);
}

/// Open in Browser on a folder opens the page of the folder's one running server at once; the
/// server outside the folder is not offered.
fn open_in_browser_opens_the_folders_server(through: Through, cx: &mut TestAppContext) {
    let (view, cx, _folders, store) = folder_project(cx);
    run_everything(&view, &store, cx);
    assert_eq!(cx.opened_url(), None);
    press(&view, through, "open-browser", "run.services.openBrowser", cx);
    wait_long(cx, "the page opens", |cx| cx.opened_url().is_some());
    assert_eq!(cx.opened_url().as_deref(), Some("http://localhost:8123"));
    close_backend_project(cx);
}

#[gpui_kit::test]
async fn run_all_on_a_folders_toolbar_starts_only_the_folders_configurations(cx: &mut TestAppContext) {
    run_all_starts_the_folders_configurations(Through::Toolbar, cx);
}

#[gpui_kit::test]
async fn run_services_run_all_on_a_folder_starts_only_the_folders_configurations(cx: &mut TestAppContext) {
    run_all_starts_the_folders_configurations(Through::Registry, cx);
}

#[gpui_kit::test]
async fn stop_all_on_a_folders_toolbar_stops_only_the_folders_configurations(cx: &mut TestAppContext) {
    stop_all_stops_the_folders_configurations(Through::Toolbar, cx);
}

#[gpui_kit::test]
async fn run_services_stop_all_on_a_folder_stops_only_the_folders_configurations(cx: &mut TestAppContext) {
    stop_all_stops_the_folders_configurations(Through::Registry, cx);
}

#[gpui_kit::test]
async fn restart_all_on_a_folders_toolbar_restarts_only_the_folders_configurations(cx: &mut TestAppContext) {
    restart_all_restarts_the_folders_configurations(Through::Toolbar, cx);
}

#[gpui_kit::test]
async fn run_services_restart_all_on_a_folder_restarts_only_the_folders_configurations(cx: &mut TestAppContext) {
    restart_all_restarts_the_folders_configurations(Through::Registry, cx);
}

#[gpui_kit::test]
async fn open_in_browser_on_a_folders_toolbar_opens_the_folders_server(cx: &mut TestAppContext) {
    open_in_browser_opens_the_folders_server(Through::Toolbar, cx);
}

#[gpui_kit::test]
async fn run_services_open_browser_on_a_folder_opens_the_folders_server(cx: &mut TestAppContext) {
    open_in_browser_opens_the_folders_server(Through::Registry, cx);
}

/// The ids of the actions the keystrokes `keys` run.
fn actions_run_by(keys: &str, cx: &mut VisualTestContext) -> Vec<String> {
    let ran: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = ran.clone();
    let listener = cx.update(|_, cx| {
        cx.global_mut::<ActionRegistry>().on_action_executed(move |id, _, _, _| sink.borrow_mut().push(id.into()))
    });
    cx.simulate_keystrokes(keys);
    cx.run_until_parked();
    cx.update(|_, cx| cx.global_mut::<ActionRegistry>().remove_listener(listener));
    ran.take()
}

fn context_on(cx: &mut VisualTestContext, key: &str) -> bool {
    cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
}

/// servicesViewFocus is on while the Services tree has the keyboard, where Add Service's key (⌘N)
/// opens the Add Service menu; with the editor focused it is off and the key does not reach Add
/// Service.
#[gpui_kit::test]
async fn add_services_key_works_in_the_services_tree_and_not_in_the_editor(cx: &mut TestAppContext) {
    let (workbench, cx, _folders, root, _store) = run_project(cx, vec![program("nap", "/bin/sleep", "30")]);
    cx.update(|window, _| window.activate_window());
    std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
    run_action(cx, "services.show", None);
    run_action(cx, "run.services.addType", Some(serde_json::Value::from("program")));
    let view = workbench
        .read_with(cx, |w, _| w.tool_window_view(SERVICES_TOOL_WINDOW))
        .and_then(|view| view.downcast::<ServicesView>().ok())
        .expect("the Services tool window holds its view");
    wait_long(cx, "the configuration's row", |cx| {
        view.read_with(cx, |view, _| view.rows().iter().any(|row| row.key == "run:config:nap"))
    });
    click_on(cx, "services-row:run:config:nap");
    // Read before the key, whose menu takes the keyboard; asserted after it, so that the key is
    // seen to reach Add Service only through servicesViewFocus.
    let tree_focused = context_on(cx, "servicesViewFocus");
    assert_eq!(actions_run_by("cmd-n", cx), ["services.addService"]);
    assert!(tree_focused, "the Services tree has the keyboard");
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<Vec<NodeCommand>>()).is_some(), "Add Service's menu opens");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();

    workbench.update_in(cx, |w, window, cx| w.open_project_file(root.join("notes.txt"), window, cx));
    wait_long(cx, "the editor to have the keyboard", |cx| context_on(cx, "editorFocus"));
    assert!(!context_on(cx, "servicesViewFocus"), "the editor has the keyboard");
    let ran = actions_run_by("cmd-n", cx);
    assert!(!ran.iter().any(|id| id == "services.addService"), "{ran:?}");
    assert!(workbench.read_with(cx, |w, _| w.open_menu::<Vec<NodeCommand>>()).is_none(), "no Add Service menu");
    close_backend_project(cx);
}
