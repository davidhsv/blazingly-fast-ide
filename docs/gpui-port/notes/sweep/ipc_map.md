# IPC commands: native call map

Source: the committed tree (HEAD) of the worktree. 599 commands: `collect_commands!` in `src-tauri/src/commands/mod.rs` plus the raw `fonts::user_font_data`. Backend functions are the calls each adapter body makes (one level; AppState wrappers and ops wrappers followed one more level where the native side calls the callee). Native calls are non-test sites in `app-gpui/src`: files reached from `lib.rs`/`main.rs` through `mod` declarations, minus `#[cfg(test)]` items and the modules declared inside them.

18 files are not declared by any module at HEAD and count as not compiled: `build_tools/{execute,mod,model,view}.rs`, `database/{dialog_parts,history_view,import_dialog,modify_table_dialog,plan_view,truncate_dialog}.rs`, `editor/extras.rs`, `editor_lsp/reformat_dialog.rs`, `vcs/{local_history_view,revision_chooser}.rs`, `workbench/{build_tools,database_extras,diff_tools,editor_extras}.rs`. Class C rows that one of them already calls are marked "drafted".

Classes: A = native code calls the adapter backend function; B = native code reaches the same result through another function (named in `via`); C = not called natively; D = only meaningful for the Tauri/webview shell. Verdict `recorded` (outside the requested enum) = the row is already `done` in parity.json. `native_calls` are search hits; for rows already done they can include unrelated sites with the same method name.

## Counts

| Class | Count |
| ----- | ----- |
| A     | 283   |
| B     | 26    |
| C     | 240   |
| D     | 50    |

| Verdict         | Count |
| --------------- | ----- |
| not_called      | 240   |
| recorded        | 203   |
| needs_test      | 63    |
| shell_only      | 50    |
| needs_assertion | 33    |
| already_proven  | 10    |

| Class / verdict     | Count |
| ------------------- | ----- |
| A / already_proven  | 6     |
| A / needs_assertion | 28    |
| A / needs_test      | 60    |
| A / recorded        | 189   |
| B / already_proven  | 4     |
| B / needs_assertion | 5     |
| B / needs_test      | 3     |
| B / recorded        | 14    |
| C / not_called      | 240   |
| D / shell_only      | 50    |

parity.json `ipc:` rows at HEAD: 206 done, 32 partial, 361 without an entry (measured).

## Ready to record (`already_proven`)

Class B rows here need the reviewer to accept that the native call is the whole body of the adapter function (pass-through), since the rule asks for the same function.

| Command                 | Class | Status now | Proof                                                                                                                                                                                                                                                                              |
| ----------------------- | ----- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `vcs_diff_hunks`        | B     | none       | `workbench::shelf::model_tests::finds_todo_items_in_the_changed_lines_only`; the before-commit TODO check calls ide_vcs::diff_hunks (the whole body of ops::vcs::vcs_diff_hunks) and only the TODOs of the changed lines come back                                                 |
| `vcs_git_version`       | A     | none       | `settings_view::custom_controls::tests::git_is_detected_after_the_dialog_opens_and_test_runs_it`; Test runs ide_host::ops::vcs::vcs_git_version on the path and the answer line names the detected git                                                                             |
| `vcs_detected_git`      | A     | none       | `settings_view::custom_controls::tests::git_is_detected_after_the_dialog_opens_and_test_runs_it`; the Git page shows "Auto-detected: <path>" from ide_host::ops::vcs::vcs_detected_git                                                                                             |
| `term_default_shell`    | A     | none       | `settings_view::custom_controls::tests::the_login_shell_is_read_after_the_dialog_opens`; the Terminal page fills the Shell path placeholder from ide_terminal::default_shell_path, as the adapter answers                                                                          |
| `debug_restart_frame`   | A     | none       | `workbench::debug::tests::proofs_debug::force_run_to_cursor_skips_breakpoints_force_step_into_steps_and_reset_frame_restarts`; Reset Frame sends ide_host::ops::debug::debug_restart_frame and the fake adapter restarted frame (main.fake:1) is shown with no Reset Frame failure |
| `plugin_view_render`    | B     | none       | `workbench::plugin_tool_windows::tests::a_plugins_view_is_a_tool_window_with_its_title_and_icon_that_shows_the_rendered_rows`; the tool window shows the rows the plugin rendered through view_invocation + PluginHost::render_view (the body of ide_host::plugins::views::render) |
| `plugin_view_event`     | B     | none       | `workbench::plugin_tool_windows::tests::clicking_a_node_reaches_the_plugin_and_its_refresh_repaints_the_view`; a click reaches the plugin through view_invocation + PluginHost::view_event (the body of ide_host::plugins::views::event) and its refresh repaints the view         |
| `git_credentials`       | A     | none       | `settings_view::custom_controls::tests::passwords_lists_a_saved_credential_and_forget_removes_it`; the Git page Passwords list shows the host GitCredentials::list returns from the git_creds store                                                                                |
| `git_credential_forget` | A     | none       | `settings_view::custom_controls::tests::passwords_lists_a_saved_credential_and_forget_removes_it`; Forget on the Git page calls GitCredentials::forget on the git_creds store and the store no longer holds the credential                                                         |
| `db_statements`         | B     | none       | `database::console_view::tests::executes_the_statement_at_the_caret`; Execute splits the console text with ide_db::split::statement_at (what ops::db::statements calls once it has the driver) and runs only 'select 2', the statement at the caret                                |

## Needs an assertion (a test reaches the call), by native module

### `debugger/driver` (5)

- `java_resolve_launch` (A, partial; `app-gpui/src/debugger/driver.rs:813`, `app-gpui/src/run/driver.rs:1003`): Needs the Java language server; with a fake jdtls (suggested fixture) answering resolveMainClass, start a Java debug and assert the main class reaches the launch request.
- `debug_capabilities` (A, none; `app-gpui/src/debugger/driver.rs:620`): `workbench::debug::tests::proofs_debug::reset_frame_tells_that_an_adapter_cannot_restart_frames` may depend on the capabilities read (supportsRestartFrame false); confirm the message comes from the capabilities and not from a refused request, then record, else assert an action is disabled by them.
- `debug_set_function_breakpoints` (A, none; `app-gpui/src/debugger/driver.rs:585`): `workbench::debug::tests::the_debuggers_other_actions_reach_the_adapter_and_the_editor` runs debug.toggleMethodBreakpoint without asserting; during a session assert the fake adapter receives setFunctionBreakpoints (or stops in the method).
- `debug_set_data_breakpoints` (A, none; `app-gpui/src/debugger/driver.rs:587`): `workbench::debug::tests::proofs3_debug::toggle_field_watchpoint_on_a_java_field_sets_and_removes_a_watchpoint` and `workbench::debug::tests::the_debuggers_other_actions_reach_the_adapter_and_the_editor` toggle a field watchpoint but assert only the local list; with a session running, assert the fake adapter receives setDataBreakpoints for the field.
- `plugin_prepare_debug_run` (A, partial; `app-gpui/src/debugger/driver.rs:849`): Partial: the existing test reaches it; assert the prepared configuration (plugin-modified args) reaches the adapter launch.

### `workbench/database` (4)

- `db_set_password` (A, none; `app-gpui/src/workbench/database.rs:173`): `workbench::tests::the_password_a_connection_asks_for_is_masked_and_answers_the_store` stops at the prompt answer; feed the answer to the store (DbOp::SetPassword, workbench/database.rs:173) and assert a second connect of that source asks nothing.
- `db_connected` (A, partial; `app-gpui/src/workbench/database.rs:166`): Connect a SQLite source, reopen the window and assert the tree marks it connected from ops::db::connected.
- `db_explain` (A, partial; `app-gpui/src/workbench/database.rs:214`): Partial: assert the Query Plan tab shows the plan nodes.
- `db_set_manual_commit` (A, none; `app-gpui/src/workbench/database.rs:183`): `database::store::tests::marks_a_manual_transaction_pending_and_asks_before_auto_commit` is a model test; run it on a real SQLite source and assert an uncommitted insert is invisible from a second connection.

### `workbench/github` (3)

- `gh_pr_list` (B, none; `app-gpui/src/workbench/github.rs:1027`): Via GhCli::api with the GraphQL issueSearch document (not `gh pr list`, which ide_github::list runs). B: native lists through GhCli::api issueSearch; `workbench::github::tests::the_window_connects_and_lists_two_pull_requests_with_their_state_author_and_labels` already asserts the list. Decide whether parity accepts a different gh call; no new test needed.
- `gh_pr_view` (B, none; `app-gpui/src/workbench/github.rs:1157`): Via GhCli::api GraphQL pullRequest queries (not `gh pr view`). B: `workbench::github::tests::view_pull_request_opens_its_tab_with_its_title_branches_checks_and_changed_files` asserts the details via GhCli::api; decide whether that counts.
- `gh_pr_review_comments` (B, none; `app-gpui/src/workbench/github.rs:1161`): Via GhCli::api GraphQL reviewThreads (not ide_github::review_comments). B: add an assertion that a review thread from the GraphQL answer shows in the diff; then decide on the different gh call.

### `workbench/log_actions` (3)

- `vcs_restore_head` (A, partial; `app-gpui/src/workbench/log_actions.rs:1652`): Run a merge or rebase in two repositories, Undo from the notification and assert both HEADs are back.
- `vcs_config_get` (A, partial; `app-gpui/src/workbench/log_actions.rs:965`, `app-gpui/src/workbench/shelf.rs:1919`): Add: the commit window reads commit.template; set it in the fixture repo and assert the message box is prefilled.
- `vcs_diff_with_worktree` (A, partial; `app-gpui/src/workbench/log_actions.rs:606`): Keep partial or build the Changes between dialog and assert it lists the files that differ.

### `platform/settings` (2)

- `settings_get_global` (A, partial; `app-gpui/src/platform/settings.rs:31`, `app-gpui/src/workbench/app_actions.rs:936`): Existing partial test lacks the proxy-password merge: save an HTTP proxy password, open Settings under Global and assert the proxy page shows it (Host::with_proxy_password), or record as partial.
- `settings_set` (A, partial; `app-gpui/src/platform/settings.rs:44`): `platform::settings::tests::a_write_is_stored_and_announced` covers the write; add a project-scope write and assert only that project window gets SettingsChanged (the adapter emits to the owner window only).

### `plugins` (2)

- `plugin_list` (A, partial; `app-gpui/src/plugins.rs:288`, `app-gpui/src/workbench/plugin_runtime.rs:1415`): Partial: add a project plugin under .openidea/plugins and assert the Plugins page lists it after sync_project (only the global list is asserted).
- `plugin_run_command` (A, partial; `app-gpui/src/plugins.rs:459`, `app-gpui/src/workbench/plugin_runtime.rs:838`): Partial: cover a command run with an editor context and assert the plugin sees the selection (PluginEditor::context).

### `workbench/app_actions` (2)

- `settings_import` (A, partial; `app-gpui/src/workbench/app_actions.rs:929`): Extend `workbench::app_actions::tests::exports_templates_macros_layouts_and_view_options_not_per_project_state` to import a bundle with mcpServer/ssh keys and assert host.global_settings and the applied proxy (apply_proxy) change.
- `vcs_init` (A, none; `app-gpui/src/workbench/app_actions.rs:1114`): `workbench::app_actions::tests::new_project_creates_the_folder_and_opens_it` asserts no .git; add a run with new-project-git on and assert created/.git exists and vcs_roots lists it.

### `workbench/log_window` (2)

- `vcs_log_users` (A, partial; `app-gpui/src/workbench/log_window.rs:251`): Two repository roots with different authors; assert the User filter lists both authors (the adapter reads every root).
- `vcs_commits_changes` (A, partial; `app-gpui/src/workbench/log_window.rs:77`): In the log window select two commits and assert the changes pane lists the union of their files.

### `workbench/vcs_operations` (2)

- `vcs_line_hunks` (A, partial; `app-gpui/src/workbench/vcs_operations.rs:839`): Gutter markers do not read it; either wire the gutter or keep partial. The Rollback Lines test already covers its own path.
- `vcs_stashes` (A, partial; `app-gpui/src/workbench/vcs_operations.rs:487`): Build the Commit window stash list or keep partial; the Unstash dialog test exists.

### `editor/core_actions` (1)

- `doc_selection_ranges` (A, partial; `app-gpui/src/editor/core_actions.rs:268`): `editor::core_actions::tests::extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret` uses text candidates; use a case only the syntax ranges give (a string inside a call argument) and assert the second Extend selects the whole argument list.

### `editor/intelligence` (1)

- `lsp_resolve_completion` (A, partial; `app-gpui/src/editor/intelligence.rs:539`, `app-gpui/src/editor/intelligence.rs:636`): In `editor::intelligence::tests::typing_completes_from_the_server_and_enter_inserts_the_item_with_its_import`, have the fake server answer completionItem/resolve with documentation only it holds and assert the documentation popup shows it.

### `settings_view/custom_controls` (1)

- `term_available_shells` (A, none; `app-gpui/src/settings_view/custom_controls.rs:106`): `settings_view::custom_controls::tests::the_login_shell_is_read_after_the_dialog_opens` already reaches the call (custom_controls.rs:106 reads both); assert the Shell path dropdown lists ide_terminal::available_shells().

### `vcs/log_view` (1)

- `vcs_log` (A, partial; `app-gpui/src/vcs/log_view.rs:293`, `app-gpui/src/workbench/branches.rs:289`): Partial callers (branches push_repos, commit amend message) are untested; add: Push dialog lists the unpushed commit from vcs_log; Amend loads the last message (already in `workbench::tests::amend_loads_the_last_message_and_amends_the_commit`, check it reaches commit.rs:352).

### `workbench/mod` (1)

- `recent_project_groups` (B, partial; `app-gpui/src/workbench/mod.rs:2863`): Via ide_workspace::RecentProjects::groups on a store loaded from the config dir. Seed RecentProjects::set_groups with one group in the test home, open the Welcome screen (`workbench::tests::a_window_with_nothing_open_welcomes_with_the_recent_projects` setup) and assert the group header and its project row are shown.

### `workbench/plugin_contributions` (1)

- `plugin_activation_event` (A, partial; `app-gpui/src/workbench/plugin_contributions.rs:951`): Partial: assert a plugin with onCommand/onView activation loads only after that event.

### `workbench/plugin_runtime` (1)

- `plugin_status_items` (A, partial; `app-gpui/src/workbench/plugin_runtime.rs:634`): Partial: tests exist; add the case the proof says is missing or record.

### `workbench/terminal_actions` (1)

- `term_is_busy` (B, partial; `app-gpui/src/workbench/terminal_actions.rs:206`): Via ide_host::ops::term::is_busy, a wrapper of Terminals::is_busy. Run `sleep 60` in the terminal, Close Terminal Session, assert the confirmation appears (the busy branch); the idle branch is covered.

## Needs a test, by native module

### `debugger/driver` (18)

- `cancel_request` (A, none; `app-gpui/src/debugger/driver.rs:800`, `app-gpui/src/editor/intelligence.rs:419`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): editor::intelligence::abort_completion: with the fake LSP that never answers completion, type to open completion, press Escape, then assert the fake server log receives $/cancelRequest for that id.
- `run_dotnet_resolve` (A, none; `app-gpui/src/debugger/driver.rs:856`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), start a .NET launch configuration with a fake dotnet project and assert the resolved program path from ops::run::run_dotnet_resolve reaches the adapter launch arguments.
- `java_debug_server` (A, none; `app-gpui/src/debugger/driver.rs:827`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): With a fake jdtls (suggested fixture) answering vscode.java.startDebugSession, start a Java debug and assert the adapter connects to that port.
- `java_build_workspace` (A, none; `app-gpui/src/debugger/driver.rs:806`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): With a fake jdtls (suggested fixture), start a Java debug and assert java.project.build was requested before launch (fake server log).
- `debug_threads` (A, none; `app-gpui/src/debugger/driver.rs:625`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), pause a two-thread fake program and assert the Threads list shows both thread names the adapter answered.
- `debug_send_input` (A, none; `app-gpui/src/debugger/driver.rs:723`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), type into the debug console input and assert the fake adapter receives it (adapter log) or echoes it to the console.
- `debug_breakpoint_states` (A, none; `app-gpui/src/debugger/driver.rs:753`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), set a breakpoint the fake adapter reports unverified and assert the gutter mark shows the unverified state.
- `sdk_php_effective` (A, none; `app-gpui/src/debugger/driver.rs:861`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), start a PHP debug configuration and assert the interpreter from ops::sdk::sdk_php_effective is the launched program.
- `sdk_php_xdebug` (A, none; `app-gpui/src/debugger/driver.rs:880`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Start PHP debug with an interpreter lacking xdebug and assert the missing-Xdebug notification.
- `sdk_locate_tools` (A, none; `app-gpui/src/debugger/driver.rs:869`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Start a debug whose adapter tool must be located and assert the found tool path is used (or the install offer when absent).
- `debug_exception_filters` (A, none; `app-gpui/src/debugger/driver.rs:769`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), open Breakpoints dialog during a session and assert the exception filters listed are the adapter capabilities filters.
- `debug_set_exception_breakpoints` (A, none; `app-gpui/src/debugger/driver.rs:765`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), tick an exception filter and assert the fake adapter receives setExceptionBreakpoints with it.
- `debug_step_in_targets` (A, none; `app-gpui/src/debugger/driver.rs:711`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), Smart Step Into on a line with two calls and assert the chooser lists the adapter targets.
- `debug_step_in_target` (A, none; `app-gpui/src/debugger/driver.rs:684`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), choose a Smart Step Into target and assert the adapter receives stepIn with targetId and the frame moves into it.
- `debug_data_breakpoint_info` (A, none; `app-gpui/src/debugger/driver.rs:376`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), Add data breakpoint on a variable and assert the adapter answer (dataId/description) names the breakpoint row.
- `debug_attachable_processes` (A, none; `app-gpui/src/debugger/driver.rs:779`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Attach to Process: spawn a child process in the test and assert the chooser lists its pid.
- `debug_listen_for_jvm` (A, none; `app-gpui/src/debugger/driver.rs:786`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Listen-mode remote JVM configuration: start it, connect a fake JDWP client to the port and assert the session starts.
- `debug_redefine_classes` (A, none; `app-gpui/src/debugger/driver.rs:719`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In `workbench::debug::tests` (fake DAP adapter fixture), hot swap after a compile and assert the adapter receives redefineClasses for the changed class.

### `workbench/database` (10)

- `db_set_ssh_secret` (A, none; `app-gpui/src/workbench/database.rs:177`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): SSH-tunnelled source: enter the SSH secret and assert it is stored (next connect asks nothing).
- `db_trust_ssh_host` (A, none; `app-gpui/src/workbench/database.rs:179`, `app-gpui/src/workbench/database.rs:1438`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Unknown SSH host key prompt: accept it and assert the connection proceeds and known_hosts holds the key.
- `db_connect` (A, none; `app-gpui/src/workbench/database.rs:180`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Connect action on a SQLite source; assert the tree shows it connected and its tables load.
- `db_databases` (A, none; `app-gpui/src/workbench/database.rs:188`, `app-gpui/src/workbench/database.rs:1467`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): A server source (or SQLite attached dbs): expand it and assert the databases from ops::db::databases are listed.
- `db_schema_objects` (A, none; `app-gpui/src/workbench/database.rs:192`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Expand a schema with a view and a trigger; assert both listed.
- `db_history` (A, none; `app-gpui/src/workbench/database.rs:216`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Run two statements in a console, open history and assert both are listed newest first.
- `db_remove_history` (A, none; `app-gpui/src/workbench/database.rs:218`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Remove a history entry and assert it is gone after reopening.
- `db_completion` (A, none; `app-gpui/src/workbench/database.rs:206`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): In a console, type `select * from ` and assert completion offers a table of the SQLite file.
- `db_export_file` (B, none; `app-gpui/src/workbench/database.rs:224`): Via std::fs::write in the DbOp::WriteFile handler (the adapter is the same std::fs::write). By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): B: export a result to a file and assert the file content (CSV rows).
- `db_finish_transaction` (A, none; `app-gpui/src/workbench/database.rs:186`): By search, not traced (the dispatch fn is generic; tests of workbench::debug / workbench::tests were not mapped per variant): Manual commit mode: insert, Commit, assert row visible from another connection; Rollback variant asserts it is gone.

### `workbench/plugins_window` (8)

- `plugin_grant` (A, none; `app-gpui/src/workbench/plugins_window.rs:446`, `app-gpui/src/workbench/plugins_window.rs:740`): workbench::plugins_window grant_asked: a project plugin waiting for consent; accept the dialog and assert PluginHost lists it Running and its command runs.
- `plugin_set_capability` (A, none; `app-gpui/src/workbench/plugins_window.rs:452`): Plugins page row: revoke one declared capability and assert PluginHost::list shows it revoked and the plugin call needing it fails.
- `plugin_catalog` (A, none; `app-gpui/src/workbench/plugins_window.rs:616`): workbench::plugins_window::read_plugin_catalog: point the catalog at a local folder, open Marketplace tab and assert its entries are listed.
- `plugin_market` (A, none; `app-gpui/src/workbench/plugins_window.rs:625`, `app-gpui/src/workbench/plugins_window.rs:674`): Marketplace tab against a local file:// market listing; assert its plugins are listed.
- `plugin_set_market_url` (A, none; `app-gpui/src/workbench/plugins_window.rs:671`): Edit Plugin Repository: save a new URL and assert PluginHost::market_url returns it after restart.
- `plugin_market_download` (A, none; `app-gpui/src/workbench/plugins_window.rs:467`): Install from a local market listing; assert the plugin is installed and listed.
- `plugin_unpack_package` (A, none; `app-gpui/src/workbench/plugins_window.rs:549`): Install from disk with a packaged .zip; assert it unpacks and the install preview shows its manifest.
- `plugin_roll_back` (A, none; `app-gpui/src/workbench/plugins_window.rs:457`): Update a plugin then Roll Back; assert the previous version is listed.

### `workbench/agents` (5)

- `agent_unqueue` (A, none; `app-gpui/src/workbench/agents.rs:237`): workbench::agents call dispatch (AgentCall::Unqueue): with the fake ACP agent, queue a second prompt while a turn runs, remove it from the queue and assert the agent never receives that prompt and the queued chip is gone.
- `agent_send_queued_now` (A, none; `app-gpui/src/workbench/agents.rs:238`): With a fake ACP agent (suggested fixture), queue a prompt during a turn, click Send now and assert the agent receives it before the running turn ends.
- `agent_set_config` (A, none; `app-gpui/src/workbench/agents.rs:239`): With a fake agent that advertises a config option (model), pick another value in the config bar and assert the agent receives session/set_config_option with that value.
- `agent_stop_background` (A, none; `app-gpui/src/workbench/agents.rs:241`): With a fake agent that reports a background terminal, click Stop on it and assert the agent is told and the background entry ends.
- `agent_changes` (A, none; `app-gpui/src/workbench/agents.rs:641`): workbench::agents::carry_out_agents (changes): after the fake agent edits a file, open the task Changes view and assert it lists that file with its diff from ops::agent::changes.

### `workbench/code_insight` (4)

- `mcp_report_problems` (A, none; `app-gpui/src/workbench/code_insight.rs:592`): code_insight::report_problems feeds host.mcp_reports.report_problems: open a project file with a syntax error, let the Problems view refresh, then assert host.mcp_reports (Reports::problems or the MCP getDiagnostics answer) lists that file and message.
- `syntax_support` (A, none; `app-gpui/src/workbench/code_insight.rs:277`): workbench::code_insight::ask_syntax: open a .rs and a .txt file and assert the syntax-dependent actions are enabled only for the one ops::syntax::syntax_support reports.
- `syntax_identifiers` (A, none; `app-gpui/src/workbench/code_insight.rs:283`): workbench::code_insight::ask_syntax: open a file, run the word-completion/identifier feature that asks syntax_identifiers and assert an identifier from the file is offered.
- `syntax_identifiers_in_files` (A, none; `app-gpui/src/workbench/code_insight.rs:288`): workbench::code_insight::ask_syntax: two project files, ask identifiers across files and assert an identifier defined only in the second file is offered in the first.

### `workbench/github` (3)

- `gh_pr_review` (B, none; `app-gpui/src/workbench/github.rs:1490`): Via GhCli::api review mutations in run_pull_request_mutation (not ide_github::submit_review). B: submit a review (Approve) in the review mode and assert the gh stand-in received the submitPullRequestReview mutation and the state shows Approved.
- `gh_pr_comment` (B, none; `app-gpui/src/workbench/github.rs:1490`): Via GhCli::api comment mutations in run_pull_request_mutation (not ide_github::add_comment). B: post a PR comment from the timeline and assert the stand-in got addComment and the timeline shows it after reload.
- `gh_pr_prepare_diff` (A, none; `app-gpui/src/workbench/github.rs:1378`): workbench::github::open_pull_request_diff: with the gh stand-in and a local repo holding base/head, open a changed file of the PR and assert the diff window shows base vs head text.

### `workbench/plugin_runtime` (3)

- `cancel_task` (A, none; `app-gpui/src/workbench/plugin_runtime.rs:790`): The existing plugin-task test goes through PluginHost::cancel_task (plugin_runtime.rs:799), not ide_core::task::cancel (plugin_runtime.rs:790, the CancelEffect::Backend branch). Cancel a backend-numbered background task (e.g. a long index or build sync shown in the task list) and assert it ends.
- `plugin_answer_commit` (A, none; `app-gpui/src/workbench/plugin_runtime.rs:526`): A plugin asking for a commit selection: open the commit chooser, pick files and assert the plugin receives them (answer_commit).
- `plugin_answer_command` (A, none; `app-gpui/src/workbench/plugin_runtime.rs:1536`): A plugin command that asks a question: answer it and assert the plugin output depends on the answer.

### `workbench/services_window` (3)

- `docker_snapshot` (A, none; `app-gpui/src/workbench/services_window.rs:724`): workbench::services_window::carry_out_docker: with a fake docker CLI on PATH printing one container, open Services and assert the container row.
- `docker_settings` (A, none; `app-gpui/src/workbench/services_window.rs:716`): Services Docker settings: assert stored docker settings load into the connection dialog.
- `docker_save_settings` (A, none; `app-gpui/src/workbench/services_window.rs:720`): Change the Docker connection in Services and assert ide_docker::load_settings returns it.

### `everywhere/structure_view` (1)

- `doc_open_virtual` (A, none; `app-gpui/src/everywhere/structure_view.rs:196`): everywhere::structure_view::with_doc: open the File Structure popup on a virtual document (e.g. a decompiled class or diff side) and assert its symbols load.

### `plugins` (1)

- `plugin_market_url` (A, none; `app-gpui/src/plugins.rs:227`, `app-gpui/src/workbench/plugins_window.rs:623`): Edit Plugin Repository dialog: assert it shows the stored market URL.

### `workbench/app_shell` (1)

- `workspace_attach` (A, none; `app-gpui/src/workbench/app_shell.rs:834`): workbench::app_shell::carry_out_open: open project A, choose Attach for project B in the open dialog, then assert ide_workspace::workspace::Workspace::load(A) lists B and the project tree shows B as a second root.

### `workbench/diff_window` (1)

- `history_content` (A, partial; `app-gpui/src/workbench/diff_window.rs:78`): workbench::diff_window::load_text: change a file twice, open Local History, choose a revision and assert the diff left side shows that revision text.

### `workbench/mod` (1)

- `project_current` (A, none; `app-gpui/src/workbench/mod.rs:3009`, `app-gpui/src/debugger/driver.rs:170`): Native reads the window project through Host::project and ops::project::info at open; add an assertion in a project-open test that the title/ProjectInfo display name equals ops::project::info of host.project(label).

### `workbench/plugin_tool_windows` (1)

- `plugin_assets` (A, none; `app-gpui/src/workbench/plugin_tool_windows.rs:397`): workbench::plugin_tool_windows::ask_plugin_view_icons: plugin with an icon asset in a tool window; assert the tool-window stripe shows that SVG.

### `workbench/project_files` (1)

- `fs_rename` (A, partial; `app-gpui/src/workbench/project_files.rs:751`): workbench::project_files::answer fallback: rename a file with no references/refactoring provider (plain .txt outside sources) and assert the old path is gone and the new one has the content.

### `workbench/settings_window` (1)

- `ssh_set_secret` (A, none; `app-gpui/src/workbench/settings_window.rs:117`): workbench::settings_window::store_ssh_secret: save an SSH password in Settings > SSH and assert host.ssh.needs_secret is false for that config.

### `workbench/terminal_actions` (1)

- `plugin_terminal_session_close` (A, none; `app-gpui/src/workbench/terminal_actions.rs:110`): A plugin terminal session: close its tab and assert PluginHost reports the session ended (plugin hears close).

## Class C: not called natively, by webview feature

Feature = the `src/features/<name>` (or `src/<dir>`) folder whose non-test files call `commands.<camelCase>` most; "(no webview caller)" = no non-test webview file calls it.

- **plugins** (27): `plugin_decoration_sets`, `plugin_terminal_session_spawn`, `plugin_parse_test_results`, `plugin_select_tests`, `plugin_content_tab_action`, `plugin_choose_program`, `plugin_program`, `plugin_commit_checks`, `plugin_service_detail`, `plugin_service_log`, `plugin_web_page`, `plugin_web_pty_spawn`, `plugin_web_save_file`, `plugin_web_read_file`, `plugin_line_markers`, `plugin_navigation_files`, `plugin_navigation_invalidate`, `plugin_navigation_index`, `plugin_navigation_goto`, `plugin_navigation_markers`, `plugin_endpoints`, `plugin_run_targets`, `plugin_answer_run_target`, `plugin_read_intellij_theme`, `plugin_installed_intellij_themes`, `plugin_stage_package`, `plugin_set_theme_frame`
- **vcs** (18): `fs_copy_merge`, `fs_compare_directories`, `doc_snapshot`, `history_put_label` (drafted), `vcs_blame`, `vcs_block_history`, `vcs_log_refs`, `vcs_open_external_log`, `vcs_log_details_batch`, `vcs_revision_children`, `vcs_revision_files`, `vcs_delete_tag`, `vcs_restore_tag`, `vcs_stash_drop`, `vcs_stage_hunks`, `vcs_clone`, `vcs_compare_scope` (drafted), `vcs_compare_paths`
- **(no webview caller)** (17): `doc_save_all`, `history_list`, `refactor_can_undo_file_move`, `index_status`, `vcs_stage_patch`, `vcs_diff_text`, `run_system_env`, `run_list`, `debug_evaluate`, `debug_sessions`, `jdk_download_vendors`, `plugin_link`, `plugin_unlink`, `gh_pr_create`, `ssh_connected`, `ssh_disconnect`, `remote_of`
- **run** (16): `lsp_workspace_symbols`, `run_save_templates`, `run_import_sources`, `run_import_configs`, `run_tomcat_home`, `run_tomcat_installations`, `run_tomcat_artifacts`, `run_tomcat_build`, `run_application_build`, `run_application_publish`, `run_tomcat_sync`, `run_tomcat_update`, `run_test_reports`, `sdk_detect`, `sdk_php_composer_install`, `debug_free_port`
- **editor/lsp** (15): `web_goto`, `fs_resolve_reference`, `web_unresolved_references`, `spell_check`, `spell_suggest`, `lsp_inline_completion`, `lsp_goto_implementation`, `lsp_virtual_document`, `lsp_code_action_query`, `lsp_inlay_hints`, `lsp_semantic_tokens`, `lsp_code_lens`, `lsp_document_links`, `lsp_resolve_command`, `lsp_execute_code_action`
- **settings** (15): `mcp_server_status`, `plugin_formatters`, `jdk_download_releases`, `jdk_download_location`, `sdk_python_interpreter`, `sdk_php_language_level`, `sdk_python_create_venv`, `sdk_python_packages`, `sdk_python_pip`, `proxy_check_connection`, `ssh_test`, `ssh_needs_secret`, `ssh_trust_host`, `remote_list`, `deploy_test_connection`
- **services** (14): `docker_container_op`, `docker_remove`, `docker_inspect`, `docker_image_history`, `docker_container_top`, `docker_container_stats`, `docker_logs`, `docker_events`, `docker_pull`, `docker_prune`, `docker_compose_op`, `docker_set_registry_password`, `docker_registry_login`, `docker_run_env`
- **deployment** (12): `deploy_coverage`, `deploy_upload`, `deploy_download`, `deploy_delete_remote`, `deploy_compare`, `deploy_sync`, `deploy_info`, `deploy_server_root`, `remote_open`, `remote_delete`, `remote_mkdir`, `remote_rename`
- **notebook** (12): `notebook_open`, `notebook_get`, `notebook_edit`, `notebook_save`, `notebook_reload`, `notebook_undo`, `notebook_redo`, `notebook_close`, `notebook_execute`, `notebook_interrupt`, `notebook_restart`, `sdk_python_effective`
- **agents** (10): `agent_set_command`, `agent_set_env`, `agent_registry_list`, `agent_registry_install`, `agent_registry_remove`, `agent_commands`, `agent_probe`, `agent_base_text`, `find_dirs`, `path_completions`
- **build-tools** (9): `build_model` (drafted), `gradle_link_projects` (drafted), `gradle_unlink_projects` (drafted), `build_sync` (drafted), `gradle_dependencies`, `gradle_published_catalog`, `maven_dependency_declaration`, `maven_exclude_dependency`, `maven_goto_declaration`
- **analysis** (6): `prepare_workspace_edit`, `inspect_code`, `analysis_locate_duplicates`, `structural_languages`, `structural_validate`, `structural_search`
- **app** (6): `workspace_create`, `workspace_discover_projects`, `workspace_update`, `settings_read_jetbrains`, `project_rename`, `plugin_set_ui_locale`
- **http-client** (6): `spring_route_sources`, `http_requests`, `http_environments`, `http_history`, `http_history_get`, `http_history_clear`
- **remote-dev** (6): `wsl_distros`, `remote_bootstrap_list`, `remote_forwarded_ports`, `remote_forward_port`, `remote_stop_forwarding`, `env_reload`
- **platform** (5): `system_font_families`, `jvm_file_presentations`, `jdk_download`, `remote_connect`, `remote_current`
- **editor/core** (4): `clipboard_file_list`, `doc_sync_from_disk`, `doc_ui_mode`, `lsp_resolve_class_file`
- **editor/structure** (4): `syntax_node_at` (drafted), `syntax_expression_at`, `syntax_errors`, `syntax_structural_ranges`
- **mcp** (4): `mcp_ui_ready`, `mcp_respond`, `mcp_set_open_editors`, `mcp_set_focused_editor`
- **coverage** (3): `coverage_prepare`, `coverage_collect`, `coverage_import`
- **project-tree** (3): `clipboard_has_files`, `clipboard_set_file_list`, `remote_import_local`
- **tools** (3): `tools_list`, `tools_install`, `tools_uninstall`
- **web** (3): `web_browsers`, `web_url`, `web_open`
- **editor/refactor** (2): `lsp_hierarchy_members`, `lsp_hierarchy_refactor`
- **graphql** (2): `fs_create_file_bytes`, `http_execute`
- **logcat** (2): `android_devices`, `android_logcat`
- **perf** (2): `app_metrics`, `clear_caches`
- **profiler** (2): `profiler_prepare`, `profiler_collect`
- **project-trust** (2): `project_trust_list`, `project_trust_add_location`
- **ssh** (2): `ssh_askpass_answer`, `ssh_terminal_command`
- **color-preview** (1): `plugin_color_presentations`
- **debugger** (1): `debug_completions`
- **dependencies** (1): `dependency_versions`
- **editor/navigation** (1): `lsp_implementations`
- **github** (1): `gh_account_repos`
- **libraries** (1): `sdk_project_libraries`
- **terminal** (1): `term_store_image`
- **welcome** (1): `remote_disconnect`

## Class D: shell-only

| Command                          | Row     | Reason                                                                                                                                                |
| -------------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app_info`                       | none    | platform facts the webview cannot read itself (Wayland, Windows build number, config dir)                                                             |
| `window_group_current`           | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_merge`             | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_select`            | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_detach`            | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_close`             | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_close_tabs`        | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_cancel_close`      | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_reorder`           | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_group_restore`           | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `log_frontend`                   | none    | forwards the webview console to the shell log                                                                                                         |
| `clipboard_image`                | none    | reads a clipboard image for the webview (GTK path on Linux); GPUI reads its clipboard itself                                                          |
| `app_restart`                    | partial | relaunches the Tauri process (crate::commands::window::app_quit with restart)                                                                         |
| `window_open_project`            | partial | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_files`              | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_file_in_new_window` | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_remote`             | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_new`                     | done    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_settings`           | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_comparison`         | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_deployment`         | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_cycle`                   | done    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_toggle_fullscreen`       | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_flash_taskbar`           | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_set_titlebar_height`     | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_is_fullscreen`           | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_start_drag`              | none    | window drag and tab groups of Tauri webview windows; GPUI manages its own windows                                                                     |
| `window_take_launch`             | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_complete_launch`         | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_close_context`           | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `window_open_projects`           | none    | Tauri window management (crate::windows / window_groups / titlebar): opening, cycling, launch handshake, fullscreen, taskbar flash of webview windows |
| `app_quit`                       | partial | quit handshake between the Tauri shell and its webview windows (crate::windows::continue_quit)                                                        |
| `app_cancel_quit`                | none    | quit handshake between the Tauri shell and its webview windows (crate::windows::continue_quit)                                                        |
| `menu_configure`                 | partial | native menu bar built from the webview menu model (crate::menu); GPUI builds its own menu                                                             |
| `menu_refresh`                   | done    | native menu bar built from the webview menu model (crate::menu); GPUI builds its own menu                                                             |
| `menu_model`                     | partial | native menu bar built from the webview menu model (crate::menu); GPUI builds its own menu                                                             |
| `menu_set_disabled`              | partial | native menu bar built from the webview menu model (crate::menu); GPUI builds its own menu                                                             |
| `app_log_location`               | none    | the Tauri shell log folder (crate::logging)                                                                                                           |
| `app_cli_install`                | none    | installs or removes the Tauri app command-line launcher (crate::cli)                                                                                  |
| `app_cli_uninstall`              | none    | installs or removes the Tauri app command-line launcher (crate::cli)                                                                                  |
| `app_build_info`                 | none    | About data of the Tauri build (Tauri version, updater configuration)                                                                                  |
| `app_check_update`               | none    | Tauri updater (crate::updates)                                                                                                                        |
| `app_update_window_ready`        | none    | Tauri updater (crate::updates)                                                                                                                        |
| `app_update_status`              | none    | Tauri updater (crate::updates)                                                                                                                        |
| `app_update_announcement`        | none    | Tauri updater (crate::updates)                                                                                                                        |
| `app_install_update`             | none    | Tauri updater (crate::updates)                                                                                                                        |
| `plugin_subscribe`               | partial | registers the webview channel that receives plugin events                                                                                             |
| `wsl_resolve`                    | none    | turns a WSL path into a Tauri window launch (crate::windows::wsl_launch)                                                                              |
| `remote_invoke`                  | none    | forwards a webview IPC call to the remote backend process of a remote-dev window                                                                      |
| `user_font_data`                 | none    | raw IPC that serves font bytes to the webview CSS                                                                                                     |

## Notes on rows already `done` or `partial`

- Class D rows that parity.json counts: `window_new`, `window_cycle` and `menu_refresh` are `done`; `window_open_project`, `app_quit`, `app_restart`, `menu_set_disabled`, `menu_configure`, `menu_model` and `plugin_subscribe` are `partial`. The native app cannot call these adapters' `crate::windows` / `crate::menu` / shell functions; their proofs show equivalent native behaviour, not the same backend function.
- Traced `done` rows where native code calls the adapter function's callee instead of the function (class B, consistent with their proofs): `doc_edit` (Documents::edit, editor/view.rs:903), `doc_set_encoding`, `doc_set_line_ending` (Documents methods), `term_write`, `term_resize` (ops::term wrappers of Terminals), `gh_availability`, `gh_api`, `gh_pr_checkout` (GhCli methods that the ide_github free functions delegate to), `open_project` (ops::project::open plus RecentProjects), `remove_recent_project`, `recent_projects`, `set_recent_project_groups` (RecentProjects loaded from the config dir), `light_edit_open`, `close_project` (Host methods behind AppState).
- Among those traced rows none reaches a different operation or none. The other done rows were matched by search only (a native non-test call of the adapter primary function exists); their proofs were not re-read.
- `cancel_task` is not proven by the plugin-task test: plugin tasks are cancelled through PluginHost::cancel_task, not ide_core::task::cancel.
