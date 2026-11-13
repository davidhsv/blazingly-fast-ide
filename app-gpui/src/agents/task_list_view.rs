//! The Agents tool window: a toolbar (New Task, the conversation placement buttons), the tasks grouped by
//! what they need from the user (each row with its status, provider, title, unread dot, branch and age),
//! their selection by pointer (⇧ for a range, ⌘/Ctrl to toggle) and keyboard (↑/↓, ⇧ to extend, Enter to
//! open), the menu of a task or of the selection, and the docked conversation with the task list in a
//! history popup.
//!
//! [`TaskListView`] reads the [`AgentsStore`] and keeps only what it shows ([`AgentsPanel`]: groups,
//! selection, history, drop highlight). What needs the store changed or the workbench is a
//! [`TaskListEvent`]; the docked conversation (or New Task page) is a view its owner hands in.

use crate::agents::model::{GroupStatus, age, branch_label, provider_icon, status_label};
use crate::agents::store::{AgentTab, AgentsStore, SessionLocation};
use crate::agents::task_actions::{self, TaskCommand, task_menu};
use crate::agents::task_list::{AgentsPanel, Click, ListKey, PanelAction, RowKey, TaskRow, location_buttons};
use crate::platform::i18n::{I18n, Localization};
use crate::platform::local_time;
use crate::theme;
use crate::vcs_ops::menu::MenuItem;
use gpui_kit::{
    AnyElement, AnyView, App, ClickEvent, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render, SharedString, Stateful,
    Subscription, Window, deferred, div, prelude::*, px,
};
use ide_agent::{AgentStatus, AgentTask, Isolation};

/// What the Agents tool window asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskListEvent {
    /// Open the task where conversations are shown ([`AgentsStore::open_task`]).
    OpenTask(String),
    /// Dock a session tab dropped on the tool window ([`AgentsStore::dock_task`]).
    DockTask(String),
    /// New Task ([`AgentsStore::open_home`]).
    NewTask,
    /// A conversation placement button ([`AgentsStore::set_session_location`]).
    SetLocation(SessionLocation),
    /// Show the menu of the right-clicked task or of the selection it belongs to, at `position`; the window
    /// carries out the chosen [`TaskCommand`] (archive, unarchive and delete through
    /// [`task_actions::TaskBatches`]).
    ShowMenu { items: Vec<MenuItem<TaskCommand>>, position: Point<Pixels> },
}

/// The task menu's rows as the workbench's popup menu takes them; Delete is the destructive row.
pub fn menu_items(items: Vec<task_actions::MenuItem>) -> Vec<MenuItem<TaskCommand>> {
    items
        .into_iter()
        .map(|item| match item {
            task_actions::MenuItem::Separator => MenuItem::Separator,
            task_actions::MenuItem::Action { command, label, icon, disabled } => {
                let danger = matches!(command, TaskCommand::Delete(_));
                let mut row = MenuItem::item(label, command).disabled(disabled);
                if let Some(icon) = icon {
                    row = row.icon(icon);
                }
                if danger { row.danger() } else { row }
            }
        })
        .collect()
}

/// The selector part of a group's rows.
fn group_id(status: GroupStatus) -> &'static str {
    match status {
        GroupStatus::NeedsInput => "needs-input",
        GroupStatus::Failed => "failed",
        GroupStatus::Limited => "limited",
        GroupStatus::Running => "running",
        GroupStatus::Ready => "ready",
        GroupStatus::Archived => "archived",
    }
}

/// The icon and colour token of a task's status.
fn status_icon(status: AgentStatus) -> (&'static str, &'static str) {
    match status {
        AgentStatus::NeedsInput => ("warning", "warning"),
        AgentStatus::Running | AgentStatus::Starting => ("refresh", "fg-muted"),
        AgentStatus::Ready => ("check", "success"),
        AgentStatus::Failed => ("close", "danger"),
        AgentStatus::Limited => ("clock", "danger"),
        AgentStatus::Archived => ("minus", "fg-faint"),
    }
}

/// UI text in the active language.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx(text))
}

/// A row as drawn.
#[derive(Clone, Debug, PartialEq)]
enum RowView {
    Group { status: GroupStatus, label: String, count: usize, expanded: bool },
    Task(TaskView),
}

#[derive(Clone, Debug, PartialEq)]
struct TaskView {
    id: String,
    title: String,
    status: AgentStatus,
    status_label: String,
    provider_icon: String,
    /// The provider's name, with the branch of a worktree task.
    detail: String,
    age: String,
    unread: bool,
}

impl RowView {
    fn key(&self) -> RowKey {
        match self {
            Self::Group { status, .. } => RowKey::Group(*status),
            Self::Task(task) => RowKey::Task(task.id.clone()),
        }
    }
}

pub struct TaskListView {
    store: Entity<AgentsStore>,
    panel: AgentsPanel,
    focus: FocusHandle,
    /// The docked conversation or the New Task page, while the tool window shows one.
    docked: Option<AnyView>,
    clock: fn() -> f64,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<TaskListEvent> for TaskListView {}

impl TaskListView {
    /// The tool window of `store`'s tasks; `active` is the active editor tab, whose task starts selected.
    pub fn new(store: Entity<AgentsStore>, active: Option<&AgentTab>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&store, |_, _, cx| cx.notify())];
        Self {
            store,
            panel: AgentsPanel::new(active),
            focus: cx.focus_handle(),
            docked: None,
            clock: local_time::now_ms,
            _subscriptions: subscriptions,
        }
    }

    /// The clock the ages are measured against.
    pub fn with_clock(mut self, clock: fn() -> f64) -> Self {
        self.clock = clock;
        self
    }

    /// The conversation (or New Task page) shown while a task is docked or the page is open.
    pub fn set_docked_view(&mut self, view: Option<AnyView>, cx: &mut Context<Self>) {
        self.docked = view;
        cx.notify();
    }

    /// The active editor tab changed: a session tab selects its task.
    pub fn active_tab_changed(&mut self, tab: Option<&AgentTab>, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        self.panel.list.active_tab_changed(tab, store.tasks());
        cx.notify();
    }

    /// An editor tab (`tab` as the window resolved it) is dragged over the tool window.
    pub fn tab_over(&mut self, tab: Option<&AgentTab>, cx: &mut Context<Self>) {
        self.panel.tab_over(tab);
        cx.notify();
    }

    pub fn tab_left(&mut self, cx: &mut Context<Self>) {
        self.panel.tab_left();
        cx.notify();
    }

    /// A dragged editor tab was dropped on the tool window: a session tab docks.
    pub fn tab_dropped(&mut self, tab: Option<&AgentTab>, cx: &mut Context<Self>) {
        if let Some(action) = self.panel.tab_dropped(tab) {
            self.act(action, cx);
        }
        cx.notify();
    }

    pub fn is_drop_target(&self) -> bool {
        self.panel.is_drop_target()
    }

    pub fn history_open(&self) -> bool {
        self.panel.history_open()
    }

    /// Whether the task list is on screen: in the tool window, or in the open history popup.
    pub fn shows_task_list(&self, cx: &App) -> bool {
        self.panel.shows_task_list(self.store.read(cx))
    }

    /// The selected tasks, in the order they were selected.
    pub fn selected(&self) -> Vec<String> {
        self.panel.list.selected_tasks().into_iter().map(str::to_owned).collect()
    }

    /// The rows, top to bottom: `v Needs Input (1)` for an expanded group (`>` collapsed), and
    /// `<status> | <title> | <provider · branch> | <age>` for a task, with ` | unread` when it has unseen
    /// activity.
    pub fn rows(&self, cx: &App) -> Vec<String> {
        self.row_views(cx)
            .iter()
            .map(|row| match row {
                RowView::Group { label, count, expanded, .. } => {
                    format!("{} {label} ({count})", if *expanded { "v" } else { ">" })
                }
                RowView::Task(task) => {
                    let unread = if task.unread { " | unread" } else { "" };
                    format!("{} | {} | {} | {}{unread}", task.status_label, task.title, task.detail, task.age)
                }
            })
            .collect()
    }

    /// What the tool window says instead of a list: while loading, why the tasks could not be loaded,
    /// or that there are none yet.
    pub fn empty_text(&self, cx: &App) -> Option<String> {
        let store = self.store.read(cx);
        if !store.loaded {
            Some(tx(cx, "Loading…"))
        } else if let Some(error) = &store.error {
            Some(format!("{}: {error}", tx(cx, "Cannot load agent tasks")))
        } else {
            store.tasks().is_empty().then(|| tx(cx, "No agent tasks yet"))
        }
    }

    fn row_views(&self, cx: &App) -> Vec<RowView> {
        let store = self.store.read(cx);
        let now = (self.clock)();
        self.panel
            .list
            .rows(store.tasks())
            .into_iter()
            .map(|row| match row {
                TaskRow::Group { status, label, count, expanded } => {
                    RowView::Group { status, label: tx(cx, label), count, expanded }
                }
                TaskRow::Task(task) => RowView::Task(Self::task_view(store, task, now, cx)),
            })
            .collect()
    }

    fn task_view(store: &AgentsStore, task: &AgentTask, now: f64, cx: &App) -> TaskView {
        let provider = store.provider(&task.provider).map_or(task.provider.as_str(), |p| p.name.as_str());
        let detail = if task.isolation == Isolation::Worktree {
            format!("{provider} · {}", branch_label(task.branch.as_deref()))
        } else {
            provider.to_owned()
        };
        TaskView {
            id: task.id.clone(),
            title: task.title.clone(),
            status: task.status,
            status_label: tx(cx, status_label(task.status)),
            provider_icon: provider_icon(&task.provider),
            detail,
            age: age(task.updated_ms, now),
            unread: store.is_unread(&task.id),
        }
    }

    fn act(&mut self, action: PanelAction, cx: &mut Context<Self>) {
        cx.emit(match action {
            PanelAction::OpenTask(task) => TaskListEvent::OpenTask(task),
            PanelAction::DockTask(task) => TaskListEvent::DockTask(task),
        });
    }

    fn row_mouse_down(&mut self, key: &RowKey, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        // The press focuses the list; the tool window around it must not take the focus back.
        window.prevent_default();
        let modifiers = event.modifiers;
        let secondary = event.button == MouseButton::Right;
        let click = Click { shift: modifiers.shift, toggle: modifiers.platform || modifiers.control, secondary };
        let action = self.panel.mouse_down(self.store.read(cx), key, click);
        if let Some(action) = action {
            self.act(action, cx);
        }
        if secondary {
            self.show_menu(key, event.position, cx);
        }
        cx.notify();
    }

    fn show_menu(&mut self, key: &RowKey, position: Point<Pixels>, cx: &mut Context<Self>) {
        if !matches!(key, RowKey::Task(_)) {
            return;
        }
        let ids = self.panel.list.context_selection(key);
        let store = self.store.read(cx);
        let tasks: Vec<AgentTask> = ids.iter().filter_map(|id| store.task(id).cloned()).collect();
        let refs: Vec<&AgentTask> = tasks.iter().collect();
        let items = task_menu(&refs, &mut cx.default_global::<Localization>().0);
        if !items.is_empty() {
            cx.emit(TaskListEvent::ShowMenu { items: menu_items(items), position });
        }
    }

    fn row_clicked(&mut self, key: &RowKey, event: &ClickEvent, cx: &mut Context<Self>) {
        match key {
            RowKey::Group(status) => {
                let expanded = self
                    .panel
                    .list
                    .rows(self.store.read(cx).tasks())
                    .iter()
                    .any(|row| matches!(row, TaskRow::Group { status: s, expanded: true, .. } if s == status));
                self.panel.list.set_expanded(*status, !expanded);
            }
            RowKey::Task(task) => {
                let modifiers = event.modifiers();
                let toggle = modifiers.platform || modifiers.control;
                let click = Click { shift: modifiers.shift, toggle, ..Click::default() };
                let action = self.panel.clicked(self.store.read(cx), task, click);
                if let Some(action) = action {
                    self.act(action, cx);
                }
            }
        }
        cx.notify();
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform {
            return;
        }
        let key = match event.keystroke.key.as_str() {
            "up" => ListKey::Up,
            "down" => ListKey::Down,
            "enter" => ListKey::Enter,
            "escape" if self.panel.history_open() => {
                self.panel.close_history();
                cx.stop_propagation();
                cx.notify();
                return;
            }
            _ => return,
        };
        let action = self.panel.key_down(self.store.read(cx), key, modifiers.shift);
        if let Some(action) = action {
            self.act(action, cx);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The history button of the docked conversation's bar.
    pub fn toggle_history(&mut self, cx: &mut Context<Self>) {
        self.panel.toggle_history(self.store.read(cx));
        cx.notify();
    }

    fn icon_button(
        &self,
        selector: &'static str,
        icon: &'static str,
        pressed: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        div()
            .id(selector)
            .debug_selector(move || selector.to_owned())
            .size(px(24.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(theme::length(cx, "radius-small"))
            .when(pressed, |el| el.bg(theme::color(cx, "bg-selected")))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(theme::icon(cx, icon, 16.0, theme::color(cx, if pressed { "fg-strong" } else { "fg-muted" })))
            .on_click(cx.listener(move |view, _, window, cx| on_click(view, window, cx)))
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> Div {
        let english = I18n::default();
        let i18n = cx.try_global::<Localization>().map_or(&english, |localization| &localization.0);
        let buttons = location_buttons(self.store.read(cx), i18n);
        let mut location = Vec::with_capacity(buttons.len());
        for button in buttons {
            let (selector, icon) = match button.location {
                SessionLocation::ToolWindow => ("agents-use-tool-window", "open-in-tool-window"),
                SessionLocation::Editor => ("agents-use-editor", "editor-only"),
            };
            let target = button.location;
            location.push(self.icon_button(selector, icon, button.pressed, cx, move |_, _, cx| {
                cx.emit(TaskListEvent::SetLocation(target));
            }));
        }
        div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .px(px(6.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.icon_button("agents-new-task", "plus", false, cx, |_, _, cx| cx.emit(TaskListEvent::NewTask)))
            .child(div().flex().gap(px(2.0)).children(location))
    }

    fn row_element(&self, row: &RowView, selected: bool, focused: bool, cx: &mut Context<Self>) -> AnyElement {
        let key = row.key();
        let (selector, id) = match row {
            RowView::Group { status, .. } => {
                let id = format!("agents-group-{}", group_id(*status));
                (id.clone(), id)
            }
            RowView::Task(task) => {
                let id = format!("agents-task-{}", task.id);
                (id.clone(), id)
            }
        };
        let (fg, muted) = (theme::color(cx, "fg"), theme::color(cx, "fg-muted"));
        let selected_bg = theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" });
        let down_key = key.clone();
        let menu_key = key.clone();
        let base = div()
            .id(SharedString::from(id))
            .debug_selector(move || selector)
            .h(theme::length(cx, "h-row"))
            .w_full()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pr(px(8.0))
            .when(selected, |el| el.bg(selected_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    view.row_mouse_down(&down_key, event, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| {
                    view.row_mouse_down(&menu_key, event, window, cx);
                }),
            )
            .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| view.row_clicked(&key, event, cx)));
        match row {
            RowView::Group { label, count, expanded, .. } => base
                .pl(px(4.0))
                .text_color(muted)
                .child(theme::icon(cx, if *expanded { "chevron-down" } else { "chevron-right" }, 16.0, muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .child(SharedString::from(label.clone())),
                )
                .child(div().flex_none().child(SharedString::from(count.to_string())))
                .into_any_element(),
            RowView::Task(task) => {
                let (icon, color) = status_icon(task.status);
                let unread_selector = format!("agents-unread-{}", task.id);
                base.pl(px(24.0))
                    .text_color(fg)
                    .child(theme::icon(cx, icon, 13.0, theme::color(cx, color)))
                    .child(theme::icon(cx, &task.provider_icon, 14.0, muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .when(task.unread, |el| el.font_weight(FontWeight::SEMIBOLD))
                            .child(SharedString::from(task.title.clone())),
                    )
                    .when(task.unread, |el| {
                        el.child(
                            div()
                                .debug_selector(move || unread_selector)
                                .size(px(6.0))
                                .flex_none()
                                .rounded_full()
                                .bg(theme::color(cx, "accent")),
                        )
                    })
                    .child(
                        div()
                            .flex_none()
                            .max_w(px(160.0))
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_color(muted)
                            .child(SharedString::from(task.detail.clone())),
                    )
                    .child(div().flex_none().text_color(muted).child(SharedString::from(task.age.clone())))
                    .into_any_element()
            }
        }
    }

    fn task_list(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let focused = self.focus.is_focused(window);
        if let Some(text) = self.empty_text(cx) {
            let loaded = self.store.read(cx).loaded && self.store.read(cx).error.is_none();
            return div()
                .debug_selector(|| "agents-empty".to_owned())
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.0))
                .p(px(24.0))
                .text_color(theme::color(cx, "fg-muted"))
                .child(SharedString::from(text))
                .when(loaded, |el| {
                    el.child(
                        div()
                            .id("agents-empty-new-task")
                            .debug_selector(|| "agents-empty-new-task".to_owned())
                            .text_color(theme::color(cx, "link"))
                            .cursor_pointer()
                            .child(SharedString::from(tx(cx, "New Task…")))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(TaskListEvent::NewTask))),
                    )
                });
        }
        let rows = self.row_views(cx);
        let mut children = Vec::with_capacity(rows.len());
        for row in &rows {
            let selected = self.panel.list.is_selected(&row.key());
            children.push(self.row_element(row, selected, focused, cx));
        }
        div()
            .flex_1()
            .min_h_0()
            .child(div().id("agents-tasks").size_full().flex().flex_col().overflow_y_scroll().children(children))
    }

    fn docked_bar(&self, cx: &mut Context<Self>) -> Div {
        let store = self.store.read(cx);
        let title = if store.home_open {
            tx(cx, "New Agent Task")
        } else {
            let docked = store.docked.as_deref().unwrap_or_default();
            store.task(docked).map_or_else(|| docked.to_owned(), |t| t.title.clone())
        };
        let history = self.panel.history_open();
        div()
            .h(px(28.0))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(4.0))
            .px(px(4.0))
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .child(self.icon_button("agents-history", "history", history, cx, |view, _, cx| view.toggle_history(cx)))
            .child(
                div()
                    .debug_selector(|| "agents-docked-title".to_owned())
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .child(SharedString::from(title)),
            )
    }

    fn history_popup(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let popup = div()
            .debug_selector(|| "agents-history-popup".to_owned())
            .mt(px(30.0))
            .ml(px(4.0))
            .w(px(320.0))
            .max_h(px(420.0))
            .flex()
            .flex_col()
            .rounded(theme::length(cx, "radius-popup"))
            .border_1()
            .border_color(theme::color(cx, "border"))
            .bg(theme::color(cx, "bg-popup"))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(theme::color(cx, "fg-muted"))
                    .child(SharedString::from(tx(cx, "Agent sessions"))),
            )
            .child(self.task_list(window, cx));
        let backdrop = div()
            .id("agents-history-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    view.panel.close_history();
                    cx.notify();
                }),
            )
            .child(popup);
        deferred(backdrop).with_priority(1).into_any_element()
    }
}

impl Focusable for TaskListView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TaskListView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let inline = AgentsPanel::inline(self.store.read(cx));
        let drop_target = self.panel.is_drop_target();
        let root = div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .text_color(theme::color(cx, "fg"))
            .when(drop_target, |el| el.border_1().border_color(theme::color(cx, "accent")))
            .child(self.toolbar(cx));
        if !inline {
            return root.child(self.task_list(window, cx));
        }
        let root = root.child(self.docked_bar(cx)).child(div().flex_1().min_h_0().children(self.docked.clone()));
        if self.panel.history_open() { root.child(self.history_popup(window, cx)) } else { root }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::fixtures::{connected_store, task};
    use crate::agents::store::Saved;
    use gpui_kit::{Bounds, Modifiers, TestAppContext, VisualTestContext};
    use ide_agent::AgentEvent;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<TaskListEvent>>>;

    /// Ten minutes after the epoch: tasks updated at 0 are "10m" old.
    fn clock() -> f64 {
        600_000.0
    }

    fn open(
        cx: &mut TestAppContext,
        store: AgentsStore,
        active: Option<AgentTab>,
    ) -> (Entity<TaskListView>, Entity<AgentsStore>, Events, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(theme::ActiveTheme::default()));
        let store = cx.new(|_| store);
        let shared = store.clone();
        let (view, cx) =
            cx.add_window_view(move |_, cx| TaskListView::new(shared, active.as_ref(), cx).with_clock(clock));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &TaskListEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, store, events, cx)
    }

    fn tasks() -> Vec<AgentTask> {
        vec![
            task("a", AgentStatus::Ready, 3.0),
            task("b", AgentStatus::Ready, 2.0),
            task("c", AgentStatus::Ready, 1.0),
            task("d", AgentStatus::Ready, 0.0),
        ]
    }

    fn bounds_of(cx: &mut VisualTestContext, selector: &'static str) -> Bounds<gpui_kit::Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str, modifiers: Modifiers) {
        let centre = bounds_of(cx, selector).center();
        cx.simulate_click(centre, modifiers);
        cx.run_until_parked();
    }

    fn right_click(cx: &mut VisualTestContext, selector: &'static str) {
        let position = bounds_of(cx, selector).center();
        cx.simulate_event(MouseDownEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(gpui_kit::MouseUpEvent {
            position,
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
        });
        cx.run_until_parked();
    }

    fn selected(view: &Entity<TaskListView>, cx: &mut VisualTestContext) -> Vec<String> {
        view.read_with(cx, |view, _| view.selected())
    }

    fn opened(events: &Events) -> Vec<String> {
        events
            .borrow()
            .iter()
            .filter_map(|e| match e {
                TaskListEvent::OpenTask(task) => Some(task.clone()),
                _ => None,
            })
            .collect()
    }

    fn shift() -> Modifiers {
        Modifiers { shift: true, ..Modifiers::default() }
    }

    #[gpui_kit::test]
    async fn lists_tasks_grouped_by_what_they_need_with_their_details(cx: &mut TestAppContext) {
        let mut tasks = vec![task("b", AgentStatus::NeedsInput, 2.0), task("a", AgentStatus::Ready, 1.0)];
        tasks[1].isolation = Isolation::Local;
        tasks.push(task("z", AgentStatus::Archived, 0.0));
        let (view, _, _, cx) = open(cx, connected_store(tasks).0, None);
        assert_eq!(
            view.read_with(cx, |view, cx| view.rows(cx)),
            [
                "v Needs Input (1)",
                "Needs input | b | claude · b | 10m",
                "v Ready to Review (1)",
                "Ready | a | claude | 10m",
                "> Archived (1)",
            ]
        );
        // A collapsed group opens with a click.
        click(cx, "agents-group-archived", Modifiers::default());
        let rows = view.read_with(cx, |view, cx| view.rows(cx));
        assert_eq!(rows.last().map(String::as_str), Some("Archived | z | claude · z | 10m"));
    }

    #[gpui_kit::test]
    async fn extends_and_shrinks_a_range_without_opening_its_lead_or_losing_selection_to_an_editor_echo(
        cx: &mut TestAppContext,
    ) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        click(cx, "agents-task-a", Modifiers::default());
        assert_eq!(opened(&events), ["a"]);
        click(cx, "agents-task-c", shift());
        assert_eq!(selected(&view, cx), ["a", "b", "c"]);
        click(cx, "agents-task-b", shift());
        assert_eq!(selected(&view, cx), ["a", "b"]);
        assert_eq!(opened(&events), ["a"], "a range opens nothing");
        // The editor echoes the task that opened: the range stays.
        view.update(cx, |view, cx| view.active_tab_changed(Some(&AgentTab::Session("a".to_owned())), cx));
        assert_eq!(selected(&view, cx), ["a", "b"]);
    }

    #[gpui_kit::test]
    async fn keeps_the_initial_active_task_as_the_anchor_for_repeated_keyboard_extension(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, Some(AgentTab::Session("b".to_owned())));
        assert_eq!(selected(&view, cx), ["b"]);
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("shift-down shift-down");
        assert_eq!(selected(&view, cx), ["b", "c", "d"]);
        cx.simulate_keystrokes("shift-up");
        assert_eq!(selected(&view, cx), ["b", "c"]);
        assert!(opened(&events).is_empty());
        // A plain step selects the next task alone, which opens it.
        cx.simulate_keystrokes("down");
        assert_eq!(selected(&view, cx), ["d"]);
        assert_eq!(opened(&events), ["d"]);
    }

    #[gpui_kit::test]
    async fn toggles_individual_tasks_with_the_command_key(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        let command = Modifiers { platform: true, ..Modifiers::default() };
        click(cx, "agents-task-a", Modifiers::default());
        click(cx, "agents-task-c", command);
        assert_eq!(selected(&view, cx), ["a", "c"]);
        click(cx, "agents-task-a", command);
        assert_eq!(selected(&view, cx), ["c"]);
        let control = Modifiers { control: true, ..Modifiers::default() };
        click(cx, "agents-task-d", control);
        assert_eq!(selected(&view, cx), ["c", "d"]);
        assert_eq!(opened(&events), ["a"], "toggling opens nothing");
    }

    #[gpui_kit::test]
    async fn opens_session_history_on_demand_and_closes_it_after_switching_tasks(cx: &mut TestAppContext) {
        let (mut store, _) = connected_store(tasks());
        store.docked = Some("a".to_owned());
        let (view, _, events, cx) = open(cx, store, None);
        assert!(!view.read_with(cx, |view, cx| view.shows_task_list(cx)));
        assert!(cx.debug_bounds("agents-task-a").is_none());
        click(cx, "agents-history", Modifiers::default());
        assert!(view.read_with(cx, |view, _| view.history_open()));
        // The docked task is selected in the history.
        assert_eq!(selected(&view, cx), ["a"]);
        click(cx, "agents-task-b", Modifiers::default());
        assert_eq!(opened(&events), ["b"]);
        assert!(!view.read_with(cx, |view, _| view.history_open()));
    }

    #[gpui_kit::test]
    async fn lets_keyboard_users_browse_history_before_confirming_a_session(cx: &mut TestAppContext) {
        let (mut store, _) = connected_store(tasks());
        store.docked = Some("a".to_owned());
        let (view, _, events, cx) = open(cx, store, None);
        view.update(cx, |view, cx| view.toggle_history(cx));
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("down down");
        assert_eq!(selected(&view, cx), ["c"]);
        assert!(opened(&events).is_empty(), "browsing opens nothing");
        cx.simulate_keystrokes("enter");
        assert_eq!(opened(&events), ["c"]);
        assert!(!view.read_with(cx, |view, _| view.history_open()));
    }

    #[gpui_kit::test]
    async fn closes_the_history_with_escape_or_a_click_outside(cx: &mut TestAppContext) {
        let (mut store, _) = connected_store(tasks());
        store.docked = Some("a".to_owned());
        let (view, _, _, cx) = open(cx, store, None);
        click(cx, "agents-history", Modifiers::default());
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.history_open()));
        click(cx, "agents-history", Modifiers::default());
        let popup = bounds_of(cx, "agents-history-popup");
        cx.simulate_click(popup.bottom_right() + gpui_kit::point(px(20.0), px(20.0)), Modifiers::default());
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.history_open()));
    }

    #[gpui_kit::test]
    async fn switches_placement_from_the_agents_header(cx: &mut TestAppContext) {
        let (_, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        click(cx, "agents-use-tool-window", Modifiers::default());
        click(cx, "agents-new-task", Modifiers::default());
        assert_eq!(*events.borrow(), [TaskListEvent::SetLocation(SessionLocation::ToolWindow), TaskListEvent::NewTask]);
    }

    #[gpui_kit::test]
    async fn right_clicking_outside_the_selection_targets_only_that_task(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        click(cx, "agents-task-a", Modifiers::default());
        click(cx, "agents-task-b", shift());
        right_click(cx, "agents-task-d");
        assert_eq!(selected(&view, cx), ["d"]);
        let items = events.borrow().iter().find_map(|e| match e {
            TaskListEvent::ShowMenu { items, .. } => Some(items.clone()),
            _ => None,
        });
        let items = items.expect("a menu");
        let commands: Vec<TaskCommand> = items.iter().filter_map(|item| item.command().cloned()).collect();
        assert!(commands.contains(&TaskCommand::Open("d".to_owned())));
        assert!(commands.contains(&TaskCommand::Delete(vec!["d".to_owned()])));
    }

    #[gpui_kit::test]
    async fn the_menu_of_a_selection_acts_on_all_of_it_and_delete_is_destructive(cx: &mut TestAppContext) {
        let (_, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        click(cx, "agents-task-a", Modifiers::default());
        click(cx, "agents-task-b", shift());
        right_click(cx, "agents-task-b");
        let events = events.borrow();
        let Some(TaskListEvent::ShowMenu { items, .. }) = events.last() else { panic!("a menu: {events:?}") };
        let ids = vec!["a".to_owned(), "b".to_owned()];
        assert!(items.iter().all(|item| item.command() != Some(&TaskCommand::Open("a".to_owned()))));
        let delete = items.iter().find(|item| item.command() == Some(&TaskCommand::Delete(ids.clone())));
        assert!(matches!(delete, Some(MenuItem::Item(entry)) if entry.danger));
        assert!(items.iter().any(|item| item.command() == Some(&TaskCommand::Archive(ids.clone()))));
    }

    #[gpui_kit::test]
    async fn docks_a_session_editor_tab_dropped_on_the_tool_window_and_highlights_while_it_hovers(
        cx: &mut TestAppContext,
    ) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        let session = AgentTab::Session("b".to_owned());
        view.update(cx, |view, cx| view.tab_over(Some(&session), cx));
        assert!(view.read_with(cx, |view, _| view.is_drop_target()));
        view.update(cx, |view, cx| view.tab_dropped(Some(&session), cx));
        assert!(!view.read_with(cx, |view, _| view.is_drop_target()));
        assert_eq!(*events.borrow(), [TaskListEvent::DockTask("b".to_owned())]);
    }

    #[gpui_kit::test]
    async fn ignores_editor_tab_drags_that_are_not_agent_sessions(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, connected_store(tasks()).0, None);
        view.update(cx, |view, cx| view.tab_over(Some(&AgentTab::Other), cx));
        assert!(!view.read_with(cx, |view, _| view.is_drop_target()));
        view.update(cx, |view, cx| view.tab_dropped(Some(&AgentTab::Other), cx));
        assert!(events.borrow().is_empty());
    }

    #[gpui_kit::test]
    async fn marks_a_task_that_finished_in_the_background_with_an_unread_dot(cx: &mut TestAppContext) {
        let (store, generation) = connected_store(vec![task("a", AgentStatus::Running, 0.0)]);
        let (view, store, _, cx) = open(cx, store, None);
        assert!(cx.debug_bounds("agents-unread-a").is_none());
        store.update(cx, |store, cx| {
            let finished = AgentEvent::Task { task: task("a", AgentStatus::Ready, 0.0) };
            store.on_event(generation, finished, &I18n::default());
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, cx| view.rows(cx))[1], "Ready | a | claude · a | 10m | unread");
        assert!(cx.debug_bounds("agents-unread-a").is_some());
    }

    #[gpui_kit::test]
    async fn says_when_there_are_no_tasks_yet_and_offers_a_new_one(cx: &mut TestAppContext) {
        let (view, _, events, cx) = open(cx, connected_store(Vec::new()).0, None);
        assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("No agent tasks yet"));
        click(cx, "agents-empty-new-task", Modifiers::default());
        assert_eq!(*events.borrow(), [TaskListEvent::NewTask]);
    }

    #[gpui_kit::test]
    async fn waits_for_the_tasks_to_load(cx: &mut TestAppContext) {
        let (view, _, _, cx) = open(cx, AgentsStore::new(Saved::default()), None);
        assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("Loading…"));
        assert!(cx.debug_bounds("agents-empty-new-task").is_none());
    }

    #[test]
    fn delete_is_the_destructive_row_of_the_menu() {
        let items = menu_items(vec![
            task_actions::MenuItem::Action {
                command: TaskCommand::Archive(vec!["a".to_owned()]),
                label: "Archive".to_owned(),
                icon: None,
                disabled: true,
            },
            task_actions::MenuItem::Separator,
            task_actions::MenuItem::Action {
                command: TaskCommand::Delete(vec!["a".to_owned()]),
                label: "Delete…".to_owned(),
                icon: Some("trash"),
                disabled: false,
            },
        ]);
        assert!(items[0].is_disabled());
        assert!(matches!(items[1], MenuItem::Separator));
        assert!(matches!(&items[2], MenuItem::Item(entry) if entry.danger && entry.icon == Some("trash")));
    }
}
