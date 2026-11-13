//! What the consent dialog shows of a plugin's capabilities: each with its risk and a description
//! whose parameter (a host, a socket) stays out of the translated text, ordered from the highest risk,
//! and which optional ones start accepted.

use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use ide_host::plugins::PluginInfo;
use serde::Deserialize;
use std::cmp::Ordering;

/// How a grant is presented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Risk {
    Low,
    Medium,
    High,
}

/// A declared capability with its risk and grant state.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    /// `project.read`, `net:crates.io`, …
    pub id: String,
    pub risk: Risk,
    pub description: String,
    pub optional: bool,
    pub granted: bool,
}

/// The capabilities of a listed plugin.
pub fn capabilities_of(plugin: &PluginInfo) -> Vec<Capability> {
    serde_json::to_value(&plugin.capabilities).ok().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

/// The capabilities from the highest risk to the lowest, by id within a risk.
pub fn sort_by_risk(capabilities: &[Capability]) -> Vec<Capability> {
    let mut sorted = capabilities.to_vec();
    sorted.sort_by(|a, b| match b.risk.cmp(&a.risk) {
        Ordering::Equal => collate::compare(&a.id, &b.id),
        other => other,
    });
    sorted
}

/// The optional capabilities the dialog starts with accepted: those granted before and the low-risk ones.
pub fn initially_accepted(capabilities: &[Capability]) -> Vec<String> {
    capabilities.iter().filter(|c| c.optional && (c.granted || c.risk == Risk::Low)).map(|c| c.id.clone()).collect()
}

/// Whether the dialog warns that it is like running arbitrary code: a required high-risk capability,
/// or an optional one that is accepted.
pub fn is_high_risk(capabilities: &[Capability], accepted: &[String]) -> bool {
    capabilities.iter().any(|c| c.risk == Risk::High && (!c.optional || accepted.contains(&c.id)))
}

/// What the capability lets the plugin do, translated independently of the host or path it names.
pub fn capability_description(capability: &Capability, i18n: &mut I18n) -> String {
    let id = capability.id.as_str();
    if id == "database.sources" {
        return i18n.tx("Manage its own database sources and store their passwords in trusted projects");
    }
    if id == "ide.commands" {
        return i18n.tx(concat!(
            "Run any IDE action or other plugins' commands as if you chose them, ",
            "including running programs, committing and pushing"
        ));
    }
    if let Some(host) = id.strip_prefix("net.http:") {
        return i18n.tx_built(
            &["Connect to ", " over unencrypted HTTP (from the machine the project is on)"],
            &[Param::Text(host.to_owned())],
        );
    }
    if let Some(path) = id.strip_prefix("ipc:") {
        return i18n.tx_built(
            &["Connect to local socket ", " — equivalent to running arbitrary code on the project machine"],
            &[Param::Text(path.to_owned())],
        );
    }
    i18n.tx(&capability.description)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(id: &str, risk: Risk, optional: bool, granted: bool) -> Capability {
        Capability { id: id.to_owned(), risk, description: String::new(), optional, granted }
    }

    #[test]
    fn describes_ide_commands_explicitly_in_consent() {
        let commands = capability("ide.commands", Risk::High, false, false);
        let description = capability_description(&commands, &mut I18n::default());
        assert!(description.contains("including running programs, committing and pushing"));
    }

    #[test]
    fn keeps_the_host_out_of_the_translated_text_and_orders_by_risk() {
        let mut i18n = I18n::default();
        let http = capability("net.http:example.com", Risk::Medium, true, false);
        let text = capability_description(&http, &mut i18n);
        assert_eq!(text, "Connect to example.com over unencrypted HTTP (from the machine the project is on)");
        let all = [
            capability("process:bash", Risk::High, false, false),
            http,
            capability("project.read", Risk::Low, true, false),
            capability("auth:github", Risk::Medium, true, true),
        ];
        let ids: Vec<String> = sort_by_risk(&all).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["process:bash", "auth:github", "net.http:example.com", "project.read"]);
        let accepted = initially_accepted(&all);
        assert_eq!(accepted, ["project.read", "auth:github"]);
        assert!(is_high_risk(&all, &accepted));
        use crate::plugin_models::test_support::{capability as listed_capability, plugin};
        let listed = plugin("p", serde_json::json!({ "capabilities": [listed_capability("process:bash", true)] }));
        assert_eq!(capabilities_of(&listed), vec![capability("process:bash", Risk::High, false, true)]);
    }

    // src/features/plugins/consent.tsx › sortByRisk: high risk first, then by id within a risk.
    #[test]
    fn lists_high_risk_capabilities_first_and_by_id_within_a_risk() {
        let all = [
            capability("storage", Risk::Low, false, false),
            capability("process:zsh", Risk::High, false, false),
            capability("clipboard", Risk::Medium, true, false),
            capability("ide.commands", Risk::High, false, false),
            capability("events", Risk::Low, false, false),
        ];
        let ids: Vec<String> = sort_by_risk(&all).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["ide.commands", "process:zsh", "clipboard", "events", "storage"]);
    }
}
