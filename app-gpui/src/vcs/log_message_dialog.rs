//! The commit message dialog of the Git log's history rewriting: Edit Commit Message… edits one
//! commit's message, Squash Commits… the message of the commit the selection becomes. The message
//! is edited in a multi-line area; OK needs a message that is not blank, Cancel and Escape answer
//! nothing.
//!
//! The dialog runs nothing: the message goes to the workbench, which rewrites the history.

use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};

/// The ids of the dialog's buttons.
pub const OK: &str = "ok";
pub const CANCEL: &str = "cancel";
const WIDTH: f32 = 520.0;

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageDialogEvent {
    /// The dialog is done: the message as typed, `None` when it was cancelled.
    Closed(Option<String>),
}

pub struct MessageDialog {
    title: String,
    area: Entity<TextArea>,
    _area_events: Subscription,
}

impl EventEmitter<MessageDialogEvent> for MessageDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(28.0))
        .min_w(px(72.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

impl MessageDialog {
    /// The dialog titled `title` (already in the UI language), with `initial` in the area.
    pub fn new(title: &str, initial: &str, cx: &mut Context<Self>) -> Self {
        let area = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_rows(8, 16, cx);
            area.set_text(initial, cx);
            area
        });
        let events = cx.subscribe(&area, |dialog: &mut Self, _, event: &TextAreaEvent, cx| match event {
            TextAreaEvent::Changed => cx.notify(),
            TextAreaEvent::Cancelled => dialog.press(CANCEL, cx),
            TextAreaEvent::Submitted | TextAreaEvent::Blurred => {}
        });
        Self { title: title.to_owned(), area, _area_events: events }
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The message area.
    pub fn area(&self) -> &Entity<TextArea> {
        &self.area
    }

    /// The message as typed.
    pub fn message(&self, cx: &App) -> String {
        self.area.read(cx).text().to_owned()
    }

    /// OK can be pressed: the message is not blank.
    pub fn can_confirm(&self, cx: &App) -> bool {
        !self.area.read(cx).text().trim().is_empty()
    }

    /// Presses the button `id` ([`OK`] or [`CANCEL`]); OK does nothing while the message is blank.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == OK {
            if !self.can_confirm(cx) {
                return;
            }
            let message = self.message(cx);
            cx.emit(MessageDialogEvent::Closed(Some(message)));
        } else {
            cx.emit(MessageDialogEvent::Closed(None));
        }
    }
}

impl Focusable for MessageDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.area.focus_handle(cx)
    }
}

impl Render for MessageDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"));
        let title = self.title.clone();
        let enabled = self.can_confirm(cx);
        let buttons: Vec<_> = [(OK, "OK", true), (CANCEL, "Cancel", false)]
            .into_iter()
            .map(|(id, label, primary)| {
                let label = tx(cx, label);
                button(cx, label, primary, id != OK || enabled)
                    .id(SharedString::from(format!("message-button-{id}")))
                    .debug_selector(move || format!("message-button-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
            })
            .collect();
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(fg)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(strong)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div().px(px(16.0)).py(px(4.0)).child(
                    div()
                        .p(px(6.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border"))
                        .bg(theme::color(cx, "bg-input"))
                        .child(self.area.clone()),
                ),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::input;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<String>>>>;

    fn open<'a>(
        cx: &'a mut TestAppContext,
        initial: &str,
    ) -> (Entity<MessageDialog>, Answers, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let initial = initial.to_owned();
        let (dialog, cx) =
            cx.add_window_view(move |_, cx| MessageDialog::new("Edit Commit Message (1234abcd)", &initial, cx));
        let answers: Answers = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &MessageDialogEvent, _| {
                let MessageDialogEvent::Closed(answer) = event;
                log.borrow_mut().push(answer.clone());
            })
            .detach();
            window.activate_window();
            let focus = dialog.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        (dialog, answers, cx)
    }

    // Not from a spec: the area holds the whole message, typed lines join it, and OK answers it.
    #[gpui_kit::test]
    async fn the_message_is_edited_with_its_body_and_ok_answers_it(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, "Subject\n\nBody");
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.message(cx)), "Subject\n\nBody");
        cx.simulate_keystrokes("enter");
        cx.simulate_input("More");
        cx.run_until_parked();
        assert_eq!(dialog.read_with(cx, |dialog, cx| dialog.message(cx)), "Subject\n\nBody\nMore");
        dialog.update(cx, |dialog, cx| dialog.press(OK, cx));
        assert_eq!(answers.borrow().as_slice(), [Some("Subject\n\nBody\nMore".to_owned())]);
    }

    // Not from a spec: a blank message cannot be accepted; Escape answers nothing.
    #[gpui_kit::test]
    async fn a_blank_message_is_refused_and_escape_cancels(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx, "  ");
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.can_confirm(cx)));
        dialog.update(cx, |dialog, cx| dialog.press(OK, cx));
        assert!(answers.borrow().is_empty(), "OK does nothing while the message is blank");
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().as_slice(), [None]);
    }
}
