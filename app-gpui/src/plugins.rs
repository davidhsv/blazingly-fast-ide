//! The plugin host in this app: the backend's [`PluginHost`] wired to the open projects, its
//! events brought to the main thread, and its contributions projected into the UI.
//!
//! Plugins are loaded from `<data>/plugins` (installed ones, and folders linked for
//! development). Loading instantiates no Wasm: a plugin's code runs on its activation events.

use crate::backend::{APP_IDENTIFIER, Backend};
use crate::theme::appearance::{self, PluginTheme};
use crate::theme::themes::{ThemeChoice, ThemeChoiceId, ThemeKind, parse_plugin_theme_ref, plugin_theme_ref};
use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::{App, AsyncApp, Global, Task};
use ide_host::plugins::{ActiveTheme, HostWorkspace, PluginEvent, PluginHost, PluginsSnapshot, ThemeDef};
use ide_plugin::Limits;
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
    _pump: Task<()>,
}

impl Global for Plugins {}

/// Plugin secrets in the login keychain, one service per plugin under this app's identifier.
#[cfg(target_os = "macos")]
struct KeychainSecrets;

#[cfg(target_os = "macos")]
impl KeychainSecrets {
    fn service(plugin: &str) -> String {
        format!("{APP_IDENTIFIER}.plugin.{plugin}")
    }
}

#[cfg(target_os = "macos")]
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
        #[cfg(target_os = "macos")]
        host.set_secret_store(Arc::new(KeychainSecrets));
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

    /// Lists the plugins again, off the main thread, and projects what they contribute.
    pub fn refresh(cx: &mut App) {
        let host = cx.global::<Plugins>().host.clone();
        let listed = cx.background_executor().spawn(async move {
            // A listing made at start-up would otherwise hold no plugins and drop the selected theme.
            host.wait_initial_load(INITIAL_LOAD_WAIT);
            host.sync_links();
            host.list(None)
        });
        let listing = cx.spawn(async move |cx: &mut AsyncApp| {
            let snapshot = listed.await;
            cx.update(|cx| Self::listed(snapshot, cx));
        });
        cx.global_mut::<Plugins>().listing = Some(listing);
    }

    fn listed(snapshot: PluginsSnapshot, cx: &mut App) {
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
        cx.global_mut::<Plugins>().snapshot = Some(snapshot);
        appearance::set_plugin_theme(active, cx);
        appearance::set_contributed(themes, cx);
        follow_theme_setting(cx);
    }
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
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a home");
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../plugins/official/harbor-night");
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

    /// For a plugin outside this repository: `OPENIDEA_GPUI_TEST_PLUGIN=<folder> cargo test --lib
    /// a_plugin_folder_named_by_the_environment_loads -- --ignored --nocapture`. The folder is
    /// linked into a temporary home, agreed to with its required capabilities only, and must
    /// come up without an error, contributing to the UI.
    #[gpui_kit::test]
    #[ignore = "needs OPENIDEA_GPUI_TEST_PLUGIN to name a plugin folder"]
    async fn a_plugin_folder_named_by_the_environment_loads(cx: &mut TestAppContext) {
        use ide_host::plugins::{Consent, PluginStatus};
        cx.executor().allow_parking();
        let folder = std::env::var_os("OPENIDEA_GPUI_TEST_PLUGIN").expect("OPENIDEA_GPUI_TEST_PLUGIN names a folder");
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
            let info = host.link(Path::new(&folder)).expect("the plugin links");
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
    }
}
