//! Debug sessions as the UI sees them, fed by the backend's debug events and by pulls (threads,
//! frames, locals) after each stop.
//!
//! [`DebugState`] does no I/O. What a transition asks for collects in the state and is handed out
//! by [`DebugState::take_effects`]; the answer to an [`Effect::Ask`] comes back, with its ticket,
//! through the continuation named after the command (`frames_loaded` for a stack trace, `started`
//! for a start). An answer that arrives after the session moved on is dropped.

use crate::debugger::breakpoints::Breakpoints;
use crate::debugger::breakpoints::model::{log_expressions, log_parts, render_log};
use crate::debugger::breakpoints::store::Flush;
use crate::debugger::effects::{
    BackendCall, Effect, Env, EvaluateContext, error_text, is_cancelled, notification_action, run_notification,
};
use crate::debugger::frames::frame_path;
use crate::debugger::missing_adapter::missing_adapter;
use crate::debugger::start_failure::start_failure_message;
use crate::debugger::taskbar_flash::TaskbarFlash;
use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use crate::run::ansi::{AnsiStream, Span, append_spans};
use crate::run::store::started_port;
use crate::workbench::notifications::NotificationLevel;
use ide_core::event::DebugEvent;
use ide_core::workflow_settings::ConsoleSettings;
use ide_core::{DebugSessionId, IdeError, RequestId, RunId};
use ide_dap::{DebugCapabilities, LaunchConfig, Scope, StackFrame, ThreadInfo, Variable};
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Ids from here on are sessions that only prepare a launch (build, classpath): the backend has
/// not started them and never names them.
pub const PREPARATION_BASE: u32 = 1 << 31;
/// Storage key of the watch expressions.
pub const WATCH_KEY: &str = "openidea.debugger.watches";
/// Command of the "Install and Debug" button of the missing-adapter notification:
/// [`DebugState::install_and_debug`].
pub const INSTALL_AND_DEBUG: &str = "debug.installAndDebug";
/// Console buffer: at most this many physical lines per session.
const MAX_CONSOLE_LINES: usize = 50_000;
/// A JDWP agent in server mode listens again, on a new port, when its debugger detaches; a
/// debugger that detaches as the JVM exits leaves an announcement of a JVM about to be gone. An
/// attach to a process this soon after a session on it ended may be to such a JVM.
const RELISTEN_MS: f64 = 3000.0;
/// Queued output is shown at the latest this long after it arrived, in milliseconds.
const OUTPUT_FLUSH_MS: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Starting,
    Running,
    Paused,
    Stopping,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct DebugSession {
    pub id: DebugSessionId,
    pub name: String,
    /// Originating configuration, retained for toolbar targeting and prepared debug reruns.
    pub run_config: Option<RunConfig>,
    /// Milliseconds since the epoch.
    pub started_at: Option<f64>,
    /// When it terminated.
    pub ended_at: Option<f64>,
    pub adapter: String,
    pub parent: Option<DebugSessionId>,
    /// Launch configuration (root sessions), used by Rerun.
    pub launch: Option<LaunchConfig>,
    /// The program runs in a terminal the IDE gave it: the console types into it while it runs.
    pub program_input: bool,
    /// Process the session is attached to (a Run tab's build tool); stopped with the session.
    pub process: Option<RunId>,
    pub status: SessionStatus,
    pub stopped_thread: Option<i64>,
    pub stop_reason: Option<String>,
    pub stop_description: Option<String>,
    pub threads: Vec<ThreadInfo>,
    pub selected_thread: Option<i64>,
    pub frames: Vec<StackFrame>,
    /// Counts the times `frames` was replaced: what was asked about an earlier stack is stale.
    pub frames_epoch: u64,
    pub selected_frame: Option<i64>,
    /// Breakpoint hits in this session.
    pub hits: u32,
    pub exit_code: Option<i32>,
    /// What the adapter supports (fetched after start).
    pub caps: Option<DebugCapabilities>,
    /// A cancelled JDT build may have lost class-file deltas needed by this session.
    pub hot_swap_needs_restart: bool,
}

impl DebugSession {
    fn blank(id: DebugSessionId, now_ms: f64) -> Self {
        Self {
            id,
            name: "Debug".to_owned(),
            run_config: None,
            started_at: Some(now_ms),
            ended_at: None,
            adapter: String::new(),
            parent: None,
            launch: None,
            program_input: false,
            process: None,
            status: SessionStatus::Starting,
            stopped_thread: None,
            stop_reason: None,
            stop_description: None,
            threads: Vec::new(),
            selected_thread: None,
            frames: Vec::new(),
            frames_epoch: 0,
            selected_frame: None,
            hits: 0,
            exit_code: None,
            caps: None,
            hot_swap_needs_restart: false,
        }
    }

    pub fn is_live(&self) -> bool {
        self.status != SessionStatus::Terminated
    }

    /// Replaces the stack; answers to what was asked about the old one no longer apply.
    pub fn set_frames(&mut self, frames: Vec<StackFrame>) {
        self.frames = frames;
        self.frames_epoch += 1;
    }

    /// The frame execution is at: the first one with a source that is not a label.
    pub fn top_source_frame(&self) -> Option<&StackFrame> {
        self.frames.iter().find(|f| frame_path(f).is_some() && f.presentation_hint.as_deref() != Some("label"))
    }

    /// Whether the console types into the session's program (it runs, in a terminal the IDE gave it).
    pub fn reads_program_input(&self) -> bool {
        self.program_input && self.status == SessionStatus::Running
    }

    /// Whether the session's adapter supports what `cap` reads (false while unknown).
    pub fn supports(&self, cap: impl Fn(&DebugCapabilities) -> bool) -> bool {
        self.caps.as_ref().is_some_and(cap)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleLine {
    pub id: u64,
    pub category: String,
    /// Plain text without escape sequences.
    pub text: String,
    /// `text` styled by its ANSI colours.
    pub spans: Vec<Span>,
    /// Length of `text` in UTF-16 code units.
    units: u32,
}

/// Execution point of the selected frame, plus the frame's locals for inline values.
#[derive(Debug, Clone, Default)]
pub struct ExecutionPoint {
    pub path: Option<String>,
    pub line: u32,
    /// The selected frame is the top frame (current position, not a caller).
    pub top: bool,
    /// The frame's locals by name (the first of a name wins).
    pub locals: BTreeMap<String, Variable>,
}

/// A configured debug root as the Services tool window lists it, also after it ended.
#[derive(Debug, Clone, PartialEq)]
pub struct ServiceSession {
    pub id: DebugSessionId,
    pub config: RunConfig,
    pub started_at: f64,
    /// `Paused` when the root or one of its child sessions is suspended.
    pub status: SessionStatus,
    pub process_id: Option<RunId>,
    /// The HTTP port the application announced on its console.
    pub http_port: Option<u16>,
    pub can_rerun: bool,
}

/// A live root session as the main toolbar lists it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolbarSession {
    pub id: DebugSessionId,
    pub config_id: Option<String>,
    pub name: String,
    pub started_at: f64,
    pub status: SessionStatus,
    /// The session, or one of its children, is suspended: `paused_session` is the one.
    pub paused: bool,
    pub paused_session: Option<DebugSessionId>,
    pub can_rerun: bool,
}

/// The context keys the debugger publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DebugContext {
    pub session_active: bool,
    pub session_running: bool,
    pub session_paused: bool,
    /// Dart and Flutter sessions: Hot Reload / Hot Restart.
    pub session_dart: bool,
    pub can_rerun: bool,
    pub can_hot_swap: bool,
}

impl DebugContext {
    /// The keys and their values.
    pub fn entries(&self) -> [(&'static str, bool); 6] {
        [
            ("debugSessionActive", self.session_active),
            ("debugSessionRunning", self.session_running),
            ("debugSessionPaused", self.session_paused),
            ("debugSessionDart", self.session_dart),
            ("debugCanRerun", self.can_rerun),
            ("debugCanHotSwap", self.can_hot_swap),
        ]
    }
}

/// What the debugger shows other features: toolbar and Services entries, and its context keys.
#[derive(Debug, Clone, Default)]
pub struct Published {
    pub sessions: Vec<ToolbarSession>,
    pub service_sessions: Vec<ServiceSession>,
    pub context: DebugContext,
}

/// How a session is started.
#[derive(Debug, Clone)]
pub struct StartOptions {
    pub run_config: Option<RunConfig>,
    /// The preparation session this launch continues.
    pub preparation: Option<DebugSessionId>,
    /// A process started for this session (e.g. Maven with a JDWP agent): Stop ends it too.
    pub process: Option<RunId>,
    /// Rerun repeats the launch (false for attaching to a process that is gone afterwards).
    pub rerunnable: bool,
    /// `Some(false)`: keep the current tool window (the Services panel starts debuggers in place).
    pub show: Option<bool>,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self { run_config: None, preparation: None, process: None, rerunnable: true, show: None }
    }
}

/// How a start that [`DebugState::start_session`] began has ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOutcome {
    /// What `start_session` returned.
    pub start: u64,
    /// The session, or `None` when it did not start.
    pub session: Option<DebugSessionId>,
}

/// A start between `start_session` and the backend's answer.
#[derive(Debug, Clone)]
struct StartInFlight {
    id: u64,
    launch: LaunchConfig,
    options: StartOptions,
    after_detach: bool,
    previous_active: Option<DebugSessionId>,
    started_at: f64,
}

/// What a session had while suspended, to put back when a resuming command fails.
#[derive(Debug, Clone)]
struct Suspended {
    status: SessionStatus,
    frames: Vec<StackFrame>,
    selected_frame: Option<i64>,
    stopped_thread: Option<i64>,
}

/// What an [`Effect::Ask`] was for.
#[derive(Debug, Clone)]
enum Pending {
    Capabilities(DebugSessionId),
    Threads(DebugSessionId),
    Frames { session: DebugSessionId, thread: i64, reveal: bool, epoch: u64, show_execution_point: bool },
    Scopes { session: DebugSessionId, frame: i64, epoch: u64 },
    Variables { session: DebugSessionId, frame: i64, epoch: u64 },
    LogValue { log: u64, part: usize, slot: usize },
    Start(Box<StartInFlight>),
    Stop { root: DebugSessionId },
    Resume { session: DebugSessionId, before: Box<Suspended>, epoch: u64 },
    SetVariable { session: DebugSessionId, frame: Option<i64> },
}

/// The value of a log expression, once evaluated: the text, or why it has none.
type LogValue = Option<Result<String, String>>;
/// A log line's template and the values of its expressions so far.
type LogPart = (String, Vec<LogValue>);

/// The log lines of a suspending breakpoint's hit, while their expressions are evaluated.
#[derive(Debug, Clone)]
struct HitLog {
    session: DebugSessionId,
    parts: Vec<LogPart>,
}

/// Streamed output buffered until the next frame.
#[derive(Debug, Clone)]
struct PendingOutput {
    session: DebugSessionId,
    category: String,
    raw: String,
}

/// What the transitions that reach beyond the sessions work with.
pub struct Ctx<'a> {
    pub breakpoints: &'a mut Breakpoints,
    pub env: Env<'a>,
    pub i18n: &'a mut I18n,
}

/// A failed resuming command: the session is suspended again as it was, unless something newer
/// happened to it.
#[derive(Debug, Clone)]
pub struct ResumeFailure {
    pub session: DebugSessionId,
    pub error: IdeError,
}

/// What closing a session's tab needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseTab {
    Missing,
    /// The session is live: ask whether to terminate `name`, then [`DebugState::close_confirmed`].
    Confirm {
        name: String,
    },
    Closed,
}

#[derive(Debug, Default)]
pub struct DebugState {
    sessions: BTreeMap<DebugSessionId, DebugSession>,
    /// Session shown in the Debug tool window / title bar.
    active: Option<DebugSessionId>,
    console: BTreeMap<DebugSessionId, Vec<ConsoleLine>>,
    watches: Vec<String>,
    /// Incremented on every stop; views re-pull data when it changes.
    stop_seq: u64,
    execution_point: ExecutionPoint,
    /// Counts the changes of the execution point's place (path, line, top).
    execution_point_rev: u64,
    line_seq: u64,
    /// ANSI state per session and output category (a colour can span several output events).
    ansi_streams: HashMap<(DebugSessionId, String), AnsiStream>,
    pending_output: Vec<PendingOutput>,
    /// When queued output is shown if no frame came first.
    output_flush_at: Option<f64>,
    /// The HTTP port each session's application announced; kept after the line scrolls out.
    announced_ports: HashMap<DebugSessionId, u16>,
    /// Starts in flight: their root session's id is not known yet.
    pending_starts: u32,
    /// Sessions adopted while a start was in flight, not yet confirmed by its result.
    provisional: BTreeSet<DebugSessionId>,
    /// Every session id this window has owned (late events of dropped sessions are ignored).
    owned: HashSet<DebugSessionId>,
    preparation_seq: u32,
    /// Preparations under way, with the backend request that Stop cancels.
    preparations: BTreeMap<DebugSessionId, Option<RequestId>>,
    /// Preparations Stop cancelled: what they still deliver is dropped.
    aborted: HashSet<DebugSessionId>,
    /// Starts that wait for the breakpoints to reach the backend.
    waiting_for_breakpoints: Vec<StartInFlight>,
    start_seq: u64,
    /// The launch "Install and Debug" repeats, with the tool to install first.
    retry: Option<(String, LaunchConfig, StartOptions)>,
    tickets: u64,
    pending: HashMap<u64, Pending>,
    hit_logs: HashMap<u64, HitLog>,
    published: Published,
    publish_seq: u64,
    taskbar: TaskbarFlash,
    effects: Vec<Effect>,
}

fn is_breakpoint_stop(reason: &str) -> bool {
    matches!(reason, "breakpoint" | "function breakpoint")
}

/// The spans of a chunk of output: escapes other than colours dropped, and a carriage return
/// before a line break or the end too (the console doesn't overwrite lines).
fn parse_output(stream: &mut AnsiStream, raw: &str) -> Vec<Span> {
    let cleaned = stream.clean(raw);
    let mut kept = String::with_capacity(cleaned.len());
    let mut chars = cleaned.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\r' || !matches!(chars.peek(), None | Some('\n')) {
            kept.push(c);
        }
    }
    stream.spans(&kept)
}

/// Drops the oldest lines once past the line or configured character cap.
fn trim_console(lines: &mut Vec<ConsoleLine>, settings: &ConsoleSettings) {
    let mut drop = 0;
    if lines.len() > MAX_CONSOLE_LINES {
        drop = lines.len() - MAX_CONSOLE_LINES + MAX_CONSOLE_LINES.div_ceil(10);
    }
    if settings.override_buffer {
        let limit = u64::from(settings.buffer_kb) * 1024;
        let mut chars = 0;
        for (i, line) in lines.iter().enumerate().rev() {
            chars += u64::from(line.units) + 1;
            if chars > limit {
                drop = drop.max(i + 1);
                break;
            }
        }
    }
    if drop > 0 {
        *lines = lines.split_off(drop.min(lines.len()));
    }
}

impl DebugState {
    // --- accessors ------------------------------------------------------------

    pub fn sessions(&self) -> &BTreeMap<DebugSessionId, DebugSession> {
        &self.sessions
    }

    pub fn session(&self, id: DebugSessionId) -> Option<&DebugSession> {
        self.sessions.get(&id)
    }

    /// A session to change directly (tests, and the hot swap flags). [`DebugState::publish_context`]
    /// afterwards when the change shows outside.
    pub fn session_mut(&mut self, id: DebugSessionId) -> Option<&mut DebugSession> {
        self.sessions.get_mut(&id)
    }

    pub fn active(&self) -> Option<DebugSessionId> {
        self.active
    }

    pub fn active_session(&self) -> Option<&DebugSession> {
        self.active.and_then(|id| self.sessions.get(&id))
    }

    pub fn console(&self, id: DebugSessionId) -> &[ConsoleLine] {
        self.console.get(&id).map_or(&[], Vec::as_slice)
    }

    pub fn watches(&self) -> &[String] {
        &self.watches
    }

    pub fn stop_seq(&self) -> u64 {
        self.stop_seq
    }

    pub fn execution_point(&self) -> &ExecutionPoint {
        &self.execution_point
    }

    /// Changes when the execution point moves to another place: whatever is tied to the paused
    /// line (picking a Smart Step Into target) ends then.
    pub fn execution_point_rev(&self) -> u64 {
        self.execution_point_rev
    }

    pub fn published(&self) -> &Published {
        &self.published
    }

    /// Counts the times [`DebugState::published`] was renewed.
    pub fn publish_seq(&self) -> u64 {
        self.publish_seq
    }

    /// What the transitions since the last call ask for, in order.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        std::mem::take(&mut self.effects)
    }

    /// Whether `id` is a session that only prepares a launch.
    pub fn is_preparation(id: DebugSessionId) -> bool {
        id.0 >= PREPARATION_BASE
    }

    /// Root (user-started) session of `id`.
    pub fn root_of(&self, id: DebugSessionId) -> DebugSessionId {
        let parent_of = |id: DebugSessionId| {
            let parent = self.sessions.get(&id)?.parent?;
            self.sessions.contains_key(&parent).then_some(parent)
        };
        let mut current = id;
        while let Some(parent) = parent_of(current) {
            current = parent;
        }
        current
    }

    /// Breakpoint ↔ session glue: the unverified look only makes sense while debugging.
    pub fn sessions_running(&self) -> bool {
        self.sessions.values().any(DebugSession::is_live)
    }

    fn ask(&mut self, call: BackendCall, pending: Pending) {
        self.tickets += 1;
        self.pending.insert(self.tickets, pending);
        self.effects.push(Effect::Ask { ticket: self.tickets, call });
    }

    fn call(&mut self, call: Option<BackendCall>) {
        self.effects.extend(call.map(Effect::Call));
    }

    fn patch(&mut self, id: DebugSessionId, change: impl FnOnce(&mut DebugSession)) {
        if let Some(session) = self.sessions.get_mut(&id) {
            change(session);
        }
        self.publish_context();
    }

    /// Takes over what the breakpoint stores ask for.
    fn drain(&mut self, breakpoints: &mut Breakpoints) {
        self.effects.extend(breakpoints.take_effects());
    }

    /// Registers a session as owned by this window (idempotent): running, unless `init` says
    /// otherwise. Returns false when the session is there already.
    pub fn adopt_session(&mut self, id: DebugSessionId, now_ms: f64, init: impl FnOnce(&mut DebugSession)) -> bool {
        if self.sessions.contains_key(&id) {
            return false;
        }
        self.owned.insert(id);
        let mut session = DebugSession::blank(id, now_ms);
        session.status = SessionStatus::Running;
        init(&mut session);
        self.ask(BackendCall::DebugCapabilities(id), Pending::Capabilities(id));
        self.sessions.insert(id, session);
        if !self.active_session().is_some_and(DebugSession::is_live) {
            self.active = Some(id);
        }
        self.publish_context();
        true
    }

    /// The adapter's capabilities arrived.
    pub fn capabilities_loaded(&mut self, ticket: u64, result: Result<DebugCapabilities, IdeError>) {
        let Some(Pending::Capabilities(id)) = self.pending.remove(&ticket) else { return };
        if let Ok(caps) = result
            && self.sessions.contains_key(&id)
        {
            self.patch(id, |s| s.caps = Some(caps));
        }
    }

    /// Whether an event of session `id` is this window's, adopting the session when it is: a
    /// child (announced by `started` with a parent) of one of its sessions, or the root session of
    /// a start still in flight here (its build output and `started` arrive before the start
    /// answers with the id). Anything else is not ours.
    fn own_session(&mut self, id: DebugSessionId, event: &DebugEvent, now_ms: f64) -> bool {
        if self.sessions.contains_key(&id) {
            return true;
        }
        if self.owned.contains(&id) {
            return false;
        }
        let started = match event {
            DebugEvent::Started { name, adapter, parent } => Some((name.clone(), adapter.clone(), *parent)),
            _ => None,
        };
        if let Some((name, adapter, Some(parent))) = started {
            if !self.sessions.contains_key(&parent) {
                return false;
            }
            return self.adopt_session(id, now_ms, |s| {
                s.name = name;
                s.adapter = adapter;
                s.parent = Some(parent);
            });
        }
        if self.pending_starts == 0 {
            return false;
        }
        self.provisional.insert(id);
        self.adopt_session(id, now_ms, |s| {
            if let Some((name, adapter, _)) = started {
                s.name = name;
                s.adapter = adapter;
            }
        })
    }

    /// A start finished (`id` on success): sessions no start claimed are failed starts' leftovers,
    /// kept as terminated or, with `discard`, removed.
    fn settle_start(&mut self, id: Option<DebugSessionId>, discard: bool, ctx: &mut Ctx) {
        self.pending_starts = self.pending_starts.saturating_sub(1);
        if let Some(id) = id {
            self.provisional.remove(&id);
        }
        if self.pending_starts > 0 {
            return;
        }
        let leftovers = std::mem::take(&mut self.provisional);
        if discard {
            for id in &leftovers {
                self.sessions.remove(id);
                self.console.remove(id);
                self.ansi_streams.retain(|(session, _), _| session != id);
            }
            self.active = self.active.filter(|active| self.sessions.contains_key(active));
            self.publish_context();
            return;
        }
        for id in leftovers {
            if self.sessions.get(&id).is_some_and(DebugSession::is_live) {
                self.mark_terminated(id, ctx);
            }
        }
    }

    /// Drops finished sessions; running ones stay (multiple concurrent sessions are fine).
    fn drop_finished(&mut self) {
        self.sessions.retain(|_, session| session.is_live());
        self.console.retain(|id, _| self.sessions.contains_key(id));
        self.ansi_streams.retain(|(id, _), _| self.sessions.contains_key(id));
        self.active = self.active.filter(|active| self.sessions.contains_key(active));
        self.publish_context();
    }

    fn just_detached(&self, process: Option<RunId>, now_ms: f64) -> bool {
        let Some(process) = process else { return false };
        self.sessions.values().any(|s| {
            s.process == Some(process)
                && s.status == SessionStatus::Terminated
                && s.ended_at.is_some_and(|ended| now_ms - ended < RELISTEN_MS)
        })
    }

    /// Renews what other features see of the debugger: the toolbar and Services entries and the
    /// context keys.
    pub fn publish_context(&mut self) {
        let paused_under = |root: DebugSessionId| {
            self.sessions
                .values()
                .find(|child| child.status == SessionStatus::Paused && self.root_of(child.id) == root)
                .map(|child| child.id)
        };
        let can_rerun = |session: &DebugSession| session.run_config.is_some() || session.launch.is_some();
        let service_sessions = self
            .sessions
            .values()
            .filter(|session| session.parent.is_none())
            .filter_map(|session| {
                let paused = session.is_live() && paused_under(session.id).is_some();
                Some(ServiceSession {
                    id: session.id,
                    config: session.run_config.clone()?,
                    started_at: session.started_at.unwrap_or(0.0),
                    status: if paused { SessionStatus::Paused } else { session.status },
                    process_id: session.process,
                    http_port: self.announced_ports.get(&session.id).copied(),
                    can_rerun: can_rerun(session),
                })
            })
            .collect();
        let sessions = self
            .sessions
            .values()
            .filter(|session| session.parent.is_none() && session.is_live())
            .map(|session| {
                let paused_session = paused_under(session.id);
                ToolbarSession {
                    id: session.id,
                    config_id: session.run_config.as_ref().map(|config| config.id.clone()),
                    name: session.run_config.as_ref().map_or(&session.name, |config| &config.name).clone(),
                    started_at: session.started_at.unwrap_or(0.0),
                    status: session.status,
                    paused: paused_session.is_some(),
                    paused_session,
                    can_rerun: can_rerun(session),
                }
            })
            .collect();
        let active = self.active_session();
        let root = active.and_then(|s| self.sessions.get(&self.root_of(s.id)));
        let context = DebugContext {
            session_active: self.sessions_running(),
            session_running: active.is_some_and(|s| s.status == SessionStatus::Running),
            session_paused: active.is_some_and(|s| s.status == SessionStatus::Paused),
            session_dart: root.is_some_and(|root| root.adapter == "dart"),
            can_rerun: root.is_some_and(can_rerun),
            can_hot_swap: self.sessions.values().any(|s| s.is_live() && s.supports(|caps| caps.hot_code_replace)),
        };
        self.published = Published { sessions, service_sessions, context };
        self.publish_seq += 1;
    }

    // --- console --------------------------------------------------------------

    /// The HTTP port the session's application announced on its console ("Tomcat started on port
    /// 8080"), the same embedded web server line the Run console is read for.
    pub fn session_http_port(&self, id: DebugSessionId) -> Option<u16> {
        self.announced_ports.get(&id).copied()
    }

    /// Republishes the Services entries only when a console announced a port they don't show yet.
    fn republish_for_ports(&mut self) {
        let stale = |entry: &ServiceSession| entry.http_port != self.announced_ports.get(&entry.id).copied();
        if self.published.service_sessions.iter().any(stale) {
            self.publish_context();
        }
    }

    /// Splits styled output at real line breaks into the session's console, continuing a partial
    /// line of the same category.
    fn append_lines(&mut self, session: DebugSessionId, category: &str, spans: Vec<Span>, settings: &ConsoleSettings) {
        if spans.iter().all(|span| span.text.is_empty()) {
            return;
        }
        let lines = self.console.entry(session).or_default();
        let mut open = lines.last().is_some_and(|last| last.category == category && !last.text.ends_with('\n'));
        for span in spans {
            for piece in span.text.split_inclusive('\n') {
                if !open {
                    self.line_seq += 1;
                    lines.push(ConsoleLine {
                        id: self.line_seq,
                        category: category.to_owned(),
                        text: String::new(),
                        spans: Vec::new(),
                        units: 0,
                    });
                }
                let Some(line) = lines.last_mut() else { continue };
                line.text.push_str(piece);
                line.units += utf16::len(piece);
                append_spans(&mut line.spans, [Span { text: piece.to_owned(), style: span.style }]);
                if let Some(port) = started_port(&line.text) {
                    self.announced_ports.insert(session, port);
                }
                open = !piece.ends_with('\n');
            }
        }
        trim_console(lines, settings);
    }

    /// Appends output to a session's console right away (queued output first, to keep the order).
    pub fn append_console(&mut self, session: DebugSessionId, category: &str, raw: &str, settings: &ConsoleSettings) {
        if raw.is_empty() {
            return;
        }
        self.flush_console_output(settings);
        let stream = self.ansi_streams.entry((session, category.to_owned())).or_default();
        let spans = parse_output(stream, raw);
        self.append_lines(session, category, spans, settings);
        self.republish_for_ports();
    }

    /// Queues streamed debuggee output: it is parsed and shown once per frame (or after
    /// [`DebugState::next_output_flush`] while no frame comes), with pending chunks merged per
    /// session and category, so a flood of output events costs one update.
    pub fn append_console_queued(&mut self, session: DebugSessionId, category: &str, raw: &str, now_ms: f64) {
        if raw.is_empty() {
            return;
        }
        let same_stream = |last: &&mut PendingOutput| last.session == session && last.category == category;
        match self.pending_output.last_mut().filter(same_stream) {
            Some(last) => last.raw.push_str(raw),
            None => {
                let chunk = PendingOutput { session, category: category.to_owned(), raw: raw.to_owned() };
                self.pending_output.push(chunk);
            }
        }
        if self.output_flush_at.is_none() {
            self.output_flush_at = Some(now_ms + OUTPUT_FLUSH_MS);
        }
    }

    /// When queued output has to be shown at the latest ([`DebugState::flush_console_output`]); the
    /// caller also flushes on the next frame it draws.
    pub fn next_output_flush(&self) -> Option<f64> {
        self.output_flush_at
    }

    /// Shows the queued output. Output of a session that is gone by now is dropped.
    pub fn flush_console_output(&mut self, settings: &ConsoleSettings) {
        self.output_flush_at = None;
        if self.pending_output.is_empty() {
            return;
        }
        for chunk in std::mem::take(&mut self.pending_output) {
            if !self.sessions.contains_key(&chunk.session) {
                continue;
            }
            let stream = self.ansi_streams.entry((chunk.session, chunk.category.clone())).or_default();
            let spans = parse_output(stream, &chunk.raw);
            self.append_lines(chunk.session, &chunk.category, spans, settings);
        }
        self.republish_for_ports();
    }

    pub fn clear_console(&mut self, session: DebugSessionId, settings: &ConsoleSettings) {
        self.flush_console_output(settings);
        self.ansi_streams.retain(|(id, _), _| *id != session);
        self.console.insert(session, Vec::new());
    }
}

impl DebugState {
    // --- lifecycle ------------------------------------------------------------

    /// A visible session owns the preparation of `config`'s launch (build, classpath), including
    /// its console and Stop, before the backend has a session. Returns its id.
    pub fn begin_preparation(&mut self, config: RunConfig, show: Option<bool>, ctx: &mut Ctx) -> DebugSessionId {
        self.preparation_seq += 1;
        let id = DebugSessionId(PREPARATION_BASE + self.preparation_seq);
        self.preparations.insert(id, None);
        let name = config.name.clone();
        let mut session = DebugSession::blank(id, ctx.env.now_ms);
        session.name.clone_from(&name);
        session.adapter = config.debug_adapter.clone().unwrap_or_default();
        session.run_config = Some(config);
        self.sessions.insert(id, session);
        self.console.insert(id, Vec::new());
        self.active = Some(id);
        self.publish_context();
        if show != Some(false) && ctx.env.run.show_on_start {
            self.effects.push(Effect::ShowToolWindow { focus: false });
        }
        let starting = ctx.i18n.tx_built(&["Starting debugger for ", "…"], &[Param::from(name)]);
        self.preparation_report(id, &format!("{starting}\n"), false, ctx.env.console);
        id
    }

    /// The cancellable backend request the preparation `id` waits on: Stop cancels it.
    pub fn set_preparation_request(&mut self, id: DebugSessionId, request: Option<RequestId>) {
        if let Some(waiting_on) = self.preparations.get_mut(&id) {
            *waiting_on = request;
        }
    }

    /// Whether Stop cancelled the preparation `id`: what it still delivers is to be dropped.
    pub fn preparation_aborted(&self, id: DebugSessionId) -> bool {
        self.aborted.contains(&id)
    }

    /// Progress of a preparation, for its console (`stderr`: as error output).
    pub fn preparation_report(&mut self, id: DebugSessionId, text: &str, stderr: bool, settings: &ConsoleSettings) {
        if !self.aborted.contains(&id) && self.sessions.contains_key(&id) {
            self.append_console(id, if stderr { "stderr" } else { "system" }, text, settings);
        }
    }

    /// The preparation is over: its session, unless a started one took its place, ends.
    fn finish_preparation(&mut self, id: Option<DebugSessionId>, ctx: &mut Ctx) {
        let Some(id) = id else { return };
        self.preparations.remove(&id);
        self.aborted.remove(&id);
        if self.sessions.contains_key(&id) {
            self.mark_terminated(id, ctx);
        }
    }

    /// The preparation `id` produced `launch`: the debug adapter starts, and its session takes
    /// over the preparation's console. Returns what [`DebugState::start_session`] returns.
    pub fn preparation_ready(
        &mut self,
        id: DebugSessionId,
        launch: LaunchConfig,
        show: Option<bool>,
        ctx: &mut Ctx,
    ) -> Option<u64> {
        if self.aborted.contains(&id) {
            self.finish_preparation(Some(id), ctx);
            return None;
        }
        let run_config = self.sessions.get(&id).and_then(|session| session.run_config.clone());
        let options = StartOptions { run_config, preparation: Some(id), show, ..Default::default() };
        let started = self.start_session(launch, options, ctx);
        if started.is_none() {
            self.finish_preparation(Some(id), ctx);
        }
        started
    }

    /// The preparation `id` failed with `message` (`cancelled`: because its request was). Returns
    /// whether the failure is one to tell the user about: not after Stop, not a cancellation.
    pub fn preparation_failed(&mut self, id: DebugSessionId, message: &str, cancelled: bool, ctx: &mut Ctx) -> bool {
        let report = !self.aborted.contains(&id) && !cancelled;
        if report {
            self.preparation_report(id, &format!("{message}\n"), true, ctx.env.console);
        }
        self.finish_preparation(Some(id), ctx);
        report
    }

    /// Starts a debug session for `launch`. Returns the start's id, which the [`StartOutcome`] of
    /// [`DebugState::started`] or [`DebugState::breakpoints_synced`] carries; `None` when nothing
    /// can start here.
    pub fn start_session(&mut self, launch: LaunchConfig, options: StartOptions, ctx: &mut Ctx) -> Option<u64> {
        if !ctx.env.desktop {
            let request = run_notification(NotificationLevel::Warning, "Debugging needs the desktop app");
            self.effects.push(Effect::Notify(Box::new(request)));
            return None;
        }
        // Attaching to a JVM that may be gone: the finished session stays until the attach succeeds.
        let after_detach = self.just_detached(options.process, ctx.env.now_ms);
        let previous_active = self.active;
        if !after_detach {
            self.drop_finished();
        }
        if options.show != Some(false) && ctx.env.run.show_on_start {
            self.effects.push(Effect::ShowToolWindow { focus: false });
        }
        self.pending_starts += 1;
        self.start_seq += 1;
        let start = StartInFlight {
            id: self.start_seq,
            launch,
            options,
            after_detach,
            previous_active,
            started_at: ctx.env.now_ms,
        };
        let id = start.id;
        // The backend takes its startup snapshot of the breakpoints: every edit has to be there.
        let flush = ctx.breakpoints.lines.flush(ctx.env.can_sync());
        self.drain(ctx.breakpoints);
        match flush {
            Flush::Done => self.send_start(start, ctx).then_some(id),
            Flush::Pending => {
                self.waiting_for_breakpoints.push(start);
                Some(id)
            }
        }
    }

    /// Asks the backend to start; false when Stop cancelled the launch's preparation meanwhile.
    fn send_start(&mut self, start: StartInFlight, ctx: &mut Ctx) -> bool {
        if start.options.preparation.is_some_and(|id| self.aborted.contains(&id)) {
            self.start_failed(start, None, ctx);
            return false;
        }
        let call = BackendCall::DebugStart(Box::new(start.launch.clone()));
        self.ask(call, Pending::Start(Box::new(start)));
        true
    }

    /// The backend answered a breakpoint sync of `path`. Starts that waited for the breakpoints go
    /// on once the last one is there; a failed sync fails them: their outcomes are returned.
    pub fn breakpoints_synced(
        &mut self,
        path: &str,
        result: Result<Vec<ide_dap::BreakpointState>, IdeError>,
        ctx: &mut Ctx,
    ) -> Vec<StartOutcome> {
        let flushed = ctx.breakpoints.lines.sync_finished(path, result, ctx.env.can_sync());
        self.drain(ctx.breakpoints);
        let Some(flushed) = flushed else { return Vec::new() };
        let mut outcomes = Vec::new();
        for start in std::mem::take(&mut self.waiting_for_breakpoints) {
            let id = start.id;
            let sent = match &flushed {
                Ok(()) => self.send_start(start, ctx),
                Err(error) => {
                    self.start_failed(start, Some(error.clone()), ctx);
                    false
                }
            };
            if !sent {
                outcomes.push(StartOutcome { start: id, session: None });
            }
        }
        outcomes
    }

    /// The backend answered a start.
    pub fn started(
        &mut self,
        ticket: u64,
        result: Result<DebugSessionId, IdeError>,
        ctx: &mut Ctx,
    ) -> Option<StartOutcome> {
        let Some(Pending::Start(start)) = self.pending.remove(&ticket) else { return None };
        let id = start.id;
        let session = match result {
            Ok(session) => self.start_succeeded(*start, session, ctx),
            Err(error) => {
                self.start_failed(*start, Some(error), ctx);
                None
            }
        };
        Some(StartOutcome { start: id, session })
    }

    fn start_succeeded(&mut self, start: StartInFlight, id: DebugSessionId, ctx: &mut Ctx) -> Option<DebugSessionId> {
        let StartInFlight { launch, options, after_detach, started_at, .. } = start;
        self.owned.insert(id);
        if after_detach {
            self.drop_finished();
        }
        if options.preparation.is_some_and(|preparation| self.aborted.contains(&preparation)) {
            // Stop came while the adapter was starting: the session it started ends right away.
            self.effects.push(Effect::Call(BackendCall::DebugStop(id)));
            self.settle_start(Some(id), false, ctx);
            if self.sessions.contains_key(&id) {
                self.mark_terminated(id, ctx);
            }
            self.finish_preparation(options.preparation, ctx);
            return None;
        }
        self.flush_console_output(ctx.env.console);
        let preparation = options.preparation.and_then(|preparation| self.sessions.remove(&preparation));
        if let Some(preparation) = &preparation {
            let mut lines = self.console.remove(&preparation.id).unwrap_or_default();
            lines.extend(self.console.remove(&id).unwrap_or_default());
            self.console.insert(id, lines);
            self.ansi_streams.retain(|(session, _), _| *session != preparation.id);
        }
        let mut session = self.sessions.remove(&id).unwrap_or_else(|| {
            let mut session = DebugSession::blank(id, started_at);
            session.status = SessionStatus::Running;
            session
        });
        session.name.clone_from(&launch.name);
        session.run_config = options.run_config;
        let prepared_at = preparation.as_ref().and_then(|preparation| preparation.started_at);
        session.started_at = prepared_at.or(Some(started_at));
        session.adapter.clone_from(&launch.adapter);
        session.launch = options.rerunnable.then_some(launch);
        session.process = options.process;
        if session.status == SessionStatus::Starting {
            session.status = SessionStatus::Running;
        }
        self.sessions.insert(id, session);
        self.active = Some(id);
        self.settle_start(Some(id), false, ctx);
        self.publish_context();
        ctx.breakpoints.exceptions.refresh(ctx.env.desktop);
        self.ask(BackendCall::DebugCapabilities(id), Pending::Capabilities(id));
        // Method breakpoints / watchpoints went out during configuration: pull their states.
        ctx.breakpoints.special.sync(ctx.env.can_sync(), ctx.breakpoints.lines.muted());
        self.drain(ctx.breakpoints);
        if let Some(preparation) = options.preparation {
            self.preparations.remove(&preparation);
        }
        Some(id)
    }

    /// A start did not happen; `error`: the backend's failure, `None` when Stop came first.
    fn start_failed(&mut self, start: StartInFlight, error: Option<IdeError>, ctx: &mut Ctx) {
        let StartInFlight { launch, options, after_detach, previous_active, .. } = start;
        let preparation = options.preparation;
        self.settle_start(None, after_detach, ctx);
        if after_detach {
            // The JVM that announced the port ended as its previous debugger detached.
            if self.active.is_none() {
                self.active = previous_active.filter(|previous| self.sessions.contains_key(previous));
            }
            self.publish_context();
            self.finish_preparation(preparation, ctx);
            return;
        }
        let aborted = preparation.is_some_and(|preparation| self.aborted.contains(&preparation));
        let Some(error) = error.filter(|error| !aborted && !is_cancelled(error)) else {
            self.finish_preparation(preparation, ctx);
            return;
        };
        if let Some(preparation) = preparation {
            self.append_console(preparation, "stderr", &format!("{}\n", error_text(&error)), ctx.env.console);
        }
        let mut request = match missing_adapter(&launch.adapter, &error) {
            Some(missing) => {
                let mut request = run_notification(NotificationLevel::Warning, missing.title);
                request.body = Some(missing.body);
                request.sticky = Some(true);
                request.actions = vec![notification_action("Install and Debug", INSTALL_AND_DEBUG)];
                self.retry = Some((missing.tool.to_owned(), launch, options));
                request
            }
            None => {
                let title = format!("Cannot start debugger for {}", launch.name);
                let mut request = run_notification(NotificationLevel::Error, title);
                request.body = Some(start_failure_message(error_text(&error), ctx.i18n));
                request
            }
        };
        request.key = Some("debug.start".to_owned());
        self.effects.push(Effect::Notify(Box::new(request)));
        self.finish_preparation(preparation, ctx);
    }

    /// "Install and Debug": installs the missing adapter. Once that is done the caller repeats
    /// the launch with [`DebugState::retry_start`]. Returns false when there is nothing to retry.
    pub fn install_and_debug(&mut self) -> bool {
        let Some((tool, ..)) = &self.retry else { return false };
        self.effects.push(Effect::ExecuteAction { id: "tools.install".to_owned(), arg: Some(json!(tool)) });
        true
    }

    /// Repeats the launch whose adapter was missing.
    pub fn retry_start(&mut self, ctx: &mut Ctx) -> Option<u64> {
        let (_, launch, options) = self.retry.take()?;
        self.start_session(launch, options, ctx)
    }

    /// Stops a Run process a session is attached to, through its owner when there is one, which
    /// records the user's stop intent.
    pub fn stop_associated_process(&mut self, process: RunId, env: &Env) {
        self.effects.push(if env.run_stop_action {
            Effect::ExecuteAction { id: "run.stopProcess".to_owned(), arg: Some(json!(process.0)) }
        } else {
            Effect::Call(BackendCall::RunStop(process))
        });
    }

    /// Stops the session `id` belongs to; `stop_process`: also the process it is attached to.
    pub fn stop_session(&mut self, id: DebugSessionId, stop_process: bool, ctx: &mut Ctx) {
        if !self.sessions.contains_key(&id) {
            return;
        }
        let root = self.root_of(id);
        if let Some(request) = self.preparations.get(&root).copied() {
            self.aborted.insert(root);
            self.call(request.map(BackendCall::CancelRequest));
            let cancelled = ctx.i18n.tx("Launch cancelled.");
            // Reported as the preparation's last line, past the mark that silences it from here on.
            self.append_console(root, "system", &format!("{cancelled}\n"), ctx.env.console);
            self.mark_terminated(root, ctx);
            return;
        }
        if Self::is_preparation(root) {
            return;
        }
        // Between the Stop press and the adapter's terminated event the session is shutting down:
        // keep it live (Run stays disabled) and let the toolbar show the spinner.
        for session in BTreeSet::from([root, id]) {
            if self.sessions.get(&session).is_some_and(DebugSession::is_live) {
                self.patch(session, |s| s.status = SessionStatus::Stopping);
            }
        }
        let process = self.sessions.get(&root).and_then(|root| root.process).filter(|_| stop_process);
        if let Some(process) = process {
            self.stop_associated_process(process, &ctx.env);
        }
        self.ask(BackendCall::DebugStop(root), Pending::Stop { root });
    }

    /// The backend answered a stop: a session that was gone already is finished here.
    pub fn stop_finished(&mut self, ticket: u64, result: Result<(), IdeError>, ctx: &mut Ctx) {
        let Some(Pending::Stop { root }) = self.pending.remove(&ticket) else { return };
        if result.is_err() {
            self.mark_terminated(root, ctx);
        }
    }

    /// Closes a session's tab: a live session is stopped first once the user agrees (the Run
    /// tab's question), then the window forgets it, and its children.
    pub fn close_session_tab(&mut self, id: DebugSessionId, ctx: &mut Ctx) -> CloseTab {
        let Some(session) = self.sessions.get(&id) else { return CloseTab::Missing };
        if session.is_live() && ctx.env.run.confirm_terminate {
            return CloseTab::Confirm { name: session.name.clone() };
        }
        self.close_confirmed(id, ctx);
        CloseTab::Closed
    }

    /// Closes the tab of `id` (the user agreed to terminate it, or nothing had to be asked).
    pub fn close_confirmed(&mut self, id: DebugSessionId, ctx: &mut Ctx) {
        if self.sessions.get(&id).is_some_and(DebugSession::is_live) {
            self.stop_session(id, true, ctx);
        }
        self.remove_session(id);
    }

    /// Forgets a session and its children; a selected one passes the selection to the tab on its left.
    pub fn remove_session(&mut self, id: DebugSessionId) {
        let order: Vec<DebugSessionId> = self.sessions.keys().copied().collect();
        let gone: BTreeSet<DebugSessionId> =
            self.sessions.values().filter(|s| s.id == id || s.parent == Some(id)).map(|s| s.id).collect();
        for id in &gone {
            self.sessions.remove(id);
            self.console.remove(id);
            self.owned.remove(id);
        }
        let at = order.iter().position(|other| *other == id).unwrap_or(order.len());
        let remains = |other: &&DebugSessionId| self.sessions.contains_key(*other);
        let neighbour = order[..at].iter().rev().find(remains).or_else(|| order[at..].iter().find(remains)).copied();
        self.active = self.active.filter(|active| self.sessions.contains_key(active)).or(neighbour);
        self.update_execution_point();
        self.publish_context();
    }

    /// Rerun: through the Run feature's debug flow for a session of a run configuration (stopping
    /// it first when it is live), else by repeating the launch. `show`: see [`StartOptions::show`].
    pub fn rerun_session(&mut self, id: DebugSessionId, show: Option<bool>, ctx: &mut Ctx) -> Option<u64> {
        let root = self.sessions.get(&self.root_of(id))?;
        let (root_id, live) = (root.id, root.is_live());
        let (run_config, launch) = (root.run_config.clone(), root.launch.clone());
        if run_config.is_none() && launch.is_none() {
            return None;
        }
        if live {
            self.stop_session(root_id, true, ctx);
        }
        if let Some(config) = run_config {
            let config = serde_json::to_value(config).unwrap_or(Value::Null);
            let arg = if show == Some(false) { json!({ "config": config, "show": false }) } else { config };
            self.effects.push(Effect::ExecuteAction { id: "run.debugConfig".to_owned(), arg: Some(arg) });
            return None;
        }
        self.start_session(launch?, StartOptions { show, ..Default::default() }, ctx)
    }

    fn mark_terminated(&mut self, id: DebugSessionId, ctx: &mut Ctx) {
        let stop_flash = self.taskbar.stop(ctx.env.desktop);
        self.call(stop_flash);
        let now_ms = ctx.env.now_ms;
        self.patch(id, |s| {
            s.status = SessionStatus::Terminated;
            s.ended_at = Some(now_ms);
            s.set_frames(Vec::new());
            s.selected_frame = None;
            s.stopped_thread = None;
        });
        // Prefer another live session (e.g. the parent of a finished child).
        if self.active == Some(id)
            && let Some(other) = self.sessions.values().find(|s| s.is_live())
        {
            self.active = Some(other.id);
        }
        if !self.sessions_running() {
            ctx.breakpoints.lines.reset_statuses();
            ctx.breakpoints.special.reset_statuses();
            if ctx.env.debugger.hide_on_termination {
                self.effects.push(Effect::HideToolWindow);
            }
        }
        self.update_execution_point();
        self.publish_context();
    }

    // --- selection / pulls ------------------------------------------------------

    pub fn select_session(&mut self, id: DebugSessionId) {
        self.active = Some(id);
        self.publish_context();
        self.update_execution_point();
    }

    /// "Show in Debug" of a Services entry: the session becomes the one shown, in the Debug tool
    /// window. Nothing happens for a session that is gone.
    pub fn show_service_session(&mut self, id: DebugSessionId) {
        if self.sessions.contains_key(&id) {
            self.select_session(id);
            self.effects.push(Effect::ShowToolWindow { focus: true });
        }
    }

    /// Resume of a toolbar entry: the suspended session under the root `id` becomes the active
    /// one and resumes.
    pub fn resume_toolbar_session(&mut self, id: DebugSessionId) {
        let entry = self.published.sessions.iter().find(|entry| entry.id == id);
        let paused = entry.and_then(|entry| entry.paused_session);
        self.select_session(paused.unwrap_or(id));
        self.effects.push(Effect::ExecuteAction { id: "debug.resume".to_owned(), arg: None });
    }

    pub fn select_thread(&mut self, id: DebugSessionId, thread: i64) {
        self.patch(id, |s| {
            s.selected_thread = Some(thread);
            s.set_frames(Vec::new());
            s.selected_frame = None;
        });
        self.update_execution_point();
        self.load_frames(id, thread, true, false);
    }

    /// Selects a frame; `reveal`: also open its source (without taking the focus).
    pub fn select_frame(&mut self, id: DebugSessionId, frame_id: i64, reveal: bool) {
        self.patch(id, |s| s.selected_frame = Some(frame_id));
        self.update_execution_point();
        let frame = self.sessions.get(&id).and_then(|s| s.frames.iter().find(|f| f.id == frame_id));
        if let Some(frame) = frame.filter(|_| reveal && self.active == Some(id)) {
            let opened = Self::reveal_frame(frame, false);
            self.effects.extend(opened);
        }
        self.load_locals(id, frame_id);
    }

    /// Opens a frame's source at its line (Jump to Source); nothing for a frame without one.
    pub fn reveal_frame(frame: &StackFrame, focus: bool) -> Option<Effect> {
        let path = frame_path(frame)?;
        Some(Effect::OpenFile { path, line: frame.line, column: frame.column, focus })
    }

    fn load_frames(&mut self, id: DebugSessionId, thread: i64, reveal: bool, show_execution_point: bool) {
        let Some(epoch) = self.sessions.get(&id).map(|s| s.frames_epoch) else { return };
        self.ask(
            BackendCall::DebugStackTrace { session: id, thread },
            Pending::Frames { session: id, thread, reveal, epoch, show_execution_point },
        );
    }

    /// A stack trace arrived. One asked for before the session stopped again, or moved to another
    /// thread, is dropped.
    pub fn frames_loaded(&mut self, ticket: u64, result: Result<Vec<StackFrame>, IdeError>, ctx: &mut Ctx) {
        let Some(Pending::Frames { session: id, thread, reveal, epoch, show_execution_point }) =
            self.pending.remove(&ticket)
        else {
            return;
        };
        let frames = result.unwrap_or_default();
        let Some(session) = self.sessions.get(&id) else { return };
        let current = session.status == SessionStatus::Paused
            && session.selected_thread == Some(thread)
            && session.frames_epoch == epoch;
        if !current {
            return;
        }
        let at_breakpoint = session.stop_reason.as_deref().is_some_and(is_breakpoint_stop);
        self.patch(id, |s| s.set_frames(frames));
        let top = self.sessions.get(&id).and_then(|s| s.top_source_frame().or(s.frames.first())).cloned();
        match top {
            Some(top) => {
                if let Some(path) = frame_path(&top).filter(|_| at_breakpoint) {
                    self.log_suspending_hit(id, &top, &path, ctx);
                    ctx.breakpoints.lines.consume_temporary(&path, top.line, ctx.env.now_ms);
                    self.drain(ctx.breakpoints);
                }
                self.select_frame(id, top.id, reveal);
            }
            None => self.update_execution_point(),
        }
        if show_execution_point {
            self.show_top_frame(id);
        }
    }

    /// A suspending breakpoint that also logs: DAP logpoints never suspend, so the message is
    /// evaluated in the stopped frame and printed here instead.
    fn log_suspending_hit(&mut self, id: DebugSessionId, frame: &StackFrame, path: &str, ctx: &mut Ctx) {
        let hit = ctx.breakpoints.lines.breakpoints_of(path).iter().find(|b| b.line == frame.line && b.enabled);
        let Some(breakpoint) = hit.filter(|b| b.suspend) else { return };
        let parts: Vec<LogPart> = log_parts(breakpoint, path)
            .into_iter()
            .map(|part| {
                let slots = vec![None; log_expressions(&part).len()];
                (part, slots)
            })
            .collect();
        if parts.is_empty() {
            return;
        }
        self.tickets += 1;
        let log = self.tickets;
        for (part, (template, _)) in parts.iter().enumerate() {
            for (slot, expression) in log_expressions(template).into_iter().enumerate() {
                let context = EvaluateContext::Repl;
                self.ask(
                    BackendCall::DebugEvaluateIn { session: id, frame: Some(frame.id), expression, context },
                    Pending::LogValue { log, part, slot },
                );
            }
        }
        self.hit_logs.insert(log, HitLog { session: id, parts });
        self.print_hit_log(log, ctx.env.console);
    }

    /// Prints the lines of a hit once every expression of them has its value.
    fn print_hit_log(&mut self, log: u64, settings: &ConsoleSettings) {
        let complete = |hit: &HitLog| hit.parts.iter().all(|(_, values)| values.iter().all(Option::is_some));
        if !self.hit_logs.get(&log).is_some_and(complete) {
            return;
        }
        let Some(hit) = self.hit_logs.remove(&log) else { return };
        for (template, values) in hit.parts {
            let values: Vec<Result<String, String>> = values.into_iter().flatten().collect();
            let line = format!("{}\n", render_log(&template, &values));
            self.append_console(hit.session, "console", &line, settings);
        }
    }

    /// An expression of a breakpoint's log message was evaluated.
    pub fn log_value_evaluated(&mut self, ticket: u64, result: Result<Variable, IdeError>, settings: &ConsoleSettings) {
        let Some(Pending::LogValue { log, part, slot }) = self.pending.remove(&ticket) else { return };
        let value = result.map(|variable| variable.value).map_err(|error| error.message);
        if let Some(place) = self.hit_logs.get_mut(&log).and_then(|hit| hit.parts.get_mut(part)?.1.get_mut(slot)) {
            *place = Some(value);
        }
        self.print_hit_log(log, settings);
    }

    fn load_threads(&mut self, id: DebugSessionId) {
        self.ask(BackendCall::DebugThreads(id), Pending::Threads(id));
    }

    pub fn threads_loaded(&mut self, ticket: u64, result: Result<Vec<ThreadInfo>, IdeError>) {
        let Some(Pending::Threads(id)) = self.pending.remove(&ticket) else { return };
        if let Ok(threads) = result {
            self.patch(id, |s| s.threads = threads);
        }
    }

    /// Asks for the locals of `frame_id` (first non-expensive scope), for inline values.
    fn load_locals(&mut self, id: DebugSessionId, frame_id: i64) {
        let Some(epoch) = self.sessions.get(&id).map(|s| s.frames_epoch) else { return };
        self.ask(
            BackendCall::DebugScopes { session: id, frame: frame_id },
            Pending::Scopes { session: id, frame: frame_id, epoch },
        );
    }

    pub fn scopes_loaded(&mut self, ticket: u64, result: Result<Vec<Scope>, IdeError>) {
        let Some(Pending::Scopes { session, frame, epoch }) = self.pending.remove(&ticket) else { return };
        let scope = result.ok().and_then(|scopes| scopes.into_iter().find(|scope| !scope.expensive));
        match scope {
            Some(scope) => self.ask(
                BackendCall::DebugVariables { session, reference: scope.variables_reference },
                Pending::Variables { session, frame, epoch },
            ),
            None => self.apply_locals(session, frame, epoch, BTreeMap::new()),
        }
    }

    pub fn variables_loaded(&mut self, ticket: u64, result: Result<Vec<Variable>, IdeError>) {
        let Some(Pending::Variables { session, frame, epoch }) = self.pending.remove(&ticket) else { return };
        let mut locals = BTreeMap::new();
        for variable in result.unwrap_or_default() {
            if !variable.name.is_empty() {
                locals.entry(variable.name.clone()).or_insert(variable);
            }
        }
        self.apply_locals(session, frame, epoch, locals);
    }

    /// Takes the locals of a frame, unless the session has moved on since they were asked for
    /// (also when a later stop reuses the frame id).
    fn apply_locals(&mut self, id: DebugSessionId, frame: i64, epoch: u64, locals: BTreeMap<String, Variable>) {
        let current = self.active == Some(id)
            && self.sessions.get(&id).is_some_and(|s| {
                s.status == SessionStatus::Paused && s.selected_frame == Some(frame) && s.frames_epoch == epoch
            });
        if current {
            self.execution_point.locals = locals;
        }
    }

    /// Show Execution Point: back to the suspended thread's top frame, revealed and focused.
    pub fn show_execution_point(&mut self, id: DebugSessionId) {
        let Some(session) = self.sessions.get(&id).filter(|s| s.status == SessionStatus::Paused) else { return };
        let Some(stopped) = session.stopped_thread else { return };
        if session.selected_thread != Some(stopped) {
            self.patch(id, |s| {
                s.selected_thread = Some(stopped);
                s.set_frames(Vec::new());
                s.selected_frame = None;
            });
            self.update_execution_point();
            self.load_frames(id, stopped, false, true);
            return;
        }
        self.show_top_frame(id);
    }

    fn show_top_frame(&mut self, id: DebugSessionId) {
        let Some(top) = self.sessions.get(&id).and_then(|s| s.top_source_frame()).cloned() else { return };
        self.select_frame(id, top.id, false);
        let opened = Self::reveal_frame(&top, true);
        self.effects.extend(opened);
    }

    /// Brings the execution point in line with the active session's selected frame.
    pub fn update_execution_point(&mut self) {
        let place = self.active_session().filter(|s| s.status == SessionStatus::Paused).and_then(|s| {
            let frame = s.frames.iter().find(|f| Some(f.id) == s.selected_frame)?;
            let top = s.top_source_frame().is_some_and(|top| top.id == frame.id);
            Some((frame_path(frame)?, frame.line, top))
        });
        let point = &mut self.execution_point;
        match place {
            None if point.path.is_none() => {}
            None => {
                point.path = None;
                point.locals.clear();
                self.execution_point_rev += 1;
            }
            Some((path, line, top)) => {
                if point.path.as_deref() == Some(path.as_str()) && point.line == line && point.top == top {
                    return;
                }
                *point = ExecutionPoint { path: Some(path), line, top, locals: BTreeMap::new() };
                self.execution_point_rev += 1;
            }
        }
    }

    /// Sends a command that resumes execution, with the suspended data invalidated first. Returns
    /// the ticket of the command, whose answer goes to [`DebugState::resumed`]; `None`, sending
    /// nothing, when the session is not suspended.
    pub fn resume_execution(&mut self, id: DebugSessionId, call: BackendCall) -> Option<u64> {
        let paused = |s: &&DebugSession| s.status == SessionStatus::Paused;
        let session = self.sessions.get(&id).filter(paused)?;
        let before = Suspended {
            status: session.status,
            frames: session.frames.clone(),
            selected_frame: session.selected_frame,
            stopped_thread: session.stopped_thread,
        };
        self.patch(id, |s| {
            s.status = SessionStatus::Running;
            s.set_frames(Vec::new());
            s.selected_frame = None;
            s.stopped_thread = None;
        });
        self.update_execution_point();
        let epoch = self.sessions.get(&id).map_or(0, |s| s.frames_epoch);
        self.ask(call, Pending::Resume { session: id, before: Box::new(before), epoch });
        Some(self.tickets)
    }

    /// A resuming command was answered. A failure puts the suspended data back, unless a stop,
    /// continue or termination received meanwhile owns the newer state, and is returned.
    pub fn resumed(&mut self, ticket: u64, result: Result<(), IdeError>) -> Option<ResumeFailure> {
        let Some(Pending::Resume { session: id, before, epoch }) = self.pending.remove(&ticket) else {
            return None;
        };
        let error = result.err()?;
        let untouched = |s: &DebugSession| s.status == SessionStatus::Running && s.frames_epoch == epoch;
        if self.sessions.get(&id).is_some_and(untouched) {
            let Suspended { status, frames, selected_frame, stopped_thread } = *before;
            self.patch(id, |s| {
                s.status = status;
                s.set_frames(frames);
                s.selected_frame = selected_frame;
                s.stopped_thread = stopped_thread;
            });
            self.update_execution_point();
            if let Some(frame) = selected_frame {
                self.load_locals(id, frame);
            }
        }
        Some(ResumeFailure { session: id, error })
    }

    // --- watches ----------------------------------------------------------------

    fn persist_watches(&mut self) {
        let value = serde_json::to_string(&self.watches).unwrap_or_default();
        self.effects.push(Effect::StorageSet { key: WATCH_KEY.to_owned(), value });
    }

    pub fn add_watch(&mut self, expression: &str) {
        let expression = expression.trim();
        if expression.is_empty() {
            return;
        }
        if !self.watches.iter().any(|watch| watch == expression) {
            self.watches.push(expression.to_owned());
        }
        self.persist_watches();
    }

    pub fn remove_watch(&mut self, expression: &str) {
        self.watches.retain(|watch| watch != expression);
        self.persist_watches();
    }

    /// Takes the watches of `stored` (the value under [`WATCH_KEY`]): the strings of a JSON array.
    pub fn load_watches(&mut self, stored: Option<&str>) {
        let Some(stored) = stored else {
            self.watches.clear();
            return;
        };
        if let Ok(Value::Array(items)) = serde_json::from_str::<Value>(stored) {
            self.watches = items.iter().filter_map(Value::as_str).map(str::to_owned).collect();
        }
    }

    /// "Set Value" in the variables tree: `setVariable` in `container`, or `setExpression` on the
    /// evaluate name, in the suspended session `id`. The answer goes to [`DebugState::variable_set`].
    pub fn set_variable_value(
        &mut self,
        id: DebugSessionId,
        container: i64,
        name: &str,
        evaluate_name: Option<&str>,
        value: &str,
    ) -> Result<(), &'static str> {
        let session = self.sessions.get(&id).filter(|s| s.status == SessionStatus::Paused);
        let frame = session.ok_or("The debugger is not suspended")?.selected_frame;
        let call = BackendCall::DebugSetVariable {
            session: id,
            container,
            name: name.to_owned(),
            evaluate_name: evaluate_name.map(str::to_owned),
            frame,
            value: value.to_owned(),
        };
        self.ask(call, Pending::SetVariable { session: id, frame });
        Ok(())
    }

    /// A value was set: variables, watches and inline values are read again.
    pub fn variable_set(&mut self, ticket: u64, result: Result<Variable, IdeError>) -> Result<Variable, IdeError> {
        let Some(Pending::SetVariable { session, frame }) = self.pending.remove(&ticket) else { return result };
        if result.is_ok() {
            self.stop_seq += 1;
            if let Some(frame) = frame {
                self.load_locals(session, frame);
            }
        }
        result
    }

    /// The command that types `text` into the session's program; its terminal echoes it to the
    /// console.
    pub fn send_program_input(session: DebugSessionId, text: &str) -> BackendCall {
        BackendCall::DebugSendInput { session, text: text.to_owned() }
    }

    /// The command that evaluates `expression` in the selected frame of `session` (the active
    /// session when `None`); the caller shows its answer.
    pub fn evaluate(
        &self,
        expression: &str,
        context: EvaluateContext,
        session: Option<DebugSessionId>,
    ) -> Result<BackendCall, &'static str> {
        let session = session.or(self.active).and_then(|id| self.sessions.get(&id)).filter(|s| s.is_live());
        let session = session.ok_or("No active debug session")?;
        Ok(BackendCall::DebugEvaluateIn {
            session: session.id,
            frame: session.selected_frame,
            expression: expression.to_owned(),
            context,
        })
    }

    // --- events -----------------------------------------------------------------

    /// A debug event of session `id` arrived. Events of sessions this window does not own are
    /// dropped. (Hot code replace events are the hot swap model's.)
    pub fn handle_debug_event(&mut self, id: DebugSessionId, event: DebugEvent, ctx: &mut Ctx) {
        if !self.own_session(id, &event, ctx.env.now_ms) {
            return;
        }
        match event {
            DebugEvent::Started { name, adapter, parent } => {
                if self.sessions.get(&id).is_some_and(|s| s.launch.is_none()) {
                    self.patch(id, |s| {
                        s.name = name;
                        s.adapter = adapter;
                        s.parent = parent;
                    });
                }
            }
            DebugEvent::Initialized => {
                if self.sessions.get(&id).is_some_and(|s| s.status == SessionStatus::Starting) {
                    self.patch(id, |s| s.status = SessionStatus::Running);
                }
            }
            DebugEvent::Stopped { thread_id, reason, description, .. } => {
                let at_breakpoint = is_breakpoint_stop(&reason);
                self.patch(id, |s| {
                    s.status = SessionStatus::Paused;
                    s.stopped_thread = Some(thread_id);
                    s.selected_thread = Some(thread_id);
                    s.stop_reason = Some(reason);
                    s.stop_description = description;
                    s.set_frames(Vec::new());
                    s.selected_frame = None;
                    s.hits += u32::from(at_breakpoint);
                });
                self.active = Some(id);
                self.stop_seq += 1;
                self.publish_context();
                self.update_execution_point();
                self.load_threads(id);
                self.load_frames(id, thread_id, true, false);
                if ctx.env.debugger.show_on_breakpoint {
                    self.effects.push(Effect::ShowToolWindow { focus: false });
                }
                // Attention for a background window: focusing the app and flashing the taskbar are
                // alternatives (IDEA), so a window configured to grab focus does not also flash.
                if ctx.env.debugger.focus_app_on_breakpoint {
                    if ctx.env.desktop {
                        self.effects.push(Effect::FocusAppWindow);
                    }
                } else if at_breakpoint {
                    let flash = self.taskbar.flash_for_breakpoint(ctx.env.desktop, ctx.env.window_focused);
                    self.call(flash);
                }
            }
            DebugEvent::Continued { thread_id, all_threads_continued } => {
                let Some(session) = self.sessions.get(&id).filter(|s| s.is_live()) else { return };
                if !all_threads_continued && Some(thread_id) != session.selected_thread.or(session.stopped_thread) {
                    return;
                }
                let stop_flash = self.taskbar.stop(ctx.env.desktop);
                self.call(stop_flash);
                self.patch(id, |s| {
                    s.status = SessionStatus::Running;
                    s.set_frames(Vec::new());
                    s.selected_frame = None;
                    s.stopped_thread = None;
                });
                self.update_execution_point();
            }
            DebugEvent::Output { category, output } => {
                self.append_console_queued(id, &category, &output, ctx.env.now_ms);
            }
            DebugEvent::Thread { .. } => {
                if self.sessions.get(&id).is_some_and(|s| s.status == SessionStatus::Paused) {
                    self.load_threads(id);
                }
            }
            DebugEvent::ProgramInput => self.patch(id, |s| s.program_input = true),
            DebugEvent::BreakpointsChanged { path } => {
                if ctx.env.desktop {
                    ctx.breakpoints.lines.refresh_states(&ide_core::ipc_path::to_ipc(&path));
                    self.drain(ctx.breakpoints);
                }
            }
            DebugEvent::Exited { code } => {
                self.patch(id, |s| s.exit_code = Some(code));
                let finished = format!("\nProcess finished with exit code {code}\n");
                self.append_console(id, "system", &finished, ctx.env.console);
            }
            DebugEvent::Terminated => self.mark_terminated(id, ctx),
            DebugEvent::HotCodeReplace { .. } => {}
        }
    }

    /// The window got the keyboard focus: a taskbar flash it started stops.
    pub fn window_focused(&mut self, desktop: bool) {
        let stop_flash = self.taskbar.stop(desktop);
        self.call(stop_flash);
    }

    /// The shell could not flash the taskbar: the next breakpoint hit asks again.
    pub fn taskbar_flash_failed(&mut self) {
        self.taskbar.flash_failed();
    }

    /// The window switched projects: stops its sessions and forgets them (late events stay ignored).
    pub fn reset_sessions(&mut self, ctx: &mut Ctx) {
        let roots: Vec<DebugSessionId> =
            self.sessions.values().filter(|s| s.parent.is_none() && s.is_live()).map(|s| s.id).collect();
        for root in roots {
            self.stop_session(root, true, ctx);
        }
        self.pending_output.clear();
        self.output_flush_at = None;
        self.ansi_streams.clear();
        self.announced_ports.clear();
        self.sessions.clear();
        self.active = None;
        self.console.clear();
        self.stop_seq = 0;
        if self.execution_point.path.is_some() {
            self.execution_point_rev += 1;
        }
        self.execution_point = ExecutionPoint { top: true, ..Default::default() };
        self.publish_context();
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::debugger::effects::ipc;
    use crate::debugger::effects::testing::Settings;

    /// What the backend of a test answers a command with.
    #[derive(Debug, Clone)]
    pub enum Reply {
        /// Nothing yet: the command stays in [`World::deferred`] until [`World::resolve`].
        Later,
        /// Success, with nothing in it: an empty list, default capabilities, session 42 for a start.
        Done,
        Fail(IdeError),
        Frames(Vec<StackFrame>),
        Threads(Vec<ThreadInfo>),
        Scopes(Vec<Scope>),
        Variables(Vec<Variable>),
        Session(u32),
        Value(Variable),
        Caps(DebugCapabilities),
    }

    /// A debugger with its surroundings: breakpoints, settings, a clock and a scripted backend.
    pub struct World {
        pub state: DebugState,
        pub breakpoints: Breakpoints,
        pub settings: Settings,
        pub i18n: I18n,
        pub now: f64,
        pub desktop: bool,
        pub focused: bool,
        pub run_stop_action: bool,
        /// Every backend command made so far, in order.
        pub calls: Vec<BackendCall>,
        /// Every other effect so far, in order.
        pub effects: Vec<Effect>,
        /// Commands the backend has not answered yet.
        pub deferred: Vec<(Option<u64>, BackendCall)>,
        /// How the starts ended, in order.
        pub outcomes: Vec<StartOutcome>,
        pub resume_failures: Vec<ResumeFailure>,
    }

    impl Default for World {
        fn default() -> Self {
            Self {
                state: DebugState::default(),
                breakpoints: Breakpoints::default(),
                settings: Settings::default(),
                i18n: I18n::default(),
                now: 1_000_000.0,
                desktop: true,
                focused: true,
                run_stop_action: false,
                calls: Vec::new(),
                effects: Vec::new(),
                deferred: Vec::new(),
                outcomes: Vec::new(),
                resume_failures: Vec::new(),
            }
        }
    }

    /// The state and what its transitions work with, borrowed from the fields of a [`World`].
    macro_rules! parts {
        ($world:expr) => {{
            let world = &mut *$world;
            let env = Env {
                desktop: world.desktop,
                window_focused: world.focused,
                run_stop_action: world.run_stop_action,
                ..world.settings.env(world.now)
            };
            (&mut world.state, Ctx { breakpoints: &mut world.breakpoints, env, i18n: &mut world.i18n })
        }};
    }
    pub(crate) use parts;

    impl World {
        /// Routes the answer of a command to the continuation that takes it.
        fn deliver(&mut self, ticket: Option<u64>, call: BackendCall, reply: Reply) {
            if matches!(reply, Reply::Later) {
                self.deferred.push((ticket, call));
                return;
            }
            let failed = match &reply {
                Reply::Fail(error) => Some(error.clone()),
                _ => None,
            };
            let unit = || failed.clone().map_or(Ok(()), Err);
            let (state, mut ctx) = parts!(self);
            match (call, ticket) {
                (BackendCall::DebugStackTrace { .. }, Some(ticket)) => {
                    let frames = if let Reply::Frames(frames) = reply { frames } else { Vec::new() };
                    state.frames_loaded(ticket, failed.map_or(Ok(frames), Err), &mut ctx);
                }
                (BackendCall::DebugThreads(_), Some(ticket)) => {
                    let threads = if let Reply::Threads(threads) = reply { threads } else { Vec::new() };
                    state.threads_loaded(ticket, failed.map_or(Ok(threads), Err));
                }
                (BackendCall::DebugScopes { .. }, Some(ticket)) => {
                    let locals = Scope { name: "Locals".to_owned(), variables_reference: 10, expensive: false };
                    let scopes = if let Reply::Scopes(scopes) = reply { scopes } else { vec![locals] };
                    state.scopes_loaded(ticket, failed.map_or(Ok(scopes), Err));
                }
                (BackendCall::DebugVariables { .. }, Some(ticket)) => {
                    let variables = if let Reply::Variables(variables) = reply { variables } else { Vec::new() };
                    state.variables_loaded(ticket, failed.map_or(Ok(variables), Err));
                }
                (BackendCall::DebugCapabilities(_), Some(ticket)) => {
                    let caps = if let Reply::Caps(caps) = reply { caps } else { DebugCapabilities::default() };
                    state.capabilities_loaded(ticket, failed.map_or(Ok(caps), Err));
                }
                (BackendCall::DebugStart(_), Some(ticket)) => {
                    let session = if let Reply::Session(session) = reply { session } else { 42 };
                    let outcome = state.started(ticket, failed.map_or(Ok(DebugSessionId(session)), Err), &mut ctx);
                    self.outcomes.extend(outcome);
                }
                (BackendCall::DebugStop(_), Some(ticket)) => state.stop_finished(ticket, unit(), &mut ctx),
                (BackendCall::DebugEvaluateIn { expression, .. }, Some(ticket)) => {
                    let value = if let Reply::Value(value) = reply { value } else { variable(&expression, "") };
                    state.log_value_evaluated(ticket, failed.map_or(Ok(value), Err), ctx.env.console);
                }
                (BackendCall::DebugSetBreakpoints { path, .. }, None) => {
                    let outcomes = state.breakpoints_synced(&path, failed.map_or(Ok(Vec::new()), Err), &mut ctx);
                    self.outcomes.extend(outcomes);
                }
                (
                    BackendCall::DebugStep { .. }
                    | BackendCall::DebugResume { .. }
                    | BackendCall::DebugForceStepOver { .. }
                    | BackendCall::DebugRunTo { .. }
                    | BackendCall::DebugStepInTarget { .. }
                    | BackendCall::DebugRestartFrame { .. },
                    Some(ticket),
                ) => {
                    let failure = state.resumed(ticket, unit());
                    self.resume_failures.extend(failure);
                }
                _ => {}
            }
        }

        /// Carries out what the debugger asked for until it asks for nothing more: `backend`
        /// answers the commands, everything is recorded.
        pub fn pump(&mut self, backend: &mut dyn FnMut(&BackendCall) -> Reply) {
            loop {
                let mut effects = self.state.take_effects();
                effects.extend(self.breakpoints.take_effects());
                if effects.is_empty() {
                    return;
                }
                for effect in effects {
                    match effect {
                        Effect::Ask { ticket, call } => {
                            self.calls.push(call.clone());
                            let reply = backend(&call);
                            self.deliver(Some(ticket), call, reply);
                        }
                        Effect::Call(call) => {
                            self.calls.push(call.clone());
                            let reply = backend(&call);
                            self.deliver(None, call, reply);
                        }
                        other => self.effects.push(other),
                    }
                }
            }
        }

        /// [`World::pump`] with a backend that answers everything with [`Reply::Done`].
        pub fn settle(&mut self) {
            self.pump(&mut |_| Reply::Done);
        }

        /// Answers the first unanswered command `wanted` accepts.
        pub fn resolve(&mut self, wanted: impl Fn(&BackendCall) -> bool, reply: Reply) {
            let at = self.deferred.iter().position(|(_, call)| wanted(call)).expect("an unanswered command");
            let (ticket, call) = self.deferred.remove(at);
            self.deliver(ticket, call, reply);
        }

        pub fn adopt(&mut self, id: u32, init: impl FnOnce(&mut DebugSession)) {
            self.state.adopt_session(DebugSessionId(id), self.now, init);
        }

        pub fn event(&mut self, id: u32, event: DebugEvent) {
            let (state, mut ctx) = parts!(self);
            state.handle_debug_event(DebugSessionId(id), event, &mut ctx);
        }

        pub fn stopped(&mut self, id: u32, thread_id: i64, reason: &str) {
            let reason = reason.to_owned();
            self.event(id, DebugEvent::Stopped { thread_id, reason, description: None, all_threads_stopped: true });
        }

        pub fn start(&mut self, launch: LaunchConfig, options: StartOptions) -> Option<u64> {
            let (state, mut ctx) = parts!(self);
            state.start_session(launch, options, &mut ctx)
        }

        pub fn stop(&mut self, id: u32, stop_process: bool) {
            let (state, mut ctx) = parts!(self);
            state.stop_session(DebugSessionId(id), stop_process, &mut ctx);
        }

        pub fn session(&self, id: u32) -> &DebugSession {
            self.state.session(DebugSessionId(id)).expect("the session")
        }

        pub fn console_text(&self, id: DebugSessionId) -> Vec<&str> {
            self.state.console(id).iter().map(|line| line.text.as_str()).collect()
        }

        /// How many of the commands made so far `wanted` accepts.
        pub fn count(&self, wanted: impl Fn(&BackendCall) -> bool) -> usize {
            self.calls.iter().filter(|call| wanted(call)).count()
        }

        /// The titles of the notifications raised so far.
        pub fn notified(&self) -> Vec<&str> {
            self.effects.iter().filter_map(Effect::notification).map(|n| n.title.as_str()).collect()
        }
    }

    pub fn variable(name: &str, value: &str) -> Variable {
        Variable {
            name: name.to_owned(),
            value: value.to_owned(),
            type_name: None,
            variables_reference: 0,
            evaluate_name: Some(name.to_owned()),
        }
    }

    pub fn frame(id: i64, name: &str, path: &str, line: u32) -> StackFrame {
        StackFrame { id, name: name.to_owned(), path: Some(ipc(path)), line, column: 0, presentation_hint: None }
    }

    pub fn launch(name: &str, adapter: &str, request: &str) -> LaunchConfig {
        LaunchConfig {
            name: name.to_owned(),
            adapter: adapter.to_owned(),
            request: request.to_owned(),
            program: None,
            args: Vec::new(),
            cwd: Some(ipc("/p")),
            env: BTreeMap::new(),
            extra_json: None,
        }
    }

    pub fn is_start(call: &BackendCall) -> bool {
        matches!(call, BackendCall::DebugStart(_))
    }

    pub fn is_stack_trace(call: &BackendCall) -> bool {
        matches!(call, BackendCall::DebugStackTrace { .. })
    }

    pub fn is_set_breakpoints(call: &BackendCall) -> bool {
        matches!(call, BackendCall::DebugSetBreakpoints { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::testing::{Reply, World, frame, is_set_breakpoints, is_stack_trace, is_start, launch, parts, variable};
    use super::*;
    use crate::debugger::breakpoints::model::BreakpointProps;
    use crate::debugger::launch::testing::rc;
    use crate::run::ansi::{AnsiColor, SpanStyle};
    use std::collections::VecDeque;

    const PATH: &str = "/proj/MsMemberUtils.java";

    fn id(id: u32) -> DebugSessionId {
        DebugSessionId(id)
    }

    fn append(world: &mut World, session: u32, category: &str, raw: &str) {
        world.state.append_console(id(session), category, raw, &world.settings.console);
    }

    fn coloured(text: &str, color: u8) -> Span {
        Span { text: text.to_owned(), style: SpanStyle { fg: Some(AnsiColor::Named(color)), ..Default::default() } }
    }

    fn config() -> RunConfig {
        let mut config = rc("node", "node", &["server.js"], Some("js-debug"));
        config.id = "node:server".to_owned();
        config.name = "server".to_owned();
        config
    }

    fn java_launch(request: &str) -> LaunchConfig {
        launch("Java", "java-debug", request)
    }

    fn top(line: u32) -> Reply {
        Reply::Frames(vec![frame(100, "queryMsTicket", PATH, line)])
    }

    /// A world with session 1 adopted, as a Java debug session.
    fn java_session() -> World {
        let mut world = World::default();
        world.adopt(1, |s| s.adapter = "java-debug".to_owned());
        world.settle();
        world
    }

    /// Answers stack traces with `stacks` in turn (then with a frame on line 6840), variables with
    /// `variables` in turn, and everything else with nothing.
    fn pump(world: &mut World, stacks: &mut VecDeque<Reply>, variables: &mut VecDeque<Reply>) {
        world.pump(&mut |call| match call {
            BackendCall::DebugStackTrace { .. } => stacks.pop_front().unwrap_or_else(|| top(6840)),
            BackendCall::DebugVariables { .. } => variables.pop_front().unwrap_or(Reply::Done),
            BackendCall::DebugThreads(_) => Reply::Threads(vec![
                ThreadInfo { id: 5, name: "http-nio-8179-exec-5".to_owned() },
                ThreadInfo { id: 1, name: "main".to_owned() },
            ]),
            BackendCall::DebugStep { .. } | BackendCall::DebugResume { .. } => Reply::Later,
            _ => Reply::Done,
        });
    }

    /// A stop of session 1 on `thread`, with the pulls it starts answered: the stack by `stack`.
    fn stop_on(world: &mut World, thread: i64, all: bool, stack: Reply) {
        let event = DebugEvent::Stopped {
            thread_id: thread,
            reason: "step".to_owned(),
            description: None,
            all_threads_stopped: all,
        };
        world.event(1, event);
        pump(world, &mut VecDeque::from([stack]), &mut VecDeque::new());
    }

    fn point(world: &World) -> (Option<&str>, u32) {
        let point = world.state.execution_point();
        (point.path.as_deref(), point.line)
    }

    // --- console.test.ts ------------------------------------------------------

    #[test]
    fn renders_ansi_colours_instead_of_showing_the_escapes() {
        let mut world = World::default();
        // A logback pattern whose colour spans the line break, with CRLF.
        let first = concat!(
            "\x1b[36m2026-10-29 09:13:55\x1b[0;39m \x1b[32m[main]\x1b[0;39m ",
            "\x1b[1;35mo.d.c.p.l.MessageTopicListener\r\n",
        );
        append(&mut world, 5, "stdout", first);
        append(&mut world, 5, "stdout", "\x1b[0;39m - 初始化消息主题订阅监听器成功\r\n");
        let lines = world.state.console(id(5));
        assert_eq!(lines[0].text, "2026-10-29 09:13:55 [main] o.d.c.p.l.MessageTopicListener\n");
        assert_eq!(lines[0].spans[0], coloured("2026-10-29 09:13:55", 6));
        assert!(lines[0].spans.contains(&coloured("[main]", 2)));
        let mut logger = coloured("o.d.c.p.l.MessageTopicListener\n", 5);
        logger.style.bold = true;
        assert_eq!(lines[0].spans.last(), Some(&logger));
        assert_eq!(lines[1].text, " - 初始化消息主题订阅监听器成功\n");
        assert_eq!(lines[1].spans, [Span::plain(" - 初始化消息主题订阅监听器成功\n")]);
        assert!(!lines.iter().any(|line| line.text.contains('\x1b')));
    }

    #[test]
    fn keeps_a_colour_across_output_events_and_an_escape_split_between_them() {
        let mut world = World::default();
        append(&mut world, 6, "stderr", "\x1b[3");
        append(&mut world, 6, "stderr", "1mred ");
        append(&mut world, 6, "stderr", "still\x1b[0m plain\n");
        let line = &world.state.console(id(6))[0];
        assert_eq!(line.text, "red still plain\n");
        assert_eq!(line.spans, [coloured("red still", 1), Span::plain(" plain\n")]);
        // Streams of other categories and sessions have their own state.
        append(&mut world, 6, "stdout", "\x1b[32mgreen");
        append(&mut world, 7, "stdout", "plain\n");
        assert_eq!(world.state.console(id(7))[0].spans, [Span::plain("plain\n")]);
        let console = world.settings.console.clone();
        world.state.clear_console(id(6), &console);
        append(&mut world, 6, "stdout", "after clear\n");
        assert_eq!(world.state.console(id(6))[0].spans, [Span::plain("after clear\n")]);
    }

    #[test]
    fn splits_styled_chunks_into_physical_lines_and_keeps_partial_line_identity() {
        let mut world = World::default();
        append(&mut world, 10, "stdout", "prefix ");
        let first = world.state.console(id(10))[0].id;
        append(&mut world, 10, "stdout", "\x1b[31mred\n\nlast\x1b[0m");
        assert_eq!(world.console_text(id(10)), ["prefix red\n", "\n", "last"]);
        assert_eq!(world.state.console(id(10))[0].id, first);
        assert_eq!(world.state.console(id(10))[1].spans, [coloured("\n", 1)]);
        append(&mut world, 10, "stderr", "error\n");
        append(&mut world, 10, "stdout", "next\n");
        let categories = world.state.console(id(10)).iter().map(|line| line.category.as_str());
        assert_eq!(categories.collect::<Vec<_>>(), ["stdout", "stdout", "stdout", "stderr", "stdout"]);
    }

    #[test]
    fn keeps_per_frame_batching_while_bounding_one_event_containing_60k_lines() {
        let mut world = World::default();
        world.adopt(11, |_| {});
        world.state.append_console_queued(id(11), "stdout", &"line\n".repeat(60_000), world.now);
        world.state.append_console_queued(id(11), "stdout", "tail\n", world.now + 5.0);
        assert!(world.state.console(id(11)).is_empty());
        assert_eq!(world.state.next_output_flush(), Some(world.now + 100.0));
        let console = world.settings.console.clone();
        world.state.flush_console_output(&console);
        assert_eq!(world.state.next_output_flush(), None);
        let lines = world.state.console(id(11));
        assert!(lines.len() <= 50_000 && lines.len() > 40_000);
        assert_eq!(lines.last().map(|line| line.text.as_str()), Some("tail\n"));
        assert!(lines.iter().all(|line| line.text == "line\n" || line.text == "tail\n"));
        let ids: HashSet<u64> = lines.iter().map(|line| line.id).collect();
        assert_eq!(ids.len(), lines.len());
    }

    #[test]
    fn trims_a_multiline_event_by_the_configured_character_buffer() {
        let mut world = World::default();
        world.settings.console.override_buffer = true;
        world.settings.console.buffer_kb = 8;
        append(&mut world, 12, "stdout", &format!("{}\n", "x".repeat(4000)).repeat(6000));
        let output = world.state.console(id(12));
        assert!(!output.is_empty());
        assert!(output.iter().map(|line| line.text.len() + 1).sum::<usize>() <= 8 * 1024);
        assert!(output.iter().all(|line| line.text.len() == 4001));
    }

    #[test]
    fn flushes_output_while_an_unfocused_window_draws_no_frames() {
        let mut world = World::default();
        world.adopt(13, |_| {});
        world.state.append_console_queued(id(13), "stdout", "Spring Boot starting…\n", world.now);
        assert!(world.state.console(id(13)).is_empty());
        // No frame comes: the deadline is what shows the output.
        let deadline = world.state.next_output_flush().expect("a flush deadline");
        assert_eq!(deadline, world.now + 100.0);
        let console = world.settings.console.clone();
        world.state.flush_console_output(&console);
        assert_eq!(world.console_text(id(13)), ["Spring Boot starting…\n"]);
        assert_eq!(world.session(13).status, SessionStatus::Running);
    }

    // --- editor.test.ts › session events ------------------------------------------

    #[test]
    fn tracks_pause_continue_termination_and_publishes_context_keys() {
        let mut world = World::default();
        world.adopt(77, |_| {});
        world.event(77, DebugEvent::Output { category: "stdout".to_owned(), output: "hel".to_owned() });
        world.event(77, DebugEvent::Output { category: "stdout".to_owned(), output: "lo\n".to_owned() });
        let console = world.settings.console.clone();
        world.state.flush_console_output(&console);
        assert_eq!(world.console_text(id(77)), ["hello\n"]);
        assert!(world.state.published().context.session_active);
        world.stopped(77, 1, "breakpoint");
        assert_eq!((world.session(77).status, world.session(77).hits), (SessionStatus::Paused, 1));
        assert!(world.state.published().context.session_paused);
        world.event(77, DebugEvent::Continued { thread_id: 1, all_threads_continued: true });
        assert_eq!(world.session(77).status, SessionStatus::Running);
        assert!(!world.state.published().context.session_paused);
        world.event(77, DebugEvent::Exited { code: 0 });
        world.event(77, DebugEvent::Terminated);
        assert_eq!(world.session(77).status, SessionStatus::Terminated);
        assert_eq!(world.session(77).exit_code, Some(0));
        assert!(world.console_text(id(77)).contains(&"Process finished with exit code 0\n"));
        assert!(!world.state.published().context.session_active);
    }

    #[test]
    fn ignores_sessions_this_window_did_not_start_but_adopts_children_of_its_own() {
        let mut world = World::default();
        world.adopt(90, |s| {
            s.name = "app".to_owned();
            s.adapter = "js-debug".to_owned();
        });
        let started = |name: &str, adapter: &str, parent: Option<u32>| DebugEvent::Started {
            name: name.to_owned(),
            adapter: adapter.to_owned(),
            parent: parent.map(DebugSessionId),
        };
        let output = |text: &str| DebugEvent::Output { category: "stdout".to_owned(), output: text.to_owned() };
        // Another window's session: no session, no console, no tool window.
        world.event(91, started("other", "debugpy", None));
        world.stopped(91, 1, "breakpoint");
        world.event(91, output("x\n"));
        assert!(world.state.session(id(91)).is_none());
        // A child of someone else's session is not ours either.
        world.event(93, started("child", "debugpy", Some(91)));
        assert!(world.state.session(id(93)).is_none());
        // A child the adapter of our session started is adopted with its parent.
        world.event(92, started("worker", "js-debug", Some(90)));
        world.event(92, output("child\n"));
        let console = world.settings.console.clone();
        world.state.flush_console_output(&console);
        let child = world.session(92);
        assert_eq!((child.name.as_str(), child.parent), ("worker", Some(id(90))));
        assert_eq!(child.status, SessionStatus::Running);
        assert_eq!(world.console_text(id(92)), ["child\n"]);
        assert!(world.state.console(id(91)).is_empty());
        world.event(92, DebugEvent::Terminated);
        world.event(90, DebugEvent::Terminated);
        assert!(!world.state.published().context.session_active);
        world.settle();
        assert!(!world.effects.iter().any(|effect| matches!(effect, Effect::ShowToolWindow { .. })));
    }

    // --- executionState.test.ts -----------------------------------------------------

    #[test]
    fn selects_each_stopped_thread_and_its_fresh_top_frame() {
        for all in [false, true] {
            let mut world = java_session();
            stop_on(&mut world, 5, all, top(6840));
            assert_eq!(point(&world), (Some(PATH), 6840));
            stop_on(&mut world, 5, all, top(6842));
            assert_eq!((world.session(1).selected_thread, world.session(1).selected_frame), (Some(5), Some(100)));
            assert_eq!(point(&world), (Some(PATH), 6842));
            stop_on(&mut world, 1, all, top(9));
            assert_eq!(world.session(1).selected_thread, Some(1));
            assert_eq!(point(&world), (Some(PATH), 9));
            assert_eq!(world.session(1).threads.len(), 2);
        }
    }

    #[test]
    fn clears_the_old_point_while_a_new_stack_or_selected_thread_is_loading() {
        let mut world = java_session();
        stop_on(&mut world, 5, false, top(6840));
        stop_on(&mut world, 5, false, Reply::Later);
        assert_eq!(point(&world).0, None);
        world.resolve(is_stack_trace, top(6842));
        world.settle();
        world.state.select_thread(id(1), 1);
        pump(&mut world, &mut VecDeque::from([Reply::Later]), &mut VecDeque::new());
        assert_eq!(point(&world).0, None);
        world.resolve(is_stack_trace, top(9));
        world.settle();
        assert_eq!(point(&world), (Some(PATH), 9));
    }

    #[test]
    fn ignores_a_stack_response_from_an_earlier_stop_on_the_same_thread() {
        let mut world = java_session();
        stop_on(&mut world, 5, false, Reply::Later);
        stop_on(&mut world, 5, false, top(6842));
        world.resolve(is_stack_trace, top(6840));
        world.settle();
        assert_eq!(point(&world), (Some(PATH), 6842));
        assert_eq!(world.session(1).frames[0].line, 6842);
    }

    #[test]
    fn keeps_the_selected_paused_thread_when_only_another_thread_continues() {
        let mut world = java_session();
        stop_on(&mut world, 5, false, top(6840));
        world.event(1, DebugEvent::Continued { thread_id: 1, all_threads_continued: false });
        assert_eq!(point(&world).0, Some(PATH));
        assert_eq!(world.session(1).status, SessionStatus::Paused);
        world.event(1, DebugEvent::Continued { thread_id: 5, all_threads_continued: false });
        assert_eq!(point(&world).0, None);
    }

    #[test]
    fn clears_the_execution_point_when_continued_or_terminated() {
        let events = [DebugEvent::Continued { thread_id: 5, all_threads_continued: true }, DebugEvent::Terminated];
        for event in events {
            let mut world = java_session();
            let locals = Reply::Variables(vec![variable("count", "40")]);
            world.event(
                1,
                DebugEvent::Stopped {
                    thread_id: 5,
                    reason: "step".to_owned(),
                    description: None,
                    all_threads_stopped: false,
                },
            );
            pump(&mut world, &mut VecDeque::new(), &mut VecDeque::from([locals]));
            assert_eq!(world.state.execution_point().locals.len(), 1);
            world.event(1, event);
            assert_eq!(point(&world).0, None);
            assert!(world.state.execution_point().locals.is_empty());
        }
    }

    #[test]
    fn retains_locals_whatever_their_names() {
        let names = ["constructor", "hasOwnProperty", "toString", "valueOf", "__proto__", "isPrototypeOf"];
        let mut world = java_session();
        world.stopped(1, 5, "step");
        let locals = Reply::Variables(names.iter().map(|name| variable(name, "known")).collect());
        pump(&mut world, &mut VecDeque::new(), &mut VecDeque::from([locals]));
        for name in names {
            assert!(world.state.execution_point().locals.contains_key(name), "{name}");
        }
    }

    #[test]
    fn clears_suspended_data_before_a_step_or_continue_request_resolves() {
        let requests = [
            BackendCall::DebugStep { session: id(1), thread: 5, kind: ide_dap::StepKind::Over },
            BackendCall::DebugResume { session: id(1), thread: 5 },
        ];
        for request in requests {
            let mut world = java_session();
            stop_on(&mut world, 5, false, top(6840));
            assert!(world.state.resume_execution(id(1), request).is_some());
            // What the backend sees when the request reaches it.
            assert_eq!(point(&world).0, None);
            let session = world.session(1);
            assert_eq!(session.status, SessionStatus::Running);
            assert!(session.frames.is_empty() && session.selected_frame.is_none());
            pump(&mut world, &mut VecDeque::new(), &mut VecDeque::new());
            assert_eq!(world.deferred.len(), 1);
            stop_on(&mut world, 5, false, top(6842));
            world.resolve(|call| !is_stack_trace(call), Reply::Done);
            world.settle();
            assert_eq!(point(&world), (Some(PATH), 6842));
            assert!(world.resume_failures.is_empty());
        }
    }

    #[test]
    fn restores_suspended_data_if_the_command_fails_without_a_newer_event() {
        let mut world = java_session();
        stop_on(&mut world, 5, false, top(6840));
        world.state.resume_execution(id(1), BackendCall::DebugResume { session: id(1), thread: 5 });
        world.pump(&mut |call| match call {
            BackendCall::DebugResume { .. } => Reply::Fail(IdeError::internal("rejected")),
            _ => Reply::Done,
        });
        assert_eq!(world.resume_failures[0].error.message, "rejected");
        assert_eq!(world.session(1).status, SessionStatus::Paused);
        assert_eq!(point(&world), (Some(PATH), 6840));
    }

    #[test]
    fn does_not_restore_an_older_stop_when_a_failed_command_has_already_stopped_again() {
        let mut world = java_session();
        stop_on(&mut world, 5, false, top(6840));
        world.state.resume_execution(id(1), BackendCall::DebugResume { session: id(1), thread: 5 });
        pump(&mut world, &mut VecDeque::new(), &mut VecDeque::new());
        stop_on(&mut world, 5, false, top(6842));
        let rejected = Reply::Fail(IdeError::internal("rejected"));
        world.resolve(|call| matches!(call, BackendCall::DebugResume { .. }), rejected);
        world.settle();
        assert_eq!(world.resume_failures.len(), 1);
        assert_eq!(point(&world), (Some(PATH), 6842));
    }

    #[test]
    fn discards_locals_from_an_earlier_stop_even_when_the_frame_id_is_reused() {
        let mut world = java_session();
        world.stopped(1, 5, "step");
        pump(&mut world, &mut VecDeque::new(), &mut VecDeque::from([Reply::Later]));
        world.stopped(1, 5, "step");
        let fresh = Reply::Variables(vec![variable("count", "42")]);
        pump(&mut world, &mut VecDeque::from([top(6842)]), &mut VecDeque::from([fresh]));
        let stale = Reply::Variables(vec![variable("count", "40")]);
        world.resolve(|call| matches!(call, BackendCall::DebugVariables { .. }), stale);
        world.settle();
        assert_eq!(world.state.execution_point().locals["count"].value, "42");
    }

    // --- relisten.test.ts ------------------------------------------------------------

    fn attach() -> LaunchConfig {
        LaunchConfig {
            extra_json: Some(json!({ "hostName": "127.0.0.1", "port": 35549 }).to_string()),
            ..launch("AppKt", "kotlin-debug-adapter", "attach")
        }
    }

    /// A finished session that debugged a JVM of run 7.
    fn finished(ended_ago_ms: f64) -> World {
        let mut world = World::default();
        world.adopt(1, |s| {
            s.name = "AppKt".to_owned();
            s.process = Some(RunId(7));
        });
        world.event(1, DebugEvent::Terminated);
        let ended = world.now - ended_ago_ms;
        world.state.session_mut(id(1)).expect("the finished session").ended_at = Some(ended);
        world.settle();
        world
    }

    /// An attach to the JVM of `process` that fails after the adapter's session announced itself.
    fn failed_attach(world: &mut World, process: u32) {
        let options = StartOptions { process: Some(RunId(process)), rerunnable: false, ..Default::default() };
        world.start(attach(), options);
        world.pump(&mut |call| if is_start(call) { Reply::Later } else { Reply::Done });
        let started =
            DebugEvent::Started { name: "AppKt".to_owned(), adapter: "kotlin-debug-adapter".to_owned(), parent: None };
        world.event(43, started);
        world.resolve(is_start, Reply::Fail(IdeError::internal("attach failed: Internal error.")));
        world.settle();
    }

    #[test]
    fn ignores_a_failed_attach_to_the_port_a_jvm_announced_as_its_debugger_detached() {
        let mut world = finished(100.0);
        failed_attach(&mut world, 7);
        assert_eq!(world.outcomes.last().map(|outcome| outcome.session), Some(None));
        assert!(world.notified().is_empty());
        assert_eq!(world.state.sessions().keys().copied().collect::<Vec<_>>(), [id(1)]);
        assert_eq!(world.state.active(), Some(id(1)));
    }

    #[test]
    fn reports_a_failed_attach_to_a_jvm_of_another_run_or_long_after_the_last_session_ended() {
        let mut world = finished(100.0);
        failed_attach(&mut world, 8);
        assert_eq!(world.notified(), ["Cannot start debugger for AppKt"]);

        let mut world = finished(60_000.0);
        failed_attach(&mut world, 7);
        assert_eq!(world.notified(), ["Cannot start debugger for AppKt"]);
        assert!(world.state.session(id(1)).is_none());
    }

    #[test]
    fn replaces_the_finished_session_once_an_attach_soon_after_it_succeeds() {
        let mut world = finished(100.0);
        let options = StartOptions { process: Some(RunId(7)), rerunnable: false, ..Default::default() };
        world.start(attach(), options);
        world.pump(&mut |call| if is_start(call) { Reply::Session(44) } else { Reply::Done });
        assert_eq!(world.outcomes.last().and_then(|outcome| outcome.session), Some(id(44)));
        assert_eq!(world.state.sessions().keys().copied().collect::<Vec<_>>(), [id(44)]);
        assert!(world.notified().is_empty());
    }

    // --- runOrigin.test.ts -------------------------------------------------------------

    #[test]
    fn retains_only_configured_debug_roots_for_services_without_changing_live_toolbar_sessions() {
        let mut world = World::default();
        world.adopt(1, |s| {
            s.run_config = Some(config());
            s.process = Some(RunId(7));
            s.started_at = Some(10.0);
        });
        world.adopt(2, |s| {
            s.parent = Some(id(1));
            s.status = SessionStatus::Paused;
        });
        world.adopt(3, |_| {});
        world.state.publish_context();
        let expected = ServiceSession {
            id: id(1),
            config: config(),
            started_at: 10.0,
            status: SessionStatus::Paused,
            process_id: Some(RunId(7)),
            http_port: None,
            can_rerun: true,
        };
        assert_eq!(world.state.published().service_sessions, std::slice::from_ref(&expected));
        world.event(1, DebugEvent::Terminated);
        let published = world.state.published();
        assert_eq!(published.sessions.iter().map(|s| s.id).collect::<Vec<_>>(), [id(3)]);
        assert_eq!(published.service_sessions, [ServiceSession { status: SessionStatus::Terminated, ..expected }]);
    }

    #[test]
    fn does_not_republish_service_metadata_when_console_output_changes() {
        let mut world = World::default();
        world.adopt(1, |s| s.run_config = Some(config()));
        assert_eq!(world.state.published().service_sessions.len(), 1);
        let published = world.state.publish_seq();
        append(&mut world, 1, "stdout", "application output\n");
        assert_eq!(world.state.publish_seq(), published);
    }

    #[test]
    fn announces_the_apps_web_server_port_once_it_appears_on_the_console() {
        let mut world = World::default();
        world.adopt(1, |s| s.run_config = Some(config()));
        let before = world.state.publish_seq();
        append(&mut world, 1, "stdout", "Tomcat started on port 8080 (http) with context path '/'\n");
        let with_port = world.state.publish_seq();
        assert_ne!(with_port, before);
        assert_eq!(world.state.published().service_sessions[0].http_port, Some(8080));
        assert_eq!(world.state.session_http_port(id(1)), Some(8080));
        // Later output without a new port keeps the announced one without republishing.
        append(&mut world, 1, "stdout", "application output\n");
        assert_eq!(world.state.publish_seq(), with_port);
    }

    fn attached_to_process() -> World {
        let mut world = World::default();
        world.adopt(1, |s| {
            s.run_config = Some(config());
            s.process = Some(RunId(7));
        });
        world.settle();
        world
    }

    #[test]
    fn stops_shared_process_debugger_roots_without_stopping_their_process_until_requested() {
        let mut world = attached_to_process();
        world.stop(1, false);
        world.settle();
        assert_eq!(world.count(|call| matches!(call, BackendCall::DebugStop(session) if *session == id(1))), 1);
        assert_eq!(world.count(|call| matches!(call, BackendCall::RunStop(_))), 0);
        assert_eq!(world.session(1).status, SessionStatus::Stopping);
        world.stop(1, true);
        world.settle();
        assert_eq!(world.count(|call| matches!(call, BackendCall::RunStop(RunId(7)))), 1);
    }

    #[test]
    fn stops_associated_processes_through_run_before_disconnecting_the_debugger() {
        let mut world = attached_to_process();
        world.run_stop_action = true;
        world.stop(1, true);
        let effects = world.state.take_effects();
        assert!(matches!(
            effects.as_slice(),
            [
                Effect::ExecuteAction { id: action, arg: Some(process) },
                Effect::Ask { call: BackendCall::DebugStop(DebugSessionId(1)), .. },
            ] if action == "run.stopProcess" && *process == json!(7)
        ));
    }

    #[test]
    fn pins_show_in_debug_to_the_configured_session_and_clears_summaries_with_the_project() {
        let mut world = World::default();
        world.adopt(1, |s| s.run_config = Some(config()));
        world.adopt(2, |s| s.run_config = Some(RunConfig { id: "other".to_owned(), ..config() }));
        world.state.select_session(id(2));
        let shown = world.state.published().service_sessions[0].id;
        world.state.show_service_session(shown);
        assert_eq!(world.state.active(), Some(id(1)));
        {
            let (state, mut ctx) = parts!(&mut world);
            state.reset_sessions(&mut ctx);
        }
        assert!(world.state.published().service_sessions.is_empty());
        world.state.show_service_session(shown);
        assert_eq!(world.state.active(), None);
    }

    #[test]
    fn removes_terminated_summaries_when_a_subsequent_debug_start_prunes_their_source() {
        let mut world = World::default();
        world.adopt(1, |s| s.run_config = Some(config()));
        world.event(1, DebugEvent::Terminated);
        let next = RunConfig { id: "next".to_owned(), ..config() };
        let options = StartOptions { run_config: Some(next), ..Default::default() };
        world.start(launch("next", "js-debug", "launch"), options);
        world.settle();
        let services: Vec<DebugSessionId> = world.state.published().service_sessions.iter().map(|s| s.id).collect();
        assert_eq!(services, [id(42)]);
    }

    #[test]
    fn exposes_a_paused_child_through_its_originating_root_rather_than_the_active_unrelated_session() {
        let mut world = World::default();
        world.adopt(1, |s| s.run_config = Some(config()));
        world.adopt(2, |s| {
            s.parent = Some(id(1));
            s.status = SessionStatus::Paused;
            s.stopped_thread = Some(1);
        });
        world.adopt(3, |s| s.run_config = Some(RunConfig { id: "other".to_owned(), ..config() }));
        world.state.select_session(id(3));
        let roots: Vec<(Option<&str>, bool)> =
            world.state.published().sessions.iter().map(|s| (s.config_id.as_deref(), s.paused)).collect();
        assert_eq!(roots, [(Some("node:server"), true), (Some("other"), false)]);
        world.state.resume_toolbar_session(id(1));
        assert_eq!(world.state.active(), Some(id(2)));
        let effects = world.state.take_effects();
        assert!(effects.iter().any(|e| matches!(e, Effect::ExecuteAction { id, .. } if id == "debug.resume")));
    }

    // --- startFailure.test.ts ------------------------------------------------------------

    #[test]
    fn start_failures_are_what_the_notification_of_a_failed_start_says() {
        let mut world = World::default();
        let listen = LaunchConfig {
            extra_json: Some(json!({ "port": 9003 }).to_string()),
            ..launch("Listen for Xdebug", "php-debug", "launch")
        };
        world.start(listen, StartOptions::default());
        let message = "launch failed: listen EADDRINUSE: address already in use :::9003";
        world.pump(&mut |call| if is_start(call) { Reply::Fail(IdeError::invalid(message)) } else { Reply::Done });
        assert_eq!(world.outcomes[0].session, None);
        let notification = world.effects.iter().find_map(Effect::notification).expect("a notification");
        assert_eq!(notification.title, "Cannot start debugger for Listen for Xdebug");
        assert_eq!(
            notification.body.as_deref(),
            Some(concat!(
                "Port 9003 is already in use: another IDE or debug session may be listening on it. ",
                "Stop it or choose another port.",
            )),
        );
    }

    // --- startupBreakpoints.test.ts ---------------------------------------------------

    /// A world whose project `/p` is open, with no breakpoints yet.
    fn project() -> World {
        let mut world = World::default();
        world.breakpoints.lines.load(Some("/p"), None, world.now);
        world
    }

    fn add(world: &mut World, path: &str, line: u32) {
        world.breakpoints.lines.add_breakpoint(path, line, BreakpointProps::default(), world.now);
    }

    fn start_java(world: &mut World, request: &str) {
        world.start(java_launch(request), StartOptions::default());
    }

    /// Answers every command but breakpoint syncs, which stay unanswered.
    fn hold_breakpoints(world: &mut World) {
        world.pump(&mut |call| if is_set_breakpoints(call) { Reply::Later } else { Reply::Done });
    }

    /// The breakpoint syncs made so far: each file with the lines sent.
    fn synced(world: &World) -> Vec<(&str, Vec<u32>)> {
        world
            .calls
            .iter()
            .filter_map(|call| match call {
                BackendCall::DebugSetBreakpoints { path, breakpoints } => {
                    Some((path.as_str(), breakpoints.iter().map(|b| b.line).collect()))
                }
                _ => None,
            })
            .collect()
    }

    fn shows_tool_window(world: &World) -> bool {
        world.effects.iter().any(|effect| matches!(effect, Effect::ShowToolWindow { focus: false }))
    }

    #[test]
    fn respects_show_on_start_for_direct_and_prepared_starts() {
        for show in [None, Some(true), Some(false)] {
            for (show_on_start, prepared) in [(true, false), (true, true), (false, false), (false, true)] {
                let mut world = World::default();
                world.settings.run.show_on_start = show_on_start;
                if prepared {
                    let mut java = rc("application", "java", &[], Some("java-debug"));
                    java.name = "Java".to_owned();
                    let (state, mut ctx) = parts!(&mut world);
                    let preparation = state.begin_preparation(java, show, &mut ctx);
                    state.preparation_ready(preparation, java_launch("launch"), show, &mut ctx);
                } else {
                    world.start(java_launch("launch"), StartOptions { show, ..Default::default() });
                }
                world.settle();
                assert_eq!(shows_tool_window(&world), show_on_start && show != Some(false), "{show:?} {prepared}");
                assert_eq!(world.outcomes[0].session, Some(id(42)));
                if prepared {
                    assert_eq!(world.console_text(id(42)), ["Starting debugger for Java…\n"]);
                    assert_eq!(world.state.sessions().len(), 1);
                }
            }
        }
    }

    #[test]
    fn reveals_an_in_place_session_on_suspend() {
        for reason in ["breakpoint", "function breakpoint", "exception", "pause"] {
            let mut world = World::default();
            world.start(java_launch("launch"), StartOptions { show: Some(false), ..Default::default() });
            world.settle();
            assert!(!shows_tool_window(&world));
            world.stopped(42, 1, reason);
            world.settle();
            assert!(shows_tool_window(&world), "{reason}");
            assert_eq!(world.state.active(), Some(id(42)));
            let session = world.session(42);
            assert_eq!((session.status, session.selected_thread), (SessionStatus::Paused, Some(1)));
        }
    }

    #[test]
    fn awaits_every_file_before_the_start_can_send_configuration_done() {
        for request in ["launch", "attach"] {
            let mut world = project();
            add(&mut world, "/p/Main.java", 4);
            add(&mut world, "/p/Other.java", 7);
            start_java(&mut world, request);
            hold_breakpoints(&mut world);
            assert_eq!(synced(&world), [("/p/Main.java", vec![4]), ("/p/Other.java", vec![7])]);
            assert_eq!(world.count(is_start), 0);
            world.resolve(is_set_breakpoints, Reply::Done);
            world.settle();
            assert_eq!(world.count(is_start), 0);
            world.resolve(is_set_breakpoints, Reply::Done);
            world.settle();
            assert_eq!(world.count(is_start), 1);
            world.breakpoints.sync_due(world.now + 120.0, true);
            world.settle();
            assert_eq!(world.count(is_set_breakpoints), 2);
        }
    }

    #[test]
    fn drains_an_in_flight_update_and_sends_pending_removals_before_startup() {
        let mut world = project();
        add(&mut world, "/p/Main.java", 4);
        world.breakpoints.sync_due(world.now + 120.0, true);
        hold_breakpoints(&mut world);
        world.breakpoints.lines.remove_all_breakpoints(world.now + 130.0);
        start_java(&mut world, "launch");
        hold_breakpoints(&mut world);
        assert_eq!(world.count(is_start), 0);
        world.resolve(is_set_breakpoints, Reply::Done);
        world.settle();
        assert_eq!(synced(&world).last(), Some(&("/p/Main.java", Vec::new())));
        assert_eq!(world.count(is_start), 1);
    }

    #[test]
    fn flushes_persisted_breakpoints_immediately_after_project_loading() {
        let mut world = World::default();
        let stored = r#"{"files":{"/p/Main.java":[{"line":4}]}}"#;
        world.breakpoints.lines.load(Some("/p"), Some(stored), world.now);
        start_java(&mut world, "launch");
        world.settle();
        assert_eq!(world.count(is_set_breakpoints), 1);
        let sync = world.calls.iter().position(is_set_breakpoints);
        assert!(sync < world.calls.iter().position(is_start));
    }

    #[test]
    fn flushes_edits_made_while_another_file_is_still_syncing() {
        let mut world = project();
        add(&mut world, "/p/Main.java", 4);
        start_java(&mut world, "launch");
        hold_breakpoints(&mut world);
        add(&mut world, "/p/Other.java", 7);
        world.resolve(is_set_breakpoints, Reply::Done);
        world.settle();
        assert_eq!(synced(&world), [("/p/Main.java", vec![4]), ("/p/Other.java", vec![7])]);
        let last_sync = world.calls.iter().rposition(is_set_breakpoints);
        assert!(last_sync < world.calls.iter().position(is_start));
    }

    #[test]
    fn does_not_start_if_the_initial_breakpoint_sync_fails() {
        let mut world = project();
        add(&mut world, "/p/Main.java", 4);
        start_java(&mut world, "launch");
        let failed = Reply::Fail(IdeError::internal("breakpoint sync failed"));
        world.pump(&mut |call| if is_set_breakpoints(call) { failed.clone() } else { Reply::Done });
        assert_eq!(world.outcomes.iter().map(|outcome| outcome.session).collect::<Vec<_>>(), [None]);
        assert_eq!(world.count(is_start), 0);
    }

    #[test]
    fn shares_the_breakpoint_drain_between_concurrent_starts() {
        for request in ["launch", "attach"] {
            let mut world = project();
            add(&mut world, "/p/Main.java", 4);
            start_java(&mut world, request);
            start_java(&mut world, request);
            hold_breakpoints(&mut world);
            assert_eq!(world.count(is_start), 0);
            add(&mut world, "/p/Main.java", 7);
            world.resolve(is_set_breakpoints, Reply::Done);
            let mut sessions = VecDeque::from([Reply::Session(42), Reply::Session(43)]);
            world.pump(&mut |call| match call {
                BackendCall::DebugStart(_) => sessions.pop_front().unwrap_or(Reply::Done),
                _ => Reply::Done,
            });
            let started: Vec<Option<DebugSessionId>> = world.outcomes.iter().map(|outcome| outcome.session).collect();
            assert_eq!(started, [Some(id(42)), Some(id(43))]);
            assert_eq!(synced(&world), [("/p/Main.java", vec![4]), ("/p/Main.java", vec![4, 7])]);
            world.breakpoints.sync_due(world.now + 120.0, true);
            world.settle();
            assert_eq!(world.count(is_set_breakpoints), 2);
        }
    }

    #[test]
    fn releases_a_failed_shared_drain_so_a_later_start_can_retry() {
        let mut world = project();
        add(&mut world, "/p/Main.java", 4);
        start_java(&mut world, "launch");
        start_java(&mut world, "attach");
        let failed = Reply::Fail(IdeError::internal("breakpoint sync failed"));
        world.pump(&mut |call| if is_set_breakpoints(call) { failed.clone() } else { Reply::Done });
        assert_eq!(world.outcomes.iter().map(|outcome| outcome.session).collect::<Vec<_>>(), [None, None]);
        assert_eq!(world.count(is_start), 0);
        start_java(&mut world, "launch");
        world.settle();
        assert_eq!(world.outcomes.last().and_then(|outcome| outcome.session), Some(id(42)));
        assert_eq!(world.count(is_start), 1);
    }

    // --- taskbarFlash.test.ts ---------------------------------------------------------

    /// A Java session in a window that is in the background.
    fn in_background() -> World {
        let mut world = java_session();
        world.focused = false;
        world
    }

    /// How often the taskbar was asked to flash, and to stop flashing.
    fn flashes(world: &mut World) -> (usize, usize) {
        world.settle();
        let on = world.count(|call| matches!(call, BackendCall::WindowFlashTaskbar(true)));
        let off = world.count(|call| matches!(call, BackendCall::WindowFlashTaskbar(false)));
        (on, off)
    }

    #[test]
    fn flashes_when_a_breakpoint_stops_an_unfocused_window() {
        let mut world = in_background();
        world.stopped(1, 1, "breakpoint");
        assert_eq!(flashes(&mut world), (1, 0));
    }

    #[test]
    fn does_not_flash_when_the_window_is_focused() {
        let mut world = java_session();
        world.stopped(1, 1, "breakpoint");
        assert_eq!(flashes(&mut world), (0, 0));
    }

    #[test]
    fn does_not_flash_for_a_stop_that_is_not_a_breakpoint() {
        let mut world = in_background();
        world.stopped(1, 1, "step");
        world.stopped(1, 1, "exception");
        assert_eq!(flashes(&mut world), (0, 0));
    }

    #[test]
    fn does_not_re_request_attention_for_repeated_breakpoints_while_already_flashing() {
        let mut world = in_background();
        world.stopped(1, 1, "breakpoint");
        world.stopped(1, 1, "breakpoint");
        world.stopped(1, 1, "function breakpoint");
        assert_eq!(flashes(&mut world), (1, 0));
    }

    #[test]
    fn stops_the_flash_when_the_window_is_focused() {
        let mut world = in_background();
        world.stopped(1, 1, "breakpoint");
        world.focused = true;
        world.state.window_focused(true);
        assert_eq!(flashes(&mut world), (1, 1));
        // Further focus events send no more stops: the flash is forgotten. A breakpoint that hits
        // afterwards finds the window focused and does not flash again.
        world.state.window_focused(true);
        world.stopped(1, 1, "breakpoint");
        assert_eq!(flashes(&mut world), (1, 1));
    }

    #[test]
    fn stops_the_flash_when_the_session_resumes() {
        let mut world = in_background();
        world.stopped(1, 1, "breakpoint");
        world.event(1, DebugEvent::Continued { thread_id: 1, all_threads_continued: true });
        assert_eq!(flashes(&mut world), (1, 1));
    }

    #[test]
    fn stops_the_flash_when_the_session_terminates() {
        let mut world = in_background();
        world.stopped(1, 1, "breakpoint");
        world.event(1, DebugEvent::Terminated);
        assert_eq!(flashes(&mut world), (1, 1));
    }

    #[test]
    fn does_not_flash_when_focus_application_on_breakpoint_takes_the_window_instead() {
        let mut world = in_background();
        world.settings.debugger.focus_app_on_breakpoint = true;
        world.stopped(1, 1, "breakpoint");
        assert_eq!(flashes(&mut world), (0, 0));
        assert!(world.effects.iter().any(|effect| matches!(effect, Effect::FocusAppWindow)));
    }

    #[test]
    fn never_touches_the_shell_without_one() {
        let mut world = in_background();
        world.desktop = false;
        world.stopped(1, 1, "breakpoint");
        assert_eq!(flashes(&mut world), (0, 0));
    }

    // --- further behaviour ---------------------------------------------------------------

    // Not from a spec: a suspending breakpoint that logs prints its lines once the expressions are
    // evaluated in the stopped frame, and a temporary breakpoint goes with its first hit.
    #[test]
    fn logs_the_hit_of_a_suspending_breakpoint_and_consumes_a_temporary_one() {
        let mut world = java_session();
        world.breakpoints.lines.load(Some("/proj"), None, world.now);
        let props = BreakpointProps {
            log_hit: true,
            log_message: Some("i={i} j={j}".to_owned()),
            temporary: true,
            ..Default::default()
        };
        world.breakpoints.lines.add_breakpoint(PATH, 10, props, world.now);
        world.stopped(1, 5, "breakpoint");
        world.pump(&mut |call| match call {
            BackendCall::DebugStackTrace { .. } => top(10),
            BackendCall::DebugEvaluateIn { expression, frame: Some(100), .. } if expression == "i" => {
                Reply::Value(variable("i", "7"))
            }
            BackendCall::DebugEvaluateIn { .. } => Reply::Fail(IdeError::internal("undefined")),
            _ => Reply::Done,
        });
        let logged: Vec<(&str, &str)> =
            world.state.console(id(1)).iter().map(|line| (line.category.as_str(), line.text.as_str())).collect();
        assert_eq!(
            logged,
            [("console", "Breakpoint reached at MsMemberUtils.java:11\n"), ("console", "i=7 j=<undefined>\n")],
        );
        assert!(world.breakpoints.lines.breakpoints_of(PATH).is_empty());
        assert_eq!(world.session(1).hits, 1);
    }

    // Not from a spec: what the adapter supports decides the hot swap key, and a frame whose only
    // scope is expensive gets no inline values.
    #[test]
    fn reads_capabilities_and_skips_expensive_scopes() {
        let mut world = World::default();
        world.adopt(1, |s| s.adapter = "dart".to_owned());
        let caps = DebugCapabilities { hot_code_replace: true, restart_frame: true, ..Default::default() };
        world.pump(&mut |call| match call {
            BackendCall::DebugCapabilities(_) => Reply::Caps(caps.clone()),
            _ => Reply::Done,
        });
        let context = world.state.published().context;
        assert!(context.can_hot_swap && context.session_dart && context.session_running && !context.can_rerun);
        assert!(context.entries().contains(&("debugCanHotSwap", true)));
        assert!(world.session(1).supports(|caps| caps.restart_frame));
        world.stopped(1, 5, "step");
        let expensive = Scope { name: "Globals".to_owned(), variables_reference: 3, expensive: true };
        world.pump(&mut |call| match call {
            BackendCall::DebugStackTrace { .. } => top(3),
            BackendCall::DebugScopes { .. } => Reply::Scopes(vec![expensive.clone()]),
            _ => Reply::Done,
        });
        assert_eq!(world.count(|call| matches!(call, BackendCall::DebugVariables { .. })), 0);
        assert_eq!(point(&world), (Some(PATH), 3));
    }

    // Not from a spec: watches are trimmed, unique and stored as a JSON array of strings.
    #[test]
    fn stores_the_watch_expressions() {
        let mut state = DebugState::default();
        state.add_watch(" total ");
        state.add_watch("total");
        state.add_watch("  ");
        state.add_watch("items.size()");
        state.remove_watch("total");
        assert_eq!(state.watches(), ["items.size()"]);
        let stored = state.take_effects().into_iter().rev().find_map(|effect| match effect {
            Effect::StorageSet { key, value } if key == WATCH_KEY => Some(value),
            _ => None,
        });
        assert_eq!(stored.as_deref(), Some(r#"["items.size()"]"#));
        state.load_watches(Some(r#"["a", 1, "b"]"#));
        assert_eq!(state.watches(), ["a", "b"]);
        state.load_watches(Some("{nope"));
        assert_eq!(state.watches(), ["a", "b"]);
        state.load_watches(None);
        assert!(state.watches().is_empty());
    }

    // Not from a spec: closing the tab of a live session asks first, then stops it and passes the
    // selection to the tab on its left; a child session goes with its root.
    #[test]
    fn closes_a_session_tab_after_asking_and_selects_its_neighbour() {
        let mut world = World::default();
        world.adopt(1, |s| s.name = "first".to_owned());
        world.adopt(2, |s| s.name = "second".to_owned());
        world.adopt(3, |s| s.parent = Some(id(2)));
        world.state.select_session(id(2));
        world.settle();
        {
            let (state, mut ctx) = parts!(&mut world);
            assert_eq!(state.close_session_tab(id(9), &mut ctx), CloseTab::Missing);
            assert_eq!(state.close_session_tab(id(2), &mut ctx), CloseTab::Confirm { name: "second".to_owned() });
            state.close_confirmed(id(2), &mut ctx);
        }
        world.settle();
        assert_eq!(world.state.sessions().keys().copied().collect::<Vec<_>>(), [id(1)]);
        assert_eq!(world.state.active(), Some(id(1)));
        assert_eq!(world.count(|call| matches!(call, BackendCall::DebugStop(DebugSessionId(2)))), 1);
        world.settings.run.confirm_terminate = false;
        let (state, mut ctx) = parts!(&mut world);
        assert_eq!(state.close_session_tab(id(1), &mut ctx), CloseTab::Closed);
        assert_eq!(state.active(), None);
    }

    // Not from a spec: values are set and evaluated in the selected frame of a suspended session.
    #[test]
    fn sets_values_and_evaluates_in_the_selected_frame() {
        let mut world = java_session();
        let refused = world.state.set_variable_value(id(1), 10, "count", None, "5");
        assert_eq!(refused, Err("The debugger is not suspended"));
        assert!(world.state.evaluate("count", EvaluateContext::Watch, Some(id(9))).is_err());
        stop_on(&mut world, 5, false, top(6840));
        let stops = world.state.stop_seq();
        assert_eq!(world.state.set_variable_value(id(1), 10, "count", Some("count"), "5"), Ok(()));
        let effects = world.state.take_effects();
        let Some(Effect::Ask { ticket, call: BackendCall::DebugSetVariable { frame: Some(100), value, .. } }) =
            effects.first()
        else {
            panic!("a set variable request in the selected frame");
        };
        assert_eq!(value, "5");
        assert!(world.state.variable_set(*ticket, Ok(variable("count", "5"))).is_ok());
        assert_eq!(world.state.stop_seq(), stops + 1);
        let call = world.state.evaluate("count + 1", EvaluateContext::Hover, None).expect("an evaluation");
        let BackendCall::DebugEvaluateIn { session, frame, context, expression } = call else {
            panic!("an evaluation");
        };
        assert_eq!((session, frame, context), (id(1), Some(100), EvaluateContext::Hover));
        assert_eq!(expression, "count + 1");
        assert!(matches!(
            DebugState::send_program_input(id(1), "y\n"),
            BackendCall::DebugSendInput { text, .. } if text == "y\n"
        ));
        assert!(!world.session(1).reads_program_input());
    }

    // Not from a spec: a missing adapter offers its install, and the launch is repeated afterwards.
    #[test]
    fn offers_to_install_a_missing_adapter_and_debugs_again() {
        let mut world = World::default();
        world.start(launch("server", "js-debug", "launch"), StartOptions::default());
        let missing = Reply::Fail(IdeError::unavailable("debug adapter `js-debug` not found (x)"));
        world.pump(&mut |call| if is_start(call) { missing.clone() } else { Reply::Done });
        let offer = world.effects.iter().find_map(Effect::notification).expect("the install offer");
        assert_eq!(offer.title, "The JavaScript debugger (vscode-js-debug) is not installed");
        assert_eq!((offer.sticky, offer.key.as_deref()), (Some(true), Some("debug.start")));
        assert_eq!(offer.actions[0].command, INSTALL_AND_DEBUG);
        assert!(world.state.install_and_debug());
        let install = world.state.take_effects();
        assert!(matches!(
            install.as_slice(),
            [Effect::ExecuteAction { id, arg: Some(tool) }] if id == "tools.install" && *tool == json!("js-debug")
        ));
        let (state, mut ctx) = parts!(&mut world);
        assert!(state.retry_start(&mut ctx).is_some());
        assert!(!state.install_and_debug());
        world.settle();
        assert_eq!(world.outcomes.last().and_then(|outcome| outcome.session), Some(id(42)));
    }
}
