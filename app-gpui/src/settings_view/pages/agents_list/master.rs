//! The master list of the Coding Agents page (the search, the rows, the custom-agent buttons, the
//! notice and the registry's line) and the question the page asks before it acts.

use super::{AgentListPageView, LIST_MIN_WIDTH, LIST_WIDTH, ROW_HEIGHT, SECTION_HEIGHT, with_i18n};
use crate::agents::agent_list::{ListRow, RegistryState};
use crate::settings_view::controls::{button, tx};
use crate::settings_view::pages::icon_button;
use crate::theme;
use gpui_kit::{AnyElement, ClickEvent, Context, IntoElement, SharedString, div, prelude::*, px};

impl AgentListPageView {
    /// The master list: the search, the rows, Add Custom Agent… and Import acp.json, the notice and the
    /// registry's line.
    pub(super) fn master(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.list.selected_id();
        let clear = (!self.list.query.is_empty()).then(|| {
            icon_button(cx, "settings-agents-search-clear".to_owned(), "close", "Clear").on_click(cx.listener(
                |view, _: &ClickEvent, _, cx| {
                    view.search.update(cx, |field, cx| field.set_text("", cx));
                    view.list.query.clear();
                    view.sync(cx);
                },
            ))
        });
        let search = div()
            .id("settings-agents-search")
            .debug_selector(|| "settings-agents-search".to_owned())
            .flex()
            .items_center()
            .gap(px(4.0))
            .m(px(2.0))
            .h(theme::length(cx, "h-control"))
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-input"))
            .child(theme::icon(cx, "search", 14.0, theme::color(cx, "fg-muted")))
            .child(div().flex_1().min_w_0().child(self.search.clone()))
            .children(clear);
        let rows: Vec<AnyElement> = self
            .rows(cx)
            .into_iter()
            .map(|row| match row {
                ListRow::Section(title) => div()
                    .h(px(SECTION_HEIGHT))
                    .px(px(8.0))
                    .pb(px(4.0))
                    .flex()
                    .items_end()
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(tx(cx, title)))
                    .into_any_element(),
                ListRow::Agent(item) => {
                    let selector = format!("settings-agents-row-{}", item.id);
                    let chosen = selected.as_deref() == Some(item.id.as_str());
                    let id = item.id.clone();
                    // While it installs, a mark in place of the tag.
                    let trailing = if item.working {
                        Some(div().flex_none().text_color(theme::color(cx, "fg-subtle")).child("…"))
                    } else {
                        item.tag.map(|tag| {
                            div()
                                .flex_none()
                                .text_size(theme::length(cx, "fs-small"))
                                .text_color(theme::color(cx, if tag.accent { "accent" } else { "fg-subtle" }))
                                .child(SharedString::from(tag.label))
                        })
                    };
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector)
                        .h(px(ROW_HEIGHT))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .when(chosen, |el| el.bg(theme::color(cx, "bg-selected")))
                        .when(!chosen, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                        .child(theme::icon(cx, &item.icon, 16.0, theme::color(cx, "fg-icon")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_color(theme::color(cx, if item.muted { "fg-muted" } else { "fg" }))
                                .child(SharedString::from(item.name)),
                        )
                        .children(trailing)
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            window.focus(&view.list_focus, cx);
                            view.select(&id, cx);
                        }))
                        .into_any_element()
                }
            })
            .collect();
        let empty = self.list.nothing_matches().then(|| {
            div()
                .debug_selector(|| "settings-agents-empty".to_owned())
                .p(px(16.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(tx(cx, "No agents match")))
        });
        let list = div()
            .id("settings-agents-list")
            .track_focus(&self.list_focus)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(4.0))
            .pb(px(4.0))
            .flex()
            .flex_col()
            .children(rows)
            .children(empty);
        let custom = self.backend.custom.as_ref().map(|_| {
            let enabled = !self.list.busy();
            let import_enabled = enabled && !self.list.importing();
            let import = if self.list.importing() { "Importing…" } else { "Import acp.json" };
            let add = div()
                .id("settings-agents-add-custom")
                .debug_selector(|| "settings-agents-add-custom".to_owned())
                .child(button(cx, &tx(cx, "Add Custom Agent…"), false, enabled))
                .when(enabled, |el| {
                    el.on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.add_custom(window, cx)))
                });
            let import = div()
                .id("settings-agents-import")
                .debug_selector(|| "settings-agents-import".to_owned())
                .child(button(cx, &tx(cx, import), false, import_enabled))
                .when(import_enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.import(cx))));
            div().flex().flex_wrap().gap(px(6.0)).px(px(6.0)).child(add).child(import)
        });
        let notice = self.list.notice().map(|notice| {
            div()
                .debug_selector(|| "settings-agents-notice".to_owned())
                .px(px(8.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(notice.to_owned()))
        });
        let state = with_i18n(cx, |i18n| self.list.registry_state(i18n)).map(|state| {
            let (text, color) = match state {
                RegistryState::Loading => (tx(cx, "Loading the ACP Registry…"), "fg-muted"),
                RegistryState::Warning(text) => (text, "warning"),
                RegistryState::Error(text) => (text, "danger"),
            };
            div()
                .debug_selector(|| "settings-agents-registry-state".to_owned())
                .px(px(12.0))
                .py(px(6.0))
                .border_t_1()
                .border_color(theme::color(cx, "border"))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, color))
                .child(SharedString::from(text))
        });
        div()
            .flex_none()
            .w(px(LIST_WIDTH))
            .min_w(px(LIST_MIN_WIDTH))
            .py(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(search)
            .child(list)
            .children(custom)
            .children(notice)
            .children(state)
            .into_any_element()
    }

    /// The question the page asks, over the page.
    pub(super) fn confirm_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let request = self.list.pending()?.request().clone();
        let ok = div()
            .id("settings-agents-confirm-ok")
            .debug_selector(|| "settings-agents-confirm-ok".to_owned())
            .child(button(cx, &request.ok, true, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer(true, cx)));
        let cancel = div()
            .id("settings-agents-confirm-cancel")
            .debug_selector(|| "settings-agents-confirm-cancel".to_owned())
            .child(button(cx, &tx(cx, "Cancel"), false, true))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.answer(false, cx)));
        let panel = div()
            .w(px(440.0))
            .flex()
            .flex_col()
            .gap(px(10.0))
            .p(px(16.0))
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-dialog"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .child(theme::icon(cx, "warning", 16.0, theme::color(cx, "warning")))
                    .child(SharedString::from(request.title)),
            )
            .child(div().child(SharedString::from(request.message)))
            .child(div().flex().justify_end().gap(px(8.0)).child(ok).child(cancel));
        Some(overlay("settings-agents-confirm", panel.into_any_element()))
    }
}

/// A dialog drawn over the whole page, which takes the mouse from what is under it.
pub(super) fn overlay(id: &'static str, panel: AnyElement) -> AnyElement {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .child(panel)
        .into_any_element()
}
