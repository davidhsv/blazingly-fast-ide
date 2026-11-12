//! File › Remote Development…: picks an SSH configuration (or a WSL distribution) and a folder
//! there, then opens it as a remote project.
//!
//! The SSH configurations are this machine's (`settings_get_global`): the connection is made from
//! here, and a remote project's window has the remote host's settings, which don't list them.
//! Every backend call is a [`ConnectRequest`]; an answer for a request a newer one replaced is
//! dropped.

use super::container::RemoteOpen;
use super::model::{browsable_folders, listed_directory, remote_parent};
use crate::platform::i18n::I18n;
use crate::platform_more::remote::RemoteTarget;
use crate::platform_more::remote_settings::normalized_ssh;
use ide_core::IdeError;
use ide_core::error::ErrorKind;
use ide_core::settings::{SshConfiguration, SshSettings};
use ide_ssh::RemoteEntry;

/// The action "Configure…" runs, with [`SSH_SETTINGS_PAGE`] as its argument.
pub const SETTINGS_ACTION: &str = "file.settings";
/// Settings › Tools › SSH Configurations.
pub const SSH_SETTINGS_PAGE: &str = "tools.sshConfigurations";

/// How the dialog reaches the project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Transport {
    #[default]
    Ssh,
    Wsl,
}

/// What the caller carries out for the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectRequest {
    /// Read this machine's global settings (`settings_get_global`) and hand their `ssh` to
    /// [`ConnectDialog::ssh_configs_loaded`]; a failure leaves the list as it is.
    LoadSshConfigs,
    /// List the installed WSL distributions (`wsl_distros`) for
    /// [`ConnectDialog::distros_loaded`].
    WslDistros,
    /// List `dir` on SSH configuration `config` (`remote_bootstrap_list`) for
    /// [`ConnectDialog::listed`].
    List { ticket: u64, config: String, dir: String },
    /// Open the remote project in this or a new window, answered with whether it opened, for
    /// [`ConnectDialog::opened`].
    Open { ticket: u64, open: RemoteOpen },
    /// Run [`SETTINGS_ACTION`] with [`SSH_SETTINGS_PAGE`].
    OpenSettings,
}

/// An SSH configuration as the picker shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshOption {
    pub value: String,
    pub label: String,
    /// `user@host:port`.
    pub detail: String,
}

/// A row of the folder list: `..` (when the directory is not `/`) and the folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderRow {
    pub label: String,
    /// What clicking the row lists ([`ConnectDialog::list_folder`]).
    pub path: String,
}

/// A listed directory and its folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browse {
    pub dir: String,
    pub folders: Vec<RemoteEntry>,
}

/// The state of the connect dialog.
#[derive(Debug, Clone)]
pub struct ConnectDialog {
    configs: Vec<SshConfiguration>,
    picked: String,
    transport: Transport,
    /// Installed WSL distributions; none (or no WSL at all) hides the WSL choice.
    distros: Vec<String>,
    distro: String,
    path: String,
    busy: bool,
    error: Option<String>,
    browse: Option<Browse>,
    sequence: u64,
    /// The listing in flight: its ticket and the directory asked for.
    listing: Option<(u64, String)>,
    /// The ticket of the open in flight.
    opening: Option<u64>,
}

impl ConnectDialog {
    /// A dialog just opened, with what it reads first.
    pub fn open() -> (Self, Vec<ConnectRequest>) {
        let dialog = Self {
            configs: Vec::new(),
            picked: String::new(),
            transport: Transport::Ssh,
            distros: Vec::new(),
            distro: String::new(),
            path: "~".to_owned(),
            busy: false,
            error: None,
            browse: None,
            sequence: 0,
            listing: None,
            opening: None,
        };
        (dialog, vec![ConnectRequest::LoadSshConfigs, ConnectRequest::WslDistros])
    }

    /// This machine's SSH settings were read.
    pub fn ssh_configs_loaded(&mut self, ssh: &SshSettings) {
        self.configs = normalized_ssh(ssh).configs;
    }

    /// The settings changed (`settingsChanged`): the SSH configurations are read again.
    pub fn settings_changed(&self) -> ConnectRequest {
        ConnectRequest::LoadSshConfigs
    }

    /// The installed WSL distributions; detection is a convenience, so a failure just leaves SSH
    /// as the only choice. The first one is picked unless one is already.
    pub fn distros_loaded(&mut self, result: Result<Vec<String>, IdeError>) {
        let Ok(found) = result else { return };
        if self.distro.is_empty() {
            self.distro = found.first().cloned().unwrap_or_default();
        }
        self.distros = found;
    }

    /// The selected SSH configuration: the one picked while it exists, else the first.
    pub fn ssh(&self) -> &str {
        if self.configs.iter().any(|config| config.id == self.picked) {
            return &self.picked;
        }
        self.configs.first().map_or("", |config| config.id.as_str())
    }

    pub fn ssh_options(&self) -> Vec<SshOption> {
        self.configs
            .iter()
            .map(|config| {
                let detail = config.label();
                let name = config.name.trim();
                SshOption {
                    value: config.id.clone(),
                    label: if name.is_empty() { detail.clone() } else { name.to_owned() },
                    detail,
                }
            })
            .collect()
    }

    pub fn select_ssh(&mut self, id: &str) {
        id.clone_into(&mut self.picked);
        self.browse = None;
    }

    /// Whether the dialog offers WSL next to SSH.
    pub fn shows_transport_choice(&self) -> bool {
        !self.distros.is_empty()
    }

    pub fn transport(&self) -> Transport {
        self.transport
    }

    pub fn select_transport(&mut self, transport: Transport) {
        self.transport = transport;
        self.browse = None;
    }

    pub fn distros(&self) -> &[String] {
        &self.distros
    }

    pub fn distro(&self) -> &str {
        &self.distro
    }

    pub fn select_distro(&mut self, distro: &str) {
        distro.clone_into(&mut self.distro);
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn set_path(&mut self, path: &str) {
        path.clone_into(&mut self.path);
    }

    pub fn busy(&self) -> bool {
        self.busy
    }

    /// Why the last listing or open failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The listed directory, once Browse answered.
    pub fn browsed(&self) -> Option<&Browse> {
        self.browse.as_ref()
    }

    /// The SSH configuration id or the WSL distribution the project is opened from.
    fn name(&self) -> &str {
        match self.transport {
            Transport::Ssh => self.ssh(),
            Transport::Wsl => self.distro.as_str(),
        }
    }

    pub fn target(&self) -> RemoteTarget {
        match self.transport {
            Transport::Ssh => RemoteTarget::Ssh { config: self.ssh().to_owned() },
            Transport::Wsl => RemoteTarget::Wsl { distro: self.distro.clone() },
        }
    }

    /// Whether Browse is there: browsing lists over SFTP, and a WSL distribution has no such
    /// session before connecting.
    pub fn browse_visible(&self) -> bool {
        self.transport == Transport::Ssh
    }

    pub fn can_browse(&self) -> bool {
        self.browse_visible() && !self.ssh().is_empty() && !self.busy
    }

    /// Whether Open is enabled.
    pub fn can_open(&self) -> bool {
        !self.name().is_empty() && !self.busy && !self.path.trim().is_empty()
    }

    /// The default button's label while a connection is (or is not) in progress.
    pub fn open_label(connecting: bool, i18n: &I18n) -> String {
        i18n.tx(if connecting { "Connecting…" } else { "Open" })
    }

    /// "Configure…": opens the SSH Configurations settings page.
    pub fn configure(&self) -> ConnectRequest {
        ConnectRequest::OpenSettings
    }

    /// Browse: lists the typed directory (`~` when the field is empty).
    pub fn browse(&mut self) -> Option<ConnectRequest> {
        if !self.can_browse() {
            return None;
        }
        let dir = Self::typed_or_home(&self.path);
        Some(self.list_folder(&dir))
    }

    /// Lists `dir` (a folder row was clicked).
    pub fn list_folder(&mut self, dir: &str) -> ConnectRequest {
        self.error = None;
        self.busy = true;
        self.sequence += 1;
        self.listing = Some((self.sequence, dir.to_owned()));
        ConnectRequest::List { ticket: self.sequence, config: self.ssh().to_owned(), dir: dir.to_owned() }
    }

    /// The rows of the folder list: `..` unless the directory is `/`, then the folders.
    pub fn folder_rows(&self) -> Vec<FolderRow> {
        let Some(browse) = &self.browse else { return Vec::new() };
        let parent =
            (browse.dir != "/").then(|| FolderRow { label: "..".to_owned(), path: remote_parent(&browse.dir) });
        let folders =
            browse.folders.iter().map(|folder| FolderRow { label: folder.name.clone(), path: folder.path.clone() });
        parent.into_iter().chain(folders).collect()
    }

    /// The answer to the listing of `ticket`: the folders, and the directory they are in becomes
    /// the project directory.
    pub fn listed(&mut self, ticket: u64, result: Result<Vec<RemoteEntry>, IdeError>) {
        let Some((_, requested)) = self.listing.take_if(|(pending, _)| *pending == ticket) else { return };
        self.busy = false;
        match result {
            Ok(entries) => {
                let dir = listed_directory(&requested, &entries);
                self.path.clone_from(&dir);
                self.browse = Some(Browse { dir, folders: browsable_folders(&entries) });
            }
            Err(error) => self.error = Some(error.message),
        }
    }

    /// Open: opens the project directory (`~` when the field is empty) on the selected target.
    pub fn connect(&mut self) -> Option<ConnectRequest> {
        if !self.can_open() {
            return None;
        }
        self.error = None;
        self.busy = true;
        self.sequence += 1;
        self.opening = Some(self.sequence);
        let open = RemoteOpen { target: self.target(), path: Self::typed_or_home(&self.path) };
        Some(ConnectRequest::Open { ticket: self.sequence, open })
    }

    /// The answer to the open of `ticket`: `Ok(true)` when the project opened (the dialog closes;
    /// this returns `true`), `Ok(false)` when it did not (the user declined something along the
    /// way). A cancel shows no error.
    pub fn opened(&mut self, ticket: u64, result: Result<bool, IdeError>) -> bool {
        if self.opening != Some(ticket) {
            return false;
        }
        self.opening = None;
        self.busy = false;
        match result {
            Ok(opened) => opened,
            Err(error) => {
                if error.kind != ErrorKind::Cancelled {
                    self.error = Some(error.message);
                }
                false
            }
        }
    }

    fn typed_or_home(path: &str) -> String {
        let trimmed = path.trim();
        if trimmed.is_empty() { "~".to_owned() } else { trimmed.to_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::tests::entry;
    use super::*;

    fn config(id: &str, name: &str, host: &str) -> SshConfiguration {
        SshConfiguration {
            id: id.to_owned(),
            name: name.to_owned(),
            host: host.to_owned(),
            user: "dev".to_owned(),
            ..Default::default()
        }
    }

    fn with_hosts() -> ConnectDialog {
        let (mut dialog, _) = ConnectDialog::open();
        let configs = vec![
            config("first", "First host", "first.example"),
            config("selected", "Selected host", "selected.example"),
        ];
        dialog.ssh_configs_loaded(&SshSettings { configs });
        dialog.distros_loaded(Ok(Vec::new()));
        dialog
    }

    fn list_request(request: Option<ConnectRequest>) -> (u64, String, String) {
        match request {
            Some(ConnectRequest::List { ticket, config, dir }) => (ticket, config, dir),
            other => panic!("expected a listing, got {other:?}"),
        }
    }

    fn row_labels(dialog: &ConnectDialog) -> Vec<String> {
        dialog.folder_rows().into_iter().map(|row| row.label).collect()
    }

    fn row(dialog: &ConnectDialog, label: &str) -> String {
        dialog.folder_rows().into_iter().find(|row| row.label == label).map(|row| row.path).expect("the row is listed")
    }

    // browses the selected SSH host, navigates folders and opens one without a current project
    #[test]
    fn browses_the_selected_ssh_host_navigates_folders_and_opens_one_without_a_current_project() {
        let mut dialog = with_hosts();
        assert!(dialog.can_browse());
        dialog.select_ssh("selected");
        let (ticket, config, dir) = list_request(dialog.browse());
        assert_eq!((config.as_str(), dir.as_str()), ("selected", "~"));
        assert!(!dialog.can_browse() && !dialog.can_open());
        let home = vec![
            entry("/home/dev/.cache", true),
            entry("/home/dev/projects", true),
            entry("/home/dev/README.md", false),
        ];
        dialog.listed(ticket, Ok(home));
        assert_eq!(row_labels(&dialog), ["..", "projects", ".cache"]);
        assert_eq!(dialog.path(), "/home/dev");

        let projects = row(&dialog, "projects");
        let (ticket, config, dir) = list_request(Some(dialog.list_folder(&projects)));
        assert_eq!((config.as_str(), dir.as_str()), ("selected", "/home/dev/projects"));
        dialog.listed(ticket, Ok(vec![entry("/home/dev/projects/demo", true)]));
        assert!(row_labels(&dialog).contains(&"demo".to_owned()));

        let parent = row(&dialog, "..");
        let (ticket, _, dir) = list_request(Some(dialog.list_folder(&parent)));
        assert_eq!(dir, "/home/dev");
        dialog.listed(ticket, Ok(vec![entry("/home/dev/projects", true)]));
        assert_eq!(dialog.path(), "/home/dev");

        let projects = row(&dialog, "projects");
        let (ticket, _, _) = list_request(Some(dialog.list_folder(&projects)));
        dialog.listed(ticket, Ok(Vec::new()));
        assert_eq!(dialog.path(), "/home/dev/projects");
        assert_eq!(dialog.folder_rows().len(), 1);

        let Some(ConnectRequest::Open { ticket, open }) = dialog.connect() else { panic!("an open request") };
        let target = RemoteTarget::Ssh { config: "selected".to_owned() };
        assert_eq!(open, RemoteOpen { target, path: "/home/dev/projects".to_owned() });
        assert!(dialog.opened(ticket, Ok(true)));
    }

    // shows a connection error and lets the welcome window retry Browse
    #[test]
    fn shows_a_connection_error_and_lets_the_welcome_window_retry_browse() {
        let mut dialog = with_hosts();
        dialog.set_path("  /srv/projects  ");
        let (ticket, config, dir) = list_request(dialog.browse());
        assert_eq!((config.as_str(), dir.as_str()), ("first", "/srv/projects"));
        dialog.listed(ticket, Err(IdeError::new(ErrorKind::Io, "Connection refused")));
        assert_eq!(dialog.error(), Some("Connection refused"));
        assert!(dialog.can_browse());
        let (ticket, _, _) = list_request(dialog.browse());
        dialog.listed(ticket, Ok(Vec::new()));
        assert!(dialog.browsed().is_some());
        assert_eq!(dialog.error(), None);
    }

    // offers SSH configuration and disables Browse until one exists, then lists the new one
    #[test]
    fn offers_ssh_configuration_and_disables_browse_until_one_exists_then_lists_the_new_one() {
        let (mut dialog, requests) = ConnectDialog::open();
        assert!(requests.contains(&ConnectRequest::LoadSshConfigs));
        dialog.ssh_configs_loaded(&SshSettings::default());
        assert!(!dialog.can_browse() && !dialog.can_open());
        assert_eq!(dialog.configure(), ConnectRequest::OpenSettings);
        assert_eq!(dialog.browse(), None);

        assert_eq!(dialog.settings_changed(), ConnectRequest::LoadSshConfigs);
        let robin = SshConfiguration { user: "robin".to_owned(), ..config("dev", "dev", "192.168.1.2") };
        dialog.ssh_configs_loaded(&SshSettings { configs: vec![robin] });
        assert!(dialog.can_browse());
        assert_eq!(dialog.ssh(), "dev");
        let labels: Vec<String> = dialog.ssh_options().into_iter().map(|option| option.label).collect();
        assert_eq!(labels, ["dev"]);
    }

    // hides the WSL choice when no distribution is installed
    #[test]
    fn hides_the_wsl_choice_when_no_distribution_is_installed() {
        let dialog = with_hosts();
        assert!(!dialog.shows_transport_choice());
    }

    // opens a folder of the chosen WSL distribution, without browsing over SFTP
    #[test]
    fn opens_a_folder_of_the_chosen_wsl_distribution_without_browsing_over_sftp() {
        let mut dialog = with_hosts();
        dialog.distros_loaded(Ok(vec!["Ubuntu-24.04".to_owned(), "Debian".to_owned()]));
        assert!(dialog.shows_transport_choice());
        dialog.select_transport(Transport::Wsl);
        assert!(!dialog.browse_visible());
        assert_eq!(dialog.browse(), None);
        dialog.select_distro("Debian");
        let Some(ConnectRequest::Open { open, .. }) = dialog.connect() else { panic!("an open request") };
        let target = RemoteTarget::Wsl { distro: "Debian".to_owned() };
        assert_eq!(open, RemoteOpen { target, path: "~".to_owned() });
    }
}
