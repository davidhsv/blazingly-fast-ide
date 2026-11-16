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
//! While the preview has the keyboard it edits, undoes, copies and saves its own text, so the IDE's
//! shortcuts are kept out; Escape is handed to the owner, which closes the popup. A preview with unsaved
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
    App, Context, Entity, EventEmitter, Focusable, Global, IntoElement, KeyDownEvent, Render, SharedString, Window,
    div, prelude::*,
};
use ropey::Rope;
use std::collections::HashMap;
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
    /// The claim that keeps the IDE's shortcuts out while the editor has the keyboard.
    shield: u64,
}

/// Previews with unsaved edits that went away, by window and path: shown again when their file is.
#[derive(Default)]
struct HeldPreviews(HashMap<(String, String), Entity<Editor>>);

impl Global for HeldPreviews {}

/// Keeps `editor` when its document has unsaved edits, so they survive the preview going away; drops it
/// otherwise, which lets its document go.
/// With Files › Autosave on, the edits are saved now, as when the last tab of a document closes; the editor
/// stays held until they are.
fn hold_or_release(label: &str, path: &str, editor: Entity<Editor>, cx: &mut App) {
    if !editor.read(cx).is_dirty() {
        return;
    }
    if crate::platform::settings::effective(label, cx).files.auto_save {
        let saving = editor.read(cx).save(cx);
        let file = path.to_owned();
        cx.spawn(async move |_| match saving.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!("autosave of {file} failed: {}", error.message),
            Err(error) => tracing::warn!("autosave of {file} failed: {error:#}"),
        })
        .detach();
    }
    cx.default_global::<HeldPreviews>().0.insert((label.to_owned(), path.to_owned()), editor);
}

/// The held preview of `path`, taken out of the held ones. The held previews of the window whose documents
/// were saved meanwhile, or that the window's project no longer has open (another project was opened in
/// it), are dropped first.
fn take_held(label: &str, path: &str, cx: &mut App) -> Option<Entity<Editor>> {
    let held = cx.try_global::<HeldPreviews>()?;
    let project = cx.try_global::<Backend>().and_then(|backend| backend.host.project(label).ok());
    let gone: Vec<(String, String)> = held
        .0
        .iter()
        .filter(|((window, _), editor)| {
            let editor = editor.read(cx);
            let open_doc = project.as_ref().and_then(|project| project.documents.by_path(editor.path()));
            let open = open_doc == Some(editor.doc());
            window == label && (!editor.is_dirty() || !open)
        })
        .map(|(key, _)| key.clone())
        .collect();
    let held = &mut cx.default_global::<HeldPreviews>().0;
    for key in gone {
        held.remove(&key);
    }
    held.remove(&(label.to_owned(), path.to_owned()))
}

/// The previews of the window `label` held with unsaved edits: saving every document, autosave and the
/// question on closing the window count them with the editors of the tabs.
pub fn held_editors(label: &str, cx: &App) -> Vec<Entity<Editor>> {
    let Some(held) = cx.try_global::<HeldPreviews>() else { return Vec::new() };
    held.0.iter().filter(|((window, _), _)| window == label).map(|(_, editor)| editor.clone()).collect()
}

pub struct CodePreview {
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
                // Nobody shows the document this answer opened: its reference goes again.
                let host = cx.update(|cx| cx.try_global::<Backend>().map(|backend| backend.host.clone()));
                if let Some(host) = host
                    && let Err(error) = ide_host::ops::doc::close(&host, &label, info.id)
                {
                    tracing::debug!("preview: cannot close {path}: {}", error.message);
                }
            }
        })
        .detach();
    }

    /// Makes `editor` (on `path`) the one shown: the previous one goes, or is held when it has unsaved
    /// edits; the new one takes Escape and keeps the IDE's shortcuts out while it has the keyboard.
    fn install(&mut self, path: String, editor: Entity<Editor>, cx: &mut Context<Self>) {
        self.release_shown(cx);
        let version = self.target.as_ref().map_or(0, |target| target.version);
        let weak = cx.weak_entity();
        editor.update(cx, |editor, _| {
            editor.set_escape_handler(Some(Rc::new(move |_: &mut Window, cx: &mut App| {
                weak.update(cx, |_, cx| cx.emit(CodePreviewEvent::Escape)).is_ok()
            })));
        });
        let shield = input::keep_keymap_out(editor.focus_handle(cx), cx);
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
            let centred = editor.clone();
            window.on_next_frame(move |_, cx| centred.update(cx, |editor, cx| editor.scroll_caret_to_center(cx)));
        }
        pane.child(div().size_full().min_w_0().child(editor))
    }
}

#[cfg(test)]
mod proofs26_code_preview;
