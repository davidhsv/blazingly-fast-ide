//! New Tag… of the Git log: the name of a lightweight tag on a commit, checked as it is typed
//! against git's rules for ref names and the repository's tags. Create needs a name without a
//! problem; Enter presses it, Cancel and Escape answer nothing.
//!
//! The dialog runs nothing: the name goes to the workbench, which creates the tag.

use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs_ops::dialogs::PromptRequest;
use crate::vcs_ops::operations::validate_tag_name;
use gpui_kit::{
    App, AppContext, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    Keystroke, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};

/// The ids of the dialog's buttons.
pub const CREATE: &str = "create";
pub const CANCEL: &str = "cancel";
const WIDTH: f32 = 420.0;

/// What the dialog asks of the workbench.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagDialogEvent {
    /// The dialog is done: the tag's name, trimmed; `None` when it was cancelled.
    Closed(Option<String>),
}

pub struct TagDialog {
    request: PromptRequest,
    /// The repository's tags: a name among them is refused.
    existing: Vec<String>,
    field: Entity<TextField>,
    /// The name was edited: its problem shows.
    touched: bool,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<TagDialogEvent> for TagDialog {}

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

impl TagDialog {
    /// The dialog `request` describes (its title, label and button), refusing the `existing` tags.
    pub fn new(request: PromptRequest, existing: Vec<String>, cx: &mut Context<Self>) -> Self {
        let field = cx.new(|cx| {
            let mut field = TextField::new(cx);
            field.set_text(&request.initial, cx);
            field
        });
        let events = cx.subscribe(&field, |dialog: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => {
                dialog.touched = true;
                cx.notify();
            }
            TextFieldEvent::Submitted => dialog.press(CREATE, cx),
            TextFieldEvent::Cancelled => dialog.press(CANCEL, cx),
        });
        let focus = field.focus_handle(cx);
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus,
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { request, existing, field, touched: false, key_claim, _field_events: events }
    }

    pub fn title(&self) -> &str {
        &self.request.title
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// The name as typed, trimmed.
    pub fn name(&self, cx: &App) -> String {
        self.field.read(cx).text().trim().to_owned()
    }

    /// Why the typed name cannot be the tag's, in English.
    pub fn problem(&self, cx: &App) -> Option<String> {
        validate_tag_name(&self.name(cx), &self.existing)
    }

    /// The problem shown under the field, in the UI language: once the name is edited and while it
    /// is not empty.
    pub fn errors(&self, cx: &mut App) -> Vec<String> {
        if !self.touched || self.field.read(cx).text().is_empty() {
            return Vec::new();
        }
        let problem = self.problem(cx);
        problem.map(|problem| tx(cx, &problem)).into_iter().collect()
    }

    /// Presses the button `id` ([`CREATE`] or [`CANCEL`]); Create does nothing while the name has a
    /// problem, which then shows.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if id != CREATE {
            cx.emit(TagDialogEvent::Closed(None));
            return;
        }
        if self.problem(cx).is_some() {
            self.touched = true;
            cx.notify();
            return;
        }
        let name = self.name(cx);
        cx.emit(TagDialogEvent::Closed(Some(name)));
    }

    /// Enter creates, Escape cancels: the dialog's own keys, ahead of the keymap.
    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.press(CREATE, cx),
            "escape" => self.press(CANCEL, cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for TagDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for TagDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (fg, strong, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"));
        let danger = theme::color(cx, "danger");
        let title = tx(cx, &self.request.title);
        let label = self.request.label.clone().map(|label| tx(cx, &label));
        let errors = self.errors(cx);
        let enabled = self.problem(cx).is_none();
        let ok = if self.request.ok_label.is_empty() { "OK".to_owned() } else { self.request.ok_label.clone() };
        let buttons: Vec<_> = [(CREATE, ok, true), (CANCEL, "Cancel".to_owned(), false)]
            .into_iter()
            .map(|(id, label, primary)| {
                let label = tx(cx, &label);
                button(cx, label, primary, id != CREATE || enabled)
                    .id(SharedString::from(format!("tag-button-{id}")))
                    .debug_selector(move || format!("tag-button-{id}"))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.press(id, cx)))
            })
            .collect();
        let frame = div()
            .h(px(28.0))
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, if errors.is_empty() { "border" } else { "danger" }))
            .bg(theme::color(cx, "bg-input"))
            .child(div().flex_1().min_w_0().child(self.field.clone()));
        let small = theme::length(cx, "fs-small");
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
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .children(label.map(|label| div().text_color(muted).child(SharedString::from(label))))
                    .child(frame)
                    .children(
                        errors
                            .into_iter()
                            .map(|error| div().text_color(danger).text_size(small).child(SharedString::from(error))),
                    ),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0)).children(buttons))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Answers = Rc<RefCell<Vec<Option<String>>>>;

    fn open(cx: &mut TestAppContext) -> (Entity<TagDialog>, Answers, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let request = PromptRequest {
            title: "Create New Tag On 1234abcd".to_owned(),
            label: Some("Enter the name of new tag:".to_owned()),
            ok_label: "Create".to_owned(),
            mono: true,
            ..PromptRequest::default()
        };
        let (dialog, cx) = cx.add_window_view(move |_, cx| TagDialog::new(request, vec!["v1".to_owned()], cx));
        let answers: Answers = Rc::default();
        let log = Rc::clone(&answers);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &TagDialogEvent, _| {
                let TagDialogEvent::Closed(answer) = event;
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

    // Not from a spec: a tag in use and a name git refuses show their problem and Enter does not
    // create; a free name is created with Enter.
    #[gpui_kit::test]
    async fn a_name_in_use_or_invalid_is_refused_and_a_free_one_is_created(cx: &mut TestAppContext) {
        let (dialog, answers, cx) = open(cx);
        assert!(dialog.update(cx, |dialog, cx| dialog.errors(cx)).is_empty(), "nothing typed, nothing shown");
        cx.simulate_input("v1");
        cx.run_until_parked();
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.errors(cx)), ["Tag 'v1' already exists"]);
        cx.simulate_keystrokes("enter");
        assert!(answers.borrow().is_empty(), "a tag in use is not created");
        cx.simulate_keystrokes("backspace backspace");
        cx.simulate_input("bad name");
        cx.run_until_parked();
        assert_eq!(dialog.update(cx, |dialog, cx| dialog.errors(cx)), ["Invalid tag name"]);
        for _ in 0.."bad name".len() {
            cx.simulate_keystrokes("backspace");
        }
        cx.simulate_input("v2");
        cx.simulate_keystrokes("enter");
        assert_eq!(answers.borrow().as_slice(), [Some("v2".to_owned())]);
    }

    // Not from a spec: Escape answers nothing.
    #[gpui_kit::test]
    async fn escape_answers_nothing(cx: &mut TestAppContext) {
        let (_dialog, answers, cx) = open(cx);
        cx.simulate_keystrokes("escape");
        assert_eq!(answers.borrow().as_slice(), [None]);
    }
}
