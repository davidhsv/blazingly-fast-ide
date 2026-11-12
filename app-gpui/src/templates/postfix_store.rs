//! The postfix completion settings, stored for all projects under [`POSTFIX_KEY`].

use super::storage::{StorageWrite, bool_field, load_object, string_list};
use serde_json::{Map, json};

/// Storage key of the settings.
pub const POSTFIX_KEY: &str = "openidea.postfix.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixSettings {
    pub enabled: bool,
    /// Expand `expr.key` with Tab (IDEA's default "Expand templates with Tab").
    pub expand_with_tab: bool,
    /// Offer postfix templates in the completion list.
    pub show_in_completion: bool,
    /// Disabled template ids.
    pub disabled: Vec<String>,
}

impl Default for PostfixSettings {
    fn default() -> Self {
        Self { enabled: true, expand_with_tab: true, show_in_completion: true, disabled: Vec::new() }
    }
}

impl PostfixSettings {
    /// The stored settings; what is missing or unreadable is at its default.
    pub fn parse(raw: Option<&str>) -> Self {
        let stored = load_object(raw, Map::new());
        Self {
            enabled: bool_field(&stored, "enabled", true),
            expand_with_tab: bool_field(&stored, "expandWithTab", true),
            show_in_completion: bool_field(&stored, "showInCompletion", true),
            disabled: string_list(&stored, "disabled"),
        }
    }

    pub fn write(&self) -> StorageWrite {
        let value = json!({
            "enabled": self.enabled,
            "expandWithTab": self.expand_with_tab,
            "showInCompletion": self.show_in_completion,
            "disabled": self.disabled,
        });
        StorageWrite::json(POSTFIX_KEY, &value)
    }

    /// Turns one template on or off; returns what to store.
    pub fn set_template_enabled(&mut self, id: &str, on: bool) -> StorageWrite {
        self.disabled.retain(|disabled| disabled != id);
        if !on {
            self.disabled.push(id.to_owned());
        }
        self.write()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: everything is on by default, stored values are kept, and disabling a
    // template stores its id once.
    #[test]
    fn settings_default_to_on_and_round_trip() {
        assert_eq!(PostfixSettings::parse(None), PostfixSettings::default());
        assert_eq!(PostfixSettings::parse(Some("garbage")), PostfixSettings::default());
        let mut settings = PostfixSettings::parse(Some(r#"{"expandWithTab":false,"disabled":["ts.if",3]}"#));
        assert!(settings.enabled && !settings.expand_with_tab && settings.show_in_completion);
        assert_eq!(settings.disabled, ["ts.if"]);
        assert_eq!(settings.set_template_enabled("rust.not", false).key, POSTFIX_KEY);
        let write = settings.set_template_enabled("rust.not", false);
        assert_eq!(settings.disabled, ["ts.if", "rust.not"]);
        assert_eq!(PostfixSettings::parse(Some(&write.value)), settings);
        let write = settings.set_template_enabled("ts.if", true);
        assert!(write.value.contains(r#""disabled":["rust.not"]"#), "{}", write.value);
    }
}
