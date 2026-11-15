use super::*;
use crate::agents::composer::FileRef;
use crate::agents::store::{AgentsStore, HOME};
use ide_agent::AgentImage;

/// An ACP agent for tests that reads images, can copy and roll back its session, and takes its session settings
/// from `agent-config.json` beside the script (an ACP `configOptions` array, none without the file). It logs, in
/// the session's folder, the method of every message it reads to `agent-calls.log`, each prompt as a JSON line
/// `{"text", "images": [base64 data], "files": [file names]}` to `agent-prompts.log`, and each setting it is
/// given as `<id>=<value>` to `agent-settings.log`, answering with its settings as they now are. Prompts are
/// numbered from 1: one ending with "wait" says "Working" and works until it is cancelled; any other is answered
/// with "Answer <n>". It ends with its input. Every line it reads and sends is written to `agent-transcript.log`
/// beside the script, for the report of a wait that runs out.
pub(super) const IMAGE_AGENT: &str = r#"import json, os, sys

transcript = os.path.join(os.path.dirname(os.path.abspath(__file__)), "agent-transcript.log")

def note(direction, line):
    with open(transcript, "a") as out:
        out.write("%s %s\n" % (direction, line))

def send(message):
    line = json.dumps(message)
    note("sent", line)
    sys.stdout.write(line + "\n")
    sys.stdout.flush()

def answer(request, result):
    send({"jsonrpc": "2.0", "id": request, "result": result})

def say(session, text):
    update = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}})

def log(name, line):
    with open(os.path.join(folder, name), "a") as out:
        out.write(line + "\n")

settings = os.path.join(os.path.dirname(os.path.abspath(__file__)), "agent-config.json")
config = []
if os.path.exists(settings):
    with open(settings) as source:
        config = json.load(source)
folder = os.getcwd()
prompts = 0
working = None
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    note("read", line)
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    if method == "session/new":
        folder = params.get("cwd") or folder
    if method is not None:
        log("agent-calls.log", method)
    if method == "session/cancel":
        if working is not None:
            answer(working, {"stopReason": "cancelled"})
            working = None
        continue
    if method is None or "id" not in message:
        continue
    request = message["id"]
    if method == "initialize":
        capabilities = {"loadSession": False, "promptCapabilities": {"image": True},
                        "sessionCapabilities": {"fork": {}}, "_meta": {"openidea": {"rollback": True}}}
        answer(request, {"protocolVersion": 1, "agentCapabilities": capabilities, "authMethods": []})
    elif method == "session/new":
        answer(request, {"sessionId": "image-session", "configOptions": config})
    elif method == "session/fork":
        answer(request, {"sessionId": "image-copy"})
    elif method == "session/load":
        answer(request, {})
    elif method == "_openidea/rollback":
        answer(request, {})
    elif method == "session/set_config_option":
        for option in config:
            if option.get("id") == params.get("configId"):
                option["currentValue"] = params.get("value")
        log("agent-settings.log", "%s=%s" % (params.get("configId"), params.get("value")))
        answer(request, {"configOptions": config})
    elif method == "session/prompt":
        prompts += 1
        session = params.get("sessionId", "image-session")
        blocks = params.get("prompt") or []
        text = " ".join(block.get("text", "") for block in blocks if block.get("type") == "text").strip()
        images = [block.get("data") for block in blocks if block.get("type") == "image"]
        files = [os.path.basename(block.get("uri") or (block.get("resource") or {}).get("uri", ""))
                 for block in blocks if block.get("type") in ("resource_link", "resource")]
        log("agent-prompts.log", json.dumps({"text": text, "images": images, "files": files}))
        if text.endswith("wait"):
            say(session, "Working")
            working = request
        else:
            say(session, "Answer %d" % prompts)
            answer(request, {"stopReason": "end_turn"})
    else:
        send({"jsonrpc": "2.0", "id": request, "error": {"code": -32601, "message": method}})
"#;

/// A `&'static str` selector built at run time.
pub(super) fn leak(selector: String) -> &'static str {
    Box::leak(selector.into_boxed_str())
}

/// A trusted project in an active window whose agent `fake` is [`IMAGE_AGENT`], with the session settings
/// `config` (an ACP `configOptions` array) when given.
pub(super) fn image_project<'a>(
    cx: &'a mut TestAppContext,
    home: &std::path::Path,
    config: Option<serde_json::Value>,
) -> AgentsFixture<'a> {
    install_fake_agent(home, cx);
    // The agent's file names this script; nothing has started it yet.
    std::fs::write(home.join("fake_agent.py"), IMAGE_AGENT).expect("the agent script is written");
    if let Some(config) = config {
        std::fs::write(home.join("agent-config.json"), config.to_string()).expect("the settings are written");
    }
    let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
    cx.update(|window, _| window.activate_window());
    let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
    wait_long(cx, "the agents to be listed", |cx| {
        store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
    });
    (folder, workbench, store, cx)
}

pub(super) fn task_ids(store: &Entity<AgentsStore>, cx: &mut VisualTestContext) -> Vec<String> {
    store.read_with(cx, |store, _| store.tasks().iter().map(|t| t.id.clone()).collect())
}

pub(super) fn docked_task(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Option<String> {
    let session = workbench.read_with(cx, |w, _| w.docked_agent_session().map(|(session, _)| session))?;
    Some(session.read_with(cx, |session, _| session.task().to_owned()))
}

pub(super) fn shows(workbench: &Entity<Workbench>, line: &str, cx: &mut VisualTestContext) -> bool {
    agent_conversation(workbench, cx).iter().any(|shown| shown.starts_with(line))
}

/// A PNG image named `name` whose base64 data is `data`.
pub(super) fn png(name: &str, data: &str) -> AgentImage {
    AgentImage { mime_type: "image/png".to_owned(), data: data.to_owned(), name: Some(name.to_owned()) }
}

/// `README.md` of the project in `folder`, as a composer references it.
pub(super) fn readme(folder: &tempfile::TempDir) -> FileRef {
    let root = folder.path().canonicalize().expect("the folder exists");
    FileRef { path: ide_core::ipc_path::to_ipc(&root.join("README.md")), rel: "README.md".to_owned() }
}

/// Starts a task from the New Task page with `text`, `files` and `images` (New Agent Task with the text, the
/// attachments added to the page's message, then Start) and waits for its conversation to dock; returns the new
/// task's id.
pub(super) fn start_with(
    workbench: &Entity<Workbench>,
    store: &Entity<AgentsStore>,
    text: &str,
    files: Vec<FileRef>,
    images: Vec<AgentImage>,
    cx: &mut VisualTestContext,
) -> String {
    let before = task_ids(store, cx);
    run_action(cx, "agents.newTask", Some(serde_json::json!(text)));
    wait_long(cx, "the New Task page", |cx| workbench.read_with(cx, |w, _| w.agents_home_page().is_some()));
    // Only the fake agent may ever start.
    assert_eq!(store.read_with(cx, |store, _| store.draft.provider.clone()), "fake");
    store.update(cx, |store, cx| {
        store.update_composer(HOME, |draft| {
            draft.files = files;
            draft.images = images;
        });
        cx.notify();
    });
    click_on(cx, "agents-home-start");
    wait_long(cx, "the new task", |cx| task_ids(store, cx).len() == before.len() + 1);
    let task = task_ids(store, cx).into_iter().find(|id| !before.contains(id)).expect("a new task");
    wait_long(cx, "the new task's conversation", |cx| docked_task(workbench, cx).as_deref() == Some(task.as_str()));
    task
}

/// Waits for the turn of `task` to end with `line` in the conversation.
pub(super) fn wait_for_answer(
    workbench: &Entity<Workbench>,
    store: &Entity<AgentsStore>,
    task: &str,
    line: &str,
    cx: &mut VisualTestContext,
) {
    wait_long(cx, line, |cx| {
        !agent_working(store, task, cx) && shows(workbench, line, cx) && turn_ended(store, task, cx)
    });
}

/// Whether the turn of the last message sent to `task` has its end recorded. The footers of the conversation show
/// that time (and the reply's buttons move right to make room), so a click on them waits for it.
pub(super) fn turn_ended(store: &Entity<AgentsStore>, task: &str, cx: &mut VisualTestContext) -> bool {
    store.read_with(cx, |store, _| {
        let entries = store.entries(task, None).unwrap_or_default();
        let last = entries.iter().rev().find_map(|entry| match entry {
            ide_agent::AgentEntry::User { steered: false, done_ms, .. } => Some(done_ms.is_some()),
            _ => None,
        });
        last.unwrap_or(false)
    })
}

/// The lines of the agent's log `name` in the project's folder.
pub(super) fn log_lines(folder: &tempfile::TempDir, name: &str) -> Vec<String> {
    let root = folder.path().canonicalize().expect("the folder exists");
    let log = std::fs::read_to_string(root.join(name)).unwrap_or_default();
    log.lines().map(str::to_owned).collect()
}

/// The prompts the agent was sent, in order, as `{"text", "images", "files"}`.
pub(super) fn agent_prompts(folder: &tempfile::TempDir) -> Vec<serde_json::Value> {
    log_lines(folder, "agent-prompts.log").iter().filter_map(|line| serde_json::from_str(line).ok()).collect()
}

/// The settings the agent was given, in order, as `<id>=<value>`.
pub(super) fn agent_settings(folder: &tempfile::TempDir) -> Vec<String> {
    log_lines(folder, "agent-settings.log")
}

/// Confirms the fresh session that running the first message again asks for.
pub(super) fn start_fresh(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
    let dialog = wait_for_prompt(workbench, cx);
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.title().to_owned()), "Start a Fresh Agent Session");
    click_on(cx, "prompt-button-ok");
}
