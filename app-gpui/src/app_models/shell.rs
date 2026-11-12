//! What Help › About and Help › Keymap Reference show.

use crate::platform::collate;
use crate::platform::i18n::I18n;
use crate::platform::keymap::format_binding;
use std::collections::HashMap;

/// What the build says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildInfo {
    pub version: String,
    /// The CI build id, or `dev`.
    pub build: String,
    /// `debug` or `release`.
    pub profile: String,
    /// `macos`, `windows`, `linux`.
    pub os: String,
    pub arch: String,
    /// The UI runtime and its version.
    pub runtime: String,
}

/// The operating system's display name (`os.<id>` in the catalog), else the id.
fn os_name(os: &str, i18n: &I18n) -> String {
    let key = format!("os.{os}");
    let name = i18n.t(&key, &[]);
    if name == key { os.to_owned() } else { name }
}

/// The rows of About: version, build, platform and runtime (or the platform alone when the build
/// cannot say, `fallback_os`), then the configuration folder when there is one.
pub fn about_rows(
    build: Option<&BuildInfo>,
    fallback_os: &str,
    config_dir: Option<&str>,
    i18n: &I18n,
) -> Vec<(String, String)> {
    let label = |key: &str| i18n.t(key, &[]);
    let mut rows = match build {
        Some(b) => vec![
            (label("app.about.version"), b.version.clone()),
            (label("app.about.build"), format!("{} ({})", b.build, b.profile)),
            (label("app.about.platform"), format!("{} {}", os_name(&b.os, i18n), b.arch)),
            (label("app.about.runtime"), b.runtime.clone()),
        ],
        None => vec![(label("app.about.platform"), os_name(fallback_os, i18n))],
    };
    rows.extend(config_dir.map(|dir| (label("app.about.configDir"), dir.to_owned())));
    rows
}

/// An action as the reference lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReferencedAction {
    pub title: String,
    pub category: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapEntry {
    pub id: String,
    pub title: String,
    pub shortcuts: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeymapGroup {
    pub category: String,
    pub entries: Vec<KeymapEntry>,
}

const CATEGORY_OF_PREFIX: [(&str, &str); 11] = [
    ("navigate", "Navigate"),
    ("find", "Find"),
    ("code", "Code"),
    ("refactor", "Refactor"),
    ("file", "File"),
    ("editor", "Editor"),
    ("project", "Project View"),
    ("toolwindow", "Tool Windows"),
    ("run", "Run"),
    ("debug", "Debug"),
    ("vcs", "Git"),
];

/// The last part of an action id as words: `code.foldToggle` → `Fold Toggle`.
fn humanize(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    let mut out = String::new();
    let mut previous_lower = false;
    for (index, c) in last.chars().enumerate() {
        if previous_lower && c.is_ascii_uppercase() {
            out.push(' ');
        }
        if index == 0 {
            out.extend(c.to_uppercase())
        } else {
            out.push(c)
        }
        previous_lower = c.is_ascii_lowercase();
    }
    out
}

/// Every bound action's shortcuts (`bindings`: binding → action id, in keymap order), grouped by
/// category: the action's own, else by id prefix, else `other` (listed last). A hidden action is
/// left out; an unregistered one is titled from its id. `filter` keeps the entries whose title,
/// id or shortcut contains it.
pub fn keymap_reference(
    actions: &HashMap<String, ReferencedAction>,
    bindings: &[(String, String)],
    filter: &str,
    other: &str,
    mac: bool,
) -> Vec<KeymapGroup> {
    let mut by_id: Vec<(&str, Vec<&str>)> = Vec::new();
    for (binding, id) in bindings {
        let (id, binding) = (id.as_str(), binding.as_str());
        match by_id.iter_mut().find(|(known, _)| *known == id) {
            Some((_, keys)) if !keys.contains(&binding) => keys.push(binding),
            Some(_) => {}
            None => by_id.push((id, vec![binding])),
        }
    }
    let query = filter.trim().to_lowercase();
    let mut groups: Vec<KeymapGroup> = Vec::new();
    for (id, keys) in by_id {
        let action = actions.get(id);
        if action.is_some_and(|action| action.hidden) {
            continue;
        }
        let title = action.map_or_else(|| humanize(id), |action| action.title.clone());
        let shortcuts: Vec<String> = keys.iter().map(|key| format_binding(key, mac)).collect();
        if !query.is_empty() {
            let hay = [title.as_str(), id]
                .into_iter()
                .chain(shortcuts.iter().map(String::as_str))
                .chain(keys.iter().copied());
            if !hay.collect::<Vec<_>>().join(" ").to_lowercase().contains(&query) {
                continue;
            }
        }
        let prefix = id.split('.').next().unwrap_or_default();
        let category = action
            .and_then(|action| action.category.clone())
            .or_else(|| CATEGORY_OF_PREFIX.iter().find(|(p, _)| *p == prefix).map(|(_, c)| (*c).to_owned()))
            .unwrap_or_else(|| other.to_owned());
        let entry = KeymapEntry { id: id.to_owned(), title, shortcuts };
        match groups.iter_mut().find(|group| group.category == category) {
            Some(group) => group.entries.push(entry),
            None => groups.push(KeymapGroup { category, entries: vec![entry] }),
        }
    }
    groups.sort_by(|a, b| match (a.category == other, b.category == other) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => collate::compare(&a.category, &b.category),
    });
    for group in &mut groups {
        group.entries.sort_by(|a, b| collate::compare(&a.title, &b.title));
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions() -> HashMap<String, ReferencedAction> {
        HashMap::from([
            (
                "file.save".to_owned(),
                ReferencedAction { title: "Save All".into(), category: Some("File".into()), hidden: false },
            ),
            ("hidden.one".to_owned(), ReferencedAction { title: "Hidden".into(), category: None, hidden: true }),
        ])
    }

    fn bindings() -> Vec<(String, String)> {
        [
            ("Meta+S", "file.save"),
            ("Meta+O", "navigate.gotoClass"),
            ("Meta+-", "code.foldToggle"),
            ("Meta+=", "code.foldToggle"),
            ("Meta+H", "hidden.one"),
            ("Alt+X", "zzz.custom"),
        ]
        .map(|(binding, id)| (binding.to_owned(), id.to_owned()))
        .to_vec()
    }

    fn ids(groups: &[KeymapGroup]) -> Vec<&str> {
        groups.iter().flat_map(|group| group.entries.iter().map(|entry| entry.id.as_str())).collect()
    }

    // keymap reference › groups bound actions by category, unknown ones by id prefix
    #[test]
    fn groups_bound_actions_by_category_unknown_ones_by_id_prefix() {
        let groups = keymap_reference(&actions(), &bindings(), "", "Other", true);
        let categories: Vec<&str> = groups.iter().map(|group| group.category.as_str()).collect();
        assert_eq!(categories, ["Code", "File", "Navigate", "Other"]);
        let file = groups.iter().find(|group| group.category == "File").expect("File");
        let save = KeymapEntry { id: "file.save".into(), title: "Save All".into(), shortcuts: vec!["⌘S".into()] };
        assert_eq!(file.entries, [save]);
        let code = &groups.iter().find(|group| group.category == "Code").expect("Code").entries[0];
        assert_eq!(
            (code.title.as_str(), code.shortcuts.as_slice()),
            ("Fold Toggle", ["⌘-".to_owned(), "⌘=".to_owned()].as_slice())
        );
        assert!(!ids(&groups).contains(&"hidden.one"));
    }

    // keymap reference › filters by title, id or shortcut
    #[test]
    fn filters_by_title_id_or_shortcut() {
        assert_eq!(ids(&keymap_reference(&actions(), &bindings(), "save", "Other", true)), ["file.save"]);
        assert_eq!(ids(&keymap_reference(&actions(), &bindings(), "⌘O", "Other", true)), ["navigate.gotoClass"]);
    }

    // about › lists version, build and platform
    #[test]
    fn lists_version_build_and_platform() {
        let build = BuildInfo {
            version: "0.1.0".into(),
            build: "dev".into(),
            profile: "debug".into(),
            os: "macos".into(),
            arch: "aarch64".into(),
            runtime: "GPUI 0.9.1".into(),
        };
        let rows = about_rows(Some(&build), "macos", Some("/cfg"), &I18n::default());
        let rows: Vec<(&str, &str)> = rows.iter().map(|(label, value)| (label.as_str(), value.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("Version", "0.1.0"),
                ("Build", "dev (debug)"),
                ("Platform", "macOS aarch64"),
                ("Runtime", "GPUI 0.9.1"),
                ("Configuration", "/cfg")
            ]
        );
    }
}
