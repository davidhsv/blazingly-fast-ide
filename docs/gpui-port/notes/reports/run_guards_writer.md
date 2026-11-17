I wrote all three parts as new, untracked files plus hunks for existing files. Part 1 is complete, part 2 is complete with one deliberate deviation, and part 3 is partial. Nothing was compiled or run, as the brief required; no existing file was edited and no git command changed anything.

## Files written

| File                                                                                                                                                               | Lines | What it is                                                                                                                       |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----- | -------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/run/proofs40_start_gate.rs`                                                                                                                          | 149   | Test helper `StartGate`: holds a stand-in's start before the process host is asked, or holds the host's answer; has 2 unit tests |
| `app-gpui/src/run/store/tests/proofs40_run_guards.rs`                                                                                                              | 197   | 7 model tests, parts 1 and 3                                                                                                     |
| `app-gpui/src/run/store/tests/proofs40_run_tabs_pending.rs`                                                                                                        | 50    | The 2 pinned-tab cases; leave it undeclared until `RunSession::pinned` and `RunSettings::pin_tabs` exist                         |
| `app-gpui/src/workbench/tests/proofs40_run_start.rs`                                                                                                               | 165   | 3 end-to-end tests, part 1 (unix only)                                                                                           |
| `app-gpui/src/workbench/debug_guards.rs`                                                                                                                           | 192   | Part 2 logic, as `impl Workbench`                                                                                                |
| `app-gpui/src/workbench/debug/tests/proofs40_debug_guards.rs`                                                                                                      | 235   | 3 end-to-end tests, parts 2 and 3                                                                                                |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/run_guards_integration.md` | 733   | Integration notes, sections (a) to (f), every anchor copied from HEAD `e0854289f`                                                |

What the tests start: `/bin/sh` stand-in scripts in each test's temp folder (plus their `sleep`), and in the debug tests `python3` running the existing fake adapter. "No process left" is checked by the scripts' own `.started` and `.stopped` files, never by listing processes.

## Per part

- **Part 1, complete.** The driver now asks the backend for the start with a request id (`start_request(…, Some(request))`). Stop during a start cancels that request. A cancelled answer reports nothing, and a process that started anyway is stopped quietly. The hunks are in `store.rs` and `driver.rs`.
- **Part 2, complete, with one deviation.**
  - Debug on a running configuration now asks "Process 'x' Is Running" / Stop and Rerun. It then stops the Run and debug sessions, waits for the Run processes to end, and debugs.
  - A second Debug while the first is still starting does nothing.
  - "Do not ask again" is left out, because the native prompt has no checkbox.
  - **Deviation:** a debug session that is already stopping is not asked about; the Debug waits up to 10 s for it to end. Without this, every native Rerun would show the dialog, because native Rerun does not wait for the stop the way the webview does (`state.ts:961-962`).
- **Part 3, partial.**
  - Built:
    - The webview's choice of which finished tab a run reuses: same configuration first, then the selected tab, then the first.
    - Agent runs keep to their own tabs.
    - Runs started together each get their own tab.
  - The debug Rerun's new session already takes its own finished tab's place natively.
  - Left out:
    - Pins (none natively).
    - `store.ts:865`: a Run also stopping the configuration's debug sessions. The run driver cannot see the debugger.
    - `closeRunTab.test.ts`: there is no `run.closeTab` action and no stack-trace tabs natively.

## Rows

| Key                                  | In parity.json                    | Becomes                                                                                                                  |
| ------------------------------------ | --------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| `ipc:run_start`                      | yes                               | done                                                                                                                     |
| `action:run.debugConfig`             | yes                               | done                                                                                                                     |
| `action:run.runConfig`               | yes                               | stays partial (`store.ts:865`, `:929`; `:881` proven only at the model level)                                            |
| `action:debug.rerun`                 | yes                               | stays partial (the pin)                                                                                                  |
| `unit:…/javaModule.test.ts`          | yes                               | request clause closed (Hypothesis: the six cases' existing model tests plus the driver's single request path are enough) |
| `unit:…/resolveCancellation.test.ts` | yes                               | done, 3 of 3                                                                                                             |
| `unit:…/runTabReuse.test.ts`         | **no** (only `INVENTORY.md:3639`) | 2 whole, 1 in part, 1 pending                                                                                            |
| `unit:…/closeRunTab.test.ts`         | **no** (only `INVENTORY.md:3598`) | MISSING                                                                                                                  |

## Hypotheses

- Hypothesis: the async window context inside `spawn_in` has `background_executor()`.
- Hypothesis: `Window::prompt` takes the arguments `RunDriver::ask` passes it.
- Hypothesis: the Run tool window is shown by default, so the `run-stop` button is drawn.
- Hypothesis: the fake adapter's `program` is the configuration's absolute `command`.
- Hypothesis: the debugger's `launching()` covers every waiting stage of a start.
- Hypothesis: in the webview, a session still being prepared counts as a live session.
- Hypothesis: other store tests besides the one below assume a fresh tab per configuration.

How to settle each is in section (d) of the notes.

## What turned out wrong

- The brief's premise "a start that waits" does not exist natively for a local program. The backend only checks the cancel token at entry and after a plugin target's preparation. That is why the stand-in gate holds the start (or its answer) inside `run_process`, under `#[cfg(test)]`.
- The native store's existing abort flag lives in the per-tab runtime, which a rerun replaces. A late answer from a start superseded by a rerun would have orphaned its process. So the cancelled requests are now kept on the store.
- Native `start_session` drops every finished debug tab. The webview replaces only one, so this mismatch is left as is.
- The existing test `run::store::tests::a_failed_run_notifies_when_the_run_window_is_hidden` breaks once tabs are reused. Hunk 3.7 in the notes fixes it.
- The `proofs40_run_guards` tab tests need part 3's hunks, so declare that file after part 3 lands.
