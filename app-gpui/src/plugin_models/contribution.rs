//! A UI contribution of a live plugin as the models read it: the extension point, the key the
//! plugin gave it and its validated declaration.

use ide_host::plugins::PluginsSnapshot;
use serde_json::Value;

/// One declaration of a live plugin for an extension point (`commands`, `views`, `iconThemes`, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Contribution {
    pub plugin_id: String,
    pub point: String,
    /// Stable within (plugin, point).
    pub key: String,
    pub decl: Value,
}

impl Contribution {
    pub fn new(plugin_id: &str, point: &str, key: &str, decl: Value) -> Self {
        Self { plugin_id: plugin_id.to_owned(), point: point.to_owned(), key: key.to_owned(), decl }
    }

    /// The contributions a listing carries, in its order.
    pub fn from_snapshot(snapshot: &PluginsSnapshot) -> Vec<Self> {
        snapshot
            .ui
            .iter()
            .map(|item| Self {
                plugin_id: item.plugin_id.clone(),
                point: item.point.clone(),
                key: item.key.clone(),
                decl: item.decl.clone(),
            })
            .collect()
    }

    /// The string field `name` of the declaration.
    pub fn text(&self, name: &str) -> Option<&str> {
        self.decl.get(name).and_then(Value::as_str)
    }

    /// The declaration's `when` clause.
    pub fn when(&self) -> Option<&str> {
        self.text("when")
    }
}

/// The number of UTF-16 code units of `text`.
pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_listing_becomes_contributions_in_its_order() {
        let snapshot: PluginsSnapshot = serde_json::from_value(json!({
            "plugins": [],
            "activeTheme": null,
            "ui": [
                { "pluginId": "a", "point": "commands", "key": "x", "decl": { "id": "x", "title": "X" } },
                { "pluginId": "b", "point": "menus", "key": "m", "decl": { "when": "editorFocus" } },
            ],
        }))
        .expect("a listing");
        let contributions = Contribution::from_snapshot(&snapshot);
        assert_eq!(contributions.len(), 2);
        assert_eq!(contributions[0].text("title"), Some("X"));
        assert_eq!(contributions[1].when(), Some("editorFocus"));
        assert_eq!(utf16_len("😀a"), 3);
    }
}
