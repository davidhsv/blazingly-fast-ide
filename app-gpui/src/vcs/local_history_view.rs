//! The Local History tab of a file or a folder: the revisions the backend recorded (saves, external
//! changes, deletions), the labels and system events ("Before Git checkout") newest first, and the
//! diff of the revision chosen against the current file. Several revisions of one file selected with
//! Shift- or Cmd-click are diffed against each other. Revert (Restore for a deleted file) writes the
//! chosen revision back, or at a label every file's last revision before it.
//!
//! The view reads nothing itself: [`LocalHistoryView::load`] asks its window for the history
//! ([`LocalHistoryEvent::Load`]), which answers with [`LocalHistoryView::set_history`]; a diff to show
//! is asked for with [`LocalHistoryEvent::LoadDiff`] and arrives through [`LocalHistoryView::diff_loaded`].

use crate::editor::tabs::EditorInput;
use crate::platform::date_format::DateFormatOptions;
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::input;
use crate::platform::local_time::{now_ms, utc_offset_minutes};
use crate::platform::virtual_docs::relative_path;
use crate::theme;
use crate::vcs::changes::base_name;
use crate::vcs_diff::diff_input::{DiffData, DiffSide, diff_key, side_label};
use crate::vcs_diff::diff_options::DiffViewOptions;
use crate::vcs_diff::model::format_time;
use crate::vcs_diff::view::DiffView;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Keystroke, Modifiers, Render,
    SharedString, Window, div, prelude::*, px,
};
use ide_core::ipc_path::to_ipc;
use ide_workspace::{DirectoryHistory, HistoryEntry, HistoryKind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// The editor kind of Local History tabs.
pub const LOCAL_HISTORY_KIND: &str = "localHistory";

const SIDEBAR_WIDTH: f32 = 300.0;

/// The tab input of the Local History of `path`.
pub fn local_history_input(path: &str) -> EditorInput {
    EditorInput {
        kind: LOCAL_HISTORY_KIND.to_owned(),
        key: format!("localHistory:{path}"),
        path: Some(path.to_owned()),
        data: Some(json!({ "path": path })),
    }
}

/// The file or folder a Local History tab shows.
pub fn local_history_path(input: &EditorInput) -> Option<String> {
    if input.kind != LOCAL_HISTORY_KIND {
        return None;
    }
    let path = input.data.as_ref().and_then(|data| data.get("path")).and_then(Value::as_str);
    path.map(str::to_owned).or_else(|| input.path.clone()).filter(|path| !path.is_empty())
}

/// "Local History: <name>".
pub fn local_history_title(input: &EditorInput) -> String {
    let path = local_history_path(input).unwrap_or_default();
    format!("Local History: {}", base_name(&path))
}

pub fn local_history_icon(_: &EditorInput) -> String {
    "clock".to_owned()
}

/// A revision or a label, as the list shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct Revision {
    /// The stored content's id; empty for a label.
    pub id: String,
    /// Absolute path of the file (the labelled folder for a label).
    pub path: String,
    pub timestamp_ms: f64,
    /// "Saved", "External change", "Deleted", or the label's name.
    pub label: String,
    pub size: u64,
    pub is_label: bool,
    /// When it was recorded, as lists show times; set once the history is read.
    pub time: String,
}

impl From<&HistoryEntry> for Revision {
    fn from(entry: &HistoryEntry) -> Self {
        Self {
            id: entry.id.clone(),
            path: to_ipc(&entry.path),
            timestamp_ms: entry.timestamp_ms,
            label: entry.label.clone(),
            size: entry.size,
            is_label: entry.kind == HistoryKind::Label,
            time: String::new(),
        }
    }
}

/// Per file, the newest snapshot recorded before `at`: the state of every file at a label.
pub fn snapshots_before(entries: &[Revision], at: f64) -> BTreeMap<String, Revision> {
    let mut out: BTreeMap<String, Revision> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| !entry.is_label && entry.timestamp_ms < at) {
        let newer = out.get(&entry.path).is_none_or(|known| known.timestamp_ms < entry.timestamp_ms);
        if newer {
            out.insert(entry.path.clone(), entry.clone());
        }
    }
    out
}

/// Several revisions selected: the oldest and newest of them when they are snapshots of one file,
/// to diff against each other. `entries` are newest first; the result is `(older, newer)`.
pub fn revision_range<'a>(
    entries: &'a [Revision],
    selected: impl IntoIterator<Item = usize>,
) -> Option<(&'a Revision, &'a Revision)> {
    let mut indices: Vec<usize> = selected.into_iter().collect();
    indices.sort_unstable();
    indices.dedup();
    let picked: Vec<&Revision> =
        indices.into_iter().filter_map(|index| entries.get(index)).filter(|entry| !entry.is_label).collect();
    let (first, last) = (picked.first()?, picked.last()?);
    if picked.len() < 2 || picked.iter().any(|entry| entry.path != first.path) {
        return None;
    }
    Some((*last, *first))
}

/// The latest events of the project's Local History, newest first, at most `limit`: consecutive
/// revisions of one file with the same label collapse into the newest (typing saves often).
pub fn recent_changes(entries: &[Revision], limit: usize) -> Vec<Revision> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| b.timestamp_ms.total_cmp(&a.timestamp_ms));
    let mut out: Vec<Revision> = Vec::new();
    for entry in sorted {
        let repeated = out.last().is_some_and(|previous| {
            !previous.is_label && !entry.is_label && previous.path == entry.path && previous.label == entry.label
        });
        if repeated {
            continue;
        }
        out.push(entry);
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// A row of Recent Changes: a label's name, or the file below the project with what happened to it.
pub fn recent_change_label(entry: &Revision, root: &str) -> String {
    if entry.is_label {
        return entry.label.clone();
    }
    let path = relative_path(root, &entry.path);
    if entry.label.is_empty() { path.into_owned() } else { format!("{path} — {}", entry.label) }
}

/// A size as the list shows it.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
    }
}

/// When a revision was recorded, as lists show times.
fn recorded_at(ms: f64, i18n: &mut I18n) -> String {
    let now = now_ms();
    format_time(Some(ms), now, utc_offset_minutes(now), &DateFormatOptions::default(), i18n)
}

/// A row of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryRow {
    pub time: String,
    /// The file below the project, in a folder's history.
    pub path: Option<String>,
    /// A label's name, or "<what> · <size>" for a revision.
    pub text: String,
    pub is_label: bool,
    /// The file of the revision no longer exists.
    pub deleted: bool,
    /// The row the caret is on.
    pub active: bool,
    /// Selected besides the active row.
    pub selected: bool,
}

/// What the right side shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Detail {
    /// A message in the UI language.
    Message(String),
    Diff(Box<DiffData>),
}

/// What the view asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum LocalHistoryEvent {
    /// Read the Local History of this path (`history_list_dir`) and call [`LocalHistoryView::set_history`].
    Load(String),
    /// Load both sides of this diff and call [`LocalHistoryView::diff_loaded`].
    LoadDiff(Box<DiffData>),
    /// Ask `question`; once confirmed, restore every target (`history_restore`) and load again.
    Revert { targets: Vec<Revision>, question: String },
    /// Raise this informational notice.
    Notify(&'static str),
}

/// The history as read.
enum Loaded {
    Loading,
    Failed(String),
    Shown { entries: Vec<Revision>, deleted: HashSet<String>, folder: bool },
}

pub struct LocalHistoryView {
    path: String,
    root: Option<String>,
    loaded: Loaded,
    active: usize,
    /// Rows selected besides the active one: Cmd/Ctrl-click adds, Shift-click extends from `anchor`.
    extra: BTreeSet<usize>,
    anchor: usize,
    detail: Detail,
    diff: Entity<DiffView>,
    /// The key of the diff whose texts the viewer shows.
    diff_shown: Option<String>,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<LocalHistoryEvent> for LocalHistoryView {}

impl LocalHistoryView {
    /// The Local History of `path`, its diffs under the viewer `options`. Nothing is read until
    /// [`Self::load`].
    pub fn new(path: String, options: DiffViewOptions, cx: &mut Context<Self>) -> Self {
        let diff = cx.new(|cx| DiffView::new("", "", options, cx));
        let focus = cx.focus_handle();
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| claimant.update(cx, |view, cx| view.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self {
            path,
            root: None,
            loaded: Loaded::Loading,
            active: 0,
            extra: BTreeSet::new(),
            anchor: 0,
            detail: Detail::Message(String::new()),
            diff,
            diff_shown: None,
            focus,
            key_claim,
        }
    }

    /// Asks for the history (again).
    pub fn load(&mut self, cx: &mut Context<Self>) {
        cx.emit(LocalHistoryEvent::Load(self.path.clone()));
    }

    /// The history arrived, or why it could not be read. `root` is the project's folder.
    pub fn set_history(
        &mut self,
        history: Result<DirectoryHistory, String>,
        root: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.root = root;
        self.loaded = match history {
            Err(error) => Loaded::Failed(error),
            Ok(history) => {
                let mut entries: Vec<Revision> = history.entries.iter().map(Revision::from).collect();
                entries.sort_by(|a, b| b.timestamp_ms.total_cmp(&a.timestamp_ms));
                let i18n = &mut cx.default_global::<Localization>().0;
                for entry in &mut entries {
                    entry.time = recorded_at(entry.timestamp_ms, i18n);
                }
                let folder = entries.iter().any(|entry| !entry.is_label && entry.path != self.path);
                let deleted = history.deleted.iter().map(|path| to_ipc(&path.0)).collect();
                Loaded::Shown { entries, deleted, folder }
            }
        };
        self.extra.clear();
        let len = self.entries().len();
        self.active = self.active.min(len.saturating_sub(1));
        self.anchor = self.active;
        self.refresh_detail(cx);
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// The revisions and labels, newest first; none until they are read.
    pub fn entries(&self) -> &[Revision] {
        match &self.loaded {
            Loaded::Shown { entries, .. } => entries,
            Loaded::Loading | Loaded::Failed(_) => &[],
        }
    }

    /// The history holds revisions of files other than the path: a folder's history.
    pub fn is_folder(&self) -> bool {
        matches!(self.loaded, Loaded::Shown { folder: true, .. })
    }

    pub fn error(&self) -> Option<&str> {
        match &self.loaded {
            Loaded::Failed(error) => Some(error),
            Loaded::Loading | Loaded::Shown { .. } => None,
        }
    }

    fn is_deleted(&self, path: &str) -> bool {
        matches!(&self.loaded, Loaded::Shown { deleted, .. } if deleted.contains(path))
    }

    /// The row the caret is on.
    pub fn current(&self) -> Option<&Revision> {
        let entries = self.entries();
        entries.get(self.active.min(entries.len().saturating_sub(1)))
    }

    pub fn active(&self) -> usize {
        self.active
    }

    /// The rows as the list shows them.
    pub fn rows(&self) -> Vec<HistoryRow> {
        let folder = self.is_folder();
        let root = self.root.clone().unwrap_or_default();
        self.entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| HistoryRow {
                time: entry.time.clone(),
                path: (folder && !entry.is_label).then(|| {
                    let shown = relative_path(&root, &entry.path);
                    if shown.is_empty() { base_name(&entry.path).to_owned() } else { shown.into_owned() }
                }),
                text: if entry.is_label {
                    entry.label.clone()
                } else {
                    format!("{} · {}", entry.label, format_size(entry.size))
                },
                is_label: entry.is_label,
                deleted: !entry.is_label && self.is_deleted(&entry.path),
                active: index == self.active,
                selected: self.extra.contains(&index),
            })
            .collect()
    }

    /// What the right side shows.
    pub fn detail(&self) -> &Detail {
        &self.detail
    }

    /// The viewer of the detail's diff.
    pub fn diff_viewer(&self) -> &Entity<DiffView> {
        &self.diff
    }

    /// "Restore" for a revision of a deleted file, else "Revert" (English).
    pub fn revert_label(&self) -> &'static str {
        match self.current() {
            Some(current) if !current.is_label && self.is_deleted(&current.path) => "Restore",
            _ => "Revert",
        }
    }

    /// Selects row `index` alone (the keyboard, a plain click).
    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.entries().len() {
            return;
        }
        self.active = index;
        self.anchor = index;
        self.extra.clear();
        self.refresh_detail(cx);
    }

    /// A click on row `index`: Shift extends the selection from the anchor, Cmd (Ctrl) adds or
    /// removes rows, a plain click selects the row alone.
    pub fn click(&mut self, index: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        if index >= self.entries().len() {
            return;
        }
        let previous = self.active;
        self.active = index;
        if modifiers.shift {
            let (from, to) = (self.anchor.min(index), self.anchor.max(index));
            self.extra = (from..=to).filter(|row| *row != index).collect();
        } else if modifiers.platform || modifiers.control {
            self.anchor = index;
            if !self.extra.remove(&index) && previous != index {
                self.extra.insert(previous);
            }
        } else {
            self.anchor = index;
            self.extra.clear();
        }
        self.refresh_detail(cx);
    }

    fn history_diff(entry: &Revision, deleted: bool) -> DiffData {
        let left = DiffSide::History { id: entry.id.clone(), label: format!("{} · {}", entry.time, entry.label) };
        let right = if deleted {
            DiffSide::text("", "Deleted")
        } else {
            DiffSide::Worktree { label: Some("Current".to_owned()) }
        };
        DiffData::new(&entry.path, left, right)
    }

    fn compute_detail(&self, i18n: &mut I18n) -> Detail {
        let entries = self.entries();
        let Some(current) = self.current().cloned() else {
            return Detail::Message(i18n.tx("Select a revision"));
        };
        if !self.extra.is_empty() {
            let selected = self.extra.iter().copied().chain([self.active]);
            return match revision_range(entries, selected) {
                Some((older, newer)) => {
                    let label = |entry: &Revision| format!("{} · {}", entry.time, entry.label);
                    let left = DiffSide::History { id: older.id.clone(), label: label(older) };
                    let right = DiffSide::History { id: newer.id.clone(), label: label(newer) };
                    Detail::Diff(Box::new(DiffData::new(&newer.path, left, right)))
                }
                None => Detail::Message(i18n.tx("Select revisions of one file to compare them")),
            };
        }
        if !current.is_label {
            let deleted = self.is_deleted(&current.path);
            return Detail::Diff(Box::new(Self::history_diff(&current, deleted)));
        }
        let at = snapshots_before(entries, current.timestamp_ms);
        if !self.is_folder()
            && let Some(file) = at.get(&self.path)
        {
            let deleted = self.is_deleted(&self.path);
            return Detail::Diff(Box::new(Self::history_diff(file, deleted)));
        }
        let mut message = i18n.tx_built(&["Label '", "'"], &[Param::from(current.label.as_str())]);
        if self.is_folder() && !at.is_empty() {
            let count = if at.len() == 1 {
                i18n.tx_built(&["", " file recorded before it"], &[Param::from(at.len())])
            } else {
                i18n.tx_built(&["", " files recorded before it"], &[Param::from(at.len())])
            };
            message = format!("{message} · {count}");
        }
        Detail::Message(message)
    }

    /// Works out the detail; a diff whose texts are not shown yet is asked for.
    fn refresh_detail(&mut self, cx: &mut Context<Self>) {
        let detail = {
            let i18n = &mut cx.default_global::<Localization>().0;
            self.compute_detail(i18n)
        };
        if let Detail::Diff(data) = &detail {
            let key = diff_key(data);
            if self.diff_shown.as_deref() != Some(key.as_str()) {
                self.diff_shown = None;
                cx.emit(LocalHistoryEvent::LoadDiff(data.clone()));
            }
        }
        self.detail = detail;
        cx.notify();
    }

    /// The texts of the diff `key` arrived; ignored when the detail moved on meanwhile.
    pub fn diff_loaded(&mut self, key: &str, left: &str, right: &str, cx: &mut Context<Self>) {
        let Detail::Diff(data) = &self.detail else { return };
        if diff_key(data) != key {
            return;
        }
        let labels = (side_label(&data.left), side_label(&data.right));
        self.diff.update(cx, |viewer, cx| {
            viewer.set_texts(left, right, cx);
            viewer.set_labels(labels.0, labels.1, cx);
        });
        self.diff_shown = Some(key.to_owned());
        cx.notify();
    }

    /// Revert (Restore): the revision of the caret's row, or at a label every file's last revision
    /// before it, once the window has asked.
    pub fn revert(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current().cloned() else { return };
        if !current.is_label {
            let question = {
                let i18n = &mut cx.default_global::<Localization>().0;
                let values = [
                    Param::from(base_name(&current.path)),
                    Param::from(current.time.as_str()),
                    Param::from(current.label.as_str()),
                ];
                i18n.tx_built(&["Revert ", " to the revision from ", " (", ")?"], &values)
            };
            cx.emit(LocalHistoryEvent::Revert { targets: vec![current], question });
            return;
        }
        let at = snapshots_before(self.entries(), current.timestamp_ms);
        let folder = self.is_folder();
        let targets: Vec<Revision> =
            if folder { at.into_values().collect() } else { at.get(&self.path).cloned().into_iter().collect() };
        if targets.is_empty() {
            cx.emit(LocalHistoryEvent::Notify("Nothing recorded before this label"));
            return;
        }
        let name = Param::from(base_name(&self.path));
        let label = Param::from(current.label.as_str());
        let question = {
            let i18n = &mut cx.default_global::<Localization>().0;
            if folder {
                let values = [Param::from(targets.len()), name, label];
                i18n.tx_built(&["Revert ", " files under ", " to label '", "'?"], &values)
            } else {
                i18n.tx_built(&["Revert ", " to label '", "'?"], &[name, label])
            }
        };
        cx.emit(LocalHistoryEvent::Revert { targets, question });
    }

    /// ↑ and ↓ move the caret, one revision alone selected.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        let len = self.entries().len();
        let next = match keystroke.key.as_str() {
            "down" if self.active + 1 < len => self.active + 1,
            "up" if self.active > 0 => self.active - 1,
            "down" | "up" => self.active,
            "home" => 0,
            "end" => len.saturating_sub(1),
            _ => return false,
        };
        self.select(next, cx);
        true
    }

    fn render_row(&self, index: usize, row: &HistoryRow, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let folder = self.is_folder();
        let selected = row.active || row.selected;
        let main = if row.is_label {
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .text_color(theme::color(cx, "fg-strong"))
                .child(theme::icon(cx, "bookmark", 12.0, muted))
                .child(SharedString::from(row.text.clone()))
        } else {
            div().text_color(muted).child(SharedString::from(row.text.clone()))
        };
        let path = row.path.clone().map(|path| {
            let color = if row.deleted { theme::color(cx, "fg-muted") } else { theme::color(cx, "fg") };
            div().text_color(color).when(row.deleted, |el| el.line_through()).child(SharedString::from(path))
        });
        let selector = format!("local-history-row-{index}");
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .h(px(if folder { 52.0 } else { 38.0 }))
            .px(px(10.0))
            .flex()
            .flex_col()
            .justify_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .when(selected, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(div().text_color(theme::color(cx, "fg")).child(SharedString::from(row.time.clone())))
            .children(path)
            .child(main)
            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                window.focus(&view.focus, cx);
                view.click(index, event.modifiers(), cx);
            }))
    }
}

impl Focusable for LocalHistoryView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for LocalHistoryView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-muted");
        let message = |text: String| div().p(px(12.0)).text_color(muted).child(SharedString::from(text));
        let title = cx.default_global::<Localization>().0.tx("Local History");
        let revert = cx.default_global::<Localization>().0.tx(self.revert_label());
        let enabled = self.current().is_some();
        let button = div()
            .id("local-history-revert")
            .debug_selector(|| "local-history-revert".to_owned())
            .h(px(22.0))
            .px(px(10.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-button"))
            .bg(theme::color(cx, "bg-button"))
            .text_color(theme::color(cx, "fg-strong"))
            .when(!enabled, |el| el.opacity(0.5))
            .child(SharedString::from(revert))
            .when(enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.revert(cx))));
        let header = div()
            .h(theme::length(cx, "h-header"))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(10.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
            .child(button);
        let list = match &self.loaded {
            Loaded::Failed(error) => message(error.clone()).into_any_element(),
            Loaded::Loading => message("…".to_owned()).into_any_element(),
            Loaded::Shown { entries, folder, .. } if entries.is_empty() => {
                let empty =
                    if *folder { "No local history for this folder yet" } else { "No local history for this path yet" };
                message(cx.default_global::<Localization>().0.tx(empty)).into_any_element()
            }
            Loaded::Shown { .. } => {
                let rows = self.rows();
                let drawn: Vec<_> =
                    rows.iter().enumerate().map(|(index, row)| self.render_row(index, row, cx)).collect();
                div()
                    .id("local-history-list")
                    .debug_selector(|| "local-history-list".to_owned())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_y_scroll()
                    .children(drawn)
                    .into_any_element()
            }
        };
        let detail = match &self.detail {
            Detail::Message(text) => message(text.clone()).into_any_element(),
            Detail::Diff(_) => div().size_full().child(self.diff.clone()).into_any_element(),
        };
        div()
            .debug_selector(|| "local-history".to_owned())
            .size_full()
            .flex()
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .w(px(SIDEBAR_WIDTH))
                    .flex_none()
                    .h_full()
                    .flex()
                    .flex_col()
                    .border_r_1()
                    .border_color(theme::color(cx, "border"))
                    .track_focus(&self.focus)
                    .child(header)
                    .child(list),
            )
            .child(div().flex_1().min_w_0().h_full().child(detail))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<LocalHistoryEvent>>>;

    fn id(c: char) -> String {
        std::iter::repeat_n(c, 64).collect()
    }

    fn snap(path: &str, t: f64, label: &str, c: char) -> HistoryEntry {
        HistoryEntry {
            id: id(c),
            path: std::path::PathBuf::from(path).into(),
            timestamp_ms: t,
            label: label.to_owned(),
            size: 3,
            kind: HistoryKind::Snapshot,
        }
    }

    fn label(t: f64, name: &str) -> HistoryEntry {
        HistoryEntry {
            id: String::new(),
            path: std::path::PathBuf::from("/p").into(),
            timestamp_ms: t,
            label: name.to_owned(),
            size: 0,
            kind: HistoryKind::Label,
        }
    }

    fn revisions(entries: &[HistoryEntry]) -> Vec<Revision> {
        entries.iter().map(Revision::from).collect()
    }

    fn open<'a>(
        cx: &'a mut TestAppContext,
        path: &str,
        history: DirectoryHistory,
    ) -> (Entity<LocalHistoryView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let path = path.to_owned();
        let (view, cx) = cx.add_window_view(move |_, cx| LocalHistoryView::new(path, DiffViewOptions::default(), cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &LocalHistoryEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.load(cx));
        view.update(cx, |view, cx| view.set_history(Ok(history), Some("/p".to_owned()), cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    /// The labels of the last diff the view asked for.
    fn asked_diff(events: &Events) -> Option<(String, String)> {
        events.borrow().iter().rev().find_map(|event| match event {
            LocalHistoryEvent::LoadDiff(data) => Some((side_label(&data.left), side_label(&data.right))),
            _ => None,
        })
    }

    // src/features/vcs/__tests__/localHistory.test.tsx › Local History › finds each file's revision at a label
    #[test]
    fn finds_each_files_revision_at_a_label() {
        let entries = revisions(&[
            snap("/p/a", 1.0, "Saved", 'a'),
            snap("/p/a", 3.0, "Saved", 'b'),
            snap("/p/b", 2.0, "Saved", 'c'),
            label(4.0, "L"),
        ]);
        let at = snapshots_before(&entries, 2.5);
        assert_eq!(at.keys().cloned().collect::<Vec<_>>(), ["/p/a", "/p/b"]);
        assert_eq!(at["/p/a"].id, id('a'));
    }

    // src/features/vcs/__tests__/localHistory.test.tsx › Local History › picks the oldest and newest of several
    // selected revisions of one file
    #[test]
    fn picks_the_oldest_and_newest_of_several_selected_revisions_of_one_file() {
        let entries = revisions(&[
            snap("/p/a", 3.0, "C", 'c'),
            label(2.0, "L"),
            snap("/p/a", 1.0, "B", 'b'),
            snap("/p/b", 0.0, "X", 'x'),
        ]);
        let (older, newer) = revision_range(&entries, [2, 0, 1]).expect("two revisions of one file");
        assert_eq!((older.label.as_str(), newer.label.as_str()), ("B", "C"));
        assert!(revision_range(&entries, [0]).is_none());
        assert!(revision_range(&entries, [0, 1]).is_none());
        // Revisions of different files have nothing to diff.
        assert!(revision_range(&entries, [0, 3]).is_none());
    }

    // src/features/vcs/__tests__/localHistory.test.tsx › Local History › shows a folder's history with deleted
    // files and reverts to a label
    #[gpui_kit::test]
    async fn shows_a_folders_history_with_deleted_files_and_reverts_to_a_label(cx: &mut TestAppContext) {
        let history = DirectoryHistory {
            entries: vec![
                label(50.0, "Before Git checkout"),
                snap("/p/src/gone.ts", 40.0, "Deleted", 'd'),
                snap("/p/src/a.ts", 30.0, "External change", 'e'),
                snap("/p/src/a.ts", 10.0, "Saved", 'f'),
            ],
            deleted: vec![std::path::PathBuf::from("/p/src/gone.ts").into()],
        };
        let (view, events, cx) = open(cx, "/p/src", history);
        assert_eq!(events.borrow().first(), Some(&LocalHistoryEvent::Load("/p/src".to_owned())));
        assert!(view.read_with(cx, |view, _| view.is_folder()));
        let rows = view.read_with(cx, |view, _| view.rows());
        assert_eq!(rows[0].text, "Before Git checkout");
        assert!(rows[0].is_label);
        assert_eq!(rows[1].path.as_deref(), Some("src/gone.ts"));
        assert!(rows[1].deleted);
        let detail = view.read_with(cx, |view, _| view.detail().clone());
        assert_eq!(detail, Detail::Message("Label 'Before Git checkout' · 2 files recorded before it".to_owned()));

        // Revert at the label restores every file's last revision before it.
        view.update(cx, |view, cx| view.revert(cx));
        let reverted = events.borrow().iter().rev().find_map(|event| match event {
            LocalHistoryEvent::Revert { targets, question } => Some((targets.clone(), question.clone())),
            _ => None,
        });
        let (targets, question) = reverted.expect("the revert is asked for");
        let restored: Vec<String> = targets.iter().map(|target| format!("{} {}", target.path, target.id)).collect();
        assert_eq!(restored, [format!("/p/src/a.ts {}", id('e')), format!("/p/src/gone.ts {}", id('d'))]);
        assert_eq!(question, "Revert 2 files under src to label 'Before Git checkout'?");

        // A deleted file's row offers Restore and diffs against nothing.
        let centre = cx.debug_bounds("local-history-row-1").expect("the row is drawn").center();
        cx.simulate_click(centre, Modifiers::default());
        assert_eq!(view.read_with(cx, |view, _| view.revert_label()), "Restore");
        let (_, right) = asked_diff(&events).expect("the revision's diff is asked for");
        assert_eq!(right, "Deleted");
    }

    // src/features/vcs/__tests__/localHistory.test.tsx › Local History › diffs two revisions selected with Shift-
    // or Cmd-click
    #[gpui_kit::test]
    async fn diffs_two_revisions_selected_with_shift_or_cmd_click(cx: &mut TestAppContext) {
        let history = DirectoryHistory {
            entries: vec![
                snap("/p/a.ts", 30.0, "Third", 'c'),
                snap("/p/a.ts", 20.0, "Second", 'b'),
                snap("/p/a.ts", 10.0, "First", 'a'),
            ],
            deleted: Vec::new(),
        };
        let (view, events, cx) = open(cx, "/p/a.ts", history);
        // The labels name a revision "<time> · <what>": what is compared is the part after the dot.
        let ends = |events: &Events| {
            let (left, right) = asked_diff(events).expect("a diff is asked for");
            let what = |label: &str| label.rsplit(" · ").next().unwrap_or_default().to_owned();
            (what(&left), what(&right))
        };
        assert_eq!(ends(&events), ("Third".to_owned(), "Current".to_owned()));
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        let command = Modifiers { platform: true, ..Modifiers::default() };

        view.update(cx, |view, cx| view.click(2, shift, cx));
        let rows = view.read_with(cx, |view, _| view.rows());
        let selected = rows.iter().filter(|row| row.active || row.selected).count();
        assert_eq!(selected, 3);
        assert_eq!(ends(&events), ("First".to_owned(), "Third".to_owned()));

        view.update(cx, |view, cx| view.click(1, Modifiers::default(), cx));
        view.update(cx, |view, cx| view.click(0, command, cx));
        assert_eq!(ends(&events), ("Second".to_owned(), "Third".to_owned()));

        // Keyboard navigation goes back to a single revision.
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down");
        assert_eq!(ends(&events), ("Second".to_owned(), "Current".to_owned()));
        assert_eq!(view.read_with(cx, |view, _| view.active()), 1);
    }

    // The texts of a diff the detail moved past are not shown; those of the shown one are.
    #[gpui_kit::test]
    async fn shows_the_texts_of_the_diff_of_the_active_revision_only(cx: &mut TestAppContext) {
        let history = DirectoryHistory {
            entries: vec![snap("/p/a.ts", 20.0, "Second", 'b'), snap("/p/a.ts", 10.0, "First", 'a')],
            deleted: Vec::new(),
        };
        let (view, _events, cx) = open(cx, "/p/a.ts", history);
        let key = |view: &Entity<LocalHistoryView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |view, _| match view.detail() {
                Detail::Diff(data) => diff_key(data),
                Detail::Message(_) => String::new(),
            })
        };
        let first = key(&view, cx);
        view.update(cx, |view, cx| view.select(1, cx));
        view.update(cx, |view, cx| view.diff_loaded(&first, "stale\n", "x\n", cx));
        let viewer = view.read_with(cx, |view, _| view.diff_viewer().clone());
        assert!(viewer.read_with(cx, |viewer, _| viewer.left_lines()).iter().all(|line| line.text != "stale"));
        let second = key(&view, cx);
        view.update(cx, |view, cx| view.diff_loaded(&second, "old\n", "new\n", cx));
        let left: Vec<String> =
            viewer.read_with(cx, |viewer, _| viewer.left_lines()).into_iter().map(|line| line.text).collect();
        assert_eq!(left.first().map(String::as_str), Some("old"));
    }

    // Not from a spec: Recent Changes lists the newest events first, repeated saves of a file collapsed.
    #[test]
    fn recent_changes_come_newest_first_and_repeated_saves_of_a_file_collapse() {
        let entries = revisions(&[
            snap("/p/a.ts", 10.0, "Saved", 'a'),
            snap("/p/a.ts", 20.0, "Saved", 'b'),
            label(30.0, "Before refactoring"),
            snap("/p/b.ts", 40.0, "Saved", 'c'),
        ]);
        let shown: Vec<String> =
            recent_changes(&entries, 40).iter().map(|entry| recent_change_label(entry, "/p")).collect();
        assert_eq!(shown, ["b.ts — Saved", "Before refactoring", "a.ts — Saved"]);
        assert_eq!(recent_changes(&entries, 2).len(), 2);
        assert_eq!((format_size(3), format_size(2048)), ("3 B".to_owned(), "2.0 KB".to_owned()));
    }

    #[test]
    fn the_tab_input_names_its_path() {
        let input = local_history_input("/p/src/a.ts");
        assert_eq!(input.key, "localHistory:/p/src/a.ts");
        assert_eq!(local_history_path(&input).as_deref(), Some("/p/src/a.ts"));
        assert_eq!(local_history_title(&input), "Local History: a.ts");
        let other = EditorInput { kind: "file".to_owned(), ..input };
        assert_eq!(local_history_path(&other), None);
    }
}
