//! A one-line field for a secret (a password, an API key): it edits like a [`TextField`], with the
//! caret, the selection, pasting and input method support, but paints one bullet per character.
//! The text is never painted, never put on the clipboard (copy and cut do nothing) and never part
//! of a `Debug` output; the input method is handed bullets. The owner reads the text with
//! [`SecretField::text`] and hears of edits, Enter and Escape through [`SecretFieldEvent`].

use crate::theme;
use crate::ui::text_field::{TEXT_FIELD_CONTEXT, TextField, TextFieldEvent};
use gpui_kit::{
    App, Bounds, Context, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Subscription,
    UTF16Selection, Window, canvas, div, prelude::*, px,
};
use std::fmt;
use std::ops::Range;

/// What a secret field shows for each character of its text.
pub const BULLET: char = '\u{2022}';

/// What a secret field tells its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretFieldEvent {
    /// The text is different now.
    Changed,
    /// Enter was pressed.
    Submitted,
    /// Escape was pressed.
    Cancelled,
}

pub struct SecretField {
    focus: FocusHandle,
    /// The edited text and selection; never rendered.
    state: Entity<TextField>,
    placeholder: SharedString,
    _state_events: Subscription,
}

impl EventEmitter<SecretFieldEvent> for SecretField {}

/// Names the field and whether it holds anything, never what.
impl fmt::Debug for SecretField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SecretField").field("placeholder", &self.placeholder).finish_non_exhaustive()
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

impl SecretField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(TextField::new);
        // The state hears no keys (it is never drawn): only its edits are passed on.
        let events = cx.subscribe(&state, |_, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Changed {
                cx.emit(SecretFieldEvent::Changed);
            }
            cx.notify();
        });
        Self { focus: cx.focus_handle(), state, placeholder: SharedString::default(), _state_events: events }
    }

    /// The text shown, muted, while the field is empty.
    pub fn with_placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    /// The secret typed.
    pub fn text<'a>(&self, cx: &'a App) -> &'a str {
        self.state.read(cx).text()
    }

    /// Replaces the text and puts the caret at its end. The owner is not told: it knows.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.set_text(text, cx));
        cx.notify();
    }

    /// Empties the field.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_text("", cx);
    }

    /// What the field paints of its text: one bullet per character.
    pub fn shown(&self, cx: &App) -> String {
        self.text(cx).chars().map(|_| BULLET).collect()
    }

    /// The text the field paints: the bullets before, inside and after the selection. Rendering
    /// draws these and the placeholder only.
    pub fn painted(&self, cx: &App) -> [SharedString; 3] {
        let bullets: Vec<char> = self.shown(cx).chars().collect();
        let selection = self.char_selection(cx);
        let part = |range: Range<usize>| SharedString::from(bullets[range].iter().collect::<String>());
        [part(0..selection.start), part(selection.clone()), part(selection.end..bullets.len())]
    }

    /// The selection, in characters.
    fn char_selection(&self, cx: &App) -> Range<usize> {
        let state = self.state.read(cx);
        let text = state.text();
        let selection = state.selection();
        text[..selection.start].chars().count()..text[..selection.end].chars().count()
    }

    fn replace_units(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.replace_text_in_range(range, text, window, cx));
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let (modifiers, key) = (keystroke.modifiers, keystroke.key.as_str());
        let (text, selection) = {
            let state = self.state.read(cx);
            (state.text().to_owned(), state.selection())
        };
        let prev = |at: usize| text[..at].chars().next_back().map_or(at, |c| at - c.len_utf8());
        let next = |at: usize| text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
        let units = |byte: usize| utf16_len(&text[..byte]);
        let jump = modifiers.alt || modifiers.platform;
        if modifiers.secondary(cx) && !modifiers.alt && !modifiers.shift {
            match key {
                "a" => self.state.update(cx, |state, cx| state.select_all(cx)),
                // A secret is not copied or cut: the keys are taken so that nothing else copies it.
                "c" | "x" => {}
                "v" => {
                    let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
                    self.replace_units(None, &pasted, window, cx);
                }
                _ => return,
            }
            cx.notify();
            cx.stop_propagation();
            return;
        }
        match key {
            "left" | "right" | "home" | "end" => {
                let forward = matches!(key, "right" | "end");
                let edge = matches!(key, "home" | "end") || jump;
                let to = if edge {
                    if forward { text.len() } else { 0 }
                } else if !modifiers.shift && !selection.is_empty() {
                    if forward { selection.end } else { selection.start }
                } else if forward {
                    next(selection.end)
                } else {
                    prev(selection.start)
                };
                let range = match (modifiers.shift, forward) {
                    (true, true) => selection.start..to,
                    (true, false) => to..selection.end,
                    (false, _) => to..to,
                };
                self.state.update(cx, |state, cx| state.select_range(range, cx));
            }
            "backspace" | "delete" => {
                let range = if !selection.is_empty() {
                    selection
                } else if key == "backspace" {
                    (if jump { 0 } else { prev(selection.start) })..selection.start
                } else {
                    selection.end..(if jump { text.len() } else { next(selection.end) })
                };
                if !range.is_empty() {
                    self.replace_units(Some(units(range.start)..units(range.end)), "", window, cx);
                }
            }
            "enter" if !modifiers.modified() => cx.emit(SecretFieldEvent::Submitted),
            "escape" if !modifiers.modified() => cx.emit(SecretFieldEvent::Cancelled),
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }

    /// A press takes the keyboard (marked handled, so that a parent tracking focus keeps out of
    /// it) and puts the caret at the end: bullets give no position to aim at.
    fn mouse_down(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        window.prevent_default();
        let end = self.state.read(cx).text().len();
        self.state.update(cx, |state, cx| state.select_range(end..end, cx));
    }
}

impl Focusable for SecretField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for SecretField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        // The input method sees bullets, one per UTF-16 unit, so that its offsets still map.
        let len = utf16_len(self.text(cx));
        let clamped = range.start.min(len)..range.end.min(len);
        if clamped != range {
            *adjusted = Some(clamped.clone());
        }
        Some(std::iter::repeat_n(BULLET, clamped.len()).collect())
    }

    fn selected_text_range(
        &mut self,
        ignore_disabled_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.state.update(cx, |state, cx| state.selected_text_range(ignore_disabled_input, window, cx))
    }

    fn marked_text_range(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let state = self.state.clone();
        state.update(cx, |state, cx| state.marked_text_range(window, cx))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.unmark_text(window, cx));
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_units(range, text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        marked: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| state.replace_and_mark_text_in_range(range, new_text, marked, window, cx));
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for SecretField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.focus.is_focused(window);
        let [before, selection, after] = self.painted(cx);
        let caret = focused.then(|| div().flex_none().w(px(1.5)).h(px(16.0)).bg(theme::color(cx, "caret")));
        let content = if before.is_empty() && selection.is_empty() && after.is_empty() {
            div()
                .flex()
                .items_center()
                .children(caret)
                .child(div().text_color(theme::color(cx, "fg-muted")).child(self.placeholder.clone()))
        } else {
            let selected = theme::color(cx, if focused { "bg-editor-selection" } else { "bg-selected" });
            div()
                .flex()
                .items_center()
                .text_color(theme::color(cx, "fg-strong"))
                .child(before)
                .when(selection.is_empty(), |el| el.children(caret))
                .when(!selection.is_empty(), |el| el.child(div().bg(selected).child(selection)))
                .child(after)
        };
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .relative()
            .w_full()
            .h(theme::length(cx, "h-row"))
            .flex()
            .items_center()
            .overflow_hidden()
            .key_context(TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(content)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{ClipboardItem, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SecretFieldEvent>>>;

    /// A focused field in an active window, and what it has told its owner.
    fn field(cx: &mut TestAppContext) -> (Entity<SecretField>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            crate::platform::input::init(cx);
        });
        let (field, cx) = cx.add_window_view(|_, cx| SecretField::new(cx).with_placeholder("Value"));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&field, move |_, event: &SecretFieldEvent, _| log.borrow_mut().push(*event)).detach();
            window.activate_window();
            let focus = field.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (field, events, cx)
    }

    fn text(field: &Entity<SecretField>, cx: &mut VisualTestContext) -> String {
        field.read_with(cx, |field, cx| field.text(cx).to_owned())
    }

    fn shown(field: &Entity<SecretField>, cx: &mut VisualTestContext) -> String {
        field.read_with(cx, |field, cx| field.shown(cx))
    }

    // Not from a spec: what is typed is held as typed and painted as one bullet per character, and the
    // field's debug output names no character of it.
    #[gpui_kit::test]
    async fn typing_shows_bullets_and_keeps_the_secret(cx: &mut TestAppContext) {
        let (field, events, cx) = field(cx);
        assert_eq!(shown(&field, cx), "");
        cx.simulate_input("ské1");
        assert_eq!(text(&field, cx), "ské1");
        assert_eq!(shown(&field, cx), BULLET.to_string().repeat(4));
        assert!(events.borrow().contains(&SecretFieldEvent::Changed));
        let debug = field.read_with(cx, |field, _| format!("{field:?}"));
        assert!(!debug.contains("sk"), "{debug}");
        cx.simulate_keystrokes("left backspace");
        assert_eq!(text(&field, cx), "sk1");
        cx.simulate_keystrokes("cmd-a backspace");
        assert_eq!(text(&field, cx), "");
        field.update(cx, |field, cx| field.set_text("kept", cx));
        assert_eq!(shown(&field, cx), BULLET.to_string().repeat(4));
        field.update(cx, |field, cx| field.clear(cx));
        assert_eq!(text(&field, cx), "");
    }

    fn painted(field: &Entity<SecretField>, cx: &mut VisualTestContext) -> [SharedString; 3] {
        field.read_with(cx, |field, cx| field.painted(cx))
    }

    fn only_bullets(parts: &[SharedString; 3]) -> bool {
        parts.iter().all(|part| part.chars().all(|c| c == BULLET))
    }

    // src/features/plugins/secrets.tsx (`type="password"`): the value is masked as typed and while
    // selected, copy and cut leave the clipboard as it was and the text in place; paste inserts the
    // clipboard's text.
    #[gpui_kit::test]
    async fn copy_and_cut_put_nothing_on_the_clipboard_and_paste_inserts(cx: &mut TestAppContext) {
        let (field, _, cx) = field(cx);
        cx.simulate_input("hunter2");
        let typed = painted(&field, cx);
        assert!(only_bullets(&typed), "{typed:?}");
        assert_eq!(typed.iter().map(|part| part.chars().count()).sum::<usize>(), "hunter2".len());
        cx.write_to_clipboard(ClipboardItem::new_string("before".to_owned()));
        cx.simulate_keystrokes("cmd-a");
        let selected = painted(&field, cx);
        assert!(only_bullets(&selected) && selected[1].chars().count() == "hunter2".len(), "{selected:?}");
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("before"));
        cx.simulate_keystrokes("cmd-a cmd-x");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("before"));
        assert_eq!(text(&field, cx), "hunter2", "cut removes nothing");
        let debug = field.read_with(cx, |field, _| format!("{field:?}"));
        assert!(!debug.contains("hunter2"), "{debug}");
        cx.write_to_clipboard(ClipboardItem::new_string("pasted".to_owned()));
        cx.simulate_keystrokes("cmd-a cmd-v");
        assert_eq!(text(&field, cx), "pasted");
        assert_eq!(shown(&field, cx), BULLET.to_string().repeat(6));
    }

    // Not from a spec: Enter submits and Escape cancels, without changing the text.
    #[gpui_kit::test]
    async fn enter_submits_and_escape_cancels(cx: &mut TestAppContext) {
        let (field, events, cx) = field(cx);
        cx.simulate_input("abc");
        events.borrow_mut().clear();
        cx.simulate_keystrokes("enter");
        assert_eq!(events.borrow().as_slice(), [SecretFieldEvent::Submitted]);
        cx.simulate_keystrokes("escape");
        assert_eq!(events.borrow().last(), Some(&SecretFieldEvent::Cancelled));
        assert_eq!(text(&field, cx), "abc");
    }

    // Not from a spec: an input method composes and commits into the text, and what it reads back of
    // the field is bullets.
    #[gpui_kit::test]
    async fn an_input_method_commits_and_reads_bullets(cx: &mut TestAppContext) {
        let (field, _, cx) = field(cx);
        cx.simulate_input("a");
        field.update_in(cx, |field, window, cx| field.replace_and_mark_text_in_range(None, "に", None, window, cx));
        let marked = field.update_in(cx, |field, window, cx| field.marked_text_range(window, cx));
        assert_eq!(marked, Some(1..2));
        field.update_in(cx, |field, window, cx| field.replace_text_in_range(None, "日本", window, cx));
        assert_eq!(text(&field, cx), "a日本");
        assert_eq!(field.update_in(cx, |field, window, cx| field.marked_text_range(window, cx)), None);
        let mut adjusted = None;
        let read = field.update_in(cx, |field, window, cx| field.text_for_range(0..10, &mut adjusted, window, cx));
        assert_eq!(read, Some(BULLET.to_string().repeat(3)));
        assert_eq!(adjusted, Some(0..3));
        assert_eq!(shown(&field, cx), BULLET.to_string().repeat(3));
    }
}
