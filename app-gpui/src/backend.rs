//! The backend, in process.
//!
//! The Tauri app reaches `ide_host::ops` through IPC commands; this app links `ide-host` and
//! calls the same functions with typed arguments. The backend runs on its own Tokio runtime;
//! [`Backend::call`] awaits its work from GPUI's executors, and the events it publishes
//! ([`IdeEvent`]) are delivered on the main thread, tagged with the window they are for.

use anyhow::Context as _;
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::{App, AsyncApp, Global, Task};
use ide_core::{IdeEvent, SharedSink};
use ide_host::{Host, HostConfig};
use ide_language::{LanguageRegistry, Syntax};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

/// Identifier of the app's directories. Its own, so the GPUI app never shares settings, caches
/// or single-instance state with an installed Tauri build running next to it.
pub const APP_IDENTIFIER: &str =
    if cfg!(debug_assertions) { "ai.l0ng.openidea.gpui.dev" } else { "ai.l0ng.openidea.gpui" };

/// Overrides every app directory with subfolders of one folder (tests, measurements).
pub const HOME_ENV: &str = "OPENIDEA_GPUI_HOME";

/// Where the app keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub config: PathBuf,
    /// Persistent user state, including credentials and recovery history.
    pub data: PathBuf,
    /// Machine-local tools and rebuildable caches.
    pub local_data: PathBuf,
}

impl AppDirs {
    /// The platform's directories for [`APP_IDENTIFIER`], or the folder [`HOME_ENV`] names.
    pub fn resolve() -> anyhow::Result<Self> {
        if let Some(home) = std::env::var_os(HOME_ENV).filter(|h| !h.is_empty()) {
            return Ok(Self::under(Path::new(&home)));
        }
        let dir = |base: Option<PathBuf>, what: &str| {
            base.map(|b| b.join(APP_IDENTIFIER)).with_context(|| format!("no {what} directory on this system"))
        };
        Ok(Self {
            config: dir(dirs::config_dir(), "configuration")?,
            data: dir(dirs::data_dir(), "data")?,
            local_data: dir(dirs::data_local_dir(), "local data")?,
        })
    }

    /// `config`, `data` and `local-data` inside `home`.
    pub fn under(home: &Path) -> Self {
        Self { config: home.join("config"), data: home.join("data"), local_data: home.join("local-data") }
    }
}

/// An event from the backend: for one window (by label), or for every window.
#[derive(Debug, Clone)]
pub struct BackendEvent {
    pub window: Option<String>,
    pub event: IdeEvent,
}

type Listener = dyn Fn(&BackendEvent, &mut App);

/// The process-wide backend: the [`Host`] plus what delivers its work and events to the UI.
pub struct Backend {
    pub host: Arc<Host>,
    pub dirs: AppDirs,
    runtime: tokio::runtime::Runtime,
    listeners: Vec<(u64, Rc<Listener>)>,
    next_listener: u64,
    /// Kept so the event pump lives as long as the backend.
    _pump: Task<()>,
}

impl Global for Backend {}

struct ChannelSink {
    window: Option<String>,
    events: UnboundedSender<BackendEvent>,
}

impl ide_core::EventSink for ChannelSink {
    fn emit(&self, event: IdeEvent) {
        // The receiver is gone only while the app shuts down.
        let _ = self.events.unbounded_send(BackendEvent { window: self.window.clone(), event });
    }
}

impl Backend {
    /// Starts the backend with the app's directories and installs it as a global.
    pub fn init(dirs: AppDirs, cx: &mut App) -> anyhow::Result<()> {
        let (backend, events) = Self::start(dirs, cx)?;
        cx.set_global(backend);
        Self::pump(events, cx);
        Ok(())
    }

    fn start(dirs: AppDirs, cx: &mut App) -> anyhow::Result<(Self, UnboundedReceiver<BackendEvent>)> {
        ide_workspace::process::mark_start();
        std::fs::create_dir_all(&dirs.config).with_context(|| format!("cannot create {}", dirs.config.display()))?;
        std::fs::create_dir_all(&dirs.data).with_context(|| format!("cannot create {}", dirs.data.display()))?;
        if let Err(error) = std::fs::create_dir_all(&dirs.local_data) {
            tracing::warn!(local_data_dir = ?dirs.local_data, %error, "cannot create local data directory");
        }
        tracing::info!(data_dir = ?dirs.data, local_data_dir = ?dirs.local_data, "application data directories");
        // Saved git HTTPS credentials (askpass answers them silently).
        ide_ssh::git_creds::init(dirs.data.join("git-credentials.json"));
        // File › Invalidate Caches… requested at the last run.
        let removed = ide_workspace::caches::apply_pending(&dirs.data, &dirs.local_data);
        if !removed.is_empty() {
            tracing::info!("invalidated caches: {removed:?}");
        }
        ide_host::data_dirs::migrate(&dirs.data, &dirs.local_data);

        // Grammar set-up is not needed for the first paint: build it off the main thread.
        let syntax: Arc<OnceLock<Arc<Syntax>>> = Arc::default();
        {
            let syntax = syntax.clone();
            std::thread::Builder::new().name("openidea-syntax-init".into()).spawn(move || {
                syntax.get_or_init(|| Arc::new(Syntax::new()));
            })?;
        }
        let runtime =
            tokio::runtime::Builder::new_multi_thread().enable_all().thread_name("openidea-backend").build()?;
        let languages = Arc::new(
            LanguageRegistry::load(Some(&dirs.config.join("languages"))).map_err(|e| anyhow::anyhow!(e.message))?,
        );
        let (sender, events) = unbounded();
        let host = Arc::new(Host::new(HostConfig {
            config_dir: dirs.config.clone(),
            data_dir: dirs.data.clone(),
            local_data_dir: dirs.local_data.clone(),
            languages,
            syntax,
            sink_for: Box::new(move |window| -> SharedSink {
                Arc::new(ChannelSink { window: Some(window.to_owned()), events: sender.clone() })
            }),
            owner: owner_window,
            runtime: runtime.handle().clone(),
            project_environment: false,
        }));
        let backend = Self { host, dirs, runtime, listeners: Vec::new(), next_listener: 0, _pump: Task::ready(()) };
        let _ = cx;
        Ok((backend, events))
    }

    /// Delivers backend events to the listeners, on the main thread, in order.
    fn pump(mut events: UnboundedReceiver<BackendEvent>, cx: &mut App) {
        let pump = cx.spawn(async move |cx: &mut AsyncApp| {
            while let Some(event) = events.next().await {
                cx.update(|cx| {
                    let listeners: Vec<Rc<Listener>> =
                        cx.global::<Backend>().listeners.iter().map(|(_, l)| l.clone()).collect();
                    for listener in listeners {
                        listener(&event, cx);
                    }
                });
            }
        });
        cx.global_mut::<Backend>()._pump = pump;
    }

    /// Calls `listener` for every backend event; returns its id for [`Backend::unlisten`].
    pub fn listen(&mut self, listener: impl Fn(&BackendEvent, &mut App) + 'static) -> u64 {
        self.next_listener += 1;
        self.listeners.push((self.next_listener, Rc::new(listener)));
        self.next_listener
    }

    pub fn unlisten(&mut self, id: u64) {
        self.listeners.retain(|(listener, _)| *listener != id);
    }

    /// Runs backend work on the backend's runtime; the returned future is awaited from GPUI's
    /// executors. Dropping it does not cancel the work (cancellable operations take a request id).
    pub fn call<T, W>(&self, work: W) -> impl Future<Output = anyhow::Result<T>> + Send + 'static + use<T, W>
    where
        T: Send + 'static,
        W: Future<Output = T> + Send + 'static,
    {
        let handle = self.runtime.spawn(work);
        async move { handle.await.context("the backend task did not complete") }
    }

    /// Runs backend work that takes the host.
    pub fn with_host<T, F, W>(&self, work: W) -> impl Future<Output = anyhow::Result<T>> + Send + 'static + use<T, F, W>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
        W: FnOnce(Arc<Host>) -> F + Send + 'static,
    {
        let host = self.host.clone();
        self.call(async move { work(host).await })
    }

    /// Blocks until `work` finishes on the backend's runtime (start-up and tests only).
    pub fn block_on<T>(&self, work: impl Future<Output = T>) -> T {
        self.runtime.block_on(work)
    }
}

/// The window that owns what `window` starts: a settings or detached-editor window's parent,
/// else `window`. Auxiliary windows are labelled `<parent>::<kind>`.
pub fn owner_window(window: &str) -> &str {
    window.split_once("::").map_or(window, |(parent, _)| parent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::TestAppContext;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../fixtures").join(name).canonicalize().expect("the fixture exists")
    }

    #[test]
    fn auxiliary_windows_belong_to_their_parent() {
        assert_eq!(owner_window("main"), "main");
        assert_eq!(owner_window("w3::settings"), "w3");
    }

    #[test]
    fn directories_follow_the_home_override() {
        let dirs = AppDirs::under(Path::new("/tmp/x"));
        assert_eq!(dirs.config, Path::new("/tmp/x/config"));
        assert_eq!(dirs.local_data, Path::new("/tmp/x/local-data"));
    }

    /// The real backend opens a fixture project and lists it through `ide_host::ops`, and its
    /// events arrive on the main thread tagged with the window.
    #[gpui_kit::test]
    async fn opens_a_project_and_lists_its_files(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| Backend::init(AppDirs::under(home.path()), cx)).expect("the backend starts");
        let seen: Rc<std::cell::RefCell<Vec<Option<String>>>> = Rc::default();
        let log = seen.clone();
        cx.update(|cx| cx.global_mut::<Backend>().listen(move |event, _| log.borrow_mut().push(event.window.clone())));

        let root = fixture("rust-app");
        let open = cx.update(|cx| {
            let root = root.clone();
            cx.global::<Backend>().with_host(move |host| async move {
                let project = ide_host::ops::project::open(&host, "w1", &root).await?;
                let info = ide_host::ops::project::info(&project);
                let entries = ide_host::ops::fs::read_dir(&host, "w1", root).await?;
                ide_core::IdeResult::Ok((info.name, entries))
            })
        });
        let (name, entries) = open.await.expect("the task completes").expect("the project opens");
        assert_eq!(name, "rust-app");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Cargo.toml") && names.contains(&"src"), "{names:?}");

        // Opening a project publishes events for its window.
        cx.run_until_parked();
        assert!(seen.borrow().iter().all(|w| w.as_deref() == Some("w1") || w.is_none()));
        let host = cx.update(|cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }
}
