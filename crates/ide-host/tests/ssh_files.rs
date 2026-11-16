//! A host built with SSH files of its own checks and records host keys there, for its SSH
//! connections and for the SFTP connections of its remote service alike.

use ide_core::NullSink;
use ide_core::remote_settings::{SshAuth, SshConfiguration};
use ide_host::{Host, HostConfig};
use ide_ssh::{FsConnection, SshFiles};
use std::path::Path;
use std::sync::Arc;

fn config(base: &Path) -> HostConfig {
    HostConfig {
        config_dir: base.join("config"),
        data_dir: base.join("data"),
        local_data_dir: base.join("data"),
        languages: Arc::new(ide_language::LanguageRegistry::load(None).unwrap()),
        syntax: Arc::default(),
        sink_for: Box::new(|_| Arc::new(NullSink)),
        owner: |w| w,
        runtime: tokio::runtime::Handle::current(),
        project_environment: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_with_ssh_files_of_its_own_trusts_and_checks_host_keys_there() {
    let tmp = tempfile::tempdir().unwrap();
    let server_root = tmp.path().join("server");
    std::fs::create_dir_all(server_root.join("projects")).unwrap();
    let port = ide_ssh::testing::sshd(server_root, Arc::new(|cmd| panic!("unexpected SSH exec: {cmd}"))).await;
    let known_hosts = tmp.path().join("home/ssh/known_hosts");
    let host = Arc::new(Host::with_ssh_files(
        config(&tmp.path().join("app")),
        SshFiles { known_hosts: known_hosts.clone(), config: None },
    ));
    assert_eq!(host.ssh.files().known_hosts, known_hosts);
    assert_eq!(host.ssh.files().config, None);
    host.ssh.set_configs(vec![SshConfiguration {
        id: "box".into(),
        host: "127.0.0.1".into(),
        port: Some(port),
        user: "dev".into(),
        auth: SshAuth::Password,
        ..Default::default()
    }]);
    host.ssh.set_secret("box", Some("secret".into()), false);

    // The remote service shares the host's SSH manager: its unknown host key is the one Trust records.
    let connection = FsConnection::Sftp { ssh: "box".into() };
    assert!(host.remote.list(&connection, "/projects").await.is_err(), "an unknown host key is refused first");
    assert!(!known_hosts.exists());
    host.ssh.trust_host("box").await.unwrap();
    let recorded = std::fs::read_to_string(&known_hosts).unwrap();
    assert!(
        recorded.contains(&format!("[127.0.0.1]:{port}")),
        "the key is recorded in the host's own file: {recorded}"
    );
    assert!(host.remote.list(&connection, "/projects").await.is_ok(), "the trusted key is checked in that file");
    host.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_built_without_ssh_files_keeps_the_default_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let host = Host::new(config(tmp.path()));
    let default = SshFiles::default();
    assert_eq!(host.ssh.files().known_hosts, default.known_hosts);
    assert_eq!(host.ssh.files().config, default.config);
    host.shutdown().await;
}
