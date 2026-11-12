//! One notebook's editing and execution session, shared by every tab that shows the file.
//!
//! Edits go through a queue: each is sent with the version it was made against, one at a time,
//! and typing into a cell whose edit has not been sent yet replaces that edit. A source edit the
//! backend rejects as out of sync is rebased once on the current snapshot (cells have stable
//! ids). Typed source stays visible over older acknowledgements until it is committed.
//!
//! Nothing here talks to the backend: calls go out as [`Request`]s (taken with
//! [`NotebookSession::take_requests`]) and come back through [`NotebookSession::answer`] with
//! their ticket; operations a caller waits for (save, restart…) are numbered and their outcome
//! is taken with [`NotebookSession::take_finished`]. An answer for an older execution is dropped.

use super::outputs::OutputReducer;
use ide_core::RequestId;
use ide_host::notebook::NotebookOutputEvent;
use ide_notebook::{NotebookCell, NotebookEdit, NotebookSnapshot};
use std::collections::{HashSet, VecDeque};

/// What identifies a failure, so that reporting it again (by a caller that waited on it) keeps its
/// first owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    /// A backend failure, numbered by the caller.
    Backend(u64),
    /// A message the kernel sent.
    Text(String),
    /// A failure the session raised itself.
    Local(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub cause: Cause,
    pub message: String,
    /// The backend rejected an edit made against an older version.
    pub out_of_sync: bool,
}

impl Failure {
    pub fn backend(id: u64, message: &str) -> Self {
        Self { cause: Cause::Backend(id), message: message.to_owned(), out_of_sync: false }
    }

    pub fn out_of_sync(id: u64, message: &str) -> Self {
        Self { out_of_sync: true, ..Self::backend(id, message) }
    }
}

/// A backend call.
#[derive(Clone, Debug)]
pub enum Call {
    Open { path: String },
    Get { id: String },
    Edit { id: String, version: u32, edit: NotebookEdit },
    Undo { id: String },
    Redo { id: String },
    Save { id: String },
    Reload { id: String },
    Close { id: String },
    Interrupt { id: String },
    Cancel { request: RequestId },
    Execute { id: String, cells: Vec<String>, python: Option<String>, request: RequestId },
    Restart { id: String },
    Environments,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub ticket: u64,
    pub call: Call,
}

/// A backend answer.
#[derive(Clone, Debug)]
pub enum Answer {
    Snapshot(Box<NotebookSnapshot>),
    Done,
    /// Environments: the effective interpreter.
    Python(Option<String>),
}

/// An operation that finished.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub op: u64,
    pub result: Result<(), Failure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Owner {
    Load,
    Mutation,
    Save,
    Execution,
    Kernel,
    Environment,
    Reload,
    Other,
}

/// What the view shows.
#[derive(Clone, Debug, Default)]
pub struct NotebookViewState {
    pub snapshot: Option<NotebookSnapshot>,
    pub loading: bool,
    pub error: Option<String>,
    pub running: bool,
    pub frozen: bool,
    pub kernel: String,
    pub python: Option<String>,
    pub pending: usize,
}

#[derive(Clone, Debug)]
enum Intent {
    Edit(NotebookEdit),
    Undo,
    Redo,
}

#[derive(Clone, Debug)]
struct Pending {
    intent: Intent,
    committed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpStep {
    Sent,
    /// Fetching the current snapshot to rebase on.
    Rebasing,
    Rebased,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpKind {
    Load,
    Flush,
    Save,
    History,
    Restart,
    PrepareClose,
    Discard,
    Close,
    Interrupt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Start,
    /// Restart the queue and wait until it is empty (or fails).
    Flush,
    /// Wait for a running queue to stop, without restarting it.
    Settle,
    /// Stopping a running execution: cancel, interrupt, then wait for it.
    Cancel,
    Interrupt,
    AwaitExecution,
    /// The operation's own backend call is out.
    Call,
}

#[derive(Clone, Debug)]
struct Op {
    id: u64,
    kind: OpKind,
    phase: Phase,
    ticket: Option<u64>,
    /// A failure to report once the operation can finish (a drain that failed).
    failure: Option<Failure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExecPhase {
    Flush,
    Execute,
    Refresh,
}

#[derive(Clone, Debug)]
struct Execution {
    op: u64,
    generation: u64,
    phase: ExecPhase,
    cells: Option<Vec<String>>,
    request: RequestId,
    ticket: Option<u64>,
    failure: Option<Failure>,
}

const BUSY: &str = "Notebook is busy or closing.";
const UNCOMMITTED: &str = "Notebook edits have not been committed. Retry edits or discard explicitly.";

/// The session of one notebook file.
#[derive(Debug)]
pub struct NotebookSession {
    path: String,
    state: NotebookViewState,
    errors: Vec<(Owner, Cause, String)>,
    pending: VecDeque<Pending>,
    pump: Option<(u64, PumpStep)>,
    pump_failure: Option<Failure>,
    blocked: bool,
    disposed: bool,
    listening: bool,
    authoritative_version: Option<u32>,
    generation: u64,
    cancelled: bool,
    execution: Option<Execution>,
    refresh: Option<u64>,
    refresh_again: bool,
    refresh_failure: Option<Failure>,
    environments: Option<u64>,
    ops: VecDeque<Op>,
    output: OutputReducer,
    outbox: Vec<Request>,
    finished: Vec<Finished>,
    tickets: u64,
    locals: u64,
    /// Cells with editor state, dropped when the cell goes away.
    pub editors: HashSet<String>,
}

impl NotebookSession {
    /// A session for `path`; `python` is the interpreter chosen for it earlier.
    pub fn new(path: &str, python: Option<String>) -> Self {
        Self {
            path: path.to_owned(),
            state: NotebookViewState { kernel: "idle".to_owned(), python, ..NotebookViewState::default() },
            errors: Vec::new(),
            pending: VecDeque::new(),
            pump: None,
            pump_failure: None,
            blocked: false,
            disposed: false,
            listening: false,
            authoritative_version: None,
            generation: 0,
            cancelled: false,
            execution: None,
            refresh: None,
            refresh_again: false,
            refresh_failure: None,
            environments: None,
            ops: VecDeque::new(),
            output: OutputReducer::default(),
            outbox: Vec::new(),
            finished: Vec::new(),
            tickets: 0,
            locals: 0,
            editors: HashSet::new(),
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn state(&self) -> &NotebookViewState {
        &self.state
    }

    /// The calls to send now.
    pub fn take_requests(&mut self) -> Vec<Request> {
        std::mem::take(&mut self.outbox)
    }

    /// The operations that finished since the last call.
    pub fn take_finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.finished)
    }

    pub fn is_dirty(&self) -> bool {
        !self.pending.is_empty() || self.state.snapshot.as_ref().is_some_and(|s| s.dirty)
    }

    fn send(&mut self, call: Call) -> u64 {
        self.tickets += 1;
        self.outbox.push(Request { ticket: self.tickets, call });
        self.tickets
    }

    fn local(&mut self, message: &str) -> Failure {
        self.locals += 1;
        Failure { cause: Cause::Local(self.locals), message: message.to_owned(), out_of_sync: false }
    }

    fn publish(&mut self) {
        self.state.error = self.errors.last().map(|(_, _, m)| m.clone());
        self.state.pending = self.pending.len();
    }

    /// A failure a caller saw (a UI promise catch): kept under the owner it was first reported by.
    pub fn report(&mut self, failure: &Failure) {
        self.report_as(failure, Owner::Other);
    }

    fn report_as(&mut self, failure: &Failure, owner: Owner) {
        if self.errors.iter().any(|(_, cause, _)| *cause == failure.cause) {
            return;
        }
        self.errors.retain(|(o, _, _)| *o != owner);
        self.errors.push((owner, failure.cause.clone(), failure.message.clone()));
        self.publish();
    }

    fn recover(&mut self, owners: &[Owner]) {
        self.errors.retain(|(o, _, _)| !owners.contains(o));
        self.publish();
    }

    fn snapshot_id(&self) -> Option<String> {
        self.state.snapshot.as_ref().map(|s| s.id.clone())
    }

    fn accept(&mut self, mut snapshot: NotebookSnapshot) {
        if self.disposed {
            return;
        }
        if let Some(current) = &self.state.snapshot
            && (current.id != snapshot.id || self.authoritative_version.is_some_and(|v| snapshot.version < v))
        {
            return;
        }
        self.authoritative_version = Some(snapshot.version);
        if let Some(current) = self.state.snapshot.as_ref().filter(|c| c.version > snapshot.version) {
            // Only output reductions can be newer than the latest full snapshot: take the
            // acknowledged fields and order, keep the newer outputs.
            snapshot.version = current.version;
            snapshot.dirty = current.dirty;
            for cell in snapshot.cells.iter_mut().filter(|c| c.cell_type == "code") {
                if let Some(newer) = current.cells.iter().find(|c| c.id == cell.id) {
                    cell.outputs.clone_from(&newer.outputs);
                    cell.execution_count = newer.execution_count;
                }
            }
        }
        let live: HashSet<&str> = snapshot.cells.iter().map(|c| c.id.as_str()).collect();
        self.editors.retain(|id| live.contains(id.as_str()));
        self.state.snapshot = Some(snapshot);
        self.publish();
    }

    // --- operations ------------------------------------------------------------------------------

    fn start_op(&mut self, kind: OpKind) -> u64 {
        self.tickets += 1;
        let id = self.tickets;
        self.ops.push_back(Op { id, kind, phase: Phase::Start, ticket: None, failure: None });
        self.advance();
        id
    }

    fn fail_now(&mut self, message: &str) -> u64 {
        self.tickets += 1;
        let id = self.tickets;
        let failure = self.local(message);
        self.finished.push(Finished { op: id, result: Err(failure) });
        id
    }

    /// Loads the document (`opened` when it is already open).
    pub fn load(&mut self, opened: Option<NotebookSnapshot>) -> u64 {
        if self.disposed {
            return self.fail_now("Notebook is closed");
        }
        if let Some(snapshot) = opened {
            self.loaded(snapshot);
            return self.done_now();
        }
        if self.state.snapshot.is_some() {
            return self.done_now();
        }
        self.state.loading = true;
        self.start_op(OpKind::Load)
    }

    /// An operation that is already done.
    fn done_now(&mut self) -> u64 {
        self.tickets += 1;
        self.finished.push(Finished { op: self.tickets, result: Ok(()) });
        self.tickets
    }

    fn loaded(&mut self, snapshot: NotebookSnapshot) {
        self.accept(snapshot);
        self.recover(&[Owner::Load]);
        self.listening = true;
        self.detect_environments();
    }

    /// Looks for the Python environments a notebook can run with.
    pub fn detect_environments(&mut self) {
        if self.environments.is_none() && !self.disposed {
            self.environments = Some(self.send(Call::Environments));
        }
    }

    /// A kernel state event of the backend.
    pub fn kernel_event(&mut self, id: &str, path: &str, state: &str, message: Option<&str>) {
        let current = self.state.snapshot.as_ref();
        let other = current.is_none_or(|s| s.id != id || s.path.to_string_lossy() != path);
        if self.disposed || !self.listening || other {
            return;
        }
        self.state.kernel = state.to_owned();
        self.publish();
        if let Some(message) = message {
            let cause = Cause::Text(message.to_owned());
            let failure = Failure { cause, message: message.to_owned(), out_of_sync: false };
            self.report_as(&failure, Owner::Kernel);
        }
    }

    /// Chooses the interpreter; `false` while a batch runs or the session is frozen.
    pub fn select_python(&mut self, python: &str) -> bool {
        if self.state.running || self.state.frozen {
            return false;
        }
        self.state.python = Some(python.to_owned());
        true
    }

    /// Source edits not committed yet, the latest per cell.
    pub fn pending_sources(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for p in &self.pending {
            if let Intent::Edit(NotebookEdit::SetSource { cell, source }) = &p.intent {
                match out.iter_mut().find(|(c, _)| c == cell) {
                    Some(entry) => entry.1.clone_from(source),
                    None => out.push((cell.clone(), source.clone())),
                }
            }
        }
        out
    }

    /// The cells as shown: pending source over the acknowledged one.
    pub fn cells(&self) -> Vec<NotebookCell> {
        let Some(snapshot) = &self.state.snapshot else { return Vec::new() };
        let sources = self.pending_sources();
        snapshot
            .cells
            .iter()
            .map(|cell| {
                let mut cell = cell.clone();
                if let Some((_, source)) = sources.iter().find(|(id, _)| *id == cell.id) {
                    cell.source.clone_from(source);
                }
                cell
            })
            .collect()
    }

    /// Queues an edit.
    pub fn edit(&mut self, edit: NotebookEdit) {
        if self.disposed || self.state.frozen || self.state.snapshot.is_none() {
            return;
        }
        let source_edit = matches!(edit, NotebookEdit::SetSource { .. });
        if self.state.running && !source_edit {
            let failure = self.local("Stop execution before changing notebook structure.");
            self.report_as(&failure, Owner::Mutation);
            return;
        }
        let in_flight = self.pump.is_some() && self.pending.len() == 1;
        let coalesce = match (&edit, self.pending.back()) {
            (NotebookEdit::SetSource { cell, .. }, Some(last)) => {
                !last.committed
                    && !in_flight
                    && matches!(&last.intent, Intent::Edit(NotebookEdit::SetSource { cell: c, .. }) if c == cell)
            }
            _ => false,
        };
        match self.pending.back_mut() {
            Some(last) if coalesce => last.intent = Intent::Edit(edit),
            _ => self.pending.push_back(Pending { intent: Intent::Edit(edit), committed: false }),
        }
        self.publish();
        if !self.blocked {
            self.advance();
        }
    }

    /// Sends pending edits until every one is committed.
    pub fn flush(&mut self) -> u64 {
        self.start_op(OpKind::Flush)
    }

    pub fn save(&mut self) -> u64 {
        self.start_op(OpKind::Save)
    }

    fn history(&mut self, intent: Intent) -> u64 {
        if self.state.running || self.state.frozen {
            return self.fail_now("Stop execution before undo or redo.");
        }
        if self.disposed || self.state.snapshot.is_none() {
            return self.fail_now("Notebook is busy or closing.");
        }
        // History is a queue barrier: later typing commits after it.
        self.pending.push_back(Pending { intent, committed: false });
        self.publish();
        self.start_op(OpKind::History)
    }

    pub fn undo(&mut self) -> u64 {
        self.history(Intent::Undo)
    }

    pub fn redo(&mut self) -> u64 {
        self.history(Intent::Redo)
    }

    /// Runs `cells` (every code cell when `None`); `request` identifies the run for cancellation.
    pub fn execute(&mut self, cells: Option<Vec<String>>, request: RequestId) -> u64 {
        if self.state.running || self.state.frozen || self.disposed {
            return self.fail_now(BUSY);
        }
        self.state.running = true;
        self.cancelled = false;
        self.generation += 1;
        self.output.reset();
        self.tickets += 1;
        let op = self.tickets;
        self.execution = Some(Execution {
            op,
            generation: self.generation,
            phase: ExecPhase::Flush,
            cells,
            request,
            ticket: None,
            failure: None,
        });
        self.blocked = false;
        self.pump_failure = None;
        self.advance();
        op
    }

    /// Stops the running batch.
    pub fn interrupt(&mut self) -> u64 {
        self.start_op(OpKind::Interrupt)
    }

    pub fn restart(&mut self) -> u64 {
        if self.state.frozen {
            return self.fail_now("Notebook is closing.");
        }
        self.state.frozen = true;
        self.generation += 1;
        self.start_op(OpKind::Restart)
    }

    /// The tab is about to close: no more edits, commit the pending ones, stop the batch.
    pub fn prepare_close(&mut self) -> u64 {
        self.state.frozen = true;
        self.start_op(OpKind::PrepareClose)
    }

    pub fn cancel_close(&mut self) {
        if !self.disposed {
            self.state.frozen = false;
        }
    }

    /// Drops every uncommitted change and reloads the file.
    pub fn discard(&mut self) -> u64 {
        self.state.frozen = true;
        self.generation += 1;
        self.start_op(OpKind::Discard)
    }

    pub fn close(&mut self) -> u64 {
        self.state.frozen = true;
        self.generation += 1;
        self.start_op(OpKind::Close)
    }

    /// An output event of the running batch.
    pub fn output_event(&mut self, event: &NotebookOutputEvent) {
        let Some(snapshot) = self.state.snapshot.as_ref() else { return };
        let current = self.execution.as_ref().is_some_and(|e| e.generation == self.generation);
        if self.disposed || !current || event.id != snapshot.id {
            return;
        }
        let reduction = self.output.reduce(snapshot, event);
        if let Some(next) = reduction.snapshot {
            self.state.snapshot = Some(next);
            self.publish();
        }
        if reduction.refresh {
            self.start_refresh();
        }
    }

    fn start_refresh(&mut self) {
        self.refresh_again = true;
        if self.refresh.is_none() {
            self.next_refresh();
        }
    }

    fn next_refresh(&mut self) {
        match self.snapshot_id().filter(|_| self.refresh_again && !self.disposed) {
            Some(id) => {
                self.refresh_again = false;
                self.refresh = Some(self.send(Call::Get { id }));
            }
            None => self.refresh = None,
        }
    }

    // --- progress --------------------------------------------------------------------------------

    fn pump_idle(&self) -> bool {
        self.pump.is_none()
    }

    fn start_pump(&mut self) {
        if self.pump.is_some() || self.blocked || self.disposed {
            return;
        }
        let (Some(front), Some(snapshot)) = (self.pending.front_mut(), self.state.snapshot.as_ref()) else { return };
        if front.committed {
            self.pending.pop_front();
            return self.start_pump();
        }
        let id = snapshot.id.clone();
        let call = match &front.intent {
            Intent::Undo => Call::Undo { id },
            Intent::Redo => Call::Redo { id },
            Intent::Edit(edit) => Call::Edit { id, version: snapshot.version, edit: edit.clone() },
        };
        let ticket = self.send(call);
        self.pump = Some((ticket, PumpStep::Sent));
    }

    fn pump_answer(&mut self, step: PumpStep, answer: Result<Answer, Failure>) {
        self.pump = None;
        let history = self.pending.front().is_none_or(|p| !matches!(p.intent, Intent::Edit(_)));
        match (step, answer) {
            (PumpStep::Rebasing, Ok(Answer::Snapshot(snapshot))) => {
                self.accept(*snapshot);
                let (Some(front), Some(current)) = (self.pending.front(), self.state.snapshot.as_ref()) else { return };
                if let Intent::Edit(edit) = &front.intent {
                    let call = Call::Edit { id: current.id.clone(), version: current.version, edit: edit.clone() };
                    let ticket = self.send(call);
                    self.pump = Some((ticket, PumpStep::Rebased));
                }
            }
            (_, Ok(answer)) => {
                if let Some(front) = self.pending.front_mut() {
                    front.committed = true;
                }
                if let Answer::Snapshot(snapshot) = answer {
                    self.accept(*snapshot);
                }
                self.pending.pop_front();
                self.recover(&[Owner::Mutation]);
                self.start_pump();
            }
            (PumpStep::Sent, Err(failure)) if failure.out_of_sync && !history => {
                // Semantic stable-id rebase, once; a missing target stays pending with the error.
                if let Some(id) = self.snapshot_id() {
                    let ticket = self.send(Call::Get { id });
                    self.pump = Some((ticket, PumpStep::Rebasing));
                }
            }
            (_, Err(failure)) => {
                self.blocked = true;
                self.report_as(&failure, Owner::Mutation);
                self.pump_failure = Some(failure);
            }
        }
    }

    /// The result of waiting for the queue, when it has stopped.
    fn queue_settled(&self) -> Option<Result<(), Failure>> {
        if !self.pump_idle() {
            return None;
        }
        Some(match &self.pump_failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        })
    }

    fn finish_op(&mut self, result: Result<(), Failure>) {
        let Some(op) = self.ops.pop_front() else { return };
        let owner = match op.kind {
            OpKind::Load => Some(Owner::Load),
            OpKind::Save => Some(Owner::Save),
            OpKind::Restart => Some(Owner::Kernel),
            OpKind::Discard => Some(Owner::Reload),
            _ => None,
        };
        match (&result, owner) {
            (Ok(()), Some(Owner::Save)) => self.recover(&[Owner::Save]),
            (Err(failure), Some(owner)) => self.report_as(failure, owner),
            _ => {}
        }
        match op.kind {
            OpKind::Load => self.state.loading = false,
            OpKind::Restart => self.state.frozen = false,
            OpKind::Close => {
                self.disposed = true;
                self.listening = false;
                self.editors.clear();
            }
            _ => {}
        }
        self.finished.push(Finished { op: op.id, result });
    }

    /// Moves the running operation, the execution and the queue forward as far as they can go.
    fn advance(&mut self) {
        loop {
            self.start_pump();
            let progressed = self.advance_execution() | self.advance_op();
            if !progressed {
                break;
            }
        }
    }

    fn advance_execution(&mut self) -> bool {
        let Some(exec) = self.execution.clone() else { return false };
        let current = exec.generation == self.generation;
        match exec.phase {
            ExecPhase::Flush => {
                let Some(settled) = self.queue_settled() else { return false };
                if let Err(failure) = settled {
                    return self.finish_execution(Some(failure));
                }
                let ids: Vec<String> = match (&exec.cells, &self.state.snapshot) {
                    (Some(cells), _) => cells.clone(),
                    (None, Some(s)) => s.cells.iter().filter(|c| c.cell_type == "code").map(|c| c.id.clone()).collect(),
                    (None, None) => Vec::new(),
                };
                let Some(id) = self.snapshot_id().filter(|_| !self.state.frozen && !self.cancelled && current) else {
                    return self.finish_execution(None);
                };
                if ids.is_empty() {
                    return self.finish_execution(None);
                }
                let python = self.state.python.clone();
                let ticket = self.send(Call::Execute { id, cells: ids, python, request: exec.request });
                self.set_execution(ExecPhase::Execute, Some(ticket), None);
                true
            }
            ExecPhase::Execute => false,
            ExecPhase::Refresh => {
                if self.refresh.is_some() {
                    return false;
                }
                let failure = exec.failure.or_else(|| self.refresh_failure.take());
                self.finish_execution(failure)
            }
        }
    }

    fn set_execution(&mut self, phase: ExecPhase, ticket: Option<u64>, failure: Option<Failure>) {
        if let Some(exec) = &mut self.execution {
            exec.phase = phase;
            exec.ticket = ticket;
            if failure.is_some() {
                exec.failure = failure;
            }
        }
    }

    fn execution_answer(&mut self, answer: Result<Answer, Failure>) {
        let Some(exec) = self.execution.clone() else { return };
        let current = exec.generation == self.generation;
        let failure = match answer {
            Ok(Answer::Snapshot(snapshot)) if current => {
                self.accept(*snapshot);
                None
            }
            Ok(_) => None,
            Err(failure) => Some(failure),
        };
        if current && !self.disposed {
            self.refresh_failure = None;
            self.start_refresh();
            self.set_execution(ExecPhase::Refresh, None, failure);
        } else {
            self.finish_execution(failure);
        }
    }

    fn finish_execution(&mut self, failure: Option<Failure>) -> bool {
        let Some(exec) = self.execution.take() else { return false };
        let result = match failure {
            Some(failure) => {
                self.report_as(&failure, Owner::Execution);
                Err(failure)
            }
            None => {
                if exec.generation == self.generation && !self.cancelled {
                    self.recover(&[Owner::Execution, Owner::Kernel]);
                }
                Ok(())
            }
        };
        self.state.running = false;
        self.publish();
        self.finished.push(Finished { op: exec.op, result });
        true
    }

    fn set_op(&mut self, phase: Phase, ticket: Option<u64>) {
        if let Some(op) = self.ops.front_mut() {
            op.phase = phase;
            op.ticket = ticket;
        }
    }

    /// Phases of draining: cancel and interrupt a running batch, then wait for it.
    fn start_drain(&mut self) -> Phase {
        if self.execution.is_none() {
            return Phase::AwaitExecution;
        }
        self.cancelled = true;
        match self.execution.as_ref().map(|e| e.request) {
            Some(request) => {
                let ticket = self.send(Call::Cancel { request });
                self.set_op(Phase::Cancel, Some(ticket));
                Phase::Cancel
            }
            None => Phase::Interrupt,
        }
    }

    fn send_interrupt(&mut self) -> bool {
        match self.snapshot_id() {
            Some(id) => {
                let ticket = self.send(Call::Interrupt { id });
                self.set_op(Phase::Interrupt, Some(ticket));
            }
            None => self.set_op(Phase::AwaitExecution, None),
        }
        true
    }

    fn begin_flush(&mut self) {
        self.blocked = false;
        self.pump_failure = None;
        self.set_op(Phase::Flush, None);
        self.start_pump();
    }

    fn advance_op(&mut self) -> bool {
        let Some(op) = self.ops.front().cloned() else { return false };
        match (op.kind, op.phase) {
            (_, Phase::Call | Phase::Cancel | Phase::Interrupt) => false,
            (OpKind::Load, Phase::Start) => {
                let ticket = self.send(Call::Open { path: self.path.clone() });
                self.set_op(Phase::Call, Some(ticket));
                true
            }
            (OpKind::Flush | OpKind::Save | OpKind::History | OpKind::PrepareClose, Phase::Start) => {
                self.begin_flush();
                true
            }
            (OpKind::Restart | OpKind::Discard | OpKind::Close | OpKind::Interrupt, Phase::Start) => {
                match self.start_drain() {
                    Phase::Interrupt => self.send_interrupt(),
                    Phase::AwaitExecution => {
                        self.set_op(Phase::AwaitExecution, None);
                        true
                    }
                    _ => true,
                }
            }
            (kind, Phase::AwaitExecution) => {
                if self.execution.is_some() && kind != OpKind::Interrupt {
                    return false;
                }
                if let Some(failure) = op.failure.filter(|_| kind != OpKind::Close) {
                    self.finish_op(Err(failure));
                    return true;
                }
                match kind {
                    OpKind::Interrupt => self.finish_op(Ok(())),
                    OpKind::Restart => self.begin_flush(),
                    OpKind::PrepareClose => self.finish_op(Ok(())),
                    _ => self.set_op(Phase::Settle, None),
                }
                true
            }
            (kind, Phase::Flush) => {
                let Some(settled) = self.queue_settled() else { return false };
                match settled {
                    Err(failure) => self.finish_op(Err(failure)),
                    Ok(()) if kind == OpKind::History => self.finish_op(Ok(())),
                    Ok(()) if !self.pending.is_empty() && kind != OpKind::PrepareClose => {
                        let failure = self.local(UNCOMMITTED);
                        self.finish_op(Err(failure));
                    }
                    Ok(()) => match kind {
                        OpKind::Save => self.call(|id| Call::Save { id }),
                        OpKind::Restart => {
                            self.output.reset();
                            self.call(|id| Call::Restart { id });
                        }
                        OpKind::PrepareClose => match self.start_drain() {
                            Phase::Interrupt => {
                                self.send_interrupt();
                            }
                            Phase::AwaitExecution => self.finish_op(Ok(())),
                            _ => {}
                        },
                        _ => self.finish_op(Ok(())),
                    },
                }
                true
            }
            (kind, Phase::Settle) => {
                if !self.pump_idle() {
                    return false;
                }
                match kind {
                    OpKind::Discard => self.call(|id| Call::Reload { id }),
                    OpKind::Close => match self.snapshot_id() {
                        Some(id) => {
                            let ticket = self.send(Call::Close { id });
                            self.set_op(Phase::Call, Some(ticket));
                        }
                        None => self.finish_op(Ok(())),
                    },
                    _ => self.finish_op(Ok(())),
                }
                true
            }
        }
    }

    /// Sends the operation's own call on the current document.
    fn call(&mut self, make: impl FnOnce(String) -> Call) {
        match self.snapshot_id() {
            Some(id) => {
                let ticket = self.send(make(id));
                self.set_op(Phase::Call, Some(ticket));
            }
            None => {
                let failure = self.local("Notebook is closed");
                self.finish_op(Err(failure));
            }
        }
    }

    fn op_answer(&mut self, op: Op, answer: Result<Answer, Failure>) {
        match op.phase {
            Phase::Cancel => {
                if let Err(failure) = answer
                    && let Some(front) = self.ops.front_mut()
                {
                    front.failure.get_or_insert(failure);
                }
                self.send_interrupt();
            }
            Phase::Interrupt => {
                if let Some(front) = self.ops.front_mut() {
                    if let Err(failure) = answer {
                        front.failure.get_or_insert(failure);
                    }
                    front.phase = Phase::AwaitExecution;
                    front.ticket = None;
                }
            }
            _ => match (op.kind, answer) {
                (OpKind::Load, Ok(Answer::Snapshot(snapshot))) => {
                    let snapshot = *snapshot;
                    if self.disposed {
                        self.send(Call::Close { id: snapshot.id.clone() });
                    } else {
                        self.loaded(snapshot);
                    }
                    self.finish_op(Ok(()));
                }
                (OpKind::Restart, Ok(answer)) => {
                    if let Answer::Snapshot(snapshot) = answer {
                        self.accept(*snapshot);
                    }
                    self.state.kernel = "idle".to_owned();
                    self.recover(&[Owner::Execution, Owner::Kernel]);
                    self.finish_op(Ok(()));
                }
                (OpKind::Discard, Ok(answer)) => {
                    self.pending.clear();
                    self.blocked = false;
                    self.pump_failure = None;
                    self.editors.clear();
                    self.output.reset();
                    if let Answer::Snapshot(snapshot) = answer {
                        self.accept(*snapshot);
                    }
                    self.recover(&[Owner::Mutation, Owner::Save, Owner::Reload]);
                    self.finish_op(Ok(()));
                }
                (_, Ok(answer)) => {
                    if let Answer::Snapshot(snapshot) = answer {
                        self.accept(*snapshot);
                    }
                    self.finish_op(Ok(()));
                }
                (_, Err(failure)) => self.finish_op(Err(failure)),
            },
        }
    }

    /// The answer to the call sent with `ticket`; an answer nothing waits for is dropped.
    pub fn answer(&mut self, ticket: u64, answer: Result<Answer, Failure>) {
        if let Some((pump_ticket, step)) = self.pump
            && pump_ticket == ticket
        {
            self.pump_answer(step, answer);
        } else if self.execution.as_ref().is_some_and(|e| e.ticket == Some(ticket)) {
            self.execution_answer(answer);
        } else if self.refresh == Some(ticket) {
            match answer {
                Ok(Answer::Snapshot(snapshot)) => self.accept(*snapshot),
                Ok(_) => {}
                Err(failure) => {
                    self.refresh_again = false;
                    if self.execution.is_some() {
                        self.refresh_failure = Some(failure);
                    } else {
                        self.report_as(&failure, Owner::Execution);
                    }
                }
            }
            self.next_refresh();
        } else if self.environments == Some(ticket) {
            self.environments = None;
            if !self.disposed {
                match answer {
                    Ok(Answer::Python(effective)) => {
                        if self.state.python.is_none() {
                            self.state.python = effective;
                        }
                        self.recover(&[Owner::Environment]);
                    }
                    Ok(_) => self.recover(&[Owner::Environment]),
                    Err(failure) => self.report_as(&failure, Owner::Environment),
                }
            }
        } else if let Some(op) = self.ops.front().filter(|op| op.ticket == Some(ticket)).cloned() {
            self.op_answer(op, answer);
        }
        self.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::super::outputs::fixtures::notebook;
    use super::*;
    use std::collections::HashMap;

    fn snapshot() -> NotebookSnapshot {
        notebook("original", Vec::new(), None)
    }

    fn source(text: &str) -> NotebookEdit {
        NotebookEdit::SetSource { cell: "cell-a".to_owned(), source: text.to_owned() }
    }

    /// A backend over one document, answering like the real one for source edits.
    struct Backend {
        doc: NotebookSnapshot,
        failures: u64,
        edit: Option<Failure>,
        save: Option<Failure>,
        reload: Option<Failure>,
        restart: Option<Failure>,
        execute: Option<Failure>,
        environments: Option<Failure>,
        close: Option<Failure>,
        interrupt: Option<Failure>,
        /// Calls held back until released, by name.
        hold: Vec<&'static str>,
        held: Vec<Request>,
        closed: Vec<String>,
        outcomes: HashMap<u64, Result<(), Failure>>,
    }

    fn name(call: &Call) -> &'static str {
        match call {
            Call::Open { .. } => "open",
            Call::Get { .. } => "get",
            Call::Edit { .. } => "edit",
            Call::Undo { .. } => "undo",
            Call::Redo { .. } => "redo",
            Call::Save { .. } => "save",
            Call::Reload { .. } => "reload",
            Call::Close { .. } => "close",
            Call::Interrupt { .. } => "interrupt",
            Call::Cancel { .. } => "cancel",
            Call::Execute { .. } => "execute",
            Call::Restart { .. } => "restart",
            Call::Environments => "environments",
        }
    }

    impl Backend {
        fn new() -> Self {
            Self {
                doc: snapshot(),
                failures: 0,
                edit: None,
                save: None,
                reload: None,
                restart: None,
                execute: None,
                environments: None,
                close: None,
                interrupt: None,
                hold: Vec::new(),
                held: Vec::new(),
                closed: Vec::new(),
                outcomes: HashMap::new(),
            }
        }

        fn failure(&mut self, message: &str) -> Failure {
            self.failures += 1;
            Failure::backend(self.failures, message)
        }

        fn doc(&self) -> Result<Answer, Failure> {
            Ok(Answer::Snapshot(Box::new(self.doc.clone())))
        }

        fn respond(&mut self, call: &Call) -> Result<Answer, Failure> {
            let fail = |f: &Option<Failure>| f.clone().map_or(Ok(()), Err);
            match call {
                Call::Open { .. } | Call::Get { .. } => self.doc(),
                Call::Edit { version, edit, .. } => {
                    fail(&self.edit)?;
                    if *version != self.doc.version {
                        self.failures += 1;
                        return Err(Failure::out_of_sync(self.failures, "The document changed"));
                    }
                    let NotebookEdit::SetSource { cell, source } = edit else { panic!("source edits only") };
                    self.doc.version += 1;
                    self.doc.dirty = true;
                    self.doc.can_undo = true;
                    for c in self.doc.cells.iter_mut().filter(|c| c.id == *cell) {
                        c.source.clone_from(source);
                    }
                    self.doc()
                }
                Call::Undo { .. } => {
                    self.doc.version += 1;
                    self.doc.can_redo = true;
                    for c in &mut self.doc.cells {
                        c.source = "original".to_owned();
                    }
                    self.doc()
                }
                Call::Save { .. } => {
                    fail(&self.save)?;
                    self.doc.dirty = false;
                    self.doc()
                }
                Call::Reload { .. } => {
                    fail(&self.reload)?;
                    self.doc = snapshot();
                    self.doc()
                }
                Call::Restart { .. } => {
                    fail(&self.restart)?;
                    self.doc()
                }
                Call::Execute { .. } => {
                    fail(&self.execute)?;
                    self.doc()
                }
                Call::Environments => {
                    fail(&self.environments)?;
                    Ok(Answer::Python(None))
                }
                Call::Close { id } => {
                    self.closed.push(id.clone());
                    fail(&self.close).map(|()| Answer::Done)
                }
                Call::Interrupt { .. } => fail(&self.interrupt).map(|()| Answer::Done),
                Call::Redo { .. } | Call::Cancel { .. } => Ok(Answer::Done),
            }
        }

        /// Answers every call the session sends, except held ones, until it is idle.
        fn run(&mut self, s: &mut NotebookSession) {
            loop {
                let requests = s.take_requests();
                for f in s.take_finished() {
                    self.outcomes.insert(f.op, f.result);
                }
                if requests.is_empty() {
                    break;
                }
                for request in requests {
                    if let Some(at) = self.hold.iter().position(|h| *h == name(&request.call)) {
                        self.hold.remove(at);
                        self.held.push(request);
                        continue;
                    }
                    let answer = self.respond(&request.call);
                    s.answer(request.ticket, answer);
                }
            }
        }

        /// Answers the held calls (with `answer` when given), then runs.
        fn release(&mut self, s: &mut NotebookSession, answer: Option<Result<Answer, Failure>>) {
            for request in std::mem::take(&mut self.held) {
                let answer = answer.clone().unwrap_or_else(|| self.respond(&request.call));
                s.answer(request.ticket, answer);
            }
            self.run(s);
        }

        fn outcome(&self, op: u64) -> Option<Result<(), Failure>> {
            self.outcomes.get(&op).cloned()
        }

        fn ok(&mut self, s: &mut NotebookSession, op: u64) {
            self.run(s);
            assert_eq!(self.outcome(op), Some(Ok(())));
        }

        fn fails(&mut self, s: &mut NotebookSession, op: u64) -> Failure {
            self.run(s);
            match self.outcome(op) {
                Some(Err(failure)) => failure,
                other => panic!("expected a failure, got {other:?}"),
            }
        }
    }

    fn loaded(m: &mut Backend) -> NotebookSession {
        let mut s = NotebookSession::new("/p/a.ipynb", None);
        let op = s.load(None);
        m.ok(&mut s, op);
        s
    }

    fn kernel_failure(s: &mut NotebookSession, message: &str) {
        s.kernel_event("document-a", "/p/a.ipynb", "dead", Some(message));
    }

    fn shown(s: &NotebookSession) -> String {
        s.cells()[0].source.clone()
    }

    #[test]
    fn retires_event_listeners_even_when_the_backend_project_disappears_during_close() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let kernel = s.state().kernel.clone();
        let failure = m.failure("The backend project was replaced");
        m.close = Some(failure.clone());
        let op = s.close();
        assert_eq!(m.fails(&mut s, op), failure);
        kernel_failure(&mut s, "Late failure from the retired kernel");
        assert_eq!(s.state().error, None);
        assert_eq!(s.state().kernel, kernel);
    }

    #[test]
    fn still_releases_the_backend_document_when_interrupting_a_running_batch_fails_during_close() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let retired = m.failure("notebook project has closed");
        m.interrupt = Some(retired.clone());
        m.hold.push("execute");
        let execution = s.execute(None, RequestId(7));
        m.run(&mut s);
        assert_eq!(m.held.len(), 1);
        let close = s.close();
        m.run(&mut s);
        // The interrupt fails, and the batch ends with the same failure.
        m.release(&mut s, Some(Err(retired.clone())));
        assert_eq!(m.outcome(execution), Some(Err(retired)));
        assert_eq!(m.outcome(close), Some(Ok(())));
        assert_eq!(m.closed, ["document-a"]);
    }

    #[test]
    fn retains_rapid_typing_over_an_older_acknowledgement_and_serializes_the_latest_intent() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        m.hold.push("edit");
        s.edit(source("first"));
        m.run(&mut s);
        s.edit(source("second"));
        s.edit(source("latest typing"));
        assert_eq!(shown(&s), "latest typing");
        assert!(s.is_dirty());
        let mut visible = Vec::new();
        for request in std::mem::take(&mut m.held) {
            let answer = m.respond(&request.call);
            s.answer(request.ticket, answer);
            visible.push(shown(&s));
            loop {
                let requests = s.take_requests();
                if requests.is_empty() {
                    break;
                }
                for r in requests {
                    let answer = m.respond(&r.call);
                    s.answer(r.ticket, answer);
                    visible.push(shown(&s));
                }
            }
        }
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert!(visible.iter().all(|v| v == "latest typing"), "{visible:?}");
        assert_eq!(m.doc.cells[0].source, "latest typing");
        assert_eq!(m.doc.version, 3);
        let save = s.save();
        m.ok(&mut s, save);
        assert!(!s.is_dirty());
    }

    #[test]
    fn rebases_stable_cell_source_intent_after_an_out_of_sync_response() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        m.doc.version = 4;
        m.doc.cells[0].source = "other split changed this".to_owned();
        s.edit(source("retained user intent"));
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(m.doc.cells[0].source, "retained user intent");
        assert_eq!(s.state().snapshot.as_ref().map(|s| s.version), Some(5));
        assert_eq!(s.state().pending, 0);
    }

    #[test]
    fn orders_typing_after_an_in_flight_undo_so_its_response_cannot_overwrite_the_new_source() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        m.hold.push("undo");
        s.edit(source("before undo"));
        let undo = s.undo();
        s.edit(source("immediately after undo"));
        m.run(&mut s);
        assert_eq!(m.held.len(), 1);
        s.edit(source("typing after undo"));
        assert_eq!(shown(&s), "typing after undo");
        m.release(&mut s, None);
        assert_eq!(m.outcome(undo), Some(Ok(())));
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(m.doc.cells[0].source, "typing after undo");
        assert_eq!(shown(&s), "typing after undo");
        assert_eq!(m.doc.version, 4);
    }

    #[test]
    fn keeps_failed_and_subsequent_source_visible_dirty_until_an_explicit_retry_succeeds() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        m.edit = Some(m.failure("Backend disconnected"));
        s.edit(source("unsent draft"));
        let flush = s.flush();
        assert_eq!(m.fails(&mut s, flush).message, "Backend disconnected");
        s.edit(source("more typing after failure"));
        assert_eq!(shown(&s), "more typing after failure");
        assert_eq!(s.pending_sources(), [("cell-a".to_owned(), "more typing after failure".to_owned())]);
        assert!(s.is_dirty());
        assert_eq!(m.doc.cells[0].source, "original");
        m.edit = None;
        let save = s.save();
        m.ok(&mut s, save);
        assert_eq!(m.doc.cells[0].source, "more typing after failure");
        assert!(!s.is_dirty());
    }

    #[test]
    fn freezes_edit_admission_during_a_close_decision_and_resumes_only_after_cancellation() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        s.edit(source("checkpoint"));
        let prepare = s.prepare_close();
        m.ok(&mut s, prepare);
        s.edit(source("must not enter closing document"));
        assert_eq!(shown(&s), "checkpoint");
        s.cancel_close();
        s.edit(source("resumed"));
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(m.doc.cells[0].source, "resumed");
    }

    #[test]
    fn retains_a_kernel_failure_arriving_before_a_late_source_acknowledgement_and_subsequent_undo() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        m.hold.push("edit");
        s.edit(source("pending draft"));
        m.run(&mut s);
        let failure = "Kernel process exited";
        kernel_failure(&mut s, failure);
        m.release(&mut s, None);
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(shown(&s), "pending draft");
        assert_eq!(s.state().pending, 0);
        assert_eq!(s.state().error.as_deref(), Some(failure));
        let undo = s.undo();
        m.ok(&mut s, undo);
        assert_eq!(shown(&s), "original");
        assert_eq!(s.state().error.as_deref(), Some(failure));
    }

    #[test]
    fn retains_an_external_save_conflict_through_editing_until_save_succeeds() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let conflict = m.failure("File changed on disk");
        m.save = Some(conflict.clone());
        let save = s.save();
        assert_eq!(m.fails(&mut s, save), conflict);
        // The editor's promise catch keeps the failure's owner.
        s.report(&conflict);
        s.edit(source("draft after conflict"));
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(m.doc.cells[0].source, "draft after conflict");
        assert!(s.is_dirty());
        assert_eq!(s.state().error.as_deref(), Some(conflict.message.as_str()));
        m.save = None;
        let save = s.save();
        m.ok(&mut s, save);
        assert!(!s.is_dirty());
        assert_eq!(s.state().error, None);
    }

    #[test]
    fn clears_a_recovered_mutation_failure_on_source_retry_without_requiring_save() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let failure = m.failure("Edit transport unavailable");
        m.edit = Some(failure.clone());
        s.edit(source("retained draft"));
        let save = s.save();
        assert_eq!(m.fails(&mut s, save), failure);
        s.report(&failure);
        assert_eq!(s.state().error.as_deref(), Some(failure.message.as_str()));
        assert_eq!(s.state().pending, 1);
        assert_eq!(shown(&s), "retained draft");
        m.edit = None;
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(m.doc.cells[0].source, "retained draft");
        assert_eq!(s.state().pending, 0);
        assert!(s.is_dirty());
        assert_eq!(s.state().error, None);
    }

    #[test]
    fn reveals_a_retained_save_conflict_after_a_newer_mutation_failure_recovers() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let conflict = m.failure("External save conflict");
        m.save = Some(conflict.clone());
        let save = s.save();
        assert_eq!(m.fails(&mut s, save), conflict);
        let mutation = m.failure("Edit rejected");
        m.edit = Some(mutation.clone());
        s.edit(source("draft"));
        let flush = s.flush();
        assert_eq!(m.fails(&mut s, flush), mutation);
        assert_eq!(s.state().error.as_deref(), Some(mutation.message.as_str()));
        m.edit = None;
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(shown(&s), "draft");
        assert_eq!(s.state().error.as_deref(), Some(conflict.message.as_str()));
    }

    #[test]
    fn retains_execution_failure_across_editing_and_save_then_clears_it_after_successful_execution() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let failure = m.failure("Selected interpreter lacks ipykernel");
        m.execute = Some(failure.clone());
        let execution = s.execute(None, RequestId(1));
        assert_eq!(m.fails(&mut s, execution), failure);
        s.report(&failure);
        s.edit(source("print(42)"));
        let save = s.save();
        m.ok(&mut s, save);
        assert_eq!(m.doc.cells[0].source, "print(42)");
        assert_eq!(s.state().error.as_deref(), Some(failure.message.as_str()));
        m.execute = None;
        let execution = s.execute(None, RequestId(2));
        m.ok(&mut s, execution);
        assert!(!s.state().running);
        assert_eq!(s.state().error, None);
    }

    #[test]
    fn recovers_kernel_failure_on_restart_without_erasing_a_save_conflict() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let conflict = m.failure("External save conflict");
        m.save = Some(conflict.clone());
        let save = s.save();
        assert_eq!(m.fails(&mut s, save), conflict);
        kernel_failure(&mut s, "Kernel died");
        let restart = s.restart();
        m.ok(&mut s, restart);
        assert_eq!(s.state().kernel, "idle");
        assert!(!s.state().frozen);
        assert_eq!(s.state().error.as_deref(), Some(conflict.message.as_str()));
    }

    #[test]
    fn retains_environment_discovery_failure_through_editing_save_execution_and_restart_until_discovery_recovers() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let failure = m.failure("Environment discovery unavailable");
        m.environments = Some(failure.clone());
        s.detect_environments();
        m.run(&mut s);
        s.edit(source("draft"));
        let save = s.save();
        m.ok(&mut s, save);
        let execution = s.execute(None, RequestId(1));
        m.ok(&mut s, execution);
        let restart = s.restart();
        m.ok(&mut s, restart);
        assert_eq!(s.state().error.as_deref(), Some(failure.message.as_str()));
        m.environments = None;
        s.detect_environments();
        m.run(&mut s);
        assert_eq!(s.state().error, None);
    }

    #[test]
    fn recovers_reload_and_document_failures_after_discard_without_clearing_a_kernel_failure() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let conflict = m.failure("External save conflict");
        m.save = Some(conflict.clone());
        let save = s.save();
        assert_eq!(m.fails(&mut s, save), conflict);
        m.edit = Some(m.failure("Edit unavailable"));
        s.edit(source("discarded draft"));
        let flush = s.flush();
        assert_eq!(m.fails(&mut s, flush).message, "Edit unavailable");
        let failure = m.failure("Reload unavailable");
        m.reload = Some(failure.clone());
        let discard = s.discard();
        assert_eq!(m.fails(&mut s, discard), failure);
        s.report(&failure);
        kernel_failure(&mut s, "Kernel is still dead");
        m.reload = None;
        m.edit = None;
        let discard = s.discard();
        m.ok(&mut s, discard);
        assert_eq!(shown(&s), "original");
        assert_eq!(s.state().pending, 0);
        assert!(!s.is_dirty());
        assert_eq!(s.state().error.as_deref(), Some("Kernel is still dead"));
        s.cancel_close();
        let restart = s.restart();
        m.ok(&mut s, restart);
        assert_eq!(s.state().error, None);
    }

    #[test]
    fn retains_restart_failure_across_editing_and_clears_it_only_after_a_successful_restart() {
        let mut m = Backend::new();
        let mut s = loaded(&mut m);
        let failure = m.failure("Kernel restart failed");
        m.restart = Some(failure.clone());
        let restart = s.restart();
        assert_eq!(m.fails(&mut s, restart), failure);
        s.report(&failure);
        assert!(!s.state().frozen);
        s.edit(source("editable after failed restart"));
        let flush = s.flush();
        m.ok(&mut s, flush);
        assert_eq!(shown(&s), "editable after failed restart");
        assert_eq!(s.state().error.as_deref(), Some(failure.message.as_str()));
        m.restart = None;
        let restart = s.restart();
        m.ok(&mut s, restart);
        assert_eq!(s.state().error, None);
    }
}
