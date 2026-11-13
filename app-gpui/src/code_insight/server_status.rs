//! The status bar item listing the language servers ("rust-analyzer +1" with a dot in the worst
//! state's colour), and its popup: each server with its state, languages, root and message, and what
//! can be done about it (Restart, Install, Download JDK N). In Blaze mode every server is off: the
//! item says so and the popup offers Deep mode.

use super::servers::{ServerEntry, ServerTone, Servers, ServersSummary, entry_tone, servers_summary, status_label};
use crate::platform::i18n::{I18n, Localization, Param};
use crate::platform::misc::jdk::{DownloadJdkArgs, download_jdk_fix};
use crate::platform::virtual_docs::relative_path;
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, EventEmitter, FontWeight, IntoElement, MouseButton, Render,
    SharedString, Stateful, Window, deferred, div, prelude::*, px,
};
use ide_core::{ServerId, ServerStatus};

/// What the status item asks of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerStatusEvent {
    /// `lsp_restart(server)`.
    Restart(ServerId),
    /// Run `tools.install` with the server's name.
    Install(String),
    /// Run `java.downloadJdk` with these arguments.
    DownloadJdk(DownloadJdkArgs),
    /// Run `ide.toggleMode` (Blaze → Deep mode).
    SwitchToDeepMode,
}

/// What a server's row offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerRowAction {
    /// "Download JDK N": jdtls cannot start for want of a JDK.
    DownloadJdk(DownloadJdkArgs),
    /// The server is not installed and something can install it.
    Install,
    /// Restart; disabled while the server is missing or starting.
    Restart { enabled: bool },
}

/// The status bar item and its popup.
pub struct ServerStatusItem {
    /// By name.
    servers: Vec<ServerEntry>,
    project_root: Option<String>,
    /// Blaze mode: language servers are off.
    light_mode: bool,
    /// `tools.install` is registered.
    can_install: bool,
    /// `java.downloadJdk` is registered.
    can_download_jdk: bool,
    open: bool,
}

impl EventEmitter<ServerStatusEvent> for ServerStatusItem {}

impl ServerStatusItem {
    pub fn new(_: &mut Context<Self>) -> Self {
        Self {
            servers: Vec::new(),
            project_root: None,
            light_mode: false,
            can_install: false,
            can_download_jdk: false,
            open: false,
        }
    }

    /// The servers changed.
    pub fn set_servers(&mut self, servers: &Servers, cx: &mut Context<Self>) {
        self.servers = servers.sorted();
        if self.summary().is_none() {
            self.open = false;
        }
        cx.notify();
    }

    pub fn set_project_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        self.project_root = root;
        cx.notify();
    }

    pub fn set_light_mode(&mut self, light_mode: bool, cx: &mut Context<Self>) {
        self.light_mode = light_mode;
        cx.notify();
    }

    /// Which actions are registered: `tools.install` and `java.downloadJdk`.
    pub fn set_handlers(&mut self, can_install: bool, can_download_jdk: bool, cx: &mut Context<Self>) {
        (self.can_install, self.can_download_jdk) = (can_install, can_download_jdk);
        cx.notify();
    }

    /// What the item shows; `None` hides it (no servers, not in Blaze mode).
    pub fn summary(&self) -> Option<ServersSummary> {
        if self.light_mode {
            return Some(ServersSummary { text: "Blaze mode".to_owned(), tone: ServerTone::Idle });
        }
        servers_summary(&self.servers)
    }

    /// The item's accessible name, e.g. "Language servers: rust-analyzer".
    pub fn label(&self, i18n: &mut I18n) -> Option<String> {
        let summary = self.summary()?;
        let text = if self.light_mode { i18n.tx(&summary.text) } else { summary.text };
        Some(i18n.tx_built(&["Language servers: ", ""], &[Param::from(text)]))
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens or closes the popup.
    pub fn toggle(&mut self, cx: &mut Context<Self>) {
        self.open = !self.open && self.summary().is_some();
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        self.open = false;
        cx.notify();
    }

    /// The servers the popup lists, by name.
    pub fn servers(&self) -> &[ServerEntry] {
        &self.servers
    }

    /// What server `server`'s row offers.
    pub fn row_action(&self, server: &ServerEntry) -> ServerRowAction {
        let fix = (server.status == ServerStatus::Misconfigured)
            .then(|| download_jdk_fix(server.message.as_deref(), self.can_download_jdk))
            .flatten();
        match fix {
            Some(fix) => ServerRowAction::DownloadJdk(fix.args),
            None if server.status == ServerStatus::Missing && self.can_install => ServerRowAction::Install,
            None => ServerRowAction::Restart {
                enabled: !matches!(server.status, ServerStatus::Missing | ServerStatus::Starting),
            },
        }
    }

    /// The accessible name of a server's button: "Download JDK 25", "Install gopls", "Restart jdtls".
    pub fn action_label(&self, server: &ServerEntry, i18n: &mut I18n) -> String {
        let name = Param::from(server.name.as_str());
        match self.row_action(server) {
            ServerRowAction::DownloadJdk(args) => {
                i18n.tx_built(&["Download JDK ", ""], &[Param::Number(f64::from(args.version))])
            }
            ServerRowAction::Install => i18n.tx_built(&["Install ", ""], &[name]),
            ServerRowAction::Restart { .. } => i18n.tx_built(&["Restart ", ""], &[name]),
        }
    }

    /// The line under a server's name: its languages, and its root when it is not the project's.
    pub fn meta(&self, server: &ServerEntry) -> Option<String> {
        if server.languages.is_empty() && server.root.is_none() {
            return None;
        }
        let root = server.root.as_deref().filter(|root| Some(*root) != self.project_root.as_deref()).map(|root| {
            let shown = self.project_root.as_deref().map(|project| relative_path(project, root).into_owned());
            format!(" · {}", shown.filter(|shown| !shown.is_empty()).as_deref().unwrap_or(root))
        });
        Some(format!("{}{}", server.languages.join(", "), root.unwrap_or_default()))
    }

    /// Carries out a row's button; everything but Restart closes the popup (a restart is watched).
    fn run(&mut self, event: ServerStatusEvent, cx: &mut Context<Self>) {
        if !matches!(event, ServerStatusEvent::Restart(_)) {
            self.open = false;
        }
        cx.emit(event);
        cx.notify();
    }

    fn server_row(&self, index: usize, server: &ServerEntry, cx: &mut Context<Self>) -> AnyElement {
        let (strong, muted, subtle) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-subtle"));
        let message_color = if server.status == ServerStatus::Missing { theme::color(cx, "warning") } else { muted };
        let action = self.row_action(server);
        let i18n = &mut cx.default_global::<Localization>().0;
        let status = i18n.tx(status_label(server.status));
        let label = match &action {
            ServerRowAction::DownloadJdk(args) => {
                i18n.tx_built(&["Download JDK ", ""], &[Param::Number(f64::from(args.version))])
            }
            ServerRowAction::Install => i18n.tx("Install"),
            ServerRowAction::Restart { .. } => String::new(),
        };
        let tone = entry_tone(server, &self.servers);
        let (id, name) = (server.id, server.name.clone());
        let button = match action {
            ServerRowAction::DownloadJdk(args) => text_button(cx, format!("server-fix-{index}"), label)
                .on_click(cx.listener(move |item, _: &ClickEvent, _, cx| {
                    item.run(ServerStatusEvent::DownloadJdk(args), cx);
                }))
                .into_any_element(),
            ServerRowAction::Install => {
                let name = name.clone();
                text_button(cx, format!("server-fix-{index}"), label)
                    .on_click(cx.listener(move |item, _: &ClickEvent, _, cx| {
                        item.run(ServerStatusEvent::Install(name.clone()), cx);
                    }))
                    .into_any_element()
            }
            ServerRowAction::Restart { enabled } => {
                let selector = format!("server-restart-{index}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .flex_none()
                    .size(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-hover"))))
                    .child(theme::icon(cx, "refresh", 14.0, if enabled { muted } else { subtle }))
                    .when(enabled, |el| {
                        el.on_click(cx.listener(move |item, _: &ClickEvent, _, cx| {
                            item.run(ServerStatusEvent::Restart(id), cx);
                        }))
                    })
                    .into_any_element()
            }
        };
        let heading = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(dot(cx, tone))
            .child(div().text_color(strong).font_weight(FontWeight::SEMIBOLD).child(SharedString::from(name)))
            .child(div().text_color(muted).child(SharedString::from(status)))
            .child(div().flex_1())
            .child(button);
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .px(px(10.0))
            .py(px(6.0))
            .child(heading)
            .children(
                self.meta(server).map(|meta| div().pl(px(16.0)).text_color(subtle).child(SharedString::from(meta))),
            )
            .children(
                server
                    .message
                    .clone()
                    .map(|message| div().pl(px(16.0)).text_color(message_color).child(SharedString::from(message))),
            )
            .into_any_element()
    }
}

fn dot(cx: &App, tone: ServerTone) -> Div {
    let color = theme::color(
        cx,
        match tone {
            ServerTone::Ok => "success",
            ServerTone::Busy => "warning",
            ServerTone::Error => "danger",
            ServerTone::Idle => "fg-faint",
        },
    );
    div().flex_none().size(px(8.0)).rounded(px(4.0)).bg(color)
}

fn text_button(cx: &App, selector: String, label: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .flex_none()
        .h(px(22.0))
        .px(px(8.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-control"))
        .border_1()
        .border_color(theme::color(cx, "border-button"))
        .bg(theme::color(cx, "bg-button"))
        .text_color(theme::color(cx, "fg-strong"))
        .hover(|el| el.bg(theme::color(cx, "bg-control-hover")))
        .child(SharedString::from(label))
}

impl Render for ServerStatusItem {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(summary) = self.summary() else { return div().into_any_element() };
        let i18n = &mut cx.default_global::<Localization>().0;
        let text = if self.light_mode { i18n.tx(&summary.text) } else { summary.text.clone() };
        let heading = i18n.tx("Language Servers");
        let blaze = i18n.tx("Language servers are off in Blaze mode.");
        let deep = i18n.tx("Switch to Deep Mode");
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let button = div()
            .id("server-status")
            .debug_selector(|| "server-status".to_owned())
            .h(px(20.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .gap(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .text_color(fg)
            .when(self.open, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(dot(cx, summary.tone))
            .child(SharedString::from(text))
            .on_click(cx.listener(|item, _: &ClickEvent, _, cx| item.toggle(cx)));
        let popup = self.open.then(|| {
            let rows: Vec<_> = (0..self.servers.len())
                .map(|index| {
                    let server = self.servers[index].clone();
                    self.server_row(index, &server, cx)
                })
                .collect();
            let blaze_row = self.light_mode.then(|| {
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .child(div().text_color(muted).child(SharedString::from(blaze)))
                    .child(div().flex_1())
                    .child(text_button(cx, "server-deep-mode".to_owned(), deep).on_click(
                        cx.listener(|item, _: &ClickEvent, _, cx| item.run(ServerStatusEvent::SwitchToDeepMode, cx)),
                    ))
            });
            let panel = div()
                .debug_selector(|| "server-status-popup".to_owned())
                .absolute()
                .bottom(px(26.0))
                .right(px(0.0))
                .w(px(380.0))
                .py(px(4.0))
                .flex()
                .flex_col()
                .bg(theme::color(cx, "bg-popup"))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .rounded(theme::length(cx, "radius-popup"))
                .text_size(theme::length(cx, "fs-ui"))
                .occlude()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .px(px(10.0))
                        .py(px(4.0))
                        .text_color(theme::color(cx, "fg-emphasis"))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(SharedString::from(heading)),
                )
                .children(blaze_row)
                .children(rows);
            deferred(panel).with_priority(1)
        });
        div().relative().flex_none().child(button).children(popup).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    const ISSUE_JDK: &str = concat!(
        "jdtls needs JDK 25 or newer to run (found JDK 8, 17, 21). Download JDK 25, or pick an installed one ",
        "in Settings › Languages & Frameworks › Java › Language Server › JDK (it restarts automatically); ",
        "the project can still target an older Java version."
    );

    /// The item at the bottom right of its window, as in the status bar.
    struct Host {
        item: Entity<ServerStatusItem>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().size_full().flex().items_end().justify_end().child(self.item.clone())
        }
    }

    type Events = Rc<RefCell<Vec<ServerStatusEvent>>>;

    fn open(
        cx: &mut TestAppContext,
        status: ServerStatus,
        message: Option<&str>,
        light_mode: bool,
    ) -> (Entity<ServerStatusItem>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let mut servers = Servers::default();
        servers.server_event(ServerId(1), "jdtls", status, message.map(str::to_owned), true);
        let (host, cx) = cx.add_window_view(|_, cx| Host { item: cx.new(ServerStatusItem::new) });
        let item = host.read_with(cx, |host, _| host.item.clone());
        item.update(cx, |item, cx| {
            item.set_servers(&servers, cx);
            item.set_light_mode(light_mode, cx);
            item.set_handlers(true, true, cx);
        });
        let events: Events = Rc::default();
        let log = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&item, move |_, event: &ServerStatusEvent, _| log.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (item, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::none());
    }

    #[gpui_kit::test]
    async fn offers_download_jdk_n_when_jdtls_cant_start_for_want_of_a_jdk(cx: &mut TestAppContext) {
        let (item, events, cx) = open(cx, ServerStatus::Misconfigured, Some(ISSUE_JDK), false);
        click(cx, "server-status");
        assert!(item.read_with(cx, |item, _| item.is_open()));
        let action = item.read_with(cx, |item, _| item.row_action(&item.servers()[0]));
        let ServerRowAction::DownloadJdk(args) = action else { panic!("Download JDK is offered: {action:?}") };
        let label = item.read_with(cx, |item, _| item.action_label(&item.servers()[0], &mut I18n::default()));
        assert_eq!(label, "Download JDK 25");
        assert_eq!((args.version, args.language_server), (25, Some(true)));
        click(cx, "server-fix-0");
        assert_eq!(*events.borrow(), [ServerStatusEvent::DownloadJdk(args)]);
        assert!(!item.read_with(cx, |item, _| item.is_open()));
    }

    #[gpui_kit::test]
    async fn says_blaze_mode_and_offers_deep_mode_while_language_servers_are_off(cx: &mut TestAppContext) {
        let (item, events, cx) = open(cx, ServerStatus::Stopped, None, true);
        let label = item.read_with(cx, |item, _| item.label(&mut I18n::default()));
        assert_eq!(label.as_deref(), Some("Language servers: Blaze mode"));
        click(cx, "server-status");
        cx.run_until_parked();
        assert!(cx.debug_bounds("server-status-popup").is_some());
        click(cx, "server-deep-mode");
        assert_eq!(*events.borrow(), [ServerStatusEvent::SwitchToDeepMode]);
    }

    #[gpui_kit::test]
    async fn keeps_restart_for_other_failures(cx: &mut TestAppContext) {
        let (item, events, cx) = open(cx, ServerStatus::Crashed, Some("exit code 1"), false);
        click(cx, "server-status");
        assert_eq!(
            item.read_with(cx, |item, _| item.row_action(&item.servers()[0])),
            ServerRowAction::Restart { enabled: true }
        );
        cx.run_until_parked();
        assert!(cx.debug_bounds("server-restart-0").is_some());
        assert!(cx.debug_bounds("server-fix-0").is_none(), "no Download JDK");
        let label = item.read_with(cx, |item, _| item.action_label(&item.servers()[0], &mut I18n::default()));
        assert_eq!(label, "Restart jdtls");
        click(cx, "server-restart-0");
        assert_eq!(*events.borrow(), [ServerStatusEvent::Restart(ServerId(1))]);
        // Restarting keeps the popup open, to watch the server come back.
        assert!(item.read_with(cx, |item, _| item.is_open()));
    }

    // Not from a spec: Install for a missing server, nothing without servers, the meta line.
    #[gpui_kit::test]
    async fn offers_install_for_a_missing_server_and_hides_without_servers(cx: &mut TestAppContext) {
        let (item, events, cx) = open(cx, ServerStatus::Missing, None, false);
        click(cx, "server-status");
        click(cx, "server-fix-0");
        assert_eq!(*events.borrow(), [ServerStatusEvent::Install("jdtls".to_owned())]);
        item.update(cx, |item, cx| {
            let mut servers = Servers::default();
            servers.server_event(ServerId(2), "gopls", ServerStatus::Ready, None, true);
            servers.snapshot(0, Vec::new(), true);
            item.set_servers(&servers, cx);
        });
        assert_eq!(item.read_with(cx, |item, _| item.summary()), None);
        cx.run_until_parked();
        assert!(cx.debug_bounds("server-status").is_none());
        let entry = ServerEntry {
            id: ServerId(3),
            name: "rust-analyzer".to_owned(),
            status: ServerStatus::Ready,
            message: None,
            languages: vec!["rust".to_owned()],
            root: Some("/p/crates/a".to_owned()),
        };
        item.update(cx, |item, cx| item.set_project_root(Some("/p".to_owned()), cx));
        assert_eq!(item.read_with(cx, |item, _| item.meta(&entry)).as_deref(), Some("rust · crates/a"));
    }
}
