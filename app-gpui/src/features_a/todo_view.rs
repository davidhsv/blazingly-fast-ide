//! The TODO tool window: the tabs Project, Current File, Scope Based and Changes, each a tree of the
//! files with TODO items (with their count) and, under a file, its items at their line and column with
//! the keyword emphasised in the pattern's colour. The header filters by keyword, switches Match
//! Case, rescans the project and steps through the items; the Scope Based tab chooses its scope and
//! the Changes tab its change list. Beside the tree a preview pane shows the selected item's file.
//!
//! The view scans nothing and owns no editor: the workbench hands it the items and the project's
//! state ([`TodoData`]) and the preview pane's content ([`TodoView::set_preview`]), and does what
//! [`TodoViewEvent`] asks. The tab, the hidden keywords, the scope and the change list are the
//! view's own ([`TodoViewState`]); the scope and the view options are kept between sessions.

use crate::features_a::todo::{
    BUILTIN_TODO_SCOPES, TODO_VIEW_OPTIONS_KEY, TodoGroup, TodoItem, TodoMap, TodoScope, TodoSettings, TodoSources,
    TodoTab, TodoViewOptions, TodoViewState, active_patterns, count_items, count_label, item_parts, position_label,
};
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::search::CustomScope;
use crate::platform::storage::UiStorage;
use crate::platform::virtual_docs::{basename, relative_path};
use crate::theme;
use crate::vcs_ops::changelists::LocalChangelist;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    Keystroke, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// Where the Scope Based tab's scope is kept.
pub const TODO_SCOPE_KEY: &str = "openidea.todo.scope.v1";
/// How far an item is indented under its file, in pixels.
const INDENT: f32 = 20.0;

/// What the workbench knows of the project's TODO items.
#[derive(Clone, Debug, Default)]
pub struct TodoData {
    /// The project scan, by file.
    pub scanned: TodoMap,
    /// The scan of the Scope Based tab's scope, by file.
    pub scoped: TodoMap,
    /// The open editors' text scanned as it is now, by file.
    pub live: TodoMap,
    pub settings: TodoSettings,
    /// The active editor's file.
    pub current: Option<String>,
    pub changelists: Vec<LocalChangelist>,
    pub custom_scopes: Vec<CustomScope>,
    pub project_root: String,
    /// The project is being scanned.
    pub scanning: bool,
    /// The project was scanned at least once.
    pub scanned_once: bool,
    /// Why the project scan failed.
    pub error: Option<String>,
    /// The scan stopped at its limit of matches.
    pub limit_hit: bool,
    /// The scope is being scanned.
    pub scoped_scanning: bool,
    /// The scope `scoped` was scanned for.
    pub scoped_scope: Option<TodoScope>,
}

/// One visible row of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoRow {
    File { key: String, path: String, count: usize, expanded: bool },
    Item { key: String, item: TodoItem },
}

impl TodoRow {
    pub fn key(&self) -> &str {
        match self {
            Self::File { key, .. } | Self::Item { key, .. } => key,
        }
    }
}

/// A header button with its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub enabled: bool,
    pub pressed: bool,
}

/// The menus the view asks to open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TodoMenu {
    /// Show All and a row per keyword.
    Filter,
    /// The Scope Based tab's scopes.
    Scope,
    /// The Changes tab's change lists.
    Changelist,
    /// The tool window's ⋮ menu: Show Preview, Autoscroll to Source.
    Options,
}

/// A row of one of the view's menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoCommand {
    ShowAll,
    ToggleKeyword(String),
    SetScope(TodoScope),
    SetChangelist(String),
    TogglePreview,
    ToggleAutoscroll,
}

/// What the TODO tool window asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum TodoViewEvent {
    /// Open `path` with the caret at the 0-based line and UTF-16 column; `focus` gives the editor
    /// the keyboard, otherwise the file shows as a preview tab.
    Open { path: PathBuf, line: u32, col: u32, focus: bool },
    /// The preview pane now shows this item: supply its editor with [`TodoView::set_preview`].
    /// `None`: the pane is empty.
    PreviewChanged(Option<TodoItem>),
    /// Scan this scope for the Scope Based tab (it was chosen, or the tab shows a stale scan).
    ScanScope(TodoScope),
    /// Scan the project again (Refresh).
    Refresh,
    /// Switch Match Case in the TODO settings.
    ToggleMatchCase,
    /// Edit Scopes…, on the custom scope with this name.
    EditScopes(Option<String>),
    /// Run this action (the Previous and Next Occurrence buttons).
    RunAction(&'static str),
    /// Show [`TodoView::menu_items`] of `menu` here; run the chosen row with [`TodoView::run_command`].
    ShowMenu { menu: TodoMenu, position: Point<Pixels> },
}

/// The tab of a debug selector.
fn tab_id(tab: TodoTab) -> &'static str {
    match tab {
        TodoTab::Project => "todo-tab-project",
        TodoTab::CurrentFile => "todo-tab-file",
        TodoTab::Scope => "todo-tab-scope",
        TodoTab::Changes => "todo-tab-changes",
    }
}

/// The colour token of a CSS colour such as `var(--info)`; `info` for anything else.
fn color_token(color: &str) -> &str {
    color.trim().strip_prefix("var(--").and_then(|rest| rest.strip_suffix(')')).unwrap_or("info")
}

/// The pattern colour of `keyword`.
fn keyword_color<'a>(settings: &'a TodoSettings, keyword: &str) -> &'a str {
    let same = |pattern: &str| {
        if settings.case_sensitive { pattern == keyword } else { pattern.eq_ignore_ascii_case(keyword) }
    };
    settings.patterns.iter().find(|pattern| same(pattern.keyword.trim())).map_or("info", |p| color_token(&p.color))
}

/// The folder of `path` relative to the project at `root`.
fn relative_dir(root: &str, path: &str) -> String {
    let rel = if root.is_empty() { path.into() } else { relative_path(root, path) };
    let name = basename(&rel);
    let dir = rel.strip_suffix(&*name).map_or("", |dir| dir.strip_suffix('/').unwrap_or(dir));
    dir.to_owned()
}

/// The key of an item's row.
fn item_key(item: &TodoItem) -> String {
    format!("{}\u{0}{}:{}", item.path, item.line, item.col)
}

pub struct TodoView {
    focus: FocusHandle,
    data: TodoData,
    state: TodoViewState,
    options: TodoViewOptions,
    /// Paths of the collapsed files.
    collapsed: HashSet<String>,
    /// The key of the selected row.
    selected: Option<String>,
    /// The item the preview pane was last said to show.
    previewed: Option<TodoItem>,
    preview: Option<AnyView>,
    scroll: UniformListScrollHandle,
    key_claim: u64,
}

impl EventEmitter<TodoViewEvent> for TodoView {}

impl TodoView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let storage = cx.try_global::<UiStorage>();
        let options = TodoViewOptions::load(storage.and_then(|storage| storage.get(TODO_VIEW_OPTIONS_KEY)));
        let scope = storage
            .and_then(|storage| storage.get(TODO_SCOPE_KEY))
            .and_then(TodoScope::parse)
            .unwrap_or(TodoScope::ProjectFiles);
        Self {
            focus,
            data: TodoData::default(),
            state: TodoViewState { scope, ..TodoViewState::default() },
            options,
            collapsed: HashSet::new(),
            selected: None,
            previewed: None,
            preview: None,
            scroll: UniformListScrollHandle::new(),
            key_claim,
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Fed by the workbench

    /// The items and the project's state as they are now.
    pub fn set_data(&mut self, data: TodoData, cx: &mut Context<Self>) {
        self.data = data;
        let rows = self.rows();
        if self.selected.as_deref().is_some_and(|key| !rows.iter().any(|row| row.key() == key)) {
            self.selected = None;
        }
        self.sync_preview(cx);
        cx.notify();
    }

    /// The content of the preview pane for `item`; ignored when the pane shows another item by now.
    pub fn set_preview(&mut self, item: &TodoItem, view: AnyView, cx: &mut Context<Self>) {
        if self.previewed.as_ref() == Some(item) {
            self.preview = Some(view);
            cx.notify();
        }
    }

    /// Shows a tab (Show TODO Items in Current File shows the Current File tab).
    pub fn set_tab(&mut self, tab: TodoTab, cx: &mut Context<Self>) {
        if self.state.tab == tab {
            return;
        }
        self.state.tab = tab;
        self.selected = None;
        self.request_scope_scan(cx);
        self.sync_preview(cx);
        cx.notify();
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    pub fn state(&self) -> &TodoViewState {
        &self.state
    }

    pub fn options(&self) -> TodoViewOptions {
        self.options
    }

    /// The groups the tab `tab` shows.
    pub fn groups(&self, tab: TodoTab) -> Vec<TodoGroup> {
        let sources = TodoSources {
            scanned: &self.data.scanned,
            scoped: &self.data.scoped,
            live: &self.data.live,
            patterns: &self.data.settings.patterns,
            current: self.data.current.as_deref(),
            changelists: &self.data.changelists,
        };
        self.state.groups(tab, &sources)
    }

    /// The items on the Project tab: the tool window's badge, `None` when there are none.
    pub fn badge(&self) -> Option<usize> {
        Some(count_items(&self.groups(TodoTab::Project))).filter(|count| *count > 0)
    }

    /// The visible rows of the current tab.
    pub fn rows(&self) -> Vec<TodoRow> {
        let mut rows = Vec::new();
        for group in self.groups(self.state.tab) {
            let expanded = !self.collapsed.contains(&group.path);
            rows.push(TodoRow::File {
                key: group.path.clone(),
                path: group.path.clone(),
                count: group.items.len(),
                expanded,
            });
            if expanded {
                rows.extend(group.items.into_iter().map(|item| TodoRow::Item { key: item_key(&item), item }));
            }
        }
        rows
    }

    /// The rows as text: a file's name, its folder and its count; under it, indented, each item's
    /// position and text.
    pub fn lines(&self, cx: &mut App) -> Vec<String> {
        let i18n = &mut cx.default_global::<Localization>().0;
        let root = self.data.project_root.clone();
        self.rows()
            .into_iter()
            .map(|row| match row {
                TodoRow::File { path, count, .. } => {
                    let dir = relative_dir(&root, &path);
                    let parts = [basename(&path).into_owned(), dir, count_label(count, i18n)];
                    parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("  ")
                }
                TodoRow::Item { item, .. } => format!("  {} {}", position_label(&item), item.text),
            })
            .collect()
    }

    /// What the preview pane shows, once the workbench supplied it.
    pub fn preview(&self) -> Option<&AnyView> {
        self.preview.as_ref()
    }

    /// The key of the selected row.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The tabs as text: the title, the current file's name on Current File, and the count of
    /// items where the tab shows one.
    pub fn tab_titles(&self, cx: &App) -> Vec<String> {
        let tx = |text: &str| cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |l| l.0.tx(text));
        [TodoTab::Project, TodoTab::CurrentFile, TodoTab::Scope, TodoTab::Changes]
            .into_iter()
            .map(|tab| {
                let (title, counted) = match tab {
                    TodoTab::Project => (tx("Project"), true),
                    TodoTab::CurrentFile => {
                        let file = self.data.current.as_deref().map(|path| format!(" {}", basename(path)));
                        (format!("{}{}", tx("Current File"), file.unwrap_or_default()), true)
                    }
                    TodoTab::Scope => (tx("Scope Based"), false),
                    TodoTab::Changes => (tx("Changes"), true),
                };
                match Some(count_items(&self.groups(tab))).filter(|n| counted && *n > 0) {
                    Some(count) => format!("{title} {count}"),
                    None => title,
                }
            })
            .collect()
    }

    /// The header buttons: Previous and Next Occurrence, Filter (pressed while keywords are hidden),
    /// Match Case and Refresh (greyed while scanning).
    pub fn toolbar(&self) -> Vec<ToolButton> {
        let button = |id, icon, title, enabled, pressed| ToolButton { id, icon, title, enabled, pressed };
        vec![
            button("todo-previous", "arrow-up", "Previous Occurrence", true, false),
            button("todo-next", "arrow-down", "Next Occurrence", true, false),
            button("todo-filter", "filter", "Filter TODO Items", true, !self.state.hidden.is_empty()),
            button("todo-match-case", "case", "Match Case", true, self.data.settings.case_sensitive),
            button("todo-refresh", "refresh", "Refresh", !self.data.scanning, false),
        ]
    }

    /// What the tree says in place of rows on the current tab.
    pub fn empty_text(&self) -> Option<&'static str> {
        if !self.rows().is_empty() {
            return None;
        }
        let data = &self.data;
        Some(match self.state.tab {
            TodoTab::CurrentFile if data.current.is_none() => "No file is open",
            TodoTab::Scope if data.scoped_scanning || data.scoped_scope.as_ref() != Some(&self.state.scope) => {
                "Scanning for TODO items…"
            }
            TodoTab::Scope => "No TODO items found in this scope",
            TodoTab::Changes => "No TODO items in changed files",
            TodoTab::Project if data.error.is_some() => "TODO items unavailable",
            TodoTab::Project if data.scanning || !data.scanned_once => "Scanning for TODO items…",
            TodoTab::CurrentFile if data.scanning => "Scanning for TODO items…",
            TodoTab::CurrentFile => "No TODO items in this file",
            TodoTab::Project => "No TODO items found",
        })
    }

    /// The rows of `menu`; a row that is on has a check mark.
    pub fn menu_items(&self, menu: TodoMenu) -> Vec<MenuItem<TodoCommand>> {
        fn checked(item: MenuItem<TodoCommand>, on: bool) -> MenuItem<TodoCommand> {
            if on { item.icon("check") } else { item }
        }
        match menu {
            TodoMenu::Filter => {
                let hidden = &self.state.hidden;
                let mut items = vec![
                    checked(MenuItem::item("Show All", TodoCommand::ShowAll), hidden.is_empty()),
                    MenuItem::Separator,
                ];
                items.extend(active_patterns(&self.data.settings).map(|pattern| {
                    let keyword = pattern.keyword.trim().to_owned();
                    let on = !hidden.contains(&keyword);
                    checked(MenuItem::item(pattern.keyword.clone(), TodoCommand::ToggleKeyword(keyword)), on)
                }));
                items
            }
            TodoMenu::Scope => {
                let scope = &self.state.scope;
                let builtin = BUILTIN_TODO_SCOPES.iter().filter_map(|(value, label)| {
                    let parsed = TodoScope::parse(value)?;
                    let on = parsed == *scope;
                    Some(checked(MenuItem::item(*label, TodoCommand::SetScope(parsed)), on))
                });
                let custom = self.data.custom_scopes.iter().map(|custom| {
                    let parsed = TodoScope::Custom(custom.name.clone());
                    let on = parsed == *scope;
                    checked(MenuItem::item(custom.name.clone(), TodoCommand::SetScope(parsed)), on)
                });
                builtin.chain(custom).collect()
            }
            TodoMenu::Changelist => {
                let chosen = self.chosen_changelist();
                self.data
                    .changelists
                    .iter()
                    .map(|list| {
                        let on = chosen.is_some_and(|chosen| chosen.id == list.id);
                        checked(MenuItem::item(list.name.clone(), TodoCommand::SetChangelist(list.id.clone())), on)
                    })
                    .collect()
            }
            TodoMenu::Options => vec![
                checked(MenuItem::item("Show Preview", TodoCommand::TogglePreview), self.options.preview),
                checked(MenuItem::item("Autoscroll to Source", TodoCommand::ToggleAutoscroll), self.options.autoscroll),
            ],
        }
    }

    /// The change list the Changes tab shows: the chosen one, else the active one, else the first.
    fn chosen_changelist(&self) -> Option<&LocalChangelist> {
        let lists = &self.data.changelists;
        lists
            .iter()
            .find(|list| Some(list.id.as_str()) == self.state.changelist.as_deref())
            .or_else(|| lists.iter().find(|list| list.active))
            .or_else(|| lists.first())
    }

    /// The label of the Scope Based tab's scope.
    fn scope_label(&self) -> String {
        match &self.state.scope {
            TodoScope::Custom(name) => name.clone(),
            scope => {
                let value = scope.value();
                BUILTIN_TODO_SCOPES
                    .iter()
                    .find(|(builtin, _)| *builtin == value)
                    .map_or(value.clone(), |(_, label)| (*label).to_owned())
            }
        }
    }

    // -----------------------------------------------------------------------------------------------
    // Changes

    /// Runs a row of one of the view's menus.
    pub fn run_command(&mut self, command: TodoCommand, cx: &mut Context<Self>) {
        match command {
            TodoCommand::ShowAll => self.state.show_all(),
            TodoCommand::ToggleKeyword(keyword) => self.state.toggle_keyword(&keyword),
            TodoCommand::SetScope(scope) => {
                if cx.has_global::<UiStorage>() {
                    cx.global_mut::<UiStorage>().set(TODO_SCOPE_KEY, scope.value());
                }
                self.state.scope = scope;
                self.request_scope_scan(cx);
            }
            TodoCommand::SetChangelist(id) => self.state.changelist = Some(id),
            TodoCommand::TogglePreview => self.set_options(|options| options.preview = !options.preview, cx),
            TodoCommand::ToggleAutoscroll => self.set_options(|options| options.autoscroll = !options.autoscroll, cx),
        }
        let rows = self.rows();
        if self.selected.as_deref().is_some_and(|key| !rows.iter().any(|row| row.key() == key)) {
            self.selected = None;
        }
        self.sync_preview(cx);
        cx.notify();
    }

    fn set_options(&mut self, change: impl FnOnce(&mut TodoViewOptions), cx: &mut Context<Self>) {
        change(&mut self.options);
        if cx.has_global::<UiStorage>() {
            cx.global_mut::<UiStorage>().set(TODO_VIEW_OPTIONS_KEY, self.options.to_json());
        }
    }

    /// On the Scope Based tab, asks for a scan of the scope when the one shown is of another scope.
    fn request_scope_scan(&mut self, cx: &mut Context<Self>) {
        if self.state.tab == TodoTab::Scope && self.data.scoped_scope.as_ref() != Some(&self.state.scope) {
            cx.emit(TodoViewEvent::ScanScope(self.state.scope.clone()));
        }
    }

    /// The item the preview pane shows: the selected item, or the first item of the selected file.
    fn preview_item(&self) -> Option<TodoItem> {
        let key = self.selected.as_deref()?;
        let group = self
            .groups(self.state.tab)
            .into_iter()
            .find(|group| group.path == key || group.items.iter().any(|item| item_key(item) == key))?;
        if group.path == key {
            return group.items.into_iter().next();
        }
        group.items.into_iter().find(|item| item_key(item) == key)
    }

    /// Tells the workbench when the preview pane is to show another item; a hidden pane shows none.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let wanted = self.preview_item().filter(|_| self.options.preview);
        if wanted != self.previewed {
            self.preview = None;
            self.previewed.clone_from(&wanted);
            cx.emit(TodoViewEvent::PreviewChanged(wanted));
        }
    }

    fn open(&mut self, item: &TodoItem, focus: bool, cx: &mut Context<Self>) {
        let (path, line, col) = (PathBuf::from(&item.path), item.line, item.col);
        cx.emit(TodoViewEvent::Open { path, line, col, focus });
    }

    /// Selects the row `key`; with Autoscroll to Source an item opens as a preview tab.
    pub fn select(&mut self, key: &str, cx: &mut Context<Self>) {
        self.selected = Some(key.to_owned());
        if self.options.autoscroll
            && let Some(TodoRow::Item { item, .. }) = self.rows().into_iter().find(|row| row.key() == key)
        {
            self.open(&item, false, cx);
        }
        self.sync_preview(cx);
        cx.notify();
    }

    /// Enter and a double click: opens the item, or a file's first item, with the keyboard.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let group = self
            .groups(self.state.tab)
            .into_iter()
            .find(|group| group.path == key || group.items.iter().any(|item| item_key(item) == key));
        let Some(group) = group else { return };
        let item = if group.path == key {
            group.items.into_iter().next()
        } else {
            group.items.into_iter().find(|item| item_key(item) == key)
        };
        if let Some(item) = item {
            self.open(&item, true, cx);
        }
    }

    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        if !self.collapsed.remove(path) {
            self.collapsed.insert(path.to_owned());
            if self.selected.as_deref().is_some_and(|key| key.starts_with(&format!("{path}\u{0}"))) {
                self.selected = Some(path.to_owned());
            }
        }
        cx.notify();
    }

    /// Previous and Next Occurrence: selects the item before or after the selection, opening files
    /// that fold it away, and opens it. Returns whether there was one.
    pub fn select_occurrence(&mut self, forward: bool, focus: bool, cx: &mut Context<Self>) -> bool {
        let items: Vec<TodoItem> = self.groups(self.state.tab).into_iter().flat_map(|group| group.items).collect();
        let Some(last) = items.len().checked_sub(1) else { return false };
        let at = self.selected.as_deref().and_then(|key| items.iter().position(|item| item_key(item) == key));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(at), true) if at < last => at + 1,
            (Some(at), false) if at > 0 => at - 1,
            _ => return false,
        };
        let item = items[next].clone();
        self.collapsed.remove(&item.path);
        self.selected = Some(item_key(&item));
        if let Some(index) = self.rows().iter().position(|row| row.key() == item_key(&item)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        self.open(&item, focus, cx);
        self.sync_preview(cx);
        cx.notify();
        true
    }

    /// A header button was pressed at `position`.
    fn press(&mut self, id: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        match id {
            "todo-previous" => cx.emit(TodoViewEvent::RunAction("navigate.prevOccurrence")),
            "todo-next" => cx.emit(TodoViewEvent::RunAction("navigate.nextOccurrence")),
            "todo-filter" => cx.emit(TodoViewEvent::ShowMenu { menu: TodoMenu::Filter, position }),
            "todo-match-case" => cx.emit(TodoViewEvent::ToggleMatchCase),
            "todo-refresh" => cx.emit(TodoViewEvent::Refresh),
            "todo-options" => cx.emit(TodoViewEvent::ShowMenu { menu: TodoMenu::Options, position }),
            "todo-scope" => cx.emit(TodoViewEvent::ShowMenu { menu: TodoMenu::Scope, position }),
            "todo-changelist" => cx.emit(TodoViewEvent::ShowMenu { menu: TodoMenu::Changelist, position }),
            "todo-edit-scopes" => {
                let name = match &self.state.scope {
                    TodoScope::Custom(name) => Some(name.clone()),
                    _ => None,
                };
                cx.emit(TodoViewEvent::EditScopes(name));
            }
            _ => {}
        }
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(row) = self.rows().into_iter().nth(index) else { return };
        self.select(row.key(), cx);
        self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
    }

    /// The keys the tree handles while it has the keyboard, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let rows = self.rows();
        let Some(last) = rows.len().checked_sub(1) else { return false };
        let at = self.selected.as_deref().and_then(|key| rows.iter().position(|row| row.key() == key));
        match (keystroke.key.as_str(), at.map(|at| (at, &rows[at]))) {
            ("down", None) | ("home", _) => self.select_index(0, cx),
            ("down", Some((at, _))) => self.select_index((at + 1).min(last), cx),
            ("up", None) | ("end", _) => self.select_index(last, cx),
            ("up", Some((at, _))) => self.select_index(at.saturating_sub(1), cx),
            ("right", Some((_, TodoRow::File { path, expanded: false, .. }))) => self.toggle(path, cx),
            ("right", Some((at, TodoRow::File { .. }))) => self.select_index(at + 1, cx),
            ("left", Some((_, TodoRow::File { path, expanded: true, .. }))) => self.toggle(path, cx),
            ("left", Some((_, TodoRow::Item { item, .. }))) => {
                let path = item.path.clone();
                self.select(&path, cx);
            }
            ("enter", Some((_, row))) => self.activate(row.key(), cx),
            _ => return false,
        }
        true
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let titles = self.tab_titles(cx);
        let mut header = div()
            .h(px(30.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        let tabs = [TodoTab::Project, TodoTab::CurrentFile, TodoTab::Scope, TodoTab::Changes];
        for (tab, title) in tabs.into_iter().zip(titles) {
            let id = tab_id(tab);
            let selected = self.state.tab == tab;
            header = header.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_owned())
                    .h(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .px(px(8.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(theme::color(cx, if selected { "fg-strong" } else { "fg-muted" }))
                    .when(selected, |el| el.bg(theme::color(cx, "bg-tab-selected")))
                    .when(!selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(SharedString::from(title))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.set_tab(tab, cx))),
            );
        }
        header = header.child(div().flex_1());
        for button in self.toolbar() {
            header = header.child(tool_button(&button, cx));
        }
        let options = ToolButton { id: "todo-options", icon: "more", title: "Options", enabled: true, pressed: false };
        header.child(tool_button(&options, cx))
    }

    /// The bar above the tree: the scope of the Scope Based tab, the change list of the Changes tab
    /// (when there is more than one), or the notice that the project scan hit its limit.
    fn render_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        let bar = |cx: &App| {
            div()
                .h(px(28.0))
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(8.0))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
        };
        let chooser = |id: &'static str, label: String, cx: &mut Context<Self>| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(6.0))
                .h(px(22.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                .child(SharedString::from(label))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
                .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| view.press(id, event.position(), cx)))
        };
        let i18n = &mut cx.default_global::<Localization>().0;
        match self.state.tab {
            TodoTab::Scope => {
                let (title, label) = (i18n.tx("Scope:"), i18n.tx(&self.scope_label()));
                let edit = ToolButton {
                    id: "todo-edit-scopes",
                    icon: "more-horizontal",
                    title: "Edit Scopes…",
                    enabled: true,
                    pressed: false,
                };
                let muted = theme::color(cx, "fg-muted");
                Some(
                    bar(cx)
                        .child(div().text_color(muted).child(SharedString::from(title)))
                        .child(chooser("todo-scope", label, cx))
                        .child(tool_button(&edit, cx)),
                )
            }
            TodoTab::Changes if self.data.changelists.len() > 1 => {
                let title = i18n.tx("Changelist:");
                let chosen = self.chosen_changelist().map(|list| {
                    let files = list.files.len();
                    let strings: [&str; 2] = if files == 1 { ["", " file"] } else { ["", " files"] };
                    format!("{}  {}", list.name, i18n.tx_built(&strings, &[Param::from(files)]))
                });
                let muted = theme::color(cx, "fg-muted");
                Some(bar(cx).child(div().text_color(muted).child(SharedString::from(title))).child(chooser(
                    "todo-changelist",
                    chosen.unwrap_or_default(),
                    cx,
                )))
            }
            TodoTab::Project if self.data.limit_hit => {
                let notice = i18n.tx("Too many matches; showing the first 10 000.");
                Some(bar(cx).text_color(theme::color(cx, "warning")).child(SharedString::from(notice)))
            }
            _ => None,
        }
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = self.rows();
        let i18n = &mut cx.default_global::<Localization>().0;
        let counts: Vec<Option<String>> = rows
            .iter()
            .map(|row| match row {
                TodoRow::File { count, .. } => Some(count_label(*count, i18n)),
                TodoRow::Item { .. } => None,
            })
            .collect();
        let empty = self.empty_text().map(|text| i18n.tx(text));
        let error = self.data.error.clone().filter(|_| self.state.tab == TodoTab::Project);
        let area =
            div().id("todo-tree-area").debug_selector(|| "todo-tree".to_owned()).flex_1().min_w(px(160.0)).h_full();
        if let Some(empty) = empty {
            return area
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(div().debug_selector(|| "todo-empty".to_owned()).child(SharedString::from(empty)))
                .children(
                    error.map(|error| div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(error))),
                );
        }
        let colors: Vec<Option<Hsla>> = rows
            .iter()
            .map(|row| match row {
                TodoRow::Item { item, .. } => Some(theme::color(cx, keyword_color(&self.data.settings, &item.keyword))),
                TodoRow::File { .. } => None,
            })
            .collect();
        let rows = Rc::new(rows);
        let counts = Rc::new(counts);
        let colors = Rc::new(colors);
        let root = self.data.project_root.clone();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, strong, muted, chevron_color) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "fg-icon"),
        );
        let selected = self.selected.clone();
        let list = uniform_list(
            "todo-tree",
            rows.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let row = &rows[index];
                        let is_selected = selected.as_deref() == Some(row.key());
                        let line = div()
                            .id(index)
                            .debug_selector(move || format!("todo-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(is_selected, |el| el.bg(selected_bg));
                        let line = match row {
                            TodoRow::File { path, expanded, .. } => {
                                let name = basename(path).into_owned();
                                let chevron = if *expanded { "chevron-down" } else { "chevron-right" };
                                let toggled = path.clone();
                                line.pl(px(4.0))
                                    .child(
                                        div()
                                            .id("toggle")
                                            .size(px(16.0))
                                            .flex_none()
                                            .child(theme::icon(cx, chevron, 16.0, chevron_color))
                                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                                view.toggle(&toggled, cx);
                                                cx.stop_propagation();
                                            })),
                                    )
                                    .child(theme::icon(cx, theme::icons::file_icon_name(&name), 16.0, fg))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(if is_selected { strong } else { fg })
                                            .child(SharedString::from(name)),
                                    )
                                    .child(div().text_color(muted).child(SharedString::from(relative_dir(&root, path))))
                                    .children(
                                        counts[index]
                                            .clone()
                                            .map(|count| div().text_color(muted).child(SharedString::from(count))),
                                    )
                            }
                            TodoRow::Item { item, .. } => {
                                let (keyword, rest) = item_parts(item);
                                let color = colors[index].unwrap_or(fg);
                                line.pl(px(4.0 + 16.0 + INDENT))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(muted)
                                            .child(SharedString::from(position_label(item))),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_color(color)
                                            .child(
                                                div()
                                                    .font_weight(FontWeight::BOLD)
                                                    .child(SharedString::from(keyword.to_owned())),
                                            )
                                            .child(SharedString::from(rest.to_owned())),
                                    )
                            }
                        };
                        let clicked = row.key().to_owned();
                        line.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
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
        area.child(list)
    }

    fn render_preview(&self, cx: &mut Context<Self>) -> Div {
        let pane = div()
            .debug_selector(|| "todo-preview".to_owned())
            .flex_1()
            .min_w(px(160.0))
            .h_full()
            .border_l_1()
            .border_color(theme::color(cx, "border"));
        match (&self.previewed, &self.preview) {
            (Some(_), Some(view)) => pane.child(view.clone()),
            (Some(_), None) => pane,
            (None, _) => {
                let text = cx.default_global::<Localization>().0.tx("Select an item to preview");
                pane.flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(div().debug_selector(|| "todo-preview-empty".to_owned()).child(SharedString::from(text)))
            }
        }
    }
}

/// A header button, greyed when it does nothing now and drawn pressed when its switch is on.
fn tool_button(button: &ToolButton, cx: &mut Context<TodoView>) -> Stateful<Div> {
    let color = theme::color(
        cx,
        if !button.enabled {
            "fg-faint"
        } else if button.pressed {
            "fg-strong"
        } else {
            "fg-muted"
        },
    );
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
                .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| view.press(id, event.position(), cx)))
        })
        .child(theme::icon(cx, button.icon, 16.0, color))
}

impl Focusable for TodoView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TodoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let bar = self.render_bar(cx);
        let tree = self.render_tree(window, cx);
        let preview = self.options.preview.then(|| self.render_preview(cx));
        div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(header)
            .children(bar)
            .child(div().flex_1().min_h_0().flex().child(tree).children(preview))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_ops::changelists::LocalChange;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<TodoViewEvent>>>;

    fn todo(path: &str, line: u32, col: u32, keyword: &str, text: &str) -> TodoItem {
        TodoItem { path: path.to_owned(), line, col, keyword: keyword.to_owned(), text: text.to_owned() }
    }

    fn scanned(items: Vec<TodoItem>) -> TodoMap {
        let mut map = TodoMap::new();
        for item in items {
            map.entry(item.path.clone()).or_default().push(item);
        }
        map
    }

    fn data(items: Vec<TodoItem>) -> TodoData {
        TodoData { scanned: scanned(items), scanned_once: true, project_root: "/p".to_owned(), ..TodoData::default() }
    }

    fn open(cx: &mut TestAppContext, data: TodoData) -> (Entity<TodoView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| TodoView::new(cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &TodoViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        view.update(cx, |view, cx| view.set_data(data, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn lines(view: &Entity<TodoView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.update(cx, |view, cx| view.lines(cx))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn command(view: &Entity<TodoView>, command: TodoCommand, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| view.run_command(command, cx));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn renders_grouped_items_with_the_keyword_emphasised_and_filters_by_keyword(cx: &mut TestAppContext) {
        let items = vec![
            todo("/p/src/a.ts", 4, 3, "TODO", "TODO: wire it"),
            todo("/p/src/a.ts", 9, 3, "FIXME", "FIXME broken"),
        ];
        let (view, events, cx) = open(cx, data(items));
        assert_eq!(lines(&view, cx), ["a.ts  src  2 items", "  (5, 4) TODO: wire it", "  (10, 4) FIXME broken"]);
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx))[0], "Project 2");
        click(cx, "todo-filter");
        assert!(matches!(events.borrow().last(), Some(TodoViewEvent::ShowMenu { menu: TodoMenu::Filter, .. })));
        command(&view, TodoCommand::ToggleKeyword("FIXME".to_owned()), cx);
        assert_eq!(lines(&view, cx), ["a.ts  src  1 item", "  (5, 4) TODO: wire it"]);
        let toolbar = view.read_with(cx, |view, _| view.toolbar());
        let pressed = toolbar.into_iter().find(|button| button.id == "todo-filter");
        assert!(pressed.is_some_and(|button| button.pressed));
        let menu = view.read_with(cx, |view, _| view.menu_items(TodoMenu::Filter));
        assert_eq!(menu[0].command(), Some(&TodoCommand::ShowAll));
        command(&view, TodoCommand::ShowAll, cx);
        assert_eq!(lines(&view, cx).len(), 3);
    }

    #[gpui_kit::test]
    async fn lists_only_changed_files_on_the_changes_tab_by_change_list(cx: &mut TestAppContext) {
        let change = |path: &str| LocalChange { path: path.to_owned(), deleted: false };
        let changelists = vec![
            LocalChangelist {
                id: "default".to_owned(),
                name: "Changes".to_owned(),
                active: true,
                files: vec![change("/p/src/b.ts")],
            },
            LocalChangelist {
                id: "ui".to_owned(),
                name: "UI work".to_owned(),
                active: false,
                files: vec![change("/p/src/a.ts")],
            },
        ];
        let items = vec![todo("/p/src/a.ts", 0, 3, "TODO", "TODO a"), todo("/p/src/b.ts", 0, 3, "TODO", "TODO b")];
        let (view, events, cx) = open(cx, TodoData { changelists, ..data(items) });
        click(cx, "todo-tab-changes");
        assert_eq!(lines(&view, cx), ["b.ts  src  1 item", "  (1, 4) TODO b"]);
        click(cx, "todo-changelist");
        assert!(matches!(events.borrow().last(), Some(TodoViewEvent::ShowMenu { menu: TodoMenu::Changelist, .. })));
        let menu = view.read_with(cx, |view, _| view.menu_items(TodoMenu::Changelist));
        assert_eq!(menu[1].label(), Some("UI work"));
        let chosen = menu[1].command().cloned().expect("a command");
        command(&view, chosen, cx);
        assert_eq!(view.read_with(cx, |view, _| view.state().changelist.clone()).as_deref(), Some("ui"));
        assert_eq!(lines(&view, cx), ["a.ts  src  1 item", "  (1, 4) TODO a"]);
    }

    #[gpui_kit::test]
    async fn shows_the_preview_pane_and_does_not_autoscroll_by_default(cx: &mut TestAppContext) {
        let items = vec![todo("/p/src/a.ts", 4, 3, "TODO", "TODO: x")];
        let (view, events, cx) = open(cx, data(items));
        assert_eq!(view.read_with(cx, |view, _| view.options()), TodoViewOptions { preview: true, autoscroll: false });
        assert!(cx.debug_bounds("todo-preview-empty").is_some(), "Select an item to preview");
        click(cx, "todo-row-1");
        assert!(
            !events.borrow().iter().any(|event| matches!(event, TodoViewEvent::Open { .. })),
            "selecting does not open the file"
        );
        command(&view, TodoCommand::TogglePreview, cx);
        assert!(cx.debug_bounds("todo-preview").is_none());
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(TODO_VIEW_OPTIONS_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some(r#"{"preview":false,"autoscroll":false}"#));
        command(&view, TodoCommand::TogglePreview, cx);
        assert!(cx.debug_bounds("todo-preview").is_some());
    }

    /// The pane's content in a window of the test.
    struct Pane;

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().debug_selector(|| "pane".to_owned()).size_full().child("editor")
        }
    }

    #[gpui_kit::test]
    async fn previews_the_selected_item_as_an_editable_editor_without_a_switch(cx: &mut TestAppContext) {
        let item = todo("/p/src/a.ts", 4, 3, "TODO", "TODO: x");
        let (view, events, cx) = open(cx, data(vec![item.clone()]));
        click(cx, "todo-row-0");
        assert_eq!(events.borrow().last(), Some(&TodoViewEvent::PreviewChanged(Some(item.clone()))));
        let pane: AnyView = cx.update(|_, cx| cx.new(|_| Pane).into());
        view.update(cx, |view, cx| view.set_preview(&item, pane, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("pane").is_some());
        cx.simulate_keystrokes("enter");
        let opened = TodoViewEvent::Open { path: PathBuf::from("/p/src/a.ts"), line: 4, col: 3, focus: true };
        assert_eq!(events.borrow().last(), Some(&opened));
    }

    #[gpui_kit::test]
    async fn autoscroll_to_source_opens_the_selected_item_without_the_keyboard(cx: &mut TestAppContext) {
        let items = vec![todo("/p/a.ts", 1, 0, "TODO", "TODO one"), todo("/p/a.ts", 7, 2, "HACK", "HACK two")];
        let (view, events, cx) = open(cx, data(items));
        command(&view, TodoCommand::ToggleAutoscroll, cx);
        click(cx, "todo-row-1");
        cx.simulate_keystrokes("down");
        let opened: Vec<(u32, bool)> = events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                TodoViewEvent::Open { line, focus, .. } => Some((*line, *focus)),
                _ => None,
            })
            .collect();
        assert_eq!(opened, [(1, false), (7, false)]);
        cx.simulate_keystrokes("left left");
        assert_eq!(lines(&view, cx), ["a.ts  2 items"]);
    }

    #[gpui_kit::test]
    async fn current_file_and_scope_tabs_say_what_they_wait_for(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, data(vec![todo("/p/a.ts", 1, 0, "TODO", "TODO one")]));
        click(cx, "todo-tab-file");
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No file is open"));
        click(cx, "todo-tab-scope");
        assert_eq!(events.borrow().last(), Some(&TodoViewEvent::ScanScope(TodoScope::ProjectFiles)));
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("Scanning for TODO items…"));
        command(&view, TodoCommand::SetScope(TodoScope::OpenFiles), cx);
        assert_eq!(events.borrow().last(), Some(&TodoViewEvent::ScanScope(TodoScope::OpenFiles)));
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(TODO_SCOPE_KEY).map(str::to_owned));
        assert_eq!(stored.as_deref(), Some("openFiles"));
        let scoped = TodoData {
            scoped: scanned(vec![todo("/p/b.ts", 2, 0, "TODO", "TODO b")]),
            scoped_scope: Some(TodoScope::OpenFiles),
            ..data(Vec::new())
        };
        view.update(cx, |view, cx| view.set_data(scoped, cx));
        assert_eq!(lines(&view, cx), ["b.ts  1 item", "  (3, 1) TODO b"]);
        assert_eq!(view.read_with(cx, |view, cx| view.tab_titles(cx))[2], "Scope Based");
    }
}
