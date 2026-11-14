use super::*;
use crate::agents::task_actions::TaskCommand;
use crate::agents::task_list_view::TaskListView;
use ide_agent::{AgentStatus, Isolation};

/// An ACP agent for tests that can copy and roll back its session, and logs the method of every message it
/// reads, a line each, to `agent-calls.log` in the session's folder. Every prompt is numbered from 1 across the
/// process. A prompt ending with "wait" says "Working" and works until it is cancelled; one ending with
/// "stubborn" says "Working" and never stops, cancelled or not; any other is answered with "Answer <n>". Once a
/// prompt has mentioned "refuse", rolling back fails with "Rollback failed". It ends with its input.
const EDITING_AGENT: &str = r#"import json, os, sys

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

def answer(request, result):
    send({"jsonrpc": "2.0", "id": request, "result": result})

def say(session, text):
    update = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}})

folder = os.getcwd()
prompts = 0
working = None
refuse = False
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    if method == "session/new":
        folder = params.get("cwd") or folder
    if method is not None:
        with open(os.path.join(folder, "agent-calls.log"), "a") as log:
            log.write(method + "\n")
    if method == "session/cancel":
        if working is not None and not working[1]:
            answer(working[0], {"stopReason": "cancelled"})
            working = None
        continue
    if method is None or "id" not in message:
        continue
    request = message["id"]
    if method == "initialize":
        capabilities = {"loadSession": False, "sessionCapabilities": {"fork": {}},
                        "_meta": {"openidea": {"rollback": True}}}
        answer(request, {"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []})
    elif method == "session/new":
        answer(request, {"sessionId": "editing-session"})
    elif method == "session/fork":
        answer(request, {"sessionId": "editing-copy"})
    elif method == "session/load":
        answer(request, {})
    elif method == "_openidea/rollback":
        if refuse:
            send({"jsonrpc": "2.0", "id": request, "error": {"code": -32000, "message": "Rollback failed"}})
        else:
            answer(request, {})
    elif method == "session/prompt":
        prompts += 1
        session = params.get("sessionId", "editing-session")
        blocks = params.get("prompt") or []
        text = " ".join(block.get("text", "") for block in blocks if block.get("type") == "text").strip()
        refuse = refuse or "refuse" in text
        if text.endswith("wait") or text.endswith("stubborn"):
            say(session, "Working")
            working = (request, text.endswith("stubborn"))
        else:
            say(session, "Answer %d" % prompts)
            answer(request, {"stopReason": "end_turn"})
    else:
        send({"jsonrpc": "2.0", "id": request, "error": {"code": -32601, "message": method}})
"#;

/// A `&'static str` selector built at run time.
fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

fn task_ids(store: &Entity<crate::agents::store::AgentsStore>, cx: &mut VisualTestContext) -> Vec<String> {
    store.read_with(cx, |store, _| store.tasks().iter().map(|t| t.id.clone()).collect())
}

fn shows(workbench: &Entity<Workbench>, line: &str, cx: &mut VisualTestContext) -> bool {
    agent_conversation(workbench, cx).iter().any(|shown| shown.starts_with(line))
}

fn docked_task(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let session = workbench.read_with(cx, |w, _| w.docked_agent_session().map(|(session, _)| session))?;
    Some(session.read_with(cx, |session, _| session.task().to_owned()))
}

fn list_of(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TaskListView> {
    workbench.read_with(cx, |w, _| w.agents_list()).expect("the tool window has its view")
}

/// Starts a task with `prompt` from the New Task page (New Agent Task with the prompt, then Start) and waits for
/// the conversation to dock; returns the new task's id.
fn start_task(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    prompt: &str,
    cx: &mut VisualTestContext,
) -> String {
    let before = task_ids(store, cx);
    run_action(cx, "agents.newTask", Some(serde_json::json!(prompt)));
    wait_long(cx, "the New Task page", |cx| workbench.read_with(cx, |w, _| w.agents_home_page().is_some()));
    // Only the fake agent may ever start.
    assert_eq!(store.read_with(cx, |store, _| store.draft.provider.clone()), "fake");
    click_on(cx, "agents-home-start");
    wait_long(cx, "the new task", |cx| task_ids(store, cx).len() == before.len() + 1);
    let task = task_ids(store, cx).into_iter().find(|id| !before.contains(id)).expect("a new task");
    wait_long(cx, "the new task's conversation", |cx| docked_task(workbench, cx).as_deref() == Some(task.as_str()));
    task
}

/// Starts a task with `prompt` and waits for its first reply to end.
fn finished_task(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    prompt: &str,
    cx: &mut VisualTestContext,
) -> String {
    let task = start_task(workbench, store, prompt, cx);
    wait_long(cx, "the reply", |cx| !agent_working(store, &task, cx) && shows(workbench, "agent: ", cx));
    task
}

/// Starts a task whose prompt keeps the fake agent working, and waits for it to say hello.
fn running_task(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    prompt: &str,
    cx: &mut VisualTestContext,
) -> String {
    let task = start_task(workbench, store, prompt, cx);
    wait_long(cx, "the agent at work", |cx| {
        agent_status(store, &task, cx) == Some(AgentStatus::Running) && shows(workbench, "agent: Hello", cx)
    });
    task
}

fn open_task(workbench: &Entity<Workbench>, task: &str, cx: &mut VisualTestContext) {
    run_action(cx, "agents.openTask", Some(serde_json::json!(task)));
    wait_long(cx, "the task to show", |cx| docked_task(workbench, cx).as_deref() == Some(task));
}

/// Opens the history popup of the docked conversation, which selects the docked task.
fn open_history(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Entity<TaskListView> {
    let list = list_of(workbench, cx);
    if !list.read_with(cx, |list, _| list.history_open()) {
        click_on(cx, "agents-history");
    }
    assert!(list.read_with(cx, |list, _| list.history_open()), "the history opens");
    list
}

fn click_row(cx: &mut VisualTestContext, task: &str, modifiers: gpui_kit::Modifiers) {
    cx.run_until_parked();
    let selector = leak(format!("agents-task-{task}"));
    let bounds = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"));
    cx.simulate_click(bounds.center(), modifiers);
    cx.run_until_parked();
}

fn right_click_row(cx: &mut VisualTestContext, task: &str) {
    cx.run_until_parked();
    let selector = leak(format!("agents-task-{task}"));
    let at = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    cx.simulate_mouse_down(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.simulate_mouse_up(at, gpui_kit::MouseButton::Right, gpui_kit::Modifiers::default());
    cx.run_until_parked();
}

/// Chooses the row labelled `label` of the open task menu.
fn choose_in_menu(workbench: &Entity<Workbench>, label: &str, cx: &mut VisualTestContext) {
    let menu = workbench.read_with(cx, |w, _| w.open_menu::<TaskCommand>()).expect("the task menu is open");
    let lines = menu.read_with(cx, |menu, _| menu.lines());
    let index = lines[0].iter().position(|line| line.trim_start_matches("> ") == label);
    let index = index.unwrap_or_else(|| panic!("{label} in {lines:?}"));
    click_on(cx, leak(format!("menu-row-0-{index}")));
}

fn toggle() -> gpui_kit::Modifiers {
    gpui_kit::Modifiers { platform: true, ..gpui_kit::Modifiers::default() }
}

/// Where a row of the task list stands: the index of the row whose title is `title`.
fn row_index(list: &Entity<TaskListView>, title: &str, cx: &mut VisualTestContext) -> usize {
    let rows = list.read_with(cx, |list, cx| list.rows(cx));
    let wanted = format!(" | {title} | ");
    rows.iter().position(|row| row.contains(&wanted)).unwrap_or_else(|| panic!("{title} in {rows:?}"))
}

/// Selecting a range of tasks across two groups in the history and deleting it from the right-clicked row's menu
/// asks once for the three tasks and deletes them all, keeping the project's files.
#[gpui_kit::test]
async fn deletes_the_right_clicked_selection_after_one_confirmation_excluding_group_headers(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = agents_project(cx, home.path());
    let first = finished_task(&workbench, &store, "First greeting", cx);
    let running = running_task(&workbench, &store, "Second, then wait", cx);
    let third = finished_task(&workbench, &store, "Third greeting", cx);
    // The running task heads the list; the range runs from the lower of the two others up to it.
    let list = list_of(&workbench, cx);
    let lower = if row_index(&list, "First greeting", cx) > row_index(&list, "Third greeting", cx) {
        first.clone()
    } else {
        third.clone()
    };
    assert!(row_index(&list, "Second, then wait", cx) < row_index(&list, "First greeting", cx));
    open_task(&workbench, &lower, cx);
    let list = open_history(&workbench, cx);
    assert_eq!(list.read_with(cx, |list, _| list.selected()), [lower.clone()]);
    click_row(cx, &running, gpui_kit::Modifiers { shift: true, ..gpui_kit::Modifiers::default() });
    let mut selected = list.read_with(cx, |list, _| list.selected());
    selected.sort();
    let mut all = vec![first.clone(), running.clone(), third.clone()];
    all.sort();
    assert_eq!(selected, all, "the group header between them is not a task");

    right_click_row(cx, &lower);
    choose_in_menu(&workbench, "Delete (3)…", cx);
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Delete Agent Tasks");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), None, "no task has a worktree");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the three tasks to be deleted", |cx| task_ids(&store, cx).is_empty());
    assert!(workbench.read_with(cx, |w, _| w.vcs_prompt()).is_none(), "one confirmation was enough");
    let readme = folder.path().join("README.md");
    assert_eq!(std::fs::read_to_string(readme).ok().as_deref(), Some("A project for agents.\n"));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Cancelling the confirmation of a delete of several tasks deletes none of them.
#[gpui_kit::test]
async fn cancels_the_whole_batch(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = agents_project(cx, home.path());
    let one = finished_task(&workbench, &store, "First greeting", cx);
    let two = finished_task(&workbench, &store, "Second greeting", cx);
    run_action(cx, "agents.deleteTasks", Some(serde_json::json!([one, two])));
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Delete Agent Tasks");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the dialog to close", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
    let mut left = task_ids(&store, cx);
    left.sort();
    let mut both = vec![one, two];
    both.sort();
    assert_eq!(left, both);
    let rows = list_of(&workbench, cx).read_with(cx, |list, cx| list.rows(cx));
    assert!(rows.iter().any(|row| row.contains(" | First greeting | ")), "{rows:?}");
    assert!(rows.iter().any(|row| row.contains(" | Second greeting | ")), "{rows:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// In a selection of a running, an idle and an archived task, the menu offers Archive for the one idle task, and
/// choosing it archives that task alone.
#[gpui_kit::test]
async fn archives_only_eligible_tasks_in_a_mixed_selection(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = agents_project(cx, home.path());
    let idle = finished_task(&workbench, &store, "First greeting", cx);
    let running = running_task(&workbench, &store, "Second, then wait", cx);
    let archived = finished_task(&workbench, &store, "Third greeting", cx);
    run_action(cx, "agents.archiveTasks", Some(serde_json::json!([archived])));
    wait_long(cx, "the archive", |cx| agent_status(&store, &archived, cx) == Some(AgentStatus::Archived));
    open_task(&workbench, &idle, cx);
    open_history(&workbench, cx);
    // The archived group starts collapsed; a click opens it.
    click_on(cx, "agents-group-archived");
    for task in [&idle, &running, &archived] {
        click_row(cx, task, toggle());
    }
    let list = list_of(&workbench, cx);
    assert_eq!(list.read_with(cx, |list, _| list.selected()).len(), 3);
    right_click_row(cx, &idle);
    choose_in_menu(&workbench, "Archive (1)", cx);
    wait_long(cx, "the idle task to be archived", |cx| agent_status(&store, &idle, cx) == Some(AgentStatus::Archived));
    assert_eq!(agent_status(&store, &running, cx), Some(AgentStatus::Running), "a running task is not archived");
    assert_eq!(agent_status(&store, &archived, cx), Some(AgentStatus::Archived));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// In a selection of an idle and two archived tasks, the menu offers Unarchive for the two archived tasks, and
/// choosing it brings back both and leaves the idle one as it was.
#[gpui_kit::test]
async fn unarchives_only_archived_tasks_in_a_mixed_selection(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = agents_project(cx, home.path());
    let idle = finished_task(&workbench, &store, "First greeting", cx);
    let second = finished_task(&workbench, &store, "Second greeting", cx);
    let third = finished_task(&workbench, &store, "Third greeting", cx);
    run_action(cx, "agents.archiveTasks", Some(serde_json::json!([second, third])));
    wait_long(cx, "the archive", |cx| {
        [&second, &third].iter().all(|task| agent_status(&store, task, cx) == Some(AgentStatus::Archived))
    });
    open_task(&workbench, &idle, cx);
    open_history(&workbench, cx);
    click_on(cx, "agents-group-archived");
    for task in [&idle, &second, &third] {
        click_row(cx, task, toggle());
    }
    right_click_row(cx, &second);
    choose_in_menu(&workbench, "Unarchive (2)", cx);
    wait_long(cx, "both archived tasks to come back", |cx| {
        [&second, &third].iter().all(|task| agent_status(&store, task, cx).is_some_and(|s| s != AgentStatus::Archived))
    });
    assert_eq!(agent_status(&store, &idle, cx), Some(AgentStatus::Ready));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Deleting worktree tasks asks with a checkbox that removes their worktree folders: ticked, the worktree task's
/// folder goes and the local task's project stays; unticked, the worktree folder stays on disk.
#[gpui_kit::test]
async fn applies_worktree_cleanup_only_to_worktree_tasks(cx: &mut TestAppContext) {
    let home = start(cx);
    install_fake_agent(home.path(), cx);
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    init_repo(&root);
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    let local = finished_task(&workbench, &store, "Local greeting", cx);
    store.update(cx, |store, cx| {
        store.draft.isolation = Isolation::Worktree;
        store.draft.isolation_chosen = true;
        cx.notify();
    });
    let cleaned = finished_task(&workbench, &store, "Worktree greeting", cx);
    let kept = finished_task(&workbench, &store, "Another worktree greeting", cx);
    let folder_of = |task: &str, cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| store.task(task).map(|t| (t.isolation, t.cwd.0.clone())))
    };
    let (isolation, cleaned_folder) = folder_of(&cleaned, cx).expect("the worktree task");
    assert_eq!(isolation, Isolation::Worktree);
    let (_, kept_folder) = folder_of(&kept, cx).expect("the other worktree task");
    assert_eq!(folder_of(&local, cx).map(|(isolation, _)| isolation), Some(Isolation::Local));
    assert!(cleaned_folder.is_dir() && kept_folder.is_dir() && cleaned_folder != root);

    run_action(cx, "agents.deleteTasks", Some(serde_json::json!([cleaned, local])));
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(true), "the checkbox starts ticked");
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "both tasks to be deleted", |cx| task_ids(&store, cx) == [kept.clone()]);
    wait_long(cx, "the worktree folder to go", |_| !cleaned_folder.exists());
    assert_eq!(std::fs::read_to_string(root.join("README.md")).ok().as_deref(), Some("A project for agents.\n"));

    run_action(cx, "agents.deleteTasks", Some(serde_json::json!([kept])));
    let dialog = wait_for_prompt(&workbench, cx);
    click_on(cx, "prompt-checkbox");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.checked()), Some(false));
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the last task to be deleted", |cx| task_ids(&store, cx).is_empty());
    assert!(kept_folder.is_dir(), "an unticked delete keeps the worktree folder");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

// ----- editing and retracting sent messages -----

/// A trusted project in an active window whose agent `fake` is [`EDITING_AGENT`].
fn editing_project<'a>(cx: &'a mut TestAppContext, home: &std::path::Path) -> AgentsFixture<'a> {
    install_fake_agent(home, cx);
    // The agent's file names this script; nothing has started it yet.
    std::fs::write(home.join("fake_agent.py"), EDITING_AGENT).expect("the agent script is written");
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    (folder, workbench, store, cx)
}

/// The methods the agent has read, in order.
fn agent_calls(folder: &tempfile::TempDir) -> Vec<String> {
    let root = folder.path().canonicalize().expect("the folder exists");
    let log = std::fs::read_to_string(root.join("agent-calls.log")).unwrap_or_default();
    log.lines().map(str::to_owned).collect()
}

/// Where the agent first read `method`.
fn first_index(calls: &[String], method: &str) -> usize {
    calls.iter().position(|call| call == method).unwrap_or_else(|| panic!("{method} in {calls:?}"))
}

fn count(calls: &[String], method: &str) -> usize {
    calls.iter().filter(|call| *call == method).count()
}

fn draft_text(store: &Entity<crate::agents::store::AgentsStore>, task: &str, cx: &mut VisualTestContext) -> String {
    store.read_with(cx, |store, _| store.composer(task).text)
}

/// Sends `text` from the docked conversation's composer.
fn send_in_composer(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    task: &str,
    text: &str,
    cx: &mut VisualTestContext,
) {
    let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    store.update(cx, |store, cx| {
        store.update_composer(task, |draft| draft.text = text.to_owned());
        cx.notify();
    });
    composer.update(cx, |composer, cx| composer.send(false, cx));
}

/// Waits for the agent to be at work on `task`, having said so.
fn wait_until_working(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    task: &str,
    cx: &mut VisualTestContext,
) {
    wait_long(cx, "the agent at work", |cx| agent_working(store, task, cx) && shows(workbench, "agent: Working", cx));
}

/// Waits for the turn of `task` to end with `line` in the conversation.
fn wait_for_answer(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    task: &str,
    line: &str,
    cx: &mut VisualTestContext,
) {
    wait_long(cx, line, |cx| !agent_working(store, task, cx) && shows(workbench, line, cx));
}

/// A task whose first message was answered, then `then` sent; waits for the agent to take it up.
fn two_turns(
    workbench: &Entity<Workbench>,
    store: &Entity<crate::agents::store::AgentsStore>,
    first: &str,
    then: &str,
    cx: &mut VisualTestContext,
) -> String {
    let task = start_task(workbench, store, first, cx);
    wait_for_answer(workbench, store, &task, "agent: Answer 1", cx);
    send_in_composer(workbench, store, &task, then, cx);
    let then_line = format!("you: {then}");
    wait_long(cx, "the second message", |cx| shows(workbench, &then_line, cx));
    task
}

/// Edit on the first message, a change and Send: the agent's session is rolled back before the edited text is
/// sent, and the conversation shows the edited message with its new answer in place of the old turn.
#[gpui_kit::test]
async fn rolls_back_before_sending_the_edited_text_when_send_is_clicked(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = start_task(&workbench, &store, "Original message", cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 1", cx);
    click_on(cx, "session-user-0-edit");
    cx.simulate_input(" edited");
    click_on(cx, "session-edit-send");
    wait_long(cx, "the edited turn", |cx| {
        !agent_working(&store, &task, cx)
            && shows(&workbench, "you: Original message edited", cx)
            && shows(&workbench, "agent: Answer 2", cx)
    });
    let lines = agent_conversation(&workbench, cx);
    assert!(!lines.iter().any(|line| line.starts_with("agent: Answer 1")), "{lines:?}");
    assert_eq!(lines.iter().filter(|line| line.starts_with("you: ")).count(), 1, "{lines:?}");
    let calls = agent_calls(&folder);
    let rollback = calls.iter().position(|call| call == "_openidea/rollback").expect("a rollback");
    let last_prompt = calls.iter().rposition(|call| call == "session/prompt").expect("a prompt");
    assert!(rollback < last_prompt, "the rollback comes before the edited text: {calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 2, "{calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Sending an edit of a message while its turn still runs first stops the agent, then rolls the session back,
/// then sends the edited text.
#[gpui_kit::test]
async fn waits_for_a_running_turn_to_stop_before_rolling_it_back(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = start_task(&workbench, &store, "Say it and wait", cx);
    wait_until_working(&workbench, &store, &task, cx);
    click_on(cx, "session-user-0-edit");
    cx.simulate_input(" now");
    click_on(cx, "session-edit-send");
    wait_long(cx, "the edited turn", |cx| {
        !agent_working(&store, &task, cx)
            && shows(&workbench, "you: Say it and wait now", cx)
            && shows(&workbench, "agent: Answer 2", cx)
    });
    let calls = agent_calls(&folder);
    let at = |method: &str| first_index(&calls, method);
    let last_prompt = calls.iter().rposition(|call| call == "session/prompt").expect("a prompt");
    assert!(at("session/cancel") < at("_openidea/rollback") && at("_openidea/rollback") < last_prompt, "{calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Retract Message on a running first turn stops the agent before rolling back, and puts the message back in the
/// composer above the draft there, sending nothing.
#[gpui_kit::test]
async fn stops_before_rollback_and_restores_text_without_sending_turn_0(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = start_task(&workbench, &store, "Say it and wait", cx);
    wait_until_working(&workbench, &store, &task, cx);
    store.update(cx, |store, cx| {
        store.update_composer(&task, |draft| draft.text = "Draft".to_owned());
        cx.notify();
    });
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 0 })));
    wait_long(cx, "the message back in the composer", |cx| draft_text(&store, &task, cx) == "Say it and wait\nDraft");
    wait_long(cx, "the turn to leave the conversation", |cx| !shows(&workbench, "you: Say it and wait", cx));
    let calls = agent_calls(&folder);
    let at = |method: &str| first_index(&calls, method);
    assert!(at("session/cancel") < at("_openidea/rollback"), "{calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 1, "nothing is sent: {calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Retract Message on a running second turn stops the agent before rolling back that turn, keeps the first one,
/// and puts the message back in the composer above the draft there, sending nothing.
#[gpui_kit::test]
async fn stops_before_rollback_and_restores_text_without_sending_turn_1(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = two_turns(&workbench, &store, "First question", "Then wait", cx);
    wait_long(cx, "the agent at work", |cx| agent_working(&store, &task, cx));
    store.update(cx, |store, cx| {
        store.update_composer(&task, |draft| draft.text = "Draft".to_owned());
        cx.notify();
    });
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 1 })));
    wait_long(cx, "the message back in the composer", |cx| draft_text(&store, &task, cx) == "Then wait\nDraft");
    wait_long(cx, "the turn to leave the conversation", |cx| !shows(&workbench, "you: Then wait", cx));
    assert!(shows(&workbench, "you: First question", cx) && shows(&workbench, "agent: Answer 1", cx));
    let calls = agent_calls(&folder);
    let at = |method: &str| first_index(&calls, method);
    assert!(at("session/cancel") < at("_openidea/rollback"), "{calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 2, "nothing more is sent: {calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Retracting a turn whose agent does not stop gives up once the stop times out: the message is back in the
/// composer and the session is never rolled back.
#[gpui_kit::test]
async fn keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = two_turns(&workbench, &store, "First question", "Then stubborn", cx);
    wait_until_working(&workbench, &store, &task, cx);
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 1 })));
    wait_long(cx, "the message back in the composer", |cx| draft_text(&store, &task, cx) == "Then stubborn");
    let calls = agent_calls(&folder);
    assert_eq!(count(&calls, "session/cancel"), 1, "{calls:?}");
    assert_eq!(count(&calls, "_openidea/rollback"), 0, "{calls:?}");
    assert!(shows(&workbench, "you: Then stubborn", cx), "{:?}", agent_conversation(&workbench, cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Retracting a turn whose rollback the agent refuses puts the message back in the composer and sends nothing.
#[gpui_kit::test]
async fn recovers_the_original_prompt_when_rollback_fails(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = two_turns(&workbench, &store, "First, refuse rollbacks", "Second question", cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 2", cx);
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 1 })));
    wait_long(cx, "the message back in the composer", |cx| draft_text(&store, &task, cx) == "Second question");
    let calls = agent_calls(&folder);
    assert_eq!(count(&calls, "_openidea/rollback"), 1, "{calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 2, "nothing more is sent: {calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Retract Message on a turn with later messages asks first, once however often it is asked for; Cancel keeps
/// the conversation, and a confirmed retract rolls back that turn and puts its message in the composer.
#[gpui_kit::test]
async fn confirms_before_retracting_a_turn_with_later_messages_and_guards_repeated_clicks(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = two_turns(&workbench, &store, "First question", "Second question", cx);
    wait_for_answer(&workbench, &store, &task, "agent: Answer 2", cx);
    click_on(cx, "session-user-0-retract");
    let dialog = wait_for_prompt(&workbench, cx);
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 0 })));
    let shown = workbench.read_with(cx, |w, _| w.vcs_prompt()).map(|dialog| dialog.entity_id());
    assert_eq!(shown, Some(dialog.entity_id()), "the second request asks nothing more");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the dialog to close", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
    assert!(shows(&workbench, "you: First question", cx) && shows(&workbench, "you: Second question", cx));
    assert_eq!(count(&agent_calls(&folder), "_openidea/rollback"), 0);

    click_on(cx, "session-user-0-retract");
    wait_for_prompt(&workbench, cx);
    click_on(cx, "prompt-button-ok");
    wait_long(cx, "the message back in the composer", |cx| draft_text(&store, &task, cx) == "First question");
    wait_long(cx, "the conversation to empty", |cx| !shows(&workbench, "you: ", cx));
    assert_eq!(count(&agent_calls(&folder), "_openidea/rollback"), 1);
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Run Again From Here on the first message asks before starting a fresh session; cancelled, the running turn
/// and the conversation are left as they were.
#[gpui_kit::test]
async fn keeps_the_transcript_and_running_session_when_a_fresh_session_rerun_is_cancelled(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = editing_project(cx, home.path());
    let task = start_task(&workbench, &store, "Say it and wait", cx);
    wait_until_working(&workbench, &store, &task, cx);
    let before = agent_conversation(&workbench, cx);
    click_on(cx, "session-user-0-run-again");
    let dialog = wait_for_prompt(&workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Start a Fresh Agent Session");
    click_on(cx, "prompt-button-cancel");
    wait_long(cx, "the dialog to close", |cx| workbench.read_with(cx, |w, _| w.vcs_prompt().is_none()));
    assert_eq!(agent_conversation(&workbench, cx), before);
    assert_eq!(agent_status(&store, &task, cx), Some(AgentStatus::Running));
    let calls = agent_calls(&folder);
    assert_eq!((count(&calls, "session/cancel"), count(&calls, "_openidea/rollback")), (0, 0), "{calls:?}");
    assert_eq!(count(&calls, "session/prompt"), 1, "{calls:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
