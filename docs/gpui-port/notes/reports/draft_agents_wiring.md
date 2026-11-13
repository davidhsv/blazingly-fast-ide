I've written `agents.rs` and the integration notes. Nothing has been compiled or run: cargo was off limits.

## 1. Files

- **Written:** `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/agents.rs`.
- **Edited:** none. The four view files were not changed; the wiring needs nothing more from them.

**The slot (`AgentsSlot`, one per project window):**

- `Entity<AgentsStore>`, built from `UiStorage` (`DRAFT_KEY`, `HISTORY_KEY`, `SESSION_LOCATION_KEY`).
- `ReplyActions` and `TaskBatches`.
- The window handle, the trusted flag and the `TaskListView`.
- The docked `SessionView`, with a `ComposerView` in its slot.
- A minimal New Task page, a ticket → purpose map, and the subscription task.
- An observer on the store that keeps the docked view in step with it.

**Methods:**

- **Lifecycle:** `mount_agents`, `drop_agents`, `agents_trust_changed`, `agents_window_active`.
- **Tool window:** `mount_agents_panel`, `sync_agents_window`, `publish_agents_context`, `open_agent_task`.
- **Test accessors:** `agents_store`, `docked_agent_session`, `agents_home_page`, `agents_list`.

**Connecting:** `store.connect(trusted)` returns `Subscribe`. One task per generation then:

1. Calls `ops::agent::subscribe` with an `Emit` that feeds an unbounded channel, and calls `providers` when the effect asks for it.
2. Passes the answer to `store.connected`.
3. Drains the channel in batches into `store.on_event`.

Dropping the task (on reconnect or close) ends the reader.

**Effects carried out:**

- **Every `AgentCall`** goes to its `ide_host::ops::agent::*` function, off the main thread.
  - Answers go to all ticket holders: `store.call_answered`, `TaskBatches::answered`, `SessionView::answered` and `ComposerView::call_answered`.
  - A failed call without a ticket raises a notification (send, setting, background task, queue, rename) or is logged.
- **`Create`:** `task_created` runs, then a `LoadEntries`, because entries that arrive before the answer are dropped.
- **Data and storage:** `LoadEntries` / `RefreshChanges` / `RefreshProviders` call the backend; `Save` writes to `UiStorage`.
- **Timers:** `ScheduleChanges` and `WakeAt` are timers that check the project hasn't changed before firing.
- **Notifications:** `Notify` rewrites the Open / Review command to `agents.openTask:<id>`.
- **Dialogs:** `Confirm` goes through `PromptDialog::confirm` (reply actions first, else the store); a closed dialog declines. Batch confirms use `PromptDialog::ask` with its checkbox, and batch errors show in a message box.
- **`Finished`:** goes to `ReplyActions::finished`, then by purpose:
  - an edited message → `SessionView::edit_sent`;
  - a send or fork failure → a notification;
  - a Create → shown on the New Task page.
- **Placement:** `OpenSession` / `OpenHome` dock in the tool window (they set `store.docked` / `home_open`, then show it). `CloseTab` clears that placement only in Editor location. `ShowToolWindow` shows the window; `TabTitlesChanged` does nothing.

**Events handled:**

- **Task list:** `OpenTask`, `DockTask`, `NewTask`, `SetLocation`, and `ShowMenu`, which opens the workbench menu and runs the chosen `TaskCommand` (Open, Rename prompt, Open in Terminal, Fork, and the Archive / Unarchive / Delete batches).
- **Session:** `Effects`; every `MessageAction` (to `ReplyActions` or the store; Send Again sends the message again directly, as the webview does); `Link` (browser or file).
- **Composer:** `Effects`.
- **New Task page:** Start, and choosing the agent.

**Actions:** `agents.showToolWindow`, `newTask`, `useToolWindow`, `useEditor`, `openTask`, `dockTask`, `undockTask`, `openTaskInEditor`, `renameTask`, `forkTask`, `archiveTasks`, `unarchiveTasks`, `deleteTasks`, `jumpToLatest`, `addAgent`, `sendToAgent` (editor selection with its 1-based line range, or a list of Project view paths), `copyReplyMarkdown`, `quoteReply`, `regenerateReply`, `forkReply`, `retractMessage`.

**Context key:** `agentSessionFocus`.

## 2. Drafts file

`/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/agents_integration.md`, by section:

- **Test path:** (i). A Python ACP agent in `<config>/agents/fake.toml` is reachable through `custom_agents_dir` and is spawned with newline-delimited JSON. The tests store the draft provider as `fake`, so the default `claude` never starts, and Start refuses an agent that isn't listed and installed.
- **§1:** the four `pub mod` lines for `agents/mod.rs`.
- **§2 (`workbench/mod.rs`, anchored by quoted text):**
  - `mod agents;`;
  - `TOOL_WINDOWS` 15 → 16 (re-count before pasting; other drafts are landing);
  - the field and its initialiser;
  - the window-activation line and the `register_actions` call;
  - the mount arm, plus a sync line at the end of `mount_panels`;
  - the `publish_context` line and a `ProjectTrustChanged` arm;
  - a notification arm for `agents.openTask:`;
  - `drop_agents` in close and open, and `mount_agents` after `mount_vcs`;
  - no new dialog layer.
- **§3:** the `pub(super)` items `agents.rs` relies on.
- **§4:** three end-to-end tests, complete Rust:
  - `a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn`
  - `a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent`
  - `an_unseen_reply_marks_its_task_unread_and_archive_and_delete_remove_it`
- **§5:** what isn't wired, including what changes once view tabs exist.
- **§6:** 13 assumptions, each with the file to check.
- **§7:** the places least likely to compile.

## 3. Not wired

- **Editor tabs for a conversation or New Task:** the editor area holds file tabs only, so everything docks in the tool window. The notes say exactly what changes once view tabs exist.
- **The New Task page is minimal:** description, agent menu and Start only. There are no workspace member, isolation, base branch, config pickers, completions or history, because the webview's New Task page was never built in Rust.
- **Find in Conversation:** there is no find bar for a conversation.
- **The Agents status bar item:** there is no widget for it.
- **Queued messages, background tasks, the changes pane:** the views draw none of them.
- **Settings changes reaching an open conversation:** the settings are given only when the conversation is built.
- **Dragging session tabs onto the tool window:** there are no session tabs.
- **`agents.openSampleConversations`:** development-only.

## 4. Most likely wrong or not compiling

**Assumptions:**

- **Delete confirmation:** its "remove worktree" checkbox starts unchecked, because `PromptDialog::ask` can't start it checked. The exact addition to `AskRequest` is in the notes.
- **Fake agent protocol:**
  - The v1 `initialize` reply is accepted.
  - Answering every unknown request with `-32601` does not abort the start.
  - Permission ids start at 1, so the first button is `session-permission-1-0`.
  - The two reply chunks merge into one message.
- **Fake agent end states:** a cancelled turn leaves the task `Ready`, and the archived group reads "Archived (1)".
- **`file.settings`:** assumed to take the page id as its argument.

**Compile risks:**

- **Borrows:**
  - `agents_step` holds a `&mut` to the models across `update_default_global` + `store.update`.
  - `sync_agents_window` builds the docked session inside an `Option::map` while the slot is mutably borrowed.
  - `agent_request_finished` calls `self.agents_error` after borrowing the slot.
- **Inference and method paths:**
  - `cx.subscribe` takes method paths with `Entity` by value.
  - Function-pointer arrays are built from enum variant constructors.
  - The answer closures of `agents_backend` infer their types.
- **Lifecycle risks:**
  - `in_agents_window` copies the stored-window-handle pattern from `conflicts.rs`, which isn't compiled either.
  - `agents_window_active` is dead code until its draft line is pasted.
