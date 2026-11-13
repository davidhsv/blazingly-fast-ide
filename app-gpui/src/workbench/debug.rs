//! The debugger of a project window: its [`DebugDriver`], the Debug tool window with the debug
//! console, the Breakpoints dialog, breakpoints and the execution line in the editors, and the
//! `debug.*` actions with `run.debug` / `run.debugConfig`.
//!
//! The Breakpoints dialog is a layer of its own over the window (as Edit Configurations is): only
//! its own buttons and Escape close it.

use super::services_window::on_workbench;
use super::{Panel, Workbench, for_window};
use crate::debugger::actions::{ActiveEditor, Dispatch, ViewCommand};
use crate::debugger::breakpoints_view::{BreakpointsDialog, BreakpointsDialogEvent};
use crate::debugger::driver::{DebugDriver, DebugDriverEvent};
use crate::debugger::editor::breakpoint_gutter::{
    ClickOutcome, GutterButton, GutterEditor, GutterPress, PressOutcome, build_markers,
};
use crate::debugger::effects::{DEBUG_TOOL_WINDOW, Effect, run_notification};
use crate::debugger::focused_view::{DebugView, FRAMES_FOCUS, VARIABLES_FOCUS};
use crate::debugger::frames_view::{FramesView, FramesViewEvent};
use crate::debugger::state::{CloseTab, Ctx, DebugState};
use crate::debugger::variables_view::{VariablesView, VariablesViewEvent};
use crate::debugger::window_view::{DebugWindowEvent, DebugWindowView};
use crate::editor::buffer;
use crate::editor::marks::{Mark, MarkKind, MarkLayer};
use crate::editor::view::{Editor, EditorEvent};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::context::set_context;
use crate::platform::i18n::{Localization, Param};
use crate::run::store::ChooserMode;
use crate::theme;
use crate::workbench::notifications::NotificationLevel;
use futures::channel::oneshot;
use gpui_kit::{
    AnyElement, App, ClipboardItem, Context, Entity, EntityId, FocusHandle, Focusable, IntoElement, Render,
    SharedString, Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ropey::Rope;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;

/// The transitions' surroundings, from the fields of a debugger and its host.
macro_rules! ctx {
    ($debugger:ident, $host:ident) => {
        Ctx { breakpoints: &mut $debugger.breakpoints, env: $host.env, i18n: &mut *$host.i18n }
    };
}

/// The `debug.*` actions this window carries out on its debugger. Evaluate Expression, Attach to
/// Process, Smart Step Into, Quick Evaluate and the console history have no view here yet.
const DEBUG_ACTIONS: [&str; 28] = [
    "debug.startConfig",
    "debug.stop",
    "debug.reloadChangedClasses",
    "debug.cancelHotSwap",
    "debug.rerun",
    "debug.resume",
    "debug.pause",
    "debug.hotReload",
    "debug.hotRestart",
    "debug.stepOver",
    "debug.stepInto",
    "debug.stepOut",
    "debug.forceStepOver",
    "debug.forceStepInto",
    "debug.showExecutionPoint",
    "debug.resetFrame",
    "debug.setValue",
    "debug.jumpToSource",
    "debug.runToCursor",
    "debug.forceRunToCursor",
    "debug.toggleBreakpoint",
    "debug.toggleTemporaryBreakpoint",
    "debug.toggleMethodBreakpoint",
    "debug.toggleFieldWatchpoint",
    "debug.viewBreakpoints",
    "debug.removeAllBreakpoints",
    "debug.showToolWindow",
    "debug.muteBreakpoints",
];

/// The debugger of the open project.
pub(super) struct DebugSlot {
    driver: Entity<DebugDriver>,
    /// The Debug tool window, once it was shown.
    window: Option<DebugWindowSlot>,
    /// The Breakpoints dialog, while it is open.
    breakpoints: Option<BreakpointsWindow>,
    /// What tells the window of a press in each editor's gutter, by editor.
    gutters: HashMap<EntityId, Subscription>,
    _subscriptions: Vec<Subscription>,
}

/// The Debug tool window's view and panes, and what the window listens to them for.
struct DebugWindowSlot {
    view: Entity<DebugWindowView>,
    frames: Entity<FramesView>,
    variables: Entity<VariablesView>,
    _subscriptions: Vec<Subscription>,
}

/// The Breakpoints dialog over the window.
struct BreakpointsWindow {
    dialog: Entity<BreakpointsDialog>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// What the debugger reads of an editor, owned so the editor is free again while it decides.
struct EditorSnapshot {
    path: String,
    language: String,
    rope: Rope,
    selection: (u32, u32),
    head: u32,
}

impl EditorSnapshot {
    fn of(editor: &Entity<Editor>, cx: &App) -> Self {
        let editor = editor.read(cx);
        let selection = editor.selection();
        Self {
            path: editor.path().to_string_lossy().into_owned(),
            language: editor.language().as_str().to_owned(),
            rope: editor.rope().clone(),
            selection: (selection.start, selection.end),
            head: editor.caret(),
        }
    }

    fn active(&self) -> ActiveEditor<'_> {
        ActiveEditor {
            path: &self.path,
            language: &self.language,
            doc: &self.rope,
            selection: self.selection,
            head: self.head,
            comments: &[],
        }
    }

    fn gutter(&self) -> GutterEditor<'_> {
        GutterEditor { path: &self.path, language: &self.language, doc: &self.rope, comments: &[] }
    }
}

/// Runs `act` with the workbench of the action's window.
fn on_debug(
    window: &mut Window,
    cx: &mut App,
    act: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>),
) -> anyhow::Result<()> {
    on_workbench(window, cx, act)
}

/// Registers the debugger's actions and the Run feature's Debug entries.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    for id in DEBUG_ACTIONS {
        if id == "debug.muteBreakpoints" {
            registry.handle_toggle(
                id,
                |window, cx| {
                    let workbench = for_window(window, cx);
                    let driver = workbench.as_ref().and_then(|workbench| workbench.read(cx).debug_driver().cloned());
                    driver.is_some_and(|driver| driver.read(cx).debugger().read(cx).breakpoints_muted())
                },
                move |call, window, cx| {
                    let arg = call.arg.cloned();
                    on_debug(window, cx, move |workbench, window, cx| {
                        workbench.run_debug_action(id, arg.as_ref(), window, cx);
                    })
                },
            );
            continue;
        }
        registry.handle(id, move |call, window, cx| {
            let arg = call.arg.cloned();
            on_debug(window, cx, move |workbench, window, cx| workbench.run_debug_action(id, arg.as_ref(), window, cx))
        });
    }
    registry.handle("run.debug", |_, window, cx| {
        on_debug(window, cx, |workbench, window, cx| workbench.debug_selected(window, cx))
    });
    registry.handle("run.debugConfig", |call, window, cx| {
        let arg = call.arg.cloned();
        on_debug(window, cx, move |workbench, window, cx| workbench.debug_config(arg, window, cx))
    });
}

impl Workbench {
    /// The debugger of the window's project.
    pub fn debug_driver(&self) -> Option<&Entity<DebugDriver>> {
        self.debug.as_ref().map(|slot| &slot.driver)
    }

    /// The Debug tool window's view, once it was shown.
    pub fn debug_window_view(&self) -> Option<&Entity<DebugWindowView>> {
        self.debug.as_ref()?.window.as_ref().map(|window| &window.view)
    }

    /// The Breakpoints dialog, while it is open.
    pub fn breakpoints_dialog(&self) -> Option<&Entity<BreakpointsDialog>> {
        self.debug.as_ref()?.breakpoints.as_ref().map(|open| &open.dialog)
    }

    // ---------------------------------------------------------------------------------------------
    // The driver

    /// The debugger of a project just opened at `root`: its breakpoints and watches are read, and
    /// the window follows what it asks for and what changes in it.
    pub(super) fn mount_debugger(&mut self, root: &str, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let save_all: crate::run::driver::SaveAll = std::rc::Rc::new(move |cx: &mut App| {
            use futures::FutureExt as _;
            match weak.upgrade() {
                Some(workbench) => workbench.read(cx).save_all(cx).boxed_local(),
                None => futures::future::ready(Ok(())).boxed_local(),
            }
        });
        let label = self.label.clone();
        let driver = cx.new(|cx| DebugDriver::new(&label, cx).with_save_all(save_all));
        let debugger = driver.read(cx).debugger().clone();
        let subscriptions = vec![
            cx.subscribe_in(&driver, window, Self::debug_driver_event),
            cx.observe_in(&debugger, window, |workbench, _, window, cx| workbench.debugger_changed(window, cx)),
        ];
        let root = root.to_owned();
        driver.update(cx, |driver, cx| driver.load(Some(&root), cx));
        self.debug = Some(DebugSlot {
            driver,
            window: None,
            breakpoints: None,
            gutters: HashMap::new(),
            _subscriptions: subscriptions,
        });
    }

    /// The project goes: its debug sessions stop and the debugger goes with it.
    pub(super) fn reset_debugger(&mut self, cx: &mut Context<Self>) {
        if let Some(slot) = self.debug.take() {
            slot.driver.update(cx, |driver, cx| driver.reset(cx));
        }
    }

    /// A debug event of the backend for this window.
    pub(super) fn debug_backend_event(&mut self, event: &ide_core::IdeEvent, cx: &mut Context<Self>) {
        if let Some(driver) = self.debug_driver().cloned() {
            driver.update(cx, |driver, cx| driver.backend_event(event, cx));
        }
    }

    /// A notification button: true when it was one of the debugger's.
    pub(super) fn debug_notification_command(&mut self, command: &str, cx: &mut Context<Self>) -> bool {
        let Some(driver) = self.debug_driver().cloned() else { return false };
        driver.update(cx, |driver, cx| driver.notification_command(command, cx))
    }

    /// Carries out what the debug driver asks of the window.
    fn debug_driver_event(
        &mut self,
        _: &Entity<DebugDriver>,
        event: &DebugDriverEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            DebugDriverEvent::Notify(request) => self.notify(request, cx),
            DebugDriverEvent::ExpireNotification(key) => {
                self.notifications.expire_notification(&key);
                self.sync_notifications(cx);
            }
            DebugDriverEvent::ShowToolWindow { focus } => {
                self.with_layout(window, cx, |layout| layout.show(DEBUG_TOOL_WINDOW, focus));
            }
            DebugDriverEvent::HideToolWindow => self.with_layout(window, cx, |layout| layout.hide(DEBUG_TOOL_WINDOW)),
            DebugDriverEvent::RunAction { id, arg } => window.defer(cx, move |window, cx| {
                if !execute_action_from(ActionSource::Other, &id, arg.as_ref(), window, cx) {
                    tracing::info!("{id} is not available in this window");
                }
            }),
            DebugDriverEvent::OpenFile { path, line, column, focus } => {
                self.open_debug_file(path, line, column, focus, window, cx);
            }
            DebugDriverEvent::StatusInfo(text) => {
                let mut request = run_notification(NotificationLevel::Info, text);
                request.key = Some("debug.status".to_owned());
                self.notify(request, cx);
            }
            DebugDriverEvent::FocusAppWindow => window.activate_window(),
            DebugDriverEvent::ValueSet { request, result } => {
                if let Some(variables) = self.debug_variables_view() {
                    variables.update(cx, |variables, cx| variables.value_set(request, result, window, cx));
                }
            }
        }
    }

    /// Opens a frame's (or a breakpoint's) source at its line. The editor takes the keyboard only
    /// when `focus` says so; otherwise the keyboard goes back to where it was.
    fn open_debug_file(
        &mut self,
        path: String,
        line: u32,
        column: u32,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let restore = if focus { None } else { window.focused(cx) };
        self.open_project_file_at(PathBuf::from(path), line, column, window, cx);
        if let Some(restore) = restore {
            window.focus(&restore, cx);
        }
    }

    /// Something in the debugger changed: the editors' marks, the Breakpoints dialog and the context
    /// keys follow it.
    fn debugger_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_debug_marks(window, cx);
        self.sync_breakpoints_dialog(window, cx);
        cx.notify();
    }

    /// Publishes the context keys of the debugger: its sessions' state and which of its views has
    /// the keyboard.
    pub(super) fn publish_debug_context(&self, window: &mut Window, cx: &mut App) {
        let state = self.debug_driver().map(|driver| {
            let debugger = driver.read(cx).debugger().read(cx);
            (debugger.state.published().context, debugger.focused.current())
        });
        let (context, focused) = state.unwrap_or_default();
        for (key, value) in context.entries() {
            set_context(window, cx, key, value);
        }
        let view = focused.map(|commands| commands.view);
        set_context(window, cx, FRAMES_FOCUS, view == Some(DebugView::Frames));
        set_context(window, cx, VARIABLES_FOCUS, view == Some(DebugView::Variables));
    }

    // ---------------------------------------------------------------------------------------------
    // Actions

    /// Runs the debugger action `id` with the editor that has the focus, and does what only a view
    /// of the window can.
    pub fn run_debug_action(&mut self, id: &str, arg: Option<&Value>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let editor = self.editor().map(|editor| EditorSnapshot::of(editor, cx));
        let active = editor.as_ref().map(EditorSnapshot::active);
        let dispatch = driver.update(cx, |driver, cx| driver.run_action(id, arg, active.as_ref(), cx));
        match dispatch {
            Dispatch::View(ViewCommand::SetValue) => {
                if let Some(variables) = self.debug_variables_view() {
                    variables.update(cx, |variables, cx| variables.begin_set_value(window, cx));
                }
            }
            Dispatch::View(ViewCommand::JumpToSource) => {
                let focused = driver.read(cx).debugger().read(cx).focused.current().map(|commands| commands.view);
                match (focused, self.debug.as_ref().and_then(|slot| slot.window.as_ref())) {
                    (Some(DebugView::Frames), Some(open)) => {
                        open.frames.update(cx, |frames, cx| frames.jump_to_source(cx));
                    }
                    (Some(DebugView::Variables), Some(open)) => {
                        open.variables.update(cx, |variables, cx| variables.jump_to_source(cx));
                    }
                    _ => {}
                }
            }
            Dispatch::View(ViewCommand::BrowseConsoleHistory) | Dispatch::QuickEvaluate(_) => {
                tracing::debug!("{id} has no view in this window");
            }
            Dispatch::Done | Dispatch::Unknown => {}
        }
        // The choice between several debuggable configurations is the run configuration chooser's.
        let chose = driver.update(cx, |driver, cx| {
            driver.debugger().update(cx, |debugger, _| debugger.overlays.chooser.take().is_some())
        });
        if chose {
            self.choose_config(ChooserMode::Debug, window, cx);
        }
    }

    /// Does `then` in the window once the run driver has listed the configurations.
    fn after_configs_listed(
        &mut self,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.run_driver().cloned() else { return };
        let (sender, listed) = oneshot::channel();
        run.update(cx, |run, cx| {
            run.when_loaded(
                move |_, _| {
                    // The receiver is gone only with the window.
                    let _ = sender.send(());
                },
                cx,
            );
        });
        cx.spawn_in(window, async move |workbench, cx| {
            // The project closed before its configurations were listed: there is nothing to debug.
            if listed.await.is_err() {
                return;
            }
            let _ = workbench.update_in(cx, then);
        })
        .detach();
    }

    /// Run › Debug: the selected configuration, once the configurations are listed; the active
    /// file's context when "Current File" is selected; the chooser when nothing is.
    pub fn debug_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.after_configs_listed(
            |workbench, window, cx| {
                let Some(run) = workbench.run_driver().cloned() else { return };
                let (selected, current_file) = {
                    let store = run.read(cx).store().read(cx);
                    (store.selected_config().cloned(), store.current_file_selected())
                };
                if current_file {
                    return window.defer(cx, |window, cx| {
                        if !execute_action_from(ActionSource::Other, "run.debugContext", None, window, cx) {
                            tracing::info!("run.debugContext is not available in this window");
                        }
                    });
                }
                match selected {
                    Some(config) => workbench.debug_config(serde_json::to_value(config).ok(), window, cx),
                    None => workbench.choose_config(ChooserMode::Debug, window, cx),
                }
            },
            window,
            cx,
        );
    }

    /// Debug Configuration: the configuration `arg` names (an id, a configuration, or `{ config,
    /// show }`) is started under the debugger once the configurations are listed.
    pub fn debug_config(&mut self, arg: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        if self.debug_driver().is_none() {
            return;
        }
        let show = arg.as_ref().and_then(|arg| arg.get("show")).and_then(Value::as_bool);
        self.after_configs_listed(
            move |workbench, window, cx| {
                let Some(run) = workbench.run_driver().cloned() else { return };
                let Some(config) = run.update(cx, |run, cx| run.config_from_arg(arg.as_ref(), cx)) else { return };
                let mut start = json!({ "config": serde_json::to_value(&config).unwrap_or(Value::Null) });
                if let Some(show) = show {
                    start["show"] = Value::Bool(show);
                }
                workbench.run_debug_action("debug.startConfig", Some(&start), window, cx);
            },
            window,
            cx,
        );
    }

    // ---------------------------------------------------------------------------------------------
    // The Debug tool window

    /// The Debug tool window over the project's debugger, with its console.
    pub(super) fn mount_debug_window(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        let debugger = self.debug_driver()?.read(cx).debugger().clone();
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let sort = crate::platform::settings::effective(&self.label, cx).debugger.sort_values;
        let view = cx.new(|cx| DebugWindowView::new(debugger.clone(), window, cx));
        let console = cx.new(|cx| DebugConsolePane::new(debugger, cx));
        view.update(cx, |view, cx| view.set_console(Some(console.into()), cx));
        let (frames, variables) = {
            let view = view.read(cx);
            (view.frames().clone(), view.variables().clone())
        };
        frames.update(cx, |frames, cx| frames.set_project_root(root, cx));
        variables.update(cx, |variables, _| variables.set_sort_values(sort));
        let subscriptions = vec![
            cx.subscribe_in(&view, window, Self::debug_window_event),
            cx.subscribe_in(&frames, window, Self::debug_frames_event),
            cx.subscribe_in(&variables, window, Self::debug_variables_event),
        ];
        let slot = self.debug.as_mut()?;
        slot.window = Some(DebugWindowSlot { view: view.clone(), frames, variables, _subscriptions: subscriptions });
        Some(Panel::new(view, None))
    }

    fn debug_variables_view(&self) -> Option<Entity<VariablesView>> {
        self.debug.as_ref()?.window.as_ref().map(|open| open.variables.clone())
    }

    /// Runs a debugger action once this update is over: the action may reach for the workbench.
    fn defer_debug_action(id: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        window.defer(cx, move |window, cx| {
            if !execute_action_from(ActionSource::Toolbar, id, None, window, cx) {
                tracing::info!("{id} is not available now");
            }
        });
    }

    fn debug_window_event(
        &mut self,
        _: &Entity<DebugWindowView>,
        event: &DebugWindowEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        match event.clone() {
            DebugWindowEvent::SelectSession(id) => {
                driver.update(cx, |driver, cx| driver.change(cx, |debugger, _| debugger.state.select_session(id)));
            }
            DebugWindowEvent::CloseTab(id) => {
                let closed = driver.update(cx, |driver, cx| {
                    driver.change(cx, |debugger, host| {
                        let mut ctx = ctx!(debugger, host);
                        debugger.state.close_session_tab(id, &mut ctx)
                    })
                });
                if let CloseTab::Confirm { name } = closed {
                    self.confirm_close_session(id, &name, window, cx);
                }
            }
            DebugWindowEvent::RunAction(id) => Self::defer_debug_action(id, window, cx),
            DebugWindowEvent::ShowMoreMenu(position) => {
                let items = DebugWindowView::more_menu();
                self.show_menu(&items, position, |_, (), _, _| {}, window, cx);
            }
        }
    }

    /// Asks whether to terminate the live session of a tab being closed; yes closes it.
    fn confirm_close_session(
        &mut self,
        id: ide_core::DebugSessionId,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (title, message, ok, cancel) = {
            let i18n = &mut cx.default_global::<Localization>().0;
            (
                i18n.tx_built(&["Process '", "' Is Running"], &[Param::from(name)]),
                i18n.tx_built(&["Do you want to terminate the process '", "'?"], &[Param::from(name)]),
                i18n.tx("Terminate"),
                i18n.tx("Cancel"),
            )
        };
        let answers = [ok.as_str(), cancel.as_str()];
        let answer = window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(message.as_str()), &answers, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = workbench.update(cx, |workbench, cx| {
                let Some(driver) = workbench.debug_driver().cloned() else { return };
                driver.update(cx, |driver, cx| {
                    driver.change(cx, |debugger, host| {
                        let mut ctx = ctx!(debugger, host);
                        debugger.state.close_confirmed(id, &mut ctx);
                    });
                });
            });
        })
        .detach();
    }

    fn debug_frames_event(
        &mut self,
        frames: &Entity<FramesView>,
        event: &FramesViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        match event.clone() {
            FramesViewEvent::SelectThread { session, thread } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| debugger.state.select_thread(session, thread));
            }),
            FramesViewEvent::SelectFrame { session, frame, reveal } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| debugger.state.select_frame(session, frame, reveal));
            }),
            FramesViewEvent::JumpToSource { session, frame } => {
                let opened = driver.read(cx).debugger().read(cx).state.session(session).and_then(|session| {
                    let frame = session.frames.iter().find(|candidate| candidate.id == frame)?;
                    DebugState::reveal_frame(frame, true)
                });
                if let Some(Effect::OpenFile { path, line, column, focus }) = opened {
                    self.open_debug_file(path, line, column, focus, window, cx);
                }
            }
            FramesViewEvent::RunAction(id) => Self::defer_debug_action(id, window, cx),
            FramesViewEvent::ShowFrameMenu(position) => {
                let items = frames.read(cx).frame_menu(cx);
                let frames = frames.clone();
                let run = move |_: &mut Self, command, _: &mut Window, cx: &mut Context<Self>| {
                    frames.update(cx, |frames, cx| frames.run_frame_command(command, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            FramesViewEvent::ShowThreadMenu(position) => {
                let items = frames.read(cx).thread_menu(cx);
                let frames = frames.clone();
                let run = move |_: &mut Self, thread, _: &mut Window, cx: &mut Context<Self>| {
                    frames.update(cx, |frames, cx| frames.choose_thread(thread, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            FramesViewEvent::Focused(commands) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| {
                    debugger.focused.focus(commands);
                });
            }),
            FramesViewEvent::Blurred(view) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| {
                    debugger.focused.blur(view);
                });
            }),
        }
    }

    fn debug_variables_event(
        &mut self,
        variables: &Entity<VariablesView>,
        event: &VariablesViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let target = variables.downgrade();
        match event.clone() {
            VariablesViewEvent::LoadScopes { request, session, frame } => {
                let loading = driver.read(cx).scopes(session, frame, cx);
                cx.spawn(async move |_, cx| {
                    let scopes = loading.await;
                    let _ = target.update(cx, |variables, cx| variables.scopes_loaded(request, scopes, cx));
                })
                .detach();
            }
            VariablesViewEvent::LoadChildren { request, session, reference } => {
                let loading = driver.read(cx).children(session, reference, cx);
                cx.spawn(async move |_, cx| {
                    let children = loading.await;
                    let _ = target.update(cx, |variables, cx| variables.children_loaded(request, children, cx));
                })
                .detach();
            }
            VariablesViewEvent::Evaluate { request, session, expression, context } => {
                let evaluating = driver.read(cx).evaluate(session, &expression, context, cx);
                cx.spawn(async move |_, cx| {
                    let value = evaluating.await;
                    let _ = target.update(cx, |variables, cx| variables.evaluated(request, value, cx));
                })
                .detach();
            }
            VariablesViewEvent::SetValue { request, session, container, name, evaluate_name, value } => {
                driver.update(cx, |driver, cx| {
                    driver.set_value(request, session, container, &name, evaluate_name.as_deref(), &value, cx);
                });
            }
            VariablesViewEvent::AddWatch(expression) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| debugger.state.add_watch(&expression));
            }),
            VariablesViewEvent::RemoveWatch(expression) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| debugger.state.remove_watch(&expression));
            }),
            // The edited watch moves to the end of the list.
            VariablesViewEvent::EditWatch { old, new } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| {
                    debugger.state.remove_watch(&old);
                    debugger.state.add_watch(&new);
                });
            }),
            VariablesViewEvent::CopyValue { session, value, reference } => {
                let copying = driver.read(cx).copy_value(session, value, reference, cx);
                cx.spawn(async move |workbench, cx| {
                    let copied = copying.await;
                    let _ = workbench.update(cx, |workbench, cx| match copied {
                        Ok(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                        Err(error) => {
                            let mut request = run_notification(NotificationLevel::Error, "Cannot copy the value");
                            request.body = Some(error);
                            workbench.notify(request, cx);
                        }
                    });
                })
                .detach();
            }
            VariablesViewEvent::AddFieldWatchpoint { session, container, name, label } => {
                let frame = driver.read(cx).debugger().read(cx).state.session(session).and_then(|s| s.selected_frame);
                let asking = driver.read(cx).data_breakpoint_info(session, container, name.clone(), frame, cx);
                cx.spawn(async move |workbench, cx| {
                    let info = asking.await;
                    let _ = workbench.update(cx, |workbench, cx| {
                        let data_id = info.ok().and_then(|info| info.data_id);
                        match (data_id, workbench.debug_driver().cloned()) {
                            (Some(data_id), Some(driver)) => driver.update(cx, |driver, cx| {
                                driver.change(cx, |debugger, host| {
                                    let special = &mut debugger.breakpoints.special;
                                    special.add_field_watchpoint(&data_id, &label, None, None, host.env.now_ms);
                                });
                            }),
                            _ => {
                                let title = format!("“{name}” can't be watched");
                                workbench.notify(run_notification(NotificationLevel::Info, title), cx);
                            }
                        }
                    });
                })
                .detach();
            }
            // The selected frame's source; the declaration of a local is not looked for.
            VariablesViewEvent::JumpToSource { .. } => {
                let opened = driver.read(cx).debugger().read(cx).state.active_session().and_then(|session| {
                    let frame = session.frames.iter().find(|frame| Some(frame.id) == session.selected_frame)?;
                    DebugState::reveal_frame(frame, true)
                });
                if let Some(Effect::OpenFile { path, line, column, focus }) = opened {
                    self.open_debug_file(path, line, column, focus, window, cx);
                }
            }
            VariablesViewEvent::ShowMenu(position) => {
                let items = variables.read(cx).variable_menu(cx);
                let variables = variables.clone();
                let run = move |_: &mut Self, command, window: &mut Window, cx: &mut Context<Self>| {
                    variables.update(cx, |variables, cx| variables.run_variable_command(command, window, cx));
                };
                self.show_menu(&items, position, run, window, cx);
            }
            VariablesViewEvent::Focused(commands) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| {
                    debugger.focused.focus(commands);
                });
            }),
            VariablesViewEvent::Blurred(view) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, _| {
                    debugger.focused.blur(view);
                });
            }),
        }
    }

    // ---------------------------------------------------------------------------------------------
    // The editors

    /// Listens to `editor`'s gutter, and draws the breakpoints and the execution line in it.
    pub(super) fn watch_debug_editor(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.debug.as_mut() else { return };
        let pressed = cx.subscribe_in(editor, window, |workbench, editor, event: &EditorEvent, _, cx| {
            if let &EditorEvent::GutterPressed { line, shift, alt } = event {
                workbench.debug_gutter_pressed(editor, line, shift, alt, cx);
            }
        });
        slot.gutters.insert(editor.entity_id(), pressed);
        self.sync_debug_marks_of(editor, cx);
    }

    /// A press on `line` of an editor's gutter: a breakpoint goes there, or goes away.
    fn debug_gutter_pressed(
        &mut self,
        editor: &Entity<Editor>,
        line: u32,
        shift: bool,
        alt: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let snapshot = EditorSnapshot::of(editor, cx);
        let press = GutterPress { button: GutterButton::Primary, shift, alt };
        let gutter = snapshot.gutter();
        let pressed = driver.update(cx, |driver, cx| driver.gutter_press(&gutter, line, press, cx));
        match pressed {
            PressOutcome::Blocked(reason) => {
                let text = cx.default_global::<Localization>().0.tx(reason);
                let mut request = run_notification(NotificationLevel::Info, text);
                request.key = Some("debug.status".to_owned());
                self.notify(request, cx);
            }
            // The editor reports a click: the release is not told apart from the press.
            PressOutcome::AwaitRelease => {
                let clicked = driver.update(cx, |driver, cx| driver.gutter_click(&gutter, line, press, cx));
                if let ClickOutcome::OpenProperties { id } = clicked {
                    driver.update(cx, |driver, cx| {
                        driver.change(cx, |debugger, _| {
                            let select = crate::debugger::overlays::BreakpointSelection { path: snapshot.path, id };
                            debugger.overlays.open_breakpoints_dialog(Some(select));
                        });
                    });
                }
            }
            PressOutcome::Ignored | PressOutcome::Toggled => {}
        }
    }

    /// Draws the breakpoints and the execution line in every editor of the project, and forgets the
    /// gutters of the editors that are gone.
    fn sync_debug_marks(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let editors = self.editors();
        if let Some(slot) = self.debug.as_mut() {
            slot.gutters.retain(|id, _| editors.iter().any(|editor| editor.entity_id() == *id));
        }
        for editor in &editors {
            self.sync_debug_marks_of(editor, cx);
        }
    }

    fn sync_debug_marks_of(&self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        let Some(driver) = self.debug_driver() else { return };
        let (path, rope) = {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.rope().clone())
        };
        let (breakpoints, execution) = {
            let debugger = driver.read(cx).debugger().read(cx);
            let lines = buffer::line_count(&rope);
            let running = debugger.state.sessions_running();
            let (stored, special) = (&debugger.breakpoints.lines, &debugger.breakpoints.special);
            let markers = build_markers(stored, special, &path, lines, running);
            let line_mark = |line: u32, kind: MarkKind| Mark::new(buffer::line_range(&rope, line), kind);
            let breakpoints: Vec<Mark> = markers
                .iter()
                .map(|marker| {
                    let active = marker.look.tints_line();
                    let kind = if active { MarkKind::BreakpointLine } else { MarkKind::InactiveBreakpoint };
                    line_mark(marker.line, kind)
                })
                .collect();
            let point = debugger.state.execution_point();
            let execution: Vec<Mark> = point
                .path
                .as_deref()
                .filter(|at| *at == path && point.line < lines)
                .map(|_| line_mark(point.line, MarkKind::ExecutionLine))
                .into_iter()
                .collect();
            (breakpoints, execution)
        };
        editor.update(cx, |editor, cx| {
            editor.set_marks(MarkLayer::Breakpoints, breakpoints, cx);
            editor.set_marks(MarkLayer::Execution, execution, cx);
        });
    }

    // ---------------------------------------------------------------------------------------------
    // The Breakpoints dialog

    /// Opens the dialog when the debugger asks for it, and closes it when the debugger no longer does.
    fn sync_breakpoints_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.debug.as_ref() else { return };
        let debugger = slot.driver.read(cx).debugger().clone();
        let wanted = debugger.read(cx).overlays.breakpoints.clone();
        match (wanted, slot.breakpoints.is_some()) {
            (Some(select), false) => {
                let dialog = cx.new(|cx| BreakpointsDialog::new(debugger, select, cx));
                let events = cx.subscribe_in(&dialog, window, Self::breakpoints_dialog_event);
                let restore = window.focused(cx);
                window.focus(&dialog.focus_handle(cx), cx);
                dialog.update(cx, |dialog, cx| dialog.opened(cx));
                if let Some(slot) = self.debug.as_mut() {
                    slot.breakpoints = Some(BreakpointsWindow { dialog, restore, _events: events });
                }
                cx.notify();
            }
            (None, true) => {
                let open = self.debug.as_mut().and_then(|slot| slot.breakpoints.take());
                let restore = open.and_then(|open| open.restore);
                if let Some(target) = restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
                    window.focus(&target, cx);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn breakpoints_dialog_event(
        &mut self,
        dialog: &Entity<BreakpointsDialog>,
        event: &BreakpointsDialogEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.debug_driver().cloned() else { return };
        let close = |debugger: &mut crate::debugger::actions::Debugger| debugger.overlays.breakpoints = None;
        match event.clone() {
            BreakpointsDialogEvent::UpdateLine { path, id, patch } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| {
                    debugger.breakpoints.lines.update_breakpoint(&path, &id, host.env.now_ms, |b| patch.apply(b));
                });
            }),
            BreakpointsDialogEvent::UpdateSpecial { id, patch } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| {
                    debugger.breakpoints.special.update_special(&id, patch, host.env.now_ms);
                });
            }),
            BreakpointsDialogEvent::RemoveLine { path, id } => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| {
                    debugger.breakpoints.lines.remove_breakpoint(&path, &id, host.env.now_ms);
                });
            }),
            BreakpointsDialogEvent::RemoveSpecial(id) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| debugger.breakpoints.special.remove_special(&id, host.env.now_ms));
            }),
            BreakpointsDialogEvent::RemoveAll => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| {
                    debugger.breakpoints.lines.remove_all_breakpoints(host.env.now_ms);
                    debugger.breakpoints.special.remove_all_special(host.env.now_ms);
                });
            }),
            BreakpointsDialogEvent::AddMethodBreakpoint => {
                let added = driver.update(cx, |driver, cx| {
                    driver.change(cx, |debugger, host| {
                        debugger.breakpoints.special.add_method_breakpoint("", None, host.env.now_ms).id
                    })
                });
                dialog.update(cx, |dialog, cx| dialog.select(&added, cx));
            }
            BreakpointsDialogEvent::SetMuted(muted) => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| debugger.breakpoints.set_muted(muted, host.env.now_ms));
            }),
            BreakpointsDialogEvent::SetExceptionFilter { adapter, filter, enabled } => {
                driver.update(cx, |driver, cx| {
                    driver.change(cx, |debugger, host| {
                        let can_sync = host.env.can_sync();
                        debugger.breakpoints.exceptions.set_exception_filter(&adapter, &filter, enabled, can_sync);
                    });
                })
            }
            BreakpointsDialogEvent::RefreshExceptionFilters => driver.update(cx, |driver, cx| {
                driver.change(cx, |debugger, host| debugger.breakpoints.exceptions.refresh(host.env.desktop));
            }),
            BreakpointsDialogEvent::GoToSource { path, line } => {
                driver.update(cx, |driver, cx| driver.change(cx, |debugger, _| close(debugger)));
                self.open_debug_file(path, line, 0, true, window, cx);
            }
            BreakpointsDialogEvent::Close => {
                driver.update(cx, |driver, cx| driver.change(cx, |debugger, _| close(debugger)));
            }
        }
    }

    /// The Breakpoints dialog over the window, while it is open.
    pub(super) fn debug_breakpoints_layer(&self) -> Option<AnyElement> {
        let dialog = self.breakpoints_dialog()?.clone();
        let layer = div()
            .id("debug-breakpoints-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(dialog);
        Some(gpui_kit::deferred(layer).with_priority(1).into_any_element())
    }
}

/// The debug console of the shown session: its output, read-only.
pub struct DebugConsolePane {
    debugger: Entity<crate::debugger::actions::Debugger>,
    scroll: UniformListScrollHandle,
    _observer: Subscription,
}

impl DebugConsolePane {
    fn new(debugger: Entity<crate::debugger::actions::Debugger>, cx: &mut Context<Self>) -> Self {
        let observer = cx.observe(&debugger, |_, _, cx| cx.notify());
        Self { debugger, scroll: UniformListScrollHandle::new(), _observer: observer }
    }
}

impl Render for DebugConsolePane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = {
            let state = &self.debugger.read(cx).state;
            state.active().map_or(0, |session| state.console(session).len())
        };
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
        let list = uniform_list(
            "debug-console-lines",
            count,
            cx.processor(|pane: &mut Self, range: std::ops::Range<usize>, _, cx| {
                let state = &pane.debugger.read(cx).state;
                let texts: Vec<String> = state.active().map_or_else(Vec::new, |session| {
                    let lines = state.console(session).get(range.clone()).unwrap_or_default();
                    lines.iter().map(|line| line.text.trim_end_matches('\n').to_owned()).collect()
                });
                texts
                    .into_iter()
                    .map(|text| div().h(px(18.0)).px(px(8.0)).whitespace_nowrap().child(SharedString::from(text)))
                    .collect()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .size_full()
            .font_family(SharedString::from(family))
            .text_size(theme::length(cx, "fs-small"))
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg-editor"))
            .child(list)
    }
}
