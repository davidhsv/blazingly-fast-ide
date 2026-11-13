//! The Structure tool window: the symbols of the active editor's file as a persistent tree under the
//! file itself. The toolbar sorts the symbols alphabetically, opens the view options (fields,
//! non-public symbols, sorting by visibility), lists the members inherited from supertypes when the
//! file's language server knows them, switches Autoscroll to Source and Autoscroll from Source, and
//! expands or collapses the tree.
//!
//! With Autoscroll from Source the tree selects the symbol around the caret, or around the line a
//! preview of the file shows; with Autoscroll to Source selecting a symbol shows it in the editor.
//! Enter and a double click go to the symbol.
//!
//! The window asks nothing of the backend: the workbench hands it the file, its symbols, what its
//! types inherit and where the caret is, and does what [`StructureWindowEvent`] asks. The view
//! options are the ones the File Structure popup shares ([`StructureOptions`]).

use super::inherited::InheritedMember;
use super::outline::{
    OutlineNode, OutlineViewFilter, all_keys, apply_view_filter, has_fields, has_visibility, key_outline, parent_keys,
    sort_outline, symbol_path_at, with_inherited,
};
use super::structure_view::{OPTIONS_KEY, StructureOptions, symbol_icon};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::storage::UiStorage;
use crate::platform::virtual_docs::basename;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Pixels, Point, Render,
    ScrollStrategy, SharedString, Stateful, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_core::{OutlineItem, Pos, SymbolKind};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// The key of the file's own row, above its symbols. Symbol keys start with `/`.
const FILE_KEY: &str = "file";
/// How far each level of the tree is indented, in pixels.
const INDENT: f32 = 16.0;

/// The inherited members of each type of a file, by [`super::inherited::type_key`].
pub type Inherited = HashMap<String, Vec<InheritedMember>>;

/// One visible row of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureRow {
    pub key: String,
    pub depth: usize,
    pub expandable: bool,
    pub expanded: bool,
    /// `None` for the file's own row.
    pub kind: Option<SymbolKind>,
    pub name: String,
    pub detail: Option<String>,
    /// The supertype an inherited member comes from.
    pub owner: Option<String>,
}

/// A toolbar button with its state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub pressed: bool,
}

/// A row of the View Options menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureCommand {
    ToggleFields,
    ToggleNonPublic,
    ToggleSortByVisibility,
}

/// What the Structure tool window asks of the workbench.
#[derive(Debug, Clone, PartialEq)]
pub enum StructureWindowEvent {
    /// Show `path` with the caret at the 0-based line and UTF-16 column (a symbol's identifier);
    /// `focus` moves the keyboard to the editor, otherwise it stays in the tree.
    Navigate { path: PathBuf, line: u32, col: u32, focus: bool },
    /// Show inherited members was switched: load the file's symbols and what its types inherit
    /// again ([`StructureWindow::outline_loaded`], [`StructureWindow::inherited_loaded`]).
    Reload,
    /// Show [`StructureWindow::view_options_menu`] here; run the chosen row with
    /// [`StructureWindow::run_command`].
    ShowViewOptions(Point<Pixels>),
}

/// The node with `key` among `nodes` and their descendants.
fn find<'a>(nodes: &'a [OutlineNode], key: &str) -> Option<&'a OutlineNode> {
    nodes.iter().find_map(|node| if node.key == key { Some(node) } else { find(&node.children, key) })
}

/// Appends the visible rows of `nodes`, top to bottom.
fn flatten(nodes: &[OutlineNode], depth: usize, collapsed: &HashSet<String>, rows: &mut Vec<StructureRow>) {
    for node in nodes {
        let expandable = !node.children.is_empty();
        let expanded = expandable && !collapsed.contains(&node.key);
        rows.push(StructureRow {
            key: node.key.clone(),
            depth,
            expandable,
            expanded,
            kind: Some(node.item.kind),
            name: node.item.name.clone(),
            detail: node.item.detail.clone().filter(|detail| !detail.is_empty()),
            owner: node.inherited.as_ref().map(|from| from.owner.clone()),
        });
        if expanded {
            flatten(&node.children, depth + 1, collapsed, rows);
        }
    }
}

pub struct StructureWindow {
    focus: FocusHandle,
    /// The active editor's file; `None` without one.
    path: Option<String>,
    /// The file's symbols; `None` until they arrive.
    items: Option<Vec<OutlineItem>>,
    loading: bool,
    /// Why the symbols could not be read, while there are none to show.
    error: Option<String>,
    /// The file's language server answers type hierarchy requests: Show Inherited is offered.
    inherited_supported: bool,
    inherited: Option<Inherited>,
    options: StructureOptions,
    /// The symbols with the view options applied.
    nodes: Vec<OutlineNode>,
    /// The parent of every node.
    parents: HashMap<String, Option<String>>,
    /// Keys of the collapsed rows; the tree starts expanded.
    collapsed: HashSet<String>,
    selected: Option<String>,
    /// Where the caret is in the file.
    caret: Option<Pos>,
    /// The line a preview of the file shows (a Markdown file shown as Preview only).
    shown_line: Option<u32>,
    scroll: UniformListScrollHandle,
    key_claim: u64,
}

impl EventEmitter<StructureWindowEvent> for StructureWindow {}

impl StructureWindow {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self {
            focus,
            path: None,
            items: None,
            loading: false,
            error: None,
            inherited_supported: false,
            inherited: None,
            options: cx.try_global::<UiStorage>().map(StructureOptions::stored).unwrap_or_default(),
            nodes: Vec::new(),
            parents: HashMap::new(),
            collapsed: HashSet::new(),
            selected: None,
            caret: None,
            shown_line: None,
            scroll: UniformListScrollHandle::new(),
            key_claim,
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Fed by the workbench

    /// The active editor shows another file (`None`: no editor): its symbols are being asked for.
    pub fn file_changed(&mut self, path: Option<String>, cx: &mut Context<Self>) {
        if path == self.path {
            self.loading = path.is_some();
            cx.notify();
            return;
        }
        self.loading = path.is_some();
        self.path = path;
        self.items = None;
        self.error = None;
        self.inherited_supported = false;
        self.inherited = None;
        self.selected = None;
        self.caret = None;
        self.shown_line = None;
        self.rebuild();
        cx.notify();
    }

    /// The symbols of `path`; ignored when the window shows another file by now.
    pub fn outline_loaded(&mut self, path: &str, items: Vec<OutlineItem>, cx: &mut Context<Self>) {
        if self.path.as_deref() != Some(path) {
            return;
        }
        self.items = Some(items);
        self.loading = false;
        self.error = None;
        self.rebuild();
        self.follow_source(cx);
        cx.notify();
    }

    /// The symbols of `path` could not be read: the ones shown stay, else the window says why.
    pub fn outline_failed(&mut self, path: &str, message: &str, cx: &mut Context<Self>) {
        if self.path.as_deref() != Some(path) {
            return;
        }
        self.loading = false;
        self.error = Some(message.to_owned());
        cx.notify();
    }

    /// Whether the language server of `path` knows supertypes and, when Show inherited members is
    /// on, what each type of the file inherits.
    pub fn inherited_loaded(
        &mut self,
        path: &str,
        supported: bool,
        inherited: Option<Inherited>,
        cx: &mut Context<Self>,
    ) {
        if self.path.as_deref() != Some(path) {
            return;
        }
        self.inherited_supported = supported;
        self.inherited = inherited;
        self.rebuild();
        self.follow_source(cx);
        cx.notify();
    }

    /// The caret moved in `path` (or another file got the caret: `path` is not the one shown).
    pub fn set_caret(&mut self, path: &str, caret: Option<Pos>, cx: &mut Context<Self>) {
        self.caret = caret.filter(|_| self.path.as_deref() == Some(path));
        self.follow_source(cx);
    }

    /// The line a preview of `path` shows, or `None` once no preview shows it. A preview of
    /// another file changes nothing.
    pub fn set_shown_line(&mut self, path: &str, line: Option<u32>, cx: &mut Context<Self>) {
        if self.path.as_deref() != Some(path) {
            return;
        }
        self.shown_line = line;
        self.follow_source(cx);
    }

    /// The options changed elsewhere (the File Structure popup): reads them again.
    pub fn reload_options(&mut self, cx: &mut Context<Self>) {
        let stored = cx.try_global::<UiStorage>().map(StructureOptions::stored).unwrap_or_default();
        if stored != self.options {
            self.options = stored;
            self.rebuild();
            self.follow_source(cx);
            cx.notify();
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    pub fn options(&self) -> &StructureOptions {
        &self.options
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    /// The visible rows: the file, then its symbols.
    pub fn rows(&self) -> Vec<StructureRow> {
        let Some(path) = self.path.as_deref().filter(|_| self.items.is_some()) else { return Vec::new() };
        let expanded = !self.collapsed.contains(FILE_KEY);
        let mut rows = vec![StructureRow {
            key: FILE_KEY.to_owned(),
            depth: 0,
            expandable: !self.nodes.is_empty(),
            expanded: expanded && !self.nodes.is_empty(),
            kind: None,
            name: basename(path).into_owned(),
            detail: None,
            owner: None,
        }];
        if expanded {
            flatten(&self.nodes, 1, &self.collapsed, &mut rows);
        }
        rows
    }

    /// The rows as text: the name indented two spaces per level; an inherited member names its
    /// supertype after `↑`.
    pub fn lines(&self) -> Vec<String> {
        self.rows()
            .into_iter()
            .map(|row| {
                let owner = row.owner.map(|owner| format!(" ↑{owner}")).unwrap_or_default();
                format!("{}{}{owner}", "  ".repeat(row.depth), row.name)
            })
            .collect()
    }

    /// The name of the selected row.
    pub fn selected(&self) -> Option<String> {
        let key = self.selected.as_deref()?;
        self.rows().into_iter().find(|row| row.key == key).map(|row| row.name)
    }

    /// What the window says in place of the tree: no file, the reason the symbols are missing, or
    /// that they are loading.
    pub fn empty_text(&self) -> Option<&'static str> {
        match (&self.path, &self.items, &self.error) {
            (None, _, _) => Some("No structure to show"),
            (Some(_), None, Some(_)) => Some("Structure unavailable"),
            (Some(_), None, None) if self.loading => Some("Loading…"),
            _ => None,
        }
    }

    /// The toolbar: Sort Alphabetically, View Options (when an option applies to the file), Show
    /// Inherited (when the server knows supertypes), the two autoscroll switches, Expand All and
    /// Collapse All.
    pub fn toolbar(&self) -> Vec<ToolButton> {
        let options = &self.options;
        let button = |id, icon, title, pressed| ToolButton { id, icon, title, pressed };
        let mut buttons = vec![button("structure-sort", "sort", "Sort Alphabetically", options.alphabetical)];
        if !self.view_options_menu().is_empty() {
            let narrowed = !options.show_fields || !options.show_non_public || options.sort_by_visibility;
            buttons.push(button("structure-view-options", "filter", "View Options", narrowed));
        }
        if self.inherited_supported {
            buttons.push(button("structure-show-inherited", "hierarchy", "Show Inherited", options.show_inherited));
        }
        buttons.extend([
            button(
                "structure-autoscroll-to-source",
                "autoscroll-to-source",
                "Autoscroll to Source",
                options.autoscroll_to_source,
            ),
            button(
                "structure-autoscroll-from-source",
                "autoscroll-from-source",
                "Autoscroll from Source",
                options.autoscroll_from_source,
            ),
            button("structure-expand-all", "expand-all", "Expand All", false),
            button("structure-collapse-all", "collapse-all", "Collapse All", false),
        ]);
        buttons
    }

    /// The View Options menu: Show Fields when the file has fields (or they are hidden), and Show
    /// Non-Public and Sort by Visibility when its symbols carry a visibility. A row that is on has a
    /// check mark.
    pub fn view_options_menu(&self) -> Vec<MenuItem<StructureCommand>> {
        let items = self.items.as_deref().unwrap_or_default();
        let options = &self.options;
        let row = |label: &str, on: bool, command| {
            let item = MenuItem::item(label, command);
            if on { item.icon("check") } else { item }
        };
        let mut menu = Vec::new();
        if has_fields(items) || !options.show_fields {
            menu.push(row("Show Fields", options.show_fields, StructureCommand::ToggleFields));
        }
        if has_visibility(items) {
            menu.push(row("Show Non-Public", options.show_non_public, StructureCommand::ToggleNonPublic));
            menu.push(MenuItem::Separator);
            menu.push(row("Sort by Visibility", options.sort_by_visibility, StructureCommand::ToggleSortByVisibility));
        }
        menu
    }

    // -----------------------------------------------------------------------------------------------
    // Changes

    pub fn run_command(&mut self, command: StructureCommand, cx: &mut Context<Self>) {
        self.set_options(
            |options| match command {
                StructureCommand::ToggleFields => options.show_fields = !options.show_fields,
                StructureCommand::ToggleNonPublic => options.show_non_public = !options.show_non_public,
                StructureCommand::ToggleSortByVisibility => options.sort_by_visibility = !options.sort_by_visibility,
            },
            cx,
        );
    }

    /// Changes the view options, for every Structure view from now on. Switching Show inherited
    /// members asks for the symbols again.
    pub fn set_options(&mut self, change: impl FnOnce(&mut StructureOptions), cx: &mut Context<Self>) {
        let before = self.options.clone();
        change(&mut self.options);
        if self.options == before {
            return;
        }
        if cx.has_global::<UiStorage>() {
            match serde_json::to_string(&self.options) {
                Ok(text) => cx.global_mut::<UiStorage>().set(OPTIONS_KEY, text),
                Err(error) => tracing::warn!("structure: cannot store the options: {error}"),
            }
        }
        self.rebuild();
        self.follow_source(cx);
        if before.show_inherited != self.options.show_inherited && self.path.is_some() {
            cx.emit(StructureWindowEvent::Reload);
        }
        cx.notify();
    }

    /// Applies the view options to the file's symbols; a selection that is gone is dropped.
    fn rebuild(&mut self) {
        let keyed = key_outline(self.items.as_deref().unwrap_or_default());
        let shown = self.inherited.as_ref().filter(|_| self.options.show_inherited && self.inherited_supported);
        let all = with_inherited(&keyed, shown);
        let view = OutlineViewFilter { fields: self.options.show_fields, non_public: self.options.show_non_public };
        self.nodes =
            sort_outline(&apply_view_filter(&all, view), self.options.alphabetical, self.options.sort_by_visibility);
        self.parents = parent_keys(&self.nodes);
        if self.selected.as_deref().is_some_and(|key| key != FILE_KEY && find(&self.nodes, key).is_none()) {
            self.selected = None;
        }
    }

    /// Autoscroll from Source: selects the innermost symbol around the line a preview shows, else
    /// around the caret, opening what folds it away. Nothing is shown in the editor for it.
    fn follow_source(&mut self, cx: &mut Context<Self>) {
        if !self.options.autoscroll_from_source {
            return;
        }
        let at = self.shown_line.map(|line| Pos::new(line, 0)).or(self.caret);
        let Some(key) = at.and_then(|at| symbol_path_at(&self.nodes, at).pop()) else { return };
        self.collapsed.remove(FILE_KEY);
        let mut above = self.parents.get(&key).cloned().flatten();
        while let Some(parent) = above {
            self.collapsed.remove(&parent);
            above = self.parents.get(&parent).cloned().flatten();
        }
        if let Some(index) = self.rows().iter().position(|row| row.key == key) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Center);
        }
        self.selected = Some(key);
        cx.notify();
    }

    /// Asks the workbench to show the symbol with `key`; the file's own row goes nowhere.
    fn navigate(&mut self, key: &str, focus: bool, cx: &mut Context<Self>) {
        let (Some(path), Some(node)) = (self.path.as_deref(), find(&self.nodes, key)) else { return };
        let at = node.item.selection_range.start;
        let path = PathBuf::from(node.inherited.as_ref().map_or(path, |from| from.path.as_str()));
        cx.emit(StructureWindowEvent::Navigate { path, line: at.line, col: at.col, focus });
    }

    /// Selects the row `key` as the user does: with Autoscroll to Source the editor shows it.
    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() == Some(key) {
            return;
        }
        self.selected = Some(key.to_owned());
        if self.options.autoscroll_to_source {
            self.navigate(key, false, cx);
        }
        cx.notify();
    }

    /// Enter and a double click: goes to the symbol, with the keyboard.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        self.navigate(key, true, cx);
    }

    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(key) {
            self.collapsed.insert(key.to_owned());
            // A selection inside the folded row moves to it.
            let mut above = self.selected.as_ref().and_then(|selected| self.parents.get(selected).cloned().flatten());
            let inside_file = self.selected.as_deref().is_some_and(|selected| selected != FILE_KEY);
            while let Some(at) = above {
                if at == key {
                    self.selected = Some(key.to_owned());
                    break;
                }
                above = self.parents.get(&at).cloned().flatten();
            }
            if key == FILE_KEY && inside_file {
                self.selected = Some(FILE_KEY.to_owned());
            }
        }
        cx.notify();
    }

    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed.clear();
        cx.notify();
    }

    /// Collapses every symbol that has members; the file stays open.
    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.collapsed = all_keys(&self.nodes).into_iter().collect();
        let rows = self.rows();
        let visible = |key: &str| rows.iter().any(|row| row.key == key);
        let mut key = self.selected.clone();
        while let Some(at) = key.clone().filter(|at| !visible(at)) {
            key = self.parents.get(&at).cloned().flatten();
        }
        self.selected = key;
        cx.notify();
    }

    /// A toolbar button was pressed at `position`.
    fn press(&mut self, id: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        match id {
            "structure-sort" => self.set_options(|options| options.alphabetical = !options.alphabetical, cx),
            "structure-view-options" => cx.emit(StructureWindowEvent::ShowViewOptions(position)),
            "structure-show-inherited" => {
                self.set_options(|options| options.show_inherited = !options.show_inherited, cx);
            }
            "structure-autoscroll-to-source" => {
                self.set_options(|options| options.autoscroll_to_source = !options.autoscroll_to_source, cx);
            }
            "structure-autoscroll-from-source" => {
                self.set_options(|options| options.autoscroll_from_source = !options.autoscroll_from_source, cx);
            }
            "structure-expand-all" => self.expand_all(cx),
            "structure-collapse-all" => self.collapse_all(cx),
            _ => {}
        }
    }

    /// Moves the selection to row `index` and keeps it in view.
    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().into_iter().nth(index) else { return };
        self.select(&row.key, cx);
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
    }

    /// The keys the tree handles while it has the keyboard, ahead of the keymap: the arrows walk and
    /// fold the tree, Home and End go to its ends, Enter goes to the symbol.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let rows = self.rows();
        let Some(last) = rows.len().checked_sub(1) else { return false };
        let at = self.selected.as_deref().and_then(|key| rows.iter().position(|row| row.key == key));
        match (keystroke.key.as_str(), at) {
            ("down", None) | ("home", _) => self.select_index(0, cx),
            ("down", Some(at)) => self.select_index((at + 1).min(last), cx),
            ("up", None) | ("end", _) => self.select_index(last, cx),
            ("up", Some(at)) => self.select_index(at.saturating_sub(1), cx),
            ("right", Some(at)) if rows[at].expandable && !rows[at].expanded => self.toggle(&rows[at].key, cx),
            ("right", Some(at)) if rows[at].expanded => self.select_index(at + 1, cx),
            ("left", Some(at)) if rows[at].expanded => self.toggle(&rows[at].key, cx),
            ("left", Some(at)) => {
                if let Some(parent) = rows[..at].iter().rposition(|above| above.depth < rows[at].depth) {
                    self.select_index(parent, cx);
                }
            }
            ("enter", Some(at)) => self.activate(&rows[at].key, cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let mut bar = div().h(px(32.0)).flex_none().flex().items_center().gap(px(2.0)).px(px(6.0));
        for button in self.toolbar() {
            match button.id {
                "structure-autoscroll-to-source" => bar = bar.child(separator(cx)),
                "structure-expand-all" => bar = bar.child(div().flex_1()),
                _ => {}
            }
            bar = bar.child(tool_button(&button, cx));
        }
        bar
    }

    fn render_empty(&self, text: &str, cx: &mut Context<Self>) -> Div {
        let i18n = &cx.default_global::<Localization>().0;
        let title = SharedString::from(i18n.tx(text));
        let error = self.error.clone().filter(|_| self.items.is_none() && self.path.is_some());
        div()
            .debug_selector(|| "structure-empty".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .p(px(12.0))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(title)
            .children(
                error.map(|error| div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(error))),
            )
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = Rc::new(self.rows());
        let no_symbols = SharedString::from(cx.default_global::<Localization>().0.tx("no symbols"));
        let empty_file = self.nodes.is_empty();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, strong, muted, subtle, chevron_color) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "fg-subtle"),
            theme::color(cx, "fg-icon"),
        );
        let selected = self.selected.clone();
        let list = uniform_list(
            "structure-tree",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let chevron = if row.expandable {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let key = row.key.clone();
                            div()
                                .id("toggle")
                                .debug_selector(move || format!("structure-toggle-{index}"))
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
                        let is_selected = selected.as_deref() == Some(row.key.as_str());
                        let name_color = if row.owner.is_some() {
                            muted
                        } else if is_selected {
                            strong
                        } else {
                            fg
                        };
                        let icon = match row.kind {
                            Some(kind) => theme::icon(cx, symbol_icon(kind), 16.0, fg),
                            None => theme::icon(cx, theme::icons::file_icon_name(&row.name), 16.0, fg),
                        };
                        let file_note = (row.kind.is_none() && empty_file).then(|| no_symbols.clone());
                        let clicked = row.key.clone();
                        div()
                            .id(index)
                            .debug_selector(move || format!("structure-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + INDENT * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(is_selected, |el| el.bg(selected_bg))
                            .child(chevron)
                            .child(icon)
                            .child(div().flex_none().text_color(name_color).child(SharedString::from(row.name.clone())))
                            .children(row.detail.clone().map(|detail| {
                                div().min_w_0().overflow_hidden().text_color(subtle).child(SharedString::from(detail))
                            }))
                            .children(
                                row.owner.as_ref().map(|owner| {
                                    div().text_color(subtle).child(SharedString::from(format!("↑{owner}")))
                                }),
                            )
                            .children(file_note.map(|note| div().text_color(subtle).child(note)))
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.select(&clicked, cx);
                                if event.click_count() >= 2 {
                                    view.activate(&clicked, cx);
                                }
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("structure-tree-area").debug_selector(|| "structure-tree".to_owned()).flex_1().min_h_0().child(list)
    }
}

/// The line between groups of toolbar buttons.
fn separator(cx: &App) -> Div {
    div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(theme::color(cx, "border"))
}

/// A toolbar button, drawn pressed when its option is on.
fn tool_button(button: &ToolButton, cx: &mut Context<StructureWindow>) -> Stateful<Div> {
    let color = theme::color(cx, if button.pressed { "fg-strong" } else { "fg-muted" });
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
        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
        .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| view.press(id, event.position(), cx)))
        .child(theme::icon(cx, button.icon, 16.0, color))
}

impl Focusable for StructureWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for StructureWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        if self.items.is_none() || self.path.is_none() {
            let text = self.empty_text().unwrap_or_default();
            return root.child(self.render_empty(text, cx));
        }
        root.child(self.render_toolbar(cx)).child(self.render_tree(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use super::super::inherited::type_key;
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<StructureWindowEvent>>>;

    const PATH: &str = "/p/Order.kt";

    fn item(name: &str, kind: SymbolKind, line: u32, children: Vec<OutlineItem>) -> OutlineItem {
        let len = name.len() as u32;
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind,
            range: ide_core::Range::new(Pos::new(line, 0), Pos::new(line + 1, 1)),
            selection_range: ide_core::Range::new(Pos::new(line, 4), Pos::new(line, 4 + len)),
            children,
            visibility: None,
        }
    }

    fn order() -> Vec<OutlineItem> {
        let members = vec![item("list", SymbolKind::Method, 2, vec![]), item("create", SymbolKind::Method, 8, vec![])];
        vec![item("OrderController", SymbolKind::Class, 0, members), item("helper", SymbolKind::Function, 22, vec![])]
    }

    fn open(cx: &mut TestAppContext) -> (Entity<StructureWindow>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| StructureWindow::new(cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &StructureWindowEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    /// The window on `items` as the symbols of `path`.
    fn show(view: &Entity<StructureWindow>, path: &str, items: Vec<OutlineItem>, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| {
            view.file_changed(Some(path.to_owned()), cx);
            view.outline_loaded(path, items, cx);
        });
        cx.run_until_parked();
    }

    fn lines(view: &Entity<StructureWindow>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn navigations(events: &Events) -> Vec<(u32, bool)> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                StructureWindowEvent::Navigate { line, focus, .. } => Some((*line, *focus)),
                _ => None,
            })
            .collect()
    }

    #[gpui_kit::test]
    async fn shows_the_outline_as_an_expanded_tree_left_and_right_collapse_and_expand(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        show(&view, PATH, order(), cx);
        assert_eq!(lines(&view, cx), ["Order.kt", "  OrderController", "    list", "    create", "  helper"]);
        click(cx, "structure-row-1");
        cx.simulate_keystrokes("left");
        assert_eq!(lines(&view, cx), ["Order.kt", "  OrderController", "  helper"]);
        cx.simulate_keystrokes("right");
        assert_eq!(lines(&view, cx), ["Order.kt", "  OrderController", "    list", "    create", "  helper"]);
        cx.simulate_keystrokes("right");
        assert_eq!(view.read_with(cx, |view, _| view.selected()).as_deref(), Some("list"));
    }

    #[gpui_kit::test]
    async fn offers_show_inherited_members_only_when_the_server_knows_type_hierarchies(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, PATH, order(), cx);
        let ids = |view: &Entity<StructureWindow>, cx: &mut VisualTestContext| -> Vec<&'static str> {
            view.read_with(cx, |view, _| view.toolbar().iter().map(|button| button.id).collect())
        };
        assert!(!ids(&view, cx).contains(&"structure-show-inherited"));
        let controller = order().remove(0);
        let member = InheritedMember {
            item: item("toString", SymbolKind::Method, 40, vec![]),
            path: "/lib/Object.kt".to_owned(),
            owner: "Any".to_owned(),
        };
        let inherited = Inherited::from([(type_key(&controller), vec![member])]);
        view.update(cx, |view, cx| view.inherited_loaded(PATH, true, Some(inherited), cx));
        assert_eq!(lines(&view, cx), ["Order.kt", "  OrderController", "    list", "    create", "  helper"]);
        assert!(ids(&view, cx).contains(&"structure-show-inherited"));
        click(cx, "structure-show-inherited");
        assert!(view.read_with(cx, |view, _| view.options().show_inherited));
        assert_eq!(events.borrow().last(), Some(&StructureWindowEvent::Reload));
        assert_eq!(
            lines(&view, cx),
            ["Order.kt", "  OrderController", "    list", "    create", "    toString ↑Any", "  helper"]
        );
        // An inherited member lives in the file of its supertype.
        click(cx, "structure-row-4");
        let last = events.borrow().last().cloned();
        let expected =
            StructureWindowEvent::Navigate { path: PathBuf::from("/lib/Object.kt"), line: 40, col: 4, focus: false };
        assert_eq!(last, Some(expected));
    }

    #[gpui_kit::test]
    async fn follows_the_line_a_view_shows_instead_of_the_editor_else_the_caret(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let path = "/p/README.md";
        let section = |name: &str, line: u32| OutlineItem {
            range: ide_core::Range::new(Pos::new(line, 0), Pos::new(line + 9, 0)),
            ..item(name, SymbolKind::Namespace, line, vec![])
        };
        view.update(cx, |view, cx| {
            view.set_options(|options| options.autoscroll_from_source = true, cx);
        });
        show(&view, path, vec![section("H0", 0), section("H1", 10), section("H2", 20), section("H3", 30)], cx);
        let selected =
            |view: &Entity<StructureWindow>, cx: &mut VisualTestContext| view.read_with(cx, |view, _| view.selected());
        view.update(cx, |view, cx| view.set_caret(path, Some(Pos::new(12, 0)), cx));
        assert_eq!(selected(&view, cx).as_deref(), Some("H1"));
        view.update(cx, |view, cx| view.set_shown_line(path, Some(25), cx));
        assert_eq!(selected(&view, cx).as_deref(), Some("H2"));
        view.update(cx, |view, cx| view.set_shown_line("/p/other.md", Some(35), cx));
        assert_eq!(selected(&view, cx).as_deref(), Some("H2"));
        view.update(cx, |view, cx| view.set_shown_line(path, None, cx));
        assert_eq!(selected(&view, cx).as_deref(), Some("H1"));
        assert!(navigations(&events).is_empty(), "following the source does not move the editor");
    }

    #[gpui_kit::test]
    async fn selecting_a_symbol_shows_it_and_enter_goes_there(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        show(&view, PATH, order(), cx);
        click(cx, "structure-row-4");
        cx.simulate_keystrokes("enter");
        assert_eq!(navigations(&events), [(22, false), (22, true)]);
        click(cx, "structure-autoscroll-to-source");
        click(cx, "structure-row-2");
        assert_eq!(navigations(&events).len(), 2, "without Autoscroll to Source a selection stays in the tree");
        cx.simulate_keystrokes("up enter");
        assert_eq!(navigations(&events), [(22, false), (22, true), (0, true)]);
    }

    #[gpui_kit::test]
    async fn sorts_alphabetically_and_folds_everything(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        show(&view, PATH, order(), cx);
        click(cx, "structure-sort");
        assert_eq!(lines(&view, cx), ["Order.kt", "  helper", "  OrderController", "    create", "    list"]);
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(OPTIONS_KEY).map(str::to_owned));
        assert!(stored.is_some_and(|stored| stored.contains(r#""alphabetical":true"#)));
        click(cx, "structure-collapse-all");
        assert_eq!(lines(&view, cx), ["Order.kt", "  helper", "  OrderController"]);
        click(cx, "structure-expand-all");
        assert_eq!(lines(&view, cx).len(), 5);
        assert!(view.read_with(cx, |view, _| view.view_options_menu().is_empty()), "no option applies to the file");
    }

    #[gpui_kit::test]
    async fn view_options_hide_non_public_symbols(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let private = OutlineItem {
            visibility: Some(ide_core::Visibility::Private),
            ..item("load", SymbolKind::Method, 2, vec![])
        };
        let field = item("cache", SymbolKind::Field, 1, vec![]);
        show(&view, PATH, vec![item("Repo", SymbolKind::Class, 0, vec![field, private])], cx);
        click(cx, "structure-view-options");
        assert!(matches!(events.borrow().last(), Some(StructureWindowEvent::ShowViewOptions(_))));
        let menu = view.read_with(cx, |view, _| view.view_options_menu());
        let labels: Vec<Option<&str>> = menu.iter().map(MenuItem::label).collect();
        assert_eq!(labels, [Some("Show Fields"), Some("Show Non-Public"), None, Some("Sort by Visibility")]);
        view.update(cx, |view, cx| view.run_command(StructureCommand::ToggleNonPublic, cx));
        assert_eq!(lines(&view, cx), ["Order.kt", "  Repo", "    cache"]);
        let toolbar = view.read_with(cx, |view, _| view.toolbar());
        let pressed = toolbar.into_iter().find(|button| button.id == "structure-view-options");
        assert!(pressed.is_some_and(|button| button.pressed));
    }

    #[gpui_kit::test]
    async fn says_why_there_is_no_tree(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No structure to show"));
        view.update(cx, |view, cx| view.file_changed(Some(PATH.to_owned()), cx));
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("Loading…"));
        view.update(cx, |view, cx| view.outline_failed(PATH, "no server", cx));
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("Structure unavailable"));
        cx.run_until_parked();
        assert!(cx.debug_bounds("structure-empty").is_some());
        view.update(cx, |view, cx| view.outline_loaded("/p/Other.kt", order(), cx));
        assert!(view.read_with(cx, |view, _| view.lines().is_empty()), "the symbols of another file are dropped");
        view.update(cx, |view, cx| view.outline_loaded(PATH, Vec::new(), cx));
        assert_eq!(lines(&view, cx), ["Order.kt"]);
    }
}
