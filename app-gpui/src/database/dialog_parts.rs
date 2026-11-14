//! The parts the Database dialogs are drawn from: the frame with its title, buttons, check boxes,
//! the frame of a text field, and the read-only SQL preview a dialog shows of what it runs.

use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{App, Div, FontWeight, MouseButton, SharedString, div, prelude::*, px};

const BUTTON_HEIGHT: f32 = 28.0;
const FIELD_HEIGHT: f32 = 28.0;

/// `text` in the UI language.
pub fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// `text` in the UI language, with the values of a template (`Import {n} Rows`) kept.
pub fn tx_template(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx_template(text)
}

/// The first font of the theme's `--font-mono` list that is installed: SQL is code.
pub fn code_font(cx: &App) -> SharedString {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|active| active.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family = families.into_iter().find(|family| installed.contains(family)).unwrap_or_else(|| "Menlo".to_owned());
    SharedString::from(family)
}

/// A dialog's frame, `width` wide, with its (translated) title; a click inside it is not a click
/// beside it.
pub fn dialog_frame(cx: &mut App, title: &str, width: f32) -> Div {
    let title = tx(cx, title);
    div()
        .w(px(width))
        .max_h(px(720.0))
        .flex()
        .flex_col()
        .bg(theme::color(cx, "bg-popup"))
        .border_1()
        .border_color(theme::color(cx, "border-popup"))
        .rounded(theme::length(cx, "radius-popup"))
        .text_size(theme::length(cx, "fs-ui"))
        .text_color(theme::color(cx, "fg"))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex_none()
                .px(px(16.0))
                .pt(px(14.0))
                .pb(px(8.0))
                .text_color(theme::color(cx, "fg-strong"))
                .font_weight(FontWeight::SEMIBOLD)
                .child(SharedString::from(title)),
        )
}

/// The dialog's body: its rows one under the other.
pub fn dialog_body() -> Div {
    div().flex_1().min_h_0().px(px(16.0)).py(px(4.0)).flex().flex_col().gap(px(10.0))
}

/// The row of buttons at the bottom right of a dialog.
pub fn dialog_footer() -> Div {
    div().flex_none().flex().items_center().justify_end().gap(px(8.0)).px(px(16.0)).py(px(12.0))
}

/// A button: filled when `primary`, outlined otherwise, greyed out when it cannot be pressed.
pub fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
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

/// A check box with its label, greyed out when it cannot be changed.
pub fn checkbox(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    let face = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
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
        .child(SharedString::from(label));
    if enabled { face } else { face.opacity(0.5) }
}

/// The frame a text field is drawn in.
pub fn field_frame(cx: &App) -> Div {
    div()
        .h(px(FIELD_HEIGHT))
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, "border"))
        .bg(theme::color(cx, "bg-input"))
}

/// A label in front of a field.
pub fn label(cx: &mut App, text: &str) -> Div {
    let text = tx(cx, text);
    div().flex_none().w(px(96.0)).text_color(theme::color(cx, "fg-muted")).child(SharedString::from(text))
}

/// The read-only SQL a dialog runs, under its (translated) caption, one line per row.
pub fn sql_preview(cx: &mut App, caption: &str, sql: &str) -> Div {
    let caption = tx(cx, caption);
    let font = code_font(cx);
    let lines: Vec<Div> = sql
        .lines()
        .map(|line| div().min_h(px(16.0)).whitespace_nowrap().child(SharedString::from(line.to_owned())))
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(caption)))
        .child(
            div()
                .debug_selector(|| "db-sql-preview".to_owned())
                .max_h(px(180.0))
                .overflow_hidden()
                .p(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border"))
                .bg(theme::color(cx, "bg-editor"))
                .font_family(font)
                .text_size(theme::length(cx, "fs-small"))
                .children(lines),
        )
}
