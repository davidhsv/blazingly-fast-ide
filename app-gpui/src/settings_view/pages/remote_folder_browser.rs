//! The folder chooser the settings pages open over a filesystem connection (an SFTP server reached
//! with an SSH configuration, or a local or mounted folder): Select Folder on Target of Run
//! Targets, and Choose Remote Folder / Choose Folder behind the Browse… buttons of Deployment.
//!
//! It opens on a folder (`~`, the login folder, while none is given) and lists its subfolders by
//! name; `~` turns absolute once a listing tells where it is. A folder is selected with a click and
//! opened with a double click; the path can be typed (Enter lists it), the parent opened and the
//! listing read again. OK hands back the folder selected, else the one listed
//! ([`FolderBrowserEvent::Chosen`]); nothing on the server is created or changed by browsing, and a
//! listed name is only drawn as text.
//!
//! A listing that fails on an unknown host key offers to trust it; one that fails for a missing or
//! wrong password or passphrase takes it in a [`SecretField`] and hands it to the backend, then
//! lists again. The secret leaves the chooser only as a [`SecretText`]; it is never staged, drawn
//! (bullets only), listed or logged, and it is cleared once a new listing starts.
//!
//! The two pages ask for slightly different choosers, named by [`Flavour`]: the title, the order
//! of OK and Cancel, when Parent Folder can be used, what an empty typed path does, which listings
//! turn the path absolute, how a failure is answered, and whether the list takes the keyboard.
//!
//! Listing runs on the backend's runtime and sorting off the window's thread; an answer to a
//! listing the user has since moved on from is dropped. Only the rows in view are drawn, so a
//! folder of any size keeps the window responsive.

mod panel;
#[cfg(test)]
mod proofs37_remote_folder_browser;

use crate::database::store::SecretText;
use crate::platform::collate;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::settings_model::deployment_model::remote_parent;
use crate::settings_model::run_targets::parent_path;
use crate::settings_model::ssh_connection::{SshTestRemedy, ssh_test_remedy};
use crate::settings_view::controls::tx;
use crate::settings_view::pages::ssh_backend::{HostTrust, SecretSetter, on_backend};
use crate::settings_view::pages::{Then, new_field};
use crate::ui::secret_field::{SecretField, SecretFieldEvent};
use crate::ui::text_field::{TextField, TextFieldEvent};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, Keystroke, ScrollStrategy,
    Subscription, UniformListScrollHandle,
};
use ide_core::IdeError;
use ide_core::settings::SshAuth;
use ide_ssh::{FsConnection, RemoteEntry};
use std::rc::Rc;

/// Lists folder `path` (`~` for the login folder) of a connection.
pub type ListFolder = Rc<dyn Fn(FsConnection, String, Then<Result<Vec<RemoteEntry>, IdeError>>, &mut App)>;

/// What the chooser asks of the backend: the listing, host key trust and the password.
#[derive(Clone)]
pub struct FolderBackend {
    pub list: ListFolder,
    pub trust_host: HostTrust,
    pub set_secret: SecretSetter,
}

/// The backend's listing (`remote_list`), as its command calls it.
pub fn backend_list() -> ListFolder {
    type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
    Rc::new(|connection: FsConnection, path: String, then: Listed, cx: &mut App| {
        on_backend(
            move |host| async move { ide_host::ops::deploy::remote_list(&host, connection, path).await },
            then,
            cx,
        );
    })
}

/// The key of the password a connection asks for, as the backend keeps it: the SSH configuration
/// of an SFTP server, `ftp:<server id>` for an FTP or FTPS one, none for a local folder.
pub fn secret_key(connection: &FsConnection) -> String {
    match connection {
        FsConnection::Sftp { ssh } => ssh.clone(),
        FsConnection::Ftp { server, .. } => ide_ssh::ftp::secret_key(server),
        FsConnection::Local => String::new(),
    }
}

/// The listing failed on a host key the user has not trusted yet.
pub fn needs_host_trust(error: &str) -> bool {
    error.starts_with("Unknown SSH host key")
}

/// The listing failed for a missing or wrong password or key passphrase.
pub fn needs_secret(error: &str) -> bool {
    error.starts_with("SSH password required")
        || error.starts_with("SSH key passphrase required")
        || error.contains("passphrase required")
        || error.starts_with("cannot decrypt SSH key")
}

/// Which page's chooser this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavour {
    /// Run Targets › Project folder on target.
    Target,
    /// Deployment › Root path and a mapping's Deployment path.
    Deployment,
}

impl Flavour {
    /// The prefix of the selectors the chooser is drawn under.
    fn selector(self) -> &'static str {
        match self {
            Self::Target => "settings-target-folder",
            Self::Deployment => "settings-deployment-folder",
        }
    }

    fn secret_selector(self) -> &'static str {
        match self {
            Self::Target => "settings-secret-target.folder",
            Self::Deployment => "settings-secret-deployment.folder",
        }
    }

    fn list_id(self) -> &'static str {
        match self {
            Self::Target => "settings-target-folder-rows",
            Self::Deployment => "settings-deployment-folder-rows",
        }
    }

    fn title(self, connection: &FsConnection) -> &'static str {
        match (self, connection) {
            (Self::Target, _) => "Select Folder on Target",
            (Self::Deployment, FsConnection::Local) => "Choose Folder",
            (Self::Deployment, FsConnection::Sftp { .. } | FsConnection::Ftp { .. }) => "Choose Remote Folder",
        }
    }

    /// The folder first listed for the text of the field the chooser fills.
    fn initial(self, text: &str) -> String {
        let text = match self {
            Self::Target => text,
            Self::Deployment => text.trim(),
        };
        if text.is_empty() { "~".to_owned() } else { text.to_owned() }
    }

    /// The parent of the folder shown; none where Parent Folder cannot be used.
    fn parent(self, path: &str) -> Option<String> {
        match self {
            Self::Target => parent_path(path).map(str::to_owned),
            Self::Deployment => (path != "/" && path != "~").then(|| remote_parent(path).to_owned()),
        }
    }

    /// The folder listed for a path typed and Enter; none when nothing is listed.
    fn submitted(self, typed: &str) -> Option<String> {
        let typed = typed.trim();
        match self {
            Self::Target => (!typed.is_empty()).then(|| typed.to_owned()),
            Self::Deployment => Some(if typed.is_empty() { "~".to_owned() } else { typed.to_owned() }),
        }
    }

    /// The absolute folder a listing of `listed` turned out to be, from the path of its first
    /// entry; none when the path shown stays as asked. Run Targets names every listing by where it
    /// is; Deployment names only the login folder, a relative path staying as typed.
    fn absolute(self, listed: &str, first: &str) -> Option<String> {
        match self {
            Self::Target => parent_path(first).filter(|absolute| *absolute != listed).map(str::to_owned),
            Self::Deployment => (listed == "~" || listed.is_empty()).then(|| remote_parent(first).to_owned()),
        }
    }
}

/// What the chooser shows below its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderState {
    Loading,
    /// The names of the subfolders.
    Listed(Vec<String>),
    /// The error as shown.
    Failed(String),
}

/// What the chooser reports to the page that opened it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FolderBrowserEvent {
    /// OK: the folder chosen, absolute or as typed.
    Chosen(String),
    /// Cancel or Escape.
    Closed,
    /// What it shows changed.
    Changed,
}

/// How a chooser opens.
pub struct FolderSetup {
    pub flavour: Flavour,
    pub connection: FsConnection,
    /// The text of the field it fills; the folder first listed is made from it.
    pub initial: String,
    pub backend: FolderBackend,
    /// The authentication of the SSH configuration named by the connection's password key, which
    /// decides whether a rejected SSH login asks for a password.
    pub auth: SshAuth,
}

pub struct RemoteFolderBrowser {
    flavour: Flavour,
    connection: FsConnection,
    backend: FolderBackend,
    auth: SshAuth,
    /// The folder shown: the one asked for, or where it turned out to be.
    path: String,
    /// The folder last asked for.
    listed: String,
    path_field: Entity<TextField>,
    /// The subfolders of the folder shown, sorted by name; none while they are read.
    entries: Option<Vec<RemoteEntry>>,
    error: Option<IdeError>,
    selected: Option<usize>,
    secret: Entity<SecretField>,
    /// Save of the password step (Deployment): the backend keeps the password in the saved ones.
    save: bool,
    /// A trust or a password is being handed over (Deployment).
    busy: bool,
    /// Why the last trust or password failed (Deployment).
    failure: Option<String>,
    /// Counts the listings asked for: an answer to an earlier one is dropped.
    request: u64,
    focus: FocusHandle,
    /// The list's claim on the keyboard (Deployment).
    claim: Option<u64>,
    scroll: UniformListScrollHandle,
    /// The window last drawn in, for the password field to take the keyboard when it appears.
    window: Option<AnyWindowHandle>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<FolderBrowserEvent> for RemoteFolderBrowser {}

impl RemoteFolderBrowser {
    /// The chooser, listing nothing yet: [`Self::start`] lists the first folder once the page
    /// listens to it.
    pub fn new(setup: FolderSetup, cx: &mut Context<Self>) -> Self {
        let FolderSetup { flavour, connection, initial, backend, auth } = setup;
        let initial = flavour.initial(&initial);
        let path_field = new_field(&initial, None, cx);
        let placeholder = tx(cx, "Password or passphrase");
        let secret = cx.new(|cx| SecretField::new(cx).with_placeholder(placeholder));
        let path_events = cx.subscribe(&path_field, |view: &mut Self, field, event: &TextFieldEvent, cx| match event {
            TextFieldEvent::Changed => cx.notify(),
            TextFieldEvent::Submitted => {
                let typed = field.read(cx).text().to_owned();
                if let Some(next) = view.flavour.submitted(&typed) {
                    view.go(next, cx);
                }
            }
            TextFieldEvent::Cancelled => cx.emit(FolderBrowserEvent::Closed),
        });
        let secret_events = cx.subscribe(&secret, |view: &mut Self, _, event: &SecretFieldEvent, cx| match event {
            SecretFieldEvent::Changed => cx.notify(),
            SecretFieldEvent::Submitted => view.connect(cx),
            SecretFieldEvent::Cancelled => cx.emit(FolderBrowserEvent::Closed),
        });
        let focus = cx.focus_handle();
        let claim = (flavour == Flavour::Deployment).then(|| {
            let host = cx.weak_entity();
            input::claim_keys(
                focus.clone(),
                move |keystroke, _, cx| host.update(cx, |view, cx| view.list_key(keystroke, cx)).unwrap_or(false),
                cx,
            )
        });
        // A closed chooser keeps no password typed, even for a moment while its field outlives it.
        cx.on_release(|view: &mut Self, cx: &mut App| {
            if let Some(claim) = view.claim {
                input::release_keys(claim, cx);
            }
            view.secret.update(cx, |field, cx| field.clear(cx));
        })
        .detach();
        Self {
            flavour,
            connection,
            backend,
            auth,
            path: initial.clone(),
            listed: initial,
            path_field,
            entries: None,
            error: None,
            selected: None,
            secret,
            save: false,
            busy: false,
            failure: None,
            request: 0,
            focus,
            claim,
            scroll: UniformListScrollHandle::new(),
            window: None,
            _subscriptions: vec![path_events, secret_events],
        }
    }

    /// Lists the first folder.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let first = self.path.clone();
        self.go(first, cx);
    }

    pub fn flavour(&self) -> Flavour {
        self.flavour
    }

    pub fn connection(&self) -> &FsConnection {
        &self.connection
    }

    /// The folder shown.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn path_field(&self) -> &Entity<TextField> {
        &self.path_field
    }

    pub fn secret_field(&self) -> &Entity<SecretField> {
        &self.secret
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    pub fn state(&self, cx: &App) -> FolderState {
        match (&self.error, &self.entries) {
            (Some(error), _) => FolderState::Failed(self.error_text(&error.message, cx)),
            (None, None) => FolderState::Loading,
            (None, Some(entries)) => FolderState::Listed(entries.iter().map(|entry| entry.name.clone()).collect()),
        }
    }

    /// The folder OK hands back: the one selected, else the one shown.
    pub fn chosen(&self) -> String {
        let selected = self.selected.and_then(|index| self.entries.as_ref()?.get(index));
        selected.map_or_else(|| self.path.clone(), |entry| entry.path.clone())
    }

    /// OK can be used: the listing did not fail, and Deployment's chooser has a folder that is not
    /// the login folder still unnamed.
    pub fn can_choose(&self) -> bool {
        self.error.is_none() && (self.flavour == Flavour::Target || self.chosen() != "~")
    }

    /// What changed is drawn, and the page told.
    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.notify();
        cx.emit(FolderBrowserEvent::Changed);
    }

    /// Lists folder `next`, forgetting the listing, the error, the selection and the password step
    /// shown.
    pub fn go(&mut self, next: String, cx: &mut Context<Self>) {
        self.request += 1;
        let request = self.request;
        self.entries = None;
        self.error = None;
        self.selected = None;
        self.secret.update(cx, |field, cx| field.clear(cx));
        self.save = false;
        self.busy = false;
        self.failure = None;
        self.path = next.clone();
        self.listed = next.clone();
        self.path_field.update(cx, |field, cx| field.set_text(&next, cx));
        let view = cx.weak_entity();
        let then: Then<Result<Vec<RemoteEntry>, IdeError>> =
            Rc::new(move |result: Result<Vec<RemoteEntry>, IdeError>, cx: &mut App| {
                view.update(cx, |view, cx| view.listed(request, result, cx)).ok();
            });
        let (list, connection) = (self.backend.list.clone(), self.connection.clone());
        // After this turn: a listing that answers at once finds the chooser free to update.
        cx.defer(move |cx| list(connection, next, then, cx));
        self.changed(cx);
    }

    /// Lists the folder shown again (Refresh, and after a trust or a password).
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        let path = self.path.clone();
        self.go(path, cx);
    }

    /// The answer of listing `request`: its subfolders are sorted off the window's thread.
    fn listed(&mut self, request: u64, result: Result<Vec<RemoteEntry>, IdeError>, cx: &mut Context<Self>) {
        if request != self.request {
            return;
        }
        match result {
            Ok(list) => {
                let (flavour, listed) = (self.flavour, self.listed.clone());
                let sorting = cx.background_executor().spawn(async move { sorted_folders(flavour, &listed, list) });
                cx.spawn(async move |this, cx| {
                    let (folders, absolute) = sorting.await;
                    this.update(cx, |view, cx| view.show(request, folders, absolute, cx)).ok();
                })
                .detach();
            }
            Err(error) => {
                self.error = Some(error);
                self.focus_secret_when_asked(cx);
                self.changed(cx);
            }
        }
    }

    /// The sorted subfolders of listing `request`; the folder shown becomes absolute when the
    /// listing told where it is, without listing it again.
    fn show(&mut self, request: u64, folders: Vec<RemoteEntry>, absolute: Option<String>, cx: &mut Context<Self>) {
        if request != self.request {
            return;
        }
        self.entries = Some(folders);
        if let Some(absolute) = absolute {
            self.path = absolute.clone();
            self.path_field.update(cx, |field, cx| field.set_text(&absolute, cx));
        }
        self.changed(cx);
    }

    /// The password field of Deployment's chooser takes the keyboard when a listing asks for it.
    fn focus_secret_when_asked(&self, cx: &mut Context<Self>) {
        if self.flavour != Flavour::Deployment || self.remedy() != Some(SshTestRemedy::Secret) {
            return;
        }
        let (Some(window), focus) = (self.window, self.secret.focus_handle(cx)) else { return };
        cx.defer(move |cx| {
            window.update(cx, |_, window, cx| window.focus(&focus, cx)).ok();
        });
    }

    /// The key of the password the connection asks for ([`secret_key`]).
    fn ssh(&self) -> String {
        secret_key(&self.connection)
    }

    /// What the failed listing asks of the user.
    pub fn remedy(&self) -> Option<SshTestRemedy> {
        let error = self.error.as_ref()?;
        match self.flavour {
            Flavour::Target if needs_host_trust(&error.message) => Some(SshTestRemedy::TrustHostKey),
            Flavour::Target if needs_secret(&error.message) => Some(SshTestRemedy::Secret),
            Flavour::Target => None,
            Flavour::Deployment => {
                let kind = serde_json::to_value(error.kind).ok().and_then(|kind| kind.as_str().map(str::to_owned));
                ssh_test_remedy(kind.as_deref(), &error.message, self.auth)
            }
        }
    }

    /// An error as shown: Deployment's through the template lookup (`Folder does not exist: …`).
    fn error_text(&self, message: &str, cx: &App) -> String {
        match (self.flavour, cx.try_global::<Localization>()) {
            (Flavour::Deployment, Some(localization)) => localization.0.tx_template(message),
            _ => message.to_owned(),
        }
    }

    /// Hands `step` the SSH configuration and lists again once it succeeds.
    fn remedy_step(
        &mut self,
        step: impl FnOnce(String, Then<Result<(), IdeError>>, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.busy {
            return;
        }
        self.busy = self.flavour == Flavour::Deployment;
        self.failure = None;
        let (ssh, request) = (self.ssh(), self.request);
        let view = cx.weak_entity();
        let then: Then<Result<(), IdeError>> = Rc::new(move |result: Result<(), IdeError>, cx: &mut App| {
            view.update(cx, |view, cx| view.remedied(request, result, cx)).ok();
        });
        cx.defer(move |cx| step(ssh, then, cx));
        self.changed(cx);
    }

    /// How a trust or a password went: listed again, or the failure shown (Run Targets shows it in
    /// the error's place, Deployment under the problem it keeps).
    fn remedied(&mut self, request: u64, result: Result<(), IdeError>, cx: &mut Context<Self>) {
        if request != self.request {
            return;
        }
        self.busy = false;
        if self.flavour == Flavour::Target {
            self.secret.update(cx, |field, cx| field.clear(cx));
        }
        match result {
            Ok(()) => self.retry(cx),
            Err(error) => {
                match self.flavour {
                    Flavour::Target => self.error = Some(error),
                    Flavour::Deployment => self.failure = Some(self.error_text(&error.message, cx)),
                }
                self.changed(cx);
            }
        }
    }

    /// Trust Host / Trust Host Key and Retry.
    pub fn trust(&mut self, cx: &mut Context<Self>) {
        let trust = self.backend.trust_host.clone();
        self.remedy_step(move |ssh, then, cx| trust(ssh, then, cx), cx);
    }

    /// Connect: hands the backend the password or passphrase typed, for this session (Deployment:
    /// also kept in the saved passwords when Save is ticked), and lists again.
    pub fn connect(&mut self, cx: &mut Context<Self>) {
        let typed = self.secret.read(cx).text(cx).to_owned();
        if self.flavour == Flavour::Deployment && (typed.is_empty() || self.busy) {
            return;
        }
        let secret = SecretText::from(typed);
        // Run Targets' Connect never saves: a password saved before for the configuration is then
        // forgotten by the backend.
        let save = self.flavour == Flavour::Deployment && self.save;
        let set_secret = self.backend.set_secret.clone();
        self.remedy_step(move |ssh, then, cx| set_secret(ssh, secret, save, then, cx), cx);
    }

    /// Empties the password field: the chooser is closing.
    pub fn forget_secret(&mut self, cx: &mut Context<Self>) {
        self.secret.update(cx, |field, cx| field.clear(cx));
    }

    pub fn toggle_save(&mut self, cx: &mut Context<Self>) {
        self.save = !self.save;
        self.changed(cx);
    }

    pub fn select(&mut self, index: usize, cx: &mut Context<Self>) {
        self.selected = Some(index);
        self.changed(cx);
    }

    /// OK: hands back the folder chosen.
    pub fn choose(&mut self, cx: &mut Context<Self>) {
        if self.can_choose() {
            cx.emit(FolderBrowserEvent::Chosen(self.chosen()));
        }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(FolderBrowserEvent::Closed);
    }

    /// The keyboard of Deployment's list: Up / Down select, Enter opens the folder selected,
    /// Backspace opens the parent, while folders are listed; Escape closes. These keys stay with the
    /// list while it lists nothing or nothing is selected, as the list owns them in the chooser:
    /// they never reach the Settings dialog behind it.
    fn list_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        if keystroke.modifiers.modified() {
            return false;
        }
        if keystroke.key == "escape" {
            self.close(cx);
            return true;
        }
        if !matches!(keystroke.key.as_str(), "up" | "down" | "enter" | "backspace") {
            return false;
        }
        let len = self.entries.as_ref().filter(|_| self.error.is_none()).map_or(0, Vec::len);
        if len == 0 {
            return true;
        }
        match keystroke.key.as_str() {
            "up" | "down" => {
                let down = keystroke.key == "down";
                let next = match self.selected {
                    None => 0,
                    Some(at) if down => (at + 1).min(len - 1),
                    Some(at) => at.saturating_sub(1),
                };
                self.scroll.scroll_to_item(next, ScrollStrategy::Center);
                self.select(next, cx);
            }
            "enter" => {
                let selected = self.selected.and_then(|index| self.entries.as_ref()?.get(index));
                if let Some(path) = selected.map(|entry| entry.path.clone()) {
                    self.go(path, cx);
                }
            }
            "backspace" => {
                let parent = remote_parent(&self.path).to_owned();
                self.go(parent, cx);
            }
            _ => return false,
        }
        true
    }

    /// The chooser as text: its title, `[Parent Folder] <path> [Refresh]`, the subfolders (`>` the
    /// selected one), `Loading…`, `No subfolders` or `! <error>` with what can help (`[Trust …]`,
    /// the password as bullets and `[Connect]`), then OK and Cancel in the page's order, with
    /// `(disabled)` flags. The password is never listed.
    pub fn lines(&self, cx: &App) -> Vec<String> {
        let flag = |open: bool| if open { "" } else { " (disabled)" };
        let path = self.path_field.read(cx).text().to_owned();
        let parent = self.flavour.parent(&self.path).is_some();
        let mut out = vec![
            format!("— {} —", tx(cx, self.flavour.title(&self.connection))),
            format!("[{}]{} {path} [{}]", tx(cx, "Parent Folder"), flag(parent), tx(cx, "Refresh")),
        ];
        match (&self.error, &self.entries) {
            (Some(error), _) => {
                out.push(format!("! {}", self.error_text(&error.message, cx)));
                let bullets = self.secret.read(cx).shown(cx);
                match (self.flavour, self.remedy()) {
                    (Flavour::Target, Some(SshTestRemedy::TrustHostKey)) => {
                        out.push(format!("[{}]", tx(cx, "Trust Host")));
                    }
                    (Flavour::Target, Some(SshTestRemedy::Secret)) => {
                        out.push(format!("{}: {bullets} [{}]", tx(cx, "Password or passphrase"), tx(cx, "Connect")));
                    }
                    (Flavour::Deployment, Some(SshTestRemedy::TrustHostKey)) => {
                        out.push(format!("[{}]{}", tx(cx, "Trust Host Key and Retry"), flag(!self.busy)));
                    }
                    (Flavour::Deployment, Some(SshTestRemedy::Secret)) => {
                        let mark = if self.save { "x" } else { " " };
                        let can = !self.busy && !bullets.is_empty();
                        out.push(format!(
                            "{}: {bullets} [{mark}] {} [{}]{}",
                            tx(cx, "Password or passphrase"),
                            tx(cx, "Save"),
                            tx(cx, "Connect"),
                            flag(can)
                        ));
                    }
                    (_, None) => {}
                }
                out.extend(self.failure.as_ref().map(|failure| format!("! {failure}")));
            }
            (None, None) => out.push(tx(cx, "Loading…")),
            (None, Some(entries)) if entries.is_empty() => out.push(tx(cx, "No subfolders")),
            (None, Some(entries)) => out.extend(entries.iter().enumerate().map(|(index, entry)| {
                let mark = if self.selected == Some(index) { ">" } else { " " };
                format!("{mark} {}", entry.name)
            })),
        }
        let (ok, cancel) =
            (format!("[{}]{}", tx(cx, "OK"), flag(self.can_choose())), format!("[{}]", tx(cx, "Cancel")));
        out.push(match self.flavour {
            Flavour::Target => format!("{ok} {cancel}"),
            Flavour::Deployment => format!("{cancel} {ok}"),
        });
        out
    }
}

/// The folders of a listing sorted by name, and the absolute folder it turned out to be.
fn sorted_folders(flavour: Flavour, listed: &str, list: Vec<RemoteEntry>) -> (Vec<RemoteEntry>, Option<String>) {
    let absolute = list.first().and_then(|first| flavour.absolute(listed, &first.path));
    let mut folders: Vec<RemoteEntry> = list.into_iter().filter(|entry| entry.is_dir).collect();
    folders.sort_by(|a, b| collate::compare(&a.name, &b.name));
    (folders, absolute)
}

impl Focusable for RemoteFolderBrowser {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
