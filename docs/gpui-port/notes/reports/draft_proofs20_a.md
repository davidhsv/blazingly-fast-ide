I wrote 8 tests in 5 new files and the notes file; none of them has been compiled or run. Each file parses under `rustfmt --check` with the repo's settings (width 120, `use_small_heuristics = "Max"`) and has no line over 120 columns. No existing file was edited. The five `mod` lines are in the notes as exact hunks, each placed after an anchor that occurs once today. No accessor hunks are needed. The two drivers being edited were re-read at the end and the parts these tests use have not moved.

**Files (tests)**

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/debug/tests/proofs20_debug.rs` (1)
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs20_database.rs` (1)
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/tests/proofs20_run_widget.rs` (2)
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/run_context/tests/proofs20_run.rs` (3, unix only)
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/build_tools/tests/proofs20_build_title.rs` (1)
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs20_a.md`

**Rows → test → expected state once it passes**

- `event:Debug` → `the_adapters_output_breakpoint_and_hot_code_replace_events_show_in_the_window` → done. Output and breakpoint-changed were already covered in `proofs6_debug`; the hot code replace "Classes changed" notice with its Reload button is new.
- `ipc:debug_redefine_classes` → no new test: the existing `proofs11_hotswap::reload_changed_classes_…` already meets the standard → done.
- `ipc:db_set_password` → `the_password_a_connection_asks_for_reaches_the_backend_and_the_retry_connects` → done. This uses a **new fixture you may want to veto**: a fake ClickHouse HTTP server on a loopback port that refuses requests until the password arrives. "Save password" stays off, because saving writes to the real OS keychain in this build.
- `ipc:sdk_python_effective` → `run_context_runs_a_python_file_with_the_projects_virtual_environment` → done.
- `action:run.debugContext` → `debug_context_debugs_a_python_file_with_the_projects_virtual_environment` → done; the `when` is read ON and OFF.
- `ipc:sdk_locate_tools` → `run_context_runs_a_script_with_the_tool_found_beside_the_project` (a stand-in `lua` in `node_modules/.bin`) → done.
- `action:run.run` → `run_runs_the_selection_with_a_project_open_and_is_unavailable_without_one` → done.
- `point.registerTitleItem:run.widget` → `the_run_widgets_rerun_stop_and_more_actions_restart_stop_and_offer_the_run_actions` → stays partial. The native More menu has no run-executor items and the widget has no "Resume Program" button; the webview has both.
- `point.registerTitleItem:build.project` → `the_build_hammer_shows_for_a_maven_project_and_builds_it_with_the_projects_maven` → done.

**Rows left out (not test-only)**

- `ipc:debug_attachable_processes`: the window never draws an Attach to Process popup.
- `ipc:debug_send_input`: the debug console never calls `send_program_input`.
- `ipc:java_build_workspace`, `ipc:java_debug_server`: in a test build the driver answers both from test stand-ins before the backend function is reached, and the real path needs jdtls. The existing hot swap tests therefore do not prove these rows.
- `ipc:db_connect`: the window never issues a connect. Expanding a source goes straight to listing its schemas, and `db.pluginConnect` is not registered. The webview connects on every expand.
- `ipc:db_databases` needs Postgres.
- `ipc:db_schema_objects` returns nothing for SQLite or ClickHouse and its error is dropped, so a test cannot see it was called.
- `ipc:db_set_ssh_secret`, `ipc:db_trust_ssh_host` need an SSH server, and Trust writes `~/.ssh/known_hosts`.
- `context:run.updatableRunning` needs a running spring-boot or Tomcat session, which goes through jdtls. Separately, the native key ignores debug sessions while the webview counts them.

**Riskiest tests**

1. **The database test:** if a system HTTP proxy catches `127.0.0.1`, or the HTTP client behaves differently with one request per connection, nothing reaches the fake server.
2. **The Python debug-context test:** it relies on the launcher answering the debugpy install probe and on the fake adapter reading `program` from the launch request. The Python interpreter row has its own run test, so it no longer depends on this one.
3. **The build hammer test:** it uses private helpers of `workbench/build_tools.rs`, which the integrator is editing right now, and Build Project has to reach the stand-in Maven. The exact More Actions labels in the widget test could also break the comparison.
