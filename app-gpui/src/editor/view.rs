//! The editor view: one document of the backend, painted natively.
//!
//! The text is the backend's own rope. [`Editor`] holds a clone of it at a known version (a
//! rope clone shares its chunks, so the text exists once in the process) and every edit goes
//! through `Documents::edit` with that version, exactly as the webview's `doc_edit` does, only
//! without a copy of the document on the UI side. Edits the backend makes itself (formatting,
//! refactorings, reloads from disk) arrive as `DocPatched` events and move the selection.
//!
//! Only the visible lines are shaped and painted, so the cost of a frame does not depend on the
//! length of the document.

use super::buffer::{self, Assoc};
use super::history::{EditKind, History, invert};
use crate::backend::Backend;
use crate::platform::input::EDITOR_CONTEXT;
use crate::theme;
use gpui_kit::MouseMoveEvent;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, Font, GlobalElementId, Hsla, InspectorElementId, IntoElement,
    KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollEvent, ShapedLine, SharedString,
    Style, TextAlign, TextRun, UTF16Selection, WeakEntity, Window, div, fill, font, point, prelude::*, px, relative,
    size,
};
use ide_core::error::ErrorKind;
use ide_core::{ChangeSet, DocId, IdeEvent, IdeResult, LanguageId, TextChange};
use ide_host::Host;
use ide_language::SyntaxHighlightKind;
use ide_text::LineEnding;
use ide_workspace::DocumentInfo;
use ropey::Rope;
use std::future::Future;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

/// The colour token of a highlight kind, as the webview's highlight style assigns them.
pub fn syntax_token(kind: SyntaxHighlightKind) -> &'static str {
    use SyntaxHighlightKind as K;
    match kind {
        K::Keyword => "syn-keyword",
        K::String | K::StringSpecial => "syn-string",
        K::Escape => "syn-escape",
        K::Regexp => "syn-regexp",
        K::Atom | K::Constant | K::Label => "syn-constant",
        K::Number => "syn-number",
        K::Comment => "syn-comment",
        K::DocComment => "syn-doc-comment",
        K::Function => "syn-function",
        K::FunctionCall => "syn-function-call",
        K::Builtin => "syn-builtin",
        K::Type | K::Namespace => "syn-type",
        K::Variable => "syn-variable",
        K::SelfVariable => "syn-self",
        K::Property => "syn-property",
        K::Attribute | K::Macro => "syn-decorator",
        K::Tag => "syn-tag",
        K::Operator | K::Punctuation => "syn-punct",
        K::Parameter => "syn-param",
    }
}

/// How long the text must be still before it is highlighted again.
const HIGHLIGHT_DELAY: std::time::Duration = std::time::Duration::from_millis(40);

/// A highlighted range: UTF-16 offsets and its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub from: u32,
    pub to: u32,
    pub kind: SyntaxHighlightKind,
}

/// The active editor's state as the status bar shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct EditorStatus {
    /// Zero-based line and UTF-16 column of the caret.
    pub line: u32,
    pub col: u32,
    /// Length of the selection in UTF-16 code units, and the line breaks it spans.
    pub selected_chars: u32,
    pub selected_line_breaks: u32,
    pub line_ending: LineEnding,
    pub encoding: String,
    pub read_only: bool,
}

impl EditorStatus {
    /// `12:5`, with the selection's size when there is one: `12:5 (40 chars, 2 line breaks)`.
    pub fn caret_text(&self) -> String {
        let mut text = format!("{}:{}", self.line + 1, self.col + 1);
        if self.selected_chars > 0 {
            text.push_str(&format!(" ({} chars", self.selected_chars));
            match self.selected_line_breaks {
                0 => {}
                1 => text.push_str(", 1 line break"),
                n => text.push_str(&format!(", {n} line breaks")),
            }
            text.push(')');
        }
        text
    }

    pub fn line_ending_text(&self) -> &'static str {
        match self.line_ending {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
            LineEnding::Cr => "CR",
        }
    }
}

/// What the last frame drew, for hit-testing and the platform's text input.
struct Layout {
    bounds: Bounds<Pixels>,
    /// Left edge of the text, right of the gutter.
    text_left: Pixels,
    line_height: Pixels,
    first_line: u32,
    lines: Vec<(String, ShapedLine)>,
}

/// The editors that show each document. The backend reports the edits it makes itself, not the
/// ones an editor sends it: an editor tells the others of its document about those.
#[derive(Default)]
struct DocumentEditors(std::collections::HashMap<DocId, Vec<WeakEntity<Editor>>>);

impl gpui_kit::Global for DocumentEditors {}

pub struct Editor {
    focus: FocusHandle,
    host: Arc<Host>,
    /// The backend's label for the window whose project owns the document.
    window_label: String,
    doc: DocId,
    path: PathBuf,
    language: LanguageId,
    read_only: bool,
    large: bool,
    line_ending: LineEnding,
    encoding: String,
    rope: Rope,
    version: u32,
    /// The selection in UTF-16 offsets: `anchor` stays, `head` is the caret.
    anchor: u32,
    head: u32,
    /// Text being composed by an input method.
    marked: Option<Range<u32>>,
    /// The column Up/Down try to keep, in pixels from the text's left edge.
    goal_x: Option<Pixels>,
    scroll_top: Pixels,
    highlights: Vec<HighlightSpan>,
    layout: Option<Layout>,
    listener: u64,
    history: History,
    /// Where a drag selection began, while the mouse button is down.
    drag_anchor: Option<u32>,
    /// The document has changes that are not on disk.
    dirty: bool,
    /// Counts edits, so that a highlight request an edit has overtaken is dropped.
    generation: u64,
    created: std::time::Instant,
}

impl Editor {
    /// An editor for a document the backend has open for `window_label`.
    pub fn new(host: Arc<Host>, window_label: &str, info: &DocumentInfo, cx: &mut Context<Self>) -> IdeResult<Self> {
        let (version, rope) = host.project(window_label)?.documents.rope_snapshot(info.id)?;
        Ok(Self::with_text(host, window_label, info, version, rope, cx))
    }

    /// An editor showing `rope`, the document's text at `version`.
    pub fn with_text(
        host: Arc<Host>,
        window_label: &str,
        info: &DocumentInfo,
        version: u32,
        rope: Rope,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak: WeakEntity<Self> = cx.weak_entity();
        let doc = info.id;
        cx.default_global::<DocumentEditors>().0.entry(doc).or_default().push(weak.clone());
        let listener = cx.global_mut::<Backend>().listen(move |event, cx| match &event.event {
            IdeEvent::DocPatched { doc: patched, base_version, changes, workspace_edit, .. } if *patched == doc => {
                let _ = weak.update(cx, |editor, cx| editor.remote_patch(*base_version, changes, *workspace_edit, cx));
            }
            IdeEvent::DocDirty { doc: changed, dirty } if *changed == doc => {
                let _ = weak.update(cx, |editor, cx| {
                    editor.dirty = *dirty;
                    cx.notify();
                });
            }
            _ => {}
        });
        cx.on_release(|editor, cx| {
            let shown = &mut cx.default_global::<DocumentEditors>().0;
            if let Some(editors) = shown.get_mut(&editor.doc) {
                editors.retain(|other| other.upgrade().is_some());
                if editors.is_empty() {
                    shown.remove(&editor.doc);
                }
            }
            cx.global_mut::<Backend>().unlisten(editor.listener);
            // Every editor holds one reference to its document; the backend drops the document
            // with its last reference. A project that is already closed has closed it.
            if let Ok(documents) = editor.documents() {
                documents.close(editor.doc);
            }
        })
        .detach();
        let mut editor = Self {
            focus: cx.focus_handle(),
            host,
            window_label: window_label.to_owned(),
            doc,
            path: PathBuf::from(info.path.clone()),
            language: info.language.clone(),
            read_only: info.read_only,
            large: info.large,
            line_ending: info.line_ending,
            encoding: info.encoding.clone(),
            rope,
            version,
            anchor: 0,
            head: 0,
            marked: None,
            goal_x: None,
            scroll_top: px(0.0),
            highlights: Vec::new(),
            layout: None,
            listener,
            history: History::default(),
            drag_anchor: None,
            dirty: info.dirty,
            generation: 0,
            created: std::time::Instant::now(),
        };
        editor.refresh_highlights(cx);
        editor
    }

    pub fn doc(&self) -> DocId {
        self.doc
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn language(&self) -> &LanguageId {
        &self.language
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    pub fn line_count(&self) -> u32 {
        buffer::line_count(&self.rope)
    }

    /// The selection as `from..to` UTF-16 offsets.
    pub fn selection(&self) -> Range<u32> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    /// The selection as zero-based lines and UTF-16 columns: what the backend and plugins take.
    pub fn selection_range(&self) -> ide_core::Range {
        let selection = self.selection();
        let position = |offset| {
            let (line, col) = buffer::line_col(&self.rope, offset);
            ide_core::Pos { line, col }
        };
        ide_core::Range { start: position(selection.start), end: position(selection.end) }
    }

    /// The UTF-16 offset of the zero-based `line` and UTF-16 column `col`, clamped to the text.
    pub fn offset_of(&self, line: u32, col: u32) -> u32 {
        let line = line.min(self.line_count().saturating_sub(1));
        buffer::offset_at(&self.rope, line, col).min(buffer::line_range(&self.rope, line).end)
    }

    /// The selection's fixed end, as a UTF-16 offset: the caret is its moving end.
    pub fn anchor(&self) -> u32 {
        self.anchor
    }

    /// The first line the view shows, as scrolled.
    pub fn top_line(&self, cx: &App) -> u32 {
        (self.scroll_top / Self::metrics(cx).1).floor() as u32
    }

    /// Scrolls so that `line` is the first one shown (the last line, if the document is shorter).
    pub fn scroll_to_line(&mut self, line: u32, cx: &mut Context<Self>) {
        let line = line.min(self.line_count().saturating_sub(1));
        self.scroll_top = Self::metrics(cx).1 * line as f32;
        cx.notify();
    }

    /// The caret's UTF-16 offset.
    pub fn caret(&self) -> u32 {
        self.head
    }

    /// What the status bar shows about this editor.
    pub fn status(&self) -> EditorStatus {
        let selection = self.selection();
        let (line, col) = self.caret_position();
        let line_of = |offset| buffer::line_col(&self.rope, offset).0;
        EditorStatus {
            line,
            col,
            selected_chars: selection.end - selection.start,
            selected_line_breaks: line_of(selection.end) - line_of(selection.start),
            line_ending: self.line_ending,
            encoding: self.encoding.clone(),
            read_only: self.read_only,
        }
    }

    /// The text of the zero-based `line`, without its line break; empty past the last line.
    pub fn line_text(&self, line: u32) -> String {
        if line >= self.line_count() { String::new() } else { buffer::line_text(&self.rope, line) }
    }

    /// The caret's zero-based line and UTF-16 column.
    pub fn caret_position(&self) -> (u32, u32) {
        buffer::line_col(&self.rope, self.head)
    }

    pub fn highlights(&self) -> &[HighlightSpan] {
        &self.highlights
    }

    pub fn scroll_top(&self) -> Pixels {
        self.scroll_top
    }

    /// The lines the last frame shaped: the first one's index and how many.
    pub fn painted_lines(&self) -> Option<Range<u32>> {
        self.layout.as_ref().map(|l| l.first_line..l.first_line + l.lines.len() as u32)
    }

    fn metrics(cx: &App) -> (Pixels, Pixels, Pixels) {
        (theme::length(cx, "fs-code"), theme::length(cx, "lh-code"), theme::length(cx, "w-gutter"))
    }

    /// The first font of the theme's `--font-mono` list that is installed.
    fn code_font(cx: &App) -> Font {
        let families =
            cx.try_global::<theme::ActiveTheme>().map(|t| t.theme.font_families("font-mono")).unwrap_or_default();
        let installed = cx.text_system().all_font_names();
        let family =
            families.into_iter().find(|f| installed.iter().any(|name| name == f)).unwrap_or_else(|| "Menlo".to_owned());
        font(SharedString::from(family))
    }

    /// Sets the selection; offsets are clamped to the document.
    pub fn select(&mut self, anchor: u32, head: u32, cx: &mut Context<Self>) {
        let len = buffer::len_utf16(&self.rope);
        (self.anchor, self.head) = (anchor.min(len), head.min(len));
        self.goal_x = None;
        self.history.seal();
        self.reveal_caret(cx);
        cx.notify();
    }

    fn move_to(&mut self, offset: u32, extend: bool, cx: &mut Context<Self>) {
        let anchor = if extend { self.anchor } else { offset };
        self.select(anchor, offset, cx);
    }

    /// Scrolls so that the caret's line is inside the viewport.
    fn reveal_caret(&mut self, cx: &App) {
        let Some(layout) = &self.layout else { return };
        let (_, line_height, _) = Self::metrics(cx);
        let line_top = line_height * self.caret_position().0 as f32;
        let height = layout.bounds.size.height;
        if line_top < self.scroll_top {
            self.scroll_top = line_top;
        } else if line_top + line_height > self.scroll_top + height {
            self.scroll_top = line_top + line_height - height;
        }
    }

    /// Scrolls by `delta` pixels (positive moves the text up), clamped to the document.
    pub fn scroll_by(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        let (_, line_height, _) = Self::metrics(cx);
        let viewport = self.layout.as_ref().map_or(px(0.0), |l| l.bounds.size.height);
        let content = line_height * self.line_count() as f32;
        let max = (content - viewport).max(px(0.0));
        let next = (self.scroll_top + delta).clamp(px(0.0), max);
        if next != self.scroll_top {
            self.scroll_top = next;
            cx.notify();
        }
    }

    fn documents(&self) -> IdeResult<Arc<ide_workspace::Documents>> {
        Ok(self.host.project(&self.window_label)?.documents.clone())
    }

    fn snapshot(&mut self) -> IdeResult<()> {
        (self.version, self.rope) = self.documents()?.rope_snapshot(self.doc)?;
        Ok(())
    }

    /// Takes the backend's current text; the selection follows through `changes` when they are
    /// known, else through the difference between the old and the new text. `record` makes the
    /// change an undo entry of that kind. Returns the changes that were followed.
    fn follow(
        &mut self,
        changes: Option<&[TextChange]>,
        record: Option<EditKind>,
        cx: &mut Context<Self>,
    ) -> Vec<TextChange> {
        let old = self.rope.clone();
        if let Err(error) = self.snapshot() {
            tracing::warn!("cannot read document {}: {}", self.doc, error.message);
            return Vec::new();
        }
        let changes = match changes {
            Some(changes) => changes.to_vec(),
            None => difference(&old, &self.rope),
        };
        let before = (self.anchor, self.head);
        self.anchor = buffer::map_offset(self.anchor, &changes, Assoc::After);
        self.head = buffer::map_offset(self.head, &changes, Assoc::After);
        self.marked = None;
        if let Some(kind) = record.filter(|_| !changes.is_empty()) {
            let now = self.created.elapsed().as_secs_f64() * 1000.0;
            self.history.record(changes.clone(), invert(&old, &changes), before, (self.anchor, self.head), kind, now);
        }
        self.generation += 1;
        self.refresh_highlights(cx);
        cx.notify();
        changes
    }

    /// Tells the other editors of the document about `changes`, which this one made to the text
    /// at `base_version`: they follow them as they follow the backend's own edits.
    fn tell_other_editors(&self, base_version: u32, changes: &[TextChange], cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let others: Vec<Entity<Editor>> = cx
            .default_global::<DocumentEditors>()
            .0
            .get(&self.doc)
            .into_iter()
            .flatten()
            .filter(|other| **other != this)
            .filter_map(WeakEntity::upgrade)
            .collect();
        for other in others {
            other.update(cx, |other, cx| other.remote_patch(base_version, changes, false, cx));
        }
    }

    /// The backend edited the document itself. Like any edit it can be undone here, unless it
    /// is part of a workspace edit, which is undone as a whole elsewhere: then the changes this
    /// editor could undo no longer apply to the text, and its history starts over.
    fn remote_patch(
        &mut self,
        base_version: u32,
        changes: &[TextChange],
        workspace_edit: bool,
        cx: &mut Context<Self>,
    ) {
        let current = self.documents().and_then(|d| d.version(self.doc)).unwrap_or(self.version);
        if current == self.version {
            return; // already reflected (the edit was ours, or a snapshot overtook the event)
        }
        if workspace_edit {
            self.history.clear();
        }
        let record = (!workspace_edit).then_some(EditKind::Other);
        self.follow((base_version == self.version).then_some(changes), record, cx);
    }

    /// Applies the edit `make` describes for the current text and selection. When the backend
    /// has moved on (its own edit not yet delivered here), the text is refreshed and `make` is
    /// asked again, so what the user typed lands at the caret's new place instead of being lost.
    fn edit(&mut self, kind: EditKind, cx: &mut Context<Self>, make: impl Fn(&Self) -> Vec<TextChange>) -> bool {
        if self.read_only {
            return false;
        }
        for attempt in 0..3 {
            let changes = make(self);
            if changes.is_empty() {
                return false;
            }
            let result =
                self.documents().and_then(|d| d.edit(self.doc, self.version, ChangeSet { changes: changes.clone() }));
            match result {
                Ok(_) => {
                    // Known here and now: the backend's own report arrives a moment later, and
                    // closing in between must still see the document as unsaved.
                    self.dirty = true;
                    let base = self.version;
                    self.follow(Some(&changes), Some(kind), cx);
                    self.tell_other_editors(base, &changes, cx);
                    self.goal_x = None;
                    self.reveal_caret(cx);
                    return true;
                }
                // The backend's edit becomes an undo entry of its own before the user's.
                Err(error) if error.kind == ErrorKind::OutOfSync && attempt < 2 => {
                    self.follow(None, Some(EditKind::Other), cx);
                }
                Err(error) => {
                    tracing::warn!("edit of {} rejected: {}", self.path.display(), error.message);
                    return false;
                }
            }
        }
        false
    }

    /// Applies one side of history entries. On a rejection the history no longer describes the
    /// text: it is dropped and the text refreshed.
    fn replay(&mut self, steps: &[(Vec<TextChange>, (u32, u32))], cx: &mut Context<Self>) -> bool {
        for (changes, selection) in steps {
            let applied =
                self.documents().and_then(|d| d.edit(self.doc, self.version, ChangeSet { changes: changes.clone() }));
            if let Err(error) = applied {
                tracing::warn!("cannot replay an edit of {}: {}", self.path.display(), error.message);
                self.history.clear();
                self.follow(None, None, cx);
                return false;
            }
            self.dirty = true;
            let base = self.version;
            self.follow(Some(changes), None, cx);
            self.tell_other_editors(base, changes, cx);
            (self.anchor, self.head) = *selection;
        }
        self.goal_x = None;
        self.reveal_caret(cx);
        cx.notify();
        true
    }

    /// Undoes the newest group of edits; the selection returns to where it was before them.
    pub fn undo(&mut self, cx: &mut Context<Self>) -> bool {
        let entries = self.history.take_undo();
        if entries.is_empty() || self.read_only {
            return false;
        }
        let steps: Vec<_> = entries.iter().map(|e| (e.inverse.clone(), e.before)).collect();
        let done = self.replay(&steps, cx);
        if done {
            self.history.undone(entries);
        }
        done
    }

    /// Redoes the group that was undone last.
    pub fn redo(&mut self, cx: &mut Context<Self>) -> bool {
        let entries = self.history.take_redo();
        if entries.is_empty() || self.read_only {
            return false;
        }
        let steps: Vec<_> = entries.iter().map(|e| (e.forward.clone(), e.after)).collect();
        let done = self.replay(&steps, cx);
        if done {
            self.history.redone(entries);
        }
        done
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Changes the line separator the document is saved with; the document then has unsaved changes.
    pub fn set_line_ending(&mut self, line_ending: LineEnding, cx: &mut Context<Self>) -> IdeResult<()> {
        if line_ending == self.line_ending {
            return Ok(());
        }
        self.documents()?.set_line_ending(self.doc, line_ending)?;
        (self.line_ending, self.dirty) = (line_ending, true);
        cx.notify();
        Ok(())
    }

    /// Changes the document's encoding. With `reload` the file is read again from disk in that
    /// encoding, which the backend refuses while there are unsaved changes; without, the text
    /// stays and is written in the encoding on the next save.
    pub fn set_encoding(&mut self, encoding: &str, reload: bool, cx: &mut Context<Self>) -> IdeResult<()> {
        self.encoding = self.documents()?.set_encoding(self.doc, encoding, reload)?;
        if reload {
            // The text is another one: what could be undone no longer applies to it.
            self.history.clear();
            self.follow(None, None, cx);
            self.dirty = false;
        } else {
            self.dirty = true;
        }
        cx.notify();
        Ok(())
    }

    /// Whether the document has changes that are not on disk.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Writes the document to disk; the backend reports the document clean when it is done.
    pub fn save(&self, cx: &App) -> impl Future<Output = anyhow::Result<IdeResult<()>>> + Send + 'static + use<> {
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
        cx.global::<Backend>().call(async move { ide_host::ops::doc::save(&host, &window, doc).await })
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.select(0, buffer::len_utf16(&self.rope), cx);
    }

    /// The selected text.
    pub fn selected_text(&self) -> String {
        let selection = self.selection();
        buffer::slice(&self.rope, selection.start, selection.end)
    }

    /// Copies the selection to the clipboard; nothing is copied without one.
    pub fn copy(&self, cx: &mut Context<Self>) -> bool {
        if self.selection().is_empty() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text()));
        true
    }

    pub fn cut(&mut self, cx: &mut Context<Self>) -> bool {
        self.copy(cx) && self.replace(None, "", EditKind::Other, cx)
    }

    /// Replaces the selection with the clipboard's text.
    pub fn paste(&mut self, cx: &mut Context<Self>) -> bool {
        match cx.read_from_clipboard().and_then(|item| item.text()) {
            Some(text) if !text.is_empty() => self.replace(None, &text, EditKind::Other, cx),
            _ => false,
        }
    }

    fn replace(&mut self, range: Option<Range<u32>>, text: &str, kind: EditKind, cx: &mut Context<Self>) -> bool {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.edit(kind, cx, |editor| {
            let range = range.clone().unwrap_or_else(|| editor.selection());
            if range.is_empty() && text.is_empty() {
                return Vec::new();
            }
            vec![TextChange { from: range.start, to: range.end, insert: text.clone() }]
        })
    }

    /// Replaces the selection (or `range`) with typed `text`; the caret ends after it.
    pub fn insert(&mut self, range: Option<Range<u32>>, text: &str, cx: &mut Context<Self>) -> bool {
        self.replace(range, text, EditKind::Insert, cx)
    }

    /// Deletes the selection, or the character before (`forward` false) or after the caret.
    pub fn delete(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        self.edit(EditKind::Delete, cx, |editor| {
            let selection = editor.selection();
            let range = if !selection.is_empty() {
                selection
            } else if forward {
                selection.start..editor.next_offset(selection.start)
            } else {
                editor.previous_offset(selection.start)..selection.start
            };
            if range.is_empty() {
                Vec::new()
            } else {
                vec![TextChange { from: range.start, to: range.end, insert: String::new() }]
            }
        })
    }

    /// The offset one character (not code unit) after `offset`; a line break counts as one.
    fn next_offset(&self, offset: u32) -> u32 {
        let char = self.rope.utf16_cu_to_char(offset as usize);
        if char >= self.rope.len_chars() {
            return offset;
        }
        let step =
            if self.rope.char(char) == '\r' && char + 1 < self.rope.len_chars() && self.rope.char(char + 1) == '\n' {
                2
            } else {
                1
            };
        self.rope.char_to_utf16_cu(char + step) as u32
    }

    fn previous_offset(&self, offset: u32) -> u32 {
        let char = self.rope.utf16_cu_to_char(offset as usize);
        if char == 0 {
            return 0;
        }
        let step =
            if char >= 2 && self.rope.char(char - 1) == '\n' && self.rope.char(char - 2) == '\r' { 2 } else { 1 };
        self.rope.char_to_utf16_cu(char - step) as u32
    }

    fn refresh_highlights(&mut self, cx: &mut Context<Self>) {
        if self.large {
            self.highlights.clear();
            return;
        }
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
        let generation = self.generation;
        cx.spawn(async move |editor, cx| {
            // Highlighting reads the whole document: while the user types, only the last edit
            // of a burst asks for it.
            cx.background_executor().timer(HIGHLIGHT_DELAY).await;
            let request = editor.update(cx, |editor, cx| {
                (editor.generation == generation).then(|| {
                    let work = async move { ide_host::ops::doc::doc_highlights(&host, &window, doc).await };
                    (editor.version, cx.global::<Backend>().call(work))
                })
            });
            let Ok(Some((version, work))) = request else { return };
            let Ok(Ok(highlights)) = work.await else { return };
            let _ = editor.update(cx, |editor: &mut Editor, cx| {
                // Highlights of a version the text has already left would colour the wrong ranges.
                if editor.version != version {
                    return;
                }
                let (triples, _) = highlights.data.as_chunks::<3>();
                editor.highlights = triples
                    .iter()
                    .filter_map(|&[from, to, kind]| {
                        Some(HighlightSpan { from, to, kind: *highlights.kinds.get(kind as usize)? })
                    })
                    .collect();
                cx.notify();
            });
        })
        .detach();
    }

    /// The pixel position (top left of the caret) of `offset`, when its line was painted.
    pub fn position_of(&self, offset: u32) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let (line, col) = buffer::line_col(&self.rope, offset);
        let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
        let x = shaped.x_for_index(buffer::byte_of_col(text, col));
        let y = layout.line_height * line as f32 - self.scroll_top;
        Some(point(layout.bounds.left() + layout.text_left + x, layout.bounds.top() + y))
    }

    /// The offset nearest to a point of the window.
    pub fn offset_at(&self, position: Point<Pixels>) -> Option<u32> {
        let layout = self.layout.as_ref()?;
        let y = (position.y - layout.bounds.top() + self.scroll_top).max(px(0.0));
        let line = ((y / layout.line_height) as u32).min(self.line_count().saturating_sub(1));
        let x = position.x - layout.bounds.left() - layout.text_left;
        let col = match layout.lines.get(line.checked_sub(layout.first_line)? as usize) {
            Some((text, shaped)) => buffer::col_of_byte(text, shaped.closest_index_for_x(x)),
            None => 0,
        };
        Some(buffer::offset_at(&self.rope, line, col))
    }

    fn vertical(&mut self, lines: i64, extend: bool, window: &mut Window, cx: &mut Context<Self>) {
        let (line, col) = self.caret_position();
        let target = (i64::from(line) + lines).clamp(0, i64::from(self.line_count()) - 1) as u32;
        let goal = self.goal_x.or_else(|| {
            let layout = self.layout.as_ref()?;
            let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
            Some(shaped.x_for_index(buffer::byte_of_col(text, col)))
        });
        // Columns are measured on the target line, shaped here when it is off screen.
        let text = buffer::line_text(&self.rope, target);
        let col = match goal {
            Some(x) => {
                let (font_size, _, _) = Self::metrics(cx);
                let run = TextRun {
                    len: text.len(),
                    font: Self::code_font(cx),
                    color: Hsla::default(),
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(text.clone().into(), font_size, &[run], None);
                buffer::col_of_byte(&text, shaped.closest_index_for_x(x))
            }
            None => col,
        };
        let offset = buffer::offset_at(&self.rope, target, col);
        self.move_to(offset, extend, cx);
        self.goal_x = goal;
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let modifiers = keystroke.modifiers;
        // The editing shortcuts every text field has (⌘Z, ⌘C…; Ctrl on Windows and Linux).
        if modifiers.secondary(cx) && !modifiers.alt && modifiers.number_of_modifiers() <= 2 {
            let handled = match (keystroke.key.as_str(), modifiers.shift) {
                ("z", false) => Some(self.undo(cx)),
                ("z", true) => Some(self.redo(cx)),
                ("c", false) => Some(self.copy(cx)),
                ("x", false) => Some(self.cut(cx)),
                ("v", false) => Some(self.paste(cx)),
                ("a", false) => {
                    self.select_all(cx);
                    Some(true)
                }
                _ => None,
            };
            if handled.is_some() {
                window.invalidate_character_coordinates();
                cx.stop_propagation();
                return;
            }
        }
        let extend = modifiers.shift;
        let (line, _) = self.caret_position();
        let line_range = buffer::line_range(&self.rope, line);
        // Movement and deletion by word (⌥) and to the line's or document's edge (⌘), as
        // CodeMirror's standard keymap binds them on macOS.
        if (modifiers.alt || modifiers.platform) && !modifiers.control && !(modifiers.alt && modifiers.platform) {
            let end = buffer::len_utf16(&self.rope);
            match (keystroke.key.as_str(), modifiers.alt) {
                ("left", true) => self.move_to(buffer::group_boundary(&self.rope, self.head, false), extend, cx),
                ("right", true) => self.move_to(buffer::group_boundary(&self.rope, self.head, true), extend, cx),
                ("left", false) => self.move_to(line_range.start, extend, cx),
                ("right", false) => self.move_to(line_range.end, extend, cx),
                ("up", false) => self.move_to(0, extend, cx),
                ("down", false) => self.move_to(end, extend, cx),
                ("backspace", by_word) if !extend => {
                    let from =
                        if by_word { buffer::group_boundary(&self.rope, self.head, false) } else { line_range.start };
                    self.delete_to(from, cx);
                }
                ("delete", true) if !extend => {
                    let to = buffer::group_boundary(&self.rope, self.head, true);
                    self.delete_to(to, cx);
                }
                _ => return,
            }
            window.invalidate_character_coordinates();
            cx.stop_propagation();
            return;
        }
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        match keystroke.key.as_str() {
            "left" if !extend && !self.selection().is_empty() => self.move_to(self.selection().start, false, cx),
            "right" if !extend && !self.selection().is_empty() => self.move_to(self.selection().end, false, cx),
            "left" => self.move_to(self.previous_offset(self.head), extend, cx),
            "right" => self.move_to(self.next_offset(self.head), extend, cx),
            "up" => self.vertical(-1, extend, window, cx),
            "down" => self.vertical(1, extend, window, cx),
            "home" => self.move_to(line_range.start, extend, cx),
            "end" => self.move_to(line_range.end, extend, cx),
            "pageup" => self.vertical(-self.page_lines(cx), extend, window, cx),
            "pagedown" => self.vertical(self.page_lines(cx), extend, window, cx),
            "backspace" => {
                self.delete(false, cx);
            }
            "delete" => {
                self.delete(true, cx);
            }
            "enter" => {
                self.insert(None, "\n", cx);
            }
            "tab" => {
                self.insert(None, "    ", cx);
            }
            _ => return,
        }
        window.invalidate_character_coordinates();
        cx.stop_propagation();
    }

    /// Lines a Page Up / Page Down moves by: one viewport less a line of context.
    fn page_lines(&self, cx: &App) -> i64 {
        let viewport = self.layout.as_ref().map_or(px(0.0), |l| l.bounds.size.height);
        (((viewport / Self::metrics(cx).1).floor() as i64) - 1).max(1)
    }

    /// Deletes the selection, or the text between the caret and `other` when there is none.
    fn delete_to(&mut self, other: u32, cx: &mut Context<Self>) -> bool {
        self.edit(EditKind::Delete, cx, |editor| {
            let selection = editor.selection();
            let range = if selection.is_empty() { other.min(editor.head)..other.max(editor.head) } else { selection };
            if range.is_empty() {
                Vec::new()
            } else {
                vec![TextChange { from: range.start, to: range.end, insert: String::new() }]
            }
        })
    }

    /// A click places the caret (Shift extends), a double click selects the word, a triple
    /// click the line; dragging extends from where the press began.
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(offset) = self.offset_at(event.position) else { return };
        match event.click_count {
            0 | 1 => self.move_to(offset, event.modifiers.shift, cx),
            2 => {
                let word = buffer::word_at(&self.rope, offset);
                self.select(word.start, word.end, cx);
            }
            _ => {
                let (line, _) = buffer::line_col(&self.rope, offset);
                let range = buffer::line_range(&self.rope, line);
                // The whole line with its break, so that typing replaces the line.
                let end = if line + 1 < self.line_count() {
                    buffer::line_range(&self.rope, line + 1).start
                } else {
                    range.end
                };
                self.select(range.start, end, cx);
            }
        }
        self.drag_anchor = Some(self.anchor);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(anchor) = self.drag_anchor.filter(|_| event.pressed_button == Some(MouseButton::Left)) else {
            self.drag_anchor = None;
            return;
        };
        if let Some(offset) = self.offset_at(event.position).filter(|offset| *offset != self.head) {
            self.select(anchor, offset, cx);
        }
    }
}

/// One change turning `old` into `new`: what lies between their common prefix and suffix.
fn difference(old: &Rope, new: &Rope) -> Vec<TextChange> {
    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
    let (old_rest, new_rest) = (old.len_chars() - prefix, new.len_chars() - prefix);
    let mut suffix = 0;
    {
        let (mut a, mut b) = (old.chars_at(old.len_chars()), new.chars_at(new.len_chars()));
        while suffix < old_rest.min(new_rest) && a.prev() == b.prev() {
            suffix += 1;
        }
    }
    let (from, to) = (prefix, old.len_chars() - suffix);
    let insert = new.slice(prefix..new.len_chars() - suffix).to_string();
    if from == to && insert.is_empty() {
        return Vec::new();
    }
    vec![TextChange { from: old.char_to_utf16_cu(from) as u32, to: old.char_to_utf16_cu(to) as u32, insert }]
}

impl Focusable for Editor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for Editor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let len = buffer::len_utf16(&self.rope) as usize;
        let clamped = range.start.min(len)..range.end.min(len);
        if clamped != range {
            *adjusted = Some(clamped.clone());
        }
        Some(buffer::slice(&self.rope, clamped.start as u32, clamped.end as u32))
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let selection = self.selection();
        Some(UTF16Selection {
            range: selection.start as usize..selection.end as usize,
            reversed: self.head < self.anchor,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| m.start as usize..m.end as usize)
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Committed text replaces the composition, else the given range, else the selection.
        let range = range.map(|r| r.start as u32..r.end as u32).or_else(|| self.marked.clone());
        self.insert(range, text, cx);
        self.marked = None;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| r.start as u32..r.end as u32)
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let start = range.start;
        if !self.insert(Some(range), new_text, cx) && !new_text.is_empty() {
            return;
        }
        let length = new_text.encode_utf16().count() as u32;
        self.marked = (length > 0).then_some(start..start + length);
        // The composition's own selection is relative to the composed text.
        let selected = new_selected.map_or(length..length, |r| r.start as u32..r.end as u32);
        (self.anchor, self.head) = (start + selected.start.min(length), start + selected.end.min(length));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let layout = self.layout.as_ref()?;
        let from = self.position_of(range.start as u32)?;
        let to = self.position_of(range.end as u32).filter(|to| to.y == from.y).unwrap_or(from);
        Some(Bounds::new(from, size((to.x - from.x).max(px(1.0)), layout.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        self.offset_at(point).map(|offset| offset as usize)
    }
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .key_context(EDITOR_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|editor, _, _, _| editor.drag_anchor = None))
            .child(EditorElement { editor: cx.entity() })
    }
}

/// Paints the visible part of an [`Editor`].
struct EditorElement {
    editor: Entity<Editor>,
}

impl IntoElement for EditorElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct Prepared {
    layout: Layout,
    numbers: Vec<ShapedLine>,
    gutter: Pixels,
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepared;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepared {
        let (font_size, line_height, gutter) = Editor::metrics(cx);
        let code_font = Editor::code_font(cx);
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let editor = self.editor.read(cx);
        let total = editor.line_count();
        let first_line = ((editor.scroll_top / line_height).floor() as u32).min(total.saturating_sub(1));
        let visible = (bounds.size.height / line_height).ceil() as u32 + 1;
        let last_line = (first_line + visible).min(total);
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: code_font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };

        // Spans are sorted: skip to the first one that can touch the first visible line.
        let first_offset = buffer::line_range(&editor.rope, first_line).start;
        let mut span = editor.highlights.partition_point(|s| s.to <= first_offset);
        let mut lines = Vec::with_capacity((last_line - first_line) as usize);
        let mut numbers = Vec::with_capacity(lines.capacity());
        for line in first_line..last_line {
            let range = buffer::line_range(&editor.rope, line);
            let text = buffer::slice(&editor.rope, range.start, range.end);
            let mut runs = Vec::new();
            let mut at = 0usize;
            while let Some(s) = editor.highlights.get(span).filter(|s| s.from < range.end) {
                let from = buffer::byte_of_col(&text, s.from.saturating_sub(range.start)).max(at);
                let to = buffer::byte_of_col(&text, s.to.min(range.end) - range.start);
                if from > at {
                    runs.push(run(from - at, plain));
                }
                if to > from {
                    runs.push(run(to - from, theme::color(cx, syntax_token(s.kind))));
                    at = to;
                }
                if s.to > range.end {
                    break; // continues on the next line
                }
                span += 1;
            }
            if at < text.len() || runs.is_empty() {
                runs.push(run(text.len() - at, plain));
            }
            let shaped = window.text_system().shape_line(text.clone().into(), font_size, &runs, None);
            lines.push((text, shaped));
            let label = (line + 1).to_string();
            numbers.push(window.text_system().shape_line(
                label.clone().into(),
                font_size,
                &[run(label.len(), number_color)],
                None,
            ));
        }
        Prepared { layout: Layout { bounds, text_left: gutter, line_height, first_line, lines }, numbers, gutter }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepared: &mut Prepared,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, selection, caret, scroll_top, marked) = {
            let editor = self.editor.read(cx);
            (editor.focus.clone(), editor.selection(), editor.head, editor.scroll_top, editor.marked.clone())
        };
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.editor.clone()), cx);
        let editor = self.editor.clone();
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                let line_height = Editor::metrics(cx).1;
                let delta = event.delta.pixel_delta(line_height).y;
                editor.update(cx, |editor, cx| editor.scroll_by(-delta, cx));
                window.refresh();
            }
        });

        let Prepared { layout, numbers, gutter } = prepared;
        let line_height = layout.line_height;
        let focused = focus.is_focused(window);
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, theme::color(cx, "bg-editor")));
            let rope = self.editor.read(cx).rope.clone();
            let (caret_line, _) = buffer::line_col(&rope, caret);
            for (index, (text, shaped)) in layout.lines.iter().enumerate() {
                let line = layout.first_line + index as u32;
                let top = bounds.top() + line_height * line as f32 - scroll_top;
                let range = buffer::line_range(&rope, line);
                let text_origin = point(bounds.left() + *gutter, top);
                let x_of = |offset: u32| {
                    shaped.x_for_index(buffer::byte_of_col(text, offset.clamp(range.start, range.end) - range.start))
                };
                if selection.is_empty() && line == caret_line && focused {
                    let row = Bounds::new(
                        point(bounds.left() + *gutter, top),
                        size(bounds.size.width - *gutter, line_height),
                    );
                    window.paint_quad(fill(row, theme::color(cx, "bg-current-line")));
                }
                if selection.start <= range.end && selection.end >= range.start && !selection.is_empty() {
                    let from = x_of(selection.start);
                    // A selection running past the line end covers the line break too.
                    let to = if selection.end > range.end { x_of(range.end) + px(6.0) } else { x_of(selection.end) };
                    let area =
                        Bounds::new(point(text_origin.x + from, top), size((to - from).max(px(0.0)), line_height));
                    window.paint_quad(fill(area, theme::color(cx, "bg-selected")));
                }
                if let Some(marked) = marked.as_ref().filter(|m| m.start <= range.end && m.end >= range.start) {
                    let (from, to) = (x_of(marked.start), x_of(marked.end));
                    let underline = Bounds::new(
                        point(text_origin.x + from, top + line_height - px(2.0)),
                        size((to - from).max(px(0.0)), px(1.0)),
                    );
                    window.paint_quad(fill(underline, theme::color(cx, "fg-editor")));
                }
                let _ = shaped.paint(text_origin, line_height, TextAlign::Left, None, window, cx);
                if let Some(number) = numbers.get(index) {
                    let x = bounds.left() + *gutter - number.width - px(12.0);
                    let _ =
                        number.paint(point(x.max(bounds.left()), top), line_height, TextAlign::Left, None, window, cx);
                }
                if focused && line == caret_line {
                    let caret_bounds = Bounds::new(point(text_origin.x + x_of(caret), top), size(px(2.0), line_height));
                    window.paint_quad(fill(caret_bounds, theme::color(cx, "caret")));
                }
            }
        });
        let layout = std::mem::replace(
            layout,
            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new() },
        );
        self.editor.update(cx, |editor, _| editor.layout = Some(layout));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_core::{Pos, TextEdit};
    use std::time::Duration;

    /// A project in a temporary folder with `files`, and an editor on the first of them.
    struct Fixture {
        editor: Entity<Editor>,
        host: Arc<Host>,
        path: PathBuf,
        _home: tempfile::TempDir,
        _project: tempfile::TempDir,
    }

    impl Fixture {
        fn documents(&self) -> Arc<ide_workspace::Documents> {
            self.host.project("w1").expect("the project is open").documents.clone()
        }

        fn backend_text(&self) -> String {
            self.documents().text_of(&self.path).expect("the document is open")
        }
    }

    async fn open<'a>(cx: &'a mut TestAppContext, files: &[(&str, &str)]) -> (Fixture, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        let project = tempfile::tempdir().expect("a temporary project");
        let root = project.path().canonicalize().expect("the project folder exists");
        for (name, text) in files {
            std::fs::write(root.join(name), text).expect("the fixture file is written");
        }
        cx.update(|cx| {
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            cx.set_global(theme::ActiveTheme::default());
        });
        let path = root.join(files[0].0);
        let (root_for_open, path_for_open) = (root.clone(), path.clone());
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &root_for_open).await?;
                ide_host::ops::doc::open(&host, "w1", path_for_open).await
            })
        });
        let info = opened.await.expect("the task completes").expect("the document opens");
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        let editor_host = host.clone();
        let (editor, cx) =
            cx.add_window_view(move |_, cx| Editor::new(editor_host, "w1", &info, cx).expect("the editor opens"));
        cx.update(|window, cx| {
            let focus = editor.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (Fixture { editor, host, path, _home: home, _project: project }, cx)
    }

    /// Waits for work on the backend's own threads to reach the UI.
    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..200 {
            // Timers run on the test clock; the backend's threads run on the real one.
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within two seconds");
    }

    #[gpui_kit::test]
    async fn typing_edits_the_backends_document(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "world\n")]).await;
        let before = f.documents().version(f.editor.read_with(cx, |e, _| e.doc())).expect("a version");
        cx.simulate_input("hello ");
        f.editor.read_with(cx, |editor, _| {
            assert_eq!(editor.text(), "hello world\n");
            assert_eq!(editor.caret(), 6);
            assert!(editor.version() > before);
        });
        // The text lives in the backend: it is what a save, a search or a language server sees.
        assert_eq!(f.backend_text(), "hello world\n");
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn keys_move_the_caret_and_edit(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "ab\ncdef\n")]).await;
        cx.simulate_keystrokes("right right right");
        assert_eq!(f.editor.read_with(cx, |e, _| e.caret_position()), (1, 0));
        cx.simulate_keystrokes("end");
        assert_eq!(f.editor.read_with(cx, |e, _| e.caret_position()), (1, 4));
        // Up keeps the column where the line is long enough and clamps where it is not.
        cx.simulate_keystrokes("up");
        assert_eq!(f.editor.read_with(cx, |e, _| e.caret_position()), (0, 2));
        cx.simulate_keystrokes("down");
        assert_eq!(f.editor.read_with(cx, |e, _| e.caret_position()), (1, 4));
        cx.simulate_keystrokes("backspace enter");
        assert_eq!(f.backend_text(), "ab\ncde\n\n");
        cx.simulate_keystrokes("shift-up shift-up");
        assert_eq!(f.editor.read_with(cx, |e, _| e.selection()), 0..7);
        cx.simulate_keystrokes("delete");
        assert_eq!(f.backend_text(), "\n");
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn an_input_method_composes_in_place(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "x")]).await;
        f.editor.update_in(cx, |editor, window, cx| {
            editor.select(1, 1, cx);
            editor.replace_and_mark_text_in_range(None, "n", Some(1..1), window, cx);
            editor.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
            // The composition is in the document, marked, with the caret inside it.
            assert_eq!(editor.text(), "xni");
            assert_eq!(editor.marked_text_range(window, cx), Some(1..3));
            assert_eq!(editor.caret(), 3);
            // The platform asks where the composition is drawn, in UTF-16.
            assert_eq!(editor.text_for_range(1..3, &mut None, window, cx).as_deref(), Some("ni"));
            editor.replace_text_in_range(None, "你", window, cx);
            assert_eq!(editor.text(), "x你");
            assert_eq!(editor.marked_text_range(window, cx), None);
            assert_eq!(editor.caret(), 2);
            // Cancelling a composition leaves nothing behind.
            editor.replace_and_mark_text_in_range(None, "h", Some(1..1), window, cx);
            editor.replace_and_mark_text_in_range(None, "", None, window, cx);
            assert_eq!((editor.text().as_str(), editor.marked_text_range(window, cx)), ("x你", None));
        });
        assert_eq!(f.backend_text(), "x你");
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn columns_are_utf16_and_map_to_pixels_and_back(cx: &mut TestAppContext) {
        // a (1 unit) 😀 (2 units) 中 (1 unit) b (1 unit)
        let (f, cx) = open(cx, &[("notes.txt", "a😀中b\nsecond\n")]).await;
        let positions: Vec<Point<Pixels>> = f
            .editor
            .read_with(cx, |e, _| [0, 1, 3, 4, 5].map(|o| e.position_of(o).expect("the line is painted")).to_vec());
        assert!(positions.windows(2).all(|w| w[0].x < w[1].x && w[0].y == w[1].y), "{positions:?}");
        f.editor.read_with(cx, |editor, _| {
            for (offset, position) in [0, 1, 3, 4, 5].into_iter().zip(&positions) {
                assert_eq!(editor.offset_at(*position + point(px(0.0), px(3.0))), Some(offset));
            }
            // Inside the surrogate pair is not a position: it rounds to the emoji's start.
            assert_eq!(editor.position_of(2), editor.position_of(1));
            let second = editor.position_of(6).expect("the second line is painted");
            assert!(second.y > positions[0].y && second.x == positions[0].x);
        });
        // A click places the caret at the nearest character boundary.
        cx.simulate_click(positions[3] + point(px(0.0), px(3.0)), Modifiers::default());
        assert_eq!(f.editor.read_with(cx, |e, _| (e.caret(), e.caret_position())), (4, (0, 4)));
        // The platform's text input asks for the caret rectangle in the same coordinates.
        let caret = f
            .editor
            .update_in(cx, |e, window, cx| e.bounds_for_range(4..4, Bounds::default(), window, cx))
            .expect("a caret rectangle");
        assert_eq!(caret.origin, positions[3]);
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn a_long_file_costs_a_viewport_per_frame(cx: &mut TestAppContext) {
        let text: String = (1..=100_000).map(|n| format!("line {n}\n")).collect();
        let (f, cx) = open(cx, &[("long.txt", &text)]).await;
        let (painted, viewport_lines) = f.editor.read_with(cx, |e, cx| {
            let layout = e.layout.as_ref().expect("a frame was painted");
            (e.painted_lines().expect("painted"), (layout.bounds.size.height / Editor::metrics(cx).1).ceil() as u32)
        });
        assert_eq!(painted.start, 0);
        assert!(painted.len() as u32 <= viewport_lines + 1, "{painted:?} for a viewport of {viewport_lines} lines");
        assert_eq!(f.editor.read_with(cx, |e, _| e.line_count()), 100_001);
        // Scrolling to the end paints the end, still one viewport's worth.
        f.editor.update(cx, |e, cx| e.scroll_by(px(1e9), cx));
        cx.run_until_parked();
        let painted = f.editor.read_with(cx, |e, _| e.painted_lines().expect("painted"));
        assert_eq!(painted.end, 100_001);
        assert!(painted.len() as u32 <= viewport_lines + 1);
        // Typing at the end of a long file is one small edit, not a copy of the document.
        f.editor.update(cx, |e, cx| e.select(u32::MAX, u32::MAX, cx));
        cx.simulate_input("end");
        assert!(f.backend_text().ends_with("line 100000\nend"));
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn highlights_come_from_the_backends_grammar(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("build.zig", "const answer = 42; // why\n")]).await;
        settle(cx, |cx| !f.editor.read_with(cx, |e, _| e.highlights().is_empty()));
        f.editor.read_with(cx, |editor, _| {
            let kind_at =
                |offset: u32| editor.highlights().iter().find(|s| s.from <= offset && offset < s.to).map(|s| s.kind);
            assert_eq!(kind_at(0), Some(SyntaxHighlightKind::Keyword), "{:?}", editor.highlights());
            assert_eq!(kind_at(15), Some(SyntaxHighlightKind::Number));
            assert_eq!(kind_at(20), Some(SyntaxHighlightKind::Comment));
        });
        assert_eq!(syntax_token(SyntaxHighlightKind::Namespace), "syn-type");
        f.host.close_project("w1");
    }

    /// Languages the webview highlights with CodeMirror are highlighted by their grammar's
    /// own query here (ide-language's `native-highlights` feature).
    #[gpui_kit::test]
    async fn rust_is_highlighted_without_codemirror(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("main.rs", "fn main() {\n    let greeting = \"hi\"; // say it\n}\n")]).await;
        settle(cx, |cx| !f.editor.read_with(cx, |e, _| e.highlights().is_empty()));
        f.editor.read_with(cx, |editor, _| {
            let text = editor.text();
            let kind_of = |token: &str| {
                let at = text.find(token).expect("the token is in the text") as u32;
                editor.highlights().iter().find(|s| s.from <= at && at < s.to).map(|s| s.kind)
            };
            assert_eq!(kind_of("fn"), Some(SyntaxHighlightKind::Keyword));
            assert_eq!(kind_of("main"), Some(SyntaxHighlightKind::Function));
            assert_eq!(kind_of("\"hi\""), Some(SyntaxHighlightKind::String));
            assert_eq!(kind_of("// say it"), Some(SyntaxHighlightKind::Comment));
        });
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn the_backends_own_edits_move_the_caret(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "body\n")]).await;
        f.editor.update(cx, |e, cx| e.select(2, 2, cx));
        let doc = f.editor.read_with(cx, |e, _| e.doc());
        let start = Pos { line: 0, col: 0 };
        // A refactoring, a formatter or a reload from disk edits the document in the backend.
        f.documents()
            .apply_edits(
                doc,
                &[TextEdit { range: ide_core::Range { start, end: start }, new_text: "// header\n".to_owned() }],
            )
            .expect("the edit applies");
        settle(cx, |cx| f.editor.read_with(cx, |e, _| e.text().starts_with("// header")));
        f.editor.read_with(cx, |editor, _| {
            assert_eq!(editor.text(), "// header\nbody\n");
            assert_eq!(editor.caret_position(), (1, 2));
        });
        f.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn typing_survives_a_backend_edit_that_has_not_arrived_yet(cx: &mut TestAppContext) {
        let (f, cx) = open(cx, &[("notes.txt", "body\n")]).await;
        f.editor.update(cx, |e, cx| e.select(4, 4, cx));
        let doc = f.editor.read_with(cx, |e, _| e.doc());
        let start = Pos { line: 0, col: 0 };
        f.documents()
            .apply_edits(doc, &[TextEdit { range: ide_core::Range { start, end: start }, new_text: ">> ".to_owned() }])
            .expect("the edit applies");
        // The editor still shows the old version: its edit is rejected as out of sync, the text
        // is refreshed and the typed character goes in at the caret's new place.
        cx.simulate_input("!");
        assert_eq!(f.backend_text(), ">> body!\n");
        assert_eq!(f.editor.read_with(cx, |e, _| (e.text(), e.caret())), (">> body!\n".to_owned(), 8));
        f.host.close_project("w1");
    }

    #[test]
    fn finds_the_difference_between_two_texts() {
        let diff = |old: &str, new: &str| difference(&Rope::from_str(old), &Rope::from_str(new));
        assert_eq!(diff("same", "same"), []);
        assert_eq!(diff("body", ">> body"), [TextChange { from: 0, to: 0, insert: ">> ".to_owned() }]);
        assert_eq!(diff("a😀c", "a😀bc"), [TextChange { from: 3, to: 3, insert: "b".to_owned() }]);
        assert_eq!(diff("hello world", "hello"), [TextChange { from: 5, to: 11, insert: String::new() }]);
        assert_eq!(diff("aaa", "aa"), [TextChange { from: 2, to: 3, insert: String::new() }]);
        assert_eq!(diff("", "x"), [TextChange { from: 0, to: 0, insert: "x".to_owned() }]);
    }
}

#[cfg(test)]
mod editing_tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use ide_core::{Pos, TextEdit};
    use std::time::Duration;

    struct Opened {
        editor: Entity<Editor>,
        host: Arc<Host>,
        path: PathBuf,
        _folders: (tempfile::TempDir, tempfile::TempDir),
    }

    async fn open<'a>(cx: &'a mut TestAppContext, text: &str) -> (Opened, &'a mut VisualTestContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        let path = root.join("notes.txt");
        std::fs::write(&path, text).expect("the file is written");
        cx.update(|cx| {
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            cx.set_global(theme::ActiveTheme::default());
        });
        let (open_root, open_path) = (root.clone(), path.clone());
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await?;
                ide_host::ops::doc::open(&host, "w1", open_path).await
            })
        });
        let info = opened.await.expect("the task completes").expect("the document opens");
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        let editor_host = host.clone();
        let (editor, cx) =
            cx.add_window_view(move |_, cx| Editor::new(editor_host, "w1", &info, cx).expect("the editor opens"));
        cx.update(|window, cx| {
            let focus = editor.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (Opened { editor, host, path, _folders: (home, project) }, cx)
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.executor().advance_clock(Duration::from_millis(50));
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    fn text(o: &Opened, cx: &mut VisualTestContext) -> String {
        o.editor.read_with(cx, |e, _| e.text())
    }

    #[gpui_kit::test]
    async fn undo_and_redo_walk_groups_of_typing(cx: &mut TestAppContext) {
        let (o, cx) = open(cx, "end\n").await;
        cx.simulate_input("one ");
        // Moving the caret ends the group; typing after it is a second one.
        cx.simulate_keystrokes("right left");
        cx.simulate_input("two ");
        assert_eq!(text(&o, cx), "one two end\n");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&o, cx), "one end\n");
        assert_eq!(o.editor.read_with(cx, |e, _| e.caret()), 4);
        cx.simulate_keystrokes("cmd-z");
        assert_eq!((text(&o, cx).as_str(), o.editor.read_with(cx, |e, _| e.caret())), ("end\n", 0));
        // Nothing left to undo: the key does nothing.
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&o, cx), "end\n");
        cx.simulate_keystrokes("cmd-shift-z cmd-shift-z");
        assert_eq!((text(&o, cx).as_str(), o.editor.read_with(cx, |e, _| e.caret())), ("one two end\n", 8));
        // The backend holds the same text after every step.
        assert_eq!(o.host.project("w1").unwrap().documents.text_of(&o.path).as_deref(), Some("one two end\n"));
        // A new edit after an undo drops what could be redone.
        cx.simulate_keystrokes("cmd-z");
        cx.simulate_input("x");
        assert!(!o.editor.read_with(cx, |e, _| e.can_redo()));
        o.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn a_backend_edit_is_undone_like_any_other(cx: &mut TestAppContext) {
        let (o, cx) = open(cx, "body\n").await;
        let doc = o.editor.read_with(cx, |e, _| e.doc());
        let start = Pos { line: 0, col: 0 };
        let edit = TextEdit { range: ide_core::Range { start, end: start }, new_text: "// formatted\n".to_owned() };
        o.host.project("w1").unwrap().documents.apply_edits(doc, &[edit]).expect("the edit applies");
        settle(cx, |cx| o.editor.read_with(cx, |e, _| e.text().starts_with("// formatted")));
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&o, cx), "body\n");
        cx.simulate_keystrokes("cmd-shift-z");
        assert_eq!(text(&o, cx), "// formatted\nbody\n");
        o.host.close_project("w1");
    }

    // src/editor/core/cm/caretCommands.test.ts, baseKeymap.ts (word, line and document movement)
    #[gpui_kit::test]
    async fn option_and_command_move_by_word_line_and_document(cx: &mut TestAppContext) {
        let (o, cx) = open(cx, "alpha beta gamma\nsecond line\n").await;
        let caret = |cx: &mut VisualTestContext| o.editor.read_with(cx, |e, _| e.caret());
        cx.simulate_keystrokes("alt-right alt-right");
        assert_eq!(caret(cx), 10);
        cx.simulate_keystrokes("alt-left");
        assert_eq!(caret(cx), 6);
        cx.simulate_keystrokes("cmd-right");
        assert_eq!(caret(cx), 16);
        cx.simulate_keystrokes("shift-cmd-left");
        assert_eq!(o.editor.read_with(cx, |e, _| e.selection()), 0..16);
        cx.simulate_keystrokes("cmd-down");
        assert_eq!(caret(cx), 29);
        cx.simulate_keystrokes("cmd-up alt-right alt-right");
        // ⌥⌫ deletes the word before the caret, ⌥⌦ the one after, ⌘⌫ to the line's start.
        cx.simulate_keystrokes("alt-backspace");
        assert_eq!(text(&o, cx), "alpha  gamma\nsecond line\n");
        cx.simulate_keystrokes("alt-delete");
        assert_eq!(text(&o, cx), "alpha \nsecond line\n");
        cx.simulate_keystrokes("cmd-backspace");
        assert_eq!(text(&o, cx), "\nsecond line\n");
        // Each is one undo step.
        cx.simulate_keystrokes("cmd-z cmd-z cmd-z");
        assert_eq!(text(&o, cx), "alpha beta gamma\nsecond line\n");
        o.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn the_mouse_selects_by_drag_word_and_line(cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseDownEvent, MouseUpEvent};
        let (o, cx) = open(cx, "alpha beta\nsecond\n").await;
        let at = |cx: &mut VisualTestContext, offset: u32| {
            o.editor.read_with(cx, |e, _| e.position_of(offset).expect("painted")) + point(px(0.0), px(3.0))
        };
        // Press at 2, drag to 8, release.
        let (from, to) = (at(cx, 2), at(cx, 8));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::default());
        assert_eq!(o.editor.read_with(cx, |e, _| (e.selection(), e.caret())), (2..8, 8));
        // Moving without a button held does not select.
        let origin = at(cx, 0);
        cx.simulate_mouse_move(origin, None, Modifiers::default());
        assert_eq!(o.editor.read_with(cx, |e, _| e.selection()), 2..8);
        // A double click selects the word, a triple click the line with its break.
        let word = at(cx, 7);
        let click = |cx: &mut VisualTestContext, count: usize| {
            cx.simulate_event(MouseDownEvent {
                position: word,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
                first_mouse: false,
            });
            cx.simulate_event(MouseUpEvent {
                position: word,
                modifiers: Modifiers::default(),
                button: MouseButton::Left,
                click_count: count,
            });
        };
        click(cx, 2);
        assert_eq!(o.editor.read_with(cx, |e, _| e.selected_text()), "beta");
        click(cx, 3);
        assert_eq!(o.editor.read_with(cx, |e, _| e.selected_text()), "alpha beta\n");
        // Shift+click extends from the anchor.
        let (first, second) = (at(cx, 1), at(cx, 5));
        cx.simulate_click(first, Modifiers::default());
        cx.simulate_click(second, Modifiers { shift: true, ..Modifiers::default() });
        assert_eq!(o.editor.read_with(cx, |e, _| e.selection()), 1..5);
        o.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn clipboard_keys_copy_cut_and_paste(cx: &mut TestAppContext) {
        let (o, cx) = open(cx, "alpha beta\n").await;
        o.editor.update(cx, |e, cx| e.select(0, 6, cx));
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("alpha "));
        cx.simulate_keystrokes("end cmd-v");
        assert_eq!(text(&o, cx), "alpha betaalpha \n");
        o.editor.update(cx, |e, cx| e.select(0, 6, cx));
        cx.simulate_keystrokes("cmd-x");
        assert_eq!(text(&o, cx), "betaalpha \n");
        // Without a selection nothing is copied: the clipboard keeps the cut text.
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("alpha "));
        cx.simulate_keystrokes("cmd-a");
        assert_eq!(o.editor.read_with(cx, |e, _| e.selection()), 0..11);
        // A paste is one undo step, whatever its length.
        cx.simulate_keystrokes("cmd-v cmd-z");
        assert_eq!(text(&o, cx), "betaalpha \n");
        o.host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn saving_writes_the_file_and_clears_the_dirty_state(cx: &mut TestAppContext) {
        let (o, cx) = open(cx, "draft\n").await;
        assert!(!o.editor.read_with(cx, |e, _| e.is_dirty()));
        cx.simulate_input("final ");
        settle(cx, |cx| o.editor.read_with(cx, |e, _| e.is_dirty()));
        assert_eq!(std::fs::read_to_string(&o.path).expect("readable"), "draft\n");
        let saved = o.editor.read_with(cx, |e, cx| e.save(cx));
        saved.await.expect("the task completes").expect("the file saves");
        assert_eq!(std::fs::read_to_string(&o.path).expect("readable"), "final draft\n");
        settle(cx, |cx| !o.editor.read_with(cx, |e, _| e.is_dirty()));
        o.host.close_project("w1");
    }
}
