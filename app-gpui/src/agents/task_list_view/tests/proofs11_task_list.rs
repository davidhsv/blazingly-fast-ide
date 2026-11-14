use super::*;
use crate::agents::fixtures::{agent, user};
use crate::agents::session_view::SessionView;
use crate::agents::store::{AgentEffect, Editors};
use gpui_kit::{Context, IntoElement, Render, Window, div};
use ide_agent::AgentEntry;

/// What the tool window shows in place of the task list while a task is docked.
struct Conversation;

impl Render for Conversation {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().debug_selector(|| "docked-conversation".to_owned()).size_full()
    }
}

/// Hands the tool window a docked conversation to show.
fn dock_conversation(view: &Entity<TaskListView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        let conversation = cx.new(|_| Conversation);
        view.set_docked_view(Some(conversation.into()), cx);
    });
    cx.run_until_parked();
}

fn focus(view: &Entity<TaskListView>, cx: &mut VisualTestContext) {
    cx.update(|window, cx| {
        let focus = view.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
}

/// The rows of the last menu the view asked for: each label with its command.
fn last_menu(events: &Events) -> Vec<(String, Option<TaskCommand>, bool)> {
    let events = events.borrow();
    let items = events.iter().rev().find_map(|event| match event {
        TaskListEvent::ShowMenu { items, .. } => Some(items.clone()),
        _ => None,
    });
    let items = items.unwrap_or_else(|| panic!("a menu: {events:?}"));
    items
        .iter()
        .filter_map(|item| item.label().map(|label| (label.to_owned(), item.command().cloned(), item.is_disabled())))
        .collect()
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn reply(store: &Entity<AgentsStore>, generation: u64, task: &str, entry: AgentEntry, cx: &mut VisualTestContext) {
    store.update(cx, |store, cx| {
        let event = AgentEvent::Entry { task: task.to_owned(), subagent: None, index: 0, entry };
        store.on_event(generation, event, &I18n::default());
        cx.notify();
    });
    cx.run_until_parked();
}

fn emit(store: &Entity<AgentsStore>, generation: u64, event: AgentEvent, cx: &mut VisualTestContext) {
    store.update(cx, |store, cx| {
        store.on_event(generation, event, &I18n::default());
        cx.notify();
    });
    cx.run_until_parked();
}

/// The tasks whose rows carry the unread mark, top to bottom, and whose dot is drawn.
fn unread(view: &Entity<TaskListView>, cx: &mut VisualTestContext) -> Vec<String> {
    let rows = view.read_with(cx, |view, cx| view.rows(cx));
    let marked: Vec<String> = rows
        .iter()
        .filter(|row| row.ends_with(" | unread"))
        .filter_map(|row| row.split(" | ").nth(1).map(str::to_owned))
        .collect();
    cx.run_until_parked();
    for task in &marked {
        let selector: &'static str = Box::leak(format!("agents-unread-{task}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "the dot of {task} is drawn");
    }
    marked
}

fn session_of(store: &Entity<AgentsStore>, task: &str, cx: &mut VisualTestContext) -> Entity<SessionView> {
    let store = store.clone();
    let task = task.to_owned();
    cx.update(|_, cx| cx.new(|cx| SessionView::new(store, &task, false, cx)))
}

fn set_shown(session: &Entity<SessionView>, shown: bool, cx: &mut VisualTestContext) {
    session.update(cx, |session, cx| session.set_shown(shown, cx));
    cx.run_until_parked();
}

fn generation_of(effects: &[AgentEffect]) -> u64 {
    effects
        .iter()
        .find_map(|effect| match effect {
            AgentEffect::Subscribe { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("a subscription")
}

// ----- the docked conversation -----

/// The history popup lists the docked task with the others; Escape closes it and the conversation it covered
/// is still drawn.
#[gpui_kit::test]
async fn includes_the_current_session_in_history_and_keeps_the_conversation_mounted(cx: &mut TestAppContext) {
    let (mut store, _) = connected_store(tasks());
    store.docked = Some("a".to_owned());
    let (view, _, _, cx) = open(cx, store, None);
    dock_conversation(&view, cx);
    assert!(cx.debug_bounds("docked-conversation").is_some());
    click(cx, "agents-history", Modifiers::default());
    assert!(cx.debug_bounds("agents-task-a").is_some(), "the docked task is in the history");
    assert!(cx.debug_bounds("agents-task-b").is_some());
    focus(&view, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |view, _| view.history_open()));
    assert!(cx.debug_bounds("agents-task-a").is_none());
    assert!(cx.debug_bounds("docked-conversation").is_some(), "the conversation stays");
}

/// With its only task docked, the tool window shows the conversation and no task list.
#[gpui_kit::test]
async fn gives_the_whole_tool_window_to_the_conversation_when_the_docked_task_is_the_only_one(cx: &mut TestAppContext) {
    let (mut store, _) = connected_store(vec![task("a", AgentStatus::Ready, 0.0)]);
    store.docked = Some("a".to_owned());
    let (view, _, _, cx) = open(cx, store, None);
    dock_conversation(&view, cx);
    assert!(!view.read_with(cx, |view, cx| view.shows_task_list(cx)));
    assert!(cx.debug_bounds("agents-task-a").is_none() && cx.debug_bounds("agents-tasks").is_none());
    assert!(cx.debug_bounds("docked-conversation").is_some());
}

/// Without a docked conversation, a click on a task opens it where conversations show, and docks nothing.
#[gpui_kit::test]
async fn opens_tabs_as_usual_without_a_docked_conversation(cx: &mut TestAppContext) {
    let (_, _, events, cx) = open(cx, connected_store(tasks()).0, None);
    assert!(cx.debug_bounds("agents-history").is_none(), "no conversation bar");
    click(cx, "agents-task-b", Modifiers::default());
    assert_eq!(*events.borrow(), [TaskListEvent::OpenTask("b".to_owned())]);
}

/// A task's menu offers Open and none of the placement choices, docked or not.
#[gpui_kit::test]
async fn keeps_placement_out_of_session_menus(cx: &mut TestAppContext) {
    for docked in [None, Some("a")] {
        let (mut store, _) = connected_store(tasks());
        store.docked = docked.map(str::to_owned);
        let (_, _, events, cx) = open(cx, store, None);
        if docked.is_some() {
            click(cx, "agents-history", Modifiers::default());
        }
        right_click(cx, "agents-task-a");
        let menu = last_menu(&events);
        assert!(menu.iter().any(|(_, command, _)| *command == Some(TaskCommand::Open("a".to_owned()))), "{menu:?}");
        for placement in ["Dock", "Undock", "Open in Editor", "Chat in Tool Window", "Chat in Editor"] {
            assert!(!menu.iter().any(|(label, _, _)| label.contains(placement)), "{docked:?}: {menu:?}");
        }
    }
}

// ----- the menu of a selection -----

/// A range selected across a group header counts its tasks only: the right-clicked row's menu deletes the three.
#[gpui_kit::test]
async fn offers_to_delete_the_right_clicked_selection_counting_its_tasks_and_not_its_group_headers(
    cx: &mut TestAppContext,
) {
    let mut one = task("one", AgentStatus::Running, 3.0);
    one.isolation = Isolation::Local;
    let mut two = task("two", AgentStatus::Ready, 2.0);
    two.isolation = Isolation::Local;
    let mut three = task("three", AgentStatus::Ready, 1.0);
    three.isolation = Isolation::Local;
    let (view, _, events, cx) = open(cx, connected_store(vec![one, two, three]).0, None);
    click(cx, "agents-task-one", Modifiers::default());
    click(cx, "agents-task-three", shift());
    assert_eq!(selected(&view, cx), ["one", "two", "three"]);
    right_click(cx, "agents-task-two");
    let menu = last_menu(&events);
    let delete = menu.iter().find(|(label, _, _)| label == "Delete (3)…");
    assert_eq!(
        delete.map(|(_, command, disabled)| (command.clone(), *disabled)),
        Some((Some(TaskCommand::Delete(ids(&["one", "two", "three"]))), false)),
        "{menu:?}"
    );
}

/// In a selection of a running, an idle and an archived task, Archive counts the idle task alone.
#[gpui_kit::test]
async fn offers_to_archive_only_the_eligible_tasks_of_a_mixed_selection(cx: &mut TestAppContext) {
    let tasks = vec![
        task("one", AgentStatus::Running, 3.0),
        task("two", AgentStatus::Ready, 2.0),
        task("three", AgentStatus::Archived, 1.0),
    ];
    let (view, _, events, cx) = open(cx, connected_store(tasks).0, None);
    click(cx, "agents-group-archived", Modifiers::default());
    click(cx, "agents-task-one", Modifiers::default());
    click(cx, "agents-task-three", shift());
    assert_eq!(selected(&view, cx), ["one", "two", "three"]);
    right_click(cx, "agents-task-two");
    let menu = last_menu(&events);
    let archive = menu.iter().find(|(label, _, _)| label == "Archive (1)");
    assert_eq!(archive.map(|(_, _, disabled)| *disabled), Some(false), "{menu:?}");
    assert!(menu.iter().any(|(label, _, _)| label == "Unarchive (1)"), "{menu:?}");
}

/// In a selection of an idle and two archived tasks, Unarchive counts the archived tasks.
#[gpui_kit::test]
async fn offers_to_unarchive_only_the_archived_tasks_of_a_mixed_selection(cx: &mut TestAppContext) {
    let tasks = vec![
        task("one", AgentStatus::Ready, 3.0),
        task("two", AgentStatus::Archived, 2.0),
        task("three", AgentStatus::Archived, 1.0),
    ];
    let (view, _, events, cx) = open(cx, connected_store(tasks).0, None);
    click(cx, "agents-group-archived", Modifiers::default());
    click(cx, "agents-task-one", Modifiers::default());
    click(cx, "agents-task-three", shift());
    assert_eq!(selected(&view, cx), ["one", "two", "three"]);
    right_click(cx, "agents-task-two");
    let menu = last_menu(&events);
    let unarchive = menu.iter().find(|(label, _, _)| label == "Unarchive (2)");
    assert_eq!(
        unarchive.map(|(_, command, _)| command.clone()),
        Some(Some(TaskCommand::Unarchive(ids(&["one", "two", "three"])))),
        "{menu:?}"
    );
    assert!(menu.iter().any(|(label, _, _)| label == "Archive (1)"), "{menu:?}");
}

// ----- unread tasks -----

/// Tasks loaded with the project are not unread; a reply to a task whose conversation was never loaded marks it.
#[gpui_kit::test]
async fn does_not_mark_historical_tasks_unread_but_tracks_replies_even_without_loaded_entries(cx: &mut TestAppContext) {
    let (store, generation) =
        connected_store(vec![task("one", AgentStatus::Running, 1.0), task("two", AgentStatus::Ready, 0.0)]);
    let (view, store, _, cx) = open(cx, store, None);
    assert!(unread(&view, cx).is_empty());
    reply(&store, generation, "one", agent("New reply"), cx);
    reply(&store, generation, "one", agent("New reply, streamed further"), cx);
    reply(&store, generation, "two", agent("New reply"), cx);
    assert_eq!(unread(&view, cx), ["one", "two"]);
    assert!(store.read_with(cx, |store, _| store.entries("one", None).is_none_or(<[AgentEntry]>::is_empty)));
}

/// A running task that turns ready, needs input, fails or hits a limit in the background shows the unread mark.
#[gpui_kit::test]
async fn marks_background_status_transitions_unread(cx: &mut TestAppContext) {
    for status in [AgentStatus::Ready, AgentStatus::NeedsInput, AgentStatus::Failed, AgentStatus::Limited] {
        let (store, generation) = connected_store(vec![task("one", AgentStatus::Running, 0.0)]);
        let (view, store, _, cx) = open(cx, store, None);
        assert!(unread(&view, cx).is_empty());
        emit(&store, generation, AgentEvent::Task { task: task("one", status, 0.0) }, cx);
        assert_eq!(unread(&view, cx), ["one"], "{status:?}");
    }
}

/// The user's own messages, reasoning, an empty reply, a subagent's streaming and a rename leave no unread mark.
#[gpui_kit::test]
async fn ignores_user_messages_thoughts_empty_replies_and_subagent_streaming(cx: &mut TestAppContext) {
    let (store, generation) = connected_store(vec![task("one", AgentStatus::Running, 0.0)]);
    let (view, store, _, cx) = open(cx, store, None);
    for entry in [user("Do this", false), AgentEntry::Thought { text: "Thinking".into() }, agent(" ")] {
        reply(&store, generation, "one", entry, cx);
    }
    let child = Some("child".to_owned());
    let streaming = AgentEvent::Entry { task: "one".into(), subagent: child, index: 0, entry: agent("Working") };
    emit(&store, generation, streaming, cx);
    let mut renamed = task("one", AgentStatus::Running, 0.0);
    renamed.title = "Renamed".into();
    emit(&store, generation, AgentEvent::Task { task: renamed }, cx);
    assert!(unread(&view, cx).is_empty());
    let rows = view.read_with(cx, |view, cx| view.rows(cx));
    assert!(rows.iter().any(|row| row.contains(" | Renamed | ")), "{rows:?}");
}

/// Showing a task's conversation clears its mark and leaves the other task's; while a second view of the same
/// conversation still shows it, a new reply stays read.
#[gpui_kit::test]
async fn clears_only_the_viewed_task_and_keeps_reading_while_either_split_shows_it(cx: &mut TestAppContext) {
    let (store, generation) =
        connected_store(vec![task("one", AgentStatus::Running, 1.0), task("two", AgentStatus::Ready, 0.0)]);
    let (view, store, _, cx) = open(cx, store, None);
    reply(&store, generation, "one", agent("New reply"), cx);
    reply(&store, generation, "two", agent("New reply"), cx);
    let (left, right) = (session_of(&store, "one", cx), session_of(&store, "one", cx));
    set_shown(&left, true, cx);
    set_shown(&right, true, cx);
    assert_eq!(unread(&view, cx), ["two"]);
    set_shown(&left, false, cx);
    reply(&store, generation, "one", agent("Another reply"), cx);
    emit(&store, generation, AgentEvent::Task { task: task("one", AgentStatus::Ready, 1.0) }, cx);
    assert_eq!(unread(&view, cx), ["two"]);
}

/// An archived task loses its mark and gets none from a late reply; a removed task's row goes and a late reply
/// brings nothing back; after the project changes, only its tasks are listed, none unread.
#[gpui_kit::test]
async fn cleans_up_archived_and_deleted_tasks_ignores_late_replies_and_resets_on_project_switch(
    cx: &mut TestAppContext,
) {
    let (store, generation) =
        connected_store(vec![task("one", AgentStatus::Running, 1.0), task("two", AgentStatus::Ready, 0.0)]);
    let (view, store, _, cx) = open(cx, store, None);
    reply(&store, generation, "one", agent("New reply"), cx);
    reply(&store, generation, "two", agent("New reply"), cx);
    emit(&store, generation, AgentEvent::Task { task: task("one", AgentStatus::Archived, 1.0) }, cx);
    reply(&store, generation, "one", agent("Late reply"), cx);
    click(cx, "agents-group-archived", Modifiers::default());
    assert_eq!(unread(&view, cx), ["two"]);
    emit(&store, generation, AgentEvent::Removed { id: "two".into() }, cx);
    reply(&store, generation, "two", agent("Late reply"), cx);
    assert!(unread(&view, cx).is_empty());
    assert!(cx.debug_bounds("agents-task-two").is_none());

    store.update(cx, |store, cx| {
        store.disconnect();
        let fresh = generation_of(&store.connect(true));
        let other = vec![task("other", AgentStatus::Running, 0.0)];
        store.connected(fresh, Ok((other, Vec::new())), &Editors::default());
        // A late event of the old project is dropped.
        let late = AgentEvent::Task { task: task("one", AgentStatus::Ready, 1.0) };
        store.on_event(generation, late, &I18n::default());
        cx.notify();
    });
    cx.run_until_parked();
    let rows = view.read_with(cx, |view, cx| view.rows(cx));
    assert_eq!(rows, ["v Running (1)", "Running | other | claude · other | 10m"]);
    assert!(unread(&view, cx).is_empty());
}

/// A conversation kept alive but not shown leaves its task unread; once shown, the task is read.
#[gpui_kit::test]
async fn leaves_a_task_unread_while_its_kept_alive_tab_is_hidden_and_reads_it_once_shown(cx: &mut TestAppContext) {
    let (store, generation) = connected_store(vec![task("one", AgentStatus::Running, 0.0)]);
    let (view, store, _, cx) = open(cx, store, None);
    reply(&store, generation, "one", agent("Done"), cx);
    let session = session_of(&store, "one", cx);
    set_shown(&session, false, cx);
    assert_eq!(unread(&view, cx), ["one"]);
    set_shown(&session, true, cx);
    assert!(unread(&view, cx).is_empty());
}
