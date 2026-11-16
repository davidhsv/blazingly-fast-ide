//! Notebook tabs: a `.ipynb` file opens as a notebook editor instead of a text editor, and the
//! notebook's backend calls are made here.
//!
//! Opening a notebook asks the backend to open it first (`notebook_open`), so the tab is keyed by
//! the canonical path every alias of the file resolves to; a file that cannot be opened still gets
//! its tab, which shows why. Every tab of one file in a window shares one [`NotebookModel`]; when
//! the last of them goes, the backend document (and its kernel) is closed (`notebook_close`). A
//! tab holding unsaved changes asks Save / Don't Save / Cancel before it goes, after freezing the
//! notebook and stopping a running batch; Save All saves the window's notebooks too; Edit › Undo
//! and Redo in a notebook go to the notebook's history.
//!
//! Each call of [`ide_host::ops::notebook`] is made with the arguments the desktop adapters pass;
//! `notebook_execute` streams the kernel's output events back to the window while the batch runs,
//! and the backend's `NotebookKernel` events update the kernel state of the notebook they name.

use super::tab_views::TabView;
use super::{Workbench, for_window};
use crate::app_models::edit_command::EditCommand;
use crate::backend::Backend;
use crate::editor::tabs::EditorInput;
use crate::features_a::notebook::session::{Answer, Call, Failure, Request};
use crate::features_b::viewers::render::{LinkTarget, resolve_link};
use crate::notebook::model::{Environment, NotebookModel, NotebookModelEvent, NotebookOp};
use crate::notebook::view::{NotebookView, NotebookViewEvent};
use crate::notebook::{NOTEBOOK_KIND, notebook_input};
use crate::platform::actions::{Action, ActionRegistry};
use crate::platform::i18n::Localization;
use crate::settings_view::pages::languages_sdk::{SdkDetection, ToolchainKind};
use futures::StreamExt;
use gpui_kit::{
    App, AppContext, AsyncApp, Context, Entity, Focusable, Global, PromptLevel, Subscription, WeakEntity, Window,
    WindowId,
};
use ide_core::{IdeEvent, IdeResult};
use ide_host::ops::Channel;
use ide_host::ops::notebook as ops;
use ide_notebook::NotebookSnapshot;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

/// A notebook of a window: the window and the notebook's canonical path.
type NotebookKey = (WindowId, String);

/// A notebook open in a window: its model, the window's label (what the backend calls take),
/// how many tabs show it, and what feeds it.
struct OpenNotebook {
    model: Entity<NotebookModel>,
    /// Empty until the window's workbench is known; no call is made before.
    label: String,
    tabs: usize,
    /// Set once the last tab went and the backend document is being closed.
    closing: bool,
    listener: u64,
    _calls: Subscription,
}

/// The notebooks open in each window.
#[derive(Default)]
struct OpenNotebooks(HashMap<NotebookKey, OpenNotebook>);

impl Global for OpenNotebooks {}

fn label_of(key: &NotebookKey, cx: &App) -> Option<String> {
    let open = cx.try_global::<OpenNotebooks>()?.0.get(key)?;
    (!open.label.is_empty()).then(|| open.label.clone())
}

/// The model of the notebook `key`, made on first use; `label` names the window for the backend
/// once it is known.
fn notebook_model(key: &NotebookKey, label: Option<&str>, cx: &mut App) -> Entity<NotebookModel> {
    if let Some(open) = cx.default_global::<OpenNotebooks>().0.get_mut(key) {
        if let Some(label) = label {
            label.clone_into(&mut open.label);
        }
        return open.model.clone();
    }
    let model = cx.new(|cx| NotebookModel::new(&key.1, cx));
    let calls_key = key.clone();
    let calls = cx.subscribe(&model, move |model, event: &NotebookModelEvent, cx| match event {
        NotebookModelEvent::Send(requests) => {
            let label = label_of(&calls_key, cx);
            for request in requests.clone() {
                match &label {
                    Some(label) => make_call(label, model.downgrade(), request, cx),
                    None => model.update(cx, |model, cx| {
                        let failure = model.transport_failure("no project is open in this window");
                        model.answer(request.ticket, Err(failure), cx);
                    }),
                }
            }
        }
    });
    let weak = model.downgrade();
    // The session takes only the events of its own document incarnation.
    let listener = cx.global_mut::<Backend>().listen(move |event, cx| {
        if let IdeEvent::NotebookKernel { id, path, state, message } = &event.event {
            let path = path.to_string_lossy().into_owned();
            let _ = weak.update(cx, |model, cx| model.kernel_event(id, &path, state, message.as_deref(), cx));
        }
    });
    let label = label.unwrap_or_default().to_owned();
    let open = OpenNotebook { model: model.clone(), label, tabs: 0, closing: false, listener, _calls: calls };
    cx.default_global::<OpenNotebooks>().0.insert(key.clone(), open);
    model
}

/// A tab of the notebook `key` was shown.
fn tab_opened(key: &NotebookKey, cx: &mut App) {
    if let Some(open) = cx.default_global::<OpenNotebooks>().0.get_mut(key) {
        open.tabs += 1;
        open.closing = false;
    }
}

/// A tab of the notebook went; with the last one the backend document is closed and the model
/// forgotten.
fn tab_closed(key: NotebookKey, cx: &mut App) {
    let Some(open) = cx.default_global::<OpenNotebooks>().0.get_mut(&key) else { return };
    open.tabs = open.tabs.saturating_sub(1);
    if open.tabs > 0 || open.closing {
        return;
    }
    open.closing = true;
    let model = open.model.clone();
    let closed = model.update(cx, |model, cx| model.wait(NotebookOp::Close, cx));
    cx.spawn(async move |cx: &mut AsyncApp| {
        if let Ok(Err(failure)) = closed.await {
            tracing::warn!("the notebook was not closed: {}", failure.message);
        }
        cx.update(|cx| {
            let notebooks = &mut cx.default_global::<OpenNotebooks>().0;
            // A tab of the file shown again while it closed keeps its model.
            let done = notebooks.get(&key).is_some_and(|open| open.closing && open.model == model);
            if done && let Some(open) = notebooks.remove(&key) {
                cx.global_mut::<Backend>().unlisten(open.listener);
            }
        });
    })
    .detach();
}

/// A backend answer as the session takes it.
fn answer_of<T>(
    model: &WeakEntity<NotebookModel>,
    result: anyhow::Result<IdeResult<T>>,
    into: impl FnOnce(T) -> Answer,
    cx: &mut App,
) -> Option<Result<Answer, Failure>> {
    let model = model.upgrade()?;
    Some(match result {
        Ok(Ok(value)) => Ok(into(value)),
        Ok(Err(error)) => Err(model.update(cx, |model, _| model.failure(&error))),
        Err(error) => Err(model.update(cx, |model, _| model.transport_failure(&error.to_string()))),
    })
}

fn snapshot(snapshot: NotebookSnapshot) -> Answer {
    Answer::Snapshot(Box::new(snapshot))
}

fn done((): ()) -> Answer {
    Answer::Done
}

/// Runs `work` on the backend and answers the call `ticket` of `model` with its result.
fn answer_with<T, F>(model: WeakEntity<NotebookModel>, ticket: u64, work: F, into: fn(T) -> Answer, cx: &mut App)
where
    T: Send + 'static,
    F: Future<Output = anyhow::Result<IdeResult<T>>> + Send + 'static,
{
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = work.await;
        cx.update(|cx| {
            let Some(answer) = answer_of(&model, result, into, cx) else { return };
            if let Some(model) = model.upgrade() {
                model.update(cx, |model, cx| model.answer(ticket, answer, cx));
            }
        });
    })
    .detach();
}

/// Makes the backend call `request` of the notebook `model` in window `label`.
fn make_call(label: &str, model: WeakEntity<NotebookModel>, request: Request, cx: &mut App) {
    let Some(backend) = cx.try_global::<Backend>() else { return };
    let label = label.to_owned();
    let ticket = request.ticket;
    match request.call {
        Call::Open { path } => {
            let work = backend.with_host(move |host| async move { ops::open(&host, &label, path).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Get { id } => {
            let work = backend.with_host(move |host| async move { ops::get(&host, &label, id) });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Edit { id, version, edit } => {
            let work = backend.with_host(move |host| async move { ops::edit(&host, &label, id, version, edit).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Undo { id } => {
            let work = backend.with_host(move |host| async move { ops::undo(&host, &label, id).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Redo { id } => {
            let work = backend.with_host(move |host| async move { ops::redo(&host, &label, id).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Save { id } => {
            let work = backend.with_host(move |host| async move { ops::save(&host, &label, id).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Reload { id } => {
            let work = backend.with_host(move |host| async move { ops::reload(&host, &label, id).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Restart { id } => {
            let work = backend.with_host(move |host| async move { ops::restart(&host, &label, id).await });
            answer_with(model, ticket, work, snapshot, cx);
        }
        Call::Close { id } => {
            let work = backend.with_host(move |host| async move { ops::close(&host, &label, id).await });
            answer_with(model, ticket, work, done, cx);
        }
        Call::Interrupt { id } => {
            let work = backend.with_host(move |host| async move { ops::interrupt(&host, &label, id).await });
            answer_with(model, ticket, work, done, cx);
        }
        Call::Cancel { request } => {
            backend.host.cancel_request(request);
            if let Some(model) = model.upgrade() {
                model.update(cx, |model, cx| model.answer(ticket, Ok(Answer::Done), cx));
            }
        }
        Call::Execute { id, cells, python, request } => execute(label, model, ticket, id, cells, python, request, cx),
        Call::Environments => environments(label, model, ticket, cx),
    }
}

/// Runs a batch; its output events reach the model as they come, in the order they were sent,
/// those that arrived together applied at once.
#[allow(clippy::too_many_arguments)]
fn execute(
    label: String,
    model: WeakEntity<NotebookModel>,
    ticket: u64,
    id: String,
    cells: Vec<String>,
    python: Option<String>,
    request: ide_core::RequestId,
    cx: &mut App,
) {
    let (sender, mut receiver) = futures::channel::mpsc::unbounded();
    let output = Channel::new(move |event| {
        // A closed receiver means the window is gone; the batch's state stays in the backend.
        let _ = sender.unbounded_send(event);
    });
    let events = model.clone();
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Some(first) = receiver.next().await {
            let mut batch = vec![first];
            while let Ok(more) = receiver.try_recv() {
                batch.push(more);
            }
            let applied = cx.update(|cx| {
                let model = events.upgrade()?;
                model.update(cx, |model, cx| model.output_events(&batch, cx));
                Some(())
            });
            if applied.is_none() {
                break;
            }
        }
    })
    .detach();
    let Some(backend) = cx.try_global::<Backend>() else { return };
    let work = backend
        .with_host(move |host| async move { ops::execute(&host, &label, id, cells, python, request, output).await });
    answer_with(model, ticket, work, snapshot, cx);
}

/// The interpreters a notebook can run with and the project's own. The backend's detection runs
/// the interpreters it finds, so a test answers in its place with an [`SdkDetection`], and a test
/// build without one finds none and runs nothing.
fn environments(label: String, model: WeakEntity<NotebookModel>, ticket: u64, cx: &mut App) {
    if let Some(test) = cx.try_global::<SdkDetection>().cloned() {
        let found: Vec<Environment> = test
            .found
            .iter()
            .filter(|(kind, _)| *kind == ToolchainKind::Python)
            .flat_map(|(_, toolchains)| toolchains.iter())
            .map(|toolchain| Environment { name: toolchain.name.clone(), path: toolchain.path.clone() })
            .collect();
        test.asked.borrow_mut().push(ToolchainKind::Python);
        if let Some(model) = model.upgrade() {
            model.update(cx, |model, cx| {
                model.set_environments(found);
                model.answer(ticket, Ok(Answer::Python(test.python.clone())), cx);
            });
        }
        return;
    }
    if cfg!(test) {
        if let Some(model) = model.upgrade() {
            model.update(cx, |model, cx| {
                model.set_environments(Vec::new());
                model.answer(ticket, Ok(Answer::Python(None)), cx);
            });
        }
        return;
    }
    let Some(backend) = cx.try_global::<Backend>() else { return };
    let work = backend.with_host(move |host| async move {
        let kind = serde_json::from_value(serde_json::Value::from(ToolchainKind::Python.wire()))
            .map_err(|error| ide_core::IdeError::internal(error.to_string()))?;
        let found = ide_host::ops::sdk::sdk_detect(&host, &label, kind).await?;
        let effective = ide_host::ops::sdk::sdk_python_effective(&host, &label, None)?;
        let found: Vec<Environment> = found
            .into_iter()
            .map(|sdk| Environment { name: sdk.name, path: sdk.path.to_string_lossy().into_owned() })
            .collect();
        IdeResult::Ok((found, effective.map(|path| path.to_string_lossy().into_owned())))
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = work.await;
        cx.update(|cx| {
            let Some(target) = model.upgrade() else { return };
            let answer = match result {
                Ok(Ok((found, effective))) => {
                    target.update(cx, |model, _| model.set_environments(found));
                    Ok(Answer::Python(effective))
                }
                Ok(Err(error)) => Err(target.update(cx, |model, _| model.failure(&error))),
                Err(error) => Err(target.update(cx, |model, _| model.transport_failure(&error.to_string()))),
            };
            target.update(cx, |model, cx| model.answer(ticket, answer, cx));
        });
    })
    .detach();
}

/// Builds the view of a notebook tab.
pub(super) fn build_notebook_tab(
    input: &EditorInput,
    window: &mut Window,
    cx: &mut Context<Workbench>,
) -> Option<TabView> {
    if input.kind != NOTEBOOK_KIND {
        return None;
    }
    let path = input.path.clone()?;
    let key: NotebookKey = (window.window_handle().window_id(), path);
    // The workbench is being updated while it builds the tab: its label is read once it is not.
    let model = notebook_model(&key, None, cx);
    tab_opened(&key, cx);
    let view = cx.new(|cx| NotebookView::new(model.clone(), window, cx));
    let events = cx.subscribe_in(&view, window, |workbench, view, event: &NotebookViewEvent, window, cx| {
        workbench.notebook_view_event(view, event, window, cx);
    });
    let closed = key.clone();
    cx.observe_release(&view, move |_, _, cx| tab_closed(closed, cx)).detach();
    let loaded = model.clone();
    cx.defer_in(window, move |workbench, _, cx| {
        if let Some(open) = cx.default_global::<OpenNotebooks>().0.get_mut(&key) {
            workbench.label.clone_into(&mut open.label);
            loaded.update(cx, |model, cx| model.ensure_loaded(cx));
        }
    });
    let mut tab = TabView::new(NOTEBOOK_KIND, input.clone(), &view, cx).with_subscription(events);
    let dirty = model.clone();
    tab.dirty = Some(Rc::new(move |cx: &App| dirty.read(cx).is_dirty()));
    tab.close_guard = Some(Rc::new(move |window: &mut Window, cx: &mut App| close_decision(&model, window, cx)));
    Some(tab)
}

/// Before an unsaved notebook's last tab goes: the notebook stops taking edits and runs, commits
/// its pending edits and stops its batch, then Save, Don't Save or Cancel decides. A failed save
/// keeps the tab, as Cancel does; both let edits and runs resume.
fn close_decision(model: &Entity<NotebookModel>, window: &mut Window, cx: &mut App) -> gpui_kit::Task<bool> {
    let prepared = model.update(cx, |model, cx| model.wait(NotebookOp::PrepareClose, cx));
    let path = model.read(cx).path().to_owned();
    let (title, body, answers) = {
        let i18n = &cx.default_global::<Localization>().0;
        (
            i18n.t("app.unsaved.title", &[]),
            i18n.t("app.unsaved.body", &[("count", 1usize.into()), ("project", path.into())]),
            [i18n.t("app.unsaved.save", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])],
        )
    };
    let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
    let answer = window.prompt(PromptLevel::Warning, &title, Some(&body), &answers, cx);
    let model = model.clone();
    window.spawn(cx, async move |cx| {
        if let Ok(Err(failure)) = prepared.await {
            model.update(cx, |model, cx| model.report(&failure, cx));
        }
        let outcome = match answer.await {
            Ok(0) => Some(model.update(cx, |model, cx| model.wait(NotebookOp::Save, cx))),
            Ok(1) => Some(model.update(cx, |model, cx| model.wait(NotebookOp::Discard, cx))),
            _ => None,
        };
        let closes = match outcome {
            Some(outcome) => matches!(outcome.await, Ok(Ok(()))),
            None => false,
        };
        if !closes {
            model.update(cx, |model, cx| model.cancel_close(cx));
        }
        closes
    })
}

/// Edit › Undo and Redo while a notebook tab has the keyboard: the notebook's own history takes
/// them. `false` when no notebook has the keyboard.
pub(super) fn document_history(command: EditCommand, window: &mut Window, cx: &mut App) -> bool {
    let op = match command {
        EditCommand::Undo => NotebookOp::Undo,
        EditCommand::Redo => NotebookOp::Redo,
        _ => return false,
    };
    let Some(workbench) = for_window(window, cx) else { return false };
    let Some(view) = workbench.read(cx).active_tab_view().and_then(TabView::downcast::<NotebookView>) else {
        return false;
    };
    if !view.read(cx).focus_handle(cx).contains_focused(window, cx) {
        return false;
    }
    let model = view.read(cx).model().clone();
    model.update(cx, |model, cx| model.invoke(op, cx));
    true
}

/// Puts Save All (`file.save`) in front of the editors' handler: the window's unsaved notebooks are
/// saved too, a failure showing in the notebook's error bar; with a notebook tab in front and no text
/// editor, the notebooks alone are saved.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    let Some(previous) = registry.get("file.save") else { return };
    let meta = previous.meta.clone();
    registry.register(Action::new(meta, move |call, window, cx| {
        let Some(workbench) = for_window(window, cx) else { return previous.run_handler(call, window, cx) };
        let (label, notebook_in_front) = {
            let workbench = workbench.read(cx);
            (workbench.label.clone(), workbench.notebook_view().is_some())
        };
        let models: Vec<Entity<NotebookModel>> = cx
            .default_global::<OpenNotebooks>()
            .0
            .values()
            .filter(|open| open.label == label && !open.closing)
            .map(|open| open.model.clone())
            .collect();
        for model in models {
            model.update(cx, |model, cx| {
                if model.is_dirty() {
                    model.invoke(NotebookOp::Save, cx);
                }
            });
        }
        if notebook_in_front { Ok(()) } else { previous.run_handler(call, window, cx) }
    }));
}

/// Saves every unsaved notebook of the window `label`: what the operations that save first wait
/// for besides the text editors.
pub(super) fn save_notebooks(label: &str, cx: &mut App) -> Vec<crate::notebook::model::Outcome> {
    let models: Vec<Entity<NotebookModel>> = cx
        .default_global::<OpenNotebooks>()
        .0
        .values()
        .filter(|open| open.label == label && !open.closing)
        .map(|open| open.model.clone())
        .collect();
    let dirty: Vec<Entity<NotebookModel>> = models.into_iter().filter(|model| model.read(cx).is_dirty()).collect();
    dirty.into_iter().map(|model| model.update(cx, |model, cx| model.wait(NotebookOp::Save, cx))).collect()
}

impl Workbench {
    /// The notebook tab being shown, when the active tab is one.
    pub fn notebook_view(&self) -> Option<Entity<NotebookView>> {
        self.active_tab_view().and_then(TabView::downcast::<NotebookView>)
    }

    /// Opens the notebook `path` in a tab of the active pane, or brings its tab to the front. The
    /// backend opens it first: the tab is the canonical file's, its document already loaded.
    pub(super) fn open_notebook(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.label.clone();
        let file = path.to_string_lossy().into_owned();
        let opened = cx.global::<Backend>().with_host(move |host| async move { ops::open(&host, &label, file).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let snapshot = match result {
                    Ok(Ok(snapshot)) => Some(snapshot),
                    // The tab still opens: it shows why the notebook cannot be edited, never the
                    // file as raw JSON text.
                    Ok(Err(error)) => {
                        tracing::warn!("the notebook did not open: {}", error.message);
                        None
                    }
                    Err(error) => {
                        tracing::warn!("the notebook did not open: {error}");
                        None
                    }
                };
                let canonical = snapshot
                    .as_ref()
                    .map_or_else(|| path.to_string_lossy().into_owned(), |s| s.path.to_string_lossy().into_owned());
                if let Some(snapshot) = snapshot {
                    let key: NotebookKey = (window.window_handle().window_id(), canonical.clone());
                    let model = notebook_model(&key, Some(&workbench.label), cx);
                    model.update(cx, |model, cx| {
                        if model.state().snapshot.is_none() {
                            model.load_opened(snapshot, cx);
                        }
                    });
                }
                workbench.open_view_tab(notebook_input(&canonical), window, cx);
            });
        })
        .detach();
    }

    fn notebook_view_event(
        &mut self,
        view: &Entity<NotebookView>,
        event: &NotebookViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            NotebookViewEvent::ShowMenu { items, position } => {
                let view = view.downgrade();
                self.show_menu(
                    items,
                    *position,
                    move |_, choice, _, cx| {
                        let _ = view.update(cx, |view, cx| view.choose(choice, cx));
                    },
                    window,
                    cx,
                );
            }
            NotebookViewEvent::OpenLink { notebook, href } => match resolve_link(notebook, href) {
                Some(LinkTarget::External { url }) => cx.open_url(&url),
                Some(LinkTarget::File { path, line: Some(line), .. }) => {
                    self.open_project_file_at(PathBuf::from(path), line, 0, window, cx);
                }
                Some(LinkTarget::File { path, line: None, .. }) => {
                    self.open_project_file(PathBuf::from(path), window, cx)
                }
                // A link to a heading of the notebook, or one that names nothing, is inert.
                Some(LinkTarget::Anchor { .. }) | None => {}
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notebook_kind_is_restorable_and_titled_by_its_file() {
        let kind = super::super::tab_views::kind(NOTEBOOK_KIND).expect("the notebook kind is registered");
        assert!(kind.restorable);
        assert_eq!((kind.title)(&notebook_input("/p/a.ipynb")), "a.ipynb");
        assert_eq!((kind.icon)(&notebook_input("/p/a.ipynb")), "file");
    }
}
