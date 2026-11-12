//! The live templates in effect: the built-in ones with the user's edits applied, the user's own
//! templates and the snippets plugins contribute. The user's part is stored for all projects under
//! [`LIVE_TEMPLATES_KEY`], the options of the settings page under [`LIVE_TEMPLATE_OPTIONS_KEY`].

use super::live_builtins::{builtin_templates, is_builtin_id};
use super::live_model::LiveTemplate;
use super::live_snippets::{ContributedSnippet, PLUGIN_SNIPPET_PREFIX, snippet_templates};
use super::storage::{StorageWrite, bool_field, load_object};
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, json};
use std::fmt;

/// Storage key of the user's edits.
pub const LIVE_TEMPLATES_KEY: &str = "openidea.liveTemplates.v1";
/// Storage key of the options.
pub const LIVE_TEMPLATE_OPTIONS_KEY: &str = "openidea.liveTemplates.options.v1";

/// Templates by id, in the order they were added: stored as one JSON object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplatesById(Vec<LiveTemplate>);

impl TemplatesById {
    pub fn get(&self, id: &str) -> Option<&LiveTemplate> {
        self.0.iter().find(|t| t.id == id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.get(id).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = &LiveTemplate> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Puts `template` under its id; a template already there keeps its place.
    pub fn insert(&mut self, template: LiveTemplate) {
        match self.0.iter_mut().find(|t| t.id == template.id) {
            Some(slot) => *slot = template,
            None => self.0.push(template),
        }
    }

    pub fn remove(&mut self, id: &str) {
        self.0.retain(|t| t.id != id);
    }
}

impl FromIterator<LiveTemplate> for TemplatesById {
    fn from_iter<I: IntoIterator<Item = LiveTemplate>>(templates: I) -> Self {
        let mut out = Self::default();
        for template in templates {
            out.insert(template);
        }
        out
    }
}

impl Serialize for TemplatesById {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for template in &self.0 {
            map.serialize_entry(&template.id, template)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TemplatesById {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Entries;

        impl<'de> Visitor<'de> for Entries {
            type Value = TemplatesById;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of live templates by id")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = TemplatesById::default();
                while let Some((id, mut template)) = map.next_entry::<String, LiveTemplate>()? {
                    // The key is the id in effect, whatever the entry says.
                    template.id = id;
                    out.insert(template);
                }
                Ok(out)
            }
        }

        deserializer.deserialize_map(Entries)
    }
}

/// What the user changed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LiveTemplateCustomizations {
    /// Edited built-ins and user templates, by id.
    pub templates: TemplatesById,
    /// Removed built-ins.
    pub deleted: Vec<String>,
}

impl LiveTemplateCustomizations {
    /// The stored customizations; none when nothing readable is stored.
    pub fn parse(raw: Option<&str>) -> Self {
        raw.filter(|raw| !raw.is_empty()).and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_default()
    }

    pub fn write(&self) -> StorageWrite {
        StorageWrite::json(LIVE_TEMPLATES_KEY, self)
    }
}

/// `builtins` without the deleted ones and with the user's edits, then the user's templates, then
/// the templates of the contributed `snippets`.
pub fn merge_templates(
    builtins: &[LiveTemplate],
    custom: &LiveTemplateCustomizations,
    snippets: &[ContributedSnippet],
) -> Vec<LiveTemplate> {
    let deleted = |id: &str| custom.deleted.iter().any(|d| d == id);
    let own = |t: &&LiveTemplate| !t.id.starts_with(PLUGIN_SNIPPET_PREFIX);
    let edited = builtins.iter().filter(|t| !deleted(&t.id)).map(|t| custom.templates.get(&t.id).unwrap_or(t));
    let added = custom.templates.iter().filter(|t| !is_builtin_id(&t.id) && !deleted(&t.id));
    edited.chain(added).filter(own).cloned().chain(snippets.iter().flat_map(snippet_templates)).collect()
}

/// The templates in effect and what they are made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTemplates {
    custom: LiveTemplateCustomizations,
    snippets: Vec<ContributedSnippet>,
    templates: Vec<LiveTemplate>,
}

impl Default for LiveTemplates {
    fn default() -> Self {
        Self::load(None)
    }
}

impl LiveTemplates {
    /// The templates with the customizations stored as `raw`.
    pub fn load(raw: Option<&str>) -> Self {
        let custom = LiveTemplateCustomizations::parse(raw);
        let templates = merge_templates(builtin_templates(), &custom, &[]);
        Self { custom, snippets: Vec::new(), templates }
    }

    /// Re-reads the customizations (another window changed them); contributed snippets stay.
    pub fn reload(&mut self, raw: Option<&str>) {
        self.custom = LiveTemplateCustomizations::parse(raw);
        self.refresh();
    }

    fn refresh(&mut self) {
        self.templates = merge_templates(builtin_templates(), &self.custom, &self.snippets);
    }

    fn commit(&mut self, custom: LiveTemplateCustomizations) -> StorageWrite {
        self.custom = custom;
        self.refresh();
        self.custom.write()
    }

    pub fn templates(&self) -> &[LiveTemplate] {
        &self.templates
    }

    pub fn custom(&self) -> &LiveTemplateCustomizations {
        &self.custom
    }

    /// Replaces the snippets plugins contribute.
    pub fn set_contributed_snippets(&mut self, snippets: Vec<ContributedSnippet>) {
        self.snippets = snippets;
        self.refresh();
    }

    /// Saves a user template or an edited built-in. A plugin's template is read-only: nothing
    /// is stored for it.
    pub fn save_template(&mut self, template: LiveTemplate) -> Option<StorageWrite> {
        if template.plugin_origin.is_some() {
            return None;
        }
        let mut custom = self.custom.clone();
        custom.deleted.retain(|id| *id != template.id);
        custom.templates.insert(template);
        Some(self.commit(custom))
    }

    /// Removes a user template, or hides a built-in. A plugin's template stays.
    pub fn remove_template(&mut self, id: &str) -> Option<StorageWrite> {
        if id.starts_with(PLUGIN_SNIPPET_PREFIX) {
            return None;
        }
        let mut custom = self.custom.clone();
        custom.templates.remove(id);
        if is_builtin_id(id) && !custom.deleted.iter().any(|d| d == id) {
            custom.deleted.push(id.to_owned());
        }
        Some(self.commit(custom))
    }

    /// Drops the user's edits of a built-in, or restores a deleted one.
    pub fn reset_template(&mut self, id: &str) -> StorageWrite {
        let mut custom = self.custom.clone();
        custom.templates.remove(id);
        custom.deleted.retain(|d| d != id);
        self.commit(custom)
    }

    pub fn is_modified_builtin(&self, id: &str) -> bool {
        is_builtin_id(id) && self.custom.templates.contains(id)
    }
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8_lossy(&out).into_owned()
}

/// The id of a new user template: the time in milliseconds and four characters of `entropy`,
/// both in base 36.
pub fn new_template_id(now_ms: u64, entropy: u32) -> String {
    format!("user.{}{:0>4}", base36(now_ms), base36(u64::from(entropy) % 36u64.pow(4)))
}

/// The options of Settings › Editor › Live Templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveTemplateOptions {
    /// Expand abbreviations with Tab.
    pub expand_with_tab: bool,
    /// Offer live templates in the completion list.
    pub show_in_completion: bool,
}

impl Default for LiveTemplateOptions {
    fn default() -> Self {
        Self { expand_with_tab: true, show_in_completion: true }
    }
}

impl LiveTemplateOptions {
    /// The stored options, each defaulting to on.
    pub fn parse(raw: Option<&str>) -> Self {
        let stored = load_object(raw, Map::new());
        Self {
            expand_with_tab: bool_field(&stored, "expandWithTab", true),
            show_in_completion: bool_field(&stored, "showInCompletion", true),
        }
    }

    pub fn write(&self) -> StorageWrite {
        let value = json!({ "expandWithTab": self.expand_with_tab, "showInCompletion": self.show_in_completion });
        StorageWrite::json(LIVE_TEMPLATE_OPTIONS_KEY, &value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::live_snippets::testing::select_snippet;

    fn user_template() -> LiveTemplate {
        LiveTemplate {
            id: "user.x".to_owned(),
            group: "User".to_owned(),
            abbrev: "zz".to_owned(),
            body: "z".to_owned(),
            contexts: vec!["any".to_owned()],
            ..LiveTemplate::default()
        }
    }

    // expansion › merges user edits, deletions and user templates
    #[test]
    fn merges_user_edits_deletions_and_user_templates() {
        let builtins = builtin_templates();
        let user = user_template();
        let edited = LiveTemplate { body: "edited".to_owned(), ..builtins[0].clone() };
        let custom = LiveTemplateCustomizations {
            templates: [edited.clone(), user.clone()].into_iter().collect(),
            deleted: vec![builtins[1].id.clone()],
        };
        let merged = merge_templates(builtins, &custom, &[]);
        assert_eq!(merged.iter().find(|t| t.id == edited.id).map(|t| t.body.as_str()), Some("edited"));
        assert!(!merged.iter().any(|t| t.id == builtins[1].id));
        assert_eq!(merged.last(), Some(&user));
    }

    // plugin snippets › withdraws read-only plugin entries without persisting customizations
    #[test]
    fn withdraws_read_only_plugin_entries_without_persisting_customizations() {
        let snippet = select_snippet();
        let mut store = LiveTemplates::default();
        store.set_contributed_snippets(vec![snippet.clone()]);
        let from_plugin =
            |store: &LiveTemplates| store.templates().iter().find(|t| t.id.starts_with(&snippet.id)).cloned();
        let template = from_plugin(&store).expect("the plugin's template is listed");
        assert_eq!(template.plugin_origin.as_deref(), Some("Plugin"));
        assert_eq!(store.save_template(LiveTemplate { body: "changed".to_owned(), ..template.clone() }), None);
        assert_eq!(store.remove_template(&template.id), None);
        assert_eq!(from_plugin(&store).map(|t| t.body), Some(template.body.clone()));
        assert_eq!(store.custom(), &LiveTemplateCustomizations::default());
        store.set_contributed_snippets(Vec::new());
        assert_eq!(from_plugin(&store), None);
    }

    // Not from a spec: edits are stored under the key in the order they were made, read back the
    // same, and resetting a built-in drops both its edit and its deletion.
    #[test]
    fn stores_and_reads_back_the_customizations() {
        let mut store = LiveTemplates::default();
        let builtin = builtin_templates()[0].clone();
        let write = store.save_template(user_template()).expect("a user template is stored");
        assert_eq!(write.key, LIVE_TEMPLATES_KEY);
        assert!(write.value.starts_with(r#"{"templates":{"user.x":{"id":"user.x","#), "{}", write.value);
        let edit = LiveTemplate { body: "edited".to_owned(), ..builtin.clone() };
        assert!(store.save_template(edit).is_some());
        assert!(store.is_modified_builtin(&builtin.id) && !store.is_modified_builtin("user.x"));
        let write = store.remove_template(&builtin.id).expect("a built-in is hidden");
        assert!(write.value.ends_with(&format!(r#""deleted":["{}"]}}"#, builtin.id)), "{}", write.value);
        assert!(!store.templates().iter().any(|t| t.id == builtin.id));
        let reloaded = LiveTemplates::load(Some(&write.value));
        assert_eq!(reloaded.custom(), store.custom());
        assert_eq!(reloaded.templates().last(), Some(&user_template()));
        assert_eq!(store.reset_template(&builtin.id).key, LIVE_TEMPLATES_KEY);
        assert_eq!(store.templates().first(), Some(&builtin));
        assert!(store.remove_template("user.x").is_some());
        assert_eq!(store.custom(), &LiveTemplateCustomizations::default());
        assert_eq!(LiveTemplateCustomizations::parse(Some("{ not json")), LiveTemplateCustomizations::default());
    }

    // Not from a spec: options default to on, keep stored booleans and are written with both fields.
    #[test]
    fn options_default_to_on_and_round_trip() {
        assert_eq!(LiveTemplateOptions::parse(None), LiveTemplateOptions::default());
        let stored = LiveTemplateOptions::parse(Some(r#"{"expandWithTab":false,"showInCompletion":"no"}"#));
        assert_eq!(stored, LiveTemplateOptions { expand_with_tab: false, show_in_completion: true });
        let write = stored.write();
        assert_eq!(write.key, LIVE_TEMPLATE_OPTIONS_KEY);
        assert_eq!(LiveTemplateOptions::parse(Some(&write.value)), stored);
        assert_eq!(new_template_id(36, 37), "user.100011");
    }
}
