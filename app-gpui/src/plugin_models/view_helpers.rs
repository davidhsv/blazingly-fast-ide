//! Helpers of plugin views and custom editors: a tree view's items as a tree, the table rows to ask
//! for, the file-name patterns custom editors claim, and the plugin settings map.

use ide_plugin::{ViewTreeItem, Viewport};
use regex::RegexBuilder;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Rows asked for above and below the visible ones.
pub const OVERSCAN: u32 = 50;

/// A tree view item and the items under it.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    pub item: ViewTreeItem,
    pub children: Vec<TreeNode>,
}

/// The tree of a flat item list: an item goes under the first earlier item with its parent key;
/// one whose parent is not listed before it is a root.
pub fn tree_nodes(items: &[ViewTreeItem]) -> Vec<TreeNode> {
    /// The nodes of `ids` with their children, recursively.
    fn build(items: &[ViewTreeItem], children: &[Vec<usize>], ids: &[usize]) -> Vec<TreeNode> {
        ids.iter()
            .map(|&at| TreeNode { item: items[at].clone(), children: build(items, children, &children[at]) })
            .collect()
    }
    let mut first: HashMap<&str, usize> = HashMap::new();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); items.len()];
    let mut roots = Vec::new();
    for (at, item) in items.iter().enumerate() {
        match item.parent.as_deref().and_then(|parent| first.get(parent).copied()) {
            Some(parent) => children[parent].push(at),
            None => roots.push(at),
        }
        first.entry(item.key.as_str()).or_insert(at);
    }
    build(items, &children, &roots)
}

/// The rows of a table to ask for: the `visible` ones from `first`, with [`OVERSCAN`] around them.
pub fn wanted_viewport(first: u32, visible: u32, total: u32) -> Viewport {
    let start = first.saturating_sub(OVERSCAN);
    let end = total.min(first + visible + OVERSCAN);
    Viewport { first_row: start, rows: end.saturating_sub(start) }
}

/// Whether file name `name` matches a custom editor's `pattern` (`*`, `?`, `{a,b}`, `[…]`), ignoring
/// case.
pub fn matches_pattern(pattern: &str, name: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut re = String::from("^");
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            '{' => match chars[i..].iter().position(|&c| c == '}') {
                None => re.push_str("\\{"),
                Some(len) => {
                    let inner: String = chars[i + 1..i + len].iter().collect();
                    let options: Vec<String> =
                        inner.split(',').map(|p| regex::escape(p).replace("\\*", "[^/]*")).collect();
                    re.push_str(&format!("(?:{})", options.join("|")));
                    i += len;
                }
            },
            '[' => match chars[i..].iter().position(|&c| c == ']') {
                None => re.push_str("\\["),
                Some(len) => {
                    re.extend(&chars[i..=i + len]);
                    i += len;
                }
            },
            c => re.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    re.push('$');
    RegexBuilder::new(&re).case_insensitive(true).build().is_ok_and(|re| re.is_match(name))
}

/// The plugin settings map with `key` of `plugin_id` set to `value` (`Null`: back to its default,
/// so removed); a plugin without values has no entry.
pub fn with_plugin_setting(
    settings: Option<&Map<String, Value>>,
    plugin_id: &str,
    key: &str,
    value: Value,
) -> Map<String, Value> {
    let mut next = settings.cloned().unwrap_or_default();
    let mut own = next.get(plugin_id).and_then(Value::as_object).cloned().unwrap_or_default();
    if value.is_null() {
        own.remove(key);
    } else {
        own.insert(key.to_owned(), value);
    }
    if own.is_empty() {
        next.remove(plugin_id);
    } else {
        next.insert(plugin_id.to_owned(), Value::Object(own));
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(key: &str, parent: Option<&str>) -> ViewTreeItem {
        ViewTreeItem {
            key: key.to_owned(),
            parent: parent.map(str::to_owned),
            label: key.to_owned(),
            icon: None,
            detail: None,
            expandable: false,
            expanded: false,
        }
    }

    #[test]
    fn builds_the_tree_from_the_flat_item_list() {
        let items = [item("a", None), item("a/1", Some("a")), item("b", None), item("x", Some("missing"))];
        let roots = tree_nodes(&items);
        let keys: Vec<&str> = roots.iter().map(|n| n.item.key.as_str()).collect();
        assert_eq!(keys, ["a", "b", "x"]);
        assert_eq!(roots[0].children.iter().map(|n| n.item.key.as_str()).collect::<Vec<_>>(), ["a/1"]);
    }

    #[test]
    fn asks_for_the_visible_table_rows_plus_some_around_them() {
        assert_eq!(wanted_viewport(0, 20, 10), Viewport { first_row: 0, rows: 10 });
        assert_eq!(wanted_viewport(1000, 20, 5000), Viewport { first_row: 950, rows: 120 });
    }

    #[test]
    fn matches_custom_editor_file_name_patterns() {
        assert!(matches_pattern("*.csv", "People.CSV"));
        assert!(matches_pattern("*.{csv,tsv}", "a.tsv"));
        assert!(matches_pattern("data?.csv", "data1.csv"));
        assert!(!matches_pattern("*.csv", "a.csv.bak"));
    }

    #[test]
    fn stores_plugin_settings_under_the_plugins_id_dropping_defaults() {
        let one = with_plugin_setting(None, "p.x", "watch", json!(false));
        assert_eq!(Value::Object(one.clone()), json!({ "p.x": { "watch": false } }));
        assert!(with_plugin_setting(Some(&one), "p.x", "watch", Value::Null).is_empty());
    }
}
