//! A multi-line text input: the agents composer and message editor, commit messages, the Find
//! bar's multi-line query, run configuration fields, long database values. It edits its own text
//! with the caret, selection, clipboard, undo and input method support every text input has,
//! soft-wraps it to its width, grows with its content between a minimum and a maximum number of
//! rows and scrolls beyond that. It has the shape of [`TextField`](super::text_field::TextField):
//! the same constructor, text accessors and events, with byte offsets in its public API.
//!
//! Enter inserts a line break. An owner that submits on Enter (or on Shift+Enter, or on a
//! modified Enter) claims that key with [`crate::platform::input::claim_keys`]; what it does not
//! claim reaches the area.

use super::text_field::TEXT_FIELD_CONTEXT;
use crate::platform::keymap::is_mac_host;
use crate::theme;
use gpui_kit::{
    App, AvailableSpace, Bounds, ClipboardItem, ContentMask, Context, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, Render, ScrollEvent, SharedString, Size, Style, Subscription, TextAlign, TextRun, UTF16Selection,
    Window, WrappedLine, div, fill, font, point, prelude::*, px, relative, size,
};
use std::ops::Range;

/// The caret's width; the wrap width leaves room for it at the end of a full row.
const CARET_WIDTH: f32 = 1.5;
/// How much of a selection shows past the end of a line it continues beyond: the line break.
const LINE_BREAK_WIDTH: f32 = 4.0;
/// Undo steps kept; the oldest go first.
const UNDO_LIMIT: usize = 200;

/// What a text area tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAreaEvent {
    /// The text is different now.
    Changed,
    /// The owner asked the area to submit ([`TextArea::submit`]), usually from a key it claimed.
    Submitted,
    /// Escape was pressed.
    Cancelled,
    /// The area lost the keyboard focus.
    Blurred,
}

/// How tall the area is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sizing {
    /// As many rows as the wrapped text needs, between `min` and `max`; scrolls beyond `max`.
    Rows { min: usize, max: usize },
    /// The height its owner gives it; scrolls beyond it.
    Fill,
}

/// What an edit was, to decide whether it joins the previous undo step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditKind {
    Typing,
    Deleting,
    Composing,
    Other,
}

/// The text and selection before an undo step.
struct Snapshot {
    text: String,
    anchor: usize,
    head: usize,
}

/// Where the area was last painted, for clicks, the input method, scrolling and paging.
struct Painted {
    bounds: Bounds<Pixels>,
    /// The width rows wrap at; none without soft wrapping.
    wrap: Option<Pixels>,
    /// The size of the whole laid-out text.
    content: Size<Pixels>,
}

pub struct TextArea {
    focus: FocusHandle,
    text: String,
    /// The selection's fixed end and the caret, as byte offsets on character boundaries.
    anchor: usize,
    head: usize,
    placeholder: SharedString,
    /// The text being composed by an input method.
    marked: Option<Range<usize>>,
    soft_wrap: bool,
    sizing: Sizing,
    disabled: bool,
    read_only: bool,
    /// The x a run of Up/Down/Page presses aims for, kept across shorter rows.
    goal_x: Option<Pixels>,
    /// Where a mouse drag started; the selection runs from it to the pointer.
    drag_anchor: Option<usize>,
    /// How far the text is scrolled, right and down.
    scroll: Point<Pixels>,
    /// The caret moved: the next paint scrolls it into view.
    reveal: bool,
    painted: Option<Painted>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<EditKind>,
    blur: Option<Subscription>,
}

impl EventEmitter<TextAreaEvent> for TextArea {}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The byte offset of UTF-16 offset `at` in `text`, clamped to its end.
fn byte_of(text: &str, at: usize) -> usize {
    let mut units = 0;
    for (byte, c) in text.char_indices() {
        if units >= at {
            return byte;
        }
        units += c.len_utf16();
    }
    text.len()
}

/// Line breaks as one `\n`, whatever platform wrote them.
fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// The offset one character (or, with `word`, one word) from `from`.
fn boundary(text: &str, from: usize, forward: bool, word: bool) -> usize {
    let step = |at: usize| -> Option<(usize, char)> {
        if forward {
            text[at..].chars().next().map(|c| (at + c.len_utf8(), c))
        } else {
            text[..at].chars().next_back().map(|c| (at - c.len_utf8(), c))
        }
    };
    let Some((mut at, first)) = step(from) else { return from };
    if !word {
        return at;
    }
    // A word step crosses the white space it starts in, then one run of like characters.
    let is_word = |c: char| c.is_alphanumeric() || c == '_';
    let mut kind = first;
    while kind.is_whitespace() {
        let Some((next, c)) = step(at) else { return at };
        (at, kind) = (next, c);
    }
    while let Some((next, c)) = step(at) {
        if c.is_whitespace() || is_word(c) != is_word(kind) {
            break;
        }
        at = next;
    }
    at
}

/// The run of like characters around `at` (a word, white space or punctuation), within its line.
fn word_range(text: &str, at: usize) -> Range<usize> {
    fn class(c: char) -> u8 {
        match c {
            '\n' => 0,
            c if c.is_alphanumeric() || c == '_' => 1,
            c if c.is_whitespace() => 2,
            _ => 3,
        }
    }
    let (before, after) = (text[..at].chars().next_back(), text[at..].chars().next());
    let kind = match (before, after) {
        (_, Some(c)) if class(c) == 1 => 1,
        (Some(c), _) if class(c) == 1 => 1,
        (_, Some(c)) if c != '\n' => class(c),
        (Some(c), _) if c != '\n' => class(c),
        _ => return at..at,
    };
    let start = text[..at].char_indices().rev().take_while(|&(_, c)| class(c) == kind).last().map_or(at, |(i, _)| i);
    let end = text[at..]
        .char_indices()
        .take_while(|&(_, c)| class(c) == kind)
        .last()
        .map_or(at, |(i, c)| at + i + c.len_utf8());
    start..end
}

/// The line around `at` with its line break, so that typing over it replaces the line.
fn line_range(text: &str, at: usize) -> Range<usize> {
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i + 1);
    start..end
}

/// The font size and the row height of the area's text.
fn metrics(cx: &App) -> (Pixels, Pixels) {
    let font_size = theme::length(cx, "fs-ui");
    (font_size, font_size * 1.5)
}

fn ui_font(cx: &App) -> Font {
    let family = cx
        .try_global::<theme::ActiveTheme>()
        .and_then(|active| active.theme.font_families("font-ui").into_iter().next())
        .unwrap_or_else(|| ".SystemUIFont".to_owned());
    font(SharedString::from(family))
}

/// One visual row: a whole line, or the part of a line between two soft wraps.
struct Row {
    /// Byte offsets in the text; `end` is the next row's start, or the line's end.
    start: usize,
    end: usize,
    /// The line the row is part of.
    line: usize,
    /// Where the row starts in its unwrapped line.
    x0: Pixels,
}

/// One line of text (between line breaks), shaped and wrapped.
struct Line {
    start: usize,
    shaped: WrappedLine,
    first_row: usize,
}

/// The text laid out in visual rows: what the caret, the selection, clicks and vertical
/// movement are measured against.
struct Laid {
    lines: Vec<Line>,
    rows: Vec<Row>,
    line_height: Pixels,
}

impl Laid {
    /// The row showing offset `at`. An offset where a line wraps shows at the start of the next
    /// row; the end of a line shows on its last row.
    fn row_of(&self, at: usize) -> usize {
        self.rows.partition_point(|row| row.start <= at).saturating_sub(1)
    }

    /// Whether `row` is the last row of its line (the line break, if any, follows it).
    fn ends_line(&self, row: usize) -> bool {
        self.rows.get(row + 1).is_none_or(|next| next.line != self.rows[row].line)
    }

    /// The x of offset `at` (clamped to `row`) from the row's left edge.
    fn x_of(&self, row: usize, at: usize) -> Pixels {
        let row = &self.rows[row];
        let line = &self.lines[row.line];
        line.shaped.unwrapped_layout.x_for_index(at.clamp(row.start, row.end) - line.start) - row.x0
    }

    /// The last offset the caret takes on `row`: its end, or before its last character when the
    /// line wraps after it (the wrap offset itself shows on the next row).
    fn row_last(&self, row: usize, text: &str) -> usize {
        let r = &self.rows[row];
        if self.ends_line(row) {
            r.end
        } else {
            text[r.start..r.end].char_indices().next_back().map_or(r.start, |(i, _)| r.start + i)
        }
    }

    /// The offset on `row` closest to `x`.
    fn index_at(&self, row: usize, x: Pixels, text: &str) -> usize {
        let r = &self.rows[row];
        let line = &self.lines[r.line];
        let at = line.start + line.shaped.unwrapped_layout.closest_index_for_x(x.max(Pixels::ZERO) + r.x0);
        at.clamp(r.start, self.row_last(row, text))
    }

    /// The row at height `y` from the top of the text.
    fn row_at_y(&self, y: Pixels) -> usize {
        let row = if y <= Pixels::ZERO { 0 } else { (y / self.line_height) as usize };
        row.min(self.rows.len().saturating_sub(1))
    }

    fn height(&self) -> Pixels {
        self.line_height * self.rows.len() as f32
    }

    /// The width of the widest row.
    fn width(&self) -> Pixels {
        self.rows.iter().enumerate().map(|(index, row)| self.x_of(index, row.end)).max().unwrap_or(Pixels::ZERO)
    }

    /// The areas `range` covers, one per row, from the top left of the text at `origin`.
    fn areas(&self, range: &Range<usize>, origin: Point<Pixels>) -> Vec<Bounds<Pixels>> {
        let mut areas = Vec::new();
        for (index, row) in self.rows.iter().enumerate() {
            let ends_line = self.ends_line(index);
            if range.end <= row.start || range.start > row.end || (range.start == row.end && !ends_line) {
                continue;
            }
            let left = self.x_of(index, range.start.max(row.start));
            let mut right = self.x_of(index, range.end.min(row.end));
            if ends_line && range.end > row.end {
                right += px(LINE_BREAK_WIDTH);
            }
            if right > left {
                let top = origin.y + self.line_height * index as f32;
                areas.push(Bounds::new(point(origin.x + left, top), size(right - left, self.line_height)));
            }
        }
        areas
    }
}

/// Shapes `text` line by line, wrapping at `wrap` when given, and splits it into visual rows.
fn lay_out(text: &str, wrap: Option<Pixels>, color: Hsla, window: &Window, cx: &App) -> Laid {
    let (font_size, line_height) = metrics(cx);
    let font = ui_font(cx);
    let wrap = wrap.filter(|width| *width > Pixels::ZERO);
    let (mut lines, mut rows) = (Vec::new(), Vec::new());
    let mut start = 0;
    for (index, part) in text.split('\n').enumerate() {
        let run = TextRun {
            len: part.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window
            .text_system()
            .shape_text(SharedString::from(part.to_owned()), font_size, &[run], wrap, None)
            .ok()
            .and_then(|shaped| shaped.into_iter().next())
            .unwrap_or_default();
        let mut starts = vec![0];
        for wrapped in shaped.wrap_boundaries() {
            let runs = &shaped.unwrapped_layout.runs;
            let glyph = runs.get(wrapped.run_ix).and_then(|run| run.glyphs.get(wrapped.glyph_ix));
            let last = starts.last().copied().unwrap_or(0);
            if let Some(glyph) = glyph.filter(|glyph| glyph.index > last && glyph.index <= part.len()) {
                starts.push(glyph.index);
            }
        }
        let first_row = rows.len();
        for (i, &from) in starts.iter().enumerate() {
            let to = starts.get(i + 1).copied().unwrap_or(part.len());
            let x0 = shaped.unwrapped_layout.x_for_index(from);
            rows.push(Row { start: start + from, end: start + to, line: index, x0 });
        }
        lines.push(Line { start, shaped, first_row });
        start += part.len() + 1;
    }
    Laid { lines, rows, line_height }
}

impl TextArea {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            anchor: 0,
            head: 0,
            placeholder: SharedString::default(),
            marked: None,
            soft_wrap: true,
            sizing: Sizing::Rows { min: 1, max: 8 },
            disabled: false,
            read_only: false,
            goal_x: None,
            drag_anchor: None,
            scroll: Point::default(),
            reveal: false,
            painted: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            blur: None,
        }
    }

    /// The text shown, muted, while the area is empty.
    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text, puts the caret at its end and forgets the undo history. The owner is
    /// not told: it knows.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = normalize_breaks(text);
        (self.anchor, self.head) = (self.text.len(), self.text.len());
        self.marked = None;
        self.undo.clear();
        self.redo.clear();
        (self.last_edit, self.goal_x, self.reveal) = (None, None, true);
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        (self.anchor, self.head) = (0, self.text.len());
        (self.last_edit, self.goal_x) = (None, None);
        cx.notify();
    }

    /// The selected bytes, in order.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Wraps long lines at the area's width (the default); without it the area scrolls sideways.
    pub fn set_soft_wrap(&mut self, soft_wrap: bool, cx: &mut Context<Self>) {
        self.soft_wrap = soft_wrap;
        (self.scroll.x, self.reveal) = (Pixels::ZERO, true);
        cx.notify();
    }

    /// Grows with the text from `min` to `max` visual rows (at least one), then scrolls. The
    /// default is 1 to 8.
    pub fn set_rows(&mut self, min: usize, max: usize, cx: &mut Context<Self>) {
        let min = min.max(1);
        self.sizing = Sizing::Rows { min, max: max.max(min) };
        self.reveal = true;
        cx.notify();
    }

    /// Takes the height its owner gives it (the full height of its parent) instead of sizing to
    /// its rows, and scrolls beyond it.
    pub fn set_fill_height(&mut self, cx: &mut Context<Self>) {
        self.sizing = Sizing::Fill;
        self.reveal = true;
        cx.notify();
    }

    /// A disabled area is dimmed and takes no input: no typing, no selecting, no focus by click.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        self.marked = None;
        self.drag_anchor = None;
        cx.notify();
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// A read-only area can be focused, selected and copied, but not edited.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        self.read_only = read_only;
        self.marked = None;
        cx.notify();
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Tells the owner the text is submitted; owners call it from the key they claimed for it.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        cx.emit(TextAreaEvent::Submitted);
    }

    /// Takes back the last group of edits (a run of typing, of deleting, a paste, a line break).
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        let Some(before) = self.undo.pop() else { return };
        let now = self.snapshot();
        self.redo.push(now);
        self.restore(before, cx);
    }

    /// Makes again what [`TextArea::undo`] took back.
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        let Some(after) = self.redo.pop() else { return };
        let now = self.snapshot();
        self.undo.push(now);
        self.restore(after, cx);
    }

    fn editable(&self) -> bool {
        !self.disabled && !self.read_only
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { text: self.text.clone(), anchor: self.anchor, head: self.head }
    }

    fn restore(&mut self, snapshot: Snapshot, cx: &mut Context<Self>) {
        self.text = snapshot.text;
        (self.anchor, self.head) = (snapshot.anchor, snapshot.head);
        (self.marked, self.last_edit, self.goal_x, self.reveal) = (None, None, None, true);
        cx.emit(TextAreaEvent::Changed);
        cx.notify();
    }

    /// Starts a new undo step for an edit of `kind` replacing `range` with `inserted`, unless it
    /// continues the previous one: typing on at the caret (a new word starts a new step),
    /// deleting on from the caret, or composing the same input method text.
    fn record(&mut self, kind: EditKind, range: &Range<usize>, inserted: &str) {
        let caret = self.anchor == self.head;
        let continues = match (self.last_edit, kind) {
            (Some(EditKind::Typing), EditKind::Typing) => {
                let new_word = inserted.starts_with(char::is_whitespace)
                    && !self.text[..range.start].ends_with(char::is_whitespace);
                caret && range.is_empty() && range.start == self.head && !inserted.contains('\n') && !new_word
            }
            (Some(EditKind::Deleting), EditKind::Deleting) => {
                caret && (range.end == self.head || range.start == self.head)
            }
            (Some(EditKind::Composing), EditKind::Composing) => true,
            _ => false,
        };
        if !continues {
            if self.undo.len() >= UNDO_LIMIT {
                self.undo.remove(0);
            }
            let before = self.snapshot();
            self.undo.push(before);
        }
        self.redo.clear();
        self.last_edit = Some(kind);
    }

    /// Replaces `range` (the selection when none is given) with `new_text`.
    fn edit(&mut self, range: Option<Range<usize>>, new_text: &str, kind: EditKind, cx: &mut Context<Self>) {
        if !self.editable() {
            return;
        }
        let range = range.unwrap_or_else(|| self.selection());
        let inserted = normalize_breaks(new_text);
        if range.is_empty() && inserted.is_empty() {
            return;
        }
        self.record(kind, &range, &inserted);
        self.text.replace_range(range.clone(), &inserted);
        let caret = range.start + inserted.len();
        (self.anchor, self.head) = (caret, caret);
        (self.goal_x, self.reveal) = (None, true);
        cx.emit(TextAreaEvent::Changed);
        cx.notify();
    }

    fn select_range(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        (self.anchor, self.head) = (anchor, head);
        (self.last_edit, self.goal_x, self.reveal) = (None, None, true);
        cx.notify();
    }

    fn move_to(&mut self, to: usize, extend: bool, cx: &mut Context<Self>) {
        let anchor = if extend { self.anchor } else { to };
        self.select_range(anchor, to, cx);
    }

    /// The text laid out as it was last painted (at the same wrap width).
    fn laid(&self, window: &Window, cx: &App) -> Laid {
        let wrap = self.painted.as_ref().and_then(|painted| painted.wrap);
        lay_out(&self.text, wrap, theme::color(cx, "fg-strong"), window, cx)
    }

    /// The start (or the caret's last offset) of the visual row the caret is on.
    fn row_edge(&self, end: bool, window: &Window, cx: &App) -> usize {
        let laid = self.laid(window, cx);
        let row = laid.row_of(self.head);
        if end { laid.row_last(row, &self.text) } else { laid.rows[row].start }
    }

    /// Moves the caret `rows` visual rows down (up when negative), aiming at the column the run
    /// of vertical moves started in. Past the first row it goes to the start, past the last to
    /// the end.
    fn vertical(&mut self, rows: isize, extend: bool, window: &Window, cx: &mut Context<Self>) {
        let laid = self.laid(window, cx);
        let row = laid.row_of(self.head);
        let goal = self.goal_x.unwrap_or_else(|| laid.x_of(row, self.head));
        let target = row as isize + rows;
        let to = if target < 0 {
            0
        } else if target as usize >= laid.rows.len() {
            self.text.len()
        } else {
            laid.index_at(target as usize, goal, &self.text)
        };
        self.move_to(to, extend, cx);
        self.goal_x = Some(goal);
    }

    /// The rows a Page Up / Page Down moves by: those visible.
    fn page_rows(&self, cx: &App) -> isize {
        let height = self.painted.as_ref().map_or(Pixels::ZERO, |painted| painted.bounds.size.height);
        ((height / metrics(cx).1).floor() as isize).max(1)
    }

    /// The offset under the window position `position`.
    fn offset_at(&self, position: Point<Pixels>, window: &Window, cx: &App) -> Option<usize> {
        let painted = self.painted.as_ref()?;
        let laid = self.laid(window, cx);
        let local = position - painted.bounds.origin + self.scroll;
        Some(laid.index_at(laid.row_at_y(local.y), local.x, &self.text))
    }

    /// Scrolls by a wheel's `delta`, within the text; whether anything moved.
    fn scroll_by(&mut self, delta: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(painted) = &self.painted else { return false };
        let viewport = painted.bounds.size;
        let max_x = (painted.content.width - viewport.width).max(Pixels::ZERO);
        let max_y = (painted.content.height - viewport.height).max(Pixels::ZERO);
        let next = point(
            (self.scroll.x - delta.x).clamp(Pixels::ZERO, max_x),
            (self.scroll.y - delta.y).clamp(Pixels::ZERO, max_y),
        );
        if next == self.scroll {
            return false;
        }
        self.scroll = next;
        cx.notify();
        true
    }

    /// Scrolls the caret into view after it moved, and keeps the scroll within the text.
    fn keep_in_view(&mut self, laid: &Laid, viewport: Size<Pixels>) {
        if self.reveal && viewport.height > Pixels::ZERO {
            let row = laid.row_of(self.head);
            let top = laid.line_height * row as f32;
            let bottom = top + laid.line_height;
            if top < self.scroll.y {
                self.scroll.y = top;
            } else if bottom > self.scroll.y + viewport.height {
                self.scroll.y = bottom - viewport.height;
            }
            if !self.soft_wrap {
                let (x, caret) = (laid.x_of(row, self.head), px(CARET_WIDTH));
                if x < self.scroll.x {
                    self.scroll.x = x;
                } else if x + caret > self.scroll.x + viewport.width {
                    self.scroll.x = x + caret - viewport.width;
                }
            }
            self.reveal = false;
        }
        let max_y = (laid.height() - viewport.height).max(Pixels::ZERO);
        let max_x = if self.soft_wrap {
            Pixels::ZERO
        } else {
            (laid.width() + px(CARET_WIDTH) - viewport.width).max(Pixels::ZERO)
        };
        self.scroll = point(self.scroll.x.clamp(Pixels::ZERO, max_x), self.scroll.y.clamp(Pixels::ZERO, max_y));
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let keystroke = &event.keystroke;
        let (modifiers, key) = (keystroke.modifiers, keystroke.key.as_str());
        let editable = !self.read_only;
        let selection = self.selection();
        if modifiers.secondary(cx) && !modifiers.alt {
            let handled = match (key, modifiers.shift) {
                ("a", false) => {
                    self.select_all(cx);
                    true
                }
                ("c" | "x", false) if !selection.is_empty() => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text().to_owned()));
                    if key == "x" {
                        self.edit(None, "", EditKind::Other, cx);
                    }
                    true
                }
                ("v", false) if editable => match cx.read_from_clipboard().and_then(|item| item.text()) {
                    Some(text) => {
                        self.edit(None, &text, EditKind::Other, cx);
                        true
                    }
                    None => false,
                },
                ("z", false) if editable => {
                    self.undo(cx);
                    true
                }
                ("z", true) if editable => {
                    self.redo(cx);
                    true
                }
                ("y", false) if editable && !is_mac_host() => {
                    self.redo(cx);
                    true
                }
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }
        let (word, line, extend) = (modifiers.alt, modifiers.platform, modifiers.shift);
        let document = modifiers.platform || modifiers.control;
        let plain_or_shift = !(modifiers.control || modifiers.alt || modifiers.platform || modifiers.function);
        match key {
            "left" | "right" => {
                let forward = key == "right";
                let to = if line {
                    self.row_edge(forward, window, cx)
                } else if !extend && !selection.is_empty() {
                    // A plain arrow collapses the selection to the side it points at.
                    if forward { selection.end } else { selection.start }
                } else {
                    boundary(&self.text, self.head, forward, word)
                };
                self.move_to(to, extend, cx);
            }
            "up" | "down" if line => {
                let to = if key == "up" { 0 } else { self.text.len() };
                self.move_to(to, extend, cx);
            }
            "up" | "down" => self.vertical(if key == "up" { -1 } else { 1 }, extend, window, cx),
            "pageup" | "pagedown" => {
                let page = self.page_rows(cx);
                self.vertical(if key == "pageup" { -page } else { page }, extend, window, cx);
            }
            "home" | "end" if document => {
                let to = if key == "home" { 0 } else { self.text.len() };
                self.move_to(to, extend, cx);
            }
            "home" | "end" => {
                let to = self.row_edge(key == "end", window, cx);
                self.move_to(to, extend, cx);
            }
            "backspace" | "delete" if editable => {
                let forward = key == "delete";
                let (range, kind) = if !selection.is_empty() {
                    (selection, EditKind::Other)
                } else if line && !forward {
                    // Back to the start of the visual row; at its start, the character before.
                    let start = self.row_edge(false, window, cx);
                    let start = if start < self.head { start } else { boundary(&self.text, self.head, false, false) };
                    (start..self.head, EditKind::Other)
                } else {
                    let to = boundary(&self.text, self.head, forward, word);
                    let kind = if word { EditKind::Other } else { EditKind::Deleting };
                    (to.min(self.head)..to.max(self.head), kind)
                };
                if !range.is_empty() {
                    self.edit(Some(range), "", kind, cx);
                }
            }
            "enter" if editable && plain_or_shift => {
                self.edit(None, "\n", EditKind::Other, cx);
            }
            "escape" if !modifiers.modified() => cx.emit(TextAreaEvent::Cancelled),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// A click places the caret (Shift extends), a double click selects the word, a triple click
    /// the line; dragging extends from where the press began.
    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus, cx);
        // The press is the area's: a parent tracking focus does not take it back.
        window.prevent_default();
        let Some(at) = self.offset_at(event.position, window, cx) else { return };
        let (anchor, head) = match event.click_count {
            0 | 1 => (if event.modifiers.shift { self.anchor } else { at }, at),
            2 => {
                let word = word_range(&self.text, at);
                (word.start, word.end)
            }
            _ => {
                let line = line_range(&self.text, at);
                (line.start, line.end)
            }
        };
        self.select_range(anchor, head, cx);
        self.drag_anchor = Some(anchor);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.drag_anchor = None;
            return;
        }
        let Some(anchor) = self.drag_anchor else { return };
        let Some(at) = self.offset_at(event.position, window, cx) else { return };
        if at != self.head {
            self.select_range(anchor, at, cx);
        }
    }
}

impl Focusable for TextArea {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TextArea {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let len = utf16_len(&self.text);
        let clamped = range.start.min(len)..range.end.min(len);
        if clamped != range {
            *adjusted = Some(clamped.clone());
        }
        Some(self.text[byte_of(&self.text, clamped.start)..byte_of(&self.text, clamped.end)].to_owned())
    }

    fn selected_text_range(&mut self, _: bool, _: &mut Window, _: &mut Context<Self>) -> Option<UTF16Selection> {
        let selection = self.selection();
        let units = |byte: usize| utf16_len(&self.text[..byte]);
        Some(UTF16Selection { range: units(selection.start)..units(selection.end), reversed: self.head < self.anchor })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let marked = self.marked.as_ref()?;
        Some(utf16_len(&self.text[..marked.start])..utf16_len(&self.text[..marked.end]))
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
        // Committed text replaces the composition, else the given range, else the selection; it
        // joins the composition's undo step.
        let composing = self.marked.is_some();
        let range =
            range.map(|r| byte_of(&self.text, r.start)..byte_of(&self.text, r.end)).or_else(|| self.marked.take());
        self.marked = None;
        let kind = if composing {
            EditKind::Composing
        } else if text.contains(['\n', '\r']) {
            EditKind::Other
        } else {
            EditKind::Typing
        };
        self.edit(range, text, kind, cx);
        if composing && self.last_edit == Some(EditKind::Composing) {
            self.last_edit = Some(EditKind::Typing);
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.editable() {
            return;
        }
        let range = range
            .map(|r| byte_of(&self.text, r.start)..byte_of(&self.text, r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let start = range.start;
        self.edit(Some(range), new_text, EditKind::Composing, cx);
        self.marked = (self.head > start).then_some(start..self.head);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let painted = self.painted.as_ref()?;
        let laid = self.laid(window, cx);
        let (start, end) = (byte_of(&self.text, range.start), byte_of(&self.text, range.end));
        let row = laid.row_of(start);
        let origin = painted.bounds.origin - self.scroll;
        let left = laid.x_of(row, start);
        let right = laid.x_of(row, if laid.row_of(end) == row { end } else { laid.rows[row].end });
        Some(Bounds::new(
            point(origin.x + left, origin.y + laid.line_height * row as f32),
            size((right - left).max(px(1.0)), laid.line_height),
        ))
    }

    fn character_index_for_point(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<usize> {
        let offset = self.offset_at(at, window, cx)?;
        Some(utf16_len(&self.text[..offset]))
    }
}

impl Render for TextArea {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.blur.is_none() {
            let subscription = cx.on_blur(&self.focus, window, |area, _, cx| {
                area.drag_anchor = None;
                cx.emit(TextAreaEvent::Blurred);
                cx.notify();
            });
            self.blur = Some(subscription);
        }
        let root = div()
            .w_full()
            .key_context(TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(|area, _: &MouseUpEvent, _, _| area.drag_anchor = None))
            .child(TextAreaElement { area: cx.entity() });
        match self.sizing {
            Sizing::Fill => root.h_full(),
            Sizing::Rows { .. } => root,
        }
    }
}

/// Paints a [`TextArea`]'s text, selection, composition and caret.
struct TextAreaElement {
    area: Entity<TextArea>,
}

impl IntoElement for TextAreaElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct Prepared {
    laid: Laid,
    /// The placeholder, laid out, while the text is empty.
    placeholder: Option<Laid>,
}

impl Element for TextAreaElement {
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
        let Sizing::Rows { min, max } = self.area.read(cx).sizing else {
            style.size.height = relative(1.0).into();
            return (window.request_layout(style, [], cx), ());
        };
        // The height follows the rows the text wraps into at the width the layout gives.
        let area = self.area.clone();
        let layout = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let area = area.read(cx);
            let wrap = width.filter(|_| area.soft_wrap).map(|width| width - px(CARET_WIDTH));
            let color = theme::color(cx, "fg-strong");
            let mut rows = lay_out(&area.text, wrap, color, window, cx).rows.len();
            if area.text.is_empty() && !area.placeholder.is_empty() {
                rows = rows.max(lay_out(&area.placeholder, wrap, color, window, cx).rows.len());
            }
            size(width.unwrap_or(Pixels::ZERO), metrics(cx).1 * rows.clamp(min, max) as f32)
        });
        (layout, ())
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
        let area = self.area.read(cx);
        let wrap = area.soft_wrap.then(|| bounds.size.width - px(CARET_WIDTH));
        let (text_color, placeholder_color) =
            if area.disabled { ("fg-faint", "fg-faint") } else { ("fg-strong", "fg-muted") };
        let laid = lay_out(&area.text, wrap, theme::color(cx, text_color), window, cx);
        let placeholder = (area.text.is_empty() && !area.placeholder.is_empty())
            .then(|| lay_out(&area.placeholder, wrap, theme::color(cx, placeholder_color), window, cx));
        let content = size(laid.width() + px(CARET_WIDTH), laid.height());
        self.area.update(cx, |area, _| {
            area.painted = Some(Painted { bounds, wrap, content });
            area.keep_in_view(&laid, bounds.size);
        });
        Prepared { laid, placeholder }
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
        let (focus, selection, head, marked, scroll, editable, disabled) = {
            let area = self.area.read(cx);
            (
                area.focus.clone(),
                area.selection(),
                area.head,
                area.marked.clone(),
                area.scroll,
                area.editable(),
                area.disabled,
            )
        };
        if editable {
            window.handle_input(&focus, ElementInputHandler::new(bounds, self.area.clone()), cx);
        }
        let line_height = prepared.laid.line_height;
        let area = self.area.clone();
        window.on_mouse_event(move |event: &ScrollEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                let delta = event.delta.pixel_delta(line_height);
                // Wheel turns the area cannot follow scroll whatever is around it.
                if area.update(cx, |area, cx| area.scroll_by(delta, cx)) {
                    cx.stop_propagation();
                }
            }
        });
        let focused = focus.is_focused(window);
        let laid = &prepared.laid;
        let origin = bounds.origin - scroll;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            if !selection.is_empty() {
                let color = theme::color(cx, if focused { "bg-editor-selection" } else { "bg-selected" });
                for area in laid.areas(&selection, origin) {
                    window.paint_quad(fill(area, color));
                }
            }
            if let Some(marked) = marked.filter(|marked| !marked.is_empty()) {
                let color = theme::color(cx, "fg-strong");
                for area in laid.areas(&marked, origin) {
                    let start = point(area.left(), area.bottom() - px(2.0));
                    window.paint_quad(fill(Bounds::new(start, size(area.size.width, px(1.0))), color));
                }
            }
            let shown = prepared.placeholder.as_ref().unwrap_or(laid);
            for (index, line) in shown.lines.iter().enumerate() {
                let next_row = shown.lines.get(index + 1).map_or(shown.rows.len(), |next| next.first_row);
                let top = origin.y + shown.line_height * line.first_row as f32;
                if top > bounds.bottom() {
                    break;
                }
                if top + shown.line_height * ((next_row - line.first_row) as f32) < bounds.top() {
                    continue;
                }
                let _ = line.shaped.paint(point(origin.x, top), shown.line_height, TextAlign::Left, None, window, cx);
            }
            if focused && !disabled {
                let row = laid.row_of(head);
                let caret = Bounds::new(
                    point(origin.x + laid.x_of(row, head), origin.y + line_height * row as f32),
                    size(px(CARET_WIDTH), line_height),
                );
                window.paint_quad(fill(caret, theme::color(cx, "caret")));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    const WIDTH: f32 = 200.0;

    /// Gives the area a fixed width, as a dialog or a panel would.
    struct Host {
        area: Entity<TextArea>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(WIDTH)).child(self.area.clone())
        }
    }

    type Events = Rc<RefCell<Vec<TextAreaEvent>>>;

    fn area(cx: &mut TestAppContext) -> (Entity<TextArea>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (host, cx) =
            cx.add_window_view(|_, cx| Host { area: cx.new(|cx| TextArea::new(cx).with_placeholder("Message")) });
        let area = host.read_with(cx, |host, _| host.area.clone());
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&area, move |_, event: &TextAreaEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = area.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (area, events, cx)
    }

    fn text(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> String {
        area.read_with(cx, |area, _| area.text().to_owned())
    }

    fn head(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> usize {
        area.read_with(cx, |area, _| area.head)
    }

    fn selected(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> String {
        area.read_with(cx, |area, _| area.selected_text().to_owned())
    }

    fn set_text(area: &Entity<TextArea>, value: &str, cx: &mut VisualTestContext) {
        area.update(cx, |area, cx| area.set_text(value, cx));
        cx.run_until_parked();
    }

    /// The visual rows, as byte ranges.
    fn rows(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> Vec<Range<usize>> {
        area.update_in(cx, |area, window, cx| area.laid(window, cx).rows.iter().map(|row| row.start..row.end).collect())
    }

    fn caret_row(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> usize {
        area.update_in(cx, |area, window, cx| area.laid(window, cx).row_of(area.head))
    }

    fn line_height(cx: &mut VisualTestContext) -> Pixels {
        cx.update(|_, cx| metrics(cx).1)
    }

    /// The window position of offset `at`, in the middle of its row.
    fn point_of(area: &Entity<TextArea>, at: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
        area.update_in(cx, |area, window, cx| {
            let laid = area.laid(window, cx);
            let bounds = area.painted.as_ref().expect("the area is painted").bounds;
            let row = laid.row_of(at);
            bounds.origin - area.scroll + point(laid.x_of(row, at), laid.line_height * (row as f32 + 0.5))
        })
    }

    fn press(cx: &mut VisualTestContext, position: Point<Pixels>, clicks: usize, modifiers: Modifiers) {
        let button = MouseButton::Left;
        cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: clicks, first_mouse: false });
        cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: clicks });
    }

    /// The painted height and whether the caret's row is inside it.
    fn viewport(area: &Entity<TextArea>, cx: &mut VisualTestContext) -> (Pixels, bool) {
        let row = caret_row(area, cx);
        let line_height = line_height(cx);
        area.read_with(cx, |area, _| {
            let height = area.painted.as_ref().expect("the area is painted").bounds.size.height;
            let top = line_height * row as f32 - area.scroll.y;
            (height, top >= -px(0.5) && top + line_height <= height + px(0.5))
        })
    }

    #[gpui_kit::test]
    async fn enter_breaks_lines_and_the_arrows_move_through_them(cx: &mut TestAppContext) {
        let (area, events, cx) = area(cx);
        cx.simulate_input("one");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("two words");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("3");
        assert_eq!(text(&area, cx), "one\ntwo words\n3");
        assert!(events.borrow().iter().all(|event| *event == TextAreaEvent::Changed));
        // Up and Down keep the column they started in across shorter lines.
        cx.simulate_keystrokes("up");
        assert_eq!(head(&area, cx), 5);
        cx.simulate_keystrokes("up");
        assert_eq!(head(&area, cx), 1);
        cx.simulate_keystrokes("up");
        assert_eq!(head(&area, cx), 0, "Up on the first row goes to the start");
        cx.simulate_keystrokes("down");
        assert_eq!(head(&area, cx), 5);
        cx.simulate_keystrokes("down down");
        assert_eq!(head(&area, cx), 15, "Down on the last row goes to the end");
        // Home and End stay on the row; with ⌘ (Ctrl) they go to the ends of the text.
        cx.simulate_keystrokes("up end");
        assert_eq!(head(&area, cx), 13);
        cx.simulate_keystrokes("home");
        assert_eq!(head(&area, cx), 4);
        cx.simulate_keystrokes("cmd-end");
        assert_eq!(head(&area, cx), 15);
        cx.simulate_keystrokes("cmd-home");
        assert_eq!(head(&area, cx), 0);
        // Words move and delete as in a text field.
        cx.simulate_keystrokes("down alt-right alt-right");
        assert_eq!(head(&area, cx), 13);
        cx.simulate_keystrokes("alt-backspace");
        assert_eq!(text(&area, cx), "one\ntwo \n3");
        cx.simulate_keystrokes("backspace backspace");
        assert_eq!(text(&area, cx), "one\ntw\n3");
        cx.simulate_keystrokes("delete");
        assert_eq!(text(&area, cx), "one\ntw3");
        // ⌘Backspace deletes back to the start of the row.
        cx.simulate_keystrokes("cmd-backspace");
        assert_eq!(text(&area, cx), "one\n3");
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&TextAreaEvent::Cancelled));
    }

    #[gpui_kit::test]
    async fn shift_extends_the_selection_and_a_plain_arrow_collapses_it(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        set_text(&area, "one\ntwo words\n3", cx);
        cx.simulate_keystrokes("shift-up");
        assert_eq!(selected(&area, cx), "wo words\n3");
        cx.simulate_keystrokes("shift-alt-left");
        assert_eq!(selected(&area, cx), "two words\n3");
        cx.simulate_keystrokes("shift-cmd-home");
        assert_eq!(selected(&area, cx), "one\ntwo words\n3");
        cx.simulate_keystrokes("right");
        assert_eq!((head(&area, cx), selected(&area, cx)), (15, String::new()));
        cx.simulate_keystrokes("shift-home");
        assert_eq!(selected(&area, cx), "3");
    }

    #[gpui_kit::test]
    async fn the_mouse_places_the_caret_drags_and_selects_words_and_lines(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        set_text(&area, "one\ntwo words\n3", cx);
        let none = Modifiers::default();
        let shift = Modifiers { shift: true, ..Modifiers::default() };
        let at = point_of(&area, 4, cx);
        press(cx, at, 1, none);
        assert_eq!(head(&area, cx), 4);
        let at = point_of(&area, 9, cx);
        press(cx, at, 1, shift);
        assert_eq!(selected(&area, cx), "two w");
        let (from, to) = (point_of(&area, 1, cx), point_of(&area, 6, cx));
        cx.simulate_mouse_down(from, MouseButton::Left, none);
        cx.simulate_mouse_move(to, MouseButton::Left, none);
        cx.simulate_mouse_up(to, MouseButton::Left, none);
        assert_eq!(selected(&area, cx), "ne\ntw");
        let at = point_of(&area, 9, cx);
        press(cx, at, 2, none);
        assert_eq!(selected(&area, cx), "words");
        let at = point_of(&area, 9, cx);
        press(cx, at, 3, none);
        assert_eq!(selected(&area, cx), "two words\n");
        cx.simulate_input("x");
        assert_eq!(text(&area, cx), "one\nx3");
    }

    #[gpui_kit::test]
    async fn the_clipboard_cuts_copies_and_pastes_lines(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        set_text(&area, "alpha\nbeta", cx);
        cx.simulate_keystrokes("shift-up");
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("a\nbeta"));
        cx.simulate_keystrokes("cmd-x");
        assert_eq!(text(&area, cx), "alph");
        // Line breaks are pasted as they are, written the one way.
        cx.write_to_clipboard(ClipboardItem::new_string("one\r\ntwo\rthree".to_owned()));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(text(&area, cx), "alphone\ntwo\nthree");
        cx.simulate_keystrokes("cmd-a cmd-x");
        assert_eq!(text(&area, cx), "");
    }

    #[gpui_kit::test]
    async fn undo_takes_back_words_line_breaks_and_pastes_and_redo_makes_them_again(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        cx.simulate_input("hello world");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("again");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "hello world\n");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "hello world");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "hello");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "");
        cx.simulate_keystrokes("cmd-shift-z cmd-shift-z");
        assert_eq!(text(&area, cx), "hello world");
        // A run of deletions is one step; a new edit forgets what could be redone.
        cx.simulate_keystrokes("backspace backspace backspace");
        assert_eq!(text(&area, cx), "hello wo");
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "hello world");
        cx.simulate_input("!");
        cx.simulate_keystrokes("cmd-shift-z");
        assert_eq!(text(&area, cx), "hello world!");
    }

    #[gpui_kit::test]
    async fn a_long_line_wraps_into_rows_that_up_down_home_and_end_move_through(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        let long = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron";
        set_text(&area, long, cx);
        let wrapped = rows(&area, cx);
        assert!(wrapped.len() >= 3, "{wrapped:?}");
        assert_eq!((wrapped[0].start, wrapped[wrapped.len() - 1].end), (0, long.len()));
        let last = wrapped.len() - 1;
        assert_eq!(caret_row(&area, cx), last);
        cx.simulate_keystrokes("up");
        assert_eq!(caret_row(&area, cx), last - 1);
        cx.simulate_keystrokes("up");
        assert_eq!(caret_row(&area, cx), last - 2);
        cx.simulate_keystrokes("home");
        assert_eq!(head(&area, cx), wrapped[last - 2].start);
        cx.simulate_keystrokes("end");
        assert_eq!(caret_row(&area, cx), last - 2);
        assert!(head(&area, cx) < wrapped[last - 2].end && head(&area, cx) > wrapped[last - 2].start);
        cx.simulate_keystrokes("down");
        assert_eq!(caret_row(&area, cx), last - 1);
        // The area is as tall as its rows.
        let (height, _) = viewport(&area, cx);
        assert!((height - line_height(cx) * wrapped.len() as f32).abs() < px(1.0));
        // Without soft wrapping the line stays one row.
        area.update(cx, |area, cx| area.set_soft_wrap(false, cx));
        cx.run_until_parked();
        assert_eq!(rows(&area, cx).len(), 1);
    }

    #[gpui_kit::test]
    async fn the_area_grows_to_its_maximum_rows_then_scrolls_with_the_caret(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        area.update(cx, |area, cx| area.set_rows(1, 3, cx));
        cx.run_until_parked();
        let line_height = line_height(cx);
        let rows_shown = |cx: &mut VisualTestContext| viewport(&area, cx).0 / line_height;
        assert!((rows_shown(cx) - 1.0).abs() < 0.05);
        cx.simulate_input("a");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("b");
        assert!((rows_shown(cx) - 2.0).abs() < 0.05);
        for line in ["c", "d", "e"] {
            cx.simulate_keystrokes("enter");
            cx.simulate_input(line);
        }
        assert!((rows_shown(cx) - 3.0).abs() < 0.05);
        let scrolled = area.read_with(cx, |area, _| area.scroll.y);
        assert!(scrolled > Pixels::ZERO);
        assert!(viewport(&area, cx).1, "the caret on the last row is in view");
        // Page Up moves by the rows in view, and the caret stays in view.
        cx.simulate_keystrokes("pageup");
        assert_eq!(caret_row(&area, cx), 1);
        assert!(viewport(&area, cx).1);
        cx.simulate_keystrokes("pagedown");
        assert_eq!(caret_row(&area, cx), 4);
        assert!(viewport(&area, cx).1);
        cx.simulate_keystrokes("cmd-home");
        assert_eq!(area.read_with(cx, |area, _| area.scroll.y), Pixels::ZERO);
    }

    #[gpui_kit::test]
    async fn the_placeholder_shows_only_while_the_text_is_empty(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        // A placeholder longer than a row wraps, and the empty area is as tall as it.
        let hint = "Ask anything about the project, then press the key that sends the message";
        area.update(cx, |area, cx| area.set_placeholder(hint, cx));
        cx.run_until_parked();
        let line_height = line_height(cx);
        let rows_shown = |cx: &mut VisualTestContext| (viewport(&area, cx).0 / line_height).round() as usize;
        assert!(rows_shown(cx) >= 3);
        cx.simulate_input("x");
        assert_eq!(rows_shown(cx), 1, "typed text takes the placeholder's place");
        cx.simulate_keystrokes("backspace");
        assert!(rows_shown(cx) >= 3);
        area.update(cx, |area, cx| area.set_placeholder("", cx));
        cx.run_until_parked();
        assert_eq!(rows_shown(cx), 1);
    }

    #[gpui_kit::test]
    async fn a_disabled_area_takes_no_input_and_a_read_only_one_only_selects_and_copies(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        set_text(&area, "fixed\ntext", cx);
        area.update(cx, |area, cx| area.set_disabled(true, cx));
        cx.run_until_parked();
        cx.simulate_input("x");
        cx.simulate_keystrokes("backspace enter cmd-a");
        assert_eq!((text(&area, cx), selected(&area, cx)), ("fixed\ntext".to_owned(), String::new()));

        area.update(cx, |area, cx| {
            area.set_disabled(false, cx);
            area.set_read_only(true, cx);
        });
        cx.run_until_parked();
        cx.simulate_input("x");
        cx.simulate_keystrokes("backspace enter");
        cx.write_to_clipboard(ClipboardItem::new_string("pasted".to_owned()));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(text(&area, cx), "fixed\ntext");
        cx.simulate_keystrokes("shift-up cmd-x");
        assert_eq!(text(&area, cx), "fixed\ntext", "cutting a read-only area copies");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("d\ntext"));
    }

    #[gpui_kit::test]
    async fn losing_the_focus_is_reported(cx: &mut TestAppContext) {
        let (area, events, cx) = area(cx);
        cx.update(|window, cx| {
            window.activate_window();
            let focus = area.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        assert!(!events.borrow().contains(&TextAreaEvent::Blurred));
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(events.borrow().contains(&TextAreaEvent::Blurred));
    }

    #[gpui_kit::test]
    async fn an_input_method_composes_on_any_line_and_commits(cx: &mut TestAppContext) {
        let (area, _, cx) = area(cx);
        set_text(&area, "first\n", cx);
        area.update_in(cx, |area, window, cx| {
            area.replace_and_mark_text_in_range(None, "n", None, window, cx);
            area.replace_and_mark_text_in_range(None, "ni", None, window, cx);
        });
        cx.run_until_parked();
        assert_eq!(text(&area, cx), "first\nni");
        let (marked, bounds, line_height) = area.update_in(cx, |area, window, cx| {
            let marked = area.marked_text_range(window, cx);
            let element = area.painted.as_ref().expect("the area is painted").bounds;
            let bounds = area.bounds_for_range(6..8, element, window, cx).expect("the composition has bounds");
            (marked, bounds.top() - element.top(), metrics(cx).1)
        });
        assert_eq!(marked, Some(6..8));
        assert!((bounds - line_height).abs() < px(0.5), "the composition shows on the second row");
        area.update_in(cx, |area, window, cx| area.replace_text_in_range(None, "你", window, cx));
        cx.run_until_parked();
        assert_eq!(text(&area, cx), "first\n你");
        assert_eq!(area.update_in(cx, |area, window, cx| area.marked_text_range(window, cx)), None);
        // The composition and its commit are one undo step.
        cx.simulate_keystrokes("cmd-z");
        assert_eq!(text(&area, cx), "first\n");
    }

    #[gpui_kit::test]
    async fn an_owner_claiming_enter_leaves_shift_enter_to_break_the_line(cx: &mut TestAppContext) {
        let (area, events, cx) = area(cx);
        let claimed = Rc::new(Cell::new(0));
        let seen = claimed.clone();
        let submitter = area.clone();
        cx.update(|_, cx| {
            let focus = area.focus_handle(cx);
            input::claim_keys(
                focus,
                move |keystroke, _, cx| {
                    let mine = keystroke.key == "enter" && !keystroke.modifiers.modified();
                    if mine {
                        seen.set(seen.get() + 1);
                        submitter.update(cx, |area, cx| area.submit(cx));
                    }
                    mine
                },
                cx,
            );
        });
        cx.simulate_input("a");
        cx.simulate_keystrokes("enter");
        assert_eq!((text(&area, cx), claimed.get()), ("a".to_owned(), 1));
        assert_eq!(events.borrow().last(), Some(&TextAreaEvent::Submitted));
        cx.simulate_keystrokes("shift-enter");
        cx.simulate_input("b");
        assert_eq!((text(&area, cx), claimed.get()), ("a\nb".to_owned(), 1));
    }
}
