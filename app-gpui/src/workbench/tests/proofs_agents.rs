use super::*;

/// An ACP agent for tests that can copy and roll back its session: `initialize` offers `session/fork` and the
/// `_openidea/rollback` extension. Every prompt is answered with "Answer <n>", numbered from 1 across the
/// process, so a regenerated reply is told apart from the one it replaces. It ends with its input.
const CAPABLE_AGENT: &str = r#"import json, sys

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

def answer(request, result):
    send({"jsonrpc": "2.0", "id": request, "result": result})

answered = 0
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    if method is None or "id" not in message:
        continue
    request = message["id"]
    if method == "initialize":
        capabilities = {"loadSession": False, "sessionCapabilities": {"fork": {}},
                        "_meta": {"openidea": {"rollback": True}}}
        answer(request, {"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []})
    elif method == "session/new":
        answer(request, {"sessionId": "capable-session"})
    elif method == "session/fork":
        answer(request, {"sessionId": "capable-copy"})
    elif method in ("_openidea/rollback", "session/load"):
        answer(request, {})
    elif method == "session/prompt":
        answered += 1
        session = params.get("sessionId", "capable-session")
        update = {"sessionUpdate": "agent_message_chunk",
                  "content": {"type": "text", "text": "Answer %d" % answered}}
        send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}})
        answer(request, {"stopReason": "end_turn"})
    else:
        send({"jsonrpc": "2.0", "id": request, "error": {"code": -32601, "message": method}})
"#;

/// A trusted project in an active window whose agent `fake` is [`CAPABLE_AGENT`].
fn capable_agents_project<'a>(cx: &'a mut TestAppContext, home: &std::path::Path) -> AgentsFixture<'a> {
    install_fake_agent(home, cx);
    // The agent's file names this script; nothing has started it yet.
    std::fs::write(home.join("fake_agent.py"), CAPABLE_AGENT).expect("the agent script is written");
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    (folder, workbench, store, cx)
}

fn docked_task(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let session = workbench.read_with(cx, |w, _| w.docked_agent_session().map(|(session, _)| session))?;
    Some(session.read_with(cx, |session, _| session.task().to_owned()))
}

fn shows(workbench: &Entity<Workbench>, line: &str, cx: &mut VisualTestContext) -> bool {
    agent_conversation(workbench, cx).iter().any(|shown| shown.starts_with(line))
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

/// With an agent that can roll its session back and copy it: Regenerate Response replaces the last reply with a
/// new one; Branch Conversation from Here copies the conversation up to that reply into a new task, which opens;
/// Fork copies a whole task; Retract drops the last message from the conversation and puts it back in the
/// composer.
#[gpui_kit::test]
async fn an_agent_that_can_roll_back_regenerates_branches_forks_and_retracts(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, store, cx) = capable_agents_project(cx, home.path());
    let task = start_agent_task(&workbench, &store, "Say hello", cx);
    let replied =
        |line: &str, cx: &mut VisualTestContext| !agent_working(&store, &task, cx) && shows(&workbench, line, cx);
    wait_long(cx, "the first reply", |cx| replied("agent: Answer 1", cx));
    let capabilities = store.read_with(cx, |store, _| store.task(&task).map(|t| t.capabilities));
    assert!(capabilities.is_some_and(|c| c.fork && c.rollback), "{capabilities:?}");
    send_in_composer(&workbench, &store, &task, "Say hello again", cx);
    wait_long(cx, "the second reply", |cx| replied("agent: Answer 2", cx));

    // Regenerate Response on the last reply: the agent answers that message again.
    run_action(cx, "agents.regenerateReply", Some(serde_json::json!({ "task": task, "turn": 1 })));
    wait_long(cx, "the regenerated reply", |cx| replied("agent: Answer 3", cx));
    let lines = agent_conversation(&workbench, cx);
    assert!(!lines.iter().any(|line| line.starts_with("agent: Answer 2")), "{lines:?}");
    assert_eq!(lines.iter().filter(|line| line.starts_with("you: Say hello again")).count(), 1, "{lines:?}");

    // Branch Conversation from Here on the first reply: a new task with the first turn only, shown.
    run_action(cx, "agents.forkReply", Some(serde_json::json!({ "task": task, "turn": 0 })));
    wait_long(cx, "the branch", |cx| store.read_with(cx, |store, _| store.tasks().len() == 2));
    wait_long(cx, "the branch to show", |cx| {
        docked_task(&workbench, cx).is_some_and(|docked| docked != task) && shows(&workbench, "agent: Answer 1", cx)
    });
    let lines = agent_conversation(&workbench, cx);
    assert!(lines.iter().any(|line| line.starts_with("you: Say hello")), "{lines:?}");
    assert!(!lines.iter().any(|line| line.starts_with("you: Say hello again")), "{lines:?}");

    // Fork copies the whole task.
    run_action(cx, "agents.forkTask", Some(serde_json::json!(task)));
    wait_long(cx, "the fork", |cx| store.read_with(cx, |store, _| store.tasks().len() == 3));

    // Retract on the last message of the first task: it leaves the conversation for the composer.
    run_action(cx, "agents.openTask", Some(serde_json::json!(task)));
    wait_long(cx, "the first task to show", |cx| docked_task(&workbench, cx).as_deref() == Some(task.as_str()));
    run_action(cx, "agents.retractMessage", Some(serde_json::json!({ "task": task, "turn": 1 })));
    wait_long(cx, "the retraction", |cx| {
        !shows(&workbench, "you: Say hello again", cx) && !shows(&workbench, "agent: Answer 3", cx)
    });
    assert!(shows(&workbench, "agent: Answer 1", cx), "{:?}", agent_conversation(&workbench, cx));
    let draft = store.read_with(cx, |store, _| store.composer(&task));
    assert!(draft.text.contains("Say hello again"), "{draft:?}");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// The keyboard in a conversation's composer turns `agentSessionFocus` on; in the text editor it is off.
#[gpui_kit::test]
async fn agent_session_focus_is_on_only_while_a_conversation_has_the_keyboard(cx: &mut TestAppContext) {
    let home = start(cx);
    let (folder, workbench, store, cx) = agents_project(cx, home.path());
    let task = start_agent_task(&workbench, &store, "Say hello", cx);
    wait_long(cx, "the reply", |cx| !agent_working(&store, &task, cx) && shows(&workbench, "agent: Hello", cx));
    let readme = folder.path().canonicalize().expect("the folder exists").join("README.md");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(readme, window, cx));
    wait_long(cx, "the editor", |cx| workbench.read_with(cx, |w, _| w.editor().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("an editor");
    let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
    let focus_editor = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| window.focus(&editor.focus_handle(cx), cx));
        cx.run_until_parked();
    };
    focus_editor(cx);
    assert!(!context_is_on(cx, "agentSessionFocus"), "the editor has the keyboard");
    cx.update(|window, cx| window.focus(&composer.focus_handle(cx), cx));
    cx.run_until_parked();
    assert!(context_is_on(cx, "agentSessionFocus"), "the conversation has the keyboard");
    focus_editor(cx);
    assert!(!context_is_on(cx, "agentSessionFocus"), "the editor has the keyboard again");
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// More Agents… opens Settings on the Agents page.
#[gpui_kit::test]
async fn more_agents_opens_settings_on_the_agents_page(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, workbench, _store, cx) = agents_project(cx, home.path());
    run_action(cx, "agents.addAgent", None);
    wait_long(cx, "the Settings dialog", |cx| settings_dialog_of(&workbench, cx).is_some());
    let dialog = settings_dialog_of(&workbench, cx).expect("Settings opens");
    assert_eq!(dialog.read_with(cx, |view, cx| view.selected_page(cx)).as_deref(), Some("tools.agents"));
    click_on(cx, "settings-cancel");
    assert!(settings_dialog_of(&workbench, cx).is_none());
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
