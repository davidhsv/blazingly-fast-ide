//! Tools › SSH Configurations: the app-wide SSH hosts that data source tunnels, deployment servers,
//! run targets and Tools › Start SSH Session connect with. The list has + / − / copy; the
//! configuration selected is edited below it (name, host and port, user, authentication, the key
//! file, the password or passphrase and whether to save it) with Test Connection.
//!
//! The configurations are the global `ssh.configs`, whatever scope the dialog shows. The password
//! or passphrase is typed in a [`SecretField`] and staged in the dialog's model apart from the
//! settings ([`SettingsDialog::stage_ssh_secret`]): the field always shows what is staged for the
//! configuration selected, so switching configurations, Reset, Cancel and Apply (which hands it to
//! the backend's saved passwords) all show through it. It is never drawn, listed or logged, and
//! leaves the view only as a [`SecretText`] for Test Connection.
//!
//! The form is keyed by configuration and authentication: either change starts its Test
//! Connection result, its Port text and its "already stored" answer over.

mod form;
#[cfg(test)]
mod proofs29_ssh_view;

use super::{Binding, ControlEvent, Then, new_field};
use crate::backend::Backend;
use crate::database::store::SecretText;
use crate::platform_more::remote_settings::with_unique_ids;
use crate::settings_model::deployment_model::RemoteIds;
use crate::settings_model::schema::get_at;
use crate::settings_model::ssh_connection::{SshTestRemedy, parse_ssh_port, ssh_test_remedy};
use crate::settings_model::staging::{SSH_SETTINGS_PAGE, Scope};
use crate::settings_view::controls::tx;
use crate::theme;
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, ClickEvent, Context, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, MouseButton, Render, SharedString, Subscription, Window, div, prelude::*, px,
};
use ide_core::IdeError;
use ide_core::settings::{SshAuth, SshConfiguration};
use serde_json::Value;
use std::rc::Rc;
use std::sync::Arc;

pub const PAGE: &str = SSH_SETTINGS_PAGE;
/// The slot the editor fills.
pub const SLOT: &str = "SSH Configurations";
const KEY: &str = "ssh.configs";
const EMPTY: &str = "No SSH configurations. Click + to add one.";
const NOTE: &str = "SSH configurations are shared by all projects: data source tunnels, deployment servers, run \
                    targets and Tools › Start SSH Session connect with them.";

/// The authentication types: (value, name in the settings, label).
pub const AUTHS: [(SshAuth, &str, &str); 3] = [
    (SshAuth::Password, "password", "Password"),
    (SshAuth::KeyPair, "keyPair", "Key pair (OpenSSH or PuTTY)"),
    (SshAuth::OpenSsh, "openSsh", "OpenSSH config and authentication agent"),
];

/// Connects with a configuration as edited and the secret typed (none: the one the backend knows),
/// and hands `then` what the host says about itself, or why it could not connect.
pub type SshTester = Rc<dyn Fn(SshConfiguration, Option<SecretText>, Then<Result<String, IdeError>>, &mut App)>;
/// Trusts the host key that failed the last connection of a configuration.
pub type HostTrust = Rc<dyn Fn(String, Then<Result<(), IdeError>>, &mut App)>;
/// Whether connecting with a saved configuration needs a password the backend does not know.
pub type SecretCheck = Rc<dyn Fn(String, Then<Result<bool, IdeError>>, &mut App)>;

/// What the page asks of the backend. Without a tester Test Connection is unavailable.
#[derive(Clone)]
pub struct SshChecks {
    pub test: Option<SshTester>,
    pub trust_host: HostTrust,
    pub needs_secret: SecretCheck,
}

/// The checks the page uses, set by a test as a global; without one the backend's.
pub struct SshPageChecks(pub SshChecks);

impl gpui_kit::Global for SshPageChecks {}

/// The checks set as [`SshPageChecks`], else the backend's. A test build gets no Test Connection
/// of its own making: the backend's would read the user's `~/.ssh` files (the OpenSSH
/// configuration, the agent, `known_hosts`) and reach the network, so a test that wants it sets it.
pub fn checks(cx: &App) -> SshChecks {
    match cx.try_global::<SshPageChecks>() {
        Some(set) => set.0.clone(),
        None if cfg!(test) => SshChecks { test: None, ..backend_checks() },
        None => backend_checks(),
    }
}

/// Runs `work` on the backend's runtime and hands its answer to `then` on the window's thread.
fn on_backend<T, F, W>(work: W, then: Then<Result<T, IdeError>>, cx: &mut App)
where
    T: Send + 'static,
    F: std::future::Future<Output = Result<T, IdeError>> + Send + 'static,
    W: FnOnce(Arc<ide_host::Host>) -> F + Send + 'static,
{
    let Some(backend) = cx.try_global::<Backend>() else {
        then(Err(IdeError::internal("The backend is not running")), cx);
        return;
    };
    let work = backend.with_host(work);
    cx.spawn(async move |cx| {
        let result = work.await.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())));
        cx.update(|cx| then(result, cx));
    })
    .detach();
}

/// The backend's Test Connection (`ssh_test`), as its command calls it.
pub fn backend_tester() -> SshTester {
    type Answer = Then<Result<String, IdeError>>;
    Rc::new(|config: SshConfiguration, secret: Option<SecretText>, then: Answer, cx: &mut App| {
        on_backend(
            move |host| async move { host.ssh.test(&config, secret.map(SecretText::into_inner)).await },
            then,
            cx,
        );
    })
}

/// The backend's Test Connection, host key trust (`ssh_trust_host`) and stored-secret check
/// (`ssh_needs_secret`), each called as their commands call them.
pub fn backend_checks() -> SshChecks {
    let trust_host: HostTrust = Rc::new(|id: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        on_backend(move |host| async move { host.ssh.trust_host(&id).await }, then, cx);
    });
    let needs_secret: SecretCheck = Rc::new(|id: String, then: Then<Result<bool, IdeError>>, cx: &mut App| {
        on_backend(
            move |host| async move { host.ssh.config(&id).map(|config| host.ssh.needs_secret(&config)) },
            then,
            cx,
        );
    });
    SshChecks { test: Some(backend_tester()), trust_host, needs_secret }
}

/// The configurations as the page shows them (staged, else stored, in the global settings), each
/// under an id of its own; a value that is not a list of configurations reads as none.
pub fn configs(binding: &Binding, cx: &App) -> Vec<SshConfiguration> {
    let value = binding.model.read(cx).staged_value(Scope::Global, KEY).filter(|value| !value.is_null());
    let stored: Vec<SshConfiguration> = value.and_then(|value| serde_json::from_value(value).ok()).unwrap_or_default();
    with_unique_ids(&stored, "ssh")
}

/// Stages `next` as the configurations, after forgetting the secret typed for `forget` (an
/// authentication changed or a configuration removed), in one change of the model.
fn commit(binding: &Binding, next: &[SshConfiguration], forget: Option<&str>, cx: &mut App) {
    let value = match serde_json::to_value(next) {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("Settings: the SSH configurations could not be staged: {error}");
            return;
        }
    };
    binding.model.update(cx, |dialog, cx| {
        if let Some(id) = forget {
            dialog.stage_ssh_secret(id, "");
        }
        dialog.stage(PAGE, Scope::Global, KEY, value);
        cx.notify();
    });
}

/// A new configuration (`SSH <n>`, the defaults), or a copy of `base` named after it; either with
/// the fresh `id`.
pub fn new_config(configs: &[SshConfiguration], base: Option<&SshConfiguration>, id: String) -> SshConfiguration {
    match base {
        Some(base) => {
            let shown = if base.name.is_empty() { base.label() } else { base.name.clone() };
            SshConfiguration { id, name: format!("{shown} (copy)"), ..base.clone() }
        }
        None => SshConfiguration { id, name: format!("SSH {}", configs.len() + 1), ..SshConfiguration::default() },
    }
}

/// What the list shows for a configuration: its name, else what it connects to, else its id.
pub fn shown_name(config: &SshConfiguration) -> String {
    let name = config.name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    let label = config.label();
    if label.is_empty() { config.id.clone() } else { label }
}

/// The form a configuration is edited in: it starts over when the configuration or its
/// authentication changes.
fn form_key(config: &SshConfiguration) -> String {
    format!("{}:{}", config.id, auth_name(config.auth))
}

pub fn auth_name(auth: SshAuth) -> &'static str {
    AUTHS.iter().find(|(own, _, _)| *own == auth).map_or("openSsh", |(_, name, _)| *name)
}

fn auth_label(auth: SshAuth) -> &'static str {
    AUTHS.iter().find(|(own, _, _)| *own == auth).map_or("", |(_, _, label)| *label)
}

/// The text field of a configuration's `name` (`name`, `host`, `user`, `keyFile`).
fn text_of(config: &SshConfiguration, name: &str) -> String {
    match name {
        "name" => config.name.clone(),
        "host" => config.host.clone(),
        "user" => config.user.clone(),
        "keyFile" => config.key_file.clone(),
        _ => String::new(),
    }
}

fn set_text(config: &mut SshConfiguration, name: &str, text: String) {
    match name {
        "name" => config.name = text,
        "host" => config.host = text,
        "user" => config.user = text,
        "keyFile" => config.key_file = text,
        _ => {}
    }
}

/// The error's kind as the backend names it (`permissionDenied`).
fn kind_name(error: &IdeError) -> Option<String> {
    serde_json::to_value(error.kind).ok().and_then(|kind| kind.as_str().map(str::to_owned))
}

/// Where Test Connection stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestState {
    Idle,
    Running,
    /// What the host said about itself.
    Connected(String),
    Failed {
        message: String,
        remedy: Option<SshTestRemedy>,
    },
}

/// The text fields of the form, in the order they are drawn.
const FIELDS: [&str; 5] = ["name", "host", "port", "user", "keyFile"];

pub struct SshConfigurationsView {
    binding: Binding,
    checks: SshChecks,
    ids: RemoteIds,
    selected: Option<String>,
    /// The form shown ([`form_key`]); none without a configuration selected.
    form: Option<String>,
    fields: Vec<(&'static str, Entity<TextField>)>,
    secret: Entity<SecretField>,
    /// The placeholder the secret field was given last.
    secret_placeholder: String,
    /// Whether a secret was staged for the form at the last look: one that goes (Apply, emptied)
    /// asks the backend again whether one is stored.
    had_secret: bool,
    /// The backend knows the password of the saved configuration shown.
    stored: bool,
    /// Counts the stored-secret questions: an answer to an earlier one is dropped.
    stored_asked: u64,
    auth_open: bool,
    test: TestState,
    /// Counts the tests and trusts started: an answer for an earlier one, or for another form, is
    /// dropped.
    test_run: u64,
    /// The window the editor was last drawn in, for the keyboard to go to the secret field when a
    /// test asks for it.
    window: Option<AnyWindowHandle>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ControlEvent> for SshConfigurationsView {}

impl SshConfigurationsView {
    pub fn new(binding: Binding, checks: SshChecks, cx: &mut Context<Self>) -> Self {
        let first = configs(&binding, cx).into_iter().next();
        let mut subscriptions = Vec::new();
        let mut fields = Vec::new();
        for name in FIELDS {
            let placeholder = match name {
                "port" => Some("22".to_owned()),
                "keyFile" => Some("~/.ssh/id_ed25519".to_owned()),
                _ => None,
            };
            let field = new_field("", placeholder, cx);
            subscriptions.push(cx.subscribe(&field, move |view: &mut Self, field, event: &TextFieldEvent, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = field.read(cx).text().to_owned();
                        if name == "port" {
                            let port = parse_ssh_port(&text);
                            view.update_current(cx, |config| config.port = port);
                        } else {
                            view.update_current(cx, |config| set_text(config, name, text));
                        }
                    }
                    TextFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
                    TextFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
                }
            }));
            fields.push((name, field));
        }
        let secret = cx.new(SecretField::new);
        subscriptions.push(cx.subscribe(&secret, |view: &mut Self, field, event: &SecretFieldEvent, cx| match event {
            SecretFieldEvent::Changed => {
                let Some(id) = view.selected.clone() else { return };
                let typed = field.read(cx).text(cx).to_owned();
                view.binding.model.update(cx, |dialog, cx| {
                    dialog.stage_ssh_secret(&id, &typed);
                    cx.notify();
                });
            }
            SecretFieldEvent::Submitted => cx.emit(ControlEvent::Submit),
            SecretFieldEvent::Cancelled => cx.emit(ControlEvent::Cancel),
        }));
        subscriptions.push(cx.observe(&binding.model, |view: &mut Self, _, cx| view.sync(cx)));
        let mut view = Self {
            binding,
            checks,
            ids: RemoteIds::default(),
            selected: first.map(|config| config.id),
            form: None,
            fields,
            secret,
            secret_placeholder: String::new(),
            had_secret: false,
            stored: false,
            stored_asked: 0,
            auth_open: false,
            test: TestState::Idle,
            test_run: 0,
            window: None,
            _subscriptions: subscriptions,
        };
        view.sync(cx);
        view
    }

    pub fn configs(&self, cx: &App) -> Vec<SshConfiguration> {
        configs(&self.binding, cx)
    }

    /// The configuration selected; none when the selection is gone from the list.
    pub fn current(&self, cx: &App) -> Option<SshConfiguration> {
        let id = self.selected.as_deref()?;
        self.configs(cx).into_iter().find(|config| config.id == id)
    }

    /// The text field of `name` (`name`, `host`, `port`, `user`, `keyFile`).
    pub fn field(&self, name: &str) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|(own, _)| *own == name).map(|(_, field)| field)
    }

    pub fn secret_field(&self) -> &Entity<SecretField> {
        &self.secret
    }

    pub fn test_state(&self) -> &TestState {
        &self.test
    }

    /// The backend said it knows the password of the configuration shown.
    pub fn password_stored(&self) -> bool {
        self.stored
    }

    fn field_text(&self, name: &str, cx: &App) -> String {
        self.field(name).map(|field| field.read(cx).text().to_owned()).unwrap_or_default()
    }

    /// The Port text is not a port (an empty one is the default port).
    pub fn port_invalid(&self, cx: &App) -> bool {
        let text = self.field_text("port", cx);
        !text.trim().is_empty() && parse_ssh_port(&text).is_none()
    }

    /// The secret staged for configuration `id`, as the backend takes it.
    fn staged_secret(&self, id: &str, cx: &App) -> Option<SecretText> {
        self.binding.model.read(cx).staged_secret(id).map(|secret| SecretText::from(secret.expose()))
    }

    /// Changes the configuration selected.
    fn update_current(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut SshConfiguration)) {
        let Some(id) = self.selected.clone() else { return };
        self.update_config(&id, cx, change);
    }

    fn update_config(&mut self, id: &str, cx: &mut Context<Self>, change: impl FnOnce(&mut SshConfiguration)) {
        let mut all = self.configs(cx);
        let Some(config) = all.iter_mut().find(|config| config.id == id) else { return };
        change(config);
        commit(&self.binding, &all, None, cx);
    }

    pub fn select(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.selected = id;
        self.auth_open = false;
        self.sync(cx);
    }

    /// + : a new configuration, or a copy of the selected one, added at the end and selected.
    pub fn add(&mut self, copy: bool, cx: &mut Context<Self>) {
        let mut all = self.configs(cx);
        let base = if copy { self.current(cx) } else { None };
        let id = self.ids.next("ssh", crate::platform::local_time::now_ms() as u64);
        let config = new_config(&all, base.as_ref(), id);
        let id = config.id.clone();
        all.push(config);
        commit(&self.binding, &all, None, cx);
        self.select(Some(id), cx);
    }

    /// − : the selected configuration goes with the secret typed for it; the one now in its place
    /// (else the last) is selected.
    pub fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.current(cx) else { return };
        let all = self.configs(cx);
        let index = all.iter().position(|config| config.id == current.id).unwrap_or_default();
        let rest: Vec<SshConfiguration> = all.into_iter().filter(|config| config.id != current.id).collect();
        commit(&self.binding, &rest, Some(&current.id), cx);
        let next = rest.get(index.min(rest.len().saturating_sub(1))).map(|config| config.id.clone());
        self.select(next, cx);
    }

    /// Another authentication type: the secret typed for the old one is forgotten.
    pub fn choose_auth(&mut self, auth: SshAuth, cx: &mut Context<Self>) {
        self.auth_open = false;
        let Some(current) = self.current(cx) else { return };
        if current.auth != auth {
            let mut all = self.configs(cx);
            if let Some(config) = all.iter_mut().find(|config| config.id == current.id) {
                config.auth = auth;
            }
            commit(&self.binding, &all, Some(&current.id), cx);
        }
        cx.notify();
    }

    /// The form shows the configuration selected; a field already showing its value is left alone,
    /// and the Port text is kept while it spells the port staged (an invalid text stages none).
    fn sync(&mut self, cx: &mut Context<Self>) {
        let current = self.current(cx);
        let key = current.as_ref().map(form_key);
        let new_form = key != self.form;
        if new_form {
            self.form = key;
            self.test = TestState::Idle;
            self.test_run += 1;
            self.auth_open = false;
        }
        let mut updates: Vec<(Entity<TextField>, String)> = Vec::new();
        for (name, field) in &self.fields {
            let shown = field.read(cx).text().to_owned();
            let wanted = match (current.as_ref(), *name) {
                (None, _) => String::new(),
                (Some(config), "port") => {
                    if !new_form && parse_ssh_port(&shown) == config.port {
                        continue;
                    }
                    config.port.map(|port| port.to_string()).unwrap_or_default()
                }
                (Some(config), name) => text_of(config, name),
            };
            if shown != wanted {
                updates.push((field.clone(), wanted));
            }
        }
        for (field, text) in updates {
            field.update(cx, |field, cx| field.set_text(&text, cx));
        }
        if new_form {
            let open_ssh = current.as_ref().is_some_and(|config| config.auth == SshAuth::OpenSsh);
            let host = tx(cx, if open_ssh { "host or ~/.ssh/config alias" } else { "example.com" });
            let user = if open_ssh { tx(cx, "from ~/.ssh/config") } else { String::new() };
            for (name, placeholder) in [("host", host), ("user", user)] {
                if let Some(field) = self.field(name).cloned() {
                    field.update(cx, |field, cx| field.set_placeholder(placeholder, cx));
                }
            }
        }
        let staged = current.as_ref().and_then(|config| self.staged_secret(&config.id, cx));
        let wanted = staged.clone().unwrap_or_default();
        if self.secret.read(cx).text(cx) != wanted.expose() {
            self.secret.update(cx, |field, cx| field.set_text(wanted.expose(), cx));
        }
        let has_secret = staged.is_some();
        if new_form || (self.had_secret && !has_secret) {
            self.refresh_stored(cx);
        }
        self.had_secret = has_secret;
        self.sync_secret_placeholder(cx);
        cx.notify();
    }

    /// "Password already stored" while the backend knows the password and nothing is typed.
    fn sync_secret_placeholder(&mut self, cx: &mut Context<Self>) {
        let wanted = if self.stored && !self.had_secret { tx(cx, "Password already stored") } else { String::new() };
        if wanted != self.secret_placeholder {
            self.secret_placeholder = wanted.clone();
            self.secret.update(cx, |field, cx| field.set_placeholder(wanted, cx));
        }
    }

    /// Whether the saved settings (not the edits) hold configuration `id` with a password.
    fn saved_with_password(&self, id: &str, cx: &App) -> bool {
        let dialog = self.binding.model.read(cx);
        let saved = get_at(&dialog.global_base, KEY).and_then(Value::as_array);
        saved.into_iter().flatten().any(|config| config["id"] == id && config["auth"] == "password")
    }

    /// Asks the backend whether it knows the password of the configuration shown: only for a
    /// password configuration already saved as one, with nothing typed. An error counts as no.
    fn refresh_stored(&mut self, cx: &mut Context<Self>) {
        self.stored = false;
        self.stored_asked += 1;
        let Some(config) = self.current(cx) else { return };
        let typed = self.staged_secret(&config.id, cx).is_some();
        if config.auth != SshAuth::Password || typed || !self.saved_with_password(&config.id, cx) {
            return;
        }
        let asked = self.stored_asked;
        let view = cx.weak_entity();
        let then: Then<Result<bool, IdeError>> = Rc::new(move |result: Result<bool, IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| {
                if view.stored_asked == asked {
                    view.stored = matches!(result, Ok(false));
                    view.sync_secret_placeholder(cx);
                    cx.notify();
                }
            })
            .ok();
        });
        let check = self.checks.needs_secret.clone();
        let id = config.id;
        // After this turn: a check that answers at once finds the view free to update.
        cx.defer(move |cx| check(id, then, cx));
    }

    /// Test Connection can run: a tester is there, the host is not empty and no test runs.
    pub fn can_test(&self, cx: &App) -> bool {
        let host = self.current(cx).is_some_and(|config| !config.host.trim().is_empty());
        self.checks.test.is_some() && host && self.test != TestState::Running
    }

    /// Test Connection, when it can run.
    pub fn run_test(&mut self, cx: &mut Context<Self>) {
        if self.can_test(cx) {
            self.start_test(cx);
        }
    }

    /// Tests the configuration as edited with the secret staged for it (none with OpenSSH, which
    /// takes no secret here).
    fn start_test(&mut self, cx: &mut Context<Self>) {
        let (Some(config), Some(test)) = (self.current(cx), self.checks.test.clone()) else { return };
        self.test_run += 1;
        let run = self.test_run;
        self.test = TestState::Running;
        let secret = match config.auth {
            SshAuth::OpenSsh => None,
            _ => self.staged_secret(&config.id, cx),
        };
        let view = cx.weak_entity();
        let window = self.window;
        let then: Then<Result<String, IdeError>> = Rc::new(move |result: Result<String, IdeError>, cx: &mut App| {
            let focus = view.update(cx, |view, cx| view.test_answered(run, result, cx)).ok().flatten();
            if let (Some(focus), Some(window)) = (focus, window) {
                window.update(cx, |_, window, cx| window.focus(&focus, cx)).ok();
            }
        });
        cx.defer(move |cx| test(config, secret, then, cx));
        cx.notify();
    }

    /// The answer of test `run`; the secret field to give the keyboard to when it asks for the
    /// password or passphrase.
    fn test_answered(
        &mut self,
        run: u64,
        result: Result<String, IdeError>,
        cx: &mut Context<Self>,
    ) -> Option<FocusHandle> {
        if run != self.test_run {
            return None;
        }
        let auth = self.current(cx).map_or(SshAuth::OpenSsh, |config| config.auth);
        let mut focus = None;
        self.test = match result {
            Ok(text) => TestState::Connected(text),
            Err(error) => {
                let remedy = ssh_test_remedy(kind_name(&error).as_deref(), &error.message, auth);
                if remedy == Some(SshTestRemedy::Secret) && auth != SshAuth::OpenSsh {
                    focus = Some(self.secret.focus_handle(cx));
                }
                TestState::Failed { message: error.message, remedy }
            }
        };
        cx.notify();
        focus
    }

    /// Trust Host and Retry: trusts the key of the configuration's host, then tests again.
    pub fn trust_and_retry(&mut self, cx: &mut Context<Self>) {
        let Some(config) = self.current(cx) else { return };
        self.test_run += 1;
        let run = self.test_run;
        self.test = TestState::Running;
        let view = cx.weak_entity();
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| {
                if view.test_run != run {
                    return;
                }
                match result {
                    Ok(()) => view.start_test(cx),
                    Err(error) => view.test = TestState::Failed { message: error.message, remedy: None },
                }
                cx.notify();
            })
            .ok();
        });
        let trust = self.checks.trust_host.clone();
        let id = config.id;
        cx.defer(move |cx| trust(id, then, cx));
        cx.notify();
    }

    /// Connect, after a missing or rejected secret: tests again with what is typed now.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        if self.test != TestState::Running {
            self.start_test(cx);
        }
    }

    /// Browse… for the private key file: the file chosen is the key of the configuration it was
    /// asked for.
    fn browse_key(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        let view = cx.weak_entity();
        let then: Then<std::path::PathBuf> = Rc::new(move |path: std::path::PathBuf, cx: &mut App| {
            let path = path.to_string_lossy().into_owned();
            view.update(cx, |view, cx| view.update_config(&id, cx, |config| config.key_file = path)).ok();
        });
        cx.emit(ControlEvent::Browse { directories: false, then });
    }

    /// The editor as text: the configurations (`>` the selected one: name | what it connects to),
    /// the form of the selected one ([`Self::form_lines`]) and the note. A secret is listed as
    /// bullets.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let all = self.configs(cx);
        let mut out: Vec<String> = all
            .iter()
            .map(|config| {
                let mark = if self.selected.as_deref() == Some(config.id.as_str()) { ">" } else { " " };
                format!("{mark} {} | {}", shown_name(config), config.label())
            })
            .collect();
        if all.is_empty() {
            out.push(format!("  {}", tx(cx, EMPTY)));
        }
        if let Some(config) = self.current(cx) {
            out.extend(self.form_lines(&config, cx));
        }
        out.push(tx(cx, NOTE));
        out
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let has = self.current(cx).is_some();
        let add = tool(cx, "add", "plus", true, "Add")
            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(false, cx)));
        let remove = tool(cx, "remove", "minus", has, "Remove")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.remove(cx))));
        let copy = tool(cx, "copy", "copy", has, "Copy")
            .when(has, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.add(true, cx))));
        div()
            .flex()
            .gap(px(2.0))
            .p(px(2.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(add)
            .child(remove)
            .child(copy)
    }

    fn config_list(&self, cx: &mut Context<Self>) -> gpui_kit::Div {
        let all = self.configs(cx);
        let selected_bg = theme::color(cx, "bg-selected");
        let rows: Vec<AnyElement> = all
            .iter()
            .enumerate()
            .map(|(index, config)| {
                let id = config.id.clone();
                let selector = format!("settings-ssh-config-{index}");
                let selected = self.selected.as_deref() == Some(config.id.as_str());
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
                    .child(div().flex_1().min_w_0().whitespace_nowrap().child(SharedString::from(shown_name(config))))
                    .child(
                        div()
                            .text_color(theme::color(cx, "fg-subtle"))
                            .whitespace_nowrap()
                            .child(SharedString::from(config.label())),
                    )
                    .into_any_element()
            })
            .collect();
        let empty = all.is_empty().then(|| {
            let text = tx(cx, EMPTY);
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
}

/// An icon button of the list's toolbar, drawn as `settings-ssh-<name>`, faint while it cannot be
/// used.
fn tool(cx: &App, name: &str, icon: &str, enabled: bool, title: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
    let selector = format!("settings-ssh-{name}");
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

impl Render for SshConfigurationsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.window = Some(window.window_handle());
        let list = self.config_list(cx);
        let form = self.current(cx).map(|config| self.form(&config, cx));
        let note = tx(cx, NOTE);
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            // A press anywhere else in the editor closes the open list of authentication types.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if std::mem::take(&mut view.auth_open) {
                        cx.notify();
                    }
                }),
            )
            .child(list)
            .children(form)
            .child(div().pt(px(6.0)).text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(note)))
    }
}
