//! The plugin host in this app: the backend's [`PluginHost`] wired to the open projects, its
//! events brought to the main thread, and its contributions projected into the UI.
//!
//! Plugins are loaded from `<data>/plugins` (installed ones, and folders linked for
//! development). Loading instantiates no Wasm: a plugin's code runs on its activation events.

use crate::backend::Backend;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::theme::appearance::{self, PluginTheme};
use crate::theme::themes::{ThemeChoice, ThemeChoiceId, ThemeKind, parse_plugin_theme_ref, plugin_theme_ref};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use crate::workbench::{self, Workbench};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::{App, AsyncApp, Entity, Global, Task, Window};
use ide_host::plugins::{
    ActiveTheme, HostWorkspace, PluginEditor, PluginEvent, PluginHost, PluginStatus, PluginsSnapshot, ThemeDef,
};
use ide_plugin::{Invocation, Limits};
use serde::Deserialize;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long the first listing waits for the installed plugins to load before answering with
/// what there is.
const INITIAL_LOAD_WAIT: Duration = Duration::from_secs(5);
/// `on-startup-finished` fires this long after the installed plugins are loaded.
const STARTUP_FINISHED_DELAY: Duration = Duration::from_millis(1500);
/// How often the marketplace index is read again (revocations).
const MARKET_REFRESH: Duration = Duration::from_secs(24 * 60 * 60);
/// How often queued plugin events are flushed; supervision runs every
/// [`TICKS_PER_SUPERVISION`] flushes.
const EVENT_FLUSH: Duration = Duration::from_millis(250);
const TICKS_PER_SUPERVISION: u32 = 20;

/// The extension point plugin themes are contributed to.
const THEMES_POINT: &str = "themes";

/// The extension point plugin commands are contributed to.
const COMMANDS_POINT: &str = "commands";

/// A declared command, as the `commands` point carries it.
#[derive(Debug, Clone, Deserialize)]
struct CommandDecl {
    id: String,
    title: String,
    #[serde(default)]
    icon: Option<String>,
    #[serde(default)]
    category: Option<String>,
    /// Enablement, evaluated against the window's context keys.
    #[serde(default)]
    when: Option<String>,
}

/// The id of the action that runs command `command` of plugin `plugin`.
pub fn plugin_action_id(plugin: &str, command: &str) -> String {
    format!("plugin.{plugin}.{command}")
}

/// What an action's argument means to a plugin command: its string argument and the selected
/// paths. A string is the argument itself; `{ path, paths }` (a menu on files) passes the path
/// and the selection; with `line` and `column` (an editor menu) the argument is that position
/// as JSON.
pub fn command_args(arg: Option<&serde_json::Value>) -> (Option<String>, Vec<PathBuf>) {
    let Some(arg) = arg else { return (None, Vec::new()) };
    if let Some(text) = arg.as_str() {
        return (Some(text.to_owned()), Vec::new());
    }
    let (Some(path), Some(paths)) =
        (arg.get("path").and_then(|p| p.as_str()), arg.get("paths").and_then(|p| p.as_array()))
    else {
        return (None, Vec::new());
    };
    let paths = paths.iter().filter_map(|path| path.as_str().map(PathBuf::from)).collect();
    match (arg.get("line").filter(|v| v.is_number()), arg.get("column").filter(|v| v.is_number())) {
        (Some(line), Some(column)) => {
            (Some(serde_json::json!({ "path": path, "line": line, "column": column }).to_string()), paths)
        }
        _ => (Some(path.to_owned()), paths),
    }
}

/// A plugin event for one window (by label), or for every window.
type Delivery = (Option<String>, PluginEvent);
type Listener = dyn Fn(Option<&str>, &PluginEvent, &mut App);

/// The process-wide plugin host and what the UI last learned from it.
pub struct Plugins {
    pub host: Arc<PluginHost>,
    /// The plugins and their UI contributions as last listed; none before the first listing.
    snapshot: Option<PluginsSnapshot>,
    listeners: Vec<(u64, Rc<Listener>)>,
    next_listener: u64,
    /// A listing in progress; a newer one replaces it.
    listing: Option<Task<()>>,
    /// Ids of the actions registered for plugin commands.
    command_actions: Vec<String>,
    listings: u64,
    _pump: Task<()>,
}

impl Global for Plugins {}

/// Plugin secrets in the login keychain, one service per plugin under this app's identifier. Test
/// builds and homes of their own never use it: their hosts keep the default file store there.
#[cfg(all(target_os = "macos", not(test)))]
struct KeychainSecrets;

#[cfg(all(target_os = "macos", not(test)))]
impl KeychainSecrets {
    fn service(plugin: &str) -> String {
        format!("{}.plugin.{plugin}", crate::backend::APP_IDENTIFIER)
    }
}

#[cfg(all(target_os = "macos", not(test)))]
impl ide_host::plugins::SecretStore for KeychainSecrets {
    fn get(&self, plugin: &str, name: &str) -> Option<String> {
        let bytes = security_framework::passwords::get_generic_password(&Self::service(plugin), name).ok()?;
        String::from_utf8(bytes).ok()
    }

    fn set(&self, plugin: &str, name: &str, value: Option<&str>) -> Result<(), String> {
        /// `errSecItemNotFound`: deleting what is not there is not a failure.
        const NOT_FOUND: i32 = -25300;
        let service = Self::service(plugin);
        match value {
            Some(value) => security_framework::passwords::set_generic_password(&service, name, value.as_bytes()),
            None => match security_framework::passwords::delete_generic_password(&service, name) {
                Err(error) if error.code() == NOT_FOUND => Ok(()),
                other => other,
            },
        }
        .map_err(|error| error.to_string())
    }
}

impl Plugins {
    /// Creates the plugin host over the backend's data directory, connects it to the open
    /// projects and starts loading the installed plugins in the background. Call once the
    /// backend runs.
    pub fn init(cx: &mut App) -> anyhow::Result<()> {
        let backend = cx.global::<Backend>();
        let (projects, plugins_dir) = (backend.host.clone(), backend.dirs.data.join("plugins"));
        let (sender, events) = unbounded::<Delivery>();
        let weak_projects = Arc::downgrade(&projects);
        let sink = move |client: Option<&str>, event: PluginEvent| {
            // Languages are the backend's business whether or not a window is listening.
            if matches!(event, PluginEvent::LanguagesChanged)
                && let Some(projects) = weak_projects.upgrade()
            {
                projects.plugin_languages_changed();
            }
            // The receiver is gone only while the app shuts down.
            let _ = sender.unbounded_send((client.map(str::to_owned), event));
        };
        let host = Arc::new(
            PluginHost::new(plugins_dir, Arc::new(sink), Limits::default()).map_err(|e| anyhow::anyhow!(e.message))?,
        );
        projects.set_plugin_host(&host);
        // Plugin languages join detection, language servers and debuggers of every project, and
        // the syntax of the ones without a built-in grammar comes from their plugin.
        projects.languages.attach_plugins(host.language_store().clone());
        ide_language::set_external_syntax(Some(host.syntax_provider()));
        ide_run::set_external_detection(Some(host.run_detection()));
        host.set_workspace(Some(Arc::new(HostWorkspace(Arc::downgrade(&projects)))));
        // Released builds list the official plugins until the user picks another index.
        if !cfg!(debug_assertions) {
            host.set_default_market_url(Some(ide_host::plugins::OFFICIAL_MARKET_INDEX.into()));
        }
        // Without the keychain (other systems, test builds, a home of its own) the host keeps them
        // in files under its data directory.
        #[cfg(all(target_os = "macos", not(test)))]
        if !cx.global::<Backend>().dirs.isolated {
            host.set_secret_store(Arc::new(KeychainSecrets));
        }
        ide_host::plugins::set_editor_providers(Some(&host));
        let trust = Arc::downgrade(&projects);
        host.set_project_policy(Arc::new(move |root| {
            trust.upgrade().is_some_and(|projects| projects.trust.trusted(root).unwrap_or(false))
        }));
        Self::supervise(&host)?;

        let pump = cx.spawn(async move |cx: &mut AsyncApp| Self::pump(events, cx).await);
        cx.set_global(Self {
            host,
            snapshot: None,
            listeners: Vec::new(),
            next_listener: 0,
            listing: None,
            command_actions: Vec::new(),
            listings: 0,
            _pump: pump,
        });
        // The plugin host's selected theme follows the theme the settings name.
        appearance::after_apply(follow_theme_setting, cx);
        Self::refresh(cx);
        Ok(())
    }

    /// The host's own thread: the first load, then event delivery, supervision (crash reports,
    /// idle reclaim, schedules) and the daily marketplace check, for as long as the host lives.
    fn supervise(host: &Arc<PluginHost>) -> anyhow::Result<()> {
        let loader = Arc::downgrade(host);
        std::thread::Builder::new().name("plugin-load".into()).spawn(move || {
            let Some(host) = loader.upgrade() else { return };
            host.load_all();
            drop(host);
            std::thread::sleep(STARTUP_FINISHED_DELAY);
            let Some(host) = loader.upgrade() else { return };
            host.startup_finished();
            host.warm_component_cache();
            drop(host);
            let mut market_due = Instant::now() + Duration::from_secs(60);
            for flush in 1u32.. {
                std::thread::sleep(EVENT_FLUSH);
                let Some(host) = loader.upgrade() else { return };
                host.flush_events();
                if flush.is_multiple_of(TICKS_PER_SUPERVISION) {
                    host.tick();
                }
                if Instant::now() >= market_due {
                    market_due = Instant::now() + MARKET_REFRESH;
                    if host.market_url().is_some()
                        && let Err(error) = host.refresh_market()
                    {
                        tracing::info!("plugin marketplace: {error}");
                    }
                }
            }
        })?;
        Ok(())
    }

    /// Delivers plugin events on the main thread, in order.
    async fn pump(mut events: UnboundedReceiver<Delivery>, cx: &mut AsyncApp) {
        while let Some((client, event)) = events.next().await {
            cx.update(|cx| {
                match &event {
                    // Plugins, their status or their contributions changed.
                    PluginEvent::CommandsChanged => Self::refresh(cx),
                    PluginEvent::ThemeChanged { theme } => appearance::set_plugin_theme(applied_theme(theme), cx),
                    _ => {}
                }
                let listeners: Vec<Rc<Listener>> =
                    cx.global::<Plugins>().listeners.iter().map(|(_, listener)| listener.clone()).collect();
                for listener in listeners {
                    listener(client.as_deref(), &event, cx);
                }
            });
        }
    }

    /// Calls `listener` for every plugin event, with the window it is for (none: every window);
    /// returns its id for [`Plugins::unlisten`].
    pub fn listen(&mut self, listener: impl Fn(Option<&str>, &PluginEvent, &mut App) + 'static) -> u64 {
        self.next_listener += 1;
        self.listeners.push((self.next_listener, Rc::new(listener)));
        self.next_listener
    }

    pub fn unlisten(&mut self, id: u64) {
        self.listeners.retain(|(listener, _)| *listener != id);
    }

    /// The plugins and their UI contributions as last listed.
    pub fn snapshot(&self) -> Option<&PluginsSnapshot> {
        self.snapshot.as_ref()
    }

    /// How many listings have been projected into the UI since the host started.
    pub fn listings(&self) -> u64 {
        self.listings
    }

    /// Lists the plugins again, off the main thread, and projects what they contribute: the
    /// installed and linked ones, and the own plugins of every open trusted project, loaded first.
    /// A project that is not trusted is left alone: its plugins were unloaded when it lost its
    /// trust, and restricting it again would announce a change, which lists the plugins again.
    pub fn refresh(cx: &mut App) {
        let host = cx.global::<Plugins>().host.clone();
        let projects = cx.try_global::<Backend>().map(|backend| backend.host.clone());
        let listed = cx.background_executor().spawn(async move {
            // A listing made at start-up would otherwise hold no plugins and drop the selected theme.
            host.wait_initial_load(INITIAL_LOAD_WAIT);
            host.sync_links();
            let mut roots: Vec<PathBuf> = projects
                .iter()
                .flat_map(|projects| {
                    projects
                        .projects()
                        .into_iter()
                        .map(|project| project.root.clone())
                        .filter(|root| projects.trust.trusted(root).unwrap_or(false))
                })
                .collect();
            roots.sort();
            roots.dedup();
            for root in &roots {
                host.sync_project(root);
            }
            let mut snapshot = host.list(None);
            for root in &roots {
                let own = host.list(Some(root));
                let added: Vec<_> = own
                    .plugins
                    .into_iter()
                    .filter(|plugin| !snapshot.plugins.iter().any(|listed| listed.id == plugin.id))
                    .collect();
                snapshot.ui.extend(own.ui.into_iter().filter(|c| added.iter().any(|plugin| plugin.id == c.plugin_id)));
                snapshot.plugins.extend(added);
            }
            snapshot.plugins.sort_by_key(|plugin| plugin.name.to_lowercase());
            snapshot
        });
        let listing = cx.spawn(async move |cx: &mut AsyncApp| {
            let snapshot = listed.await;
            cx.update(|cx| Self::listed(snapshot, cx));
        });
        cx.global_mut::<Plugins>().listing = Some(listing);
    }

    /// Unloads the own plugins of the project at `root` off the main thread, unless another
    /// window still has it open; the host announces the change, and the plugins are listed again.
    pub fn release_project(root: PathBuf, cx: &mut App) {
        let host = cx.global::<Plugins>().host.clone();
        let projects = cx.try_global::<Backend>().map(|backend| backend.host.clone());
        cx.background_executor()
            .spawn(async move {
                let open = projects.is_some_and(|projects| projects.projects().iter().any(|p| p.root == root));
                if !open {
                    host.restrict_project(&root);
                }
            })
            .detach();
    }

    fn listed(snapshot: PluginsSnapshot, cx: &mut App) {
        cx.global_mut::<Plugins>().listings += 1;
        let name_of = |plugin_id: &str| {
            snapshot.plugins.iter().find(|plugin| plugin.id == plugin_id).map(|plugin| plugin.name.clone())
        };
        // Offered by every theme picker while their plugin is live.
        let themes: Vec<ThemeChoice> = snapshot
            .ui
            .iter()
            .filter(|contribution| contribution.point == THEMES_POINT)
            .filter_map(|contribution| {
                let decl: ThemeDef = serde_json::from_value(contribution.decl.clone()).ok()?;
                let id = ThemeChoiceId::parse(&plugin_theme_ref(&contribution.plugin_id, &decl.id))?;
                Some(ThemeChoice {
                    id,
                    label: decl.name,
                    kind: kind_of(decl.kind),
                    origin: name_of(&contribution.plugin_id),
                })
            })
            .collect();
        let active = applied_theme(&snapshot.active_theme);
        Self::project_commands(&snapshot, cx);
        cx.global_mut::<Plugins>().snapshot = Some(snapshot);
        appearance::set_plugin_theme(active, cx);
        appearance::set_contributed(themes, cx);
        follow_theme_setting(cx);
    }
}

impl Plugins {
    /// Makes every declared command of a live plugin an action (Find Action, the keymap, menus),
    /// in place of the ones registered for the previous listing. Running one runs the command
    /// in the plugin, loading its code on first use.
    fn project_commands(snapshot: &PluginsSnapshot, cx: &mut App) {
        let previous = std::mem::take(&mut cx.global_mut::<Plugins>().command_actions);
        let registry = cx.default_global::<ActionRegistry>();
        for id in &previous {
            registry.unregister(id);
        }
        let mut registered = Vec::new();
        for contribution in snapshot.ui.iter().filter(|contribution| contribution.point == COMMANDS_POINT) {
            let Ok(decl) = serde_json::from_value::<CommandDecl>(contribution.decl.clone()) else { continue };
            let Some(plugin) = snapshot.plugins.iter().find(|plugin| plugin.id == contribution.plugin_id) else {
                continue;
            };
            let id = plugin_action_id(&plugin.id, &decl.id);
            let meta = ActionMeta {
                category: Some(decl.category.clone().unwrap_or_else(|| plugin.name.clone())),
                // A named icon is used as is; a plugin's own image is not loaded yet.
                icon: Some(
                    decl.icon.clone().filter(|icon| !icon.ends_with(".svg")).unwrap_or_else(|| "package".to_owned()),
                ),
                when: decl.when.clone(),
                verbatim: true,
                keywords: vec!["plugin".to_owned(), plugin.name.clone(), plugin.id.clone()],
                description: Some(format!("From plugin {}", plugin.name)),
                ..ActionMeta::new(&id, &decl.title)
            };
            let (plugin_id, command_id, title) = (plugin.id.clone(), decl.id.clone(), decl.title.clone());
            registry.register(Action::new(meta, move |call, window, cx| {
                let (arg, paths) = command_args(call.arg);
                run_command(&plugin_id, &command_id, &title, arg, paths, window, cx);
                Ok(())
            }));
            registered.push(id);
        }
        cx.global_mut::<Plugins>().command_actions = registered;
    }
}

/// The editor a plugin call is about: the window's active editor, with its selection as lines
/// and UTF-16 columns.
fn editor_context(workbench: &Entity<Workbench>, cx: &App) -> Option<PluginEditor> {
    let editor = workbench.read(cx).editor()?.read(cx);
    Some(PluginEditor {
        path: editor.path().to_string_lossy().into_owned(),
        language: editor.language().0.clone(),
        selections: vec![editor.selection_range()],
    })
}

/// Runs blocking plugin host `work` on the backend's blocking threads, as the command adapters do,
/// never on the UI's own: a command may wait on a program for minutes, and the window keeps hearing
/// what the plugin reports meanwhile. The task fails when the backend is not running.
pub(crate) fn host_blocking<T: Send + 'static>(
    cx: &App,
    work: impl FnOnce() -> ide_core::IdeResult<T> + Send + 'static,
) -> Task<ide_core::IdeResult<T>> {
    let call =
        cx.try_global::<Backend>().map(|backend| backend.call(async move { tokio::task::spawn_blocking(work).await }));
    cx.background_executor().spawn(async move {
        let Some(call) = call else { return Err(ide_core::IdeError::internal("the backend is not running")) };
        match call.await {
            Ok(Ok(result)) => result,
            Ok(Err(stopped)) => Err(ide_core::IdeError::internal(stopped.to_string())),
            Err(error) => Err(ide_core::IdeError::internal(error.to_string())),
        }
    })
}

/// Runs command `command_id` of plugin `plugin_id` for `window`: its project, its focused
/// editor and the given selection of paths. A failure is shown in the window, unless the
/// plugin was stopped (the host says why) or lacks a permission (it asks for it).
fn run_command(
    plugin_id: &str,
    command_id: &str,
    title: &str,
    arg: Option<String>,
    paths: Vec<PathBuf>,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(workbench) = workbench::for_window(window, cx) else { return };
    let invocation = Invocation {
        command_chain: Vec::new(),
        client: Some(workbench.read(cx).label().to_owned()),
        workspace_root: workbench.read(cx).project_root(cx),
        editor: editor_context(&workbench, cx).map(PluginEditor::context),
        selected_paths: paths,
        global_settings: false,
    };
    let host = cx.global::<Plugins>().host.clone();
    let (plugin, command, title) = (plugin_id.to_owned(), command_id.to_owned(), title.to_owned());
    // A command may run for as long as the host's time limit allows.
    let ran = host_blocking(cx, {
        let (host, plugin) = (host.clone(), plugin.clone());
        move || host.run_command(&plugin, &command, arg.as_deref(), invocation)
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let Err(error) = ran.await else { return };
        cx.update(|cx| {
            Plugins::refresh(cx);
            let stopped = host.info_of(&plugin).is_ok_and(|info| info.status == PluginStatus::Failed);
            if stopped || error.kind == ide_core::error::ErrorKind::PermissionDenied {
                return;
            }
            let request = NotificationRequest {
                body: Some(error.message.clone()),
                group: Some("plugins".to_owned()),
                ..NotificationRequest::new(NotificationLevel::Error, format!("{title} failed"))
            };
            workbench.update(cx, |workbench, cx| workbench.notify(request, cx));
        });
    })
    .detach();
}

fn kind_of(kind: ide_host::plugins::ThemeKind) -> ThemeKind {
    match kind {
        ide_host::plugins::ThemeKind::Light => ThemeKind::Light,
        ide_host::plugins::ThemeKind::Dark => ThemeKind::Dark,
    }
}

fn applied_theme(active: &Option<ActiveTheme>) -> Option<PluginTheme> {
    active.as_ref().map(|active| PluginTheme {
        reference: format!("{}/{}", active.plugin_id, active.theme.id),
        kind: kind_of(active.theme.kind),
        tokens: active.theme.tokens.clone(),
    })
}

/// Makes the plugin host's theme the one the Appearance settings select: a plugin theme is
/// selected there once its plugin is live, a built-in one clears it. While the selected plugin
/// is off, its built-in base shows.
fn follow_theme_setting(cx: &mut App) {
    let Some(plugins) = cx.try_global::<Plugins>().filter(|plugins| plugins.snapshot.is_some()) else { return };
    let host = plugins.host.clone();
    let choice = appearance::current_choice(cx);
    let available =
        cx.try_global::<appearance::ThemeState>().is_some_and(|state| state.contributed.is_available(&choice));
    let wanted = match &choice {
        ThemeChoiceId::Plugin(reference) if available => parse_plugin_theme_ref(&choice.to_string())
            .map(|_| (reference.plugin_id().to_owned(), reference.theme_id().to_owned())),
        _ => None,
    };
    let target = wanted.as_ref().map(|(plugin, theme)| format!("{plugin}/{theme}"));
    if target.as_deref() == appearance::plugin_theme(cx) {
        return;
    }
    // The host announces the change; the theme is applied when that arrives.
    let selection = wanted.as_ref().map(|(plugin, theme)| (plugin.as_str(), theme.as_str()));
    if let Err(error) = host.set_theme(selection) {
        tracing::warn!("cannot select the plugin theme: {}", error.message);
    }
}

/// Waits until no other test runs an app plugin host, and keeps the others waiting while the guard
/// lives: a host registers process-wide providers (editor providers, run detection, syntax), so two
/// at once would answer each other's windows.
#[cfg(test)]
pub(crate) fn lock_test_hosts() -> parking_lot::MutexGuard<'static, ()> {
    static HOSTS: parking_lot::Mutex<()> = parking_lot::const_mutex(());
    HOSTS.lock()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use crate::platform::settings::{self, SettingsWrite};
    use crate::platform::storage::UiStorage;
    use crate::theme::ActiveTheme;
    use crate::theme::themes::{KINDS_KEY, PLUGIN_THEME_ATTRIBUTE};
    use gpui_kit::TestAppContext;
    use serde_json::json;
    use std::path::Path;

    const HARBOR_NIGHT: &str = "plugin:openidea.harbor-night/harbor-night";

    fn settle(cx: &mut TestAppContext, what: &str, done: impl Fn(&mut App) -> bool) {
        for _ in 0..1000 {
            cx.run_until_parked();
            if cx.update(|cx| done(cx)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("{what}: not within ten seconds");
    }

    fn accent(cx: &App) -> Option<String> {
        cx.global::<ActiveTheme>().theme.color("accent").map(|color| color.to_hex().to_lowercase())
    }

    /// A data-only plugin from `plugins/official` is linked into this app's own plugin folder,
    /// its theme is offered next to the built-in ones, and selecting it in the settings makes
    /// the windows draw with its tokens; a built-in theme takes them away again.
    #[gpui_kit::test]
    async fn a_linked_plugins_theme_is_offered_and_applied(cx: &mut TestAppContext) {
        let _hosts = lock_test_hosts();
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        let plugin = home.path().join("fixtures/harbor-night");
        copy_folder(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/official/harbor-night"), &plugin);
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            appearance::init(cx);
            Plugins::init(cx).expect("the plugin host starts");
        });
        settle(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
        cx.update(|cx| {
            assert!(cx.global::<appearance::ThemeState>().contributed.themes().is_empty());
            let info = cx.global::<Plugins>().host.link(&plugin).expect("the plugin links");
            assert_eq!(info.id, "openidea.harbor-night");
            // A newly linked plugin contributes nothing until the user has agreed to it.
            assert_eq!(info.status, ide_host::plugins::PluginStatus::NeedsConsent);
            let granted = cx
                .global::<Plugins>()
                .host
                .grant(&info.id, &ide_host::plugins::Consent::default())
                .expect("consent is recorded");
            assert_ne!(granted.status, ide_host::plugins::PluginStatus::NeedsConsent);
        });
        // Linking announces the change; the listing that follows carries the theme.
        settle(cx, "the theme is offered", |cx| {
            cx.global::<appearance::ThemeState>().contributed.themes().iter().any(|t| t.id.to_string() == HARBOR_NIGHT)
        });
        cx.update(|cx| {
            let offered = cx.global::<appearance::ThemeState>().contributed.themes()[0].clone();
            assert_eq!((offered.label.as_str(), offered.kind), ("Harbor Night", ThemeKind::Dark));
            assert_eq!(offered.origin.as_deref(), Some("Harbor Night"));
            assert!(cx.global::<UiStorage>().get(KINDS_KEY).is_some_and(|kinds| kinds.contains(HARBOR_NIGHT)));
            assert!(!cx.global::<ActiveTheme>().is_dark(), "offering a theme does not select it");
            let write = SettingsWrite { key: "appearance.theme", value: json!(HARBOR_NIGHT) };
            settings::set_global("w1", &[write], cx).expect("the setting is stored");
        });
        settle(cx, "the plugin theme applies", |cx| accent(cx).as_deref() == Some("#2b9e93"));
        cx.update(|cx| {
            let theme = &cx.global::<ActiveTheme>().theme;
            assert!(cx.global::<ActiveTheme>().is_dark());
            assert_eq!(theme.attributes().get(PLUGIN_THEME_ATTRIBUTE), Some("openidea.harbor-night/harbor-night"));
            assert_eq!(theme.color("bg-app").map(|c| c.to_hex().to_lowercase()).as_deref(), Some("#14171c"));
            assert_eq!(cx.global::<Plugins>().host.active_theme().map(|t| t.theme.id), Some("harbor-night".to_owned()));
            let write = SettingsWrite { key: "appearance.theme", value: json!("light") };
            settings::set_global("w1", &[write], cx).expect("the setting is stored");
        });
        settle(cx, "the built-in theme is back", |cx| {
            !cx.global::<ActiveTheme>().is_dark() && accent(cx).as_deref() != Some("#2b9e93")
        });
        cx.update(|cx| {
            assert_eq!(cx.global::<ActiveTheme>().theme.attributes().get(PLUGIN_THEME_ATTRIBUTE), None);
            assert!(cx.global::<Plugins>().host.active_theme().is_none());
        });
        // Nothing was written outside this app's own folders: the link lives under its data.
        assert!(home.path().join("data/plugins").is_dir());
    }

    // src/features/plugins/points.tsx › commandArgs
    #[test]
    fn an_actions_argument_becomes_a_commands_argument_and_paths() {
        assert_eq!(command_args(None), (None, Vec::new()));
        assert_eq!(command_args(Some(&json!("text"))), (Some("text".to_owned()), Vec::new()));
        assert_eq!(command_args(Some(&json!({ "path": "/p/a" }))), (None, Vec::new()));
        assert_eq!(
            command_args(Some(&json!({ "path": "/p/a", "paths": ["/p/a", 7, "/p/b"] }))),
            (Some("/p/a".to_owned()), vec![PathBuf::from("/p/a"), PathBuf::from("/p/b")])
        );
        let (arg, paths) = command_args(Some(&json!({ "path": "/p/a", "paths": [], "line": 3, "column": 9 })));
        assert_eq!(paths, Vec::<PathBuf>::new());
        let position: serde_json::Value = serde_json::from_str(&arg.expect("a position")).expect("JSON");
        assert_eq!(position, json!({ "path": "/p/a", "line": 3, "column": 9 }));
        assert_eq!(plugin_action_id("openidea.string-tools", "snake-case"), "plugin.openidea.string-tools.snake-case");
    }

    /// A plugin with code: its commands are actions as soon as it is agreed to, before any of
    /// its code is loaded; running one loads the Wasm, which reads the editor's selection and
    /// rewrites it through the backend, and the editor follows. A command that fails says so
    /// in the window.
    #[gpui_kit::test]
    async fn a_plugins_command_runs_its_wasm_on_the_editors_selection(cx: &mut TestAppContext) {
        use crate::platform::actions::{execute_action_or_throw, is_action_enabled};
        use crate::platform::context::window_context;
        /// Whether the action may run in `window` now.
        fn available(id: &str, window: &Window, cx: &App) -> bool {
            let action = cx.global::<ActionRegistry>().get(id).expect("registered");
            is_action_enabled(&action.meta, &window_context(window, cx))
        }
        use ide_host::plugins::Consent;
        let _hosts = lock_test_hosts();
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        let folder = tempfile::tempdir().expect("a folder");
        let file = folder.path().canonicalize().expect("the folder exists").join("names.txt");
        std::fs::write(&file, "hello big world\n").expect("the file is written");
        let plugin = home.path().join("fixtures/string-tools");
        copy_folder(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/official/string-tools"), &plugin);
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            cx.set_global(UiStorage::default());
            crate::platform::input::init(cx);
            crate::editor::actions::register(cx);
            workbench::register_actions(cx);
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            appearance::init(cx);
            Plugins::init(cx).expect("the plugin host starts");
        });
        settle(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
        const SNAKE_CASE: &str = "plugin.openidea.string-tools.snake-case";
        cx.update(|cx| {
            let host = cx.global::<Plugins>().host.clone();
            let info = host.link(&plugin).expect("the plugin links");
            assert!(cx.global::<ActionRegistry>().get(SNAKE_CASE).is_none(), "nothing before consent");
            let info = host.grant(&info.id, &Consent::default()).expect("consent is recorded");
            assert_eq!(info.status, PluginStatus::Inactive, "agreed to, its code not loaded");
        });
        settle(cx, "the commands are actions", |cx| cx.global::<ActionRegistry>().get(SNAKE_CASE).is_some());
        cx.update(|cx| {
            let action = cx.global::<ActionRegistry>().get(SNAKE_CASE).expect("registered");
            assert_eq!(action.meta.title, "String Tools: To snake_case");
            assert_eq!(action.meta.category.as_deref(), Some("String Tools › Case"));
            assert!(action.meta.verbatim);
            assert_eq!(cx.global::<Plugins>().command_actions.len(), 26);
        });

        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let visual =
            |cx: &mut gpui_kit::VisualTestContext, what: &str, done: &dyn Fn(&mut Window, &mut App) -> bool| {
                for _ in 0..6000 {
                    cx.run_until_parked();
                    if cx.update(|window, cx| done(window, cx)) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                panic!("{what}: not within a minute");
            };
        // `when = "editorFocus"`: without an editor the command is not available.
        assert!(!cx.update(|window, cx| available(SNAKE_CASE, window, cx)));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        visual(cx, "the file opens", &|_, cx| workbench.read(cx).editor().is_some());
        let editor = workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor");
        editor.update(cx, |editor, cx| editor.select(0, 15, cx));
        visual(cx, "the command is available", &|window, cx| available(SNAKE_CASE, window, cx));
        cx.update(|window, cx| execute_action_or_throw(SNAKE_CASE, None, window, cx)).expect("it runs");
        visual(cx, "the plugin rewrote the selection", &|_, cx| editor.read(cx).text() == "hello_big_world\n");
        assert_eq!(
            cx.update(|_, cx| cx.global::<Plugins>().host.list(None).plugins[0].status),
            PluginStatus::Active,
            "running a command loaded the plugin's code"
        );
    }

    /// Copies the folder `from` into `to`, recursively, without build output (`target`) or `.git`.
    fn copy_folder(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("the copy's folder is made");
        for entry in std::fs::read_dir(from).expect("the folder is listed") {
            let entry = entry.expect("an entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("its type").is_dir() {
                if entry.file_name() == "target" || entry.file_name() == ".git" {
                    continue;
                }
                copy_folder(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), &target).expect("the file is copied");
            }
        }
    }

    /// A plugin folder linked into a temporary home, agreed to with its required capabilities only,
    /// comes up without an error and contributes to the UI; running one of its commands without a
    /// project or an editor loads its code, and the plugin is active afterwards, whatever the command
    /// answers. The folder is a copy, in a temporary folder, of the checked-in fixture `hello` (its
    /// command `word-count`), which declares no secrets, so the login keychain is never asked; `OPENIDEA_GPUI_TEST_PLUGIN=<folder>` names another plugin folder,
    /// copied the same way, and `OPENIDEA_GPUI_TEST_COMMAND=<id>` the command to run.
    #[gpui_kit::test]
    async fn a_plugin_folder_copied_into_a_temporary_home_loads_and_runs_a_command(cx: &mut TestAppContext) {
        use ide_host::plugins::{Consent, PluginStatus};
        cx.executor().allow_parking();
        let named = std::env::var_os("OPENIDEA_GPUI_TEST_PLUGIN").map(PathBuf::from);
        let source = named
            .clone()
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates/ide-plugin/tests/fixtures/hello"));
        let command = match std::env::var_os("OPENIDEA_GPUI_TEST_COMMAND") {
            Some(command) => Some(command.to_string_lossy().into_owned()),
            None => named.is_none().then(|| "word-count".to_owned()),
        };
        let _hosts = lock_test_hosts();
        let copies = tempfile::tempdir().expect("a folder for the copy");
        let folder = copies.path().join("plugin");
        copy_folder(&source, &folder);
        let home = tempfile::tempdir().expect("a home");
        cx.update(|cx| {
            cx.set_global(ActiveTheme::default());
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
            appearance::init(cx);
            Plugins::init(cx).expect("the plugin host starts");
        });
        settle(cx, "the first listing", |cx| cx.global::<Plugins>().snapshot().is_some());
        let id = cx.update(|cx| {
            let host = cx.global::<Plugins>().host.clone();
            let info = host.link(&folder).expect("the plugin links");
            let info = match info.status {
                PluginStatus::NeedsConsent => host.grant(&info.id, &Consent::default()).expect("consent is recorded"),
                _ => info,
            };
            assert_eq!(info.error, None, "the plugin reports an error");
            assert!(
                matches!(info.status, PluginStatus::Active | PluginStatus::Inactive | PluginStatus::Static),
                "the plugin is {:?}",
                info.status
            );
            info.id
        });
        settle(cx, "the plugin's contributions are listed", |cx| {
            cx.global::<Plugins>().snapshot().is_some_and(|listed| listed.ui.iter().any(|c| c.plugin_id == id))
        });
        cx.update(|cx| {
            let plugins = cx.global::<Plugins>();
            let listed = plugins.snapshot().expect("a listing");
            let mut points = std::collections::BTreeMap::<&str, usize>::new();
            for contribution in listed.ui.iter().filter(|c| c.plugin_id == id) {
                *points.entry(contribution.point.as_str()).or_default() += 1;
            }
            let languages: Vec<String> =
                plugins.host.languages().into_iter().map(|language| format!("{:?}", language.id)).collect();
            let info = listed.plugins.iter().find(|plugin| plugin.id == id).expect("the plugin is listed");
            println!(
                "{id} {}: {:?}; UI contributions by point: {points:?}; languages: {languages:?}",
                info.version, info.status
            );
        });
        if let Some(command) = command {
            let host = cx.update(|cx| cx.global::<Plugins>().host.clone());
            let invocation = Invocation {
                command_chain: Vec::new(),
                client: None,
                workspace_root: None,
                editor: None,
                selected_paths: Vec::new(),
                global_settings: false,
            };
            let answer = host.run_command(&id, &command, None, invocation);
            let status = host.list(None).plugins.iter().find(|plugin| plugin.id == id).map(|plugin| plugin.status);
            println!("{id} {command}: {:?}; the plugin is now {status:?}", answer.map_err(|error| error.message));
            assert_eq!(status, Some(PluginStatus::Active), "running a command loads the plugin's code");
        }
    }
}
