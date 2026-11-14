## 1. Suite, clippy, scope

- **Suite ×5** (`cargo test --lib`): 6758 passed, 0 failed, 0 ignored in every run (96.8 s, 110.8 s, 96.3 s, 97.1 s, 96.9 s). `cargo clippy --all-targets -- -D warnings` is clean after the last edit.
- **The first two full runs hung, and that took a fix in `app-gpui/src/backend.rs`.** About 17 `vcs_operations` tests hung, and `plugins::tests::a_plugins_command_runs_its_wasm_on_the_editors_selection` failed. Measured with `sample`: the test binary had 8052 threads against macOS's `kern.num_taskthreads` limit of 8192. 6422 of them were `openidea-backend` workers, about 640 test runtimes with 10 workers each that live until the binary exits. Tokio's `Builder::build` could then no longer start threads and blocked. The fix: under `#[cfg(test)]`, `worker_threads(2)` for the backend runtime. I killed the hung binary by its PID.
- **Compile errors:**
  - Part 0: 0.
  - Part 1: 0.
  - Part 2: 3, all E0599 (missing method) in `proofs6_database.rs`: `database_console` ×2 and `database_history_dialog`. Both exist only in the undeclared draft `workbench/database_extras.rs`.
- **Scope:** No command of mine touched a file outside scope. Running `rustfmt` on `workbench/mod.rs` and its siblings changed no other file (checksums of every `.rs` file before and after were identical). No `proofs7_*` file was touched.

## 2. Part 0: a project's plugins stay in its window

**Before the fix (measured):** with window A trusted and carrying `hello` plus `settings-pages`, window B showed all six of A's contributions: listed, action enabled, Tools menu row, status item `plugin.hello/hello`, tool window `plugin.hello.greetings`, settings page `plugins.settings-pages`. This was the same with B trusted and B restricted. Running `plugin.hello.word-count` from B ran (`execute_action` returned true) and loaded A's plugin code (status Active), in both cases.

**Backend (from reading):** `PluginHost::run_command` / `code_call` do not check the window's project. The only scoping is `list(Some(root))`. The adapter's `authorize_command(label, "plugin_run_command")` refuses only windows whose project is untrusted, which includes LightEdit windows.

**The fix:**

- `plugins.rs`:
  - Keeps `snapshot` (`list(None)`) plus one `list(Some(root))` per open trusted root. The merge into one shared listing is gone.
  - `listing_for(root)`, `window_listing(window)` and `context_listing(ctx)` give each window its own listing; no project or a restricted one falls back to the installed listing.
  - A project plugin's command action carries its project root.
  - `run_command` refuses a plugin that is not in the window's listing.
- `platform/actions.rs`: a new `ActionMeta.project` field (`#[serde(skip)]`) and `is_action_in_window`, checked inside `is_action_enabled`.
- `everywhere/view.rs`: Find Action leaves out actions from another project's plugins.
- `workbench/mod.rs`:
  - Publishes a `projectRoot` context key.
  - `on_release` now closes the window's backend project (`close_project_detached`) and calls `Plugins::release_project`.
- Every listing reader goes through the window's listing: `plugin_contributions.rs` (Tools items, slot items, menu items, settings pages, gutter-provider fingerprint), `plugin_runtime.rs` (`plugin_listing`, status items filtered to the window's plugins), `plugin_tool_windows.rs`, `plugins_window.rs`, `menu.rs`, `context_menus.rs`, `settings_window.rs`.

**Tests**, in `app-gpui/src/workbench/plugins_window/tests/project_plugins_per_window.rs`:

- `…::a_trusted_projects_own_plugins_contribute_nothing_to_another_trusted_projects_window` — also runs the command in A to show it still works there.
- `…::a_trusted_projects_own_plugins_contribute_nothing_to_a_restricted_window`
- `…::closing_a_window_unloads_only_its_own_projects_plugins` — B closes and A keeps everything; A closes and its plugin and action unload while C keeps its project and the installed plugin `greeter`, which shows in every window.

**Closing a non-last window (measured, with the hook disabled):** the window's backend project stayed open, so its plugins stayed loaded. `request_close` decides the close and `on_release` used to release nothing. It does now.

**Left as it was, decision needed:** I tried adding `authorize_command` to the action path. It broke the existing LightEdit plugin test, because LightEdit projects are untrusted. So I removed it. The webview's invoke guard (`src-tauri/src/lib.rs:170`) would refuse `plugin_run_command` there, but natively an installed plugin's command still runs in restricted and LightEdit windows. Plugin themes stay app-wide, including a trusted project's own.

## 3. Part 1: range hint on number fields

- **Where:** `settings_view/controls.rs` (`range_tooltip`, `with_range_tooltip`) on standard number fields. Also `pages/mod.rs` `draw_field` for own-control number fields, and `pages/editor_font.rs` for the custom weight (100–900, as `font.tsx:100`).
- **What it shows:** `tx_built(["Enter a value between ", " and ", ""], [min, max])` as the field's tooltip, only while the value is invalid.
- **Tests:**
  - `settings_view::custom_controls::tests::a_number_out_of_range_on_a_standard_page_tells_its_range_in_a_tooltip` — page `general`, `files.localHistoryDays`.
  - `settings_view::custom_controls::tests::a_number_out_of_range_on_a_page_of_its_own_controls_tells_its_range_in_a_tooltip` — `build.console`, "Enter a value between 16 and 1048576".
- **Pages whose only recorded gap was this hint** (each also has "the rest compared by reading"): `settings:build.console`, `settings:build.debugger.dataViews`, `settings:tools.terminal`, `settings:vcs.commit`, `settings:vcs.confirmation`, `settings:vcs.git`.

## 4. Part 2: proof tests

All declared tests pass. "Done" below means the test passes and proves the key.

| Test (path as `--list` prints)                                                                                                      | Keys                                                                 | Status                                        |
| ----------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- | --------------------------------------------- |
| `workbench::plugins_window::tests::proofs6_plugins::the_marketplace_tab_lists_the_plugins_of_the_local_catalog`                     | `ipc:plugin_catalog`                                                 | done                                          |
| `…proofs6_plugins::installing_a_package_file_from_disk_unpacks_it_and_installs_its_plugin`                                          | `ipc:plugin_unpack_package`                                          | done                                          |
| `…proofs6_plugins::roll_back_returns_an_updated_plugin_to_its_previous_version`                                                     | `ipc:plugin_roll_back`                                               | done                                          |
| `…proofs6_plugins::a_permissions_row_revokes_and_allows_the_permission_on_the_host`                                                 | `ipc:plugin_set_capability`                                          | done                                          |
| `…proofs6_plugins::review_permissions_grants_a_plugin_waiting_for_consent`                                                          | `ipc:plugin_grant`                                                   | partial                                       |
| `…proofs6_plugins_market::the_marketplace_lists_the_indexs_plugins_and_install_downloads_and_installs_one`                          | `ipc:plugin_market`, `ipc:plugin_market_download`                    | done                                          |
| `…proofs6_plugins_market::plugin_repository_shows_the_stored_url_and_saves_a_new_one_the_host_keeps`                                | `ipc:plugin_market_url`, `ipc:plugin_set_market_url`                 | done                                          |
| `workbench::plugin_runtime::tests::proofs6_plugin_runtime::a_plugin_command_runs_an_ide_action_in_its_window_and_hears_the_outcome` | `ipc:plugin_answer_command`                                          | done                                          |
| `workbench::tests::proofs6_workbench::the_welcome_screen_shows_the_stored_project_groups_with_their_projects`                       | `ipc:recent_project_groups`                                          | done                                          |
| `settings_view::custom_controls::tests::proofs6_custom_controls::the_shell_path_list_offers_the_installed_shells`                   | `ipc:term_available_shells`                                          | done                                          |
| `workbench::debug::tests::proofs6_debug::the_frame_menu_offers_reset_frame_only_when_the_adapter_can_restart_frames`                | `ipc:debug_capabilities`                                             | done                                          |
| `…proofs6_debug::the_thread_chooser_lists_every_thread_the_adapter_reports`                                                         | `ipc:debug_threads`                                                  | done                                          |
| `…proofs6_debug::a_breakpoint_the_adapter_reports_unbound_shows_its_message_in_the_breakpoints_dialog`                              | `ipc:debug_breakpoint_states`                                        | done                                          |
| `…proofs6_debug::a_breakpoint_the_adapter_reports_unbound_shows_a_grey_dot_in_the_gutter` (new)                                     | gutter difference                                                    | done                                          |
| `…proofs6_debug::the_breakpoints_dialog_lists_the_adapters_exception_filters_and_a_ticked_one_reaches_it`                           | `ipc:debug_exception_filters`, `ipc:debug_set_exception_breakpoints` | done                                          |
| `…proofs6_debug::a_method_breakpoint_set_during_a_session_reaches_the_adapter_and_shows_it_verified`                                | `ipc:debug_set_function_breakpoints`                                 | done                                          |
| `…proofs6_debug::add_field_watchpoint_on_a_local_sets_the_watchpoint_the_adapter_names`                                             | `ipc:debug_data_breakpoint_info`, `ipc:debug_set_data_breakpoints`   | done                                          |
| `…proofs6_debug::listening_for_a_remote_jvm_waits_on_its_port_until_the_jvm_connects`                                               | `ipc:debug_listen_for_jvm`                                           | done                                          |
| `…proofs6_debug::cancel_on_the_listening_notice_stops_listening_for_the_remote_jvm`                                                 | `ipc:cancel_request`                                                 | partial (debugger call site only)             |
| `…proofs6_debug::a_php_debug_start_warns_when_the_php_chosen_in_settings_loads_no_xdebug`                                           | `ipc:sdk_php_effective`, `ipc:sdk_php_xdebug`                        | done                                          |
| `…proofs6_debug::a_dotnet_debug_start_debugs_the_build_output_the_project_names`                                                    | `ipc:run_dotnet_resolve`                                             | done                                          |
| `workbench::tests::proofs6_database::the_tree_marks_a_data_source_connected_when_the_backend_already_holds_its_connection`          | `ipc:db_connected`                                                   | done                                          |
| `workbench::tests::proofs6_database::go_to_ddl_writes_the_tables_definition_to_a_file_and_opens_it`                                 | `ipc:db_export_file`                                                 | done                                          |
| `workbench::tests::proofs6_agents::picking_a_setting_in_the_composer_changes_it_in_the_running_agent`                               | `ipc:agent_set_config`                                               | done                                          |
| `workbench::tests::proofs6_agents::a_finished_turn_lists_the_file_the_agent_edited_among_the_tasks_changes`                         | `ipc:agent_changes`                                                  | partial (asserts the store, no view shows it) |

**Changed or left out:**

- **Moved to the new undeclared file `workbench/tests/proofs6_database_console.rs`:** `a_sqlite_console_runs_in_the_default_schema_its_completion_model_names` (`ipc:db_completion`) and `delete_in_query_history_removes_the_statement_from_the_stored_history` (`ipc:db_remove_history`). The console and the `db.queryHistory` action exist only in `database_extras.rs`, which is not in the build.
- **`proofs6_attach.rs` (`ipc:workspace_attach`):** undeclared by design, as briefed.
- **`proofs6_app_actions.rs` (`ipc:vcs_init`):** undeclared. `ide_vcs::init` runs `git init -q` with the process environment. It reads `/etc/gitconfig`, `~/.gitconfig` and `$XDG_CONFIG_HOME/git/config` (`init.defaultBranch`, `init.templateDir`, `core.hooksPath`) and the system template folder. It cannot be isolated without changing the process environment.

**Safety checks, from reading the code:**

- **Marketplace tests:** use local file paths only. `read_url` reads a path from disk, and the default official index URL is set only in release builds.
- **PHP and .NET:** `php` and `dotnet` are not installed on this machine (measured with `which`). The interpreter and `dotnet` are scripts the tests write.
- **JVM:** the backend listens on 127.0.0.1. `JavaDebugServerPort` answers the debug-server step, and the java adapter only connects to a `debugServer` port. Hypothesis: no JVM is started; confirm with `ps` during `cargo test --lib proofs6_debug::listening`.

**Product fix:** a breakpoint the debugger reports unbound now gets its own `MarkKind::UnverifiedBreakpoint`. Its row stays tinted and the gutter shows a grey `fg-gutter` dot, as `src/features/debugger/editor/editor.module.css:45` and `breakpointGutter.ts:55` do. Files: `editor/marks.rs`, `editor/view.rs`, `workbench/debug.rs`.

**Registered ids:** `workbench::tests::smoke_registered_actions::every_action_registered_in_a_project_window_is_known_to_the_manifest` stays green. Its 570 ids are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/registered_ids.txt`.

## 5. Notes that turned out false, and findings outside scope

- **False:** "proven by `workbench::database_extras::tests`" (`db_set_manual_commit`, `db_finish_transaction`, `db_explain`, `db_history`). That module is not built.
- **False:** `proofs6_database`'s console accessors exist in the build.
- **Outside scope, measured:** test runtimes are never dropped, so threads build up over the whole run. Before my cap the suite was within about 140 threads of the macOS limit (8052 of 8192). Other leftovers seen in the sample: 482 `openidea-fs-watch`, 241 `openidea-db-history`, 239 `openidea-index`.
- **Outside scope, hypothesis:** the webview refuses plugin commands in LightEdit windows. Confirm by running a plugin command in a LightEdit window of the Tauri app.
