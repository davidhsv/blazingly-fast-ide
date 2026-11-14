use super::*;
use crate::workbench::tasks::{BackgroundTask, can_cancel};

/// Where the fake server hears an opened or changed document.
const ON_CHANGE: &str = "    elif method in ('textDocument/didOpen', 'textDocument/didChange'):
        publish(params['textDocument']['uri'])";

/// The same, then, once its answers name a `progress` title, the start of cancellable work of that title.
const ON_CHANGE_WITH_PROGRESS: &str = "    elif method in ('textDocument/didOpen', 'textDocument/didChange'):
        publish(params['textDocument']['uri'])
        progress = script().get('progress')
        if progress and not counts.get('$progress'):
            counts['$progress'] = 1
            send({'jsonrpc': '2.0', 'method': '$/progress', 'params': {'token': 'work-1',
                  'value': {'kind': 'begin', 'title': progress, 'cancellable': True}}})";

/// [`open`] with the fake server's script made, before it starts, to report cancellable work as
/// [`ON_CHANGE_WITH_PROGRESS`] says.
fn open_reporting_work<'a>(cx: &'a mut TestAppContext, text: &str) -> (Fixture, &'a mut VisualTestContext) {
    cx.executor().allow_parking();
    let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a folder"));
    let dirs = AppDirs::under(home.path());
    let server = FakeServer::install(&dirs.config, home.path());
    let script = home.path().join("server.py");
    let source = std::fs::read_to_string(&script).expect("the server script is written");
    assert!(source.contains(ON_CHANGE), "the fake server hears changes where this fixture extends it");
    std::fs::write(&script, source.replacen(ON_CHANGE, ON_CHANGE_WITH_PROGRESS, 1)).expect("the script is extended");
    let root = folder.path().canonicalize().expect("the folder exists");
    std::fs::write(root.join("a.mock"), text).expect("the file is written");
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        crate::editor::actions::register(cx);
        workbench::register_actions(cx);
        cx.set_global(UiStorage::default());
        Backend::init(dirs, cx).expect("the backend starts");
        workbench::menu::init(cx);
        let host = cx.global::<Backend>().host.clone();
        host.global_settings
            .set(ide_workspace::settings::SettingsScope::Global, "ide.mode", json!("full"))
            .expect("Full mode is set");
        host.trust.decide(&root, true, false).expect("the project is trusted");
    });
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
    let file = root.join("a.mock");
    workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file, window, cx));
    let fixture = Fixture { workbench, root, server, _folders: (home, folder) };
    settle(cx, |cx| {
        let editor = fixture.editor(cx);
        cx.update(|window, cx| editor.is_some_and(|editor| editor.focus_handle(cx).is_focused(window)))
    });
    settle(cx, |_| !fixture.requests("textDocument/didOpen").is_empty());
    (fixture, cx)
}

/// The window's background task whose title names `title`.
fn task_titled(f: &Fixture, title: &str, cx: &mut VisualTestContext) -> Option<BackgroundTask> {
    f.workbench.read_with(cx, |w, _| w.tasks().tasks().iter().find(|task| task.title.contains(title)).cloned())
}

/// Work a language server reports as cancellable is listed among the background tasks; its cancel button in the
/// Background Tasks popup shows it cancelling and has the backend ask the server to stop that work.
#[gpui_kit::test]
async fn cancelling_a_language_servers_work_in_background_tasks_asks_the_server_to_stop_it(cx: &mut TestAppContext) {
    let (f, cx) = open_reporting_work(cx, "one\n");
    // The answers are read on every message: the server reports the work on the next change.
    f.server.answer(json!({ "progress": "Indexing mock" }));
    cx.simulate_input("x");
    settle(cx, |cx| task_titled(&f, "Indexing mock", cx).is_some());
    let task = task_titled(&f, "Indexing mock", cx).expect("the work is listed");
    assert!(can_cancel(&task), "the server made it cancellable");
    assert!(f.requests("window/workDoneProgress/cancel").is_empty(), "nothing is cancelled yet");

    f.run(cx, "view.backgroundTasks");
    let index = f
        .workbench
        .read_with(cx, |w, _| w.tasks().tasks().iter().position(|listed| listed.id == task.id))
        .expect("the popup lists the work");
    let selector: &'static str = Box::leak(format!("task-cancel:{index}").into_boxed_str());
    settle(cx, |cx| cx.debug_bounds(selector).is_some());
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), Modifiers::default());
    cx.run_until_parked();

    let message = task_titled(&f, "Indexing mock", cx).and_then(|task| task.message);
    assert_eq!(message.as_deref(), Some("Cancelling…"));
    settle(cx, |_| !f.requests("window/workDoneProgress/cancel").is_empty());
    let asked = f.requests("window/workDoneProgress/cancel");
    assert_eq!(asked[0]["params"], json!({ "token": "work-1" }));
    f.close(cx);
}
