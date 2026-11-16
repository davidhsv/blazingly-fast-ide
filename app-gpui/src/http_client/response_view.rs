//! The HTTP Client view: the response history on the left (a row per exchange: status, method,
//! name or URL, time; the request being sent above them) and the selected exchange on the right:
//! its status line with version, duration and size, then the Response (pretty-printed JSON, XML or
//! HTML, or raw), Headers and Request tabs.
//!
//! The view owns no backend: the window hands it the history, the selection, the selected exchange
//! once loaded and the running request, and hears through [`HttpClientViewEvent`] what to do. Its
//! header actions (Stop Request, Reload History, Clear History) are drawn by the Services tabs
//! while the view is shown there, through [`HttpClientView::header_actions`] and
//! [`HttpClientView::press`].

use crate::features_a::http_client::format::{BodyKind, body_kind, format_ms, format_size, pretty_body};
use crate::platform::date_format::CivilDateTime;
use crate::platform::i18n::{Localization, Param};
use crate::platform::local_time::utc_offset_minutes;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, Hsla, IntoElement, Render, SharedString,
    Stateful, Window, div, prelude::*, px, relative,
};
use ide_http::{HistoryEntry, HttpExchange};

/// What the view asks of the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpClientViewEvent {
    /// History entry `id` was clicked: select it (and load its exchange).
    Select(String),
    /// Stop Request: cancel the request being sent.
    Stop,
    /// Reload History.
    ReloadHistory,
    /// Clear History.
    ClearHistory,
}

/// A tab of the selected exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExchangeTab {
    Body,
    Headers,
    Request,
}

/// How a status is coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusTone {
    Ok,
    Redirect,
    Error,
}

impl StatusTone {
    /// No status (the request failed) and 4xx / 5xx are errors, 3xx a redirect, the rest fine.
    pub fn of(status: Option<u16>) -> Self {
        match status {
            None => Self::Error,
            Some(code) if code >= 400 => Self::Error,
            Some(code) if code >= 300 => Self::Redirect,
            Some(_) => Self::Ok,
        }
    }

    fn color(self, cx: &App) -> Hsla {
        theme::color(
            cx,
            match self {
                Self::Ok => "success",
                Self::Redirect => "warning",
                Self::Error => "danger",
            },
        )
    }
}

/// A row of the history as drawn. Its URL is the one sent, variables substituted, so the row is
/// kept out of `Debug` output.
#[derive(Clone, PartialEq, Eq)]
pub struct HistoryRow {
    pub id: String,
    /// The status code, `ERR` for a request that got no response.
    pub status: String,
    pub tone: StatusTone,
    pub method: String,
    /// The request's name, else its URL.
    pub label: String,
    /// The error, else the URL.
    pub tooltip: String,
    /// The local time the request was sent, as the clock shows it.
    pub time: String,
    pub selected: bool,
}

/// What the status line of the selected exchange shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusLine {
    /// `200 OK`, the HTTP version, the total time (its tooltip the time to the headers), the size.
    Response { status: String, tone: StatusTone, version: String, total: String, ttfb: String, size: String },
    /// No response: the error, else "No response".
    Failed(String),
}

/// What the Response tab shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyShown {
    /// The request got no response: the error.
    Error(String),
    /// A body that is not text: its content type (`None`: unknown) and size.
    Binary { content_type: Option<String>, size: String },
    /// The body (formatted unless raw is on) and, when it was cut, the size received.
    Text { text: String, truncated: Option<String> },
}

/// A button of the view's header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeaderAction {
    pub id: &'static str,
    pub icon: &'static str,
    pub title: &'static str,
    pub enabled: bool,
}

pub const STOP_REQUEST: &str = "http-stop-request";
pub const RELOAD_HISTORY: &str = "http-reload-history";
pub const CLEAR_HISTORY: &str = "http-clear-history";

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

/// The local time of Unix time `ms` as an English clock shows it, seconds included: `3:04:05 PM`.
pub fn clock_time(ms: f64) -> String {
    let local = CivilDateTime::from_unix_ms(ms, utc_offset_minutes(ms));
    let seconds = ((ms / 1000.0).floor() as i64).rem_euclid(60);
    let hour = match local.hour % 12 {
        0 => 12,
        hour => hour,
    };
    let half = if local.hour < 12 { "AM" } else { "PM" };
    format!("{hour}:{:02}:{seconds:02} {half}", local.minute)
}

pub struct HttpClientView {
    history: Vec<HistoryEntry>,
    selected: Option<String>,
    exchange: Option<HttpExchange>,
    /// The 0-based index of the request being sent.
    running: Option<u32>,
    tab: ExchangeTab,
    raw: bool,
    focus: FocusHandle,
}

impl EventEmitter<HttpClientViewEvent> for HttpClientView {}

impl HttpClientView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            history: Vec::new(),
            selected: None,
            exchange: None,
            running: None,
            tab: ExchangeTab::Body,
            raw: false,
            focus: cx.focus_handle(),
        }
    }

    /// What to show: the history (newest first), the selected entry, its exchange once loaded and
    /// the index of the request being sent. Another exchange opens on its Response tab, formatted.
    pub fn set_state(
        &mut self,
        history: Vec<HistoryEntry>,
        selected: Option<String>,
        exchange: Option<HttpExchange>,
        running: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        let shown = self.exchange.as_ref().map(|ex| ex.entry.id.as_str());
        if shown != exchange.as_ref().map(|ex| ex.entry.id.as_str()) {
            self.tab = ExchangeTab::Body;
            self.raw = false;
        }
        self.history = history;
        self.selected = selected;
        self.exchange = exchange;
        self.running = running;
        cx.notify();
    }

    /// The history's rows.
    pub fn rows(&self) -> Vec<HistoryRow> {
        self.history
            .iter()
            .map(|entry| HistoryRow {
                id: entry.id.clone(),
                status: entry.status.map_or_else(|| "ERR".to_owned(), |code| code.to_string()),
                tone: StatusTone::of(entry.status),
                method: entry.method.clone(),
                label: entry.name.clone().unwrap_or_else(|| entry.url.clone()),
                tooltip: entry.error.clone().unwrap_or_else(|| entry.url.clone()),
                time: clock_time(entry.time_ms),
                selected: self.selected.as_deref() == Some(entry.id.as_str()),
            })
            .collect()
    }

    /// The running row's text: `Sending request #2…`.
    pub fn running_text(&self, cx: &mut App) -> Option<String> {
        let index = self.running?;
        let number = Param::Number(f64::from(index) + 1.0);
        Some(cx.default_global::<Localization>().0.tx_built(&["Sending request #", "…"], &[number]))
    }

    /// Whether the list shows "No requests yet" (nothing listed, nothing running).
    pub fn is_empty(&self) -> bool {
        self.history.is_empty() && self.running.is_none()
    }

    /// Whether an entry is selected while its exchange is still loading.
    pub fn is_loading(&self) -> bool {
        self.selected.is_some() && self.exchange.is_none()
    }

    pub fn tab(&self) -> ExchangeTab {
        self.tab
    }

    /// The tab labels: Response, Headers (with their count when there is a response), Request.
    pub fn tabs(&self, cx: &mut App) -> Vec<String> {
        let count = self.exchange.as_ref().and_then(|ex| ex.response.as_ref()).map(|r| r.headers.len());
        let headers = match count {
            Some(count) => {
                cx.default_global::<Localization>().0.tx_built(&["Headers (", ")"], &[Param::Number(count as f64)])
            }
            None => tx(cx, "Headers"),
        };
        vec![tx(cx, "Response"), headers, tx(cx, "Request")]
    }

    pub fn select_tab(&mut self, tab: ExchangeTab, cx: &mut Context<Self>) {
        self.tab = tab;
        cx.notify();
    }

    fn kind(&self) -> BodyKind {
        let response = self.exchange.as_ref().and_then(|ex| ex.response.as_ref());
        match response {
            Some(r) => r.body.as_deref().map_or(BodyKind::Text, |body| body_kind(r.content_type.as_deref(), body)),
            None => BodyKind::Text,
        }
    }

    /// Whether Show Raw / Show Formatted is offered (on the Response tab of a formatted kind), and
    /// whether it is pressed.
    pub fn raw_toggle(&self) -> Option<bool> {
        (self.exchange.is_some() && self.tab == ExchangeTab::Body && self.kind() != BodyKind::Text).then_some(self.raw)
    }

    pub fn toggle_raw(&mut self, cx: &mut Context<Self>) {
        if self.raw_toggle().is_some() {
            self.raw = !self.raw;
            cx.notify();
        }
    }

    pub fn status_line(&self) -> Option<StatusLine> {
        let ex = self.exchange.as_ref()?;
        Some(match &ex.response {
            Some(r) => StatusLine::Response {
                status: format!("{} {}", r.status, r.status_text),
                tone: StatusTone::of(Some(r.status)),
                version: r.version.clone(),
                total: format_ms(Some(r.total_ms)),
                ttfb: format_ms(Some(r.ttfb_ms)),
                size: format_size(Some(r.size as u64)),
            },
            None => StatusLine::Failed(ex.entry.error.clone().unwrap_or_else(|| "No response".to_owned())),
        })
    }

    /// The request line of the status line: `GET http://…`.
    pub fn request_line(&self) -> Option<String> {
        self.exchange.as_ref().map(|ex| format!("{} {}", ex.request.method, ex.request.url))
    }

    pub fn body(&self) -> Option<BodyShown> {
        let ex = self.exchange.as_ref()?;
        let Some(r) = &ex.response else { return Some(BodyShown::Error(ex.entry.error.clone().unwrap_or_default())) };
        let size = format_size(Some(r.size as u64));
        Some(match &r.body {
            None => BodyShown::Binary { content_type: r.content_type.clone(), size },
            Some(body) => BodyShown::Text {
                text: if self.raw { body.clone() } else { pretty_body(self.kind(), body) },
                truncated: r.truncated.then_some(size),
            },
        })
    }

    /// The response headers, as received.
    pub fn header_rows(&self) -> Vec<(String, String)> {
        let response = self.exchange.as_ref().and_then(|ex| ex.response.as_ref());
        response.map(|r| r.headers.clone()).unwrap_or_default()
    }

    /// The Request tab: the request line, its headers, a blank line and its body.
    pub fn request_text(&self) -> Option<String> {
        let request = &self.exchange.as_ref()?.request;
        let mut text = format!("{} {}\n", request.method, request.url);
        for (name, value) in &request.headers {
            text.push_str(&format!("{name}: {value}\n"));
        }
        if let Some(body) = &request.body {
            text.push('\n');
            text.push_str(body);
        }
        Some(text)
    }

    /// The header's buttons: Stop Request while a request is sent, Reload History, Clear History.
    pub fn header_actions(&self) -> Vec<HeaderAction> {
        vec![
            HeaderAction { id: STOP_REQUEST, icon: "stop", title: "Stop Request", enabled: self.running.is_some() },
            HeaderAction { id: RELOAD_HISTORY, icon: "refresh", title: "Reload History", enabled: true },
            HeaderAction { id: CLEAR_HISTORY, icon: "trash", title: "Clear History", enabled: true },
        ]
    }

    /// A header button was pressed.
    pub fn press(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.header_actions().iter().any(|action| action.id == id && action.enabled) {
            return;
        }
        match id {
            STOP_REQUEST => cx.emit(HttpClientViewEvent::Stop),
            RELOAD_HISTORY => cx.emit(HttpClientViewEvent::ReloadHistory),
            CLEAR_HISTORY => cx.emit(HttpClientViewEvent::ClearHistory),
            _ => {}
        }
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn entry_row(&self, index: usize, row: HistoryRow, cx: &mut Context<Self>) -> Stateful<Div> {
        let selector = format!("http-entry-{index}");
        let id = row.id.clone();
        div()
            .id(SharedString::from(selector.clone()))
            .debug_selector(move || selector.clone())
            .flex_none()
            .h(theme::length(cx, "h-list-row"))
            .px(px(8.0))
            .flex()
            .items_center()
            .gap(px(8.0))
            .text_color(theme::color(cx, "fg"))
            .when(row.selected, |el| el.bg(theme::color(cx, "bg-tree-selected")))
            .when(!row.selected, |el| el.hover(|el| el.bg(theme::color(cx, "bg-tree-hover"))))
            .tooltip(crate::ui::tooltip::tooltip(row.tooltip.clone()))
            .child(
                div()
                    .flex_none()
                    .font_family(theme::installed_fonts::mono_family(cx))
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(row.tone.color(cx))
                    .child(SharedString::from(row.status)),
            )
            .child(
                div()
                    .flex_none()
                    .font_family(theme::installed_fonts::mono_family(cx))
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "syn-keyword"))
                    .child(SharedString::from(row.method)),
            )
            .child(div().flex_1().min_w_0().truncate().child(SharedString::from(row.label)))
            .child(
                div()
                    .flex_none()
                    .text_size(theme::length(cx, "fs-small"))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(row.time)),
            )
            .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| cx.emit(HttpClientViewEvent::Select(id.clone()))))
    }

    fn empty_state(title: &str, body: Option<String>, cx: &mut Context<Self>) -> Div {
        let title = tx(cx, title);
        div()
            .size_full()
            .p(px(16.0))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .child(div().text_color(theme::color(cx, "fg-secondary")).child(SharedString::from(title)))
            .children(body.map(|body| {
                div().text_size(theme::length(cx, "fs-small")).text_color(theme::color(cx, "fg-muted")).child(body)
            }))
    }

    fn history_list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let running = self.running_text(cx).map(|text| {
            div()
                .debug_selector(|| "http-running".to_owned())
                .flex_none()
                .h(theme::length(cx, "h-list-row"))
                .px(px(8.0))
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_color(theme::color(cx, "fg-secondary"))
                .child(theme::icon(cx, "refresh", 12.0, theme::color(cx, "fg-secondary")))
                .child(SharedString::from(text))
        });
        let list = div().id("http-history").size_full().overflow_y_scroll().flex().flex_col().children(running);
        if self.is_empty() {
            let body = tx(cx, "Run a request from an .http file with the gutter ▶ or ⌘↩.");
            return list.child(Self::empty_state("No requests yet", Some(body), cx));
        }
        let rows: Vec<Stateful<Div>> =
            self.rows().into_iter().enumerate().map(|(index, row)| self.entry_row(index, row, cx)).collect();
        list.children(rows)
    }

    fn mono(cx: &App) -> Div {
        div().font_family(theme::installed_fonts::mono_family(cx)).text_size(theme::length(cx, "fs-small"))
    }

    fn status_row(&self, cx: &mut Context<Self>) -> Div {
        let row = div().flex_none().h(theme::length(cx, "h-header")).px(px(8.0)).flex().items_center().gap(px(12.0));
        fn meta(text: String, cx: &App) -> Div {
            div().text_size(theme::length(cx, "fs-small")).text_color(theme::color(cx, "fg-secondary")).child(text)
        }
        let row = match self.status_line() {
            Some(StatusLine::Response { status, tone, version, total, ttfb, size }) => {
                let headers_after = format!("Headers after {ttfb}");
                row.child(
                    Self::mono(cx)
                        .flex_none()
                        .debug_selector(|| "http-status".to_owned())
                        .text_color(tone.color(cx))
                        .child(SharedString::from(status)),
                )
                .child(meta(version, cx))
                .child(
                    div().id("http-total").tooltip(crate::ui::tooltip::tooltip(headers_after)).child(meta(total, cx)),
                )
                .child(meta(size, cx))
            }
            Some(StatusLine::Failed(error)) => row.child(
                Self::mono(cx)
                    .flex_none()
                    .debug_selector(|| "http-status".to_owned())
                    .text_color(StatusTone::Error.color(cx))
                    .child(SharedString::from(error)),
            ),
            None => row,
        };
        let request_line = Self::mono(cx)
            .min_w_0()
            .truncate()
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(self.request_line().unwrap_or_default()));
        row.child(div().flex_1().min_w_0().flex().justify_end().child(request_line))
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let labels = self.tabs(cx);
        let tabs = [ExchangeTab::Body, ExchangeTab::Headers, ExchangeTab::Request];
        let ids = ["http-tab-body", "http-tab-headers", "http-tab-request"];
        let mut strip = div()
            .flex_none()
            .h(px(28.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .gap(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        for ((tab, id), label) in tabs.into_iter().zip(ids).zip(labels) {
            let active = self.tab == tab;
            strip = strip.child(
                div()
                    .id(id)
                    .debug_selector(move || id.to_owned())
                    .h_full()
                    .px(px(8.0))
                    .flex()
                    .items_center()
                    .text_color(theme::color(cx, if active { "fg" } else { "fg-secondary" }))
                    .when(active, |el| el.border_b_2().border_color(theme::color(cx, "accent")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.select_tab(tab, cx))),
            );
        }
        strip = strip.child(div().flex_1());
        if let Some(pressed) = self.raw_toggle() {
            let title = tx(cx, if pressed { "Show Formatted" } else { "Show Raw" });
            strip = strip.child(
                div()
                    .id("http-raw")
                    .debug_selector(|| "http-raw".to_owned())
                    .size(px(24.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.0))
                    .when(pressed, |el| el.bg(theme::color(cx, "bg-active")))
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .tooltip(crate::ui::tooltip::tooltip(title))
                    .child(theme::icon(cx, "soft-wrap", 14.0, theme::color(cx, "fg")))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_raw(cx))),
            );
        }
        strip
    }

    fn pre(text: String, cx: &App) -> Div {
        div()
            .p(px(8.0))
            .font_family(theme::installed_fonts::mono_family(cx))
            .text_size(theme::length(cx, "fs-code"))
            .text_color(theme::color(cx, "fg-editor"))
            .child(SharedString::from(text))
    }

    fn tab_body(&self, cx: &mut Context<Self>) -> Div {
        match self.tab {
            ExchangeTab::Body => match self.body() {
                Some(BodyShown::Error(error)) => Self::pre(error, cx),
                Some(BodyShown::Binary { content_type, size }) => {
                    let kind = content_type.unwrap_or_else(|| tx(cx, "unknown type"));
                    Self::empty_state("Binary response", Some(format!("{kind}, {size}")), cx)
                }
                Some(BodyShown::Text { text, truncated }) => {
                    Self::pre(text, cx).debug_selector(|| "http-body".to_owned()).children(truncated.map(|size| {
                        div()
                            .text_color(theme::color(cx, "fg-muted"))
                            .child(SharedString::from(format!("… truncated ({size} received)")))
                    }))
                }
                None => div(),
            },
            ExchangeTab::Headers => {
                let rows = self.header_rows().into_iter().map(|(name, value)| {
                    div()
                        .flex()
                        .gap(px(16.0))
                        .py(px(2.0))
                        .child(
                            div()
                                .flex_none()
                                .whitespace_nowrap()
                                .text_color(theme::color(cx, "syn-property"))
                                .child(SharedString::from(name)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_color(theme::color(cx, "fg"))
                                .child(SharedString::from(value)),
                        )
                });
                Self::mono(cx).mx(px(8.0)).my(px(4.0)).flex().flex_col().children(rows)
            }
            ExchangeTab::Request => Self::pre(self.request_text().unwrap_or_default(), cx),
        }
    }

    fn exchange_pane(&self, cx: &mut Context<Self>) -> Div {
        if self.exchange.is_some() {
            return div()
                .size_full()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.status_row(cx))
                .child(self.tab_strip(cx))
                .child(div().id("http-exchange-body").flex_1().min_h_0().overflow_y_scroll().child(self.tab_body(cx)));
        }
        if self.is_loading() {
            return div().size_full().flex().items_center().justify_center().child(theme::icon(
                cx,
                "refresh",
                16.0,
                theme::color(cx, "fg-secondary"),
            ));
        }
        Self::empty_state("No response selected", None, cx)
    }
}

impl Focusable for HttpClientView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for HttpClientView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .child(
                div()
                    .flex_none()
                    .w(relative(0.32))
                    .min_w(px(160.0))
                    .h_full()
                    .border_r_1()
                    .border_color(theme::color(cx, "border"))
                    .child(self.history_list(cx)),
            )
            .child(div().flex_1().min_w(px(200.0)).h_full().child(self.exchange_pane(cx)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_client::store::tests::exchange;
    use crate::platform::input;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use ide_http::HttpResponse;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<HttpClientViewEvent>>>;

    fn response(content_type: &str, body: Option<&str>) -> HttpResponse {
        HttpResponse {
            status: 200,
            status_text: "OK".to_owned(),
            version: "HTTP/1.1".to_owned(),
            headers: vec![("content-type".to_owned(), content_type.to_owned()), ("x-id".to_owned(), "7".to_owned())],
            content_type: Some(content_type.to_owned()),
            body: body.map(str::to_owned),
            size: 2048.0,
            truncated: false,
            ttfb_ms: 3.0,
            total_ms: 12.4,
        }
    }

    fn answered(id: &str, content_type: &str, body: Option<&str>) -> HttpExchange {
        let mut ex = exchange(id, Some(200));
        ex.response = Some(response(content_type, body));
        ex
    }

    fn open(cx: &mut TestAppContext) -> (Entity<HttpClientView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| HttpClientView::new(cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&view, move |_, event: &HttpClientViewEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    /// With nothing sent the list says so; a request being sent shows its number above the history.
    #[gpui_kit::test]
    async fn shows_the_empty_history_and_the_request_being_sent(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        assert!(view.read_with(cx, |v, _| v.is_empty()));
        view.update(cx, |v, cx| v.set_state(Vec::new(), None, None, Some(1), cx));
        assert!(!view.read_with(cx, |v, _| v.is_empty()));
        assert_eq!(view.update(cx, |v, cx| v.running_text(cx)).as_deref(), Some("Sending request #2…"));
        cx.run_until_parked();
        assert!(cx.debug_bounds("http-running").is_some());
        let stop = view.read_with(cx, |v, _| v.header_actions()[0]);
        assert_eq!((stop.title, stop.enabled), ("Stop Request", true));
    }

    /// Each entry shows its status (ERR without a response, coloured by class), method and name or
    /// URL; a click asks for that entry.
    #[gpui_kit::test]
    async fn lists_the_history_and_a_click_selects_an_entry(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut failed = exchange("e2", None).entry;
        failed.error = Some("connection refused".to_owned());
        let mut named = exchange("e1", Some(302)).entry;
        named.name = Some("Login".to_owned());
        view.update(cx, |v, cx| v.set_state(vec![failed, named], None, None, None, cx));
        let rows = view.read_with(cx, |v, _| v.rows());
        let shown: Vec<(&str, StatusTone, &str, &str)> =
            rows.iter().map(|r| (r.status.as_str(), r.tone, r.label.as_str(), r.tooltip.as_str())).collect();
        assert_eq!(
            shown,
            [
                ("ERR", StatusTone::Error, "http://127.0.0.1/e2", "connection refused"),
                ("302", StatusTone::Redirect, "Login", "http://127.0.0.1/e1")
            ]
        );
        click(cx, "http-entry-1");
        assert_eq!(events.borrow().last(), Some(&HttpClientViewEvent::Select("e1".to_owned())));
        view.update(cx, |v, cx| v.set_state(v.history.clone(), Some("e1".to_owned()), None, None, cx));
        assert!(view.read_with(cx, |v, _| v.is_loading()), "the entry's exchange is loading");
        assert!(view.read_with(cx, |v, _| v.rows()[1].selected));
    }

    /// The status line shows the status, version, duration and size; the JSON body is formatted
    /// until Show Raw is pressed, and another exchange opens formatted again.
    #[gpui_kit::test]
    async fn formats_the_body_until_show_raw_is_pressed(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let ex = answered("e1", "application/json", Some(r#"{"a":[1]}"#));
        view.update(cx, |v, cx| v.set_state(vec![ex.entry.clone()], Some("e1".into()), Some(ex.clone()), None, cx));
        let line = view.read_with(cx, |v, _| v.status_line());
        let Some(StatusLine::Response { status, version, total, size, .. }) = line else { panic!("a response") };
        assert_eq!(
            (status.as_str(), version.as_str(), total.as_str(), size.as_str()),
            ("200 OK", "HTTP/1.1", "12 ms", "2.0 KB")
        );
        let text = |view: &Entity<HttpClientView>, cx: &mut VisualTestContext| match view.read_with(cx, |v, _| v.body())
        {
            Some(BodyShown::Text { text, .. }) => text,
            other => panic!("a text body, not {other:?}"),
        };
        assert_eq!(text(&view, cx), "{\n  \"a\": [\n    1\n  ]\n}");
        assert_eq!(view.read_with(cx, |v, _| v.raw_toggle()), Some(false));
        click(cx, "http-raw");
        assert_eq!(text(&view, cx), r#"{"a":[1]}"#);
        assert_eq!(view.read_with(cx, |v, _| v.raw_toggle()), Some(true));
        let other = answered("e2", "application/json", Some("[]"));
        view.update(cx, |v, cx| v.set_state(vec![other.entry.clone()], Some("e2".into()), Some(other), None, cx));
        assert_eq!(view.read_with(cx, |v, _| v.raw_toggle()), Some(false));
    }

    /// Headers lists the response headers with their count in the tab, Request the request as sent;
    /// plain text offers no raw toggle.
    #[gpui_kit::test]
    async fn shows_the_headers_and_the_request_as_sent(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let mut ex = answered("e1", "text/plain", Some("hello"));
        ex.request.method = "POST".to_owned();
        ex.request.headers = vec![("Accept".to_owned(), "text/plain".to_owned())];
        ex.request.body = Some("ping".to_owned());
        view.update(cx, |v, cx| v.set_state(vec![ex.entry.clone()], Some("e1".into()), Some(ex.clone()), None, cx));
        assert_eq!(view.read_with(cx, |v, _| v.raw_toggle()), None);
        assert_eq!(view.update(cx, |v, cx| v.tabs(cx)), ["Response", "Headers (2)", "Request"]);
        click(cx, "http-tab-headers");
        assert_eq!(view.read_with(cx, |v, _| v.tab()), ExchangeTab::Headers);
        assert_eq!(view.read_with(cx, |v, _| v.header_rows())[1], ("x-id".to_owned(), "7".to_owned()));
        click(cx, "http-tab-request");
        let request = view.read_with(cx, |v, _| v.request_text());
        assert_eq!(request.as_deref(), Some("POST http://127.0.0.1/e1\nAccept: text/plain\n\nping"));
    }

    /// A body that is not text is described by its type and size; a request without a response shows
    /// its error on the status line and in the Response tab.
    #[gpui_kit::test]
    async fn describes_binary_bodies_and_failed_requests(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx);
        let ex = answered("e1", "image/png", None);
        view.update(cx, |v, cx| v.set_state(vec![ex.entry.clone()], Some("e1".into()), Some(ex), None, cx));
        let binary = BodyShown::Binary { content_type: Some("image/png".to_owned()), size: "2.0 KB".to_owned() };
        assert_eq!(view.read_with(cx, |v, _| v.body()), Some(binary));
        let mut failed = exchange("e2", None);
        failed.entry.error = Some("connection refused".to_owned());
        view.update(cx, |v, cx| v.set_state(vec![failed.entry.clone()], Some("e2".into()), Some(failed), None, cx));
        assert_eq!(view.read_with(cx, |v, _| v.status_line()), Some(StatusLine::Failed("connection refused".into())));
        assert_eq!(view.read_with(cx, |v, _| v.body()), Some(BodyShown::Error("connection refused".into())));
        assert_eq!(view.update(cx, |v, cx| v.tabs(cx))[1], "Headers");
    }

    /// Reload and Clear History are always offered; Stop Request only while a request is sent.
    #[gpui_kit::test]
    async fn the_header_actions_ask_for_their_work(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        view.update(cx, |v, cx| v.press(STOP_REQUEST, cx));
        assert!(events.borrow().is_empty(), "nothing to stop");
        view.update(cx, |v, cx| v.press(RELOAD_HISTORY, cx));
        view.update(cx, |v, cx| v.press(CLEAR_HISTORY, cx));
        view.update(cx, |v, cx| v.set_state(Vec::new(), None, None, Some(0), cx));
        view.update(cx, |v, cx| v.press(STOP_REQUEST, cx));
        assert_eq!(
            *events.borrow(),
            [HttpClientViewEvent::ReloadHistory, HttpClientViewEvent::ClearHistory, HttpClientViewEvent::Stop]
        );
    }

    #[test]
    fn shows_the_time_as_the_english_clock_does() {
        // 2023-11-14 22:14:00 UTC, a whole minute, five seconds later.
        let ms = 1_700_000_040_000.0 + 5_000.0;
        let text = clock_time(ms);
        assert!(text.ends_with(" AM") || text.ends_with(" PM"), "{text}");
        assert_eq!(text.split(':').nth(2).map(|rest| &rest[..2]), Some("05"));
    }
}
