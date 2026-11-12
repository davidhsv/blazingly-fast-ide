//! The remote folder browser's helpers, the stages of connecting and the status bar label of a
//! remote window.

use crate::platform::collate;
use crate::platform::i18n::{I18n, Param};
use crate::platform_more::remote::RemoteTarget;
use ide_ssh::RemoteEntry;

/// The parent of absolute POSIX path `path` (`/` stays `/`).
pub fn remote_parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(slash) if slash > 0 => trimmed[..slash].to_owned(),
        _ => "/".to_owned(),
    }
}

/// The folders of a listing, sorted by name (hidden ones last).
pub fn browsable_folders(entries: &[RemoteEntry]) -> Vec<RemoteEntry> {
    let mut folders: Vec<RemoteEntry> = entries.iter().filter(|entry| entry.is_dir).cloned().collect();
    folders.sort_by(|a, b| {
        let hidden = |entry: &RemoteEntry| entry.name.starts_with('.');
        hidden(a).cmp(&hidden(b)).then_with(|| collate::compare(&a.name, &b.name))
    });
    folders
}

/// The directory a listing of `requested` stands for (`~` resolves on the server: the parent of
/// the first entry).
pub fn listed_directory(requested: &str, entries: &[RemoteEntry]) -> String {
    if requested.starts_with('/') {
        let trimmed = requested.trim_end_matches('/');
        return if trimmed.is_empty() { "/".to_owned() } else { trimmed.to_owned() };
    }
    entries.first().map_or_else(|| requested.to_owned(), |first| remote_parent(&first.path))
}

/// Where connecting is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteStage {
    Connecting,
    /// `devcontainer up`: building or starting the container.
    Container,
    Checking,
    Downloading,
    Uploading,
    Starting,
    Opening,
}

/// What the window is waiting for at each stage of connecting (English; the view translates it
/// with [`I18n::tx`]).
pub fn stage_label(stage: RemoteStage) -> &'static str {
    match stage {
        RemoteStage::Connecting => "Connecting to the host…",
        RemoteStage::Container => "Starting the dev container…",
        RemoteStage::Checking => "Checking the IDE backend on the host…",
        RemoteStage::Downloading => "Downloading the IDE backend…",
        RemoteStage::Uploading => "Installing the IDE backend on the host…",
        RemoteStage::Starting => "Starting the IDE backend…",
        RemoteStage::Opening => "Opening the project…",
    }
}

/// How far a download or upload is, in whole percent; `None` when it is not measured.
pub fn progress_percent(fraction: Option<f64>) -> Option<u32> {
    fraction.map(|fraction| (fraction.clamp(0.0, 1.0) * 100.0).round() as u32)
}

/// Status bar label of a remote window: the transport and where its project runs.
pub fn remote_badge(target: &RemoteTarget, host: &str) -> String {
    let transport = match target {
        RemoteTarget::Ssh { .. } => "SSH",
        RemoteTarget::Wsl { .. } => "WSL",
        RemoteTarget::DevContainer { .. } => "Dev Container",
    };
    format!("{transport}: {host}")
}

/// The status bar item's tooltip for `label` (a [`remote_badge`]).
pub fn remote_status_tooltip(label: &str, i18n: &mut I18n) -> String {
    i18n.tx_built(&["Project runs on ", ""], &[Param::from(label)])
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;

    pub(crate) fn entry(path: &str, is_dir: bool) -> RemoteEntry {
        RemoteEntry {
            name: path[path.rfind('/').map_or(0, |slash| slash + 1)..].to_owned(),
            path: path.to_owned(),
            is_dir,
            is_symlink: false,
            size: 0,
            mtime: None,
            permissions: "rwxr-xr-x".to_owned(),
        }
    }

    // remote folder browser › goes up one level and stops at the root
    #[test]
    fn goes_up_one_level_and_stops_at_the_root() {
        assert_eq!(remote_parent("/home/dev/app"), "/home/dev");
        assert_eq!(remote_parent("/home/dev/"), "/home");
        assert_eq!(remote_parent("/home"), "/");
        assert_eq!(remote_parent("/"), "/");
    }

    // remote folder browser › lists folders only, hidden ones last
    #[test]
    fn lists_folders_only_hidden_ones_last() {
        let listing =
            [entry("/h/.cache", true), entry("/h/zeta", true), entry("/h/notes.txt", false), entry("/h/alpha", true)];
        let folders = browsable_folders(&listing);
        let names: Vec<&str> = folders.iter().map(|folder| folder.name.as_str()).collect();
        assert_eq!(names, ["alpha", "zeta", ".cache"]);
    }

    // remote folder browser › resolves `~` from the listing
    #[test]
    fn resolves_tilde_from_the_listing() {
        assert_eq!(listed_directory("~", &[entry("/home/dev/app", true)]), "/home/dev");
        assert_eq!(listed_directory("~", &[]), "~");
        assert_eq!(listed_directory("/srv/", &[]), "/srv");
        assert_eq!(listed_directory("/", &[]), "/");
    }

    // labels the status bar with the transport and where the project runs
    #[test]
    fn labels_the_status_bar_with_the_transport_and_where_the_project_runs() {
        let wsl = RemoteTarget::Wsl { distro: "Ubuntu".to_owned() };
        let ssh = RemoteTarget::Ssh { config: "a".to_owned() };
        let container = RemoteTarget::DevContainer { folder: "/w/app".to_owned(), config: String::new() };
        assert_eq!(remote_badge(&wsl, "Ubuntu"), "WSL: Ubuntu");
        assert_eq!(remote_badge(&ssh, "me@a:22"), "SSH: me@a:22");
        assert_eq!(remote_badge(&container, "app"), "Dev Container: app");
        let mut i18n = I18n::default();
        assert_eq!(remote_status_tooltip("SSH: me@a:22", &mut i18n), "Project runs on SSH: me@a:22");
    }
}
