//! Settings › Tools › Agents › Built-in: the built-in agents, each with its executable (empty: found on
//! `PATH`) and the environment variables it starts with. Edits are staged in the Settings dialog as values
//! kept outside the settings ([`SettingsDialog::stage_external`] on [`REGISTRY_PAGE`]), each agent's
//! independently, and written only on Apply / OK: the caller hands each [`ApplyRun::next_external`] to
//! [`external_write`], performs it, lists the agents again ([`BuiltinAgents::loaded`]) and reports the
//! value as stored ([`stored_value`]) with `ApplyRun::external_done`.
//!
//! [`ApplyRun::next_external`]: crate::settings_model::staging::ApplyRun::next_external

use crate::agents::model::REGISTRY_PAGE;
use crate::platform::i18n::{I18n, Param};
use crate::settings_model::staging::SettingsDialog;
use ide_agent::{AgentCommandReadOnlyReason, AgentProvider};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The staged key of an agent's executable.
pub fn command_key(agent: &str) -> String {
    format!("agents.{agent}.command")
}

/// The staged key of an agent's environment variables.
pub fn env_key(agent: &str) -> String {
    format!("agents.{agent}.env")
}

/// The page's tabs.
pub fn page_tabs(i18n: &I18n) -> [String; 2] {
    [i18n.tx("Built-in"), i18n.tx("ACP Registry")]
}

/// `NAME=value` pairs separated by `;` or new lines; a pair without a valid name is refused.
pub fn parse_env(text: &str, i18n: &mut I18n) -> Result<BTreeMap<String, String>, String> {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("the pattern is valid"));
    let mut env = BTreeMap::new();
    for pair in text.split([';', '\n']).map(str::trim).filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=').map_or(("", ""), |(k, v)| (k.trim(), v.trim()));
        if !NAME.is_match(key) {
            return Err(i18n.tx_built(&["Expected NAME=value: ", ""], &[Param::from(pair)]));
        }
        env.insert(key.to_owned(), value.to_owned());
    }
    Ok(env)
}

/// The environment as the field shows it: `NAME=value; OTHER=value`.
pub fn format_env(env: &BTreeMap<String, String>) -> String {
    env.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
}

/// A piece of a login hint: plain text, or a command written between backticks (shown as code).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HintPart {
    Text(String),
    Code(String),
}

/// The agent's login hint, translated, with its commands as code.
pub fn login_hint(text: &str, i18n: &I18n) -> Vec<HintPart> {
    static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`]+`").expect("the pattern is valid"));
    let text = i18n.tx_template(text);
    let mut out = Vec::new();
    let mut last = 0;
    for m in CODE.find_iter(&text) {
        if m.start() > last {
            out.push(HintPart::Text(text[last..m.start()].to_owned()));
        }
        out.push(HintPart::Code(text[m.start() + 1..m.end() - 1].to_owned()));
        last = m.end();
    }
    if last < text.len() {
        out.push(HintPart::Text(text[last..].to_owned()));
    }
    out
}

/// A write of a staged value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalWrite {
    /// `agent_set_command`; `None` finds the executable on `PATH` again.
    SetCommand { agent: String, command: Option<String> },
    /// `agent_set_env`.
    SetEnv { agent: String, env: BTreeMap<String, String> },
}

/// The write of the staged value `id` (`page|key`), `None` when it isn't this page's; `Err` when the
/// environment can't be read.
pub fn external_write(id: &str, value: &Value, i18n: &mut I18n) -> Option<Result<ExternalWrite, String>> {
    let key = id.strip_prefix(REGISTRY_PAGE)?.strip_prefix('|')?.strip_prefix("agents.")?;
    let text = value.as_str().unwrap_or_default();
    if let Some(agent) = key.strip_suffix(".command") {
        let command = Some(text.trim()).filter(|c| !c.is_empty()).map(str::to_owned);
        return Some(Ok(ExternalWrite::SetCommand { agent: agent.to_owned(), command }));
    }
    let agent = key.strip_suffix(".env")?;
    Some(parse_env(text, i18n).map(|env| ExternalWrite::SetEnv { agent: agent.to_owned(), env }))
}

/// What a written value shows as afterwards: the executable (empty when it is the default program), or
/// the environment as it was read.
pub fn stored_value(write: &ExternalWrite, agents: &[AgentProvider]) -> Value {
    match write {
        ExternalWrite::SetCommand { agent, command } => {
            let default = agents.iter().find(|a| a.id == *agent).and_then(|a| a.default_program.as_deref());
            let shown = command.as_deref().filter(|c| Some(*c) != default).unwrap_or_default();
            Value::String(shown.to_owned())
        }
        ExternalWrite::SetEnv { env, .. } => Value::String(format_env(env)),
    }
}

/// A row of the agent list: the name and its tag ("Custom" or "Not found").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentRow {
    pub id: String,
    pub name: String,
    pub tag: Option<String>,
    /// Not found and not overridden: muted.
    pub missing: bool,
}

/// What the page shows of the selected agent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    /// The section's name: "Claude Code executable".
    pub title: String,
    /// "Custom executable", "Found on PATH" or "Not found on PATH".
    pub status: String,
    /// The default command line.
    pub launch: String,
    /// The executable field (`None`: read-only, with `note` saying why).
    pub executable: Option<String>,
    pub placeholder: String,
    /// The Reset button shows while the field has a value.
    pub reset: bool,
    pub note: Option<String>,
    /// The environment field, for an agent launched by its own executable.
    pub env: Option<String>,
    pub login: Vec<HintPart>,
}

/// The value of an agent's staged key: staged, else as written since the dialog opened, else `base`.
fn shown(dialog: &SettingsDialog, key: &str, base: String) -> (String, String) {
    let current = dialog.external_applied(REGISTRY_PAGE, key).and_then(Value::as_str).map_or(base, str::to_owned);
    let staged = dialog.staged_external(REGISTRY_PAGE, key).and_then(Value::as_str);
    let value = staged.map_or_else(|| current.clone(), str::to_owned);
    (value, current)
}

fn command_base(agent: &AgentProvider) -> String {
    if agent.command_overridden { agent.program.clone() } else { String::new() }
}

/// The page's state: the agents (those with a default executable), which is selected, and why they
/// could not be read.
#[derive(Clone, Debug, Default)]
pub struct BuiltinAgents {
    agents: Option<Vec<AgentProvider>>,
    error: Option<String>,
    selected: usize,
}

/// Something the page asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuiltinEffect {
    /// Ask for an executable, then call [`BuiltinAgents::chosen`].
    ChooseExecutable { title: String, current: String },
}

impl BuiltinAgents {
    /// The agents were listed (again, after a write); a failure (no trusted project) is shown instead.
    pub fn loaded(&mut self, agents: Result<Vec<AgentProvider>, String>) {
        match agents {
            Ok(agents) => {
                self.agents = Some(agents.into_iter().filter(|a| a.default_program.is_some()).collect());
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn agents(&self) -> &[AgentProvider] {
        self.agents.as_deref().unwrap_or_default()
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index;
    }

    /// The selected agent (the first when the selection is gone).
    pub fn selected(&self) -> Option<&AgentProvider> {
        let agents = self.agents();
        agents.get(self.selected).or(agents.first())
    }

    pub fn rows(&self, i18n: &I18n) -> Vec<AgentRow> {
        self.agents()
            .iter()
            .map(|a| AgentRow {
                id: a.id.clone(),
                name: a.name.clone(),
                tag: if a.command_overridden {
                    Some(i18n.tx("Custom"))
                } else {
                    (!a.installed).then(|| i18n.tx("Not found"))
                },
                missing: !a.installed && !a.command_overridden,
            })
            .collect()
    }

    pub fn details(&self, dialog: &SettingsDialog, i18n: &mut I18n) -> Option<Details> {
        if self.error.is_some() {
            return None;
        }
        let agent = self.selected()?;
        let default = agent.default_program.clone().unwrap_or_default();
        let status = if agent.command_overridden {
            "Custom executable"
        } else if agent.installed {
            "Found on PATH"
        } else {
            "Not found on PATH"
        };
        let executable = agent.command_editable.then(|| shown(dialog, &command_key(&agent.id), command_base(agent)).0);
        let note = (!agent.command_editable).then(|| {
            i18n.tx(match agent.command_read_only_reason {
                Some(AgentCommandReadOnlyReason::PackageRunner) => {
                    "This agent runs through a package runner (npm or uv) and has no executable to choose."
                }
                Some(AgentCommandReadOnlyReason::Registry) => concat!(
                    "This agent is installed from the ACP Registry. ",
                    "Manage its installation in the ACP Registry tab."
                ),
                _ => "This agent is configured in its agent file.",
            })
        });
        let own_env = !matches!(
            agent.command_read_only_reason,
            Some(AgentCommandReadOnlyReason::Registry | AgentCommandReadOnlyReason::CustomAgent)
        );
        let env = own_env.then(|| shown(dialog, &env_key(&agent.id), format_env(&agent.env)).0);
        Some(Details {
            title: i18n.tx_built(&["", " executable"], &[Param::from(agent.name.as_str())]),
            status: i18n.tx(status),
            launch: std::iter::once(&default).chain(&agent.default_args).cloned().collect::<Vec<_>>().join(" "),
            reset: executable.as_deref().is_some_and(|v| !v.is_empty()),
            executable,
            placeholder: i18n.tx_built(&["Auto-detect (", ")"], &[Param::from(default.as_str())]),
            note,
            env,
            login: agent.login_hint.as_deref().map(|h| login_hint(h, i18n)).unwrap_or_default(),
        })
    }

    /// The executable field changed (or was cleared, or reset); a read-only agent has none.
    pub fn set_command(&self, dialog: &mut SettingsDialog, text: &str) {
        let Some(agent) = self.selected().filter(|a| a.command_editable) else { return };
        let key = command_key(&agent.id);
        let (_, current) = shown(dialog, &key, command_base(agent));
        dialog.stage_external(REGISTRY_PAGE, &key, Value::String(text.to_owned()), &Value::String(current));
    }

    /// The environment field changed.
    pub fn set_env(&self, dialog: &mut SettingsDialog, text: &str) {
        let Some(agent) = self.selected() else { return };
        let key = env_key(&agent.id);
        let (_, current) = shown(dialog, &key, format_env(&agent.env));
        dialog.stage_external(REGISTRY_PAGE, &key, Value::String(text.to_owned()), &Value::String(current));
    }

    /// Browse…: asks for an executable, starting from the field's value. A read-only agent has no button.
    pub fn browse(&self, dialog: &SettingsDialog, i18n: &mut I18n) -> Option<BuiltinEffect> {
        let agent = self.selected().filter(|a| a.command_editable)?;
        let (current, _) = shown(dialog, &command_key(&agent.id), command_base(agent));
        let title = i18n.tx_built(&["Select ", ""], &[Param::from(agent.name.as_str())]);
        Some(BuiltinEffect::ChooseExecutable { title, current })
    }

    /// The executable chosen with Browse… is staged; a cancelled dialog changes nothing.
    pub fn chosen(&self, dialog: &mut SettingsDialog, path: Option<&str>) {
        if let Some(path) = path {
            self.set_command(dialog, path);
        }
    }

    /// Whether the agent's edits wait for Apply.
    pub fn modified(dialog: &SettingsDialog, agent: &str) -> bool {
        [command_key(agent), env_key(agent)].iter().any(|key| dialog.staged_external(REGISTRY_PAGE, key).is_some())
    }
}

#[cfg(test)]
mod tests;
