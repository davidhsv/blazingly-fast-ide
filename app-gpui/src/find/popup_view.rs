//! The Find in Files popup, and its Replace in Files mode: a query with its options and scope, the matching
//! lines of the project listed as they are found, and the selected match shown in its file below them.
//!
//! [`FindSession`] runs the search, the popup's options and what they resolve to live in [`super::store`], and
//! [`super::replace_flow`] decides what a Replace covers. This view carries their effects out against the
//! backend, draws the result and tells its window what the user chose through [`FindPopupEvent`].

use super::backend::{file_matches, summary, to_backend};
use super::ipc::{ExtensionCount, Pos, ReplaceEvent, ReplaceSummary, ReplaceTarget, SearchContext, SearchQuery};
use super::replace_flow::{
    GroupState, PreviewKind, PreviewLine, ReplaceMarks, ReplaceProgress, ReplaceRequest, ReplaceRun, ReplaceStep,
    Usage, confirm_text, count_usages, group_state, next_pending, pending_usages, preview_positions,
    replace_preview_lines, replace_targets, summary_text,
};
use super::row_selection::{
    RowSelection, SINGLE_FIRST, SelectMode, is_row_selected, select_row, selected_rows, selection_mode,
};
use super::session::{FindEffect, FindSession, FindStatus, MatchRow, match_rows};
use super::store::{
    BuiltinScope, FindArg, FindOptions, FindPopupCommand, FindPopupState, HistoryKind, MASKS_KEY, NEW_TAB_KEY,
    NamedScope, PINNED_KEY, SEARCH_CONTEXTS, ScopeInputs, ScopeTab, StorageWrite, build_query, custom_scope_name,
    custom_scope_value, describe_query, find_history, parse_masks, project_masks, recent_masks, remember_find_string,
    remember_mask, resolve_find_scope,
};
use super::utf16;
use super::window_store::OpenOptions;
use crate::backend::Backend;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::platform::keymap::{format_binding, host_binding, is_mac_host};
use crate::platform::project_roots::ProjectModule;
use crate::platform::search::{CustomScope, parse_scopes, scopes_key};
use crate::platform::storage::UiStorage;
use crate::platform::virtual_docs::{basename, dirname};
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Global, Hsla,
    IntoElement, Keystroke, MouseButton, PathPromptOptions, PromptLevel, Render, ScrollStrategy, SharedString,
    Subscription, Task, UniformListScrollHandle, Window, deferred, div, prelude::*, px, uniform_list,
};
use ide_core::{IdeError, IpcPath};
use ide_host::ops::search::SearchEvent;
use regex::Regex;
use serde_json::json;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

/// How long the options rest before they are searched with.
pub const SEARCH_DEBOUNCE: Duration = Duration::from_millis(150);
/// How long the selection rests before its file is read for the preview.
pub const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(80);
/// The popup lists this many matches at most; the Find window lists them all.
const POPUP_MAX_MATCHES: u32 = 2000;
/// How many rows Page Up and Page Down move by.
const PAGE_ROWS: usize = 9;
/// Lines of a replacement preview kept on either side of the change it shows.
const DIFF_WINDOW: usize = 300;
const WIDTH: f32 = 820.0;
const HEADER_HEIGHT: f32 = 42.0;
const FIELD_HEIGHT: f32 = 30.0;
const ERROR_HEIGHT: f32 = 18.0;
const SCOPE_ROW_HEIGHT: f32 = 36.0;
const ROW_HEIGHT: f32 = 24.0;
const RESULTS_HEIGHT: f32 = ROW_HEIGHT * 9.0;
const PREVIEW_HEADER_HEIGHT: f32 = 36.0;
const PREVIEW_HEIGHT: f32 = 220.0;
const FOOTER_HEIGHT: f32 = 46.0;
const MENU_WIDTH: f32 = 300.0;
const GUTTER_WIDTH: f32 = 48.0;

/// The scope tabs: which one, its label and the letter that selects it with Alt.
const SCOPE_TABS: [(ScopeTab, &str, &str); 4] = [
    (ScopeTab::Project, "In Project", "p"),
    (ScopeTab::Module, "Module", "m"),
    (ScopeTab::Directory, "Directory", "d"),
    (ScopeTab::Scope, "Scope", "s"),
];

/// The named scopes every project has, with their labels.
const NAMED_SCOPES: [(BuiltinScope, &str); 5] = [
    (BuiltinScope::ProjectFiles, "Project Files"),
    (BuiltinScope::ProductionFiles, "Project Production Files"),
    (BuiltinScope::TestFiles, "Project Test Files"),
    (BuiltinScope::OpenFiles, "Open Files"),
    (BuiltinScope::RecentFiles, "Recently Viewed Files"),
];

/// What the popup is opened with.
#[derive(Debug, Clone, Default)]
pub struct FindPopupInput {
    /// The label of the window, which names its project to the backend.
    pub window_label: String,
    /// The project's folder; in a workspace, the folder of its first project.
    pub root: Option<String>,
    /// The folders of a workspace's projects. Empty for a plain project.
    pub workspace_folders: Vec<String>,
    /// Replace in Files rather than Find in Files.
    pub replace: bool,
    /// What the action was invoked with: a directory to search in and a query to start with.
    pub arg: FindArg,
    /// The editor's selection, searched for when the argument names no query.
    pub selection: Option<String>,
    /// The file of the active editor: the Directory tab starts in its folder.
    pub active_path: Option<String>,
    /// The files of the open editors: the "Open Files" scope.
    pub open_paths: Vec<String>,
    /// The project's modules, for the Module tab.
    pub modules: Vec<ProjectModule>,
}

/// What the popup asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindPopupEvent {
    /// Escape, or a Replace All that is done: the popup closes.
    Dismissed,
    /// Open `path` with the match selected: on the 0-based `line`, from the UTF-16 column `col` to `end_col`.
    /// `side` asks for the other editor pane. With `keep_open` the popup is pinned: it stays, with the keyboard.
    Open { path: PathBuf, line: u32, col: u32, end_col: u32, side: bool, keep_open: bool },
    /// Close the popup and run `query` in a tab of the Find window titled `title`.
    OpenInFindWindow { query: SearchQuery, title: String, options: OpenOptions },
    /// Open the scope editor, on the custom scope `select` when one is chosen.
    EditScopes { select: Option<String> },
    /// Tell the user how a Replace went.
    Notify(NotificationRequest),
}

/// A search option shown as a toggle in the query fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindToggle {
    MatchCase,
    Words,
    Regex,
    /// Replace: keep the case of each occurrence.
    PreserveCase,
}

/// A list the popup opens over itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindMenu {
    /// The recent search or replacement strings.
    History(HistoryKind),
    /// The file masks used recently, then those of the project's file types.
    Masks,
    /// Where in the code a match may lie.
    Context,
    Module,
    NamedScope,
}

/// What choosing a menu item sets.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MenuChoice {
    Pattern(String),
    Replacement(String),
    Mask(String),
    Context(SearchContext),
    Module(String),
    Scope(NamedScope),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MenuItem {
    label: String,
    detail: Option<String>,
    checked: bool,
    /// `None` for a heading and for a note that the list is empty.
    choice: Option<MenuChoice>,
}

struct Menu {
    kind: FindMenu,
    items: Rc<Vec<MenuItem>>,
    /// Index into `items` of the item the keyboard is on.
    active: Option<usize>,
}

/// One of the popup's text fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Pattern,
    Replacement,
    Mask,
    Directory,
}

/// What follows a Replace that went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterReplace {
    /// Select the next row with something left to replace after row `0`.
    SelectNext(usize),
    /// Close the popup.
    Close,
}

/// Which text of a file the preview shows: the file as it was after `version` replacements.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewKey {
    path: String,
    version: u64,
}

struct FilePreview {
    key: PreviewKey,
    lines: Rc<Vec<String>>,
}

/// What a replacement preview shows: the usages of one row replaced as the options say.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffKey {
    row: String,
    path: String,
    line: u32,
    query: SearchQuery,
    replacement: String,
    preserve_case: bool,
    /// The usages of the row still to replace.
    only: Vec<Pos>,
    version: u64,
}

struct DiffPreview {
    key: DiffKey,
    /// The lines around the change.
    lines: Rc<Vec<PreviewLine>>,
    /// Index into `lines` of the first changed line of the row.
    focus: Option<usize>,
}

/// Why nothing is listed, as the lines shown in place of the results, and what the user can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct EmptyState {
    lines: Vec<String>,
    /// A link below the lines: its label, the shortcut shown beside it and what it does.
    link: Option<(String, String, EmptyLink)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmptyLink {
    SearchWithRegex,
    ClearOptions,
}

/// The options each window's popup was left with: it opens again as the user left it.
#[derive(Default)]
struct PopupMemory(HashMap<String, (Option<String>, FindPopupState)>);

impl Global for PopupMemory {}

pub struct FindPopup {
    window_label: String,
    root: Option<String>,
    workspace_folders: Vec<String>,
    active_path: Option<String>,
    open_paths: Vec<String>,
    modules: Vec<ProjectModule>,
    custom_scopes: Vec<CustomScope>,
    state: FindPopupState,
    pattern: Entity<TextField>,
    replacement: Entity<TextField>,
    mask: Entity<TextField>,
    directory: Entity<TextField>,
    session: FindSession,
    /// The query the options ask for. The session's own query lags behind it while the options rest.
    query: SearchQuery,
    /// Counts the changes of the query and of the results: a Replace confirmed for an earlier count is void.
    version: u64,
    /// The result rows, shared with the list being drawn.
    rows: Rc<Vec<MatchRow>>,
    selection: RowSelection,
    marks: ReplaceMarks,
    /// The recently viewed files, once the "Recently Viewed Files" scope asked for them.
    recent_paths: Option<Vec<String>>,
    /// The file masks used recently, newest first.
    recent_masks: Vec<String>,
    /// The masks of the project's most common file types.
    project_masks: Vec<String>,
    menu: Option<Menu>,
    /// Replace: the preview shows the replacement as a diff.
    preview_mode: bool,
    preview: Option<FilePreview>,
    /// The text the preview is waiting for, or shows.
    preview_wanted: Option<PreviewKey>,
    /// Why the wanted file cannot be shown.
    preview_error: Option<String>,
    /// The file and line the preview was last scrolled to.
    preview_shown: Option<(String, u32)>,
    /// Counts the replacements made: the files they changed are read again.
    preview_version: u64,
    diff: Option<DiffPreview>,
    diff_wanted: Option<DiffKey>,
    running: Option<ReplaceRun>,
    progress: Option<ReplaceProgress>,
    list_scroll: UniformListScrollHandle,
    preview_scroll: UniformListScrollHandle,
    diff_scroll: UniformListScrollHandle,
    /// The claims on the keys the popup handles itself, ahead of the keymap: one per text field.
    key_claims: Vec<u64>,
    /// The options resting before they are searched with.
    _debounce: Option<Task<()>>,
    /// The search in flight; a newer one replaces it.
    _search_task: Option<Task<()>>,
    _preview_task: Option<Task<()>>,
    _replace_task: Option<Task<()>>,
    _confirm_task: Option<Task<()>>,
    recent_load: Option<Task<()>>,
    _masks_load: Option<Task<()>>,
    _browse: Option<Task<()>>,
    _field_events: Vec<Subscription>,
}

impl EventEmitter<FindPopupEvent> for FindPopup {}

/// `text` in the active locale.
fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The value stored under `key`.
fn stored(cx: &App, key: &str) -> Option<String> {
    cx.try_global::<UiStorage>().and_then(|storage| storage.get(key)).map(str::to_owned)
}

fn store(write: StorageWrite, cx: &mut App) {
    if cx.has_global::<UiStorage>() {
        cx.global_mut::<UiStorage>().set(write.key, write.value);
    }
}

/// Puts `text` at the front of the recent search or replacement strings.
fn remember_history(kind: HistoryKind, text: &str, cx: &mut App) {
    let mut history = find_history(&stored(cx, kind.storage_key()).unwrap_or_default());
    if remember_find_string(&mut history, text) {
        store(StorageWrite { key: kind.storage_key(), value: json!(history).to_string() }, cx);
    }
}

/// `binding`, written for macOS, as this OS shows it.
fn key_label(binding: &str) -> String {
    let mac = is_mac_host();
    format_binding(&host_binding(binding, mac), mac)
}

/// The row a navigation key moves to from row `active` of `len`: the arrows and the page keys stop at the
/// ends of the list. `None` for any other key and for an empty list.
fn list_navigate(key: &str, len: usize, active: usize) -> Option<usize> {
    let last = len.checked_sub(1)?;
    let active = active.min(last);
    Some(match key {
        "down" => (active + 1).min(last),
        "up" => active.saturating_sub(1),
        "pagedown" => (active + PAGE_ROWS).min(last),
        "pageup" => active.saturating_sub(PAGE_ROWS),
        _ => return None,
    })
}

/// `text` cut into the pieces to draw, each with whether it is matched. `ranges` are UTF-16 columns in
/// ascending order; one that starts inside the piece before it is left out.
fn segments(text: &str, ranges: &[(u32, u32)]) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut at = 0;
    for &(from, to) in ranges {
        if from < at || to <= from {
            continue;
        }
        if from > at {
            parts.push((utf16::slice(text, at, from).to_owned(), false));
        }
        let matched = utf16::slice(text, from, to);
        if !matched.is_empty() {
            parts.push((matched.to_owned(), true));
        }
        at = to;
    }
    let rest = utf16::slice_from(text, at);
    if !rest.is_empty() {
        parts.push((rest.to_owned(), false));
    }
    parts
}

/// The pieces of a line as elements, the matched ones on `mark`. A tab is drawn as four spaces.
fn marked(pieces: Vec<(String, bool)>, mark: Hsla, mark_fg: Hsla) -> Vec<Div> {
    pieces
        .into_iter()
        .map(|(piece, matched)| {
            let piece = div().flex_none().child(SharedString::from(piece.replace('\t', "    ")));
            if matched { piece.bg(mark).text_color(mark_fg) } else { piece }
        })
        .collect()
}

/// `text` on one line, as the text fields hold it: every run of line breaks becomes a space.
fn single_line(text: &str) -> String {
    text.split(['\r', '\n']).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Puts the options' texts on one line each: a selection or a remembered string may span several, and the
/// fields hold one.
fn on_one_line(options: &mut FindOptions) {
    for text in [&mut options.pattern, &mut options.replacement, &mut options.mask, &mut options.directory] {
        if text.contains(['\r', '\n']) {
            *text = single_line(text);
        }
    }
}

/// The directory as its field shows it: relative to the project when it lies in it, empty for the project's
/// own folder.
fn display_directory(directory: &str, root: Option<&str>) -> String {
    let below = root.filter(|root| !root.is_empty()).and_then(|root| directory.strip_prefix(root));
    match below {
        Some("") => String::new(),
        Some(rest) => rest.strip_prefix('/').unwrap_or(directory).to_owned(),
        None => directory.to_owned(),
    }
}

/// Whether text with regular expression syntax in it is also a valid pattern: the empty results then offer
/// to search with Regex.
fn could_be_regex(text: &str) -> bool {
    text.contains(['.', '$', '|', '(', ')', '[', ']', '{', '}', '^', '?', '*', '+', '\\']) && Regex::new(text).is_ok()
}

/// The first family of the code font.
fn code_font(cx: &App) -> SharedString {
    let family = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families("font-mono").into_iter().next());
    SharedString::from(family.unwrap_or_else(|| "Menlo".to_owned()))
}

/// A square button showing `icon`, lit while `pressed`.
fn icon_button(cx: &App, icon: &str, pressed: bool) -> Div {
    let color = theme::color(cx, if pressed { "fg-strong" } else { "fg-muted" });
    div()
        .flex_none()
        .size(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(pressed, |el| el.bg(theme::color(cx, "bg-toolbar-pressed")))
        .hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover")))
        .child(theme::icon(cx, icon, 16.0, color))
}

/// A button with a text label. The primary one is filled; one that cannot be used is drawn faint.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let (bg, fg, border) = if primary && enabled {
        ("bg-button-primary", "fg-button-primary", "bg-button-primary")
    } else {
        ("bg-button", if enabled { "fg" } else { "fg-faint" }, "border-button")
    };
    div()
        .flex_none()
        .h(theme::length(cx, "h-button"))
        .px(px(12.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, border))
        .bg(theme::color(cx, bg))
        .text_color(theme::color(cx, fg))
        .whitespace_nowrap()
        .child(SharedString::from(label))
}

/// A check box with its label.
fn checkbox(cx: &App, label: String, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(SharedString::from(label))
}

/// The frame of a query field.
fn field_frame(cx: &App, invalid: bool) -> Div {
    div()
        .w_full()
        .h(px(FIELD_HEIGHT))
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if invalid { "danger" } else { "border-strong" }))
}

/// A choice shown as its current value with a chevron; a click opens the list of the others.
fn select_box(cx: &App, label: String) -> Div {
    div()
        .flex_none()
        .h(theme::length(cx, "h-control"))
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border-strong"))
        .bg(theme::color(cx, "bg-field"))
        .text_color(theme::color(cx, "fg"))
        .whitespace_nowrap()
        .child(SharedString::from(label))
        .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-muted")))
}

/// The name a module is listed by: its own, else its folder; the project itself has neither.
fn module_label(module: &ProjectModule, cx: &mut App) -> String {
    if !module.name.is_empty() {
        module.name.clone()
    } else if !module.path.is_empty() {
        module.path.clone()
    } else {
        tx(cx, "Project")
    }
}

fn replace_targets_to_backend(targets: Vec<ReplaceTarget>) -> Vec<ide_search::ReplaceTarget> {
    targets
        .into_iter()
        .map(|ReplaceTarget { path, only }| ide_search::ReplaceTarget { path: IpcPath(PathBuf::from(path)), only })
        .collect()
}

fn replace_summary(summary: ide_search::ReplaceSummary) -> ReplaceSummary {
    let ide_search::ReplaceSummary { files, replacements, failed, cancelled, elapsed_ms } = summary;
    ReplaceSummary { files, replacements, failed, cancelled, elapsed_ms }
}

/// A backend replace event as the models hold it.
fn replace_event(event: ide_search::ReplaceEvent) -> ReplaceEvent {
    let text = |path: IpcPath| path.0.to_string_lossy().into_owned();
    match event {
        ide_search::ReplaceEvent::File { path, replacements, done, total } => {
            ReplaceEvent::File { path: text(path), replacements, done, total }
        }
        ide_search::ReplaceEvent::Failed { path, message, done, total } => {
            ReplaceEvent::Failed { path: text(path), message, done, total }
        }
        ide_search::ReplaceEvent::Done { summary } => ReplaceEvent::Done { summary: replace_summary(summary) },
    }
}

impl FindPopup {
    /// The popup over what `input` describes. It opens with the options the window's popup was left with,
    /// and with the query to start with selected, so that typing replaces it.
    pub fn new(input: FindPopupInput, cx: &mut Context<Self>) -> Self {
        let FindPopupInput {
            window_label,
            root,
            workspace_folders,
            replace,
            arg,
            selection,
            active_path,
            open_paths,
            modules,
        } = input;
        let remembered = cx.try_global::<PopupMemory>().and_then(|memory| memory.0.get(&window_label)).cloned();
        let mut state = match remembered {
            Some((kept_root, mut state)) => {
                // A directory of another project is not searched in this one.
                if kept_root != root && state.options.scope_tab == ScopeTab::Directory {
                    state.options.scope_tab = ScopeTab::Project;
                    state.options.directory.clear();
                }
                state
            }
            None => FindPopupState::restored(stored(cx, PINNED_KEY).as_deref(), stored(cx, NEW_TAB_KEY).as_deref()),
        };
        state.show(replace, &arg, selection.as_deref());
        on_one_line(&mut state.options);
        let recent_masks = stored(cx, MASKS_KEY).map(|masks| parse_masks(&masks)).unwrap_or_default();

        let (search, replace_with) = (tx(cx, "Search"), tx(cx, "Replace"));
        let directory_hint = tx(cx, "Directory (relative to project root)");
        let pattern = cx.new(|cx| TextField::new(cx).with_placeholder(search));
        let replacement = cx.new(|cx| TextField::new(cx).with_placeholder(replace_with));
        let mask = cx.new(TextField::new);
        let directory = cx.new(|cx| TextField::new(cx).with_placeholder(directory_hint));
        let fields = [
            (Field::Pattern, &pattern),
            (Field::Replacement, &replacement),
            (Field::Mask, &mask),
            (Field::Directory, &directory),
        ];
        let field_events = fields
            .iter()
            .map(|&(which, field)| {
                cx.subscribe(field, move |popup: &mut FindPopup, _, event: &TextFieldEvent, cx| {
                    popup.field_event(which, event, cx);
                })
            })
            .collect();
        let key_claims = fields
            .iter()
            .map(|&(which, field)| {
                let claimant = cx.weak_entity();
                input::claim_keys(
                    field.focus_handle(cx),
                    move |keystroke, window, cx| {
                        claimant.update(cx, |popup, cx| popup.claim_key(which, keystroke, window, cx)).unwrap_or(false)
                    },
                    cx,
                )
            })
            .collect();
        cx.on_release(|popup, cx| {
            for claim in &popup.key_claims {
                input::release_keys(*claim, cx);
            }
            popup.remember_strings(cx);
            popup.remember_state(cx);
            // A search still running has nobody left to report to.
            let Some(backend) = cx.try_global::<Backend>() else { return };
            for effect in popup.session.dispose() {
                if let FindEffect::Cancel(request) = effect {
                    backend.host.cancel_request(request);
                }
            }
        })
        .detach();
        let mut popup = Self {
            window_label,
            root,
            workspace_folders,
            active_path,
            open_paths,
            modules,
            custom_scopes: Vec::new(),
            state,
            pattern,
            replacement,
            mask,
            directory,
            session: FindSession::new(),
            query: SearchQuery::default(),
            version: 0,
            rows: Rc::default(),
            selection: SINGLE_FIRST,
            marks: ReplaceMarks::default(),
            recent_paths: None,
            recent_masks,
            project_masks: Vec::new(),
            menu: None,
            preview_mode: false,
            preview: None,
            preview_wanted: None,
            preview_error: None,
            preview_shown: None,
            preview_version: 0,
            diff: None,
            diff_wanted: None,
            running: None,
            progress: None,
            list_scroll: UniformListScrollHandle::new(),
            preview_scroll: UniformListScrollHandle::new(),
            diff_scroll: UniformListScrollHandle::new(),
            key_claims,
            _debounce: None,
            _search_task: None,
            _preview_task: None,
            _replace_task: None,
            _confirm_task: None,
            recent_load: None,
            _masks_load: None,
            _browse: None,
            _field_events: field_events,
        };
        popup.custom_scopes = popup.stored_scopes(cx);
        popup.adopt_module();
        popup.fill_fields(cx);
        popup.load_project_masks(cx);
        popup.load_recent_paths(cx);
        popup.remember_state(cx);
        popup.sync_query(true, cx);
        popup
    }

    /// Shows the open popup again for Find or Replace, as the action invoked on it asks: with the query to
    /// start with selected and the keyboard in it.
    pub fn show(
        &mut self,
        replace: bool,
        arg: &FindArg,
        selection: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.show(replace, arg, selection);
        on_one_line(&mut self.state.options);
        self.menu = None;
        self.fill_fields(cx);
        window.focus(&self.pattern.focus_handle(cx), cx);
        self.options_changed(cx);
    }

    /// The field of the text to find, for typing into.
    pub fn pattern_field(&self) -> &Entity<TextField> {
        &self.pattern
    }

    pub fn replacement_field(&self) -> &Entity<TextField> {
        &self.replacement
    }

    pub fn mask_field(&self) -> &Entity<TextField> {
        &self.mask
    }

    pub fn directory_field(&self) -> &Entity<TextField> {
        &self.directory
    }

    /// The popup's options and how it shows.
    pub fn state(&self) -> &FindPopupState {
        &self.state
    }

    pub fn options(&self) -> &FindOptions {
        &self.state.options
    }

    /// Whether the popup replaces.
    pub fn is_replace(&self) -> bool {
        self.state.replace
    }

    /// Whether the popup stays open when a result is opened or the user clicks beside it.
    pub fn pinned(&self) -> bool {
        self.state.pinned
    }

    /// The query the options ask for.
    pub fn query(&self) -> &SearchQuery {
        &self.query
    }

    pub fn status(&self) -> FindStatus {
        self.session.state().status
    }

    pub fn error(&self) -> Option<&str> {
        self.session.state().error.as_deref()
    }

    /// How many matches the search has found so far.
    pub fn match_count(&self) -> u32 {
        self.session.state().match_count
    }

    /// Whether the rows listed are those of an earlier query: the options changed and the search for them
    /// has not found anything yet.
    pub fn is_stale(&self) -> bool {
        let state = self.session.state();
        state.stale
            || match &state.query {
                Some(query) => *query != self.query,
                None => !self.state.options.pattern.is_empty(),
            }
    }

    /// The count beside the title: empty until something matched.
    pub fn status_text(&self, cx: &mut App) -> String {
        if self.is_stale() {
            return if self.state.options.pattern.is_empty() { String::new() } else { tx(cx, "Updating results…") };
        }
        let state = self.session.state();
        if state.match_count == 0 {
            return String::new();
        }
        let incomplete = state.summary.as_ref().is_some_and(|summary| summary.limit_hit);
        let key = if incomplete { "search.find.matchesIncomplete" } else { "search.find.matches" };
        cx.default_global::<Localization>()
            .0
            .t(key, &[("matches", (state.match_count as usize).into()), ("files", state.files.len().into())])
    }

    fn line_of(row: &MatchRow) -> String {
        format!("{}:{} {}", row.rel_path, row.line + 1, row.text)
    }

    /// The result rows as text: `src/a.rs:12 the matching line`.
    pub fn lines(&self) -> Vec<String> {
        self.rows.iter().map(Self::line_of).collect()
    }

    /// The row the keyboard is on, as [`Self::lines`] shows it.
    pub fn selected(&self) -> Option<String> {
        self.selected_row().map(Self::line_of)
    }

    /// Every selected row, as [`Self::lines`] shows them.
    pub fn chosen_lines(&self) -> Vec<String> {
        selected_rows(self.rows.as_slice(), &self.selection).into_iter().map(Self::line_of).collect()
    }

    /// The rows whose usages were all replaced, as [`Self::lines`] shows them.
    pub fn replaced_lines(&self) -> Vec<String> {
        let done = |row: &&MatchRow| group_state(*row, &self.marks) == GroupState::Done;
        self.rows.iter().filter(done).map(Self::line_of).collect()
    }

    /// The lines shown in place of the results while there are none; empty while there are results.
    pub fn empty_text(&self, cx: &mut App) -> Vec<String> {
        if self.rows.is_empty() { self.empty_state(cx).lines } else { Vec::new() }
    }

    /// The file the preview shows, once it is read.
    pub fn preview_path(&self) -> Option<&str> {
        self.shown_preview().map(|preview| preview.key.path.as_str())
    }

    /// The 0-based line the preview is on and its text.
    pub fn preview_line(&self) -> Option<(u32, String)> {
        let (preview, row) = (self.shown_preview()?, self.selected_row()?);
        Some((row.line, preview.lines.get(row.line as usize)?.clone()))
    }

    /// Every line of the previewed file.
    pub fn preview_lines(&self) -> Vec<String> {
        self.shown_preview().map(|preview| preview.lines.to_vec()).unwrap_or_default()
    }

    /// The matches the preview marks on the 0-based `line`, as UTF-16 column ranges.
    pub fn preview_marks(&self, line: u32) -> Vec<(u32, u32)> {
        self.file_highlights().get(&line).cloned().unwrap_or_default()
    }

    /// The replacement preview's changed lines: `-3 old text`, `+3 new text` (1-based lines).
    pub fn diff_lines(&self) -> Vec<String> {
        let Some(diff) = self.shown_diff() else { return Vec::new() };
        diff.lines
            .iter()
            .filter_map(|line| {
                let sign = match line.kind {
                    PreviewKind::Same => return None,
                    PreviewKind::Del => '-',
                    PreviewKind::Add => '+',
                };
                Some(format!("{sign}{} {}", line.line + 1, line.text))
            })
            .collect()
    }

    /// Whether the preview shows the replacement as a diff.
    pub fn preview_mode(&self) -> bool {
        self.preview_mode
    }

    /// The list open over the popup.
    pub fn menu(&self) -> Option<FindMenu> {
        self.menu.as_ref().map(|menu| menu.kind)
    }

    /// The items of the open list, as text.
    pub fn menu_lines(&self) -> Vec<String> {
        self.menu.as_ref().map(|menu| menu.items.iter().map(|item| item.label.clone()).collect()).unwrap_or_default()
    }

    /// Whether a Replace is running.
    pub fn is_replacing(&self) -> bool {
        self.running.is_some()
    }

    /// The label of the Replace button: it counts the occurrences when it replaces more than one.
    pub fn replace_label(&self, cx: &mut App) -> String {
        let pending =
            if self.state.replace && !self.is_stale() { pending_usages(&self.chosen(), &self.marks).len() } else { 0 };
        if pending > 1 {
            cx.default_global::<Localization>().0.tx_built(&["Replace ", " Occurrences"], &[Param::from(pending)])
        } else {
            tx(cx, "Replace")
        }
    }

    fn field(&self, field: Field) -> &Entity<TextField> {
        match field {
            Field::Pattern => &self.pattern,
            Field::Replacement => &self.replacement,
            Field::Mask => &self.mask,
            Field::Directory => &self.directory,
        }
    }

    /// The custom scopes of the project, as stored.
    fn stored_scopes(&self, cx: &App) -> Vec<CustomScope> {
        let root = self.root.as_deref().unwrap_or_default();
        stored(cx, &scopes_key(root)).map(|scopes| parse_scopes(&scopes)).unwrap_or_default()
    }

    /// The Module tab searches a module of the project: the first one unless the user chose another.
    fn adopt_module(&mut self) {
        let options = &mut self.state.options;
        if let Some(first) = self.modules.first()
            && !self.modules.iter().any(|module| module.path == options.module)
        {
            options.module = first.path.clone();
        }
    }

    /// Puts the options' texts into their fields, the text to find selected.
    fn fill_fields(&mut self, cx: &mut Context<Self>) {
        let options = &self.state.options;
        let directory = display_directory(&options.directory, self.root.as_deref());
        self.pattern.update(cx, |field, cx| {
            field.set_text(&options.pattern, cx);
            field.select_all(cx);
        });
        self.replacement.update(cx, |field, cx| field.set_text(&options.replacement, cx));
        self.mask.update(cx, |field, cx| field.set_text(&options.mask, cx));
        self.directory.update(cx, |field, cx| field.set_text(&directory, cx));
    }

    /// Keeps the options for the next time the window's popup opens.
    fn remember_state(&self, cx: &mut App) {
        let state = FindPopupState { open: false, ..self.state.clone() };
        cx.default_global::<PopupMemory>().0.insert(self.window_label.clone(), (self.root.clone(), state));
    }

    /// Puts the text searched for, the replacement and the file mask in use at the front of their recent
    /// lists.
    fn remember_strings(&mut self, cx: &mut App) {
        remember_history(HistoryKind::Find, &self.state.options.pattern, cx);
        if self.state.replace {
            remember_history(HistoryKind::Replace, &self.state.options.replacement, cx);
        }
        self.remember_mask(cx);
    }

    fn remember_mask(&mut self, cx: &mut App) {
        if self.state.options.mask_enabled
            && let Some(write) = remember_mask(&mut self.recent_masks, &self.state.options.mask)
        {
            store(write, cx);
        }
    }

    /// Asks the backend which file types the project has most of: their masks are offered after the recent
    /// ones.
    fn load_project_masks(&mut self, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let window = self.window_label.clone();
        let found = backend.with_host(move |host| async move {
            let inner = host.clone();
            host.blocking(move || {
                let counts = ide_host::ops::nav::index_file_extensions(&inner, &window)?;
                let extensions: Vec<String> =
                    inner.languages.all().into_iter().flat_map(|language| language.extensions).collect();
                Ok((counts, extensions))
            })
            .await
        });
        self._masks_load = Some(cx.spawn(async move |popup, cx| {
            let (counts, extensions) = match found.await {
                Ok(Ok(found)) => found,
                // A window without a project has no file types to offer.
                Ok(Err(error)) => {
                    tracing::debug!("no file masks of the project: {error}");
                    return;
                }
                Err(error) => {
                    tracing::warn!("cannot ask for the project's file types: {error}");
                    return;
                }
            };
            let counts: Vec<ExtensionCount> = counts
                .into_iter()
                .map(|count| ExtensionCount { extension: count.extension, files: count.files })
                .collect();
            let masks = project_masks(&counts, extensions.iter().map(String::as_str));
            let _ = popup.update(cx, |popup, cx| {
                popup.project_masks = masks;
                cx.notify();
            });
        }));
    }

    /// Asks the backend for the recently viewed files once the scope of that name is chosen. Without a
    /// backend there are none.
    fn load_recent_paths(&mut self, cx: &mut Context<Self>) {
        let options = &self.state.options;
        let wanted = options.scope_tab == ScopeTab::Scope
            && options.named_scope == NamedScope::Builtin(BuiltinScope::RecentFiles);
        if !wanted || self.recent_paths.is_some() || self.recent_load.is_some() {
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else {
            self.recent_paths = Some(Vec::new());
            return;
        };
        let window = self.window_label.clone();
        let found = backend.with_host(move |host| async move {
            let inner = host.clone();
            host.blocking(move || ide_host::ops::nav::recent_files(&inner, &window)).await
        });
        self.recent_load = Some(cx.spawn(async move |popup, cx| {
            let paths: Vec<String> = match found.await {
                Ok(Ok(files)) => files.into_iter().map(|path| path.0.to_string_lossy().into_owned()).collect(),
                Ok(Err(error)) => {
                    tracing::debug!("no recent files: {error}");
                    Vec::new()
                }
                Err(error) => {
                    tracing::warn!("cannot ask for the recent files: {error}");
                    Vec::new()
                }
            };
            let _ = popup.update(cx, |popup, cx| {
                popup.recent_paths = Some(paths);
                popup.options_changed(cx);
            });
        }));
    }

    /// The query the options ask for now.
    fn current_query(&self) -> SearchQuery {
        let inputs = ScopeInputs {
            root: self.root.as_deref(),
            open_paths: &self.open_paths,
            recent_paths: self.recent_paths.as_deref(),
            custom_scopes: &self.custom_scopes,
        };
        let options = &self.state.options;
        build_query(options, resolve_find_scope(options, &inputs), POPUP_MAX_MATCHES)
    }

    /// An option changed: it is kept for the next popup, and searched with once the options have rested.
    fn options_changed(&mut self, cx: &mut Context<Self>) {
        self.remember_state(cx);
        self.load_recent_paths(cx);
        self.sync_query(false, cx);
        self.sync_preview(cx);
        cx.notify();
    }

    /// Takes the query the options ask for. A new one, or the same one with `rerun`, marks the rows as
    /// outdated at once and is searched for after [`SEARCH_DEBOUNCE`].
    fn sync_query(&mut self, rerun: bool, cx: &mut Context<Self>) {
        let query = self.current_query();
        if !rerun && query == self.query {
            return;
        }
        self.query = query;
        self.version += 1;
        let effects = self.session.invalidate();
        self.carry_out(effects, cx);
        self._debounce = Some(cx.spawn(async move |popup, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let _ = popup.update(cx, |popup, cx| popup.search(cx));
        }));
        cx.notify();
    }

    /// Searches for the query, from the first row and with nothing replaced.
    fn search(&mut self, cx: &mut Context<Self>) {
        let effects = self.session.search(self.query.clone());
        self.carry_out(effects, cx);
        self.selection = SINGLE_FIRST;
        self.marks = ReplaceMarks::default();
        self.results_changed(cx);
    }

    fn carry_out(&mut self, effects: Vec<FindEffect>, cx: &mut Context<Self>) {
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let host = backend.host.clone();
        let mut pending = effects;
        while !pending.is_empty() {
            for effect in std::mem::take(&mut pending) {
                match effect {
                    FindEffect::Cancel(request) => host.cancel_request(request),
                    FindEffect::Validate { run, query } => {
                        let verdict =
                            ide_host::ops::search::search_validate(&host, &self.window_label, to_backend(&query));
                        pending.extend(self.session.validated(run, verdict, Backend::next_request_id));
                    }
                    FindEffect::Start { run, request, query } => self.start(run, request, to_backend(&query), cx),
                }
            }
        }
    }

    fn start(
        &mut self,
        run: u64,
        request: ide_core::RequestId,
        query: ide_search::SearchQuery,
        cx: &mut Context<Self>,
    ) {
        let (sender, mut events) = unbounded::<SearchEvent>();
        let on_event: ide_host::ops::Emit<SearchEvent> = Arc::new(move |event| {
            // The receiver is gone only when the popup is, or a newer search took its place.
            let _ = sender.unbounded_send(event);
        });
        let window = self.window_label.clone();
        let started = cx.global::<Backend>().with_host(move |host| async move {
            ide_host::ops::search::start(&host, &window, request, query, on_event).await
        });
        self._search_task = Some(cx.spawn(async move |popup, cx| {
            let failed = async {
                match started.await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(error) => Some(IdeError::internal(error.to_string())),
                }
            };
            let mut failed = Box::pin(futures::FutureExt::fuse(failed));
            loop {
                let outcome = futures::select! {
                    event = events.next() => match event {
                        // What has arrived together is shown together.
                        Some(first) => {
                            let mut batch = vec![first];
                            while let Ok(more) = events.try_recv() {
                                batch.push(more);
                            }
                            Ok(batch)
                        }
                        None => break,
                    },
                    error = failed => match error {
                        Some(error) => Err(error),
                        None => continue,
                    },
                };
                let ended = popup
                    .update(cx, |popup, cx| {
                        let ended = match outcome {
                            Ok(batch) => {
                                let mut done = false;
                                for event in batch {
                                    match event {
                                        SearchEvent::File { file } => popup.session.file_found(run, file_matches(file)),
                                        SearchEvent::Done { summary: ended } => {
                                            popup.session.finished(run, Ok(Some(summary(ended))));
                                            done = true;
                                        }
                                    }
                                }
                                done
                            }
                            Err(error) => {
                                popup.session.finished(run, Err(error));
                                true
                            }
                        };
                        popup.results_changed(cx);
                        ended
                    })
                    .unwrap_or(true);
                if ended {
                    break;
                }
            }
        }));
    }

    /// Brings the rows up to what the session holds: the rows of the search before stay until the new one
    /// has found a file or ended.
    fn results_changed(&mut self, cx: &mut Context<Self>) {
        self.rows = Rc::new(match_rows(&self.session.state().files));
        self.version += 1;
        self.sync_preview(cx);
        cx.notify();
    }

    /// Index into the rows of the one the keyboard is on.
    fn active(&self) -> Option<usize> {
        let last = self.rows.len().checked_sub(1)?;
        Some(self.selection.lead.unwrap_or(0).min(last))
    }

    fn selected_row(&self) -> Option<&MatchRow> {
        self.rows.get(self.active()?)
    }

    /// The selected rows, in list order.
    fn chosen(&self) -> Vec<MatchRow> {
        selected_rows(self.rows.as_slice(), &self.selection).into_iter().cloned().collect()
    }

    fn move_to(&mut self, index: usize, mode: SelectMode, cx: &mut Context<Self>) {
        self.selection = select_row(self.rows.as_slice(), &self.selection, index, mode, None);
        self.list_scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        self.sync_preview(cx);
        cx.notify();
    }

    /// A click on row `index` selects it: alone, up to it with ⇧, or added and removed with the platform's
    /// primary modifier. A double click opens it, with ⇧ in the other pane.
    fn click_row(&mut self, index: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        let modifiers = event.modifiers();
        if event.click_count() >= 2 {
            self.open_row(index, modifiers.shift, cx);
        } else {
            self.move_to(index, selection_mode(modifiers.shift, modifiers.platform || modifiers.control), cx);
        }
    }

    /// Asks the window to open the match of row `index`. A pinned popup stays open, so the text searched
    /// for is remembered now.
    fn open_row(&mut self, index: usize, side: bool, cx: &mut Context<Self>) {
        let Some(row) = self.rows.get(index) else { return };
        let keep_open = self.state.pinned;
        let event = FindPopupEvent::Open {
            path: PathBuf::from(&row.path),
            line: row.line,
            col: row.col,
            end_col: row.end_col,
            side,
            keep_open,
        };
        if keep_open {
            remember_history(HistoryKind::Find, &self.state.options.pattern, cx);
        }
        cx.emit(event);
    }

    /// Opens the match the keyboard is on.
    pub fn open_selected(&mut self, side: bool, cx: &mut Context<Self>) {
        if let Some(active) = self.active() {
            self.open_row(active, side, cx);
        }
    }

    /// Open in Find Window: asks the window to run the search, without the popup's limit, in the Find
    /// window. Nothing to find, nothing to open.
    pub fn open_in_find_window(&mut self, cx: &mut Context<Self>) {
        let options = &self.state.options;
        if options.pattern.is_empty() {
            return;
        }
        let query = SearchQuery { max_matches: 0, ..self.query.clone() };
        let title = describe_query(&query, self.root.as_deref(), &self.custom_scopes);
        let open = OpenOptions {
            replacement: self.state.replace.then(|| options.replacement.clone()),
            preserve_case: options.preserve_case,
            new_tab: self.state.new_tab,
        };
        self.remember_mask(cx);
        cx.emit(FindPopupEvent::OpenInFindWindow { query, title, options: open });
    }

    /// A key that is a shortcut elsewhere and belongs to the popup while it is open: the next or previous
    /// result (round the ends), the selected result's source, the recent strings of the field in use.
    pub fn run_command(&mut self, command: FindPopupCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            FindPopupCommand::NextResult | FindPopupCommand::PreviousResult => {
                let (Some(active), len) = (self.active(), self.rows.len()) else { return };
                let next = if command == FindPopupCommand::NextResult { active + 1 } else { active + len - 1 } % len;
                self.move_to(next, SelectMode::Single, cx);
            }
            FindPopupCommand::OpenResult => self.open_selected(false, cx),
            FindPopupCommand::RecentSearches => {
                let replacing = self.state.replace && self.replacement.focus_handle(cx).is_focused(window);
                let kind = if replacing { HistoryKind::Replace } else { HistoryKind::Find };
                self.open_menu(FindMenu::History(kind), cx);
            }
        }
    }

    /// Replaces the text to find, as typing it would.
    pub fn set_pattern(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = single_line(text);
        self.pattern.update(cx, |field, cx| field.set_text(&text, cx));
        self.state.options.pattern = text;
        self.options_changed(cx);
    }

    /// Replaces the replacement text, as typing it would.
    pub fn set_replacement(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = single_line(text);
        self.replacement.update(cx, |field, cx| field.set_text(&text, cx));
        self.state.options.replacement = text;
        self.options_changed(cx);
    }

    /// Replaces the file mask, as typing it would: a mask that is typed is in use.
    pub fn set_mask(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = single_line(text);
        self.mask.update(cx, |field, cx| field.set_text(&text, cx));
        self.state.options.mask = text;
        self.state.options.mask_enabled = true;
        self.options_changed(cx);
    }

    /// Replaces the directory searched in: an absolute path, or one relative to the project.
    pub fn set_directory(&mut self, text: &str, cx: &mut Context<Self>) {
        let text = single_line(text);
        let shown = display_directory(&text, self.root.as_deref());
        self.directory.update(cx, |field, cx| field.set_text(&shown, cx));
        self.state.options.directory = text;
        self.options_changed(cx);
    }

    /// Turns a search option on or off.
    pub fn toggle(&mut self, toggle: FindToggle, cx: &mut Context<Self>) {
        let options = &mut self.state.options;
        let flag = match toggle {
            FindToggle::MatchCase => &mut options.case_sensitive,
            FindToggle::Words => &mut options.whole_word,
            FindToggle::Regex => &mut options.is_regex,
            FindToggle::PreserveCase => &mut options.preserve_case,
        };
        *flag = !*flag;
        self.options_changed(cx);
    }

    /// Puts the file mask in or out of use. Turned on with nothing typed, it takes the first mask on offer.
    pub fn set_mask_enabled(&mut self, on: bool, cx: &mut Context<Self>) {
        if on && self.state.options.mask.trim().is_empty() {
            let first = recent_masks(&self.recent_masks, &self.project_masks).into_iter().next().unwrap_or_default();
            self.mask.update(cx, |field, cx| field.set_text(&first, cx));
            self.state.options.mask = first;
        }
        self.state.options.mask_enabled = on;
        self.options_changed(cx);
    }

    /// Selects a scope tab. The Directory tab starts in the folder of the file being edited, or in the
    /// project's. The keyboard leaves the directory field when its tab does.
    pub fn select_scope(&mut self, tab: ScopeTab, window: &mut Window, cx: &mut Context<Self>) {
        if tab == ScopeTab::Directory && self.state.options.directory.trim().is_empty() {
            let root = self.root.as_deref();
            let folders: Vec<&str> = if self.workspace_folders.is_empty() {
                root.into_iter().collect()
            } else {
                self.workspace_folders.iter().map(String::as_str).collect()
            };
            let inside = |path: &&str| {
                folders.iter().any(|folder| path.strip_prefix(*folder).is_some_and(|rest| rest.starts_with('/')))
            };
            let start = self.active_path.as_deref().filter(inside).map(dirname).or(root).unwrap_or_default();
            let (start, shown) = (start.to_owned(), display_directory(start, root));
            self.directory.update(cx, |field, cx| field.set_text(&shown, cx));
            self.state.options.directory = start;
        }
        self.state.options.scope_tab = tab;
        if tab != ScopeTab::Directory && self.directory.focus_handle(cx).is_focused(window) {
            window.focus(&self.pattern.focus_handle(cx), cx);
        }
        self.options_changed(cx);
    }

    /// Pins the popup, or lets it close again when a result is opened.
    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        store(self.state.set_pinned(pinned), cx);
        self.remember_state(cx);
        cx.notify();
    }

    /// "Open results in new tab": whether Open in Find Window keeps the Find window's active tab.
    pub fn set_new_tab(&mut self, new_tab: bool, cx: &mut Context<Self>) {
        store(self.state.set_new_tab(new_tab), cx);
        self.remember_state(cx);
        cx.notify();
    }

    /// Replace: shows the replacement as a diff in the preview, or the file again.
    pub fn set_preview_mode(&mut self, on: bool, cx: &mut Context<Self>) {
        self.preview_mode = on;
        self.sync_preview(cx);
        cx.notify();
    }

    /// "Clear all options": back to a plain search everywhere in the scope.
    pub fn clear_options(&mut self, cx: &mut Context<Self>) {
        let options = &mut self.state.options;
        options.case_sensitive = false;
        options.whole_word = false;
        options.is_regex = false;
        options.mask_enabled = false;
        options.context = SearchContext::Any;
        self.options_changed(cx);
    }

    /// The custom scopes were edited: they are read again, and a search in one of them is made again.
    pub fn scopes_changed(&mut self, cx: &mut Context<Self>) {
        self.custom_scopes = self.stored_scopes(cx);
        self.options_changed(cx);
    }

    /// Asks for a directory to search in, starting from the Directory tab.
    fn browse(&mut self, cx: &mut Context<Self>) {
        let title = tx(cx, "Select Search Directory");
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(title.into()),
        });
        self._browse = Some(cx.spawn(async move |popup, cx| {
            let Ok(Ok(Some(paths))) = picked.await else { return };
            let Some(directory) = paths.into_iter().next() else { return };
            let _ = popup.update(cx, |popup, cx| popup.set_directory(&directory.to_string_lossy(), cx));
        }));
    }

    fn menu_items(&self, kind: FindMenu, cx: &mut App) -> Vec<MenuItem> {
        let options = &self.state.options;
        let item = |label: String, checked: bool, choice: MenuChoice| MenuItem {
            label,
            detail: None,
            checked,
            choice: Some(choice),
        };
        let note = |label: String| MenuItem { label, detail: None, checked: false, choice: None };
        match kind {
            FindMenu::History(kind) => {
                let find = kind == HistoryKind::Find;
                let history = find_history(&stored(cx, kind.storage_key()).unwrap_or_default());
                if history.is_empty() {
                    return vec![note(tx(cx, if find { "No recent searches" } else { "No recent replacements" }))];
                }
                let heading = note(tx(cx, if find { "Recent Searches" } else { "Recent Replacements" }));
                let entries = history.into_iter().map(|text| {
                    let choice =
                        if find { MenuChoice::Pattern(text.clone()) } else { MenuChoice::Replacement(text.clone()) };
                    item(single_line(&text), false, choice)
                });
                std::iter::once(heading).chain(entries).collect()
            }
            FindMenu::Masks => recent_masks(&self.recent_masks, &self.project_masks)
                .into_iter()
                .map(|mask| item(mask.clone(), mask == options.mask, MenuChoice::Mask(mask)))
                .collect(),
            FindMenu::Context => SEARCH_CONTEXTS
                .into_iter()
                .map(|(context, label)| item(tx(cx, label), context == options.context, MenuChoice::Context(context)))
                .collect(),
            FindMenu::Module => self
                .modules
                .iter()
                .map(|module| MenuItem {
                    label: module_label(module, cx),
                    detail: (!module.path.is_empty()).then(|| module.path.clone()),
                    checked: module.path == options.module,
                    choice: Some(MenuChoice::Module(module.path.clone())),
                })
                .collect(),
            FindMenu::NamedScope => {
                let shown = self.shown_scope();
                let mut items: Vec<MenuItem> = NAMED_SCOPES
                    .into_iter()
                    .map(|(scope, label)| {
                        let scope = NamedScope::Builtin(scope);
                        item(tx(cx, label), scope == shown, MenuChoice::Scope(scope))
                    })
                    .collect();
                if !self.custom_scopes.is_empty() {
                    items.push(note(tx(cx, "Custom Scopes")));
                    items.extend(self.custom_scopes.iter().map(|custom| {
                        let scope = custom_scope_value(&custom.name);
                        MenuItem {
                            label: custom.name.clone(),
                            detail: Some(custom.pattern.clone()),
                            checked: scope == shown,
                            choice: Some(MenuChoice::Scope(scope)),
                        }
                    }));
                }
                items
            }
        }
    }

    /// The named scope shown as chosen: a custom scope that is gone shows as "Project Files".
    fn shown_scope(&self) -> NamedScope {
        let chosen = &self.state.options.named_scope;
        let gone =
            custom_scope_name(chosen).is_some_and(|name| !self.custom_scopes.iter().any(|custom| custom.name == name));
        if gone { NamedScope::default() } else { chosen.clone() }
    }

    /// Opens a list over the popup; one with nothing in it is not opened.
    pub fn open_menu(&mut self, kind: FindMenu, cx: &mut Context<Self>) {
        let items = self.menu_items(kind, cx);
        let choosable = |item: &MenuItem| item.choice.is_some();
        let active =
            items.iter().position(|item| item.checked && choosable(item)).or_else(|| items.iter().position(choosable));
        self.menu = (!items.is_empty()).then(|| Menu { kind, items: Rc::new(items), active });
        cx.notify();
    }

    fn toggle_menu(&mut self, kind: FindMenu, cx: &mut Context<Self>) {
        if self.menu() == Some(kind) {
            self.close_menu(cx);
        } else {
            self.open_menu(kind, cx);
        }
    }

    pub fn close_menu(&mut self, cx: &mut Context<Self>) {
        self.menu = None;
        cx.notify();
    }

    /// Moves along the items of the open list that can be chosen, round its ends.
    fn step_menu(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(menu) = &mut self.menu else { return };
        let choosable: Vec<usize> = (0..menu.items.len()).filter(|&index| menu.items[index].choice.is_some()).collect();
        let Some(last) = choosable.len().checked_sub(1) else { return };
        let at = menu.active.and_then(|active| choosable.iter().position(|&index| index == active));
        let next = match at {
            None => 0,
            Some(at) if forward => (at + 1) % (last + 1),
            Some(at) => (at + last) % (last + 1),
        };
        menu.active = Some(choosable[next]);
        cx.notify();
    }

    /// Chooses item `index` of the open list and closes it. A heading is not chosen.
    pub fn choose_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let choice = self.menu.as_ref().and_then(|menu| menu.items.get(index)).and_then(|item| item.choice.clone());
        let Some(choice) = choice else { return };
        self.menu = None;
        match choice {
            MenuChoice::Pattern(text) => self.set_pattern(&text, cx),
            MenuChoice::Replacement(text) => self.set_replacement(&text, cx),
            MenuChoice::Mask(mask) => self.set_mask(&mask, cx),
            MenuChoice::Context(context) => {
                self.state.options.context = context;
                self.options_changed(cx);
            }
            MenuChoice::Module(module) => {
                self.state.options.module = module;
                self.options_changed(cx);
            }
            MenuChoice::Scope(scope) => {
                self.state.options.named_scope = scope;
                self.options_changed(cx);
            }
        }
    }

    /// Replace: replaces every selected row's usages that are left, then selects the next row that has
    /// some. Does nothing while the rows are outdated or a Replace is running.
    pub fn replace_selected(&mut self, cx: &mut Context<Self>) {
        let chosen = self.chosen();
        let Some(last) = chosen.last().and_then(|last| self.rows.iter().position(|row| row.key == last.key)) else {
            return;
        };
        self.replace_usages(pending_usages(&chosen, &self.marks), AfterReplace::SelectNext(last), cx);
    }

    /// Replace All: replaces every usage listed that is left, and closes the popup. Usages in more than one
    /// file are replaced only once the user has confirmed, and only if the results are still the ones asked
    /// about.
    pub fn replace_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.state.replace || self.rows.is_empty() || self.running.is_some() || self.is_stale() {
            return;
        }
        let usages = pending_usages(self.rows.as_slice(), &self.marks);
        if usages.is_empty() {
            return;
        }
        let counts = count_usages(&usages);
        if self.rows.len() <= 1 || counts.files <= 1 {
            self.replace_usages(usages, AfterReplace::Close, cx);
            return;
        }
        let options = &self.state.options;
        let mut message = confirm_text(counts, &options.pattern, &options.replacement);
        if !self.marks.excluded.is_empty() {
            message.push(' ');
            message.push_str(&tx(cx, "Excluded usages are skipped."));
        }
        if self.session.state().summary.as_ref().is_some_and(|summary| summary.limit_hit) {
            message.push('\n');
            message.push_str(&tx(cx, "The search hit its limit; only usages found so far are replaced."));
        }
        let (title, replace) = (tx(cx, "Replace All"), tx(cx, "Replace"));
        let cancel = cx.default_global::<Localization>().0.t("common.cancel", &[]);
        let answers = [replace.as_str(), cancel.as_str()];
        let answer = window.prompt(PromptLevel::Info, &title, Some(&message), &answers, cx);
        let asked = self.version;
        self._confirm_task = Some(cx.spawn(async move |popup, cx| {
            if answer.await.ok() != Some(0) {
                return;
            }
            let _ = popup.update(cx, |popup, cx| {
                if popup.version == asked {
                    popup.replace_usages(usages, AfterReplace::Close, cx);
                }
            });
        }));
    }

    /// Has the backend replace `usages`, one undoable step per file, then does what `then` says.
    fn replace_usages(&mut self, usages: Vec<Usage>, then: AfterReplace, cx: &mut Context<Self>) {
        if self.is_stale() || usages.is_empty() || self.running.is_some() {
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let options = &self.state.options;
        let request = ReplaceRequest {
            query: self.query.clone(),
            replacement: options.replacement.clone(),
            preserve_case: options.preserve_case,
            targets: replace_targets(&usages),
        };
        let run = ReplaceRun::new(&request, Backend::next_request_id());
        // An editor's edits are in the backend's document as they are made: there is nothing to wait for.
        let id = match run.synced() {
            ReplaceStep::Start(id) => id,
            ReplaceStep::Finished(_) => return,
        };
        let (sender, mut events) = unbounded::<ide_search::ReplaceEvent>();
        let on_event = ide_host::ops::Channel::new(move |event| {
            // The receiver is gone only when the popup is.
            let _ = sender.unbounded_send(event);
        });
        let window = self.window_label.clone();
        let ReplaceRequest { query, replacement, preserve_case, targets } = request;
        let (query, targets) = (to_backend(&query), replace_targets_to_backend(targets));
        let started = backend.with_host(move |host| async move {
            ide_host::ops::search::replace_in_files(
                &host,
                &window,
                id,
                query,
                replacement,
                preserve_case,
                targets,
                on_event,
            )
            .await
        });
        self.running = Some(run);
        self.progress = None;
        self._replace_task = Some(cx.spawn(async move |popup, cx| {
            // The events end when the backend is done with them.
            while let Some(event) = events.next().await {
                if popup.update(cx, |popup, cx| popup.replace_progressed(replace_event(event), cx)).is_err() {
                    return;
                }
            }
            let outcome = match started.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error.message),
                Err(error) => Err(error.to_string()),
            };
            let _ = popup.update(cx, |popup, cx| popup.replace_finished(outcome, &usages, then, cx));
        }));
        cx.notify();
    }

    fn replace_progressed(&mut self, event: ReplaceEvent, cx: &mut Context<Self>) {
        let Some(run) = &mut self.running else { return };
        if let Some(progress) = run.event(event) {
            self.progress = Some(progress.clone());
            cx.notify();
        }
    }

    /// The backend is done replacing `usages`: the user is told how it went and the rows are marked. A
    /// replacement that was stopped, or that changes the number of lines, leaves the positions found
    /// wrong: the search is made again instead.
    fn replace_finished(
        &mut self,
        outcome: Result<(), String>,
        usages: &[Usage],
        then: AfterReplace,
        cx: &mut Context<Self>,
    ) {
        let Some(run) = self.running.take() else { return };
        self.progress = None;
        cx.notify();
        let search = |request: NotificationRequest| NotificationRequest { group: Some("search".to_owned()), ..request };
        if let Err(message) = outcome {
            let failed = NotificationRequest::new(NotificationLevel::Error, "Replace failed");
            cx.emit(FindPopupEvent::Notify(search(NotificationRequest { body: Some(message), ..failed })));
            return;
        }
        let summary = run.finish();
        let level = if summary.failed > 0 { NotificationLevel::Warning } else { NotificationLevel::Info };
        let file = usages.first().filter(|_| summary.files == 1).map(|usage| basename(&usage.path).into_owned());
        cx.emit(FindPopupEvent::Notify(search(NotificationRequest {
            body: file,
            key: Some("search.replace".to_owned()),
            ..NotificationRequest::new(level, summary_text(&summary))
        })));
        self.preview_version += 1;
        let replacement = &self.state.options.replacement;
        if summary.cancelled || replacement.contains('\n') || replacement.contains("\\n") {
            self.sync_query(true, cx);
            self.sync_preview(cx);
            return;
        }
        self.marks.mark_done(usages);
        match then {
            AfterReplace::SelectNext(last) => {
                if let Some(next) = next_pending(self.rows.as_slice(), last, &self.marks) {
                    self.selection = RowSelection { lead: Some(next), anchor: Some(next), keys: None };
                    self.list_scroll.scroll_to_item(next, ScrollStrategy::Nearest);
                }
                self.sync_preview(cx);
            }
            AfterReplace::Close => cx.emit(FindPopupEvent::Dismissed),
        }
    }

    /// Stop: ends the running Replace between two files.
    pub fn stop_replace(&mut self, cx: &mut Context<Self>) {
        let Some(run) = &mut self.running else { return };
        let request = run.cancel();
        if let Some(backend) = cx.try_global::<Backend>() {
            backend.host.cancel_request(request);
        }
    }

    /// Whether the preview shows the replacement of the selected row rather than its file.
    fn shows_diff(&self) -> bool {
        self.preview_mode && self.state.replace && !self.is_stale()
    }

    /// The file preview, when it is the selected row's file as it is now.
    fn shown_preview(&self) -> Option<&FilePreview> {
        let row = self.selected_row()?;
        let key = PreviewKey { path: row.path.clone(), version: self.preview_version };
        self.preview.as_ref().filter(|preview| preview.key == key)
    }

    /// The replacement preview, when it is the one of the selected row as the options are now.
    fn shown_diff(&self) -> Option<&DiffPreview> {
        let key = self.diff_key(self.selected_row()?);
        self.diff.as_ref().filter(|diff| diff.key == key)
    }

    fn diff_key(&self, row: &MatchRow) -> DiffKey {
        let options = &self.state.options;
        let only = pending_usages(std::slice::from_ref(row), &self.marks);
        DiffKey {
            row: row.key.clone(),
            path: row.path.clone(),
            line: row.line,
            query: self.query.clone(),
            replacement: options.replacement.clone(),
            preserve_case: options.preserve_case,
            only: preview_positions(Some(only.as_slice())).unwrap_or_default(),
            version: self.preview_version,
        }
    }

    /// The matches of the selected row's file by line, at their columns in the real lines. None while the
    /// rows are outdated.
    fn file_highlights(&self) -> HashMap<u32, Vec<(u32, u32)>> {
        let Some(row) = self.selected_row().filter(|_| !self.is_stale()) else { return HashMap::new() };
        let file = self.session.state().files.iter().find(|file| file.path == row.path);
        let lines = file.into_iter().flat_map(|file| &file.matches).map(|line| {
            let ranges = line.ranges.iter().map(|&(from, to)| (from + line.text_offset, to + line.text_offset));
            (line.line, ranges.collect())
        });
        lines.collect()
    }

    /// Brings the preview up to the selected row: its file is read once the selection has rested on it,
    /// and scrolled to the row's line; in the diff mode of Replace the row's replacement is asked for
    /// instead.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.selected_row().cloned() else {
            self._preview_task = None;
            self.preview_wanted = self.preview.as_ref().map(|preview| preview.key.clone());
            self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
            return;
        };
        if self.shows_diff() {
            self.sync_diff(&row, cx);
            return;
        }
        let key = PreviewKey { path: row.path.clone(), version: self.preview_version };
        if self.preview.as_ref().is_some_and(|preview| preview.key == key) {
            self.reveal_preview_line(&row);
            return;
        }
        if self.preview_wanted.as_ref() == Some(&key) {
            return;
        }
        self.preview_wanted = Some(key.clone());
        self.diff_wanted = self.diff.as_ref().map(|diff| diff.key.clone());
        self._preview_task = Some(cx.spawn(async move |popup, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            let read = popup.update(cx, |popup, cx| {
                let backend = cx.try_global::<Backend>()?;
                let (window, path) = (popup.window_label.clone(), PathBuf::from(&key.path));
                Some(
                    backend
                        .with_host(move |host| async move { ide_host::ops::fs::read_text(&host, &window, path).await }),
                )
            });
            let Ok(Some(read)) = read else { return };
            let text = match read.await {
                Ok(Ok(text)) => Ok(text),
                Ok(Err(error)) => Err(error.message),
                Err(error) => Err(error.to_string()),
            };
            let _ = popup.update(cx, |popup, cx| popup.preview_loaded(key, text, cx));
        }));
    }

    fn preview_loaded(&mut self, key: PreviewKey, text: Result<String, String>, cx: &mut Context<Self>) {
        if self.preview_wanted.as_ref() != Some(&key) {
            return;
        }
        self.preview_shown = None;
        match text {
            Ok(text) => {
                let lines = text.lines().map(str::to_owned).collect();
                self.preview = Some(FilePreview { key, lines: Rc::new(lines) });
                self.preview_error = None;
                if let Some(row) = self.selected_row().cloned() {
                    self.reveal_preview_line(&row);
                }
            }
            Err(message) => {
                self.preview = None;
                self.preview_error = Some(message);
            }
        }
        cx.notify();
    }

    /// Scrolls the preview to the row's line, in the middle, when it is not the line it was last put on.
    fn reveal_preview_line(&mut self, row: &MatchRow) {
        let at = (row.path.clone(), row.line);
        if self.preview_shown.as_ref() != Some(&at) {
            self.preview_scroll.scroll_to_item(row.line as usize, ScrollStrategy::Center);
            self.preview_shown = Some(at);
        }
    }

    fn sync_diff(&mut self, row: &MatchRow, cx: &mut Context<Self>) {
        let key = self.diff_key(row);
        if self.diff.as_ref().is_some_and(|diff| diff.key == key) || self.diff_wanted.as_ref() == Some(&key) {
            return;
        }
        self.diff_wanted = Some(key.clone());
        self.preview_wanted = self.preview.as_ref().map(|preview| preview.key.clone());
        self._preview_task = Some(cx.spawn(async move |popup, cx| {
            cx.background_executor().timer(PREVIEW_DEBOUNCE).await;
            let asked = key.clone();
            let preview = popup.update(cx, |popup, cx| {
                let backend = cx.try_global::<Backend>()?;
                let DiffKey { path, query, replacement, preserve_case, only, .. } = asked;
                let (window, path, query) = (popup.window_label.clone(), PathBuf::from(path), to_backend(&query));
                Some(backend.with_host(move |host| async move {
                    let only = Some(only);
                    ide_host::ops::search::replace_preview(
                        &host,
                        &window,
                        query,
                        replacement,
                        preserve_case,
                        path,
                        only,
                    )
                    .await
                }))
            });
            let Ok(Some(preview)) = preview else { return };
            let lines = match preview.await {
                Ok(Ok(preview)) => Some(replace_preview_lines(&preview.before, &preview.edits)),
                Ok(Err(error)) => {
                    tracing::debug!("no replacement preview: {error}");
                    None
                }
                Err(error) => {
                    tracing::warn!("cannot ask for the replacement preview: {error}");
                    None
                }
            };
            let _ = popup.update(cx, |popup, cx| popup.diff_loaded(key, lines, cx));
        }));
    }

    /// The replacement preview of `key` arrived: the lines around the row's first change are kept, and the
    /// preview is scrolled to that change.
    fn diff_loaded(&mut self, key: DiffKey, lines: Option<Vec<PreviewLine>>, cx: &mut Context<Self>) {
        if self.diff_wanted.as_ref() != Some(&key) {
            return;
        }
        self.diff = lines.map(|lines| {
            let changed = |line: &PreviewLine| match line.kind {
                PreviewKind::Same => false,
                PreviewKind::Del => line.line >= key.line,
                PreviewKind::Add => true,
            };
            let focus = lines.iter().position(changed);
            let from = focus.unwrap_or(0).saturating_sub(DIFF_WINDOW);
            let to = (focus.unwrap_or(0) + DIFF_WINDOW).min(lines.len());
            let focus = focus.map(|focus| focus - from);
            if let Some(focus) = focus {
                self.diff_scroll.scroll_to_item(focus, ScrollStrategy::Center);
            }
            DiffPreview { key, lines: Rc::new(lines[from..to].to_vec()), focus }
        });
        cx.notify();
    }

    /// Why nothing is listed: what to do, what went wrong, or which options the search that found nothing
    /// was made with and how to drop them.
    fn empty_state(&self, cx: &mut App) -> EmptyState {
        let options = &self.state.options;
        let state = self.session.state();
        let masked = options.mask_enabled && !options.mask.trim().is_empty();
        let message = |text: String| EmptyState { lines: vec![text], link: None };
        if options.pattern.is_empty() && !masked {
            return message(tx(cx, "Type search query to find in files"));
        }
        match state.status {
            FindStatus::Invalid => return message(state.error.clone().unwrap_or_else(|| tx(cx, "Invalid pattern"))),
            FindStatus::Error => return message(state.error.clone().unwrap_or_else(|| tx(cx, "Search failed"))),
            status if !options.pattern.is_empty() && status != FindStatus::Done => {
                return message(tx(cx, "Searching…"));
            }
            _ => {}
        }
        let used: Vec<(&str, &str)> = [
            (options.case_sensitive, "Match Case", "Alt+C"),
            (options.whole_word, "Words", "Alt+W"),
            (options.is_regex, "Regex", "Alt+X"),
            (masked, "File mask", "Alt+K"),
            (options.context != SearchContext::Any, "Filter", ""),
        ]
        .into_iter()
        .filter_map(|(on, label, key)| on.then_some((label, key)))
        .collect();
        let regex = !options.pattern.is_empty() && !options.is_regex && could_be_regex(&options.pattern);
        let mut headline = tx(cx, if options.pattern.is_empty() { "No search text entered" } else { "Nothing found" });
        if regex || !used.is_empty() {
            headline.push('.');
        }
        let mut lines = vec![headline];
        if regex {
            let link = (tx(cx, "Search with Regex"), key_label("Alt+X"), EmptyLink::SearchWithRegex);
            return EmptyState { lines, link: Some(link) };
        }
        let link = match used.as_slice() {
            [] => return EmptyState { lines, link: None },
            [(label, _)] => {
                let i18n = &mut cx.default_global::<Localization>().0;
                lines.push(i18n.tx_built(&["Search option used: ", ""], &[Param::from(*label)]));
                tx(cx, "Clear option")
            }
            several => {
                lines.push(tx(cx, "Search options used:"));
                let named: Vec<String> = several
                    .iter()
                    .map(|(label, key)| {
                        let label = tx(cx, label);
                        if key.is_empty() { label } else { format!("{label} {}", key_label(key)) }
                    })
                    .collect();
                lines.push(named.join("  "));
                tx(cx, "Clear all options")
            }
        };
        EmptyState { lines, link: Some((link, key_label("Shift+Alt+Backspace"), EmptyLink::ClearOptions)) }
    }

    /// A text field changed, or Escape was pressed in it.
    fn field_event(&mut self, field: Field, event: &TextFieldEvent, cx: &mut Context<Self>) {
        match event {
            TextFieldEvent::Changed => {
                let text = self.field(field).read(cx).text().to_owned();
                let options = &mut self.state.options;
                match field {
                    Field::Pattern => options.pattern = text,
                    Field::Replacement => options.replacement = text,
                    // A mask that is typed is in use.
                    Field::Mask => {
                        options.mask = text;
                        options.mask_enabled = true;
                    }
                    // The text as typed: it is resolved against the project only when searching.
                    Field::Directory => options.directory = text,
                }
                self.options_changed(cx);
            }
            // Enter in a query field is claimed before the field sees it; in the others it does nothing.
            TextFieldEvent::Submitted => {}
            TextFieldEvent::Cancelled => {
                if self.menu.is_some() {
                    self.close_menu(cx);
                } else {
                    cx.emit(FindPopupEvent::Dismissed);
                }
            }
        }
    }

    /// Alt with a letter: the option, scope tab or button that letter stands for. Returns whether `key` is
    /// such a letter.
    fn mnemonic(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if let Some(&(tab, _, _)) = SCOPE_TABS.iter().find(|(_, _, letter)| *letter == key) {
            self.select_scope(tab, window, cx);
            return true;
        }
        match key {
            "c" => self.toggle(FindToggle::MatchCase, cx),
            "w" => self.toggle(FindToggle::Words, cx),
            "x" => self.toggle(FindToggle::Regex, cx),
            "k" => {
                let on = !self.state.options.mask_enabled;
                self.set_mask_enabled(on, cx);
                if on {
                    self.mask.update(cx, |field, cx| field.select_all(cx));
                    window.focus(&self.mask.focus_handle(cx), cx);
                }
            }
            "e" if self.state.replace => self.toggle(FindToggle::PreserveCase, cx),
            "b" => self.set_new_tab(!self.state.new_tab, cx),
            "a" if self.state.replace => self.replace_all(window, cx),
            _ => return false,
        }
        true
    }

    /// Tab moves the keyboard to the next text field shown, ⇧Tab to the one before.
    fn focus_next_field(&self, from: Field, backward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let shown: Vec<Field> = [
            (true, Field::Pattern),
            (self.state.replace, Field::Replacement),
            (true, Field::Mask),
            (self.state.options.scope_tab == ScopeTab::Directory, Field::Directory),
        ]
        .into_iter()
        .filter_map(|(shown, field)| shown.then_some(field))
        .collect();
        let at = shown.iter().position(|field| *field == from).unwrap_or(0);
        let next = shown[if backward { at + shown.len() - 1 } else { at + 1 } % shown.len()];
        window.focus(&self.field(next).focus_handle(cx), cx);
    }

    /// The keys the popup handles itself while the keyboard is in `field`, ahead of the field and of the
    /// keymap. Returns whether `keystroke` is one of them.
    ///
    /// In every field: the keys of an open list, ⇧⌥⌫ (clear the options), ⇧⌥↵ (Replace All), the Alt
    /// letters and Tab. In the two query fields also Enter (open the match, ⇧ in the other pane; in Replace
    /// replace it; with the primary modifier open the Find window) and the keys that move along the results,
    /// ⇧ extending the selection. Keys that are shortcuts of their own (next occurrence, jump to source,
    /// recent searches) are left to the keymap, which hands them to [`Self::run_command`].
    fn claim_key(&mut self, field: Field, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let (alt, shift, primary) = (modifiers.alt, modifiers.shift, modifiers.platform || modifiers.control);
        if let Some(chosen) = self.menu.as_ref().map(|menu| menu.active) {
            match (key, chosen) {
                ("up" | "down", _) => self.step_menu(key == "down", cx),
                ("enter", Some(item)) => self.choose_menu_item(item, cx),
                ("enter" | "escape", _) => self.close_menu(cx),
                _ => return false,
            }
            return true;
        }
        if alt && shift && !primary {
            match key {
                "backspace" => self.clear_options(cx),
                "enter" => self.replace_all(window, cx),
                _ => return false,
            }
            return true;
        }
        if alt && !shift && !primary && self.mnemonic(key, window, cx) {
            return true;
        }
        if key == "tab" && !alt && !primary {
            self.focus_next_field(field, shift, window, cx);
            return true;
        }
        if !matches!(field, Field::Pattern | Field::Replacement) {
            return false;
        }
        match key {
            "enter" if primary => self.open_in_find_window(cx),
            "enter" if self.state.replace && !shift => self.replace_selected(cx),
            "enter" => self.open_selected(shift, cx),
            "up" | "down" | "pageup" | "pagedown" if !alt && !primary => {
                if let Some(next) = self.active().and_then(|active| list_navigate(key, self.rows.len(), active)) {
                    self.move_to(next, selection_mode(shift, false), cx);
                }
            }
            _ => return false,
        }
        true
    }
}

impl Focusable for FindPopup {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.pattern.focus_handle(cx)
    }
}

impl FindPopup {
    /// The title with the count of matches, then the file mask, the search context and the pin.
    fn render_header(&self, cx: &mut Context<Self>) -> Div {
        let options = &self.state.options;
        let title = tx(cx, if self.state.replace { "Replace in Files" } else { "Find in Files" });
        let status = self.status_text(cx);
        let mask_label = tx(cx, "File mask:");
        let mask_box = div()
            .flex_none()
            .w(px(150.0))
            .h(theme::length(cx, "h-control"))
            .flex()
            .items_center()
            .pl(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-field"))
            .child(div().flex_1().min_w_0().child(self.mask.clone()))
            .child(
                icon_button(cx, "chevron-down", self.menu() == Some(FindMenu::Masks))
                    .id("find-popup-masks")
                    .debug_selector(|| "find-popup-masks".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_menu(FindMenu::Masks, cx))),
            );
        div()
            .flex_none()
            .h(px(HEADER_HEIGHT))
            .flex()
            .items_center()
            .gap(px(10.0))
            .pl(px(20.0))
            .pr(px(14.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .text_color(theme::color(cx, "fg-subtle"))
            .child(
                div()
                    .flex_none()
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(theme::color(cx, "fg-faint"))
                    .text_size(theme::length(cx, "fs-small"))
                    .child(SharedString::from(status)),
            )
            .child(
                checkbox(cx, mask_label, options.mask_enabled)
                    .id("find-popup-mask-enabled")
                    .debug_selector(|| "find-popup-mask-enabled".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| {
                        popup.set_mask_enabled(!popup.state.options.mask_enabled, cx);
                    })),
            )
            .child(mask_box)
            .child(
                icon_button(cx, "filter", options.context != SearchContext::Any)
                    .id("find-popup-context")
                    .debug_selector(|| "find-popup-context".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_menu(FindMenu::Context, cx))),
            )
            .child(
                icon_button(cx, "pin", self.state.pinned)
                    .id("find-popup-pin")
                    .debug_selector(|| "find-popup-pin".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.set_pinned(!popup.state.pinned, cx))),
            )
    }

    /// The text to find with its toggles, the error of an invalid pattern, and in Replace the replacement.
    fn render_fields(&self, cx: &mut Context<Self>) -> Div {
        let options = &self.state.options;
        let state = self.session.state();
        let invalid = state.status == FindStatus::Invalid;
        let muted = theme::color(cx, "fg-muted");
        let history = |cx: &mut Context<Self>, kind: HistoryKind, id: &'static str| {
            icon_button(cx, "search-history", false).id(id).debug_selector(move || id.to_owned()).on_click(
                cx.listener(move |popup, _: &ClickEvent, _, cx| popup.toggle_menu(FindMenu::History(kind), cx)),
            )
        };
        let toggle = |cx: &mut Context<Self>, option: FindToggle, icon: &str, on: bool, id: &'static str| {
            icon_button(cx, icon, on)
                .id(id)
                .debug_selector(move || id.to_owned())
                .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.toggle(option, cx)))
        };
        let find = field_frame(cx, invalid)
            .child(theme::icon(cx, "search", 16.0, muted))
            .child(div().flex_1().min_w_0().child(self.pattern.clone()))
            .child(history(cx, HistoryKind::Find, "find-popup-history"))
            .child(toggle(cx, FindToggle::MatchCase, "case", options.case_sensitive, "find-popup-case"))
            .child(toggle(cx, FindToggle::Words, "word", options.whole_word, "find-popup-word"))
            .child(toggle(cx, FindToggle::Regex, "regex", options.is_regex, "find-popup-regex"));
        let error = state.error.clone().filter(|_| invalid).map(|error| {
            div()
                .flex_none()
                .h(px(ERROR_HEIGHT))
                .pl(px(2.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_color(theme::color(cx, "danger"))
                .text_size(theme::length(cx, "fs-small"))
                .child(SharedString::from(error))
        });
        let replace = self.state.replace.then(|| {
            let preserve = toggle(
                cx,
                FindToggle::PreserveCase,
                "preserve-case",
                options.preserve_case,
                "find-popup-preserve-case",
            );
            field_frame(cx, false)
                .child(theme::icon(cx, "replace", 16.0, muted))
                .child(div().flex_1().min_w_0().child(self.replacement.clone()))
                .child(history(cx, HistoryKind::Replace, "find-popup-replace-history"))
                .child(preserve)
        });
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(8.0))
            .pt(px(8.0))
            .child(find)
            .children(error)
            .children(replace)
    }

    /// The scope tabs, and what the selected one searches in.
    fn render_scope_row(&self, cx: &mut Context<Self>) -> Div {
        let options = &self.state.options;
        let (strong, muted) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let (pressed, hover) = (theme::color(cx, "bg-toolbar-pressed"), theme::color(cx, "bg-toolbar-hover"));
        let tabs: Vec<AnyElement> = SCOPE_TABS
            .into_iter()
            .map(|(tab, label, letter)| {
                let on = tab == options.scope_tab;
                div()
                    .id(letter)
                    .debug_selector(move || format!("find-popup-scope-{letter}"))
                    .flex_none()
                    .h(theme::length(cx, "h-control"))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .text_color(if on { strong } else { muted })
                    .when(on, |el| el.bg(pressed))
                    .hover(|el| el.bg(hover))
                    .child(SharedString::from(tx(cx, label)))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, window, cx| {
                        popup.select_scope(tab, window, cx);
                    }))
                    .into_any_element()
            })
            .collect();
        let detail: Vec<AnyElement> = match options.scope_tab {
            ScopeTab::Project => Vec::new(),
            ScopeTab::Directory => vec![
                div()
                    .flex_1()
                    .min_w_0()
                    .h(theme::length(cx, "h-control"))
                    .flex()
                    .items_center()
                    .px(px(6.0))
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .bg(theme::color(cx, "bg-input"))
                    .child(self.directory.clone())
                    .into_any_element(),
                icon_button(cx, "folder-open", false)
                    .id("find-popup-browse")
                    .debug_selector(|| "find-popup-browse".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.browse(cx)))
                    .into_any_element(),
            ],
            ScopeTab::Module => {
                let chosen = self.modules.iter().find(|module| module.path == options.module);
                let label = chosen.map(|module| module_label(module, cx)).unwrap_or_default();
                vec![
                    select_box(cx, label)
                        .id("find-popup-module")
                        .debug_selector(|| "find-popup-module".to_owned())
                        .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_menu(FindMenu::Module, cx)))
                        .into_any_element(),
                ]
            }
            ScopeTab::Scope => {
                let label = match self.shown_scope() {
                    NamedScope::Custom(name) => name,
                    NamedScope::Builtin(scope) => {
                        let label = NAMED_SCOPES.iter().find(|(named, _)| *named == scope).map(|(_, label)| *label);
                        tx(cx, label.unwrap_or_default())
                    }
                };
                vec![
                    select_box(cx, label)
                        .id("find-popup-named-scope")
                        .debug_selector(|| "find-popup-named-scope".to_owned())
                        .on_click(
                            cx.listener(|popup, _: &ClickEvent, _, cx| popup.toggle_menu(FindMenu::NamedScope, cx)),
                        )
                        .into_any_element(),
                    icon_button(cx, "more-horizontal", false)
                        .id("find-popup-edit-scopes")
                        .debug_selector(|| "find-popup-edit-scopes".to_owned())
                        .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| {
                            let select = custom_scope_name(&popup.state.options.named_scope).map(str::to_owned);
                            cx.emit(FindPopupEvent::EditScopes { select });
                        }))
                        .into_any_element(),
                ]
            }
        };
        div()
            .flex_none()
            .h(px(SCOPE_ROW_HEIGHT))
            .flex()
            .items_center()
            .gap(px(4.0))
            .pl(px(20.0))
            .pr(px(14.0))
            .children(tabs)
            .child(
                div().flex_1().min_w_0().flex().items_center().justify_end().gap(px(4.0)).pl(px(8.0)).children(detail),
            )
    }

    /// The matching lines, each with its file and line number.
    fn render_results(&self, cx: &mut Context<Self>) -> AnyElement {
        let stale = self.is_stale();
        let workspace = !self.workspace_folders.is_empty();
        let replace = self.state.replace;
        let active = self.active();
        let replaced = tx(cx, "replaced ·");
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let (subtle, faint) = (theme::color(cx, "fg-subtle"), theme::color(cx, "fg-faint"));
        let (match_bg, match_fg) = (theme::color(cx, "bg-search-match"), theme::color(cx, "fg-search-match"));
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let mono = code_font(cx);
        uniform_list(
            "find-popup-results",
            self.rows.len(),
            cx.processor(move |popup, range: Range<usize>, _, cx| {
                range
                    .filter_map(|index| {
                        let row = popup.rows.get(index)?;
                        let done = replace && group_state(row, &popup.marks) == GroupState::Done;
                        let selected = active == Some(index) || is_row_selected(&popup.selection, &row.key);
                        let ranges: &[(u32, u32)] = if stale { &[] } else { &row.ranges };
                        let file = if workspace { row.rel_path.clone() } else { basename(&row.rel_path).into_owned() };
                        let line = div()
                            .id(index)
                            .debug_selector(|| format!("find-popup-row-{index}"))
                            .h(px(ROW_HEIGHT))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .px(px(20.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(selected, |el| el.bg(selected_bg))
                            .when(!selected, |el| el.hover(|el| el.bg(hover_bg)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .flex()
                                    .font_family(mono.clone())
                                    .text_color(if stale || done { faint } else { fg })
                                    .children(marked(segments(&row.text, ranges), match_bg, match_fg)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .flex()
                                    .gap(px(4.0))
                                    .text_color(if stale {
                                        faint
                                    } else if active == Some(index) {
                                        muted
                                    } else {
                                        subtle
                                    })
                                    .children(
                                        done.then(|| {
                                            div().text_color(faint).child(SharedString::from(replaced.clone()))
                                        }),
                                    )
                                    .child(SharedString::from(file))
                                    .child(
                                        div().text_color(faint).child(SharedString::from((row.line + 1).to_string())),
                                    ),
                            )
                            .on_click(cx.listener(move |popup, event: &ClickEvent, _, cx| {
                                popup.click_row(index, event, cx);
                            }));
                        Some(line)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.list_scroll)
        .size_full()
        .into_any_element()
    }

    /// What is shown in place of the results while there are none.
    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let EmptyState { lines, link } = self.empty_state(cx);
        let link = link.map(|(label, keys, link)| {
            div()
                .flex()
                .gap(px(6.0))
                .child(
                    div()
                        .id("find-popup-empty-link")
                        .debug_selector(|| "find-popup-empty-link".to_owned())
                        .text_color(theme::color(cx, "link"))
                        .child(SharedString::from(label))
                        .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| match link {
                            EmptyLink::SearchWithRegex => popup.toggle(FindToggle::Regex, cx),
                            EmptyLink::ClearOptions => popup.clear_options(cx),
                        })),
                )
                .child(SharedString::from(keys))
        });
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(4.0))
            .px(px(20.0))
            .text_color(theme::color(cx, "fg-muted"))
            .children(lines.into_iter().map(|line| div().child(SharedString::from(line))))
            .children(link)
    }

    /// The selected match in its file, or in Replace's diff mode what replacing it changes.
    fn render_preview(&self, cx: &mut Context<Self>) -> Div {
        let (strong, subtle) = (theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"));
        let (fg, gutter) = (theme::color(cx, "fg"), theme::color(cx, "fg-gutter"));
        let mono = code_font(cx);
        let header = self.selected_row().map(|row| {
            let folder = row.rel_path.contains('/').then(|| dirname(&row.rel_path).to_owned());
            div()
                .flex()
                .gap(px(8.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(div().text_color(strong).child(SharedString::from(basename(&row.rel_path).into_owned())))
                .children(folder.map(SharedString::from))
        });
        let line_frame = move |number: u32, sign: Option<&'static str>| {
            div()
                .h(px(ROW_HEIGHT - 4.0))
                .w_full()
                .flex()
                .items_center()
                .whitespace_nowrap()
                .overflow_hidden()
                .child(
                    div()
                        .flex_none()
                        .w(px(GUTTER_WIDTH))
                        .flex()
                        .justify_end()
                        .pr(px(8.0))
                        .text_color(gutter)
                        .child(SharedString::from((number + 1).to_string())),
                )
                .children(sign.map(|sign| div().flex_none().w(px(14.0)).text_color(gutter).child(sign)))
        };
        let body = if self.shows_diff() {
            match self.shown_diff() {
                None => div().size_full().into_any_element(),
                Some(diff) => {
                    let (lines, focus) = (Rc::clone(&diff.lines), diff.focus);
                    let (added, deleted) = (theme::color(cx, "diff-inserted"), theme::color(cx, "diff-deleted"));
                    let current = theme::color(cx, "bg-current-line");
                    uniform_list(
                        "find-popup-diff",
                        lines.len(),
                        cx.processor(move |_, range: Range<usize>, _, _| {
                            range
                                .map(|index| {
                                    let line = &lines[index];
                                    let (sign, bg) = match line.kind {
                                        PreviewKind::Same => (" ", None),
                                        PreviewKind::Del => ("-", Some(deleted)),
                                        PreviewKind::Add => ("+", Some(added)),
                                    };
                                    let bg = bg.or((focus == Some(index)).then_some(current));
                                    let frame = line_frame(line.line, Some(sign));
                                    let frame = match bg {
                                        Some(bg) => frame.bg(bg),
                                        None => frame,
                                    };
                                    frame.child(SharedString::from(line.text.replace('\t', "    ")))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.diff_scroll)
                    .size_full()
                    .into_any_element()
                }
            }
        } else {
            match (self.shown_preview(), self.selected_row()) {
                (Some(preview), Some(row)) => {
                    let (lines, current_line) = (Rc::clone(&preview.lines), row.line);
                    let highlights = Rc::new(self.file_highlights());
                    let current = theme::color(cx, "bg-current-line");
                    let (mark, mark_current) =
                        (theme::color(cx, "bg-search-match"), theme::color(cx, "bg-search-match-current"));
                    let mark_fg = theme::color(cx, "fg-search-match");
                    uniform_list(
                        "find-popup-preview",
                        lines.len(),
                        cx.processor(move |_, range: Range<usize>, _, _| {
                            range
                                .map(|index| {
                                    let number = index as u32;
                                    let on_line = number == current_line;
                                    let ranges = highlights.get(&number).map(Vec::as_slice).unwrap_or_default();
                                    let pieces = segments(&lines[index], ranges);
                                    line_frame(number, None).when(on_line, |el| el.bg(current)).children(marked(
                                        pieces,
                                        if on_line { mark_current } else { mark },
                                        mark_fg,
                                    ))
                                })
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(&self.preview_scroll)
                    .size_full()
                    .into_any_element()
                }
                _ => {
                    let note = match (&self.preview_error, self.selected_row()) {
                        (_, None) => tx(cx, "Select a result to preview"),
                        (Some(error), Some(_)) => error.clone(),
                        (None, Some(_)) => String::new(),
                    };
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(subtle)
                        .child(SharedString::from(note))
                        .into_any_element()
                }
            }
        };
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .h(px(PREVIEW_HEADER_HEIGHT))
                    .flex()
                    .items_center()
                    .px(px(20.0))
                    .border_t_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .text_color(subtle)
                    .children(header),
            )
            .child(
                div()
                    .flex_none()
                    .h(px(PREVIEW_HEIGHT))
                    .bg(theme::color(cx, "bg-app"))
                    .text_color(fg)
                    .font_family(mono)
                    .child(body),
            )
    }

    /// "Open results in new tab", the progress of a Replace, and the buttons.
    fn render_footer(&self, cx: &mut Context<Self>) -> Div {
        let replace = self.state.replace;
        let stale = self.is_stale();
        let busy = self.running.is_some();
        let searching = self.session.state().status == FindStatus::Searching;
        let faint = theme::color(cx, "fg-faint");
        let (new_tab, preview_label) = (tx(cx, "Open results in new tab"), tx(cx, "Preview"));
        let (stop_label, open_label) = (tx(cx, "Stop"), tx(cx, "Open in Find Window"));
        let (replace_all_label, replace_label) = (tx(cx, "Replace All"), self.replace_label(cx));
        let preview = replace.then(|| {
            checkbox(cx, preview_label, self.preview_mode)
                .id("find-popup-preview-mode")
                .debug_selector(|| "find-popup-preview-mode".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.set_preview_mode(!popup.preview_mode, cx)))
        });
        let progress = self.progress.as_ref().filter(|_| replace).map(|progress| {
            let i18n = &mut cx.default_global::<Localization>().0;
            let (done, total) = (Param::from(progress.done as usize), Param::from(progress.total as usize));
            let text = i18n.tx_built(&["Replacing… ", "/", " files"], &[done, total]);
            div().flex_none().text_color(faint).text_size(theme::length(cx, "fs-small")).child(SharedString::from(text))
        });
        let stop = (replace && busy).then(|| {
            button(cx, stop_label, false, true)
                .id("find-popup-stop")
                .debug_selector(|| "find-popup-stop".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.stop_replace(cx)))
        });
        let can_open = !self.state.options.pattern.is_empty();
        let open = button(cx, open_label, false, can_open)
            .id("find-popup-open-in-find-window")
            .debug_selector(|| "find-popup-open-in-find-window".to_owned())
            .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.open_in_find_window(cx)));
        let replace_all = replace.then(|| {
            let enabled = !stale && !self.rows.is_empty() && !busy && !searching;
            button(cx, replace_all_label, false, enabled)
                .id("find-popup-replace-all")
                .debug_selector(|| "find-popup-replace-all".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, window, cx| {
                    if popup.session.state().status != FindStatus::Searching {
                        popup.replace_all(window, cx);
                    }
                }))
        });
        let replace_one = replace.then(|| {
            let enabled = !stale && self.selected_row().is_some() && !busy;
            button(cx, replace_label, true, enabled)
                .id("find-popup-replace")
                .debug_selector(|| "find-popup-replace".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.replace_selected(cx)))
        });
        div()
            .flex_none()
            .h(px(FOOTER_HEIGHT))
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .child(
                checkbox(cx, new_tab, self.state.new_tab)
                    .id("find-popup-new-tab")
                    .debug_selector(|| "find-popup-new-tab".to_owned())
                    .on_click(cx.listener(|popup, _: &ClickEvent, _, cx| popup.set_new_tab(!popup.state.new_tab, cx))),
            )
            .children(preview)
            .child(div().flex_1())
            .children(progress)
            .children(stop)
            .child(div().flex_none().text_color(faint).child(SharedString::from(key_label("Meta+Enter"))))
            .child(open)
            .children(replace_all)
            .children(replace_one)
    }

    /// The open list, over the popup below the control it belongs to. A click beside it closes it.
    fn render_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let menu = self.menu.as_ref()?;
        let invalid = self.session.state().status == FindStatus::Invalid;
        let fields = 8.0
            + FIELD_HEIGHT
            + if invalid { 8.0 + ERROR_HEIGHT } else { 0.0 }
            + if self.state.replace { 8.0 + FIELD_HEIGHT } else { 0.0 };
        let (top, right) = match menu.kind {
            FindMenu::History(HistoryKind::Find) => (HEADER_HEIGHT + 8.0 + FIELD_HEIGHT, false),
            FindMenu::History(HistoryKind::Replace) => (HEADER_HEIGHT + fields, false),
            FindMenu::Masks | FindMenu::Context => (HEADER_HEIGHT - 6.0, true),
            FindMenu::Module | FindMenu::NamedScope => (HEADER_HEIGHT + fields + SCOPE_ROW_HEIGHT - 4.0, true),
        };
        let (fg, faint, subtle) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-faint"), theme::color(cx, "fg-subtle"));
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-hover"));
        let check = theme::color(cx, "fg-muted");
        let active = menu.active;
        let items: Vec<AnyElement> = menu
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mark = if item.checked {
                    theme::icon(cx, "check", 12.0, check)
                } else {
                    div().flex_none().size(px(12.0)).into_any_element()
                };
                let line = div()
                    .id(index)
                    .debug_selector(move || format!("find-popup-menu-{index}"))
                    .flex_none()
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(8.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(mark)
                    .child(SharedString::from(item.label.clone()))
                    .children(
                        item.detail.clone().map(|detail| div().text_color(subtle).child(SharedString::from(detail))),
                    );
                if item.choice.is_none() {
                    return line.text_color(faint).into_any_element();
                }
                line.text_color(fg)
                    .when(active == Some(index), |el| el.bg(selected_bg))
                    .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                    .on_click(cx.listener(move |popup, _: &ClickEvent, _, cx| popup.choose_menu_item(index, cx)))
                    .into_any_element()
            })
            .collect();
        let list = div()
            .flex_none()
            .w(px(MENU_WIDTH))
            .flex()
            .flex_col()
            .px(px(4.0))
            .py(px(4.0))
            .rounded(px(8.0))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(items);
        let backdrop = div()
            .id("find-popup-menu-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .pt(px(top))
            .px(px(12.0))
            .on_mouse_down(MouseButton::Left, cx.listener(|popup, _, _, cx| popup.close_menu(cx)))
            .child(div().flex_none().flex().when(right, |el| el.justify_end()).child(list));
        Some(deferred(backdrop).with_priority(1).into_any_element())
    }
}

impl Render for FindPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let header = self.render_header(cx);
        let fields = self.render_fields(cx);
        let scope = self.render_scope_row(cx);
        let body = if self.rows.is_empty() {
            div()
                .flex_none()
                .h(px(RESULTS_HEIGHT + PREVIEW_HEADER_HEIGHT + PREVIEW_HEIGHT))
                .child(self.render_empty(cx))
        } else {
            div()
                .flex_none()
                .flex()
                .flex_col()
                .child(div().flex_none().h(px(RESULTS_HEIGHT)).py(px(2.0)).child(self.render_results(cx)))
                .child(self.render_preview(cx))
        };
        let footer = self.render_footer(cx);
        let menu = self.render_menu(cx);
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
            .text_color(theme::color(cx, "fg"))
            .child(header)
            .child(fields)
            .child(scope)
            .child(body)
            .child(footer)
            .children(menu)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::find::ipc::{ProjectFileSet, SearchScope};
    use gpui_kit::{Modifiers, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<FindPopupEvent>>>;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
    }

    /// What the popup of the window `w1` is opened with for the project at `root`.
    fn window_input(root: &str) -> FindPopupInput {
        FindPopupInput { window_label: "w1".to_owned(), root: Some(root.to_owned()), ..FindPopupInput::default() }
    }

    /// A project on disk, open in the backend as the window `w1`.
    struct Project {
        _home: tempfile::TempDir,
        _folder: tempfile::TempDir,
        root: PathBuf,
    }

    impl Project {
        fn input(&self) -> FindPopupInput {
            window_input(&self.root.to_string_lossy())
        }

        fn read(&self, name: &str) -> String {
            std::fs::read_to_string(self.root.join(name)).expect("the file is read")
        }
    }

    /// Starts the backend and opens a project with `files` (name and text) in it.
    async fn project(cx: &mut TestAppContext, files: &[(&str, &str)]) -> Project {
        cx.executor().allow_parking();
        let (home, folder) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = folder.path().canonicalize().expect("the project folder exists");
        for (name, text) in files {
            let path = root.join(name);
            std::fs::create_dir_all(path.parent().expect("a file lies in a folder")).expect("the folder is made");
            std::fs::write(path, text).expect("the file is written");
        }
        init(cx);
        cx.update(|cx| Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts"));
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");
        Project { _home: home, _folder: folder, root }
    }

    fn close_project(cx: &mut VisualTestContext) {
        cx.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));
    }

    /// The popup in a window of its own, with the keyboard, and what it has asked of the window so far.
    fn open(cx: &mut TestAppContext, input: FindPopupInput) -> (Entity<FindPopup>, Events, &mut VisualTestContext) {
        let (popup, cx) = cx.add_window_view(|_, cx| FindPopup::new(input, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &FindPopupEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    /// Lets the popup's timers run out and the backend answer, until `done`.
    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.executor().advance_clock(SEARCH_DEBOUNCE + PREVIEW_DEBOUNCE);
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the popup did not settle within three seconds");
    }

    /// Waits for the search for what the options ask now to end.
    fn searched(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) {
        let ended = |status| matches!(status, FindStatus::Done | FindStatus::Invalid | FindStatus::Error);
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_stale() && ended(popup.status())));
    }

    fn lines(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.read_with(cx, |popup, _| popup.lines())
    }

    fn sorted_lines(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        let mut lines = lines(popup, cx);
        lines.sort();
        lines
    }

    fn selected(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Option<String> {
        popup.read_with(cx, |popup, _| popup.selected())
    }

    fn query(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> SearchQuery {
        popup.read_with(cx, |popup, _| popup.query().clone())
    }

    fn empty_text(popup: &Entity<FindPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.update(cx, |popup, cx| popup.empty_text(cx))
    }

    fn last(events: &Events) -> Option<FindPopupEvent> {
        events.borrow().last().cloned()
    }

    /// The list stored under `key`.
    fn stored_list(cx: &mut VisualTestContext, key: &str) -> Vec<String> {
        cx.update(|_, cx| find_history(cx.global::<UiStorage>().get(key).unwrap_or_default()))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
    }

    fn double_click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: 2, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: 2 });
    }

    const ROWS: [&str; 3] = ["find-popup-row-0", "find-popup-row-1", "find-popup-row-2"];

    /// Typing searches once the query has rested for [`SEARCH_DEBOUNCE`]: until then the popup says that the
    /// results are being updated. The matching lines are listed without their indentation, and counted.
    #[gpui_kit::test]
    async fn searches_as_you_type_once_the_query_has_rested(cx: &mut TestAppContext) {
        let project = project(
            cx,
            &[
                ("src/a.txt", "first line\n  a Needle here\nlast\n"),
                ("src/b.txt", "needle one\nnothing\nneedle two, needle three\n"),
                ("c.txt", "hay\n"),
            ],
        )
        .await;
        let (popup, _, cx) = open(cx, project.input());
        assert_eq!(empty_text(&popup, cx), ["Type search query to find in files"]);

        cx.simulate_input("needle");
        popup.update(cx, |popup, cx| {
            assert!(popup.is_stale());
            assert_eq!(popup.status(), FindStatus::Idle);
            assert_eq!(popup.status_text(cx), "Updating results…");
        });
        cx.executor().advance_clock(SEARCH_DEBOUNCE - Duration::from_millis(1));
        cx.run_until_parked();
        assert_eq!(popup.read_with(cx, |popup, _| popup.status()), FindStatus::Idle, "the query is still resting");

        searched(&popup, cx);
        assert_eq!(
            sorted_lines(&popup, cx),
            ["src/a.txt:2 a Needle here", "src/b.txt:1 needle one", "src/b.txt:3 needle two, needle three"]
        );
        assert_eq!(popup.read_with(cx, |popup, _| popup.match_count()), 4);
        assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "4 matches in 2 files");
        assert!(empty_text(&popup, cx).is_empty());
        // The first row is selected.
        assert_eq!(selected(&popup, cx), lines(&popup, cx).first().cloned());

        // What is typed next is searched for in its turn.
        cx.simulate_input("x");
        assert!(popup.read_with(cx, |popup, _| popup.is_stale()));
        searched(&popup, cx);
        assert!(lines(&popup, cx).is_empty());
        assert_eq!(empty_text(&popup, cx), ["Nothing found"]);
        close_project(cx);
    }

    /// Match Case, Words and Regex narrow the search, from their buttons and with Alt and their letters;
    /// ⇧⌥⌫ drops them all.
    #[gpui_kit::test]
    async fn the_option_toggles_search_again_with_them(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "Needle\nneedles\nneedle\n")]).await;
        let (popup, _, cx) = open(cx, project.input());
        cx.simulate_input("needle");
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:1 Needle", "a.txt:2 needles", "a.txt:3 needle"]);

        cx.simulate_keystrokes("alt-c");
        assert!(query(&popup, cx).case_sensitive);
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:2 needles", "a.txt:3 needle"]);

        click(cx, "find-popup-word", Modifiers::default());
        assert!(query(&popup, cx).whole_word);
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:3 needle"]);

        cx.simulate_keystrokes("shift-alt-backspace");
        let cleared = query(&popup, cx);
        assert!(!cleared.case_sensitive && !cleared.whole_word && !cleared.is_regex);
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx).len(), 3);

        cx.simulate_keystrokes("alt-x");
        assert!(query(&popup, cx).is_regex);
        popup.update(cx, |popup, cx| popup.set_pattern("ne+dle[s]", cx));
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:2 needles"]);
        click(cx, "find-popup-regex", Modifiers::default());
        assert!(!query(&popup, cx).is_regex);
        close_project(cx);
    }

    /// A pattern that is not a valid expression says so in place of the results, and finds nothing.
    #[gpui_kit::test]
    async fn an_invalid_regex_is_reported_instead_of_searched(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "(needle)\n")]).await;
        let (popup, _, cx) = open(cx, project.input());
        popup.update(cx, |popup, cx| {
            popup.toggle(FindToggle::Regex, cx);
            popup.set_pattern("(", cx);
        });
        searched(&popup, cx);
        let error = popup.read_with(cx, |popup, _| {
            assert_eq!(popup.status(), FindStatus::Invalid);
            popup.error().map(str::to_owned).expect("the pattern's error")
        });
        assert!(lines(&popup, cx).is_empty());
        assert_eq!(empty_text(&popup, cx), [error]);
        assert_eq!(popup.update(cx, |popup, cx| popup.status_text(cx)), "");

        // Once the pattern is one, it is searched for.
        popup.update(cx, |popup, cx| popup.set_pattern("\\(ne+dle\\)", cx));
        searched(&popup, cx);
        assert_eq!(popup.read_with(cx, |popup, _| popup.status()), FindStatus::Done);
        assert_eq!(lines(&popup, cx), ["a.txt:1 (needle)"]);
        close_project(cx);
    }

    /// The file mask limits the search to the files it names. Its check box and ⌥K put it in and out of
    /// use, ⌥K with the keyboard going to the mask; a mask in use when the search goes to the Find window is
    /// remembered.
    #[gpui_kit::test]
    async fn the_file_mask_limits_the_files_searched(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "needle\n"), ("b.rs", "needle\n")]).await;
        let (popup, _, cx) = open(cx, project.input());
        cx.simulate_input("needle");
        searched(&popup, cx);
        assert_eq!(sorted_lines(&popup, cx), ["a.txt:1 needle", "b.rs:1 needle"]);

        popup.update(cx, |popup, cx| popup.set_mask("*.rs", cx));
        assert_eq!(query(&popup, cx).include, ["*.rs"]);
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["b.rs:1 needle"]);

        click(cx, "find-popup-mask-enabled", Modifiers::default());
        assert!(query(&popup, cx).include.is_empty());
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx).len(), 2);

        cx.simulate_keystrokes("alt-k");
        assert_eq!(query(&popup, cx).include, ["*.rs"]);
        assert!(cx.update(|window, cx| popup.read(cx).mask_field().focus_handle(cx).is_focused(window)));
        // Typing replaces the mask, which was selected.
        cx.simulate_input("b");
        assert_eq!(query(&popup, cx).include, ["b"]);
        popup.update(cx, |popup, cx| popup.set_mask("*.txt", cx));
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:1 needle"]);

        popup.update(cx, |popup, cx| popup.open_in_find_window(cx));
        assert_eq!(stored_list(cx, MASKS_KEY), ["*.txt"]);
        close_project(cx);
    }

    /// Turned on with nothing typed, the mask takes the one used last; the list beside it offers the recent
    /// ones.
    #[gpui_kit::test]
    async fn an_empty_file_mask_takes_the_most_recent_one(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| cx.global_mut::<UiStorage>().set(MASKS_KEY, r#"["*.md","*.kt"]"#.to_owned()));
        let (popup, _, cx) = open(cx, window_input("/p"));
        cx.simulate_keystrokes("alt-k");
        popup.read_with(cx, |popup, cx| {
            assert_eq!(popup.query().include, ["*.md"]);
            assert_eq!(popup.mask_field().read(cx).text(), "*.md");
        });
        // With only a mask there is no text to search for.
        assert_eq!(empty_text(&popup, cx), ["No search text entered.", "Search option used: File mask"]);

        click(cx, "find-popup-masks", Modifiers::default());
        assert_eq!(
            popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines())),
            (Some(FindMenu::Masks), vec!["*.md".to_owned(), "*.kt".to_owned()])
        );
        click(cx, "find-popup-menu-1", Modifiers::default());
        popup.read_with(cx, |popup, cx| {
            assert_eq!(popup.menu(), None);
            assert_eq!(popup.query().include, ["*.kt"]);
            assert_eq!(popup.mask_field().read(cx).text(), "*.kt");
        });
    }

    /// The scope tabs decide where the search looks: the project, a module, a directory (which starts as
    /// the folder of the file being edited) or a named scope.
    #[gpui_kit::test]
    async fn the_scope_tabs_decide_where_the_search_looks(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| {
            let scopes = r#"[{"name":"Tests","pattern":"file:*test*"}]"#;
            cx.global_mut::<UiStorage>().set(&scopes_key("/p"), scopes.to_owned());
        });
        let (popup, events, cx) = open(
            cx,
            FindPopupInput {
                active_path: Some("/p/src/main.rs".to_owned()),
                open_paths: vec!["/p/a.rs".to_owned()],
                modules: vec![ProjectModule { name: "app".to_owned(), path: "app".to_owned() }],
                ..window_input("/p")
            },
        );
        let scope = |cx: &mut VisualTestContext| popup.read_with(cx, |popup, _| popup.query().scope.clone());
        let directory = |path: &str| SearchScope::Directory { path: path.to_owned() };
        assert_eq!(scope(cx), SearchScope::Project);

        // Directory: the folder of the file being edited, shown relative to the project.
        click(cx, "find-popup-scope-d", Modifiers::default());
        assert_eq!(scope(cx), directory("/p/src"));
        popup.read_with(cx, |popup, cx| {
            assert_eq!(popup.options().scope_tab, ScopeTab::Directory);
            assert_eq!(popup.directory_field().read(cx).text(), "src");
        });
        // Tab goes from the text to find to the mask and on to the directory, where typing edits it.
        cx.simulate_keystrokes("tab tab");
        cx.simulate_input("x");
        assert_eq!(scope(cx), directory("/p/srcx"));
        popup.update(cx, |popup, cx| popup.set_directory("/elsewhere/lib", cx));
        assert_eq!(scope(cx), directory("/elsewhere/lib"));

        // In Project, with ⌥P: the keyboard leaves the directory with its tab.
        cx.simulate_keystrokes("alt-p");
        assert_eq!(scope(cx), SearchScope::Project);
        assert!(cx.update(|window, cx| popup.read(cx).pattern_field().focus_handle(cx).is_focused(window)));

        cx.simulate_keystrokes("alt-m");
        assert_eq!(scope(cx), directory("/p/app"));

        // Scope: the project's files, then what its list offers.
        cx.simulate_keystrokes("alt-s");
        assert_eq!(scope(cx), SearchScope::FileSet { set: ProjectFileSet::All });
        click(cx, "find-popup-named-scope", Modifiers::default());
        assert_eq!(
            popup.read_with(cx, |popup, _| popup.menu_lines()),
            [
                "Project Files",
                "Project Production Files",
                "Project Test Files",
                "Open Files",
                "Recently Viewed Files",
                "Custom Scopes",
                "Tests"
            ]
        );
        popup.update(cx, |popup, cx| popup.choose_menu_item(2, cx));
        assert_eq!(scope(cx), SearchScope::FileSet { set: ProjectFileSet::Test });
        for (item, expected) in [
            (3, SearchScope::Files { paths: vec!["/p/a.rs".to_owned()] }),
            (6, SearchScope::Pattern { pattern: "file:*test*".to_owned() }),
            // Without a backend nothing was viewed recently.
            (4, SearchScope::Files { paths: Vec::new() }),
        ] {
            popup.update(cx, |popup, cx| {
                popup.open_menu(FindMenu::NamedScope, cx);
                // The heading of the custom scopes is not a choice.
                popup.choose_menu_item(5, cx);
                assert_eq!(popup.menu(), Some(FindMenu::NamedScope));
                popup.choose_menu_item(item, cx);
            });
            assert_eq!(scope(cx), expected);
        }
        // The scope editor is asked for with the custom scope that is chosen, if one is.
        click(cx, "find-popup-edit-scopes", Modifiers::default());
        assert_eq!(last(&events), Some(FindPopupEvent::EditScopes { select: None }));
        popup.update(cx, |popup, cx| {
            popup.open_menu(FindMenu::NamedScope, cx);
            popup.choose_menu_item(6, cx);
        });
        click(cx, "find-popup-edit-scopes", Modifiers::default());
        assert_eq!(last(&events), Some(FindPopupEvent::EditScopes { select: Some("Tests".to_owned()) }));
    }

    /// The arrows and the page keys move along the results and stop at their ends, ⇧ extends the selection,
    /// the preview follows the selected line, and Enter asks for the match to be opened.
    #[gpui_kit::test]
    async fn the_keyboard_walks_the_results_and_enter_opens_the_match(cx: &mut TestAppContext) {
        let text: String = (0..12).map(|n| format!("needle {n}\n")).collect();
        let project = project(cx, &[("notes.txt", text.as_str())]).await;
        let path = project.root.join("notes.txt");
        let (popup, events, cx) = open(cx, project.input());
        cx.simulate_input("needle");
        searched(&popup, cx);
        let row = |n: usize| Some(format!("notes.txt:{} needle {n}", n + 1));
        assert_eq!(selected(&popup, cx), row(0));

        // The preview shows the selected match in its file, with the file's matches marked.
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path().is_some()));
        popup.read_with(cx, |popup, _| {
            assert_eq!(popup.preview_path(), Some(&*path.to_string_lossy()));
            assert_eq!(popup.preview_line(), Some((0, "needle 0".to_owned())));
            assert_eq!(popup.preview_lines().len(), 12);
            assert_eq!((popup.preview_marks(0), popup.preview_marks(3)), (vec![(0, 6)], vec![(0, 6)]));
        });

        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx), row(1));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_line()), Some((1, "needle 1".to_owned())));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(selected(&popup, cx), row(10));
        cx.simulate_keystrokes("pagedown down");
        assert_eq!(selected(&popup, cx), row(11), "the keys stop at the last row");
        cx.simulate_keystrokes("pageup");
        assert_eq!(selected(&popup, cx), row(2));
        cx.simulate_keystrokes("up up up");
        assert_eq!(selected(&popup, cx), row(0), "and at the first");

        cx.simulate_keystrokes("down shift-down shift-down");
        assert_eq!(selected(&popup, cx), row(3));
        assert_eq!(
            popup.read_with(cx, |popup, _| popup.chosen_lines()),
            [row(1), row(2), row(3)].map(|row| row.expect("a row"))
        );
        cx.simulate_keystrokes("down");
        assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_lines().len()), 1);

        // The shortcuts for the next and the previous occurrence go round the ends.
        let run = |cx: &mut VisualTestContext, command: FindPopupCommand| {
            popup.update_in(cx, |popup, window, cx| popup.run_command(command, window, cx));
        };
        run(cx, FindPopupCommand::NextResult);
        assert_eq!(selected(&popup, cx), row(5));
        cx.simulate_keystrokes("pagedown");
        run(cx, FindPopupCommand::NextResult);
        assert_eq!(selected(&popup, cx), row(0));
        run(cx, FindPopupCommand::PreviousResult);
        assert_eq!(selected(&popup, cx), row(11));

        let open = |side: bool, keep_open: bool| {
            Some(FindPopupEvent::Open { path: path.clone(), line: 11, col: 0, end_col: 6, side, keep_open })
        };
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), open(false, false));
        cx.simulate_keystrokes("shift-enter");
        assert_eq!(last(&events), open(true, false));
        run(cx, FindPopupCommand::OpenResult);
        assert_eq!((events.borrow().len(), last(&events)), (3, open(false, false)));

        // A pinned popup stays open when a result is opened: what it searched for is remembered at once.
        click(cx, "find-popup-pin", Modifiers::default());
        assert!(popup.read_with(cx, |popup, _| popup.pinned()));
        let pinned = cx.update(|_, cx| cx.global::<UiStorage>().get(PINNED_KEY).map(str::to_owned));
        assert_eq!(pinned.as_deref(), Some("true"));
        assert!(stored_list(cx, HistoryKind::Find.storage_key()).is_empty());
        cx.simulate_keystrokes("enter");
        assert_eq!(last(&events), open(false, true));
        assert_eq!(stored_list(cx, HistoryKind::Find.storage_key()), ["needle"]);
        close_project(cx);
    }

    /// A click selects a row, with ⇧ up to it and with the primary modifier besides the others; the preview
    /// reads the file of the row it moves to; a double click asks for the match to be opened.
    #[gpui_kit::test]
    async fn the_mouse_selects_rows_and_the_preview_follows(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "needle a1\nneedle a2\n"), ("b.txt", "x\nneedle b\n")]).await;
        let (popup, events, cx) = open(cx, project.input());
        cx.simulate_input("needle");
        searched(&popup, cx);
        let rows = lines(&popup, cx);
        assert_eq!(rows.len(), 3);
        let b = rows.iter().position(|row| row.starts_with("b.txt")).expect("the row of b.txt");
        let other = if b == 0 { 2 } else { 0 };

        click(cx, ROWS[b], Modifiers::default());
        assert_eq!(selected(&popup, cx).as_ref(), Some(&rows[b]));
        let shown = project.root.join("b.txt").to_string_lossy().into_owned();
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_path() == Some(shown.as_str())));
        popup.read_with(cx, |popup, _| {
            assert_eq!(popup.preview_lines(), ["x", "needle b"]);
            assert_eq!(popup.preview_line(), Some((1, "needle b".to_owned())));
            assert_eq!((popup.preview_marks(0), popup.preview_marks(1)), (Vec::<(u32, u32)>::new(), vec![(0, 6)]));
        });

        let primary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        click(cx, ROWS[other], primary);
        assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_lines().len()), 2);
        click(cx, ROWS[1], Modifiers::default());
        click(cx, ROWS[2], Modifiers { shift: true, ..Modifiers::default() });
        assert_eq!(popup.read_with(cx, |popup, _| popup.chosen_lines()), rows[1..]);
        assert!(events.borrow().is_empty(), "selecting opens nothing");

        double_click(cx, ROWS[0]);
        let (name, rest) = rows[0].split_once(':').expect("a row names its file");
        let line: u32 = rest.split_once(' ').and_then(|(line, _)| line.parse().ok()).expect("and its line");
        assert_eq!(
            last(&events),
            Some(FindPopupEvent::Open {
                path: project.root.join(name),
                line: line - 1,
                col: 0,
                end_col: 6,
                side: false,
                keep_open: false
            })
        );
        close_project(cx);
    }

    /// Open in Find Window hands the search over without the popup's limit, titled by what it looks for and
    /// where; "Open results in new tab" is kept between sessions.
    #[gpui_kit::test]
    async fn open_in_find_window_hands_the_search_over(cx: &mut TestAppContext) {
        init(cx);
        let (popup, events, cx) = open(cx, window_input("/p"));
        // Without a text to find there is nothing to hand over.
        cx.simulate_keystrokes("cmd-enter");
        assert!(events.borrow().is_empty());

        cx.simulate_input("needle");
        assert_eq!(query(&popup, cx).max_matches, POPUP_MAX_MATCHES);
        cx.simulate_keystrokes("cmd-enter");
        let handed = |options: OpenOptions| {
            Some(FindPopupEvent::OpenInFindWindow {
                query: SearchQuery { pattern: "needle".to_owned(), ..SearchQuery::default() },
                title: "'needle' in Project".to_owned(),
                options,
            })
        };
        assert_eq!(last(&events), handed(OpenOptions::default()));

        click(cx, "find-popup-new-tab", Modifiers::default());
        let new_tab = cx.update(|_, cx| cx.global::<UiStorage>().get(NEW_TAB_KEY).map(str::to_owned));
        assert_eq!(new_tab.as_deref(), Some("true"));
        click(cx, "find-popup-open-in-find-window", Modifiers::default());
        assert_eq!(last(&events), handed(OpenOptions { new_tab: true, ..OpenOptions::default() }));

        // In Replace the replacement and Preserve Case go with it.
        popup.update_in(cx, |popup, window, cx| popup.show(true, &FindArg::default(), None, window, cx));
        popup.update(cx, |popup, cx| popup.set_replacement("pin", cx));
        cx.simulate_keystrokes("alt-e alt-b cmd-enter");
        let options = OpenOptions { replacement: Some("pin".to_owned()), preserve_case: true, new_tab: false };
        assert_eq!(last(&events), handed(options));
        assert_eq!(events.borrow().len(), 3);
    }

    /// The recent searches are listed from the field's history button and with the Recent Searches shortcut;
    /// the arrows and Enter choose one, Escape closes the list before it closes the popup.
    #[gpui_kit::test]
    async fn the_history_lists_the_recent_searches_and_replacements(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| {
            let recent = r#"["old query","older"]"#.to_owned();
            cx.global_mut::<UiStorage>().set(HistoryKind::Find.storage_key(), recent);
        });
        let (popup, events, cx) = open(cx, FindPopupInput { replace: true, ..window_input("/p") });
        let menu = |cx: &mut VisualTestContext| popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines()));
        let text = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, cx| popup.pattern_field().read(cx).text().to_owned())
        };

        click(cx, "find-popup-history", Modifiers::default());
        let recent = ["Recent Searches", "old query", "older"].map(str::to_owned).to_vec();
        assert_eq!(menu(cx), (Some(FindMenu::History(HistoryKind::Find)), recent.clone()));
        cx.simulate_keystrokes("down enter");
        assert_eq!((menu(cx).0, text(cx)), (None, "older".to_owned()));
        assert_eq!(query(&popup, cx).pattern, "older");

        // Escape closes the list, and only the list.
        popup.update_in(cx, |popup, window, cx| popup.run_command(FindPopupCommand::RecentSearches, window, cx));
        assert_eq!(menu(cx), (Some(FindMenu::History(HistoryKind::Find)), recent));
        cx.simulate_keystrokes("escape");
        assert_eq!((menu(cx).0, events.borrow().len()), (None, 0));
        click(cx, "find-popup-history", Modifiers::default());
        click(cx, "find-popup-menu-1", Modifiers::default());
        assert_eq!((menu(cx).0, text(cx)), (None, "old query".to_owned()));

        // With the keyboard in the replacement, the shortcut lists the recent replacements.
        cx.simulate_keystrokes("tab");
        popup.update_in(cx, |popup, window, cx| popup.run_command(FindPopupCommand::RecentSearches, window, cx));
        assert_eq!(
            menu(cx),
            (Some(FindMenu::History(HistoryKind::Replace)), vec!["No recent replacements".to_owned()])
        );
        cx.simulate_keystrokes("escape escape");
        assert_eq!(last(&events), Some(FindPopupEvent::Dismissed));
    }

    /// A popup that goes away remembers what it searched for and replaced with, and the next one of its
    /// window opens with the options it was left with.
    #[gpui_kit::test]
    async fn a_closed_popup_is_remembered_by_the_next_one(cx: &mut TestAppContext) {
        init(cx);
        let first = cx.update(|cx| {
            gpui_kit::AppContext::new(cx, |cx| {
                let arg = FindArg { query: Some("needle".to_owned()), directory: Some("/p/lib".to_owned()) };
                let mut popup = FindPopup::new(FindPopupInput { replace: true, arg, ..window_input("/p") }, cx);
                popup.toggle(FindToggle::Words, cx);
                popup.set_replacement("pin", cx);
                popup
            })
        });
        // The popup's window lets go of it.
        drop(first);
        cx.update(|_| {});
        let history = |cx: &mut TestAppContext, kind: HistoryKind| {
            cx.update(|cx| find_history(cx.global::<UiStorage>().get(kind.storage_key()).unwrap_or_default()))
        };
        assert_eq!(history(cx, HistoryKind::Find), ["needle"]);
        assert_eq!(history(cx, HistoryKind::Replace), ["pin"]);

        // The selection of the editor is what the next one looks for; the rest is as it was left.
        let again = FindPopupInput { selection: Some("thread".to_owned()), ..window_input("/p") };
        let (popup, _, cx) = open(cx, again);
        popup.read_with(cx, |popup, cx| {
            assert!(!popup.is_replace());
            assert_eq!(popup.pattern_field().read(cx).selected_text(), "thread");
            assert_eq!(popup.options().replacement, "pin");
            assert!(popup.query().whole_word);
            assert_eq!(popup.query().scope, SearchScope::Directory { path: "/p/lib".to_owned() });
            assert_eq!(popup.directory_field().read(cx).text(), "lib");
        });
        // Typing replaces the selected query.
        cx.simulate_input("x");
        assert_eq!(query(&popup, cx).pattern, "x");

        // In another project the directory of this one is not searched.
        let (elsewhere, _, cx) = open(cx, window_input("/q"));
        assert_eq!(query(&elsewhere, cx).scope, SearchScope::Project);
    }

    /// Replace: Enter replaces the selected row's occurrences in its file and moves to the next row with
    /// some left; ⇧Enter opens the match instead.
    #[gpui_kit::test]
    async fn replace_replaces_the_selected_row_and_moves_on(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "foo one\nfoo two foo\nbar\n")]).await;
        let (popup, events, cx) = open(cx, FindPopupInput { replace: true, ..project.input() });
        cx.simulate_input("foo");
        popup.update(cx, |popup, cx| popup.set_replacement("qux", cx));
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx), ["a.txt:1 foo one", "a.txt:2 foo two foo"]);
        assert_eq!(popup.update(cx, |popup, cx| popup.replace_label(cx)), "Replace");

        let replaced = |cx: &mut VisualTestContext| popup.read_with(cx, |popup, _| popup.replaced_lines());
        cx.simulate_keystrokes("enter");
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_replacing()) && replaced(cx).len() == 1);
        assert_eq!(project.read("a.txt"), "qux one\nfoo two foo\nbar\n");
        assert_eq!(replaced(cx), ["a.txt:1 foo one"]);
        assert_eq!(selected(&popup, cx).as_deref(), Some("a.txt:2 foo two foo"));
        let told = |title: &str| {
            Some(FindPopupEvent::Notify(NotificationRequest {
                body: Some("a.txt".to_owned()),
                key: Some("search.replace".to_owned()),
                group: Some("search".to_owned()),
                ..NotificationRequest::new(NotificationLevel::Info, title)
            }))
        };
        assert_eq!(last(&events), told("Replaced 1 occurrence in 1 file"));

        // Both occurrences of the second row go together.
        assert_eq!(popup.update(cx, |popup, cx| popup.replace_label(cx)), "Replace 2 Occurrences");
        click(cx, "find-popup-replace", Modifiers::default());
        settle(cx, |cx| popup.read_with(cx, |popup, _| !popup.is_replacing()) && replaced(cx).len() == 2);
        assert_eq!(project.read("a.txt"), "qux one\nqux two qux\nbar\n");
        assert_eq!(last(&events), told("Replaced 2 occurrences in 1 file"));
        // The preview shows the file as it is now.
        settle(cx, |cx| popup.read_with(cx, |popup, _| popup.preview_line().is_some()));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_line()), Some((1, "qux two qux".to_owned())));

        // Nothing is left to replace; ⇧Enter opens the match.
        cx.simulate_keystrokes("enter");
        assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()));
        cx.simulate_keystrokes("shift-enter");
        assert!(matches!(last(&events), Some(FindPopupEvent::Open { line: 1, side: true, .. })));
        close_project(cx);
    }

    /// Replace All asks before it replaces in more than one file, replaces everything listed once the user
    /// agrees, and closes the popup.
    #[gpui_kit::test]
    async fn replace_all_asks_first_when_several_files_change(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "foo\n"), ("b.txt", "foo foo\n")]).await;
        let (popup, events, cx) = open(cx, FindPopupInput { replace: true, ..project.input() });
        cx.simulate_input("foo");
        popup.update(cx, |popup, cx| popup.set_replacement("bar", cx));
        searched(&popup, cx);
        assert_eq!(lines(&popup, cx).len(), 2);

        cx.simulate_keystrokes("shift-alt-enter");
        let asked = ("Replace All".to_owned(), "Replace 3 occurrences of 'foo' with 'bar' in 2 files?".to_owned());
        assert_eq!(cx.pending_prompt(), Some(asked));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert!(!popup.read_with(cx, |popup, _| popup.is_replacing()));
        assert_eq!((project.read("a.txt"), project.read("b.txt")), ("foo\n".to_owned(), "foo foo\n".to_owned()));

        click(cx, "find-popup-replace-all", Modifiers::default());
        assert!(cx.has_pending_prompt());
        cx.simulate_prompt_answer("Replace");
        settle(cx, |_| last(&events) == Some(FindPopupEvent::Dismissed));
        assert_eq!((project.read("a.txt"), project.read("b.txt")), ("bar\n".to_owned(), "bar bar\n".to_owned()));
        let told = events.borrow().iter().any(|event| {
            matches!(event, FindPopupEvent::Notify(request)
                if request.title == "Replaced 3 occurrences in 2 files" && request.body.is_none())
        });
        assert!(told, "the user is told what was replaced");
        close_project(cx);
    }

    /// Within one file Replace All replaces at once, with ⌥A as with its button.
    #[gpui_kit::test]
    async fn replace_all_in_one_file_does_not_ask(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "foo\nother\nfoo\n")]).await;
        let (popup, events, cx) = open(cx, FindPopupInput { replace: true, ..project.input() });
        cx.simulate_input("foo");
        popup.update(cx, |popup, cx| popup.set_replacement("bar", cx));
        searched(&popup, cx);
        cx.simulate_keystrokes("alt-a");
        assert!(!cx.has_pending_prompt());
        settle(cx, |_| last(&events) == Some(FindPopupEvent::Dismissed));
        assert_eq!(project.read("a.txt"), "bar\nother\nbar\n");
        close_project(cx);
    }

    /// Replace's Preview shows what replacing the selected row changes, and follows the replacement typed.
    #[gpui_kit::test]
    async fn the_replace_preview_shows_the_change(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "Foo one\nbar\n")]).await;
        let (popup, _, cx) = open(cx, FindPopupInput { replace: true, ..project.input() });
        cx.simulate_input("foo");
        popup.update(cx, |popup, cx| popup.set_replacement("qux", cx));
        searched(&popup, cx);
        let diff = |cx: &mut VisualTestContext| popup.read_with(cx, |popup, _| popup.diff_lines());
        assert!(diff(cx).is_empty());

        click(cx, "find-popup-preview-mode", Modifiers::default());
        assert!(popup.read_with(cx, |popup, _| popup.preview_mode()));
        settle(cx, |cx| !diff(cx).is_empty());
        assert_eq!(diff(cx), ["-1 Foo one", "+1 qux one"]);

        // Preserve Case, with ⌥E, gives the replacement the case of what it replaces.
        cx.simulate_keystrokes("alt-e");
        settle(cx, |cx| diff(cx) == ["-1 Foo one", "+1 Qux one"]);
        popup.update(cx, |popup, cx| popup.set_replacement("zap", cx));
        settle(cx, |cx| diff(cx) == ["-1 Foo one", "+1 Zap one"]);
        assert_eq!(project.read("a.txt"), "Foo one\nbar\n", "a preview changes nothing");
        close_project(cx);
    }

    /// A search that finds nothing names the options it was made with and offers to drop them, or to search
    /// with Regex when the text could be an expression.
    #[gpui_kit::test]
    async fn empty_results_say_which_options_were_used(cx: &mut TestAppContext) {
        let project = project(cx, &[("a.txt", "hay\n")]).await;
        let (popup, _, cx) = open(cx, project.input());
        cx.simulate_input("zzz");
        assert_eq!(empty_text(&popup, cx), ["Searching…"]);
        searched(&popup, cx);
        assert_eq!(empty_text(&popup, cx), ["Nothing found"]);

        cx.simulate_keystrokes("alt-c");
        searched(&popup, cx);
        assert_eq!(empty_text(&popup, cx), ["Nothing found.", "Search option used: Match Case"]);
        cx.simulate_keystrokes("alt-w");
        searched(&popup, cx);
        let used = format!("Match Case {}  Words {}", key_label("Alt+C"), key_label("Alt+W"));
        assert_eq!(empty_text(&popup, cx), ["Nothing found.".to_owned(), "Search options used:".to_owned(), used]);
        click(cx, "find-popup-empty-link", Modifiers::default());
        let cleared = query(&popup, cx);
        assert!(!cleared.case_sensitive && !cleared.whole_word);
        searched(&popup, cx);
        assert_eq!(empty_text(&popup, cx), ["Nothing found"]);

        popup.update(cx, |popup, cx| popup.set_pattern("z+y", cx));
        searched(&popup, cx);
        assert_eq!(empty_text(&popup, cx), ["Nothing found."]);
        click(cx, "find-popup-empty-link", Modifiers::default());
        assert!(query(&popup, cx).is_regex);
        close_project(cx);
    }

    /// The search context limits the matches to comments or string literals, or keeps them out of those;
    /// it counts as an option of the search.
    #[gpui_kit::test]
    async fn the_search_context_says_where_a_match_may_lie(cx: &mut TestAppContext) {
        init(cx);
        let (popup, _, cx) = open(cx, window_input("/p"));
        assert_eq!(query(&popup, cx).context, SearchContext::Any);
        click(cx, "find-popup-context", Modifiers::default());
        assert_eq!(
            popup.read_with(cx, |popup, _| (popup.menu(), popup.menu_lines())),
            (Some(FindMenu::Context), SEARCH_CONTEXTS.map(|(_, label)| label.to_owned()).to_vec())
        );
        // The list opens on the context in use; ↓ and Enter choose the one after it.
        cx.simulate_keystrokes("down enter");
        assert_eq!(query(&popup, cx).context, SearchContext::InComments);
        click(cx, "find-popup-context", Modifiers::default());
        click(cx, "find-popup-menu-3", Modifiers::default());
        assert_eq!(query(&popup, cx).context, SearchContext::ExceptComments);

        popup.update(cx, |popup, cx| popup.set_mask("*.rs", cx));
        let used = format!("File mask {}  Filter", key_label("Alt+K"));
        assert_eq!(
            empty_text(&popup, cx),
            ["No search text entered.".to_owned(), "Search options used:".to_owned(), used]
        );
        cx.simulate_keystrokes("shift-alt-backspace");
        let cleared = query(&popup, cx);
        assert_eq!((cleared.context, cleared.include.is_empty()), (SearchContext::Any, true));
    }

    /// The fields hold one line: a selection, a query or a remembered string of several lines is taken with
    /// spaces for its line breaks.
    #[gpui_kit::test]
    async fn a_text_of_several_lines_is_taken_on_one(cx: &mut TestAppContext) {
        init(cx);
        cx.update(|cx| {
            let recent = r#"["first\nsecond"]"#.to_owned();
            cx.global_mut::<UiStorage>().set(HistoryKind::Find.storage_key(), recent);
        });
        let selection = Some("let a = 1;\r\n    let b = 2;".to_owned());
        let (popup, _, cx) = open(cx, FindPopupInput { selection, ..window_input("/p") });
        let text = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, cx| {
                (popup.pattern_field().read(cx).text().to_owned(), popup.query().pattern.clone())
            })
        };
        let both = |text: &str| (text.to_owned(), text.to_owned());
        assert_eq!(text(cx), both("let a = 1;     let b = 2;"));
        popup.update(cx, |popup, cx| popup.set_pattern("one\ntwo", cx));
        assert_eq!(text(cx), both("one two"));
        popup.update(cx, |popup, cx| {
            popup.open_menu(FindMenu::History(HistoryKind::Find), cx);
            assert_eq!(popup.menu_lines(), ["Recent Searches", "first second"]);
            popup.choose_menu_item(1, cx);
        });
        assert_eq!(text(cx), both("first second"));
    }

    /// Invoked on the open popup, Find and Replace show it again: with the query they bring selected, in
    /// the directory they name.
    #[gpui_kit::test]
    async fn showing_the_open_popup_again_takes_the_new_query_and_directory(cx: &mut TestAppContext) {
        init(cx);
        let (popup, _, cx) = open(cx, window_input("/p"));
        cx.simulate_input("first");
        cx.simulate_keystrokes("tab");
        let arg = FindArg { query: Some("second".to_owned()), directory: Some("/p/lib".to_owned()) };
        popup.update_in(cx, |popup, window, cx| popup.show(true, &arg, Some("ignored"), window, cx));
        popup.read_with(cx, |popup, cx| {
            assert!(popup.is_replace());
            assert_eq!(popup.pattern_field().read(cx).selected_text(), "second");
            assert_eq!(popup.query().pattern, "second");
            assert_eq!(popup.query().scope, SearchScope::Directory { path: "/p/lib".to_owned() });
        });
        assert!(cx.update(|window, cx| popup.read(cx).pattern_field().focus_handle(cx).is_focused(window)));
    }

    // Not from a spec: every design token and icon the popup draws with is defined.
    #[test]
    fn the_design_tokens_and_icons_the_popup_uses_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-faint",
            "fg-gutter",
            "fg-button-primary",
            "fg-search-match",
            "bg-popup",
            "bg-app",
            "bg-field",
            "bg-input",
            "bg-hover",
            "bg-tree-selected",
            "bg-toolbar-pressed",
            "bg-toolbar-hover",
            "bg-search-match",
            "bg-search-match-current",
            "bg-current-line",
            "bg-checkbox",
            "bg-button",
            "bg-button-primary",
            "border",
            "border-popup",
            "border-strong",
            "border-button",
            "border-checkbox",
            "diff-inserted",
            "diff-deleted",
            "danger",
            "link",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "fs-small", "h-row", "h-control", "h-button", "radius-small"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
        assert!(!theme.font_families("font-mono").is_empty());
        let icons = [
            "search",
            "search-history",
            "case",
            "word",
            "regex",
            "replace",
            "preserve-case",
            "filter",
            "pin",
            "check",
            "chevron-down",
            "folder-open",
            "more-horizontal",
        ];
        for name in icons {
            assert!(theme::icons::icon_path(name).is_some(), "{name} is an icon");
        }
    }

    // Not from a spec: a line is cut at its matches' UTF-16 columns, and a match inside another is left out.
    #[test]
    fn a_line_is_cut_into_its_matched_and_other_pieces() {
        let piece = |text: &str, matched: bool| (text.to_owned(), matched);
        assert_eq!(
            segments("a needle here", &[(2, 8)]),
            [piece("a ", false), piece("needle", true), piece(" here", false)]
        );
        assert_eq!(segments("needle", &[(0, 6)]), [piece("needle", true)]);
        assert_eq!(segments("😀ab", &[(2, 3)]), [piece("😀", false), piece("a", true), piece("b", false)]);
        assert_eq!(
            segments("abcd", &[(0, 2), (1, 3), (3, 4)]),
            [piece("ab", true), piece("c", false), piece("d", true)]
        );
        assert_eq!(segments("plain", &[]), [piece("plain", false)]);
        assert!(segments("", &[]).is_empty());
    }

    // Not from a spec: the arrows and the page keys stop at the ends of the list.
    #[test]
    fn navigation_keys_stop_at_the_ends_of_the_list() {
        assert_eq!(list_navigate("down", 3, 0), Some(1));
        assert_eq!(list_navigate("down", 3, 2), Some(2));
        assert_eq!(list_navigate("up", 3, 0), Some(0));
        assert_eq!(list_navigate("up", 3, 2), Some(1));
        assert_eq!(list_navigate("pagedown", 25, 3), Some(12));
        assert_eq!(list_navigate("pagedown", 25, 20), Some(24));
        assert_eq!(list_navigate("pageup", 25, 12), Some(3));
        assert_eq!(list_navigate("pageup", 25, 4), Some(0));
        assert_eq!(list_navigate("home", 3, 1), None);
        assert_eq!(list_navigate("down", 0, 0), None);
    }

    // Not from a spec: a directory in the project shows relative to it, and only text with expression syntax
    // that compiles is offered to Regex.
    #[test]
    fn directories_show_relative_and_expressions_are_recognised() {
        assert_eq!(display_directory("/p/src/lib", Some("/p")), "src/lib");
        assert_eq!(display_directory("/p", Some("/p")), "");
        assert_eq!(display_directory("/pother/src", Some("/p")), "/pother/src");
        assert_eq!(display_directory("/q/src", Some("/p")), "/q/src");
        assert_eq!(display_directory("src", Some("/p")), "src");
        assert_eq!(display_directory("/p/src", None), "/p/src");
        assert!(could_be_regex("a.b") && could_be_regex("x+") && could_be_regex("(a|b)"));
        assert!(!could_be_regex("plain") && !could_be_regex("(") && !could_be_regex("a[b"));
    }
}
