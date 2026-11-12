//! The Services tree host: the roots every provider contributes, merged in provider order; which
//! nodes are expanded and selected; the toolbar buttons and the context menu of a selection,
//! batched over the selected nodes; reordering; and whether the detail pane exists.
//!
//! Providers own their nodes' meaning. A provider's roots come in as [`ServiceNode`]s
//! ([`ServicesTree::set_provider_roots`]), its toolbar buttons as [`ItemAction`]s and its menu as
//! [`ServiceMenuEntry`]s; what choosing one does is a [`NodeCommand`] for the caller to carry out.
//! What the providers must hear about (a node expanded, a new lead selection, an activation) is
//! returned as [`ServiceRequest`]s.
//!
//! The run-configuration provider is [`crate::run_testing::services`]; [`run_roots`],
//! [`run_actions`], [`run_menu`] and [`run_add_menu`] read it into this vocabulary.

use crate::find::row_selection::SelectMode;
use crate::platform::collate;
use crate::platform::i18n::I18n;
use crate::run_testing::services::{
    AddMenuGroup, DropPosition, MenuCommand, RUN_PROVIDER, ServiceMenuItem, ServiceTreeItem, Services,
};
use crate::services::docker::DockerCommand;
use crate::vcs_ops::menu::{MenuEntry, MenuItem};
use std::cmp::Ordering;
use std::collections::BTreeSet;

/// The Services tool window's id.
pub const SERVICES_TOOL_WINDOW: &str = "services";

/// Nodes expanded before the user expands or collapses anything: the default Docker connection and
/// its containers.
pub const DEFAULT_EXPANDED: [&str; 2] = ["docker:default", "docker:default:group:containers"];

/// The run-configuration provider's place among the providers.
pub const RUN_PROVIDER_ORDER: i32 = 0;
/// The Docker provider's place among the providers.
pub const DOCKER_PROVIDER_ORDER: i32 = 10;
/// A plugin's Services view's place among the providers.
pub const PLUGIN_PROVIDER_ORDER: i32 = 900;

// ---------------------------------------------------------------------------------------------------
// Nodes

/// A node of the Services tree. Its key is unique across providers (each provider prefixes its
/// keys).
///
/// Unlike [`ServiceTreeItem`] it can say it has children before they are known (`expandable`):
/// a lazy provider lists them once the node is expanded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceNode {
    pub key: String,
    pub label: String,
    pub icon: String,
    /// Grey text after the label (state, port, count).
    pub detail: Option<String>,
    /// Greyed label (stopped, not running).
    pub dim: bool,
    /// Expanded the first time the node appears.
    pub expanded: bool,
    /// Shows an expander before its children are known.
    pub expandable: bool,
    /// `None` for leaves.
    pub children: Option<Vec<ServiceNode>>,
}

impl ServiceNode {
    pub fn leaf(key: impl Into<String>, label: impl Into<String>, icon: impl Into<String>) -> Self {
        Self { key: key.into(), label: label.into(), icon: icon.into(), ..Self::default() }
    }

    /// Whether the node has an expander: it has children, or says it may have some.
    pub fn is_expandable(&self) -> bool {
        self.expandable || self.children.as_ref().is_some_and(|children| !children.is_empty())
    }

    fn children(&self) -> &[ServiceNode] {
        self.children.as_deref().unwrap_or_default()
    }
}

impl From<ServiceTreeItem> for ServiceNode {
    fn from(item: ServiceTreeItem) -> Self {
        Self {
            key: item.key,
            label: item.label,
            icon: item.icon,
            detail: item.detail,
            dim: item.dim,
            expanded: item.expanded,
            expandable: false,
            children: item.children.map(|children| children.into_iter().map(Self::from).collect()),
        }
    }
}

/// Keys of every expandable node of `nodes` and below (Expand All). Below a node without an
/// expander nothing is visited.
pub fn expandable_keys(nodes: &[ServiceNode]) -> Vec<String> {
    fn visit(node: &ServiceNode, out: &mut Vec<String>) {
        if node.is_expandable() {
            out.push(node.key.clone());
            for child in node.children() {
                visit(child, out);
            }
        }
    }
    let mut out = Vec::new();
    for node in nodes {
        visit(node, &mut out);
    }
    out
}

/// Where a node is: the provider owning it and the node above it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Located<'a> {
    pub provider: &'a str,
    pub node: &'a ServiceNode,
    pub parent: Option<&'a ServiceNode>,
}

/// One visible line of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceRow {
    pub provider: String,
    pub key: String,
    pub label: String,
    pub icon: String,
    pub detail: Option<String>,
    pub dim: bool,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
}

/// What a provider must hear about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServiceRequest {
    /// The node was expanded or collapsed; a provider listing children lazily loads them now.
    Expand { provider: String, key: String, open: bool },
    /// The node became the lead selection.
    Select { provider: String, key: String },
    /// Double click or Enter on the node.
    Activate { provider: String, key: String },
}

/// The node whose detail the pane shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetailSlot {
    pub provider: String,
    pub key: String,
}

/// A provider's roots and whether it lets its nodes be reordered.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ProviderRoots {
    id: String,
    order: i32,
    roots: Vec<ServiceNode>,
    reorderable: bool,
}

/// The order of providers: by `order`, then by id.
fn provider_order(a: &ProviderRoots, b: &ProviderRoots) -> Ordering {
    a.order.cmp(&b.order).then_with(|| collate::compare(&a.id, &b.id))
}

/// Which rows are selected: the lead (the row the keyboard is on, whose detail shows), the start of
/// Shift ranges, and the selected keys. Keys of nodes that are gone stay until the selection
/// changes, and are skipped by everything that reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Selection {
    lead: Option<String>,
    anchor: Option<String>,
    keys: BTreeSet<String>,
}

/// The Services tree as the tool window holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServicesTree {
    providers: Vec<ProviderRoots>,
    expanded: BTreeSet<String>,
    /// Keys of nodes asking to start expanded that were shown once: collapsing them then sticks.
    seen_expanded: BTreeSet<String>,
    selection: Selection,
    /// Whether the tool window is shown; a hidden one has no detail pane.
    shown: bool,
}

impl Default for ServicesTree {
    fn default() -> Self {
        Self::new()
    }
}

impl ServicesTree {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
            expanded: DEFAULT_EXPANDED.iter().map(|key| key.to_string()).collect(),
            seen_expanded: BTreeSet::new(),
            selection: Selection::default(),
            shown: true,
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Providers

    /// Replaces the roots of provider `id`. A node asking to start expanded is expanded the first
    /// time it appears only.
    pub fn set_provider_roots(&mut self, id: &str, order: i32, roots: Vec<ServiceNode>, reorderable: bool) {
        fn visit(node: &ServiceNode, seen: &mut BTreeSet<String>, expanded: &mut BTreeSet<String>) {
            if node.expanded && seen.insert(node.key.clone()) {
                expanded.insert(node.key.clone());
            }
            for child in node.children() {
                visit(child, seen, expanded);
            }
        }
        for node in &roots {
            visit(node, &mut self.seen_expanded, &mut self.expanded);
        }
        let entry = ProviderRoots { id: id.to_string(), order, roots, reorderable };
        match self.providers.iter_mut().find(|provider| provider.id == id) {
            Some(known) => *known = entry,
            None => self.providers.push(entry),
        }
        self.providers.sort_by(provider_order);
    }

    /// The provider `id` is gone, and its nodes with it.
    pub fn clear_provider_roots(&mut self, id: &str) {
        self.providers.retain(|provider| provider.id != id);
    }

    /// The roots of every provider, in provider order.
    pub fn roots(&self) -> Vec<&ServiceNode> {
        self.providers.iter().flat_map(|provider| provider.roots.iter()).collect()
    }

    /// Whether no provider has a node: the tool window shows its empty state.
    pub fn is_empty(&self) -> bool {
        self.providers.iter().all(|provider| provider.roots.is_empty())
    }

    /// The node with `key`, the provider owning it and its parent.
    pub fn locate(&self, key: &str) -> Option<Located<'_>> {
        fn find<'a>(
            provider: &'a str,
            nodes: &'a [ServiceNode],
            parent: Option<&'a ServiceNode>,
            key: &str,
        ) -> Option<Located<'a>> {
            for node in nodes {
                if node.key == key {
                    return Some(Located { provider, node, parent });
                }
                if let Some(found) = find(provider, node.children(), Some(node), key) {
                    return Some(found);
                }
            }
            None
        }
        self.providers.iter().find_map(|provider| find(&provider.id, &provider.roots, None, key))
    }

    fn is_reorderable(&self, provider: &str) -> bool {
        self.providers.iter().any(|known| known.id == provider && known.reorderable)
    }

    // -----------------------------------------------------------------------------------------------
    // Rows and expansion

    /// The visible lines, top to bottom.
    pub fn rows(&self) -> Vec<ServiceRow> {
        fn walk(tree: &ServicesTree, provider: &str, nodes: &[ServiceNode], depth: usize, out: &mut Vec<ServiceRow>) {
            for node in nodes {
                let expandable = node.is_expandable();
                let expanded = expandable && tree.expanded.contains(&node.key);
                out.push(ServiceRow {
                    provider: provider.to_string(),
                    key: node.key.clone(),
                    label: node.label.clone(),
                    icon: node.icon.clone(),
                    detail: node.detail.clone(),
                    dim: node.dim,
                    depth,
                    expandable,
                    expanded,
                });
                if expanded {
                    walk(tree, provider, node.children(), depth + 1, out);
                }
            }
        }
        let mut out = Vec::new();
        for provider in &self.providers {
            walk(self, &provider.id, &provider.roots, 0, &mut out);
        }
        out
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    /// Expands or collapses one node; its provider hears about it.
    pub fn set_expanded(&mut self, key: &str, open: bool) -> Option<ServiceRequest> {
        let located = self.locate(key)?;
        if !located.node.is_expandable() || self.expanded.contains(key) == open {
            return None;
        }
        let provider = located.provider.to_string();
        if open {
            self.expanded.insert(key.to_string());
        } else {
            self.expanded.remove(key);
        }
        Some(ServiceRequest::Expand { provider, key: key.to_string(), open })
    }

    /// Flips one node's expansion.
    pub fn toggle(&mut self, key: &str) -> Option<ServiceRequest> {
        let open = !self.expanded.contains(key);
        self.set_expanded(key, open)
    }

    /// Replaces the expansion; the provider of every node that opened or closed hears about it,
    /// also of a lazy node whose children are not known yet.
    fn replace_expanded(&mut self, next: BTreeSet<String>) -> Vec<ServiceRequest> {
        let changed: Vec<String> = self.expanded.symmetric_difference(&next).cloned().collect();
        let requests = changed
            .into_iter()
            .filter_map(|key| {
                let provider = self.locate(&key)?.provider.to_string();
                Some(ServiceRequest::Expand { provider, open: next.contains(&key), key })
            })
            .collect();
        self.expanded = next;
        requests
    }

    /// Expand All: every expandable node, lazy ones included.
    pub fn expand_all(&mut self) -> Vec<ServiceRequest> {
        let roots: Vec<ServiceNode> = self.roots().into_iter().cloned().collect();
        self.replace_expanded(expandable_keys(&roots).into_iter().collect())
    }

    /// Collapse All.
    pub fn collapse_all(&mut self) -> Vec<ServiceRequest> {
        self.replace_expanded(BTreeSet::new())
    }

    // -----------------------------------------------------------------------------------------------
    // Selection

    /// The lead selection: the node whose detail shows. `None` while nothing is selected.
    pub fn selected(&self) -> Option<&str> {
        self.selection.lead.as_deref().filter(|_| !self.selection.keys.is_empty())
    }

    /// The selected nodes that exist, in tree order.
    pub fn selected_keys(&self) -> Vec<String> {
        fn walk(nodes: &[ServiceNode], keys: &BTreeSet<String>, out: &mut Vec<String>) {
            for node in nodes {
                if keys.contains(&node.key) {
                    out.push(node.key.clone());
                }
                walk(node.children(), keys, out);
            }
        }
        let mut out = Vec::new();
        for provider in &self.providers {
            walk(&provider.roots, &self.selection.keys, &mut out);
        }
        out
    }

    pub fn is_selected(&self, key: &str) -> bool {
        self.selection.keys.contains(key)
    }

    /// Sets the lead; its provider hears about a lead that changed.
    fn set_lead(&mut self, lead: Option<String>) -> Vec<ServiceRequest> {
        let changed = lead.is_some() && lead != self.selection.lead;
        self.selection.lead = lead;
        let Some(key) = self.selection.lead.as_deref().filter(|_| changed) else { return Vec::new() };
        let Some(located) = self.locate(key) else { return Vec::new() };
        vec![ServiceRequest::Select { provider: located.provider.to_string(), key: key.to_string() }]
    }

    /// A click on the node `key`: alone, as the end of a range from the anchor (Shift), or added to
    /// or removed from the selection (the platform's primary modifier).
    pub fn click(&mut self, key: &str, mode: SelectMode) -> Vec<ServiceRequest> {
        if self.locate(key).is_none() {
            return Vec::new();
        }
        match mode {
            SelectMode::Single => {
                self.selection.keys = BTreeSet::from([key.to_string()]);
                self.selection.anchor = Some(key.to_string());
                self.set_lead(Some(key.to_string()))
            }
            SelectMode::Range => self.select_range(key),
            SelectMode::Toggle => {
                if self.selection.keys.insert(key.to_string()) {
                    self.selection.anchor = Some(key.to_string());
                    return self.set_lead(Some(key.to_string()));
                }
                self.selection.keys.remove(key);
                // The lead moves to the last row still selected; with none left nothing is selected.
                let rest = self.selected_keys().pop();
                self.selection.anchor.clone_from(&rest);
                match rest {
                    Some(rest) => self.set_lead(Some(rest)),
                    None => {
                        self.selection = Selection::default();
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Selects the visible rows from the anchor to `key`.
    fn select_range(&mut self, key: &str) -> Vec<ServiceRequest> {
        let rows = self.rows();
        let Some(to) = rows.iter().position(|row| row.key == key) else { return Vec::new() };
        let anchor = self.selection.anchor.as_deref().and_then(|anchor| rows.iter().position(|row| row.key == anchor));
        let from = anchor.unwrap_or(to);
        let (start, end) = (from.min(to), from.max(to));
        self.selection.keys = rows[start..=end].iter().map(|row| row.key.clone()).collect();
        self.selection.anchor = Some(rows[from].key.clone());
        self.set_lead(Some(key.to_string()))
    }

    /// Up / Down (`delta` -1 / 1), Home / End (`delta` `isize::MIN` / `isize::MAX`): moves the lead
    /// along the visible rows, alone or (`extend`, Shift) extending the range from the anchor.
    pub fn step(&mut self, delta: isize, extend: bool) -> Vec<ServiceRequest> {
        let rows = self.rows();
        let Some(last) = rows.len().checked_sub(1) else { return Vec::new() };
        let current = self.selected().and_then(|lead| rows.iter().position(|row| row.key == lead));
        let next = match (current, delta) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => last,
            (None, _) => 0,
            (Some(at), delta) => at.saturating_add_signed(delta).min(last),
        };
        let key = rows[next].key.clone();
        self.click(&key, if extend { SelectMode::Range } else { SelectMode::Single })
    }

    /// A right click on the node `key`: a selected node keeps the selection, another one is selected
    /// alone; either way it leads.
    pub fn context_click(&mut self, key: &str) {
        if self.locate(key).is_none() {
            return;
        }
        if !self.selection.keys.contains(key) {
            self.selection.keys = BTreeSet::from([key.to_string()]);
            self.selection.anchor = Some(key.to_string());
        }
        self.selection.lead = Some(key.to_string());
    }

    /// Right / Left on the lead: expands a closed node, steps into an open one; collapses an open
    /// node, else goes to the parent.
    pub fn horizontal(&mut self, right: bool) -> Vec<ServiceRequest> {
        let rows = self.rows();
        let Some(at) = self.selected().and_then(|lead| rows.iter().position(|row| row.key == lead)) else {
            return Vec::new();
        };
        let row = &rows[at];
        match (right, row.expandable, row.expanded) {
            (true, true, false) | (false, true, true) => self.toggle(&row.key).into_iter().collect(),
            (true, true, true) => match rows.get(at + 1).filter(|next| next.depth > row.depth) {
                Some(next) => self.click(&next.key, SelectMode::Single),
                None => Vec::new(),
            },
            (false, _, _) => match rows[..at].iter().rev().find(|above| above.depth < row.depth) {
                Some(parent) => self.click(&parent.key, SelectMode::Single),
                None => Vec::new(),
            },
            (true, false, _) => Vec::new(),
        }
    }

    /// Enter or a double click on the node `key`: its provider activates it.
    pub fn activate(&self, key: &str) -> Option<ServiceRequest> {
        let located = self.locate(key)?;
        Some(ServiceRequest::Activate { provider: located.provider.to_string(), key: key.to_string() })
    }

    /// The selected nodes that exist, in tree order; a node below another selected one is left out
    /// (the selected group already stands for it).
    pub fn selected_services(&self) -> Vec<Located<'_>> {
        let mut below: BTreeSet<&str> = BTreeSet::new();
        fn descend<'a>(node: &'a ServiceNode, out: &mut BTreeSet<&'a str>) {
            for child in node.children() {
                out.insert(&child.key);
                descend(child, out);
            }
        }
        let selected: Vec<Located<'_>> = self.selected_keys().iter().filter_map(|key| self.locate(key)).collect();
        for located in &selected {
            descend(located.node, &mut below);
        }
        selected.into_iter().filter(|located| !below.contains(located.node.key.as_str())).collect()
    }

    /// Whether exactly one node is selected (Move Up / Move Down apply).
    pub fn single_selection(&self) -> bool {
        self.selected_keys().len() == 1
    }

    // -----------------------------------------------------------------------------------------------
    // Detail pane

    /// The tool window was shown or hidden. A hidden one drops its detail pane, and with it what the
    /// pane streams (a followed log, a console); it comes back once shown.
    pub fn set_shown(&mut self, shown: bool) {
        self.shown = shown;
    }

    pub fn is_shown(&self) -> bool {
        self.shown
    }

    /// The node whose detail the pane shows: the lead selection, while the window is shown.
    pub fn detail_slot(&self) -> Option<DetailSlot> {
        let key = self.selected().filter(|_| self.shown)?;
        let located = self.locate(key)?;
        Some(DetailSlot { provider: located.provider.to_string(), key: key.to_string() })
    }

    // -----------------------------------------------------------------------------------------------
    // Reordering

    /// Whether the node `source` can be dropped next to `target`: both belong to one reorderable
    /// provider and one parent, and the provider agrees (`can_move`).
    pub fn can_move(&self, source: &str, target: &str, can_move: &dyn Fn(&str, &str) -> bool) -> bool {
        let (Some(from), Some(to)) = (self.locate(source), self.locate(target)) else { return false };
        let parent_key = |parent: Option<&ServiceNode>| parent.map(|parent| parent.key.clone());
        source != target
            && from.provider == to.provider
            && parent_key(from.parent) == parent_key(to.parent)
            && self.is_reorderable(from.provider)
            && can_move(source, target)
    }

    /// Move Up and Move Down for the node `key`: offered for a node with a parent in a reorderable
    /// provider, disabled where the neighbour cannot be swapped with.
    pub fn move_actions(&self, key: &str, can_move: &dyn Fn(&str, &str) -> bool) -> Vec<ItemAction<NodeCommand>> {
        let Some(located) = self.locate(key) else { return Vec::new() };
        let Some(parent) = located.parent.filter(|_| self.is_reorderable(located.provider)) else {
            return Vec::new();
        };
        let siblings = parent.children();
        let Some(at) = siblings.iter().position(|sibling| sibling.key == key) else { return Vec::new() };
        let entry = |up: bool| {
            let neighbour = if up { at.checked_sub(1).and_then(|i| siblings.get(i)) } else { siblings.get(at + 1) };
            let target = neighbour.filter(|target| self.can_move(key, &target.key, can_move));
            let (id, title, icon, position) = if up {
                ("moveUp", "Move Up", "arrow-up", DropPosition::Before)
            } else {
                ("moveDown", "Move Down", "arrow-down", DropPosition::After)
            };
            ItemAction {
                id: id.to_string(),
                title: title.to_string(),
                icon: icon.to_string(),
                disabled: target.is_none(),
                command: target.map(|target| NodeCommand::Move {
                    provider: located.provider.to_string(),
                    source: key.to_string(),
                    target: target.key.clone(),
                    position,
                }),
            }
        };
        vec![entry(true), entry(false)]
    }
}

// ---------------------------------------------------------------------------------------------------
// Commands, toolbar buttons and menus

/// What a toolbar button or a menu entry does, for the caller to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeCommand {
    /// The run-configuration toolbar button `action` of the node `key`: carry it out with
    /// [`Services::invoke`], which rechecks it against the run store as it is then.
    RunAction {
        key: String,
        action: &'static str,
    },
    /// A run-configuration menu entry.
    RunMenu(MenuCommand),
    /// "Add Service › Run Configuration Type" › a type:
    /// [`RunServices::add_service`](crate::run_testing::services::RunServices::add_service).
    AddRunType(String),
    Docker(DockerCommand),
    /// A plugin view's toolbar button or menu entry `command` on its node `key`.
    Plugin {
        provider: String,
        key: String,
        command: String,
    },
    /// Drop the node `source` before or after `target`: for run configurations
    /// [`Services::moved`] and
    /// [`RunServices::set_order`](crate::run_testing::services::RunServices::set_order).
    Move {
        provider: String,
        source: String,
        target: String,
        position: DropPosition,
    },
}

/// A toolbar button for one node.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemAction<C> {
    /// Stable across nodes: buttons with the same id are batched over a selection.
    pub id: String,
    pub title: String,
    pub icon: String,
    pub disabled: bool,
    pub command: Option<C>,
}

/// A toolbar button for the selection: the commands of the selected nodes that can carry it out.
#[derive(Clone, Debug, PartialEq)]
pub struct BatchAction<C> {
    pub id: String,
    pub title: String,
    pub icon: String,
    /// No selected node can carry it out.
    pub disabled: bool,
    pub commands: Vec<C>,
}

/// The buttons every selected node has (`groups` holds each node's), each running on the nodes
/// where it is enabled; the first node's order and titles.
pub fn selection_actions<C: Clone>(groups: &[Vec<ItemAction<C>>]) -> Vec<BatchAction<C>> {
    let Some(first) = groups.first() else { return Vec::new() };
    first
        .iter()
        .filter_map(|action| {
            let matches: Vec<&ItemAction<C>> =
                groups.iter().flat_map(|group| group.iter().filter(|other| other.id == action.id)).collect();
            if matches.len() != groups.len() {
                return None;
            }
            let enabled = matches.iter().filter(|other| !other.disabled);
            let commands: Vec<C> = enabled.filter_map(|other| other.command.clone()).collect();
            Some(BatchAction {
                id: action.id.clone(),
                title: action.title.clone(),
                icon: action.icon.clone(),
                disabled: commands.is_empty(),
                commands,
            })
        })
        .collect()
}

/// A provider's menu entry.
#[derive(Clone, Debug, PartialEq)]
pub enum ServiceMenuEntry<C> {
    Separator,
    Row(ServiceMenuRow<C>),
}

/// A menu row. Rows with the same `id` (else the same label) are batched over a selection.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceMenuRow<C> {
    pub id: Option<String>,
    /// English text: the menu translates it.
    pub label: String,
    pub icon: Option<&'static str>,
    pub disabled: bool,
    pub danger: bool,
    /// A toggle that is on: shown with a check mark.
    pub checked: bool,
    pub command: Option<C>,
    pub submenu: Vec<ServiceMenuEntry<C>>,
}

impl<C> ServiceMenuEntry<C> {
    /// A row that does `command`; without one it is disabled.
    pub fn row(label: impl Into<String>, icon: Option<&'static str>, command: Option<C>) -> Self {
        Self::Row(ServiceMenuRow {
            id: None,
            label: label.into(),
            icon,
            disabled: command.is_none(),
            danger: false,
            checked: false,
            command,
            submenu: Vec::new(),
        })
    }

    /// A row that only opens `submenu`.
    pub fn submenu(label: impl Into<String>, icon: Option<&'static str>, submenu: Vec<Self>) -> Self {
        Self::Row(ServiceMenuRow {
            id: None,
            label: label.into(),
            icon,
            disabled: false,
            danger: false,
            checked: false,
            command: None,
            submenu,
        })
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        if let Self::Row(row) = &mut self {
            row.id = Some(id.into());
        }
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        if let Self::Row(row) = &mut self {
            row.disabled = disabled;
        }
        self
    }

    pub fn danger(mut self) -> Self {
        if let Self::Row(row) = &mut self {
            row.danger = true;
        }
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        if let Self::Row(row) = &mut self {
            row.checked = checked;
        }
        self
    }

    fn key(&self) -> Option<&str> {
        match self {
            Self::Separator => None,
            Self::Row(row) => Some(row.id.as_deref().unwrap_or(&row.label)),
        }
    }
}

/// A batched row as the popup menu shows it: choosing it carries out every command.
fn popup_row<C>(
    label: String,
    icon: Option<&'static str>,
    disabled: bool,
    danger: bool,
    commands: Vec<C>,
    submenu: Vec<MenuItem<Vec<C>>>,
) -> MenuItem<Vec<C>> {
    let run = (!disabled && !commands.is_empty()).then_some(commands);
    let disabled = disabled || (run.is_none() && submenu.is_empty());
    MenuItem::Item(MenuEntry { label, icon, shortcut: None, disabled, danger, run, submenu })
}

fn row_icon<C>(row: &ServiceMenuRow<C>) -> Option<&'static str> {
    if row.checked { Some("check") } else { row.icon }
}

/// One node's menu as the popup shows it.
pub fn single_menu<C: Clone>(entries: &[ServiceMenuEntry<C>]) -> Vec<MenuItem<Vec<C>>> {
    entries
        .iter()
        .map(|entry| match entry {
            ServiceMenuEntry::Separator => MenuItem::Separator,
            ServiceMenuEntry::Row(row) => popup_row(
                row.label.clone(),
                row_icon(row),
                row.disabled,
                row.danger,
                row.command.iter().cloned().collect(),
                single_menu(&row.submenu),
            ),
        })
        .collect()
}

/// The rows every node's menu has (`groups` holds each node's), each running on the nodes where it
/// is enabled. A row that opens a dialog (its label ends in "…") is disabled for several nodes, so
/// one dialog does not open per node.
fn common_menu<C: Clone>(groups: &[Vec<ServiceMenuEntry<C>>]) -> Vec<MenuItem<Vec<C>>> {
    let Some(first) = groups.first() else { return Vec::new() };
    first
        .iter()
        .filter_map(|entry| {
            let ServiceMenuEntry::Row(row) = entry else { return Some(MenuItem::Separator) };
            let key = entry.key();
            let matches: Vec<&ServiceMenuRow<C>> = groups
                .iter()
                .flat_map(|group| group.iter())
                .filter_map(|other| match other {
                    ServiceMenuEntry::Row(other_row) if other.key() == key => Some(other_row),
                    _ => None,
                })
                .collect();
            if matches.len() != groups.len() {
                return None;
            }
            let commands: Vec<C> =
                matches.iter().filter(|other| !other.disabled).filter_map(|other| other.command.clone()).collect();
            let submenu = if matches.iter().any(|other| !other.submenu.is_empty()) {
                let submenus: Vec<Vec<ServiceMenuEntry<C>>> = matches
                    .iter()
                    .map(|other| if other.disabled { Vec::new() } else { other.submenu.clone() })
                    .collect();
                common_menu(&submenus)
            } else {
                Vec::new()
            };
            let dialog = groups.len() > 1 && row.label.ends_with('…');
            let disabled = dialog || (commands.is_empty() && submenu.is_empty());
            Some(popup_row(row.label.clone(), row_icon(row), disabled, row.danger, commands, submenu))
        })
        .collect()
}

/// The context menu of a selection, from each selected node's menu.
pub fn selection_menu<C: Clone>(groups: &[Vec<ServiceMenuEntry<C>>]) -> Vec<MenuItem<Vec<C>>> {
    match groups {
        [one] => single_menu(one),
        _ => common_menu(groups),
    }
}

/// The "Add Service" menu: every provider's entries (`groups`, in provider order), a separator
/// between providers.
pub fn add_service_menu<C: Clone>(groups: &[Vec<ServiceMenuEntry<C>>]) -> Vec<MenuItem<Vec<C>>> {
    let mut out = Vec::new();
    for group in groups.iter().filter(|group| !group.is_empty()) {
        if !out.is_empty() {
            out.push(MenuItem::Separator);
        }
        out.extend(single_menu(group));
    }
    out
}

/// The menu rows of `actions` (Move Up / Move Down below a node's own menu).
pub fn action_rows<C: Clone>(actions: &[ItemAction<C>]) -> Vec<ServiceMenuEntry<C>> {
    actions
        .iter()
        .map(|action| {
            ServiceMenuEntry::row(action.title.clone(), static_icon(&action.icon), action.command.clone())
                .disabled(action.disabled)
        })
        .collect()
}

/// The icon named `name`, when it is one of the app's icons.
pub fn static_icon(name: &str) -> Option<&'static str> {
    crate::theme::icons::icon_names().find(|known| *known == name)
}

// ---------------------------------------------------------------------------------------------------
// The run-configuration provider

/// The run-configuration provider's roots: one group per configuration type.
pub fn run_roots(services: &Services<'_>, i18n: &mut I18n) -> Vec<ServiceNode> {
    services.roots(i18n).into_iter().map(ServiceNode::from).collect()
}

/// The toolbar buttons of the run-configuration node `key`.
pub fn run_actions(services: &Services<'_>, key: &str) -> Vec<ItemAction<NodeCommand>> {
    services
        .item_actions(key)
        .into_iter()
        .map(|action| ItemAction {
            id: action.id.to_string(),
            title: action.title.to_string(),
            icon: action.icon.to_string(),
            disabled: action.disabled,
            command: Some(NodeCommand::RunAction { key: key.to_string(), action: action.id }),
        })
        .collect()
}

/// The context menu of the run-configuration node `key`; a toolbar button's entry is batched by the
/// button's id ("Run" and "Rerun" are one entry).
pub fn run_menu(services: &Services<'_>, key: &str) -> Vec<ServiceMenuEntry<NodeCommand>> {
    services
        .menu(key)
        .into_iter()
        .map(|item| match item {
            ServiceMenuItem::Separator => ServiceMenuEntry::Separator,
            ServiceMenuItem::Item { label, icon, disabled, command } => {
                let id = match &command {
                    Some(MenuCommand::Action(id)) => Some(id.to_string()),
                    _ => None,
                };
                let command = command.map(|command| match command {
                    MenuCommand::Action(action) => NodeCommand::RunAction { key: key.to_string(), action },
                    other => NodeCommand::RunMenu(other),
                });
                let row = ServiceMenuEntry::row(label, static_icon(icon), command).disabled(disabled);
                match id {
                    Some(id) => row.with_id(id),
                    None => row,
                }
            }
        })
        .collect()
}

fn add_menu_groups(groups: Vec<AddMenuGroup>) -> Vec<ServiceMenuEntry<NodeCommand>> {
    groups
        .into_iter()
        .map(|group| {
            let types = group.types.into_iter().map(|ty| {
                ServiceMenuEntry::row(ty.label, static_icon(&ty.icon), Some(NodeCommand::AddRunType(ty.kind)))
            });
            ServiceMenuEntry::submenu(group.label, static_icon(&group.icon), types.collect())
        })
        .collect()
}

/// The run-configuration provider's "Add Service" entry: "Run Configuration Type" with the
/// project's categories, a separator, then the rest; disabled when every type is added.
pub fn run_add_menu(services: &Services<'_>) -> Vec<ServiceMenuEntry<NodeCommand>> {
    let menu = services.add_menu();
    let separator = !menu.project.is_empty() && !menu.rest.is_empty();
    let mut submenu = add_menu_groups(menu.project);
    if separator {
        submenu.push(ServiceMenuEntry::Separator);
    }
    submenu.extend(add_menu_groups(menu.rest));
    let empty = submenu.is_empty();
    vec![ServiceMenuEntry::submenu("Run Configuration Type", Some("toolwindow-run"), submenu).disabled(empty)]
}

/// The id of the run-configuration provider.
pub fn is_run_provider(provider: &str) -> bool {
    provider == RUN_PROVIDER
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::debugger::state::{ServiceSession as DebugServiceSession, SessionStatus as DebugStatus};
    use crate::run::config_types::RunConfigTypes;
    use crate::run::configs::blank_config;
    use crate::run::store::RunStore;
    use crate::run_testing::services::{
        CONFIG_PREFIX, RunServices, ServiceCommand, StoredServices, TYPE_PREFIX, order_key,
    };
    use ide_core::DebugSessionId;
    use ide_run::RunConfig;

    fn leaf(key: &str) -> ServiceNode {
        ServiceNode::leaf(key, key, "run")
    }

    fn group(key: &str, children: Vec<ServiceNode>) -> ServiceNode {
        ServiceNode { children: Some(children), ..leaf(key) }
    }

    /// A tree with the provider "test" listing `one`, `two`, `three`.
    fn three() -> ServicesTree {
        let mut tree = ServicesTree::new();
        tree.set_provider_roots("test", 0, vec![leaf("one"), leaf("two"), leaf("three")], false);
        tree
    }

    /// A toolbar: Run everywhere, Stop disabled on `three`.
    fn actions(key: &str) -> Vec<ItemAction<String>> {
        let action = |id: &str, title: &str, disabled: bool| ItemAction {
            id: id.to_string(),
            title: title.to_string(),
            icon: id.to_string(),
            disabled,
            command: Some(format!("{id} {key}")),
        };
        vec![action("run", "Run", false), action("stop", "Stop", key == "three")]
    }

    /// A menu: Run (Rerun on `two`) with a stable id, and a menu-only row.
    fn menu(key: &str) -> Vec<ServiceMenuEntry<String>> {
        let label = if key == "two" { "Rerun" } else { "Run" };
        vec![
            ServiceMenuEntry::row(label, None, Some(format!("run {key}"))).with_id("run"),
            ServiceMenuEntry::row("Remove from Services", None, Some(format!("remove {key}"))),
        ]
    }

    fn toolbar(tree: &ServicesTree) -> Vec<BatchAction<String>> {
        let groups: Vec<_> = tree.selected_services().iter().map(|located| actions(&located.node.key)).collect();
        selection_actions(&groups)
    }

    fn button<'a>(buttons: &'a [BatchAction<String>], title: &str) -> &'a BatchAction<String> {
        buttons.iter().find(|button| button.title == title).expect("the toolbar has the button")
    }

    fn row<C>(items: &[MenuItem<C>], label: &str) -> MenuEntry<C>
    where
        C: Clone,
    {
        items
            .iter()
            .find_map(|item| match item {
                MenuItem::Item(entry) if entry.label == label => Some(entry.clone()),
                _ => None,
            })
            .expect("the menu has the row")
    }

    fn selects(provider: &str, key: &str) -> ServiceRequest {
        ServiceRequest::Select { provider: provider.to_string(), key: key.to_string() }
    }

    // services tree host

    #[test]
    fn expands_nodes_asking_for_it_once_when_they_first_appear() {
        let npm = || vec![ServiceNode { expanded: true, children: Some(Vec::new()), ..leaf("run:type:npm") }];
        let mut tree = ServicesTree::new();
        tree.set_provider_roots("run", 0, npm(), false);
        assert!(tree.is_expanded("run:type:npm"));
        tree.collapse_all();
        tree.set_provider_roots("run", 0, npm(), false);
        assert!(!tree.is_expanded("run:type:npm"));
    }

    #[test]
    fn joins_the_providers_add_service_entries() {
        let entry = |label: &str| ServiceMenuEntry::row(label, None, Some(label.to_string()));
        let groups = vec![
            vec![entry("Run Configuration Type")],
            Vec::new(),
            vec![entry("Docker Registry…"), entry("Docker Connection…")],
        ];
        let labels: Vec<String> = add_service_menu(&groups)
            .iter()
            .map(|item| item.label().map_or_else(|| "separator".to_string(), str::to_string))
            .collect();
        assert_eq!(labels, ["Run Configuration Type", "separator", "Docker Registry…", "Docker Connection…"]);
    }

    #[test]
    fn orders_roots_by_provider_order_then_id() {
        let mut tree = ServicesTree::new();
        tree.set_provider_roots("docker", DOCKER_PROVIDER_ORDER, vec![leaf("docker:default")], false);
        tree.set_provider_roots("b", 0, vec![leaf("b1")], false);
        tree.set_provider_roots("a", 0, vec![leaf("a1")], false);
        let keys: Vec<&str> = tree.roots().iter().map(|node| node.key.as_str()).collect();
        assert_eq!(keys, ["a1", "b1", "docker:default"]);
        tree.clear_provider_roots("a");
        assert_eq!(tree.roots().len(), 2);
        assert!(!tree.is_empty());
    }

    // Services selection → actions

    #[test]
    fn ctrl_or_cmd_toggles_rows_and_run_applies_to_the_selection() {
        let mut tree = three();
        assert_eq!(tree.click("one", SelectMode::Single), [selects("test", "one")]);
        tree.click("two", SelectMode::Toggle);
        assert_eq!(tree.selected_keys(), ["one", "two"]);
        assert_eq!(tree.detail_slot().map(|slot| slot.key).as_deref(), Some("two"));
        assert_eq!(button(&toolbar(&tree), "Run").commands, ["run one", "run two"]);
        tree.click("one", SelectMode::Toggle);
        assert_eq!(tree.selected_keys(), ["two"]);
        tree.click("two", SelectMode::Toggle);
        assert!(tree.selected_keys().is_empty());
        assert_eq!(tree.detail_slot(), None);
    }

    #[test]
    fn tells_the_provider_which_node_became_the_lead_selection() {
        let mut tree = three();
        let mut told = tree.click("one", SelectMode::Single);
        told.extend(tree.click("two", SelectMode::Single));
        told.extend(tree.click("two", SelectMode::Single));
        assert_eq!(told, [selects("test", "one"), selects("test", "two")]);
    }

    #[test]
    fn drops_the_detail_pane_and_what_it_streams_while_the_services_window_is_hidden() {
        let mut tree = three();
        tree.click("one", SelectMode::Single);
        let slot = DetailSlot { provider: "test".to_string(), key: "one".to_string() };
        assert_eq!(tree.detail_slot(), Some(slot.clone()));
        tree.set_shown(false);
        assert_eq!(tree.detail_slot(), None);
        tree.set_shown(true);
        assert_eq!(tree.detail_slot(), Some(slot));
    }

    #[test]
    fn expand_all_and_collapse_all_reach_lazy_nodes_and_tell_their_provider() {
        let mut tree = ServicesTree::new();
        let lazy = ServiceNode { expandable: true, children: Some(Vec::new()), ..leaf("lazy") };
        tree.set_provider_roots("lazy", 0, vec![lazy], false);
        let expand =
            |open: bool| ServiceRequest::Expand { provider: "lazy".to_string(), key: "lazy".to_string(), open };
        assert_eq!(tree.expand_all(), [expand(true)]);
        assert!(tree.is_expanded("lazy"));
        assert_eq!(tree.collapse_all(), [expand(false)]);
        assert!(!tree.is_expanded("lazy"));
    }

    #[test]
    fn shift_click_selects_a_range_stop_skips_disabled_services() {
        let mut tree = three();
        tree.click("one", SelectMode::Single);
        tree.click("three", SelectMode::Range);
        assert_eq!(tree.selected_keys(), ["one", "two", "three"]);
        let stop = button(&toolbar(&tree), "Stop").clone();
        assert!(!stop.disabled);
        assert_eq!(stop.commands, ["stop one", "stop two"]);
    }

    #[test]
    fn shift_arrows_extend_and_shrink_the_selection_used_by_run() {
        let mut tree = three();
        tree.click("one", SelectMode::Single);
        tree.step(1, true);
        tree.step(1, true);
        assert_eq!(tree.selected_keys(), ["one", "two", "three"]);
        tree.step(-1, true);
        assert_eq!(tree.selected_keys(), ["one", "two"]);
        assert_eq!(button(&toolbar(&tree), "Run").commands, ["run one", "run two"]);
    }

    #[test]
    fn context_menu_preserves_selected_rows_and_batches_stable_ids_and_menu_only_actions() {
        let mut tree = three();
        tree.click("one", SelectMode::Single);
        tree.click("two", SelectMode::Toggle);
        tree.context_click("one");
        assert_eq!(tree.selected_keys(), ["one", "two"]);
        let groups: Vec<_> = tree.selected_services().iter().map(|located| menu(&located.node.key)).collect();
        let items = selection_menu(&groups);
        assert_eq!(row(&items, "Run").run, Some(vec!["run one".to_string(), "run two".to_string()]));
        let remove = row(&items, "Remove from Services").run;
        assert_eq!(remove, Some(vec!["remove one".to_string(), "remove two".to_string()]));
        tree.context_click("three");
        assert_eq!(tree.selected_keys(), ["three"]);
        assert_eq!(tree.detail_slot().map(|slot| slot.key).as_deref(), Some("three"));
        let groups: Vec<_> = tree.selected_services().iter().map(|located| menu(&located.node.key)).collect();
        assert_eq!(selection_menu(&groups)[0].command(), Some(&vec!["run three".to_string()]));
    }

    #[test]
    fn batches_common_submenu_entries_across_providers() {
        let prune = |provider: &str| {
            let containers = ServiceMenuEntry::row("Containers", None, Some(provider.to_string()));
            vec![ServiceMenuEntry::submenu("Prune", None, vec![containers])]
        };
        let items = selection_menu(&[prune("a"), prune("b")]);
        let submenu = row(&items, "Prune").submenu;
        assert_eq!(submenu[0].command(), Some(&vec!["a".to_string(), "b".to_string()]));
    }

    #[test]
    fn ignores_removed_rows_and_avoids_invoking_a_selected_groups_descendants_twice() {
        let mut tree = ServicesTree::new();
        tree.set_provider_roots("test", 0, vec![group("group", vec![leaf("one")]), leaf("removed")], false);
        tree.expand_all();
        tree.click("group", SelectMode::Single);
        tree.click("one", SelectMode::Toggle);
        tree.click("removed", SelectMode::Toggle);
        tree.set_provider_roots("test", 0, vec![group("group", vec![leaf("one")])], false);
        let selected: Vec<&str> = tree.selected_services().iter().map(|located| located.node.key.as_str()).collect();
        assert_eq!(selected, ["group"]);
        assert_eq!(button(&toolbar(&tree), "Stop").commands, ["stop group"]);
    }

    #[test]
    fn disables_actions_when_no_selected_row_can_run_them() {
        let mut tree = three();
        tree.click("three", SelectMode::Single);
        assert!(button(&toolbar(&tree), "Stop").disabled);
    }

    #[test]
    fn disables_dialog_entries_for_a_multi_selection() {
        let edit = || vec![ServiceMenuEntry::row("Edit Configuration…", None, Some(()))];
        assert!(!selection_menu(&[edit()])[0].is_disabled());
        assert!(selection_menu(&[edit(), edit()])[0].is_disabled());
    }

    #[test]
    fn steps_through_rows_and_folds_with_the_arrows() {
        let mut tree = ServicesTree::new();
        tree.set_provider_roots("test", 0, vec![group("g", vec![leaf("a"), leaf("b")]), leaf("c")], false);
        assert_eq!(tree.step(1, false), [selects("test", "g")]);
        let open = tree.horizontal(true);
        assert_eq!(open, [ServiceRequest::Expand { provider: "test".to_string(), key: "g".to_string(), open: true }]);
        tree.horizontal(true);
        assert_eq!(tree.selected(), Some("a"));
        tree.step(isize::MAX, false);
        assert_eq!(tree.selected(), Some("c"));
        tree.step(-1, false);
        assert_eq!(tree.selected(), Some("b"));
        tree.horizontal(false);
        assert_eq!(tree.selected(), Some("g"));
        tree.horizontal(false);
        assert!(!tree.is_expanded("g"));
        assert_eq!(tree.rows().len(), 2);
        let mut empty = ServicesTree::new();
        assert!(empty.step(1, false).is_empty());
    }

    // service reordering

    fn reorderable() -> ServicesTree {
        let mut tree = ServicesTree::new();
        let groups = vec![group("g1", vec![leaf("a"), leaf("b")]), group("g2", vec![leaf("c")])];
        tree.set_provider_roots("run", 0, groups, true);
        tree.set_provider_roots("docker", 10, vec![group("docker:one", vec![leaf("docker:x")])], false);
        tree.expand_all();
        tree
    }

    #[test]
    fn offers_moves_for_single_rows_of_a_reorderable_provider_only() {
        let anything = |_: &str, _: &str| true;
        let mut tree = reorderable();
        tree.click("b", SelectMode::Single);
        let moves = tree.move_actions("b", &anything);
        assert_eq!(
            moves.iter().map(|action| (action.title.as_str(), action.disabled)).collect::<Vec<_>>(),
            [("Move Up", false), ("Move Down", true)]
        );
        let up = NodeCommand::Move {
            provider: "run".to_string(),
            source: "b".to_string(),
            target: "a".to_string(),
            position: DropPosition::Before,
        };
        assert_eq!(moves[0].command, Some(up));
        assert!(tree.move_actions("g1", &anything).is_empty());
        assert!(tree.move_actions("docker:x", &anything).is_empty());
        assert!(!tree.can_move("b", "c", &anything));
        tree.click("a", SelectMode::Toggle);
        assert!(!tree.single_selection());
        assert!(tree.move_actions("b", &|_: &str, _: &str| false)[0].disabled);
    }

    fn config(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: "java".into(), detected: true, ..blank_config(id, id, kind) }
    }

    /// A project with Spring Boot and npm added, its configurations `Gateway`, `Portal` (Spring
    /// Boot), `Web` and `Worker` (npm).
    fn run_world() -> (RunStore, RunServices) {
        let mut store = RunStore::new();
        store.set_configs(vec![
            config("Gateway", "spring-boot"),
            config("Portal", "spring-boot"),
            config("Web", "npm"),
            config("Worker", "npm"),
        ]);
        let types = r#"["spring-boot","npm"]"#;
        let state = RunServices::open(
            Some("/services"),
            Some("/services"),
            StoredServices { types: Some(types), ..StoredServices::default() },
        );
        (store, state)
    }

    fn run_tree(services: &Services<'_>) -> ServicesTree {
        let mut tree = ServicesTree::new();
        tree.set_provider_roots(RUN_PROVIDER, RUN_PROVIDER_ORDER, run_roots(services, &mut I18n::default()), true);
        tree.set_provider_roots(
            "docker",
            DOCKER_PROVIDER_ORDER,
            vec![ServiceNode::leaf("docker:one", "Docker", "file-docker")],
            false,
        );
        tree
    }

    fn labels(tree: &ServicesTree) -> Vec<String> {
        tree.rows().into_iter().map(|row| row.label).collect()
    }

    #[test]
    fn moves_detected_configurations_with_accessible_controls_and_retains_order_after_reopening() {
        let (store, mut state) = run_world();
        let types = RunConfigTypes::new();
        let portal = format!("{CONFIG_PREFIX}Portal");
        let (order, moved) = {
            let services = Services { store: &store, debug: &[], types: &types, state: &state };
            let mut tree = run_tree(&services);
            assert_eq!(labels(&tree), ["Spring Boot", "Gateway", "Portal", "npm", "Web", "Worker", "Docker"]);
            tree.click(&portal, SelectMode::Single);
            let can_move = |source: &str, target: &str| services.can_move(source, target);
            let moves = tree.move_actions(&portal, &can_move);
            let Some(NodeCommand::Move { source, target, position, .. }) = moves[0].command.clone() else {
                panic!("Move Up is enabled")
            };
            (services.moved(&source, &target, position), source)
        };
        assert_eq!(moved, portal);
        let written = state.set_order(order.expect("the order changes")).expect("the order is stored");
        assert_eq!(written.key, order_key("/services"));
        let services = Services { store: &store, debug: &[], types: &types, state: &state };
        let mut tree = run_tree(&services);
        assert_eq!(labels(&tree), ["Spring Boot", "Portal", "Gateway", "npm", "Web", "Worker", "Docker"]);
        tree.click(&portal, SelectMode::Single);
        assert!(tree.move_actions(&portal, &|source, target| services.can_move(source, target))[0].disabled);
        let types_stored = Some(r#"["spring-boot","npm"]"#);
        let stored = StoredServices { types: types_stored, order: written.value.as_deref(), ..Default::default() };
        let reopened = RunServices::open(Some("/services"), Some("/services"), stored);
        let services = Services { store: &store, debug: &[], types: &types, state: &reopened };
        let reordered = labels(&run_tree(&services));
        assert_eq!(reordered, ["Spring Boot", "Portal", "Gateway", "npm", "Web", "Worker", "Docker"]);
    }

    #[test]
    fn uses_the_same_ordering_for_npm_without_enabling_groups_or_docker() {
        let (store, state) = run_world();
        let types = RunConfigTypes::new();
        let services = Services { store: &store, debug: &[], types: &types, state: &state };
        let tree = run_tree(&services);
        let can_move = |source: &str, target: &str| services.can_move(source, target);
        let worker = format!("{CONFIG_PREFIX}Worker");
        let up = tree.move_actions(&worker, &can_move);
        assert!(!up[0].disabled && up[1].disabled);
        assert!(tree.move_actions("docker:one", &can_move).is_empty());
        assert!(tree.move_actions(&format!("{TYPE_PREFIX}spring-boot"), &can_move).is_empty());
    }

    #[test]
    fn preserves_multi_selection_without_offering_single_row_moves() {
        let (store, state) = run_world();
        let types = RunConfigTypes::new();
        let services = Services { store: &store, debug: &[], types: &types, state: &state };
        let mut tree = run_tree(&services);
        tree.click(&format!("{CONFIG_PREFIX}Portal"), SelectMode::Single);
        tree.click(&format!("{CONFIG_PREFIX}Gateway"), SelectMode::Toggle);
        assert_eq!(tree.selected_keys().len(), 2);
        assert!(!tree.single_selection());
        assert_eq!(labels(&tree), ["Spring Boot", "Gateway", "Portal", "npm", "Web", "Worker", "Docker"]);
    }

    // the run-configuration provider in Services

    fn debug_root(config: &RunConfig, id: u32, status: DebugStatus) -> DebugServiceSession {
        DebugServiceSession {
            id: DebugSessionId(id),
            config: config.clone(),
            started_at: f64::from(id),
            status,
            process_id: None,
            http_port: None,
            can_rerun: true,
        }
    }

    #[test]
    fn shows_debug_output_state_pins_actions_and_does_not_rebuild_the_tree_for_stdout() {
        let (mut store, state) = run_world();
        let gateway = config("Gateway", "spring-boot");
        store.set_configs(vec![gateway.clone()]);
        let types = RunConfigTypes::new();
        let key = format!("{CONFIG_PREFIX}Gateway");
        let running = [debug_root(&gateway, 1, DebugStatus::Running)];
        let services = Services { store: &store, debug: &running, types: &types, state: &state };
        let buttons = run_actions(&services, &key);
        let enabled = |id: &str| buttons.iter().any(|button| button.id == id && !button.disabled);
        assert!(enabled("stop") && enabled("run"));
        assert_eq!(services.invoke(&key, "run"), Some(ServiceCommand::RerunDebugger(DebugSessionId(1))));
        let paused = [debug_root(&gateway, 1, DebugStatus::Paused)];
        let services = Services { store: &store, debug: &paused, types: &types, state: &state };
        let tree = run_tree(&services);
        let gateway_row = tree.rows().into_iter().find(|row| row.key == key).expect("the row is shown");
        assert_eq!(gateway_row.detail.as_deref(), Some("Suspended"));
    }

    #[test]
    fn shows_the_service_port_announced_by_a_debug_sessions_application() {
        let (mut store, state) = run_world();
        let gateway = config("Gateway", "spring-boot");
        store.set_configs(vec![gateway.clone()]);
        let types = RunConfigTypes::new();
        let debug = [DebugServiceSession { http_port: Some(8080), ..debug_root(&gateway, 1, DebugStatus::Running) }];
        let services = Services { store: &store, debug: &debug, types: &types, state: &state };
        let row = run_tree(&services).rows().into_iter().find(|row| row.label == "Gateway").expect("shown");
        assert_eq!(row.detail.as_deref(), Some(":8080"));
    }

    #[test]
    fn debugs_a_service_in_place_without_switching_to_the_debug_tool_window() {
        let (mut store, state) = run_world();
        let gateway = config("Gateway", "spring-boot");
        store.set_configs(vec![gateway.clone()]);
        let types = RunConfigTypes::new();
        let services = Services { store: &store, debug: &[], types: &types, state: &state };
        let key = format!("{CONFIG_PREFIX}Gateway");
        let debug = run_actions(&services, &key).into_iter().find(|action| action.id == "debug").expect("Debug");
        let Some(NodeCommand::RunAction { action, .. }) = debug.command else { panic!("Debug runs a run action") };
        assert_eq!(services.invoke(&key, action), Some(ServiceCommand::DebugConfig(Box::new(gateway))));
    }

    #[test]
    fn reruns_a_debug_service_in_place_from_the_services_panel() {
        let (mut store, state) = run_world();
        let gateway = config("Gateway", "spring-boot");
        store.set_configs(vec![gateway.clone()]);
        let types = RunConfigTypes::new();
        let debug = [debug_root(&gateway, 4, DebugStatus::Running)];
        let services = Services { store: &store, debug: &debug, types: &types, state: &state };
        let key = format!("{CONFIG_PREFIX}Gateway");
        assert_eq!(services.invoke(&key, "run"), Some(ServiceCommand::RerunDebugger(DebugSessionId(4))));
        let menu = run_menu(&services, &key);
        let rerun = menu.iter().find_map(|entry| match entry {
            ServiceMenuEntry::Row(row) if row.label == "Rerun" => Some(row.clone()),
            _ => None,
        });
        let rerun = rerun.expect("the menu has Rerun");
        assert_eq!(rerun.id.as_deref(), Some("run"));
        assert_eq!(rerun.command, Some(NodeCommand::RunAction { key, action: "run" }));
    }

    #[test]
    fn group_stop_all_stops_all_roots_of_the_group() {
        let (mut store, state) = run_world();
        let gateway = config("Gateway", "spring-boot");
        store.set_configs(vec![gateway.clone()]);
        let types = RunConfigTypes::new();
        let shared = |id: u32| DebugServiceSession {
            process_id: Some(ide_core::RunId(7)),
            ..debug_root(&gateway, id, DebugStatus::Running)
        };
        let (first, second) = (shared(1), shared(2));
        let debug = [first, second];
        let services = Services { store: &store, debug: &debug, types: &types, state: &state };
        let group = format!("{TYPE_PREFIX}spring-boot");
        let stop_all = run_actions(&services, &group).into_iter().find(|action| action.id == "stop-all");
        assert!(!stop_all.expect("the group has Stop All").disabled);
        let Some(ServiceCommand::Stop(sessions)) = services.invoke(&group, "stop-all") else {
            panic!("Stop All stops the group's executions")
        };
        let roots: Vec<DebugSessionId> =
            sessions.iter().flat_map(|session| session.debug.iter().map(|debug| debug.id)).collect();
        assert_eq!(roots, [DebugSessionId(1), DebugSessionId(2)]);
    }

    #[test]
    fn lists_the_run_configuration_types_to_add() {
        let (store, state) = run_world();
        let types = RunConfigTypes::new();
        let services = Services { store: &store, debug: &[], types: &types, state: &state };
        let entries = run_add_menu(&services);
        let ServiceMenuEntry::Row(row) = &entries[0] else { panic!("a row") };
        assert_eq!(row.label, "Run Configuration Type");
        assert!(!row.disabled && !row.submenu.is_empty());
        let has_spring = row.submenu.iter().any(|entry| match entry {
            ServiceMenuEntry::Row(category) => category.submenu.iter().any(|ty| {
                matches!(ty,
                ServiceMenuEntry::Row(ServiceMenuRow { command: Some(NodeCommand::AddRunType(kind)), .. })
                    if kind == "spring-boot")
            }),
            ServiceMenuEntry::Separator => false,
        });
        assert!(!has_spring, "added types are not offered again");
    }
}
