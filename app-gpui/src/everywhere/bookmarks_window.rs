//! The Bookmarks tool window: every bookmark list as a tree of its bookmarked folders, its files and,
//! under a file, its line bookmarks with their mnemonics and descriptions.
//!
//! The toolbar adds a list (named in place), removes every bookmark and expands or collapses the
//! tree. Enter, F4 or a double click opens the selected bookmark (a folder is selected in the Project
//! view), Delete removes the selection, F2 renames a list or describes a line bookmark in place. The
//! context menu jumps to the source, renames, moves bookmarks to another list and deletes them; on a
//! list it adds, renames, deletes it or makes it the default.
//!
//! The window works on its own copy of the [`BookmarkStore`], like the Bookmarks popup: after each
//! change it reports what to save and [`BookmarksWindow::store`] holds the bookmarks as they are now.
//! What it cannot do itself (open a file, show the context menu, ask before deleting a list) is a
//! [`BookmarksWindowEvent`].

use super::bookmarks_store::{
    Bookmark, BookmarkData, BookmarkRef, BookmarkStore, ListNameError, create_list, describe_bookmark, move_bookmarks,
    rename_list, set_default_list, validate_list_name,
};
use super::bookmarks_view::{BookmarkNode, build_bookmark_tree};
use super::open::StorageWrite;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::platform::virtual_docs::basename;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    Modifiers, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    Subscription, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// How far each level of the tree is indented, in pixels.
const INDENT: f32 = 16.0;
/// The key of the row a new list is named in.
const NEW_LIST_KEY: &str = "new-list";
/// The action whose shortcut the empty state names.
const TOGGLE_BOOKMARK_ACTION: &str = "navigate.toggleBookmark";

/// What a row of the tree stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BookmarkRowKind {
    List,
    Folder,
    File,
    Mark,
    /// The row a new list is being named in.
    NewList,
}

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookmarkRow {
    pub key: String,
    pub kind: BookmarkRowKind,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
    pub label: String,
    /// Grey text after the label: a list's count, a folder's or file's place, a described bookmark's line.
    pub meta: String,
    /// The line's text after the place of a bookmark without a description.
    pub code: Option<String>,
    pub mnemonic: Option<char>,
    /// A file that is bookmarked itself, not only lines in it.
    pub bookmarked: bool,
    /// The default list, whose name is bold.
    pub is_default: bool,
}

/// What is being named or described in place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkEdit {
    /// The description of the line bookmark at this path and 0-based line.
    Description { path: String, line: u32 },
    /// The name of the list with this id.
    ListName(String),
    /// A new list; these bookmarks move into it once it exists.
    NewList(Vec<BookmarkRef>),
}

/// A row of the context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarksCommand {
    /// Open the bookmark (or the file) of the row with this key.
    JumpToSource(String),
    /// Describe the line bookmark of the row with this key.
    Rename(String),
    MoveTo {
        refs: Vec<BookmarkRef>,
        list: String,
    },
    /// Name a new list and move these bookmarks into it.
    MoveToNewList(Vec<BookmarkRef>),
    /// Remove the bookmarks of the rows with these keys.
    Delete(Vec<String>),
    NewList,
    RenameList(String),
    MakeDefault(String),
    DeleteList(String),
}

/// A toolbar button with its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub enabled: bool,
    pub pressed: bool,
}

/// What the Bookmarks tool window asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum BookmarksWindowEvent {
    /// Open `path`: with the caret at the start of the 0-based `line` for a line bookmark.
    Open { path: PathBuf, line: Option<u32> },
    /// Select this bookmarked folder in the Project view.
    SelectInProjectView(PathBuf),
    /// The bookmarks changed: save these values; [`BookmarksWindow::store`] holds the bookmarks now.
    Changed(Vec<StorageWrite>),
    /// Show [`BookmarksWindow::context_menu`] here; run the chosen row with
    /// [`BookmarksWindow::run_command`].
    ShowMenu(Point<Pixels>),
    /// Ask whether to delete the list with its bookmarks ([`delete_list_message`]); on yes call
    /// [`BookmarksWindow::delete_list`].
    ConfirmDeleteList { id: String, name: String, count: usize },
}

/// The question asked before a list with bookmarks is deleted.
pub fn delete_list_message(name: &str, count: usize, i18n: &mut I18n) -> String {
    let strings: [&str; 3] = if count == 1 {
        ["Delete the list '", "' with its ", " bookmark?"]
    } else {
        ["Delete the list '", "' with its ", " bookmarks?"]
    };
    i18n.tx_built(&strings, &[Param::from(name), Param::from(count)])
}

/// The node with `key` among `nodes` and their children.
fn find_node<'a>(nodes: &'a [BookmarkNode], key: &str) -> Option<&'a BookmarkNode> {
    nodes.iter().find_map(|node| if node.key() == key { Some(node) } else { find_node(node.children(), key) })
}

/// The bookmarks a node stands for: a line, a folder, a file with its line bookmarks; none for a list.
fn refs_of(node: &BookmarkNode) -> Vec<BookmarkRef> {
    let line = |bookmark: &Bookmark| BookmarkRef { path: bookmark.path.clone(), line: Some(bookmark.line) };
    match node {
        BookmarkNode::List { .. } => Vec::new(),
        BookmarkNode::Mark { bookmark, .. } => vec![line(bookmark)],
        BookmarkNode::Folder { path, .. } => vec![BookmarkRef { path: path.clone(), line: None }],
        BookmarkNode::File { path, bookmarked, children, .. } => {
            let own = bookmarked.then(|| BookmarkRef { path: path.clone(), line: None });
            let marks = children.iter().filter_map(|child| match child {
                BookmarkNode::Mark { bookmark, .. } => Some(line(bookmark)),
                _ => None,
            });
            own.into_iter().chain(marks).collect()
        }
    }
}

/// `data` without the bookmarks `refs` names.
fn remove_refs(data: &BookmarkData, refs: &[BookmarkRef]) -> BookmarkData {
    let names = |path: &str, line: Option<u32>| refs.iter().any(|wanted| wanted.path == path && wanted.line == line);
    let mut next = data.clone();
    next.items.retain(|bookmark| !names(&bookmark.path, Some(bookmark.line)));
    next.paths.retain(|bookmark| !names(&bookmark.path, None));
    next
}

/// The id of the list a node is in; empty for a list itself.
fn list_of(node: &BookmarkNode) -> &str {
    match node {
        BookmarkNode::List { .. } => "",
        BookmarkNode::Mark { bookmark, .. } => bookmark.list.as_deref().unwrap_or_default(),
        BookmarkNode::File { list, .. } | BookmarkNode::Folder { list, .. } => list,
    }
}

/// The row of `node` as the tree shows it.
fn row_of(node: &BookmarkNode, depth: usize, collapsed: &HashSet<String>, i18n: &I18n) -> BookmarkRow {
    let base = BookmarkRow {
        key: node.key().to_owned(),
        kind: BookmarkRowKind::Mark,
        depth,
        expandable: false,
        expanded: false,
        label: String::new(),
        meta: String::new(),
        code: None,
        mnemonic: None,
        bookmarked: false,
        is_default: false,
    };
    match node {
        BookmarkNode::List { list, is_default, count, .. } => {
            let mut meta = i18n.t("search.bookmarks.count", &[("count", Param::from(*count))]);
            if *is_default {
                meta.push_str(&i18n.tx(" · default"));
            }
            BookmarkRow {
                kind: BookmarkRowKind::List,
                expandable: true,
                expanded: !collapsed.contains(node.key()),
                label: list.name.clone(),
                meta,
                is_default: *is_default,
                ..base
            }
        }
        BookmarkNode::Folder { name, dir, .. } => {
            BookmarkRow { kind: BookmarkRowKind::Folder, label: name.clone(), meta: dir.clone(), ..base }
        }
        BookmarkNode::File { name, dir, bookmarked, children, .. } => BookmarkRow {
            kind: BookmarkRowKind::File,
            expandable: !children.is_empty(),
            expanded: !children.is_empty() && !collapsed.contains(node.key()),
            label: name.clone(),
            meta: dir.clone(),
            bookmarked: *bookmarked,
            ..base
        },
        BookmarkNode::Mark { bookmark, .. } => {
            let place = format!("{}:{}", basename(&bookmark.path), u64::from(bookmark.line) + 1);
            let described = bookmark.description.as_deref().filter(|text| !text.trim().is_empty());
            let (label, meta, code) = match described {
                Some(description) => (description.to_owned(), place, None),
                None => {
                    let text = bookmark.text.trim();
                    let code = if text.is_empty() { i18n.tx("(empty line)") } else { text.to_owned() };
                    (place, String::new(), Some(code))
                }
            };
            BookmarkRow { label, meta, code, mnemonic: bookmark.mnemonic, ..base }
        }
    }
}

/// Appends the visible rows of `nodes`, top to bottom.
fn push_rows(
    nodes: &[BookmarkNode],
    depth: usize,
    collapsed: &HashSet<String>,
    i18n: &I18n,
    rows: &mut Vec<BookmarkRow>,
) {
    for node in nodes {
        let row = row_of(node, depth, collapsed, i18n);
        let open = row.expanded;
        rows.push(row);
        if open {
            push_rows(node.children(), depth + 1, collapsed, i18n, rows);
        }
    }
}

/// The text of a list name the store refuses.
fn name_error_text(error: &ListNameError, i18n: &mut I18n) -> String {
    match error {
        ListNameError::Empty => i18n.tx("Name cannot be empty"),
        ListNameError::Duplicate(name) => {
            i18n.tx_built(&["A list named '", "' already exists"], &[Param::from(name.as_str())])
        }
    }
}

pub struct BookmarksWindow {
    focus: FocusHandle,
    store: BookmarkStore,
    tree: Vec<BookmarkNode>,
    /// Keys of the collapsed lists and files; the tree starts expanded.
    collapsed: HashSet<String>,
    /// Keys of the selected rows.
    selected: Vec<String>,
    /// The row the keyboard is on.
    lead: Option<String>,
    /// Where a Shift range starts.
    anchor: Option<String>,
    editing: Option<BookmarkEdit>,
    /// Why the name typed for a list cannot be used.
    edit_error: Option<String>,
    edit_field: Entity<TextField>,
    scroll: UniformListScrollHandle,
    /// The claims on the keys the tree and the field handle ahead of the keymap.
    key_claims: [u64; 2],
    _field_events: Subscription,
}

impl EventEmitter<BookmarksWindowEvent> for BookmarksWindow {}

impl BookmarksWindow {
    /// The tool window over a copy of `store`.
    pub fn new(store: BookmarkStore, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let edit_field = cx.new(TextField::new);
        let claimant = cx.weak_entity();
        let tree_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |view, cx| view.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let claimant = cx.weak_entity();
        let edit_claim = input::claim_keys(
            edit_field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |view, cx| view.claim_edit_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|view, cx| {
            for claim in view.key_claims {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        let field_events = cx.subscribe(&edit_field, |view: &mut Self, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed && view.edit_error.take().is_some() {
                cx.notify();
            }
        });
        let tree = build_bookmark_tree(store.data(), store.scope());
        Self {
            focus,
            store,
            tree,
            collapsed: HashSet::new(),
            selected: Vec::new(),
            lead: None,
            anchor: None,
            editing: None,
            edit_error: None,
            edit_field,
            scroll: UniformListScrollHandle::new(),
            key_claims: [tree_claim, edit_claim],
            _field_events: field_events,
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Fed by the workbench

    /// The bookmarks changed elsewhere (the editor's gutter, the popup, another project).
    pub fn set_store(&mut self, store: BookmarkStore, cx: &mut Context<Self>) {
        if store == self.store {
            return;
        }
        self.store = store;
        self.rebuild();
        cx.notify();
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    /// The bookmarks as the window left them.
    pub fn store(&self) -> &BookmarkStore {
        &self.store
    }

    pub fn edit_field(&self) -> &Entity<TextField> {
        &self.edit_field
    }

    /// What is being named or described in place.
    pub fn editing(&self) -> Option<&BookmarkEdit> {
        self.editing.as_ref()
    }

    /// Why the typed list name is refused.
    pub fn edit_error(&self) -> Option<&str> {
        self.edit_error.as_deref()
    }

    /// Whether there is nothing to show: no bookmark and only the one list.
    pub fn is_empty(&self) -> bool {
        let data = self.store.data();
        data.items.is_empty() && data.paths.is_empty() && data.lists.len() == 1
    }

    /// The visible rows, top to bottom, with the row of a list being named last.
    pub fn rows(&self, cx: &App) -> Vec<BookmarkRow> {
        let default = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&default, |localization| &localization.0);
        self.rows_with(i18n)
    }

    fn rows_with(&self, i18n: &I18n) -> Vec<BookmarkRow> {
        let mut rows = Vec::new();
        push_rows(&self.tree, 0, &self.collapsed, i18n, &mut rows);
        if matches!(self.editing, Some(BookmarkEdit::NewList(_))) {
            rows.push(BookmarkRow {
                key: NEW_LIST_KEY.to_owned(),
                kind: BookmarkRowKind::NewList,
                depth: 0,
                expandable: false,
                expanded: false,
                label: String::new(),
                meta: String::new(),
                code: None,
                mnemonic: None,
                bookmarked: false,
                is_default: false,
            });
        }
        rows
    }

    /// The rows as text: indented two spaces per level, a mnemonic in brackets, the label, then the
    /// grey text and the line's text, each after two spaces.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        self.rows(cx)
            .into_iter()
            .map(|row| {
                let mut line = "  ".repeat(row.depth);
                if let Some(mnemonic) = row.mnemonic {
                    line.push_str(&format!("[{mnemonic}] "));
                }
                line.push_str(&row.label);
                for extra in [Some(row.meta.as_str()), row.code.as_deref()].into_iter().flatten() {
                    if !extra.is_empty() {
                        line.push_str("  ");
                        line.push_str(extra);
                    }
                }
                line
            })
            .collect()
    }

    /// The keys of the selected rows, in tree order.
    pub fn selected(&self) -> Vec<String> {
        self.selected_nodes().iter().map(|node| node.key().to_owned()).collect()
    }

    /// The toolbar: New List, Remove All (while there are bookmarks), Expand All and Collapse All.
    pub fn toolbar(&self) -> Vec<ToolButton> {
        let data = self.store.data();
        let any = !data.items.is_empty() || !data.paths.is_empty();
        let button = |id, icon, title, enabled| ToolButton { id, icon, title, enabled, pressed: false };
        let mut buttons = vec![button("bookmarks-new-list", "plus", "New Bookmark List…", true)];
        if any {
            buttons.push(button("bookmarks-remove-all", "trash", "Remove All Bookmarks", true));
        }
        buttons.push(button("bookmarks-expand-all", "expand-all", "Expand All", !self.is_empty()));
        buttons.push(button("bookmarks-collapse-all", "collapse-all", "Collapse All", !self.is_empty()));
        buttons
    }

    /// The context menu of the selection: a list's own menu when one list is selected, else the
    /// bookmarks' menu.
    pub fn context_menu(&self) -> Vec<MenuItem<BookmarksCommand>> {
        let targets = self.selected_nodes();
        let Some(lead) = self.lead.as_deref().and_then(|key| find_node(&self.tree, key)) else { return Vec::new() };
        if let (BookmarkNode::List { list, is_default, .. }, 1) = (lead, targets.len()) {
            return vec![
                MenuItem::item("New List…", BookmarksCommand::NewList).icon("plus"),
                MenuItem::item("Rename List…", BookmarksCommand::RenameList(list.id.clone())).shortcut("F2"),
                MenuItem::item("Make Default", BookmarksCommand::MakeDefault(list.id.clone())).disabled(*is_default),
                MenuItem::Separator,
                MenuItem::item("Delete List", BookmarksCommand::DeleteList(list.id.clone())).danger(),
            ];
        }
        let mut items = Vec::new();
        if matches!(lead, BookmarkNode::File { .. } | BookmarkNode::Mark { .. }) {
            items.push(MenuItem::item("Jump to Source", BookmarksCommand::JumpToSource(lead.key().to_owned())));
        }
        if let (BookmarkNode::Mark { .. }, 1) = (lead, targets.len()) {
            items.push(MenuItem::item("Rename…", BookmarksCommand::Rename(lead.key().to_owned())).shortcut("F2"));
        }
        let bookmarks: Vec<&BookmarkNode> =
            targets.iter().copied().filter(|node| !matches!(node, BookmarkNode::List { .. })).collect();
        if !bookmarks.is_empty() {
            let refs: Vec<BookmarkRef> = bookmarks.iter().flat_map(|node| refs_of(node)).collect();
            let from: HashSet<&str> = bookmarks.iter().map(|node| list_of(node)).collect();
            let mut lists: Vec<MenuItem<BookmarksCommand>> = self
                .store
                .data()
                .lists
                .iter()
                .map(|list| {
                    let command = BookmarksCommand::MoveTo { refs: refs.clone(), list: list.id.clone() };
                    MenuItem::item(list.name.clone(), command)
                        .disabled(from.len() == 1 && from.contains(list.id.as_str()))
                })
                .collect();
            lists.push(MenuItem::Separator);
            lists.push(MenuItem::item("New List…", BookmarksCommand::MoveToNewList(refs)));
            items.push(MenuItem::submenu("Move to Another List", lists));
        }
        items.push(MenuItem::Separator);
        let keys: Vec<String> = targets.iter().map(|node| node.key().to_owned()).collect();
        let label =
            if keys.len() > 1 { format!("Delete {} Bookmarks", keys.len()) } else { "Delete Bookmark".to_owned() };
        items.push(MenuItem::item(label, BookmarksCommand::Delete(keys)).danger());
        items
    }

    // -----------------------------------------------------------------------------------------------
    // Changes

    /// Runs a row of the context menu.
    pub fn run_command(&mut self, command: BookmarksCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            BookmarksCommand::JumpToSource(key) => self.activate(&key, cx),
            BookmarksCommand::Rename(key) => {
                self.select_only(&key, cx);
                self.rename_selected(window, cx);
            }
            BookmarksCommand::MoveTo { refs, list } => {
                self.apply(|store| store.update_data(|data| move_bookmarks(data, &refs, &list)), cx);
            }
            BookmarksCommand::MoveToNewList(refs) => self.start_editing(BookmarkEdit::NewList(refs), "", window, cx),
            BookmarksCommand::Delete(keys) => {
                let refs: Vec<BookmarkRef> =
                    keys.iter().filter_map(|key| find_node(&self.tree, key)).flat_map(refs_of).collect();
                self.apply(|store| store.update_data(|data| remove_refs(data, &refs)), cx);
            }
            BookmarksCommand::NewList => self.start_editing(BookmarkEdit::NewList(Vec::new()), "", window, cx),
            BookmarksCommand::RenameList(id) => self.rename_list(&id, window, cx),
            BookmarksCommand::MakeDefault(id) => {
                self.apply(|store| store.update_data(|data| set_default_list(data, &id)), cx);
            }
            BookmarksCommand::DeleteList(id) => self.request_delete_list(&id, cx),
        }
    }

    /// Deletes the list `id` with its bookmarks. When no list is left, a new default one takes the
    /// project's name.
    pub fn delete_list(&mut self, id: &str, cx: &mut Context<Self>) {
        let fallback = self.store.scope().map_or_else(|| "Bookmarks".to_owned(), |scope| basename(scope).into_owned());
        self.apply(|store| store.update_data(|data| super::bookmarks_store::delete_list(data, id, &fallback)), cx);
    }

    /// Changes the bookmarks, rebuilds the tree and tells the workbench what to save.
    fn apply(&mut self, change: impl FnOnce(&mut BookmarkStore) -> Vec<StorageWrite>, cx: &mut Context<Self>) {
        let before = self.store.data().clone();
        let writes = change(&mut self.store);
        if *self.store.data() == before {
            return;
        }
        self.rebuild();
        cx.emit(BookmarksWindowEvent::Changed(writes));
        cx.notify();
    }

    /// The tree from the store; the selection keeps the rows that are still there, or moves to the
    /// row that took the lead's place.
    fn rebuild(&mut self) {
        let old_index = self.lead_index(&I18n::default());
        self.tree = build_bookmark_tree(self.store.data(), self.store.scope());
        let tree = &self.tree;
        self.selected.retain(|key| find_node(tree, key).is_some());
        if self.lead.as_deref().is_some_and(|key| find_node(tree, key).is_none()) {
            let rows = self.rows_with(&I18n::default());
            let next = old_index.and_then(|index| rows.get(index.min(rows.len().saturating_sub(1))));
            self.lead = next.map(|row| row.key.clone());
            self.anchor.clone_from(&self.lead);
            self.selected = self.lead.iter().cloned().collect();
        }
    }

    fn lead_index(&self, i18n: &I18n) -> Option<usize> {
        let lead = self.lead.as_deref()?;
        self.rows_with(i18n).iter().position(|row| row.key == lead)
    }

    /// The selected nodes, in tree order.
    fn selected_nodes(&self) -> Vec<&BookmarkNode> {
        fn walk<'a>(nodes: &'a [BookmarkNode], selected: &[String], out: &mut Vec<&'a BookmarkNode>) {
            for node in nodes {
                if selected.iter().any(|key| key == node.key()) {
                    out.push(node);
                }
                walk(node.children(), selected, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.tree, &self.selected, &mut out);
        out
    }

    fn select_only(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected = vec![key.to_owned()];
        self.lead = Some(key.to_owned());
        self.anchor = Some(key.to_owned());
        cx.notify();
    }

    /// A click on the row `key`: Shift selects the range from the anchor, Ctrl or ⌘ adds or removes
    /// the row, a plain click selects it alone.
    pub fn click(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        if modifiers.shift {
            self.select_range(key, cx);
        } else if modifiers.control || modifiers.platform {
            if let Some(at) = self.selected.iter().position(|selected| selected == key) {
                self.selected.remove(at);
                self.lead = self.selected.last().cloned();
            } else {
                self.selected.push(key.to_owned());
                self.lead = Some(key.to_owned());
            }
            self.anchor.clone_from(&self.lead);
            cx.notify();
        } else {
            self.select_only(key, cx);
        }
    }

    fn select_range(&mut self, key: &str, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let at = |wanted: &str| rows.iter().position(|row| row.key == wanted);
        let Some(to) = at(key) else { return };
        let from = self.anchor.as_deref().and_then(at).unwrap_or(to);
        let (low, high) = (from.min(to), from.max(to));
        self.selected = rows[low..=high].iter().map(|row| row.key.clone()).collect();
        self.lead = Some(key.to_owned());
        cx.notify();
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
        }
        cx.notify();
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        cx.notify();
    }

    /// Collapses every list and every file.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        fn walk(nodes: &[BookmarkNode], out: &mut HashSet<String>) {
            let folds = |node: &&BookmarkNode| !node.children().is_empty() || matches!(node, BookmarkNode::List { .. });
            for node in nodes.iter().filter(folds) {
                out.insert(node.key().to_owned());
                walk(node.children(), out);
            }
        }
        walk(&self.tree, &mut self.collapsed);
        let rows = self.rows(cx);
        self.selected.retain(|key| rows.iter().any(|row| &row.key == key));
        if self.lead.as_deref().is_some_and(|lead| !rows.iter().any(|row| row.key == lead)) {
            self.lead = self.selected.last().cloned();
        }
        cx.notify();
    }

    /// Enter, F4 and a double click: opens a line bookmark at its line or a file, selects a folder in
    /// the Project view, and expands or collapses a list.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let event = match find_node(&self.tree, key) {
            Some(BookmarkNode::Mark { bookmark, .. }) => {
                BookmarksWindowEvent::Open { path: PathBuf::from(&bookmark.path), line: Some(bookmark.line) }
            }
            Some(BookmarkNode::File { path, .. }) => {
                BookmarksWindowEvent::Open { path: PathBuf::from(path), line: None }
            }
            Some(BookmarkNode::Folder { path, .. }) => BookmarksWindowEvent::SelectInProjectView(PathBuf::from(path)),
            Some(BookmarkNode::List { .. }) => {
                self.toggle(key, cx);
                return;
            }
            None => return,
        };
        cx.emit(event);
    }

    /// A right click on the row `key`: a row outside the selection becomes the selection, then the
    /// workbench shows the menu.
    pub fn context_click(&mut self, key: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selected.iter().any(|selected| selected == key) {
            self.select_only(key, cx);
        }
        self.lead = Some(key.to_owned());
        cx.emit(BookmarksWindowEvent::ShowMenu(position));
    }

    /// Delete: removes the selected bookmarks; a selected list is deleted with its bookmarks, after
    /// asking when it has any. Returns whether anything was selected.
    fn remove_selected(&mut self, cx: &mut Context<Self>) -> bool {
        let nodes = self.selected_nodes();
        if nodes.is_empty() {
            return false;
        }
        let list = nodes.iter().find_map(|node| match node {
            BookmarkNode::List { list, .. } => Some(list.id.clone()),
            _ => None,
        });
        if let Some(id) = list {
            self.request_delete_list(&id, cx);
            return true;
        }
        let refs: Vec<BookmarkRef> = nodes.iter().flat_map(|node| refs_of(node)).collect();
        self.apply(|store| store.update_data(|data| remove_refs(data, &refs)), cx);
        true
    }

    /// Deletes an empty list at once; asks the workbench first for one with bookmarks.
    fn request_delete_list(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(BookmarkNode::List { list, count, .. }) = find_node(&self.tree, &format!("l:{id}")) else { return };
        if *count == 0 {
            self.delete_list(id, cx);
            return;
        }
        let (name, count) = (list.name.clone(), *count);
        cx.emit(BookmarksWindowEvent::ConfirmDeleteList { id: id.to_owned(), name, count });
    }

    /// F2: renames the one selected list or describes the one selected line bookmark. Returns whether
    /// the selection is one of those.
    fn rename_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let nodes = self.selected_nodes();
        let target = match nodes.as_slice() {
            [BookmarkNode::Mark { bookmark, .. }] => {
                let text = bookmark.description.clone().unwrap_or_else(|| bookmark.text.clone());
                (BookmarkEdit::Description { path: bookmark.path.clone(), line: bookmark.line }, text)
            }
            [BookmarkNode::List { list, .. }] => (BookmarkEdit::ListName(list.id.clone()), list.name.clone()),
            _ => return false,
        };
        self.start_editing(target.0, &target.1, window, cx);
        true
    }

    fn rename_list(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(list) = self.store.data().lists.iter().find(|list| list.id == id).cloned() else { return };
        self.select_only(&format!("l:{id}"), cx);
        self.start_editing(BookmarkEdit::ListName(list.id), &list.name, window, cx);
    }

    /// Opens the field in place with `text` selected and gives it the keyboard.
    fn start_editing(&mut self, target: BookmarkEdit, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_field.update(cx, |field, cx| {
            field.set_text(text, cx);
            field.select_all(cx);
        });
        self.editing = Some(target);
        self.edit_error = None;
        window.focus(&self.edit_field.focus_handle(cx), cx);
        if let Some(index) = self.editing_index(cx) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        cx.notify();
    }

    /// The row the field is drawn in.
    fn editing_index(&self, cx: &App) -> Option<usize> {
        let key = match self.editing.as_ref()? {
            BookmarkEdit::NewList(_) => NEW_LIST_KEY.to_owned(),
            BookmarkEdit::ListName(id) => format!("l:{id}"),
            BookmarkEdit::Description { path, line } => format!("b:{path}:{line}"),
        };
        self.rows(cx).iter().position(|row| row.key == key)
    }

    /// Enter in the field: describes the bookmark (a blank description shows its line again), or
    /// names the list. A name another list has, or no name, keeps the field open with the reason.
    fn commit_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.editing.clone() else { return };
        let text = self.edit_field.read(cx).text().to_owned();
        let own = match &target {
            BookmarkEdit::Description { .. } => None,
            BookmarkEdit::ListName(id) => Some(Some(id.as_str())),
            BookmarkEdit::NewList(_) => Some(None),
        };
        if let Some(own) = own
            && let Some(error) = validate_list_name(&self.store.data().lists, &text, own)
        {
            let i18n = &mut cx.default_global::<Localization>().0;
            self.edit_error = Some(name_error_text(&error, i18n));
            cx.notify();
            return;
        }
        self.end_editing(window, cx);
        match target {
            BookmarkEdit::Description { path, line } => {
                self.apply(|store| store.update(|items| describe_bookmark(items, &path, line, &text)), cx);
            }
            BookmarkEdit::ListName(id) => {
                self.apply(|store| store.update_data(|data| rename_list(data, &id, &text)), cx);
            }
            BookmarkEdit::NewList(refs) => {
                let mut created = String::new();
                self.apply(
                    |store| {
                        store.update_data(|data| {
                            let (next, id) = create_list(data, &text);
                            let moved = move_bookmarks(&next, &refs, &id);
                            created = id;
                            moved
                        })
                    },
                    cx,
                );
                if !created.is_empty() {
                    self.collapsed.remove(&format!("l:{created}"));
                    self.select_only(&format!("l:{created}"), cx);
                }
            }
        }
    }

    /// Escape in the field: nothing changes.
    fn cancel_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.end_editing(window, cx);
    }

    fn end_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        self.edit_error = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// A toolbar button was pressed.
    fn press(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match id {
            "bookmarks-new-list" => self.start_editing(BookmarkEdit::NewList(Vec::new()), "", window, cx),
            "bookmarks-remove-all" => {
                self.apply(
                    |store| {
                        store.update_data(|data| BookmarkData { items: Vec::new(), paths: Vec::new(), ..data.clone() })
                    },
                    cx,
                );
            }
            "bookmarks-expand-all" => self.expand_all(cx),
            "bookmarks-collapse-all" => self.collapse_all(cx),
            _ => {}
        }
    }

    /// Moves the lead `delta` rows (to an end for the extremes); Shift extends the selection.
    fn step(&mut self, delta: isize, shift: bool, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(last) = rows.len().checked_sub(1) else { return };
        let current = self.lead.as_deref().and_then(|lead| rows.iter().position(|row| row.key == lead));
        let next = match (current, delta) {
            (_, isize::MIN) => 0,
            (_, isize::MAX) => last,
            (None, _) => 0,
            (Some(at), delta) => at.saturating_add_signed(delta).min(last),
        };
        let key = rows[next].key.clone();
        if shift {
            self.select_range(&key, cx);
        } else {
            self.select_only(&key, cx);
        }
        self.scroll.scroll_to_item(next, ScrollStrategy::Nearest);
    }

    /// Right opens a closed row, then steps into it; Left closes an open row, else goes to its parent.
    fn horizontal(&mut self, right: bool, cx: &mut Context<Self>) {
        let rows = self.rows(cx);
        let Some(at) = self.lead.as_deref().and_then(|lead| rows.iter().position(|row| row.key == lead)) else {
            return;
        };
        let row = &rows[at];
        if (right && row.expandable && !row.expanded) || (!right && row.expanded) {
            self.toggle(&row.key, cx);
        } else if right && row.expanded {
            if let Some(child) = rows.get(at + 1) {
                self.select_only(&child.key, cx);
            }
        } else if !right && let Some(parent) = rows[..at].iter().rev().find(|above| above.depth < row.depth) {
            self.select_only(&parent.key, cx);
        }
    }

    /// The keys the tree handles itself while it has the keyboard, ahead of the keymap (F2 and Delete
    /// are shortcuts elsewhere). Returns whether `keystroke` was one of them.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return false;
        }
        let shift = modifiers.shift;
        match keystroke.key.as_str() {
            "up" => self.step(-1, shift, cx),
            "down" => self.step(1, shift, cx),
            "home" => self.step(isize::MIN, shift, cx),
            "end" => self.step(isize::MAX, shift, cx),
            "left" | "right" if !shift => self.horizontal(keystroke.key == "right", cx),
            "enter" | "f4" if !shift => {
                let Some(lead) = self.lead.clone() else { return false };
                self.activate(&lead, cx);
            }
            "f2" if !shift => return self.rename_selected(window, cx),
            "delete" | "backspace" if !shift => return self.remove_selected(cx),
            _ => return false,
        }
        true
    }

    /// Enter and Escape while the field has the keyboard.
    fn claim_edit_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.commit_edit(window, cx),
            "escape" => self.cancel_edit(window, cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div().h(px(32.0)).flex_none().flex().items_center().gap(px(2.0)).px(px(6.0));
        for button in self.toolbar() {
            if button.id == "bookmarks-expand-all" {
                bar = bar.child(div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(theme::color(cx, "border")));
            }
            bar = bar.child(tool_button(&button, cx));
        }
        bar
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let shortcut = cx
            .try_global::<Keymap>()
            .and_then(|keymap| keymap.0.shortcut_label(TOGGLE_BOOKMARK_ACTION, is_mac_host()))
            .unwrap_or_else(|| "F3".to_owned());
        let text = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["No bookmarks yet. Toggle a line bookmark with ", ""], &[Param::from(shortcut)]);
        div()
            .debug_selector(|| "bookmarks-empty".to_owned())
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p(px(16.0))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(text))
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::new(self.rows(cx));
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let selected: Rc<HashSet<String>> = Rc::new(self.selected.iter().cloned().collect());
        let editing = self.editing_index(cx);
        let field = self.edit_field.clone();
        let error = self.edit_error.clone();
        let list = uniform_list(
            "bookmarks-tree",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let field = (editing == Some(index)).then(|| (field.clone(), error.clone()));
                        render_row(row, selected.contains(&row.key), selected_bg, row_height, field, cx)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("bookmarks-tree-area").debug_selector(|| "bookmarks-tree".to_owned()).flex_1().min_h_0().child(list)
    }
}

/// A toolbar button, enabled or greyed.
fn tool_button(button: &ToolButton, cx: &mut Context<BookmarksWindow>) -> Stateful<Div> {
    let color = theme::color(cx, if button.enabled { "fg-muted" } else { "fg-faint" });
    let id = button.id;
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(button.pressed, |el| el.bg(theme::color(cx, "bg-selected")))
        .when(button.enabled, |el| {
            el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.press(id, window, cx)))
        })
        .child(theme::icon(cx, button.icon, 16.0, color))
}

/// The mnemonic of a bookmark, in the box that stands for the bookmark icon.
fn mnemonic_badge(cx: &App, mnemonic: char) -> Div {
    div()
        .flex_none()
        .size(px(14.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(2.0))
        .border_1()
        .border_color(theme::color(cx, "accent"))
        .bg(theme::color(cx, "accent-soft"))
        .text_color(theme::color(cx, "fg-strong"))
        .text_size(theme::length(cx, "fs-tiny"))
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(mnemonic.to_string()))
}

/// One row of the tree; `field` is the field in place of the label, with the reason a name is refused.
fn render_row(
    row: &BookmarkRow,
    selected: bool,
    selected_bg: gpui_kit::Hsla,
    row_height: Pixels,
    field: Option<(Entity<TextField>, Option<String>)>,
    cx: &mut Context<BookmarksWindow>,
) -> Stateful<Div> {
    let (fg, strong, subtle, chevron_color) = (
        theme::color(cx, "fg"),
        theme::color(cx, "fg-strong"),
        theme::color(cx, "fg-subtle"),
        theme::color(cx, "fg-icon"),
    );
    let chevron = if row.expandable {
        let name = if row.expanded { "chevron-down" } else { "chevron-right" };
        let key = row.key.clone();
        let selector = format!("bookmarks-toggle:{}", row.key);
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .size(px(16.0))
            .flex_none()
            .child(theme::icon(cx, name, 16.0, chevron_color))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.toggle(&key, cx);
                cx.stop_propagation();
            }))
            .into_any_element()
    } else {
        div().size(px(16.0)).flex_none().into_any_element()
    };
    let icon = match (row.kind, row.mnemonic) {
        (BookmarkRowKind::Mark, Some(mnemonic)) => mnemonic_badge(cx, mnemonic).into_any_element(),
        (BookmarkRowKind::Mark, None) => theme::icon(cx, "bookmark", 16.0, fg),
        (BookmarkRowKind::List | BookmarkRowKind::NewList, _) => theme::icon(cx, "toolwindow-bookmarks", 16.0, fg),
        (BookmarkRowKind::Folder, _) => theme::icon(cx, "folder", 16.0, fg),
        (BookmarkRowKind::File, _) => theme::icon(cx, theme::icons::file_icon_name(&row.label), 16.0, fg),
    };
    let label_color = if selected { strong } else { fg };
    let label =
        match field {
            Some((field, error)) => div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .debug_selector(|| "bookmarks-edit-field".to_owned())
                        .flex_1()
                        .min_w(px(80.0))
                        .px(px(4.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, if error.is_some() { "danger" } else { "border" }))
                        .bg(theme::color(cx, "bg-input"))
                        .child(field),
                )
                .children(error.map(|error| {
                    div().flex_none().text_color(theme::color(cx, "danger")).child(SharedString::from(error))
                }))
                .into_any_element(),
            None => {
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .flex_none()
                            .text_color(label_color)
                            .when(row.is_default, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .child(SharedString::from(row.label.clone())),
                    )
                    .when(row.bookmarked, |el| el.child(theme::icon(cx, "bookmark", 12.0, subtle)))
                    .when(!row.meta.is_empty(), |el| {
                        let meta = SharedString::from(row.meta.clone());
                        el.child(div().min_w_0().overflow_hidden().text_color(subtle).child(meta))
                    })
                    .children(row.code.clone().map(|code| {
                        div().min_w_0().overflow_hidden().text_color(subtle).child(SharedString::from(code))
                    }))
                    .into_any_element()
            }
        };
    let selector = format!("bookmarks-row:{}", row.key);
    let (clicked, menu) = (row.key.clone(), row.key.clone());
    let is_new_list = row.kind == BookmarkRowKind::NewList;
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector.clone())
        .h(row_height)
        .w_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .pl(px(4.0 + INDENT * row.depth as f32))
        .pr(px(8.0))
        .whitespace_nowrap()
        .overflow_hidden()
        .when(selected, |el| el.bg(selected_bg))
        .child(chevron)
        .child(icon)
        .child(label)
        .when(!is_new_list, |el| {
            el.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                if view.editing.is_some() {
                    view.cancel_edit(window, cx);
                }
                window.focus(&view.focus, cx);
                view.click(&clicked, event.modifiers(), cx);
                if event.click_count() >= 2 && !event.modifiers().shift {
                    view.activate(&clicked, cx);
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    if view.editing.is_some() {
                        view.cancel_edit(window, cx);
                    }
                    window.focus(&view.focus, cx);
                    view.context_click(&menu, event.position, cx);
                    cx.stop_propagation();
                }),
            )
        })
}

impl Focusable for BookmarksWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BookmarksWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let toolbar = self.render_toolbar(cx);
        let editing_new_list = matches!(self.editing, Some(BookmarkEdit::NewList(_)));
        let body = if self.is_empty() && !editing_new_list {
            self.render_empty(cx).into_any_element()
        } else {
            self.render_tree(window, cx).into_any_element()
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(toolbar)
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::super::bookmarks_store::{BookmarkList, PathBookmark, bookmarks_key, lists_key};
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BookmarksWindowEvent>>>;

    fn mark(path: &str, line: u32, text: &str) -> Bookmark {
        Bookmark {
            path: path.to_owned(),
            line,
            text: text.to_owned(),
            created: 1,
            mnemonic: None,
            description: None,
            list: None,
        }
    }

    fn path_mark(path: &str, dir: bool, list: &str) -> PathBookmark {
        PathBookmark { path: path.to_owned(), dir, list: list.to_owned(), created: 1 }
    }

    /// The bookmarks of the project at `/p`: `items` in its one list.
    fn store_of(items: Vec<Bookmark>) -> BookmarkStore {
        let mut store = BookmarkStore::default();
        store.load(Some("/p"), None, None);
        let _ = store.update(|_| items);
        store
    }

    /// Two lists. The project's own holds a folder, a bookmarked file and a file with two line bookmarks,
    /// one of them an empty line with a mnemonic; Review holds a described line of another file.
    fn project() -> BookmarkStore {
        let mut store = store_of(vec![
            mark("/p/src/a.ts", 2, "alpha()"),
            Bookmark { mnemonic: Some('A'), ..mark("/p/src/a.ts", 9, "") },
            Bookmark {
                description: Some("the b".to_owned()),
                list: Some("list2".to_owned()),
                ..mark("/p/src/b.ts", 4, "let b = 1;")
            },
        ]);
        let _ = store.update_data(|data| BookmarkData {
            lists: vec![data.lists[0].clone(), BookmarkList { id: "list2".to_owned(), name: "Review".to_owned() }],
            paths: vec![path_mark("/p/README.md", false, "default"), path_mark("/p/lib", true, "default")],
            ..data.clone()
        });
        store
    }

    const PROJECT_LINES: [&str; 8] = [
        "p  4 bookmarks · default",
        "  lib",
        "  README.md",
        "  a.ts  src",
        "    a.ts:3  alpha()",
        "    [A] a.ts:10  (empty line)",
        "Review  1 bookmark",
        "  b.ts  src",
    ];

    fn open(
        cx: &mut TestAppContext,
        store: BookmarkStore,
    ) -> (Entity<BookmarksWindow>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| BookmarksWindow::new(store, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &BookmarksWindowEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn lines(view: &Entity<BookmarksWindow>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, cx| view.lines(cx))
    }

    fn data(view: &Entity<BookmarksWindow>, cx: &mut VisualTestContext) -> BookmarkData {
        view.read_with(cx, |view, _| view.store().data().clone())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
        cx.run_until_parked();
    }

    fn platform() -> Modifiers {
        Modifiers { platform: true, ..Modifiers::default() }
    }

    #[gpui_kit::test]
    async fn shows_each_list_with_its_folders_files_and_line_bookmarks(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, project());
        let mut expected: Vec<String> = PROJECT_LINES.iter().map(|line| (*line).to_owned()).collect();
        expected.push("    the b  b.ts:5".to_owned());
        assert_eq!(lines(&view, cx), expected);
        let rows = view.read_with(cx, |view, cx| view.rows(cx));
        assert!(rows[0].is_default && !rows[6].is_default);
        assert!(rows[2].bookmarked && !rows[3].bookmarked);
        assert!(cx.debug_bounds("bookmarks-empty").is_none());
        assert!(events.borrow().is_empty());
    }

    #[gpui_kit::test]
    async fn enter_and_f4_open_a_bookmark_at_its_line_and_a_folder_is_selected_in_the_project_view(
        cx: &mut TestAppContext,
    ) {
        let (_, events, cx) = open(cx, project());
        click(cx, "bookmarks-row:b:/p/src/a.ts:2", Modifiers::default());
        cx.simulate_keystrokes("enter down f4");
        click(cx, "bookmarks-row:d:default:/p/lib", Modifiers::default());
        cx.simulate_keystrokes("enter");
        let path = PathBuf::from("/p/src/a.ts");
        assert_eq!(
            *events.borrow(),
            [
                BookmarksWindowEvent::Open { path: path.clone(), line: Some(2) },
                BookmarksWindowEvent::Open { path, line: Some(9) },
                BookmarksWindowEvent::SelectInProjectView(PathBuf::from("/p/lib")),
            ]
        );
    }

    #[gpui_kit::test]
    async fn a_double_click_on_a_file_opens_it(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, project());
        let bounds = cx.debug_bounds("bookmarks-row:f:default:/p/README.md").expect("the file is drawn");
        cx.simulate_event(MouseDownEvent {
            position: bounds.center(),
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position: bounds.center(),
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
        });
        cx.run_until_parked();
        assert_eq!(
            events.borrow().last(),
            Some(&BookmarksWindowEvent::Open { path: PathBuf::from("/p/README.md"), line: None })
        );
    }

    #[gpui_kit::test]
    async fn delete_removes_the_selected_bookmarks_and_says_what_to_save(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, project());
        click(cx, "bookmarks-row:b:/p/src/a.ts:2", Modifiers::default());
        click(cx, "bookmarks-row:f:default:/p/README.md", platform());
        assert_eq!(view.read_with(cx, |view, _| view.selected()), ["f:default:/p/README.md", "b:/p/src/a.ts:2"]);
        cx.simulate_keystrokes("delete");
        let data = data(&view, cx);
        assert_eq!(data.items.iter().map(|bookmark| bookmark.line).collect::<Vec<_>>(), [9, 4]);
        assert_eq!(data.paths.iter().map(|bookmark| bookmark.path.as_str()).collect::<Vec<_>>(), ["/p/lib"]);
        let Some(BookmarksWindowEvent::Changed(writes)) = events.borrow().last().cloned() else {
            panic!("the change is reported")
        };
        let keys: Vec<String> = writes.into_iter().map(|write| write.key).collect();
        assert_eq!(keys, [bookmarks_key("/p"), lists_key("/p")]);
    }

    #[gpui_kit::test]
    async fn f2_describes_a_line_bookmark_in_place_and_escape_keeps_the_old_text(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, project());
        click(cx, "bookmarks-row:b:/p/src/a.ts:2", Modifiers::default());
        cx.simulate_keystrokes("f2");
        let editing = view.read_with(cx, |view, _| view.editing().cloned());
        assert_eq!(editing, Some(BookmarkEdit::Description { path: "/p/src/a.ts".to_owned(), line: 2 }));
        assert!(cx.debug_bounds("bookmarks-edit-field").is_some());
        cx.simulate_input("entry");
        cx.simulate_keystrokes("enter");
        assert_eq!(lines(&view, cx)[4], "    entry  a.ts:3");
        assert!(view.read_with(cx, |view, _| view.editing().is_none()));
        cx.simulate_keystrokes("f2");
        cx.simulate_input("dropped");
        cx.simulate_keystrokes("escape");
        assert_eq!(lines(&view, cx)[4], "    entry  a.ts:3");
        assert_eq!(data(&view, cx).items[0].description.as_deref(), Some("entry"));
    }

    #[gpui_kit::test]
    async fn f2_renames_a_list_and_a_name_another_list_has_is_refused(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, project());
        click(cx, "bookmarks-row:l:list2", Modifiers::default());
        cx.simulate_keystrokes("f2");
        cx.simulate_input("p");
        cx.simulate_keystrokes("enter");
        let error = view.read_with(cx, |view, _| view.edit_error().map(str::to_owned));
        assert_eq!(error.as_deref(), Some("A list named 'p' already exists"));
        cx.simulate_input("Later");
        assert!(view.read_with(cx, |view, _| view.edit_error().is_none()), "typing clears the reason");
        cx.simulate_keystrokes("enter");
        assert_eq!(data(&view, cx).lists[1].name, "pLater");
    }

    #[gpui_kit::test]
    async fn new_list_from_the_toolbar_is_named_in_place(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, project());
        click(cx, "bookmarks-new-list", Modifiers::default());
        assert!(matches!(view.read_with(cx, |view, _| view.editing().cloned()), Some(BookmarkEdit::NewList(_))));
        cx.simulate_keystrokes("enter");
        let error = view.read_with(cx, |view, _| view.edit_error().map(str::to_owned));
        assert_eq!(error.as_deref(), Some("Name cannot be empty"));
        cx.simulate_input("Later");
        cx.simulate_keystrokes("enter");
        let names: Vec<String> = data(&view, cx).lists.into_iter().map(|list| list.name).collect();
        assert_eq!(names, ["p", "Review", "Later"]);
        assert_eq!(view.read_with(cx, |view, _| view.selected()), ["l:list3"]);
        assert_eq!(lines(&view, cx).last().map(String::as_str), Some("Later  0 bookmarks"));
        assert!(matches!(events.borrow().last(), Some(BookmarksWindowEvent::Changed(_))));
    }

    #[gpui_kit::test]
    async fn deleting_a_list_with_bookmarks_asks_first(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, project());
        click(cx, "bookmarks-row:l:list2", Modifiers::default());
        cx.simulate_keystrokes("delete");
        assert_eq!(
            events.borrow().last(),
            Some(&BookmarksWindowEvent::ConfirmDeleteList {
                id: "list2".to_owned(),
                name: "Review".to_owned(),
                count: 1
            })
        );
        assert_eq!(data(&view, cx).lists.len(), 2, "nothing is deleted before the answer");
        view.update(cx, |view, cx| view.delete_list("list2", cx));
        assert_eq!(lines(&view, cx), PROJECT_LINES[..6]);
        let mut i18n = I18n::default();
        assert_eq!(delete_list_message("Review", 1, &mut i18n), "Delete the list 'Review' with its 1 bookmark?");
    }

    #[gpui_kit::test]
    async fn the_context_menu_moves_a_bookmark_to_another_list(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, project());
        let at = cx.debug_bounds("bookmarks-row:b:/p/src/a.ts:2").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(BookmarksWindowEvent::ShowMenu(_))));
        let menu = view.read_with(cx, |view, _| view.context_menu());
        let labels: Vec<Option<&str>> = menu.iter().map(MenuItem::label).collect();
        let expected =
            [Some("Jump to Source"), Some("Rename…"), Some("Move to Another List"), None, Some("Delete Bookmark")];
        assert_eq!(labels, expected);
        let MenuItem::Item(entry) = &menu[2] else { panic!("a submenu") };
        assert!(entry.submenu[0].is_disabled(), "the list it is in");
        let move_to = entry.submenu[1].command().cloned().expect("Review moves it");
        cx.update(|window, cx| view.update(cx, |view, cx| view.run_command(move_to, window, cx)));
        cx.run_until_parked();
        let moved = data(&view, cx).items.into_iter().find(|bookmark| bookmark.line == 2).expect("kept");
        assert_eq!(moved.list.as_deref(), Some("list2"));
    }

    #[gpui_kit::test]
    async fn the_context_menu_of_a_list_offers_its_own_rows(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, project());
        let at = cx.debug_bounds("bookmarks-row:l:list2").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let menu = view.read_with(cx, |view, _| view.context_menu());
        let labels: Vec<Option<&str>> = menu.iter().map(MenuItem::label).collect();
        let expected = [Some("New List…"), Some("Rename List…"), Some("Make Default"), None, Some("Delete List")];
        assert_eq!(labels, expected);
        let make_default = menu[2].command().cloned().expect("a command");
        cx.update(|window, cx| view.update(cx, |view, cx| view.run_command(make_default, window, cx)));
        assert_eq!(data(&view, cx).default_list, "list2");
    }

    #[gpui_kit::test]
    async fn collapse_all_and_expand_all_fold_the_lists_and_files(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, project());
        click(cx, "bookmarks-collapse-all", Modifiers::default());
        assert_eq!(lines(&view, cx), ["p  4 bookmarks · default", "Review  1 bookmark"]);
        click(cx, "bookmarks-expand-all", Modifiers::default());
        assert_eq!(lines(&view, cx).len(), 9);
        click(cx, "bookmarks-toggle:f:default:/p/src/a.ts", Modifiers::default());
        assert_eq!(lines(&view, cx).len(), 7);
    }

    #[gpui_kit::test]
    async fn shows_how_to_add_a_bookmark_while_there_is_none(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, store_of(Vec::new()));
        assert!(cx.debug_bounds("bookmarks-empty").is_some());
        let ids: Vec<&str> = view.read_with(cx, |view, _| view.toolbar()).iter().map(|button| button.id).collect();
        assert_eq!(ids, ["bookmarks-new-list", "bookmarks-expand-all", "bookmarks-collapse-all"]);
        view.update(cx, |view, cx| view.set_store(project(), cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("bookmarks-empty").is_none());
        assert!(view.read_with(cx, |view, _| view.toolbar()).iter().any(|button| button.id == "bookmarks-remove-all"));
        click(cx, "bookmarks-remove-all", Modifiers::default());
        assert!(cx.debug_bounds("bookmarks-empty").is_none(), "the second list is still there");
        assert_eq!(lines(&view, cx), ["p  0 bookmarks · default", "Review  0 bookmarks"]);
    }
}
