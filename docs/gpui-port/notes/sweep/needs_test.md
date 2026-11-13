# List B: native code exists and is reachable, no test proves it

Grouped by the module a test would live in, groups and recipes ordered by how many rows one test would tick (row counts are the shard reader's estimate after reading the code). Each recipe names the shard that wrote it. File:line references were taken between 05:10 and 05:40 on 2026-11-13 while another agent was editing app-gpui; prefer the fn names. Recipes over "ui", "e2e", "wit" and the 26-row editor::intelligence group are multi-test audits, not single tests.

## First: tests that already exist in source but are not in rust_tests.txt

Regenerating the test list (cargo test -- --list) and re-reading these bodies is the cheapest step of all. In app-gpui/src/workbench/mod.rs: `a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn`, `the_new_agent_task_shortcut_needs_a_project`, `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key`, `a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent`, `an_unseen_reply_marks_its_task_unread_and_archive_and_delete_remove_it` (the features shard estimates up to 23 agents action/ipc/context rows move to A; see Recipe F1 below). In app-gpui/src/workbench/debug.rs, declared since about 05:36 (`mod debug;` at workbench/mod.rs:13, `debug::register_actions` at :462, and `debugger::{driver, breakpoints_view}` declared): `toggling_a_breakpoint_at_a_line_marks_it_persists_it_and_lists_it_in_the_breakpoints_dialog`, `clicking_the_gutter_toggles_the_lines_breakpoint`, `debugging_a_configuration_stops_at_the_breakpoint_steps_over_and_resumes_to_the_end`, `stop_ends_a_running_debug_session`. Those debug tests run debug.toggleBreakpoint, debug.viewBreakpoints, debug.muteBreakpoints, debug.stepOver, debug.resume, debug.stop and run.debugConfig; the debug shard was classified before this landed, so its 82 list C rows that say "only in undeclared workbench/debug.rs / debugger/driver.rs" are now list B (and some A once listed). Five debugger `[m]` unit rows (actions, hotswap, valueChanges, valueText, toolWindow) and six agents `[m]` rows already render `[x]` on regeneration with no parity.json change.

## workbench::tests (mod.rs `mod tests`) — 195 rows

### Workbench actions, project view, e2e-spec ports (workbench/project_files.rs, mod.rs) — 53 rows [workbenchB]

#### e2e (workbench mod.rs tests): 32 row(s)

Rows: `e2e:e2e/boot.spec.ts`, `e2e:e2e/codeInsight.spec.ts`, `e2e:e2e/commentToggle.spec.ts`, `e2e:e2e/commit.spec.ts`, `e2e:e2e/commitInclusion.spec.ts`, `e2e:e2e/compareWith.spec.ts`, `e2e:e2e/conflictMarkers.spec.ts`, `e2e:e2e/console-selection.spec.ts`, `e2e:e2e/debug.spec.ts`, `e2e:e2e/diff-navigation.spec.ts`, `e2e:e2e/diff.spec.ts`, `e2e:e2e/editor.spec.ts`, `e2e:e2e/gitLogBranchMenu.spec.ts`, `e2e:e2e/goHighlight.spec.ts`, `e2e:e2e/keymap.spec.ts`, `e2e:e2e/kotlinHighlight.spec.ts`, `e2e:e2e/mergeEditor.spec.ts`, `e2e:e2e/newBranch.spec.ts`, `e2e:e2e/openProject.spec.ts`, `e2e:e2e/projectTree.spec.ts`, `e2e:e2e/pythonHighlight.spec.ts`, `e2e:e2e/run.spec.ts`, `e2e:e2e/search.spec.ts`, `e2e:e2e/services.spec.ts`, `e2e:e2e/settings.spec.ts`, `e2e:e2e/stripeTooltip.spec.ts`, `e2e:e2e/terminal-clipboard.spec.ts`, `e2e:e2e/themes.spec.ts`, `e2e:e2e/toolwindows.spec.ts`, `e2e:e2e/workbench.spec.ts`, `e2e:e2e/httpHighlight.spec.ts`, `e2e:e2e/usage-highlight-contrast.spec.ts`

Playwright specs whose flow has a native counterpart with workbench tests; each would need its spec's steps replayed in a workbench test. Rows: listed under 'e2e' in this group's row list. Not one-test ticks.

#### workbench (mod.rs tests): 7 row(s)

Rows: `module:src/workbench/toolwindows/actions.ts`, `module:src/workbench/Notifications.tsx`, `module:src/workbench/EditorArea.tsx`, `module:src/workbench/OverlayHost.tsx`, `module:src/workbench/inputDialogs.tsx`, `module:src/workbench/softWrap.ts`, `module:src/workbench/WindowControls.tsx`

Module-level rows whose native counterpart is in workbench/mod.rs (tool window actions, notifications tool window and balloons, editor area splits, the popup slot, rename/new-name popups, soft wrap toggles, the window controls drawn by the title bar). Each needs an audit of the TS module against the native code before a proof can be written; not cheap one-test ticks. toolwindows/actions.ts is closest: the per-window, hide/focus/maximize/resize actions are tested, closeContent and the layout actions are not registered.

#### workbench (mod.rs tests): 2 row(s)

Rows: `unit:src/workbench/messages.test.tsx`, `module:src/workbench/messages.tsx`

The safe-delete question is native (`delete_asks_and_cancelling_keeps_everything`, `a_confirmed_delete_searches_deletes_and_reports_what_could_not_be_deleted`). Port the four cases: assert the button order and default for macOS, Escape/Cancel cancel, and the checkbox state reported with the chosen button, using `wait_for_prompt` like the merge test.

#### ipc (mod.rs tests): 2 row(s)

Rows: `ipc:window_toggle_fullscreen`, `ipc:window_open_settings`

Desktop-shell commands with no ops function. window_toggle_fullscreen: the native window.toggleFullScreen (windows.rs:288) uses GPUI's own toggle, which the test platform cannot show (see that action's partial). window_open_settings: the native Settings dialog opens in-window and is tested (`settings_searches_the_pages_applies_a_ticked_checkbox_and_ok_closes`); whether that counts for the command is the owner's call, since the rule needs a shared ops function.

#### workbench (mod.rs tests): 1 row(s)

Rows: `action:(dynamic) src/features/vcs/mergeActions.ts:64`

The merge family is registered in one loop in vcs_more/merge_actions.rs (reached) and `workbench::tests::the_merge_editor_opened_from_the_banner_runs_its_actions_and_the_rebase_is_continued_from_the_notice` already runs 6 of its 30 commands (compare, settings, autoApply, reload, cancel, acceptYours) through the registry. Copy that test's setup (`git_project` with `conflicting_branches`, open the merge editor from the banner) and run applyLeft, applyRight, applyAll, acceptTheirs, resolveSimple, revert, apply and the compare* variants through `run_action`, asserting the result pane text / `is_modified()` after each; then the family row can be cited with both tests.

#### workbench (mod.rs tests): 1 row(s)

Rows: `action:(dynamic) src/workbench/toolwindows/actions.ts:247`

toolwindow.resize{Left,Right,Up,Down} come from one loop at workbench/mod.rs:499. `appearance_toggles_hide_the_bars_and_tool_windows_maximize_and_resize` runs Left/Right on the Project view only. Extend it (or add a sibling) to show the terminal tool window (`run_action(cx, "toolwindow.terminal")`), run toolwindow.resizeUp then resizeDown and assert `layout.sizes().bottom` grows and returns; this also upgrades boundid:toolwindow.resizeUp/Down.

#### workbench (mod.rs tests): 1 row(s)

Rows: `action:toolwindow.hideActive`

Registered at workbench/mod.rs:856. In `tool_window_actions_show_focus_and_hide_the_project_view`, after the Project view is shown and focused, run `execute_action_or_throw("toolwindow.hideActive", None, window, cx)` and assert `!is_visible("project")` and that `toolWindowFocus` is off; this covers the gap its partial proof names.

#### workbench (mod.rs tests): 1 row(s)

Rows: `action:file.openInNewWindow`

Registered at workbench/windows.rs:253 with a path argument. Copy `projects_open_in_this_window_a_new_one_or_the_one_that_has_them`, run `file.openInNewWindow` with a temp folder path as arg and assert a second window exists whose workbench has that project open (and the first window keeps its own).

#### workbench (mod.rs tests): 1 row(s)

Rows: `module:src/workbench/projectColor.ts`

project_color is now used by the window (title_bar.rs:549 initials, welcome.rs:907 colors), so the partial's 'no window uses it yet' is stale. In `workbench::welcome::tests::shows_brand_version_and_project_rows_with_badges_home_relative_paths_and_branches` (or a title-bar test) also assert the badge's background equals `project_colors(name, theme)` for the row's name; then the module is done with `workbench::project_color::tests`.

#### workbench (mod.rs tests): 1 row(s)

Rows: `module:src/workbench/MainToolbarDialog.tsx`

The Customize Main Toolbar dialog is reached through `toolbar.customize`; `title_widgets_sort_by_priority_and_customize_toolbar_removes_and_restores_one` covers remove, restore and OK. Extend it with the move up/down buttons and the add-action picker (pick an action, OK, assert `action:<id>` appears in `appearance.main_toolbar.right` and is drawn as `title:action:<id>`).

#### workbench (mod.rs tests): 1 row(s)

Rows: `unit:src/workbench/lightEdit.test.tsx`

LightEdit is native (tab_views.rs:827 open_loose_file, status_items.rs:40 Open Project item). `opens_a_loose_file_in_an_editor_and_types_into_it` covers the editor; add asserts for its 7 cases: the window title is the file name, project-only context keys (projectOpen) are off, the status bar shows the LightEdit Open Project item, and no project tool window stripe is drawn.

#### workbench (mod.rs tests): 1 row(s)

Rows: `unit:src/workbench/toolwindows/badges.test.tsx`

Stripe badges exist (a tool window view's `badge()`, asserted at mod.rs:5530 and 7298). Add a test that sets the setting that hides stripe counts, asserts the badge element (`stripe-badge:<id>`) disappears at once and reappears with the current count when re-enabled.

#### workbench (mod.rs tests): 1 row(s)

Rows: `unit:src/features/project-tree/ProjectView.test.tsx`

28 cases over the native project tree (project_tree.rs, workbench/project_files.rs). Several are already exercised by workbench tests (rename dialog, move, delete, new file, tree focus context key); a dedicated pass would add single/double-click open and expand options, Always Select Opened File, members (file structure) under files, and fsChanged refresh. Not a one-test tick; at best partial.

#### ipc (mod.rs tests): 1 row(s)

Rows: `ipc:fs_entry_kind`

run/driver.rs:1371 calls ide_host::ops::fs::entry_kind for a path printed in run output. Copy `a_cargo_build_lists_its_error_succeeds_once_fixed_and_ends_cancelled_when_stopped` or a shell run printing an existing file path, click the link in the console and assert the file opens in an editor; that covers the partial's gap.

### Agents, Database, Plugins (workbench/agents.rs, database.rs, settings_window.rs) — 48 rows [features]

#### workbench (mod.rs `mod tests`, agents block): 32 rows, mostly by getting five existing tests into the test list

#### Recipe F1: list the five agents tests that already exist (ticks up to 27 rows)

The five agent end-to-end tests already sit in app-gpui/src/workbench/mod.rs:7104-7331 (`a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn`, `the_new_agent_task_shortcut_needs_a_project`, `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key`, `a_permission_request_shows_its_answers_and_the_one_clicked_reaches_the_agent`, `an_unseen_reply_marks_its_task_unread_and_archive_and_delete_remove_it`), but they are not in rust_tests.txt, so nothing can cite them yet. They may be newer than the list, or gated off. Regenerate the test list (`cargo test --lib -- --list`) and confirm they run. I read the bodies, and they then prove these rows. action:agents.copyReplyMarkdown: clipboard text asserted. action:agents.quoteReply: the draft holds `> Hello`. action:agents.sendToAgent: the draft holds `@README.md` and `@README.md:1`. action:agents.renameTask: the prompt is answered and the title becomes Greeting. action:agents.archiveTasks: the status becomes Archived and an Archived (1) row shows. action:agents.deleteTasks: after the confirm prompt the tasks are empty and the empty text shows. action:agents.newTask: cmd-shift-i opens the New Task page and the tool window, and the action fails without a project. action:agents.useToolWindow, agents.openTask, agents.dockTask and agents.openTaskInEditor: each re-shows the hidden tool window with the conversation docked. For action:agents.useEditor and agents.undockTask the test asserts only that the conversation stays docked in this window, so I would leave those partial. ipc:agent_create, agent_prompt, agent_subscribe and agent_entries: the conversation lines `you: …` and `agent: Hello from the fake agent` can only come from the fake agent through ops agent create/prompt/subscribe/entries (app-gpui/src/workbench/agents.rs:153-176, 495, 663, 683). ipc:agent_cancel: Stop ends the turn. ipc:agent_respond: the permission click leads to `agent: You chose yes`. ipc:agent_archive, agent_delete and agent_rename: as for the actions. context:agentSessionFocus is asserted on only; add one `assert!(!context_is_on(cx, "agentSessionFocus"))` after focusing the editor to tick it. action:agents.jumpToLatest is run but has no assertion; add an assertion on the session's scroll-to-latest state.

#### Recipe F2: the remaining agents actions and ops (ticks 13 rows)

Registrations are in app-gpui/src/workbench/agents.rs:182-300 and the op dispatch is at :153-176. Copy `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key` (agents_project, start_agent_task, run_action, wait_long). After a finished turn, run `agents.forkTask` and assert a second task in `store.tasks()`. Run `agents.forkReply`, `agents.regenerateReply` and `agents.retractMessage` with `{task,…}` and assert the conversation lines change (the fake agent answers again, or the user message is gone after the rollback). Archive, then `agents.unarchiveTasks`, and assert the status is no longer Archived. Run `agents.showToolWindow` with the window hidden and assert it becomes visible. Run `agents.addAgent` and assert the add-agent dialog or page. This ticks action:agents.forkTask, forkReply, regenerateReply, retractMessage, unarchiveTasks, showToolWindow and addAgent, plus ipc:agent_fork, agent_rollback, agent_unarchive and agent_providers (the composer's provider list names Fake Agent). Add a queued second prompt while one is running to tick ipc:agent_unqueue and agent_send_queued_now. Change the composer's config chip to tick ipc:agent_set_config (`composer_view`). Add a background-task stop to tick ipc:agent_stop_background. Open the changes view of a task that edited a file to tick ipc:agent_changes.

#### workbench (mod.rs `mod tests`, database block): 13 rows

#### Recipe F3: the Database console and tree extras on SQLite (ticks 6 rows)

The op dispatch is in app-gpui/src/workbench/database.rs:128-205 and the action registrations at :670-735. Copy `the_database_trees_actions_and_shortcuts_reach_the_store_and_the_backend` (trusted_project, sqlite_source, db_tree, db_expand, run_action, db_query). Then do the following. ipc:db_explain: run `db.explainPlan` in the console and assert the results pane shows a plan answer (the `Answer::Plan` state) before any `db.execute`; this also needs the Query Plan view, which the partial says is not built, so check first. ipc:db_object_ddl: create a view in sqlite_source, expand the views folder and run `db.goToDdl` on the view; assert the editor text contains `create view`. ipc:db_schema_objects: assert the expanded schema lists the view or trigger that only the objects answer adds. ipc:db_history and db_remove_history: execute twice in the console, open the console's history (the store's History op) and assert both statements, then remove one. ipc:db_completion: type `sel` in a `.sql` editor bound to the source, request completion and assert a `users` table item.

#### Recipe F4: manual commit on SQLite (ticks 2 rows)

Covers ipc:db_set_manual_commit and db_finish_transaction (database.rs:147-151). In the console, switch to manual commit through the console's transaction control. Then insert a row, assert that `db_query` from a second connection does not see it, run commit, and assert that it does.

#### Recipe F5: password prompt reaching the backend (ticks 1 row)

Covers ipc:db_set_password (database.rs:137-139). Extend `the_password_a_connection_asks_for_is_masked_and_answers_the_store`: instead of only subscribing to the prompt event, let the store carry the answer out and assert that the backend holds it for the source. This may need a pg-like source whose connect fails with an auth error, so it is probably not cheap.

#### Recipe F6 (expensive): ssh and server databases (4 rows)

ipc:db_set_ssh_secret, db_trust_ssh_host and db_databases (database.rs:141-153, 1388, 1416) need an SSH server and a server engine (Postgres or MySQL) in the test. They are listed for completeness, not cheap.

#### workbench (features): 2 rows

feature:agents and feature:database: after F1 to F3 land, write the partial proofs naming them. A `done` would need every agents and database view; not cheap.
feature:viewers (features_b::viewers is reached; its partial unit rows previewOnly and zoom show the preview host in the window): a feature row would need every viewer; not cheap, so it stays out of the counts.

#### unit rows with some native coverage: 1 row

unit:src/features/agents/composerFolders.test.tsx (9 cases): `agents::composer::tests::ranks_folders_among_files_by_score_files_first_on_a_tie` covers one. Folder references dragged from the Project view and @-completion inside a folder are in `agents::composer`/`composer_view`. A composer_view test per case would tick it; at most partial today.

### Settings dialog (workbench/settings_window.rs, settings_view) — 24 rows [settings]

#### workbench::tests, Settings dialog: 11 rows to partial with the same test

Rows: `settings:general`, `settings:notifications`, `settings:general.updates`, `settings:build.console`, `settings:build.debugger.dataViews`, `settings:build.deployment.options`, `settings:tools.terminal`, `settings:tools.mcpServer`, `settings:vcs.commit`, `settings:vcs.confirmation`, `settings:vcs.git`.

These pages have keyed controls and also controls of their own that draw "This page is not available yet" (no `set_custom_control` caller exists outside a dialog unit test). Add them to the table above. Each then gets a `partial` entry naming the controls of its own (for example "Popup type" on notifications, "Override console cycle buffer size" on build.console, "Limit subject line" on vcs.commit), not `done`. `general.updates` is shown only when the registry has `app.checkForUpdates` (settings_window.rs:235), and nothing registers that action today, so the test must register a stub handler for it first.

#### workbench::tests (app-gpui/src/workbench/mod.rs), Settings dialog: 8 rows to done with one test

Rows: `settings:general.mode`, `settings:general.language`, `settings:tools.agents.chat`, `settings:build.debugger`, `settings:build.debugger.stepping`, `settings:build.pythonDebugger`, `settings:build.run`, `settings:vcs.shelf`.

Code: pages declared in app-gpui/src/settings_model/builtin_pages/behavior.rs, build.rs, tools.rs and vcs.rs, put into the dialog by `settings_registry` (app-gpui/src/settings_model/builtin_pages.rs:91) from `Workbench::open_settings` (app-gpui/src/workbench/settings_window.rs:202, `file.settings` at :74). Each of these pages is made only of keyed controls (Toggle/Select/Number/Text, no control of its own), and their keys match the webview page's option keys one for one (I compared the key lists by script; labels and order were not compared). Recipe: copy `settings_searches_the_pages_applies_a_ticked_checkbox_and_ok_closes` (mod.rs:9696). For each (page, key, click ids) in a table, such as `("build.run", "run.confirmRerun", toggle)`, `("vcs.shelf", "vcs.shelf.removeApplied", toggle)`, `("general.mode", "ide.mode", select→choice)`, `("general.language", "appearance.locale", select→choice "zh")`, `("tools.agents.chat", "agents.showCost", toggle)`, `("build.debugger", "debugger.hideOnTermination", toggle)`, `("build.debugger.stepping", "debugger.skipLibraryCode", toggle)` and `("build.pythonDebugger", "debugger.python.gevent", toggle)`, do the following: `run_action(cx, "file.settings", Some(page.into()))`; assert `selected_page == page`; assert `control_lines` lists every option label of the page; `click_on(cx, "settings-toggle-<key>")` (or `settings-select-<key>` then `settings-choice-<key>-<value>`); `click_on(cx, "settings-apply")`; assert `setting_of(&global(cx), key)` holds the new value and `global_settings_file(&home)` names it. Close the dialog with `settings-ok` between pages. One test (or one per group of pages) ticks all 8.

#### workbench::tests, Settings page registration: 2 rows

Rows: `settings:(at) src/features/settings/pages/registry.tsx:62` (the built-in pages as a set), `settings:(at) src/features/settings/pages/registry.tsx:82` (Code Style › language pages from the backend's language list).

Code: `settings_registry` (builtin_pages.rs:91) and `code_style_language_pages` (builtin_pages/editor.rs:717), both called from open_settings (settings_window.rs:236-238). Recipe: open `file.settings` and assert that `view.pages(cx)` contains every page id of `settings_registry(false)`, and that a Code Style › <language> entry exists for a language of `backend.host.languages.all()` (for example "Rust"). The first row can be done when the assertion covers every built-in page of the webview. The second needs a check that adding a language updates the pages, because the webview reloads on `onLanguageListChanged` and the native code reads the list only when the dialog opens. Without that check it is partial.

#### workbench::tests, Updates page: 1 row

Row: `settings:(at) src/features/settings/pages/registry.tsx:69`. Code: settings_window.rs:235 shows `behavior::updates()` only when `app.checkForUpdates` is registered. Recipe: in a test, register a stub `app.checkForUpdates` handler, open `file.settings` on `general.updates`, toggle `general.autoCheckUpdates`, apply, and read it back. A second open without the action asserts the page is absent. This ticks this row and gives `settings:general.updates` its partial entry.

#### workbench::tests, Plugins page: 1 row

Row: `settings:plugins`. Code: open_settings installs `PluginsPage` as the page view when a `Plugins` global exists (settings_window.rs:248-253); `plugins_page_event` (settings_window.rs:407) carries out its events. `settings_for_new_projects_and_plugins_open_on_their_pages` (mod.rs:9958) covers only the case with no plugin host. Recipe: copy `plugins::tests::a_linked_plugins_theme_is_offered_and_applied` for a running plugin host with a linked plugin, then run `plugins.settings`. Assert `settings_plugins_page()` is Some and lists the linked plugin. Disable it through the page (`PluginsPageEvent::SetEnabled`) and assert the host reports it disabled.

#### project_tree / workbench::tests: 1 row

Row: `module:src/platform/ignoredNames.ts`. Code: project_tree.rs:201 reads `files.ignoredNames` for the window, and project_tree_model.rs:147 hides matching entries. Recipe: copy `a_project_shows_its_tree_and_opens_files_in_the_editor` with files `a.txt`, `b.pyc` and `__pycache__/c`. Assert the tree lists only `a.txt`, then `platform::settings::set(... "files.ignoredNames", json!([]))` and assert all three show.

### Run and Services (workbench/run_configs.rs, services_window.rs) — 21 rows [run]

#### workbench (mod.rs `mod tests`), run configuration types — one test ticks 10 rows

Rows: `point.registerRunConfigType:shell`, `point.registerRunConfigType:npm`, `point.registerRunConfigType:pytest`, `point.registerRunConfigType:compound`, `point.registerRunConfigType:application`, `point.registerRunConfigType:spring-boot`, `point.registerRunConfigType:go`, `point.registerRunConfigType:go-test`, `point.registerRunConfigType:django-test`, `point.registerRunConfigType:godot`.

The types are registered for every window by `register_builtin_types` in `RunDriver::new` (app-gpui/src/run/driver.rs:169; list in app-gpui/src/run/types.rs:451, jvm_types.rs:195/199, go_types.rs:99/115, django_types.rs:99). Copy `workbench::tests::edit_configurations_saves_a_configuration_added_in_its_form_and_cancel_saves_nothing`: `run_project(cx, vec![])`, `run_action(cx, "run.editConfigs", None)`, then for each type click `run-edit-add`, `simulate_input("<type label>")` + enter, assert the selected entry is the new Unnamed configuration, type its required fields with `type_into` (e.g. shell: `scriptText`/script path; npm: `script`; pytest: target; go: package; application: main class), click `run-edit-ok` and assert `ide_run::load_user_configs(&root)` holds a configuration of that `kind` with the command line the type builds. For `shell` and `compound` (of two Program configurations) also `run_action(cx, "run.run", None)` after selecting it and assert the sessions finish with exit code 0, as `a_run_configuration_runs_into_the_run_tool_window_and_stop_ends_a_long_running_one` does. Types whose tools may be missing on the test host (npm, pytest, go, godot, Java) should stop at the saved command line, which comes only from the type's `command_line`.

#### workbench (mod.rs `mod tests`), Services group actions — one test ticks 4 rows

Rows: `action:run-all`, `action:stop-all`, `action:restart-all`, `action:run.services.hideConfig`.

Registered in app-gpui/src/workbench/services_window.rs:288-321 (`run.services.runAll/stopAll/restartAll` map to `NodeCommand::RunAction { action: "run-all" | "stop-all" | "restart-all" }`; `run.services.hideConfig` at :316). Copy `workbench::tests::a_run_configuration_added_to_services_runs_from_its_row_into_the_detail_pane_and_stop_ends_a_service` with configurations `program("nap1", "/bin/sleep", "30")`, `program("nap2", "/bin/sleep", "30")`: add the Program type, click `services-row:run:type:program`, click `services-action:run-all` and wait until both sessions are Running; click `services-action:restart-all` and assert both have a later `started_at`; click `services-action:stop-all` and assert both are Finished with `stopped`; finally `run_action(cx, "run.services.hideConfig", Some(json!("nap1")))` and assert the `run:config:nap1` row is gone while `run:config:nap2` remains.

#### workbench (mod.rs `mod tests`), Docker in Services without a daemon — one test ticks 4 rows

Rows: `point.registerServiceProvider:docker`, `ipc:docker_settings`, `ipc:docker_save_settings`, `ipc:docker_snapshot`.

`carry_out_docker` (app-gpui/src/workbench/services_window.rs:707-725) calls `ide_docker::load_settings`, `ide_docker::save_settings` and `ide_docker::snapshot`, the same functions the adapters in src-tauri/src/commands/docker.rs call. Write a Docker settings file under the test home's data dir naming a connection whose endpoint is a unix socket path that does not exist, open a project, run `services.show`, and assert the Services tree shows the Docker root and that connection's row (settings loaded), and that the row reports the connection error (only a snapshot attempt can produce it). Then change a Docker setting through the model command that saves settings (e.g. `docker.toggleStoppedContainers`, which is registered at services_window.rs:340) and assert the settings file on disk changed.

#### workbench (mod.rs `mod tests`), run actions run through the registry — one test ticks 3 rows (upgrades three partials)

Rows: `action:run.runConfig` [~], `action:run.stopProcess` [~], `action:run.reloadConfigs` [~]; each partial's stated gap is only "no test runs it through the action".

Registered in app-gpui/src/workbench/mod.rs:682 (`run.runConfig`), :714 (`run.stopProcess`), :720 (`run.reloadConfigs`). Copy `run_project` with `program("nap", "/bin/sleep", "30")`: `run_action(cx, "run.runConfig", Some(json!("nap")))` and wait for a Running session of nap; read its run id from the store and `run_action(cx, "run.stopProcess", Some(json!(id)))`, wait for Finished with `stopped`; then write a second configuration with `ide_run::save_user_configs`, `run_action(cx, "run.reloadConfigs", None)` and wait until `store.configs()` lists it.

### Editor actions and find (editor/*, workbench/editor_find.rs, editor_intelligence.rs) — 19 rows [editorA]

#### workbench (mod.rs tests) — actions with a known gap "no test runs it", 9 rows

**Find popup commands as actions** — ticks `action:find.popup.nextResult`, `action:find.popup.previousResult`, `action:find.popup.openResult`, `action:find.popup.recentSearches` (all upgrades). Code: app-gpui/src/workbench/mod.rs:772-787. Copy `workbench::tests::find_in_files_opens_a_popup_that_searches_opens_and_hands_over`: with the popup open on a query that has 3 matches, run each id through `execute_action_or_throw` and assert the selected row moves down/up, that openResult opens the selected match in the editor (path + caret), and that recentSearches opens the history menu.

**Find bar Scroll to Results toggle** — ticks `action:find.scrollToResults` (upgrade). Code: app-gpui/src/workbench/mod.rs:763-770. In the find-bar test (`workbench::tests::find_opens_a_bar_over_the_tabs_editor_that_searches_replaces_and_closes`), run the id twice and assert `action_toggled` flips and the stored preference follows.

**Go to Class** — ticks `action:navigate.gotoClass` (upgrade). Code: app-gpui/src/workbench/mod.rs:582-595. Copy `workbench::tests::go_to_file_finds_a_file_by_name_and_opens_it`; run `navigate.gotoClass`; assert Search Everywhere opens on the Classes tab.

**Previous Bookmark** — ticks `action:navigate.prevBookmark` (upgrade). Code: app-gpui/src/workbench/mod.rs:615. In `workbench::tests::go_to_line_and_bookmarks_move_the_caret_through_the_project`, after the two `navigate.nextBookmark` runs add `navigate.prevBookmark` and assert the caret is back on the previous bookmark (file and line), and once more wraps round.

**Switcher opened backward** — ticks `action:navigate.switcherBack` (upgrade; gap "opening it backward is tested in the view only"). Code: app-gpui/src/workbench/mod.rs:596-608. In `workbench::tests::recent_files_and_the_switcher_go_between_the_editors`, with Ctrl held and no popup open, run `navigate.switcherBack` first; assert the Switcher opens with the last row selected.

**Close All Tabs** — ticks `action:editor.closeAllTabs` (upgrade). Code: app-gpui/src/workbench/mod.rs:805. `workbench::tests::a_projects_tabs_and_carets_are_restored` already runs the action but only asserts the stored session is gone; add `assert!(tab_titles().is_empty())`, and pin one tab first (`editor.pinTab`) to assert what the webview does with pinned tabs (close_all(pane, false)). editor.pinTab itself stays partial (pinned tabs are not drawn differently).

#### workbench (mod.rs tests) — syntax-tree usages without a server, 3 rows

**Find Usages through the syntax tree** — ticks `ipc:syntax_support`, `ipc:syntax_identifiers_in_files`, `ipc:syntax_identifiers`. Code: `ask_syntax` at app-gpui/src/workbench/code_insight.rs:272-292, driven by app-gpui/src/code_insight/syntax_usages.rs:104-170. Copy `workbench::tests::go_to_declaration_without_a_server_finds_a_local_through_the_syntax_tree` (trusted `.js` project, Light mode, no server). Test 1: two files `a.js` (`export function shared() {}`) and `b.js` (`shared(); // shared`), caret on `shared`, run `find.usages`; assert the rows hold the two code occurrences and not the one in the comment (only identifier occurrences from syntax_identifiers_in_files can drop it; the existing txt-based test gets plain text matches). Test 2: a name re-declared in the same scope (`let x = 1; { let x = 2; x; }`) to reach `SyntaxRequest::Identifiers`; assert only in-scope unqualified occurrences.

#### workbench (mod.rs tests) — context keys, 3 rows

**editorAreaFocus and editorPath** — ticks `context:editorAreaFocus`, `context:editorPath`. Code: app-gpui/src/workbench/mod.rs:1872 (set with the editor's focus) and :1906/:1912. Extend `workbench::tests::a_project_shows_its_tree_and_opens_files_in_the_editor`: assert `editorAreaFocus` on with the editor focused and off when the tree has focus (before Enter), and `editorPath` equal to the file's path, cleared after `editor.closeAllTabs`. Caveat: natively editorAreaFocus is the same as editorFocus; check src/workbench/focus.ts that the webview means the same before ticking.

**dbTableEditorFocus** — ticks `context:dbTableEditorFocus`. Code: `sync_database_context` at app-gpui/src/workbench/database.rs:1858-1866. In the workbench test that opens a table with F4 (mod.rs around line 10682, the one asserting "F4 opens the table"), assert the key on while the table editor has the keyboard and off after focus moves back to the Database tree.

#### workbench (mod.rs tests) — documents, 2 rows

**Closing a tab releases its document** — ticks `ipc:doc_close` (upgrade; gap "no test asserts the backend released it"). Code: editor drop path (app-gpui/src/editor/view.rs, ops::doc::close). In `workbench::tests::files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens`, after closing a tab assert `host.project("w1").documents.open_documents()` no longer holds it (the hex-view test uses the same probe).

**Reload with Encoding** — ticks `ipc:doc_set_encoding` (upgrade; gap "reloading only as the refusal"). In `workbench::tests::the_status_bars_items_open_go_to_line_and_their_menus`, write a Latin-1 file, open it, choose Reload in ISO-8859-1 from the encoding menu with no unsaved changes, assert the editor text shows the accented character and the item reads ISO-8859-1.

#### workbench — editor contributions, 2 rows (would be partial)

**Live Markdown source: scroll and caret** — `point.registerEditorContribution:viewers.markdownLive`. Code: app-gpui/src/workbench/tab_views.rs:273-349 publishes text, caret moves and scrolling; font zoom is passed as 0.0 (tab_views.rs:728), so this stays partial. Extend `workbench::tests::a_markdown_file_opens_as_text_with_its_layouts_and_a_preview_that_follows_the_typing`: in Preview Only, move the hidden editor's caret to a far line and assert the preview's scroll target follows; scroll the editor in split mode and assert the preview top line follows.

**LSP editor contribution** — `point.registerEditorContribution:lsp`. Native: app-gpui/src/editor/intelligence.rs, with completion, hover/diagnostics, signature help, code actions and go to declaration all proven by the seven `editor::intelligence::tests::*`. This can be marked partial now by citing them. Inlay hints, semantic tokens, code lens, document highlights and format are not wired (their models are in unreached editor_lsp units).

### Window context, tool windows, backend events (workbench/mod.rs, tool_windows.rs) — 11 rows [workbenchA]

#### workbench (mod.rs `mod tests`): backend events, 6 rows

One test in the style of `backend_tasks_and_notifications_reach_the_window` (send `BackendEvent { window: Some("w1"), event }` through `workbench.backend_event`) ticks the window-level ones; the editor ones need an open editor.

- `event:DbSourcesChanged`: handled at app-gpui/src/workbench/mod.rs:1705 → `database_sources_changed` (app-gpui/src/workbench/database.rs:819). Copy `new_data_source_tests_and_saves_a_sqlite_file_that_then_shows_in_the_tree`: write a data source into the project's store behind the window's back (the backend op the dialog uses), send `IdeEvent::DbSourcesChanged`, assert the Database tree lists it.
- `event:ProjectTrustChanged`: handled at mod.rs:1710 → `agents_trust_changed` (workbench/agents.rs:360). Send `ProjectTrustChanged { trusted: false, .. }` and assert the agents list is emptied / reloaded as that function does (read the function first; if its only effect is on the unlisted agents tests' state, cite one of those once it is in the test list).
- `event:SettingsChanged`: handled at mod.rs:1672 (Settings dialog, Run consoles, code insight), editor/view.rs:291 (`read_facts`), project_tree.rs:406 (ignored names). Copy `project_tree::tests::follows_changes_on_disk`: change the `files.exclude`/ignored-names setting through the backend without `platform::settings::set`, send `SettingsChanged { key: "files" }`, assert the tree drops the newly ignored entry.
- `event:ExcludedDirsChanged`: project_tree.rs:410 reloads a loaded folder. Same test: mark a loaded directory excluded in the backend, send `ExcludedDirsChanged { dirs: [dir] }`, assert its listing is reloaded (row changes).
- `event:DocPatched`: editor/view.rs:282 → `remote_patch`. Copy `opens_a_loose_file_in_an_editor_and_types_into_it`: apply an edit to the open document through `ide_host::ops::doc` from the host (as a refactoring would), then assert `editor.text()` shows it without a reload.
- `event:DocDirty`: editor/view.rs:285 sets `dirty`. Same test: emit `DocDirty { doc, dirty: true }` through `Backend::emit` and assert `editor.is_dirty()` and the tab's modified mark.

Upgrade of a partial: `event:Notify` (partial, gap "no balloon or Notifications tool window is drawn"): balloons and the tool window are now built. Add to `backend_tasks_and_notifications_reach_the_window`, after the Notify send, the same balloon and tool-window assertions `notifications_show_as_balloons_and_in_their_tool_window` makes (copy them from that test; I did not check which selector it reads); with that the row can be done.

#### workbench (mod.rs `mod tests`): window context keys and tool-window activation, 4 rows

**Run and Services activation actions** (`boundid:toolwindow.run`, `boundid:toolwindow.services`). Both are registered by the `for id in TOOL_WINDOWS` loop at app-gpui/src/workbench/mod.rs:516-526 (`toolwindow.{id}` → `layout.activate(id)`). Copy `workbench::tests::the_git_tool_window_shows_the_projects_log`: open a project with `search_project`, assert `!is_tool_window_visible(RUN_TOOL_WINDOW)`, run `execute_action_or_throw("toolwindow.run", …)`, assert the window is visible and its view is the Run panel (`tool_window_view(RUN_TOOL_WINDOW)`); run it again and assert it hides. Same three lines for `toolwindow.services` / `SERVICES_TOOL_WINDOW`. One test ticks both rows.

**`activeToolWindow` and `projectOpening`** (`context:activeToolWindow`, `context:projectOpening`). Published in `Workbench::publish_context` at app-gpui/src/workbench/mod.rs:1851 (`projectOpening`) and :1897-1898 (`activeToolWindow`, cleared when no window is active). `a_run_configuration_runs_into_the_run_tool_window_and_stop_ends_a_long_running_one` asserts `activeToolWindow == "run"` (on) only, and `a_project_shows_its_tree_and_opens_files_in_the_editor` asserts `projectOpening` off only. Extend `tool_window_actions_show_focus_and_hide_the_project_view`: after `cmd-1` hides the Project view, assert `window_context(..).get("activeToolWindow")` is `None`, and after showing it, `Some(Text("project"))`. For `projectOpening`, in a fresh window call `open_project(root)` and read the key before `settle` (the content is `Content::Opening`), assert it truthy, then after the tree appears assert it off.

#### agents (mod.rs `mod tests`), 1 row

`point.registerToolWindow:agents`: registered in `TOOL_WINDOWS` (mod.rs:417, `crate::agents::model::TOOL_WINDOW`) with actions in workbench/agents.rs:182. Five tests that exercise it already exist in app-gpui/src/workbench/mod.rs (lines 7105-7300: `a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn`, `the_agents_actions_keep_the_conversation_docked_and_never_show_the_agents_key`, …) but they are not in rust_tests.txt yet. Once listed, `workbench::tests::a_task_started_from_the_tool_window_streams_its_reply_and_stop_cancels_the_turn` is the proof to cite (read it first).

### Editor search popups and refactoring — 10 rows [editorB]

#### workbench::tests (search popups): 4 rows

Rows: `unit:src/features/search/__tests__/dialogs.test.tsx` (22 cases), `unit:src/features/search/__tests__/codePreview.test.tsx`, `unit:src/features/search/__tests__/pathScopeActions.test.tsx`, `unit:src/features/search/__tests__/toggleActions.test.ts`.

- **dialogs:** copy `workbench::tests::search_everywhere_moves_its_selection_shows_more_and_answers_the_mouse` (mod.rs:4953). Assert recent files for an empty query, `/` scope commands, ⌥↑/⌥↓ history, ⇥ scope cycling and the Actions-tab checkbox. Most of these are separate tests, and 22 cases means partial at best.
- **codePreview:** copy `find_in_files_opens_a_popup_that_searches_opens_and_hands_over` (6200). Select a match and assert that the popup's FilePreview (find/popup_view.rs:210) marks the current line. The spec's editable preview that shares undo with a tab is likely not built.
- **pathScopeActions:** focus a directory in the project view, run `find.inPath`, and assert the popup's directory scope.
- **toggleActions:** open Search Everywhere's Actions tab and assert that the toggle check state appears only for actions registered with `handle_toggle` (platform/actions.rs:286).

#### workbench::tests (refactoring): 3 rows

Rows: `module:src/editor/refactor/catalog.ts`, `module:src/editor/refactor/safeDelete.ts`, `module:src/editor/refactor/index.ts` (a barrel; it also re-exports hierarchy, which is not built, so `[~]` at most).

safe_delete and catalog are used by app-gpui/src/project_view/file_dialogs.rs and workbench/project_files.rs. Copy `workbench::tests::a_confirmed_delete_searches_deletes_and_reports_what_could_not_be_deleted`. Assert the usages-found dialog and that the catalog marks a refactoring the server cannot perform as unavailable.

#### Partial candidates (features): 3 rows

Rows: `feature:code-insight`, `feature:templates`, `feature:vim`. Each feature is live in the app, but no test proves it whole.

- **code-insight:** usages, problems, hierarchy and server status are wired; workspace undo is not.
- **templates:** live templates, postfix and surround are wired; file templates and emmet are not.
- **vim:** the mode and the status item are wired (`workbench::status_items::tests::the_vim_item_shows_the_mode_while_on_and_a_faint_name_while_off`, `features_a::vim::mode::tests::vim_toggle_turns_vim_on_and_off_for_every_editor_live_and_persists_it`).

Each can get a `[~]` entry citing those tests now. Done needs the missing parts built, so this is not a single-test job.

### VCS (handlers in workbench/branches.rs, commit.rs, log_window.rs, diff_window.rs, conflicts.rs) — 9 rows [vcs]

#### workbench::tests — Git log window (3 rows)

- Rows: `ipc:vcs_log_users` (partial). Code: `app-gpui/src/workbench/log_window.rs:233-240` (`LogFilterEvent::LoadUsers` calls ops::vcs::vcs_log_users). Copy `the_log_filters_by_text_and_by_the_branch_chosen_in_the_branches_pane`; commit as two authors (`-c user.name=Ada` and `-c user.name=Bob`), open the User filter of the log filter bar (emit/click its user filter button so the bar sends `LoadUsers`), and assert the filter's popup lists exactly Ada and Bob; choosing Bob leaves only Bob's commit in the log.
- Rows: `ipc:vcs_commits_changes` (partial), `ipc:vcs_diff_numstat` (partial). Code: `app-gpui/src/workbench/log_window.rs:76-78` (`ChangesPart::Commits` → vcs_commits_changes) and `:84-96` (`read_line_stats` → vcs_diff_numstat). Copy `a_selected_commit_shows_its_details_and_the_diff_of_a_changed_file`; select two commits (shift-click/shift-down in the log) and assert the details pane lists the union of both commits' files; for a single commit that adds 3 lines and removes 1, assert the details pane's line counts (+3 −1) for that file. One test can tick both rows.

#### workbench::tests — diff window (2 rows, one test)

- Rows: `action:diff.prevFile` (partial), `action:diff.showSettings` (partial). Code: both registered in `app-gpui/src/workbench/diff_window.rs:169-180` (`register_actions`: the NEXT_FILE/PREV_FILE loop calls `step_diff_file`, SHOW_SETTINGS calls `show_diff_settings`). Copy `show_diff_on_a_change_of_the_commit_window_shows_and_walks_its_diff` up to the `diff.nextFile` step (the dialog shows b.txt), then `execute_action_or_throw("diff.prevFile", …)` and `settle` until `dialog.data().path.ends_with("a.txt")` with left/right changed lines `["two"]`/`["TWO"]`; then `execute_action_or_throw("diff.showSettings", …)` and assert the settings menu is open in the workbench's popup slot (`workbench.popup.is_some()`, as `the_merge_editor_opened_from_the_banner_…` asserts for `merge.settings`), Escape closes it. Ticks both partial rows to done.

#### workbench::tests — branches (2 rows)

- Rows: `ipc:vcs_remove_worktree` (partial). Code: `app-gpui/src/workbench/branches.rs:755-765` (`DeleteStep::RemoveWorktree`). Copy `new_branch_rename_and_delete_change_the_repositorys_branches`; before deleting, `git worktree add ../wt feature` in the fixture, delete `feature` from the branch popup, confirm the worktree question, and assert `git worktree list` no longer shows `wt` and `git branch` no longer lists `feature`.
- Rows: `ipc:vcs_merge_base` (partial). Code: `app-gpui/src/workbench/branches.rs:233-240` (`published_tip` on an update), called at `:255`/`:267`. Copy `push_sends_a_new_commit_to_origin_and_fetch_shows_what_came_in` (local bare origin); push one new commit to origin from a second clone, run Update Project from the branch popup, and assert the success notice names the commit(s) that came in (the before/after tips come from vcs_merge_base). The row's own note mentions the checkout diverged-branch question instead: a second test could check out a branch that diverged from its upstream and assert that question is asked.

#### workbench::tests — database (1 row)

- Rows: `context:dbTableEditorChanges`. Code: `app-gpui/src/workbench/database.rs:1859-1867` (`sync_database_context` sets it to focused && dirty) and `:804` (cleared on project close). Extend `edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one`: focus the table editor (`editor.focus_handle(cx).focus(window)`), assert `window_context(window, cx).is_truthy("dbTableEditorChanges")` is false, edit a cell and assert it true, run `db.submitChanges` and assert it false again after the page reloads (revert gives a second on/off pair).

#### workbench::tests — merge-commit diff (1 row)

- Rows: `unit:src/features/vcs/__tests__/GitMergeDiff.test.tsx` (1 case). Code: `app-gpui/src/workbench/log_window.rs:70-75` (`ChangesPart::Parent` diffs a merge commit against each parent via vcs_diff_revisions) and `vcs_more/log_changes.rs` (reached). Copy `a_selected_commit_shows_its_details_and_the_diff_of_a_changed_file` with a fixture that makes a non-fast-forward merge commit touching the same file on both sides; select the merge commit and assert the details pane offers both parents and that Show Diff of the file opens a diff whose panes are read-only (no worktree side). Hypothesis to check while writing it: the spec also asserts highlighting of both parents in one view; if the native diff shows one parent at a time the row stays partial.

## editor::intelligence — 33 rows

### editor::intelligence tests (fake language server): 26 rows, best done as 5 to 6 tests [editorB]

Rows: `module:src/editor/lsp/completion.ts`, `module:src/editor/lsp/completionItems.ts`, `module:src/editor/lsp/autoImport.ts`, `module:src/editor/lsp/snippet.ts`, `module:src/editor/lsp/hover.ts`, `module:src/editor/lsp/docs.ts`, `module:src/editor/lsp/markdown.ts`, `module:src/editor/lsp/markdownParser.ts`, `module:src/editor/lsp/diagnostics.ts`, `module:src/editor/lsp/signature.ts`, `module:src/editor/lsp/intentions.ts`, `module:src/editor/lsp/workspaceEdit.ts`, `module:src/editor/lsp/navigation.ts`, `module:src/editor/lsp/Popups.tsx`, `module:src/editor/lsp/popupStore.ts`, `unit:src/editor/lsp/Popups.test.tsx`, `module:src/editor/lsp/hint.ts`, `module:src/editor/lsp/lsp.ts`, `module:src/editor/lsp/availability.ts`, `module:src/editor/lsp/files.ts`, `module:src/editor/lsp/fileReferences.ts`, `module:src/editor/lsp/configurationNavigation.ts`, `module:src/editor/lsp/syntaxFallback.ts`, `unit:src/editor/lsp/syntaxFallback.test.ts`, `module:src/editor/lsp/actions.ts`, `module:src/editor/lsp/index.ts`, `module:src/editor/navigation/index.ts`, `module:src/editor/lsp/theme.ts`.

The code is in app-gpui/src/editor/intelligence.rs: the actions are registered from line 1345 (`code.quickDocumentation`, `code.showIntentions`, `code.parameterInfo`, then the `navigate.*` go-to table at 1355), the syntax fallback is at 749, and the views are editor_lsp::{completion_view,hover_view,signature_view,intentions_view,targets_view}. Five tests already drive it with a scripted server: `editor::intelligence::tests::typing_completes_from_the_server_and_enter_inserts_the_item_with_its_import` (1708), `cmd_click_goes_to_the_declaration_or_chooses_between_several` (1797), `problems_are_underlined_and_the_hover_shows_them_with_the_documentation` (1869), `parameter_info_opens_after_a_parenthesis_and_follows_the_commas` (1932) and `show_intentions_lists_the_code_actions_and_applies_the_chosen_one` (1962). Those five can already be cited as partial proofs (`[~]`) for completion, completionItems, autoImport, hover, docs, diagnostics, signature, intentions and navigation.

The tests still to write, copying the same fixture:

- (a) A go-to-declaration with the server answering nothing, so `syntax_fallback` jumps to a local definition. Assert the caret position. This covers syntaxFallback, files and fileReferences.
- (b) A code action whose `WorkspaceEdit` touches two files, run from Show Intentions. Assert both documents changed and the preview or summary text. This covers workspaceEdit.
- (c) A request that the server rejects as unavailable. Assert the hint text and that a repeat inside the backoff does not reach the server. This covers hint, lsp and availability.
- (d) Cmd-click on a key in a configuration file (application.properties or yaml), where the server reports the target. Assert the opened file. This covers configurationNavigation.
- (e) The numbered-row and → submenu cases of the targets and intentions popups. This covers Popups and popupStore.

The index, actions and theme modules tick once the rest do, because they hold only activation, registration and styles.

### editor::intelligence (fake language server fixture) — up to 7 rows [editorA]

**Call and type hierarchy with a server** — ticks `action:navigate.callHierarchy` (upgrade), `action:navigate.typeHierarchy` (upgrade), `ipc:lsp_call_hierarchy`, `ipc:lsp_type_hierarchy`. Code: actions at app-gpui/src/workbench/code_insight.rs:172 and :175; the backend calls are `hierarchy_level` at app-gpui/src/workbench/code_insight.rs:296-318 (ops::lsp::lsp_call_hierarchy / lsp_type_hierarchy, the adapters' ops). Copy `editor::intelligence::tests::cmd_click_goes_to_the_declaration_or_chooses_between_several` (its `open` fixture installs `fake_server::FakeServer` for `*.mock` files and answers any method from JSON). Answer `textDocument/prepareCallHierarchy` with one item and `callHierarchy/incomingCalls` with one caller; run `navigate.callHierarchy` through `execute_action_or_throw`; assert the hierarchy tool window's rows list the caller and that `server.received()` contains both methods. Repeat with `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes` for `navigate.typeHierarchy`. Two short tests, or one test doing both.

**Language server status: list and restart** — ticks `ipc:lsp_servers`, `ipc:lsp_restart`. Code: `list_servers` at app-gpui/src/workbench/code_insight.rs:1252 and `ServerStatusEvent::Restart` at :1278. Same fixture: after the mock server starts, assert the server status item lists the `mock` server (this list comes only from ops::lsp::lsp_servers); emit Restart from the item and assert the fake server log shows a second `initialize`.

**Completion item command** — ticks `ipc:lsp_execute_command` (upgrade; current gap "no test item has one"). Code: app-gpui/src/editor/intelligence.rs:585. Copy `editor::intelligence::tests::typing_completes_from_the_server_and_enter_inserts_the_item_with_its_import`, give the answered item a `command`, accept it, assert `server.received()` contains `workspace/executeCommand` with that command.

**References with a server** — ticks `ipc:lsp_references` (upgrade; gap "no language server answers in tests"). Code: app-gpui/src/workbench/code_insight.rs:862. Same fixture; answer `textDocument/references` with two locations in the `.mock` files, run `find.usages`, assert the Find Usages rows are exactly those two (not text matches).

**Structure popup inherited members** — ticks `ipc:doc_open_virtual` (and exercises lsp_type_hierarchy from app-gpui/src/everywhere/structure_view.rs:178). Code: app-gpui/src/everywhere/structure_view.rs:196. Fixture answers `textDocument/documentSymbol`, `textDocument/prepareTypeHierarchy` and `typeHierarchy/supertypes` with a supertype whose uri is a virtual (non-file) document; run `navigate.fileStructure` twice (second press shows inherited members); assert the supertype's members are listed. Only worth it if the virtual-document path is reachable with a fake uri; otherwise use a second `.mock` file and drop the doc_open_virtual claim.

## ui — 22 rows

### ui (reached ui:: / run:: modules): 15 row(s) [workbenchB]

Rows: `module:src/ui/Dialog.tsx`, `module:src/ui/List.tsx`, `module:src/ui/Tree.tsx`, `module:src/ui/Tabs.tsx`, `module:src/ui/Popup.tsx`, `module:src/ui/Select.tsx`, `module:src/ui/Scrollbar.tsx`, `module:src/ui/Splitter.tsx`, `unit:src/ui/Dialog.test.tsx`, `unit:src/ui/List.test.tsx`, `unit:src/ui/Tree.test.tsx`, `unit:src/ui/Tabs.test.tsx`, `unit:src/ui/Popup.test.tsx`, `unit:src/ui/Select.test.tsx`, `unit:src/ui/Scrollbar.test.tsx`

React design-system primitives with ad hoc native equivalents (dialogs, lists, trees, tab strips, popups, selects, scrollbars, split handles drawn per view). No single native module mirrors them; proving one means porting its spec's cases against a representative native view (e.g. Tabs.test against editor::tabs and `a_tabs_menu_closes_pins_and_splits`). Low priority, not one-test ticks.

### ui (reached ui:: / run:: modules): 7 row(s) [workbenchB]

Rows: `module:src/ui/ansi.ts`, `module:src/ui/ConsoleFindBar.tsx`, `module:src/ui/Input.tsx`, `module:src/ui/Tooltip.tsx`, `module:src/ui/rowSelection.ts`, `unit:src/ui/rowSelection.test.ts`, `module:src/ui/symbolKind.ts`

Native counterparts exist in reached units: run::ansi (4 tests), the run console's find bar over ui::console_find (run/view.rs:27), ui::text_field, ui::tooltip, find::row_selection, the symbol-kind icons. For rowSelection the spec's one case (range across section headers up and down from an anchor, toggle on a header is a no-op) is not exactly mirrored by find::row_selection's two tests: add `fn selects_only_options_across_headers_in_either_direction` with the spec's six rows to find/row_selection.rs, then unit and module rows are done. The others need a module audit.

## workbench::settings_window — 7 rows

### workbench::settings_window (or mod.rs tests next to `settings_for_new_projects_and_plugins_open_on_their_pages`): 6 rows [features]

#### Recipe F7: Settings › Plugins with a real plugin host (ticks 6 rows)

The Plugins page's events are carried out in app-gpui/src/workbench/settings_window.rs:407-475: listing with sync_project, the catalog, the market, enable and reload. Combine the plugin host setup of `plugins::tests::a_linked_plugins_theme_is_offered_and_applied` (app-gpui/src/plugins.rs:484; it links a plugin folder and inits the host) with `settings_for_new_projects_and_plugins_open_on_their_pages`. Put a plugin in the project's plugin folder of a trusted project and run `plugins.settings`. Assert the page lists both the linked plugin and the project's plugin. Toggle the linked one off and assert `host.list` reports it disabled and the page row says Disabled. Reload it and assert its finished answer. With the catalog dirs, assert the Available tab lists a catalog plugin. This ticks ipc:plugin_list (it upgrades the partial: project plugins are now included), ipc:plugin_set_enabled, ipc:plugin_reload, ipc:plugin_catalog, ipc:plugin_market (an empty market URL lists none; set a file:// index to assert one), and action:plugins.settings (it upgrades the partial: the page is available with a host).

### settings_window (template pages): 1 row [editorB]

Row: `unit:src/features/templates/__tests__/views.test.tsx`. The live, postfix and file template pages are in settings_model/builtin_pages/features.rs. Copy `workbench::settings_window::tests`. For each of live, postfix and file templates, open that page, edit an entry and read it back through `templates::live_store` or `templates::postfix_store`. This is hypothesis-level: check first that the settings view renders those pages, not just lists them.

## wit — 7 rows

### wit (plugins::tests): 7 row(s) [workbenchB]

Rows: `wit:wit/exports.wit#commands`, `wit:wit/exports.wit#lifecycle`, `wit:wit/workspace.wit#editor`, `wit:wit/workspace.wit#invocation`, `wit:wit/host.wit#log`, `wit:wit/host.wit#storage`, `wit:wit/types.wit#types`

`plugins::tests::a_plugins_command_runs_its_wasm_on_the_editors_selection` already runs a WASM command export from the app (activating the plugin) on the editor's selection. Copy it with the string-tools plugin (or a fixture plugin) and assert, per interface, one observable effect: the command's edit applied (commands, invocation, editor), the plugin Active after activation (lifecycle), a log line in the plugin log, a stored value read back (storage).

## editor::view — 5 rows

### editor::view tests (highlighting): 5 rows, one test per language [editorB]

Rows: `unit:src/editor/core/cm/cmake.test.ts` (7 cases), `unit:src/editor/core/cm/shellInterpolation.test.ts` (4), `unit:src/editor/core/cm/interpolation.test.ts` (4; Groovy has no native grammar, so at most 3 of 4), `unit:src/editor/core/cm/syntaxHighlight.test.ts` (2) and `module:src/editor/core/colorSchemeRuntime.ts`.

Native highlights come from `ide_host::ops::doc::doc_highlights` (app-gpui/src/editor/view.rs:1257). Grammar queries for cmake, bash, kotlin and csharp exist in crates/ide-language/src/syntax/grammars.rs (`native_highlights`, plus kotlin's own query). Copy `editor::view::tests::rust_is_highlighted_without_codemirror` (view.rs:2227): open CMakeLists.txt, script.sh, Main.kt and Program.cs, wait for highlights, and assert `SyntaxHighlightKind` at the tokens each spec case names. The tokens are command, variable expansion inside a quoted string, bracket comment, `$(...)` inside a string, a Kotlin `${}` template and a C# `{x}` in `$"..."`. Each unit row needs its case count in tests, or a `;` clause explaining that the spec's tables are loops. For syntaxHighlight, add an edit before the highlights arrive and assert that the stale answer is dropped. For colorSchemeRuntime, check first that a chosen scheme reaches `syntax_token` in editor::view. If it does, switch the scheme through settings in a workbench test and assert the editor's syntax colour; if it does not, this row belongs in C.

## editor::command_actions / editor::view::editing_tests — 5 rows

### editor::command_actions / editor::view::editing_tests: 5 rows [editorB]

Rows: `unit:src/editor/core/cm/multiCaret.test.ts`, `module:src/editor/core/clipboardHistory.ts`, `module:src/editor/core/actions.ts`, `module:src/editor/core/backend.ts`, `module:src/editor/core/index.ts`.

- **multiCaret:** copy `editor::view::editing_tests::the_keymaps_caret_click_adds_and_removes_carets`. Repeat it for each keymap preset, rebind the caret-click gesture through the keymap, remove the binding, and assert that Alt-drag then makes a column selection.
- **clipboardHistory:** editor::view records copies into `EditorClipboard(ClipboardHistory)` (view.rs:22, 153). Copy `editor::view::editing_tests::clipboard_keys_copy_cut_and_paste`, then assert the history entries' order and their cap.
- **actions, backend and index:** these are the editor's action table and document IPC (1207 and 629 lines). `editor::command_actions::tests::every_command_is_run_through_the_registry_by_a_test` and the doc_* IPC tests are the natural proof. List the ids the TS table registers that command_actions lacks, then decide between `[~]` and done.

## workbench::services_window — 4 rows

### workbench::services_window (mod.rs tests): 2 row(s) [workbenchB]

Rows: `action:(dynamic) src/features/run/services.tsx:620`, `action:open-browser`

run.services.runAll/restartAll/stopAll/openBrowser are registered in the loop at workbench/services_window.rs:291 and dispatch NodeCommand::RunAction with the button id (open-browser is the row button built at run_testing/services.rs:736). Copy `a_run_configuration_added_to_services_runs_from_its_row_into_the_detail_pane_and_stop_ends_a_service`: add a shell configuration, run `run.services.runAll` with the type group's key and assert the session runs, `run.services.stopAll` and assert it ends. For open-browser a configuration that reports an HTTP port is needed; assert the button is enabled only while it runs (opening the URL itself is not observable).

### workbench::services_window (mod.rs tests): 2 row(s) [workbenchB]

Rows: `action:delete`, `action:refresh`

Docker's item buttons (refresh, delete) come from services::docker DockerStore::actions, shown by workbench/services_window.rs:116. A test needs a fake docker endpoint; the Docker store is fed by DockerRequest/DockerCommand, so a test can drive `carry_out_docker` with a canned listing and assert the refresh request is issued and delete removes the container row. Not cheap without a docker double.

## find::popup_view — 3 rows

### find::popup_view tests — 2 rows [editorA]

**Project file masks** — ticks `ipc:index_file_extensions`, `ipc:languages` (the native side reads `host.languages.all()` directly, the adapter's source). Code: `load_project_masks` at app-gpui/src/find/popup_view.rs:971-1004. Copy `find::popup_view::tests::the_file_mask_limits_the_files_searched` (real backend, project with `a.rs`, `b.rs`, `c.txt`), with no recent masks stored; open the masks menu (`find-popup-masks`) and assert it offers `*.rs` first (most files) and other language extensions after.

### find (reached find::ipc): 1 row [settings]

Row: `module:src/platform/textSearch.ts`. The webview module wraps `search_start` streaming and `cancel_request`; native find code calls `ide_host::ops::search::start` (see ipc_triage). Recipe: a Find in Files workbench test that asserts streamed results arrive and that a second query cancels the first. Whether an existing find test already asserts cancellation was not checked: hypothesis; check with `grep -n cancel app-gpui/src/find/*.rs`.

## platform::keymap::tests — 3 rows

### platform::keymap::tests (app-gpui/src/platform/keymap.rs): 3 rows [settings]

Rows: `unit:src/platform/keymaps/eclipse.test.ts` (8 cases), `unit:src/platform/keymaps/windows.test.ts` (6), `unit:src/platform/keymaps/navExtras.test.ts` (2). The presets are the webview's files, loaded from assets/gen/keymaps.json (keymap.rs:36-43), so every case of these specs can be written over `preset(id)` / `preset_order(id)` / `KeymapState`. Recipe: port each `it` case by name, next to `every_preset_holds_valid_normalized_bindings`. Examples: the Windows presets cover the same action ids as their macOS presets; no Windows preset binding contains `Meta`/`Win`; `vscode-windows` reads ⌘ as Ctrl; eclipse switching via `apply_settings` drops stale overrides and can parent a custom keymap (copy `applies_settings_with_validation_and_custom_keymaps`); in intellij-macos ⇧⌘T binds only Go to Test. That makes 16 tests for 16 cases.

## run — 3 rows

### run (reached run::store / run::java_launch): 3 rows [settings]

Rows: `module:src/platform/javaLaunch.ts` (run::java_launch, used by run/store.rs:17 and run/edit_configs_view.rs:23), `module:src/platform/serverPort.ts` (`started_port`, run/store.rs:351), `module:src/platform/runTargetSelection.ts` (`with_run_target`, run/store.rs:239). Recipe: copy `workbench::tests::a_cargo_build_lists_its_error_succeeds_once_fixed_and_ends_cancelled_when_stopped` for a run configuration. For serverPort, run a script printing `Listening on http://localhost:8123` and assert the run tab/services row shows port 8123. For runTargetSelection, set a run target in settings and assert the started configuration carries it. For javaLaunch, the run shard should decide (a JVM run needs a JDK).

## run::view or workbench — 2 rows

### run::view or workbench — key claims, 2 rows (would be partial) [editorA]

**Run console find bar keeps the IDE keymap out** — ticks `point.registerKeyClaim:(at) src/features/run/ConsoleView.tsx:346`. Code: claims at app-gpui/src/run/view.rs:217-232, handler `find_key` at :797. The run::view tests open the console in a bare window with no keymap, so they do not show the claim winning. In a workbench test with the keymap loaded, run something that prints three matches, open the console's find bar, press `f3` and `cmd-g`, and assert the bar's counter moves and the editor's `find.next` did not run.

**Diff window closes on its own key** — `point.registerKeyClaim:(at) src/features/vcs/diffDialog.tsx:85`. Code: app-gpui/src/vcs_diff/dialog_view.rs:114-123 claims plain Escape; the webview claims the `toolwindow.closeContent` bindings (⌘W). `vcs_diff::dialog_view::tests::keeps_the_changes_tree_mounted_underneath_and_closes_on_esc` proves Escape. This can only be partial until ⌘W is claimed too.

## workbench::project_files — 1 rows

### workbench::project_files (copy path / reference): 1 row [editorB]

Row: `module:src/editor/core/copyReference.ts`. workbench/project_files.rs:1412 builds the reference with editor_commands::copy_reference. The model tests (`editor_commands::copy_reference::tests`) and the popup tests (`project_view::copy_path_popup::tests`) exist. No test runs `editor.copyReference` in a window on a real file. Copy a workbench test that opens a Java or Rust file, run Copy Path/Reference with the caret on a method, choose Copy Reference, and assert the clipboard reads `pkg.Class#method` (Rust: `crate::mod::fn`).

## workbench::status_items — 1 rows

### workbench::status_items: 1 row [editorB]

Row: `unit:src/editor/core/view/statusItems.test.tsx`. `editor.breadcrumbs` is registered at workbench/status_items.rs:20/47. Copy `workbench::tests::the_status_bar_follows_the_editors_caret_and_selection`, then move the caret across a symbol boundary and assert that the breadcrumb text changes. The second case, workspace member breadcrumbs, needs a multi-module project. Check first that the breadcrumb view draws symbols; if it does not, the row is C (editor_model::nav_bar).

## plugins — 1 rows

### plugins (app-gpui/src/plugins.rs tests): 1 row [features]

#### Recipe F8: plugin events reach the app (1 row)

Covers ipc:plugin_subscribe. Natively, plugins.rs:199-250 (supervise and pump) receives the host's events in place of the IPC channel. A test in `plugins::tests` that makes a linked plugin raise an event (a notification or a status item) and asserts the window received it would prove it. This is hypothesis: the native path uses a different function from the adapter's (the host's subscriber versus `plugin_subscribe`), so a reviewer may want this row marked partial.

## run::segments — 1 rows

### run::segments (unit tests in app-gpui/src/run/segments.rs) — one test ticks 1 row [run]

Row: `unit:src/features/run/segments.test.ts` (3 cases; `run::segments::tests` has 3 tests but none splits a find match inside a link, and none checks that the current match keeps its span style). Add `splits_matches_inside_links` (a url link over "http://x" and a mark over "://x" gives "see ", "http" linked, "://x" linked and marked) and extend `splits_at_find_matches_and_flags_the_current_one` with a coloured span; then cite the three tests plus the new one.

## workbench::tests or workbench::menu::tests — 1 rows

### Settings dialog (workbench/settings_window.rs, settings_view) — 1 rows [settings]

#### workbench::tests or workbench::menu::tests, quit: 1 row

Row: `ipc:app_quit`. Code: `app.quit` handler at app-gpui/src/workbench/menu.rs:393. It asks each window's `confirm_close` and calls `cx.quit()` when all agree. Recipe: open a window with a dirty editor (as `opens_a_loose_file_in_an_editor_and_types_into_it`), run `app.quit`, and assert the unsaved-changes question is shown and the window is still open. A second run on a clean window should reach quit; whether gpui's test context lets `cx.quit()` be observed was not checked. This also gives `action:app.quit` the "a test runs it" part of its gap. The gap where it does not quit after the question is answered is a code gap.

## platform::keymap / platform::dispatcher / settings_view::keymap — 1 rows

### platform::keymap / platform::dispatcher / settings_view::keymap: 1 row [settings]

Row: `unit:src/platform/keymapMouse.test.ts` (13 cases). Mouse bindings are implemented and tested piecemeal: `platform::keymap::tests::recognizes_mouse_bindings`, `formats_macos_style`, `normalizes_modifier_order_and_case`, `platform::dispatcher::tests::mouse_shortcuts`, `platform::input::tests::side_buttons_run_mouse_bindings`, and `settings_view::keymap::tests::records_and_submits_option_command_left_click` and its siblings. Recipe: map the 13 cases one by one with `// src/platform/keymapMouse.test.ts › <case>` comments, as keymap.rs already does for keymap.test.ts, and add the missing ones. The row is done when there are 13 named tests or a `;` reason.

## platform::actions::tests — 1 rows

### platform::actions::tests: 1 row [settings]

Row: `unit:src/platform/locales/actionTitles.test.ts` (1 case). Recipe: iterate `manifest()` and, for every action whose title is a literal (not a catalog key, not `verbatim`), assert `meta.localized(&zh)` differs from the English title (zh is `I18n` with `set_locale(Some("zh"), "en")`), as in `localizes_titles_through_catalog_keys_then_source_text` (actions.rs:445).

Near-miss (partial row, not counted in B): `unit:src/platform/actions.test.ts` says "error propagation of `execute_action_or_throw` has no window test yet". `workbench::tests::the_keymap_runs_editor_actions_on_the_windows_editor` already asserts that `execute_action_or_throw("file.save")` errs in a window with no editor. Adding `assert!(execute_action("file.save", …))` (the non-throwing path returns true and logs) covers the spec's one case, and the row can then be done.

## editor_lsp::navigation — 1 rows

### editor_lsp::navigation (reached): 1 row [settings]

Row: `module:src/platform/navigationModifier.ts`. Code: app-gpui/src/editor_lsp/navigation.rs (`is_navigation_gesture`, `navigation_modifier_held`, `reset_navigation_modifier`). Recipe: a workbench test that holds ⌘ over an identifier in an editor with a navigation provider, asserts the link underline/state, and asserts it is reset on window blur. Copy `editor_lsp::navigation::tests::follows_a_different_mouse_binding_assigned_to_the_implementation_action` for the setup.
