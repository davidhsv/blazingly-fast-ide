//! Editor tabs and splits as state transitions (IDEA semantics):
//!  - one preview tab per pane (italic), replaced by the next preview, promoted to a regular tab
//!    when edited or opened explicitly, closed when another regular tab opens;
//!  - pinned tabs stay at the front and survive Close Others / Close Tabs to the Right / Close All
//!    but Pinned;
//!  - new tabs open right of the active tab; closing activates the left neighbour (or the right
//!    one / the most recently used, per settings);
//!  - splits form a tree of rows / columns; an emptied pane collapses away unless it is the last
//!    one.
//!
//! Transitions that change nothing return `false` (or `None`), so the owner knows when to repaint.

use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How many closed tabs Reopen Closed Tab remembers.
const MAX_CLOSED: usize = 50;

/// How many hidden tabs of keep-alive editor kinds a pane keeps built.
const KEPT_TABS: usize = 5;

/// What an editor tab shows. Tabs of editor kinds that are restored with the project are saved
/// with their whole input, so its JSON shape is part of the saved editor session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EditorInput {
    /// `"file"` for text documents.
    pub kind: String,
    /// Stable key; tabs with equal keys are the same tab.
    pub key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl EditorInput {
    /// Takes over `newer`: its `path` and `data` win where it has them, the present ones stay.
    fn update(&mut self, newer: EditorInput) {
        self.kind = newer.kind;
        self.key = newer.key;
        if newer.path.is_some() {
            self.path = newer.path;
        }
        if newer.data.is_some() {
            self.data = newer.data;
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tab {
    pub id: String,
    pub input: EditorInput,
    pub preview: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pane {
    pub id: String,
    pub tabs: Vec<Tab>,
    pub active: Option<String>,
    /// Tab ids, most recently used first.
    pub mru: Vec<String>,
}

impl Pane {
    pub fn tab(&self, tab_id: &str) -> Option<&Tab> {
        self.tabs.iter().find(|tab| tab.id == tab_id)
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active.as_deref().and_then(|id| self.tab(id))
    }

    /// Tabs kept built while hidden: the most recently used ones whose editor kind asks for it
    /// (`keep_alive`), in tab order so that switching tabs does not reorder them.
    pub fn kept_tabs(&self, keep_alive: impl Fn(&EditorInput) -> bool) -> Vec<&Tab> {
        let recent: Vec<&str> = self
            .mru
            .iter()
            .filter_map(|id| self.tab(id))
            .filter(|tab| keep_alive(&tab.input))
            .take(KEPT_TABS)
            .map(|tab| tab.id.as_str())
            .collect();
        self.tabs.iter().filter(|tab| recent.contains(&tab.id.as_str())).collect()
    }

    /// Editor Tabs › Show pinned tabs in a separate row: how many leading tabs form that row.
    /// `None` when all tabs share one row: the setting is off, or the pane lacks either pinned or
    /// other tabs.
    pub fn pinned_row(&self, separate_row: bool) -> Option<usize> {
        let pinned = self.pinned_count();
        (separate_row && pinned > 0 && pinned < self.tabs.len()).then_some(pinned)
    }

    fn position(&self, tab_id: &str) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.id == tab_id)
    }

    fn pinned_count(&self) -> usize {
        self.tabs.iter().filter(|tab| tab.pinned).count()
    }

    /// Length of the run of pinned tabs at the front.
    fn pinned_prefix(&self) -> usize {
        self.tabs.iter().take_while(|tab| tab.pinned).count()
    }

    fn touch(&mut self, tab_id: &str) {
        self.mru.retain(|id| id != tab_id);
        self.mru.insert(0, tab_id.to_owned());
        self.active = Some(tab_id.to_owned());
    }

    /// Takes a tab out without recording it as closed; the pane stays even when that empties it.
    fn detach(&mut self, tab_id: &str) {
        self.tabs.retain(|tab| tab.id != tab_id);
        self.mru.retain(|id| id != tab_id);
        if self.active.as_deref() == Some(tab_id) {
            self.active = self.mru.first().cloned().or_else(|| self.tabs.first().map(|tab| tab.id.clone()));
        }
    }

    /// Hands the slot of the tab `saved_id` to `live`: it takes that tab's id and input, keeping
    /// the slot's position, flags and place in the activation order.
    fn adopt(&mut self, saved_id: &str, live: &Tab) {
        if let Some(slot) = self.tabs.iter_mut().find(|tab| tab.id == saved_id) {
            slot.id.clone_from(&live.id);
            slot.input.clone_from(&live.input);
        }
        if self.active.as_deref() == Some(saved_id) {
            self.active = Some(live.id.clone());
        }
        for id in self.mru.iter_mut().filter(|id| id.as_str() == saved_id) {
            id.clone_from(&live.id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDir {
    /// Panes side by side.
    Row,
    /// Panes stacked.
    Column,
}

impl SplitDir {
    fn flipped(self) -> Self {
        match self {
            SplitDir::Row => SplitDir::Column,
            SplitDir::Column => SplitDir::Row,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutNode {
    Pane { pane_id: String },
    Split { id: String, dir: SplitDir, children: Vec<LayoutNode> },
}

impl LayoutNode {
    /// Pane ids in visual order (depth-first).
    pub fn pane_ids(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_pane_ids(&mut out);
        out
    }

    fn collect_pane_ids<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            LayoutNode::Pane { pane_id } => out.push(pane_id),
            LayoutNode::Split { children, .. } => children.iter().for_each(|child| child.collect_pane_ids(out)),
        }
    }
}

/// What a layout rewrite does with a node.
enum Mapped {
    /// Leave it; its children are visited.
    Keep,
    /// Put this in its place (`None` removes it); the replacement is not visited.
    Replace(Option<LayoutNode>),
}

/// Children of a split along `dir`, with nested splits in the same direction dissolved into it.
fn flatten(children: Vec<LayoutNode>, dir: SplitDir) -> Vec<LayoutNode> {
    children
        .into_iter()
        .flat_map(|child| match child {
            LayoutNode::Split { dir: nested, children, .. } if nested == dir => children,
            other => vec![other],
        })
        .collect()
}

/// Rewrites a layout top-down. A split left with one child becomes that child, one left with none
/// disappears.
fn map_layout(node: &LayoutNode, f: &mut dyn FnMut(&LayoutNode) -> Mapped) -> Option<LayoutNode> {
    if let Mapped::Replace(replacement) = f(node) {
        return replacement;
    }
    let LayoutNode::Split { id, dir, children } = node else { return Some(node.clone()) };
    let mut children: Vec<LayoutNode> = children.iter().filter_map(|child| map_layout(child, f)).collect();
    if children.len() < 2 {
        return children.pop();
    }
    Some(LayoutNode::Split { id: id.clone(), dir: *dir, children: flatten(children, *dir) })
}

/// A closed tab, for Reopen Closed Tab.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedTab {
    pub input: EditorInput,
    pub pane_id: String,
    pub index: usize,
    pub pinned: bool,
}

/// Editor Tabs › "When the current tab is closed, activate".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActivateOnClose {
    Mru,
    #[default]
    Left,
    Right,
}

impl ActivateOnClose {
    /// The value of the `editor.tabs.activateOnClose` setting; an unknown one behaves as `"mru"`.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "left" => ActivateOnClose::Left,
            "right" => ActivateOnClose::Right,
            _ => ActivateOnClose::Mru,
        }
    }
}

/// Editor Tabs › "Tab closing policy" when the tab limit is exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosingPolicy {
    /// The least recently used unmodified tab first.
    Unchanged,
    /// The least recently used tab.
    Lru,
}

impl ClosingPolicy {
    /// The value of the `editor.tabs.closingPolicy` setting; anything but `"unchanged"` is LRU.
    pub fn from_setting(value: &str) -> Self {
        if value == "unchanged" { ClosingPolicy::Unchanged } else { ClosingPolicy::Lru }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpenTabOptions {
    pub preview: bool,
    pub pinned: bool,
    /// Insert at this index instead of after the active tab.
    pub index: Option<usize>,
    pub activate: bool,
    /// Leave the pane's preview tab open although a regular tab opens.
    pub preserve_preview: bool,
}

impl Default for OpenTabOptions {
    fn default() -> Self {
        Self { preview: false, pinned: false, index: None, activate: true, preserve_preview: false }
    }
}

/// The edge of an editor a tab is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The tab Reopen Closed Tab brought back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reopened {
    pub tab_id: String,
    pub pane_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TabsState {
    pub layout: LayoutNode,
    /// In creation order.
    pub panes: Vec<Pane>,
    pub active_pane: String,
    /// Most recently closed last (Reopen Closed Tab pops).
    pub closed: Vec<ClosedTab>,
    /// Set from settings by the owner.
    pub activate_on_close: ActivateOnClose,
    /// Last number given to a pane, tab or split id.
    seq: u64,
}

impl Default for TabsState {
    fn default() -> Self {
        Self::new()
    }
}

impl TabsState {
    /// One empty pane.
    pub fn new() -> Self {
        let id = "pane1".to_owned();
        Self {
            layout: LayoutNode::Pane { pane_id: id.clone() },
            panes: vec![Pane { id: id.clone(), tabs: Vec::new(), active: None, mru: Vec::new() }],
            active_pane: id,
            closed: Vec::new(),
            activate_on_close: ActivateOnClose::default(),
            seq: 1,
        }
    }

    /// A state built elsewhere (a restored session). Its ids must not have the form of the ids
    /// this state generates (`pane`, `tab` or `split` followed by a number), or later ones could
    /// repeat them.
    pub fn from_parts(layout: LayoutNode, panes: Vec<Pane>, active_pane: String) -> Self {
        Self { layout, panes, active_pane, closed: Vec::new(), activate_on_close: ActivateOnClose::default(), seq: 0 }
    }

    fn next_id(&mut self, prefix: &str) -> String {
        self.seq += 1;
        format!("{prefix}{}", self.seq)
    }

    pub fn pane(&self, pane_id: &str) -> Option<&Pane> {
        self.panes.iter().find(|pane| pane.id == pane_id)
    }

    fn pane_mut(&mut self, pane_id: &str) -> Option<&mut Pane> {
        self.panes.iter_mut().find(|pane| pane.id == pane_id)
    }

    /// The tab showing `key` and its pane, looking in `pane_id` first, then in visual order.
    pub fn find_tab(&self, key: &str, pane_id: Option<&str>) -> Option<(&str, &Tab)> {
        let others = self.pane_ids().into_iter().filter(|id| Some(*id) != pane_id);
        pane_id
            .into_iter()
            .chain(others)
            .filter_map(|id| self.pane(id))
            .find_map(|pane| pane.tabs.iter().find(|tab| tab.input.key == key).map(|tab| (pane.id.as_str(), tab)))
    }

    /// Pane ids in visual order (depth-first through the layout).
    pub fn pane_ids(&self) -> Vec<&str> {
        self.layout.pane_ids()
    }

    /// Every distinct input key open in any pane.
    pub fn open_keys(&self) -> BTreeSet<&str> {
        self.panes.iter().flat_map(|pane| &pane.tabs).map(|tab| tab.input.key.as_str()).collect()
    }

    /// Replays the tabs of `current` into the saved layout `restored`: a live tab takes the slot
    /// saved for its key (keeping its own id), or opens in the saved active pane, where the tab
    /// active in `current` ends up active. The ids of the two states must not overlap.
    pub fn merge_session_tabs(current: &TabsState, mut restored: TabsState) -> TabsState {
        restored.seq = restored.seq.max(current.seq);
        restored.activate_on_close = current.activate_on_close;
        let mut live: Vec<&Tab> = Vec::new();
        for tab in current.panes.iter().flat_map(|pane| &pane.tabs) {
            match live.iter().position(|known| known.input.key == tab.input.key) {
                Some(known) => live[known] = tab,
                None => live.push(tab),
            }
        }
        if live.is_empty() {
            return restored;
        }
        let active = current.pane(&current.active_pane).and_then(Pane::active_tab);
        if let Some(active) = active
            && let Some(known) = live.iter_mut().find(|known| known.input.key == active.input.key)
        {
            *known = active;
        }
        let target = restored.active_pane.clone();
        let mut next = restored;
        for tab in live {
            let launch_target = active.is_some_and(|active| active.id == tab.id);
            let saved = next
                .find_tab(&tab.input.key, Some(target.as_str()))
                .map(|(pane_id, saved)| (pane_id.to_owned(), saved.id.clone(), saved.pinned, saved.preview));
            let (pane_id, saved_id) = match saved {
                Some((pane_id, saved_id, ..)) if !launch_target || pane_id == target => (pane_id, saved_id),
                elsewhere => {
                    // The launch target goes to the saved active pane; the pane it was saved in
                    // stays, even when that leaves it empty.
                    let (pinned, preview) = match elsewhere {
                        Some((pane_id, saved_id, pinned, preview)) => {
                            if let Some(source) = next.pane_mut(&pane_id) {
                                source.detach(&saved_id);
                            }
                            (pinned, preview)
                        }
                        None => (tab.pinned, tab.preview),
                    };
                    let no_preview = next.pane(&target).is_some_and(|pane| pane.tabs.iter().all(|t| !t.preview));
                    let preview = preview && no_preview;
                    let opts = OpenTabOptions { pinned, preview, index: None, activate: false, preserve_preview: true };
                    let Some(opened) = next.open_tab(&target, tab.input.clone(), opts) else { continue };
                    (target.clone(), opened)
                }
            };
            if let Some(pane) = next.pane_mut(&pane_id) {
                pane.adopt(&saved_id, tab);
            }
        }
        if let Some(active) = active {
            next.activate_tab(&target, &active.id);
        }
        next
    }

    /// Opens `input` in `pane_id`, or reuses the tab already showing its key there. Returns the
    /// tab's id; `None` when there is no such pane.
    pub fn open_tab(&mut self, pane_id: &str, input: EditorInput, opts: OpenTabOptions) -> Option<String> {
        let at = self.panes.iter().position(|pane| pane.id == pane_id)?;
        if let Some(existing) = self.panes[at].tabs.iter().position(|tab| tab.input.key == input.key) {
            let pane = &mut self.panes[at];
            let tab = &mut pane.tabs[existing];
            if !opts.preview && tab.preview {
                tab.preview = false;
                tab.input = input;
            } else {
                tab.input.update(input);
            }
            let id = tab.id.clone();
            if opts.activate {
                pane.touch(&id);
                self.active_pane = pane_id.to_owned();
            }
            return Some(id);
        }
        let id = self.next_id("tab");
        let pane = &mut self.panes[at];
        let tab = Tab { id: id.clone(), input, preview: opts.preview && !opts.pinned, pinned: opts.pinned };
        let is_preview = tab.preview;
        let preview_slot = if is_preview { pane.tabs.iter().position(|t| t.preview) } else { None };
        if let Some(slot) = preview_slot {
            // The preview slot is reused in place.
            let replaced = std::mem::replace(&mut pane.tabs[slot], tab);
            pane.mru.retain(|used| *used != replaced.id);
        } else {
            let pins = pane.pinned_prefix();
            let index = if tab.pinned {
                pins
            } else if let Some(index) = opts.index {
                pins.max(index.min(pane.tabs.len()))
            } else {
                let after_active = pane.active.as_deref().and_then(|active| pane.position(active)).map_or(0, |i| i + 1);
                pins.max(after_active)
            };
            pane.tabs.insert(index, tab);
        }
        if !is_preview && !opts.preserve_preview {
            // A new regular editor closes the preview tab (IDEA trims previews on every open).
            let previews: Vec<String> = pane.tabs.iter().filter(|t| t.preview).map(|t| t.id.clone()).collect();
            pane.tabs.retain(|t| !t.preview);
            pane.mru.retain(|used| !previews.contains(used));
            if pane.active.as_ref().is_some_and(|active| previews.contains(active)) {
                pane.active = None;
            }
        }
        if opts.activate {
            pane.touch(&id);
            self.active_pane = pane_id.to_owned();
        } else {
            pane.mru.push(id.clone());
            if pane.active.is_none() {
                pane.active = Some(id.clone());
            }
        }
        Some(id)
    }

    /// Makes `tab_id` the active tab of `pane_id` and that pane the active one.
    pub fn activate_tab(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let pane_is_active = self.active_pane == pane_id;
        let Some(pane) = self.pane_mut(pane_id) else { return false };
        if pane.tab(tab_id).is_none() {
            return false;
        }
        let most_recent = pane.mru.first().is_some_and(|id| id == tab_id);
        if pane_is_active && most_recent && pane.active.as_deref() == Some(tab_id) {
            return false;
        }
        pane.touch(tab_id);
        self.active_pane = pane_id.to_owned();
        true
    }

    pub fn set_active_pane(&mut self, pane_id: &str) -> bool {
        if self.active_pane == pane_id || self.pane(pane_id).is_none() {
            return false;
        }
        self.active_pane = pane_id.to_owned();
        true
    }

    /// Turns a preview tab into a regular one.
    pub fn promote_tab(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let tab = self.pane_mut(pane_id).and_then(|pane| pane.tabs.iter_mut().find(|tab| tab.id == tab_id));
        let Some(tab) = tab.filter(|tab| tab.preview) else { return false };
        tab.preview = false;
        true
    }

    /// Pinning moves the tab to the end of the pinned group; unpinning right after it.
    pub fn toggle_pin(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let Some(pane) = self.pane_mut(pane_id) else { return false };
        let Some(at) = pane.position(tab_id) else { return false };
        let mut tab = pane.tabs.remove(at);
        tab.pinned = !tab.pinned;
        tab.preview = false;
        let pins = pane.pinned_count();
        pane.tabs.insert(pins, tab);
        true
    }

    /// Ids of the tabs of `pane_id` that `select` (given index and tab) picks.
    fn tab_ids(&self, pane_id: &str, select: impl Fn(usize, &Tab) -> bool) -> Vec<String> {
        let Some(pane) = self.pane(pane_id) else { return Vec::new() };
        pane.tabs.iter().enumerate().filter(|(index, tab)| select(*index, tab)).map(|(_, tab)| tab.id.clone()).collect()
    }

    /// Closes the tabs `ids` of `pane_id`, remembering them for Reopen Closed Tab; a pane left
    /// without tabs collapses away unless it is the last one.
    fn remove_tabs(&mut self, pane_id: &str, ids: &[String]) -> bool {
        if ids.is_empty() {
            return false;
        }
        let activate_on_close = self.activate_on_close;
        let Some(pane) = self.pane_mut(pane_id) else { return false };
        let closing_active = pane.active.as_ref().is_some_and(|active| ids.contains(active));
        let neighbour = pane
            .active
            .as_deref()
            .filter(|_| closing_active)
            .and_then(|active| pane.position(active))
            .and_then(|at| {
                let left = pane.tabs[..at].iter().rev().find(|tab| !ids.contains(&tab.id));
                let right = pane.tabs[at + 1..].iter().find(|tab| !ids.contains(&tab.id));
                match activate_on_close {
                    ActivateOnClose::Left => left.or(right),
                    ActivateOnClose::Right => right.or(left),
                    ActivateOnClose::Mru => None,
                }
            })
            .map(|tab| tab.id.clone());
        let mut closed = Vec::new();
        let mut kept = Vec::with_capacity(pane.tabs.len());
        for (index, tab) in std::mem::take(&mut pane.tabs).into_iter().enumerate() {
            if ids.contains(&tab.id) {
                closed.push(ClosedTab { input: tab.input, pane_id: pane_id.to_owned(), index, pinned: tab.pinned });
            } else {
                kept.push(tab);
            }
        }
        pane.tabs = kept;
        pane.mru.retain(|id| !ids.contains(id));
        if closing_active {
            pane.active =
                neighbour.or_else(|| pane.mru.first().cloned()).or_else(|| pane.tabs.first().map(|tab| tab.id.clone()));
        }
        let emptied = pane.tabs.is_empty();
        let removed = !closed.is_empty();
        // The leftmost of several tabs closed together is the first to reopen.
        self.closed.extend(closed.into_iter().rev());
        let excess = self.closed.len().saturating_sub(MAX_CLOSED);
        self.closed.drain(..excess);
        let collapsed = emptied && self.remove_pane(pane_id);
        removed || collapsed
    }

    pub fn close_tab(&mut self, pane_id: &str, tab_id: &str) -> bool {
        self.remove_tabs(pane_id, &[tab_id.to_owned()])
    }

    /// Closes every unpinned tab but `tab_id`, which becomes the active tab of the active pane.
    pub fn close_others(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let ids = self.tab_ids(pane_id, |_, tab| tab.id != tab_id && !tab.pinned);
        let removed = self.remove_tabs(pane_id, &ids);
        let activated = self.activate_tab(pane_id, tab_id);
        removed || activated
    }

    pub fn close_all(&mut self, pane_id: &str, keep_pinned: bool) -> bool {
        let ids = self.tab_ids(pane_id, |_, tab| !keep_pinned || !tab.pinned);
        self.remove_tabs(pane_id, &ids)
    }

    pub fn close_to_right(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let Some(at) = self.pane(pane_id).and_then(|pane| pane.position(tab_id)) else { return false };
        let ids = self.tab_ids(pane_id, |index, tab| index > at && !tab.pinned);
        self.remove_tabs(pane_id, &ids)
    }

    pub fn close_to_left(&mut self, pane_id: &str, tab_id: &str) -> bool {
        let Some(at) = self.pane(pane_id).and_then(|pane| pane.position(tab_id)) else { return false };
        let ids = self.tab_ids(pane_id, |index, tab| index < at && !tab.pinned);
        self.remove_tabs(pane_id, &ids)
    }

    /// IDEA "Close Unmodified Tabs": closes the unpinned tabs `modified` says no to.
    pub fn close_unmodified(&mut self, pane_id: &str, modified: impl Fn(&Tab) -> bool) -> bool {
        let ids = self.tab_ids(pane_id, |_, tab| !tab.pinned && !modified(tab));
        self.remove_tabs(pane_id, &ids)
    }

    /// Closes every tab showing `key` in all panes (file deleted, document closed).
    pub fn close_key(&mut self, key: &str) -> bool {
        let panes: Vec<String> = self.pane_ids().into_iter().map(str::to_owned).collect();
        let mut changed = false;
        for pane_id in &panes {
            let ids = self.tab_ids(pane_id, |_, tab| tab.input.key == key);
            changed |= self.remove_tabs(pane_id, &ids);
        }
        changed
    }

    /// Reopens the most recently closed tab that is not open in its pane any more, at its old
    /// position (in the active pane when its pane is gone).
    pub fn reopen_closed(&mut self) -> Option<Reopened> {
        let still_open = |entry: &ClosedTab| {
            self.pane(&entry.pane_id).is_some_and(|pane| pane.tabs.iter().any(|tab| tab.input.key == entry.input.key))
        };
        let at = self.closed.iter().rposition(|entry| !still_open(entry))?;
        self.closed.truncate(at + 1);
        let entry = self.closed.pop()?;
        let pane_id = if self.pane(&entry.pane_id).is_some() { entry.pane_id } else { self.active_pane.clone() };
        let opts = OpenTabOptions { index: Some(entry.index), pinned: entry.pinned, ..OpenTabOptions::default() };
        let tab_id = self.open_tab(&pane_id, entry.input, opts)?;
        Some(Reopened { tab_id, pane_id })
    }

    /// Tab limit: closes tabs of `pane_id` beyond `limit` (0: no limit); pinned tabs, the active
    /// one, `keep` and `is_protected` tabs stay. A tab never activated counts as the least
    /// recently used.
    pub fn enforce_tab_limit(
        &mut self,
        pane_id: &str,
        limit: usize,
        policy: ClosingPolicy,
        is_dirty: impl Fn(&Tab) -> bool,
        keep: Option<&str>,
        is_protected: impl Fn(&Tab) -> bool,
    ) -> bool {
        if limit == 0 {
            return false;
        }
        let mut changed = false;
        loop {
            let Some(pane) = self.pane(pane_id) else { return changed };
            if pane.tabs.iter().filter(|tab| !tab.pinned).count() <= limit {
                return changed;
            }
            let mut closable: Vec<&Tab> = pane
                .tabs
                .iter()
                .filter(|tab| !tab.pinned && Some(tab.id.as_str()) != keep && Some(&tab.id) != pane.active.as_ref())
                .filter(|tab| !is_protected(tab))
                .collect();
            closable.sort_by_key(|tab| Reverse(pane.mru.iter().position(|used| *used == tab.id).unwrap_or(usize::MAX)));
            let unmodified = match policy {
                ClosingPolicy::Unchanged => closable.iter().find(|tab| !is_dirty(tab)),
                ClosingPolicy::Lru => None,
            };
            let Some(victim) = unmodified.or(closable.first()) else { return changed };
            let victim = victim.id.clone();
            changed |= self.remove_tabs(pane_id, &[victim]);
        }
    }

    /// Editor Tabs › Sort tabs alphabetically: orders the pinned and the other tabs of a pane by
    /// `title`.
    pub fn sort_tabs(&mut self, pane_id: &str, title: impl Fn(&Tab) -> String) -> bool {
        let Some(pane) = self.pane_mut(pane_id) else { return false };
        let before: Vec<String> = pane.tabs.iter().map(|tab| tab.id.clone()).collect();
        let mut titled: Vec<(String, Tab)> =
            std::mem::take(&mut pane.tabs).into_iter().map(|tab| (title(&tab), tab)).collect();
        titled.sort_by(|(a, x), (b, y)| y.pinned.cmp(&x.pinned).then_with(|| compare_titles(a, b)));
        pane.tabs = titled.into_iter().map(|(_, tab)| tab).collect();
        pane.tabs.iter().map(|tab| &tab.id).ne(before.iter())
    }

    /// Activates the tab `delta` places after the active one, wrapping around; in the active pane
    /// unless `pane_id` names one.
    pub fn cycle_tab(&mut self, delta: isize, pane_id: Option<&str>) -> bool {
        let pane_id = pane_id.unwrap_or(&self.active_pane).to_owned();
        let Some(pane) = self.pane(&pane_id) else { return false };
        if pane.tabs.len() < 2 {
            return false;
        }
        let len = pane.tabs.len() as isize;
        let at = pane.active.as_deref().and_then(|active| pane.position(active)).map_or(-1, |i| i as isize);
        let target = pane.tabs[(at + delta).rem_euclid(len) as usize].id.clone();
        self.activate_tab(&pane_id, &target)
    }

    /// Moves a tab (drag & drop) to `index` of `to_pane` and activates it there. `pin` set (a
    /// drop on the pinned or the other row of a split tab bar): the tab takes that state and
    /// lands inside the matching group. A pane that already shows the tab's input keeps its own
    /// tab, which becomes active.
    pub fn move_tab(&mut self, from_pane: &str, tab_id: &str, to_pane: &str, index: usize, pin: Option<bool>) -> bool {
        let Some(at) = self.pane(from_pane).and_then(|pane| pane.position(tab_id)) else { return false };
        if from_pane == to_pane {
            let Some(pane) = self.pane_mut(from_pane) else { return false };
            let tab = pane.tabs.remove(at);
            place(&mut pane.tabs, tab, if index > at { index - 1 } else { index }, pin);
            pane.touch(tab_id);
            self.active_pane = to_pane.to_owned();
            return true;
        }
        let Some(tab) = self.pane(from_pane).and_then(|pane| pane.tabs.get(at)).cloned() else { return false };
        let Some(destination) = self.pane(to_pane) else { return false };
        let duplicate = destination.tabs.iter().find(|t| t.input.key == tab.input.key).map(|t| t.id.clone());
        self.remove_unrecorded(from_pane, tab_id);
        if let Some(duplicate) = duplicate {
            self.activate_tab(to_pane, &duplicate);
        } else if let Some(pane) = self.pane_mut(to_pane) {
            place(&mut pane.tabs, tab, index, pin);
            pane.touch(tab_id);
        }
        self.active_pane = to_pane.to_owned();
        true
    }

    /// Takes a tab out of its pane as closing does, but without remembering it as closed.
    fn remove_unrecorded(&mut self, pane_id: &str, tab_id: &str) {
        let closed = std::mem::take(&mut self.closed);
        self.remove_tabs(pane_id, &[tab_id.to_owned()]);
        self.closed = closed;
    }

    /// Splits `pane_id` and opens a copy of its active tab in the new pane, which becomes active.
    /// Returns the new pane's id.
    pub fn split_pane(&mut self, pane_id: &str, dir: SplitDir) -> Option<String> {
        let active = self.pane(pane_id)?.active_tab().cloned();
        let id = self.next_id("pane");
        let copy = active.map(|tab| Tab { id: self.next_id("tab"), preview: false, pinned: false, ..tab });
        self.add_pane(pane_id, id.clone(), dir, false, copy.into_iter().collect());
        Some(id)
    }

    /// Puts the new pane `id`, holding `tabs` (the first one active), next to `anchor` in a split
    /// along `dir` and makes it the active pane.
    fn add_pane(&mut self, anchor: &str, id: String, dir: SplitDir, before: bool, tabs: Vec<Tab>) {
        let split = self.next_id("split");
        let layout = map_layout(&self.layout, &mut |node: &LayoutNode| match node {
            LayoutNode::Pane { pane_id } if pane_id == anchor => {
                let added = LayoutNode::Pane { pane_id: id.clone() };
                let children = if before { vec![added, node.clone()] } else { vec![node.clone(), added] };
                Mapped::Replace(Some(LayoutNode::Split { id: split.clone(), dir, children }))
            }
            _ => Mapped::Keep,
        });
        if let Some(layout) = layout {
            self.layout = layout;
        }
        let active = tabs.first().map(|tab| tab.id.clone());
        self.panes.push(Pane { id: id.clone(), mru: active.iter().cloned().collect(), active, tabs });
        self.active_pane = id;
    }

    /// Removes a pane with its tabs; the last pane stays. The pane left of it (else right of it)
    /// becomes active when it was.
    pub fn remove_pane(&mut self, pane_id: &str) -> bool {
        let order = self.pane_ids();
        if order.len() <= 1 {
            return false;
        }
        let Some(at) = self.panes.iter().position(|pane| pane.id == pane_id) else { return false };
        let spot = order.iter().position(|id| *id == pane_id);
        let beside = spot.and_then(|i| i.checked_sub(1)).or(Some(spot.map_or(0, |i| i + 1)));
        let neighbour = beside.and_then(|i| order.get(i)).map(|id| (*id).to_owned());
        let layout = map_layout(&self.layout, &mut |node: &LayoutNode| match node {
            LayoutNode::Pane { pane_id: id } if id == pane_id => Mapped::Replace(None),
            _ => Mapped::Keep,
        });
        if let Some(layout) = layout {
            self.layout = layout;
        }
        self.panes.remove(at);
        if self.active_pane == pane_id
            && let Some(next) = neighbour.or_else(|| self.panes.first().map(|pane| pane.id.clone()))
        {
            self.active_pane = next;
        }
        true
    }

    /// Flips the split that directly holds `pane_id` between side by side and stacked.
    pub fn change_split_orientation(&mut self, pane_id: &str) -> bool {
        let mut changed = false;
        let flipped = map_layout(&self.layout, &mut |node: &LayoutNode| {
            let LayoutNode::Split { id, dir, children } = node else { return Mapped::Keep };
            let holds = |child: &LayoutNode| matches!(child, LayoutNode::Pane { pane_id: held } if held == pane_id);
            if !children.iter().any(holds) {
                return Mapped::Keep;
            }
            changed = true;
            // The flipped split absorbs children that now run its way; its parent absorbs it
            // when the two now run the same way.
            let dir = dir.flipped();
            Mapped::Replace(Some(LayoutNode::Split { id: id.clone(), dir, children: flatten(children.clone(), dir) }))
        });
        if changed && let Some(layout) = flipped {
            self.layout = layout;
        }
        changed
    }

    /// Removes every pane except `keep_pane`.
    pub fn unsplit_all(&mut self, keep_pane: &str) -> bool {
        if self.pane(keep_pane).is_none() {
            return false;
        }
        self.panes.retain(|pane| pane.id == keep_pane);
        self.layout = LayoutNode::Pane { pane_id: keep_pane.to_owned() };
        self.active_pane = keep_pane.to_owned();
        true
    }

    /// The pane right of / below `pane_id`, else the one before it (for "open to the side"). An
    /// unknown id gives the first pane.
    pub fn neighbour_pane(&self, pane_id: &str) -> Option<&str> {
        let order = self.pane_ids();
        let at = order.iter().position(|id| *id == pane_id);
        let after = order.get(at.map_or(0, |i| i + 1));
        after.or_else(|| at.and_then(|i| i.checked_sub(1)).and_then(|i| order.get(i))).copied()
    }

    /// Drag & drop onto an editor edge: moves the tab into a new pane split off `target_pane` on
    /// that side. The tab keeps its id (and editor state). Returns the new pane's id; `None` when
    /// there is nothing to move or it is the only tab of the target pane.
    pub fn move_tab_to_new_split(
        &mut self,
        from_pane: &str,
        tab_id: &str,
        target_pane: &str,
        edge: SplitEdge,
    ) -> Option<String> {
        let source = self.pane(from_pane)?;
        let tab = source.tab(tab_id)?;
        self.pane(target_pane)?;
        if from_pane == target_pane && source.tabs.len() == 1 {
            return None;
        }
        let moved = Tab { preview: false, ..tab.clone() };
        self.remove_unrecorded(from_pane, tab_id);
        // The source pane may have collapsed away; split the target if it survived, else the
        // active pane.
        let anchor = if self.pane(target_pane).is_some() { target_pane.to_owned() } else { self.active_pane.clone() };
        Some(self.split_off(&anchor, edge, vec![moved]))
    }

    /// A new empty pane at `edge` of `anchor`, made active. Returns its id.
    pub fn split_at_edge(&mut self, anchor: &str, edge: SplitEdge) -> Option<String> {
        self.pane(anchor)?;
        Some(self.split_off(anchor, edge, Vec::new()))
    }

    fn split_off(&mut self, anchor: &str, edge: SplitEdge, tabs: Vec<Tab>) -> String {
        let id = self.next_id("pane");
        let (dir, before) = match edge {
            SplitEdge::Left => (SplitDir::Row, true),
            SplitEdge::Right => (SplitDir::Row, false),
            SplitEdge::Top => (SplitDir::Column, true),
            SplitEdge::Bottom => (SplitDir::Column, false),
        };
        self.add_pane(anchor, id.clone(), dir, before, tabs);
        id
    }
}

/// Inserts `tab` at `at`. With `pin` set the tab takes that state and stays inside the matching
/// group.
fn place(tabs: &mut Vec<Tab>, mut tab: Tab, at: usize, pin: Option<bool>) {
    let pins = tabs.iter().filter(|t| t.pinned).count();
    let clamped = at.min(tabs.len());
    let index = match pin {
        None => clamped,
        Some(true) => clamped.min(pins),
        Some(false) => clamped.max(pins),
    };
    if let Some(pin) = pin {
        tab.pinned = pin;
        tab.preview = tab.preview && !pin;
    }
    tabs.insert(index, tab);
}

/// Alphabetical order ignoring case; titles that differ in case only put the lowercase one first.
fn compare_titles(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| b.cmp(a))
}

fn base_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |slash| &path[slash + 1..])
}

/// Parent directory names of `path`, nearest first.
fn parents(path: &str) -> Vec<&str> {
    let mut segments: Vec<&str> = path.split('/').collect();
    segments.pop();
    segments.retain(|segment| !segment.is_empty());
    segments.reverse();
    segments
}

/// The `depth` nearest parents as a path.
fn parent_path(dirs: &[&str], depth: usize) -> String {
    let nearest = &dirs[..depth.min(dirs.len())];
    nearest.iter().rev().copied().collect::<Vec<_>>().join("/")
}

/// Whether none of the parent lists other than the one at `own` satisfies `clashes`.
fn stands_out(dirs: &[Vec<&str>], own: usize, clashes: impl Fn(&[&str]) -> bool) -> bool {
    dirs.iter().enumerate().all(|(other, list)| other == own || !clashes(list))
}

/// IDEA-style editor tab titles for files that share a name: for every path whose file name
/// occurs more than once in `paths`, the nearest parent directory that tells it apart
/// (`ide-lsp` for `crates/ide-lsp/src/lib.rs` next to `crates/ide-vcs/src/lib.rs`), or the
/// shortest distinguishing parent path when no single directory does. Paths with a unique name
/// are absent.
pub fn disambiguate_titles<S: AsRef<str>>(paths: &[S]) -> BTreeMap<String, String> {
    let unique: BTreeSet<&str> = paths.iter().map(AsRef::as_ref).collect();
    let mut groups: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for path in unique {
        groups.entry(base_name(path)).or_default().push(path);
    }
    let mut hints = BTreeMap::new();
    for group in groups.values().filter(|group| group.len() > 1) {
        let dirs: Vec<Vec<&str>> = group.iter().copied().map(parents).collect();
        for (i, path) in group.iter().enumerate() {
            let own = &dirs[i];
            let single = |depth: &usize| stands_out(&dirs, i, |other| other.get(*depth) == own.get(*depth));
            let hint = match (0..own.len()).find(single) {
                Some(depth) => own[depth].to_owned(),
                None => (2..=own.len())
                    .find(|&depth| stands_out(&dirs, i, |other| parent_path(other, depth) == parent_path(own, depth)))
                    .map_or_else(
                        || if own.is_empty() { "/".to_owned() } else { parent_path(own, own.len()) },
                        |depth| parent_path(own, depth),
                    ),
            };
            hints.insert((*path).to_owned(), hint);
        }
    }
    hints
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn file(path: &str) -> EditorInput {
        EditorInput { kind: "file".to_owned(), key: format!("file:{path}"), path: Some(path.to_owned()), data: None }
    }

    fn open_with(s: &mut TabsState, path: &str, opts: OpenTabOptions) -> String {
        let pane = s.active_pane.clone();
        s.open_tab(&pane, file(path), opts).unwrap()
    }

    fn open(s: &mut TabsState, path: &str) -> String {
        open_with(s, path, OpenTabOptions::default())
    }

    fn opened(paths: &[&str]) -> TabsState {
        let mut s = TabsState::new();
        for path in paths {
            open(&mut s, path);
        }
        s
    }

    fn preview() -> OpenTabOptions {
        OpenTabOptions { preview: true, ..OpenTabOptions::default() }
    }

    fn labels_in(s: &TabsState, pane: &str) -> Vec<String> {
        let label = |tab: &Tab| {
            let path = tab.input.path.as_deref().unwrap_or_default();
            format!("{path}{}{}", if tab.preview { "*" } else { "" }, if tab.pinned { "^" } else { "" })
        };
        s.pane(pane).unwrap().tabs.iter().map(label).collect()
    }

    fn labels(s: &TabsState) -> Vec<String> {
        labels_in(s, &s.active_pane)
    }

    fn active(s: &TabsState) -> Option<&str> {
        s.pane(&s.active_pane).unwrap().active_tab().and_then(|tab| tab.input.path.as_deref())
    }

    /// Id of the tab showing `path`, looking in the active pane first.
    fn id_of(s: &TabsState, path: &str) -> String {
        s.find_tab(&format!("file:{path}"), Some(s.active_pane.as_str())).unwrap().1.id.clone()
    }

    fn id_at(s: &TabsState, index: usize) -> String {
        s.pane(&s.active_pane).unwrap().tabs[index].id.clone()
    }

    // src/editor/core/model/model.test.ts › tab model › opens new tabs right of the active one and reuses existing tabs
    #[test]
    fn opens_new_tabs_right_of_the_active_one_and_reuses_existing_tabs() {
        let mut s = opened(&["a", "b", "c"]);
        open(&mut s, "a");
        open(&mut s, "d");
        assert_eq!(labels(&s), ["a", "d", "b", "c"]);
        assert_eq!(active(&s), Some("d"));
    }

    // src/editor/core/model/model.test.ts › tab model › keeps a single preview tab that the next preview replaces
    #[test]
    fn keeps_a_single_preview_tab_that_the_next_preview_replaces() {
        let mut s = opened(&["a"]);
        open_with(&mut s, "p1", preview());
        open_with(&mut s, "p2", preview());
        assert_eq!(labels(&s), ["a", "p2*"]);
        // Opening explicitly (or editing) promotes the preview.
        open(&mut s, "p2");
        assert_eq!(labels(&s), ["a", "p2"]);
        let p3 = open_with(&mut s, "p3", preview());
        let pane = s.active_pane.clone();
        assert!(s.pane(&pane).unwrap().tab(&p3).unwrap().preview);
        s.promote_tab(&pane, &p3);
        assert_eq!(labels(&s), ["a", "p2", "p3"]);
    }

    // src/editor/core/model/model.test.ts › tab model › pins tabs to the front and protects them from bulk close
    #[test]
    fn pins_tabs_to_the_front_and_protects_them_from_bulk_close() {
        let mut s = opened(&["a", "b", "c", "d"]);
        let pane = s.active_pane.clone();
        let c = id_at(&s, 2);
        s.toggle_pin(&pane, &c);
        assert_eq!(labels(&s), ["c^", "a", "b", "d"]);
        let b = id_at(&s, 2);
        s.close_others(&pane, &b);
        assert_eq!(labels(&s), ["c^", "b"]);
        assert_eq!(active(&s), Some("b"));
        open(&mut s, "e");
        let first = id_at(&s, 0);
        s.close_to_right(&pane, &first);
        assert_eq!(labels(&s), ["c^"]);
        open(&mut s, "f");
        s.close_all(&pane, true);
        assert_eq!(labels(&s), ["c^"]);
        s.close_all(&pane, false);
        assert!(labels(&s).is_empty());
    }

    // src/editor/core/model/model.test.ts › tab model › closes tabs to the left and unmodified tabs, keeping pinned ones
    #[test]
    fn closes_tabs_to_the_left_and_unmodified_tabs_keeping_pinned_ones() {
        let mut s = opened(&["a", "b", "c", "d"]);
        let pane = s.active_pane.clone();
        let a = id_at(&s, 0);
        s.toggle_pin(&pane, &a);
        let c = id_at(&s, 2);
        s.close_to_left(&pane, &c);
        assert_eq!(labels(&s), ["a^", "c", "d"]);
        open(&mut s, "e");
        s.close_unmodified(&pane, |tab| tab.input.path.as_deref() == Some("d"));
        assert_eq!(labels(&s), ["a^", "d"]);
    }

    // src/editor/core/model/model.test.ts › tab model › activates the left neighbour when closing the active tab (IDEA default)
    #[test]
    fn activates_the_left_neighbour_when_closing_the_active_tab() {
        let mut s = opened(&["a", "b", "c"]);
        let pane = s.active_pane.clone();
        open(&mut s, "a");
        let c = open(&mut s, "c");
        s.close_tab(&pane, &c);
        assert_eq!(active(&s), Some("b"));
        // The first tab has no left neighbour: the right one.
        let a = open(&mut s, "a");
        s.close_tab(&pane, &a);
        assert_eq!(active(&s), Some("b"));
    }

    // src/editor/core/model/model.test.ts › tab model › activates the most recently used tab when closing the active one if set
    #[test]
    fn activates_the_most_recently_used_tab_when_closing_the_active_one_if_set() {
        let mut s = TabsState::new();
        s.activate_on_close = ActivateOnClose::Mru;
        for path in ["a", "b", "c"] {
            open(&mut s, path);
        }
        let pane = s.active_pane.clone();
        open(&mut s, "a");
        let c = open(&mut s, "c");
        s.close_tab(&pane, &c);
        assert_eq!(active(&s), Some("a"));
    }

    // src/editor/core/model/model.test.ts › tab model › closes the preview tab when a regular tab opens
    #[test]
    fn closes_the_preview_tab_when_a_regular_tab_opens() {
        let mut s = opened(&["a"]);
        open_with(&mut s, "p", preview());
        open(&mut s, "b");
        assert_eq!(labels(&s), ["a", "b"]);
        assert_eq!(active(&s), Some("b"));
        assert_eq!(s.pane(&s.active_pane).unwrap().mru.len(), 2);
        // Reopening an existing regular tab keeps the preview.
        open_with(&mut s, "q", preview());
        open(&mut s, "a");
        assert_eq!(labels(&s), ["a", "b", "q*"]);
    }

    // src/editor/core/model/model.test.ts › tab model › reopens closed tabs at their position, most recent first
    #[test]
    fn reopens_closed_tabs_at_their_position_most_recent_first() {
        let mut s = opened(&["a", "b", "c"]);
        let pane = s.active_pane.clone();
        let (b, c) = (id_of(&s, "b"), id_of(&s, "c"));
        s.close_tab(&pane, &b);
        s.close_tab(&pane, &c);
        assert!(s.reopen_closed().is_some());
        assert_eq!(labels(&s), ["a", "c"]);
        assert!(s.reopen_closed().is_some());
        assert_eq!(labels(&s), ["a", "b", "c"]);
        assert_eq!(s.reopen_closed(), None);
    }

    // src/editor/core/model/model.test.ts › tab model › splits, moves tabs across panes and collapses empty panes
    #[test]
    fn splits_moves_tabs_across_panes_and_collapses_empty_panes() {
        let mut s = opened(&["a", "b"]);
        let first = s.active_pane.clone();
        let split = s.split_pane(&first, SplitDir::Row).unwrap();
        assert_eq!(s.pane_ids(), [first.as_str(), split.as_str()]);
        assert_eq!(labels_in(&s, &split), ["b"]);
        assert_eq!(s.active_pane, split);
        let a = s.find_tab("file:a", Some(first.as_str())).unwrap().1.id.clone();
        s.move_tab(&first, &a, &split, 0, None);
        assert_eq!(labels_in(&s, &split), ["a", "b"]);
        assert_eq!(labels_in(&s, &first), ["b"]);
        let last = s.pane(&first).unwrap().tabs[0].id.clone();
        s.close_tab(&first, &last);
        assert_eq!(s.pane_ids(), [split.as_str()]);
        assert_eq!(s.layout, LayoutNode::Pane { pane_id: split });
    }

    /// Direction of the root split, then of each child (`"pane"` for a pane).
    fn dirs(s: &TabsState) -> Vec<&'static str> {
        let name = |dir: &SplitDir| match dir {
            SplitDir::Row => "row",
            SplitDir::Column => "column",
        };
        let LayoutNode::Split { dir, children, .. } = &s.layout else { return Vec::new() };
        let children = children.iter().map(|child| match child {
            LayoutNode::Split { dir, .. } => name(dir),
            LayoutNode::Pane { .. } => "pane",
        });
        std::iter::once(name(dir)).chain(children).collect()
    }

    // src/editor/core/model/model.test.ts › tab model › flips the orientation of a split and unsplits everything
    #[test]
    fn flips_the_orientation_of_a_split_and_unsplits_everything() {
        let mut s = opened(&["a"]);
        let first = s.active_pane.clone();
        let right = s.split_pane(&first, SplitDir::Row).unwrap();
        let below = s.split_pane(&right, SplitDir::Column).unwrap();
        assert_eq!(dirs(&s), ["row", "pane", "column"]);
        // The nested split turns side by side and merges into the outer row.
        s.change_split_orientation(&below);
        assert_eq!(dirs(&s), ["row", "pane", "pane", "pane"]);
        s.change_split_orientation(&first);
        assert_eq!(dirs(&s), ["column", "pane", "pane", "pane"]);
        let mut single = opened(&["x"]);
        assert!(!single.change_split_orientation("nope"));
        assert!(matches!(single.layout, LayoutNode::Pane { .. }));
        s.unsplit_all(&right);
        assert_eq!(s.pane_ids(), [right.as_str()]);
        assert_eq!(s.active_pane, right);
        assert_eq!(labels(&s), ["a"]);
    }

    // src/editor/core/model/model.test.ts › tab model › closes a key everywhere and cycles tabs
    #[test]
    fn closes_a_key_everywhere_and_cycles_tabs() {
        let mut s = opened(&["a", "b"]);
        let first = s.active_pane.clone();
        s.split_pane(&first, SplitDir::Column);
        s.close_key("file:b");
        assert_eq!(s.pane_ids().len(), 1);
        assert_eq!(labels(&s), ["a"]);
        open(&mut s, "c");
        s.cycle_tab(1, None);
        assert_eq!(active(&s), Some("a"));
        s.cycle_tab(-1, None);
        assert_eq!(active(&s), Some("c"));
    }

    // src/editor/core/model/model.test.ts › tab model › sorts tabs alphabetically, pinned ones first
    #[test]
    fn sorts_tabs_alphabetically_pinned_ones_first() {
        let mut s = opened(&["c", "B", "a", "d"]);
        let pane = s.active_pane.clone();
        let d = id_of(&s, "d");
        s.toggle_pin(&pane, &d);
        let title = |tab: &Tab| tab.input.path.clone().unwrap_or_default();
        assert!(s.sort_tabs(&pane, title));
        assert_eq!(labels(&s), ["d^", "a", "B", "c"]);
        assert!(!s.sort_tabs(&pane, title));
    }

    /// `a^ b^ | c d e` (`^` = pinned).
    fn pinned_and_plain() -> TabsState {
        let mut s = opened(&["a", "b", "c", "d", "e"]);
        let pane = s.active_pane.clone();
        for path in ["a", "b"] {
            let id = id_of(&s, path);
            s.toggle_pin(&pane, &id);
        }
        s
    }

    // src/editor/core/model/model.test.ts › moveTab between the pinned row and the other row › it.each:
    // a plain tab onto the pinned row; a plain tab to the front of the pinned row; a plain tab to the end of the
    // pinned row; a pinned tab onto the other row; a pinned tab to the start of the other row; a plain tab inside
    // the other row keeps it plain; an index past the pinned group stays in it
    #[test]
    fn moves_tabs_between_the_pinned_row_and_the_other_row() {
        let cases: [(&str, usize, bool, [&str; 5]); 7] = [
            ("d", 1, true, ["a^", "d^", "b^", "c", "e"]),
            ("e", 0, true, ["e^", "a^", "b^", "c", "d"]),
            ("c", 2, true, ["a^", "b^", "c^", "d", "e"]),
            ("a", 4, false, ["b^", "c", "d", "a", "e"]),
            ("b", 2, false, ["a^", "b", "c", "d", "e"]),
            ("c", 5, false, ["a^", "b^", "d", "e", "c"]),
            ("d", 4, true, ["a^", "b^", "d^", "c", "e"]),
        ];
        for (path, index, pin, expected) in cases {
            let mut s = pinned_and_plain();
            let pane = s.active_pane.clone();
            let id = id_of(&s, path);
            s.move_tab(&pane, &id, &pane, index, Some(pin));
            assert_eq!(labels(&s), expected, "{path} to {index}, pin {pin}");
        }
    }

    // src/editor/core/model/model.test.ts › moveTab between the pinned row and the other row › leaves the pinned state alone without a target row
    #[test]
    fn move_leaves_the_pinned_state_alone_without_a_target_row() {
        let mut s = pinned_and_plain();
        let pane = s.active_pane.clone();
        let d = id_of(&s, "d");
        s.move_tab(&pane, &d, &pane, 0, None);
        assert_eq!(labels(&s), ["d", "a^", "b^", "c", "e"]);
    }

    // src/editor/core/model/session.test.ts › moveTabToNewSplit › moves a tab into a new pane on the given edge
    #[test]
    fn moves_a_tab_into_a_new_pane_on_the_given_edge() {
        let mut s = opened(&["/a", "/b"]);
        let pane = s.active_pane.clone();
        let b = id_at(&s, 1);
        let added = s.move_tab_to_new_split(&pane, &b, &pane, SplitEdge::Left).unwrap();
        assert_eq!(s.pane_ids(), [added.as_str(), pane.as_str()]);
        assert_eq!(s.pane(&added).unwrap().tabs[0].id, b);
        assert_eq!(labels_in(&s, &pane), ["/a"]);
        assert_eq!(s.active_pane, added);
        assert!(s.closed.is_empty());
    }

    // src/editor/core/model/session.test.ts › moveTabToNewSplit › is a no-op for the only tab of the target pane
    #[test]
    fn moving_the_only_tab_of_the_target_pane_to_a_new_split_is_a_no_op() {
        let mut s = opened(&["/a"]);
        let pane = s.active_pane.clone();
        let only = id_at(&s, 0);
        assert_eq!(s.move_tab_to_new_split(&pane, &only, &pane, SplitEdge::Right), None);
    }

    // src/editor/core/model/session.test.ts › moveTabToNewSplit › splits the target when the source pane empties and collapses
    #[test]
    fn splits_the_target_when_the_source_pane_empties_and_collapses() {
        let mut s = opened(&["/a"]);
        let first = s.active_pane.clone();
        let split = s.split_pane(&first, SplitDir::Row).unwrap();
        let b = open(&mut s, "/b");
        let source = s.panes.iter_mut().find(|pane| pane.id == split).unwrap();
        source.tabs.retain(|tab| tab.id == b);
        source.active = Some(b.clone());
        source.mru = vec![b.clone()];
        let added = s.move_tab_to_new_split(&split, &b, &first, SplitEdge::Bottom).unwrap();
        assert!(s.pane(&split).is_none());
        assert!(matches!(s.layout, LayoutNode::Split { dir: SplitDir::Column, .. }));
        assert_eq!(s.pane_ids(), [first.as_str(), added.as_str()]);
    }

    // src/editor/core/model/session.test.ts › splitAtEdge › adds an empty active pane on the given side
    #[test]
    fn split_at_edge_adds_an_empty_active_pane_on_the_given_side() {
        let mut s = opened(&["/a"]);
        let pane = s.active_pane.clone();
        let added = s.split_at_edge(&pane, SplitEdge::Right).unwrap();
        assert_eq!(s.pane_ids(), [pane.as_str(), added.as_str()]);
        assert!(s.pane(&added).unwrap().tabs.is_empty());
        assert_eq!(s.active_pane, added);
        assert_eq!(labels_in(&s, &pane), ["/a"]);
    }

    // src/editor/core/cm/prefs.test.ts › editor settings behaviour › closes unchanged least recently used tabs beyond the limit
    #[test]
    fn closes_unchanged_least_recently_used_tabs_beyond_the_limit() {
        let mut s = TabsState::new();
        let pane = s.active_pane.clone();
        let ids: Vec<String> = ["a", "b", "c"]
            .into_iter()
            .map(|p| {
                let input = EditorInput { key: p.to_owned(), ..file(p) };
                s.open_tab(&pane, input, OpenTabOptions::default()).unwrap()
            })
            .collect();
        let dirty = |tab: &Tab| tab.input.path.as_deref() == Some("a");
        s.enforce_tab_limit(&pane, 2, ClosingPolicy::Unchanged, dirty, Some(ids[2].as_str()), |_| false);
        let keys: Vec<&str> = s.pane(&pane).unwrap().tabs.iter().map(|tab| tab.input.key.as_str()).collect();
        assert_eq!(keys, ["a", "c"]);
    }

    /// A pane with one tab per `(key, kind)`, each activated once in that order.
    fn pane_of_kinds(tabs: &[(&str, &str)]) -> TabsState {
        let mut s = TabsState::new();
        let pane = s.active_pane.clone();
        for (key, kind) in tabs {
            let input = EditorInput { kind: (*kind).to_owned(), key: (*key).to_owned(), path: None, data: None };
            s.open_tab(&pane, input, OpenTabOptions::default());
        }
        s
    }

    fn show(s: &mut TabsState, key: &str) {
        let (pane, tab) = s.find_tab(key, None).map(|(pane, tab)| (pane.to_owned(), tab.id.clone())).unwrap();
        s.activate_tab(&pane, &tab);
    }

    fn kept(s: &TabsState) -> Vec<&str> {
        let pane = s.pane(&s.active_pane).unwrap();
        pane.kept_tabs(|input| input.kind == "kept").iter().map(|tab| tab.input.key.as_str()).collect()
    }

    // src/editor/core/view/keptTabs.test.tsx › keep-alive editor kinds › keep their recent tabs mounted and hidden while another tab is active
    #[test]
    fn keep_alive_kinds_keep_their_recent_tabs_while_another_tab_is_active() {
        let mut s = pane_of_kinds(&[("a", "kept"), ("b", "kept"), ("c", "plain")]);
        for key in ["a", "b", "c"] {
            show(&mut s, key);
        }
        assert_eq!(kept(&s), ["a", "b"]);
        assert_eq!(s.pane(&s.active_pane).unwrap().active_tab().unwrap().input.key, "c");
        show(&mut s, "a");
        show(&mut s, "c");
        // Only the plain tab is built again; the kept ones are shown as they were.
        assert_eq!(kept(&s), ["a", "b"]);
    }

    // src/editor/core/view/keptTabs.test.tsx › keep-alive editor kinds › let the least recently used kept tabs go beyond the limit
    #[test]
    fn keep_alive_kinds_let_the_least_recently_used_kept_tabs_go_beyond_the_limit() {
        let keys = ["k0", "k1", "k2", "k3", "k4", "k5", "k6"];
        let mut s = pane_of_kinds(&keys.map(|key| (key, "kept")));
        show(&mut s, "k0");
        for key in keys {
            show(&mut s, key);
        }
        assert_eq!(kept(&s), ["k2", "k3", "k4", "k5", "k6"]);
        assert_eq!(s.pane(&s.active_pane).unwrap().active_tab().unwrap().input.key, "k6");
    }

    fn pane_with_pins(pinned: &[bool]) -> Pane {
        let tabs: Vec<Tab> = pinned
            .iter()
            .enumerate()
            .map(|(i, &pinned)| saved_tab(&format!("t{}", i + 1), &format!("/p/f{}.txt", i + 1), pinned, false))
            .collect();
        let mru: Vec<String> = tabs.iter().map(|tab| tab.id.clone()).collect();
        Pane { id: "p1".to_owned(), active: mru.first().cloned(), mru, tabs }
    }

    // src/editor/core/view/pinnedTabsRow.test.tsx › pinned tabs in a separate row › it.each:
    // $placement multipleRows=$multipleRows pinnedRow=$pinnedSeparateRow → $rows strip(s)
    #[test]
    fn pinned_tabs_take_a_separate_row_when_the_setting_asks_for_it() {
        let pane = pane_with_pins(&[true, true, false, false, false]);
        // Two strips: the two pinned tabs, then the other three.
        assert_eq!(pane.pinned_row(true), Some(2));
        assert_eq!(pane.tabs[2..].len(), 3);
        assert_eq!(pane.pinned_row(false), None);
    }

    // src/editor/core/view/pinnedTabsRow.test.tsx › pinned tabs in a separate row › it.each: every tab pinned → one strip;
    // no tab pinned → one strip
    #[test]
    fn only_pinned_or_only_other_tabs_share_one_row() {
        assert_eq!(pane_with_pins(&[true, true, true]).pinned_row(true), None);
        assert_eq!(pane_with_pins(&[false, false, false]).pinned_row(true), None);
    }

    fn saved_tab(id: &str, path: &str, pinned: bool, preview: bool) -> Tab {
        Tab { id: id.to_owned(), input: file(path), preview, pinned }
    }

    /// A pane as a restored session builds it: the active tab most recently used, then the rest.
    fn saved_pane(id: &str, tabs: Vec<Tab>, active: usize) -> Pane {
        let active = tabs[active].id.clone();
        let rest = tabs.iter().map(|tab| tab.id.clone()).filter(|tab| *tab != active);
        let mru = std::iter::once(active.clone()).chain(rest).collect();
        Pane { id: id.to_owned(), tabs, active: Some(active), mru }
    }

    fn saved_single(tabs: Vec<Tab>) -> TabsState {
        let layout = LayoutNode::Pane { pane_id: "paner1".to_owned() };
        TabsState::from_parts(layout, vec![saved_pane("paner1", tabs, 0)], "paner1".to_owned())
    }

    /// `first | [c* shared]`, the second pane active.
    fn saved_split(dir: SplitDir, first: Pane) -> TabsState {
        let second = saved_pane(
            "paner2",
            vec![saved_tab("tabr3", "/p/c.ts", false, true), saved_tab("tabr4", "/p/shared.ts", false, false)],
            1,
        );
        let children =
            vec![LayoutNode::Pane { pane_id: first.id.clone() }, LayoutNode::Pane { pane_id: second.id.clone() }];
        let layout = LayoutNode::Split { id: "splitr1".to_owned(), dir, children };
        TabsState::from_parts(layout, vec![first, second], "paner2".to_owned())
    }

    /// `[a^ shared*]`: with `saved_split`, both panes show `shared`.
    fn saved_first_pane() -> Pane {
        saved_pane(
            "paner1",
            vec![saved_tab("tabr1", "/p/a.ts", true, false), saved_tab("tabr2", "/p/shared.ts", false, true)],
            1,
        )
    }

    fn tabs_showing<'a>(s: &'a TabsState, path: &str) -> Vec<&'a Tab> {
        s.panes.iter().flat_map(|pane| &pane.tabs).filter(|tab| tab.input.path.as_deref() == Some(path)).collect()
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › it.each(row, column):
    // keeps the saved %s split and intentional copies, with the CLI file active in the saved active pane
    #[test]
    fn late_restore_keeps_the_saved_split_and_copies_with_the_live_file_active_in_the_saved_active_pane() {
        for dir in [SplitDir::Row, SplitDir::Column] {
            let mut current = TabsState::new();
            let live = open(&mut current, "/p/b.ts");
            let s = TabsState::merge_session_tabs(&current, saved_split(dir, saved_first_pane()));
            assert!(matches!(&s.layout, LayoutNode::Split { dir: saved, .. } if *saved == dir));
            assert_eq!(s.pane_ids().len(), 2);
            assert_eq!(s.active_pane, s.pane_ids()[1]);
            assert_eq!(tabs_showing(&s, "/p/shared.ts").len(), 2);
            assert_eq!(active(&s), Some("/p/b.ts"));
            assert_eq!(s.pane(&s.active_pane).unwrap().active.as_deref(), Some(live.as_str()));
        }
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › keeps pins before regular tabs, including after a subsequent pin and open
    #[test]
    fn late_restore_keeps_pins_before_regular_tabs_including_after_a_subsequent_pin_and_open() {
        let mut current = TabsState::new();
        open(&mut current, "/p/b.ts");
        let saved = saved_single(vec![saved_tab("tabr1", "/p/a.ts", true, false)]);
        let mut s = TabsState::merge_session_tabs(&current, saved);
        let pins = |s: &TabsState| -> Vec<bool> {
            s.pane(&s.active_pane).unwrap().tabs.iter().map(|tab| tab.pinned).collect()
        };
        assert_eq!(pins(&s), [true, false]);
        let pane = s.active_pane.clone();
        let active_tab = s.pane(&pane).unwrap().active.clone().unwrap();
        s.toggle_pin(&pane, &active_tab);
        open(&mut s, "/p/c.ts");
        assert_eq!(pins(&s), [true, true, false]);
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › keeps at most one preview per pane when the live tab and both saved panes have previews
    #[test]
    fn late_restore_keeps_at_most_one_preview_per_pane() {
        let mut current = TabsState::new();
        open_with(&mut current, "/p/b.ts", preview());
        let s = TabsState::merge_session_tabs(&current, saved_split(SplitDir::Row, saved_first_pane()));
        for pane in &s.panes {
            assert!(pane.tabs.iter().filter(|tab| tab.preview).count() <= 1);
        }
        assert!(s.open_keys().contains("file:/p/b.ts"));
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › keeps the live same-key slot and launch target instead of the saved caret
    #[test]
    fn late_restore_keeps_the_live_tab_in_the_slot_saved_for_its_key() {
        let mut current = TabsState::new();
        let live = open(&mut current, "/p/b.ts");
        let saved = saved_single(vec![saved_tab("tabr1", "/p/b.ts", true, false)]);
        let s = TabsState::merge_session_tabs(&current, saved);
        assert_eq!(labels(&s), ["/p/b.ts^"]);
        assert_eq!(s.pane(&s.active_pane).unwrap().active.as_deref(), Some(live.as_str()));
        assert_eq!(id_at(&s, 0), live);
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › moves a saved same-key launch target into the saved active pane without losing the split
    #[test]
    fn late_restore_moves_a_saved_same_key_launch_target_into_the_saved_active_pane() {
        let mut current = TabsState::new();
        let live = open(&mut current, "/p/b.ts");
        let first = saved_pane("paner1", vec![saved_tab("tabr1", "/p/b.ts", true, false)], 0);
        let s = TabsState::merge_session_tabs(&current, saved_split(SplitDir::Row, first));
        assert!(matches!(s.layout, LayoutNode::Split { dir: SplitDir::Row, .. }));
        assert_eq!(s.active_pane, s.pane_ids()[1]);
        assert_eq!(tabs_showing(&s, "/p/b.ts").len(), 1);
        let first_tab = &s.pane(&s.active_pane).unwrap().tabs[0];
        assert!(first_tab.pinned);
        assert_eq!(first_tab.id, live);
        assert_eq!(active(&s), Some("/p/b.ts"));
    }

    // src/editor/core/lightedit-launch.test.ts › late session restore › takes the saved state unchanged when the window is empty
    #[test]
    fn late_restore_takes_the_saved_state_unchanged_when_the_window_is_empty() {
        let saved = saved_split(SplitDir::Row, saved_first_pane());
        let s = TabsState::merge_session_tabs(&TabsState::new(), saved.clone());
        assert_eq!(s.pane_ids().len(), 2);
        assert_eq!(s.panes, saved.panes);
        assert_eq!(s.layout, saved.layout);
        assert_eq!(active(&s), Some("/p/shared.ts"));
    }

    #[test]
    fn an_editor_input_keeps_its_saved_json_shape() {
        let data = Some(json!({ "big": true }));
        let diff = EditorInput { kind: "diff".to_owned(), key: "diff:x".to_owned(), path: None, data };
        let saved = json!({ "kind": "diff", "key": "diff:x", "data": { "big": true } });
        assert_eq!(serde_json::to_value(&diff).unwrap(), saved);
        assert_eq!(serde_json::from_value::<EditorInput>(saved).unwrap(), diff);
        let text = json!({ "kind": "file", "key": "file:/p/a.ts", "path": "/p/a.ts" });
        assert_eq!(serde_json::from_value::<EditorInput>(text.clone()).unwrap(), file("/p/a.ts"));
        assert_eq!(serde_json::to_value(file("/p/a.ts")).unwrap(), text);
    }

    // src/editor/core/model/tabTitles.test.ts › disambiguateTitles › leaves unique names alone
    #[test]
    fn disambiguate_titles_leaves_unique_names_alone() {
        assert!(disambiguate_titles(&["/p/a/main.rs", "/p/b/lib.rs"]).is_empty());
    }

    // src/editor/core/model/tabTitles.test.ts › disambiguateTitles › uses the nearest distinguishing directory
    #[test]
    fn disambiguate_titles_uses_the_nearest_distinguishing_directory() {
        let paths = ["/p/crates/ide-lsp/src/lib.rs", "/p/crates/ide-vcs/src/lib.rs", "/p/src/main.rs"];
        let hints = disambiguate_titles(&paths);
        assert_eq!(hints.get("/p/crates/ide-lsp/src/lib.rs").map(String::as_str), Some("ide-lsp"));
        assert_eq!(hints.get("/p/crates/ide-vcs/src/lib.rs").map(String::as_str), Some("ide-vcs"));
        assert!(!hints.contains_key("/p/src/main.rs"));
    }

    // src/editor/core/model/tabTitles.test.ts › disambiguateTitles › uses the parent directly when it differs
    #[test]
    fn disambiguate_titles_uses_the_parent_directly_when_it_differs() {
        let hints = disambiguate_titles(&["/p/a/index.ts", "/p/b/index.ts"]);
        assert_eq!(hints.get("/p/a/index.ts").map(String::as_str), Some("a"));
        assert_eq!(hints.get("/p/b/index.ts").map(String::as_str), Some("b"));
    }

    // src/editor/core/model/tabTitles.test.ts › disambiguateTitles › falls back to a parent path when no single directory is unique
    #[test]
    fn disambiguate_titles_falls_back_to_a_parent_path_when_no_single_directory_is_unique() {
        let hints = disambiguate_titles(&["/r/a/b/f.ts", "/r/c/b/f.ts", "/r/a/d/f.ts"]);
        assert_eq!(hints.get("/r/a/b/f.ts").map(String::as_str), Some("a/b"));
        assert_eq!(hints.get("/r/c/b/f.ts").map(String::as_str), Some("c"));
        assert_eq!(hints.get("/r/a/d/f.ts").map(String::as_str), Some("d"));
    }

    // src/editor/core/model/tabTitles.test.ts › disambiguateTitles › ignores duplicates of the same path
    #[test]
    fn disambiguate_titles_ignores_duplicates_of_the_same_path() {
        assert!(disambiguate_titles(&["/p/a/x.ts", "/p/a/x.ts"]).is_empty());
    }
}
