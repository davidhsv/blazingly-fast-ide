//! The Pull Requests tool window: while gh is not ready it says why (with Install gh…, Run gh
//! auth login and Retry), with several GitHub repositories and none chosen it offers the
//! repository selector, and once connected it shows the content tabs: the list tab named after
//! the repository, one `#N` tab per opened pull request and "New Pull Request at {repo}".
//!
//! [`PullRequestsWindow`] owns the tabs ([`TabsState`]) and the list tab's [`PrListPanel`], and
//! keeps the list's last search and history in [`UiStorage`]. The workbench owns the connection
//! ([`RepositoryState`]), passes each new state in with [`PullRequestsWindow::set_repository`], and
//! carries out the [`PullRequestsEvent`]s, the list's included.

use super::data_model::{SearchValue, repository_display_name};
use super::list_panel::{ListPanelEvent, PrListPanel, tx};
use super::repository::RepositoryState;
use super::search_history::{history_key, history_value, initial_search, last_search_key, parse_history};
use super::toolwindow::{GhTab, TabItem, TabsState, ToolWindowContent, Unavailable, content, tab_items, unavailable};
use super::types::Coordinates;
use crate::platform::i18n::Localization;
use crate::platform::storage::UiStorage;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Stateful, Subscription, Window, deferred, div,
    prelude::*, px,
};
use ide_github::GhAvailability;
use serde_json::{Value, json};
use std::collections::HashMap;

/// The tool window's id (`registerToolWindow` in `src/features/github/index.ts`).
pub const PULL_REQUESTS_TOOL_WINDOW: &str = "pullRequests";

/// Where "Install gh…" leads.
pub const GH_INSTALL_URL: &str = "https://cli.github.com";

const TAB_STRIP_HEIGHT: f32 = 28.0;

/// Commands of the tab strip's menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabCommand {
    Close(GhTab),
    CloseOthers(GhTab),
}

/// What the tool window asks of the workbench.
#[derive(Clone, Debug, PartialEq)]
pub enum PullRequestsEvent {
    /// Check gh and the project's GitHub remotes again (Retry, and the first show):
    /// `RepositoryState::begin_refresh` → `remotes_listed` → `ide_github::availability` →
    /// `availability_answered`, then [`PullRequestsWindow::set_repository`].
    Refresh,
    /// "View Pull Requests" in the selector: `RepositoryState::connect(&repo, None, true)`, then
    /// the repository and current user (`ide_github::api`), then `connected`.
    Connect(Coordinates),
    /// Open this web page in the browser.
    OpenUrl(String),
    /// Run this registered action (with this argument).
    RunAction { id: &'static str, arg: Option<Value> },
    /// The `#N` tab opened or was selected: load the pull request's details (refreshing details
    /// loaded before) and, with `timeline`, open its timeline editor without focusing it.
    PullRequestOpened { number: u32, timeline: bool },
    /// The "New Pull Request" tab opened.
    CreateOpened,
    /// A tab closed: what it showed can be dropped.
    TabClosed(GhTab),
    /// The active tab changed: the `githubPrTab` context value is [`GhTab::context`].
    ActiveTabChanged(GhTab),
    /// Show this menu at this point of the window; its rows go to
    /// [`PullRequestsWindow::run_command`].
    ShowMenu { position: Point<Pixels>, items: Vec<MenuItem<TabCommand>> },
    /// An event of the list tab for the workbench: [`ListPanelEvent::Fetch`],
    /// [`ListPanelEvent::LoadChooser`] and [`ListPanelEvent::ShowMenu`] (the others the window
    /// handles itself, and passes on for information).
    List(ListPanelEvent),
}

pub struct PullRequestsWindow {
    focus: FocusHandle,
    project_root: String,
    repo: RepositoryState,
    tabs: TabsState,
    list: Entity<PrListPanel>,
    /// The repository the list shows.
    connected: Option<Coordinates>,
    /// The selector's choice, an index into the remotes.
    choice: Option<usize>,
    selector_open: bool,
    /// Loaded pull requests' titles, the `#N` tabs' tooltips.
    titles: HashMap<u32, String>,
    /// What the `#N` and New Pull Request tabs show.
    views: HashMap<GhTab, AnyView>,
    _list_events: Subscription,
}

impl EventEmitter<PullRequestsEvent> for PullRequestsWindow {}

impl PullRequestsWindow {
    /// The tool window of the project at `project_root` (the storage keys of its searches).
    pub fn new(project_root: &str, cx: &mut Context<Self>) -> Self {
        let list = cx.new(PrListPanel::new);
        let list_events =
            cx.subscribe(&list, |window: &mut Self, _, event: &ListPanelEvent, cx| window.list_event(event, cx));
        Self {
            focus: cx.focus_handle(),
            project_root: project_root.to_owned(),
            repo: RepositoryState::default(),
            tabs: TabsState::default(),
            list,
            connected: None,
            choice: None,
            selector_open: false,
            titles: HashMap::new(),
            views: HashMap::new(),
            _list_events: list_events,
        }
    }

    // -----------------------------------------------------------------------
    // From the workbench

    /// The connection's new state. Another repository (or none) resets the tabs; a new one has
    /// the list start with the stored last search.
    pub fn set_repository(&mut self, repo: RepositoryState, cx: &mut Context<Self>) {
        let next = repo.connection.as_ref().map(|c| c.coordinates.clone());
        let changed = match (&self.connected, &next) {
            (Some(a), Some(b)) => !a.same(b),
            (None, None) => false,
            _ => true,
        };
        if changed {
            let closed: Vec<GhTab> = self.tabs.tabs.clone();
            let was_list = self.tabs.active == GhTab::List;
            self.tabs.reset();
            self.titles.clear();
            self.views.clear();
            for tab in closed {
                cx.emit(PullRequestsEvent::TabClosed(tab));
            }
            if !was_list {
                cx.emit(PullRequestsEvent::ActiveTabChanged(GhTab::List));
            }
            match &repo.connection {
                Some(connection) => {
                    let (initial, history) = self.stored_search(cx);
                    self.list.update(cx, |list, cx| list.connect(connection, initial, history, cx));
                }
                None => self.list.update(cx, |list, cx| list.disconnect(cx)),
            }
        }
        if self.choice.is_some_and(|i| i >= repo.remotes.len()) {
            self.choice = None;
        }
        self.connected = next;
        self.repo = repo;
        cx.notify();
    }

    /// The tool window is shown: gh is checked the first time; the list is refreshed, as IntelliJ
    /// does whenever the list tab comes into view.
    pub fn shown(&mut self, cx: &mut Context<Self>) {
        if self.repo.availability.is_none() && !self.repo.checking {
            cx.emit(PullRequestsEvent::Refresh);
        } else if self.connected.is_some() && self.tabs.active == GhTab::List {
            self.refresh_list(cx);
        }
    }

    /// "View Pull Request": opens or selects the `#N` tab.
    pub fn open_pull_request(&mut self, number: u32, timeline: bool, cx: &mut Context<Self>) {
        if self.connected.is_none() {
            return;
        }
        self.tabs.open_pull_request(number);
        cx.emit(PullRequestsEvent::PullRequestOpened { number, timeline });
        cx.emit(PullRequestsEvent::ActiveTabChanged(self.tabs.active));
        cx.notify();
    }

    /// "Create Pull Request…": opens or selects the New Pull Request tab.
    pub fn open_create(&mut self, cx: &mut Context<Self>) {
        if self.connected.is_none() {
            return;
        }
        self.tabs.open_create();
        cx.emit(PullRequestsEvent::CreateOpened);
        cx.emit(PullRequestsEvent::ActiveTabChanged(self.tabs.active));
        cx.notify();
    }

    pub fn select_tab(&mut self, tab: GhTab, cx: &mut Context<Self>) {
        if self.tabs.active == tab {
            return;
        }
        self.tabs.select(tab);
        if tab == GhTab::List {
            self.refresh_list(cx);
        }
        cx.emit(PullRequestsEvent::ActiveTabChanged(tab));
        cx.notify();
    }

    /// Closes a `#N` or New Pull Request tab; the list tab stays.
    pub fn close_tab(&mut self, tab: GhTab, cx: &mut Context<Self>) {
        if !self.tabs.tabs.contains(&tab) {
            return;
        }
        let active = self.tabs.active;
        self.tabs.close(tab);
        self.views.remove(&tab);
        cx.emit(PullRequestsEvent::TabClosed(tab));
        if self.tabs.active != active {
            cx.emit(PullRequestsEvent::ActiveTabChanged(self.tabs.active));
        }
        cx.notify();
    }

    /// What tab `tab` shows (a pull request's details, New Pull Request).
    pub fn set_tab_view(&mut self, tab: GhTab, view: AnyView, cx: &mut Context<Self>) {
        self.views.insert(tab, view);
        cx.notify();
    }

    /// The title of a loaded pull request, its tab's tooltip.
    pub fn set_title(&mut self, number: u32, title: &str, cx: &mut Context<Self>) {
        self.titles.insert(number, title.to_owned());
        cx.notify();
    }

    /// A command of a tab's menu.
    pub fn run_command(&mut self, command: TabCommand, cx: &mut Context<Self>) {
        match command {
            TabCommand::Close(tab) => self.close_tab(tab, cx),
            TabCommand::CloseOthers(keep) => {
                let others: Vec<GhTab> = self.tabs.tabs.iter().copied().filter(|t| *t != keep).collect();
                for tab in others {
                    self.close_tab(tab, cx);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Accessors

    pub fn list(&self) -> &Entity<PrListPanel> {
        &self.list
    }

    pub fn repository(&self) -> &RepositoryState {
        &self.repo
    }

    pub fn tabs(&self) -> &TabsState {
        &self.tabs
    }

    /// The pull request actions such as Checkout Pull Request… apply to: the active `#N` tab's,
    /// else the list's selection.
    pub fn context_pull_request(&self) -> Option<u32> {
        self.tabs.context_pull_request()
    }

    /// What the window shows for the connection's state.
    pub fn content(&self, cx: &mut App) -> ToolWindowContent {
        content(&self.repo, &cx.default_global::<Localization>().0)
    }

    /// The header's tabs while connected; none otherwise.
    pub fn tab_items(&self, cx: &mut App) -> Vec<TabItem> {
        let Some(repo) = &self.connected else { return Vec::new() };
        let all: Vec<Coordinates> = self.repo.remotes.iter().map(|r| r.coordinates()).collect();
        let titles = &self.titles;
        let title_of = |n: u32| titles.get(&n).cloned();
        tab_items(&self.tabs, repo, &all, &title_of, &mut cx.default_global::<Localization>().0)
    }

    /// The tabs' labels, in order.
    pub fn tab_titles(&self, cx: &mut App) -> Vec<String> {
        self.tab_items(cx).into_iter().map(|t| t.label).collect()
    }

    /// The selector's repositories (owners shown) and the index of the one chosen.
    pub fn selector_options(&self) -> (Vec<String>, Option<usize>) {
        let all: Vec<Coordinates> = self.repo.remotes.iter().map(|r| r.coordinates()).collect();
        let names = all.iter().map(|c| repository_display_name(&all, c, true)).collect();
        (names, self.chosen_remote())
    }

    /// The "⋮" menu's own rows: Change Repository or Account… while connected with several
    /// GitHub repositories.
    pub fn header_menu(&self) -> Vec<MenuItem<TabCommand>> {
        if self.connected.is_some() && self.repo.remotes.len() > 1 {
            vec![MenuItem::action("github.changeRepository")]
        } else {
            Vec::new()
        }
    }

    /// A tab's context menu: Close Tab and Close Other Tabs (English labels: the menu translates
    /// them).
    pub fn tab_menu(&self, tab: GhTab) -> Vec<MenuItem<TabCommand>> {
        let others = self.tabs.tabs.iter().any(|t| *t != tab);
        vec![
            MenuItem::entry("Close Tab", (tab != GhTab::List).then_some(TabCommand::Close(tab))),
            MenuItem::entry("Close Other Tabs", others.then_some(TabCommand::CloseOthers(tab))),
        ]
    }

    // -----------------------------------------------------------------------

    fn chosen_remote(&self) -> Option<usize> {
        let remotes = &self.repo.remotes;
        self.choice.filter(|i| *i < remotes.len()).or_else(|| {
            let default = self.repo.default_remote.as_ref();
            let preselected = remotes.iter().position(|r| default.is_some_and(|d| r.coordinates().same(d)));
            preselected.or((!remotes.is_empty()).then_some(0))
        })
    }

    /// The stored last search (else every open pull request) and the stored history.
    fn stored_search(&self, cx: &App) -> (SearchValue, Vec<SearchValue>) {
        let read = |key: String| -> Option<Value> {
            cx.try_global::<UiStorage>().and_then(|s| s.get(&key)).and_then(|text| serde_json::from_str(text).ok())
        };
        let last = read(last_search_key(&self.project_root));
        let history = read(history_key(&self.project_root));
        (initial_search(last.as_ref()), parse_history(history.as_ref()))
    }

    fn store(&self, key: String, value: Value, cx: &mut App) {
        if cx.has_global::<UiStorage>() {
            cx.global_mut::<UiStorage>().set(&key, value.to_string());
        }
    }

    fn refresh_list(&mut self, cx: &mut Context<Self>) {
        let loaded = self.list.read(cx).list().loaded;
        if loaded {
            self.list.update(cx, |list, cx| list.reload(cx));
        }
    }

    fn list_event(&mut self, event: &ListPanelEvent, cx: &mut Context<Self>) {
        match event {
            ListPanelEvent::Selected(number) => self.tabs.set_list_selection(*number),
            ListPanelEvent::Open(number) => self.open_pull_request(*number, true, cx),
            ListPanelEvent::SaveLastSearch(v) => {
                let value = serde_json::to_value(v).unwrap_or(Value::Null);
                self.store(last_search_key(&self.project_root), value, cx);
            }
            ListPanelEvent::SaveHistory(history) => {
                self.store(history_key(&self.project_root), history_value(history), cx);
            }
            ListPanelEvent::Fetch(_) | ListPanelEvent::LoadChooser(_) | ListPanelEvent::ShowMenu { .. } => {}
        }
        cx.emit(PullRequestsEvent::List(event.clone()));
    }
}

impl Focusable for PullRequestsWindow {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.connected.is_some() && self.tabs.active == GhTab::List {
            self.list.focus_handle(cx)
        } else {
            self.focus.clone()
        }
    }
}

/// A small button with a label.
fn button(cx: &App, id: &'static str, label: String, primary: bool, enabled: bool) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .flex_none()
        .h(theme::length(cx, "h-control"))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(theme::length(cx, "radius-small"))
        .when(primary, |el| el.bg(theme::color(cx, "accent")).text_color(theme::color(cx, "bg-island")))
        .when(!primary, |el| el.border_1().border_color(theme::color(cx, "border-strong")))
        .when(!enabled, |el| el.opacity(0.5))
        .child(SharedString::from(label))
}

fn icon_button(cx: &App, id: &'static str, icon: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.to_owned())
        .size(px(22.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(theme::length(cx, "radius-small"))
        .hover(|el| el.bg(theme::color(cx, "bg-hover")))
        .child(theme::icon(cx, icon, 16.0, theme::color(cx, "fg-icon")))
}

impl PullRequestsWindow {
    fn centred(cx: &App, selector: &'static str) -> Div {
        div()
            .debug_selector(move || selector.to_owned())
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .p(px(16.0))
            .text_color(theme::color(cx, "fg-secondary"))
    }

    fn render_unavailable(&self, u: Unavailable, cx: &mut Context<Self>) -> Div {
        let availability = self.repo.availability.clone();
        let (gh, login) = ("gh", "gh auth login");
        let explanation: Vec<String> = match availability {
            Some(GhAvailability::NotInstalled) => vec![format!(
                "{} {gh} {} {login}.",
                tx(cx, "Pull requests use the"),
                tx(cx, "command line tool. Install it, then run")
            )],
            Some(GhAvailability::NotLoggedIn { .. }) => {
                vec![format!("{} {login} {}", tx(cx, "Run"), tx(cx, "in a terminal, then retry."))]
            }
            Some(GhAvailability::NotGitHub { .. }) => vec![tx(cx, "This project has no remote on GitHub.")],
            _ => Vec::new(),
        };
        let (install, run_login, retry) = (tx(cx, "Install gh…"), tx(cx, "Run gh auth login"), tx(cx, "Retry"));
        let strong = theme::color(cx, "fg-strong");
        let buttons = div()
            .flex()
            .gap(px(6.0))
            .when(u.install, |el| {
                el.child(button(cx, "gh-install", install, false, true).on_click(cx.listener(
                    |_, _: &ClickEvent, _, cx| cx.emit(PullRequestsEvent::OpenUrl(GH_INSTALL_URL.to_owned())),
                )))
            })
            .when(u.login, |el| {
                el.child(button(cx, "gh-login", run_login, true, true).on_click(cx.listener(
                    |_, _: &ClickEvent, _, cx| {
                        cx.emit(PullRequestsEvent::RunAction {
                            id: "terminal.runCommand",
                            arg: Some(json!({ "program": "gh", "args": ["auth", "login"], "name": "gh auth login" })),
                        });
                    },
                )))
            })
            .child(
                button(cx, "gh-retry", retry, false, true)
                    .on_click(cx.listener(|_, _: &ClickEvent, _, cx| cx.emit(PullRequestsEvent::Refresh))),
            );
        Self::centred(cx, "gh-unavailable")
            .child(div().text_color(strong).child(SharedString::from(u.title)))
            .children(explanation.into_iter().map(|line| div().child(SharedString::from(line))))
            .children(u.message.map(|m| {
                div()
                    .debug_selector(|| "gh-unavailable-message".to_owned())
                    .text_size(theme::length(cx, "fs-small"))
                    .child(SharedString::from(m))
            }))
            .child(buttons)
    }

    fn render_selector(&self, cx: &mut Context<Self>) -> Div {
        let (names, chosen) = self.selector_options();
        let target = chosen.and_then(|i| self.repo.remotes.get(i)).map(|r| r.coordinates());
        let connecting = self.repo.connecting;
        let label = chosen.and_then(|i| names.get(i).cloned()).unwrap_or_default();
        let view = tx(cx, "View Pull Requests");
        let can_connect = target.is_some() && !connecting;
        let select = div()
            .id("gh-repository-select")
            .debug_selector(|| "gh-repository-select".to_owned())
            .flex_none()
            .min_w(px(180.0))
            .h(theme::length(cx, "h-control"))
            .px(px(8.0))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(6.0))
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-field"))
            .text_color(theme::color(cx, "fg-strong"))
            .child(SharedString::from(label))
            .child(theme::icon(cx, "chevron-down", 12.0, theme::color(cx, "fg-icon")))
            .on_click(cx.listener(|window, _: &ClickEvent, _, cx| {
                window.selector_open = !window.selector_open;
                cx.notify();
            }));
        let connect = button(cx, "gh-view-pull-requests", view, true, can_connect).when(can_connect, |el| {
            el.on_click(cx.listener(move |window, _: &ClickEvent, _, cx| {
                window.selector_open = false;
                if let Some(target) = &target {
                    cx.emit(PullRequestsEvent::Connect(target.clone()));
                }
            }))
        });
        let options = self.selector_open.then(|| {
            let (hover, selected) = (theme::color(cx, "bg-hover"), theme::color(cx, "bg-tree-selected"));
            let rows: Vec<AnyElement> = names
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    div()
                        .id(index)
                        .debug_selector(move || format!("gh-repository-option-{index}"))
                        .h(theme::length(cx, "h-row"))
                        .px(px(8.0))
                        .flex()
                        .items_center()
                        .rounded(px(4.0))
                        .when(chosen == Some(index), |el| el.bg(selected))
                        .when(chosen != Some(index), |el| el.hover(|el| el.bg(hover)))
                        .child(SharedString::from(name.clone()))
                        .on_click(cx.listener(move |window, _: &ClickEvent, _, cx| {
                            window.choice = Some(index);
                            window.selector_open = false;
                            cx.notify();
                        }))
                        .into_any_element()
                })
                .collect();
            let list = div()
                .w(px(240.0))
                .mt(px(4.0))
                .p(px(4.0))
                .flex()
                .flex_col()
                .rounded(px(8.0))
                .border_1()
                .border_color(theme::color(cx, "border-popup"))
                .bg(theme::color(cx, "bg-popup"))
                .text_color(theme::color(cx, "fg-strong"))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .children(rows);
            deferred(
                div()
                    .id("gh-repository-backdrop")
                    .absolute()
                    .inset_0()
                    .occlude()
                    .pt(px(12.0 + 26.0))
                    .px(px(12.0))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|window, _, _, cx| {
                            window.selector_open = false;
                            cx.notify();
                        }),
                    )
                    .child(list),
            )
            .with_priority(1)
        });
        div()
            .debug_selector(|| "gh-repository-selector".to_owned())
            .size_full()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(select)
                    .child(connect)
                    .when(connecting, |el| el.child(SharedString::from(tx(cx, "Connecting to GitHub…")))),
            )
            .children(self.repo.connection_error.clone().map(|error| {
                div()
                    .debug_selector(|| "gh-connection-error".to_owned())
                    .text_color(theme::color(cx, "danger"))
                    .child(SharedString::from(error))
            }))
            .children(options)
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> Div {
        let items = self.tab_items(cx);
        let active = self.tabs.active;
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let (selected_bg, hover) = (theme::color(cx, "bg-tab-selected"), theme::color(cx, "bg-hover"));
        let mut strip = div()
            .h(px(TAB_STRIP_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"));
        for (index, item) in items.into_iter().enumerate() {
            let tab = item.tab;
            let selector = format!("gh-tab-{index}");
            let close_selector = format!("gh-tab-close-{index}");
            let selected = tab == active;
            strip = strip.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .h(px(24.0))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .pl(px(8.0))
                    .pr(px(if item.closable { 2.0 } else { 8.0 }))
                    .rounded(theme::length(cx, "radius-small"))
                    .text_color(fg)
                    .when(selected, |el| el.bg(selected_bg))
                    .when(!selected, |el| el.hover(|el| el.bg(hover)))
                    .child(SharedString::from(item.label))
                    .on_click(cx.listener(move |window, _: &ClickEvent, _, cx| window.select_tab(tab, cx)))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |window, event: &MouseDownEvent, _, cx| {
                            let items = window.tab_menu(tab);
                            cx.emit(PullRequestsEvent::ShowMenu { position: event.position, items });
                        }),
                    )
                    .when(item.closable, |el| {
                        el.child(
                            div()
                                .id(SharedString::from(close_selector.clone()))
                                .debug_selector(move || close_selector.clone())
                                .size(px(16.0))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.0))
                                .hover(|el| el.bg(hover))
                                .child(theme::icon(cx, "close", 12.0, muted))
                                .on_click(cx.listener(move |window, _: &ClickEvent, _, cx| {
                                    cx.stop_propagation();
                                    window.close_tab(tab, cx);
                                })),
                        )
                    }),
            );
        }
        let header_menu = self.header_menu();
        strip
            .child(div().flex_1())
            .child(icon_button(cx, "gh-create-pull-request", "plus").on_click(cx.listener(
                |_, _: &ClickEvent, _, cx| {
                    cx.emit(PullRequestsEvent::RunAction { id: "github.createPullRequest", arg: None });
                },
            )))
            .child(icon_button(cx, "gh-select-opened", "locate").on_click(cx.listener(|_, _: &ClickEvent, _, cx| {
                cx.emit(PullRequestsEvent::RunAction { id: "github.selectOpenedPullRequest", arg: None });
            })))
            .when(!header_menu.is_empty(), |el| {
                el.child(icon_button(cx, "gh-header-menu", "more").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |_, event: &MouseDownEvent, _, cx| {
                        cx.emit(PullRequestsEvent::ShowMenu { position: event.position, items: header_menu.clone() });
                    }),
                ))
            })
    }

    fn render_connected(&self, cx: &mut Context<Self>) -> Div {
        let tabs = self.render_tabs(cx);
        let body: AnyElement = match self.tabs.active {
            GhTab::List => self.list.clone().into_any_element(),
            tab => match self.views.get(&tab) {
                Some(view) => view.clone().into_any_element(),
                None => {
                    let loading = tx(cx, "Loading…");
                    Self::centred(cx, "gh-tab-loading").child(SharedString::from(loading)).into_any_element()
                }
            },
        };
        let content = div().debug_selector(|| "gh-tab-content".to_owned()).flex_1().min_h_0().flex().flex_col();
        div().size_full().flex().flex_col().child(tabs).child(content.child(body))
    }
}

impl Render for PullRequestsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let shown = self.content(cx);
        let body = match shown {
            ToolWindowContent::Connecting { .. } => {
                let text = tx(cx, "Connecting to GitHub…");
                Self::centred(cx, "gh-connecting").child(SharedString::from(text))
            }
            ToolWindowContent::Unavailable(u) => self.render_unavailable(u, cx),
            ToolWindowContent::Selector => self.render_selector(cx),
            ToolWindowContent::Connected => self.render_connected(cx),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .child(body)
    }
}

/// `unavailable` for a test or a caller that has only gh's state.
pub fn unavailable_title(availability: &GhAvailability, cx: &mut App) -> Option<String> {
    unavailable(availability, &cx.default_global::<Localization>().0).map(|u| u.title)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::data_model::fixtures::user;
    use crate::github::list::fixtures::query_of;
    use crate::github::new_comments::fixtures::variables;
    use crate::github::repository::{GhConnection, GhRemote};
    use crate::github::types::GhRepository;
    use crate::platform::input;
    use crate::workbench::tool_window_declaration;
    use gpui_kit::{Bounds, Modifiers, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<PullRequestsEvent>>>;

    const ROOT: &str = "/p";

    fn open(cx: &mut TestAppContext) -> (Entity<PullRequestsWindow>, Events, &mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            cx.set_global(UiStorage::default());
            input::init(cx);
        });
        let (view, cx) = cx.add_window_view(|_, cx| PullRequestsWindow::new(ROOT, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &PullRequestsEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn ready() -> GhAvailability {
        GhAvailability::Ready {
            repo: ide_github::GhRepo {
                name_with_owner: "org/app".into(),
                url: String::new(),
                default_branch: None,
                merge_commit_allowed: true,
                squash_merge_allowed: true,
                rebase_merge_allowed: true,
                viewer_permission: String::new(),
            },
            login: Some("me".into()),
        }
    }

    fn remote(owner: &str, name: &str) -> GhRemote {
        GhRemote { owner: owner.into(), name: name.into(), ..GhRemote::default() }
    }

    fn state(availability: Option<GhAvailability>, remotes: Vec<GhRemote>) -> RepositoryState {
        let mut repo = RepositoryState::default();
        repo.availability = availability;
        repo.remotes = remotes;
        repo
    }

    fn connected(remotes: Vec<GhRemote>) -> RepositoryState {
        let mut repo = state(Some(ready()), remotes);
        repo.connection = Some(GhConnection {
            coordinates: Coordinates::new("org", "app"),
            repository: GhRepository::default(),
            current_user: user("me"),
            root: None,
            remotes: Vec::new(),
        });
        repo
    }

    fn set(view: &Entity<PullRequestsWindow>, repo: RepositoryState, cx: &mut VisualTestContext) {
        view.update(cx, |view, cx| view.set_repository(repo, cx));
        cx.run_until_parked();
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
        cx.run_until_parked();
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn drain(events: &Events) -> Vec<PullRequestsEvent> {
        events.borrow_mut().drain(..).collect()
    }

    fn list_fetches(events: &Events) -> Vec<String> {
        drain(events)
            .into_iter()
            .filter_map(|e| match e {
                PullRequestsEvent::List(ListPanelEvent::Fetch(r)) => Some(query_of(&r)),
                _ => None,
            })
            .collect()
    }

    // toolwindow/toolwindow.test.tsx › tool window › registers the window, the timeline editor and the actions
    #[gpui_kit::test]
    async fn registers_the_window_and_offers_its_header_actions(cx: &mut TestAppContext) {
        let declared = tool_window_declaration(PULL_REQUESTS_TOOL_WINDOW).expect("the window is declared");
        assert_eq!((declared.title.as_str(), declared.icon.as_str()), ("Pull Requests", "toolwindow-pull-requests"));
        let (view, events, cx) = open(cx);
        set(&view, connected(vec![remote("org", "app"), remote("me", "app")]), cx);
        drain(&events);
        click(cx, "gh-create-pull-request");
        click(cx, "gh-select-opened");
        assert_eq!(
            drain(&events),
            [
                PullRequestsEvent::RunAction { id: "github.createPullRequest", arg: None },
                PullRequestsEvent::RunAction { id: "github.selectOpenedPullRequest", arg: None },
            ]
        );
        let menu = view.read_with(cx, |view, _| view.header_menu());
        assert_eq!(menu.iter().map(MenuItem::action_id).collect::<Vec<_>>(), [Some("github.changeRepository")]);
        set(&view, connected(vec![remote("org", "app")]), cx);
        assert!(view.read_with(cx, |view, _| view.header_menu()).is_empty());
        assert!(cx.debug_bounds("gh-header-menu").is_none());
    }

    // Not from a spec: the first show checks gh; until it answers the window says it is connecting.
    #[gpui_kit::test]
    async fn checks_gh_when_first_shown_and_says_it_is_connecting(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        assert_eq!(view.update(cx, |view, cx| view.content(cx)), ToolWindowContent::Connecting { spinner: false });
        assert!(cx.debug_bounds("gh-connecting").is_some());
        view.update(cx, |view, cx| view.shown(cx));
        assert_eq!(drain(&events), [PullRequestsEvent::Refresh]);
    }

    // toolwindow/toolwindow.test.tsx › tool window › shows gh's state when it is not ready
    #[gpui_kit::test]
    async fn shows_ghs_state_when_it_is_not_ready(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        set(&view, state(Some(GhAvailability::NotInstalled), Vec::new()), cx);
        let ToolWindowContent::Unavailable(u) = view.update(cx, |view, cx| view.content(cx)) else {
            panic!("gh's state is shown")
        };
        assert_eq!(u.title, "GitHub CLI is not installed");
        assert!(cx.debug_bounds("gh-unavailable").is_some() && cx.debug_bounds("gh-login").is_none());
        click(cx, "gh-install");
        click(cx, "gh-retry");
        assert_eq!(drain(&events), [PullRequestsEvent::OpenUrl(GH_INSTALL_URL.to_owned()), PullRequestsEvent::Refresh]);
        set(&view, state(Some(GhAvailability::NotLoggedIn { message: String::new() }), Vec::new()), cx);
        click(cx, "gh-login");
        let seen = drain(&events);
        let [PullRequestsEvent::RunAction { id: "terminal.runCommand", arg: Some(arg) }] = seen.as_slice() else {
            panic!("gh auth login runs in a terminal: {seen:?}")
        };
        assert_eq!(arg["args"], json!(["auth", "login"]));
        let message = "no remote".to_owned();
        set(&view, state(Some(GhAvailability::NotGitHub { message: message.clone() }), Vec::new()), cx);
        assert!(cx.debug_bounds("gh-unavailable-message").is_some());
        assert_eq!(
            cx.update(|_, cx| unavailable_title(&GhAvailability::NotGitHub { message }, cx)).as_deref(),
            Some("No GitHub repository")
        );
    }

    // toolwindow/toolwindow.test.tsx › tool window › offers the repository selector for several GitHub remotes
    #[gpui_kit::test]
    async fn offers_the_repository_selector_for_several_github_remotes(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mut repo = state(Some(ready()), vec![remote("me", "app"), remote("org", "app")]);
        repo.default_remote = Some(Coordinates::new("org", "app"));
        set(&view, repo, cx);
        assert_eq!(view.update(cx, |view, cx| view.content(cx)), ToolWindowContent::Selector);
        assert_eq!(
            view.read_with(cx, |view, _| view.selector_options()),
            (vec!["me/app".to_owned(), "org/app".to_owned()], Some(1))
        );
        click(cx, "gh-view-pull-requests");
        assert_eq!(drain(&events), [PullRequestsEvent::Connect(Coordinates::new("org", "app"))]);
        click(cx, "gh-repository-select");
        click(cx, "gh-repository-option-0");
        assert_eq!(view.read_with(cx, |view, _| view.selector_options()).1, Some(0));
        click(cx, "gh-view-pull-requests");
        assert_eq!(drain(&events), [PullRequestsEvent::Connect(Coordinates::new("me", "app"))]);
        // A failed connection says why under the selector.
        let mut failed = state(Some(ready()), vec![remote("me", "app"), remote("org", "app")]);
        failed.connection_error = Some("Could not connect".into());
        set(&view, failed, cx);
        assert!(cx.debug_bounds("gh-connection-error").is_some());
    }

    // toolwindow/toolwindow.test.tsx › tool window › shows the list tab, named after the repository, once connected
    #[gpui_kit::test]
    async fn shows_the_list_tab_named_after_the_repository_once_connected(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        set(&view, connected(vec![remote("org", "app")]), cx);
        assert_eq!(view.update(cx, |view, cx| view.content(cx)), ToolWindowContent::Connected);
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["app"]);
        let first = drain(&events)
            .into_iter()
            .find_map(|e| match e {
                PullRequestsEvent::List(ListPanelEvent::Fetch(r)) => Some(r),
                _ => None,
            })
            .expect("the list asks for its first page");
        assert_eq!(query_of(&first), "type:pr repo:org/app is:open");
        assert_eq!(variables(&first.request)["pageSize"], 50);
        let items = view.update(cx, |view, cx| view.tab_items(cx));
        assert_eq!((items[0].tooltip.as_deref(), items[0].closable), (Some("org/app"), false));
        view.update(cx, |view, cx| {
            view.open_pull_request(4, true, cx);
            view.set_title(4, "Fix the thing", cx);
        });
        cx.run_until_parked();
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["app", "#4"]);
        assert_eq!(view.update(cx, |view, cx| view.tab_items(cx))[1].tooltip.as_deref(), Some("Fix the thing"));
        assert_eq!(
            drain(&events),
            [
                PullRequestsEvent::PullRequestOpened { number: 4, timeline: true },
                PullRequestsEvent::ActiveTabChanged(GhTab::PullRequest(4)),
            ]
        );
        assert!(cx.debug_bounds("gh-tab-loading").is_some(), "the tab waits for its view");
        assert_eq!(view.read_with(cx, |view, _| view.context_pull_request()), Some(4));
        click(cx, "gh-tab-close-1");
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["app"]);
        assert_eq!(
            drain(&events),
            [PullRequestsEvent::TabClosed(GhTab::PullRequest(4)), PullRequestsEvent::ActiveTabChanged(GhTab::List),]
        );
    }

    // Not from a spec: Enter in the list opens the pull request's tab; the tab menu closes the others.
    #[gpui_kit::test]
    async fn opening_from_the_list_adds_a_tab_and_the_tab_menu_closes_the_others(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        set(&view, connected(vec![remote("org", "app")]), cx);
        let list = view.read_with(cx, |view, _| view.list().clone());
        list.update(cx, |_, cx| cx.emit(ListPanelEvent::Selected(Some(5))));
        assert_eq!(view.read_with(cx, |view, _| view.context_pull_request()), Some(5));
        list.update(cx, |_, cx| cx.emit(ListPanelEvent::Open(5)));
        view.update(cx, |view, cx| view.open_create(cx));
        cx.run_until_parked();
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["app", "#5", "New Pull Request at app"]);
        drain(&events);
        let at = bounds_of(cx, "gh-tab-1").center();
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
        cx.run_until_parked();
        let seen = drain(&events);
        let [PullRequestsEvent::ShowMenu { items, .. }] = seen.as_slice() else {
            panic!("the tab's menu is asked for: {seen:?}")
        };
        assert_eq!(
            items.iter().map(|i| (i.label().map(str::to_owned), i.is_disabled())).collect::<Vec<_>>(),
            [(Some("Close Tab".to_owned()), false), (Some("Close Other Tabs".to_owned()), false)]
        );
        let others = items[1].command().copied().expect("a command");
        view.update(cx, |view, cx| view.run_command(others, cx));
        assert_eq!(view.update(cx, |view, cx| view.tab_titles(cx)), ["app", "#5"]);
        let list_menu = view.read_with(cx, |view, _| view.tab_menu(GhTab::List));
        assert!(list_menu[0].is_disabled(), "the list tab does not close");
        // Back on the list tab, IntelliJ refreshes the list once it has loaded.
        click(cx, "gh-tab-0");
        assert_eq!(view.read_with(cx, |view, _| view.tabs().active), GhTab::List);
    }

    // Not from a spec: the list starts with the stored last search and stores what it saves.
    #[gpui_kit::test]
    async fn the_list_keeps_its_last_search_and_history_in_the_ui_storage(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx);
        let mine = SearchValue { author: Some("me".into()), ..SearchValue::default_search() };
        cx.update(|_, cx| {
            let text = serde_json::to_string(&mine).expect("a search serializes");
            cx.global_mut::<UiStorage>().set(&last_search_key(ROOT), text);
        });
        set(&view, connected(vec![remote("org", "app")]), cx);
        assert_eq!(list_fetches(&events), ["type:pr repo:org/app is:open author:me"]);
        let list = view.read_with(cx, |view, _| view.list().clone());
        let history = vec![mine.clone()];
        list.update(cx, |_, cx| cx.emit(ListPanelEvent::SaveHistory(history)));
        let stored = cx.update(|_, cx| cx.global::<UiStorage>().get(&history_key(ROOT)).map(str::to_owned));
        let parsed: Option<Value> = stored.and_then(|text| serde_json::from_str(&text).ok());
        assert_eq!(parse_history(parsed.as_ref()), [mine]);
        // Disconnected: no tabs, and the list forgets its rows.
        set(&view, state(Some(ready()), vec![remote("org", "app")]), cx);
        assert!(view.update(cx, |view, cx| view.tab_items(cx)).is_empty());
        assert!(view.read_with(cx, |view, cx| view.list().read(cx).list().items.is_empty()));
    }
}
