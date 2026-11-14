# proofs6: debugger, database and agents backend operations

Nothing here was compiled or run. All statements about native behaviour come from reading the code; they are hypotheses until the integrator's run confirms them. For every command with a new test, the Tauri adapter in `src-tauri/src/commands/` was read and calls the same `ide_host::ops::…` function as the native path (`cancel_request`: `AppState::cancel_request` / `Host::cancel_request`; `db_export_file`: `std::fs::write` in both); the arguments match the webview's (`run_dotnet_resolve(config, true)` when debugging, `debug_data_breakpoint_info` with the selected frame, `sdk_php_xdebug(php, launch.cwd)`).

## Files written

- `app-gpui/src/workbench/debug/tests/proofs6_debug.rs` (533 lines, 10 tests, two of them `#[cfg(unix)]`)
- `app-gpui/src/workbench/tests/proofs6_database.rs` (186 lines, 4 tests)
- `app-gpui/src/workbench/tests/proofs6_agents.rs` (159 lines, 2 tests)

## `mod` lines to add

1. `app-gpui/src/workbench/debug.rs`, inside `pub(super) mod tests`, after the anchor `    mod proofs3_debug;`, add `    mod proofs6_debug;`.
2. `app-gpui/src/workbench/mod.rs`, inside `mod tests`, after the anchor `    mod proofs5_run;`, add:
   ```rust
       mod proofs6_agents;
       mod proofs6_database;
   ```

## Fixture changes

None. The debug tests extend the fake adapter inside the test file: they write their own script over `fake_dap.py` before the first session, as `proofs_debug.rs` already does (`FAKE_ADAPTER.replace(...)`). The agent tests overwrite `fake_agent.py` after `install_fake_agent`, as `proofs_agents.rs` does. No accessor is added; every accessor used exists (`frame_menu`, `thread_menu`, `thread_label`, `variable_menu`, `open_menu_on`, `run_variable_command`, `exception_groups`, `rows`, `notifications().balloons`, `run_notification_action`, `database_console`, `database_history_dialog`, `ConsoleView::bar`, `ResultTab::origin.model`, `ComposerView::chips/picker_entries/send`, `AgentsStore::changes`).

## Tests and the rows they prove

### `proofs6_debug.rs` (module `workbench::debug::tests::proofs6_debug`)

- `the_frame_menu_offers_reset_frame_only_when_the_adapter_can_restart_frames` → `ipc:debug_capabilities`: **done**. Proof: "`workbench::debug::tests::proofs6_debug::the_frame_menu_offers_reset_frame_only_when_the_adapter_can_restart_frames`: Reset Frame in the frame menu is disabled once the fixture adapter's capabilities are read (`caps.is_some()`) and enabled in a session of an adapter announcing `supportsRestartFrame`; the menu reads `session.caps`, which only `debug_capabilities` fills."
- `the_thread_chooser_lists_every_thread_the_adapter_reports` → `ipc:debug_threads`: **done**. Proof: "the thread chooser lists `main` and `worker`, the two threads the adapter's `threads` answer names (the fixture adapter has one)."
- `a_breakpoint_the_adapter_reports_unbound_shows_its_message_in_the_breakpoints_dialog` → `ipc:debug_breakpoint_states`: **done** (but see the webview difference below). Proof: "the adapter sends a `breakpoint` changed event after configuration; the Breakpoints dialog row of the line breakpoint gets `verified: false` with the adapter's message `No code at this line`, which only the state re-read through `debug_breakpoint_states` after `BreakpointsChanged` carries."
- `the_breakpoints_dialog_lists_the_adapters_exception_filters_and_a_ticked_one_reaches_it` → `ipc:debug_exception_filters`, `ipc:debug_set_exception_breakpoints`: both **done**. Proof: "before any session the dialog lists no `fake-dap` filter group; with the session running it lists `Raised Exceptions` (off), the adapter's `exceptionBreakpointFilters`, read through `debug_exception_filters`; ticking it (the dialog's own `SetExceptionFilter` event, emitted on the dialog as the checkbox `breakpoints-exception-<group>-<filter>` does; the group index of `fake-dap` among the built-in groups is not known to the test, so it is not clicked) makes the adapter print `exception filters: raised` in the debug console, which it does only on a `setExceptionBreakpoints` request carrying that filter."
- `a_method_breakpoint_set_during_a_session_reaches_the_adapter_and_shows_it_verified` → `ipc:debug_set_function_breakpoints`: **done**. Proof: "Toggle Method Breakpoint on `def run()` while paused makes the adapter print `function breakpoints: run` (its `setFunctionBreakpoints` handler) and the dialog's method row turns verified, the state `debug_set_function_breakpoints` answers (without a supporting session the backend answers unverified)."
- `add_field_watchpoint_on_a_local_sets_the_watchpoint_the_adapter_names` → `ipc:debug_data_breakpoint_info`, `ipc:debug_set_data_breakpoints`: both **done**. Proof: "Add Field Watchpoint on the local `line` lists a watchpoint whose data id is `line@main`, the id the adapter's `dataBreakpointInfo` answer invents, labelled `line`; the adapter prints `data breakpoints: line@main` (its `setDataBreakpoints` handler) and the row turns verified." The menu entry being offered also re-proves `debug_capabilities` (`supportsDataBreakpoints`).
- `listening_for_a_remote_jvm_waits_on_its_port_until_the_jvm_connects` → `ipc:debug_listen_for_jvm`: **done**. Proof: "a Remote JVM Debug configuration in listen mode shows the listening balloon and its port becomes taken (a bind on it fails); a TCP connection to the port makes the balloon go, which happens only when `debug_listen_for_jvm` answers (`jvm_connected` expires the notice)." The Java attach that follows (read: `ReadDir` scan, `DebugServer`, `ReadProjectFiles`, then a `java-debug` attach to the debug server's port; no `ResolveLaunch` on the attach plan) is answered by the `JavaDebugServerPort` test global with a closed port, so no jdtls is looked for and nothing is downloaded; hypothesis: the attach then fails at once with a refused connection (check: the notices after the test).
- `cancel_on_the_listening_notice_stops_listening_for_the_remote_jvm` → `ipc:cancel_request`: **done for the debugger's call site** (the row's other call sites, code completion and the searches, are not exercised here). Proof: "Cancel on the listening balloon (through `run_notification_action`, the button's own path) makes the balloon go and frees the port, which only the cancellation of the listen's `RequestId` through `host.cancel_request` does; no `Cannot start debugging` notice."
- `a_php_debug_start_warns_when_the_php_chosen_in_settings_loads_no_xdebug` (unix) → `ipc:sdk_php_effective`, `ipc:sdk_php_xdebug`: both **done**. Proof: "with `languages.php.interpreter` set to a script printing `8.3.0`, Run › Debug of a `php main.php` configuration under `php-debug` raises `Xdebug is not loaded in PHP 8.3.0` whose body starts with that interpreter's path: the path comes from `sdk_php_effective` (without it the launch has no `runtimeExecutable` and `sdk_locate_tools` would look on PATH instead), the version from running it through `sdk_php_xdebug`."
- `a_dotnet_debug_start_debugs_the_build_output_the_project_names` (unix) → `ipc:run_dotnet_resolve`: **done**. Proof: "a `dotnet` configuration under `netcoredbg` whose `dotnet` is a script answering `msbuild … -getProperty:TargetPath` with `main.cs` stops at the breakpoint of `main.cs` with the frame `main, main.cs:2`: the launch's program is the `targetPath` only `run_dotnet_resolve` sets."

### `proofs6_database.rs` (module `workbench::tests::proofs6_database`)

- `the_tree_marks_a_data_source_connected_when_the_backend_already_holds_its_connection` → `ipc:db_connected`: **done**. Proof: "the statements run through the backend leave `app` connected (`ops::db::connected` == [`s1`]); showing the Database tool window marks `s1` connected in the tree without anything being expanded, and `s2` not: the store's `LoadSources` reads `db::connected`."
- `a_sqlite_console_runs_in_the_default_schema_its_completion_model_names` → `ipc:db_completion`: **done** for what native does with it; see the webview difference. Proof: "the console of the SQLite source shows `main` as its schema (the completion model's `default_schema`; no pick was made), and its result carries the completion model listing `main.users`."
- `go_to_ddl_writes_the_tables_definition_to_a_file_and_opens_it` → `ipc:db_export_file`: **done**. Proof: "Go to DDL on `users` opens a `…users.sql` file outside the project whose content on disk holds `create table users`: the store's `DbOp::WriteFile` (the adapter's `std::fs::write`) wrote it." This is the webview's use of `db_export_file` in `store.ts:851` (DDL to file); the CSV export of the webview's `ExportDialog` is not this path.
- `delete_in_query_history_removes_the_statement_from_the_stored_history` → `ipc:db_remove_history`: **done**. Proof: "Delete in Browse Query History on `select id from users` removes it from `ops::db::history` of `s1` while `select name from users` stays."

Rows already proven by existing tests in `app-gpui/src/workbench/database_extras.rs` (`mod tests`), which the map did not look at (it mapped only `workbench::tests`); record them, no new test:

- `ipc:db_set_manual_commit`, `ipc:db_finish_transaction`: **done** by "`workbench::database_extras::tests::manual_commit_keeps_changes_pending_until_commit_or_roll_back`: Tx: Manual keeps an insert invisible to a second source on the same file, Roll Back discards it, Commit makes the next one visible" (`run_op` calls `db::set_manual_commit` / `db::finish_transaction`, as the adapters do).
- `ipc:db_explain`: **done** by "`workbench::database_extras::tests::the_console_banner_and_the_query_plan_tab_follow_the_runs`: the Query Plan tab draws the plan rooted in `Select` with at least two nodes" (with `workbench::tests::proofs_database::explain_plan_logs_the_explained_statement_and_its_plan_nodes`).
- `ipc:db_history`: **done** by "`workbench::database_extras::tests::query_history_lists_the_statements_run_and_pastes_one_into_the_console`: the History dialog lists the two statements run, newest first".

### `proofs6_agents.rs` (module `workbench::tests::proofs6_agents`)

- `picking_a_setting_in_the_composer_changes_it_in_the_running_agent` → `ipc:agent_set_config`: **done**. Proof: "the fake agent offers `mode` (Ask/Code); picking Code from the composer's chip (`composer-config-N`, `composer-picker-1`) shows Code and the agent's next answer is `Mode code`, which it says only after `session/set_config_option` set it (the backend updates the chip itself, so the chip alone would not prove it)."
- `a_finished_turn_lists_the_file_the_agent_edited_among_the_tasks_changes` → `ipc:agent_changes`: **partial**. Proof: "after a turn in which the agent appended a line to `README.md` of the repository, the store's changes of the task are `[README.md, +1, -0]`, from `ops::agent::changes` on the turn's end." Missing: native has no view that shows `AgentsStore::changes` (no Changes list in the task view; `changes()` has no reader outside the store), so no user-visible state can be asserted. Done once a Changes view reads it and the test asserts its rows.

## Commands not issued by the native code (rows "not called")

- `ipc:debug_send_input`: `DebugState::send_program_input` has no caller outside a `state.rs` unit test; the Debug tool window's Console tab (`DebugConsolePane`) draws lines only, with no input line. Missing: the debug console input that types into a program started with `ProgramInput` (webview `DebugConsole.tsx:265`, `readsProgramInput` / `sendProgramInput`).
- `ipc:debug_step_in_targets`, `ipc:debug_step_in_target`: `debug.smartStepInto` is not in `DEBUG_ACTIONS` (`workbench/debug.rs:48`, "Smart Step Into … have no view here yet"), and nothing draws `overlays.smart_step`. The menu entries (`menu_spec.rs:576`, `window_view.rs:252`) name an unregistered action. Missing: the action's registration and the Smart Step Into chooser.
- `ipc:debug_attachable_processes`: `debug.attachToProcess` is not registered and nothing draws `overlays.attach`. Missing: the action and the Attach to Process popup.
- `ipc:db_connect`: `DbStore::connect` and `DbStore::plugin_connect` have no caller; expanding a source in the tree issues `DbOp::Schemas` directly (the backend connects inside it). Missing: the webview's connect-first expansion (`DatabaseView.tsx:309/313`, `connect(id)` then `loadSchemas`) and the plugin data source connect (`features/database/index.ts:140`).
- `ipc:agent_unqueue`: `AgentsStore::edit_queued` has no caller. `ipc:agent_send_queued_now`, `ipc:agent_stop_background`: no `AgentCall::SendQueuedNow` / `AgentCall::StopBackground` is built anywhere (only `run_call` matches them). Missing: the queued messages strip of the task view (Edit, Send now, Remove) and the background tasks' Stop.

## Commands left out, with why

- `ipc:java_resolve_launch`, `ipc:java_build_workspace`, `ipc:debug_redefine_classes`: need jdtls with the java-debug bundle (resolve, build, hot swap all go through it); no fake jdtls fixture exists. `ipc:java_debug_server`: same, and in a test build the `JavaDebugServerPort` global, when set, answers in place of `java_debug_server` (`driver.rs:819`), so no test of the native path can reach the backend function with that global; without it, jdtls is needed.
- `ipc:plugin_prepare_debug_run`: needs a plugin (wasm component) whose `prepare_debug_run` changes the configuration; not written here.
- `ipc:sdk_locate_tools`: issued only when the PHP launch has no `runtimeExecutable` (no PHP in Settings); it then looks for `php` on the machine's PATH, which a test cannot control without changing the process environment for every test.
- `ipc:db_databases`: issued only for PostgreSQL sources that list databases (`tree_view::lists_databases`) and SQL Server consoles; needs a server.
- `ipc:db_schema_objects`: for SQLite it answers no routines and no sequences (`SchemaObjects` holds only those; views come from `db::tables`, triggers from table details), so nothing visible depends on it; needs PostgreSQL. The map's todo (view and trigger) was wrong.
- `ipc:db_set_password`, `ipc:db_set_ssh_secret`, `ipc:db_trust_ssh_host`: need a password-protected server and an SSH server; a stored password also goes to the secret store.

## Behaviours that differ from the webview

1. Unverified breakpoints in the gutter: the webview draws an unverified marker; native `sync_debug_marks_of` (`workbench/debug.rs:824-831`) maps every look whose `tints_line()` is true, `BreakpointLook::Unverified` included, to `MarkKind::BreakpointLine`, so the editor shows a bound and an unbound breakpoint alike. The dialog shows the difference, which is what the test asserts. Fix (outside my write scope, `editor/marks.rs` is the other agent's): add `MarkKind::UnverifiedBreakpoint` with its own paint, then
   ```diff
   -                    let active = marker.look.tints_line();
   -                    let kind = if active { MarkKind::BreakpointLine } else { MarkKind::InactiveBreakpoint };
   +                    let kind = match marker.look {
   +                        BreakpointLook::Unverified => MarkKind::UnverifiedBreakpoint,
   +                        look if look.tints_line() => MarkKind::BreakpointLine,
   +                        _ => MarkKind::InactiveBreakpoint,
   +                    };
   ```
   and a test asserting the mark kind on the unbound line (the same fixture as `a_breakpoint_the_adapter_reports_unbound_…`).
2. SQL completion: the webview uses `db_completion` to complete table and column names in the console editor (`store.ts:634`); native uses the model only for the console's default schema and the result filter. Missing: completion in `.sql` consoles.
3. Expanding a data source: webview connects first (`db_connect`, which also runs the password / SSH remedies), native lists schemas directly. Same tree for SQLite.
4. Agent changes: webview's task Changes view lists `agent_changes`; native has no such view (see `agent_changes` above).

## Tests most likely to fail on first run

1. `picking_a_setting_in_the_composer_changes_it_in_the_running_agent`: assumes the Mode chip's text contains the current value's name (`Ask`/`Code`) and that the picker lists exactly `["Ask", "Code"]` with no section row, so `composer-picker-1` is Code. If `chips()` shows only the label `Mode`, change the waits to compare `chips()` before and after. If `session/new` options for category `mode` are grouped with an allow-all switch (`Picker::ModeAllowAll`), the entries may differ.
2. `a_dotnet_debug_start_debugs_the_build_output_the_project_names`: `adapter::find_config` takes the first language declaring `netcoredbg`; bundled `csharp` and `fsharp` both do, so the test replaces both with user definitions. Assumes a user language replaces the bundled one entirely (no C# language server starts) and that breakpoints may be placed in `main.cs` (the language has a debugger). `resolve` clears the environment (`include_system_env` false): the script has an absolute `#!/bin/sh`, so it should still run.
3. `listening_for_a_remote_jvm_…` / `cancel_on_the_listening_notice_…`: rely on the listening notice being shown as a balloon (`balloons(false)`); if the debug group's balloons are hidden by default, use `items()` plus `balloon` for the presence check. The cancel test's `port_taken` probe binds the very port the backend is about to bind: if the probe holds it at that moment, the listen fails (a `Cannot start debugging` notice, which the test asserts against) — a rare flake; the listen test connects with retries instead. `port_taken` relies on a second bind of a listening port failing (true on Linux and macOS with `SO_REUSEADDR`). Balloons: `Notifications::notify` gives one when the group's prefs allow it; the tests check presence before absence, so hidden balloons fail loudly, not silently.
4. `a_breakpoint_the_adapter_reports_unbound_…`: relies on the backend mapping the adapter's breakpoint id 1 back to the file (`breakpoint_ids`) and on the native path key matching `to_ipc(path)`; if the row keeps `verified: true`, check that `BreakpointsChanged` reaches `refresh_states` for the same path string the store keys by.
5. `a_php_debug_start_warns_…`: assumes a user `php.toml` replaces the bundled PHP (whose adapter command is `php-debug-adapter`); the warning does not depend on the session starting.
6. `the_tree_marks_a_data_source_connected_…`: asserts first that the backend kept the connection of the statements (`["s1"]`); if `save_sources` or `execute` behave otherwise, that assertion says so before the tree is looked at.
