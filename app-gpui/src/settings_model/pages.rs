//! The Settings tree and its search. Nesting: a page whose dotted id extends another page's id
//! goes under it (`languages.servers` under `languages`); otherwise under a folder for its group,
//! otherwise at the top level.

use super::page_spec::PageSpec;
use crate::platform::collate;
use crate::platform::i18n::I18n;
use std::collections::BTreeSet;

/// The order of a page that names none.
const DEFAULT_ORDER: f64 = 100.0;

/// A page of the Settings, as a feature or a plugin contributes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsPage {
    /// The dotted path defines nesting, e.g. `languages.servers`.
    pub id: String,
    pub title: String,
    /// Parent group title for top-level grouping, e.g. "Editor", "Tools".
    pub group: Option<String>,
    /// Where the page sorts among its siblings (lower first).
    pub order: Option<f64>,
    /// Extra words the settings search matches.
    pub keywords: Vec<String>,
    /// The page edits settings a project can override (Global / This project switch).
    pub project_scope: bool,
}

impl SettingsPage {
    pub fn new(id: &str, title: &str) -> Self {
        Self { id: id.to_owned(), title: title.to_owned(), ..Self::default() }
    }

    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_owned());
        self
    }

    pub fn order(mut self, order: impl Into<f64>) -> Self {
        self.order = Some(order.into());
        self
    }

    pub fn keywords(mut self, keywords: &[&str]) -> Self {
        self.keywords = keywords.iter().map(|&keyword| keyword.to_owned()).collect();
        self
    }
}

impl From<&PageSpec> for SettingsPage {
    fn from(spec: &PageSpec) -> Self {
        Self {
            id: spec.id.to_owned(),
            title: spec.title.to_owned(),
            group: spec.group.map(str::to_owned),
            order: Some(spec.order),
            keywords: spec.keywords.iter().map(|&keyword| keyword.to_owned()).collect(),
            project_scope: spec.project_scope,
        }
    }
}

/// A row of the Settings tree.
#[derive(Debug, Clone, PartialEq)]
pub struct PageNode {
    /// The page's id, or [`group_key`] for a group folder.
    pub key: String,
    pub title: String,
    /// `None` for a folder made for a group.
    pub page: Option<SettingsPage>,
    pub children: Vec<PageNode>,
    pub order: f64,
    /// Set by [`filter_page_tree`]: the page itself matches (it is not only kept as an ancestor).
    pub matched: bool,
}

pub fn group_key(group: &str) -> String {
    format!("group:{group}")
}

/// The id of the nearest registered page `id` is nested under.
fn parent_id<'p>(id: &'p str, ids: &BTreeSet<&str>) -> Option<&'p str> {
    let mut prefix = id;
    while let Some(dot) = prefix.rfind('.').filter(|&dot| dot > 0) {
        prefix = &prefix[..dot];
        if ids.contains(prefix) {
            return Some(prefix);
        }
    }
    None
}

/// The node of `page` with the pages nested under it: those whose nearest parent it is.
fn page_node(page: &SettingsPage, pages: &[SettingsPage], ids: &BTreeSet<&str>) -> PageNode {
    let below = pages.iter().filter(|other| parent_id(&other.id, ids) == Some(page.id.as_str()));
    PageNode {
        key: page.id.clone(),
        title: page.title.clone(),
        page: Some(page.clone()),
        children: below.map(|child| page_node(child, pages, ids)).collect(),
        order: page.order.unwrap_or(DEFAULT_ORDER),
        matched: false,
    }
}

fn sort_nodes(nodes: &mut [PageNode]) {
    nodes.sort_by(|a, b| a.order.total_cmp(&b.order).then_with(|| collate::compare(&a.title, &b.title)));
    for node in nodes {
        sort_nodes(&mut node.children);
    }
}

/// The tree of `pages`, each level by order and then title. A group folder takes the lowest order
/// of its pages.
pub fn build_page_tree(pages: &[SettingsPage]) -> Vec<PageNode> {
    let ids: BTreeSet<&str> = pages.iter().map(|page| page.id.as_str()).collect();
    let mut roots: Vec<PageNode> = Vec::new();
    for page in pages.iter().filter(|page| parent_id(&page.id, &ids).is_none()) {
        let top = page_node(page, pages, &ids);
        let Some(group) = page.group.as_deref().filter(|group| !group.is_empty()) else {
            roots.push(top);
            continue;
        };
        let key = group_key(group);
        let at = roots.iter().position(|root| root.page.is_none() && root.key == key).unwrap_or_else(|| {
            let (title, order) = (group.to_owned(), top.order);
            roots.push(PageNode { key, title, page: None, children: Vec::new(), order, matched: false });
            roots.len() - 1
        });
        roots[at].order = roots[at].order.min(top.order);
        roots[at].children.push(top);
    }
    sort_nodes(&mut roots);
    roots
}

/// The nodes depth-first.
pub fn flatten(nodes: &[PageNode]) -> Vec<&PageNode> {
    nodes.iter().flat_map(|node| std::iter::once(node).chain(flatten(&node.children))).collect()
}

/// The titles from the root to `key`; empty when there is no such node.
pub fn breadcrumb(nodes: &[PageNode], key: &str) -> Vec<String> {
    for node in nodes {
        if node.key == key {
            return vec![node.title.clone()];
        }
        let below = breadcrumb(&node.children, key);
        if !below.is_empty() {
            return std::iter::once(node.title.clone()).chain(below).collect();
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Search

/// The lower-cased words of the query.
pub fn query_terms(query: &str) -> Vec<String> {
    query.to_lowercase().split_whitespace().map(str::to_owned).collect()
}

/// Whether every term occurs in at least one of the texts, as written or as translated.
pub fn matches_terms<S: AsRef<str>>(texts: &[S], terms: &[String], i18n: &I18n) -> bool {
    if terms.is_empty() {
        return true;
    }
    // Translated texts are searched alongside the English ones.
    let hay: Vec<String> =
        texts.iter().flat_map(|text| [text.as_ref().to_lowercase(), i18n.tx(text.as_ref()).to_lowercase()]).collect();
    terms.iter().all(|term| hay.iter().any(|text| text.contains(term.as_str())))
}

/// The UTF-16 indices of `text` covered by any term of `query` (for highlighting option labels);
/// `None` when no term occurs.
pub fn highlight_indices(text: &str, query: &str) -> Option<Vec<usize>> {
    let terms = query_terms(query);
    let lower: Vec<u16> = text.to_lowercase().encode_utf16().collect();
    let mut hit: BTreeSet<usize> = BTreeSet::new();
    for term in &terms {
        let term: Vec<u16> = term.encode_utf16().collect();
        if term.is_empty() || term.len() > lower.len() {
            continue;
        }
        for at in 0..=lower.len() - term.len() {
            if lower[at..at + term.len()] == term[..] {
                hit.extend(at..at + term.len());
            }
        }
    }
    (!hit.is_empty()).then(|| hit.into_iter().collect())
}

/// Whether one option label matches the query: any term, so partial queries still light up rows.
pub fn label_matches(text: &str, query: &str, i18n: &I18n) -> bool {
    highlight_indices(text, query).is_some() || highlight_indices(&i18n.tx(text), query).is_some()
}

/// The search texts of a page: its title, group, keywords and the option labels registered for it.
pub fn page_texts<'a>(page: &'a SettingsPage, labels: &'a [String]) -> Vec<&'a str> {
    let own = [page.title.as_str(), page.group.as_deref().unwrap_or_default()];
    own.into_iter().chain(page.keywords.iter().map(String::as_str)).chain(labels.iter().map(String::as_str)).collect()
}

fn filter_below(
    nodes: &[PageNode],
    terms: &[String],
    labels_for: &dyn Fn(&str) -> Vec<String>,
    path: &[&str],
    i18n: &I18n,
) -> Vec<PageNode> {
    let mut out = Vec::new();
    for node in nodes {
        let trail: Vec<&str> = path.iter().copied().chain([node.title.as_str()]).collect();
        let children = filter_below(&node.children, terms, labels_for, &trail, i18n);
        let matched = node.page.as_ref().is_some_and(|page| {
            let labels = labels_for(&page.id);
            let texts: Vec<&str> = trail.iter().copied().chain(page_texts(page, &labels)).collect();
            matches_terms(&texts, terms, i18n)
        });
        if matched || !children.is_empty() {
            out.push(PageNode { children, matched, ..node.clone() });
        }
    }
    out
}

/// The tree narrowed to the pages matching the query; a page also matches when the titles on the
/// way to it do together ("editor font"). Ancestors of matches are kept. `labels_for` gives the
/// option labels of a page id. An empty query keeps everything.
pub fn filter_page_tree(
    nodes: &[PageNode],
    query: &str,
    labels_for: &dyn Fn(&str) -> Vec<String>,
    i18n: &I18n,
) -> Vec<PageNode> {
    let terms = query_terms(query);
    if terms.is_empty() { nodes.to_vec() } else { filter_below(nodes, &terms, labels_for, &[], i18n) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages() -> Vec<SettingsPage> {
        vec![
            SettingsPage::new("appearance", "Appearance").group("Appearance & Behavior").order(0),
            SettingsPage::new("general", "System Settings").group("Appearance & Behavior").order(1),
            SettingsPage::new("keymap", "Keymap").order(10).keywords(&["shortcut"]),
            SettingsPage::new("editor.font", "Font").group("Editor").order(21),
            SettingsPage::new("editor.general", "General").group("Editor").order(20),
            SettingsPage::new("languages", "Languages & Frameworks").order(40),
            SettingsPage::new("languages.servers", "Language Servers").keywords(&["lsp"]),
            SettingsPage::new("plugins", "Plugins"),
        ]
    }

    fn keys(nodes: &[PageNode]) -> Vec<&str> {
        nodes.iter().map(|node| node.key.as_str()).collect()
    }

    // page tree › nests by dotted id, then by group, sorted by order
    #[test]
    fn nests_by_dotted_id_then_by_group_sorted_by_order() {
        let tree = build_page_tree(&pages());
        let titles: Vec<&str> = tree.iter().map(|node| node.title.as_str()).collect();
        assert_eq!(titles, ["Appearance & Behavior", "Keymap", "Editor", "Languages & Frameworks", "Plugins"]);
        assert_eq!(keys(&tree[2].children), ["editor.general", "editor.font"]);
        assert_eq!(keys(&tree[3].children), ["languages.servers"]);
        assert_eq!(breadcrumb(&tree, "editor.font"), ["Editor", "Font"]);
    }

    // page tree › filters by title, keywords, group path and option labels
    #[test]
    fn filters_by_title_keywords_group_path_and_option_labels() {
        let i18n = I18n::default();
        let tree = build_page_tree(&pages());
        let found = |query: &str, labelled: Option<(&str, &str)>| -> Vec<String> {
            let labels_for = |id: &str| match labelled {
                Some((page, label)) if page == id => vec![label.to_owned()],
                _ => Vec::new(),
            };
            let filtered = filter_page_tree(&tree, query, &labels_for, &i18n);
            let listed = flatten(&filtered).into_iter().filter(|node| node.page.is_some());
            listed.filter(|node| query.is_empty() || node.matched).map(|node| node.key.clone()).collect()
        };
        assert_eq!(found("", None).len(), pages().len());
        assert_eq!(found("lsp", None), ["languages.servers"]);
        assert_eq!(found("editor font", None), ["editor.font"]);
        assert_eq!(found("shortcut", None), ["keymap"]);
        assert_eq!(found("soft wrap", Some(("editor.general", "Soft-wrap files"))), ["editor.general"]);
        assert!(found("nothing-matches-this", None).is_empty());
    }

    // page tree › highlights every term occurrence in labels
    #[test]
    fn highlights_every_term_occurrence_in_labels() {
        let i18n = I18n::default();
        assert_eq!(query_terms("  Soft  WRAP "), ["soft", "wrap"]);
        assert_eq!(highlight_indices("Soft-wrap files", "wrap soft"), Some(vec![0, 1, 2, 3, 5, 6, 7, 8]));
        assert_eq!(highlight_indices("Font", ""), None);
        assert_eq!(highlight_indices("Font", "xyz"), None);
        let terms = query_terms("tab indent");
        assert!(matches_terms(&["Tab size", "Indentation"], &terms, &i18n));
        assert!(!matches_terms(&["Tab size"], &terms, &i18n));
    }

    // Not from a spec: a page nests under the nearest page it extends, however deep; a group
    // folder takes the lowest order of its pages; overlapping terms highlight once.
    #[test]
    fn nests_under_the_nearest_page_and_orders_group_folders() {
        let pages = [
            SettingsPage::new("a.b.c.d", "Deep").order(1),
            SettingsPage::new("a", "A").group("G").order(50),
            SettingsPage::new("a.b", "B"),
            SettingsPage::new("z", "Z").group("G").order(5),
            SettingsPage::new("m", "M").order(20),
        ];
        let tree = build_page_tree(&pages);
        assert_eq!(keys(&tree), ["group:G", "m"]);
        assert_eq!((tree[0].order, keys(&tree[0].children)), (5.0, vec!["z", "a"]));
        assert_eq!(breadcrumb(&tree, "a.b.c.d"), ["G", "A", "B", "Deep"]);
        assert!(breadcrumb(&tree, "nope").is_empty());
        assert_eq!(highlight_indices("aaa", "aa a"), Some(vec![0, 1, 2]));
        assert!(label_matches("Tab size", "size", &I18n::default()));
        assert_eq!(page_texts(&pages[1], &["Label".to_owned()]), ["A", "G", "Label"]);
    }
}
