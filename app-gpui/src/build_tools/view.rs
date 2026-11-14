//! The Maven and Gradle tool windows' view: the toolbar, a banner (a sync in progress, build files
//! changed since the last reload, a failed sync) and the tree of the project's builds.
//!
//! The workbench feeds it ([`BuildToolView::set_input`]) with the build model, the run
//! configurations and the toolbar toggles; the view owns what is expanded and selected. What it
//! cannot do itself is a [`BuildToolEvent`]: run an action or a configuration, show a menu, change
//! a profile choice, reload a build.

use super::model::{
    BuildRow, BuildSettings, NodeKind, TreeSource, children, expandable_keys, gradle_project_key, initial_expanded,
    is_runnable, node_config, node_project, roots, visible_rows,
};
use crate::find::row_selection::selection_mode;
use crate::platform::actions::ActionRegistry;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_build::{BuildModel, BuildTool, MavenProfile};
use ide_run::RunConfig;
use serde_json::{Value, json};
use std::collections::HashSet;

/// A sync in progress, as the banner shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct SyncBanner {
    pub message: String,
    /// 0..1, or `None` when indeterminate.
    pub fraction: Option<f32>,
}

/// What the workbench feeds the view.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildToolInput {
    /// The project's root folder.
    pub root: String,
    /// `None` until the model is read (or when reading it failed).
    pub model: Option<BuildModel>,
    pub configs: Vec<RunConfig>,
    pub settings: BuildSettings,
    pub sync: Option<SyncBanner>,
    pub loading: bool,
    pub error: Option<String>,
}

/// A line above the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Banner {
    /// A sync is running: its last message and progress in percent.
    Syncing { message: String, percent: Option<u32> },
    /// A build's files changed since it was last reloaded; the button reloads that build.
    Stale { build_dir: Option<String>, button: &'static str },
    /// The last sync of a build failed: the first line of its error, and the whole error.
    Failed { line: String, error: String },
}

/// A toolbar button: an action's, or Run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    /// The action it runs; `None` for Run, which starts the selection's configuration.
    pub action: Option<&'static str>,
    pub icon: &'static str,
    /// English source text of its tooltip.
    pub title: String,
    pub enabled: bool,
    /// A toggle that is on.
    pub pressed: bool,
}

impl ToolbarButton {
    /// The button's debug selector.
    pub fn selector(&self) -> String {
        format!("build-tool-button:{}", self.action.unwrap_or("run"))
    }
}

/// What a context menu row does that is not an action.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildMenuCommand {
    Run(RunConfig),
    Debug(RunConfig),
    CreateConfig(RunConfig),
    /// Opens a file of the project (absolute path).
    OpenFile(String),
    /// Copies text to the clipboard.
    Copy(String),
}

/// What the view asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum BuildToolEvent {
    /// Run action `id` with `arg`.
    RunAction { id: &'static str, arg: Option<Value> },
    /// Start `config` as a temporary configuration; `debug` starts it under the debugger.
    Run { config: RunConfig, debug: bool },
    /// Show the context menu of node `key` ([`BuildToolView::context_menu`]) at `position`.
    ShowMenu { key: String, position: Point<Pixels> },
    /// A profile's checkbox was changed.
    ToggleProfile { profile: MavenProfile, checked: bool },
    /// The lead selection belongs to this project (a Maven `pom`, a Gradle project key).
    CurrentProject(Option<String>),
    /// Load Maven / Gradle Changes on the stale banner of the build at `build_dir`.
    LoadChanges { build_dir: Option<String> },
}

/// The Maven or Gradle tool window's view.
pub struct BuildToolView {
    tool: BuildTool,
    focus: FocusHandle,
    input: BuildToolInput,
    expanded: HashSet<String>,
    /// The first model has opened its top-level projects.
    opened: bool,
    /// The selected nodes' keys, in the order they were selected.
    selected: Vec<String>,
    lead: Option<String>,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<BuildToolEvent> for BuildToolView {}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// Whether action `id` is registered in this app.
fn registered(id: &str, cx: &App) -> bool {
    cx.try_global::<ActionRegistry>().is_some_and(|registry| registry.contains(id))
}

/// `file` of the project at `root`: as it is when absolute.
fn project_file(root: &str, file: &str) -> String {
    let absolute = file.starts_with('/') || file.as_bytes().get(1) == Some(&b':');
    if absolute { file.to_owned() } else { format!("{root}/{file}") }
}

impl BuildToolView {
    pub fn new(tool: BuildTool, cx: &mut Context<Self>) -> Self {
        Self {
            tool,
            focus: cx.focus_handle(),
            input: BuildToolInput::default(),
            expanded: HashSet::new(),
            opened: false,
            selected: Vec::new(),
            lead: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    pub fn tool(&self) -> BuildTool {
        self.tool
    }

    /// The model, configurations, toggles and sync state to show. The first model with projects of
    /// this tool opens its top-level projects.
    pub fn set_input(&mut self, input: BuildToolInput, cx: &mut Context<Self>) {
        if let Some(model) = &input.model
            && !self.opened
            && !roots(self.tool, model).is_empty()
        {
            self.expanded = initial_expanded(self.tool, model);
            self.opened = true;
        }
        self.input = input;
        let shown: HashSet<String> = self.rows().into_iter().map(|row| row.node.key).collect();
        self.selected.retain(|key| shown.contains(key));
        if self.lead.as_ref().is_some_and(|lead| !shown.contains(lead)) {
            self.lead = None;
        }
        cx.notify();
    }

    /// Opens the keys of a build that was just linked, keeping what is open.
    pub fn open_keys(&mut self, keys: HashSet<String>, cx: &mut Context<Self>) {
        self.expanded.extend(keys);
        cx.notify();
    }

    pub fn input(&self) -> &BuildToolInput {
        &self.input
    }

    fn source(&self) -> Option<TreeSource<'_>> {
        let model = self.input.model.as_ref()?;
        Some(TreeSource { model, configs: &self.input.configs, root: &self.input.root })
    }

    // -----------------------------------------------------------------------------------------------
    // Read by the workbench and by tests

    /// The visible lines, top to bottom.
    pub fn rows(&self) -> Vec<BuildRow> {
        match self.source() {
            Some(src) => visible_rows(self.tool, src, &self.expanded, &self.input.settings),
            None => Vec::new(),
        }
    }

    pub fn is_expanded(&self, key: &str) -> bool {
        self.expanded.contains(key)
    }

    /// The selected nodes' keys, top to bottom.
    pub fn selected(&self) -> Vec<String> {
        self.rows().into_iter().map(|row| row.node.key).filter(|key| self.selected.contains(key)).collect()
    }

    pub fn lead(&self) -> Option<&str> {
        self.lead.as_deref()
    }

    /// The configuration the selection's runnable nodes start, if any.
    pub fn selection_config(&self) -> Option<RunConfig> {
        let src = self.source()?;
        let nodes: Vec<_> = self
            .rows()
            .into_iter()
            .filter(|row| self.selected.contains(&row.node.key) && is_runnable(&row.node))
            .map(|row| row.node)
            .collect();
        node_config(&nodes, src, &self.input.settings)
    }

    /// The toolbar's buttons, without those whose action this app does not have.
    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarButton> {
        let maven = self.tool == BuildTool::Maven;
        let settings = &self.input.settings;
        let offline = if maven { settings.maven.offline } else { settings.gradle.offline };
        let action = |action: &'static str, icon: &'static str, title: &str, pressed: bool| ToolbarButton {
            action: Some(action),
            icon,
            title: title.to_owned(),
            enabled: true,
            pressed,
        };
        let reload = if self.input.sync.is_some() {
            let stop = if maven { "maven.cancelReload" } else { "gradle.cancelReload" };
            action(
                stop,
                "stop",
                if maven { "Stop Reloading Maven Projects" } else { "Stop Reloading Gradle Projects" },
                false,
            )
        } else {
            let reload = if maven { "maven.reload" } else { "gradle.reload" };
            action(
                reload,
                "refresh",
                if maven { "Reload All Maven Projects" } else { "Reload All Gradle Projects" },
                false,
            )
        };
        let mut buttons = vec![
            reload,
            if maven {
                action("maven.addProjects", "plus", "Add Maven Projects", false)
            } else {
                action("gradle.linkProjects", "plus", "Link Gradle Projects", false)
            },
            ToolbarButton {
                action: None,
                icon: "run",
                title: if maven { "Run Maven Build" } else { "Run Gradle Task" }.to_owned(),
                enabled: self.selection_config().is_some(),
                pressed: false,
            },
            if maven {
                action("maven.executeGoal", "run-anything", "Execute Maven Goal…", false)
            } else {
                action("gradle.executeTask", "run-anything", "Execute Gradle Task…", false)
            },
            action(
                if maven { "maven.toggleOffline" } else { "gradle.toggleOffline" },
                "build-offline",
                "Toggle Offline Mode",
                offline,
            ),
            action(
                if maven { "maven.analyzeDependencies" } else { "gradle.analyzeDependencies" },
                "build-dependencies",
                "Analyze Dependencies…",
                false,
            ),
        ];
        if maven {
            buttons.push(action(
                "maven.toggleSkipTests",
                "skip-tests",
                "Toggle 'Skip Tests' Mode",
                settings.maven.skip_tests,
            ));
        }
        buttons.push(action(
            if maven { "maven.expandAll" } else { "gradle.expandAll" },
            "expand-all",
            "Expand All",
            false,
        ));
        buttons.push(action(
            if maven { "maven.collapseAll" } else { "gradle.collapseAll" },
            "collapse-all",
            "Collapse All",
            false,
        ));
        if maven {
            buttons.push(action("maven.settings", "settings", "Maven Settings…", false));
        }
        buttons.retain(|button| button.action.is_none_or(|action| registered(action, cx)));
        buttons
    }

    /// The lines above the tree: the running sync, else each build's stale or failed state.
    pub fn banners(&self) -> Vec<Banner> {
        if let Some(sync) = &self.input.sync {
            let percent = sync.fraction.map(|f| (f * 100.0).round() as u32);
            return vec![Banner::Syncing { message: sync.message.clone(), percent }];
        }
        let Some(model) = &self.input.model else { return Vec::new() };
        let maven = self.tool == BuildTool::Maven;
        let states: Vec<(Option<String>, bool, Option<&String>)> = if maven {
            model.maven.iter().map(|m| (None, m.stale, m.sync_error.as_ref())).collect()
        } else {
            model.gradle.iter().map(|g| (Some(g.dir.clone()), g.stale, g.sync_error.as_ref())).collect()
        };
        states
            .into_iter()
            .filter_map(|(build_dir, stale, error)| {
                if stale {
                    let button = if maven { "Load Maven Changes" } else { "Load Gradle Changes" };
                    return Some(Banner::Stale { build_dir, button });
                }
                let error = error?;
                let line = error.lines().next().unwrap_or_default().to_owned();
                Some(Banner::Failed { line, error: error.clone() })
            })
            .collect()
    }

    /// The context menu of node `key`: Run / Debug and Create or Edit Configuration for what
    /// runs, then the node's own rows. Rows of actions this app does not have are left out.
    pub fn context_menu(&self, key: &str, cx: &App) -> Vec<MenuItem<BuildMenuCommand>> {
        let rows = self.rows();
        let Some(node) = rows.iter().find(|row| row.node.key == key).map(|row| row.node.clone()) else {
            return Vec::new();
        };
        let mut items: Vec<MenuItem<BuildMenuCommand>> = Vec::new();
        let action = |items: &mut Vec<MenuItem<BuildMenuCommand>>, item: MenuItem<BuildMenuCommand>| {
            if item.action_id().is_none_or(|id| registered(id, cx)) {
                items.push(item);
            }
        };
        let nodes: Vec<_> = if self.selected.iter().any(|selected| selected == key) {
            rows.iter().filter(|row| self.selected.contains(&row.node.key)).map(|row| row.node.clone()).collect()
        } else {
            vec![node.clone()]
        };
        let runnable: Vec<_> = nodes.into_iter().filter(is_runnable).collect();
        let config = if is_runnable(&node) {
            self.source().and_then(|src| node_config(&runnable, src, &self.input.settings))
        } else {
            None
        };
        if let Some(config) = config {
            items.push(
                MenuItem::item(format!("Run '{}'", config.name), BuildMenuCommand::Run(config.clone())).icon("run"),
            );
            items.push(
                MenuItem::item(format!("Debug '{}'", config.name), BuildMenuCommand::Debug(config.clone()))
                    .icon("debug"),
            );
            items.push(MenuItem::Separator);
            match &node.kind {
                NodeKind::RunConfig(saved) => action(
                    &mut items,
                    MenuItem::action_with("run.editConfigs", Value::from(saved.id.clone()))
                        .labelled("Edit Configuration…"),
                ),
                _ => items.push(
                    MenuItem::item("Create Run Configuration…", BuildMenuCommand::CreateConfig(config)).icon("edit"),
                ),
            }
        }
        let root = self.input.root.as_str();
        match &node.kind {
            NodeKind::MavenProject { pom, .. } => {
                action(&mut items, MenuItem::action_with("maven.unlinkProjects", json!({ "pom": pom })));
                let in_workspace = self
                    .input
                    .model
                    .as_ref()
                    .and_then(|m| m.maven.as_ref())
                    .and_then(|m| m.projects.iter().find(|p| p.pom == *pom))
                    .is_some_and(|p| p.project_root.is_some());
                let file = if in_workspace { pom.clone() } else { project_file(root, pom) };
                items.push(MenuItem::item("Open pom.xml", BuildMenuCommand::OpenFile(file)).icon("file-maven"));
                action(&mut items, MenuItem::action("maven.reload").labelled("Reload All Maven Projects"));
                action(&mut items, MenuItem::action("maven.executeGoal"));
                action(&mut items, MenuItem::action_with("maven.analyzeDependencies", json!({ "pom": pom })));
            }
            NodeKind::Dependencies { pom, .. } => {
                action(&mut items, MenuItem::action_with("maven.analyzeDependencies", json!({ "pom": pom })));
            }
            NodeKind::GradleProject { build, path, build_file, .. } => {
                if let Some(file) = build_file {
                    let name = file.rsplit('/').next().unwrap_or(file);
                    let open = BuildMenuCommand::OpenFile(project_file(root, file));
                    items.push(MenuItem::item(format!("Open {name}"), open).icon("file-gradle"));
                }
                if path == ":" {
                    action(&mut items, MenuItem::action_with("gradle.unlinkProjects", json!({ "buildDir": build })));
                }
                let reload = MenuItem::action_with("gradle.reload", json!({ "buildDir": build }));
                action(&mut items, reload.labelled("Reload Gradle Project"));
                let key = gradle_project_key(build, path);
                action(&mut items, MenuItem::action_with("gradle.executeTask", json!({ "projectKey": key })));
                let analyze = json!({ "buildDir": build, "gradlePath": path });
                action(&mut items, MenuItem::action_with("gradle.analyzeDependencies", analyze));
            }
            NodeKind::Dependency(d) => {
                let coordinates: Vec<&str> =
                    [Some(d.group_id.as_str()), Some(d.artifact_id.as_str()), d.version.as_deref()]
                        .into_iter()
                        .flatten()
                        .filter(|part| !part.is_empty())
                        .collect();
                let copy = BuildMenuCommand::Copy(coordinates.join(":"));
                items.push(MenuItem::item("Copy Coordinates", copy).icon("copy"));
            }
            _ => {}
        }
        items
    }

    // -----------------------------------------------------------------------------------------------
    // Interaction

    /// Expand All: every folder down to the dependencies (which stay closed).
    pub fn expand_all(&mut self, cx: &mut Context<Self>) {
        let Some(src) = self.source() else { return };
        let tool = self.tool;
        let keys = expandable_keys(roots(tool, src.model), &|node| children(tool, node, src));
        self.expanded = keys;
        cx.notify();
    }

    pub fn collapse_all(&mut self, cx: &mut Context<Self>) {
        self.expanded.clear();
        cx.notify();
    }

    /// Opens or closes the row `key`.
    pub fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.to_owned());
        }
        cx.notify();
    }

    fn set_lead(&mut self, key: Option<String>, cx: &mut Context<Self>) {
        if self.lead != key {
            let project = key.as_ref().and_then(|key| {
                self.rows().into_iter().find(|row| row.node.key == *key).and_then(|row| node_project(&row.node))
            });
            self.lead = key;
            cx.emit(BuildToolEvent::CurrentProject(project));
        }
    }

    /// A click on row `key`: Ctrl or ⌘ adds it to (or takes it from) the selection, Shift selects
    /// up to it; a plain click selects it alone.
    pub fn click(&mut self, key: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        use crate::find::row_selection::SelectMode;
        match selection_mode(modifiers.shift, modifiers.control || modifiers.platform) {
            SelectMode::Toggle => {
                if let Some(at) = self.selected.iter().position(|selected| selected == key) {
                    self.selected.remove(at);
                } else {
                    self.selected.push(key.to_owned());
                }
            }
            SelectMode::Range => {
                let keys: Vec<String> = self.rows().into_iter().map(|row| row.node.key).collect();
                let from = self.lead.as_ref().and_then(|lead| keys.iter().position(|k| k == lead));
                let to = keys.iter().position(|k| k == key);
                self.selected = match (from, to) {
                    (Some(a), Some(b)) => keys[a.min(b)..=a.max(b)].to_vec(),
                    _ => vec![key.to_owned()],
                };
            }
            SelectMode::Single => self.selected = vec![key.to_owned()],
        }
        self.set_lead(Some(key.to_owned()), cx);
        cx.notify();
    }

    /// Enter or a double click on row `key`: a profile flips, what runs starts.
    pub fn activate(&mut self, key: &str, cx: &mut Context<Self>) {
        let rows = self.rows();
        let Some(row) = rows.into_iter().find(|row| row.node.key == key) else { return };
        if let NodeKind::Profile(profile) = &row.node.kind {
            let checked = !row.checked.unwrap_or(false);
            cx.emit(BuildToolEvent::ToggleProfile { profile: profile.clone(), checked });
            return;
        }
        if !is_runnable(&row.node) {
            return;
        }
        let Some(src) = self.source() else { return };
        if let Some(config) = node_config(std::slice::from_ref(&row.node), src, &self.input.settings) {
            cx.emit(BuildToolEvent::Run { config, debug: false });
        }
    }

    /// A right click on row `key`: it is selected unless it already is, then its menu is asked for.
    pub fn context_click(&mut self, key: &str, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.selected.iter().any(|selected| selected == key) {
            self.selected = vec![key.to_owned()];
            self.set_lead(Some(key.to_owned()), cx);
        }
        cx.emit(BuildToolEvent::ShowMenu { key: key.to_owned(), position });
        cx.notify();
    }

    /// A toolbar button was pressed.
    pub fn press(&mut self, button: &ToolbarButton, cx: &mut Context<Self>) {
        if !button.enabled {
            return;
        }
        match button.action {
            Some(id) => cx.emit(BuildToolEvent::RunAction { id, arg: None }),
            None => {
                if let Some(config) = self.selection_config() {
                    cx.emit(BuildToolEvent::Run { config, debug: false });
                }
            }
        }
    }

    fn reveal_lead(&self) {
        let rows = self.rows();
        if let Some(index) = self.lead.as_ref().and_then(|lead| rows.iter().position(|row| row.node.key == *lead)) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return;
        }
        let rows = self.rows();
        let at = self.lead.as_ref().and_then(|lead| rows.iter().position(|row| row.node.key == *lead));
        let select = |index: usize| rows.get(index).map(|row| row.node.key.clone());
        let target = match event.keystroke.key.as_str() {
            "down" => select(at.map_or(0, |at| (at + 1).min(rows.len().saturating_sub(1)))),
            "up" => select(at.map_or(0, |at| at.saturating_sub(1))),
            "right" | "left" => {
                let Some(row) = at.and_then(|at| rows.get(at)) else { return };
                let open = event.keystroke.key == "right";
                if row.expandable && row.expanded != open {
                    let key = row.node.key.clone();
                    self.toggle(&key, cx);
                } else if !open {
                    // Left on a closed row goes to its parent.
                    let parent = rows[..at.unwrap_or(0)].iter().rev().find(|r| r.depth < row.depth);
                    if let Some(parent) = parent.map(|parent| parent.node.key.clone()) {
                        self.click(&parent, Modifiers::default(), cx);
                    }
                }
                cx.stop_propagation();
                return;
            }
            "enter" => {
                if let Some(lead) = self.lead.clone() {
                    self.activate(&lead, cx);
                }
                cx.stop_propagation();
                return;
            }
            _ => return,
        };
        if let Some(key) = target {
            self.click(&key, Modifiers::default(), cx);
            self.reveal_lead();
        }
        cx.stop_propagation();
    }

    // -----------------------------------------------------------------------------------------------
    // Rendering

    fn tool_button(&self, button: ToolbarButton, cx: &mut Context<Self>) -> Stateful<Div> {
        let color = theme::color(cx, if button.enabled { "fg-muted" } else { "fg-faint" });
        let selector = button.selector();
        let id = selector.clone();
        let title = tx(cx, &button.title);
        let (shortcut, icon, pressed, enabled) = (button.action, button.icon, button.pressed, button.enabled);
        let el = div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-tree-selected-inactive")))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.press(&button, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color));
        match shortcut {
            Some(action) => el.with_tooltip_shortcut(title, action),
            None => el.with_tooltip(title),
        }
    }

    fn separator(cx: &App) -> Div {
        div().w(px(1.0)).h(px(16.0)).mx(px(4.0)).flex_none().bg(theme::color(cx, "border"))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let buttons = self.toolbar(cx);
        let mut bar = div().h(px(32.0)).flex_none().flex().items_center().gap(px(2.0)).px(px(6.0));
        for button in buttons {
            // Separators: before Run and before Expand All, as the toolbar groups them.
            let before = button.action.is_none() || button.action.is_some_and(|a| a.ends_with(".expandAll"));
            if before {
                bar = bar.child(Self::separator(cx));
            }
            bar = bar.child(self.tool_button(button, cx));
        }
        bar
    }

    fn render_banners(&self, cx: &mut Context<Self>) -> Vec<Div> {
        let banner = |cx: &App| {
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(6.0))
                .min_h(px(28.0))
                .px(px(10.0))
                .text_size(theme::length(cx, "fs-small"))
                .text_color(theme::color(cx, "fg-secondary"))
                .border_b_1()
                .border_color(theme::color(cx, "border"))
        };
        self.banners()
            .into_iter()
            .enumerate()
            .map(|(index, shown)| match shown {
                Banner::Syncing { message, percent } => banner(cx)
                    .debug_selector(|| "build-tool-syncing".to_owned())
                    .child(theme::icon(cx, "refresh", 12.0, theme::color(cx, "fg-muted")))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(message)))
                    .children(percent.map(|percent| {
                        div().text_color(theme::color(cx, "fg-subtle")).child(SharedString::from(format!("{percent}%")))
                    })),
                Banner::Stale { build_dir, button } => {
                    let icon = if self.tool == BuildTool::Maven { "maven-load-changes" } else { "gradle-load-changes" };
                    let selector = format!("build-tool-load-changes-{index}");
                    banner(cx)
                        .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-muted")))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .child(SharedString::from(tx(cx, "Build files changed since the last reload"))),
                        )
                        .child(
                            div()
                                .id(SharedString::from(selector.clone()))
                                .debug_selector(move || selector.clone())
                                .px(px(6.0))
                                .rounded(theme::length(cx, "radius-small"))
                                .text_color(theme::color(cx, "link"))
                                .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                                .on_click(cx.listener(move |_, _: &ClickEvent, _, cx| {
                                    cx.emit(BuildToolEvent::LoadChanges { build_dir: build_dir.clone() });
                                }))
                                .child(SharedString::from(tx(cx, button))),
                        )
                }
                Banner::Failed { line, .. } => banner(cx)
                    .text_color(theme::color(cx, "warning"))
                    .child(theme::icon(cx, "warning", 16.0, theme::color(cx, "warning")))
                    .child(div().flex_1().min_w_0().overflow_hidden().child(SharedString::from(line))),
            })
            .collect()
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, strong, subtle, muted, chevron) = (
            theme::color(cx, "fg"),
            theme::color(cx, "fg-strong"),
            theme::color(cx, "fg-subtle"),
            theme::color(cx, "fg-muted"),
            theme::color(cx, "fg-icon"),
        );
        let selected = self.selected.clone();
        let list = uniform_list(
            "build-tool-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let key = row.node.key.clone();
                        let is_selected = selected.contains(&key);
                        let toggle = if row.expandable {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let toggled = key.clone();
                            div()
                                .id(SharedString::from(format!("build-tool-toggle:{key}")))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let label_color = if row.skipped {
                            muted
                        } else if is_selected {
                            strong
                        } else {
                            fg
                        };
                        let lead = match row.checked {
                            Some(checked) => {
                                let flipped = key.clone();
                                let check = format!("build-tool-check:{key}");
                                div()
                                    .id(SharedString::from(check.clone()))
                                    .debug_selector(move || check.clone())
                                    .size(px(14.0))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(2.0))
                                    .border_1()
                                    .border_color(theme::color(cx, "border"))
                                    .when(checked, |el| el.child(theme::icon(cx, "check", 12.0, strong)))
                                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                        view.activate(&flipped, cx);
                                        cx.stop_propagation();
                                    }))
                                    .into_any_element()
                            }
                            None => theme::icon(cx, row.icon, 16.0, label_color),
                        };
                        let detail = row.detail.clone();
                        let selector = format!("build-tool-row:{key}");
                        let (clicked, menu) = (key.clone(), key.clone());
                        div()
                            .id(SharedString::from(selector.clone()))
                            .debug_selector(move || selector.clone())
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .pl(px(4.0 + 16.0 * row.depth as f32))
                            .pr(px(8.0))
                            .whitespace_nowrap()
                            .when(is_selected, |el| el.bg(selected_bg))
                            .child(toggle)
                            .child(lead)
                            .child(
                                div()
                                    .flex_none()
                                    .overflow_hidden()
                                    .text_color(label_color)
                                    .when(row.skipped, |el| el.line_through())
                                    .child(SharedString::from(row.label.clone())),
                            )
                            .when_some(detail, |el, detail| {
                                el.child(
                                    div()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_color(subtle)
                                        .child(SharedString::from(detail)),
                                )
                            })
                            .on_click(cx.listener(move |view, event: &ClickEvent, window, cx| {
                                window.focus(&view.focus, cx);
                                view.click(&clicked, event.modifiers(), cx);
                                if event.click_count() >= 2 && !event.modifiers().shift {
                                    view.activate(&clicked, cx);
                                }
                            }))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                                    window.focus(&view.focus, cx);
                                    view.context_click(&menu, event.position, cx);
                                    cx.stop_propagation();
                                }),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div().id("build-tool-tree-area").debug_selector(|| "build-tool-tree".to_owned()).flex_1().min_h_0().child(list)
    }

    fn render_message(text: String, cx: &App) -> Div {
        div()
            .debug_selector(|| "build-tool-message".to_owned())
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(text))
    }
}

impl Focusable for BuildToolView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for BuildToolView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let base = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        if self.input.model.is_none() {
            let text = match (&self.input.error, self.input.loading) {
                (Some(error), _) => error.clone(),
                (None, true) => tx(cx, "Loading…"),
                (None, false) => String::new(),
            };
            return base.child(Self::render_message(text, cx));
        }
        let toolbar = self.render_toolbar(cx);
        let banners = self.render_banners(cx);
        let empty = self.input.model.as_ref().is_some_and(|model| roots(self.tool, model).is_empty());
        let body = if empty {
            let text = if self.tool == BuildTool::Maven { "No Maven projects" } else { "No Gradle projects" };
            Self::render_message(tx(cx, text), cx).into_any_element()
        } else {
            self.render_tree(window, cx).into_any_element()
        };
        base.child(toolbar).children(banners).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use ide_build::{GradleModel, GradleProject, GradleTask, MavenModel, MavenPlugin, MavenProject};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<BuildToolEvent>>>;

    fn maven_model() -> BuildModel {
        let project = MavenProject {
            pom: "pom.xml".to_owned(),
            group_id: "com.example".to_owned(),
            artifact_id: "spring-demo".to_owned(),
            packaging: "jar".to_owned(),
            display_name: "spring-demo".to_owned(),
            profiles: vec!["ci".to_owned()],
            command: "/p/mvnw".to_owned(),
            plugins: vec![MavenPlugin {
                group_id: "org.apache.maven.plugins".to_owned(),
                artifact_id: "maven-surefire-plugin".to_owned(),
                version: None,
                prefix: "surefire".to_owned(),
                goals: vec!["test".to_owned()],
            }],
            ..MavenProject::default()
        };
        let profiles = vec![MavenProfile { project_root: None, id: "ci".to_owned(), active_by_default: false }];
        let maven =
            MavenModel { projects: vec![project], profiles, synced_at: Some(1.0), stale: true, sync_error: None };
        BuildModel { maven: Some(maven), gradle: Vec::new() }
    }

    fn gradle_model() -> BuildModel {
        let project = GradleProject {
            path: ":".to_owned(),
            name: "demo".to_owned(),
            build_file: Some("build.gradle.kts".to_owned()),
            tasks: vec![GradleTask { name: "test".to_owned(), group: "verification".to_owned(), description: None }],
            ..GradleProject::default()
        };
        let build = GradleModel { projects: vec![project], command: "gradle".to_owned(), ..GradleModel::default() };
        BuildModel { maven: None, gradle: vec![build] }
    }

    fn open(
        cx: &mut TestAppContext,
        tool: BuildTool,
        model: BuildModel,
        settings: BuildSettings,
    ) -> (Entity<BuildToolView>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            let registry = cx.default_global::<ActionRegistry>();
            for id in ["maven.reload", "maven.toggleSkipTests", "maven.expandAll", "maven.collapseAll", "gradle.reload"]
            {
                registry.handle(id, |_, _, _| Ok(()));
            }
        });
        let (view, cx) = cx.add_window_view(|_, cx| BuildToolView::new(tool, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &BuildToolEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        let input = BuildToolInput { root: "/p".to_owned(), model: Some(model), settings, ..BuildToolInput::default() };
        view.update(cx, |view, cx| view.set_input(input, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: String) {
        cx.run_until_parked();
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), Modifiers::default());
        cx.run_until_parked();
    }

    fn labels(view: &Entity<BuildToolView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.rows().into_iter().map(|row| row.label).collect())
    }

    fn skip_tests() -> BuildSettings {
        let mut settings = BuildSettings::default();
        settings.maven.skip_tests = true;
        settings
    }

    #[gpui_kit::test]
    async fn runs_a_lifecycle_phase_through_the_run_feature_with_the_toolbar_options(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, BuildTool::Maven, maven_model(), skip_tests());
        let banners = view.read_with(cx, |view, _| view.banners());
        assert_eq!(banners, [Banner::Stale { build_dir: None, button: "Load Maven Changes" }]);
        assert_eq!(labels(&view, cx), ["Profiles", "spring-demo", "Lifecycle", "Plugins"]);
        click(cx, "build-tool-row:mvn:pom.xml:lifecycle".to_owned());
        cx.simulate_keystrokes("right");
        let test = view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.label == "test"));
        let test = test.expect("the test phase is shown");
        assert!(test.skipped, "Skip Tests strikes the test phase");
        click(cx, format!("build-tool-row:{}", test.node.key));
        cx.simulate_keystrokes("enter");
        let started: Vec<RunConfig> = events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                BuildToolEvent::Run { config, debug: false } => Some(config.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(started.len(), 1);
        assert_eq!(started[0].name, "spring-demo [test]");
        assert_eq!(started[0].command, "/p/mvnw");
        assert_eq!(started[0].args, ["test", "-DskipTests"]);
        assert_eq!(started[0].kind, "maven");
        let current = events.borrow().iter().rev().find_map(|event| match event {
            BuildToolEvent::CurrentProject(project) => Some(project.clone()),
            _ => None,
        });
        assert_eq!(current, Some(Some("pom.xml".to_owned())));
    }

    #[gpui_kit::test]
    async fn does_not_mark_gradle_test_tasks_as_skipped_when_maven_skip_tests_is_on(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, BuildTool::Gradle, gradle_model(), skip_tests());
        view.update(cx, |view, cx| view.expand_all(cx));
        let test = view.read_with(cx, |view, _| view.rows().into_iter().find(|row| row.label == "test"));
        assert!(!test.expect("the test task is shown").skipped);
    }

    // Not from a spec: the toolbar shows the actions the app has, Skip Tests pressed while it is on,
    // and Expand All / Collapse All act on the tree.
    #[gpui_kit::test]
    async fn the_toolbar_offers_registered_actions_only_and_expands_and_collapses_the_tree(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, BuildTool::Maven, maven_model(), skip_tests());
        let buttons = view.read_with(cx, |view, cx| view.toolbar(cx));
        let actions: Vec<Option<&str>> = buttons.iter().map(|b| b.action).collect();
        assert_eq!(
            actions,
            [
                Some("maven.reload"),
                None,
                Some("maven.toggleSkipTests"),
                Some("maven.expandAll"),
                Some("maven.collapseAll")
            ]
        );
        assert!(buttons[2].pressed);
        assert!(!buttons[1].enabled, "Run waits for a runnable selection");
        click(cx, "build-tool-button:maven.reload".to_owned());
        assert_eq!(events.borrow().last(), Some(&BuildToolEvent::RunAction { id: "maven.reload", arg: None }));
        view.update(cx, |view, cx| view.expand_all(cx));
        assert!(view.read_with(cx, |view, _| view.is_expanded("mvn:pom.xml:plugins")));
        view.update(cx, |view, cx| view.collapse_all(cx));
        assert_eq!(labels(&view, cx), ["Profiles", "spring-demo"]);
    }

    // Not from a spec: a project's context menu opens its pom.xml and reloads; a profile's checkbox
    // flips its choice.
    #[gpui_kit::test]
    async fn the_project_menu_opens_its_pom_and_a_profile_checkbox_flips_its_choice(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, BuildTool::Maven, maven_model(), BuildSettings::default());
        let menu = view.read_with(cx, |view, cx| view.context_menu("mvn:pom.xml", cx));
        let labels: Vec<Option<&str>> = menu.iter().map(MenuItem::label).collect();
        assert_eq!(labels, [Some("Open pom.xml"), Some("Reload All Maven Projects")]);
        assert_eq!(menu[0].command(), Some(&BuildMenuCommand::OpenFile("/p/pom.xml".to_owned())));
        view.update(cx, |view, cx| view.toggle("mvn:profiles", cx));
        click(cx, "build-tool-check:mvn:profile:ci".to_owned());
        let toggled = events.borrow().iter().find_map(|event| match event {
            BuildToolEvent::ToggleProfile { profile, checked } => Some((profile.id.clone(), *checked)),
            _ => None,
        });
        assert_eq!(toggled, Some(("ci".to_owned(), true)));
    }
}
