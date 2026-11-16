//! The Connection form of the SSH configuration selected: name, host and port, user name, the
//! authentication type, the private key file, the password or passphrase with its Save checkbox
//! (or, with OpenSSH, what is read from `~/.ssh/config`), and Test Connection with what a failure
//! asks of the user (trust the host key, or type the secret and connect again).

use super::{AUTHS, SshConfigurationsView, TestState, auth_label};
use crate::settings_model::ssh_connection::SshTestRemedy;
use crate::settings_view::controls::{button, check_box, search_label, section, tx};
use crate::settings_view::pages::{
    MEDIUM_FIELD, NUMBER_FIELD, ROW_HEIGHT, WIDE_FIELD, drop_down, drop_down_row, frame, icon_button, row, row_label,
    select_box,
};
use crate::theme;
use gpui_kit::{AnyElement, App, ClickEvent, Context, IntoElement, MouseButton, SharedString, div, prelude::*, px};
use ide_core::settings::{SshAuth, SshConfiguration};

const OPEN_SSH_NOTE: &str =
    "Uses HostName, User, Port, IdentityFile and ProxyJump from ~/.ssh/config, and keys of the SSH agent.";
const TRUST_NOTE: &str =
    "The authenticity of the host cannot be established. Trust it and add the key to ~/.ssh/known_hosts?";

/// The label of the secret row and of its Save checkbox, by authentication.
fn secret_labels(auth: SshAuth) -> (&'static str, &'static str) {
    match auth {
        SshAuth::Password => ("Password", "Save password"),
        _ => ("Passphrase", "Save passphrase"),
    }
}

impl SshConfigurationsView {
    /// What the last test shows beside Test Connection: the text and whether it is an error.
    fn status(&self, cx: &App) -> Option<(String, bool)> {
        match &self.test {
            TestState::Idle => None,
            TestState::Running => Some((tx(cx, "Connecting…"), false)),
            TestState::Connected(text) => Some((text.clone(), false)),
            TestState::Failed { message, .. } => Some((message.clone(), true)),
        }
    }

    /// The remedy the last failed test offers for `config`: Connect only where a secret is typed.
    fn remedy(&self, config: &SshConfiguration) -> Option<SshTestRemedy> {
        match &self.test {
            TestState::Failed { remedy: Some(SshTestRemedy::Secret), .. } if config.auth == SshAuth::OpenSsh => None,
            TestState::Failed { remedy, .. } => *remedy,
            _ => None,
        }
    }

    /// The form as text: `— Connection —`, `Label: text` rows (`Host: h Port: p`, `(invalid)` after
    /// a Port that is not one), `Authentication type: label ▾`, the secret as bullets (or its
    /// placeholder in parentheses), `[x] Save …` or the OpenSSH note, `[Test Connection]` with
    /// `(disabled)`, and what the last test came to (`! ` before an error) with its remedy.
    pub(super) fn form_lines(&self, config: &SshConfiguration, cx: &App) -> Vec<String> {
        let text = |name: &str| self.field_text(name, cx);
        let mut out = vec![format!("— {} —", tx(cx, "Connection"))];
        out.push(format!("{}: {}", tx(cx, "Name"), text("name")));
        let invalid = if self.port_invalid(cx) { " (invalid)" } else { "" };
        out.push(format!("{}: {} {} {}{invalid}", tx(cx, "Host"), text("host"), tx(cx, "Port:"), text("port")));
        out.push(format!("{}: {}", tx(cx, "User name"), text("user")));
        out.push(format!("{}: {} ▾", tx(cx, "Authentication type"), tx(cx, auth_label(config.auth))));
        if config.auth == SshAuth::KeyPair {
            out.push(format!("{}: {} [{}]", tx(cx, "Private key file"), text("keyFile"), tx(cx, "Browse…")));
        }
        if config.auth == SshAuth::OpenSsh {
            out.push(tx(cx, OPEN_SSH_NOTE));
        } else {
            let (label, save) = secret_labels(config.auth);
            let bullets = self.secret.read(cx).shown(cx);
            let shown = if bullets.is_empty() && !self.secret_placeholder.is_empty() {
                format!("({})", self.secret_placeholder)
            } else {
                bullets
            };
            out.push(format!("{}: {shown}", tx(cx, label)));
            let mark = if config.save_secret { "x" } else { " " };
            out.push(format!("[{mark}] {}", tx(cx, save)));
        }
        let disabled = if self.can_test(cx) { "" } else { " (disabled)" };
        out.push(format!("[{}]{disabled}", tx(cx, "Test Connection")));
        if let Some((status, error)) = self.status(cx) {
            out.push(if error { format!("! {status}") } else { status });
        }
        match self.remedy(config) {
            Some(SshTestRemedy::TrustHostKey) => {
                out.push(format!("{} [{}]", tx(cx, TRUST_NOTE), tx(cx, "Trust Host and Retry")));
            }
            Some(SshTestRemedy::Secret) => out.push(format!("[{}]", tx(cx, "Connect"))),
            None => {}
        }
        out
    }

    /// `Label [field]` for one of the configuration's text fields.
    fn text_row(&self, label: &str, name: &'static str, width: f32, mono: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(field) = self.field(name).cloned() else { return div().into_any_element() };
        let query = self.binding.query(cx);
        let text = tx(cx, label);
        let selector = format!("settings-text-ssh.{name}");
        row(cx, self.binding.matches(label, cx))
            .child(row_label(cx, &text, &query))
            .child(
                frame(cx, width, false)
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector)
                    .when(mono, |el| el.font_family("monospace"))
                    .child(field),
            )
            .into_any_element()
    }

    /// `Host [host] Port: [port]`, the port flagged while it is not one.
    fn host_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let (Some(host), Some(port)) = (self.field("host").cloned(), self.field("port").cloned()) else {
            return div().into_any_element();
        };
        let query = self.binding.query(cx);
        let label = tx(cx, "Host");
        let port_label = tx(cx, "Port:");
        let invalid = self.port_invalid(cx);
        row(cx, self.binding.matches("Host", cx))
            .child(row_label(cx, &label, &query))
            .child(
                frame(cx, MEDIUM_FIELD, false)
                    .id("settings-text-ssh.host")
                    .debug_selector(|| "settings-text-ssh.host".to_owned())
                    .child(host),
            )
            .child(search_label(cx, &port_label, &query))
            .child(
                frame(cx, NUMBER_FIELD, invalid)
                    .id("settings-text-ssh.port")
                    .debug_selector(|| "settings-text-ssh.port".to_owned())
                    .child(port),
            )
            .into_any_element()
    }

    /// `Authentication type [choice ▾]`, and its list while open.
    fn auth_row(&self, config: &SshConfiguration, cx: &mut Context<Self>) -> AnyElement {
        let query = self.binding.query(cx);
        let label = tx(cx, "Authentication type");
        let shown = tx(cx, auth_label(config.auth));
        let boxed = select_box(cx, &shown)
            .id("settings-select-ssh.auth")
            .debug_selector(|| "settings-select-ssh.auth".to_owned())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.auth_open = !view.auth_open;
                cx.notify();
            }));
        let list = self.auth_open.then(|| {
            let rows: Vec<AnyElement> = AUTHS
                .iter()
                .map(|&(auth, name, text)| {
                    let text = tx(cx, text);
                    drop_down_row(cx, format!("settings-choice-ssh.auth-{name}"), &text, auth == config.auth)
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_auth(auth, cx)))
                        .into_any_element()
                })
                .collect();
            drop_down(cx, rows)
        });
        row(cx, self.binding.matches("Authentication type", cx))
            .child(row_label(cx, &label, &query))
            .child(div().relative().child(boxed).children(list))
            .into_any_element()
    }

    /// `Private key file [path] [Browse…]`.
    fn key_file_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(field) = self.field("keyFile").cloned() else { return div().into_any_element() };
        let query = self.binding.query(cx);
        let label = tx(cx, "Private key file");
        let browse = icon_button(cx, "settings-ssh-browse-key".to_owned(), "folder-open", "Browse…")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse_key(cx)));
        row(cx, self.binding.matches("Private key file", cx))
            .child(row_label(cx, &label, &query))
            .child(
                frame(cx, WIDE_FIELD, false)
                    .id("settings-text-ssh.keyFile")
                    .debug_selector(|| "settings-text-ssh.keyFile".to_owned())
                    .font_family("monospace")
                    .child(field),
            )
            .child(browse)
            .into_any_element()
    }

    /// The password or passphrase field and its Save checkbox; with OpenSSH, the note on what is
    /// read instead.
    fn secret_rows(&self, config: &SshConfiguration, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let query = self.binding.query(cx);
        if config.auth == SshAuth::OpenSsh {
            let note = tx(cx, OPEN_SSH_NOTE);
            let shown = div()
                .px(px(4.0))
                .py(px(4.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-subtle"))
                .child(SharedString::from(note));
            return vec![shown.into_any_element()];
        }
        let (label, save_label) = secret_labels(config.auth);
        let text = tx(cx, label);
        let secret = row(cx, self.binding.matches(label, cx)).child(row_label(cx, &text, &query)).child(
            frame(cx, WIDE_FIELD, false)
                .id("settings-secret-ssh.secret")
                .debug_selector(|| "settings-secret-ssh.secret".to_owned())
                .child(self.secret.clone()),
        );
        let save_text = tx(cx, save_label);
        let save = row(cx, self.binding.matches(save_label, cx))
            .id("settings-toggle-ssh.saveSecret")
            .debug_selector(|| "settings-toggle-ssh.saveSecret".to_owned())
            .child(check_box(cx, config.save_secret))
            .child(search_label(cx, &save_text, &query))
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                view.update_current(cx, |config| config.save_secret = !config.save_secret);
            }));
        vec![secret.into_any_element(), save.into_any_element()]
    }

    /// Test Connection, what the last test came to, and the remedy it offers.
    fn test_rows(&self, config: &SshConfiguration, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let can_test = self.can_test(cx);
        let label = tx(cx, "Test Connection");
        let test = div()
            .id("settings-ssh-test")
            .debug_selector(|| "settings-ssh-test".to_owned())
            .child(button(cx, &label, false, can_test))
            .when(can_test, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.run_test(cx))));
        let status = self.status(cx).map(|(text, error)| {
            let color = match (&self.test, error) {
                (_, true) => "danger",
                (TestState::Connected(_), _) => "success",
                _ => "fg-subtle",
            };
            div()
                .debug_selector(|| "settings-ssh-test-result".to_owned())
                .text_color(theme::color(cx, color))
                .child(SharedString::from(text))
        });
        let first = div().min_h(px(ROW_HEIGHT)).px(px(4.0)).flex().items_center().gap(px(8.0));
        let mut rows = vec![first.child(test).children(status)];
        match self.remedy(config) {
            Some(SshTestRemedy::TrustHostKey) => {
                let note = tx(cx, TRUST_NOTE);
                let trust_label = tx(cx, "Trust Host and Retry");
                let trust = div()
                    .id("settings-ssh-trust")
                    .debug_selector(|| "settings-ssh-trust".to_owned())
                    .flex_none()
                    .child(button(cx, &trust_label, false, true))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.trust_and_retry(cx)));
                // Wrapping keeps the button inside the page when the note fills the row.
                rows.push(
                    div()
                        .px(px(4.0))
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.0))
                        .child(
                            div()
                                .min_w_0()
                                .text_size(theme::length(cx, "fs-small"))
                                .text_color(theme::color(cx, "fg-subtle"))
                                .child(SharedString::from(note)),
                        )
                        .child(trust),
                );
            }
            Some(SshTestRemedy::Secret) => {
                let connect_label = tx(cx, "Connect");
                let connect = div()
                    .id("settings-ssh-connect")
                    .debug_selector(|| "settings-ssh-connect".to_owned())
                    .child(button(cx, &connect_label, false, true))
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.connect(cx)));
                rows.push(div().px(px(4.0)).flex().child(connect));
            }
            None => {}
        }
        rows.into_iter().map(IntoElement::into_any_element).collect()
    }

    /// The Connection section of `config`.
    pub(super) fn form(&mut self, config: &SshConfiguration, cx: &mut Context<Self>) -> AnyElement {
        let query = self.binding.query(cx);
        let mut rows = vec![self.text_row("Name", "name", WIDE_FIELD, false, cx), self.host_row(cx)];
        rows.push(self.text_row("User name", "user", WIDE_FIELD, false, cx));
        rows.push(self.auth_row(config, cx));
        if config.auth == SshAuth::KeyPair {
            rows.push(self.key_file_row(cx));
        }
        rows.extend(self.secret_rows(config, cx));
        rows.extend(self.test_rows(config, cx));
        let title = tx(cx, "Connection");
        section(cx, Some(title.as_str()), &query, rows).into_any_element()
    }
}
