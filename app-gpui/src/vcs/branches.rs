//! Branches popup and Git log Branches pane models: grouping, ordering and filtering of branches and
//! tags, for one repository and for a project with several.
//!
//! Highlighted characters are byte offsets into the text they highlight, as
//! [`crate::ui::matching`] reports them.

use super::changes::{compare_text, root_of};
use crate::platform::i18n::{I18n, Param};
use crate::ui::matching::fuzzy_match;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// A branch as the backend lists it, with what the branch models read of it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pub name: String,
    pub remote: bool,
    pub current: bool,
}

/// Highlighted characters of a shown text; `None` when there is no query.
pub type Matches = Option<Vec<usize>>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PopupAction {
    pub id: String,
    pub label: String,
    pub icon: Option<String>,
    /// Argument the action is executed with.
    pub arg: Option<serde_json::Value>,
    /// A separator goes between actions of different groups (Update / Commit / Push | New Branch /
    /// Checkout…).
    pub group: Option<String>,
    /// A toggle: shown with a check mark when on.
    pub checked: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BranchGroup {
    Recent,
    Local,
    Remote,
}

impl BranchGroup {
    /// The id row keys are built from.
    pub fn id(self) -> &'static str {
        match self {
            Self::Recent => "recent",
            Self::Local => "local",
            Self::Remote => "remote",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum BranchRow {
    /// Section header; an empty label draws a separator.
    Section {
        key: String,
        label: String,
    },
    Action {
        key: String,
        action: PopupAction,
        matches: Matches,
    },
    /// Collapsible node: Recent / Local / Remote / Tags (depth 0) or a prefix folder (`feat`).
    Group {
        key: String,
        label: String,
        expanded: bool,
        depth: usize,
    },
    Branch {
        key: String,
        branch: Branch,
        group: BranchGroup,
        /// Shown name: the last segment inside a prefix folder.
        label: String,
        depth: usize,
        matches: Matches,
        favorite: bool,
    },
    Tag {
        key: String,
        tag: String,
        depth: usize,
        matches: Matches,
    },
}

/// Branches popup options (the popup's gear menu).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTreeOptions {
    /// Groups and folders the user expanded (true) or collapsed (false); the others use their defaults.
    pub expanded: HashMap<String, bool>,
    /// `feat/x` under a `feat` folder; remote branches under their remote.
    pub group_by_prefix: bool,
    pub show_recent: bool,
    pub show_tags: bool,
}

impl Default for BranchTreeOptions {
    fn default() -> Self {
        Self { expanded: HashMap::new(), group_by_prefix: true, show_recent: true, show_tags: true }
    }
}

/// Copy of `expanded` with `key` set to `open`.
pub fn with_expanded(expanded: &HashMap<String, bool>, key: &str, open: bool) -> HashMap<String, bool> {
    let mut out = expanded.clone();
    out.insert(key.to_owned(), open);
    out
}

#[derive(Clone, Debug)]
pub struct PrefixItem<'a, T> {
    pub item: &'a T,
    /// Full name, split into folders at `/`.
    pub name: &'a str,
    /// Highlighted characters of `name`.
    pub matches: Matches,
    /// Listed before the folders of its level (current branch, favorites).
    pub pinned: bool,
}

/// A row of a prefix tree: a folder, or an item shown by the rest of its name.
#[derive(Clone, Debug, PartialEq)]
pub enum PrefixRow<'a, T> {
    Folder { key: String, label: String, depth: usize, expanded: bool },
    Leaf { item: &'a T, label: String, depth: usize, matches: Matches },
}

/// Decides whether a prefix folder shows its items, given its key, its prefix and the items below it.
pub type FolderOpen<'f, 'a, T> = &'f dyn Fn(&str, &str, &[&PrefixItem<'a, T>]) -> bool;

/// An item on its way down the prefix tree: what is left of its name, and where that starts.
struct PrefixEntry<'e, 'a, T> {
    it: &'e PrefixItem<'a, T>,
    rest: &'a str,
    offset: usize,
}

fn prefix_level<'a, T>(
    list: Vec<PrefixEntry<'_, 'a, T>>,
    path: &str,
    depth: usize,
    key_prefix: &str,
    is_open: FolderOpen<'_, 'a, T>,
) -> Vec<PrefixRow<'a, T>> {
    let mut folders: Vec<(&'a str, Vec<PrefixEntry<'_, 'a, T>>)> = Vec::new();
    let mut leaves: Vec<PrefixEntry<'_, 'a, T>> = Vec::new();
    for entry in list {
        match entry.rest.find('/') {
            Some(slash) if slash > 0 && slash < entry.rest.len() - 1 => {
                let segment = &entry.rest[..slash];
                let inner =
                    PrefixEntry { it: entry.it, rest: &entry.rest[slash + 1..], offset: entry.offset + slash + 1 };
                match folders.iter_mut().find(|(known, _)| *known == segment) {
                    Some((_, entries)) => entries.push(inner),
                    None => folders.push((segment, vec![inner])),
                }
            }
            _ => leaves.push(entry),
        }
    }
    let leaf = |entry: &PrefixEntry<'_, 'a, T>| PrefixRow::Leaf {
        item: entry.it.item,
        label: entry.rest.to_owned(),
        depth,
        // Highlights are of the full name: shift them onto the shown part.
        matches: entry.it.matches.as_ref().map(|m| m.iter().filter_map(|at| at.checked_sub(entry.offset)).collect()),
    };
    let mut out: Vec<PrefixRow<'a, T>> = leaves.iter().filter(|entry| entry.it.pinned).map(leaf).collect();
    folders.sort_by(|a, b| compare_text(a.0, b.0, false));
    for (segment, inner) in folders {
        let folder_path = format!("{path}{segment}");
        let key = format!("{key_prefix}{folder_path}");
        let items: Vec<&PrefixItem<'a, T>> = inner.iter().map(|entry| entry.it).collect();
        let expanded = is_open(&key, &folder_path, &items);
        out.push(PrefixRow::Folder { key, label: segment.to_owned(), depth, expanded });
        if expanded {
            out.extend(prefix_level(inner, &format!("{folder_path}/"), depth + 1, key_prefix, is_open));
        }
    }
    out.extend(leaves.iter().filter(|entry| !entry.it.pinned).map(leaf));
    out
}

/// Rows of `items` grouped by name prefix (`feat/x` → feat › x), as in IDEA's Branches popup: at each
/// level pinned items, then folders by name, then the other items in their order. `is_open` decides
/// whether a folder (its key is `key_prefix` and its prefix) shows its items. Not `grouped`: the items
/// as they are, by full name.
pub fn prefix_tree_rows<'a, T>(
    items: &[PrefixItem<'a, T>],
    key_prefix: &str,
    depth: usize,
    grouped: bool,
    is_open: FolderOpen<'_, 'a, T>,
) -> Vec<PrefixRow<'a, T>> {
    if !grouped {
        let leaf = |it: &PrefixItem<'a, T>| PrefixRow::Leaf {
            item: it.item,
            label: it.name.to_owned(),
            depth,
            matches: it.matches.clone(),
        };
        return items.iter().map(leaf).collect();
    }
    let entries = items.iter().map(|it| PrefixEntry { it, rest: it.name, offset: 0 }).collect();
    prefix_level(entries, "", depth, key_prefix, is_open)
}

/// Default expansion of a prefix folder: in Recent, a remote (`origin`), or holding the current branch.
pub fn default_folder_open(group: BranchGroup, path: &str, has_current: bool) -> bool {
    group == BranchGroup::Recent || (group == BranchGroup::Remote && !path.contains('/')) || has_current
}

/// How `text` matches the trimmed `query`: `None` when it does not, else what to highlight (nothing
/// without a query).
fn query_match(query: &str, text: &str) -> Option<Matches> {
    if query.is_empty() { Some(None) } else { fuzzy_match(text, query).map(Some) }
}

/// An action of a branches popup, or the separator before it.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionRow {
    Section { key: String, label: String },
    Action { key: String, action: PopupAction, matches: Matches },
}

impl From<ActionRow> for BranchRow {
    fn from(row: ActionRow) -> Self {
        match row {
            ActionRow::Section { key, label } => Self::Section { key, label },
            ActionRow::Action { key, action, matches } => Self::Action { key, action, matches },
        }
    }
}

impl From<ActionRow> for RepoRow {
    fn from(row: ActionRow) -> Self {
        match row {
            ActionRow::Section { key, label } => Self::Section { key, label },
            ActionRow::Action { key, action, matches } => Self::Action { key, action, matches },
        }
    }
}

/// Rows of the actions whose label, in the UI language, matches the trimmed `query` (all of them
/// without one); with `separators`, a separator between actions of different groups.
pub fn action_rows(actions: &[PopupAction], query: &str, separators: bool, i18n: &I18n) -> Vec<ActionRow> {
    let mut rows = Vec::new();
    let mut last: Option<&str> = None;
    for action in actions {
        let Some(matches) = query_match(query, &i18n.tx(&action.label)) else { continue };
        if separators && !rows.is_empty() && action.group.as_deref() != last {
            rows.push(ActionRow::Section { key: format!("separator:action:{}", action.id), label: String::new() });
        }
        last = action.group.as_deref();
        rows.push(ActionRow::Action { key: format!("action:{}", action.id), action: action.clone(), matches });
    }
    rows
}

/// Current branch first, then favorites, then the rest; by name within each.
fn by_rank(a: &Branch, b: &Branch, favorites: Option<&[String]>) -> Ordering {
    let rank = |branch: &Branch| {
        if branch.current {
            0u8
        } else if is_favorite_branch(&branch.name, branch.remote, favorites) {
            1
        } else {
            2
        }
    };
    rank(a).cmp(&rank(b)).then_with(|| compare_text(&a.name, &b.name, false))
}

pub fn sort_local<'a>(branches: &'a [Branch], favorites: Option<&[String]>) -> Vec<&'a Branch> {
    let mut out: Vec<&Branch> = branches.iter().filter(|b| !b.remote).collect();
    out.sort_by(|a, b| by_rank(a, b, favorites));
    out
}

pub fn sort_remote<'a>(branches: &'a [Branch], favorites: Option<&[String]>) -> Vec<&'a Branch> {
    let mut out: Vec<&Branch> = branches.iter().filter(|b| b.remote && !b.name.ends_with("/HEAD")).collect();
    out.sort_by(|a, b| by_rank(a, b, favorites));
    out
}

/// `origin/feature/x` → `feature/x`.
pub fn short_remote_name(name: &str) -> &str {
    name.split_once('/').map_or(name, |(_, rest)| rest)
}

pub const MAX_RECENT: usize = 5;

/// Recently checked-out local branches other than the current one, newest first.
pub fn recent_local<'a>(local: &[&'a Branch], recent: &[String]) -> Vec<&'a Branch> {
    let by_name: HashMap<&str, &'a Branch> = local.iter().map(|&b| (b.name.as_str(), b)).collect();
    let known = recent.iter().filter_map(|name| by_name.get(name.as_str()).copied());
    known.filter(|b| !b.current).take(MAX_RECENT).collect()
}

/// Tags, newest version first.
pub fn sort_tags(tags: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = tags.iter().map(String::as_str).collect();
    out.sort_by(|a, b| compare_text(b, a, true));
    out
}

/// What decides which rows of a branches popup are shown, and which of its nodes are open.
struct RowContext<'a> {
    /// The query, trimmed.
    query: &'a str,
    favorites: Option<&'a [String]>,
    expanded: &'a HashMap<String, bool>,
    group_by_prefix: bool,
}

impl RowContext<'_> {
    /// Whether the node `key` is open: always while searching, else the user's choice or the default.
    fn open(&self, key: &str, by_default: bool) -> bool {
        !self.query.is_empty() || self.expanded.get(key).copied().unwrap_or(by_default)
    }

    fn folder_open(&self, key: &str, group: BranchGroup, path: &str, has_current: bool) -> bool {
        self.open(key, default_folder_open(group, path, has_current))
    }

    fn matches(&self, text: &str) -> Option<Matches> {
        query_match(self.query, text)
    }

    fn favorite(&self, branch: &Branch) -> bool {
        is_favorite_branch(&branch.name, branch.remote, self.favorites)
    }

    /// The branches of `list` that match the query, as items of a prefix tree.
    fn branch_items<'b>(&self, list: &[&'b Branch]) -> Vec<PrefixItem<'b, Branch>> {
        let item = |&branch: &&'b Branch| {
            let matches = self.matches(&branch.name)?;
            let pinned = branch.current || self.favorite(branch);
            Some(PrefixItem { item: branch, name: &branch.name, matches, pinned })
        };
        list.iter().filter_map(item).collect()
    }
}

/// What the Branches popup of a single repository shows.
#[derive(Clone, Debug, Default)]
pub struct BranchRowsInput<'a> {
    pub branches: &'a [Branch],
    /// Recently checked-out branches, newest first.
    pub recent: &'a [String],
    pub query: &'a str,
    pub actions: &'a [PopupAction],
    /// The user's favorite branches; `None` until they edit them.
    pub favorites: Option<&'a [String]>,
    pub tags: &'a [String],
    pub options: BranchTreeOptions,
}

/// A collapsible group of branches with, when it is open, its branches; nothing when no branch matches.
fn branch_group_rows(
    ctx: &RowContext,
    group: BranchGroup,
    label: &str,
    list: &[&Branch],
    by_default: bool,
) -> Vec<BranchRow> {
    let items = ctx.branch_items(list);
    if items.is_empty() {
        return Vec::new();
    }
    let key = format!("group:{}", group.id());
    let expanded = ctx.open(&key, by_default);
    let mut rows = vec![BranchRow::Group { key, label: label.to_owned(), expanded, depth: 0 }];
    if !expanded {
        return rows;
    }
    let is_open = |key: &str, path: &str, inner: &[&PrefixItem<Branch>]| {
        ctx.folder_open(key, group, path, inner.iter().any(|i| i.item.current))
    };
    let children = prefix_tree_rows(&items, &format!("folder:{}:", group.id()), 1, ctx.group_by_prefix, &is_open);
    rows.extend(children.into_iter().map(|row| match row {
        PrefixRow::Folder { key, label, depth, expanded } => BranchRow::Group { key, label, expanded, depth },
        PrefixRow::Leaf { item, label, depth, matches } => BranchRow::Branch {
            key: format!("{}:{}", group.id(), item.name),
            branch: item.clone(),
            group,
            label,
            depth,
            matches,
            favorite: ctx.favorite(item),
        },
    }));
    rows
}

/// Rows for the Branches popup: actions (separated by group), then the collapsible Recent (recently
/// checked-out local branches other than the current one), Local, Remote and Tags groups. Recent is
/// open by default, Local only without Recent, Remote and Tags closed; `options.expanded` holds the
/// user's choices. Names are grouped by prefix into folders unless `group_by_prefix` is off. With a
/// query: matching actions, branches and tags with every group open; the Recent group is dropped so
/// each branch appears once. Favorites (★) come right after the current branch.
pub fn build_branch_rows(input: &BranchRowsInput, i18n: &I18n) -> Vec<BranchRow> {
    let query = input.query.trim();
    let options = &input.options;
    let ctx = RowContext {
        query,
        favorites: input.favorites,
        expanded: &options.expanded,
        group_by_prefix: options.group_by_prefix,
    };
    let actions = action_rows(input.actions, query, query.is_empty(), i18n);
    let mut rows: Vec<BranchRow> = actions.into_iter().map(BranchRow::from).collect();

    let local = sort_local(input.branches, input.favorites);
    let recent = if query.is_empty() && options.show_recent { recent_local(&local, input.recent) } else { Vec::new() };
    let mut tree = branch_group_rows(&ctx, BranchGroup::Recent, "Recent", &recent, true);
    tree.extend(branch_group_rows(&ctx, BranchGroup::Local, "Local", &local, recent.is_empty()));
    let remote = sort_remote(input.branches, input.favorites);
    tree.extend(branch_group_rows(&ctx, BranchGroup::Remote, "Remote", &remote, false));
    if options.show_tags {
        let tag_row = |tag: &str| {
            let matches = ctx.matches(tag)?;
            Some(BranchRow::Tag { key: format!("tag:{tag}"), tag: tag.to_owned(), depth: 1, matches })
        };
        let tag_rows: Vec<BranchRow> = sort_tags(input.tags).into_iter().filter_map(tag_row).collect();
        if !tag_rows.is_empty() {
            let expanded = ctx.open("group:tags", false);
            tree.push(BranchRow::Group { key: "group:tags".to_owned(), label: "Tags".to_owned(), expanded, depth: 0 });
            if expanded {
                tree.extend(tag_rows);
            }
        }
    }
    if !tree.is_empty() && !rows.is_empty() {
        rows.push(BranchRow::Section { key: "separator:tree".to_owned(), label: String::new() });
    }
    rows.extend(tree);
    rows
}

/// How many recently checked-out branches are remembered.
pub const RECENT_LIMIT: usize = 20;

/// Moves `name` to the front of a recent-branches list of at most `max` names.
pub fn push_recent(list: &[String], name: &str, max: usize) -> Vec<String> {
    let others = list.iter().filter(|known| known.as_str() != name).cloned();
    std::iter::once(name.to_owned()).chain(others).take(max).collect()
}

const DEFAULT_FAVORITES: [&str; 2] = ["main", "master"];

/// Favorite branches (★): the user's list, or `main` / `master` (local and remote) until they edit it.
pub fn is_favorite_branch(name: &str, remote: bool, stored: Option<&[String]>) -> bool {
    match stored {
        Some(stored) => stored.iter().any(|favorite| favorite == name),
        None => DEFAULT_FAVORITES.contains(&if remote { short_remote_name(name) } else { name }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BranchNodeKind {
    Repository,
    Head,
    Group,
    Remote,
    Folder,
    Branch,
    Tag,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTreeNode {
    pub key: String,
    pub label: String,
    pub repo: Option<String>,
    pub kind: BranchNodeKind,
    /// Branch name for `Branch` / `Head` (`HEAD`), tag name for `Tag`.
    pub ref_name: Option<String>,
    pub branch: Option<Branch>,
    pub favorite: bool,
    /// Nodes below an expandable node; `None` for a branch, a tag and HEAD.
    pub children: Option<Vec<BranchTreeNode>>,
}

impl BranchTreeNode {
    fn new(key: String, label: &str, kind: BranchNodeKind) -> Self {
        Self {
            key,
            label: label.to_owned(),
            repo: None,
            kind,
            ref_name: None,
            branch: None,
            favorite: false,
            children: None,
        }
    }

    fn head() -> Self {
        let mut head = Self::new("head".to_owned(), "HEAD (Current Branch)", BranchNodeKind::Head);
        head.ref_name = Some("HEAD".to_owned());
        head
    }

    fn parent(key: String, label: &str, kind: BranchNodeKind, children: Vec<BranchTreeNode>) -> Self {
        let mut node = Self::new(key, label, kind);
        node.children = Some(children);
        node
    }

    /// This node above other children.
    fn with_children(&self, children: Option<Vec<BranchTreeNode>>) -> Self {
        Self {
            key: self.key.clone(),
            label: self.label.clone(),
            repo: self.repo.clone(),
            kind: self.kind,
            ref_name: self.ref_name.clone(),
            branch: self.branch.clone(),
            favorite: self.favorite,
            children,
        }
    }
}

/// A branch and the part of its name that is still to be placed in the tree.
type Unplaced<'a> = (&'a Branch, &'a str);

fn by_label(a: &BranchTreeNode, b: &BranchTreeNode) -> Ordering {
    compare_text(&a.label, &b.label, false)
}

/// One level of the Branches pane: the branches whose remaining name has no folder, and a folder for
/// each first segment of the others.
fn branch_level(list: Vec<Unplaced>, key_prefix: &str, favorites: Option<&[String]>) -> Vec<BranchTreeNode> {
    let mut folders: Vec<(&str, Vec<Unplaced>)> = Vec::new();
    let mut leaves: Vec<BranchTreeNode> = Vec::new();
    for (branch, rest) in list {
        match rest.find('/') {
            Some(slash) if slash > 0 && slash < rest.len() - 1 => {
                let (segment, inner) = (&rest[..slash], (branch, &rest[slash + 1..]));
                match folders.iter_mut().find(|(known, _)| *known == segment) {
                    Some((_, items)) => items.push(inner),
                    None => folders.push((segment, vec![inner])),
                }
            }
            _ => {
                let mut leaf = BranchTreeNode::new(format!("b:{}", branch.name), rest, BranchNodeKind::Branch);
                leaf.ref_name = Some(branch.name.clone());
                leaf.branch = Some(branch.clone());
                leaf.favorite = is_favorite_branch(&branch.name, branch.remote, favorites);
                leaves.push(leaf);
            }
        }
    }
    let folder = |(segment, items): (&str, Vec<Unplaced>)| {
        let key = format!("{key_prefix}/{segment}");
        let children = branch_level(items, &key, favorites);
        BranchTreeNode::parent(key, segment, BranchNodeKind::Folder, children)
    };
    let mut folder_nodes: Vec<BranchTreeNode> = folders.into_iter().map(folder).collect();
    folder_nodes.sort_by(by_label);
    // IDEA: current branch, favorites, folders, the rest.
    let rank = |node: &BranchTreeNode| {
        if node.branch.as_ref().is_some_and(|branch| branch.current) {
            0u8
        } else if node.favorite {
            1
        } else {
            2
        }
    };
    let (mut top, mut rest): (Vec<BranchTreeNode>, Vec<BranchTreeNode>) =
        leaves.into_iter().partition(|node| rank(node) < 2);
    top.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| by_label(a, b)));
    rest.sort_by(by_label);
    top.into_iter().chain(folder_nodes).chain(rest).collect()
}

/// Git Log Branches pane: HEAD, Local, Remote › <remote> and Tags. Names with `/` are grouped into
/// folders (`feat/x` → feat › x); at each level the current branch and favorites come first, then
/// folders, then the other branches. Tags: newest version first.
pub fn build_branch_tree(branches: &[Branch], favorites: Option<&[String]>, tags: &[String]) -> Vec<BranchTreeNode> {
    let local: Vec<Unplaced> = branches.iter().filter(|b| !b.remote).map(|b| (b, b.name.as_str())).collect();
    let mut by_remote: Vec<(&str, Vec<&Branch>)> = Vec::new();
    for branch in branches.iter().filter(|b| b.remote && !b.name.ends_with("/HEAD")) {
        let remote = match branch.name.find('/') {
            Some(slash) if slash > 0 => &branch.name[..slash],
            _ => branch.name.as_str(),
        };
        match by_remote.iter_mut().find(|(known, _)| *known == remote) {
            Some((_, list)) => list.push(branch),
            None => by_remote.push((remote, vec![branch])),
        }
    }
    let mut out = vec![BranchTreeNode::head()];
    if !local.is_empty() {
        let children = branch_level(local, "f:local", favorites);
        out.push(BranchTreeNode::parent("g:local".to_owned(), "Local", BranchNodeKind::Group, children));
    }
    if !by_remote.is_empty() {
        let remote_node = |(remote, list): (&str, Vec<&Branch>)| {
            let below = list.into_iter().map(|b| (b, b.name.get(remote.len() + 1..).unwrap_or_default())).collect();
            let children = branch_level(below, &format!("f:r:{remote}"), favorites);
            BranchTreeNode::parent(format!("r:{remote}"), remote, BranchNodeKind::Remote, children)
        };
        let children = by_remote.into_iter().map(remote_node).collect();
        out.push(BranchTreeNode::parent("g:remote".to_owned(), "Remote", BranchNodeKind::Group, children));
    }
    if !tags.is_empty() {
        let tag_node = |tag: &str| {
            let mut node = BranchTreeNode::new(format!("t:{tag}"), tag, BranchNodeKind::Tag);
            node.ref_name = Some(tag.to_owned());
            node
        };
        let children = sort_tags(tags).into_iter().map(tag_node).collect();
        out.push(BranchTreeNode::parent("g:tags".to_owned(), "Tags", BranchNodeKind::Group, children));
    }
    out
}

/// A repository of the project with its branches and tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepositoryBranches {
    pub root: String,
    pub name: String,
    pub branches: Vec<Branch>,
    pub tags: Vec<String>,
}

/// `nodes` as the nodes of repository `repo`: marked with it, and with keys that tell them from the
/// same nodes of another repository.
fn scoped(nodes: &[BranchTreeNode], repo: &str) -> Vec<BranchTreeNode> {
    let scope = |node: &BranchTreeNode| {
        let mut scoped_node = node.with_children(node.children.as_deref().map(|children| scoped(children, repo)));
        scoped_node.key = format!("{repo}:{}", node.key);
        scoped_node.repo = Some(repo.to_owned());
        scoped_node
    };
    nodes.iter().map(scope).collect()
}

/// Local / remote / tag groups contain a subtree for each registered repository.
pub fn build_repository_branch_tree(
    repositories: &[RepositoryBranches],
    favorites: Option<&[String]>,
) -> Vec<BranchTreeNode> {
    let trees: Vec<(&RepositoryBranches, Vec<BranchTreeNode>)> =
        repositories.iter().map(|r| (r, build_branch_tree(&r.branches, favorites, &r.tags))).collect();
    let mut out = vec![BranchTreeNode::head()];
    for (key, label) in [("g:local", "Local"), ("g:remote", "Remote"), ("g:tags", "Tags")] {
        let repository = |(repo, nodes): &(&RepositoryBranches, Vec<BranchTreeNode>)| {
            let group = nodes.iter().find(|node| node.key == key).and_then(|node| node.children.as_deref());
            let children = scoped(group.unwrap_or_default(), &repo.root);
            let key = format!("{key}:{}", repo.root);
            let mut node = BranchTreeNode::parent(key, &repo.name, BranchNodeKind::Repository, children);
            node.repo = Some(repo.root.clone());
            node
        };
        let children = trees.iter().map(repository).collect();
        out.push(BranchTreeNode::parent(key.to_owned(), label, BranchNodeKind::Group, children));
    }
    out
}

fn folder_keys_to(nodes: &[BranchTreeNode], name: &str) -> Option<Vec<String>> {
    for node in nodes {
        if node.kind == BranchNodeKind::Branch && node.ref_name.as_deref() == Some(name) {
            return Some(Vec::new());
        }
        if let Some(inner) = node.children.as_deref().and_then(|children| folder_keys_to(children, name)) {
            return Some(std::iter::once(node.key.clone()).chain(inner).collect());
        }
    }
    None
}

/// Keys of the nodes on the way to branch `name` (to reveal it).
pub fn branch_folder_keys(nodes: &[BranchTreeNode], name: &str) -> Vec<String> {
    folder_keys_to(nodes, name).unwrap_or_default()
}

/// Branches pane search: keeps branches and tags whose full name contains `query` (case-insensitive)
/// and their folders.
pub fn filter_branch_tree(nodes: &[BranchTreeNode], query: &str) -> Vec<BranchTreeNode> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return nodes.to_vec();
    }
    let kept = |node: &BranchTreeNode| match node.kind {
        BranchNodeKind::Head => None,
        BranchNodeKind::Branch | BranchNodeKind::Tag => {
            let named = node.ref_name.as_ref().is_some_and(|name| name.to_lowercase().contains(&query));
            named.then(|| node.clone())
        }
        _ => {
            let children = filter_branch_tree(node.children.as_deref().unwrap_or_default(), &query);
            (!children.is_empty()).then(|| node.with_children(Some(children)))
        }
    };
    nodes.iter().filter_map(kept).collect()
}

/// Keys of every expandable node.
pub fn branch_group_keys(nodes: &[BranchTreeNode]) -> Vec<String> {
    let mut out = Vec::new();
    for node in nodes {
        if let Some(children) = &node.children {
            out.push(node.key.clone());
            out.extend(branch_group_keys(children));
        }
    }
    out
}

/// A repository of a project with several.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoEntry {
    pub root: String,
    pub name: String,
    /// Current branch; `None` when HEAD is detached.
    pub head: Option<String>,
    pub head_commit: Option<String>,
}

/// Number of project colour tokens repositories are coloured with.
pub const REPO_COLORS: usize = 9;

/// Colour slot (1-based) of the repository at `index`: distinct for up to [`REPO_COLORS`] repositories.
pub fn repo_color_index(index: usize) -> usize {
    (index % REPO_COLORS) + 1
}

/// Display name of a repository's HEAD: its branch, else the short commit.
pub fn repo_head_label(repo: &RepoEntry, i18n: &I18n) -> String {
    let given = |value: &Option<String>| value.clone().filter(|v| !v.is_empty());
    given(&repo.head)
        .or_else(|| given(&repo.head_commit).map(|commit| commit.chars().take(8).collect()))
        .unwrap_or_else(|| i18n.tx("No commits"))
}

/// The current repository, shown by the branch widget and the lower part of the Branches popup: the
/// one holding the active editor's file, else the selected one, else the first.
pub fn current_repo_for<'a>(active_path: Option<&str>, roots: &'a [String], selected: Option<&str>) -> Option<&'a str> {
    let of_file = active_path.filter(|path| !path.is_empty()).and_then(|path| root_of(path, roots));
    let chosen = selected.filter(|s| !s.is_empty()).and_then(|s| roots.iter().find(|root| root.as_str() == s));
    of_file.or(chosen.map(String::as_str)).or(roots.first().map(String::as_str))
}

/// Whether the branch widget names the repository besides the branch: only with several repositories
/// that are not all on the same branch.
pub fn widget_shows_repo(repos: &[RepoEntry]) -> bool {
    let head = |repo: &RepoEntry| repo.head.clone().filter(|head| !head.is_empty());
    match repos.first().map(head) {
        Some(first) if repos.len() >= 2 => first.is_none() || repos.iter().any(|repo| repo.head != first),
        _ => false,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommonBranch {
    pub name: String,
    pub remote: bool,
    /// Checked out in every repository.
    pub current: bool,
    pub favorite: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommonBranches {
    pub local: Vec<CommonBranch>,
    pub remote: Vec<CommonBranch>,
}

/// Branches present in every repository (by name), local and remote, sorted like the single-repository
/// lists.
pub fn common_branches(branches_by_repo: &[&[Branch]], favorites: Option<&[String]>) -> CommonBranches {
    let Some((first, rest)) = branches_by_repo.split_first() else { return CommonBranches::default() };
    let others: Vec<HashMap<(bool, &str), &Branch>> =
        rest.iter().map(|list| list.iter().map(|b| ((b.remote, b.name.as_str()), b)).collect()).collect();
    let pick = |sorted: Vec<&Branch>| {
        let common = |branch: &Branch| {
            let everywhere: Option<Vec<&Branch>> =
                others.iter().map(|known| known.get(&(branch.remote, branch.name.as_str())).copied()).collect();
            let everywhere = everywhere?;
            Some(CommonBranch {
                name: branch.name.clone(),
                remote: branch.remote,
                current: branch.current && everywhere.iter().all(|other| other.current),
                favorite: is_favorite_branch(&branch.name, branch.remote, favorites),
            })
        };
        let mut out: Vec<CommonBranch> = sorted.into_iter().filter_map(common).collect();
        // Checked out everywhere first, then favorites.
        out.sort_by_key(|branch| (!branch.current, !branch.favorite));
        out
    };
    CommonBranches { local: pick(sort_local(first, favorites)), remote: pick(sort_remote(first, favorites)) }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RepoRow {
    /// Section header; an empty label draws a separator.
    Section {
        key: String,
        label: String,
    },
    Action {
        key: String,
        action: PopupAction,
        matches: Matches,
    },
    Repo {
        key: String,
        repo: RepoEntry,
        color: usize,
        matches: Matches,
    },
    /// Collapsible group (common branches, or one of the current repository's lists) or prefix / remote
    /// folder.
    Group {
        key: String,
        label: String,
        expanded: bool,
        depth: usize,
    },
    Common {
        key: String,
        branch: CommonBranch,
        /// Shown name: the last segment inside a folder.
        label: String,
        depth: usize,
        matches: Matches,
    },
    /// A branch of one repository.
    Branch {
        key: String,
        repo: RepoEntry,
        branch: Branch,
        group: BranchGroup,
        label: String,
        depth: usize,
        matches: Matches,
        favorite: bool,
    },
    Tag {
        key: String,
        repo: RepoEntry,
        tag: String,
        depth: usize,
        matches: Matches,
    },
}

/// What the Branches popup of a project with several repositories shows.
#[derive(Clone, Debug)]
pub struct MultiRepoInput<'a> {
    pub repos: &'a [RepoEntry],
    /// Root → its branches (missing while loading).
    pub branches: &'a HashMap<String, Vec<Branch>>,
    pub query: &'a str,
    pub actions: &'a [PopupAction],
    /// Groups / folders the user expanded or collapsed (`common:local`, `repo:remote`,
    /// `repo:folder:local:feat`…).
    pub expanded: &'a HashMap<String, bool>,
    /// The user's favorite branches; `None` until they edit them.
    pub favorites: Option<&'a [String]>,
    /// The current repository (of the active file), whose lists are shown when the repositories'
    /// branches differ.
    pub current: Option<&'a str>,
    /// Recently checked-out branches (newest first).
    pub recent: &'a [String],
    /// Tags of the current repository.
    pub tags: &'a [String],
    /// `feat/x` under a `feat` folder; remote branches under their remote.
    pub group_by_prefix: bool,
    pub show_recent: bool,
    pub show_tags: bool,
}

/// Appends a group row followed (when expanded) by its children; nothing when it has none.
fn push_repo_group(
    ctx: &RowContext,
    out: &mut Vec<RepoRow>,
    key: &str,
    label: String,
    children: Vec<RepoRow>,
    by_default: bool,
) {
    if children.is_empty() {
        return;
    }
    let expanded = ctx.open(key, by_default);
    out.push(RepoRow::Group { key: key.to_owned(), label, expanded, depth: 0 });
    if expanded {
        out.extend(children);
    }
}

fn repo_folder_row<T>(row: PrefixRow<T>, leaf: impl FnOnce(&T, String, usize, Matches) -> RepoRow) -> RepoRow {
    match row {
        PrefixRow::Folder { key, label, depth, expanded } => RepoRow::Group { key, label, expanded, depth },
        PrefixRow::Leaf { item, label, depth, matches } => leaf(item, label, depth, matches),
    }
}

/// Rows of the branches of `repo` that match the query, in folders by prefix when `grouped`.
fn repo_branch_rows(
    ctx: &RowContext,
    repo: &RepoEntry,
    list: &[&Branch],
    kind: BranchGroup,
    depth: usize,
    grouped: bool,
) -> Vec<RepoRow> {
    let items = ctx.branch_items(list);
    let is_open = |key: &str, path: &str, inner: &[&PrefixItem<Branch>]| {
        ctx.folder_open(key, kind, path, inner.iter().any(|i| i.item.current))
    };
    let leaf = |branch: &Branch, label: String, depth: usize, matches: Matches| RepoRow::Branch {
        key: format!("branch:{}:{}:{}", repo.root, kind.id(), branch.name),
        repo: repo.clone(),
        branch: branch.clone(),
        group: kind,
        label,
        depth,
        matches,
        favorite: ctx.favorite(branch),
    };
    let rows = prefix_tree_rows(&items, &format!("repo:folder:{}:", kind.id()), depth, grouped, &is_open);
    rows.into_iter().map(|row| repo_folder_row(row, leaf)).collect()
}

/// Rows of the common branches that match the query.
fn common_rows<'c>(ctx: &RowContext, list: &'c [CommonBranch], kind: BranchGroup) -> Vec<RepoRow> {
    let item = |branch: &'c CommonBranch| {
        let matches = ctx.matches(&branch.name)?;
        Some(PrefixItem { item: branch, name: &branch.name, matches, pinned: branch.current || branch.favorite })
    };
    let items: Vec<PrefixItem<CommonBranch>> = list.iter().filter_map(item).collect();
    let is_open = |key: &str, path: &str, inner: &[&PrefixItem<CommonBranch>]| {
        ctx.folder_open(key, kind, path, inner.iter().any(|i| i.item.current))
    };
    let leaf = |branch: &CommonBranch, label: String, depth: usize, matches: Matches| RepoRow::Common {
        key: format!("common:{}", branch.name),
        branch: branch.clone(),
        label,
        depth,
        matches,
    };
    let rows = prefix_tree_rows(&items, &format!("common:folder:{}:", kind.id()), 1, ctx.group_by_prefix, &is_open);
    rows.into_iter().map(|row| repo_folder_row(row, leaf)).collect()
}

/// The branches of `sorted` that are not among the `shown` ones (by remote flag and name).
fn not_shown<'b>(sorted: Vec<&'b Branch>, shown: &HashSet<(bool, &str)>) -> Vec<&'b Branch> {
    sorted.into_iter().filter(|b| !shown.contains(&(b.remote, b.name.as_str()))).collect()
}

/// Rows of the Branches popup with several repositories: actions; the repositories, each with its
/// current branch and opening its own branches; then, when they are all on the same branch, the
/// branches common to all of them (acting on all of them), otherwise the current repository's recent,
/// local and remote branches and tags, in collapsible groups (Recent open, Local open only without
/// Recent). A query filters actions, repositories (by name) and branches, expands every group, and
/// adds the other repositories' matching branches.
pub fn build_multi_repo_rows(input: &MultiRepoInput, i18n: &mut I18n) -> Vec<RepoRow> {
    let query = input.query.trim();
    let (repos, favorites) = (input.repos, input.favorites);
    let ctx = RowContext { query, favorites, expanded: input.expanded, group_by_prefix: input.group_by_prefix };
    let branches_of = |repo: &RepoEntry| input.branches.get(&repo.root).map(Vec::as_slice);
    let mut blocks: Vec<Vec<RepoRow>> = Vec::new();

    let actions = action_rows(input.actions, query, query.is_empty(), i18n);
    blocks.push(actions.into_iter().map(RepoRow::from).collect());

    let repo_row = |(index, repo): (usize, &RepoEntry)| {
        let matches = ctx.matches(&repo.name)?;
        let key = format!("repo:{}", repo.root);
        Some(RepoRow::Repo { key, repo: repo.clone(), color: repo_color_index(index), matches })
    };
    blocks.push(repos.iter().enumerate().filter_map(repo_row).collect());

    let shared = !widget_shows_repo(repos);
    let current = repos.iter().position(|repo| Some(repo.root.as_str()) == input.current).or(repos.first().map(|_| 0));
    let loaded: Option<Vec<&[Branch]>> = repos.iter().map(branches_of).collect();
    let common = match loaded {
        Some(lists) if shared => common_branches(&lists, favorites),
        _ => CommonBranches::default(),
    };
    let shown: HashSet<(bool, &str)> =
        common.local.iter().chain(&common.remote).map(|b| (b.remote, b.name.as_str())).collect();
    let mut groups: Vec<RepoRow> = Vec::new();
    if shared {
        let local = common_rows(&ctx, &common.local, BranchGroup::Local);
        push_repo_group(&ctx, &mut groups, "common:local", i18n.tx("Common Local Branches"), local, true);
        let remote = common_rows(&ctx, &common.remote, BranchGroup::Remote);
        push_repo_group(&ctx, &mut groups, "common:remote", i18n.tx("Common Remote Branches"), remote, false);
    } else if let Some(repo) = current.map(|index| &repos[index]) {
        let list = branches_of(repo).unwrap_or_default();
        let local = sort_local(list, favorites);
        let recent =
            if query.is_empty() && input.show_recent { recent_local(&local, input.recent) } else { Vec::new() };
        let mut title = |start: &str| i18n.tx_built(&[start, ""], &[Param::from(repo.name.as_str())]);
        let grouped = input.group_by_prefix;
        let recent_rows = repo_branch_rows(&ctx, repo, &recent, BranchGroup::Recent, 1, grouped);
        let local_open = recent_rows.is_empty();
        push_repo_group(&ctx, &mut groups, "repo:recent", title("Recent Branches in "), recent_rows, true);
        let local_rows = repo_branch_rows(&ctx, repo, &local, BranchGroup::Local, 1, grouped);
        push_repo_group(&ctx, &mut groups, "repo:local", title("Local Branches in "), local_rows, local_open);
        let remote_rows = repo_branch_rows(&ctx, repo, &sort_remote(list, favorites), BranchGroup::Remote, 1, grouped);
        push_repo_group(&ctx, &mut groups, "repo:remote", title("Remote Branches in "), remote_rows, false);
        if input.show_tags {
            let tag_row = |tag: &str| {
                let matches = ctx.matches(tag)?;
                let key = format!("tag:{}:{tag}", repo.root);
                Some(RepoRow::Tag { key, repo: repo.clone(), tag: tag.to_owned(), depth: 1, matches })
            };
            let tag_rows = sort_tags(input.tags).into_iter().filter_map(tag_row).collect();
            push_repo_group(&ctx, &mut groups, "repo:tags", title("Tags in "), tag_rows, false);
        }
    }
    blocks.push(groups);

    if !query.is_empty() {
        for (index, repo) in repos.iter().enumerate() {
            if !shared && Some(index) == current {
                continue;
            }
            let list = branches_of(repo).unwrap_or_default();
            let local = not_shown(sort_local(list, favorites), &shown);
            let remote = not_shown(sort_remote(list, favorites), &shown);
            let mut rows = vec![RepoRow::Section { key: format!("section:{}", repo.root), label: repo.name.clone() }];
            rows.extend(repo_branch_rows(&ctx, repo, &local, BranchGroup::Local, 0, false));
            rows.extend(repo_branch_rows(&ctx, repo, &remote, BranchGroup::Remote, 0, false));
            if rows.len() > 1 {
                blocks.push(rows);
            }
        }
    }

    let mut rows: Vec<RepoRow> = Vec::new();
    for block in blocks {
        let Some(first) = block.first() else { continue };
        if !rows.is_empty() && !matches!(first, RepoRow::Section { .. }) {
            rows.push(RepoRow::Section { key: format!("separator:{}", rows.len()), label: String::new() });
        }
        rows.extend(block);
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str) -> Branch {
        let remote = name.starts_with("origin/") || name.starts_with("upstream/");
        Branch { name: name.to_owned(), remote, current: false }
    }

    fn current(name: &str) -> Branch {
        Branch { current: true, ..b(name) }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn branches() -> Vec<Branch> {
        vec![
            current("feature/order-paging"),
            b("zeta"),
            b("main"),
            b("bugfix"),
            b("origin/HEAD"),
            b("origin/main"),
            b("origin/feature/order-paging"),
        ]
    }

    fn action(id: &str, label: &str, group: Option<&str>) -> PopupAction {
        PopupAction {
            id: id.to_owned(),
            label: label.to_owned(),
            group: group.map(str::to_owned),
            ..PopupAction::default()
        }
    }

    fn actions() -> Vec<PopupAction> {
        vec![action("vcs.update", "Update Project…", None), action("vcs.push", "Push…", None)]
    }

    fn mark(expanded: bool) -> &'static str {
        if expanded { "v" } else { ">" }
    }

    fn summary(rows: &[BranchRow]) -> Vec<String> {
        let line = |row: &BranchRow| match row {
            BranchRow::Section { label, .. } if label.is_empty() => "---".to_owned(),
            BranchRow::Section { label, .. } => format!("# {label}"),
            BranchRow::Action { action, .. } => format!("> {}", action.label),
            BranchRow::Group { label, expanded, depth, .. } => {
                format!("{}{} {label}", "  ".repeat(*depth), mark(*expanded))
            }
            BranchRow::Tag { tag, depth, .. } => format!("{}tag:{tag}", "  ".repeat(*depth)),
            BranchRow::Branch { group, label, depth, favorite, .. } => {
                format!("{}{}:{label}{}", "  ".repeat(*depth), group.id(), if *favorite { " ★" } else { "" })
            }
        };
        rows.iter().map(line).collect()
    }

    fn rows(input: &BranchRowsInput) -> Vec<String> {
        summary(&build_branch_rows(input, &I18n::default()))
    }

    fn open(keys: &[&str]) -> HashMap<String, bool> {
        keys.iter().map(|key| ((*key).to_owned(), true)).collect()
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › shows actions, then Recent open and Local / Remote collapsed
    #[test]
    fn shows_actions_then_recent_open_and_local_remote_collapsed() {
        let (branches, actions) = (branches(), actions());
        let recent = strings(&["bugfix", "feature/order-paging", "gone"]);
        let input = BranchRowsInput { branches: &branches, recent: &recent, actions: &actions, ..Default::default() };
        assert_eq!(
            rows(&input),
            ["> Update Project…", "> Push…", "---", "v Recent", "  recent:bugfix", "> Local", "> Remote"]
        );
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › opens Local by default without recent branches
    #[test]
    fn opens_local_by_default_without_recent_branches() {
        let branches = branches();
        let options = BranchTreeOptions { group_by_prefix: false, ..Default::default() };
        assert_eq!(
            rows(&BranchRowsInput { branches: &branches, options, ..Default::default() }),
            ["v Local", "  local:feature/order-paging", "  local:main ★", "  local:bugfix", "  local:zeta", "> Remote"]
        );
        let recent = strings(&["bugfix"]);
        let options = BranchTreeOptions { show_recent: false, ..Default::default() };
        let input = BranchRowsInput { branches: &branches, recent: &recent, options, ..Default::default() };
        assert_eq!(rows(&input)[..1], ["v Local"]);
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › groups names by prefix: pinned first, then folders, then the rest; remotes are folders
    #[test]
    fn groups_names_by_prefix_pinned_first_then_folders_then_the_rest_remotes_are_folders() {
        let branches = branches();
        let recent = strings(&["bugfix"]);
        let options = BranchTreeOptions { expanded: open(&["group:local", "group:remote"]), ..Default::default() };
        let shown = rows(&BranchRowsInput { branches: &branches, recent: &recent, options, ..Default::default() });
        assert_eq!(
            shown[2..],
            [
                "v Local",
                "  local:main ★",
                // Holds the current branch: open.
                "  v feature",
                "    local:order-paging",
                "  local:bugfix",
                "  local:zeta",
                "v Remote",
                "  v origin",
                "    remote:main ★",
                "    > feature",
            ]
        );
        let expanded = HashMap::from([("group:local".to_owned(), true), ("folder:local:feature".to_owned(), false)]);
        let options = BranchTreeOptions { expanded, ..Default::default() };
        let closed = rows(&BranchRowsInput { branches: &branches, recent: &recent, options, ..Default::default() });
        assert!(closed.iter().any(|line| line == "  > feature"));
        assert!(!closed.iter().any(|line| line == "    local:order-paging"));
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › separates actions of different groups
    #[test]
    fn separates_actions_of_different_groups() {
        let grouped = [
            action("a", "Update Project…", Some("sync")),
            action("b", "Push…", Some("sync")),
            action("c", "New Branch…", Some("branch")),
        ];
        assert_eq!(
            rows(&BranchRowsInput { actions: &grouped, ..Default::default() }),
            ["> Update Project…", "> Push…", "---", "> New Branch…"]
        );
        assert_eq!(
            rows(&BranchRowsInput { actions: &grouped, query: "u", ..Default::default() }),
            ["> Update Project…", "> Push…"]
        );
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › filters branches and actions by fuzzy query, opens every group and drops the Recent group
    #[test]
    fn filters_branches_and_actions_by_fuzzy_query_opens_every_group_and_drops_the_recent_group() {
        let (branches, actions) = (branches(), actions());
        let recent = strings(&["main"]);
        let input = BranchRowsInput {
            branches: &branches,
            recent: &recent,
            query: "main",
            actions: &actions,
            ..Default::default()
        };
        let found = build_branch_rows(&input, &I18n::default());
        assert_eq!(summary(&found), ["v Local", "  local:main ★", "v Remote", "  v origin", "    remote:main ★"]);
        let BranchRow::Branch { matches, .. } = &found[1] else { panic!("not a branch: {:?}", found[1]) };
        assert_eq!(matches.as_deref(), Some(&[0, 1, 2, 3][..]));
        // Highlights shifted onto the shown part of `origin/main`.
        let BranchRow::Branch { label, matches, .. } = &found[4] else { panic!("not a branch: {:?}", found[4]) };
        assert_eq!((label.as_str(), matches.as_deref()), ("main", Some(&[0, 1, 2, 3][..])));
        let push = BranchRowsInput { branches: &branches, query: "push", actions: &actions, ..Default::default() };
        assert_eq!(rows(&push), ["> Push…"]);
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › omits empty groups
    #[test]
    fn omits_empty_groups() {
        let branches = [current("main")];
        assert_eq!(rows(&BranchRowsInput { branches: &branches, ..Default::default() }), ["v Local", "  local:main ★"]);
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › puts the user's favorites right after the current branch
    #[test]
    fn puts_the_users_favorites_right_after_the_current_branch() {
        let branches = branches();
        let stored = strings(&["zeta", "origin/feature/order-paging"]);
        let favorites = Some(stored.as_slice());
        let options =
            BranchTreeOptions { group_by_prefix: false, expanded: open(&["group:remote"]), ..Default::default() };
        let input = BranchRowsInput { branches: &branches, favorites, options, ..Default::default() };
        assert_eq!(
            rows(&input),
            [
                "v Local",
                "  local:feature/order-paging",
                "  local:zeta ★",
                "  local:bugfix",
                "  local:main",
                "v Remote",
                "  remote:origin/feature/order-paging ★",
                "  remote:origin/main",
            ]
        );
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchRows › lists tags last in a collapsed group, newest version first, and filters them by the query
    #[test]
    fn lists_tags_last_in_a_collapsed_group_newest_version_first_and_filters_them_by_the_query() {
        let tags = strings(&["v1.2", "v1.10", "release"]);
        assert_eq!(rows(&BranchRowsInput { tags: &tags, ..Default::default() }), ["> Tags"]);
        let options = BranchTreeOptions { expanded: open(&["group:tags"]), ..Default::default() };
        assert_eq!(
            rows(&BranchRowsInput { tags: &tags, options, ..Default::default() }),
            ["v Tags", "  tag:v1.10", "  tag:v1.2", "  tag:release"]
        );
        let branches = branches();
        assert_eq!(
            rows(&BranchRowsInput { branches: &branches, query: "v1.1", tags: &tags, ..Default::default() }),
            ["v Tags", "  tag:v1.10"]
        );
        let options = BranchTreeOptions { show_tags: false, ..Default::default() };
        assert!(rows(&BranchRowsInput { tags: &tags, options, ..Default::default() }).is_empty());
    }

    // src/features/vcs/__tests__/branches.test.ts › pushRecent › moves a branch to the front without duplicates
    #[test]
    fn moves_a_branch_to_the_front_without_duplicates() {
        assert_eq!(push_recent(&strings(&["a", "b", "c"]), "b", RECENT_LIMIT), ["b", "a", "c"]);
        assert_eq!(push_recent(&strings(&["a", "b"]), "c", 2), ["c", "a"]);
    }

    fn outline(nodes: &[BranchTreeNode]) -> Vec<String> {
        fn walk(nodes: &[BranchTreeNode], depth: usize, out: &mut Vec<String>) {
            for node in nodes {
                out.push(format!("{}{}{}", "  ".repeat(depth), if node.favorite { "*" } else { "" }, node.label));
                walk(node.children.as_deref().unwrap_or_default(), depth + 1, out);
            }
        }
        let mut out = Vec::new();
        walk(nodes, 0, &mut out);
        out
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchTree › groups names into folders with favorites first
    #[test]
    fn groups_names_into_folders_with_favorites_first() {
        let branches = [
            b("zeta"),
            b("feat/b"),
            b("feat/a"),
            b("main"),
            b("fix/deep/x"),
            current("dev"),
            b("origin/main"),
            b("origin/feat/a"),
            b("origin/HEAD"),
        ];
        let tree = build_branch_tree(&branches, None, &[]);
        assert_eq!(
            outline(&tree),
            [
                "HEAD (Current Branch)",
                "Local",
                "  dev",
                "  *main",
                "  feat",
                "    a",
                "    b",
                "  fix",
                "    deep",
                "      x",
                "  zeta",
                "Remote",
                "  origin",
                "    *main",
                "    feat",
                "      a",
            ]
        );
        assert_eq!(branch_folder_keys(&tree, "fix/deep/x"), ["g:local", "f:local/fix", "f:local/fix/deep"]);
        assert!(branch_folder_keys(&tree, "nope").is_empty());
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchTree › filters by branch name, keeping the folders of matches
    #[test]
    fn filters_by_branch_name_keeping_the_folders_of_matches() {
        let branches = [b("main"), b("feat/login"), b("fix/login-crash"), b("origin/feat/login")];
        let tree = build_branch_tree(&branches, None, &[]);
        assert_eq!(
            outline(&filter_branch_tree(&tree, "LOGIN")),
            [
                "Local",
                "  feat",
                "    login",
                "  fix",
                "    login-crash",
                "Remote",
                "  origin",
                "    feat",
                "      login",
            ]
        );
        assert!(filter_branch_tree(&tree, "nothing").is_empty());
    }

    // src/features/vcs/__tests__/branches.test.ts › buildBranchTree › uses the stored favorites once the user edits them
    #[test]
    fn uses_the_stored_favorites_once_the_user_edits_them() {
        let stored = strings(&["dev"]);
        assert!(is_favorite_branch("main", false, None));
        assert!(is_favorite_branch("origin/master", true, None));
        assert!(!is_favorite_branch("main", false, Some(stored.as_slice())));
        assert!(is_favorite_branch("dev", false, Some(stored.as_slice())));
    }

    // src/features/vcs/__tests__/logPanes.test.tsx › Branches pane › lists tags in their own group, newest version first, and finds them by name
    #[test]
    fn lists_tags_in_their_own_group_newest_version_first_and_finds_them_by_name() {
        let roots = build_branch_tree(&[current("main")], None, &strings(&["v1.2", "v1.10", "v1.9"]));
        let tags = roots.iter().find(|node| node.key == "g:tags").unwrap();
        let labels: Vec<&str> = tags.children.iter().flatten().map(|node| node.label.as_str()).collect();
        assert_eq!(labels, ["v1.10", "v1.9", "v1.2"]);
        let found: Vec<String> = filter_branch_tree(&roots, "1.9").into_iter().map(|node| node.key).collect();
        assert_eq!(found, ["g:tags"]);
        assert!(!build_branch_tree(&[b("main")], None, &[]).iter().any(|node| node.key == "g:tags"));
    }

    // src/features/vcs/__tests__/multiRoot.test.ts › groups identical local and remote names under distinct repository nodes
    #[test]
    fn groups_identical_local_and_remote_names_under_distinct_repository_nodes() {
        let repository = |name: &str| RepositoryBranches {
            root: format!("/project/{name}"),
            name: name.to_owned(),
            branches: vec![b("main"), b("origin/main")],
            tags: strings(&["v1"]),
        };
        let favorites: Vec<String> = Vec::new();
        let registered = [repository("front"), repository("back")];
        let roots = build_repository_branch_tree(&registered, Some(favorites.as_slice()));
        for group in roots.iter().filter(|node| node.kind == BranchNodeKind::Group) {
            let repositories = group.children.as_deref().unwrap();
            let labels: Vec<&str> = repositories.iter().map(|node| node.label.as_str()).collect();
            assert_eq!(labels, ["front", "back"]);
            let children: Vec<&BranchTreeNode> =
                repositories.iter().flat_map(|node| node.children.iter().flatten()).collect();
            let keys: HashSet<&str> = children.iter().map(|node| node.key.as_str()).collect();
            assert_eq!(keys.len(), children.len());
            let repos: Vec<Option<&str>> = children.iter().map(|node| node.repo.as_deref()).collect();
            assert_eq!(repos, [Some("/project/front"), Some("/project/back")]);
        }
    }

    fn repo(name: &str, head: Option<&str>) -> RepoEntry {
        RepoEntry {
            root: format!("/p/{name}"),
            name: name.to_owned(),
            head: head.map(str::to_owned),
            head_commit: Some("0123456789abcdef".to_owned()),
        }
    }

    // The reported layout: several repositories, all on `dev`.
    fn repos() -> Vec<RepoEntry> {
        ["ology-collect", "ology-dependencies", "ology-gateway"].map(|name| repo(name, Some("dev"))).to_vec()
    }

    fn repo_branches() -> Vec<(String, Vec<Branch>)> {
        let collect = vec![
            current("dev"),
            b("main"),
            b("collect-only"),
            b("origin/HEAD"),
            b("origin/dev"),
            b("origin/main"),
            b("origin/feature/x"),
        ];
        let dependencies = vec![current("dev"), b("main"), b("origin/dev"), b("origin/main")];
        let gateway = vec![current("dev"), b("main"), b("origin/dev"), b("origin/main"), b("gw-fix")];
        vec![
            ("/p/ology-collect".to_owned(), collect),
            ("/p/ology-dependencies".to_owned(), dependencies),
            ("/p/ology-gateway".to_owned(), gateway),
        ]
    }

    fn repo_actions() -> Vec<PopupAction> {
        vec![action("vcs.update", "Update Project…", None), action("vcs.newBranch", "New Branch…", None)]
    }

    fn repo_summary(rows: &[RepoRow]) -> Vec<String> {
        let line = |row: &RepoRow| match row {
            RepoRow::Section { label, .. } if label.is_empty() => "---".to_owned(),
            RepoRow::Section { label, .. } => format!("# {label}"),
            RepoRow::Action { action, .. } => format!("> {}", action.label),
            RepoRow::Repo { repo, color, .. } => {
                format!("repo {} [{}] c{color}", repo.name, repo.head.as_deref().unwrap_or("null"))
            }
            RepoRow::Group { label, expanded, depth, .. } => {
                format!("{}{} {label}", "  ".repeat(*depth), mark(*expanded))
            }
            RepoRow::Common { branch, label, depth, .. } => {
                format!("{}{label}{}", "  ".repeat(*depth), if branch.current { " (current)" } else { "" })
            }
            RepoRow::Branch { repo, branch, depth, .. } => {
                format!("{}{}:{}", "  ".repeat(*depth), repo.name, branch.name)
            }
            RepoRow::Tag { tag, depth, .. } => format!("{}tag {tag}", "  ".repeat(*depth)),
        };
        rows.iter().map(line).collect()
    }

    /// What a popup over fixed repositories, branches and actions is asked to show.
    struct Popup {
        repos: Vec<RepoEntry>,
        branches: HashMap<String, Vec<Branch>>,
        actions: Vec<PopupAction>,
        expanded: HashMap<String, bool>,
        current: Option<&'static str>,
        recent: Vec<String>,
        tags: Vec<String>,
        group_by_prefix: bool,
        show_recent: bool,
    }

    impl Popup {
        fn shared() -> Self {
            Self {
                repos: repos(),
                branches: repo_branches().into_iter().collect(),
                actions: repo_actions(),
                expanded: HashMap::new(),
                current: None,
                recent: Vec::new(),
                tags: Vec::new(),
                group_by_prefix: true,
                show_recent: true,
            }
        }

        fn diverged() -> Self {
            let mut popup = Self::shared();
            popup.repos[0] = repo("ology-collect", Some("release/1.0"));
            let own = vec![current("release/1.0"), b("dev"), b("main"), b("origin/dev"), b("origin/release/1.0")];
            popup.branches.insert("/p/ology-collect".to_owned(), own);
            popup.actions = Vec::new();
            popup.recent = strings(&["dev", "release/1.0", "gone"]);
            popup.tags = strings(&["v1.2", "v1.10"]);
            popup
        }

        fn rows_in(&self, query: &str, i18n: &mut I18n) -> Vec<RepoRow> {
            let input = MultiRepoInput {
                repos: &self.repos,
                branches: &self.branches,
                query,
                actions: &self.actions,
                expanded: &self.expanded,
                favorites: None,
                current: self.current,
                recent: &self.recent,
                tags: &self.tags,
                group_by_prefix: self.group_by_prefix,
                show_recent: self.show_recent,
                show_tags: true,
            };
            build_multi_repo_rows(&input, i18n)
        }

        fn rows(&self, query: &str) -> Vec<RepoRow> {
            self.rows_in(query, &mut I18n::default())
        }

        fn summary(&self, query: &str) -> Vec<String> {
            repo_summary(&self.rows(query))
        }
    }

    const ROOTS: [&str; 4] = ["/p", "/p/ology-collect", "/p/ology-collect/sub", "/p/ology-gateway"];

    fn current_repo(active: Option<&str>, selected: Option<&str>) -> Option<String> {
        current_repo_for(active, &strings(&ROOTS), selected).map(str::to_owned)
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › the innermost repository of the active file
    #[test]
    fn current_repo_is_the_innermost_repository_of_the_active_file() {
        let found = current_repo(Some("/p/ology-collect/src/A.java"), Some("/p/ology-gateway"));
        assert_eq!(found.as_deref(), Some("/p/ology-collect"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › a nested repository's file
    #[test]
    fn current_repo_of_a_nested_repositorys_file() {
        assert_eq!(current_repo(Some("/p/ology-collect/sub/x.ts"), None).as_deref(), Some("/p/ology-collect/sub"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › the project's own repository
    #[test]
    fn current_repo_is_the_projects_own_repository() {
        assert_eq!(current_repo(Some("/p/README.md"), Some("/p/ology-gateway")).as_deref(), Some("/p"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › no active file: the selected repository
    #[test]
    fn current_repo_without_an_active_file_is_the_selected_repository() {
        assert_eq!(current_repo(None, Some("/p/ology-gateway")).as_deref(), Some("/p/ology-gateway"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › a file outside every repository: the selected one
    #[test]
    fn current_repo_of_a_file_outside_every_repository_is_the_selected_one() {
        let found = current_repo(Some("/elsewhere/a.txt"), Some("/p/ology-gateway"));
        assert_eq!(found.as_deref(), Some("/p/ology-gateway"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › a selected repository that is gone: the first
    #[test]
    fn current_repo_when_the_selected_repository_is_gone_is_the_first() {
        assert_eq!(current_repo(None, Some("/gone")).as_deref(), Some("/p"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › nothing selected: the first
    #[test]
    fn current_repo_with_nothing_selected_is_the_first() {
        assert_eq!(current_repo(None, None).as_deref(), Some("/p"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › currentRepoFor (shared by the widget and the popup) › is null without repositories
    #[test]
    fn current_repo_is_none_without_repositories() {
        assert_eq!(current_repo_for(Some("/p/a"), &[], None), None);
    }

    fn shows_repo(heads: &[Option<&str>]) -> bool {
        let repos: Vec<RepoEntry> = heads.iter().map(|head| repo("r", *head)).collect();
        widget_shows_repo(&repos)
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › one repository
    #[test]
    fn widget_hides_the_repository_with_one_repository() {
        assert!(!shows_repo(&[Some("main")]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › all on the same branch
    #[test]
    fn widget_hides_the_repository_when_all_are_on_the_same_branch() {
        assert!(!shows_repo(&[Some("dev"), Some("dev"), Some("dev")]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › one on another branch
    #[test]
    fn widget_shows_the_repository_with_one_on_another_branch() {
        assert!(shows_repo(&[Some("dev"), Some("feature"), Some("dev")]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › one detached
    #[test]
    fn widget_shows_the_repository_with_one_detached() {
        assert!(shows_repo(&[Some("dev"), None]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › all detached
    #[test]
    fn widget_shows_the_repository_with_all_detached() {
        assert!(shows_repo(&[None, None]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › widgetShowsRepo › no repositories
    #[test]
    fn widget_hides_the_repository_without_repositories() {
        assert!(!shows_repo(&[]));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › commonBranches › keeps the branches every repository has, current in all first, then favourites
    #[test]
    fn keeps_the_branches_every_repository_has_current_in_all_first_then_favourites() {
        let by_repo = repo_branches();
        let lists: Vec<&[Branch]> = by_repo.iter().map(|(_, list)| list.as_slice()).collect();
        let common = common_branches(&lists, None);
        let local: Vec<(&str, bool, bool)> =
            common.local.iter().map(|x| (x.name.as_str(), x.current, x.favorite)).collect();
        assert_eq!(local, [("dev", true, false), ("main", false, true)]);
        let remote: Vec<&str> = common.remote.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(remote, ["origin/main", "origin/dev"]);
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › commonBranches › is current only when checked out everywhere
    #[test]
    fn is_current_only_when_checked_out_everywhere() {
        let common = common_branches(&[&[current("dev")], &[b("dev")]], None);
        let dev = CommonBranch { name: "dev".to_owned(), remote: false, current: false, favorite: false };
        assert_eq!(common.local, [dev]);
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › commonBranches › does not match a local branch with a remote one of the same name
    #[test]
    fn does_not_match_a_local_branch_with_a_remote_one_of_the_same_name() {
        let local = Branch { remote: false, ..b("origin/x") };
        assert_eq!(common_branches(&[&[local], &[b("origin/x")]], None), CommonBranches::default());
        assert_eq!(common_branches(&[], None), CommonBranches::default());
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › lists actions, repositories with their branch, then the common groups
    #[test]
    fn lists_actions_repositories_with_their_branch_then_the_common_groups() {
        assert_eq!(
            Popup::shared().summary(""),
            [
                "> Update Project…",
                "> New Branch…",
                "---",
                "repo ology-collect [dev] c1",
                "repo ology-dependencies [dev] c2",
                "repo ology-gateway [dev] c3",
                "---",
                "v Common Local Branches",
                "  dev (current)",
                "  main",
                "> Common Remote Branches",
            ]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › groups common remote branches by remote when expanded
    #[test]
    fn groups_common_remote_branches_by_remote_when_expanded() {
        let mut popup = Popup::shared();
        popup.expanded = HashMap::from([("common:local".to_owned(), false), ("common:remote".to_owned(), true)]);
        let shown = popup.summary("");
        assert_eq!(
            shown[shown.len() - 6..],
            ["---", "> Common Local Branches", "v Common Remote Branches", "  v origin", "    main", "    dev"]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › filters repositories and branches, expands every group and lists each repository's own matches
    #[test]
    fn filters_repositories_and_branches_expands_every_group_and_lists_each_repositorys_own_matches() {
        let popup = Popup::shared();
        assert_eq!(
            popup.summary("dev"),
            ["v Common Local Branches", "  dev (current)", "v Common Remote Branches", "  v origin", "    dev"]
        );
        assert_eq!(popup.summary("deps"), ["repo ology-dependencies [dev] c2"]);
        assert_eq!(popup.summary("fix"), ["# ology-gateway", "ology-gateway:gw-fix"]);
        assert_eq!(
            popup.summary("collect"),
            ["repo ology-collect [dev] c1", "# ology-collect", "ology-collect:collect-only"]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › highlights the shown part of a remote branch name
    #[test]
    fn highlights_the_shown_part_of_a_remote_branch_name() {
        let rows = Popup::shared().rows("origin/dev");
        let dev = rows.iter().find_map(|row| match row {
            RepoRow::Common { branch, label, matches, .. } if branch.name == "origin/dev" => Some((label, matches)),
            _ => None,
        });
        let (label, matches) = dev.unwrap();
        assert_eq!((label.as_str(), matches.as_deref()), ("dev", Some(&[0, 1, 2][..])));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › shows no common branches until every repository's branches are loaded
    #[test]
    fn shows_no_common_branches_until_every_repositorys_branches_are_loaded() {
        let mut popup = Popup::shared();
        popup.branches.retain(|root, _| root == "/p/ology-collect");
        assert_eq!(popup.summary("").last().map(String::as_str), Some("repo ology-gateway [dev] c3"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows › matches and shows actions and groups in the UI language
    #[test]
    fn matches_and_shows_actions_and_groups_in_the_ui_language() {
        let popup = Popup::shared();
        let mut i18n = I18n::default();
        i18n.set_locale(Some("zh"), "en");
        let found = popup.rows_in("更新", &mut i18n);
        assert_eq!(found.len(), 1);
        let RepoRow::Action { matches, .. } = &found[0] else { panic!("not an action: {:?}", found[0]) };
        // The two characters of the query, each three bytes long.
        assert_eq!(matches.as_deref(), Some(&[0, 3][..]));
        assert!(repo_summary(&popup.rows_in("", &mut i18n)).iter().any(|line| line == "v 公共本地分支"));
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows with repositories on different branches › lists the current repository's recent, local, remote branches and tags instead of the common ones
    #[test]
    fn lists_the_current_repositorys_recent_local_remote_branches_and_tags_instead_of_the_common_ones() {
        let mut popup = Popup::diverged();
        popup.current = Some("/p/ology-collect");
        assert_eq!(
            popup.summary(""),
            [
                "repo ology-collect [release/1.0] c1",
                "repo ology-dependencies [dev] c2",
                "repo ology-gateway [dev] c3",
                "---",
                "v Recent Branches in ology-collect",
                "  ology-collect:dev",
                "> Local Branches in ology-collect",
                "> Remote Branches in ology-collect",
                "> Tags in ology-collect",
            ]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows with repositories on different branches › groups the current repository's branches by prefix, opening the current branch's folder
    #[test]
    fn groups_the_current_repositorys_branches_by_prefix_opening_the_current_branchs_folder() {
        let mut popup = Popup::diverged();
        popup.current = Some("/p/ology-collect");
        popup.expanded = HashMap::from([("repo:local".to_owned(), true)]);
        assert_eq!(
            popup.summary("")[6..11],
            [
                "v Local Branches in ology-collect",
                "  ology-collect:main",
                "  v release",
                "    ology-collect:release/1.0",
                "  ology-collect:dev",
            ]
        );
        popup.group_by_prefix = false;
        popup.show_recent = false;
        assert_eq!(
            popup.summary("")[4..8],
            [
                "v Local Branches in ology-collect",
                "  ology-collect:release/1.0",
                "  ology-collect:main",
                "  ology-collect:dev",
            ]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › buildMultiRepoRows with repositories on different branches › falls back to the first repository and expands the matching lists on search
    #[test]
    fn falls_back_to_the_first_repository_and_expands_the_matching_lists_on_search() {
        let mut popup = Popup::diverged();
        popup.current = Some("/elsewhere");
        popup.expanded = HashMap::from([("repo:tags".to_owned(), true), ("repo:recent".to_owned(), true)]);
        assert_eq!(
            popup.summary("")[4..],
            [
                "v Recent Branches in ology-collect",
                "  ology-collect:dev",
                "> Local Branches in ology-collect",
                "> Remote Branches in ology-collect",
                "v Tags in ology-collect",
                "  tag v1.10",
                "  tag v1.2",
            ]
        );
        // Search: the current repository's lists, then the other repositories' branches.
        let mut popup = Popup::diverged();
        popup.current = Some("/p/ology-gateway");
        assert_eq!(popup.summary("fix"), ["v Local Branches in ology-gateway", "  ology-gateway:gw-fix"]);
        assert_eq!(
            popup.summary("rel"),
            ["# ology-collect", "ology-collect:release/1.0", "ology-collect:origin/release/1.0"]
        );
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › repository colours and heads › gives up to nine repositories distinct colours, then cycles
    #[test]
    fn gives_up_to_nine_repositories_distinct_colours_then_cycles() {
        let colours: Vec<usize> = (0..10).map(repo_color_index).collect();
        assert_eq!(colours, [1, 2, 3, 4, 5, 6, 7, 8, 9, 1]);
    }

    // src/features/vcs/__tests__/repoBranches.test.ts › repository colours and heads › names a detached HEAD by its short commit
    #[test]
    fn names_a_detached_head_by_its_short_commit() {
        let i18n = I18n::default();
        let entry = |head: Option<&str>, commit: Option<&str>| RepoEntry {
            head_commit: commit.map(str::to_owned),
            ..repo("r", head)
        };
        assert_eq!(repo_head_label(&entry(Some("dev"), Some("0123456789")), &i18n), "dev");
        assert_eq!(repo_head_label(&entry(None, Some("0123456789")), &i18n), "01234567");
        assert_eq!(repo_head_label(&entry(None, None), &i18n), "No commits");
    }
}
