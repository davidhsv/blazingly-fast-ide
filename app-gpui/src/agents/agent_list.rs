//! Settings › Agents › Coding Agents: one searchable list of every agent, those this machine can run
//! first (built in, installed from the ACP Registry, or defined in an agent file), then those it can
//! install, each under a header; and the selected agent's details: its facts, its actions, the
//! settings its new tasks start with and how it is launched.
//!
//! [`AgentList`] does no I/O. The agents come from `agent_providers` ([`AgentList::loaded`]); the
//! registry's state is a [`RegistryPage`] whose [`RegistryEffect`]s the caller carries out. Launch
//! edits are staged in the Settings dialog as values kept outside the settings
//! ([`SettingsDialog::stage_external`] on [`REGISTRY_PAGE`]) and written on Apply / OK through
//! [`agent_write`]; the New Tasks choices are the ordinary setting [`DEFAULTS_KEY`]. Installs,
//! removals and custom agents act at once, each after the confirmation the page asks for.
//!
//! An agent's environment values may be API keys. They are shown in the environment field only:
//! no row, fact, error text or `Debug` output of this module holds one.

use crate::agents::builtin_agents::{HintPart, command_key, env_key, format_env, login_hint};
use crate::agents::model::{REGISTRY_PAGE, provider_icon};
use crate::agents::registry::{Failure, RegistryEffect, RegistryPage, distribution_label, matches_query, sort_agents};
use crate::agents::store::{ConfirmRequest, ProvidersDebug};
use crate::platform::i18n::{I18n, Param};
use crate::settings_model::staging::SettingsDialog;
use ide_agent::registry::{AgentRegistry, RegistryAgent};
use ide_agent::{AcpImportOutcome, AgentCommandReadOnlyReason, AgentProvider, AgentSource};
use ide_core::RequestId;
use ide_tools::InstallEvent;
use serde_json::Value;
use std::collections::BTreeMap;

#[cfg(test)]
mod proofs38_agent_list;
mod values;

pub use values::{
    AgentWrite, NewTaskChoice, NewTaskOption, agent_write, chosen_defaults, default_options, defaults_with,
    import_report, is_per_model_option, new_task_options, parse_args, parse_env, slugify_id, stored_value,
};
use values::{command_base, shown};

/// The setting new tasks take their options from: `{agent id: {option id: value}}`.
pub const DEFAULTS_KEY: &str = "agents.defaults";
/// The list's headers, English text shown through `I18n::tx`.
pub const INSTALLED: &str = "Installed";
pub const AVAILABLE: &str = "Available";

// ----- the list -----

/// An agent of the list: one this machine knows (`provider`), one of the registry, or both.
#[derive(Clone, Copy)]
pub struct Agent<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub provider: Option<&'a AgentProvider>,
    pub registry: Option<&'a RegistryAgent>,
}

impl Agent<'_> {
    /// Installed, or not built in, or with an overridden command; a registry-only agent once installed.
    pub fn runnable(&self) -> bool {
        match self.provider {
            Some(p) => p.installed || p.source != AgentSource::Builtin || p.command_overridden,
            None => self.registry.is_some_and(|r| r.installed.is_some()),
        }
    }

    fn matches(&self, query: &str) -> bool {
        if let Some(registry) = self.registry {
            return matches_query(registry, query);
        }
        let text = format!("{} {}", self.name, self.id).to_lowercase();
        query.to_lowercase().split_whitespace().all(|word| text.contains(word))
    }
}

/// A row's trailing tag; `accent` for Update and Custom.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub label: String,
    pub accent: bool,
}

/// What the agent needs, if anything, as English text and its tone.
pub fn agent_status(agent: &AgentProvider) -> Option<(&'static str, bool)> {
    match &agent.source {
        AgentSource::Registry { update: Some(_), .. } => Some(("Update", true)),
        _ if agent.command_overridden || agent.source == AgentSource::Custom => Some(("Custom", true)),
        _ if !agent.installed => Some(("Not found", false)),
        _ => None,
    }
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListRow {
    /// [`INSTALLED`] or [`AVAILABLE`], in English.
    Section(&'static str),
    Agent(AgentItem),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentItem {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub tag: Option<Tag>,
    /// Not runnable: drawn muted.
    pub muted: bool,
    /// An install of it runs: a spinner in place of the tag.
    pub working: bool,
}

/// Where the agent's executable comes from, as the dot before the facts shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dot {
    Custom,
    Found,
    Missing,
}

/// A button of the details' header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Install,
    Update,
    /// Removes a registry install.
    Remove,
    /// Removes a custom agent's file.
    RemoveCustom,
    /// Stops the install the page started.
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionButton {
    pub action: Action,
    pub label: String,
    pub enabled: bool,
    pub primary: bool,
}

/// An install's progress: 0..1 when the size is known, and the last line it reported.
#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub fraction: Option<f64>,
    pub message: String,
}

/// The Sign In, New Tasks and Launch sections of an agent this machine knows.
#[derive(Clone, PartialEq)]
pub struct ProviderSettings {
    pub login: Vec<HintPart>,
    pub new_tasks: Vec<NewTaskOption>,
    /// The default command line, also the Command row's tooltip.
    pub launch: String,
    /// The executable field (`None`: read-only, with `note` saying why).
    pub executable: Option<String>,
    pub placeholder: String,
    /// The Reset button shows while the field has a value.
    pub reset: bool,
    pub note: Option<String>,
    /// The environment field, for an agent launched by its own executable. Holds the values.
    pub env: Option<String>,
}

/// Leaves the environment's value out.
impl std::fmt::Debug for ProviderSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderSettings")
            .field("login", &self.login)
            .field("new_tasks", &self.new_tasks)
            .field("launch", &self.launch)
            .field("executable", &self.executable)
            .field("placeholder", &self.placeholder)
            .field("reset", &self.reset)
            .field("note", &self.note)
            .field("env", &self.env.as_ref().map(|_| "…"))
            .finish()
    }
}

/// What the details show of the selected agent.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentDetails {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// For an agent this machine knows.
    pub dot: Option<Dot>,
    /// "Found on PATH", "ACP Registry · 3.0.70"…; for a registry-only agent its version, distribution,
    /// authors and license.
    pub facts: String,
    pub actions: Vec<ActionButton>,
    pub progress: Option<Progress>,
    /// "3.0.74 is available".
    pub update: Option<String>,
    pub error: Option<String>,
    pub description: Option<String>,
    /// "No checksum provided: the download can't be verified.", for a registry-only agent.
    pub no_checksum: Option<String>,
    /// Repository and Website (English labels), web pages only.
    pub links: Vec<(&'static str, String)>,
    pub third_party: Option<String>,
    pub settings: Option<ProviderSettings>,
}

/// The registry's state line, under the buttons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryState {
    Loading,
    /// The registry could not be reached and the copy saved earlier is listed.
    Warning(String),
    /// "Cannot load the ACP Registry: …".
    Error(String),
}

/// A question the page asks before it acts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pending {
    /// Install or Update of an agent whose download has no checksum.
    Install { id: String, request: ConfirmRequest },
    /// Remove of a registry install.
    Remove { id: String, request: ConfirmRequest },
    /// Remove of a custom agent and its agent file.
    RemoveCustom { id: String, request: ConfirmRequest },
}

impl Pending {
    pub fn request(&self) -> &ConfirmRequest {
        match self {
            Self::Install { request, .. } | Self::Remove { request, .. } | Self::RemoveCustom { request, .. } => {
                request
            }
        }
    }
}

/// What the page asks of the caller.
#[derive(Clone, Debug, PartialEq)]
pub enum ListEffect {
    Registry(RegistryEffect),
    /// `agent_custom_remove`, then call [`AgentList::custom_removed`].
    RemoveCustom(String),
}

/// What the registry's model says of one agent, kept after each change of it.
#[derive(Clone, Debug, Default, PartialEq)]
struct RowState {
    job: Option<Progress>,
    cancellable: bool,
    error: Option<String>,
}

/// The page's state. Its `Debug` leaves out the agents' environment values.
#[derive(Default)]
pub struct AgentList {
    providers: Option<Vec<AgentProvider>>,
    error: Option<String>,
    /// `None` while the page lists no registry.
    registry: Option<RegistryPage>,
    /// The listing the page opened with; its answer, unless it failed, makes the registry known.
    opened_with: Option<RequestId>,
    registry_listed: bool,
    /// The registry's agents and what its model says of each, after its last change.
    registry_agents: Vec<RegistryAgent>,
    registry_rows: BTreeMap<String, RowState>,
    registry_busy: bool,
    pub query: String,
    selected: Option<String>,
    removing: Option<String>,
    errors: BTreeMap<String, String>,
    pending: Option<Pending>,
    importing: bool,
    notice: Option<String>,
}

impl std::fmt::Debug for AgentList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentList")
            .field("providers", &self.providers.as_deref().map(ProvidersDebug))
            .field("error", &self.error)
            .field("registry", &self.registry)
            .field("query", &self.query)
            .field("selected", &self.selected)
            .field("removing", &self.removing)
            .field("errors", &self.errors)
            .field("pending", &self.pending)
            .field("importing", &self.importing)
            .field("notice", &self.notice)
            .finish()
    }
}

impl AgentList {
    /// The page opens: everything starts afresh, and the registry is listed with `registry` when the
    /// page shows it.
    pub fn open(registry: Option<RequestId>) -> (Self, Vec<ListEffect>) {
        let mut list = Self::default();
        let Some(request) = registry else { return (list, Vec::new()) };
        let (page, effect) = RegistryPage::open(request);
        list.registry = Some(page);
        list.opened_with = Some(request);
        (list, vec![ListEffect::Registry(effect)])
    }

    /// The agents were listed (again, after a change); a failure (no trusted project) is shown instead.
    pub fn loaded(&mut self, agents: Result<Vec<AgentProvider>, String>) {
        match agents {
            Ok(agents) => {
                self.providers = Some(agents);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.providers.is_some() || self.error.is_some()
    }

    /// Why the agents could not be read: the page shows only that.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn providers(&self) -> &[AgentProvider] {
        self.providers.as_deref().unwrap_or_default()
    }

    pub fn shows_registry(&self) -> bool {
        self.registry.is_some()
    }

    /// The registry's model changed: what the list and the details read of it is taken again.
    fn sync_registry(&mut self, i18n: &mut I18n) {
        let Some(page) = &self.registry else { return };
        let rows = page.rows(i18n);
        self.registry_busy = rows.first().is_some_and(|row| !row.enabled);
        self.registry_agents = rows.iter().map(|row| row.agent.clone()).collect();
        self.registry_rows = rows
            .iter()
            .map(|row| {
                let job = row.job.map(|job| Progress { fraction: job.fraction, message: job.message.clone() });
                let state = RowState { job, cancellable: row.cancellable, error: row.error.map(str::to_owned) };
                (row.agent.id.clone(), state)
            })
            .collect();
    }

    fn registry_effects(effects: Vec<RegistryEffect>) -> Vec<ListEffect> {
        effects.into_iter().map(ListEffect::Registry).collect()
    }

    /// The answer of a [`RegistryEffect::List`]: the opening listing or a poll.
    pub fn registry_answered(
        &mut self,
        request: RequestId,
        answer: Result<AgentRegistry, Failure>,
        i18n: &mut I18n,
    ) -> Vec<ListEffect> {
        let Some(page) = &mut self.registry else { return Vec::new() };
        let effects = if self.opened_with == Some(request) {
            self.opened_with = None;
            self.registry_listed |= answer.is_ok();
            page.listed(request, answer, i18n)
        } else {
            page.polled(request, answer, i18n)
        };
        self.sync_registry(i18n);
        Self::registry_effects(effects)
    }

    /// A tick of [`RegistryEffect::PollEvery`]; `None` once nothing runs in the background any more.
    pub fn poll(&mut self, request: RequestId) -> Option<ListEffect> {
        self.registry.as_mut()?.poll(request).map(ListEffect::Registry)
    }

    /// Whether installs started elsewhere are followed.
    pub fn following_background(&self) -> bool {
        self.registry.as_ref().is_some_and(RegistryPage::following_background)
    }

    /// The registry's state line: while it is listed, a warning, or why it could not be.
    pub fn registry_state(&self, i18n: &mut I18n) -> Option<RegistryState> {
        let page = self.registry.as_ref()?;
        if let Some(error) = page.load_error(i18n) {
            return Some(RegistryState::Error(error));
        }
        if let Some(warning) = page.warning() {
            return Some(RegistryState::Warning(warning.to_owned()));
        }
        (!self.registry_listed).then_some(RegistryState::Loading)
    }

    /// One install or removal at a time, and nothing while one runs in the background.
    pub fn busy(&self) -> bool {
        self.removing.is_some() || self.registry_busy
    }

    fn agents(&self) -> Vec<Agent<'_>> {
        let providers = self.providers();
        let mut others: Vec<RegistryAgent> =
            self.registry_agents.iter().filter(|r| !providers.iter().any(|p| p.id == r.id)).cloned().collect();
        sort_agents(&mut others);
        let order: Vec<&str> = others.iter().map(|r| r.id.as_str()).collect();
        let known = providers.iter().map(|p| Agent {
            id: &p.id,
            name: &p.name,
            provider: Some(p),
            registry: self.registry_agents.iter().find(|r| r.id == p.id),
        });
        let unknown = order
            .into_iter()
            .filter_map(|id| self.registry_agents.iter().find(|r| r.id == id))
            .map(|r| Agent { id: &r.id, name: &r.name, provider: None, registry: Some(r) });
        known.chain(unknown).collect()
    }

    /// The agents the search keeps: the runnable ones in the backend's order, then those that can be
    /// installed (built-ins not found, then the registry's).
    fn shown_agents(&self) -> (Vec<Agent<'_>>, Vec<Agent<'_>>) {
        let all: Vec<Agent<'_>> = self.agents().into_iter().filter(|a| a.matches(&self.query)).collect();
        all.into_iter().partition(Agent::runnable)
    }

    fn working(&self, id: &str) -> bool {
        self.registry_rows.get(id).is_some_and(|row| row.job.is_some())
    }

    /// The rows of the list: each section's header over its agents; empty with nothing to show.
    pub fn rows(&self, i18n: &I18n) -> Vec<ListRow> {
        if self.providers.is_none() {
            return Vec::new();
        }
        let (installed, available) = self.shown_agents();
        let mut out = Vec::new();
        for (title, agents) in [(INSTALLED, installed), (AVAILABLE, available)] {
            if agents.is_empty() {
                continue;
            }
            out.push(ListRow::Section(title));
            for agent in agents {
                let tag = match agent.provider {
                    Some(provider) => agent_status(provider),
                    None => agent.registry.filter(|r| r.installed.is_some() && r.update).map(|_| ("Update", true)),
                };
                out.push(ListRow::Agent(AgentItem {
                    id: agent.id.to_owned(),
                    name: agent.name.to_owned(),
                    icon: provider_icon(agent.id),
                    tag: tag.map(|(label, accent)| Tag { label: i18n.tx(label), accent }),
                    muted: !agent.runnable(),
                    working: self.working(agent.id),
                }));
            }
        }
        out
    }

    /// "No agents match", when the search keeps nothing.
    pub fn nothing_matches(&self) -> bool {
        self.providers.is_some() && !self.query.trim().is_empty() && self.shown_ids().is_empty()
    }

    fn shown_ids(&self) -> Vec<String> {
        let (installed, available) = self.shown_agents();
        installed.into_iter().chain(available).map(|a| a.id.to_owned()).collect()
    }

    pub fn select(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
    }

    /// The agent selected, else the first one shown.
    pub fn selected_id(&self) -> Option<String> {
        let ids = self.shown_ids();
        let chosen = self.selected.as_ref().filter(|id| ids.contains(id));
        chosen.cloned().or_else(|| ids.into_iter().next())
    }

    fn selected_agent(&self) -> Option<Agent<'_>> {
        let id = self.selected_id()?;
        self.agents().into_iter().find(|a| a.id == id)
    }

    /// The agent selected moved by `by` rows (sections skipped), kept within the list.
    pub fn select_next(&mut self, by: isize) {
        let ids = self.shown_ids();
        let Some(last) = ids.len().checked_sub(1) else { return };
        let at = self.selected_id().and_then(|id| ids.iter().position(|i| *i == id)).unwrap_or_default();
        let next = at.saturating_add_signed(by).min(last);
        self.selected = Some(ids[next].clone());
    }

    pub fn select_edge(&mut self, last: bool) {
        let ids = self.shown_ids();
        let edge = if last { ids.last() } else { ids.first() };
        self.selected = edge.cloned();
    }

    /// The selected agent's details. `defaults` is the value of [`DEFAULTS_KEY`] as the page shows it.
    pub fn details(&self, dialog: &SettingsDialog, defaults: Option<&Value>, i18n: &mut I18n) -> Option<AgentDetails> {
        if self.error.is_some() {
            return None;
        }
        let agent = self.selected_agent()?;
        let (provider, registry) = (agent.provider, agent.registry);
        let installed = provider.map_or(registry.is_some_and(|r| r.installed.is_some()), |p| {
            matches!(p.source, AgentSource::Registry { .. })
        });
        let custom = provider.is_some_and(|p| p.source == AgentSource::Custom);
        let can_install = registry.is_some_and(|r| r.distribution.is_some());
        let update = match provider.map(|p| &p.source) {
            Some(AgentSource::Registry { update, .. }) => update.clone(),
            _ => registry.filter(|r| r.update).map(|r| r.version.clone()),
        };
        let facts = match provider {
            Some(p) => provider_status(p, i18n),
            None => {
                let r = registry?;
                let distribution = r.distribution.map(|d| i18n.tx(distribution_label(d)));
                let authors = Some(r.authors.join(", ")).filter(|a| !a.is_empty());
                let facts = [Some(r.version.clone()), distribution, authors, r.license.clone()];
                facts.into_iter().flatten().filter(|f| !f.is_empty()).collect::<Vec<_>>().join(" · ")
            }
        };
        let dot = provider.map(|p| {
            if p.command_overridden || p.source == AgentSource::Custom {
                Dot::Custom
            } else if p.installed {
                Dot::Found
            } else {
                Dot::Missing
            }
        });
        let row = self.registry_rows.get(agent.id).cloned().unwrap_or_default();
        let enabled = !self.busy();
        let removing = self.removing.as_deref() == Some(agent.id);
        let mut actions = Vec::new();
        let button = |action, label: &str, primary, i18n: &mut I18n| ActionButton {
            action,
            label: i18n.tx(label),
            enabled,
            primary,
        };
        if row.job.is_some() {
            if row.cancellable {
                let label = i18n.tx("Cancel");
                actions.push(ActionButton { action: Action::Cancel, label, enabled: true, primary: false });
            }
        } else {
            if !agent.runnable() && can_install {
                actions.push(button(Action::Install, "Install", true, i18n));
            }
            if installed && update.is_some() && can_install {
                actions.push(button(Action::Update, "Update", true, i18n));
            }
            let remove = if removing { "Removing…" } else { "Remove" };
            if installed {
                actions.push(button(Action::Remove, remove, false, i18n));
            }
            if custom {
                actions.push(button(Action::RemoveCustom, remove, false, i18n));
            }
        }
        let update_notice = update
            .filter(|_| row.job.is_none() && installed)
            .map(|version| i18n.tx_built(&["", " is available"], &[Param::from(version.as_str())]));
        let web = |url: &Option<String>| {
            url.as_ref().filter(|u| u.starts_with("https://") || u.starts_with("http://")).cloned()
        };
        let mut links = Vec::new();
        if let Some(r) = registry {
            if let Some(url) = web(&r.repository) {
                links.push(("Repository", url));
            }
            if let Some(url) = web(&r.website).filter(|url| r.repository.as_ref() != Some(url)) {
                links.push(("Website", url));
            }
        }
        Some(AgentDetails {
            id: agent.id.to_owned(),
            name: agent.name.to_owned(),
            icon: provider_icon(agent.id),
            dot,
            facts,
            actions,
            progress: row.job.clone(),
            update: update_notice,
            error: self.errors.get(agent.id).cloned().or(row.error),
            description: registry.map(|r| r.description.clone()).filter(|d| !d.is_empty()),
            no_checksum: registry
                .filter(|r| provider.is_none() && r.no_checksum)
                .map(|_| i18n.tx("No checksum provided: the download can't be verified.")),
            links,
            third_party: registry.map(|_| {
                i18n.tx(concat!(
                    "Agents in the ACP Registry are third-party software. They run on this computer with your ",
                    "permissions and can read and change your files. Install only agents you trust."
                ))
            }),
            settings: provider.map(|p| provider_settings(p, dialog, defaults, i18n)),
        })
    }

    /// The executable field changed (typed, cleared, reset or browsed); a read-only agent has none.
    pub fn set_command(&self, dialog: &mut SettingsDialog, text: &str) {
        let Some(agent) = self.selected_agent().and_then(|a| a.provider).filter(|p| p.command_editable) else {
            return;
        };
        let key = command_key(&agent.id);
        let (_, current) = shown(dialog, &key, command_base(agent));
        dialog.stage_external(REGISTRY_PAGE, &key, Value::String(text.to_owned()), &Value::String(current));
    }

    /// The environment field changed.
    pub fn set_env(&self, dialog: &mut SettingsDialog, text: &str) {
        let Some(agent) = self.selected_agent().and_then(|a| a.provider) else { return };
        let key = env_key(&agent.id);
        let (_, current) = shown(dialog, &key, format_env(&agent.env));
        dialog.stage_external(REGISTRY_PAGE, &key, Value::String(text.to_owned()), &Value::String(current));
    }

    /// Browse… beside the executable: the agent it is for and the chooser's title. A read-only agent
    /// has no button.
    pub fn browse(&self, i18n: &mut I18n) -> Option<(String, String)> {
        let agent = self.selected_agent()?.provider.filter(|p| p.command_editable)?;
        let title = i18n.tx_built(&["Select ", ""], &[Param::from(agent.name.as_str())]);
        Some((agent.id.clone(), title))
    }

    // ----- acting on agents -----

    fn name_of(&self, id: &str) -> String {
        self.agents().into_iter().find(|a| a.id == id).map_or_else(|| id.to_owned(), |a| a.name.to_owned())
    }

    /// Install or Update of agent `id`; `request` is the id the install runs with. An agent whose
    /// download has no checksum is asked about first.
    pub fn install(&mut self, id: &str, request: RequestId, i18n: &mut I18n) -> Vec<ListEffect> {
        if self.removing.is_some() {
            return Vec::new();
        }
        let Some(page) = &mut self.registry else { return Vec::new() };
        let mut effects = page.install(id, request, i18n);
        if let Some(at) = effects.iter().position(|e| matches!(e, RegistryEffect::Confirm { .. }))
            && let RegistryEffect::Confirm { id, request } = effects.remove(at)
        {
            self.pending = Some(Pending::Install { id, request });
        }
        self.errors.remove(id);
        self.sync_registry(i18n);
        Self::registry_effects(effects)
    }

    /// Remove: a registry install or a custom agent, after asking.
    pub fn ask_remove(&mut self, id: &str, custom: bool, i18n: &mut I18n) {
        if self.busy() {
            return;
        }
        let name = Param::from(self.name_of(id));
        let title = i18n.tx_built(&["Remove ", ""], std::slice::from_ref(&name));
        let ok = i18n.tx("Remove");
        self.pending = Some(if custom {
            let message = i18n.tx_built(
                &["Remove ", " and delete its agent file? Its tasks stay, but can't continue without it."],
                &[name],
            );
            Pending::RemoveCustom { id: id.to_owned(), request: ConfirmRequest { title, message, ok } }
        } else {
            let message = i18n.tx_built(
                &["Remove ", " from this computer? Its tasks stay, but can't continue until it is installed again."],
                &[name],
            );
            Pending::Remove { id: id.to_owned(), request: ConfirmRequest { title, message, ok } }
        });
    }

    /// The question the page asks now.
    pub fn pending(&self) -> Option<&Pending> {
        self.pending.as_ref()
    }

    /// The answer to [`AgentList::pending`]; `request` is the id an install it allows runs with.
    pub fn answer(&mut self, ok: bool, request: RequestId, i18n: &mut I18n) -> Vec<ListEffect> {
        let Some(pending) = self.pending.take() else { return Vec::new() };
        if !ok || self.busy() {
            return Vec::new();
        }
        let effects = match pending {
            Pending::Install { id, .. } => {
                let Some(page) = &mut self.registry else { return Vec::new() };
                Self::registry_effects(page.confirmed(&id, true, request, i18n))
            }
            Pending::Remove { id, .. } => {
                let Some(page) = &mut self.registry else { return Vec::new() };
                let effects = page.remove(&id);
                if !effects.is_empty() {
                    self.errors.remove(&id);
                    self.removing = Some(id);
                }
                Self::registry_effects(effects)
            }
            Pending::RemoveCustom { id, .. } => {
                self.errors.remove(&id);
                self.removing = Some(id.clone());
                vec![ListEffect::RemoveCustom(id)]
            }
        };
        self.sync_registry(i18n);
        effects
    }

    pub fn install_event(&mut self, request: RequestId, event: InstallEvent, i18n: &mut I18n) {
        if let Some(page) = &mut self.registry {
            page.install_event(request, event);
        }
        self.sync_registry(i18n);
    }

    /// The install ended: on success the agent list changed; a failure (other than a cancellation)
    /// shows in its details.
    pub fn installed(&mut self, request: RequestId, result: Result<(), Failure>, i18n: &mut I18n) -> Vec<ListEffect> {
        let Some(page) = &mut self.registry else { return Vec::new() };
        let effects = page.installed(request, result);
        self.sync_registry(i18n);
        Self::registry_effects(effects)
    }

    /// Cancel in the details of the install the page started.
    pub fn cancel_install(&self) -> Option<ListEffect> {
        self.registry.as_ref()?.cancel_install().map(ListEffect::Registry)
    }

    /// The answer of [`RegistryEffect::Remove`].
    pub fn removed(&mut self, id: &str, result: Result<(), String>, i18n: &mut I18n) -> Vec<ListEffect> {
        if self.removing.as_deref() == Some(id) {
            self.removing = None;
        }
        let Some(page) = &mut self.registry else { return Vec::new() };
        let effects = page.removed(id, result);
        self.sync_registry(i18n);
        Self::registry_effects(effects)
    }

    /// The answer of [`ListEffect::RemoveCustom`]: `true` when the agent list changed.
    pub fn custom_removed(&mut self, id: &str, result: Result<(), String>) -> bool {
        if self.removing.as_deref() == Some(id) {
            self.removing = None;
        }
        match result {
            Ok(()) => true,
            Err(error) => {
                self.errors.insert(id.to_owned(), error);
                false
            }
        }
    }

    /// The page closes: its listings are cancelled; an install keeps running in the background.
    pub fn close(&mut self) -> Vec<ListEffect> {
        let effects = self.registry.as_mut().map(RegistryPage::close).unwrap_or_default();
        Self::registry_effects(effects)
    }

    // ----- Import acp.json -----

    pub fn importing(&self) -> bool {
        self.importing
    }

    /// Import acp.json was pressed: the report of the last one goes.
    pub fn import_started(&mut self) -> bool {
        if self.importing || self.busy() {
            return false;
        }
        self.importing = true;
        self.notice = None;
        true
    }

    /// The import ended: `None` when no file was chosen, else what it saved and skipped, or why it
    /// failed. `true` when the agent list may have changed.
    pub fn import_finished(&mut self, result: Option<Result<Vec<AcpImportOutcome>, String>>, i18n: &mut I18n) -> bool {
        self.importing = false;
        match result {
            None => false,
            Some(Ok(outcomes)) => {
                self.notice = Some(import_report(&outcomes, i18n));
                true
            }
            Some(Err(error)) => {
                self.notice = Some(error);
                false
            }
        }
    }

    /// The line under the buttons: what the last import did.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }
}

/// Where the agent's command comes from.
pub fn provider_status(agent: &AgentProvider, i18n: &mut I18n) -> String {
    match &agent.source {
        AgentSource::Registry { version, .. } => {
            i18n.tx_built(&["ACP Registry · ", ""], &[Param::from(version.as_str())])
        }
        AgentSource::Custom => i18n.tx("Agent file"),
        AgentSource::Builtin if agent.command_overridden => i18n.tx("Custom executable"),
        AgentSource::Builtin if agent.installed => i18n.tx("Found on PATH"),
        AgentSource::Builtin => i18n.tx("Not found on PATH"),
    }
}

fn provider_settings(
    agent: &AgentProvider,
    dialog: &SettingsDialog,
    defaults: Option<&Value>,
    i18n: &mut I18n,
) -> ProviderSettings {
    let default = agent.default_program.clone().unwrap_or_default();
    let executable = agent.command_editable.then(|| shown(dialog, &command_key(&agent.id), command_base(agent)).0);
    let note = (!agent.command_editable).then(|| {
        i18n.tx(match agent.command_read_only_reason {
            Some(AgentCommandReadOnlyReason::PackageRunner) => {
                "This agent runs through a package runner (npm or uv) and has no executable to choose."
            }
            Some(AgentCommandReadOnlyReason::Registry) => {
                "This agent is installed from the ACP Registry, which launches it."
            }
            _ => "This agent is configured in its agent file.",
        })
    });
    let own_env = !matches!(
        agent.command_read_only_reason,
        Some(AgentCommandReadOnlyReason::Registry | AgentCommandReadOnlyReason::CustomAgent)
    );
    let env = own_env.then(|| shown(dialog, &env_key(&agent.id), format_env(&agent.env)).0);
    ProviderSettings {
        login: agent.login_hint.as_deref().map(|hint| login_hint(hint, i18n)).unwrap_or_default(),
        new_tasks: new_task_options(agent, defaults, i18n),
        launch: std::iter::once(&default).chain(&agent.default_args).cloned().collect::<Vec<_>>().join(" "),
        reset: executable.as_deref().is_some_and(|v| !v.is_empty()),
        executable,
        placeholder: i18n.tx_built(&["Auto-detect (", ")"], &[Param::from(default.as_str())]),
        note,
        env,
    }
}
