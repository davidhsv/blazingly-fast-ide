# Integrating the Agents feature into the workbench

New file, not compiled yet: `app-gpui/src/workbench/agents.rs`. The four view files (`agents/{markdown_view,task_list_view,session_view,composer_view}.rs`) were not edited. Every anchor below is quoted from the files as they are now; line numbers are left out because `workbench/mod.rs` is moving (other drafts land in it too).

**Test path: (i), a real agent process through configuration.** `ide-host` builds each project's `AgentManager` with `custom_agents_dir: <config>/agents` (`crates/ide-host/src/project.rs`), `provider::load` turns every `<config>/agents/<id>.toml` into an ACP agent (`crates/ide-agent/src/provider.rs`, `CustomAgent { name, command, args, env }`, driver ACP by default), and an ACP agent is spawned with `Framing::Lines` (newline-delimited JSON-RPC, `crates/ide-agent/src/lib.rs` `Peer::spawn_framed(..., Framing::Lines)`). So the tests write a Python script speaking ACP v1 (the protocol `crates/ide-agent/src/tests.rs` `FakeAgent` speaks in memory: `initialize`, `session/new`, `session/prompt` with `session/update` chunks, `session/request_permission`, `session/cancel`) and a `fake.toml` naming it under the test's configuration folder before the project opens, the same trick `app-gpui/src/editor/intelligence.rs` `fake_server` uses for a language server. The tests also store the New Task draft with `"provider": "fake"`, so nothing ever starts an agent installed on the machine (the default draft provider is `claude`), and the New Task page refuses an agent that is not listed and installed. The tests need `python3` on `PATH`, as the editor's fake language server does; they use no network.

## 1. `app-gpui/src/agents/mod.rs`

Add these four lines, keeping the list alphabetical (after `pub mod composer;`, after `pub mod markdown_images;`, after `pub mod session;`, after `pub mod task_list;`):

```rust
pub mod composer_view;
pub mod markdown_view;
pub mod session_view;
pub mod task_list_view;
```

## 2. `app-gpui/src/workbench/mod.rs`

### 2a. The module

Before `mod bookmarks_window;` (the first `mod` line) add:

```rust
mod agents;
```

### 2b. The tool window

In `const TOOL_WINDOWS`, add the Agents window after `HIERARCHY_TOOL_WINDOW,` and add one to the array length. Today the declaration reads `const TOOL_WINDOWS: [&str; 15] = [` with 15 entries, so it becomes `[&str; 16]`; if another draft added an entry meanwhile, count the entries again and use that count.

```rust
    crate::agents::model::TOOL_WINDOW,
```

`assets/gen/contributions.json` already declares `agents` (title Agents, icon `toolwindow-agents`, anchor right, order 0.5), so `tool_window_declaration` resolves it and `register_actions` registers `toolwindow.agents` from this list.

### 2c. The `Workbench` field

In `pub struct Workbench`, after

```rust
    /// Problems, Find / Show Usages, the hierarchies and the language servers of the project.
    code_insight: code_insight::CodeInsight,
```

add:

```rust
    /// The project's agent tasks: their store, the subscription to their events and the conversation
    /// docked in the Agents tool window.
    agents: Option<agents::AgentsSlot>,
```

### 2d. Its initial value

In `Workbench::new`, in `let mut workbench = Self { … }`, after `code_insight,` add:

```rust
            agents: None,
```

### 2e. Window activation

In `Workbench::new`, in the `cx.observe_window_activation` closure, replace

```rust
            workbench.balloon_view.window_active = window.is_window_active();
            workbench.sync_notifications(cx);
```

with

```rust
            workbench.balloon_view.window_active = window.is_window_active();
            workbench.sync_notifications(cx);
            // A conversation on screen counts as read only while the window is active.
            workbench.agents_window_active(window.is_window_active(), cx);
```

### 2f. The actions

In `pub fn register_actions(cx: &mut App)`, after `code_insight::register_actions(registry);` add:

```rust
    agents::register_actions(registry);
```

### 2g. The mount arm

In `fn mount_panels`, in `let panel = match id.as_str() {`, before `FIND_TOOL_WINDOW => {` add:

```rust
                crate::agents::model::TOOL_WINDOW => match self.mount_agents_panel(window, cx) {
                    Some(panel) => panel,
                    None => continue,
                },
```

### 2h. Following the tool window's visibility

At the end of `fn mount_panels`, replace

```rust
        self.sync_services_window(cx);
        self.sync_structure_window(cx);
        self.sync_todo_window(cx);
        self.sync_code_insight(cx);
    }
```

with

```rust
        self.sync_services_window(cx);
        self.sync_structure_window(cx);
        self.sync_todo_window(cx);
        self.sync_code_insight(cx);
        self.sync_agents_window(cx);
    }
```

### 2i. The context key

In `fn publish_context(&mut self, window: &mut Window, cx: &mut App)`, after `self.publish_vcs_context(window, cx);` add:

```rust
        self.publish_agents_context(window, cx);
```

### 2j. Trust changes

In `pub fn backend_event`, before the final `_ => return,` of the `match &event.event` add (no other arm handles `ProjectTrustChanged` today; if another draft added one, call this from it):

```rust
            // The project's agents are listed again, with or without the agents themselves.
            IdeEvent::ProjectTrustChanged { trusted, .. } => {
                self.agents_trust_changed(*trusted, cx);
                return;
            }
```

### 2k. The Open / Review buttons of the agents' notifications

In `fn run_notification_action`, before

```rust
            EntryAction::Own(own) if own.command.starts_with("vcs.deleted.") => {
```

add:

```rust
            // An agent task's notification: its button opens the task it names.
            EntryAction::Own(own) if own.command.starts_with("agents.openTask:") => {
                if let Some((_, task)) = own.command.split_once(':') {
                    self.open_agent_task(task, cx);
                }
            }
```

`agents.rs` rewrites the store's `agents.openTask` command of a notification to `agents.openTask:<task id>` (taken from the notification's key `agent:<id>`) when it raises it, because `run_notification_action` passes neither an argument nor the key to a registered action.

### 2l. Resets on project close and open

In `pub fn close_project`, replace

```rust
        self.drop_git_log();
        self.reset_code_insight(cx);
        // The project's servers and runs stop in the background: this is the UI thread.
```

with

```rust
        self.drop_git_log();
        self.reset_code_insight(cx);
        self.drop_agents(cx);
        // The project's servers and runs stop in the background: this is the UI thread.
```

In `pub fn open_project`, replace

```rust
        self.reset_code_insight(cx);
        self.files = Default::default();
```

with

```rust
        self.reset_code_insight(cx);
        self.drop_agents(cx);
        self.files = Default::default();
```

### 2m. The project-opened hook

In `pub fn open_project`, in the `cx.spawn_in` answer, replace

```rust
                workbench.mount_vcs(window, cx);
```

with

```rust
                workbench.mount_vcs(window, cx);
                workbench.mount_agents(window, cx);
```

It must stay before the `workbench.with_layout(window, cx, |layout| { … })` that registers the tool windows: a layout restored with the Agents window open mounts its panel there, and the panel needs the slot.

### 2n. Dialog layer

None to add: the confirmations, the Rename prompt and the error box go through `show_prompt_dialog` (`branches.rs`), whose dialog layer `render` already draws.

## 3. What `agents.rs` needs from compiled files (already there; listed to check)

- `services_window::on_workbench` is `pub(super)`; `branches::show_prompt_dialog` is `pub(super)`. Both are reached from the child module `workbench::agents`.
- `Workbench` fields `label`, `agents` and the methods `with_layout`, `notify`, `show_menu`, `open_project_file`, `open_project_file_at`, `project_root`, `editor`, `is_tool_window_visible` are reached as a child module reaches its parent's private items.

## 4. End-to-end tests for `workbench/mod.rs` `mod tests`

Paste after `fn run_action(…)` (they use `start`, `trusted_project`, `wait_long`, `click_on` and `run_action` of that module). Run `cargo fmt` after pasting.

```rust
    /// An ACP agent for tests: a Python script speaking newline-delimited JSON-RPC on stdio. A prompt
    /// is answered with "Hello from the fake agent" in two chunks; one mentioning "permission" first
    /// asks to run `cargo test` and then says which option it was given; one mentioning "wait" says
    /// hello and works until it is cancelled. It ends with its input.
    const FAKE_AGENT: &str = r#"import json, sys

def send(message):
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()

def answer(request, result):
    send({"jsonrpc": "2.0", "id": request, "result": result})

def say(session, text):
    update = {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}}
    send({"jsonrpc": "2.0", "method": "session/update", "params": {"sessionId": session, "update": update}})

asked = {}
working = None
next_request = 1000
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    if method is None:
        turn = asked.pop(message.get("id"), None)
        if turn is not None:
            outcome = (message.get("result") or {}).get("outcome") or {}
            say(turn[1], "You chose " + outcome.get("optionId", "nothing"))
            answer(turn[0], {"stopReason": "end_turn"})
        continue
    if method == "session/cancel":
        if working is not None:
            answer(working, {"stopReason": "cancelled"})
            working = None
        continue
    if "id" not in message:
        continue
    request = message["id"]
    if method == "initialize":
        answer(request, {"protocolVersion": 1, "agentCapabilities": {"loadSession": False}, "authMethods": []})
    elif method == "session/new":
        answer(request, {"sessionId": "fake-session"})
    elif method == "session/prompt":
        session = params.get("sessionId", "fake-session")
        blocks = params.get("prompt") or []
        text = " ".join(block.get("text", "") for block in blocks if block.get("type") == "text")
        if "permission" in text:
            say(session, "I need to run the tests.")
            next_request += 1
            asked[next_request] = (request, session)
            send({"jsonrpc": "2.0", "id": next_request, "method": "session/request_permission", "params": {
                "sessionId": session,
                "toolCall": {"toolCallId": "run-tests", "title": "Run cargo test", "kind": "execute"},
                "options": [{"optionId": "yes", "name": "Allow", "kind": "allow_once"},
                            {"optionId": "no", "name": "Reject", "kind": "reject_once"}]}})
        else:
            say(session, "Hello ")
            say(session, "from the fake agent")
            if "wait" in text:
                working = request
            else:
                answer(request, {"stopReason": "end_turn"})
    else:
        send({"jsonrpc": "2.0", "id": request, "error": {"code": -32601, "message": method}})
"#;

    type AgentsFixture<'a> =
        (tempfile::TempDir, Entity<Workbench>, Entity<crate::agents::store::AgentsStore>, &'a mut VisualTestContext);

    /// Installs the fake agent as the agent `fake` of the configuration under `home`, as a user writes
    /// `<config>/agents/<id>.toml`, and makes it the New Task page's agent: no test reaches an agent
    /// installed on the machine.
    fn install_fake_agent(home: &std::path::Path, cx: &mut TestAppContext) {
        let script = home.join("fake_agent.py");
        std::fs::write(&script, FAKE_AGENT).expect("the agent script is written");
        let agents = AppDirs::under(home).config.join("agents");
        std::fs::create_dir_all(&agents).expect("the agents folder is made");
        let file = format!("name = \"Fake Agent\"\ncommand = \"python3\"\nargs = [{:?}]\n", script.to_string_lossy());
        std::fs::write(agents.join("fake.toml"), file).expect("the agent file is written");
        let draft = r#"{"provider":"fake"}"#.to_owned();
        cx.update(|cx| cx.global_mut::<UiStorage>().set(crate::agents::store::DRAFT_KEY, draft));
    }

    /// A trusted project in an active window, with the fake agent listed among its agents.
    fn agents_project<'a>(cx: &'a mut TestAppContext, home: &std::path::Path) -> AgentsFixture<'a> {
        install_fake_agent(home, cx);
        let (folder, _root, workbench, cx) = trusted_project(cx, &[("README.md", "A project for agents.\n")]);
        cx.update(|window, _| window.activate_window());
        let store = workbench.read_with(cx, |w, _| w.agents_store()).expect("the project has its agents");
        wait_long(cx, "the agents to be listed", |cx| {
            store.read_with(cx, |store, _| store.loaded && store.provider("fake").is_some_and(|p| p.installed))
        });
        (folder, workbench, store, cx)
    }

    /// Starts a task with `prompt` from the New Task page of the Agents tool window (New Agent Task
    /// with the prompt as its argument, then Start) and waits for its conversation to dock there;
    /// returns the task's id.
    fn start_agent_task(
        workbench: &Entity<Workbench>,
        store: &Entity<crate::agents::store::AgentsStore>,
        prompt: &str,
        cx: &mut VisualTestContext,
    ) -> String {
        run_action(cx, "agents.newTask", Some(serde_json::json!(prompt)));
        wait_long(cx, "the New Task page", |cx| workbench.read_with(cx, |w, _| w.agents_home_page().is_some()));
        assert!(workbench.read_with(cx, |w, _| w.is_tool_window_visible(crate::agents::model::TOOL_WINDOW)));
        let page = workbench.read_with(cx, |w, _| w.agents_home_page()).expect("the New Task page shows");
        assert_eq!(page.read_with(cx, |page, cx| page.field().read(cx).text().to_owned()), prompt);
        assert!(page.read_with(cx, |page, cx| page.can_start(cx)));
        click_on(cx, "agents-home-start");
        wait_long(cx, "the task's conversation", |cx| {
            workbench.read_with(cx, |w, _| w.docked_agent_session().is_some())
        });
        assert!(page.read_with(cx, |page, _| page.error().is_none()));
        let tasks: Vec<String> = store.read_with(cx, |store, _| store.tasks().iter().map(|t| t.id.clone()).collect());
        assert_eq!(tasks.len(), 1, "{tasks:?}");
        tasks[0].clone()
    }

    /// The docked conversation, a line per row (`SessionView::lines`).
    fn agent_conversation(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) -> Vec<String> {
        let Some((session, _)) = workbench.read_with(cx, |w, _| w.docked_agent_session()) else { return Vec::new() };
        session.update(cx, |session, cx| session.lines(cx))
    }

    fn agent_status(
        store: &Entity<crate::agents::store::AgentsStore>,
        task: &str,
        cx: &mut VisualTestContext,
    ) -> Option<ide_agent::AgentStatus> {
        store.read_with(cx, |store, _| store.task(task).map(|t| t.status))
    }

    fn agent_working(
        store: &Entity<crate::agents::store::AgentsStore>,
        task: &str,
        cx: &mut VisualTestContext,
    ) -> bool {
        use ide_agent::AgentStatus;
        matches!(agent_status(store, task, cx), Some(AgentStatus::Running | AgentStatus::Starting))
    }

    /// A task started from the Agents tool window shows the user's message and the agent's reply while
    /// the agent still works on the turn, with Stop in its composer; Stop cancels the turn, and the
    /// task list names the task with its agent.
    #[gpui_kit::test]
    async fn a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn(
        cx: &mut TestAppContext,
    ) {
        let home = start(cx);
        let (_folder, workbench, store, cx) = agents_project(cx, home.path());
        let task = start_agent_task(&workbench, &store, "Say hello and wait", cx);
        wait_long(cx, "the agent's reply", |cx| {
            agent_conversation(&workbench, cx).iter().any(|line| line.starts_with("agent: Hello from the fake agent"))
        });
        let lines = agent_conversation(&workbench, cx);
        assert!(lines.iter().any(|line| line.starts_with("you: Say hello and wait")), "{lines:?}");
        assert!(agent_working(&store, &task, cx));
        let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
        assert!(composer.read_with(cx, |composer, cx| composer.buttons(cx)).contains(&"Stop".to_owned()));

        click_on(cx, "composer-stop");
        wait_long(cx, "the turn to stop", |cx| !agent_working(&store, &task, cx));
        assert!(!composer.read_with(cx, |composer, cx| composer.buttons(cx)).contains(&"Stop".to_owned()));
        let list = workbench.read_with(cx, |w, _| w.agents_list()).expect("the tool window has its view");
        let rows = list.read_with(cx, |list, cx| list.rows(cx));
        assert!(rows.iter().any(|row| row.contains(" | Say hello and wait | Fake Agent | ")), "{rows:?}");
        workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    }

    /// A permission request shows in the conversation with its answers while the task waits for the
    /// user; the answer clicked reaches the agent, which carries on with it.
    #[gpui_kit::test]
    async fn a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent(cx: &mut TestAppContext) {
        use ide_agent::AgentStatus;
        let home = start(cx);
        let (_folder, workbench, store, cx) = agents_project(cx, home.path());
        let task = start_agent_task(&workbench, &store, "Run the tests, permission please", cx);
        wait_long(cx, "the permission request", |cx| {
            agent_conversation(&workbench, cx).iter().any(|line| line.starts_with("permission: "))
        });
        let lines = agent_conversation(&workbench, cx);
        let request = lines.iter().find(|line| line.starts_with("permission: ")).expect("the request is listed");
        assert!(request.contains("cargo test"), "{lines:?}");
        wait_long(cx, "the task to wait for the user", |cx| {
            agent_status(&store, &task, cx) == Some(AgentStatus::NeedsInput)
        });

        // The backend numbers permission requests from 1; Allow is the request's first answer.
        click_on(cx, "session-permission-1-0");
        wait_long(cx, "the agent to carry on", |cx| {
            agent_conversation(&workbench, cx).iter().any(|line| line.starts_with("agent: You chose yes"))
        });
        wait_long(cx, "the turn to end", |cx| !agent_working(&store, &task, cx));
        assert_ne!(agent_status(&store, &task, cx), Some(AgentStatus::NeedsInput));
        workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    }

    /// A reply that arrives while the Agents tool window is hidden leaves its task unread, with the
    /// unread mark in the list and the tool window's badge; Archive moves the task to the archived
    /// group, and Delete, once confirmed, removes it and its conversation.
    #[gpui_kit::test]
    async fn an_unseen_reply_marks_its_task_unread_and_archive_and_delete_remove_it(cx: &mut TestAppContext) {
        use crate::agents::model::TOOL_WINDOW;
        use ide_agent::AgentStatus;
        let home = start(cx);
        let (_folder, workbench, store, cx) = agents_project(cx, home.path());
        let task = start_agent_task(&workbench, &store, "Say hello", cx);
        wait_long(cx, "the first reply", |cx| {
            !agent_working(&store, &task, cx)
                && agent_conversation(&workbench, cx).iter().any(|line| line.starts_with("agent: Hello"))
        });
        assert!(!store.read_with(cx, |store, _| store.is_unread(&task)));

        workbench.update_in(cx, |workbench, window, cx| workbench.with_layout(window, cx, |l| l.hide(TOOL_WINDOW)));
        let (_, composer) = workbench.read_with(cx, |w, _| w.docked_agent_session()).expect("the conversation docks");
        store.update(cx, |store, cx| {
            store.update_composer(&task, |draft| draft.text = "Say hello again".to_owned());
            cx.notify();
        });
        composer.update(cx, |composer, cx| composer.send(false, cx));
        wait_long(cx, "the unseen reply", |cx| {
            !agent_working(&store, &task, cx) && store.read_with(cx, |store, _| store.is_unread(&task))
        });
        let list = workbench.read_with(cx, |w, _| w.agents_list()).expect("the tool window has its view");
        let rows = list.read_with(cx, |list, cx| list.rows(cx));
        assert!(
            rows.iter().any(|row| row.starts_with("Ready | Say hello | Fake Agent | ") && row.ends_with(" | unread")),
            "{rows:?}"
        );
        assert_eq!(store.read_with(cx, |store, _| store.badge()), Some(1));

        run_action(cx, "agents.archiveTasks", Some(serde_json::json!([task])));
        wait_long(cx, "the task to be archived", |cx| agent_status(&store, &task, cx) == Some(AgentStatus::Archived));
        assert!(!store.read_with(cx, |store, _| store.is_unread(&task)));
        let rows = list.read_with(cx, |list, cx| list.rows(cx));
        assert!(rows.iter().any(|row| row.contains("Archived (1)")), "{rows:?}");

        run_action(cx, "agents.deleteTasks", Some(serde_json::json!([task])));
        click_on(cx, "prompt-button-ok");
        wait_long(cx, "the task to be deleted", |cx| store.read_with(cx, |store, _| store.tasks().is_empty()));
        assert_eq!(list.read_with(cx, |list, cx| list.empty_text(cx)), Some("No agent tasks yet".to_owned()));
        assert!(workbench.read_with(cx, |w, _| w.docked_agent_session().is_none()));
        workbench.update_in(cx, |workbench, window, cx| workbench.close_project(window, cx));
    }
```

## 5. Not wired, and why

- **An editor tab for a conversation or the New Task page.** The editor area holds file tabs only (`Content::Project.editors: HashMap<String, Entity<Editor>>`, `EditorInput { kind: "file", … }`). Every conversation docks in the tool window instead: `OpenSession` / `OpenHome` set `store.docked` / `store.home_open` and show the tool window, `CloseTab` clears that placement in Editor location and does nothing in ToolWindow location, `TabTitlesChanged` does nothing. **When view tabs exist:** carry out `OpenSession { task, title }` as a tab keyed `session_key(task)` holding a `SessionView::new(store, task, false, cx)` with a `ComposerView` in its slot (the same pair `new_docked_session` builds, `docked: false`), `OpenHome` as a tab keyed `HOME_KEY` holding `NewTaskPage`, `CloseTab { key }` as closing that tab, `TabTitlesChanged` as re-reading the titles from `store.task(id).title`, `agent_editors()` from the real tabs (`AgentTab::Home` / `Session(id)`), and drop the Editor-location branches of `dock_agent_view` and `close_agent_placement`. `agents.openTaskInEditor`, `agents.undockTask` and `agents.useEditor` then mean what they say; today they keep the conversation docked.
- **The New Task page is minimal** (`NewTaskPage` in `agents.rs`): the description (one line), the agent (a menu of the listed agents) and Start. Not built: the project of a workspace (`TaskProjects` is passed as `None`, so the window's project is used), isolation and base branch, the config pickers, @ / / completions, ↑ history, the probe line, "More Agents…" in the agent menu.
- **Find in Conversation** (`agents.findInConversation`): there is no find bar for a conversation; `SessionView::set_finding` alone would only unfold rows.
- **The Agents status bar item** (`store.status_item()`): no status bar widget for it.
- **Queued messages, background tasks, the changes pane**: the views draw none of them (`edit_queued`, `SendQueuedNow`, `StopBackground` are carried out when asked, but nothing asks).
- **Settings changes reaching an open conversation**: `SessionView::set_agent_settings` is given the settings when the docked conversation is built, not on `IdeEvent::SettingsChanged`.
- **Dragging a session tab onto the tool window** (`tab_over` / `tab_dropped`): there are no session tabs to drag.
- `agents.openSampleConversations`: a development-only action of the performance harness.

## 6. Assumptions I could not confirm by reading

1. **The delete confirmation's checkbox starts unchecked.** `ConfirmChecked { checked: true }` (worktree tasks) has no `PromptDialog` constructor: `PromptDialog::ask` starts its checkbox unchecked (`checkbox: request.checkbox.map(|label| (label, false))`, `vcs/prompt_dialogs.rs`) and takes `&'static str` button labels, so the dialog's buttons are the English "Delete" / "Cancel" (the dialog translates labels, as for every other caller). To match the webview, add to `vcs_ops::dialogs::AskRequest` a `pub checked: bool` (used as the initial state in `PromptDialog::ask`), and set `checked: request.checked` in `agents.rs` `confirm_batch`.
2. **The fake agent's protocol.** The script answers `initialize` with a v1 response (no `info`, so `Negotiated::from_initialize` takes the v1 branch), `session/new` with a session id, and every other request with `-32601`, which `FakeAgent` in `crates/ide-agent/src/tests.rs` also does. Check `crates/ide-agent/src/lib.rs` `Task::run` (from `let init = peer.initialize(...)`) for a request whose failure aborts the start, and that the user's own message becomes an `AgentEntry::User` without the agent echoing it (the script sends no `user_message_chunk`).
3. **Permission ids start at 1 per project** (`next_permission: AtomicU32::new(1)`, `crates/ide-agent/src/lib.rs`), so the first answer of the first request is the selector `session-permission-1-0` (`session_view.rs` `permission_element`).
4. **The task's title is its prompt** for a short one-line prompt (`title_of` in `crates/ide-agent/src/lib.rs`); the row assertions use "Say hello" and "Say hello and wait".
5. **The streamed chunks merge into one reply** ("Hello " + "from the fake agent"), as `FakeAgent::say` relies on; check the timeline merge in `crates/ide-agent/src/timeline.rs`.
6. **A cancelled turn leaves the task `Ready`** (not `Failed`): the tests only assert that it is no longer `Running` / `Starting`.
7. **The Archived group's row reads `… Archived (1)`** (`task_list.rs` group label, collapsed or not).
8. **The fake agent process ends with the project.** `close_project` calls `host.close_project_detached`; the script also exits when its input closes.
9. **`UiStorage::set(&mut self, key: &str, value: String)`** and `cx.global_mut::<UiStorage>()` in a test's `cx.update` (the test's `start` sets a default `UiStorage`).
10. **`AnyWindowHandle::update` from `cx.spawn`** (the `in_agents_window` helper) follows `workbench/conflicts.rs` `in_window` and `workbench/database.rs`, which are not compiled yet either; the API (`gpui-pre` `window.rs` `AnyWindowHandle::update<C: AppContext>`) was read.
11. **The subscription's events after `connected`.** `AgentManager::subscribe` replaces the project's single sink, so one subscription per window; events emitted between `subscribe` and the answer wait in the channel and are applied after `connected`, in order.
12. **Entries that arrive before the Create answer** find no conversation in the store and are dropped (`AgentsStore::on_event`, `Entry` arm); `agents.rs` reads the conversation (`LoadEntries`) once the task is created. An entry emitted between that read and its answer could be overwritten by the older read until the next entry arrives with a gap (`upsert_entry` → `LoadEntries`).
13. **`file.settings` takes the page id as its argument** (`agents.addAgent` passes `REGISTRY_PAGE`); not checked in the Rust handler of `file.settings`.

## 7. Places most likely not to compile

- `agents_step`: `cx.update_default_global::<Localization, _>(|localization, cx| store.update(cx, …))` capturing `&mut slot.models` while `cx` is borrowed (same shape as `services_window.rs` `carry_out_services`).
- `sync_agents_window`: `slot.docked = docked.as_deref().map(|task| Self::new_docked_session(&store, task, settings, cx))` with `slot` a `&mut` into `self.agents` and `cx` captured by the closure.
- `agent_request_finished`: `self.agents_error(…)` after `let Some(slot) = self.agents.as_mut()` in the same match (relies on NLL ending the borrow of `slot`).
- `cx.subscribe(&session, Self::agents_session_event)`: a method path as the callback, with `Entity<SessionView>` taken by value.
- `registry.handle` loops over `fn(Vec<String>) -> TaskCommand` and `fn(ReplyTarget) -> MessageAction` built from enum variant constructors.
- `agents_backend`'s two generic closures: the answer closures' parameter types are inferred from the work closure's `F::Output`.
- `NewTaskPage::render`'s `cx.listener` inside `.when(enabled, …)`.
