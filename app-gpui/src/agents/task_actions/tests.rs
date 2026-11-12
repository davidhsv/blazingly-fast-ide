use super::*;
use crate::agents::fixtures::{connected_store, local_task, task};
use crate::agents::task_list::{Click, RowKey, TaskList};

fn with_status(id: &str, status: AgentStatus) -> AgentTask {
    let mut t = local_task(id);
    t.status = status;
    t
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn key(id: &str) -> RowKey {
    RowKey::Task(id.to_owned())
}

const SHIFT: Click = Click { shift: true, toggle: false, secondary: false };
const RIGHT: Click = Click { shift: false, toggle: false, secondary: true };

/// Runs a batch from its first effect, answering each call with `answer`: the calls made and what ends it.
fn drive(
    batches: &mut TaskBatches,
    store: &mut AgentsStore,
    first: Option<BatchEffect>,
    answer: impl Fn(&AgentCall) -> Result<(), String>,
) -> (Vec<AgentCall>, Option<BatchEffect>) {
    let mut calls = Vec::new();
    let mut next = first;
    while let Some(BatchEffect::Call { ticket, call }) = next {
        let result = answer(&call);
        calls.push(call);
        next = batches.answered(store, ticket, result);
    }
    (calls, next)
}

fn deleted(task: &str, remove_worktree: bool) -> AgentCall {
    AgentCall::Delete { task: task.to_owned(), remove_worktree }
}

/// The menu item labelled `label`, and what it runs.
fn item<'a>(menu: &'a [MenuItem], label: &str) -> &'a TaskCommand {
    menu.iter()
        .find_map(|i| match i {
            MenuItem::Action { command, label: l, .. } if l == label => Some(command),
            _ => None,
        })
        .expect("the menu item")
}

fn menu_of(store: &AgentsStore, list: &TaskList, row: &str, i18n: &mut I18n) -> Vec<MenuItem> {
    let selection = list.context_selection(&key(row));
    let tasks: Vec<&AgentTask> = selection.iter().filter_map(|id| store.task(id)).collect();
    task_menu(&tasks, i18n)
}

fn confirm_ticket(effect: Option<BatchEffect>) -> (Ticket, ConfirmChecked) {
    match effect {
        Some(BatchEffect::Confirm { ticket, request }) => (ticket, request),
        other => panic!("a confirmation, not {other:?}"),
    }
}

#[test]
fn deletes_the_right_clicked_selection_after_one_confirmation_excluding_group_headers() {
    let mut i18n = I18n::default();
    let (mut store, _) =
        connected_store(vec![with_status("one", AgentStatus::Running), local_task("two"), local_task("three")]);
    let mut list = TaskList::new(None);
    list.mouse_down(store.tasks(), &key("one"), Click::default());
    list.mouse_down(store.tasks(), &key("three"), SHIFT);
    list.mouse_down(store.tasks(), &key("two"), RIGHT);
    let menu = menu_of(&store, &list, "two", &mut i18n);
    let TaskCommand::Delete(selection) = item(&menu, "Delete (3)…").clone() else { panic!("a delete") };
    let mut batches = TaskBatches::default();
    let (ticket, request) = confirm_ticket(batches.delete(&mut store, &selection, &mut i18n));
    assert!(request.message.contains("3 agent tasks"));
    let first = batches.confirmed(&mut store, ticket, Some(false), &i18n);
    let (calls, end) = drive(&mut batches, &mut store, first, |_| Ok(()));
    assert_eq!(calls, [deleted("one", false), deleted("two", false), deleted("three", false)]);
    assert_eq!(end, None);
}

#[test]
fn right_clicking_outside_the_selection_targets_only_that_task() {
    let mut i18n = I18n::default();
    let (mut store, _) = connected_store(["one", "two", "three", "four"].into_iter().map(local_task).collect());
    let mut list = TaskList::new(None);
    list.mouse_down(store.tasks(), &key("one"), Click::default());
    list.mouse_down(store.tasks(), &key("two"), SHIFT);
    list.mouse_down(store.tasks(), &key("four"), RIGHT);
    let menu = menu_of(&store, &list, "four", &mut i18n);
    let TaskCommand::Delete(selection) = item(&menu, "Delete…").clone() else { panic!("a delete") };
    let mut batches = TaskBatches::default();
    let (ticket, _) = confirm_ticket(batches.delete(&mut store, &selection, &mut i18n));
    let first = batches.confirmed(&mut store, ticket, Some(false), &i18n);
    assert_eq!(drive(&mut batches, &mut store, first, |_| Ok(())).0, [deleted("four", false)]);
}

#[test]
fn archives_only_eligible_tasks_in_a_mixed_selection() {
    let mut i18n = I18n::default();
    let (mut store, _) = connected_store(vec![
        with_status("one", AgentStatus::Running),
        local_task("two"),
        with_status("three", AgentStatus::Archived),
    ]);
    let mut list = TaskList::new(None);
    list.set_expanded(crate::agents::model::GroupStatus::Archived, true);
    list.mouse_down(store.tasks(), &key("one"), Click::default());
    list.mouse_down(store.tasks(), &key("three"), SHIFT);
    let menu = menu_of(&store, &list, "two", &mut i18n);
    let TaskCommand::Archive(selection) = item(&menu, "Archive (1)").clone() else { panic!("an archive") };
    let mut batches = TaskBatches::default();
    let first = batches.archive(&mut store, &selection, &i18n);
    assert_eq!(drive(&mut batches, &mut store, first, |_| Ok(())).0, [AgentCall::Archive { task: "two".to_owned() }]);
}

#[test]
fn unarchives_only_archived_tasks_in_a_mixed_selection() {
    let mut i18n = I18n::default();
    let (mut store, _) = connected_store(vec![
        local_task("one"),
        with_status("two", AgentStatus::Archived),
        with_status("three", AgentStatus::Archived),
    ]);
    let mut list = TaskList::new(None);
    list.set_expanded(crate::agents::model::GroupStatus::Archived, true);
    list.mouse_down(store.tasks(), &key("one"), Click::default());
    list.mouse_down(store.tasks(), &key("three"), SHIFT);
    let menu = menu_of(&store, &list, "two", &mut i18n);
    let TaskCommand::Unarchive(selection) = item(&menu, "Unarchive (2)").clone() else { panic!("an unarchive") };
    let mut batches = TaskBatches::default();
    let first = batches.unarchive(&mut store, &selection, &i18n);
    let unarchived = |id: &str| AgentCall::Unarchive { task: id.to_owned() };
    assert_eq!(drive(&mut batches, &mut store, first, |_| Ok(())).0, [unarchived("two"), unarchived("three")]);
}

#[test]
fn cancels_the_whole_batch() {
    let mut i18n = I18n::default();
    let (mut store, _) = connected_store(vec![local_task("one"), local_task("two")]);
    let mut batches = TaskBatches::default();
    let (ticket, _) = confirm_ticket(batches.delete(&mut store, &ids(&["one", "two"]), &mut i18n));
    assert_eq!(batches.confirmed(&mut store, ticket, None, &i18n), None);
}

#[test]
fn applies_worktree_cleanup_only_to_worktree_tasks() {
    for checked in [false, true] {
        let mut i18n = I18n::default();
        let (mut store, _) = connected_store(vec![local_task("one"), task("two", AgentStatus::Ready, 0.0)]);
        let mut batches = TaskBatches::default();
        let (ticket, request) = confirm_ticket(batches.delete(&mut store, &ids(&["one", "two"]), &mut i18n));
        assert_eq!(request.checkbox.as_deref(), Some("Also remove their worktree folders"));
        assert!(request.checked);
        let first = batches.confirmed(&mut store, ticket, Some(checked), &i18n);
        let calls = drive(&mut batches, &mut store, first, |_| Ok(())).0;
        assert_eq!(calls, [deleted("one", false), deleted("two", checked)]);
    }
}

#[test]
fn continues_after_an_error_and_reports_the_failed_task() {
    let mut i18n = I18n::default();
    let (mut store, _) = connected_store(vec![local_task("one"), local_task("two")]);
    let mut batches = TaskBatches::default();
    let (ticket, _) = confirm_ticket(batches.delete(&mut store, &ids(&["one", "two"]), &mut i18n));
    let first = batches.confirmed(&mut store, ticket, Some(false), &i18n);
    let fail_first = |call: &AgentCall| match call {
        AgentCall::Delete { task, .. } if task == "one" => Err("Cannot remove worktree".to_owned()),
        _ => Ok(()),
    };
    let (calls, end) = drive(&mut batches, &mut store, first, fail_first);
    assert_eq!(calls.len(), 2);
    let error = BatchEffect::ShowError {
        title: "Cannot delete agent tasks".to_owned(),
        message: "one: Cannot remove worktree".to_owned(),
    };
    assert_eq!(end, Some(error));
}

#[test]
fn keeps_placement_out_of_session_menus() {
    let mut i18n = I18n::default();
    for docked in [None, Some("one")] {
        let (mut store, _) = connected_store(vec![local_task("one")]);
        store.docked = docked.map(str::to_owned);
        let one = store.task("one").expect("the task");
        let menu = task_menu(&[one], &mut i18n);
        let commands: Vec<&TaskCommand> = menu
            .iter()
            .filter_map(|i| match i {
                MenuItem::Action { command, .. } => Some(command),
                MenuItem::Separator => None,
            })
            .collect();
        let cwd = "/project".to_owned();
        let id = || "one".to_owned();
        assert_eq!(
            commands,
            [
                &TaskCommand::Open(id()),
                &TaskCommand::Rename(id()),
                &TaskCommand::OpenInTerminal(cwd),
                &TaskCommand::Archive(vec![id()]),
                &TaskCommand::Delete(vec![id()]),
            ]
        );
    }
}
