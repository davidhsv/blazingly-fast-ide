//! Background task progress: what the status bar and the Background Tasks popup show, fed by the
//! backend's `task` / `indexState` events and by the features' own work.
//!
//! [`Tasks`] does no I/O and reads no clock. Stopping a task is data: [`Tasks::cancel_task`]
//! returns the [`CancelEffect`] its caller carries out. Delays are deadlines: a method given the
//! time first does what fell due before it, and [`Tasks::next_deadline`] says when
//! [`Tasks::expire`] is next worth calling.

use crate::platform::i18n::{DEFAULT_LOCALE, format_number};
use std::collections::{BTreeMap, VecDeque};

/// How long a finished task keeps showing (with its full progress bar).
pub const DONE_LINGER_MS: f64 = 600.0;
/// Backend tasks that finish sooner (a language server's per-keystroke work) never reach the
/// status bar.
pub const SHOW_DELAY_MS: f64 = 500.0;
pub const INDEX_TASK_ID: &str = "indexing";

/// A task's identity: the backend numbers its tasks, the UI names its own.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TaskId {
    Number(u64),
    Name(String),
}

impl From<u64> for TaskId {
    fn from(id: u64) -> Self {
        Self::Number(id)
    }
}

impl From<&str> for TaskId {
    fn from(id: &str) -> Self {
        Self::Name(id.to_owned())
    }
}

/// How a task is stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cancel {
    /// The backend stops it: `cancel_task` with this id.
    Backend(u64),
    /// It waits in this queue: cancelling drops it.
    Queue(String),
    /// Its producer stops it; the caller knows how, by the task's id.
    Producer,
}

/// What the caller of [`Tasks::cancel_task`] has to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelEffect {
    /// Call the backend's `cancel_task` with this id.
    Backend(u64),
    /// Tell the task's producer to stop.
    Producer(TaskId),
    /// The task left its queue: its work never starts, and whoever waits for it gets a
    /// cancellation (an error of kind `cancelled`).
    Dequeued(TaskId),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BackgroundTask {
    pub id: TaskId,
    pub title: String,
    pub message: Option<String>,
    /// 0..1, or `None` when indeterminate.
    pub fraction: Option<f64>,
    pub done: bool,
    /// When the task was first listed, in milliseconds since the epoch.
    pub started_at: f64,
    /// How to stop the task (a cancel button in the Background Tasks popup). Stop it through
    /// [`Tasks::cancel_task`].
    pub cancel: Option<Cancel>,
    /// Why the task can't be stopped (a disabled cancel button with this tooltip), when it has
    /// no `cancel`.
    pub uncancellable: Option<String>,
    /// Cancel was requested: shown as "Cancelling…" until the task ends, then removed at once.
    pub cancelling: bool,
    /// Waiting for the tasks ahead of it in its queue; cancelling drops it.
    pub queued: bool,
}

/// Whether `task` offers a working Cancel now.
pub fn can_cancel(task: &BackgroundTask) -> bool {
    task.cancel.is_some() && !task.done && !task.cancelling
}

/// What an update does to an optional property of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change<T> {
    /// Leave it as it is.
    Keep,
    /// The task has none (any more).
    Clear,
    Set(T),
}

impl<T> Change<T> {
    fn apply(self, slot: &mut Option<T>) {
        match self {
            Self::Keep => {}
            Self::Clear => *slot = None,
            Self::Set(value) => *slot = Some(value),
        }
    }

    /// The property of a task this update starts.
    fn initial(self) -> Option<T> {
        match self {
            Self::Set(value) => Some(value),
            Self::Keep | Self::Clear => None,
        }
    }
}

/// The state a task's producer reports.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskUpdate {
    pub id: TaskId,
    pub title: String,
    pub message: Option<String>,
    /// 0..1, or `None` when indeterminate.
    pub fraction: Option<f64>,
    pub done: bool,
    pub cancel: Change<Cancel>,
    pub uncancellable: Change<String>,
}

impl TaskUpdate {
    /// A running task without message or progress, its cancel properties kept as they are.
    pub fn new(id: TaskId, title: &str) -> Self {
        Self {
            id,
            title: title.to_owned(),
            message: None,
            fraction: None,
            done: false,
            cancel: Change::Keep,
            uncancellable: Change::Keep,
        }
    }
}

/// The backend's `task` event: progress of one of its tasks.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskEvent {
    pub id: u64,
    pub title: String,
    pub message: Option<String>,
    pub fraction: Option<f64>,
    pub done: bool,
    /// The UI may stop it with `cancel_task(id)`.
    pub cancellable: bool,
}

/// What the index holds while the project is being indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexingCounts {
    pub files: u64,
    pub symbols: u64,
}

/// Whether work handed to [`Tasks::queue_task`] runs now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueStart {
    /// Start the work now.
    Started,
    /// The work waits, listed as queued, until [`Tasks::finish_queued`] returns its id.
    Queued,
}

/// Tasks that run a few at a time, the rest waiting (listed, cancellable).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Queue {
    limit: usize,
    running: usize,
    waiting: VecDeque<TaskId>,
}

#[derive(Debug, Clone, PartialEq)]
enum TimerKind {
    /// A finished task leaves the list.
    Remove(TaskId),
    /// A task that did not end at once is listed, in the state it last reported.
    Show(TaskUpdate),
}

#[derive(Debug, Clone, PartialEq)]
struct Timer {
    due: f64,
    kind: TimerKind,
}

impl Timer {
    fn shows(&self, id: &TaskId) -> bool {
        matches!(&self.kind, TimerKind::Show(update) if update.id == *id)
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tasks {
    /// In the order they were first listed.
    tasks: Vec<BackgroundTask>,
    indexing: Option<IndexingCounts>,
    queues: BTreeMap<String, Queue>,
    /// What is still to happen, in the order it was scheduled.
    timers: Vec<Timer>,
}

impl Tasks {
    /// The listed tasks, oldest first.
    pub fn tasks(&self) -> &[BackgroundTask] {
        &self.tasks
    }

    pub fn task(&self, id: &TaskId) -> Option<&BackgroundTask> {
        self.tasks.iter().find(|task| task.id == *id)
    }

    /// The index counts while the project is being indexed.
    pub fn indexing(&self) -> Option<IndexingCounts> {
        self.indexing
    }

    /// Lists a task or brings it up to date. A finished task lingers for [`DONE_LINGER_MS`];
    /// a cancelled one leaves at once, without the finished (full) progress bar.
    pub fn upsert_task(&mut self, update: TaskUpdate, now_ms: f64) {
        self.expire(now_ms);
        self.apply(update, now_ms);
    }

    fn apply(&mut self, update: TaskUpdate, now_ms: f64) {
        let TaskUpdate { id, title, message, fraction, done, cancel, uncancellable } = update;
        let mut cancelled = false;
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id) {
            cancelled = task.cancelling;
            task.title = title;
            // Progress that arrives after Cancel keeps saying "Cancelling…".
            if !cancelled {
                task.message = message;
            }
            task.fraction = fraction;
            task.done = done;
            cancel.apply(&mut task.cancel);
            uncancellable.apply(&mut task.uncancellable);
            task.queued = false;
        } else if !done {
            self.tasks.push(BackgroundTask {
                id: id.clone(),
                title,
                message,
                fraction,
                done,
                started_at: now_ms,
                cancel: cancel.initial(),
                uncancellable: uncancellable.initial(),
                cancelling: false,
                queued: false,
            });
        }
        if !done {
            return;
        }
        if cancelled {
            self.remove_task(&id);
        } else {
            // By id only: whatever is listed under it when the time comes leaves.
            self.timers.push(Timer { due: now_ms + DONE_LINGER_MS, kind: TimerKind::Remove(id) });
        }
    }

    pub fn remove_task(&mut self, id: &TaskId) {
        self.tasks.retain(|task| task.id != *id);
    }

    /// Cancels task `id`: it shows "Cancelling…" until its producer ends it. `None` when the
    /// task has no working Cancel (now).
    #[must_use]
    pub fn cancel_task(&mut self, id: &TaskId) -> Option<CancelEffect> {
        let task = self.tasks.iter_mut().find(|task| task.id == *id).filter(|task| can_cancel(task))?;
        task.cancelling = true;
        task.message = Some("Cancelling…".to_owned());
        match task.cancel.clone()? {
            Cancel::Backend(backend_id) => Some(CancelEffect::Backend(backend_id)),
            Cancel::Producer => Some(CancelEffect::Producer(id.clone())),
            Cancel::Queue(queue) => {
                let waiting = &mut self.queues.get_mut(&queue)?.waiting;
                // The listed row belongs to the work queued last under this id.
                let index = waiting.iter().rposition(|waiter| waiter == id)?;
                waiting.remove(index);
                self.remove_task(id);
                Some(CancelEffect::Dequeued(id.clone()))
            }
        }
    }

    /// The backend's `indexState` event: indexing is listed as a task that can't be stopped.
    pub fn set_indexing(&mut self, indexing: bool, files: u64, symbols: u64, now_ms: f64) {
        self.indexing = indexing.then_some(IndexingCounts { files, symbols });
        let count = |n: u64| format_number(n as f64, DEFAULT_LOCALE);
        self.upsert_task(
            TaskUpdate {
                id: TaskId::from(INDEX_TASK_ID),
                title: "Indexing".to_owned(),
                message: indexing.then(|| format!("{} files · {} symbols", count(files), count(symbols))),
                fraction: None,
                done: !indexing,
                cancel: Change::Keep,
                uncancellable: Change::Set("Code insight needs the index; it can't be stopped".to_owned()),
            },
            now_ms,
        );
    }

    /// Asks to run work in `queue`, where `limit` run at a time (the limit of the first request
    /// of a queue counts). While it waits, task `id` is listed as queued and its Cancel drops it
    /// ([`CancelEffect::Dequeued`]). Once started, the work reports its own progress with
    /// [`Self::upsert_task`] under `id` and ends the task. Whoever runs the work calls
    /// [`Self::finish_queued`] when it ends, however it ends.
    #[must_use]
    pub fn queue_task(&mut self, queue: &str, id: TaskId, title: &str, limit: usize, now_ms: f64) -> QueueStart {
        self.expire(now_ms);
        let slot = self.queues.entry(queue.to_owned()).or_insert_with(|| Queue {
            limit,
            running: 0,
            waiting: VecDeque::new(),
        });
        if slot.running < slot.limit && slot.waiting.is_empty() {
            slot.running += 1;
            return QueueStart::Started;
        }
        slot.waiting.push_back(id.clone());
        self.tasks.retain(|task| task.id != id);
        self.tasks.push(BackgroundTask {
            id,
            title: title.to_owned(),
            message: Some("Waiting for other tasks".to_owned()),
            fraction: None,
            done: false,
            started_at: now_ms,
            cancel: Some(Cancel::Queue(queue.to_owned())),
            uncancellable: None,
            cancelling: false,
            queued: true,
        });
        QueueStart::Queued
    }

    /// Work of `queue` ended: the ids of the waiting work to start now, in order.
    #[must_use]
    pub fn finish_queued(&mut self, queue: &str) -> Vec<TaskId> {
        let Some(slot) = self.queues.get_mut(queue) else { return Vec::new() };
        slot.running = slot.running.saturating_sub(1);
        let mut started = Vec::new();
        while slot.running < slot.limit {
            let Some(id) = slot.waiting.pop_front() else { break };
            slot.running += 1;
            started.push(id);
        }
        // The queue's Cancel is spent; the work sets its own (or says why it has none).
        for task in self.tasks.iter_mut().filter(|task| started.contains(&task.id)) {
            task.queued = false;
            task.cancel = None;
        }
        started
    }

    /// [`Self::upsert_task`], except that a task ending within [`SHOW_DELAY_MS`] of its start is
    /// never shown.
    pub fn upsert_task_soon(&mut self, update: TaskUpdate, now_ms: f64) {
        self.expire(now_ms);
        if self.task(&update.id).is_some() {
            self.apply(update, now_ms);
            return;
        }
        let pending = self.timers.iter().position(|timer| timer.shows(&update.id));
        match (pending, update.done) {
            (Some(index), true) => {
                self.timers.remove(index);
            }
            (None, true) => {}
            (Some(index), false) => self.timers[index].kind = TimerKind::Show(update),
            (None, false) => self.timers.push(Timer { due: now_ms + SHOW_DELAY_MS, kind: TimerKind::Show(update) }),
        }
    }

    /// The backend's `task` event. Cancellable tasks cancel through the backend; the others say
    /// they can't be cancelled.
    pub fn on_task_event(&mut self, event: TaskEvent, now_ms: f64) {
        let (cancel, uncancellable) = if event.cancellable {
            (Change::Set(Cancel::Backend(event.id)), Change::Clear)
        } else {
            (Change::Clear, Change::Set("This task can't be cancelled".to_owned()))
        };
        self.upsert_task_soon(
            TaskUpdate {
                id: TaskId::Number(event.id),
                title: event.title,
                message: event.message,
                fraction: event.fraction,
                done: event.done,
                cancel,
                uncancellable,
            },
            now_ms,
        );
    }

    /// Forgets the tasks still waiting to be shown (the event source went away).
    pub fn clear_pending(&mut self) {
        self.timers.retain(|timer| !matches!(timer.kind, TimerKind::Show(_)));
    }

    /// When [`Self::expire`] next has something to do.
    pub fn next_deadline(&self) -> Option<f64> {
        self.timers.iter().map(|timer| timer.due).min_by(|a, b| a.total_cmp(b))
    }

    /// Does what fell due by `now_ms`, in the order it fell due: finished tasks leave, and tasks
    /// that outlived [`SHOW_DELAY_MS`] are listed.
    pub fn expire(&mut self, now_ms: f64) {
        if !self.timers.iter().any(|timer| timer.due <= now_ms) {
            return;
        }
        let (mut due, waiting): (Vec<Timer>, Vec<Timer>) =
            std::mem::take(&mut self.timers).into_iter().partition(|timer| timer.due <= now_ms);
        self.timers = waiting;
        // Stable: what fell due at the same time happens in the order it was scheduled.
        due.sort_by(|a, b| a.due.total_cmp(&b.due));
        for timer in due {
            match timer.kind {
                TimerKind::Remove(id) => self.remove_task(&id),
                TimerKind::Show(update) => self.apply(update, now_ms),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> TaskId {
        TaskId::from(name)
    }

    fn update(name: &str, title: &str) -> TaskUpdate {
        TaskUpdate::new(id(name), title)
    }

    fn event(id: u64, title: &str, cancellable: bool) -> TaskEvent {
        TaskEvent { id, title: title.to_owned(), message: None, fraction: None, done: false, cancellable }
    }

    fn ids(tasks: &Tasks) -> Vec<TaskId> {
        tasks.tasks().iter().map(|task| task.id.clone()).collect()
    }

    // src/workbench/tasks.test.tsx › task queues › runs one task at a time; the others wait, listed as queued
    #[test]
    fn runs_one_task_at_a_time_the_others_wait_listed_as_queued() {
        let mut tasks = Tasks::default();
        assert_eq!(tasks.queue_task("q1", id("a"), "A", 1, 0.0), QueueStart::Started);
        tasks.upsert_task(update("a", "A"), 0.0);
        assert_eq!(tasks.queue_task("q1", id("b"), "B", 1, 0.0), QueueStart::Queued);
        let b = tasks.task(&id("b")).unwrap();
        assert!(b.queued);
        assert_eq!(b.message.as_deref(), Some("Waiting for other tasks"));
        // The first work ends: the second starts.
        assert_eq!(tasks.finish_queued("q1"), [id("b")]);
        assert!(!tasks.task(&id("b")).unwrap().queued);
        assert!(tasks.finish_queued("q1").is_empty());
    }

    // src/workbench/tasks.test.tsx › task queues › cancelling a queued task drops it at once: its work never
    // starts and its caller sees a cancellation
    #[test]
    fn cancelling_a_queued_task_drops_it_at_once() {
        let mut tasks = Tasks::default();
        assert_eq!(tasks.queue_task("q2", id("run"), "Running", 1, 0.0), QueueStart::Started);
        assert_eq!(tasks.queue_task("q2", id("wait"), "Waiting", 1, 0.0), QueueStart::Queued);
        assert_eq!(tasks.queue_task("q2", id("next"), "Next", 1, 0.0), QueueStart::Queued);

        assert_eq!(tasks.cancel_task(&id("wait")), Some(CancelEffect::Dequeued(id("wait"))));
        assert_eq!(tasks.task(&id("wait")), None);

        // The running work ends: only the work still queued starts.
        assert_eq!(tasks.finish_queued("q2"), [id("next")]);
        assert!(tasks.finish_queued("q2").is_empty());
    }

    // src/workbench/tasks.test.tsx › task queues › the queue's Cancel is gone once the work starts (the work
    // brings its own)
    #[test]
    fn the_queues_cancel_is_gone_once_the_work_starts() {
        let mut tasks = Tasks::default();
        assert_eq!(tasks.queue_task("q3", id("x"), "X", 1, 0.0), QueueStart::Started);
        assert_eq!(tasks.queue_task("q3", id("y"), "Y", 1, 0.0), QueueStart::Queued);
        assert!(can_cancel(tasks.task(&id("y")).unwrap()));
        assert_eq!(tasks.finish_queued("q3"), [id("y")]);
        let y = tasks.task(&id("y")).unwrap();
        assert!(!y.queued);
        assert_eq!(y.cancel, None);
        assert!(tasks.finish_queued("q3").is_empty());
    }

    fn keeps_a_replacement_task_when_the_old_completion_arrives(delayed: bool) {
        let mut tasks = Tasks::default();
        let mut now = 0.0;
        let running = |id: u64| event(id, "Finding references", false);
        let done = |id: u64| TaskEvent { done: true, ..event(id, "Finding references", false) };
        tasks.on_task_event(running(51), now);
        if delayed {
            now += 600.0;
            tasks.expire(now);
        }
        tasks.on_task_event(running(52), now);
        tasks.on_task_event(done(51), now);
        now += 600.0;
        tasks.expire(now);
        assert_eq!(ids(&tasks), [TaskId::Number(52)]);
        tasks.on_task_event(done(52), now);
        tasks.on_task_event(done(52), now);
        now += 600.0;
        tasks.expire(now);
        assert!(tasks.tasks().is_empty());
        assert_eq!(tasks.next_deadline(), None);
    }

    // src/workbench/tasks.test.tsx › cancelling › keeps a replacement task when the old completion arrives
    // (delayed: false)
    #[test]
    fn keeps_a_replacement_task_when_the_old_completion_arrives_at_once() {
        keeps_a_replacement_task_when_the_old_completion_arrives(false);
    }

    // src/workbench/tasks.test.tsx › cancelling › keeps a replacement task when the old completion arrives
    // (delayed: true)
    #[test]
    fn keeps_a_replacement_task_when_the_old_completion_arrives_delayed() {
        keeps_a_replacement_task_when_the_old_completion_arrives(true);
    }

    // src/workbench/tasks.test.tsx › cancelling › shows Cancelling… until the task ends, then removes it
    // without the finished bar
    #[test]
    fn shows_cancelling_until_the_task_ends_then_removes_it_without_the_finished_bar() {
        let mut tasks = Tasks::default();
        let progress = |message: &str, fraction: f64| TaskUpdate {
            message: Some(message.to_owned()),
            fraction: Some(fraction),
            cancel: Change::Set(Cancel::Producer),
            ..update("sync", "Syncing")
        };
        tasks.upsert_task(progress("Resolving", 0.3), 0.0);
        // The producer is told to stop once.
        assert_eq!(tasks.cancel_task(&id("sync")), Some(CancelEffect::Producer(id("sync"))));
        assert_eq!(tasks.cancel_task(&id("sync")), None);
        let sync = tasks.task(&id("sync")).unwrap();
        assert!(sync.cancelling);
        assert_eq!(sync.message.as_deref(), Some("Cancelling…"));
        tasks.upsert_task(progress("Downloading", 0.4), 0.0);
        assert_eq!(tasks.task(&id("sync")).unwrap().message.as_deref(), Some("Cancelling…"));
        tasks.upsert_task(TaskUpdate { fraction: Some(1.0), done: true, ..update("sync", "Syncing") }, 0.0);
        assert_eq!(tasks.task(&id("sync")), None);
    }

    // src/workbench/tasks.test.tsx › cancelling › the popup's cancel buttons: working, Cancelling…, and
    // disabled with the reason
    #[test]
    fn cancel_buttons_working_cancelling_and_disabled_with_the_reason() {
        let mut tasks = Tasks::default();
        tasks.upsert_task(
            TaskUpdate { cancel: Change::Set(Cancel::Producer), ..update("dl", "Downloading JDK 25") },
            0.0,
        );
        tasks.upsert_task(
            TaskUpdate {
                uncancellable: Change::Set("This step can't be cancelled".to_owned()),
                ..update("inst", "Installing update")
            },
            0.0,
        );
        assert!(can_cancel(tasks.task(&id("dl")).unwrap()));
        assert_eq!(tasks.cancel_task(&id("dl")), Some(CancelEffect::Producer(id("dl"))));
        // Now a disabled "Cancelling…" button.
        let dl = tasks.task(&id("dl")).unwrap();
        assert!(dl.cancelling && !can_cancel(dl));
        // A disabled button with the reason.
        let inst = tasks.task(&id("inst")).unwrap();
        assert!(!can_cancel(inst));
        assert_eq!(inst.uncancellable.as_deref(), Some("This step can't be cancelled"));
        assert_eq!(tasks.cancel_task(&id("inst")), None);
    }

    // src/workbench/tasks.test.tsx › cancelling › indexing says why it can't be stopped
    #[test]
    fn indexing_says_why_it_cant_be_stopped() {
        let mut tasks = Tasks::default();
        tasks.set_indexing(true, 10, 20, 0.0);
        let indexing = tasks.task(&id(INDEX_TASK_ID)).unwrap();
        assert!(indexing.uncancellable.as_deref().is_some_and(|reason| !reason.is_empty()));
        assert_eq!(indexing.cancel, None);
    }

    // src/workbench/tasks.test.tsx › cancelling › backend tasks: cancellable ones cancel through cancel_task,
    // the others say they can't
    #[test]
    fn backend_tasks_cancel_through_cancel_task_or_say_they_cant() {
        let mut tasks = Tasks::default();
        tasks.on_task_event(event(41, "Building workspace", true), 0.0);
        tasks.on_task_event(event(42, "Loading", false), 0.0);
        tasks.expire(600.0);
        assert_eq!(tasks.cancel_task(&TaskId::Number(41)), Some(CancelEffect::Backend(41)));
        let loading = tasks.task(&TaskId::Number(42)).unwrap();
        assert_eq!(loading.cancel, None);
        assert_eq!(loading.uncancellable.as_deref(), Some("This task can't be cancelled"));
    }

    // src/workbench/tasks.test.tsx › shows progress and uncancellable tasks through an action without the
    // status bar
    #[test]
    fn lists_a_task_with_its_progress_and_no_cancel() {
        let mut tasks = Tasks::default();
        tasks.upsert_task(
            TaskUpdate {
                message: Some("Reading files".to_owned()),
                fraction: Some(0.25),
                ..update("scan", "Scanning")
            },
            0.0,
        );
        let scan = tasks.task(&id("scan")).unwrap();
        assert_eq!((scan.message.as_deref(), scan.fraction), (Some("Reading files"), Some(0.25)));
        assert!(!can_cancel(scan));
        assert_eq!(scan.uncancellable, None);
    }

    // src/workbench/Workbench.test.tsx › background tasks › tracks task events and shows the current task
    #[test]
    fn tracks_task_events_and_lists_the_tasks_that_do_not_end_at_once() {
        let mut tasks = Tasks::default();
        tasks.on_task_event(event(6, "Publish Diagnostics", false), 0.0);
        tasks.on_task_event(TaskEvent { fraction: Some(0.5), ..event(7, "Building index", false) }, 0.0);
        assert!(tasks.tasks().is_empty());
        assert_eq!(tasks.next_deadline(), Some(SHOW_DELAY_MS));
        tasks.on_task_event(
            TaskEvent { fraction: Some(1.0), done: true, ..event(6, "Publish Diagnostics", false) },
            0.0,
        );
        tasks.expire(600.0);
        assert_eq!(ids(&tasks), [TaskId::Number(7)]);
        assert_eq!(tasks.tasks()[0].fraction, Some(0.5));
        tasks.set_indexing(true, 10, 20, 600.0);
        assert_eq!(tasks.tasks().len(), 2);
    }

    #[test]
    fn indexing_reports_its_counts_and_leaves_when_done() {
        let mut tasks = Tasks::default();
        tasks.set_indexing(true, 12_345, 1_234_567, 0.0);
        assert_eq!(tasks.indexing(), Some(IndexingCounts { files: 12_345, symbols: 1_234_567 }));
        let indexing = tasks.task(&id(INDEX_TASK_ID)).unwrap();
        assert_eq!(indexing.message.as_deref(), Some("12,345 files · 1,234,567 symbols"));
        assert_eq!(tasks.cancel_task(&id(INDEX_TASK_ID)), None);

        tasks.set_indexing(false, 12_345, 1_234_567, 1000.0);
        assert_eq!(tasks.indexing(), None);
        let indexing = tasks.task(&id(INDEX_TASK_ID)).unwrap();
        assert!(indexing.done);
        assert_eq!(indexing.message, None);
        assert_eq!(tasks.next_deadline(), Some(1000.0 + DONE_LINGER_MS));
        tasks.expire(1000.0 + DONE_LINGER_MS);
        assert!(tasks.tasks().is_empty());
    }

    #[test]
    fn a_task_waiting_to_be_shown_is_forgotten_with_its_event_source() {
        let mut tasks = Tasks::default();
        tasks.on_task_event(event(1, "Loading", false), 0.0);
        tasks.on_task_event(TaskEvent { message: Some("Step 2".to_owned()), ..event(1, "Loading", false) }, 100.0);
        // The later report does not push the moment the task shows.
        assert_eq!(tasks.next_deadline(), Some(SHOW_DELAY_MS));
        let mut detached = tasks.clone();
        detached.clear_pending();
        detached.expire(SHOW_DELAY_MS);
        assert!(detached.tasks().is_empty());

        tasks.expire(SHOW_DELAY_MS);
        let loading = tasks.task(&TaskId::Number(1)).unwrap();
        assert_eq!(loading.message.as_deref(), Some("Step 2"));
        assert_eq!(loading.started_at, SHOW_DELAY_MS);
    }
}
