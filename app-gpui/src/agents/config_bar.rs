//! The model, mode, approvals and thinking-effort controls of a composer's bottom row. With both a model
//! and an effort option, one button shows both and opens the model list and an effort slider; with a mode
//! and an on/off permissions option (Allow All), the mode menu carries the switch. Everything but the
//! model (and its effort) is also in the compact Agent settings popup. Names and descriptions are the
//! agent's own; a choice is sent as `(option id, value)`.

use crate::agents::composer::{main_options, mode_icon};
use crate::agents::config_pickers::{PickerEntry, config_picker_options, switch_values};
use crate::platform::i18n::I18n;
use crate::ui::matching::fuzzy_match;
use ide_agent::{AgentConfigOption, AgentConfigValue};
use std::collections::BTreeMap;

/// Chosen values that override an option's `current`, by option id.
pub type Values = BTreeMap<String, String>;

/// A choice to send: the option id and the value.
pub type Choice = (String, String);

/// The value shown for `option`: the chosen one, else the agent's current one.
pub fn value_of<'a>(option: &'a AgentConfigOption, values: &'a Values) -> Option<&'a str> {
    values.get(&option.id).or(option.current.as_ref()).map(String::as_str)
}

fn selected<'a>(option: &'a AgentConfigOption, value: Option<&str>) -> Option<&'a AgentConfigValue> {
    option.values.iter().find(|v| Some(v.value.as_str()) == value)
}

fn name_or_default(option: &AgentConfigOption, value: Option<&str>, i18n: &I18n) -> String {
    selected(option, value).map_or_else(|| i18n.tx("Default"), |v| v.name.clone())
}

/// A control of the row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Picker<'a> {
    /// One button for the model and its effort ([`model_chip`]), opening the model list and the slider.
    ModelEffort { model: &'a AgentConfigOption, effort: &'a AgentConfigOption },
    /// The mode menu with the Allow All switch under it ([`mode_chip`]).
    ModeAllowAll { mode: &'a AgentConfigOption, allow_all: &'a AgentConfigOption },
    /// An on/off permissions option ([`switch_choice`]).
    Switch(&'a AgentConfigOption),
    /// A dropdown of the option's [`config_picker_options`], searchable for the model.
    Select(&'a AgentConfigOption),
}

fn plain(option: &AgentConfigOption) -> Picker<'_> {
    if switch_values(option).is_some() { Picker::Switch(option) } else { Picker::Select(option) }
}

fn category(option: &AgentConfigOption) -> &str {
    option.category.as_deref().unwrap_or_default()
}

/// The controls in the row, and those of the compact Agent settings popup.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfigBar<'a> {
    pub inline: Vec<Picker<'a>>,
    /// Everything but the model and its effort; only [`Picker::Switch`] and [`Picker::Select`].
    pub secondary: Vec<Picker<'a>>,
}

pub fn config_bar(config: &[AgentConfigOption]) -> ConfigBar<'_> {
    let options = main_options(config);
    let model = options.iter().copied().find(|o| category(o) == "model");
    let effort = model.and(options.iter().copied().find(|o| category(o) == "thought_level" && !o.values.is_empty()));
    let mode = options.iter().copied().find(|o| category(o) == "mode");
    let allow_all = mode.and(options.iter().copied().find(|o| switch_values(o).is_some()));
    let is = |o: &AgentConfigOption, other: Option<&AgentConfigOption>| other.is_some_and(|x| std::ptr::eq(o, x));
    let inline = options
        .iter()
        .copied()
        .filter(|o| !is(o, effort) && !is(o, allow_all))
        .map(|o| match (effort, allow_all) {
            (Some(effort), _) if is(o, model) => Picker::ModelEffort { model: o, effort },
            (_, Some(allow_all)) if is(o, mode) => Picker::ModeAllowAll { mode: o, allow_all },
            _ => plain(o),
        })
        .collect();
    let secondary = options.iter().copied().filter(|o| category(o) != "model" && !is(o, effort)).map(plain).collect();
    ConfigBar { inline, secondary }
}

/// The icon of a dropdown entry: approvals lock or unlock, modes show what they let the agent do.
pub fn select_icon(option: &AgentConfigOption, value: &str) -> Option<&'static str> {
    match category(option) {
        "approval" => Some(if value == "bypass" { "unlock" } else { "lock" }),
        "mode" => Some(mode_icon(value)),
        _ => None,
    }
}

/// A switch was flipped.
pub fn switch_choice(option: &AgentConfigOption, checked: bool) -> Option<Choice> {
    let (on, off) = switch_values(option)?;
    Some((option.id.clone(), if checked { on } else { off }.to_owned()))
}

/// The Agent settings button: the icon of the current approval or mode, and a tooltip summing up every
/// secondary setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsButton {
    pub icon: &'static str,
    pub title: String,
}

pub fn settings_button(secondary: &[Picker<'_>], values: &Values, i18n: &I18n) -> SettingsButton {
    let options: Vec<&AgentConfigOption> = secondary
        .iter()
        .filter_map(|p| match p {
            Picker::Switch(o) | Picker::Select(o) => Some(*o),
            _ => None,
        })
        .collect();
    let approval = options.iter().find(|o| matches!(category(o), "approval" | "mode"));
    let icon = approval.map_or("settings", |o| mode_icon(value_of(o, values).unwrap_or_default()));
    let summary: Vec<String> = options
        .iter()
        .map(|o| format!("{}: {}", i18n.tx(&o.name), name_or_default(o, value_of(o, values), i18n)))
        .collect();
    SettingsButton { icon, title: format!("{}: {}", i18n.tx("Agent settings"), summary.join(" · ")) }
}

/// A chip button: its value, the detail after it and its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chip {
    pub value: String,
    pub detail: Option<String>,
    pub title: String,
    pub icon: Option<&'static str>,
}

/// The model button: the model's name and the agent's name for the effort level.
pub fn model_chip(model: &AgentConfigOption, effort: &AgentConfigOption, values: &Values, i18n: &I18n) -> Chip {
    let model_name = name_or_default(model, value_of(model, values), i18n);
    let effort_name = name_or_default(effort, value_of(effort, values), i18n);
    let title = format!("{}: {model_name} · {}: {effort_name}", i18n.tx(&model.name), i18n.tx(&effort.name));
    Chip { value: model_name, detail: Some(effort_name), title, icon: None }
}

/// The mode button: the mode's name, and Allow All after it while it is on.
pub fn mode_chip(mode: &AgentConfigOption, allow_all: &AgentConfigOption, values: &Values, i18n: &I18n) -> Chip {
    let current = selected(mode, value_of(mode, values));
    let on = switch_values(allow_all).is_some_and(|(on, _)| value_of(allow_all, values) == Some(on));
    let name = current.map_or_else(|| i18n.tx("Default"), |v| v.name.clone());
    let state = i18n.tx(if on { "On" } else { "Off" });
    let title = format!("{}: {name} · {}: {state}", i18n.tx(&mode.name), i18n.tx(&allow_all.name));
    let icon = if on { "unlock" } else { mode_icon(current.map_or("", |v| v.value.as_str())) };
    Chip { value: name, detail: on.then(|| i18n.tx(&allow_all.name)), title, icon: Some(icon) }
}

/// The effort slider: a step per level the agent lists, in its order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortSlider {
    pub max: usize,
    pub index: usize,
    /// The level's name, as the slider's value text.
    pub value_text: Option<String>,
}

pub fn effort_slider(effort: &AgentConfigOption, values: &Values) -> EffortSlider {
    let value = value_of(effort, values);
    let index = effort.values.iter().position(|v| Some(v.value.as_str()) == value).unwrap_or(0);
    EffortSlider {
        max: effort.values.len().saturating_sub(1),
        index,
        value_text: effort.values.get(index).map(|v| v.name.clone()),
    }
}

/// The slider moved to step `index`: the level to send, unless it is the current one.
pub fn effort_step(effort: &AgentConfigOption, values: &Values, index: usize) -> Option<Choice> {
    let next = effort.values.get(index)?;
    (Some(next.value.as_str()) != value_of(effort, values)).then(|| (effort.id.clone(), next.value.clone()))
}

/// A key of an option list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Enter,
}

/// The list of a dropdown (or of the model popup): its search, the highlighted entry and the choice.
#[derive(Clone, Debug)]
pub struct OptionList {
    option: String,
    entries: Vec<PickerEntry>,
    searchable: bool,
    query: String,
    active: Option<usize>,
}

impl OptionList {
    /// The list of `option`, highlighting its value. The model's list is searchable.
    pub fn new(option: &AgentConfigOption, values: &Values) -> Self {
        let entries = config_picker_options(option);
        let value = value_of(option, values);
        let active = entries.iter().position(|e| !e.section && Some(e.value.as_str()) == value).unwrap_or(0);
        Self {
            option: option.id.clone(),
            entries,
            searchable: category(option) == "model",
            query: String::new(),
            active: Some(active),
        }
    }

    /// A search: every word must be in an entry's label, detail or keywords (or follow in order), and a
    /// group title stays while one of its entries matches.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_owned();
        self.active = None;
    }

    /// The entries shown.
    pub fn filtered(&self) -> Vec<&PickerEntry> {
        let query = self.query.trim().to_lowercase();
        if !self.searchable || query.is_empty() {
            return self.entries.iter().collect();
        }
        let tokens: Vec<&str> = query.split_whitespace().collect();
        let matches = |e: &PickerEntry| {
            let haystack = format!(
                "{} {} {}",
                e.label,
                e.detail.as_deref().unwrap_or_default(),
                e.keywords.as_deref().unwrap_or_default()
            )
            .to_lowercase();
            tokens.iter().all(|t| fuzzy_match(&haystack, t).is_some())
        };
        fn flush<'a>(
            section: &mut Option<&'a PickerEntry>,
            bucket: &mut Vec<&'a PickerEntry>,
            out: &mut Vec<&'a PickerEntry>,
        ) {
            if !bucket.is_empty() {
                out.extend(section.take());
                out.append(bucket);
            }
            *section = None;
        }
        let mut out = Vec::new();
        let mut section: Option<&PickerEntry> = None;
        let mut bucket: Vec<&PickerEntry> = Vec::new();
        for entry in &self.entries {
            if entry.section {
                flush(&mut section, &mut bucket, &mut out);
                section = Some(entry);
            } else if matches(entry) {
                bucket.push(entry);
            }
        }
        flush(&mut section, &mut bucket, &mut out);
        out
    }

    /// The highlighted entry's index in [`Self::filtered`]: the remembered one, else the first choice.
    pub fn active(&self) -> Option<usize> {
        let filtered = self.filtered();
        self.active
            .filter(|i| filtered.get(*i).is_some_and(|e| !e.section))
            .or_else(|| filtered.iter().position(|e| !e.section))
    }

    /// ↑/↓ move the highlight over the choices, wrapping; Enter chooses it.
    pub fn key(&mut self, key: ListKey) -> Option<Choice> {
        let filtered = self.filtered();
        let active = self.active();
        match key {
            ListKey::Enter => active.and_then(|i| filtered.get(i)).map(|e| (self.option.clone(), e.value.clone())),
            ListKey::Down | ListKey::Up => {
                let len = filtered.len();
                let start = active?;
                let step = |i: usize| if key == ListKey::Down { (i + 1) % len } else { (i + len - 1) % len };
                let mut next = step(start);
                while filtered[next].section && next != start {
                    next = step(next);
                }
                self.active = Some(next);
                None
            }
        }
    }

    /// An entry was clicked: a choice is sent, a group title does nothing.
    pub fn click(&self, value: &str) -> Option<Choice> {
        self.entries.iter().find(|e| !e.section && e.value == value).map(|e| (self.option.clone(), e.value.clone()))
    }
}

#[cfg(test)]
mod tests;
