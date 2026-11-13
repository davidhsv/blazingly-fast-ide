//! The application's own dialogs: About, New Project, Invalidate Caches and the question asked
//! before an untrusted project opens. Each view shows what its constructor was given and reports
//! through events what the workbench must do; none of them reaches the backend.

pub mod about_dialog;
pub mod invalidate_caches_dialog;
pub mod new_project_dialog;
pub mod trust_dialog;

use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{App, Div, FontWeight, MouseButton, SharedString, Stateful, div, prelude::*, px};

const BUTTON_HEIGHT: f32 = 28.0;
const FIELD_HEIGHT: f32 = 28.0;

/// `text` in the UI language.
pub(crate) fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// Catalog entry `key` in the UI language.
pub(crate) fn t(cx: &mut App, key: &str) -> String {
    cx.default_global::<Localization>().0.t(key, &[])
}

/// The box of a dialog `width` pixels wide; a click inside it is not a click beside it.
pub(crate) fn frame(cx: &App, width: f32) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .bg(theme::color(cx, "bg-popup"))
        .border_1()
        .border_color(theme::color(cx, "border-popup"))
        .rounded(theme::length(cx, "radius-popup"))
        .text_size(theme::length(cx, "fs-ui"))
        .text_color(theme::color(cx, "fg"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// A dialog's title row.
pub(crate) fn title_row(cx: &App, title: String) -> Div {
    div()
        .px(px(16.0))
        .pt(px(14.0))
        .pb(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(title))
}

/// The row of buttons at the foot of a dialog, right-aligned after `start`.
pub(crate) fn footer(start: Option<Stateful<Div>>, buttons: Vec<Stateful<Div>>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(12.0))
        .children(start)
        .child(div().flex_1())
        .children(buttons)
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
pub(crate) fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(BUTTON_HEIGHT))
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

/// A check box with its label.
pub(crate) fn checkbox(cx: &App, label: String, on: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(SharedString::from(label))
}

/// The frame a text field is drawn in.
pub(crate) fn field_frame(cx: &App, invalid: bool) -> Div {
    div()
        .h(px(FIELD_HEIGHT))
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if invalid { "danger" } else { "border" }))
        .bg(theme::color(cx, "bg-input"))
}

/// A path on one line: the folders before its last part, then the last part, so that a long path
/// that is cut keeps its final folder in view.
pub(crate) fn path_line(cx: &App, path: &str) -> Div {
    let cut = path.rfind(['/', '\\']).unwrap_or(0);
    let (head, tail) = path.split_at(cut);
    div()
        .flex()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_color(theme::color(cx, "fg-strong"))
        .child(div().min_w_0().truncate().child(SharedString::from(head.to_owned())))
        .child(div().flex_none().child(SharedString::from(tail.to_owned())))
}
