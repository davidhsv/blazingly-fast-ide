//! Browse… of Root path (Connection tab) and of a mapping's Deployment path (Mappings tab): the
//! settings pages' [`RemoteFolderBrowser`] over the server as edited (its type, SSH configuration
//! and root path, saved or not), drawn under the field it fills.
//!
//! Root path's chooser opens on the root path typed (`~`, the login folder, while it is empty)
//! and puts the folder chosen in the field. A mapping's opens on the root path joined with the
//! deployment path typed and puts the folder chosen there relative to the root path; a folder
//! outside the root path is not taken. A mapping's chooser needs an absolute root path, as the
//! deployment paths are relative to it. An SFTP server's chooser needs its SSH configuration and an
//! FTP or FTPS server's its host; when a listing asks for the host key to be trusted or for a
//! password (the SSH configuration's, or the FTP server's kept as `ftp:<server id>`), the trust and
//! the password go through the page's [`Connections`](super::Connections).
//!
//! The chooser is drawn inside the page, not over it, so the page stays usable while it is open: it
//! closes with another server, another tab or another page, when the server it lists stops being
//! reached the same way (its type, SSH configuration or FTP login changed), and, for a mapping's,
//! when a mapping is added or removed. Closing it empties its password field. It takes the keyboard
//! in its path field when it opens.

use super::connection::ssh_configs;
use super::{DeploymentServersView, tool};
use crate::platform_more::remote_settings::deployment_connection;
use crate::settings_model::deployment_model::{MappingPatch, folder_within, update_mapping};
use crate::settings_view::controls::{button, tx};
use crate::settings_view::pages::remote_folder_browser::{
    Flavour, FolderBackend, FolderBrowserEvent, FolderSetup, ListFolder, RemoteFolderBrowser, backend_list, secret_key,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, Focusable, IntoElement, Subscription, div, prelude::*,
};
use ide_core::settings::{DeploymentServer, DeploymentType, SshAuth};

/// The folder listing the page's choosers use, set by a test as a global; without one the
/// backend's.
pub struct DeploymentFolders(pub ListFolder);

impl gpui_kit::Global for DeploymentFolders {}

/// The listing set as [`DeploymentFolders`], else the backend's. A test build gets none of its own
/// making (Browse… is then unavailable): an SFTP server's listing opens an SSH connection, which
/// reads the user's `~/.ssh` files and reaches the network, and a local server's `~` is the user's
/// home folder.
pub fn folder_lister(cx: &App) -> Option<ListFolder> {
    match cx.try_global::<DeploymentFolders>() {
        Some(set) => Some(set.0.clone()),
        None if cfg!(test) => None,
        None => Some(backend_list()),
    }
}

/// The field a chooser fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseTarget {
    Root,
    /// The deployment path of mapping row `n`.
    Mapping(usize),
}

/// The open chooser and the field it fills.
pub(super) struct Chooser {
    target: BrowseTarget,
    /// How many mappings the server had when the chooser opened: a mapping's chooser fills a row
    /// by its place, which another number of rows moves.
    mappings: usize,
    view: Entity<RemoteFolderBrowser>,
    _events: Subscription,
}

/// The root path a mapping's deployment path is relative to: trimmed, without trailing slashes.
fn remote_root(server: &DeploymentServer) -> &str {
    server.root_path.trim().trim_end_matches('/')
}

/// A path that names its folder from the top: `/…`, or a drive letter with its separator.
fn is_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    path.starts_with('/')
        || (bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/'))
}

/// A server can be listed once it says how to reach it: an SFTP server its SSH configuration, an
/// FTP or FTPS server its host; a local one always.
fn reachable(server: &DeploymentServer) -> bool {
    match server.kind {
        DeploymentType::Sftp => !server.ssh_config.is_empty(),
        DeploymentType::Ftp | DeploymentType::Ftps => !server.ftp.host.trim().is_empty(),
        DeploymentType::Local => true,
    }
}

impl DeploymentServersView {
    /// Root path's Browse… can be used: a server is selected, the page can list folders, and the
    /// server says how to reach it (an SFTP server's SSH configuration, an FTP server's host).
    pub fn can_browse_root(&self, cx: &App) -> bool {
        folder_lister(cx).is_some() && self.current(cx).is_some_and(|server| reachable(&server))
    }

    /// A mapping's Browse… of its deployment path can be used: as Root path's, with an absolute
    /// root path.
    pub fn can_browse_mapping(&self, cx: &App) -> bool {
        folder_lister(cx).is_some()
            && self.current(cx).is_some_and(|server| reachable(&server) && is_absolute(remote_root(&server)))
    }

    /// The open chooser.
    pub fn chooser(&self) -> Option<&Entity<RemoteFolderBrowser>> {
        self.chooser.as_ref().map(|chooser| &chooser.view)
    }

    /// The field the open chooser fills.
    pub fn chooser_target(&self) -> Option<BrowseTarget> {
        self.chooser.as_ref().map(|chooser| chooser.target)
    }

    /// Root path › Browse…: the chooser on the root path typed.
    pub fn browse_root(&mut self, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx).filter(|_| self.can_browse_root(cx)) else { return };
        let initial = server.root_path.clone();
        self.open_chooser(BrowseTarget::Root, &server, initial, cx);
    }

    /// A mapping's Browse… of its deployment path: the chooser on the root path joined with the
    /// deployment path typed.
    pub fn browse_mapping(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(server) = self.current(cx).filter(|_| self.can_browse_mapping(cx)) else { return };
        let Some(mapping) = server.mappings.get(row) else { return };
        let deployment = mapping.deployment.trim().trim_start_matches('/');
        let parts = [remote_root(&server), deployment];
        let initial = parts.iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join("/");
        self.mapping_selected = Some(row);
        self.open_chooser(BrowseTarget::Mapping(row), &server, initial, cx);
    }

    fn open_chooser(
        &mut self,
        target: BrowseTarget,
        server: &DeploymentServer,
        initial: String,
        cx: &mut Context<Self>,
    ) {
        let Some(list) = folder_lister(cx) else { return };
        let connection = deployment_connection(server);
        // The configuration named by the password key: an FTP or local server's names none.
        let key = secret_key(&connection);
        let auth = ssh_configs(self, cx).iter().find(|config| config.id == key).map_or(SshAuth::OpenSsh, |c| c.auth);
        let backend = FolderBackend {
            list,
            trust_host: self.connections.trust_host.clone(),
            set_secret: self.connections.set_secret.clone(),
        };
        let setup = FolderSetup { flavour: Flavour::Deployment, connection, initial, backend, auth };
        let view = cx.new(|cx| RemoteFolderBrowser::new(setup, cx));
        let events = cx.subscribe(&view, |page: &mut Self, _, event: &FolderBrowserEvent, cx| match event {
            FolderBrowserEvent::Chosen(path) => page.chosen(path.clone(), cx),
            FolderBrowserEvent::Closed => page.close_chooser(cx),
            FolderBrowserEvent::Changed => cx.notify(),
        });
        self.open_select = None;
        self.close_chooser(cx);
        let mappings = server.mappings.len();
        self.chooser = Some(Chooser { target, mappings, view: view.clone(), _events: events });
        view.update(cx, |browser, cx| browser.start(cx));
        if let Some(window) = self.window {
            let focus = view.read(cx).path_field().focus_handle(cx);
            cx.defer(move |cx| {
                window.update(cx, |_, window, cx| window.focus(&focus, cx)).ok();
            });
        }
        cx.notify();
    }

    /// OK of the chooser: Root path takes the folder chosen; a mapping takes it relative to the
    /// root path, and nothing when it is outside it.
    fn chosen(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(target) = self.chooser_target() else { return };
        self.close_chooser(cx);
        match target {
            BrowseTarget::Root => self.update_current(cx, |server| server.root_path = path),
            BrowseTarget::Mapping(row) => {
                let root = self.current(cx).map(|server| remote_root(&server).to_owned()).unwrap_or_default();
                if let Some(relative) = folder_within(&root, &path).map(str::to_owned) {
                    let patch = MappingPatch { deployment: Some(relative), ..MappingPatch::default() };
                    self.mapping_selected = Some(row);
                    self.update_current(cx, |server| server.mappings = update_mapping(&server.mappings, row, &patch));
                }
            }
        }
        cx.notify();
    }

    /// Closes the chooser, emptying its password field first.
    pub fn close_chooser(&mut self, cx: &mut Context<Self>) {
        if let Some(chooser) = self.chooser.take() {
            chooser.view.update(cx, |browser, cx| browser.forget_secret(cx));
            cx.notify();
        }
    }

    /// Closes the chooser once the selected server no longer lists folders as it did when the
    /// chooser opened (another type, SSH configuration or FTP login, or no server), and a mapping's
    /// chooser once a mapping was added or removed.
    pub(super) fn drop_stale_chooser(&mut self, cx: &mut Context<Self>) {
        let Some(chooser) = self.chooser.as_ref() else { return };
        let stale = self.current(cx).is_none_or(|server| {
            let moved = matches!(chooser.target, BrowseTarget::Mapping(_)) && server.mappings.len() != chooser.mappings;
            moved || deployment_connection(&server) != *chooser.view.read(cx).connection()
        });
        if stale {
            self.close_chooser(cx);
        }
    }

    /// The chooser of `root` (Root path) or of a mapping, as text; nothing while it is closed.
    pub(super) fn chooser_lines(&self, root: bool, cx: &App) -> Vec<String> {
        let open = self.chooser.as_ref().filter(|chooser| (chooser.target == BrowseTarget::Root) == root);
        open.map(|chooser| chooser.view.read(cx).lines(cx)).unwrap_or_default()
    }

    /// The chooser of `root` (Root path) or of a mapping, drawn under what it fills.
    pub(super) fn chooser_panel(&self, root: bool) -> Option<AnyElement> {
        let open = self.chooser.as_ref().filter(|chooser| (chooser.target == BrowseTarget::Root) == root);
        open.map(|chooser| chooser.view.clone().into_any_element())
    }

    /// Root path's Browse…, drawn as `settings-deployment-root-browse`, faint while it cannot be
    /// used.
    pub(super) fn root_browse_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let enabled = self.can_browse_root(cx);
        let label = tx(cx, "Browse…");
        div()
            .id("settings-deployment-root-browse")
            .debug_selector(|| "settings-deployment-root-browse".to_owned())
            .child(button(cx, &label, false, enabled))
            .when(enabled, |el| el.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.browse_root(cx))))
            .into_any_element()
    }
}

/// A mapping's Browse… of its deployment path, drawn as `settings-deployment-mapping-<row>-remote`;
/// while it cannot be used its tooltip says why.
pub(super) fn mapping_browse_button(row: usize, enabled: bool, cx: &mut Context<DeploymentServersView>) -> AnyElement {
    let selector = format!("settings-deployment-mapping-{row}-remote");
    let title = if enabled { "Browse…" } else { "Set an absolute root path on the Connection tab first" };
    let browse = cx.listener(move |view, _: &ClickEvent, _, cx| view.browse_mapping(row, cx));
    tool(cx, &selector, "folder-open", enabled, title).when(enabled, |el| el.on_click(browse)).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a root path is absolute from `/` or from a drive letter, and its trailing
    // slashes do not count.
    #[test]
    fn a_mapping_browses_only_under_an_absolute_root_path() {
        assert!(is_absolute("/srv/www") && is_absolute("C:\\www") && is_absolute("d:/www"));
        assert!(!is_absolute("www") && !is_absolute("~/www") && !is_absolute("") && !is_absolute("C:"));
        let server = |root: &str| DeploymentServer { root_path: root.to_owned(), ..DeploymentServer::default() };
        assert_eq!(remote_root(&server(" /srv/www// ")), "/srv/www");
        assert_eq!(remote_root(&server("/")), "", "the server's top is no root to be relative to");
    }
}
