# Parity triage: batches

Source: `rows.json` (910 rows). That is 482 partial, 122 model only and 306 blank IPC rows, matching INVENTORY.md as regenerated at 07:41. Three partial rows were added during the triage. "high" means the native code was read. `test-only` requires a file:line in reached, non-test code.

## (a) Counts

| Class               | Rows |
| ------------------- | ---- |
| small               | 279  |
| feature             | 238  |
| test-only           | 56   |
| blocked             | 16   |
| record-only         | 15   |
| IPC not-called      | 238  |
| IPC called-natively | 36   |
| IPC tauri-only      | 32   |

| Area                                         | test-only | small | feature | blocked | record-only | called | not-called | tauri-only |
| -------------------------------------------- | --------- | ----- | ------- | ------- | ----------- | ------ | ---------- | ---------- |
| editor                                       | 12        | 57    | 77      | 0       | 1           | 1      | 26         | 0          |
| app shell                                    | 10        | 45    | 21      | 11      | 4           | 1      | 22         | 32         |
| vcs                                          | 2         | 41    | 23      | 0       | 0           | 1      | 18         | 0          |
| run                                          | 7         | 12    | 29      | 2       | 0           | 9      | 49         | 0          |
| plugins                                      | 13        | 18    | 10      | 0       | 1           | 2      | 32         | 0          |
| settings                                     | 1         | 19    | 13      | 2       | 7           | 1      | 17         | 0          |
| search                                       | 2         | 17    | 19      | 0       | 0           | 0      | 6          | 0          |
| project view                                 | 3         | 27    | 3       | 0       | 0           | 1      | 4          | 0          |
| agents                                       | 1         | 6     | 12      | 0       | 0           | 4      | 14         | 0          |
| database                                     | 1         | 10    | 2       | 1       | 0           | 6      | 0          | 0          |
| debugger                                     | 2         | 10    | 0       | 0       | 1           | 5      | 4          | 0          |
| github                                       | 2         | 4     | 7       | 0       | 0           | 5      | 2          | 0          |
| terminal                                     | 0         | 13    | 0       | 0       | 1           | 0      | 1          | 0          |
| remote / deployment / http client / notebook | 0         | 0     | 22      | 0       | 0           | 0      | 43         | 0          |

## (b) Ten densest test-only batches

IPC `called-natively` rows are included because only a test is missing for them as well. The batches are ordered by size, and each is labelled with its confidence.

1. **app shell, 8 rows (high, spread over 8 files).** Keys:
   - `unit:src/platform/actions.test.ts` and `module:src/platform/actions.ts` (`platform/actions.rs:358`)
   - `context:popupOpen` (`workbench/mod.rs:2123`)
   - `unit:src/workbench/toolwindows/store.test.ts` (`mod.rs:669`)
   - `unit:src/workbench/toolwindows/layouts.test.ts` (`tool_windows.rs:1069`)
   - `module:src/workbench/quickSwitch.ts` (`zoom_actions.rs:44`)
   - `module:src/workbench/tasks.ts` (`plugin_runtime.rs:800`)
   - `module:src/workbench/TitleBar.tsx` (`title_bar.rs:542`)

   Extend `workbench::tests` and `platform::actions::tests`.

2. **plugins, 7 rows (low: the code is in a crate).** Keys: `wit:wit/host.wit#output`, `#storage`, `wit:wit/io.wit#process`, `#secrets`, `wit:wit/workspace.wit#editor`, `#vcs`, `#workspace`. The host functions are in `crates/ide-plugin/src/wit_host.rs`. A fixture plugin calls each one and asserts the effect. Extend `workbench::plugin_runtime::tests::proofs10_host_api`.
3. **github, 7 rows (2 high, 5 low).**
   - `unit:…/github/create/create.test.ts` and `RemoteBranchDialog.test.tsx` (high, `workbench/github_part2.rs:19,247`).
   - `ipc:gh_pr_comment`, `gh_pr_list`, `gh_pr_review`, `gh_pr_review_comments`, `gh_pr_view` (low). They reach GitHub through `gh_api` (`workbench/github.rs:336`) and need a fake gh.

   Extend `workbench::github::part2::tests`.

4. **debugger, 6 rows (high).** Keys: `event:Debug`, `ipc:debug_attachable_processes`, `ipc:debug_redefine_classes`, `ipc:debug_send_input`, `ipc:java_build_workspace`, `ipc:java_debug_server`. Code is in `debugger/driver.rs` (lines 330, 736, 740, 796, 838, 860). Use the fake adapter. Extend `workbench::debug::tests::proofs6_debug`.
5. **database, 6 rows (high).** Keys: `ipc:db_connect`, `db_databases`, `db_schema_objects`, `db_set_password`, `db_set_ssh_secret`, `db_trust_ssh_host`. The DbOp dispatcher is at `workbench/database.rs:173-192`. Use a SQLite source. Extend `workbench::tests::proofs6_database`.
6. **run, 6 rows (high).** Keys: `action:run.debugContext`, `context:run.updatableRunning`, `point.registerTitleItem:run.widget`, `point.registerTitleItem:build.project`, `ipc:sdk_python_effective`, `ipc:sdk_locate_tools`. Code is in `workbench/run_context.rs:206,222,570`, `run_configs.rs:162`, `run/widget_view.rs:263` and `chrome.rs:373`. Extend `workbench::run_context::tests`.
7. **project view, 6 rows (4 high, 2 low).** Keys: `action:project.compareWith`, `feature:project-tree`, `point.registerUndoHandler:(at) src/features/project-tree/index.ts:209` (`project_view_actions.rs:425,484`), `ipc:workspace_attach` (`app_shell.rs:913`), and `projectWindow.ts` / `projectWindow.test.ts` (low, `app_models/windows.rs:302`). Extend `workbench::project_view_actions::tests`.
8. **editor, 5 rows (high).** Keys: `action:code.generate`, `action:code.overrideMethods`, `action:code.applyQuickFix`, `module:src/editor/lsp/workspaceEdit.ts`, `unit:src/platform/keymaps/newElement.test.ts`. The path is `apply_action` → `reformat_generated` (`workbench/editor_language.rs:518`). Extend `workbench::editor_language::tests::proofs15_implement`.
9. **agents, 5 rows (high).** Keys: `ipc:agent_send_queued_now`, `ipc:agent_stop_background`, `ipc:agent_unqueue` (`workbench/agents.rs:244-248`), `ipc:mcp_report_problems` (`code_insight.rs:592`), `point.registerMenuGroup:agents.send`. Use the fake agent. Extend `workbench::tests::proofs_agents`.
10. **build tools, 4 rows (high).** Keys: `ipc:build_model`, `ipc:build_sync`, `ipc:gradle_link_projects`, `ipc:gradle_unlink_projects` (`workbench/build_tools.rs:406,647,828,909`). This module was being declared during the triage, so use a new `workbench::build_tools::tests`.

Smaller batches:

- 3 Docker settings and snapshot rows (`workbench/services_window.rs:728-736`), extend `workbench::services_actions::tests::proofs11_docker`.
- 2 editor-view rows (camel humps, doc comment), extend `editor::view::editing_tests`.
- 2 console find rows (`run/view.rs:117`).

## (c) Ten densest small batches

1. **app shell, 11 rows, `workbench/mod.rs`.**
   - Recent Files / Switcher never get the `edited` list: `navigate.recentFiles`, `navigate.switcher`, `search.recent` overlay.
   - Recent Projects popup in a project window: `file.openRecent`, `welcome.recentProjects`.
   - Unsplit should ask once: `editor.unsplit`, `editor.unsplitAll`.
   - Stripe badges for Build, Notifications and Problems: 3 tool-window rows.
   - Register `toolwindow.closeContent`.
2. **editor, 14 rows, `editor/view.rs` paint.**
   - Wire unreached models into the paint: indent guides, selection marks, doc-comment tags, colour-scheme overrides (2 rows), right margin, smooth caret, string clipboard, conflict markers (2 rows).
   - Middle-button column drag.
3. **vcs, 14 rows, `vcs/log_view.rs` and `workbench/log_window.rs`.**
   - Multi-selection: `vcs.log.squashCommits`, `ipc:vcs_commits_changes`, `registerToolWindow:git`.
   - Draw `collapse_linear`: `collapseLinear`, `expandLinear`, `logGraphCorrectness`.
   - Handle the filter-bar events the window drops (`log_window.rs:291`): `ipc:vcs_log`, `logPanes`, `logFilters`.
   - Wire the `vcs_more::log_*` and `project_switch` models: 5 rows.
4. **app shell, 11 rows, `workbench/app_actions.rs`.**
   - Register `help.showLog` and the About logo: 2 rows.
   - Titled list dialog for `layout.restore`, `layout.rename` and `layout.delete`.
   - Untrusted attached member in the Restricted Mode banner: 3 rows.
   - Wire `app_models::jetbrains_settings` (2 rows) and `cli_command` (1 row).
5. **terminal, 13 rows.** Code is in `terminal/panel.rs` and `terminal/view.rs`.
   - Per-group tab strip and draggable divider: 5 split rows plus `feature:terminal`.
   - Route the tab ✕ through `terminal.closeSession`.
   - Wire the `terminal::{drop,paste,fit,ime_commit,session,windows_pty}` models into the view: 6 rows.
6. **plugins, 7 rows, `workbench/plugin_contributions.rs`.** Wire `features_b::color_preview` (2 rows), the plugin marker store, navigation and indexed markers (3 rows), plugin visuals (1 row) and the contributed toolbar icon (1 row).
7. **project view, 6 rows, `project_tree.rs`.** Cmd+Z reaches `edit.undo`, the folder-kind and icon-theme icons (2 rows), the `file_presentation` batching (2 rows) and `module_groups`.
8. **editor / project view, 6 rows, `workbench/project_files.rs`.** Pass project roots to `copy_path_items` (2 rows), retarget image and binary tabs on rename (2 rows), and New > From Template from `templates::files_*` (2 rows).
9. **settings, 5 rows, `settings_model/schema.rs`.** Normalise wrong-typed values instead of refusing them: agents, behavior, cmake, goBuildTags, languages.
10. **run, 4 rows, `workbench/build_window.rs`.** Let the build actions be awaited: `build.buildProject`, `buildModule`, `rebuildProject`, `buildArtifacts`.

Next in size:

- Paste from History grammar and font (4 rows, `editor/paste_history_view.rs:297`)
- Database DDL/import password retry (4 rows, `workbench/database_extras.rs`)
- Menu bridge (4 rows, `workbench/menu.rs`)
- Keymap page (4 rows, `settings_view/keymap.rs`)
- Debug sessions in `run.stop` and the run widget (2 rows, `workbench/mod.rs`)

## (d) Features ranked by rows unlocked

The counts include feature rows and IPC not-called rows.

| #   | Feature                                                                                                                                                                     | Rows   |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------ |
| 1   | deployment tool window                                                                                                                                                      | 22     |
| 2   | remote development (+5 attach and remote project windows)                                                                                                                   | 18     |
| 3   | document syntax tree in the editor                                                                                                                                          | 17     |
| 4   | docker services (needs a daemon: test against a fake endpoint)                                                                                                              | 17     |
| 5   | notebook editor                                                                                                                                                             | 14     |
| 6   | http client view                                                                                                                                                            | 13     |
| 7   | settings custom pages                                                                                                                                                       | 10     |
| 8   | highlighted code preview                                                                                                                                                    | 10     |
| 9   | plugin web views                                                                                                                                                            | 9      |
| 10  | test runner tree                                                                                                                                                            | 9      |
| 11  | agent conversation editor tab                                                                                                                                               | 8      |
| 12  | diff editor tab                                                                                                                                                             | 8      |
| 13  | search everywhere extras                                                                                                                                                    | 8      |
| 14  | mcp server                                                                                                                                                                  | 8      |
| 15  | editor model wiring (`editor_model::*` replacing `editor/tabs.rs`)                                                                                                          | 8      |
| 16  | agents settings and registry                                                                                                                                                | 8      |
| 17  | git log extras                                                                                                                                                              | 7      |
| 18  | plugin-contributed formatter                                                                                                                                                | 6      |
| 19  | jdk download / tomcat run configurations / virtual documents / semantic token colouring / changes between dialog / edit configurations extras / plugin navigation providers | 6 each |
| 20  | in-place rename / multi-repository vcs operations / agents new task page / auto updater                                                                                     | 5 each |

Sixteen IPC rows have no webview caller at all. They are dead commands and unlock nothing.

## (e) Already proven, not recorded

I read each test body.

- `action:run.run` → `workbench::run_context::tests::run_with_current_file_selected_runs_the_file_and_keeps_current_file_selected`. `run.run` with Current File selected runs the file as a temporary configuration and keeps Current File selected. The proof says this is "not built", which is stale.
- `unit:src/features/database/results-view.test.tsx` → `database::store::tests::records_the_selected_schema_or_connection_default`. This is the one missing case, asserted for an explicit schema and for the connection default.
- `unit:src/editor/core/cm/rainbowBrackets.test.ts` → `editor::extras::tests::strings_and_comments_are_skipped_lexically`. That test is already cited in the row's own proof, but the proof still lists the lexical skip as missing.
