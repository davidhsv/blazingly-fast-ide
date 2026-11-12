//! The run configurations of a project window: the run widget of the title bar, the configuration
//! chooser and Edit Configurations, carried out on the window's run driver.
//!
//! The chooser is a popup: it lives in the popup slot and a click beside it closes it, as any other
//! popup that opens does. Edit Configurations is a dialog over the window, a layer of its own below the
//! popup slot (as the diff window is): only its own buttons and Escape close it, so its working copy is
//! not lost to a stray click or to a menu opened over it. Its menus are drawn inside it.

use super::services_window::on_workbench;
use super::{TOOL_WINDOWS, Workbench, tool_window_declaration};
use crate::platform::actions::{ActionRegistry, ActionSource, execute_action_from};
use crate::platform::context::{clear_context, set_context};
use crate::run::application_update::updatable_sessions;
use crate::run::chooser_view::{ChooserAction, ConfigChooser, ConfigChooserEvent};
use crate::run::cmake::cmake_build_id;
use crate::run::config_types::BrowseKind;
use crate::run::driver::RunDriver;
use crate::run::edit_configs_view::{AfterSave, EditConfigs, EditConfigsEvent};
use crate::run::store::{CURRENT_FILE, ChooserMode, RunStore};
use crate::run::widget_view::{RunWidget, RunWidgetEvent};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use futures::channel::oneshot;
use gpui_kit::{
    AnyElement, AnyView, App, Context, Entity, FocusHandle, Focusable, IntoElement, PathPromptOptions, Subscription,
    Window, div, prelude::*,
};
use ide_run::RunConfig;
use serde_json::Value;

/// The debugger's action the run widget, the chooser and the dialog hand Debug to.
const DEBUG_ACTION: &str = "run.debug";

/// The run widget of the project's title bar.
pub(super) struct RunWidgetSlot {
    widget: Entity<RunWidget>,
    /// The context keys last published for the store, to tell a change that needs them published again.
    published: RunContext,
    _subscriptions: Vec<Subscription>,
}

/// Edit Configurations, while it is open.
pub(super) struct EditConfigsWindow {
    dialog: Entity<EditConfigs>,
    /// What had the keyboard before the dialog opened.
    restore: Option<FocusHandle>,
    _events: Subscription,
}

/// The context keys that follow the run configurations.
#[derive(Clone, Debug, Default, PartialEq)]
struct RunContext {
    /// The kind of the selected configuration, empty without one.
    selected_kind: String,
    /// The CMake build configuration that builds the selection (or the project).
    cmake_build: Option<String>,
    /// A running process can be updated (Update application).
    updatable_running: bool,
}

impl RunContext {
    fn of(store: &RunStore) -> Self {
        let selected = store.selected_config();
        let cmake = selected.or_else(|| {
            store.configs().iter().find(|c| c.kind == "cmake" && c.args.first().is_some_and(|a| a == "--build"))
        });
        Self {
            selected_kind: selected.map(|config| config.kind.clone()).unwrap_or_default(),
            cmake_build: cmake_build_id(cmake, store.configs()),
            // There is no debugger in this window: only run sessions can be updated.
            updatable_running: !updatable_sessions(&store.sessions, &[]).is_empty(),
        }
    }
}

fn registered(id: &str, cx: &App) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

/// The string argument of an action.
fn text_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(Value::as_str).map(str::to_owned)
}

/// Registers the actions of the chooser and of Edit Configurations.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("run.chooseConfig", |call, window, cx| {
        let mode = if text_arg(call.arg).as_deref() == Some("select") { ChooserMode::Select } else { ChooserMode::Run };
        on_workbench(window, cx, |workbench, window, cx| workbench.choose_config(mode, window, cx))
    });
    registry.handle("run.editConfigs", |call, window, cx| {
        let target = text_arg(call.arg);
        on_workbench(window, cx, |workbench, window, cx| workbench.edit_configs(target, window, cx))
    });
    registry.handle("run.createConfig", |call, window, cx| {
        let Some(config) = call.arg.and_then(|arg| serde_json::from_value::<RunConfig>(arg.clone()).ok()) else {
            return Ok(());
        };
        on_workbench(window, cx, |workbench, window, cx| workbench.create_config(config, window, cx))
    });
}

impl Workbench {
    /// The run widget of the title bar, while a project is open.
    pub fn run_widget(&self) -> Option<&Entity<RunWidget>> {
        self.run_driver().and(self.run_widget.as_ref()).map(|slot| &slot.widget)
    }

    /// The configuration chooser, while it is open.
    pub fn config_chooser(&self) -> Option<Entity<ConfigChooser>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// Edit Configurations, while it is open.
    pub fn edit_configs_dialog(&self) -> Option<&Entity<EditConfigs>> {
        self.edit_configs.as_ref().map(|open| &open.dialog)
    }

    /// Puts the run widget of `driver`'s project in the title bar.
    pub(super) fn mount_run_widget(&mut self, driver: &Entity<RunDriver>, window: &mut Window, cx: &mut Context<Self>) {
        let (store, types) = {
            let driver = driver.read(cx);
            (driver.store().clone(), driver.types())
        };
        let debug = registered(DEBUG_ACTION, cx);
        let widget = cx.new(|cx| {
            let mut widget = RunWidget::new(store.clone(), types, cx);
            widget.set_debug_available(debug, cx);
            widget
        });
        let events = cx.subscribe_in(&widget, window, Self::run_widget_event);
        // The context keys follow the store; the window is drawn again only when one of them changes.
        let observed = cx.observe(&store, |workbench, store, cx| {
            let now = RunContext::of(store.read(cx));
            if let Some(slot) = workbench.run_widget.as_mut().filter(|slot| slot.published != now) {
                slot.published = now;
                cx.notify();
            }
        });
        let published = RunContext::of(store.read(cx));
        self.run_widget = Some(RunWidgetSlot { widget, published, _subscriptions: vec![events, observed] });
    }

    /// The project goes: its widget and its dialog go with it.
    pub(super) fn drop_run_configs(&mut self) {
        self.run_widget = None;
        self.edit_configs = None;
    }

    /// Publishes the context keys of the run configurations.
    pub(super) fn publish_run_context(&self, window: &mut Window, cx: &mut App) {
        let context = self.run_driver().map(|driver| RunContext::of(driver.read(cx).store().read(cx)));
        let context = context.unwrap_or_default();
        set_context(window, cx, "run.selectedKind", context.selected_kind);
        match context.cmake_build {
            Some(id) => set_context(window, cx, "run.cmakeBuild", id),
            None => clear_context(window, cx, "run.cmakeBuild"),
        }
        set_context(window, cx, "run.updatableRunning", context.updatable_running);
    }

    /// A popup closed: when it was the chooser, the store says so (the widget's button is no longer
    /// pressed).
    pub(super) fn run_popup_closed(&mut self, view: &AnyView, cx: &mut Context<Self>) {
        if view.clone().downcast::<ConfigChooser>().is_err() {
            return;
        }
        if let Some(driver) = self.run_driver().cloned() {
            driver.read(cx).store().clone().update(cx, |store, cx| {
                store.chooser_open = false;
                cx.notify();
            });
        }
    }

    /// Waits until `driver` has listed the configurations, then does `then` in the window.
    fn when_configs_listed(
        &mut self,
        driver: &Entity<RunDriver>,
        then: impl FnOnce(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (sender, listed) = oneshot::channel();
        driver.update(cx, |driver, cx| {
            driver.when_loaded(
                move |_, _| {
                    // The receiver is gone only with the window.
                    let _ = sender.send(());
                },
                cx,
            );
        });
        cx.spawn_in(window, async move |workbench, cx| {
            // The project closed before its configurations were listed: there is nothing to show.
            if listed.await.is_err() {
                return;
            }
            let _ = workbench.update_in(cx, then);
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // The run widget

    fn run_widget_event(
        &mut self,
        widget: &Entity<RunWidget>,
        event: &RunWidgetEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            RunWidgetEvent::OpenChooser => window.defer(cx, |window, cx| {
                let arg = Value::from("select");
                execute_action_from(ActionSource::Toolbar, "run.chooseConfig", Some(&arg), window, cx);
            }),
            &RunWidgetEvent::Launch { debug } => self.launch_from_widget(debug, widget, window, cx),
            RunWidgetEvent::Stop => window.defer(cx, |window, cx| {
                let arg = Value::from("widget");
                execute_action_from(ActionSource::Toolbar, "run.stop", Some(&arg), window, cx);
            }),
            &RunWidgetEvent::ShowMore(position) => {
                let items = widget.read(cx).more_menu();
                self.show_menu(&items, position, |_, never, _, _| match never {}, window, cx);
            }
        }
    }

    /// Run or Debug from the widget: Run is `run.run`'s launch, which the widget waits for; Debug goes to
    /// the debugger's action, when one is registered.
    fn launch_from_widget(
        &mut self,
        debug: bool,
        widget: &Entity<RunWidget>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let driver = self.run_driver().cloned();
        let selected = driver.as_ref().and_then(|driver| driver.read(cx).store().read(cx).selected_id.clone());
        let Some(driver) = driver.filter(|_| !debug) else {
            if debug {
                window.defer(cx, |window, cx| {
                    execute_action_from(ActionSource::Toolbar, DEBUG_ACTION, None, window, cx);
                });
            }
            widget.update(cx, |widget, cx| widget.launch_ended(selected.as_deref(), cx));
            return;
        };
        let launch = driver.update(cx, |driver, cx| driver.launch_selected(cx));
        let widget = widget.downgrade();
        cx.spawn(async move |_, cx| {
            launch.await;
            // The widget is gone with its project.
            let _ = widget.update(cx, |widget, cx| widget.launch_ended(selected.as_deref(), cx));
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // The chooser

    /// Select Run/Debug Configuration: closes the chooser when it is open, else opens it once the
    /// configurations are listed. In `ChooserMode::Select` choosing a row only selects it.
    pub fn choose_config(&mut self, mode: ChooserMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        if self.config_chooser().is_some() {
            self.close_popup(window, cx);
            return;
        }
        self.when_configs_listed(
            &driver,
            move |workbench, window, cx| workbench.open_chooser(mode, window, cx),
            window,
            cx,
        );
    }

    fn open_chooser(&mut self, mode: ChooserMode, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let (store, types) = {
            let driver = driver.read(cx);
            (driver.store().clone(), driver.types())
        };
        let debug = registered(DEBUG_ACTION, cx);
        let edit_shortcut = cx
            .try_global::<crate::platform::input::Keymap>()
            .and_then(|keymap| keymap.0.shortcut_label("run.editConfigs", crate::platform::keymap::is_mac_host()));
        let chooser = cx.new(|cx| {
            let mut chooser = ConfigChooser::new(store.clone(), types, mode, cx);
            chooser.set_capabilities(debug, edit_shortcut, cx);
            chooser
        });
        let events = cx.subscribe_in(&chooser, window, Self::chooser_event);
        self.open_popup(&chooser, events, window, cx);
        store.update(cx, |store, cx| {
            store.chooser_open = true;
            store.chooser_mode = mode;
            cx.notify();
        });
    }

    fn chooser_event(
        &mut self,
        _: &Entity<ConfigChooser>,
        event: &ConfigChooserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.run_driver().cloned() else { return };
        match event.clone() {
            ConfigChooserEvent::Dismissed => self.close_popup(window, cx),
            ConfigChooserEvent::Chosen { id, how } => {
                self.close_popup(window, cx);
                driver.update(cx, |driver, cx| driver.select_config(Some(&id), cx));
                let current_file = id == CURRENT_FILE;
                let (action, arg) = match how {
                    ChooserAction::Select => return,
                    ChooserAction::Run if current_file => ("run.runContext", None),
                    ChooserAction::Run => {
                        return driver.update(cx, |driver, cx| {
                            if let Some(config) = driver.store().read(cx).config(&id).cloned() {
                                driver.start(config, Default::default(), cx).detach();
                            }
                        });
                    }
                    ChooserAction::Debug if current_file => ("run.debugContext", None),
                    ChooserAction::Debug => ("run.debugConfig", Some(Value::from(id))),
                    ChooserAction::Edit => ("run.editConfigs", Some(Value::from(id))),
                };
                Self::run_action(action, arg, window, cx);
            }
            ConfigChooserEvent::EditConfigs => {
                self.close_popup(window, cx);
                Self::run_action("run.editConfigs", None, window, cx);
            }
            ConfigChooserEvent::SaveTemporary(id) => {
                self.close_popup(window, cx);
                // A failed save is told by the driver.
                driver.update(cx, |driver, cx| driver.save_temporary(&id, cx)).detach();
            }
            ConfigChooserEvent::Rerun(session) => {
                self.close_popup(window, cx);
                driver.update(cx, |driver, cx| {
                    if let Some(rerun) = driver.rerun(&session, cx) {
                        rerun.detach();
                    }
                });
            }
            ConfigChooserEvent::Stop(session) => driver.update(cx, |driver, cx| driver.stop(&session, cx)),
            ConfigChooserEvent::TogglePinned(id) => driver.update(cx, |driver, cx| driver.toggle_pinned(&id, cx)),
            // A failed deletion is told by the driver.
            ConfigChooserEvent::Delete(id) => driver.update(cx, |driver, cx| driver.delete_config(&id, cx)).detach(),
        }
    }

    /// Runs `id` once this update is over: the action may reach for the workbench.
    fn run_action(id: &'static str, arg: Option<Value>, window: &mut Window, cx: &mut Context<Self>) {
        window.defer(cx, move |window, cx| {
            if !execute_action_from(ActionSource::Other, id, arg.as_ref(), window, cx) {
                tracing::info!("{id} is not available in this window");
            }
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Edit Configurations

    /// Edit Configurations, on `target` (else the selection) once the configurations are listed.
    pub fn edit_configs(&mut self, target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        if self.config_chooser().is_some() {
            self.close_popup(window, cx);
        }
        if let Some(open) = &self.edit_configs {
            let focus = open.dialog.focus_handle(cx);
            window.focus(&focus, cx);
            return;
        }
        self.when_configs_listed(
            &driver,
            move |workbench, window, cx| workbench.open_edit_configs(target, window, cx),
            window,
            cx,
        );
    }

    /// Reads the project's templates and opens the dialog on them; without them it opens with the
    /// types' own defaults, and the failure is told.
    fn open_edit_configs(&mut self, target: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let templates = driver.update(cx, |driver, cx| driver.load_templates(cx));
        cx.spawn_in(window, async move |workbench, cx| {
            let templates = templates.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let templates = templates.unwrap_or_else(|error| {
                    let mut request =
                        NotificationRequest::new(NotificationLevel::Warning, "Cannot read the configuration templates");
                    request.group = Some("run".to_owned());
                    request.body = Some(error);
                    workbench.notify(request, cx);
                    Vec::new()
                });
                workbench.show_edit_configs(target, templates, window, cx);
            });
        })
        .detach();
    }

    fn show_edit_configs(
        &mut self,
        target: Option<String>,
        templates: Vec<RunConfig>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.run_driver().cloned() else { return };
        if self.edit_configs.is_some() {
            return;
        }
        let (store, types) = {
            let driver = driver.read(cx);
            (driver.store().clone(), driver.types())
        };
        store.update(cx, |store, cx| {
            store.editor_open = true;
            store.chooser_open = false;
            store.edit_target = target;
            cx.notify();
        });
        let show_on_start = crate::platform::settings::effective(&self.label, cx).run.show_on_start;
        let debug = registered(DEBUG_ACTION, cx);
        let tool_windows: Vec<(String, String)> = TOOL_WINDOWS
            .into_iter()
            .filter_map(tool_window_declaration)
            .map(|declared| (declared.id.clone(), declared.title.clone()))
            .collect();
        let dialog = cx.new(|cx| {
            let mut dialog = EditConfigs::new(&store, types, templates, window, cx);
            dialog.set_capabilities(show_on_start, debug, Vec::new(), tool_windows, cx);
            dialog
        });
        let events = cx.subscribe_in(&dialog, window, Self::edit_configs_event);
        let restore = window.focused(cx);
        window.focus(&dialog.focus_handle(cx), cx);
        self.edit_configs = Some(EditConfigsWindow { dialog, restore, _events: events });
        cx.notify();
    }

    /// Closes the dialog: the store forgets it, and the keyboard goes back to what had it before.
    fn close_edit_configs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(open) = self.edit_configs.take() else { return };
        if let Some(driver) = self.run_driver().cloned() {
            driver.update(cx, |driver, cx| {
                driver.store().clone().update(cx, |store, _| store.edit_target = None);
                driver.editor_closed(cx);
            });
        }
        if let Some(target) = open.restore.or_else(|| self.editor().map(|editor| editor.focus_handle(cx))) {
            window.focus(&target, cx);
        }
        cx.notify();
    }

    fn edit_configs_event(
        &mut self,
        dialog: &Entity<EditConfigs>,
        event: &EditConfigsEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.edit_configs_dialog() != Some(dialog) {
            return;
        }
        let Some(driver) = self.run_driver().cloned() else { return };
        match event.clone() {
            EditConfigsEvent::Save { changes: None, then } => self.after_edit_save(then, window, cx),
            EditConfigsEvent::Save { changes: Some(changes), then } => {
                let saving = driver.update(cx, |driver, cx| {
                    driver.save_edited_configs(&changes.draft, changes.removed_detected_ids, cx)
                });
                let dialog = dialog.downgrade();
                cx.spawn_in(window, async move |workbench, cx| {
                    // A failure shows in the dialog; the driver told it in a notification too.
                    let saved = saving.await;
                    let ok = saved.is_ok();
                    let _ = dialog.update(cx, |dialog, cx| dialog.saved(saved, cx));
                    if ok {
                        let _ = workbench
                            .update_in(cx, |workbench, window, cx| workbench.after_edit_save(then, window, cx));
                    }
                })
                .detach();
            }
            EditConfigsEvent::Cancelled => self.close_edit_configs(window, cx),
            EditConfigsEvent::Browse { target, kind, multiple, .. } => {
                // The native dialog opens where it last was: it is not given a folder to start in.
                let picked = cx.prompt_for_paths(PathPromptOptions {
                    files: kind == BrowseKind::File,
                    directories: kind == BrowseKind::Directory,
                    multiple,
                    prompt: None,
                });
                let dialog = dialog.downgrade();
                cx.spawn_in(window, async move |workbench, cx| {
                    let paths = match picked.await {
                        Ok(Ok(paths)) => paths.unwrap_or_default(),
                        Ok(Err(error)) => {
                            let _ = workbench.update(cx, |workbench, cx| {
                                let mut request =
                                    NotificationRequest::new(NotificationLevel::Error, "Cannot open the file dialog");
                                request.group = Some("run".to_owned());
                                request.body = Some(error.to_string());
                                workbench.notify(request, cx);
                            });
                            return;
                        }
                        // The dialog was dropped without an answer: nothing was picked.
                        Err(_) => return,
                    };
                    let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
                    let _ = dialog.update(cx, |dialog, cx| dialog.browse_picked(&target, &paths, cx));
                })
                .detach();
            }
        }
    }

    /// What OK, Run or Debug do once the configurations are saved.
    fn after_edit_save(&mut self, then: AfterSave, window: &mut Window, cx: &mut Context<Self>) {
        match then {
            AfterSave::Stay => {}
            AfterSave::Close { select } => {
                if let (Some(id), Some(driver)) = (select, self.run_driver().cloned()) {
                    driver.update(cx, |driver, cx| driver.select_config(Some(&id), cx));
                }
                self.close_edit_configs(window, cx);
            }
            AfterSave::Launch { id, debug } => {
                self.close_edit_configs(window, cx);
                let action = if debug { "run.debugConfig" } else { "run.runConfig" };
                Self::run_action(action, Some(Value::from(id)), window, cx);
            }
        }
    }

    /// Create Run Configuration: saves `config` as a user configuration, selects it and opens it in
    /// Edit Configurations.
    pub fn create_config(&mut self, config: RunConfig, window: &mut Window, cx: &mut Context<Self>) {
        let Some(driver) = self.run_driver().cloned() else { return };
        let config = RunConfig { detected: false, ..config };
        self.when_configs_listed(
            &driver,
            move |workbench, window, cx| {
                let Some(driver) = workbench.run_driver().cloned() else { return };
                let id = config.id.clone();
                let saving = driver.update(cx, |driver, cx| {
                    let store = driver.store().read(cx);
                    let mut user: Vec<RunConfig> = store
                        .configs()
                        .iter()
                        .filter(|own| !own.detected && !store.temporary.contains(&own.id) && own.id != config.id)
                        .cloned()
                        .collect();
                    user.push(config);
                    driver.save_configs(user, Vec::new(), cx)
                });
                cx.spawn_in(window, async move |workbench, cx| {
                    // A failed save is told by the driver.
                    if saving.await.is_err() {
                        return;
                    }
                    let _ = workbench.update_in(cx, |workbench, window, cx| {
                        if let Some(driver) = workbench.run_driver().cloned() {
                            driver.update(cx, |driver, cx| driver.select_config(Some(&id), cx));
                        }
                        workbench.edit_configs(Some(id), window, cx);
                    });
                })
                .detach();
            },
            window,
            cx,
        );
    }

    /// Edit Configurations over the window, while it is open.
    pub(super) fn edit_configs_layer(&self) -> Option<AnyElement> {
        let dialog = self.edit_configs_dialog()?.clone();
        let layer = div()
            .id("run-edit-configs-layer")
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
