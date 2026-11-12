//! File icon themes contributed by plugins (`iconThemes`). The selection is the
//! `appearance.fileIconTheme` setting; a theme's mapping and sanitised SVGs are read from the host
//! only once it is selected, so installed icon themes cost nothing at start-up.
//!
//! [`IconThemes`] reads nothing: [`IconThemes::sync`] says what to load or clear for the selection,
//! [`IconThemes::loaded`] takes the assets and returns the icons to register; the resolver for the
//! current options comes from [`IconThemes::resolver`].

use super::contribution::Contribution;
use super::icon_theme_resolver::{IconThemeMap, IconThemeResolver};
use crate::theme::themes::{IconThemeChoice, IconThemeDetails};
use ide_core::settings::FileIconThemeOptions;
use std::collections::{BTreeMap, HashSet};

const ICON_THEMES_POINT: &str = "iconThemes";
/// The empty icon the hide options draw.
const HIDDEN_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"></svg>"#;

/// `<pluginId>/<themeId>`: the `appearance.fileIconTheme` value of a theme.
pub fn icon_theme_key(plugin_id: &str, id: &str) -> String {
    format!("{plugin_id}/{id}")
}

/// The id of a plugin's settings page.
pub fn plugin_settings_page_id(plugin_id: &str) -> String {
    format!("plugins.{}", plugin_id.replace('.', "-"))
}

/// A selectable icon theme of a live plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconThemeRef {
    pub plugin_id: String,
    pub id: String,
    pub label: String,
    /// The plugin's name.
    pub origin: Option<String>,
    pub settings_page: Option<String>,
}

impl IconThemeRef {
    /// The theme an `iconThemes` declaration of `plugin_name`'s plugin offers, with its settings page.
    pub fn of(contribution: &Contribution, plugin_name: &str) -> Option<Self> {
        let id = contribution.text("id")?;
        let settings_page = format!("{}.{}", plugin_settings_page_id(&contribution.plugin_id), id.replace('.', "-"));
        Some(Self {
            plugin_id: contribution.plugin_id.clone(),
            id: id.to_owned(),
            label: contribution.text("label").unwrap_or(id).to_owned(),
            origin: Some(plugin_name.to_owned()),
            settings_page: Some(settings_page),
        })
    }

    pub fn key(&self) -> String {
        icon_theme_key(&self.plugin_id, &self.id)
    }

    /// How Settings › Appearance lists it.
    pub fn choice(&self) -> IconThemeChoice {
        IconThemeChoice {
            id: self.key(),
            label: self.label.clone(),
            origin: self.origin.clone(),
            settings_page: self.settings_page.clone(),
        }
    }

    /// The asset its settings (packs, choices, settings sections) are read from.
    pub fn details_asset(&self) -> String {
        format!("iconTheme/{}/settings", self.id)
    }
}

/// The settings of a theme from the assets read for [`IconThemeRef::details_asset`].
pub fn parse_details(theme: &IconThemeRef, assets: &[(String, String)]) -> Option<IconThemeDetails> {
    let id = theme.details_asset();
    let json = assets.iter().find(|(asset, _)| *asset == id).map(|(_, json)| json)?;
    serde_json::from_str(json).ok()
}

/// The first icon theme of `plugin_id`, to select when the user has none: a plugin enabled or
/// installed with one shows it at once.
pub fn adopt_icon_theme(selected: Option<&str>, contributions: &[Contribution], plugin_id: &str) -> Option<String> {
    if selected.is_some_and(|s| !s.is_empty()) {
        return None;
    }
    let first = contributions.iter().find(|c| c.plugin_id == plugin_id && c.point == ICON_THEMES_POINT)?;
    Some(icon_theme_key(plugin_id, first.text("id")?))
}

/// A theme whose icons are registered.
#[derive(Clone, Debug)]
struct Applied {
    key: String,
    map: IconThemeMap,
    names: HashSet<String>,
}

/// What the selection needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IconThemeStep {
    /// Read the plugin's assets under `prefix` and answer with [`IconThemes::loaded`].
    Load { ticket: u64, key: String, plugin_id: String, prefix: String },
    /// The built-in icons: unregister these icon names and drop the resolver.
    Clear { unregister: Vec<String> },
}

/// The icons to register for a loaded theme.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledIconTheme {
    pub key: String,
    /// `(name, svg)`.
    pub icons: Vec<(String, String)>,
    /// The icon names of the theme replaced by this one, to unregister.
    pub unregister: Vec<String>,
}

#[derive(Debug, Default)]
pub struct IconThemes {
    available: BTreeMap<String, IconThemeRef>,
    applied: Option<Applied>,
    /// The key being loaded and its ticket.
    loading: Option<(String, u64)>,
    next: u64,
}

impl IconThemes {
    /// The themes Settings › Appearance offers.
    pub fn choices(&self) -> Vec<IconThemeChoice> {
        self.available.values().map(IconThemeRef::choice).collect()
    }

    pub fn get(&self, key: &str) -> Option<&IconThemeRef> {
        self.available.get(key)
    }

    /// The key of the theme whose icons are registered.
    pub fn applied(&self) -> Option<&str> {
        self.applied.as_ref().map(|applied| applied.key.as_str())
    }

    /// The `iconThemes` projection: the theme can be selected while its plugin is live.
    pub fn add(&mut self, theme: IconThemeRef) {
        self.available.insert(theme.key(), theme);
    }

    pub fn remove(&mut self, key: &str) {
        self.available.remove(key);
    }

    /// Brings the registered icons in line with `selected` (the setting) and the available themes.
    pub fn sync(&mut self, selected: Option<&str>) -> Option<IconThemeStep> {
        let wanted = selected.filter(|key| !key.is_empty()).and_then(|key| self.available.get(key));
        let Some(theme) = wanted else {
            self.loading = None;
            let applied = self.applied.take()?;
            return Some(IconThemeStep::Clear { unregister: applied.names.into_iter().collect() });
        };
        let key = theme.key();
        if self.applied() == Some(key.as_str()) || self.loading.as_ref().is_some_and(|(k, _)| *k == key) {
            return None;
        }
        self.next += 1;
        self.loading = Some((key.clone(), self.next));
        let prefix = format!("iconTheme/{}/", theme.id);
        Some(IconThemeStep::Load { ticket: self.next, key, plugin_id: theme.plugin_id.clone(), prefix })
    }

    /// The assets read for a [`IconThemeStep::Load`]: the theme's `map` and its `svg/<icon>` files.
    /// `Ok(None)` when a newer selection superseded it.
    pub fn loaded(&mut self, ticket: u64, assets: &[(String, String)]) -> Result<Option<InstalledIconTheme>, String> {
        let Some((key, current)) = self.loading.clone() else { return Ok(None) };
        if current != ticket {
            return Ok(None);
        }
        self.loading = None;
        let Some(theme) = self.available.get(&key) else { return Ok(None) };
        let prefix = format!("iconTheme/{}/", theme.id);
        let map_id = format!("{prefix}map");
        let map_json = assets
            .iter()
            .find(|(id, _)| *id == map_id)
            .map(|(_, json)| json)
            .ok_or_else(|| format!("icon theme {key} has no mapping"))?;
        let map: IconThemeMap = serde_json::from_str(map_json).map_err(|e| format!("icon theme {key}: {e}"))?;
        let svg_prefix = format!("{prefix}svg/");
        let mut icons: Vec<(String, String)> = assets
            .iter()
            .filter_map(|(id, svg)| id.strip_prefix(&svg_prefix).map(|icon| (icon_name(&key, icon), svg.clone())))
            .collect();
        icons.push((icon_name(&key, "__hidden"), HIDDEN_SVG.to_owned()));
        let names = icons.iter().map(|(name, _)| name.clone()).collect();
        let unregister = self.applied.take().map(|applied| applied.names.into_iter().collect()).unwrap_or_default();
        self.applied = Some(Applied { key: key.clone(), map, names });
        Ok(Some(InstalledIconTheme { key, icons, unregister }))
    }

    /// The resolver of the applied theme with the user's `options` for it.
    pub fn resolver(&self, options: FileIconThemeOptions) -> Option<IconThemeResolver> {
        let applied = self.applied.as_ref()?;
        let key = applied.key.clone();
        let names = applied.names.clone();
        let hidden = icon_name(&key, "__hidden");
        Some(IconThemeResolver::new(
            applied.map.clone(),
            move |icon| icon_name(&key, icon),
            move |name| names.contains(name),
            &hidden,
            options,
        ))
    }
}

/// The registered name of icon `icon` of theme `key`.
pub fn icon_name(key: &str, icon: &str) -> String {
    format!("ext:{key}/{icon}")
}

/// Whether the options of `key` differ between two settings snapshots (the resolver is rebuilt
/// only then).
pub fn options_changed(
    key: &str,
    before: &BTreeMap<String, FileIconThemeOptions>,
    after: &BTreeMap<String, FileIconThemeOptions>,
) -> bool {
    before.get(key) != after.get(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M0 0h16v16z"/></svg>"#;

    fn mapping() -> String {
        json!({
            "file": "file",
            "folder": "folder",
            "fileExtensions": {},
            "fileNames": {},
            "folderNames": {},
            "patterns": [{
                "pattern": "^.*\\.spec\\.ts$", "icon": "test", "target": "file", "path": false,
                "ignoreCase": false, "group": null, "openIcon": null,
            }],
            "overrides": { "chevron-right": "arrow" },
            "groups": [{ "id": "extra", "name": "Extra", "enabled": false }],
            "choices": [],
            "accent": true,
        })
        .to_string()
    }

    fn theme(plugin_id: &str, id: &str, settings_page: Option<&str>) -> IconThemeRef {
        IconThemeRef {
            plugin_id: plugin_id.to_owned(),
            id: id.to_owned(),
            label: "T".to_owned(),
            origin: None,
            settings_page: settings_page.map(str::to_owned),
        }
    }

    fn options(value: serde_json::Value) -> FileIconThemeOptions {
        serde_json::from_value(value).expect("options")
    }

    #[test]
    fn applies_the_themes_patterns_overrides_and_the_users_options() {
        let mut themes = IconThemes::default();
        themes.add(theme("p", "t", None));
        let Some(IconThemeStep::Load { ticket, plugin_id, prefix, .. }) = themes.sync(Some("p/t")) else {
            panic!("the theme is loaded")
        };
        assert_eq!((plugin_id.as_str(), prefix.as_str()), ("p", "iconTheme/t/"));
        assert_eq!(themes.sync(Some("p/t")), None, "a load in flight is not repeated");
        let mut assets = vec![("iconTheme/t/map".to_owned(), mapping())];
        let svgs = ["file", "folder", "test", "arrow"].map(|icon| (format!("iconTheme/t/svg/{icon}"), SVG.to_owned()));
        assets.extend(svgs);
        let installed = themes.loaded(ticket, &assets).expect("a mapping").expect("current");
        assert!(installed.icons.iter().any(|(name, svg)| name == "ext:p/t/arrow" && svg.contains("M0 0h16v16z")));
        let resolver = themes.resolver(options(json!({}))).expect("applied");
        assert_eq!(resolver.file("a.spec.ts", None).as_deref(), Some("ext:p/t/test"));
        assert_eq!(resolver.file("a.rs", None).as_deref(), Some("ext:p/t/file"));
        assert_eq!(resolver.icon("chevron-right").as_deref(), Some("ext:p/t/arrow"));

        let before = BTreeMap::from([("p/t".to_owned(), options(json!({})))]);
        assert!(!options_changed("p/t", &before, &before), "the same options keep the resolver");
        let hidden = options(json!({ "hideFiles": true, "rowHeight": 30 }));
        let after = BTreeMap::from([("p/t".to_owned(), hidden.clone())]);
        assert!(options_changed("p/t", &before, &after));
        let resolver = themes.resolver(hidden.clone()).expect("applied");
        assert_eq!(resolver.file("a.spec.ts", None).as_deref(), Some("ext:p/t/__hidden"));
        assert_eq!(resolver.folder("src", false, false, None).as_deref(), Some("ext:p/t/folder"));
        assert_eq!(hidden.row_height, Some(30));

        let Some(IconThemeStep::Clear { unregister }) = themes.sync(None) else { panic!("cleared") };
        assert!(unregister.contains(&"ext:p/t/test".to_owned()));
        assert!(themes.resolver(options(json!({}))).is_none());
        assert_eq!(themes.applied(), None);
    }

    #[test]
    fn offers_the_themes_settings_whether_or_not_it_is_selected() {
        let settings = json!({
            "groups": [{ "id": "extra", "name": "Extra", "enabled": false }],
            "choices": [],
            "accent": true,
            "settings": [{ "title": "T", "items": [] }],
        });
        let mut themes = IconThemes::default();
        let acme = theme("acme.icons", "t", Some("plugins.acme-icons.t"));
        themes.add(acme.clone());
        themes.add(theme("plain", "t", None));
        let of = |themes: &IconThemes, id: &str| themes.choices().into_iter().find(|t| t.id == id);
        let page = of(&themes, "acme.icons/t").and_then(|t| t.settings_page);
        assert_eq!(page.as_deref(), Some("plugins.acme-icons.t"));
        assert_eq!(acme.details_asset(), "iconTheme/t/settings");
        let read = [("iconTheme/t/settings".to_owned(), settings.to_string())];
        let details = parse_details(&acme, &read).expect("settings");
        assert_eq!(serde_json::to_value(details).expect("JSON"), settings);
        assert_eq!(parse_details(themes.get("plain/t").expect("listed"), &[]), None);
        themes.remove("acme.icons/t");
        assert!(of(&themes, "acme.icons/t").is_none());
    }

    fn icon_theme_ui(plugin: &str, id: &str) -> Contribution {
        Contribution::new(plugin, "iconThemes", id, json!({ "id": id, "label": id.to_uppercase(), "icons": 1 }))
    }

    #[test]
    fn adopts_the_first_icon_theme_of_a_plugin_when_none_is_selected() {
        let ui = [icon_theme_ui("q", "other"), icon_theme_ui("p", "first"), icon_theme_ui("p", "second")];
        let adopted = adopt_icon_theme(None, &ui, "p");
        assert_eq!(adopted.as_deref(), Some("p/first"));
        assert_eq!(adopt_icon_theme(adopted.as_deref(), &ui, "q"), None);
    }

    #[test]
    fn names_each_icon_themes_settings_page_after_its_plugin_and_theme() {
        let theme = IconThemeRef::of(&icon_theme_ui("acme.icons", "atom.dark"), "Acme").expect("a theme");
        let choice = theme.choice();
        assert_eq!(choice.id, "acme.icons/atom.dark");
        assert_eq!(choice.label, "ATOM.DARK");
        assert_eq!(choice.settings_page.as_deref(), Some("plugins.acme-icons.atom-dark"));
    }

    #[test]
    fn adopts_nothing_for_plugins_without_icon_themes() {
        assert_eq!(adopt_icon_theme(None, &[icon_theme_ui("q", "other")], "p"), None);
    }
}
