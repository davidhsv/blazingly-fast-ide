//! The window shell. For now: a frame around one editor (LightEdit), which is the part of the
//! workbench the editor slice needs. Tool windows, the title bar and the status bar follow.

pub mod main_toolbar;
pub mod notifications;
pub mod project_color;
pub mod quick_switch;
pub mod status_bar_widgets;
pub mod tasks;
pub mod tool_windows;

use crate::backend::{Backend, BackendEvent};
use crate::editor::session::{self, FileViewState};
use crate::editor::tabs::{EditorInput, OpenTabOptions, TabsState, disambiguate_titles};
use crate::editor::view::Editor;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry, ActionSource, execute_action_from, manifest};
use crate::platform::input;
use crate::platform::local_time::now_ms;
use crate::platform::storage::UiStorage;
use crate::project_tree::{OpenFile, ProjectTree};
use crate::terminal::view::TerminalView;
use crate::theme;
use gpui_kit::Subscription;
use gpui_kit::{AnyView, FocusHandle};
use gpui_kit::{
    App, Context, Entity, Focusable, Global, IntoElement, ModifiersChangedEvent, MouseDownEvent, Render, SharedString,
    WeakEntity, Window, WindowId, div, prelude::*,
};
use ide_core::IdeEvent;
use ide_core::event::NotifyLevel;
use notifications::{NotificationLevel, NotificationRequest, Notifications};
use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use tasks::{TaskEvent, Tasks};
use tool_windows::{Anchor, Effects, Slot, Stripe, ToolWindow, ToolWindowLayout, storage_key};

/// What a window shows.
#[expect(clippy::large_enum_variant, reason = "one per window, replaced only when a project opens or closes")]
enum Content {
    /// Nothing is open.
    Empty,
    Opening(PathBuf),
    Editor(Entity<Editor>),
    /// A project: its file tree and the file being edited.
    Project {
        name: String,
        /// Which tool windows are shown, where and how large.
        layout: Box<ToolWindowLayout>,
        /// The last focus request of `layout` that was carried out.
        focus_served: u64,
        tree: Entity<ProjectTree>,
        /// The view of every tool window that has been shown, by tool window id. A hidden
        /// window keeps its view, and what runs in it.
        panels: HashMap<String, Panel>,
        /// The editor tabs, and the editor of each tab that has finished opening, by tab id.
        tabs: Box<TabsState>,
        editors: HashMap<String, Entity<Editor>>,
        /// Tabs whose document is being opened.
        loading: std::collections::HashSet<String>,
        /// Where the files of the restored session were left, for the ones not opened yet.
        restored_views: std::collections::BTreeMap<String, FileViewState>,
        _opens: Subscription,
    },
    Failed(SharedString),
}

/// A tool window's content and where its keyboard focus goes.
struct Panel {
    view: AnyView,
    focus: FocusHandle,
}

/// How long the tabs rest before the editor session is written.
const SESSION_SAVE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// One window: its backend label and what it shows.
pub struct Workbench {
    /// The label the backend knows this window by.
    label: String,
    content: Content,
    /// Redraws the status bar when the shown editor changes.
    editor_observer: Option<Subscription>,
    /// The title the window currently carries.
    shown_title: Option<String>,
    /// Background work the backend reports for this window, and what it tells the user.
    tasks: Tasks,
    notifications: Notifications,
    /// Wakes the task list when its next deadline (a finished task leaving, a slow one
    /// appearing) falls due.
    task_timer: Option<gpui_kit::Task<()>>,
    listener: u64,
    /// The editor that was active before the last tab change: it is saved when another takes over.
    previous_editor: Option<gpui_kit::WeakEntity<Editor>>,
    /// The unsaved-changes question is on screen.
    closing: bool,
    /// A pending write of the editor session.
    session_save: Option<gpui_kit::Task<()>>,
}

/// Window title, IDEA style: `<project> – <file>`; with "Always show full path in window header"
/// the file's full path. A remote project names its host: `<project> [<user@host>] – <file>`.
/// Without a project the file alone, or `default` when nothing is open.
pub fn window_title(
    project: Option<&str>,
    file: Option<&std::path::Path>,
    full_path: bool,
    host: Option<&str>,
    default: &str,
) -> String {
    let name = file.map(|file| match file.file_name().filter(|_| !full_path) {
        Some(name) => name.to_string_lossy().into_owned(),
        None => file.to_string_lossy().into_owned(),
    });
    let Some(project) = project.filter(|p| !p.is_empty()) else { return name.unwrap_or_else(|| default.to_owned()) };
    let place = match host {
        Some(host) => format!("{project} [{host}]"),
        None => project.to_owned(),
    };
    match name {
        Some(name) => format!("{place} – {name}"),
        None => place,
    }
}

/// What a tab shows: the file's name, the folder that tells it apart from a tab of the same
/// name, and whether it is the selected one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabTitle {
    pub name: String,
    pub hint: Option<String>,
    pub active: bool,
}

/// How a workbench window is created: the title bar is the app's own, drawn under the native
/// window buttons (the webview's "Overlay" title bar).
pub fn window_options() -> gpui_kit::WindowOptions {
    gpui_kit::WindowOptions {
        titlebar: Some(gpui_kit::TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(gpui_kit::point(gpui_kit::px(13.0), gpui_kit::px(13.0))),
        }),
        ..Default::default()
    }
}

/// Room the title bar leaves for the native macOS window buttons.
const WINDOW_BUTTONS_WIDTH: f32 = 82.0;

/// The Project view's tool window id.
pub const PROJECT_TOOL_WINDOW: &str = "project";

/// The Terminal's tool window id.
pub const TERMINAL_TOOL_WINDOW: &str = "terminal";

/// The Git log's tool window id.
pub const GIT_TOOL_WINDOW: &str = "git";

/// The tool windows this app has a view for.
const TOOL_WINDOWS: [&str; 3] = [PROJECT_TOOL_WINDOW, TERMINAL_TOOL_WINDOW, GIT_TOOL_WINDOW];

/// What the webview UI declares for tool window `id` (title, icon, side, order…), from the
/// contributions manifest.
pub fn tool_window_declaration(id: &str) -> Option<ToolWindow> {
    static DECLARED: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    let manifest = DECLARED.get_or_init(|| {
        serde_json::from_str(include_str!("../../assets/gen/contributions.json"))
            .expect("assets/gen/contributions.json is generated and valid")
    });
    let declared = manifest
        .get("registerToolWindow")?
        .as_array()?
        .iter()
        .find(|w| w.get("id").and_then(|v| v.as_str()) == Some(id))?;
    let text = |key: &str| declared.get(key).and_then(|v| v.as_str());
    let anchor = match text("anchor")? {
        "left" => Anchor::Left,
        "right" => Anchor::Right,
        "bottom" => Anchor::Bottom,
        _ => return None,
    };
    let flag = |key: &str| declared.get(key).and_then(|v| v.as_bool());
    Some(ToolWindow {
        order: declared.get("order").and_then(|v| v.as_f64()),
        secondary: flag("secondary").unwrap_or(false),
        stripe: if text("stripe") == Some("onUse") { Stripe::OnUse } else { Stripe::Shown },
        verbatim: flag("verbatim").unwrap_or(false),
        requires_project: flag("requiresProject").unwrap_or(true),
        ..ToolWindow::new(id, text("title")?, text("icon")?, anchor)
    })
}

/// Registers the workbench's actions: one `toolwindow.<id>` per tool window this app has a view
/// for, and the window-level ones (hide all, jump to last, hide active, focus editor).
pub fn register_actions(cx: &mut App) {
    fn on_layout(
        window: &mut Window,
        cx: &mut App,
        change: impl FnOnce(&mut ToolWindowLayout) -> Effects,
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.with_layout(window, cx, change));
        Ok(())
    }
    let registry = cx.default_global::<ActionRegistry>();
    for id in TOOL_WINDOWS {
        let Some(declared) = tool_window_declaration(id) else { continue };
        let meta = ActionMeta {
            category: Some("Tool Windows".to_owned()),
            icon: Some(declared.icon.clone()),
            keywords: ["tool window", "show", "hide"].map(str::to_owned).to_vec(),
            verbatim: declared.verbatim,
            ..ActionMeta::new(&format!("toolwindow.{id}"), &declared.title)
        };
        registry.register(Action::new(meta, move |_, window, cx| on_layout(window, cx, |layout| layout.activate(id))));
    }
    fn on_tabs(
        window: &mut Window,
        cx: &mut App,
        change: impl FnOnce(&mut TabsState, &str) -> bool,
    ) -> anyhow::Result<()> {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.with_tabs(window, cx, change));
        Ok(())
    }
    /// The active tab of `pane`, for actions that act on it.
    fn active(tabs: &TabsState, pane: &str) -> Option<String> {
        tabs.pane(pane).and_then(|pane| pane.active.clone())
    }
    registry.handle("view.toggleTheme", |_, window, cx| {
        let label = for_window(window, cx).map(|workbench| workbench.read(cx).label.clone()).unwrap_or_default();
        crate::theme::appearance::toggle_theme(&label, cx)
    });
    registry.handle_toggle(
        "ide.toggleMode",
        |window, cx| for_window(window, cx).is_some_and(|workbench| workbench.read(cx).ide_mode(cx).is_full()),
        |_, window, cx| {
            let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
            workbench.update(cx, |workbench, cx| workbench.toggle_ide_mode(cx));
            Ok(())
        },
    );
    registry.handle("editor.closeTab", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.close_tab(pane, &tab)))
    });
    registry.handle("editor.closeOtherTabs", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.close_others(pane, &tab)))
    });
    registry
        .handle("editor.closeAllTabs", |_, window, cx| on_tabs(window, cx, |tabs, pane| tabs.close_all(pane, false)));
    registry.handle("editor.pinTab", |_, window, cx| {
        on_tabs(window, cx, |tabs, pane| active(tabs, pane).is_some_and(|tab| tabs.toggle_pin(pane, &tab)))
    });
    registry.handle("editor.nextTab", |_, window, cx| on_tabs(window, cx, |tabs, _| tabs.cycle_tab(1, None)));
    registry.handle("editor.prevTab", |_, window, cx| on_tabs(window, cx, |tabs, _| tabs.cycle_tab(-1, None)));
    registry.handle("editor.reopenClosedTab", |_, window, cx| {
        let workbench = for_window(window, cx).ok_or_else(|| anyhow::anyhow!("this window has no workbench"))?;
        workbench.update(cx, |workbench, cx| workbench.reopen_closed_tab(window, cx));
        Ok(())
    });
    debug_assert!(manifest().contains_key("toolwindow.hideAll"));
    registry.handle("toolwindow.hideAll", |_, window, cx| on_layout(window, cx, ToolWindowLayout::hide_all));
    registry.handle("toolwindow.jumpToLast", |_, window, cx| on_layout(window, cx, ToolWindowLayout::jump_to_last));
    registry.handle("toolwindow.hideActive", |_, window, cx| on_layout(window, cx, ToolWindowLayout::hide_active));
    registry.handle("toolwindow.focusEditor", |_, window, cx| {
        on_layout(window, cx, |_| Effects { persist: false, focus_editor: true })
    });
}

/// The workbench of each window, so that an action can find what it acts on.
#[derive(Default)]
struct Workbenches(HashMap<WindowId, WeakEntity<Workbench>>);

impl Global for Workbenches {}

/// The workbench shown in `window`.
pub fn for_window(window: &Window, cx: &App) -> Option<Entity<Workbench>> {
    cx.try_global::<Workbenches>()?.0.get(&window.window_handle().window_id())?.upgrade()
}

/// The editor an action invoked in `window` acts on.
pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> {
    for_window(window, cx)?.read(cx).editor().cloned()
}

impl Workbench {
    /// The workbench of `window`, known to the backend as `label`.
    pub fn new(label: &str, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Closing the window asks about unsaved changes first.
        let guard = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            guard.update(cx, |workbench, cx| workbench.confirm_close(window, cx)).unwrap_or(true)
        });
        cx.observe_window_appearance(window, |_, window, cx| {
            crate::theme::appearance::system_appearance_changed(window.appearance(), cx);
        })
        .detach();
        // Leaving the window saves what is unsaved, as switching tabs does.
        cx.observe_window_activation(window, |workbench, window, cx| {
            if !window.is_window_active() {
                workbench.autosave(None, cx);
                workbench.save_session(cx);
            }
        })
        .detach();
        let id = window.window_handle().window_id();
        let this = cx.weak_entity();
        cx.default_global::<Workbenches>().0.insert(id, this);
        // Events for this window, and those for every window, reach its models.
        let weak = cx.weak_entity();
        let listener = cx.try_global::<Backend>().is_some().then(|| {
            cx.global_mut::<Backend>().listen(move |event, cx| {
                let _ = weak.update(cx, |workbench, cx| workbench.backend_event(event, cx));
            })
        });
        cx.on_release(move |workbench, cx| {
            // The window is going: the session is written as it stands.
            workbench.save_session(cx);
            if cx.has_global::<Backend>() {
                cx.global_mut::<Backend>().unlisten(workbench.listener);
            }
            cx.default_global::<Workbenches>().0.remove(&id);
            input::window_blurred(id, cx);
            crate::platform::context::drop_window_context(id, cx);
        })
        .detach();
        Self {
            label: label.to_owned(),
            content: Content::Empty,
            editor_observer: None,
            shown_title: None,
            tasks: Tasks::default(),
            notifications: Notifications::default(),
            task_timer: None,
            listener: listener.unwrap_or_default(),
            previous_editor: None,
            closing: false,
            session_save: None,
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    /// How much of the IDE runs for this window: Blaze (editor, git and search only) or Deep
    /// (language servers and the tooling feeding them too).
    pub fn ide_mode(&self, cx: &App) -> ide_core::settings::IdeMode {
        crate::platform::settings::effective(&self.label, cx).ide.mode
    }

    /// Switches between Blaze and Deep mode, for every project; a project whose own settings
    /// name the mode is switched too, so the toggle always takes effect in this window.
    pub fn toggle_ide_mode(&mut self, cx: &mut Context<Self>) {
        use crate::platform::settings::set;
        use ide_core::settings::IdeMode;
        use ide_workspace::settings::SettingsScope;
        let next = if self.ide_mode(cx).is_full() { IdeMode::Light } else { IdeMode::Full };
        let value = serde_json::Value::from(next.as_str());
        let label = self.label.clone();
        let in_project = matches!(self.content, Content::Project { .. });
        let switched = set(&label, SettingsScope::Global, "ide.mode", &value, cx).and_then(|settings| {
            if in_project && settings.ide.mode != next {
                set(&label, SettingsScope::Project, "ide.mode", &value, cx).map(|_| ())
            } else {
                Ok(())
            }
        });
        if let Err(error) = switched {
            let mut request = NotificationRequest::new(NotificationLevel::Error, "Cannot change the IDE mode");
            request.body = Some(error.message);
            self.notify(request, cx);
        }
        cx.notify();
    }

    /// The folder of the project open in this window.
    pub fn project_root(&self, cx: &App) -> Option<PathBuf> {
        match &self.content {
            Content::Project { tree, .. } => Some(PathBuf::from(tree.read(cx).root())),
            _ => None,
        }
    }

    /// Shows `request` in this window, subject to the notification settings.
    pub fn notify(&mut self, request: NotificationRequest, cx: &mut Context<Self>) {
        let _ = self.notifications.notify(request, None, now_ms());
        cx.notify();
    }

    pub fn editor(&self) -> Option<&Entity<Editor>> {
        match &self.content {
            Content::Editor(editor) => Some(editor),
            Content::Project { tabs, editors, .. } => {
                tabs.pane(&tabs.active_pane).and_then(|pane| pane.active.as_ref()).and_then(|id| editors.get(id))
            }
            _ => None,
        }
    }

    /// The editor tabs, when a project is open.
    pub fn tabs(&self) -> Option<&TabsState> {
        match &self.content {
            Content::Project { tabs, .. } => Some(tabs.as_ref()),
            _ => None,
        }
    }

    /// The titles of the active pane's tabs, in strip order. Files of the same name carry the
    /// folder that tells them apart as a hint.
    pub fn tab_titles(&self) -> Vec<TabTitle> {
        let Some(pane) = self.tabs().and_then(|tabs| tabs.pane(&tabs.active_pane)) else { return Vec::new() };
        let paths: Vec<&str> = pane.tabs.iter().filter_map(|tab| tab.input.path.as_deref()).collect();
        let hints = disambiguate_titles(&paths);
        pane.tabs
            .iter()
            .map(|tab| {
                let path = tab.input.path.as_deref().unwrap_or(&tab.input.key);
                TabTitle {
                    name: path.rsplit('/').next().unwrap_or(path).to_owned(),
                    hint: hints.get(path).cloned(),
                    active: pane.active.as_deref() == Some(tab.id.as_str()),
                }
            })
            .collect()
    }

    /// Changes the editor tabs with `change` (given the model and the active pane's id), then
    /// brings the window in line: editors of closed tabs are saved and released, and the focus
    /// goes to the tab that is now active.
    pub fn with_tabs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut TabsState, &str) -> bool,
    ) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        if change(tabs, &pane) {
            self.tabs_changed(true, window, cx);
        }
    }

    fn tabs_changed(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let auto_save = self.auto_save(cx);
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let open: std::collections::HashSet<&str> =
            tabs.panes.iter().flat_map(|pane| pane.tabs.iter().map(|tab| tab.id.as_str())).collect();
        let closed: Vec<String> = editors.keys().filter(|id| !open.contains(id.as_str())).cloned().collect();
        for id in closed {
            let Some(editor) = editors.remove(&id) else { continue };
            // Closing never loses edits: an unsaved document is written before it is released,
            // and when it cannot be written its tab comes back with the text still in it. With
            // autosave off the user decides instead.
            if !editor.read(cx).is_dirty() {
                continue;
            }
            let decision = (!auto_save).then(|| {
                let path = editor.read(cx).path().to_string_lossy().into_owned();
                let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
                let (title, body) = (
                    i18n.t("app.unsaved.title", &[]),
                    i18n.t("app.unsaved.body", &[("count", 1usize.into()), ("project", path.into())]),
                );
                let answers =
                    [i18n.t("app.unsaved.save", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])];
                let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
                window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(&body), &answers, cx)
            });
            cx.spawn_in(window, async move |workbench, cx| {
                let answer = match decision {
                    Some(decision) => decision.await.ok(),
                    None => Some(0),
                };
                let failure = match answer {
                    Some(0) => {
                        let saving = editor.read_with(cx, |editor, cx| editor.save(cx));
                        match saving.await {
                            Ok(Ok(())) => return,
                            Ok(Err(error)) => Some(error.message),
                            Err(error) => Some(error.to_string()),
                        }
                    }
                    // Don't Save.
                    Some(1) => return,
                    // Cancel, or the question went away unanswered: the tab stays.
                    _ => None,
                };
                let _ = workbench
                    .update_in(cx, |workbench, window, cx| workbench.restore_unsaved(editor, failure, window, cx));
            })
            .detach();
        }
        self.load_active_tab(focus, window, cx);
        self.session_save = Some(cx.spawn(async move |workbench, cx| {
            cx.background_executor().timer(SESSION_SAVE_DELAY).await;
            let _ = workbench.update(cx, |workbench, cx| workbench.save_session(cx));
        }));
        let active = self.editor().cloned();
        // The editor that was active is saved when another one takes its place.
        let previous = self.previous_editor.take().and_then(|previous| previous.upgrade());
        if let Some(previous) = previous.filter(|previous| active.as_ref() != Some(previous)) {
            self.autosave(Some(&previous), cx);
        }
        self.previous_editor = active.as_ref().map(Entity::downgrade);
        match active {
            Some(editor) => {
                self.observe_editor(&editor, cx);
                if focus {
                    let handle = editor.focus_handle(cx);
                    window.focus(&handle, cx);
                }
            }
            None => self.editor_observer = None,
        }
        cx.notify();
    }

    /// A closed tab whose document was not saved, because saving failed or the user chose to
    /// stay: it reopens with its editor, and a failure is reported.
    fn restore_unsaved(
        &mut self,
        editor: Entity<Editor>,
        failure: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let path = editor.read(cx).path().to_string_lossy().into_owned();
        if let Some(failure) = failure {
            self.report_save_failure(failure, cx);
        }
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let pane = tabs.active_pane.clone();
        let input = EditorInput { kind: "file".to_owned(), key: path.clone(), path: Some(path), data: None };
        if let Some(id) = tabs.open_tab(&pane, input, OpenTabOptions::default()) {
            // A tab for the same file opened meanwhile has its own editor: the unsaved one wins,
            // since it is the one holding the text that is not on disk.
            editors.insert(id, editor);
        }
        self.tabs_changed(true, window, cx);
    }

    fn report_save_failure(&mut self, failure: String, cx: &mut Context<Self>) {
        let title = cx.default_global::<crate::platform::i18n::Localization>().0.t("app.unsaved.saveFailed", &[]);
        let mut request = NotificationRequest::new(NotificationLevel::Error, title);
        request.body = Some(failure);
        let _ = self.notifications.notify(request, None, now_ms());
        cx.notify();
    }

    /// Every open editor, of every tab.
    fn editors(&self) -> Vec<Entity<Editor>> {
        match &self.content {
            Content::Editor(editor) => vec![editor.clone()],
            Content::Project { editors, .. } => editors.values().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// How many open documents have changes that are not on disk.
    pub fn unsaved_count(&self, cx: &App) -> usize {
        self.editors().iter().filter(|editor| editor.read(cx).is_dirty()).count()
    }

    /// Files › Autosave (`files.autoSave`): save when a tab is left or closed and when the
    /// window loses focus.
    fn auto_save(&self, cx: &App) -> bool {
        crate::platform::settings::effective(&self.label, cx).files.auto_save
    }

    /// With autosave on, saves `editor` if it is unsaved, or every unsaved editor when none is
    /// named. A failure is reported; the text stays in its editor.
    fn autosave(&mut self, editor: Option<&Entity<Editor>>, cx: &mut Context<Self>) {
        if !self.auto_save(cx) {
            return;
        }
        let editors = match editor {
            Some(editor) => vec![editor.clone()],
            None => self.editors(),
        };
        for editor in editors.into_iter().filter(|editor| editor.read(cx).is_dirty()) {
            let saving = editor.read(cx).save(cx);
            cx.spawn(async move |workbench, cx| {
                let failure = match saving.await {
                    Ok(Ok(())) => return,
                    Ok(Err(error)) => error.message,
                    Err(error) => error.to_string(),
                };
                let _ = workbench.update(cx, |workbench, cx| workbench.report_save_failure(failure, cx));
            })
            .detach();
        }
    }

    /// Saves every unsaved editor; the first failure's message, if any.
    fn save_all(&self, cx: &App) -> impl Future<Output = Result<(), String>> + 'static + use<> {
        let saving: Vec<_> = self
            .editors()
            .iter()
            .filter(|editor| editor.read(cx).is_dirty())
            .map(|editor| editor.read(cx).save(cx))
            .collect();
        async move {
            let mut failure = None;
            for save in saving {
                match save.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failure = failure.or(Some(error.message)),
                    Err(error) => failure = failure.or(Some(error.to_string())),
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }

    /// Whether the window may close now. With unsaved changes it may not: the user is asked to
    /// save them, discard them or stay, and the window closes once that is settled.
    pub fn confirm_close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let unsaved = self.unsaved_count(cx);
        if unsaved == 0 {
            return true;
        }
        if self.closing {
            return false;
        }
        self.closing = true;
        let project = match &self.content {
            Content::Project { name, .. } => Some(name.clone()),
            _ => None,
        };
        let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
        let project = project.unwrap_or_else(|| i18n.t("app.title.default", &[]));
        let (title, body) = (
            i18n.t("app.unsaved.title", &[]),
            i18n.t("app.unsaved.body", &[("count", unsaved.into()), ("project", project.into())]),
        );
        let answers =
            [i18n.t("app.unsaved.save", &[]), i18n.t("app.unsaved.discard", &[]), i18n.t("common.cancel", &[])];
        let answers: Vec<&str> = answers.iter().map(String::as_str).collect();
        let answer = window.prompt(gpui_kit::PromptLevel::Warning, &title, Some(&body), &answers, cx);
        cx.spawn_in(window, async move |workbench, cx| {
            let answer = answer.await.ok();
            let saving = workbench.update(cx, |workbench, cx| {
                workbench.closing = false;
                (answer == Some(0)).then(|| workbench.save_all(cx))
            });
            let close = match (answer, saving) {
                (Some(0), Ok(Some(saving))) => match saving.await {
                    Ok(()) => true,
                    Err(failure) => {
                        let _ = workbench.update(cx, |workbench, cx| workbench.report_save_failure(failure, cx));
                        false
                    }
                },
                // Don't Save.
                (Some(1), _) => true,
                _ => false,
            };
            if close {
                let _ = cx.update(|window, _| window.remove_window());
            }
        })
        .detach();
        false
    }

    /// Background tasks of this window, in listing order.
    pub fn tasks(&self) -> &Tasks {
        &self.tasks
    }

    pub fn notifications(&self) -> &Notifications {
        &self.notifications
    }

    /// What the status bar says about background work: the first task's title with its message,
    /// and how many more are running.
    pub fn task_status(&self) -> Option<String> {
        let tasks = self.tasks.tasks();
        let first = tasks.first()?;
        let mut text = match &first.message {
            Some(message) if !message.is_empty() => format!("{}: {message}", first.title),
            _ => first.title.clone(),
        };
        if tasks.len() > 1 {
            text.push_str(&format!(" (+{})", tasks.len() - 1));
        }
        Some(text)
    }

    /// A backend event: its task, indexing and notification reports feed this window's models.
    pub fn backend_event(&mut self, event: &BackendEvent, cx: &mut Context<Self>) {
        if event.window.as_deref().is_some_and(|window| window != self.label) {
            return;
        }
        let now = now_ms();
        match &event.event {
            IdeEvent::Task { id, title, message, fraction, done, cancellable } => self.tasks.on_task_event(
                TaskEvent {
                    id: u64::from(id.0),
                    title: title.clone(),
                    message: message.clone(),
                    fraction: fraction.map(f64::from),
                    done: *done,
                    cancellable: *cancellable,
                },
                now,
            ),
            IdeEvent::IndexState { indexing, files, symbols } => {
                self.tasks.set_indexing(*indexing, u64::from(*files), u64::from(*symbols), now);
            }
            IdeEvent::Notify { level, title, body } => {
                let level = match level {
                    NotifyLevel::Info => NotificationLevel::Info,
                    NotifyLevel::Warning => NotificationLevel::Warning,
                    NotifyLevel::Error => NotificationLevel::Error,
                };
                let mut request = NotificationRequest::new(level, title.clone());
                request.body = body.clone();
                // Balloon settings are not read yet: every group uses its defaults.
                let _ = self.notifications.notify(request, None, now);
            }
            _ => return,
        }
        self.schedule_tasks(cx);
        cx.notify();
    }

    /// Advances the task list to `now` and arranges to be woken at its next deadline.
    pub fn tick_tasks(&mut self, now: f64, cx: &mut Context<Self>) {
        self.tasks.expire(now);
        self.schedule_tasks(cx);
        cx.notify();
    }

    fn schedule_tasks(&mut self, cx: &mut Context<Self>) {
        self.task_timer = self.tasks.next_deadline().map(|due| {
            let wait = std::time::Duration::from_secs_f64(((due - now_ms()) / 1000.0).max(0.0));
            cx.spawn(async move |workbench, cx| {
                cx.background_executor().timer(wait).await;
                let _ = workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms(), cx));
            })
        });
    }

    /// The status bar's right-hand items for the shown editor: caret position, line separator
    /// and encoding, in that order.
    pub fn status_items(&self, cx: &App) -> Vec<String> {
        let Some(status) = self.editor().map(|editor| editor.read(cx).status()) else { return Vec::new() };
        vec![status.caret_text(), status.line_ending_text().to_owned(), status.encoding.to_uppercase()]
    }

    /// The window's title for what it shows now.
    pub fn title(&self, cx: &App) -> String {
        let project = match &self.content {
            Content::Project { name, .. } => Some(name.as_str()),
            _ => None,
        };
        let file = self.editor().map(|editor| editor.read(cx).path().to_path_buf());
        let default = cx
            .try_global::<crate::platform::i18n::Localization>()
            .map_or_else(|| "app.title.default".to_owned(), |l| l.0.t("app.title.default", &[]));
        window_title(project, file.as_deref(), false, None, &default)
    }

    /// Publishes the facts `when` clauses read about this window: what is open and where the
    /// focus is. Unchanged values cost nothing.
    fn publish_context(&mut self, window: &mut Window, cx: &mut App) {
        use crate::platform::context::{clear_context, set_context};
        let editor = self.editor().cloned();
        let focused = editor.as_ref().is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
        set_context(window, cx, "projectOpening", matches!(self.content, Content::Opening(_)));
        set_context(window, cx, "projectOpen", self.project_tree().is_some());
        set_context(window, cx, "lightEdit", matches!(self.content, Content::Editor(_)));
        set_context(window, cx, "textEditorActive", editor.is_some());
        set_context(window, cx, "editorSplit", self.tabs().is_some_and(|tabs| tabs.panes.len() > 1));
        let tabs_open =
            self.tabs().and_then(|tabs| tabs.pane(&tabs.active_pane)).is_some_and(|pane| !pane.tabs.is_empty());
        set_context(window, cx, "editorTabsOpen", tabs_open);
        for key in ["editorFocus", "editorTextFocus", "editorAreaFocus"] {
            set_context(window, cx, key, focused);
        }
        let focused_panel = match &self.content {
            Content::Project { panels, .. } => {
                panels.iter().find(|(_, panel)| panel.focus.contains_focused(window, cx)).map(|(id, _)| id.clone())
            }
            _ => None,
        };
        set_context(window, cx, "toolWindowFocus", focused_panel.is_some());
        if let Content::Project { layout, .. } = &mut self.content {
            // The layout decides by this what `toolwindow.<id>` does: focus, or hide.
            layout.set_focused(focused_panel.as_deref());
            for (key, visible) in layout.visibility_context() {
                set_context(window, cx, &key, visible);
            }
            match layout.focused().map(str::to_owned) {
                Some(id) => set_context(window, cx, "activeToolWindow", id),
                None => clear_context(window, cx, "activeToolWindow"),
            }
        }
        match editor.map(|editor| {
            let editor = editor.read(cx);
            (editor.path().to_string_lossy().into_owned(), editor.language().to_string(), editor.status())
        }) {
            Some((path, language, status)) => {
                set_context(window, cx, "editorPath", path);
                set_context(window, cx, "editorLangId", language);
                set_context(window, cx, "editorReadOnly", status.read_only);
                set_context(window, cx, "editorHasSelection", status.selected_chars > 0);
            }
            None => {
                clear_context(window, cx, "editorPath");
                clear_context(window, cx, "editorLangId");
                set_context(window, cx, "editorReadOnly", false);
                set_context(window, cx, "editorHasSelection", false);
            }
        }
    }

    fn observe_editor(&mut self, editor: &Entity<Editor>, cx: &mut Context<Self>) {
        self.editor_observer = Some(cx.observe(editor, |_, _, cx| cx.notify()));
    }

    /// The tool window layout, when a project is open.
    pub fn tool_windows(&self) -> Option<&ToolWindowLayout> {
        match &self.content {
            Content::Project { layout, .. } => Some(layout.as_ref()),
            _ => None,
        }
    }

    /// Changes the tool window layout with `change` and carries out what it asks for: storing
    /// the layout, moving the focus to the editor, or to the tool window that requested it.
    pub fn with_layout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut ToolWindowLayout) -> Effects,
    ) {
        let editor = self.editor().cloned();
        let Content::Project { layout, .. } = &mut self.content else { return };
        let effects = change(layout);
        if effects.persist {
            match serde_json::to_string(&layout.serialized()) {
                Ok(json) => cx.default_global::<UiStorage>().set(&storage_key(layout.scope()), json),
                Err(error) => tracing::warn!("cannot store the tool window layout: {error}"),
            }
        }
        // A window shown for the first time gets its view before the focus can go to it.
        self.mount_panels(cx);
        let Content::Project { layout, focus_served, panels, .. } = &mut self.content else { return };
        let request = layout.focus_request().filter(|request| request.seq > *focus_served).cloned();
        if let Some(request) = request {
            *focus_served = request.seq;
            if let Some(panel) = panels.get(&request.id) {
                window.focus(&panel.focus, cx);
            }
        } else if effects.focus_editor {
            match editor {
                Some(editor) => {
                    let focus = editor.focus_handle(cx);
                    window.focus(&focus, cx);
                }
                // Nothing to edit: the focus leaves the tool window all the same.
                None => window.blur(cx),
            }
        }
        cx.notify();
    }

    /// Creates the view of every visible tool window that has none yet.
    fn mount_panels(&mut self, cx: &mut Context<Self>) {
        let Content::Project { layout, panels, tree, .. } = &self.content else { return };
        let missing: Vec<String> =
            layout.visible().ids().filter(|id| !panels.contains_key(*id)).map(str::to_owned).collect();
        let root = PathBuf::from(tree.read(cx).root());
        for id in missing {
            let panel = match id.as_str() {
                TERMINAL_TOOL_WINDOW => {
                    let (label, options) = (self.label.clone(), self.terminal_options(&root, cx));
                    let terminal = cx.new(|cx| TerminalView::new(&label, options, cx));
                    Panel { focus: terminal.focus_handle(cx), view: terminal.into() }
                }
                GIT_TOOL_WINDOW => {
                    let label = self.label.clone();
                    let log = cx.new(|cx| crate::vcs::log_view::GitLog::new(&label, cx));
                    Panel { focus: log.focus_handle(cx), view: log.into() }
                }
                _ => continue,
            };
            if let Content::Project { panels, .. } = &mut self.content {
                panels.insert(id, panel);
            }
        }
    }

    /// How a terminal of this window's project starts: Settings › Terminal's start directory
    /// (the project folder unless set; relative to it unless absolute or under `~`), shell
    /// command line, environment and shell integration.
    fn terminal_options(&self, root: &std::path::Path, cx: &App) -> ide_terminal::TerminalOptions {
        let terminal = crate::platform::settings::effective(&self.label, cx).terminal;
        let directory = terminal.start_directory.trim_end_matches('/');
        let cwd = if directory.is_empty() {
            root.to_path_buf()
        } else if directory.starts_with('/') || directory.starts_with("~/") {
            PathBuf::from(directory)
        } else {
            root.join(directory.strip_prefix("./").unwrap_or(directory))
        };
        let mut command = crate::platform::command_line::parse_shell_command(&terminal.shell_path, cfg!(windows));
        let shell = (!command.is_empty()).then(|| command.remove(0));
        ide_terminal::TerminalOptions {
            cwd: ide_core::IpcPath(cwd),
            shell,
            args: command,
            env: terminal.env,
            // The grid is fitted to the tool window as soon as it is painted.
            cols: 80,
            rows: 24,
            shell_integration: terminal.shell_integration,
        }
    }

    /// The view of tool window `id`, once it has been shown.
    pub fn tool_window_view(&self, id: &str) -> Option<AnyView> {
        match &self.content {
            Content::Project { panels, .. } => panels.get(id).map(|panel| panel.view.clone()),
            _ => None,
        }
    }

    /// Whether tool window `id` is showing.
    pub fn is_tool_window_visible(&self, id: &str) -> bool {
        matches!(&self.content, Content::Project { layout, .. } if layout.is_visible(id))
    }

    /// The file tree, when a project is open.
    pub fn project_tree(&self) -> Option<&Entity<ProjectTree>> {
        match &self.content {
            Content::Project { tree, .. } => Some(tree),
            _ => None,
        }
    }

    /// Opens the folder at `root` as this window's project.
    pub fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.content = Content::Opening(root.clone());
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let project = ide_host::ops::project::open(&host, &label, &root).await?;
            let info = ide_host::ops::project::info(&project);
            let name = if info.display_name.is_empty() { info.name } else { info.display_name };
            ide_core::IdeResult::Ok((project.root.clone(), name))
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let opened =
                    result.map_err(|error| error.to_string()).and_then(|opened| opened.map_err(|error| error.message));
                workbench.content = match opened {
                    Ok((root, name)) => {
                        let label = workbench.label.clone();
                        // The layout is stored per project folder, as the webview keys it.
                        let scope = root.to_string_lossy().into_owned();
                        let stored =
                            cx.try_global::<UiStorage>().and_then(|s| s.get(&storage_key(&scope)).map(str::to_owned));
                        let mut layout = Box::new(ToolWindowLayout::new());
                        layout.load(&scope, stored.as_deref());
                        let tree = cx.new(|cx| ProjectTree::new(&label, root, cx));
                        let opens = cx.subscribe_in(&tree, window, |workbench, _, event: &OpenFile, window, cx| {
                            workbench.open_project_file(event.0.clone(), window, cx);
                        });
                        let project_panel = Panel { focus: tree.focus_handle(cx), view: tree.clone().into() };
                        // The tabs, splits and carets the project was left with.
                        let saved = cx.try_global::<UiStorage>().and_then(|s| session::load_session(s, &scope));
                        let (tabs, restored_views) = match saved {
                            Some(saved) => (session::restore_tabs_state(&saved, |path| path.to_owned()), saved.files),
                            None => (TabsState::new(), Default::default()),
                        };
                        Content::Project {
                            name,
                            layout,
                            focus_served: 0,
                            tree,
                            panels: HashMap::from([(PROJECT_TOOL_WINDOW.to_owned(), project_panel)]),
                            tabs: Box::new(tabs),
                            editors: HashMap::new(),
                            loading: Default::default(),
                            restored_views,
                            _opens: opens,
                        }
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                // Registering a window reconciles the stored layout with what exists; a project
                // opens with the focus in its Project view when that is showing.
                workbench.with_layout(window, cx, |layout| {
                    let mut effects = Effects::default();
                    let mut merge = |more: Effects| {
                        effects.persist |= more.persist;
                        effects.focus_editor |= more.focus_editor;
                    };
                    for declared in TOOL_WINDOWS.into_iter().filter_map(tool_window_declaration) {
                        merge(layout.register(declared).1);
                    }
                    if layout.is_visible(PROJECT_TOOL_WINDOW) {
                        merge(layout.focus(PROJECT_TOOL_WINDOW));
                    }
                    effects
                });
                // A restored tab gets its editor when it is the one showing.
                workbench.tabs_changed(false, window, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Opens a file of the project in a tab of the active pane; a file that already has a tab
    /// there is brought to the front instead.
    pub fn open_project_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let key = path.to_string_lossy().into_owned();
        let pane = tabs.active_pane.clone();
        let existing = tabs.find_tab(&key, Some(&pane)).map(|(_, tab)| tab.id.clone());
        let tab_id = match existing {
            Some(id) => {
                tabs.activate_tab(&pane, &id);
                id
            }
            None => {
                let input = EditorInput { kind: "file".to_owned(), key: key.clone(), path: Some(key), data: None };
                let Some(id) = tabs.open_tab(&pane, input, OpenTabOptions::default()) else { return };
                id
            }
        };
        let loaded = editors.contains_key(&tab_id);
        self.tabs_changed(true, window, cx);
        if !loaded {
            self.load_tab(tab_id, path, true, window, cx);
        }
    }

    /// Brings back the tab that was closed last, where it was.
    pub fn reopen_closed_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, .. } = &mut self.content else { return };
        let Some(reopened) = tabs.reopen_closed() else { return };
        let path = tabs
            .pane(&reopened.pane_id)
            .and_then(|pane| pane.tab(&reopened.tab_id))
            .and_then(|tab| tab.input.path.clone());
        self.tabs_changed(true, window, cx);
        if let Some(path) = path {
            self.load_tab(reopened.tab_id, PathBuf::from(path), true, window, cx);
        }
    }

    /// Opens the document of the active tab if it has none yet: a tab restored with the project
    /// is only a title until it is shown.
    fn load_active_tab(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Content::Project { tabs, editors, loading, .. } = &self.content else { return };
        let Some(tab) = tabs.pane(&tabs.active_pane).and_then(|pane| pane.active_tab()) else { return };
        if editors.contains_key(&tab.id) || loading.contains(&tab.id) || tab.input.kind != "file" {
            return;
        }
        let Some(path) = tab.input.path.clone() else { return };
        self.load_tab(tab.id.clone(), PathBuf::from(path), focus, window, cx);
    }

    /// Stores the project's editor session now: its tabs and splits, and where each file's
    /// editor is; a file not opened since the session was restored keeps what was saved for it.
    pub fn save_session(&mut self, cx: &mut App) {
        self.session_save = None;
        let Content::Project { tabs, editors, restored_views, layout, .. } = &self.content else { return };
        let mut files = restored_views.clone();
        for editor in editors.values() {
            let editor = editor.read(cx);
            let view =
                FileViewState { anchor: editor.anchor(), head: editor.caret(), top_line: Some(editor.top_line(cx)) };
            files.insert(editor.path().to_string_lossy().into_owned(), view);
        }
        let saved = session::serialize_session(tabs, &files, |_| false);
        let root = layout.scope().to_owned();
        if cx.has_global::<UiStorage>() {
            session::store_session(cx.global_mut::<UiStorage>(), &root, saved.as_ref());
        }
    }

    /// Opens the document of tab `tab_id` in the backend and gives the tab its editor.
    /// With `focus` the editor takes the keyboard once it is there; a tab restored with the
    /// project loads without taking it from wherever it is.
    fn load_tab(&mut self, tab_id: String, path: PathBuf, focus: bool, window: &mut Window, cx: &mut Context<Self>) {
        // One document per tab: a tab that has its editor, or is getting it, is left alone.
        let Content::Project { loading, editors, .. } = &mut self.content else { return };
        if editors.contains_key(&tab_id) || !loading.insert(tab_id.clone()) {
            return;
        }
        let label = self.label.clone();
        let opened = cx
            .global::<Backend>()
            .with_host(move |host| async move { ide_host::ops::doc::open(&host, &label, path).await });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                let text = result
                    .map_err(|error| error.to_string())
                    .and_then(|opened| opened.map_err(|error| error.message))
                    .and_then(|info| {
                        let project = host.project(&label).map_err(|error| error.message)?;
                        let (version, rope) =
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                        Ok((info, version, rope))
                    });
                let still_open =
                    workbench.tabs().is_some_and(|tabs| tabs.panes.iter().any(|pane| pane.tab(&tab_id).is_some()));
                if let Content::Project { loading, .. } = &mut workbench.content {
                    loading.remove(&tab_id);
                }
                match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        // A tab closed while its file was opening has no use for the editor;
                        // dropping it releases the document.
                        if let (true, Content::Project { editors, restored_views, .. }) =
                            (still_open, &mut workbench.content)
                        {
                            // The file is where it was left when the project was last open.
                            let path = editor.read(cx).path().to_string_lossy().into_owned();
                            if let Some(view) = restored_views.remove(&path) {
                                editor.update(cx, |editor, cx| {
                                    editor.select(view.anchor, view.head, cx);
                                    if let Some(line) = view.top_line {
                                        editor.scroll_to_line(line, cx);
                                    }
                                });
                            }
                            editors.insert(tab_id.clone(), editor);
                        }
                    }
                    Err(message) => {
                        tracing::error!("cannot open the file: {message}");
                        // A tab without a document would be a dead end: it goes away again.
                        if let Content::Project { tabs, .. } = &mut workbench.content {
                            let pane =
                                tabs.panes.iter().find(|pane| pane.tab(&tab_id).is_some()).map(|pane| pane.id.clone());
                            if let Some(pane) = pane {
                                tabs.close_tab(&pane, &tab_id);
                            }
                        }
                    }
                }
                workbench.tabs_changed(focus, window, cx);
            });
        })
        .detach();
    }

    /// The message shown instead of an editor, when opening failed.
    pub fn failure(&self) -> Option<&str> {
        match &self.content {
            Content::Failed(message) => Some(message),
            _ => None,
        }
    }

    /// Opens a loose file for editing (LightEdit: no project, no tool windows).
    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.content = Content::Opening(path.clone());
        cx.notify();
        let label = self.label.clone();
        let opened = cx.global::<Backend>().with_host(move |host| async move {
            let (blocking_host, blocking_label, file) = (host.clone(), label.clone(), path.clone());
            tokio::task::spawn_blocking(move || blocking_host.open_light(&blocking_label, &file).map(|_| ()))
                .await
                .map_err(|e| ide_core::IdeError::internal(e.to_string()))??;
            let file = path.canonicalize().unwrap_or(path);
            ide_host::ops::doc::open(&host, &label, file).await
        });
        cx.spawn_in(window, async move |workbench, cx| {
            let result = opened.await;
            let _ = workbench.update_in(cx, |workbench, window, cx| {
                let host = cx.global::<Backend>().host.clone();
                let label = workbench.label.clone();
                // The document's text is read before the view exists, so a failure is a message
                // in the window instead of an editor without a document.
                let text = result
                    .map_err(|error| error.to_string())
                    .and_then(|opened| opened.map_err(|error| error.message))
                    .and_then(|info| {
                        let project = host.project(&label).map_err(|error| error.message)?;
                        let (version, rope) =
                            project.documents.rope_snapshot(info.id).map_err(|error| error.message)?;
                        Ok((info, version, rope))
                    });
                workbench.content = match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        let focus = editor.focus_handle(cx);
                        window.focus(&focus, cx);
                        workbench.observe_editor(&editor, cx);
                        Content::Editor(editor)
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let title = self.title(cx);
        if self.shown_title.as_deref() != Some(title.as_str()) {
            window.set_window_title(&title);
            self.shown_title = Some(title);
        }
        self.publish_context(window, cx);
        let message = |text: SharedString| {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(text)
        };
        let body = match &self.content {
            Content::Empty => message("".into()).into_any_element(),
            Content::Opening(path) => message(format!("Opening {}…", path.display()).into()).into_any_element(),
            Content::Failed(error) => message(error.clone()).into_any_element(),
            Content::Editor(editor) => editor.clone().into_any_element(),
            Content::Project { layout, panels, .. } => {
                let editor = self.editor().cloned();
                let editor_focused = editor.as_ref().is_some_and(|editor| editor.focus_handle(cx).is_focused(window));
                // The tab strip: the selected tab is a rounded pill; the rest are plain titles.
                let strip = (!self.tab_titles().is_empty()).then(|| {
                    let ids: Vec<String> = self
                        .tabs()
                        .and_then(|tabs| tabs.pane(&tabs.active_pane))
                        .map(|pane| pane.tabs.iter().map(|tab| tab.id.clone()).collect())
                        .unwrap_or_default();
                    div()
                        .h(theme::length(cx, "h-header"))
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(gpui_kit::px(2.0))
                        .px(gpui_kit::px(4.0))
                        .border_b_1()
                        .border_color(theme::color(cx, "border-editor-tabs"))
                        .children(self.tab_titles().into_iter().zip(ids).map(
                            |(TabTitle { name, hint, active }, id)| {
                                let (select, close) = (id.clone(), id.clone());
                                let selected =
                                    if editor_focused { "bg-tab-selected" } else { "bg-tab-selected-inactive" };
                                div()
                                    .id(SharedString::from(format!("tab:{id}")))
                                    .h(theme::length(cx, "h-tab"))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .gap(gpui_kit::px(6.0))
                                    .pl(gpui_kit::px(8.0))
                                    .pr(gpui_kit::px(4.0))
                                    .rounded(gpui_kit::px(6.0))
                                    .text_color(theme::color(cx, "fg-strong"))
                                    .when(active, |el| el.bg(theme::color(cx, selected)))
                                    .when(!active, |el| el.hover(|el| el.bg(theme::color(cx, "bg-tab-hover"))))
                                    .child(theme::icon(
                                        cx,
                                        theme::icons::file_icon_name(&name),
                                        16.0,
                                        theme::color(cx, "fg-strong"),
                                    ))
                                    .child(SharedString::from(name))
                                    .children(hint.map(|hint| {
                                        div().text_color(theme::color(cx, "fg-muted")).child(SharedString::from(hint))
                                    }))
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("close:{id}")))
                                            .size(gpui_kit::px(16.0))
                                            .flex_none()
                                            .rounded(gpui_kit::px(4.0))
                                            .hover(|el| el.bg(theme::color(cx, "bg-tab-hover")))
                                            .child(theme::icon(cx, "close", 16.0, theme::color(cx, "fg-muted")))
                                            .on_click(cx.listener(move |workbench, _, window, cx| {
                                                workbench
                                                    .with_tabs(window, cx, |tabs, pane| tabs.close_tab(pane, &close));
                                                cx.stop_propagation();
                                            })),
                                    )
                                    .on_click(cx.listener(move |workbench, _, window, cx| {
                                        workbench.with_tabs(window, cx, |tabs, pane| {
                                            tabs.activate_tab(pane, &select);
                                            true
                                        });
                                    }))
                            },
                        ))
                });
                let gap = theme::length(cx, "island-gap");
                let island = || {
                    div()
                        .bg(theme::color(cx, "bg-island"))
                        .rounded(theme::length(cx, "island-radius"))
                        .overflow_hidden()
                };
                let button = theme::length(cx, "stripe-icon-button");
                let stripe_gap = theme::length(cx, "h-titlebar") - button;
                let stripe_button = |declared: &ToolWindow| {
                    let shown = layout.is_visible(&declared.id);
                    let color = theme::color(cx, if shown { "fg-emphasis" } else { "fg-muted" });
                    let action = format!("toolwindow.{}", declared.id);
                    div()
                        .id(SharedString::from(format!("stripe:{}", declared.id)))
                        .size(button)
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme::length(cx, "radius-toolbar-button"))
                        .when(shown, |el| el.bg(theme::color(cx, "bg-stripe-selected")))
                        .hover(|el| el.bg(theme::color(cx, "bg-stripe-hover")))
                        .child(theme::icon(cx, &declared.icon, 20.0, color))
                        .on_click(cx.listener(move |_, _, window, cx| {
                            execute_action_from(ActionSource::Toolbar, &action, None, window, cx);
                        }))
                };
                // A stripe lists the windows docked on its side, then the split area's, and at
                // its foot those of the bottom: Bottom Left on the left stripe, Bottom Right on
                // the right one.
                let on_stripe = |slot: Slot| -> Vec<&ToolWindow> {
                    let showing = layout.visible().get(slot);
                    layout
                        .windows_in(slot)
                        .into_iter()
                        .filter(|w| layout.has_stripe_button(w) || showing == Some(w.id.as_str()))
                        .collect()
                };
                let stripe = |side: Anchor| {
                    let group = |slot: Slot| {
                        let windows = on_stripe(slot);
                        (!windows.is_empty()).then(|| {
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(stripe_gap)
                                .children(windows.into_iter().map(&stripe_button))
                        })
                    };
                    let lower = if side == Anchor::Left { Slot::Bottom } else { Slot::BottomSplit };
                    let outer = theme::length(cx, "stripe-outer-padding");
                    div()
                        .w(button + outer + gap)
                        .h_full()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(stripe_gap)
                        .when(side == Anchor::Left, |el| el.pl(outer).pr(gap))
                        .when(side == Anchor::Right, |el| el.pr(outer).pl(gap))
                        .children(group(Slot::new(side, false)))
                        .children(group(Slot::new(side, true)))
                        .child(div().flex_1())
                        .children(group(lower))
                };
                // A tool window: its title, a button to hide it, and its view.
                let tool_window = |id: &str| {
                    let (declared, panel) = (layout.window(id)?, panels.get(id)?);
                    let hidden = id.to_owned();
                    Some(
                        island()
                            .min_w_0()
                            .min_h_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(theme::length(cx, "h-header"))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .pl(gpui_kit::px(12.0))
                                    .pr(gpui_kit::px(6.0))
                                    .text_color(theme::color(cx, "fg-strong"))
                                    .child(SharedString::from(declared.title.clone()))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("hide:{id}")))
                                            .size(gpui_kit::px(22.0))
                                            .flex_none()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .rounded(gpui_kit::px(4.0))
                                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                                            .child(theme::icon(cx, "minus", 16.0, theme::color(cx, "fg-muted")))
                                            .on_click(cx.listener(move |workbench, _, window, cx| {
                                                workbench.with_layout(window, cx, |layout| layout.hide(&hidden));
                                            })),
                                    ),
                            )
                            .child(div().flex_1().min_h_0().child(panel.view.clone())),
                    )
                };
                // A side: its primary and split areas, stacked on the left and right, side by
                // side at the bottom, sharing it by the stored ratio; a lone area takes it all.
                let side = |anchor: Anchor| {
                    let shown = |split: bool| layout.visible().get(Slot::new(anchor, split)).and_then(tool_window);
                    let (primary, split) = (shown(false), shown(true));
                    let both = primary.is_some() && split.is_some();
                    if primary.is_none() && split.is_none() {
                        return None;
                    }
                    let ratio = (layout.splits().get(anchor) as f32).clamp(0.0, 1.0);
                    let share = |area: gpui_kit::Div, part: f32| match (both, anchor) {
                        (false, _) => area.size_full(),
                        (true, Anchor::Bottom) => area.h_full().w(gpui_kit::relative(part)),
                        (true, _) => area.w_full().h(gpui_kit::relative(part)),
                    };
                    Some(
                        div()
                            .flex()
                            .gap(gap)
                            .when(anchor != Anchor::Bottom, |el| el.flex_col())
                            .children(primary.map(|area| share(area, ratio)))
                            .children(split.map(|area| share(area, 1.0 - ratio))),
                    )
                };
                let sizes = layout.sizes();
                let right_stripe = [Slot::Right, Slot::RightSplit, Slot::BottomSplit]
                    .into_iter()
                    .any(|slot| !on_stripe(slot).is_empty())
                    .then(|| stripe(Anchor::Right));
                // The editor between the left and right sides; the bottom side spans below all three.
                let top = div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .flex()
                    .children(
                        side(Anchor::Left).map(|s| s.w(gpui_kit::px(sizes.left as f32)).h_full().flex_none().mr(gap)),
                    )
                    .child(
                        island()
                            .flex_1()
                            .h_full()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .children(strip)
                            .child(div().flex_1().min_h_0().children(editor)),
                    )
                    .children(
                        side(Anchor::Right).map(|s| s.w(gpui_kit::px(sizes.right as f32)).h_full().flex_none().ml(gap)),
                    );
                let bottom =
                    side(Anchor::Bottom).map(|s| s.h(gpui_kit::px(sizes.bottom as f32)).w_full().flex_none().mt(gap));
                div()
                    .size_full()
                    .flex()
                    // Without a right stripe the islands keep the gap it would have given them.
                    .when(right_stripe.is_none(), |el| el.pr(gap))
                    // Without a status bar the islands keep their distance from the window's edge.
                    .when(self.editor().is_none(), |el| el.pb(gap))
                    .child(stripe(Anchor::Left))
                    .child(div().flex_1().min_w_0().h_full().flex().flex_col().child(top).children(bottom))
                    .children(right_stripe)
                    .into_any_element()
            }
        };
        let framed = self.project_tree().is_some();
        div()
            .size_full()
            .bg(theme::color(cx, if framed { "bg-frame" } else { "bg-app" }))
            .text_color(theme::color(cx, "fg"))
            .text_size(theme::length(cx, "fs-ui"))
            // Modifier taps and mouse buttons have no application-level hook: forward them.
            .on_modifiers_changed(cx.listener(|_, event: &ModifiersChangedEvent, window, cx| {
                input::modifiers_changed(&event.modifiers, window, cx);
            }))
            .capture_any_mouse_down(cx.listener(|_, event: &MouseDownEvent, window, cx| {
                if input::mouse_down(event.button, &event.modifiers, false, window, cx).consumed() {
                    cx.stop_propagation();
                }
            }))
            .flex()
            .flex_col()
            .child(
                // The title bar: the window's title on the frame colour; dragging it moves the window.
                div()
                    .h(theme::length(cx, "h-titlebar"))
                    .flex_none()
                    .flex()
                    .items_center()
                    .pl(gpui_kit::px(if cfg!(target_os = "macos") { WINDOW_BUTTONS_WIDTH } else { 10.0 }))
                    .pr(gpui_kit::px(8.0))
                    .text_color(theme::color(cx, "fg-strong"))
                    .window_control_area(gpui_kit::WindowControlArea::Drag)
                    .child(SharedString::from(self.shown_title.clone().unwrap_or_default()))
                    .child(div().flex_1())
                    // The IDE mode of the window's project; a click switches it.
                    .children(framed.then(|| {
                        let full = self.ide_mode(cx).is_full();
                        let i18n = &cx.default_global::<crate::platform::i18n::Localization>().0;
                        let label = i18n.tx(if full { "Deep mode" } else { "Blaze mode" });
                        let color = theme::color(cx, "fg-strong");
                        div()
                            .id("title:app.ideMode")
                            .h(gpui_kit::px(30.0))
                            .flex_none()
                            .flex()
                            .items_center()
                            .gap(gpui_kit::px(5.0))
                            .px(gpui_kit::px(8.0))
                            .rounded(theme::length(cx, "radius-toolbar-button"))
                            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                            .child(theme::icon(cx, if full { "ide-mode-full" } else { "ide-mode-light" }, 16.0, color))
                            .child(SharedString::from(label))
                            .on_click(cx.listener(|_, _, window, cx| {
                                execute_action_from(ActionSource::Toolbar, "ide.toggleMode", None, window, cx);
                            }))
                    })),
            )
            .child(div().flex_1().min_h_0().w_full().child(body))
            .children((self.editor().is_some() || self.task_status().is_some()).then(|| {
                // The status bar: plain UI text, items right-aligned as in the webview's footer.
                div()
                    .h(theme::length(cx, "h-statusbar") + gpui_kit::px(4.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(gpui_kit::px(16.0))
                    .pl(gpui_kit::px(12.0))
                    .pr(gpui_kit::px(10.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .children(self.task_status().map(SharedString::from))
                    .children(self.status_items(cx).into_iter().map(SharedString::from))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{TestAppContext, VisualTestContext};
    use std::time::Duration;

    fn start(cx: &mut TestAppContext) -> tempfile::TempDir {
        cx.executor().allow_parking();
        let home = tempfile::tempdir().expect("a temporary home");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
            crate::editor::actions::register(cx);
            register_actions(cx);
            cx.set_global(UiStorage::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        home
    }

    /// View › Toggle Light/Dark Theme switches every window and stores the choice.
    #[gpui_kit::test]
    async fn the_theme_toggle_action_switches_and_stores_the_theme(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        cx.update(crate::theme::appearance::init);
        let (_workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        cx.update(|window, cx| execute_action_or_throw("view.toggleTheme", None, window, cx)).expect("it runs");
        assert!(cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
        assert_eq!(cx.update(|_, cx| crate::platform::settings::global(cx).appearance.theme), "dark");
        cx.update(|window, cx| execute_action_or_throw("view.toggleTheme", None, window, cx)).expect("it runs");
        assert!(!cx.update(|_, cx| cx.global::<theme::ActiveTheme>().is_dark()));
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    #[gpui_kit::test]
    async fn opens_a_loose_file_in_an_editor_and_types_into_it(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        let editor = workbench
            .read_with(cx, |w, _| w.editor().cloned())
            .unwrap_or_else(|| panic!("{:?}", workbench.read_with(cx, |w, _| w.failure().map(str::to_owned))));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        // The editor has focus: typing goes to the document without a click.
        cx.simulate_input("oat ");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    /// ⌘S is `file.save` in the default keymap: the keystroke goes through the dispatcher, the
    /// registry and the editor to the backend, which writes the file.
    #[gpui_kit::test]
    async fn the_keymap_runs_editor_actions_on_the_windows_editor(cx: &mut TestAppContext) {
        use crate::platform::actions::{ActionSource, execute_action_from};
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("todo.txt");
        std::fs::write(&file, "milk\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // Without an editor the action has nothing to act on, and says so instead of failing silently.
        assert!(
            cx.update(|window, cx| crate::platform::actions::execute_action_or_throw("file.save", None, window, cx))
                .is_err()
        );
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");

        cx.simulate_input("oat ");
        cx.simulate_keystrokes("cmd-s");
        settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text == "oat milk\n"));

        // The Edit menu's actions act on the same editor.
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.undo", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "milk\n");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.redo", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.selectAll", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.cut", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "");
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.paste", None, window, cx)));
        assert!(cx.update(|window, cx| execute_action_from(ActionSource::Menu, "edit.copy", None, window, cx)));
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "oat milk\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    #[gpui_kit::test]
    async fn a_project_shows_its_tree_and_opens_files_in_the_editor(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(folder.path().join("main.zig"), "const a = 1;\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.rows().len() == 2));
        // The tree has focus after a project opens: Down, Down, Enter opens the file.
        cx.simulate_keystrokes("down down enter");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "const a = 1;\n");
        // The window is titled `<project> – <file>`.
        let root = folder.path().canonicalize().expect("the folder exists");
        let project_name = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(cx.window_title(), Some(format!("{project_name} – main.zig")));
        // The facts `when` clauses read follow what is open and focused.
        let truthy = |cx: &mut VisualTestContext, key: &str| {
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
        };
        for key in ["projectOpen", "textEditorActive", "editorFocus", "editorTextFocus"] {
            assert!(truthy(cx, key), "{key}");
        }
        for key in ["lightEdit", "editorHasSelection", "toolWindowFocus", "projectOpening", "editorReadOnly"] {
            assert!(!truthy(cx, key), "{key}");
        }
        let language =
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).get("editorLangId").cloned());
        assert_eq!(language, Some(crate::platform::context::ContextValue::from("zig")));
        cx.simulate_keystrokes("cmd-a");
        cx.run_until_parked();
        assert!(truthy(cx, "editorHasSelection"));
        cx.simulate_keystrokes("left");
        // The editor takes focus, so typing edits the file that was opened.
        cx.simulate_input("// ");
        assert_eq!(editor.read_with(cx, |e, _| e.text()), "// const a = 1;\n");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/editor/core/view/statusItems.test.tsx (caret, line separator and encoding widgets)
    #[gpui_kit::test]
    async fn the_status_bar_follows_the_editors_caret_and_selection(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let file = folder.path().join("notes.txt");
        std::fs::write(&file, "ab\r\ncd\r\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        assert!(workbench.read_with(cx, |w, cx| w.status_items(cx)).is_empty());
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        let items = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.status_items(cx));
        assert_eq!(items(cx), ["1:1", "CRLF", "UTF-8"]);
        cx.simulate_keystrokes("down right");
        assert_eq!(items(cx), ["2:2", "CRLF", "UTF-8"]);
        cx.simulate_keystrokes("shift-right");
        assert_eq!(items(cx), ["2:3 (1 chars)", "CRLF", "UTF-8"]);
        // A line break is one character of the selection, whatever the file's line separator.
        cx.simulate_keystrokes("cmd-a");
        assert_eq!(items(cx)[0], "3:1 (6 chars, 2 line breaks)");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/workbench/toolwindows/store.test.ts › registers toolwindow.<id> and generic actions
    #[gpui_kit::test]
    async fn tool_window_actions_show_focus_and_hide_the_project_view(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        std::fs::write(folder.path().join("main.zig"), "const a = 1;\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let tree = workbench.read_with(cx, |w, _| w.project_tree().cloned()).expect("a tree");
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.rows().len() == 2));
        let shown = |cx: &mut VisualTestContext| {
            workbench.read_with(cx, |w, _| w.tool_windows().expect("a layout").is_visible("project"))
        };
        let truthy = |cx: &mut VisualTestContext, key: &str| {
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy(key))
        };
        let tree_focused =
            |cx: &mut VisualTestContext| cx.update(|window, cx| tree.focus_handle(cx).is_focused(window));
        // A project opens with its Project view shown and focused.
        assert!(shown(cx) && tree_focused(cx));
        assert!(truthy(cx, "toolWindow.project.visible") && truthy(cx, "toolWindowFocus"));

        // ⌘1 on the focused window hides it; again shows and focuses it.
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(!shown(cx) && !tree_focused(cx));
        assert!(!truthy(cx, "toolWindow.project.visible") && !truthy(cx, "toolWindowFocus"));
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));

        // With an editor open, Escape in the tool window goes to the editor and ⌘1 comes back.
        cx.simulate_keystrokes("down down enter");
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_keystrokes("cmd-1");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(shown(cx) && !tree_focused(cx) && truthy(cx, "editorFocus"));
        // Hide All Tool Windows hides it; Jump to Last Tool Window (F12) brings it back, focused.
        cx.simulate_keystrokes("cmd-shift-f12");
        cx.run_until_parked();
        assert!(!shown(cx));
        cx.simulate_keystrokes("f12");
        cx.run_until_parked();
        assert!(shown(cx) && tree_focused(cx));

        // The layout is stored under the webview's key for the project folder, in its JSON shape.
        let scope = folder.path().canonicalize().expect("the folder exists").to_string_lossy().into_owned();
        let stored =
            cx.update(|_, cx| cx.global::<UiStorage>().get(&storage_key(&scope)).map(str::to_owned)).expect("stored");
        let stored: serde_json::Value = serde_json::from_str(&stored).expect("JSON");
        assert_eq!(stored["v"], 2);
        assert_eq!(stored["visible"]["left"], "project");
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/editor/core/model/model.test.ts (tabs), driven through the window and the keymap
    #[gpui_kit::test]
    async fn files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("a")).expect("a folder");
        std::fs::create_dir(root.join("b")).expect("a folder");
        for (file, text) in [("a/mod.rs", "// a\n"), ("b/mod.rs", "// b\n"), ("notes.txt", "notes\n")] {
            std::fs::write(root.join(file), text).expect("the file is written");
        }
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let open = |cx: &mut VisualTestContext, file: &str| {
            let path = root.join(file);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path, window, cx));
            let expected = if file == "notes.txt" {
                "notes\n"
            } else if file.starts_with('a') {
                "// a\n"
            } else {
                "// b\n"
            };
            settle(cx, |cx| workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text() == expected)));
        };
        let titles = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.tab_titles());
        let active = |cx: &mut VisualTestContext| {
            titles(cx).into_iter().find(|title| title.active).map(|title| title.name).unwrap_or_default()
        };
        let tab = |name: &str, hint: Option<&str>, active: bool| TabTitle {
            name: name.to_owned(),
            hint: hint.map(str::to_owned),
            active,
        };
        let text = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text()));

        open(cx, "a/mod.rs");
        assert_eq!(titles(cx), [tab("mod.rs", None, true)]);
        open(cx, "notes.txt");
        open(cx, "b/mod.rs");
        // Two files of the same name are told apart by their folders.
        assert_eq!(
            titles(cx),
            [tab("mod.rs", Some("a"), false), tab("notes.txt", None, false), tab("mod.rs", Some("b"), true)]
        );
        assert_eq!(text(cx).as_deref(), Some("// b\n"));
        // Opening a file that has a tab brings that tab to the front instead of adding one.
        open(cx, "notes.txt");
        assert_eq!((titles(cx).len(), active(cx)), (3, "notes.txt".to_owned()));

        // Ctrl+Right / Ctrl+Left cycle through the tabs, wrapping around.
        cx.simulate_keystrokes("ctrl-right");
        cx.run_until_parked();
        assert_eq!(text(cx).as_deref(), Some("// b\n"));
        cx.simulate_keystrokes("ctrl-right");
        cx.run_until_parked();
        assert_eq!(text(cx).as_deref(), Some("// a\n"));
        cx.simulate_keystrokes("ctrl-left ctrl-left");
        cx.run_until_parked();
        assert_eq!(active(cx), "notes.txt");

        // ⌘W closes the active tab, saving what was typed into it; the editor of a neighbour takes over.
        cx.simulate_input("more ");
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        assert_eq!(titles(cx).len(), 2);
        assert!(text(cx).is_some());
        settle(cx, |_| std::fs::read_to_string(root.join("notes.txt")).is_ok_and(|text| text == "more notes\n"));
        // Reopen Closed Tab brings it back where it was, with the saved text.
        cx.update(|window, cx| crate::platform::actions::execute_action("editor.reopenClosedTab", None, window, cx));
        settle(cx, |cx| {
            workbench.read_with(cx, |w, cx| w.editor().is_some_and(|e| e.read(cx).text() == "more notes\n"))
        });
        assert_eq!((titles(cx).len(), active(cx)), (3, "notes.txt".to_owned()));

        // Close Other Tabs, then closing the last one leaves an empty editor area.
        cx.update(|window, cx| crate::platform::actions::execute_action("editor.closeOtherTabs", None, window, cx));
        cx.run_until_parked();
        assert_eq!(titles(cx), [tab("notes.txt", None, true)]);
        cx.simulate_keystrokes("cmd-w");
        cx.run_until_parked();
        assert!(titles(cx).is_empty() && text(cx).is_none());
        let truthy =
            cx.update(|window, cx| crate::platform::context::window_context(window, cx).is_truthy("editorTabsOpen"));
        assert!(!truthy);
        // Every closed tab released its document in the backend.
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        settle(cx, |_| host.project("w1").is_ok_and(|project| project.documents.open_documents().is_empty()));
        host.close_project("w1");
    }

    // src/workbench/Workbench.test.tsx › background tasks › tracks task events and shows the current task
    #[gpui_kit::test]
    async fn backend_tasks_and_notifications_reach_the_window(cx: &mut TestAppContext) {
        use ide_core::TaskId;
        let _home = start(cx);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        let send = |cx: &mut VisualTestContext, window: Option<&str>, event: IdeEvent| {
            let event = BackendEvent { window: window.map(str::to_owned), event };
            workbench.update(cx, |workbench, cx| workbench.backend_event(&event, cx));
        };
        let task = |done: bool, message: &str| IdeEvent::Task {
            id: TaskId(7),
            title: "Importing".to_owned(),
            message: Some(message.to_owned()),
            fraction: Some(0.5),
            done,
            cancellable: true,
        };
        let status = |cx: &mut VisualTestContext| workbench.read_with(cx, |w, _| w.task_status());
        // Indexing is listed at once, with its counts; a second task is counted behind it.
        send(cx, Some("w1"), IdeEvent::IndexState { indexing: true, files: 1200, symbols: 34 });
        assert_eq!(status(cx).as_deref(), Some("Indexing: 1,200 files · 34 symbols"));
        // A short task is not shown unless it outlives the show delay; another window's is ignored.
        send(cx, Some("w1"), task(false, "module a"));
        send(cx, Some("other"), IdeEvent::IndexState { indexing: false, files: 0, symbols: 0 });
        assert_eq!(workbench.read_with(cx, |w, _| w.tasks().tasks().len()), 1);
        workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms() + tasks::SHOW_DELAY_MS + 1.0, cx));
        assert_eq!(status(cx).as_deref(), Some("Indexing: 1,200 files · 34 symbols (+1)"));
        // Indexing ends and leaves after its linger; the import remains.
        send(cx, Some("w1"), IdeEvent::IndexState { indexing: false, files: 1200, symbols: 34 });
        workbench.update(cx, |workbench, cx| workbench.tick_tasks(now_ms() + tasks::DONE_LINGER_MS + 1.0, cx));
        assert_eq!(status(cx).as_deref(), Some("Importing: module a"));

        // A notification for every window (no window named) is kept in this one's list.
        send(
            cx,
            None,
            IdeEvent::Notify { level: NotifyLevel::Warning, title: "Disk is almost full".to_owned(), body: None },
        );
        let titles: Vec<String> =
            workbench.read_with(cx, |w, _| w.notifications().items().iter().map(|n| n.title.clone()).collect());
        assert_eq!(titles, ["Disk is almost full"]);
    }

    // src/features/app/closeGuard.ts › decideClose (the unsaved-changes question)
    #[gpui_kit::test]
    async fn closing_a_window_asks_about_unsaved_changes(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        // Nothing unsaved: the window may close without a question.
        assert!(workbench.update_in(cx, |workbench, window, cx| workbench.confirm_close(window, cx)));

        workbench
            .update_in(cx, |workbench, window, cx| workbench.open_project_file(root.join("notes.txt"), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_input("my ");
        // Typed a moment ago: the document counts as unsaved before the backend has said so.
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);
        assert!(!cx.simulate_close());
        let (title, body) = cx.pending_prompt().expect("the question is asked");
        assert_eq!(title, "Unsaved Changes");
        let project = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(body, format!("1 file has unsaved changes in {project}. Save before closing?"));
        // Cancel: the window and the text stay, nothing is written.
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(
            workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(),
            Some("my notes\n")
        );
        assert_eq!(std::fs::read_to_string(root.join("notes.txt")).expect("readable"), "notes\n");

        // Save All writes the file, then the window closes.
        assert!(!cx.simulate_close());
        cx.simulate_prompt_answer("Save All");
        settle(cx, |_| std::fs::read_to_string(root.join("notes.txt")).is_ok_and(|text| text == "my notes\n"));
        settle(cx, |cx| cx.windows().is_empty());
    }

    /// A tab whose document cannot be saved comes back with its text, and the failure is reported.
    #[cfg(unix)]
    #[gpui_kit::test]
    async fn a_tab_that_cannot_be_saved_reopens_with_its_text(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::create_dir(root.join("locked")).expect("a folder");
        let file = root.join("locked/notes.txt");
        std::fs::write(&file, "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(file.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
        cx.simulate_input("my ");
        // Neither the file nor its folder can be written any more.
        let lock = |mode: u32, path: &std::path::Path| {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("permissions change")
        };
        lock(0o444, &file);
        lock(0o555, &root.join("locked"));

        cx.simulate_keystrokes("cmd-w");
        settle(cx, |cx| workbench.read_with(cx, |w, _| !w.notifications().items().is_empty()));
        workbench.read_with(cx, |w, cx| {
            assert_eq!(w.notifications().items()[0].title, "Could not save all files");
            assert_eq!(w.tab_titles().len(), 1, "the tab is back");
            assert_eq!(w.editor().map(|e| e.read(cx).text()).as_deref(), Some("my notes\n"));
            assert_eq!(w.unsaved_count(cx), 1);
        });
        assert_eq!(std::fs::read_to_string(&file).expect("readable"), "notes\n");

        // Once the file can be written again, saving succeeds and nothing was lost.
        lock(0o755, &root.join("locked"));
        lock(0o644, &file);
        cx.simulate_keystrokes("cmd-s");
        settle(cx, |_| std::fs::read_to_string(&file).is_ok_and(|text| text == "my notes\n"));
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }

    // src/features/app/title.ts
    /// With Files › Autosave off nothing is saved behind the user's back: leaving a tab keeps
    /// it unsaved, and closing it asks. Cancel brings the tab back, Don't Save drops the edits,
    /// Save All writes them.
    #[gpui_kit::test]
    async fn without_autosave_closing_an_unsaved_tab_asks(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("a.txt"), "a\n").expect("the file is written");
        std::fs::write(root.join("b.txt"), "b\n").expect("the file is written");
        cx.update(|cx| {
            crate::platform::settings::set("w1", SettingsScope::Global, "files.autoSave", &false.into(), cx)
                .expect("the setting is stored");
        });
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let open = |name: &str, cx: &mut VisualTestContext| {
            let path = root.join(name);
            workbench.update_in(cx, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
            settle(cx, |cx| {
                workbench.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
            });
        };
        let on_disk = |name: &str| std::fs::read_to_string(root.join(name)).expect("readable");
        let close = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| execute_action_or_throw("editor.closeTab", None, window, cx)).expect("it runs");
        };
        let names = |cx: &mut VisualTestContext| -> Vec<String> {
            workbench.read_with(cx, |w, _| w.tab_titles().into_iter().map(|tab| tab.name).collect())
        };

        open("a.txt", cx);
        cx.simulate_input("my ");
        // Leaving the tab does not save it.
        open("b.txt", cx);
        cx.run_until_parked();
        assert_eq!(on_disk("a.txt"), "a\n");
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 1);

        // Closing it asks; Cancel brings the tab back with its text.
        open("a.txt", cx);
        close(cx);
        let (title, body) = cx.pending_prompt().expect("the question is asked");
        assert_eq!(title, "Unsaved Changes");
        let path = root.join("a.txt").to_string_lossy().into_owned();
        assert_eq!(body, format!("1 file has unsaved changes in {path}. Save before closing?"));
        cx.simulate_prompt_answer("Cancel");
        cx.run_until_parked();
        assert_eq!(names(cx), ["b.txt", "a.txt"]);
        assert_eq!(workbench.read_with(cx, |w, cx| w.editor().map(|e| e.read(cx).text())).as_deref(), Some("my a\n"));
        assert_eq!(on_disk("a.txt"), "a\n");

        // Save All writes it and the tab stays closed.
        close(cx);
        cx.simulate_prompt_answer("Save All");
        settle(cx, |_| on_disk("a.txt") == "my a\n");
        assert_eq!(names(cx), ["b.txt"]);

        // Don't Save drops the edits.
        cx.simulate_input("their ");
        close(cx);
        cx.simulate_prompt_answer("Don't Save");
        cx.run_until_parked();
        assert!(names(cx).is_empty());
        assert_eq!(on_disk("b.txt"), "b\n");
        assert_eq!(workbench.read_with(cx, |w, cx| w.unsaved_count(cx)), 0);
    }

    /// The Terminal docks at the bottom: its action shows it with a shell of the project's
    /// folder and the focus in it, typing reaches the shell, its stripe button is at the foot
    /// of the left stripe, and hiding it keeps the shell running for the next time.
    #[gpui_kit::test]
    async fn the_terminal_tool_window_runs_a_shell_at_the_bottom(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::terminal::view::TerminalView;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        workbench.read_with(cx, |workbench, _| {
            let Content::Project { layout, .. } = &workbench.content else { panic!("a project is open") };
            assert_eq!(layout.slot_of(TERMINAL_TOOL_WINDOW), Some(Slot::Bottom));
            assert!(!workbench.is_tool_window_visible(TERMINAL_TOOL_WINDOW));
            assert!(workbench.tool_window_view(TERMINAL_TOOL_WINDOW).is_none(), "no shell before it is shown");
        });

        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        let terminal = workbench
            .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
            .and_then(|view| view.downcast::<TerminalView>().ok())
            .expect("the terminal has a view");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert_eq!(terminal.read_with(cx, |terminal, _| terminal.error().map(str::to_owned)), None);
        let long = |cx: &mut VisualTestContext, done: &dyn Fn(&mut VisualTestContext) -> bool| {
            for _ in 0..3000 {
                cx.run_until_parked();
                if done(cx) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            panic!("the shell did not answer within thirty seconds");
        };
        let screen = |cx: &mut VisualTestContext| terminal.read_with(cx, |t, _| t.emulator().lines().join("\n"));
        // The focus is in the terminal: typing goes to the shell, which starts in the project.
        cx.simulate_input("echo in-$(basename \"$PWD\")-$((6*7))");
        cx.simulate_keystrokes("enter");
        let expected = format!("in-{}-42", root.file_name().expect("a name").to_string_lossy());
        long(cx, &|cx| screen(cx).contains(&expected));
        assert!(cx.update(|window, cx| {
            crate::platform::context::window_context(window, cx).is_truthy("toolWindowFocus")
        }));

        // The same action hides the focused window; the shell stays, and shows again as it was.
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        assert!(!workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert!(terminal.read_with(cx, |terminal, _| terminal.id().is_some()), "hidden, not ended");
        cx.update(|window, cx| execute_action_or_throw("toolwindow.terminal", None, window, cx)).expect("it runs");
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(TERMINAL_TOOL_WINDOW)));
        assert!(screen(cx).contains(&expected));
        let again = workbench
            .read_with(cx, |w, _| w.tool_window_view(TERMINAL_TOOL_WINDOW))
            .and_then(|view| view.downcast::<TerminalView>().ok())
            .expect("the terminal has a view");
        assert_eq!(again.entity_id(), terminal.entity_id(), "the same session");
    }

    /// Blaze and Deep mode: the toggle writes `ide.mode` for every project and the backend
    /// applies it to this one; a project that names the mode itself is switched too.
    #[gpui_kit::test]
    async fn the_ide_mode_toggle_switches_between_blaze_and_deep(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::platform::settings;
        use ide_core::settings::IdeMode;
        use ide_workspace::settings::SettingsScope;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        let toggle = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| execute_action_or_throw("ide.toggleMode", None, window, cx)).expect("it runs");
        };
        let backend_mode = |cx: &mut VisualTestContext| {
            cx.update(|_, cx| cx.global::<Backend>().host.project("w1").map(|p| p.ide_mode()))
        };
        let toggled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let action = cx.global::<ActionRegistry>().get("ide.toggleMode").expect("registered");
                action.is_toggled(window, cx)
            })
        };
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Light);
        assert_eq!(toggled(cx), Some(false));

        toggle(cx);
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Full);
        assert_eq!(cx.update(|_, cx| settings::global(cx).ide.mode), IdeMode::Full);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Full));
        assert_eq!(toggled(cx), Some(true));
        toggle(cx);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Light));

        // The project says Blaze itself: the global switch alone would change nothing here.
        cx.update(|_, cx| {
            settings::set("w1", SettingsScope::Project, "ide.mode", &"light".into(), cx).expect("stored");
        });
        toggle(cx);
        assert_eq!(cx.update(|_, cx| settings::global(cx).ide.mode), IdeMode::Full);
        assert_eq!(workbench.read_with(cx, |w, cx| w.ide_mode(cx)), IdeMode::Full);
        assert_eq!(backend_mode(cx).ok(), Some(IdeMode::Full));
    }

    /// A project's tabs come back when it is opened again: the same files in the same order,
    /// the same one active with its selection and scroll position, and the others loaded only
    /// when they are shown.
    #[gpui_kit::test]
    async fn a_projects_tabs_and_carets_are_restored(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        let long: String = (1..=200).map(|n| format!("line {n}\n")).collect();
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(root.join(name), &long).expect("the file is written");
        }
        let scope = root.to_string_lossy().into_owned();
        let key = format!("{}{scope}", session::STORAGE_PREFIX);
        let (first, cx1) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        first.update_in(cx1, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx1, |cx| first.read_with(cx, |w, _| w.project_tree().is_some()));
        assert_eq!(cx1.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned)), None);
        for name in ["a.txt", "b.txt"] {
            let path = root.join(name);
            first.update_in(cx1, |workbench, window, cx| workbench.open_project_file(path.clone(), window, cx));
            settle(cx1, |cx| {
                first.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == path))
            });
        }
        let editor = first.read_with(cx1, |w, _| w.editor().cloned()).expect("b.txt is showing");
        editor.update(cx1, |editor, cx| {
            editor.select(700, 706, cx);
            editor.scroll_to_line(90, cx);
        });
        // Leaving the window writes the session as it stands.
        first.update(cx1, |workbench, cx| workbench.save_session(cx));
        let stored: serde_json::Value = cx1
            .update(|_, cx| serde_json::from_str(cx.global::<UiStorage>().get(&key).expect("stored")))
            .expect("JSON");
        let b = root.join("b.txt").to_string_lossy().into_owned();
        assert_eq!(stored["layout"]["active"], 1);
        assert_eq!(stored["layout"]["tabs"][1]["path"], b.as_str());
        assert_eq!(stored["files"][b.as_str()], serde_json::json!({ "anchor": 700, "head": 706, "topLine": 90 }));
        cx1.update(|_, cx| cx.global::<Backend>().host.close_project("w1"));

        let (second, cx2) = cx.add_window_view(|window, cx| Workbench::new("w2", window, cx));
        second.update_in(cx2, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx2, |cx| second.read_with(cx, |w, _| w.editor().is_some()));
        let names: Vec<(String, bool)> =
            second.read_with(cx2, |w, _| w.tab_titles().into_iter().map(|tab| (tab.name, tab.active)).collect());
        assert_eq!(names, [("a.txt".to_owned(), false), ("b.txt".to_owned(), true)]);
        let restored = second.read_with(cx2, |w, _| w.editor().cloned()).expect("an editor");
        // Restoring a tab does not take the keyboard: it is in the Project view, as on any open.
        cx2.run_until_parked();
        assert!(cx2.update(|window, cx| {
            let tree = second.read(cx).project_tree().expect("a tree").focus_handle(cx);
            tree.is_focused(window) && !restored.focus_handle(cx).is_focused(window)
        }));
        restored.read_with(cx2, |editor, cx| {
            assert_eq!(editor.path(), root.join("b.txt"));
            assert_eq!((editor.anchor(), editor.caret(), editor.top_line(cx)), (700, 706, 90));
        });
        // Only the tab that shows has a document; the other gets its own when it is shown.
        let open_documents = |cx: &mut VisualTestContext| {
            second.read_with(cx, |w, _| match &w.content {
                Content::Project { editors, .. } => editors.len(),
                _ => 0,
            })
        };
        assert_eq!(open_documents(cx2), 1);
        // Tab switching belongs to the editor: with the keyboard there, the previous tab shows.
        cx2.update(|window, cx| {
            let focus = restored.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx2.run_until_parked();
        cx2.update(|window, cx| execute_action_or_throw("editor.prevTab", None, window, cx)).expect("it runs");
        settle(cx2, |cx| {
            second.read_with(cx, |w, cx| w.editor().is_some_and(|editor| editor.read(cx).path() == root.join("a.txt")))
        });
        assert_eq!(open_documents(cx2), 2);

        // Closing every tab leaves no session behind once the change has rested.
        cx2.update(|window, cx| execute_action_or_throw("editor.closeAllTabs", None, window, cx)).expect("it runs");
        cx2.executor().advance_clock(SESSION_SAVE_DELAY);
        cx2.run_until_parked();
        assert_eq!(cx2.update(|_, cx| cx.global::<UiStorage>().get(&key).map(str::to_owned)), None);
    }

    /// The Git tool window docks at the bottom and holds the project's log, read when the
    /// window is first shown.
    #[gpui_kit::test]
    async fn the_git_tool_window_shows_the_projects_log(cx: &mut TestAppContext) {
        use crate::platform::actions::execute_action_or_throw;
        use crate::vcs::log_view::GitLog;
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let root = folder.path().canonicalize().expect("the folder exists");
        std::fs::write(root.join("notes.txt"), "notes\n").expect("the file is written");
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git runs");
            assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "."]);
        git(&["-c", "user.name=Ada", "-c", "user.email=ada@example.test", "commit", "-q", "-m", "notes"]);
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        workbench.update_in(cx, |workbench, window, cx| workbench.open_project(root.clone(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.project_tree().is_some()));
        assert!(
            workbench.read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW).is_none()),
            "not read before it shows"
        );
        cx.update(|window, cx| execute_action_or_throw("toolwindow.git", None, window, cx)).expect("it runs");
        let log = workbench
            .read_with(cx, |w, _| w.tool_window_view(GIT_TOOL_WINDOW))
            .and_then(|view| view.downcast::<GitLog>().ok())
            .expect("the Git window holds the log");
        workbench.read_with(cx, |workbench, _| {
            let Content::Project { layout, .. } = &workbench.content else { panic!("a project is open") };
            assert_eq!(layout.slot_of(GIT_TOOL_WINDOW), Some(Slot::Bottom));
            assert!(workbench.is_tool_window_visible(GIT_TOOL_WINDOW));
        });
        settle(cx, |cx| log.read_with(cx, |log, _| !log.is_loading()));
        let subjects: Vec<String> =
            log.read_with(cx, |log, _| log.commits().iter().map(|c| c.summary.clone()).collect());
        assert_eq!(subjects, ["notes"]);
        assert!(cx.update(|window, cx| log.focus_handle(cx).is_focused(window)), "showing it gives it the keyboard");
    }

    #[test]
    fn titles_name_the_project_then_the_file() {
        let file = std::path::Path::new("/work/shop/src/main.rs");
        assert_eq!(window_title(Some("shop"), Some(file), false, None, "OpenIDEA"), "shop – main.rs");
        assert_eq!(window_title(Some("shop"), Some(file), true, None, "OpenIDEA"), "shop – /work/shop/src/main.rs");
        assert_eq!(window_title(Some("shop"), None, false, Some("me@box"), "OpenIDEA"), "shop [me@box]");
        assert_eq!(
            window_title(Some("shop"), Some(file), false, Some("me@box"), "OpenIDEA"),
            "shop [me@box] – main.rs"
        );
        assert_eq!(window_title(None, Some(file), false, None, "OpenIDEA"), "main.rs");
        assert_eq!(window_title(None, None, false, None, "OpenIDEA"), "OpenIDEA");
        assert_eq!(window_title(Some(""), None, false, None, "OpenIDEA"), "OpenIDEA");
    }

    #[gpui_kit::test]
    async fn a_file_that_cannot_open_is_a_message_in_the_window(cx: &mut TestAppContext) {
        let _home = start(cx);
        let folder = tempfile::tempdir().expect("a temporary folder");
        let (workbench, cx) = cx.add_window_view(|window, cx| Workbench::new("w1", window, cx));
        // A folder is not a file to edit.
        workbench.update_in(cx, |workbench, window, cx| workbench.open_file(folder.path().to_path_buf(), window, cx));
        settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() || w.failure().is_some()));
        workbench.read_with(cx, |w, _| {
            assert!(w.editor().is_none());
            assert!(w.failure().is_some_and(|message| message.contains("is a folder")), "{:?}", w.failure());
        });
    }
}
