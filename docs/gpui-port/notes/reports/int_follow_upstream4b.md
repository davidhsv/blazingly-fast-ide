# Integrator report: follow upstream 4, items 4 to 7

Items 4, 5 and 6 are done. Item 7 is done except two sub-items: the plugin command running state (not started) and the Unity configuration (the brief says to list it, not build it). Nothing is committed or pushed. All logs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dfol4b/`.

## 1. Gates (all measured)

| Gate                                                     | Result                                                                                                | Log                 |
| -------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------- |
| Suite 1 (`cargo test --lib`)                             | `ok. 8344 passed; 0 failed`, 227.71 s. `uptime` load before 4.44, after 57.37. Nothing ran beside it. | `suite1.txt`        |
| Suite 2                                                  | `ok. 8344 passed; 0 failed`, 226.60 s. Load before 38.82, after 33.37.                                | `suite2.txt`        |
| Twenty-fold repeats (after suite 2, nothing beside them) | All 32 added or changed tests passed 20/20; no failures.                                              | `rep20/summary.txt` |
| `cargo test --test web_view`                             | `ok. 1 passed`                                                                                        | `webview.txt`       |
| `cargo clippy --all-targets -- -D warnings`              | exit 0, no warnings, run after the last edit                                                          | `clippy2.txt`       |
| `cargo build`                                            | exit 0, 0 warnings                                                                                    | `build.txt`         |
| `node docs/gpui-port/gen-data.mjs --check`               | exit 0, prints nothing                                                                                | `gencheck.txt`      |
| `git status --short src/ src-tauri/ crates/ e2e/ docs/`  | prints nothing                                                                                        |                     |

- **Disk:** 322 GB free was the lowest reading before the heavy runs (`log.txt`).
- **Failures:** none in either suite. The eight load-sensitive run and debug tests the brief names did not fail.
- **Failure during development:** `proofs3_windows::open_in_new_window_opens_the_given_or_picked_folder_in_a_new_window` failed once while I was working. My first new-window code opened the window inside the Workbench's own update, and GPUI panicked with "cannot read openidea_gpui::workbench::Workbench while it is already being updated". It was fixed (the window now opens after that update) and the test passed in every run since.
- **Failed first:** this means a mutation check, one switched-off behaviour per script run, restored by a trap, file hash compared. All 13 checks made their test fail and all report `restored: yes` (`mut/*.log`): qd, qf, db, dbcell, reset, trust_flow, trust_win, hide, env, welcome, term, http, version. The two condition tests of item 7 (`TEXT_WHEN`, `EDITOR_WHEN`) were not mutation-checked; they assert the new text directly.
- **Formatting:** only the files from `git diff --name-only` plus my two new files, through `fmt.sh`.

## 2. Per item

### Item 4: Editor

**Apply Quick Fix matches the fix to its problem.**

- Webview: `src/editor/lsp/intentions.ts:72` (`actionsForDiagnostic`), `:168` and `:189`; commit bdb8ca554 (#2411).
- Native changes:
  - `app-gpui/src/editor_lsp/diagnostics.rs`: `LintDiagnostic` now keeps the diagnostic as the server reported it, in a `problem` field.
  - `app-gpui/src/editor_lsp/intentions.rs`: new `actions_for_diagnostic`; `quick_fix_order` now returns the problems themselves.
  - `app-gpui/src/workbench/editor_language.rs` (`apply_quick_fix`): only fixes that name the problem are tried.
  - `app-gpui/src/editor_lsp/hover_view.rs`: test literal updated.
- Tests:
  - `workbench::editor_language::tests::proofs41_stale_and_matching::uses_the_matching_fix_for_apply_quick_fix`, failed first (qf).
  - `editor_lsp::intentions::tests::{keeps_only_actions_for_each_problem_even_when_another_problems_fix_is_preferred, retains_the_range_fallback_only_when_every_action_omits_diagnostics, checks_diagnostic_identity}`, the three spec cases of `intentions.test.ts`.
  - `editor_lsp::diagnostics::tests::converts_ranges_sorts_and_adds_mark_classes`: expectation now includes the kept diagnostic, as `diagnostics.ts:149` does.
- Left out: the webview also puts local inspection fixes first (`contributedQuickFixes`); the native Apply Quick Fix never had them. The problem hover, auto-import and imports-on-paste use the same filter in the webview; I did not check whether the native ones do.

**Quick Definition drops a late answer.**

- Webview: `src/editor/lsp/hover.ts:364` (`stale`), checked at `:368` and `:381`.
- Native change, `editor_language.rs` (`quick_definition`): the answer is dropped after either await when the editor's version or document changed, or the editor is no longer one of the window's editors.
- Test: `...proofs41_stale_and_matching::quick_definition_drops_an_answer_that_arrives_after_the_text_changed`, failed first (qd). The stand-in server answers the first request 2 s late, the text is edited meanwhile, and the test records every popup shown and asserts the late one never was.
- Not tested: the "editor gone" branch of the check.

### Item 5: Database

**Submit, Revert and Go to Column reach the console's result grid.**

- Webview: `src/features/database/ResultsView.tsx:435-483`, `tableEditorFocus.ts:90`; commit 68dd88993.
- Native changes:
  - `app-gpui/src/database/results_view.rs`: Go to Column for a result (`show_columns`, `shown_columns`, `columns_query`, `pick_column`, `revealed_column`). The placeholder is "Columns of `<table>`", else the tab title, as `ResultsView.tsx:619` has it.
  - `app-gpui/src/workbench/database.rs`:
    - `focused_database_results`: the result pane counts when it shows a result and has the keyboard outside its search field.
    - `sync_database_context` sets `dbTableEditorFocus` and `dbTableEditorChanges` from it; the pane's focus and changes now resync them.
    - New `on_database_grid`: confirms the cell being edited in the table editor's or the result's grid, then runs the command once the confirmed edit has arrived.
  - `app-gpui/src/workbench/database_extras.rs`: Go to Column goes through `on_database_grid`.
- Test: `workbench::tests::proofs43_database_results::submit_revert_and_go_to_column_reach_a_focused_console_result`, failed first twice:
  - db: switching off the result pane fails the context-key assertion.
  - dbcell: switching off the cell confirm loses the typed value.
  - Covers: context keys on and off; Submit writes to SQLite; Revert drops an edit; Go to Column filters as typed and Enter selects the column; Submit with a cell still being edited writes both that value and the pending edit.
- Not tested: confirming the cell before Revert and Go to Column, and before the table editor's commands. The same code path runs them.

**Requests that wait for the data sources when the project changes (settled by reading and one test).**

- Webview: `src/features/database/requests.ts:20`.
- The native Database slot is rebuilt per project:
  - `open_project` and `close_project` call `reset_database` (`app-gpui/src/workbench/mod.rs`, near 3184 and 3355), which takes the slot.
  - A statement waiting for the sources (`pending_run`) and Query History's wait (`extras.history_wanted`) live in that slot.
  - Backend answers carry the slot's epoch, and `db_answered` drops an answer from another epoch (`database.rs`, `filter(|slot| slot.epoch == epoch)`).
  - New Data Source opens its dialog at once and waits for nothing.
- Test: `...proofs43_database_results::a_statement_waiting_for_the_data_sources_is_dropped_when_the_project_changes`, failed first (reset: with `reset_database` keeping the slot, the first project's statement shows up).
- Measured for `db.execute` only. For `db.explainPlan` (same `pending_run`) and `db.queryHistory` (`history_wanted`), this is read, not tested.
- Gaps: answers that go through `db_call` (Test Connection, name loading and others) have no epoch check. I left them because none of them waits for the data sources.

### Item 6: Trust

- Webview: `src/features/app/windows.ts:140` (`openProjectWindow`); commit 7709897db (#3663); spec `src/features/app/__tests__/windows.test.ts` "trust is asked before a new project window opens".
- Native changes:
  - `app-gpui/src/workbench/app_shell.rs`, the `RequestWindow` step: a new window is asked about in this window first. Don't Open stops the opening with no window and nothing decided. Uses the new `OpenFlow::new_window_trust` in `app-gpui/src/app_models/windows.rs`.
  - `app-gpui/src/workbench/app_actions.rs`: new `ask_before_new_window`; a new trust answer, `TrustAnswer::NewWindow`, opens the window only after Trust or Preview; the status read moved into `trust_question`.
  - `app-gpui/src/workbench/windows.rs` (`open_project`, used by Open in New Window): asks the same way, and opens the window after the Workbench update.
- Tests:
  - The stale test was rewritten to the webview's order: `workbench::app_actions::tests::a_project_opening_in_a_new_window_is_asked_about_in_this_window_first`, failed first (trust_flow).
  - `workbench::app_actions::tests::open_in_new_window_opens_no_window_when_the_user_chooses_dont_open`, failed first (trust_win).
- Remote trust: native cannot open remote projects (`app_shell.rs`, `OpenStep::ConnectRemote` answers "is on another machine"), so the webview's desktop-side remote trust (`src/features/project-trust/index.tsx:204`, `:282`) is listed, not built.

### Item 7: Small ones

| Sub-item                                                  | Webview source                                                                 | Native change                                                                                                                                                                                                       | Test                                                                                                                                                                                                                                                                                                                          | Failed first         |
| --------------------------------------------------------- | ------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------- |
| Hide Active Tool Window clears an orphaned maximized side | `src/workbench/toolwindows/store.ts:653`                                       | `tool_windows.rs` (`hide_active`, `shows_registered_window_on`)                                                                                                                                                     | `workbench::tests::proofs_tool_windows::hide_active_tool_window_clears_a_maximized_side_whose_window_went` (through the registry) and `workbench::tool_windows::tests::hide_active_clears_a_maximized_side_whose_window_unregistered`                                                                                         | yes (hide, both)     |
| Welcome window's buttons                                  | `src/features/welcome/ProjectsPage.tsx:29-63`                                  | `welcome_model.rs`: New Project and Open File added, Open File always shown; `landing_text` follows the webview's three texts; `welcome.rs` passes `can_create`                                                     | `workbench::welcome_model::tests::open_and_open_file_are_always_offered_and_the_other_buttons_when_their_action_is_registered` (renamed), `workbench::welcome::tests::the_buttons_follow_the_registered_actions` (clicks New Project and Open File), `...::shows_the_landing_page_without_recent_projects` (clicks Open File) | yes (welcome)        |
| Welcome window's version                                  | —                                                                              | `build.rs` reads the root `Cargo.toml` `[workspace.package]` version and passes it as `OPENIDEA_VERSION`; every `CARGO_PKG_VERSION` use in `app-gpui/src` now reads it (about dialog, updates page, Welcome, tests) | `workbench::tests::a_window_with_nothing_open_welcomes_with_the_recent_projects` compares the Welcome version with `src-tauri/tauri.conf.json`'s `version`                                                                                                                                                                    | yes (version)        |
| HTTP endpoint request takes the server address            | `src/features/http-client/endpoints.ts:28`                                     | `render_endpoint(e, host)`; `workbench/http_client.rs::open_endpoint` reads the module's config files on the backend through `config_candidates` and `host_from_files`                                              | `workbench::http_client::tests::open_in_http_client_takes_the_server_address_of_the_endpoints_module`; unit test `renders_optional_handlers_and_any_method_routes_into_http_blocks`                                                                                                                                           | yes (http)           |
| Debug launches load `.env` files                          | `src/features/debugger/launch.ts:45` (`withEnvFiles`), `actions.ts:369`        | `debugger/effects.rs` new `BackendCall::RunEnvFiles`; `actions.rs` new `Stage::EnvFiles`, `env_files_loaded`, `launch_with_env`; `driver.rs` answers it with `ide_host::ops::run::run_env_files`                    | `debugger::actions::tests::a_debug_launch_loads_the_configurations_env_files_under_its_own_variables` and `...::a_debug_launch_without_env_files_reads_none_and_one_whose_file_is_unreadable_does_not_start`                                                                                                                  | yes (env)            |
| "Couldn't fork the task"                                  | `taskActions.ts:307`, `replyActions.ts:149`                                    | `agents/reply_actions.rs` new shared `FORK_FAILED`, used there and at both sites in `workbench/agents.rs`                                                                                                           | `agents::reply_actions::tests::reports_a_failed_fork_as_a_failed_fork`                                                                                                                                                                                                                                                        | no mutation check    |
| Agent task menu "Terminal"                                | `taskActions.ts:399-401`, `model.ts:113` (`hasTaskTerminal`); commit 59fba1a65 | `agents/task_actions.rs` new `has_task_terminal`; `task_menu` takes the window's project root, which the task list gets through `with_project_root` (`task_list_view.rs`, `workbench/agents.rs`)                    | `agents::task_actions::tests::offers_terminal_for_worktree_or_off_root_tasks_and_hides_it_for_local_checkout_tasks_at_the_project_root` (the spec's three cases plus a task with no root); `keeps_placement_out_of_session_menus` expectation changed to `model.ts:113`                                                       | yes (term)           |
| The two hard-coded conditions                             | `src/editor/core/actions.ts:102`, `src/editor/lsp/actions.ts:33`               | `editor_extras.rs` `TEXT_WHEN` gains `&& !toolWindowFocus`; `refactor_actions.rs` `EDITOR_WHEN` drops `popupOpen`                                                                                                   | `workbench::editor_extras::tests::expand_to_level_is_off_while_a_tool_window_has_the_keyboard_over_a_popup` (also equals the manifest's `code.fold` condition), `workbench::refactor_actions::tests::refactor_this_is_not_the_editors_while_a_tool_window_has_the_keyboard_over_a_popup`                                      | not mutation-checked |
| Plugin command running state                              | `src/features/plugins/points.tsx:213`                                          | not started                                                                                                                                                                                                         | —                                                                                                                                                                                                                                                                                                                             | —                    |
| Unity Editor run configuration                            | `src/features/run/store.ts:241`                                                | listed, not built                                                                                                                                                                                                   | —                                                                                                                                                                                                                                                                                                                             | —                    |

Notes on these sub-items:

- **Welcome, measured in a real window:** it shows New Project, Open and Open File. New File (`file.newUntitled`) has no native action. Clone Repository and Remote Development are not registered in that window; I found no `registry.handle` for `vcs.clone` or `remote.connect`.
- **`.env` files:**
  - Which files and in what order: the configuration's files resolved below the project root, blank entries skipped, later files winning. The configuration's own variables win over all of them.
  - Reading fails: the launch stops with "Cannot start debugging". The backend's error names the file path and the I/O error, never a value (`crates/ide-run/src/envfile.rs:124`).
  - Not logged: measured by grep, which finds no Debug or tracing output of a launch, a `BackendCall` or an environment in `app-gpui/src/debugger`, `app-gpui/src/workbench/debug*`, `crates/ide-dap/src` or `crates/ide-host/src/ops/debug*`.
  - Not tested: the driver's `RunEnvFiles` arm is not run by any test (the debugger tests use a stand-in backend). Hypothesis: it reaches the backend unchanged. Confirm with an end-to-end test using the fake debug adapter and a `.env` file in the temp project.
- **Fork failure text:** the text is tested in `ReplyActions` only; the two `Purpose::Failure` sites in `workbench/agents.rs` share the constant but are not driven by a test.
- **Plugin running state, why not started:** it needs a count of running commands per action and a spinner on three toolbar hosts (`editor.title`, `mainToolbar`, `commit.message`), which is more than one pass allowed.
- **Unity:** native has Unity only as an attach kind (`app-gpui/src/debugger/attach.rs:25`).

## 3. Rows whose missing clause is now proven

| Row                                                 | Missing clause now proven                                                               | Test                                                                                                                                                                                 |
| --------------------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:code.applyQuickFix`                         | the fix applied names the problem at the caret                                          | `...proofs41_stale_and_matching::uses_the_matching_fix_for_apply_quick_fix`                                                                                                          |
| `action:code.quickDefinition`                       | an answer arriving after the document changed is dropped                                | `...proofs41_stale_and_matching::quick_definition_drops_an_answer_that_arrives_after_the_text_changed`                                                                               |
| `action:db.submitChanges`                           | acts on the focused console grid; confirms the cell being edited                        | `...proofs43_database_results::submit_revert_and_go_to_column_reach_a_focused_console_result`                                                                                        |
| `action:db.revertChanges`                           | acts on the focused console grid (cell confirm not tested for Revert, so partial)       | same                                                                                                                                                                                 |
| `action:db.goToColumn`                              | acts on the focused console grid (cell confirm not tested for Go to Column, so partial) | same                                                                                                                                                                                 |
| `action:db.execute`                                 | a request whose project changed while the sources loaded is dropped                     | `...proofs43_database_results::a_statement_waiting_for_the_data_sources_is_dropped_when_the_project_changes`                                                                         |
| `action:file.openInNewWindow`                       | the trust question comes before the new window                                          | `workbench::app_actions::tests::open_in_new_window_opens_no_window_when_the_user_chooses_dont_open` and `...::a_project_opening_in_a_new_window_is_asked_about_in_this_window_first` |
| `action:toolwindow.hideActive`                      | clears an orphaned maximized side                                                       | `workbench::tests::proofs_tool_windows::hide_active_tool_window_clears_a_maximized_side_whose_window_went`                                                                           |
| `action:file.openFile`                              | the Welcome screen's Open File button runs it                                           | `workbench::welcome::tests::the_buttons_follow_the_registered_actions`, `...::shows_the_landing_page_without_recent_projects`                                                        |
| `action:http.openEndpoint`                          | uses the module's server address                                                        | `workbench::http_client::tests::open_in_http_client_takes_the_server_address_of_the_endpoints_module`                                                                                |
| `action:debug.startConfig`                          | `.env` files loaded (the browser debugging clause is still missing, so partial)         | `debugger::actions::tests::a_debug_launch_loads_the_configurations_env_files_under_its_own_variables`                                                                                |
| `action:terminal.openAt`                            | reads Terminal and shows only for a task with its own terminal                          | `agents::task_actions::tests::offers_terminal_for_worktree_or_off_root_tasks_and_hides_it_for_local_checkout_tasks_at_the_project_root`                                              |
| `action:agents.forkTask`, `action:agents.forkReply` | failure text, partial: tested through `ReplyActions` only, not through the action       | `agents::reply_actions::tests::reports_a_failed_fork_as_a_failed_fork`                                                                                                               |

Not changed:

- `action:db.explainPlan`, `action:db.queryHistory`: same mechanism as `db.execute`, read but not tested.
- `action:db.newDataSource`: it waits for nothing, so the clause does not apply to the native action.
- `action:project.previewRestricted`, `action:project.revokeTrust`: remote only.
- `action:run.reloadConfigs`: Unity not built.
- The plugin `points.tsx:200` row: not started.

Renamed tests cited in `parity.json`:

- `workbench::app_actions::tests::a_project_opening_in_a_new_window_is_asked_about_in_that_window` is now `..._in_this_window_first`. The old name is cited by `action:project.trust`, `ipc:project_trust_status`, `point.registerOverlay:project.trustDialog` and `module:src/platform/projectTrust.ts`; their citations need the new name.
- `open_is_always_offered_and_the_other_buttons_when_their_action_is_registered` is now `open_and_open_file_are_always_offered_...`; no row cites it.

## 4. Hypotheses

- The driver's `RunEnvFiles` arm reaches `run_env_files` unchanged. Confirm with an end-to-end debug test using a `.env` file in the temp project.
- `db.explainPlan` and `db.queryHistory` drop a request after a project change. Confirm with the project-change test run with `db.explainPlan` and with `db.queryHistory`.

## 5. False or stale in the brief and notes

- Item 4 says Quick Definition drops an answer "after the caret moved or the popup closed". At HEAD the webview drops it when the document changed or the editor is gone (`hover.ts:364`); a caret move does not count, and no popup exists before the answer. I followed the webview.
- The report's line numbers no longer match: `apply_quick_fix` and `quick_definition` were found by name, not at the quoted lines.
- Found while reading, not fixed (outside this slice):
  - The task menu says "Copy to a New Task" where the webview says "Fork" (renamed in 9e46d61dc).
  - The native endpoint host lacks the active Spring profile files (`application-<profile>.*`, upstream 614301188).
  - Attaching a project to the workspace natively answers its trust step without asking (`app_shell.rs`, `OpenStep::ConfirmTrust` calls `trust_confirmed(Ok(()))` directly), where the webview asks (`windows.ts`, `attachProject`).
  - Unregistering a maximized tool window does not clear the maximized side natively; the webview clears it at unregistration too (68a9bf37e). Only Hide Active was in this brief.

## Files

- Changed (from `git diff --name-only`), all under `app-gpui/`: `build.rs`, `src/agents/{reply_actions.rs, task_actions.rs, task_actions/tests.rs, task_list_view.rs}`, `src/app_models/windows.rs`, `src/app_views/about_dialog.rs`, `src/database/results_view.rs`, `src/debugger/{actions.rs, driver.rs, effects.rs}`, `src/editor_lsp/{diagnostics.rs, hover_view.rs, intentions.rs}`, `src/features_a/http_client/endpoints.rs`, `src/settings_view/custom_controls.rs`, `src/workbench/{agents.rs, app_actions.rs, app_shell.rs, database.rs, database_extras.rs, editor_extras.rs, editor_language.rs, http_client.rs, http_client/tests.rs, mod.rs, refactor_actions.rs, tool_windows.rs, welcome.rs, welcome_model.rs, windows.rs}`, `src/workbench/tests/{proofs13_shell.rs, proofs19_settings_misc.rs, proofs21_shell.rs, proofs_tool_windows.rs}`.
- New, untracked, declared with `mod`:
  - `app-gpui/src/workbench/editor_language/tests/proofs41_stale_and_matching.rs`
  - `app-gpui/src/workbench/tests/proofs43_database_results.rs`
- No file named in `undeclared_now.txt` was touched. `docs/gpui-port/gen-data.mjs` was not edited.
