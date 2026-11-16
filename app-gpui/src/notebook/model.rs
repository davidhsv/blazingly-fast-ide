//! The notebook of one file as every tab that shows it shares it: its [`NotebookSession`] (edit
//! queue, execution, kernel state, failures) and what the tabs keep across a redraw (the selected
//! cell, the Markdown cells being edited, the interpreters found).
//!
//! The model talks to no backend. The calls its session wants to make go out as
//! [`NotebookModelEvent::Send`]; whoever owns the backend answers each with [`NotebookModel::answer`].
//! An operation started with [`NotebookModel::invoke`] reports its failure in the notebook's error
//! bar, as a toolbar button does; one started with [`NotebookModel::wait`] hands its outcome to the
//! caller (Save All, a close decision).

use crate::backend::Backend;
use crate::features_a::notebook::session::{Answer, Failure, NotebookSession, NotebookViewState, Request};
use crate::platform::storage::UiStorage;
use futures::channel::oneshot;
use gpui_kit::{Context, EventEmitter};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_host::notebook::NotebookOutputEvent;
use ide_notebook::{NotebookCell, NotebookEdit};
use std::collections::{HashMap, HashSet};

/// Where the interpreter chosen for a notebook is kept, followed by the notebook's path.
pub const PYTHON_KEY: &str = "openidea.notebook.python:";

/// An interpreter a notebook can run with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Environment {
    pub name: String,
    pub path: String,
}

/// An operation of the notebook's document or kernel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotebookOp {
    Load,
    Flush,
    Save,
    Undo,
    Redo,
    /// Runs these cells, or every code cell.
    Execute(Option<Vec<String>>),
    Interrupt,
    Restart,
    PrepareClose,
    Discard,
    Close,
}

/// What the model asks of its owner.
#[derive(Clone, Debug)]
pub enum NotebookModelEvent {
    /// Make these backend calls and answer each with [`NotebookModel::answer`].
    Send(Vec<Request>),
}

/// The outcome of an operation someone waits for.
pub type Outcome = oneshot::Receiver<Result<(), Failure>>;

enum Waiter {
    /// A failure goes to the error bar.
    Report,
    /// As [`Waiter::Report`]; edits and runs resume afterwards whatever the outcome (Reload from
    /// Disk ends its freeze that way).
    ReportAndResume,
    Caller(oneshot::Sender<Result<(), Failure>>),
}

pub struct NotebookModel {
    session: NotebookSession,
    waiters: HashMap<u64, Waiter>,
    environments: Vec<Environment>,
    /// Numbers backend failures, so a failure reported again keeps its first owner.
    failures: u64,
    /// The cell the toolbar acts on, kept across tabs.
    pub selected: Option<String>,
    /// The Markdown cells shown as source instead of their preview.
    pub markdown_editing: HashSet<String>,
}

impl EventEmitter<NotebookModelEvent> for NotebookModel {}

impl NotebookModel {
    /// The notebook at `path` (its canonical path), with the interpreter chosen for it earlier.
    pub fn new(path: &str, cx: &mut Context<Self>) -> Self {
        let key = format!("{PYTHON_KEY}{path}");
        let python = cx.default_global::<UiStorage>().get(&key).filter(|python| !python.is_empty()).map(str::to_owned);
        Self {
            session: NotebookSession::new(path, python),
            waiters: HashMap::new(),
            environments: Vec::new(),
            failures: 0,
            selected: None,
            markdown_editing: HashSet::new(),
        }
    }

    pub fn path(&self) -> &str {
        self.session.path()
    }

    pub fn state(&self) -> &NotebookViewState {
        self.session.state()
    }

    /// The cells as shown: typed source not committed yet over the acknowledged one.
    pub fn cells(&self) -> Vec<NotebookCell> {
        self.session.cells()
    }

    pub fn pending_sources(&self) -> Vec<(String, String)> {
        self.session.pending_sources()
    }

    pub fn is_dirty(&self) -> bool {
        self.session.is_dirty()
    }

    /// The interpreters found, in the order the backend lists them.
    pub fn environments(&self) -> &[Environment] {
        &self.environments
    }

    /// The backend document's id, once it is open.
    pub fn document_id(&self) -> Option<String> {
        self.session.state().snapshot.as_ref().map(|snapshot| snapshot.id.clone())
    }

    /// A backend error as the session takes it; an edit against an older version is out of sync.
    pub fn failure(&mut self, error: &IdeError) -> Failure {
        self.failures += 1;
        if error.kind == ErrorKind::OutOfSync {
            Failure::out_of_sync(self.failures, &error.message)
        } else {
            Failure::backend(self.failures, &error.message)
        }
    }

    /// A failure that is not the backend's own (the call could not be made).
    pub fn transport_failure(&mut self, message: &str) -> Failure {
        self.failures += 1;
        Failure::backend(self.failures, message)
    }

    fn begin(&mut self, op: NotebookOp) -> u64 {
        match op {
            NotebookOp::Load => self.session.load(None),
            NotebookOp::Flush => self.session.flush(),
            NotebookOp::Save => self.session.save(),
            NotebookOp::Undo => self.session.undo(),
            NotebookOp::Redo => self.session.redo(),
            NotebookOp::Execute(cells) => self.session.execute(cells, Backend::next_request_id()),
            NotebookOp::Interrupt => self.session.interrupt(),
            NotebookOp::Restart => self.session.restart(),
            NotebookOp::PrepareClose => self.session.prepare_close(),
            NotebookOp::Discard => self.session.discard(),
            NotebookOp::Close => self.session.close(),
        }
    }

    /// Starts `op`; a failure shows in the error bar.
    pub fn invoke(&mut self, op: NotebookOp, cx: &mut Context<Self>) {
        let id = self.begin(op);
        self.waiters.insert(id, Waiter::Report);
        self.pump(cx);
    }

    /// Drops every unsaved change and reloads the file, then lets edits and runs resume.
    pub fn discard_and_resume(&mut self, cx: &mut Context<Self>) {
        let id = self.begin(NotebookOp::Discard);
        self.waiters.insert(id, Waiter::ReportAndResume);
        self.pump(cx);
    }

    /// Starts `op` and hands its outcome to the caller.
    pub fn wait(&mut self, op: NotebookOp, cx: &mut Context<Self>) -> Outcome {
        let id = self.begin(op);
        let (sender, receiver) = oneshot::channel();
        self.waiters.insert(id, Waiter::Caller(sender));
        self.pump(cx);
        receiver
    }

    /// Loads the document already opened as `snapshot` (the tab was opened from it).
    pub fn load_opened(&mut self, snapshot: ide_notebook::NotebookSnapshot, cx: &mut Context<Self>) {
        let id = self.session.load(Some(snapshot));
        self.waiters.insert(id, Waiter::Report);
        self.pump(cx);
    }

    /// Loads the document unless it is loaded or loading.
    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        let state = self.session.state();
        if state.snapshot.is_none() && !state.loading {
            self.invoke(NotebookOp::Load, cx);
        }
    }

    pub fn edit(&mut self, edit: NotebookEdit, cx: &mut Context<Self>) {
        self.session.edit(edit);
        self.pump(cx);
    }

    /// The close did not happen: edits and runs resume.
    pub fn cancel_close(&mut self, cx: &mut Context<Self>) {
        self.session.cancel_close();
        self.pump(cx);
    }

    pub fn detect_environments(&mut self, cx: &mut Context<Self>) {
        self.session.detect_environments();
        self.pump(cx);
    }

    /// Chooses the interpreter and keeps the choice for the file; nothing while a batch runs or
    /// the notebook is frozen.
    pub fn select_python(&mut self, python: &str, cx: &mut Context<Self>) {
        if !self.session.select_python(python) {
            return;
        }
        let key = format!("{PYTHON_KEY}{}", self.session.path());
        cx.default_global::<UiStorage>().set(&key, python.to_owned());
        cx.notify();
    }

    /// A failure a caller saw, kept under the owner it was first reported by.
    pub fn report(&mut self, failure: &Failure, cx: &mut Context<Self>) {
        self.session.report(failure);
        cx.notify();
    }

    /// The interpreters the backend found, before the answer of the environments call.
    pub fn set_environments(&mut self, environments: Vec<Environment>) {
        self.environments = environments;
    }

    /// The answer of the call sent with `ticket`.
    pub fn answer(&mut self, ticket: u64, answer: Result<Answer, Failure>, cx: &mut Context<Self>) {
        self.session.answer(ticket, answer);
        self.pump(cx);
    }

    /// A kernel state event of the backend.
    pub fn kernel_event(&mut self, id: &str, path: &str, state: &str, message: Option<&str>, cx: &mut Context<Self>) {
        self.session.kernel_event(id, path, state, message);
        self.pump(cx);
    }

    /// Output events of the running batch, in the order they came.
    pub fn output_events(&mut self, events: &[NotebookOutputEvent], cx: &mut Context<Self>) {
        for event in events {
            self.session.output_event(event);
        }
        self.pump(cx);
    }

    /// Sends what the session wants sent and settles the operations that finished.
    fn pump(&mut self, cx: &mut Context<Self>) {
        loop {
            let requests = self.session.take_requests();
            let finished = self.session.take_finished();
            if requests.is_empty() && finished.is_empty() {
                break;
            }
            for done in finished {
                match self.waiters.remove(&done.op) {
                    Some(Waiter::Report) => {
                        if let Err(failure) = &done.result {
                            self.session.report(failure);
                        }
                    }
                    Some(Waiter::ReportAndResume) => {
                        if let Err(failure) = &done.result {
                            self.session.report(failure);
                        }
                        self.session.cancel_close();
                    }
                    Some(Waiter::Caller(sender)) => {
                        let _ = sender.send(done.result);
                    }
                    None => {}
                }
            }
            if !requests.is_empty() {
                cx.emit(NotebookModelEvent::Send(requests));
            }
        }
        // The cells of the snapshot may have changed: forget the state of cells that are gone.
        let live: HashSet<String> = self.session.cells().into_iter().map(|cell| cell.id).collect();
        self.markdown_editing.retain(|id| live.contains(id));
        if self.selected.as_ref().is_some_and(|id| !live.contains(id)) {
            self.selected = None;
        }
        cx.notify();
    }
}
