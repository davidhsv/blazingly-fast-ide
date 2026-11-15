//! Appearance & Behavior › System Settings › HTTP Proxy: how the IDE's own connections reach the
//! network (no proxy, the system's, or one configured here, with its credentials), what keeps a
//! manual configuration from working, and Check connection…, which requests a URL with the
//! settings as edited.
//!
//! The page edits the global settings only. The password is drawn as bullets by a
//! [`SecretField`], is never part of [`HttpProxyView::lines`] and leaves the view only inside a
//! [`ProxySettings`] (whose `Debug` hides it) for Check connection; on Apply the backend keeps
//! it in the saved passwords rather than in the settings file.

use super::{
    Binding, ControlEvent, INDENT, NUMBER_FIELD, Then, WIDE_FIELD, frame, hint_line, new_field, row, row_label,
};
use crate::platform::i18n::{Localization, Param};
use crate::settings_view::controls::{button, check_box, number_json, parse_number_input, radio_dot, search_label, tx};
use crate::theme;
use crate::ui::secret_field::{BULLET, SecretField, SecretFieldEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, Render, SharedString,
    Subscription, Window, div, prelude::*, px,
};
use ide_core::settings::ProxySettings;
use serde_json::{Value, json};
use std::rc::Rc;

pub const PAGE: &str = "general.httpProxy";
/// The slot the controls fill.
pub const SLOT: &str = "Manual proxy configuration";

const PORT_RANGE: (f64, f64) = (1.0, 65_535.0);

const MODES: [(&str, &str, Option<&str>); 3] = [
    ("none", "No proxy", None),
    (
        "auto",
        "Auto-detect proxy settings",
        Some("Uses the system proxy: the HTTP_PROXY, HTTPS_PROXY and NO_PROXY environment variables."),
    ),
    ("manual", "Manual proxy configuration", None),
];

const KINDS: [(&str, &str); 2] = [("http", "HTTP"), ("socks", "SOCKS")];

/// Requests `url` through the proxy given and hands `then` the response's status, or why there
/// was none. The page's caller provides it; without one Check connection is unavailable.
pub type Checker = Rc<dyn Fn(ProxySettings, String, Then<Result<String, String>>, &mut App)>;

/// The Check connection… the Settings dialog offers: set by the app (or a test) as a global; the
/// button is unavailable while there is none.
pub struct ConnectionCheck(pub Checker);

impl gpui_kit::Global for ConnectionCheck {}

/// The checker set as [`ConnectionCheck`], if any.
pub fn checker(cx: &App) -> Option<Checker> {
    cx.try_global::<ConnectionCheck>().map(|check| check.0.clone())
}

/// The port a kind of proxy listens on unless told otherwise.
fn default_port(kind: &str) -> f64 {
    if kind == "socks" { 1080.0 } else { 80.0 }
}

/// The proxy settings as the page shows them: values of the wrong kind read as their defaults.
#[derive(Clone, PartialEq)]
pub struct ProxyForm {
    pub mode: String,
    pub kind: String,
    pub host: String,
    pub port: f64,
    pub no_proxy: String,
    pub authenticate: bool,
    pub login: String,
    password: String,
}

impl ProxyForm {
    pub fn read(binding: &Binding, cx: &App) -> Self {
        let value = binding.value("proxy", cx).unwrap_or(Value::Null);
        let text = |name: &str| value.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
        let mode = match value.get("mode").and_then(Value::as_str) {
            Some(mode @ ("none" | "manual")) => mode.to_owned(),
            _ => "auto".to_owned(),
        };
        let kind = if value.get("kind").and_then(Value::as_str) == Some("socks") { "socks" } else { "http" };
        let port = value
            .get("port")
            .and_then(Value::as_f64)
            .filter(|port| port.fract() == 0.0 && (PORT_RANGE.0..=PORT_RANGE.1).contains(port))
            .unwrap_or_else(|| default_port(kind));
        Self {
            mode,
            kind: kind.to_owned(),
            host: text("host"),
            port,
            no_proxy: text("noProxy"),
            authenticate: value.get("authenticate").and_then(Value::as_bool) == Some(true),
            login: text("login"),
            password: text("password"),
        }
    }

    fn manual(&self) -> bool {
        self.mode == "manual"
    }

    /// What keeps a manual configuration from working, as the backend checks it.
    pub fn problem(&self) -> Option<&'static str> {
        if !self.manual() {
            return None;
        }
        let host = self.host.trim();
        if host.is_empty() {
            return Some("Proxy host is empty");
        }
        if host.chars().any(|c| c == '/' || c == '@' || c.is_whitespace()) {
            return Some("Invalid proxy host");
        }
        if !(PORT_RANGE.0..=PORT_RANGE.1).contains(&self.port) {
            return Some("Proxy port must be between 1 and 65535");
        }
        if self.authenticate && self.login.trim().is_empty() {
            return Some("Proxy login is empty");
        }
        None
    }

    /// The settings as the backend takes them, password included.
    pub fn settings(&self) -> ProxySettings {
        let value = json!({
            "mode": self.mode,
            "kind": self.kind,
            "host": self.host,
            "port": self.port as u16,
            "noProxy": self.no_proxy,
            "authenticate": self.authenticate,
            "login": self.login,
            "password": self.password,
        });
        serde_json::from_value(value).unwrap_or_default()
    }
}

/// Where Check connection… stands.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckState {
    Idle,
    Running,
    Done(Result<String, String>),
}

pub struct HttpProxyView {
    binding: Binding,
    host: Entity<TextField>,
    port: Entity<TextField>,
    no_proxy: Entity<TextField>,
    login: Entity<TextField>,
    password: Entity<SecretField>,
    /// The port typed is not one: it is flagged and not staged.
    port_invalid: bool,
    checker: Option<Checker>,
    check_open: bool,
    url: Entity<TextField>,
    check: CheckState,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for HttpProxyView {}

impl HttpProxyView {
    pub fn new(binding: Binding, checker: Option<Checker>, cx: &mut Context<Self>) -> Self {
        let form = ProxyForm::read(&binding, cx);
        let mut subscriptions = Vec::new();
        let mut text_field = |key: &'static str, text: &str, cx: &mut Context<Self>| {
            let field = new_field(text, None, cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.edited(key, text, cx);
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
                }
            }));
            field
        };
        let host = text_field("host", &form.host, cx);
        let port = text_field("port", &super::super::controls::number_text(form.port), cx);
        let no_proxy = text_field("noProxy", &form.no_proxy, cx);
        let login = text_field("login", &form.login, cx);
        let password = cx.new(|cx| {
            let mut field = SecretField::new(cx);
            field.set_text(&form.password, cx);
            field
        });
        subscriptions.push(cx.subscribe(
            &password,
            |view: &mut Self, field, event: &SecretFieldEvent, cx| match event {
                SecretFieldEvent::Changed => {
                    let text = field.read(cx).text(cx).to_owned();
                    view.binding.stage("proxy.password", Value::String(text), cx);
                }
                SecretFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                SecretFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
            },
        ));
        let url = new_field("http://", None, cx);
        subscriptions.push(cx.subscribe(&url, |view: &mut Self, _, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => cx.notify(),
            TextFieldEvent::Submitted => view.run_check(cx),
            TextFieldEvent::Cancelled => {
                view.check_open = false;
                cx.notify();
            }
        }));
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let mut view = Self {
            binding,
            host,
            port,
            no_proxy,
            login,
            password,
            port_invalid: false,
            checker,
            check_open: false,
            url,
            check: CheckState::Idle,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn form(&self, cx: &App) -> ProxyForm {
        ProxyForm::read(&self.binding, cx)
    }

    /// The text field of `proxy.<name>` (`host`, `port`, `noProxy`, `login`).
    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        match name {
            "host" => Some(&self.host),
            "port" => Some(&self.port),
            "noProxy" => Some(&self.no_proxy),
            "login" => Some(&self.login),
            _ => None,
        }
    }

    pub fn password_field(&self) -> &Entity<SecretField> {
        &self.password
    }

    pub fn url_field(&self) -> &Entity<TextField> {
        &self.url
    }

    pub fn check_state(&self) -> &CheckState {
        &self.check
    }

    pub fn is_check_open(&self) -> bool {
        self.check_open
    }

    fn edited(&mut self, name: &'static str, text: String, cx: &mut Context<Self>) {
        if name == "port" {
            let parsed = parse_number_input(&text, PORT_RANGE.0, PORT_RANGE.1, true);
            self.port_invalid = parsed.is_none();
            if let Some(port) = parsed {
                self.binding.stage("proxy.port", number_json(port), cx);
            }
            cx.notify();
            return;
        }
        self.binding.stage(&format!("proxy.{name}"), Value::String(text), cx);
    }

    /// Follows Reset and the other controls: each field shows its value (a port being typed that is
    /// not one stays), and is editable only while it can be used.
    fn sync(&mut self, cx: &mut Context<Self>) {
        let form = self.form(cx);
        let port = super::super::controls::number_text(form.port);
        let texts: [(&Entity<TextField>, &str, bool); 4] = [
            (&self.host, &form.host, form.manual()),
            (&self.no_proxy, &form.no_proxy, form.manual()),
            (&self.login, &form.login, form.manual() && form.authenticate),
            (&self.port, &port, form.manual()),
        ];
        let mut updates: Vec<(Entity<TextField>, Option<String>, bool)> = Vec::new();
        for (field, wanted, open) in texts {
            let read = field.read(cx);
            let is_port = field == &self.port;
            let text = (read.text() != wanted && !(is_port && self.port_invalid)).then(|| wanted.to_owned());
            if text.is_some() || read.is_read_only() == open {
                updates.push((field.clone(), text, open));
            }
        }
        for (field, text, open) in updates {
            field.update(cx, |field, cx| {
                if let Some(text) = text {
                    field.set_text(&text, cx);
                }
                field.set_read_only(!open, cx);
            });
        }
        if !form.manual() {
            self.port_invalid = false;
        }
        if self.password.read(cx).text(cx) != form.password {
            let password = form.password.clone();
            self.password.update(cx, |field, cx| field.set_text(&password, cx));
        }
        cx.notify();
    }

    fn choose_mode(&mut self, mode: &'static str, cx: &mut Context<Self>) {
        self.binding.stage("proxy.mode", Value::from(mode), cx);
    }

    /// HTTP ⇄ SOCKS: a port left at one kind's default moves to the other's.
    fn choose_kind(&mut self, kind: &'static str, cx: &mut Context<Self>) {
        let form = self.form(cx);
        let port = if form.port == default_port(&form.kind) { default_port(kind) } else { form.port };
        self.binding.stage("proxy.kind", Value::from(kind), cx);
        self.binding.stage("proxy.port", number_json(port), cx);
    }

    fn flip_authenticate(&mut self, cx: &mut Context<Self>) {
        let now = self.form(cx).authenticate;
        self.binding.stage("proxy.authenticate", Value::Bool(!now), cx);
    }

    /// Whether Check connection… can be opened: a checker is there and the configuration works.
    pub fn can_check(&self, cx: &App) -> bool {
        self.checker.is_some() && self.form(cx).problem().is_none()
    }

    pub fn toggle_check(&mut self, cx: &mut Context<Self>) {
        if self.can_check(cx) {
            self.check_open = !self.check_open;
            cx.notify();
        }
    }

    /// Check: requests the URL typed with the settings as edited.
    pub fn run_check(&mut self, cx: &mut Context<Self>) {
        let url = self.url.read(cx).text().trim().to_owned();
        let Some(checker) = self.checker.clone() else { return };
        if self.check == CheckState::Running || url.is_empty() {
            return;
        }
        self.check = CheckState::Running;
        let view = cx.weak_entity();
        let then: Then<Result<String, String>> = Rc::new(move |result: Result<String, String>, cx: &mut App| {
            view.update(cx, |view, cx| {
                view.check = CheckState::Done(result);
                cx.notify();
            })
            .ok();
        });
        let settings = self.form(cx).settings();
        checker(settings, url, then, cx);
        cx.notify();
    }

    /// What the result line under the URL says.
    fn result_text(&self, cx: &mut App) -> Option<(String, bool)> {
        let i18n = &mut cx.default_global::<Localization>().0;
        match &self.check {
            CheckState::Idle => None,
            CheckState::Running => Some((i18n.tx("Connecting…"), true)),
            CheckState::Done(Ok(status)) => {
                Some((i18n.tx_built(&["Connection successful: ", ""], &[Param::Text(status.clone())]), true))
            }
            CheckState::Done(Err(message)) => {
                Some((i18n.tx_built(&["Problem with connection: ", ""], &[Param::Text(message.clone())]), false))
            }
        }
    }

    /// The controls as text, the password as one bullet per character: radio rows, `Label: text`
    /// with `(disabled)` and `(invalid)` flags, `[x] Proxy authentication`, the problem, and
    /// `[Check connection…]`.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let form = self.form(cx);
        let flag = |open: bool| if open { "" } else { " (disabled)" };
        let radios = |choices: &[(&str, &str)], current: &str| {
            let shown: Vec<String> = choices
                .iter()
                .map(|(value, label)| format!("({}) {}", if *value == current { "•" } else { " " }, tx(cx, label)))
                .collect();
            shown.join(" ")
        };
        let modes: Vec<(&str, &str)> = MODES.iter().map(|(value, label, _)| (*value, *label)).collect();
        let (manual, auth) = (form.manual(), form.manual() && form.authenticate);
        let text = |field: &Entity<TextField>| field.read(cx).text().to_owned();
        let invalid = if self.port_invalid { " (invalid)" } else { "" };
        let bullets: String = self.password.read(cx).text(cx).chars().map(|_| BULLET).collect();
        let mut out = vec![radios(&modes, &form.mode)];
        out.push(format!("    {}", tx(cx, MODES[1].2.unwrap_or_default())));
        out.push(format!("{}{}", radios(&KINDS, &form.kind), flag(manual)));
        out.push(format!("{}: {}{}", tx(cx, "Host name"), text(&self.host), flag(manual)));
        out.push(format!("{}: {}{invalid}{}", tx(cx, "Port number"), text(&self.port), flag(manual)));
        out.push(format!("{}: {}{}", tx(cx, "No proxy for"), text(&self.no_proxy), flag(manual)));
        out.push(format!("    {}", tx(cx, NO_PROXY_HINT)));
        let mark = if form.authenticate { "x" } else { " " };
        out.push(format!("[{mark}] {}{}", tx(cx, "Proxy authentication"), flag(manual)));
        out.push(format!("{}: {}{}", tx(cx, "Login"), text(&self.login), flag(auth)));
        out.push(format!("{}: {bullets}{}", tx(cx, "Password"), flag(auth)));
        out.extend(form.problem().map(|problem| format!("! {}", tx(cx, problem))));
        out.push(format!("[{}]{}", tx(cx, "Check connection…"), flag(self.can_check(cx))));
        out
    }

    fn radio(
        &self,
        key: &'static str,
        value: &'static str,
        label: &str,
        on: bool,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let query = self.binding.query(cx);
        let text = tx(cx, label);
        div()
            .id(SharedString::from(format!("settings-radio-{key}-{value}")))
            .debug_selector(move || format!("settings-radio-{key}-{value}"))
            .flex()
            .items_center()
            .gap(px(6.0))
            .px(px(4.0))
            .when(!open, |el| el.opacity(0.5))
            .when(self.binding.matches(label, cx), |el| el.bg(theme::color(cx, "warning").opacity(0.08)))
            .child(radio_dot(cx, on))
            .child(search_label(cx, &text, &query))
    }

    /// `Label [field]`; the port's field is flagged while what is typed is not a port.
    fn text_row(
        &self,
        label: &str,
        selector: &'static str,
        field: &Entity<TextField>,
        width: f32,
        open: bool,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let query = self.binding.query(cx);
        let invalid = field == &self.port && self.port_invalid;
        let text = tx(cx, label);
        row(cx, self.binding.matches(label, cx))
            .when(!open, |el| el.opacity(0.5))
            .child(row_label(cx, &text, &query))
            .child(
                frame(cx, width, invalid).id(selector).debug_selector(move || selector.to_owned()).child(field.clone()),
            )
    }
}

const NO_PROXY_HINT: &str = "Comma-separated host names; * matches any characters, e.g. *.example.com, 192.168.*";

impl Render for HttpProxyView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.binding.query(cx);
        let form = self.form(cx);
        let (manual, auth) = (form.manual(), form.manual() && form.authenticate);
        let modes: Vec<AnyElement> = MODES
            .iter()
            .map(|(value, label, hint)| {
                let value = *value;
                let radio = self
                    .radio("proxy.mode", value, label, form.mode == value, true, cx)
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_mode(value, cx)));
                let hint = hint.map(|hint| hint_line(cx, &tx(cx, hint), 28.0));
                div().flex().flex_col().child(row(cx, false).child(radio)).children(hint).into_any_element()
            })
            .collect();
        let kinds: Vec<AnyElement> = KINDS
            .iter()
            .map(|(value, label)| {
                let value = *value;
                self.radio("proxy.kind", value, label, form.kind == value, manual, cx)
                    .when(manual, |el| {
                        el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_kind(value, cx)))
                    })
                    .into_any_element()
            })
            .collect();
        let host = self.text_row("Host name", "settings-text-proxy.host", &self.host, 240.0, manual, cx);
        let port = self.text_row("Port number", "settings-number-proxy.port", &self.port, NUMBER_FIELD, manual, cx);
        let no_proxy =
            self.text_row("No proxy for", "settings-text-proxy.noProxy", &self.no_proxy, WIDE_FIELD, manual, cx);
        let authenticate_label = tx(cx, "Proxy authentication");
        let authenticate = row(cx, self.binding.matches("Proxy authentication", cx))
            .id("settings-toggle-proxy.authenticate")
            .debug_selector(|| "settings-toggle-proxy.authenticate".to_owned())
            .when(!manual, |el| el.opacity(0.5))
            .child(check_box(cx, form.authenticate))
            .child(search_label(cx, &authenticate_label, &query))
            .when(manual, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.flip_authenticate(cx))));
        let login = self.text_row("Login", "settings-text-proxy.login", &self.login, 240.0, auth, cx);
        let password_label = tx(cx, "Password");
        // While authentication is off the field is shown, dimmed, as the bullets it holds.
        let password_box = if auth {
            frame(cx, 240.0, false).child(self.password.clone())
        } else {
            let bullets = self.password.read(cx).shown(cx);
            frame(cx, 240.0, false).child(SharedString::from(bullets))
        };
        let password = row(cx, self.binding.matches("Password", cx))
            .when(!auth, |el| el.opacity(0.5))
            .child(row_label(cx, &password_label, &query))
            .child(
                password_box
                    .id("settings-secret-proxy.password")
                    .debug_selector(|| "settings-secret-proxy.password".to_owned()),
            );
        let problem = form.problem().map(|problem| {
            let text = tx(cx, problem);
            div()
                .debug_selector(|| "settings-proxy-problem".to_owned())
                .px(px(4.0))
                .text_color(theme::color(cx, "danger"))
                .child(SharedString::from(text))
        });
        let can_check = self.can_check(cx);
        let check_label = tx(cx, "Check connection…");
        let check = div()
            .id("settings-proxy-check")
            .debug_selector(|| "settings-proxy-check".to_owned())
            .child(button(cx, &check_label, false, can_check))
            .when(can_check, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_check(cx))));
        let popup = self.check_open.then(|| {
            let url_text = self.url.read(cx).text().trim().to_owned();
            let running = self.check == CheckState::Running;
            let can_run = !running && !url_text.is_empty();
            let run_label = tx(cx, "Check");
            let run = div()
                .id("settings-proxy-check-run")
                .debug_selector(|| "settings-proxy-check-run".to_owned())
                .child(button(cx, &run_label, false, can_run))
                .when(can_run, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.run_check(cx))));
            let result = self.result_text(cx).map(|(text, ok)| {
                let color = match (&self.check, ok) {
                    (CheckState::Running, _) => "fg-subtle",
                    (_, true) => "success",
                    (_, false) => "danger",
                };
                div()
                    .debug_selector(|| "settings-proxy-check-result".to_owned())
                    .text_color(theme::color(cx, color))
                    .child(SharedString::from(text))
            });
            let prompt = tx(cx, "Enter any URL to check connection to:");
            div()
                .ml(px(4.0))
                .mt(px(4.0))
                .p(px(8.0))
                .max_w(px(480.0))
                .flex()
                .flex_col()
                .gap(px(6.0))
                .rounded(theme::length(cx, "radius-small"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .bg(theme::color(cx, "bg-popup"))
                .child(SharedString::from(prompt))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .child(
                            frame(cx, WIDE_FIELD, false)
                                .id("settings-proxy-check-url")
                                .debug_selector(|| "settings-proxy-check-url".to_owned())
                                .child(self.url.clone()),
                        )
                        .child(run),
                )
                .children(result)
        });
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .children(modes)
            .child(
                div()
                    .ml(px(INDENT))
                    .flex()
                    .flex_col()
                    .child(row(cx, false).gap(px(16.0)).children(kinds))
                    .child(div().flex().gap(px(16.0)).child(host).child(port))
                    .child(no_proxy)
                    .child(hint_line(cx, &tx(cx, NO_PROXY_HINT), 4.0))
                    .child(authenticate)
                    .child(div().ml(px(INDENT)).flex().gap(px(16.0)).child(login).child(password))
                    .children(problem),
            )
            .child(div().mt(px(8.0)).child(check))
            .children(popup)
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use crate::settings_model::staging::Scope;
    use gpui_kit::TestAppContext;
    use std::cell::RefCell;

    const SECRET: &str = "pr0xyS3cretX";

    fn manual() -> Vec<(&'static str, Value)> {
        vec![
            ("proxy.mode", json!("manual")),
            ("proxy.host", json!("proxy.example.test")),
            ("proxy.port", json!(3128)),
            ("proxy.authenticate", json!(true)),
            ("proxy.login", json!("me")),
            ("proxy.password", json!(SECRET)),
        ]
    }

    // Not from a spec: by default the system's proxy is used and the manual controls are dimmed;
    // Manual makes them editable, an empty host is the problem shown and Check is unavailable.
    #[gpui_kit::test]
    async fn manual_configuration_enables_the_controls_and_names_what_is_missing(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &[], cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let checker: Checker =
            Rc::new(|_: ProxySettings, _: String, then: Then<Result<String, String>>, cx: &mut App| {
                then(Ok("200".to_owned()), cx)
            });
        let (view, _, cx) = open(cx, move |_, cx| HttpProxyView::new(binding, Some(checker), cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert_eq!(lines[0], "( ) No proxy (•) Auto-detect proxy settings ( ) Manual proxy configuration");
        assert_eq!(lines[2], "(•) HTTP ( ) SOCKS (disabled)");
        assert_eq!(lines[4], "Port number: 80 (disabled)");
        click(cx, "settings-radio-proxy.mode-manual");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.contains(&"! Proxy host is empty".to_owned()), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("[Check connection…] (disabled)"));
        click(cx, "settings-radio-proxy.kind-socks");
        let host = view.read_with(cx, |view, _| view.field("host").cloned()).expect("the host field");
        type_over(&host, "proxy.example.test", cx);
        let staged = staged(&model, cx);
        for (key, value) in [
            ("global:proxy.mode", "\"manual\""),
            ("global:proxy.kind", "\"socks\""),
            ("global:proxy.port", "1080"),
            ("global:proxy.host", "\"proxy.example.test\""),
        ] {
            assert!(staged.contains(&(key.to_owned(), value.to_owned())), "{key} in {staged:?}");
        }
        assert!(view.read_with(cx, |view, cx| view.can_check(cx)));
    }

    // Not from a spec: the password is drawn and listed as bullets only; Check connection hands the
    // checker the settings as edited, password included, and shows its answer.
    #[gpui_kit::test]
    async fn the_password_is_shown_as_bullets_and_check_uses_the_settings_as_edited(cx: &mut TestAppContext) {
        let model = cx.update(|cx| dialog_model(PAGE, &manual(), cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let asked: Rc<RefCell<Vec<(String, String, u16, bool)>>> = Rc::default();
        let log = asked.clone();
        let checker: Checker =
            Rc::new(move |proxy: ProxySettings, url: String, then: Then<Result<String, String>>, cx: &mut App| {
                log.borrow_mut().push((url, proxy.host.clone(), proxy.port, proxy.password == SECRET));
                then(Err("timed out".to_owned()), cx);
            });
        let (view, _, cx) = open(cx, move |_, cx| HttpProxyView::new(binding, Some(checker), cx));
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.iter().all(|line| !line.contains(SECRET)), "{lines:?}");
        let bullets: String = SECRET.chars().map(|_| BULLET).collect();
        assert!(lines.contains(&format!("Password: {bullets}")), "{lines:?}");
        click(cx, "settings-proxy-check");
        assert!(view.read_with(cx, |view, _| view.is_check_open()));
        let url = view.read_with(cx, |view, _| view.url_field().clone());
        type_over(&url, "http://example.test/", cx);
        click(cx, "settings-proxy-check-run");
        assert_eq!(
            asked.borrow().as_slice(),
            [("http://example.test/".to_owned(), "proxy.example.test".to_owned(), 3128, true)]
        );
        assert_eq!(
            view.read_with(cx, |view, _| view.check_state().clone()),
            CheckState::Done(Err("timed out".to_owned()))
        );
        assert!(cx.debug_bounds("settings-proxy-check-result").is_some());
    }

    // Not from a spec: a port out of its range is flagged and not staged; the login and password
    // are dimmed until authentication is on.
    #[gpui_kit::test]
    async fn a_port_out_of_range_is_flagged_and_credentials_follow_authentication(cx: &mut TestAppContext) {
        let mut settings = manual();
        settings.retain(|(key, _)| *key != "proxy.authenticate");
        let model = cx.update(|cx| dialog_model(PAGE, &settings, cx));
        let binding = Binding::fixed(model.clone(), PAGE, Scope::Global);
        let (view, _, cx) = open(cx, move |_, cx| HttpProxyView::new(binding, None, cx));
        let port = view.read_with(cx, |view, _| view.field("port").cloned()).expect("the port field");
        type_over(&port, "70000", cx);
        assert!(staged(&model, cx).is_empty());
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.contains(&"Port number: 70000 (invalid)".to_owned()), "{lines:?}");
        assert!(lines.contains(&"Login: me (disabled)".to_owned()), "{lines:?}");
        click(cx, "settings-toggle-proxy.authenticate");
        let lines = view.read_with(cx, |view, cx| view.lines(cx));
        assert!(lines.contains(&"Login: me".to_owned()), "{lines:?}");
        assert_eq!(lines.last().map(String::as_str), Some("[Check connection…] (disabled)"), "no checker");
    }
}
