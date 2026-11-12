//! The Agents tool window: the tasks grouped by what they need from the user, their selection (a click,
//! ⇧ for a range from the anchor, ⌘/Ctrl to toggle one, ⇧↑/↓ to extend), the docked conversation with the
//! task list in a history popup, the conversation placement buttons, and docking a session tab dropped on
//! the tool window.

use crate::agents::model::{GroupStatus, group_tasks};
use crate::agents::store::{AgentTab, AgentsStore, SessionLocation};
use crate::platform::i18n::I18n;
use ide_agent::AgentTask;

/// A row of the task list: a group header or a task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKey {
    Group(GroupStatus),
    Task(String),
}

#[derive(Clone, Debug)]
pub enum TaskRow<'a> {
    Group { status: GroupStatus, label: &'static str, count: usize, expanded: bool },
    Task(&'a AgentTask),
}

impl TaskRow<'_> {
    pub fn key(&self) -> RowKey {
        match self {
            Self::Group { status, .. } => RowKey::Group(*status),
            Self::Task(task) => RowKey::Task(task.id.clone()),
        }
    }
}

/// How a row was pressed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Click {
    pub shift: bool,
    /// ⌘ or Ctrl: toggles the row.
    pub toggle: bool,
    /// The secondary button: selects the row unless it is selected already (the menu acts on the selection).
    pub secondary: bool,
}

/// A key the task list handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListKey {
    Up,
    Down,
    Enter,
}

/// The rows of the tasks, the groups in attention order with their tasks under the expanded ones.
pub fn task_rows<'a>(tasks: &'a [AgentTask], collapsed: &[GroupStatus]) -> Vec<TaskRow<'a>> {
    let mut rows = Vec::new();
    for group in group_tasks(tasks) {
        let expanded = !collapsed.contains(&group.status);
        rows.push(TaskRow::Group { status: group.status, label: group.label, count: group.tasks.len(), expanded });
        if expanded {
            rows.extend(group.tasks.into_iter().map(TaskRow::Task));
        }
    }
    rows
}

fn session_task(tab: Option<&AgentTab>) -> Option<&str> {
    match tab {
        Some(AgentTab::Session(task)) => Some(task),
        _ => None,
    }
}

/// The task list's groups and selection.
#[derive(Clone, Debug)]
pub struct TaskList {
    /// Archived tasks start collapsed.
    collapsed: Vec<GroupStatus>,
    selected: Vec<RowKey>,
    /// The keyboard lead row.
    lead: Option<RowKey>,
    /// Where a range starts.
    anchor: Option<RowKey>,
}

impl TaskList {
    /// The list selects the task of the active editor tab, if it is a session.
    pub fn new(active: Option<&AgentTab>) -> Self {
        let selected: Vec<RowKey> = session_task(active).map(|t| RowKey::Task(t.to_owned())).into_iter().collect();
        Self { collapsed: vec![GroupStatus::Archived], lead: selected.first().cloned(), selected, anchor: None }
    }

    pub fn rows<'a>(&self, tasks: &'a [AgentTask]) -> Vec<TaskRow<'a>> {
        task_rows(tasks, &self.collapsed)
    }

    pub fn is_selected(&self, key: &RowKey) -> bool {
        self.selected.contains(key)
    }

    /// The selected tasks, in the order they were selected (group headers are left out).
    pub fn selected_tasks(&self) -> Vec<&str> {
        self.selected
            .iter()
            .filter_map(|k| match k {
                RowKey::Task(id) => Some(id.as_str()),
                RowKey::Group(_) => None,
            })
            .collect()
    }

    /// A group was expanded or collapsed.
    pub fn set_expanded(&mut self, status: GroupStatus, open: bool) {
        self.collapsed.retain(|s| *s != status);
        if !open {
            self.collapsed.push(status);
        }
    }

    /// The active editor tab changed: a session tab selects its task (unless it is selected already, as
    /// the echo of a click is) and expands its group.
    pub fn active_tab_changed(&mut self, tab: Option<&AgentTab>, tasks: &[AgentTask]) {
        let Some(task) = session_task(tab) else { return };
        let key = RowKey::Task(task.to_owned());
        if !self.selected.contains(&key) {
            self.selected = vec![key];
        }
        if let Some(group) = group_tasks(tasks).into_iter().find(|g| g.tasks.iter().any(|t| t.id == task)) {
            self.set_expanded(group.status, true);
        }
    }

    /// Selects `task` alone, as the lead.
    pub fn select_only(&mut self, task: Option<&str>) {
        self.selected = task.map(|t| RowKey::Task(t.to_owned())).into_iter().collect();
        self.lead = self.selected.first().cloned();
        self.anchor = None;
    }

    fn index_of(rows: &[TaskRow], key: Option<&RowKey>) -> Option<usize> {
        let key = key?;
        rows.iter().position(|r| r.key() == *key)
    }

    /// Commits a selection; returns its task when the selection is just that one task, the lead.
    fn commit(&mut self, selected: Vec<RowKey>, lead: RowKey) -> Option<String> {
        let single = match (&lead, selected.as_slice()) {
            (RowKey::Task(id), [only]) if *only == lead => Some(id.clone()),
            _ => None,
        };
        self.selected = selected;
        self.lead = Some(lead);
        single
    }

    fn select_index(&mut self, rows: &[TaskRow], i: usize, click: Click) -> Option<String> {
        let key = rows.get(i)?.key();
        if click.shift {
            let lead = Self::index_of(rows, self.lead.as_ref());
            let from = Self::index_of(rows, self.anchor.as_ref()).or(lead).unwrap_or(i);
            self.anchor = Some(rows[from].key());
            let range = rows[from.min(i)..=from.max(i)].iter().map(TaskRow::key).collect();
            self.commit(range, key)
        } else if click.toggle {
            let mut next = self.selected.clone();
            if let Some(at) = next.iter().position(|k| *k == key) {
                next.remove(at);
            } else {
                next.push(key.clone());
            }
            self.anchor = Some(key.clone());
            self.commit(next, key)
        } else {
            self.anchor = Some(key.clone());
            self.commit(vec![key.clone()], key)
        }
    }

    /// A row was pressed; returns the task selected alone, as for [`Self::key_down`].
    pub fn mouse_down(&mut self, tasks: &[AgentTask], key: &RowKey, click: Click) -> Option<String> {
        let rows = self.rows(tasks);
        let i = rows.iter().position(|r| r.key() == *key)?;
        if click.secondary {
            return if self.selected.contains(key) { None } else { self.select_index(&rows, i, Click::default()) };
        }
        self.select_index(&rows, i, click)
    }

    /// ↑/↓ move the lead (⇧ extends the selection from the anchor); returns the task selected alone. Enter
    /// is answered by [`Self::lead_task`].
    pub fn key_down(&mut self, tasks: &[AgentTask], key: ListKey, shift: bool) -> Option<String> {
        let rows = self.rows(tasks);
        if rows.is_empty() {
            return None;
        }
        let lead = Self::index_of(&rows, self.lead.as_ref());
        let next = match (key, lead) {
            (ListKey::Enter, _) => return None,
            (_, None) => 0,
            (ListKey::Down, Some(i)) => (i + 1).min(rows.len() - 1),
            (ListKey::Up, Some(i)) => i.saturating_sub(1),
        };
        let click = Click { shift: shift && lead.is_some(), ..Click::default() };
        self.select_index(&rows, next, click)
    }

    /// The task of the lead row, which Enter opens.
    pub fn lead_task(&self) -> Option<&str> {
        match &self.lead {
            Some(RowKey::Task(id)) => Some(id),
            _ => None,
        }
    }

    /// The tasks a context menu on `key` acts on: the selection when the row is in it, else the row alone.
    pub fn context_selection(&self, key: &RowKey) -> Vec<String> {
        if self.selected.contains(key) {
            self.selected_tasks().into_iter().map(str::to_owned).collect()
        } else {
            match key {
                RowKey::Task(id) => vec![id.clone()],
                RowKey::Group(_) => Vec::new(),
            }
        }
    }
}

/// What the tool window asks the store for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PanelAction {
    /// Open the task where conversations are shown (`AgentsStore::open_task`).
    OpenTask(String),
    /// Dock the dropped session (`AgentsStore::dock_task`).
    DockTask(String),
}

/// The Agents tool window.
#[derive(Clone, Debug)]
pub struct AgentsPanel {
    pub list: TaskList,
    history_open: bool,
    tab_over: bool,
}

impl AgentsPanel {
    pub fn new(active: Option<&AgentTab>) -> Self {
        Self { list: TaskList::new(active), history_open: false, tab_over: false }
    }

    /// A conversation (or the New Task page) fills the tool window; the tasks are in the history popup.
    pub fn inline(store: &AgentsStore) -> bool {
        store.docked.is_some() || store.home_open
    }

    /// Whether the task list is shown: in the tool window, or in the open history popup.
    pub fn shows_task_list(&self, store: &AgentsStore) -> bool {
        !Self::inline(store) || self.history_open
    }

    pub fn history_open(&self) -> bool {
        self.history_open
    }

    /// The history button: opens (or closes) the popup with the docked task selected.
    pub fn toggle_history(&mut self, store: &AgentsStore) {
        self.list.select_only(store.docked.as_deref());
        self.history_open = !self.history_open;
    }

    /// Escape or a click outside the popup.
    pub fn close_history(&mut self) {
        self.history_open = false;
    }

    fn show_task(&mut self, task: &str) -> PanelAction {
        self.history_open = false;
        PanelAction::OpenTask(task.to_owned())
    }

    /// A row was pressed. In the tool window, selecting one task alone opens it.
    pub fn mouse_down(&mut self, store: &AgentsStore, key: &RowKey, click: Click) -> Option<PanelAction> {
        let single = self.list.mouse_down(store.tasks(), key, click)?;
        (!Self::inline(store)).then(|| self.show_task(&single))
    }

    /// A task row was clicked (after [`Self::mouse_down`]): in the history popup a plain click opens it.
    pub fn clicked(&mut self, store: &AgentsStore, task: &str, click: Click) -> Option<PanelAction> {
        (Self::inline(store) && !click.shift && !click.toggle).then(|| self.show_task(task))
    }

    /// A key in the task list: browsing the history popup selects without opening; Enter opens the lead.
    pub fn key_down(&mut self, store: &AgentsStore, key: ListKey, shift: bool) -> Option<PanelAction> {
        if key == ListKey::Enter {
            let task = self.list.lead_task()?.to_owned();
            return Some(self.show_task(&task));
        }
        let single = self.list.key_down(store.tasks(), key, shift)?;
        (!Self::inline(store)).then(|| self.show_task(&single))
    }

    /// An editor tab is dragged over the tool window (`tab` as the view resolved it): only a session tab
    /// can be dropped, and highlights the tool window.
    pub fn tab_over(&mut self, tab: Option<&AgentTab>) {
        self.tab_over = session_task(tab).is_some();
    }

    pub fn tab_left(&mut self) {
        self.tab_over = false;
    }

    pub fn is_drop_target(&self) -> bool {
        self.tab_over
    }

    /// A dragged editor tab was dropped: a session docks.
    pub fn tab_dropped(&mut self, tab: Option<&AgentTab>) -> Option<PanelAction> {
        self.tab_over = false;
        session_task(tab).map(|task| PanelAction::DockTask(task.to_owned()))
    }
}

/// A conversation placement button of the Agents header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationButton {
    pub location: SessionLocation,
    pub label: String,
    pub pressed: bool,
}

/// "Chat in Tool Window" and "Chat in Editor"; a click calls `AgentsStore::set_session_location`.
pub fn location_buttons(store: &AgentsStore, i18n: &I18n) -> [LocationButton; 2] {
    let button = |location, label| LocationButton {
        location,
        label: i18n.tx(label),
        pressed: store.session_location == location,
    };
    [button(SessionLocation::ToolWindow, "Chat in Tool Window"), button(SessionLocation::Editor, "Chat in Editor")]
}

#[cfg(test)]
mod tests;
