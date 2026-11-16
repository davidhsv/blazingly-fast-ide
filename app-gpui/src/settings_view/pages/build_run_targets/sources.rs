//! What the Run Targets page reads from the backend and the machine, behind seams a test can
//! replace: the folder browser's listing, host key trust and password ([`RemoteFolders`]), the
//! Docker connections ([`DockerSource`]), and, read once when the page is built, the project's name
//! and the targets plugins registered for it ([`load_sources`]).

use super::RunTargetsView;
use crate::backend::Backend;
use crate::database::store::SecretText;
use crate::settings_view::pages::Then;
use gpui_kit::{App, Entity};
use ide_core::IdeError;
use ide_ssh::RemoteEntry;
use std::rc::Rc;
use std::sync::Arc;

/// Lists the folder `path` of the server an SSH configuration reaches (`~` for the login folder).
pub type FolderLister = Rc<dyn Fn(String, String, Then<Result<Vec<RemoteEntry>, IdeError>>, &mut App)>;
/// Trusts the host key that failed the last connection of an SSH configuration.
pub type HostTrust = Rc<dyn Fn(String, Then<Result<(), IdeError>>, &mut App)>;
/// Hands an SSH configuration the password or passphrase typed, for this session only.
pub type SecretSetter = Rc<dyn Fn(String, SecretText, Then<Result<(), IdeError>>, &mut App)>;

/// What the folder browser asks of the backend.
#[derive(Clone)]
pub struct RemoteFolders {
    pub list: FolderLister,
    pub trust_host: HostTrust,
    pub set_secret: SecretSetter,
}

/// The folder browser's backend, set by a test as a global; without one the backend's.
pub struct RunTargetFolders(pub RemoteFolders);

impl gpui_kit::Global for RunTargetFolders {}

/// The folder browser's backend set as [`RunTargetFolders`], else the backend's. A test build gets
/// none of its own making (Browse… is then unavailable): listing a folder opens an SSH connection,
/// which reads the user's `~/.ssh` files and reaches the network.
pub fn folders(cx: &App) -> Option<RemoteFolders> {
    match cx.try_global::<RunTargetFolders>() {
        Some(set) => Some(set.0.clone()),
        None if cfg!(test) => None,
        None => Some(backend_folders()),
    }
}

/// A Docker connection offered: (id, name).
pub type DockerChoice = (String, String);
/// Reads the Services tool window's Docker connections.
pub type DockerSource = Rc<dyn Fn(Then<Result<Vec<DockerChoice>, String>>, &mut App)>;

/// The Docker connections' source, set by a test as a global; without one the backend's.
pub struct RunTargetDocker(pub DockerSource);

impl gpui_kit::Global for RunTargetDocker {}

/// The source set as [`RunTargetDocker`], else the backend's; a test build reads none unless a
/// test sets one.
pub fn docker_source(cx: &App) -> Option<DockerSource> {
    match cx.try_global::<RunTargetDocker>() {
        Some(set) => Some(set.0.clone()),
        None if cfg!(test) => None,
        None => Some(backend_docker()),
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

/// The backend's folder listing (`remote_list` over SFTP), host key trust (`ssh_trust_host`) and
/// password (`ssh_set_secret` without saving it), each called as their commands call them.
pub fn backend_folders() -> RemoteFolders {
    type Listed = Then<Result<Vec<RemoteEntry>, IdeError>>;
    let list: FolderLister = Rc::new(|ssh: String, path: String, then: Listed, cx: &mut App| {
        let connection = ide_ssh::FsConnection::Sftp { ssh };
        on_backend(
            move |host| async move { ide_host::ops::deploy::remote_list(&host, connection, path).await },
            then,
            cx,
        );
    });
    let trust_host: HostTrust = Rc::new(|id: String, then: Then<Result<(), IdeError>>, cx: &mut App| {
        on_backend(move |host| async move { host.ssh.trust_host(&id).await }, then, cx);
    });
    let set_secret: SecretSetter =
        Rc::new(|id: String, secret: SecretText, then: Then<Result<(), IdeError>>, cx: &mut App| {
            on_backend(
                move |host| async move {
                    host.ssh.set_secret(&id, Some(secret.into_inner()), false);
                    Ok(())
                },
                then,
                cx,
            );
        });
    RemoteFolders { list, trust_host, set_secret }
}

/// The backend's Docker connections (`docker_settings`): the app's `docker.json`, read off the
/// window's thread.
pub fn backend_docker() -> DockerSource {
    Rc::new(|then: Then<Result<Vec<DockerChoice>, String>>, cx: &mut App| {
        let Some(backend) = cx.try_global::<Backend>() else {
            then(Err("The backend is not running".to_owned()), cx);
            return;
        };
        let data = backend.dirs.data.clone();
        let read = move || ide_docker::load_settings(&data);
        let work = backend.call(async move { tokio::task::spawn_blocking(read).await });
        cx.spawn(async move |cx| {
            let result = match work.await {
                Ok(Ok(settings)) => {
                    Ok(settings.connections.into_iter().map(|connection| (connection.id, connection.name)).collect())
                }
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            cx.update(|cx| then(result, cx));
        })
        .detach();
    })
}

/// A target a plugin registered for the project: listed, offered as the default, never edited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginTarget {
    pub id: String,
    pub name: String,
    /// The name of the plugin that registered it.
    pub plugin: String,
}

/// Reads what the page lists from the machine and the backend, off the window's thread, and hands
/// it to `view`: the project's name (for the folder placeholders), the plugins' targets for the
/// project of `window`, and the Docker connections.
pub fn load_sources(view: &Entity<RunTargetsView>, window: &str, cx: &mut App) {
    let host = cx.try_global::<Backend>().map(|backend| backend.host.clone());
    let plugins = cx.try_global::<crate::plugins::Plugins>().map(|plugins| plugins.host.clone());
    if let Some(host) = host {
        let (weak, label) = (view.downgrade(), window.to_owned());
        cx.spawn(async move |cx| {
            let found = cx
                .background_executor()
                .spawn(async move {
                    let project = host.project(&label).ok()?;
                    let name = ide_host::ops::project::info(&project).name;
                    let targets = plugins.map(|plugins| plugins.run_targets(&project.root)).unwrap_or_default();
                    Some((name, targets))
                })
                .await;
            let Some((name, targets)) = found else { return };
            let targets: Vec<PluginTarget> = targets
                .into_iter()
                .map(|target| PluginTarget { id: target.id, name: target.target.name, plugin: target.plugin_name })
                .collect();
            weak.update(cx, |view, cx| {
                view.set_project_name(name, cx);
                view.set_plugin_targets(targets, cx);
            })
            .ok();
        })
        .detach();
    }
    if let Some(read) = docker_source(cx) {
        let weak = view.downgrade();
        let then: Then<Result<Vec<DockerChoice>, String>> =
            Rc::new(move |result: Result<Vec<DockerChoice>, String>, cx: &mut App| match result {
                Ok(connections) => {
                    weak.update(cx, |view, cx| view.set_docker_connections(connections, cx)).ok();
                }
                // As in the Services tool window: without its settings no connection is offered.
                Err(error) => tracing::debug!("Settings: the Docker connections were not read: {error}"),
            });
        read(then, cx);
    }
}
