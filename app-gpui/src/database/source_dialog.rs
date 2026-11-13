//! The Data Source dialog: name, colour, Global and comment above the General / Options / SSH/SSL
//! / Schemas / Advanced tabs, Test Connection with its result, OK and Cancel.
//!
//! [`SourceForm`] is the dialog's state without GPUI: the data source being edited, the typed
//! password and SSH secret, the URL typed in default mode (which fills the fields it names, while
//! the fields keep the URL shown up to date), the validation and what OK saves. [`SourceDialog`]
//! draws it; testing, saving, loading the server's names and picking files are
//! [`SourceDialogEvent`]s its owner answers. Passwords are edited in a [`PasswordField`], which
//! paints one bullet per character and never the text.

use super::dialogs::blank_ssh;
use super::model::{
    ConnectRemedy, color_token, connect_remedy, effective_ssh, file_source_name, parse_source_url, ssh_config_of,
    ssh_label, ssh_tunnel_ready,
};
use super::store::SecretChange;
use super::vendors::{VENDORS, driver_id, source_url, vendor_by_id, vendor_of};
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use crate::ui::text_area::{TextArea, TextAreaEvent};
use crate::ui::text_field::{TEXT_FIELD_CONTEXT, TextField, TextFieldEvent};
use gpui_kit::{
    App, Bounds, ClickEvent, Context, Div, ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle,
    Focusable, FontWeight, IntoElement, KeyDownEvent, Keystroke, MouseButton, MouseDownEvent, Pixels, Point, Render,
    SharedString, Stateful, Subscription, UTF16Selection, Window, canvas, deferred, div, prelude::*, px,
};
use ide_core::IdeError;
use ide_core::remote_settings::SshConfiguration;
use ide_db::{DataSource, DbColor, DbDriver, DbProperty, DbSsh, DbSshAuth, DbSslMode, DbTxMode};
use std::ops::Range;

const WIDTH: f32 = 640.0;
const LABEL_WIDTH: f32 = 150.0;
const ROW_HEIGHT: f32 = 28.0;

/// A tab of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTab {
    General,
    Options,
    Ssh,
    Schemas,
    Advanced,
}

impl SourceTab {
    fn label(self) -> &'static str {
        match self {
            SourceTab::General => "General",
            SourceTab::Options => "Options",
            SourceTab::Ssh => "SSH/SSL",
            SourceTab::Schemas => "Schemas",
            SourceTab::Advanced => "Advanced",
        }
    }
}

/// A one-line field of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Field {
    Name,
    File,
    Host,
    Port,
    User,
    Database,
    Url,
    DefaultSchema,
    InstantClient,
    TimeZone,
    KeepAlive,
    AutoDisconnect,
    SshHost,
    SshPort,
    SshUser,
    SshKey,
    SslCa,
    SslCert,
    SslKey,
    ObjectFilter,
    DatabasePattern,
    SchemaPattern,
    PropertyName(usize),
    PropertyValue(usize),
}

impl Field {
    /// The fields with a Browse… button, and the title of their file dialog.
    fn browse_title(self) -> Option<&'static str> {
        Some(match self {
            Field::File => "SQLite Database File",
            Field::SshKey => "Private Key File",
            Field::SslCa => "CA Certificate File",
            Field::SslCert => "Client Certificate File",
            Field::SslKey => "Client Key File",
            _ => return None,
        })
    }
}

/// A drop-down of the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Picker {
    Vendor,
    Color,
    Transaction,
    SshConfig,
    SshAuth,
    SslMode,
}

/// The outcome of Test Connection (or of a failed save).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestOutcome {
    pub ok: bool,
    pub text: String,
    /// The host key is unknown: Trust Host and Retry is offered.
    pub trust: bool,
}

/// `parseInt` of a port: leading digits, 1–65535.
fn parse_port(text: &str) -> Option<u16> {
    parse_leading(text).and_then(|n| u16::try_from(n).ok()).filter(|&n| n > 0)
}

/// `parseInt` of seconds: leading digits, positive.
fn parse_secs(text: &str) -> Option<u32> {
    parse_leading(text).and_then(|n| u32::try_from(n).ok()).filter(|&n| n > 0)
}

fn parse_leading(text: &str) -> Option<u64> {
    let t = text.trim_start();
    let t = t.strip_prefix('+').unwrap_or(t);
    let digits: String = t.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() { None } else { Some(digits.parse().unwrap_or(u64::MAX)) }
}

/// Driver property names the Advanced tab suggests, and what the backend does with them.
pub fn property_hint(driver: DbDriver) -> (&'static [&'static str], &'static str) {
    match driver {
        DbDriver::Clickhouse => (
            &["max_execution_time", "max_threads", "connect_timeout"],
            "ClickHouse settings sent with each HTTP request. connect_timeout is in seconds.",
        ),
        DbDriver::Postgres => (
            &[
                "statement_timeout",
                "lock_timeout",
                "idle_in_transaction_session_timeout",
                "search_path",
                "work_mem",
                "application_name",
                "connect_timeout",
            ],
            "Sent as run-time parameters when connecting (any server setting, e.g. statement_timeout). \
             application_name names the session; connect_timeout is in seconds.",
        ),
        DbDriver::Mysql => (
            &["sql_mode", "max_execution_time", "wait_timeout", "charset", "collation", "connect_timeout"],
            "Set as session variables after connecting (SET SESSION name = value). charset and collation pick \
             the connection character set; connect_timeout is in seconds.",
        ),
        DbDriver::Sqlite => (
            &["foreign_keys", "busy_timeout", "cache_size", "journal_mode", "synchronous"],
            "Applied as pragmas when opening the file (PRAGMA name = value).",
        ),
        DbDriver::SqlServer => (&["connect_timeout"], "Connection settings for SQL Server."),
        DbDriver::Oracle => (
            &["NLS_DATE_FORMAT", "NLS_TIMESTAMP_FORMAT", "NLS_LANGUAGE", "connect_timeout"],
            "Session parameters set after connecting (ALTER SESSION SET name = value). connect_timeout is in \
             seconds.",
        ),
    }
}

const COLORS: [(Option<DbColor>, &str); 7] = [
    (None, "No color"),
    (Some(DbColor::Red), "Red"),
    (Some(DbColor::Orange), "Orange"),
    (Some(DbColor::Yellow), "Yellow"),
    (Some(DbColor::Green), "Green"),
    (Some(DbColor::Blue), "Blue"),
    (Some(DbColor::Violet), "Violet"),
];

const SSL_MODES: [(DbSslMode, &str); 5] = [
    (DbSslMode::Disable, "Disable"),
    (DbSslMode::Prefer, "Prefer"),
    (DbSslMode::Require, "Require"),
    (DbSslMode::VerifyCa, "Verify CA"),
    (DbSslMode::VerifyFull, "Verify Full"),
];

const SSH_AUTHS: [(DbSshAuth, &str); 3] = [
    (DbSshAuth::OpenSsh, "OpenSSH config and authentication agent"),
    (DbSshAuth::Password, "Password"),
    (DbSshAuth::KeyPair, "Key pair"),
];

/// `user@host:port` of an SSH configuration.
fn config_label(c: &SshConfiguration) -> String {
    let user = if c.user.is_empty() { String::new() } else { format!("{}@", c.user) };
    let port = c.port.map(|p| format!(":{p}")).unwrap_or_default();
    if c.host.is_empty() { String::new() } else { format!("{user}{}{port}", c.host) }
}

/// The Data Source dialog's state.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceForm {
    pub source: DataSource,
    /// The name the dialog was opened for (`None`: a new data source).
    pub editing: Option<String>,
    /// The typed password; `None` keeps the stored one.
    pub password: Option<String>,
    /// The typed SSH password / passphrase; `None` keeps the stored one.
    pub ssh_secret: Option<String>,
    pub tab: SourceTab,
    pub test: Option<TestOutcome>,
    /// A test or a save is running.
    pub busy: bool,
    /// The URL being typed in default mode (`None`: the fields' URL shows).
    pub url_draft: Option<String>,
    pub show_comment: bool,
    pub keep_alive_draft: String,
    pub auto_disconnect_draft: String,
    pub database_pattern: String,
    pub schema_pattern: String,
    /// The server's names (Load Names from Server).
    pub known_databases: Vec<String>,
    pub known_schemas: Vec<String>,
    pub ssh_configs: Vec<SshConfiguration>,
}

impl SourceForm {
    /// The form over `source`; `editing` names the data source being edited (a new one: `None`);
    /// `password` is a preset password (Data Source from URL).
    pub fn new(source: DataSource, editing: Option<String>, password: Option<String>) -> Self {
        let keep_alive_draft = source.options.keep_alive_secs.unwrap_or(60).to_string();
        let auto_disconnect_draft = source.options.auto_disconnect_secs.unwrap_or(300).to_string();
        Self {
            show_comment: !source.comment.is_empty(),
            source,
            editing,
            password,
            ssh_secret: None,
            tab: SourceTab::General,
            test: None,
            busy: false,
            url_draft: None,
            keep_alive_draft,
            auto_disconnect_draft,
            database_pattern: String::new(),
            schema_pattern: String::new(),
            known_databases: Vec::new(),
            known_schemas: Vec::new(),
            ssh_configs: Vec::new(),
        }
    }

    pub fn title(&self) -> String {
        match &self.editing {
            Some(name) => format!("Data Source: {name}"),
            None => "New Data Source".to_owned(),
        }
    }

    pub fn sqlite(&self) -> bool {
        self.source.driver == DbDriver::Sqlite
    }

    /// The tabs of the driver; the SQLite file has no SSH/SSL and Schemas.
    pub fn tabs(&self) -> Vec<SourceTab> {
        let mut tabs = vec![SourceTab::General, SourceTab::Options];
        if !self.sqlite() {
            tabs.extend([SourceTab::Ssh, SourceTab::Schemas]);
        }
        tabs.push(SourceTab::Advanced);
        tabs
    }

    /// The tab shown: the chosen one while the driver has it.
    pub fn current_tab(&self) -> SourceTab {
        if self.tabs().contains(&self.tab) { self.tab } else { SourceTab::General }
    }

    pub fn ssh(&self) -> DbSsh {
        self.source.ssh.clone().unwrap_or_else(blank_ssh)
    }

    /// The SSH host, user and authentication in effect (the chosen configuration's).
    pub fn ssh_used(&self) -> DbSsh {
        let ssh = self.ssh();
        effective_ssh(Some(&ssh), &self.ssh_configs).unwrap_or(ssh)
    }

    fn edit_ssh(&mut self, edit: impl FnOnce(&mut DbSsh)) {
        super::dialogs::set_ssh(&mut self.source, edit);
    }

    pub fn ssh_on(&self) -> bool {
        !self.sqlite() && self.ssh().enabled
    }

    pub fn url_only(&self) -> bool {
        !self.sqlite() && self.source.url_only
    }

    /// URL only: the URL names this driver.
    pub fn url_valid(&self) -> bool {
        parse_source_url(&self.source.url).is_some_and(|p| p.source.driver == Some(self.source.driver))
    }

    /// The name saved: the typed one, else one made of the file or the server.
    pub fn name(&self) -> String {
        let typed = self.source.name.trim();
        if !typed.is_empty() {
            return typed.to_owned();
        }
        if self.sqlite() {
            let file = self.source.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            return file_source_name(&file);
        }
        let (host, database) = if self.url_only() {
            let parsed = parse_source_url(&self.source.url);
            let fields = parsed.map(|p| p.source);
            (
                fields.as_ref().and_then(|f| f.host.clone()).unwrap_or_default(),
                fields.and_then(|f| f.database).unwrap_or_default(),
            )
        } else {
            (self.source.host.clone(), self.source.database.clone())
        };
        let who = [database.as_str(), self.source.user.as_str()].into_iter().find(|s| !s.is_empty()).unwrap_or("db");
        format!("{who}@{}", if host.is_empty() { "localhost" } else { &host })
    }

    /// OK and Test Connection can be pressed: a file, or a host (a valid URL), and the SSH host
    /// of an enabled tunnel.
    pub fn valid(&self) -> bool {
        if self.sqlite() {
            return self.source.file.as_ref().is_some_and(|f| !f.to_string_lossy().trim().is_empty());
        }
        let target = if self.url_only() { self.url_valid() } else { !self.source.host.trim().is_empty() };
        target && (!self.ssh_on() || ssh_tunnel_ready(Some(&self.ssh()), &self.ssh_configs))
    }

    /// What is saved and tested: the name made up when none was typed, a preset's port when the
    /// field was left empty.
    pub fn effective(&self) -> DataSource {
        let vendor = vendor_of(&self.source.vendor, self.source.driver);
        let port = self.source.port.or(if self.source.vendor.is_empty() { None } else { vendor.port });
        DataSource { name: self.name(), port, ..self.source.clone() }
    }

    /// Another type: its driver and preset; a port left at the old type's default follows.
    pub fn set_type(&mut self, id: &str) {
        let Some(v) = vendor_by_id(id) else { return };
        let old = vendor_of(&self.source.vendor, self.source.driver);
        let s = &mut self.source;
        if s.port.is_none() || s.port == old.port {
            s.port = v.port;
        }
        s.driver = v.driver;
        s.vendor = if v.id == driver_id(v.driver) { String::new() } else { v.id.to_owned() };
        s.ssl = v.ssl.unwrap_or(if v.driver == DbDriver::Clickhouse { DbSslMode::Disable } else { s.ssl });
        if v.driver == DbDriver::Clickhouse {
            if s.user.is_empty() {
                s.user = "default".into();
            }
            if s.database.is_empty() {
                s.database = "default".into();
            }
            s.options.transaction = DbTxMode::Auto;
        }
    }

    /// Typing a URL in default mode fills the fields it names (its password too).
    fn type_url(&mut self, text: &str) {
        self.url_draft = Some(text.to_owned());
        let Some(p) = parse_source_url(text) else { return };
        if p.source.driver != Some(self.source.driver) {
            return;
        }
        let f = p.source;
        let s = &mut self.source;
        s.host = f.host.unwrap_or_default();
        s.port = f.port.flatten();
        s.database = f.database.unwrap_or_default();
        let filled = |v: Option<String>, slot: &mut String| {
            if let Some(v) = v.filter(|v| !v.is_empty()) {
                *slot = v;
            }
        };
        filled(f.user, &mut s.user);
        filled(f.ssl_ca_file, &mut s.ssl_ca_file);
        filled(f.ssl_cert_file, &mut s.ssl_cert_file);
        filled(f.ssl_key_file, &mut s.ssl_key_file);
        filled(f.vendor, &mut s.vendor);
        if let Some(ssl) = f.ssl {
            s.ssl = ssl;
        }
        if let Some(password) = p.password {
            self.password = Some(password);
        }
    }

    /// Connection type: default or URL only; URL only starts from the fields' URL.
    pub fn set_url_only(&mut self, on: bool) {
        if on && self.source.url.trim().is_empty() {
            self.source.url = source_url(&self.source);
        }
        self.source.url_only = on;
    }

    /// SSL mode; ClickHouse's port follows HTTP / HTTPS while it is at a default.
    pub fn set_ssl(&mut self, ssl: DbSslMode) {
        let secure = |m: DbSslMode| !matches!(m, DbSslMode::Disable | DbSslMode::Prefer);
        let s = &mut self.source;
        let default_port = if secure(s.ssl) { 8443 } else { 8123 };
        if s.driver == DbDriver::Clickhouse && (s.port.is_none() || s.port == Some(default_port)) {
            s.port = Some(if secure(ssl) { 8443 } else { 8123 });
        }
        s.ssl = ssl;
    }

    /// The text a field shows.
    pub fn field_value(&self, field: Field) -> String {
        let s = &self.source;
        let ssh = self.ssh();
        let property = |i: usize| s.properties.get(i);
        match field {
            Field::Name => s.name.clone(),
            Field::File => s.file.as_ref().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(),
            Field::Host => s.host.clone(),
            Field::Port => s.port.map(|p| p.to_string()).unwrap_or_default(),
            Field::User => s.user.clone(),
            Field::Database => s.database.clone(),
            Field::Url if self.url_only() => s.url.clone(),
            Field::Url => self.url_draft.clone().unwrap_or_else(|| source_url(s)),
            Field::DefaultSchema => s.options.default_schema.clone(),
            Field::InstantClient => s.options.instant_client.clone(),
            Field::TimeZone => s.options.time_zone.clone(),
            Field::KeepAlive => self.keep_alive_draft.clone(),
            Field::AutoDisconnect => self.auto_disconnect_draft.clone(),
            Field::SshHost => ssh.host,
            Field::SshPort => ssh.port.map(|p| p.to_string()).unwrap_or_default(),
            Field::SshUser => ssh.user,
            Field::SshKey => ssh.key_file,
            Field::SslCa => s.ssl_ca_file.clone(),
            Field::SslCert => s.ssl_cert_file.clone(),
            Field::SslKey => s.ssl_key_file.clone(),
            Field::ObjectFilter => s.introspection.object_filter.clone(),
            Field::DatabasePattern => self.database_pattern.clone(),
            Field::SchemaPattern => self.schema_pattern.clone(),
            Field::PropertyName(i) => property(i).map(|p| p.name.clone()).unwrap_or_default(),
            Field::PropertyValue(i) => property(i).map(|p| p.value.clone()).unwrap_or_default(),
        }
    }

    /// A field was edited.
    pub fn set_field(&mut self, field: Field, text: &str) {
        if field != Field::Url {
            // The URL shows the fields again.
            self.url_draft = None;
        }
        let s = &mut self.source;
        match field {
            Field::Name => s.name = text.to_owned(),
            Field::File => s.file = (!text.is_empty()).then(|| text.to_owned().into()),
            Field::Host => s.host = text.to_owned(),
            Field::Port => s.port = parse_port(text),
            Field::User => s.user = text.to_owned(),
            Field::Database => s.database = text.to_owned(),
            Field::Url if self.url_only() => s.url = text.to_owned(),
            Field::Url => self.type_url(text),
            Field::DefaultSchema => s.options.default_schema = text.to_owned(),
            Field::InstantClient => s.options.instant_client = text.to_owned(),
            Field::TimeZone => s.options.time_zone = text.to_owned(),
            Field::KeepAlive => {
                self.keep_alive_draft = text.to_owned();
                if let Some(n) = parse_secs(text).filter(|_| s.options.keep_alive_secs.is_some()) {
                    s.options.keep_alive_secs = Some(n);
                }
            }
            Field::AutoDisconnect => {
                self.auto_disconnect_draft = text.to_owned();
                if let Some(n) = parse_secs(text).filter(|_| s.options.auto_disconnect_secs.is_some()) {
                    s.options.auto_disconnect_secs = Some(n);
                }
            }
            Field::SshHost => self.edit_ssh(|ssh| ssh.host = text.to_owned()),
            Field::SshPort => self.edit_ssh(|ssh| ssh.port = parse_port(text)),
            Field::SshUser => self.edit_ssh(|ssh| ssh.user = text.to_owned()),
            Field::SshKey => self.edit_ssh(|ssh| ssh.key_file = text.to_owned()),
            Field::SslCa => s.ssl_ca_file = text.to_owned(),
            Field::SslCert => s.ssl_cert_file = text.to_owned(),
            Field::SslKey => s.ssl_key_file = text.to_owned(),
            Field::ObjectFilter => s.introspection.object_filter = text.to_owned(),
            Field::DatabasePattern => self.database_pattern = text.to_owned(),
            Field::SchemaPattern => self.schema_pattern = text.to_owned(),
            Field::PropertyName(i) => {
                if let Some(p) = s.properties.get_mut(i) {
                    p.name = text.to_owned();
                }
            }
            Field::PropertyValue(i) => {
                if let Some(p) = s.properties.get_mut(i) {
                    p.value = text.to_owned();
                }
            }
        }
    }

    /// Run keep-alive query each N sec. / Auto-disconnect after N sec. checked or not.
    pub fn set_seconds(&mut self, auto_disconnect: bool, on: bool) {
        let (draft, fallback) =
            if auto_disconnect { (&self.auto_disconnect_draft, 300) } else { (&self.keep_alive_draft, 60) };
        let value = on.then(|| parse_secs(draft).unwrap_or(fallback));
        if auto_disconnect {
            self.source.options.auto_disconnect_secs = value;
        } else {
            self.source.options.keep_alive_secs = value;
        }
    }

    /// Schemas tab: adds the typed pattern to the databases (or schemas) shown.
    pub fn add_pattern(&mut self, databases: bool) {
        let intro = &mut self.source.introspection;
        let (pattern, chosen) = if databases {
            (&mut self.database_pattern, &mut intro.databases)
        } else {
            (&mut self.schema_pattern, &mut intro.schemas)
        };
        let p = pattern.trim().to_owned();
        if !p.is_empty() && !chosen.contains(&p) {
            chosen.push(p);
        }
        pattern.clear();
    }

    /// Schemas tab: checks or unchecks a name.
    pub fn toggle_name(&mut self, databases: bool, name: &str) {
        let intro = &mut self.source.introspection;
        let chosen = if databases { &mut intro.databases } else { &mut intro.schemas };
        match chosen.iter().position(|n| n == name) {
            Some(i) => {
                chosen.remove(i);
            }
            None => chosen.push(name.to_owned()),
        }
    }

    /// The names a picker lists: the server's and the chosen ones.
    pub fn picker_names(&self, databases: bool) -> Vec<String> {
        let intro = &self.source.introspection;
        let (known, chosen) =
            if databases { (&self.known_databases, &intro.databases) } else { (&self.known_schemas, &intro.schemas) };
        let mut names = known.clone();
        for n in chosen {
            if !names.contains(n) {
                names.push(n.clone());
            }
        }
        names
    }

    /// What OK saves: the data source (properties without a name dropped), and the password and
    /// SSH secret to store (an emptied field forgets the stored one).
    pub fn save_request(&self) -> (DataSource, SecretChange, SecretChange) {
        let mut source = self.effective();
        source.properties.retain(|p| !p.name.trim().is_empty());
        let secret = |value: &Option<String>| match value.as_deref() {
            None => SecretChange::Keep,
            Some("") => SecretChange::Forget,
            Some(v) => SecretChange::Set(v.to_owned()),
        };
        let password = if self.sqlite() { SecretChange::Keep } else { secret(&self.password) };
        let ssh = if !self.ssh_on() || self.ssh_used().auth == DbSshAuth::OpenSsh {
            SecretChange::Keep
        } else {
            secret(&self.ssh_secret)
        };
        (source, password, ssh)
    }

    /// What Test Connection connects with: the unsaved settings, the typed password and SSH secret.
    pub fn test_request(&self) -> (DataSource, Option<String>, Option<String>) {
        let secret = if self.ssh_on() { self.ssh_secret.clone() } else { None };
        (self.effective(), self.password.clone(), secret)
    }

    /// Test Connection's answer: the server version, or the error (an unknown host key offers
    /// Trust Host and Retry).
    pub fn test_finished(&mut self, result: Result<String, IdeError>) {
        self.busy = false;
        self.test = Some(match result {
            Ok(version) => TestOutcome { ok: true, text: format!("Succeeded: {version}"), trust: false },
            Err(e) => {
                let ssh = self.ssh_used();
                let trust = connect_remedy(Some(e.kind), &e.message, Some(&ssh)) == Some(ConnectRemedy::TrustHostKey);
                TestOutcome { ok: false, text: e.message, trust }
            }
        });
    }

    /// The save failed: the message shows where the test result does.
    pub fn save_failed(&mut self, message: &str) {
        self.busy = false;
        self.test = Some(TestOutcome { ok: false, text: message.to_owned(), trust: false });
    }

    /// The URL field's text is not a URL of this driver.
    pub fn url_invalid(&self) -> bool {
        if self.url_only() {
            return !self.url_valid();
        }
        let driver = Some(self.source.driver);
        self.url_draft.as_deref().is_some_and(|d| parse_source_url(d).and_then(|p| p.source.driver) != driver)
    }

    /// The hint under the URL field (English).
    pub fn url_hint(&self) -> String {
        if !self.url_only() {
            return "Editing the URL fills in the settings above.".to_owned();
        }
        if self.url_valid() || self.source.url.trim().is_empty() {
            "Connects with the URL alone: host, port, database, SSL and parameters come from it.".to_owned()
        } else {
            format!("Not a {} URL", vendor_of(&self.source.vendor, self.source.driver).label)
        }
    }
}

// ---- password field --------------------------------------------------------------------------

/// A one-line password input: [`TextField`]'s editing state, painted as one bullet per character.
/// The text is never painted, copied or cut; the input method sees bullets.
pub struct PasswordField {
    focus: FocusHandle,
    /// The edited text and selection; never rendered.
    state: Entity<TextField>,
    placeholder: SharedString,
    _state_events: Subscription,
}

impl EventEmitter<TextFieldEvent> for PasswordField {}

const BULLET: char = '\u{2022}';

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

impl PasswordField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = cx.new(TextField::new);
        let events = cx.subscribe(&state, |_, _, event: &TextFieldEvent, cx| {
            cx.emit(event.clone());
            cx.notify();
        });
        Self { focus: cx.focus_handle(), state, placeholder: SharedString::default(), _state_events: events }
    }

    pub fn set_placeholder(&mut self, placeholder: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    /// The password typed.
    pub fn text<'a>(&self, cx: &'a App) -> &'a str {
        self.state.read(cx).text()
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.set_text(text, cx));
        cx.notify();
    }

    /// What the field shows: one bullet per character.
    pub fn display(&self, cx: &App) -> String {
        self.text(cx).chars().map(|_| BULLET).collect()
    }

    /// The selection, in characters.
    fn char_selection(&self, cx: &App) -> Range<usize> {
        let state = self.state.read(cx);
        let text = state.text();
        let sel = state.selection();
        text[..sel.start].chars().count()..text[..sel.end].chars().count()
    }

    fn replace_units(&mut self, range: Option<Range<usize>>, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.replace_text_in_range(range, text, window, cx));
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let (m, key) = (keystroke.modifiers, keystroke.key.as_str());
        let (text, sel) = {
            let state = self.state.read(cx);
            (state.text().to_owned(), state.selection())
        };
        let prev = |at: usize| text[..at].chars().next_back().map_or(at, |c| at - c.len_utf8());
        let next = |at: usize| text[at..].chars().next().map_or(at, |c| at + c.len_utf8());
        let units = |byte: usize| utf16_len(&text[..byte]);
        let jump = m.alt || m.platform;
        if m.secondary(cx) && !m.alt && !m.shift {
            match key {
                "a" => self.state.update(cx, |state, cx| state.select_all(cx)),
                // A password is not copied or cut.
                "c" | "x" => {}
                "v" => {
                    let Some(pasted) = cx.read_from_clipboard().and_then(|item| item.text()) else { return };
                    self.replace_units(None, &pasted, window, cx);
                }
                _ => return,
            }
            cx.stop_propagation();
            return;
        }
        match key {
            "left" | "right" | "home" | "end" => {
                let forward = matches!(key, "right" | "end");
                let edge = matches!(key, "home" | "end") || jump;
                let to = if edge {
                    if forward { text.len() } else { 0 }
                } else if !m.shift && !sel.is_empty() {
                    if forward { sel.end } else { sel.start }
                } else if forward {
                    next(sel.end)
                } else {
                    prev(sel.start)
                };
                let range = match (m.shift, forward) {
                    (true, true) => sel.start..to,
                    (true, false) => to..sel.end,
                    (false, _) => to..to,
                };
                self.state.update(cx, |state, cx| state.select_range(range, cx));
            }
            "backspace" | "delete" => {
                let range = if !sel.is_empty() {
                    sel
                } else if key == "backspace" {
                    (if jump { 0 } else { prev(sel.start) })..sel.start
                } else {
                    sel.end..(if jump { text.len() } else { next(sel.end) })
                };
                if !range.is_empty() {
                    self.replace_units(Some(units(range.start)..units(range.end)), "", window, cx);
                }
            }
            "enter" if !m.modified() => cx.emit(TextFieldEvent::Submitted),
            "escape" if !m.modified() => cx.emit(TextFieldEvent::Cancelled),
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn mouse_down(&mut self, _: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        window.prevent_default();
        let end = self.state.read(cx).text().len();
        self.state.update(cx, |state, cx| state.select_range(end..end, cx));
    }
}

impl Focusable for PasswordField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl EntityInputHandler for PasswordField {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        // Bullets are one UTF-16 unit each: offsets past a character outside the BMP still map.
        let len = utf16_len(self.text(cx));
        let clamped = range.start.min(len)..range.end.min(len);
        if clamped != range {
            *adjusted = Some(clamped.clone());
        }
        Some(std::iter::repeat_n(BULLET, clamped.len()).collect())
    }

    fn selected_text_range(
        &mut self,
        ignore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        self.state.update(cx, |state, cx| state.selected_text_range(ignore, window, cx))
    }

    fn marked_text_range(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<Range<usize>> {
        let state = self.state.clone();
        state.update(cx, |state, cx| state.marked_text_range(window, cx))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.state.update(cx, |state, cx| state.unmark_text(window, cx));
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_units(range, text, window, cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        marked: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.state.update(cx, |state, cx| state.replace_and_mark_text_in_range(range, new_text, marked, window, cx));
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        Some(bounds)
    }

    fn character_index_for_point(&mut self, _: Point<Pixels>, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        None
    }
}

impl Render for PasswordField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.display(cx);
        let focused = self.focus.is_focused(window);
        let sel = self.char_selection(cx);
        let pieces: Vec<char> = shown.chars().collect();
        let part = |r: Range<usize>| SharedString::from(pieces[r].iter().collect::<String>());
        let strong = theme::color(cx, "fg-strong");
        let caret = focused.then(|| div().flex_none().w(px(1.5)).h(px(16.0)).bg(theme::color(cx, "caret")));
        let content = if pieces.is_empty() {
            div()
                .flex()
                .items_center()
                .children(caret)
                .child(div().text_color(theme::color(cx, "fg-muted")).child(self.placeholder.clone()))
        } else {
            let selected = theme::color(cx, if focused { "bg-editor-selection" } else { "bg-selected" });
            div()
                .flex()
                .items_center()
                .text_color(strong)
                .child(part(0..sel.start))
                .when(sel.is_empty(), |el| el.children(caret))
                .when(!sel.is_empty(), |el| el.child(div().bg(selected).child(part(sel.clone()))))
                .child(part(sel.end..pieces.len()))
        };
        let entity = cx.entity();
        let focus = self.focus.clone();
        div()
            .relative()
            .w_full()
            .h(theme::length(cx, "h-row"))
            .flex()
            .items_center()
            .overflow_hidden()
            .key_context(TEXT_FIELD_CONTEXT)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .child(content)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                    },
                )
                .absolute()
                .size_full(),
            )
    }
}

// ---- the dialog ------------------------------------------------------------------------------

/// What the Data Source dialog asks of its owner.
#[derive(Clone, Debug, PartialEq)]
pub enum SourceDialogEvent {
    /// Test Connection (`ide_host::ops::db::test`); answer with [`SourceDialog::test_finished`].
    Test {
        source: Box<DataSource>,
        password: Option<String>,
        ssh_secret: Option<String>,
    },
    /// Trust the unknown host key, then answer with [`SourceDialog::trust_finished`]: an SSH
    /// configuration records it under its own id (`config`), else the data source does.
    TrustHost {
        config: Option<String>,
        source_id: String,
    },
    /// OK: `DbStore::save_source`; its `SourceSaved` closes the dialog, `SourceSaveFailed` goes to
    /// [`SourceDialog::save_failed`].
    Save {
        source: Box<DataSource>,
        password: SecretChange,
        ssh_secret: SecretChange,
    },
    /// Load Names from Server for the Schemas tab; answer with [`SourceDialog::set_known_names`].
    LoadNames {
        id: String,
        databases: bool,
    },
    /// Browse…: a file dialog titled `title`; answer with [`SourceDialog::file_picked`].
    PickFile {
        field: Field,
        title: &'static str,
    },
    /// Edit SSH Configurations…: Settings › Tools › SSH Configurations.
    EditSshConfigurations,
    Cancelled,
}

pub struct SourceDialog {
    form: SourceForm,
    fields: Vec<(Field, Entity<TextField>)>,
    password: Entity<PasswordField>,
    ssh_secret: Entity<PasswordField>,
    comment: Entity<TextArea>,
    startup: Entity<TextArea>,
    open_picker: Option<Picker>,
    focus: FocusHandle,
    key_claim: u64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SourceDialogEvent> for SourceDialog {}

fn tx(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx(text)
}

fn tx_template(cx: &mut App, text: &str) -> String {
    cx.default_global::<Localization>().0.tx_template(text)
}

fn checkbox(cx: &App, label: String, on: bool, enabled: bool) -> Div {
    let mark = on.then(|| theme::icon(cx, "check", 12.0, theme::color(cx, "fg-button-primary")));
    div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .text_color(theme::color(cx, "fg-strong"))
        .whitespace_nowrap()
        .when(!enabled, |el| el.opacity(0.5))
        .child(
            div()
                .flex_none()
                .size(px(16.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.0))
                .border_1()
                .border_color(theme::color(cx, if on { "bg-button-primary" } else { "border-checkbox" }))
                .bg(theme::color(cx, if on { "bg-button-primary" } else { "bg-checkbox" }))
                .children(mark),
        )
        .child(SharedString::from(label))
}

fn button(cx: &App, label: String, primary: bool, enabled: bool) -> Div {
    let face = div()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .min_w(px(72.0))
        .px(px(12.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .whitespace_nowrap()
        .child(SharedString::from(label));
    let face = if primary {
        let fill = theme::color(cx, "bg-button-primary");
        face.bg(fill).border_color(fill).text_color(theme::color(cx, "fg-button-primary"))
    } else {
        face.bg(theme::color(cx, "bg-button"))
            .border_color(theme::color(cx, "border-button"))
            .text_color(theme::color(cx, "fg-strong"))
    };
    if enabled { face } else { face.opacity(0.5) }
}

fn frame(cx: &App, invalid: bool) -> Div {
    div()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .px(px(8.0))
        .rounded(theme::length(cx, "radius-small"))
        .border_1()
        .border_color(theme::color(cx, if invalid { "danger" } else { "border" }))
        .bg(theme::color(cx, "bg-input"))
}

fn selector(prefix: &str, field: Field) -> String {
    format!("{prefix}-{field:?}")
}

impl SourceDialog {
    /// The dialog over `form`, with the keyboard in the Name field.
    pub fn new(form: SourceForm, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password = cx.new(PasswordField::new);
        let ssh_secret = cx.new(PasswordField::new);
        let comment = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_text(&form.source.comment, cx);
            area.set_rows(2, 4, cx);
            area
        });
        let startup = cx.new(|cx| {
            let mut area = TextArea::new(cx);
            area.set_text(&form.source.options.startup_script, cx);
            area.set_rows(3, 6, cx);
            area
        });
        let mut subscriptions = vec![
            cx.subscribe_in(&password, window, |dialog: &mut Self, field, event: &TextFieldEvent, _, cx| match event {
                TextFieldEvent::Changed => {
                    dialog.form.password = Some(field.read(cx).text(cx).to_owned());
                    cx.notify();
                }
                TextFieldEvent::Submitted => dialog.press_ok(cx),
                TextFieldEvent::Cancelled => cx.emit(SourceDialogEvent::Cancelled),
            }),
            cx.subscribe_in(
                &ssh_secret,
                window,
                |dialog: &mut Self, field, event: &TextFieldEvent, _, cx| match event {
                    TextFieldEvent::Changed => {
                        dialog.form.ssh_secret = Some(field.read(cx).text(cx).to_owned());
                        cx.notify();
                    }
                    TextFieldEvent::Submitted => dialog.press_ok(cx),
                    TextFieldEvent::Cancelled => cx.emit(SourceDialogEvent::Cancelled),
                },
            ),
            cx.subscribe_in(&comment, window, |dialog: &mut Self, area, event: &TextAreaEvent, _, cx| {
                if *event == TextAreaEvent::Changed {
                    dialog.form.source.comment = area.read(cx).text().to_owned();
                    cx.notify();
                }
            }),
            cx.subscribe_in(&startup, window, |dialog: &mut Self, area, event: &TextAreaEvent, _, cx| {
                if *event == TextAreaEvent::Changed {
                    dialog.form.source.options.startup_script = area.read(cx).text().to_owned();
                    cx.notify();
                }
            }),
        ];
        let mut dialog = Self {
            form,
            fields: Vec::new(),
            password,
            ssh_secret,
            comment,
            startup,
            open_picker: None,
            focus: cx.focus_handle(),
            key_claim: 0,
            _subscriptions: Vec::new(),
        };
        let fixed = [
            Field::Name,
            Field::File,
            Field::Host,
            Field::Port,
            Field::User,
            Field::Database,
            Field::Url,
            Field::DefaultSchema,
            Field::InstantClient,
            Field::TimeZone,
            Field::KeepAlive,
            Field::AutoDisconnect,
            Field::SshHost,
            Field::SshPort,
            Field::SshUser,
            Field::SshKey,
            Field::SslCa,
            Field::SslCert,
            Field::SslKey,
            Field::ObjectFilter,
            Field::DatabasePattern,
            Field::SchemaPattern,
        ];
        for field in fixed {
            subscriptions.push(dialog.add_field(field, window, cx));
        }
        for i in 0..dialog.form.source.properties.len() {
            subscriptions.push(dialog.add_field(Field::PropertyName(i), window, cx));
            subscriptions.push(dialog.add_field(Field::PropertyValue(i), window, cx));
        }
        dialog._subscriptions = subscriptions;
        if let Some(preset) = dialog.form.password.clone() {
            dialog.password.update(cx, |field, cx| field.set_text(&preset, cx));
        }
        dialog.sync_placeholders(cx);
        let focus = dialog.field(Field::Name).map_or_else(|| dialog.focus.clone(), |f| f.focus_handle(cx));
        dialog.focus = focus.clone();
        let claimant = cx.weak_entity();
        dialog.key_claim = input::claim_keys(
            focus,
            move |keystroke, _, cx| claimant.update(cx, |dialog, cx| dialog.claim_key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|dialog, cx| input::release_keys(dialog.key_claim, cx)).detach();
        dialog
    }

    fn add_field(&mut self, field: Field, window: &mut Window, cx: &mut Context<Self>) -> Subscription {
        let value = self.form.field_value(field);
        let placeholder = match field {
            Field::DatabasePattern | Field::SchemaPattern => "Name or pattern, e.g. app_*",
            Field::ObjectFilter => "e.g. order*, customers (tables, views, routines, sequences)",
            Field::PropertyName(_) => "<user defined>",
            Field::PropertyValue(_) => "<value>",
            Field::InstantClient => "/opt/oracle/instantclient_23_5",
            Field::SshKey => "~/.ssh/id_ed25519",
            Field::SshPort => "22",
            Field::File => "path/to/database.db (absolute or project-relative)",
            _ => "",
        };
        let placeholder = tx(cx, placeholder);
        let entity = cx.new(|cx| {
            let mut f = TextField::new(cx).with_placeholder(placeholder);
            f.set_text(&value, cx);
            f
        });
        let subscription =
            cx.subscribe_in(&entity, window, move |dialog: &mut Self, entity, event: &TextFieldEvent, window, cx| {
                match event {
                    TextFieldEvent::Changed => {
                        let text = entity.read(cx).text().to_owned();
                        dialog.form.set_field(field, &text);
                        dialog.sync(Some(field), window, cx);
                    }
                    TextFieldEvent::Submitted => match field {
                        Field::DatabasePattern => dialog.add_pattern(true, window, cx),
                        Field::SchemaPattern => dialog.add_pattern(false, window, cx),
                        _ => dialog.press_ok(cx),
                    },
                    TextFieldEvent::Cancelled => cx.emit(SourceDialogEvent::Cancelled),
                }
            });
        self.fields.push((field, entity));
        subscription
    }

    /// Shows the form's values in the fields other than `edited` (which shows what was typed).
    fn sync(&mut self, edited: Option<Field>, _: &mut Window, cx: &mut Context<Self>) {
        for (field, entity) in &self.fields {
            if edited == Some(*field) && !matches!(field, Field::Port | Field::SshPort) {
                continue;
            }
            let value = self.form.field_value(*field);
            if entity.read(cx).text() != value {
                entity.update(cx, |f, cx| f.set_text(&value, cx));
            }
        }
        let password = self.form.password.clone().unwrap_or_default();
        if self.password.read(cx).text(cx) != password {
            self.password.update(cx, |f, cx| f.set_text(&password, cx));
        }
        self.sync_placeholders(cx);
        cx.notify();
    }

    fn sync_placeholders(&mut self, cx: &mut Context<Self>) {
        let unchanged = tx(cx, "<unchanged>");
        let editing = self.form.editing.is_some();
        let password = if editing && self.form.password.is_none() { unchanged.clone() } else { String::new() };
        let secret = if editing && self.form.ssh_secret.is_none() { unchanged } else { String::new() };
        self.password.update(cx, |f, cx| f.set_placeholder(password, cx));
        self.ssh_secret.update(cx, |f, cx| f.set_placeholder(secret, cx));
    }

    pub fn form(&self) -> &SourceForm {
        &self.form
    }

    /// Changes the form from outside (tests, Data Source from URL) and shows the change.
    pub fn edit(&mut self, edit: impl FnOnce(&mut SourceForm), window: &mut Window, cx: &mut Context<Self>) {
        edit(&mut self.form);
        self.ensure_property_fields(window, cx);
        self.sync(None, window, cx);
    }

    pub fn field(&self, field: Field) -> Option<&Entity<TextField>> {
        self.fields.iter().find(|(f, _)| *f == field).map(|(_, e)| e)
    }

    pub fn password_field(&self) -> &Entity<PasswordField> {
        &self.password
    }

    pub fn ssh_secret_field(&self) -> &Entity<PasswordField> {
        &self.ssh_secret
    }

    pub fn open_picker(&self) -> Option<Picker> {
        self.open_picker
    }

    /// The answer to [`SourceDialogEvent::Test`].
    pub fn test_finished(&mut self, result: Result<String, IdeError>, cx: &mut Context<Self>) {
        self.form.test_finished(result);
        cx.notify();
    }

    /// The host key is trusted: test again.
    pub fn trust_finished(&mut self, cx: &mut Context<Self>) {
        self.press_test(cx);
    }

    /// The answer to a failed [`SourceDialogEvent::Save`].
    pub fn save_failed(&mut self, message: &str, cx: &mut Context<Self>) {
        self.form.save_failed(message);
        cx.notify();
    }

    /// The answer to [`SourceDialogEvent::LoadNames`].
    pub fn set_known_names(&mut self, databases: Vec<String>, schemas: Vec<String>, cx: &mut Context<Self>) {
        self.form.busy = false;
        self.form.known_databases = databases;
        self.form.known_schemas = schemas;
        cx.notify();
    }

    /// The answer to [`SourceDialogEvent::PickFile`].
    pub fn file_picked(&mut self, field: Field, path: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.form.set_field(field, path);
        self.sync(None, window, cx);
    }

    pub fn select_tab(&mut self, tab: SourceTab, cx: &mut Context<Self>) {
        self.form.tab = tab;
        self.open_picker = None;
        cx.notify();
    }

    /// Test Connection.
    pub fn press_test(&mut self, cx: &mut Context<Self>) {
        if self.form.busy || !self.form.valid() {
            return;
        }
        self.form.busy = true;
        self.form.test = None;
        let (source, password, ssh_secret) = self.form.test_request();
        cx.emit(SourceDialogEvent::Test { source: Box::new(source), password, ssh_secret });
        cx.notify();
    }

    /// OK: saves when the settings are complete.
    pub fn press_ok(&mut self, cx: &mut Context<Self>) {
        if self.form.busy || !self.form.valid() {
            return;
        }
        self.form.busy = true;
        let (source, password, ssh_secret) = self.form.save_request();
        cx.emit(SourceDialogEvent::Save { source: Box::new(source), password, ssh_secret });
        cx.notify();
    }

    fn trust(&mut self, cx: &mut Context<Self>) {
        let config = self.form.ssh().config.trim().to_owned();
        let config = (!config.is_empty()).then_some(config);
        cx.emit(SourceDialogEvent::TrustHost { config, source_id: self.form.source.id.clone() });
    }

    fn load_names(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.form.editing.as_ref().map(|_| self.form.source.id.clone()) else { return };
        if self.form.busy {
            return;
        }
        self.form.busy = true;
        cx.emit(SourceDialogEvent::LoadNames { id, databases: self.form.source.driver == DbDriver::Postgres });
        cx.notify();
    }

    fn add_pattern(&mut self, databases: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.form.add_pattern(databases);
        self.sync(None, window, cx);
    }

    fn ensure_property_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.form.source.properties.len();
        self.fields.retain(|(f, _)| !matches!(f, Field::PropertyName(i) | Field::PropertyValue(i) if *i >= count));
        for i in 0..count {
            if self.field(Field::PropertyName(i)).is_none() {
                let a = self.add_field(Field::PropertyName(i), window, cx);
                let b = self.add_field(Field::PropertyValue(i), window, cx);
                self._subscriptions.extend([a, b]);
            }
        }
    }

    fn add_property(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.form.source.properties.push(DbProperty { name: String::new(), value: String::new() });
        self.ensure_property_fields(window, cx);
        cx.notify();
    }

    fn remove_property(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.form.source.properties.len() {
            self.form.source.properties.remove(index);
        }
        self.ensure_property_fields(window, cx);
        self.sync(None, window, cx);
    }

    fn choose(&mut self, picker: Picker, value: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.open_picker = None;
        let f = &mut self.form;
        match picker {
            Picker::Vendor => {
                if let Some(v) = VENDORS.get(value) {
                    f.set_type(v.id);
                }
            }
            Picker::Color => f.source.color = COLORS.get(value).and_then(|(c, _)| *c),
            Picker::Transaction => {
                f.source.options.transaction = if value == 1 { DbTxMode::Manual } else { DbTxMode::Auto }
            }
            Picker::SshAuth => {
                if let Some((auth, _)) = SSH_AUTHS.get(value) {
                    f.edit_ssh(|ssh| ssh.auth = *auth);
                }
            }
            Picker::SshConfig => {
                let config = match value {
                    0 => String::new(),
                    i => f.ssh_configs.get(i - 1).map(|c| c.id.clone()).unwrap_or_else(|| f.ssh().config),
                };
                f.edit_ssh(|ssh| ssh.config = config);
            }
            Picker::SslMode => {
                if let Some((mode, _)) = SSL_MODES.get(value) {
                    f.set_ssl(*mode);
                }
            }
        }
        self.sync(None, window, cx);
    }

    /// The options of a picker (English labels) and the one chosen.
    pub fn picker_options(&self, picker: Picker) -> (Vec<String>, Option<usize>) {
        let f = &self.form;
        let s = &f.source;
        match picker {
            Picker::Vendor => {
                let current = vendor_of(&s.vendor, s.driver).id;
                (VENDORS.iter().map(|v| v.label.to_owned()).collect(), VENDORS.iter().position(|v| v.id == current))
            }
            Picker::Color => {
                (COLORS.iter().map(|(_, l)| (*l).to_owned()).collect(), COLORS.iter().position(|(c, _)| *c == s.color))
            }
            Picker::Transaction => {
                let manual = s.driver != DbDriver::Clickhouse && s.options.transaction == DbTxMode::Manual;
                let options = if s.driver == DbDriver::Clickhouse {
                    vec!["Auto".to_owned()]
                } else {
                    vec!["Auto".to_owned(), "Manual".to_owned()]
                };
                (options, Some(usize::from(manual)))
            }
            Picker::SshAuth => {
                let auth = f.ssh().auth;
                let labels = SSH_AUTHS.iter().map(|(_, l)| (*l).to_owned()).collect();
                (labels, SSH_AUTHS.iter().position(|(a, _)| *a == auth))
            }
            Picker::SshConfig => {
                let id = f.ssh().config.trim().to_owned();
                let mut options = vec!["Custom (fields below)".to_owned()];
                options.extend(f.ssh_configs.iter().map(|c| {
                    let label = config_label(c);
                    match (c.name.trim().is_empty(), label.is_empty()) {
                        (false, _) => c.name.clone(),
                        (true, true) => c.id.clone(),
                        (true, false) => label,
                    }
                }));
                let chosen = if id.is_empty() {
                    Some(0)
                } else {
                    match f.ssh_configs.iter().position(|c| c.id == id) {
                        Some(i) => Some(i + 1),
                        None => {
                            options.push(format!("{id} (missing)"));
                            Some(options.len() - 1)
                        }
                    }
                };
                (options, chosen)
            }
            Picker::SslMode => {
                let modes: Vec<&(DbSslMode, &str)> = SSL_MODES
                    .iter()
                    .filter(|(m, _)| s.driver != DbDriver::Clickhouse || *m != DbSslMode::Prefer)
                    .collect();
                let chosen = modes.iter().position(|(m, _)| *m == s.ssl);
                (modes.iter().map(|(_, l)| (*l).to_owned()).collect(), chosen)
            }
        }
    }

    /// The picker's option index to the value index `choose` takes (ClickHouse hides Prefer).
    fn picker_value(&self, picker: Picker, option: usize) -> usize {
        if picker == Picker::SslMode && self.form.source.driver == DbDriver::Clickhouse && option >= 1 {
            option + 1
        } else {
            option
        }
    }

    fn claim_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        match keystroke.key.as_str() {
            "escape" if self.open_picker.is_some() => {
                self.open_picker = None;
                cx.notify();
            }
            "escape" => cx.emit(SourceDialogEvent::Cancelled),
            _ => return false,
        }
        true
    }

    // ---- rendering ----------------------------------------------------------------------------

    fn row(control: impl IntoElement, label: &str, cx: &mut App) -> Div {
        let label = tx(cx, label);
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .min_h(px(ROW_HEIGHT))
            .child(
                div()
                    .flex_none()
                    .w(px(LABEL_WIDTH))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(label)),
            )
            .child(div().flex_1().min_w_0().flex().items_center().gap(px(8.0)).child(control))
    }

    fn hint(cx: &mut App, text: &str) -> Div {
        let text = tx_template(cx, text);
        div()
            .pl(px(LABEL_WIDTH + 8.0))
            .text_size(theme::length(cx, "fs-small"))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(text))
    }

    fn text_box(&self, field: Field, invalid: bool, enabled: bool, cx: &mut Context<Self>) -> Div {
        let Some(entity) = self.field(field).cloned() else { return div() };
        let boxed = frame(cx, invalid).flex_1().min_w_0().when(!enabled, |el| el.opacity(0.5)).child(
            div()
                .flex_1()
                .min_w_0()
                .debug_selector(move || selector("source-field", field))
                .when(enabled, |el| el.child(entity.clone()))
                .when(!enabled, |el| el.child(SharedString::from(entity.read(cx).text().to_owned()))),
        );
        let browse = field.browse_title().map(|title| {
            let label = tx(cx, "Browse…");
            button(cx, label, false, enabled)
                .id(SharedString::from(selector("source-browse", field)))
                .debug_selector(move || selector("source-browse", field))
                .when(enabled, |el| {
                    el.on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                        cx.emit(SourceDialogEvent::PickFile { field, title });
                    }))
                })
        });
        div().flex_1().min_w_0().flex().items_center().gap(px(8.0)).child(boxed).children(browse)
    }

    fn password_box(&self, entity: &Entity<PasswordField>, id: &'static str, enabled: bool, cx: &App) -> Div {
        frame(cx, false)
            .flex_1()
            .min_w_0()
            .debug_selector(move || id.to_owned())
            .when(!enabled, |el| el.opacity(0.5))
            .when(enabled, |el| el.child(entity.clone()))
    }

    fn check(
        &self,
        id: &'static str,
        label: &str,
        on: bool,
        enabled: bool,
        toggle: impl Fn(&mut SourceForm) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label = tx(cx, label);
        checkbox(cx, label, on, enabled).id(id).debug_selector(move || id.to_owned()).when(enabled, |el| {
            el.on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| {
                toggle(&mut dialog.form);
                dialog.sync(None, window, cx);
            }))
        })
    }

    fn picker(&self, picker: Picker, enabled: bool, cx: &mut Context<Self>) -> Div {
        let (options, chosen) = self.picker_options(picker);
        let label = chosen.and_then(|i| options.get(i)).cloned().unwrap_or_default();
        let label = tx(cx, &label);
        let open = self.open_picker == Some(picker) && enabled;
        let swatch = (picker == Picker::Color).then(|| color_token(self.form.source.color)).flatten().map(|token| {
            let token = token.trim_start_matches("--").to_owned();
            div().flex_none().size(px(10.0)).rounded(px(2.0)).bg(theme::color(cx, &token))
        });
        let list = open.then(|| {
            let rows: Vec<_> = options
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let text = tx(cx, text);
                    let value = self.picker_value(picker, i);
                    div()
                        .id(SharedString::from(format!("source-picker-{picker:?}-{i}")))
                        .debug_selector(move || format!("source-picker-{picker:?}-{i}"))
                        .h(theme::length(cx, "h-list-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .whitespace_nowrap()
                        .text_color(theme::color(cx, "fg-strong"))
                        .when(Some(i) == chosen, |el| el.bg(theme::color(cx, "bg-tree-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                        .child(SharedString::from(text))
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| {
                            dialog.choose(picker, value, window, cx);
                        }))
                })
                .collect();
            deferred(
                div()
                    .id(SharedString::from(format!("source-picker-list-{picker:?}")))
                    .absolute()
                    .top(px(ROW_HEIGHT))
                    .left(px(0.0))
                    .min_w(px(200.0))
                    .max_h(px(320.0))
                    .overflow_y_scroll()
                    .p(px(4.0))
                    .flex()
                    .flex_col()
                    .bg(theme::color(cx, "bg-popup"))
                    .border_1()
                    .border_color(theme::color(cx, "border-popup"))
                    .rounded(theme::length(cx, "radius-popup"))
                    .occlude()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(rows),
            )
            .with_priority(1)
        });
        div().relative().child(
            frame(cx, false)
                .id(SharedString::from(format!("source-picker-{picker:?}")))
                .debug_selector(move || format!("source-picker-{picker:?}"))
                .min_w(px(160.0))
                .gap(px(6.0))
                .text_color(theme::color(cx, "fg-strong"))
                .when(!enabled, |el| el.opacity(0.5))
                .children(swatch)
                .child(div().flex_1().whitespace_nowrap().child(SharedString::from(label)))
                .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-subtle")))
                .when(enabled, |el| {
                    el.on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                        dialog.open_picker = if dialog.open_picker == Some(picker) { None } else { Some(picker) };
                        cx.notify();
                    }))
                })
                .children(list),
        )
    }

    fn general_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let f = &self.form;
        let mut rows = vec![Self::row(self.picker(Picker::Vendor, true, cx), "Driver:", cx)];
        if f.sqlite() {
            rows.push(Self::row(self.text_box(Field::File, false, true, cx), "File:", cx));
            return rows;
        }
        let url_only = f.url_only();
        let segment = |cx: &mut Context<Self>, id: &'static str, label: &str, on: bool, value: bool| {
            let label = tx(cx, label);
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .px(px(10.0))
                .h(px(24.0))
                .flex()
                .items_center()
                .rounded(px(4.0))
                .when(on, |el| {
                    el.bg(theme::color(cx, "bg-button-primary")).text_color(theme::color(cx, "fg-button-primary"))
                })
                .child(SharedString::from(label))
                .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| {
                    dialog.form.set_url_only(value);
                    dialog.sync(None, window, cx);
                }))
        };
        let kinds = div()
            .flex()
            .gap(px(2.0))
            .child(segment(cx, "source-default", "default", !url_only, false))
            .child(segment(cx, "source-url-only", "URL only", url_only, true));
        rows.push(Self::row(kinds, "Connection type:", cx));
        if !url_only {
            let port = Self::row(self.text_box(Field::Port, false, true, cx), "Port:", cx).w(px(200.0));
            let host_box = self.text_box(Field::Host, false, true, cx);
            let host = div().flex_1().flex().items_center().gap(px(8.0)).child(host_box);
            rows.push(Self::row(host.child(port), "Host:", cx));
        }
        if f.ssh_on() {
            let label = ssh_label(Some(&f.ssh_used()));
            rows.push(Self::hint(cx, &format!("Reached from the SSH host {label}.")));
        }
        rows.push(Self::row(self.text_box(Field::User, false, true, cx), "User:", cx));
        let save_password = self.check(
            "source-save-password",
            "Save password",
            f.source.save_password,
            true,
            |f| {
                f.source.save_password = !f.source.save_password;
            },
            cx,
        );
        let password = div()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.password_box(&self.password, "source-password", true, cx))
            .child(save_password);
        rows.push(Self::row(password, "Password:", cx));
        if !url_only {
            rows.push(Self::row(self.text_box(Field::Database, false, true, cx), "Database:", cx));
        }
        rows.push(Self::row(self.text_box(Field::Url, f.url_invalid(), true, cx), "URL:", cx));
        let hint = f.url_hint();
        rows.push(Self::hint(cx, &hint));
        rows
    }

    fn options_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let f = &self.form;
        let s = &f.source;
        let (postgres, oracle, clickhouse) =
            (s.driver == DbDriver::Postgres, s.driver == DbDriver::Oracle, s.driver == DbDriver::Clickhouse);
        let mut rows = vec![section(cx, "Connection")];
        let read_only = self.check(
            "source-read-only",
            "Read-only",
            s.read_only,
            true,
            |f| {
                f.source.read_only = !f.source.read_only;
            },
            cx,
        );
        rows.push(Self::row(read_only, "", cx));
        rows.push(Self::row(self.picker(Picker::Transaction, !clickhouse, cx), "Transaction control:", cx));
        if postgres {
            rows.push(Self::row(self.text_box(Field::DefaultSchema, false, true, cx), "Default schema:", cx));
        }
        if oracle {
            rows.push(Self::row(self.text_box(Field::DefaultSchema, false, true, cx), "Current schema:", cx));
            rows.push(Self::row(self.text_box(Field::InstantClient, false, true, cx), "Instant Client:", cx));
            rows.push(Self::hint(
                cx,
                "Directory of Oracle Instant Client 19 or later. Empty: ORACLE_HOME or the system library path.",
            ));
        }
        if !f.sqlite() {
            rows.push(Self::row(self.text_box(Field::TimeZone, false, true, cx), "Time zone:", cx));
            rows.push(self.seconds(false, cx));
        }
        rows.push(self.seconds(true, cx));
        if postgres {
            let single = self.check(
                "source-single-database",
                "Single database mode (show only the configured database)",
                s.options.single_database,
                true,
                |f| f.source.options.single_database = !f.source.options.single_database,
                cx,
            );
            rows.push(Self::row(single, "", cx));
        }
        let startup = frame(cx, false).h(px(ROW_HEIGHT * 2.5)).flex_1().child(self.startup.clone());
        rows.push(Self::row(startup, "Startup script:", cx));
        rows.push(Self::hint(cx, "Runs on every new connection, before any other statement."));
        rows
    }

    fn seconds(&self, auto_disconnect: bool, cx: &mut Context<Self>) -> Div {
        let options = &self.form.source.options;
        let on =
            if auto_disconnect { options.auto_disconnect_secs.is_some() } else { options.keep_alive_secs.is_some() };
        let (id, label, field) = if auto_disconnect {
            ("source-auto-disconnect", "Auto-disconnect after", Field::AutoDisconnect)
        } else {
            ("source-keep-alive", "Run keep-alive query each", Field::KeepAlive)
        };
        let check = self.check(id, label, on, true, move |f| f.set_seconds(auto_disconnect, !on), cx);
        let secs = tx(cx, "sec.");
        let control = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(check)
            .child(div().w(px(80.0)).child(self.text_box(field, false, on, cx)))
            .child(SharedString::from(secs));
        Self::row(control, "", cx)
    }

    fn ssh_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let f = &self.form;
        let ssh = f.ssh();
        let used = f.ssh_used();
        let on = f.ssh_on();
        let config_id = ssh.config.trim().to_owned();
        let mut rows = Vec::new();
        let tunnel = self.check(
            "source-ssh",
            "Use SSH tunnel",
            ssh.enabled,
            true,
            |f| {
                f.edit_ssh(|ssh| ssh.enabled = !ssh.enabled);
            },
            cx,
        );
        rows.push(Self::row(tunnel, "", cx));
        rows.push(Self::row(self.picker(Picker::SshConfig, on, cx), "Configuration:", cx));
        if !config_id.is_empty() {
            let text = match ssh_config_of(Some(&ssh), &f.ssh_configs) {
                Some(c) => {
                    let label = config_label(c);
                    format!("Connects to {}.", if label.is_empty() { c.name.clone() } else { label })
                }
                None => format!("SSH configuration {config_id} no longer exists."),
            };
            let text = tx_template(cx, &text);
            let edit = tx(cx, "Edit SSH Configurations…");
            let link = div()
                .id("source-edit-ssh")
                .debug_selector(|| "source-edit-ssh".to_owned())
                .text_color(theme::color(cx, "accent"))
                .child(SharedString::from(edit))
                .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SourceDialogEvent::EditSshConfigurations)));
            rows.push(Self::row(div().flex().gap(px(6.0)).child(SharedString::from(text)).child(link), "", cx));
        } else {
            let port = Self::row(self.text_box(Field::SshPort, false, on, cx), "Port:", cx).w(px(200.0));
            let host_box = self.text_box(Field::SshHost, false, on, cx);
            let host = div().flex_1().flex().items_center().gap(px(8.0)).child(host_box);
            rows.push(Self::row(host.child(port), "Host:", cx));
            rows.push(Self::row(self.text_box(Field::SshUser, false, on, cx), "User:", cx));
            rows.push(Self::row(self.picker(Picker::SshAuth, on, cx), "Authentication:", cx));
            if ssh.auth == DbSshAuth::KeyPair {
                rows.push(Self::row(self.text_box(Field::SshKey, false, on, cx), "Private key:", cx));
            }
        }
        if used.auth == DbSshAuth::OpenSsh && config_id.is_empty() {
            rows.push(Self::hint(
                cx,
                "Uses HostName, User, Port, IdentityFile and ProxyJump from ~/.ssh/config, and keys of the SSH agent.",
            ));
        }
        if used.auth != DbSshAuth::OpenSsh {
            let label = if used.auth == DbSshAuth::Password { "Password:" } else { "Passphrase:" };
            let secret = self.password_box(&self.ssh_secret, "source-ssh-secret", on, cx);
            let mut control = div().flex_1().flex().items_center().gap(px(8.0)).child(secret);
            if config_id.is_empty() {
                let save = self.check(
                    "source-ssh-save",
                    "Save password",
                    ssh.save_secret,
                    on,
                    |f| {
                        f.edit_ssh(|ssh| ssh.save_secret = !ssh.save_secret);
                    },
                    cx,
                );
                control = control.child(save);
            }
            rows.push(Self::row(control, label, cx));
        }
        rows.push(section(cx, "SSL"));
        rows.push(Self::row(self.picker(Picker::SslMode, true, cx), "Mode:", cx));
        let ssl_on = f.source.ssl != DbSslMode::Disable;
        rows.push(Self::row(self.text_box(Field::SslCa, false, ssl_on, cx), "CA file:", cx));
        rows.push(Self::row(self.text_box(Field::SslCert, false, ssl_on, cx), "Client certificate file:", cx));
        rows.push(Self::row(self.text_box(Field::SslKey, false, ssl_on, cx), "Client key file:", cx));
        rows
    }

    fn name_picker(&self, databases: bool, cx: &mut Context<Self>) -> Div {
        let intro = &self.form.source.introspection;
        let mysql = self.form.source.driver == DbDriver::Mysql;
        let (all, all_label, id) = if databases {
            (intro.all_databases, "All databases", "source-all-databases")
        } else if mysql {
            (intro.all_schemas, "All schemas (databases)", "source-all-schemas")
        } else {
            (intro.all_schemas, "All schemas", "source-all-schemas")
        };
        let all_check = self.check(
            id,
            all_label,
            all,
            true,
            move |f| {
                let intro = &mut f.source.introspection;
                if databases {
                    intro.all_databases = !intro.all_databases;
                } else {
                    intro.all_schemas = !intro.all_schemas;
                }
            },
            cx,
        );
        let mut picker = div().flex().flex_col().gap(px(4.0)).child(all_check);
        if !all {
            let chosen = if databases { &intro.databases } else { &intro.schemas };
            let names = self.form.picker_names(databases);
            let checks: Vec<_> = names
                .into_iter()
                .enumerate()
                .map(|(i, name)| {
                    let on = chosen.contains(&name);
                    checkbox(cx, name.clone(), on, true)
                        .id(SharedString::from(format!("source-name-{databases}-{i}")))
                        .debug_selector(move || format!("source-name-{databases}-{i}"))
                        .pl(px(16.0))
                        .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| {
                            dialog.form.toggle_name(databases, &name);
                            cx.notify();
                        }))
                })
                .collect();
            let field = if databases { Field::DatabasePattern } else { Field::SchemaPattern };
            let pattern_empty = self.form.field_value(field).trim().is_empty();
            let pattern = self.text_box(field, false, true, cx);
            let add = tx(cx, "Add Pattern");
            let add = button(cx, add, false, !pattern_empty)
                .id(if databases { "source-add-database-pattern" } else { "source-add-schema-pattern" })
                .on_click(
                    cx.listener(move |dialog, _: &ClickEvent, window, cx| dialog.add_pattern(databases, window, cx)),
                );
            picker = picker
                .children(checks)
                .child(div().pl(px(16.0)).flex().items_center().gap(px(8.0)).child(pattern).child(add));
        }
        picker
    }

    fn schemas_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let s = &self.form.source;
        let postgres = s.driver == DbDriver::Postgres;
        let mut rows = Vec::new();
        if postgres {
            rows.push(self.name_picker(true, cx));
        }
        rows.push(self.name_picker(false, cx));
        rows.push(Self::row(self.text_box(Field::ObjectFilter, false, true, cx), "Object filter:", cx));
        let toggle_system = |f: &mut SourceForm| {
            f.source.introspection.show_system = !f.source.introspection.show_system;
        };
        let system_label = "Show internal system schemas";
        let system =
            self.check("source-show-system", system_label, s.introspection.show_system, true, toggle_system, cx);
        rows.push(div().child(system));
        if postgres {
            let toggle = |f: &mut SourceForm| {
                f.source.introspection.show_templates = !f.source.introspection.show_templates;
            };
            let on = s.introspection.show_templates;
            let templates = self.check("source-show-templates", "Show template databases", on, true, toggle, cx);
            rows.push(div().child(templates));
        }
        let editing = self.form.editing.is_some();
        let load = tx(cx, "Load Names from Server");
        let load = button(cx, load, false, editing && !self.form.busy)
            .id("source-load-names")
            .debug_selector(|| "source-load-names".to_owned())
            .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.load_names(cx)));
        let hint = if editing {
            "The Database tree shows only the checked names and patterns (* and ?); the connected database \
             always shows."
        } else {
            "Save the data source to pick from the server's names; patterns (* and ?) work any time."
        };
        let hint = tx(cx, hint);
        rows.push(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(load)
                .child(div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(hint))),
        );
        rows
    }

    fn advanced_tab(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let (names, hint) = property_hint(self.form.source.driver);
        let mut rows = Vec::new();
        let (name_head, value_head) = (tx(cx, "Name"), tx(cx, "Value"));
        rows.push(
            div()
                .flex()
                .gap(px(8.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(div().flex_1().child(SharedString::from(name_head)))
                .child(div().flex_1().child(SharedString::from(value_head)))
                .child(div().w(px(24.0))),
        );
        for i in 0..self.form.source.properties.len() {
            let remove = div()
                .id(SharedString::from(format!("source-remove-property-{i}")))
                .debug_selector(move || format!("source-remove-property-{i}"))
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .child(theme::icon(cx, "close", 12.0, theme::color(cx, "fg-subtle")))
                .on_click(cx.listener(move |dialog, _: &ClickEvent, window, cx| dialog.remove_property(i, window, cx)));
            rows.push(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(self.text_box(Field::PropertyName(i), false, true, cx))
                    .child(self.text_box(Field::PropertyValue(i), false, true, cx))
                    .child(remove),
            );
        }
        let add = tx(cx, "Add Property");
        rows.push(
            div().child(
                button(cx, add, false, true)
                    .id("source-add-property")
                    .debug_selector(|| "source-add-property".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, window, cx| dialog.add_property(window, cx))),
            ),
        );
        let suggested = names.join(", ");
        rows.push(Self::hint(cx, &suggested).pl(px(0.0)));
        rows.push(Self::hint(cx, hint).pl(px(0.0)));
        rows
    }
}

fn section(cx: &mut App, title: &str) -> Div {
    let title = tx(cx, title);
    div()
        .pt(px(6.0))
        .text_color(theme::color(cx, "fg-strong"))
        .font_weight(FontWeight::SEMIBOLD)
        .child(SharedString::from(title))
}

impl Focusable for SourceDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SourceDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.form.title();
        let title = tx_template(cx, &title);
        let name_hint = self.form.source.name.trim().is_empty().then(|| {
            div().flex_none().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(self.form.name()))
        });
        let (on, enabled) = (self.form.source.global, self.form.source.plugin_owner.is_none());
        let global = self.check("source-global", "Global", on, enabled, |f| f.source.global = !f.source.global, cx);
        let name_row = div()
            .flex_1()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(self.text_box(Field::Name, false, true, cx))
            .children(name_hint)
            .child(self.picker(Picker::Color, true, cx))
            .child(global);
        let name_row = Self::row(name_row, "Name:", cx);
        let comment = if self.form.show_comment {
            frame(cx, false).h(px(ROW_HEIGHT * 2.5)).child(div().flex_1().child(self.comment.clone()))
        } else {
            let add = tx(cx, "Add comment");
            div().child(
                div()
                    .id("source-add-comment")
                    .debug_selector(|| "source-add-comment".to_owned())
                    .text_color(theme::color(cx, "accent"))
                    .child(SharedString::from(add))
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| {
                        dialog.form.show_comment = true;
                        cx.notify();
                    })),
            )
        };
        let current = self.form.current_tab();
        let tabs: Vec<_> = self
            .form
            .tabs()
            .into_iter()
            .map(|tab| {
                let label = tx(cx, tab.label());
                div()
                    .id(SharedString::from(format!("source-tab-{tab:?}")))
                    .debug_selector(move || format!("source-tab-{tab:?}"))
                    .px(px(10.0))
                    .py(px(4.0))
                    .when(tab == current, |el| el.border_b_1().border_color(theme::color(cx, "accent")))
                    .text_color(theme::color(cx, if tab == current { "fg-strong" } else { "fg-muted" }))
                    .child(SharedString::from(label))
                    .on_click(cx.listener(move |dialog, _: &ClickEvent, _, cx| dialog.select_tab(tab, cx)))
            })
            .collect();
        let body = match current {
            SourceTab::General => self.general_tab(cx),
            SourceTab::Options => self.options_tab(cx),
            SourceTab::Ssh => self.ssh_tab(cx),
            SourceTab::Schemas => self.schemas_tab(cx),
            SourceTab::Advanced => self.advanced_tab(cx),
        };
        let status = match (&self.form.test, self.form.busy) {
            (None, true) => Some((tx(cx, "Connecting…"), true, false)),
            (Some(t), _) => Some((tx_template(cx, &t.text), t.ok, t.trust)),
            (None, false) => None,
        };
        let status = status.map(|(text, ok, trust)| {
            let retry = trust.then(|| {
                let label = tx(cx, "Trust Host and Retry");
                button(cx, label, false, !self.form.busy)
                    .id("source-trust")
                    .debug_selector(|| "source-trust".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.trust(cx)))
            });
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .debug_selector(|| "source-status".to_owned())
                .text_color(theme::color(cx, if ok { "success" } else { "danger" }))
                .child(SharedString::from(text))
                .children(retry)
        });
        let valid = self.form.valid() && !self.form.busy;
        let (test, ok, cancel) = (tx(cx, "Test Connection"), tx(cx, "OK"), tx(cx, "Cancel"));
        let footer = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(16.0))
            .py(px(12.0))
            .child(
                button(cx, test, false, valid)
                    .id("source-test")
                    .debug_selector(|| "source-test".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press_test(cx))),
            )
            .child(div().flex_1())
            .child(
                button(cx, cancel, false, true)
                    .id("source-cancel")
                    .debug_selector(|| "source-cancel".to_owned())
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(SourceDialogEvent::Cancelled))),
            )
            .child(
                button(cx, ok, true, valid)
                    .id("source-ok")
                    .debug_selector(|| "source-ok".to_owned())
                    .on_click(cx.listener(|dialog, _: &ClickEvent, _, cx| dialog.press_ok(cx))),
            );
        div()
            .w(px(WIDTH))
            .flex()
            .flex_col()
            .bg(theme::color(cx, "bg-popup"))
            .border_1()
            .border_color(theme::color(cx, "border-popup"))
            .rounded(theme::length(cx, "radius-popup"))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .pt(px(14.0))
                    .pb(px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(SharedString::from(title)),
            )
            .child(div().px(px(16.0)).flex().flex_col().gap(px(6.0)).child(name_row).child(comment))
            .child(div().px(px(16.0)).pt(px(8.0)).flex().gap(px(2.0)).children(tabs))
            .child(div().px(px(16.0)).py(px(8.0)).flex().flex_col().gap(px(6.0)).children(body).children(status))
            .child(footer)
    }
}

#[cfg(test)]
mod tests {
    use super::super::dialogs::blank_source;
    use super::*;
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext};
    use ide_core::error::ErrorKind;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<SourceDialogEvent>>>;

    fn new_form() -> SourceForm {
        SourceForm::new(blank_source("id-1".into()), None, None)
    }

    fn open(cx: &mut TestAppContext, form: SourceForm) -> (Entity<SourceDialog>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (dialog, cx) = cx.add_window_view(move |window, cx| SourceDialog::new(form, window, cx));
        let events: Events = Rc::default();
        let log = Rc::clone(&events);
        cx.update(|window, cx| {
            cx.subscribe(&dialog, move |_, event: &SourceDialogEvent, _| log.borrow_mut().push(event.clone())).detach();
            window.activate_window();
        });
        cx.run_until_parked();
        (dialog, events, cx)
    }

    fn focus_field(dialog: &Entity<SourceDialog>, field: Field, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let handle = dialog.read(cx).field(field).expect("the field exists").focus_handle(cx);
            window.focus(&handle, cx);
        });
    }

    fn focus_password(dialog: &Entity<SourceDialog>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let handle = dialog.read(cx).password_field().focus_handle(cx);
            window.focus(&handle, cx);
        });
    }

    fn field_text(dialog: &Entity<SourceDialog>, field: Field, cx: &mut VisualTestContext) -> String {
        dialog.read_with(cx, |d, cx| d.field(field).map(|f| f.read(cx).text().to_owned())).unwrap_or_default()
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        cx.run_until_parked();
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
    }

    fn saved(events: &Events) -> Option<(DataSource, SecretChange, SecretChange)> {
        events.borrow().iter().rev().find_map(|e| match e {
            SourceDialogEvent::Save { source, password, ssh_secret } => {
                Some(((**source).clone(), password.clone(), ssh_secret.clone()))
            }
            _ => None,
        })
    }

    #[gpui_kit::test]
    async fn does_not_save_the_database_password_unless_asked_to(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, new_form());
        focus_password(&dialog, cx);
        cx.simulate_input("pw");
        click(cx, "source-ok");
        let (source, password, _) = saved(&events).expect("OK saves");
        assert!(!source.save_password);
        assert_eq!(password, SecretChange::Set("pw".into()));
        assert_eq!(source.name, "db@localhost");
    }

    #[gpui_kit::test]
    async fn does_not_save_the_ssh_tunnel_password_unless_asked_to(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, new_form());
        cx.update(|window, cx| {
            dialog.update(cx, |d, cx| {
                d.edit(
                    |f| {
                        f.edit_ssh(|ssh| {
                            ssh.enabled = true;
                            ssh.auth = DbSshAuth::Password;
                            ssh.host = "bastion".into();
                        })
                    },
                    window,
                    cx,
                );
                d.select_tab(SourceTab::Ssh, cx);
            });
        });
        cx.update(|window, cx| {
            let handle = dialog.read(cx).ssh_secret_field().focus_handle(cx);
            window.focus(&handle, cx);
        });
        cx.simulate_input("tunnel");
        click(cx, "source-ok");
        let (source, password, ssh_secret) = saved(&events).expect("OK saves");
        assert!(!source.save_password);
        assert_eq!(source.ssh.map(|s| s.save_secret), Some(false));
        assert_eq!(password, SecretChange::Keep);
        assert_eq!(ssh_secret, SecretChange::Set("tunnel".into()));
    }

    #[gpui_kit::test]
    async fn shows_the_password_as_one_bullet_per_character(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, new_form());
        focus_password(&dialog, cx);
        cx.simulate_input("s3cré");
        let password = dialog.read_with(cx, |d, _| d.password_field().clone());
        assert_eq!(password.read_with(cx, |p, cx| p.display(cx)), "\u{2022}".repeat(5));
        assert_eq!(dialog.read_with(cx, |d, _| d.form().password.clone()).as_deref(), Some("s3cré"));
        // The password is not copied, and editing keys work on it.
        cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("before".to_owned()));
        cx.simulate_keystrokes("cmd-a cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("before"));
        cx.simulate_keystrokes("right backspace");
        assert_eq!(dialog.read_with(cx, |d, _| d.form().password.clone()).as_deref(), Some("s3cr"));
        let typed = password.read_with(cx, |p, cx| p.text(cx).to_owned());
        assert_eq!(typed, "s3cr");
    }

    #[gpui_kit::test]
    async fn fills_the_fields_from_a_typed_url_and_shows_the_fields_url(cx: &mut TestAppContext) {
        let (dialog, _, cx) = open(cx, new_form());
        focus_field(&dialog, Field::Url, cx);
        cx.simulate_keystrokes("cmd-a backspace");
        cx.simulate_input("postgres://ann:pw@db.example:6543/app");
        assert_eq!(field_text(&dialog, Field::Host, cx), "db.example");
        assert_eq!(field_text(&dialog, Field::Port, cx), "6543");
        assert_eq!(field_text(&dialog, Field::User, cx), "ann");
        assert_eq!(field_text(&dialog, Field::Database, cx), "app");
        let password = dialog.read_with(cx, |d, _| d.password_field().clone());
        assert_eq!(password.read_with(cx, |p, cx| p.display(cx)), "\u{2022}".repeat(2));
        focus_field(&dialog, Field::Database, cx);
        cx.simulate_input("2");
        let url = field_text(&dialog, Field::Url, cx);
        let expected = dialog.read_with(cx, |d, _| source_url(&d.form().source));
        assert_eq!(url, expected);
        assert!(url.contains("app2"));
    }

    #[gpui_kit::test]
    async fn tests_the_connection_and_offers_to_trust_an_unknown_host(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, new_form());
        click(cx, "source-test");
        assert!(matches!(events.borrow().last(), Some(SourceDialogEvent::Test { .. })));
        let unknown = IdeError::new(ErrorKind::PermissionDenied, "Unknown SSH host key for bastion");
        dialog.update(cx, |d, cx| d.test_finished(Err(unknown), cx));
        click(cx, "source-trust");
        assert_eq!(
            events.borrow().last(),
            Some(&SourceDialogEvent::TrustHost { config: None, source_id: "id-1".into() })
        );
        dialog.update(cx, |d, cx| d.test_finished(Ok("PostgreSQL 16".into()), cx));
        let test = dialog.read_with(cx, |d, _| d.form().test.clone());
        assert_eq!(test, Some(TestOutcome { ok: true, text: "Succeeded: PostgreSQL 16".into(), trust: false }));
    }

    #[gpui_kit::test]
    async fn keeps_ok_disabled_until_the_settings_are_complete(cx: &mut TestAppContext) {
        let (dialog, events, cx) = open(cx, new_form());
        focus_field(&dialog, Field::Host, cx);
        cx.simulate_keystrokes("cmd-a backspace");
        click(cx, "source-ok");
        assert!(saved(&events).is_none());
        dialog.update(cx, |d, cx| d.save_failed("disk full", cx));
        assert_eq!(dialog.read_with(cx, |d, _| d.form().test.clone().map(|t| t.text)).as_deref(), Some("disk full"));
    }

    #[test]
    fn makes_up_a_name_from_the_server_or_the_file() {
        let mut f = new_form();
        assert_eq!(f.name(), "db@localhost");
        f.source.user = "ann".into();
        assert_eq!(f.name(), "ann@localhost");
        f.source.database = "app".into();
        f.source.host = "db".into();
        assert_eq!(f.name(), "app@db");
        f.set_type("sqlite");
        f.set_field(Field::File, "/data/shop.db");
        assert_eq!(f.name(), "shop");
        assert!(f.valid());
        assert_eq!(f.tabs(), vec![SourceTab::General, SourceTab::Options, SourceTab::Advanced]);
    }

    #[test]
    fn switches_type_with_its_port_and_clickhouse_defaults() {
        let mut f = new_form();
        f.set_type("mysql");
        assert_eq!((f.source.driver, f.source.port), (DbDriver::Mysql, Some(3306)));
        f.set_field(Field::Port, "3307x");
        assert_eq!(f.source.port, Some(3307));
        f.set_type("postgres");
        assert_eq!(f.source.port, Some(3307), "a port the user typed stays");
        f.set_field(Field::Port, "");
        f.set_type("clickhouse");
        assert_eq!((f.source.port, f.source.ssl), (Some(8123), DbSslMode::Disable));
        assert_eq!((f.source.user.as_str(), f.source.database.as_str()), ("default", "default"));
        f.set_ssl(DbSslMode::VerifyFull);
        assert_eq!(f.source.port, Some(8443));
        f.set_ssl(DbSslMode::Disable);
        assert_eq!(f.source.port, Some(8123));
    }

    #[test]
    fn validates_url_only_and_the_ssh_tunnel() {
        let mut f = new_form();
        f.set_url_only(true);
        assert!(f.source.url.starts_with("jdbc:postgresql://"), "URL only starts from the fields");
        f.set_field(Field::Url, "mysql://h/db");
        assert!(!f.valid() && f.url_invalid());
        assert_eq!(f.url_hint(), "Not a PostgreSQL URL");
        f.set_field(Field::Url, "postgres://h/db");
        assert!(f.valid());
        f.set_url_only(false);
        f.edit_ssh(|ssh| ssh.enabled = true);
        assert!(!f.valid(), "an enabled tunnel needs its host");
        f.set_field(Field::SshHost, "bastion");
        assert!(f.valid());
    }

    #[test]
    fn saves_secrets_only_when_typed() {
        let mut f = SourceForm::new(blank_source("id".into()), Some("Local".into()), None);
        assert_eq!(f.save_request().1, SecretChange::Keep);
        f.password = Some(String::new());
        assert_eq!(f.save_request().1, SecretChange::Forget);
        f.edit_ssh(|ssh| {
            ssh.enabled = true;
            ssh.host = "b".into();
        });
        f.ssh_secret = Some("s".into());
        assert_eq!(f.save_request().2, SecretChange::Keep, "OpenSSH authentication has nothing to store");
        f.edit_ssh(|ssh| ssh.auth = DbSshAuth::KeyPair);
        assert_eq!(f.save_request().2, SecretChange::Set("s".into()));
        f.source.properties = vec![
            DbProperty { name: " ".into(), value: "x".into() },
            DbProperty { name: "work_mem".into(), value: "64MB".into() },
        ];
        assert_eq!(f.save_request().0.properties.len(), 1);
        assert_eq!(f.title(), "Data Source: Local");
    }

    #[test]
    fn keeps_seconds_options_and_name_patterns() {
        let mut f = new_form();
        f.set_seconds(true, true);
        assert_eq!(f.source.options.auto_disconnect_secs, Some(300));
        f.set_field(Field::AutoDisconnect, "45");
        assert_eq!(f.source.options.auto_disconnect_secs, Some(45));
        f.set_seconds(true, false);
        assert_eq!(f.source.options.auto_disconnect_secs, None);
        f.known_schemas = vec!["public".into()];
        f.set_field(Field::SchemaPattern, " app_* ");
        f.add_pattern(false);
        f.toggle_name(false, "public");
        assert_eq!(f.source.introspection.schemas, vec!["app_*".to_owned(), "public".to_owned()]);
        assert_eq!(f.picker_names(false), vec!["public".to_owned(), "app_*".to_owned()]);
        f.toggle_name(false, "public");
        assert_eq!(f.source.introspection.schemas, vec!["app_*".to_owned()]);
    }
}
