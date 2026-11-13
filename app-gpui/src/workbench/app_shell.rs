//! Opening and closing as the application shell's models decide them: where a project opens
//! ([`OpenFlow`]: this window, a new one, or asked with "Don't ask again"), where loose files open
//! ([`LooseFiles`]), and what is settled before a window closes, its project is left or the app
//! quits ([`CloseRequests`] and [`CloseDecision`]: the exit confirmation, the running processes,
//! the unsaved documents).
//!
//! The models hand out steps; the functions here carry them out against the window's workbench and
//! the backend. A window's close requests are decided one at a time. A decision that asks nothing
//! settles at once, so that the window closes on the request itself; one that asks or waits goes on
//! in a task. Questions with a "Don't ask again" checkbox show as a [`PromptDialog`] over the window;
//! the unsaved-changes question, which has none, is the system prompt.
//!
//! The drivers take `(&mut Window, &mut App)` with the workbench as an [`Entity`] rather than
//! being workbench methods: they read every window's workbench, the asking one included.

use super::windows::{self, Target};
use super::{Content, Workbench, Workbenches, for_window};
use crate::app_models::close_guard::{
    Answer, CloseContext, CloseDecision, CloseInput, CloseRoute, CloseStep, Prompt, WindowCloseState,
    confirm_exit_enabled, leave_input, process_names, quit_input,
};
use crate::app_models::windows::{
    FileOpen, LooseFiles, LooseStep, OpenFlow, OpenOutcome, OpenStep, WindowState, WorkspaceMembers, editor_opens,
};
use crate::backend::Backend;
use crate::debugger::actions::Debugger;
use crate::debugger::state::{DebugSession, DebugState};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::settings;
use crate::run::store::RunStore;
use crate::vcs::prompt_dialogs::{CANCEL, DialogAnswer, DialogButton, Message, PromptDialog, PromptDialogEvent};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use futures::channel::oneshot;
use futures::future::{Either, select};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AsyncApp, AsyncWindowContext, Context, Entity, FocusHandle, Focusable, Global,
    PromptLevel, Subscription, Task, WeakEntity, Window, div, prelude::*,
};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_workspace::settings::SettingsScope;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

type This = WeakEntity<Workbench>;

/// How long the processes of a closing window are given to end after Stop, before they are
/// stopped again (which kills them), and again before the close gives up.
const TERMINATE_GRACE: Duration = Duration::from_secs(10);

/// Attach (to the window's workspace) is not offered: the Project view shows the project's own
/// folder only, so an attached folder would not be seen, and there is no trust question to ask
/// before attaching one.
const ATTACH: &str = "attach";

/// The dialog a close or open question waits on, over the window.
pub(super) struct AppDialog {
    view: Entity<PromptDialog>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// A quit is under way: every window's close is decided as part of it.
#[derive(Default)]
struct Quitting(bool);

impl Global for Quitting {}

fn quitting(cx: &App) -> bool {
    cx.try_global::<Quitting>().is_some_and(|quitting| quitting.0)
}

/// The messages in the UI language.
fn i18n(cx: &mut App) -> &I18n {
    &cx.default_global::<Localization>().0
}

/// A folder or file as the backend knows it once open: with links resolved.
fn canonical(path: &Path) -> String {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().into_owned()
}

fn workbench_of(handle: AnyWindowHandle, cx: &App) -> Option<Entity<Workbench>> {
    cx.try_global::<Workbenches>()?.0.get(&handle.window_id())?.upgrade()
}

fn label_of(handle: AnyWindowHandle, cx: &App) -> String {
    workbench_of(handle, cx).map(|workbench| workbench.read(cx).label.clone()).unwrap_or_default()
}

/// Project windows open, the asking one included.
fn window_count(cx: &App) -> u32 {
    u32::try_from(windows::windows(cx).len()).unwrap_or(u32::MAX)
}

/// The window whose project is `root`.
fn window_with_project(root: &Path, cx: &App) -> Option<AnyWindowHandle> {
    windows::windows(cx).into_iter().map(|(_, handle)| handle).find(|handle| {
        workbench_of(*handle, cx).is_some_and(|workbench| workbench.read(cx).project_root(cx).as_deref() == Some(root))
    })
}

/// The window, other than `except`, that already shows `file`: as a file of its project, or as the
/// file it edits on its own.
fn window_showing(file: &Path, except: AnyWindowHandle, cx: &App) -> Option<AnyWindowHandle> {
    let others = windows::windows(cx).into_iter().map(|(_, handle)| handle);
    others.filter(|handle| handle.window_id() != except.window_id()).find(|handle| {
        workbench_of(*handle, cx).is_some_and(|workbench| {
            let workbench = workbench.read(cx);
            workbench.project_root(cx).is_some_and(|root| file.starts_with(root))
                || workbench.light_edit_file(cx).as_deref() == Some(file)
        })
    })
}

/// The ids of the run sessions whose process is alive.
fn live_sessions(store: &RunStore) -> Vec<String> {
    store.sessions.iter().filter(|session| session.is_live()).map(|session| session.id.clone()).collect()
}

/// The debug sessions whose program still runs, without the child sessions of another one.
fn live_debug_sessions(state: &DebugState) -> impl Iterator<Item = &DebugSession> {
    state.sessions().values().filter(|session| session.is_live() && session.parent.is_none())
}

/// Writes `false` to the global setting `key` on behalf of the window; a failure is logged, the close
/// goes on either way.
fn turn_off(label: &str, key: &str, cx: &mut App) {
    if let Err(error) = settings::set(label, SettingsScope::Global, key, &Value::Bool(false), cx) {
        tracing::warn!("cannot turn off {key}: {}", error.message);
    }
}

impl Workbench {
    /// What a close decision of this window needs to know.
    fn close_state(&self, cx: &App) -> WindowCloseState {
        let settings = settings::effective(&self.label, cx);
        WindowCloseState {
            has_project: self.project_tree().is_some(),
            light_edit: self.is_light_edit(),
            settings_confirm_exit: confirm_exit_enabled(Some(&settings)),
            confirm_terminate: settings.run.confirm_terminate,
            processes: self.running_process_names(cx),
            dirty: self.unsaved_count(cx),
            project: match &self.content {
                Content::Project { name, .. } => Some(name.clone()),
                _ => None,
            },
        }
    }

    /// The names of the run configurations whose process is alive, each once.
    fn running_process_names(&self, cx: &App) -> Vec<String> {
        let mut names: Vec<String> = self
            .run_driver()
            .map(|driver| {
                let store = driver.read(cx).store().read(cx);
                store.sessions.iter().filter(|s| s.is_live()).map(|s| s.config.name.clone()).collect()
            })
            .unwrap_or_default();
        // A program running under the debugger is a running process too.
        if let Some(debugger) = self.debug_driver().map(|driver| driver.read(cx).debugger().read(cx)) {
            names.extend(live_debug_sessions(&debugger.state).map(|session| session.name.clone()));
        }
        process_names::<std::convert::Infallible>(Ok(names))
    }

    /// What the window shows, as the opening models read it.
    fn window_state(&self, cx: &App) -> WindowState {
        let settings = settings::effective(&self.label, cx);
        let workspace = match (&self.content, cx.try_global::<Backend>()) {
            (Content::Project { .. }, Some(backend)) => ide_host::ops::project::current(&backend.host, &self.label)
                .and_then(|info| info.workspace)
                .map(|workspace| WorkspaceMembers {
                    name: workspace.name,
                    projects: workspace.projects.iter().map(|path| path.to_string_lossy().into_owned()).collect(),
                }),
            _ => None,
        };
        WindowState {
            project: self.project_root(cx).map(|root| root.to_string_lossy().into_owned()),
            workspace,
            light_edit: self.is_light_edit(),
            remote: false,
            opening: matches!(self.content, Content::Opening(_)),
            open_project_in: settings.general.open_project_in,
        }
    }

    /// Whether the window shows a loose file on its own (LightEdit): in an editor or a viewer.
    fn is_light_edit(&self) -> bool {
        matches!(self.content, Content::Editor(_) | Content::Viewer(_))
    }

    /// The file the window edits on its own (LightEdit), or is opening to edit.
    fn light_edit_file(&self, cx: &App) -> Option<PathBuf> {
        match &self.content {
            Content::Editor(editor) => Some(editor.read(cx).path().to_path_buf()),
            Content::Viewer(view) => view.input.path.as_ref().map(PathBuf::from),
            Content::Opening(path) => Some(path.clone()),
            _ => None,
        }
    }

    /// How the last [`Workbench::open_project`] ended; `None` while it is still opening.
    fn open_result(&self) -> Option<Result<(), IdeError>> {
        match &self.content {
            Content::Opening(_) => None,
            Content::Project { .. } => Some(Ok(())),
            Content::Failed(message) => Some(Err(IdeError::new(ErrorKind::Io, message.to_string()))),
            // The trust question of a project opening in this window is still waiting.
            Content::Empty if self.trust_dialog().is_some() => None,
            Content::Empty | Content::Editor(_) | Content::Viewer(_) => {
                Some(Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")))
            }
        }
    }

    /// Opens files of the project as tabs, each at its caret position when it has one; the last one
    /// is the one shown.
    fn open_tabs(&mut self, files: Vec<FileOpen>, window: &mut Window, cx: &mut Context<Self>) {
        for file in files {
            let path = PathBuf::from(&file.path);
            match file.position {
                Some((line, column)) => self.open_project_file_at(path, line, column, window, cx),
                None => self.open_project_file(path, window, cx),
            }
        }
    }

    /// Leaves what the window shows for the Welcome screen: its project, or the file it edits on its
    /// own.
    fn close_to_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The close was decided: what is unsaved was saved or let go, so leaving saves nothing.
        self.leave_decided = true;
        if self.project_tree().is_some() {
            self.close_project(window, cx);
            self.leave_decided = false;
            return;
        }
        if !self.is_light_edit() {
            self.leave_decided = false;
            return;
        }
        self.leave(cx);
        if let Some(backend) = cx.try_global::<Backend>() {
            // The file's session stops in the background: this is the UI thread.
            backend.host.close_project_detached(&self.label);
        }
        self.content = Content::Empty;
        self.editor_observer = None;
        self.light_find_bar = None;
        self.sync_welcome(window, cx);
        cx.notify();
    }

    /// The dialog a close or open question shows over the window, while one is open.
    pub fn app_dialog(&self) -> Option<&Entity<PromptDialog>> {
        self.app_dialog.as_ref().map(|dialog| &dialog.view)
    }

    /// Shows `prompt` with its checkbox and a Cancel button; the receiver gets the answer. A dialog
    /// shown in its place answers this one with Cancel.
    fn show_app_dialog(
        &mut self,
        prompt: Prompt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<DialogAnswer> {
        let (sender, answer) = oneshot::channel();
        let mut buttons: Vec<DialogButton> = prompt
            .buttons
            .iter()
            .map(|button| DialogButton {
                id: button.id.to_owned(),
                label: button.label.clone(),
                primary: button.primary,
                danger: false,
            })
            .collect();
        if !buttons.iter().any(|button| button.id == CANCEL) {
            let label = cx.default_global::<Localization>().0.t("common.cancel", &[]);
            buttons.push(DialogButton { id: CANCEL.to_owned(), label, primary: false, danger: false });
        }
        let Prompt { title, body, checkbox, warning, .. } = prompt;
        let message = Message { title, body, warning, checkbox, buttons };
        let dialog = cx.new(|cx| PromptDialog::message(message, window, cx));
        let mut sender = Some(sender);
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &PromptDialogEvent, window, cx| {
            let PromptDialogEvent::Closed(closed) = event;
            workbench.close_app_dialog(window, cx);
            if let Some(sender) = sender.take() {
                // The decision may have ended meanwhile.
                let _ = sender.send(closed.clone());
            }
        });
        let focus = dialog.focus_handle(cx);
        // Another dialog in its place gives the keyboard back to where it was before the first.
        let restore = match self.app_dialog.take() {
            Some(previous) => previous.restore,
            None => window.focused(cx),
        };
        window.focus(&focus, cx);
        self.app_dialog = Some(AppDialog { view: dialog, restore, _events: events });
        cx.notify();
        answer
    }

    fn close_app_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.app_dialog.take() else { return };
        if let Some(target) = dialog.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    /// The close or open question over the window, while one is open.
    pub(super) fn app_dialog_layer(&self) -> Option<AnyElement> {
        let view = self.app_dialog.as_ref()?.view.clone();
        let layer = div()
            .id("app-dialog-layer")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .child(view);
        Some(gpui_kit::deferred(layer).with_priority(2).into_any_element())
    }
}

// -------------------------------------------------------------------------------------------------
// Questions

/// The system prompt's buttons for `prompt`: the default one first, then the others, then Cancel;
/// the ids answer the indexes it returns.
fn native_answers(prompt: &Prompt, cancel: String) -> (Vec<&'static str>, Vec<String>) {
    let (primary, others): (Vec<_>, Vec<_>) = prompt.buttons.iter().partition(|button| button.primary);
    let ordered = primary.into_iter().chain(others);
    let (mut ids, mut labels): (Vec<&'static str>, Vec<String>) =
        ordered.map(|button| (button.id, button.label.clone())).unzip();
    if !ids.contains(&CANCEL) {
        ids.push(CANCEL);
        labels.push(cancel);
    }
    (ids, labels)
}

/// Asks `prompt` in the workbench's window: with its checkbox in a dialog over the window, without
/// one as the system prompt. Dismissing either answers Cancel.
async fn ask(this: &This, cx: &mut AsyncWindowContext, prompt: Prompt) -> anyhow::Result<Answer> {
    if prompt.checkbox.is_none() {
        let cancel = cx.update(|_, cx| i18n(cx).t("common.cancel", &[]))?;
        let (ids, labels) = native_answers(&prompt, cancel);
        let level = if prompt.warning { PromptLevel::Warning } else { PromptLevel::Info };
        let shown = cx.update(|window, cx| {
            let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
            window.prompt(level, &prompt.title, Some(&prompt.body), &labels, cx)
        })?;
        let chosen = shown.await.ok().and_then(|index| ids.get(index).copied()).unwrap_or(CANCEL);
        return Ok(Answer::new(chosen, false));
    }
    let shown = this.update_in(cx, |workbench, window, cx| workbench.show_app_dialog(prompt, window, cx))?;
    let answer = shown.await.unwrap_or_else(|_| DialogAnswer { button: CANCEL.to_owned(), ..DialogAnswer::default() });
    Ok(Answer::new(&answer.button, answer.checked))
}

// -------------------------------------------------------------------------------------------------
// Closing

/// A close decision: settled already, or the task that settles it.
enum Settled {
    Now(bool),
    Later(Task<bool>),
}

/// The window is asked to close (its close button, Close Window, Quit): the close is decided as the
/// models say. `true`: it closes now. Otherwise it stays, and closes by itself once the decision
/// says so; the last window with a project returns to the Welcome screen instead.
pub(super) fn window_should_close(workbench: &WeakEntity<Workbench>, window: &mut Window, cx: &mut App) -> bool {
    let Some(workbench) = workbench.upgrade() else { return true };
    match request_close(&workbench, window, cx) {
        Settled::Now(close) => close,
        Settled::Later(decided) => {
            window
                .spawn(cx, async move |cx| {
                    if decided.await {
                        let _ = cx.update(|window, _| window.remove_window());
                    }
                })
                .detach();
            false
        }
    }
}

/// Routes a close request of the workbench's window: refused while another is decided, the last
/// project window to the Welcome screen, any other window to a close decision.
fn request_close(workbench: &Entity<Workbench>, window: &mut Window, cx: &mut App) -> Settled {
    let context = CloseContext { quitting: quitting(cx), windows: window_count(cx) };
    let state = workbench.read(cx).close_state(cx);
    let route = workbench.update(cx, |workbench, _| workbench.close_requests.request(context, true, state.clone()));
    match route {
        None => Settled::Now(false),
        Some(CloseRoute::Refused) => {
            workbench.update(cx, |workbench, _| workbench.close_requests.finished());
            Settled::Now(false)
        }
        Some(CloseRoute::ToWelcome) => {
            let input = leave_input(state.confirm_terminate, state.processes, state.dirty, state.project);
            let settled = decide_close(workbench, input, window, cx);
            after(workbench, settled, window, cx, |workbench, close, window, cx| {
                if close {
                    workbench.close_to_welcome(window, cx);
                }
            });
            Settled::Now(false)
        }
        Some(CloseRoute::Decide(input)) => decide_close(workbench, input, window, cx),
    }
}

/// Leaving the window's project (Close Project, another project opening here): the processes and
/// the unsaved documents as for any close, without the exit confirmation. Refused while another
/// close of the window is decided.
fn request_leave(workbench: &Entity<Workbench>, window: &mut Window, cx: &mut App) -> Settled {
    let state = workbench.read(cx).close_state(cx);
    // Two windows: leaving a project never asks to confirm the exit.
    let context = CloseContext { quitting: false, windows: 2 };
    let route = workbench.update(cx, |workbench, _| workbench.close_requests.request(context, true, state));
    match route {
        Some(CloseRoute::Decide(input)) => decide_close(workbench, input, window, cx),
        Some(_) => {
            workbench.update(cx, |workbench, _| workbench.close_requests.finished());
            Settled::Now(false)
        }
        None => Settled::Now(false),
    }
}

/// File › Close Project: once leaving it is settled, the window shows the Welcome screen.
pub(super) fn request_close_project(window: &mut Window, cx: &mut App) {
    let Some(workbench) = for_window(window, cx) else { return };
    let shows = {
        let workbench = workbench.read(cx);
        workbench.project_tree().is_some() || workbench.is_light_edit()
    };
    if !shows {
        return;
    }
    let settled = request_leave(&workbench, window, cx);
    after(&workbench, settled, window, cx, |workbench, close, window, cx| {
        if close {
            workbench.close_to_welcome(window, cx);
        }
    });
}

/// Calls `then` with the decision once it is settled: now, or when its task ends.
fn after(
    workbench: &Entity<Workbench>,
    settled: Settled,
    window: &mut Window,
    cx: &mut App,
    then: impl FnOnce(&mut Workbench, bool, &mut Window, &mut Context<Workbench>) + 'static,
) {
    match settled {
        Settled::Now(close) => workbench.update(cx, |workbench, cx| then(workbench, close, window, cx)),
        Settled::Later(decided) => {
            let this = workbench.downgrade();
            window
                .spawn(cx, async move |cx| {
                    let close = decided.await;
                    let _ = this.update_in(cx, |workbench, window, cx| then(workbench, close, window, cx));
                })
                .detach();
        }
    }
}

/// Decides `input` for the workbench's window, settling at once when nothing is asked or waited
/// for. Unsaved documents are asked about whatever Files › Autosave says. The window's close request
/// is finished either way.
fn decide_close(workbench: &Entity<Workbench>, input: CloseInput, window: &mut Window, cx: &mut App) -> Settled {
    let this = workbench.downgrade();
    let (mut decision, mut step) = CloseDecision::start(input, i18n(cx));
    loop {
        step = match step {
            // The editor has no documents of its own to prepare.
            CloseStep::PrepareClose => decision.prepared(None, i18n(cx)),
            CloseStep::Decided { close, failure, .. } => {
                workbench.update(cx, |workbench, cx| {
                    workbench.close_requests.finished();
                    if let Some(failure) = failure {
                        workbench.report_save_failure(failure, cx);
                    }
                });
                return Settled::Now(close);
            }
            waiting => {
                return Settled::Later(window.spawn(cx, async move |cx| settle(this, cx, decision, waiting).await));
            }
        };
    }
}

/// Carries a decision through to the end, then finishes the window's close request.
async fn settle(this: This, cx: &mut AsyncWindowContext, decision: CloseDecision, step: CloseStep) -> bool {
    let decided = decide(&this, cx, decision, step).await;
    let _ = this.update(cx, |workbench, _| workbench.close_requests.finished());
    decided.unwrap_or_else(|error| {
        tracing::warn!("the close of a window was not decided: {error:#}");
        false
    })
}

/// Carries out the steps of `decision` from `step` on; whether the window may close.
async fn decide(
    this: &This,
    cx: &mut AsyncWindowContext,
    mut decision: CloseDecision,
    mut step: CloseStep,
) -> anyhow::Result<bool> {
    // A failure to stop the processes is reported as such, not as one to save.
    let mut stop_failed = false;
    loop {
        step = match step {
            CloseStep::Ask(prompt) => {
                let answer = ask(this, cx, prompt).await?;
                cx.update(|_, cx| decision.answered(&answer, i18n(cx)))?
            }
            CloseStep::DisableConfirmExit => {
                let label = this.read_with(cx, |workbench, _| workbench.label.clone())?;
                cx.update(|_, cx| {
                    turn_off(&label, "general.confirmExit", cx);
                    decision.setting_written(i18n(cx))
                })?
            }
            CloseStep::DisableConfirmTerminate => {
                let label = this.read_with(cx, |workbench, _| workbench.label.clone())?;
                cx.update(|_, cx| {
                    turn_off(&label, "run.confirmTerminate", cx);
                    decision.setting_written(i18n(cx))
                })?
            }
            CloseStep::PrepareClose => cx.update(|_, cx| decision.prepared(None, i18n(cx)))?,
            CloseStep::SaveAll => {
                let saving = this.update(cx, |workbench, cx| workbench.save_all(cx))?;
                decision.saved(saving.await)
            }
            // Nothing was prepared: Don't Save leaves the documents to close with the window.
            CloseStep::DiscardPrepared => decision.discarded(Ok(())),
            CloseStep::Terminate { names } => {
                let result = terminate(this, cx, &names).await?;
                stop_failed = result.is_err();
                decision.terminated(result)
            }
            CloseStep::Decided { close, failure, .. } => {
                if let Some(failure) = failure.filter(|_| !stop_failed) {
                    this.update(cx, |workbench, cx| workbench.report_save_failure(failure, cx))?;
                }
                return Ok(close);
            }
        };
    }
}

/// Stops the window's running processes and waits until none is left: Stop first, Stop again (which
/// kills what is still shutting down) after [`TERMINATE_GRACE`]. A failure names the processes.
async fn terminate(this: &This, cx: &mut AsyncWindowContext, names: &[String]) -> anyhow::Result<Result<(), String>> {
    for _ in 0..2 {
        let debugger = this.update_in(cx, |workbench, window, cx| {
            let debugger = workbench.debug_driver().map(|driver| driver.read(cx).debugger().clone());
            let debugging = debugger.as_ref().is_some_and(|d| live_debug_sessions(&d.read(cx).state).next().is_some());
            if debugging {
                workbench.run_debug_action("debug.stop", None, window, cx);
            }
            debugger
        })?;
        let store = this.update(cx, |workbench, cx| {
            let driver = workbench.run_driver()?.clone();
            let live = live_sessions(driver.read(cx).store().read(cx));
            driver.update(cx, |driver, cx| live.iter().for_each(|session| driver.stop(session, cx)));
            Some(driver.read(cx).store().clone())
        })?;
        let runs_ended = match &store {
            Some(store) => stopped(store, cx).await?,
            None => true,
        };
        let debugging_ended = match &debugger {
            Some(debugger) => debugging_stopped(debugger, cx).await?,
            None => true,
        };
        if runs_ended && debugging_ended {
            return Ok(Ok(()));
        }
    }
    let message = names.join(", ");
    this.update(cx, |workbench, cx| {
        let title = cx.default_global::<Localization>().0.tx("Cannot stop process");
        let mut request = NotificationRequest::new(NotificationLevel::Error, title);
        request.body = Some(message.clone());
        workbench.notify(request, cx);
    })?;
    Ok(Err(message))
}

/// Whether every program of `debugger` ended within [`TERMINATE_GRACE`].
async fn debugging_stopped(debugger: &Entity<Debugger>, cx: &mut AsyncWindowContext) -> anyhow::Result<bool> {
    let (sender, receiver) = oneshot::channel::<()>();
    let mut sender = Some(sender);
    let _watch = cx.update(|_, cx| {
        cx.observe(debugger, move |debugger, cx| {
            if live_debug_sessions(&debugger.read(cx).state).next().is_none()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        })
    })?;
    if cx.update(|_, cx| live_debug_sessions(&debugger.read(cx).state).next().is_none())? {
        return Ok(true);
    }
    let timer = std::pin::pin!(cx.background_executor().timer(TERMINATE_GRACE));
    let ended = std::pin::pin!(receiver);
    Ok(matches!(select(ended, timer).await, Either::Left((Ok(()), _))))
}

/// Whether every process of `store` ended within [`TERMINATE_GRACE`].
async fn stopped(store: &Entity<RunStore>, cx: &mut AsyncWindowContext) -> anyhow::Result<bool> {
    let (sender, receiver) = oneshot::channel::<()>();
    let mut sender = Some(sender);
    let _watch = cx.update(|_, cx| {
        cx.observe(store, move |store, cx| {
            if live_sessions(store.read(cx)).is_empty()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(());
            }
        })
    })?;
    if cx.update(|_, cx| live_sessions(store.read(cx)).is_empty())? {
        return Ok(true);
    }
    let timer = std::pin::pin!(cx.background_executor().timer(TERMINATE_GRACE));
    let ended = std::pin::pin!(receiver);
    Ok(matches!(select(ended, timer).await, Either::Left((Ok(()), _))))
}

/// File › Exit: the exit is confirmed once (when `general.confirmExit` is on), then every window
/// decides its own close in turn, its processes and unsaved documents; the app quits once all of
/// them agreed. A window that keeps open calls the quit off.
pub(super) fn quit(window: &mut Window, cx: &mut App) {
    quit_then(window, cx, |cx| cx.quit());
}

/// [`quit`], ending with `end` once every window agreed to close: quitting, or quitting and
/// starting again (Restart IDE).
pub(super) fn quit_then(window: &mut Window, cx: &mut App, end: fn(&mut App)) {
    if quitting(cx) {
        return;
    }
    let exit = quit_input(confirm_exit_enabled(Some(&settings::global(cx))));
    let asking = window.window_handle();
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Some(input) = exit
            && !confirm_exit(asking, input, cx).await
        {
            return;
        }
        cx.update(|cx| cx.set_global(Quitting(true)));
        for (_, handle) in cx.update(|cx| windows::windows(cx)) {
            let settled = handle.update(cx, |_, window, cx| {
                for_window(window, cx).map(|workbench| request_close(&workbench, window, cx))
            });
            let closes = match settled {
                Ok(Some(Settled::Now(close))) => close,
                Ok(Some(Settled::Later(decided))) => decided.await,
                // A window that is gone or holds no workbench keeps nothing open.
                Ok(None) | Err(_) => true,
            };
            if !closes {
                cx.update(|cx| cx.set_global(Quitting(false)));
                return;
            }
        }
        cx.update(end);
    })
    .detach();
}

/// The quit's exit confirmation, in the window it was asked from; a window without a workbench quits
/// without one.
async fn confirm_exit(asking: AnyWindowHandle, input: CloseInput, cx: &mut AsyncApp) -> bool {
    let decided = asking.update(cx, |_, window, cx| {
        let this = for_window(window, cx)?.downgrade();
        let (decision, step) = CloseDecision::start(input, i18n(cx));
        Some(window.spawn(cx, async move |cx| {
            decide(&this, cx, decision, step).await.unwrap_or_else(|error| {
                tracing::warn!("the exit was not confirmed: {error:#}");
                false
            })
        }))
    });
    match decided {
        Ok(Some(decided)) => decided.await,
        Ok(None) | Err(_) => true,
    }
}

// -------------------------------------------------------------------------------------------------
// Opening projects

/// Opens the project at `path` where "Open project in" says (asking, with "Don't ask again"), then
/// `files` in its editor. A window without a workbench opens it in a new window.
pub(super) fn open_project_with_policy(
    path: &Path,
    files: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<()> {
    let Some(workbench) = for_window(window, cx) else { return windows::open_project(path, true, window, cx) };
    let path = canonical(path);
    let state = workbench.read(cx).window_state(cx);
    let (flow, step) = OpenFlow::with_policy(&path, &files, &state, i18n(cx));
    start_open(&workbench, flow, step, window, cx);
    Ok(())
}

/// Opens the project at `path` in a new window when `new_window` says so, else in this one (leaving
/// what it shows, once that is settled), whatever "Open project in" says.
pub(super) fn open_project_explicit(
    path: &Path,
    new_window: bool,
    window: &mut Window,
    cx: &mut App,
) -> anyhow::Result<()> {
    let Some(workbench) = for_window(window, cx) else { return windows::open_project(path, true, window, cx) };
    let state = workbench.read(cx).window_state(cx);
    let (flow, step) = OpenFlow::explicit(&canonical(path), new_window, &state);
    start_open(&workbench, flow, step, window, cx);
    Ok(())
}

fn start_open(workbench: &Entity<Workbench>, flow: OpenFlow, step: OpenStep, window: &mut Window, cx: &mut App) {
    let this = workbench.downgrade();
    window
        .spawn(cx, async move |cx| {
            if let Err(error) = carry_out_open(&this, cx, flow, step).await {
                tracing::debug!("opening a project stopped: {error:#}");
            }
        })
        .detach();
}

/// Carries out the steps of opening a project, from `step` on.
async fn carry_out_open(
    this: &This,
    cx: &mut AsyncWindowContext,
    mut flow: OpenFlow,
    mut step: OpenStep,
) -> anyhow::Result<()> {
    loop {
        step = match step {
            OpenStep::Ask(mut prompt) => {
                prompt.buttons.retain(|button| button.id != ATTACH);
                let answer = ask(this, cx, prompt).await?;
                flow.answered(&answer)
            }
            OpenStep::WriteOpenPolicy(mode) => {
                let label = this.read_with(cx, |workbench, _| workbench.label.clone())?;
                cx.update(|_, cx| {
                    let value = Value::from(mode.as_str());
                    let written = settings::set(&label, SettingsScope::Global, "general.openProjectIn", &value, cx);
                    if let Err(error) = written {
                        // The choice still applies to this open.
                        tracing::warn!("cannot remember where projects open: {}", error.message);
                    }
                })?;
                flow.policy_written()
            }
            OpenStep::RequestWindow { path, new_window, files } => {
                let outcome = cx.update(|window, cx| request_window(&path, new_window, files, window, cx))?;
                cx.update(|_, cx| flow.window_answered(outcome, i18n(cx)))?
            }
            OpenStep::ConfirmLeave => {
                let confirmed = confirm_leave(this, cx).await?;
                flow.left(confirmed)
            }
            // A folder opens as a local project: this window does not run projects elsewhere.
            OpenStep::ResolveWsl { .. } => flow.wsl_resolved(None),
            OpenStep::ConnectRemote { path, .. } => {
                let error = IdeError::new(ErrorKind::Unavailable, format!("{path} is on another machine"));
                cx.update(|_, cx| flow.project_opened(Err(error), i18n(cx)))?
            }
            OpenStep::OpenProject { path } => {
                // A folder nobody decided about is asked about first. Don't Open is a cancelled
                // opening: nothing is reported and the window keeps what it shows.
                let confirmed = super::app_actions::confirm_project_trust(this, cx, Path::new(&path)).await?;
                let opened = match confirmed {
                    Ok(true) => {
                        this.update_in(cx, |workbench, window, cx| {
                            // Leaving what the window showed was decided first: it saves nothing more.
                            workbench.leave_decided = true;
                            workbench.open_project(PathBuf::from(path), window, cx);
                            // An open that did not reach `leave` must not spare a later one its autosave.
                            workbench.leave_decided = false;
                        })?;
                        project_opened(this, cx).await?
                    }
                    Ok(false) => Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled")),
                    Err(error) => Err(error),
                };
                cx.update(|_, cx| flow.project_opened(opened, i18n(cx)))?
            }
            // A folder is attached only when it is trusted already: there is no trust question here.
            OpenStep::ConfirmTrust { .. } => cx.update(|_, cx| flow.trust_confirmed(Ok(()), i18n(cx)))?,
            OpenStep::ReadTrust { path } => cx.update(|_, cx| {
                let trusted = match cx.try_global::<Backend>() {
                    Some(backend) => backend.host.trust.trusted(Path::new(&path)),
                    None => Err(IdeError::internal("the backend is not running")),
                };
                flow.trust_read(trusted, &mut cx.default_global::<Localization>().0)
            })?,
            OpenStep::Attach { path } => {
                let label = this.read_with(cx, |workbench, _| workbench.label.clone())?;
                cx.update(|_, cx| {
                    let attached = match cx.try_global::<Backend>() {
                        Some(backend) => {
                            let path = Path::new(&path);
                            ide_host::ops::project::attach_workspace(&backend.host, &label, path).map(|_| ())
                        }
                        None => Err(IdeError::internal("the backend is not running")),
                    };
                    flow.attached(attached, i18n(cx))
                })?
            }
            OpenStep::ReadCurrentProject => flow.project_read(),
            OpenStep::RevealProject { path } => {
                this.update(cx, |workbench, cx| {
                    if let Some(tree) = workbench.project_tree().cloned() {
                        tree.update(cx, |tree, cx| tree.reveal_path(&path, cx));
                    }
                })?;
                flow.revealed()
            }
            OpenStep::OpenFiles(files) => {
                this.update_in(cx, |workbench, window, cx| workbench.open_tabs(files, window, cx))?;
                flow.files_opened()
            }
            // Nothing was prepared for a close, so there is no close decision to release.
            OpenStep::Failed { notice, .. } => {
                if let Some(notice) = notice {
                    this.update(cx, |workbench, cx| workbench.notify(notice, cx))?;
                }
                flow.reported()
            }
            OpenStep::Done => return Ok(()),
        };
    }
}

/// Leaving what the window shows, decided as a close of its project; whether it was confirmed.
async fn confirm_leave(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<bool> {
    let workbench = this.upgrade().ok_or_else(|| anyhow::anyhow!("the window is gone"))?;
    let settled = cx.update(|window, cx| request_leave(&workbench, window, cx))?;
    Ok(match settled {
        Settled::Now(confirmed) => confirmed,
        Settled::Later(decided) => decided.await,
    })
}

/// Where the project at `path` opens: the window that has it comes to the front (and opens `files`),
/// a new window opens it when `new_window` asks for one, else this window does.
fn request_window(
    path: &str,
    new_window: bool,
    files: Vec<String>,
    window: &mut Window,
    cx: &mut App,
) -> Result<OpenOutcome, IdeError> {
    let root = PathBuf::from(path);
    let here = window.window_handle();
    if let Some(open) = window_with_project(&root, cx) {
        if open.window_id() == here.window_id() {
            return Ok(OpenOutcome::Current);
        }
        let label = label_of(open, cx);
        let opens = editor_opens(&files, &[]);
        // The asking window is the one being updated: the other is free.
        let _ = open.update(cx, |_, window, cx| {
            window.activate_window();
            if let Some(workbench) = for_window(window, cx) {
                workbench.update(cx, |workbench, cx| workbench.open_tabs(opens, window, cx));
            }
        });
        return Ok(OpenOutcome::Focused { label });
    }
    if !new_window {
        return Ok(OpenOutcome::Current);
    }
    let handle =
        windows::open_window(Target::Project(root), cx).map_err(|error| IdeError::internal(format!("{error:#}")))?;
    if !files.is_empty() {
        open_once_opened(handle, editor_opens(&files, &[]), cx);
    }
    Ok(OpenOutcome::Opened { label: label_of(handle, cx) })
}

/// Opens `files` in the window `handle` once its project has opened.
fn open_once_opened(handle: AnyWindowHandle, files: Vec<FileOpen>, cx: &mut App) {
    let _ = handle.update(cx, |_, window, cx| {
        let Some(workbench) = for_window(window, cx) else { return };
        workbench.update(cx, |_, cx| {
            cx.spawn_in(window, async move |this, cx| {
                if let Ok(Ok(())) = project_opened(&this, cx).await {
                    let _ = this.update_in(cx, |workbench, window, cx| workbench.open_tabs(files, window, cx));
                }
            })
            .detach();
        });
    });
}

/// Waits until the project the window is opening has opened, or failed to.
async fn project_opened(this: &This, cx: &mut AsyncWindowContext) -> anyhow::Result<Result<(), IdeError>> {
    let (sender, receiver) = oneshot::channel();
    let mut sender = Some(sender);
    let _watch = this.update(cx, |workbench, cx| match workbench.open_result() {
        Some(result) => {
            if let Some(sender) = sender.take() {
                let _ = sender.send(result);
            }
            None
        }
        None => Some(cx.observe_self(move |workbench, _| {
            if let Some(result) = workbench.open_result()
                && let Some(sender) = sender.take()
            {
                let _ = sender.send(result);
            }
        })),
    })?;
    Ok(receiver.await.unwrap_or_else(|_| Err(IdeError::new(ErrorKind::Cancelled, "Project opening cancelled"))))
}

// -------------------------------------------------------------------------------------------------
// Opening loose files

/// Opens loose `files` (File › Open File…): in this window's project as tabs; otherwise a file of a
/// project open in another window opens there, and the rest open for editing on their own. A
/// window edits one file on its own: the first takes this window when it shows nothing, every other
/// file gets a window of its own.
pub(super) fn open_files(files: Vec<PathBuf>, window: &mut Window, cx: &mut App) -> anyhow::Result<()> {
    let Some(workbench) = for_window(window, cx) else { return Ok(()) };
    let files: Vec<String> = files.iter().map(|file| canonical(file)).collect();
    let state = workbench.read(cx).window_state(cx);
    let (mut flow, mut step) = LooseFiles::open(&files, &state);
    loop {
        step = match step {
            LooseStep::Route(files) => {
                let first = files.first().cloned().unwrap_or_default();
                let here = route_files(files, window, cx);
                flow.routed(Ok(here), &first, i18n(cx))
            }
            LooseStep::OpenLightEdit { path } => {
                workbench.update(cx, |workbench, cx| workbench.open_file(PathBuf::from(path), window, cx));
                // A file that cannot be opened says so in the window.
                flow.light_edit_opened(Ok(()), i18n(cx))
            }
            LooseStep::OpenFiles(opens) => {
                if workbench.read(cx).project_tree().is_some() {
                    workbench.update(cx, |workbench, cx| workbench.open_tabs(opens, window, cx));
                    return Ok(());
                }
                let shown = workbench.read(cx).light_edit_file(cx);
                for open in opens.into_iter().filter(|open| shown.as_deref() != Some(Path::new(&open.path))) {
                    windows::open_window(Target::File(PathBuf::from(open.path)), cx)?;
                }
                return Ok(());
            }
            LooseStep::Notify(notice) => {
                workbench.update(cx, |workbench, cx| workbench.notify(notice, cx));
                return Ok(());
            }
            LooseStep::Done => return Ok(()),
        };
    }
}

/// Hands each file that another window shows (in its project, or on its own) to that window and
/// brings it to the front; the files left to this window.
fn route_files(files: Vec<String>, window: &mut Window, cx: &mut App) -> Vec<String> {
    let here = window.window_handle();
    let mut left = Vec::new();
    for file in files {
        let path = PathBuf::from(&file);
        let Some(other) = window_showing(&path, here, cx) else {
            left.push(file);
            continue;
        };
        // The asking window is the one being updated: the other is free.
        let _ = other.update(cx, |_, window, cx| {
            window.activate_window();
            if let Some(workbench) = for_window(window, cx) {
                workbench.update(cx, |workbench, cx| {
                    if workbench.project_tree().is_some() {
                        workbench.open_project_file(path, window, cx);
                    }
                });
            }
        });
    }
    left
}
