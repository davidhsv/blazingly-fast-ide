//! The Remote Host tool window: a toolbar (the server chooser, Refresh, Collapse All, Deployment
//! Configuration…), the server root, and the tree of the server's files, listed folder by folder
//! as they are expanded.
//!
//! The view draws a [`RemoteTree`] the workbench hands it ([`RemoteHostView::set_state`]) and owns
//! only the selection. Expanding a folder, opening a file, the context menu's commands and the
//! toolbar are [`RemoteHostEvent`]s: the workbench lists, opens, creates, renames, deletes and
//! downloads through the backend and hands the tree back.

use crate::features_b::deployment::remote_host::RemoteTree;
use crate::platform::i18n::Localization;
use crate::theme;
use crate::ui::tooltip::WithTooltip;
use crate::vcs_ops::menu::{MenuEntry, MenuItem};
use gpui_kit::{
    App, ClickEvent, Context, Div, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, Render, ScrollStrategy, SharedString, Stateful,
    UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};
use ide_ssh::RemoteEntry;
use std::collections::HashMap;

/// The action Deployment Configuration… runs.
pub const CONFIGURE_ACTION: &str = "deployment.configure";

/// A server the chooser offers: its id and the name it is shown under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostServer {
    pub id: String,
    pub label: String,
}

/// What the tool window shows: the configured servers, the one shown and its tree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RemoteHostState {
    pub servers: Vec<HostServer>,
    /// The id of the server shown; `None` without servers.
    pub server: Option<String>,
    /// The tree of that server, once its root was asked for.
    pub tree: Option<RemoteTree>,
}

/// A line of the tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostRow {
    /// The entry's remote path; for the "Empty folder" line, the folder's path with a suffix.
    pub key: String,
    pub label: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
    /// Why the folder could not be listed.
    pub detail: Option<String>,
    /// The "Empty folder" line of an expanded folder without entries.
    pub placeholder: bool,
}

/// What a row of the context menu does.
#[derive(Clone, Debug, PartialEq)]
pub enum HostCommand {
    Open(RemoteEntry),
    /// Download to Project: the remote entries to download to their mapped project paths.
    Download(Vec<RemoteEntry>),
    /// New Folder… in this remote folder.
    NewFolder(String),
    Rename(RemoteEntry),
    Delete(Vec<RemoteEntry>),
    /// Refresh on a folder: lists it again.
    RefreshFolder(String),
    /// Refresh elsewhere: the root and every expanded folder again.
    RefreshAll,
    CopyPath(String),
}

/// What the view asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum RemoteHostEvent {
    /// Open the chooser of [`RemoteHostState::servers`] at `position`.
    ShowServerMenu { position: Point<Pixels> },
    /// Open [`RemoteHostView::menu_items`] at `position`.
    ShowMenu { position: Point<Pixels> },
    /// The toolbar's Refresh: the root and the expanded folders again.
    Refresh,
    /// The toolbar's Collapse All.
    CollapseAll,
    /// Resolve the root again after it failed (Retry).
    Retry,
    /// Run a registered action (Deployment Configuration…).
    RunAction(&'static str),
    /// Show or hide a folder's entries; a folder not listed yet is listed.
    Expand { path: String, expanded: bool },
    /// Open a file of the server in the editor.
    Open(RemoteEntry),
}

/// A toolbar button and whether it can be pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarButton {
    pub id: &'static str,
    pub enabled: bool,
}

pub struct RemoteHostView {
    focus: FocusHandle,
    state: RemoteHostState,
    /// The selected entries' paths, in the order they were selected.
    selected: Vec<String>,
    /// The entry the keyboard moves from, and where a Shift range starts.
    lead: Option<String>,
    /// The row the context menu was opened on (`None`: below the rows).
    menu_node: Option<String>,
    scroll: UniformListScrollHandle,
}

impl EventEmitter<RemoteHostEvent> for RemoteHostView {}

/// The suffix of the key of a folder's "Empty folder" line.
const EMPTY_SUFFIX: &str = "\u{0}empty";

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

impl RemoteHostView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus: cx.focus_handle(),
            state: RemoteHostState::default(),
            selected: Vec::new(),
            lead: None,
            menu_node: None,
            scroll: UniformListScrollHandle::new(),
        }
    }

    /// Shows `state`. Selected entries no longer listed leave the selection.
    pub fn set_state(&mut self, state: RemoteHostState, cx: &mut Context<Self>) {
        let server_changed = state.server != self.state.server;
        self.state = state;
        if server_changed {
            self.selected.clear();
            self.lead = None;
        } else {
            let listed = self.entries_by_path();
            self.selected.retain(|path| listed.contains_key(path));
            if self.lead.as_ref().is_some_and(|lead| !listed.contains_key(lead)) {
                self.lead = None;
            }
        }
        cx.notify();
    }

    pub fn state(&self) -> &RemoteHostState {
        &self.state
    }

    /// The selected entries' paths.
    pub fn selected(&self) -> &[String] {
        &self.selected
    }

    fn tree(&self) -> Option<&RemoteTree> {
        self.state.tree.as_ref()
    }

    /// Every listed entry by its path.
    fn entries_by_path(&self) -> HashMap<String, RemoteEntry> {
        let mut map = HashMap::new();
        for entries in self.tree().into_iter().flat_map(|tree| tree.listings.values()) {
            for entry in entries {
                map.insert(entry.path.clone(), entry.clone());
            }
        }
        map
    }

    /// The visible lines, top to bottom: the root's entries, and the entries of every expanded
    /// folder that is listed under it.
    pub fn rows(&self) -> Vec<HostRow> {
        fn add(tree: &RemoteTree, folder: &str, depth: usize, out: &mut Vec<HostRow>, empty_label: &str) {
            let Some(entries) = tree.listings.get(folder) else { return };
            if entries.is_empty() && depth > 0 {
                out.push(HostRow {
                    key: format!("{folder}{EMPTY_SUFFIX}"),
                    label: empty_label.to_owned(),
                    depth,
                    is_dir: false,
                    expanded: false,
                    detail: None,
                    placeholder: true,
                });
            }
            for entry in entries {
                let expanded = entry.is_dir && tree.expanded.contains(&entry.path);
                out.push(HostRow {
                    key: entry.path.clone(),
                    label: entry.name.clone(),
                    depth,
                    is_dir: entry.is_dir,
                    expanded,
                    detail: tree.folder_errors.get(&entry.path).cloned(),
                    placeholder: false,
                });
                if expanded {
                    add(tree, &entry.path, depth + 1, out, empty_label);
                }
            }
        }
        let mut out = Vec::new();
        if let Some(tree) = self.tree()
            && let Some(root) = tree.root.as_deref()
        {
            add(tree, root, 0, &mut out, "Empty folder");
        }
        out
    }

    /// The root's entries are listed (and the root has no error): the tree shows.
    fn roots_listed(&self) -> bool {
        self.tree().is_some_and(|tree| tree.root.as_ref().is_some_and(|root| tree.listings.contains_key(root)))
    }

    /// The toolbar's buttons after the server chooser.
    pub fn toolbar(&self) -> Vec<ToolbarButton> {
        let has_server = self.state.server.is_some();
        vec![
            ToolbarButton { id: "remote-host-refresh", enabled: has_server },
            ToolbarButton { id: "remote-host-collapse-all", enabled: has_server },
            ToolbarButton { id: "remote-host-configure", enabled: true },
        ]
    }

    /// The label of the server shown.
    pub fn server_label(&self) -> Option<&str> {
        let id = self.state.server.as_deref()?;
        self.state.servers.iter().find(|server| server.id == id).map(|server| server.label.as_str())
    }

    // ---------------------------------------------------------------------------------------------
    // Selection and keyboard

    /// A click on the row `path`: Shift extends from the lead, Ctrl or ⌘ toggles, otherwise the row
    /// alone is selected.
    pub fn click(&mut self, path: &str, modifiers: Modifiers, cx: &mut Context<Self>) {
        if path.ends_with(EMPTY_SUFFIX) {
            return;
        }
        if modifiers.shift
            && let Some(lead) = self.lead.clone()
        {
            let keys: Vec<String> = self.rows().into_iter().filter(|row| !row.placeholder).map(|row| row.key).collect();
            if let (Some(from), Some(to)) =
                (keys.iter().position(|key| *key == lead), keys.iter().position(|key| key == path))
            {
                let (start, end) = if from <= to { (from, to) } else { (to, from) };
                self.selected = keys[start..=end].to_vec();
                cx.notify();
                return;
            }
        }
        if modifiers.control || modifiers.platform {
            match self.selected.iter().position(|selected| selected == path) {
                Some(index) => {
                    self.selected.remove(index);
                }
                None => self.selected.push(path.to_owned()),
            }
        } else {
            self.selected = vec![path.to_owned()];
        }
        self.lead = Some(path.to_owned());
        cx.notify();
    }

    /// Enter or a double click: a file opens, a folder shows or hides its entries.
    pub fn activate(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(entry) = self.entries_by_path().remove(path) else { return };
        if entry.is_dir {
            self.toggle(path, cx);
        } else {
            cx.emit(RemoteHostEvent::Open(entry));
        }
    }

    /// Shows or hides a folder's entries.
    pub fn toggle(&mut self, path: &str, cx: &mut Context<Self>) {
        let expanded = self.tree().is_some_and(|tree| tree.expanded.contains(path));
        cx.emit(RemoteHostEvent::Expand { path: path.to_owned(), expanded: !expanded });
    }

    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let keys: Vec<String> = self.rows().into_iter().filter(|row| !row.placeholder).map(|row| row.key).collect();
        if keys.is_empty() {
            return;
        }
        let at = self.lead.as_ref().and_then(|lead| keys.iter().position(|key| key == lead));
        let next = match at {
            Some(at) => at.saturating_add_signed(delta).min(keys.len() - 1),
            None => 0,
        };
        self.selected = vec![keys[next].clone()];
        self.lead = Some(keys[next].clone());
        let rows = self.rows();
        if let Some(index) = rows.iter().position(|row| row.key == keys[next]) {
            self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Right shows a collapsed folder's entries; left hides an expanded folder's, else moves to
    /// the parent folder.
    fn horizontal(&mut self, right: bool, cx: &mut Context<Self>) {
        let Some(lead) = self.lead.clone() else { return };
        let Some(row) = self.rows().into_iter().find(|row| row.key == lead) else { return };
        if row.is_dir && row.expanded != right {
            cx.emit(RemoteHostEvent::Expand { path: lead, expanded: right });
            return;
        }
        if !right {
            let parent = crate::features_b::deployment::model::remote_parent(&lead).to_owned();
            if self.entries_by_path().contains_key(&parent) {
                self.selected = vec![parent.clone()];
                self.lead = Some(parent);
                cx.notify();
            }
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return;
        }
        match event.keystroke.key.as_str() {
            "down" => self.step(1, cx),
            "up" => self.step(-1, cx),
            "right" => self.horizontal(true, cx),
            "left" => self.horizontal(false, cx),
            "enter" => {
                if let Some(lead) = self.lead.clone() {
                    self.activate(&lead, cx);
                }
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// A right click on the row `path` (`None`: below the rows): a row outside the selection is
    /// selected alone, and the menu opens.
    pub fn context_click(&mut self, path: Option<&str>, position: Point<Pixels>, cx: &mut Context<Self>) {
        let path = path.filter(|path| !path.ends_with(EMPTY_SUFFIX));
        if let Some(path) = path
            && !self.selected.iter().any(|selected| selected == path)
        {
            self.selected = vec![path.to_owned()];
            self.lead = Some(path.to_owned());
        }
        self.menu_node = path.map(str::to_owned);
        cx.notify();
        cx.emit(RemoteHostEvent::ShowMenu { position });
    }

    /// The context menu of the row it was opened on, or of the selection.
    pub fn menu_items(&self) -> Vec<MenuItem<HostCommand>> {
        let by_path = self.entries_by_path();
        let node = self.menu_node.as_ref().and_then(|path| by_path.get(path));
        let entries: Vec<RemoteEntry> = match node {
            Some(node) if !self.selected.contains(&node.path) => vec![node.clone()],
            _ => self.selected.iter().filter_map(|path| by_path.get(path).cloned()).collect(),
        };
        let single = match entries.as_slice() {
            [only] => Some(only.clone()),
            _ => None,
        };
        let folder = match &single {
            Some(entry) if entry.is_dir => Some(entry.path.clone()),
            Some(_) => None,
            None => self.tree().and_then(|tree| tree.root.clone()),
        };
        let row = |label: &str, icon: Option<&'static str>, run: Option<HostCommand>, danger: bool| {
            MenuItem::Item(MenuEntry {
                label: label.to_owned(),
                icon,
                shortcut: None,
                disabled: run.is_none(),
                danger,
                run,
                submenu: Vec::new(),
            })
        };
        let file = single.clone().filter(|entry| !entry.is_dir);
        let refresh = match &single {
            Some(entry) if entry.is_dir => HostCommand::RefreshFolder(entry.path.clone()),
            _ => HostCommand::RefreshAll,
        };
        vec![
            row("Open", Some("edit"), file.map(HostCommand::Open), false),
            row(
                "Download to Project",
                None,
                (!entries.is_empty()).then(|| HostCommand::Download(entries.clone())),
                false,
            ),
            MenuItem::Separator,
            row("New Folder…", Some("folder"), folder.map(HostCommand::NewFolder), false),
            row("Rename…", None, single.clone().map(HostCommand::Rename), false),
            row("Delete…", Some("trash"), (!entries.is_empty()).then(|| HostCommand::Delete(entries.clone())), true),
            MenuItem::Separator,
            row("Refresh", Some("refresh"), Some(refresh), false),
            row("Copy Path", Some("copy"), single.map(|entry| HostCommand::CopyPath(entry.path)), false),
        ]
    }

    // ---------------------------------------------------------------------------------------------
    // Rendering

    fn tool_button(
        &self,
        id: &'static str,
        icon: &str,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &ClickEvent, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let color = theme::color(cx, if enabled { "fg-muted" } else { "fg-faint" });
        div()
            .id(id)
            .debug_selector(move || id.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(enabled, |el| {
                el.hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| on_click(view, event, cx)))
            })
            .child(theme::icon(cx, icon, 16.0, color))
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let enabled = |id: &str| self.toolbar().iter().any(|button| button.id == id && button.enabled);
        let label = self.server_label().unwrap_or_default().to_owned();
        let chooser = div()
            .id("remote-host-server")
            .debug_selector(|| "remote-host-server".to_owned())
            .flex_1()
            .min_w_0()
            .max_w(px(220.0))
            .h(px(24.0))
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-input"))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(SharedString::from(label)))
            .child(theme::icon(cx, "chevron-down", 14.0, theme::color(cx, "fg-icon")))
            .on_click(cx.listener(|_, event: &ClickEvent, _, cx| {
                cx.emit(RemoteHostEvent::ShowServerMenu { position: event.position() });
            }))
            .with_tooltip(tx(cx, "Deployment server"));
        div()
            .h(px(32.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(6.0))
            .child(chooser)
            .child(
                self.tool_button("remote-host-refresh", "refresh", enabled("remote-host-refresh"), cx, |_, _, cx| {
                    cx.emit(RemoteHostEvent::Refresh)
                })
                .with_tooltip("Refresh"),
            )
            .child(
                self.tool_button(
                    "remote-host-collapse-all",
                    "collapse-all",
                    enabled("remote-host-collapse-all"),
                    cx,
                    |_, _, cx| cx.emit(RemoteHostEvent::CollapseAll),
                )
                .with_tooltip("Collapse All"),
            )
            .child(
                self.tool_button("remote-host-configure", "settings", true, cx, |_, _, cx| {
                    cx.emit(RemoteHostEvent::RunAction(CONFIGURE_ACTION))
                })
                .with_tooltip("Deployment Configuration…"),
            )
    }

    fn render_empty(&self, cx: &mut Context<Self>) -> Div {
        let link = theme::color(cx, "link");
        div()
            .debug_selector(|| "remote-host-empty".to_owned())
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .text_color(theme::color(cx, "fg-muted"))
            .child(SharedString::from(tx(cx, "No deployment servers")))
            .child(
                div()
                    .id("remote-host-empty-configure")
                    .debug_selector(|| "remote-host-empty-configure".to_owned())
                    .text_color(link)
                    .hover(|el| el.text_color(theme::color(cx, "link-hover")))
                    .on_click(
                        cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(RemoteHostEvent::RunAction(CONFIGURE_ACTION))),
                    )
                    .child(SharedString::from(tx(cx, "Configure…"))),
            )
    }

    fn render_error(&self, error: &str, cx: &mut Context<Self>) -> Div {
        div()
            .debug_selector(|| "remote-host-error".to_owned())
            .flex()
            .flex_col()
            .items_start()
            .gap(px(8.0))
            .p(px(10.0))
            .child(div().text_color(theme::color(cx, "danger")).child(SharedString::from(error.to_owned())))
            .child(
                crate::vcs::prompt_dialogs::button(cx, tx(cx, "Retry"), false, false, true)
                    .id("remote-host-retry")
                    .debug_selector(|| "remote-host-retry".to_owned())
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(RemoteHostEvent::Retry))),
            )
    }

    fn render_tree(&self, window: &Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let rows = self.rows();
        let focused = self.focus.is_focused(window);
        let row_height = theme::length(cx, "h-row");
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let (fg, subtle, chevron) =
            (theme::color(cx, "fg"), theme::color(cx, "fg-subtle"), theme::color(cx, "fg-icon"));
        let hover_bg = theme::color(cx, "bg-tree-hover");
        let selected = self.selected.clone();
        let empty_label = tx(cx, "Empty folder");
        let empty_root = rows.is_empty();
        let list = uniform_list(
            "remote-host-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let is_selected = selected.contains(&row.key);
                        let toggle = if row.is_dir {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let key = row.key.clone();
                            let selector = format!("remote-host-toggle:{}", row.key);
                            div()
                                .id(SharedString::from(selector.clone()))
                                .debug_selector(move || selector.clone())
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron))
                                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                                    view.toggle(&key, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let icon = if row.placeholder {
                            None
                        } else if row.is_dir {
                            Some("folder")
                        } else {
                            Some(theme::icons::file_icon_name(&row.label))
                        };
                        let label = if row.placeholder { empty_label.clone() } else { row.label.clone() };
                        let selector = format!("remote-host-row:{}", row.key);
                        let (clicked, menu) = (row.key.clone(), row.key.clone());
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
                            .when(!is_selected, |el| el.hover(|el| el.bg(hover_bg)))
                            .child(toggle)
                            .children(icon.map(|icon| theme::icon(cx, icon, 16.0, fg)))
                            .child(
                                div()
                                    .flex_none()
                                    .overflow_hidden()
                                    .text_color(if row.placeholder { subtle } else { fg })
                                    .child(SharedString::from(label)),
                            )
                            .when_some(row.detail.clone(), |el, detail| {
                                let text = SharedString::from(detail);
                                el.child(div().min_w_0().overflow_hidden().text_color(subtle).child(text))
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
                                    view.context_click(Some(menu.as_str()), event.position, cx);
                                    cx.stop_propagation();
                                }),
                            )
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(&self.scroll)
        .size_full();
        div()
            .id("remote-host-tree-area")
            .debug_selector(|| "remote-host-tree".to_owned())
            .flex_1()
            .min_h_0()
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|view, event: &MouseDownEvent, _, cx| view.context_click(None, event.position, cx)),
            )
            .when(empty_root, |el| {
                el.child(div().p(px(10.0)).text_color(subtle).child(SharedString::from(tx(cx, "Empty folder"))))
            })
            .when(!empty_root, |el| el.child(list))
    }
}

impl Focusable for RemoteHostView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RemoteHostView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .min_h_0()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"));
        if self.state.servers.is_empty() || self.state.server.is_none() {
            return root.child(self.render_empty(cx));
        }
        let toolbar = self.render_toolbar(cx);
        let root_path = self.tree().and_then(|tree| tree.root.clone()).map(|path| {
            div()
                .debug_selector(|| "remote-host-root".to_owned())
                .flex_none()
                .px(px(10.0))
                .pb(px(4.0))
                .text_color(theme::color(cx, "fg-subtle"))
                .text_size(theme::length(cx, "fs-small"))
                .font_family(crate::theme::installed_fonts::mono_family(cx))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .child(SharedString::from(path))
        });
        let error = self.tree().and_then(|tree| tree.error.clone());
        let body = match error {
            Some(error) => self.render_error(&error, cx).into_any_element(),
            None if !self.roots_listed() => div()
                .debug_selector(|| "remote-host-connecting".to_owned())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(tx(cx, "Connecting…")))
                .into_any_element(),
            None => self.render_tree(window, cx).into_any_element(),
        };
        root.child(toolbar).children(root_path).child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<RemoteHostEvent>>>;

    fn entry(path: &str, is_dir: bool) -> RemoteEntry {
        RemoteEntry {
            name: crate::features_b::deployment::model::remote_name(path).to_owned(),
            path: path.to_owned(),
            is_dir,
            is_symlink: false,
            size: 4,
            mtime: None,
            permissions: "rw-r--r--".to_owned(),
        }
    }

    /// `/srv` listing the folder `app` (expanded, with `index.php`) and the file `readme.txt`, and the
    /// empty folder `logs`.
    fn state() -> RemoteHostState {
        let mut tree = RemoteTree::default();
        tree.root = Some("/srv".to_owned());
        tree.listings.insert(
            "/srv".to_owned(),
            vec![entry("/srv/app", true), entry("/srv/logs", true), entry("/srv/readme.txt", false)],
        );
        tree.listings.insert("/srv/app".to_owned(), vec![entry("/srv/app/index.php", false)]);
        tree.expanded.insert("/srv/app".to_owned());
        RemoteHostState {
            servers: vec![
                HostServer { id: "s1".to_owned(), label: "Staging".to_owned() },
                HostServer { id: "s2".to_owned(), label: "Live".to_owned() },
            ],
            server: Some("s1".to_owned()),
            tree: Some(tree),
        }
    }

    fn open(
        cx: &mut TestAppContext,
        state: RemoteHostState,
    ) -> (Entity<RemoteHostView>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let (view, cx) = cx.add_window_view(|_, cx| RemoteHostView::new(cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &RemoteHostEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        view.update(cx, |view, cx| view.set_state(state, cx));
        cx.run_until_parked();
        (view, events, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
        cx.simulate_click(bounds.center(), modifiers);
        cx.run_until_parked();
    }

    fn labels(view: &Entity<RemoteHostView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| {
            view.rows().into_iter().map(|row| format!("{}{}", "  ".repeat(row.depth), row.label)).collect()
        })
    }

    fn menu_rows(view: &Entity<RemoteHostView>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        view.read_with(cx, |view, _| {
            view.menu_items()
                .into_iter()
                .filter_map(|item| match item {
                    MenuItem::Item(entry) => Some((entry.label, !entry.disabled)),
                    _ => None,
                })
                .collect()
        })
    }

    /// Folders and files show with the expanded folder's entries under it, an expanded empty folder
    /// says so, and the root path shows above the tree.
    #[gpui_kit::test]
    async fn shows_the_servers_files_with_the_expanded_folders_entries_under_them(cx: &mut TestAppContext) {
        let mut state = state();
        if let Some(tree) = state.tree.as_mut() {
            tree.listings.insert("/srv/logs".to_owned(), Vec::new());
            tree.expanded.insert("/srv/logs".to_owned());
        }
        let (view, _, cx) = open(cx, state);
        assert_eq!(labels(&view, cx), ["app", "  index.php", "logs", "  Empty folder", "readme.txt"]);
        assert!(cx.debug_bounds("remote-host-root").is_some());
        assert_eq!(view.read_with(cx, |view, _| view.server_label().map(str::to_owned)).as_deref(), Some("Staging"));
    }

    /// Without servers the window offers Configure…, which runs Deployment Configuration….
    #[gpui_kit::test]
    async fn without_servers_the_window_offers_to_configure_them(cx: &mut TestAppContext) {
        let (_, events, cx) = open(cx, RemoteHostState::default());
        assert!(cx.debug_bounds("remote-host-tree").is_none());
        click(cx, "remote-host-empty-configure", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::RunAction(CONFIGURE_ACTION)));
    }

    /// While the root is not listed the window says it is connecting; a failure shows its error
    /// with Retry.
    #[gpui_kit::test]
    async fn connecting_then_an_error_with_retry(cx: &mut TestAppContext) {
        let mut waiting = state();
        waiting.tree = Some(RemoteTree::default());
        let (view, events, cx) = open(cx, waiting);
        assert!(cx.debug_bounds("remote-host-connecting").is_some());
        let mut failed = state();
        let mut tree = RemoteTree::default();
        tree.error = Some("Connection refused".to_owned());
        failed.tree = Some(tree);
        view.update(cx, |view, cx| view.set_state(failed, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("remote-host-error").is_some());
        click(cx, "remote-host-retry", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::Retry));
    }

    /// The toolbar's buttons ask for the server chooser, Refresh, Collapse All and the settings.
    #[gpui_kit::test]
    async fn the_toolbar_chooses_the_server_refreshes_collapses_and_configures(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, state());
        click(cx, "remote-host-server", Modifiers::default());
        assert!(matches!(events.borrow().last(), Some(RemoteHostEvent::ShowServerMenu { .. })));
        click(cx, "remote-host-refresh", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::Refresh));
        click(cx, "remote-host-collapse-all", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::CollapseAll));
        click(cx, "remote-host-configure", Modifiers::default());
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::RunAction(CONFIGURE_ACTION)));
        let enabled: Vec<bool> =
            view.read_with(cx, |view, _| view.toolbar().iter().map(|button| button.enabled).collect());
        assert_eq!(enabled, [true, true, true]);
    }

    /// A folder's chevron and Right ask to show its entries, Left to hide them; Enter or a double
    /// click on a file opens it.
    #[gpui_kit::test]
    async fn folders_expand_from_the_chevron_and_the_keyboard_and_files_open(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, state());
        click(cx, "remote-host-toggle:/srv/logs", Modifiers::default());
        assert_eq!(
            events.borrow().last(),
            Some(&RemoteHostEvent::Expand { path: "/srv/logs".to_owned(), expanded: true })
        );
        cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
        click(cx, "remote-host-row:/srv/app", Modifiers::default());
        cx.simulate_keystrokes("left");
        assert_eq!(
            events.borrow().last(),
            Some(&RemoteHostEvent::Expand { path: "/srv/app".to_owned(), expanded: false })
        );
        cx.simulate_keystrokes("down enter");
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::Open(entry("/srv/app/index.php", false))));
        let bounds = cx.debug_bounds("remote-host-row:/srv/readme.txt").expect("the file is drawn");
        cx.simulate_event(gpui_kit::MouseDownEvent {
            position: bounds.center(),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position: bounds.center(),
            button: MouseButton::Left,
            modifiers: Modifiers::default(),
            click_count: 2,
        });
        cx.run_until_parked();
        assert_eq!(events.borrow().last(), Some(&RemoteHostEvent::Open(entry("/srv/readme.txt", false))));
    }

    /// A right click on a row outside the selection selects that row alone before the menu opens;
    /// one on a row of a wider selection keeps the selection.
    #[gpui_kit::test]
    async fn a_right_click_selects_the_row_before_its_menu_opens(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, state());
        click(cx, "remote-host-row:/srv/app", Modifiers::default());
        let at = cx.debug_bounds("remote-host-row:/srv/readme.txt").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(RemoteHostEvent::ShowMenu { .. })));
        assert_eq!(view.read_with(cx, |view, _| view.selected().to_vec()), ["/srv/readme.txt"]);
        click(cx, "remote-host-row:/srv/app", Modifiers { platform: true, ..Modifiers::default() });
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.selected().to_vec()), ["/srv/readme.txt", "/srv/app"]);
    }

    /// The menu of one file offers every row; of a folder, New Folder… in it and no Open; of two
    /// entries, Download and Delete for both and nothing that needs one entry.
    #[gpui_kit::test]
    async fn the_context_menu_offers_what_the_selection_allows(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, state());
        let at = cx.debug_bounds("remote-host-row:/srv/readme.txt").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert!(matches!(events.borrow().last(), Some(RemoteHostEvent::ShowMenu { .. })));
        let on = |label: &str| (label.to_owned(), true);
        let off = |label: &str| (label.to_owned(), false);
        let file = [
            on("Open"),
            on("Download to Project"),
            off("New Folder…"),
            on("Rename…"),
            on("Delete…"),
            on("Refresh"),
            on("Copy Path"),
        ];
        assert_eq!(menu_rows(&view, cx), file);
        let at = cx.debug_bounds("remote-host-row:/srv/app").expect("drawn").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        assert_eq!(menu_rows(&view, cx)[0], off("Open"));
        let folder = view.read_with(cx, |view, _| view.menu_items()[3].command().cloned());
        assert_eq!(folder, Some(HostCommand::NewFolder("/srv/app".to_owned())));
        let refresh = view.read_with(cx, |view, _| view.menu_items()[7].command().cloned());
        assert_eq!(refresh, Some(HostCommand::RefreshFolder("/srv/app".to_owned())));
        click(cx, "remote-host-row:/srv/readme.txt", Modifiers { platform: true, ..Modifiers::default() });
        assert_eq!(view.read_with(cx, |view, _| view.selected().to_vec()), ["/srv/app", "/srv/readme.txt"]);
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let two = [
            off("Open"),
            on("Download to Project"),
            on("New Folder…"),
            off("Rename…"),
            on("Delete…"),
            on("Refresh"),
            off("Copy Path"),
        ];
        assert_eq!(menu_rows(&view, cx), two);
        let delete = view.read_with(cx, |view, _| view.menu_items()[5].command().cloned());
        let both = vec![entry("/srv/app", true), entry("/srv/readme.txt", false)];
        assert_eq!(delete, Some(HostCommand::Delete(both)));
    }
}
