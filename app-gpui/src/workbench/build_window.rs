//! Build › Build Project, Build Module, Rebuild Project and Build Artifacts, and the Build tool
//! window: the model of the current build ([`BuildRun`]), the backend's `build_run` that runs it with
//! its streamed events fed back, and the effects the model returns carried out in the window.

use super::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::tasks::{Cancel, Change, TaskId, TaskUpdate};
use super::{Panel, Workbench};
use crate::backend::Backend;
use crate::build_window::model::{
    BUILD_LAST_STATUS, BUILD_RUNNING, BUILD_TASK_ID, BUILD_TOOL_WINDOW, BuildEffect, BuildRun, NotifyLevel,
};
use crate::build_window::view::{BuildView, BuildViewEvent};
use crate::platform::actions::{ActionCall, ActionRegistry};
use crate::platform::context::{ContextValue, clear_context, set_context};
use crate::platform::local_time::now_ms;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{AppContext, Context, Entity, Focusable, Task, Window};
use ide_build::compile::{BuildEvent, BuildKind};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use std::path::PathBuf;

/// The current build of the window's project, and the work that runs it.
pub(super) struct BuildSlot {
    run: Entity<BuildRun>,
    /// The running build: saving, the backend's `build_run` and its events.
    running: Option<Task<()>>,
}

/// The `activate` flag of an action's argument: the tool window shows unless it is `false`.
fn activates(arg: Option<&serde_json::Value>) -> bool {
    arg.and_then(|arg| arg.get("activate")).and_then(serde_json::Value::as_bool) != Some(false)
}

/// The CMake target the project builds with: the `run.cmakeBuild` context key.
fn cmake_of_context(call: &ActionCall) -> Option<String> {
    match call.ctx.get("run.cmakeBuild") {
        Some(ContextValue::Text(target)) if !target.is_empty() => Some(target.to_string()),
        _ => None,
    }
}

/// Registers the Build actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("build.buildProject", |call, window, cx| {
        let activate = activates(call.arg);
        // Without an argument the context's CMake target builds; an argument names its own.
        let cmake = match call.arg {
            None => cmake_of_context(call),
            Some(arg) => arg.get("cmake").and_then(serde_json::Value::as_str).map(str::to_owned),
        };
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.start_build(BuildKind::Build, None, activate, cmake, window, cx);
        })
    });
    registry.handle("build.buildModule", |call, window, cx| {
        let activate = activates(call.arg);
        let named = call.arg.and_then(|arg| {
            arg.as_str().or_else(|| arg.get("target").and_then(serde_json::Value::as_str)).map(str::to_owned)
        });
        on_workbench(window, cx, |workbench, window, cx| {
            let target = named
                .filter(|target| !target.is_empty())
                .or_else(|| workbench.editor().map(|editor| editor.read(cx).path().to_string_lossy().into_owned()));
            workbench.start_build(BuildKind::Build, target, activate, None, window, cx);
        })
    });
    registry.handle("build.rebuildProject", |call, window, cx| {
        let cmake = cmake_of_context(call);
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.start_build(BuildKind::Rebuild, None, true, cmake, window, cx);
        })
    });
    registry.handle("build.buildArtifacts", |call, window, cx| {
        let activate = activates(call.arg);
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.start_build(BuildKind::Artifacts, None, activate, None, window, cx);
        })
    });
    registry.handle("build.stop", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.stop_build(window, cx))
    });
}

impl Workbench {
    /// The build of the open project.
    pub(super) fn build_run(&mut self, cx: &mut Context<Self>) -> Option<Entity<BuildRun>> {
        self.project_root(cx)?;
        let slot = self.build.get_or_insert_with(|| BuildSlot { run: cx.new(|_| BuildRun::default()), running: None });
        Some(slot.run.clone())
    }

    /// The project goes: its build stops and is forgotten.
    pub(super) fn reset_build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(slot) = self.build.take() else { return };
        let effects = slot.run.update(cx, |run, cx| {
            cx.notify();
            run.reset()
        });
        // The dropped build never reports its end: its status bar task goes with it.
        self.tasks.remove_task(&TaskId::Name(BUILD_TASK_ID.to_owned()));
        self.carry_out_build(effects, window, cx);
    }

    /// The tool window's panel, over the project's build.
    pub(super) fn mount_build(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        let run = self.build_run(cx)?;
        let root = self.project_root(cx).map(|root| root.to_string_lossy().into_owned());
        let view = cx.new(|cx| {
            let mut view = BuildView::new(run, cx);
            view.set_project_root(root, cx);
            view
        });
        let events = cx.subscribe_in(&view, window, Self::build_view_event);
        Some(Panel::new(view, Some(events)))
    }

    /// Starts a build of `kind`, of the module of `file` or of the project: the documents are saved,
    /// then the backend builds with its events fed to the model. A build already running starts
    /// nothing.
    fn start_build(
        &mut self,
        kind: BuildKind,
        file: Option<String>,
        activate: bool,
        cmake: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.build_run(cx) else { return };
        let request = Backend::next_request_id();
        let started = run.update(cx, |run, cx| {
            cx.notify();
            run.start(kind, file.clone(), activate, request, true)
        });
        let Some(started) = started else { return };
        self.carry_out_build(started.effects, window, cx);
        let saving = self.save_all(cx);
        let label = self.label.clone();
        let clear: Vec<PathBuf> = started.clear.into_iter().map(PathBuf::from).collect();
        let task = cx.spawn_in(window, async move |workbench, cx| {
            if let Err(error) = saving.await {
                tracing::warn!("build: not every document was saved first: {error}");
            }
            let (sender, mut events) = unbounded::<BuildEvent>();
            let channel = ide_host::ops::Channel::new(move |event| {
                // The receiver is gone only when the window is.
                let _ = sender.unbounded_send(event);
            });
            let Ok(built) = workbench.update(cx, |_, cx| {
                cx.global::<Backend>().with_host(move |host| async move {
                    let file = file.map(PathBuf::from);
                    ide_host::ops::build::build_run(&host, &label, kind, file, cmake, request, clear, channel).await
                })
            }) else {
                return;
            };
            let mut built = Box::pin(futures::FutureExt::fuse(built));
            let outcome = loop {
                futures::select! {
                    event = events.next() => match event {
                        Some(first) => {
                            // What has arrived together is applied together.
                            let mut batch = vec![first];
                            while let Ok(more) = events.try_recv() {
                                batch.push(more);
                            }
                            let applied = workbench.update_in(cx, |workbench, window, cx| {
                                let effects = run.update(cx, |run, cx| {
                                    cx.notify();
                                    batch.into_iter().flat_map(|event| run.apply(request, event)).collect()
                                });
                                workbench.carry_out_build(effects, window, cx);
                            });
                            if applied.is_err() {
                                return;
                            }
                        }
                        // Every event is in: the outcome follows.
                        None => break built.await,
                    },
                    outcome = built => break outcome,
                }
            };
            // Events that arrived with the outcome come before it.
            let late: Vec<BuildEvent> = std::iter::from_fn(|| events.try_recv().ok()).collect();
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let effects = run.update(cx, |run, cx| {
                    cx.notify();
                    let mut effects: Vec<BuildEffect> =
                        late.into_iter().flat_map(|event| run.apply(request, event)).collect();
                    effects.extend(match outcome {
                        Ok(Ok(result)) => run.finish(request, result),
                        Ok(Err(IdeError { kind: ErrorKind::Cancelled, .. })) => run.cancelled(request),
                        Ok(Err(error)) => run.fail(request, &error.message),
                        Err(error) => run.fail(request, &error.to_string()),
                    });
                    effects
                });
                if let Some(slot) = workbench.build.as_mut() {
                    slot.running = None;
                }
                workbench.carry_out_build(effects, window, cx);
            });
        });
        if let Some(slot) = self.build.as_mut() {
            slot.running = Some(task);
        }
    }

    /// Stop Build: the running build's request is cancelled.
    fn stop_build(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stop) = self.build.as_ref().and_then(|slot| slot.run.read(cx).stop()) else { return };
        self.carry_out_build(vec![stop], window, cx);
    }

    /// Carries out what the build model asks of the window.
    fn carry_out_build(&mut self, effects: Vec<BuildEffect>, window: &mut Window, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                BuildEffect::ShowToolWindow => {
                    self.with_layout(window, cx, |layout| layout.show(BUILD_TOOL_WINDOW, false));
                }
                BuildEffect::Context { running, last_status } => {
                    set_context(window, cx, BUILD_RUNNING, running);
                    match last_status {
                        Some(status) => set_context(window, cx, BUILD_LAST_STATUS, status.as_str()),
                        None => clear_context(window, cx, BUILD_LAST_STATUS),
                    }
                }
                BuildEffect::Task { title, message, done } => {
                    let id = TaskId::Name(BUILD_TASK_ID.to_owned());
                    // Stop Build stops it: the window is its producer.
                    let cancel = if done { Change::Clear } else { Change::Set(Cancel::Producer) };
                    let update = TaskUpdate { message, done, cancel, ..TaskUpdate::new(id, &title) };
                    self.tasks.upsert_task(update, now_ms());
                    self.schedule_tasks(cx);
                }
                BuildEffect::Notify { level, title, body, show_build } => {
                    let level = match level {
                        NotifyLevel::Info => NotificationLevel::Info,
                        NotifyLevel::Warning => NotificationLevel::Warning,
                        NotifyLevel::Error => NotificationLevel::Error,
                    };
                    let mut request = NotificationRequest::new(level, title);
                    request.body = body;
                    request.group = Some("run".to_owned());
                    request.key = Some("build-finished".to_owned());
                    if show_build {
                        let command = format!("toolwindow.{BUILD_TOOL_WINDOW}");
                        request.actions.push(NotificationAction { label: "Show Build".to_owned(), command });
                    }
                    self.notify(request, cx);
                }
                BuildEffect::Navigate { path, line, col, focus } => {
                    self.open_build_location(path, line, col, focus, window, cx);
                }
                BuildEffect::Cancel(request) => {
                    if let Some(backend) = cx.try_global::<Backend>() {
                        backend.host.cancel_request(request);
                    }
                }
            }
        }
        cx.notify();
    }

    /// Opens a message's file at its place; without `focus` the keyboard stays in the Build window.
    fn open_build_location(
        &mut self,
        path: String,
        line: Option<u32>,
        col: Option<u32>,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match line {
            Some(line) => self.open_project_file_at(PathBuf::from(path), line, col.unwrap_or(0), window, cx),
            None => self.open_project_file(PathBuf::from(path), window, cx),
        }
        if !focus && let Some(view) = self.tool_window_view(BUILD_TOOL_WINDOW) {
            let handle = view.downcast::<BuildView>().map(|view| view.focus_handle(cx));
            if let Ok(handle) = handle {
                window.focus(&handle, cx);
            }
        }
    }

    /// Carries out what the Build tool window asks for.
    fn build_view_event(
        &mut self,
        _: &Entity<BuildView>,
        event: &BuildViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            BuildViewEvent::Rerun => {
                let Some(run) = self.build.as_ref().map(|slot| slot.run.clone()) else { return };
                let (kind, file) = run.read(cx).rerun_target();
                self.start_build(kind, file, true, None, window, cx);
            }
            BuildViewEvent::Stop => self.stop_build(window, cx),
            BuildViewEvent::Navigate { path, line, col, focus } => {
                self.open_build_location(path, line, col, focus, window, cx);
            }
            BuildViewEvent::OpenUrl(url) => cx.open_url(&url),
            BuildViewEvent::OpenLocation { target, cwd } => {
                if let Some(driver) = self.run_driver().cloned() {
                    driver.update(cx, |driver, cx| driver.open_location(target, cwd, cx));
                }
            }
        }
    }
}
