# Tests-only slice report: Part 1 done, Part 2 partly done, all gates green

Part 1 is finished for all thirteen files. Part 2 has five rows proven by new or changed tests. I found and fixed one app defect: Select In › Structure could never appear. I stopped Part 2 near the two-thirds budget mark, as the brief asks. Nothing is committed.

## 1. Gates (measured; logs in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dtests/`)

| Gate                                        | Log            | Result                                                                                                                                                            |
| ------------------------------------------- | -------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Whole suite, run 1, nothing beside it       | `suite1.log`   | `test result: ok. 8269 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 216.55s`. Uptime before: load 3.99 3.71 4.75; after: 17.48 10.69 7.44 |
| Whole suite, run 2                          | `suite2.log`   | `test result: ok. 8269 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 228.42s`. Uptime before: 15.10 10.40 7.37; after: 86.22 27.42 14.07   |
| My 8 tests, 20 times, during run 2          | `repeat20.log` | 20 of 20 runs gave `test result: ok. 8 passed; 0 failed`. No test failed even once                                                                                |
| `cargo test --test web_view`                | `web_view.log` | `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.58s`                                                                   |
| `cargo clippy --all-targets -- -D warnings` | `clippy.log`   | exit 0, no warnings                                                                                                                                               |
| `cargo build`                               | `build.log`    | exit 0, 0 warnings                                                                                                                                                |

- **Test count:** it went from 8262 at HEAD to 8269. That is my 7 new tests (2 kept highlight tests, unsubscribe, New › HTTP Request, Evaluate menu, Services Debug, plugin run target). The 8th test in the repeats is the existing Select In test, which I changed.
- **How the 20 repeats ran:** script `$S/dtests/repeat20.sh`, running the test binary `target/debug/deps/openidea_gpui-dce820e51898f091` directly so it would not contend with cargo's lock.
- **Unsubscribe test can fail:** I removed its `off` call temporarily. It then failed with `an unsubscribed plugin hears the save and not the opening, left: 70, right: 64` (log `unsubscribe_mutant.log`). I put the line back afterwards. That check came after the gates, so `target/` now holds a build of the mutant and the next cargo run recompiles the crate. The source is the same as what passed the gates.
- **Formatting:** clippy and build ran after formatting. Formatting changed only `workbench/mod.rs` (the order of `mod` lines), so suite 1 recompiled first.
- **Processes:** no process of mine is left. I checked by command pattern, not by PID. All runs went through the harness's background mode, never with `&`.
- **Disclosures:**
  - The highlight run's full output is not in a file: it came back in the tool result, and I quote it below.
  - `part1_workbench.log` is cut short by a `tail -80`. `part1_attach_annotate.log` has the attach and annotate failures in full.
  - Read-only git, `grep` and polling loops ran beside suite 1. No cargo command did.

## 2. Part 1: the thirteen files

1. **`editor/view/editing_tests/proofs23_middle_column.rs`, `proofs_editor_clipboard_history.rs`, `proofs_editor_multi_caret.rs`:** these were already declared, at `editor/view.rs:3147-3149` inside `mod editing_tests`, so the census was wrong about them. All 10 tests ran by filter: `test result: ok. 10 passed`. No change.

2. **`editor/view/tests/proofs_editor_highlight_cmake_queries.rs`:** all 6 tests fail. Parked as `proofs_editor_highlight_cmake_queries_pending.rs`, undeclared, a copy.
   - Example failure: `expected: "VERSION" #0: expected Some("syn-keyword"), painted [Some("syn-constant"), …]`.
   - Another: `expected: "3.20" #0: expected Some("syn-number"), painted [None, None, None, None]`.
   - Another: `toolchain.cmake.in` is detected as `LanguageId("plaintext")` where `"cmake"` is expected.
   - Cause: the native CMake colours come from the query files in `crates/ide-language`, which I may not edit. The webview recognises `.cmake.in` inside its own editor (`src/editor/core/cm/languages.ts:81-82`); the native app relies on `languages/cmake.toml`, which the Tauri backend shares.

3. **`editor/view/tests/proofs_editor_highlight_interpolation_templates.rs`:** now declared with its 2 passing tests: `keeps_nested_braces_strings_and_comments_inside_expressions` and `handles_kotlin_multiline_raw_templates_and_nested_templates`.
   - The 2 failing tests moved, with their helpers, to the new undeclared file `proofs_editor_highlight_interpolation_templates_pending.rs`.
   - `separates_delimiters_names_operators_and_numbers` fails with `Kotlin: "$«i» +" #0: expected Some("syn-variable"), painted [Some("syn-string")]`, and the same for Scala and Dart.
   - `preserves_literal_dollars_and_braces` fails because Kotlin `"\$name \${value}"` is painted `syn-escape` inside the string.
   - Cause: the template queries live in `crates/ide-language`.

4. **`editor/view/tests/proofs_editor_highlight_shell.rs`:** parked whole as `proofs_editor_highlight_shell_pending.rs`.
   - 3 of 4 tests fail. Example: `expected: "«$»name" #0: expected Some("syn-decorator"), painted [Some("syn-punct")]`.
   - The one that passes (`leaves_single_quotes_escaped_dollars_comments_and_heredocs_intact`) only asserts that `syn-decorator` appears nowhere. The three failures show nothing in shell is ever painted `syn-decorator`, so it passes without proving anything.

5. **`workbench/app_actions/tests/proofs6_app_actions.rs`:** not declared and not run.
   - `ide_vcs::init` runs `git init -q` with the process environment. I read `crates/ide-vcs/src/cli.rs:252-278`: it sets no `HOME` or `GIT_CONFIG_*`. There is no per-call seam, and setting the environment for the whole process would need `unsafe` in Rust 2024.
   - Hypothesis: it reads the user's `~/.gitconfig` (`init.templateDir`, `init.defaultBranch`), which the safety rules forbid. To confirm: `GIT_TRACE=1 git init -q /tmp/x` shows which config files are read.
   - The file stays in place, undeclared. Row `ipc:vcs_init` stays open.

6. **`workbench/plugin_runtime/tests/proofs10_locale.rs`:** fails because of an app defect, so it is parked.
   - The original asks the api14 fixture for a `locale` command, which the fixture has neither in its source nor in its wasm. `grep -ac locale` on `dist/plugin.wasm` gives 0. `git log -S'"locale" =>'` finds nothing in history.
   - So I wrote `proofs10_locale_pending.rs` to assert the host's `ui_locale()`, which is what plugin code's `locale()` returns (`crates/ide-host/src/plugins/api.rs:2919-2921`). It fails: `the plugin host is told the chosen language did not happen within thirty seconds`.
   - Defect: the native app tells the plugin host the language only once, at `plugins.rs:178`. Nothing outside tests ever changes the UI language: there is no `set_locale` call and `language_tag` is never read. The webview re-sends the language after every switch, once its own catalogue has switched (`src/features/app/locale.ts:13-21`).
   - This is not a few lines, because the native UI does not follow `appearance.locale` at all. Reported, not fixed.
   - I left the original file unchanged. Deleting it loses the version that asserts through plugin code.

7. **`workbench/plugin_runtime/tests/proofs10_unsubscribe.rs`:** declared and passing.
   - I replaced the fixed two-second wait with a barrier. After `off` and the reopen, the test saves the file, which the plugin still watches, waits until the save is counted, and then asserts the count rose by exactly 1.
   - As described above, the test fails when the unsubscribe is removed.
   - Hypothesis: the window posts events in order, so the opening can never be counted after the save. Its posting queue is in `plugin_runtime.rs:1459-1471`.

8. **`workbench/plugins_window/tests/proofs10_lifecycle.rs`:** parked as `proofs10_lifecycle_pending.rs`.
   - Failure at line 45: `left: None, right: Some("yes")`.
   - Cause: the api14 fixture has no `deactivated` command (`grep -ac` on the wasm gives 0) and `crates/` is off-limits. This is a missing fixture command, not a measured app defect. Whether the app runs `deactivate` is unmeasured.

9. **`workbench/tests/proofs6_attach.rs`:** parked as `proofs6_attach_pending.rs`.
   - Failure: `the folder is attached to the workspace did not happen within thirty seconds`.
   - Defect: `app_shell.rs:849` removes the Attach button, and `app_models/windows.rs:289-290` acts only on "new" and "same". The webview attaches in place (`src/features/app/windows.ts:177-180`, `:186-205`).
   - Hypothesis: not a few lines, because the Project view would have to show attached folders (per `notes/integration/proofs20_b.md`). Reported, not fixed.

10. **`workbench/tests/proofs_merge_annotate.rs`:** parked as `proofs_merge_annotate_pending.rs`.
    - Failure at line 16: `Yours' pane can be annotated`.
    - Defect: `vcs/merge_view.rs:832` always disables the command and `:902` does nothing. The webview enables it outside the result pane (`src/features/vcs/MergeEditor.tsx:429-430`) and shows the blame (`:490-500`).
    - Hypothesis: not a few lines, because it needs blame painted in the merge panes.

11. **`workbench/tests/proofs_quit.rs`:** a duplicate of `workbench::tests::quit_from_a_window_asks_about_its_unsaved_changes_and_cancel_keeps_it` (`workbench/mod.rs:5255`). That test runs Quit, Exit, then Cancel on Unsaved Changes, twice, and passed in suite 1. Listed for deletion.

### Defect found and fixed: Select In › Structure was unreachable

- **Cause:** the ⌥F1 chooser in `workbench/project_view_actions.rs` `select_in` built its menu with an empty set of contributed groups (`with_menu_context` → `Contributed::default()`). So `search.structure.selectIn` (`context_menus.rs:307`) never reached it.
- **Webview:** `src/features/project-tree/actions.ts:122` adds the `selectIn` menu groups, and `src/features/search/index.ts:202-213` adds Structure for the edited file.
- **Fix:** the chooser now passes the contributed groups for its path. `with_contributed_menu_context` in `project_files.rs` became `pub(super)`, and the now-unused `with_menu_context` was removed.
- **Side effect:** the fix also computes the Project view, Open In and New groups and then drops them. Hypothesis: computing them has no side effect.
- **Test changed to match the webview:** `select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it` expected `["Select In", "> Project View", Finder, "Terminal"]`. The webview's list is `["Select In", "> Project View", "Structure", "-", Finder, "Terminal"]`, with the separator from `src/workbench/contributions.ts:1369`. The test now also chooses Structure and asserts the Structure tool window shows.
- **Possible difference:** the native `toolwindow.structure` calls `layout.activate(id)` (`workbench/mod.rs:720`), while the webview always shows the window (`search/index.ts:210`). Hypothesis: native may hide an already-active Structure window instead. To confirm: run Select In › Structure twice in a test.

## 3. Part 2: one line per row of `code_no_test.md` (the file has 23 rows, not 22)

| Row                                                 | Test or reason                                                                                                                                                                                                                                                                                        | State                                                                                                                                                                                                                               |
| --------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `point.registerMenuGroup:search.structure.selectIn` | `workbench::project_view_actions::tests::select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it`: ⌥F1 lists Structure, and choosing it shows the Structure tool window (after the fix)                                                                                         | done                                                                                                                                                                                                                                |
| `point.registerMenuGroup:http.new`                  | `workbench::project_view_actions::tests::menu_groups::new_http_request_in_a_folders_project_view_menu_creates_the_request_file_there`: right-click a folder, New › HTTP Request, type a name, and `api/requests.http` is created with the example request                                             | done                                                                                                                                                                                                                                |
| `point.registerMenuGroup:debugger.evaluate`         | `workbench::run_context::tests::editor_menu_evaluate::the_editors_menu_offers_evaluate_expression_while_the_session_is_paused`: right-click in the editor; the menu lacks it before a session, offers it while paused and opens the Evaluate dialog, and lacks it after Stop                          | done                                                                                                                                                                                                                                |
| `action:debug` (Services toolbar)                   | `workbench::run_context::tests::services_debug::debug_on_the_services_toolbar_debugs_the_selected_configuration`: click the configuration's row, then Debug; the session stops at the breakpoint (`main.fake:2`)                                                                                      | partial: `RerunDebugger` only logs (`services_window.rs:677`)                                                                                                                                                                       |
| `ipc:plugin_run_targets`                            | `workbench::tests::run_targets_plugins::run_targets_lists_the_target_a_plugin_registered_for_the_project`: the `execution` fixture registers a target, and Settings › Run Targets lists `Execution target \| Plugin execution tests · Read-only`. The program is a stand-in script in the test's home | done or partial, your call: native calls `plugins.run_targets(&root)` directly (`sources.rs:126`); the adapter calls `ide_host::plugins::run_targets::list` (`src-tauri/src/commands/plugin.rs:1075`), whose body is that same call |
| `ipc:plugin_set_ui_locale`                          | `proofs10_locale_pending.rs` (fails, defect above)                                                                                                                                                                                                                                                    | open                                                                                                                                                                                                                                |
| `ipc:workspace_attach`                              | `proofs6_attach_pending.rs` (fails, defect above)                                                                                                                                                                                                                                                     | open                                                                                                                                                                                                                                |
| `ipc:debug_attachable_processes`                    | not written: the call runs the machine's `ps` and opens every user process's executable (`crates/ide-dap/src/attach.rs:203-228`, `is_go_binary :165-173`), including ones under the home                                                                                                              | open                                                                                                                                                                                                                                |
| `ipc:lsp_goto_implementation`                       | not written: native uses `ops::lsp::lsp_goto(.., GotoKind::Implementation)` (`editor/intelligence.rs:829`), not `lsp_goto_implementation` with its partial results (`src-tauri/src/commands/lsp.rs:97`; webview `src/editor/lsp/navigation.ts:281`). A test cannot close the row                      | open, behaviour differs                                                                                                                                                                                                             |
| `ipc:vcs_log_details_batch`                         | not written: native calls only `git::vcs_log_details` (`log_window.rs:411`, `log_actions.rs:496`, `:816`), never the batch                                                                                                                                                                            | open, behaviour differs                                                                                                                                                                                                             |
| `ipc:doc_ui_mode`                                   | not written: native uses the mode only for the structure cache (`editor/structure/commands.rs:311-318`, `:334`). Hypothesis: the editor's Angular mode on open (`src/editor/core/backend.ts:480`) has no native counterpart                                                                           | not started                                                                                                                                                                                                                         |
| `ipc:doc_open_virtual`                              | not written: both paths need a language server that serves virtual documents (`jdt://`)                                                                                                                                                                                                               | not started                                                                                                                                                                                                                         |
| `ipc:db_databases`                                  | not written. Hypothesis: SQLite sources never ask for databases (`lists_databases`, `database/tree_view.rs:727`); a server is needed                                                                                                                                                                  | open                                                                                                                                                                                                                                |
| `ipc:db_schema_objects`                             | not written. Hypothesis: SQLite has no routines or sequences, so a test would assert an empty list                                                                                                                                                                                                    | open                                                                                                                                                                                                                                |
| `ipc:db_set_ssh_secret`                             | not written. Hypothesis: needs an SSH server                                                                                                                                                                                                                                                          | open                                                                                                                                                                                                                                |
| `ipc:db_trust_ssh_host`                             | not written: it writes `~/.ssh/known_hosts` (`crates/ide-host/src/ops/db.rs:238-243`). Hypothesis: it also needs an SSH server                                                                                                                                                                        | open                                                                                                                                                                                                                                |
| `ipc:docker_snapshot`                               | not written: needs Docker                                                                                                                                                                                                                                                                             | open                                                                                                                                                                                                                                |
| `action:refresh` (Docker node)                      | not started                                                                                                                                                                                                                                                                                           | open                                                                                                                                                                                                                                |
| `ipc:doc_snapshot`                                  | not started (census statement not re-checked)                                                                                                                                                                                                                                                         | open                                                                                                                                                                                                                                |
| `event:LspRefresh`                                  | not started                                                                                                                                                                                                                                                                                           | open                                                                                                                                                                                                                                |
| `event:ProjectRootsChanged`                         | not written: only the build tool window listens (`build_tools.rs:386`), and showing a reload needs a build model; the webview's other listeners have no native counterpart                                                                                                                            | open                                                                                                                                                                                                                                |
| `ipc:syntax_expression_at`                          | not started                                                                                                                                                                                                                                                                                           | open                                                                                                                                                                                                                                |
| `event:UpdateStatusChanged`                         | a feature gap (the page is listed only when `app.checkForUpdates` is registered, `settings_window.rs:121`)                                                                                                                                                                                            | open                                                                                                                                                                                                                                |

Other rows moved by Part 1: `ipc:plugin_subscribe`. Its note said file-opened and unsubscribe had a test that was not compiled; `workbench::plugin_runtime::tests::proofs10_unsubscribe::a_file_the_window_opens_reaches_the_plugin_subscribed_to_file_opened` now runs. The "Missing" note in `parity.json` can drop that part; input-mode-changed stays. The two interpolation tests add cases to `unit:src/editor/core/cm/interpolation.test.ts` (now 4 of 8 cases), so it stays partial.

Features listed and not built, as the brief asks: nine status items built but never put in the status bar, run types nothing registers, and Docker operations shown but not carried out.

## 4. Files

**Edited:**

- `app-gpui/src/editor/view.rs`
- `app-gpui/src/editor/view/tests/proofs_editor_highlight_interpolation_templates.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/plugin_runtime.rs`
- `app-gpui/src/workbench/plugin_runtime/tests/proofs10_unsubscribe.rs`
- `app-gpui/src/workbench/project_files.rs`
- `app-gpui/src/workbench/project_view_actions.rs`
- `app-gpui/src/workbench/run_context.rs`
- `app-gpui/src/workbench/tests/proofs30_settings_run_targets.rs` (only `open_run_targets` made `pub(super)`)

**New, declared:**

- `app-gpui/src/workbench/project_view_actions/tests/menu_groups.rs`
- `app-gpui/src/workbench/run_context/tests/editor_menu_evaluate.rs`
- `app-gpui/src/workbench/run_context/tests/services_debug.rs`
- `app-gpui/src/workbench/tests/run_targets_plugins.rs`

**New, undeclared:**

- `editor/view/tests/proofs_editor_highlight_cmake_queries_pending.rs`
- `editor/view/tests/proofs_editor_highlight_interpolation_templates_pending.rs`
- `editor/view/tests/proofs_editor_highlight_shell_pending.rs`
- `workbench/plugin_runtime/tests/proofs10_locale_pending.rs`
- `workbench/plugins_window/tests/proofs10_lifecycle_pending.rs`
- `workbench/tests/proofs6_attach_pending.rs`
- `workbench/tests/proofs_merge_annotate_pending.rs`

All are under `app-gpui/src/`. The two whole-file pending copies were made with `cp`. `git status` shows nothing outside `app-gpui/`.

**For you to delete** (originals of the pending copies, plus the duplicate), all under `app-gpui/src/`:

- `editor/view/tests/proofs_editor_highlight_cmake_queries.rs`
- `editor/view/tests/proofs_editor_highlight_shell.rs`
- `workbench/plugin_runtime/tests/proofs10_locale.rs` (its plugin-code version is lost if deleted)
- `workbench/plugins_window/tests/proofs10_lifecycle.rs`
- `workbench/tests/proofs6_attach.rs`
- `workbench/tests/proofs_merge_annotate.rs`
- `workbench/tests/proofs_quit.rs` (the duplicate)

## 5. Stale or false facts in notes and briefs

- HEAD is `f5fa08c8c`, a notes commit on top of `2ad1903b4`.
- Three of the "thirteen undeclared" files were already declared.
- `notes/integration/proofs10_plugins.md:114` names `deactivated` and `locale` arms in the api14 fixture and a `proofs10_host_api::a_plugin_reads_the_ui_language_the_settings_choose` test. Neither the arms nor the test exist.
- Census row `ipc:plugin_set_ui_locale` presents the locale test as the proof "if wired". It could not pass even when declared.
