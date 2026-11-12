//! The document outline as the Structure view and the File Structure popup show it: keyed nodes, the
//! view's options (sorting, hiding fields and non-public symbols, inherited members), the symbols around
//! the caret and the popup's speed search.

use super::inherited::{InheritedMember, type_key};
use super::open::kind_name;
use super::rank::{WORD_ALIGNED, match_text};
use crate::platform::collate::{self, Sensitivity};
use ide_core::{OutlineItem, Pos, SymbolKind, Visibility};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Where an inherited member comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InheritedFrom {
    /// File (or virtual document URI) declaring the member.
    pub path: String,
    /// Name of the supertype declaring it.
    pub owner: String,
}

/// A row of the outline tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineNode {
    pub key: String,
    /// The symbol itself. Its members are this node's `children`; `item.children` is left empty.
    pub item: OutlineItem,
    pub children: Vec<OutlineNode>,
    /// Show Inherited: set on a supertype's member.
    pub inherited: Option<InheritedFrom>,
}

/// `item` without its members.
fn own(item: &OutlineItem) -> OutlineItem {
    OutlineItem {
        name: item.name.clone(),
        detail: item.detail.clone(),
        kind: item.kind,
        range: item.range,
        selection_range: item.selection_range,
        children: Vec::new(),
        visibility: item.visibility,
    }
}

/// `node` with other children.
fn with_children(node: &OutlineNode, children: Vec<OutlineNode>) -> OutlineNode {
    OutlineNode { key: node.key.clone(), item: node.item.clone(), children, inherited: node.inherited.clone() }
}

/// `#n` for every occurrence of `id` after the first, counted in `seen`.
fn occurrence_suffix(seen: &mut HashMap<String, usize>, id: &str) -> String {
    let count = seen.entry(id.to_owned()).or_insert(0);
    let suffix = if *count > 0 { format!("#{count}") } else { String::new() };
    *count += 1;
    suffix
}

fn key_children(items: &[OutlineItem], parent: &str) -> Vec<OutlineNode> {
    let mut seen = HashMap::new();
    items
        .iter()
        .map(|item| {
            let suffix = occurrence_suffix(&mut seen, &item.name);
            let key = format!("{parent}/{}:{}{suffix}", kind_name(item.kind), item.name);
            let children = key_children(&item.children, &key);
            OutlineNode { key, item: own(item), children, inherited: None }
        })
        .collect()
}

/// The outline as a tree of nodes with stable keys: the parent's key, the symbol's kind and name, and the
/// occurrence among same-named siblings.
pub fn key_outline(items: &[OutlineItem]) -> Vec<OutlineNode> {
    key_children(items, "")
}

/// Appends each type's inherited members (by [`type_key`]) after its own ones.
pub fn with_inherited(
    nodes: &[OutlineNode],
    inherited: Option<&HashMap<String, Vec<InheritedMember>>>,
) -> Vec<OutlineNode> {
    let Some(inherited) = inherited.filter(|members| !members.is_empty()) else { return nodes.to_vec() };
    nodes
        .iter()
        .map(|node| {
            let mut children = with_inherited(&node.children, Some(inherited));
            let extra = if node.inherited.is_some() { None } else { inherited.get(&type_key(&node.item)) };
            let mut seen = HashMap::new();
            for member in extra.into_iter().flatten() {
                let id = format!("{}:{}:{}", member.owner, kind_name(member.item.kind), member.item.name);
                let suffix = occurrence_suffix(&mut seen, &id);
                children.push(OutlineNode {
                    key: format!("{}/inherited:{id}{suffix}", node.key),
                    item: own(&member.item),
                    children: Vec::new(),
                    inherited: Some(InheritedFrom { path: member.path.clone(), owner: member.owner.clone() }),
                });
            }
            with_children(node, children)
        })
        .collect()
}

/// Rank for "Sort by Visibility": unknown counts as public.
pub fn visibility_rank(item: &OutlineItem) -> u8 {
    match item.visibility {
        None | Some(Visibility::Public) => 0,
        Some(Visibility::Protected) => 1,
        Some(Visibility::Package) => 2,
        Some(Visibility::Private) => 3,
    }
}

/// The tree sorted on every level: by visibility when asked, then by name (ignoring case and accents)
/// when alphabetical; symbols that compare equal keep the file's order.
pub fn sort_outline(nodes: &[OutlineNode], alphabetical: bool, by_visibility: bool) -> Vec<OutlineNode> {
    if !alphabetical && !by_visibility {
        return nodes.to_vec();
    }
    let mut sorted: Vec<&OutlineNode> = nodes.iter().collect();
    sorted.sort_by(|a, b| {
        let visibility =
            if by_visibility { visibility_rank(&a.item).cmp(&visibility_rank(&b.item)) } else { Ordering::Equal };
        visibility.then_with(|| {
            if alphabetical {
                collate::compare_with(&a.item.name, &b.item.name, false, Sensitivity::Base)
            } else {
                Ordering::Equal
            }
        })
    });
    sorted
        .into_iter()
        .map(|node| with_children(node, sort_outline(&node.children, alphabetical, by_visibility)))
        .collect()
}

fn is_field(kind: SymbolKind) -> bool {
    matches!(kind, SymbolKind::Field | SymbolKind::Property)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutlineViewFilter {
    /// IDEA's "Show Fields": fields and properties.
    pub fields: bool,
    /// IDEA's "Show Non-Public": private, protected and package symbols.
    pub non_public: bool,
}

/// Hides the filtered-out symbols together with their members.
pub fn apply_view_filter(nodes: &[OutlineNode], filter: OutlineViewFilter) -> Vec<OutlineNode> {
    if filter.fields && filter.non_public {
        return nodes.to_vec();
    }
    nodes
        .iter()
        .filter(|node| {
            let public = node.item.visibility.is_none_or(|visibility| visibility == Visibility::Public);
            (filter.fields || !is_field(node.item.kind)) && (filter.non_public || public)
        })
        .map(|node| with_children(node, apply_view_filter(&node.children, filter)))
        .collect()
}

/// Whether any symbol carries a visibility (else the visibility options would do nothing).
pub fn has_visibility(items: &[OutlineItem]) -> bool {
    items.iter().any(|item| item.visibility.is_some() || has_visibility(&item.children))
}

/// Whether any symbol is a field or a property.
pub fn has_fields(items: &[OutlineItem]) -> bool {
    items.iter().any(|item| is_field(item.kind) || has_fields(&item.children))
}

fn contains(item: &OutlineItem, pos: Pos) -> bool {
    item.range.start <= pos && pos <= item.range.end
}

/// Keys from the root to the innermost symbol containing `pos`; empty if there is none. Inherited
/// members live in other files and are never on the path.
pub fn symbol_path_at(nodes: &[OutlineNode], pos: Pos) -> Vec<String> {
    for node in nodes {
        if node.inherited.is_none() && contains(&node.item, pos) {
            let mut path = vec![node.key.clone()];
            path.extend(symbol_path_at(&node.children, pos));
            return path;
        }
    }
    Vec::new()
}

/// Symbols from the outermost to the innermost one containing `pos` (breadcrumbs).
pub fn outline_path_at(items: &[OutlineItem], pos: Pos) -> Vec<&OutlineItem> {
    for item in items {
        if contains(item, pos) {
            let mut path = vec![item];
            path.extend(outline_path_at(&item.children, pos));
            return path;
        }
    }
    Vec::new()
}

/// Keys of the nodes that have children, parents before their descendants.
pub fn all_keys(nodes: &[OutlineNode]) -> Vec<String> {
    let mut out = Vec::new();
    collect_keys(nodes, &mut out);
    out
}

fn collect_keys(nodes: &[OutlineNode], out: &mut Vec<String>) {
    for node in nodes {
        if !node.children.is_empty() {
            out.push(node.key.clone());
        }
        collect_keys(&node.children, out);
    }
}

/// The result of the File Structure popup's speed search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutlineFilter {
    /// The tree to show: when narrowing, only the matches and their ancestors.
    pub nodes: Vec<OutlineNode>,
    /// Per matching node, the matched characters of its name (byte offsets).
    pub matches: HashMap<String, Vec<usize>>,
    /// Matching keys in tree order (↑/↓ cycle them when not narrowing).
    pub order: Vec<String>,
    /// The best-scoring match, preselected while filtering.
    pub best: Option<String>,
}

#[derive(Default)]
struct Found {
    matches: HashMap<String, Vec<usize>>,
    order: Vec<String>,
    best: Option<(String, i32)>,
}

/// Records the matches below `list`; when narrowing, returns the matching nodes with their ancestors.
fn search(list: &[OutlineNode], query: &str, narrow: bool, found: &mut Found) -> Vec<OutlineNode> {
    let mut out = Vec::new();
    for node in list {
        let hit = match_text(&node.item.name, query, WORD_ALIGNED);
        let matched = hit.is_some();
        if let Some(hit) = hit {
            if found.best.as_ref().is_none_or(|(_, score)| hit.score > *score) {
                found.best = Some((node.key.clone(), hit.score));
            }
            found.order.push(node.key.clone());
            found.matches.insert(node.key.clone(), hit.indices);
        }
        let children = search(&node.children, query, narrow, found);
        if narrow && (matched || !children.is_empty()) {
            out.push(with_children(node, children));
        }
    }
    out
}

/// The File Structure popup's speed search: names matching `query` at word and camel-hump starts.
/// Matches keep their ancestors; without `narrow` nothing is hidden and the matches are only
/// highlighted.
pub fn filter_outline(nodes: &[OutlineNode], query: &str, narrow: bool) -> OutlineFilter {
    let query = query.trim();
    if query.is_empty() {
        return OutlineFilter { nodes: nodes.to_vec(), ..OutlineFilter::default() };
    }
    let mut found = Found::default();
    let narrowed = search(nodes, query, narrow, &mut found);
    OutlineFilter {
        nodes: if narrow { narrowed } else { nodes.to_vec() },
        matches: found.matches,
        order: found.order,
        best: found.best.map(|(key, _)| key),
    }
}

/// Parent key of every node (`None` for top-level ones).
pub fn parent_keys(nodes: &[OutlineNode]) -> HashMap<String, Option<String>> {
    let mut out = HashMap::new();
    collect_parents(nodes, None, &mut out);
    out
}

fn collect_parents(nodes: &[OutlineNode], parent: Option<&str>, out: &mut HashMap<String, Option<String>>) {
    for node in nodes {
        out.insert(node.key.clone(), parent.map(str::to_owned));
        collect_parents(&node.children, Some(node.key.as_str()), out);
    }
}

/// Rows a tree of `nodes` shows with the `collapsed` keys collapsed.
pub fn visible_count(nodes: &[OutlineNode], collapsed: &HashSet<String>) -> usize {
    nodes
        .iter()
        .map(|node| 1 + if collapsed.contains(&node.key) { 0 } else { visible_count(&node.children, collapsed) })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Range;

    fn item(name: &str, kind: SymbolKind, from: u32, to: u32, children: Vec<OutlineItem>) -> OutlineItem {
        let len = name.len() as u32;
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            range: Range::new(Pos::new(from, 0), Pos::new(to, 1)),
            selection_range: Range::new(Pos::new(from, 4), Pos::new(from, 4 + len)),
            children,
            visibility: None,
        }
    }

    fn outline() -> Vec<OutlineItem> {
        let members = vec![
            item("list", SymbolKind::Method, 2, 6, vec![]),
            item("create", SymbolKind::Method, 8, 12, vec![]),
            item("list", SymbolKind::Method, 14, 16, vec![]),
        ];
        vec![
            item("OrderController", SymbolKind::Class, 0, 20, members),
            item("helper", SymbolKind::Function, 22, 25, vec![]),
        ]
    }

    fn vis(item: OutlineItem, visibility: Visibility) -> OutlineItem {
        OutlineItem { visibility: Some(visibility), ..item }
    }

    fn repo() -> OutlineItem {
        let members = vec![
            vis(item("cache", SymbolKind::Field, 1, 1, vec![]), Visibility::Private),
            vis(item("find", SymbolKind::Method, 2, 5, vec![]), Visibility::Public),
            vis(item("load", SymbolKind::Method, 6, 9, vec![]), Visibility::Private),
            vis(item("check", SymbolKind::Method, 10, 12, vec![]), Visibility::Protected),
            item("size", SymbolKind::Property, 13, 13, vec![]),
            item("all", SymbolKind::Method, 14, 16, vec![]),
        ];
        item("Repo", SymbolKind::Class, 0, 30, members)
    }

    fn names(nodes: &[OutlineNode]) -> Vec<&str> {
        nodes.iter().map(|node| node.item.name.as_str()).collect()
    }

    #[test]
    fn hides_fields_and_non_public_symbols() {
        let nodes = key_outline(&[repo()]);
        let no_fields = apply_view_filter(&nodes, OutlineViewFilter { fields: false, non_public: true });
        assert_eq!(names(&no_fields[0].children), ["find", "load", "check", "all"]);
        let public = apply_view_filter(&nodes, OutlineViewFilter { fields: true, non_public: false });
        assert_eq!(names(&public[0].children), ["find", "size", "all"]);
        assert_eq!(apply_view_filter(&nodes, OutlineViewFilter { fields: true, non_public: true }), nodes);
    }

    #[test]
    fn sorts_by_visibility_then_by_name_when_alphabetical() {
        let nodes = key_outline(&[repo()]);
        let by_visibility = sort_outline(&nodes, false, true);
        assert_eq!(names(&by_visibility[0].children), ["find", "size", "all", "check", "cache", "load"]);
        let both = sort_outline(&nodes, true, true);
        assert_eq!(names(&both[0].children), ["all", "find", "size", "check", "cache", "load"]);
    }

    #[test]
    fn offers_options_only_when_the_outline_has_the_data() {
        assert!(has_visibility(&[repo()]));
        assert!(!has_visibility(&outline()));
        assert!(has_fields(&[repo()]));
        assert!(!has_fields(&outline()));
    }

    #[test]
    fn finds_the_symbols_around_a_position() {
        let outline = outline();
        let around: Vec<&str> =
            outline_path_at(&outline, Pos::new(9, 0)).into_iter().map(|item| item.name.as_str()).collect();
        assert_eq!(around, ["OrderController", "create"]);
        assert!(outline_path_at(&outline, Pos::new(40, 0)).is_empty());
    }

    #[test]
    fn gives_stable_unique_keys_to_same_named_siblings() {
        let nodes = key_outline(&outline());
        let keys: HashSet<&str> = nodes[0].children.iter().map(|child| child.key.as_str()).collect();
        assert_eq!(keys.len(), 3);
        assert!(nodes[0].children[2].key.ends_with("#1"));
        assert_eq!(all_keys(&nodes), [nodes[0].key.clone()]);
    }

    #[test]
    fn finds_the_innermost_symbol_around_a_position() {
        let nodes = key_outline(&outline());
        let path = symbol_path_at(&nodes, Pos::new(9, 3));
        assert_eq!(path, [nodes[0].key.clone(), nodes[0].children[1].key.clone()]);
        assert!(symbol_path_at(&nodes, Pos::new(21, 0)).is_empty());
    }

    #[test]
    fn sorts_alphabetically_recursively() {
        let nodes = key_outline(&outline());
        let sorted = sort_outline(&nodes, true, false);
        assert_eq!(names(&sorted), ["helper", "OrderController"]);
        assert_eq!(names(&sorted[1].children), ["create", "list", "list"]);
        assert_eq!(sort_outline(&nodes, false, false), nodes);
    }

    #[test]
    fn narrows_to_matches_and_their_ancestors() {
        let nodes = key_outline(&outline());
        let filtered = filter_outline(&nodes, "creat", true);
        assert_eq!(names(&filtered.nodes), ["OrderController"]);
        assert_eq!(names(&filtered.nodes[0].children), ["create"]);
        assert_eq!(filtered.best.as_deref(), Some(nodes[0].children[1].key.as_str()));
        assert!(!filtered.matches.contains_key(&nodes[0].key));
        assert_eq!(filter_outline(&nodes, "", true).nodes, nodes);
    }

    #[test]
    fn matches_word_and_camel_hump_starts_not_scattered_letters() {
        let loose = key_outline(&[
            item("items.reduce() callback", SymbolKind::Function, 0, 1, vec![]),
            item("subtotal", SymbolKind::Method, 2, 3, vec![]),
        ]);
        assert_eq!(names(&filter_outline(&loose, "sub", true).nodes), ["subtotal"]);
        let nodes = key_outline(&outline());
        assert_eq!(names(&filter_outline(&nodes, "OC", true).nodes), ["OrderController"]);
    }

    #[test]
    fn only_highlights_matches_when_not_narrowing_down() {
        let nodes = key_outline(&outline());
        let filtered = filter_outline(&nodes, "list", false);
        assert_eq!(filtered.nodes, nodes);
        assert_eq!(filtered.order, [nodes[0].children[0].key.clone(), nodes[0].children[2].key.clone()]);
    }

    #[test]
    fn counts_visible_rows_and_maps_parents() {
        let nodes = key_outline(&outline());
        assert_eq!(visible_count(&nodes, &HashSet::new()), 5);
        assert_eq!(visible_count(&nodes, &HashSet::from([nodes[0].key.clone()])), 2);
        let parents = parent_keys(&nodes);
        assert_eq!(parents.get(&nodes[0].children[1].key), Some(&Some(nodes[0].key.clone())));
        assert_eq!(parents.get(&nodes[0].key), Some(&None));
    }

    // Not from a spec: keys name the kind as the backend does, and a node keeps the symbol without a second
    // copy of its members.
    #[test]
    fn keys_are_built_from_parent_kind_and_name() {
        let nodes = key_outline(&outline());
        assert_eq!(nodes[0].key, "/class:OrderController");
        assert_eq!(nodes[0].children[0].key, "/class:OrderController/method:list");
        assert_eq!(nodes[0].children[2].key, "/class:OrderController/method:list#1");
        assert!(nodes[0].item.children.is_empty());
        assert_eq!(nodes[0].children.len(), 3);
    }

    // Not from a spec: the best match is the highest-scoring one, the first of them on a tie, and its
    // highlight is recorded.
    #[test]
    fn the_best_match_is_the_highest_scoring_one() {
        let nodes = key_outline(&outline());
        let filtered = filter_outline(&nodes, "l", false);
        assert_eq!(filtered.best.as_deref(), Some(nodes[0].children[0].key.as_str()));
        assert_eq!(filtered.matches[&nodes[0].children[0].key], [0]);
        let helper = filter_outline(&nodes, "help", true);
        assert_eq!(helper.best.as_deref(), Some(nodes[1].key.as_str()));
        assert_eq!(names(&helper.nodes), ["helper"]);
    }
}
