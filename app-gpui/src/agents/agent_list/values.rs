//! The values the Coding Agents page reads and writes outside its list: the agents' launch values
//! (executable and environment, staged in the Settings dialog), the New Tasks choices kept in
//! `agents.defaults`, and what Add Custom Agent and Import acp.json read and report.

use crate::agents::builtin_agents::format_env;
use crate::agents::model::REGISTRY_PAGE;
use crate::platform::i18n::{I18n, Param};
use crate::settings_model::staging::SettingsDialog;
use ide_agent::{AcpImportOutcome, AgentConfigOption, AgentProvider};
use regex::Regex;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::LazyLock;

// ----- environment and launch values -----

/// `NAME=value` pairs separated by `;` or new lines. A pair that is refused is named by its variable,
/// or by its place when it has no name: its text may be a pasted key and never shows.
pub fn parse_env(text: &str, i18n: &mut I18n) -> Result<BTreeMap<String, String>, String> {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").expect("the pattern is valid"));
    let mut env = BTreeMap::new();
    for (index, pair) in text.split([';', '\n']).map(str::trim).filter(|p| !p.is_empty()).enumerate() {
        let (key, value) = pair.split_once('=').map_or(("", ""), |(k, v)| (k.trim(), v.trim()));
        if key.is_empty() {
            return Err(i18n.tx_built(&["Expected NAME=value: entry ", ""], &[Param::from(index + 1)]));
        }
        if !NAME.is_match(key) {
            return Err(i18n.tx_built(&["Expected NAME=value: ", ""], &[Param::from(key)]));
        }
        env.insert(key.to_owned(), value.to_owned());
    }
    Ok(env)
}

/// A write of a staged launch value.
#[derive(Clone, PartialEq, Eq)]
pub enum AgentWrite {
    /// `agent_set_command`; `None` finds the executable on `PATH` again.
    SetCommand { agent: String, command: Option<String> },
    /// `agent_set_env`.
    SetEnv { agent: String, env: BTreeMap<String, String> },
}

/// Prints the variables' names only.
impl std::fmt::Debug for AgentWrite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SetCommand { agent, command } => {
                f.debug_struct("SetCommand").field("agent", agent).field("command", command).finish()
            }
            Self::SetEnv { agent, env } => {
                let names: Vec<&str> = env.keys().map(String::as_str).collect();
                f.debug_struct("SetEnv").field("agent", agent).field("env", &names).finish()
            }
        }
    }
}

/// The write of the staged value `id` (`page|key`); `None` when it isn't this page's, `Err` when the
/// environment can't be read.
pub fn agent_write(id: &str, value: &Value, i18n: &mut I18n) -> Option<Result<AgentWrite, String>> {
    let key = id.strip_prefix(REGISTRY_PAGE)?.strip_prefix('|')?.strip_prefix("agents.")?;
    let text = value.as_str().unwrap_or_default();
    if let Some(agent) = key.strip_suffix(".command") {
        let command = Some(text.trim()).filter(|c| !c.is_empty()).map(str::to_owned);
        return Some(Ok(AgentWrite::SetCommand { agent: agent.to_owned(), command }));
    }
    let agent = key.strip_suffix(".env")?;
    Some(parse_env(text, i18n).map(|env| AgentWrite::SetEnv { agent: agent.to_owned(), env }))
}

/// What a written value shows as afterwards: the executable (empty when it is the default program), or
/// the environment as written.
pub fn stored_value(write: &AgentWrite, agents: &[AgentProvider]) -> Value {
    match write {
        AgentWrite::SetCommand { agent, command } => {
            let default = agents.iter().find(|a| a.id == *agent).and_then(|a| a.default_program.as_deref());
            let shown = command.as_deref().filter(|c| Some(*c) != default).unwrap_or_default();
            Value::String(shown.to_owned())
        }
        AgentWrite::SetEnv { env, .. } => Value::String(format_env(env)),
    }
}

/// The value of an agent's staged key: staged, else as written since the dialog opened, else `base`;
/// and the value it is staged against.
pub(super) fn shown(dialog: &SettingsDialog, key: &str, base: String) -> (String, String) {
    let current = dialog.external_applied(REGISTRY_PAGE, key).and_then(Value::as_str).map_or(base, str::to_owned);
    let staged = dialog.staged_external(REGISTRY_PAGE, key).and_then(Value::as_str);
    let value = staged.map_or_else(|| current.clone(), str::to_owned);
    (value, current)
}

pub(super) fn command_base(agent: &AgentProvider) -> String {
    if agent.command_overridden { agent.program.clone() } else { String::new() }
}

// ----- New Tasks -----

/// Whether an option's values and default belong to the selected model rather than to the agent.
pub fn is_per_model_option(option: &AgentConfigOption) -> bool {
    matches!(option.category.as_deref(), Some("thought_level" | "context_tier"))
        || matches!(option.id.as_str(), "context_tier" | "contextTier")
}

/// The options a new task of `agent` starts with, each the agent's own default unless chosen: those
/// with more than one value, then the thinking levels of the model chosen (or current).
pub fn default_options(agent: &AgentProvider, chosen: &BTreeMap<String, String>) -> Vec<AgentConfigOption> {
    let base: Vec<&AgentConfigOption> =
        agent.config.iter().filter(|o| !is_per_model_option(o) && o.values.len() > 1).collect();
    let model_option = base.iter().copied().find(|o| o.category.as_deref() == Some("model"));
    let model = model_option.and_then(|o| chosen.get(&o.id).filter(|v| !v.is_empty()).or(o.current.as_ref()));
    let levels = model.and_then(|model| agent.thought_levels.get(model)).map(Vec::as_slice).unwrap_or_default();
    base.into_iter().chain(levels.iter().filter(|o| o.values.len() > 1)).cloned().collect()
}

/// The choices staged or stored for `agent` in the value of [`super::DEFAULTS_KEY`].
pub fn chosen_defaults(all: Option<&Value>, agent: &str) -> BTreeMap<String, String> {
    let chosen = all.and_then(|all| all.get(agent)).and_then(Value::as_object);
    let pairs = chosen.into_iter().flatten().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned())));
    pairs.collect()
}

/// The value of [`super::DEFAULTS_KEY`] with `option` of `agent` set to `value` (empty: the agent's default);
/// an agent left without choices is dropped.
pub fn defaults_with(all: Option<&Value>, agent: &str, option: &str, value: &str) -> Value {
    let mut all: Map<String, Value> = all.and_then(Value::as_object).cloned().unwrap_or_default();
    let mut chosen: Map<String, Value> = all.get(agent).and_then(Value::as_object).cloned().unwrap_or_default();
    chosen.remove(option);
    if !value.is_empty() {
        chosen.insert(option.to_owned(), Value::from(value));
    }
    all.remove(agent);
    if !chosen.is_empty() {
        all.insert(agent.to_owned(), Value::Object(chosen));
    }
    Value::Object(all)
}

/// One entry of a New Tasks choice; `value` is empty for the agent's own default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTaskChoice {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
}

/// A New Tasks choice: the option's name, what it shows now and its entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewTaskOption {
    pub id: String,
    pub name: String,
    /// The value chosen, empty for the agent's default.
    pub value: String,
    pub shown: String,
    pub choices: Vec<NewTaskChoice>,
}

/// The New Tasks section of `agent`, over the value of [`super::DEFAULTS_KEY`] as the page shows it. Empty
/// before the agent's first session, which is when it reports its options.
pub fn new_task_options(agent: &AgentProvider, all: Option<&Value>, i18n: &mut I18n) -> Vec<NewTaskOption> {
    let chosen = chosen_defaults(all, &agent.id);
    default_options(agent, &chosen)
        .into_iter()
        .map(|option| {
            let fallback = option.values.iter().find(|v| Some(&v.value) == option.current.as_ref());
            let first = match fallback {
                Some(value) => i18n.tx_built(&["Agent's default (", ")"], &[Param::from(value.name.as_str())]),
                None => i18n.tx("Agent's default"),
            };
            let mut choices = vec![NewTaskChoice { value: String::new(), label: first, detail: None }];
            choices.extend(option.values.iter().map(|v| NewTaskChoice {
                value: v.value.clone(),
                label: v.name.clone(),
                detail: v.description.clone(),
            }));
            let value = chosen.get(&option.id).cloned().unwrap_or_default();
            let shown = choices.iter().find(|c| c.value == value).map_or_else(|| value.clone(), |c| c.label.clone());
            NewTaskOption { id: option.id.clone(), name: option.name.clone(), value, shown, choices }
        })
        .collect()
}

// ----- custom agents -----

/// Suggests an agent-file id for a display name (`"My Agent!"` → `"my-agent"`).
pub fn slugify_id(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(ch);
        } else {
            dash = true;
        }
    }
    out
}

/// Splits arguments on whitespace, honouring single and double quotes. They are handed to the agent
/// as they are: nothing here goes through a shell.
pub fn parse_args(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for ch in text.trim().chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            None => current.push(ch),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// What Import acp.json reports: the agents saved, then those skipped with why.
pub fn import_report(outcomes: &[AcpImportOutcome], i18n: &mut I18n) -> String {
    let saved: Vec<&str> = outcomes.iter().filter(|o| o.saved).map(|o| o.name.as_str()).collect();
    let mut parts = vec![if saved.is_empty() {
        i18n.tx("Imported nothing from acp.json.")
    } else {
        i18n.tx_built(&["Imported ", " from acp.json."], &[Param::from(saved.join(", "))])
    }];
    let skipped: Vec<String> =
        outcomes.iter().filter(|o| !o.saved).map(|o| format!("{} ({})", o.name, o.reason)).collect();
    if !skipped.is_empty() {
        parts.push(i18n.tx_built(&["Skipped ", "."], &[Param::from(skipped.join(", "))]));
    }
    parts.join(" ")
}
