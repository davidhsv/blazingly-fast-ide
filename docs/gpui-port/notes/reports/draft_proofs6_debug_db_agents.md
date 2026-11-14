I wrote 16 tests that cover 19 of the 42 commands: 18 would become done and 1 stays partial. Nothing was compiled or run. Every claim below comes from reading the code, so treat it as a hypothesis until your integrator runs the tests. I edited no existing file, ran no git, cargo or fmt, and spawned no agents.

**Files (new):**

- `app-gpui/src/workbench/debug/tests/proofs6_debug.rs`: 533 lines, 10 tests, two of them unix-only.
- `app-gpui/src/workbench/tests/proofs6_database.rs`: 186 lines, 4 tests.
- `app-gpui/src/workbench/tests/proofs6_agents.rs`: 159 lines, 2 tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs6_debug_db_agents.md`

**`mod` lines to add:**

- In `workbench/debug.rs`, add `mod proofs6_debug;` after `    mod proofs3_debug;`.
- In `workbench/mod.rs`, add `mod proofs6_agents;` and `mod proofs6_database;` after `    mod proofs5_run;`.

For each of the 19 commands I read the Tauri adapter: it calls the same `ide_host::ops` function as the native path, with the same kind of arguments as the webview.

**Of the 42 commands:**

- **19 get a new test, 18 done and 1 partial.**
  - Debugger: `debug_capabilities`, `debug_threads`, `debug_breakpoint_states`, `debug_exception_filters`, `debug_set_exception_breakpoints`, `debug_set_function_breakpoints`, `debug_set_data_breakpoints`, `debug_data_breakpoint_info`, `debug_listen_for_jvm`, `cancel_request` (only its debugger call site), `sdk_php_effective`, `sdk_php_xdebug`, `run_dotnet_resolve`.
  - Database: `db_connected`, `db_completion`, `db_export_file`, `db_remove_history`.
  - Agents: `agent_set_config`.
  - `agent_changes` is the partial one: the test reads the agents store, because no native view shows a task's changes.
- **4 are already proven by existing tests** in `workbench::database_extras::tests`, which the map did not cover: `db_set_manual_commit`, `db_finish_transaction`, `db_explain` and `db_history`. I wrote no new tests for them.
- **0 depend on a fixture change.** The tests extend the fake adapter and the fake agent scripts inside the test files, as `proofs_debug.rs` and `proofs_agents.rs` already do. No accessor is needed either.
- **8 are not issued by the native code:**
  - `debug_send_input`: the Debug console has no input line.
  - `debug_step_in_targets` and `debug_step_in_target`: `debug.smartStepInto` is not registered and has no chooser.
  - `debug_attachable_processes`: `debug.attachToProcess` is not registered.
  - `db_connect`: no caller; expanding a source lists its schemas directly, whereas the webview connects first.
  - `agent_unqueue`, `agent_send_queued_now`, `agent_stop_background`: there is no queued-messages or background-task UI.
- **11 are left out:**
  - `java_resolve_launch`, `java_debug_server`, `java_build_workspace`, `debug_redefine_classes`: need jdtls. In a test build the `JavaDebugServerPort` global, when set, answers instead of `java_debug_server`.
  - `plugin_prepare_debug_run`: needs a plugin.
  - `sdk_locate_tools`: depends on the machine's PATH.
  - `db_databases`: PostgreSQL only.
  - `db_schema_objects`: SQLite has no routines or sequences, so the map's view-and-trigger idea doesn't apply.
  - `db_set_password`, `db_set_ssh_secret`, `db_trust_ssh_host`: need a server or SSH.

**Differences from the webview:**

- The editor gutter draws an unverified breakpoint the same as a verified one; my test asserts the Breakpoints dialog instead. The proposed fix is in the notes as a hunk, but it touches `editor/marks.rs`, which is in the other agent's scope.
- There is no SQL completion in consoles.
- There is no Changes view for agent tasks.

**Most likely to fail first:**

1. The agent setting test: it assumes the Mode chip's text names the current value and that the picker lists exactly Ask and Code.
2. The .NET test: it replaces both bundled `csharp` and `fsharp`, because both declare `netcoredbg`.
3. The cancel-listen test: its free-port probe can, rarely, collide with the backend's bind.
4. The unverified-breakpoint test: it depends on the adapter's breakpoint id 1 mapping back to the file's path.
