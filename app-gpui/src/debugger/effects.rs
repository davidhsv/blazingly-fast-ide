//! What the debugger asks its surroundings to do, and what it reads from them.
//!
//! The debugger models hold state and decide; they run nothing. A transition returns the
//! [`Effect`]s its caller carries out: a [`BackendCall`], a notification, a tool window to show.
//! The answer to a backend call comes back through the continuation of the model that asked: an
//! [`Effect::Ask`] carries the ticket that continuation wants back with the answer.
//!
//! Paths are `/`-separated strings, as the UI holds them; [`ipc`] and [`path_text`] convert at the
//! edge of the backend types.

use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_core::workflow_settings::{ConsoleSettings, DebuggerSettings, RunSettings};
use ide_core::{DebugSessionId, IdeError, IpcPath, RequestId, RunId};
use ide_dap::{Breakpoint, DataBreakpoint, FunctionBreakpoint, LaunchConfig, StepKind};
use ide_run::{BuildPolicy, RunConfig};
use serde_json::Value;
use std::collections::BTreeMap;

/// The Debug tool window's id.
pub const DEBUG_TOOL_WINDOW: &str = "debug";
/// The notification group of everything Run and Debug raise.
pub const RUN_GROUP: &str = "run";

/// `path` as a path field of a backend type.
pub fn ipc(path: &str) -> IpcPath {
    IpcPath::from(path)
}

/// A backend path as the UI holds it.
pub fn path_text(path: &IpcPath) -> String {
    ide_core::ipc_path::to_ipc(path)
}

/// What an evaluation is for (DAP `evaluate` context).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvaluateContext {
    Repl,
    Watch,
    Hover,
    Clipboard,
}

impl EvaluateContext {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Repl => "repl",
            Self::Watch => "watch",
            Self::Hover => "hover",
            Self::Clipboard => "clipboard",
        }
    }
}

/// A backend command.
#[derive(Debug, Clone)]
pub enum BackendCall {
    DebugStart(Box<LaunchConfig>),
    DebugStop(DebugSessionId),
    DebugCapabilities(DebugSessionId),
    DebugThreads(DebugSessionId),
    DebugStackTrace {
        session: DebugSessionId,
        thread: i64,
    },
    DebugScopes {
        session: DebugSessionId,
        frame: i64,
    },
    DebugVariables {
        session: DebugSessionId,
        reference: i64,
    },
    DebugEvaluateIn {
        session: DebugSessionId,
        frame: Option<i64>,
        expression: String,
        context: EvaluateContext,
    },
    DebugStep {
        session: DebugSessionId,
        thread: i64,
        kind: StepKind,
    },
    DebugResume {
        session: DebugSessionId,
        thread: i64,
    },
    DebugPause {
        session: DebugSessionId,
        thread: i64,
    },
    DebugForceStepOver {
        session: DebugSessionId,
        thread: i64,
    },
    DebugRunTo {
        session: DebugSessionId,
        thread: i64,
        path: String,
        line: u32,
        force: bool,
    },
    DebugStepInTargets {
        session: DebugSessionId,
        frame: i64,
    },
    DebugStepInTarget {
        session: DebugSessionId,
        thread: i64,
        target: i64,
    },
    DebugRestartFrame {
        session: DebugSessionId,
        frame: i64,
    },
    DebugHotReload(DebugSessionId),
    DebugHotRestart(DebugSessionId),
    DebugRedefineClasses(DebugSessionId),
    DebugSendInput {
        session: DebugSessionId,
        text: String,
    },
    DebugSetVariable {
        session: DebugSessionId,
        container: i64,
        name: String,
        evaluate_name: Option<String>,
        frame: Option<i64>,
        value: String,
    },
    DebugSetBreakpoints {
        path: String,
        breakpoints: Vec<Breakpoint>,
    },
    DebugBreakpointStates {
        path: String,
    },
    DebugSetFunctionBreakpoints(Vec<FunctionBreakpoint>),
    DebugSetDataBreakpoints(Vec<DataBreakpoint>),
    DebugSetExceptionBreakpoints(BTreeMap<String, bool>),
    DebugExceptionFilters,
    DebugAttachableProcesses,
    DebugListenForJvm {
        request: RequestId,
        host: String,
        port: u16,
    },
    RunStop(RunId),
    RunConfigs,
    CancelRequest(RequestId),
    JavaBuildWorkspace {
        module: String,
        request: RequestId,
    },
    JavaResolveLaunch {
        module: String,
        test_classpath: bool,
        policy: BuildPolicy,
        request: RequestId,
    },
    JavaDebugServer {
        module: String,
    },
    FsReadText(String),
    FsReadDir(String),
    FindFiles {
        name: String,
        limit: u32,
    },
    PluginPrepareDebugRun(Box<RunConfig>),
    /// The .NET configuration with its target evaluated for debugging.
    RunDotnetResolve(Box<RunConfig>),
    SdkPhpEffective,
    SdkLocateTools {
        names: Vec<String>,
    },
    SdkPhpXdebug {
        php: String,
        cwd: Option<String>,
    },
    WindowFlashTaskbar(bool),
    Languages,
}

/// Something for the debugger's caller to carry out.
#[derive(Debug, Clone)]
pub enum Effect {
    /// A backend command whose answer the debugger does not wait for, or which names what it is
    /// about itself (a breakpoint sync names its file).
    Call(BackendCall),
    /// A backend command whose answer goes to the debugger's continuation for that command,
    /// together with `ticket`.
    Ask {
        ticket: u64,
        call: BackendCall,
    },
    /// Backend commands to run together: their answers go to the continuation in this order,
    /// with `ticket`.
    AskAll {
        ticket: u64,
        calls: Vec<BackendCall>,
    },
    Notify(Box<NotificationRequest>),
    /// Takes down the balloon of the notification with this key.
    ExpireNotification(String),
    /// Shows the Debug tool window, focusing it or not.
    ShowToolWindow {
        focus: bool,
    },
    HideToolWindow,
    /// Runs another feature's action.
    ExecuteAction {
        id: String,
        arg: Option<Value>,
    },
    /// Opens a file at a 0-based line and UTF-16 column.
    OpenFile {
        path: String,
        line: u32,
        column: u32,
        focus: bool,
    },
    SaveAll,
    /// Saves the open documents, then tells the debugger how it went, with `ticket`.
    SaveAllThen {
        ticket: u64,
    },
    /// Writes `value` under `key` in the UI storage.
    StorageSet {
        key: String,
        value: String,
    },
    /// A message in the status bar.
    StatusInfo(String),
    /// Brings the application window to the front (un-minimised and focused).
    FocusAppWindow,
}

impl Effect {
    /// The backend command of a [`Effect::Call`] or [`Effect::Ask`].
    pub fn call(&self) -> Option<&BackendCall> {
        match self {
            Self::Call(call) | Self::Ask { call, .. } => Some(call),
            _ => None,
        }
    }

    /// The request of a [`Effect::Notify`].
    pub fn notification(&self) -> Option<&NotificationRequest> {
        match self {
            Self::Notify(request) => Some(request),
            _ => None,
        }
    }
}

/// A Run and Debug notification.
pub fn run_notification(level: NotificationLevel, title: impl Into<String>) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.group = Some(RUN_GROUP.to_owned());
    request
}

/// A notification button: `command` is what the caller hands back when it is pressed.
pub fn notification_action(label: &str, command: impl Into<String>) -> NotificationAction {
    NotificationAction { label: label.to_owned(), command: command.into() }
}

/// What the user reads of a failed backend call.
pub fn error_text(error: &IdeError) -> &str {
    &error.message
}

/// Whether a backend call failed because it was cancelled.
pub fn is_cancelled(error: &IdeError) -> bool {
    error.kind == ide_core::error::ErrorKind::Cancelled
}

/// What the debugger reads from its surroundings when it decides something.
#[derive(Debug, Clone, Copy)]
pub struct Env<'a> {
    pub debugger: &'a DebuggerSettings,
    pub run: &'a RunSettings,
    pub console: &'a ConsoleSettings,
    /// Milliseconds since the Unix epoch.
    pub now_ms: f64,
    /// A backend is attached (false in a preview without one: nothing can be debugged).
    pub desktop: bool,
    /// The window has the keyboard focus.
    pub window_focused: bool,
    /// The open project is trusted (untrusted projects send no breakpoints to the backend).
    pub trusted: bool,
    /// The Run feature's `run.stopProcess` action exists: a process a session is attached to is
    /// stopped through it, so Run records that the user asked.
    pub run_stop_action: bool,
}

impl Env<'_> {
    /// Whether breakpoints go to the backend: one is attached and the project is trusted.
    pub fn can_sync(&self) -> bool {
        self.desktop && self.trusted
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// Default settings, owned so tests can borrow an [`Env`] from them.
    #[derive(Debug, Clone, Default)]
    pub struct Settings {
        pub debugger: DebuggerSettings,
        pub run: RunSettings,
        pub console: ConsoleSettings,
    }

    impl Settings {
        /// A focused, trusted desktop window at `now_ms`.
        pub fn env(&self, now_ms: f64) -> Env<'_> {
            Env {
                debugger: &self.debugger,
                run: &self.run,
                console: &self.console,
                now_ms,
                desktop: true,
                window_focused: true,
                trusted: true,
                run_stop_action: false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a path survives the round trip through the backend's path type.
    #[test]
    fn paths_round_trip_through_the_backend_type() {
        assert_eq!(path_text(&ipc("/p/src/a.py")), "/p/src/a.py");
        assert_eq!(EvaluateContext::Hover.as_str(), "hover");
    }

    // Not from a spec: Run and Debug notifications carry their group.
    #[test]
    fn run_notifications_belong_to_the_run_group() {
        let request = run_notification(NotificationLevel::Info, "Hot Swap cancelled");
        assert_eq!(request.group.as_deref(), Some(RUN_GROUP));
        assert!(is_cancelled(&IdeError::cancelled()));
        assert_eq!(error_text(&IdeError::internal("boom")), "boom");
    }
}
