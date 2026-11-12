//! What can be done to tasks: the context menu of a task (or of the selected tasks), and the batches it
//! starts. A batch runs its backend calls one after another, keeps going past a failure and reports the
//! failed tasks at the end; deleting asks once for the whole batch, with a checkbox to remove the worktree
//! folders of the worktree tasks among them.
//!
//! Each step is a [`BatchEffect`] with a ticket from the store; its answer comes back to
//! [`TaskBatches::confirmed`] or [`TaskBatches::answered`].

use crate::agents::model::is_busy;
use crate::agents::store::{AgentCall, AgentsStore, Ticket};
use crate::platform::i18n::{I18n, Param};
use ide_agent::{AgentStatus, AgentTask, Isolation};
use std::collections::VecDeque;

/// Idle and not archived yet.
pub fn can_archive(task: &AgentTask) -> bool {
    !is_busy(task.status) && task.status != AgentStatus::Archived
}

pub fn can_fork(task: &AgentTask) -> bool {
    can_archive(task) && task.capabilities.fork
}

/// What a menu item runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TaskCommand {
    /// Opens the task where conversations are shown (`AgentsStore::open_task`).
    Open(String),
    Rename(String),
    /// Opens a terminal in the task's directory.
    OpenInTerminal(String),
    Fork(String),
    Unarchive(Vec<String>),
    Archive(Vec<String>),
    Delete(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Action { command: TaskCommand, label: String, icon: Option<&'static str>, disabled: bool },
    Separator,
}

fn action(command: TaskCommand, label: String, icon: Option<&'static str>, disabled: bool) -> MenuItem {
    MenuItem::Action { command, label, icon, disabled }
}

fn counted(i18n: &mut I18n, before: &str, after: &str, n: usize) -> String {
    i18n.tx_built(&[before, after], &[Param::Number(n as f64)])
}

/// The menu of `tasks` (the right-clicked task, or the selection it belongs to). Where a conversation is
/// shown is the Agents-wide placement, not a choice of this menu.
pub fn task_menu(tasks: &[&AgentTask], i18n: &mut I18n) -> Vec<MenuItem> {
    let Some(task) = tasks.first() else { return Vec::new() };
    let ids: Vec<String> = tasks.iter().map(|t| t.id.clone()).collect();
    let multiple = tasks.len() > 1;
    let archived = tasks.iter().filter(|t| t.status == AgentStatus::Archived).count();
    let archivable = tasks.iter().filter(|t| can_archive(t)).count();
    let mut out = Vec::new();
    if !multiple {
        out.push(action(TaskCommand::Open(task.id.clone()), i18n.tx("Open"), None, false));
        out.push(action(TaskCommand::Rename(task.id.clone()), i18n.tx("Rename…"), Some("edit"), false));
        let cwd = task.cwd.0.to_string_lossy().into_owned();
        out.push(action(TaskCommand::OpenInTerminal(cwd), i18n.tx("Open in Terminal"), Some("terminal"), false));
        if task.capabilities.fork && task.status != AgentStatus::Archived {
            let label = i18n.tx("Copy to a New Task");
            out.push(action(TaskCommand::Fork(task.id.clone()), label, Some("branch"), !can_fork(task)));
        }
        out.push(MenuItem::Separator);
    }
    if archived > 0 {
        let label = if multiple { counted(i18n, "Unarchive (", ")", archived) } else { i18n.tx("Unarchive") };
        out.push(action(TaskCommand::Unarchive(ids.clone()), label, None, false));
    }
    let label = if multiple { counted(i18n, "Archive (", ")", archivable) } else { i18n.tx("Archive") };
    out.push(action(TaskCommand::Archive(ids.clone()), label, None, archivable == 0));
    let label = if multiple { counted(i18n, "Delete (", ")…", tasks.len()) } else { i18n.tx("Delete…") };
    out.push(action(TaskCommand::Delete(ids), label, None, false));
    out
}

/// A dialog asking to go ahead, with a checkbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmChecked {
    pub title: String,
    pub message: String,
    pub ok: String,
    pub checkbox: Option<String>,
    pub checked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BatchEffect {
    /// Ask, then call [`TaskBatches::confirmed`] with the checkbox's state (`None`: declined).
    Confirm {
        ticket: Ticket,
        request: ConfirmChecked,
    },
    /// Run the call, then call [`TaskBatches::answered`].
    Call {
        ticket: Ticket,
        call: AgentCall,
    },
    ShowError {
        title: String,
        message: String,
    },
}

#[derive(Clone, Debug)]
struct Run {
    step: Ticket,
    error_title: String,
    /// The calls left, with the title of their task.
    calls: VecDeque<(String, AgentCall)>,
    current: String,
    errors: Vec<String>,
}

/// The tasks of `ids` that still exist, each once, in the given order.
fn resolve<'a>(store: &'a AgentsStore, ids: &[String]) -> Vec<&'a AgentTask> {
    let mut out: Vec<&AgentTask> = Vec::new();
    for id in ids {
        if let Some(task) = store.task(id)
            && !out.iter().any(|t| t.id == task.id)
        {
            out.push(task);
        }
    }
    out
}

/// A task a delete asks about: its id, its title and whether it is a worktree task.
type DeleteItem = (String, String, bool);

/// The archive, unarchive and delete batches on their way.
#[derive(Debug, Default)]
pub struct TaskBatches {
    /// Deletes waiting for their confirmation, by its ticket: the tasks and whether each is a worktree.
    confirming: Vec<(Ticket, Vec<DeleteItem>)>,
    runs: Vec<Run>,
}

impl TaskBatches {
    fn start(
        &mut self,
        store: &mut AgentsStore,
        calls: Vec<(String, AgentCall)>,
        error_title: String,
    ) -> Option<BatchEffect> {
        let mut calls: VecDeque<(String, AgentCall)> = calls.into();
        let (title, call) = calls.pop_front()?;
        let step = store.new_ticket();
        self.runs.push(Run { step, error_title, calls, current: title, errors: Vec::new() });
        Some(BatchEffect::Call { ticket: step, call })
    }

    /// Archives the idle tasks of `ids` that aren't archived yet.
    pub fn archive(&mut self, store: &mut AgentsStore, ids: &[String], i18n: &I18n) -> Option<BatchEffect> {
        let calls = resolve(store, ids)
            .into_iter()
            .filter(|t| can_archive(t))
            .map(|t| (t.title.clone(), AgentCall::Archive { task: t.id.clone() }))
            .collect();
        self.start(store, calls, i18n.tx("Cannot archive agent tasks"))
    }

    /// Unarchives the archived tasks of `ids`.
    pub fn unarchive(&mut self, store: &mut AgentsStore, ids: &[String], i18n: &I18n) -> Option<BatchEffect> {
        let calls = resolve(store, ids)
            .into_iter()
            .filter(|t| t.status == AgentStatus::Archived)
            .map(|t| (t.title.clone(), AgentCall::Unarchive { task: t.id.clone() }))
            .collect();
        self.start(store, calls, i18n.tx("Cannot unarchive agent tasks"))
    }

    /// Asks once to delete the tasks of `ids`.
    pub fn delete(&mut self, store: &mut AgentsStore, ids: &[String], i18n: &mut I18n) -> Option<BatchEffect> {
        let tasks = resolve(store, ids);
        let first = tasks.first()?;
        let multiple = tasks.len() > 1;
        let worktree = tasks.iter().any(|t| t.isolation == Isolation::Worktree);
        let title = Param::from(first.title.as_str());
        let message = if multiple {
            let kept = " agent tasks? Their branches and files outside worktrees are kept.";
            i18n.tx_built(&["Delete ", kept], &[Param::Number(tasks.len() as f64)])
        } else if worktree {
            let branch = Param::from(first.branch.as_deref().unwrap_or_default());
            i18n.tx_built(&["Delete “", "”? Its branch ", " is kept."], &[title, branch])
        } else {
            i18n.tx_built(&["Delete “", "”? Files it changed are kept."], &[title])
        };
        let checkbox = worktree.then(|| {
            i18n.tx(if multiple { "Also remove their worktree folders" } else { "Also remove its worktree folder" })
        });
        let request = ConfirmChecked {
            title: i18n.tx(if multiple { "Delete Agent Tasks" } else { "Delete Agent Task" }),
            message,
            ok: i18n.tx("Delete"),
            checkbox,
            checked: worktree,
        };
        let items = tasks.iter().map(|t| (t.id.clone(), t.title.clone(), t.isolation == Isolation::Worktree)).collect();
        let ticket = store.new_ticket();
        self.confirming.push((ticket, items));
        Some(BatchEffect::Confirm { ticket, request })
    }

    /// The answer of a delete's dialog: `Some(checked)` deletes every task (removing the worktree folder of
    /// a worktree task when checked), `None` cancels the whole batch.
    pub fn confirmed(
        &mut self,
        store: &mut AgentsStore,
        ticket: Ticket,
        checked: Option<bool>,
        i18n: &I18n,
    ) -> Option<BatchEffect> {
        let at = self.confirming.iter().position(|(t, _)| *t == ticket)?;
        let (_, items) = self.confirming.remove(at);
        let checked = checked?;
        let calls = items
            .into_iter()
            .map(|(task, title, worktree)| (title, AgentCall::Delete { task, remove_worktree: worktree && checked }))
            .collect();
        self.start(store, calls, i18n.tx("Cannot delete agent tasks"))
    }

    /// The answer of a batch's call: the next call, or the failures once all ran.
    pub fn answered(
        &mut self,
        store: &mut AgentsStore,
        ticket: Ticket,
        result: Result<(), String>,
    ) -> Option<BatchEffect> {
        let at = self.runs.iter().position(|r| r.step == ticket)?;
        let run = &mut self.runs[at];
        if let Err(error) = result {
            run.errors.push(format!("{}: {error}", run.current));
        }
        if let Some((title, call)) = run.calls.pop_front() {
            run.current = title;
            run.step = store.new_ticket();
            return Some(BatchEffect::Call { ticket: run.step, call });
        }
        let run = self.runs.remove(at);
        (!run.errors.is_empty())
            .then(|| BatchEffect::ShowError { title: run.error_title, message: run.errors.join("\n") })
    }
}

#[cfg(test)]
mod tests;
