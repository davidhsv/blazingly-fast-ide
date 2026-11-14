use super::*;

/// An ACP agent for tests with one setting, `mode` ("Mode": Ask or Code, Ask at first), offered by `session/new` and
/// changed by `session/set_config_option`, whose answer gives the options again. A prompt is answered with
/// "Mode <value>"; one mentioning "edit" first appends a line to `README.md` of the session's folder and answers
/// "Edited". It ends with its input.
const SCRIPTED_AGENT: &str = r#"import json, os, sys

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

def answer(request, result):
    send({"jsonrpc": "2.0", "id": request, "result": result})

def say(session, text):
    update = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}})

def options():
    values = [{"value": "ask", "name": "Ask"}, {"value": "code", "name": "Code"}]
    return [{"id": "mode", "name": "Mode", "category": "mode", "type": "select", "currentValue": mode,
             "options": values}]

mode = "ask"
folder = os.getcwd()
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
        answer(request, {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}, "authMethods": []})
    elif method == "session/new":
        folder = params.get("cwd") or folder
        answer(request, {"sessionId": "scripted-session", "configOptions": options()})
    elif method == "session/set_config_option":
        if params.get("configId") == "mode":
            mode = params.get("value", mode)
        answer(request, {"configOptions": options()})
    elif method == "session/prompt":
        session = params.get("sessionId", "scripted-session")
        blocks = params.get("prompt") or []
        text = " ".join(block.get("text", "") for block in blocks if block.get("type") == "text")
        if "edit" in text:
            with open(os.path.join(folder, "README.md"), "a") as readme:
                readme.write("Edited by the agent.\n")
            say(session, "Edited")
        else:
            say(session, "Mode " + mode)
        answer(request, {"stopReason": "end_turn"})
    else:
        send({"jsonrpc": "2.0", "id": request, "error": {"code": -32601, "message": method}})
"#;

/// The project's folder and root, its window and its agents.
type ScriptedProject<'a> = (
    tempfile::TempDir,
    std::path::PathBuf,
    Entity<Workbench>,
    Entity<crate::agents::store::AgentsStore>,
    &'a mut VisualTestContext,
);

/// A trusted project in an active window whose agent `fake` is [`SCRIPTED_AGENT`]; `repository`: the project is a
/// repository with its files committed before any task starts.
fn scripted_agents_project<'a>(
    cx: &'a mut TestAppContext,
    home: &std::path::Path,
    repository: bool,
) -> ScriptedProject<'a> {
    install_fake_agent(home, cx);
    // The agent's file names this script; nothing has started it yet.
    std::fs::write(home.join("fake_agent.py"), SCRIPTED_AGENT).expect("the agent script is written");
    let (folder, root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    if repository {
        init_repo(&root);
    }
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    (folder, root, workbench, store, cx)
}

fn shows(workbench: &Entity<Workbench>, line: &str, cx: &mut VisualTestContext) -> bool {
    agent_conversation(workbench, cx).iter().any(|shown| shown.starts_with(line))
}

fn docked_composer(
    workbench: &Entity<Workbench>,
    cx: &mut VisualTestContext,
) -> Entity<crate::agents::composer_view::ComposerView> {
    let docked = workbench.read_with(cx, |w, _| w.docked_agent_session());
    docked.map(|(_, composer)| composer).expect("the conversation docks")
}

/// The settings chips of the docked conversation's composer.
fn chips(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
    let composer = docked_composer(workbench, cx);
    composer.read_with(cx, |composer, cx| composer.chips(cx))
}

/// Picking another value of an agent's setting in the composer changes it in the running agent: its chip shows the
/// new value, and the agent's next answer is given in it.
#[gpui_kit::test]
async fn picking_a_setting_in_the_composer_changes_it_in_the_running_agent(cx: &mut TestAppContext) {
    const CHIPS: [&str; 3] = ["composer-config-0", "composer-config-1", "composer-config-2"];
    let home = start(cx);
    let (_folder, _root, workbench, store, cx) = scripted_agents_project(cx, home.path(), false);
    let task = start_agent_task(&workbench, &store, "Which mode?", cx);
    let replied =
        |line: &str, cx: &mut VisualTestContext| !agent_working(&store, &task, cx) && shows(&workbench, line, cx);
    wait_long(cx, "the first reply", |cx| replied("agent: Mode ask", cx));
    wait_long(cx, "the Mode chip", |cx| chips(&workbench, cx).iter().any(|chip| chip.contains("Ask")));
    let index = chips(&workbench, cx).iter().position(|chip| chip.contains("Ask")).expect("the Mode chip");
    click_on(cx, CHIPS.get(index).copied().expect("one of the first chips"));
    let composer = docked_composer(&workbench, cx);
    let entries = composer.read_with(cx, |composer, _| composer.picker_entries()).expect("the picker opens");
    assert_eq!(entries.iter().map(|entry| entry.trim_end_matches(" *")).collect::<Vec<_>>(), ["Ask", "Code"]);
    click_on(cx, "composer-picker-1");
    wait_long(cx, "the chip to show Code", |cx| chips(&workbench, cx).iter().any(|chip| chip.contains("Code")));

    store.update(cx, |store, cx| {
        store.update_composer(&task, |draft| draft.text = "Which mode now?".to_owned());
        cx.notify();
    });
    composer.update(cx, |composer, cx| composer.send(false, cx));
    wait_long(cx, "the reply in the new mode", |cx| replied("agent: Mode code", cx));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}

/// Once a turn in which the agent edited a file of the repository ends, the task's changes list that file with the
/// lines it added.
#[gpui_kit::test]
async fn a_finished_turn_lists_the_file_the_agent_edited_among_the_tasks_changes(cx: &mut TestAppContext) {
    let home = start(cx);
    let (_folder, root, workbench, store, cx) = scripted_agents_project(cx, home.path(), true);
    let task = start_agent_task(&workbench, &store, "Please edit the readme", cx);
    wait_long(cx, "the reply", |cx| !agent_working(&store, &task, cx) && shows(&workbench, "agent: Edited", cx));
    let text = std::fs::read_to_string(root.join("README.md")).expect("the readme is read");
    assert!(text.ends_with("Edited by the agent.\n"), "{text}");
    let changed = |cx: &mut VisualTestContext| {
        store.read_with(cx, |store, _| {
            store.changes(&task).map(|changes| {
                changes.iter().map(|change| (change.rel_path.clone(), change.added, change.removed)).collect::<Vec<_>>()
            })
        })
    };
    wait_long(cx, "the task's changes", |cx| changed(cx).is_some_and(|changes| !changes.is_empty()));
    assert_eq!(changed(cx), Some(vec![("README.md".to_owned(), 1, 0)]));
    workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
}
