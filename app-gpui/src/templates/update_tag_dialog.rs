//! The prompt of Update Tag with Emmet: the abbreviation to apply to the tag at the caret
//! (`section.card#main[data-x=1]`). OK is pressed by Enter once something is typed (not while an
//! input method is composing); Escape or Cancel closes it. The dialog edits nothing: the workbench
//! applies the abbreviation it reports.

use crate::platform::i18n::{Localization, Param};
use crate::platform::input;
use crate::theme;
use crate::theme::installed_fonts::mono_family;
use crate::ui::text_field::{TextField, TextFieldEvent};
use crate::vcs::prompt_dialogs::button;
use gpui_kit::{
    App, ClickEvent, Context, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, Keystroke, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};

const WIDTH: f32 = 420.0;
const FIELD_HEIGHT: f32 = 28.0;
/// The abbreviation the prompt shows as an example.
const EXAMPLE: &str = "section.card#main[data-x=1]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateTagEvent {
    /// OK: apply this abbreviation (as typed) to the tag.
    Apply(String),
    /// Escape or Cancel.
    Cancelled,
}

pub struct UpdateTagDialog {
    tag: String,
    field: Entity<TextField>,
    code_font: SharedString,
    key_claim: u64,
    _field_events: Subscription,
}

impl EventEmitter<UpdateTagEvent> for UpdateTagDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

impl UpdateTagDialog {
    /// The prompt for the element `tag` (its name).
    pub fn new(tag: &str, cx: &mut Context<Self>) -> Self {
        let field = cx.new(TextField::new);
        let field_events = cx.subscribe(&field, |_: &mut Self, _, event: &TextFieldEvent, cx| {
            // Enter and Escape are claimed before the field sees them.
            if *event == TextFieldEvent::Changed {
                cx.notify();
            }
        });
        let claimant = cx.weak_entity();
        let key_claim = input::claim_keys(
            field.focus_handle(cx),
            move |keystroke, window, cx| {
                claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, window, cx)).unwrap_or(false)
            },
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        Self { tag: tag.to_owned(), field, code_font: mono_family(cx), key_claim, _field_events: field_events }
    }

    pub fn title(&self, cx: &mut App) -> String {
        tx(cx, "Update Tag with Emmet")
    }

    /// The text above the field, in the UI language, followed by the example abbreviation.
    pub fn label(&self, cx: &mut App) -> String {
        let label = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["Abbreviation for <", ">, e.g."], &[Param::from(self.tag.as_str())]);
        format!("{label} {EXAMPLE}")
    }

    pub fn field(&self) -> &Entity<TextField> {
        &self.field
    }

    /// Whether OK can be pressed: something besides blanks is typed.
    pub fn can_apply(&self, cx: &App) -> bool {
        !self.field.read(cx).text().trim().is_empty()
    }

    /// The buttons, in the UI language, with whether each can be pressed.
    pub fn buttons(&self, cx: &mut App) -> Vec<(String, bool)> {
        let can_apply = self.can_apply(cx);
        vec![(tx(cx, "OK"), can_apply), (tx(cx, "Cancel"), true)]
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.can_apply(cx) {
            let abbreviation = self.field.read(cx).text().to_owned();
            cx.emit(UpdateTagEvent::Apply(abbreviation));
        }
    }

    fn cancel(&mut self, cx: &mut Context<Self>) {
        cx.emit(UpdateTagEvent::Cancelled);
    }

    fn claim_key(&mut self, keystroke: &Keystroke, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        // Enter confirms the text an input method is composing.
        if self.field.update(cx, |field, cx| field.marked_text_range(window, cx)).is_some() {
            return false;
        }
        match keystroke.key.as_str() {
            "enter" => self.apply(cx),
            "escape" => self.cancel(cx),
            _ => return false,
        }
        true
    }
}

impl Focusable for UpdateTagDialog {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.field.focus_handle(cx)
    }
}

impl Render for UpdateTagDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        let label = cx
            .default_global::<Localization>()
            .0
            .tx_built(&["Abbreviation for <", ">, e.g."], &[Param::from(self.tag.as_str())]);
        let buttons = self.buttons(cx);
        let row: Vec<_> = buttons
            .into_iter()
            .enumerate()
            .map(|(index, (text, enabled))| {
                let id = if index == 0 { "update-tag-ok" } else { "update-tag-cancel" };
                button(cx, text, index == 0, false, enabled).id(id).debug_selector(move || id.to_owned()).on_click(
                    cx.listener(
                        move |dialog, _: &ClickEvent, _, cx| {
                            if index == 0 { dialog.apply(cx) } else { dialog.cancel(cx) }
                        },
                    ),
                )
            })
            .collect();
        let example = div().font_family(self.code_font.clone()).text_color(theme::color(cx, "fg")).child(EXAMPLE);
        let field = div()
            .h(px(FIELD_HEIGHT))
            .flex()
            .items_center()
            .px(px(8.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-input"))
            .font_family(self.code_font.clone())
            .child(div().flex_1().min_w_0().child(self.field.clone()));
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
            // A click on the dialog is not a click beside it.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(
                div()
                    .px(px(16.0))
                    .py(px(12.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(4.0))
                            .text_color(theme::color(cx, "fg-muted"))
                            .child(SharedString::from(label))
                            .child(example),
                    )
                    .child(field),
            )
            .child(div().flex().justify_end().gap(px(8.0)).px(px(16.0)).pb(px(12.0)).children(row))
    }
}
