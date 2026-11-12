//! The Bookmarks popup and the mnemonic chooser.
//!
//! The Bookmarks popup lists every bookmark of the project by bookmark list: the bookmarked folders, then
//! the files by path, each file followed by its line bookmarks, beside the text of the selected one. Enter
//! or a double click goes to the selected bookmark, Delete removes it and F2 edits the description of a line
//! bookmark. The mnemonic chooser gives the bookmark of one line a digit or a letter, and a description.
//!
//! Both work on the [`BookmarkStore`] they are handed and say what to save after every change; neither
//! reads or writes storage.

use super::bookmarks_store::{
    Bookmark, BookmarkData, BookmarkStore, MNEMONICS, MnemonicTarget, assign_mnemonic, bookmark_at, bookmark_label,
    describe_bookmark, is_mnemonic, remove_bookmarks,
};
use super::bookmarks_view::{BookmarkNode, build_bookmark_tree, group_bookmarks};
use super::open::StorageWrite;
use crate::backend::Backend;
use crate::platform::collate::compare_code_units;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::platform::keymap::is_mac_host;
use crate::platform::local_time::now_ms;
use crate::platform::virtual_docs::{basename, relative_path};
use crate::theme;
use crate::ui::text_field::TextField;
use gpui_kit::{
    App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    Keystroke, Render, ScrollStrategy, SharedString, Task, UniformListScrollHandle, Window, div, prelude::*, px,
    uniform_list,
};
use std::borrow::Cow;
use std::cmp::Ordering;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;

const WIDTH: f32 = 820.0;
const LIST_WIDTH: f32 = 300.0;
const BODY_HEIGHT: f32 = 420.0;
/// Height of a row of the list, in pixels.
const ROW_HEIGHT: f32 = 22.0;
/// How many rows Page Up and Page Down move by: one less than the list shows.
const PAGE_ROWS: usize = 18;
/// The most that is read of a file that no editor has open, for the text beside the list.
const PREVIEW_BYTES: usize = 2 * 1024 * 1024;
/// How many UTF-16 code units of a selection become the description of a bookmark set on it.
const SELECTION_UNITS: usize = 1000;
const BUTTON_HEIGHT: f32 = 24.0;
const CHOOSER_WIDTH: f32 = 316.0;
/// Width and height of a key of the mnemonic chooser, in pixels.
const KEY_SIZE: f32 = 24.0;

/// The bookmark that Next Bookmark (`forward`) or Previous Bookmark goes to from the caret on `line` of
/// `path`: the nearest one after or before it, by the files' paths and then by line, going round the
/// ends. Without an editor there is no `path` and no `line`: forward is the first bookmark, back the last.
pub fn neighbour_bookmark<'a>(
    items: &'a [Bookmark],
    path: Option<&str>,
    line: Option<u32>,
    forward: bool,
) -> Option<&'a Bookmark> {
    let (path, line) = (path.unwrap_or_default(), line.map_or(-1, i64::from));
    // Where a bookmark lies from the caret.
    let side = |bookmark: &Bookmark| compare_code_units(&bookmark.path, path).then(i64::from(bookmark.line).cmp(&line));
    let mut sorted: Vec<&Bookmark> = items.iter().collect();
    sorted.sort_by(|a, b| compare_code_units(&a.path, &b.path).then(a.line.cmp(&b.line)));
    let found = if forward {
        sorted.iter().find(|&&bookmark| side(bookmark) == Ordering::Greater).or(sorted.first())
    } else {
        sorted.iter().rfind(|&&bookmark| side(bookmark) == Ordering::Less).or(sorted.last())
    };
    found.copied()
}

/// The description of a bookmark set while `selected` is the editor's selection: the start of the
/// selection with every run of white space as one space. `None` when that leaves nothing.
pub fn selection_description(selected: &str) -> Option<String> {
    let mut units = 0;
    let start: String = selected
        .chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= SELECTION_UNITS
        })
        .collect();
    let text = start.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then_some(text)
}

/// `path` as the popup shows it: relative to the project at `root` when it lies below it.
fn shown_path<'a>(root: Option<&str>, path: &'a str) -> Cow<'a, str> {
    match root {
        Some(root) => relative_path(root, path),
        None => Cow::Borrowed(path),
    }
}

/// A row of the popup's list.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Row {
    /// The header of a bookmark list.
    List { key: String, name: String },
    /// The header of a file that has line bookmarks and is not bookmarked itself.
    File { key: String, path: String },
    /// A bookmarked file or folder.
    Path { key: String, path: String, dir: bool },
    /// A bookmarked line.
    Mark { key: String, bookmark: Bookmark },
}

impl Row {
    fn key(&self) -> &str {
        match self {
            Self::List { key, .. } | Self::File { key, .. } | Self::Path { key, .. } | Self::Mark { key, .. } => key,
        }
    }

    /// Headers are not selected: the selection passes over them.
    fn is_section(&self) -> bool {
        matches!(self, Self::List { .. } | Self::File { .. })
    }
}

/// Appends the rows of one list's `nodes`: each folder, then each file with its line bookmarks under it.
/// A bookmarked file is a row to select; a file that only has line bookmarks is a header.
fn push_rows(nodes: Vec<BookmarkNode>, rows: &mut Vec<Row>) {
    for node in nodes {
        match node {
            BookmarkNode::Folder { key, path, .. } => rows.push(Row::Path { key, path, dir: true }),
            BookmarkNode::File { key, path, bookmarked, children, .. } => {
                rows.push(if bookmarked {
                    // Its key tells it from the header the file has in a list it is not bookmarked in.
                    let key = format!("p{}", key.strip_prefix('f').unwrap_or(&key));
                    Row::Path { key, path, dir: false }
                } else {
                    Row::File { key, path }
                });
                for child in children {
                    if let BookmarkNode::Mark { key, bookmark } = child {
                        rows.push(Row::Mark { key, bookmark });
                    }
                }
            }
            BookmarkNode::List { .. } | BookmarkNode::Mark { .. } => {}
        }
    }
}

/// The rows of the popup: each bookmark list that has bookmarks as a header with its folders, then its
/// files by path, each file followed by its line bookmarks. Bookmarks kept without any list show without
/// a header.
fn bookmark_rows(data: &BookmarkData) -> Vec<Row> {
    let mut rows = Vec::new();
    if data.lists.is_empty() {
        push_rows(group_bookmarks(&data.items, &data.paths, "", None), &mut rows);
    }
    for node in build_bookmark_tree(data, None) {
        if let BookmarkNode::List { key, list, count, children, .. } = node
            && count > 0
        {
            rows.push(Row::List { key, name: list.name });
            push_rows(children, &mut rows);
        }
    }
    rows
}

/// The row a navigation key moves the selection to from row `active`. Headers are passed over and the
/// ends are not gone round: a key that leads nowhere leaves the selection where it is. Page Up and Page
/// Down move by [`PAGE_ROWS`], or to the nearest row to select when that lands on a header.
fn navigate(key: &str, rows: &[Row], active: Option<usize>) -> Option<usize> {
    let Some(last) = rows.len().checked_sub(1) else { return active };
    let down = |from: usize| (from..=last).find(|&index| !rows[index].is_section());
    let up = |from: usize| (0..=from.min(last)).rev().find(|&index| !rows[index].is_section());
    let below = active.map_or(0, |active| active + 1);
    let above = || active.and_then(|active| active.checked_sub(1)).and_then(up);
    let next = match key {
        "down" => down(below),
        "up" if active.is_none() => up(last),
        "up" => above(),
        "home" => down(0),
        "end" => up(last),
        "pagedown" => {
            let next = up(active.map_or(PAGE_ROWS - 1, |active| active + PAGE_ROWS));
            if next <= active { down(below) } else { next }
        }
        "pageup" => {
            let next = down(active.map_or(0, |active| active.saturating_sub(PAGE_ROWS)));
            if next.is_none() || next >= active { above() } else { next }
        }
        _ => None,
    };
    next.or(active)
}

/// The lines of `text` as the popup draws them: tabs take four columns.
fn preview_lines(text: &str) -> Vec<SharedString> {
    text.lines().map(|line| SharedString::from(line.replace('\t', "    "))).collect()
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// A button of the description editor: filled when `primary`, outlined otherwise.
fn button(cx: &App, label: String, primary: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
        .min_w(px(64.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .child(SharedString::from(label));
    if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    }
}

/// The box a text field is drawn in.
fn field_box(cx: &App, field: &Entity<TextField>) -> Div {
    div()
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-input"))
        .child(field.clone())
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
        .text_size(theme::length(cx, "fs-tiny"))
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(mnemonic.to_string()))
}

/// The icon, the name and the place of a file or folder.
fn file_label(cx: &App, path: &str, dir: bool, root: Option<&str>) -> Div {
    let name = basename(path);
    let icon = if dir { "folder" } else { theme::icons::file_icon_name(&name) };
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .min_w_0()
        .child(theme::icon(cx, icon, 14.0, theme::color(cx, "fg-muted")))
        .child(div().flex_none().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(name.into_owned())))
        .child(
            div()
                .min_w_0()
                .overflow_hidden()
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(shown_path(root, path).into_owned())),
        )
}

/// What the Bookmarks popup asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarksPopupEvent {
    /// Escape.
    Dismissed,
    /// Close the popup and open `path`: with the caret at the start of the 0-based `line` for a line
    /// bookmark, as it was last shown for a bookmarked file.
    Open { path: PathBuf, line: Option<u32> },
    /// Close the popup and select this bookmarked folder in the Project view.
    SelectInProjectView(PathBuf),
    /// A bookmark was removed or described; the popup stays open. Save these values: there are none when
    /// no project is open. [`BookmarksPopup::store`] holds the bookmarks as they are now.
    Changed(Vec<StorageWrite>),
}

/// The file whose text is shown beside the list.
struct Preview {
    path: String,
    /// The file's lines; `None` until they are read.
    lines: Option<Rc<Vec<SharedString>>>,
}

pub struct BookmarksPopup {
    /// The window, as the backend knows it.
    window_label: String,
    store: BookmarkStore,
    rows: Rc<Vec<Row>>,
    /// The row the selection was last put on. The selection is that row, or the last row once fewer
    /// are left.
    index: Option<usize>,
    /// What has the keyboard while the list is used.
    focus: FocusHandle,
    /// The path and line of the bookmark whose description is being edited.
    editing: Option<(String, u32)>,
    edit_field: Entity<TextField>,
    preview: Option<Preview>,
    code_font: SharedString,
    scroll: UniformListScrollHandle,
    preview_scroll: UniformListScrollHandle,
    /// The claims on the keys the popup handles itself, ahead of the keymap: in the list and in the
    /// description field.
    key_claims: [u64; 2],
    /// The question to the backend for the text of the file shown; a newer one takes its place.
    _preview_load: Option<Task<()>>,
}

impl EventEmitter<BookmarksPopupEvent> for BookmarksPopup {}

impl BookmarksPopup {
    /// The popup over the bookmarks of `store`, for the window `window_label`. Paths show relative to the
    /// project the store was loaded for.
    pub fn new(window_label: &str, store: BookmarkStore, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let edit_field = cx.new(TextField::new);
        let claimant = cx.weak_entity();
        let list_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |popup, cx| popup.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let claimant = cx.weak_entity();
        let edit_claim = input::claim_keys(
            edit_field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |popup, cx| popup.claim_edit_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|popup, cx| {
            for claim in popup.key_claims {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        let rows = bookmark_rows(store.data());
        let mut popup = Self {
            window_label: window_label.to_owned(),
            store,
            index: rows.iter().position(|row| !row.is_section()),
            rows: Rc::new(rows),
            focus,
            editing: None,
            edit_field,
            preview: None,
            code_font: code_font(cx),
            scroll: UniformListScrollHandle::new(),
            preview_scroll: UniformListScrollHandle::new(),
            key_claims: [list_claim, edit_claim],
            _preview_load: None,
        };
        popup.sync_preview(cx);
        popup
    }

    /// The bookmarks as the popup left them.
    pub fn store(&self) -> &BookmarkStore {
        &self.store
    }

    /// The field the description of a bookmark is edited in.
    pub fn edit_field(&self) -> &Entity<TextField> {
        &self.edit_field
    }

    /// The path and line of the bookmark whose description is being edited.
    pub fn editing(&self) -> Option<(&str, u32)> {
        self.editing.as_ref().map(|(path, line)| (path.as_str(), *line))
    }

    /// The key of every row: `l:` a list, `d:` a folder, `p:` a bookmarked file and `f:` the header of a
    /// file, each with the id of its list, and `b:` a line bookmark.
    pub fn keys(&self) -> Vec<&str> {
        self.rows.iter().map(Row::key).collect()
    }

    /// The rows as text: `[list]`, the path of a file header, `* path` for a bookmarked file or folder
    /// (a folder ends in `/`), and under a file its bookmarks as the line number from 1, the mnemonic in
    /// brackets and the label.
    pub fn lines(&self) -> Vec<String> {
        let root = self.root();
        self.rows
            .iter()
            .map(|row| match row {
                Row::List { name, .. } => format!("[{name}]"),
                Row::File { path, .. } => shown_path(root, path).into_owned(),
                Row::Path { path, dir, .. } => {
                    format!("* {}{}", shown_path(root, path), if *dir { "/" } else { "" })
                }
                Row::Mark { bookmark, .. } => {
                    let mnemonic = bookmark.mnemonic.map(|mnemonic| format!("[{mnemonic}] ")).unwrap_or_default();
                    let label = Some(bookmark_label(bookmark)).filter(|label| !label.is_empty());
                    format!("  {} {mnemonic}{}", u64::from(bookmark.line) + 1, label.unwrap_or("(empty line)"))
                }
            })
            .collect()
    }

    /// The selected bookmark, as [`Self::lines`] shows it. A header is never selected: there is no
    /// selection while the place the selection was at holds one.
    pub fn selected(&self) -> Option<String> {
        let index = self.active().filter(|&index| !self.rows[index].is_section())?;
        self.lines().into_iter().nth(index)
    }

    /// What the list says in place of rows, when there is nothing bookmarked.
    pub fn empty_text(&self) -> Option<&'static str> {
        let data = self.store.data();
        (data.items.is_empty() && data.paths.is_empty()).then_some("No bookmarks")
    }

    /// The file whose text shows beside the list and the 0-based line marked in it: the selected line
    /// bookmark, or the selected bookmarked file from its top. A folder has no text.
    pub fn preview_target(&self) -> Option<(&str, Option<u32>)> {
        match self.active_row()? {
            Row::Mark { bookmark, .. } => Some((bookmark.path.as_str(), Some(bookmark.line))),
            Row::Path { path, dir: false, .. } => Some((path.as_str(), None)),
            _ => None,
        }
    }

    /// The lines shown beside the list; none until the file is read.
    pub fn preview_text(&self) -> Vec<String> {
        let lines = self.preview.as_ref().and_then(|preview| preview.lines.as_ref());
        lines.map(|lines| lines.iter().map(ToString::to_string).collect()).unwrap_or_default()
    }

    /// The folder of the project, which paths are shown relative to.
    fn root(&self) -> Option<&str> {
        self.store.scope().filter(|root| !root.is_empty())
    }

    /// The row the selection is at.
    fn active(&self) -> Option<usize> {
        let last = self.rows.len().checked_sub(1)?;
        self.index.map(|index| index.min(last))
    }

    fn active_row(&self) -> Option<&Row> {
        self.rows.get(self.active()?)
    }

    /// Puts the selection on row `index` and scrolls it into view.
    fn select(&mut self, index: Option<usize>, cx: &mut Context<Self>) {
        self.index = index;
        if let Some(index) = index {
            self.scroll.scroll_to_item(index, ScrollStrategy::Nearest);
        }
        self.sync_preview(cx);
        cx.notify();
    }

    /// Shows the text of the selected bookmark's file beside the list, scrolled to its line. The text of
    /// another file is asked of the backend.
    fn sync_preview(&mut self, cx: &mut Context<Self>) {
        let Some((path, line)) = self.preview_target().map(|(path, line)| (path.to_owned(), line)) else {
            (self.preview, self._preview_load) = (None, None);
            return;
        };
        if self.preview.as_ref().is_none_or(|preview| preview.path != path) {
            self.preview = Some(Preview { path: path.clone(), lines: None });
            self.load_preview(path, cx);
        }
        self.preview_scroll.scroll_to_item(line.unwrap_or(0) as usize, ScrollStrategy::Center);
    }

    /// Asks the backend for the text of `path`: what an editor of the file shows when one is open, else
    /// what is on disk. Reading it opens no document and leaves the recent files as they are.
    fn load_preview(&mut self, path: String, cx: &mut Context<Self>) {
        self._preview_load = None;
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let (window, file) = (self.window_label.clone(), path.clone());
        let text = backend.with_host(move |host| async move {
            let project = host.project(&window)?;
            host.blocking(move || {
                let file = Path::new(&file);
                match project.documents.text_of(file) {
                    Some(text) => Ok(text),
                    None => project.documents.load_file_bounded(file, PREVIEW_BYTES).map(|loaded| loaded.text),
                }
            })
            .await
        });
        self._preview_load = Some(cx.spawn(async move |popup, cx| {
            let text = match text.await {
                Ok(Ok(text)) => text,
                Ok(Err(error)) => {
                    tracing::debug!("bookmarks: cannot read {path}: {}", error.message);
                    String::new()
                }
                Err(error) => {
                    tracing::debug!("bookmarks: cannot read {path}: {error}");
                    String::new()
                }
            };
            // The popup is gone when it was closed meanwhile.
            let _ = popup.update(cx, |popup, cx| popup.preview_loaded(&path, &text, cx));
        }));
    }

    /// The text of `path` arrived. It is dropped when the selection has moved to another file since.
    fn preview_loaded(&mut self, path: &str, text: &str, cx: &mut Context<Self>) {
        let line = self.preview_target().and_then(|(_, line)| line).unwrap_or(0);
        let Some(preview) = self.preview.as_mut().filter(|preview| preview.path == path) else { return };
        preview.lines = Some(Rc::new(preview_lines(text)));
        self.preview_scroll.scroll_to_item(line as usize, ScrollStrategy::Center);
        cx.notify();
    }

    /// Enter and a double click: asks the window to go to the selected bookmark.
    pub fn activate(&mut self, cx: &mut Context<Self>) {
        let event = match self.active_row() {
            Some(Row::Mark { bookmark, .. }) => {
                BookmarksPopupEvent::Open { path: PathBuf::from(&bookmark.path), line: Some(bookmark.line) }
            }
            Some(Row::Path { path, dir: true, .. }) => BookmarksPopupEvent::SelectInProjectView(PathBuf::from(path)),
            Some(Row::Path { path, dir: false, .. }) => {
                BookmarksPopupEvent::Open { path: PathBuf::from(path), line: None }
            }
            _ => return,
        };
        cx.emit(event);
    }

    /// Changes the bookmarks, lists them again and tells the window what to save. The selection stays at
    /// the place it was at, which now holds what came after the bookmark that is gone.
    fn apply(&mut self, change: impl FnOnce(&mut BookmarkStore) -> Vec<StorageWrite>, cx: &mut Context<Self>) {
        let before = self.store.data().clone();
        let writes = change(&mut self.store);
        if *self.store.data() == before {
            return;
        }
        self.rows = Rc::new(bookmark_rows(self.store.data()));
        self.sync_preview(cx);
        cx.emit(BookmarksPopupEvent::Changed(writes));
        cx.notify();
    }

    /// Delete and Backspace: removes the selected bookmark. A bookmarked file or folder leaves every list
    /// it is in.
    fn remove_selected(&mut self, cx: &mut Context<Self>) {
        match self.active_row().cloned() {
            Some(Row::Path { path, .. }) => self.apply(
                |store| {
                    store.update_data(|data| {
                        let mut next = data.clone();
                        next.paths.retain(|bookmark| bookmark.path != path);
                        next
                    })
                },
                cx,
            ),
            Some(Row::Mark { bookmark, .. }) => self.apply(
                |store| {
                    store.update(|items| {
                        remove_bookmarks(items, |other| other.path == bookmark.path && other.line == bookmark.line)
                    })
                },
                cx,
            ),
            _ => {}
        }
    }

    /// F2: edits the description of the selected line bookmark, starting from the text it shows.
    fn edit_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Row::Mark { bookmark, .. }) = self.active_row().cloned() else { return };
        let text = bookmark.description.unwrap_or(bookmark.text);
        self.edit_field.update(cx, |field, cx| {
            field.set_text(&text, cx);
            field.select_all(cx);
        });
        self.editing = Some((bookmark.path, bookmark.line));
        window.focus(&self.edit_field.focus_handle(cx), cx);
        cx.notify();
    }

    /// Enter and OK in the description editor: the bookmark shows the typed text, or its line again
    /// when nothing is typed. The keyboard goes back to the list.
    fn save_description(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((path, line)) = self.editing.take() else { return };
        let text = self.edit_field.read(cx).text().to_owned();
        window.focus(&self.focus, cx);
        self.apply(|store| store.update(|items| describe_bookmark(items, &path, line, &text)), cx);
        cx.notify();
    }

    /// Escape and Cancel in the description editor: the bookmark keeps its description.
    fn cancel_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editing = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// A click on row `index` selects it, a double click goes to its bookmark. The list is not used
    /// while a description is edited.
    fn click_row(&mut self, index: usize, event: &ClickEvent, cx: &mut Context<Self>) {
        if self.editing.is_some() {
            return;
        }
        self.select(Some(index), cx);
        if event.click_count() >= 2 {
            self.activate(cx);
        }
    }

    /// The keys the popup handles itself while the keyboard is in its list, in every modifier
    /// combination; the keymap does not see them. Returns whether `keystroke` is one of them. While a
    /// description is edited the list only takes the editor's Enter and Escape.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let key = keystroke.key.as_str();
        let navigation = matches!(key, "up" | "down" | "home" | "end" | "pageup" | "pagedown");
        if !navigation && !matches!(key, "enter" | "escape" | "delete" | "backspace" | "f2") {
            return false;
        }
        if self.editing.is_some() {
            self.claim_edit_key(keystroke, window, cx);
            return true;
        }
        match key {
            "enter" => self.activate(cx),
            "escape" => cx.emit(BookmarksPopupEvent::Dismissed),
            "delete" | "backspace" => self.remove_selected(cx),
            "f2" => self.edit_selected(window, cx),
            _ => {
                let next = navigate(key, &self.rows, self.active());
                self.select(next, cx);
            }
        }
        true
    }

    /// Enter and Escape while the keyboard is in the description field. Returns whether `keystroke` is
    /// one of them.
    fn claim_edit_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match keystroke.key.as_str() {
            "enter" => self.save_description(window, cx),
            "escape" => self.cancel_editing(window, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for BookmarksPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BookmarksPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &cx.default_global::<Localization>().0;
        let title = i18n.tx("Bookmarks");
        let empty = self.empty_text().map(|text| i18n.tx(text));
        let empty_line = SharedString::from(i18n.tx("(empty line)"));
        let (edit_title, ok_label, cancel_label) =
            (i18n.tx("Edit Bookmark Description"), i18n.tx("OK"), i18n.tx("Cancel"));

        let (fg, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let (muted, subtle) = (theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let divider = theme::color(cx, "border-popup");
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-tree-hover"));

        let rows = Rc::clone(&self.rows);
        let active = self.active();
        let root = self.root().map(str::to_owned);
        let list = if rows.is_empty() {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(muted)
                .children(empty.map(SharedString::from))
                .into_any_element()
        } else {
            uniform_list(
                "bookmarks",
                rows.len(),
                cx.processor(move |_, range: Range<usize>, _, cx| {
                    range
                        .map(|index| {
                            let line = div()
                                .id(index)
                                .debug_selector(|| format!("bookmarks-row-{index}"))
                                .h(px(ROW_HEIGHT))
                                .w_full()
                                .flex()
                                .items_center()
                                .gap(px(6.0))
                                .px(px(10.0))
                                .whitespace_nowrap()
                                .overflow_hidden()
                                .text_color(fg);
                            let line = match &rows[index] {
                                Row::List { name, .. } => {
                                    return line
                                        .text_color(muted)
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(SharedString::from(name.clone()));
                                }
                                Row::File { path, .. } => {
                                    return line.child(file_label(cx, path, false, root.as_deref()));
                                }
                                Row::Path { path, dir, .. } => line.child(file_label(cx, path, *dir, root.as_deref())),
                                Row::Mark { bookmark, .. } => {
                                    let mark = match bookmark.mnemonic {
                                        Some(mnemonic) => mnemonic_badge(cx, mnemonic).into_any_element(),
                                        None => theme::icon(cx, "bookmark", 14.0, muted),
                                    };
                                    let label = bookmark_label(bookmark);
                                    let label = if label.is_empty() {
                                        empty_line.clone()
                                    } else {
                                        SharedString::from(label.to_owned())
                                    };
                                    let number = (u64::from(bookmark.line) + 1).to_string();
                                    line.child(mark)
                                        .child(div().flex_1().min_w_0().overflow_hidden().child(label))
                                        .child(div().flex_none().text_color(subtle).child(SharedString::from(number)))
                                }
                            };
                            line.when(active == Some(index), |el| el.bg(selected_bg))
                                .when(active != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                                .on_click(cx.listener(move |popup, event: &ClickEvent, _, cx| {
                                    popup.click_row(index, event, cx);
                                }))
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .into_any_element()
        };

        let marked = self.preview_target().and_then(|(_, line)| line).map(|line| line as usize);
        let shown = self.preview.as_ref().and_then(|preview| preview.lines.clone()).filter(|lines| !lines.is_empty());
        let (line_height, number_color) = (theme::length(cx, "lh-code"), theme::color(cx, "fg-line-number"));
        let marked_bg = theme::color(cx, "bg-current-line");
        let preview = match shown {
            Some(lines) => uniform_list(
                "bookmarks-preview",
                lines.len(),
                cx.processor(move |_, range: Range<usize>, _, _| {
                    range
                        .map(|index| {
                            div()
                                .id(index)
                                .h(line_height)
                                .w_full()
                                .flex()
                                .items_center()
                                .whitespace_nowrap()
                                .when(marked == Some(index), |el| el.bg(marked_bg))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(52.0))
                                        .pr(px(12.0))
                                        .flex()
                                        .justify_end()
                                        .text_color(number_color)
                                        .child(SharedString::from((index + 1).to_string())),
                                )
                                .child(lines[index].clone())
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.preview_scroll)
            .size_full()
            .into_any_element(),
            None => div().size_full().into_any_element(),
        };

        let editor = self.editing.is_some().then(|| {
            let ok = button(cx, ok_label, true)
                .id("ok")
                .debug_selector(|| "bookmarks-edit-ok".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, window, cx| popup.save_description(window, cx)))
                .into_any_element();
            let cancel = button(cx, cancel_label, false)
                .id("cancel")
                .debug_selector(|| "bookmarks-edit-cancel".to_owned())
                .on_click(cx.listener(|popup, _: &ClickEvent, window, cx| popup.cancel_editing(window, cx)))
                .into_any_element();
            // The action that confirms comes last on macOS and first elsewhere.
            let buttons = if is_mac_host() { [cancel, ok] } else { [ok, cancel] };
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .py(px(6.0))
                .border_t_1()
                .border_color(divider)
                .text_color(strong)
                .child(div().flex_none().child(SharedString::from(edit_title)))
                .child(field_box(cx, &self.edit_field).flex_1().min_w_0())
                .children(buttons)
        });

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
            .track_focus(&self.focus)
            .child(
                div()
                    .flex_none()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .px(px(12.0))
                    .text_color(strong)
                    .child(SharedString::from(title)),
            )
            .children(editor)
            .child(
                div()
                    .flex_none()
                    .h(px(BODY_HEIGHT))
                    .flex()
                    .border_t_1()
                    .border_color(divider)
                    .child(div().flex_none().w(px(LIST_WIDTH)).h_full().border_r_1().border_color(divider).child(list))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .bg(theme::color(cx, "bg-editor"))
                            .font_family(self.code_font.clone())
                            .text_size(theme::length(cx, "fs-code"))
                            .text_color(theme::color(cx, "fg-editor"))
                            .child(preview),
                    ),
            )
    }
}

/// What the mnemonic chooser asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MnemonicChooserEvent {
    /// Escape.
    Dismissed,
    /// A mnemonic was chosen, or the description confirmed: close the chooser and save these values.
    /// There are none when nothing changed or no project is open. [`MnemonicChooser::store`] holds the
    /// bookmarks as they are now.
    Done(Vec<StorageWrite>),
}

pub struct MnemonicChooser {
    store: BookmarkStore,
    /// The line whose bookmark gets the mnemonic.
    target: MnemonicTarget,
    /// What has the keyboard while a mnemonic is typed.
    focus: FocusHandle,
    description: Entity<TextField>,
    /// The claims on the keys the chooser handles itself, ahead of the keymap: on its keys and in the
    /// description field.
    key_claims: [u64; 2],
}

impl EventEmitter<MnemonicChooserEvent> for MnemonicChooser {}

impl MnemonicChooser {
    /// The chooser for the line `target` names, over the bookmarks of `store`. The line needs no
    /// bookmark yet: choosing a mnemonic sets one.
    pub fn new(store: BookmarkStore, target: MnemonicTarget, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let placeholder = cx.default_global::<Localization>().0.tx("Description");
        let described = bookmark_at(&store.data().items, &target.path, target.line)
            .and_then(|bookmark| bookmark.description.clone())
            .unwrap_or_default();
        let description = cx.new(|cx| {
            let mut field = TextField::new(cx).with_placeholder(placeholder);
            field.set_text(&described, cx);
            field
        });
        let claimant = cx.weak_entity();
        let keys_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, window, cx| {
                claimant.update(cx, |chooser, cx| chooser.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        let claimant = cx.weak_entity();
        let field_claim = input::claim_keys(
            description.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |chooser, cx| chooser.claim_field_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|chooser, cx| {
            for claim in chooser.key_claims {
                input::release_keys(claim, cx);
            }
        })
        .detach();
        Self { store, target, focus, description, key_claims: [keys_claim, field_claim] }
    }

    /// The bookmarks as the chooser left them.
    pub fn store(&self) -> &BookmarkStore {
        &self.store
    }

    /// The field the description of the bookmark is typed in.
    pub fn description(&self) -> &Entity<TextField> {
        &self.description
    }

    /// The bookmark the line has now.
    fn current(&self) -> Option<&Bookmark> {
        bookmark_at(&self.store.data().items, &self.target.path, self.target.line)
    }

    /// What choosing a mnemonic does to the line.
    pub fn title(&self) -> &'static str {
        match self.current() {
            None => "Add Mnemonic Bookmark",
            Some(bookmark) if bookmark.mnemonic.is_some() => "Change Mnemonic",
            Some(_) => "Assign Mnemonic",
        }
    }

    /// The mnemonic of the line's bookmark.
    pub fn current_mnemonic(&self) -> Option<char> {
        self.current().and_then(|bookmark| bookmark.mnemonic)
    }

    /// The mnemonics other bookmarks have, in the order of the bookmarks. Choosing one takes it from
    /// its bookmark.
    pub fn assigned(&self) -> Vec<char> {
        let current = self.current_mnemonic();
        let mnemonics = self.store.data().items.iter().filter_map(|bookmark| bookmark.mnemonic);
        mnemonics.filter(|mnemonic| Some(*mnemonic) != current).collect()
    }

    /// Gives the line's bookmark `mnemonic` and the typed description, setting the bookmark when the
    /// line has none, and asks the window to close the chooser.
    pub fn choose(&mut self, mnemonic: char, cx: &mut Context<Self>) {
        if !is_mnemonic(mnemonic) {
            return;
        }
        let description = self.description.read(cx).text().to_owned();
        let MnemonicTarget { path, line, text, .. } = self.target.clone();
        let now = now_ms() as u64;
        let writes = self.store.update(|items| {
            assign_mnemonic(items, &path, line, &text, Some(mnemonic), Some(description.as_str()), now)
        });
        cx.emit(MnemonicChooserEvent::Done(writes));
    }

    /// Enter in the description: a bookmark with a mnemonic keeps it and takes the description, a plain
    /// bookmark takes the description, and a line without a bookmark stays without one.
    fn confirm_description(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current() else { return };
        if let Some(mnemonic) = current.mnemonic {
            self.choose(mnemonic, cx);
            return;
        }
        let description = self.description.read(cx).text().to_owned();
        let (path, line) = (self.target.path.clone(), self.target.line);
        let writes = self.store.update(|items| describe_bookmark(items, &path, line, &description));
        cx.emit(MnemonicChooserEvent::Done(writes));
    }

    /// The keys the chooser handles itself while the keyboard is on its keys: a digit or a letter
    /// chooses that mnemonic unless Ctrl, Alt or the platform key is held, Tab goes to the description
    /// and Escape gives up. Returns whether `keystroke` is one of them.
    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let modifiers = keystroke.modifiers;
        match keystroke.key.as_str() {
            "escape" => cx.emit(MnemonicChooserEvent::Dismissed),
            _ if modifiers.control || modifiers.alt || modifiers.platform => return false,
            "tab" => window.focus(&self.description.focus_handle(cx), cx),
            _ => {
                // The character the key types, where the platform gives one: ⇧1 is not a digit.
                let typed = keystroke.key_char.as_deref().unwrap_or(&keystroke.key);
                let mut chars = typed.chars();
                let (Some(c), None) = (chars.next(), chars.next()) else { return false };
                let mnemonic = c.to_ascii_uppercase();
                if !MNEMONICS.contains(&mnemonic) {
                    return false;
                }
                self.choose(mnemonic, cx);
            }
        }
        true
    }

    /// Enter, Tab and Escape while the keyboard is in the description. Returns whether `keystroke` is
    /// one of them.
    fn claim_field_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match keystroke.key.as_str() {
            "enter" => self.confirm_description(cx),
            "tab" => window.focus(&self.focus, cx),
            "escape" => cx.emit(MnemonicChooserEvent::Dismissed),
            _ => return false,
        }
        true
    }
}

impl Focusable for MnemonicChooser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for MnemonicChooser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let i18n = &cx.default_global::<Localization>().0;
        let title = i18n.tx(self.title());
        let (assigned_label, current_label) = (i18n.tx("Assigned bookmark"), i18n.tx("Current bookmark"));

        let (current, assigned) = (self.current_mnemonic(), self.assigned());
        let (fg, strong, subtle) =
            (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-subtle"));
        let (key_bg, key_border) = (theme::color(cx, "bg-elev"), theme::color(cx, "border-strong"));
        let accent = theme::color(cx, "accent");
        let (assigned_bg, current_bg) = (theme::color(cx, "accent-soft"), Hsla { a: 0.45, ..accent });
        let radius = theme::length(cx, "radius-small");
        let small = theme::length(cx, "fs-small");

        let key = |mnemonic: char| {
            let is_current = current == Some(mnemonic);
            let fill = if is_current {
                current_bg
            } else if assigned.contains(&mnemonic) {
                assigned_bg
            } else {
                key_bg
            };
            div()
                .id(mnemonic as usize)
                .debug_selector(move || format!("mnemonic-key-{mnemonic}"))
                .flex_none()
                .size(px(KEY_SIZE))
                .flex()
                .items_center()
                .justify_center()
                .rounded(radius)
                .border_1()
                .border_color(key_border)
                .bg(fill)
                .text_color(if is_current { strong } else { fg })
                .text_size(small)
                .hover(|el| el.border_color(accent))
                .child(SharedString::from(mnemonic.to_string()))
                .on_click(cx.listener(move |chooser, _: &ClickEvent, _, cx| chooser.choose(mnemonic, cx)))
        };
        let grid = |keys: &[char], columns: usize| {
            div().flex_none().flex().flex_col().gap(px(4.0)).children(
                keys.chunks(columns)
                    .map(|row| div().flex().gap(px(4.0)).children(row.iter().map(|&mnemonic| key(mnemonic)))),
            )
        };
        let mnemonics = MNEMONICS;
        let (digits, letters) = mnemonics.split_at(10);
        let grids = div().flex().gap(px(14.0)).child(grid(digits, 3)).child(grid(letters, 7));
        let dot = |color: Hsla| div().flex_none().size(px(8.0)).rounded(px(4.0)).bg(color);
        let legend = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_color(subtle)
            .text_size(small)
            .child(dot(assigned_bg))
            .child(SharedString::from(assigned_label))
            .child(div().flex_none().w(px(8.0)))
            .child(dot(current_bg))
            .child(SharedString::from(current_label));

        div()
            .w(px(CHOOSER_WIDTH))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(14.0))
            .pt(px(10.0))
            .pb(px(12.0))
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .rounded(px(10.0))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            .track_focus(&self.focus)
            .child(
                div()
                    .text_color(theme::color(cx, "fg-emphasis"))
                    .font_weight(FontWeight::MEDIUM)
                    .child(SharedString::from(title)),
            )
            .child(grids)
            .child(field_box(cx, &self.description).w_full())
            .child(legend)
    }
}

#[cfg(test)]
mod tests {
    use super::super::bookmarks_store::{BookmarkList, PathBookmark, bookmarks_key, lists_key};
    use super::*;
    use gpui_kit::{Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<BookmarksPopupEvent>>>;
    type ChooserEvents = Rc<RefCell<Vec<MnemonicChooserEvent>>>;

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

    const PROJECT_LINES: [&str; 9] = [
        "[p]",
        "* lib/",
        "* README.md",
        "src/a.ts",
        "  3 alpha()",
        "  10 [A] (empty line)",
        "[Review]",
        "src/b.ts",
        "  5 the b",
    ];

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
    }

    /// The popup over `store` in a window of its own, with the keyboard, and what it asks of the window.
    fn open(cx: &mut TestAppContext, store: BookmarkStore) -> (Entity<BookmarksPopup>, Events, &mut VisualTestContext) {
        init(cx);
        let (popup, cx) = cx.add_window_view(move |_, cx| BookmarksPopup::new("w1", store, cx));
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&popup, move |_, event: &BookmarksPopupEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = popup.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (popup, events, cx)
    }

    /// The chooser for line 3 of `/p/a` in a window of its own, with the keyboard.
    fn choose(
        cx: &mut TestAppContext,
        store: BookmarkStore,
    ) -> (Entity<MnemonicChooser>, ChooserEvents, &mut VisualTestContext) {
        init(cx);
        let target = MnemonicTarget { path: "/p/a".to_owned(), line: 2, text: "fun a()".to_owned(), at: None };
        let (chooser, cx) = cx.add_window_view(move |_, cx| MnemonicChooser::new(store, target, cx));
        let events: ChooserEvents = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&chooser, move |_, event: &MnemonicChooserEvent, _| log.borrow_mut().push(event.clone()))
                .detach();
            let focus = chooser.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (chooser, events, cx)
    }

    fn lines(popup: &Entity<BookmarksPopup>, cx: &mut VisualTestContext) -> Vec<String> {
        popup.read_with(cx, |popup, _| popup.lines())
    }

    fn selected(popup: &Entity<BookmarksPopup>, cx: &mut VisualTestContext) -> Option<String> {
        popup.read_with(cx, |popup, _| popup.selected())
    }

    fn items(popup: &Entity<BookmarksPopup>, cx: &mut VisualTestContext) -> Vec<Bookmark> {
        popup.read_with(cx, |popup, _| popup.store().data().items.clone())
    }

    fn typed(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
        field.read_with(cx, |field, _| field.text().to_owned())
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, count: usize) {
        cx.run_until_parked();
        let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: count,
        });
    }

    /// The keys of what the last change said to save.
    fn saved_keys(events: &Events) -> Vec<String> {
        match events.borrow().last() {
            Some(BookmarksPopupEvent::Changed(writes)) => writes.iter().map(|write| write.key.clone()).collect(),
            other => panic!("a change is reported, not {other:?}"),
        }
    }

    fn kinds(rows: &[Row]) -> Vec<&'static str> {
        rows.iter()
            .map(|row| match row {
                Row::List { .. } => "list",
                Row::File { .. } => "file",
                Row::Path { .. } => "path",
                Row::Mark { .. } => "mark",
            })
            .collect()
    }

    #[test]
    fn groups_line_bookmarks_by_file_under_the_header_of_their_list() {
        let store = store_of(vec![
            mark("/p/a", 1, "one"),
            Bookmark { mnemonic: Some('3'), ..mark("/p/a", 5, "five") },
            mark("/p/b", 0, "zero"),
        ]);
        let data = store.data();
        assert_eq!(kinds(&bookmark_rows(data)), ["list", "file", "mark", "mark", "file", "mark"]);
        let listless = BookmarkData { lists: Vec::new(), ..data.clone() };
        assert_eq!(kinds(&bookmark_rows(&listless)), ["file", "mark", "mark", "file", "mark"]);
    }

    #[test]
    fn groups_by_list_folders_first_then_files_with_their_line_bookmarks() {
        let mut store = store_of(Vec::new());
        let _ = store.update_data(|data| BookmarkData {
            lists: vec![data.lists[0].clone(), BookmarkList { id: "list2".to_owned(), name: "Review".to_owned() }],
            items: vec![
                Bookmark { list: Some("default".to_owned()), ..mark("/p/a.ts", 3, "three") },
                Bookmark { list: Some("list2".to_owned()), ..mark("/p/c.ts", 0, "zero") },
            ],
            paths: vec![
                path_mark("/p/src", true, "default"),
                path_mark("/p/a.ts", false, "default"),
                path_mark("/p/b.ts", false, "list2"),
            ],
            default_list: data.default_list.clone(),
        });
        let rows = bookmark_rows(store.data());
        assert_eq!(
            rows.iter().map(Row::key).collect::<Vec<_>>(),
            [
                "l:default",
                "d:default:/p/src",
                "p:default:/p/a.ts",
                "b:/p/a.ts:3",
                "l:list2",
                "p:list2:/p/b.ts",
                "f:list2:/p/c.ts",
                "b:/p/c.ts:0",
            ]
        );
    }

    /// The folder is the first row to select: Enter selects it in the Project view.
    #[gpui_kit::test]
    async fn enter_on_a_folder_selects_it_in_the_project_view(cx: &mut TestAppContext) {
        let mut store = store_of(vec![mark("/p/a.ts", 3, "three")]);
        let _ = store.update_data(|data| BookmarkData {
            paths: vec![path_mark("/p/src", true, "default"), path_mark("/p/a.ts", false, "default")],
            ..data.clone()
        });
        let (popup, events, cx) = open(cx, store);
        let keys = popup.read_with(cx, |popup, _| popup.keys().join(" "));
        assert_eq!(keys, "l:default d:default:/p/src p:default:/p/a.ts b:/p/a.ts:3");
        assert_eq!(selected(&popup, cx).as_deref(), Some("* src/"));
        cx.simulate_keystrokes("enter");
        assert_eq!(*events.borrow(), [BookmarksPopupEvent::SelectInProjectView(PathBuf::from("/p/src"))]);
    }

    /// Delete removes the selected bookmark and F2 edits the description of the one that takes its
    /// place.
    #[gpui_kit::test]
    async fn delete_removes_and_f2_edits_the_description(cx: &mut TestAppContext) {
        let store = store_of(vec![
            mark("/p/a", 1, "one"),
            Bookmark { mnemonic: Some('3'), ..mark("/p/a", 5, "five") },
            mark("/p/b", 0, "zero"),
        ]);
        let (popup, events, cx) = open(cx, store);
        assert_eq!(lines(&popup, cx), ["[p]", "a", "  2 one", "  6 [3] five", "b", "  1 zero"]);
        cx.simulate_keystrokes("delete");
        assert_eq!(items(&popup, cx).iter().map(|bookmark| bookmark.line).collect::<Vec<_>>(), [5, 0]);
        cx.simulate_keystrokes("f2");
        let editing = popup.read_with(cx, |popup, _| popup.editing().map(|(path, line)| (path.to_owned(), line)));
        assert_eq!(editing, Some(("/p/a".to_owned(), 5)));
        cx.simulate_input("main");
        cx.simulate_keystrokes("enter");
        assert_eq!(items(&popup, cx)[0].description.as_deref(), Some("main"));
        assert_eq!(events.borrow().len(), 2, "each change is reported");
    }

    /// The lists show as headers over their folders, files and line bookmarks, with the first bookmark
    /// selected and the keyboard in the list.
    #[gpui_kit::test]
    async fn lists_the_bookmarks_by_list_and_starts_on_the_first_one(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, project());
        assert_eq!(lines(&popup, cx), PROJECT_LINES);
        assert_eq!(selected(&popup, cx).as_deref(), Some("* lib/"));
        assert_eq!(popup.read_with(cx, |popup, _| popup.empty_text()), None);
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the list has the keyboard");
        assert!(events.borrow().is_empty());
    }

    /// The arrows, Home, End and the page keys move over the headers and stop at the ends.
    #[gpui_kit::test]
    async fn the_navigation_keys_pass_over_the_headers_and_stop_at_the_ends(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, project());
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&popup, cx).as_deref(), Some("* lib/"));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("* README.md"));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  3 alpha()"));
        cx.simulate_keystrokes("down down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
        cx.simulate_keystrokes("up");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  10 [A] (empty line)"));
        cx.simulate_keystrokes("home");
        assert_eq!(selected(&popup, cx).as_deref(), Some("* lib/"));
        cx.simulate_keystrokes("end");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
        cx.simulate_keystrokes("pageup");
        assert_eq!(selected(&popup, cx).as_deref(), Some("* lib/"));
        cx.simulate_keystrokes("pagedown");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
    }

    /// Enter opens a bookmarked file as it was and a line bookmark at its line.
    #[gpui_kit::test]
    async fn enter_goes_to_the_selected_bookmark(cx: &mut TestAppContext) {
        let (_popup, events, cx) = open(cx, project());
        cx.simulate_keystrokes("down enter");
        assert_eq!(*events.borrow(), [BookmarksPopupEvent::Open { path: PathBuf::from("/p/README.md"), line: None }]);
        cx.simulate_keystrokes("down down enter");
        assert_eq!(
            events.borrow().last(),
            Some(&BookmarksPopupEvent::Open { path: PathBuf::from("/p/src/a.ts"), line: Some(9) })
        );
    }

    /// Delete and Backspace remove the selected bookmark and say what to save. The selection stays at
    /// its place; a file without bookmarks left loses its header.
    #[gpui_kit::test]
    async fn delete_removes_the_selected_bookmark_and_reports_what_to_save(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, project());
        cx.simulate_keystrokes("backspace");
        assert_eq!(lines(&popup, cx)[..3], ["[p]", "* README.md", "src/a.ts"]);
        assert_eq!(selected(&popup, cx).as_deref(), Some("* README.md"));
        assert_eq!(saved_keys(&events), [lists_key("/p")]);
        assert_eq!(popup.read_with(cx, |popup, _| popup.store().data().paths.len()), 1);

        cx.simulate_keystrokes("down delete");
        assert_eq!(lines(&popup, cx)[2..4], ["src/a.ts", "  10 [A] (empty line)"]);
        assert_eq!(selected(&popup, cx).as_deref(), Some("  10 [A] (empty line)"));
        assert_eq!(saved_keys(&events), [bookmarks_key("/p")]);

        // The last bookmark of the file takes the file's header with it: the header of the next list is
        // at the selection's place now, and nothing is selected until a key moves on.
        cx.simulate_keystrokes("delete");
        assert_eq!(lines(&popup, cx), ["[p]", "* README.md", "[Review]", "src/b.ts", "  5 the b"]);
        assert_eq!(selected(&popup, cx), None);
        cx.simulate_keystrokes("delete");
        assert_eq!(lines(&popup, cx).len(), 5, "there is nothing selected to remove");
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
        cx.simulate_keystrokes("delete");
        assert_eq!(lines(&popup, cx), ["[p]", "* README.md"]);
        assert_eq!(selected(&popup, cx).as_deref(), Some("* README.md"));
        assert_eq!(events.borrow().len(), 4);
    }

    /// F2 opens the description of a line bookmark on the text it shows, selected. Enter keeps what is
    /// typed, an empty description shows the line again, and Escape leaves the editor before it
    /// dismisses the popup.
    #[gpui_kit::test]
    async fn f2_edits_the_description_of_a_line_bookmark(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, project());
        let field = popup.read_with(cx, |popup, _| popup.edit_field().clone());
        cx.simulate_keystrokes("f2");
        assert!(popup.read_with(cx, |popup, _| popup.editing().is_none()), "a folder has no description");

        cx.simulate_keystrokes("down down f2");
        assert_eq!(popup.read_with(cx, |popup, _| popup.editing().map(|(_, line)| line)), Some(2));
        assert_eq!(typed(&field, cx), "alpha()");
        assert_eq!(field.read_with(cx, |field, _| field.selected_text().to_owned()), "alpha()");
        assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)), "the field has the keyboard");
        cx.simulate_input("entry");
        cx.simulate_keystrokes("enter");
        assert_eq!(lines(&popup, cx)[4], "  3 entry");
        assert!(popup.read_with(cx, |popup, _| popup.editing().is_none()));
        assert!(cx.update(|window, cx| popup.focus_handle(cx).is_focused(window)), "the list has the keyboard again");
        assert!(matches!(events.borrow().as_slice(), [BookmarksPopupEvent::Changed(writes)] if writes.len() == 1));

        cx.simulate_keystrokes("f2");
        assert_eq!(typed(&field, cx), "entry");
        cx.simulate_keystrokes("backspace enter");
        assert_eq!(lines(&popup, cx)[4], "  3 alpha()");
        assert_eq!(events.borrow().len(), 2);

        cx.simulate_keystrokes("f2");
        cx.simulate_input("dropped");
        cx.simulate_keystrokes("escape");
        assert!(popup.read_with(cx, |popup, _| popup.editing().is_none()));
        assert_eq!(lines(&popup, cx)[4], "  3 alpha()");
        assert_eq!(events.borrow().len(), 2, "Escape in the editor changes nothing and keeps the popup");
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&BookmarksPopupEvent::Dismissed));
    }

    /// The buttons of the description editor confirm and cancel.
    #[gpui_kit::test]
    async fn the_buttons_of_the_description_editor_confirm_and_cancel(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, project());
        cx.simulate_keystrokes("end f2");
        cx.simulate_input("review");
        click(cx, "bookmarks-edit-ok", 1);
        assert_eq!(lines(&popup, cx)[8], "  5 review");
        assert_eq!(events.borrow().len(), 1);
        cx.simulate_keystrokes("f2");
        click(cx, "bookmarks-edit-cancel", 1);
        assert!(popup.read_with(cx, |popup, _| popup.editing().is_none()));
        assert_eq!(events.borrow().len(), 1);
    }

    /// A click selects a row and a double click goes to its bookmark; headers take no clicks.
    #[gpui_kit::test]
    async fn the_mouse_selects_and_a_double_click_goes_to_the_bookmark(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, project());
        click(cx, "bookmarks-row-4", 1);
        assert_eq!(selected(&popup, cx).as_deref(), Some("  3 alpha()"));
        click(cx, "bookmarks-row-3", 1);
        click(cx, "bookmarks-row-6", 2);
        assert_eq!(selected(&popup, cx).as_deref(), Some("  3 alpha()"));
        assert!(events.borrow().is_empty());
        click(cx, "bookmarks-row-8", 2);
        assert_eq!(selected(&popup, cx).as_deref(), Some("  5 the b"));
        assert_eq!(*events.borrow(), [BookmarksPopupEvent::Open { path: PathBuf::from("/p/src/b.ts"), line: Some(4) }]);
    }

    /// Without bookmarks the list says so and only Escape does anything.
    #[gpui_kit::test]
    async fn without_bookmarks_the_list_says_so(cx: &mut TestAppContext) {
        let (popup, events, cx) = open(cx, store_of(Vec::new()));
        assert!(lines(&popup, cx).is_empty());
        assert_eq!(popup.read_with(cx, |popup, _| popup.empty_text()), Some("No bookmarks"));
        assert_eq!(selected(&popup, cx), None);
        cx.simulate_keystrokes("down enter delete f2 pagedown end");
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [BookmarksPopupEvent::Dismissed]);
    }

    /// The text beside the list is the file of the selected bookmark, marked at its line; a folder has
    /// none, and text that arrives for a file the selection has left is dropped.
    #[gpui_kit::test]
    async fn the_text_beside_the_list_follows_the_selection(cx: &mut TestAppContext) {
        let (popup, _, cx) = open(cx, project());
        let preview = |cx: &mut VisualTestContext| {
            popup.read_with(cx, |popup, _| popup.preview_target().map(|(path, line)| (path.to_owned(), line)))
        };
        assert_eq!(preview(cx), None, "a folder has no text");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/README.md".to_owned(), None)));
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(2))));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty(), "nothing is read without a backend");

        popup.update(cx, |popup, cx| popup.preview_loaded("/p/README.md", "late", cx));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty());
        popup.update(cx, |popup, cx| popup.preview_loaded("/p/src/a.ts", "one\n\ttwo\r\nalpha()\n", cx));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_text()), ["one", "    two", "alpha()"]);
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/a.ts".to_owned(), Some(9))));
        assert_eq!(popup.read_with(cx, |popup, _| popup.preview_text()).len(), 3, "the same file stays shown");
        cx.simulate_keystrokes("down");
        assert_eq!(preview(cx), Some(("/p/src/b.ts".to_owned(), Some(4))));
        assert!(popup.read_with(cx, |popup, _| popup.preview_text()).is_empty());
    }

    // Not from a spec: every way a key can leave the selection where it is, and the page keys around
    // headers.
    #[test]
    fn navigation_skips_headers_and_never_goes_round() {
        let header = |key: &str| Row::File { key: key.to_owned(), path: key.to_owned() };
        let row = |key: &str| Row::Path { key: key.to_owned(), path: key.to_owned(), dir: false };
        let rows = [header("h0"), row("r1"), row("r2"), header("h3"), row("r4")];
        assert_eq!(navigate("down", &rows, Some(2)), Some(4));
        assert_eq!(navigate("up", &rows, Some(1)), Some(1));
        assert_eq!(navigate("down", &rows, Some(4)), Some(4));
        assert_eq!(navigate("down", &rows, None), Some(1));
        assert_eq!(navigate("up", &rows, None), Some(4));
        assert_eq!(navigate("up", &rows, Some(4)), Some(2));
        assert_eq!(navigate("home", &rows, Some(4)), Some(1));
        assert_eq!(navigate("end", &rows, Some(1)), Some(4));
        assert_eq!(navigate("pagedown", &rows, Some(1)), Some(4));
        assert_eq!(navigate("pageup", &rows, Some(4)), Some(1));
        assert_eq!(navigate("pageup", &rows, None), None);
        assert_eq!(navigate("pagedown", &rows, None), Some(4));
        assert_eq!(navigate("x", &rows, Some(1)), Some(1));
        assert_eq!(navigate("down", &[], None), None);
        assert_eq!(navigate("down", &[header("h0")], None), None);

        // A page lands on a header: Page Down takes the row above it, Page Up the one below.
        let mut long: Vec<Row> = (0..40).map(|index| row(&format!("r{index}"))).collect();
        long[PAGE_ROWS] = header("h");
        assert_eq!(navigate("pagedown", &long, Some(0)), Some(PAGE_ROWS - 1));
        assert_eq!(navigate("pagedown", &long, Some(PAGE_ROWS - 1)), Some(2 * PAGE_ROWS - 1));
        assert_eq!(navigate("pageup", &long, Some(2 * PAGE_ROWS)), Some(PAGE_ROWS + 1));
        assert_eq!(navigate("pageup", &long, Some(PAGE_ROWS + 1)), Some(1));
        assert_eq!(navigate("pagedown", &long, Some(39)), Some(39));
        assert_eq!(navigate("pageup", &long, Some(0)), Some(0));
    }

    // Not from a spec: the nearest bookmark after and before the caret, round the ends, and the first
    // and the last one without an editor.
    #[test]
    fn next_and_previous_bookmark_go_round_from_the_caret() {
        let items = [mark("/p/b", 4, ""), mark("/p/a", 7, ""), mark("/p/a", 2, ""), mark("/p/ab", 0, "")];
        let at = |path: Option<&str>, line: Option<u32>, forward: bool| {
            neighbour_bookmark(&items, path, line, forward).map(|bookmark| (bookmark.path.as_str(), bookmark.line))
        };
        assert_eq!(at(Some("/p/a"), Some(2), true), Some(("/p/a", 7)), "the bookmark at the caret is passed over");
        assert_eq!(at(Some("/p/a"), Some(7), true), Some(("/p/ab", 0)));
        assert_eq!(at(Some("/p/ab"), Some(3), true), Some(("/p/b", 4)));
        assert_eq!(at(Some("/p/b"), Some(4), true), Some(("/p/a", 2)), "past the last one comes the first");
        assert_eq!(at(Some("/p/a"), Some(5), false), Some(("/p/a", 2)));
        assert_eq!(at(Some("/p/a"), Some(2), false), Some(("/p/b", 4)), "before the first one comes the last");
        assert_eq!(at(Some("/p/b"), Some(0), false), Some(("/p/ab", 0)));
        assert_eq!(at(Some("/other"), Some(0), true), Some(("/p/a", 2)));
        assert_eq!(at(None, None, true), Some(("/p/a", 2)));
        assert_eq!(at(None, None, false), Some(("/p/b", 4)));
        assert_eq!(neighbour_bookmark(&[], Some("/p/a"), Some(1), true), None);
    }

    // Not from a spec: white space collapses, only the start of a long selection is kept and an empty
    // selection describes nothing.
    #[test]
    fn a_selection_describes_the_bookmark_set_on_it() {
        assert_eq!(selection_description("  fun  main()\n\t{ }  ").as_deref(), Some("fun main() { }"));
        assert_eq!(selection_description(""), None);
        assert_eq!(selection_description(" \n\t "), None);
        let long = "x".repeat(SELECTION_UNITS + 200);
        assert_eq!(selection_description(&long).map(|text| text.len()), Some(SELECTION_UNITS));
    }

    /// A typed letter gives the line's bookmark that mnemonic with the description, taking it from the
    /// bookmark that had it, and closes the chooser.
    #[gpui_kit::test]
    async fn a_typed_letter_assigns_that_mnemonic_with_the_description(cx: &mut TestAppContext) {
        let store = store_of(vec![Bookmark { mnemonic: Some('K'), ..mark("/p/b", 0, "b") }]);
        let (chooser, events, cx) = choose(cx, store);
        chooser.read_with(cx, |chooser, _| {
            assert_eq!(chooser.title(), "Add Mnemonic Bookmark");
            assert_eq!(chooser.assigned(), ['K']);
            assert_eq!(chooser.current_mnemonic(), None);
        });
        cx.simulate_keystrokes("tab");
        cx.simulate_input("entry");
        cx.simulate_keystrokes("tab");
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("k");
        assert!(matches!(events.borrow().as_slice(), [MnemonicChooserEvent::Done(writes)] if writes.len() == 1));
        let items = chooser.read_with(cx, |chooser, _| chooser.store().data().items.clone());
        let set = items.iter().find(|bookmark| bookmark.path == "/p/a").expect("the line is bookmarked");
        assert_eq!((set.line, set.mnemonic, set.description.as_deref()), (2, Some('K'), Some("entry")));
        assert_eq!(set.text, "fun a()");
        let other = items.iter().find(|bookmark| bookmark.path == "/p/b").expect("the other bookmark stays");
        assert_eq!(other.mnemonic, None);
    }

    /// A click on a key chooses it; a digit can be typed too, and a key pressed with Ctrl or the
    /// platform key is not a choice.
    #[gpui_kit::test]
    async fn a_key_is_chosen_by_click_or_by_typing_it_without_modifiers(cx: &mut TestAppContext) {
        let (chooser, events, cx) = choose(cx, store_of(vec![mark("/p/a", 2, "fun a()")]));
        assert_eq!(chooser.read_with(cx, |chooser, _| chooser.title()), "Assign Mnemonic");
        cx.simulate_keystrokes("ctrl-k cmd-7");
        assert!(events.borrow().is_empty());
        click(cx, "mnemonic-key-Q", 1);
        assert_eq!(events.borrow().len(), 1);
        chooser.read_with(cx, |chooser, _| {
            assert_eq!(chooser.current_mnemonic(), Some('Q'));
            assert_eq!(chooser.title(), "Change Mnemonic");
        });
        cx.simulate_keystrokes("7");
        assert_eq!(chooser.read_with(cx, |chooser, _| chooser.current_mnemonic()), Some('7'));
        assert_eq!(chooser.read_with(cx, |chooser, _| chooser.store().data().items.len()), 1);
        assert_eq!(events.borrow().len(), 2);
    }

    /// Enter in the description gives it to the line's bookmark, which keeps its mnemonic.
    #[gpui_kit::test]
    async fn enter_in_the_description_keeps_the_mnemonic_of_the_bookmark(cx: &mut TestAppContext) {
        let described = Bookmark { mnemonic: Some('M'), description: Some("old".to_owned()), ..mark("/p/a", 2, "x") };
        let (chooser, events, cx) = choose(cx, store_of(vec![described]));
        let field = chooser.read_with(cx, |chooser, _| chooser.description().clone());
        assert_eq!(typed(&field, cx), "old");
        cx.simulate_keystrokes("tab");
        assert!(cx.update(|window, cx| field.focus_handle(cx).is_focused(window)), "Tab goes to the description");
        cx.simulate_input("er");
        cx.simulate_keystrokes("enter");
        let item = chooser.read_with(cx, |chooser, _| chooser.store().data().items[0].clone());
        assert_eq!((item.mnemonic, item.description.as_deref()), (Some('M'), Some("older")));
        assert_eq!(events.borrow().len(), 1);
    }

    /// Enter in the description describes a bookmark that has no mnemonic: a letter typed there is text.
    #[gpui_kit::test]
    async fn enter_in_the_description_describes_a_plain_bookmark(cx: &mut TestAppContext) {
        let (chooser, events, cx) = choose(cx, store_of(vec![mark("/p/a", 2, "x")]));
        cx.simulate_keystrokes("tab");
        cx.simulate_input("k");
        cx.simulate_keystrokes("enter");
        let item = chooser.read_with(cx, |chooser, _| chooser.store().data().items[0].clone());
        assert_eq!((item.mnemonic, item.description.as_deref()), (None, Some("k")));
        assert!(matches!(events.borrow().as_slice(), [MnemonicChooserEvent::Done(writes)] if writes.len() == 1));
    }

    /// Enter in the description of a line without a bookmark sets none and leaves the chooser open.
    #[gpui_kit::test]
    async fn enter_in_the_description_sets_no_bookmark(cx: &mut TestAppContext) {
        let (chooser, events, cx) = choose(cx, store_of(Vec::new()));
        cx.simulate_keystrokes("tab");
        cx.simulate_input("note");
        cx.simulate_keystrokes("enter");
        assert!(chooser.read_with(cx, |chooser, _| chooser.store().data().items.is_empty()));
        assert!(events.borrow().is_empty());
        cx.simulate_keystrokes("tab escape");
        assert_eq!(*events.borrow(), [MnemonicChooserEvent::Dismissed]);
    }

    #[gpui_kit::test]
    async fn escape_dismisses_the_chooser(cx: &mut TestAppContext) {
        let (_chooser, events, cx) = choose(cx, store_of(Vec::new()));
        cx.simulate_keystrokes("escape");
        assert_eq!(*events.borrow(), [MnemonicChooserEvent::Dismissed]);
    }

    // Not from a spec: every design token the popups draw with is defined, as the kind it is used as.
    #[test]
    fn the_design_tokens_the_popups_use_exist() {
        let theme = theme::ActiveTheme::default().theme;
        let colors = [
            "fg",
            "fg-strong",
            "fg-muted",
            "fg-subtle",
            "fg-emphasis",
            "fg-editor",
            "fg-line-number",
            "fg-button-primary",
            "bg-popup",
            "bg-input",
            "bg-elev",
            "bg-editor",
            "bg-current-line",
            "bg-tree-selected",
            "bg-tree-hover",
            "bg-button",
            "bg-button-primary",
            "border",
            "border-popup",
            "border-strong",
            "border-button",
            "accent",
            "accent-soft",
        ];
        for name in colors {
            assert!(theme.color(name).is_some(), "--{name} is a colour");
        }
        for name in ["fs-ui", "fs-small", "fs-tiny", "fs-code", "lh-code", "radius-small"] {
            assert!(theme.px(name).is_some(), "--{name} is a length");
        }
    }
}
