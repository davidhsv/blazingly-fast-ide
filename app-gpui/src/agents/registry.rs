//! Agents of the ACP Registry (Settings › Tools › Agents): listing and searching them, install / update /
//! remove with progress, and the New Task page's agent picker.
//!
//! [`RegistryPage`] does no I/O: it returns [`RegistryEffect`]s and takes the answers with the request id
//! they were made with. Installs keep running after the page closes (they become background tasks of
//! the project windows); a page opened meanwhile follows them by listing the registry again every
//! [`BACKGROUND_POLL_MS`].

use crate::agents::store::ConfirmRequest;
use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use ide_agent::registry::{AgentRegistry, RegistryAgent, RegistryDistribution};
use ide_agent::{AgentProvider, AgentSource};
use ide_core::RequestId;
use ide_tools::InstallEvent;
use std::collections::BTreeMap;

/// How often the page checks on installs it did not start.
pub const BACKGROUND_POLL_MS: f64 = 1500.0;

/// English text, shown through `I18n::tx`.
pub fn distribution_label(distribution: RegistryDistribution) -> &'static str {
    match distribution {
        RegistryDistribution::Binary => "Binary",
        RegistryDistribution::Npx => "npm",
        RegistryDistribution::Uvx => "uv",
    }
}

/// Every word of `query` occurs in the agent's name, id, description or authors.
pub fn matches_query(agent: &RegistryAgent, query: &str) -> bool {
    let text = [agent.name.as_str(), agent.id.as_str(), agent.description.as_str()]
        .into_iter()
        .chain(agent.authors.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    query.to_lowercase().split_whitespace().all(|word| text.contains(word))
}

/// What an agent's row offers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RowActions {
    pub install: bool,
    pub update: bool,
    pub remove: bool,
}

/// Install, or for an installed agent update and remove.
pub fn row_actions(agent: &RegistryAgent) -> RowActions {
    let installed = agent.installed.is_some();
    RowActions {
        install: !installed && agent.distribution.is_some(),
        update: installed && agent.update && agent.distribution.is_some(),
        remove: installed,
    }
}

/// The registry list after `id` was installed at its current version.
pub fn after_install(agents: &mut [RegistryAgent], id: &str) {
    for agent in agents.iter_mut().filter(|a| a.id == id) {
        agent.installed = Some(agent.version.clone());
        agent.update = false;
    }
}

/// The registry list after `id` was removed: an agent this system cannot install again goes.
pub fn after_remove(agents: &mut Vec<RegistryAgent>, id: &str) {
    for agent in agents.iter_mut().filter(|a| a.id == id) {
        agent.installed = None;
        agent.update = false;
    }
    agents.retain(|a| a.installed.is_some() || a.distribution.is_some());
}

/// Registry ids of agents from established vendors, most familiar first. The registry has no popularity
/// data and lists alphabetically, which puts small or unrelated projects on top.
const FEATURED: [&str; 16] = [
    "copilot",
    "gemini",
    "cursor",
    "opencode",
    "amp-acp",
    "devin",
    "factory-droid",
    "junie",
    "cline",
    "kilo",
    "antigravity-acp",
    "grok-build",
    "qoder",
    "pi-acp",
    "poolside",
    "deepagents",
];

/// The order of the registry list: installed agents first, then the featured ones in their order, then
/// the rest by name.
pub fn agent_order(a: &RegistryAgent, b: &RegistryAgent) -> std::cmp::Ordering {
    let rank =
        |a: &RegistryAgent| (a.installed.is_none(), FEATURED.iter().position(|f| *f == a.id).unwrap_or(FEATURED.len()));
    rank(a).cmp(&rank(b)).then_with(|| collate::compare(&a.name, &b.name))
}

pub fn sort_agents(agents: &mut [RegistryAgent]) {
    agents.sort_by(agent_order);
}

/// Picker entry that opens Settings › Tools › Agents instead of choosing an agent.
pub const ADD_AGENT: &str = "__addAgent__";
/// The title of the agents that are not installed.
pub const MISSING: &str = "__missing__";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerOption {
    pub value: String,
    pub label: String,
    pub icon: Option<String>,
    pub detail: Option<String>,
    /// A section title, not a choice.
    pub section: bool,
}

fn choice(p: &AgentProvider, icon: &impl Fn(&str) -> String, detail: Option<String>) -> PickerOption {
    PickerOption { value: p.id.clone(), label: p.name.clone(), icon: Some(icon(&p.id)), detail, section: false }
}

/// The New Task page's agent picker: installed agents first; the rest under a "Not installed" title,
/// with the command they need; then More agents…. Registry agents show their version, or the update
/// on offer.
pub fn provider_options(
    providers: &[AgentProvider],
    i18n: &mut I18n,
    icon: impl Fn(&str) -> String,
) -> Vec<PickerOption> {
    let mut out = Vec::new();
    for p in providers.iter().filter(|p| p.installed) {
        let detail = match &p.source {
            AgentSource::Registry { update: Some(update), .. } => {
                Some(i18n.tx_built(&["", " available"], &[Param::from(update.as_str())]))
            }
            AgentSource::Registry { version, .. } => Some(version.clone()),
            _ => None,
        };
        out.push(choice(p, &icon, detail));
    }
    let missing: Vec<&AgentProvider> = providers.iter().filter(|p| !p.installed).collect();
    if !missing.is_empty() {
        let label = i18n.tx("Not installed");
        out.push(PickerOption { value: MISSING.into(), label, icon: None, detail: None, section: true });
    }
    for p in missing {
        out.push(choice(p, &icon, Some(p.command.clone())));
    }
    let label = i18n.tx("More agents…");
    out.push(PickerOption { value: ADD_AGENT.into(), label, icon: None, detail: None, section: false });
    out
}

// ----- the settings page -----

/// Why a request failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// It was cancelled: nothing to report.
    Cancelled,
    Error(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RegistryEffect {
    /// List the registry with this request id, then call [`RegistryPage::listed`].
    List(RequestId),
    /// Install (or update) the agent, streaming its events to [`RegistryPage::install_event`]; then
    /// call [`RegistryPage::installed`].
    Install {
        id: String,
        request: RequestId,
    },
    /// Remove the agent, then call [`RegistryPage::removed`].
    Remove {
        id: String,
    },
    Cancel(RequestId),
    /// Ask, then call [`RegistryPage::confirmed`] for the agent.
    Confirm {
        id: String,
        request: ConfirmRequest,
    },
    /// Every `ms`, call [`RegistryPage::poll`] while the page shows background installs.
    PollEvery {
        ms: f64,
    },
    /// An agent was installed, updated or removed: the agent list changed.
    Changed,
}

/// An install the page follows.
#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub id: String,
    /// `None` for an install started before the page opened.
    request: Option<RequestId>,
    /// 0..1 when the size is known.
    pub fraction: Option<f64>,
    pub message: String,
}

/// How an agent's row looks.
#[derive(Clone, Debug, PartialEq)]
pub struct RowView<'a> {
    pub agent: &'a RegistryAgent,
    /// Which buttons show, and whether they can be clicked now (one install or removal at a time).
    pub actions: RowActions,
    pub enabled: bool,
    /// "0.10.0 available", for an installed agent with an update.
    pub update_label: Option<String>,
    pub installed_label: Option<String>,
    pub no_checksum: Option<String>,
    /// The install under way for this agent; its Cancel shows when the page started it.
    pub job: Option<&'a Job>,
    pub cancellable: bool,
    pub error: Option<&'a str>,
}

#[derive(Debug, Default)]
pub struct RegistryPage {
    registry: Option<AgentRegistry>,
    load_error: Option<String>,
    request: Option<RequestId>,
    poll: Option<RequestId>,
    pub query: String,
    job: Option<Job>,
    removing: Option<String>,
    errors: BTreeMap<String, String>,
}

impl RegistryPage {
    /// The page opens: the registry is listed with `request`.
    pub fn open(request: RequestId) -> (Self, RegistryEffect) {
        (Self { request: Some(request), ..Self::default() }, RegistryEffect::List(request))
    }

    pub fn listed(
        &mut self,
        request: RequestId,
        answer: Result<AgentRegistry, Failure>,
        i18n: &mut I18n,
    ) -> Vec<RegistryEffect> {
        if self.request != Some(request) {
            return Vec::new();
        }
        self.request = None;
        match answer {
            Ok(registry) => self.set_registry(registry, i18n),
            Err(Failure::Cancelled) => Vec::new(),
            Err(Failure::Error(e)) => {
                self.load_error = Some(e);
                Vec::new()
            }
        }
    }

    fn set_registry(&mut self, registry: AgentRegistry, i18n: &mut I18n) -> Vec<RegistryEffect> {
        let background = registry.agents.iter().find(|a| a.installing).map(|a| Job {
            id: a.id.clone(),
            request: None,
            fraction: None,
            message: i18n.tx_built(&["Installing ", " in the background…"], &[Param::from(a.name.as_str())]),
        });
        self.registry = Some(registry);
        if self.job.as_ref().is_none_or(|j| j.request.is_none()) {
            self.job = background;
        }
        if self.job.as_ref().is_some_and(|j| j.request.is_none()) {
            vec![RegistryEffect::PollEvery { ms: BACKGROUND_POLL_MS }]
        } else {
            Vec::new()
        }
    }

    /// Shown when the registry could not be reached and the copy saved earlier is listed.
    pub fn warning(&self) -> Option<&str> {
        self.registry.as_ref()?.warning.as_deref()
    }

    /// "Cannot load the ACP Registry: …".
    pub fn load_error(&self, i18n: &mut I18n) -> Option<String> {
        let error = self.load_error.as_deref()?;
        Some(i18n.tx_built(&["Cannot load the ACP Registry: ", ""], &[Param::from(error)]))
    }

    /// Installs started before the page opened are followed until they end.
    pub fn following_background(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.request.is_none())
    }

    fn busy(&self) -> bool {
        self.job.is_some() || self.removing.is_some()
    }

    /// The rows matching the search, in the order [`sort_agents`] gives.
    pub fn rows(&self, i18n: &mut I18n) -> Vec<RowView<'_>> {
        let Some(registry) = &self.registry else { return Vec::new() };
        let mut agents: Vec<&RegistryAgent> =
            registry.agents.iter().filter(|a| matches_query(a, &self.query)).collect();
        agents.sort_by(|a, b| agent_order(a, b));
        agents
            .into_iter()
            .map(|agent| {
                let job = self.job.as_ref().filter(|j| j.id == agent.id);
                // An install started elsewhere is stopped from the background tasks: the row offers nothing.
                let elsewhere = job.is_some_and(|j| j.request.is_none());
                let actions = if elsewhere { RowActions::default() } else { row_actions(agent) };
                RowView {
                    agent,
                    actions,
                    enabled: !self.busy(),
                    update_label: (agent.installed.is_some() && agent.update)
                        .then(|| i18n.tx_built(&["", " available"], &[Param::from(agent.version.as_str())])),
                    installed_label: (agent.installed.is_some() && !agent.update).then(|| i18n.tx("Installed")),
                    no_checksum: (agent.no_checksum && agent.installed.is_none())
                        .then(|| i18n.tx("No checksum provided")),
                    job,
                    cancellable: job.is_some_and(|j| j.request.is_some()),
                    error: self.errors.get(&agent.id).map(String::as_str),
                }
            })
            .collect()
    }

    fn agent(&self, id: &str) -> Option<&RegistryAgent> {
        self.registry.as_ref()?.agents.iter().find(|a| a.id == id)
    }

    /// Install or Update: an agent whose download has no checksum is confirmed first. `request` is the
    /// id the install runs with.
    pub fn install(&mut self, id: &str, request: RequestId, i18n: &mut I18n) -> Vec<RegistryEffect> {
        let Some(agent) = self.agent(id).cloned() else { return Vec::new() };
        if self.busy() {
            return Vec::new();
        }
        if agent.no_checksum {
            let name = Param::from(agent.name.as_str());
            let request = ConfirmRequest {
                title: i18n.tx_built(&["Install ", ""], std::slice::from_ref(&name)),
                message: i18n.tx_built(
                    &[
                        "",
                        " provides no checksum for its download, so the downloaded file cannot be verified. \
                         Install it anyway?",
                    ],
                    &[name],
                ),
                ok: i18n.tx("Install"),
            };
            return vec![RegistryEffect::Confirm { id: id.to_owned(), request }];
        }
        self.start_install(&agent, request, i18n)
    }

    /// The answer of [`RegistryEffect::Confirm`].
    pub fn confirmed(&mut self, id: &str, ok: bool, request: RequestId, i18n: &mut I18n) -> Vec<RegistryEffect> {
        match self.agent(id).cloned() {
            Some(agent) if ok && !self.busy() => self.start_install(&agent, request, i18n),
            _ => Vec::new(),
        }
    }

    fn start_install(&mut self, agent: &RegistryAgent, request: RequestId, i18n: &mut I18n) -> Vec<RegistryEffect> {
        self.errors.remove(&agent.id);
        let message = i18n.tx_built(
            &["Installing ", " ", "…"],
            &[Param::from(agent.name.as_str()), Param::from(agent.version.as_str())],
        );
        self.job = Some(Job { id: agent.id.clone(), request: Some(request), fraction: None, message });
        vec![RegistryEffect::Install { id: agent.id.clone(), request }]
    }

    pub fn install_event(&mut self, request: RequestId, event: InstallEvent) {
        let Some(job) = self.job.as_mut().filter(|j| j.request == Some(request)) else { return };
        match event {
            InstallEvent::Progress { fraction, message } => {
                job.fraction = fraction;
                job.message = message;
            }
            InstallEvent::Line { text } => job.message = text,
        }
    }

    /// The install ended: on success the row shows the version installed and the agent list changed; a
    /// failure (other than a cancellation) shows on its row.
    pub fn installed(&mut self, request: RequestId, result: Result<(), Failure>) -> Vec<RegistryEffect> {
        let Some(job) = self.job.take_if(|j| j.request == Some(request)) else { return Vec::new() };
        match result {
            Ok(()) => {
                if let Some(registry) = &mut self.registry {
                    after_install(&mut registry.agents, &job.id);
                }
                vec![RegistryEffect::Changed]
            }
            Err(Failure::Cancelled) => Vec::new(),
            Err(Failure::Error(e)) => {
                self.errors.insert(job.id, e);
                Vec::new()
            }
        }
    }

    /// Cancel on the row of the install the page started.
    pub fn cancel_install(&self) -> Option<RegistryEffect> {
        self.job.as_ref().and_then(|j| j.request).map(RegistryEffect::Cancel)
    }

    pub fn remove(&mut self, id: &str) -> Vec<RegistryEffect> {
        if self.busy() || self.agent(id).is_none() {
            return Vec::new();
        }
        self.errors.remove(id);
        self.removing = Some(id.to_owned());
        vec![RegistryEffect::Remove { id: id.to_owned() }]
    }

    pub fn removed(&mut self, id: &str, result: Result<(), String>) -> Vec<RegistryEffect> {
        if self.removing.as_deref() != Some(id) {
            return Vec::new();
        }
        self.removing = None;
        match result {
            Ok(()) => {
                if let Some(registry) = &mut self.registry {
                    after_remove(&mut registry.agents, id);
                }
                vec![RegistryEffect::Changed]
            }
            Err(e) => {
                self.errors.insert(id.to_owned(), e);
                Vec::new()
            }
        }
    }

    /// A tick of [`RegistryEffect::PollEvery`]: lists the registry again unless a listing is in flight
    /// or nothing runs in the background any more.
    pub fn poll(&mut self, request: RequestId) -> Option<RegistryEffect> {
        if !self.following_background() || self.poll.is_some() {
            return None;
        }
        self.poll = Some(request);
        Some(RegistryEffect::List(request))
    }

    /// The answer of a poll: once no install runs any more, the agent list changed.
    pub fn polled(
        &mut self,
        request: RequestId,
        answer: Result<AgentRegistry, Failure>,
        i18n: &mut I18n,
    ) -> Vec<RegistryEffect> {
        if self.poll != Some(request) {
            return Vec::new();
        }
        self.poll = None;
        let Ok(registry) = answer else { return Vec::new() };
        let mut effects = self.set_registry(registry, i18n);
        if !self.following_background() {
            effects.retain(|e| !matches!(e, RegistryEffect::PollEvery { .. }));
            effects.push(RegistryEffect::Changed);
        }
        effects
    }

    /// The page closes: its listings are cancelled; an install keeps running in the background.
    pub fn close(&mut self) -> Vec<RegistryEffect> {
        self.request.take().into_iter().chain(self.poll.take()).map(RegistryEffect::Cancel).collect()
    }
}

#[cfg(test)]
mod tests;
