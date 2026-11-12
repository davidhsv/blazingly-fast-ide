//! SSH configurations, deployment servers and run targets as features consume them: every
//! item has an id of its own. The shape and the defaults are the backend's
//! (`ide_core::settings`); the display label of an SSH configuration is
//! `SshConfiguration::label`.

use ide_core::settings::{DeploymentServer, DeploymentSettings, DeploymentType, RunTarget, RunTargetKind};
use ide_core::settings::{SshConfiguration, SshSettings};
use ide_ssh::FsConnection;
use std::collections::HashSet;

/// Something features refer to by id.
pub trait Identified {
    fn id(&self) -> &str;
    fn set_id(&mut self, id: String);
}

macro_rules! identified {
    ($($item:ty),*) => {$(
        impl Identified for $item {
            fn id(&self) -> &str {
                &self.id
            }

            fn set_id(&mut self, id: String) {
                self.id = id;
            }
        }
    )*};
}

identified!(SshConfiguration, DeploymentServer, RunTarget);

/// `items` with a unique, non-empty id each: a missing id becomes `<prefix><position>`, and an
/// id that an earlier item has gets `_` appended until it is free.
pub fn with_unique_ids<T: Identified + Clone>(items: &[T], prefix: &str) -> Vec<T> {
    let mut seen = HashSet::new();
    items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let trimmed = item.id().trim();
            let mut id = if trimmed.is_empty() { format!("{prefix}{}", index + 1) } else { trimmed.to_owned() };
            while seen.contains(&id) {
                id.push('_');
            }
            seen.insert(id.clone());
            let mut item = item.clone();
            item.set_id(id);
            item
        })
        .collect()
}

/// The SSH configurations with ids of their own; a port of 0 is no port (the default one).
pub fn normalized_ssh(ssh: &SshSettings) -> SshSettings {
    let configs = with_unique_ids(&ssh.configs, "ssh")
        .into_iter()
        .map(|config| SshConfiguration { port: config.port.filter(|port| *port > 0), ..config })
        .collect();
    SshSettings { configs }
}

/// The deployment settings with every server under an id of its own.
pub fn normalized_deployment(deployment: &DeploymentSettings) -> DeploymentSettings {
    DeploymentSettings { servers: with_unique_ids(&deployment.servers, "server"), ..deployment.clone() }
}

/// The run targets with ids of their own.
pub fn normalized_run_targets(targets: &[RunTarget]) -> Vec<RunTarget> {
    with_unique_ids(targets, "target")
}

/// Whether a target of this kind runs configurations in a container (Docker, Compose) rather
/// than over SSH.
pub fn in_container(kind: RunTargetKind) -> bool {
    matches!(kind, RunTargetKind::Docker | RunTargetKind::Compose)
}

/// How the backend reaches a deployment server's files, for deployment and folder browsing.
pub fn deployment_connection(server: &DeploymentServer) -> FsConnection {
    match server.kind {
        DeploymentType::Local => FsConnection::Local,
        DeploymentType::Sftp => FsConnection::Sftp { ssh: server.ssh_config.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn deployment(raw: Value) -> DeploymentSettings {
        let parsed: DeploymentSettings = serde_json::from_value(raw).expect("the backend takes the settings");
        normalized_deployment(&parsed)
    }

    fn sftp(ssh: &str) -> FsConnection {
        FsConnection::Sftp { ssh: ssh.to_owned() }
    }

    // deployment types › reads existing servers as SFTP without changing their connection
    #[test]
    fn reads_existing_servers_as_sftp_without_changing_their_connection() {
        let legacy = json!({
            "id": "existing",
            "name": "Staging",
            "sshConfig": "ssh",
            "rootPath": "/srv",
            "webUrl": "http://localhost",
        });
        let settings = deployment(json!({ "servers": [legacy] }));
        let server = &settings.servers[0];
        assert_eq!(server.kind, DeploymentType::Sftp);
        assert_eq!((server.id.as_str(), server.name.as_str()), ("existing", "Staging"));
        assert_eq!(server.ssh_config, "ssh");
        assert_eq!((server.root_path.as_str(), server.web_url.as_str()), ("/srv", "http://localhost"));
        assert_eq!(deployment_connection(server), sftp("ssh"));
    }

    // deployment types › round trips %s settings
    #[test]
    fn round_trips_settings() {
        for kind in ["sftp", "local"] {
            let raw = json!({ "servers": [{ "kind": kind, "rootPath": "/mounted/www", "sshConfig": "retained" }] });
            let settings = deployment(raw);
            assert_eq!(settings.servers[0].id, "server1");
            let stored = serde_json::to_value(&settings).expect("deployment settings are plain values");
            assert_eq!(deployment(stored), settings);
            let expected = if kind == "local" { FsConnection::Local } else { sftp("retained") };
            assert_eq!(deployment_connection(&settings.servers[0]), expected);
        }
    }

    // deployment types › normalizes unsupported types to SFTP
    #[test]
    fn normalizes_unsupported_types_to_sftp() {
        assert_eq!(deployment(json!({ "servers": [{}] })).servers[0].kind, DeploymentType::Sftp);
        // A type the backend does not know is refused where the settings are stored.
        for kind in [json!("ftp"), json!("ftps"), json!("webdav"), json!("inPlace"), Value::Null] {
            let raw = json!({ "servers": [{ "kind": kind }] });
            assert!(serde_json::from_value::<DeploymentSettings>(raw).is_err(), "{kind}");
        }
    }

    // Not from a spec: ids are made unique in order, SSH ports of 0 mean the default port, and
    // container targets are told from SSH ones.
    #[test]
    fn gives_every_item_an_id_of_its_own() {
        let config = |id: &str, port: Option<u16>| SshConfiguration { id: id.to_owned(), port, ..Default::default() };
        let ssh = SshSettings { configs: vec![config("a", Some(0)), config(" ", Some(22)), config("a", None)] };
        let normalized = normalized_ssh(&ssh);
        let ids: Vec<&str> = normalized.configs.iter().map(|config| config.id.as_str()).collect();
        assert_eq!(ids, ["a", "ssh2", "a_"]);
        assert_eq!(normalized.configs.iter().map(|config| config.port).collect::<Vec<_>>(), [None, Some(22), None]);
        let target = |id: &str, kind: RunTargetKind| RunTarget { id: id.to_owned(), kind, ..Default::default() };
        let targets = normalized_run_targets(&[
            target("", RunTargetKind::Ssh),
            target("target1", RunTargetKind::Docker),
            target("target1_", RunTargetKind::Compose),
        ]);
        let ids: Vec<&str> = targets.iter().map(|target| target.id.as_str()).collect();
        assert_eq!(ids, ["target1", "target1_", "target1__"]);
        assert_eq!(targets.iter().map(|target| in_container(target.kind)).collect::<Vec<_>>(), [false, true, true]);
    }
}
