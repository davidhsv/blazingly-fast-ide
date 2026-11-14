use super::*;
use crate::workbench::background_tasks_view::{BackgroundTasksPopup, CancelButton};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Hands `event` to the workbench as the backend's event for its window.
fn send(workbench: &Entity<Workbench>, event: IdeEvent, cx: &mut VisualTestContext) {
    let event = BackendEvent { window: Some("w1".to_owned()), event };
    workbench.update(cx, |workbench, cx| workbench.backend_event(&event, cx));
}

/// The backend's report on task `id`.
fn task_event(id: u32, title: &str, done: bool, cancellable: bool) -> IdeEvent {
    IdeEvent::Task {
        id: ide_core::TaskId(id),
        title: title.to_owned(),
        message: None,
        fraction: None,
        done,
        cancellable,
    }
}

/// Moves the window's task list well past every show delay and linger.
fn later(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let far = now_ms() + tasks::SHOW_DELAY_MS + tasks::DONE_LINGER_MS + 1_000.0;
    workbench.update(cx, |workbench, cx| workbench.tick_tasks(far, cx));
}

/// The ids of the tasks the window lists.
fn listed(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<tasks::TaskId> {
    workbench.read_with(cx, |w, _| w.tasks().tasks().iter().map(|task| task.id.clone()).collect())
}

/// The Background Tasks popup that View › Background Tasks opens.
fn open_popup(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<BackgroundTasksPopup> {
    run_action(cx, "view.backgroundTasks", None);
    workbench.read_with(cx, |w, _| w.background_tasks_popup()).expect("Background Tasks is open")
}

/// A task that the backend replaces with another before the first one's completion arrives stays
/// listed: the late completion removes only the old task, with or without the old one shown first.
#[gpui_kit::test]
async fn keeps_a_replacement_task_when_the_old_completion_arrives(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    for delayed in [false, true] {
        send(&workbench, task_event(51, "Finding references", false, false), cx);
        if delayed {
            later(&workbench, cx);
            assert_eq!(listed(&workbench, cx), [tasks::TaskId::Number(51)]);
        }
        send(&workbench, task_event(52, "Finding references", false, false), cx);
        send(&workbench, task_event(51, "Finding references", true, false), cx);
        later(&workbench, cx);
        assert_eq!(listed(&workbench, cx), [tasks::TaskId::Number(52)], "delayed: {delayed}");
        assert_eq!(workbench.read_with(cx, |w, _| w.task_status()).as_deref(), Some("Finding references"));
        send(&workbench, task_event(52, "Finding references", true, false), cx);
        send(&workbench, task_event(52, "Finding references", true, false), cx);
        later(&workbench, cx);
        assert!(listed(&workbench, cx).is_empty(), "delayed: {delayed}");
        assert_eq!(workbench.read_with(cx, |w, _| w.task_status()), None);
    }
}

/// Indexing reported by the backend shows in Background Tasks with its Cancel button disabled and
/// the reason it cannot be stopped.
#[gpui_kit::test]
async fn indexing_says_why_it_cant_be_stopped(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    send(&workbench, IdeEvent::IndexState { indexing: true, files: 10, symbols: 20 }, cx);
    let popup = open_popup(&workbench, cx);
    let rows = popup.read_with(cx, |popup, _| popup.rows());
    let indexing = rows.iter().find(|row| row.title == "Indexing").expect("indexing is listed");
    assert_eq!(indexing.cancel, CancelButton::Disabled("Code insight needs the index; it can't be stopped".to_owned()));
    assert_eq!(indexing.message.as_deref(), Some("10 files · 20 symbols"));
}

/// A backend task reported cancellable gets a working Cancel button whose click stops it through
/// the backend's task cancel; one reported otherwise says it can't be cancelled.
#[gpui_kit::test]
async fn backend_tasks_cancellable_ones_cancel_through_cancel_task_the_others_say_they_cant(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    // Ids no other test reports: the backend's cancel handles are shared by the whole process.
    let (building, loading) = (913_041, 913_042);
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let _handle = ide_core::task::on_cancel(ide_core::TaskId(building), move || flag.store(true, Ordering::SeqCst));
    send(&workbench, task_event(building, "Building workspace", false, true), cx);
    send(&workbench, task_event(loading, "Loading", false, false), cx);
    later(&workbench, cx);

    let popup = open_popup(&workbench, cx);
    let rows = popup.read_with(cx, |popup, _| popup.rows());
    let index = |title: &str| rows.iter().position(|row| row.title == title).expect("listed");
    let (build_row, load_row) = (index("Building workspace"), index("Loading"));
    assert_eq!(rows[build_row].cancel, CancelButton::Working);
    assert_eq!(rows[load_row].cancel, CancelButton::Disabled("This task can't be cancelled".to_owned()));

    let selector: &'static str = Box::leak(format!("task-cancel:{build_row}").into_boxed_str());
    click_on(cx, selector);
    assert!(cancelled.load(Ordering::SeqCst), "the backend's cancel ran");
    let row = popup.read_with(cx, |popup, _| popup.rows()[build_row].clone());
    assert_eq!(row.cancel, CancelButton::Disabled("Cancelling…".to_owned()));
}

/// View › Background Tasks, run as an action, shows a task's progress and message; Escape closes it.
#[gpui_kit::test]
async fn shows_progress_and_uncancellable_tasks_through_an_action_without_the_status_bar(cx: &mut TestAppContext) {
    let _home = start(cx);
    let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
    cx.update(|window, _| window.activate_window());
    let scan = IdeEvent::Task {
        id: ide_core::TaskId(913_051),
        title: "Scanning".to_owned(),
        message: Some("Reading files".to_owned()),
        fraction: Some(0.25),
        done: false,
        cancellable: false,
    };
    send(&workbench, scan, cx);
    later(&workbench, cx);
    let popup = open_popup(&workbench, cx);
    let rows = popup.read_with(cx, |popup, _| popup.rows());
    let row = rows.iter().find(|row| row.title == "Scanning").expect("listed");
    assert_eq!((row.percent, row.message.as_deref()), (Some(25), Some("Reading files")));
    assert!(cx.debug_bounds("background-tasks").is_some(), "the popup is drawn");
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(workbench.read_with(cx, |w, _| w.background_tasks_popup().is_none()), "Escape closes it");
}
