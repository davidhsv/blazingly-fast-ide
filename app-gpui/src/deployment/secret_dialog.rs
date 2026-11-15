//! SSH Password: asks for the password or key passphrase a deployment connection was refused
//! without, with Save password. The secret is typed in a [`SecretField`], which paints bullets
//! only; it leaves the dialog in [`SecretDialogEvent::Answered`] as a [`SecretText`], whose `Debug`
//! output never shows it.

use crate::database::store::SecretText;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
use gpui_kit::{
    App, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, MouseButton,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};

const WIDTH: f32 = 420.0;

/// How the dialog was closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SecretDialogEvent {
    /// OK or Enter with something typed.
    Answered { secret: SecretText, save: bool },
    /// Cancel or Escape.
    Cancelled,
}

pub struct SecretDialog {
    focus: FocusHandle,
    message: String,
    field: Entity<SecretField>,
    save: bool,
    _events: Subscription,
}

impl EventEmitter<SecretDialogEvent> for SecretDialog {}

fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl SecretDialog {
    /// The prompt for the connection refused with `message`.
    pub fn new(message: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let placeholder = tx(cx, "Password or passphrase");
        let field = cx.new(|cx| SecretField::new(cx).with_placeholder(placeholder));
        let events = cx.subscribe_in(&field, window, |dialog, _, event: &SecretFieldEvent, _, cx| match event {
            SecretFieldEvent::Changed => cx.notify(),
            SecretFieldEvent::Submitted => dialog.submit(cx),
            SecretFieldEvent::Cancelled => cx.emit(SecretDialogEvent::Cancelled),
        });
        Self { focus: cx.focus_handle(), message, field, save: false, _events: events }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn field(&self) -> &Entity<SecretField> {
        &self.field
    }

    /// Whether Save password is ticked.
    pub fn save(&self) -> bool {
        self.save
    }

    /// OK can be pressed: something is typed.
    pub fn can_submit(&self, cx: &App) -> bool {
        !self.field.read(cx).text(cx).is_empty()
    }

    /// OK or Enter: answers with what is typed, once something is.
    pub fn submit(&mut self, cx: &mut Context<Self>) {
        if !self.can_submit(cx) {
            return;
        }
        let secret = SecretText::from(self.field.read(cx).text(cx));
        self.field.update(cx, |field, cx| field.clear(cx));
        cx.emit(SecretDialogEvent::Answered { secret, save: self.save });
    }

    fn toggle_save(&mut self, cx: &mut Context<Self>) {
        self.save = !self.save;
        cx.notify();
    }
}

impl Focusable for SecretDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for SecretDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let can_submit = self.can_submit(cx);
        let check = crate::vcs::prompt_dialogs::checkbox(cx, tx(cx, "Save password"), self.save)
            .id("secret-save")
            .debug_selector(|| "secret-save".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.toggle_save(cx)));
        let cancel = crate::vcs::prompt_dialogs::button(cx, tx(cx, "Cancel"), false, false, true)
            .id("secret-cancel")
            .debug_selector(|| "secret-cancel".to_owned())
            .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SecretDialogEvent::Cancelled)));
        let ok = crate::vcs::prompt_dialogs::button(cx, tx(cx, "OK"), true, false, can_submit)
            .id("secret-ok")
            .debug_selector(|| "secret-ok".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.submit(cx)));
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .track_focus(&self.focus)
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(tx(cx, "SSH Password"))),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .child(
                        div()
                            .debug_selector(|| "secret-message".to_owned())
                            .text_color(theme::color(cx, "fg-subtle"))
                            .text_size(theme::length(cx, "fs-small"))
                            .child(SharedString::from(self.message.clone())),
                    )
                    .child(
                        div()
                            .debug_selector(|| "secret-field".to_owned())
                            .h(px(28.0))
                            .flex()
                            .items_center()
                            .px(px(8.0))
                            .rounded(theme::length(cx, "radius-small"))
                            .border_1()
                            .border_color(theme::color(cx, "border"))
                            .bg(theme::color(cx, "bg-input"))
                            .child(div().flex_1().min_w_0().child(self.field.clone())),
                    )
                    .child(check),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).child(cancel).child(ok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::secret_field::BULLET;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SecretDialogEvent>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<SecretDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            crate::platform::input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(|window, cx| {
            SecretDialog::new("SSH password required for deploy@box".to_owned(), window, cx)
        });
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&dialog, move |_, event: &SecretDialogEvent, _| seen.borrow_mut().push(event.clone()))
                .detach();
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.focus(&dialog.focus_handle(cx), cx);
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// The prompt shows why the connection was refused; what is typed is painted as bullets, OK
    /// waits for it, and OK answers it with Save password.
    #[gpui_kit::test]
    async fn the_secret_is_painted_as_bullets_and_ok_answers_it_with_save_password(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx);
        assert_eq!(
            dialog.read_with(cx, |dialog, _| dialog.message().to_owned()),
            "SSH password required for deploy@box"
        );
        assert!(!dialog.read_with(cx, |dialog, cx| dialog.can_submit(cx)));
        click(cx, "secret-ok");
        assert!(events.borrow().is_empty(), "OK waits for a secret");
        cx.simulate_input("pw");
        cx.run_until_parked();
        let field = dialog.read_with(cx, |dialog, _| dialog.field().clone());
        assert_eq!(field.read_with(cx, |field, cx| field.shown(cx)), format!("{BULLET}{BULLET}"));
        click(cx, "secret-save");
        assert!(dialog.read_with(cx, |dialog, _| dialog.save()));
        click(cx, "secret-ok");
        let answered = SecretDialogEvent::Answered { secret: SecretText::from("pw"), save: true };
        assert_eq!(events.borrow().as_slice(), [answered]);
        let printed = format!("{:?}", events.borrow()[0]);
        assert!(!printed.contains("pw\""), "the secret is not printed: {printed}");
    }

    /// Escape in the field and Cancel close the prompt without an answer.
    #[gpui_kit::test]
    async fn escape_and_cancel_close_the_prompt_without_an_answer(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx);
        cx.simulate_input("pw");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(events.borrow().as_slice(), [SecretDialogEvent::Cancelled]);
        click(cx, "secret-cancel");
        assert_eq!(events.borrow().len(), 2);
    }
}
