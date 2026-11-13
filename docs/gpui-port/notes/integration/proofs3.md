# proofs3: tests for rows whose recorded gap is "only a test is missing"

Nothing was compiled or run. Every claim below about behaviour comes from reading the code (hypothesis) unless it says otherwise.

## `mod` lines to add

1. `app-gpui/src/workbench/mod.rs`, inside `mod tests {`, after the anchor line `    mod proofs_vcs;`:

```rust
    mod proofs3_context;
    mod proofs3_rename_lsp;
    mod proofs3_run;
    mod proofs3_windows;
```

2. `app-gpui/src/workbench/debug.rs`, inside `pub(super) mod tests {`, after the anchor line `    mod proofs_debug;`:

```rust
    mod proofs3_debug;
```

3. `app-gpui/src/workbench/editor_language.rs`, inside `mod tests {` (line ~1547), after the anchor line `    use std::path::Path;`:

```rust

    mod proofs3_editor_keys;
```

(`editor_language.rs` has no child test module yet; the file lives at `app-gpui/src/workbench/editor_language/tests/proofs3_editor_keys.rs`.)

## Files

- `app-gpui/src/workbench/tests/proofs3_run.rs` (5 tests)
- `app-gpui/src/workbench/tests/proofs3_windows.rs` (2 tests)
- `app-gpui/src/workbench/tests/proofs3_rename_lsp.rs` (1 test)
- `app-gpui/src/workbench/tests/proofs3_context.rs` (2 tests)
- `app-gpui/src/workbench/debug/tests/proofs3_debug.rs` (4 tests, plus a `dart` fixture `open_dart`)
- `app-gpui/src/workbench/editor_language/tests/proofs3_editor_keys.rs` (9 tests)

23 tests in all. No accessor had to be added: every accessor used exists (checked: `RunStore::{sessions, temporary, config, configs, selected_config}`, `RunSession::{run_id, accepts_input, started_at}`, `RunView::{lines, pending_input}`, `ServicesView::{rows, toolbar_actions}`, `Workbench::{open_menu, app_dialog, debug_driver, file_dialog, tool_window_view}`, `PopupMenu::lines`, `BreakpointsDialog::rows`, `FakeServer::{install, answer, received}`, test helpers `run_project`, `program`, `wait_long`, `click_on`, `run_action`, `select_in_tree`, `text_of`, `context_text`, `context_is_on`, `trusted_project`, `close_backend_project`, and in `debug::tests` `open`, `long`, `run`, `context`, `notification_titles`, `Fixture`).

## Per test → rows

### `workbench::tests::proofs3_run`

- `run_configuration_runs_the_named_or_given_configuration_and_stop_process_ends_the_process`
  - `action:run.runConfig` → **done**. Proposed proof: "`workbench::tests::proofs3_run::run_configuration_runs_the_named_or_given_configuration_and_stop_process_ends_the_process`; run through the registry by id (the configuration runs, exits 0, its output shows in the Run tool window) and given whole with `temporary` (it runs and is kept as a temporary configuration)".
  - `action:run.stopProcess` → **done**. Proof: "same test; run through the registry with the running process's run id: the session ends as stopped".
- `reload_run_configurations_lists_a_configuration_saved_since_the_project_opened`
  - `action:run.reloadConfigs` → **done**. Proof: "`…proofs3_run::reload_run_configurations_lists_a_configuration_saved_since_the_project_opened`; a configuration saved after the project opened is listed only once the action has run through the registry". The "nothing lists it by itself" step holds by reading: `RunDriver::load` is called only by the project open (`mod.rs:2245`), this action (`mod.rs:775`), `when_loaded`, `reload` (after saves) and the debugger (`debug.rs:233`); `backend_event` handles only `RunExited`; no file watcher reloads the list.
- `a_line_typed_in_the_run_console_reaches_the_process_and_ctrl_d_ends_its_input`
  - `ipc:run_stdin` → **done**. Proof: "`…proofs3_run::a_line_typed_in_the_run_console_reaches_the_process_and_ctrl_d_ends_its_input`; a line typed in the Run console of `sh -c 'read line; echo "got $line"'` comes back as `got hello`, which only the process reading its stdin can print". The driver calls `ide_host::ops::run::stdin`, as `src-tauri/src/commands/run.rs::run_stdin` does (checked).
  - `ipc:run_close_stdin` → **done**. Proof: "same test; ⌃D in the console of `cat >/dev/null; echo drained` closes its input: the process prints `drained` and exits 0 without being stopped". Driver calls `ops::run::close_stdin`, as `run_close_stdin` does (checked). ⌃D is also `run.debug` in the IntelliJ keymap (`intellij-macos.ts:258`); the console takes it first through `input::claim_keys` (`run/view.rs:220`) while its process takes input, so the keymap should not see it (read, not run).
- `run_on_a_running_services_row_reruns_it_and_stop_ends_it`
  - `action:run` → **done**. Proof: "`…proofs3_run::run_on_a_running_services_row_reruns_it_and_stop_ends_it` and `workbench::tests::a_run_configuration_added_to_services_runs_from_its_row_into_the_detail_pane_and_stop_ends_a_service`; the Services toolbar button: Run on a stopped row starts it, on a running row it reads Rerun and starts it again in a later session (one live process)".
  - `action:stop` → **done**. Proof: "same tests; Stop ends the session, the row reads Stopped, the button reads Run again".
  - Note on the gap text ("the action itself is not run through the registry"): `run` and `stop` are not registry actions in either UI. In the webview they are item actions of the Services toolbar (`src/features/run/services.tsx:576` and `:587`), not passed to `registerActions`; natively there is no `registry.handle("run")` / `("stop")` / `register` of those ids (searched `app-gpui/src`). Their only user path is the toolbar button (or the node's context menu), which the tests click. Precedent in `parity.json`: the sibling toolbar ids `action:run-all`, `action:restart-all`, `action:stop-all` are **done** with proofs that say "the toolbar's Run All, run through run.services.runAll"; those buttons have a registry twin (`services_window.rs:291`), `run` / `stop` have none. Proposed status: **done** on the button click, since no registry id exists to run. If the integrator wants the same shape as the precedent instead, the native-only option is adding `("run.services.run", "run", "Run", "run")` and `("run.services.stop", "stop", "Stop", "stop")` to the list at `services_window.rs:291` (the handler already accepts a `run:config:` key), but the webview has no such ids, so I did not propose it as the fix.
- `add_service_opens_its_menu_and_a_chosen_type_shows_its_configurations`
  - `action:services.addService` → stays **partial**. Proof: "`…proofs3_run::add_service_opens_its_menu_and_a_chosen_type_shows_its_configurations`; run through the registry it shows the Services tool window and opens Add Service; Run Configuration Type › the project's category › Program shows the Program group with its configuration. Not yet: the menu opens near the top of the window, not under the toolbar button (`services_window.rs:282`, a fixed point at `(width/2 - 120, 44)`), and a type with several configurations is added with its preselected configurations where the webview asks with a picker: `add_run_type` (`services_window.rs:841`, read) logs "no configuration picker; the preselected ones show" and answers `AddService::Pick` itself". The picker is not built (no view exists to assert), so no test for that case.

### `workbench::tests::proofs3_windows`

- `open_file_opens_the_picked_file_in_the_project_or_in_the_window_that_has_it`
  - `action:file.openFile` → **done** if it passes. Proof: "`…proofs3_windows::open_file_opens_the_picked_file_in_the_project_or_in_the_window_that_has_it`; run through the registry it asks for files; the picked file opens as a tab of the window's project; from an empty window, a file of the project another window has opens there and that window comes to the front; a file of no open project is edited on its own in the asking window".
  - The assignment asked for a path argument; neither the webview (`src/features/app/windows.ts:472`, `run: async () => openFiles(await pickFiles(...))`) nor the native handler (`workbench/windows.rs`, `file.openFile` ignores `call.arg`) takes one, so the test answers the picker with `simulate_path_prompt_response`, as `projects_open_in_this_window_…` does for Open….
- `open_in_new_window_opens_the_given_or_picked_folder_in_a_new_window`
  - `action:file.openInNewWindow` → **done**. Proof: "`…proofs3_windows::open_in_new_window_opens_the_given_or_picked_folder_in_a_new_window`; with a folder it opens it in a new window although the asking window has a project; a folder another window has brings that window to the front; without one it asks for a folder and opens the picked one in a new window".

### `workbench::tests::proofs3_rename_lsp`

- `renaming_a_file_applies_the_reference_updates_its_language_server_returns`
  - `action:project.rename` → **done**. Proof: "`…proofs3_rename_lsp::renaming_a_file_applies_the_reference_updates_its_language_server_returns` and `workbench::tests::rename_renames_the_file_its_reference_and_its_open_editor`; the fake server watching `*.mock` is asked `workspace/willRenameFiles` with the old and new URIs, its update of another file (which does not name the file as text) is previewed, applied with the rename, the server hears `didRenameFiles`, and Undo Move / Rename takes both back". Native path: `project_files.rs:373` calls `ide_host::ops::refactor::refactor_file_move_edits`.

### `workbench::tests::proofs3_context`

- `the_focused_directory_follows_the_project_views_keyboard`
  - `context:focusedDirectory` → stays **partial**. Proof: "`…proofs3_context::the_focused_directory_follows_the_project_views_keyboard` and `…proofs_editor_search_find::takes_the_focused_directory_of_the_project_view`; read ON (the selected folder while the Project view has the keyboard) and OFF (cleared when the editor takes it), then ON again for another folder. Not yet: the webview publishes it from every `Tree` given `getDirectory` (`src/ui/treeDirectoryContext.ts`, used by `BookmarksView.tsx:241`, `CommitView.tsx:713`, `ProjectView.tsx:804`); natively only the Project view sets it (`workbench/mod.rs:1980`)". See "Behaviours that differ".
- `the_cmake_build_key_follows_the_selected_configuration`
  - `context:run.cmakeBuild` → **done**. Proof: "`…proofs3_context::the_cmake_build_key_follows_the_selected_configuration`; read OFF with a program configuration selected, ON (`app:build`) with the CMake configure step selected (the build of the same directory) and with the build selected".
  - `context:run.updatableRunning` was NOT reached: it needs a live session of a local `tomcat` / `spring-boot` configuration (`run/application_update.rs::is_updatable`); whether such a configuration can launch a plain `sleep` (its type builds its own command line) was not checked.

### `workbench::debug::tests::proofs3_debug`

- `toggle_method_breakpoint_on_a_declaration_sets_and_removes_a_method_breakpoint`
  - `action:debug.toggleMethodBreakpoint` → **done**. Proof: "`workbench::debug::tests::proofs3_debug::toggle_method_breakpoint_on_a_declaration_sets_and_removes_a_method_breakpoint` and `…the_debuggers_other_actions_reach_the_adapter_and_the_editor`; on a Python `def run():` line it sets the method breakpoint `run` listed in View Breakpoints; again it removes it; on a line without a declaration it says so".
- `toggle_field_watchpoint_on_a_java_field_sets_and_removes_a_watchpoint`
  - `action:debug.toggleFieldWatchpoint` → **done**. Proof: "same pattern; on `private int count;` of `class A` in package `a` it sets the watchpoint `a.A#count` listed in View Breakpoints; again it removes it".
- `hot_reload_and_hot_restart_reach_the_adapter_of_a_running_dart_session`
  - `action:debug.hotReload` → **done**. Proof: "`workbench::debug::tests::proofs3_debug::hot_reload_and_hot_restart_reach_the_adapter_of_a_running_dart_session`; unavailable without a Dart session (the registry refuses it); on a running Dart session it reaches the adapter (`hotReload` request), whose answer shows in the debug console".
  - `action:debug.hotRestart` → **done**. Same test, `hotRestart`.
  - Fixture: `open_dart` writes a user `dart.toml` (id `dart`), which replaces the bundled Dart language (`LanguageRegistry::load`: "a user file with the same `id` replaces the built-in one"), so `adapter::find_config(.., "dart")` finds the fake adapter only; its command is `python3`, so `ide-dap`'s Flutter switch (`config.adapter == "dart" && spec.command == "dart"`) does not apply. The launch goes through `flutter_launch` (`debugger/launch.rs:466`), whose program is `<cwd>/lib/main.dart`, where the fixture writes the program.
- `quit_with_a_running_debug_session_asks_to_terminate_it`
  - `action:app.quit` → stays **partial** (the macOS menu key path stays untested). Proof once the fix lands: "`workbench::debug::tests::proofs3_debug::quit_with_a_running_debug_session_asks_to_terminate_it`, `workbench::menu::tests`, `workbench::tests::quit_from_a_window_asks_about_its_unsaved_changes_and_cancel_keeps_it`, `workbench::tests::closing_the_last_window_confirms_the_exit_and_dont_ask_again_turns_it_off`; the exit is confirmed once and each window decides its close in turn; a program running under the debugger is asked about by name: Cancel keeps the window and the session, Terminate (closing the project window) ends the session before the window shows the Welcome screen". **Expected to fail** today: see "Behaviours that differ". The Terminate leg goes through the project window's close (`simulate_close`), which shares `running_process_names` and `terminate` with the quit, so the test needs both parts of the fix.

### `workbench::editor_language::tests::proofs3_editor_keys`

These prove the keys of rows already listed elsewhere (the language actions); each presses the key with the editor focused, on the fixtures the neighbouring tests use:

| test                                                               | key    | action id                       | asserts                                                            |
| ------------------------------------------------------------------ | ------ | ------------------------------- | ------------------------------------------------------------------ |
| `ctrl_i_in_the_editor_implements_the_missing_members`              | ⌃I     | `code.implementMethods`         | the only implement action is applied                               |
| `ctrl_o_in_the_editor_overrides_the_offered_method`                | ⌃O     | `code.overrideMethods`          | the only override action is applied                                |
| `alt_space_in_the_editor_shows_the_quick_definition`               | ⌥Space | `code.quickDefinition`          | the popup with the definition's header and lines; nothing typed    |
| `cmd_y_in_the_editor_shows_the_quick_definition`                   | ⌘Y     | `code.quickDefinition`          | same                                                               |
| `cmd_shift_t_in_the_editor_goes_to_the_test_and_back`              | ⇧⌘T    | `navigate.gotoTest`             | the test opens, the same key goes back                             |
| `shift_f6_in_the_editor_renames_the_element_at_the_caret`          | ⇧F6    | `refactor.rename`               | the Rename dialog, the server's edit applied                       |
| `shift_f2_in_the_editor_goes_to_the_previous_error`                | ⇧F2    | `navigate.prevError`            | wraps to the last error, then the one before, skipping the warning |
| `ctrl_shift_up_in_the_editor_goes_to_the_previous_method`          | ⌃⇧↑    | `navigate.previousMethod`       | caret on the previous method; text unchanged                       |
| `ctrl_alt_up_in_the_editor_goes_to_the_previous_highlighted_usage` | ⌃⌥↑    | `navigate.prevHighlightedUsage` | wraps to the last usage, then the one before                       |

Keys checked in `src/platform/keymaps/intellij-macos.ts` (`Ctrl+I`, `Ctrl+O`, `Alt+Space`, `Meta+Y`, `Meta+Shift+T`, `Shift+F6`, `Shift+F2`, `Ctrl+Shift+Up`, `Ctrl+Alt+Up`). If the rows these belong to (`action:code.implementMethods`, `action:code.overrideMethods`, `action:code.quickDefinition`, `action:navigate.gotoTest`, `action:refactor.rename`, `action:navigate.prevError`, `action:navigate.previousMethod`, `action:navigate.prevHighlightedUsage`) cite key proofs, add these test names to their proof text.

## Tests most likely to fail first

1. `quit_with_a_running_debug_session_asks_to_terminate_it`: expected to fail (times out waiting for "Process 'main' Is Running"): natively the close asks only about Run sessions. Fix below.
2. `ctrl_o_…`, `ctrl_i_…`, `ctrl_shift_up_…`: a text-editing binding of the editor may take ⌃O (open line), ⌃I or ⌃⇧↑ before the IDE keymap. If so, the key's IDE binding loses in the editor; that is a real bug, not a test problem.
3. `hot_reload_and_hot_restart_…`: depends on the user `dart.toml` replacing the bundled one in the backend's registry (read in `ide-language/src/registry.rs`, not run), on `run.debug` accepting a `program` configuration with adapter `dart`, and on adapter `output` events reaching `state.console`.
4. `open_file_…`: the step where an empty window routes a file to the window that has its project (`app_shell::route_files`); the row's gap text says this does not happen, while the code reads as if it does.
5. `renaming_a_file_…`: whether `will_rename_files` matches the fake server's `**/*.mock` filter on the canonical temp path, and whether the preview appears for an LSP-only edit (`RefactoringPreview`).
6. `add_service_…`: the menu row selectors `menu-row-<depth>-<index>` and a click on a submenu row opening it (as `context_menus.rs::choose` does); and the category: it assumes the project's own category is the first non-separator row of the submenu.
7. `run_on_a_running_services_row_…`: whether the rerun reuses the session (same id, later `started_at`) as `run_command` asks with `reuse` (`run_testing/services.rs:721`), and no rerun confirmation (`instances()` in `run/store.rs` excludes the reused session from `others`, so none should be asked).
8. `the_focused_directory_…` / `the_cmake_build_key_…`: they force a redraw (`cx.notify()`) before reading the context, as `a_run_configuration_runs_into_the_run_tool_window…` does; if the keys still lag, the context is published only on some other event.

## Behaviours that differ from the webview (with fixes)

### `app.quit` / window close ignores live debug sessions

Webview: the close asks about `runningProcesses()`, whose `run` provider lists `serviceSessions(useRun.sessions, useDebugRunSessions.serviceSessions).filter(liveServiceSession)` (`src/features/run/index.ts:401`), i.e. run AND debug sessions, and its `stop` stops both. Natively `Workbench::running_process_names` (`workbench/app_shell.rs:148`) and `terminate` (`app_shell.rs:~560`) read only the `RunStore`. Fix, part 1 (names):

```diff
     fn running_process_names(&self, cx: &App) -> Vec<String> {
-        let names: Vec<String> = self
+        let mut names: Vec<String> = self
             .run_driver()
             .map(|driver| {
                 let store = driver.read(cx).store().read(cx);
                 store.sessions.iter().filter(|s| s.is_live()).map(|s| s.config.name.clone()).collect()
             })
             .unwrap_or_default();
+        if let Some(debug) = self.debug_driver() {
+            let state = &debug.read(cx).debugger().read(cx).state;
+            names.extend(state.sessions().values().filter(|s| s.is_live()).map(|s| s.name.clone()));
+        }
         process_names::<std::convert::Infallible>(Ok(names))
     }
```

Part 2 (`terminate`, `app_shell.rs:~560`): also stop the live debug sessions, through the entry the debug tests use (`Workbench::run_debug_action`, `debug.rs:351`). `debug.stop` stops the active session or else one live one, so it is sent once per live session; the wait then covers the debugger too:

```diff
 async fn terminate(this: &This, cx: &mut AsyncWindowContext, names: &[String]) -> anyhow::Result<Result<(), String>> {
     for _ in 0..2 {
+        this.update_in(cx, |workbench, window, cx| {
+            let live = workbench.debug_driver().map_or(0, |debug| {
+                debug.read(cx).debugger().read(cx).state.sessions().values().filter(|s| s.is_live()).count()
+            });
+            for _ in 0..live {
+                workbench.run_debug_action("debug.stop", None, window, cx);
+            }
+        })?;
         let store = this.update(cx, |workbench, cx| {
```

and in the success test of that loop, `stopped(&store, cx)` must also see no live debug session (e.g. poll `debugger().read(cx).state.sessions()` alongside `live_sessions`, with the same `TERMINATE_GRACE` timer). Not compiled: `this.update_in` on `WeakEntity` with `AsyncWindowContext` is assumed to exist as in GPUI.

### `focusedDirectory` comes only from the Project view

Webview: any focused `Tree` with `getDirectory` publishes it (Project, Bookmarks, Commit). Natively only the Project view does (`workbench/mod.rs:1980`). Fix: in `publish_context`, when the focused panel is the Bookmarks or the Commit tool window, take the directory of its selected file node (folder: itself; file: its parent; group: none). The accessors for the selected node of those views were not checked, so no hunk.

### `services.addService` menu position

Webview opens the menu under the toolbar's Add Service button; natively at a fixed point (`services_window.rs:285`). Fix needs the button's bounds from `ServicesView`, not read.

## Rows left out, with why

- `action:docker.refresh`, `action:docker.toggleStoppedContainers`, `action:docker.toggleUntaggedImages`: the Docker daemon.
- `action:project.delete`: the backend sends the file to the system Trash.
- `action:window.toggleFullScreen`: real full screen (the test platform has none).
- `action:debug.reloadChangedClasses`, `action:debug.cancelHotSwap`: `debugCanHotSwap` can be turned on by the fake adapter (it is "any live session with `hot_code_replace`"), but Reload Changed Classes then builds with jdtls (`HotSwapStep::Build` → `BackendCall::JavaBuildWorkspace`) and Cancel Hot Swap Build only acts during that build. Needs the Java language server. Fixture need, if a test is wanted: a test hook answering `java_build_workspace` without jdtls (none exists).
- `ipc:java_resolve_launch`: needs the Java language server.
- `action:find.usages` (the syntax fallback): the native fallback (`code_insight/usage_search.rs`, `SyntaxSearch`) runs when the server's references fail (e.g. "No language server available" in Full mode). Reaching it in a window needs Full mode with a tree-sitter language whose server is absent; whether the backend then tries to install that server (network) was not established. With the fake server it cannot be reached: the `mock` language has no syntax tree, and `FakeServer` cannot answer an error. Fixture need: an `"errors": {method: {code, message}}` key in the fake server script (`editor/intelligence.rs`, before `send({... 'result': result})`: `err = answers.get('errors', {}).get(method)`; `if err: send({'jsonrpc': '2.0', 'id': msg['id'], 'error': err}); continue`) AND the fake server attached to a language with a tree (e.g. a user `javascript.toml` replacing the bundled one with `[[language-servers]]` pointing at the fake script).

## Rows that are really "not built"

- `module:src/workbench/quickSwitch.ts`: `view.quickSwitchScheme` is in the menu spec but no handler is registered (searched `app-gpui/src` for `"view.quickSwitchScheme"` outside `menu_spec.rs`: none), and nothing outside `workbench/quick_switch.rs` uses its model. Missing: the action, opening the Quick Switch popup from the model.
- `unit:src/features/app/__tests__/lightEditClose.test.ts`: `app_models::windows::light_edit_should_close` has no caller. Missing: closing a single-file (LightEdit) window when its last tab closes.

## Rows not reached in this pass

Not looked at beyond the gap text (no test, no verdict): 52 rows.

- `context:run.updatableRunning` (see above).
- `ipc:run_without_derived` was looked at: the driver calls `ops::run::without_derived` whenever a temporary configuration is adopted (`run/driver.rs:229`, `carry_out_persist`), so `run_configuration_runs_the_named_or_given_configuration_…` already passes through it; but an assertion only the backend can produce needs a configuration with a settings-derived field to see stripped from the stored copy, which I did not identify. Counted as not reached.
- `ipc:fs_entry_kind`, `ipc:history_content`, `ipc:history_list_dir`, `ipc:history_restore`, `ipc:menu_refresh` (note: `file.openRecentProject` IS registered now, `workbench/windows.rs`, so the gap's "no handler yet" is stale), `ipc:run_apply_settings`, `ipc:run_gradle_init_script`, `ipc:run_gradle_jvm_args_script`, `ipc:run_resize`, `ipc:set_recent_project_groups`, `ipc:term_kill`, `ipc:vcs_diff_revisions`, `ipc:vcs_log`.
- `module:src/editor/core/copyReference.ts`, `module:src/editor/lsp/workspaceEdit.ts`, `module:src/platform/navigationModifier.ts`, `module:src/workbench/projectColor.ts` (stale gap: `workbench/welcome.rs:9` and `title_bar.rs:549` use `project_color`), `module:src/workbench/toolwindows/ToolWindows.tsx`, `module:src/workbench/toolwindows/content.ts`, `module:src/workbench/toolwindows/layouts.ts` (possibly stale: `layout.save` / `layout.rename` are registered in `app_actions.rs:259`).
- `point.registerStatusItem:code-insight.servers`, `point.registerTitleItem:vcs.mergeRebase`.
- All 28 remaining `unit:` rows: `structural.test.ts`, `Popups.test.tsx`, `AgentReply.test.tsx`, `AgentsView.test.tsx`, `Markdown.test.tsx`, `messageEditing.test.tsx`, `sessionState.test.tsx`, `windows.test.ts` (gap likely stale: `app_shell.rs` now drives `OpenFlow` / `LooseFiles` of `app_models::windows`), `Grid.test.tsx`, `results-view.test.tsx`, `fs.test.ts`, `unityRefresh.test.ts`, `runConfigTypes.test.tsx`, `session.test.tsx`, `behavior.test.ts`, `dialog.test.tsx`, `keymap.test.ts`, `mouseShortcut.test.tsx`, `pages.test.ts`, `branches.test.ts`, `diffScroll.test.ts`, `ignore.test.ts`, `logOptions.test.ts`, `logUiFeatures.test.tsx`, `worktreesPanel.test.tsx`, `previewOnly.test.tsx`, `Tooltip.test.tsx`, `Workbench.test.tsx`.

## Count (of the 81)

- With a test here: 18 rows (`action:run.runConfig`, `action:run.stopProcess`, `action:run.reloadConfigs`, `action:run`, `action:stop`, `action:services.addService`, `action:file.openFile`, `action:file.openInNewWindow`, `action:project.rename`, `action:debug.toggleMethodBreakpoint`, `action:debug.toggleFieldWatchpoint`, `action:debug.hotReload`, `action:debug.hotRestart`, `action:app.quit`, `ipc:run_stdin`, `ipc:run_close_stdin`, `context:focusedDirectory`, `context:run.cmakeBuild`): 15 would become done, 3 stay partial (`services.addService`, `app.quit`, `focusedDirectory`).
- Left out: 9. Really "not built": 2. Not reached: 52.
