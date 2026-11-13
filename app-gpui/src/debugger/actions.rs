//! Debugger actions (ids match the keymap presets; their titles, icons and enablement are in the
//! action manifest). [`Debugger`] owns the debugger's models and decides what each action does:
//! what goes to the debug adapter, what opens, what the user is told.
//!
//! Nothing here does I/O. Effects collect for [`Debugger::take_effects`]. The answer to an
//! [`Effect::Ask`] goes to the continuation of its command, with its ticket:
//!
//! - session pulls (stack trace, scopes, variables, capabilities, stop, evaluations of a log
//!   message, set variable): the continuations of [`DebugState`];
//! - start: [`Debugger::started`]; breakpoint syncs: [`Debugger::breakpoints_synced`];
//! - step / resume / run to cursor / reset frame: [`Debugger::resumed`];
//! - pause, hot reload, hot restart: [`Debugger::reported`]; threads: [`Debugger::threads_loaded`];
//! - step-in targets, attachable processes, run configurations, prepared configurations, the PHP
//!   interpreter and its Xdebug, the JVM listener, the Java launch steps and the hot swap steps:
//!   the method of [`Debugger`] named after each.

use crate::debugger::breakpoints::Breakpoints;
use crate::debugger::breakpoints::code_lines::line_has_code;
use crate::debugger::breakpoints::special::SpecialKind;
use crate::debugger::breakpoints::symbols::{field_at, method_at};
use crate::debugger::breakpoints::toggle::toggle_breakpoint_smart;
use crate::debugger::editor::quick_evaluate::{QuickEvaluate, evaluate_seed, quick_evaluate};
use crate::debugger::editor::smart_step::{PausedLine, paused_line_targets, start_inline_smart_step};
use crate::debugger::effects::{
    BackendCall, Effect, Env, EvaluateContext, error_text, is_cancelled, notification_action, path_text,
    run_notification,
};
use crate::debugger::focused_view::FocusedView;
use crate::debugger::hotswap::{self, HotSwap, HotSwapStep};
use crate::debugger::java::{self, JavaAttachTarget, JavaFlow, JavaStep, is_java_debuggable};
use crate::debugger::launch::{LaunchEnv, debug_unsupported_reason, launch_config_for};
use crate::debugger::overlays::{AttachPopup, BreakpointSelection, Overlays};
use crate::debugger::remote::{
    JdwpMode, PythonAttachTarget, REMOTE_JVM, is_remote_config, python_attach_config, remote_attach_config,
    remote_jvm_target,
};
use crate::debugger::state::{self, Ctx, DebugSession, DebugState, SessionStatus, StartOptions, StartOutcome};
use crate::editor::buffer;
use crate::platform::i18n::{I18n, Param};
use crate::platform::misc::jdk::{DOWNLOAD_JDK_ACTION, download_jdk_fix};
use crate::platform::virtual_docs::class_file_breakpoint_reason;
use crate::run::java_launch::FoundSource;
use crate::run::store::{RunSettings as RunTargets, with_run_target};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::{DebugSessionId, IdeError, RequestId, RunId};
use ide_dap::attach::AttachableProcess;
use ide_dap::{HotSwapResult, LaunchConfig, StepInTarget, StepKind, ThreadInfo};
use ide_lsp::jdtls::{JavaBuildStatus, JavaLaunchInfo};
use ide_run::RunConfig;
use ide_vfs::FileEntry;
use ropey::Rope;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::ops::Range;

/// Tickets of this model's own asks start here; the sessions' are below.
const OWN_TICKETS: u64 = 1 << 40;
/// Command of the "Open PHP Settings" button of the Xdebug warning.
pub const OPEN_PHP_SETTINGS: &str = "debug.openPhpSettings";
/// Command prefix of the "Cancel" button of a JVM listener's notification; the request id follows.
const CANCEL_LISTEN: &str = "debug.cancelListen:";

/// The text editor that has the focus.
#[derive(Debug, Clone, Copy)]
pub struct ActiveEditor<'a> {
    pub path: &'a str,
    pub language: &'a str,
    pub doc: &'a Rope,
    /// Start and end of the main selection, as UTF-16 offsets.
    pub selection: (u32, u32),
    /// The caret: the end of the main selection that moves.
    pub head: u32,
    /// The document's comment nodes as UTF-16 offset ranges (none without a parser).
    pub comments: &'a [Range<u32>],
}

impl ActiveEditor<'_> {
    /// The 0-based line of the caret.
    pub fn line(&self) -> u32 {
        buffer::line_col(self.doc, self.head).0
    }
}

/// What the actions read from the rest of the IDE.
pub struct Host<'a> {
    pub env: Env<'a>,
    pub i18n: &'a mut I18n,
    /// The open project's root.
    pub root: Option<&'a str>,
    /// The host that runs commands is Windows.
    pub windows: bool,
    /// The Run feature's `run.debugConfig` action exists: detected configurations start through it
    /// (Maven / Gradle / scripts start the JVM with a JDWP agent there).
    pub run_debug_action: bool,
    /// The action that downloads a JDK exists.
    pub download_jdk_action: bool,
    /// The run targets chosen in the settings.
    pub run_targets: &'a RunTargets,
    /// The module directories of the build model.
    pub modules: &'a [&'a str],
}

/// Something only the focused view can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewCommand {
    /// The focused Variables view edits the selected value.
    SetValue,
    /// The focused view opens the source of its selection.
    JumpToSource,
    /// The focused debug console input shows its history.
    BrowseConsoleHistory,
}

/// What Quick Evaluate Expression asks of the editor.
#[derive(Debug, Clone)]
pub enum QuickEvaluation {
    /// Show `message` as a hint at the caret.
    Hint { message: &'static str },
    /// Run `call`, then show `quick_evaluate::evaluated` of its answer for `expression` at `pos`.
    Evaluate { call: BackendCall, expression: String, pos: u32 },
    /// Show why nothing can be evaluated, in the error tone, at `pos`.
    Failed { message: &'static str, pos: u32 },
}

/// What running an action came to.
#[derive(Debug, Clone)]
pub enum Dispatch {
    /// Not a debugger action.
    Unknown,
    Done,
    View(ViewCommand),
    QuickEvaluate(QuickEvaluation),
}

/// What a plugin that started a debug session is told, under the operation it started it for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginEvent {
    /// The session could not start.
    Failed { operation: String, message: String },
    /// The session ended; `stopped`: because the user stopped it.
    Exited { operation: String, session: DebugSessionId, code: Option<i32>, stopped: bool },
}

/// How far the start of a configuration has come.
#[derive(Debug, Clone)]
enum Stage {
    /// Waiting for the project's run configurations, to choose one.
    Configs,
    /// A plugin prepares its configuration for debugging.
    PluginPrepared,
    /// The .NET target of `config` is evaluated.
    DotnetResolved { config: Box<RunConfig> },
    /// The PHP chosen in the settings is looked up.
    Php { config: Box<RunConfig>, prepared: Box<RunConfig> },
    /// The open documents are saved before the launch.
    Saving { config: Box<RunConfig>, launch: Box<LaunchConfig>, run_after: bool },
    /// The IDE listens for the remote JVM of `config` to connect.
    Listening { config: Box<RunConfig>, module: String, key: String },
}

/// The start of a run configuration under the debugger.
#[derive(Debug, Clone)]
struct StartFlow {
    root: String,
    /// The configuration the user started, when what is launched was derived from it.
    origin: Option<RunConfig>,
    show: Option<bool>,
}

/// What a Java flow is for.
#[derive(Debug, Clone)]
enum JavaKind {
    /// The launch of an Application configuration, in the preparation session.
    Launch { preparation: DebugSessionId, request: RequestId, show: Option<bool> },
    /// An attach to a JDWP agent; the session starts with `options`.
    Attach { options: Box<StartOptions>, title: &'static str },
}

#[derive(Debug, Clone)]
struct JavaRun {
    flow: JavaFlow,
    kind: JavaKind,
}

/// Which answer a Java flow waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JavaWait {
    Save,
    Files,
    Configs,
    Sources,
    Resolve,
    Dir,
    Server,
    ProjectFiles,
}

/// What an ask of this model was for.
#[derive(Debug, Clone)]
enum Waiting {
    /// A command whose failure is reported under this title.
    Report(String),
    StepInTargets {
        session: DebugSessionId,
        thread: i64,
    },
    PauseThreads(DebugSessionId),
    AttachableProcesses,
    Start(Box<StartFlow>, Stage),
    Java(Box<JavaRun>, JavaWait),
    /// The `php` on the search path is located, to probe it in `cwd`.
    LocatePhp {
        cwd: Option<String>,
    },
    Xdebug,
    HotSwap,
    /// A plugin's configuration for its operation is prepared, then saved before its session starts.
    PluginPrepare {
        operation: String,
    },
    PluginSave {
        operation: String,
        config: Box<RunConfig>,
        launch: Box<LaunchConfig>,
    },
}

/// What happens once a start has ended.
#[derive(Debug, Clone)]
enum AfterStart {
    /// A PHP program on a plugin's run target: it is run there once the debugger listens.
    RunPluginTarget(Box<RunConfig>),
    /// A container waiting for the debugger: it stops when the attach fails.
    StopProcessOnFailure(RunId),
    /// A plugin started the session for this operation and is told how it went.
    Plugin(String),
}

/// The debugger: its sessions, breakpoints, overlays, and what its actions do.
#[derive(Debug, Default)]
pub struct Debugger {
    pub state: DebugState,
    pub breakpoints: Breakpoints,
    pub overlays: Overlays,
    pub hot_swap: HotSwap,
    pub focused: FocusedView,
    /// A debug console input has the focus: Browse Console History is its to show.
    pub console_input_focused: bool,
    tickets: u64,
    waiting: HashMap<u64, Waiting>,
    /// The failure titles of the resuming commands under way, by the sessions' tickets.
    resume_titles: HashMap<u64, String>,
    starts: HashMap<u64, AfterStart>,
    /// Sessions plugins started, with the operation each is for and whether a stop of it was seen.
    plugin_sessions: HashMap<DebugSessionId, (String, bool)>,
    plugin_events: Vec<PluginEvent>,
    effects: Vec<Effect>,
}

/// The transitions' surroundings, from the fields of a [`Debugger`] and a [`Host`].
macro_rules! ctx {
    ($debugger:ident, $host:ident) => {
        Ctx { breakpoints: &mut $debugger.breakpoints, env: $host.env, i18n: &mut *$host.i18n }
    };
}

fn is_run_config(value: &Value) -> bool {
    ["command", "args", "debugAdapter"].iter().all(|key| value.get(key).is_some())
}

fn run_config(value: &Value) -> Option<RunConfig> {
    is_run_config(value).then(|| serde_json::from_value(value.clone()).ok()).flatten()
}

/// The selected frame of a session (the top frame when none is selected).
fn selected_frame(session: &DebugSession) -> Option<&ide_dap::StackFrame> {
    session.frames.iter().find(|f| Some(f.id) == session.selected_frame).or(session.frames.first())
}

impl Debugger {
    /// What the actions and transitions since the last call ask for: the sessions' effects first,
    /// then the breakpoints', the hot swap's and this model's own.
    pub fn take_effects(&mut self) -> Vec<Effect> {
        let mut effects = self.state.take_effects();
        effects.extend(self.breakpoints.take_effects());
        effects.extend(self.hot_swap.take_effects());
        effects.append(&mut self.effects);
        effects
    }

    /// What plugins that started sessions are to be told, in order.
    pub fn take_plugin_events(&mut self) -> Vec<PluginEvent> {
        std::mem::take(&mut self.plugin_events)
    }

    fn ask(&mut self, call: BackendCall, waiting: Waiting) {
        let ticket = self.wait(waiting);
        self.effects.push(Effect::Ask { ticket, call });
    }

    fn ask_all(&mut self, calls: Vec<BackendCall>, waiting: Waiting) {
        let ticket = self.wait(waiting);
        self.effects.push(Effect::AskAll { ticket, calls });
    }

    fn wait(&mut self, waiting: Waiting) -> u64 {
        self.tickets += 1;
        let ticket = OWN_TICKETS + self.tickets;
        self.waiting.insert(ticket, waiting);
        ticket
    }

    fn notify(&mut self, request: NotificationRequest) {
        self.effects.push(Effect::Notify(Box::new(request)));
    }

    /// Tells the user that something failed. A Java launch without a JDK new enough (for jdtls or
    /// the project) offers to download one.
    fn report(&mut self, title: &str, message: &str, host: &mut Host) {
        let mut request = run_notification(NotificationLevel::Error, title);
        request.body = Some(message.to_owned());
        request.key = Some("debug.action".to_owned());
        if let Some(fix) = download_jdk_fix(Some(message), host.download_jdk_action) {
            let args = serde_json::to_string(&fix.args).unwrap_or_default();
            let label = fix.localized_label(host.i18n);
            request.actions.push(notification_action(&label, format!("{DOWNLOAD_JDK_ACTION} {args}")));
        }
        self.notify(request);
    }

    /// The paused session and the thread to act on.
    fn paused_target(&self) -> Option<(DebugSessionId, i64)> {
        let session = self.state.active_session().filter(|s| s.status == SessionStatus::Paused)?;
        let first_thread = || session.threads.first().map(|thread| thread.id);
        let thread = session.selected_thread.or(session.stopped_thread).or_else(first_thread)?;
        Some((session.id, thread))
    }

    /// Whether breakpoints and Run to Cursor work in `path`; otherwise the status bar says why not.
    fn breakpoint_path_allowed(&mut self, path: &str, host: &mut Host) -> bool {
        let Some(reason) = class_file_breakpoint_reason(path) else { return true };
        self.effects.push(Effect::StatusInfo(host.i18n.tx(reason)));
        false
    }

    /// Sends a resuming command of the paused session; a failure is reported under `title`.
    fn resume(&mut self, title: &str, call: impl FnOnce(DebugSessionId, i64) -> BackendCall, host: &mut Host) {
        let Some((session, thread)) = self.paused_target() else { return };
        if let Some(ticket) = self.state.resume_execution(session, call(session, thread)) {
            self.resume_titles.insert(ticket, host.i18n.tx(title));
        }
    }

    /// A resuming command was answered.
    pub fn resumed(&mut self, ticket: u64, result: Result<(), IdeError>, host: &mut Host) {
        let title = self.resume_titles.remove(&ticket);
        if let (Some(failure), Some(title)) = (self.state.resumed(ticket, result), title) {
            self.report(&title, error_text(&failure.error), host);
        }
    }

    /// A command whose failure is all there is to tell was answered.
    pub fn reported(&mut self, ticket: u64, result: Result<(), IdeError>, host: &mut Host) {
        if let (Some(Waiting::Report(title)), Err(error)) = (self.waiting.remove(&ticket), result) {
            self.report(&title, error_text(&error), host);
        }
    }

    /// Run to Cursor at `line` (0-based) of `path` in the paused session.
    pub fn run_to_line(&mut self, path: &str, line: u32, force: bool, host: &mut Host) {
        if self.paused_target().is_none() || !self.breakpoint_path_allowed(path, host) {
            return;
        }
        let title = if force { "Force Run to Cursor failed" } else { "Run to Cursor failed" };
        let path = path.to_owned();
        self.resume(title, |session, thread| BackendCall::DebugRunTo { session, thread, path, line, force }, host);
    }

    /// Smart Step Into: the calls on the current line, picked in the editor (the dialog when they
    /// cannot be placed); one call (or none) steps straight in, like IDEA.
    fn smart_step_into(&mut self, host: &mut Host) {
        let Some((id, thread)) = self.paused_target() else { return };
        let session = self.state.session(id).filter(|s| s.supports(|caps| caps.step_in_targets));
        match session.and_then(selected_frame).map(|frame| frame.id) {
            Some(frame) => {
                let call = BackendCall::DebugStepInTargets { session: id, frame };
                self.ask(call, Waiting::StepInTargets { session: id, thread });
            }
            None => self.step_into("Smart Step Into failed", None, host),
        }
    }

    fn step_into(&mut self, title: &str, target: Option<i64>, host: &mut Host) {
        self.resume(
            title,
            |session, thread| match target {
                Some(target) => BackendCall::DebugStepInTarget { session, thread, target },
                None => BackendCall::DebugStep { session, thread, kind: StepKind::Into },
            },
            host,
        );
    }

    /// The step-in targets of the selected frame arrived. `paused_line`: where execution is
    /// suspended, as the editors show it.
    pub fn step_in_targets_loaded(
        &mut self,
        ticket: u64,
        result: Result<Vec<StepInTarget>, IdeError>,
        paused_line: Option<&PausedLine>,
        host: &mut Host,
    ) {
        let Some(Waiting::StepInTargets { session, thread }) = self.waiting.remove(&ticket) else { return };
        let title = "Smart Step Into failed";
        let targets = match result {
            Ok(targets) => targets,
            Err(error) => return self.report(&host.i18n.tx(title), error_text(&error), host),
        };
        let line_text = paused_line.and_then(|paused| paused.text);
        let targets = paused_line_targets(line_text, paused_line.map_or(0, |paused| paused.line), targets);
        if targets.len() <= 1 {
            return self.step_into(title, targets.first().map(|target| target.id), host);
        }
        if !start_inline_smart_step(&mut self.overlays, paused_line, session, thread, &targets) {
            self.overlays.open_smart_step_chooser(session, thread, targets);
        }
    }

    /// Opens the Attach to Process popup and loads the process list.
    pub fn open_attach_popup(&mut self, host: &mut Host) {
        if !host.env.desktop {
            self.notify(run_notification(NotificationLevel::Warning, "Attaching needs the desktop app"));
            return;
        }
        self.overlays.attach = Some(AttachPopup::default());
        self.ask(BackendCall::DebugAttachableProcesses, Waiting::AttachableProcesses);
    }

    /// The attachable processes arrived, for the popup when it is still open.
    pub fn attachable_processes_loaded(&mut self, ticket: u64, result: Result<Vec<AttachableProcess>, IdeError>) {
        let Some(Waiting::AttachableProcesses) = self.waiting.remove(&ticket) else { return };
        let Some(popup) = &mut self.overlays.attach else { return };
        *popup = match result {
            Ok(processes) => AttachPopup { processes: Some(processes), error: None },
            Err(error) => {
                let message = Some(error.message).filter(|message| !message.is_empty());
                let error = message.unwrap_or_else(|| "Cannot list processes".to_owned());
                AttachPopup { processes: Some(Vec::new()), error: Some(error) }
            }
        };
    }

    /// Pause Program: the selected thread, else the first one (asked for when none is known).
    fn pause(&mut self, host: &mut Host) {
        let Some(session) = self.state.active_session().filter(|s| s.status == SessionStatus::Running) else { return };
        let known = session.selected_thread.or_else(|| session.threads.first().map(|thread| thread.id));
        let id = session.id;
        match known {
            Some(thread) => self.pause_thread(id, thread, host),
            None => self.ask(BackendCall::DebugThreads(id), Waiting::PauseThreads(id)),
        }
    }

    fn pause_thread(&mut self, session: DebugSessionId, thread: i64, host: &mut Host) {
        let title = host.i18n.tx("Pause failed");
        self.ask(BackendCall::DebugPause { session, thread }, Waiting::Report(title));
    }

    /// The threads of a session arrived: for Pause when it asked, else for the session.
    pub fn threads_loaded(&mut self, ticket: u64, result: Result<Vec<ThreadInfo>, IdeError>, host: &mut Host) {
        match self.waiting.remove(&ticket) {
            Some(Waiting::PauseThreads(session)) => {
                let thread = result.ok().and_then(|threads| threads.first().map(|thread| thread.id));
                self.pause_thread(session, thread.unwrap_or(0), host);
            }
            _ => self.state.threads_loaded(ticket, result),
        }
    }

    /// Toggle Line Breakpoint at the caret.
    fn toggle_breakpoint(&mut self, editor: &ActiveEditor, host: &mut Host) {
        let (path, line, now_ms) = (editor.path, editor.line(), host.env.now_ms);
        if !self.breakpoint_path_allowed(path, host) {
            return;
        }
        let exists = self.breakpoints.lines.at_line(path, line).is_some();
        let any_file = host.env.debugger.breakpoints_any_file;
        if !exists && !self.breakpoints.languages.can_place_breakpoint(editor.language, "", any_file) {
            let mut request = run_notification(NotificationLevel::Info, "Breakpoints can't be placed in this file");
            request.group = None;
            request.key = Some("debug.breakpointNotAllowed".to_owned());
            request.body = Some(
                "Enable Settings › Build, Execution, Deployment › Debugger › Allow placing breakpoints in any file."
                    .to_owned(),
            );
            self.notify(request);
            return;
        }
        // Like IDEA, no new breakpoint on a blank or comment-only line; an existing one still goes.
        let taken = exists || self.breakpoints.special.special_on_line(path, line).is_some();
        if !taken && !line_has_code(editor.doc, line, editor.comments) {
            return;
        }
        let text = editor.doc.to_string();
        let Breakpoints { lines, special, .. } = &mut self.breakpoints;
        toggle_breakpoint_smart(lines, special, path, line, editor.language, &text, now_ms);
    }

    fn toggle_temporary_breakpoint(&mut self, editor: &ActiveEditor, host: &mut Host) {
        let (path, line) = (editor.path, editor.line());
        let any_file = host.env.debugger.breakpoints_any_file;
        if !self.breakpoint_path_allowed(path, host)
            || !self.breakpoints.languages.can_place_breakpoint(editor.language, path, any_file)
        {
            return;
        }
        let exists = self.breakpoints.lines.at_line(path, line).is_some();
        if exists || line_has_code(editor.doc, line, editor.comments) {
            self.breakpoints.lines.toggle_temporary_breakpoint(path, line, host.env.now_ms);
        }
    }

    /// Toggle Method Breakpoint / Toggle Field Watchpoint at the caret.
    fn toggle_special(&mut self, kind: SpecialKind, editor: &ActiveEditor, host: &mut Host) {
        let (path, line, now_ms) = (editor.path, editor.line(), host.env.now_ms);
        if !self.breakpoint_path_allowed(path, host) {
            return;
        }
        let placed = self.breakpoints.special.special_at(path);
        let existing = placed.iter().any(|b| b.kind() == kind && b.line() == Some(line));
        let text = editor.doc.to_string();
        let special = &mut self.breakpoints.special;
        let (key, title, body) = match kind {
            SpecialKind::Method => {
                let method = method_at(editor.language, &text, line);
                if existing || method.is_some() {
                    let name = method.as_ref().map_or("", |method| method.name.as_str());
                    special.toggle_method_at(path, line, name, now_ms);
                    return;
                }
                (
                    "debug.noMethod",
                    "No method declaration on this line",
                    "Put the caret on a method declaration, or add a method breakpoint by name in View Breakpoints.",
                )
            }
            SpecialKind::Field => {
                let field = field_at(editor.language, &text, line);
                if existing || field.is_some() {
                    let (data_id, label) = field.map(|field| (field.data_id, field.label)).unwrap_or_default();
                    special.toggle_field_at(path, line, &data_id, &label, now_ms);
                    return;
                }
                (
                    "debug.noField",
                    "No field declaration on this line",
                    "Field watchpoints can be set on Java field declarations, or from the Variables view while \
                     suspended.",
                )
            }
        };
        let mut request = run_notification(NotificationLevel::Info, title);
        request.group = None;
        request.key = Some(key.to_owned());
        request.body = Some(body.to_owned());
        self.notify(request);
    }

    /// Quick Evaluate Expression in `editor`; `click`: the position of the ⌥-click that asked.
    fn quick_evaluate(&self, editor: &ActiveEditor, click: Option<u32>) -> QuickEvaluation {
        match quick_evaluate(editor.doc, editor.selection, click) {
            QuickEvaluate::Nothing(message) => QuickEvaluation::Hint { message },
            QuickEvaluate::Evaluate { expression, pos } => {
                match self.state.evaluate(&expression, EvaluateContext::Hover, None) {
                    Ok(call) => QuickEvaluation::Evaluate { call, expression, pos },
                    Err(message) => QuickEvaluation::Failed { message, pos },
                }
            }
        }
    }

    /// Whether the editor menu offers Evaluate Expression: while the active session is suspended.
    pub fn offers_evaluate(&self) -> bool {
        self.state.active_session().is_some_and(|s| s.status == SessionStatus::Paused)
    }

    /// The live root sessions Stop offers beside the run processes: (id, name).
    pub fn stoppable_sessions(&self) -> Vec<(DebugSessionId, &str)> {
        let roots = self.state.sessions().values().filter(|s| s.parent.is_none() && s.is_live());
        roots.map(|s| (s.id, s.name.as_str())).collect()
    }

    /// Runs the debugger action `id`. `editor`: the text editor that has the focus.
    pub fn run_action(
        &mut self,
        id: &str,
        arg: Option<&Value>,
        editor: Option<&ActiveEditor>,
        host: &mut Host,
    ) -> Dispatch {
        let step = |kind| move |session, thread| BackendCall::DebugStep { session, thread, kind };
        match id {
            "debug.pluginStart" => self.plugin_start(arg, host),
            "debug.startConfig" => self.start_config(arg, host),
            "debug.attach" => self.attach(arg, host),
            "debug.stop" => {
                let live = || self.state.sessions().values().find(|s| s.is_live());
                if let Some(session) = self.state.active_session().or_else(live).map(|s| s.id) {
                    let mut ctx = ctx!(self, host);
                    self.state.stop_session(session, true, &mut ctx);
                }
            }
            "debug.reloadChangedClasses" => {
                let config = arg.and_then(|arg| arg.get("configId")).and_then(Value::as_str);
                self.reload_changed_classes(true, config, host);
            }
            "debug.cancelHotSwap" => {
                let cancel = self.hot_swap.cancel_build();
                self.effects.extend(cancel.map(Effect::Call));
            }
            "debug.rerun" => {
                if let Some(active) = self.state.active() {
                    let root = self.state.root_of(active);
                    let mut ctx = ctx!(self, host);
                    self.state.rerun_session(root, None, &mut ctx);
                }
            }
            "debug.resume" => {
                self.resume("Resume failed", |session, thread| BackendCall::DebugResume { session, thread }, host);
            }
            "debug.pause" => self.pause(host),
            "debug.hotReload" | "debug.hotRestart" => {
                if let Some(session) = self.state.active() {
                    let (title, call) = if id == "debug.hotReload" {
                        ("Hot Reload failed", BackendCall::DebugHotReload(session))
                    } else {
                        ("Hot Restart failed", BackendCall::DebugHotRestart(session))
                    };
                    self.ask(call, Waiting::Report(title.to_owned()));
                }
            }
            "debug.stepOver" => self.resume("Step Over", step(StepKind::Over), host),
            "debug.stepInto" => self.resume("Step Into", step(StepKind::Into), host),
            "debug.stepOut" => self.resume("Step Out", step(StepKind::Out), host),
            "debug.forceStepOver" => self.resume(
                "Force Step Over failed",
                |session, thread| BackendCall::DebugForceStepOver { session, thread },
                host,
            ),
            // DAP has no "ignore stepping filters" flag: adapters step into what their launch
            // options (justMyCode, skipFiles, stepFilters) allow.
            "debug.forceStepInto" => self.step_into("Force Step Into failed", None, host),
            "debug.smartStepInto" => self.smart_step_into(host),
            "debug.showExecutionPoint" => {
                if let Some(session) = self.state.active() {
                    self.state.show_execution_point(session);
                }
            }
            "debug.resetFrame" => self.reset_frame(host),
            "debug.setValue" => {
                if self.focused.current().is_some_and(|view| view.set_value) {
                    return Dispatch::View(ViewCommand::SetValue);
                }
            }
            "debug.jumpToSource" => {
                if self.focused.current().is_some_and(|view| view.jump_to_source) {
                    return Dispatch::View(ViewCommand::JumpToSource);
                }
                let frame = self.state.active_session().and_then(selected_frame);
                let opened = frame.and_then(|frame| DebugState::reveal_frame(frame, true));
                self.effects.extend(opened);
            }
            "debug.attachToProcess" => self.open_attach_popup(host),
            "debug.runToCursor" | "debug.forceRunToCursor" => {
                if let Some(editor) = editor {
                    self.run_to_line(editor.path, editor.line(), id == "debug.forceRunToCursor", host);
                }
            }
            "debug.toggleBreakpoint" => {
                if let Some(editor) = editor {
                    self.toggle_breakpoint(editor, host);
                }
            }
            "debug.toggleTemporaryBreakpoint" => {
                if let Some(editor) = editor {
                    self.toggle_temporary_breakpoint(editor, host);
                }
            }
            "debug.toggleMethodBreakpoint" | "debug.toggleFieldWatchpoint" => {
                let kind = if id == "debug.toggleMethodBreakpoint" { SpecialKind::Method } else { SpecialKind::Field };
                if let Some(editor) = editor {
                    self.toggle_special(kind, editor, host);
                }
            }
            "debug.viewBreakpoints" => {
                let at_caret = editor.and_then(|editor| {
                    let breakpoint = self.breakpoints.lines.at_line(editor.path, editor.line())?;
                    Some(BreakpointSelection { path: editor.path.to_owned(), id: breakpoint.id.clone() })
                });
                self.overlays.open_breakpoints_dialog(at_caret);
            }
            "debug.muteBreakpoints" => self.breakpoints.set_muted(!self.breakpoints.lines.muted(), host.env.now_ms),
            "debug.removeAllBreakpoints" => {
                self.breakpoints.lines.remove_all_breakpoints(host.env.now_ms);
                self.breakpoints.special.remove_all_special(host.env.now_ms);
            }
            "debug.evaluate" => {
                let seed = editor.map(|editor| evaluate_seed(editor.doc, editor.selection, None)).unwrap_or_default();
                self.overlays.open_evaluate_dialog(&seed);
            }
            "debug.browseConsoleHistory" => {
                if self.console_input_focused {
                    return Dispatch::View(ViewCommand::BrowseConsoleHistory);
                }
                self.effects.push(Effect::StatusInfo(host.i18n.tx("Focus the debug console input first")));
            }
            "debug.quickEvaluate" => {
                // ⌥⌘F8: the selection (or expression at the caret) evaluated in a value hint, no dialog.
                let click = arg.and_then(|arg| arg.get("pos")).and_then(Value::as_u64).map(|pos| pos as u32);
                if let Some(editor) = editor {
                    return Dispatch::QuickEvaluate(self.quick_evaluate(editor, click));
                }
            }
            "debug.showToolWindow" => self.effects.push(Effect::ShowToolWindow { focus: true }),
            _ => return Dispatch::Unknown,
        }
        Dispatch::Done
    }

    /// Whether Mute Breakpoints is on (the toggle state of its action).
    pub fn breakpoints_muted(&self) -> bool {
        self.breakpoints.lines.muted()
    }

    /// Reset Frame: return to the start of the selected frame's method.
    fn reset_frame(&mut self, host: &mut Host) {
        let Some((id, _)) = self.paused_target() else { return };
        let Some(session) = self.state.session(id) else { return };
        let Some(frame) = selected_frame(session).map(|frame| frame.id) else { return };
        if !session.supports(|caps| caps.restart_frame) {
            let adapter = if session.adapter.is_empty() { "this debugger" } else { session.adapter.as_str() };
            let title = format!("Reset Frame is not supported by {adapter}");
            let mut request = run_notification(NotificationLevel::Info, title);
            request.key = Some("debug.unsupported".to_owned());
            self.notify(request);
            return;
        }
        self.resume("Reset Frame failed", |session, _| BackendCall::DebugRestartFrame { session, frame }, host);
    }

    /// A notification button of the debugger was pressed. Returns false for a command that is
    /// none of the debugger's.
    pub fn run_notification_command(&mut self, command: &str, host: &mut Host) -> bool {
        let install = |tool: &str| Effect::ExecuteAction { id: "tools.install".to_owned(), arg: Some(json!(tool)) };
        if command == state::INSTALL_AND_DEBUG {
            self.state.install_and_debug();
        } else if command == java::INSTALL_JAVA_DEBUG {
            self.effects.push(install(java::JAVA_ADAPTER));
        } else if command == OPEN_PHP_SETTINGS {
            let arg = Some(json!("languages.php"));
            self.effects.push(Effect::ExecuteAction { id: "file.settings".to_owned(), arg });
        } else if command == hotswap::CANCEL_BUILD {
            let cancel = self.hot_swap.cancel_build();
            self.effects.extend(cancel.map(Effect::Call));
        } else if command == hotswap::RELOAD_WITHOUT_BUILD {
            self.reload_changed_classes(false, None, host);
        } else if let Some(session) = hotswap::reset_frame_session(command) {
            let reset = self.hot_swap.reset_frame(session, &mut self.state);
            self.effects.extend(reset);
        } else if let Some(request) = command.strip_prefix(CANCEL_LISTEN).and_then(|id| id.parse().ok()) {
            self.effects.push(Effect::Call(BackendCall::CancelRequest(RequestId(request))));
        } else if let Some(args) = command.strip_prefix(DOWNLOAD_JDK_ACTION).map(str::trim) {
            let arg = serde_json::from_str(args).ok();
            self.effects.push(Effect::ExecuteAction { id: DOWNLOAD_JDK_ACTION.to_owned(), arg });
        } else {
            return false;
        }
        true
    }

    /// The adapter was installed after "Install and Debug": the launch is repeated.
    pub fn adapter_installed(&mut self, host: &mut Host) {
        let mut ctx = ctx!(self, host);
        self.state.retry_start(&mut ctx);
    }
}

impl Debugger {
    // --- starting a configuration ---------------------------------------------------

    fn fail_start(&mut self, message: &str, host: &mut Host) {
        let title = host.i18n.tx("Cannot start debugging");
        self.report(&title, message, host);
    }

    /// Debug…: starts debugging the run configuration of `arg` (a configuration, or
    /// `{ config, origin, show }`), or the project's only debuggable one.
    fn start_config(&mut self, arg: Option<&Value>, host: &mut Host) {
        let Some(root) = host.root.map(str::to_owned) else { return };
        let origin = arg.and_then(|arg| arg.get("origin")).and_then(run_config);
        let show = arg.and_then(|arg| arg.get("show")).and_then(Value::as_bool);
        let input = arg.map(|arg| arg.get("config").unwrap_or(arg));
        let flow = StartFlow { root, origin, show };
        match input.and_then(run_config) {
            Some(config) => self.start_with_config(flow, config, host),
            None if !host.env.desktop => {
                self.notify(run_notification(NotificationLevel::Warning, "Debugging needs the desktop app"));
            }
            None => self.ask(BackendCall::RunConfigs, Waiting::Start(Box::new(flow), Stage::Configs)),
        }
    }

    /// The run configurations arrived: for Debug… without a configuration, or for a Java launch
    /// that looks for its module.
    pub fn run_configs_loaded(&mut self, ticket: u64, result: Result<Vec<RunConfig>, IdeError>, host: &mut Host) {
        match self.waiting.remove(&ticket) {
            Some(Waiting::Start(flow, Stage::Configs)) => {
                let configs = match result {
                    Ok(configs) => configs,
                    Err(error) => return self.fail_start(error_text(&error), host),
                };
                let has_adapter = |c: &RunConfig| c.debug_adapter.as_deref().is_some_and(|a| !a.is_empty());
                let mut debuggable: Vec<RunConfig> = configs.into_iter().filter(has_adapter).collect();
                if debuggable.len() > 1 {
                    return self.overlays.open_config_chooser(debuggable);
                }
                let Some(config) = debuggable.pop() else {
                    let mut request = run_notification(NotificationLevel::Info, "Nothing to debug");
                    request.body = Some("No run configuration in this project has a debug adapter.".to_owned());
                    return self.notify(request);
                };
                // Through Run's debug flow: Maven / Gradle / scripts start the JVM with a JDWP agent there.
                if host.run_debug_action {
                    let config = serde_json::to_value(config).unwrap_or(Value::Null);
                    let in_place = flow.show == Some(false);
                    let arg = if in_place { json!({ "config": config, "show": false }) } else { config };
                    self.effects.push(Effect::ExecuteAction { id: "run.debugConfig".to_owned(), arg: Some(arg) });
                    return;
                }
                self.start_with_config(*flow, config, host);
            }
            Some(Waiting::Java(mut run, JavaWait::Configs)) => {
                if self.java_abandoned(&run, host) {
                    return;
                }
                let step = run.flow.configs_listed(result.unwrap_or_default(), host.i18n);
                self.java_step(*run, step, host);
            }
            _ => {}
        }
    }

    fn start_with_config(&mut self, flow: StartFlow, config: RunConfig, host: &mut Host) {
        let config = with_run_target(&config, host.run_targets);
        let plugin_target = config.extras.target.as_deref().is_some_and(|target| target.starts_with("plugin-target:"));
        if !plugin_target && is_remote_config(&config.kind) {
            return self.start_remote(flow, config, host);
        }
        if !plugin_target && is_java_debuggable(&config) {
            return self.start_java(flow, config, host);
        }
        if config.kind == "plugin" {
            let call = BackendCall::PluginPrepareDebugRun(Box::new(config));
            return self.ask(call, Waiting::Start(Box::new(flow), Stage::PluginPrepared));
        }
        self.resolve_dotnet(flow, config, host);
    }

    /// A configuration the backend prepared for debugging arrived: a plugin's, or a .NET one with
    /// its target evaluated.
    pub fn config_prepared(&mut self, ticket: u64, result: Result<RunConfig, IdeError>, host: &mut Host) {
        match self.waiting.remove(&ticket) {
            Some(Waiting::Start(flow, stage @ (Stage::PluginPrepared | Stage::DotnetResolved { .. }))) => {
                let prepared = match result {
                    Ok(prepared) => prepared,
                    Err(error) => return self.fail_start(error_text(&error), host),
                };
                match stage {
                    Stage::DotnetResolved { config } => self.resolve_php(*flow, *config, prepared, host),
                    _ => self.resolve_dotnet(*flow, prepared, host),
                }
            }
            Some(Waiting::PluginPrepare { operation }) => self.plugin_prepared(operation, result, host),
            _ => {}
        }
    }

    fn resolve_dotnet(&mut self, flow: StartFlow, config: RunConfig, host: &mut Host) {
        if config.kind == "dotnet" && config.debug_adapter.as_deref() == Some("netcoredbg") {
            let call = BackendCall::RunDotnetResolve(Box::new(config.clone()));
            let stage = Stage::DotnetResolved { config: Box::new(config) };
            return self.ask(call, Waiting::Start(Box::new(flow), stage));
        }
        let prepared = config.clone();
        self.resolve_php(flow, config, prepared, host);
    }

    fn resolve_php(&mut self, flow: StartFlow, config: RunConfig, prepared: RunConfig, host: &mut Host) {
        // php-debug starts the PHP chosen in Settings unless the command names one.
        if config.debug_adapter.as_deref() == Some("php-debug") && host.env.desktop {
            let stage = Stage::Php { config: Box::new(config), prepared: Box::new(prepared) };
            return self.ask(BackendCall::SdkPhpEffective, Waiting::Start(Box::new(flow), stage));
        }
        self.launch(flow, config, prepared, None, host);
    }

    /// The PHP chosen in the settings arrived (`None`: the default, or it could not be told).
    pub fn php_resolved(&mut self, ticket: u64, php: Option<String>, host: &mut Host) {
        if let Some(Waiting::Start(flow, Stage::Php { config, prepared })) = self.waiting.remove(&ticket) {
            self.launch(*flow, *config, *prepared, php.as_deref(), host);
        }
    }

    fn launch(&mut self, flow: StartFlow, config: RunConfig, prepared: RunConfig, php: Option<&str>, host: &mut Host) {
        let env = LaunchEnv { debugger: host.env.debugger, windows: host.windows };
        let Some(mut launch) = launch_config_for(&prepared, &flow.root, php, &env) else {
            let title = format!("“{}” can't be debugged", config.name);
            let mut request = run_notification(NotificationLevel::Warning, title);
            let reason = debug_unsupported_reason(&config);
            request.body = Some(reason.unwrap_or_else(|| "It has no debug adapter.".to_owned()));
            return self.notify(request);
        };
        let target = config.extras.target.as_deref().filter(|target| target.starts_with("plugin-target:"));
        let php_debug = config.debug_adapter.as_deref() == Some("php-debug");
        if let Some(target) = target {
            let extra = launch.extra_json.as_deref().and_then(|extra| serde_json::from_str::<Value>(extra).ok());
            let mut extra = extra.and_then(|extra| extra.as_object().cloned()).unwrap_or_default();
            extra.insert("__runTarget".to_owned(), json!(target));
            launch.extra_json = Some(Value::Object(extra).to_string());
        } else if php_debug && host.env.desktop {
            self.warn_without_xdebug(&launch);
        }
        let run_after = target.is_some() && php_debug;
        let stage = Stage::Saving { config: Box::new(config), launch: Box::new(launch), run_after };
        let ticket = self.wait(Waiting::Start(Box::new(flow), stage));
        self.effects.push(Effect::SaveAllThen { ticket });
    }

    /// A PHP without the Xdebug extension ignores the `-dxdebug.*` options php-debug starts it
    /// with: the script runs past every breakpoint. Warns, without holding the session back (a
    /// command may load Xdebug itself with `-dzend_extension=`).
    fn warn_without_xdebug(&mut self, launch: &LaunchConfig) {
        let extra = launch.extra_json.as_deref().and_then(|extra| serde_json::from_str::<Value>(extra).ok());
        let runtime = extra.as_ref().and_then(|extra| extra.get("runtimeExecutable")).and_then(Value::as_str);
        // In the session's folder: a version manager's `php` shim picks the PHP from it.
        let cwd = launch.cwd.as_ref().map(path_text);
        match runtime {
            Some(php) => {
                let call = BackendCall::SdkPhpXdebug { php: php.to_owned(), cwd };
                self.ask(call, Waiting::Xdebug);
            }
            // Without one, php-debug runs the `php` on the search path.
            None => {
                let call = BackendCall::SdkLocateTools { names: vec!["php".to_owned()] };
                self.ask(call, Waiting::LocatePhp { cwd });
            }
        }
    }

    /// The `php` on the search path was located (`None`: there is none).
    pub fn php_located(&mut self, ticket: u64, php: Option<String>) {
        if let (Some(Waiting::LocatePhp { cwd }), Some(php)) = (self.waiting.remove(&ticket), php) {
            self.ask(BackendCall::SdkPhpXdebug { php, cwd }, Waiting::Xdebug);
        }
    }

    /// A PHP interpreter was probed: its path, its version and whether it loads Xdebug (`None`:
    /// the probe failed, which warns about nothing).
    pub fn xdebug_probed(&mut self, ticket: u64, probe: Option<(&str, &str, bool)>, host: &mut Host) {
        let Some(Waiting::Xdebug) = self.waiting.remove(&ticket) else { return };
        let Some((php, version, false)) = probe else { return };
        let title = format!("Xdebug is not loaded in PHP {version}");
        let mut request = run_notification(NotificationLevel::Warning, title);
        request.key = Some("debug.phpXdebug".to_owned());
        request.body = Some(host.i18n.tx_built(
            &[
                "",
                concat!(
                    " runs without Xdebug, so breakpoints are skipped. ",
                    "Install Xdebug 3 for this PHP or choose another PHP interpreter.",
                ),
            ],
            &[Param::from(php)],
        ));
        request.actions = vec![notification_action("Open PHP Settings", OPEN_PHP_SETTINGS)];
        self.notify(request);
    }

    /// The open documents were saved (or could not be), for whichever flow asked.
    pub fn saved(&mut self, ticket: u64, result: Result<(), String>, host: &mut Host) {
        match self.waiting.remove(&ticket) {
            // A launch goes on with what is on disk.
            Some(Waiting::Start(flow, Stage::Saving { config, launch, run_after })) => {
                let run_config = Some(flow.origin.unwrap_or_else(|| (*config).clone()));
                let options = StartOptions { run_config, show: flow.show, ..Default::default() };
                let mut ctx = ctx!(self, host);
                let start = self.state.start_session(*launch, options, &mut ctx);
                if let Some(start) = start.filter(|_| run_after) {
                    self.starts.insert(start, AfterStart::RunPluginTarget(config));
                }
            }
            Some(Waiting::Java(mut run, JavaWait::Save)) => {
                if self.java_abandoned(&run, host) {
                    return;
                }
                let step = run.flow.saved(result, host.i18n);
                self.java_step(*run, step, host);
            }
            Some(Waiting::HotSwap) => {
                let step = self.hot_swap.saved(&self.state, host.env.now_ms);
                self.hot_swap_step(step);
            }
            Some(Waiting::PluginSave { operation, config, launch }) => match result {
                Err(message) => self.plugin_events.push(PluginEvent::Failed { operation, message }),
                Ok(()) => {
                    let options = StartOptions { run_config: Some(*config), ..Default::default() };
                    let mut ctx = ctx!(self, host);
                    match self.state.start_session(*launch, options, &mut ctx) {
                        Some(start) => {
                            self.starts.insert(start, AfterStart::Plugin(operation));
                        }
                        None => self.plugin_failed(operation, "Debug session did not start"),
                    }
                }
            },
            _ => {}
        }
    }

    /// Remote JVM Debug, Go Remote, Python / Node.js remote: attach over host:port.
    fn start_remote(&mut self, flow: StartFlow, config: RunConfig, host: &mut Host) {
        if config.kind != REMOTE_JVM {
            let skip = host.env.debugger.skip_library_code;
            if let Some(launch) = remote_attach_config(&config, &flow.root, skip) {
                let options = StartOptions { run_config: Some(config), show: flow.show, ..Default::default() };
                let mut ctx = ctx!(self, host);
                self.state.start_session(launch, options, &mut ctx);
            }
            return;
        }
        let target = remote_jvm_target(&config);
        if target.mode == JdwpMode::Attach {
            let attach = JavaAttachTarget {
                name: config.name.clone(),
                module: target.module,
                host_name: target.host,
                port: target.port,
            };
            let options = StartOptions { run_config: Some(config), show: flow.show, ..Default::default() };
            return self.java_attach(&flow.root, attach, options, "Cannot start debugging", host);
        }
        // Listen mode: the IDE listens and the remote JVM connects; java-debug then attaches
        // through a loopback relay.
        let request = RequestId::next();
        let key = format!("debug.listen.{}", request.0);
        let title = format!("'{}': listening for the remote JVM on port {}", config.name, target.port);
        let mut notification = run_notification(NotificationLevel::Info, title);
        notification.key = Some(key.clone());
        notification.sticky = Some(true);
        notification.body = Some(host.i18n.tx_built(
            &["Start the remote JVM with -agentlib:jdwp=transport=dt_socket,server=n,address=", ":", ",suspend=y"],
            &[Param::from(target.host.as_str()), Param::Number(f64::from(target.port))],
        ));
        notification.actions = vec![notification_action("Cancel", format!("{CANCEL_LISTEN}{}", request.0))];
        self.notify(notification);
        let call = BackendCall::DebugListenForJvm { request, host: target.host, port: target.port };
        let stage = Stage::Listening { config: Box::new(config), module: target.module, key };
        self.ask(call, Waiting::Start(Box::new(flow), stage));
    }

    /// The remote JVM connected to the listener (the relay's port), or listening ended.
    pub fn jvm_connected(&mut self, ticket: u64, result: Result<u16, IdeError>, host: &mut Host) {
        let Some(Waiting::Start(flow, Stage::Listening { config, module, key })) = self.waiting.remove(&ticket) else {
            return;
        };
        self.effects.push(Effect::ExpireNotification(key));
        let relay = match result {
            Ok(relay) => relay,
            Err(error) if is_cancelled(&error) => return,
            Err(error) => return self.fail_start(error_text(&error), host),
        };
        let attach =
            JavaAttachTarget { name: config.name.clone(), module, host_name: "127.0.0.1".to_owned(), port: relay };
        // A listen relay accepts one debugger: rerunning listens again through the configuration.
        let options =
            StartOptions { run_config: Some(*config), rerunnable: false, show: flow.show, ..Default::default() };
        self.java_attach(&flow.root, attach, options, "Cannot start debugging", host);
    }

    /// Attach Debugger: to a JVM listening for JDWP, or debugpy to a Python program in a run
    /// target's container (arg: the target, with `process`, `runConfig` and `show`).
    fn attach(&mut self, arg: Option<&Value>, host: &mut Host) {
        let (Some(root), Some(arg)) = (host.root.map(str::to_owned), arg) else { return };
        let process = arg.get("process").and_then(Value::as_u64).and_then(|id| u32::try_from(id).ok()).map(RunId);
        let options = StartOptions {
            process,
            run_config: arg.get("runConfig").and_then(run_config),
            rerunnable: false,
            show: arg.get("show").and_then(Value::as_bool),
            ..Default::default()
        };
        if let Some(target) = PythonAttachTarget::from_arg(arg) {
            let launch = python_attach_config(&target, &root, host.env.debugger.skip_library_code);
            let mut ctx = ctx!(self, host);
            let start = self.state.start_session(launch, options, &mut ctx);
            // The container waits for the debugger: without one it has nothing to wait for.
            match (start, process) {
                (Some(start), Some(process)) => {
                    self.starts.insert(start, AfterStart::StopProcessOnFailure(process));
                }
                (None, Some(process)) => self.state.stop_associated_process(process, &host.env),
                _ => {}
            }
        } else if let Some(target) = JavaAttachTarget::from_arg(arg) {
            self.java_attach(&root, target, options, "Cannot attach the debugger", host);
        }
    }

    fn java_attach(
        &mut self,
        root: &str,
        target: JavaAttachTarget,
        options: StartOptions,
        title: &'static str,
        host: &mut Host,
    ) {
        let (flow, step) = JavaFlow::attach(target, root);
        let kind = JavaKind::Attach { options: Box::new(options), title };
        self.java_step(JavaRun { flow, kind }, step, host);
    }

    /// An Application configuration: its preparation belongs to a session from the moment the
    /// action begins.
    fn start_java(&mut self, flow: StartFlow, config: RunConfig, host: &mut Host) {
        let origin = flow.origin.unwrap_or_else(|| config.clone());
        let request = RequestId::next();
        let preparation = {
            let mut ctx = ctx!(self, host);
            let preparation = self.state.begin_preparation(origin, flow.show, &mut ctx);
            self.state.set_preparation_request(preparation, Some(request));
            preparation
        };
        let (java, step) = JavaFlow::launch(config, &flow.root, true, host.i18n);
        let kind = JavaKind::Launch { preparation, request, show: flow.show };
        self.java_step(JavaRun { flow: java, kind }, step, host);
    }

    /// Whether Stop cancelled the launch a Java flow prepares: its preparation ends, quietly.
    fn java_abandoned(&mut self, run: &JavaRun, host: &mut Host) -> bool {
        let JavaKind::Launch { preparation, .. } = run.kind else { return false };
        if !self.state.preparation_aborted(preparation) {
            return false;
        }
        let mut ctx = ctx!(self, host);
        self.state.preparation_failed(preparation, "", true, &mut ctx);
        true
    }

    /// Carries a Java flow one step on: asks for what it needs, or ends it.
    fn java_step(&mut self, mut run: JavaRun, step: JavaStep, host: &mut Host) {
        if let JavaKind::Launch { preparation, .. } = run.kind {
            for report in run.flow.take_reports() {
                self.state.preparation_report(preparation, &report.text, report.stderr, host.env.console);
            }
        }
        self.effects.extend(run.flow.take_effects());
        let wait = |run: JavaRun, wait: JavaWait| Waiting::Java(Box::new(run), wait);
        match step {
            JavaStep::SaveAll => {
                let ticket = self.wait(wait(run, JavaWait::Save));
                self.effects.push(Effect::SaveAllThen { ticket });
            }
            JavaStep::FindFiles { names, limit } => {
                let calls = names.into_iter().map(|name| BackendCall::FindFiles { name, limit }).collect();
                self.ask_all(calls, wait(run, JavaWait::Files));
            }
            JavaStep::ListConfigs => self.ask(BackendCall::RunConfigs, wait(run, JavaWait::Configs)),
            JavaStep::ReadSources { paths } => {
                let calls = paths.into_iter().map(BackendCall::FsReadText).collect();
                self.ask_all(calls, wait(run, JavaWait::Sources));
            }
            JavaStep::ResolveLaunch { module, test_classpath, policy } => {
                let request = match run.kind {
                    JavaKind::Launch { request, .. } => request,
                    JavaKind::Attach { .. } => RequestId::next(),
                };
                let call = BackendCall::JavaResolveLaunch { module, test_classpath, policy, request };
                self.ask(call, wait(run, JavaWait::Resolve));
            }
            JavaStep::ReadDir(path) => self.ask(BackendCall::FsReadDir(path), wait(run, JavaWait::Dir)),
            JavaStep::DebugServer { module } => {
                self.ask(BackendCall::JavaDebugServer { module }, wait(run, JavaWait::Server));
            }
            JavaStep::ReadProjectFiles { descriptor, pom } => {
                let calls = vec![BackendCall::FsReadText(descriptor), BackendCall::FsReadText(pom)];
                self.ask_all(calls, wait(run, JavaWait::ProjectFiles));
            }
            JavaStep::Done(launch) => {
                let mut ctx = ctx!(self, host);
                match run.kind {
                    JavaKind::Launch { preparation, show, .. } => {
                        self.state.preparation_ready(preparation, *launch, show, &mut ctx);
                    }
                    JavaKind::Attach { options, .. } => {
                        self.state.start_session(*launch, *options, &mut ctx);
                    }
                }
            }
            JavaStep::Failed(error) => match run.kind {
                JavaKind::Launch { preparation, .. } => {
                    let mut ctx = ctx!(self, host);
                    if self.state.preparation_failed(preparation, error.message(), error.is_cancelled(), &mut ctx) {
                        self.fail_start(error.message(), host);
                    }
                }
                JavaKind::Attach { title, .. } => {
                    if !error.is_cancelled() {
                        let title = host.i18n.tx(title);
                        self.report(&title, error.message(), host);
                    }
                }
            },
        }
    }

    /// Takes the Java flow that waits for `wait` under `ticket`, unless its launch was cancelled.
    fn java_waiting(&mut self, ticket: u64, wait: JavaWait, host: &mut Host) -> Option<JavaRun> {
        let Some(Waiting::Java(run, waiting)) = self.waiting.remove(&ticket) else { return None };
        (waiting == wait && !self.java_abandoned(&run, host)).then_some(*run)
    }

    /// The file index answered a Java launch's search for its main class (every answer together).
    pub fn files_found(&mut self, ticket: u64, found: Vec<FoundSource>, host: &mut Host) {
        let Some(mut run) = self.java_waiting(ticket, JavaWait::Files, host) else { return };
        let step = run.flow.files_found(found, host.i18n);
        self.java_step(run, step, host);
    }

    /// The files a Java flow asked for were read, in the order asked: the sources declaring a main
    /// class, or a module's project descriptor and POM (which may be missing).
    pub fn texts_read(&mut self, ticket: u64, texts: Vec<Result<String, IdeError>>, host: &mut Host) {
        let Some(Waiting::Java(mut run, wait)) = self.waiting.remove(&ticket) else { return };
        if !matches!(wait, JavaWait::Sources | JavaWait::ProjectFiles) || self.java_abandoned(&run, host) {
            return;
        }
        let step = if wait == JavaWait::ProjectFiles {
            let mut texts = texts.into_iter().map(Result::unwrap_or_default);
            let (descriptor, pom) = (texts.next().unwrap_or_default(), texts.next().unwrap_or_default());
            run.flow.project_files_read(&descriptor, &pom)
        } else {
            match texts.into_iter().collect::<Result<Vec<String>, IdeError>>() {
                Ok(texts) => run.flow.sources_read(&texts, host.modules, host.i18n),
                Err(error) => JavaStep::Failed(crate::run::java_launch::JavaLaunchError::Backend(error)),
            }
        };
        self.java_step(*run, step, host);
    }

    /// jdtls built and resolved a Java launch.
    pub fn launch_resolved(&mut self, ticket: u64, result: Result<JavaLaunchInfo, IdeError>, host: &mut Host) {
        let Some(mut run) = self.java_waiting(ticket, JavaWait::Resolve, host) else { return };
        let step = run.flow.launch_resolved(result, host.i18n);
        self.java_step(run, step, host);
    }

    /// A directory of a module's sources was listed (a failure counts as an empty directory).
    pub fn dir_read(&mut self, ticket: u64, entries: Result<Vec<FileEntry>, IdeError>, host: &mut Host) {
        let Some(mut run) = self.java_waiting(ticket, JavaWait::Dir, host) else { return };
        let step = run.flow.dir_read(&entries.unwrap_or_default());
        self.java_step(run, step, host);
    }

    /// jdtls' debug server started (its port), or could not.
    pub fn debug_server_started(&mut self, ticket: u64, result: Result<u16, IdeError>, host: &mut Host) {
        let Some(mut run) = self.java_waiting(ticket, JavaWait::Server, host) else { return };
        let step = run.flow.debug_server_started(result);
        self.java_step(run, step, host);
    }

    // --- start outcomes ---------------------------------------------------------------

    /// The backend answered a start.
    pub fn started(&mut self, ticket: u64, result: Result<DebugSessionId, IdeError>, host: &mut Host) {
        let outcome = {
            let mut ctx = ctx!(self, host);
            self.state.started(ticket, result, &mut ctx)
        };
        if let Some(outcome) = outcome {
            self.start_ended(outcome, host);
        }
    }

    /// The backend answered a breakpoint sync of `path`.
    pub fn breakpoints_synced(
        &mut self,
        path: &str,
        result: Result<Vec<ide_dap::BreakpointState>, IdeError>,
        host: &mut Host,
    ) {
        let outcomes = {
            let mut ctx = ctx!(self, host);
            self.state.breakpoints_synced(path, result, &mut ctx)
        };
        for outcome in outcomes {
            self.start_ended(outcome, host);
        }
    }

    fn start_ended(&mut self, outcome: StartOutcome, host: &mut Host) {
        match (self.starts.remove(&outcome.start), outcome.session) {
            (Some(AfterStart::RunPluginTarget(config)), Some(_)) => {
                let arg = serde_json::to_value(*config).ok();
                self.effects.push(Effect::ExecuteAction { id: "run.runConfig".to_owned(), arg });
            }
            (Some(AfterStart::StopProcessOnFailure(process)), None) => {
                self.state.stop_associated_process(process, &host.env);
            }
            (Some(AfterStart::Plugin(operation)), Some(session)) => {
                self.plugin_sessions.insert(session, (operation, false));
                self.observe_plugin_sessions();
            }
            (Some(AfterStart::Plugin(operation)), None) => self.plugin_failed(operation, "Debug session did not start"),
            _ => {}
        }
    }

    // --- plugins ------------------------------------------------------------------------

    fn plugin_failed(&mut self, operation: String, message: &str) {
        self.plugin_events.push(PluginEvent::Failed { operation, message: message.to_owned() });
    }

    /// Debug Plugin Configuration (arg: `{ config, operation }`): the plugin's backend prepares the
    /// configuration, then it is debugged; the plugin of `operation` hears how it went through
    /// [`Debugger::take_plugin_events`].
    fn plugin_start(&mut self, arg: Option<&Value>, host: &mut Host) {
        let operation = arg.and_then(|arg| arg.get("operation")).and_then(Value::as_str).unwrap_or_default();
        let operation = operation.to_owned();
        match arg.and_then(|arg| arg.get("config")).and_then(run_config) {
            Some(config) => {
                self.ask(BackendCall::PluginPrepareDebugRun(Box::new(config)), Waiting::PluginPrepare { operation });
            }
            None => self.plugin_failed(operation, &host.i18n.tx("This configuration has no debugger")),
        }
    }

    fn plugin_prepared(&mut self, operation: String, result: Result<RunConfig, IdeError>, host: &mut Host) {
        let config = match result {
            Ok(config) => config,
            Err(error) => return self.plugin_failed(operation, error_text(&error)),
        };
        let Some(root) = host.root else { return self.plugin_failed(operation, "No project") };
        let env = LaunchEnv { debugger: host.env.debugger, windows: host.windows };
        let Some(launch) = launch_config_for(&config, root, None, &env) else {
            return self.plugin_failed(operation, "This configuration has no debugger");
        };
        let waiting = Waiting::PluginSave { operation, config: Box::new(config), launch: Box::new(launch) };
        let ticket = self.wait(waiting);
        self.effects.push(Effect::SaveAllThen { ticket });
    }

    /// Looks at the sessions plugins started (after anything about them changed): one that is
    /// gone or has ended is reported with its exit code and whether a stop of it was seen.
    pub fn observe_plugin_sessions(&mut self) {
        let mut ended = Vec::new();
        for (id, (operation, stopped)) in &mut self.plugin_sessions {
            let session = self.state.session(*id);
            *stopped |= session.is_some_and(|s| s.status == SessionStatus::Stopping);
            if session.is_none_or(|s| s.status == SessionStatus::Terminated) {
                let code = session.and_then(|s| s.exit_code);
                ended.push(PluginEvent::Exited { operation: operation.clone(), session: *id, code, stopped: *stopped });
            }
        }
        for event in &ended {
            if let PluginEvent::Exited { session, .. } = event {
                self.plugin_sessions.remove(session);
            }
        }
        self.plugin_events.extend(ended);
    }

    // --- hot swap -----------------------------------------------------------------------

    /// Reload Changed Classes: compiles (unless `build` is false) and reloads the changed classes
    /// into the Java debug sessions, or only into the one of the run configuration `config`.
    pub fn reload_changed_classes(&mut self, build: bool, config: Option<&str>, host: &mut Host) {
        let step = self.hot_swap.reload_changed_classes(build, config, &self.state, host.env.desktop, host.env.now_ms);
        self.hot_swap_step(step);
    }

    fn hot_swap_step(&mut self, step: HotSwapStep) {
        match step {
            HotSwapStep::Done => {}
            HotSwapStep::SaveAll => {
                let ticket = self.wait(Waiting::HotSwap);
                self.effects.push(Effect::SaveAllThen { ticket });
            }
            HotSwapStep::Build { module, request } => {
                self.ask(BackendCall::JavaBuildWorkspace { module, request }, Waiting::HotSwap);
            }
            HotSwapStep::Redefine(session) => self.ask(BackendCall::DebugRedefineClasses(session), Waiting::HotSwap),
        }
    }

    /// A hot swap compile ended.
    pub fn hot_swap_built(&mut self, ticket: u64, result: Result<JavaBuildStatus, IdeError>, host: &mut Host) {
        if let Some(Waiting::HotSwap) = self.waiting.remove(&ticket) {
            let step = self.hot_swap.built(result, &mut self.state, host.env.now_ms);
            self.hot_swap_step(step);
        }
    }

    /// A session answered the reload of its classes.
    pub fn hot_swap_redefined(&mut self, ticket: u64, result: Result<HotSwapResult, IdeError>, host: &mut Host) {
        if let Some(Waiting::HotSwap) = self.waiting.remove(&ticket) {
            let step = self.hot_swap.redefined(result, &self.state, host.env.now_ms, host.i18n);
            self.hot_swap_step(step);
        }
    }

    /// A `hotcodereplace` event of java-debug arrived for `session`.
    pub fn on_hot_code_replace(&mut self, session: DebugSessionId, change_type: &str, message: &str, host: &mut Host) {
        self.hot_swap.on_hot_code_replace(session, change_type, message, &self.state, host.env.now_ms, host.i18n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::editor::quick_evaluate::{EvaluateHint, evaluated, selection_after_click};
    use crate::debugger::effects::ipc;
    use crate::debugger::effects::testing::Settings;
    use crate::debugger::focused_view::{DebugView, DebugViewCommands};
    use crate::debugger::launch::testing::{extra, rc, with_params};
    use crate::debugger::state::testing::{frame, variable};
    use crate::platform::actions::{is_action_enabled, manifest};
    use crate::platform::context::ContextKeys;
    use crate::platform::keymap;
    use ide_core::error::ErrorKind;
    use ide_core::event::DebugEvent;
    use ide_dap::DebugCapabilities;
    use ide_dap::attach::AttachKind;

    /// What the backend of a test answers a command with.
    #[derive(Debug, Clone)]
    enum Answer {
        /// Nothing yet: the command stays in [`Bench::deferred`] until [`Bench::resolve`].
        Later,
        /// Success with nothing in it; a start answers with session 42.
        Done,
        Fail(IdeError),
        Session(u32),
        Targets(Vec<StepInTarget>),
        Configs(Vec<RunConfig>),
        Php(Option<&'static str>),
        /// A PHP's version and whether it loads Xdebug; `None`: the probe failed.
        Probe(Option<(&'static str, bool)>),
        Launch(Box<JavaLaunchInfo>),
    }

    /// A debugger with its surroundings and a scripted backend.
    struct Bench {
        debugger: Debugger,
        settings: Settings,
        i18n: I18n,
        targets: RunTargets,
        now: f64,
        root: Option<&'static str>,
        run_debug_action: bool,
        /// What saving the open documents comes to.
        save: Result<(), String>,
        /// Every backend command made so far, in order.
        calls: Vec<BackendCall>,
        /// Every other effect so far, in order.
        effects: Vec<Effect>,
        /// Commands the backend has not answered yet.
        deferred: Vec<(u64, BackendCall)>,
    }

    impl Default for Bench {
        fn default() -> Self {
            Self {
                debugger: Debugger::default(),
                settings: Settings::default(),
                i18n: I18n::default(),
                targets: RunTargets::default(),
                now: 1_000_000.0,
                root: Some("/p"),
                run_debug_action: false,
                save: Ok(()),
                calls: Vec::new(),
                effects: Vec::new(),
                deferred: Vec::new(),
            }
        }
    }

    /// The debugger and the host its actions read, borrowed from the fields of a [`Bench`].
    macro_rules! parts {
        ($bench:expr) => {{
            let bench: &mut Bench = $bench;
            let host = Host {
                env: bench.settings.env(bench.now),
                i18n: &mut bench.i18n,
                root: bench.root,
                windows: false,
                run_debug_action: bench.run_debug_action,
                download_jdk_action: false,
                run_targets: &bench.targets,
                modules: &[],
            };
            (&mut bench.debugger, host)
        }};
    }

    impl Bench {
        /// Routes the answer of a command to the continuation that takes it.
        fn deliver(&mut self, ticket: u64, call: BackendCall, answer: Answer) {
            if matches!(answer, Answer::Later) {
                self.deferred.push((ticket, call));
                return;
            }
            let failed = if let Answer::Fail(error) = &answer { Some(error.clone()) } else { None };
            let done: Result<(), IdeError> = failed.clone().map_or(Ok(()), Err);
            let (debugger, mut host) = parts!(self);
            macro_rules! session_ctx {
                () => {
                    Ctx { breakpoints: &mut debugger.breakpoints, env: host.env, i18n: &mut *host.i18n }
                };
            }
            match call {
                BackendCall::DebugStackTrace { .. } => {
                    debugger.state.frames_loaded(ticket, Ok(Vec::new()), &mut session_ctx!());
                }
                BackendCall::DebugThreads(_) => debugger.threads_loaded(ticket, Ok(Vec::new()), &mut host),
                BackendCall::DebugScopes { .. } => debugger.state.scopes_loaded(ticket, Ok(Vec::new())),
                BackendCall::DebugVariables { .. } => debugger.state.variables_loaded(ticket, Ok(Vec::new())),
                BackendCall::DebugCapabilities(_) => {
                    debugger.state.capabilities_loaded(ticket, Err(IdeError::internal("no capabilities")));
                }
                BackendCall::DebugStart(_) => {
                    let session = if let Answer::Session(session) = answer { session } else { 42 };
                    debugger.started(ticket, failed.map_or(Ok(DebugSessionId(session)), Err), &mut host);
                }
                BackendCall::DebugStop(_) => debugger.state.stop_finished(ticket, done, &mut session_ctx!()),
                BackendCall::DebugSetBreakpoints { path, .. } => {
                    debugger.breakpoints_synced(&path, Ok(Vec::new()), &mut host);
                }
                BackendCall::DebugStep { .. }
                | BackendCall::DebugResume { .. }
                | BackendCall::DebugForceStepOver { .. }
                | BackendCall::DebugRunTo { .. }
                | BackendCall::DebugStepInTarget { .. }
                | BackendCall::DebugRestartFrame { .. } => debugger.resumed(ticket, done, &mut host),
                BackendCall::DebugPause { .. } | BackendCall::DebugHotReload(_) | BackendCall::DebugHotRestart(_) => {
                    debugger.reported(ticket, done, &mut host);
                }
                BackendCall::DebugStepInTargets { .. } => {
                    let targets = if let Answer::Targets(targets) = answer { targets } else { Vec::new() };
                    debugger.step_in_targets_loaded(ticket, Ok(targets), None, &mut host);
                }
                BackendCall::DebugAttachableProcesses => {
                    let node = crate::debugger::attach::testing::proc(9, AttachKind::Node);
                    debugger.attachable_processes_loaded(ticket, Ok(vec![node]));
                }
                BackendCall::RunConfigs => {
                    let configs = if let Answer::Configs(configs) = answer { configs } else { Vec::new() };
                    debugger.run_configs_loaded(ticket, Ok(configs), &mut host);
                }
                BackendCall::PluginPrepareDebugRun(config) | BackendCall::RunDotnetResolve(config) => {
                    debugger.config_prepared(ticket, Ok(*config), &mut host);
                }
                BackendCall::SdkPhpEffective => {
                    let php = if let Answer::Php(php) = answer { php } else { Some("/opt/php81/bin/php") };
                    debugger.php_resolved(ticket, php.map(str::to_owned), &mut host);
                }
                BackendCall::SdkLocateTools { .. } => debugger.php_located(ticket, Some("/usr/bin/php".to_owned())),
                BackendCall::SdkPhpXdebug { php, .. } => {
                    let probe = if let Answer::Probe(probe) = answer { probe } else { Some(("8.1.33", false)) };
                    let probe = probe.map(|(version, loaded)| (php.as_str(), version, loaded));
                    debugger.xdebug_probed(ticket, probe, &mut host);
                }
                BackendCall::JavaResolveLaunch { .. } => {
                    let info = if let Answer::Launch(info) = answer { *info } else { resolved() };
                    debugger.launch_resolved(ticket, failed.map_or(Ok(info), Err), &mut host);
                }
                BackendCall::FsReadDir(_) => debugger.dir_read(ticket, Ok(Vec::new()), &mut host),
                BackendCall::JavaDebugServer { .. } => debugger.debug_server_started(ticket, Ok(5005), &mut host),
                BackendCall::JavaBuildWorkspace { .. } => {
                    debugger.hot_swap_built(ticket, Ok(JavaBuildStatus::Succeeded), &mut host);
                }
                BackendCall::DebugRedefineClasses(_) => {
                    let reloaded = HotSwapResult { changed_classes: vec!["a.Main".to_owned()], ..Default::default() };
                    debugger.hot_swap_redefined(ticket, Ok(reloaded), &mut host);
                }
                _ => {}
            }
        }

        /// Carries out what the debugger asked for until it asks for nothing more.
        fn pump(&mut self, backend: &mut dyn FnMut(&BackendCall) -> Answer) {
            loop {
                let effects = self.debugger.take_effects();
                if effects.is_empty() {
                    return;
                }
                for effect in effects {
                    match effect {
                        Effect::Ask { ticket, call } => {
                            self.calls.push(call.clone());
                            let answer = backend(&call);
                            self.deliver(ticket, call, answer);
                        }
                        Effect::Call(call) => {
                            self.calls.push(call.clone());
                            self.deliver(0, call, Answer::Done);
                        }
                        Effect::AskAll { ticket, calls } => {
                            self.calls.extend(calls.iter().cloned());
                            let files = matches!(calls.first(), Some(BackendCall::FindFiles { .. }));
                            let (debugger, mut host) = parts!(self);
                            if files {
                                debugger.files_found(ticket, Vec::new(), &mut host);
                            } else {
                                let texts = calls.iter().map(|_| Ok(String::new())).collect();
                                debugger.texts_read(ticket, texts, &mut host);
                            }
                        }
                        Effect::SaveAllThen { ticket } => {
                            let saved = self.save.clone();
                            let (debugger, mut host) = parts!(self);
                            debugger.saved(ticket, saved, &mut host);
                        }
                        other => self.effects.push(other),
                    }
                }
            }
        }

        fn settle(&mut self) {
            self.pump(&mut |_| Answer::Done);
        }

        /// Answers the first unanswered command `wanted` accepts.
        fn resolve(&mut self, wanted: impl Fn(&BackendCall) -> bool, answer: Answer) {
            let at = self.deferred.iter().position(|(_, call)| wanted(call)).expect("an unanswered command");
            let (ticket, call) = self.deferred.remove(at);
            self.deliver(ticket, call, answer);
        }

        fn run(&mut self, action: &str, arg: Option<Value>, editor: Option<&Doc>) -> Dispatch {
            let (debugger, mut host) = parts!(self);
            let editor = editor.map(Doc::editor);
            debugger.run_action(action, arg.as_ref(), editor.as_ref(), &mut host)
        }

        fn count(&self, wanted: impl Fn(&BackendCall) -> bool) -> usize {
            self.calls.iter().filter(|call| wanted(call)).count()
        }

        fn notifications(&self) -> Vec<&NotificationRequest> {
            self.effects.iter().filter_map(Effect::notification).collect()
        }

        fn session(&self, id: u32) -> &DebugSession {
            self.debugger.state.session(DebugSessionId(id)).expect("the session")
        }

        fn console(&self, id: DebugSessionId) -> String {
            self.debugger.state.console(id).iter().map(|line| line.text.as_str()).collect()
        }

        /// Session 3, suspended in `run` (selected) called from `parse` (top), as the only session.
        fn paused(&mut self, restart_frame: bool, step_in_targets: bool) {
            self.debugger.state = DebugState::default();
            self.debugger.state.adopt_session(DebugSessionId(3), self.now, |s| {
                s.name = "app".to_owned();
                s.adapter = "debugpy".to_owned();
                s.status = SessionStatus::Paused;
                s.stopped_thread = Some(1);
                s.stop_reason = Some("breakpoint".to_owned());
                s.threads = vec![ThreadInfo { id: 1, name: "main".to_owned() }];
                s.selected_thread = Some(1);
                let mut top = frame(70, "parse", "/p/app.py", 2);
                top.column = 4;
                s.set_frames(vec![top, frame(71, "run", "/p/app.py", 9)]);
                s.selected_frame = Some(71);
                s.hits = 1;
                s.caps = Some(DebugCapabilities { restart_frame, step_in_targets, ..Default::default() });
            });
            self.debugger.state.update_execution_point();
            self.debugger.state.take_effects();
        }
    }

    /// A document in the focused editor.
    struct Doc {
        path: &'static str,
        language: &'static str,
        rope: Rope,
        selection: (u32, u32),
        head: u32,
    }

    impl Doc {
        fn editor(&self) -> ActiveEditor<'_> {
            ActiveEditor {
                path: self.path,
                language: self.language,
                doc: &self.rope,
                selection: self.selection,
                head: self.head,
                comments: &[],
            }
        }
    }

    /// An editor of `text` with the caret at the start of `line`.
    fn caret(path: &'static str, language: &'static str, text: &str, line: u32) -> Doc {
        let rope = Rope::from_str(text);
        let at = buffer::line_range(&rope, line).start;
        Doc { path, language, rope, selection: (at, at), head: at }
    }

    fn selecting(text: &str, from: u32, to: u32) -> Doc {
        Doc { path: "/p/a.py", language: "python", rope: Rope::from_str(text), selection: (from, to), head: to }
    }

    fn resolved() -> JavaLaunchInfo {
        JavaLaunchInfo {
            build: Some(JavaBuildStatus::Succeeded),
            class_paths: vec!["/p/target/classes".to_owned()],
            path_list_separator: ":".to_owned(),
            java_exec: Some("java".to_owned()),
            ..Default::default()
        }
    }

    fn server() -> RunConfig {
        let mut config = rc("node", "node", &["server.js"], Some("js-debug"));
        config.id = "node:server".to_owned();
        config.name = "server".to_owned();
        config
    }

    fn java_main(module: &str) -> RunConfig {
        let mut config = rc("application", "java", &["-cp", "${classpath}", "Main"], Some("java-debug"));
        config.id = "application:Main".to_owned();
        config.name = "Main".to_owned();
        config.extras.module = Some(module.to_owned());
        config
    }

    fn as_arg(config: &RunConfig) -> Value {
        serde_json::to_value(config).expect("a configuration as JSON")
    }

    fn is_start(call: &BackendCall) -> bool {
        matches!(call, BackendCall::DebugStart(_))
    }

    fn is_resolve(call: &BackendCall) -> bool {
        matches!(call, BackendCall::JavaResolveLaunch { .. })
    }

    // --- actions of the debugger (IDEA parity) ---------------------------------------------

    #[test]
    fn registers_stable_ids_with_ideas_macos_shortcuts() {
        let expected: [(&str, &[&str]); 12] = [
            ("debug.attachToProcess", &["Alt+Shift+F5"]),
            ("debug.toggleTemporaryBreakpoint", &["Meta+Alt+Shift+F8"]),
            ("debug.forceStepOver", &["Alt+Shift+F8"]),
            ("debug.forceStepInto", &["Alt+Shift+F7"]),
            ("debug.smartStepInto", &["Shift+F7"]),
            ("debug.forceRunToCursor", &["Meta+Alt+F9"]),
            ("debug.showExecutionPoint", &["Alt+F10"]),
            ("debug.setValue", &["F2"]),
            ("debug.jumpToSource", &["F4", "Meta+Down"]),
            ("debug.resetFrame", &[]),
            ("debug.toggleMethodBreakpoint", &[]),
            ("debug.toggleFieldWatchpoint", &[]),
        ];
        let preset = keymap::preset("intellij-macos").expect("the macOS preset");
        let mut bench = Bench::default();
        for (id, keys) in expected {
            assert!(manifest().contains_key(id), "{id}");
            let bound = preset.get(id).map(Vec::as_slice).unwrap_or_default();
            assert_eq!(bound, keys, "{id}");
            assert!(!matches!(bench.run(id, None, None), Dispatch::Unknown), "{id}");
        }
        // F2 / F4 only act inside the debugger views, so the editor keeps its own.
        assert_eq!(manifest()["debug.setValue"].key_when.as_deref(), Some("debugVariablesFocus"));
        let jump = manifest()["debug.jumpToSource"].key_when.as_deref();
        assert_eq!(jump, Some("debugFramesFocus || debugVariablesFocus"));
        assert!(matches!(bench.run("editor.copy", None, None), Dispatch::Unknown));
    }

    #[test]
    fn each_resuming_action_clears_the_old_execution_point_before_its_request() {
        /// An action and what tells its request from the others.
        type Case = (&'static str, fn(&BackendCall) -> bool);
        let cases: [Case; 10] = [
            ("debug.resume", |call| matches!(call, BackendCall::DebugResume { .. })),
            ("debug.stepOver", |call| matches!(call, BackendCall::DebugStep { kind: StepKind::Over, .. })),
            ("debug.stepInto", |call| matches!(call, BackendCall::DebugStep { kind: StepKind::Into, .. })),
            ("debug.stepOut", |call| matches!(call, BackendCall::DebugStep { kind: StepKind::Out, .. })),
            ("debug.forceStepOver", |call| matches!(call, BackendCall::DebugForceStepOver { .. })),
            ("debug.forceStepInto", |call| matches!(call, BackendCall::DebugStep { kind: StepKind::Into, .. })),
            ("debug.smartStepInto", |call| matches!(call, BackendCall::DebugStep { kind: StepKind::Into, .. })),
            ("debug.runToCursor", |call| matches!(call, BackendCall::DebugRunTo { force: false, .. })),
            ("debug.forceRunToCursor", |call| matches!(call, BackendCall::DebugRunTo { force: true, .. })),
            ("debug.resetFrame", |call| matches!(call, BackendCall::DebugRestartFrame { .. })),
        ];
        for (action, expected) in cases {
            let mut bench = Bench::default();
            bench.paused(true, false);
            assert!(bench.debugger.state.execution_point().path.is_some());
            bench.run(action, None, Some(&caret("/p/app.py", "python", "a\nb\nc", 2)));
            // What the backend finds when the request reaches it.
            assert!(bench.debugger.state.execution_point().path.is_none(), "{action}");
            assert_eq!(bench.session(3).status, SessionStatus::Running, "{action}");
            bench.settle();
            assert_eq!(bench.count(expected), 1, "{action}");
        }
    }

    #[test]
    fn force_step_over_force_step_into_and_force_run_to_cursor() {
        let editor = caret("/p/app.py", "python", "a\nb\nc\nd", 2);
        let mut bench = Bench::default();
        for action in ["debug.forceStepOver", "debug.forceStepInto", "debug.forceRunToCursor", "debug.runToCursor"] {
            bench.paused(false, false);
            bench.run(action, None, Some(&editor));
            bench.settle();
        }
        let session = DebugSessionId(3);
        assert!(matches!(
            bench.calls.as_slice(),
            [
                BackendCall::DebugForceStepOver { session: a, thread: 1 },
                BackendCall::DebugStep { session: b, thread: 1, kind: StepKind::Into },
                BackendCall::DebugRunTo { session: c, thread: 1, path: forced, line: 2, force: true },
                BackendCall::DebugRunTo { session: d, thread: 1, path, line: 2, force: false },
            ] if [a, b, c, d].iter().all(|id| **id == session) && forced == "/p/app.py" && path == "/p/app.py"
        ));
    }

    #[test]
    fn smart_step_into_chooser_for_several_calls_direct_step_for_one_step_into_without_support() {
        let target = |id: i64, label: &str| {
            let label = label.to_owned();
            StepInTarget { id, label, line: None, column: None, end_column: None }
        };
        let mut bench = Bench::default();
        bench.paused(false, true);
        let several = vec![target(1, "str(i)"), target(2, "parse(str(i))")];
        bench.run("debug.smartStepInto", None, None);
        bench.pump(&mut |call| match call {
            BackendCall::DebugStepInTargets { frame: 71, .. } => Answer::Targets(several.clone()),
            _ => Answer::Done,
        });
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugStepInTargets { frame: 71, .. })), 1);
        let chooser = bench.debugger.overlays.smart_step.as_ref().expect("the chooser");
        let labels: Vec<&str> = chooser.targets.iter().map(|target| target.label.as_str()).collect();
        assert_eq!(labels, ["str(i)", "parse(str(i))"]);
        assert!(chooser.inline.is_none());

        bench.paused(false, true);
        bench.run("debug.smartStepInto", None, None);
        bench.pump(&mut |call| match call {
            BackendCall::DebugStepInTargets { .. } => Answer::Targets(vec![target(5, "parse()")]),
            _ => Answer::Done,
        });
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugStepInTarget { thread: 1, target: 5, .. })), 1);

        bench.paused(false, false);
        bench.run("debug.smartStepInto", None, None);
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugStep { kind: StepKind::Into, .. })), 1);
    }

    #[test]
    fn reset_frame_restarts_the_selected_frame_when_the_adapter_can() {
        let mut bench = Bench::default();
        bench.paused(false, false);
        bench.run("debug.resetFrame", None, None);
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugRestartFrame { .. })), 0);
        assert_eq!(bench.notifications()[0].title, "Reset Frame is not supported by debugpy");
        bench.paused(true, false);
        bench.run("debug.resetFrame", None, None);
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugRestartFrame { frame: 71, .. })), 1);
    }

    #[test]
    fn show_execution_point_reveals_the_top_frame_with_focus() {
        let mut bench = Bench::default();
        bench.paused(false, false);
        bench.run("debug.showExecutionPoint", None, None);
        bench.settle();
        assert_eq!(bench.session(3).selected_frame, Some(70));
        let opened = bench.effects.iter().rev().find_map(|effect| match effect {
            Effect::OpenFile { path, focus, .. } => Some((path.as_str(), *focus)),
            _ => None,
        });
        assert_eq!(opened, Some(("/p/app.py", true)));
    }

    #[test]
    fn jump_to_source_and_set_value_go_to_the_focused_debugger_view() {
        let mut bench = Bench::default();
        bench.paused(false, false);
        let view = DebugViewCommands { view: DebugView::Variables, set_value: true, jump_to_source: true };
        bench.debugger.focused.focus(view);
        assert!(matches!(bench.run("debug.setValue", None, None), Dispatch::View(ViewCommand::SetValue)));
        assert!(matches!(bench.run("debug.jumpToSource", None, None), Dispatch::View(ViewCommand::JumpToSource)));
        bench.debugger.focused.blur(DebugView::Variables);
        // No view focused: the selected frame's source.
        assert!(matches!(bench.run("debug.jumpToSource", None, None), Dispatch::Done));
        bench.settle();
        assert!(matches!(
            bench.effects.last(),
            Some(Effect::OpenFile { path, line: 9, focus: true, .. }) if path == "/p/app.py"
        ));
    }

    #[test]
    fn puts_no_new_breakpoint_on_a_blank_line() {
        let text = "x = 1\n\n   \ny = 2";
        let mut bench = Bench::default();
        for action in ["debug.toggleBreakpoint", "debug.toggleTemporaryBreakpoint"] {
            for line in [1, 2] {
                bench.run(action, None, Some(&caret("/p/a.py", "python", text, line)));
            }
            assert!(bench.debugger.breakpoints.lines.breakpoints_of("/p/a.py").is_empty(), "{action}");
        }
        // One placed before the line went blank can still be toggled off.
        bench.debugger.breakpoints.lines.toggle_breakpoint("/p/a.py", 1, 0.0);
        bench.run("debug.toggleBreakpoint", None, Some(&caret("/p/a.py", "python", text, 1)));
        assert!(bench.debugger.breakpoints.lines.breakpoints_of("/p/a.py").is_empty());
    }

    #[test]
    fn toggles_temporary_method_and_field_breakpoints_at_the_caret() {
        let python = "x = 1\ndef run():\n  pass";
        let mut bench = Bench::default();
        bench.run("debug.toggleTemporaryBreakpoint", None, Some(&caret("/p/a.py", "python", python, 0)));
        let temporary = &bench.debugger.breakpoints.lines.breakpoints_of("/p/a.py")[0];
        assert!(temporary.line == 0 && temporary.temporary);

        let def = caret("/p/a.py", "python", python, 1);
        bench.run("debug.toggleMethodBreakpoint", None, Some(&def));
        let method = &bench.debugger.breakpoints.special.methods()[0];
        assert_eq!((method.name.as_str(), method.path.as_deref(), method.line), ("run", Some("/p/a.py"), Some(1)));
        bench.run("debug.toggleMethodBreakpoint", None, Some(&def));
        assert!(bench.debugger.breakpoints.special.methods().is_empty());

        let field = caret("/p/A.java", "java", "package a;\nclass A {\n  private int count;\n}", 2);
        bench.run("debug.toggleFieldWatchpoint", None, Some(&field));
        let watchpoint = &bench.debugger.breakpoints.special.fields()[0];
        assert_eq!(watchpoint.data_id, "a.A#count");
        assert_eq!(watchpoint.access_type.map(|access| access.as_str()), Some("write"));
        // Toggle Line Breakpoint on a Java declaration toggles the watchpoint off again.
        bench.run("debug.toggleBreakpoint", None, Some(&field));
        assert!(bench.debugger.breakpoints.special.fields().is_empty());
        // A line without a declaration says so.
        bench.run("debug.toggleMethodBreakpoint", None, Some(&caret("/p/a.py", "python", python, 0)));
        bench.settle();
        assert_eq!(bench.notifications()[0].title, "No method declaration on this line");
    }

    #[test]
    fn attach_to_process_opens_the_popup_and_loads_the_process_list() {
        let mut bench = Bench::default();
        bench.run("debug.attachToProcess", None, None);
        assert_eq!(bench.debugger.overlays.attach, Some(AttachPopup::default()));
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugAttachableProcesses)), 1);
        let processes = bench.debugger.overlays.attach.as_ref().and_then(|popup| popup.processes.as_ref());
        assert_eq!(processes.map(|list| list.iter().map(|p| p.pid).collect::<Vec<_>>()), Some(vec![9]));
    }

    #[test]
    fn evaluate_prefills_the_expression_at_the_caret_or_the_whole_selection() {
        let text = "total = order.items\nfor i in x:\n  pass";
        let mut bench = Bench::default();
        bench.paused(false, false);
        let at = text.find("items").expect("items") as u32 + 2;
        bench.run("debug.evaluate", None, Some(&selecting(text, at, at)));
        assert_eq!(bench.debugger.overlays.evaluate.as_deref(), Some("order.items"));
        let end = text.find("\n  pass").expect("the last line") as u32;
        bench.run("debug.evaluate", None, Some(&selecting(text, 0, end)));
        assert_eq!(bench.debugger.overlays.evaluate.as_deref(), Some("total = order.items\nfor i in x:"));
    }

    // --- PHP debugging without Xdebug -------------------------------------------------------

    fn php(command: &str, cwd: Option<&str>) -> Value {
        let mut config = rc("php", command, &["index.php"], Some("php-debug"));
        config.id = "php:index".to_owned();
        config.name = "index.php".to_owned();
        config.cwd = cwd.map(ipc);
        as_arg(&config)
    }

    /// The PHP interpreters probed for Xdebug, each with the folder it was probed in.
    fn probes(bench: &Bench) -> Vec<(&str, Option<&str>)> {
        let probes = bench.calls.iter().filter_map(|call| match call {
            BackendCall::SdkPhpXdebug { php, cwd } => Some((php.as_str(), cwd.as_deref())),
            _ => None,
        });
        probes.collect()
    }

    /// The PHP the first started session runs, when its launch names one.
    fn runtime(bench: &Bench) -> Option<Value> {
        let started = bench.calls.iter().find_map(|call| match call {
            BackendCall::DebugStart(launch) => Some(extra(launch)),
            _ => None,
        });
        started.and_then(|extra| extra.get("runtimeExecutable").cloned())
    }

    #[test]
    fn warns_that_breakpoints_are_skipped_and_still_starts_the_session() {
        let mut bench = Bench::default();
        bench.run("debug.startConfig", Some(php("php", None)), None);
        bench.settle();
        // In the session's folder, where a version manager's shim picks the PHP.
        assert_eq!(probes(&bench), [("/opt/php81/bin/php", Some("/p"))]);
        assert_eq!(bench.count(is_start), 1);
        assert_eq!(runtime(&bench), Some(json!("/opt/php81/bin/php")));
        let notifications = bench.notifications();
        assert_eq!(notifications.len(), 1);
        let n = notifications[0];
        assert_eq!((n.group.as_deref(), n.level), (Some("run"), NotificationLevel::Warning));
        assert_eq!(n.key.as_deref(), Some("debug.phpXdebug"));
        assert_eq!(n.title, "Xdebug is not loaded in PHP 8.1.33");
        let body = n.body.as_deref().unwrap_or_default();
        assert!(body.contains("/opt/php81/bin/php") && body.contains("Xdebug 3"));
        let labels: Vec<&str> = n.actions.iter().map(|action| action.label.as_str()).collect();
        assert_eq!(labels, ["Open PHP Settings"]);
        let command = n.actions[0].command.clone();
        let (debugger, mut host) = parts!(&mut bench);
        assert!(debugger.run_notification_command(&command, &mut host));
        assert!(matches!(
            debugger.take_effects().as_slice(),
            [Effect::ExecuteAction { id, arg: Some(page) }] if id == "file.settings" && *page == json!("languages.php")
        ));
    }

    #[test]
    fn does_not_warn_when_the_php_loads_xdebug_or_the_check_fails() {
        let mut bench = Bench::default();
        for probe in [Some(("8.3.14", true)), None] {
            bench.run("debug.startConfig", Some(php("php", None)), None);
            bench.pump(&mut |call| match call {
                BackendCall::SdkPhpXdebug { .. } => Answer::Probe(probe),
                _ => Answer::Done,
            });
        }
        assert_eq!(probes(&bench).len(), 2);
        assert_eq!(bench.count(is_start), 2);
        assert!(bench.notifications().is_empty());
    }

    #[test]
    fn checks_the_php_on_the_search_path_for_the_default_interpreter() {
        let mut bench = Bench::default();
        bench.run("debug.startConfig", Some(php("php", None)), None);
        bench.pump(&mut |call| match call {
            BackendCall::SdkPhpEffective => Answer::Php(None),
            _ => Answer::Done,
        });
        assert_eq!(bench.count(|call| matches!(call, BackendCall::SdkLocateTools { names } if *names == ["php"])), 1);
        assert_eq!(probes(&bench), [("/usr/bin/php", Some("/p"))]);
        assert_eq!(runtime(&bench), None);
        assert!(bench.notifications()[0].body.as_deref().is_some_and(|body| body.contains("/usr/bin/php")));
    }

    #[test]
    fn checks_the_php_the_command_names_in_the_configurations_working_directory() {
        let mut bench = Bench::default();
        bench.run("debug.startConfig", Some(php("/opt/php83/bin/php", Some("web"))), None);
        bench.settle();
        assert_eq!(probes(&bench), [("/opt/php83/bin/php", Some("/p/web"))]);
        assert_eq!(bench.count(|call| matches!(call, BackendCall::SdkLocateTools { .. })), 0);
    }

    // --- output class breakpoint restrictions ------------------------------------------------

    #[test]
    fn blocks_breakpoints_and_run_to_cursor_for_a_source_less_output_class() {
        let actions = [
            "debug.toggleBreakpoint",
            "debug.toggleTemporaryBreakpoint",
            "debug.toggleMethodBreakpoint",
            "debug.toggleFieldWatchpoint",
            "debug.runToCursor",
            "debug.forceRunToCursor",
        ];
        let path = "file:///p/target/classes/p/C.class";
        for action in actions {
            let mut bench = Bench::default();
            bench.paused(false, false);
            bench.run(action, None, Some(&caret(path, "java", "class C {\n int n;\n void run() {}\n}", 2)));
            bench.settle();
            let breakpoints = &bench.debugger.breakpoints;
            assert!(breakpoints.lines.breakpoints_of(path).is_empty(), "{action}");
            assert!(breakpoints.special.methods().is_empty() && breakpoints.special.fields().is_empty(), "{action}");
            assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugRunTo { .. })), 0, "{action}");
            assert!(bench.effects.iter().any(|effect| matches!(effect, Effect::StatusInfo(_))), "{action}");
        }
    }

    #[test]
    fn stops_the_waiting_python_container_if_attaching_fails() {
        let mut bench = Bench::default();
        let arg = json!({
            "name": "container", "adapter": "debugpy", "hostName": "127.0.0.1", "port": 41234, "remoteRoot": "/app",
            "process": 9,
        });
        bench.run("debug.attach", Some(arg), None);
        let closed = Answer::Fail(IdeError::internal("connection closed"));
        bench.pump(&mut |call| if is_start(call) { closed.clone() } else { Answer::Done });
        assert_eq!(bench.count(|call| matches!(call, BackendCall::RunStop(RunId(9)))), 1);
    }

    #[test]
    fn reports_java_debug_save_failures_and_does_not_start_the_adapter() {
        let mut bench = Bench { save: Err("save failed".to_owned()), ..Default::default() };
        let mut config = java_main("");
        config.args = vec!["a.Main".to_owned()];
        bench.run("debug.startConfig", Some(as_arg(&config)), None);
        bench.settle();
        assert_eq!(bench.count(is_start), 0);
        let failure = bench.notifications().into_iter().find(|n| n.level == NotificationLevel::Error);
        assert_eq!(failure.and_then(|n| n.body.as_deref()), Some("save failed"));
    }

    // --- cancelling a Java launch --------------------------------------------------------------

    /// Starts debugging `Main`, with jdtls still resolving the launch.
    fn resolving(module: &str) -> (Bench, DebugSessionId) {
        let mut bench = Bench::default();
        let config = java_main(module);
        bench.run("debug.startConfig", Some(as_arg(&config)), None);
        bench.pump(&mut |call| if is_resolve(call) { Answer::Later } else { Answer::Done });
        assert_eq!(bench.count(is_resolve), 1);
        let preparation = *bench.debugger.state.sessions().keys().next().expect("the preparation session");
        let session = bench.debugger.state.session(preparation).expect("the preparation session");
        assert_eq!((session.name.as_str(), session.status), ("Main", SessionStatus::Starting));
        assert_eq!(session.run_config.as_ref(), Some(&config));
        assert!(bench.console(preparation).contains("Building and resolving the classpath of Main…"));
        let published = bench.debugger.state.published();
        assert_eq!(published.sessions[0].config_id.as_deref(), Some("application:Main"));
        (bench, preparation)
    }

    /// Stop, while jdtls still resolves the launch.
    fn start_and_stop() -> Bench {
        let (mut bench, preparation) = resolving("");
        bench.run("debug.stop", None, None);
        bench.settle();
        let request = bench.calls.iter().find_map(|call| match call {
            BackendCall::JavaResolveLaunch { request, .. } => Some(*request),
            _ => None,
        });
        let cancelled = |call: &BackendCall| matches!(call, BackendCall::CancelRequest(id) if Some(*id) == request);
        assert_eq!(bench.count(cancelled), 1);
        assert_eq!(bench.debugger.state.session(preparation).map(|s| s.status), Some(SessionStatus::Terminated));
        assert!(bench.debugger.state.published().sessions.is_empty());
        bench
    }

    fn expect_no_launch(bench: &Bench) {
        assert_eq!(bench.count(|call| matches!(call, BackendCall::JavaDebugServer { .. })), 0);
        assert_eq!(bench.count(is_start), 0);
        assert!(bench.debugger.state.sessions().values().all(|s| s.status == SessionStatus::Terminated));
        assert!(bench.notifications().is_empty());
    }

    #[test]
    fn stop_cancels_the_resolve_request_and_ends_the_preparation_session_without_starting_dap() {
        let mut bench = start_and_stop();
        bench.resolve(is_resolve, Answer::Fail(IdeError::cancelled()));
        bench.settle();
        expect_no_launch(&bench);
    }

    #[test]
    fn ignores_a_successful_resolve_arriving_after_stop_including_compile_warnings() {
        let mut bench = start_and_stop();
        let with_errors = JavaLaunchInfo { error_count: 1, ..resolved() };
        bench.resolve(is_resolve, Answer::Launch(Box::new(with_errors)));
        bench.settle();
        expect_no_launch(&bench);
    }

    #[test]
    fn ignores_a_failed_resolve_arriving_after_stop() {
        let mut bench = start_and_stop();
        bench.resolve(is_resolve, Answer::Fail(IdeError::internal("Build failed")));
        bench.settle();
        expect_no_launch(&bench);
    }

    #[test]
    fn still_reports_a_real_resolve_failure_in_the_preparation_console() {
        let (mut bench, preparation) = resolving("");
        bench.resolve(is_resolve, Answer::Fail(IdeError::internal("Build failed")));
        bench.settle();
        let n = *bench.notifications().last().expect("a notification");
        assert_eq!((n.level, n.title.as_str()), (NotificationLevel::Error, "Cannot start debugging"));
        assert_eq!(n.body.as_deref(), Some("Build failed"));
        assert!(bench.console(preparation).contains("Build failed"));
        assert_eq!(bench.count(is_start), 0);
    }

    #[test]
    fn explains_how_to_import_a_module_when_debug_fails_because_it_is_not_imported() {
        for message in [
            "jdtls: Given URI does not belong to any Java project.",
            "jdtls: Launch configuration 1793460461218 references non-existing project parent.",
        ] {
            let (mut bench, preparation) = resolving("web");
            bench.resolve(is_resolve, Answer::Fail(IdeError::new(ErrorKind::Protocol, message)));
            bench.settle();
            let n = *bench.notifications().last().expect("a notification");
            assert_eq!(n.title, "Cannot start debugging");
            assert!(n.body.as_deref().is_some_and(|body| body.contains("Java module 'web' is not imported.")));
            assert!(bench.console(preparation).contains("open its folder as a project"));
            assert_eq!(bench.count(|call| matches!(call, BackendCall::JavaDebugServer { .. })), 0);
            assert_eq!(bench.count(is_start), 0);
        }
    }

    #[test]
    fn keeps_progress_and_the_originating_configuration_when_dap_takes_over() {
        let (mut bench, _) = resolving("");
        bench.resolve(is_resolve, Answer::Done);
        bench.settle();
        let sessions: Vec<DebugSessionId> = bench.debugger.state.sessions().keys().copied().collect();
        assert_eq!(sessions, [DebugSessionId(42)]);
        assert_eq!(bench.session(42).status, SessionStatus::Running);
        assert_eq!(bench.session(42).run_config, Some(java_main("")));
        assert!(bench.console(DebugSessionId(42)).contains("Building and resolving the classpath of Main…"));
    }

    #[test]
    fn handles_a_late_dap_answer_after_preparation_was_cancelled() {
        for success in [true, false] {
            let (mut bench, _) = resolving("");
            bench.resolve(is_resolve, Answer::Done);
            bench.pump(&mut |call| if is_start(call) { Answer::Later } else { Answer::Done });
            assert_eq!(bench.count(is_start), 1);
            bench.run("debug.stop", None, None);
            bench.settle();
            let failure = Answer::Fail(IdeError::internal("Adapter start failed"));
            bench.resolve(is_start, if success { Answer::Session(42) } else { failure });
            bench.settle();
            let stopped = bench.count(|call| matches!(call, BackendCall::DebugStop(DebugSessionId(42))));
            assert_eq!(stopped, usize::from(success));
            assert!(bench.notifications().is_empty());
            assert!(bench.debugger.state.published().sessions.is_empty());
        }
    }

    // --- the originating run configuration ---------------------------------------------------

    /// A session of the `server` configuration, started through Debug….
    fn started(arg: Value) -> Bench {
        let mut bench = Bench::default();
        bench.run("debug.startConfig", Some(arg), None);
        bench.settle();
        bench
    }

    fn rerun(bench: &mut Bench, show: Option<bool>) -> Option<Value> {
        {
            let (debugger, host) = parts!(bench);
            let mut ctx = Ctx { breakpoints: &mut debugger.breakpoints, env: host.env, i18n: &mut *host.i18n };
            debugger.state.rerun_session(DebugSessionId(42), show, &mut ctx);
        }
        bench.settle();
        bench.effects.iter().rev().find_map(|effect| match effect {
            Effect::ExecuteAction { id, arg } if id == "run.debugConfig" => arg.clone(),
            _ => None,
        })
    }

    #[test]
    fn keeps_the_run_config_through_debug_start_config_and_reruns_through_its_debug_preparation() {
        let mut bench = started(as_arg(&server()));
        assert_eq!(bench.session(42).run_config, Some(server()));
        assert!(bench.session(42).started_at.is_some_and(|at| at > 0.0));
        assert_eq!(bench.debugger.state.published().sessions[0].config_id.as_deref(), Some("node:server"));
        assert_eq!(rerun(&mut bench, None), Some(as_arg(&server())));
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugStop(DebugSessionId(42)))), 1);
    }

    #[test]
    fn reruns_a_debug_service_in_place_without_showing_the_debug_tool_window() {
        let mut bench = started(as_arg(&server()));
        assert_eq!(rerun(&mut bench, Some(false)), Some(json!({ "config": as_arg(&server()), "show": false })));
    }

    #[test]
    fn preserves_the_source_configuration_for_a_separately_built_debug_binary() {
        let origin = RunConfig { id: "source:server".to_owned(), ..server() };
        let bench = started(json!({ "config": as_arg(&server()), "origin": as_arg(&origin) }));
        assert_eq!(bench.session(42).run_config.as_ref().map(|config| config.id.as_str()), Some("source:server"));
    }

    #[test]
    fn keeps_the_originating_jvm_configuration_when_attaching_to_its_build_process() {
        let mut bench = Bench::default();
        let arg = json!({
            "name": "server", "module": "", "hostName": "127.0.0.1", "port": 5005, "process": 7,
            "runConfig": as_arg(&server()),
        });
        bench.run("debug.attach", Some(arg), None);
        bench.settle();
        let session = bench.session(42);
        assert_eq!((session.run_config.as_ref(), session.process), (Some(&server()), Some(RunId(7))));
        assert!(session.launch.is_none());
        assert!(bench.debugger.state.published().sessions[0].can_rerun);
    }

    // --- Quick Evaluate Expression -----------------------------------------------------------

    const EXPRESSION: &str = "total + order.items";

    /// Session 7 in `status`, with frame 10 selected.
    fn evaluating(adapter: &str, status: SessionStatus) -> Bench {
        let mut bench = Bench::default();
        bench.debugger.state.adopt_session(DebugSessionId(7), bench.now, |s| {
            s.adapter = adapter.to_owned();
            s.status = status;
            s.selected_frame = Some(10);
        });
        bench.debugger.state.take_effects();
        bench
    }

    fn quick_evaluate_enabled(bench: &Bench) -> bool {
        let keys = bench.debugger.state.published().context.entries();
        let context: ContextKeys = keys.into_iter().chain([("textEditorActive", true)]).collect();
        is_action_enabled(&manifest()["debug.quickEvaluate"], &context)
    }

    #[test]
    fn evaluates_the_clicked_member_through_each_adapter() {
        for adapter in ["java-debug", "js-debug", "debugpy"] {
            let mut bench = evaluating(adapter, SessionStatus::Paused);
            assert!(quick_evaluate_enabled(&bench));
            // The click lands outside the selection, so the caret moves to it.
            let editor = selecting(EXPRESSION, 0, 5);
            assert_eq!(selection_after_click(editor.selection, 16), (16, 16));
            let clicked = bench.run("debug.quickEvaluate", Some(json!({ "pos": 16 })), Some(&editor));
            let Dispatch::QuickEvaluate(QuickEvaluation::Evaluate { call, expression, pos }) = clicked else {
                panic!("an evaluation");
            };
            assert_eq!((expression.as_str(), pos), ("order.items", 16));
            let BackendCall::DebugEvaluateIn { session, frame, expression, context } = call else {
                panic!("an evaluate request");
            };
            assert_eq!((session, frame, context), (DebugSessionId(7), Some(10), EvaluateContext::Hover));
            assert_eq!(expression, "order.items");
            let EvaluateHint::Value { hint, pos: 16, .. } = evaluated(&expression, Ok(&variable("result", "42")), pos)
            else {
                panic!("a value hint at the click");
            };
            assert_eq!((hint.name.as_str(), hint.value.as_str()), ("order.items = ", "42"));
        }
    }

    #[test]
    fn leaves_the_click_to_the_editor_unless_a_session_is_paused() {
        assert!(!quick_evaluate_enabled(&Bench::default()));
        for status in [SessionStatus::Running, SessionStatus::Terminated] {
            assert!(!quick_evaluate_enabled(&evaluating("debugpy", status)), "{status:?}");
        }
        // Without a suspended session, the keyboard shortcut finds nothing to evaluate in.
        let mut bench = evaluating("debugpy", SessionStatus::Terminated);
        let asked = bench.run("debug.quickEvaluate", None, Some(&selecting(EXPRESSION, 0, 5)));
        assert!(matches!(
            asked,
            Dispatch::QuickEvaluate(QuickEvaluation::Failed { message: "No active debug session", pos: 0 })
        ));
    }

    #[test]
    fn shows_the_contributed_menu_only_while_paused() {
        assert!(!Bench::default().debugger.offers_evaluate());
        for status in [SessionStatus::Running, SessionStatus::Paused, SessionStatus::Terminated] {
            let mut bench = evaluating("debugpy", status);
            assert_eq!(bench.debugger.offers_evaluate(), status == SessionStatus::Paused, "{status:?}");
            if status == SessionStatus::Paused {
                bench.run("debug.evaluate", None, Some(&selecting(EXPRESSION, 16, 16)));
                assert_eq!(bench.debugger.overlays.evaluate.as_deref(), Some("order.items"));
            }
        }
    }

    // --- further behaviour -----------------------------------------------------------------------

    // Not from a spec: Debug… without a configuration starts the project's only debuggable one,
    // through Run's debug flow when that exists, and offers a choice among several.
    #[test]
    fn debug_without_a_configuration_picks_the_only_debuggable_one() {
        let plain = rc("shell", "make", &[], None);
        let mut bench = Bench { run_debug_action: true, ..Default::default() };
        bench.run("debug.startConfig", None, None);
        bench.pump(&mut |call| match call {
            BackendCall::RunConfigs => Answer::Configs(vec![plain.clone(), server()]),
            _ => Answer::Done,
        });
        let only = as_arg(&server());
        assert!(matches!(
            bench.effects.as_slice(),
            [Effect::ExecuteAction { id, arg: Some(config) }] if id == "run.debugConfig" && *config == only
        ));
        bench.run("debug.startConfig", None, None);
        bench.pump(&mut |call| match call {
            BackendCall::RunConfigs => Answer::Configs(vec![server(), server()]),
            _ => Answer::Done,
        });
        assert_eq!(bench.debugger.overlays.chooser.as_ref().map(Vec::len), Some(2));
        bench.run("debug.startConfig", None, None);
        bench.pump(&mut |call| match call {
            BackendCall::RunConfigs => Answer::Configs(vec![plain.clone()]),
            _ => Answer::Done,
        });
        assert_eq!(bench.notifications().last().map(|n| n.title.as_str()), Some("Nothing to debug"));
    }

    // Not from a spec: Pause asks for the threads when it knows none, a failed step is reported
    // under the action's name, and Mute, View and Remove All act on the breakpoints.
    #[test]
    fn pauses_reports_failures_and_mutes_breakpoints() {
        let mut bench = evaluating("debugpy", SessionStatus::Running);
        bench.run("debug.pause", None, None);
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugThreads(_))), 1);
        assert_eq!(bench.count(|call| matches!(call, BackendCall::DebugPause { thread: 0, .. })), 1);

        bench.paused(false, false);
        bench.run("debug.stepOver", None, None);
        let refused = Answer::Fail(IdeError::internal("thread is gone"));
        bench.pump(&mut |call| match call {
            BackendCall::DebugStep { .. } => refused.clone(),
            _ => Answer::Done,
        });
        let n = *bench.notifications().last().expect("the failure");
        assert_eq!((n.title.as_str(), n.body.as_deref()), ("Step Over", Some("thread is gone")));
        assert_eq!(n.key.as_deref(), Some("debug.action"));
        assert_eq!(bench.session(3).status, SessionStatus::Paused);

        bench.debugger.breakpoints.lines.toggle_breakpoint("/p/a.py", 1, 0.0);
        bench.run("debug.muteBreakpoints", None, None);
        assert!(bench.debugger.breakpoints_muted());
        bench.run("debug.viewBreakpoints", None, Some(&caret("/p/a.py", "python", "a\nb", 1)));
        assert!(matches!(&bench.debugger.overlays.breakpoints, Some(Some(selection)) if selection.path == "/p/a.py"));
        bench.run("debug.removeAllBreakpoints", None, None);
        assert!(bench.debugger.breakpoints.lines.by_file().is_empty());
        assert!(matches!(bench.run("debug.browseConsoleHistory", None, None), Dispatch::Done));
        assert_eq!(bench.debugger.stoppable_sessions(), [(DebugSessionId(3), "app")]);
    }

    // Not from a spec: Remote JVM Debug in listen mode waits for the JVM, then attaches through
    // the relay; cancelling the wait ends it without a word.
    #[test]
    fn listens_for_a_remote_jvm_and_attaches_through_the_relay() {
        let params = [("mode", "listen"), ("host", "dev-box"), ("port", "5005")];
        let mut config = with_params(rc(REMOTE_JVM, "", &[], Some("java-debug")), &params);
        config.name = "Remote".to_owned();
        for connects in [true, false] {
            let mut bench = Bench::default();
            bench.run("debug.startConfig", Some(as_arg(&config)), None);
            bench.pump(&mut |call| match call {
                BackendCall::DebugListenForJvm { host, port: 5005, .. } if host == "dev-box" => Answer::Later,
                _ => Answer::Done,
            });
            let waiting = *bench.notifications().last().expect("the listening notification");
            assert_eq!(waiting.title, "'Remote': listening for the remote JVM on port 5005");
            assert!(waiting.body.as_deref().is_some_and(|body| body.contains("address=dev-box:5005,suspend=y")));
            let cancel = waiting.actions[0].command.clone();
            let (ticket, _) = bench.deferred.remove(0);
            let (debugger, mut host) = parts!(&mut bench);
            if connects {
                debugger.jvm_connected(ticket, Ok(41000), &mut host);
            } else {
                assert!(debugger.run_notification_command(&cancel, &mut host));
                debugger.jvm_connected(ticket, Err(IdeError::cancelled()), &mut host);
            }
            bench.settle();
            assert!(bench.effects.iter().any(|effect| matches!(effect, Effect::ExpireNotification(_))));
            assert_eq!(bench.count(|call| matches!(call, BackendCall::CancelRequest(_))), usize::from(!connects));
            let attached = bench.calls.iter().find_map(|call| match call {
                BackendCall::DebugStart(launch) => Some(extra(launch)),
                _ => None,
            });
            match attached {
                Some(extra) => {
                    assert!(connects);
                    assert_eq!((&extra["hostName"], &extra["port"]), (&json!("127.0.0.1"), &json!(41000)));
                    assert!(bench.session(42).launch.is_none());
                }
                None => assert!(!connects),
            }
        }
    }

    // Not from a spec: a plugin's session is prepared by its backend, and the plugin hears when it
    // ends and whether the user stopped it.
    #[test]
    fn tells_a_plugin_how_its_debug_session_ended() {
        let mut bench = Bench::default();
        bench.run("debug.pluginStart", Some(json!({ "config": as_arg(&server()), "operation": "op-1" })), None);
        bench.settle();
        assert_eq!(bench.count(|call| matches!(call, BackendCall::PluginPrepareDebugRun(_))), 1);
        assert_eq!(bench.session(42).status, SessionStatus::Running);
        assert!(bench.debugger.take_plugin_events().is_empty());
        bench.run("debug.stop", None, None);
        bench.debugger.observe_plugin_sessions();
        {
            let (debugger, host) = parts!(&mut bench);
            let mut ctx = Ctx { breakpoints: &mut debugger.breakpoints, env: host.env, i18n: host.i18n };
            debugger.state.handle_debug_event(DebugSessionId(42), DebugEvent::Exited { code: 3 }, &mut ctx);
            debugger.state.handle_debug_event(DebugSessionId(42), DebugEvent::Terminated, &mut ctx);
        }
        bench.debugger.observe_plugin_sessions();
        let exited = PluginEvent::Exited {
            operation: "op-1".to_owned(),
            session: DebugSessionId(42),
            code: Some(3),
            stopped: true,
        };
        assert_eq!(bench.debugger.take_plugin_events(), [exited]);
        bench.run("debug.pluginStart", None, None);
        assert!(matches!(bench.debugger.take_plugin_events().as_slice(), [PluginEvent::Failed { .. }]));
    }

    // Not from a spec: Reload Changed Classes saves, compiles the session's module and reloads its
    // classes, one answer after the other.
    #[test]
    fn reloads_changed_classes_through_the_hot_swap_steps() {
        let mut bench = Bench::default();
        bench.debugger.state.adopt_session(DebugSessionId(7), bench.now, |s| {
            s.adapter = "java-debug".to_owned();
            s.caps = Some(DebugCapabilities { hot_code_replace: true, ..Default::default() });
        });
        bench.debugger.state.take_effects();
        bench.run("debug.reloadChangedClasses", None, None);
        bench.settle();
        assert!(matches!(
            bench.calls.as_slice(),
            [
                BackendCall::JavaBuildWorkspace { module, .. },
                BackendCall::DebugRedefineClasses(DebugSessionId(7)),
            ] if module.is_empty()
        ));
        let titles: Vec<&str> = bench.notifications().iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["Compiling changed classes…", "1 class reloaded"]);
        assert!(!bench.debugger.hot_swap.building());
    }
}
