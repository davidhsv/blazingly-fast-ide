//! The Services tool window once a view besides the services tree has been used in it: a tab per
//! view in its header (Services, then HTTP Client), the selected view below, and the selected
//! view's header buttons at the right of the tabs.
//!
//! The views are the workbench's; this host only switches between them and forwards a header
//! button's press to the HTTP Client view, which reports it as its own event.

use super::response_view::{HeaderAction, HttpClientView};
use crate::platform::i18n::Localization;
use crate::theme;
use gpui_kit::{
    AnyView, App, ClickEvent, Context, Entity, FocusHandle, Focusable, IntoElement, Render, SharedString, Window, div,
    prelude::*, px,
};

/// A tab of the Services tool window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceTab {
    Services,
    HttpClient,
}

/// A tab as drawn: its title, icon and whether it is selected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabRow {
    pub tab: ServiceTab,
    pub title: &'static str,
    pub icon: &'static str,
    pub selected: bool,
}

pub struct ServiceTabs {
    services: AnyView,
    services_focus: FocusHandle,
    http: Entity<HttpClientView>,
    active: ServiceTab,
}

impl ServiceTabs {
    /// The services tree (`services`, whose keyboard focus is `services_focus`) and the HTTP Client
    /// view, with `active` shown.
    pub fn new(
        services: AnyView,
        services_focus: FocusHandle,
        http: Entity<HttpClientView>,
        active: ServiceTab,
    ) -> Self {
        Self { services, services_focus, http, active }
    }

    pub fn active(&self) -> ServiceTab {
        self.active
    }

    pub fn http(&self) -> &Entity<HttpClientView> {
        &self.http
    }

    pub fn select(&mut self, tab: ServiceTab, cx: &mut Context<Self>) {
        self.active = tab;
        cx.notify();
    }

    /// The tabs, in their order.
    pub fn tabs(&self) -> Vec<TabRow> {
        [
            (ServiceTab::Services, "Services", "toolwindow-services"),
            (ServiceTab::HttpClient, "HTTP Client", "file-http"),
        ]
        .into_iter()
        .map(|(tab, title, icon)| TabRow { tab, title, icon, selected: tab == self.active })
        .collect()
    }

    /// The header buttons of the selected view.
    pub fn header_actions(&self, cx: &App) -> Vec<HeaderAction> {
        match self.active {
            ServiceTab::Services => Vec::new(),
            ServiceTab::HttpClient => self.http.read(cx).header_actions(),
        }
    }
}

impl Focusable for ServiceTabs {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active {
            ServiceTab::Services => self.services_focus.clone(),
            ServiceTab::HttpClient => self.http.focus_handle(cx),
        }
    }
}

impl Render for ServiceTabs {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut header = div()
            .flex_none()
            .h(theme::length(cx, "h-header"))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        for row in self.tabs() {
            let tab = row.tab;
            let selector: &'static str = match tab {
                ServiceTab::Services => "service-tab-services",
                ServiceTab::HttpClient => "service-tab-http",
            };
            let title = cx.default_global::<Localization>().0.tx(row.title);
            header = header.child(
                div()
                    .id(selector)
                    .debug_selector(move || selector.to_owned())
                    .h(px(24.0))
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .rounded(px(4.0))
                    .text_color(theme::color(cx, if row.selected { "fg" } else { "fg-secondary" }))
                    .when(row.selected, |el| el.bg(theme::color(cx, "bg-active")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, row.icon, 14.0, theme::color(cx, "fg")))
                    .child(SharedString::from(title))
                    .on_click(cx.listener(move |host, _: &ClickEvent, _, cx| host.select(tab, cx))),
            );
        }
        header = header.child(div().flex_1());
        for action in self.header_actions(cx) {
            let id = action.id;
            let http = self.http.clone();
            let title = cx.default_global::<Localization>().0.tx(action.title);
            let color = theme::color(cx, if action.enabled { "fg" } else { "fg-faint" });
            header = header.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_owned())
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .when(action.enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .tooltip(crate::ui::tooltip::tooltip(title))
                    .child(theme::icon(cx, action.icon, 14.0, color))
                    .on_click(move |_: &ClickEvent, _, cx| http.update(cx, |view, cx| view.press(id, cx))),
            );
        }
        let body = match self.active {
            ServiceTab::Services => self.services.clone(),
            ServiceTab::HttpClient => self.http.clone().into(),
        };
        div().size_full().flex().flex_col().child(header).child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::super::response_view::{CLEAR_HISTORY, HttpClientViewEvent, STOP_REQUEST};
    use super::*;
    use crate::platform::input;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct Tree {
        focus: FocusHandle,
    }

    impl Render for Tree {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().debug_selector(|| "services-tree".to_owned())
        }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// The tabs switch between the services tree and the HTTP Client, whose header buttons show only
    /// while it is selected and reach the view.
    #[gpui_kit::test]
    async fn switches_between_the_services_and_the_http_client(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (host, cx) = cx.add_window_view(|_, cx| {
            let tree = cx.new(|cx| Tree { focus: cx.focus_handle() });
            let focus = tree.read(cx).focus.clone();
            let http = cx.new(HttpClientView::new);
            ServiceTabs::new(tree.into(), focus, http, ServiceTab::HttpClient)
        });
        let events = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&events);
        let http = host.read_with(cx, |host, _| host.http().clone());
        cx.update(|window, cx| {
            cx.subscribe(&http, move |_, event: &HttpClientViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        let titles: Vec<&str> = host.read_with(cx, |host, _| host.tabs().iter().map(|t| t.title).collect());
        assert_eq!(titles, ["Services", "HTTP Client"]);
        let ids: Vec<&str> = host.read_with(cx, |host, cx| host.header_actions(cx).iter().map(|a| a.id).collect());
        assert_eq!(ids.first().copied(), Some(STOP_REQUEST));
        click(cx, CLEAR_HISTORY);
        assert_eq!(*events.borrow(), [HttpClientViewEvent::ClearHistory]);
        click(cx, "service-tab-services");
        assert_eq!(host.read_with(cx, |host, _| host.active()), ServiceTab::Services);
        assert!(host.read_with(cx, |host, cx| host.header_actions(cx).is_empty()));
        assert!(cx.debug_bounds("services-tree").is_some());
        assert!(cx.debug_bounds(CLEAR_HISTORY).is_none());
    }
}
