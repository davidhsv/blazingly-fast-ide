//! The code preview beside a list of results (Find in Files, Bookmarks, Find Usages, Show Usages): a
//! second editor on the document of the chosen result's file, in the editor's font and colours, with
//! line numbers, the result's line tinted and scrolled to the middle and the matches marked.
//!
//! The editor shares the document with the tabs of the file: an edit made in either shows in both, and
//! saving saves the document. An editable preview stays read-only for a document that is (a library
//! source, an archive entry, a binary or large file). The document is opened off the window's thread once
//! the selection rests; an answer for a result the selection has left closes its document again. Only
//! the lines in view are shaped and painted, as in any editor.
//!
//! While the preview has the keyboard it edits, undoes, copies and saves its own text, so the shortcuts
//! of editing actions are kept out (the IDE's other shortcuts still run); Escape is handed to the owner,
//! which closes the popup. A preview with unsaved
//! edits that goes away keeps its editor until the document is saved or the preview shows the file
//! again, so the edits and their undo stay.

pub mod line_tokens;

use crate::backend::Backend;
use crate::editor::buffer;
use crate::editor::marks::{Mark, MarkKind, MarkLayer};
use crate::editor::view::Editor;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, Entity, EntityId, EventEmitter, Focusable, Global, IntoElement, KeyDownEvent, Render, SharedString,
    Subscription, WeakEntity, Window, div, prelude::*,
};
use ropey::Rope;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// How long the selection rests on a result before its file is opened: arrowing through the results
/// opens none of the files passed over.
pub const PREVIEW_DEBOUNCE: Duration = Duration::from_millis(80);
/// The rest of the Show Usages popup, whose list is walked faster.
pub const POPUP_PREVIEW_DEBOUNCE: Duration = Duration::from_millis(60);

/// The ranges marked on one line: UTF-16 columns of the line as it is in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewHighlight {
    /// 0-based.
    pub line: u32,
    pub ranges: Vec<(u32, u32)>,
}

/// What the preview shows: the file at `path`, on the 0-based `line`, with `highlights` marked. A new
/// `version` opens the file again (after a Replace changed it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewTarget {
    pub path: String,
    pub line: u32,
    pub highlights: Vec<PreviewHighlight>,
    pub version: u64,
}

/// Whether the preview edits the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    /// Typing edits the document, unless it is read-only.
    Editable,
    /// The text can be selected and copied, never edited.
    ReadOnly,
}

/// How a range of [`preview_marks`] is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreviewMarkKind {
    /// The previewed line: its row is tinted. The range is the line's start, empty.
    Line,
    /// A match on the previewed line.
    CurrentMatch,
    /// A match on another line.
    Match,
}

/// The marks of the preview of `text` on the 0-based `line` with `highlights`, as UTF-16 offsets of the
/// text, in document order: the line, then each match, the ones on `line` current. Columns past a line's
/// end are clamped to it, lines past the text's end and empty ranges are left out.
pub fn preview_marks(text: &Rope, line: u32, highlights: &[PreviewHighlight]) -> Vec<(Range<u32>, PreviewMarkKind)> {
    let lines = buffer::line_count(text);
    let mut out = Vec::new();
    if line < lines {
        let from = buffer::line_range(text, line).start;
        out.push((from..from, PreviewMarkKind::Line));
    }
    for highlight in highlights.iter().filter(|highlight| highlight.line < lines) {
        let range = buffer::line_range(text, highlight.line);
        let len = range.end - range.start;
        let kind = if highlight.line == line { PreviewMarkKind::CurrentMatch } else { PreviewMarkKind::Match };
        for &(a, b) in &highlight.ranges {
            let (from, to) = (range.start + a.min(len), range.start + b.min(len));
            if to > from {
                out.push((from..to, kind));
            }
        }
    }
    out.sort_by_key(|(range, kind)| (range.start, *kind, range.end));
    out
}

/// [`preview_marks`] as the editor draws them: the line's row tinted, the matches on the search colours.
/// A read-only preview marks every usage alike; an editable one tells the current line's matches apart.
fn editor_marks(text: &Rope, line: u32, highlights: &[PreviewHighlight], mode: PreviewMode) -> Vec<Mark> {
    let mark = |(range, kind): (Range<u32>, PreviewMarkKind)| match kind {
        PreviewMarkKind::Line => Mark::new(buffer::line_range(text, line), MarkKind::CurrentLine),
        PreviewMarkKind::CurrentMatch if mode == PreviewMode::Editable => {
            Mark::new(range, MarkKind::CurrentFindOccurrence)
        }
        PreviewMarkKind::CurrentMatch | PreviewMarkKind::Match => Mark::new(range, MarkKind::FindOccurrence),
    };
    preview_marks(text, line, highlights).into_iter().map(mark).collect()
}

/// What the preview tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodePreviewEvent {
    /// Escape was pressed in the preview: the owner closes what it shows the preview in.
    Escape,
    /// Saving from the preview failed: the reason, for a notification.
    SaveFailed(String),
}

/// The preview's editor and what it was made for.
struct Shown {
    path: String,
    version: u64,
    editor: Entity<Editor>,
    /// The claim that keeps the shortcuts of editing actions out while the editor has the keyboard.
    shield: u64,
}

/// The editors of the previews: the ones shown now, by preview, and the ones with unsaved edits whose
/// preview went away, by window and path (shown again when their file is).
#[derive(Default)]
struct PreviewEditors {
    shown: HashMap<EntityId, (String, WeakEntity<Editor>)>,
    /// With the watch that lets a held editor go once its document is saved, by Save All, autosave or a
    /// tab of the same document.
    held: HashMap<(String, String), (Entity<Editor>, Subscription)>,
    /// Windows whose project is being left: their previews go without being held or saved, since the
    /// question asked before leaving decided about their edits (and autosave ran before it).
    leaving: HashSet<String>,
}

impl Global for PreviewEditors {}

/// Keeps `editor` when its document has unsaved edits, so they survive the preview going away; drops it
/// otherwise, which lets its document go. With Files › Autosave on, the edits are saved now, as when the
/// last tab of a document closes, a failure told to the user; the editor stays held until they are. A
/// document the window's project no longer has open (the project closed, or another one opened in the
/// window) is not held: the question asked before the project went decided about it.
fn hold_or_release(label: &str, path: &str, editor: Entity<Editor>, cx: &mut App) {
    let leaving = cx.try_global::<PreviewEditors>().is_some_and(|editors| editors.leaving.contains(label));
    let Some(project) = cx.try_global::<Backend>().and_then(|backend| backend.host.project(label).ok()) else {
        return;
    };
    let doc = editor.read(cx).doc();
    // Dropping an editor whose document the backend knows to be changed would drop the change with it.
    let dirty = unsaved(&editor, label, cx);
    let open = project.documents.by_path(editor.read(cx).path()) == Some(doc);
    if !dirty || leaving || !open {
        return;
    }
    if crate::platform::settings::effective(label, cx).files.auto_save {
        let saving = editor.read(cx).save(cx);
        let window = label.to_owned();
        cx.spawn(async move |cx| {
            let failure = match saving.await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error.message,
                Err(error) => error.to_string(),
            };
            cx.update(|cx| crate::workbench::report_save_failure_in(&window, failure, cx));
        })
        .detach();
    }
    let key = (label.to_owned(), path.to_owned());
    let watched = key.clone();
    let window = label.to_owned();
    let saved = cx.observe(&editor, move |editor, cx| {
        if !unsaved(&editor, &window, cx) {
            let key = watched.clone();
            cx.defer(move |cx| release_saved(&key, cx));
        }
    });
    cx.default_global::<PreviewEditors>().held.insert(key, (editor, saved));
}

/// Whether the document of `editor` (of the window `label`) has changes that are not on disk, as the editor
/// knows it or, sooner, as the backend does: an edit the backend made itself (a Replace) reaches the
/// editor's own flag only with the backend's next event.
fn unsaved(editor: &Entity<Editor>, label: &str, cx: &App) -> bool {
    let editor = editor.read(cx);
    editor.is_dirty()
        || cx
            .try_global::<Backend>()
            .and_then(|backend| backend.host.project(label).ok())
            .is_some_and(|project| project.documents.is_dirty(editor.doc()).unwrap_or(false))
}

/// Lets the held editor of `key` go when its document has no unsaved edits any more: nothing is left to
/// keep, and its document closes unless something else holds it.
fn release_saved(key: &(String, String), cx: &mut App) {
    let saved = cx
        .try_global::<PreviewEditors>()
        .and_then(|editors| editors.held.get(key))
        .is_some_and(|(editor, _)| !unsaved(editor, &key.0, cx));
    if saved {
        cx.default_global::<PreviewEditors>().held.remove(key);
    }
}

/// The held preview of `path`, taken out of the held ones. The held previews of the window whose documents
/// were saved meanwhile, or that the window's project no longer has open (another project was opened in
/// it), are dropped first.
fn take_held(label: &str, path: &str, cx: &mut App) -> Option<Entity<Editor>> {
    let held = cx.try_global::<PreviewEditors>()?;
    let project = cx.try_global::<Backend>().and_then(|backend| backend.host.project(label).ok());
    let gone: Vec<(String, String)> = held
        .held
        .iter()
        .filter(|((window, _), (editor, _))| {
            let dirty = unsaved(editor, window, cx);
            let editor = editor.read(cx);
            let open_doc = project.as_ref().and_then(|project| project.documents.by_path(editor.path()));
            let open = open_doc == Some(editor.doc());
            window == label && (!dirty || !open)
        })
        .map(|(key, _)| key.clone())
        .collect();
    let held = &mut cx.default_global::<PreviewEditors>().held;
    for key in gone {
        held.remove(&key);
    }
    held.remove(&(label.to_owned(), path.to_owned())).map(|(editor, _)| editor)
}

/// The editors of the window `label`'s previews, shown or held with unsaved edits: saving every document,
/// autosave and the question on closing the window or the project count them with the editors of the tabs.
pub fn preview_editors(label: &str, cx: &App) -> Vec<Entity<Editor>> {
    let Some(editors) = cx.try_global::<PreviewEditors>() else { return Vec::new() };
    let shown = editors.shown.values().filter(|(window, _)| window == label).filter_map(|(_, editor)| editor.upgrade());
    let held = editors.held.iter().filter(|((window, _), _)| window == label).map(|(_, (editor, _))| editor.clone());
    shown.chain(held).collect()
}

/// The window `label` leaves its project: the held previews go, closing their documents, and the previews
/// that go with the popups are neither held nor saved. The question asked before leaving decided what
/// happens to their edits. The next preview shown in the window ends this.
pub fn leave(label: &str, cx: &mut App) {
    let editors = cx.default_global::<PreviewEditors>();
    editors.held.retain(|(window, _), _| window != label);
    editors.leaving.insert(label.to_owned());
}

pub struct CodePreview {
    /// This preview, as [`PreviewEditors`] knows its editor.
    id: EntityId,
    /// The window, as the backend knows it.
    window_label: String,
    mode: PreviewMode,
    /// Shown while nothing is chosen, untranslated.
    empty_text: &'static str,
    debounce: Duration,
    target: Option<PreviewTarget>,
    /// The file and version being opened, and the number of that request.
    wanted: Option<(String, u64, u64)>,
    requests: u64,
    shown: Option<Shown>,
    /// Why the file last asked for cannot be shown.
    error: Option<String>,
    /// The file and line the editor was last centred on.
    revealed: Option<(String, u32)>,
    /// The editor waits for its first frame to be centred on the line.
    center_pending: bool,
}

impl EventEmitter<CodePreviewEvent> for CodePreview {}

impl CodePreview {
    /// A preview for the window `window_label`; `empty_text` (an English text of the catalog) shows while
    /// nothing is chosen.
    pub fn new(
        window_label: &str,
        mode: PreviewMode,
        empty_text: &'static str,
        debounce: Duration,
        cx: &mut Context<Self>,
    ) -> Self {
        // The editor shown goes with the preview, held while its document has unsaved edits.
        cx.on_release(|preview, cx| preview.release_shown(cx)).detach();
        Self {
            id: cx.entity_id(),
            window_label: window_label.to_owned(),
            mode,
            empty_text,
            debounce,
            target: None,
            wanted: None,
            requests: 0,
            shown: None,
            error: None,
            revealed: None,
            center_pending: false,
        }
    }

    /// Shows `target`, or the empty text without one. The file shown already only moves to the new line
    /// and marks; another file is opened once the selection has rested on it.
    pub fn show(&mut self, target: Option<PreviewTarget>, cx: &mut Context<Self>) {
        let Some(target) = target else {
            self.target = None;
            self.wanted = None;
            self.error = None;
            self.release_shown(cx);
            cx.notify();
            return;
        };
        let current =
            self.shown.as_ref().is_some_and(|shown| shown.path == target.path && shown.version == target.version);
        self.target = Some(target.clone());
        if current {
            self.wanted = None;
            self.apply_target(cx);
            cx.notify();
            return;
        }
        if self.wanted.as_ref().is_some_and(|(path, version, _)| *path == target.path && *version == target.version) {
            return;
        }
        self.requests += 1;
        let request = self.requests;
        self.wanted = Some((target.path.clone(), target.version, request));
        cx.notify();
        self.open(target.path, request, cx);
    }

    /// The file shown, once its editor is there and it is the one asked for.
    pub fn path(&self) -> Option<&str> {
        self.current().map(|shown| shown.path.as_str())
    }

    /// The editor of the file shown.
    pub fn editor(&self) -> Option<&Entity<Editor>> {
        self.current().map(|shown| &shown.editor)
    }

    /// The line the preview is on, and its text in the editor.
    pub fn line(&self, cx: &App) -> Option<(u32, String)> {
        let (shown, target) = (self.current()?, self.target.as_ref()?);
        let editor = shown.editor.read(cx);
        (target.line < editor.line_count()).then(|| (target.line, editor.line_text(target.line)))
    }

    /// The lines of the text shown, as the editor has them now.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let Some(shown) = self.current() else { return Vec::new() };
        shown.editor.read(cx).text().lines().map(str::to_owned).collect()
    }

    /// What shows in place of the editor: the empty text while nothing is chosen, why the file cannot be
    /// shown, or nothing.
    pub fn note(&self, cx: &mut App) -> Option<String> {
        match (&self.target, &self.error) {
            (None, _) => Some(cx.default_global::<Localization>().0.tx(self.empty_text)),
            (Some(_), Some(error)) if self.current().is_none() => Some(error.clone()),
            _ => None,
        }
    }

    /// The editor, when it shows the file of the target as asked.
    fn current(&self) -> Option<&Shown> {
        let target = self.target.as_ref()?;
        self.shown.as_ref().filter(|shown| shown.path == target.path && shown.version == target.version)
    }

    /// Opens the document of `path` for request `request` once the selection has rested, and shows it when
    /// it is still the one asked for; an answer for a request overtaken meanwhile closes its document.
    fn open(&mut self, path: String, request: u64, cx: &mut Context<Self>) {
        let debounce = self.debounce;
        cx.spawn(async move |preview, cx| {
            cx.background_executor().timer(debounce).await;
            let asked = preview.update(cx, |preview, cx| {
                if preview.wanted.as_ref().is_none_or(|(_, _, wanted)| *wanted != request) {
                    return None;
                }
                let label = preview.window_label.clone();
                if let Some(editor) = take_held(&label, &path, cx) {
                    preview.install(path.clone(), editor, cx);
                    return None;
                }
                let backend = cx.try_global::<Backend>()?;
                let file = PathBuf::from(&path);
                let opening = {
                    let label = label.clone();
                    backend.with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, file).await })
                };
                Some((label, opening))
            });
            let Ok(Some((label, opening))) = asked else { return };
            let info = match opening.await {
                Ok(Ok(info)) => info,
                Ok(Err(error)) => return failed(&preview, request, error.message, cx),
                Err(error) => return failed(&preview, request, error.to_string(), cx),
            };
            let shown = preview.update(cx, |preview, cx| {
                if preview.wanted.as_ref().is_none_or(|(_, _, wanted)| *wanted != request) {
                    return false;
                }
                let host = cx.global::<Backend>().host.clone();
                let snapshot = host.project(&label).and_then(|project| project.documents.rope_snapshot(info.id));
                let (version, rope) = match snapshot {
                    Ok(snapshot) => snapshot,
                    Err(error) => {
                        preview.wanted = None;
                        preview.error = Some(error.message);
                        cx.notify();
                        return false;
                    }
                };
                let read_only = preview.mode == PreviewMode::ReadOnly || info.read_only;
                let info = ide_workspace::DocumentInfo { read_only, ..info.clone() };
                // The editor holds the document it was opened for and closes it when it goes.
                let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                preview.install(path.clone(), editor, cx);
                true
            });
            if !shown.unwrap_or(false) {
                // Nobody shows the document this answer opened: its reference goes again, as a preview's
                // does when it goes.
                cx.update(|cx| let_go(&label, &path, &info, cx));
            }
        })
        .detach();
    }

    /// Makes `editor` (on `path`) the one shown: the previous one goes, or is held when it has unsaved
    /// edits; the new one takes Escape and keeps the editing shortcuts out while it has the keyboard.
    fn install(&mut self, path: String, editor: Entity<Editor>, cx: &mut Context<Self>) {
        self.release_shown(cx);
        let version = self.target.as_ref().map_or(0, |target| target.version);
        let weak = cx.weak_entity();
        editor.update(cx, |editor, cx| {
            editor.set_escape_handler(Some(Rc::new(move |_: &mut Window, cx: &mut App| {
                weak.update(cx, |_, cx| cx.emit(CodePreviewEvent::Escape)).is_ok()
            })));
            // Beside a list of results the gutter has the line numbers alone: no folding, no icons.
            editor.set_numbers_only_gutter(true, cx);
        });
        // The shortcuts of editing actions would act on the tab behind the popup: the preview keeps them.
        let shield = input::keep_keys_out(editor.focus_handle(cx), input::is_editor_text_key, cx);
        let entry = (self.window_label.clone(), editor.downgrade());
        let editors = cx.default_global::<PreviewEditors>();
        editors.leaving.remove(&self.window_label);
        editors.shown.insert(self.id, entry);
        self.shown = Some(Shown { path, version, editor, shield });
        self.wanted = None;
        self.error = None;
        self.revealed = None;
        self.apply_target(cx);
        cx.notify();
    }

    /// Lets the editor shown go, holding it while its document has unsaved edits.
    fn release_shown(&mut self, cx: &mut App) {
        let Some(shown) = self.shown.take() else { return };
        cx.default_global::<PreviewEditors>().shown.remove(&self.id);
        input::release_keys(shown.shield, cx);
        shown.editor.update(cx, |editor, _| editor.set_escape_handler(None));
        hold_or_release(&self.window_label, &shown.path, shown.editor, cx);
        self.revealed = None;
    }

    /// Puts the target's marks on the editor; a new line also moves the caret there and centres it.
    fn apply_target(&mut self, cx: &mut Context<Self>) {
        let (Some(shown), Some(target)) = (self.shown.as_ref(), self.target.as_ref()) else { return };
        let editor = shown.editor.clone();
        let (line, highlights, mode) = (target.line, target.highlights.clone(), self.mode);
        let reveal = self.revealed.as_ref() != Some(&(shown.path.clone(), line));
        let revealed = (shown.path.clone(), line);
        editor.update(cx, |editor, cx| {
            let marks = editor_marks(editor.rope(), line, &highlights, mode);
            editor.set_marks(MarkLayer::Find, marks, cx);
            if reveal && line < editor.line_count() {
                let at = editor.offset_of(line, 0);
                editor.select(at, at, cx);
                // Centred once the editor knows its height; until then the line is near the top.
                editor.scroll_to_line(line.saturating_sub(5), cx);
            }
        });
        if reveal {
            self.revealed = Some(revealed);
            self.center_pending = true;
        }
    }

    /// The keys the editor leaves to the preview: ⌘S (Ctrl+S) saves the document, a failure told to the
    /// owner; ⌘Y (Ctrl+Y) redoes, as ⌘⇧Z does in the editor.
    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        let command = modifiers.secondary(cx) && !modifiers.shift && !modifiers.alt;
        let Some(shown) = self.current().filter(|_| command) else { return };
        let editor = shown.editor.clone();
        if keystroke.key == "y" {
            cx.stop_propagation();
            editor.update(cx, |editor, cx| {
                editor.redo(cx);
            });
            return;
        }
        if keystroke.key != "s" || editor.read(cx).is_read_only() {
            return;
        }
        cx.stop_propagation();
        let saving = editor.read(cx).save(cx);
        cx.spawn(async move |preview, cx| {
            let failure = match saving.await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error.message,
                Err(error) => error.to_string(),
            };
            let _ = preview.update(cx, |_, cx| cx.emit(CodePreviewEvent::SaveFailed(failure)));
        })
        .detach();
    }
}

/// Lets go of the reference to the document `info` that an answer nobody shows opened (the selection left
/// the file, or the preview went, while it was on its way). The document may have been changed meanwhile (a
/// Replace edits an open document in place): then it goes as a preview with unsaved edits does, held or
/// saved; otherwise it is closed.
fn let_go(label: &str, path: &str, info: &ide_workspace::DocumentInfo, cx: &mut App) {
    let Some(host) = cx.try_global::<Backend>().map(|backend| backend.host.clone()) else { return };
    let project = host.project(label).ok();
    let changed = project.as_ref().is_some_and(|project| project.documents.is_dirty(info.id).unwrap_or(false));
    let snapshot =
        project.as_ref().filter(|_| changed).and_then(|project| project.documents.rope_snapshot(info.id).ok());
    if let Some((version, rope)) = snapshot {
        // The editor takes over the reference and closes the document when it goes.
        let editor = cx.new(|cx| Editor::with_text(host, label, info, version, rope, cx));
        hold_or_release(label, path, editor, cx);
        return;
    }
    if let Err(error) = ide_host::ops::doc::close(&host, label, info.id) {
        tracing::debug!("preview: cannot close {path}: {}", error.message);
    }
}

/// The answer to request `request` is an error: it is shown when the request is still the one asked for.
fn failed(preview: &gpui_kit::WeakEntity<CodePreview>, request: u64, message: String, cx: &mut gpui_kit::AsyncApp) {
    let _ = preview.update(cx, |preview, cx| {
        if preview.wanted.as_ref().is_some_and(|(_, _, wanted)| *wanted == request) {
            preview.wanted = None;
            preview.error = Some(message);
            cx.notify();
        }
    });
}

impl Render for CodePreview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pane = div()
            .debug_selector(|| "code-preview".to_owned())
            .size_full()
            .flex()
            .bg(theme::color(cx, "bg-app"))
            .on_key_down(cx.listener(Self::key_down));
        if let Some(note) = self.note(cx) {
            return pane
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(note));
        }
        let Some(editor) = self.current().map(|shown| shown.editor.clone()) else { return pane };
        if std::mem::take(&mut self.center_pending) {
            // Weak: an editor let go before that frame closes its document at once.
            let centred = editor.downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = centred.update(cx, |editor, cx| editor.scroll_caret_to_center(cx));
            });
        }
        pane.child(div().size_full().min_w_0().child(editor))
    }
}

#[cfg(test)]
mod proofs26_code_preview;
