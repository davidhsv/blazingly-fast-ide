//! Vim mode settings (Settings › Editor › Vim): on/off and the handler of each shortcut conflict.
//! IDE-level, stored as JSON under [`VIM_SETTINGS_KEY`] and picked up by every window.

use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Who gets a conflicting keystroke first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Handler {
    Vim,
    Ide,
}

impl Handler {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vim => "vim",
            Self::Ide => "ide",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "vim" => Some(Self::Vim),
            "ide" => Some(Self::Ide),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VimSettings {
    pub enabled: bool,
    /// Per-keystroke handler overrides (`Ctrl+R` → IDE); missing keys use the default.
    pub handlers: BTreeMap<String, Handler>,
}

pub const VIM_SETTINGS_KEY: &str = "openidea.vim.v1";

/// Settings as stored, validated: unknown values fall back to defaults.
pub fn parse_vim_settings(raw: Option<&str>) -> VimSettings {
    let Some(Value::Object(v)) = raw.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else {
        return VimSettings::default();
    };
    let handlers = v
        .get("handlers")
        .and_then(Value::as_object)
        .map(|handlers| {
            handlers.iter().filter_map(|(k, h)| h.as_str().and_then(Handler::parse).map(|h| (k.clone(), h))).collect()
        })
        .unwrap_or_default();
    VimSettings { enabled: v.get("enabled") == Some(&Value::Bool(true)), handlers }
}

/// The settings as stored.
pub fn vim_settings_json(settings: &VimSettings) -> String {
    let handlers: Map<String, Value> =
        settings.handlers.iter().map(|(k, h)| (k.clone(), Value::String(h.as_str().to_owned()))).collect();
    let mut out = Map::new();
    out.insert("enabled".to_owned(), Value::Bool(settings.enabled));
    out.insert("handlers".to_owned(), Value::Object(handlers));
    Value::Object(out).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stored_settings_defensively() {
        assert_eq!(parse_vim_settings(None), VimSettings::default());
        assert_eq!(parse_vim_settings(Some("{bad")), VimSettings::default());
        assert_eq!(
            parse_vim_settings(Some(r#"{"enabled":true,"handlers":{"Ctrl+R":"ide","Ctrl+V":"nope"}}"#)),
            VimSettings { enabled: true, handlers: BTreeMap::from([("Ctrl+R".to_owned(), Handler::Ide)]) }
        );
    }

    #[test]
    fn stores_what_it_reads_back() {
        let settings = VimSettings { enabled: true, handlers: BTreeMap::from([("Escape".to_owned(), Handler::Vim)]) };
        assert_eq!(parse_vim_settings(Some(&vim_settings_json(&settings))), settings);
    }
}
