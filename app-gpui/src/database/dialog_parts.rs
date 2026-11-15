//! The parts the Database dialogs are drawn from: the application's shared dialog frame, title,
//! buttons, check boxes and field frame (`crate::app_views`, which follow the IDE zoom), plus what
//! only these dialogs have: a label column, the body and footer rows, and the read-only SQL preview
//! a dialog shows of what it runs.

use crate::app_views;
use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{App, Div, SharedString, div, prelude::*, px};

/// `text` in the UI language.
pub fn tx(cx: &mut App, text: &str) -> String {
    app_views::tx(cx, text)
}

/// `text` in the UI language, with the values of a template (`Import {n} Rows`) kept.
pub fn tx_template(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx_template(text)
}

/// The first font of the theme's `--font-mono` list that is installed: SQL is code.
pub fn code_font(cx: &App) -> SharedString {
    crate::theme::installed_fonts::mono_family(cx)
}

/// A dialog's frame, `width` wide, with its (translated) title; a click inside it is not a click
/// beside it.
pub fn dialog_frame(cx: &mut App, title: &str, width: f32) -> Div {
    let title = tx(cx, title);
    app_views::frame(cx, width).max_h(theme::zoomed(cx, 720.0)).child(app_views::title_row(cx, title).flex_none())
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
    app_views::button(cx, label, primary, enabled)
}

/// A check box with its label, greyed out when it cannot be changed.
pub fn checkbox(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let face = app_views::checkbox(cx, label, on).whitespace_nowrap();
    if enabled { face } else { face.opacity(0.5) }
}

/// The frame a text field is drawn in.
pub fn field_frame(cx: &App) -> Div {
    app_views::field_frame(cx, false)
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
