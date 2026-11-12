//! Recent remote projects, most recent first, and the one-time trust migration of the projects
//! remembered before the trust prompt existed. Both live in [`UiStorage`] as the JSON arrays the
//! other app reads under the same keys.

use crate::platform::storage::UiStorage;
use crate::platform_more::remote::{RemoteTarget, same_target};
use serde_json::{Value, json};

const RECENT_KEY: &str = "openidea.remote.recent";
const RECENT_MAX: usize = 20;
const TRUST_MIGRATION_KEY: &str = "openidea.remote.trustMigration";

/// A remote project opened before.
#[derive(Debug, Clone, PartialEq)]
pub struct RecentRemote {
    pub target: RemoteTarget,
    pub path: String,
    pub name: String,
    /// `user@host:port` when it was opened.
    pub host: String,
    pub opened_at_ms: f64,
}

/// A target as stored: `{ kind: "ssh", config }`, `{ kind: "wsl", distro }` or
/// `{ kind: "devContainer", folder, config }`.
pub fn target_to_json(target: &RemoteTarget) -> Value {
    match target {
        RemoteTarget::Ssh { config } => json!({ "kind": "ssh", "config": config }),
        RemoteTarget::Wsl { distro } => json!({ "kind": "wsl", "distro": distro }),
        RemoteTarget::DevContainer { folder, config } => {
            json!({ "kind": "devContainer", "folder": folder, "config": config })
        }
    }
}

/// The target a stored value names; `None` when it names none.
pub fn target_from_json(value: &Value) -> Option<RemoteTarget> {
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_owned);
    match value.get("kind")?.as_str()? {
        "ssh" => Some(RemoteTarget::Ssh { config: text("config")? }),
        "wsl" => Some(RemoteTarget::Wsl { distro: text("distro")? }),
        "devContainer" => {
            Some(RemoteTarget::DevContainer { folder: text("folder")?, config: text("config").unwrap_or_default() })
        }
        _ => None,
    }
}

fn to_json(recent: &RecentRemote) -> Value {
    json!({
        "target": target_to_json(&recent.target),
        "path": recent.path,
        "name": recent.name,
        "host": recent.host,
        "openedAtMs": recent.opened_at_ms,
    })
}

/// An entry as stored. Entries saved before WSL support carry `sshConfig` instead of `target`;
/// one without a path or a target is dropped.
fn normalize(entry: &Value) -> Option<RecentRemote> {
    let path = entry.get("path").and_then(Value::as_str).filter(|path| !path.is_empty())?;
    let target = entry.get("target").and_then(target_from_json).or_else(|| {
        let config = entry.get("sshConfig").and_then(Value::as_str).filter(|config| !config.is_empty())?;
        Some(RemoteTarget::Ssh { config: config.to_owned() })
    })?;
    let text = |name: &str| entry.get(name).and_then(Value::as_str).unwrap_or_default().to_owned();
    Some(RecentRemote {
        target,
        path: path.to_owned(),
        name: text("name"),
        host: text("host"),
        opened_at_ms: entry.get("openedAtMs").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

/// The JSON array stored under `key`; `None` when there is none or it is not an array.
fn stored_array(storage: &UiStorage, key: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(storage.get(key)?) {
        Ok(Value::Array(items)) => Some(items),
        _ => None,
    }
}

fn same_remote(a: &RecentRemote, target: &RemoteTarget, path: &str) -> bool {
    same_target(&a.target, target) && a.path == path
}

/// The remembered remote projects, most recent first.
pub fn recent_remotes(storage: &UiStorage) -> Vec<RecentRemote> {
    stored_array(storage, RECENT_KEY).unwrap_or_default().iter().filter_map(normalize).collect()
}

fn save_remotes(storage: &mut UiStorage, list: &[RecentRemote]) {
    let stored = Value::Array(list.iter().take(RECENT_MAX).map(to_json).collect());
    storage.set(RECENT_KEY, stored.to_string());
}

/// Puts `entry` first, replacing an earlier entry of the same target and path.
pub fn remember_remote(storage: &mut UiStorage, entry: RecentRemote) {
    let rest: Vec<RecentRemote> =
        recent_remotes(storage).into_iter().filter(|recent| !same_remote(recent, &entry.target, &entry.path)).collect();
    let list: Vec<RecentRemote> = std::iter::once(entry).chain(rest).collect();
    save_remotes(storage, &list);
}

/// Drops the entry of `path` on `target`.
pub fn forget_remote(storage: &mut UiStorage, target: &RemoteTarget, path: &str) {
    let list: Vec<RecentRemote> =
        recent_remotes(storage).into_iter().filter(|recent| !same_remote(recent, target, path)).collect();
    save_remotes(storage, &list);
}

/// Snapshots the recents the user had before any new connection is remembered, once: a later
/// call leaves the first snapshot as it is.
pub fn initialize_remote_trust_migration(storage: &mut UiStorage) {
    if storage.get(TRUST_MIGRATION_KEY).is_none() {
        let snapshot = Value::Array(recent_remotes(storage).iter().map(to_json).collect());
        storage.set(TRUST_MIGRATION_KEY, snapshot.to_string());
    }
}

/// Whether `path` on SSH `target`, opened as `host`, was remembered before the trust prompt
/// existed. A match is taken out of the snapshot, so it grants trust once. A WSL distribution or
/// a dev container never was remembered that way.
pub fn take_legacy_remote_trust(storage: &mut UiStorage, target: &RemoteTarget, path: &str, host: &str) -> bool {
    if !matches!(target, RemoteTarget::Ssh { .. }) {
        return false;
    }
    // No snapshot yet reads as an empty one, which is written so that a later
    // `initialize_remote_trust_migration` takes no snapshot of projects remembered since.
    let snapshot = match storage.get(TRUST_MIGRATION_KEY) {
        None => Vec::new(),
        Some(_) => match stored_array(storage, TRUST_MIGRATION_KEY) {
            Some(snapshot) => snapshot,
            None => return false,
        },
    };
    let matches =
        |stored: &Value| normalize(stored).is_some_and(|entry| same_remote(&entry, target, path) && entry.host == host);
    let found = snapshot.iter().any(matches);
    let kept: Vec<Value> = snapshot.into_iter().filter(|stored| !matches(stored)).collect();
    storage.set(TRUST_MIGRATION_KEY, Value::Array(kept).to_string());
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform_more::remote::{parse_remote_location, remote_location};

    fn ssh(config: &str) -> RemoteTarget {
        RemoteTarget::Ssh { config: config.to_owned() }
    }

    fn wsl(distro: &str) -> RemoteTarget {
        RemoteTarget::Wsl { distro: distro.to_owned() }
    }

    fn container(folder: &str) -> RemoteTarget {
        RemoteTarget::DevContainer { folder: folder.to_owned(), config: String::new() }
    }

    fn recent(target: RemoteTarget, path: &str, host: &str, opened_at_ms: f64) -> RecentRemote {
        let name = path.rsplit('/').next().unwrap_or_default().to_owned();
        RecentRemote { target, path: path.to_owned(), name, host: host.to_owned(), opened_at_ms }
    }

    fn paths(storage: &UiStorage) -> Vec<String> {
        recent_remotes(storage).into_iter().map(|recent| recent.path).collect()
    }

    fn targets(storage: &UiStorage) -> Vec<RemoteTarget> {
        recent_remotes(storage).into_iter().map(|recent| recent.target).collect()
    }

    // remote project locations › round-trip a WSL distribution and the path
    #[test]
    fn round_trip_a_wsl_distribution_and_the_path() {
        let location = remote_location(&wsl("Ubuntu-24.04"), "/home/me/app");
        assert_eq!(location, "wsl://Ubuntu-24.04/home/me/app");
        assert_eq!(parse_remote_location(&location), Some((wsl("Ubuntu-24.04"), "/home/me/app".to_owned())));
        assert_eq!(parse_remote_location("wsl://Ubuntu"), None);
    }

    // remote project locations › round-trip a dev container's local folder and the path in the container
    #[test]
    fn round_trip_a_dev_containers_local_folder_and_the_path_in_the_container() {
        let location = remote_location(&container("/Users/me/my app"), "/workspaces/my app");
        assert_eq!(location, "devcontainer://%2FUsers%2Fme%2Fmy%20app/workspaces/my app");
        let parsed = parse_remote_location(&location);
        assert_eq!(parsed, Some((container("/Users/me/my app"), "/workspaces/my app".to_owned())));
    }

    // remote project locations › round-trips container configurations and Windows folders with reserved characters
    #[test]
    fn round_trips_container_configurations_and_windows_folders_with_reserved_characters() {
        let target = RemoteTarget::DevContainer {
            folder: "C:\\work\\my app".to_owned(),
            config: ".devcontainer/a:b?c/devcontainer.json".to_owned(),
        };
        let location = remote_location(&target, "/workspaces/app");
        assert_eq!(parse_remote_location(&location), Some((target, "/workspaces/app".to_owned())));
    }

    // remote project locations › remembers recent remote projects most recent first, once each
    #[test]
    fn remembers_recent_remote_projects_most_recent_first_once_each() {
        let mut storage = UiStorage::default();
        remember_remote(&mut storage, recent(ssh("a"), "/p", "me@a", 1.0));
        remember_remote(&mut storage, recent(ssh("b"), "/q", "me@b", 2.0));
        remember_remote(&mut storage, recent(ssh("a"), "/p", "me@a", 3.0));
        assert_eq!(paths(&storage), ["/p", "/q"]);
        forget_remote(&mut storage, &ssh("a"), "/p");
        assert_eq!(paths(&storage), ["/q"]);
    }

    // remote project locations › keeps an SSH host and a WSL distribution of the same name apart
    #[test]
    fn keeps_an_ssh_host_and_a_wsl_distribution_of_the_same_name_apart() {
        let mut storage = UiStorage::default();
        remember_remote(&mut storage, recent(ssh("dev"), "/p", "me@dev", 1.0));
        remember_remote(&mut storage, recent(wsl("dev"), "/p", "dev", 2.0));
        assert_eq!(recent_remotes(&storage).len(), 2);
        forget_remote(&mut storage, &wsl("dev"), "/p");
        assert_eq!(targets(&storage), [ssh("dev")]);
    }

    // remote project locations › keeps the dev containers of two configurations of a folder apart
    #[test]
    fn keeps_the_dev_containers_of_two_configurations_of_a_folder_apart() {
        let mut storage = UiStorage::default();
        let python = RemoteTarget::DevContainer {
            folder: "/w/app".to_owned(),
            config: ".devcontainer/python/devcontainer.json".to_owned(),
        };
        remember_remote(&mut storage, recent(container("/w/app"), "/workspaces/app", "app", 1.0));
        remember_remote(&mut storage, recent(python.clone(), "/workspaces/app", "app", 2.0));
        assert_eq!(recent_remotes(&storage).len(), 2);
        let saved = remote_location(&python, "/workspaces/app");
        assert_ne!(saved, remote_location(&container("/w/app"), "/workspaces/app"));
        let (target, path) = parse_remote_location(&saved).expect("a remote location");
        assert_eq!((&target, path.as_str()), (&python, "/workspaces/app"));
        forget_remote(&mut storage, &target, "/workspaces/app");
        assert_eq!(targets(&storage), [container("/w/app")]);
    }

    // remote project locations › reads recents saved before WSL support, which carried `sshConfig`
    #[test]
    fn reads_recents_saved_before_wsl_support_which_carried_ssh_config() {
        let mut storage = UiStorage::default();
        let stored = json!([
            { "sshConfig": "old", "path": "/p", "name": "p", "host": "me@old", "openedAtMs": 1 },
            { "path": "/no-target" },
        ]);
        storage.set(RECENT_KEY, stored.to_string());
        assert_eq!(targets(&storage), [ssh("old")]);
    }

    // migrates only the original remote recents once, scoped to the saved host
    #[test]
    fn migrates_only_the_original_remote_recents_once_scoped_to_the_saved_host() {
        let mut storage = UiStorage::default();
        remember_remote(&mut storage, recent(ssh("a"), "/old", "me@old", 1.0));
        initialize_remote_trust_migration(&mut storage);
        remember_remote(&mut storage, recent(ssh("a"), "/new", "me@old", 2.0));
        initialize_remote_trust_migration(&mut storage);
        assert!(!take_legacy_remote_trust(&mut storage, &ssh("a"), "/new", "me@old"));
        assert!(!take_legacy_remote_trust(&mut storage, &ssh("a"), "/old", "me@other"));
        assert!(take_legacy_remote_trust(&mut storage, &ssh("a"), "/old", "me@old"));
        assert!(!take_legacy_remote_trust(&mut storage, &ssh("a"), "/old", "me@old"));
    }

    // migrates a snapshot taken before WSL support, and never trusts a WSL distribution
    #[test]
    fn migrates_a_snapshot_taken_before_wsl_support_and_never_trusts_a_wsl_distribution() {
        let mut storage = UiStorage::default();
        let snapshot = json!([{ "sshConfig": "a", "path": "/old", "host": "me@old", "name": "old", "openedAtMs": 1 }]);
        storage.set(TRUST_MIGRATION_KEY, snapshot.to_string());
        assert!(!take_legacy_remote_trust(&mut storage, &wsl("a"), "/old", "me@old"));
        assert!(take_legacy_remote_trust(&mut storage, &ssh("a"), "/old", "me@old"));
    }

    // Not from a spec: asking before any snapshot was taken leaves an empty one, so projects
    // remembered afterwards are never trusted by the migration.
    #[test]
    fn a_check_before_the_snapshot_leaves_nothing_to_migrate() {
        let mut storage = UiStorage::default();
        assert!(!take_legacy_remote_trust(&mut storage, &ssh("a"), "/p", "me@a"));
        remember_remote(&mut storage, recent(ssh("a"), "/p", "me@a", 1.0));
        initialize_remote_trust_migration(&mut storage);
        assert!(!take_legacy_remote_trust(&mut storage, &ssh("a"), "/p", "me@a"));
    }
}
