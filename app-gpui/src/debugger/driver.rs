//! Carries out what the debugger models ask of the backend. [`DebugDriver`] owns the window's
//! [`Debugger`]: every [`Effect`] its transitions return is run here, a backend command off the main
//! thread with its answer handed to the continuation the model names for it, and the backend's debug
//! events go to the sessions. Breakpoints and watches are stored in the UI storage as the models ask.
//!
//! The driver belongs to a project window. What only the window can do (open a file at a frame,
//! show the Debug tool window, raise a notification, run another feature's action) it reports as
//! [`DebugDriverEvent`]s.

use crate::backend::Backend;
use crate::debugger::actions::{ActiveEditor, Debugger, Dispatch, Host};
use crate::debugger::breakpoints::exceptions::{self, ExceptionBreakpoints, KNOWN_KEY};
use crate::debugger::breakpoints::{special, store};
use crate::debugger::editor::breakpoint_gutter::{
    ClickOutcome, GutterEditor, GutterPress, PressOutcome, gutter_click, gutter_mouse_down,
};
use crate::debugger::effects::{BackendCall, Effect, Env, EvaluateContext, path_text};
use crate::debugger::state::{Ctx, WATCH_KEY};
use crate::debugger::value_text::{VALUE_TEXT_LIMITS, value_text};
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::platform::local_time::now_ms;
use crate::platform::misc::jdk::DOWNLOAD_JDK_ACTION;
use crate::platform::storage::UiStorage;
use crate::run::driver::{SaveAll, run_settings};
use crate::run::java_launch::FoundSource;
use crate::workbench::notifications::NotificationRequest;
use gpui_kit::{App, BorrowAppContext, Context, Entity, EventEmitter, Task};
use ide_core::event::DebugEvent;
use ide_core::{DebugSessionId, IdeError, IdeEvent};
use ide_dap::{BreakpointState, DataBreakpoint, DataBreakpointInfo, FunctionBreakpoint, Scope, Variable};
use ide_host::Host as IdeHost;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// What the driver asks of its window.
#[derive(Clone, Debug)]
pub enum DebugDriverEvent {
    Notify(NotificationRequest),
    /// Take down the balloon of the notification with this key.
    ExpireNotification(String),
    /// Show the Debug tool window; `focus`: and give it the keyboard.
    ShowToolWindow {
        focus: bool,
    },
    HideToolWindow,
    /// Run another feature's action.
    RunAction {
        id: String,
        arg: Option<Value>,
    },
    /// Open `path` at the 0-based `line` and UTF-16 `column`.
    OpenFile {
        path: String,
        line: u32,
        column: u32,
        focus: bool,
    },
    /// A message for the status bar.
    StatusInfo(String),
    /// Bring the application window to the front.
    FocusAppWindow,
    /// The Set Value the Variables view asked for under `request` was answered.
    ValueSet {
        request: u64,
        result: Result<Variable, String>,
    },
}

/// The transitions' surroundings, from the fields of a [`Debugger`] and a [`Host`].
macro_rules! ctx {
    ($debugger:ident, $host:ident) => {
        Ctx { breakpoints: &mut $debugger.breakpoints, env: $host.env, i18n: &mut *$host.i18n }
    };
}

fn flatten<T>(result: anyhow::Result<Result<T, IdeError>>) -> Result<T, IdeError> {
    result.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())))
}

fn registered(id: &str, cx: &App) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

/// The answers of a method breakpoint and watchpoint sync: their states, in the order sent.
type SpecialSync = (Result<Vec<BreakpointState>, IdeError>, Result<Vec<BreakpointState>, IdeError>);

/// A continuation of the driver for the answer of a backend command.
type Then<T> = Box<dyn FnOnce(&mut DebugDriver, Result<T, IdeError>, &mut Context<DebugDriver>)>;

pub struct DebugDriver {
    label: String,
    debugger: Entity<Debugger>,
    /// The open project's folder.
    root: Option<String>,
    save_all: Option<SaveAll>,
    /// The window has the keyboard focus (a breakpoint hit in a background window asks for attention).
    window_focused: bool,
    /// The Set Value commands under way: the model's ticket and the Variables view's request.
    set_values: HashMap<u64, u64>,
    /// The breakpoint sync wake-up and when it is due.
    sync_timer: Option<(f64, Task<()>)>,
    /// The console output wake-up and when it is due.
    flush_timer: Option<(f64, Task<()>)>,
}

impl EventEmitter<DebugDriverEvent> for DebugDriver {}

impl DebugDriver {
    /// The driver of window `label`, with a debugger whose exception filter cache comes from the UI
    /// storage.
    pub fn new(label: &str, cx: &mut Context<Self>) -> Self {
        let known = cx.try_global::<UiStorage>().and_then(|storage| storage.get(KNOWN_KEY)).map(str::to_owned);
        let debugger = cx.new(|_| {
            let mut debugger = Debugger::default();
            debugger.breakpoints.exceptions = ExceptionBreakpoints::new(known.as_deref());
            debugger
        });
        Self {
            label: label.to_owned(),
            debugger,
            root: None,
            save_all: None,
            window_focused: true,
            set_values: HashMap::new(),
            sync_timer: None,
            flush_timer: None,
        }
    }

    /// Saves the open documents before a launch goes on with what is on disk.
    pub fn with_save_all(mut self, save_all: SaveAll) -> Self {
        self.save_all = Some(save_all);
        self
    }

    pub fn debugger(&self) -> &Entity<Debugger> {
        &self.debugger
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    // ---------------------------------------------------------------------------------------------
    // The models and what they read

    /// Runs `act` on the debugger with what its transitions read (the settings in effect, the clock,
    /// the text catalog), then tells its observers.
    pub fn with_host<R>(&mut self, cx: &mut Context<Self>, act: impl FnOnce(&mut Debugger, &mut Host) -> R) -> R {
        let settings = crate::platform::settings::effective(&self.label, cx);
        let targets = run_settings(&settings);
        let desktop = cx.has_global::<Backend>();
        let trusted = cx
            .try_global::<Backend>()
            .is_some_and(|backend| backend.host.project(&self.label).is_ok_and(|project| project.is_trusted()));
        let env = Env {
            debugger: &settings.debugger,
            run: &settings.run,
            console: &settings.console,
            now_ms: now_ms(),
            desktop,
            window_focused: self.window_focused,
            trusted,
            run_stop_action: registered("run.stopProcess", cx),
        };
        let run_debug_action = registered("run.debugConfig", cx);
        let download_jdk_action = registered(DOWNLOAD_JDK_ACTION, cx);
        let root = self.root.clone();
        let debugger = self.debugger.clone();
        cx.default_global::<Localization>();
        cx.update_global::<Localization, _>(|localization, cx| {
            let mut host = Host {
                env,
                i18n: &mut localization.0,
                root: root.as_deref(),
                windows: cfg!(windows),
                run_debug_action,
                download_jdk_action,
                run_targets: &targets,
                modules: &[],
            };
            debugger.update(cx, |debugger, cx| {
                let result = act(debugger, &mut host);
                cx.notify();
                result
            })
        })
    }

    /// [`Self::with_host`], then carries out what the change asked for.
    pub fn change<R>(&mut self, cx: &mut Context<Self>, act: impl FnOnce(&mut Debugger, &mut Host) -> R) -> R {
        let result = self.with_host(cx, act);
        self.pump(cx);
        result
    }

    /// The window got or lost the keyboard focus: a taskbar flash a breakpoint started stops when it
    /// comes back.
    pub fn set_window_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        if self.window_focused == focused {
            return;
        }
        self.window_focused = focused;
        if focused {
            self.change(cx, |debugger, host| debugger.state.window_focused(host.env.desktop));
        }
    }

    /// The window's project is now the one at `root` (`None`: it closed): the sessions of the last
    /// one stop and are forgotten, and the breakpoints, exception choices and watches of this one
    /// are read from the UI storage.
    pub fn load(&mut self, root: Option<&str>, cx: &mut Context<Self>) {
        self.root = root.map(str::to_owned);
        let mut stored: HashMap<String, String> = HashMap::new();
        if let (Some(root), Some(storage)) = (root, cx.try_global::<UiStorage>()) {
            for key in [store::storage_key(root), exceptions::choices_key(root), special::storage_key(root)] {
                if let Some(value) = storage.get(&key) {
                    stored.insert(key, value.to_owned());
                }
            }
        }
        let watches = cx.try_global::<UiStorage>().and_then(|storage| storage.get(WATCH_KEY)).map(str::to_owned);
        self.set_values.clear();
        let languages = self.change(cx, |debugger, host| {
            let mut ctx = ctx!(debugger, host);
            debugger.state.reset_sessions(&mut ctx);
            debugger.overlays = Default::default();
            debugger.focused = Default::default();
            let can_sync = host.env.can_sync();
            debugger.breakpoints.load(root, |key| stored.get(key).cloned(), can_sync, host.env.now_ms);
            debugger.state.load_watches(watches.as_deref());
            let languages = debugger.breakpoints.languages.load(host.env.desktop);
            debugger.breakpoints.lines.reset_statuses();
            debugger.breakpoints.special.reset_statuses();
            languages
        });
        if let Some(call) = languages {
            self.carry_out(vec![Effect::Call(call)], cx);
        }
    }

    /// The project closed: its sessions stop and nothing of it is kept.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        self.load(None, cx);
        self.sync_timer = None;
        self.flush_timer = None;
    }

    /// Runs the debugger action `id`; `editor`: the text editor that has the focus.
    pub fn run_action(
        &mut self,
        id: &str,
        arg: Option<&Value>,
        editor: Option<&ActiveEditor>,
        cx: &mut Context<Self>,
    ) -> Dispatch {
        self.change(cx, |debugger, host| debugger.run_action(id, arg, editor, host))
    }

    /// A notification button of the debugger was pressed; false for a command that is not one of
    /// the debugger's.
    pub fn notification_command(&mut self, command: &str, cx: &mut Context<Self>) -> bool {
        self.change(cx, |debugger, host| debugger.run_notification_command(command, host))
    }

    /// A press on `line` of the editor's gutter; a release over the same line is then
    /// [`Self::gutter_click`].
    pub fn gutter_press(
        &mut self,
        editor: &GutterEditor,
        line: u32,
        press: GutterPress,
        cx: &mut Context<Self>,
    ) -> PressOutcome {
        self.change(cx, |debugger, host| {
            let any_file = host.env.debugger.breakpoints_any_file;
            let can_place = debugger.breakpoints.languages.can_place_breakpoint(editor.language, editor.path, any_file);
            let crate::debugger::breakpoints::Breakpoints { lines, special, .. } = &mut debugger.breakpoints;
            gutter_mouse_down(lines, special, editor, line, press, can_place, host.env.now_ms)
        })
    }

    /// A click on `line` of the editor's gutter: a breakpoint is added or removed there.
    pub fn gutter_click(
        &mut self,
        editor: &GutterEditor,
        line: u32,
        press: GutterPress,
        cx: &mut Context<Self>,
    ) -> ClickOutcome {
        self.change(cx, |debugger, host| {
            let crate::debugger::breakpoints::Breakpoints { lines, special, .. } = &mut debugger.breakpoints;
            gutter_click(lines, special, editor, line, press, host.env.now_ms)
        })
    }

    /// A debug event of the backend for this window.
    pub fn backend_event(&mut self, event: &IdeEvent, cx: &mut Context<Self>) {
        let IdeEvent::Debug { session, event } = event else { return };
        let (session, event) = (*session, event.clone());
        self.change(cx, |debugger, host| match event {
            DebugEvent::HotCodeReplace { change_type, message } => {
                debugger.on_hot_code_replace(session, &change_type, &message, host);
            }
            event => {
                let mut ctx = ctx!(debugger, host);
                debugger.state.handle_debug_event(session, event, &mut ctx);
                debugger.observe_plugin_sessions();
            }
        });
    }

    // ---------------------------------------------------------------------------------------------
    // What the Variables view asks for

    /// The scopes of `frame`.
    pub fn scopes(&self, session: DebugSessionId, frame: i64, cx: &App) -> Task<Result<Vec<Scope>, String>> {
        self.request(cx, move |host, label| async move {
            ide_host::ops::debug::debug_scopes(&host, &label, session, frame).await
        })
    }

    /// The children of a variables reference.
    pub fn children(&self, session: DebugSessionId, reference: i64, cx: &App) -> Task<Result<Vec<Variable>, String>> {
        self.request(cx, move |host, label| async move {
            ide_host::ops::debug::debug_variables(&host, &label, session, reference).await
        })
    }

    /// Evaluates `expression` in the selected frame of `session`.
    pub fn evaluate(
        &self,
        session: DebugSessionId,
        expression: &str,
        context: EvaluateContext,
        cx: &App,
    ) -> Task<Result<Variable, String>> {
        let (session, frame, expression, context) =
            match self.debugger.read(cx).state.evaluate(expression, context, Some(session)) {
                Ok(BackendCall::DebugEvaluateIn { session, frame, expression, context }) => {
                    (session, frame, expression, Some(context.as_str().to_owned()))
                }
                Ok(_) => return Task::ready(Err("No active debug session".to_owned())),
                Err(message) => return Task::ready(Err(message.to_owned())),
            };
        self.request(cx, move |host, label| async move {
            ide_host::ops::debug::debug_evaluate_in(&host, &label, session, frame, expression, context).await
        })
    }

    /// The data breakpoint a field watchpoint on `name` of `container` would set.
    pub fn data_breakpoint_info(
        &self,
        session: DebugSessionId,
        container: i64,
        name: String,
        frame: Option<i64>,
        cx: &App,
    ) -> Task<Result<DataBreakpointInfo, String>> {
        self.request(cx, move |host, label| async move {
            ide_host::ops::debug::debug_data_breakpoint_info(&host, &label, session, container, name, frame).await
        })
    }

    /// Copy Value of a structured value: its summary and its children, walked off the main thread.
    pub fn copy_value(
        &self,
        session: DebugSessionId,
        value: String,
        reference: i64,
        cx: &App,
    ) -> Task<Result<String, String>> {
        self.request(cx, move |host, label| async move {
            let inner = Arc::clone(&host);
            host.blocking(move || {
                let runtime = tokio::runtime::Handle::current();
                let mut fetch = |reference: i64| {
                    let children = ide_host::ops::debug::debug_variables(&inner, &label, session, reference);
                    runtime.block_on(children).unwrap_or_default()
                };
                Ok(value_text(&value, reference, &mut fetch, VALUE_TEXT_LIMITS))
            })
            .await
        })
    }

    /// Set Value in the Variables view: the answer comes back as [`DebugDriverEvent::ValueSet`]
    /// with `request`.
    #[expect(clippy::too_many_arguments, reason = "the Variables view's request, field by field")]
    pub fn set_value(
        &mut self,
        request: u64,
        session: DebugSessionId,
        container: i64,
        name: &str,
        evaluate_name: Option<&str>,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let asked = self.with_host(cx, |debugger, _| {
            debugger.state.set_variable_value(session, container, name, evaluate_name, value)
        });
        if let Err(message) = asked {
            cx.emit(DebugDriverEvent::ValueSet { request, result: Err(message.to_owned()) });
            return;
        }
        let effects = self.debugger.update(cx, |debugger, _| debugger.take_effects());
        for effect in &effects {
            if let Effect::Ask { ticket, call: BackendCall::DebugSetVariable { .. } } = effect {
                self.set_values.insert(*ticket, request);
            }
        }
        self.carry_out(effects, cx);
        self.pump(cx);
    }

    /// Runs `work` on the backend; the error is the message the user reads.
    fn request<T, F, W>(&self, cx: &App, work: W) -> Task<Result<T, String>>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, IdeError>> + Send + 'static,
        W: FnOnce(Arc<IdeHost>, String) -> F + Send + 'static,
    {
        let label = self.label.clone();
        let Some(call) = cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)))
        else {
            return Task::ready(Err("the backend is not running".to_owned()));
        };
        cx.background_executor().spawn(async move { flatten(call.await).map_err(|error| error.message) })
    }

    // ---------------------------------------------------------------------------------------------
    // Carrying out the effects

    /// Carries out what the debugger asked for until it asks for nothing more, then arranges the
    /// wake-ups its stores wait for.
    fn pump(&mut self, cx: &mut Context<Self>) {
        loop {
            let effects = self.debugger.update(cx, |debugger, _| debugger.take_effects());
            if effects.is_empty() {
                break;
            }
            self.carry_out(effects, cx);
        }
        // Only plugins hear these; no plugin starts sessions in this window.
        let dropped = self.debugger.update(cx, |debugger, _| debugger.take_plugin_events());
        if !dropped.is_empty() {
            tracing::debug!("debug session events for plugins: {dropped:?}");
        }
        self.schedule(cx);
        cx.notify();
    }

    fn carry_out(&mut self, effects: Vec<Effect>, cx: &mut Context<Self>) {
        // A sync of the method breakpoints and watchpoints is two commands answered together.
        let mut functions: Option<Vec<FunctionBreakpoint>> = None;
        for effect in effects {
            match effect {
                Effect::Call(BackendCall::DebugSetFunctionBreakpoints(list)) => {
                    if let Some(earlier) = functions.replace(list) {
                        self.sync_special(earlier, None, cx);
                    }
                }
                Effect::Call(BackendCall::DebugSetDataBreakpoints(data)) => {
                    self.sync_special(functions.take().unwrap_or_default(), Some(data), cx);
                }
                Effect::Call(call) => self.call(None, call, cx),
                Effect::Ask { ticket, call } => self.call(Some(ticket), call, cx),
                Effect::AskAll { ticket, calls } => self.ask_all(ticket, calls, cx),
                Effect::Notify(request) => cx.emit(DebugDriverEvent::Notify(*request)),
                Effect::ExpireNotification(key) => cx.emit(DebugDriverEvent::ExpireNotification(key)),
                Effect::ShowToolWindow { focus } => cx.emit(DebugDriverEvent::ShowToolWindow { focus }),
                Effect::HideToolWindow => cx.emit(DebugDriverEvent::HideToolWindow),
                Effect::ExecuteAction { id, arg } => cx.emit(DebugDriverEvent::RunAction { id, arg }),
                Effect::OpenFile { path, line, column, focus } => {
                    cx.emit(DebugDriverEvent::OpenFile { path, line, column, focus });
                }
                Effect::SaveAll => {
                    let saving = self.save(cx);
                    cx.spawn(async move |_, _| {
                        if let Err(error) = saving.await {
                            tracing::warn!("cannot save the open documents: {error}");
                        }
                    })
                    .detach();
                }
                Effect::SaveAllThen { ticket } => {
                    let saving = self.save(cx);
                    cx.spawn(async move |this, cx| {
                        let saved = saving.await;
                        let _ = this.update(cx, |driver, cx| {
                            driver.change(cx, |debugger, host| debugger.saved(ticket, saved, host));
                        });
                    })
                    .detach();
                }
                Effect::StorageSet { key, value } => {
                    if cx.has_global::<UiStorage>() {
                        cx.global_mut::<UiStorage>().set(&key, value);
                    }
                }
                Effect::StatusInfo(text) => cx.emit(DebugDriverEvent::StatusInfo(text)),
                Effect::FocusAppWindow => cx.emit(DebugDriverEvent::FocusAppWindow),
            }
        }
        if let Some(functions) = functions {
            self.sync_special(functions, None, cx);
        }
    }

    /// Saves the open documents, or nothing when the window gave no way to.
    fn save(&self, cx: &mut Context<Self>) -> futures::future::LocalBoxFuture<'static, Result<(), String>> {
        use futures::FutureExt as _;
        match self.save_all.clone() {
            Some(save_all) => save_all(cx),
            None => futures::future::ready(Ok(())).boxed_local(),
        }
    }

    /// Runs `work` on the backend and hands its answer to `then`, followed by whatever that asks.
    fn ask<T, F, W>(&mut self, cx: &mut Context<Self>, work: W, then: Then<T>)
    where
        T: Send + 'static,
        F: Future<Output = Result<T, IdeError>> + Send + 'static,
        W: FnOnce(Arc<IdeHost>, String) -> F + Send + 'static,
    {
        let label = self.label.clone();
        let call = cx.try_global::<Backend>().map(|backend| backend.with_host(move |host| work(host, label)));
        cx.spawn(async move |this, cx| {
            let result = match call {
                Some(call) => flatten(call.await),
                None => Err(IdeError::internal("the backend is not running")),
            };
            let _ = this.update(cx, |driver, cx| {
                then(driver, result, cx);
                driver.pump(cx);
            });
        })
        .detach();
    }

    /// A command whose answer only matters when it failed.
    fn fire<F, W>(&mut self, cx: &mut Context<Self>, what: &'static str, work: W)
    where
        F: Future<Output = Result<(), IdeError>> + Send + 'static,
        W: FnOnce(Arc<IdeHost>, String) -> F + Send + 'static,
    {
        let then: Then<()> = Box::new(move |_, result, _| {
            if let Err(error) = result {
                tracing::warn!("{what} failed: {}", error.message);
            }
        });
        self.ask(cx, work, then);
    }

    /// Answers `then` with the result of a continuation on the debugger.
    fn on_debugger<T: 'static>(then: impl FnOnce(&mut Debugger, Result<T, IdeError>, &mut Host) + 'static) -> Then<T> {
        Box::new(move |driver, result, cx| driver.with_host(cx, |debugger, host| then(debugger, result, host)))
    }

    /// Sends the method breakpoints and watchpoints; their states come back together. `data`:
    /// `None` when only the method breakpoints were sent.
    fn sync_special(
        &mut self,
        functions: Vec<FunctionBreakpoint>,
        data: Option<Vec<DataBreakpoint>>,
        cx: &mut Context<Self>,
    ) {
        let work = move |host: Arc<IdeHost>, label: String| async move {
            let methods = ide_host::ops::debug::debug_set_function_breakpoints(&host, &label, functions).await;
            let fields = match data {
                Some(data) => ide_host::ops::debug::debug_set_data_breakpoints(&host, &label, data).await,
                None => Ok(Vec::new()),
            };
            Ok::<SpecialSync, IdeError>((methods, fields))
        };
        let then = Self::on_debugger(|debugger, result: Result<SpecialSync, IdeError>, _| {
            if let Ok((methods, fields)) = result {
                debugger.breakpoints.special.synced(methods, fields);
            }
        });
        self.ask(cx, work, then);
    }

    /// Carries out one backend command; `ticket`: the continuation's, for an [`Effect::Ask`].
    fn call(&mut self, ticket: Option<u64>, call: BackendCall, cx: &mut Context<Self>) {
        use ide_host::ops::{debug, fs, java, run, sdk};
        let ticket = ticket.unwrap_or(0);
        match call {
            BackendCall::DebugStart(launch) => self.ask(
                cx,
                move |host, label| async move { debug::debug_start(&host, &label, *launch).await },
                Self::on_debugger(move |debugger, result, host| debugger.started(ticket, result, host)),
            ),
            BackendCall::DebugStop(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_stop(&host, &label, session).await },
                Self::on_debugger(move |debugger, result, host| {
                    let mut ctx = ctx!(debugger, host);
                    debugger.state.stop_finished(ticket, result, &mut ctx);
                }),
            ),
            BackendCall::DebugCapabilities(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_capabilities(&host, &label, session) },
                Self::on_debugger(move |debugger, result, _| debugger.state.capabilities_loaded(ticket, result)),
            ),
            BackendCall::DebugThreads(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_threads(&host, &label, session).await },
                Self::on_debugger(move |debugger, result, host| debugger.threads_loaded(ticket, result, host)),
            ),
            BackendCall::DebugStackTrace { session, thread } => self.ask(
                cx,
                move |host, label| async move { debug::debug_stack_trace(&host, &label, session, thread).await },
                Self::on_debugger(move |debugger, result, host| {
                    let mut ctx = ctx!(debugger, host);
                    debugger.state.frames_loaded(ticket, result, &mut ctx);
                }),
            ),
            BackendCall::DebugScopes { session, frame } => self.ask(
                cx,
                move |host, label| async move { debug::debug_scopes(&host, &label, session, frame).await },
                Self::on_debugger(move |debugger, result, _| debugger.state.scopes_loaded(ticket, result)),
            ),
            BackendCall::DebugVariables { session, reference } => self.ask(
                cx,
                move |host, label| async move { debug::debug_variables(&host, &label, session, reference).await },
                Self::on_debugger(move |debugger, result, _| debugger.state.variables_loaded(ticket, result)),
            ),
            BackendCall::DebugEvaluateIn { session, frame, expression, context } => {
                let context = Some(context.as_str().to_owned());
                self.ask(
                    cx,
                    move |host, label| async move {
                        debug::debug_evaluate_in(&host, &label, session, frame, expression, context).await
                    },
                    Self::on_debugger(move |debugger, result, host| {
                        debugger.state.log_value_evaluated(ticket, result, host.env.console);
                    }),
                );
            }
            BackendCall::DebugStep { session, thread, kind } => self.ask(
                cx,
                move |host, label| async move { debug::debug_step(&host, &label, session, thread, kind).await },
                Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
            ),
            BackendCall::DebugResume { session, thread } => self.ask(
                cx,
                move |host, label| async move { debug::debug_resume(&host, &label, session, thread).await },
                Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
            ),
            BackendCall::DebugForceStepOver { session, thread } => self.ask(
                cx,
                move |host, label| async move { debug::debug_force_step_over(&host, &label, session, thread).await },
                Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
            ),
            BackendCall::DebugRunTo { session, thread, path, line, force } => self.ask(
                cx,
                move |host, label| async move {
                    debug::debug_run_to(&host, &label, session, thread, PathBuf::from(path), line, force).await
                },
                Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
            ),
            BackendCall::DebugStepInTarget { session, thread, target } => {
                self.ask(
                    cx,
                    move |host, label| async move {
                        debug::debug_step_in_target(&host, &label, session, thread, target).await
                    },
                    Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
                )
            }
            BackendCall::DebugRestartFrame { session, frame } => self.ask(
                cx,
                move |host, label| async move { debug::debug_restart_frame(&host, &label, session, frame).await },
                Self::on_debugger(move |debugger, result, host| debugger.resumed(ticket, result, host)),
            ),
            BackendCall::DebugPause { session, thread } => self.ask(
                cx,
                move |host, label| async move { debug::debug_pause(&host, &label, session, thread).await },
                Self::on_debugger(move |debugger, result, host| debugger.reported(ticket, result, host)),
            ),
            BackendCall::DebugHotReload(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_hot_reload(&host, &label, session).await },
                Self::on_debugger(move |debugger, result, host| debugger.reported(ticket, result, host)),
            ),
            BackendCall::DebugHotRestart(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_hot_restart(&host, &label, session).await },
                Self::on_debugger(move |debugger, result, host| debugger.reported(ticket, result, host)),
            ),
            BackendCall::DebugStepInTargets { session, frame } => self.ask(
                cx,
                move |host, label| async move { debug::debug_step_in_targets(&host, &label, session, frame).await },
                // Without the paused line's text the targets are offered in the chooser.
                Self::on_debugger(move |debugger, result, host| {
                    debugger.step_in_targets_loaded(ticket, result, None, host);
                }),
            ),
            BackendCall::DebugRedefineClasses(session) => self.ask(
                cx,
                move |host, label| async move { debug::debug_redefine_classes(&host, &label, session).await },
                Self::on_debugger(move |debugger, result, host| debugger.hot_swap_redefined(ticket, result, host)),
            ),
            BackendCall::DebugSendInput { session, text } => self.fire(cx, "Sending input to the program", {
                move |host, label| async move { debug::debug_send_input(&host, &label, session, text).await }
            }),
            BackendCall::DebugSetVariable { session, container, name, evaluate_name: evaluated, frame, value } => {
                let work = move |host: Arc<IdeHost>, label: String| async move {
                    debug::debug_set_variable(&host, &label, session, container, name, evaluated, frame, value).await
                };
                let then: Then<Variable> = Box::new(move |driver, result, cx| {
                    let result = driver.with_host(cx, |debugger, _| debugger.state.variable_set(ticket, result));
                    if let Some(request) = driver.set_values.remove(&ticket) {
                        let result = result.map_err(|error| error.message);
                        cx.emit(DebugDriverEvent::ValueSet { request, result });
                    }
                });
                self.ask(cx, work, then);
            }
            BackendCall::DebugSetBreakpoints { path, breakpoints } => {
                let sent = path.clone();
                self.ask(
                    cx,
                    move |host, label| async move {
                        debug::debug_set_breakpoints(&host, &label, PathBuf::from(sent), breakpoints).await
                    },
                    Self::on_debugger(move |debugger, result, host| debugger.breakpoints_synced(&path, result, host)),
                );
            }
            BackendCall::DebugBreakpointStates { path } => {
                let asked = path.clone();
                self.ask(
                    cx,
                    move |host, label| async move {
                        debug::debug_breakpoint_states(&host, &label, PathBuf::from(asked))
                    },
                    Self::on_debugger(move |debugger, result, _| match result {
                        Ok(states) => debugger.breakpoints.lines.apply_states(&path, &states),
                        Err(error) => tracing::debug!("breakpoint states of {path}: {}", error.message),
                    }),
                );
            }
            // Paired in `carry_out`; one alone is sent with its own answer only.
            BackendCall::DebugSetFunctionBreakpoints(functions) => self.sync_special(functions, None, cx),
            BackendCall::DebugSetDataBreakpoints(data) => self.sync_special(Vec::new(), Some(data), cx),
            BackendCall::DebugSetExceptionBreakpoints(choices) => self.fire(cx, "Setting exception breakpoints", {
                move |host, label| async move { debug::debug_set_exception_breakpoints(&host, &label, choices).await }
            }),
            BackendCall::DebugExceptionFilters => self.ask(
                cx,
                move |host, label| async move { debug::debug_exception_filters(&host, &label) },
                Self::on_debugger(move |debugger, result, _| match result {
                    Ok(sessions) => debugger.breakpoints.exceptions.filters_loaded(&sessions),
                    Err(error) => tracing::debug!("exception filters: {}", error.message),
                }),
            ),
            BackendCall::DebugAttachableProcesses => self.ask(
                cx,
                move |host, label| async move { debug::debug_attachable_processes(&host, &label).await },
                Self::on_debugger(move |debugger, result, _| debugger.attachable_processes_loaded(ticket, result)),
            ),
            BackendCall::DebugListenForJvm { request, host: jvm_host, port } => {
                self.ask(
                    cx,
                    move |host, label| async move {
                        debug::debug_listen_for_jvm(&host, &label, request, jvm_host, port).await
                    },
                    Self::on_debugger(move |debugger, result, host| debugger.jvm_connected(ticket, result, host)),
                )
            }
            BackendCall::RunStop(process) => self.fire(cx, "Stopping the process", {
                move |host, label| async move { run::stop(&host, &label, process) }
            }),
            BackendCall::RunConfigs => self.ask(
                cx,
                move |host, label| async move { run::configs(&host, &label) },
                Self::on_debugger(move |debugger, result, host| debugger.run_configs_loaded(ticket, result, host)),
            ),
            BackendCall::CancelRequest(request) => self.fire(cx, "Cancelling", move |host, _| async move {
                host.cancel_request(request);
                Ok(())
            }),
            BackendCall::JavaBuildWorkspace { module, request } => self.ask(
                cx,
                move |host, label| async move {
                    java::java_build_workspace(&host, &label, PathBuf::from(module), Some(request)).await
                },
                Self::on_debugger(move |debugger, result, host| debugger.hot_swap_built(ticket, result, host)),
            ),
            BackendCall::JavaResolveLaunch { module, test_classpath, policy, request } => self.ask(
                cx,
                move |host, label| async move {
                    java::java_resolve_launch(&host, &label, PathBuf::from(module), test_classpath, policy, request)
                        .await
                },
                Self::on_debugger(move |debugger, result, host| debugger.launch_resolved(ticket, result, host)),
            ),
            BackendCall::JavaDebugServer { module } => self.ask(
                cx,
                move |host, label| async move { java::java_debug_server(&host, &label, PathBuf::from(module)).await },
                Self::on_debugger(move |debugger, result, host| debugger.debug_server_started(ticket, result, host)),
            ),
            BackendCall::FsReadText(path) => self.ask_all(ticket, vec![BackendCall::FsReadText(path)], cx),
            BackendCall::FsReadDir(path) => self.ask(
                cx,
                move |host, label| async move { fs::read_dir(&host, &label, PathBuf::from(path)).await },
                Self::on_debugger(move |debugger, result, host| debugger.dir_read(ticket, result, host)),
            ),
            call @ BackendCall::FindFiles { .. } => self.ask_all(ticket, vec![call], cx),
            // A plugin's backend is not reachable from this window: the start says so.
            BackendCall::PluginPrepareDebugRun(_) => self.with_host(cx, |debugger, host| {
                let error = IdeError::unavailable("Plugin run configurations cannot be debugged in this window");
                debugger.config_prepared(ticket, Err(error), host);
            }),
            BackendCall::RunDotnetResolve(config) => self.ask(
                cx,
                move |host, label| async move { run::run_dotnet_resolve(&host, &label, *config, true).await },
                Self::on_debugger(move |debugger, result, host| debugger.config_prepared(ticket, result, host)),
            ),
            BackendCall::SdkPhpEffective => self.ask(
                cx,
                move |host, label| async move { sdk::sdk_php_effective(&host, &label) },
                Self::on_debugger(move |debugger, result, host| {
                    let php = result.ok().flatten().map(|php| path_text(&php));
                    debugger.php_resolved(ticket, php, host);
                }),
            ),
            BackendCall::SdkLocateTools { names } => self.ask(
                cx,
                move |host, label| async move { sdk::sdk_locate_tools(&host, &label, names, None).await },
                Self::on_debugger(move |debugger, result, _| {
                    let found = result.ok().and_then(|located| located.tools.into_iter().find_map(|tool| tool.path));
                    debugger.php_located(ticket, found.map(|php| path_text(&php)));
                }),
            ),
            BackendCall::SdkPhpXdebug { php, cwd } => self.ask(
                cx,
                move |host, label| async move {
                    sdk::sdk_php_xdebug(&host, &label, PathBuf::from(php), cwd.map(PathBuf::from)).await
                },
                Self::on_debugger(move |debugger, result, host| match result {
                    Ok(probe) => {
                        let php = path_text(&probe.php);
                        let loaded = probe.xdebug.is_some();
                        debugger.xdebug_probed(ticket, Some((php.as_str(), probe.php_version.as_str(), loaded)), host);
                    }
                    Err(_) => debugger.xdebug_probed(ticket, None, host),
                }),
            ),
            // The window cannot ask the shell for attention: the next breakpoint hit asks again.
            BackendCall::WindowFlashTaskbar(flash) => {
                if flash {
                    self.with_host(cx, |debugger, _| debugger.state.taskbar_flash_failed());
                }
            }
            BackendCall::Languages => self.ask(
                cx,
                move |host, _| async move {
                    let debuggable = host.languages.all().into_iter().filter(|language| language.debugger.is_some());
                    Ok(debuggable.map(|language| language.id.as_str().to_owned()).collect::<Vec<String>>())
                },
                Self::on_debugger(move |debugger, result, _| debugger.breakpoints.languages.loaded(result.ok())),
            ),
        }
    }

    /// Commands whose answers go to the continuation together, in order: the file index searched
    /// for each name, or the texts of files.
    fn ask_all(&mut self, ticket: u64, calls: Vec<BackendCall>, cx: &mut Context<Self>) {
        if matches!(calls.first(), Some(BackendCall::FindFiles { .. })) {
            let searches: Vec<(String, u32)> = calls
                .into_iter()
                .filter_map(|call| match call {
                    BackendCall::FindFiles { name, limit } => Some((name, limit)),
                    _ => None,
                })
                .collect();
            let work = move |host: Arc<IdeHost>, label: String| async move {
                let inner = Arc::clone(&host);
                host.blocking(move || {
                    let mut found = Vec::new();
                    for (name, limit) in searches {
                        let hits = ide_host::ops::nav::find_files(&inner, &label, name.clone(), limit)?;
                        let named = hits.into_iter().filter(|hit| hit.rel_path.rsplit('/').next() == Some(&name));
                        found.extend(named.map(|hit| FoundSource {
                            path: hit.path.0.to_string_lossy().into_owned(),
                            rel_path: hit.rel_path,
                        }));
                    }
                    Ok(found)
                })
                .await
            };
            let then = Self::on_debugger(move |debugger, result: Result<Vec<FoundSource>, IdeError>, host| {
                debugger.files_found(ticket, result.unwrap_or_default(), host);
            });
            return self.ask(cx, work, then);
        }
        let paths: Vec<String> = calls
            .into_iter()
            .filter_map(|call| match call {
                BackendCall::FsReadText(path) => Some(path),
                _ => None,
            })
            .collect();
        let work = move |host: Arc<IdeHost>, label: String| async move {
            let mut texts = Vec::with_capacity(paths.len());
            for path in paths {
                texts.push(ide_host::ops::fs::read_text(&host, &label, PathBuf::from(path)).await);
            }
            Ok::<_, IdeError>(texts)
        };
        let then = Self::on_debugger(move |debugger, result: Result<Vec<Result<String, IdeError>>, IdeError>, host| {
            let texts = result.unwrap_or_else(|error| vec![Err(error)]);
            debugger.texts_read(ticket, texts, host);
        });
        self.ask(cx, work, then);
    }

    // ---------------------------------------------------------------------------------------------
    // Wake-ups

    /// Wakes the breakpoint stores when their sync is due and the console when queued output has to
    /// show; an unchanged deadline keeps its timer.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        let (sync, flush) = {
            let debugger = self.debugger.read(cx);
            (debugger.breakpoints.next_deadline(), debugger.state.next_output_flush())
        };
        if self.sync_timer.as_ref().map(|(due, _)| *due) != sync {
            self.sync_timer = sync.map(|due| (due, Self::wake_at(due, cx, Self::sync_due)));
        }
        if self.flush_timer.as_ref().map(|(due, _)| *due) != flush {
            self.flush_timer = flush.map(|due| (due, Self::wake_at(due, cx, Self::flush_due)));
        }
    }

    fn wake_at(due: f64, cx: &mut Context<Self>, then: fn(&mut Self, &mut Context<Self>)) -> Task<()> {
        let wait = Duration::from_secs_f64(((due - now_ms()) / 1000.0).max(0.0));
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(wait).await;
            let _ = this.update(cx, |driver, cx| then(driver, cx));
        })
    }

    /// The breakpoint edits that waited out their delay go to the backend.
    fn sync_due(&mut self, cx: &mut Context<Self>) {
        self.sync_timer = None;
        self.change(cx, |debugger, host| debugger.breakpoints.sync_due(host.env.now_ms, host.env.can_sync()));
    }

    /// Queued output shows in the consoles.
    fn flush_due(&mut self, cx: &mut Context<Self>) {
        self.flush_timer = None;
        self.change(cx, |debugger, host| debugger.state.flush_console_output(host.env.console));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::run::configs::blank_config;
    use gpui_kit::{AppContext as _, TestAppContext};
    use ropey::Rope;
    use serde_json::json;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// The temporary folders a test lives in: the app's home and the project.
    struct Folders {
        _home: tempfile::TempDir,
        _project: tempfile::TempDir,
        root: String,
    }

    /// A backend whose configuration folder holds `languages` (file name, text), with a trusted
    /// project opened in window `w1`, and a driver of that window over it.
    async fn project(
        cx: &mut TestAppContext,
        languages: &[(&str, &str)],
    ) -> (Entity<DebugDriver>, Rc<RefCell<Vec<DebugDriverEvent>>>, Folders) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        let project = tempfile::tempdir().expect("a temporary project");
        let root = project.path().canonicalize().expect("the project folder exists");
        let dirs = AppDirs::under(home.path());
        let folder = dirs.config.join("languages");
        std::fs::create_dir_all(&folder).expect("the languages folder is made");
        for (name, text) in languages {
            std::fs::write(folder.join(name), text).expect("the language is written");
        }
        cx.update(|cx| {
            cx.set_global(UiStorage::default());
            Backend::init(dirs, cx).expect("the backend starts");
            cx.global::<Backend>().host.trust.decide(&root, true, false).expect("the project is trusted");
        });
        let open_root = root.clone();
        let opening = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opening.await.expect("the task completes").expect("the project opens");
        let root = root.to_string_lossy().into_owned();
        let driver = cx.update(|cx| cx.new(|cx| DebugDriver::new("w1", cx)));
        let events: Rc<RefCell<Vec<DebugDriverEvent>>> = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|cx| {
            cx.subscribe(&driver, move |_, event: &DebugDriverEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        let loaded = root.clone();
        driver.update(cx, |driver, cx| driver.load(Some(&loaded), cx));
        (driver, events, Folders { _home: home, _project: project, root })
    }

    /// Lets the backend and the timers run, real time passing, until `done` holds.
    fn settle(cx: &mut TestAppContext, what: &str, done: &dyn Fn(&mut TestAppContext) -> bool) {
        for _ in 0..1000 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what} did not happen within ten seconds");
    }

    fn stored(cx: &mut TestAppContext, key: &str) -> Option<String> {
        cx.update(|cx| cx.global::<UiStorage>().get(key).map(str::to_owned))
    }

    fn lines_of(driver: &Entity<DebugDriver>, path: &str, cx: &mut TestAppContext) -> Vec<u32> {
        driver.read_with(cx, |driver, cx| {
            driver.debugger.read(cx).breakpoints.lines.breakpoints_of(path).iter().map(|b| b.line).collect()
        })
    }

    fn synced(driver: &Entity<DebugDriver>, cx: &mut TestAppContext) -> bool {
        driver.read_with(cx, |driver, cx| driver.debugger.read(cx).breakpoints.next_deadline().is_none())
    }

    fn toggle_at(driver: &Entity<DebugDriver>, path: &str, rope: &Rope, line: u32, cx: &mut TestAppContext) {
        let at = crate::editor::buffer::line_range(rope, line).start;
        let editor = ActiveEditor { path, language: "python", doc: rope, selection: (at, at), head: at, comments: &[] };
        driver.update(cx, |driver, cx| driver.run_action("debug.toggleBreakpoint", None, Some(&editor), cx));
    }

    // Not from a spec: Toggle Line Breakpoint stores the breakpoint for the project and sends it to
    // the backend once the edits rest; the project read again has it, and toggling again removes it.
    #[gpui_kit::test]
    async fn toggled_breakpoints_are_stored_sent_and_read_back(cx: &mut TestAppContext) {
        let (driver, events, folders) = project(cx, &[]).await;
        let path = format!("{}/app.py", folders.root);
        let rope = Rope::from_str("x = 1\ny = 2\n");
        toggle_at(&driver, &path, &rope, 1, cx);
        assert_eq!(lines_of(&driver, &path, cx), [1]);
        let key = store::storage_key(&folders.root);
        assert!(stored(cx, &key).is_some_and(|value| value.contains("\"line\":1")), "{:?}", stored(cx, &key));
        settle(cx, "the breakpoint sync", &|cx| synced(&driver, cx));

        let again = cx.update(|cx| cx.new(|cx| DebugDriver::new("w1", cx)));
        let root = folders.root.clone();
        again.update(cx, |driver, cx| driver.load(Some(&root), cx));
        assert_eq!(lines_of(&again, &path, cx), [1]);

        toggle_at(&driver, &path, &rope, 1, cx);
        assert!(lines_of(&driver, &path, cx).is_empty());
        assert!(stored(cx, &key).is_some_and(|value| !value.contains("\"line\":1")), "{:?}", stored(cx, &key));
        settle(cx, "the second breakpoint sync", &|cx| synced(&driver, cx));
        let notified = events.borrow().iter().any(|event| matches!(event, DebugDriverEvent::Notify(_)));
        assert!(!notified, "{:?}", events.borrow());
    }

    // Not from a spec: Mute Breakpoints applies to every breakpoint and is stored with them.
    #[gpui_kit::test]
    async fn muting_breakpoints_is_stored_and_sent(cx: &mut TestAppContext) {
        let (driver, _, folders) = project(cx, &[]).await;
        let path = format!("{}/app.py", folders.root);
        toggle_at(&driver, &path, &Rope::from_str("x = 1\n"), 0, cx);
        driver.update(cx, |driver, cx| driver.run_action("debug.muteBreakpoints", None, None, cx));
        assert!(driver.read_with(cx, |driver, cx| driver.debugger.read(cx).breakpoints_muted()));
        settle(cx, "the breakpoint sync", &|cx| synced(&driver, cx));
        let again = cx.update(|cx| cx.new(|cx| DebugDriver::new("w1", cx)));
        let root = folders.root.clone();
        again.update(cx, |driver, cx| driver.load(Some(&root), cx));
        assert!(again.read_with(cx, |driver, cx| driver.debugger.read(cx).breakpoints_muted()));
    }

    // Not from a spec: a start whose adapter is not installed offers to install it and debug again,
    // and no session is left behind.
    #[gpui_kit::test]
    async fn a_debug_start_without_its_adapter_offers_to_install_it(cx: &mut TestAppContext) {
        let shell = "id = \"shellscript\"\nname = \"Shell Script\"\nextensions = [\"sh\"]\n\n\
                     line-comment = \"#\"\n\n[debugger]\nadapter = \"bash-debug\"\n\
                     command = \"openidea-test-missing-bash-debug\"\n";
        let (driver, events, _folders) = project(cx, &[("shellscript.toml", shell)]).await;
        let config = ide_run::RunConfig {
            command: "script.sh".to_owned(),
            debug_adapter: Some("bash-debug".to_owned()),
            ..blank_config("script", "script", "program")
        };
        let arg = json!({ "config": serde_json::to_value(&config).expect("a configuration as JSON") });
        driver.update(cx, |driver, cx| driver.run_action("debug.startConfig", Some(&arg), None, cx));
        let offered = |events: &[DebugDriverEvent]| {
            events.iter().find_map(|event| match event {
                DebugDriverEvent::Notify(request) if request.title == "Bash Debug is not installed" => {
                    Some(request.actions.iter().map(|action| action.label.clone()).collect::<Vec<_>>())
                }
                _ => None,
            })
        };
        settle(cx, "the failed start", &|_| offered(&events.borrow()).is_some());
        assert_eq!(offered(&events.borrow()), Some(vec!["Install and Debug".to_owned()]));
        let live = driver.read_with(cx, |driver, cx| driver.debugger.read(cx).state.sessions_running());
        assert!(!live);
        // Install and Debug asks the tools installer for the adapter.
        let command = crate::debugger::state::INSTALL_AND_DEBUG;
        assert!(driver.update(cx, |driver, cx| driver.notification_command(command, cx)));
        let installs = events.borrow().iter().any(|event| {
            matches!(event, DebugDriverEvent::RunAction { id, arg } if id == "tools.install"
                && arg.as_ref() == Some(&json!("bash-debug")))
        });
        assert!(installs, "{:?}", events.borrow());
    }
}
