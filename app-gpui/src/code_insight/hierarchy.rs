//! The call hierarchy (⌃⌥H) and type hierarchy (⌃H): a tree whose levels load when expanded.
//!
//! Each level is a [`HierarchyRequest`] for the caller to carry out, tagged with a ticket; its answer
//! comes back with [`Hierarchy::level_answered`]. Every (re)query of the root bumps the tree's
//! generation: answers for an older tree are dropped and its call hierarchy requests cancelled.

use super::usage_search::{UsageHint, error_usage_hint};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::lsp::result_note;
use crate::platform::i18n::{I18n, Param};
use crate::platform::virtual_docs::basename;
use ide_core::error::ErrorKind;
use ide_core::ipc_path::to_ipc;
use ide_core::{DocId, IdeError, OutlineItem, Pos, RequestId, SymbolKind};
use ide_lsp::types::{HierarchyDirection, HierarchyItem, ReferencesIncomplete};
use std::collections::{HashMap, HashSet};

/// Which hierarchy the tool window shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum HierarchyMode {
    #[default]
    Call,
    Type,
}

/// The id of the root's level.
pub const ROOT_ID: &str = "root";

/// The symbol the hierarchy is of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyRoot {
    pub name: String,
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// The caret a hierarchy is asked from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyCaret {
    pub doc: DocId,
    pub path: String,
    pub pos: Pos,
    /// The identifier at the caret, the root's name.
    pub word: Option<String>,
}

/// A node of the tree.
#[derive(Debug, Clone)]
pub struct HierarchyNode {
    pub id: String,
    pub item: HierarchyItem,
    /// The node repeats an ancestor: it stays a leaf, marked "recursive".
    pub recursion: bool,
    /// The position keys of the node and its ancestors (a child repeating one is a cycle).
    keys: Vec<String>,
}

/// A level's answer.
#[derive(Debug, Clone, Default)]
pub struct LevelAnswer {
    pub items: Vec<HierarchyItem>,
    pub incomplete: Option<ReferencesIncomplete>,
    /// Text matches: same-named symbols are not told apart.
    pub approximate: bool,
}

/// What the tree needs from the backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyRequest {
    /// The level below the symbol at `pos` of `path`: `lsp_call_hierarchy(request, doc, pos,
    /// direction)` with the `request` given, or `lsp_type_hierarchy(doc, pos, direction)`. `doc` is
    /// the open editor's document of `path`, else one opened for the call.
    Level {
        ticket: u64,
        mode: HierarchyMode,
        request: Option<RequestId>,
        path: String,
        pos: Pos,
        direction: HierarchyDirection,
    },
    /// `doc_outline(doc)`, to find the method or type enclosing the caret.
    Outline { ticket: u64, doc: DocId },
}

/// What the tree needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyEffect {
    ShowToolWindow,
    Request(HierarchyRequest),
    Cancel(RequestId),
    Hint(UsageHint),
}

#[derive(Debug, Clone)]
enum Pending {
    Level { generation: u64, parent: String, keys: Vec<String>, name: String, request: Option<RequestId>, quiet: bool },
    Outline { generation: u64, caret: HierarchyCaret },
}

/// A visible row of the tree.
#[derive(Debug, Clone)]
pub enum HierarchyRow {
    Root { expanded: bool },
    Node { node: Box<HierarchyNode>, depth: usize, expanded: bool, expandable: bool },
}

impl HierarchyRow {
    pub fn id(&self) -> &str {
        match self {
            Self::Root { .. } => ROOT_ID,
            Self::Node { node, .. } => &node.id,
        }
    }
}

/// The hierarchy tool window's tree.
#[derive(Debug, Clone)]
pub struct Hierarchy {
    mode: HierarchyMode,
    direction: HierarchyDirection,
    root: Option<HierarchyRoot>,
    children: HashMap<String, Vec<HierarchyNode>>,
    failed: HashSet<String>,
    notes: HashMap<String, String>,
    loading: bool,
    generation: u64,
    tickets: u64,
    pending: HashMap<u64, Pending>,
    /// The caret of the last Show: when nothing is there, the enclosing declaration is tried.
    retry: Option<HierarchyCaret>,
    light_mode: bool,
}

impl Default for Hierarchy {
    fn default() -> Self {
        Self::new(false)
    }
}

fn root_key(root: &HierarchyRoot) -> String {
    format!("{}:{}:{}:{}", root.path, root.line, root.col, root.name)
}

/// Position-unique key of an item: the same symbol always has the same one.
fn item_key(item: &HierarchyItem) -> String {
    let at = item.selection_range.start;
    format!("{}:{}:{}:{}", to_ipc(&item.path), at.line, at.col, item.name)
}

/// The id of `item` below `parent`: the parent chain and the item's position.
pub fn child_id(parent: &str, item: &HierarchyItem) -> String {
    format!("{parent}/{}", item_key(item))
}

/// The empty-result line of the tree.
pub fn none_found_label(mode: HierarchyMode, direction: HierarchyDirection) -> &'static str {
    match (mode, direction) {
        (HierarchyMode::Call, HierarchyDirection::Incoming) => "— no callers found",
        (HierarchyMode::Call, HierarchyDirection::Outgoing) => "— no callees found",
        (HierarchyMode::Type, HierarchyDirection::Incoming) => "— no supertypes found",
        (HierarchyMode::Type, HierarchyDirection::Outgoing) => "— no subtypes found",
    }
}

/// What a direction lists: Callers, Callees, Supertypes or Subtypes.
pub fn direction_label(mode: HierarchyMode, direction: HierarchyDirection) -> &'static str {
    match (mode, direction) {
        (HierarchyMode::Call, HierarchyDirection::Incoming) => "Callers",
        (HierarchyMode::Call, HierarchyDirection::Outgoing) => "Callees",
        (HierarchyMode::Type, HierarchyDirection::Incoming) => "Supertypes",
        (HierarchyMode::Type, HierarchyDirection::Outgoing) => "Subtypes",
    }
}

/// The tab title of the tree, e.g. "Callers of main".
pub fn hierarchy_title(mode: HierarchyMode, direction: HierarchyDirection, name: &str, i18n: &mut I18n) -> String {
    let prefix = format!("{} of ", direction_label(mode, direction));
    i18n.tx_built(&[&prefix, ""], &[Param::from(name)])
}

/// The toolbar button of a direction, e.g. "Callers Hierarchy".
pub fn direction_button_label(mode: HierarchyMode, direction: HierarchyDirection, i18n: &mut I18n) -> String {
    let label = i18n.tx(direction_label(mode, direction));
    i18n.tx_built(&["", " Hierarchy"], &[Param::from(label)])
}

fn before(a: Pos, b: Pos) -> bool {
    a <= b
}

/// The innermost outline symbol of `kinds` whose extent contains `pos`.
pub fn enclosing_declaration<'a>(items: &'a [OutlineItem], pos: Pos, kinds: &[SymbolKind]) -> Option<&'a OutlineItem> {
    let item = items.iter().find(|item| before(item.range.start, pos) && before(pos, item.range.end))?;
    enclosing_declaration(&item.children, pos, kinds).or_else(|| kinds.contains(&item.kind).then_some(item))
}

const CALLABLE: [SymbolKind; 3] = [SymbolKind::Function, SymbolKind::Method, SymbolKind::Constructor];
const TYPES: [SymbolKind; 5] =
    [SymbolKind::Class, SymbolKind::Interface, SymbolKind::Struct, SymbolKind::Enum, SymbolKind::Trait];

/// Where activating a node goes: a caller's call site (as IDEA), else the symbol itself.
pub fn hierarchy_target(
    mode: HierarchyMode,
    direction: HierarchyDirection,
    item: &HierarchyItem,
) -> (String, u32, u32) {
    let at = match (mode, direction, item.from_ranges.first()) {
        (HierarchyMode::Call, HierarchyDirection::Incoming, Some(site)) => site.start,
        _ => item.selection_range.start,
    };
    (to_ipc(&item.path), at.line, at.col)
}

fn hint(text: &str) -> HierarchyEffect {
    HierarchyEffect::Hint(UsageHint { text: text.to_owned(), tone: Tone::Info })
}

impl Hierarchy {
    pub fn new(light_mode: bool) -> Self {
        Self {
            mode: HierarchyMode::Call,
            direction: HierarchyDirection::Incoming,
            root: None,
            children: HashMap::new(),
            failed: HashSet::new(),
            notes: HashMap::new(),
            loading: false,
            generation: 0,
            tickets: 0,
            pending: HashMap::new(),
            retry: None,
            light_mode,
        }
    }

    pub fn set_light_mode(&mut self, light_mode: bool) {
        self.light_mode = light_mode;
    }

    pub fn mode(&self) -> HierarchyMode {
        self.mode
    }

    pub fn direction(&self) -> HierarchyDirection {
        self.direction
    }

    pub fn root(&self) -> Option<&HierarchyRoot> {
        self.root.as_ref()
    }

    /// The loaded children of `parent` (`None`: not loaded yet).
    pub fn children(&self, parent: &str) -> Option<&[HierarchyNode]> {
        self.children.get(parent).map(Vec::as_slice)
    }

    /// Whether loading `parent`'s children failed.
    pub fn failed(&self, parent: &str) -> bool {
        self.failed.contains(parent)
    }

    /// Why `parent`'s children may be missing some.
    pub fn note(&self, parent: &str) -> Option<&str> {
        self.notes.get(parent).map(String::as_str)
    }

    pub fn notes_len(&self) -> usize {
        self.notes.len()
    }

    /// The root's level is being asked for.
    pub fn loading(&self) -> bool {
        self.loading
    }

    /// Bumped per (re)query of the root: the view resets the tree's expansion then.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Whether `id` has children to show or still to load.
    pub fn is_expandable(&self, id: &str) -> bool {
        self.children.get(id).is_none_or(|children| !children.is_empty())
    }

    /// The node `id`, if loaded.
    pub fn node(&self, id: &str) -> Option<&HierarchyNode> {
        self.children.values().flat_map(|nodes| nodes.iter()).find(|node| node.id == id)
    }

    /// The visible rows with `expanded` open, in tree order.
    pub fn rows(&self, expanded: &HashSet<String>) -> Vec<HierarchyRow> {
        let mut rows = Vec::new();
        if self.root.is_none() {
            return rows;
        }
        let open = expanded.contains(ROOT_ID);
        rows.push(HierarchyRow::Root { expanded: open });
        if open {
            self.push_rows(ROOT_ID, 1, expanded, &mut rows);
        }
        rows
    }

    fn push_rows(&self, parent: &str, depth: usize, expanded: &HashSet<String>, rows: &mut Vec<HierarchyRow>) {
        for node in self.children.get(parent).into_iter().flatten() {
            let expandable = self.is_expandable(&node.id);
            let open = expandable && expanded.contains(&node.id);
            rows.push(HierarchyRow::Node { node: Box::new(node.clone()), depth, expanded: open, expandable });
            if open {
                self.push_rows(&node.id, depth + 1, expanded, rows);
            }
        }
    }

    /// Where activating row `id` goes.
    pub fn target(&self, id: &str) -> Option<(String, u32, u32)> {
        if id == ROOT_ID {
            return self.root.as_ref().map(|root| (root.path.clone(), root.line, root.col));
        }
        self.node(id).map(|node| hierarchy_target(self.mode, self.direction, &node.item))
    }

    fn cancel_inflight(&mut self) -> Vec<HierarchyEffect> {
        self.pending
            .drain()
            .filter_map(|(_, pending)| match pending {
                Pending::Level { request: Some(request), .. } => Some(HierarchyEffect::Cancel(request)),
                _ => None,
            })
            .collect()
    }

    /// The window switched projects: the tree refers to the previous one's documents.
    pub fn reset(&mut self) -> Vec<HierarchyEffect> {
        let effects = self.cancel_inflight();
        self.root = None;
        self.retry = None;
        self.children.clear();
        self.failed.clear();
        self.notes.clear();
        self.loading = false;
        self.generation += 1;
        effects
    }

    fn level(
        &mut self,
        parent: String,
        keys: Vec<String>,
        name: String,
        at: (String, Pos),
        quiet: bool,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> HierarchyEffect {
        self.tickets += 1;
        let request = (self.mode == HierarchyMode::Call).then(next_request);
        let generation = self.generation;
        self.pending.insert(self.tickets, Pending::Level { generation, parent, keys, name, request, quiet });
        HierarchyEffect::Request(HierarchyRequest::Level {
            ticket: self.tickets,
            mode: self.mode,
            request,
            path: at.0,
            pos: at.1,
            direction: self.direction,
        })
    }

    /// (Re)loads the root's level: a new tree. `quiet`: a position without a symbol is not reported,
    /// the enclosing declaration is tried instead.
    fn load_root(&mut self, quiet: bool, next_request: &mut impl FnMut() -> RequestId) -> Vec<HierarchyEffect> {
        let Some(root) = self.root.clone() else { return Vec::new() };
        let mut effects = self.cancel_inflight();
        self.generation += 1;
        self.children.clear();
        self.failed.clear();
        self.notes.clear();
        self.loading = true;
        let at = (root.path.clone(), Pos::new(root.line, root.col));
        effects.push(self.level(ROOT_ID.to_owned(), vec![root_key(&root)], root.name, at, quiet, next_request));
        effects
    }

    /// Call Hierarchy / Type Hierarchy at `caret`.
    pub fn show(
        &mut self,
        mode: HierarchyMode,
        caret: Option<HierarchyCaret>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> Vec<HierarchyEffect> {
        let Some(caret) = caret else { return vec![hint("Open a file to show the hierarchy")] };
        if self.mode != mode {
            self.direction = match mode {
                HierarchyMode::Call => HierarchyDirection::Incoming,
                HierarchyMode::Type => HierarchyDirection::Outgoing,
            };
        }
        self.mode = mode;
        let name = caret.word.clone().unwrap_or_else(|| basename(&caret.path).into_owned());
        self.root = Some(HierarchyRoot { name, path: caret.path.clone(), line: caret.pos.line, col: caret.pos.col });
        let mut effects = vec![HierarchyEffect::ShowToolWindow];
        effects.extend(self.load_root(true, next_request));
        self.retry = Some(caret);
        effects
    }

    /// Shows the other direction (Callers / Callees, Supertypes / Subtypes).
    pub fn set_direction(
        &mut self,
        direction: HierarchyDirection,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> Vec<HierarchyEffect> {
        if self.direction == direction {
            return Vec::new();
        }
        self.direction = direction;
        self.load_root(false, next_request)
    }

    /// Refresh: the root's level again.
    pub fn refresh(&mut self, next_request: &mut impl FnMut() -> RequestId) -> Vec<HierarchyEffect> {
        self.load_root(false, next_request)
    }

    /// Expanding node `id` loads its children (once).
    pub fn load_children(&mut self, id: &str, next_request: &mut impl FnMut() -> RequestId) -> Vec<HierarchyEffect> {
        if self.children.contains_key(id) {
            return Vec::new();
        }
        let loading =
            self.pending.values().any(|pending| matches!(pending, Pending::Level { parent, .. } if parent == id));
        let Some(node) = self.node(id).filter(|_| !loading).cloned() else { return Vec::new() };
        let at = (to_ipc(&node.item.path), node.item.selection_range.start);
        vec![self.level(node.id, node.keys, node.item.name, at, false, next_request)]
    }

    fn set_children(&mut self, parent: &str, parent_keys: &[String], items: Vec<HierarchyItem>, note: Option<String>) {
        let mut seen = HashSet::new();
        let mut nodes = Vec::new();
        for item in items {
            let id = child_id(parent, &item);
            if !seen.insert(id.clone()) {
                continue;
            }
            let key = item_key(&item);
            // A recursive symbol arrives with a fresh id on every level (the id carries the parent
            // chain); only its position key reveals the cycle. Such a node stays a leaf: expanding it
            // again would ask for the same level forever.
            let recursion = parent_keys.contains(&key);
            if recursion {
                self.children.insert(id.clone(), Vec::new());
            }
            let keys = parent_keys.iter().cloned().chain(std::iter::once(key)).collect();
            nodes.push(HierarchyNode { id, item, recursion, keys });
        }
        self.children.insert(parent.to_owned(), nodes);
        if let Some(note) = note {
            self.notes.insert(parent.to_owned(), note);
        }
    }

    /// The answer of level `ticket`.
    pub fn level_answered(
        &mut self,
        ticket: u64,
        result: Result<LevelAnswer, IdeError>,
        i18n: &mut I18n,
    ) -> Vec<HierarchyEffect> {
        let (generation, parent, keys, name, quiet) = match self.pending.remove(&ticket) {
            Some(Pending::Level { generation, parent, keys, name, quiet, .. }) => {
                (generation, parent, keys, name, quiet)
            }
            Some(other) => {
                self.pending.insert(ticket, other);
                return Vec::new();
            }
            None => return Vec::new(),
        };
        if generation != self.generation {
            return Vec::new();
        }
        let root = parent == ROOT_ID;
        if root {
            self.loading = false;
        }
        match result {
            Ok(answer) => {
                let note = result_note(i18n, answer.incomplete, answer.approximate, Some(&name), self.light_mode);
                self.set_children(&parent, &keys, answer.items, note);
                Vec::new()
            }
            Err(error) if root && quiet && error.kind == ErrorKind::NotFound => {
                // Nothing at the caret: retry at the enclosing method / class.
                match self.retry.clone() {
                    Some(caret) => {
                        self.tickets += 1;
                        let doc = caret.doc;
                        self.pending.insert(self.tickets, Pending::Outline { generation, caret });
                        vec![HierarchyEffect::Request(HierarchyRequest::Outline { ticket: self.tickets, doc })]
                    }
                    None => {
                        self.failed.insert(parent);
                        Vec::new()
                    }
                }
            }
            Err(error) => {
                if !root {
                    self.children.insert(parent.clone(), Vec::new());
                }
                self.failed.insert(parent);
                error_usage_hint(&error).map(HierarchyEffect::Hint).into_iter().collect()
            }
        }
    }

    /// The outline asked for to find the declaration enclosing the caret.
    pub fn outline_answered(
        &mut self,
        ticket: u64,
        outline: Result<Vec<OutlineItem>, IdeError>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> Vec<HierarchyEffect> {
        let (generation, caret) = match self.pending.remove(&ticket) {
            Some(Pending::Outline { generation, caret }) => (generation, caret),
            Some(other) => {
                self.pending.insert(ticket, other);
                return Vec::new();
            }
            None => return Vec::new(),
        };
        if generation != self.generation {
            return Vec::new();
        }
        let kinds: &[SymbolKind] = match self.mode {
            HierarchyMode::Call => &CALLABLE,
            HierarchyMode::Type => &TYPES,
        };
        let items = outline.unwrap_or_default();
        let found = enclosing_declaration(&items, caret.pos, kinds).map(|decl| HierarchyRoot {
            name: decl.name.clone(),
            path: caret.path.clone(),
            line: decl.selection_range.start.line,
            col: decl.selection_range.start.col,
        });
        match found {
            Some(root) if Pos::new(root.line, root.col) != caret.pos => {
                self.root = Some(root);
                self.load_root(false, next_request)
            }
            _ => {
                self.failed.insert(ROOT_ID.to_owned());
                vec![hint(match self.mode {
                    HierarchyMode::Call => "No method at the caret to show the call hierarchy of",
                    HierarchyMode::Type => "No type at the caret to show the hierarchy of",
                })]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{IpcPath, Range};
    use std::path::PathBuf;

    fn range(l1: u32, c1: u32, l2: u32, c2: u32) -> Range {
        Range::new(Pos::new(l1, c1), Pos::new(l2, c2))
    }

    fn outline_item(
        name: &str,
        kind: SymbolKind,
        extent: Range,
        at: (u32, u32),
        children: Vec<OutlineItem>,
    ) -> OutlineItem {
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            range: extent,
            selection_range: range(at.0, at.1, at.0, at.1 + name.len() as u32),
            children,
            visibility: None,
        }
    }

    /// class Orders { void list() { int x = 1; } }: the class on lines 0-10, `list` on lines 2-6.
    fn outline() -> Vec<OutlineItem> {
        vec![outline_item(
            "Orders",
            SymbolKind::Class,
            range(0, 0, 10, 1),
            (0, 6),
            vec![
                outline_item("list", SymbolKind::Method, range(2, 2, 6, 3), (2, 7), Vec::new()),
                outline_item("count", SymbolKind::Field, range(8, 2, 8, 12), (8, 6), Vec::new()),
            ],
        )]
    }

    fn item(name: &str, from_ranges: Vec<Range>) -> HierarchyItem {
        HierarchyItem {
            name: name.to_owned(),
            kind: SymbolKind::Method,
            detail: None,
            path: IpcPath::from(PathBuf::from("/p/Caller.java")),
            uri: None,
            range: range(10, 0, 20, 1),
            selection_range: range(10, 7, 10, 7 + name.len() as u32),
            from_ranges,
        }
    }

    fn caret() -> HierarchyCaret {
        let (path, word) = ("/p/Orders.java".to_owned(), Some("x".to_owned()));
        HierarchyCaret { doc: DocId(1), path, pos: Pos::new(4, 10), word }
    }

    fn requests() -> impl FnMut() -> RequestId {
        let mut next = 0;
        move || {
            next += 1;
            RequestId(next)
        }
    }

    fn level_ticket(effects: &[HierarchyEffect]) -> (u64, Pos) {
        effects
            .iter()
            .find_map(|effect| match effect {
                HierarchyEffect::Request(HierarchyRequest::Level { ticket, pos, .. }) => Some((*ticket, *pos)),
                _ => None,
            })
            .expect("a level is asked for")
    }

    fn outline_ticket(effects: &[HierarchyEffect]) -> u64 {
        effects
            .iter()
            .find_map(|effect| match effect {
                HierarchyEffect::Request(HierarchyRequest::Outline { ticket, .. }) => Some(*ticket),
                _ => None,
            })
            .expect("the outline is asked for")
    }

    fn names(hierarchy: &Hierarchy, parent: &str) -> Vec<String> {
        hierarchy.children(parent).unwrap_or_default().iter().map(|node| node.item.name.clone()).collect()
    }

    fn answer(items: Vec<HierarchyItem>) -> Result<LevelAnswer, IdeError> {
        Ok(LevelAnswer { items, ..LevelAnswer::default() })
    }

    /// Call Hierarchy at the caret inside `list`: the server prepares a hierarchy only on the method
    /// name, so the enclosing method is tried and its callers answered with `callers`.
    fn shown_in_list(callers: Result<LevelAnswer, IdeError>, i18n: &mut I18n) -> (Hierarchy, Vec<HierarchyEffect>) {
        let mut ids = requests();
        let mut hierarchy = Hierarchy::default();
        let effects = hierarchy.show(HierarchyMode::Call, Some(caret()), &mut ids);
        assert_eq!(effects[0], HierarchyEffect::ShowToolWindow);
        let (ticket, pos) = level_ticket(&effects);
        assert_eq!(pos, Pos::new(4, 10));
        let effects = hierarchy.level_answered(ticket, Err(IdeError::not_found("No symbol")), i18n);
        let effects = hierarchy.outline_answered(outline_ticket(&effects), Ok(outline()), &mut ids);
        let (ticket, pos) = level_ticket(&effects);
        assert_eq!(pos, Pos::new(2, 7));
        let effects = hierarchy.level_answered(ticket, callers, i18n);
        (hierarchy, effects)
    }

    #[test]
    fn finds_the_innermost_enclosing_declaration_of_the_wanted_kinds() {
        let items = outline();
        let name =
            |pos: Pos, kinds: &[SymbolKind]| enclosing_declaration(&items, pos, kinds).map(|item| item.name.clone());
        assert_eq!(name(Pos::new(4, 10), &CALLABLE).as_deref(), Some("list"));
        assert_eq!(name(Pos::new(4, 10), &[SymbolKind::Class]).as_deref(), Some("Orders"));
        assert_eq!(name(Pos::new(8, 4), &CALLABLE), None);
        assert_eq!(name(Pos::new(12, 0), &[SymbolKind::Class]), None);
    }

    #[test]
    fn navigates_callers_to_their_call_site_other_nodes_to_the_symbol() {
        let caller = item("main", vec![range(12, 4, 12, 8), range(14, 4, 14, 8)]);
        let at = |line: u32, col: u32| ("/p/Caller.java".to_owned(), line, col);
        assert_eq!(hierarchy_target(HierarchyMode::Call, HierarchyDirection::Incoming, &caller), at(12, 4));
        assert_eq!(hierarchy_target(HierarchyMode::Call, HierarchyDirection::Outgoing, &caller), at(10, 7));
        let base = item("Base", Vec::new());
        assert_eq!(hierarchy_target(HierarchyMode::Type, HierarchyDirection::Incoming, &base), at(10, 7));
    }

    #[test]
    fn retries_at_the_enclosing_method_when_nothing_is_at_the_caret() {
        let mut i18n = I18n::default();
        let (hierarchy, effects) = shown_in_list(answer(vec![item("main", vec![range(12, 4, 12, 8)])]), &mut i18n);
        let root = HierarchyRoot { name: "list".to_owned(), path: "/p/Orders.java".to_owned(), line: 2, col: 7 };
        assert_eq!(hierarchy.root(), Some(&root));
        assert_eq!(names(&hierarchy, ROOT_ID), ["main"]);
        assert!(!hierarchy.failed(ROOT_ID) && !hierarchy.loading());
        assert_eq!(hierarchy.notes_len(), 0);
        assert!(effects.is_empty(), "no hint: {effects:?}");
    }

    #[test]
    fn notes_a_level_whose_callers_may_be_incomplete_until_it_is_loaded_again() {
        let mut i18n = I18n::default();
        let incomplete = Ok(LevelAnswer {
            items: vec![item("main", Vec::new())],
            incomplete: Some(ReferencesIncomplete::TooManyFiles),
            approximate: false,
        });
        let (mut hierarchy, _) = shown_in_list(incomplete, &mut i18n);
        let note = "May be incomplete: too many files mention 'list' to search them all";
        assert_eq!(hierarchy.note(ROOT_ID), Some(note));
        let effects = hierarchy.refresh(&mut requests());
        let (ticket, _) = level_ticket(&effects);
        hierarchy.level_answered(ticket, answer(vec![item("main", Vec::new())]), &mut i18n);
        assert!(hierarchy.children(ROOT_ID).is_some());
        assert_eq!(hierarchy.notes_len(), 0);
    }

    #[test]
    fn cancels_the_requests_of_a_tree_that_is_replaced() {
        let mut i18n = I18n::default();
        let (mut hierarchy, _) = shown_in_list(answer(vec![item("main", Vec::new())]), &mut i18n);
        let mut ids = || RequestId(77);
        let effects = hierarchy.refresh(&mut ids);
        let (ticket, _) = level_ticket(&effects);
        assert_eq!(hierarchy.reset(), [HierarchyEffect::Cancel(RequestId(77))]);
        // The answer of the replaced tree is dropped.
        assert!(hierarchy.level_answered(ticket, answer(vec![item("stale", Vec::new())]), &mut i18n).is_empty());
        assert_eq!(hierarchy.root(), None);
        assert!(hierarchy.children(ROOT_ID).is_none());
    }

    #[test]
    fn keeps_a_caller_that_repeats_an_ancestor_a_recursion_leaf() {
        // `main` calls itself: every level answers with the same item.
        let mut i18n = I18n::default();
        let mut ids = requests();
        let mut hierarchy = Hierarchy::default();
        let effects = hierarchy.show(HierarchyMode::Call, Some(caret()), &mut ids);
        let (ticket, _) = level_ticket(&effects);
        hierarchy.level_answered(ticket, answer(vec![item("main", Vec::new())]), &mut i18n);
        let top = hierarchy.children(ROOT_ID).and_then(|nodes| nodes.first()).cloned().expect("a caller");
        assert_eq!(top.item.name, "main");
        assert!(!top.recursion);
        let effects = hierarchy.load_children(&top.id, &mut ids);
        let (ticket, pos) = level_ticket(&effects);
        assert_eq!(pos, Pos::new(10, 7));
        hierarchy.level_answered(ticket, answer(vec![item("main", Vec::new())]), &mut i18n);
        let nested = hierarchy.children(&top.id).and_then(|nodes| nodes.first()).cloned().expect("a nested caller");
        assert_eq!(nested.item.name, "main");
        assert!(nested.recursion);
        // The leaf carries no expandable children, so the tree never requests it again.
        assert_eq!(hierarchy.children(&nested.id).map(<[HierarchyNode]>::len), Some(0));
        assert!(!hierarchy.is_expandable(&nested.id));
        assert!(hierarchy.load_children(&nested.id, &mut ids).is_empty());
        assert!(!hierarchy.failed(ROOT_ID) && !hierarchy.failed(&top.id));
    }

    #[test]
    fn labels_directions_and_builds_unique_child_ids() {
        assert_eq!(direction_label(HierarchyMode::Call, HierarchyDirection::Incoming), "Callers");
        assert_eq!(direction_label(HierarchyMode::Type, HierarchyDirection::Outgoing), "Subtypes");
        let item = HierarchyItem {
            name: "f".to_owned(),
            kind: SymbolKind::Function,
            detail: None,
            path: IpcPath::from(PathBuf::from("/a.rs")),
            uri: None,
            range: range(1, 0, 3, 1),
            selection_range: range(1, 3, 1, 4),
            from_ranges: Vec::new(),
        };
        assert_eq!(child_id(ROOT_ID, &item), "root//a.rs:1:3:f");
    }

    // Not from a spec: the type hierarchy starts on subtypes, titles name the direction, a failed level
    // is marked, and no type at the caret says so.
    #[test]
    fn titles_rows_and_failures_follow_the_mode_and_direction() {
        let mut i18n = I18n::default();
        let mut ids = requests();
        let mut hierarchy = Hierarchy::default();
        let effects = hierarchy.show(HierarchyMode::Type, Some(caret()), &mut ids);
        assert_eq!(hierarchy.direction(), HierarchyDirection::Outgoing);
        let HierarchyEffect::Request(HierarchyRequest::Level { request, .. }) = &effects[1] else {
            panic!("a level is asked for: {effects:?}");
        };
        assert_eq!(*request, None, "type hierarchy requests are not cancellable");
        let (call, ty) = (HierarchyMode::Call, HierarchyMode::Type);
        let title = hierarchy_title(ty, HierarchyDirection::Outgoing, "Orders", &mut i18n);
        assert_eq!(title, "Subtypes of Orders");
        let button = direction_button_label(call, HierarchyDirection::Incoming, &mut i18n);
        assert_eq!(button, "Callers Hierarchy");

        let (ticket, _) = level_ticket(&effects);
        let effects = hierarchy.level_answered(ticket, Err(IdeError::not_found("No symbol")), &mut i18n);
        let effects = hierarchy.outline_answered(outline_ticket(&effects), Ok(Vec::new()), &mut ids);
        assert_eq!(
            effects,
            [HierarchyEffect::Hint(UsageHint {
                text: "No type at the caret to show the hierarchy of".to_owned(),
                tone: Tone::Info
            })]
        );
        assert!(hierarchy.failed(ROOT_ID));

        let effects = hierarchy.set_direction(HierarchyDirection::Incoming, &mut ids);
        let (ticket, _) = level_ticket(&effects);
        hierarchy.level_answered(ticket, answer(vec![item("Base", Vec::new())]), &mut i18n);
        let expanded = HashSet::from([ROOT_ID.to_owned()]);
        let rows = hierarchy.rows(&expanded);
        assert_eq!(rows.iter().map(HierarchyRow::id).collect::<Vec<_>>(), ["root", "root//p/Caller.java:10:7:Base"]);
        let base = rows[1].id().to_owned();
        let effects = hierarchy.load_children(&base, &mut ids);
        let (ticket, _) = level_ticket(&effects);
        let failed = hierarchy.level_answered(ticket, Err(IdeError::internal("boom")), &mut i18n);
        assert_eq!(failed, [HierarchyEffect::Hint(UsageHint { text: "boom".to_owned(), tone: Tone::Error })]);
        assert!(hierarchy.failed(&base) && !hierarchy.is_expandable(&base));
        assert_eq!(hierarchy.target(&base), Some(("/p/Caller.java".to_owned(), 10, 7)));
        assert_eq!(hierarchy.show(HierarchyMode::Call, None, &mut ids), [hint("Open a file to show the hierarchy")]);
    }
}
