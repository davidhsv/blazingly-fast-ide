//! How a plugin's file icon theme names the icon of a file, a folder or a built-in UI icon: its
//! patterns in order (only those of enabled packs), then file and folder names, then extensions
//! from the longest, then its defaults; the user's options (hidden icons, hollow folders, choices)
//! on top. An icon the theme does not have is skipped as if it were not mapped.

use crate::ui::pattern_index::{IndexedPattern, PatternIndex};
use ide_core::settings::FileIconThemeOptions;
use regex::Regex;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;

/// Longest name and path a pattern is tested against, in UTF-16 code units.
const MAX_NAME_LEN: usize = 255;
const MAX_PATH_LEN: usize = 1024;
/// Most file names remembered before the cache starts over.
const MAX_CACHED: usize = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternTarget {
    File,
    Folder,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemePattern {
    pub pattern: String,
    pub icon: String,
    pub target: PatternTarget,
    pub path: bool,
    pub ignore_case: bool,
    pub group: Option<String>,
    pub open_icon: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemeGroup {
    pub id: String,
    pub name: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemeChoiceOption {
    pub id: String,
    pub name: String,
    pub overrides: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemeChoice {
    pub id: String,
    pub name: String,
    pub default: String,
    pub options: Vec<IconThemeChoiceOption>,
}

/// An icon theme's mapping (`iconTheme/<id>/map`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconThemeMap {
    pub file: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub folder_expanded: Option<String>,
    #[serde(default)]
    pub folder_open: Option<String>,
    #[serde(default)]
    pub file_extensions: HashMap<String, String>,
    #[serde(default)]
    pub file_names: HashMap<String, String>,
    #[serde(default)]
    pub folder_names: HashMap<String, String>,
    #[serde(default)]
    pub patterns: Vec<IconThemePattern>,
    #[serde(default)]
    pub overrides: HashMap<String, String>,
    #[serde(default)]
    pub groups: Vec<IconThemeGroup>,
    #[serde(default)]
    pub choices: Vec<IconThemeChoice>,
    #[serde(default)]
    pub accent: bool,
}

/// The kinds of code symbols whose icons the "symbol icons" option covers (besides `symbol-*`).
const SYMBOL_KINDS: [&str; 19] = [
    "class",
    "interface",
    "enum",
    "struct",
    "trait",
    "method",
    "function",
    "constructor",
    "field",
    "property",
    "variable",
    "constant",
    "parameter",
    "module",
    "namespace",
    "package",
    "type",
    "annotation",
    "snippet",
];

/// Whether `name` is a code symbol's icon rather than a UI icon.
pub fn is_symbol_icon(name: &str) -> bool {
    name.starts_with("symbol-") || SYMBOL_KINDS.contains(&name)
}

struct Compiled {
    re: Regex,
    source: String,
    path: bool,
    order: usize,
    icon: String,
    open_icon: Option<String>,
}

impl IndexedPattern for Compiled {
    fn source(&self) -> &str {
        &self.source
    }

    fn path(&self) -> bool {
        self.path
    }

    fn order(&self) -> usize {
        self.order
    }

    fn is_match(&self, subject: &str) -> bool {
        let limit = if self.path { MAX_PATH_LEN } else { MAX_NAME_LEN };
        subject.encode_utf16().count() <= limit && self.re.is_match(subject)
    }
}

fn compile(map: &IconThemeMap, target: PatternTarget, options: &FileIconThemeOptions) -> PatternIndex<Compiled> {
    let enabled = |group: Option<&str>| {
        let Some(group) = group else { return true };
        options
            .groups
            .get(group)
            .copied()
            .or_else(|| map.groups.iter().find(|g| g.id == group).map(|g| g.enabled))
            .unwrap_or(true)
    };
    let compiled = map
        .patterns
        .iter()
        .enumerate()
        .filter(|(_, p)| p.target == target && enabled(p.group.as_deref()))
        .filter_map(|(order, p)| {
            let source = if p.ignore_case { format!("(?i){}", p.pattern) } else { p.pattern.clone() };
            let re = Regex::new(&source).ok()?;
            let (icon, open_icon) = (p.icon.clone(), p.open_icon.clone());
            Some(Compiled { re, source: p.pattern.clone(), path: p.path, order, icon, open_icon })
        })
        .collect();
    PatternIndex::new(compiled)
}

/// The overrides of the option the user chose for each choice (else its default).
fn chosen_overrides(map: &IconThemeMap, options: &FileIconThemeOptions) -> HashMap<String, String> {
    let mut chosen = HashMap::new();
    for choice in &map.choices {
        let picked = options.choices.get(&choice.id);
        let option = choice
            .options
            .iter()
            .find(|o| Some(&o.id) == picked)
            .or_else(|| choice.options.iter().find(|o| o.id == choice.default));
        if let Some(option) = option {
            chosen.extend(option.overrides.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }
    chosen
}

/// A theme's icons for files, folders and built-in icons.
pub struct IconThemeResolver {
    map: IconThemeMap,
    icon_name: Box<dyn Fn(&str) -> String>,
    has: Box<dyn Fn(&str) -> bool>,
    hidden: String,
    options: FileIconThemeOptions,
    files: PatternIndex<Compiled>,
    folders: PatternIndex<Compiled>,
    chosen: HashMap<String, String>,
    cache: RefCell<HashMap<String, Option<String>>>,
}

impl IconThemeResolver {
    /// `icon_name` names a theme icon (`ext:<plugin>/<theme>/<icon>`), `has` tells which names exist,
    /// and `hidden` is the empty icon the hide options draw (empty: they hide nothing).
    pub fn new(
        map: IconThemeMap,
        icon_name: impl Fn(&str) -> String + 'static,
        has: impl Fn(&str) -> bool + 'static,
        hidden: &str,
        options: FileIconThemeOptions,
    ) -> Self {
        let files = compile(&map, PatternTarget::File, &options);
        let folders = compile(&map, PatternTarget::Folder, &options);
        let chosen = chosen_overrides(&map, &options);
        Self {
            map,
            icon_name: Box::new(icon_name),
            has: Box::new(has),
            hidden: hidden.to_owned(),
            options,
            files,
            folders,
            chosen,
            cache: RefCell::default(),
        }
    }

    fn pick(&self, icon: Option<&str>) -> Option<String> {
        let name = (self.icon_name)(icon.filter(|icon| !icon.is_empty())?);
        (self.has)(&name).then_some(name)
    }

    fn resolve_file(&self, file_name: &str, path: Option<&str>) -> Option<String> {
        let lower = file_name.to_lowercase();
        if let Some(matched) = self.files.find(file_name, path).and_then(|hit| self.pick(Some(&hit.icon))) {
            return Some(matched);
        }
        if let Some(named) = self.pick(self.map.file_names.get(&lower).map(String::as_str)) {
            return Some(named);
        }
        let mut rest = lower.as_str();
        while let Some(dot) = rest.find('.') {
            rest = &rest[dot + 1..];
            if let Some(by_ext) = self.pick(self.map.file_extensions.get(rest).map(String::as_str)) {
                return Some(by_ext);
            }
        }
        self.pick(self.map.file.as_deref())
    }

    /// The icon of file `file_name`, at `path` for path patterns (backslashes count as slashes).
    pub fn file(&self, file_name: &str, path: Option<&str>) -> Option<String> {
        if self.options.hide_files && !self.hidden.is_empty() {
            return Some(self.hidden.clone());
        }
        let path = path.filter(|p| !p.is_empty()).map(|p| p.replace('\\', "/"));
        let key = path.clone().unwrap_or_else(|| format!("\0{file_name}"));
        if let Some(hit) = self.cache.borrow().get(&key) {
            return hit.clone();
        }
        let icon = self.resolve_file(file_name, path.as_deref());
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(key, icon.clone());
        icon
    }

    /// The icon of folder `folder_name`; `holds_open_file` picks the open variant with hollow folders.
    pub fn folder(
        &self,
        folder_name: &str,
        expanded: bool,
        holds_open_file: bool,
        path: Option<&str>,
    ) -> Option<String> {
        if self.options.hide_folders && !self.hidden.is_empty() {
            return Some(self.hidden.clone());
        }
        let open = self.options.hollow_folders && holds_open_file;
        let path = path.filter(|p| !p.is_empty()).map(|p| p.replace('\\', "/"));
        if let Some(hit) = self.folders.find(folder_name, path.as_deref()) {
            let matched = if open { self.pick(hit.open_icon.as_deref()) } else { None };
            if let Some(matched) = matched.or_else(|| self.pick(Some(&hit.icon))) {
                return Some(matched);
            }
        }
        if let Some(named) = self.pick(self.map.folder_names.get(&folder_name.to_lowercase()).map(String::as_str)) {
            return Some(named);
        }
        if open && let Some(open) = self.pick(self.map.folder_open.as_deref()) {
            return Some(open);
        }
        let expanded = if expanded { self.pick(self.map.folder_expanded.as_deref()) } else { None };
        expanded.or_else(|| self.pick(self.map.folder.as_deref()))
    }

    /// The theme's replacement for built-in icon `name`: the chosen option's first, then the theme's
    /// own; `None` keeps the built-in one.
    pub fn icon(&self, name: &str) -> Option<String> {
        let hidden = if is_symbol_icon(name) { self.options.hide_psi_icons } else { self.options.hide_ui_icons };
        if hidden {
            return None;
        }
        let replaced = self.chosen.get(name).or_else(|| self.map.overrides.get(name))?;
        self.pick(Some(replaced))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn name(icon: &str) -> String {
        format!("ext:p/t/{icon}")
    }

    const HIDDEN: &str = "ext:p/t/__hidden";

    fn spec() -> serde_json::Value {
        json!({
            "pattern": "^.*\\.spec\\.ts$", "icon": "test", "target": "file", "path": false,
            "ignoreCase": true, "group": null, "openIcon": null,
        })
    }

    fn map() -> IconThemeMap {
        serde_json::from_value(json!({
            "file": "file",
            "folder": "folder",
            "folderOpen": "folder-open",
            "fileExtensions": { "ts": "ts" },
            "fileNames": { "readme.md": "readme" },
            "folderNames": { "docs": "folder-docs" },
            "patterns": [
                spec(),
                { "pattern": "/fixtures/", "icon": "fixture", "target": "file", "path": true, "ignoreCase": false,
                  "group": "extra", "openIcon": null },
                { "pattern": "^src$", "icon": "folder-src", "target": "folder", "path": false, "ignoreCase": false,
                  "group": null, "openIcon": "folder-src-open" },
                { "pattern": "^angular\\.json$", "icon": "angular", "target": "file", "path": false,
                  "ignoreCase": false, "group": "angular", "openIcon": null },
            ],
            "overrides": { "chevron-right": "arrow", "symbol-class": "klass", "folder": "ui-folder", "method": "meth" },
            "groups": [
                { "id": "extra", "name": "Extra", "enabled": true },
                { "id": "angular", "name": "Angular", "enabled": false },
            ],
            "choices": [{
                "id": "arrows", "name": "Arrows", "default": "plain",
                "options": [
                    { "id": "plain", "name": "Default", "overrides": {} },
                    { "id": "plus", "name": "Plus", "overrides": { "chevron-right": "plus" } },
                ],
            }],
            "accent": true,
        }))
        .expect("a mapping")
    }

    fn options(value: serde_json::Value) -> FileIconThemeOptions {
        serde_json::from_value(value).expect("options")
    }

    fn resolve(value: serde_json::Value) -> IconThemeResolver {
        IconThemeResolver::new(map(), name, |_| true, HIDDEN, options(value))
    }

    fn some(icon: &str) -> Option<String> {
        Some(name(icon))
    }

    #[test]
    fn matches_patterns_in_order_before_names_and_extensions() {
        let theme = resolve(json!({}));
        assert_eq!(theme.file("A.SPEC.ts", None), some("test"));
        assert_eq!(theme.file("a.ts", None), some("ts"));
        assert_eq!(theme.file("README.md", None), some("readme"));
        assert_eq!(theme.file("x.bin", None), some("file"));
    }

    #[test]
    fn matches_path_patterns_against_the_normalized_path_only_when_one_is_given() {
        let theme = resolve(json!({}));
        assert_eq!(theme.file("a.json", Some("C:\\p\\fixtures\\a.json")), some("fixture"));
        assert_eq!(theme.file("a.json", Some("/p/fixtures/a.json")), some("fixture"));
        assert_eq!(theme.file("a.json", None), some("file"));
        assert_eq!(theme.file("a.json", Some("/p/src/a.json")), some("file"));
    }

    #[test]
    fn follows_the_groups_defaults_and_the_users_choice_of_packs() {
        assert_eq!(resolve(json!({})).file("angular.json", None), some("file"));
        assert_eq!(resolve(json!({ "groups": { "angular": true } })).file("angular.json", None), some("angular"));
        let no_extra = resolve(json!({ "groups": { "extra": false } }));
        assert_eq!(no_extra.file("a.json", Some("/p/fixtures/a.json")), some("file"));
    }

    #[test]
    fn uses_the_open_variants_for_folders_holding_open_files_only_with_hollow_folders() {
        let hollow = || resolve(json!({ "hollowFolders": true }));
        assert_eq!(resolve(json!({})).folder("src", false, true, None), some("folder-src"));
        assert_eq!(hollow().folder("src", false, true, None), some("folder-src-open"));
        assert_eq!(hollow().folder("src", true, false, None), some("folder-src"));
        assert_eq!(hollow().folder("lib", false, true, None), some("folder-open"));
        assert_eq!(resolve(json!({})).folder("Docs", false, false, None), some("folder-docs"));
        assert_eq!(resolve(json!({})).folder("lib", true, false, None), some("folder"));
    }

    #[test]
    fn hides_file_or_folder_icons() {
        assert_eq!(resolve(json!({ "hideFiles": true })).file("a.ts", None).as_deref(), Some(HIDDEN));
        assert_eq!(resolve(json!({ "hideFiles": true })).folder("src", false, false, None), some("folder-src"));
        let no_folders = resolve(json!({ "hideFolders": true }));
        assert_eq!(no_folders.folder("src", false, false, None).as_deref(), Some(HIDDEN));
    }

    #[test]
    fn replaces_built_in_icons_the_chosen_option_first() {
        assert_eq!(resolve(json!({})).icon("chevron-right"), some("arrow"));
        assert_eq!(resolve(json!({})).icon("symbol-class"), some("klass"));
        assert_eq!(resolve(json!({ "choices": { "arrows": "plus" } })).icon("chevron-right"), some("plus"));
        assert_eq!(resolve(json!({ "choices": { "arrows": "gone" } })).icon("chevron-right"), some("arrow"));
        assert_eq!(resolve(json!({})).icon("terminal"), None);
    }

    #[test]
    fn drops_the_ui_or_the_symbol_icon_replacements_the_user_turned_off() {
        let no_ui = resolve(json!({ "hideUiIcons": true, "choices": { "arrows": "plus" } }));
        assert_eq!(no_ui.icon("chevron-right"), None);
        assert_eq!(no_ui.icon("folder"), None);
        assert_eq!(no_ui.icon("symbol-class"), some("klass"));
        let no_psi = resolve(json!({ "hidePsiIcons": true }));
        assert_eq!(no_psi.icon("symbol-class"), None);
        assert_eq!(no_psi.icon("method"), None);
        assert_eq!(no_ui.icon("method"), some("meth"));
        assert_eq!(no_psi.icon("folder"), some("ui-folder"));
    }

    #[test]
    fn skips_icons_the_theme_does_not_have_and_patterns_that_cannot_compile() {
        let mut mapping = map();
        let mut broken: IconThemePattern = serde_json::from_value(spec()).expect("a pattern");
        broken.pattern = "(".to_owned();
        let mut missing: IconThemePattern = serde_json::from_value(spec()).expect("a pattern");
        missing.icon = "missing".to_owned();
        mapping.patterns = vec![broken, missing];
        let theme = IconThemeResolver::new(mapping, name, |n| n != name("missing"), HIDDEN, options(json!({})));
        assert_eq!(theme.file("a.spec.ts", None), some("ts"));
    }

    #[test]
    fn resolves_file_names_then_the_longest_extension_then_the_default_icon() {
        let map: IconThemeMap = serde_json::from_value(json!({
            "file": "file",
            "fileNames": { "cargo.toml": "cargo" },
            "fileExtensions": { "d.ts": "dts", "ts": "ts", "toml": "toml" },
        }))
        .expect("a mapping");
        let theme = IconThemeResolver::new(map, name, |n| n != "ext:p/t/toml", "", options(json!({})));
        assert_eq!(theme.file("Cargo.toml", None), some("cargo"));
        assert_eq!(theme.file("a.d.ts", None), some("dts"));
        assert_eq!(theme.file("a.ts", None), some("ts"));
        assert_eq!(theme.file("x.toml", None), some("file"));
        assert_eq!(theme.folder("src", false, false, None), None, "folders keep the built-in icons");
    }

    #[test]
    fn resolves_folders_by_name_then_the_default_expanded_folder_icon() {
        let map: IconThemeMap = serde_json::from_value(json!({
            "file": null,
            "folder": "folder",
            "folderExpanded": "folder-open",
            "fileNames": {},
            "fileExtensions": {},
            "folderNames": { "src": "folder-src" },
        }))
        .expect("a mapping");
        let theme = IconThemeResolver::new(map, name, |_| true, "", options(json!({})));
        assert_eq!(theme.folder("SRC", true, false, None), some("folder-src"));
        assert_eq!(theme.folder("lib", false, false, None), some("folder"));
        assert_eq!(theme.folder("lib", true, false, None), some("folder-open"));
        assert_eq!(theme.file("a.txt", None), None);
    }
}
