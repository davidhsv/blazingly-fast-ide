//! The Find Usages tool window ("usages"): one tab per search ("Usages of 'foo' in Project Files"),
//! pinnable and closable, over a tree of the usages found: a "Found usages" row with the count, each
//! file with its folder and count, and each usage's line with the usage marked. Backspace / Delete
//! exclude the selected usage, file or everything (struck through) and move on, or include them
//! again; Enter, F4 or a double click opens a usage; ← / → close and open rows. The toolbar reruns
//! the search, steps through the usages and expands or collapses every file; the read / write
//! buttons show when the server told accesses apart, and "Search in Project and Libraries" widens
//! a search that skipped the libraries. The preview pane on the right shows what the window hands
//! in for the selected usage.
//!
//! The view shows what [`super::usage_search::UsagesTabs`] holds, handed in with
//! [`UsagesView::set_tabs`]; it reports what the user asks for through [`UsagesViewEvent`].

use super::usage_search::{UsageSearch, UsagesTabs};
use super::usages::{
    AccessButton, Usage, UsageAccessOptions, UsageGroup, UsageScope, filter_usage_access, results_label,
    supports_usage_access, tab_title, toggle_excluded_usages, trim_preview, usage_count, usage_key,
};
use crate::find::store::StorageWrite;
use crate::find::utf16;
use crate::find::window_store::HeaderMenuItem;
use crate::platform::i18n::{I18n, Localization, Param, format_message};
use crate::platform::input;
use crate::platform::virtual_docs::{basename, relative_dir};
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// The key the view options (the preview pane) are kept under.
pub const VIEW_OPTIONS_KEY: &str = "openidea.usages.view.v1";
/// The id of the ⋮ menu's preview option.
pub const PREVIEW_OPTION: &str = "preview";
/// Rows Page Up / Page Down move.
const PAGE_ROWS: usize = 10;

/// The preview option as kept under [`VIEW_OPTIONS_KEY`] (`{"preview": bool}`); on unless stored off.
pub fn restored_preview_source(stored: Option<&str>) -> bool {
    stored
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .and_then(|value| value.get(PREVIEW_OPTION).and_then(Value::as_bool))
        .unwrap_or(true)
}

/// A row of the tree, as the selection and the context menu name it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RowKey {
    /// The "Found usages" row over every file.
    Summary,
    /// A file, by path.
    File(String),
    /// A usage, by [`usage_key`].
    Usage(String),
}

/// What a context menu row of the tool window does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsagesCommand {
    Pin {
        id: u64,
        pinned: bool,
    },
    CloseTab(u64),
    CloseOtherTabs(u64),
    /// Exclude the usages under the row, or include them again when all are excluded.
    ToggleExcluded(RowKey),
}

/// What the Find Usages tool window asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum UsagesViewEvent {
    /// Open the usage; `focus`: the editor takes the keyboard.
    Open { path: String, line: u32, col: u32, focus: bool },
    /// Select tab `id` ([`UsagesTabs::select`]).
    SelectTab(u64),
    /// Close tab `id` ([`UsagesTabs::close`]).
    CloseTab(u64),
    /// Pin or unpin tab `id` ([`UsagesTabs::pin`]).
    PinTab { id: u64, pinned: bool },
    /// Search tab `id`'s symbol again ([`UsagesTabs::rerun`]).
    Rerun(u64),
    /// Search tab `id`'s symbol in Project and Libraries ([`UsagesTabs::search_libraries`]).
    SearchLibraries(u64),
    /// Run this action (the Previous and Next Occurrence buttons).
    RunAction(&'static str),
    /// Show this context menu at `position`; carry out the chosen row with [`UsagesView::run_command`].
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<UsagesCommand>> },
    /// Keep the access filter or the view options.
    Store(StorageWrite),
    /// The preview pane now shows this usage: supply its view with [`UsagesView::set_preview`].
    /// `None`: the pane is empty.
    PreviewChanged(Option<Usage>),
}

/// A row as text, for the workbench's accessibility and for tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsagesRow {
    pub key: RowKey,
    pub text: String,
    /// Struck through: every usage under it is excluded.
    pub excluded: bool,
}

/// A tab of the header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsagesTabTitle {
    pub id: u64,
    pub title: String,
    pub pinned: bool,
    pub active: bool,
}

/// A button of the toolbar on the left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsagesToolButton {
    pub id: &'static str,
    pub icon: &'static str,
    pub enabled: bool,
}

/// A line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    Summary { expanded: bool },
    File { group: usize, expanded: bool },
    Usage { group: usize, item: usize },
}

/// What the view keeps per search: excluded usages and closed rows.
#[derive(Debug, Clone, Default)]
struct SearchState {
    excluded: HashSet<String>,
    collapsed: HashSet<RowKey>,
}

/// The usage `key` names among `files`.
fn find_usage<'a>(files: &'a [UsageGroup], key: &str) -> Option<&'a Usage> {
    files.iter().flat_map(|group| group.usages.iter()).find(|usage| usage_key(usage) == key)
}

/// The usages under a row: its own, a file's, or every file's.
fn usages_under<'a>(files: &'a [UsageGroup], key: &RowKey) -> Vec<&'a Usage> {
    match key {
        RowKey::Summary => files.iter().flat_map(|group| group.usages.iter()).collect(),
        RowKey::File(path) => {
            files.iter().filter(|group| group.path == *path).flat_map(|group| group.usages.iter()).collect()
        }
        RowKey::Usage(key) => find_usage(files, key).into_iter().collect(),
    }
}

/// The usage a row opens and previews: its own, or a file's first; none for the summary.
fn target_usage<'a>(files: &'a [UsageGroup], key: &RowKey) -> Option<&'a Usage> {
    match key {
        RowKey::Summary => None,
        RowKey::File(path) => files.iter().find(|group| group.path == *path).and_then(|group| group.usages.first()),
        RowKey::Usage(key) => find_usage(files, key),
    }
}

/// The Find Usages tool window.
pub struct UsagesView {
    tabs: Vec<UsageSearch>,
    active: Option<u64>,
    access: UsageAccessOptions,
    preview_source: bool,
    root: String,
    states: HashMap<u64, SearchState>,
    /// The selected search's files, the access filter applied, files left empty dropped.
    files: Rc<Vec<UsageGroup>>,
    rows: Rc<Vec<Row>>,
    selected: Option<RowKey>,
    /// The usage the preview pane was last said to show.
    previewed: Option<Usage>,
    preview: Option<AnyView>,
    focus: FocusHandle,
    scroll: UniformListScrollHandle,
    key_claim: u64,
}

impl EventEmitter<UsagesViewEvent> for UsagesView {}

impl UsagesView {
    /// The tool window with the access filter and the preview option as they were kept.
    pub fn new(access: UsageAccessOptions, preview_source: bool, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self {
            tabs: Vec::new(),
            active: None,
            access,
            preview_source,
            root: String::new(),
            states: HashMap::new(),
            files: Rc::default(),
            rows: Rc::default(),
            selected: None,
            previewed: None,
            preview: None,
            focus,
            scroll: UniformListScrollHandle::new(),
            key_claim,
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Fed by the workbench

    /// The searches changed (a tab opened, closed or was selected, usages arrived).
    pub fn set_tabs(&mut self, tabs: &UsagesTabs, cx: &mut Context<Self>) {
        self.tabs = tabs.tabs().to_vec();
        let active = tabs.active().map(|tab| tab.id);
        if active != self.active {
            self.selected = None;
        }
        self.active = active;
        let ids: HashSet<u64> = self.tabs.iter().map(|tab| tab.id).collect();
        self.states.retain(|id, _| ids.contains(id));
        self.rebuild(cx);
    }

    pub fn set_project_root(&mut self, root: &str, cx: &mut Context<Self>) {
        self.root = root.to_owned();
        cx.notify();
    }

    /// The access filter changed elsewhere (the Show Usages popup shares it).
    pub fn set_access(&mut self, access: UsageAccessOptions, cx: &mut Context<Self>) {
        if self.access != access {
            self.access = access;
            self.rebuild(cx);
        }
    }

    /// The content of the preview pane for `usage`; ignored when the pane shows another usage by now.
    pub fn set_preview(&mut self, usage: &Usage, view: AnyView, cx: &mut Context<Self>) {
        if self.previewed.as_ref() == Some(usage) {
            self.preview = Some(view);
            cx.notify();
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    /// The selected search.
    pub fn search(&self) -> Option<&UsageSearch> {
        self.tabs.iter().find(|tab| Some(tab.id) == self.active)
    }

    /// The tabs as the header shows them.
    pub fn tab_titles(&self, i18n: &I18n) -> Vec<UsagesTabTitle> {
        self.tabs
            .iter()
            .map(|tab| UsagesTabTitle {
                id: tab.id,
                title: tab_title(tab.symbol.as_deref(), tab.scope, i18n),
                pinned: tab.pinned,
                active: Some(tab.id) == self.active,
            })
            .collect()
    }

    /// The entries of the tool window's ⋮ menu.
    pub fn header_menu(&self) -> [HeaderMenuItem; 1] {
        [HeaderMenuItem::Check { id: PREVIEW_OPTION, label: "Preview Source", checked: self.preview_source }]
    }

    /// The toolbar, top to bottom; Rerun only for a search that can run again.
    pub fn toolbar(&self) -> [UsagesToolButton; 5] {
        let rerun = self.search().is_some_and(UsageSearch::can_rerun);
        let button = |id, icon, enabled| UsagesToolButton { id, icon, enabled };
        [
            button("usages-rerun", "rerun", rerun),
            button("usages-previous", "arrow-up", true),
            button("usages-next", "arrow-down", true),
            button("usages-expand-all", "expand-all", true),
            button("usages-collapse-all", "collapse-all", true),
        ]
    }

    pub fn access(&self) -> UsageAccessOptions {
        self.access
    }

    /// The read / write buttons show: the server told accesses apart.
    pub fn shows_access_filter(&self) -> bool {
        supports_usage_access(&self.all_usages())
    }

    pub fn preview_source(&self) -> bool {
        self.preview_source
    }

    /// What the preview pane shows, once the workbench supplied it.
    pub fn preview(&self) -> Option<&AnyView> {
        self.preview.as_ref()
    }

    pub fn selected(&self) -> Option<&RowKey> {
        self.selected.as_ref()
    }

    /// The visible rows: the summary, each open file's usages under it.
    pub fn rows(&self, i18n: &I18n) -> Vec<UsagesRow> {
        let count = self.count_text(i18n);
        let note = self.search().and_then(|search| search.note.clone());
        self.rows
            .iter()
            .map(|row| {
                let key = self.key_of(row);
                let text = match row {
                    Row::Summary { .. } => {
                        let parts = [i18n.tx("Found usages"), count.clone(), note.clone().unwrap_or_default()];
                        parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
                    }
                    Row::File { group, .. } => {
                        let file = &self.files[*group];
                        let parts = [
                            basename(&file.path).into_owned(),
                            relative_dir(&self.root, &file.path).into_owned(),
                            usages_label(file.usages.len(), i18n),
                        ];
                        parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("  ")
                    }
                    Row::Usage { group, item } => {
                        let usage = &self.files[*group].usages[*item];
                        format!("{} {}", usage.line + 1, trim_preview(usage).text)
                    }
                };
                let excluded = self.is_excluded(&key);
                UsagesRow { key, text, excluded }
            })
            .collect()
    }

    /// Every usage under the row is excluded.
    pub fn is_excluded(&self, key: &RowKey) -> bool {
        let excluded = self.excluded();
        let usages = usages_under(&self.files, key);
        !usages.is_empty() && excluded.is_some_and(|excluded| usages.iter().all(|u| excluded.contains(&usage_key(u))))
    }

    /// The summary row's count: "3 results", or "1 / 3 results" while the access filter hides some.
    pub fn count_text(&self, i18n: &I18n) -> String {
        let all = self.all_usages();
        let total = all.len();
        let filtering = supports_usage_access(&all) && !(self.access.read && self.access.write);
        results_label(usage_count(&self.files), total, filtering, i18n)
    }

    /// The status line while the search goes on, or its error.
    pub fn status_text(&self, i18n: &I18n) -> Option<String> {
        let search = self.search()?;
        if search.loading {
            let text = match &search.symbol {
                Some(symbol) => format_message(
                    &i18n.tx("Searching for usages of '{symbol}'"),
                    &[("symbol", Param::from(symbol.as_str()))],
                    i18n.locale(),
                ),
                None => i18n.tx("Searching for usages"),
            };
            return Some(format!("{text}…"));
        }
        search.error.clone()
    }

    /// "Usages in Project Files" shows: the search skipped the libraries.
    pub fn shows_project_scope(&self) -> bool {
        self.search().is_some_and(|search| {
            !search.loading && search.error.is_none() && search.scope == Some(UsageScope::Project)
        })
    }

    /// "Search in Project and Libraries" is offered.
    pub fn offers_libraries(&self) -> bool {
        self.search().is_some_and(UsageSearch::offers_libraries)
    }

    /// What the empty tool window or tree says; `None` while it lists usages, searches or failed.
    pub fn empty_text(&self, i18n: &I18n) -> Option<String> {
        let Some(search) = self.search() else {
            return Some(i18n.tx("Use Find Usages (⌥F7) on a symbol to see its usages here"));
        };
        if !self.rows.is_empty() || search.loading || search.error.is_some() {
            return None;
        }
        Some(i18n.tx(if search.groups.is_empty() { "No usages found" } else { "No usages match the access filters" }))
    }

    // ---------------------------------------------------------------------------------------------
    // Actions

    /// Turns the read or write button on or off.
    pub fn toggle_access(&mut self, which: AccessButton, cx: &mut Context<Self>) {
        let stored = self.access.toggle(which);
        cx.emit(UsagesViewEvent::Store(stored));
        self.rebuild(cx);
    }

    /// Toggles the ⋮ menu option `id`.
    pub fn toggle_option(&mut self, id: &str, cx: &mut Context<Self>) {
        if id != PREVIEW_OPTION {
            return;
        }
        self.preview_source = !self.preview_source;
        let value = json!({ "preview": self.preview_source }).to_string();
        cx.emit(UsagesViewEvent::Store(StorageWrite { key: VIEW_OPTIONS_KEY, value }));
        self.sync_preview(cx);
        cx.notify();
    }

    /// Expand All / Collapse All: every file opens, or every file closes under the summary.
    pub fn set_all_collapsed(&mut self, collapse: bool, cx: &mut Context<Self>) {
        let files: HashSet<RowKey> = match collapse {
            true => self.files.iter().map(|group| RowKey::File(group.path.clone())).collect(),
            false => HashSet::new(),
        };
        let Some(state) = self.state_mut() else { return };
        state.collapsed = files;
        self.rebuild(cx);
    }

    /// Selects row `key`.
    pub fn select_row(&mut self, key: RowKey, cx: &mut Context<Self>) {
        if let Some(index) = self.index_of(&key) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        self.selected = Some(key);
        self.sync_preview(cx);
        cx.notify();
    }

    /// Backspace / Delete: excludes what the selected row holds, or includes it again when all of it
    /// is excluded; excluding moves on to the next usage after the row.
    pub fn exclude_selected(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.selected.clone() else { return };
        if self.toggle_excluded(&key)
            && let Some((next, _, _)) = self.find_occurrence(Direction::Next, true)
        {
            self.reveal(&next);
            self.rebuild(cx);
            self.select_row(next, cx);
        }
        cx.notify();
    }

    /// Next / Previous Occurrence from anywhere: selects the next usage that is not excluded and
    /// opens it, focusing the editor only when `focus_editor`. `None` when there is none that way.
    pub fn step_occurrence(
        &mut self,
        direction: Direction,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) -> Option<OccurrenceStep> {
        let (key, index, total) = self.find_occurrence(direction, false)?;
        self.reveal(&key);
        self.rebuild(cx);
        self.select_row(key.clone(), cx);
        if let Some(usage) = target_usage(&self.files, &key).cloned() {
            self.open(&usage, focus_editor, cx);
        }
        Some(OccurrenceStep { index, total })
    }

    /// The context menu of tab `id`.
    pub fn tab_menu(&self, id: u64, i18n: &I18n) -> Vec<MenuItem<UsagesCommand>> {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == id) else { return Vec::new() };
        let pin = if tab.pinned { "Unpin Tab" } else { "Pin Tab" };
        vec![
            MenuItem::item(i18n.tx(pin), UsagesCommand::Pin { id, pinned: !tab.pinned }),
            MenuItem::Separator,
            MenuItem::item(i18n.tx("Close Tab"), UsagesCommand::CloseTab(id)),
            MenuItem::item(i18n.tx("Close Other Tabs"), UsagesCommand::CloseOtherTabs(id))
                .disabled(self.tabs.len() < 2),
        ]
    }

    /// The context menu of row `key`.
    pub fn row_menu(&self, key: &RowKey, i18n: &I18n) -> Vec<MenuItem<UsagesCommand>> {
        let label = if self.is_excluded(key) { "Include" } else { "Exclude" };
        vec![MenuItem::item(i18n.tx(label), UsagesCommand::ToggleExcluded(key.clone())).shortcut("⌫")]
    }

    /// Carries out a context menu row the user chose.
    pub fn run_command(&mut self, command: UsagesCommand, cx: &mut Context<Self>) {
        match command {
            UsagesCommand::Pin { id, pinned } => cx.emit(UsagesViewEvent::PinTab { id, pinned }),
            UsagesCommand::CloseTab(id) => cx.emit(UsagesViewEvent::CloseTab(id)),
            UsagesCommand::CloseOtherTabs(id) => {
                let others: Vec<u64> = self.tabs.iter().map(|tab| tab.id).filter(|other| *other != id).collect();
                for other in others {
                    cx.emit(UsagesViewEvent::CloseTab(other));
                }
            }
            UsagesCommand::ToggleExcluded(key) => {
                self.select_row(key.clone(), cx);
                self.toggle_excluded(&key);
                cx.notify();
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Internals

    fn all_usages(&self) -> Vec<&Usage> {
        self.search()
            .map(|search| search.groups.iter().flat_map(|group| group.usages.iter()).collect())
            .unwrap_or_default()
    }

    fn excluded(&self) -> Option<&HashSet<String>> {
        self.active.and_then(|id| self.states.get(&id)).map(|state| &state.excluded)
    }

    fn state_mut(&mut self) -> Option<&mut SearchState> {
        let id = self.active?;
        Some(self.states.entry(id).or_default())
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let files: Vec<UsageGroup> = self
            .search()
            .map(|search| {
                search
                    .groups
                    .iter()
                    .filter_map(|group| {
                        let usages: Vec<&Usage> = group.usages.iter().collect();
                        let kept: Vec<Usage> = filter_usage_access(&usages, self.access).into_iter().cloned().collect();
                        (!kept.is_empty()).then(|| UsageGroup { path: group.path.clone(), usages: kept })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let collapsed = self.active.and_then(|id| self.states.get(&id)).map(|state| state.collapsed.clone());
        let collapsed = collapsed.unwrap_or_default();
        let mut rows = Vec::new();
        if !files.is_empty() {
            let open = !collapsed.contains(&RowKey::Summary);
            rows.push(Row::Summary { expanded: open });
            if open {
                for (group, file) in files.iter().enumerate() {
                    let expanded = !collapsed.contains(&RowKey::File(file.path.clone()));
                    rows.push(Row::File { group, expanded });
                    if expanded {
                        rows.extend((0..file.usages.len()).map(|item| Row::Usage { group, item }));
                    }
                }
            }
        }
        self.files = Rc::new(files);
        self.rows = Rc::new(rows);
        if self.selected.as_ref().is_some_and(|key| !self.key_exists(key)) {
            self.selected = None;
        }
        self.sync_preview(cx);
        cx.notify();
    }

    fn key_exists(&self, key: &RowKey) -> bool {
        match key {
            RowKey::Summary => !self.files.is_empty(),
            RowKey::File(path) => self.files.iter().any(|group| group.path == *path),
            RowKey::Usage(key) => find_usage(&self.files, key).is_some(),
        }
    }

    fn key_of(&self, row: &Row) -> RowKey {
        match row {
            Row::Summary { .. } => RowKey::Summary,
            Row::File { group, .. } => RowKey::File(self.files[*group].path.clone()),
            Row::Usage { group, item } => RowKey::Usage(usage_key(&self.files[*group].usages[*item])),
        }
    }

    fn index_of(&self, key: &RowKey) -> Option<usize> {
        self.rows.iter().position(|row| self.key_of(row) == *key)
    }

    fn selected_index(&self) -> Option<usize> {
        self.selected.as_ref().and_then(|key| self.index_of(key))
    }

    fn select_index(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.get(index) {
            let key = self.key_of(row);
            self.select_row(key, cx);
        }
    }

    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let wanted = if self.preview_source {
            self.selected.as_ref().and_then(|key| target_usage(&self.files, key)).cloned()
        } else {
            None
        };
        if wanted != self.previewed {
            self.preview = None;
            self.previewed.clone_from(&wanted);
            cx.emit(UsagesViewEvent::PreviewChanged(wanted));
        }
    }

    fn open(&mut self, usage: &Usage, focus: bool, cx: &mut Context<Self>) {
        cx.emit(UsagesViewEvent::Open { path: usage.path.clone(), line: usage.line, col: usage.col, focus });
    }

    /// Enter, F4 or a double click: a usage opens, a file or the summary opens or closes.
    fn activate(&mut self, key: &RowKey, cx: &mut Context<Self>) {
        match key {
            RowKey::Usage(_) => {
                if let Some(usage) = target_usage(&self.files, key).cloned() {
                    self.open(&usage, true, cx);
                }
            }
            RowKey::Summary | RowKey::File(_) => self.toggle_expanded(key, cx),
        }
    }

    fn toggle_expanded(&mut self, key: &RowKey, cx: &mut Context<Self>) {
        let Some(state) = self.state_mut() else { return };
        if !state.collapsed.remove(key) {
            state.collapsed.insert(key.clone());
        }
        self.rebuild(cx);
    }

    /// Opens the rows above `key`.
    fn reveal(&mut self, key: &RowKey) {
        let file = match key {
            RowKey::Summary => None,
            RowKey::File(path) => Some(path.clone()),
            RowKey::Usage(_) => target_usage(&self.files, key).map(|usage| usage.path.clone()),
        };
        let Some(state) = self.state_mut() else { return };
        state.collapsed.remove(&RowKey::Summary);
        if let Some(path) = file {
            state.collapsed.remove(&RowKey::File(path));
        }
    }

    /// Returns `true` when usages were excluded, `false` when they were included again.
    fn toggle_excluded(&mut self, key: &RowKey) -> bool {
        let files = Rc::clone(&self.files);
        let usages = usages_under(&files, key);
        let Some(state) = self.state_mut() else { return false };
        let next = toggle_excluded_usages(&state.excluded, &usages);
        let grew = next.len() > state.excluded.len();
        state.excluded = next;
        grew
    }

    /// The next usage that is not excluded, in tree order (closed rows included), from the selected
    /// row; with `skip_subtree`, after everything under it. The usage's 1-based position among them
    /// and their count come along.
    fn find_occurrence(&self, direction: Direction, skip_subtree: bool) -> Option<(RowKey, usize, usize)> {
        if self.files.is_empty() {
            return None;
        }
        let none = HashSet::new();
        let excluded = self.excluded().unwrap_or(&none);
        let lead = self.selected.as_ref();
        let mut occurrences: Vec<(usize, RowKey)> = Vec::new();
        let (mut lead_at, mut lead_end) = (None, None);
        let mut at = 0;
        if lead == Some(&RowKey::Summary) {
            lead_at = Some(at);
        }
        at += 1;
        for group in self.files.iter() {
            let file = RowKey::File(group.path.clone());
            if lead == Some(&file) {
                lead_at = Some(at);
            }
            at += 1;
            for usage in &group.usages {
                let key = usage_key(usage);
                let row = RowKey::Usage(key.clone());
                if lead == Some(&row) {
                    (lead_at, lead_end) = (Some(at), Some(at));
                }
                if !excluded.contains(&key) {
                    occurrences.push((at, row));
                }
                at += 1;
            }
            if lead == Some(&file) {
                lead_end = Some(at - 1);
            }
        }
        if lead == Some(&RowKey::Summary) {
            lead_end = Some(at - 1);
        }
        let after = if skip_subtree { lead_end } else { lead_at };
        let index = match (lead_at, direction) {
            (None, Direction::Next) => (!occurrences.is_empty()).then_some(0),
            (None, Direction::Previous) => occurrences.len().checked_sub(1),
            (Some(_), Direction::Next) => {
                let after = after.unwrap_or_default();
                occurrences.iter().position(|(row, _)| *row > after)
            }
            (Some(lead_at), Direction::Previous) => occurrences.iter().rposition(|(row, _)| *row < lead_at),
        }?;
        let total = occurrences.len();
        let (_, key) = occurrences.into_iter().nth(index)?;
        Some((key, index + 1, total))
    }

    fn select_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        if Some(id) != self.active {
            cx.emit(UsagesViewEvent::SelectTab(id));
        }
    }

    fn show_tab_menu(&mut self, id: u64, position: Point<Pixels>, cx: &mut Context<Self>) {
        let items = self.tab_menu(id, &cx.default_global::<Localization>().0);
        if !items.is_empty() {
            cx.emit(UsagesViewEvent::ShowMenu { position, items });
        }
    }

    fn show_row_menu(&mut self, index: usize, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else { return };
        let key = self.key_of(row);
        self.select_row(key.clone(), cx);
        let items = self.row_menu(&key, &cx.default_global::<Localization>().0);
        cx.emit(UsagesViewEvent::ShowMenu { position, items });
    }

    fn press(&mut self, id: &'static str, cx: &mut Context<Self>) {
        match id {
            "usages-rerun" => {
                if let Some(search) = self.search().filter(|search| search.can_rerun()) {
                    cx.emit(UsagesViewEvent::Rerun(search.id));
                }
            }
            "usages-previous" => cx.emit(UsagesViewEvent::RunAction("navigate.prevOccurrence")),
            "usages-next" => cx.emit(UsagesViewEvent::RunAction("navigate.nextOccurrence")),
            "usages-expand-all" => self.set_all_collapsed(false, cx),
            "usages-collapse-all" => self.set_all_collapsed(true, cx),
            _ => {}
        }
    }

    fn search_libraries(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = self.search().filter(|search| search.offers_libraries()) {
            cx.emit(UsagesViewEvent::SearchLibraries(search.id));
        }
    }

    /// The keys the tree handles while it has the keyboard, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        if modifiers.platform || modifiers.control || modifiers.alt || modifiers.function {
            return false;
        }
        let key = keystroke.key.as_str();
        let Some(last) = self.rows.len().checked_sub(1) else { return false };
        if matches!(key, "backspace" | "delete") {
            self.exclude_selected(cx);
            return true;
        }
        if modifiers.shift {
            return false;
        }
        let at = self.selected_index();
        let row = at.and_then(|at| self.rows.get(at).cloned());
        match (key, row) {
            ("down", _) => self.select_index(at.map_or(0, |at| (at + 1).min(last)), cx),
            ("up", _) => self.select_index(at.map_or(last, |at| at.saturating_sub(1)), cx),
            ("home", _) => self.select_index(0, cx),
            ("end", _) => self.select_index(last, cx),
            ("pagedown", _) => self.select_index(at.map_or(0, |at| (at + PAGE_ROWS).min(last)), cx),
            ("pageup", _) => self.select_index(at.map_or(0, |at| at.saturating_sub(PAGE_ROWS)), cx),
            ("right", Some(row @ (Row::Summary { expanded: false } | Row::File { expanded: false, .. }))) => {
                let key = self.key_of(&row);
                self.toggle_expanded(&key, cx);
            }
            ("right", Some(Row::Summary { .. } | Row::File { .. })) => {
                self.select_index(at.map_or(0, |at| (at + 1).min(last)), cx);
            }
            ("left", Some(row @ (Row::Summary { expanded: true } | Row::File { expanded: true, .. }))) => {
                let key = self.key_of(&row);
                self.toggle_expanded(&key, cx);
            }
            ("left", Some(Row::File { .. })) => self.select_row(RowKey::Summary, cx),
            ("left", Some(Row::Usage { group, .. })) => {
                let path = self.files[group].path.clone();
                self.select_row(RowKey::File(path), cx);
            }
            ("enter" | "f4", Some(row)) => {
                let key = self.key_of(&row);
                self.activate(&key, cx);
            }
            _ => return false,
        }
        true
    }
}

/// "1 usage", "3 usages".
fn usages_label(count: usize, i18n: &I18n) -> String {
    format!("{count} {}", i18n.tx(if count == 1 { "usage" } else { "usages" }))
}

impl Focusable for UsagesView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// A toolbar or filter button, greyed when it does nothing now and drawn pressed when on.
fn icon_button(cx: &App, selector: &'static str, icon: &str, enabled: bool, pressed: bool) -> Stateful<Div> {
    let color = theme::color(
        cx,
        if !enabled {
            "fg-faint"
        } else if pressed {
            "fg-strong"
        } else {
            "fg-muted"
        },
    );
    div()
        .id(selector)
        .debug_selector(move || selector.to_owned())
        .size(px(24.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-toolbar-button"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-selected")))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A usage's line, trimmed, with the usage on the search-match colour.
fn marked_line(cx: &App, usage: &Usage) -> Div {
    let preview = trim_preview(usage);
    let parts = [
        (utf16::slice(&preview.text, 0, preview.from).to_owned(), false),
        (utf16::slice(&preview.text, preview.from, preview.to).to_owned(), true),
        (utf16::slice_from(&preview.text, preview.to).to_owned(), false),
    ];
    let match_bg = theme::color(cx, "bg-search-match");
    div().flex().min_w_0().overflow_hidden().whitespace_nowrap().children(
        parts.into_iter().filter(|(text, _)| !text.is_empty()).map(|(text, marked)| {
            let part = div().child(SharedString::from(text));
            if marked { part.bg(match_bg).rounded(px(2.0)) } else { part }
        }),
    )
}

/// The strings a frame draws, translated once.
struct Labels {
    empty: Option<String>,
    status: Option<String>,
    project_scope: String,
    libraries: String,
    found: String,
    count: String,
    file_counts: Vec<String>,
}

impl Render for UsagesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &cx.default_global::<Localization>().0;
        let labels = Labels {
            empty: self.empty_text(i18n),
            status: self.status_text(i18n),
            project_scope: i18n.tx("Usages in Project Files"),
            libraries: i18n.tx("Search in Project and Libraries"),
            found: i18n.tx("Found usages"),
            count: self.count_text(i18n),
            file_counts: self.files.iter().map(|group| usages_label(group.usages.len(), i18n)).collect(),
        };
        let titles = self.tab_titles(i18n);
        let subtle = theme::color(cx, "fg-subtle");
        let border = theme::color(cx, "border");

        let Some(search) = self.search() else {
            return div()
                .debug_selector(|| "usages-empty".to_owned())
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(theme::length(cx, "fs-ui"))
                .text_color(subtle)
                .child(SharedString::from(labels.empty.unwrap_or_default()))
                .into_any_element();
        };
        let (loading, error) = (search.loading, search.error.is_some());

        let mut toolbar = div()
            .flex_none()
            .w(px(32.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(2.0))
            .py(px(4.0))
            .border_r_1()
            .border_color(border);
        for button in self.toolbar() {
            let id = button.id;
            let mut drawn = icon_button(cx, id, button.icon, button.enabled, false);
            if button.enabled {
                drawn = drawn.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(id, cx)));
            }
            toolbar = toolbar.child(drawn);
            if matches!(id, "usages-rerun" | "usages-next") {
                toolbar = toolbar.child(div().w(px(16.0)).h(px(1.0)).my(px(3.0)).bg(border));
            }
        }

        let access =
            self.shows_access_filter().then(|| {
                let options = self.access;
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .px(px(6.0))
                    .py(px(3.0))
                    .child(icon_button(cx, "usages-read-access", "show-read-access", true, options.read).on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_access(AccessButton::Read, cx)),
                    ))
                    .child(icon_button(cx, "usages-write-access", "show-write-access", true, options.write).on_click(
                        cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_access(AccessButton::Write, cx)),
                    ))
            });
        let summary_line =
            || div().flex_none().h(px(26.0)).flex().items_center().gap(px(6.0)).px(px(10.0)).whitespace_nowrap();
        let scope = self.shows_project_scope().then(|| {
            let link = self.offers_libraries().then(|| {
                div()
                    .id("usages-search-libraries")
                    .debug_selector(|| "usages-search-libraries".to_owned())
                    .flex_none()
                    .text_color(theme::color(cx, "link"))
                    .hover(|el| el.text_color(theme::color(cx, "link-hover")).underline())
                    .child(SharedString::from(labels.libraries.clone()))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.search_libraries(cx)))
            });
            summary_line()
                .debug_selector(|| "usages-project-scope".to_owned())
                .text_color(subtle)
                .child(SharedString::from(labels.project_scope.clone()))
                .children(link)
        });
        let status = labels.status.clone().filter(|_| loading || error).map(|text| {
            summary_line()
                .debug_selector(|| "usages-status".to_owned())
                .text_color(if error { theme::color(cx, "danger") } else { subtle })
                .child(SharedString::from(text))
        });
        let tree = match &labels.empty {
            Some(empty) => div()
                .debug_selector(|| "usages-tree-empty".to_owned())
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(subtle)
                .child(SharedString::from(empty.clone()))
                .into_any_element(),
            None => self.tree(window, &labels, cx),
        };
        let preview = self.preview_source.then(|| {
            div()
                .debug_selector(|| "usages-preview".to_owned())
                .flex_1()
                .min_w(px(160.0))
                .h_full()
                .border_l_1()
                .border_color(border)
                .children(self.preview.clone())
        });
        let body = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .children(access)
            .children(scope)
            .children(status)
            .child(div().flex_1().min_h_0().flex().child(tree).children(preview));
        div()
            .size_full()
            .flex()
            .flex_col()
            .text_size(theme::length(cx, "fs-ui"))
            .child(self.tab_strip(titles, cx))
            .child(div().flex_1().min_h_0().flex().child(toolbar).child(body))
            .into_any_element()
    }
}

impl UsagesView {
    fn tab_strip(&self, titles: Vec<UsagesTabTitle>, cx: &mut Context<Self>) -> Div {
        let (strong, fg, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        div()
            .flex_none()
            .h(theme::length(cx, "h-compact-header"))
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .overflow_hidden()
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .children(titles.into_iter().enumerate().map(|(index, tab)| {
                let id = tab.id;
                let close = div()
                    .id(SharedString::from(format!("usages-tab-close-{index}")))
                    .debug_selector(move || format!("usages-tab-close-{index}"))
                    .size(px(16.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, "close", 12.0, muted))
                    .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(UsagesViewEvent::CloseTab(id));
                        cx.stop_propagation();
                    }));
                div()
                    .id(SharedString::from(format!("usages-tab-{index}")))
                    .debug_selector(move || format!("usages-tab-{index}"))
                    .h(px(22.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(8.0))
                    .pr(px(4.0))
                    .whitespace_nowrap()
                    .rounded(theme::length(cx, "radius-tab"))
                    .text_color(if tab.active { strong } else { fg })
                    .when(tab.active, |el| el.bg(theme::color(cx, "bg-tab-selected")))
                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                    .children(tab.pinned.then(|| theme::icon(cx, "pin", 12.0, muted)))
                    .child(SharedString::from(tab.title))
                    .child(close)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(id, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |view, event: &MouseDownEvent, _, cx| {
                            view.show_tab_menu(id, event.position, cx);
                            cx.stop_propagation();
                        }),
                    )
            }))
    }

    fn tree(&self, window: &Window, labels: &Labels, cx: &mut Context<Self>) -> AnyElement {
        let focused = self.focus.is_focused(window);
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (strong, fg) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg"));
        let (muted, subtle) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let (gutter, chevron_color) = (theme::color(cx, "fg-line-number"), theme::color(cx, "fg-icon"));
        let row_height = theme::length(cx, "h-row");
        let small = theme::length(cx, "fs-small");
        let font = code_font(cx);
        let rows = Rc::clone(&self.rows);
        let files = Rc::clone(&self.files);
        let selected = self.selected_index();
        let root = self.root.clone();
        let excluded: HashSet<usize> =
            (0..rows.len()).filter(|index| self.is_excluded(&self.key_of(&rows[*index]))).collect();
        let note = self.search().and_then(|search| search.note.clone());
        let (found, count) = (labels.found.clone(), labels.count.clone());
        let file_counts = Rc::new(labels.file_counts.clone());
        let list = uniform_list(
            "usages-rows",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let struck = excluded.contains(&index);
                        let line = div()
                            .id(index)
                            .debug_selector(move || format!("usages-row-{index}"))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(selected == Some(index), |el| el.bg(selected_bg));
                        let chevron = |expanded: bool, cx: &App| {
                            div().size(px(16.0)).flex_none().child(theme::icon(
                                cx,
                                if expanded { "chevron-down" } else { "chevron-right" },
                                16.0,
                                chevron_color,
                            ))
                        };
                        let line = match &rows[index] {
                            Row::Summary { expanded } => line
                                .pl(px(4.0))
                                .child(chevron(*expanded, cx))
                                .child(div().flex_none().text_color(fg).child(SharedString::from(found.clone())))
                                .child(div().flex_none().text_color(subtle).child(SharedString::from(count.clone())))
                                .children(note.clone().map(|note| {
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_size(small)
                                        .italic()
                                        .text_color(subtle)
                                        .child(SharedString::from(note))
                                })),
                            Row::File { group, expanded } => {
                                let path = &files[*group].path;
                                let name = basename(path).into_owned();
                                let dir = relative_dir(&root, path).into_owned();
                                line.pl(px(20.0))
                                    .child(chevron(*expanded, cx))
                                    .child(theme::icon(cx, theme::icons::file_icon_name(&name), 16.0, muted))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(strong)
                                            .when(struck, |el| el.line_through().opacity(0.5))
                                            .child(SharedString::from(name)),
                                    )
                                    .child(div().flex_none().text_color(subtle).child(SharedString::from(dir)))
                                    .child(
                                        div()
                                            .flex_none()
                                            .text_color(subtle)
                                            .child(SharedString::from(file_counts[*group].clone())),
                                    )
                            }
                            Row::Usage { group, item } => {
                                let usage = &files[*group].usages[*item];
                                line.pl(px(58.0))
                                    .font_family(font.clone())
                                    .child(
                                        div()
                                            .flex_none()
                                            .min_w(px(28.0))
                                            .flex()
                                            .justify_end()
                                            .text_color(gutter)
                                            .when(struck, |el| el.line_through().opacity(0.5))
                                            .child(SharedString::from((usage.line + 1).to_string())),
                                    )
                                    .child(
                                        marked_line(cx, usage)
                                            .flex_1()
                                            .text_color(fg)
                                            .when(struck, |el| el.line_through().opacity(0.5)),
                                    )
                            }
                        };
                        line.on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                            window.focus(&view.focus, cx);
                            view.select_index(index, cx);
                            if event.click_count() >= 2
                                && let Some(row) = view.rows.get(index)
                            {
                                let key = view.key_of(row);
                                view.activate(&key, cx);
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.show_row_menu(index, event.position, cx);
                                cx.stop_propagation();
                            }),
                        )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .debug_selector(|| "usages-tree".to_owned())
            .flex_1()
            .min_w_0()
            .h_full()
            .track_focus(&self.focus)
            .child(list)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::usage_search::{FetchRequest, TabsEffect, UsageCaret};
    use crate::code_insight::usages::{ACCESS_KEY, FileLines, UsageAccess};
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{DocId, IpcPath, Location, Pos, Range, RequestId};
    use ide_lsp::types::References;
    use std::cell::RefCell;
    use std::path::PathBuf;

    type Events = Rc<RefCell<Vec<UsagesViewEvent>>>;

    const A: &str = "/p/src/a.ts";
    const B: &str = "/p/src/b.ts";

    fn usage(path: &str, line: u32, text: &str) -> Usage {
        Usage::at(path, line, 0, 3, text)
    }

    fn groups() -> Vec<UsageGroup> {
        vec![
            UsageGroup { path: A.to_owned(), usages: vec![usage(A, 2, "foo(order)"), usage(A, 5, "  foo(cart)")] },
            UsageGroup { path: B.to_owned(), usages: vec![usage(B, 1, "foo(order, 2)")] },
        ]
    }

    fn shown(id: u64, groups: Vec<UsageGroup>) -> UsageSearch {
        let mut search = UsageSearch::shown(id, Some("foo"));
        search.groups = groups;
        search
    }

    /// The searches in tabs of their own, the last one selected.
    fn tabs_of(searches: Vec<UsageSearch>) -> UsagesTabs {
        let mut tabs = UsagesTabs::default();
        for mut search in searches {
            search.pinned = true;
            tabs.open_tab(search);
        }
        let ids: Vec<u64> = tabs.tabs().iter().map(|tab| tab.id).collect();
        for id in ids {
            tabs.pin(id, false);
        }
        tabs
    }

    fn open(
        cx: &mut TestAppContext,
        tabs: UsagesTabs,
        access: UsageAccessOptions,
        preview: bool,
    ) -> (Entity<UsagesView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let mut view = UsagesView::new(access, preview, cx);
            view.set_project_root("/p", cx);
            view.set_tabs(&tabs, cx);
            view
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &UsagesViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn texts(view: &Entity<UsagesView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.rows(&I18n::default()).into_iter().map(|row| row.text).collect())
    }

    fn struck(view: &Entity<UsagesView>, cx: &mut VisualTestContext) -> Vec<bool> {
        view.read_with(cx, |view, _| view.rows(&I18n::default()).into_iter().map(|row| row.excluded).collect())
    }

    fn selected(view: &Entity<UsagesView>, cx: &mut VisualTestContext) -> Option<RowKey> {
        view.read_with(cx, |view, _| view.selected().cloned())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
    }

    fn opened(path: &str, line: u32, focus: bool) -> UsagesViewEvent {
        UsagesViewEvent::Open { path: path.to_owned(), line, col: 0, focus }
    }

    fn last_menu(events: &Events) -> Vec<MenuItem<UsagesCommand>> {
        events
            .borrow()
            .iter()
            .rev()
            .find_map(|event| match event {
                UsagesViewEvent::ShowMenu { items, .. } => Some(items.clone()),
                _ => None,
            })
            .expect("a context menu is asked for")
    }

    fn labels(items: &[MenuItem<UsagesCommand>]) -> Vec<Option<(String, bool)>> {
        items
            .iter()
            .map(|item| match item {
                MenuItem::Item(entry) => Some((entry.label.clone(), entry.disabled)),
                MenuItem::Separator | MenuItem::Action(_) => None,
            })
            .collect()
    }

    fn key(path: &str, line: u32) -> RowKey {
        RowKey::Usage(format!("{path}:{line}:0:{line}:3"))
    }

    fn location(path: &str, line: u32) -> Location {
        Location {
            path: IpcPath::from(PathBuf::from(path)),
            range: Range::new(Pos::new(line, 0), Pos::new(line, 3)),
            uri: None,
        }
    }

    /// A Find Usages search of `foo` in Project Files that skipped the libraries and found `lines`
    /// of `a.ts`.
    fn searched_project_files(lines: &[u32]) -> UsagesTabs {
        let caret = UsageCaret {
            doc: DocId(1),
            path: A.to_owned(),
            pos: Pos::new(0, 0),
            word: Some("foo".to_owned()),
            language: None,
        };
        let mut tabs = UsagesTabs::default();
        let mut i18n = I18n::default();
        let id = tabs
            .find(Some(caret), RequestId(1))
            .iter()
            .find_map(|effect| match effect {
                TabsEffect::Fetch { search, .. } => Some(*search),
                _ => None,
            })
            .expect("a search starts");
        tabs.indexes_synced(id, true);
        let found = References {
            locations: lines.iter().map(|line| location(A, *line)).collect(),
            libraries_skipped: true,
            ..References::default()
        };
        let effects = tabs.answered(id, Ok(found), &mut i18n);
        let read = effects
            .iter()
            .find_map(|effect| match effect {
                TabsEffect::Fetch { request: FetchRequest::ReadLines { read, .. }, .. } => Some(*read),
                _ => None,
            })
            .expect("the lines are read");
        let text: Vec<String> = (0..8).map(|line| format!("foo({line})")).collect();
        tabs.lines_read(id, read, &FileLines::from([(A.to_owned(), Some(text))]));
        tabs
    }

    #[gpui_kit::test]
    async fn shows_a_plain_count_for_unclassified_searches_after_a_java_access_mode_was_selected(
        cx: &mut TestAppContext,
    ) {
        let rows = vec![usage("/p/a.ts", 0, "foo()"), usage("/p/a.ts", 1, "foo()")];
        let tabs = tabs_of(vec![shown(123, vec![UsageGroup { path: "/p/a.ts".to_owned(), usages: rows }])]);
        let (view, _, cx) = open(cx, tabs, UsageAccessOptions { read: true, write: false }, false);
        assert_eq!(texts(&view, cx)[0], "Found usages 2 results");
        assert!(!view.read_with(cx, |view, _| view.shows_access_filter()));
        assert!(cx.debug_bounds("usages-read-access").is_none());
    }

    // Not from a spec: the tree lists the usages by file under the summary; Enter, F4 and a double
    // click open a usage, on a file they close and open it.
    #[gpui_kit::test]
    async fn lists_usages_by_file_and_opens_them(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, tabs_of(vec![shown(1, groups())]), UsageAccessOptions::default(), false);
        assert_eq!(
            texts(&view, cx),
            [
                "Found usages 3 results",
                "a.ts  src  2 usages",
                "3 foo(order)",
                "6 foo(cart)",
                "b.ts  src  1 usage",
                "2 foo(order, 2)",
            ]
        );
        cx.simulate_keystrokes("down down down");
        assert_eq!(selected(&view, cx), Some(key(A, 2)));
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().last(), Some(&opened(A, 2, true)));
        cx.simulate_keystrokes("down f4");
        assert_eq!(events.borrow().last(), Some(&opened(A, 5, true)));
        let bounds = cx.debug_bounds("usages-row-5").expect("the last usage is drawn");
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        assert!(events.borrow().contains(&opened(B, 1, true)));
        cx.simulate_keystrokes("left");
        assert_eq!(selected(&view, cx), Some(RowKey::File(B.to_owned())));
        cx.simulate_keystrokes("enter");
        assert_eq!(texts(&view, cx).len(), 5);
        cx.simulate_keystrokes("right");
        assert_eq!(texts(&view, cx).len(), 6);
    }

    // Not from a spec: Backspace excludes the selected usage and moves on to the next, on a file it
    // excludes all of the file, and again includes it; Next Occurrence skips what is excluded.
    #[gpui_kit::test]
    async fn excludes_and_includes_usages_and_steps_past_excluded_ones(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, tabs_of(vec![shown(1, groups())]), UsageAccessOptions::default(), false);
        cx.simulate_keystrokes("down down down backspace");
        assert_eq!(struck(&view, cx), [false, false, true, false, false, false]);
        assert_eq!(selected(&view, cx), Some(key(A, 5)));
        cx.simulate_keystrokes("up up delete");
        assert_eq!(struck(&view, cx), [false, true, true, true, false, false]);
        assert_eq!(selected(&view, cx), Some(key(B, 1)), "excluding a file moves past its usages");
        view.update(cx, |view, cx| view.select_row(RowKey::Summary, cx));
        let step = view.update(cx, |view, cx| view.step_occurrence(Direction::Next, false, cx));
        assert_eq!(step, Some(OccurrenceStep { index: 1, total: 1 }));
        assert_eq!(events.borrow().last(), Some(&opened(B, 1, false)));
        view.update(cx, |view, cx| view.select_row(RowKey::File(A.to_owned()), cx));
        cx.simulate_keystrokes("backspace");
        assert_eq!(struck(&view, cx), [false; 6]);
        assert_eq!(selected(&view, cx), Some(RowKey::File(A.to_owned())), "including stays on the row");
        right_click(cx, "usages-row-2");
        let menu = last_menu(&events);
        assert_eq!(labels(&menu), [Some(("Exclude".to_owned(), false))]);
        let exclude = menu[0].command().cloned().expect("the row can be excluded");
        view.update(cx, |view, cx| view.run_command(exclude, cx));
        assert!(view.read_with(cx, |view, _| view.is_excluded(&key(A, 2))));
        let include = view.read_with(cx, |view, _| labels(&view.row_menu(&key(A, 2), &I18n::default())));
        assert_eq!(include, [Some(("Include".to_owned(), false))]);
    }

    // Not from a spec: Collapse All closes every file under the summary, Next Occurrence opens the
    // rows above the usage it selects, Expand All opens every file.
    #[gpui_kit::test]
    async fn expands_and_collapses_all_and_reveals_occurrences(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, tabs_of(vec![shown(1, groups())]), UsageAccessOptions::default(), false);
        click(cx, "usages-collapse-all");
        assert_eq!(texts(&view, cx), ["Found usages 3 results", "a.ts  src  2 usages", "b.ts  src  1 usage"]);
        let step = view.update(cx, |view, cx| view.step_occurrence(Direction::Previous, true, cx));
        assert_eq!(step, Some(OccurrenceStep { index: 3, total: 3 }));
        assert_eq!(events.borrow().last(), Some(&opened(B, 1, true)));
        assert_eq!(texts(&view, cx).len(), 4);
        click(cx, "usages-expand-all");
        assert_eq!(texts(&view, cx).len(), 6);
        click(cx, "usages-next");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::RunAction("navigate.nextOccurrence")));
    }

    // Not from a spec: the read / write buttons show for classified usages, filter the tree and the
    // count, and are kept.
    #[gpui_kit::test]
    async fn filters_by_read_and_write_access(cx: &mut TestAppContext) {
        let read = Usage { access: Some(UsageAccess::Read), ..usage(A, 1, "x = foo") };
        let write = Usage { access: Some(UsageAccess::Write), ..usage(B, 2, "foo = 1") };
        let group = |path: &str, usage: Usage| UsageGroup { path: path.to_owned(), usages: vec![usage] };
        let tabs = tabs_of(vec![shown(1, vec![group(A, read), group(B, write)])]);
        let (view, events, cx) = open(cx, tabs, UsageAccessOptions::default(), false);
        assert!(view.read_with(cx, |view, _| view.shows_access_filter()));
        click(cx, "usages-write-access");
        assert_eq!(texts(&view, cx), ["Found usages 1 / 2 results", "a.ts  src  1 usage", "2 x = foo"]);
        let stored = events.borrow().iter().find_map(|event| match event {
            UsagesViewEvent::Store(write) if write.key == ACCESS_KEY => Some(write.value.clone()),
            _ => None,
        });
        assert_eq!(stored.as_deref(), Some(r#"{"read":true,"write":false}"#));
        click(cx, "usages-read-access");
        assert_eq!(view.read_with(cx, |view, _| view.access()), UsageAccessOptions { read: false, write: true });
        assert_eq!(texts(&view, cx)[1], "b.ts  src  1 usage");
    }

    // Not from a spec: one tab per search with its scope; selecting, closing and the tab menu's Pin,
    // Close and Close Other Tabs go to the window; Rerun and Search in Project and Libraries ask for
    // a search of the selected tab.
    #[gpui_kit::test]
    async fn tabs_pin_close_rerun_and_search_the_libraries(cx: &mut TestAppContext) {
        let mut tabs = searched_project_files(&[1, 3]);
        let project = tabs.active().map(|tab| tab.id).expect("a tab");
        tabs.pin(project, true);
        tabs.open_tab(shown(100, groups()));
        tabs.pin(project, false);
        let (view, events, cx) = open(cx, tabs.clone(), UsageAccessOptions::default(), false);
        let titles = view.read_with(cx, |view, _| view.tab_titles(&I18n::default()));
        let shown_titles: Vec<(&str, bool)> = titles.iter().map(|tab| (tab.title.as_str(), tab.active)).collect();
        assert_eq!(shown_titles, [("Usages of 'foo' in Project Files", false), ("Usages of 'foo'", true)]);
        assert!(!view.read_with(cx, |view, _| view.toolbar()[0].enabled), "given usages cannot be searched again");
        click(cx, "usages-tab-0");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::SelectTab(project)));
        tabs.select(project);
        view.update(cx, |view, cx| view.set_tabs(&tabs, cx));
        assert!(view.read_with(cx, |view, _| view.toolbar()[0].enabled));
        assert!(cx.debug_bounds("usages-project-scope").is_some());
        click(cx, "usages-search-libraries");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::SearchLibraries(project)));
        click(cx, "usages-rerun");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::Rerun(project)));
        click(cx, "usages-tab-close-1");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::CloseTab(100)));
        right_click(cx, "usages-tab-0");
        let menu = last_menu(&events);
        assert_eq!(
            labels(&menu),
            [
                Some(("Pin Tab".to_owned(), false)),
                None,
                Some(("Close Tab".to_owned(), false)),
                Some(("Close Other Tabs".to_owned(), false)),
            ]
        );
        let pin = menu[0].command().cloned().expect("the tab can be pinned");
        view.update(cx, |view, cx| view.run_command(pin, cx));
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::PinTab { id: project, pinned: true }));
        let others = menu[3].command().cloned().expect("the other tabs can be closed");
        view.update(cx, |view, cx| view.run_command(others, cx));
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::CloseTab(100)));
        tabs.close(100);
        view.update(cx, |view, cx| view.set_tabs(&tabs, cx));
        let single = view.read_with(cx, |view, _| labels(&view.tab_menu(project, &I18n::default())));
        assert_eq!(single[3], Some(("Close Other Tabs".to_owned(), true)));
    }

    // Not from a spec: what the window says with no search, while one searches, after it failed and
    // when it found nothing or the access filter hides everything.
    #[gpui_kit::test]
    async fn says_when_there_is_nothing_to_show(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, UsagesTabs::default(), UsageAccessOptions::default(), true);
        let empty = |view: &Entity<UsagesView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| view.empty_text(&I18n::default()))
        };
        assert_eq!(empty(&view, cx).as_deref(), Some("Use Find Usages (⌥F7) on a symbol to see its usages here"));
        assert!(cx.debug_bounds("usages-empty").is_some());
        let mut loading = shown(1, Vec::new());
        loading.loading = true;
        view.update(cx, |view, cx| view.set_tabs(&tabs_of(vec![loading]), cx));
        assert_eq!(empty(&view, cx), None);
        let status = view.read_with(cx, |view, _| view.status_text(&I18n::default()));
        assert_eq!(status.as_deref(), Some("Searching for usages of 'foo'…"));
        cx.run_until_parked();
        assert!(cx.debug_bounds("usages-status").is_some());
        let mut failed = shown(1, Vec::new());
        failed.error = Some("Server crashed".to_owned());
        view.update(cx, |view, cx| view.set_tabs(&tabs_of(vec![failed]), cx));
        let status = view.read_with(cx, |view, _| view.status_text(&I18n::default()));
        assert_eq!(status.as_deref(), Some("Server crashed"));
        view.update(cx, |view, cx| view.set_tabs(&tabs_of(vec![shown(1, Vec::new())]), cx));
        assert_eq!(empty(&view, cx).as_deref(), Some("No usages found"));
        let read = Usage { access: Some(UsageAccess::Read), ..usage(A, 1, "x = foo") };
        let only_reads = shown(1, vec![UsageGroup { path: A.to_owned(), usages: vec![read] }]);
        view.update(cx, |view, cx| {
            view.set_tabs(&tabs_of(vec![only_reads]), cx);
            view.set_access(UsageAccessOptions { read: false, write: true }, cx);
        });
        assert_eq!(empty(&view, cx).as_deref(), Some("No usages match the access filters"));
    }

    /// The preview pane's content in a window of the test.
    struct Pane;

    impl Render for Pane {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().debug_selector(|| "pane".to_owned()).size_full().child("source")
        }
    }

    // Not from a spec: the preview pane shows what the window hands in for the selected usage, a
    // file's first usage on a file; Preview Source in the ⋮ menu hides it and is kept.
    #[gpui_kit::test]
    async fn previews_the_selected_usage_and_hides_the_preview_on_request(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, tabs_of(vec![shown(1, groups())]), UsageAccessOptions::default(), true);
        cx.simulate_keystrokes("down down");
        let first = usage(A, 2, "foo(order)");
        let previewed = events.borrow().last().cloned();
        assert_eq!(previewed, Some(UsagesViewEvent::PreviewChanged(Some(first))), "a file previews its first usage");
        cx.simulate_keystrokes("down down");
        let second = usage(A, 5, "  foo(cart)");
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::PreviewChanged(Some(second.clone()))));
        let pane: AnyView = cx.update(|_, cx| cx.new(|_| Pane).into());
        view.update(cx, |view, cx| view.set_preview(&second, pane, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("pane").is_some());
        view.update(cx, |view, cx| view.toggle_option(PREVIEW_OPTION, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("usages-preview").is_none());
        assert_eq!(events.borrow().last(), Some(&UsagesViewEvent::PreviewChanged(None)));
        let stored = events.borrow().iter().rev().find_map(|event| match event {
            UsagesViewEvent::Store(write) if write.key == VIEW_OPTIONS_KEY => Some(write.value.clone()),
            _ => None,
        });
        assert_eq!(stored.as_deref(), Some(r#"{"preview":false}"#));
        assert!(!restored_preview_source(stored.as_deref()));
        assert!(restored_preview_source(None));
        let menu = view.read_with(cx, |view, _| view.header_menu());
        assert_eq!(menu, [HeaderMenuItem::Check { id: PREVIEW_OPTION, label: "Preview Source", checked: false }]);
    }
}
