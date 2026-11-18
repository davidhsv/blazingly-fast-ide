//! How the folder chooser is drawn: its title, the path row (Parent Folder, the path, Refresh),
//! the list of subfolders (only the rows in view), `Loading…` / `No subfolders` or the failed
//! listing with what can help, and OK / Cancel in the page's order.

use super::{Flavour, RemoteFolderBrowser};
use crate::settings_model::ssh_connection::SshTestRemedy;
use crate::settings_view::controls::{button, check_box, tx};
use crate::settings_view::pages::{MEDIUM_FIELD, WIDE_FIELD, frame, icon_button};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px, uniform_list,
};
use ide_core::IdeError;

/// The height of the list of each page's chooser, and the chooser's width.
const TARGET_LIST_HEIGHT: f32 = 260.0;
const DEPLOYMENT_LIST_HEIGHT: f32 = 240.0;
const WIDTH: f32 = 520.0;

impl RemoteFolderBrowser {
    /// The rows of `range`: a folder icon and the name, the selected one highlighted.
    fn rows(&self, range: std::ops::Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let Some(entries) = self.entries.as_ref() else { return Vec::new() };
        let prefix = self.flavour.selector();
        let (selected_bg, hover_bg) = (theme::color(cx, "bg-tree-selected"), theme::color(cx, "bg-tree-hover"));
        let icon_color = theme::color(cx, "fg-icon");
        let height = theme::length(cx, "h-list-row");
        range
            .filter_map(|index| entries.get(index).map(|entry| (index, entry)))
            .map(|(index, entry)| {
                let selector = format!("{prefix}-row-{index}");
                let path = entry.path.clone();
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(height)
                    .w_full()
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .when(self.selected == Some(index), |el| el.bg(selected_bg))
                    .when(self.selected != Some(index), |el| el.hover(|el| el.bg(hover_bg)))
                    .child(theme::icon(cx, "folder", 14.0, icon_color))
                    .child(SharedString::from(entry.name.clone()))
                    .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                        if view.flavour == Flavour::Deployment {
                            window.focus(&view.focus, cx);
                        }
                        if event.click_count() >= 2 {
                            view.go(path.clone(), cx);
                        } else {
                            view.select(index, cx);
                        }
                    }))
                    .into_any_element()
            })
            .collect()
    }

    /// `Loading…` (with a busy mark on Deployment's) or `No subfolders`, in the list's place.
    fn status(&self, text: &str, busy: bool, cx: &App) -> AnyElement {
        let subtle = theme::color(cx, "fg-subtle");
        let mark = (busy && self.flavour == Flavour::Deployment).then(|| theme::icon(cx, "refresh", 14.0, subtle));
        div()
            .flex()
            .items_center()
            .when(self.flavour == Flavour::Deployment, |el| el.justify_center().gap(px(8.0)).p(px(16.0)))
            .when(self.flavour == Flavour::Target, |el| el.p(px(8.0)))
            .text_color(subtle)
            .children(mark)
            .child(SharedString::from(tx(cx, text)))
            .into_any_element()
    }

    /// A text button drawn as `selector`, running `action` while it can be used.
    fn action(
        &self,
        selector: String,
        label: &str,
        primary: bool,
        enabled: bool,
        action: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text = tx(cx, label);
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector)
            .child(button(cx, &text, primary, enabled))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| action(view, cx))))
            .into_any_element()
    }

    /// A failed listing: the error, with Trust or the password field and Connect when one of them
    /// can help, and why the last of them failed.
    fn problem(&self, error: &IdeError, cx: &mut Context<Self>) -> AnyElement {
        let prefix = self.flavour.selector();
        let danger = theme::color(cx, "danger");
        let remedy = match (self.flavour, self.remedy()) {
            (Flavour::Target, Some(SshTestRemedy::TrustHostKey)) => {
                Some(self.action(format!("{prefix}-trust"), "Trust Host", false, true, Self::trust, cx))
            }
            (Flavour::Deployment, Some(SshTestRemedy::TrustHostKey)) => {
                let label = "Trust Host Key and Retry";
                Some(self.action(format!("{prefix}-trust"), label, false, !self.busy, Self::trust, cx))
            }
            (flavour, Some(SshTestRemedy::Secret)) => {
                let typed = !self.secret.read(cx).text(cx).is_empty();
                let deployment = flavour == Flavour::Deployment;
                let can = !deployment || (!self.busy && typed);
                let connect = self.action(format!("{prefix}-connect"), "Connect", false, can, Self::connect, cx);
                let secret_selector = flavour.secret_selector();
                let field = frame(cx, if deployment { MEDIUM_FIELD } else { 200.0 }, false)
                    .id(secret_selector)
                    .debug_selector(move || secret_selector.to_owned())
                    .child(self.secret.clone());
                let save = deployment.then(|| {
                    let selector = format!("{prefix}-save");
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector)
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(check_box(cx, self.save))
                        .child(SharedString::from(tx(cx, "Save")))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_save(cx)))
                });
                let row = div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(if deployment { 8.0 } else { 6.0 }))
                    .child(field)
                    .children(save)
                    .child(connect);
                Some(row.into_any_element())
            }
            (_, None) => None,
        };
        let failure =
            self.failure.as_ref().map(|failure| div().text_color(danger).child(SharedString::from(failure.clone())));
        div()
            .debug_selector(move || format!("{prefix}-error"))
            .p(px(6.0))
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(div().text_color(danger).child(SharedString::from(self.error_text(&error.message, cx))))
            .children(remedy)
            .children(failure)
            .into_any_element()
    }
}

impl Render for RemoteFolderBrowser {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let prefix = self.flavour.selector();
        let parent = self.flavour.parent(&self.path);
        let up = icon_button(cx, format!("{prefix}-up"), "arrow-up", "Parent Folder")
            .when(parent.is_none(), |el| el.opacity(0.5))
            .when_some(parent, |el, parent| {
                el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.go(parent.clone(), cx)))
            });
        let refresh = icon_button(cx, format!("{prefix}-refresh"), "refresh", "Refresh")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.retry(cx)));
        let path_selector = format!("{prefix}-path");
        let path = frame(cx, WIDE_FIELD, false)
            .id(SharedString::from(path_selector.clone()))
            .debug_selector(move || path_selector)
            .font_family("monospace")
            .child(self.path_field.clone());
        let body: AnyElement = match (&self.error, &self.entries) {
            (Some(error), _) => self.problem(error, cx),
            (None, None) => self.status("Loading…", true, cx),
            (None, Some(entries)) if entries.is_empty() => self.status("No subfolders", false, cx),
            (None, Some(entries)) => uniform_list(
                self.flavour.list_id(),
                entries.len(),
                cx.processor(|view: &mut Self, range: std::ops::Range<usize>, _, cx| view.rows(range, cx)),
            )
            .track_scroll(&self.scroll)
            .size_full()
            .into_any_element(),
        };
        let list_selector = format!("{prefix}-list");
        let list = div()
            .id(SharedString::from(list_selector.clone()))
            .debug_selector(move || list_selector)
            .when(self.flavour == Flavour::Deployment, |el| el.track_focus(&self.focus))
            .h(px(match self.flavour {
                Flavour::Target => TARGET_LIST_HEIGHT,
                Flavour::Deployment => DEPLOYMENT_LIST_HEIGHT,
            }))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .child(body);
        let can_choose = self.can_choose();
        let ok = self.action(format!("{prefix}-ok"), "OK", true, can_choose, Self::choose, cx);
        let cancel = self.action(format!("{prefix}-cancel"), "Cancel", false, true, Self::close, cx);
        let buttons = match self.flavour {
            Flavour::Target => div().flex().justify_end().gap(px(6.0)).child(ok).child(cancel),
            Flavour::Deployment => div().flex().justify_end().gap(px(6.0)).child(cancel).child(ok),
        };
        let title = tx(cx, self.flavour.title(&self.connection));
        div()
            .debug_selector(move || prefix.to_owned())
            .ml(px(4.0))
            .mt(px(4.0))
            .p(px(8.0))
            .max_w(px(WIDTH))
            .flex()
            .flex_col()
            .gap(px(if self.flavour == Flavour::Deployment { 8.0 } else { 6.0 }))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .bg(theme::color(cx, "bg-popup"))
            .child(div().text_color(theme::color(cx, "fg-strong")).child(SharedString::from(title)))
            .child(div().flex().items_center().gap(px(6.0)).child(up).child(path).child(refresh))
            .child(list)
            .child(buttons)
    }
}
