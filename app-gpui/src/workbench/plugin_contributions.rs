//! What plugins contribute to a window besides what their running code shows: the runs and debug
//! runs they ask the window for, the Run and Debug actions they put in an editor's gutter, their
//! entries in the menus, their pages in Settings, the editor events and activation events they
//! hear, and the Background Tasks popup where a plugin's task (or any other) is cancelled.
//!
//! - **Runs.** A plugin's `run.start` reaches the window as a run request: its configuration starts
//!   in the Run tool window. The backend tells the plugin when the process exits (the request's
//!   operation travels in the configuration); a run that never started is reported here as ended
//!   without a status. A debug request runs `debug.pluginStart`: the plugin's backend prepares the
//!   configuration, the session shows in the Debug tool window, and the plugin hears how it ended.
//! - **Gutter.** After each edit (debounced) the focused editor's plugin visuals are asked for; a
//!   line's actions share one icon left of the text. A click runs a single action and opens a menu
//!   of several; an answer for an older document is dropped.
//! - **Menus.** The Tools menu's plugin slot, the context menus' plugin group (with each entry's
//!   `when`, a plugin's entries of one submenu grouped, a group of one flattened), and the
//!   toolbar slots' items.
//! - **Settings.** One page per plugin (and one per declared page below it), built from its schema,
//!   staging into the dialog's `plugins` map; a page action applies first, then runs its command.
//! - **Events.** `file-saved` when a document of the window is saved, `input-mode-changed` from
//!   the editor's input mode, the `on-language:` and `on-file:` activation events when an editor
//!   is focused.
//!
//! Every host call is first authorized for the window under its desktop command's name, as the
//! plugin runtime of the window does; the host checks each plugin's grant itself.

use super::Workbench;
use super::background_tasks_view::{BackgroundTasksEvent, BackgroundTasksPopup};
use super::menu_spec::DynamicItem;
use super::plugin_runtime::{answer_plugin_command, plugin_host};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::debugger::actions::PluginEvent as DebugPluginEvent;
use crate::editor::view::{Editor, EditorEvent};
use crate::platform::actions::{ActionError, ActionRegistry, execute_action_or_throw};
use crate::platform::context::{ContextKeys, evaluate_when};
use crate::plugin_models::contribution::Contribution;
use crate::plugin_models::language_visuals::{
    ActionClick, ActionMarker, LanguageVisuals, LineAction, VisualsEffect, VisualsTicket,
};
use crate::plugin_models::marker_icons::MarkerIcons;
use crate::plugin_views::settings_page::{
    PluginSettingsEvent, PluginSettingsPage, SettingsDecl, declared_page_id, settings_page_id, settings_pages,
};
use crate::plugins::{Plugins, plugin_action_id};
use crate::run::store::SessionStatus;
use crate::settings_model::pages::SettingsPage;
use crate::settings_model::staging::Scope;
use crate::settings_view::dialog::SettingsDialogView;
use crate::theme;
use crate::ui::tooltip::WithTooltip as _;
use crate::vcs_ops::menu::{ActionItem, MenuEntry, MenuItem};
use gpui_kit::{
    AnyElement, App, Context, Entity, EntityId, MouseButton, MouseDownEvent, Pixels, Point, SharedString, Subscription,
    Task, WeakEntity, Window, anchored, deferred, div, point, prelude::*, px,
};
use ide_core::DocId;
use ide_extension::ActivationEvent;
use ide_host::plugins::{CommandAnswer, WorkbenchEvent};
use ide_plugin::{InputMode, Invocation};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::Duration;

/// The extension points read here.
const MENUS_POINT: &str = "menus";
const SETTINGS_POINT: &str = "settings";
const PROVIDERS_POINT: &str = "providers";
/// The Tools menu's entries of plugins.
const TOOLS_MENU: &str = "tools";
/// The Background Tasks popup's action, the tasks status item's click.
const BACKGROUND_TASKS: &str = "view.backgroundTasks";
/// Debug Plugin Configuration: a plugin's debug request, under the debugger of the window.
const DEBUG_PLUGIN_START: &str = "debug.pluginStart";
/// The size of a gutter action's icon, and its distance from the fold chevrons' column.
const GUTTER_ICON: f32 = 12.0;
const GUTTER_INSET: f32 = 13.0;
/// The popup's corner keeps this far from the window's bottom right (above the status bar).
const TASKS_POPUP_INSET: (f32, f32) = (16.0, 32.0);
/// Why a run request is refused: the window has no project to run (or debug) it in, or it did not
/// start.
const RUN_UNAVAILABLE: &str = "Run service unavailable";
const DEBUGGER_UNAVAILABLE: &str = "Debugger unavailable";
pub(super) const RUN_NOT_STARTED: &str = "Run did not start";
const APPLY_FAILED: &str = "Could not apply settings.";

/// A context menu plugins add entries to, with the menu id a plugin declares for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginMenu {
    Editor,
    ProjectView,
    EditorTab,
    Problems,
    SearchResults,
}

impl PluginMenu {
    fn declared(self) -> &'static str {
        match self {
            Self::Editor => "editor.context",
            Self::ProjectView => "projectView.context",
            Self::EditorTab => "editorTab.context",
            Self::Problems => "problems.context",
            Self::SearchResults => "searchResults.context",
        }
    }
}

/// An item a plugin puts in a toolbar slot (`editor.title`, `mainToolbar`, `commit.message`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotItem {
    pub action: String,
    pub label: String,
    pub icon: Option<String>,
    pub when: Option<String>,
}

/// The `&'static str` the menus' action rows take for action `id`. Plugin actions are named at run
/// time: each id is kept once, for as long as the app runs (a plugin contributes a few).
pub(super) fn static_action_id(id: &str) -> &'static str {
    static IDS: LazyLock<Mutex<BTreeSet<&'static str>>> = LazyLock::new(|| Mutex::new(BTreeSet::new()));
    let mut ids = IDS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = ids.get(id) {
        return known;
    }
    let kept: &'static str = Box::leak(id.to_owned().into_boxed_str());
    ids.insert(kept);
    kept
}

/// The listed contributions of the live plugins.
fn listed_contributions(cx: &App) -> Vec<Contribution> {
    cx.try_global::<Plugins>().and_then(Plugins::snapshot).map(Contribution::from_snapshot).unwrap_or_default()
}

/// The name of plugin `plugin_id` as listed, else its id.
fn listed_name(plugin_id: &str, cx: &App) -> String {
    cx.try_global::<Plugins>()
        .and_then(Plugins::snapshot)
        .and_then(|listed| listed.plugins.iter().find(|plugin| plugin.id == plugin_id))
        .map_or_else(|| plugin_id.to_owned(), |plugin| plugin.name.clone())
}

/// Whether the `when` clause holds in `ctx`; a clause that does not parse does not.
fn holds(when: Option<&str>, ctx: &ContextKeys) -> bool {
    evaluate_when(when, ctx).unwrap_or(false)
}

/// The Tools menu's plugin entries, in the order the plugins are listed: each runs its command's
/// action, titled as the action is (else `<plugin>: <command>`). Whether one can run is the
/// action's own `when`.
pub(super) fn plugin_tool_items(cx: &App) -> Vec<DynamicItem> {
    let registry = cx.try_global::<ActionRegistry>();
    listed_contributions(cx)
        .iter()
        .filter(|c| c.point == MENUS_POINT && c.text("menu") == Some(TOOLS_MENU))
        .filter_map(|c| {
            let command = c.text("command")?;
            let action = plugin_action_id(&c.plugin_id, command);
            let label = registry
                .and_then(|registry| registry.get(&action))
                .map_or_else(|| format!("{}: {command}", listed_name(&c.plugin_id, cx)), |a| a.meta.title.clone());
            Some(DynamicItem { action, arg: None, label, checked: None })
        })
        .collect()
}

/// The items plugins put in toolbar slot `slot` (`editor.title`, `mainToolbar`, `commit.message`),
/// each with its `when` for the slot's host to judge.
pub fn plugin_slot_items(slot: &str, cx: &App) -> Vec<SlotItem> {
    let registry = cx.try_global::<ActionRegistry>();
    listed_contributions(cx)
        .iter()
        .filter(|c| c.point == MENUS_POINT && c.text("menu") == Some(slot))
        .filter_map(|c| {
            let command = c.text("command")?;
            let action = plugin_action_id(&c.plugin_id, command);
            let registered = registry.and_then(|registry| registry.get(&action));
            let label = registered
                .as_ref()
                .map_or_else(|| format!("{}: {command}", listed_name(&c.plugin_id, cx)), |a| a.meta.title.clone());
            let icon = registered.and_then(|a| a.meta.icon.clone());
            Some(SlotItem { action, label, icon, when: c.when().map(str::to_owned) })
        })
        .collect()
}

/// The plugin group of context menu `menu` for `paths` (with the caret's `position` in the editor's
/// menu), as `ctx` judges each entry's `when`: each entry runs its command's action, with the
/// menu's file as argument except in the editor's menu (which runs with the focused editor). A
/// plugin's entries naming one submenu are grouped under it; a group of one entry is that entry.
/// Unregistered commands are left out. Followed by a separator when not empty.
pub fn plugin_menu_items<C: Clone>(
    menu: PluginMenu,
    paths: &[String],
    position: Option<(u32, u32)>,
    ctx: &ContextKeys,
    cx: &App,
) -> Vec<MenuItem<C>> {
    let registry = cx.try_global::<ActionRegistry>();
    let arg = (menu != PluginMenu::Editor).then(|| {
        let mut arg = json!({ "path": paths.first(), "paths": paths });
        if let Some((line, column)) = position {
            arg["line"] = Value::from(line);
            arg["column"] = Value::from(column);
        }
        arg
    });
    let mut out: Vec<MenuItem<C>> = Vec::new();
    let mut groups: HashMap<(String, String), usize> = HashMap::new();
    for c in listed_contributions(cx).iter().filter(|c| c.point == MENUS_POINT) {
        if c.text("menu") != Some(menu.declared()) || !holds(c.when(), ctx) {
            continue;
        }
        let Some(command) = c.text("command") else { continue };
        let action = plugin_action_id(&c.plugin_id, command);
        if !registry.is_some_and(|registry| registry.contains(&action)) {
            continue;
        }
        let item = MenuItem::Action(ActionItem {
            action: static_action_id(&action),
            arg: arg.clone(),
            label: None,
            disabled: false,
        });
        match c.text("submenu").filter(|submenu| !submenu.is_empty()) {
            None => out.push(item),
            Some(submenu) => {
                let key = (c.plugin_id.clone(), submenu.to_owned());
                let at = *groups.entry(key).or_insert_with(|| {
                    out.push(MenuItem::submenu(submenu, Vec::new()));
                    out.len() - 1
                });
                if let MenuItem::Item(entry) = &mut out[at] {
                    entry.submenu.push(item);
                }
            }
        }
    }
    let mut out: Vec<MenuItem<C>> = out
        .into_iter()
        .map(|item| match item {
            MenuItem::Item(MenuEntry { mut submenu, .. }) if submenu.len() == 1 => submenu.remove(0),
            other => other,
        })
        .collect();
    if !out.is_empty() {
        out.push(MenuItem::Separator);
    }
    out
}

/// The Settings pages of the live plugins that declare settings, each plugin's page first.
pub(super) fn plugin_settings_pages(cx: &App) -> Vec<SettingsPage> {
    listed_contributions(cx)
        .iter()
        .filter(|c| c.point == SETTINGS_POINT)
        .filter_map(|c| Some(settings_pages(&c.plugin_id, &SettingsDecl::parse(&c.decl)?)))
        .flatten()
        .collect()
}

/// What the providers of the live plugins look like: a change asks every gutter again.
fn providers_fingerprint(cx: &App) -> Vec<(String, String)> {
    let Some(listed) = cx.try_global::<Plugins>().and_then(Plugins::snapshot) else { return Vec::new() };
    let mut providers: Vec<(String, String)> = listed
        .ui
        .iter()
        .filter(|item| item.point == PROVIDERS_POINT)
        .map(|item| {
            let status = listed.plugins.iter().find(|plugin| plugin.id == item.plugin_id).map(|plugin| plugin.status);
            (item.plugin_id.clone(), format!("{}|{status:?}", item.decl))
        })
        .collect();
    providers.sort();
    providers
}

/// The gutter of one editor.
struct EditorGutter {
    editor: WeakEntity<Editor>,
    doc: DocId,
    visuals: LanguageVisuals,
    /// The pending request, after the debounce.
    timer: Option<Task<()>>,
    fetch: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// What the window keeps of plugin contributions.
#[derive(Default)]
pub(super) struct PluginContributions {
    gutters: HashMap<EntityId, EditorGutter>,
    /// The editor providers last seen, and what follows the listing for them.
    providers: Vec<(String, String)>,
    providers_observer: Option<Subscription>,
    icons: MarkerIcons,
    /// Languages whose activation event was fired, and the file last reported.
    activated_languages: BTreeSet<String>,
    activated_path: Option<String>,
    /// The request ids of the plugin debug runs under way that a plugin waits on, by operation.
    debug_requests: HashMap<String, String>,
    /// The plugin debug runs whose JVM the Java debugger attaches to once it announces its port.
    pub(super) java_debug: super::plugin_java_debug::JavaDebugRuns,
}

/// The tasks status item's click asks for a toggle (`{ "toggle": true }`); the action itself only opens the
/// popup.
fn call_toggles(arg: Option<&Value>) -> bool {
    arg.and_then(|arg| arg.get("toggle")).and_then(Value::as_bool).unwrap_or(false)
}

/// Registers the Background Tasks popup's action and Debug Plugin Configuration.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(BACKGROUND_TASKS, |call, window, cx| {
        let toggle = call_toggles(call.arg);
        on_workbench(window, cx, move |workbench, window, cx| {
            if toggle || workbench.background_tasks_popup().is_none() {
                workbench.toggle_background_tasks(window, cx);
            }
        })
    });
    // Arg: `{ config, operation }`; the plugin of `operation` hears how the session went.
    registry.handle(DEBUG_PLUGIN_START, |call, window, cx| {
        let arg = call.arg.cloned();
        on_workbench(window, cx, move |workbench, window, cx| {
            workbench.run_debug_action(DEBUG_PLUGIN_START, arg.as_ref(), window, cx);
        })
    });
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // Run requests

    /// A plugin asks the window to run (or debug) `config` for its operation `operation`; a
    /// `request_id` (none for `run.start`) is answered once the run started or failed to.
    pub(super) fn plugin_run_request(
        &mut self,
        request_id: &str,
        operation: &str,
        config: &ide_run::RunConfig,
        debug: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if debug {
            // A detector that starts its JVM with a waiting JDWP agent runs in Run; the Java
            // debugger attaches to the port the JVM announces.
            if self.plugin_java_debug_request(request_id, operation, config, window, cx) {
                return;
            }
            return self.plugin_debug_request(request_id, operation, config, window, cx);
        }
        let Some(driver) = self.run_driver().cloned() else {
            return self.refuse_plugin_run(request_id, operation, RUN_UNAVAILABLE, cx);
        };
        let started = driver.update(cx, |driver, cx| driver.start(config.clone(), Default::default(), cx));
        let store = driver.read(cx).store().clone();
        let (request, operation) = (request_id.to_owned(), operation.to_owned());
        cx.spawn(async move |workbench, cx| {
            let session = started.await;
            let _ = workbench.update(cx, |workbench, cx| {
                let session = session.and_then(|id| store.read(cx).session(&id).cloned());
                match session {
                    Some(session) if session.status != SessionStatus::Failed => {
                        // A session that ended without a process (nothing to launch) is over already.
                        if session.run_id.is_none() && session.status == SessionStatus::Finished {
                            workbench.finish_plugin_operation(&operation, session.exit_code, session.stopped, cx);
                        }
                        if !request.is_empty() {
                            answer_plugin_command(&workbench.label, &request, CommandAnswer::Value(None), cx);
                        }
                    }
                    _ => workbench.refuse_plugin_run(&request, &operation, RUN_NOT_STARTED, cx),
                }
            });
        })
        .detach();
    }

    /// A run request that is not carried out: the plugin hears that its run ended without a status,
    /// and why when it waits for the answer.
    pub(super) fn refuse_plugin_run(
        &mut self,
        request_id: &str,
        operation: &str,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        tracing::warn!("a plugin's run was not started: {reason}");
        self.finish_plugin_operation(operation, None, false, cx);
        if !request_id.is_empty() {
            answer_plugin_command(&self.label, request_id, CommandAnswer::Failed(reason.to_owned()), cx);
        }
    }

    fn finish_plugin_operation(&self, operation: &str, status: Option<i32>, stopped: bool, cx: &App) {
        if let Err(reason) = self.plugin_call_allowed("plugin_finish_operation", cx) {
            tracing::warn!("a plugin's run is not reported here: {reason}");
            return;
        }
        if let Some(host) = plugin_host(cx)
            && !host.finish_operation(&self.label, operation, status, stopped)
        {
            tracing::debug!("the plugin's run {operation} was reported already");
        }
    }

    /// A plugin asks to debug `config` for its operation `operation`: Debug Plugin Configuration
    /// runs once this update is over. A start that cannot run ends the plugin's run without a
    /// status; how the session went reaches the plugin through [`Self::plugin_debug_sessions`].
    fn plugin_debug_request(
        &mut self,
        request_id: &str,
        operation: &str,
        config: &ide_run::RunConfig,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let config = match serde_json::to_value(config) {
            Ok(config) => config,
            Err(error) => return self.refuse_plugin_run(request_id, operation, &error.to_string(), cx),
        };
        if !request_id.is_empty() {
            self.plugin_contributions.debug_requests.insert(operation.to_owned(), request_id.to_owned());
        }
        let arg = json!({ "config": config, "operation": operation });
        let (request, operation) = (request_id.to_owned(), operation.to_owned());
        let workbench = cx.weak_entity();
        window.defer(cx, move |window, cx| {
            let Err(error) = execute_action_or_throw(DEBUG_PLUGIN_START, Some(&arg), window, cx) else { return };
            let reason = match error {
                ActionError::Unavailable(_) => DEBUGGER_UNAVAILABLE.to_owned(),
                failed => failed.to_string(),
            };
            let _ = workbench.update(cx, |workbench, cx| {
                workbench.plugin_contributions.debug_requests.remove(&operation);
                workbench.refuse_plugin_run(&request, &operation, &reason, cx);
            });
        });
    }

    /// What the debugger tells the plugins about the sessions they started: a start that failed
    /// ends the plugin's run without a status; an ended session ends it with the session's exit
    /// code and whether the user stopped it.
    pub(super) fn plugin_debug_sessions(&mut self, events: Vec<DebugPluginEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                DebugPluginEvent::Failed { operation, message } => {
                    let request = self.plugin_contributions.debug_requests.remove(&operation).unwrap_or_default();
                    self.refuse_plugin_run(&request, &operation, &message, cx);
                }
                DebugPluginEvent::Exited { operation, code, stopped, .. } => {
                    if let Some(request) = self.plugin_contributions.debug_requests.remove(&operation) {
                        answer_plugin_command(&self.label, &request, CommandAnswer::Value(None), cx);
                    }
                    self.finish_plugin_operation(&operation, code, stopped, cx);
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Gutter actions

    /// Follows `editor` for its plugin gutter actions: asked for once its text rests, again after
    /// each edit and when the editor providers change.
    pub(super) fn watch_plugin_gutter(&mut self, editor: &Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let id = editor.entity_id();
        if self.plugin_contributions.gutters.contains_key(&id) || !cx.has_global::<Plugins>() {
            return;
        }
        if self.plugin_contributions.providers_observer.is_none() {
            self.plugin_contributions.providers = providers_fingerprint(cx);
            let observer =
                cx.observe_global_in::<Plugins>(window, |workbench, _, cx| workbench.plugin_providers_listed(cx));
            self.plugin_contributions.providers_observer = Some(observer);
        }
        let subscriptions = vec![
            cx.subscribe(editor, move |workbench, _, event: &EditorEvent, cx| {
                if let EditorEvent::Edited { .. } = event {
                    workbench.plugin_gutter_effects(id, |visuals| visuals.doc_changed(), cx);
                }
            }),
            cx.observe_release(editor, move |workbench, _, cx| {
                workbench.plugin_contributions.gutters.remove(&id);
                cx.notify();
            }),
        ];
        let (visuals, effects) = LanguageVisuals::new();
        let gutter = EditorGutter {
            editor: editor.downgrade(),
            doc: editor.read(cx).doc(),
            visuals,
            timer: None,
            fetch: None,
            _subscriptions: subscriptions,
        };
        self.plugin_contributions.gutters.insert(id, gutter);
        self.apply_gutter_effects(id, effects, cx);
    }

    /// The plugins were listed again: when their editor providers changed, every gutter's actions
    /// go at once and are asked for again.
    fn plugin_providers_listed(&mut self, cx: &mut Context<Self>) {
        let providers = providers_fingerprint(cx);
        if providers == self.plugin_contributions.providers {
            return;
        }
        self.plugin_contributions.providers = providers;
        let ids: Vec<EntityId> = self.plugin_contributions.gutters.keys().copied().collect();
        for id in ids {
            self.plugin_gutter_effects(id, LanguageVisuals::providers_changed, cx);
        }
        cx.notify();
    }

    /// The tabs changed: the focused editor's gutter, skipped while it was hidden, is asked again.
    pub(super) fn plugin_gutter_shown(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.editor().map(Entity::entity_id) {
            self.plugin_gutter_effects(id, LanguageVisuals::shown_again, cx);
        }
    }

    fn plugin_gutter_effects(
        &mut self,
        id: EntityId,
        change: impl FnOnce(&mut LanguageVisuals) -> Vec<VisualsEffect>,
        cx: &mut Context<Self>,
    ) {
        let Some(gutter) = self.plugin_contributions.gutters.get_mut(&id) else { return };
        let effects = change(&mut gutter.visuals);
        self.apply_gutter_effects(id, effects, cx);
        cx.notify();
    }

    fn apply_gutter_effects(&mut self, id: EntityId, effects: Vec<VisualsEffect>, cx: &mut Context<Self>) {
        for effect in effects {
            match effect {
                VisualsEffect::Schedule(ms) => {
                    let timer = cx.spawn(async move |workbench, cx| {
                        cx.background_executor().timer(Duration::from_millis(ms)).await;
                        let _ = workbench.update(cx, |workbench, cx| workbench.plugin_gutter_due(id, cx));
                    });
                    if let Some(gutter) = self.plugin_contributions.gutters.get_mut(&id) {
                        gutter.timer = Some(timer);
                    }
                }
                VisualsEffect::Fetch(ticket) => self.fetch_plugin_visuals(id, ticket, cx),
            }
        }
    }

    /// The debounce elapsed: the visuals of the document as it is now are asked for.
    fn plugin_gutter_due(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(gutter) = self.plugin_contributions.gutters.get_mut(&id) else { return };
        gutter.timer = None;
        let Some(editor) = gutter.editor.upgrade() else { return };
        let version = u64::from(editor.read(cx).version());
        let effects = gutter.visuals.run(version);
        self.apply_gutter_effects(id, effects, cx);
    }

    /// Asks the backend for the plugin visuals of the editor's document, off the main thread.
    fn fetch_plugin_visuals(&mut self, id: EntityId, ticket: VisualsTicket, cx: &mut Context<Self>) {
        let allowed = self.plugin_call_allowed("plugin_language_visuals", cx);
        let Some(gutter) = self.plugin_contributions.gutters.get(&id) else { return };
        let doc = gutter.doc;
        let label = self.label.clone();
        let call = match (allowed, cx.try_global::<Backend>()) {
            (Ok(()), Some(backend)) => Some(backend.with_host(move |host| async move {
                ide_host::ops::lsp::plugin_language_visuals(&host, &label, doc).await
            })),
            _ => None,
        };
        let fetch = cx.spawn(async move |workbench, cx| {
            let result = match call {
                Some(call) => match call.await {
                    Ok(answer) => answer.map_err(|error| error.message),
                    Err(error) => Err(error.to_string()),
                },
                None => Err("plugin visuals are not asked for in this window".to_owned()),
            };
            let _ = workbench.update(cx, |workbench, cx| workbench.plugin_visuals_answered(id, ticket, result, cx));
        });
        if let Some(gutter) = self.plugin_contributions.gutters.get_mut(&id) {
            gutter.fetch = Some(fetch);
        }
    }

    fn plugin_visuals_answered(
        &mut self,
        id: EntityId,
        ticket: VisualsTicket,
        result: Result<ide_host::ops::lsp::PluginLanguageVisuals, String>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = &result {
            tracing::debug!("plugin gutter actions: {error}");
        }
        let visible = self.editor().is_some_and(|editor| editor.entity_id() == id);
        let contributions = &mut self.plugin_contributions;
        let Some(gutter) = contributions.gutters.get_mut(&id) else { return };
        gutter.fetch = None;
        let Some(editor) = gutter.editor.upgrade() else { return };
        let (version, lines) = {
            let editor = editor.read(cx);
            (u64::from(editor.version()), editor.line_count() as usize)
        };
        if gutter.visuals.answered(ticket, version, lines, visible, result, &contributions.icons) {
            cx.notify();
        }
    }

    /// The gutter actions shown beside `editor`, one marker per line.
    pub fn plugin_gutter_markers(&self, editor: &Entity<Editor>) -> Vec<ActionMarker> {
        let gutter = self.plugin_contributions.gutters.get(&editor.entity_id());
        gutter.map(|gutter| gutter.visuals.markers().to_vec()).unwrap_or_default()
    }

    /// The icons of the focused editor's gutter actions, over the window at their lines (only the
    /// lines the editor painted, inside it).
    pub(super) fn plugin_gutter_layer(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(editor) = self.editor().cloned() else { return Vec::new() };
        let id = editor.entity_id();
        let markers = self.plugin_gutter_markers(&editor);
        // Distraction Free Mode draws no gutter.
        if markers.is_empty() || super::distraction_free_active(cx) {
            return Vec::new();
        }
        let (places, line_height, fold_column) = {
            let editor = editor.read(cx);
            let Some(painted) = editor.painted_lines() else { return Vec::new() };
            let line_height = editor.line_height(cx);
            let fold_column = editor.fold_column_width(cx);
            let first = painted.start;
            // The editor's top: where its first painted row would start unscrolled.
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * editor.row_of_line(first) as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * editor.painted_rows().saturating_sub(1) as f32);
            let places: Vec<(usize, Point<Pixels>)> = markers
                .iter()
                .enumerate()
                .filter(|(_, marker)| painted.contains(&(marker.line as u32)))
                .filter_map(|(index, marker)| {
                    let at = editor.position_of(editor.offset_of(marker.line as u32, 0))?;
                    let inside =
                        top.is_some_and(|top| at.y >= top) && bottom.is_some_and(|bottom| at.y + line_height <= bottom);
                    inside.then_some((index, at))
                })
                .collect();
            (places, line_height, fold_column)
        };
        let color = theme::color(cx, "fg-muted");
        places
            .into_iter()
            .map(|(index, at)| {
                let marker = &markers[index];
                let line = marker.line;
                // Left of the fold chevrons' column.
                let corner = point(at.x - fold_column - px(GUTTER_INSET), at.y + (line_height - px(GUTTER_ICON)) / 2.0);
                let icon = div()
                    .id(SharedString::from(format!("plugin-gutter-{line}")))
                    .debug_selector(move || format!("plugin-gutter:{line}"))
                    .with_tooltip(marker.title.clone())
                    .size(px(GUTTER_ICON))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .occlude()
                    .child(theme::icon(cx, &marker.icon, GUTTER_ICON, color))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                            // The press is the icon's: the editor neither moves its caret nor
                            // toggles a breakpoint.
                            window.prevent_default();
                            cx.stop_propagation();
                            workbench.plugin_gutter_clicked(id, line, event.position, window, cx);
                        }),
                    );
                deferred(anchored().position(corner).child(icon)).into_any_element()
            })
            .collect()
    }

    /// The icon of `line` was clicked: its single action runs, several open a menu at `position`.
    fn plugin_gutter_clicked(
        &mut self,
        id: EntityId,
        line: usize,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(gutter) = self.plugin_contributions.gutters.get(&id) else { return };
        let Some(marker) = gutter.visuals.markers().iter().find(|marker| marker.line == line).cloned() else { return };
        let doc = gutter.doc;
        match marker.click() {
            None => {}
            Some(ActionClick::Execute(_)) => {
                if let Some(action) = marker.actions.first() {
                    self.run_gutter_action(doc, action, cx);
                }
            }
            Some(ActionClick::Menu(_)) => {
                let items: Vec<MenuItem<LineAction>> = marker
                    .actions
                    .iter()
                    .map(|action| {
                        let item = MenuItem::item(action.action.tooltip.clone(), action.clone());
                        // The menu's icons are named ahead of time: the two a gutter action uses.
                        match action.menu_icon() {
                            Some("run") => item.icon("run"),
                            Some("debug") => item.icon("debug"),
                            _ => item,
                        }
                    })
                    .collect();
                self.show_menu(
                    &items,
                    position,
                    move |workbench, action: LineAction, _, cx| workbench.run_gutter_action(doc, &action, cx),
                    window,
                    cx,
                );
            }
        }
    }

    /// Runs a gutter action's command in its plugin, through the document's language features.
    fn run_gutter_action(&mut self, doc: DocId, action: &LineAction, cx: &mut Context<Self>) {
        if let Err(reason) = self.plugin_call_allowed("lsp_execute_command", cx) {
            tracing::warn!("a plugin's gutter action does not run here: {reason}");
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (label, command) = (self.label.clone(), action.command());
        let ran = backend.with_host(move |host| async move {
            ide_host::ops::lsp::lsp_execute_command(&host, &label, doc, command).await
        });
        cx.background_executor()
            .spawn(async move {
                match ran.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => tracing::warn!("a plugin's gutter action failed: {}", error.message),
                    Err(error) => tracing::warn!("a plugin's gutter action failed: {error}"),
                }
            })
            .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Settings pages

    /// Gives the Settings dialog a view for each plugin settings page it lists; what their actions
    /// ask is carried out here. The subscriptions live as long as the dialog.
    pub(super) fn mount_plugin_settings_pages(
        &mut self,
        dialog: &Entity<SettingsDialogView>,
        has_project: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Subscription> {
        let model = dialog.read(cx).model().clone();
        let mut subscriptions = Vec::new();
        for c in listed_contributions(cx).into_iter().filter(|c| c.point == SETTINGS_POINT) {
            let Some(decl) = SettingsDecl::parse(&c.decl) else {
                tracing::warn!("the settings of plugin {} are not readable here", c.plugin_id);
                continue;
            };
            let plugin_id = c.plugin_id.clone();
            let page_id = settings_page_id(&plugin_id);
            let mut views = Vec::new();
            if decl.pages.is_empty() {
                let (model, properties) = (model.clone(), decl.properties.clone());
                let view = cx.new(|cx| {
                    PluginSettingsPage::fields(model, &page_id, &plugin_id, properties, Vec::new(), has_project, cx)
                });
                views.push((page_id.clone(), view));
            } else {
                let links: Vec<(String, String)> = decl
                    .pages
                    .iter()
                    .map(|page| (declared_page_id(&plugin_id, &page.id), page.title.clone()))
                    .collect();
                let model_for_index = model.clone();
                let index = cx
                    .new(|cx| PluginSettingsPage::index(model_for_index, &page_id, &plugin_id, links, has_project, cx));
                views.push((page_id.clone(), index));
                for page in decl.pages {
                    let id = declared_page_id(&plugin_id, &page.id);
                    let model = model.clone();
                    let view = cx.new(|cx| {
                        PluginSettingsPage::fields(
                            model,
                            &id,
                            &plugin_id,
                            page.properties,
                            page.actions,
                            has_project,
                            cx,
                        )
                    });
                    views.push((id, view));
                }
            }
            for (id, view) in views {
                dialog.update(cx, |dialog, cx| dialog.set_page_view(&id, view.clone().into(), cx));
                let (weak_dialog, plugin) = (dialog.downgrade(), plugin_id.clone());
                subscriptions.push(cx.subscribe_in(
                    &view,
                    window,
                    move |workbench, page, event: &PluginSettingsEvent, window, cx| {
                        let PluginSettingsEvent::RunAction { index, command, scope } = event;
                        let action =
                            SettingsActionRun { plugin: plugin.clone(), command: command.clone(), index: *index };
                        workbench.run_plugin_settings_action(
                            &weak_dialog,
                            page.downgrade(),
                            action,
                            *scope,
                            window,
                            cx,
                        );
                    },
                ));
            }
        }
        subscriptions
    }

    /// A page action: what is pending is applied first; once it is saved the plugin's command runs
    /// with the settings of `scope`, and the page shows how that went.
    fn run_plugin_settings_action(
        &mut self,
        dialog: &WeakEntity<SettingsDialogView>,
        page: WeakEntity<PluginSettingsPage>,
        action: SettingsActionRun,
        scope: Scope,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dialog) = dialog.upgrade() else { return };
        dialog.update(cx, |dialog, cx| dialog.apply(cx));
        // The Apply is carried out when its event is: the outcome is read once that is over.
        cx.defer_in(window, move |workbench, _, cx| {
            let failed = {
                let model = dialog.read(cx).model().read(cx);
                model.saving || model.error.is_some()
            };
            if failed {
                let _ =
                    page.update(cx, |page, cx| page.action_finished(action.index, Err(APPLY_FAILED.to_owned()), cx));
                return;
            }
            let running = workbench.run_plugin_command_with_settings(&action.plugin, &action.command, scope, cx);
            cx.spawn(async move |_, cx| {
                let result = running.await.map(|_| ()).map_err(|error| error.message);
                let _ = page.update(cx, |page, cx| page.action_finished(action.index, result, cx));
            })
            .detach();
        });
    }

    /// Runs `command` of `plugin` off the main thread with the settings of `scope` (the global
    /// ones, or the project's), as a settings page's action does: no editor, no paths.
    fn run_plugin_command_with_settings(
        &self,
        plugin: &str,
        command: &str,
        scope: Scope,
        cx: &mut Context<Self>,
    ) -> Task<ide_core::IdeResult<Option<String>>> {
        let allowed = match cx.try_global::<Backend>() {
            Some(backend) => backend.host.authorize_command(&self.label, "plugin_run_command"),
            None => Err(ide_core::IdeError::internal("the backend is not running")),
        };
        let host = plugin_host(cx);
        let invocation = Invocation {
            command_chain: Vec::new(),
            client: Some(self.label.clone()),
            workspace_root: self.project_root(cx),
            editor: None,
            selected_paths: Vec::new(),
            global_settings: scope == Scope::Global,
        };
        let (plugin, command) = (plugin.to_owned(), command.to_owned());
        crate::plugins::host_blocking(cx, move || {
            allowed?;
            let host = host.ok_or_else(|| ide_core::IdeError::internal("the plugin host is not running"))?;
            host.run_command(&plugin, &command, None, invocation)
        })
    }

    // ---------------------------------------------------------------------------------------------
    // Editor and activation events

    /// Document `doc` of the window was saved: plugins that want `file-saved` hear its path.
    pub(super) fn plugin_document_saved(&mut self, doc: DocId, cx: &mut Context<Self>) {
        let path = self.editors().into_iter().find(|editor| editor.read(cx).doc() == doc);
        let Some(path) = path.map(|editor| editor.read(cx).path().to_string_lossy().into_owned()) else { return };
        self.post_plugin_event(WorkbenchEvent::FileSaved(path), cx);
    }

    /// The input mode of the editor of `path` changed (Vim's `NORMAL`, `INSERT`, `VISUAL` with its
    /// sub-mode): plugins that want `input-mode-changed` hear it.
    pub fn plugin_input_mode_changed(&mut self, path: &str, mode: &str, sub: Option<&str>, cx: &mut Context<Self>) {
        let mode = InputMode { path: path.to_owned(), mode: mode.to_owned(), sub: sub.map(str::to_owned) };
        self.post_plugin_event(WorkbenchEvent::InputModeChanged(mode), cx);
    }

    /// The focused editor changed: its language's activation event fires the first time that
    /// language is seen in the window, and its project-relative path's activation event each time
    /// another file is focused. Nothing fires for a project in Restricted Mode.
    pub(super) fn plugin_activation_events(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.editor().cloned() else {
            self.plugin_contributions.activated_path = None;
            return;
        };
        let (path, language) = {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.language().0.clone())
        };
        if self.plugin_contributions.activated_path.as_deref() == Some(path.as_str()) {
            return;
        }
        self.plugin_contributions.activated_path = Some(path.clone());
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let project = backend.host.project(&self.label).ok();
        if project.as_ref().is_some_and(|project| !project.is_trusted()) {
            return;
        }
        if let Err(reason) = self.plugin_call_allowed("plugin_activation_event", cx) {
            tracing::debug!("activation events are not fired here: {reason}");
            return;
        }
        let Some(host) = plugin_host(cx) else { return };
        let root = project.map(|project| project.root.clone());
        let mut events = Vec::new();
        if self.plugin_contributions.activated_languages.insert(language.clone()) {
            events.push(ActivationEvent::Language(language));
        }
        let relative = root.as_ref().and_then(|root| std::path::Path::new(&path).strip_prefix(root).ok());
        if let Some(relative) = relative.filter(|relative| !relative.as_os_str().is_empty()) {
            events.push(ActivationEvent::File(relative.to_string_lossy().replace('\\', "/")));
        }
        cx.background_executor()
            .spawn(async move {
                for event in events {
                    host.activation_event(&event, root.as_deref());
                }
            })
            .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Background Tasks

    /// The Background Tasks popup, while it is open.
    pub fn background_tasks_popup(&self) -> Option<Entity<BackgroundTasksPopup>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    /// View › Background Tasks (the tasks status item's click): opens the popup over the status
    /// bar's right end, or closes it when it is open.
    pub fn toggle_background_tasks(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.background_tasks_popup().is_some() {
            self.close_popup(window, cx);
            return;
        }
        let size = window.viewport_size();
        let corner = point(size.width - px(TASKS_POPUP_INSET.0), size.height - px(TASKS_POPUP_INSET.1));
        let tasks = self.tasks.tasks().to_vec();
        let popup = cx.new(|cx| BackgroundTasksPopup::new(tasks, corner, cx));
        let workbench = cx.entity();
        popup.update(cx, |popup, cx| {
            popup.follow(&workbench, |workbench: &Workbench| workbench.tasks().tasks().to_vec(), cx)
        });
        let events =
            cx.subscribe_in(&popup, window, |workbench, _, event: &BackgroundTasksEvent, window, cx| match event {
                BackgroundTasksEvent::Cancel(id) => workbench.cancel_background_task(id, cx),
                BackgroundTasksEvent::Dismissed => workbench.close_popup(window, cx),
            });
        self.open_popup(&popup, events, window, cx);
    }
}

/// A settings page's action on its way: the plugin, its command and the button's index.
#[derive(Clone, Debug)]
struct SettingsActionRun {
    plugin: String,
    command: String,
    index: usize,
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::super::plugins_window::test_support::{PluginHome, program, programs_in, settle_app};
    use super::super::plugins_window::test_support::{
        copy_dir, link, plugin_copy, start_with_plugins, trusted_project,
    };
    use super::super::tests::{click_on, notice_titled, run_action, wait_long};
    use super::*;
    use crate::editor::marks::{Mark, MarkLayer};
    use crate::platform::actions::{ActionSource, execute_action_from};
    use crate::platform::context::{ContextValue, window_context};
    use crate::run::view::RunView;
    use crate::ui::popup_menu::PopupMenu;
    use gpui_kit::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};
    use ide_host::plugins::PluginStatus;
    use std::path::{Path, PathBuf};

    const LABEL: &str = "w1";

    /// A second copy of the plugin folder `first`, at `<home>/fixtures/<copy>`, its manifest rewritten
    /// by `edit`: two plugins from one fixture.
    fn second_copy(home: &Path, first: &Path, copy: &str, edit: impl Fn(&str) -> String) -> PathBuf {
        let target = home.join("fixtures").join(copy);
        copy_dir(first, &target);
        let manifest = target.join("plugin.toml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest is readable");
        std::fs::write(&manifest, edit(&text)).expect("the manifest is written");
        target
    }

    /// Runs `command` of `plugin` for the window, off the main thread, and waits for its answer.
    fn run_in_window(
        cx: &mut VisualTestContext,
        root: &Path,
        plugin: &str,
        command: &str,
        arg: Option<&str>,
    ) -> ide_core::IdeResult<Option<String>> {
        let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
        let invocation = Invocation {
            command_chain: Vec::new(),
            client: Some(LABEL.to_owned()),
            workspace_root: Some(root.to_path_buf()),
            editor: None,
            selected_paths: Vec::new(),
            global_settings: false,
        };
        let (plugin, command, arg) = (plugin.to_owned(), command.to_owned(), arg.map(str::to_owned));
        let running = std::thread::spawn(move || host.run_command(&plugin, &command, arg.as_deref(), invocation));
        wait_long(cx, "the command answers", |_| running.is_finished());
        running.join().expect("the command ran")
    }

    /// The `kind`-`data` events the fixture stored, as its `events` command lists them.
    fn stored_events(cx: &mut VisualTestContext, root: &Path, plugin: &str) -> Vec<Value> {
        let listed = run_in_window(cx, root, plugin, "events", None).ok().flatten();
        listed.and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok()).unwrap_or_default()
    }

    /// Whether the `execution` fixture heard its run exit with `status`, not killed.
    #[cfg(unix)]
    fn heard_exit(cx: &mut VisualTestContext, root: &Path, status: Value) -> bool {
        stored_events(cx, root, "execution").iter().any(|event| {
            event["kind"] == "run-exited"
                && serde_json::from_str::<Value>(event["data"].as_str().unwrap_or("{}"))
                    .is_ok_and(|data| data["status"] == status && data["killed"] == false)
        })
    }

    fn context(key: &str, cx: &mut VisualTestContext) -> Option<ContextValue> {
        cx.update(|window, cx| window_context(window, cx).get(key).cloned())
    }

    fn status_of(plugin: &str, cx: &mut VisualTestContext) -> Option<PluginStatus> {
        let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
        host.list(None).plugins.into_iter().find(|p| p.id == plugin).map(|p| p.status)
    }

    /// A click on the element drawn with `selector`, named at run time.
    fn click(cx: &mut VisualTestContext, selector: &str) {
        click_on(cx, Box::leak(selector.to_owned().into_boxed_str()));
    }

    /// What a click in the gutter could change: the selections, the main one, the breakpoints.
    fn editor_state(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> (Vec<(u32, u32)>, usize, Vec<Mark>) {
        editor.read_with(cx, |editor, _| {
            (editor.selections().to_vec(), editor.main_index(), editor.marks(MarkLayer::Breakpoints).to_vec())
        })
    }

    fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) -> Entity<Editor> {
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        wait_long(cx, "the file opens", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path.as_path()))
        });
        workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
    }

    /// A linked copy of the `execution` fixture (its manifest rewritten by `edit`) granted its
    /// `execution-tool`, a script in the home's `bin` that prints its arguments and exits with 3, and
    /// a trusted project of `files` with the detector's marker.
    #[cfg(unix)]
    fn execution_project<'a>(
        cx: &'a mut TestAppContext,
        home: &Path,
        edit: impl Fn(&str) -> String,
        files: &[(&str, &str)],
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
        let bin = home.join("bin");
        program(&bin, "execution-tool", "#!/bin/sh\necho \"execution-tool $*\"\nexit 3\n");
        programs_in(&bin, cx);
        let folder = plugin_copy(home, "execution", edit);
        link(cx, &folder, &["process:execution-tool"], "run");
        let mut files = files.to_vec();
        files.push(("execution.marker", ""));
        trusted_project(cx, LABEL, &files)
    }

    /// A temporary home whose languages include `fakedap` (`*.fake` files), debugged by the fake
    /// adapter of the debugger's tests, with the backend and the plugin host running in it. The
    /// backend reads the languages when it starts, so they are written first.
    #[cfg(unix)]
    fn start_with_fake_debugger(cx: &mut TestAppContext) -> PluginHome {
        let hosts = crate::plugins::lock_test_hosts();
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        let dirs = crate::backend::AppDirs::under(home.path());
        let script = home.path().join("fake_dap.py");
        std::fs::write(&script, super::super::debug::tests::FAKE_ADAPTER).expect("the adapter script is written");
        let languages = dirs.config.join("languages");
        std::fs::create_dir_all(&languages).expect("the languages folder is made");
        let toml = format!(
            "id = \"fakedap\"\nname = \"Fake\"\nextensions = [\"fake\"]\nline-comment = \"#\"\n\n\
             [debugger]\nadapter = \"fake-dap\"\ncommand = \"python3\"\nargs = [{:?}]\n",
            script.to_string_lossy(),
        );
        std::fs::write(languages.join("fakedap.toml"), toml).expect("the language is written");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(crate::platform::storage::UiStorage::default());
            crate::platform::input::init(cx);
            crate::editor::actions::register(cx);
            crate::workbench::register_actions(cx);
            Backend::init(dirs, cx).expect("the backend starts");
            crate::workbench::menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            theme::appearance::init(cx);
            Plugins::init(cx).expect("the plugin host starts");
        });
        settle_app(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
        PluginHome::new(home, hosts)
    }

    // ---------------------------------------------------------------------------------------------
    // Runs

    /// A plugin's run (`run.start` of its test detector) runs in the Run tool window: its tab shows
    /// the program's output and exit code, and the plugin hears that it exited with that status.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_plugins_run_shows_its_output_in_the_run_tool_window_and_the_plugin_hears_its_exit(
        cx: &mut TestAppContext,
    ) {
        let home = start_with_plugins(cx);
        let (_folder, root, workbench, cx) = execution_project(cx, home.path(), str::to_owned, &[]);
        run_action(cx, &plugin_action_id("execution", "run"), Some(Value::from("detected")));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        let session = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "Detected test").cloned())
        };
        wait_long(cx, "the run ends", |cx| session(cx).is_some_and(|s| !s.is_live()));
        let ended = session(cx).expect("a tab");
        assert_eq!((ended.status, ended.exit_code, ended.stopped), (SessionStatus::Finished, Some(3), false));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(super::super::RUN_TOOL_WINDOW)));
        let view = workbench
            .read_with(cx, |w, _| w.tool_window_view(super::super::RUN_TOOL_WINDOW))
            .and_then(|view| view.downcast::<RunView>().ok())
            .expect("the Run tool window holds its view");
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx)), ["Detected test"]);
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().any(|line| line.contains("execution-tool test appended")), "{lines:?}");
        wait_long(cx, "the plugin hears the exit", |cx| heard_exit(cx, &root, Value::from(3)));
    }

    /// A plugin asks to debug its configuration, whose program the project's `main.fake` names:
    /// Debug Plugin Configuration has the plugin prepare it, the session runs under the fake
    /// adapter in the Debug tool window (not in Run), and when it ends the plugin hears its run
    /// exited with the session's exit code.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_plugins_debug_request_runs_under_the_debugger_and_the_plugin_hears_the_end(cx: &mut TestAppContext) {
        let home = start_with_fake_debugger(cx);
        let runs_main_fake = |text: &str| text.replace("args = [\"test\"]", "args = [\"main.fake\"]");
        let program = [("main.fake", "start\nend\n")];
        let (_folder, root, workbench, cx) = execution_project(cx, home.path(), runs_main_fake, &program);
        run_action(cx, &plugin_action_id("execution", "debug"), Some(Value::from("detected")));
        wait_long(cx, "the session shows in the Debug tool window", |cx| {
            let view = workbench.read_with(cx, |w, _| w.debug_window_view().cloned());
            view.is_some_and(|view| view.update(cx, |view, cx| view.tab_titles(cx)) == ["Detected test"])
        });
        wait_long(cx, "the plugin hears the end", |cx| heard_exit(cx, &root, Value::from(0)));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        assert!(driver.read_with(cx, |driver, cx| driver.store().read(cx).sessions.is_empty()), "nothing ran in Run");
    }

    /// Debug Plugin Configuration runs only while a project is open, as its condition says: without
    /// one the registry refuses it, with one it runs (and, given no configuration, the plugin's
    /// start fails as the debugger reports it).
    #[gpui_kit::test]
    async fn debug_plugin_configuration_runs_only_while_a_project_is_open(cx: &mut TestAppContext) {
        use crate::platform::actions::{ActionError, execute_action_or_throw};
        let _home = start_with_plugins(cx);
        let arg = json!({ "operation": "op-1" });
        {
            let (_empty, cx) = cx.add_window_view(|window, cx| Workbench::new("w0", window, cx));
            let refused = cx.update(|window, cx| execute_action_or_throw(DEBUG_PLUGIN_START, Some(&arg), window, cx));
            assert!(matches!(refused, Err(ActionError::Unavailable(_))), "no project: refused");
        }
        let (_folder, _root, _workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
        let ran = cx.update(|window, cx| execute_action_or_throw(DEBUG_PLUGIN_START, Some(&arg), window, cx));
        assert!(ran.is_ok(), "a project is open: it runs");
    }

    /// A plugin's run detector and its dynamic configurations are listed with the project's run
    /// configurations, in the configuration chooser.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_plugins_run_detectors_are_listed_in_the_configuration_chooser(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let (_folder, _root, workbench, cx) = execution_project(cx, home.path(), str::to_owned, &[]);
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        wait_long(cx, "the configurations are listed", |cx| store.read_with(cx, |store, _| store.loaded));
        let names: Vec<String> =
            store.read_with(cx, |store, _| store.configs().iter().map(|c| c.name.clone()).collect());
        for name in ["Detected test", "Dynamic tests"] {
            assert!(names.iter().any(|listed| listed == name), "{name} in {names:?}");
        }
        run_action(cx, "run.chooseConfig", None);
        wait_long(cx, "the chooser opens", |cx| workbench.read_with(cx, |w, _| w.config_chooser().is_some()));
        let chooser = workbench.read_with(cx, |w, _| w.config_chooser()).expect("the chooser");
        let lines = chooser.read_with(cx, |chooser, _| chooser.lines());
        for name in ["Detected test", "Dynamic tests"] {
            assert!(lines.iter().any(|line| line == name), "{name} in {lines:?}");
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Gutter

    /// With a region folded above them, the lines below are painted, hit and moved through by their
    /// row: a click lands on the line it is on; ↓ ↑ and Page Down / Up skip the folded lines; the
    /// breakpoint, execution line, problem underline and a plugin's gutter icons are drawn on their
    /// lines' rows; typing in the line above the fold keeps it; scrolling to the end shows the last line.
    #[gpui_kit::test]
    async fn a_fold_above_moves_every_line_below_it_to_its_row(cx: &mut TestAppContext) {
        use crate::editor::marks::MarkKind;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "test-runner", str::to_owned);
        link(cx, &folder, &[], "test");
        let mut text = String::from("intro\nsetup\n  one\n  two\n  three\ntest adds\ntest subtracts\n");
        text.extend((7..150).map(|n| format!("line {n}\n")));
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("example.tests", text.as_str())]);
        let editor = open_file(&workbench, root.join("example.tests"), cx);
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&editor.focus_handle(cx), cx);
        });
        wait_long(cx, "the gutter icons are drawn", |cx| {
            cx.debug_bounds("plugin-gutter:5").is_some() && cx.debug_bounds("plugin-gutter:6").is_some()
        });

        // Page Down without a fold, to compare with the folded one below.
        let caret_line = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.caret_position().0);
        editor.update(cx, |e, cx| e.select(0, 0, cx));
        cx.simulate_keystrokes("pagedown");
        let unfolded_page = caret_line(cx);
        cx.simulate_keystrokes("pageup");
        assert_eq!(caret_line(cx), 0);

        // Fold Selection from the end of `setup` to the end of `  three`: lines 2 to 4 are hidden.
        let (from, to) = editor.read_with(cx, |e, _| (e.offset_of(1, 5), e.offset_of(5, 0) - 1));
        editor.update(cx, |e, cx| e.select(from, to, cx));
        run_action(cx, "code.foldSelection", None);
        let shown = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.shown_lines());
        let mut expected: Vec<u32> = vec![0, 1];
        expected.extend(5..=150);
        assert_eq!(shown(cx), expected);
        cx.run_until_parked();

        // Each line below the fold is on the row three lines up.
        let line_height = editor.read_with(cx, |e, cx| e.line_height(cx));
        let place =
            |cx: &mut VisualTestContext, line: u32| editor.read_with(cx, |e, _| e.position_of(e.offset_of(line, 0)));
        let top = place(cx, 0).expect("the first line is painted").y;
        let row_top = |line: u32| top + line_height * (line - 3) as f32;
        for line in 5..12 {
            assert_eq!(place(cx, line).map(|at| at.y), Some(row_top(line)), "line {line}");
        }
        assert_eq!(place(cx, 3), None, "a folded line is not painted");

        // A click on `test subtracts` puts the caret on that line.
        let at = place(cx, 6).expect("painted");
        cx.simulate_click(gpui_kit::point(at.x + gpui_kit::px(3.0), at.y + line_height / 2.0), Modifiers::default());
        assert_eq!(caret_line(cx), 6);
        assert_eq!(shown(cx), expected, "the click opened no fold");

        // ↓ from the header goes to the first line after the fold, ↑ comes back.
        let header = editor.read_with(cx, |e, _| e.offset_of(1, 0));
        editor.update(cx, |e, cx| e.select(header, header, cx));
        cx.simulate_keystrokes("down");
        assert_eq!(caret_line(cx), 5);
        cx.simulate_keystrokes("up");
        assert_eq!(caret_line(cx), 1);
        // Page Down moves by rows: the three folded lines count for none.
        editor.update(cx, |e, cx| e.select(0, 0, cx));
        cx.simulate_keystrokes("pagedown");
        assert!(unfolded_page > 5, "a page is {unfolded_page} lines");
        assert_eq!(caret_line(cx), unfolded_page + 3);
        cx.simulate_keystrokes("pageup");
        assert_eq!(caret_line(cx), 0);
        assert_eq!(shown(cx), expected, "moving opened no fold");

        // The marks below the fold are drawn on their lines' rows.
        let line_range = |cx: &mut VisualTestContext, line: u32| {
            editor.read_with(cx, |e, _| e.offset_of(line, 0)..e.offset_of(line, 4))
        };
        let (breakpoint, execution, problem) = (line_range(cx, 7), line_range(cx, 8), line_range(cx, 9));
        editor.update(cx, |e, cx| {
            e.set_marks(MarkLayer::Breakpoints, vec![Mark { range: breakpoint, kind: MarkKind::BreakpointLine }], cx);
            e.set_marks(MarkLayer::Execution, vec![Mark { range: execution, kind: MarkKind::ExecutionLine }], cx);
            e.set_marks(MarkLayer::Diagnostics, vec![Mark { range: problem, kind: MarkKind::ErrorProblem }], cx);
        });
        cx.run_until_parked();
        let painted = |cx: &mut VisualTestContext, kind: MarkKind| editor.read_with(cx, |e, _| e.painted_marks(kind));
        let within = |bounds: &gpui_kit::Bounds<gpui_kit::Pixels>, line: u32| {
            bounds.top() >= row_top(line) && bounds.bottom() <= row_top(line) + line_height
        };
        let breakpoints = painted(cx, MarkKind::BreakpointLine);
        assert!(!breakpoints.is_empty() && breakpoints.iter().all(|b| within(b, 7)), "{breakpoints:?}");
        let executions = painted(cx, MarkKind::ExecutionLine);
        assert!(executions.len() == 1 && within(&executions[0], 8), "{executions:?}");
        let problems = painted(cx, MarkKind::ErrorProblem);
        assert!(problems.len() == 1 && within(&problems[0], 9), "{problems:?}");
        for line in [5, 6] {
            let selector: &'static str = if line == 5 { "plugin-gutter:5" } else { "plugin-gutter:6" };
            let icon = cx.debug_bounds(selector).expect("the icon");
            assert!(within(&icon, line), "the icon of line {line} at {icon:?}");
        }

        // Typing at the end of the line above the fold keeps the fold.
        let end_of_intro = editor.read_with(cx, |e, _| e.offset_of(0, 5));
        editor.update(cx, |e, cx| e.select(end_of_intro, end_of_intro, cx));
        cx.simulate_input("!");
        assert!(editor.read_with(cx, |e, _| e.text().starts_with("intro!\nsetup")));
        assert_eq!(shown(cx), expected, "the fold stays");

        // Scrolling to the end shows the last line: the content is as tall as its rows.
        cx.simulate_event(gpui_kit::ScrollEvent {
            position: gpui_kit::point(at.x + gpui_kit::px(3.0), at.y + line_height / 2.0),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.0), gpui_kit::px(-100_000.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        let (painted_lines, last) =
            editor.read_with(cx, |e, _| (e.painted_lines().expect("painted"), e.line_count() - 1));
        assert_eq!(painted_lines.end, last + 1, "the last line is painted");
        // The content is as tall as its rows: the last line ends at the bottom of the view.
        let bottom = editor.read_with(cx, |e, _| e.painted_bounds().expect("painted").bottom());
        let last_top = place(cx, last).expect("the last line is painted").y;
        assert!(
            (last_top + line_height - bottom).abs() < gpui_kit::px(1.0),
            "{last_top:?} + {line_height:?} vs {bottom:?}"
        );
        assert_eq!(shown(cx), expected, "the fold stays");

        // ⌘↓ from the top moves the caret to the end of the text and scrolls it into view.
        cx.simulate_event(gpui_kit::ScrollEvent {
            position: gpui_kit::point(at.x + gpui_kit::px(3.0), at.y + line_height / 2.0),
            delta: gpui_kit::ScrollDelta::Pixels(gpui_kit::point(gpui_kit::px(0.0), gpui_kit::px(100_000.0))),
            ..Default::default()
        });
        editor.update(cx, |e, cx| e.select(0, 0, cx));
        cx.run_until_parked();
        assert_eq!(place(cx, last), None, "scrolled back to the top");
        cx.simulate_keystrokes("cmd-down");
        cx.run_until_parked();
        assert_eq!(caret_line(cx), last);
        let caret_top = place(cx, last).expect("the caret's line is painted").y;
        let view = editor.read_with(cx, |e, _| e.painted_bounds().expect("painted"));
        assert!(caret_top >= view.top() && caret_top + line_height <= view.bottom() + gpui_kit::px(1.0));
        assert_eq!(shown(cx), expected, "the fold stays");
    }

    // src/features/plugins/languageVisuals.test.ts
    /// A plugin's gutter action shows on its line as its icon; an edit takes it away until the
    /// plugin answered for the new text; a click runs its command with its argument.
    #[gpui_kit::test]
    async fn renders_owned_gutter_commands_and_drops_answers_to_superseded_buffers(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "language-features", str::to_owned);
        let id = link(cx, &folder, &[], "run");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("notes.txt", "one\ntwo\n")]);
        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        let markers = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_gutter_markers(&editor));
        wait_long(cx, "the gutter action shows", |cx| !markers(cx).is_empty());
        let [marker] = markers(cx).try_into().expect("one marker");
        assert_eq!((marker.line, marker.icon.as_str(), marker.title.as_str()), (0, "gutter-run", "Run fixture"));
        editor.update(cx, |editor, cx| editor.insert(Some(0..0), "zero\n", cx));
        assert!(markers(cx).is_empty(), "the answer was for the old text");
        wait_long(cx, "the action shows again", |cx| !markers(cx).is_empty());
        wait_long(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        // A selection on another line, to see that the click leaves it alone.
        editor.update(cx, |editor, cx| editor.set_selections(vec![(6, 8)], 0, cx));
        let before = editor_state(&editor, cx);
        click(cx, "plugin-gutter:0");
        wait_long(cx, "the command ran", |cx| {
            run_in_window(cx, &root, &id, "state", None).ok().flatten().as_deref() == Some("gutter")
        });
        assert_eq!(editor_state(&editor, cx), before, "the click moved no caret or selection and set no breakpoint");
        assert!(before.2.is_empty(), "no breakpoint before the click");
    }

    /// A click on a gutter icon is the icon's: in a file whose gutter takes breakpoints (a press
    /// beside another line sets one there), the icon's line gets none, and the caret and the
    /// selection stay where they were.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_gutter_icon_click_neither_moves_the_caret_nor_toggles_a_breakpoint(cx: &mut TestAppContext) {
        let home = start_with_fake_debugger(cx);
        let folder = plugin_copy(home.path(), "language-features", |text| {
            text.replacen("languages = [\"plaintext\", \"text\", \"sql\"]", "languages = [\"fakedap\"]", 1)
        });
        let id = link(cx, &folder, &[], "run");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("main.fake", "start\nmiddle\nend\n")]);
        let editor = open_file(&workbench, root.join("main.fake"), cx);
        wait_long(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        let icon = cx.debug_bounds("plugin-gutter:0").expect("the icon");
        let line_height = editor.read_with(cx, |editor, cx| editor.line_height(cx));
        let breakpoints = |cx: &mut VisualTestContext| editor_state(&editor, cx).2;
        // The same place one line down is the editor's gutter: a press there sets a breakpoint.
        cx.simulate_click(point(icon.center().x, icon.center().y + line_height), gpui_kit::Modifiers::default());
        wait_long(cx, "the breakpoint beside the second line", |cx| breakpoints(cx).len() == 1);
        editor.update(cx, |editor, cx| editor.set_selections(vec![(8, 11)], 0, cx));
        let before = editor_state(&editor, cx);
        click(cx, "plugin-gutter:0");
        wait_long(cx, "the command ran", |cx| {
            run_in_window(cx, &root, &id, "state", None).ok().flatten().as_deref() == Some("gutter")
        });
        assert_eq!(editor_state(&editor, cx), before, "the caret, the selection and the breakpoints are as they were");
    }

    /// The fold chevron and a plugin's icon on one line are each their own: a click on the chevron
    /// folds the plugin's region and runs no gutter action, leaving the caret, the selection and the
    /// breakpoints alone; a click on the icon runs its action and leaves the fold as it is.
    #[gpui_kit::test]
    async fn a_click_on_the_fold_chevron_runs_no_gutter_action_and_one_on_the_icon_folds_nothing(
        cx: &mut TestAppContext,
    ) {
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "language-features", str::to_owned);
        let id = link(cx, &folder, &[], "run");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("notes.txt", "one\ntwo\nthree\nfour\n")]);
        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        wait_long(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        wait_long(cx, "the plugin's fold is offered", |cx| editor.read_with(cx, |e, _| e.fold_marker(0).is_some()));
        let shown = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.shown_lines());
        editor.update(cx, |editor, cx| editor.set_selections(vec![(14, 16)], 0, cx));
        let before = editor_state(&editor, cx);
        let chevron = editor.read_with(cx, |e, cx| {
            let at = e.position_of(e.offset_of(0, 0)).expect("the line is painted");
            point(at.x - e.fold_column_width(cx) / 2.0, at.y + e.line_height(cx) / 2.0)
        });
        let icon = cx.debug_bounds("plugin-gutter:0").expect("the icon");
        assert!(!icon.contains(&chevron), "the icon {icon:?} is left of the chevron {chevron:?}");
        cx.simulate_click(chevron, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(shown(cx), [0, 3, 4], "the region folds");
        assert_eq!(editor_state(&editor, cx), before, "the caret, the selection and the breakpoints stay");
        let state = |cx: &mut VisualTestContext| run_in_window(cx, &root, &id, "state", None).ok().flatten();
        assert_ne!(state(cx).as_deref(), Some("gutter"), "no gutter action ran");
        wait_long(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        click(cx, "plugin-gutter:0");
        wait_long(cx, "the command ran", |cx| state(cx).as_deref() == Some("gutter"));
        assert_eq!(shown(cx), [0, 3, 4], "the fold stays");
    }

    // src/features/plugins/languageVisuals.test.ts
    /// Two plugins put an action on the same line: one icon, whose click opens a menu of both; the
    /// chosen entry runs in its own plugin only.
    #[gpui_kit::test]
    async fn puts_the_actions_of_one_line_behind_one_icon_whose_click_opens_a_menu(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let first = plugin_copy(home.path(), "language-features", str::to_owned);
        let second = second_copy(home.path(), &first, "language-features-two", |text| {
            text.replace("id = \"test.language-features\"", "id = \"test.language-features-two\"")
        });
        let one = link(cx, &first, &[], "run");
        let two = link(cx, &second, &[], "run");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("notes.txt", "one\n")]);
        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        wait_long(cx, "both actions show", |cx| {
            workbench
                .read_with(cx, |w, _| w.plugin_gutter_markers(&editor).first().is_some_and(|m| m.actions.len() == 2))
        });
        wait_long(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        // A selection away from the icon's line start, to see that the click leaves it alone.
        editor.update(cx, |editor, cx| editor.set_selections(vec![(1, 3)], 0, cx));
        let before = editor_state(&editor, cx);
        click(cx, "plugin-gutter:0");
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<LineAction>()).expect("the menu is open");
        assert_eq!(editor_state(&editor, cx), before, "the click moved no caret or selection");
        let rows: Vec<(String, Option<String>)> = menu.read_with(cx, |menu: &PopupMenu<LineAction>, _| {
            menu.rows().iter().map(|r| (r.label.clone(), r.icon.clone())).collect()
        });
        assert_eq!(
            rows,
            [("Run fixture".to_owned(), Some("run".to_owned())), ("Run fixture".to_owned(), Some("run".to_owned()))]
        );
        let plugins: Vec<String> = workbench.read_with(cx, |w, _| {
            w.plugin_gutter_markers(&editor)[0].actions.iter().map(|a| a.plugin_id.clone()).collect()
        });
        cx.simulate_keystrokes("down enter");
        let chosen = plugins[1].clone();
        let other = if chosen == one { two.clone() } else { one.clone() };
        wait_long(cx, "the chosen plugin ran", |cx| {
            run_in_window(cx, &root, &chosen, "state", None).ok().flatten().as_deref() == Some("gutter")
        });
        assert_eq!(run_in_window(cx, &root, &other, "state", None).expect("it answers"), None);
        let after = editor_state(&editor, cx);
        assert!(after.2.is_empty(), "no breakpoint was toggled on the icon's line: {:?}", after.2);
    }

    // ---------------------------------------------------------------------------------------------
    // Menus

    /// The Hello fixture with entries in the Tools menu, the editor's menu (a submenu, which needs
    /// plugin API 1.3) and the Project view's menu; one command only for workspaces with a
    /// `MODULE.bazel` file.
    fn hello_menus(text: &str) -> String {
        let text = text.replace("engines.openidea = \"^1.0\"", "engines.openidea = \"^1.3\"");
        format!(
            "{text}\n\
             [[contributes.commands]]\nid = \"sync\"\ntitle = \"Hello: Sync\"\n\
             when = \"workspaceContains:MODULE.bazel\"\n\n\
             [[contributes.menus]]\nmenu = \"tools\"\ncommand = \"sync\"\n\n\
             [[contributes.menus]]\nmenu = \"tools\"\ncommand = \"insert-date\"\n\n\
             [[contributes.menus]]\nmenu = \"editor.context\"\ncommand = \"insert-date\"\nsubmenu = \"Hello\"\n\n\
             [[contributes.menus]]\nmenu = \"editor.context\"\ncommand = \"sync\"\nsubmenu = \"Hello\"\n\n\
             [[contributes.menus]]\nmenu = \"projectView.context\"\ncommand = \"word-count\"\n\
             when = \"workspaceContains:MODULE.bazel\"\n"
        )
    }

    /// Tools lists the plugin's entries in their order, greyed out while their command's `when`
    /// does not hold; chosen in the menu bar, an entry runs the plugin's command.
    #[gpui_kit::test]
    async fn the_tools_menu_lists_plugin_entries_with_their_state_and_runs_the_chosen_one(cx: &mut TestAppContext) {
        use super::super::menu::{self, MenuCommand};
        use super::super::menu_spec::DYNAMIC_PLUGIN_TOOLS;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "hello", hello_menus);
        link(cx, &folder, &[], "sync");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("notes.txt", "one two\n")]);
        let state = |cx: &mut VisualTestContext| cx.update(|window, cx| menu::state(window, cx));
        wait_long(cx, "the key is answered", |cx| {
            context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(false))
        });
        let shown = state(cx);
        let items = shown.dynamic.get(DYNAMIC_PLUGIN_TOOLS).cloned().unwrap_or_default();
        let labels: Vec<(&str, &str)> = items.iter().map(|item| (item.label.as_str(), item.action.as_str())).collect();
        assert_eq!(labels, [("Hello: Sync", "plugin.hello.sync"), ("Hello: Insert Date", "plugin.hello.insert-date")]);
        assert!(shown.disabled.contains("plugin.hello.sync"), "greyed out without MODULE.bazel");
        assert!(!shown.disabled.contains("plugin.hello.insert-date"));
        std::fs::write(root.join("MODULE.bazel"), "module(name = \"x\")\n").expect("the file is written");
        wait_long(cx, "the entry comes on", |cx| !state(cx).disabled.contains("plugin.hello.sync"));

        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        cx.dispatch_action(MenuCommand { action: "plugin.hello.insert-date".to_owned(), arg: None });
        static DATE: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("the date pattern is valid"));
        wait_long(cx, "the plugin inserts the date", |cx| editor.read_with(cx, |e, _| DATE.is_match(&e.text())));
    }

    /// The Project view's menu has the plugin's entry once its `when` holds; choosing it runs the
    /// command, whose notification appears. The editor's menu groups the plugin's two entries of
    /// one submenu under it.
    #[gpui_kit::test]
    async fn the_context_menus_offer_plugin_entries_where_their_when_holds_and_run_them(cx: &mut TestAppContext) {
        use crate::project_view::context_menu::ProjectCommand;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "hello", hello_menus);
        link(cx, &folder, &[], "word-count");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("notes.txt", "one two three\n")]);
        let file = root.join("notes.txt");
        open_file(&workbench, file.clone(), cx);
        let editor_items: Vec<MenuItem<()>> = cx.update(|window, cx| {
            let ctx = window_context(window, cx);
            plugin_menu_items(PluginMenu::Editor, &[], None, &ctx, cx)
        });
        match editor_items.as_slice() {
            [MenuItem::Action(word), MenuItem::Item(group), MenuItem::Separator] => {
                assert_eq!((word.action, word.arg.clone()), ("plugin.hello.word-count", None));
                assert_eq!(group.label, "Hello");
                let grouped: Vec<&str> = group
                    .submenu
                    .iter()
                    .filter_map(|item| match item {
                        MenuItem::Action(action) => Some(action.action),
                        _ => None,
                    })
                    .collect();
                assert_eq!(grouped, ["plugin.hello.insert-date", "plugin.hello.sync"]);
            }
            other => panic!("the editor's plugin group: {other:?}"),
        }

        let open_tree_menu = |cx: &mut VisualTestContext| {
            let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("the tree");
            let at = file.to_string_lossy().into_owned();
            wait_long(cx, "the tree lists the file", |cx| tree.update(cx, |tree, cx| tree.reveal_path(&at, cx)));
            cx.update(|window, cx| window.focus(&tree.focus_handle(cx), cx));
            cx.run_until_parked();
            let selector: &'static str = Box::leak(format!("project-row:{at}").into_boxed_str());
            let row = cx.debug_bounds(selector).expect("the row is drawn");
            cx.simulate_mouse_down(row.center(), MouseButton::Right, Modifiers::default());
            cx.simulate_mouse_up(row.center(), MouseButton::Right, Modifiers::default());
            cx.run_until_parked();
            let menu =
                workbench.read_with(cx, |w, _| w.open_menu::<ProjectCommand>()).expect("the context menu is open");
            menu.read_with(cx, |menu, _| menu.lines()[0].clone())
        };
        let entry = "Hello: Word Count of Active File";
        wait_long(cx, "the key is answered", |cx| {
            context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(false))
        });
        let lines = open_tree_menu(cx);
        assert!(!lines.iter().any(|line| line.trim_start_matches("> ") == entry), "{lines:?}");
        cx.simulate_keystrokes("escape");
        std::fs::write(root.join("MODULE.bazel"), "module(name = \"x\")\n").expect("the file is written");
        wait_long(cx, "the key holds", |cx| {
            context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(true))
        });
        let lines = open_tree_menu(cx);
        let at = lines.iter().position(|line| line.trim_start_matches("> ") == entry).expect("the plugin's entry");
        // The entry follows Delete…, as the menu's plugin group does.
        let delete = lines.iter().position(|line| line.trim_start_matches("> ") == "Delete…").expect("Delete…");
        assert!(at > delete, "{lines:?}");
        click(cx, &format!("menu-row-0-{at}"));
        wait_long(cx, "the plugin's notification", |cx| {
            notice_titled(&workbench, "Word count: notes.txt", cx).is_some()
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Settings pages

    /// The plugin's page in Settings shows its setting; ticked and saved with OK, the value is in
    /// the global settings and the plugin hears `settings-changed` with its key.
    #[gpui_kit::test]
    async fn a_plugins_settings_page_saves_its_value_and_the_plugin_hears_the_change(cx: &mut TestAppContext) {
        use crate::settings_model::schema::settings_value;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "api14", str::to_owned);
        link(cx, &folder, &["events"], "events");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
        // The plugin runs once, so it is live when the event comes.
        run_in_window(cx, &root, "api14", "events", None).expect("it answers");
        // The host keeps the settings the window told it as the plugin's baseline; a change made
        // before that would be the baseline itself.
        wait_long(cx, "the settings went out once", |cx| workbench.read_with(cx, |w, _| w.plugin_settings_told()));
        run_action(cx, "file.settings", Some(Value::from("plugins.api14")));
        wait_long(cx, "the page shows", |cx| cx.debug_bounds("plugin-setting:enabled").is_some());
        let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("Settings is open");
        assert_eq!(dialog.read_with(cx, |d, cx| d.selected_page(cx)).as_deref(), Some("plugins.api14"));
        click(cx, "plugin-setting:enabled");
        click(cx, "settings-ok");
        wait_long(cx, "Settings closes", |cx| workbench.read_with(cx, |w, _| w.settings_dialog().is_none()));
        let stored = cx.update(|_, cx| settings_value(&crate::platform::settings::global(cx)));
        assert_eq!(stored.pointer("/plugins/api14/enabled"), Some(&Value::Bool(true)));
        wait_long(cx, "the plugin heard the change", |cx| {
            stored_events(cx, &root, "api14")
                .iter()
                .any(|e| e["kind"] == "settings-changed" && e["data"] == "[\"enabled\"]")
        });
    }

    // src/features/plugins/__tests__/settings.test.tsx
    /// A declared page's action applies the pending row first: the plugin's command reads it and
    /// says so in a notification. Before a row exists, its error shows under the button.
    #[gpui_kit::test]
    async fn applies_pending_values_before_a_command_reads_them_and_shows_command_errors_inline(
        cx: &mut TestAppContext,
    ) {
        use crate::settings_model::schema::settings_value;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "settings-pages", str::to_owned);
        link(cx, &folder, &[], "test");
        let (_folder, _root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
        run_action(cx, "file.settings", Some(Value::from("plugins.settings-pages")));
        wait_long(cx, "the plugin's page lists its pages", |cx| {
            cx.debug_bounds("plugin-settings-link:plugins.settings-pages.settings:llm").is_some()
        });
        click(cx, "plugin-settings-link:plugins.settings-pages.settings:llm");
        wait_long(cx, "the LLM page shows", |cx| cx.debug_bounds("plugin-setting-add:profiles").is_some());
        click(cx, "plugin-settings-action:0");
        wait_long(cx, "the error shows under the button", |cx| {
            cx.debug_bounds("plugin-settings-action-error:0").is_some()
        });
        click(cx, "plugin-setting-add:profiles");
        click(cx, "plugin-settings-action:0");
        wait_long(cx, "the plugin read the row", |cx| {
            notice_titled(&workbench, "Model Profiles", cx)
                .is_some_and(|notice| notice.body.as_deref() == Some("Read 1 saved profile(s)"))
        });
        let stored = cx.update(|_, cx| settings_value(&crate::platform::settings::global(cx)));
        let profiles = stored.pointer("/plugins/settings-pages/profiles").and_then(Value::as_array).cloned();
        assert_eq!(profiles.map(|rows| rows.len()), Some(1), "applied before the command ran");
        assert!(cx.debug_bounds("plugin-settings-action-error:0").is_none(), "the error went");
    }

    // ---------------------------------------------------------------------------------------------
    // Events

    /// A saved file of the window reaches the plugin that watches `config/*.yaml` saves; opening it
    /// does not, since that plugin watches openings of other files.
    #[gpui_kit::test]
    async fn a_saved_file_reaches_the_plugin_that_watches_its_saves(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "path-events", str::to_owned);
        link(cx, &folder, &["events", "project.read"], "count");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("config/app.yaml", "a: 1\n")]);
        let count =
            |cx: &mut VisualTestContext| run_in_window(cx, &root, "test.path-events", "count", None).ok().flatten();
        assert_eq!(count(cx), None, "nothing heard yet");
        run_in_window(cx, &root, "test.path-events", "filter", Some("nothing/*.none")).expect("the filter is set");
        let editor = open_file(&workbench, root.join("config/app.yaml"), cx);
        editor.update(cx, |editor, cx| editor.insert(Some(0..0), "b: 2\n", cx));
        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
        cx.executor().spawn(saving).detach();
        wait_long(cx, "the file is saved", |cx| !editor.read_with(cx, |editor, _| editor.is_dirty()));
        wait_long(cx, "the plugin heard the save", |cx| count(cx).as_deref() == Some("1"));
    }

    /// The editor's input mode reaches the plugin that wants `input-mode-changed`.
    #[gpui_kit::test]
    async fn an_input_mode_change_reaches_the_plugin_that_wants_it(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "api14", str::to_owned);
        link(cx, &folder, &["events.input"], "events");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
        run_in_window(cx, &root, "api14", "events", None).expect("it answers");
        let path = root.join("a.txt").to_string_lossy().into_owned();
        // The window learns which events plugins want before it reports one.
        wait_long(cx, "the event kinds are known", |cx| {
            workbench.update(cx, |w, cx| w.plugin_input_mode_changed(&path, "INSERT", None, cx));
            stored_events(cx, &root, "api14").iter().any(|e| e["kind"] == "input-mode-changed")
        });
        let heard = stored_events(cx, &root, "api14");
        let event = heard.iter().find(|e| e["kind"] == "input-mode-changed").expect("heard");
        let data: Value = serde_json::from_str(event["data"].as_str().unwrap_or("{}")).expect("the mode");
        assert_eq!((data["path"].as_str(), data["mode"].as_str()), (Some(path.as_str()), Some("INSERT")));
    }

    /// Focusing a plain text file loads the plugin activated by that language, and focusing a file
    /// under `notes/` the one activated by that path; neither runs before.
    #[gpui_kit::test]
    async fn focusing_an_editor_fires_the_language_and_file_activation_events(cx: &mut TestAppContext) {
        let home = start_with_plugins(cx);
        let activated = |id: &'static str, activation: &'static str| {
            move |text: &str| {
                text.replace("id = \"hello\"", &format!("id = \"{id}\"")).replace(
                    "activation = [\"on-command:*\", \"on-view:greetings\"]",
                    &format!("activation = [\"{activation}\"]"),
                )
            }
        };
        let hello = plugin_copy(home.path(), "hello", str::to_owned);
        let by_language =
            second_copy(home.path(), &hello, "hello-language", activated("hello-language", "on-language:plaintext"));
        let by_file = second_copy(home.path(), &hello, "hello-file", activated("hello-file", "on-file:notes/*.md"));
        link(cx, &by_language, &[], "word-count");
        link(cx, &by_file, &[], "word-count");
        let files = [("a.txt", "a\n"), ("notes/today.md", "# Today\n")];
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &files);
        assert_eq!(status_of("hello-language", cx), Some(PluginStatus::Inactive));
        assert_eq!(status_of("hello-file", cx), Some(PluginStatus::Inactive));
        open_file(&workbench, root.join("a.txt"), cx);
        wait_long(cx, "the language's plugin loads", |cx| {
            status_of("hello-language", cx) == Some(PluginStatus::Active)
        });
        assert_eq!(status_of("hello-file", cx), Some(PluginStatus::Inactive));
        open_file(&workbench, root.join("notes/today.md"), cx);
        wait_long(cx, "the file's plugin loads", |cx| status_of("hello-file", cx) == Some(PluginStatus::Active));
    }

    // src/features/plugins/__tests__/plugins.test.tsx
    /// An action run from a menu with a shortcut reaches the plugin that wants `action-performed`,
    /// which tells the shortcut in a notification.
    #[gpui_kit::test]
    async fn reports_actions_to_action_performed_plugins(cx: &mut TestAppContext) {
        use crate::platform::input::Keymap;
        use crate::platform::keymap::is_mac_host;
        let home = start_with_plugins(cx);
        let folder = plugin_copy(home.path(), "key-promoter", str::to_owned);
        link(cx, &folder, &[], "statistics");
        let (_folder, _root, workbench, cx) = trusted_project(cx, LABEL, &[("a.txt", "a\n")]);
        const ACTION: &str = "navigate.gotoFile";
        let (title, shortcut) = cx.update(|_, cx| {
            let title = cx.global::<ActionRegistry>().get(ACTION).expect("registered").meta.title.clone();
            let shortcut = cx.global::<Keymap>().0.shortcut_label(ACTION, is_mac_host()).expect("a shortcut");
            (title, shortcut)
        });
        let expected = format!("{title}: {shortcut}");
        wait_long(cx, "the plugin hears the action", |cx| {
            cx.update(|window, cx| {
                execute_action_from(ActionSource::Menu, ACTION, None, window, cx);
            });
            cx.run_until_parked();
            workbench.update_in(cx, |w, window, cx| w.close_popup(window, cx));
            notice_titled(&workbench, &expected, cx).is_some()
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Background Tasks

    /// A click on the tasks status item opens Background Tasks with the plugin's task and its
    /// working Cancel; Cancel there shows "Cancelling…", stops the program the plugin waits on and
    /// the task leaves the list. A second click closes the popup.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn cancelling_a_plugins_task_from_the_background_tasks_popup_stops_it(cx: &mut TestAppContext) {
        use super::super::background_tasks_view::CancelButton;
        use super::super::status_bar::TASKS_ITEM;
        use std::{cell::RefCell, rc::Rc};
        let home = start_with_plugins(cx);
        let bin = home.path().join("bin");
        // Far longer than any wait: the task can only leave the list by being cancelled.
        program(&bin, "checkstyle", "#!/bin/sh\nexec /bin/sleep 600\n");
        programs_in(&bin, cx);
        let folder = plugin_copy(home.path(), "checkstyle", str::to_owned);
        let id = link(cx, &folder, &[], "check");
        let (_folder, root, workbench, cx) = trusted_project(cx, LABEL, &[("src/Main.java", "class Main {}\n")]);
        open_file(&workbench, root.join("src/Main.java"), cx);
        run_action(cx, &plugin_action_id(&id, "check"), None);
        let title = "Checkstyle: Main.java";
        wait_long(cx, "the task is listed", |cx| {
            workbench.read_with(cx, |w, _| w.tasks().tasks().iter().any(|task| task.title == title))
        });
        let item: &'static str = Box::leak(format!("status:{TASKS_ITEM}").into_boxed_str());
        wait_long(cx, "the tasks item is drawn", |cx| cx.debug_bounds(item).is_some());
        click(cx, item);
        let popup = workbench.read_with(cx, |w, _| w.background_tasks_popup()).expect("Background Tasks is open");
        let rows = popup.read_with(cx, |popup, _| popup.rows());
        let index = rows.iter().position(|row| row.title == title).expect("the plugin's task is listed");
        assert_eq!(rows[index].cancel, CancelButton::Working);
        // The program dies within milliseconds of Cancel, so the popup's states are recorded as the
        // window changes rather than looked at after the click.
        let seen: Rc<RefCell<Vec<Option<CancelButton>>>> = Rc::default();
        let _watch = cx.update(|_, cx| {
            let (popup, seen) = (popup.clone(), seen.clone());
            cx.observe(&workbench, move |_, cx| {
                let row = popup.read(cx).rows().into_iter().find(|row| row.title == title);
                seen.borrow_mut().push(row.map(|row| row.cancel));
            })
        });
        click(cx, &format!("task-cancel:{index}"));
        let row = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, _| popup.rows().into_iter().find(|row| row.title == title))
        };
        wait_long(cx, "the task ends", |cx| row(cx).is_none());
        let seen = seen.borrow().clone();
        let cancelling = Some(CancelButton::Disabled("Cancelling…".to_owned()));
        assert!(seen.contains(&cancelling), "Cancel showed Cancelling… while the task ended: {seen:?}");
        run_action(cx, BACKGROUND_TASKS, Some(json!({ "toggle": true })));
        assert!(workbench.read_with(cx, |w, _| w.background_tasks_popup().is_none()));
    }
}
