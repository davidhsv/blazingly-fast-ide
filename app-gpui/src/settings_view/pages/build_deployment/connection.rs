//! The Connection tab of a deployment server: its name, type, SSH configuration, root path and web
//! server URL, and Test Connection with what a failed SFTP test asks of the user (trust the host
//! key, or type the password or passphrase).

use super::{DeploymentServersView, TestState, test_key, tx_text};
use crate::database::store::SecretText;
use crate::platform_more::remote_settings::{deployment_connection, with_unique_ids};
use crate::settings_model::ssh_connection::{SshTestRemedy, ssh_test_remedy};
use crate::settings_model::staging::Scope;
use crate::settings_view::controls::{button, check_box, search_label, tx};
use crate::settings_view::pages::{
    ControlEvent, MEDIUM_FIELD, ROW_HEIGHT, Then, WIDE_FIELD, drop_down, drop_down_row, frame, hint_line, muted, row,
    row_label, select_box,
};
use crate::theme;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, Focusable, IntoElement, MouseButton, SharedString, div, prelude::*,
    px,
};
use ide_core::IdeError;
use ide_core::settings::{DeploymentServer, DeploymentType, SshAuth, SshConfiguration};
use std::rc::Rc;

/// The SSH configuration the page edits on its own page: always the global ones.
const SSH_PAGE: &str = "tools.sshConfigurations";
const ROOT_HINT: &str = "Mappings' deployment paths are relative to this folder.";
const KINDS: [(DeploymentType, &str, &str); 2] =
    [(DeploymentType::Sftp, "sftp", "SFTP"), (DeploymentType::Local, "local", "Local or mounted folder")];

/// One choice of a drop-down: its value, label, the detail after it and whether it can be chosen.
struct Choice {
    value: String,
    label: String,
    detail: Option<String>,
    enabled: bool,
}

/// The SSH configurations of the global settings as the dialog shows them, each with an id.
fn ssh_configs(view: &DeploymentServersView, cx: &App) -> Vec<SshConfiguration> {
    let value = view.binding.model.read(cx).staged_value(Scope::Global, "ssh.configs");
    let configs: Vec<SshConfiguration> =
        value.filter(|value| !value.is_null()).and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
    with_unique_ids(&configs, "ssh")
}

/// A configuration's name, else what it connects to.
fn config_label(config: &SshConfiguration) -> String {
    let name = config.name.trim();
    if name.is_empty() { config.label() } else { name.to_owned() }
}

/// The choices of SSH configuration: every configuration; one stored that is gone, marked
/// missing; without any, one that cannot be chosen.
fn ssh_choices(configs: &[SshConfiguration], current: &str, cx: &App) -> Vec<Choice> {
    let mut out: Vec<Choice> = configs
        .iter()
        .map(|config| Choice {
            value: config.id.clone(),
            label: config_label(config),
            detail: Some(config.label()),
            enabled: true,
        })
        .collect();
    if !current.is_empty() && !configs.iter().any(|config| config.id == current) {
        let label = tx_text(cx, &format!("{current} (missing)"));
        out.push(Choice { value: current.to_owned(), label, detail: None, enabled: true });
    }
    if out.is_empty() {
        let label = tx(cx, "No SSH configurations");
        out.push(Choice { value: String::new(), label, detail: None, enabled: false });
    }
    out
}

/// The label a drop-down shows for `value`.
fn shown_choice(choices: &[Choice], value: &str) -> String {
    let found = choices.iter().find(|choice| choice.value == value);
    found.map(|choice| choice.label.clone()).unwrap_or_default()
}

impl DeploymentServersView {
    /// Test Connection can run: an SFTP server needs its SSH configuration, a local one its folder,
    /// and no test of the server is running.
    pub fn can_test(&self, server: &DeploymentServer, cx: &App) -> bool {
        let ready = match server.kind {
            DeploymentType::Sftp => !server.ssh_config.is_empty(),
            DeploymentType::Ftp | DeploymentType::Ftps => !server.ftp.host.trim().is_empty(),
            DeploymentType::Local => !server.root_path.trim().is_empty(),
        };
        ready && self.connections.test.is_some() && self.test_state(cx) != Some(TestState::Busy)
    }

    /// Test Connection: asks the backend whether the root path (`~` while it is empty) of the
    /// server as edited can be reached.
    pub fn run_test(&mut self, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx) else { return };
        if !self.can_test(&server, cx) {
            return;
        }
        let key = test_key(&server);
        self.test = Some((key.clone(), TestState::Busy));
        self.reset_problem(cx);
        let connection = deployment_connection(&server);
        let root = server.root_path.trim();
        let path = if root.is_empty() { "~".to_owned() } else { root.to_owned() };
        let view = cx.weak_entity();
        let window = self.window;
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            let state = match result {
                Ok(()) => TestState::Connected,
                Err(error) => TestState::Failed { kind: error.kind, message: error.message },
            };
            let key = key.clone();
            let focus = view
                .update(cx, |view, cx| {
                    // A test the server or the tab left meanwhile has no one to tell.
                    if !view.test.as_ref().is_some_and(|(running, now)| *running == key && *now == TestState::Busy) {
                        return None;
                    }
                    view.test = Some((key, state));
                    cx.notify();
                    let asks = view.current(cx).and_then(|server| view.remedy(&server, cx));
                    (asks == Some(SshTestRemedy::Secret)).then(|| view.problem.secret.focus_handle(cx))
                })
                .ok()
                .flatten();
            // The password field that appears takes the keyboard, as it does when it is first drawn.
            if let (Some(focus), Some(window)) = (focus, window) {
                window.update(cx, |_, window, cx| window.focus(&focus, cx)).ok();
            }
        });
        let Some(test) = self.connections.test.clone() else { return };
        // After this turn: a tester that answers at once finds the view free to update.
        cx.defer(move |cx| test(connection, path, then, cx));
        cx.notify();
    }

    /// A new test starts with an empty secret and nothing failed.
    pub(super) fn reset_problem(&mut self, cx: &mut Context<Self>) {
        self.problem.secret.update(cx, |field, cx| field.clear(cx));
        self.problem.save = false;
        self.problem.busy = false;
        self.problem.failure = None;
    }

    /// What the failed test of an SFTP server asks for, by its SSH configuration's authentication.
    fn remedy(&self, server: &DeploymentServer, cx: &App) -> Option<SshTestRemedy> {
        if server.kind != DeploymentType::Sftp {
            return None;
        }
        let Some(TestState::Failed { kind, message }) = self.test_state(cx) else { return None };
        let configs = ssh_configs(self, cx);
        let auth = configs.iter().find(|config| config.id == server.ssh_config).map_or(SshAuth::OpenSsh, |c| c.auth);
        let kind = serde_json::to_value(kind).ok().and_then(|kind| kind.as_str().map(str::to_owned));
        ssh_test_remedy(kind.as_deref(), &message, auth)
    }

    /// Runs `step` for the server's SSH configuration, then the test again; a failure is shown
    /// under the problem.
    fn remedy_step(
        &mut self,
        step: impl FnOnce(String, Then<Result<(), IdeError>>, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(server) = self.current(cx) else { return };
        if self.problem.busy {
            return;
        }
        self.problem.busy = true;
        self.problem.failure = None;
        let view = cx.weak_entity();
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| {
                view.problem.busy = false;
                match result {
                    Ok(()) => view.run_test(cx),
                    Err(error) => view.problem.failure = Some(tx_text(cx, &error.message)),
                }
                cx.notify();
            })
            .ok();
        });
        let ssh = server.ssh_config;
        cx.defer(move |cx| step(ssh, then, cx));
        cx.notify();
    }

    /// Trust Host Key and Retry.
    pub fn trust_and_retry(&mut self, cx: &mut Context<Self>) {
        let trust = self.connections.trust_host.clone();
        self.remedy_step(move |ssh, then, cx| trust(ssh, then, cx), cx);
    }

    /// Connect: hands the backend the secret typed (saved when Save is ticked) and tests again.
    pub(super) fn connect(&mut self, cx: &mut Context<Self>) {
        let typed = self.problem.secret.read(cx).text(cx).to_owned();
        if typed.is_empty() || self.problem.busy {
            return;
        }
        let secret = SecretText::from(typed);
        let save = self.problem.save;
        let set_secret = self.connections.set_secret.clone();
        self.remedy_step(move |ssh, then, cx| set_secret(ssh, secret, save, then, cx), cx);
    }

    fn choose(&mut self, key: &'static str, value: String, cx: &mut Context<Self>) {
        self.open_select = None;
        match key {
            "kind" => {
                let kind = if value == "local" { DeploymentType::Local } else { DeploymentType::Sftp };
                self.update_current(cx, |server| server.kind = kind);
            }
            _ => self.update_current(cx, |server| server.ssh_config = value),
        }
        cx.notify();
    }

    fn kind_choices(cx: &App) -> Vec<Choice> {
        KINDS
            .iter()
            .map(|(_, value, label)| Choice {
                value: (*value).to_owned(),
                label: tx(cx, label),
                detail: None,
                enabled: true,
            })
            .collect()
    }

    fn kind_value(server: &DeploymentServer) -> &'static str {
        KINDS.iter().find(|(kind, _, _)| *kind == server.kind).map_or("sftp", |(_, value, _)| *value)
    }

    /// The tab as text: `Label: text` rows, the drop-downs as `Label: choice ▾`, the root path's
    /// hint, `[Test Connection]` with `(disabled)`, and what the last test came to. The secret is
    /// listed as bullets.
    pub(super) fn connection_lines(&self, server: &DeploymentServer, cx: &App) -> Vec<String> {
        let text = |field: &Entity<crate::ui::text_field::TextField>| field.read(cx).text().to_owned();
        let flag = |open: bool| if open { "" } else { " (disabled)" };
        let mut out = vec![format!("{}: {}", tx(cx, "Name"), text(&self.name))];
        let kind = shown_choice(&Self::kind_choices(cx), Self::kind_value(server));
        out.push(format!("{}: {kind} ▾", tx(cx, "Type")));
        if server.kind == DeploymentType::Sftp {
            let choices = ssh_choices(&ssh_configs(self, cx), &server.ssh_config, cx);
            let shown = shown_choice(&choices, &server.ssh_config);
            out.push(format!("{}: {shown} ▾ [{}]", tx(cx, "SSH configuration"), tx(cx, "Configure…")));
        }
        out.push(format!("{}: {}", tx(cx, "Root path"), text(&self.root)));
        out.push(format!("    {}", tx(cx, ROOT_HINT)));
        out.push(format!("{}: {}", tx(cx, "Web server URL"), text(&self.web)));
        let state = self.test_state(cx);
        let busy = if state == Some(TestState::Busy) { " …" } else { "" };
        out.push(format!("[{}]{}{busy}", tx(cx, "Test Connection"), flag(self.can_test(server, cx))));
        match state {
            Some(TestState::Connected) => out.push(tx(cx, "Connected successfully")),
            Some(TestState::Failed { message, .. }) => {
                out.push(format!("! {}", tx_text(cx, &message)));
                match self.remedy(server, cx) {
                    Some(SshTestRemedy::TrustHostKey) => {
                        out.push(format!("[{}]{}", tx(cx, "Trust Host Key and Retry"), flag(!self.problem.busy)));
                    }
                    Some(SshTestRemedy::Secret) => {
                        let bullets = self.problem.secret.read(cx).shown(cx);
                        let mark = if self.problem.save { "x" } else { " " };
                        let can = !self.problem.busy && !bullets.is_empty();
                        out.push(format!(
                            "{}: {bullets} [{mark}] {} [{}]{}",
                            tx(cx, "Password or passphrase"),
                            tx(cx, "Save"),
                            tx(cx, "Connect"),
                            flag(can)
                        ));
                    }
                    None => {}
                }
                out.extend(self.problem.failure.as_ref().map(|failure| format!("! {failure}")));
            }
            Some(TestState::Busy) | None => {}
        }
        out
    }

    /// A drop-down of the tab: the choice shown with a chevron, and its list while open.
    fn drop_down_control(
        &self,
        key: &'static str,
        shown: &str,
        choices: Vec<Choice>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = match key {
            "kind" => self.current(cx).map(|server| Self::kind_value(&server).to_owned()),
            _ => self.current(cx).map(|server| server.ssh_config),
        };
        let boxed = select_box(cx, shown)
            .id(SharedString::from(format!("settings-select-deployment.{key}")))
            .debug_selector(move || format!("settings-select-deployment.{key}"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                view.open_select = if view.open_select == Some(key) { None } else { Some(key) };
                cx.notify();
            }));
        let open = (self.open_select == Some(key)).then(|| {
            let rows: Vec<AnyElement> = choices
                .into_iter()
                .enumerate()
                .map(|(index, choice)| {
                    let chosen = current.as_deref() == Some(choice.value.as_str());
                    let suffix = if key == "kind" { choice.value.clone() } else { index.to_string() };
                    let detail = choice.detail.as_ref().map(|detail| muted(cx, detail));
                    let value = choice.value;
                    drop_down_row(cx, format!("settings-choice-deployment.{key}-{suffix}"), &choice.label, chosen)
                        .when(!choice.enabled, |el| el.opacity(0.5))
                        .children(detail)
                        .when(choice.enabled, |el| {
                            el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                view.choose(key, value.clone(), cx);
                            }))
                        })
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        div().relative().child(boxed).children(open).into_any_element()
    }

    /// `Label [field]` for one of the server's text fields.
    fn text_row(
        &self,
        label: &str,
        name: &'static str,
        field: &Entity<crate::ui::text_field::TextField>,
        mono: bool,
        query: &str,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
        let text = tx(cx, label);
        let selector = format!("settings-text-deployment.{name}");
        row(cx, self.binding.matches(label, cx)).child(row_label(cx, &text, query)).child(
            frame(cx, WIDE_FIELD, false)
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector)
                .when(mono, |el| el.font_family("monospace"))
                .child(field.clone()),
        )
    }

    /// A text button of the tab, drawn as `selector`, running `action` while it can be used.
    fn action_button(
        &self,
        selector: &'static str,
        label: &str,
        enabled: bool,
        action: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> gpui_kit::Stateful<gpui_kit::Div> {
        let text = tx(cx, label);
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .child(button(cx, &text, false, enabled))
            .when(enabled, |el| el.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| action(view, cx))))
    }

    /// What a failed test of an SFTP server shows: the error, then the remedy it offers.
    fn problem_block(&self, server: &DeploymentServer, message: &str, cx: &mut Context<Self>) -> gpui_kit::Div {
        let query = self.binding.query(cx);
        let error = div()
            .debug_selector(|| "settings-deployment-test-result".to_owned())
            .text_color(theme::color(cx, "danger"))
            .child(SharedString::from(tx_text(cx, message)));
        let remedy = match self.remedy(server, cx) {
            Some(SshTestRemedy::TrustHostKey) => Some(
                div()
                    .flex()
                    .child(self.action_button(
                        "settings-deployment-trust",
                        "Trust Host Key and Retry",
                        !self.problem.busy,
                        Self::trust_and_retry,
                        cx,
                    ))
                    .into_any_element(),
            ),
            Some(SshTestRemedy::Secret) => {
                let typed = !self.problem.secret.read(cx).text(cx).is_empty();
                let save_label = tx(cx, "Save");
                let save = div()
                    .id("settings-toggle-deployment.saveSecret")
                    .debug_selector(|| "settings-toggle-deployment.saveSecret".to_owned())
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .child(check_box(cx, self.problem.save))
                    .child(search_label(cx, &save_label, &query))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.problem.save = !view.problem.save;
                        cx.notify();
                    }));
                let connect = self.action_button(
                    "settings-deployment-connect",
                    "Connect",
                    !self.problem.busy && typed,
                    Self::connect,
                    cx,
                );
                Some(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            frame(cx, MEDIUM_FIELD, false)
                                .id("settings-secret-deployment.secret")
                                .debug_selector(|| "settings-secret-deployment.secret".to_owned())
                                .child(self.problem.secret.clone()),
                        )
                        .child(save)
                        .child(connect)
                        .into_any_element(),
                )
            }
            None => None,
        };
        let failure = self
            .problem
            .failure
            .as_ref()
            .map(|failure| div().text_color(theme::color(cx, "danger")).child(SharedString::from(failure.clone())));
        div().px(px(4.0)).flex().flex_col().gap(px(4.0)).child(error).children(remedy).children(failure)
    }

    pub(super) fn connection_tab(&mut self, server: &DeploymentServer, cx: &mut Context<Self>) -> AnyElement {
        let query = self.binding.query(cx);
        let name = self.text_row("Name", "name", &self.name.clone(), false, &query, cx);
        let kind_label = tx(cx, "Type");
        let kinds = Self::kind_choices(cx);
        let kind_shown = shown_choice(&kinds, Self::kind_value(server));
        let kind = row(cx, self.binding.matches("Type", cx))
            .child(row_label(cx, &kind_label, &query))
            .child(self.drop_down_control("kind", &kind_shown, kinds, cx));
        let ssh = (server.kind == DeploymentType::Sftp).then(|| {
            let choices = ssh_choices(&ssh_configs(self, cx), &server.ssh_config, cx);
            let shown = shown_choice(&choices, &server.ssh_config);
            let label = tx(cx, "SSH configuration");
            let configure = div()
                .id("settings-deployment-configure-ssh")
                .debug_selector(|| "settings-deployment-configure-ssh".to_owned())
                .text_color(theme::color(cx, "link"))
                .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                .child(SharedString::from(tx(cx, "Configure…")))
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                    cx.emit(ControlEvent::OpenPage { page: SSH_PAGE, query: None });
                }));
            row(cx, self.binding.matches("SSH configuration", cx))
                .child(row_label(cx, &label, &query))
                .child(self.drop_down_control("sshConfig", &shown, choices, cx))
                .child(configure)
        });
        let root = self.text_row("Root path", "rootPath", &self.root.clone(), true, &query, cx);
        let web = self.text_row("Web server URL", "webUrl", &self.web.clone(), false, &query, cx);
        let state = self.test_state(cx);
        let busy = (state == Some(TestState::Busy))
            .then(|| theme::icon(cx, "refresh", 14.0, theme::color(cx, "fg-subtle")).into_any_element());
        let connected = (state == Some(TestState::Connected)).then(|| {
            div()
                .debug_selector(|| "settings-deployment-test-result".to_owned())
                .text_color(theme::color(cx, "success"))
                .child(SharedString::from(tx(cx, "Connected successfully")))
        });
        let can_test = self.can_test(server, cx);
        let test = self.action_button("settings-deployment-test", "Test Connection", can_test, Self::run_test, cx);
        let test_row =
            div().min_h(px(ROW_HEIGHT)).px(px(4.0)).flex().items_center().gap(px(8.0)).child(test).children(busy);
        let failed = match state {
            Some(TestState::Failed { message, .. }) => Some(match server.kind {
                DeploymentType::Sftp => self.problem_block(server, &message, cx).into_any_element(),
                DeploymentType::Ftp | DeploymentType::Ftps | DeploymentType::Local => div()
                    .px(px(4.0))
                    .debug_selector(|| "settings-deployment-test-result".to_owned())
                    .text_color(theme::color(cx, "danger"))
                    .child(SharedString::from(tx_text(cx, &message)))
                    .into_any_element(),
            }),
            _ => None,
        };
        div()
            .flex()
            .flex_col()
            .child(name)
            .child(kind)
            .children(ssh)
            .child(root)
            .child(div().pl(px(4.0)).child(hint_line(cx, &tx(cx, ROOT_HINT), 0.0)))
            .child(web)
            .child(test_row.children(connected))
            .children(failed)
            .into_any_element()
    }
}
