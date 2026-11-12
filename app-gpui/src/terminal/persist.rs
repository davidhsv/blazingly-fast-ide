//! Terminal tabs remembered per project: which tabs were open, each one's working directory and
//! rename, and how the tab groups were split. Shell scrollback is not kept; a tab comes back as a
//! new session in that directory.
//!
//! The stored value is JSON under [`storage_key`]:
//! `{ "v": 1, "tabs": [{ "id", "name", "customName", "cwd", "group", "shell" }], "layout",
//! "selected": { group: tab }, "activeTab" }`, where a layout is a group id or
//! `{ "id", "orientation", "children" }`.

use super::store::{GroupLayout, GroupSplit, SplitOrientation};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Storage key prefix of a project's remembered tabs; the project root follows.
pub const STORAGE_PREFIX: &str = "openidea.terminal.tabs.v1:";
/// The version stored as `v`; any other value is not read.
const VERSION: u8 = 1;
/// How many tabs are read back.
const MAX_TABS: usize = 32;

/// The key the tabs of the project at `root` are stored under.
pub fn storage_key(root: &str) -> String {
    format!("{STORAGE_PREFIX}{root}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTab {
    pub id: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub cwd: String,
    pub group: String,
    /// Predefined shell program. A one-shot command is not replayed.
    pub shell: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTerminals {
    pub tabs: Vec<StoredTab>,
    pub layout: GroupLayout,
    /// The selected tab of each group.
    pub selected: BTreeMap<String, String>,
    pub active_tab: Option<String>,
}

fn layout_value(layout: &GroupLayout) -> Value {
    match layout {
        GroupLayout::Group(group) => Value::String(group.clone()),
        GroupLayout::Split(split) => json!({
            "id": split.id,
            "orientation": split.orientation.as_str(),
            "children": split.children.iter().map(layout_value).collect::<Vec<_>>(),
        }),
    }
}

impl StoredTerminals {
    /// The JSON to store.
    pub fn serialize(&self) -> String {
        let tabs: Vec<Value> = self
            .tabs
            .iter()
            .map(|tab| {
                json!({
                    "id": tab.id,
                    "name": tab.name,
                    "customName": tab.custom_name,
                    "cwd": tab.cwd,
                    "group": tab.group,
                    "shell": tab.shell,
                })
            })
            .collect();
        json!({
            "v": VERSION,
            "tabs": tabs,
            "layout": layout_value(&self.layout),
            "selected": self.selected,
            "activeTab": self.active_tab,
        })
        .to_string()
    }
}

/// The remembered tabs in a stored string; `None` when nothing usable is stored.
pub fn parse(stored: Option<&str>) -> Option<StoredTerminals> {
    decode(&serde_json::from_str::<Value>(stored?).ok()?)
}

fn text<'a>(object: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    object.get(field).and_then(Value::as_str)
}

/// Reads a stored value. One of another version, or without a usable layout, is rejected whole;
/// tabs that are malformed or repeat an id are dropped, and references to tabs or groups that
/// do not exist are forgotten.
pub fn decode(raw: &Value) -> Option<StoredTerminals> {
    let stored = raw.as_object()?;
    if stored.get("v").and_then(Value::as_f64) != Some(f64::from(VERSION)) {
        return None;
    }
    let items = stored.get("tabs")?.as_array()?;
    let layout = decode_layout(stored.get("layout")?)?;
    let groups: BTreeSet<String> = layout.groups().into_iter().map(str::to_owned).collect();
    let fallback = layout.groups().first().map(|group| (*group).to_owned())?;
    let mut tabs: Vec<StoredTab> = Vec::new();
    for item in items {
        if tabs.len() >= MAX_TABS {
            break;
        }
        if let Some(tab) = decode_tab(item, &groups, &fallback)
            && tabs.iter().all(|known| known.id != tab.id)
        {
            tabs.push(tab);
        }
    }
    let has_tab = |id: &str| tabs.iter().any(|tab| tab.id == id);
    let mut selected = BTreeMap::new();
    if let Some(entries) = stored.get("selected").and_then(Value::as_object) {
        for (group, id) in entries {
            if let Some(id) = id.as_str()
                && groups.contains(group.as_str())
                && has_tab(id)
            {
                selected.insert(group.clone(), id.to_owned());
            }
        }
    }
    let active_tab = text(stored, "activeTab").filter(|id| has_tab(id)).map(str::to_owned);
    Some(StoredTerminals { tabs, layout, selected, active_tab })
}

fn decode_tab(item: &Value, groups: &BTreeSet<String>, fallback: &str) -> Option<StoredTab> {
    let tab = item.as_object()?;
    let id = text(tab, "id").filter(|id| !id.is_empty())?;
    let name = text(tab, "name").map(str::trim).filter(|name| !name.is_empty())?;
    let cwd = text(tab, "cwd").filter(|cwd| !cwd.is_empty())?;
    let group = text(tab, "group").filter(|group| groups.contains(*group)).unwrap_or(fallback);
    let custom_name = text(tab, "customName").map(str::trim).filter(|name| !name.is_empty());
    let shell = text(tab, "shell").filter(|shell| !shell.is_empty());
    Some(StoredTab {
        id: id.to_owned(),
        name: name.to_owned(),
        custom_name: custom_name.map(str::to_owned),
        cwd: cwd.to_owned(),
        group: group.to_owned(),
        shell: shell.map(str::to_owned),
    })
}

fn decode_layout(raw: &Value) -> Option<GroupLayout> {
    if let Some(group) = raw.as_str() {
        return (!group.is_empty()).then(|| GroupLayout::Group(group.to_owned()));
    }
    let node = raw.as_object()?;
    let id = text(node, "id").filter(|id| !id.is_empty())?;
    let orientation = SplitOrientation::parse(text(node, "orientation")?)?;
    let children = node.get("children")?.as_array().filter(|children| children.len() >= 2)?;
    let children = children.iter().map(decode_layout).collect::<Option<Vec<_>>>()?;
    Some(GroupLayout::Split(GroupSplit { id: id.to_owned(), orientation, children }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // src/features/terminal/persist.test.ts › terminal tab memory › rejects a stored value that is not this version
    #[test]
    fn rejects_a_stored_value_that_is_not_this_version() {
        assert_eq!(decode(&Value::Null), None);
        assert_eq!(decode(&json!({ "v": 2, "tabs": [], "layout": "g" })), None);
        let without_tabs = decode(&json!({ "v": 1, "tabs": [{}], "layout": "g" }));
        assert_eq!(without_tabs.map(|stored| stored.tabs), Some(Vec::new()));
        assert_eq!(decode(&json!({ "v": 1, "tabs": "nope", "layout": "g" })), None);
    }

    // Not from a spec: the stored JSON keeps its field names, and reads back as it was written.
    #[test]
    fn stores_the_documented_shape() {
        let layout = GroupLayout::Split(GroupSplit {
            id: "split1".to_owned(),
            orientation: SplitOrientation::Horizontal,
            children: vec![GroupLayout::Group("g1".to_owned()), GroupLayout::Group("g2".to_owned())],
        });
        let tab = |id: &str, group: &str, custom_name: Option<&str>, shell: Option<&str>| StoredTab {
            id: id.to_owned(),
            name: "Local".to_owned(),
            custom_name: custom_name.map(str::to_owned),
            cwd: "/repo".to_owned(),
            group: group.to_owned(),
            shell: shell.map(str::to_owned),
        };
        let stored = StoredTerminals {
            tabs: vec![tab("t1", "g1", Some("Server"), None), tab("t2", "g2", None, Some("/bin/zsh"))],
            layout,
            selected: BTreeMap::from([("g1".to_owned(), "t1".to_owned()), ("g2".to_owned(), "t2".to_owned())]),
            active_tab: Some("t2".to_owned()),
        };
        let written: Value = serde_json::from_str(&stored.serialize()).unwrap();
        let first = json!({
            "id": "t1", "name": "Local", "customName": "Server", "cwd": "/repo", "group": "g1", "shell": null,
        });
        let second = json!({
            "id": "t2", "name": "Local", "customName": null, "cwd": "/repo", "group": "g2", "shell": "/bin/zsh",
        });
        assert_eq!(
            written,
            json!({
                "v": 1,
                "tabs": [first, second],
                "layout": { "id": "split1", "orientation": "horizontal", "children": ["g1", "g2"] },
                "selected": { "g1": "t1", "g2": "t2" },
                "activeTab": "t2",
            })
        );
        assert_eq!(parse(Some(&stored.serialize())), Some(stored));
        assert_eq!(parse(None), None);
        assert_eq!(parse(Some("not json")), None);
        assert_eq!(storage_key("/repo"), "openidea.terminal.tabs.v1:/repo");
    }

    // Not from a spec: what a stored value refers to must exist, or it is dropped.
    #[test]
    fn drops_what_a_stored_value_cannot_refer_to() {
        let stored = decode(&json!({
            "v": 1,
            "tabs": [
                { "id": "a", "name": " Local ", "customName": "  ", "cwd": "/p", "group": "gone", "shell": "" },
                { "id": "a", "name": "Twice", "cwd": "/p", "group": "g2" },
                { "id": "b", "name": "No directory", "cwd": "", "group": "g2" },
            ],
            "layout": { "id": "s", "orientation": "vertical", "children": ["g1", "g2"] },
            "selected": { "g1": "a", "g2": "b", "g3": "a" },
            "activeTab": "b",
        }))
        .unwrap();
        assert_eq!(stored.tabs.len(), 1);
        let tab = &stored.tabs[0];
        assert_eq!((tab.name.as_str(), tab.group.as_str()), ("Local", "g1"));
        assert_eq!((tab.custom_name.as_deref(), tab.shell.as_deref()), (None, None));
        assert_eq!(stored.selected, BTreeMap::from([("g1".to_owned(), "a".to_owned())]));
        assert_eq!(stored.active_tab, None);
        let one_child = json!({ "id": "s", "orientation": "vertical", "children": ["g"] });
        assert_eq!(decode(&json!({ "v": 1, "tabs": [], "layout": one_child })), None);
    }
}
