//! What feeds an editor's gutter besides the breakpoints, the blame and the plugins: the change bars
//! against HEAD and their popup, the bookmarks, and the run marks of the run producers.
//!
//! - Change bars: the file's line hunks are asked of the backend (`vcs_line_hunks`, off the window's
//!   thread) shortly after the editor opens, after the repository was read again and once an edit has
//!   rested; in between they follow the edits ([`HunkState::map_through`]), and an answer computed for
//!   an older text is dropped ([`HunkFetcher`]). Without version control the bars go; a virtual
//!   document has none; an untracked file is all added lines, as the backend answers. A press on a bar
//!   opens the [`HunkPopup`] of its change.
//! - Bookmarks: the bookmarks column shows an icon, or the mnemonic, on each bookmarked line; the
//!   bookmarks follow the edits and their new lines are stored. ⌘-click (Ctrl-click) left of the fold
//!   chevrons toggles the line's bookmark.
//! - Run marks: a source the producers know gets a run icon on each test, suite or `main`, found again
//!   once an edit has rested; a `package.json` gets one on each of its scripts. A press opens the
//!   mark's Run / Debug menu.
//!
//! The icons are drawn over the window at the rows the active editor painted, like the HTTP Client's
//! run marks; the editor itself paints the change bars ([`Editor::set_lane_marks`]).

use super::Workbench;
use super::branches::{host_call, run_action_later};
use crate::editor::buffer::{self, Assoc, map_offset};
use crate::editor::view::gutter_marks::{GutterMarkEvent, LaneMark};
use crate::editor::view::{Editor, EditorEvent};
use crate::everywhere::bookmarks_store::{Mark, marks_of, set_file_lines};
use crate::platform::keymap::{has_primary_modifier, is_mac_host};
use crate::platform::local_time::now_ms;
use crate::platform::virtual_docs::is_virtual_uri;
use crate::run::targets::{RunTarget, target_language};
use crate::run_producers::npm_gutter::ScriptEntry;
use crate::theme;
use crate::ui::tooltip::WithTooltip as _;
use crate::vcs::hunk_popup::{HunkPopup, HunkPopupData, HunkPopupEvent};
use crate::vcs_diff::doc::{Direction, Doc};
use crate::vcs_diff::gutter::{EditorHunk, FetchStep, HunkFetcher, HunkState, color_token};
use gpui_kit::{
    AnyElement, App, Bounds, Context, Entity, EntityId, Focusable as _, FontWeight, MouseButton, MouseDownEvent,
    Pixels, Point, SharedString, Subscription, Task, WeakEntity, Window, anchored, deferred, div, point, prelude::*,
    px, size,
};
use ide_core::TextChange;
use ide_host::ops::vcs as git;
use serde_json::json;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

mod run_marks;

/// The bookmarks column: as wide as its icon and its padding, even while the file has no bookmark.
const BOOKMARKS_COLUMN: &str = "bookmarks";
const BOOKMARKS_ORDER: u8 = 50;
const BOOKMARKS_WIDTH: f32 = 15.0;
const BOOKMARK_ICON: f32 = 11.0;
/// The run marks' columns: the producers' marks left of the HTTP Client's, a package's scripts right of
/// them. A file that can have marks keeps the column while it has none, so the text does not move.
const RUN_COLUMN: &str = "run";
const RUN_ORDER: u8 = 55;
const SCRIPTS_COLUMN: &str = "run-npm";
const SCRIPTS_ORDER: u8 = 58;
const RUN_WIDTH: f32 = 14.0;
/// The setting ids of the gutter icon groups the columns belong to.
const RUN_GROUP: &str = "run";
const BOOKMARKS_GROUP: &str = "bookmarks";
/// How long an edit rests before the run marks are found again.
const TARGETS_DELAY: Duration = Duration::from_millis(350);
const SCRIPTS_DELAY: Duration = Duration::from_millis(300);
/// The action a change's Show Diff runs.
const SHOW_DIFF: &str = "vcs.showDiff";
/// The C and C++ sources whose `main` the producers mark.
const C_FAMILY: [&str; 4] = [".c", ".cpp", ".cc", ".cxx"];

/// The run marks of one editor, each with the offset of its line's start.
enum RunMarks {
    /// The file can have no run mark.
    None,
    Targets(Vec<(RunTarget, u32)>),
    Scripts(Vec<(ScriptEntry, u32)>),
}

/// The indexes of `marks` (sorted by line) whose lines are in `lines`.
fn marks_in<T>(marks: &[T], line: impl Fn(&T) -> u32, lines: &Range<u32>) -> Range<usize> {
    marks.partition_point(|mark| line(mark) < lines.start)..marks.partition_point(|mark| line(mark) < lines.end)
}

impl RunMarks {
    /// The indexes of the marks on `lines`; the marks are sorted by line.
    fn in_lines(&self, lines: &Range<u32>) -> Range<usize> {
        match self {
            Self::None => 0..0,
            Self::Targets(marks) => marks_in(marks, |(target, _)| target.line, lines),
            Self::Scripts(marks) => marks_in(marks, |(entry, _)| entry.line, lines),
        }
    }

    /// The line of the `index`-th mark.
    fn line(&self, index: usize) -> Option<u32> {
        match self {
            Self::None => None,
            Self::Targets(marks) => marks.get(index).map(|(target, _)| target.line),
            Self::Scripts(marks) => marks.get(index).map(|(entry, _)| entry.line),
        }
    }
}

/// An editor whose gutter the window feeds.
struct FollowedGutter {
    editor: WeakEntity<Editor>,
    path: String,
    /// The file gets change bars (it is not a virtual document).
    vcs: bool,
    hunks: HunkState,
    fetcher: HunkFetcher,
    hunk_timer: Option<Task<()>>,
    hunk_fetch: Option<Task<()>>,
    /// The file's bookmarks, by line, each with the offset of its line's start.
    bookmarks: Vec<(Mark, u32)>,
    run: RunMarks,
    run_search: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

/// The change the open hunk popup shows: its editor, and the change by its start now and at HEAD.
#[derive(Clone, Copy)]
struct PopupFor {
    editor: EntityId,
    start: u32,
    old_start: u32,
}

/// What the window keeps for the editors' gutters.
#[derive(Default)]
pub(super) struct EditorGutterSlot {
    followed: HashMap<EntityId, FollowedGutter>,
    /// Whether version control was available, and the repository state's version, when last seen.
    vcs_seen: Option<(bool, u64)>,
    /// Whether the run configurations were listed, and their ids, when the run marks were last found.
    configs_seen: Option<(bool, Vec<String>)>,
    /// The run store the run marks follow, and the subscription to it.
    configs_observer: Option<(EntityId, Subscription)>,
    popup: Option<PopupFor>,
}

/// The offset `from` (a line's start) after `changes`, at the start of the line it lands on; `None`
/// when `drop_deleted` and the character before it was deleted (the line was joined to the previous
/// one, or removed with the line break before it).
fn mapped_line_start(from: u32, changes: &[TextChange], drop_deleted: bool, rope: &ropey::Rope) -> Option<(u32, u32)> {
    if drop_deleted && changes.iter().any(|change| change.from < from && change.to >= from) {
        return None;
    }
    let at = map_offset(from, changes, Assoc::Before);
    let line = buffer::line_col(rope, at).0;
    Some((line, buffer::line_range(rope, line).start))
}

/// The lane's marks of `hunks`: one bar per changed line, a notch where lines were deleted.
fn lane_marks(hunks: &HunkState) -> Vec<LaneMark> {
    let marks = hunks.markers().iter();
    marks
        .map(|marker| LaneMark { from: marker.from as u32, color: color_token(marker.kind), shape: marker.shape })
        .collect()
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // Following editors

    /// An editor finished opening: its gutter gets the change bars, the bookmarks column and, for a
    /// file the producers know, the run marks' column.
    pub(super) fn editor_gutter_opened(
        &mut self,
        editor: &Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = editor.entity_id();
        if self.editor_gutter.followed.contains_key(&id) {
            return;
        }
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        let virtual_doc = is_virtual_uri(&path);
        let has_root = self.project_root(cx).is_some();
        let run = if virtual_doc || !has_root {
            RunMarks::None
        } else if path == "package.json" || path.ends_with("/package.json") {
            RunMarks::Scripts(Vec::new())
        } else if target_language(&path).is_some() || C_FAMILY.iter().any(|ext| path.ends_with(ext)) {
            RunMarks::Targets(Vec::new())
        } else {
            RunMarks::None
        };
        let subscriptions = vec![
            cx.subscribe_in(editor, window, move |workbench, _, event: &EditorEvent, window, cx| {
                if let EditorEvent::Edited { changes, .. } = event {
                    workbench.gutter_edited(id, changes, window, cx);
                }
            }),
            cx.subscribe_in(editor, window, move |workbench, _, event: &GutterMarkEvent, window, cx| {
                workbench.gutter_mark_event(id, *event, window, cx);
            }),
            cx.observe_release(editor, move |workbench, _, cx| {
                if let Some(mut followed) = workbench.editor_gutter.followed.remove(&id) {
                    followed.fetcher.destroy();
                }
                cx.notify();
            }),
        ];
        let followed = FollowedGutter {
            editor: editor.downgrade(),
            path,
            vcs: !virtual_doc,
            hunks: HunkState::default(),
            fetcher: HunkFetcher::default(),
            hunk_timer: None,
            hunk_fetch: None,
            bookmarks: Vec::new(),
            run,
            run_search: None,
            _subscriptions: subscriptions,
        };
        self.editor_gutter.followed.insert(id, followed);
        editor.update(cx, |editor, cx| editor.set_gutter_marks_enabled(true, cx));
        self.size_gutter_columns(id, cx);
        self.load_gutter_bookmarks(id, cx);
        self.watch_run_configs(window, cx);
        self.schedule_hunks(id, HunkFetcher::INITIAL_DELAY_MS, cx);
        self.find_run_marks(id, Duration::ZERO, cx);
    }

    /// Gives the editor `id` its icon columns: the bookmarks' always, the run marks' for a file that
    /// can have them; a group of gutter icons switched off in the settings takes no room.
    fn size_gutter_columns(&self, id: EntityId, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let settings = crate::platform::settings::effective(&self.label, cx).editor;
        let shown =
            |group: &str| settings.show_gutter_icons && !settings.gutter_icons_disabled.iter().any(|id| id == group);
        let bookmarks = if shown(BOOKMARKS_GROUP) { BOOKMARKS_WIDTH } else { 0.0 };
        let run = if shown(RUN_GROUP) { RUN_WIDTH } else { 0.0 };
        let column = match followed.run {
            RunMarks::None => None,
            RunMarks::Targets(_) => Some((RUN_COLUMN, RUN_ORDER)),
            RunMarks::Scripts(_) => Some((SCRIPTS_COLUMN, SCRIPTS_ORDER)),
        };
        editor.update(cx, |editor, cx| {
            editor.set_gutter_column(BOOKMARKS_COLUMN, BOOKMARKS_ORDER, bookmarks, cx);
            if let Some((column, order)) = column {
                editor.set_gutter_column(column, order, run, cx);
            }
        });
    }

    /// The settings changed: the icon columns follow Show gutter icons and the groups switched off.
    pub(super) fn editor_gutter_settings_changed(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<EntityId> = self.editor_gutter.followed.keys().copied().collect();
        for id in ids {
            self.size_gutter_columns(id, cx);
        }
        cx.notify();
    }

    /// The editor `id`'s text changed by `changes`: an open popup of its changes is stale, the bars,
    /// the bookmarks and the run marks move with their lines, and each is found again once the text
    /// rests.
    fn gutter_edited(&mut self, id: EntityId, changes: &[TextChange], window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_gutter.popup.is_some_and(|popup| popup.editor == id) {
            self.close_hunk_popup(window, cx);
        }
        let Some(followed) = self.editor_gutter.followed.get_mut(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let rope = editor.read(cx).rope().clone();
        let mut lane = None;
        let mut hunk_delay = None;
        if followed.vcs {
            hunk_delay = Some(followed.fetcher.doc_changed());
            if !followed.hunks.hunks().is_empty() {
                let doc = Doc::new(&rope.to_string());
                followed.hunks.map_through(changes, &doc);
                lane = Some(lane_marks(&followed.hunks));
            }
        }
        // The run marks follow their lines until they are found again; two landing on one line are one.
        let mut seen = Vec::new();
        let mut follow = |from: &mut u32, line: &mut u32| {
            let Some((to_line, to_from)) = mapped_line_start(*from, changes, false, &rope) else { return false };
            (*line, *from) = (to_line, to_from);
            if seen.contains(&to_line) {
                return false;
            }
            seen.push(to_line);
            true
        };
        let run_delay = match &mut followed.run {
            RunMarks::None => None,
            RunMarks::Targets(marks) => {
                marks.retain_mut(|(target, from)| follow(from, &mut target.line));
                Some(TARGETS_DELAY)
            }
            RunMarks::Scripts(marks) => {
                marks.retain_mut(|(entry, from)| follow(from, &mut entry.line));
                Some(SCRIPTS_DELAY)
            }
        };
        // A bookmark whose line was joined to the one above goes, as it does in the gutter's marks.
        let moved: Vec<(Mark, u32)> = followed
            .bookmarks
            .iter()
            .filter_map(|(mark, from)| {
                let (line, from) = mapped_line_start(*from, changes, true, &rope)?;
                Some((Mark { line, ..*mark }, from))
            })
            .collect();
        let lines_of = |marks: &[(Mark, u32)]| marks.iter().map(|(mark, _)| mark.line).collect::<Vec<u32>>();
        let bookmarks_moved = lines_of(&moved) != lines_of(&followed.bookmarks);
        followed.bookmarks.clone_from(&moved);
        let path = followed.path.clone();
        if let Some(lane) = lane {
            editor.update(cx, |editor, cx| editor.set_lane_marks(lane, cx));
        }
        if let Some(delay) = hunk_delay {
            self.schedule_hunks(id, delay, cx);
        }
        if let Some(delay) = run_delay {
            self.find_run_marks(id, delay, cx);
        }
        if bookmarks_moved {
            self.store_moved_bookmarks(&path, &moved, &rope, cx);
        }
        cx.notify();
    }

    /// Stores the lines the bookmarks of `path` moved to (`moved`, those that are left, in their order).
    fn store_moved_bookmarks(&mut self, path: &str, moved: &[(Mark, u32)], rope: &ropey::Rope, cx: &mut Context<Self>) {
        let texts: Vec<(u32, String)> =
            moved.iter().map(|(mark, _)| (mark.line, buffer::line_text(rope, mark.line))).collect();
        let lines: Vec<(u32, &str)> = texts.iter().map(|(line, text)| (*line, text.as_str())).collect();
        let now = now_ms() as u64;
        let writes = self.bookmarks.update(|items| set_file_lines(items, path, &lines, now));
        Self::save_bookmark_writes(writes, cx);
        self.sync_bookmarks_window(cx);
    }

    /// A press on the editor `id`'s gutter: a change bar opens its change, ⌘-click toggles a bookmark.
    fn gutter_mark_event(&mut self, id: EntityId, event: GutterMarkEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.editor_gutter.followed.get(&id).map(|followed| followed.path.clone()) else { return };
        match event {
            GutterMarkEvent::BookmarkToggled { line } => self.toggle_bookmark(Some((path, line)), cx),
            GutterMarkEvent::LanePressed { line, position } => {
                self.open_hunk_popup_at_line(id, line, position, window, cx);
            }
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Change bars

    /// Asks for the editor `id`'s hunks once `delay_ms` has passed without another reason to.
    fn schedule_hunks(&mut self, id: EntityId, delay_ms: u64, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get_mut(&id).filter(|followed| followed.vcs) else { return };
        followed.hunk_timer = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(delay_ms)).await;
            let _ = this.update(cx, |workbench, cx| workbench.hunks_due(id, cx));
        }));
    }

    /// The timer of the editor `id` fired: without version control its bars go, else its hunks are
    /// asked for the text the backend holds now (the editor's, unsaved edits included).
    fn hunks_due(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let available = self.vcs_state().is_some_and(|state| state.available);
        let label = self.label.clone();
        let Some(followed) = self.editor_gutter.followed.get_mut(&id) else { return };
        followed.hunk_timer = None;
        match followed.fetcher.timer_fired(available, !followed.hunks.hunks().is_empty()) {
            FetchStep::Idle => {}
            FetchStep::Clear => {
                followed.hunks = HunkState::default();
                self.show_lane(id, cx);
            }
            FetchStep::Request { ticket } => {
                let path = PathBuf::from(&followed.path);
                let call =
                    host_call(
                        label,
                        cx,
                        move |host, label| async move { git::vcs_line_hunks(&host, &label, path).await },
                    );
                followed.hunk_fetch = Some(cx.spawn(async move |this, cx| {
                    let answer = call.await;
                    let _ = this.update(cx, |workbench, cx| workbench.hunks_answered(id, ticket, answer, cx));
                }));
            }
        }
    }

    /// The backend answered the request `ticket` of the editor `id`: hunks for the text as it still is
    /// become its bars; an answer for an older text is dropped (a newer request is on its way); a file
    /// the backend cannot diff (outside the repository) keeps what it shows.
    fn hunks_answered(
        &mut self,
        id: EntityId,
        ticket: u64,
        answer: Result<Vec<ide_vcs::LineHunk>, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(followed) = self.editor_gutter.followed.get_mut(&id) else { return };
        followed.hunk_fetch = None;
        if !followed.fetcher.accepts(ticket) {
            return;
        }
        let hunks = match answer {
            Ok(hunks) => hunks,
            Err(error) => return tracing::debug!("no change markers for {}: {error}", followed.path),
        };
        let Some(editor) = followed.editor.upgrade() else { return };
        let doc = Doc::new(&editor.read(cx).text());
        followed.hunks.set_hunks(&hunks, &doc);
        self.show_lane(id, cx);
    }

    /// Hands the editor `id` the bars of its hunks.
    fn show_lane(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let marks = lane_marks(&followed.hunks);
        editor.update(cx, |editor, cx| editor.set_lane_marks(marks, cx));
    }

    /// The repository was read again: once its state changed, every editor asks for its hunks again.
    pub(super) fn editor_gutter_vcs_changed(&mut self, cx: &mut Context<Self>) {
        let seen = self.vcs_state().map(|state| (state.available, state.version));
        if seen == self.editor_gutter.vcs_seen {
            return;
        }
        self.editor_gutter.vcs_seen = seen;
        let ids: Vec<EntityId> = self.editor_gutter.followed.keys().copied().collect();
        for id in ids {
            self.schedule_hunks(id, HunkFetcher::VCS_DELAY_MS, cx);
        }
    }

    /// Whether `editor` still waits to ask for its hunks, or for the backend's answer.
    pub fn editor_gutter_hunks_pending(&self, editor: &Entity<Editor>) -> bool {
        let followed = self.editor_gutter.followed.get(&editor.entity_id());
        followed.is_some_and(|followed| followed.hunk_timer.is_some() || followed.hunk_fetch.is_some())
    }

    /// The hunks of the editor `id` and the document's text as one [`Doc`], with its folds as offsets.
    fn hunk_context(&self, id: EntityId, cx: &App) -> Option<(Entity<Editor>, Doc, Vec<Range<usize>>)> {
        let followed = self.editor_gutter.followed.get(&id)?;
        let editor = followed.editor.upgrade()?;
        let (doc, folds) = {
            let shown = editor.read(cx);
            let folds = shown.folded_regions().iter().map(|&(from, to)| from as usize..to as usize).collect();
            (Doc::new(&shown.text()), folds)
        };
        Some((editor, doc, folds))
    }

    /// Opens the popup of the change drawn on the editor `id`'s `line` (a folded line: of a change it
    /// hides), below its last line and right of the lane.
    fn open_hunk_popup_at_line(
        &mut self,
        id: EntityId,
        line: u32,
        pressed: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((editor, doc, folds)) = self.hunk_context(id, cx) else { return };
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let line_from = buffer::line_range(editor.read(cx).rope(), line).start as usize;
        let Some(hunk) = followed.hunks.hunk_at_pos(&doc, &folds, line_from) else { return };
        let (start, old_start) = (hunk.hunk.start, hunk.hunk.old_start);
        self.show_hunk_popup(id, start, old_start, Some(pressed), window, cx);
    }

    /// Shows the popup of the editor `id`'s change that starts at `start` (and at `old_start` at HEAD);
    /// `pressed` is where a press opened it, should its line not be painted.
    fn show_hunk_popup(
        &mut self,
        id: EntityId,
        start: u32,
        old_start: u32,
        pressed: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((editor, doc, _)) = self.hunk_context(id, cx) else { return };
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let all = followed.hunks.hunks();
        let index = all.iter().position(|h| h.hunk.start == start && h.hunk.old_start == old_start);
        let Some(hunk) = index.and_then(|index| all.get(index)) else { return };
        let current = doc.slice_string(hunk.from, hunk.to);
        let data = HunkPopupData::of(hunk.hunk.kind, &hunk.hunk.old_text, &current, index, all.len());
        // Below the change's last line (on top of the line after a deletion), right of the lane.
        let has_text = hunk.to > hunk.from;
        let end = if has_text { hunk.from.max(hunk.to - 1) } else { hunk.from.min(doc.len()) };
        // The row's top follows the scroll at once, so a popup opened right after the caret moved
        // there needs no new frame; a painted line's start gives the text's left edge.
        let at = {
            let shown = editor.read(cx);
            let line = doc.line_at(end).number.saturating_sub(1) as u32;
            let line_height = shown.line_height(cx);
            let text = shown.painted_lines().and_then(|painted| shown.position_of(shown.offset_of(painted.start, 0)));
            match (text, shown.painted_bounds(), shown.change_lane(cx)) {
                (Some(text), Some(bounds), Some((left, width))) => {
                    let top = bounds.top() + line_height * shown.row_of_line(line) as f32 - shown.scroll_top();
                    let y = if has_text { top + line_height } else { top };
                    point(text.x + left + width, y + px(2.0))
                }
                _ => pressed.map_or(point(px(0.0), px(0.0)), |pressed| point(pressed.x, pressed.y + px(2.0))),
            }
        };
        let view = cx.new(|cx| HunkPopup::new(data, cx));
        let events = cx.subscribe_in(&view, window, move |workbench, _, event: &HunkPopupEvent, window, cx| {
            workbench.hunk_popup_event(*event, window, cx);
        });
        self.open_popup_at(&view, events, at, window, cx);
        self.editor_gutter.popup = Some(PopupFor { editor: id, start, old_start });
    }

    /// The hunk popup being shown, while it is.
    pub fn hunk_popup(&self) -> Option<Entity<HunkPopup>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast::<HunkPopup>().ok())
    }

    fn close_hunk_popup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor_gutter.popup = None;
        if self.hunk_popup().is_some() {
            self.close_popup(window, cx);
        }
    }

    /// Carries out what the hunk popup asks.
    fn hunk_popup_event(&mut self, event: HunkPopupEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(shown) = self.editor_gutter.popup else { return self.close_hunk_popup(window, cx) };
        match event {
            HunkPopupEvent::Close => self.close_hunk_popup(window, cx),
            HunkPopupEvent::Rollback => self.roll_back_hunk(shown, window, cx),
            HunkPopupEvent::Previous => self.step_hunk(shown, Direction::Previous, window, cx),
            HunkPopupEvent::Next => self.step_hunk(shown, Direction::Next, window, cx),
            HunkPopupEvent::ShowDiff => {
                let path = self.editor_gutter.followed.get(&shown.editor).map(|followed| followed.path.clone());
                self.close_hunk_popup(window, cx);
                if let Some(path) = path {
                    let arg = json!({ "path": path, "line": shown.start });
                    run_action_later(SHOW_DIFF.to_owned(), Some(arg), window, cx);
                }
            }
        }
    }

    /// Rollback: the change's lines become its text at HEAD, as an edit of the editor; the carets move
    /// with it and the editor gets the keyboard back.
    fn roll_back_hunk(&mut self, shown: PopupFor, window: &mut Window, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get(&shown.editor) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let same = |h: &&EditorHunk| h.hunk.start == shown.start && h.hunk.old_start == shown.old_start;
        let live = followed.hunks.hunks().iter().find(same);
        let restore =
            |h: &EditorHunk| TextChange { from: h.from as u32, to: h.to as u32, insert: h.hunk.old_text.clone() };
        let Some(change) = live.map(restore) else { return self.close_hunk_popup(window, cx) };
        self.close_hunk_popup(window, cx);
        editor.update(cx, |editor, cx| {
            let changes = std::slice::from_ref(&change);
            let selections = editor
                .selections()
                .iter()
                .map(|&(anchor, head)| {
                    (map_offset(anchor, changes, Assoc::Before), map_offset(head, changes, Assoc::Before))
                })
                .collect();
            let main = editor.main_index();
            if !editor.apply_edit(changes, selections, main, cx) {
                tracing::info!("the change in {} was not rolled back", editor.path().display());
            }
        });
        window.focus(&editor.focus_handle(cx), cx);
    }

    /// Previous / Next Change from the popup: the caret goes to that change, centred, and its popup
    /// opens there; without another change the popup closes.
    fn step_hunk(&mut self, shown: PopupFor, direction: Direction, window: &mut Window, cx: &mut Context<Self>) {
        let Some((editor, doc, _)) = self.hunk_context(shown.editor, cx) else { return };
        let Some(followed) = self.editor_gutter.followed.get(&shown.editor) else { return };
        let head = editor.read(cx).caret() as usize;
        let Some((offset, target)) = followed.hunks.goto_change(&doc, head, direction) else {
            return self.close_hunk_popup(window, cx);
        };
        let (start, old_start) = (target.hunk.start, target.hunk.old_start);
        self.close_hunk_popup(window, cx);
        let offset = offset as u32;
        editor.update(cx, |editor, cx| {
            editor.select(offset, offset, cx);
            editor.scroll_caret_to_center(cx);
        });
        window.focus(&editor.focus_handle(cx), cx);
        self.show_hunk_popup(shown.editor, start, old_start, None, window, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Bookmarks

    /// Reads the editor `id`'s bookmarks from the window's bookmarks.
    fn load_gutter_bookmarks(&mut self, id: EntityId, cx: &mut Context<Self>) {
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return };
        let Some(editor) = followed.editor.upgrade() else { return };
        let marks = marks_of(&self.bookmarks.data().items, &followed.path);
        let shown = editor.read(cx);
        let lines = shown.line_count();
        let mut placed: Vec<(Mark, u32)> = Vec::with_capacity(marks.len());
        for mark in marks.into_iter().filter(|mark| mark.line < lines) {
            if placed.last().is_none_or(|(last, _)| last.line != mark.line) {
                placed.push((mark, buffer::line_range(shown.rope(), mark.line).start));
            }
        }
        if let Some(followed) = self.editor_gutter.followed.get_mut(&id) {
            followed.bookmarks = placed;
        }
    }

    /// The window's bookmarks changed: every editor shows its file's again.
    pub(super) fn editor_gutter_bookmarks_changed(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<EntityId> = self.editor_gutter.followed.keys().copied().collect();
        for id in ids {
            self.load_gutter_bookmarks(id, cx);
        }
        cx.notify();
    }

    /// The bookmarks the gutter of `editor` shows, by line.
    pub fn editor_gutter_bookmarks(&self, editor: &Entity<Editor>) -> Vec<Mark> {
        let followed = self.editor_gutter.followed.get(&editor.entity_id());
        followed.map(|followed| followed.bookmarks.iter().map(|(mark, _)| *mark).collect()).unwrap_or_default()
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    /// The bookmarks and the run marks of the active editor, over the window at their lines (only the
    /// rows the editor painted, inside it). Each looks up the marks of the painted lines only.
    pub(super) fn editor_gutter_layer(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(editor) = self.editor().cloned() else { return Vec::new() };
        let id = editor.entity_id();
        let Some(followed) = self.editor_gutter.followed.get(&id) else { return Vec::new() };
        if super::distraction_free_active(cx) {
            return Vec::new();
        }
        let (rows, line_height, bookmarks_column, run_column) = {
            let shown = editor.read(cx);
            let Some(painted) = shown.painted_lines() else { return Vec::new() };
            let line_height = shown.line_height(cx);
            let first = painted.start;
            let top = shown
                .position_of(shown.offset_of(first, 0))
                .map(|at| at.y - line_height * shown.row_of_line(first) as f32 + shown.scroll_top());
            let bottom = top.map(|top| top + line_height * shown.painted_rows().saturating_sub(1) as f32);
            let place = |line: u32| {
                let at = shown.position_of(shown.offset_of(line, 0))?;
                let inside =
                    top.is_some_and(|top| at.y >= top) && bottom.is_some_and(|bottom| at.y + line_height <= bottom);
                inside.then_some(at)
            };
            // The marks are sorted by line: only those of the painted lines are looked at.
            let bookmark_rows: Vec<(usize, u32, Point<Pixels>)> =
                marks_in(&followed.bookmarks, |(mark, _)| mark.line, &painted)
                    .filter_map(|index| {
                        let line = followed.bookmarks[index].0.line;
                        place(line).map(|at| (index, line, at))
                    })
                    .collect();
            let run_rows: Vec<(usize, u32, Point<Pixels>)> = followed
                .run
                .in_lines(&painted)
                .filter_map(|index| {
                    let line = followed.run.line(index)?;
                    place(line).map(|at| (index, line, at))
                })
                .collect();
            let run_column = match followed.run {
                RunMarks::Targets(_) => shown.gutter_column(RUN_COLUMN, cx),
                RunMarks::Scripts(_) => shown.gutter_column(SCRIPTS_COLUMN, cx),
                RunMarks::None => None,
            };
            let rows = (bookmark_rows, run_rows);
            (rows, line_height, shown.gutter_column(BOOKMARKS_COLUMN, cx), run_column)
        };
        let (bookmark_rows, run_rows) = rows;
        let mut elements = Vec::with_capacity(bookmark_rows.len() + run_rows.len());
        if let Some((column, width)) = bookmarks_column {
            for (index, line, at) in bookmark_rows {
                let mark = followed.bookmarks[index].0;
                let cell = Bounds::new(point(at.x + column, at.y), size(width, line_height));
                elements.push(self.bookmark_mark(id, line, mark, cell, cx));
            }
        }
        if let Some((column, width)) = run_column {
            for (index, line, at) in run_rows {
                let (icon, title) = self.run_mark_look(followed, index, cx);
                let icon_size = if icon == "gutter-run" { 12.0 } else { RUN_WIDTH };
                let corner =
                    point(at.x + column + (width - px(RUN_WIDTH)) / 2.0, at.y + (line_height - px(RUN_WIDTH)) / 2.0);
                let mark = div()
                    .id(SharedString::from(format!("run-gutter-{line}")))
                    .debug_selector(move || format!("run-gutter-{line}"))
                    .with_tooltip(title)
                    .size(px(RUN_WIDTH))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .cursor_pointer()
                    .occlude()
                    .child(theme::icon(cx, icon, icon_size, theme::color(cx, "success")))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |workbench, _: &MouseDownEvent, window, cx| {
                            // The press is the mark's: the editor neither moves its caret nor toggles a
                            // breakpoint.
                            window.prevent_default();
                            cx.stop_propagation();
                            let below = point(corner.x, corner.y + px(RUN_WIDTH) + px(2.0));
                            workbench.run_mark_pressed(id, line, below, window, cx);
                        }),
                    );
                elements.push(deferred(anchored().position(corner).child(mark)).into_any_element());
            }
        }
        elements
    }

    /// The bookmark of `line` drawn in `cell` (its column on its row): the mnemonic in a small chip,
    /// else the bookmark icon. ⌘-click on it toggles the bookmark; a plain press is the mark's and does
    /// nothing else.
    fn bookmark_mark(
        &self,
        id: EntityId,
        line: u32,
        mark: Mark,
        cell: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let accent = theme::color(cx, "accent");
        let content = match mark.mnemonic {
            Some(mnemonic) => div()
                .min_w(px(BOOKMARK_ICON))
                .h(px(12.0))
                .px(px(1.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(2.0))
                .border_1()
                .border_color(accent)
                .bg(theme::color(cx, "accent-soft"))
                .text_color(theme::color(cx, "fg-strong"))
                .font_family(theme::installed_fonts::ui_family(cx))
                .text_size(px(9.0))
                .font_weight(FontWeight::SEMIBOLD)
                .debug_selector(move || format!("bookmark-mnemonic-{line}"))
                .child(SharedString::from(mnemonic.to_string()))
                .into_any_element(),
            None => theme::icon(cx, "bookmark", BOOKMARK_ICON, accent),
        };
        let mark = div()
            .id(SharedString::from(format!("bookmark-gutter-{line}")))
            .debug_selector(move || format!("bookmark-gutter-{line}"))
            .w(cell.size.width)
            .h(cell.size.height)
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .child(content)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |workbench, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    let held = event.modifiers;
                    if has_primary_modifier(&held, is_mac_host()) && !held.shift && !held.alt {
                        workbench.gutter_mark_event(id, GutterMarkEvent::BookmarkToggled { line }, window, cx);
                    }
                }),
            );
        deferred(anchored().position(cell.origin).child(mark)).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    /// A line's start moves with an insertion above it; with deletions tracked, joining it to the line
    /// above drops it, and an edit inside the line keeps it.
    #[test]
    fn a_marks_line_start_follows_the_edits_and_a_joined_line_loses_its_bookmark() {
        // "a\nb\nc\n": line 1 starts at 2.
        let inserted = Rope::from_str("x\na\nb\nc\n");
        assert_eq!(mapped_line_start(2, &[change(0, 0, "x\n")], true, &inserted), Some((2, 4)));
        let joined = Rope::from_str("ab\nc\n");
        assert_eq!(mapped_line_start(2, &[change(1, 2, "")], true, &joined), None);
        assert_eq!(mapped_line_start(2, &[change(1, 2, "")], false, &joined), Some((0, 0)));
        let typed = Rope::from_str("a\nbz\nc\n");
        assert_eq!(mapped_line_start(2, &[change(3, 3, "z")], true, &typed), Some((1, 2)));
    }
}
