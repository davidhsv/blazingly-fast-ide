//! Recent Files (⌘E) and the Switcher (⌃⇥): the files to go back to, beside the tool windows.
//!
//! Recent Files lists the project's recently opened files and narrows them, and the tool windows, as the
//! user types. The Switcher lists the open editors instead, moves one row per ⌃⇥ and opens the selection
//! when Ctrl is released. [`super::recent_model`] decides the rows and their order; this view asks the
//! backend for the recent files, keeps the selection and tells its window what the user chose.

use super::recent_model::{
    OpenEditorTab, RecentFileItem, RecentFilesOptions, SwitcherColumn, filter_recent_files, initial_file_index,
    switcher_items, switcher_selection, switcher_tool_windows,
};
use crate::backend::Backend;
use crate::editor::tabs::EditorInput;
use crate::find::row_selection::{
    Keyed, RowSelection, SINGLE_FIRST, SelectMode, select_row, selected_rows, selection_mode,
};
use crate::platform::i18n::Localization;
use crate::platform::input::{self, Keymap};
use crate::platform::keymap::is_mac_host;
use crate::theme;
use crate::ui::matching::index_runs;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::workbench::tool_windows::ToolWindow;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Keystroke,
    ModifiersChangedEvent, Render, ScrollStrategy, SharedString, Subscription, Task, UniformListScrollHandle, Window,
    div, prelude::*, px, uniform_list,
};
use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;

/// How many rows Page Up and Page Down move by.
const PAGE_ROWS: usize = 10;
const WIDTH: f32 = 720.0;
const HEADER_HEIGHT: f32 = 40.0;
const COLUMNS_HEIGHT: f32 = 440.0;
const FOOTER_HEIGHT: f32 = 30.0;
const TOOLS_WIDTH: f32 = 230.0;

/// Which of the two popups this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentFilesKind {
    /// Recent Files: the recently opened files, with a speed search.
    Recent,
    /// The Switcher: the open editors. It starts on the previous editor, or on the oldest one when
    /// opened `backward`. `control_held` says whether Ctrl is down as it opens: releasing a Ctrl that
    /// was held opens the selection.
    Switcher { backward: bool, control_held: bool },
}

/// What the popup lists and where it belongs.
#[derive(Debug, Clone, Default)]
pub struct RecentFilesInput {
    /// The window, as the backend knows it.
    pub window_label: String,
    /// The folder of the open project.
    pub root: Option<String>,
    /// The file in the focused editor.
    pub active_path: Option<String>,
    /// The Switcher's rows: every open editor tab once, most recently active first.
    pub tabs: Vec<OpenEditorTab>,
    /// The tool windows to list.
    pub tool_windows: Vec<ToolWindow>,
    /// The files edited in this session, for "Show edited only".
    pub edited: HashSet<String>,
    /// The recent files, most recent first. The backend is asked for them when `None`.
    pub recent: Option<Vec<String>>,
}

/// What the popup asks of its window.
#[derive(Debug, Clone, PartialEq)]
pub enum RecentFilesEvent {
    /// Escape, or Ctrl released in a Switcher with nothing to open.
    Dismissed,
    /// Open the file; `side` asks for the other editor pane.
    OpenFile { path: PathBuf, side: bool },
    /// Open every file of a multi-selection as a tab of its own, in this order, and leave `lead` showing.
    OpenFiles { paths: Vec<PathBuf>, lead: PathBuf, side: bool },
    /// Show an open editor that is not a file.
    OpenEditor { input: EditorInput, side: bool },
    /// Show the tool window with this id and give it the keyboard.
    ActivateToolWindow(String),
    /// Hide the tool window with this id; the popup stays open.
    HideToolWindow(String),
    /// Close every tab showing this file, then confirm with [`RecentFiles::removed`] unless the user
    /// kept it open. The popup stays open.
    ClosePath(String),
    /// Close the tab of the editor with this input key, then confirm with [`RecentFiles::removed`]
    /// unless the user kept it open. The popup stays open.
    CloseInput(String),
    /// Nothing recent matches the typed text: look for it among all the files instead.
    GoToFile(String),
    /// Show Recent Locations in this popup's place.
    ShowRecentLocations,
    /// Show the Switcher in this popup's place.
    ShowSwitcher { backward: bool },
}

/// A tool window row, with the matched characters of its title.
#[derive(Debug, Clone, PartialEq)]
struct ToolRow {
    window: ToolWindow,
    indices: Vec<usize>,
}

/// A file row as the multi-selection sees it: selected by its path.
struct FileKey<'a> {
    index: usize,
    path: &'a str,
}

impl Keyed for FileKey<'_> {
    fn key(&self) -> &str {
        self.path
    }
}

pub struct RecentFiles {
    window_label: String,
    switcher: bool,
    /// The speed search of Recent Files; the Switcher has none.
    field: Option<Entity<TextField>>,
    /// What has the keyboard while the popup is open: the speed search, or the Switcher itself.
    focus: FocusHandle,
    /// The text of the speed search.
    query: String,
    root: Option<String>,
    /// The editor showing now: a file, or the input key of an editor that is not one.
    active_path: Option<String>,
    tabs: Vec<OpenEditorTab>,
    tool_windows: Vec<ToolWindow>,
    edited: HashSet<String>,
    edited_only: bool,
    /// The recent files; unset until the backend has answered.
    paths: Option<Vec<String>>,
    /// Rows the user closed with Delete.
    removed: HashSet<String>,
    files: Rc<Vec<RecentFileItem>>,
    tools: Rc<Vec<ToolRow>>,
    /// The column the user is in; see [`Self::effective_column`].
    column: SwitcherColumn,
    /// The selected file row, once the user or a Switcher step chose one; the initial row until then.
    file_index: Option<usize>,
    tool_index: usize,
    /// The file rows selected besides the lead, and where ⇧ ranges start.
    selection: RowSelection,
    /// Net ⌃⇥ presses since the Switcher opened.
    step: i64,
    /// The file row Switcher steps count from, fixed when the recent files arrive.
    base: Option<usize>,
    control_held: bool,
    file_scroll: UniformListScrollHandle,
    tool_scroll: UniformListScrollHandle,
    /// The claim on the keys the popup handles itself, ahead of the keymap.
    key_claim: u64,
    /// The question to the backend for the recent files; it goes with the popup.
    _load: Option<Task<()>>,
    _field_events: Option<Subscription>,
}

impl EventEmitter<RecentFilesEvent> for RecentFiles {}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The shortcut of `action` as menus show it.
fn shortcut(cx: &App, action: &str) -> Option<String> {
    cx.try_global::<Keymap>()?.0.shortcut_label(action, is_mac_host())
}

/// The row a navigation key moves to from row `active` of `len`: ↑ and ↓ go round the ends when `wrap`,
/// Page Up and Page Down stop at them. `None` for any other key and for an empty list.
fn list_navigate(key: &str, len: usize, active: usize, wrap: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    let active = active.min(last);
    Some(match key {
        "down" if active < last => active + 1,
        "down" if wrap => 0,
        "up" if active > 0 => active - 1,
        "up" if wrap => last,
        "down" | "up" => active,
        "pagedown" => (active + PAGE_ROWS).min(last),
        "pageup" => active.saturating_sub(PAGE_ROWS),
        _ => return None,
    })
}

/// `text` cut into the pieces to draw, each with whether it is matched. `indices` are the byte offsets
/// of the matched characters.
fn highlighted(text: &str, indices: &[usize]) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut at = 0;
    for run in index_runs(text, indices) {
        if run.start < at {
            continue;
        }
        if run.start > at {
            parts.push((text[at..run.start].to_owned(), false));
        }
        at = run.end;
        parts.push((text[run].to_owned(), true));
    }
    if at < text.len() {
        parts.push((text[at..].to_owned(), false));
    }
    parts
}

/// `text` with its matched characters on the search-match colour.
fn marked_text(cx: &App, text: &str, indices: &[usize]) -> Div {
    let (match_bg, match_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
    div().flex().children(highlighted(text, indices).into_iter().map(|(part, matched)| {
        let part = div().child(SharedString::from(part));
        if matched { part.bg(match_bg).text_color(match_fg).rounded(px(3.0)) } else { part }
    }))
}

/// The inside of a list row: on the selection colour when `selected`, else lit under the pointer.
fn row_content(cx: &App, selected: bool) -> Div {
    let hover_bg = theme::color(cx, "bg-hover");
    div()
        .size_full()
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(px(5.0))
        .whitespace_nowrap()
        .overflow_hidden()
        .text_color(theme::color(cx, "fg-strong"))
        .when(selected, |el| el.bg(theme::color(cx, "bg-tree-selected")))
        .when(!selected, |el| el.hover(|el| el.bg(hover_bg)))
}

/// A list row: `content` inset from the edges of its list.
fn row(cx: &App, content: impl IntoElement) -> Div {
    div().h(theme::length(cx, "h-row")).w_full().px(px(6.0)).child(content)
}

impl RecentFiles {
    /// The popup of `kind` over what `input` describes. Recent files the input does not bring are asked
    /// from the backend; until they arrive the Switcher lists the open editors in tab order.
    pub fn new(kind: RecentFilesKind, input: RecentFilesInput, cx: &mut Context<Self>) -> Self {
        let RecentFilesInput { window_label, root, active_path, tabs, tool_windows, edited, recent } = input;
        let (switcher, step, control_held) = match kind {
            RecentFilesKind::Recent => (false, 0, false),
            RecentFilesKind::Switcher { backward, control_held } => (true, if backward { -1 } else { 0 }, control_held),
        };
        let tabs = if switcher { tabs } else { Vec::new() };
        // An editor that is not a file has no path: when it is the one showing, its key stands in.
        let top_editor = tabs
            .first()
            .filter(|tab| tab.last_active > 0 && !matches!(tab.input.kind.as_str(), "file" | "virtual"))
            .map(|tab| tab.input.key.clone());
        let active_path = top_editor.or(active_path);
        let field = (!switcher).then(|| cx.new(TextField::new));
        let field_events = field.as_ref().map(|field| {
            cx.subscribe(field, |popup: &mut RecentFiles, _, event: &TextFieldEvent, cx| match event {
                TextFieldEvent::Changed => popup.query_changed(cx),
                // Enter is the popup's own key: it is claimed before the field sees it.
                TextFieldEvent::Submitted => {}
                TextFieldEvent::Cancelled => cx.emit(RecentFilesEvent::Dismissed),
            })
        });
        let focus = match &field {
            Some(field) => field.focus_handle(cx),
            None => cx.focus_handle(),
        };
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |popup, cx| popup.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|popup, cx| input::release_keys(popup.key_claim, cx)).detach();
        let mut popup = Self {
            window_label,
            switcher,
            field,
            focus,
            query: String::new(),
            root,
            active_path,
            tabs,
            tool_windows,
            edited,
            edited_only: false,
            paths: recent,
            removed: HashSet::new(),
            files: Rc::default(),
            tools: Rc::default(),
            column: SwitcherColumn::Files,
            file_index: None,
            tool_index: 0,
            selection: SINGLE_FIRST,
            step,
            base: None,
            control_held,
            file_scroll: UniformListScrollHandle::new(),
            tool_scroll: UniformListScrollHandle::new(),
            key_claim,
            _load: None,
            _field_events: field_events,
        };
        if popup.paths.is_none() {
            popup.load(cx);
        }
        popup.refresh(cx);
        popup.apply_step();
        popup
    }

    pub fn is_switcher(&self) -> bool {
        self.switcher
    }

    /// The speed search of Recent Files; the Switcher has none.
    pub fn field(&self) -> Option<&Entity<TextField>> {
        self.field.as_ref()
    }

    /// Whether the recent files are still being asked for.
    pub fn is_loading(&self) -> bool {
        self.paths.is_none()
    }

    pub fn edited_only(&self) -> bool {
        self.edited_only
    }

    /// The heading, in English.
    pub fn title(&self) -> &'static str {
        if self.switcher {
            "Switcher"
        } else if self.edited_only {
            "Recently Edited Files"
        } else {
            "Recent Files"
        }
    }

    /// The file rows as text: a file's path relative to the project, another editor's title.
    pub fn lines(&self) -> Vec<String> {
        self.files.iter().map(|file| file.rel.clone()).collect()
    }

    /// The ids of the listed tool windows, in order.
    pub fn tool_windows(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.window.id.clone()).collect()
    }

    /// The selected file row: its path, or the input key of an editor that is not a file. `None` while
    /// the selection is among the tool windows.
    pub fn selected_file(&self) -> Option<String> {
        self.selected_item().map(|file| file.path.clone())
    }

    /// The id of the selected tool window. `None` while the selection is among the files.
    pub fn selected_tool(&self) -> Option<String> {
        self.selected_tool_row().map(|tool| tool.window.id.clone())
    }

    /// The paths of the selected file rows in list order: the lead alone unless ⇧ or a modified click
    /// selected more.
    pub fn chosen_files(&self) -> Vec<String> {
        self.chosen().into_iter().map(|file| file.path).collect()
    }

    /// What the files column says, in English, when it lists nothing; `None` while the recent files are
    /// still being asked for.
    pub fn empty_text(&self) -> Option<&'static str> {
        self.paths.as_ref()?;
        Some(if self.switcher {
            "No open files"
        } else if self.edited_only {
            "No edited files"
        } else if !self.query.is_empty() {
            "No matching files"
        } else {
            "No recent files"
        })
    }

    /// The line under the lists: where the selected file is, or the project when no file is selected.
    pub fn footer(&self) -> String {
        match self.selected_item() {
            Some(file) if file.editor.is_some() => file.rel.clone(),
            Some(file) => file.path.clone(),
            None if self.selected_tool_row().is_some() => String::new(),
            None => self.root.clone().unwrap_or_default(),
        }
    }

    /// Lists only the files edited in this session, or every recent file again.
    pub fn toggle_edited_only(&mut self, cx: &mut Context<Self>) {
        self.edited_only = !self.edited_only;
        self.selection = SINGLE_FIRST;
        self.file_index = None;
        self.refresh(cx);
    }

    /// ⌃⇥ (`delta` 1) or ⌃⇧⇥ (`delta` -1): the Switcher moves its selection one row, past the end of the
    /// files into the tool windows and round. Recent Files asks for the Switcher in its place.
    pub fn cycle(&mut self, delta: i64, cx: &mut Context<Self>) {
        if !self.switcher {
            cx.emit(RecentFilesEvent::ShowSwitcher { backward: delta < 0 });
            return;
        }
        self.step = self.step.saturating_add(delta);
        self.apply_step();
        cx.notify();
    }

    /// The editor of the row with this path or input key was closed, as the popup asked with
    /// [`RecentFilesEvent::ClosePath`] or [`RecentFilesEvent::CloseInput`]: the row goes, and Recent
    /// Files forgets the file. The selection stays where it was, on the row that moved up.
    pub fn removed(&mut self, key: &str, cx: &mut Context<Self>) {
        let at = self.f_index();
        if !self.switcher {
            self.forget(key, cx);
        }
        self.removed.insert(key.to_owned());
        self.file_index = at;
        self.selection = RowSelection { lead: at, anchor: at, keys: None };
        self.refresh(cx);
    }

    /// Asks the backend for the recent files. Without a backend there are none.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else {
            self.paths = Some(Vec::new());
            return;
        };
        let window = self.window_label.clone();
        let found = backend.with_host(move |host| async move {
            let inner = host.clone();
            host.blocking(move || ide_host::ops::nav::recent_files(&inner, &window)).await
        });
        self._load = Some(cx.spawn(async move |popup, cx| {
            let paths: Vec<String> = match found.await {
                Ok(Ok(files)) => files.into_iter().map(|path| path.0.to_string_lossy().into_owned()).collect(),
                // A window without a project has no recent files; the Switcher still lists its editors.
                Ok(Err(error)) => {
                    tracing::debug!("no recent files: {error}");
                    Vec::new()
                }
                Err(error) => {
                    tracing::warn!("cannot ask for the recent files: {error}");
                    Vec::new()
                }
            };
            let _ = popup.update(cx, |popup, cx| popup.loaded(paths, cx));
        }));
    }

    fn loaded(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        self.paths = Some(paths);
        self.refresh(cx);
        self.apply_step();
    }

    /// Has the backend drop `path` from the project's recent files.
    fn forget(&self, path: &str, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (window, path) = (self.window_label.clone(), PathBuf::from(path));
        let forgotten = backend.with_host(move |host| async move {
            let inner = host.clone();
            host.blocking(move || ide_host::ops::nav::remove_recent(&inner, &window, path)).await
        });
        // The popup may be gone before the backend answers: the task does not live in it.
        cx.spawn(async move |_, _| match forgotten.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("cannot forget the recent file: {error}"),
            Err(error) => tracing::warn!("cannot forget the recent file: {error}"),
        })
        .detach();
    }

    /// Rebuilds the rows from what is listed, typed and closed.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let root = self.root.as_deref();
        let recent: &[String] = self.paths.as_deref().unwrap_or_default();
        let files = if self.switcher {
            let mut items = switcher_items(recent, &self.tabs, &self.query, root);
            items.retain(|item| !self.removed.contains(&item.path));
            items
        } else {
            let kept: Vec<&str> =
                recent.iter().map(String::as_str).filter(|path| !self.removed.contains(*path)).collect();
            let opts = RecentFilesOptions { root, edited: Some(&self.edited), edited_only: self.edited_only };
            filter_recent_files(&kept, &self.query, &opts)
        };
        let tools: Vec<ToolRow> = switcher_tool_windows(&self.tool_windows, root.is_some(), &self.query)
            .into_iter()
            .map(|found| ToolRow { window: found.window.clone(), indices: found.indices })
            .collect();
        self.files = Rc::new(files);
        self.tools = Rc::new(tools);
        self.reveal();
        cx.notify();
    }

    fn query_changed(&mut self, cx: &mut Context<Self>) {
        self.query = self.field.as_ref().map(|field| field.read(cx).text().to_owned()).unwrap_or_default();
        self.file_index = None;
        self.selection = SINGLE_FIRST;
        self.tool_index = 0;
        self.refresh(cx);
    }

    /// The selected file row. Until the user chooses one it is the previous file, so that ⌘E ↵ goes
    /// back and forth between the last two.
    fn f_index(&self) -> Option<usize> {
        let last = self.files.len().checked_sub(1)?;
        let index =
            self.file_index.or_else(|| initial_file_index(&self.files, self.active_path.as_deref(), &self.query))?;
        Some(index.min(last))
    }

    fn t_index(&self) -> Option<usize> {
        Some(self.tool_index.min(self.tools.len().checked_sub(1)?))
    }

    /// The column the selection is in: the tool windows when there are no files to select.
    fn effective_column(&self) -> SwitcherColumn {
        if self.column == SwitcherColumn::Files && self.files.is_empty() && !self.tools.is_empty() {
            SwitcherColumn::Tools
        } else {
            self.column
        }
    }

    fn selected_item(&self) -> Option<&RecentFileItem> {
        (self.effective_column() == SwitcherColumn::Files).then(|| self.f_index()).flatten().map(|at| &self.files[at])
    }

    fn selected_tool_row(&self) -> Option<&ToolRow> {
        (self.effective_column() == SwitcherColumn::Tools).then(|| self.t_index()).flatten().map(|at| &self.tools[at])
    }

    /// The selected file rows in list order.
    fn chosen(&self) -> Vec<RecentFileItem> {
        let keys: Vec<FileKey<'_>> =
            self.files.iter().enumerate().map(|(index, file)| FileKey { index, path: &file.path }).collect();
        let selection = RowSelection { lead: self.f_index(), ..self.selection.clone() };
        selected_rows(&keys, &selection).into_iter().map(|key| self.files[key.index].clone()).collect()
    }

    /// Scrolls the selected row into view.
    fn reveal(&self) {
        match self.effective_column() {
            SwitcherColumn::Files => {
                if let Some(at) = self.f_index() {
                    self.file_scroll.scroll_to_item(at, ScrollStrategy::Nearest);
                }
            }
            SwitcherColumn::Tools => {
                if let Some(at) = self.t_index() {
                    self.tool_scroll.scroll_to_item(at, ScrollStrategy::Nearest);
                }
            }
        }
    }

    /// Puts the Switcher's selection `step` rows from where it started. Only a press does this: rows
    /// that go away (Delete) leave the selection where it is.
    fn apply_step(&mut self) {
        if !self.switcher || self.paths.is_none() {
            return;
        }
        let files = self.files.len();
        let base = match self.base {
            Some(base) => base,
            None => {
                // ⌃⇥ starts on the previous file, ⌃⇧⇥ on the oldest one.
                let base = if self.step < 0 {
                    files
                } else {
                    initial_file_index(&self.files, self.active_path.as_deref(), "").unwrap_or(0)
                };
                self.base = Some(base);
                base
            }
        };
        let Some(selection) = switcher_selection(base, self.step, files, self.tools.len()) else { return };
        self.column = selection.column;
        match selection.column {
            SwitcherColumn::Files => self.file_index = Some(selection.index),
            SwitcherColumn::Tools => self.tool_index = selection.index,
        }
        self.reveal();
    }

    /// Selects file row `index`: alone, with the rows from the anchor to it, or toggled. The Switcher
    /// selects one row at a time.
    fn select_file(&mut self, index: usize, mode: SelectMode, cx: &mut Context<Self>) {
        let files = Rc::clone(&self.files);
        let keys: Vec<FileKey<'_>> =
            files.iter().enumerate().map(|(index, file)| FileKey { index, path: &file.path }).collect();
        let lead = self.f_index();
        let anchor = if self.selection.keys.is_some() { self.selection.anchor } else { lead };
        let current = RowSelection { lead, anchor, keys: self.selection.keys.clone() };
        let mode = if self.switcher { SelectMode::Single } else { mode };
        let next = select_row(&keys, &current, index, mode, None);
        self.file_index = next.lead;
        self.selection = next;
        self.column = SwitcherColumn::Files;
        self.reveal();
        cx.notify();
    }

    fn open_file(&self, file: &RecentFileItem, side: bool, cx: &mut Context<Self>) {
        cx.emit(match &file.editor {
            Some(editor) => RecentFilesEvent::OpenEditor { input: editor.input.clone(), side },
            None => RecentFilesEvent::OpenFile { path: PathBuf::from(&file.path), side },
        });
    }

    /// Opens the selected files: one as it is, several each in a tab of its own with the lead showing.
    fn open_chosen(&self, side: bool, cx: &mut Context<Self>) {
        let chosen = self.chosen();
        match chosen.as_slice() {
            [] => {}
            [file] => self.open_file(file, side, cx),
            [.., last] => {
                let lead = self.selected_item().filter(|lead| chosen.contains(lead)).unwrap_or(last);
                let paths = chosen.iter().map(|file| PathBuf::from(&file.path)).collect();
                cx.emit(RecentFilesEvent::OpenFiles { paths, lead: PathBuf::from(&lead.path), side });
            }
        }
    }

    fn open_tool(&self, index: Option<usize>, cx: &mut Context<Self>) {
        if let Some(tool) = index.and_then(|at| self.tools.get(at)) {
            cx.emit(RecentFilesEvent::ActivateToolWindow(tool.window.id.clone()));
        }
    }

    /// Enter: opens the selected files (in the other pane with ⇧) or the selected tool window. Text
    /// that matches nothing recent is looked for among all the files.
    fn enter(&self, side: bool, cx: &mut Context<Self>) {
        let typed = self.query.trim();
        match self.effective_column() {
            SwitcherColumn::Files if self.f_index().is_none() && !typed.is_empty() => {
                cx.emit(RecentFilesEvent::GoToFile(typed.to_owned()));
            }
            SwitcherColumn::Files => self.open_chosen(side, cx),
            SwitcherColumn::Tools => self.open_tool(self.t_index(), cx),
        }
    }

    /// Ctrl released in the Switcher: opens the selection, or closes the popup when nothing is selected.
    fn commit(&self, cx: &mut Context<Self>) {
        if let Some(file) = self.selected_item() {
            self.open_file(file, false, cx);
        } else if self.selected_tool_row().is_some() {
            self.open_tool(self.t_index(), cx);
        } else {
            cx.emit(RecentFilesEvent::Dismissed);
        }
    }

    /// Delete: asks for the selected editor to be closed, or the selected tool window to be hidden.
    fn remove_selected(&self, cx: &mut Context<Self>) {
        if self.effective_column() == SwitcherColumn::Tools {
            if let Some(tool) = self.selected_tool_row() {
                cx.emit(RecentFilesEvent::HideToolWindow(tool.window.id.clone()));
            }
            return;
        }
        let Some(file) = self.selected_item() else { return };
        let key = file.path.clone();
        cx.emit(match file.editor {
            Some(_) => RecentFilesEvent::CloseInput(key),
            None => RecentFilesEvent::ClosePath(key),
        });
    }

    /// Moves to `column`, or to the other one. A multi-selection of files does not survive the move.
    fn switch_column(&mut self, column: Option<SwitcherColumn>, cx: &mut Context<Self>) {
        self.selection = SINGLE_FIRST;
        self.column = column.unwrap_or(match self.effective_column() {
            SwitcherColumn::Files => SwitcherColumn::Tools,
            SwitcherColumn::Tools => SwitcherColumn::Files,
        });
        self.reveal();
        cx.notify();
    }

    /// ↑, ↓, Page Up and Page Down in the column the selection is in. ⇧ extends the selection of files
    /// and stops at the ends of the list.
    fn navigate(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) {
        match self.effective_column() {
            SwitcherColumn::Files => {
                let Some(active) = self.f_index() else { return };
                let wrap = self.switcher || !shift;
                if let Some(next) = list_navigate(key, self.files.len(), active, wrap) {
                    self.select_file(next, selection_mode(shift, false), cx);
                }
            }
            SwitcherColumn::Tools => {
                let Some(active) = self.t_index() else { return };
                if let Some(next) = list_navigate(key, self.tools.len(), active, true) {
                    self.tool_index = next;
                    self.reveal();
                    cx.notify();
                }
            }
        }
    }

    /// A click on file row `index`. In Recent Files a plain click opens the file, ⇧ selects up to it
    /// and the platform's primary modifier adds or removes it. In the Switcher every click opens, ⇧ in
    /// the other pane.
    fn click_file(&mut self, index: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        let mode =
            if self.switcher { SelectMode::Single } else { selection_mode(modifiers.shift, modifiers.secondary(cx)) };
        self.select_file(index, mode, cx);
        if mode == SelectMode::Single
            && let Some(file) = self.files.get(index)
        {
            self.open_file(file, self.switcher && modifiers.shift, cx);
        }
    }

    fn click_tool(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selection = SINGLE_FIRST;
        self.tool_index = index;
        self.column = SwitcherColumn::Tools;
        self.open_tool(Some(index), cx);
        cx.notify();
    }

    /// The Switcher opens its selection when the Ctrl that was held is released; a modifier going down
    /// or another one going up does nothing.
    fn modifiers_changed(&mut self, event: &ModifiersChangedEvent, _: &mut Window, cx: &mut Context<Self>) {
        let released = self.control_held && !event.modifiers.control;
        self.control_held = event.modifiers.control;
        if self.switcher && released {
            self.commit(cx);
        }
    }

    /// The keys the popup handles itself, whatever modifiers are held (in the Switcher Ctrl always is),
    /// while it has the keyboard: the keymap's bindings for them do not run. Returns whether
    /// `keystroke` is one of them.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        // A key pressed with Ctrl proves Ctrl is down, also when it went down before the popup opened.
        self.control_held |= modifiers.control;
        let key = keystroke.key.as_str();
        match key {
            "enter" => self.enter(modifiers.shift, cx),
            "tab" if modifiers.control => self.cycle(if modifiers.shift { -1 } else { 1 }, cx),
            "delete" => self.remove_selected(cx),
            // Backspace edits the speed search first.
            "backspace" if self.query.is_empty() => self.remove_selected(cx),
            "tab" => self.switch_column(None, cx),
            // The arrows move the caret of the speed search while it has text.
            "left" if self.query.is_empty() => self.switch_column(Some(SwitcherColumn::Tools), cx),
            "right" if self.query.is_empty() => self.switch_column(Some(SwitcherColumn::Files), cx),
            "up" | "down" | "pageup" | "pagedown" => self.navigate(key, modifiers.shift, cx),
            // Recent Files hears of Escape from its speed search.
            "escape" if self.switcher => cx.emit(RecentFilesEvent::Dismissed),
            _ => return false,
        }
        true
    }
}

impl Focusable for RecentFiles {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RecentFiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let (muted, subtle) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let (chrome, line) = (theme::color(cx, "bg-frame"), theme::color(cx, "border-popup"));
        let column = self.effective_column();

        // The speed search shows as a field only once it has text.
        let speed_search = self.field.clone().map(|field| {
            let typed = !self.query.is_empty();
            let icon = if typed {
                theme::icon(cx, "search", 14.0, subtle)
            } else {
                div().flex_none().size(px(14.0)).into_any_element()
            };
            div()
                .flex_1()
                .min_w_0()
                .h(px(24.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(if typed { theme::color(cx, "border") } else { chrome })
                .when(typed, |el| el.bg(theme::color(cx, "bg-input")))
                .child(icon)
                .child(div().flex_1().min_w_0().child(field))
        });
        let edited_only = (!self.switcher).then(|| {
            let label = tx(cx, "Show edited only");
            let hint = shortcut(cx, "navigate.recentFiles");
            let checked = self.edited_only;
            let mark = checked.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
            div()
                .id("recent-edited-only")
                .debug_selector(|| "recent-edited-only".to_owned())
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(strong)
                .child(
                    div()
                        .flex_none()
                        .size(px(16.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.0))
                        .border_1()
                        .border_color(theme::color(cx, if checked { "bg-button-primary" } else { "border-checkbox" }))
                        .bg(theme::color(cx, if checked { "bg-button-primary" } else { "bg-checkbox" }))
                        .children(mark),
                )
                .child(SharedString::from(label))
                .children(hint.map(|hint| div().text_color(subtle).child(SharedString::from(hint))))
                .on_click(cx.listener(|popup, _, _, cx| popup.toggle_edited_only(cx)))
        });
        let title = tx(cx, self.title());
        let header = div()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .flex()
            .items_center()
            .gap(px(12.0))
            .pl(px(16.0))
            .pr(px(12.0))
            .bg(chrome)
            .border_b_1()
            .border_color(line)
            .text_color(fg)
            .child(
                div().flex_none().text_color(strong).font_weight(FontWeight::SEMIBOLD).child(SharedString::from(title)),
            )
            .children(speed_search)
            .children(edited_only);

        let tools = Rc::clone(&self.tools);
        let tool_active = (column == SwitcherColumn::Tools).then(|| self.t_index()).flatten();
        let tool_list = uniform_list(
            "recent-tool-windows",
            tools.len(),
            cx.processor(move |_, range: Range<usize>, _, cx| {
                range
                    .map(|index| {
                        let tool = &tools[index];
                        let hint = shortcut(cx, &format!("toolwindow.{}", tool.window.id));
                        let content = row_content(cx, tool_active == Some(index))
                            .id(index)
                            .debug_selector(|| format!("recent-tool-{index}"))
                            .child(theme::icon(cx, &tool.window.icon, 16.0, muted))
                            .child(
                                marked_text(cx, &tool.window.title, &tool.indices).flex_1().min_w_0().overflow_hidden(),
                            )
                            .children(
                                hint.map(|hint| div().flex_none().text_color(subtle).child(SharedString::from(hint))),
                            )
                            .on_click(cx.listener(move |popup, _, _, cx| popup.click_tool(index, cx)));
                        row(cx, content)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.tool_scroll)
        .size_full();
        let locations = (!self.switcher).then(|| {
            let label = tx(cx, "Recent Locations");
            let hint = shortcut(cx, "navigate.recentLocations");
            div().flex_none().px(px(10.0)).pt(px(2.0)).child(
                div()
                    .id("recent-locations")
                    .debug_selector(|| "recent-locations".to_owned())
                    .h(px(28.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(4.0))
                    .border_t_1()
                    .border_color(line)
                    .text_color(fg)
                    .hover(|el| el.text_color(strong))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(label)))
                    .children(hint.map(|hint| div().flex_none().text_color(subtle).child(SharedString::from(hint))))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(RecentFilesEvent::ShowRecentLocations))),
            )
        });

        let files = Rc::clone(&self.files);
        let file_active = (column == SwitcherColumn::Files).then(|| self.f_index()).flatten();
        let chosen = (column == SwitcherColumn::Files).then(|| self.selection.keys.clone()).flatten();
        let current = self.active_path.clone();
        let empty = self.files.is_empty().then(|| self.empty_text()).flatten().map(|text| tx(cx, text));
        let file_list = match empty {
            Some(text) => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .child(SharedString::from(text))
                .into_any_element(),
            None => uniform_list(
                "recent-files",
                files.len(),
                cx.processor(move |_, range: Range<usize>, _, cx| {
                    range
                        .map(|index| {
                            let file = &files[index];
                            let selected = file_active == Some(index)
                                || chosen.as_ref().is_some_and(|keys| keys.contains(&file.path));
                            let icon = match &file.editor {
                                Some(editor) => editor.icon.as_str(),
                                None => theme::icons::file_icon_name(&file.name),
                            };
                            // The editor showing now is told apart by its slant.
                            let name = marked_text(cx, &file.name, &file.indices)
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .when(current.as_deref() == Some(file.path.as_str()), |el| el.italic());
                            let content = row_content(cx, selected)
                                .id(index)
                                .debug_selector(|| format!("recent-file-{index}"))
                                .child(theme::icon(cx, icon, 16.0, muted))
                                .child(name)
                                .child(div().flex_none().text_color(subtle).child(SharedString::from(file.dir.clone())))
                                .on_click(cx.listener(move |popup, event: &ClickEvent, _, cx| {
                                    popup.click_file(index, event, cx);
                                }));
                            row(cx, content)
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.file_scroll)
            .size_full()
            .into_any_element(),
        };

        let columns = div()
            .flex_none()
            .h(px(COLUMNS_HEIGHT))
            .flex()
            .child(
                div()
                    .flex_none()
                    .w(px(TOOLS_WIDTH))
                    .h_full()
                    .flex()
                    .flex_col()
                    .pt(px(6.0))
                    .pb(px(4.0))
                    .border_r_1()
                    .border_color(line)
                    .child(div().flex_1().min_h_0().child(tool_list))
                    .children(locations),
            )
            .child(div().flex_1().min_w_0().h_full().py(px(6.0)).child(file_list));
        let footer = div()
            .flex_none()
            .h(px(FOOTER_HEIGHT))
            .flex()
            .items_center()
            .px(px(12.0))
            .bg(chrome)
            .border_t_1()
            .border_color(line)
            .text_color(subtle)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(SharedString::from(self.footer()));
        let focus = self.focus.clone();
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .overflow_hidden()
            .text_size(theme::length(cx, "fs-ui"))
            // The Switcher has no field to hold the keyboard: it holds it itself.
            .when(self.switcher, |el| el.track_focus(&focus))
            .on_modifiers_changed(cx.listener(Self::modifiers_changed))
            .child(header)
            .child(columns)
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::storage::UiStorage;
    use crate::workbench::tool_windows::Anchor;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::time::Duration;

    type Events = Rc<RefCell<Vec<RecentFilesEvent>>>;

    const RECENT: [&str; 5] =
        ["/p/src/Main.kt", "/p/src/Order.kt", "/p/README.md", "/p/build.gradle", "/p/src/Main.kt"];

    fn tool(id: &str, title: &str, anchor: Anchor, order: f64) -> ToolWindow {
        ToolWindow { order: Some(order), ..ToolWindow::new(id, title, "folder", anchor) }
    }

    /// A project at `/p` with `/p/src/Main.kt` showing, three tool windows and the recent files given.
    fn project(recent: &[&str]) -> RecentFilesInput {
        RecentFilesInput {
            window_label: "w1".to_owned(),
            root: Some("/p".to_owned()),
            active_path: Some("/p/src/Main.kt".to_owned()),
            tool_windows: vec![
                tool("terminal", "Terminal", Anchor::Bottom, 10.0),
                tool("project", "Project", Anchor::Left, 0.0),
                tool("structure", "Structure", Anchor::Left, 20.0),
            ],
            recent: Some(recent.iter().map(|path| (*path).to_owned()).collect()),
            ..RecentFilesInput::default()
        }
    }

    fn tab(kind: &str, key: &str, title: &str, last_active: u64) -> OpenEditorTab {
        let file = kind == "file";
        OpenEditorTab {
            input: EditorInput {
                kind: kind.to_owned(),
                key: key.to_owned(),
                path: file.then(|| key.to_owned()),
                data: None,
            },
            title: title.to_owned(),
            icon: if file { "file" } else { "toolwindow-agents" }.to_owned(),
            last_active,
        }
    }

    /// Three files and an agent session open, `a.rs` showing and `c.rs` not shown this session.
    fn editors() -> RecentFilesInput {
        RecentFilesInput {
            active_path: Some("/p/src/a.rs".to_owned()),
            tabs: vec![
                tab("file", "/p/src/a.rs", "a.rs", 3),
                tab("agent-session", "agent:1", "Fix the parser", 2),
                tab("file", "/p/src/b.rs", "b.rs", 1),
                tab("file", "/p/src/c.rs", "c.rs", 0),
            ],
            tool_windows: vec![
                tool("terminal", "Terminal", Anchor::Bottom, 10.0),
                tool("project", "Project", Anchor::Left, 0.0),
            ],
            ..project(&["/p/src/c.rs", "/p/src/a.rs"])
        }
    }

    const SWITCHER: RecentFilesKind = RecentFilesKind::Switcher { backward: false, control_held: true };

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The popup in a window of its own, with the keyboard, and what it has asked of the window so far.
    fn open(
        cx: &mut TestAppContext,
        kind: RecentFilesKind,
        input: RecentFilesInput,
    ) -> (Entity<RecentFiles>, Events, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(|_, cx| RecentFiles::new(kind, input, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &RecentFilesEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    fn lines(view: &Entity<RecentFiles>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.lines())
    }

    fn file(view: &Entity<RecentFiles>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.selected_file())
    }

    fn tool_window(view: &Entity<RecentFiles>, cx: &mut VisualTestContext) -> Option<String> {
        view.read_with(cx, |view, _| view.selected_tool())
    }

    fn last(events: &Events) -> Option<RecentFilesEvent> {
        events.borrow().last().cloned()
    }

    fn open_file(path: &str, side: bool) -> Option<RecentFilesEvent> {
        Some(RecentFilesEvent::OpenFile { path: PathBuf::from(path), side })
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
    }

    /// Recent Files lists each recent file once, most recent first, with the tool windows by stripe
    /// beside them, and starts on the previous file when the most recent one is showing.
    #[gpui_kit::test]
    async fn lists_the_recent_files_most_recent_first_and_starts_on_the_previous_one(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        assert_eq!(lines(&view, cx), ["src/Main.kt", "src/Order.kt", "README.md", "build.gradle"]);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Order.kt"));
        view.read_with(cx, |view, _| {
            assert_eq!(view.title(), "Recent Files");
            assert_eq!(view.tool_windows(), ["project", "structure", "terminal"]);
            assert_eq!(view.footer(), "/p/src/Order.kt");
            assert_eq!(view.chosen_files(), ["/p/src/Order.kt"]);
            assert!(!view.is_loading() && !view.is_switcher() && view.field().is_some());
        });
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)), "the speed search has the keyboard");
        assert!(events.borrow().is_empty());

        // Another file is showing: the most recent one is the one to go back to.
        let input = RecentFilesInput { active_path: Some("/p/other.kt".to_owned()), ..project(&RECENT) };
        let (view, _, cx) = open(cx, RecentFilesKind::Recent, input);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Main.kt"));

        // Nothing recent: the column says so, and the footer shows the project.
        let (view, _, cx) = open(cx, RecentFilesKind::Recent, project(&[]));
        view.read_with(cx, |view, _| {
            assert_eq!(view.empty_text(), Some("No recent files"));
            assert_eq!((view.selected_file(), view.selected_tool().as_deref()), (None, Some("project")));
            assert_eq!(view.footer(), "");
        });
    }

    /// Typing narrows the files and the tool windows; the first match is selected. Backspace edits the
    /// text, and Enter on text that matches nothing recent looks for it among all the files.
    #[gpui_kit::test]
    async fn typing_filters_the_files_and_the_tool_windows(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_input("ord");
        assert_eq!(lines(&view, cx), ["src/Order.kt"]);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Order.kt"));
        assert!(view.read_with(cx, |view, _| view.tool_windows().is_empty()));

        cx.simulate_input("x");
        assert!(lines(&view, cx).is_empty());
        view.read_with(cx, |view, _| {
            assert_eq!(view.empty_text(), Some("No matching files"));
            assert_eq!(view.footer(), "/p");
        });
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), Some(RecentFilesEvent::GoToFile("ordx".to_owned())));

        // Backspace takes a character of the text back; it closes nothing while there is text.
        cx.simulate_keystrokes("backspace");
        assert_eq!(lines(&view, cx), ["src/Order.kt"]);
        assert_eq!(events.borrow().len(), 1);

        // Text that only a tool window matches selects that window, and Enter shows it.
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_input("ter");
        assert!(lines(&view, cx).is_empty());
        assert_eq!(view.read_with(cx, |view, _| view.tool_windows()), ["terminal"]);
        assert_eq!(tool_window(&view, cx).as_deref(), Some("terminal"));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), Some(RecentFilesEvent::ActivateToolWindow("terminal".to_owned())));
    }

    /// Enter opens the selected file, ⇧Enter in the other pane; Escape closes the popup.
    #[gpui_kit::test]
    async fn enter_opens_the_selection_and_escape_dismisses(cx: &mut TestAppContext) {
        init(cx);
        let (_view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), open_file("/p/src/Order.kt", false));
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(last(&events), open_file("/p/src/Order.kt", true));
        cx.simulate_keystrokes("escape");
        assert_eq!(last(&events), Some(RecentFilesEvent::Dismissed));
        assert_eq!(events.borrow().len(), 3);
    }

    /// ↑ and ↓ move the selection round the ends, Page Up and Page Down stop at them. ⇧ extends the
    /// selection without going round, and Enter then opens every selected file.
    #[gpui_kit::test]
    async fn arrows_move_and_extend_the_selection(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_keystrokes("down");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));
        cx.simulate_keystrokes("down down");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Main.kt"));
        cx.simulate_keystrokes("shift-up");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Main.kt"), "⇧↑ stops at the first row");
        cx.simulate_keystrokes("up");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/build.gradle"));
        cx.simulate_keystrokes("pageup");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Main.kt"));
        cx.simulate_keystrokes("pageup pagedown pagedown");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/build.gradle"));

        cx.simulate_keystrokes("shift-up");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));
        assert_eq!(view.read_with(cx, |view, _| view.chosen_files()), ["/p/README.md", "/p/build.gradle"]);
        cx.simulate_keystrokes("enter");
        let paths = vec![PathBuf::from("/p/README.md"), PathBuf::from("/p/build.gradle")];
        let lead = PathBuf::from("/p/README.md");
        assert_eq!(last(&events), Some(RecentFilesEvent::OpenFiles { paths, lead, side: false }));

        // A plain arrow selects one row again.
        cx.simulate_keystrokes("up");
        assert_eq!(view.read_with(cx, |view, _| view.chosen_files()), ["/p/src/Order.kt"]);
    }

    /// Tab goes to the other column, ← to the tool windows and → to the files while nothing is typed.
    /// Among the tool windows the arrows move, Enter shows the selected one and Delete hides it.
    #[gpui_kit::test]
    async fn tab_and_the_arrows_move_between_the_columns(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_keystrokes("tab");
        assert_eq!((file(&view, cx), tool_window(&view, cx).as_deref()), (None, Some("project")));
        assert_eq!(view.read_with(cx, |view, _| view.footer()), "");
        cx.simulate_keystrokes("down");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("structure"));
        cx.simulate_keystrokes("up up");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("terminal"));
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), Some(RecentFilesEvent::ActivateToolWindow("terminal".to_owned())));
        cx.simulate_keystrokes("delete");
        assert_eq!(last(&events), Some(RecentFilesEvent::HideToolWindow("terminal".to_owned())));
        assert_eq!(view.read_with(cx, |view, _| view.tool_windows()).len(), 3, "a hidden window stays listed");

        cx.simulate_keystrokes("tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Order.kt"));
        cx.simulate_keystrokes("left");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("terminal"));
        cx.simulate_keystrokes("right");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Order.kt"));

        // With text typed the arrows are the speed search's: the caret moves, the column stays.
        cx.simulate_input("m");
        cx.simulate_keystrokes("left");
        cx.simulate_input("x");
        let typed = view.read_with(cx, |view, cx| view.field().map(|field| field.read(cx).text().to_owned()));
        assert_eq!(typed.as_deref(), Some("xm"));
        assert_eq!(tool_window(&view, cx), None);
    }

    /// Delete (and Backspace while nothing is typed) asks for the selected file to be closed. Once
    /// the window says it is, the row goes and the selection stays in place.
    #[gpui_kit::test]
    async fn delete_closes_the_selected_file_and_drops_its_row(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_keystrokes("delete");
        assert_eq!(last(&events), Some(RecentFilesEvent::ClosePath("/p/src/Order.kt".to_owned())));
        assert_eq!(lines(&view, cx).len(), 4, "the row stays until the editor is closed");

        view.update(cx, |view, cx| view.removed("/p/src/Order.kt", cx));
        assert_eq!(lines(&view, cx), ["src/Main.kt", "README.md", "build.gradle"]);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));

        cx.simulate_keystrokes("backspace");
        assert_eq!(last(&events), Some(RecentFilesEvent::ClosePath("/p/README.md".to_owned())));
        // The last row goes: the selection moves up to the new last one.
        cx.simulate_keystrokes("down");
        view.update(cx, |view, cx| view.removed("/p/build.gradle", cx));
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));
    }

    /// "Show edited only" lists the files edited in this session, from its checkbox or the action.
    #[gpui_kit::test]
    async fn show_edited_only_lists_the_files_edited_in_this_session(cx: &mut TestAppContext) {
        init(cx);
        let edited = HashSet::from(["/p/README.md".to_owned()]);
        let (view, _, cx) = open(cx, RecentFilesKind::Recent, RecentFilesInput { edited, ..project(&RECENT) });
        cx.simulate_keystrokes("down");
        view.update(cx, |view, cx| view.toggle_edited_only(cx));
        assert_eq!(lines(&view, cx), ["README.md"]);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));
        assert_eq!(view.read_with(cx, |view, _| (view.title(), view.edited_only())), ("Recently Edited Files", true));

        // The checkbox switches back; the selection starts over on the previous file.
        cx.run_until_parked();
        click(cx, "recent-edited-only", Modifiers::default());
        assert_eq!(lines(&view, cx).len(), 4);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/Order.kt"));
        assert_eq!(view.read_with(cx, |view, _| view.title()), "Recent Files");

        let (view, _, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        view.update(cx, |view, cx| view.toggle_edited_only(cx));
        assert_eq!(view.read_with(cx, |view, _| (view.lines().len(), view.empty_text())), (0, Some("No edited files")));
    }

    /// A click opens a file or shows a tool window; ⇧ and the primary modifier select files instead.
    #[gpui_kit::test]
    async fn clicks_open_select_and_toggle(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        click(cx, "recent-file-2", Modifiers::default());
        assert_eq!(last(&events), open_file("/p/README.md", false));
        assert_eq!(file(&view, cx).as_deref(), Some("/p/README.md"));

        cx.run_until_parked();
        click(cx, "recent-file-0", Modifiers { shift: true, ..Modifiers::default() });
        assert_eq!(events.borrow().len(), 1, "⇧ click selects without opening");
        assert_eq!(
            view.read_with(cx, |view, _| view.chosen_files()),
            ["/p/src/Main.kt", "/p/src/Order.kt", "/p/README.md"]
        );
        let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        cx.run_until_parked();
        click(cx, "recent-file-3", primary);
        assert_eq!(events.borrow().len(), 1);
        assert_eq!(view.read_with(cx, |view, _| view.chosen_files()).len(), 4);
        cx.run_until_parked();
        click(cx, "recent-file-0", primary);
        assert_eq!(
            view.read_with(cx, |view, _| view.chosen_files()),
            ["/p/src/Order.kt", "/p/README.md", "/p/build.gradle"]
        );

        cx.run_until_parked();
        click(cx, "recent-tool-1", Modifiers::default());
        assert_eq!(last(&events), Some(RecentFilesEvent::ActivateToolWindow("structure".to_owned())));
        assert_eq!(tool_window(&view, cx).as_deref(), Some("structure"));
        cx.run_until_parked();
        click(cx, "recent-locations", Modifiers::default());
        assert_eq!(last(&events), Some(RecentFilesEvent::ShowRecentLocations));
    }

    /// ⌃⇥ in Recent Files asks for the Switcher in its place.
    #[gpui_kit::test]
    async fn ctrl_tab_in_recent_files_asks_for_the_switcher(cx: &mut TestAppContext) {
        init(cx);
        let (_view, events, cx) = open(cx, RecentFilesKind::Recent, project(&RECENT));
        cx.simulate_keystrokes("ctrl-tab");
        assert_eq!(last(&events), Some(RecentFilesEvent::ShowSwitcher { backward: false }));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert_eq!(last(&events), Some(RecentFilesEvent::ShowSwitcher { backward: true }));
    }

    /// The Switcher lists every open editor, the ones shown this session first, and starts on the
    /// previous one. Each ⌃⇥ moves a row on, through the tool windows and round; ⌃⇧⇥ moves back.
    /// Releasing Ctrl opens the selection.
    #[gpui_kit::test]
    async fn the_switcher_moves_on_every_press_and_opens_when_ctrl_is_released(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, SWITCHER, editors());
        assert_eq!(lines(&view, cx), ["src/a.rs", "Fix the parser", "src/b.rs", "src/c.rs"]);
        assert_eq!(file(&view, cx).as_deref(), Some("agent:1"));
        view.read_with(cx, |view, _| {
            assert_eq!(view.title(), "Switcher");
            assert_eq!(view.tool_windows(), ["project", "terminal"]);
            assert_eq!(view.footer(), "Fix the parser");
            assert!(view.is_switcher() && view.field().is_none());
        });
        assert!(cx.update(|window, cx| view.focus_handle(cx).is_focused(window)), "the Switcher has the keyboard");

        cx.simulate_keystrokes("ctrl-tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/b.rs"));
        cx.simulate_keystrokes("ctrl-tab ctrl-tab");
        assert_eq!((file(&view, cx), tool_window(&view, cx).as_deref()), (None, Some("project")));
        cx.simulate_keystrokes("ctrl-tab ctrl-tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/a.rs"));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("terminal"));
        // The action reaches an open Switcher the same way when another key is bound to it.
        view.update(cx, |view, cx| view.cycle(-1, cx));
        assert_eq!(tool_window(&view, cx).as_deref(), Some("project"));
        assert!(events.borrow().is_empty());

        cx.simulate_modifiers_change(Modifiers::default());
        assert_eq!(*events.borrow(), [RecentFilesEvent::ActivateToolWindow("project".to_owned())]);

        // A quick ⌃⇥ goes to the previous editor, whatever kind it is.
        let (_view, events, cx) = open(cx, SWITCHER, editors());
        cx.simulate_modifiers_change(Modifiers::default());
        let input = EditorInput { kind: "agent-session".to_owned(), key: "agent:1".to_owned(), path: None, data: None };
        assert_eq!(*events.borrow(), [RecentFilesEvent::OpenEditor { input, side: false }]);

        // Nothing open and nothing to show: releasing Ctrl closes the Switcher.
        let empty = RecentFilesInput { window_label: "w1".to_owned(), recent: Some(Vec::new()), ..Default::default() };
        let (view, events, cx) = open(cx, SWITCHER, empty);
        assert_eq!(view.read_with(cx, |view, _| view.empty_text()), Some("No open files"));
        cx.simulate_modifiers_change(Modifiers::default());
        assert_eq!(*events.borrow(), [RecentFilesEvent::Dismissed]);
    }

    /// ⌃⇧⇥ opens the Switcher on the editor used longest ago.
    #[gpui_kit::test]
    async fn the_switcher_opened_backward_starts_on_the_oldest_editor(cx: &mut TestAppContext) {
        init(cx);
        let kind = RecentFilesKind::Switcher { backward: true, control_held: true };
        let (view, events, cx) = open(cx, kind, editors());
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/c.rs"));
        cx.simulate_keystrokes("ctrl-shift-tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/b.rs"));
        cx.simulate_keystrokes("ctrl-tab ctrl-tab");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("project"));
        cx.simulate_keystrokes("ctrl-up");
        assert_eq!(tool_window(&view, cx).as_deref(), Some("terminal"), "the arrows move with Ctrl held");
        // → goes back to the files, to the row the presses last left there: the first ⌃⇥ went
        // from `b.rs` to `c.rs`, the second on into the tool windows.
        cx.simulate_keystrokes("ctrl-left ctrl-right");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/c.rs"));
        cx.simulate_keystrokes("ctrl-enter");
        assert_eq!(last(&events), open_file("/p/src/c.rs", false));
        cx.simulate_keystrokes("ctrl-shift-enter");
        assert_eq!(last(&events), open_file("/p/src/c.rs", true));
        cx.simulate_keystrokes("ctrl-escape");
        assert_eq!(last(&events), Some(RecentFilesEvent::Dismissed));
    }

    /// A Switcher opened without Ctrl held stays open while other modifiers come and go; a Ctrl that
    /// goes down and up, or that a key press showed to be down, opens the selection.
    #[gpui_kit::test]
    async fn only_a_ctrl_that_was_held_opens_the_selection_on_release(cx: &mut TestAppContext) {
        init(cx);
        let kind = RecentFilesKind::Switcher { backward: false, control_held: false };
        let (_view, events, cx) = open(cx, kind, editors());
        cx.simulate_modifiers_change(Modifiers { shift: true, ..Modifiers::default() });
        cx.simulate_modifiers_change(Modifiers::default());
        assert!(events.borrow().is_empty());
        cx.simulate_modifiers_change(Modifiers { control: true, ..Modifiers::default() });
        assert!(events.borrow().is_empty());
        cx.simulate_modifiers_change(Modifiers { control: true, shift: true, ..Modifiers::default() });
        cx.simulate_modifiers_change(Modifiers { control: true, ..Modifiers::default() });
        assert!(events.borrow().is_empty(), "⇧ released with Ctrl still down");
        cx.simulate_modifiers_change(Modifiers::default());
        assert_eq!(events.borrow().len(), 1);

        let (view, events, cx) = open(cx, kind, editors());
        cx.simulate_keystrokes("ctrl-tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/b.rs"));
        cx.simulate_modifiers_change(Modifiers::default());
        assert_eq!(*events.borrow(), [open_file("/p/src/b.rs", false).expect("an event")]);
    }

    /// Delete in the Switcher closes the selected editor. Its row goes without moving the selection;
    /// the next press counts on from where the Switcher started.
    #[gpui_kit::test]
    async fn delete_in_the_switcher_closes_the_selected_editor(cx: &mut TestAppContext) {
        init(cx);
        let (view, events, cx) = open(cx, SWITCHER, editors());
        cx.simulate_keystrokes("ctrl-delete");
        assert_eq!(last(&events), Some(RecentFilesEvent::CloseInput("agent:1".to_owned())));
        view.update(cx, |view, cx| view.removed("agent:1", cx));
        assert_eq!(lines(&view, cx), ["src/a.rs", "src/b.rs", "src/c.rs"]);
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/b.rs"));
        cx.simulate_keystrokes("ctrl-backspace");
        assert_eq!(last(&events), Some(RecentFilesEvent::ClosePath("/p/src/b.rs".to_owned())));
        cx.simulate_keystrokes("ctrl-tab");
        assert_eq!(file(&view, cx).as_deref(), Some("/p/src/c.rs"));

        // A click opens the row under it, ⇧ in the other pane.
        cx.run_until_parked();
        click(cx, "recent-file-0", Modifiers { shift: true, control: true, ..Modifiers::default() });
        assert_eq!(last(&events), open_file("/p/src/a.rs", true));
    }

    /// Without injected paths the popup asks the backend, and a file whose row was closed is dropped
    /// from the project's recent files.
    #[gpui_kit::test]
    async fn asks_the_backend_for_the_recent_files_and_forgets_a_closed_one(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        for name in ["a.txt", "b.txt"] {
            std::fs::write(root.join(name), "text\n").expect("a file");
        }
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        fn recent(host: &ide_host::Host) -> Vec<PathBuf> {
            let listed = ide_host::ops::nav::recent_files(host, "w1");
            listed.map(|files| files.into_iter().map(|path| path.0).collect()).unwrap_or_default()
        }

        let input = RecentFilesInput {
            window_label: "w1".to_owned(),
            root: Some(root.to_string_lossy().into_owned()),
            ..RecentFilesInput::default()
        };
        let (a, b) = (root.join("a.txt"), root.join("b.txt"));
        // The index takes a file as recent once it knows the project's folders.
        for _ in 0..300 {
            ide_host::ops::nav::mark_recent(&host, "w1", a.clone()).expect("a.txt is marked");
            ide_host::ops::nav::mark_recent(&host, "w1", b.clone()).expect("b.txt is marked");
            if recent(&host).len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(recent(&host), [b.clone(), a.clone()]);
        let (view, events, cx) = open(cx, RecentFilesKind::Recent, input);
        settle(cx, |cx| view.read_with(cx, |view, _| !view.is_loading()));
        assert_eq!(lines(&view, cx), ["b.txt", "a.txt"]);
        assert_eq!(file(&view, cx), Some(b.to_string_lossy().into_owned()));

        cx.simulate_keystrokes("delete");
        let closed = b.to_string_lossy().into_owned();
        assert_eq!(last(&events), Some(RecentFilesEvent::ClosePath(closed.clone())));
        view.update(cx, |view, cx| view.removed(&closed, cx));
        assert_eq!(lines(&view, cx), ["a.txt"]);
        settle(cx, |_| recent(&host) == [a.clone()]);
        host.close_project("w1");
    }

    // Not from a spec: every design token the popup draws with is defined, as the kind it is used as.
    #[test]
    fn the_design_tokens_the_popup_uses_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-button-primary",
            "fg-search-match",
            "bg-frame",
            "bg-popup",
            "bg-input",
            "bg-hover",
            "bg-tree-selected",
            "bg-search-match",
            "bg-checkbox",
            "bg-button-primary",
            "border",
            "border-popup",
            "border-checkbox",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["h-row", "fs-ui", "radius-small"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
    }

    // Not from a spec: the arrows go round the ends only when asked to, the page keys never do.
    #[test]
    fn navigation_keys_move_within_the_list() {
        assert_eq!(list_navigate("down", 3, 0, false), Some(1));
        assert_eq!(list_navigate("down", 3, 2, false), Some(2));
        assert_eq!(list_navigate("down", 3, 2, true), Some(0));
        assert_eq!(list_navigate("up", 3, 0, false), Some(0));
        assert_eq!(list_navigate("up", 3, 0, true), Some(2));
        assert_eq!(list_navigate("up", 3, 2, true), Some(1));
        assert_eq!(list_navigate("pagedown", 25, 3, true), Some(13));
        assert_eq!(list_navigate("pagedown", 25, 20, true), Some(24));
        assert_eq!(list_navigate("pageup", 25, 13, true), Some(3));
        assert_eq!(list_navigate("pageup", 25, 4, true), Some(0));
        assert_eq!(list_navigate("home", 3, 1, true), None);
        assert_eq!(list_navigate("down", 0, 0, true), None);
    }

    // Not from a spec: the matched characters come out as pieces of their own, in order.
    #[test]
    fn matched_characters_are_cut_into_their_own_pieces() {
        let piece = |text: &str, matched: bool| (text.to_owned(), matched);
        assert_eq!(highlighted("Order.kt", &[0, 1, 2]), [piece("Ord", true), piece("er.kt", false)]);
        assert_eq!(highlighted("Main.kt", &[]), [piece("Main.kt", false)]);
        assert_eq!(highlighted("añb", &[1, 3]), [piece("a", false), piece("ñb", true)]);
        assert_eq!(highlighted("ab", &[1]), [piece("a", false), piece("b", true)]);
    }
}
