//! The Background Tasks popup: every listed task with its progress, its message and its Cancel
//! button, which works, says "Cancelling…", or is disabled with the reason the task cannot stop.
//!
//! The popup owns no task: it shows a copy of the window's [`BackgroundTask`]s, kept up to date
//! from the entity that holds them ([`BackgroundTasksPopup::follow`]), and asks through
//! [`BackgroundTasksEvent::Cancel`] for a task to be stopped. Escape dismisses it.

use super::tasks::{BackgroundTask, TaskId, can_cancel};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::tooltip::WithTooltip as _;
use gpui_kit::{
    Anchor, App, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Pixels, Point, Render,
    SharedString, Subscription, Window, anchored, div, prelude::*, px, relative,
};

const WIDTH: f32 = 360.0;
/// The text a task's Cancel shows once it was pressed.
const CANCELLING: &str = "Cancelling…";

/// What a task's Cancel button is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CancelButton {
    /// A working button.
    Working,
    /// A disabled button saying why (`Cancelling…`, or why the task cannot be stopped).
    Disabled(String),
    /// No button: the task is done, or says nothing about stopping.
    Hidden,
}

/// A task as the popup draws it.
#[derive(Clone, Debug, PartialEq)]
pub struct TaskRow {
    pub id: TaskId,
    pub title: String,
    /// The progress in whole percent, when the task reports a fraction.
    pub percent: Option<u32>,
    pub message: Option<String>,
    pub cancel: CancelButton,
}

/// What the popup asks of whoever opened it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackgroundTasksEvent {
    /// The Cancel button of this task was pressed.
    Cancel(TaskId),
    /// Escape.
    Dismissed,
}

/// The button a task's state gives it: none once done; working while it can be stopped; else
/// disabled with "Cancelling…" or the reason it cannot be stopped.
pub fn cancel_button(task: &BackgroundTask) -> CancelButton {
    if task.done {
        return CancelButton::Hidden;
    }
    if can_cancel(task) {
        return CancelButton::Working;
    }
    let reason = if task.cancelling { Some(CANCELLING.to_owned()) } else { task.uncancellable.clone() };
    reason.map_or(CancelButton::Hidden, CancelButton::Disabled)
}

/// The rows of `tasks`, in their order.
pub fn task_rows(tasks: &[BackgroundTask]) -> Vec<TaskRow> {
    tasks
        .iter()
        .map(|task| TaskRow {
            id: task.id.clone(),
            title: task.title.clone(),
            percent: task.fraction.map(|fraction| (fraction.clamp(0.0, 1.0) * 100.0).round() as u32),
            message: task.message.clone(),
            cancel: cancel_button(task),
        })
        .collect()
}

pub struct BackgroundTasksPopup {
    focus: FocusHandle,
    /// The popup's bottom right corner, in window coordinates.
    position: Point<Pixels>,
    tasks: Vec<BackgroundTask>,
    key_claim: u64,
    _source: Option<Subscription>,
}

impl EventEmitter<BackgroundTasksEvent> for BackgroundTasksPopup {}

impl BackgroundTasksPopup {
    /// The popup over `tasks`, its bottom right corner at `position` (moved to stay inside the
    /// window).
    pub fn new(tasks: Vec<BackgroundTask>, position: Point<Pixels>, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        Self { focus, position, tasks, key_claim, _source: None }
    }

    /// Keeps the popup showing what `read` gives of `source` each time `source` changes.
    pub fn follow<T: 'static>(
        &mut self,
        source: &Entity<T>,
        read: fn(&T) -> Vec<BackgroundTask>,
        cx: &mut Context<Self>,
    ) {
        self._source = Some(cx.observe(source, move |popup: &mut Self, source: Entity<T>, cx| {
            let tasks = read(source.read(cx));
            popup.set_tasks(tasks, cx);
        }));
    }

    /// Shows `tasks` in place of those shown.
    pub fn set_tasks(&mut self, tasks: Vec<BackgroundTask>, cx: &mut Context<Self>) {
        if self.tasks != tasks {
            self.tasks = tasks;
            cx.notify();
        }
    }

    /// The rows shown, in order.
    pub fn rows(&self) -> Vec<TaskRow> {
        task_rows(&self.tasks)
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.key == "escape" && !keystroke.modifiers.modified() {
            cx.emit(BackgroundTasksEvent::Dismissed);
            return true;
        }
        false
    }

    fn render_row(&self, index: usize, row: TaskRow, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, muted, faint) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-faint"));
        let i18n = &mut cx.default_global::<Localization>().0;
        let title = i18n.tx(&row.title);
        let cancel_title = i18n.tx_built(&["Cancel ", ""], &[crate::platform::i18n::Param::Text(title.clone())]);
        let cancel = match row.cancel.clone() {
            CancelButton::Hidden => None,
            CancelButton::Working => {
                let id = row.id.clone();
                Some(
                    div()
                        .id(SharedString::from(format!("task-cancel-{index}")))
                        .debug_selector(move || format!("task-cancel:{index}"))
                        .with_tooltip(cancel_title)
                        .size(px(18.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::length(cx, "radius-small"))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(theme::icon(cx, "stop", 14.0, muted))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(BackgroundTasksEvent::Cancel(id.clone()))))
                        .into_any_element(),
                )
            }
            CancelButton::Disabled(reason) => Some(
                div()
                    .id(SharedString::from(format!("task-cancel-{index}")))
                    .debug_selector(move || format!("task-cancel:{index}"))
                    .with_tooltip(reason)
                    .size(px(18.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .opacity(0.4)
                    .child(theme::icon(cx, "stop", 14.0, faint))
                    .into_any_element(),
            ),
        };
        let fraction = row.percent.map(|percent| percent as f32 / 100.0);
        let bar = div().h(px(3.0)).w_full().rounded(px(2.0)).bg(theme::color(cx, "bg-hover")).child(
            div()
                .h_full()
                .rounded(px(2.0))
                .bg(theme::color(cx, "accent"))
                // An indeterminate task shows a faint full bar.
                .when_some(fraction, |el, fraction| el.w(relative(fraction)))
                .when(fraction.is_none(), |el| el.w_full().opacity(0.3)),
        );
        div()
            .debug_selector(move || format!("task-row:{index}"))
            .flex()
            .flex_col()
            .gap(px(4.0))
            .px(px(12.0))
            .py(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().flex_1().min_w_0().text_color(fg).child(SharedString::from(title)))
                    .children(
                        row.percent
                            .map(|percent| div().text_color(muted).child(SharedString::from(format!("{percent}%")))),
                    )
                    .children(cancel),
            )
            .child(bar)
            .children(row.message.map(|message| {
                div().text_size(theme::length(cx, "fs-small")).text_color(muted).child(SharedString::from(message))
            }))
    }
}

impl Focusable for BackgroundTasksPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BackgroundTasksPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows();
        let header = cx.default_global::<Localization>().0.tx("Background Tasks");
        let body: Vec<gpui_kit::AnyElement> = if rows.is_empty() {
            let empty = cx.default_global::<Localization>().0.tx("No background tasks");
            vec![
                div()
                    .px(px(12.0))
                    .py(px(6.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(empty))
                    .into_any_element(),
            ]
        } else {
            rows.into_iter()
                .enumerate()
                .map(|(index, row)| self.render_row(index, row, cx).into_any_element())
                .collect()
        };
        anchored().anchor(Anchor::BottomRight).position(self.position).snap_to_window().child(
            div()
                .track_focus(&self.focus)
                .debug_selector(|| "background-tasks".to_owned())
                .w(px(WIDTH))
                .py(px(6.0))
                .flex()
                .flex_col()
                .text_size(theme::length(cx, "fs-ui"))
                .bg(theme::color(cx, "bg-popup"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .rounded(theme::length(cx, "radius-popup"))
                .occlude()
                .child(
                    div()
                        .px(px(12.0))
                        .pb(px(4.0))
                        .text_color(theme::color(cx, "fg-subtle"))
                        .child(SharedString::from(header)),
                )
                .children(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::tasks::{Cancel, Change, TaskUpdate, Tasks};
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<BackgroundTasksEvent>>>;

    fn tasks() -> Tasks {
        let mut tasks = Tasks::default();
        tasks.upsert_task(
            TaskUpdate { cancel: Change::Set(Cancel::Producer), ..TaskUpdate::new("dl".into(), "Downloading JDK 25") },
            1.0,
        );
        tasks.upsert_task(
            TaskUpdate {
                uncancellable: Change::Set("This step can't be cancelled".to_owned()),
                ..TaskUpdate::new("inst".into(), "Installing update")
            },
            1.0,
        );
        tasks.upsert_task(
            TaskUpdate {
                message: Some("Reading files".to_owned()),
                fraction: Some(0.25),
                ..TaskUpdate::new("scan".into(), "Scanning")
            },
            1.0,
        );
        tasks
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        tasks: &Tasks,
    ) -> (Entity<BackgroundTasksPopup>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let shown = tasks.tasks().to_vec();
        let (popup, cx) =
            cx.add_window_view(move |_, cx| BackgroundTasksPopup::new(shown, point(px(600.0), px(500.0)), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&popup, move |_, event: &BackgroundTasksEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&popup.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    // src/workbench/tasks.test.tsx › cancelling
    #[gpui_kit::test]
    async fn the_popups_cancel_buttons_working_cancelling_and_disabled_with_the_reason(cx: &mut TestAppContext) {
        let mut model = tasks();
        let (popup, events, cx) = open(cx, &model);
        let rows = popup.read_with(cx, |popup, _| popup.rows());
        let buttons: Vec<(&str, &CancelButton)> = rows.iter().map(|row| (row.title.as_str(), &row.cancel)).collect();
        assert_eq!(
            buttons,
            [
                ("Downloading JDK 25", &CancelButton::Working),
                ("Installing update", &CancelButton::Disabled("This step can't be cancelled".to_owned())),
                ("Scanning", &CancelButton::Hidden),
            ]
        );
        let bounds = cx.debug_bounds("task-cancel:0").expect("the Cancel button is drawn");
        cx.simulate_click(bounds.center(), Modifiers::none());
        assert_eq!(*events.borrow(), [BackgroundTasksEvent::Cancel(TaskId::from("dl"))]);
        // Whoever owns the tasks cancels it; the popup then shows Cancelling… disabled.
        assert!(model.cancel_task(&TaskId::from("dl")).is_some());
        popup.update(cx, |popup, cx| popup.set_tasks(model.tasks().to_vec(), cx));
        let first = popup.read_with(cx, |popup, _| popup.rows()[0].clone());
        assert_eq!(first.cancel, CancelButton::Disabled("Cancelling…".to_owned()));
        assert_eq!(first.message.as_deref(), Some("Cancelling…"));
        // A disabled button does not ask again.
        cx.simulate_click(bounds.center(), Modifiers::none());
        assert_eq!(events.borrow().len(), 1);
    }

    // src/workbench/tasks.test.tsx (the popup's progress)
    #[gpui_kit::test]
    async fn shows_progress_and_uncancellable_tasks(cx: &mut TestAppContext) {
        let model = tasks();
        let (popup, events, cx) = open(cx, &model);
        let scan = popup.read_with(cx, |popup, _| popup.rows()[2].clone());
        assert_eq!((scan.percent, scan.message.as_deref()), (Some(25), Some("Reading files")));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [BackgroundTasksEvent::Dismissed]);
    }

    /// Escape closes the popup only while it has the keyboard.
    #[gpui_kit::test]
    async fn escape_does_nothing_while_the_popup_has_no_keyboard(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, &tasks());
        cx.update(|window, cx| {
            let elsewhere = cx.focus_handle();
            window.focus(&elsewhere, cx);
        });
        cx.simulate_keystrokes("escape");
        assert!(events.borrow().is_empty(), "no dismissal without the keyboard");
        cx.update(|window, cx| window.focus(&popup.focus_handle(cx), cx));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [BackgroundTasksEvent::Dismissed]);
    }

    /// Without tasks the popup says so.
    #[gpui_kit::test]
    async fn an_empty_list_says_there_are_no_background_tasks(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, &Tasks::default());
        assert!(popup.read_with(cx, |popup, _| popup.rows().is_empty()));
        assert!(cx.debug_bounds("background-tasks").is_some());
    }
}
