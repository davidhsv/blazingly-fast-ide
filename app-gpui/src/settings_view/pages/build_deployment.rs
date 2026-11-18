//! Build, Execution, Deployment › Deployment: the servers project files are deployed to. The list
//! has + / − / copy / Use as Default; the server selected is edited below it on three tabs:
//! Connection (name, type, SSH configuration, root path, web server URL, Test Connection),
//! Mappings (project folders and the server folders they go to) and Excluded Paths (server paths
//! never touched).
//!
//! The servers and the default server are settings of the scope the page edits (the project's
//! first). Test Connection asks the backend with the server as edited, before Apply. When an SFTP
//! server's test fails on an unknown host key or a missing password or passphrase, the page offers
//! to trust the key or takes the secret in a [`SecretField`]; the secret leaves the view only as a
//! [`SecretText`](crate::database::store::SecretText) for the backend, which keeps it for the
//! session and, with Save, in the saved passwords. It is never staged, drawn, listed or logged.

mod connection;
mod paths;
#[cfg(test)]
mod proofs28_deployment_view;
#[cfg(test)]
mod proofs37_deployment_chooser_view;
mod remote_folder;

pub use super::ssh_backend::{HostTrust, SecretSetter};
use super::ssh_backend::{backend_set_secret, backend_trust_host, on_backend};
use super::tools_record_table::RecordTable;
use super::{Binding, ControlEvent, Then, new_field};
use crate::platform::i18n::Localization;
use crate::platform_more::remote_settings::{read_deployment_servers, with_unique_ids};
use crate::settings_model::deployment_model::{RemoteIds, fix_default_server, new_server};
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, ClickEvent, Context, Entity, EventEmitter, IntoElement, MouseButton,
    Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::{DeploymentServer, DeploymentType};
use ide_ssh::FsConnection;
pub use remote_folder::{BrowseTarget, DeploymentFolders, folder_lister};
use serde_json::Value;
use std::rc::Rc;

pub const PAGE: &str = "build.deployment";
/// The slot the editor fills.
pub const SLOT: &str = "Deployment servers";
const SERVERS: &str = "deployment.servers";
const DEFAULT_SERVER: &str = "deployment.defaultServer";

/// Tests that a server's folder can be reached: the connection, the folder (`~` for the login
/// folder), and the callback that takes the answer.
pub type Tester = Rc<dyn Fn(FsConnection, String, Then<Result<(), IdeError>>, &mut App)>;
/// What the page asks of the backend.
#[derive(Clone)]
pub struct Connections {
    /// None: Test Connection is unavailable.
    pub test: Option<Tester>,
    pub trust_host: HostTrust,
    pub set_secret: SecretSetter,
}

/// The connections the page uses, set by a test as a global; without one the backend's.
pub struct DeploymentConnections(pub Connections);

impl gpui_kit::Global for DeploymentConnections {}

/// The connections set as [`DeploymentConnections`], else the backend's. A test build gets no
/// Test Connection of its own making: an SFTP server's would connect to its host, so a test that
/// wants one sets it.
pub fn connections(cx: &App) -> Connections {
    match cx.try_global::<DeploymentConnections>() {
        Some(set) => set.0.clone(),
        None if cfg!(test) => Connections { test: None, ..backend_connections() },
        None => backend_connections(),
    }
}

/// The backend's Test Connection (`deploy_test_connection`), host key trust (`ssh_trust_host`) and
/// secret (`ssh_set_secret`), each called as their commands call them.
pub fn backend_connections() -> Connections {
    Connections { test: Some(backend_tester()), trust_host: backend_trust_host(), set_secret: backend_set_secret() }
}

/// The backend's Test Connection (`deploy_test_connection`), as its command calls it.
pub fn backend_tester() -> Tester {
    Rc::new(|connection: FsConnection, path: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        on_backend(
            move |host| async move { ide_host::ops::deploy::deploy_test_connection(&host, connection, path, None).await },
            then,
            cx,
        );
    })
}

/// `text` in the UI language; text built from a template (`Folder does not exist: …`) is
/// translated by the template.
fn tx_text(cx: &App, text: &str) -> String {
    match cx.try_global::<Localization>() {
        Some(localization) => localization.0.tx_template(text),
        None => text.to_owned(),
    }
}

/// The servers stored, each under an id of its own (a type deployment does not know reads as
/// SFTP); a value that is not a list of servers reads as none.
pub fn servers(binding: &Binding, cx: &App) -> Vec<DeploymentServer> {
    with_unique_ids(&read_deployment_servers(binding.value(SERVERS, cx)), "server")
}

/// Stages `next` as the servers, and the default server again when it is gone from them.
fn commit(binding: &Binding, next: &[DeploymentServer], cx: &mut App) {
    let stored = binding.text(DEFAULT_SERVER, cx);
    match serde_json::to_value(next) {
        Ok(value) => binding.stage(SERVERS, value, cx),
        Err(error) => {
            tracing::warn!("Settings: the deployment servers could not be staged: {error}");
            return;
        }
    }
    let fixed = fix_default_server(next, &stored);
    if fixed != stored {
        binding.stage(DEFAULT_SERVER, Value::String(fixed), cx);
    }
}

/// The tab of the selected server shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTab {
    Connection,
    Mappings,
    Excluded,
}

const TABS: [(ServerTab, &str, &str); 3] = [
    (ServerTab::Connection, "connection", "Connection"),
    (ServerTab::Mappings, "mappings", "Mappings"),
    (ServerTab::Excluded, "excluded", "Excluded Paths"),
];

/// Where Test Connection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestState {
    Busy,
    Connected,
    Failed { kind: ErrorKind, message: String },
}

/// The password or passphrase asked for after a failed test, and how its answer went.
struct Problem {
    secret: Entity<SecretField>,
    save: bool,
    busy: bool,
    failure: Option<String>,
}

/// A mapping row's three fields: local path, deployment path, web path.
struct MappingRow {
    fields: [Entity<TextField>; 3],
    _events: Vec<Subscription>,
}

pub struct DeploymentServersView {
    binding: Binding,
    /// The project's root, for the local folders of the mappings; none without a project.
    project_root: Option<String>,
    connections: Connections,
    ids: RemoteIds,
    selected: Option<String>,
    tab: ServerTab,
    /// The drop-down whose list is open (`kind`, `sshConfig`).
    open_select: Option<&'static str>,
    name: Entity<TextField>,
    root: Entity<TextField>,
    web: Entity<TextField>,
    /// The last test, under the type, SSH configuration and root path it was run with: it shows
    /// only while the server still has them.
    test: Option<(String, TestState)>,
    problem: Problem,
    mapping_selected: Option<usize>,
    /// The server whose mappings the rows show, and the rows.
    mapping_server: Option<String>,
    mapping_rows: Vec<MappingRow>,
    /// The selected server's excluded paths.
    excluded: Entity<RecordTable>,
    /// The folder chooser open under Root path or under the mappings, and the field it fills.
    chooser: Option<remote_folder::Chooser>,
    /// The window the editor was last drawn in, for the keyboard to go to the password field a
    /// failed test asks for.
    window: Option<AnyWindowHandle>,
    /// Whether the dialog shows this page.
    page_shown: bool,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for DeploymentServersView {}

impl DeploymentServersView {
    pub fn new(
        binding: Binding,
        project_root: Option<String>,
        connections: Connections,
        cx: &mut Context<Self>,
    ) -> Self {
        let first = servers(&binding, cx).into_iter().next();
        let mut subscriptions = Vec::new();
        let mut text_field = |name: &'static str, placeholder: Option<String>, cx: &mut Context<Self>| {
            let field = new_field("", placeholder, cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        view.update_current(cx, |server| match name {
                            "name" => server.name = text,
                            "rootPath" => server.root_path = text,
                            _ => server.web_url = text,
                        });
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
                }
            }));
            field
        };
        let name = text_field("name", None, cx);
        let root = text_field("rootPath", None, cx);
        let web = text_field("webUrl", Some("http://example.com".to_owned()), cx);
        let placeholder = tx(cx, "Password or passphrase");
        let secret = cx.new(|cx| SecretField::new(cx).with_placeholder(placeholder));
        subscriptions.push(cx.subscribe(&secret, |view: &mut Self, _, event: &SecretFieldEvent, cx| match event {
            SecretFieldEvent::Changed => cx.notify(),
            SecretFieldEvent::Submitted => view.connect(cx),
            SecretFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        }));
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let selected = first.map(|server| server.id);
        let excluded = Self::excluded_table(&binding, selected.clone(), cx);
        subscriptions.push(cx.subscribe(&excluded, |_: &mut Self, _, event: &ControlEvent, cx| cx.emit(event.clone())));
        let mut view = Self {
            binding,
            project_root,
            connections,
            ids: RemoteIds::default(),
            selected,
            tab: ServerTab::Connection,
            open_select: None,
            name,
            root,
            web,
            test: None,
            problem: Problem { secret, save: false, busy: false, failure: None },
            mapping_selected: None,
            mapping_server: None,
            mapping_rows: Vec::new(),
            excluded,
            chooser: None,
            window: None,
            page_shown: false,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn servers(&self, cx: &App) -> Vec<DeploymentServer> {
        servers(&self.binding, cx)
    }

    /// The server selected; none when the selection is gone from the list.
    pub fn current(&self, cx: &App) -> Option<DeploymentServer> {
        let id = self.selected.as_deref()?;
        self.servers(cx).into_iter().find(|server| server.id == id)
    }

    /// The id of the default server: the stored one while it is in the list, else the first.
    pub fn effective_default(&self, cx: &App) -> String {
        fix_default_server(&self.servers(cx), &self.binding.text(DEFAULT_SERVER, cx))
    }

    pub fn tab(&self) -> ServerTab {
        self.tab
    }

    pub fn test_state(&self, cx: &App) -> Option<TestState> {
        let server = self.current(cx)?;
        let (key, state) = self.test.as_ref()?;
        (*key == test_key(&server)).then(|| state.clone())
    }

    /// The text field of `name` (`name`, `rootPath`, `webUrl`).
    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        match name {
            "name" => Some(&self.name),
            "rootPath" => Some(&self.root),
            "webUrl" => Some(&self.web),
            _ => None,
        }
    }

    pub fn secret_field(&self) -> &Entity<SecretField> {
        &self.problem.secret
    }

    /// Changes the selected server.
    fn update_current(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut DeploymentServer)) {
        let Some(id) = self.selected.clone() else { return };
        let mut all = self.servers(cx);
        let Some(server) = all.iter_mut().find(|server| server.id == id) else { return };
        change(server);
        commit(&self.binding, &all, cx);
    }

    /// Selects a server: its tabs start over, with no test result and nothing typed.
    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.selected != id {
            self.test = None;
            self.reset_problem(cx);
            self.show_excluded_of(id.clone(), cx);
            self.close_chooser(cx);
        }
        self.selected = id;
        self.open_select = None;
        self.mapping_selected = None;
        self.sync(cx);
    }

    /// Shows `tab`; the Connection tab starts over when it is left, with no test result and
    /// nothing typed.
    pub fn show_tab(&mut self, tab: ServerTab, cx: &mut Context<Self>) {
        if self.tab == ServerTab::Connection && tab != ServerTab::Connection {
            self.test = None;
            self.reset_problem(cx);
        }
        if self.tab != tab {
            self.close_chooser(cx);
        }
        self.tab = tab;
        self.open_select = None;
        cx.notify();
    }

    /// Add (+): a new SFTP server with one empty mapping, or a copy of the selected one, added at the
    /// end, selected and shown on its Connection tab.
    pub fn add(&mut self, copy: bool, cx: &mut Context<Self>) {
        let mut all = self.servers(cx);
        let base = if copy { self.current(cx) } else { None };
        let id = self.ids.next("server", crate::platform::local_time::now_ms() as u64);
        let server = new_server(&all, base.as_ref(), id);
        let id = server.id.clone();
        all.push(server);
        commit(&self.binding, &all, cx);
        self.tab = ServerTab::Connection;
        self.select(Some(id), cx);
    }

    /// Remove (−): the selected server goes; the one now in its place (else the last) is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current(cx) else { return };
        let all = self.servers(cx);
        let index = all.iter().position(|server| server.id == current.id).unwrap_or_default();
        let rest: Vec<DeploymentServer> = all.into_iter().filter(|server| server.id != current.id).collect();
        commit(&self.binding, &rest, cx);
        let next = rest.get(index.min(rest.len().saturating_sub(1))).map(|server| server.id.clone());
        self.select(next, cx);
    }

    /// Use as Default: the selected server becomes the default one.
    pub fn use_as_default(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current(cx) else { return };
        if current.id != self.effective_default(cx) {
            self.binding.stage(DEFAULT_SERVER, Value::String(current.id), cx);
        }
    }

    /// The fields show the selected server; a field already showing its value is left alone.
    fn sync(&mut self, cx: &mut Context<Self>) {
        if self.follow_page(cx) {
            return;
        }
        let current = self.current(cx);
        let (name, root, web) = current
            .as_ref()
            .map(|server| (server.name.clone(), server.root_path.clone(), server.web_url.clone()))
            .unwrap_or_default();
        let wanted = [(self.name.clone(), name), (self.root.clone(), root), (self.web.clone(), web)];
        for (field, wanted) in wanted {
            if field.read(cx).text() != wanted {
                field.update(cx, |field, cx| field.set_text(&wanted, cx));
            }
        }
        let local = current.as_ref().is_some_and(|server| server.kind == DeploymentType::Local);
        let placeholder = tx(cx, if local { "Folder path" } else { "Login folder" });
        if self.root.read(cx).placeholder() != placeholder {
            self.root.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
        }
        self.sync_mappings(cx);
        self.drop_stale_chooser(cx);
        cx.notify();
    }

    /// The page opens each time the dialog shows it, as a page of its own: the first server
    /// selected on its Connection tab, nothing tested or typed; leaving the page drops the test
    /// result and the password typed. Whether it selected (and synced).
    fn follow_page(&mut self, cx: &mut Context<Self>) -> bool {
        let shown = self.binding.model.read(cx).page_id == self.binding.page;
        if shown == self.page_shown {
            return false;
        }
        self.page_shown = shown;
        self.test = None;
        self.reset_problem(cx);
        self.close_chooser(cx);
        if !shown {
            return false;
        }
        self.tab = ServerTab::Connection;
        let first = self.servers(cx).into_iter().next().map(|server| server.id);
        self.select(first, cx);
        true
    }

    /// The server list as text: each server (`>` the selected one) with `(default)` after the
    /// default one, or the empty text.
    fn list_lines(&self, cx: &App) -> Vec<String> {
        let all = self.servers(cx);
        let default = self.effective_default(cx);
        let mut out = vec![format!("{}:", tx(cx, SLOT))];
        if all.is_empty() {
            out.push(format!("  {}", tx(cx, "No servers. Click + to add one.")));
        }
        for server in &all {
            let mark = if self.selected.as_deref() == Some(server.id.as_str()) { ">" } else { " " };
            let shown = if server.name.is_empty() { &server.id } else { &server.name };
            let default = if server.id == default { format!(" ({})", tx(cx, "default")) } else { String::new() };
            out.push(format!("{mark} {shown}{default}"));
        }
        out
    }

    /// The editor as text: the list, then the tabs (`[Connection]` the one shown) and the tab's
    /// controls, or the note shown without a server selected. A secret is never listed.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let mut out = self.list_lines(cx);
        let Some(server) = self.current(cx) else {
            out.push(tx(cx, "Add an SFTP server to upload, download and synchronize project files."));
            return out;
        };
        let tabs: Vec<String> = TABS
            .iter()
            .map(|(tab, _, label)| if *tab == self.tab { format!("[{}]", tx(cx, label)) } else { tx(cx, label) })
            .collect();
        out.push(tabs.join(" "));
        match self.tab {
            ServerTab::Connection => out.extend(self.connection_lines(&server, cx)),
            ServerTab::Mappings => out.extend(self.mapping_lines(&server, cx)),
            ServerTab::Excluded => out.extend(self.excluded_lines(&server, cx)),
        }
        out
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let current = self.current(cx);
        let has = current.is_some();
        let can_default = current.as_ref().is_some_and(|server| server.id != self.effective_default(cx));
        let add = tool(cx, "settings-deployment-add", "plus", true, "Add")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(false, cx)));
        let remove = tool(cx, "settings-deployment-remove", "minus", has, "Remove")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        let copy = tool(cx, "settings-deployment-copy", "copy", has, "Copy")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(true, cx))));
        let default = tool(cx, "settings-deployment-default", "check", can_default, "Use as Default")
            .when(can_default, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.use_as_default(cx))));
        toolbar_strip(cx).child(add).child(remove).child(copy).child(default)
    }

    fn server_list(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let all = self.servers(cx);
        let default = self.effective_default(cx);
        let selected_bg = theme::color(cx, "bg-selected");
        let rows: Vec<AnyElement> = all
            .iter()
            .enumerate()
            .map(|(index, server)| {
                let id = server.id.clone();
                let selector = format!("settings-deployment-server-{index}");
                let selected = self.selected.as_deref() == Some(server.id.as_str());
                let is_default = server.id == default;
                let shown = if server.name.is_empty() { server.id.clone() } else { server.name.clone() };
                let badge = is_default.then(|| {
                    div()
                        .px(px(6.0))
                        .rounded(theme::length(cx, "radius-small"))
                        .bg(theme::color(cx, "accent-soft"))
                        .text_color(theme::color(cx, "accent"))
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(tx(cx, "default")))
                });
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .h(px(super::ROW_HEIGHT - 4.0))
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .when(selected, |el| el.bg(selected_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |view, _, _, cx| view.select(Some(id.clone()), cx)),
                    )
                    .child(theme::icon(cx, "web", 16.0, theme::color(cx, "fg-icon")))
                    .child(
                        div()
                            .min_w_0()
                            .whitespace_nowrap()
                            .when(is_default, |el| el.text_color(theme::color(cx, "fg-strong")))
                            .child(SharedString::from(shown)),
                    )
                    .children(badge)
                    .into_any_element()
            })
            .collect();
        let empty = all.is_empty().then(|| {
            let text = tx(cx, "No servers. Click + to add one.");
            div().px(px(6.0)).py(px(4.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(text))
        });
        div()
            .max_w(px(560.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .child(self.toolbar(cx))
            .child(div().min_h(px(96.0)).flex().flex_col().children(rows).children(empty))
    }

    fn tab_strip(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let tabs: Vec<AnyElement> = TABS
            .iter()
            .map(|(tab, name, label)| {
                let (tab, active) = (*tab, *tab == self.tab);
                let selector = format!("settings-deployment-tab-{name}");
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .px(px(8.0))
                    .h(theme::length(cx, "h-row"))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(if active { theme::color(cx, "accent") } else { theme::color(cx, "border") })
                    .text_color(theme::color(cx, if active { "fg-strong" } else { "fg-subtle" }))
                    .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                    .child(SharedString::from(tx(cx, label)))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.show_tab(tab, cx)))
                    .into_any_element()
            })
            .collect();
        div().flex().gap(px(2.0)).children(tabs)
    }
}

/// The test of a server holds while its type, SSH configuration, FTP login and root path stay the
/// same.
fn test_key(server: &DeploymentServer) -> String {
    let kind = match server.kind {
        DeploymentType::Sftp => "sftp",
        DeploymentType::Ftp => "ftp",
        DeploymentType::Ftps => "ftps",
        DeploymentType::Local => "local",
    };
    let ftp = &server.ftp;
    let ftp = format!("{}:{}:{}:{}", ftp.host, ftp.port.unwrap_or(21), ftp.user, ftp.passive);
    format!("{kind}:{}:{ftp}:{}", server.ssh_config, server.root_path)
}

/// A toolbar's strip of icon buttons.
fn toolbar_strip(cx: &App) -> gpui_kit::Div {
    div().flex().gap(px(2.0)).p(px(2.0)).border_b_1().border_color(theme::color(cx, "border"))
}

/// An icon button of a toolbar, faint while it cannot be used.
fn tool(cx: &App, selector: &str, icon: &str, enabled: bool, title: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    let selector = selector.to_owned();
    let color = theme::color(cx, if enabled { "fg-icon" } else { "fg-faint" });
    div()
        .id(SharedString::from(selector.clone()))
        .debug_selector(move || selector)
        .size(px(22.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(enabled, |el| el.hover(|el| el.bg(theme::color(cx, "bg-toolbar-hover"))))
        .tooltip(crate::ui::tooltip::tooltip(title))
        .child(theme::icon(cx, icon, 16.0, color))
}

impl Render for DeploymentServersView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let list = self.server_list(cx);
        let detail = match self.current(cx) {
            Some(server) => {
                let body = match self.tab {
                    ServerTab::Connection => self.connection_tab(&server, cx),
                    ServerTab::Mappings => self.mappings_tab(&server, cx),
                    ServerTab::Excluded => self.excluded_tab(&server, cx),
                };
                div().pt(px(8.0)).flex().flex_col().gap(px(4.0)).child(self.tab_strip(cx)).child(body)
            }
            None => {
                let note = tx(cx, "Add an SFTP server to upload, download and synchronize project files.");
                div().pt(px(6.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note))
            }
        };
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            // A press anywhere else in the editor closes its open list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.open_select.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(list)
            .child(detail)
    }
}
