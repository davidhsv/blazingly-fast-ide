//! The drawing of the Live Templates page: the two options, the list with its toolbar and filter,
//! and the editor of the selected template.

use super::super::{description_cell, hint_flow, icon_button, row};
use super::{
    ADD, EMPTY, FUNCTIONS, IN_COMPLETION, ListRow, LiveTemplatesPageView, REMOVE, RESTORE, SCOPE, WITH_TAB,
    text_variables,
};
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::templates::live_builtins::is_builtin_id;
use crate::templates::live_model::{LiveTemplate, TemplateScope};
use crate::theme;
use crate::ui::text_field::TextField;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, IntoElement, Render, SharedString, Window, div, prelude::*, px,
    uniform_list,
};
use std::ops::Range;

impl LiveTemplatesPageView {
    fn option_row(
        &self,
        selector: &'static str,
        label: &str,
        on: bool,
        flip: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.binding.query(cx);
        let text = tx(cx, label);
        row(cx, self.binding.matches(label, cx))
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .child(check_box(cx, on))
            .child(search_label(cx, &text, &query))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| flip(view, cx)))
            .into_any_element()
    }

    fn item(&self, template: &LiveTemplate, cx: &mut Context<Self>) -> AnyElement {
        let id = template.id.clone();
        let selector = format!("settings-live-item-{id}");
        let enable = format!("settings-live-enabled-{id}");
        let chosen = self.selected.as_deref() == Some(id.as_str());
        let locked = template.plugin_origin.is_some();
        let on = template.enabled != Some(false);
        let label = if !on {
            "fg-faint"
        } else if self.store.is_modified_builtin(&id) {
            "link"
        } else {
            "fg-emphasis"
        };
        let toggled = id.clone();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .w_full()
            .h(theme::length(cx, "h-row"))
            .pl(px(18.0))
            .pr(px(8.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .whitespace_nowrap()
            .overflow_hidden()
            .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")).text_color(theme::color(cx, "fg-strong")))
            .when(!chosen, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
            .child(
                div()
                    .id(SharedString::from(enable.clone()))
                    .debug_selector(move || enable)
                    .when(locked, |el| el.opacity(0.5))
                    .child(check_box(cx, on))
                    .when(!locked, |el| {
                        el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_enabled(&toggled, cx)))
                    }),
            )
            .child(
                div()
                    .font_family(super::super::code_family(cx))
                    .text_color(theme::color(cx, label))
                    .child(SharedString::from(template.abbrev.clone())),
            )
            .child(description_cell(cx, format!("settings-live-detail-{id}"), &template.description))
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select(&id, cx)))
            .into_any_element()
    }

    /// A group's header in the list.
    fn group_header(cx: &App, group: &str) -> AnyElement {
        div()
            .w_full()
            .h(theme::length(cx, "h-row"))
            .px(px(8.0))
            .flex()
            .items_center()
            .text_size(theme::length(cx, "fs-small"))
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(group.to_owned()))
            .into_any_element()
    }

    fn side(&self, cx: &mut Context<Self>) -> AnyElement {
        let removable = self.can_remove();
        // Only the rows in view are drawn: the list holds every built-in template.
        let items = uniform_list(
            "settings-live-items",
            self.rows.len(),
            cx.processor(|view, range: Range<usize>, _, cx| {
                let templates = view.store.templates();
                view.rows[range]
                    .iter()
                    .map(|row| match row {
                        ListRow::Group(group) => Self::group_header(cx, group),
                        ListRow::Template(at) => view.item(&templates[*at], cx),
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.list_scroll)
        .flex_1()
        .min_h_0()
        .py(px(2.0));
        let add = icon_button(cx, "settings-live-add".to_owned(), "plus", ADD)
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(cx)));
        let remove = icon_button(cx, "settings-live-remove".to_owned(), "minus", REMOVE)
            .when(!removable, |el| el.opacity(0.5))
            .when(removable, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        div()
            .w(px(240.0))
            .flex_none()
            .flex()
            .flex_col()
            .min_h_0()
            .border_r_1()
            .border_color(theme::color(cx, "border-strong"))
            .child(
                div()
                    .h(theme::length(cx, "h-header"))
                    .px(px(4.0))
                    .flex()
                    .items_center()
                    .gap(px(2.0))
                    .border_b_1()
                    .border_color(theme::color(cx, "border"))
                    .child(add)
                    .child(remove)
                    .child(
                        div()
                            .id("settings-live-filter")
                            .debug_selector(|| "settings-live-filter".to_owned())
                            .flex_1()
                            .min_w_0()
                            .child(self.filter.clone()),
                    ),
            )
            .child(items)
            .into_any_element()
    }

    /// A row of the editor: its label, then `control` drawn as `selector`.
    fn labelled(cx: &App, label: &str, selector: &'static str, control: AnyElement) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .w(px(92.0))
                    .flex_none()
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, label))),
            )
            .child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .flex_1()
                    .min_w_0()
                    .h(theme::length(cx, "h-control"))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .bg(theme::color(cx, "bg-input"))
                    .child(control),
            )
            .into_any_element()
    }

    /// A help line: plain text and code, the plain parts in the UI language.
    fn hint(cx: &App, parts: &[(bool, &str)]) -> AnyElement {
        let parts: Vec<(bool, String)> =
            parts.iter().map(|&(code, text)| (code, if code { text.to_owned() } else { tx(cx, text) })).collect();
        hint_flow(cx, &parts)
    }

    fn editor(&self, cx: &mut Context<Self>) -> AnyElement {
        let (Some(template), Some(fields)) = (self.selected_template().cloned(), self.editor.as_ref()) else {
            return div()
                .p(px(20.0))
                .flex()
                .justify_center()
                .text_color(theme::color(cx, "fg-faint"))
                .child(SharedString::from(tx(cx, EMPTY)))
                .into_any_element();
        };
        let locked = template.plugin_origin.is_some();
        let mark = |on: bool, cx: &App| check_box(cx, on).when(locked, |el| el.opacity(0.5));
        let field = |field: &Entity<TextField>| field.clone().into_any_element();
        let mut rows = vec![
            Self::labelled(cx, "Abbreviation", "settings-live-abbreviation", field(&fields.abbreviation)),
            Self::labelled(cx, "Description", "settings-live-description", field(&fields.description)),
            Self::labelled(cx, "Group", "settings-live-group", field(&fields.group)),
            div()
                .id("settings-live-body")
                .debug_selector(|| "settings-live-body".to_owned())
                .py(px(6.0))
                .px(px(8.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-strong"))
                .bg(theme::color(cx, "bg-app"))
                .text_color(theme::color(cx, "fg-emphasis"))
                .child(fields.body.clone())
                .into_any_element(),
            Self::hint(
                cx,
                &[
                    (true, "$NAME$"),
                    (false, "variables (repeated ones are linked),"),
                    (true, "$END$"),
                    (false, "caret,"),
                    (true, "$SELECTION$"),
                    (false, "surrounded text,"),
                    (true, "$$"),
                    (false, "a dollar sign. Leading tabs are indent units."),
                ],
            ),
            Self::labelled(cx, "Contexts", "settings-live-contexts", field(&fields.contexts)),
            div()
                .id("settings-live-scope")
                .debug_selector(|| "settings-live-scope".to_owned())
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(mark(template.scope == Some(TemplateScope::Any), cx))
                .child(SharedString::from(tx(cx, SCOPE)))
                .when(!locked, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_scope(cx))))
                .into_any_element(),
        ];
        let variables = text_variables(&template);
        if !variables.is_empty() {
            let cell = |cx: &App, text: String| {
                let size = theme::length(cx, "fs-small");
                div().flex_1().min_w_0().px(px(4.0)).py(px(2.0)).text_size(size).child(SharedString::from(text))
            };
            let header = div()
                .flex()
                .text_color(theme::color(cx, "fg-muted"))
                .font_weight(gpui_kit::FontWeight::MEDIUM)
                .child(cell(cx, tx(cx, "Variable")))
                .child(cell(cx, tx(cx, "Expression")))
                .child(cell(cx, tx(cx, "Default value")))
                .child(cell(cx, tx(cx, "Skip if defined")));
            rows.push(header.into_any_element());
            for (variable, row_fields) in variables.iter().zip(&fields.variables) {
                let name = variable.name.clone();
                let skip = format!("settings-live-skip-{name}");
                let input = |field: &Entity<TextField>, cx: &App| {
                    div()
                        .flex_1()
                        .min_w_0()
                        .h(theme::length(cx, "h-control"))
                        .mx(px(4.0))
                        .px(px(6.0))
                        .flex()
                        .items_center()
                        .rounded(theme::length(cx, "radius-small"))
                        .border_1()
                        .border_color(theme::color(cx, "border-strong"))
                        .bg(theme::color(cx, "bg-input"))
                        .font_family(super::super::code_family(cx))
                        .child(field.clone())
                };
                rows.push(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .px(px(4.0))
                                .font_family(super::super::code_family(cx))
                                .text_color(theme::color(cx, "fg-emphasis"))
                                .whitespace_nowrap()
                                .child(SharedString::from(name.clone())),
                        )
                        .child(input(&row_fields.expression, cx))
                        .child(input(&row_fields.default_value, cx))
                        .child(
                            div().flex_1().px(px(4.0)).child(
                                div()
                                    .id(SharedString::from(skip.clone()))
                                    .debug_selector(move || skip)
                                    .child(mark(variable.skip_if_defined, cx))
                                    .when(!locked, |el| {
                                        el.on_click(
                                            cx.listener(move |view, _: &ClickEvent, _, cx| view.toggle_skip(&name, cx)),
                                        )
                                    }),
                            ),
                        )
                        .into_any_element(),
                );
            }
        }
        let mut functions: Vec<(bool, &str)> = vec![(false, "Functions:")];
        functions.extend(FUNCTIONS.iter().map(|function| (true, *function)));
        rows.push(Self::hint(cx, &functions));
        if is_builtin_id(&template.id) && self.store.is_modified_builtin(&template.id) {
            let label = tx(cx, RESTORE);
            rows.push(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("settings-live-restore")
                            .debug_selector(|| "settings-live-restore".to_owned())
                            .child(button(cx, &label, false, true))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.restore_default(cx))),
                    )
                    .into_any_element(),
            );
        }
        div().flex().flex_col().gap(px(8.0)).when(locked, |el| el.opacity(0.6)).children(rows).into_any_element()
    }
}

impl Render for LiveTemplatesPageView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options;
        let with_tab = self.option_row(
            "settings-live-expandWithTab",
            WITH_TAB,
            options.expand_with_tab,
            Self::toggle_expand_with_tab,
            cx,
        );
        let in_completion = self.option_row(
            "settings-live-showInCompletion",
            IN_COMPLETION,
            options.show_in_completion,
            Self::toggle_show_in_completion,
            cx,
        );
        let side = self.side(cx);
        let editor = self.editor(cx);
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .text_color(theme::color(cx, "fg"))
            .child(div().flex().flex_wrap().gap_x(px(18.0)).child(with_tab).child(in_completion))
            .child(
                div()
                    .flex_1()
                    .min_w(px(520.0))
                    .min_h(px(320.0))
                    .flex()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme::color(cx, "border-strong"))
                    .rounded(theme::length(cx, "radius-small"))
                    .child(side)
                    .child(
                        div()
                            .id("settings-live-editor")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .py(px(10.0))
                            .px(px(12.0))
                            .child(editor),
                    ),
            )
    }
}
