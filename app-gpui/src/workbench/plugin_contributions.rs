//! What plugins contribute to a window besides what their running code shows: the runs and debug
//! runs they ask the window for, the Run and Debug actions they put in an editor's gutter, their
//! entries in the menus, their pages in Settings, the editor events and activation events they
//! hear, and the Background Tasks popup where a plugin's task (or any other) is cancelled.
//!
//! - **Runs.** A plugin's `run.start` reaches the window as a run request: its configuration starts
//!   in the Run tool window. The backend tells the plugin when the process exits (the request's
//!   operation travels in the configuration); a run that never started is reported here as ended
//!   without a status. A debug request fails, visibly, while this window has no debugger a plugin's
//!   session can be followed in.
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
use super::notifications::{NotificationLevel, NotificationRequest};
use super::plugin_runtime::{answer_plugin_command, plugin_host};
use super::services_window::on_workbench;
use crate::backend::Backend;
use crate::editor::view::{Editor, EditorEvent};
use crate::platform::actions::ActionRegistry;
use crate::platform::context::{ContextKeys, evaluate_when, window_context};
use crate::platform::i18n::{Localization, Param};
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
/// The size of a gutter action's icon, and its distance from the text.
const GUTTER_ICON: f32 = 12.0;
const GUTTER_INSET: f32 = 13.0;
/// The popup's corner keeps this far from the window's bottom right (above the status bar).
const TASKS_POPUP_INSET: (f32, f32) = (16.0, 32.0);
/// Why a run request is refused while the window has no project, and while debugging a plugin's
/// session is not followed here.
const RUN_UNAVAILABLE: &str = "Run service unavailable";
const DEBUGGER_UNAVAILABLE: &str = "Debugger unavailable";
const RUN_NOT_STARTED: &str = "Run did not start";
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
fn static_action_id(id: &str) -> &'static str {
    static IDS: LazyLock<Mutex<BTreeSet<&'static str>>> = LazyLock::new(|| Mutex::new(BTreeSet::new()));
    let mut ids = IDS.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = ids.get(id) {
        return *known;
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
}

/// Registers the Background Tasks popup's action.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle(BACKGROUND_TASKS, |_, window, cx| {
        on_workbench(window, cx, |workbench, window, cx| workbench.toggle_background_tasks(window, cx))
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
        cx: &mut Context<Self>,
    ) {
        if debug {
            // A plugin hears when its session ends; this window's debugger does not tell it yet.
            let title = cx
                .default_global::<Localization>()
                .0
                .tx_built(&["“", "” can't be debugged"], &[Param::Text(config.name.clone())]);
            let request = NotificationRequest {
                body: Some(DEBUGGER_UNAVAILABLE.to_owned()),
                group: Some("run".to_owned()),
                ..NotificationRequest::new(NotificationLevel::Warning, title)
            };
            self.notify(request, cx);
            return self.refuse_plugin_run(request_id, operation, DEBUGGER_UNAVAILABLE, cx);
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
    fn refuse_plugin_run(&mut self, request_id: &str, operation: &str, reason: &str, cx: &mut Context<Self>) {
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
        if markers.is_empty() {
            return Vec::new();
        }
        let (places, line_height) = {
            let editor = editor.read(cx);
            let Some(painted) = editor.painted_lines() else { return Vec::new() };
            let line_height = editor.line_height(cx);
            let first = painted.start;
            // The editor's top: where its first painted line would start unscrolled.
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * first as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * (painted.end - painted.start).saturating_sub(1) as f32);
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
            (places, line_height)
        };
        let color = theme::color(cx, "fg-muted");
        places
            .into_iter()
            .map(|(index, at)| {
                let marker = &markers[index];
                let line = marker.line;
                let corner = point(at.x - px(GUTTER_INSET), at.y + (line_height - px(GUTTER_ICON)) / 2.0);
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
    // Menus

    /// The plugin group of the Project view's context menu for the selected `paths`.
    pub(super) fn plugin_project_view_items<C: Clone>(
        &self,
        paths: &[String],
        window: &Window,
        cx: &App,
    ) -> Vec<MenuItem<C>> {
        plugin_menu_items(PluginMenu::ProjectView, paths, None, &window_context(window, cx), cx)
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
        cx.background_executor().spawn(async move {
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
    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::actions::{ActionSource, execute_action_from, execute_action_or_throw};
    use crate::platform::context::ContextValue;
    use crate::platform::storage::UiStorage;
    use crate::run::view::RunView;
    use crate::ui::popup_menu::PopupMenu;
    use gpui_kit::{Focusable as _, Modifiers, TestAppContext, VisualTestContext};
    use ide_host::plugins::{Consent, FileSecretStore, PluginStatus};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    const LABEL: &str = "w1";

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
    }

    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("the folder is made");
        for entry in std::fs::read_dir(from).expect("the fixture is readable").flatten() {
            let (source, target) = (entry.path(), to.join(entry.file_name()));
            if entry.file_name() == "target" {
                continue;
            }
            if source.is_dir() {
                copy_dir(&source, &target);
            } else {
                std::fs::copy(&source, &target).expect("the file is copied");
            }
        }
    }

    /// A copy named `copy` of the fixture plugin `name` (under `crates/ide-plugin/tests/fixtures`,
    /// or `plugins/official`) in the temporary home, its manifest rewritten by `edit`: the
    /// repository's own folders are only read.
    fn plugin_copy(home: &Path, name: &str, copy: &str, edit: impl Fn(&str) -> String) -> PathBuf {
        let fixture = repo().join("crates/ide-plugin/tests/fixtures").join(name);
        let source = if fixture.is_dir() { fixture } else { repo().join("plugins/official").join(name) };
        let target = home.join("fixtures").join(copy);
        copy_dir(&source, &target);
        let manifest = target.join("plugin.toml");
        let text = std::fs::read_to_string(&manifest).expect("the manifest is readable");
        std::fs::write(&manifest, edit(&text)).expect("the manifest is written");
        target
    }

    #[cfg(unix)]
    fn program(bin: &Path, name: &str, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(bin).expect("the folder is made");
        let path = bin.join(name);
        std::fs::write(&path, script).expect("the script is written");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    }

    fn settle_app(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut App) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if cx.update(|cx| done(cx)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within ten seconds");
    }

    /// Waits for `done`, moving the clock the window's timers run on.
    fn wait(cx: &mut VisualTestContext, what: &str, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..3000 {
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within thirty seconds");
    }

    /// A temporary home with the backend and the plugin host running in it. Plugin secrets are kept
    /// in a file under the home, never in the login keychain.
    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            crate::platform::input::init(cx);
            crate::editor::actions::register(cx);
            crate::workbench::register_actions(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            crate::workbench::menu::init(cx);
            crate::terminal::test_support::isolate_shells(home.path(), cx);
            theme::appearance::init(cx);
            Plugins::init(cx).expect("the plugin host starts");
            let host = cx.global::<Plugins>().host.clone();
            host.set_secret_store(Arc::new(FileSecretStore { dir: home.path().join("secrets") }));
        });
        settle_app(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
        home
    }

    /// Programs a plugin may be granted are looked up in `bin` only.
    fn programs_in(bin: &Path, cx: &mut TestAppContext) {
        let bin = bin.to_path_buf();
        cx.update(|cx| {
            cx.global::<Plugins>().host.set_program_path(Arc::new(move || Some(bin.clone().into_os_string())))
        });
    }

    /// Links `folder` and agrees to it with its required capabilities and `optional`; waits until
    /// command `command` is an action.
    fn link(cx: &mut TestAppContext, folder: &Path, optional: &[&str], command: &str) -> String {
        let id = cx.update(|cx| {
            let host = cx.global::<Plugins>().host.clone();
            let info = host.link(folder).expect("the plugin links");
            let consent =
                Consent { optional: optional.iter().map(|c| (*c).to_owned()).collect(), signer_change: false };
            let info = host.grant(&info.id, &consent).expect("consent is recorded");
            assert_eq!(info.error, None, "the plugin loads");
            info.id
        });
        let action = plugin_action_id(&id, command);
        settle_app(cx, "the plugin's commands are actions", |cx| cx.global::<ActionRegistry>().contains(&action));
        id
    }

    /// A trusted project of `files` (path, text), open in an active window.
    fn project<'a>(
        cx: &'a mut TestAppContext,
        files: &[(&str, &str)],
    ) -> (tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
            std::fs::write(path, text).expect("the file is written");
        }
        cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false)).expect("the project is trusted");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new(LABEL, window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        wait(cx, "the project opens", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        (folder, root, workbench, cx)
    }

    fn run_action(cx: &mut VisualTestContext, id: &str, arg: Option<Value>) {
        cx.update(|window, cx| execute_action_or_throw(id, arg.as_ref(), window, cx)).expect("the action runs");
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
        wait(cx, "the command answers", |_| running.is_finished());
        running.join().expect("the command ran")
    }

    /// The `kind`-`data` events the fixture stored, as its `events` command lists them.
    fn stored_events(cx: &mut VisualTestContext, root: &Path, plugin: &str) -> Vec<Value> {
        let listed = run_in_window(cx, root, plugin, "events", None).ok().flatten();
        listed.and_then(|text| serde_json::from_str::<Vec<Value>>(&text).ok()).unwrap_or_default()
    }

    fn notifications(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<(String, Option<String>)> {
        workbench
            .read_with(cx, |w, _| w.notifications().items().iter().map(|n| (n.title.clone(), n.body.clone())).collect())
    }

    fn context(key: &str, cx: &mut VisualTestContext) -> Option<ContextValue> {
        cx.update(|window, cx| window_context(window, cx).get(key).cloned())
    }

    fn status_of(plugin: &str, cx: &mut VisualTestContext) -> Option<PluginStatus> {
        let host = cx.update(|_, cx| cx.global::<Plugins>().host.clone());
        host.list(None).plugins.into_iter().find(|p| p.id == plugin).map(|p| p.status)
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let selector: &'static str = Box::leak(selector.to_owned().into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.run_until_parked();
    }

    fn open_file(workbench: &Entity<Workbench>, path: PathBuf, cx: &mut VisualTestContext) -> Entity<Editor> {
        workbench.update_in(cx, |w, window, cx| w.open_project_file(path.clone(), window, cx));
        wait(cx, "the file opens", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == path.as_path()))
        });
        workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor")
    }

    /// The `execution` fixture with its test detector given a debugger, so the host passes a debug
    /// request on to the window.
    #[cfg(unix)]
    fn with_debugger(text: &str) -> String {
        text.replace("framework = \"plugin\"\n", "framework = \"plugin\"\ndebugger = \"java-debug\"\n")
    }

    #[cfg(unix)]
    fn execution_project<'a>(
        cx: &'a mut TestAppContext,
        edit: impl Fn(&str) -> String,
    ) -> (tempfile::TempDir, tempfile::TempDir, PathBuf, Entity<Workbench>, &'a mut VisualTestContext) {
        let home = start(cx);
        let bin = home.path().join("bin");
        program(&bin, "execution-tool", "#!/bin/sh\necho \"execution-tool $*\"\nexit 3\n");
        programs_in(&bin, cx);
        let folder = plugin_copy(home.path(), "execution", "execution", edit);
        link(cx, &folder, &["process:execution-tool"], "run");
        let (folder, root, workbench, cx) = project(cx, &[("execution.marker", "")]);
        (home, folder, root, workbench, cx)
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
        let (_home, _folder, root, workbench, cx) = execution_project(cx, str::to_owned);
        run_action(cx, &plugin_action_id("execution", "run"), Some(Value::from("detected")));
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        let session = |cx: &mut VisualTestContext| {
            store.read_with(cx, |store, _| store.sessions.iter().find(|s| s.config.name == "Detected test").cloned())
        };
        wait(cx, "the run ends", |cx| session(cx).is_some_and(|s| !s.is_live()));
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
        wait(cx, "the plugin hears the exit", |cx| {
            stored_events(cx, &root, "execution").iter().any(|event| {
                event["kind"] == "run-exited"
                    && serde_json::from_str::<Value>(event["data"].as_str().unwrap_or("{}"))
                        .is_ok_and(|data| data["status"] == 3 && data["killed"] == false)
            })
        });
    }

    /// A plugin asks to debug its configuration: without a debugger for plugin sessions the window
    /// says it cannot, starts nothing, and the plugin hears its run ended without a status.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_plugins_debug_request_fails_visibly_and_the_plugin_hears_it_ended(cx: &mut TestAppContext) {
        let (_home, _folder, root, workbench, cx) = execution_project(cx, with_debugger);
        run_action(cx, &plugin_action_id("execution", "debug"), Some(Value::from("detected")));
        wait(cx, "the window says why", |cx| {
            notifications(&workbench, cx).iter().any(|(title, body)| {
                title == "“Detected test” can't be debugged" && body.as_deref() == Some("Debugger unavailable")
            })
        });
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        assert!(driver.read_with(cx, |driver, cx| driver.store().read(cx).sessions.is_empty()), "nothing started");
        wait(cx, "the plugin hears the end", |cx| {
            stored_events(cx, &root, "execution").iter().any(|event| {
                event["kind"] == "run-exited"
                    && serde_json::from_str::<Value>(event["data"].as_str().unwrap_or("{}"))
                        .is_ok_and(|data| data["status"].is_null())
            })
        });
    }

    /// A plugin's run detector and its dynamic configurations are listed with the project's run
    /// configurations, in the configuration chooser.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_plugins_run_detectors_are_listed_in_the_configuration_chooser(cx: &mut TestAppContext) {
        let (_home, _folder, _root, workbench, cx) = execution_project(cx, str::to_owned);
        let driver = workbench.read_with(cx, |w, _| w.run_driver().cloned()).expect("the project runs things");
        let store = driver.read_with(cx, |driver, _| driver.store().clone());
        wait(cx, "the configurations are listed", |cx| store.read_with(cx, |store, _| store.loaded));
        let names: Vec<String> =
            store.read_with(cx, |store, _| store.configs().iter().map(|c| c.name.clone()).collect());
        for name in ["Detected test", "Dynamic tests"] {
            assert!(names.iter().any(|listed| listed == name), "{name} in {names:?}");
        }
        run_action(cx, "run.chooseConfig", None);
        wait(cx, "the chooser opens", |cx| workbench.read_with(cx, |w, _| w.config_chooser().is_some()));
        let chooser = workbench.read_with(cx, |w, _| w.config_chooser()).expect("the chooser");
        let lines = chooser.read_with(cx, |chooser, _| chooser.lines());
        for name in ["Detected test", "Dynamic tests"] {
            assert!(lines.iter().any(|line| line == name), "{name} in {lines:?}");
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Gutter

    // src/features/plugins/languageVisuals.test.ts
    /// A plugin's gutter action shows on its line as its icon; an edit takes it away until the
    /// plugin answered for the new text; a click runs its command with its argument.
    #[gpui_kit::test]
    async fn renders_owned_gutter_commands_and_drops_answers_to_superseded_buffers(cx: &mut TestAppContext) {
        let home = start(cx);
        let folder = plugin_copy(home.path(), "language-features", "language-features", str::to_owned);
        let id = link(cx, &folder, &[], "run");
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\ntwo\n")]);
        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        let markers = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.plugin_gutter_markers(&editor));
        wait(cx, "the gutter action shows", |cx| !markers(cx).is_empty());
        let [marker] = markers(cx).try_into().expect("one marker");
        assert_eq!((marker.line, marker.icon.as_str(), marker.title.as_str()), (0, "gutter-run", "Run fixture"));
        editor.update(cx, |editor, cx| editor.insert(Some(0..0), "zero\n", cx));
        assert!(markers(cx).is_empty(), "the answer was for the old text");
        wait(cx, "the action shows again", |cx| !markers(cx).is_empty());
        wait(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        click(cx, "plugin-gutter:0");
        wait(cx, "the command ran", |cx| {
            run_in_window(cx, &root, &id, "state", None).ok().flatten().as_deref() == Some("gutter")
        });
    }

    // src/features/plugins/languageVisuals.test.ts
    /// Two plugins put an action on the same line: one icon, whose click opens a menu of both; the
    /// chosen entry runs in its own plugin only.
    #[gpui_kit::test]
    async fn puts_the_actions_of_one_line_behind_one_icon_whose_click_opens_a_menu(cx: &mut TestAppContext) {
        let home = start(cx);
        let first = plugin_copy(home.path(), "language-features", "language-features", str::to_owned);
        let second = plugin_copy(home.path(), "language-features", "language-features-two", |text| {
            text.replace("id = \"test.language-features\"", "id = \"test.language-features-two\"")
        });
        let one = link(cx, &first, &[], "run");
        let two = link(cx, &second, &[], "run");
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one\n")]);
        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        wait(cx, "both actions show", |cx| {
            workbench
                .read_with(cx, |w, _| w.plugin_gutter_markers(&editor).first().is_some_and(|m| m.actions.len() == 2))
        });
        wait(cx, "the icon is drawn", |cx| cx.debug_bounds("plugin-gutter:0").is_some());
        click(cx, "plugin-gutter:0");
        let menu = workbench.read_with(cx, |w, _| w.open_menu::<LineAction>()).expect("the menu is open");
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
        wait(cx, "the chosen plugin ran", |cx| {
            run_in_window(cx, &root, &chosen, "state", None).ok().flatten().as_deref() == Some("gutter")
        });
        assert_eq!(run_in_window(cx, &root, &other, "state", None).expect("it answers"), None);
    }

    // ---------------------------------------------------------------------------------------------
    // Menus

    /// The Hello fixture with entries in the Tools menu, the editor's menu (a submenu) and the
    /// Project view's menu, as the Bazel plugin declares them; one command only for Bazel
    /// workspaces.
    fn hello_menus(text: &str) -> String {
        format!(
            "{text}\n\
             [[contributes.commands]]\nid = \"sync\"\ntitle = \"Hello: Sync\"\nwhen = \"workspaceContains:MODULE.bazel\"\n\n\
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
        let home = start(cx);
        let folder = plugin_copy(home.path(), "hello", "hello", hello_menus);
        link(cx, &folder, &[], "sync");
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one two\n")]);
        let state = |cx: &mut VisualTestContext| cx.update(|window, cx| menu::state(window, cx));
        wait(cx, "the key is answered", |cx| {
            context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(false))
        });
        let shown = state(cx);
        let items = shown.dynamic.get(DYNAMIC_PLUGIN_TOOLS).cloned().unwrap_or_default();
        let labels: Vec<(&str, &str)> = items.iter().map(|item| (item.label.as_str(), item.action.as_str())).collect();
        assert_eq!(labels, [("Hello: Sync", "plugin.hello.sync"), ("Hello: Insert Date", "plugin.hello.insert-date")]);
        assert!(shown.disabled.contains("plugin.hello.sync"), "greyed out without MODULE.bazel");
        assert!(!shown.disabled.contains("plugin.hello.insert-date"));
        std::fs::write(root.join("MODULE.bazel"), "module(name = \"x\")\n").expect("the file is written");
        wait(cx, "the entry comes on", |cx| !state(cx).disabled.contains("plugin.hello.sync"));

        let editor = open_file(&workbench, root.join("notes.txt"), cx);
        cx.dispatch_action(MenuCommand { action: "plugin.hello.insert-date".to_owned(), arg: None });
        static DATE: LazyLock<regex::Regex> =
            LazyLock::new(|| regex::Regex::new(r"\d{4}-\d{2}-\d{2}").expect("the date pattern is valid"));
        wait(cx, "the plugin inserts the date", |cx| editor.read_with(cx, |e, _| DATE.is_match(&e.text())));
    }

    /// The Project view's menu has the plugin's entry once its `when` holds; choosing it runs the
    /// command, whose notification appears. The editor's menu groups the plugin's two entries of
    /// one submenu under it.
    #[gpui_kit::test]
    async fn the_context_menus_offer_plugin_entries_where_their_when_holds_and_run_them(cx: &mut TestAppContext) {
        use crate::project_view::context_menu::ProjectCommand;
        let home = start(cx);
        let folder = plugin_copy(home.path(), "hello", "hello", hello_menus);
        link(cx, &folder, &[], "word-count");
        let (_folder, root, workbench, cx) = project(cx, &[("notes.txt", "one two three\n")]);
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
            wait(cx, "the tree lists the file", |cx| tree.update(cx, |tree, cx| tree.reveal_path(&at, cx)));
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
        wait(cx, "the key is answered", |cx| {
            context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(false))
        });
        let lines = open_tree_menu(cx);
        assert!(!lines.iter().any(|line| line.trim_start_matches("> ") == entry), "{lines:?}");
        cx.simulate_keystrokes("escape");
        std::fs::write(root.join("MODULE.bazel"), "module(name = \"x\")\n").expect("the file is written");
        wait(cx, "the key holds", |cx| context("workspaceContains:MODULE.bazel", cx) == Some(ContextValue::Bool(true)));
        let lines = open_tree_menu(cx);
        let at = lines.iter().position(|line| line.trim_start_matches("> ") == entry).expect("the plugin's entry");
        // The entry follows Delete…, as the menu's plugin group does.
        let delete = lines.iter().position(|line| line.trim_start_matches("> ") == "Delete…").expect("Delete…");
        assert!(at > delete, "{lines:?}");
        click(cx, &format!("menu-row-0-{at}"));
        wait(cx, "the plugin's notification", |cx| {
            notifications(&workbench, cx).iter().any(|(title, _)| title == "Word count: notes.txt")
        });
    }

    // ---------------------------------------------------------------------------------------------
    // Settings pages

    /// The plugin's page in Settings shows its setting; ticked and saved with OK, the value is in
    /// the global settings and the plugin hears `settings-changed` with its key.
    #[gpui_kit::test]
    async fn a_plugins_settings_page_saves_its_value_and_the_plugin_hears_the_change(cx: &mut TestAppContext) {
        use crate::settings_model::schema::settings_value;
        let home = start(cx);
        let folder = plugin_copy(home.path(), "api14", "api14", str::to_owned);
        link(cx, &folder, &["events"], "events");
        let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
        // The plugin runs once, so it is live when the event comes.
        run_in_window(cx, &root, "api14", "events", None).expect("it answers");
        run_action(cx, "file.settings", Some(Value::from("plugins.api14")));
        wait(cx, "the page shows", |cx| cx.debug_bounds("plugin-setting:enabled").is_some());
        let dialog = workbench.read_with(cx, |w, _| w.settings_dialog().cloned()).expect("Settings is open");
        assert_eq!(dialog.read_with(cx, |d, cx| d.selected_page(cx)).as_deref(), Some("plugins.api14"));
        click(cx, "plugin-setting:enabled");
        click(cx, "settings-ok");
        wait(cx, "Settings closes", |cx| workbench.read_with(cx, |w, _| w.settings_dialog().is_none()));
        let stored = cx.update(|_, cx| settings_value(&crate::platform::settings::global(cx)));
        assert_eq!(stored.pointer("/plugins/api14/enabled"), Some(&Value::Bool(true)));
        wait(cx, "the plugin heard the change", |cx| {
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
        let home = start(cx);
        let folder = plugin_copy(home.path(), "settings-pages", "settings-pages", str::to_owned);
        link(cx, &folder, &[], "test");
        let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
        run_action(cx, "file.settings", Some(Value::from("plugins.settings-pages")));
        wait(cx, "the plugin's page lists its pages", |cx| {
            cx.debug_bounds("plugin-settings-link:plugins.settings-pages.settings:llm").is_some()
        });
        click(cx, "plugin-settings-link:plugins.settings-pages.settings:llm");
        wait(cx, "the LLM page shows", |cx| cx.debug_bounds("plugin-setting-add:profiles").is_some());
        click(cx, "plugin-settings-action:0");
        wait(cx, "the error shows under the button", |cx| cx.debug_bounds("plugin-settings-action-error:0").is_some());
        click(cx, "plugin-setting-add:profiles");
        click(cx, "plugin-settings-action:0");
        wait(cx, "the plugin read the row", |cx| {
            notifications(&workbench, cx)
                .iter()
                .any(|(title, body)| title == "Model Profiles" && body.as_deref() == Some("Read 1 saved profile(s)"))
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
        let home = start(cx);
        let folder = plugin_copy(home.path(), "path-events", "path-events", str::to_owned);
        link(cx, &folder, &["events", "project.read"], "count");
        let (_folder, root, workbench, cx) = project(cx, &[("config/app.yaml", "a: 1\n")]);
        let count =
            |cx: &mut VisualTestContext| run_in_window(cx, &root, "test.path-events", "count", None).ok().flatten();
        assert_eq!(count(cx), None, "nothing heard yet");
        run_in_window(cx, &root, "test.path-events", "filter", Some("nothing/*.none")).expect("the filter is set");
        let editor = open_file(&workbench, root.join("config/app.yaml"), cx);
        editor.update(cx, |editor, cx| editor.insert(Some(0..0), "b: 2\n", cx));
        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
        cx.executor().spawn(saving).detach();
        wait(cx, "the file is saved", |cx| !editor.read_with(cx, |editor, _| editor.is_dirty()));
        wait(cx, "the plugin heard the save", |cx| count(cx).as_deref() == Some("1"));
    }

    /// The editor's input mode reaches the plugin that wants `input-mode-changed`.
    #[gpui_kit::test]
    async fn an_input_mode_change_reaches_the_plugin_that_wants_it(cx: &mut TestAppContext) {
        let home = start(cx);
        let folder = plugin_copy(home.path(), "api14", "api14", str::to_owned);
        link(cx, &folder, &["events.input"], "events");
        let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
        run_in_window(cx, &root, "api14", "events", None).expect("it answers");
        let path = root.join("a.txt").to_string_lossy().into_owned();
        // The window learns which events plugins want before it reports one.
        wait(cx, "the event kinds are known", |cx| {
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
        let home = start(cx);
        let activated = |id: &'static str, activation: &'static str| {
            move |text: &str| {
                text.replace("id = \"hello\"", &format!("id = \"{id}\"")).replace(
                    "activation = [\"on-command:*\", \"on-view:greetings\"]",
                    &format!("activation = [\"{activation}\"]"),
                )
            }
        };
        let by_language =
            plugin_copy(home.path(), "hello", "hello-language", activated("hello-language", "on-language:plaintext"));
        let by_file = plugin_copy(home.path(), "hello", "hello-file", activated("hello-file", "on-file:notes/*.md"));
        link(cx, &by_language, &[], "word-count");
        link(cx, &by_file, &[], "word-count");
        let (_folder, root, workbench, cx) = project(cx, &[("a.txt", "a\n"), ("notes/today.md", "# Today\n")]);
        assert_eq!(status_of("hello-language", cx), Some(PluginStatus::Inactive));
        assert_eq!(status_of("hello-file", cx), Some(PluginStatus::Inactive));
        open_file(&workbench, root.join("a.txt"), cx);
        wait(cx, "the language's plugin loads", |cx| status_of("hello-language", cx) == Some(PluginStatus::Active));
        assert_eq!(status_of("hello-file", cx), Some(PluginStatus::Inactive));
        open_file(&workbench, root.join("notes/today.md"), cx);
        wait(cx, "the file's plugin loads", |cx| status_of("hello-file", cx) == Some(PluginStatus::Active));
    }

    // src/features/plugins/__tests__/plugins.test.tsx
    /// An action run from a menu with a shortcut reaches the plugin that wants `action-performed`,
    /// which tells the shortcut in a notification.
    #[gpui_kit::test]
    async fn reports_actions_to_action_performed_plugins(cx: &mut TestAppContext) {
        use crate::platform::input::Keymap;
        use crate::platform::keymap::is_mac_host;
        let home = start(cx);
        let folder = plugin_copy(home.path(), "key-promoter", "key-promoter", str::to_owned);
        link(cx, &folder, &[], "statistics");
        let (_folder, _root, workbench, cx) = project(cx, &[("a.txt", "a\n")]);
        const ACTION: &str = "navigate.gotoFile";
        let (title, shortcut) = cx.update(|_, cx| {
            let title = cx.global::<ActionRegistry>().get(ACTION).expect("registered").meta.title.clone();
            let shortcut = cx.global::<Keymap>().0.shortcut_label(ACTION, is_mac_host()).expect("a shortcut");
            (title, shortcut)
        });
        let expected = format!("{title}: {shortcut}");
        wait(cx, "the plugin hears the action", |cx| {
            cx.update(|window, cx| {
                execute_action_from(ActionSource::Menu, ACTION, None, window, cx);
            });
            cx.run_until_parked();
            workbench.update_in(cx, |w, window, cx| w.close_popup(window, cx));
            notifications(&workbench, cx).iter().any(|(t, _)| *t == expected)
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
        let home = start(cx);
        let bin = home.path().join("bin");
        // Far longer than any wait: the task can only leave the list by being cancelled.
        program(&bin, "checkstyle", "#!/bin/sh\nexec /bin/sleep 600\n");
        programs_in(&bin, cx);
        let folder = plugin_copy(home.path(), "checkstyle", "checkstyle", str::to_owned);
        let id = link(cx, &folder, &[], "check");
        let (_folder, root, workbench, cx) = project(cx, &[("src/Main.java", "class Main {}\n")]);
        open_file(&workbench, root.join("src/Main.java"), cx);
        run_action(cx, &plugin_action_id(&id, "check"), None);
        let title = "Checkstyle: Main.java";
        wait(cx, "the task is listed", |cx| {
            workbench.read_with(cx, |w, _| w.tasks().tasks().iter().any(|task| task.title == title))
        });
        let item: &'static str = Box::leak(format!("status:{TASKS_ITEM}").into_boxed_str());
        wait(cx, "the tasks item is drawn", |cx| cx.debug_bounds(item).is_some());
        click(cx, item);
        let popup = workbench.read_with(cx, |w, _| w.background_tasks_popup()).expect("Background Tasks is open");
        let rows = popup.read_with(cx, |popup, _| popup.rows());
        let index = rows.iter().position(|row| row.title == title).expect("the plugin's task is listed");
        assert_eq!(rows[index].cancel, CancelButton::Working);
        click(cx, &format!("task-cancel:{index}"));
        let row = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, _| popup.rows().into_iter().find(|row| row.title == title))
        };
        assert_eq!(row(cx).map(|row| row.cancel), Some(CancelButton::Disabled("Cancelling…".to_owned())));
        wait(cx, "the task ends", |cx| row(cx).is_none());
        run_action(cx, BACKGROUND_TASKS, None);
        assert!(workbench.read_with(cx, |w, _| w.background_tasks_popup().is_none()));
    }
}
