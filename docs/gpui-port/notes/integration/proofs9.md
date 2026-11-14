# proofs9: backend operations without a proving test (A) and test-only gap groups (B)

Nothing was compiled or run (no cargo). What is said about runtime behaviour is a hypothesis until the integrator runs the tests. What is said about code was read in this session. For each `ipc:` row with a test, I read the adapter in `src-tauri/src/commands/`, and the native path calls the same backend function: `lsp_resolve_completion` → `ide_host::ops::lsp::lsp_resolve_completion`; `cancel_task` → `ide_core::task::cancel`; `gh_pr_prepare_diff` → `ide_github::prepare_diff` with the same arguments; `plugin_assets` → `PluginHost::assets`; `plugin_status_items` → `PluginHost::status_snapshot`; `syntax_identifiers(_in_files)` → `ide_host::ops::syntax::*`; `vcs_log_users` → `ide_host::ops::vcs::vcs_log_users`.

## Files written (all new)

| file                                                                      | tests                                         | parent `mod tests`                                |
| ------------------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------- |
| `app-gpui/src/editor/intelligence/tests/proofs9_resolve.rs`               | 1                                             | `editor::intelligence::tests` (inline)            |
| `app-gpui/src/editor/intelligence/tests/proofs9_syntax_usages.rs`         | 2                                             | `editor::intelligence::tests`                     |
| `app-gpui/src/editor/intelligence/tests/proofs9_cancel_task.rs`           | 1                                             | `editor::intelligence::tests`                     |
| `app-gpui/src/workbench/github/tests/proofs9_github.rs`                   | 1                                             | `workbench::github::tests` (inline)               |
| `app-gpui/src/workbench/plugin_tool_windows/tests/proofs9_view_icon.rs`   | 1                                             | `workbench::plugin_tool_windows::tests` (inline)  |
| `app-gpui/src/workbench/plugin_runtime/tests/proofs9_status_snapshot.rs`  | 1                                             | `workbench::plugin_runtime::tests` (inline)       |
| `app-gpui/src/workbench/plugins_window/tests/proofs9_settings_plugins.rs` | 1                                             | `workbench::plugins_window::tests` (inline)       |
| `app-gpui/src/workbench/tests/proofs9_run_chooser.rs`                     | 2                                             | `workbench::tests` (inline in `workbench/mod.rs`) |
| `app-gpui/src/workbench/tests/proofs9_log_users.rs`                       | 1, EXPECTED TO FAIL until the fix below lands | `workbench::tests`                                |

Total: 9 files, 11 tests.

## mod lines to add

1. `app-gpui/src/editor/intelligence.rs`, inside `mod tests {`, after the anchor `    mod proofs_editor_navigation_modifier;`:
   ```rust
       mod proofs9_cancel_task;
       mod proofs9_resolve;
       mod proofs9_syntax_usages;
   ```
2. `app-gpui/src/workbench/github.rs`, inside `mod tests {`, after the anchor `    mod proofs7_github_urls;`: `    mod proofs9_github;`
3. `app-gpui/src/workbench/plugin_tool_windows.rs`, inside `mod tests {`, before the anchor `    const LABEL: &str = "w1";` (the one followed by `    const GREETINGS: &str = "plugin.hello.greetings";`): `    mod proofs9_view_icon;`
4. `app-gpui/src/workbench/plugin_runtime.rs`, inside `mod tests {`, after the anchor `    mod proofs6_plugin_runtime;`: `    mod proofs9_status_snapshot;`
5. `app-gpui/src/workbench/plugins_window.rs`, inside `mod tests {`, after the anchor `    mod proofs6_plugins_market;`: `    mod proofs9_settings_plugins;`
6. `app-gpui/src/workbench/mod.rs`, inside `mod tests {`, after the anchor `    mod proofs7_quit;`:
   ```rust
       mod proofs9_log_users;
       mod proofs9_run_chooser;
   ```
   (`proofs9_log_users` is its own file, so the integrator can leave its `mod` line out until the fix lands.)

No accessor hunk is needed. Every accessor and helper used exists, and I read each one in this session:

- intelligence tests: `FakeServer::{install, answer, received, answers, log}`, `Fixture` fields and `{editor, rows, text, requests, put_caret, run, close}`, `open`, `settle`, `item`.
- `Workbench::{usages_view, tasks, settings_dialog, settings_plugins_page, diff_dialog, tool_window_view, vcs_state, config_chooser, run_driver, close_project, open_project}`, `UsageSearch::{groups, note, loading}`, `tasks::{BackgroundTask, can_cancel}`, the `task-cancel:{index}` selector, `view.backgroundTasks`, `TEXT_NOTE` (pub).
- github tests: `github_project, github_answers, show_list, open_first, GITHUB_REMOTE, notification_titles, wait_long, git_in`, `DetailsPanel::tree_rows`, `gh-change-{index}`, `DiffDialog::{data, viewer}`, `DiffViewer::{left_lines, right_lines, difference_count}`.
- plugin tests: `start_with_plugins, plugin_copy, link, trusted_project`, `registered, status_of, GREETINGS, LABEL`, `status_texts, project, wait, start` (plugin_runtime tests), `items`/`installed`, the `plugins-check-<id>` selector, `PluginsPage::rows`, `ListRow::Item`, `SettingsDialogView::selected_page`.
- workbench tests: `run_project, program, chooser_lines, select_in_chooser, click_on, run_action, settle, wait_long, git_project, init_repo, git_in`, `ConfigChooser::lines`, `run-chooser-pin`, `RunStore::{pinned, sessions, loaded}`, `LogFilterBar::{open, menu_lines}`, `FilterKind::User`.

## Per test → rows

### `editor::intelligence::tests::proofs9_resolve`

- `an_items_import_that_only_its_resolve_answer_carries_goes_in_with_the_accepted_item` → `ipc:lsp_resolve_completion` → **done**. Proof text: "`editor::intelligence::tests::proofs9_resolve::an_items_import_that_only_its_resolve_answer_carries_goes_in_with_the_accepted_item`, `editor_lsp::completion_view::tests::f1_shows_the_selected_rows_documentation_once_resolved`; the fake server lists an item holding only `data`; Enter puts in `obj.fooBar` and the `use foo;` import that only the server's `completionItem/resolve` answer carries (prefetched for the selected row or resolved after the insertion), and the resolve request hands the item's data back". Both native call sites (`CompletionEvent::Resolve`, `resolve_late_edits`) lead to the same text, so the test passes through whichever runs first. The webview prefetches the selected item's resolve (`prefetchResolve`, `src/editor/lsp/completion.ts:104`) and uses it on accept, which gives the same result.

### `editor::intelligence::tests::proofs9_syntax_usages`

The fixture installs the fake server for JavaScript through a user `languages/javascript.toml` with `tree-sitter = "javascript"`. A user file with the same id replaces the bundled one (`LanguageRegistry::load`), so vtsls and oxlint never start. Before the server starts, the fixture also patches its script to answer `textDocument/references` with a JSON-RPC error `-32603`. This follows the recipe in `proofs6_rest.md`.

- `find_usages_falls_back_to_the_identifier_occurrences_of_every_file_when_the_server_fails` → `ipc:syntax_identifiers_in_files` → **done** (it also re-proves `ipc:syntax_support` on the Find Usages path). Proof text: "`editor::intelligence::tests::proofs9_syntax_usages::find_usages_falls_back_to_the_identifier_occurrences_of_every_file_when_the_server_fails`; the server fails the references request; Find Usages on the undeclared global `shared` lists its identifier occurrences in a.js and b.js and not its mentions in comments, with the note `Text occurrences (language server not ready)`; the occurrences come from `ops::syntax::syntax_identifiers_in_files` over the files the text search found".
- `find_usages_on_a_redeclared_local_lists_its_occurrences_in_its_scope_when_the_server_fails` → `ipc:syntax_identifiers` → **done**. Proof text: "`editor::intelligence::tests::proofs9_syntax_usages::find_usages_on_a_redeclared_local_lists_its_occurrences_in_its_scope_when_the_server_fails`; `var x` declared twice in one function (JS `var` re-declaration is `Redeclaration::Conflict`, so the local is uncertain); Find Usages, with the server failing, lists the three `x` inside the function and not the `x` outside it: `ops::syntax::syntax_identifiers` on the document, filtered to the scope".

### `editor::intelligence::tests::proofs9_cancel_task`

- `cancelling_a_language_servers_work_in_background_tasks_asks_the_server_to_stop_it` → `ipc:cancel_task` → **done**. Proof text: "`editor::intelligence::tests::proofs9_cancel_task::cancelling_a_language_servers_work_in_background_tasks_asks_the_server_to_stop_it`; the fake server reports cancellable work-done progress (`$/progress` begin, `cancellable: true`); the window lists it as a cancellable background task; the cancel button of the Background Tasks popup (`task-cancel:<n>`) shows it Cancelling… and the server receives `window/workDoneProgress/cancel` for its token, which only `ide_core::task::cancel` running the LSP client's cancel handle (`ide-lsp/src/handler.rs::cancel_handle`) sends". This is the `CancelEffect::Backend` branch of `cancel_background_task`. The plugin-task test goes through the other branch. Webview: `src/workbench/tasks.ts:207` (`commands.cancelTask`).

### `workbench::github::tests::proofs9_github`

- `a_double_click_on_a_changed_file_diffs_it_from_the_merge_base_to_the_pull_requests_head` → `ipc:gh_pr_prepare_diff` → **done**. Proof text: "`workbench::github::tests::proofs9_github::a_double_click_on_a_changed_file_diffs_it_from_the_merge_base_to_the_pull_requests_head`; with the fake gh, #1's base is main's tip, which moved on after the branch forked; a double click on `src/main.rs` in the pull request's changes opens the diff window with `a` (the fork point) on the left and `b`, `c` (the head) on the right, not main's later `z`: the merge base comes only from `ide_github::prepare_diff`". This is the fake-gh fixture answer the assignment asked me to design. After the project opens, the test commits a fork point, a branch head and a later main commit locally, and rewrites `pull-request.json` with `baseRefOid` = main and `headRefOid` = head. Both commits exist locally, so `prepare_diff` fetches nothing. The fetch branch (`refs/pull/N/head` from the bare remote) is not exercised.

### `workbench::plugin_tool_windows::tests::proofs9_view_icon`

- `a_views_svg_icon_is_read_from_the_plugin_and_draws_its_tool_window` → `ipc:plugin_assets` → **done**. Proof text: "`workbench::plugin_tool_windows::tests::proofs9_view_icon::a_views_svg_icon_is_read_from_the_plugin_and_draws_its_tool_window`; the Hello fixture's Greetings view declared with `icon = \"icons/greetings.svg\"`; the tool window's icon becomes `ext:hello/view-greetings`, registered only from the SVG `PluginHost::assets(\"hello\", \"viewIcon/greetings\")` answers, and the plugin stays Inactive".

### `workbench::plugin_runtime::tests::proofs9_status_snapshot`

- `a_window_opened_after_a_plugin_set_its_status_item_shows_the_item` → `ipc:plugin_status_items` → **done**. Proof text: append "`workbench::plugin_runtime::tests::proofs9_status_snapshot::a_window_opened_after_a_plugin_set_its_status_item_shows_the_item`; a global item set by the api14 fixture's code before a second window opened shows in that window, which heard no status event for it: only `status_snapshot` gives it" and drop the "Missing:" clause.

### `workbench::plugins_window::tests::proofs9_settings_plugins` (group 68)

- `settings_plugins_lists_the_hosts_plugins_and_its_checkbox_disables_one` → `action:plugins.settings` → **done**; `settings:plugins` stays **partial**. Proof text for `action:plugins.settings`: "`workbench::tests::settings_for_new_projects_and_plugins_open_on_their_pages`, `workbench::plugins_window::tests::proofs9_settings_plugins::settings_plugins_lists_the_hosts_plugins_and_its_checkbox_disables_one`; without a plugin host the page says it is unavailable; with one, Settings opens on Plugins, which lists the installed Hello plugin enabled, and its checkbox disables it on the host". For `settings:plugins`, append the test and replace "no test opens Settings › Plugins with a plugin host" in its Missing clause. It still lacks the marketplace inside Settings (network) and the IntelliJ imports.

### `workbench::tests::proofs9_run_chooser` (group 32)

- `keeps_pins_per_project_across_reloads` → `unit:src/features/run/chooserPinned.test.tsx`, case "keeps pins per project across reloads".
- `marks_a_configuration_recent_when_a_run_session_starts` → same file, case "marks a configuration recent when a run or debug session starts". Only the run half is covered. The spec's debug-session half is not.
- The case "lists the configurations started last after the pinned ones, newest first" is the existing `run::config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind` (found by proofs7).
- Row status: 5 of 5 cases by name, but the debug half of "marks … recent" is missing, so the row stays **partial** unless the owner accepts run-only. Proposed proof text: "`run::chooser_view::tests`, `run::config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind`, `workbench::tests::proofs9_run_chooser::{keeps_pins_per_project_across_reloads, marks_a_configuration_recent_when_a_run_session_starts}`; pins made with the row's button come back in order when the project opens again, an unpin too, another project has none; a configuration run from the chooser is listed under Recent, newest first. Missing: a debug session marking a configuration recent".
- `unit:src/features/run/removal.test.tsx` was not reached: cases 103 (it.each over 8 kinds), 116, 123 and 133 are unchecked.

### `workbench::tests::proofs9_log_users`: EXPECTED TO FAIL

- `the_log_user_filter_lists_the_authors_of_every_repository_of_the_project` → `ipc:vcs_log_users` → **done once the fix lands**. Proof text: "`workbench::tests::proofs_vcs::the_log_user_filter_lists_the_authors_and_keeps_the_chosen_ones_commits`, `workbench::tests::proofs9_log_users::the_log_user_filter_lists_the_authors_of_every_repository_of_the_project`; the User filter lists the authors of every repository root of the project (Carol only in `lib`'s repository)".
- The webview reads every root (`src/features/vcs/GitLog.tsx:788-794`: the filter's repositories, else every repository). Native asks once with `repo: None` (`workbench/log_window.rs`, `LogFilterEvent::LoadUsers`), which reads the default repository only.
- Expected failure: a timeout in "the authors of both repositories". If it times out earlier, in "both repositories are found", then the native root scan does not list a nested repository. That is a hypothesis; check `vcs_state().roots` in that case. Fix (`app-gpui/src/workbench/log_window.rs`):
  ```diff
               LogFilterEvent::LoadUsers => {
  +                let roots: Vec<PathBuf> =
  +                    self.vcs_state().map(|vcs| vcs.roots.iter().map(PathBuf::from).collect()).unwrap_or_default();
                   let task = self.log_read(
                       cx,
  -                    |host, label| async move { git::vcs_log_users(&host, &label, None).await },
  +                    move |host, label| async move {
  +                        if roots.is_empty() {
  +                            return git::vcs_log_users(&host, &label, None).await;
  +                        }
  +                        let mut users = Vec::new();
  +                        for root in roots {
  +                            match git::vcs_log_users(&host, &label, Some(root)).await {
  +                                Ok(found) => users.extend(found),
  +                                Err(error) => tracing::debug!("a repository's users cannot be read: {}", error.message),
  +                            }
  +                        }
  +                        users.sort();
  +                        users.dedup();
  +                        Ok(users)
  +                    },
  ```
  The webview also prefers the log's repository filter when one is set. The native log has no repository filter yet.

## A: commands examined, with what became of each

The scope is the commands proofs6 left undrafted ("not drafted for time"), plus `gh_pr_prepare_diff`, the `syntax_*` recipe and `fs_rename`.

With a test (8): `lsp_resolve_completion`, `syntax_identifiers`, `syntax_identifiers_in_files`, `cancel_task`, `gh_pr_prepare_diff`, `plugin_assets`, `plugin_status_items`, `vcs_log_users` (the last one expected to fail).

Already proven by an existing test that the map did not cite (1). No new test; record it:

- `ipc:history_content` → **done** by "`workbench::diff_tools::tests::show_history_diffs_a_saved_revision_and_revert_writes_it_back_once_confirmed`; the Local History tab's diff shows the saved revision `xone` on the left against the editor's `xyzone`; the revision's text comes from `ops::doc::history_content` (`diff_tools.rs::load_history_diff` → `diff_window::load_text`, `SideLoad::History`)".

Not issued by the native code (8 commands, 6 of them already in proofs6 and re-checked):

- `ipc:plugin_terminal_session_close`: the map's call site `terminal_actions.rs:110` is the action `terminal.closeSession` (Close Terminal Session, which asks `term::is_busy`), a name clash. `PluginHost::close_terminal_session` has no caller in `app-gpui` (grep finds only `ide-host/src/plugins/{execution,remote_host}.rs` and the adapter). Missing: closing a plugin-started terminal session's tab does not tell the plugin host.
- `ipc:vcs_commits_changes`: `ChangesPart::Commits` comes only from `change_parts` with several commits. `feed_log_details` (`log_window.rs`) always feeds the log's single selection: `GitLog::key_down` ignores Shift/Cmd and `GitLog` has one `selected`. The webview selects several commits (Shift/Cmd click). Missing: multi-selection in the Git log. The row stays partial.
- `ipc:gh_pr_list`, `ipc:gh_pr_view`, `ipc:gh_pr_review_comments`, `ipc:gh_pr_review`, `ipc:gh_pr_comment` (already in proofs6): re-checked. Every `gh_call(` in `workbench/github.rs` other than checkout, availability and prepare-diff passes `"gh_api"`.
- `ipc:fs_rename` (already in proofs6): re-checked. `FsRequest::Rename` is still never constructed.

Issued only in a narrow case, not reached (1):

- `ipc:doc_open_virtual`: `structure_view::with_doc` opens a virtual URI only when the File Structure's file (or a supertype the inherited walk reaches) is virtual. No native path opens a virtual document in an editor (`open_route`/`OpenRoute::Virtual` have no caller outside `editor_model/virtual_input.rs`, and nothing constructs `git-revision:` URIs), so in practice the path is reached only through a language server's type hierarchy naming a supertype in a `jar:`/`jdt:` document. Recipe, not drafted: a fake server answering `typeHierarchy/supertypes` with a `jar:` item, plus a jar on disk.

No own native counterpart (1):

- `ipc:project_current`: the webview asks it after a reload of its page to learn the window's project (`state.project(label)` filtered to non-light, then `project_info`). The native window holds its project itself and has no page reload. Proposal: record "no native counterpart: the window keeps its project" (owner decision).

Differs from the webview, no test written (3):

- `ipc:vcs_restore_head`: the webview's undo of a merge or rebase run in several repositories (`resetBack` in `branchOps.ts`) calls it. The native undo of those does not, per the current proof. Not drafted: the native multi-repository merge/rebase undo would be product work.
- `ipc:vcs_config_get`: `commit.template` is not read natively (current proof). A test would enshrine an unbuilt feature. Missing: the Commit window prefilled from `commit.template` (webview reads it through `vcs_config_get`).
- `ipc:vcs_diff_with_worktree`: the Changes between dialog is not built (proofs6).

Missing piece is unported or unbuilt, nothing a test can close (5): `ipc:plugin_activation_event` (view activation has no native caller, since plugin views are not hosted as web views), `ipc:settings_set` and `ipc:settings_import` (the MCP re-apply and the update-check wake are features that are not ported), `ipc:vcs_log` (its missing piece is the path filter and a typed date range: Select Folders… and the date range's Select… are not built), `ipc:vcs_diff_with_worktree` (counted above too).

Not reached, left as recipes (2):

- `ipc:plugin_run_command`: the missing clause (the selected paths a menu passes) needs a fixture command that reports `selected_paths`. None of the fixtures I read does. Recipe: an api14 command echoing `invocation.selected_paths` into a notification; run it from the Project view's context menu on a file, and assert the notification names the file.
- `ipc:debug_redefine_classes`: reachable without jdtls through the "Classes changed" notification; see the group 26 recipe.

Blocked (1): `ipc:ssh_set_secret`. The assignment lists SSH as a hard blocker, and I did not check whether the test backend's secret store is file-backed.

## B: groups

| group                                             | rows covered / rows                                                                                  | result                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 20 Database results: spec cases through the views | 1 / 2 by record, no new test                                                                         | `unit:src/features/database/Grid.test.tsx` → **done**: its 12th case ("preserves short values and truncates only beyond the preview limit") tests the pure `cellPreview` at the spec's own level, which is `database::grid::tests::preserves_short_values_and_truncates_only_beyond_the_preview_limit`; the grid draws that preview (`database::grid_view::tests`, the assert `grid.cell_text(0, 0) == cell_preview(&full)`, grid_view.rs ~1185). Proof text: "`database::grid_view::tests`, `database::results_view::tests`, `database::grid::tests`; 12 of 12 cases, the preview case at the level the spec tests it (`cellPreview`), its use by the drawn grid in `database::grid_view::tests`". `results-view.test.tsx`: not examined (8 cases only on the model) |
| 26 Hot swap with a fake adapter                   | 0 / 2                                                                                                | the claim is contradicted, see below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 28 Project-tree backend-driven tests              | 0 / 2                                                                                                | not examined against the code (time)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| 32 Run chooser and removal spec cases             | chooserPinned's two missing cases (the debug half of one is missing); removal's 4 cases not examined | partial                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| 67 Plugin debug preparation asserted              | 0 / 1 (+ closer)                                                                                     | not written; the premise is corrected, see below                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| 68 Settings › Plugins with a plugin host          | 1 / 1 (+ closer partial)                                                                             | written                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |

No other group of size S, `testable`, tests-only remains outside what proofs7 covered: groups 2, 4 and 5 are owner records, not tests.

### Groups whose claim the code contradicts

- **26 (hot swap through the `HotSwapStep` seam).**
  - `debug.reloadChangedClasses` always builds first. In `debugger/actions.rs`, the action calls `reload_changed_classes(true, …)`, which issues `BackendCall::JavaBuildWorkspace` → `ops::java::java_build_workspace` → `project.lsp.java_build` (jdtls).
  - `debug.cancelHotSwap` only acts during that build.
  - The driver has no test seam for the build like the one it has for the debug server (`JavaDebugServerPort`, `debugger/driver.rs`).
  - The backend accepts `redefineClasses` only for the adapter named `java-debug` (`ide-dap` `HOT_CODE_REPLACE_ADAPTERS`), which also decides `caps.hot_code_replace`.
  - So the two action rows need a seam hunk first, in `debugger/driver.rs`, `BackendCall::JavaBuildWorkspace`:
    ```rust
    #[cfg(test)]
    if let Some(answer) = cx.try_global::<JavaBuildAnswer>().map(|answer| answer.0) {
        self.with_host(cx, |debugger, host| debugger.hot_swap_built(ticket, Ok(answer), host));
        return;
    }
    ```
    with `#[cfg(test)] pub struct JavaBuildAnswer(pub ide_lsp::jdtls::JavaBuildStatus); impl Global for JavaBuildAnswer {}`. For cancel, the test would need an answer that never comes, so the seam has to be able to hold the answer back.
  - With that seam, the session can be the `java-debug` attach of `workbench/plugin_java_debug.rs` tests: its TCP `java_debug_server` runs the fake adapter, and the adapter script would also answer `redefineClasses` with `{changedClasses: ["Main"]}`. That fixture is what the tests would build on.
  - Without any hunk, `ipc:debug_redefine_classes` is reachable today through the "Classes changed" notification's Reload button (`hotswap::RELOAD_WITHOUT_BUILD`, build = false). The adapter sends a `hotcodereplace` event with `changeType: BUILD_COMPLETE`. Recipe: extend `attaching_adapter()` to send that event after `configurationDone` and to answer `redefineClasses`; run `run_notification_action` on "Classes changed" → Reload; then assert the reload notice. Not written for time.
- **67 (plugin debug preparation).** The premise says a fake plugin's `prepare_debug_run` changes the configuration, but `prepare_debug_run` is not plugin code. `PluginHost::prepare_debug_run` (`ide-host/src/plugins/execution.rs:491`) is host-side: it replaces the configuration's command by the plugin's own program resolved for the project (`program_for_project`) and its debugger by the plugin's. No wasm fixture is needed. The test to write: in `plugin_contributions::tests::execution_project`, change the stored plugin configuration's `command` to `/bin/false` before Debug, and assert the session still runs the plugin's program (its output or the plugin hearing exit 0). Not written for time.

## Tests most likely to fail on first run

1. `proofs9_log_users`: by design (above).
2. `proofs9_syntax_usages::*`:
   - Checked in code: `ide-lsp/src/lib.rs::rpc_err` maps a server error other than ContentModified to `ErrorKind::Protocol`, and `native_references` → `Outcome::finish` returns it as `Err`, so the backend's `Unavailable`-only text fallback does not take it and the window's syntax search starts. What remains a hypothesis: `widened` (project-wide) propagating the first query's error.
   - They assume no plugin `indexed_symbol` exists (none without a plugin host).
   - The redeclared-`var` case assumes the function body is the symbol's scope and that `var x = 3` outside is not inside it.
3. `proofs9_cancel_task`:
   - It assumes the backend accepts `$/progress` begin without a prior `window/workDoneProgress/create`.
   - It assumes the window lists the task while it runs (titled with the progress title, possibly prefixed, hence `contains`).
   - It assumes the popup's rows are in `tasks().tasks()` order, because the cancel button's index is computed from it.
4. `proofs9_github`: it assumes the changes tree row of `src/main.rs` is drawn as `gh-change-<index of tree_rows()>` and that a double click on it reaches `DetailsEvent::OpenDiff`. If the row is under a collapsed `src` folder, expand it first.
5. `proofs9_view_icon`: it assumes the Hello fixture's manifest API allows an SVG path for a view icon (`read_icon` checks a relative path and sanitises it) and that `icon = "plugin"` is written exactly so in `plugin.toml` (it is, line 51).
6. `proofs9_run_chooser`:
   - It assumes `close_project` + `open_project` in the same window gives a new run store that reads the pins back from `UiStorage`.
   - It assumes Recent rows are not repeated in their kind's section, so `select_in_chooser` finds one row by name.
   - It assumes the chooser's section lines are spelled `[Pinned]` / `[Recent]` and the last row is `edit` (as in `run::chooser_view::tests`).
7. `proofs9_resolve`: it assumes the backend sends the stored item (with `data`) back on resolve and maps `additionalTextEdits` of the resolve answer into `additional_edits`.
8. `proofs9_status_snapshot`: it assumes `status_snapshot(label)` includes global items for a window that never heard them.

## Where I stopped

- A: every command in scope has a result above, except `ssh_set_secret` (blocked, not checked).
- B: groups 68 and 32 (chooserPinned) are written, and group 20's Grid row is recorded.
- Not reached for time: group 26's no-build redefine recipe, group 67's test, `results-view.test.tsx` (group 20), group 28, and group 32's removal cases. None of these was examined against the code.
