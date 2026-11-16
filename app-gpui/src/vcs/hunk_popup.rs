//! The popup a press on an editor's change bar opens: a toolbar (Previous Change, Next Change,
//! Rollback, Show Diff, Copy Original Text, the kind of the change with its place among the file's
//! changes, Close) over the change's original lines, marked `-`, and its current lines, marked `+`.
//!
//! The workbench fills it from the editor's hunk ([`HunkPopupData::of`]) and carries out what it
//! reports through [`HunkPopupEvent`]; copying the original text is the popup's own. Rollback has the
//! keyboard: Enter rolls back, Escape closes.

use crate::platform::i18n::{Localization, Param};
use crate::theme;
use crate::ui::tooltip::WithTooltip as _;
use gpui_kit::{
    App, ClickEvent, ClipboardItem, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent,
    Render, SharedString, Stateful, Window, div, prelude::*, px,
};
use ide_vcs::HunkKind;

/// What the popup asks of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HunkPopupEvent {
    /// Move the caret to the previous change and show its popup.
    Previous,
    /// Move the caret to the next change and show its popup.
    Next,
    /// Put the change's original text back.
    Rollback,
    /// Show the file's diff at the change.
    ShowDiff,
    Close,
}

pub const PREVIOUS: &str = "hunk-previous";
pub const NEXT: &str = "hunk-next";
pub const ROLLBACK: &str = "hunk-rollback";
pub const SHOW_DIFF: &str = "hunk-show-diff";
pub const COPY: &str = "hunk-copy";
pub const CLOSE: &str = "hunk-close";

/// The actions whose shortcuts the move buttons' tooltips show.
const PREVIOUS_ACTION: &str = "vcs.prevChange";
const NEXT_ACTION: &str = "vcs.nextChange";

/// The title of a change of `kind`.
pub fn kind_title(kind: HunkKind) -> &'static str {
    match kind {
        HunkKind::Added => "Added lines",
        HunkKind::Modified => "Modified lines",
        HunkKind::Deleted => "Deleted lines",
    }
}

/// The lines of `text`, without a last empty one after a final line break.
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// What the popup shows of one change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HunkPopupData {
    pub kind: HunkKind,
    /// The change's text at HEAD, line breaks included; empty for added lines.
    pub old_text: String,
    pub old_lines: Vec<String>,
    /// The change's lines as the editor has them now.
    pub new_lines: Vec<String>,
    /// Its 0-based place among the file's changes, when it is still one of them.
    pub index: Option<usize>,
    pub count: usize,
}

impl HunkPopupData {
    /// The popup of a change of `kind` whose original text is `old_text` and whose current text is
    /// `current`, the `index`-th of `count`.
    pub fn of(kind: HunkKind, old_text: &str, current: &str, index: Option<usize>, count: usize) -> Self {
        Self {
            kind,
            old_text: old_text.to_owned(),
            old_lines: split_lines(old_text),
            new_lines: split_lines(current),
            index,
            count,
        }
    }

    /// The counter beside the title (`2 of 3`): shown only among several changes.
    fn counter(&self, cx: &mut App) -> Option<String> {
        let index = self.index.filter(|_| self.count > 1)?;
        let i18n = &mut cx.default_global::<Localization>().0;
        Some(i18n.tx_built(&["", " of ", ""], &[Param::from(index + 1), Param::from(self.count)]))
    }
}

/// A toolbar button as drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopupButton {
    pub id: &'static str,
    pub enabled: bool,
}

pub struct HunkPopup {
    data: HunkPopupData,
    /// The original text was copied: the copy button shows a check mark.
    copied: bool,
    focus: FocusHandle,
}

impl EventEmitter<HunkPopupEvent> for HunkPopup {}

impl HunkPopup {
    pub fn new(data: HunkPopupData, cx: &mut Context<Self>) -> Self {
        Self { data, copied: false, focus: cx.focus_handle() }
    }

    pub fn data(&self) -> &HunkPopupData {
        &self.data
    }

    /// The title: the kind of change, and its place among several.
    pub fn title(&self, cx: &mut App) -> (String, Option<String>) {
        let title = cx.default_global::<Localization>().0.tx(kind_title(self.data.kind));
        (title, self.data.counter(cx))
    }

    /// The body's lines as drawn: `-` before an original line, `+` before a current one.
    pub fn lines(&self) -> Vec<String> {
        let removed = self.data.old_lines.iter().map(|line| format!("- {line}"));
        let added = self.data.new_lines.iter().map(|line| format!("+ {line}"));
        removed.chain(added).collect()
    }

    pub fn is_copied(&self) -> bool {
        self.copied
    }

    /// The buttons and whether each can be pressed: Copy Original Text only with an original text.
    pub fn toolbar(&self) -> Vec<PopupButton> {
        [PREVIOUS, NEXT, ROLLBACK, SHOW_DIFF, COPY, CLOSE]
            .into_iter()
            .map(|id| PopupButton { id, enabled: id != COPY || !self.data.old_text.is_empty() })
            .collect()
    }

    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        match id {
            PREVIOUS => cx.emit(HunkPopupEvent::Previous),
            NEXT => cx.emit(HunkPopupEvent::Next),
            ROLLBACK => cx.emit(HunkPopupEvent::Rollback),
            SHOW_DIFF => cx.emit(HunkPopupEvent::ShowDiff),
            CLOSE => cx.emit(HunkPopupEvent::Close),
            COPY if !self.data.old_text.is_empty() => {
                cx.write_to_clipboard(ClipboardItem::new_string(self.data.old_text.clone()));
                self.copied = true;
                cx.notify();
            }
            _ => {}
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(HunkPopupEvent::Close),
            "enter" => cx.emit(HunkPopupEvent::Rollback),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn button(
        &self,
        id: &'static str,
        icon: &'static str,
        title: &str,
        shortcut_of: Option<&'static str>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let enabled = self.toolbar().iter().any(|button| button.id == id && button.enabled);
        let title = cx.default_global::<Localization>().0.tx(title);
        let color = theme::color(cx, if enabled { "fg" } else { "fg-faint" });
        let button = div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .flex_none()
            .size(px(22.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(!enabled, |el| el.opacity(0.4))
            .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .child(theme::icon(cx, icon, 14.0, color))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(id, cx)));
        match shortcut_of {
            Some(action) => button.with_tooltip_shortcut(title, action),
            None => button.with_tooltip(title),
        }
    }

    fn body_line(sign: &'static str, text: &str, removed: bool, cx: &App) -> Div {
        let (token, share) = if removed { ("gutter-deleted", 0.14) } else { ("gutter-added", 0.16) };
        let tint = theme::color(cx, token);
        let text = if text.is_empty() { " ".to_owned() } else { text.to_owned() };
        div()
            .flex()
            .pl(px(4.0))
            .pr(px(12.0))
            .whitespace_nowrap()
            .bg(tint.opacity(share))
            .text_color(theme::color(cx, if removed { "fg-muted" } else { "fg" }))
            .child(div().flex_none().w(px(16.0)).text_color(theme::color(cx, "fg-faint")).child(sign))
            .child(SharedString::from(text))
    }
}

impl Focusable for HunkPopup {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HunkPopup {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (title, counter) = self.title(cx);
        let copy_icon = if self.copied { "check" } else { "copy" };
        let toolbar = div()
            .flex_none()
            .h(px(28.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border-popup"))
            .child(self.button(PREVIOUS, "arrow-up", "Previous Change", Some(PREVIOUS_ACTION), cx))
            .child(self.button(NEXT, "arrow-down", "Next Change", Some(NEXT_ACTION), cx))
            .child(self.button(ROLLBACK, "undo", "Rollback", None, cx))
            .child(self.button(SHOW_DIFF, "diff", "Show Diff", None, cx))
            .child(self.button(COPY, copy_icon, "Copy Original Text", None, cx))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .gap(px(8.0))
                    .px(px(8.0))
                    .whitespace_nowrap()
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(title))
                    .children(counter.map(|counter| {
                        div().text_color(theme::color(cx, "fg-faint")).child(SharedString::from(counter))
                    })),
            )
            .child(self.button(CLOSE, "close", "Close", None, cx));
        let has_body = !self.data.old_lines.is_empty() || !self.data.new_lines.is_empty();
        let body = has_body.then(|| {
            let removed = self.data.old_lines.iter().map(|line| Self::body_line("-", line, true, cx));
            let added = self.data.new_lines.iter().map(|line| Self::body_line("+", line, false, cx));
            let rows: Vec<Div> = removed.chain(added).collect();
            div()
                .id("hunk-popup-body")
                .max_h(px(280.0))
                .overflow_y_scroll()
                .py(px(4.0))
                .font_family(theme::installed_fonts::mono_family(cx))
                .text_size(theme::length(cx, "fs-code"))
                .line_height(theme::length(cx, "lh-code"))
                .children(rows)
        });
        div()
            .id("hunk-popup")
            .debug_selector(|| "hunk-popup".to_owned())
            .min_w(px(320.0))
            .max_w(px(720.0))
            .flex()
            .flex_col()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .font_family(theme::installed_fonts::ui_family(cx))
            .text_size(theme::length(cx, "fs-ui"))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .child(toolbar)
            .children(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<HunkPopupEvent>>>;

    fn open(cx: &mut TestAppContext, data: HunkPopupData) -> (Entity<HunkPopup>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| HunkPopup::new(data, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &HunkPopupEvent, _| log.borrow_mut().push(*event)).detach();
            window.activate_window();
            window.focus(&view.focus_handle(cx), cx);
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// A modified change shows its original lines marked `-` and its current lines marked `+`, under
    /// its title and its place among the file's changes.
    #[gpui_kit::test]
    async fn shows_the_original_and_the_current_lines_under_the_title_and_counter(cx: &mut TestAppContext) {
        let data = HunkPopupData::of(HunkKind::Modified, "two\nthree\n", "TWO\n", Some(1), 3);
        let (view, _, cx) = open(cx, data);
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["- two", "- three", "+ TWO"]);
        let title = view.update(cx, |view, cx| view.title(cx));
        assert_eq!(title, ("Modified lines".to_owned(), Some("2 of 3".to_owned())));
        // The only change of a file has no counter.
        view.update(cx, |view, _| view.data.count = 1);
        assert_eq!(view.update(cx, |view, cx| view.title(cx)).1, None);
    }

    /// The buttons report what the window carries out; Enter rolls back and Escape closes.
    #[gpui_kit::test]
    async fn the_toolbar_and_the_keys_report_each_action(cx: &mut TestAppContext) {
        let data = HunkPopupData::of(HunkKind::Deleted, "gone\n", "", Some(0), 1);
        let (view, events, cx) = open(cx, data);
        for id in [PREVIOUS, NEXT, ROLLBACK, SHOW_DIFF, CLOSE] {
            click(cx, id);
        }
        cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
        cx.simulate_keystrokes("enter");
        cx.simulate_keystrokes("escape");
        use HunkPopupEvent as E;
        assert_eq!(*events.borrow(), [E::Previous, E::Next, E::Rollback, E::ShowDiff, E::Close, E::Rollback, E::Close]);
        assert_eq!(view.read_with(cx, |view, _| view.lines()), ["- gone"]);
    }

    /// Copy Original Text puts the change's original text on the clipboard and shows a check mark;
    /// added lines have no original text to copy.
    #[gpui_kit::test]
    async fn copy_original_text_copies_the_lines_at_head(cx: &mut TestAppContext) {
        let data = HunkPopupData::of(HunkKind::Modified, "b\n", "B\n", Some(0), 1);
        let (view, _, cx) = open(cx, data);
        click(cx, COPY);
        assert!(view.read_with(cx, |view, _| view.is_copied()));
        let copied = cx.update(|_, cx| cx.read_from_clipboard().and_then(|item| item.text()));
        assert_eq!(copied.as_deref(), Some("b\n"));

        let added = HunkPopupData::of(HunkKind::Added, "", "new\n", Some(0), 1);
        view.update(cx, |view, cx| *view = HunkPopup::new(added, cx));
        let copy = view.read_with(cx, |view, _| view.toolbar().into_iter().find(|button| button.id == COPY));
        assert_eq!(copy, Some(PopupButton { id: COPY, enabled: false }));
        click(cx, COPY);
        assert!(!view.read_with(cx, |view, _| view.is_copied()));
    }
}
