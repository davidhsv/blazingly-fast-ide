All three parts landed and every gate passed on the final tree; nothing was committed or pushed. Most guards were seen failing before the code that makes them pass. Two waits in part 2, the gate's own two unit tests and one early return were never seen failing, or never tested; they are named below.

## Gates (final tree)

- **Whole suite, run 1 (nothing else running):** `test result: ok. 8252 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 213.39s`. Uptime load before 3.42, after 8.15. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/suite_1.log`
- **Whole suite, run 2:** `test result: ok. 8252 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 225.42s`. Uptime load before 6.94, after 74.27. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/suite_2.log`
- **Twenty runs during suite 2:** 20 of 20 passed, each `test result: ok. 22 passed; 0 failed`. No test failed in any run. The 22 tests are all 18 `proofs40` tests plus the 4 whose expectation or setup changed. The loop ran the already-built test binary directly (no cargo, no build) and finished within the first minutes of suite 2. Summary: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/loop_summary.txt`; per-run logs are `loop_1.log` to `loop_20.log` in the same folder.
- **`cargo test --test web_view`:** `test result: ok. 1 passed; 0 failed`. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/web_view.log`
- **`cargo clippy --all-targets -- -D warnings`:** rc=0, no warnings. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/clippy.log`
- **`cargo build`:** rc=0, 0 warnings. Log: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/build.log`
- **An earlier round of the same gates** (before the extra test and assertions below) also passed; its logs have the prefix `first_round_` in the same folder.
- **No process left:** `ps -p` on the stand-in PIDs printed in the logs (96958, 96950, 8313) found none. `pgrep` for the test binary, `cargo test` and the fake adapter found nothing. Each stand-in also ends by itself once its script is gone (`while [ -e "$0" ]`). It is a one-time check by those PIDs and names, not a list of every PID the tests started.

## Part 1: Stop while a run is starting

Every process start now goes out with a request that Stop can cancel. The driver calls `ide_host::ops::run::start_request(…, Some(request))`, as `src-tauri/src/commands/run.rs:110` does. The cancelled requests are kept in the store (`cancelled_starts`). A late answer to a cancelled start stops the process it started, quietly, through `StartEffect::Stop`. The webview sources are `src/features/run/store.ts:1020`, `:1027` and `:1043` (cancelled or stopped errors are not reported), and `:1336` (Stop of a starting session).

End-to-end tests are in `workbench::tests::proofs40_run_start`. "Before" means the scaffolding stage, with no request and no guard: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/before_part1_e2e.log`.

- **`cancels_startup_and_handles_a_late_cancelled_response`:** failed before ("the start goes out with a request Stop can cancel"). It asserts no `started` marker, the tab Finished and stopped, no "Error running" or notification, an empty console, and the tab icon not shown running. Hypothesis: the webview's console also stays empty here (by reading `store.ts:1043`).
- **`cancels_startup_and_handles_a_late_success_response`:**
  - Failed before: the tab stayed `(Starting, false)`.
  - Failed with the late-answer guard switched off ("the process that started anyway to be stopped" timed out): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut2_no_late_answer_guard.log`.
  - Proves the backend has no wait point in a local start: the process starts anyway, and the stand-in's own marker files show that the one PID that started is the one that got the stop.
  - The bystander of another configuration keeps running. That assertion failed when the late stop also stopped every running session: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut3_stop_everything.log`.
- **`a_late_answer_after_stop_and_rerun_stops_only_the_first_starts_process` (new):** Stop, then Rerun at once, then the first start's late answer. Two PIDs start, only the first is stopped, and the rerun's runs in the same tab.
  - Failed before.
  - Failed with the store's cancelled list switched off: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut1_no_store_cancel.log`. That measures the writer's reason for moving the list from the per-tab runtime to the store.
  - Failed with the stop-everything mutation (`mut3_stop_everything.log`).
- **`a_start_let_go_before_stop_runs_its_program`:** failed before. Also failed when the request was not cleared on the answer: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut6_no_debug_stop_no_request_clear.log`.

Model tests are in `run::store::tests::proofs40_run_guards`:

- **`cancels_startup_and_handles_a_late_cancelled_response`, `cancels_startup_and_handles_a_late_success_response`, `a_start_nobody_stopped_ends_its_request_with_the_answer`:** all three failed with the request clear switched off (`mut6…log`); the late-success one also with the guard off (`mut2…log`).
- **`a_late_answer_of_a_cancelled_start_stops_its_process_and_leaves_the_restarted_tab_alone`:** failed in `mut1…log` and `mut2…log`.
- **Gate unit tests:** `run::proofs40_start_gate::tests::*` (2) were never seen failing.

Left out: `store.ts:865`, where a Run also stops the configuration's live debug sessions.

## Part 2: Debug on a running configuration asks; a second Debug while the first prepares does nothing

Tests are in `workbench::debug::tests::proofs40_debug_guards`. The guard-bypassed before-log is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/before_part2_no_guard.log`. It is a valid before-log for three tests; the rerun test failed there on its own race, not on the guard.

- **`debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another`:**
  - The question is the webview's (`src/features/run/dialogs.tsx:34-53`, button "Stop and Rerun"). Failed before ("the question did not happen").
  - Its bystander assertion failed when Debug stopped every run: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut5b_debug_stops_every_run.log`.
- **`debug_on_a_running_configuration_stops_it_without_asking_when_the_setting_is_off` (new):**
  - "Do not ask again" stores global `run.confirmRerun=false` (`dialogs.tsx:42`, `:46`). With that setting, the native app asks nothing.
  - Failed before.
  - Failed when the setting was ignored ("nothing is asked"): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut4_debug_guards_off.log`.
  - Its bystander assertion failed in `mut5b…log`.
- **`a_second_debug_while_the_first_one_prepares_does_nothing`:**
  - Now waits until the debugger has no start under way, instead of advancing the clock.
  - Failed before: 2 sessions.
  - Failed with the "already launching" check off: 2 sessions (`mut4…log`).
- **`debug_on_a_debugged_configuration_asks_then_stops_the_session_and_debugs_again` (new):**
  - It covers the branch that stops a debugged configuration's session, which no draft test reached.
  - Failed with that branch off (`mut6…log`).
  - The Debug now also waits for the session it just stopped, as the webview awaits `d.stop()`. That wait was never seen failing: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/mut7_no_wait_for_stopped_debug.log`.
- **The writer's deviation (a session already Stopping is not asked about; Debug waits up to 10 s for it to end):**
  - The wait polls a background timer in a spawned task, never sleeping on the window's thread.
  - Read: `src/features/debugger/state.ts:962` awaits `stopSession`, which awaits the backend's `debug_stop`. That call waits for the debuggee and the `terminated` event (`crates/ide-dap/src/lib.rs:1987`, `shutdown`).
  - Hypothesis: a webview user sees no question on Rerun and a new session once the old one has ended, which the deviation matches. It depends on the `terminated` event reaching the UI before the command's reply; a webview run of Rerun would settle it.
- **Other changes:**
  - A Debug whose window closed while it waited no longer launches. This is not tested.
  - "Mark as recent" now happens when the launch goes to the debugger, after the question. The webview marks recent when a session appears (`store.ts:370`, `:376`).

## Part 3: which finished tab a run reuses

- **What changed:** `RunStore::reusable_tab` (from `store.ts:781`) and `claimed_tabs` (`store.ts:790`, `:821`, `:884-890`) are built. The tab is chosen when it opens.
- **Before-log** (part 1 applied, part 3 not): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/before_part3_model.log`. All three tab tests below failed there:
  - `takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one`
  - `keeps_an_agents_commands_out_of_other_tabs` (`AGENT_PARAM` is `"mcp.agentRun"` on both sides)
  - `gives_runs_started_together_their_own_tabs`
- **Changed expectations:**
  - `workbench::tests::a_run_configuration_runs_into_the_run_tool_window_and_stop_ends_a_long_running_one`: tab icons `["file-binary","run"]` become `["run"]`, and `["file-binary","file-binary"]` become `["file-binary"]`. `nap` now takes `hello`'s finished tab, per `store.ts:781-787` and `:882-883`.
  - `run::store::tests::a_failed_run_notifies_when_the_run_window_is_hidden`: passes the tab's generation, because `npm:test` reuses `npm:build`'s tab (`store.ts:781-787`).
  - Setup changed, assertions unchanged: `run::view::tests::shows_the_red_error_icon_for_a_failed_exit` and `run::view::tests::shows_the_configurations_own_icon_once_the_process_ended_ideas_third_state`. Their spec, `RunView.test.tsx:42-51`, builds its sessions directly.
- **Not built:**
  - Pinned tabs: `store.ts:929` and the pinned clause; `proofs40_run_tabs_pending.rs` stays undeclared.
  - `store.ts:865`.
  - `closeRunTab` (`src/features/run/runTabs.ts:20-26`).
  - Webview `replaceFinished` (`state.ts:318`): native still drops every finished debug tab.

## Rows (keys checked in `docs/gpui-port/parity.json`)

| Key                                                 | In parity.json | Native tests                                                                                                                                                                                                                                                                                                                                                 | Proposed                                                             |
| --------------------------------------------------- | -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------- |
| `ipc:run_start`                                     | yes, partial   | `workbench::tests::proofs40_run_start::{cancels_startup_and_handles_a_late_cancelled_response, cancels_startup_and_handles_a_late_success_response, a_start_let_go_before_stop_runs_its_program, a_late_answer_after_stop_and_rerun_stops_only_the_first_starts_process}`                                                                                    | done                                                                 |
| `action:run.debugConfig`                            | yes, partial   | `workbench::debug::tests::proofs40_debug_guards::{debug_on_a_running_configuration_asks_to_stop_it_instead_of_starting_another, debug_on_a_running_configuration_stops_it_without_asking_when_the_setting_is_off, a_second_debug_while_the_first_one_prepares_does_nothing, debug_on_a_debugged_configuration_asks_then_stops_the_session_and_debugs_again}` | done for these clauses                                               |
| `action:run.runConfig`                              | yes, partial   | `run::store::tests::proofs40_run_guards::gives_runs_started_together_their_own_tabs` (model only)                                                                                                                                                                                                                                                            | stays partial: `store.ts:865` and `:929` are not built               |
| `action:debug.rerun`                                | yes, partial   | `workbench::debug::tests::proofs40_debug_guards::rerun_stops_the_live_session_first_and_takes_its_own_tabs_place`                                                                                                                                                                                                                                            | stays partial: the pin is not built; the wait was never seen failing |
| `unit:src/features/run/javaModule.test.ts`          | yes, partial   | see below                                                                                                                                                                                                                                                                                                                                                    | partial                                                              |
| `unit:src/features/run/resolveCancellation.test.ts` | yes, partial   | see below                                                                                                                                                                                                                                                                                                                                                    | done                                                                 |
| `unit:src/features/run/runTabReuse.test.ts`         | no             | see below                                                                                                                                                                                                                                                                                                                                                    | partial                                                              |
| `unit:src/features/run/closeRunTab.test.ts`         | no             | none                                                                                                                                                                                                                                                                                                                                                         | MISSING                                                              |

Every spec case at HEAD:

`resolveCancellation.test.ts`:

- "cancels the request and ignores a late %s response" → `run_producers::store_cases::cancels_the_request_and_ignores_a_late_cancelled_response` and `run_producers::store_cases::cancels_the_request_and_ignores_a_late_success_response`
- "uses $separator from the host on $client" → `run_producers::store_cases::uses_the_separator_from_the_host`
- "cancels startup and handles a late %s response" → the two `run::store::tests::proofs40_run_guards::cancels_startup_…` tests and the two `workbench::tests::proofs40_run_start::cancels_startup_…` tests

`runTabReuse.test.ts`:

- "takes the place of the same configuration's finished tab, then the selected one, never a pinned one" → `run::store::tests::proofs40_run_guards::takes_the_place_of_the_same_configurations_finished_tab_then_the_selected_one` (all but the pinned clause; the pinned clause is MISSING)
- "opens a new tab when every finished tab is pinned, pinned when the setting asks" → MISSING (pending file)
- "keeps an agent's commands out of other tabs" → `run::store::tests::proofs40_run_guards::keeps_an_agents_commands_out_of_other_tabs`
- "gives runs started together their own tabs" → `run::store::tests::proofs40_run_guards::gives_runs_started_together_their_own_tabs`

`closeRunTab.test.ts`:

- "hides the Run tool window once its last tab is closed" → MISSING

`javaModule.test.ts`, the six cases that expect the request id:

- Model tests: `run::store::tests::runs_from_another_directory_using_the_stored_modules_classpath`, `…::finds_the_source_module_for_an_old_saved_configuration_without_a_module`, `…::colours_spring_boot_runs_without_classpath_resolution`, `…::preserves_an_explicit_spring_boot_console_setting`, `…::keeps_plain_java_runs_with_an_explicit_classpath_free_of_spring_boot_options`, `…::adds_the_spring_boot_console_option_once_when_resolving_the_module_classpath`.
- The request clause is proven end to end for a program launch only, by `workbench::tests::proofs40_run_start::a_start_let_go_before_stop_runs_its_program`.
- Hypothesis: Java launches carry it too, because there is only one `StartStep::RunStart` arm in `RunDriver::drive` (by reading).
- The other 13 cases are unchanged.

## Which way the notes' hypotheses went

- **1, 2, 9** (`background_executor` on the async window context, `Window::prompt`'s signature, `RequestId` usable as a set key): held, it compiled.
- **3** (Settings › Run shows the Run tool window on start, so `run-stop` is drawn): held, the tests click it.
- **4** (the fake adapter debugs the configuration's command path): held.
- **5** (no preparation session for the fake adapter, so a second Debug meets the guard): held, measured by the second-Debug test.
- **6** (a preparation session counts as live in the webview): settled by reading. The webview's preparation session carries `runConfig` (`state.ts:681-700`) and is listed among the service sessions (`:353`).
- **7** (the six javaModule cases ticked from model tests plus the single request path): see the table above.
- **8** (other tests assume a new tab per configuration): held. Two view tests and one workbench test changed, listed in part 3.
- **10** (Debug starts after the Run process is gone because the guard polls the run store): held.
- **2.5** (only the tests use bare `json!`): false. `proofs34_remote_debug.rs` uses bare `json!`, so `use serde_json::json;` was added to `workbench/debug.rs`'s `mod tests`.

## False or stale in the notes and brief

- The brief says HEAD is `0215ddc39`; `git log` shows HEAD at `a4e6b214e`, a notes-only commit on top of it.
- The notes call the drafts untracked; they are committed.
- The drafted model tests did not drain the earlier steps' effects. The `begin` helper now does, as the driver applies them step by step.
- The drafts' waits on a marker file's existence and on `debugSessionRunning` raced. They now wait for the file's content and for a root session of `main`.

## Files

- **Edited:**
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/debugger/actions.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/debugger/state.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/driver.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/mod.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/store.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/view.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/debug.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/mod.rs`
- **Handed drafts, now declared:**
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/proofs40_start_gate.rs` (unchanged)
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/run/store/tests/proofs40_run_guards.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs40_run_start.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/debug_guards.rs`
  - `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/debug/tests/proofs40_debug_guards.rs`
- **Still undeclared:** `proofs40_run_tabs_pending.rs`.
- **New files:** none in the tree. Scripts and logs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drgi/`.
