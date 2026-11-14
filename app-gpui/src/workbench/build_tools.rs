//! The Maven and Gradle tool windows in the window: the project's build model read from its build
//! files (`build_model`), the tool windows registered while the project has a Maven project or a
//! linked Gradle build, the toolbar toggles and profile choices kept per project, reloading a build
//! with Maven or Gradle itself (`build_sync`, with progress, Stop and its errors told), linking and
//! unlinking Gradle builds, Execute Maven Goal / Execute Gradle Task, and the phases, goals and
//! tasks started as temporary run configurations through the Run window.
//!
//! The backend refuses all of it in Restricted Mode (`require_trust` in `ide_host::ops::build`):
//! the model is not read, so no tool window is registered and the `build.*` keys stay off.

use super::branches::run_action_later;
use super::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use super::services_window::on_workbench;
use super::tasks::{Cancel, Change, TaskId, TaskUpdate};
use super::tool_windows::{Effects, Registration};
use super::{Panel, Workbench, tool_window_declaration};
use crate::backend::Backend;
use crate::build_tools::execute::{ExecuteEvent, ExecutePopup, execute_config, history_line};
use crate::build_tools::model::{
    BuildSettings, GRADLE_TOOL_WINDOW, MAVEN_TOOL_WINDOW, SETTINGS_KEY, history_key, initial_expanded, parse_history,
    profile_key, profiles_list, pushed_history, tool_id, tool_of_window,
};
use crate::build_tools::view::{BuildMenuCommand, BuildToolEvent, BuildToolInput, BuildToolView, SyncBanner};
use crate::platform::actions::{Action, ActionMeta, ActionRegistry};
use crate::platform::context::{set_context, window_context};
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::vcs::prompt_dialogs::{PromptDialog, PromptDialogEvent};
use crate::vcs_ops::dialogs::ConfirmRequest;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, ClipboardItem, Context, Entity, Focusable, PathPromptOptions, Subscription, Task,
    Window,
};
use ide_build::{BuildModel, BuildTool, SyncProgress};
use ide_core::error::ErrorKind;
use ide_core::{IdeError, IdeEvent, IdeResult, RequestId};
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// The context key of a project with a Maven project.
pub(super) const MAVEN_KEY: &str = "build.maven";
/// The context key of a project with a linked Gradle build.
pub(super) const GRADLE_KEY: &str = "build.gradle";
/// The context keys the toolbar toggles' state is read from (toggle actions cannot read the
/// workbench while a menu is built inside its update).
const MAVEN_OFFLINE_KEY: &str = "build.maven.offline";
const MAVEN_SKIP_TESTS_KEY: &str = "build.maven.skipTests";
const GRADLE_OFFLINE_KEY: &str = "build.gradle.offline";
/// The status bar task of a sync is this followed by the tool's id.
pub(super) const SYNC_TASK_PREFIX: &str = "build-sync:";
/// How long build file changes are collected before the model is read again.
const RELOAD_DELAY: Duration = Duration::from_millis(300);

/// Tells one project's late answers from the next one's.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// A sync running for one tool.
struct RunningSync {
    request: RequestId,
    banner: SyncBanner,
    _task: Task<()>,
}

/// What one tool window holds.
#[derive(Default)]
struct ToolSlot {
    registration: Option<Registration>,
    view: Option<Entity<BuildToolView>>,
    _view_events: Option<Subscription>,
    sync: Option<RunningSync>,
    /// Builds to reload once the running sync ends (`None`: all of them).
    queued: VecDeque<Option<String>>,
    /// The project of the tree's lead selection: where Execute runs by default.
    current: Option<String>,
}

/// The build tools of the window's project.
#[derive(Default)]
pub(super) struct BuildToolsSlot {
    epoch: u64,
    window: Option<AnyWindowHandle>,
    root: Option<String>,
    model: Option<BuildModel>,
    /// The model has been asked for once at least, and answered.
    answered: bool,
    loading: bool,
    error: Option<String>,
    /// The model read last; an answer of an earlier one is dropped.
    load_seq: u64,
    settings: BuildSettings,
    configs: Vec<RunConfig>,
    maven: ToolSlot,
    gradle: ToolSlot,
    _loading: Option<Task<()>>,
    _reload: Option<Task<()>>,
    _runs: Option<Subscription>,
}

impl BuildToolsSlot {
    fn tool(&mut self, tool: BuildTool) -> &mut ToolSlot {
        match tool {
            BuildTool::Maven => &mut self.maven,
            BuildTool::Gradle => &mut self.gradle,
        }
    }

    fn tool_ref(&self, tool: BuildTool) -> &ToolSlot {
        match tool {
            BuildTool::Maven => &self.maven,
            BuildTool::Gradle => &self.gradle,
        }
    }
}

fn flatten<T>(outcome: anyhow::Result<IdeResult<T>>) -> IdeResult<T> {
    outcome.unwrap_or_else(|error| Err(IdeError::internal(error.to_string())))
}

fn merge(effects: &mut Effects, more: Effects) {
    effects.persist |= more.persist;
    effects.focus_editor |= more.focus_editor;
}

fn sync_title(tool: BuildTool) -> &'static str {
    match tool {
        BuildTool::Maven => "Syncing Maven projects",
        BuildTool::Gradle => "Syncing Gradle projects",
    }
}

/// `Cannot start …` / `command not found` naming a launcher, and Gradle refusing the JDK.
static MISSING_COMMAND: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)command not found: ([^\r\n]+)|cannot start ([^\r\n]+?): [^\r\n]*\(os error [23]\)")
        .expect("the pattern is valid")
});
static INCOMPATIBLE_JDK: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"(?i)Can't use Java .* and Gradle .* to import Gradle project").expect("the pattern is valid")
});

/// What a failed sync's notification says and offers: a hint and Maven's settings when the build
/// tool could not be started, the error itself.
fn recovery(message: &str) -> (String, Vec<NotificationAction>) {
    let incompatible = INCOMPATIBLE_JDK.is_match(message);
    let command = MISSING_COMMAND
        .captures(message)
        .and_then(|found| found.get(1).or_else(|| found.get(2)))
        .map(|found| found.as_str().trim().trim_matches(['"', '\'', '`']).to_owned())
        .unwrap_or_default();
    let name = command.rsplit(['/', '\\']).next().unwrap_or_default().to_ascii_lowercase();
    let stem = name.split('.').next().unwrap_or_default();
    let named = match stem {
        "mvn" | "mvnw" | "mvnd" => Some(BuildTool::Maven),
        "gradle" | "gradlew" => Some(BuildTool::Gradle),
        _ => None,
    };
    let tool = if incompatible { Some(BuildTool::Gradle) } else { named };
    let hint = match tool {
        None => return (message.to_owned(), Vec::new()),
        Some(BuildTool::Gradle) if incompatible => {
            "Choose a JDK compatible with this Gradle version in Gradle JVM. Download JDK is available in the JVM \
             selector."
        }
        Some(BuildTool::Maven) => {
            "Maven could not be started. Configure Maven home in Maven settings, install Maven on PATH, or use a \
             project with a Maven wrapper."
        }
        Some(BuildTool::Gradle) => {
            "Gradle could not be started. Configure a local installation in Gradle settings, install Gradle on PATH, \
             or use a project with a Gradle wrapper."
        }
    };
    let actions = match tool {
        Some(BuildTool::Maven) => {
            vec![NotificationAction { label: "Open Maven Settings".to_owned(), command: "maven.settings".to_owned() }]
        }
        _ => Vec::new(),
    };
    (format!("{hint}\n\n{message}"), actions)
}

/// Runs `act` on the window's workbench.
fn on_tool(
    window: &mut Window,
    cx: &mut gpui_kit::App,
    act: impl FnOnce(&mut Workbench, &mut Window, &mut Context<Workbench>),
) -> anyhow::Result<()> {
    on_workbench(window, cx, act)
}

/// The `buildDir` of an action's argument.
fn build_dir_arg(arg: Option<&Value>) -> Option<String> {
    arg.and_then(|arg| arg.get("buildDir")).and_then(Value::as_str).map(str::to_owned)
}

/// Registers the build tools' actions.
pub(super) fn register_actions(registry: &mut ActionRegistry) {
    registry.handle("maven.reload", |_, window, cx| {
        on_tool(window, cx, |w, window, cx| w.sync_build_tool(BuildTool::Maven, None, window, cx))
    });
    registry.handle("gradle.reload", |call, window, cx| {
        let dir = build_dir_arg(call.arg);
        on_tool(window, cx, move |w, window, cx| w.request_build_sync(BuildTool::Gradle, dir, window, cx))
    });
    for (id, tool) in [("maven.cancelReload", BuildTool::Maven), ("gradle.cancelReload", BuildTool::Gradle)] {
        registry.handle(id, move |_, window, cx| on_tool(window, cx, move |w, _, cx| w.cancel_build_sync(tool, cx)));
    }
    let trees = [
        ("maven.expandAll", BuildTool::Maven, true),
        ("maven.collapseAll", BuildTool::Maven, false),
        ("gradle.expandAll", BuildTool::Gradle, true),
        ("gradle.collapseAll", BuildTool::Gradle, false),
    ];
    for (id, tool, expand) in trees {
        registry.handle(id, move |_, window, cx| {
            on_tool(window, cx, move |w, _, cx| {
                let Some(view) = w.build_tool_view(tool) else { return };
                view.update(cx, |view, cx| if expand { view.expand_all(cx) } else { view.collapse_all(cx) });
            })
        });
    }
    type Flip = fn(&mut BuildSettings);
    let toggles: [(&str, &str, Flip); 3] = [
        ("maven.toggleOffline", MAVEN_OFFLINE_KEY, |s| s.maven.offline = !s.maven.offline),
        ("maven.toggleSkipTests", MAVEN_SKIP_TESTS_KEY, |s| s.maven.skip_tests = !s.maven.skip_tests),
        ("gradle.toggleOffline", GRADLE_OFFLINE_KEY, |s| s.gradle.offline = !s.gradle.offline),
    ];
    for (id, key, flip) in toggles {
        registry.handle_toggle(
            id,
            move |window, cx| window_context(window, cx).is_truthy(key),
            move |_, window, cx| {
                on_tool(window, cx, move |w, window, cx| {
                    let mut settings = w.build_tools.settings.clone();
                    flip(&mut settings);
                    w.set_build_settings(settings, window, cx);
                })
            },
        );
    }
    registry.handle("maven.executeGoal", |_, window, cx| {
        on_tool(window, cx, |w, window, cx| w.open_execute(BuildTool::Maven, None, window, cx))
    });
    registry.handle("gradle.executeTask", |call, window, cx| {
        let key = call.arg.and_then(|arg| arg.get("projectKey")).and_then(Value::as_str).map(str::to_owned);
        on_tool(window, cx, move |w, window, cx| w.open_execute(BuildTool::Gradle, key, window, cx))
    });
    registry.handle("maven.settings", |_, window, cx| {
        on_tool(window, cx, |w, window, cx| w.open_settings(Some("build.maven"), false, window, cx))
    });
    registry.handle("gradle.linkProjects", |_, window, cx| {
        on_tool(window, cx, |w, window, cx| w.link_gradle_projects(window, cx))
    });
    registry.handle("gradle.unlinkProjects", |call, window, cx| {
        let dir = build_dir_arg(call.arg);
        on_tool(window, cx, move |w, window, cx| w.unlink_gradle_project(dir, window, cx))
    });
    // The tool windows exist only while the project has such builds, so their show actions are
    // registered here rather than with the windows every project has.
    for (id, when) in
        [(MAVEN_TOOL_WINDOW, "projectOpen && build.maven"), (GRADLE_TOOL_WINDOW, "projectOpen && build.gradle")]
    {
        let Some(declared) = tool_window_declaration(id) else { continue };
        let meta = ActionMeta {
            category: Some("Tool Windows".to_owned()),
            icon: Some(declared.icon.clone()),
            keywords: ["tool window", "show", "hide"].map(str::to_owned).to_vec(),
            ..ActionMeta::new(&format!("toolwindow.{id}"), &declared.title).when(when)
        };
        registry.register(Action::new(meta, move |_, window, cx| {
            on_tool(window, cx, move |w, window, cx| w.with_layout(window, cx, |layout| layout.activate(id)))
        }));
    }
}

impl Workbench {
    // ---------------------------------------------------------------------------------------------
    // Read by tests and other parts of the window

    /// The project's build model, once read.
    pub fn build_model(&self) -> Option<&BuildModel> {
        self.build_tools.model.as_ref()
    }

    /// Whether the model was asked for and answered (or refused) since the project opened.
    pub fn build_model_answered(&self) -> bool {
        self.build_tools.answered && !self.build_tools.loading
    }

    /// Why the model could not be read.
    pub fn build_model_error(&self) -> Option<&str> {
        self.build_tools.error.as_deref()
    }

    /// The view of the Maven or Gradle tool window, while it is registered.
    pub fn build_tool_view(&self, tool: BuildTool) -> Option<Entity<BuildToolView>> {
        self.build_tools.tool_ref(tool).view.clone()
    }

    /// Whether a sync of `tool` is running.
    pub fn build_sync_running(&self, tool: BuildTool) -> bool {
        self.build_tools.tool_ref(tool).sync.is_some()
    }

    /// Execute Maven Goal / Execute Gradle Task, while it is open.
    pub fn execute_popup(&self) -> Option<Entity<ExecutePopup>> {
        self.popup.as_ref().and_then(|popup| popup.view.clone().downcast().ok())
    }

    // ---------------------------------------------------------------------------------------------
    // The project

    /// A project just opened: its toggles are read back and its model is read.
    pub(super) fn mount_build_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.project_root(cx).map(|root| root.to_string_lossy().into_owned()) else { return };
        let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(&format!("{SETTINGS_KEY}{root}")));
        let settings = BuildSettings::parse(stored);
        let runs = self.run_driver().map(|driver| {
            let store = driver.read(cx).store().clone();
            cx.observe(&store, |workbench, store, cx| {
                let configs = store.read(cx).configs().to_vec();
                if configs != workbench.build_tools.configs {
                    workbench.build_tools.configs = configs;
                    workbench.feed_build_tool_views(cx);
                }
            })
        });
        let configs =
            self.run_driver().map(|driver| driver.read(cx).store().read(cx).configs().to_vec()).unwrap_or_default();
        self.build_tools = BuildToolsSlot {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            window: Some(window.window_handle()),
            root: Some(root),
            settings,
            configs,
            _runs: runs,
            ..BuildToolsSlot::default()
        };
        self.publish_build_toggles(window, cx);
        self.load_build_model(cx);
    }

    /// The project goes: its syncs stop, its status bar tasks and keys go with it.
    pub(super) fn reset_build_tools(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let slot = std::mem::take(&mut self.build_tools);
        for tool in [BuildTool::Maven, BuildTool::Gradle] {
            if let Some(sync) = slot.tool_ref(tool).sync.as_ref()
                && let Some(backend) = cx.try_global::<Backend>()
            {
                backend.host.cancel_request(sync.request);
            }
            self.tasks.remove_task(&TaskId::Name(format!("{SYNC_TASK_PREFIX}{}", tool_id(tool))));
        }
        for key in [MAVEN_KEY, GRADLE_KEY, MAVEN_OFFLINE_KEY, MAVEN_SKIP_TESTS_KEY, GRADLE_OFFLINE_KEY] {
            set_context(window, cx, key, false);
        }
        self.schedule_tasks(cx);
    }

    /// A backend event that may change the model: a build file, the project's folders, the Java
    /// settings or the project's trust. The model is read again shortly after.
    pub(super) fn build_tools_backend_event(&mut self, event: &IdeEvent, cx: &mut Context<Self>) {
        let reload = match event {
            IdeEvent::FsChanged { created, modified, removed } => {
                let paths: Vec<String> = created
                    .iter()
                    .chain(modified)
                    .chain(removed)
                    .map(|path| path.to_string_lossy().into_owned())
                    .collect();
                crate::platform::project_presentation::changes_build_model(&paths)
            }
            IdeEvent::ProjectRootsChanged { .. } | IdeEvent::ProjectTrustChanged { .. } => true,
            IdeEvent::SettingsChanged { key } => {
                key.is_empty() || key == "languages" || key.starts_with("languages.java")
            }
            _ => false,
        };
        if reload && self.build_tools.window.is_some() {
            self.build_tools._reload = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RELOAD_DELAY).await;
                let _ = this.update(cx, |workbench, cx| workbench.load_build_model(cx));
            }));
        }
    }

    /// Reads the build model in the background; the window follows when it is answered.
    fn load_build_model(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.build_tools.window else { return };
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        let loading =
            backend.with_host(move |host| async move { ide_host::ops::build::build_model(&host, &label).await });
        let slot = &mut self.build_tools;
        slot.load_seq += 1;
        slot.loading = true;
        let seq = slot.load_seq;
        slot._loading = Some(cx.spawn(async move |this, cx| {
            let outcome = flatten(loading.await);
            let _ = handle.update(cx, |_, window, cx| {
                this.update(cx, |workbench, cx| workbench.build_model_loaded(seq, outcome, window, cx))
            });
        }));
        self.feed_build_tool_views(cx);
    }

    fn build_model_loaded(
        &mut self,
        seq: u64,
        outcome: IdeResult<BuildModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = &mut self.build_tools;
        if slot.load_seq != seq {
            return;
        }
        slot.loading = false;
        slot.answered = true;
        match outcome {
            Ok(model) => {
                slot.model = Some(model);
                slot.error = None;
            }
            Err(error) => {
                tracing::info!("build tools: the build model is not read: {}", error.message);
                slot.model = None;
                slot.error = Some(error.message);
            }
        }
        self.build_model_changed(window, cx);
    }

    /// The model changed: the keys, the tool windows, their views and the Execute popup follow.
    fn build_model_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_build_tool_windows(window, cx);
        self.feed_build_tool_views(cx);
        if let Some(popup) = self.execute_popup() {
            let model = self.build_tools.model.clone().unwrap_or_default();
            popup.update(cx, |popup, cx| popup.set_model(model, cx));
        }
        cx.notify();
    }

    /// Registers the Maven tool window while the project has a Maven project and the Gradle one
    /// while it has a linked build (or once it was shown to link one), and unregisters them after.
    fn sync_build_tool_windows(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.build_tools.model.as_ref();
        let has_maven = model.and_then(|m| m.maven.as_ref()).is_some_and(|m| !m.projects.is_empty());
        let has_gradle = model.is_some_and(|m| !m.gradle.is_empty());
        let keep_gradle = model.is_some() && self.build_tools.gradle.registration.is_some();
        set_context(window, cx, MAVEN_KEY, has_maven);
        set_context(window, cx, GRADLE_KEY, has_gradle);
        let mut register = Vec::new();
        let mut unregister = Vec::new();
        for (tool, wanted) in [(BuildTool::Maven, has_maven), (BuildTool::Gradle, has_gradle || keep_gradle)] {
            let registered = self.build_tools.tool_ref(tool).registration;
            match (wanted, registered) {
                (true, None) => register.push(tool),
                (false, Some(registration)) => {
                    unregister.push(registration);
                    let slot = self.build_tools.tool(tool);
                    slot.registration = None;
                    slot.view = None;
                    slot._view_events = None;
                    if let super::Content::Project { panels, .. } = &mut self.content {
                        panels.remove(tool_id(tool));
                    }
                }
                _ => {}
            }
        }
        self.register_build_tool_windows(register, unregister, window, cx);
    }

    fn register_build_tool_windows(
        &mut self,
        register: Vec<BuildTool>,
        unregister: Vec<Registration>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if register.is_empty() && unregister.is_empty() {
            return;
        }
        // A window's view exists before its panel is mounted, so the toolbar actions reach it.
        for tool in &register {
            self.ensure_build_tool_view(*tool, window, cx);
        }
        let mut registered = Vec::new();
        self.with_layout(window, cx, |layout| {
            let mut effects = Effects::default();
            for registration in unregister {
                merge(&mut effects, layout.unregister(registration));
            }
            for tool in register {
                if let Some(declared) = tool_window_declaration(tool_id(tool)) {
                    let (registration, more) = layout.register(declared);
                    merge(&mut effects, more);
                    registered.push((tool, registration));
                }
            }
            effects
        });
        for (tool, registration) in registered {
            self.build_tools.tool(tool).registration = Some(registration);
        }
    }

    fn ensure_build_tool_view(&mut self, tool: BuildTool, window: &mut Window, cx: &mut Context<Self>) {
        if self.build_tools.tool_ref(tool).view.is_some() {
            return;
        }
        let view = cx.new(|cx| BuildToolView::new(tool, cx));
        let events = cx.subscribe_in(&view, window, move |workbench, _, event: &BuildToolEvent, window, cx| {
            workbench.build_tool_event(tool, event.clone(), window, cx);
        });
        let slot = self.build_tools.tool(tool);
        slot.view = Some(view);
        slot._view_events = Some(events);
        self.feed_build_tool_views(cx);
    }

    /// The panel of the Maven or Gradle tool window; `None` for another id or a tool window the
    /// project does not have.
    pub(super) fn mount_build_tool(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) -> Option<Panel> {
        let tool = tool_of_window(id)?;
        self.build_tools.tool_ref(tool).registration?;
        self.ensure_build_tool_view(tool, window, cx);
        let view = self.build_tools.tool_ref(tool).view.clone()?;
        Some(Panel::new(view, None))
    }

    fn feed_build_tool_views(&mut self, cx: &mut Context<Self>) {
        let slot = &self.build_tools;
        for tool in [BuildTool::Maven, BuildTool::Gradle] {
            let Some(view) = slot.tool_ref(tool).view.clone() else { continue };
            let input = BuildToolInput {
                root: slot.root.clone().unwrap_or_default(),
                model: slot.model.clone(),
                configs: slot.configs.clone(),
                settings: slot.settings.clone(),
                sync: slot.tool_ref(tool).sync.as_ref().map(|sync| sync.banner.clone()),
                loading: slot.loading,
                error: slot.error.clone(),
            };
            view.update(cx, |view, cx| view.set_input(input, cx));
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Toolbar toggles and profiles

    fn publish_build_toggles(&self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = &self.build_tools.settings;
        set_context(window, cx, MAVEN_OFFLINE_KEY, settings.maven.offline);
        set_context(window, cx, MAVEN_SKIP_TESTS_KEY, settings.maven.skip_tests);
        set_context(window, cx, GRADLE_OFFLINE_KEY, settings.gradle.offline);
    }

    /// Keeps `settings` for the project and applies them to the trees and the next runs.
    fn set_build_settings(&mut self, settings: BuildSettings, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.build_tools.root.clone() else { return };
        cx.default_global::<UiStorage>().set(&format!("{SETTINGS_KEY}{root}"), settings.to_json());
        self.build_tools.settings = settings;
        self.publish_build_toggles(window, cx);
        self.feed_build_tool_views(cx);
    }

    // ---------------------------------------------------------------------------------------------
    // Sync

    /// Reload: now, or right after the sync in progress (which used the previous options).
    fn request_build_sync(
        &mut self,
        tool: BuildTool,
        build_dir: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = self.build_tools.tool(tool);
        if slot.sync.is_some() {
            if !slot.queued.contains(&build_dir) {
                slot.queued.push_back(build_dir);
            }
            return;
        }
        self.sync_build_tool(tool, build_dir, window, cx);
    }

    /// Reloads one build (`build_dir`) or all of `tool`'s with the build tool itself, in the
    /// background, with its progress in the banner and the status bar; a sync already running
    /// starts nothing.
    fn sync_build_tool(
        &mut self,
        tool: BuildTool,
        build_dir: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let slot = &self.build_tools;
        let (Some(handle), Some(_)) = (slot.window, slot.root.as_ref()) else { return };
        if slot.tool_ref(tool).sync.is_some() {
            return;
        }
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let request = Backend::next_request_id();
        let (offline, profiles) = match tool {
            BuildTool::Maven => {
                let profiles = slot
                    .model
                    .as_ref()
                    .and_then(|m| m.maven.as_ref())
                    .map(|m| {
                        let own = m.profiles.iter().filter(|p| p.project_root == build_dir);
                        profiles_list(own, &slot.settings.maven.profiles)
                    })
                    .unwrap_or_default();
                (slot.settings.maven.offline, profiles)
            }
            BuildTool::Gradle => (slot.settings.gradle.offline, Vec::new()),
        };
        let (sender, mut progress) = unbounded::<SyncProgress>();
        let channel = ide_host::ops::Channel::new(move |step| {
            // The receiver is gone only when the window is.
            let _ = sender.unbounded_send(step);
        });
        let label = self.label.clone();
        let syncing = backend.with_host(move |host| async move {
            ide_host::ops::build::build_sync(&host, &label, tool, request, offline, profiles, build_dir, channel).await
        });
        let epoch = slot.epoch;
        let task = cx.spawn(async move |this, cx| {
            let mut syncing = Box::pin(futures::FutureExt::fuse(syncing));
            let outcome = loop {
                futures::select! {
                    step = progress.next() => match step {
                        Some(step) => {
                            if this.update(cx, |w, cx| w.build_sync_progress(tool, request, step, cx)).is_err() {
                                return;
                            }
                        }
                        None => break syncing.await,
                    },
                    outcome = syncing => break outcome,
                }
            };
            let outcome = flatten(outcome);
            let _ = handle.update(cx, |_, window, cx| {
                this.update(cx, |w, cx| w.build_sync_finished(tool, request, epoch, outcome, window, cx))
            });
        });
        let title = sync_title(tool);
        let banner = SyncBanner { message: title.to_owned(), fraction: None };
        self.build_tools.tool(tool).sync = Some(RunningSync { request, banner, _task: task });
        let id = TaskId::Name(format!("{SYNC_TASK_PREFIX}{}", tool_id(tool)));
        let update = TaskUpdate { cancel: Change::Set(Cancel::Producer), ..TaskUpdate::new(id, title) };
        self.tasks.upsert_task(update, now_ms());
        self.schedule_tasks(cx);
        self.feed_build_tool_views(cx);
        self.publish_build_toggles(window, cx);
    }

    fn build_sync_progress(&mut self, tool: BuildTool, request: RequestId, step: SyncProgress, cx: &mut Context<Self>) {
        let Some(sync) = self.build_tools.tool(tool).sync.as_mut().filter(|sync| sync.request == request) else {
            return;
        };
        sync.banner = SyncBanner { message: step.message.clone(), fraction: step.fraction };
        let id = TaskId::Name(format!("{SYNC_TASK_PREFIX}{}", tool_id(tool)));
        let update = TaskUpdate {
            message: Some(step.message),
            fraction: step.fraction.map(f64::from),
            cancel: Change::Set(Cancel::Producer),
            ..TaskUpdate::new(id, sync_title(tool))
        };
        self.tasks.upsert_task(update, now_ms());
        self.schedule_tasks(cx);
        self.feed_build_tool_views(cx);
    }

    fn build_sync_finished(
        &mut self,
        tool: BuildTool,
        request: RequestId,
        epoch: u64,
        outcome: IdeResult<BuildModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.build_tools.epoch != epoch
            || self.build_tools.tool_ref(tool).sync.as_ref().is_none_or(|sync| sync.request != request)
        {
            return;
        }
        let id = TaskId::Name(format!("{SYNC_TASK_PREFIX}{}", tool_id(tool)));
        let done = TaskUpdate {
            fraction: Some(1.0),
            done: true,
            cancel: Change::Clear,
            ..TaskUpdate::new(id, sync_title(tool))
        };
        self.tasks.upsert_task(done, now_ms());
        self.schedule_tasks(cx);
        let failed = match outcome {
            Ok(model) => {
                let error = match tool {
                    BuildTool::Maven => model.maven.as_ref().and_then(|m| m.sync_error.clone()).unwrap_or_default(),
                    BuildTool::Gradle => {
                        let errors: Vec<&str> = model.gradle.iter().filter_map(|g| g.sync_error.as_deref()).collect();
                        errors.join("\n")
                    }
                };
                self.build_tools.model = Some(model);
                // A model read before the sync ended is older than what the sync returned.
                self.build_tools.load_seq += 1;
                self.build_tools.loading = false;
                (!error.is_empty()).then(|| {
                    let title = match tool {
                        BuildTool::Maven => "Maven sync finished with errors",
                        BuildTool::Gradle => "Gradle sync failed",
                    };
                    (NotificationLevel::Warning, title, error)
                })
            }
            Err(IdeError { kind: ErrorKind::Cancelled, .. }) => None,
            Err(error) => {
                let title = match tool {
                    BuildTool::Maven => "Cannot reload Maven projects",
                    BuildTool::Gradle => "Cannot reload Gradle projects",
                };
                Some((NotificationLevel::Error, title, error.message))
            }
        };
        if let Some((level, title, error)) = failed {
            let (body, actions) = recovery(&error);
            let mut request = NotificationRequest::new(level, title);
            request.body = Some(body);
            request.actions = actions;
            request.group = Some("run".to_owned());
            request.key = Some(format!("build-sync-{}", tool_id(tool)));
            self.notify(request, cx);
        }
        let slot = self.build_tools.tool(tool);
        slot.sync = None;
        let next = slot.queued.pop_front();
        self.build_model_changed(window, cx);
        if let Some(next) = next {
            self.sync_build_tool(tool, next, window, cx);
        }
    }

    /// Stop: the queued reloads are dropped and the running sync is cancelled; it ends without a
    /// notification.
    fn cancel_build_sync(&mut self, tool: BuildTool, cx: &mut Context<Self>) {
        let slot = self.build_tools.tool(tool);
        slot.queued.clear();
        let Some(request) = slot.sync.as_ref().map(|sync| sync.request) else { return };
        if let Some(backend) = cx.try_global::<Backend>() {
            backend.host.cancel_request(request);
        }
    }

    /// Cancel on a sync's status bar task; `false` when `name` is not a sync's task.
    pub(super) fn cancel_build_sync_task(&mut self, name: &str, cx: &mut Context<Self>) -> bool {
        let tool = match name.strip_prefix(SYNC_TASK_PREFIX) {
            Some(MAVEN_TOOL_WINDOW) => BuildTool::Maven,
            Some(GRADLE_TOOL_WINDOW) => BuildTool::Gradle,
            _ => return false,
        };
        self.cancel_build_sync(tool, cx);
        true
    }

    // ---------------------------------------------------------------------------------------------
    // Linking Gradle builds

    /// Shows the Gradle tool window, registering it when the project has no Gradle build yet.
    fn show_gradle_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.build_tools.gradle.registration.is_none() {
            self.register_build_tool_windows(vec![BuildTool::Gradle], Vec::new(), window, cx);
        }
        self.with_layout(window, cx, |layout| layout.show(GRADLE_TOOL_WINDOW, true));
    }

    /// Link Gradle Projects: the picked build files' builds are linked without changing their
    /// scripts. A project in Restricted Mode links nothing.
    fn link_gradle_projects(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.build_tools.root.is_none() || self.project_restricted(cx) {
            return;
        }
        self.show_gradle_window(window, cx);
        let epoch = self.build_tools.epoch;
        let label = self.label.clone();
        let picked =
            cx.prompt_for_paths(PathPromptOptions { files: true, directories: false, multiple: true, prompt: None });
        cx.spawn_in(window, async move |this, cx| {
            let paths: Vec<String> = match picked.await {
                Ok(Ok(Some(paths))) => paths.into_iter().map(|path| path.to_string_lossy().into_owned()).collect(),
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!("link Gradle projects: no build file is picked: {error:#}");
                    return;
                }
            };
            if paths.is_empty() {
                return;
            }
            let linking = this.update(cx, |_, cx| {
                cx.try_global::<Backend>().map(|backend| {
                    backend.with_host(move |host| async move {
                        ide_host::ops::build::gradle_link_projects(&host, &label, paths).await
                    })
                })
            });
            let Ok(Some(linking)) = linking else { return };
            let outcome = flatten(linking.await);
            let _ = this.update_in(cx, |workbench, window, cx| workbench.gradle_linked(epoch, outcome, window, cx));
        })
        .detach();
    }

    fn gradle_linked(
        &mut self,
        epoch: u64,
        outcome: IdeResult<BuildModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.build_tools.epoch != epoch {
            return;
        }
        match outcome {
            Ok(model) => {
                let opened = initial_expanded(BuildTool::Gradle, &model);
                self.build_tools.model = Some(model);
                self.build_tools.load_seq += 1;
                self.build_tools.loading = false;
                self.build_model_changed(window, cx);
                if let Some(view) = self.build_tool_view(BuildTool::Gradle) {
                    view.update(cx, |view, cx| view.open_keys(opened, cx));
                }
            }
            Err(error) => {
                let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot link Gradle projects");
                request.body = Some(error.message);
                request.group = Some("run".to_owned());
                self.notify(request, cx);
            }
        }
    }

    /// Unlink Gradle Project on a linked build's root: after a confirmation, the build is no
    /// longer imported; its files and run configurations stay. A module or an unknown folder is
    /// left alone.
    fn unlink_gradle_project(&mut self, build_dir: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = build_dir else { return };
        let linked = self.build_tools.model.as_ref().is_some_and(|m| m.gradle.iter().any(|g| g.dir == dir));
        if !linked {
            return;
        }
        let request = ConfirmRequest {
            title: "Unlink Gradle project?".to_owned(),
            message: "Remove this build from Gradle? Files will remain in the Project view; link its build file again \
                      to restore it."
                .to_owned(),
            ok_label: "Unlink".to_owned(),
            danger: false,
        };
        let epoch = self.build_tools.epoch;
        let dialog = cx.new(|cx| PromptDialog::confirm(request, window, cx));
        let events = cx.subscribe_in(&dialog, window, move |workbench, _, event: &PromptDialogEvent, window, cx| {
            let PromptDialogEvent::Closed(answer) = event;
            workbench.close_vcs_dialog(window, cx);
            if answer.confirmed() {
                workbench.unlink_confirmed(dir.clone(), epoch, window, cx);
            }
        });
        let focus = dialog.focus_handle(cx);
        self.open_vcs_dialog(dialog.into(), focus, events, window, cx);
    }

    fn unlink_confirmed(&mut self, dir: String, epoch: u64, window: &mut Window, cx: &mut Context<Self>) {
        if self.build_tools.epoch != epoch {
            return;
        }
        self.cancel_build_sync(BuildTool::Gradle, cx);
        // A model being read now still has the build: its answer is dropped.
        self.build_tools.load_seq += 1;
        let Some(backend) = cx.try_global::<Backend>() else { return };
        let label = self.label.clone();
        let unlinking = backend.with_host(move |host| async move {
            ide_host::ops::build::gradle_unlink_projects(&host, &label, vec![dir]).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let outcome = flatten(unlinking.await);
            let _ = this.update_in(cx, |workbench, window, cx| {
                if workbench.build_tools.epoch != epoch {
                    return;
                }
                match outcome {
                    Ok(model) => {
                        workbench.build_tools.model = Some(model);
                        workbench.build_tools.loading = false;
                        workbench.build_tools.gradle.current = None;
                        workbench.build_model_changed(window, cx);
                    }
                    Err(error) => {
                        let mut request =
                            NotificationRequest::new(NotificationLevel::Error, "Cannot unlink Gradle project");
                        request.body = Some(error.message);
                        request.group = Some("run".to_owned());
                        workbench.notify(request, cx);
                    }
                }
            });
        })
        .detach();
    }

    // ---------------------------------------------------------------------------------------------
    // Execute Goal / Task

    /// Opens Execute Maven Goal / Execute Gradle Task in project `project` (else the tree's
    /// current one).
    fn open_execute(&mut self, tool: BuildTool, project: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.build_tools.root.clone() else { return };
        if let Some(project) = project {
            self.build_tools.tool(tool).current = Some(project);
        }
        let stored = cx.try_global::<UiStorage>().and_then(|storage| storage.get(&history_key(tool, &root)));
        let history = parse_history(stored);
        let model = self.build_tools.model.clone().unwrap_or_default();
        let current = self.build_tools.tool_ref(tool).current.clone();
        let popup = cx.new(|cx| ExecutePopup::new(tool, model, history, current.as_deref(), cx));
        let events =
            cx.subscribe_in(&popup, window, move |workbench, _, event: &ExecuteEvent, window, cx| match event {
                ExecuteEvent::Dismissed => workbench.close_popup(window, cx),
                ExecuteEvent::Execute { line, project } => {
                    if workbench.execute_build_line(tool, line, project.as_deref(), window, cx) {
                        workbench.close_popup(window, cx);
                    }
                }
            });
        self.open_popup(&popup, events, window, cx);
    }

    /// Runs the goals / tasks of `text` in `project`; `false` when there is nothing to run.
    fn execute_build_line(
        &mut self,
        tool: BuildTool,
        text: &str,
        project: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(root) = self.build_tools.root.clone() else { return false };
        let Some(model) = self.build_tools.model.as_ref() else { return false };
        let project = project.map(str::to_owned).or_else(|| self.build_tools.tool_ref(tool).current.clone());
        let Some(config) = execute_config(tool, text, project.as_deref(), model, &self.build_tools.settings) else {
            return false;
        };
        let key = history_key(tool, &root);
        let storage = cx.default_global::<UiStorage>();
        let history = pushed_history(&parse_history(storage.get(&key)), &history_line(tool, text));
        storage.set(&key, serde_json::to_string(&history).unwrap_or_default());
        self.run_build_config(config, false, window, cx);
        true
    }

    // ---------------------------------------------------------------------------------------------
    // The views' requests

    /// Starts `config` as a temporary configuration through the Run window.
    fn run_build_config(&mut self, config: RunConfig, debug: bool, window: &mut Window, cx: &mut Context<Self>) {
        let config = match serde_json::to_value(&config) {
            Ok(config) => config,
            Err(error) => {
                tracing::warn!("build tools: {} cannot be started: {error}", config.name);
                return;
            }
        };
        let action = if debug { "run.debugConfig" } else { "run.runConfig" };
        run_action_later(action.to_owned(), Some(json!({ "config": config, "temporary": true })), window, cx);
    }

    fn build_tool_event(
        &mut self,
        tool: BuildTool,
        event: BuildToolEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            BuildToolEvent::RunAction { id, arg } => run_action_later(id.to_owned(), arg, window, cx),
            BuildToolEvent::Run { config, debug } => self.run_build_config(config, debug, window, cx),
            BuildToolEvent::ShowMenu { key, position } => {
                let Some(view) = self.build_tool_view(tool) else { return };
                let items = view.read(cx).context_menu(&key, cx);
                if !items.is_empty() {
                    self.show_menu(
                        &items,
                        position,
                        |w, command, window, cx| w.run_build_menu_command(command, window, cx),
                        window,
                        cx,
                    );
                }
            }
            BuildToolEvent::ToggleProfile { profile, checked } => {
                let mut settings = self.build_tools.settings.clone();
                settings.maven.profiles.insert(profile_key(&profile), checked);
                self.set_build_settings(settings, window, cx);
            }
            BuildToolEvent::CurrentProject(project) => self.build_tools.tool(tool).current = project,
            BuildToolEvent::LoadChanges { build_dir } => self.sync_build_tool(tool, build_dir, window, cx),
        }
    }

    /// Carries out a row of a tree's context menu.
    fn run_build_menu_command(&mut self, command: BuildMenuCommand, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            BuildMenuCommand::Run(config) => self.run_build_config(config, false, window, cx),
            BuildMenuCommand::Debug(config) => self.run_build_config(config, true, window, cx),
            BuildMenuCommand::CreateConfig(config) => self.create_config(config, window, cx),
            BuildMenuCommand::OpenFile(path) => self.open_project_file(PathBuf::from(path), window, cx),
            BuildMenuCommand::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::RUN_TOOL_WINDOW;
    use super::super::tests::{click_on, notification_titles, run_action, start, wait_long};
    use super::*;
    use crate::build_tools::execute::ExecuteRow;
    use crate::build_tools::model::NodeKind;
    use crate::build_tools::view::{Banner, BuildRow};
    use crate::run::view::RunView;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    /// A stand-in for Maven: it leaves `ran` beside it and says what it was started with (`cwd`,
    /// then one `arg` line per argument). Run with the sync's goals (`help:effective-pom`), it reports building `core`,
    /// waits while `hold` exists beside it, fails when `sync-fails` does, and else prints the
    /// dependency tree of `demo:core`. It reaches no network and no Maven installation.
    const FAKE_MVN: &str = r#"#!/bin/sh
D="$(cd "$(dirname "$0")" && pwd -P)"
touch "$D/ran"
echo "stand-in mvn"
echo "cwd $(pwd -P)"
for a in "$@"; do echo "arg $a"; done
case " $* " in
  *" help:effective-pom "*) ;;
  *) exit 0 ;;
esac
echo "[INFO] Building core 1.0 [1/2]"
while [ -e "$D/hold" ]; do sleep 0.05; done
if [ -e "$D/sync-fails" ]; then
  echo "[ERROR] Failed to execute goal on project core: the stand-in refuses"
  exit 1
fi
cat <<'EOF'
[INFO] --- maven-dependency-plugin:3.8.1:tree (default-cli) @ core ---
[INFO] demo:core:jar:1.0
[INFO] \- org.junit.jupiter:junit-jupiter:jar:5.10.2:test
[INFO] BUILD SUCCESS
EOF
"#;

    /// A stand-in for the Gradle wrapper of a build: like the Maven one, it says what it was
    /// started with. Run with the sync's `tasks --all`, it reports the build's projects as the
    /// IDE's init script would and lists one task of each; it waits while `hold` exists beside it
    /// and fails as Gradle does when `sync-fails` does.
    const FAKE_GRADLEW: &str = r#"#!/bin/sh
D="$(cd "$(dirname "$0")" && pwd -P)"
echo "stand-in gradle"
echo "cwd $(pwd -P)"
for a in "$@"; do echo "arg $a"; done
case " $* " in
  *" tasks "*) ;;
  *) exit 0 ;;
esac
while [ -e "$D/hold" ]; do sleep 0.05; done
if [ -e "$D/sync-fails" ]; then
  printf 'FAILURE: Build failed with an exception.\n\n* What went wrong:\nA problem occurred evaluating root project.\n> the stand-in refuses\n'
  exit 1
fi
printf '@openidea-model\t{"path":":","dir":"%s"}\n' "$D"
printf '@openidea-model\t{"path":":app","dir":"%s/app"}\n' "$D"
cat <<'EOF'
Build tasks
-----------
assemble - Assembles the outputs of this project.
app:customJar - A jar the stand-in adds.

EOF
"#;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder is made");
        std::fs::write(path, text).expect("the file is written");
    }

    fn executable(path: &Path, text: &str) {
        write(path, text);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("the script runs");
    }

    fn pom(artifact: &str, body: &str) -> String {
        format!(
            "<project><modelVersion>4.0.0</modelVersion>{body}<groupId>demo</groupId><artifactId>{artifact}</artifactId>\
             <version>1.0</version></project>"
        )
    }

    /// A Maven reactor `Multi Demo` with the modules `core` and `Demo App`, whose Maven is the
    /// stand-in in `tools`, with the user settings and the local repository there too: nothing is
    /// read from the user's home.
    fn maven_project(root: &Path, tools: &Path) {
        let modules = "<packaging>pom</packaging><name>Multi Demo</name><modules><module>core</module><module>app</module></modules>";
        write(&root.join("pom.xml"), &pom("parent", modules));
        let parent = "<parent><groupId>demo</groupId><artifactId>parent</artifactId><version>1.0</version></parent>";
        write(&root.join("core/pom.xml"), &pom("core", &format!("{parent}<packaging>jar</packaging>")));
        write(
            &root.join("app/pom.xml"),
            &pom("app", &format!("{parent}<packaging>jar</packaging><name>Demo App</name>")),
        );
        executable(&tools.join("mvn"), FAKE_MVN);
        write(&tools.join("settings.xml"), "<settings/>");
        std::fs::create_dir_all(tools.join("m2")).expect("the repository folder is made");
        let settings = format!(
            "[languages.java.maven]\nhome = \"{mvn}\"\nuserSettings = \"{settings}\"\nlocalRepository = \"{repo}\"\n",
            mvn = tools.join("mvn").display(),
            settings = tools.join("settings.xml").display(),
            repo = tools.join("m2").display(),
        );
        write(&root.join(".openidea/settings.toml"), &settings);
    }

    /// A Gradle build `demo` including `app` (a Java project), whose wrapper is the stand-in.
    fn gradle_project(root: &Path) {
        write(&root.join("settings.gradle"), "rootProject.name = 'demo'\ninclude 'app'\n");
        write(&root.join("build.gradle"), "// the root build\n");
        write(&root.join("app/build.gradle"), "plugins { id 'java' }\n");
        executable(&root.join("gradlew"), FAKE_GRADLEW);
    }

    /// The project `fill` writes, open in a window (trusted unless `trusted` is false), once its
    /// build model is answered.
    fn build_project(
        cx: &mut TestAppContext,
        trusted: bool,
        fill: impl FnOnce(&Path, &Path),
    ) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 3], PathBuf, PathBuf) {
        let home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let tools_folder = tempfile::tempdir().expect("a temporary folder");
        let tools = tools_folder.path().canonicalize().expect("the folder exists");
        fill(&root, &tools);
        if trusted {
            cx.update(|cx| cx.global::<Backend>().host.trust.decide(&root, true, false))
                .expect("the project is trusted");
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        cx.update(|window, _| window.activate_window());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        wait_long(cx, "the project", |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        wait_long(cx, "the build model", |cx| workbench.read_with(cx, |w, _| w.build_model_answered()));
        (workbench, cx, [home, folder, tools_folder], root, tools)
    }

    fn close(cx: &mut VisualTestContext) {
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// Shows tool window `id` through its action and returns its view once it shows the model.
    fn show(workbench: &Entity<Workbench>, tool: BuildTool, cx: &mut VisualTestContext) -> Entity<BuildToolView> {
        run_action(cx, &format!("toolwindow.{}", tool_id(tool)), None);
        wait_long(cx, "the tool window", |cx| workbench.read_with(cx, |w, _| w.is_tool_window_visible(tool_id(tool))));
        let view = workbench.read_with(cx, |w, _| w.build_tool_view(tool)).expect("the tool window has its view");
        wait_long(cx, "the tree", |cx| view.read_with(cx, |view, _| !view.rows().is_empty()));
        view
    }

    fn rows(view: &Entity<BuildToolView>, cx: &mut VisualTestContext) -> Vec<BuildRow> {
        view.read_with(cx, |view, _| view.rows())
    }

    fn labels(view: &Entity<BuildToolView>, cx: &mut VisualTestContext) -> Vec<String> {
        rows(view, cx).into_iter().map(|row| row.label).collect()
    }

    fn click_row(key: &str, cx: &mut VisualTestContext) {
        click_on(cx, Box::leak(format!("build-tool-row:{key}").into_boxed_str()));
    }

    /// The lines the Run window shows for its selected session.
    fn run_lines(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        let view = workbench
            .read_with(cx, |w, _| w.tool_window_view(RUN_TOOL_WINDOW))
            .and_then(|view| view.downcast::<RunView>().ok());
        view.map(|view| view.read_with(cx, |view, cx| view.lines(cx))).unwrap_or_default()
    }

    /// Waits until the run of configuration `name` has ended.
    fn wait_for_run(workbench: &Entity<Workbench>, name: &str, cx: &mut VisualTestContext) {
        let store = workbench
            .read_with(cx, |w, cx| w.run_driver().map(|driver| driver.read(cx).store().clone()))
            .expect("the project runs things");
        wait_long(cx, "the run to end", |cx| {
            store.read_with(cx, |store, _| store.sessions.iter().any(|s| s.config.name == name && !s.is_live()))
        });
        wait_long(cx, "its last line", |cx| run_lines(workbench, cx).iter().any(|line| line.starts_with("cwd ")));
    }

    fn args_of(lines: &[String]) -> Vec<String> {
        lines.iter().filter_map(|line| line.strip_prefix("arg ")).map(str::to_owned).collect()
    }

    fn notice_body(workbench: &Entity<Workbench>, title: &str, cx: &mut VisualTestContext) -> Option<String> {
        workbench.read_with(cx, |w, _| {
            w.notifications().items().iter().find(|n| n.title == title).and_then(|n| n.body.clone())
        })
    }

    /// The Maven tool window is registered while the project has a POM, shows the reactor as the
    /// POMs describe it without running Maven, and opens a project's pom.xml from its menu; a
    /// lifecycle phase selected in the tree runs through the Run window with Skip Tests applied,
    /// with the project's Maven, in the module's folder.
    #[gpui_kit::test]
    async fn runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_KEY)));
        assert!(!cx.update(|window, cx| window_context(window, cx).is_truthy(GRADLE_KEY)));
        assert!(workbench.read_with(cx, |w, _| w.build_tool_view(BuildTool::Gradle).is_none()), "no Gradle window");
        let view = show(&workbench, BuildTool::Maven, cx);
        // Detected run configurations of the reactor, if any, come between Plugins and the modules.
        let shown = labels(&view, cx);
        assert_eq!(shown[..3], ["Multi Demo", "Lifecycle", "Plugins"]);
        assert_eq!(shown[shown.len() - 2..], ["core", "Demo App"]);
        let buttons = view.read_with(cx, |view, cx| view.toolbar(cx));
        let actions: Vec<Option<&str>> = buttons.iter().map(|button| button.action).collect();
        assert_eq!(
            actions,
            [
                Some("maven.reload"),
                None,
                Some("maven.executeGoal"),
                Some("maven.toggleOffline"),
                Some("maven.toggleSkipTests"),
                Some("maven.expandAll"),
                Some("maven.collapseAll"),
                Some("maven.settings"),
            ]
        );
        assert!(!buttons[1].enabled, "Run waits for a runnable selection");

        // Open pom.xml from the project's menu.
        let menu = view.read_with(cx, |view, cx| view.context_menu("mvn:pom.xml", cx));
        let open =
            menu.iter().find(|item| item.label() == Some("Open pom.xml")).and_then(|item| item.command().cloned());
        let open = open.expect("the menu opens the POM");
        workbench.update_in(cx, |w, window, cx| w.run_build_menu_command(open, window, cx));
        wait_long(cx, "the POM's editor", |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).path() == root.join("pom.xml")))
        });

        run_action(cx, "maven.toggleSkipTests", None);
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_SKIP_TESTS_KEY)));
        let stored = cx.update(|_, cx| {
            cx.global::<UiStorage>().get(&format!("{SETTINGS_KEY}{}", root.display())).map(str::to_owned)
        });
        assert!(
            stored.is_some_and(|stored| stored.contains(r#""skipTests":true"#)),
            "the toggle is kept for the project"
        );
        view.update(cx, |view, cx| {
            view.toggle("mvn:core/pom.xml", cx);
            view.toggle("mvn:core/pom.xml:lifecycle", cx);
        });
        let test = rows(&view, cx).into_iter().find(|row| row.node.key == "mvn:core/pom.xml:lifecycle:test");
        assert!(test.expect("core's test phase is shown").skipped, "Skip Tests strikes the test phase");
        click_row("mvn:core/pom.xml:lifecycle:test", cx);
        assert!(view.read_with(cx, |view, cx| view.toolbar(cx))[1].enabled);
        click_on(cx, "build-tool-button:run");
        wait_for_run(&workbench, "core [test]", cx);
        let lines = run_lines(&workbench, cx);
        assert!(lines.contains(&"stand-in mvn".to_owned()), "{lines:?}");
        assert!(lines.contains(&format!("cwd {}", root.join("core").display())), "{lines:?}");
        let args = args_of(&lines);
        assert_eq!(args[args.len() - 2..], ["test", "-DskipTests"], "{args:?}");

        run_action(cx, "maven.expandAll", None);
        assert!(view.read_with(cx, |view, _| view.is_expanded("mvn:pom.xml:plugins")), "Expand All opens the folders");
        run_action(cx, "maven.collapseAll", None);
        assert_eq!(labels(&view, cx), ["Multi Demo"]);

        run_action(cx, "maven.settings", None);
        let page =
            workbench.read_with(cx, |w, cx| w.settings_dialog().and_then(|dialog| dialog.read(cx).selected_page(cx)));
        assert_eq!(page.as_deref(), Some("build.maven"), "Maven Settings… opens Settings › Maven");
        close(cx);
    }

    /// Reload runs Maven's sync goals with the stand-in, shows its progress in the banner and the
    /// status bar, and the tree then shows the dependencies Maven resolved; a failing Maven is told.
    #[gpui_kit::test]
    async fn reload_reads_the_dependency_trees_with_maven_and_refreshes_the_tree(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root, tools) = build_project(cx, true, maven_project);
        let view = show(&workbench, BuildTool::Maven, cx);
        write(&tools.join("hold"), "");
        run_action(cx, "maven.reload", None);
        wait_long(cx, "the sync's progress", |cx| {
            view.read_with(cx, |view, _| view.banners())
                == [Banner::Syncing { message: "Building model of core 1.0".to_owned(), percent: Some(0) }]
        });
        let buttons = view.read_with(cx, |view, cx| view.toolbar(cx));
        assert_eq!(buttons[0].action, Some("maven.cancelReload"), "Stop replaces Reload while it runs");
        let task = workbench.read_with(cx, |w, _| {
            w.tasks.tasks().iter().find(|t| t.id == TaskId::Name("build-sync:maven".to_owned())).cloned()
        });
        assert_eq!(task.and_then(|t| t.message).as_deref(), Some("Building model of core 1.0"));
        std::fs::remove_file(tools.join("hold")).expect("the stand-in goes on");
        wait_long(cx, "the end of the sync", |cx| {
            workbench.read_with(cx, |w, _| !w.build_sync_running(BuildTool::Maven))
        });
        assert!(view.read_with(cx, |view, _| view.banners()).is_empty(), "the build is up to date");
        view.update(cx, |view, cx| view.toggle("mvn:core/pom.xml", cx));
        view.update(cx, |view, cx| view.toggle("mvn:core/pom.xml:deps", cx));
        let dependency = rows(&view, cx).into_iter().find(|row| matches!(row.node.kind, NodeKind::Dependency(_)));
        let dependency = dependency.expect("core's resolved dependency is shown");
        assert_eq!(dependency.label, "org.junit.jupiter:junit-jupiter:5.10.2");
        assert_eq!(dependency.detail.as_deref(), Some("(test)"));
        assert!(!notification_titles(&workbench, cx).iter().any(|t| t.contains("Maven")), "nothing went wrong");

        // Maven failing: its errors are told, and the first one stays above the tree.
        write(&tools.join("sync-fails"), "");
        run_action(cx, "maven.reload", None);
        wait_long(cx, "the failure", |cx| notice_body(&workbench, "Maven sync finished with errors", cx).is_some());
        let body = notice_body(&workbench, "Maven sync finished with errors", cx).expect("the failure is told");
        assert_eq!(body, "Failed to execute goal on project core: the stand-in refuses");
        let banners = view.read_with(cx, |view, _| view.banners());
        assert!(
            matches!(banners.as_slice(), [Banner::Failed { line, .. }] if line.starts_with("Failed to execute goal")),
            "{banners:?}"
        );
        close(cx);
    }

    /// Stop while Maven syncs kills it: the sync ends without a notification and nothing it would
    /// have read is kept.
    #[gpui_kit::test]
    async fn stop_cancels_a_running_sync_without_an_error(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, _root, tools) = build_project(cx, true, maven_project);
        let view = show(&workbench, BuildTool::Maven, cx);
        write(&tools.join("hold"), "");
        run_action(cx, "maven.reload", None);
        wait_long(cx, "the sync's progress", |cx| {
            view.read_with(cx, |view, _| matches!(view.banners().as_slice(), [Banner::Syncing { message, .. }] if message.starts_with("Building")))
        });
        run_action(cx, "maven.cancelReload", None);
        wait_long(cx, "the end of the sync", |cx| {
            workbench.read_with(cx, |w, _| !w.build_sync_running(BuildTool::Maven))
        });
        let synced =
            workbench.read_with(cx, |w, _| w.build_model().and_then(|m| m.maven.as_ref()).and_then(|m| m.synced_at));
        assert_eq!(synced, None, "a stopped sync keeps nothing");
        let titles = notification_titles(&workbench, cx);
        assert!(!titles.iter().any(|t| t.contains("Maven")), "{titles:?}");
        assert_eq!(view.read_with(cx, |view, cx| view.toolbar(cx))[0].action, Some("maven.reload"));
        std::fs::remove_file(tools.join("hold")).expect("the marker is removed");
        close(cx);
    }

    /// The Gradle tool window shows the build as its scripts describe it; Reload runs the
    /// wrapper's `tasks --all`, and the tree then lists the tasks Gradle reported. A failed sync is
    /// told with Gradle's error, and its first line stays above the tree.
    #[gpui_kit::test]
    async fn gradle_reload_lists_the_reported_tasks_and_a_failed_sync_is_told(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, _tools) = build_project(cx, true, |root, _| gradle_project(root));
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(GRADLE_KEY)));
        let view = show(&workbench, BuildTool::Gradle, cx);
        // Detected run configurations of the build, if any, come between Tasks and the subprojects.
        let shown = labels(&view, cx);
        assert_eq!(shown[..2], ["demo", "Tasks"]);
        assert_eq!(shown.last().map(String::as_str), Some("app"));
        assert!(!shown.contains(&"customJar".to_owned()), "nothing Gradle reports is shown before a sync");
        let actions: Vec<Option<&str>> =
            view.read_with(cx, |view, cx| view.toolbar(cx)).iter().map(|button| button.action).collect();
        assert_eq!(
            actions,
            [
                Some("gradle.reload"),
                Some("gradle.linkProjects"),
                None,
                Some("gradle.executeTask"),
                Some("gradle.toggleOffline"),
                Some("gradle.expandAll"),
                Some("gradle.collapseAll"),
            ]
        );
        run_action(cx, "gradle.reload", None);
        wait_long(cx, "the end of the sync", |cx| {
            workbench.read_with(cx, |w, _| {
                !w.build_sync_running(BuildTool::Gradle)
                    && w.build_model().is_some_and(|m| m.gradle.iter().all(|g| g.synced_at.is_some()))
            })
        });
        run_action(cx, "gradle.expandAll", None);
        let shown = labels(&view, cx);
        assert!(shown.contains(&"customJar".to_owned()), "the reported task: {shown:?}");
        assert!(shown.contains(&"assemble".to_owned()), "{shown:?}");
        assert!(view.read_with(cx, |view, _| view.banners()).is_empty());
        run_action(cx, "gradle.collapseAll", None);
        assert_eq!(labels(&view, cx), ["demo"]);

        // Stop while Gradle runs: the sync ends without a notification.
        write(&root.join("hold"), "");
        run_action(cx, "gradle.reload", None);
        wait_long(cx, "the sync's progress", |cx| {
            view.read_with(cx, |view, _| view.banners())
                == [Banner::Syncing { message: "Loading Gradle tasks".to_owned(), percent: None }]
        });
        run_action(cx, "gradle.cancelReload", None);
        wait_long(cx, "the stop", |cx| workbench.read_with(cx, |w, _| !w.build_sync_running(BuildTool::Gradle)));
        assert!(!notification_titles(&workbench, cx).iter().any(|t| t.contains("Gradle")));
        std::fs::remove_file(root.join("hold")).expect("the marker is removed");

        write(&root.join("sync-fails"), "");
        run_action(cx, "gradle.reload", None);
        wait_long(cx, "the failure", |cx| notice_body(&workbench, "Gradle sync failed", cx).is_some());
        let body = notice_body(&workbench, "Gradle sync failed", cx).expect("the failure is told");
        assert!(body.starts_with("FAILURE: Build failed with an exception."), "{body}");
        assert!(body.contains("> the stand-in refuses"), "{body}");
        let banners = view.read_with(cx, |view, _| view.banners());
        assert!(
            matches!(banners.as_slice(), [Banner::Failed { line, .. }] if line == "FAILURE: Build failed with an exception."),
            "{banners:?}"
        );
        close(cx);
    }

    /// A task chosen in the tree runs with the build's wrapper from its folder, by its path, with
    /// Offline applied. (A test task would also get the IDE's test reporting init script.)
    #[gpui_kit::test]
    async fn a_task_runs_by_its_path_with_the_builds_wrapper_and_offline(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, _tools) = build_project(cx, true, |root, _| gradle_project(root));
        let view = show(&workbench, BuildTool::Gradle, cx);
        run_action(cx, "gradle.toggleOffline", None);
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(GRADLE_OFFLINE_KEY)));
        view.update(cx, |view, cx| view.expand_all(cx));
        let assemble = rows(&view, cx).into_iter().find(|row| {
            matches!(&row.node.kind, NodeKind::Task { path, task, .. } if path == ":app" && task.name == "assemble")
        });
        let assemble = assemble.expect("app's assemble task is shown");
        view.update(cx, |view, cx| view.activate(&assemble.node.key, cx));
        wait_for_run(&workbench, "app [assemble]", cx);
        let lines = run_lines(&workbench, cx);
        assert!(lines.contains(&"stand-in gradle".to_owned()), "{lines:?}");
        assert!(lines.contains(&format!("cwd {}", root.display())), "{lines:?}");
        assert_eq!(args_of(&lines), [":app:assemble", "--offline"]);

        // Execute Gradle Task from app's menu runs the typed task in app.
        let app = rows(&view, cx).into_iter().find(|row| row.label == "app").expect("app is shown");
        let menu = view.read_with(cx, |view, cx| view.context_menu(&app.node.key, cx));
        let execute =
            menu.iter().find(|item| item.action_id() == Some("gradle.executeTask")).and_then(|i| i.arg().cloned());
        let execute = execute.expect("app's menu executes tasks in it");
        assert_eq!(execute, json!({ "projectKey": app.node.key }));
        run_action(cx, "gradle.executeTask", Some(execute));
        let popup = workbench.read_with(cx, |w, _| w.execute_popup()).expect("the popup is open");
        let project = popup.read_with(cx, |popup, _| popup.state().project().map(str::to_owned));
        assert_eq!(project.as_deref(), Some(app.node.key.as_str()));
        cx.simulate_input("jar");
        cx.simulate_keystrokes("enter");
        wait_for_run(&workbench, "app [jar]", cx);
        assert_eq!(args_of(&run_lines(&workbench, cx)), [":app:jar", "--offline"]);
        close(cx);
    }

    /// Execute Maven Goal runs the typed goals in the chosen project with Offline applied and keeps
    /// the line; opened again, the line is offered under Recent.
    #[gpui_kit::test]
    async fn execute_maven_goal_runs_the_typed_line_and_offers_it_again_as_recent(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, _tools) = build_project(cx, true, maven_project);
        run_action(cx, "maven.toggleOffline", None);
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_OFFLINE_KEY)));
        run_action(cx, "maven.executeGoal", None);
        let popup = workbench.read_with(cx, |w, _| w.execute_popup()).expect("the popup is open");
        assert_eq!(
            popup.read_with(cx, |popup, _| popup.state().project().map(str::to_owned)),
            Some("pom.xml".to_owned())
        );
        cx.simulate_input("clean");
        let rows = popup.read_with(cx, |popup, _| popup.rows());
        assert!(matches!(&rows[1], ExecuteRow::Completion(c) if c.label == "clean"), "{rows:?}");
        cx.simulate_keystrokes("enter");
        assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_none()), "running closes the popup");
        wait_for_run(&workbench, "Multi Demo [clean]", cx);
        let lines = run_lines(&workbench, cx);
        assert!(lines.contains(&format!("cwd {}", root.display())), "{lines:?}");
        let args = args_of(&lines);
        assert_eq!(args[args.len() - 2..], ["clean", "-o"], "Offline applies to the typed goals: {args:?}");

        run_action(cx, "maven.executeGoal", None);
        let popup = workbench.read_with(cx, |w, _| w.execute_popup()).expect("the popup is open again");
        let rows = popup.read_with(cx, |popup, _| popup.rows());
        assert_eq!(rows[0], ExecuteRow::Section { key: "recent", label: "Recent" });
        assert!(matches!(&rows[1], ExecuteRow::Completion(c) if c.line == "clean" && c.is_recent()));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(workbench.read_with(cx, |w, _| w.execute_popup().is_none()), "Escape closes it");
        close(cx);
    }

    /// Link Gradle Projects links the picked build next to the project's, Unlink on its root asks
    /// first and removes it (cancelled, or on a module, nothing changes), and Link stays offered.
    #[gpui_kit::test]
    async fn links_from_the_toolbar_and_keeps_link_available_after_unlinking_the_final_build(cx: &mut TestAppContext) {
        let (workbench, cx, _folders, root, _tools) = build_project(cx, true, |root, _| {
            gradle_project(root);
            write(&root.join("libs/other/settings.gradle"), "rootProject.name = 'other'\n");
            write(&root.join("libs/other/build.gradle"), "plugins { id 'java' }\n");
        });
        let view = show(&workbench, BuildTool::Gradle, cx);
        assert!(!labels(&view, cx).contains(&"other".to_owned()), "an independent build is not linked by itself");
        click_on(cx, "build-tool-button:gradle.linkProjects");
        wait_long(cx, "the build file is asked for", |cx| cx.did_prompt_for_paths());
        let picked = root.join("libs/other/settings.gradle");
        cx.simulate_path_prompt_response(move |_| Some(vec![picked.clone()]));
        wait_long(cx, "the linked build", |cx| labels(&view, cx).contains(&"other".to_owned()));
        let other = rows(&view, cx).into_iter().find(|row| row.label == "other").expect("the linked build's root");
        let NodeKind::GradleProject { build, .. } = &other.node.kind else { panic!("a Gradle project") };
        let build = build.clone();
        let menu = view.read_with(cx, |view, cx| view.context_menu(&other.node.key, cx));
        let unlink = menu.iter().find(|item| item.action_id() == Some("gradle.unlinkProjects"));
        assert_eq!(unlink.and_then(|item| item.arg()), Some(&json!({ "buildDir": build })));

        run_action(cx, "gradle.unlinkProjects", Some(json!({ "buildDir": format!("{build}/app") })));
        assert!(workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()), "a module is not unlinked");
        run_action(cx, "gradle.unlinkProjects", Some(json!({ "buildDir": build })));
        let dialog = workbench.read_with(cx, |w, _| w.vcs_prompt()).expect("the unlink is asked about");
        assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Unlink Gradle project?");
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(labels(&view, cx).contains(&"other".to_owned()), "a cancelled unlink changes nothing");
        run_action(cx, "gradle.unlinkProjects", Some(json!({ "buildDir": build })));
        cx.simulate_keystrokes("enter");
        wait_long(cx, "the unlinked build", |cx| !labels(&view, cx).contains(&"other".to_owned()));
        assert!(labels(&view, cx).contains(&"demo".to_owned()), "the project's own build stays");
        assert!(root.join("libs/other/settings.gradle").is_file(), "its files stay");

        // The final build unlinked, the window stays with Link.
        let demo = rows(&view, cx).into_iter().find(|row| row.label == "demo").expect("the project's build");
        let NodeKind::GradleProject { build: own, .. } = &demo.node.kind else { panic!("a Gradle project") };
        run_action(cx, "gradle.unlinkProjects", Some(json!({ "buildDir": own })));
        cx.simulate_keystrokes("enter");
        wait_long(cx, "no build is left", |cx| {
            workbench.read_with(cx, |w, _| w.build_model().is_some_and(|m| m.gradle.is_empty()))
        });
        assert!(rows(&view, cx).is_empty());
        assert!(root.join("settings.gradle").is_file(), "its files stay");
        assert!(!cx.update(|window, cx| window_context(window, cx).is_truthy(GRADLE_KEY)));
        let registered =
            workbench.read_with(cx, |w, _| w.tool_windows().is_some_and(|l| l.window(GRADLE_TOOL_WINDOW).is_some()));
        assert!(registered, "the Gradle window stays to link a build again");
        let buttons = view.read_with(cx, |view, cx| view.toolbar(cx));
        assert!(buttons.iter().any(|b| b.action == Some("gradle.linkProjects")), "Link stays offered");
        close(cx);
    }

    /// In Restricted Mode the backend refuses the build model and every sync: the project has no
    /// Maven tool window, Reload is not available, Link asks for nothing, and Maven never runs.
    #[gpui_kit::test]
    async fn does_not_link_or_start_gradle_imports_in_an_untrusted_project(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let (workbench, cx, _folders, _root, tools) = build_project(cx, false, maven_project);
        assert!(workbench.read_with(cx, |w, cx| w.project_restricted(cx)));
        assert_eq!(workbench.read_with(cx, |w, _| w.build_model().cloned()), None);
        let error = workbench.read_with(cx, |w, _| w.build_model_error().map(str::to_owned));
        assert!(error.is_some_and(|error| error.contains("Restricted Mode")));
        assert!(!cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_KEY)));
        let registered =
            workbench.read_with(cx, |w, _| w.tool_windows().is_some_and(|l| l.window(MAVEN_TOOL_WINDOW).is_some()));
        assert!(!registered, "no Maven tool window");
        let reload = cx.update(|window, cx| execute_action_or_throw("maven.reload", None, window, cx));
        assert!(reload.is_err(), "Reload is not available");
        run_action(cx, "gradle.linkProjects", None);
        assert!(!cx.did_prompt_for_paths(), "Link asks for no build file");

        let label = "w1".to_owned();
        let refused = cx.update(|_, cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                let channel = ide_host::ops::Channel::new(|_: SyncProgress| {});
                let request = Backend::next_request_id();
                ide_host::ops::build::build_sync(
                    &host,
                    &label,
                    BuildTool::Maven,
                    request,
                    false,
                    Vec::new(),
                    None,
                    channel,
                )
                .await
            })
        });
        let refused = flatten(futures::executor::block_on(refused));
        let message = refused.err().map(|error| error.message).unwrap_or_default();
        assert_eq!(message, "Project is in Restricted Mode. Trust the project to enable execution.");
        assert!(!tools.join("ran").exists(), "Maven never ran");

        // A goal started as a run configuration is refused too.
        let goal = RunConfig {
            command: "mvn".to_owned(),
            args: vec!["clean".to_owned()],
            ..crate::run::configs::blank_config("maven:goal:pom.xml:clean", "restricted [clean]", "maven")
        };
        let goal = serde_json::to_value(&goal).expect("a configuration is JSON");
        run_action(cx, "run.runConfig", Some(json!({ "config": goal, "temporary": true })));
        let store = workbench
            .read_with(cx, |w, cx| w.run_driver().map(|driver| driver.read(cx).store().clone()))
            .expect("the project runs things");
        wait_long(cx, "the refused start", |cx| {
            store.read_with(cx, |store, _| {
                store.sessions.iter().any(|s| s.config.name == "restricted [clean]" && !s.is_live())
            })
        });
        assert!(!tools.join("ran").exists(), "the goal did not run");

        // Trusted from the banner, the project's model is read and the Maven tool window comes.
        click_on(cx, "restricted-banner-trust");
        wait_long(cx, "the Maven model", |cx| {
            workbench.read_with(cx, |w, _| w.build_model().is_some_and(|m| m.maven.is_some()))
        });
        assert!(cx.update(|window, cx| window_context(window, cx).is_truthy(MAVEN_KEY)));
        assert!(!tools.join("ran").exists(), "reading the model runs no Maven");
        close(cx);
    }
}
