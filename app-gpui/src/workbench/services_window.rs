//! The Services tool window of a project: its view, the run-configuration provider over the run
//! driver's store, and the Docker provider's store with the backend work its requests ask for.
//!
//! What the window keeps about the project's Services (added types, remembered runs, hidden
//! configurations, display order) and the Docker store live in a small entity of their own, so the
//! view's [`ServicesSource`] can read them while the workbench is being updated.

use super::{Panel, RUN_TOOL_WINDOW, SERVICES_TOOL_WINDOW, Workbench, for_window};
use crate::backend::Backend;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry, ActionSource, execute_action_from};
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::run::config_types::RunConfigTypes;
use crate::run::driver::RunDriver;
use crate::run::store::{RunStore, StorageWrite};
use crate::run::view::RunView;
use crate::run_testing::service_sessions::{ServiceSession, StopServiceSessions, StopStep};
use crate::run_testing::services::{
    AddService, CONFIG_PREFIX, MenuCommand, RUN_PROVIDER, RunServices, ServiceCommand, ServiceDetail, Services,
    StartAll, StartAllStep, StoredServices, TYPE_PREFIX, hidden_key, order_key, ran_key, types_key,
};
use crate::services::docker::{DOCKER_PROVIDER, DockerCommand, DockerRequest, DockerStore, FILTERS_KEY, POLL_MS};
use crate::services::model::{
    DOCKER_PROVIDER_ORDER, DetailSlot, ItemAction, NodeCommand, RUN_PROVIDER_ORDER, ServiceMenuEntry, ServiceNode,
    ServiceRequest, run_actions, run_add_menu, run_menu, run_roots,
};
use crate::services::view::{ServicesBanner, ServicesMenu, ServicesSource, ServicesView, ServicesViewEvent};
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, AsyncApp, BorrowAppContext, Context, Entity, IntoElement, Render, SharedString, Subscription, Task,
    UniformListScrollHandle, WeakEntity, Window, div, prelude::*, px, uniform_list,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// How often an awaited process exit is checked.
const EXIT_POLL_MS: u64 = 50;

/// The Services of the open project, and the Docker store.
pub(super) struct ServicesState {
    run: RunServices,
    docker: DockerStore,
}

/// The Services tool window of the open project.
pub(super) struct ServicesWindow {
    view: Entity<ServicesView>,
    state: Entity<ServicesState>,
    /// The run driver of the project the window was built for.
    driver: Entity<RunDriver>,
    store: Entity<RunStore>,
    types: Rc<RunConfigTypes>,
    /// Whether the window was last known to be shown (the Docker store is watched meanwhile).
    shown: bool,
    /// The poll of the watched Docker connections, while the window shows.
    poll: Option<Task<()>>,
    /// The detail pane of the selected run-configuration node.
    detail: Option<Entity<ServiceDetailPane>>,
    /// The start of every Run session seen, by session id: a new start is a run to remember.
    seen_runs: HashMap<String, f64>,
    /// The roots last given to the view (run configurations, Docker): output alone changes nothing.
    pushed: Option<(Vec<ServiceNode>, Vec<ServiceNode>)>,
    _subscriptions: Vec<Subscription>,
}

#[cfg(test)]
impl ServicesWindow {
    pub(super) fn detail(&self) -> Option<&Entity<ServiceDetailPane>> {
        self.detail.as_ref()
    }
}

/// Builds the run-configuration provider's [`Services`] over the store and the state as they are.
fn with_services<R>(
    store: &Entity<RunStore>,
    types: &RunConfigTypes,
    state: &Entity<ServicesState>,
    cx: &App,
    f: impl FnOnce(&Services<'_>) -> R,
) -> R {
    let state = state.read(cx);
    f(&Services { store: store.read(cx), debug: &[], types, state: &state.run })
}

/// Carries out storage writes the Services models return.
fn write_storage(writes: impl IntoIterator<Item = StorageWrite>, cx: &mut App) {
    let storage = cx.default_global::<UiStorage>();
    for write in writes {
        match write.value {
            Some(value) => storage.set(&write.key, value),
            None => storage.remove(&write.key),
        }
    }
}

/// What the view reads about the providers' nodes.
struct Source {
    store: Entity<RunStore>,
    types: Rc<RunConfigTypes>,
    state: Entity<ServicesState>,
}

impl Source {
    fn services<R>(&self, cx: &App, f: impl FnOnce(&Services<'_>) -> R) -> R {
        with_services(&self.store, &self.types, &self.state, cx, f)
    }
}

impl ServicesSource for Source {
    fn actions(&self, provider: &str, key: &str, cx: &App) -> Vec<ItemAction<NodeCommand>> {
        match provider {
            RUN_PROVIDER => self.services(cx, |services| run_actions(services, key)),
            DOCKER_PROVIDER => self.state.read(cx).docker.actions(key),
            _ => Vec::new(),
        }
    }

    fn menu(&self, provider: &str, key: &str, cx: &App) -> Vec<ServiceMenuEntry<NodeCommand>> {
        match provider {
            RUN_PROVIDER => self.services(cx, |services| run_menu(services, key)),
            DOCKER_PROVIDER => self.state.read(cx).docker.menu(key),
            _ => Vec::new(),
        }
    }

    fn can_move(&self, provider: &str, source: &str, target: &str, cx: &App) -> bool {
        provider == RUN_PROVIDER && self.services(cx, |services| services.can_move(source, target))
    }

    fn add_menu(&self, cx: &App) -> Vec<Vec<ServiceMenuEntry<NodeCommand>>> {
        vec![self.services(cx, run_add_menu), self.state.read(cx).docker.add_menu()]
    }

    fn view_options(&self, cx: &App) -> Vec<ServiceMenuEntry<NodeCommand>> {
        self.state.read(cx).docker.view_options_menu()
    }

    fn banners(&self, cx: &App) -> Vec<ServicesBanner> {
        let docker = &self.state.read(cx).docker;
        let banner = |task: &crate::services::docker::DockerTask| ServicesBanner {
            title: task.title.clone(),
            line: task.line.clone(),
            cancel: Some(NodeCommand::Docker(DockerCommand::CancelTask)),
        };
        docker.task().map(banner).into_iter().collect()
    }
}

/// The detail pane of a run-configuration node: the console of its Run session, or what the node
/// is while nothing ran. It follows the store, so a run started while the node is selected shows.
pub(super) struct ServiceDetailPane {
    store: Entity<RunStore>,
    types: Rc<RunConfigTypes>,
    state: Entity<ServicesState>,
    key: String,
    scroll: UniformListScrollHandle,
    _observers: Vec<Subscription>,
}

/// What the pane draws.
enum PaneText {
    /// The console of this Run session.
    Console(String),
    Lines(Vec<String>),
}

impl ServiceDetailPane {
    fn new(
        store: Entity<RunStore>,
        types: Rc<RunConfigTypes>,
        state: Entity<ServicesState>,
        key: String,
        cx: &mut Context<Self>,
    ) -> Self {
        let observers = vec![cx.observe(&store, |_, _, cx| cx.notify()), cx.observe(&state, |_, _, cx| cx.notify())];
        Self { store, types, state, key, scroll: UniformListScrollHandle::new(), _observers: observers }
    }

    fn text(&self, cx: &App) -> PaneText {
        let detail = with_services(&self.store, &self.types, &self.state, cx, |services| services.detail(&self.key));
        match detail {
            ServiceDetail::RunConsole { session_id, .. } => PaneText::Console(session_id),
            // No debugger runs in this window, so a debug console has nothing to show.
            ServiceDetail::DebugConsole(_) => PaneText::Lines(Vec::new()),
            ServiceDetail::Group { title } => PaneText::Lines(vec![title]),
            ServiceDetail::NotStarted { config, type_label, command_line, .. } => {
                PaneText::Lines(vec![config.name, type_label, command_line])
            }
        }
    }

    /// The pane's lines, top to bottom.
    #[cfg(test)]
    pub(super) fn lines(&self, cx: &App) -> Vec<String> {
        match self.text(cx) {
            PaneText::Console(session) => self
                .store
                .read(cx)
                .console(&session)
                .map(|console| console.lines().iter().map(|line| line.text.clone()).collect())
                .unwrap_or_default(),
            PaneText::Lines(lines) => lines,
        }
    }
}

impl Render for ServiceDetailPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = match self.text(cx) {
            PaneText::Console(session) => self.store.read(cx).console(&session).map_or(0, |c| c.lines().len()),
            PaneText::Lines(lines) => lines.len(),
        };
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
        let list = uniform_list(
            "services-detail-lines",
            count,
            cx.processor(|pane: &mut Self, range: std::ops::Range<usize>, _, cx| {
                let texts: Vec<String> = match pane.text(cx) {
                    PaneText::Console(session) => pane
                        .store
                        .read(cx)
                        .console(&session)
                        .map(|console| {
                            console
                                .lines()
                                .get(range.clone())
                                .unwrap_or_default()
                                .iter()
                                .map(|l| l.text.clone())
                                .collect()
                        })
                        .unwrap_or_default(),
                    PaneText::Lines(lines) => lines.get(range.clone()).unwrap_or_default().to_vec(),
                };
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

/// Runs `f` with the workbench of the action's window.
fn on_workbench(
    window: &mut Window,
    cx: &mut App,
    f: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>),
) -> anyhow::Result<()> {
    let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
    workbench.update(cx, |workbench, cx| f(workbench, window, cx));
    Ok(())
}

/// The string argument of an action.
fn text_arg(arg: Option<&serde_json::Value>) -> Option<String> {
    arg.and_then(serde_json::Value::as_str).map(str::to_owned)
}

/// Registers the Services and Docker actions this window carries out.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("services.show", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(SERVICES_TOOL_WINDOW, true));
        })
    });
    registry.handle("services.addService", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.with_layout(window, cx, |layout| layout.show(SERVICES_TOOL_WINDOW, true));
            let width = f32::from(window.viewport_size().width);
            let position = gpui_kit::point(px((width / 2.0 - 120.0).max(0.0)), px(44.0));
            workbench.show_services_menu(ServicesMenu::AddService, position, window, cx);
        })
    });
    // The toolbar buttons of a type group, by the node key the argument names.
    for (id, button, title, icon) in [
        ("run.services.runAll", "run-all", "Run All", "run"),
        ("run.services.restartAll", "restart-all", "Restart All", "rerun"),
        ("run.services.stopAll", "stop-all", "Stop All", "stop"),
        ("run.services.openBrowser", "open-browser", "Open in Browser", "web"),
    ] {
        let meta =
            ActionMeta { icon: Some(icon.to_owned()), hidden: true, ..ActionMeta::new(id, title).category("Run") };
        registry.register(Action::new(meta, move |call, window, cx| {
            let Some(key) =
                text_arg(call.arg).filter(|key| key.starts_with(TYPE_PREFIX) || key.starts_with(CONFIG_PREFIX))
            else {
                return Ok(());
            };
            on_workbench(window, cx, |workbench, window, cx| {
                workbench.carry_out_services(vec![NodeCommand::RunAction { key, action: button }], window, cx);
            })
        }));
    }
    registry.handle("run.services.addType", |call, window, cx| {
        let Some(kind) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, |workbench, window, cx| {
            workbench.carry_out_services(vec![NodeCommand::AddRunType(kind)], window, cx);
        })
    });
    registry.handle("run.services.hideConfig", |call, window, cx| {
        let Some(id) = text_arg(call.arg) else { return Ok(()) };
        on_workbench(window, cx, |workbench, window, cx| {
            let Some(services) = workbench.services_window(window, cx) else { return };
            let kind = services.store.read(cx).config(&id).map(|config| config.kind.clone());
            if let Some(kind) = kind {
                workbench.carry_out_services(
                    vec![NodeCommand::RunMenu(MenuCommand::HideConfig { kind, id })],
                    window,
                    cx,
                );
            }
        })
    });
    registry.handle("docker.refresh", |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| {
            let Some(state) = workbench.services_window(window, cx).map(|services| services.state.clone()) else {
                return;
            };
            let requests = state.update(cx, |state, cx| {
                cx.notify();
                state.docker.refresh_all()
            });
            workbench.carry_out_docker(&state, requests, cx);
        })
    });
    for (id, command) in [
        ("docker.toggleStoppedContainers", DockerCommand::ToggleStoppedContainers),
        ("docker.toggleUntaggedImages", DockerCommand::ToggleUntaggedImages),
    ] {
        registry.handle(id, move |_, window, cx| {
            let command = command.clone();
            on_workbench(window, cx, |workbench, window, cx| {
                workbench.carry_out_services(vec![NodeCommand::Docker(command)], window, cx);
            })
        });
    }
}

impl Workbench {
    /// The Services window of the open project, built on first use and again for another project.
    pub(super) fn services_window(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<&ServicesWindow> {
        let driver = self.run_driver()?.clone();
        if self.services.as_ref().is_none_or(|services| services.driver != driver) {
            let root = self.project_root(cx)?.to_string_lossy().into_owned();
            self.services = Some(self.new_services_window(driver, &root, window, cx));
            // The runs of the project's open sessions are remembered as well.
            self.services_store_changed(cx);
        }
        self.services.as_ref()
    }

    fn new_services_window(
        &mut self,
        driver: Entity<RunDriver>,
        root: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ServicesWindow {
        let (store, types) = (driver.read(cx).store().clone(), driver.read(cx).types());
        let stored =
            |key: String| cx.try_global::<UiStorage>().and_then(|storage| storage.get(&key)).map(str::to_owned);
        let (added, ran, hidden, order) =
            (stored(types_key(root)), stored(ran_key(root)), stored(hidden_key(root)), stored(order_key(root)));
        let filters = stored(FILTERS_KEY.to_owned());
        let run = RunServices::open(
            Some(root),
            Some(root),
            StoredServices {
                types: added.as_deref(),
                ran: ran.as_deref(),
                hidden: hidden.as_deref(),
                order: order.as_deref(),
            },
        );
        let state = cx.new(|_| ServicesState { run, docker: DockerStore::new(filters.as_deref()) });
        let source = Rc::new(Source { store: store.clone(), types: Rc::clone(&types), state: state.clone() });
        let view = cx.new(|cx| ServicesView::new(source, cx));
        let subscriptions = vec![
            cx.subscribe_in(&view, window, Self::services_view_event),
            cx.observe(&store, |workbench, _, cx| workbench.services_store_changed(cx)),
            cx.observe(&state, |workbench, _, cx| workbench.push_services_roots(cx)),
        ];
        ServicesWindow {
            view,
            state,
            driver,
            store,
            types,
            shown: false,
            poll: None,
            detail: None,
            seen_runs: HashMap::new(),
            pushed: None,
            _subscriptions: subscriptions,
        }
    }

    /// The tool window's panel, when the project has a run driver.
    pub(super) fn mount_services(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        let view = self.services_window(window, cx)?.view.clone();
        Some(Panel::new(view, None))
    }

    /// Follows the tool window's visibility: the view drops its detail pane while hidden, and the
    /// Docker store is watched (and polled) while shown.
    pub(super) fn sync_services_window(&mut self, cx: &mut Context<Self>) {
        let visible = self.is_tool_window_visible(SERVICES_TOOL_WINDOW);
        let Some(services) = self.services.as_mut().filter(|services| services.shown != visible) else { return };
        services.shown = visible;
        let (view, state) = (services.view.clone(), services.state.clone());
        services.poll = visible.then(|| {
            let state = state.clone();
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_millis(POLL_MS)).await;
                    let polled = this.update(cx, |workbench, cx| {
                        // The project closed: its window stops watching Docker.
                        let current = workbench.services.as_ref().is_some_and(|services| {
                            services.state == state && workbench.run_driver() == Some(&services.driver)
                        });
                        if !current {
                            if let Some(services) = workbench.services.as_mut().filter(|s| s.state == state) {
                                services.shown = false;
                            }
                            let requests = state.update(cx, |state, _| state.docker.unwatch());
                            workbench.carry_out_docker(&state, requests, cx);
                            return false;
                        }
                        let requests = state.update(cx, |state, _| state.docker.poll());
                        workbench.carry_out_docker(&state, requests, cx);
                        true
                    });
                    if !polled.unwrap_or(false) {
                        return;
                    }
                }
            })
        });
        view.update(cx, |view, cx| view.set_shown(visible, cx));
        let requests = state.update(cx, |state, cx| {
            cx.notify();
            if visible { state.docker.watch() } else { state.docker.unwatch() }
        });
        self.carry_out_docker(&state, requests, cx);
    }

    /// The run store changed: runs that started are remembered, and the roots follow.
    fn services_store_changed(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services.as_mut() else { return };
        let started: Vec<(String, f64, String)> = services
            .store
            .read(cx)
            .sessions
            .iter()
            .map(|session| (session.id.clone(), session.started_at, session.config.id.clone()))
            .collect();
        let mut ran = Vec::new();
        for (session, at, config) in started {
            if services.seen_runs.insert(session, at) != Some(at) {
                ran.push(config);
            }
        }
        if !ran.is_empty() {
            let writes: Vec<StorageWrite> = services.state.update(cx, |state, cx| {
                cx.notify();
                ran.iter().filter_map(|config| state.run.record_run(config)).collect()
            });
            write_storage(writes, cx);
        }
        self.push_services_roots(cx);
    }

    /// Gives the view each provider's roots as they are now.
    fn push_services_roots(&mut self, cx: &mut Context<Self>) {
        let Some(services) = self.services.as_mut() else { return };
        let (view, store, types, state) =
            (services.view.clone(), services.store.clone(), Rc::clone(&services.types), services.state.clone());
        let run = cx.update_default_global::<Localization, _>(|localization, cx| {
            with_services(&store, &types, &state, cx, |services| run_roots(services, &mut localization.0))
        });
        let docker = state.read(cx).docker.roots();
        let roots = Some((run, docker));
        if services.pushed == roots {
            return;
        }
        services.pushed.clone_from(&roots);
        let Some((run, docker)) = roots else { return };
        view.update(cx, |view, cx| {
            view.set_provider_roots(RUN_PROVIDER, RUN_PROVIDER_ORDER, run, true, cx);
            view.set_provider_roots(DOCKER_PROVIDER, DOCKER_PROVIDER_ORDER, docker, false, cx);
        });
    }

    /// Opens `menu` of the Services view at `position`; a chosen row's commands are carried out.
    fn show_services_menu(
        &mut self,
        menu: ServicesMenu,
        position: gpui_kit::Point<gpui_kit::Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(view) = self.services_window(window, cx).map(|services| services.view.clone()) else { return };
        let items = view.read(cx).menu_items(menu, cx);
        let run = |workbench: &mut Self, commands: Vec<NodeCommand>, window: &mut Window, cx: &mut Context<Self>| {
            workbench.carry_out_services(commands, window, cx);
        };
        self.show_menu(&items, position, run, window, cx);
    }

    /// Carries out what the Services view asks for.
    fn services_view_event(
        &mut self,
        _: &Entity<ServicesView>,
        event: &ServicesViewEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event.clone() {
            ServicesViewEvent::Commands(commands) => self.carry_out_services(commands, window, cx),
            ServicesViewEvent::ShowMenu { menu, position } => self.show_services_menu(menu, position, window, cx),
            ServicesViewEvent::Provider(ServiceRequest::Activate { provider, key }) => {
                let Some(services) = self.services.as_ref() else { return };
                match provider.as_str() {
                    RUN_PROVIDER => {
                        let command =
                            with_services(&services.store, &services.types, &services.state, cx, |s| s.activate(&key));
                        if let Some(command) = command {
                            self.run_service_command(command, window, cx);
                        }
                    }
                    DOCKER_PROVIDER => {
                        let state = services.state.clone();
                        let requests = state.update(cx, |state, cx| {
                            cx.notify();
                            state.docker.activate(&key)
                        });
                        self.carry_out_docker(&state, requests, cx);
                    }
                    _ => {}
                }
            }
            // Only plugin providers list children lazily or follow the selection, and none is here.
            ServicesViewEvent::Provider(ServiceRequest::Expand { .. } | ServiceRequest::Select { .. }) => {}
            ServicesViewEvent::DetailChanged(slot) => self.services_detail_changed(slot, cx),
        }
    }

    /// The detail pane shows another node: a run-configuration node gets its pane.
    fn services_detail_changed(&mut self, slot: Option<DetailSlot>, cx: &mut Context<Self>) {
        let Some(services) = self.services.as_mut() else { return };
        services.detail = None;
        let Some(slot) = slot else { return };
        if slot.provider != RUN_PROVIDER {
            tracing::info!(provider = %slot.provider, "Services: no detail pane is built for this provider");
            return;
        }
        let (store, types, state) = (services.store.clone(), Rc::clone(&services.types), services.state.clone());
        let key = slot.key.clone();
        let pane = cx.new(|cx| ServiceDetailPane::new(store, types, state, key, cx));
        services.detail = Some(pane.clone());
        services.view.clone().update(cx, |view, cx| view.set_detail(&slot, pane.into(), cx));
    }

    /// Carries out commands of the Services view, its menus and the Services actions, in order.
    fn carry_out_services(&mut self, commands: Vec<NodeCommand>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(services) = self.services_window(window, cx) else { return };
        let (store, types, state, driver) =
            (services.store.clone(), Rc::clone(&services.types), services.state.clone(), services.driver.clone());
        for command in commands {
            match command {
                NodeCommand::RunAction { key, action } => {
                    let command = with_services(&store, &types, &state, cx, |services| services.invoke(&key, action));
                    if let Some(command) = command {
                        self.run_service_command(command, window, cx);
                    }
                }
                NodeCommand::RunMenu(command) => self.run_service_menu(command, &state, &driver, window, cx),
                NodeCommand::AddRunType(kind) => add_run_type(kind, &state, &driver, cx),
                NodeCommand::Move { provider, source, target, position } if provider == RUN_PROVIDER => {
                    let order = with_services(&store, &types, &state, cx, |services| {
                        services.moved(&source, &target, position)
                    });
                    if let Some(order) = order {
                        let write = state.update(cx, |state, cx| {
                            cx.notify();
                            state.run.set_order(order)
                        });
                        write_storage(write, cx);
                    }
                }
                NodeCommand::Move { provider, .. } => {
                    tracing::info!(%provider, "Services: this provider does not reorder its nodes");
                }
                NodeCommand::Docker(command) => {
                    let requests = cx.update_default_global::<Localization, _>(|localization, cx| {
                        state.update(cx, |state, cx| {
                            cx.notify();
                            state.docker.carry_out(command, &mut localization.0)
                        })
                    });
                    self.carry_out_docker(&state, requests, cx);
                }
                NodeCommand::Plugin { provider, command, .. } => {
                    tracing::info!(%provider, %command, "Services: no plugin provider is loaded in this window");
                }
            }
        }
    }

    /// A run-configuration menu entry.
    fn run_service_menu(
        &mut self,
        command: MenuCommand,
        state: &Entity<ServicesState>,
        driver: &Entity<RunDriver>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command {
            MenuCommand::Action(action) => tracing::info!(action, "Services: a toolbar entry without its node"),
            MenuCommand::AddService(kind) => add_run_type(kind, state, driver, cx),
            MenuCommand::RemoveType(kind) => {
                let write = state.update(cx, |state, cx| {
                    cx.notify();
                    state.run.remove_type(&kind)
                });
                write_storage(write, cx);
            }
            MenuCommand::HideConfig { kind, id } => {
                let write = state.update(cx, |state, cx| {
                    cx.notify();
                    state.run.hide_config(&kind, &id)
                });
                write_storage(write, cx);
            }
            MenuCommand::ShowRun(session) => {
                self.with_layout(window, cx, |layout| layout.show(RUN_TOOL_WINDOW, true));
                if let Some(view) =
                    self.tool_window_view(RUN_TOOL_WINDOW).and_then(|view| view.downcast::<RunView>().ok())
                {
                    view.update(cx, |view, cx| view.select_tab(&session, cx));
                }
            }
            MenuCommand::ShowDebug(id) => tracing::info!(id = id.0, "Services: this window has no debugger"),
            MenuCommand::EditConfig(id) => run_action_later("run.editConfigs", serde_json::Value::from(id), window, cx),
        }
    }

    /// Carries out what a toolbar button or activating a run-configuration node does.
    fn run_service_command(&mut self, command: ServiceCommand, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        match command {
            ServiceCommand::StartRun { config, options } => {
                driver.update(cx, |driver, cx| driver.start(*config, *options, cx)).detach();
            }
            ServiceCommand::StartAll { kind, restart } => {
                let Some(services) = &self.services else { return };
                let (store, types, state) =
                    (services.store.clone(), Rc::clone(&services.types), services.state.clone());
                let (flow, step) =
                    with_services(&store, &types, &state, cx, |services| StartAll::start(services, &kind, restart));
                cx.spawn(async move |this, cx| start_all(this, flow, step, cx).await).detach();
            }
            ServiceCommand::Stop(sessions) => {
                cx.spawn(async move |this, cx| stop_sessions(this, sessions, cx).await).detach();
            }
            ServiceCommand::OpenUrl(url) => driver.update(cx, |driver, cx| driver.open_url(&url, cx)),
            ServiceCommand::ChooseUrl(choices) => {
                let items: Vec<MenuItem<String>> =
                    choices.into_iter().map(|choice| MenuItem::item(choice.label, choice.url).icon("web")).collect();
                let width = f32::from(window.viewport_size().width);
                let position = gpui_kit::point(px((width / 2.0 - 120.0).max(0.0)), px(44.0));
                let open = move |_: &mut Self, url: String, _: &mut Window, cx: &mut Context<Self>| {
                    driver.update(cx, |driver, cx| driver.open_url(&url, cx));
                };
                self.show_menu(&items, position, open, window, cx);
            }
            ServiceCommand::RerunDebugger(id) => tracing::info!(id = id.0, "Services: this window has no debugger"),
            ServiceCommand::DebugConfig(config) => match serde_json::to_value(&*config) {
                Ok(arg) => run_action_later("run.debugConfig", arg, window, cx),
                Err(error) => tracing::error!("cannot pass the configuration to the debugger: {error}"),
            },
            ServiceCommand::UpdateApplication(session) => {
                tracing::info!(?session, "Services: Update Application is not available in this window");
            }
        }
    }

    /// Carries out the Docker store's requests: settings, snapshots and delayed refreshes run here;
    /// the events stream, container operations, streamed tasks and dialogs are not built.
    fn carry_out_docker(
        &mut self,
        state: &Entity<ServicesState>,
        requests: Vec<DockerRequest>,
        cx: &mut Context<Self>,
    ) {
        for request in requests {
            match request {
                DockerRequest::LoadSettings { ticket } => {
                    let work = |data: PathBuf| Ok(ide_docker::load_settings(&data));
                    docker_blocking(state, work, move |docker, answer| docker.settings_loaded(ticket, answer), cx);
                }
                DockerRequest::SaveSettings { ticket, settings } => {
                    let work = move |data: PathBuf| ide_docker::save_settings(&data, &settings).map_err(|e| e.message);
                    docker_blocking(state, work, move |docker, answer| docker.settings_saved(ticket, answer), cx);
                }
                DockerRequest::Snapshot { ticket, endpoint, .. } => {
                    let work = move |_: PathBuf| Ok(ide_docker::snapshot(&endpoint));
                    docker_blocking(state, work, move |docker, answer| docker.snapshot_answered(ticket, answer), cx);
                }
                // No events stream runs here: it is answered as ended, so the poll keeps reading the
                // connection.
                DockerRequest::WatchEvents { ticket, conn, .. } => {
                    tracing::info!(%conn, "Services: the Docker events stream is not followed; the poll reads it");
                    let more = state.update(cx, |state, _| state.docker.events_ended(ticket));
                    self.carry_out_docker(state, more, cx);
                }
                // Cancel stops an events stream or a streamed task, and neither runs here.
                DockerRequest::Cancel { .. } => {}
                DockerRequest::RefreshLater { ticket, delay_ms } => {
                    let state = state.clone();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(Duration::from_millis(delay_ms)).await;
                        let due = this.update(cx, |workbench, cx| {
                            let requests = state.update(cx, |state, cx| {
                                cx.notify();
                                state.docker.refresh_due(ticket)
                            });
                            workbench.carry_out_docker(&state, requests, cx);
                        });
                        if due.is_err() {
                            tracing::debug!("the window closed before a Docker refresh was due");
                        }
                    })
                    .detach();
                }
                DockerRequest::Notify(request) => self.notify(*request, cx),
                DockerRequest::Store(write) => write_storage([write], cx),
                other => {
                    tracing::info!(request = docker_request_name(&other), "Services: not carried out in this window")
                }
            }
        }
    }
}

/// Runs the action `id` with `arg` once the current update is over (it may reach for the workbench).
fn run_action_later(id: &'static str, arg: serde_json::Value, window: &mut Window, cx: &mut Context<Workbench>) {
    window.defer(cx, move |window, cx| {
        if !execute_action_from(ActionSource::ContextMenu, id, Some(&arg), window, cx) {
            tracing::info!("{id} is not available in this window");
        }
    });
}

/// The name of a Docker request, for the log.
fn docker_request_name(request: &DockerRequest) -> &'static str {
    match request {
        DockerRequest::LoadSettings { .. } => "LoadSettings",
        DockerRequest::SaveSettings { .. } => "SaveSettings",
        DockerRequest::SetRegistryPassword { .. } => "SetRegistryPassword",
        DockerRequest::Snapshot { .. } => "Snapshot",
        DockerRequest::WatchEvents { .. } => "WatchEvents",
        DockerRequest::Cancel { .. } => "Cancel",
        DockerRequest::RefreshLater { .. } => "RefreshLater",
        DockerRequest::ContainerOp { .. } => "ContainerOp",
        DockerRequest::Remove { .. } => "Remove",
        DockerRequest::Stream { .. } => "Stream",
        DockerRequest::RegistryLogin { .. } => "RegistryLogin",
        DockerRequest::Inspect { .. } => "Inspect",
        DockerRequest::Notify(_) => "Notify",
        DockerRequest::Store(_) => "Store",
        DockerRequest::ConfirmDelete(_) => "ConfirmDelete",
        DockerRequest::PromptPull { .. } => "PromptPull",
        DockerRequest::ConfirmPrune { .. } => "ConfirmPrune",
        DockerRequest::ConfirmRemoveConnection { .. } => "ConfirmRemoveConnection",
        DockerRequest::ConfirmRemoveRegistry { .. } => "ConfirmRemoveRegistry",
        DockerRequest::ConnectionDialog(_) => "ConnectionDialog",
        DockerRequest::RegistryDialog(_) => "RegistryDialog",
        DockerRequest::OpenTerminal { .. } => "OpenTerminal",
        DockerRequest::CopyText(_) => "CopyText",
        DockerRequest::OpenLogInEditor { .. } => "OpenLogInEditor",
    }
}

/// Runs blocking Docker work (`work` gets the app's data directory) on the backend's runtime, then
/// gives its answer to the store and carries out what follows.
fn docker_blocking<T: Send + 'static>(
    state: &Entity<ServicesState>,
    work: impl FnOnce(PathBuf) -> Result<T, String> + Send + 'static,
    answer: impl FnOnce(&mut DockerStore, Result<T, String>) -> Vec<DockerRequest> + 'static,
    cx: &mut Context<Workbench>,
) {
    let call = cx.try_global::<Backend>().map(|backend| {
        let data = backend.dirs.data.clone();
        backend.call(async move { tokio::task::spawn_blocking(move || work(data)).await })
    });
    let state = state.clone();
    cx.spawn(async move |this, cx| {
        let result = match call {
            Some(call) => match call.await {
                Ok(Ok(result)) => result,
                Ok(Err(error)) => Err(format!("the Docker call did not complete: {error}")),
                Err(error) => Err(error.to_string()),
            },
            None => Err("the backend is not running".to_owned()),
        };
        let answered = this.update(cx, |workbench, cx| {
            let requests = state.update(cx, |state, cx| {
                cx.notify();
                answer(&mut state.docker, result)
            });
            workbench.carry_out_docker(&state, requests, cx);
        });
        if answered.is_err() {
            tracing::debug!("the window closed before a Docker call answered");
        }
    })
    .detach();
}

/// "Add Service" for a configuration type, once the configurations are listed. Several
/// configurations of the type would be chosen in a picker, which this window does not have: the
/// ones it would preselect are shown.
fn add_run_type(kind: String, state: &Entity<ServicesState>, driver: &Entity<RunDriver>, cx: &mut App) {
    let state = state.clone();
    driver.update(cx, |driver, cx| {
        driver.when_loaded(
            move |driver, cx| {
                let configs = driver.store().read(cx).configs().to_vec();
                let writes = state.update(cx, |state, cx| {
                    cx.notify();
                    match state.run.add_service(&kind, &configs) {
                        AddService::Applied(writes) => writes,
                        AddService::Pick(pick) => {
                            tracing::info!(%kind, "Services: no configuration picker; the preselected ones show");
                            let selected = pick.selected.clone();
                            state.run.services_picked(&pick, Some(&selected))
                        }
                    }
                });
                write_storage(writes, cx);
            },
            cx,
        );
    });
}

/// The run-configuration provider's state as it is now, for a step of a flow.
fn services_now<R>(this: &WeakEntity<Workbench>, cx: &mut AsyncApp, f: impl FnOnce(&Services<'_>) -> R) -> Option<R> {
    this.update(cx, |workbench, cx| {
        let services = workbench.services.as_ref()?;
        Some(with_services(&services.store, &services.types, &services.state, cx, f))
    })
    .ok()
    .flatten()
}

/// Run All / Restart All of a type group, step by step.
async fn start_all(this: WeakEntity<Workbench>, mut flow: StartAll, mut step: StartAllStep, cx: &mut AsyncApp) {
    loop {
        step = match step {
            StartAllStep::Done => return,
            StartAllStep::StopSessions(sessions) => {
                stop_sessions(this.clone(), sessions, cx).await;
                let Some(next) = services_now(&this, cx, |services| flow.advance(services)) else { return };
                next
            }
            StartAllStep::WaitForExit { session_ids } => {
                wait_for_exit(&this, &session_ids, None, cx).await;
                let Some(next) = services_now(&this, cx, |services| flow.advance(services)) else { return };
                next
            }
            StartAllStep::StartRun { config, options } => {
                let start = this.update(cx, |workbench, cx| {
                    workbench
                        .run_driver()
                        .cloned()
                        .map(|driver| driver.update(cx, |d, cx| d.start(*config, *options, cx)))
                });
                let Ok(Some(start)) = start else { return };
                let next = match start.await {
                    Some(_) => services_now(&this, cx, |services| flow.advance(services)),
                    None => cx.update(|cx| {
                        let i18n = &mut cx.default_global::<Localization>().0;
                        let error = i18n.tx("The process could not be started");
                        Some(flow.start_failed(&error, i18n))
                    }),
                };
                let Some(next) = next else { return };
                next
            }
            StartAllStep::Notify(request) => {
                if this.update(cx, |workbench, cx| workbench.notify(*request, cx)).is_err() {
                    return;
                }
                let Some(next) = services_now(&this, cx, |services| flow.advance(services)) else { return };
                next
            }
        };
    }
}

/// Stops executions: each live Run session once, waiting where the flow says so.
async fn stop_sessions(this: WeakEntity<Workbench>, sessions: Vec<ServiceSession>, cx: &mut AsyncApp) {
    let (mut stop, mut step) = StopServiceSessions::start(&sessions);
    loop {
        match step {
            StopStep::Done => return,
            StopStep::StopRun { session_id } => {
                let stopped = this.update(cx, |workbench, cx| {
                    if let Some(driver) = workbench.run_driver().cloned() {
                        driver.update(cx, |driver, cx| driver.stop(&session_id, cx));
                    }
                });
                if stopped.is_err() {
                    return;
                }
            }
            StopStep::StopDebugger { id, .. } => tracing::info!(id = id.0, "Services: this window has no debugger"),
            StopStep::WaitForExit { session_ids, timeout_ms } => {
                wait_for_exit(&this, &session_ids, timeout_ms, cx).await
            }
        }
        let runs = this.update(cx, |workbench, cx| {
            workbench.run_driver().map(|driver| driver.read(cx).store().read(cx).sessions.clone())
        });
        let Ok(Some(runs)) = runs else { return };
        step = stop.advance(&runs);
    }
}

/// Waits until none of these Run sessions is live, at most `timeout_ms` (`None`: as long as it
/// takes).
async fn wait_for_exit(
    this: &WeakEntity<Workbench>,
    session_ids: &[String],
    timeout_ms: Option<f64>,
    cx: &mut AsyncApp,
) {
    let mut waited = 0.0;
    loop {
        let live = this.update(cx, |workbench, cx| {
            workbench.run_driver().is_some_and(|driver| {
                let store = driver.read(cx).store().read(cx);
                session_ids.iter().any(|id| store.session(id).is_some_and(|session| session.is_live()))
            })
        });
        if !live.unwrap_or(false) || timeout_ms.is_some_and(|timeout| waited >= timeout) {
            return;
        }
        cx.background_executor().timer(Duration::from_millis(EXIT_POLL_MS)).await;
        waited += EXIT_POLL_MS as f64;
    }
}
