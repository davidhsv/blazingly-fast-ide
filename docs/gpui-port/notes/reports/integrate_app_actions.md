All five full-suite runs are green: 6548 passed, 0 failed, 0 ignored each time (about 61 s per run). Clippy `--all-targets -D warnings` is clean, and `rustfmt --check` over the crate shows no diffs. Nothing is committed.

## 1. Build and scope

- **Compile errors:**
  - Part 0: 0.
  - Part 1: 4 lifetime-elision errors in the `open` helpers of the three `app_views` dialogs, 1 type annotation, then clippy 4 (2 `let _` on a future, a redundant closure, an unneeded `&mut`).
  - Part 2: 2 (`px` not in scope, a closure type annotation).
- **Scope:**
  - **Out-of-scope files:** none touched (`docs/`, `src/`, `src-tauri/`, `crates/` and the later drafts are unchanged), and no git command changed anything.
  - **Formatting:** rustfmt was run only on files I edited.
  - **Processes:** two `pkill -f openidea_gpui-7e4f14697984a448` killed only this worktree's test binary.

## 2. Part 0: the Git gate

- **Mechanism:** `git_command_starts` now returns a `GitCommand` guard, numbered from a process-wide counter, and spawns a detached release task that waits on the guard's oneshot. `git_command_ended(this, cx, command)` opens the gate at once. If the future is dropped instead, the release task runs `end_git_command(number)` on the next turn, which also starts any deferred read. A late release never opens a later command's gate.
- **Test:** `workbench::tests::a_git_command_dropped_before_it_ends_lets_the_next_operation_run`. It starts `run_op` with a backend call that never completes, drops the task, asserts the gate is open, then a Checkout from the Branches popup completes and the gate is released again.
- **Callers that store a gated task (read, not run):** none. Every gated command (`run_op`, `run_git_operation`, `run_remote`) runs inside a `.detach()`ed spawn. The stored `= Some(cx.spawn…)` sites are reads (refreshing, debounce, change lists, shelves), so none can be replaced mid-command.
- **Hypothesis:** `remote_busy`, set by `run_git_operation`/`run_remote` before the gate, stays set if the future is dropped, so later operations say "Another … operation is in progress". Confirm with the Part 0 test, driven through `run_git_operation` with a hanging `vcs_operation`.

## 3. Part 1

All 20 hunks applied from the notes. All 18 action ids are registered, and the saved list (509 ids) is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/registered_ids.txt`. The action table (id, test, IPC adapter and backend function, done or partial) is section 18 of the notes; it holds as written. Every test lives under `workbench::app_actions::tests::`. The native calls are the same backend functions; `set_trust` and `remove_trusted_location` also call `PluginHost::restrict_project`, as the adapters at `app.rs:328` and `app.rs:350` do.

**Context keys:** `toolWindowLayoutsSaved` is read ON and OFF in `saves_and_restores_visible_windows_and_sizes_by_name`; `distractionFree` ON and OFF in `hides_tool_windows_and_restores_the_previous_layout_on_exit`. ⇧F12 doing nothing outside `projectOpen` is in `the_restore_default_layout_key_works_only_with_a_project`.

**Product fixes in Part 1:**

- `set_distraction_free` panicked by reading the workbench that was being updated (`windows::windows`); it now walks `cx.windows()` instead.
- In the trust dialog, a long parent folder pushed the buttons outside a 1536-pixel-wide window, so Trust could not be clicked. The folder checkbox now shrinks. New test: `app_views::trust_dialog::tests::a_long_parent_folder_is_cut_and_keeps_the_buttons_inside_the_dialog`.
- With a restricted project open, any Plugins page re-listed forever. `sync_project` unloads the project's plugins and announces a change on every call, and each change triggers a new listing. `list_plugins` now syncs project plugins only for a trusted project.

**What Restricted Mode refuses natively, each with a trusted control:**

| Refusal                                                                                                                                 | Test                                                                                                       |
| --------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| Database: connect, schema re-read, Test Connection (new gate: `authorize_command` with each DB op's IPC name in `run_op` and `db_call`) | `workbench::tests::a_project_in_restricted_mode_connects_to_no_database_until_it_is_trusted`               |
| Run configurations ("Error running 'touch'", program never runs)                                                                        | `workbench::tests::a_project_in_restricted_mode_runs_no_configuration_until_it_is_trusted`                 |
| Debugger                                                                                                                                | `workbench::debug::tests::a_project_in_restricted_mode_debugs_nothing_until_it_is_trusted`                 |
| Language server (no `initialize`)                                                                                                       | `editor::intelligence::tests::a_project_in_restricted_mode_starts_no_language_server_until_it_is_trusted`  |
| Project plugins (not loaded, then unloaded on Revoke Trust)                                                                             | `workbench::plugins_window::tests::a_projects_own_plugins_load_only_while_the_project_is_trusted`          |
| Git `post-checkout` hook (checkout refused, HEAD unchanged)                                                                             | `workbench::tests::a_project_in_restricted_mode_runs_no_git_hook_until_it_is_trusted`                      |
| Agents (existing test, trust flipped through the backend)                                                                               | `workbench::tests::the_agents_follow_notifications_trust_and_task_commands_and_come_back_with_the_project` |

**Still allowed natively that the webview forbids (by reading):** `ops::run::save_configs` and the VCS file writes (patch, shelf, change lists) are not trust-checked.

**Tests that needed a trust step:** only the notes' 3i and 3j in the Part 1 build. In Part 2, `proofs3_windows` (both tests) and `proofs4_shell::open_recent_lists_…` needed one too.

**Row lists:** no literal row list changed (`ROOT`, `GO_TO`, `GIT`, menu tests are unchanged and green).

## 4. Part 2

All 34 drafted tests are declared and pass (23 from proofs3, 11 from proofs4); the per-test `parity.json` keys are as in the notes. Each fix below names the webview source it matches:

- **Quit with a debug session:** `running_process_names` and `terminate` in `app_shell.rs` now include live top-level debug sessions, stop them, and wait for them (webview `src/features/run/index.ts:401`). The test `quit_with_a_running_debug_session_asks_to_terminate_it` was corrected because the debugger is released when the project closes.
- **Run console resize:** a canvas in `run/view.rs` measures terminal consoles in cells and emits `RunViewEvent::Resize`; `RunDriver::resize_terminal` calls `ops::run::resize` (webview `src/features/run/store.ts:1356-1363`). This uncovered a second bug: terminal-mode output never reached the view after its first render, because `RunStore::enqueue` did not bump the version. It does now, and the driver notifies. New test: `workbench::tests::a_run_in_a_terminal_console_is_told_the_consoles_size_and_follows_a_resize`.
- **Tooltips:** `Tooltip::shown` hides the bubble on any key press in its window and when the window loses activation (webview `src/ui/Tooltip.tsx`). All builders use it, including `title_bar.rs` and `hover_view.rs`.

**Test-side corrections (the tests were wrong, not the product):**

- `proofs4_vcs::routes_all_paths_…`: the status lists `out/build.bin`, not `out/`.
- `proofs4_tooltip`: it waits for the window's activation to settle before hovering.
- `proofs3_debug::open_file`: its wait no longer expects an editor that is briefly absent; this was the one failure in an earlier five-run, now gone.

No drafted test was left out.

## 5. Notes that turned out false, and open hypotheses

- **False:**
  - Notes 10.4 marks plugins "done", but a restricted project made the Plugins listing loop forever.
  - Notes 10.2 says only two test bodies meet the trust question; three more did, in the Part 2 drafts.
  - Proofs4 expects the status to list `out/`.
  - The tooltip module comment claimed GPUI hides tooltips on a key; it does not.
  - Proofs4's "only `run_resize` is missing" was incomplete: terminal consoles also showed no output.
- **Hypothesis:** the macOS relaunch may not keep `OPENIDEA_GPUI_HOME`. Confirm with `ps -E -p <pid>` after a Restart in a dev build.
- **Hypothesis:** the "Process 'X' Is Running" list skips child debug sessions; whether the webview lists them is unchecked. Confirm by reading `liveServiceSession` in `src/features/run/`.
