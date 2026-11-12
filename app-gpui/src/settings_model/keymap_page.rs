//! The Keymap page's tree: actions grouped as IDEA groups them, filtered by name and by shortcut,
//! and the action names the Settings search finds the page by.

use super::keymap_groups::{group_path, group_rank};
use super::keymap_model::{
    KeymapAction, KeymapDraft, binding_matches, is_keymap_action, is_modified, resolve_bindings,
};
use crate::platform::collate;
use crate::ui::matching::word_match;

/// A row of the keymap tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeymapNode {
    Group(GroupNode),
    Action(ActionNode),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNode {
    /// `grp:` and the path joined by `/`.
    pub key: String,
    pub name: String,
    pub path: Vec<String>,
    /// The group's own actions by title, then its subgroups, like a menu.
    pub children: Vec<KeymapNode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionNode {
    pub action: KeymapAction,
    pub bindings: Vec<String>,
    /// The bindings differ from the preset's.
    pub modified: bool,
    /// Byte offsets of the title's characters the name filter matched; empty when the filter
    /// matched the id or a keyword instead, `None` without a filter.
    pub hits: Option<Vec<usize>>,
}

impl KeymapNode {
    /// The row's key: the group's key or the action's id.
    pub fn key(&self) -> &str {
        match self {
            Self::Group(group) => &group.key,
            Self::Action(node) => &node.action.id,
        }
    }

    pub fn children(&self) -> &[KeymapNode] {
        match self {
            Self::Group(group) => &group.children,
            Self::Action(_) => &[],
        }
    }
}

/// The titles of the actions the keymap lists: what the Settings search matches the page by.
pub fn keymap_search_labels(actions: &[KeymapAction], draft: &KeymapDraft) -> Vec<String> {
    actions.iter().filter(|action| is_keymap_action(action, draft)).map(|action| action.title.clone()).collect()
}

fn group_key(path: &[String]) -> String {
    format!("grp:{}", path.join("/"))
}

/// A group while the tree is put together: its own actions apart from its subgroups.
#[derive(Default)]
struct Building {
    name: String,
    path: Vec<String>,
    actions: Vec<ActionNode>,
    groups: Vec<Building>,
}

impl Building {
    /// The group at `path` below this one, created with its ancestors when missing.
    fn group_at(&mut self, path: &[String]) -> &mut Building {
        let mut group = self;
        for (depth, name) in path.iter().enumerate() {
            let at = match group.groups.iter().position(|sub| sub.name == *name) {
                Some(at) => at,
                None => {
                    let sub = Building { name: name.clone(), path: path[..=depth].to_vec(), ..Building::default() };
                    group.groups.push(sub);
                    group.groups.len() - 1
                }
            };
            group = &mut group.groups[at];
        }
        group
    }

    /// The group's rows, ordered like a menu: its own actions by title, then its submenus by
    /// rank and name.
    fn into_nodes(self) -> Vec<KeymapNode> {
        let Building { path, mut actions, mut groups, .. } = self;
        actions.sort_by(|x, y| collate::compare(&x.action.title, &y.action.title));
        groups.sort_by(|x, y| {
            let by_rank = group_rank(&path, &x.name).cmp(&group_rank(&path, &y.name));
            by_rank.then_with(|| collate::compare(&x.name, &y.name))
        });
        let submenus = groups.into_iter().map(|group| {
            let (key, name, path) = (group_key(&group.path), group.name.clone(), group.path.clone());
            KeymapNode::Group(GroupNode { key, name, path, children: group.into_nodes() })
        });
        actions.into_iter().map(KeymapNode::Action).chain(submenus).collect()
    }
}

/// Groups `actions` into IDEA's keymap tree, applying the name filter `query` and the shortcut
/// filter `shortcut` (either may be empty). Groups left without an action are not listed.
pub fn build_keymap_tree(
    actions: &[KeymapAction],
    draft: &KeymapDraft,
    query: &str,
    shortcut: &str,
) -> Vec<KeymapNode> {
    let mut root = Building::default();
    let lowered = query.to_lowercase();
    for action in actions {
        if !is_keymap_action(action, draft) {
            continue;
        }
        let bindings = resolve_bindings(draft, &action.id);
        let hits = if query.is_empty() {
            None
        } else {
            let in_title = word_match(&action.title, query);
            let elsewhere = || {
                action.id.to_lowercase().contains(&lowered)
                    || action.keywords.iter().any(|keyword| keyword.to_lowercase().contains(&lowered))
            };
            match in_title {
                Some(hits) => Some(hits),
                None if elsewhere() => Some(Vec::new()),
                None => continue,
            }
        };
        if !binding_matches(&bindings, shortcut) {
            continue;
        }
        let path = group_path(&action.id, action.category.as_deref());
        let modified = is_modified(draft, &action.id);
        let node = ActionNode { action: action.clone(), bindings, modified, hits };
        root.group_at(&path).actions.push(node);
    }
    root.into_nodes()
}

/// The keys of every group of `nodes` (for Expand All).
pub fn group_keys(nodes: &[KeymapNode]) -> Vec<String> {
    let mut out = Vec::new();
    for node in nodes {
        if let KeymapNode::Group(group) = node {
            out.push(group.key.clone());
            out.extend(group_keys(&group.children));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::actions::manifest;
    use crate::platform::i18n::I18n;
    use crate::platform::keymap::PRESET_TITLES;
    use crate::settings_model::keymap_model::testing::{keymap, mac_draft};

    fn keys(nodes: &[KeymapNode]) -> Vec<&str> {
        nodes.iter().map(KeymapNode::key).collect()
    }

    /// Every key below (and including) `node`, in display order.
    fn all_keys(node: &KeymapNode) -> Vec<&str> {
        std::iter::once(node.key()).chain(node.children().iter().flat_map(all_keys)).collect()
    }

    fn titles(nodes: &[KeymapNode]) -> Vec<&str> {
        nodes
            .iter()
            .flat_map(|node| match node {
                KeymapNode::Group(group) => titles(&group.children),
                KeymapNode::Action(node) => vec![node.action.title.as_str()],
            })
            .collect()
    }

    // finds the translated declaration action by 声明 and its English title
    #[test]
    fn finds_the_translated_declaration_action_by_its_chinese_and_its_english_title() {
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        let meta = manifest().get("navigate.declaration").expect("the declaration action");
        let actions = [KeymapAction::of(meta, meta.localized(&i18n))];
        for query in ["声明", "Go to Declaration or Usages"] {
            let tree = build_keymap_tree(&actions, &mac_draft(&[]), query, "");
            assert!(titles(&tree).contains(&"转到声明或用法"), "{query}");
        }
    }

    // shortcut search › keeps the mouse-only caret action configurable in %s
    #[test]
    fn keeps_the_mouse_only_caret_action_configurable_in_every_preset() {
        let id = "editor.addOrRemoveCaret";
        let title = "Add or Remove Caret";
        let meta = manifest().get(id).expect("the caret action");
        // Hidden from Search Everywhere, yet listed here for its mouse shortcut.
        assert!(meta.hidden);
        let actions = [KeymapAction::of(meta, meta.localized(&I18n::default()))];
        for (preset, _) in PRESET_TITLES {
            for overrides in [keymap(&[]), keymap(&[(id, &["Alt+Button1"])]), keymap(&[(id, &[])])] {
                let draft = KeymapDraft::new(preset, overrides);
                assert!(keymap_search_labels(&actions, &draft).iter().any(|label| label == title), "{preset}");
                let tree = build_keymap_tree(&actions, &draft, title, "");
                let group = tree.iter().find(|node| node.key() == "grp:Editor Actions").expect("the editor group");
                assert_eq!(keys(group.children()), [id], "{preset}");
            }
        }
    }

    // shortcut search › filters the keymap tree by name and by shortcut
    #[test]
    fn filters_the_keymap_tree_by_name_and_by_shortcut() {
        let actions = [
            KeymapAction::new("file.save", "Save All").category("File"),
            KeymapAction::new("navigate.gotoFile", "Go to File…").category("Navigate"),
            KeymapAction { hidden: true, ..KeymapAction::new("internal.x", "Internal") },
        ];
        let draft = mac_draft(&[]);
        let all = build_keymap_tree(&actions, &draft, "", "");
        assert_eq!(keys(&all), ["grp:Main Menu"]);
        assert_eq!(keys(all[0].children()), ["grp:Main Menu/File", "grp:Main Menu/Navigate"]);
        let by_name = build_keymap_tree(&actions, &draft, "gofi", "");
        assert_eq!(keys(by_name[0].children()), ["grp:Main Menu/Navigate"]);
        let by_keys = build_keymap_tree(&actions, &draft, "", "Meta+S");
        assert_eq!(by_keys.len(), 1);
        let file = by_keys[0].children().first().expect("the File group");
        assert_eq!(keys(file.children()), ["file.save"]);
    }

    // keymap tree groups › places actions like IDEA: menu order, submenus after actions, other roots last
    #[test]
    fn places_actions_like_idea_menu_order_submenus_after_actions_other_roots_last() {
        let actions = [
            KeymapAction::new("find.inFile", "Find…").category("Find"),
            KeymapAction::new("edit.undo", "Undo").category("Edit"),
            KeymapAction::new("file.save", "Save All").category("File"),
            KeymapAction::new("debug.stepOver", "Step Over").category("Debug"),
            KeymapAction::new("toolwindow.project", "Project").category("Tool Windows"),
            KeymapAction::new("terminal.new", "New Terminal").category("Terminal"),
            KeymapAction::new("plugin.hello.say", "Say Hello").category("Hello"),
            KeymapAction::new("x.misc", "Misc").category("Weird"),
            KeymapAction::new("editor.showHint", "Show Hint"),
        ];
        let tree = build_keymap_tree(&actions, &mac_draft(&[]), "", "");
        assert_eq!(
            keys(&tree),
            ["grp:Main Menu", "grp:Editor Actions", "grp:Tool Windows", "grp:Other", "grp:Plugins"]
        );
        assert_eq!(
            all_keys(&tree[0]),
            [
                "grp:Main Menu",
                "grp:Main Menu/File",
                "file.save",
                "grp:Main Menu/Edit",
                "edit.undo",
                "grp:Main Menu/Edit/Find",
                "find.inFile",
                "grp:Main Menu/Run",
                "grp:Main Menu/Run/Debugging Actions",
                "debug.stepOver",
            ]
        );
        assert_eq!(
            all_keys(&tree[4]),
            ["grp:Plugins", "grp:Plugins/Hello", "plugin.hello.say", "grp:Plugins/Terminal", "terminal.new"]
        );
    }

    // Not from a spec: a filter that matches only the id or a keyword lists the action without
    // highlights, a modified action is marked, and Expand All gets every group.
    #[test]
    fn marks_modified_actions_and_matches_ids_and_keywords() {
        let save = KeymapAction { keywords: vec!["persist".to_owned()], ..KeymapAction::new("file.save", "Save All") };
        let actions = [save.category("File")];
        let draft = mac_draft(&[("file.save", &["Ctrl+S"])]);
        for query in ["file.sa", "PERSIST"] {
            let tree = build_keymap_tree(&actions, &draft, query, "");
            let KeymapNode::Action(node) = &tree[0].children()[0].children()[0] else { panic!("an action row") };
            assert_eq!((node.hits.as_deref(), node.modified), (Some(&[] as &[usize]), true));
            assert_eq!(node.bindings, ["Ctrl+S"]);
        }
        assert!(build_keymap_tree(&actions, &draft, "nothing", "").is_empty());
        assert!(build_keymap_tree(&actions, &draft, "", "Meta+S").is_empty());
        let tree = build_keymap_tree(&actions, &draft, "", "");
        assert_eq!(group_keys(&tree), ["grp:Main Menu", "grp:Main Menu/File"]);
    }
}
