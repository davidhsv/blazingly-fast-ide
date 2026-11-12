//! A plugin's own text in the UI language. A package's `locales/<tag>.json` maps English source
//! text to a translation; it applies to that plugin's declarations and view data only. The IDE's
//! catalogs are untouched, a plugin never sees another's, and text without an entry stays
//! English.
//!
//! [`PluginCatalogs`] reads nothing: [`PluginCatalogs::load`] names the assets to read and
//! [`PluginCatalogs::loaded`] takes their text; an answer for an older load is dropped.

use super::contribution::Contribution;
use crate::platform::i18n::LOCALES;
use ide_plugin::{View, ViewBody, ViewButton};
use serde_json::{Map, Value};
use std::borrow::Cow;
use std::collections::HashMap;

/// English source text → translation, of one plugin.
pub type Catalog = HashMap<String, String>;
/// Plugin id → its catalog for the UI language.
pub type Catalogs = HashMap<String, Catalog>;

const LOCALES_POINT: &str = "locales";

/// The tag of `tags` that serves `locale`: the exact tag (`zh-CN`), then the language (`zh`), then
/// a variant of it (`zh-TW`).
pub fn pick_catalog_tag<'a>(tags: &'a [String], locale: &str) -> Option<&'a str> {
    let full = LOCALES.iter().find(|info| info.id == locale).map_or(locale, |info| info.tag).to_lowercase();
    let language = full.split('-').next().unwrap_or_default();
    let lower: Vec<String> = tags.iter().map(|tag| tag.to_lowercase()).collect();
    let at = lower.iter().position(|tag| *tag == full).or_else(|| lower.iter().position(|tag| tag == language));
    if let Some(at) = at {
        return Some(&tags[at]);
    }
    tags.iter().zip(&lower).find(|(_, tag)| tag.split('-').next() == Some(language)).map(|(tag, _)| tag.as_str())
}

/// `text` as plugin `plugin_id` translates it.
pub fn translate<'a>(catalogs: &'a Catalogs, plugin_id: &str, text: &'a str) -> &'a str {
    catalogs.get(plugin_id).and_then(|catalog| catalog.get(text)).map_or(text, String::as_str)
}

/// An asset to read for a load: the catalog `asset` of plugin `plugin_id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogAsset {
    pub plugin_id: String,
    /// `locale/<tag>`.
    pub asset: String,
}

/// The catalogs to read for one locale.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogLoad {
    pub ticket: u64,
    pub assets: Vec<CatalogAsset>,
}

/// The plugins' catalogs for the UI language.
#[derive(Debug, Default)]
pub struct PluginCatalogs {
    latest: u64,
    catalogs: Catalogs,
}

impl PluginCatalogs {
    pub fn catalogs(&self) -> &Catalogs {
        &self.catalogs
    }

    /// The catalogs of the plugins that ship one for `locale`; supersedes earlier loads.
    pub fn load(&mut self, contributions: &[Contribution], locale: &str) -> CatalogLoad {
        self.latest += 1;
        let assets = contributions
            .iter()
            .filter(|c| c.point == LOCALES_POINT)
            .filter_map(|c| {
                let tags: Vec<String> = c
                    .decl
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|tags| tags.iter().filter_map(|t| t.as_str().map(str::to_owned)).collect())
                    .unwrap_or_default();
                let tag = pick_catalog_tag(&tags, locale)?;
                Some(CatalogAsset { plugin_id: c.plugin_id.clone(), asset: format!("locale/{tag}") })
            })
            .collect();
        CatalogLoad { ticket: self.latest, assets }
    }

    /// Takes the text read for each asset of `load` (`None`: it could not be read). A catalog that
    /// is not a JSON object of strings is left out. Returns whether the catalogs changed; an
    /// answer for an older load changes nothing.
    pub fn loaded(&mut self, load: &CatalogLoad, texts: &[(CatalogAsset, Option<String>)]) -> bool {
        if load.ticket != self.latest {
            return false;
        }
        let mut catalogs = Catalogs::new();
        for (asset, text) in texts {
            let parsed = text.as_deref().and_then(|text| serde_json::from_str::<Catalog>(text).ok());
            match parsed {
                Some(catalog) => {
                    catalogs.insert(asset.plugin_id.clone(), catalog);
                }
                None => tracing::warn!("plugin {}: cannot load its {} catalog", asset.plugin_id, asset.asset),
            }
        }
        if catalogs == self.catalogs {
            return false;
        }
        self.catalogs = catalogs;
        true
    }
}

/// The keys of each point's declaration that are shown text; ids, names of secrets and option
/// values are not.
fn text_keys(point: &str) -> Option<&'static [&'static str]> {
    Some(match point {
        "commands" => &["title", "category"],
        "menus" => &["submenu"],
        "views" | "customEditors" | "settings" => &["title"],
        "markerIcons" => &["tooltip"],
        "statusBar" => &["text", "tooltip"],
        "fileTemplates" => &["name"],
        "snippets" => &[],
        "iconThemes" => &["label"],
        "themes" => &["name"],
        "secrets" => &["label", "description"],
        _ => return None,
    })
}

fn localize_list(value: &mut Value, keys: &[&str], t: &dyn Fn(&str) -> String) {
    if let Some(items) = value.as_array_mut() {
        for item in items {
            localize_decl(item, keys, t);
        }
    }
}

fn localize_decl(decl: &mut Value, keys: &[&str], t: &dyn Fn(&str) -> String) {
    let Some(object) = decl.as_object_mut() else { return };
    for key in keys {
        if let Some(Value::String(text)) = object.get_mut(*key) {
            *text = t(text);
        }
    }
    localize_children(object, t);
}

fn localize_children(object: &mut Map<String, Value>, t: &dyn Fn(&str) -> String) {
    const FIELD: &[&str] = &["title", "description", "group"];
    if let Some(entries) = object.get_mut("entries") {
        localize_list(entries, &["description"], t);
    }
    if let Some(properties) = object.get_mut("properties") {
        localize_list(properties, FIELD, t);
    }
    if let Some(pages) = object.get_mut("pages") {
        localize_list(pages, &["title"], t);
    }
    if let Some(actions) = object.get_mut("actions") {
        localize_list(actions, &["title"], t);
    }
    if let Some(items) = object.get_mut("items").filter(|items| !items.is_null()) {
        localize_decl(items, FIELD, t);
    }
    if let Some(variables) = object.get_mut("variables") {
        localize_list(variables, &["prompt"], t);
    }
}

/// The declarations with their titles and labels translated by their plugin's catalog; a menu
/// entry also gets its command's translated title as `commandTitle`. Without catalogs they are
/// returned as they are.
pub fn localize_contributions<'a>(items: &'a [Contribution], catalogs: &Catalogs) -> Cow<'a, [Contribution]> {
    if catalogs.is_empty() {
        return Cow::Borrowed(items);
    }
    let localized: Vec<Contribution> = items
        .iter()
        .map(|item| {
            let Some(keys) = text_keys(&item.point) else { return item.clone() };
            let has_text = item.decl.as_object().is_some_and(|decl| {
                keys.iter().any(|key| decl.get(*key).is_some_and(Value::is_string))
                    || decl.get("properties").is_some_and(Value::is_array)
            });
            if !has_text || !catalogs.contains_key(&item.plugin_id) {
                return item.clone();
            }
            let mut decl = item.decl.clone();
            let t = |text: &str| translate(catalogs, &item.plugin_id, text).to_owned();
            localize_decl(&mut decl, keys, &t);
            Contribution { decl, ..item.clone() }
        })
        .collect();
    // A menu entry is labelled with its command's title, which follows the language, not the entry.
    let titles: HashMap<(String, String), String> = localized
        .iter()
        .filter(|c| c.point == "commands")
        .filter_map(|c| Some(((c.plugin_id.clone(), c.text("id")?.to_owned()), c.text("title")?.to_owned())))
        .collect();
    let with_titles = localized
        .into_iter()
        .map(|mut item| {
            if item.point == "menus" && catalogs.contains_key(&item.plugin_id) {
                let command = item.text("command").map(str::to_owned);
                let title = command.and_then(|command| titles.get(&(item.plugin_id.clone(), command)));
                if let (Some(title), Some(decl)) = (title, item.decl.as_object_mut()) {
                    decl.insert("commandTitle".to_owned(), Value::String(title.clone()));
                }
            }
            item
        })
        .collect();
    Cow::Owned(with_titles)
}

/// A view with its buttons, labels, columns and messages translated; `html` bodies and cell data
/// stay as they are.
pub fn localize_view(view: &View, t: &dyn Fn(&str) -> String) -> View {
    let detail = |detail: &Option<String>| detail.as_deref().map(t);
    let body = match &view.body {
        ViewBody::Message(text) => ViewBody::Message(t(text)),
        ViewBody::Tree(items) => ViewBody::Tree(
            items
                .iter()
                .map(|i| ide_plugin::ViewTreeItem { label: t(&i.label), detail: detail(&i.detail), ..i.clone() })
                .collect(),
        ),
        ViewBody::List(items) => ViewBody::List(
            items
                .iter()
                .map(|i| ide_plugin::ViewListItem { label: t(&i.label), detail: detail(&i.detail), ..i.clone() })
                .collect(),
        ),
        ViewBody::Form(fields) => {
            ViewBody::Form(fields.iter().map(|f| ide_plugin::ViewField { label: t(&f.label), ..f.clone() }).collect())
        }
        ViewBody::Table(table) => ViewBody::Table(ide_plugin::ViewTable {
            columns: table.columns.iter().map(String::as_str).map(t).collect(),
            ..table.clone()
        }),
        ViewBody::Html(html) => ViewBody::Html(html.clone()),
    };
    View {
        toolbar: view.toolbar.iter().map(|b| ViewButton { label: t(&b.label), ..b.clone() }).collect(),
        body,
        status: view.status.as_deref().map(t),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_plugin::{FieldValue, ViewField, ViewTreeItem};
    use serde_json::json;

    fn fr() -> Catalogs {
        let catalog: Catalog = [("Refresh", "Actualiser"), ("Word Tools", "Outils de mots"), ("Cancel", "Annuler")]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect();
        Catalogs::from([("a".to_owned(), catalog)])
    }

    fn item(plugin: &str, point: &str, decl: Value) -> Contribution {
        let key = decl.get("id").and_then(Value::as_str).unwrap_or_default().to_owned();
        Contribution::new(plugin, point, &key, decl)
    }

    /// Whether every field of `expected` is in `actual` with the same value (arrays element-wise).
    fn matches(actual: &Value, expected: &Value) -> bool {
        match (actual, expected) {
            (Value::Object(a), Value::Object(e)) => e.iter().all(|(k, v)| a.get(k).is_some_and(|a| matches(a, v))),
            (Value::Array(a), Value::Array(e)) => a.len() == e.len() && a.iter().zip(e).all(|(a, e)| matches(a, e)),
            _ => actual == expected,
        }
    }

    #[test]
    fn tag_for_locale_is_the_exact_tag_then_the_language_then_a_variant() {
        let cases: [(&[&str], &str, Option<&str>); 7] = [
            (&["fr", "zh-CN"], "fr", Some("fr")),
            (&["fr", "zh-CN"], "zh", Some("zh-CN")),
            (&["zh"], "zh", Some("zh")),
            (&["zh-TW"], "zh", Some("zh-TW")),
            (&["pt-br"], "pt", Some("pt-br")),
            (&["fr"], "en", None),
            (&[], "fr", None),
        ];
        for (tags, locale, expected) in cases {
            let tags: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
            assert_eq!(pick_catalog_tag(&tags, locale), expected, "{tags:?} for {locale}");
        }
    }

    #[test]
    fn translates_titles_and_setting_labels_of_the_plugins_own_declarations_only() {
        let items = vec![
            item("a", "commands", json!({ "id": "x", "title": "Refresh", "category": "Word Tools" })),
            item(
                "a",
                "settings",
                json!({
                    "id": "s",
                    "title": "Word Tools",
                    "properties": [{ "key": "k", "title": "Cancel", "enum": ["Refresh"] }],
                }),
            ),
            item("a", "menus", json!({ "id": "m", "menu": "tools", "command": "Refresh" })),
            item("b", "commands", json!({ "id": "y", "title": "Refresh" })),
        ];
        let out = localize_contributions(&items, &fr());
        assert!(matches(&out[0].decl, &json!({ "title": "Actualiser", "category": "Outils de mots" })));
        assert!(matches(
            &out[1].decl,
            &json!({ "title": "Outils de mots", "properties": [{ "title": "Annuler", "enum": ["Refresh"] }] })
        ));
        assert_eq!(out[2], items[2]);
        assert_eq!(out[3], items[3]);
    }

    #[test]
    fn translates_page_group_table_column_and_action_text_without_changing_values() {
        let field = json!({
            "key": "Refresh",
            "type": "array",
            "title": "Word Tools",
            "group": "Refresh",
            "default": [{ "name": "Cancel" }],
            "items": {
                "type": "object",
                "title": "Word Tools",
                "properties": [{
                    "key": "name",
                    "title": "Cancel",
                    "description": "Refresh",
                    "options": ["Cancel"],
                    "default": "Cancel",
                }],
            },
        });
        let declaration = json!({
            "title": "Word Tools",
            "properties": [field],
            "pages": [{
                "id": "Refresh",
                "title": "Cancel",
                "properties": [field],
                "actions": [{ "title": "Refresh", "command": "Refresh" }],
            }],
        });
        let items = [item("a", "settings", declaration.clone())];
        let out = localize_contributions(&items, &fr());
        let expected = json!({
            "title": "Outils de mots",
            "pages": [{
                "id": "Refresh",
                "title": "Annuler",
                "actions": [{ "title": "Actualiser", "command": "Refresh" }],
                "properties": [{
                    "key": "Refresh",
                    "title": "Outils de mots",
                    "group": "Actualiser",
                    "default": [{ "name": "Cancel" }],
                    "items": {
                        "properties": [{
                            "key": "name",
                            "title": "Annuler",
                            "description": "Actualiser",
                            "options": ["Cancel"],
                            "default": "Cancel",
                        }],
                    },
                }],
            }],
        });
        assert!(matches(&out[0].decl, &expected), "{}", out[0].decl);
        assert_eq!(items[0].decl["pages"][0]["title"], "Cancel");
    }

    #[test]
    fn keeps_the_names_of_secrets_and_translates_their_labels() {
        let decl = json!({ "id": "k", "name": "Refresh", "label": "Cancel", "description": "Refresh" });
        let secret = [item("a", "secrets", decl)];
        let out = localize_contributions(&secret, &fr());
        assert!(matches(&out[0].decl, &json!({ "name": "Refresh", "label": "Annuler", "description": "Actualiser" })));
    }

    #[test]
    fn translates_a_context_menu_entrys_submenu_label() {
        let decl = json!({ "id": "m", "menu": "editor.context", "command": "x", "submenu": "Refresh" });
        let items = [item("a", "menus", decl)];
        assert!(matches(&localize_contributions(&items, &fr())[0].decl, &json!({ "submenu": "Actualiser" })));
    }

    #[test]
    fn translates_an_indexed_marker_tooltip_without_changing_its_key_prefixes() {
        let decl = json!({ "id": "m", "icon": "icons/m.svg", "markers": ["Refresh"], "tooltip": "Refresh" });
        let items = [item("a", "markerIcons", decl)];
        let expected = json!({ "id": "m", "icon": "icons/m.svg", "markers": ["Refresh"], "tooltip": "Actualiser" });
        assert_eq!(localize_contributions(&items, &fr())[0].decl, expected);
    }

    #[test]
    fn gives_a_tools_menu_entry_the_translated_title_of_its_command() {
        let items = [
            item("a", "commands", json!({ "id": "x", "title": "Refresh" })),
            item("a", "menus", json!({ "id": "m", "menu": "tools", "command": "x" })),
        ];
        assert!(matches(&localize_contributions(&items, &fr())[1].decl, &json!({ "commandTitle": "Actualiser" })));
    }

    #[test]
    fn ignores_catalog_entries_inherited_from_object_prototype() {
        let items = [item("a", "commands", json!({ "id": "x", "title": "constructor", "category": "__proto__" }))];
        let out = localize_contributions(&items, &fr());
        assert!(matches(&out[0].decl, &json!({ "title": "constructor", "category": "__proto__" })));
        assert_eq!(translate(&fr(), "constructor", "Refresh"), "Refresh");
        assert_eq!(translate(&fr(), "a", "toString"), "toString");
    }

    #[test]
    fn translates_variable_prompts_without_translating_variable_names_or_filenames() {
        let decl = json!({
            "id": "repo",
            "name": "Word Tools",
            "fileName": "Refresh",
            "variables": [{ "name": "Refresh", "prompt": "Cancel" }],
        });
        let items = [item("a", "fileTemplates", decl)];
        let expected = json!({
            "name": "Outils de mots",
            "fileName": "Refresh",
            "variables": [{ "name": "Refresh", "prompt": "Annuler" }],
        });
        assert!(matches(&localize_contributions(&items, &fr())[0].decl, &expected));
    }

    #[test]
    fn returns_the_declarations_as_they_are_without_a_catalog() {
        let items = [item("a", "commands", json!({ "id": "x", "title": "Refresh" }))];
        assert!(matches!(localize_contributions(&items, &Catalogs::new()), Cow::Borrowed(_)));
    }

    fn view(body: ViewBody) -> View {
        View {
            toolbar: vec![ViewButton { id: "r".into(), label: "Refresh".into(), icon: None, enabled: true }],
            body,
            status: Some("Cancel".into()),
        }
    }

    #[test]
    fn translates_buttons_tree_items_fields_and_status_and_keeps_unknown_text() {
        let catalogs = fr();
        let t = |text: &str| translate(&catalogs, "a", text).to_owned();
        let tree = localize_view(
            &view(ViewBody::Tree(vec![ViewTreeItem {
                key: "1".into(),
                parent: None,
                label: "Refresh".into(),
                icon: None,
                detail: Some("other".into()),
                expandable: false,
                expanded: false,
            }])),
            &t,
        );
        assert_eq!(tree.toolbar[0].label, "Actualiser");
        assert_eq!(tree.status.as_deref(), Some("Annuler"));
        let ViewBody::Tree(items) = &tree.body else { panic!("a tree") };
        assert_eq!((items[0].label.as_str(), items[0].detail.as_deref()), ("Actualiser", Some("other")));

        let form = localize_view(
            &view(ViewBody::Form(vec![ViewField {
                key: "k".into(),
                label: "Cancel".into(),
                value: FieldValue::Text("Cancel".into()),
            }])),
            &t,
        );
        let ViewBody::Form(fields) = &form.body else { panic!("a form") };
        assert_eq!(fields[0].label, "Annuler");
        assert_eq!(fields[0].value, FieldValue::Text("Cancel".into()));
    }

    #[test]
    fn leaves_html_bodies_alone() {
        let catalogs = fr();
        let t = |text: &str| translate(&catalogs, "a", text).to_owned();
        let out = localize_view(&view(ViewBody::Html("<p>Refresh</p>".into())), &t);
        assert_eq!(out.body, ViewBody::Html("<p>Refresh</p>".into()));
    }

    #[test]
    fn a_catalog_load_reads_the_tag_of_the_locale_and_drops_an_older_answer() {
        let mut catalogs = PluginCatalogs::default();
        let contributions = [
            Contribution::new("a", "locales", "l", json!({ "tags": ["fr", "zh-CN"] })),
            Contribution::new("b", "locales", "l", json!({ "tags": ["de"] })),
        ];
        let old = catalogs.load(&contributions, "fr");
        let load = catalogs.load(&contributions, "zh");
        assert_eq!(load.assets, vec![CatalogAsset { plugin_id: "a".into(), asset: "locale/zh-CN".into() }]);
        let text = Some(r#"{ "Refresh": "刷新" }"#.to_owned());
        assert!(!catalogs.loaded(&old, &[(old.assets[0].clone(), text.clone())]));
        assert!(catalogs.loaded(&load, &[(load.assets[0].clone(), text.clone())]));
        assert_eq!(translate(catalogs.catalogs(), "a", "Refresh"), "刷新");
        assert!(!catalogs.loaded(&load, &[(load.assets[0].clone(), text)]), "the same catalogs change nothing");
    }
}
