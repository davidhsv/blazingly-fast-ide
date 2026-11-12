//! A one-line text field: the query boxes of the Find panel and the search popups, the name
//! fields of dialogs. It edits its own text, with the caret, selection, clipboard and input
//! method support every text input has, and tells its owner when the text changes, when Enter
//! submits it and when Escape gives up.

use crate::theme;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, Element, ElementId, ElementInputHandler, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, GlobalElementId, Hsla, InspectorElementId, IntoElement, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, Pixels, Point, Render, ShapedLine, SharedString, Style, TextAlign, TextRun,
    UTF16Selection, Window, div, fill, font, point, prelude::*, px, relative, size,
};
use std::ops::Range;

/// The key context of text fields: plain typing stays in them instead of running shortcuts.
pub const TEXT_FIELD_CONTEXT: &str = "TextField";

/// What a text field tells its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// The text is different now.
    Changed,
    /// Enter was pressed.
    Submitted,
    /// Escape was pressed.
    Cancelled,
}

pub struct TextField {
    focus: FocusHandle,
    text: String,
    /// The selection's fixed end and the caret, as byte offsets on character boundaries.
    anchor: usize,
    head: usize,
    placeholder: SharedString,
    /// The text being composed by an input method.
    marked: Option<Range<usize>>,
    /// Where the text was last painted, for the input method's candidate window and for clicks.
    painted: Option<(Bounds<Pixels>, ShapedLine)>,
}

impl EventEmitter<TextFieldEvent> for TextField {}

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

impl TextField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            text: String::new(),
            anchor: 0,
            head: 0,
            placeholder: SharedString::default(),
            marked: None,
            painted: None,
        }
    }

    /// The text shown, muted, while the field is empty.
    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Replaces the text and puts the caret at its end. The owner is not told: it knows.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.text = text.to_owned();
        (self.anchor, self.head) = (self.text.len(), self.text.len());
        self.marked = None;
        cx.notify();
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        (self.anchor, self.head) = (0, self.text.len());
        cx.notify();
    }

    /// The selected bytes, in order.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Replaces `range` (the selection when none is given) with `new_text`.
    fn replace(&mut self, range: Option<Range<usize>>, new_text: &str, cx: &mut Context<Self>) {
        let range = range.unwrap_or_else(|| self.selection());
        // One line: what is pasted or composed with line breaks is joined by spaces.
        let single: String = new_text.split(['\r', '\n']).filter(|part| !part.is_empty()).collect::<Vec<_>>().join(" ");
        self.text.replace_range(range.clone(), &single);
        let caret = range.start + single.len();
        (self.anchor, self.head) = (caret, caret);
        cx.emit(TextFieldEvent::Changed);
        cx.notify();
    }

    fn boundary(&self, from: usize, forward: bool, word: bool) -> usize {
        let mut at = from;
        let step = |at: usize| -> Option<(usize, char)> {
            if forward {
                self.text[at..].chars().next().map(|c| (at + c.len_utf8(), c))
            } else {
                self.text[..at].chars().next_back().map(|c| (at - c.len_utf8(), c))
            }
        };
        let Some((next, first)) = step(at) else { return at };
        at = next;
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

    fn move_to(&mut self, to: usize, extend: bool, cx: &mut Context<Self>) {
        self.head = to;
        if !extend {
            self.anchor = to;
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let (modifiers, key) = (keystroke.modifiers, keystroke.key.as_str());
        let selection = self.selection();
        if modifiers.secondary(cx) && !modifiers.alt && !modifiers.shift {
            let handled = match key {
                "a" => {
                    self.select_all(cx);
                    true
                }
                "c" | "x" if !selection.is_empty() => {
                    cx.write_to_clipboard(ClipboardItem::new_string(self.selected_text().to_owned()));
                    if key == "x" {
                        self.replace(None, "", cx);
                    }
                    true
                }
                "v" => match cx.read_from_clipboard().and_then(|item| item.text()) {
                    Some(text) => {
                        self.replace(None, &text, cx);
                        true
                    }
                    None => false,
                },
                _ => false,
            };
            if handled {
                cx.stop_propagation();
                return;
            }
        }
        let (word, line) = (modifiers.alt, modifiers.platform);
        match key {
            "left" | "right" => {
                let forward = key == "right";
                let to = if line {
                    if forward { self.text.len() } else { 0 }
                } else if !modifiers.shift && !selection.is_empty() {
                    // A plain arrow collapses the selection to the side it points at.
                    if forward { selection.end } else { selection.start }
                } else {
                    self.boundary(self.head, forward, word)
                };
                self.move_to(to, modifiers.shift, cx);
            }
            "home" => self.move_to(0, modifiers.shift, cx),
            "end" => self.move_to(self.text.len(), modifiers.shift, cx),
            "backspace" | "delete" => {
                let forward = key == "delete";
                let range = if !selection.is_empty() {
                    selection
                } else if line && !forward {
                    0..self.head
                } else {
                    let to = self.boundary(self.head, forward, word);
                    to.min(self.head)..to.max(self.head)
                };
                if !range.is_empty() {
                    self.replace(Some(range), "", cx);
                }
            }
            "enter" if !modifiers.modified() => cx.emit(TextFieldEvent::Submitted),
            "escape" if !modifiers.modified() => cx.emit(TextFieldEvent::Cancelled),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if event.click_count >= 2 {
            self.select_all(cx);
            return;
        }
        let Some((bounds, line)) = &self.painted else { return };
        let at = line.closest_index_for_x(event.position.x - bounds.left()).min(self.text.len());
        self.move_to(at, event.modifiers.shift, cx);
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for TextField {
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
        // Committed text replaces the composition, else the given range, else the selection.
        let range =
            range.map(|r| byte_of(&self.text, r.start)..byte_of(&self.text, r.end)).or_else(|| self.marked.take());
        self.marked = None;
        self.replace(range, text, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range
            .map(|r| byte_of(&self.text, r.start)..byte_of(&self.text, r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let start = range.start;
        self.replace(Some(range), new_text, cx);
        self.marked = (self.head > start).then_some(start..self.head);
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let (bounds, line) = self.painted.as_ref()?;
        let x = |at: usize| bounds.left() + line.x_for_index(byte_of(&self.text, at));
        Some(Bounds::new(
            point(x(range.start), bounds.top()),
            size((x(range.end) - x(range.start)).max(px(1.0)), bounds.size.height),
        ))
    }

    fn character_index_for_point(&mut self, at: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        let (bounds, line) = self.painted.as_ref()?;
        Some(utf16_len(&self.text[..line.closest_index_for_x(at.x - bounds.left()).min(self.text.len())]))
    }
}

impl Render for TextField {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w_full()
            .h(theme::length(cx, "h-row"))
            .flex()
            .items_center()
            .key_context(TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(TextFieldElement { field: cx.entity() })
    }
}

/// Paints a [`TextField`]'s text, selection and caret.
struct TextFieldElement {
    field: Entity<TextField>,
}

impl IntoElement for TextFieldElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextFieldElement {
    type RequestLayoutState = ();
    type PrepaintState = ShapedLine;

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
        style.size.height = (theme::length(cx, "fs-ui") * 1.5).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> ShapedLine {
        let field = self.field.read(cx);
        let (text, color) = if field.text.is_empty() {
            (field.placeholder.clone(), theme::color(cx, "fg-muted"))
        } else {
            (SharedString::from(field.text.clone()), theme::color(cx, "fg-strong"))
        };
        let family = cx
            .try_global::<theme::ActiveTheme>()
            .and_then(|active| active.theme.font_families("font-ui").into_iter().next())
            .unwrap_or_else(|| ".SystemUIFont".to_owned());
        let run = TextRun {
            len: text.len(),
            font: font(SharedString::from(family)),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        window.text_system().shape_line(text, theme::length(cx, "fs-ui"), &[run], None)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        line: &mut ShapedLine,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus, selection, head, empty) = {
            let field = self.field.read(cx);
            (field.focus.clone(), field.selection(), field.head, field.text.is_empty())
        };
        window.handle_input(&focus, ElementInputHandler::new(bounds, self.field.clone()), cx);
        let focused = focus.is_focused(window);
        let x = |at: usize| bounds.left() + if empty { px(0.0) } else { line.x_for_index(at) };
        if !selection.is_empty() {
            let area = Bounds::new(
                point(x(selection.start), bounds.top()),
                size(x(selection.end) - x(selection.start), bounds.size.height),
            );
            let color: Hsla = theme::color(cx, if focused { "bg-editor-selection" } else { "bg-selected" });
            window.paint_quad(fill(area, color));
        }
        let _ = line.paint(bounds.origin, bounds.size.height, TextAlign::Left, None, window, cx);
        if focused {
            let caret = Bounds::new(point(x(head), bounds.top()), size(px(1.5), bounds.size.height));
            window.paint_quad(fill(caret, theme::color(cx, "caret")));
        }
        let painted = (bounds, line.clone());
        self.field.update(cx, |field, _| field.painted = (!empty).then_some(painted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    fn field(cx: &mut TestAppContext) -> (Entity<TextField>, Rc<RefCell<Vec<TextFieldEvent>>>, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            crate::platform::input::init(cx);
        });
        let (field, cx) = cx.add_window_view(|_, cx| TextField::new(cx).with_placeholder("Search"));
        let events: Rc<RefCell<Vec<TextFieldEvent>>> = Rc::default();
        let log = events.clone();
        cx.update(|window, cx| {
            cx.subscribe(&field, move |_, event: &TextFieldEvent, _| log.borrow_mut().push(event.clone())).detach();
            let focus = field.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        (field, events, cx)
    }

    fn text(field: &Entity<TextField>, cx: &mut VisualTestContext) -> String {
        field.read_with(cx, |field, _| field.text().to_owned())
    }

    #[gpui_kit::test]
    async fn typing_edits_the_text_and_enter_and_escape_are_reported(cx: &mut TestAppContext) {
        let (field, events, cx) = field(cx);
        cx.simulate_input("hello wörld");
        assert_eq!(text(&field, cx), "hello wörld");
        // Left moves by character, Option+Left by word; typing goes in at the caret.
        cx.simulate_keystrokes("left alt-left");
        cx.simulate_input("big ");
        assert_eq!(text(&field, cx), "hello big wörld");
        cx.simulate_keystrokes("backspace alt-backspace");
        assert_eq!(text(&field, cx), "hello wörld");
        cx.simulate_keystrokes("end delete home delete");
        assert_eq!(text(&field, cx), "ello wörld");
        // ⌘Backspace deletes back to the start of the line.
        cx.simulate_keystrokes("cmd-right cmd-backspace");
        assert_eq!(text(&field, cx), "");
        cx.simulate_input("x");
        cx.simulate_keystrokes("enter escape");
        let seen = events.borrow();
        assert_eq!(seen.last(), Some(&TextFieldEvent::Cancelled));
        assert!(seen.contains(&TextFieldEvent::Submitted) && seen.contains(&TextFieldEvent::Changed));
    }

    #[gpui_kit::test]
    async fn selections_are_replaced_copied_cut_and_pasted(cx: &mut TestAppContext) {
        let (field, _, cx) = field(cx);
        cx.simulate_input("alpha beta");
        cx.simulate_keystrokes("shift-alt-left");
        assert_eq!(field.read_with(cx, |field, _| field.selected_text().to_owned()), "beta");
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("beta"));
        cx.simulate_input("gamma");
        assert_eq!(text(&field, cx), "alpha gamma");
        cx.simulate_keystrokes("cmd-a cmd-x");
        assert_eq!(text(&field, cx), "");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("alpha gamma"));
        // What is pasted stays on one line.
        cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo\r\nthree".to_owned()));
        cx.simulate_keystrokes("cmd-v");
        assert_eq!(text(&field, cx), "one two three");
        // A plain arrow collapses a selection to its side instead of moving past it.
        cx.simulate_keystrokes("cmd-a left");
        cx.simulate_input(">");
        assert_eq!(text(&field, cx), ">one two three");
        field.update(cx, |field, cx| field.set_text("reset", cx));
        cx.simulate_input("!");
        assert_eq!(text(&field, cx), "reset!");
    }
}
