//! Run configurations in the Services tool window (IDEA's Run Dashboard): Spring Boot by default,
//! the configuration types added there ("Add Service › Run Configuration Type"), the types of this
//! window's run sessions and the configurations that ran before (kept per project across restarts),
//! one group each, with run / debug / stop / rerun per configuration.
//!
//! [`RunServices`] holds what the project keeps about its Services (added types, remembered runs,
//! hidden configurations, display order); a change returns the [`StorageWrite`]s to make.
//! [`Services`] reads the tree, its toolbar and its menus from the run store, the debugger's
//! sessions and that state. What a toolbar button or a menu entry does is a [`ServiceCommand`] or a
//! [`MenuCommand`] for the caller to carry out; Run All / Restart All ([`StartAll`]) and the cleanup
//! after a removed workspace member ([`RemovedProjectCleanup`]) hand out their steps one at a time.

use crate::debugger::state::{ServiceSession as DebugServiceSession, SessionStatus as DebugStatus};
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::run::application_update::is_updatable;
use crate::run::config_types::RunConfigTypes;
use crate::run::configs::{CATEGORY_ORDER, KIND_LABELS, category_of, command_line, config_icon, cwd_of, kind_label};
use crate::run::store::{
    Persist, RunSession, RunStore, SessionStatus, StartOptions, StopAction, StorageWrite, parse_ids,
};
use crate::run::types::is_debug_only;
use crate::run_testing::service_sessions::{
    ServiceSession, current_service_session, current_service_sessions, service_sessions,
};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::DebugSessionId;
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};

/// Id of the Services provider that lists run configurations.
pub const RUN_PROVIDER: &str = "runConfigurations";
/// Key prefix of a configuration type's group node; the kind follows.
pub const TYPE_PREFIX: &str = "run:type:";
/// Key prefix of a configuration's node; the configuration id follows.
pub const CONFIG_PREFIX: &str = "run:config:";
/// Storage key prefixes of the added types and the remembered runs; the project root follows.
pub const TYPES_KEY: &str = "openidea.services.runTypes:";
pub const RAN_KEY: &str = "openidea.services.ran:";
/// Storage key prefixes of the hidden configurations and the display order; the project location
/// follows.
pub const HIDDEN_KEY: &str = "openidea.services.runHidden:";
pub const ORDER_KEY: &str = "openidea.services.runOrder:";
/// Types shown in Services until the user changes the list.
pub const DEFAULT_TYPES: [&str; 1] = ["spring-boot"];

/// Display order of configuration ids, by kind.
pub type ServiceOrder = BTreeMap<String, Vec<String>>;
/// Hidden configuration ids, by kind.
pub type HiddenConfigs = BTreeMap<String, Vec<String>>;

// ---------------------------------------------------------------------------------------------------
// What a project keeps about its Services

pub fn types_key(root: &str) -> String {
    format!("{TYPES_KEY}{root}")
}

pub fn ran_key(root: &str) -> String {
    format!("{RAN_KEY}{root}")
}

pub fn hidden_key(location: &str) -> String {
    format!("{HIDDEN_KEY}{location}")
}

pub fn order_key(location: &str) -> String {
    format!("{ORDER_KEY}{location}")
}

/// The project's added types: Spring Boot until the user changes the list. A stored list that is
/// not an array reads as none.
pub fn read_types(stored: Option<&str>) -> Vec<String> {
    match stored.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) {
        None => DEFAULT_TYPES.iter().map(|kind| kind.to_string()).collect(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        Some(_) => Vec::new(),
    }
}

/// The project's previously run configuration ids, most recent last.
pub fn read_ran(stored: Option<&str>) -> Vec<String> {
    parse_ids(stored)
}

/// Lists of ids stored by kind; anything that is not a list of strings is skipped. `unique` also
/// drops empty and repeated ids.
fn read_ids_by_kind(stored: Option<&str>, unique: bool) -> BTreeMap<String, Vec<String>> {
    let Some(Value::Object(entries)) = stored.and_then(|raw| serde_json::from_str(raw).ok()) else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    for (kind, ids) in entries {
        let Value::Array(ids) = ids else { continue };
        let mut list: Vec<String> = Vec::new();
        for id in ids.iter().filter_map(Value::as_str) {
            if !unique || (!id.is_empty() && !list.iter().any(|known| known == id)) {
                list.push(id.to_string());
            }
        }
        out.insert(kind, list);
    }
    out
}

/// The project's hidden configuration ids, by kind.
pub fn read_hidden(stored: Option<&str>) -> HiddenConfigs {
    read_ids_by_kind(stored, false)
}

/// The project's display order, by kind.
pub fn read_service_order(stored: Option<&str>) -> ServiceOrder {
    read_ids_by_kind(stored, true)
}

/// `entries` in the saved order; those it does not list follow, in the order they came.
pub fn order_services<'a>(mut entries: Vec<&'a RunConfig>, saved: &[String]) -> Vec<&'a RunConfig> {
    entries.sort_by_key(|entry| saved.iter().rposition(|id| *id == entry.id).unwrap_or(saved.len()));
    entries
}

/// Where a dragged node is dropped, relative to the node under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    After,
}

/// The saved order of a group after `source` was dropped before or after `target`. `ids` are the
/// group's configurations as shown; the order then lists all of them.
pub fn move_service_order(
    saved: &[String],
    ids: &[String],
    source: &str,
    target: &str,
    position: DropPosition,
) -> Vec<String> {
    let listed = |id: &str| ids.iter().any(|known| known == id);
    if source == target || !listed(source) || !listed(target) {
        return saved.to_vec();
    }
    let mut order: Vec<String> = Vec::new();
    for id in saved.iter().chain(ids) {
        if id != source && !order.contains(id) {
            order.push(id.clone());
        }
    }
    let after = usize::from(position == DropPosition::After);
    let index = order.iter().position(|id| id == target).map_or(order.len(), |index| index + after);
    order.insert(index, source.to_string());
    order
}

/// The display order could not be stored.
pub fn order_not_saved() -> NotificationRequest {
    let mut notification = NotificationRequest::new(NotificationLevel::Warning, "Cannot save service order");
    notification.group = Some("run".to_string());
    notification.body = Some("The order will be kept for this session only.".to_string());
    notification
}

/// The stored values of a project's Services, each under its key ([`types_key`], [`ran_key`],
/// [`hidden_key`], [`order_key`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoredServices<'a> {
    pub types: Option<&'a str>,
    pub ran: Option<&'a str>,
    pub hidden: Option<&'a str>,
    pub order: Option<&'a str>,
}

/// The configurations of a kind to choose from, for the "Add Service" picker of a type group.
#[derive(Clone, Debug, PartialEq)]
pub struct ServicePick {
    pub kind: String,
    pub configs: Vec<RunConfig>,
    /// Ids of the configurations shown now (preselected in the picker).
    pub selected: Vec<String>,
    /// The project the picker was opened for.
    location: Option<String>,
}

/// What "Add Service" on a type group does.
#[derive(Clone, Debug, PartialEq)]
pub enum AddService {
    /// At most one configuration: it is shown again and its type added.
    Applied(Vec<StorageWrite>),
    /// Ask which of the kind's configurations show, then [`RunServices::services_picked`].
    Pick(ServicePick),
}

/// What the open project keeps about its Services. Without a project nothing is stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunServices {
    root: Option<String>,
    location: Option<String>,
    types: Vec<String>,
    ran: Vec<String>,
    hidden: HiddenConfigs,
    order: ServiceOrder,
}

impl RunServices {
    /// The Services of a project. `root` keys its added types and remembered runs; `location` (the
    /// root, or where a remote project lives) keys its hidden configurations and display order.
    pub fn open(root: Option<&str>, location: Option<&str>, stored: StoredServices<'_>) -> Self {
        Self {
            root: root.map(str::to_string),
            location: location.map(str::to_string),
            types: if root.is_some() { read_types(stored.types) } else { Vec::new() },
            ran: if root.is_some() { read_ran(stored.ran) } else { Vec::new() },
            hidden: if location.is_some() { read_hidden(stored.hidden) } else { HiddenConfigs::new() },
            order: if location.is_some() { read_service_order(stored.order) } else { ServiceOrder::new() },
        }
    }

    pub fn root(&self) -> Option<&str> {
        self.root.as_deref()
    }

    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    /// The types added to Services.
    pub fn types(&self) -> &[String] {
        &self.types
    }

    /// The ids of the configurations that ran, most recent last.
    pub fn ran(&self) -> &[String] {
        &self.ran
    }

    pub fn hidden(&self) -> &HiddenConfigs {
        &self.hidden
    }

    pub fn order(&self) -> &ServiceOrder {
        &self.order
    }

    fn write_types(&self) -> Option<StorageWrite> {
        let root = self.root.as_deref()?;
        Some(StorageWrite { key: types_key(root), value: Some(json!(self.types).to_string()) })
    }

    fn write_ran(&self) -> Option<StorageWrite> {
        let root = self.root.as_deref()?;
        Some(StorageWrite { key: ran_key(root), value: Some(json!(self.ran).to_string()) })
    }

    fn write_hidden(&self) -> Option<StorageWrite> {
        let location = self.location.as_deref()?;
        Some(StorageWrite { key: hidden_key(location), value: Some(json!(self.hidden).to_string()) })
    }

    pub fn add_type(&mut self, kind: &str) -> Option<StorageWrite> {
        if self.types.iter().any(|added| added == kind) {
            return None;
        }
        self.types.push(kind.to_string());
        self.write_types()
    }

    pub fn remove_type(&mut self, kind: &str) -> Option<StorageWrite> {
        self.types.retain(|added| added != kind);
        self.write_types()
    }

    /// Records a run, so its configuration is back in Services after a restart.
    pub fn record_run(&mut self, config_id: &str) -> Option<StorageWrite> {
        self.ran.retain(|id| id != config_id);
        self.ran.push(config_id.to_string());
        self.write_ran()
    }

    /// Forgets the remembered runs `keep` rejects.
    fn retain_ran(&mut self, keep: impl Fn(&str) -> bool) -> Option<StorageWrite> {
        self.root.as_ref()?;
        let before = self.ran.len();
        self.ran.retain(|id| keep(id));
        if self.ran.len() == before { None } else { self.write_ran() }
    }

    /// Removes one configuration from Services; it reappears while it runs (its console stays
    /// reachable).
    pub fn hide_config(&mut self, kind: &str, id: &str) -> Option<StorageWrite> {
        let hidden = self.hidden.entry(kind.to_string()).or_default();
        if hidden.iter().any(|known| known == id) {
            return None;
        }
        hidden.push(id.to_string());
        self.write_hidden()
    }

    /// "Add Service" on a type group: with several configurations of the kind (`configs` are all
    /// the project's), ask which of them show.
    pub fn add_service(&mut self, kind: &str, configs: &[RunConfig]) -> AddService {
        let configs: Vec<RunConfig> = configs.iter().filter(|config| config.kind == kind).cloned().collect();
        if configs.len() <= 1 {
            self.hidden.insert(kind.to_string(), Vec::new());
            let shown = self.write_hidden();
            return AddService::Applied(shown.into_iter().chain(self.add_type(kind)).collect());
        }
        let hidden = self.hidden.get(kind).map(Vec::as_slice).unwrap_or_default();
        let selected = configs.iter().filter(|config| !hidden.contains(&config.id)).map(|c| c.id.clone()).collect();
        AddService::Pick(ServicePick { kind: kind.to_string(), configs, selected, location: self.location.clone() })
    }

    /// The picker's answer: the ids to show, `None` when it was dismissed. Nothing changes when
    /// another project was opened meanwhile.
    pub fn services_picked(&mut self, pick: &ServicePick, picked: Option<&[String]>) -> Vec<StorageWrite> {
        let Some(picked) = picked.filter(|_| self.location == pick.location) else { return Vec::new() };
        let added = self.add_type(&pick.kind);
        let unpicked = pick.configs.iter().filter(|config| !picked.contains(&config.id));
        self.hidden.insert(pick.kind.clone(), unpicked.map(|config| config.id.clone()).collect());
        added.into_iter().chain(self.write_hidden()).collect()
    }

    /// Replaces the display order (see [`Services::moved`]). When the write fails, tell with
    /// [`order_not_saved`].
    pub fn set_order(&mut self, order: ServiceOrder) -> Option<StorageWrite> {
        let location = self.location.as_deref()?;
        let write = StorageWrite { key: order_key(location), value: Some(json!(order).to_string()) };
        self.order = order;
        Some(write)
    }
}

// ---------------------------------------------------------------------------------------------------
// Tree

/// A node of the Services tree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceTreeItem {
    pub key: String,
    pub label: String,
    pub icon: String,
    /// Grey text after the label (state, port, count).
    pub detail: Option<String>,
    /// Greyed label (not running).
    pub dim: bool,
    /// Expanded the first time the node appears.
    pub expanded: bool,
    /// `None` for leaves.
    pub children: Option<Vec<ServiceTreeItem>>,
}

/// The state of a Run session in words; `None` without a session.
pub fn session_status(session: Option<&RunSession>) -> Option<String> {
    let session = session?;
    let status = match (session.status, session.exit_code) {
        (SessionStatus::Starting, _) => "Starting",
        (status, _) if status.is_live() => "Running",
        _ if session.stopped => "Stopped",
        (SessionStatus::Failed, Some(code)) => return Some(format!("Failed (exit {code})")),
        (SessionStatus::Failed, None) => "Failed",
        _ => "Finished",
    };
    Some(status.to_string())
}

/// A node's grey text: the HTTP port of a running web application, else the state.
pub fn session_detail(session: Option<&RunSession>) -> Option<String> {
    match session.filter(|session| session.is_live()).and_then(|session| session.http_port) {
        Some(port) if port > 0 => Some(format!(":{port}")),
        _ => session_status(session),
    }
}

fn execution_detail(session: Option<&ServiceSession>) -> Option<String> {
    let session = session?;
    let Some(debug) = session.debugger() else {
        return match &session.run {
            Some(run) => session_detail(Some(run)),
            None => Some("Finished".to_string()),
        };
    };
    if debug.status == DebugStatus::Paused {
        return Some("Suspended".to_string());
    }
    // The application's HTTP port: a Run owns the console for an attached debugger, a direct debug
    // launch announces it on the debug session's own console.
    let run_port = session.run.as_ref().filter(|run| run.is_live()).and_then(|run| run.http_port);
    match run_port.or(debug.http_port) {
        Some(port) if port > 0 => Some(format!(":{port}")),
        _ if debug.status == DebugStatus::Starting => Some("Starting".to_string()),
        _ => Some("Debugging".to_string()),
    }
}

/// What the tree is built from.
#[derive(Clone, Copy)]
pub struct TreeInput<'a> {
    pub configs: &'a [RunConfig],
    pub sessions: &'a [RunSession],
    /// The debugger's configured roots, including finished ones.
    pub debug: &'a [DebugServiceSession],
    /// The types added to Services.
    pub added: &'a [String],
    /// Ids of the configurations that ran before.
    pub ran: &'a [String],
    pub order: &'a ServiceOrder,
    pub hidden: &'a HiddenConfigs,
    /// Whether the configuration with this id was deleted since it was listed.
    pub removed: &'a dyn Fn(&str) -> bool,
}

/// A kind and its visible configurations, in display order.
type KindGroup<'a> = (String, Vec<&'a RunConfig>);

/// The groups of the tree. `latest` holds the execution shown for each configuration of
/// `executions`.
fn kind_groups<'a>(
    input: TreeInput<'a>,
    executions: &'a [ServiceSession],
    latest: &[&'a ServiceSession],
) -> Vec<KindGroup<'a>> {
    let shown = |session: &ServiceSession| session.is_live() || !(input.removed)(&session.config.id);
    let mut kinds: Vec<&'a str> = input.added.iter().map(String::as_str).collect();
    for session in executions {
        if shown(session) && !kinds.contains(&session.config.kind.as_str()) {
            kinds.push(&session.config.kind);
        }
    }
    for id in input.ran {
        if let Some(config) = input.configs.iter().find(|config| config.id == *id)
            && !kinds.contains(&config.kind.as_str())
        {
            kinds.push(&config.kind);
        }
    }
    let mut groups = Vec::new();
    for kind in kinds {
        let of_kind = |config: &&'a RunConfig| config.kind == kind;
        let mut entries: Vec<&'a RunConfig> = if input.added.iter().any(|added| added == kind) {
            input.configs.iter().filter(of_kind).collect()
        } else {
            let ran = input.ran.iter().filter_map(|id| input.configs.iter().find(|config| config.id == *id));
            ran.filter(of_kind).collect()
        };
        for &session in latest {
            let listed = entries.iter().any(|config| config.id == session.config.id);
            if session.config.kind == kind && shown(session) && !listed {
                entries.push(&session.config);
            }
        }
        // Hidden configurations reappear only while they run (their console stays reachable).
        let hidden = input.hidden.get(kind).map(Vec::as_slice).unwrap_or_default();
        let live = |id: &str| latest.iter().any(|session| session.config.id == id && session.is_live());
        entries.retain(|config| !hidden.contains(&config.id) || live(&config.id));
        // An empty group stays only when its existing configurations can be restored.
        let restorable = input.configs.iter().any(|config| config.kind == kind);
        if entries.is_empty() && DEFAULT_TYPES.contains(&kind) && !restorable {
            continue;
        }
        let saved = input.order.get(kind).map(Vec::as_slice).unwrap_or_default();
        groups.push((kind.to_string(), order_services(entries, saved)));
    }
    groups
}

/// Groups by configuration type: added types list all their configurations, other types only those
/// that ran. A default type (Spring Boot) without configurations is hidden. A configuration deleted
/// since it ran stays only while its run is live.
pub fn run_service_items(input: TreeInput<'_>, types: &RunConfigTypes, i18n: &mut I18n) -> Vec<ServiceTreeItem> {
    let executions = service_sessions(input.sessions, input.debug);
    let latest = current_service_sessions(&executions);
    let execution = |id: &str| latest.iter().copied().find(|session| session.config.id == id);
    let mut items = Vec::new();
    for (kind, configs) in kind_groups(input, &executions, &latest) {
        let mut running = 0usize;
        let mut children = Vec::new();
        for config in configs {
            let session = execution(&config.id);
            let live = session.is_some_and(ServiceSession::is_live);
            running += usize::from(live);
            let icon = if session.and_then(ServiceSession::debugger).is_some() {
                "debug".to_string()
            } else if live {
                "run".to_string()
            } else {
                config_icon(&config.kind, config.is_test, types)
            };
            children.push(ServiceTreeItem {
                key: format!("{CONFIG_PREFIX}{}", config.id),
                label: config.name.clone(),
                icon,
                detail: execution_detail(session),
                dim: !live,
                expanded: false,
                children: None,
            });
        }
        items.push(ServiceTreeItem {
            key: format!("{TYPE_PREFIX}{kind}"),
            label: kind_label(&kind, types),
            icon: config_icon(&kind, false, types),
            detail: (running > 0).then(|| i18n.tx_built(&["", " running"], &[Param::from(running)])),
            dim: false,
            expanded: true,
            children: Some(children),
        });
    }
    items
}

// ---------------------------------------------------------------------------------------------------
// Toolbar, menus and what they do

/// A toolbar button the Services tree shows for the selected node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceItemAction {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub disabled: bool,
}

fn item_action(id: &'static str, title: &'static str, icon: &'static str, disabled: bool) -> ServiceItemAction {
    ServiceItemAction { id, title, icon, disabled }
}

/// A running web application to open in the browser.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UrlChoice {
    /// The configuration's name and the URL, shown as written.
    pub label: String,
    pub url: String,
}

/// What a toolbar button, or activating a node, does.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceCommand {
    /// Run All (`restart: false`) or Restart All of a type group: drive [`StartAll`].
    StartAll {
        kind: String,
        restart: bool,
    },
    /// Stop these executions: drive
    /// [`StopServiceSessions`](crate::run_testing::service_sessions::StopServiceSessions).
    Stop(Vec<ServiceSession>),
    OpenUrl(String),
    /// Several web applications run: offer them in a menu at the centre of the window.
    ChooseUrl(Vec<UrlChoice>),
    /// Start the configuration; its output stays in Services instead of opening the Run tool window.
    StartRun {
        config: Box<RunConfig>,
        options: Box<StartOptions>,
    },
    /// Rerun the debugger root without opening the Debug tool window.
    RerunDebugger(DebugSessionId),
    /// Debug the configuration without opening the Debug tool window.
    DebugConfig(Box<RunConfig>),
    /// Update the running application of this session: a Run session id, or `debug:<id>` for a
    /// debugger root without one.
    UpdateApplication(Option<String>),
}

/// What an entry of a node's context menu does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuCommand {
    /// The toolbar button with this id: [`Services::invoke`].
    Action(&'static str),
    /// "Add Service" on the group of this kind: [`RunServices::add_service`].
    AddService(String),
    /// Remove the type from Services: [`RunServices::remove_type`].
    RemoveType(String),
    /// Make the Run session the active tab and show the Run tool window.
    ShowRun(String),
    /// Show the debugger root in the Debug tool window.
    ShowDebug(DebugSessionId),
    /// Open the configuration in the Run/Debug Configurations dialog.
    EditConfig(String),
    /// Remove the configuration from Services: [`RunServices::hide_config`].
    HideConfig { kind: String, id: String },
}

/// An entry of a node's context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceMenuItem {
    Separator,
    Item { label: &'static str, icon: &'static str, disabled: bool, command: Option<MenuCommand> },
}

fn menu_item(label: &'static str, icon: &'static str, command: Option<MenuCommand>) -> ServiceMenuItem {
    ServiceMenuItem::Item { label, icon, disabled: command.is_none(), command }
}

/// What a node's key stands for.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub kind: Option<String>,
    /// `None` for a type group.
    pub config: Option<RunConfig>,
    /// The execution shown for the configuration.
    pub session: Option<ServiceSession>,
    /// The executions of a group's configurations, or the configuration's own.
    pub sessions: Vec<ServiceSession>,
}

/// What the detail pane shows for a node.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceDetail {
    /// A type group: an empty state titled with the type's name, saying how many configurations it
    /// has.
    Group { title: String },
    /// The configuration ran: the console of its Run session, as in the Run tool window. A live
    /// `debugger` owns Rerun, Debug and Stop.
    RunConsole { session_id: String, debugger: Option<Box<DebugServiceSession>> },
    /// A direct debug launch: the debugger's console.
    DebugConsole(DebugSessionId),
    /// Not started: the configuration's facts, with Run and Debug buttons.
    NotStarted { config: Box<RunConfig>, type_label: String, command_line: String, run_disabled: bool },
}

/// The web applications among `sessions`, by URL, each named after its configuration.
fn browser_urls(sessions: &[ServiceSession]) -> Vec<(String, String)> {
    let mut urls: Vec<(String, String)> = Vec::new();
    for session in sessions.iter().filter(|session| session.is_live()) {
        let run_port = session.run.as_ref().filter(|run| run.is_live()).and_then(|run| run.http_port);
        let port = run_port.or_else(|| session.debugger().and_then(|debug| debug.http_port));
        let Some(port) = port.filter(|port| *port > 0) else { continue };
        let url = format!("http://localhost:{port}");
        match urls.iter_mut().find(|(known, _)| *known == url) {
            Some((_, name)) => name.clone_from(&session.config.name),
            None => urls.push((url, session.config.name.clone())),
        }
    }
    urls
}

fn browser_command(urls: &[(String, String)]) -> Option<ServiceCommand> {
    match urls {
        [] => None,
        [(url, _)] => Some(ServiceCommand::OpenUrl(url.clone())),
        _ => {
            let label = |url: &str, name: &str| format!("{name} — {url}");
            let choices = urls.iter().map(|(url, name)| UrlChoice { label: label(url, name), url: url.clone() });
            Some(ServiceCommand::ChooseUrl(choices.collect()))
        }
    }
}

fn debug_command(r: &Resolved) -> Option<ServiceCommand> {
    if let Some(debug) = r.session.as_ref().and_then(ServiceSession::debugger) {
        return debug.can_rerun.then_some(ServiceCommand::RerunDebugger(debug.id));
    }
    r.config.as_ref().map(|config| ServiceCommand::DebugConfig(Box::new(config.clone())))
}

/// The Services of run configurations, read from the run store, the debugger's configured roots and
/// what the project keeps.
#[derive(Clone, Copy, Debug)]
pub struct Services<'a> {
    pub store: &'a RunStore,
    pub debug: &'a [DebugServiceSession],
    pub types: &'a RunConfigTypes,
    pub state: &'a RunServices,
}

impl Services<'_> {
    fn executions(&self) -> Vec<ServiceSession> {
        service_sessions(&self.store.sessions, self.debug)
    }

    fn input<'b>(&'b self, removed: &'b dyn Fn(&str) -> bool) -> TreeInput<'b> {
        TreeInput {
            configs: self.store.configs(),
            sessions: &self.store.sessions,
            debug: self.debug,
            added: self.state.types(),
            ran: self.state.ran(),
            order: self.state.order(),
            hidden: self.state.hidden(),
            removed,
        }
    }

    /// The provider's root nodes: one group per configuration type.
    pub fn roots(&self, i18n: &mut I18n) -> Vec<ServiceTreeItem> {
        let removed = |id: &str| self.store.config_removed(id);
        run_service_items(self.input(&removed), self.types, i18n)
    }

    /// The configurations the group of `kind` shows, in display order.
    pub fn group_configs(&self, kind: &str) -> Vec<RunConfig> {
        let removed = |id: &str| self.store.config_removed(id);
        let executions = self.executions();
        let latest = current_service_sessions(&executions);
        let groups = kind_groups(self.input(&removed), &executions, &latest);
        let Some((_, configs)) = groups.into_iter().find(|(group, _)| group == kind) else { return Vec::new() };
        // A configuration as saved now, else the one its execution ran with.
        configs.into_iter().map(|config| self.store.config(&config.id).unwrap_or(config).clone()).collect()
    }

    /// What the node with `key` stands for: a configuration type's group (`run:type:spring-boot`)
    /// or a configuration (`run:config:<id>`).
    pub fn resolve(&self, key: &str) -> Resolved {
        let executions = self.executions();
        if let Some(kind) = key.strip_prefix(TYPE_PREFIX) {
            let sessions = executions.into_iter().filter(|session| session.config.kind == kind).collect();
            return Resolved { kind: Some(kind.to_string()), config: None, session: None, sessions };
        }
        let id = key.strip_prefix(CONFIG_PREFIX).unwrap_or(key);
        let session = current_service_session(id, &executions).cloned();
        let ran_with = || session.as_ref().map(|session| session.config.clone());
        let config = self.store.config(id).cloned().or_else(ran_with);
        let kind = config.as_ref().map(|config| config.kind.clone());
        Resolved { kind, config, sessions: session.iter().cloned().collect(), session }
    }

    /// Runs (or reruns, in its tab) a configuration.
    fn run_command(&self, r: &Resolved) -> Option<ServiceCommand> {
        let in_place = StartOptions { show: Some(false), ..StartOptions::default() };
        if let Some(debug) = r.session.as_ref().and_then(ServiceSession::debugger) {
            return debug.can_rerun.then_some(ServiceCommand::RerunDebugger(debug.id));
        }
        if let Some(run) = r.session.as_ref().and_then(|session| session.run.as_ref()) {
            let config = Box::new(self.store.rerun_config(run).clone());
            let (reuse, base_config) = (Some(run.id.clone()), run.base_config.clone());
            let options = Box::new(StartOptions { reuse, base_config, ..in_place });
            return Some(ServiceCommand::StartRun { config, options });
        }
        let config = Box::new(r.config.clone()?);
        Some(ServiceCommand::StartRun { config, options: Box::new(in_place) })
    }

    /// The node's toolbar buttons, each with what it does.
    fn item_commands(&self, key: &str) -> Vec<(ServiceItemAction, Option<ServiceCommand>)> {
        let r = self.resolve(key);
        let running: Vec<ServiceSession> = r.sessions.iter().filter(|session| session.is_live()).cloned().collect();
        let urls = browser_urls(&r.sessions);
        let browser = (item_action("open-browser", "Open in Browser", "web", urls.is_empty()), browser_command(&urls));
        let stop_command = Some(ServiceCommand::Stop(running.clone()));
        let Some(config) = &r.config else {
            let Some(kind) = r.kind.clone().filter(|kind| !kind.is_empty()) else {
                return vec![(item_action("stop", "Stop", "stop", running.is_empty()), stop_command)];
            };
            let mut entries = self.group_configs(&kind);
            entries.retain(|config| !is_debug_only(&config.kind, self.types));
            let all_running = entries.iter().all(|config| running.iter().any(|session| session.config.id == config.id));
            let start = |restart: bool| Some(ServiceCommand::StartAll { kind: kind.clone(), restart });
            return vec![
                (item_action("run-all", "Run All", "run", all_running), start(false)),
                (item_action("restart-all", "Restart All", "rerun", entries.is_empty()), start(true)),
                (item_action("stop-all", "Stop All", "stop", running.is_empty()), stop_command),
                browser,
            ];
        };
        let live = r.session.as_ref().is_some_and(ServiceSession::is_live);
        let debugging = r.session.as_ref().and_then(ServiceSession::debugger);
        let run_disabled = match debugging {
            Some(debug) => !debug.can_rerun,
            None => is_debug_only(&config.kind, self.types),
        };
        let debug_disabled = debugging.is_some_and(|debug| !debug.can_rerun);
        let (run_title, run_icon) = if live { ("Rerun", "rerun") } else { ("Run", "run") };
        let mut out = vec![
            (item_action("run", run_title, run_icon, run_disabled), self.run_command(&r)),
            (item_action("debug", "Debug", "debug", debug_disabled), debug_command(&r)),
            browser,
            (item_action("stop", "Stop", "stop", running.is_empty()), stop_command),
        ];
        if is_updatable(config) {
            let run = r.session.as_ref().and_then(|session| session.run.as_ref());
            let debugged = || debugging.map(|debug| format!("debug:{}", debug.id.0));
            let update = ServiceCommand::UpdateApplication(run.map(|run| run.id.clone()).or_else(debugged));
            out.push((item_action("update", "Update Application", "refresh", !live), Some(update)));
        }
        out
    }

    /// The toolbar buttons of the node with `key`.
    pub fn item_actions(&self, key: &str) -> Vec<ServiceItemAction> {
        self.item_commands(key).into_iter().map(|(action, _)| action).collect()
    }

    /// What the toolbar button `action_id` of the node with `key` does now: nothing when the
    /// button is disabled by now, or the key is no node of this provider.
    pub fn invoke(&self, key: &str, action_id: &str) -> Option<ServiceCommand> {
        if !key.starts_with(TYPE_PREFIX) && !key.starts_with(CONFIG_PREFIX) {
            return None;
        }
        let (action, command) = self.item_commands(key).into_iter().find(|(action, _)| action.id == action_id)?;
        command.filter(|_| !action.disabled)
    }

    /// Double click / Enter on a node: runs a configuration that is not running.
    pub fn activate(&self, key: &str) -> Option<ServiceCommand> {
        let r = self.resolve(key);
        let config = r.config.as_ref()?;
        let live = r.session.as_ref().is_some_and(ServiceSession::is_live);
        if live || is_debug_only(&config.kind, self.types) {
            return None;
        }
        self.run_command(&r)
    }

    /// The context menu of the node with `key`: its toolbar buttons, then what applies to a type
    /// group or to a configuration.
    pub fn menu(&self, key: &str) -> Vec<ServiceMenuItem> {
        let button = |action: ServiceItemAction| ServiceMenuItem::Item {
            label: action.title,
            icon: action.icon,
            disabled: action.disabled,
            command: Some(MenuCommand::Action(action.id)),
        };
        let mut items: Vec<ServiceMenuItem> = self.item_actions(key).into_iter().map(button).collect();
        let r = self.resolve(key);
        let Some(config) = &r.config else {
            let Some(kind) = r.kind.filter(|kind| !kind.is_empty()) else { return items };
            let addable = self.store.configs().iter().any(|config| config.kind == kind);
            items.push(ServiceMenuItem::Separator);
            items.push(menu_item("Add Service", "plus", addable.then(|| MenuCommand::AddService(kind.clone()))));
            if self.state.types().contains(&kind) {
                items.push(menu_item("Remove from Services", "close", Some(MenuCommand::RemoveType(kind))));
            }
            return items;
        };
        let run = r.session.as_ref().and_then(|session| session.run.as_ref());
        let debugger = r.session.as_ref().and_then(|session| session.debugger().or(session.debug.last()));
        let hide = MenuCommand::HideConfig { kind: config.kind.clone(), id: config.id.clone() };
        let show_run = run.map(|run| MenuCommand::ShowRun(run.id.clone()));
        let show_debug = debugger.map(|debug| MenuCommand::ShowDebug(debug.id));
        items.extend([
            ServiceMenuItem::Separator,
            menu_item("Show in Run Tool Window", "toolwindow-run", show_run),
            menu_item("Show in Debug Tool Window", "toolwindow-debug", show_debug),
            menu_item("Edit Configuration…", "edit", Some(MenuCommand::EditConfig(config.id.clone()))),
            menu_item("Remove from Services", "close", Some(hide)),
        ]);
        items
    }

    /// What the detail pane shows for the node with `key`.
    pub fn detail(&self, key: &str) -> ServiceDetail {
        let r = self.resolve(key);
        let Some(config) = r.config else {
            return ServiceDetail::Group { title: kind_label(r.kind.as_deref().unwrap_or_default(), self.types) };
        };
        if let Some(session) = &r.session {
            if let Some(run) = &session.run {
                let debugger = session.debugger().cloned().map(Box::new);
                return ServiceDetail::RunConsole { session_id: run.id.clone(), debugger };
            }
            if let Some(debug) = session.debug.first() {
                return ServiceDetail::DebugConsole(debug.id);
            }
        }
        ServiceDetail::NotStarted {
            type_label: kind_label(&config.kind, self.types),
            command_line: command_line(&config.command, &config.args),
            run_disabled: is_debug_only(&config.kind, self.types),
            config: Box::new(config),
        }
    }

    /// Whether the node `source` can be dropped next to the node `target`: both are configurations
    /// of one type. Reordering does not move services between groups.
    pub fn can_move(&self, source: &str, target: &str) -> bool {
        if !source.starts_with(CONFIG_PREFIX) || !target.starts_with(CONFIG_PREFIX) || source == target {
            return false;
        }
        let (from, to) = (self.resolve(source), self.resolve(target));
        from.config.is_some() && to.config.is_some() && from.kind == to.kind
    }

    /// The display order after the node `source` was dropped before or after the node `target`, to
    /// keep with [`RunServices::set_order`]; `None` when nothing moves.
    pub fn moved(&self, source: &str, target: &str, position: DropPosition) -> Option<ServiceOrder> {
        if !self.can_move(source, target) {
            return None;
        }
        self.state.location()?;
        let kind = self.resolve(source).kind?;
        let ids: Vec<String> = self.group_configs(&kind).into_iter().map(|config| config.id).collect();
        let (from, to) = (source.strip_prefix(CONFIG_PREFIX)?, target.strip_prefix(CONFIG_PREFIX)?);
        if !ids.iter().any(|id| id == from) || !ids.iter().any(|id| id == to) {
            return None;
        }
        let mut order = self.state.order().clone();
        let saved = order.get(&kind).map(Vec::as_slice).unwrap_or_default();
        let next = move_service_order(saved, &ids, from, to, position);
        order.insert(kind, next);
        Some(order)
    }

    /// "Add Service › Run Configuration Type": the types not added yet, the project's categories
    /// first.
    pub fn add_menu(&self) -> AddMenu {
        let mut used: Vec<String> = Vec::new();
        for config in self.store.configs() {
            if !used.contains(&config.kind) {
                used.push(config.kind.clone());
            }
        }
        let builtin = KIND_LABELS.iter().map(|(kind, _)| kind.to_string());
        let known = builtin.chain(self.types.iter().map(|ty| ty.id.clone()));
        let mut kinds: Vec<String> = Vec::new();
        for kind in used.iter().cloned().chain(known) {
            if kind != "compound" && !self.state.types().contains(&kind) && !kinds.contains(&kind) {
                kinds.push(kind);
            }
        }
        let categories = add_menu_categories(&kinds, &used, self.types);
        let groups = |buckets: Vec<TypeBucket>| -> Vec<AddMenuGroup> {
            buckets.into_iter().map(|bucket| menu_group(bucket, self.types)).collect()
        };
        AddMenu { project: groups(categories.project), rest: groups(categories.rest) }
    }
}

// ---------------------------------------------------------------------------------------------------
// Add Service › Run Configuration Type

/// The kinds of one menu category.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeBucket {
    pub category: String,
    pub kinds: Vec<String>,
}

/// The kinds grouped by menu category, in display order: [`CATEGORY_ORDER`] first, then unlisted
/// (plugin) categories alphabetically, then "Others" last; each bucket's kinds sorted by label. A
/// category is a bucket of its own even with a single member, so that adding groups and members
/// later keeps the structure.
pub fn categorized_types(kinds: &[String], types: &RunConfigTypes) -> Vec<TypeBucket> {
    let mut buckets: Vec<TypeBucket> = Vec::new();
    for kind in kinds {
        let category = category_of(kind, types);
        match buckets.iter_mut().find(|bucket| bucket.category == category) {
            Some(bucket) => bucket.kinds.push(kind.clone()),
            None => buckets.push(TypeBucket { category, kinds: vec![kind.clone()] }),
        }
    }
    // Unlisted categories sort after the fixed order, before "Others".
    let rank = |category: &str| {
        let listed = CATEGORY_ORDER.iter().position(|known| *known == category);
        (listed.unwrap_or(CATEGORY_ORDER.len()), category == "Others")
    };
    buckets.sort_by(|a, b| {
        rank(&a.category).cmp(&rank(&b.category)).then_with(|| collate::compare(&a.category, &b.category))
    });
    for bucket in &mut buckets {
        bucket.kinds.sort_by(|a, b| collate::compare(&kind_label(a, types), &kind_label(b, types)));
    }
    buckets
}

/// The categories of the Add Service menu: the project's, then the rest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddMenuCategories {
    pub project: Vec<TypeBucket>,
    pub rest: Vec<TypeBucket>,
}

/// The project's categories (those with a kind in `used`), then the rest. A category appears once,
/// even when the project uses only some of its kinds.
pub fn add_menu_categories(kinds: &[String], used: &[String], types: &RunConfigTypes) -> AddMenuCategories {
    let in_project = |bucket: &TypeBucket| bucket.kinds.iter().any(|kind| used.contains(kind));
    let (project, rest) = categorized_types(kinds, types).into_iter().partition(in_project);
    AddMenuCategories { project, rest }
}

/// A type to add: runs "Add Service" for its kind ([`RunServices::add_service`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMenuType {
    pub kind: String,
    pub label: String,
    pub icon: String,
}

/// A category's submenu, with the icon of its first type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AddMenuGroup {
    pub label: String,
    pub icon: String,
    pub types: Vec<AddMenuType>,
}

/// The submenu of "Run Configuration Type": the project's categories, a separator when both parts
/// have entries, then the rest. With no entries at all the menu item is disabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddMenu {
    pub project: Vec<AddMenuGroup>,
    pub rest: Vec<AddMenuGroup>,
}

fn menu_group(bucket: TypeBucket, types: &RunConfigTypes) -> AddMenuGroup {
    let leaf = |kind: &String| AddMenuType {
        kind: kind.clone(),
        label: kind_label(kind, types),
        icon: config_icon(kind, false, types),
    };
    let leaves: Vec<AddMenuType> = bucket.kinds.iter().map(leaf).collect();
    let icon = leaves.first().map(|leaf| leaf.icon.clone()).unwrap_or_default();
    AddMenuGroup { label: bucket.category, icon, types: leaves }
}

// ---------------------------------------------------------------------------------------------------
// Run All / Restart All

/// What Run All or Restart All asks for next.
#[derive(Clone, Debug, PartialEq)]
pub enum StartAllStep {
    /// Stop these executions (drive
    /// [`StopServiceSessions`](crate::run_testing::service_sessions::StopServiceSessions) to its
    /// end), then [`StartAll::advance`].
    StopSessions(Vec<ServiceSession>),
    /// Wait until the processes of these Run sessions are gone, however long it takes, then
    /// [`StartAll::advance`]. Nothing starts before.
    WaitForExit {
        session_ids: Vec<String>,
    },
    /// Start the configuration, then [`StartAll::advance`], or [`StartAll::start_failed`] when it
    /// could not be launched.
    StartRun {
        config: Box<RunConfig>,
        options: Box<StartOptions>,
    },
    /// Show the notification, then [`StartAll::advance`].
    Notify(Box<NotificationRequest>),
    Done,
}

/// Runs the configurations a type group shows, in display order; Restart All stops the running
/// ones first. Configurations only a debugger can launch are left out.
#[derive(Clone, Debug)]
pub struct StartAll {
    /// The project the configurations belong to.
    root: Option<String>,
    entries: VecDeque<RunConfig>,
    /// Restart: the Run sessions whose exit is awaited once they were asked to stop.
    awaiting: Option<Vec<String>>,
    /// The name of the configuration being started.
    starting: Option<String>,
}

impl StartAll {
    pub fn start(services: &Services<'_>, kind: &str, restart: bool) -> (Self, StartAllStep) {
        let mut entries = services.group_configs(kind);
        entries.retain(|config| !is_debug_only(&config.kind, services.types));
        let root = services.state.root().map(str::to_string);
        let mut flow = Self { root, entries: entries.into(), awaiting: None, starting: None };
        if !restart {
            let step = flow.next_start(services);
            return (flow, step);
        }
        let live_of_kind = |session: &ServiceSession| session.config.kind == kind && session.is_live();
        let running: Vec<ServiceSession> = services.executions().into_iter().filter(live_of_kind).collect();
        let runs = running.iter().filter_map(|session| session.run.as_ref());
        flow.awaiting = Some(runs.map(|run| run.id.clone()).collect());
        (flow, StartAllStep::StopSessions(running))
    }

    /// Continues once the last step is carried out, with the Services as they are now.
    pub fn advance(&mut self, services: &Services<'_>) -> StartAllStep {
        match self.awaiting.take() {
            Some(session_ids) if !session_ids.is_empty() => StartAllStep::WaitForExit { session_ids },
            _ => self.next_start(services),
        }
    }

    /// The configuration of the last [`StartAllStep::StartRun`] could not be launched: the others
    /// still start.
    pub fn start_failed(&mut self, error: &str, i18n: &mut I18n) -> StartAllStep {
        let name = self.starting.take().unwrap_or_default();
        let title = i18n.tx_built(&["Cannot run '", "'"], &[Param::from(name)]);
        let mut notification = NotificationRequest::new(NotificationLevel::Error, title);
        notification.group = Some("run".to_string());
        notification.body = Some(error.to_string());
        StartAllStep::Notify(Box::new(notification))
    }

    fn next_start(&mut self, services: &Services<'_>) -> StartAllStep {
        // Another project was opened: its configurations are not these.
        if services.state.root() != self.root.as_deref() {
            self.entries.clear();
        }
        let executions = services.executions();
        while let Some(config) = self.entries.pop_front() {
            if executions.iter().any(|session| session.config.id == config.id && session.is_live()) {
                continue;
            }
            self.starting = Some(config.name.clone());
            let options = StartOptions { show: Some(false), ..StartOptions::default() };
            return StartAllStep::StartRun { config: Box::new(config), options: Box::new(options) };
        }
        StartAllStep::Done
    }
}

// ---------------------------------------------------------------------------------------------------
// Project removal: a workspace member's data leaves with it

/// Whether `path` is inside the directory `root` (a removed member, not a sibling with a shared
/// prefix).
fn inside_root(path: &str, root: &str) -> bool {
    path.strip_prefix(root).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// What the cleanup leaves to do before it continues: carry out the stops and the writes, then
/// reload the configurations (the backend no longer lists the member) and call
/// [`RemovedProjectCleanup::reloaded`].
#[derive(Clone, Debug, PartialEq)]
pub struct ReloadConfigs {
    pub stops: Vec<StopAction>,
    pub persist: Persist,
}

/// Drops a removed workspace member's data from Services: stops and forgets its sessions (a
/// finished one still groups the tree), removes its temporary configurations, and after the reload
/// clears the added types and remembered runs that would otherwise render empty groups.
#[derive(Clone, Debug)]
pub struct RemovedProjectCleanup {
    /// The kinds the member had configurations of.
    kinds: Vec<String>,
}

impl RemovedProjectCleanup {
    pub fn start(removed_root: &str, store: &mut RunStore, now_ms: f64) -> (Self, ReloadConfigs) {
        let of_removed = |config: &&RunConfig| cwd_of(config).is_some_and(|cwd| inside_root(cwd, removed_root));
        let mut ids: Vec<String> = Vec::new();
        let mut kinds: Vec<String> = Vec::new();
        for config in store.configs().iter().filter(of_removed) {
            ids.push(config.id.clone());
            if !kinds.contains(&config.kind) {
                kinds.push(config.kind.clone());
            }
        }
        let of_member = |session: &&RunSession| ids.contains(&session.config.id);
        let sessions: Vec<String> = store.sessions.iter().filter(of_member).map(|session| session.id.clone()).collect();
        let stops = sessions.iter().filter_map(|id| store.close_session(id, now_ms)).collect();
        let mut persist = Persist::default();
        if store.temporary.iter().any(|id| ids.contains(id)) {
            store.temporary.retain(|id| !ids.contains(id));
            let left = store.configs().iter().filter(|config| !ids.contains(&config.id)).cloned().collect();
            store.set_configs(left);
            persist = store.persist_temporary();
        }
        (Self { kinds }, ReloadConfigs { stops, persist })
    }

    /// The configurations were reloaded: the member's kinds that are left without a configuration
    /// and without a session leave the added types, and runs of configurations that are gone are
    /// forgotten.
    pub fn reloaded(self, store: &RunStore, services: &mut RunServices) -> Vec<StorageWrite> {
        let configured = |kind: &str| store.configs().iter().any(|config| config.kind == kind);
        let ran = |kind: &str| store.sessions.iter().any(|session| session.config.kind == kind);
        let mut writes: Vec<StorageWrite> = Vec::new();
        for kind in self.kinds.iter().filter(|kind| !configured(kind) && !ran(kind)) {
            writes.extend(services.remove_type(kind));
        }
        writes.extend(services.retain_ran(|id| store.config(id).is_some()));
        writes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::config_types::RunConfigType;
    use crate::run::configs::{blank_config, to_cwd};
    use crate::run::store::started_port;
    use ide_core::RunId;

    fn config(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: kind.into(), detected: true, ..blank_config(id, id, kind) }
    }

    fn in_dir(config: RunConfig, dir: &str) -> RunConfig {
        RunConfig { cwd: to_cwd(Some(dir)), ..config }
    }

    fn session(id: &str, config: &RunConfig, status: SessionStatus, started_at: f64) -> RunSession {
        RunSession {
            id: id.into(),
            config: config.clone(),
            run_id: None,
            status,
            exit_code: None,
            started_at,
            ended_at: None,
            stdin_closed: false,
            stopped: false,
            base_config: None,
            terminal: false,
            http_port: None,
        }
    }

    fn running(id: &str, config: &RunConfig) -> RunSession {
        session(id, config, SessionStatus::Running, 1.0)
    }

    /// A running direct debug launch of `config`.
    fn debug_root(config: &RunConfig) -> DebugServiceSession {
        DebugServiceSession {
            id: DebugSessionId(1),
            config: config.clone(),
            started_at: 2.0,
            status: DebugStatus::Running,
            process_id: None,
            http_port: None,
            can_rerun: false,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn default_types() -> Vec<String> {
        strings(&DEFAULT_TYPES)
    }

    /// Ids by kind; the ids of a kind are separated by spaces.
    fn by_kind(entries: &[(&str, &str)]) -> BTreeMap<String, Vec<String>> {
        let ids = |listed: &str| -> Vec<String> { listed.split_whitespace().map(str::to_string).collect() };
        entries.iter().map(|(kind, listed)| (kind.to_string(), ids(listed))).collect()
    }

    fn children(item: &ServiceTreeItem) -> &[ServiceTreeItem] {
        item.children.as_deref().unwrap_or_default()
    }

    fn keys(item: &ServiceTreeItem) -> Vec<&str> {
        children(item).iter().map(|child| child.key.as_str()).collect()
    }

    /// What a tree is built from; `removed` lists the ids of deleted configurations.
    #[derive(Default)]
    struct Tree {
        configs: Vec<RunConfig>,
        sessions: Vec<RunSession>,
        added: Vec<String>,
        ran: Vec<String>,
        order: ServiceOrder,
        debug: Vec<DebugServiceSession>,
        removed: Vec<String>,
        hidden: HiddenConfigs,
    }

    impl Tree {
        fn items(&self) -> Vec<ServiceTreeItem> {
            let removed = |id: &str| self.removed.iter().any(|removed| removed == id);
            let input = TreeInput {
                configs: &self.configs,
                sessions: &self.sessions,
                debug: &self.debug,
                added: &self.added,
                ran: &self.ran,
                order: &self.order,
                hidden: &self.hidden,
                removed: &removed,
            };
            run_service_items(input, &RunConfigTypes::new(), &mut I18n::default())
        }
    }

    /// A window: its run store, the debugger's roots, the project's Services and what is stored.
    struct World {
        store: RunStore,
        debug: Vec<DebugServiceSession>,
        types: RunConfigTypes,
        state: RunServices,
        storage: BTreeMap<String, String>,
    }

    impl World {
        fn new(root: Option<&str>) -> Self {
            let mut world = Self {
                store: RunStore::new(),
                debug: Vec::new(),
                types: RunConfigTypes::new(),
                state: RunServices::default(),
                storage: BTreeMap::new(),
            };
            world.open(root);
            world
        }

        /// Opens the project at `root`: its Services are read from storage.
        fn open(&mut self, root: Option<&str>) {
            let get = |key: Option<String>| key.and_then(|key| self.storage.get(&key)).map(String::as_str);
            let stored = StoredServices {
                types: get(root.map(types_key)),
                ran: get(root.map(ran_key)),
                hidden: get(root.map(hidden_key)),
                order: get(root.map(order_key)),
            };
            self.state = RunServices::open(root, root, stored);
            self.store.project_root = root.map(str::to_string);
        }

        fn services(&self) -> Services<'_> {
            Services { store: &self.store, debug: &self.debug, types: &self.types, state: &self.state }
        }

        fn stored(&self, key: &str) -> Option<&str> {
            self.storage.get(key).map(String::as_str)
        }

        fn apply(&mut self, writes: impl IntoIterator<Item = StorageWrite>) {
            for write in writes {
                match write.value {
                    Some(value) => self.storage.insert(write.key, value),
                    None => self.storage.remove(&write.key),
                };
            }
        }

        fn action(&self, key: &str, id: &str) -> ServiceItemAction {
            let actions = self.services().item_actions(key);
            actions.into_iter().find(|action| action.id == id).expect("the node has the action")
        }

        /// "Remove from Services" in the menu of a configuration's node.
        fn hide(&mut self, id: &str) {
            let menu = self.services().menu(&format!("{CONFIG_PREFIX}{id}"));
            let Some(MenuCommand::HideConfig { kind, id }) = menu_command(&menu, "Remove from Services") else {
                panic!("the configuration can be removed from Services")
            };
            let write = self.state.hide_config(&kind, &id);
            self.apply(write);
        }

        /// Runs a group's Run All or Restart All to its end, every start succeeding; stopping ends
        /// the Run sessions. Returns the steps.
        fn run_batch(&mut self, key: &str, action_id: &str) -> Vec<StartAllStep> {
            let Some(ServiceCommand::StartAll { kind, restart }) = self.services().invoke(key, action_id) else {
                return Vec::new();
            };
            let (mut flow, mut step) = StartAll::start(&self.services(), &kind, restart);
            let mut steps = Vec::new();
            while step != StartAllStep::Done {
                if let StartAllStep::StopSessions(sessions) = &step {
                    let stopped: Vec<&str> =
                        sessions.iter().filter_map(|session| session.run.as_ref()).map(|run| run.id.as_str()).collect();
                    for session in &mut self.store.sessions {
                        if stopped.contains(&session.id.as_str()) {
                            session.status = SessionStatus::Finished;
                        }
                    }
                }
                steps.push(step);
                step = flow.advance(&self.services());
            }
            steps
        }
    }

    fn menu_command(items: &[ServiceMenuItem], wanted: &str) -> Option<MenuCommand> {
        items.iter().find_map(|item| match item {
            ServiceMenuItem::Item { label, command, .. } if *label == wanted => command.clone(),
            _ => None,
        })
    }

    fn start_run(config: &RunConfig) -> StartAllStep {
        let options = StartOptions { show: Some(false), ..StartOptions::default() };
        StartAllStep::StartRun { config: Box::new(config.clone()), options: Box::new(options) }
    }

    /// The ids of the configurations the steps start, in order.
    fn started(steps: &[StartAllStep]) -> Vec<&str> {
        let mut ids = Vec::new();
        for step in steps {
            if let StartAllStep::StartRun { config, .. } = step {
                ids.push(config.id.as_str());
            }
        }
        ids
    }

    // run configurations in Services

    #[test]
    fn shows_configured_debug_roots_without_a_run_session() {
        for kind in ["spring-boot", "node", "python", "go", "cargo"] {
            let c = config("debug-app", kind);
            let build = |status: DebugStatus| {
                let d = DebugServiceSession { status, ..debug_root(&c) };
                Tree { configs: vec![c.clone()], debug: vec![d], ..Tree::default() }.items()
            };
            let live = build(DebugStatus::Running);
            let child = &children(&live[0])[0];
            let shown = (child.key.as_str(), child.detail.as_deref(), child.icon.as_str(), child.dim);
            assert_eq!(shown, ("run:config:debug-app", Some("Debugging"), "debug", false), "{kind}");
            let paused = build(DebugStatus::Paused);
            assert_eq!(children(&paused[0])[0].detail.as_deref(), Some("Suspended"), "{kind}");
            let ended = build(DebugStatus::Terminated);
            let child = &children(&ended[0])[0];
            assert_eq!((child.detail.as_deref(), child.dim), (Some("Finished"), true), "{kind}");
        }
    }

    #[test]
    fn does_not_reuse_an_old_run_port_for_a_new_direct_debug_launch_or_double_count_an_attachment() {
        let c = config("app", "spring-boot");
        let ended = session("run", &c, SessionStatus::Finished, 1.0);
        let r = RunSession { run_id: Some(RunId(7)), http_port: Some(8080), ..ended };
        let d = debug_root(&c);
        let tree = |sessions: Vec<RunSession>, debug: Vec<DebugServiceSession>| {
            Tree { configs: vec![c.clone()], sessions, added: default_types(), debug, ..Tree::default() }.items()
        };
        let direct = tree(vec![r.clone()], vec![d.clone()]);
        assert_eq!(direct[0].detail.as_deref(), Some("1 running"));
        assert_eq!(children(&direct[0])[0].detail.as_deref(), Some("Debugging"));
        let attached = tree(
            vec![RunSession { status: SessionStatus::Running, ..r }],
            vec![DebugServiceSession { process_id: Some(RunId(7)), ..d }],
        );
        assert_eq!(attached[0].detail.as_deref(), Some("1 running"));
        assert_eq!(children(&attached[0]).len(), 1);
        assert_eq!(children(&attached[0])[0].detail.as_deref(), Some(":8080"));
    }

    #[test]
    fn shows_the_app_port_of_a_direct_debug_session_like_for_run() {
        let c = config("app", "spring-boot");
        let d = DebugServiceSession { http_port: Some(8080), ..debug_root(&c) };
        let tree = |d: DebugServiceSession| {
            Tree { configs: vec![c.clone()], added: default_types(), debug: vec![d], ..Tree::default() }.items()
        };
        let items = tree(d.clone());
        let child = &children(&items[0])[0];
        assert_eq!((child.detail.as_deref(), child.icon.as_str(), child.dim), (Some(":8080"), "debug", false));
        // A paused debugger still wins over the announced port.
        let paused = tree(DebugServiceSession { status: DebugStatus::Paused, ..d });
        assert_eq!(children(&paused[0])[0].detail.as_deref(), Some("Suspended"));
    }

    #[test]
    fn applies_display_order_to_detected_and_saved_configurations_of_every_language() {
        let configs = vec![
            config("java-a", "spring-boot"),
            config("java-b", "spring-boot"),
            config("node-a", "npm"),
            RunConfig { detected: false, ..config("node-b", "npm") },
            config("py-a", "python"),
            config("py-b", "python"),
        ];
        let order = by_kind(&[("spring-boot", "java-b java-a"), ("npm", "node-b node-a"), ("python", "py-b py-a")]);
        let added = strings(&["spring-boot", "npm", "python"]);
        let items = Tree { configs, added, order, ..Tree::default() }.items();
        let labels: Vec<Vec<&str>> =
            items.iter().map(|group| children(group).iter().map(|child| child.label.as_str()).collect()).collect();
        assert_eq!(labels, [["java-b", "java-a"], ["node-b", "node-a"], ["py-b", "py-a"]]);
    }

    #[test]
    fn orders_configurations_across_workspace_members() {
        for kind in ["tomcat", "application", "compound"] {
            let member = |id: &str, dir: &str, module: &str| {
                let mut config = in_dir(config(id, kind), dir);
                config.extras.module = Some(module.to_string());
                config
            };
            let first = member("workspace:one:app", "/workspace/one", "one");
            let second = member("workspace:two:app", "/workspace/two", "two");
            let fresh = config("workspace:two:new", kind);
            let order = by_kind(&[(kind, "workspace:two:app workspace:one:app")]);
            let configs = vec![first, second, fresh];
            let items = Tree { configs, added: strings(&[kind]), order, ..Tree::default() }.items();
            let expected =
                ["run:config:workspace:two:app", "run:config:workspace:one:app", "run:config:workspace:two:new"];
            assert_eq!(keys(&items[0]), expected, "{kind}");
        }
    }

    #[test]
    fn orders_remembered_configurations_when_their_type_was_not_explicitly_added() {
        let configs = vec![config("build", "cargo"), config("test", "cargo")];
        let order = by_kind(&[("cargo", "test build")]);
        let items = Tree { configs, ran: strings(&["build", "test"]), order, ..Tree::default() }.items();
        assert_eq!(keys(&items[0]), ["run:config:test", "run:config:build"]);
    }

    #[test]
    fn keeps_stable_service_identity_and_session_details_after_refresh_and_rename() {
        let renamed = RunConfig { name: "Renamed build".into(), ..config("build", "cargo") };
        let items = Tree {
            sessions: vec![RunSession { http_port: Some(8080), ..running("s1", &renamed) }],
            configs: vec![renamed, config("test", "cargo")],
            added: strings(&["cargo"]),
            order: by_kind(&[("cargo", "test build")]),
            ..Tree::default()
        }
        .items();
        let shown: Vec<(&str, &str, Option<&str>)> = children(&items[0])
            .iter()
            .map(|child| (child.key.as_str(), child.label.as_str(), child.detail.as_deref()))
            .collect();
        assert_eq!(shown, [("run:config:test", "test", None), ("run:config:build", "Renamed build", Some(":8080"))]);
    }

    #[test]
    fn lists_added_types_fully_and_other_types_only_with_their_runs() {
        let (build, test, dev) = (config("build", "cargo"), config("test", "cargo"), config("dev", "npm"));
        let scratch = config("context:file", "python");
        let sessions = vec![
            session("s1", &dev, SessionStatus::Finished, 1.0),
            session("s2", &dev, SessionStatus::Running, 2.0),
            RunSession { exit_code: Some(2), ..session("s3", &scratch, SessionStatus::Failed, 3.0) },
        ];
        let configs = vec![build, test, dev];
        let items = Tree { configs, sessions, added: strings(&["cargo"]), ..Tree::default() }.items();
        let groups: Vec<(&str, &str, Option<&str>)> =
            items.iter().map(|item| (item.key.as_str(), item.label.as_str(), item.detail.as_deref())).collect();
        let expected = [
            ("run:type:cargo", "Cargo", None),
            ("run:type:npm", "npm", Some("1 running")),
            ("run:type:python", "Python", None),
        ];
        assert_eq!(groups, expected);
        let cargo: Vec<(&str, Option<&str>, bool)> =
            children(&items[0]).iter().map(|child| (child.key.as_str(), child.detail.as_deref(), child.dim)).collect();
        assert_eq!(cargo, [("run:config:build", None, true), ("run:config:test", None, true)]);
        // The newest session of a configuration decides its state.
        let npm: Vec<(&str, Option<&str>, bool, &str)> = children(&items[1])
            .iter()
            .map(|child| (child.label.as_str(), child.detail.as_deref(), child.dim, child.icon.as_str()))
            .collect();
        assert_eq!(npm, [("dev", Some("Running"), false, "run")]);
        let python: Vec<(&str, Option<&str>)> =
            children(&items[2]).iter().map(|child| (child.key.as_str(), child.detail.as_deref())).collect();
        assert_eq!(python, [("run:config:context:file", Some("Failed (exit 2)"))]);
    }

    #[test]
    fn has_no_groups_without_added_types_or_sessions() {
        assert_eq!(Tree { configs: vec![config("build", "cargo")], ..Tree::default() }.items(), []);
    }

    #[test]
    fn describes_session_states() {
        let build = config("build", "cargo");
        let status = |session: &RunSession| session_status(Some(session));
        assert_eq!(session_status(None), None);
        assert_eq!(status(&session("a", &build, SessionStatus::Starting, 0.0)).as_deref(), Some("Starting"));
        let stopped = RunSession { stopped: true, ..session("a", &build, SessionStatus::Failed, 0.0) };
        assert_eq!(status(&stopped).as_deref(), Some("Stopped"));
        assert_eq!(status(&session("a", &build, SessionStatus::Finished, 0.0)).as_deref(), Some("Finished"));
    }

    #[test]
    fn shows_spring_boot_by_default_hidden_while_the_project_has_none() {
        let (build, app) = (config("build", "cargo"), config("spring:app", "spring-boot"));
        let tree = |configs: Vec<RunConfig>| Tree { configs, added: default_types(), ..Tree::default() }.items();
        assert_eq!(tree(vec![build.clone()]), []);
        let items = tree(vec![build, app]);
        let groups: Vec<(&str, &str, bool)> =
            items.iter().map(|item| (item.key.as_str(), item.label.as_str(), item.expanded)).collect();
        assert_eq!(groups, [("run:type:spring-boot", "Spring Boot", true)]);
        let child = &children(&items[0])[0];
        assert_eq!((child.label.as_str(), child.detail.as_deref(), child.dim), ("spring:app", None, true));
        assert_eq!(children(&items[0]).len(), 1);
    }

    #[test]
    fn shows_the_http_port_of_a_running_application_instead_of_its_state() {
        let app = config("spring:app", "spring-boot");
        let serving = RunSession { http_port: Some(8080), ..running("s1", &app) };
        assert_eq!(session_detail(Some(&serving)).as_deref(), Some(":8080"));
        assert_eq!(session_detail(Some(&running("s1", &app))).as_deref(), Some("Running"));
        let stopped =
            RunSession { http_port: Some(8080), stopped: true, ..session("s1", &app, SessionStatus::Finished, 1.0) };
        assert_eq!(session_detail(Some(&stopped)).as_deref(), Some("Stopped"));
        let (configs, sessions) = (vec![app], vec![serving]);
        let items = Tree { configs, sessions, added: default_types(), ..Tree::default() }.items();
        assert_eq!(children(&items[0])[0].detail.as_deref(), Some(":8080"));
    }

    // types added to Services

    #[test]
    fn defaults_to_spring_boot_and_persists_the_users_list_per_project() {
        let mut world = World::new(None);
        assert_eq!(read_types(world.stored(&types_key("/p"))), ["spring-boot"]);
        world.storage.insert(types_key("/p"), r#"["npm"]"#.to_string());
        assert_eq!(read_types(world.stored(&types_key("/p"))), ["npm"]);
        world.storage.insert(types_key("/p"), "[]".to_string());
        assert_eq!(read_types(world.stored(&types_key("/p"))), [""; 0]);
        assert_eq!(read_types(world.stored(&types_key("/q"))), ["spring-boot"]);
    }

    // configurations that ran

    #[test]
    fn persists_the_ran_configuration_ids_per_project() {
        let mut world = World::new(None);
        assert_eq!(read_ran(world.stored(&ran_key("/p"))), [""; 0]);
        world.storage.insert(ran_key("/p"), r#"["dev"]"#.to_string());
        assert_eq!(read_ran(world.stored(&ran_key("/p"))), ["dev"]);
        assert_eq!(read_ran(world.stored(&ran_key("/q"))), [""; 0]);
    }

    #[test]
    fn lists_previously_run_configurations_without_a_session() {
        let configs = vec![config("dev", "npm"), config("context:file", "python")];
        let items = Tree { configs, ran: strings(&["dev", "context:file"]), ..Tree::default() }.items();
        let groups: Vec<(&str, &str)> = items.iter().map(|item| (item.key.as_str(), item.label.as_str())).collect();
        assert_eq!(groups, [("run:type:npm", "npm"), ("run:type:python", "Python")]);
        let npm: Vec<(&str, bool)> =
            children(&items[0]).iter().map(|child| (child.label.as_str(), child.dim)).collect();
        assert_eq!(npm, [("dev", true)]);
        assert_eq!(keys(&items[1]), ["run:config:context:file"]);
    }

    #[test]
    fn ignores_ran_ids_whose_configuration_is_gone_and_merges_sessions() {
        let dev = config("dev", "npm");
        let gone = Tree { configs: vec![dev.clone()], ran: strings(&["missing"]), ..Tree::default() }.items();
        assert_eq!(gone, []);
        let sessions = vec![running("s1", &dev)];
        let items = Tree { configs: vec![dev], sessions, ran: strings(&["dev"]), ..Tree::default() }.items();
        assert_eq!(items.len(), 1);
        let shown: Vec<(&str, Option<&str>)> =
            children(&items[0]).iter().map(|child| (child.label.as_str(), child.detail.as_deref())).collect();
        assert_eq!(shown, [("dev", Some("Running"))]);
    }

    #[test]
    fn drops_a_deleted_configuration_once_its_run_has_ended() {
        let (dev, scratch) = (config("dev", "npm"), config("context:file", "python"));
        let tree = |sessions: Vec<RunSession>, ran: &[&str]| {
            Tree { sessions, ran: strings(ran), removed: strings(&["dev"]), ..Tree::default() }.items()
        };
        assert_eq!(tree(vec![session("s1", &dev, SessionStatus::Finished, 1.0)], &["dev"]), []);
        let live = tree(vec![running("s1", &dev)], &["dev"]);
        let shown: Vec<(&str, Option<&str>)> =
            children(&live[0]).iter().map(|child| (child.label.as_str(), child.detail.as_deref())).collect();
        assert_eq!(shown, [("dev", Some("Running"))]);
        // Never a saved configuration (a build task, an external tool): its finished run stays.
        let ad_hoc = tree(vec![session("s2", &scratch, SessionStatus::Finished, 1.0)], &[]);
        assert_eq!(keys(&ad_hoc[0]), ["run:config:context:file"]);
    }

    // embedded web server port

    #[test]
    fn reads_the_port_from_the_started_line() {
        let tomcat = "TomcatWebServer  : Tomcat started on port 8080 (http) with context path '/'";
        assert_eq!(started_port(tomcat), Some(8080));
        assert_eq!(started_port("Tomcat started on port(s): 8081 (http) with context path ''"), Some(8081));
        let netty = "o.s.b.web.embedded.netty.NettyWebServer  : Netty started on port 9090";
        assert_eq!(started_port(netty), Some(9090));
        assert_eq!(started_port("Netty started on port(s): 9091"), Some(9091));
        assert_eq!(started_port("Undertow started on port(s) 8082 (http)"), Some(8082));
        assert_eq!(started_port("Jetty started on port 9083 (http/1.1) with context path '/'"), Some(9083));
        assert_eq!(started_port("Tomcat initialized with port 8080 (http)"), None);
        assert_eq!(started_port("Started DemoApplication in 1.2 seconds"), None);
        assert_eq!(started_port("Tomcat started on port 99999"), None);
    }

    // hidden configurations

    #[test]
    fn hides_a_configuration_unless_it_runs() {
        let (a, b) = (config("a", "spring-boot"), config("b", "spring-boot"));
        let tree = |sessions: Vec<RunSession>| {
            let hidden = by_kind(&[("spring-boot", "a")]);
            let configs = vec![a.clone(), b.clone()];
            Tree { configs, sessions, added: default_types(), hidden, ..Tree::default() }.items()
        };
        assert_eq!(keys(&tree(Vec::new())[0]), ["run:config:b"]);
        assert_eq!(keys(&tree(vec![running("s1", &a)])[0]), ["run:config:a", "run:config:b"]);
    }

    #[test]
    fn keeps_an_empty_default_group_whose_configurations_are_all_hidden() {
        let hidden = by_kind(&[("spring-boot", "a")]);
        let configs = vec![config("a", "spring-boot")];
        let items = Tree { configs, added: default_types(), hidden, ..Tree::default() }.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].children, Some(Vec::new()));
    }

    #[test]
    fn persists_hidden_ids_per_project() {
        let mut world = World::new(None);
        assert_eq!(read_hidden(world.stored(&hidden_key("/p"))), HiddenConfigs::new());
        world.storage.insert(hidden_key("/p"), r#"{"spring-boot":["a"]}"#.to_string());
        assert_eq!(read_hidden(world.stored(&hidden_key("/p"))), by_kind(&[("spring-boot", "a")]));
        assert_eq!(read_hidden(world.stored(&hidden_key("/q"))), HiddenConfigs::new());
        // Without a project nothing is read.
        let stored = StoredServices { hidden: world.stored(&hidden_key("/p")), ..StoredServices::default() };
        assert_eq!(RunServices::open(None, None, stored).hidden(), &HiddenConfigs::new());
    }

    // add service categories

    #[test]
    fn categorizes_every_built_in_kind_update_the_table_when_adding_kinds() {
        let expected = [
            ("npm", "JavaScript"),
            ("node", "JavaScript"),
            ("bun", "JavaScript"),
            ("deno", "JavaScript"),
            ("cargo", "Rust"),
            ("rustc", "Rust"),
            ("gradle", "Build Tools"),
            ("maven", "Build Tools"),
            ("cmake", "Build Tools"),
            ("make", "Build Tools"),
            ("just", "Build Tools"),
            ("go", "Go"),
            ("python", "Python"),
            ("pytest", "Python"),
            ("phpunit", "Others"),
            ("composer", "Others"),
            ("cc", "C/C++"),
            ("cmake-application", "C/C++"),
            ("shell", "Shell"),
            ("program", "Shell"),
            ("compound", "Others"),
            ("application", "Java"),
            ("spring-boot", "Java"),
            ("native", "Java"),
            ("java-file", "Java"),
            ("kotlin", "Java"),
            ("kotlinc", "Java"),
            ("scala-cli", "Java"),
            ("ruby", "Others"),
            ("php", "Others"),
            ("perl", "Others"),
            ("lua", "Others"),
            ("r", "Others"),
            ("swift", "Others"),
            ("dart", "Others"),
            ("elixir", "Others"),
        ];
        let types = RunConfigTypes::new();
        for (kind, _) in KIND_LABELS {
            let category = expected.iter().find(|(listed, _)| *listed == kind).map(|(_, category)| *category);
            assert_eq!(Some(category_of(kind, &types).as_str()), category, "{kind}");
        }
    }

    #[test]
    fn orders_buckets_fixed_order_first_unlisted_plugin_categories_after_others_last() {
        let mut types = RunConfigTypes::new();
        types.register(RunConfigType::new("fake-cloud", "Cloud Run", "cloud", |_, _| None).group("Clouds"));
        let buckets = categorized_types(&strings(&["ruby", "spring-boot", "npm", "fake-cloud", "gradle"]), &types);
        let categories: Vec<&str> = buckets.iter().map(|bucket| bucket.category.as_str()).collect();
        assert_eq!(categories, ["Java", "Build Tools", "JavaScript", "Clouds", "Others"]);
    }

    #[test]
    fn keeps_a_single_member_category_as_its_own_bucket() {
        let bucket = |category: &str, kind: &str| TypeBucket { category: category.into(), kinds: strings(&[kind]) };
        let buckets = categorized_types(&strings(&["go", "ruby"]), &RunConfigTypes::new());
        assert_eq!(buckets, [bucket("Go", "go"), bucket("Others", "ruby")]);
    }

    #[test]
    fn sorts_each_buckets_kinds_by_their_label() {
        let kinds = strings(&["spring-boot", "application", "java-file", "native"]);
        let buckets = categorized_types(&kinds, &RunConfigTypes::new());
        let sorted = strings(&["application", "java-file", "native", "spring-boot"]);
        assert_eq!(buckets.first(), Some(&TypeBucket { category: "Java".into(), kinds: sorted }));
    }

    #[test]
    fn groups_nothing_for_an_empty_list() {
        assert_eq!(categorized_types(&[], &RunConfigTypes::new()), []);
    }

    #[test]
    fn lists_a_category_once_in_the_add_service_menu_when_the_project_uses_only_some_of_its_kinds() {
        let kinds = strings(&["go", "maven", "gradle", "python"]);
        let menu = add_menu_categories(&kinds, &strings(&["go", "maven"]), &RunConfigTypes::new());
        let categories =
            |buckets: &[TypeBucket]| buckets.iter().map(|bucket| bucket.category.clone()).collect::<Vec<_>>();
        assert_eq!(categories(&menu.project), ["Build Tools", "Go"]);
        for kind in ["maven", "gradle"] {
            assert!(menu.project[0].kinds.iter().any(|listed| listed == kind), "{kind}");
        }
        assert_eq!(categories(&menu.rest), ["Python"]);
    }

    // open in browser

    #[test]
    fn offers_open_in_browser_while_a_session_runs_with_an_http_port() {
        let a = config("a", "spring-boot");
        let mut world = World::new(None);
        world.store.sessions = vec![RunSession { http_port: Some(8080), ..running("s1", &a) }];
        world.store.set_configs(vec![a]);
        assert!(!world.action("run:config:a", "open-browser").disabled);
    }

    #[test]
    fn disables_open_in_browser_without_a_port_or_a_running_session() {
        let a = config("a", "spring-boot");
        let mut world = World::new(None);
        world.store.set_configs(vec![a.clone()]);
        world.store.sessions = vec![running("s1", &a)];
        assert!(world.action("run:config:a", "open-browser").disabled);
        let ended = session("s1", &a, SessionStatus::Finished, 1.0);
        world.store.sessions = vec![RunSession { http_port: Some(8080), ..ended }];
        assert!(world.action("run:config:a", "open-browser").disabled);
    }

    // group batch actions

    const GROUP: &str = "run:type:spring-boot";

    /// A window without a project whose Services list Spring Boot, with these configurations.
    fn spring_world(ids: &[&str]) -> (World, Vec<RunConfig>) {
        let configs: Vec<RunConfig> = ids.iter().map(|id| config(id, "spring-boot")).collect();
        let mut world = World::new(None);
        let write = world.state.add_type("spring-boot");
        world.apply(write);
        world.store.set_configs(configs.clone());
        (world, configs)
    }

    #[test]
    fn enables_run_all_and_restart_all_with_stopped_configurations_stop_all_only_with_running_ones() {
        let (world, _) = spring_world(&["a", "b"]);
        assert!(!world.action(GROUP, "run-all").disabled);
        assert!(!world.action(GROUP, "restart-all").disabled);
        assert!(world.action(GROUP, "stop-all").disabled);
    }

    #[test]
    fn enables_stop_all_when_one_runs_and_run_all_while_others_are_stopped() {
        let (mut world, configs) = spring_world(&["a", "b"]);
        world.store.sessions = vec![running("s1", &configs[0])];
        assert!(!world.action(GROUP, "stop-all").disabled);
        assert!(!world.action(GROUP, "run-all").disabled);
    }

    #[test]
    fn disables_run_all_when_everything_runs() {
        let (mut world, configs) = spring_world(&["a"]);
        world.store.sessions = vec![running("s1", &configs[0])];
        assert!(world.action(GROUP, "run-all").disabled);
    }

    #[test]
    fn starts_stopped_configurations_in_display_order_skipping_running_ones() {
        let (mut world, configs) = spring_world(&["a", "b", "c"]);
        world.store.sessions = vec![running("s1", &configs[0])];
        assert_eq!(world.run_batch(GROUP, "run-all"), [start_run(&configs[1]), start_run(&configs[2])]);
    }

    #[test]
    fn restarts_stops_the_running_ones_then_starts_every_configuration_in_order() {
        let (mut world, configs) = spring_world(&["a", "b"]);
        world.store.sessions = vec![running("s1", &configs[0])];
        let steps = world.run_batch(GROUP, "restart-all");
        let StartAllStep::StopSessions(stopped) = &steps[0] else { panic!("the running ones are stopped first") };
        assert_eq!(stopped.iter().map(|session| session.config.id.as_str()).collect::<Vec<_>>(), ["a"]);
        let wait = StartAllStep::WaitForExit { session_ids: strings(&["s1"]) };
        assert_eq!(steps[1..], [wait, start_run(&configs[0]), start_run(&configs[1])]);
    }

    #[test]
    fn waits_for_process_exit_before_restarting_any_configuration_and_continues_after_a_launch_failure() {
        let (mut world, configs) = spring_world(&["a", "b"]);
        world.store.sessions = vec![RunSession { run_id: Some(RunId(1)), ..running("s1", &configs[0]) }];
        let (mut flow, step) = StartAll::start(&world.services(), "spring-boot", true);
        assert!(matches!(step, StartAllStep::StopSessions(_)));
        // The process is still alive once the stop was asked for: nothing starts before it is gone.
        assert_eq!(flow.advance(&world.services()), StartAllStep::WaitForExit { session_ids: strings(&["s1"]) });
        world.store.sessions[0].status = SessionStatus::Finished;
        assert_eq!(flow.advance(&world.services()), start_run(&configs[0]));
        let StartAllStep::Notify(notification) = flow.start_failed("launch failed", &mut I18n::default()) else {
            panic!("the failure is reported")
        };
        assert_eq!(notification.title, "Cannot run 'a'");
        assert_eq!(notification.body.as_deref(), Some("launch failed"));
        assert_eq!(flow.advance(&world.services()), start_run(&configs[1]));
        assert_eq!(flow.advance(&world.services()), StartAllStep::Done);
    }

    #[test]
    fn uses_the_saved_display_order_and_leaves_other_types_alone() {
        let mut world = World::new(None);
        world.storage.insert(order_key("/group-order"), r#"{"spring-boot":["c","a","b"]}"#.to_string());
        world.open(Some("/group-order"));
        let d = config("d", "npm");
        world.store.sessions = vec![running("other", &d)];
        let spring = ["a", "b", "c"].map(|id| config(id, "spring-boot"));
        world.store.set_configs([spring.to_vec(), vec![d]].concat());
        assert_eq!(started(&world.run_batch(GROUP, "run-all")), ["c", "a", "b"]);
        assert!(world.action(GROUP, "stop-all").disabled);
        assert_eq!(world.services().invoke(GROUP, "stop-all"), None);
    }

    #[test]
    fn does_not_resurrect_a_deleted_configuration_retained_only_as_finished_output() {
        let (mut world, configs) = spring_world(&["removed"]);
        world.store.set_configs(Vec::new());
        world.store.sessions = vec![session("old", &configs[0], SessionStatus::Finished, 1.0)];
        assert!(world.action(GROUP, "run-all").disabled);
        assert!(world.action(GROUP, "restart-all").disabled);
    }

    #[test]
    fn rechecks_enabled_state_when_a_registered_action_runs() {
        let (mut world, configs) = spring_world(&["a"]);
        assert!(!world.action(GROUP, "run-all").disabled);
        world.store.sessions = vec![session("s1", &configs[0], SessionStatus::Starting, 1.0)];
        assert_eq!(world.services().invoke(GROUP, "run-all"), None);
    }

    #[test]
    fn opens_the_known_group_url_and_offers_a_named_choice_for_several_web_apps() {
        let (mut world, configs) = spring_world(&["a", "b"]);
        world.store.sessions = vec![RunSession { http_port: Some(8123), ..running("s1", &configs[0]) }];
        let open = ServiceCommand::OpenUrl("http://localhost:8123".into());
        assert_eq!(world.services().invoke(GROUP, "open-browser"), Some(open));
        world.debug = vec![DebugServiceSession { http_port: Some(8456), ..debug_root(&configs[1]) }];
        let choice = |name: &str, port: u16| {
            let url = format!("http://localhost:{port}");
            UrlChoice { label: format!("{name} — {url}"), url }
        };
        let choose = ServiceCommand::ChooseUrl(vec![choice("a", 8123), choice("b", 8456)]);
        assert_eq!(world.services().invoke(GROUP, "open-browser"), Some(choose));
    }

    // removing a workspace member from Services

    #[test]
    fn stops_and_forgets_the_members_sessions_and_drops_its_empty_type_groups() {
        let main = in_dir(config("app", "spring-boot"), "/w/main");
        let vue = in_dir(config("dev", "npm"), "/w/vue");
        let mut world = World::new(None);
        world.storage.insert(types_key("/w/main"), r#"["spring-boot","npm"]"#.to_string());
        world.storage.insert(ran_key("/w/main"), r#"["app","dev"]"#.to_string());
        world.open(Some("/w/main"));
        world.store.set_configs(vec![main.clone(), vue.clone()]);
        world.store.temporary = strings(&["dev"]);
        let live = RunSession { run_id: Some(RunId(7)), ..session("s2", &vue, SessionStatus::Running, 2.0) };
        world.store.sessions = vec![session("s1", &main, SessionStatus::Finished, 1.0), live];

        let (cleanup, reload) = RemovedProjectCleanup::start("/w/vue", &mut world.store, 3.0);
        assert_eq!(reload.stops, [StopAction::StopRun(RunId(7))]);
        assert_eq!(world.store.sessions.iter().map(|session| session.id.as_str()).collect::<Vec<_>>(), ["s1"]);
        assert_eq!(world.store.configs(), std::slice::from_ref(&main));
        assert_eq!(world.store.temporary, [""; 0]);
        world.apply(reload.persist.writes);
        // The backend lists the configurations again, without the member's.
        world.store.set_configs(vec![main]);
        let writes = cleanup.reloaded(&world.store, &mut world.state);
        world.apply(writes);
        assert_eq!(read_types(world.stored(&types_key("/w/main"))), ["spring-boot"]);
        assert_eq!(read_ran(world.stored(&ran_key("/w/main"))), ["app"]);
    }

    #[test]
    fn keeps_a_type_another_project_still_uses_and_a_sibling_with_a_shared_prefix() {
        let main = in_dir(config("app", "spring-boot"), "/w/main");
        let other = in_dir(config("serve", "npm"), "/w/main");
        let vue = in_dir(config("dev", "npm"), "/w/vue");
        let sibling = in_dir(config("dev2", "npm"), "/w/vue2");
        let mut world = World::new(None);
        world.storage.insert(types_key("/w/main"), r#"["npm"]"#.to_string());
        world.open(Some("/w/main"));
        world.store.set_configs(vec![main.clone(), other.clone(), vue, sibling.clone()]);
        world.store.sessions = vec![RunSession { run_id: Some(RunId(8)), ..running("s3", &sibling) }];

        let (cleanup, reload) = RemovedProjectCleanup::start("/w/vue", &mut world.store, 3.0);
        // The sibling is no part of the removed member: its run goes on.
        assert_eq!(reload, ReloadConfigs { stops: Vec::new(), persist: Persist::default() });
        assert_eq!(world.store.sessions.len(), 1);
        // What the backend lists once /w/vue is no longer a workspace member.
        world.store.set_configs(vec![main, other, sibling]);
        let writes = cleanup.reloaded(&world.store, &mut world.state);
        world.apply(writes);
        let listed: Vec<&str> = world.store.configs().iter().map(|config| config.id.as_str()).collect();
        assert_eq!(listed, ["app", "serve", "dev2"]);
        assert_eq!(read_types(world.stored(&types_key("/w/main"))), ["npm"]);
        assert_eq!(world.state.types(), ["npm"]);
    }

    // service selection

    const NPM_GROUP: &str = "run:type:npm";

    /// The project `/selection` with these npm configurations.
    fn selection_world(ids: &[&str]) -> World {
        let mut world = World::new(Some("/selection"));
        world.store.set_configs(ids.iter().map(|id| config(id, "npm")).collect());
        world
    }

    #[test]
    fn restores_a_hidden_single_configuration_through_its_groups_add_service() {
        let mut world = selection_world(&["dev"]);
        world.hide("dev");
        assert_eq!(read_hidden(world.stored(&hidden_key("/selection"))), by_kind(&[("npm", "dev")]));
        let menu = world.services().menu(NPM_GROUP);
        assert_eq!(menu_command(&menu, "Add Service"), Some(MenuCommand::AddService("npm".into())));
        // A single configuration needs no picker.
        let AddService::Applied(writes) = world.state.add_service("npm", world.store.configs()) else {
            panic!("the configuration is shown again without asking")
        };
        world.apply(writes);
        assert_eq!(read_hidden(world.stored(&hidden_key("/selection"))), by_kind(&[("npm", "")]));
    }

    #[test]
    fn keeps_run_all_and_restart_all_scoped_to_visible_configurations() {
        let mut world = selection_world(&["dev", "test"]);
        let write = world.state.add_type("npm");
        world.apply(write);
        world.hide("dev");
        for id in ["run-all", "restart-all"] {
            assert_eq!(started(&world.run_batch(NPM_GROUP, id)), ["test"], "{id}");
        }
    }

    #[test]
    fn does_not_apply_a_picker_result_after_switching_projects() {
        let mut world = selection_world(&["dev", "test"]);
        let AddService::Pick(pick) = world.state.add_service("npm", world.store.configs()) else {
            panic!("several configurations are offered in a picker")
        };
        assert_eq!(pick.selected, ["dev", "test"]);
        world.open(Some("/other-selection"));
        let picked = strings(&["dev"]);
        let writes = world.state.services_picked(&pick, Some(picked.as_slice()));
        assert_eq!(writes, Vec::new());
        assert_eq!(read_hidden(world.stored(&hidden_key("/other-selection"))), HiddenConfigs::new());
        assert_eq!(read_types(world.stored(&types_key("/other-selection"))), DEFAULT_TYPES);
        assert_eq!((world.state.types(), world.state.hidden()), (default_types().as_slice(), &HiddenConfigs::new()));
    }

    // Not from a spec: the picker's answer hides what was not picked, and a drop reorders within a group only.
    #[test]
    fn a_picked_selection_hides_the_rest_and_a_drop_reorders_one_group() {
        let mut world = selection_world(&["dev", "test", "lint"]);
        let AddService::Pick(pick) = world.state.add_service("npm", world.store.configs()) else {
            panic!("several configurations are offered in a picker")
        };
        let picked = strings(&["dev", "lint"]);
        let writes = world.state.services_picked(&pick, Some(picked.as_slice()));
        world.apply(writes);
        assert_eq!(read_types(world.stored(&types_key("/selection"))), ["spring-boot", "npm"]);
        assert_eq!(read_hidden(world.stored(&hidden_key("/selection"))), by_kind(&[("npm", "test")]));
        let (dev, lint) = ("run:config:dev", "run:config:lint");
        assert!(!world.services().can_move(dev, NPM_GROUP));
        assert_eq!(world.services().moved(dev, "run:config:test", DropPosition::After), None);
        let order = world.services().moved(dev, lint, DropPosition::After).expect("both are shown in one group");
        let write = world.state.set_order(order);
        world.apply(write);
        assert_eq!(read_service_order(world.stored(&order_key("/selection"))), by_kind(&[("npm", "lint dev")]));
        let shown: Vec<String> = world.services().group_configs("npm").into_iter().map(|config| config.id).collect();
        assert_eq!(shown, ["lint", "dev"]);
    }

    // Not from a spec: a node's Run reuses the tab of its last run, and only a stopped configuration is activated.
    #[test]
    fn a_configuration_reruns_in_its_tab_and_activates_only_while_stopped() {
        let a = config("a", "spring-boot");
        let mut world = World::new(None);
        world.store.set_configs(vec![a.clone()]);
        let in_place = StartOptions { show: Some(false), ..StartOptions::default() };
        let fresh = ServiceCommand::StartRun { config: Box::new(a.clone()), options: Box::new(in_place.clone()) };
        assert_eq!(world.services().activate("run:config:a"), Some(fresh));
        world.store.sessions = vec![session("s1", &a, SessionStatus::Finished, 1.0)];
        let options = Box::new(StartOptions { reuse: Some("s1".into()), ..in_place });
        let rerun = ServiceCommand::StartRun { config: Box::new(a.clone()), options };
        assert_eq!(world.services().invoke("run:config:a", "run"), Some(rerun));
        world.store.sessions[0].status = SessionStatus::Running;
        assert_eq!(world.services().activate("run:config:a"), None);
        assert_eq!(world.action("run:config:a", "run").title, "Rerun");
        assert!(!world.action("run:config:a", "update").disabled);
        let update = ServiceCommand::UpdateApplication(Some("s1".into()));
        assert_eq!(world.services().invoke("run:config:a", "update"), Some(update));
    }
}
