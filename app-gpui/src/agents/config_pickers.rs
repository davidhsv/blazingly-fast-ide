//! The entries of a session setting's picker (model, mode, permissions…). Models named by a path
//! (`provider/family/model`, as routers list them) are grouped under their provider.

use ide_agent::AgentConfigOption;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickerEntry {
    pub value: String,
    pub label: String,
    pub detail: Option<String>,
    /// Extra text the filter matches (a grouped model's provider); `None` outside the model picker.
    pub keywords: Option<String>,
    /// A group title, not a choice.
    pub section: bool,
}

/// The picker entries of `option`: plain model ids first, then each provider's group in the order the
/// providers first appear, keeping the order inside each group. Other settings list their values as
/// they are.
pub fn config_picker_options(option: &AgentConfigOption) -> Vec<PickerEntry> {
    if option.category.as_deref() != Some("model") {
        return option
            .values
            .iter()
            .map(|v| PickerEntry {
                value: v.value.clone(),
                label: v.name.clone(),
                detail: v.description.clone(),
                keywords: None,
                section: false,
            })
            .collect();
    }
    let mut plain = Vec::new();
    let mut groups: Vec<(String, Vec<PickerEntry>)> = Vec::new();
    for v in &option.values {
        let (provider, path) = v.value.split_once('/').unwrap_or(("", ""));
        let label = if path.is_empty() {
            if v.name.is_empty() { v.value.clone() } else { v.name.clone() }
        } else {
            path.rsplit('/').next().unwrap_or(path).to_owned()
        };
        let detail = if path.is_empty() {
            v.description.clone()
        } else {
            let parts: Vec<&str> =
                [Some(path), v.description.as_deref()].into_iter().flatten().filter(|p| !p.is_empty()).collect();
            Some(parts.join(" · "))
        };
        let entry =
            PickerEntry { value: v.value.clone(), label, detail, keywords: Some(provider.to_owned()), section: false };
        if provider.is_empty() {
            plain.push(entry);
        } else if let Some((_, items)) = groups.iter_mut().find(|(p, _)| p == provider) {
            items.push(entry);
        } else {
            groups.push((provider.to_owned(), vec![entry]));
        }
    }
    plain
        .into_iter()
        .chain(groups.into_iter().flat_map(|(provider, items)| {
            let title =
                PickerEntry { value: provider.clone(), label: provider, detail: None, keywords: None, section: true };
            std::iter::once(title).chain(items)
        }))
        .collect()
}

/// The on and off values of a two-state permissions setting (shown as a switch), or `None` when it has
/// other values.
pub fn switch_values(option: &AgentConfigOption) -> Option<(&str, &str)> {
    if option.category.as_deref() != Some("permissions") {
        return None;
    }
    let find = |words: [&str; 2]| {
        option.values.iter().find(|v| words.iter().any(|w| v.value.eq_ignore_ascii_case(w))).map(|v| v.value.as_str())
    };
    Some((find(["on", "true"])?, find(["off", "false"])?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_agent::{AgentConfigValue, AgentConfigVia};

    fn config(values: Vec<AgentConfigValue>, category: &str) -> AgentConfigOption {
        AgentConfigOption {
            id: "model".into(),
            name: "Model".into(),
            description: None,
            category: Some(category.into()),
            current: None,
            values,
            via: AgentConfigVia::Option,
        }
    }

    fn value(id: &str, name: Option<&str>, description: Option<&str>) -> AgentConfigValue {
        AgentConfigValue {
            value: id.into(),
            name: name.unwrap_or(id).into(),
            description: description.map(str::to_owned),
        }
    }

    fn entry(value: &str, label: &str, detail: Option<&str>, keywords: Option<&str>) -> PickerEntry {
        PickerEntry {
            value: value.into(),
            label: label.into(),
            detail: detail.map(str::to_owned),
            keywords: keywords.map(str::to_owned),
            section: false,
        }
    }

    fn section(name: &str) -> PickerEntry {
        PickerEntry { section: true, ..entry(name, name, None, None) }
    }

    #[test]
    fn groups_nested_paths_by_their_first_provider_and_preserves_the_full_value_and_description() {
        let option = config(vec![value("9router/cmc/Beam", Some("Beam display"), Some("Fast"))], "model");
        assert_eq!(
            config_picker_options(&option),
            vec![section("9router"), entry("9router/cmc/Beam", "Beam", Some("cmc/Beam · Fast"), Some("9router"))]
        );
    }

    #[test]
    fn keeps_plain_ids_ungrouped_even_when_the_display_name_contains_a_slash() {
        let option = config(vec![value("plain", Some("Local / Beam"), Some("Offline"))], "model");
        assert_eq!(config_picker_options(&option), vec![entry("plain", "Local / Beam", Some("Offline"), Some(""))]);
    }

    #[test]
    fn collects_several_providers_in_encounter_order_and_preserves_order_inside_each_group() {
        let values = ["a/One", "b/Two", "a/Three", "plain", "__section__:a"].map(|v| value(v, None, None));
        let options = config_picker_options(&config(values.to_vec(), "model"));
        let shape: Vec<(&str, bool)> = options.iter().map(|o| (o.value.as_str(), o.section)).collect();
        assert_eq!(
            shape,
            vec![
                ("plain", false),
                ("__section__:a", false),
                ("a", true),
                ("a/One", false),
                ("a/Three", false),
                ("b", true),
                ("b/Two", false),
            ]
        );
        assert_eq!(options[3].detail.as_deref(), Some("One"));
    }

    #[test]
    fn leaves_non_model_option_labels_and_descriptions_intact() {
        let option = config(vec![value("a/One", Some("Custom"), Some("Description"))], "mode");
        assert_eq!(config_picker_options(&option), vec![entry("a/One", "Custom", Some("Description"), None)]);
    }

    #[test]
    fn shows_allow_all_as_a_switch_and_sends_on_off_values() {
        let mut option = config(vec![value("off", None, None), value("on", None, None)], "permissions");
        assert_eq!(switch_values(&option), Some(("on", "off")));
        option.values.push(value("ask", None, None));
        assert_eq!(switch_values(&option), Some(("on", "off")));
        option.values.retain(|v| v.value != "off");
        assert_eq!(switch_values(&option), None);
    }
}
