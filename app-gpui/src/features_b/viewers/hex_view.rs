//! The read-only hex view of binary and very large files: a toolbar (the lock, the kind of file,
//! its size, the selected byte, Open as Text for a large text file), a header naming the columns
//! and the rows of the file, each its offset, its sixteen bytes in hex (two groups of eight) and
//! their printable ASCII.
//!
//! Only the rows on screen are drawn. The view keeps its own scroll offset over a range capped at
//! [`MAX_SCROLL_PX`] and scaled onto all rows ([`hex_viewport`]), so a file of hundreds of
//! megabytes still scrolls; it asks for the 64 KB pages of the rows coming into view as
//! [`HexViewEvent::ReadPage`] and draws `··` until a page arrives.
//!
//! [`HexView`] owns no file access: the owner answers each page with [`HexView::page_loaded`].

use super::file_kinds::{BINARY_KIND, format_size};
use super::hex::{
    BYTES_PER_ROW, MAX_SCROLL_PX, ascii_char, decode_base64, format_offset, hex_byte, hex_viewport, offset_digits,
    pages_for_rows, row_bytes, row_count, scroll_top_for_row,
};
use crate::editor::tabs::EditorInput;
use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    App, Context, EventEmitter, FocusHandle, Focusable, Font, Hsla, InteractiveText, IntoElement, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render, ScrollEvent, SharedString, StyledText, TextRun,
    Window, canvas, div, font, prelude::*, px,
};
use ide_vfs::ByteRange;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;

/// Height of a row and of the header.
const ROW_H: f64 = 20.0;
/// Width of the scroll bar's track.
const SCROLLBAR_W: f32 = 8.0;
/// The shortest scroll bar thumb.
const MIN_THUMB: f64 = 20.0;

/// What the hex view asks of its owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HexViewEvent {
    /// Read page `page` ([`super::hex::PAGE_SIZE`] bytes from `page * PAGE_SIZE`) and answer it
    /// with [`HexView::page_loaded`].
    ReadPage(u64),
    /// Open the file in a text editor, past the viewers (`openFile(path, { asText: true })`).
    OpenAsText(String),
}

/// What a part of a drawn row is, for its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Offset,
    Byte { zero: bool, selected: bool },
    Pending,
    Ascii { selected: bool },
    Plain,
}

/// A row as text: its styled parts (byte ranges of `text`) and what a click on each byte or
/// character selects.
struct RowLine {
    text: String,
    parts: Vec<(Range<usize>, Part)>,
    clickable: Vec<(Range<usize>, u64)>,
}

impl RowLine {
    fn push(&mut self, text: &str, part: Part) -> Range<usize> {
        let start = self.text.len();
        self.text.push_str(text);
        self.parts.push((start..self.text.len(), part));
        start..self.text.len()
    }
}

pub struct HexView {
    path: String,
    /// `false` for a large text file (shown here because of its size): it may still open as text.
    binary: bool,
    total: u64,
    pages: HashMap<u64, Vec<u8>>,
    /// Pages asked for or loaded.
    requested: HashSet<u64>,
    error: Option<String>,
    scroll_top: f64,
    view_height: f64,
    /// The height measured by the last frame, shared with its prepaint.
    measured: Rc<Cell<f64>>,
    selected: Option<u64>,
    /// [`HexView::load`] ran: pages are asked for from then on.
    started: bool,
    /// A drag of the scroll bar thumb: the pointer's start and the scroll offset then.
    thumb_drag: Option<(f64, f64)>,
    focus: FocusHandle,
    key_claim: u64,
}

impl EventEmitter<HexViewEvent> for HexView {}

impl HexView {
    /// The view of `path`, `size` bytes as probed when the tab opened, `binary` as the probe saw it.
    pub fn new(path: String, size: u64, binary: bool, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let weak = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| weak.update(cx, |view, cx| view.key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        Self {
            path,
            binary,
            total: size,
            pages: HashMap::new(),
            requested: HashSet::new(),
            error: None,
            scroll_top: 0.0,
            view_height: 0.0,
            measured: Rc::default(),
            selected: None,
            started: false,
            thumb_drag: None,
            focus,
            key_claim,
        }
    }

    /// The view of a tab's `binary` input ([`super::file_kinds::binary_input`]); `None` for
    /// another kind of tab.
    pub fn from_input(input: &EditorInput, cx: &mut Context<Self>) -> Option<Self> {
        if input.kind != BINARY_KIND {
            return None;
        }
        let path = input.path.clone()?;
        let data = input.data.as_ref();
        let size = data.and_then(|data| data.get("size")).and_then(serde_json::Value::as_u64).unwrap_or(0);
        let binary = data.and_then(|data| data.get("binary")).and_then(serde_json::Value::as_bool).unwrap_or(true);
        Some(Self::new(path, size, binary, cx))
    }

    /// Starts reading: the first page always (the size known when the tab opened may be stale, and
    /// the first page reports the current one), then the pages on screen.
    pub fn load(&mut self, cx: &mut Context<Self>) {
        self.started = true;
        self.request(0, cx);
        self.ensure_pages(cx);
    }

    /// The answer to [`HexViewEvent::ReadPage`]: the bytes and the file's current size, or why
    /// the read failed (the page is asked for again when it comes back into view).
    pub fn page_loaded(&mut self, page: u64, answer: Result<ByteRange, String>, cx: &mut Context<Self>) {
        match answer.and_then(|range| {
            decode_base64(&range.data).map(|bytes| (bytes, range.total)).ok_or_else(|| "invalid data".to_owned())
        }) {
            Ok((bytes, total)) => {
                self.pages.insert(page, bytes);
                self.total = total;
                self.scroll_top = self.scroll_top.min(self.max_scroll());
                self.ensure_pages(cx);
            }
            Err(error) => {
                self.requested.remove(&page);
                self.error = Some(error);
            }
        }
        cx.notify();
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn total(&self) -> u64 {
        self.total
    }

    /// The selected byte's offset.
    pub fn selected(&self) -> Option<u64> {
        self.selected
    }

    /// The first row on screen.
    pub fn first_row(&self) -> u64 {
        self.viewport().first
    }

    /// The rows on screen as text: offset, hex bytes and ASCII.
    pub fn rows(&self) -> Vec<String> {
        self.visible_rows().map(|row| self.row_line(row).text.trim_end().to_owned()).collect()
    }

    /// The toolbar's description of the file: `Binary file · 1.5 KB · read-only`.
    pub fn info(&self, cx: &App) -> String {
        let tx = |text: &str| cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |l| l.0.tx(text));
        let kind = if self.binary { tx("Binary file") } else { tx("Large file") };
        format!("{kind} · {} · {}", format_size(self.total), tx("read-only"))
    }

    /// The toolbar's description of the selected byte: `Offset 0x11 (17) · 0x41 = 65`.
    pub fn selection_label(&self, cx: &mut App) -> Option<String> {
        let selected = self.selected?;
        let (hex, decimal) = (format_offset(selected, 1), selected.to_string());
        let params = [Param::from(hex.as_str()), Param::from(decimal.as_str())];
        let offset = cx.default_global::<Localization>().0.tx_built(&["Offset 0x", " (", ")"], &params);
        let byte = row_bytes(&self.pages, selected / BYTES_PER_ROW, self.total)
            .and_then(|bytes| bytes.get((selected % BYTES_PER_ROW) as usize).copied());
        Some(match byte {
            Some(byte) => format!("{offset} · 0x{} = {byte}", hex_byte(byte)),
            None => offset,
        })
    }

    /// The message shown instead of the rows when a read failed.
    pub fn error_message(&self, cx: &mut App) -> Option<String> {
        let error = self.error.as_deref()?;
        Some(cx.default_global::<Localization>().0.tx_built(&["Cannot read the file: ", ""], &[Param::from(error)]))
    }

    /// Whether Open as Text shows: for large text files only.
    pub fn offers_text(&self) -> bool {
        !self.binary
    }

    // ---------------------------------------------------------------------------------------------
    // Rows and pages

    fn rows_total(&self) -> u64 {
        row_count(self.total)
    }

    fn viewport(&self) -> super::hex::HexViewport {
        hex_viewport(self.scroll_top, self.view_height, self.rows_total(), ROW_H, MAX_SCROLL_PX)
    }

    /// The last row (partly) on screen.
    fn last_row(&self) -> u64 {
        let visible = (self.view_height / ROW_H).ceil() as u64 + 1;
        (self.viewport().first + visible).min(self.rows_total().saturating_sub(1))
    }

    fn visible_rows(&self) -> Range<u64> {
        if self.rows_total() == 0 {
            return 0..0;
        }
        self.viewport().first..self.last_row() + 1
    }

    fn max_scroll(&self) -> f64 {
        (self.viewport().spacer.max(self.view_height) - self.view_height).max(0.0)
    }

    fn request(&mut self, page: u64, cx: &mut Context<Self>) {
        if self.requested.insert(page) {
            cx.emit(HexViewEvent::ReadPage(page));
        }
    }

    /// Asks for the pages of the rows on screen that are neither loaded nor asked for.
    fn ensure_pages(&mut self, cx: &mut Context<Self>) {
        if !self.started || self.rows_total() == 0 {
            return;
        }
        for page in pages_for_rows(self.viewport().first, self.last_row()) {
            self.request(page, cx);
        }
    }

    fn scroll_to(&mut self, top: f64, cx: &mut Context<Self>) {
        let top = top.clamp(0.0, self.max_scroll());
        if top != self.scroll_top {
            self.scroll_top = top;
            self.ensure_pages(cx);
            cx.notify();
        }
    }

    /// The scroll area's height changed (measured after a frame).
    fn resized(&mut self, height: f64, cx: &mut Context<Self>) {
        self.view_height = height;
        self.scroll_top = self.scroll_top.min(self.max_scroll());
        self.ensure_pages(cx);
        cx.notify();
    }

    fn row_line(&self, row: u64) -> RowLine {
        let mut line = RowLine { text: String::new(), parts: Vec::new(), clickable: Vec::new() };
        let base = row * BYTES_PER_ROW;
        let bytes = row_bytes(&self.pages, row, self.total);
        line.push(&format_offset(base, offset_digits(self.total)), Part::Offset);
        line.push("  ", Part::Plain);
        for i in 0..BYTES_PER_ROW {
            let offset = base + i;
            match bytes.map(|bytes| bytes.get(i as usize).copied()) {
                None => {
                    line.push("··", Part::Pending);
                }
                Some(None) => {
                    line.push("  ", Part::Plain);
                }
                Some(Some(byte)) => {
                    let selected = self.selected == Some(offset);
                    let range = line.push(&hex_byte(byte), Part::Byte { zero: byte == 0, selected });
                    line.clickable.push((range, offset));
                }
            }
            line.push(if i == 7 { "  " } else { " " }, Part::Plain);
        }
        line.push(" ", Part::Plain);
        for (i, byte) in bytes.unwrap_or_default().iter().enumerate() {
            let offset = base + i as u64;
            let range =
                line.push(&ascii_char(*byte).to_string(), Part::Ascii { selected: self.selected == Some(offset) });
            line.clickable.push((range, offset));
        }
        line
    }

    // ---------------------------------------------------------------------------------------------
    // Selection

    fn select(&mut self, offset: u64, cx: &mut Context<Self>) {
        self.selected = Some(offset);
        cx.notify();
    }

    /// Scrolls the least to show the row of `offset`.
    fn reveal(&mut self, offset: u64, cx: &mut Context<Self>) {
        let row = offset / BYTES_PER_ROW;
        let viewport = self.viewport();
        let full_rows = ((self.view_height / ROW_H).floor() as u64).saturating_sub(1).max(1);
        let rows = self.rows_total();
        let top = if row < viewport.first || (row == viewport.first && viewport.shift > 0.0) {
            Some(scroll_top_for_row(row, self.view_height, rows, ROW_H, MAX_SCROLL_PX))
        } else if row > viewport.first + full_rows {
            Some(scroll_top_for_row(row - full_rows, self.view_height, rows, ROW_H, MAX_SCROLL_PX))
        } else {
            None
        };
        if let Some(top) = top {
            self.scroll_to(top, cx);
        }
    }

    /// Arrows, Page Up / Down and Home / End move the selected byte (⌘/Ctrl+Home / End to the
    /// first and last byte of the file).
    fn key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if self.total == 0 {
            return false;
        }
        let modifiers = &keystroke.modifiers;
        let to_file_edge = modifiers.platform || modifiers.control;
        let page = ((self.view_height / ROW_H).floor() as i64 - 1).max(1) * BYTES_PER_ROW as i64;
        let current = self.selected.unwrap_or(0) as i64;
        let row_start = current - current % BYTES_PER_ROW as i64;
        let last = self.total as i64 - 1;
        let next = match keystroke.key.as_str() {
            "left" => current - 1,
            "right" => current + 1,
            "up" => current - BYTES_PER_ROW as i64,
            "down" => current + BYTES_PER_ROW as i64,
            "pageup" => current - page,
            "pagedown" => current + page,
            "home" if to_file_edge => 0,
            "home" => row_start,
            "end" if to_file_edge => last,
            "end" => last.min(row_start + BYTES_PER_ROW as i64 - 1),
            _ => return false,
        };
        let next = next.clamp(0, last) as u64;
        self.select(next, cx);
        self.reveal(next, cx);
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn mono_font(cx: &App) -> Font {
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-mono").into_iter().next())
            .unwrap_or_else(|| theme::fonts::BUNDLED_MONO_FAMILY.to_owned());
        font(SharedString::from(family))
    }

    fn part_style(part: Part, cx: &App) -> (Hsla, Option<Hsla>) {
        let selection = || Some(theme::color(cx, "bg-editor-selection"));
        match part {
            Part::Offset => (theme::color(cx, "fg-gutter"), None),
            Part::Byte { selected: true, .. } | Part::Ascii { selected: true } => {
                (theme::color(cx, "fg-strong"), selection())
            }
            Part::Byte { zero: true, .. } | Part::Pending => (theme::color(cx, "fg-faint"), None),
            Part::Byte { .. } | Part::Plain => (theme::color(cx, "fg"), None),
            Part::Ascii { .. } => (theme::color(cx, "fg-secondary"), None),
        }
    }

    fn render_row(&self, row: u64, top: f64, cx: &mut Context<Self>) -> impl IntoElement {
        let line = self.row_line(row);
        let mono = Self::mono_font(cx);
        let runs: Vec<TextRun> = line
            .parts
            .iter()
            .map(|(range, part)| {
                let (color, background_color) = Self::part_style(*part, cx);
                TextRun {
                    len: range.len(),
                    font: mono.clone(),
                    color,
                    background_color,
                    underline: None,
                    strikethrough: None,
                }
            })
            .collect();
        let (ranges, offsets): (Vec<Range<usize>>, Vec<u64>) = line.clickable.into_iter().unzip();
        let weak = cx.weak_entity();
        let text = InteractiveText::new(
            SharedString::from(format!("hex-row-{row}")),
            StyledText::new(line.text).with_runs(runs),
        )
        .on_click(ranges, move |index, _, cx| {
            if let Some(offset) = offsets.get(index).copied() {
                let _ = weak.update(cx, |view, cx| view.select(offset, cx));
            }
        });
        div()
            .debug_selector(move || format!("hex-row-{row}"))
            .absolute()
            .left_0()
            .top(px(top as f32))
            .h(px(ROW_H as f32))
            .px(px(12.0))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .child(text)
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let digits = offset_digits(self.total);
        let columns: String =
            (0..BYTES_PER_ROW as u8).map(|i| format!("{}{}", hex_byte(i), if i == 7 { "  " } else { " " })).collect();
        let decoded = cx.default_global::<Localization>().0.tx("Decoded text");
        div()
            .flex_none()
            .h(px(ROW_H as f32))
            .px(px(12.0))
            .flex()
            .items_center()
            .whitespace_nowrap()
            .font_family(Self::mono_font(cx).family)
            .text_size(theme::length(cx, "fs-code"))
            .text_color(theme::color(cx, "fg-faint"))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(SharedString::from(format!("{}  {columns} {decoded}", " ".repeat(digits))))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = theme::color(cx, "fg-secondary");
        let info = self.info(cx);
        let selection = self.selection_label(cx);
        let open_as_text = cx.default_global::<Localization>().0.tx("Open as Text");
        let path = self.path.clone();
        div()
            .flex_none()
            .h(px(36.0))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-editor-tabs"))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(muted)
            .child(theme::icon(cx, "lock", 14.0, muted))
            .child(div().px(px(6.0)).whitespace_nowrap().child(SharedString::from(info)))
            .child(div().flex_1())
            .children(selection.map(|label| {
                div()
                    .debug_selector(|| "hex-selection".to_owned())
                    .px(px(6.0))
                    .whitespace_nowrap()
                    .child(SharedString::from(label))
            }))
            .when(!self.binary, |el| {
                el.child(
                    div()
                        .id("hex-open-as-text")
                        .debug_selector(|| "hex-open-as-text".to_owned())
                        .h(px(24.0))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(theme::length(cx, "radius-toolbar-button"))
                        .text_color(theme::color(cx, "link"))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .on_click(cx.listener(move |_, _, _, cx| cx.emit(HexViewEvent::OpenAsText(path.clone()))))
                        .child(SharedString::from(open_as_text)),
                )
            })
    }

    fn render_scrollbar(&self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let spacer = self.viewport().spacer;
        let height = self.view_height;
        if spacer <= height || height <= 0.0 {
            return None;
        }
        let thumb = (height * height / spacer).max(MIN_THUMB).min(height);
        let range = self.max_scroll();
        let top = if range > 0.0 { self.scroll_top / range * (height - thumb) } else { 0.0 };
        Some(
            div().absolute().top_0().right_0().w(px(SCROLLBAR_W)).h_full().child(
                div()
                    .id("hex-scroll-thumb")
                    .debug_selector(|| "hex-scroll-thumb".to_owned())
                    .absolute()
                    .left(px(1.0))
                    .top(px(top as f32))
                    .w(px(SCROLLBAR_W - 2.0))
                    .h(px(thumb as f32))
                    .rounded(px(3.0))
                    .bg(theme::color(cx, "fg-faint"))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            view.thumb_drag = Some((f64::from(f32::from(event.position.y)), view.scroll_top));
                        }),
                    ),
            ),
        )
    }

    /// The scroll bar thumb was dragged by `dy` px from where the drag started at `start_top`.
    fn drag_thumb(&mut self, dy: f64, start_top: f64, cx: &mut Context<Self>) {
        let spacer = self.viewport().spacer;
        let height = self.view_height;
        let thumb = (height * height / spacer).max(MIN_THUMB).min(height);
        let track = height - thumb;
        if track > 0.0 {
            self.scroll_to(start_top + dy / track * self.max_scroll(), cx);
        }
    }
}

impl Focusable for HexView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HexView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(theme::color(cx, "bg-editor"))
            .text_color(theme::color(cx, "fg"))
            .child(self.render_toolbar(cx));
        if let Some(message) = self.error_message(cx) {
            return root.child(
                div()
                    .debug_selector(|| "hex-error".to_owned())
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(message)),
            );
        }
        let viewport = self.viewport();
        let rows: Vec<_> = self
            .visible_rows()
            .map(|row| {
                let top = (row - viewport.first) as f64 * ROW_H - viewport.shift;
                self.render_row(row, top, cx)
            })
            .collect();
        let measured = Rc::clone(&self.measured);
        let weak = cx.weak_entity();
        let measure = canvas(
            move |bounds, _, cx| {
                let height = f64::from(f32::from(bounds.size.height));
                if (measured.get() - height).abs() > 0.5 {
                    measured.set(height);
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |view, cx| view.resized(height, cx));
                    });
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let scrollbar = self.render_scrollbar(cx);
        root.child(self.render_header(cx)).child(
            div()
                .id("hex-rows")
                .debug_selector(|| "hex-rows".to_owned())
                .flex_1()
                .min_h_0()
                .relative()
                .overflow_hidden()
                .font_family(Self::mono_font(cx).family)
                .text_size(theme::length(cx, "fs-code"))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, _: &MouseDownEvent, window, cx| window.focus(&view.focus, cx)),
                )
                .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                    let Some((start_y, start_top)) = view.thumb_drag else { return };
                    if event.pressed_button != Some(MouseButton::Left) {
                        view.thumb_drag = None;
                        return;
                    }
                    view.drag_thumb(f64::from(f32::from(event.position.y)) - start_y, start_top, cx);
                }))
                .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, _| view.thumb_drag = None))
                .on_scroll(cx.listener(|view, event: &ScrollEvent, _, cx| {
                    let delta = f64::from(f32::from(event.delta.pixel_delta(px(ROW_H as f32)).y));
                    view.scroll_to(view.scroll_top - delta, cx);
                }))
                .child(measure)
                .children(rows)
                .children(scrollbar),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features_b::viewers::hex::PAGE_SIZE;
    use gpui_kit::{Entity, Modifiers, ScrollDelta, TestAppContext, VisualTestContext, point};
    use std::cell::RefCell;

    type Events = Rc<RefCell<Vec<HexViewEvent>>>;

    fn base64(bytes: &[u8]) -> String {
        const DIGITS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..=chunk.len() {
                out.push(char::from(DIGITS[((n >> (18 - 6 * i)) & 63) as usize]));
            }
        }
        out
    }

    fn range(offset: u64, bytes: &[u8], total: u64) -> ByteRange {
        ByteRange { offset, data: base64(bytes), total }
    }

    fn open(cx: &mut TestAppContext, size: u64, binary: bool) -> (Entity<HexView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| HexView::new("/p/data.bin".to_owned(), size, binary, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &HexViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.load(cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn focus(view: &Entity<HexView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.activate_window();
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    async fn reads_the_first_page_and_shows_offsets_hex_bytes_and_ascii(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, 20, true);
        assert_eq!(events.borrow().first(), Some(&HexViewEvent::ReadPage(0)));
        assert!(view.read_with(cx, |view, _| view.rows())[0].contains("··"));
        let mut bytes: Vec<u8> = b"Hello, hex!".to_vec();
        bytes.extend([0, 1, 2, 255, 0x41, 0x42, 0x43, 0x0a, 0x7f]);
        view.update(cx, |view, cx| view.page_loaded(0, Ok(range(0, &bytes, 20)), cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| view.rows()),
            [
                "00000000  48 65 6C 6C 6F 2C 20 68  65 78 21 00 01 02 FF 41  Hello, hex!....A",
                format!("{:<60}BC..", "00000010  42 43 0A 7F").as_str(),
            ]
        );
        assert_eq!(view.read_with(cx, |view, cx| view.info(cx)), "Binary file · 20 B · read-only");
        assert!(cx.debug_bounds("hex-open-as-text").is_none());
    }

    #[gpui_kit::test]
    async fn moves_the_selected_byte_with_the_keyboard(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, 40, true);
        let bytes: Vec<u8> = (0..40).collect();
        view.update(cx, |view, cx| view.page_loaded(0, Ok(range(0, &bytes, 40)), cx));
        focus(&view, cx);
        cx.simulate_keystrokes("right down");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(17));
        assert_eq!(
            view.update(cx, |view, cx| view.selection_label(cx)).as_deref(),
            Some("Offset 0x11 (17) · 0x11 = 17")
        );
        cx.simulate_keystrokes("end");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(31));
        cx.simulate_keystrokes("ctrl-end");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(39));
        cx.simulate_keystrokes("home left");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(31));
        cx.simulate_keystrokes("ctrl-home up");
        assert_eq!(view.read_with(cx, |view, _| view.selected()), Some(0));
    }

    #[gpui_kit::test]
    async fn reads_the_pages_scrolled_into_view_of_a_large_file(cx: &mut TestAppContext) {
        let total = 10 * 1024 * 1024;
        let (view, events, cx) = open(cx, total, false);
        let first: Vec<u8> = vec![7; PAGE_SIZE as usize];
        view.update(cx, |view, cx| view.page_loaded(0, Ok(range(0, &first, total)), cx));
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.info(cx)), "Large file · 10.0 MB · read-only");
        let rows = cx.debug_bounds("hex-rows").expect("the rows are drawn");
        cx.simulate_event(ScrollEvent {
            position: rows.center(),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-400_000.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        let first_row = view.read_with(cx, |view, _| view.first_row());
        assert!(first_row > 0);
        let wanted = pages_for_rows(first_row, first_row)[0];
        assert!(wanted > 0);
        assert!(events.borrow().contains(&HexViewEvent::ReadPage(wanted)));
        assert!(view.read_with(cx, |view, _| view.rows())[0].contains("··"));

        let bounds = cx.debug_bounds("hex-open-as-text").expect("a large text file offers its text");
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&HexViewEvent::OpenAsText("/p/data.bin".to_owned())));
    }

    #[gpui_kit::test]
    async fn says_why_the_file_cannot_be_read(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, 20, true);
        view.update(cx, |view, cx| view.page_loaded(0, Err("permission denied".to_owned()), cx));
        cx.run_until_parked();
        assert_eq!(
            view.update(cx, |view, cx| view.error_message(cx)).as_deref(),
            Some("Cannot read the file: permission denied")
        );
        assert!(cx.debug_bounds("hex-error").is_some());
        // The failed page is asked for again.
        view.update(cx, |view, cx| view.load(cx));
        assert_eq!(events.borrow().iter().filter(|event| **event == HexViewEvent::ReadPage(0)).count(), 2);
    }
}
