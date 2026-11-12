//! The window shell. For now: a frame around one editor (LightEdit), which is the part of the
//! workbench the editor slice needs. Tool windows, the title bar and the status bar follow.

pub mod main_toolbar;
pub mod notifications;
pub mod project_color;
pub mod quick_switch;
pub mod status_bar_widgets;
pub mod tasks;
pub mod tool_windows;

use crate::backend::Backend;
use crate::editor::tabs::{EditorInput, OpenTabOptions, TabsState, disambiguate_titles};
use crate::editor::view::Editor;
use crate::platform::actions::{Action, ActionMeta, ActionRegistry, ActionSource, execute_action_from, manifest};
use crate::platform::input;
use crate::platform::storage::UiStorage;
use crate::project_tree::{OpenFile, ProjectTree};
use crate::theme;
use gpui_kit::Subscription;
use gpui_kit::{
    App, Context, Entity, Focusable, Global, IntoElement, ModifiersChangedEvent, MouseDownEvent, Render, SharedString,
    WeakEntity, Window, WindowId, div, prelude::*,
};
use std::collections::HashMap;
use std::path::PathBuf;
use tool_windows::{Anchor, Effects, Slot, Stripe, ToolWindow, ToolWindowLayout, storage_key};

/// What a window shows.
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
        /// The editor tabs, and the editor of each tab that has finished opening, by tab id.
        tabs: Box<TabsState>,
        editors: HashMap<String, Entity<Editor>>,
        _opens: Subscription,
    },
    Failed(SharedString),
}

/// One window: its backend label and what it shows.
pub struct Workbench {
    /// The label the backend knows this window by.
    label: String,
    content: Content,
    /// Redraws the status bar when the shown editor changes.
    editor_observer: Option<Subscription>,
    /// The title the window currently carries.
    shown_title: Option<String>,
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
    for id in [PROJECT_TOOL_WINDOW] {
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
    pub fn new(label: &str, window: &Window, cx: &mut Context<Self>) -> Self {
        let id = window.window_handle().window_id();
        let this = cx.weak_entity();
        cx.default_global::<Workbenches>().0.insert(id, this);
        cx.on_release(move |_, cx| {
            cx.default_global::<Workbenches>().0.remove(&id);
            input::window_blurred(id, cx);
            crate::platform::context::drop_window_context(id, cx);
        })
        .detach();
        Self { label: label.to_owned(), content: Content::Empty, editor_observer: None, shown_title: None }
    }

    pub fn label(&self) -> &str {
        &self.label
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
        let Content::Project { tabs, editors, .. } = &mut self.content else { return };
        let open: std::collections::HashSet<&str> =
            tabs.panes.iter().flat_map(|pane| pane.tabs.iter().map(|tab| tab.id.as_str())).collect();
        let closed: Vec<String> = editors.keys().filter(|id| !open.contains(id.as_str())).cloned().collect();
        for id in closed {
            let Some(editor) = editors.remove(&id) else { continue };
            // Closing never loses edits: an unsaved document is written before it is released.
            if editor.read(cx).is_dirty() {
                let saving = editor.read(cx).save(cx);
                cx.spawn(async move |_, _| {
                    if !matches!(saving.await, Ok(Ok(()))) {
                        tracing::error!("a closed tab could not be saved");
                    }
                    drop(editor);
                })
                .detach();
            }
        }
        match self.editor().cloned() {
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
        let tree_focused = self.project_tree().is_some_and(|tree| tree.focus_handle(cx).is_focused(window));
        set_context(window, cx, "toolWindowFocus", tree_focused);
        if let Content::Project { layout, .. } = &mut self.content {
            // The layout decides by this what `toolwindow.<id>` does: focus, or hide.
            layout.set_focused(tree_focused.then_some(PROJECT_TOOL_WINDOW));
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
        let Content::Project { layout, focus_served, tree, .. } = &mut self.content else { return };
        let effects = change(layout);
        if effects.persist {
            match serde_json::to_string(&layout.serialized()) {
                Ok(json) => cx.default_global::<UiStorage>().set(&storage_key(layout.scope()), json),
                Err(error) => tracing::warn!("cannot store the tool window layout: {error}"),
            }
        }
        let request = layout.focus_request().filter(|request| request.seq > *focus_served).cloned();
        if let Some(request) = request {
            *focus_served = request.seq;
            if request.id == PROJECT_TOOL_WINDOW {
                let focus = tree.focus_handle(cx);
                window.focus(&focus, cx);
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
                        Content::Project {
                            name,
                            layout,
                            focus_served: 0,
                            tree,
                            tabs: Box::new(TabsState::new()),
                            editors: HashMap::new(),
                            _opens: opens,
                        }
                    }
                    Err(message) => Content::Failed(message.into()),
                };
                // Registering a window reconciles the stored layout with what exists; a project
                // opens with the focus in its Project view when that is showing.
                workbench.with_layout(window, cx, |layout| match tool_window_declaration(PROJECT_TOOL_WINDOW) {
                    Some(declared) => {
                        let (_, registered) = layout.register(declared);
                        let focused = if layout.is_visible(PROJECT_TOOL_WINDOW) {
                            layout.focus(PROJECT_TOOL_WINDOW)
                        } else {
                            Effects::default()
                        };
                        Effects {
                            persist: registered.persist || focused.persist,
                            focus_editor: registered.focus_editor || focused.focus_editor,
                        }
                    }
                    None => Effects::default(),
                });
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
            self.load_tab(tab_id, path, window, cx);
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
            self.load_tab(reopened.tab_id, PathBuf::from(path), window, cx);
        }
    }

    /// Opens the document of tab `tab_id` in the backend and gives the tab its editor.
    fn load_tab(&mut self, tab_id: String, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
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
                match text {
                    Ok((info, version, rope)) => {
                        let editor = cx.new(|cx| Editor::with_text(host, &label, &info, version, rope, cx));
                        // A tab closed while its file was opening has no use for the editor;
                        // dropping it releases the document.
                        if let (true, Content::Project { editors, .. }) = (still_open, &mut workbench.content) {
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
                workbench.tabs_changed(true, window, cx);
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
            Content::Project { tree, layout, .. } => {
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
                // The left stripe: one button per tool window docked on that side.
                let stripe = div()
                    .w(button + theme::length(cx, "stripe-outer-padding") + gap)
                    .h_full()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(theme::length(cx, "h-titlebar") - button)
                    .pl(theme::length(cx, "stripe-outer-padding"))
                    .pr(gap)
                    .children(layout.windows_in(Slot::Left).into_iter().filter(|w| layout.has_stripe_button(w)).map(
                        |declared| {
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
                        },
                    ));
                let project = layout.is_visible(PROJECT_TOOL_WINDOW).then(|| {
                    let title = layout.window(PROJECT_TOOL_WINDOW).map(|w| w.title.clone()).unwrap_or_default();
                    island()
                        .w(gpui_kit::px(layout.sizes().left as f32))
                        .h_full()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .mr(gap)
                        .child(
                            div()
                                .h(theme::length(cx, "h-header"))
                                .flex_none()
                                .flex()
                                .items_center()
                                .pl(gpui_kit::px(12.0))
                                .pr(gpui_kit::px(6.0))
                                .text_color(theme::color(cx, "fg-strong"))
                                .child(SharedString::from(title)),
                        )
                        .child(div().flex_1().min_h_0().child(tree.clone()))
                });
                div()
                    .size_full()
                    .flex()
                    .pr(gap)
                    // Without a status bar the islands keep their distance from the window's edge.
                    .when(self.editor().is_none(), |el| el.pb(gap))
                    .child(stripe)
                    .children(project)
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
                    .child(SharedString::from(self.shown_title.clone().unwrap_or_default())),
            )
            .child(div().flex_1().min_h_0().w_full().child(body))
            .children(self.editor().is_some().then(|| {
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
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.model().rows().len() == 2));
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
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.model().rows().len() == 2));
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

    // src/features/app/title.ts
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
