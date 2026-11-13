Parts A and B are both integrated and green. On every one of the five final whole-suite runs I got `test result: ok. 6102 passed; 0 failed; 1 ignored` (6019 before), and no fake agent or adapter process was left behind. `cargo clippy --all-targets -- -D warnings` is clean. Nothing is `#[ignore]`d. Two fixes mattered most:

- **Debug start crash:** Debug panicked on every start from an action.
- **Agent keys in `Debug` output:** agent API keys could appear in `Debug` output.

One backend race in `crates/ide-dap` remains, which I couldn't edit (§6).

## 1. Per step

**A1 (four `pub mod` lines):** 0 compile errors.

- Results: `composer_view` 8 passed, `markdown_view` 9, `session_view` 24 (23 + 1 failure fixed), `task_list_view` 16.
- Bug fixed in `agents/session_view.rs` `render`: the Jump to Latest flag was read before `sync_scroll` fed the last frame's scroll to `Follow`. The frame that stops following therefore drew no button. The flag is now read again after the sync. `agents::session_view` then passed 10 of 10 runs.

**A2 (wiring):** 0 compile errors; whole suite green at once (6076).

- Clippy: 1 fix (`question_mark` in `markdown_view.rs` `task_item`).
- Bug fixed: `AgentsStore` (`agents/store.rs`) and `BuiltinAgents` (`agents/builtin_agents.rs`) derived `Debug` over `AgentProvider`, whose `env` holds the agents' API keys. Both now have a redacting `Debug` (`ProvidersDebug` prints id, program, installed, env names only). Two unit tests prove it.
- Stale comments in `workbench/agents.rs` ("the editor area holds file tabs only") corrected.

**A3 (e2e):** the three drafted tests passed as written, 10 of 10 runs each.

- Added before Start in `start_agent_task`: an assertion that the draft's provider is `fake`.
- Added to the fake agent: an `[env] FAKE_AGENT_API_KEY` in `fake.toml`, and a "tool" prompt branch that reports a finished `tool_call`.

**B1 (`breakpoints_view`, `driver`):** 5 real compile errors (12 error lines), all in `driver.rs`.

- Missing `AppContext` import.
- Two answer closures whose `Result<Vec<…>>` type wasn't inferred (cascaded into unsized `[T]` errors); annotated.
- `LocatedTools` not inferred; annotated.
- `PhpXdebug` not inferred and not nameable (`ide_sdk` is not a dependency); the work closure now maps it to `(String, String, bool)`.
- Results: `breakpoints_view` 6 passed, `driver` 3.

**B2 (wiring):** 2 compile errors, both non-exhaustive `EditorEvent` matches: `intelligence.rs` (in the notes) and `tab_views.rs` `MarkdownFeed::editor_event` (not in the notes).

- Clippy: `redundant_closure` in `driver.rs` `wake_at`; one unused test import.
- Bug fixed in `workbench/debug.rs` `mount_debugger`: `save_all` read the workbench synchronously. The driver calls it inside `pump` while the workbench is updating, so every Debug started from an action panicked ("cannot read Workbench while it is already being updated"). It now reads the workbench in a spawned task.
- Wired, though the notes list it as not wired: `DebugDriver::set_window_focused` from the window-activation observer.
- Test changed: `the_run_widget_shows_the_selection_and_the_chooser_selects_runs_and_closes` asserted "without a debugger Debug is not offered". The debugger is registered now, and `src/features/run/RunWidget.tsx:157-166` draws Debug unconditionally, so it now expects an enabled "Debug 'hello'".

**B3 (e2e):** two of the four drafted tests panicked (the `save_all` bug above); after the fix, Step Over never stopped.

- Cause: `crates/ide-dap/src/lib.rs:1492/1532/1633` emit a synthetic `Continued` after a step or resume answer. The fake adapter's immediate `stopped` arrived before it and was overwritten.
- The webview's `state.ts:1093` handles `continued` the same way, so this is a backend race, not this crate's.
- Test fix: the fake adapter reports a stop after 0.2 s, as a real program needs time to run. In 10 of 10 module runs and all five suite runs, no session was left Running.
- Fake adapter also extended: `stepIn`/`stepOut`, `setVariable`, `evaluate`, a structured local `point`.
- Drafted tests otherwise unchanged; all pass 10 of 10 runs.

**Mistakes in my own added tests, found and fixed:**

- `set_text` skips the `Changed` event a prompt dialog reads, so I type instead.
- `agents.useEditor` is a no-op when the placement already is Editor, so the steps were reordered.
- A start drops finished sessions (`state.rs:1009`), so I assert the session was replaced rather than counting two.
- Shift on a line that already has a breakpoint does nothing, by design.
- An unclosed context menu swallowed the next click; it is now closed with Escape.

**Statements in the notes that were false or stale:**

- Agents notes: `TOOL_WINDOWS` "15 entries" (it was 16, now 18); "the editor area holds file tabs only" (view tabs exist now). §6.2–6.7 and §6.11 held: the v1 `initialize`, `-32601` replies, `session-permission-1-0`, title, merged chunks, "Archived (1)".
- Neither the notes nor the reports flagged the API-key `Debug` leak.
- Debug notes §2.11: the anchor `editors.insert(tab_id.clone(), editor);` no longer exists (already a clone).
- Debug notes §4.7: "only `intelligence.rs` has [an exhaustive match]" is false (`tab_views.rs` too).
- Debug notes §6: "closures infer their argument types" is false for 4; the `save_all` re-entrancy panic was not mentioned; the fake adapter's immediate `stopped` cannot work against this backend.
- Debug notes, held: `copy_value`'s `Handle::current()` inside `host.blocking` works (a test copies a structure); "12 px left of the text is the gutter" holds.

## 2. Actions, tool windows, context keys, shortcuts

**Tool windows:** `agents` and `debug` added (`TOOL_WINDOWS` is now 18).

**Tab kinds:** none added. Conversations stay docked in the tool window, though the store's default placement is Editor. Moving them into tabs is not cheap:

- `BuildView` is a plain `fn` with no access to the workbench's agents slot, so a signature change is needed.
- `Editors` would have to come from real tabs, most recently used first.
- `CloseTab` by key, title sync, restore, and answers routed to every open session (not only the docked one) would all be needed.

The test `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key` proves every placement action leaves the conversation docked and visible.

**Context keys:** `agentSessionFocus`, `debugSessionActive`, `debugSessionRunning`, `debugSessionPaused`, `debugSessionDart`, `debugCanRerun`, `debugCanHotSwap`, `debugFramesFocus`, `debugVariablesFocus`. Mark layers `Breakpoints`/`Execution` and mark kinds `BreakpointLine`/`InactiveBreakpoint`/`ExecutionLine` were added.

**Agents actions (21):** test names below are shortened: "keep_docked" is `the_agents_actions_keep_the_conversation_docked…`, "follow" is `the_agents_follow_notifications…`.

- Run end to end:
  - `newTask` (every test; ⌘⇧I in keep_docked).
  - `showToolWindow` (follow).
  - The placements `useToolWindow`, `useEditor`, `openTask`, `dockTask`, `undockTask`, `openTaskInEditor` (keep_docked).
  - `renameTask`, `jumpToLatest`, `sendToAgent` (paths and selection), `copyReplyMarkdown`, `quoteReply` (keep_docked).
  - `archiveTasks` and `deleteTasks` (drafted unseen-reply test).
  - `unarchiveTasks` (follow).
  - `forkTask` and `retractMessage`, guard path only, since the fake agent can't fork or roll back (follow).
  - Notification Open, via `agents.openTask:<id>` (follow).
- Never run: `addAgent`, `regenerateReply`, `forkReply`.

**Debug actions (28 `debug.*`, plus `run.debug` and `run.debugConfig`):**

- Run by tests:
  - `startConfig`, `stop`, `pause`, `resume`, `stepOver`, `stepInto`, `stepOut`, `forceStepOver`.
  - `showExecutionPoint`, `jumpToSource` (F4 in Frames), `runToCursor`, `setValue`, `rerun`.
  - `toggleBreakpoint`, `toggleTemporaryBreakpoint`, `toggleMethodBreakpoint`/`toggleFieldWatchpoint` (their notices), `viewBreakpoints`, `removeAllBreakpoints`, `muteBreakpoints`, `showToolWindow`.
  - The hot-swap and Dart actions (no-ops here).
  - `run.debug` (action, ⌃D, widget button), `run.debugConfig`.
- Never run: `forceStepInto`, `resetFrame`, `forceRunToCursor`.

**Shortcut checks** (conditions read in `src/features/{agents,debug}` across all six keymaps):

| Keys                                               | Confined to                                                                                                        | Proved by                                                                                                                                |
| -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------- |
| ⌘⇧I                                                | `projectOpen`                                                                                                      | does nothing without a project (`the_new_agent_task_and_debugger_shortcuts_need_a_project`); opens New Task from an editor (keep_docked) |
| ⌘F                                                 | `findInConversation` is unregistered                                                                               | ⌘F in a focused conversation does not open the editor's Find (keep_docked)                                                               |
| F7 / F8 / ⇧F8 / F9 / ⌘⌥R / ⌥F9 / ⌥F10 / ⇧F7 / ⌥⇧F8 | `debugSessionPaused`                                                                                               | nothing happens without a session; F8 steps and F9 resumes while paused (`the_debuggers_keys_act_only_where_their_conditions_hold`)      |
| F4                                                 | `debugFramesFocus` or `debugVariablesFocus`                                                                        | not Jump to Source in the editor (keys test); jumps from Frames (other-actions test)                                                     |
| F2                                                 | `debugVariablesFocus`                                                                                              | not Set Value in the editor; edits the value in Variables (keys test)                                                                    |
| ⌘R                                                 | `activeToolWindow == 'debug'`                                                                                      | no rerun from the editor; reruns from the Debug tool window (keys test)                                                                  |
| ⌘F8, ⌘⇧F8, ⌘⌥⇧F8, ⌘5, ⌃D                           | none (the webview's `cursor()` acts on the last active editor from anywhere), except ⌃D, which needs `projectOpen` | each does its job in a project (keys test); none acts without one (no-project test)                                                      |

**Still not wired:**

- Agents notes §5, all of it: a conversation or New Task editor tab; the full New Task page; Find in Conversation; the status-bar item; queued messages, background tasks and the changes pane; live settings changes; dragging tabs; sample conversations. Also §6.1: the Delete confirmation's checkbox starts unchecked and its buttons are English.
- Debug notes §7, all of it except window focus. The taskbar flash itself still has no shell API.

## 3. Part A (Agents)

**How the fake agent is declared:** `<test home>/config/agents/fake.toml` names `python3 fake_agent.py`, an ACP v1 script speaking newline-delimited JSON-RPC, plus `[env] FAKE_AGENT_API_KEY`. The stored draft has `{"provider":"fake"}`, and the tests assert it before Start. Nothing under the real home is read: the home is used only for Claude skills and worktree isolation, and neither runs here. No network.

**What a user can do end to end, with tests:**

- Start a task from New Task (action or ⌘⇧I).
- See the message and streamed reply.
- See a tool call ("Run cargo test").
- Answer a permission request (Allow reaches the agent).
- Stop the turn.
- Get the unread mark, badge and notification while hidden, and Open from the notification.
- Rename, Archive, Unarchive and Delete (with confirmation).
- Send to Agent, Quote and Copy a reply.
- Lose the agents when the project's trust is revoked and get them back when it is restored.
- Close and reopen the project in one window.
- The task-list rows are checked; the task menu was only opened (shown, then closed with Escape), not used.

**Not proven end to end:**

- The Retract and Regenerate confirmation dialog (the fake agent lacks rollback).
- A failing Start.

**Security results:**

- `tracing` calls: 5 in `workbench/agents.rs`, none in the views; none prints a provider, an env or a prompt.
- The key never appears in the conversation lines, list rows, composer buttons and chips, the clipboard, or the store's `Debug` (the test asserts the key really reached the provider's `env`).

## 4. Part B (Debug)

**How the fake adapter is declared:** a user language `fakedap.toml` in `<test home>/config/languages`, with `[debugger] adapter = "fake-dap", command = "python3", args = [fake_dap.py]`. The `main` configuration uses `debugAdapter: "fake-dap"`. No real debugger starts.

**What a user can do end to end, with tests:**

- Toggle a breakpoint with ⌘F8, the gutter, or Alt/Shift in the gutter (temporary, logging); mute it; list it in View Breakpoints; use Go to Source.
- Start with Run › Debug, ⌃D or the widget's Debug; stop at the breakpoint.
- See the frame `main, main.fake:2` and the local `line`.
- Step Over, Into and Out; Force Step Over; Run to Cursor; Show Execution Point; Jump to Source.
- Evaluate an expression; add and delete a watch; Set Value; Copy Value of a structure.
- Resume to the end ("Process finished with exit code 0"); Pause a running program; Stop; Rerun.
- Close a live tab with its Terminate prompt.
- Reopen the project with its breakpoints kept and a new debugger shown.

**What the editor shows:** breakpoint lines are tinted with a red dot (a hollow ring when inactive), and the execution line is tinted. Both marks follow steps and clear at the end.

## 5. End-to-end tests as they stand

**Agents (`workbench::tests`):**

- `a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn`: streaming, Stop, list row.
- `a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent`: the request, NeedsInput, the answer reaching the agent.
- `an_unseen_reply_marks_its_task_unread_and_archive_and_delete_remove_it`: unread mark, badge, Archive, confirmed Delete.
- `the_new_agent_task_and_debugger_shortcuts_need_a_project` (added): no agents or debugger key acts without a project; `run.debug*` refuse.
- `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key` (added): ⌘⇧I, the key never shown, ⌘F, Copy, Quote, Send to Agent, Rename, Jump to Latest, every placement.
- `the_agents_follow_notifications_trust_and_task_commands_and_come_back_with_the_project` (added): tool call, notification Open, the Retract and Fork guards, the task menu, Unarchive, trust off and on, reopening.

**Debug (`workbench::debug::tests`):**

- From the notes:
  - `toggling_a_breakpoint_at_a_line_marks_it_persists_it_and_lists_it_in_the_breakpoints_dialog`
  - `clicking_the_gutter_toggles_the_lines_breakpoint`
  - `debugging_a_configuration_stops_at_the_breakpoint_steps_over_and_resumes_to_the_end`
  - `stop_ends_a_running_debug_session`
- Added:
  - `the_debuggers_keys_act_only_where_their_conditions_hold`
  - `the_debuggers_other_actions_reach_the_adapter_and_the_editor`
  - `the_debug_views_evaluate_watch_set_and_copy_values_and_close_a_live_session`
  - `alt_and_shift_in_the_gutter_add_a_temporary_and_a_logging_breakpoint`
  - `the_run_widgets_debug_button_debugs_the_selection` (includes the reopen)

**Unit (added):** `agents::store::tests::debug_output_names_an_agents_environment_but_never_its_values` and `agents::builtin_agents::tests::debug_output_never_prints_an_agents_environment_values`.

## 6. Final state and out of scope

**Results:** five runs at `ok. 6102 passed; 0 failed; 1 ignored` each (the ignored test is the old one in `plugins.rs`). Clippy is clean. Every touched file passes rustfmt, `workbench/mod.rs` included.

**Out of scope (not mine to fix):**

- **`ide-dap` race:** the synthetic `Continued` is emitted after a step or resume answer (`crates/ide-dap/src/lib.rs:1492/1532/1633`). A `stopped` that arrives first is overwritten, which leaves the UI "Running" while the debuggee is paused. The webview has the same handling (`src/features/debugger/state.ts:1093`).
- **Agent environment in `crates/`:** `AgentProvider` (`crates/ide-agent/src/model.rs:400`) and `CustomAgent` (`provider.rs:273`) derive `Debug` including `env`.
- **Settings page:** `builtin_agents.rs` `format_env` shows env values in an editable settings field (already-compiled code, left as is).
- **Gutter:** the press now returns early for every `Editor`, including ones the debugger does not watch.
- **Stop menu:** it offers no debug sessions.

Files touched:

- `app-gpui/src/agents/{mod,session_view,markdown_view,store,builtin_agents}.rs`, plus `store/tests.rs` and `builtin_agents/tests.rs`
- `app-gpui/src/debugger/{mod,driver}.rs`
- `app-gpui/src/editor/{marks,view,intelligence}.rs`
- `app-gpui/src/workbench/{mod,agents,debug,tab_views}.rs`
