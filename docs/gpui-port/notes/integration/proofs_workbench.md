# Workbench, feature and UI proofs: integration notes

Nothing here has been compiled or run. Every test is in a new file, and each file needs one `mod` line inside an existing `mod tests`. All paths are relative to the worktree. Module names start with `proofs_` so they do not collide with the integrator's own. No existing file was edited.

## 1. `mod` lines to add

Each line goes inside the named `mod tests { … }`. Put it right after that block's `use` lines; the anchor is quoted.

| New file                                                       | `mod` line                 | Inside                                                 | Anchor (first lines of that `mod tests`)                                                                                                             |
| -------------------------------------------------------------- | -------------------------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/workbench/tests/proofs_agents.rs`                | `mod proofs_agents;`       | `app-gpui/src/workbench/mod.rs` `mod tests`            | `mod tests {` / `use super::*;` / `use crate::backend::AppDirs;` / `use gpui_kit::{TestAppContext, VisualTestContext};` / `use std::time::Duration;` |
| `app-gpui/src/workbench/tests/proofs_vcs.rs`                   | `mod proofs_vcs;`          | same                                                   | same                                                                                                                                                 |
| `app-gpui/src/workbench/tests/proofs_database.rs`              | `mod proofs_database;`     | same                                                   | same                                                                                                                                                 |
| `app-gpui/src/workbench/tests/proofs_services.rs`              | `mod proofs_services;`     | same                                                   | same                                                                                                                                                 |
| `app-gpui/src/workbench/tests/proofs_tool_windows.rs`          | `mod proofs_tool_windows;` | same                                                   | same                                                                                                                                                 |
| `app-gpui/src/workbench/tests/proofs_quit.rs`                  | `mod proofs_quit;`         | same                                                   | same                                                                                                                                                 |
| `app-gpui/src/workbench/debug/tests/proofs_debug.rs`           | `mod proofs_debug;`        | `app-gpui/src/workbench/debug.rs` `mod tests`          | `use std::time::Duration;` (the last `use` of that block, before `const FAKE_ADAPTER`)                                                               |
| `app-gpui/src/workbench/vcs_operations/tests/proofs_update.rs` | `mod proofs_update;`       | `app-gpui/src/workbench/vcs_operations.rs` `mod tests` | `use std::path::Path;` (after `use serde_json::json;`)                                                                                               |
| `app-gpui/src/run/segments/tests/proofs_marks.rs`              | `mod proofs_marks;`        | `app-gpui/src/run/segments.rs` `mod tests`             | `use crate::run::links::find_output_links;`                                                                                                          |
| `app-gpui/src/find/row_selection/tests/proofs_sections.rs`     | `mod proofs_sections;`     | `app-gpui/src/find/row_selection.rs` `mod tests`       | `use super::*;` (then `struct Row(&'static str);`)                                                                                                   |
| `app-gpui/src/platform/keymap/tests/proofs_presets.rs`         | `mod proofs_presets;`      | `app-gpui/src/platform/keymap.rs` `mod tests`          | `use serde_json::json;`                                                                                                                              |
| `app-gpui/src/platform/dispatcher/tests/proofs_keypad.rs`      | `mod proofs_keypad;`       | `app-gpui/src/platform/dispatcher.rs` `mod tests`      | `use std::collections::HashSet;`                                                                                                                     |
| `app-gpui/src/platform/actions/tests/proofs_titles.rs`         | `mod proofs_titles;`       | `app-gpui/src/platform/actions.rs` `mod tests`         | `use super::*;` (then `#[test] fn manifest_describes_the_webview_actions`)                                                                           |

The new files use the parent `mod tests`' private helpers and `use` items through `use super::*;`. The workbench files rely on these existing helpers, with the signatures they had when I read them: `start`, `settle`, `trusted_project`, `run_project`, `program`, `wait_long`, `click_on`, `run_action`, `context_is_on`, `install_fake_agent`, `agents_project`, `AgentsFixture`, `start_agent_task`, `agent_conversation`, `agent_working`, `settings_dialog_of`, `git_in`, `init_repo`, `git_branches`, `git_project`, `choose_in_branch_menu`, `wait_for_prompt`, `wait_for_notice`, `sqlite_source`, `db_query`, `db_tree`, `db_row_key` and `db_expand`. The debug file uses `FAKE_ADAPTER`, `PROGRAM`, `Fixture` (including its private `_folders` field), `open`, `long`, `run`, `context`, `close_project`, `wait_execution_at` and `notification_titles`. The update file uses `origin_project`, `push_from_another_clone`, `wait_for_form`, `press`, `OK`, `run_action`, `wait_long`, `wait_for_notice`, `notification_titles` and `git_in`.

## 2. Tests and the rows they prove

### `workbench/debug/tests/proofs_debug.rs` (4 tests)

- `the_session_keys_follow_a_session_from_paused_to_running_to_ended`: proves `context:debugSessionPaused`, `context:debugSessionRunning`, `context:debugSessionActive` and `context:debugCanRerun` as **done**. Before any session all four are off. At the breakpoint, paused, active and rerun are on and running is off. In the loop, running is on and paused is off. After Stop, active, running and paused are off. `debugCanRerun` is read off before the first session and on during it.
- `debug_without_a_configuration_starts_the_only_debuggable_one`: proves `action:debug.startConfig` as **done**. It runs with no argument, the backend's configuration list is read, the only debuggable configuration starts through `run.debugConfig` (which calls `debug.startConfig` again with `{config}`), and the program stops at the breakpoint with the frame `main, main.fake:2` and the `main` tab.
- `force_run_to_cursor_skips_breakpoints_force_step_into_steps_and_reset_frame_restarts`: proves `action:debug.forceRunToCursor` as **done**: it stops at the caret line 5 past the enabled breakpoint on line 3, which stays marked. It proves `action:debug.forceStepInto` as **done**: it steps one line. It proves `action:debug.resetFrame` as **done**: the execution line goes back to line 1 and the frame reads `main.fake:1`. **Fake extended:** the test overwrites `<home>/fake_dap.py` after `open()` and before the first session with `FAKE_ADAPTER` plus `supportsRestartFrame: True` and a `restartFrame` handler (back to line 1, `stopped` with reason `restart`). The replacement is done by string replace and asserted.
- `reset_frame_tells_that_an_adapter_cannot_restart_frames`: also proves `action:debug.resetFrame` (unsupported branch). The notice `Reset Frame is not supported by fake-dap` shows and the program stays paused on line 2.

### `workbench/tests/proofs_agents.rs` (3 tests)

- `an_agent_that_can_roll_back_regenerates_branches_forks_and_retracts`. **New fake:** `CAPABLE_AGENT`, a script that advertises `sessionCapabilities.fork` and `_meta.openidea.rollback`, answers `session/fork` and `_openidea/rollback`, and numbers its replies `Answer N`. It replaces `<home>/fake_agent.py` after `install_fake_agent` and before the project opens, so the first start (and any probe) uses it. The test asserts `task.capabilities.fork && rollback`. It proves:
  - `action:agents.regenerateReply` **done**: `Answer 2` is replaced by `Answer 3` and the second message is still listed once.
  - `action:agents.forkReply` **done**: a second task opens docked with turn 1 only.
  - `action:agents.forkTask` **done**: a third task (the existing test only proves the no-op).
  - `action:agents.retractMessage` **done**: the last message and its reply leave the conversation, and the composer draft holds `Say hello again`.
  - `ipc:agent_fork` **done**: the branch and the fork can only come from `ops::agent::fork` through `AgentCall::Fork`.
  - `ipc:agent_rollback` **done**: the regenerate and the retract of turn 1 go through `AgentCall::Rollback`, which calls `ops::agent::rollback` and then the agent's `_openidea/rollback`.
  - Check, while integrating, that `src-tauri/src/commands/agent*.rs` calls the same `ide_host::ops::agent::{fork, rollback}` (not checked).
- `agent_session_focus_is_on_only_while_a_conversation_has_the_keyboard`: proves `context:agentSessionFocus` as **done**: off in the editor, on in the composer, off again in the editor.
- `more_agents_opens_settings_on_the_agents_page`: proves `action:agents.addAgent` as **done**: the deferred `file.settings` opens the Settings dialog on page `tools.agents`.

### `workbench/tests/proofs_vcs.rs` (4 tests)

- `compare_previous_file_walks_back_and_the_diff_settings_menu_opens`: proves `action:diff.prevFile` and `action:diff.showSettings` as **done**. After `diff.nextFile` reaches `b.txt`, `diff.prevFile` brings back `a.txt` with `two`/`TWO`. `diff.showSettings` sets `workbench.popup`, Escape clears it, and the diff stays.
- `the_log_user_filter_lists_the_authors_and_keeps_the_chosen_ones_commits`: proves `ipc:vcs_log_users` as **done**. Opening the User filter (`LogFilterBar::open(FilterKind::User)`, which emits `LoadUsers`) lists `Ada <ada@example.test>` and `Bob <bob@example.test>`, read by `ops::vcs::vcs_log_users`. Choosing Bob leaves only `by bob` and `filters().authors == [Bob…]`.
- `the_commit_details_show_the_added_and_removed_lines_of_a_file`: proves `ipc:vcs_diff_numstat` as **done**. With `show_line_stats` set through `LogDetailsEvent::SetOptions` (what the view options' Show Line Counts emits), the details line reads `a.txt +3 −1` (U+2212), which is only filled by `read_line_stats` → `vcs_diff_numstat`.
- `deleting_a_branch_checked_out_in_a_worktree_removes_the_worktree_once_confirmed`: proves `ipc:vcs_remove_worktree` as **done**. The worktree sits in a second temp dir. Delete asks `Delete Branch`, and OK removes the worktree from `git worktree list`, deletes its folder, deletes the branch, and the notice `Deleted branch topic` shows.

### `workbench/vcs_operations/tests/proofs_update.rs` (1 test)

- `update_project_tells_what_came_in_from_where_the_branch_met_its_upstream`: proves `ipc:vcs_merge_base` as **done**. The local branch has its own commit (`l.txt`) and origin has `third` (`c.txt`). After Update Project with Rebase the notice is `1 file updated in 1 commit`. Counted from the HEAD fallback (no merge base) it would be `2 files updated in 1 commit`, which is asserted absent.

### `workbench/tests/proofs_database.rs` (2 tests)

- `go_to_ddl_on_a_trigger_opens_its_definition`: proves `ipc:db_object_ddl` as **done**. A SQLite trigger under users › triggers goes through `DbOp::ObjectDdl` → `ops::db::object_ddl`, and the editor holds `create trigger users_audit`.
- `explain_plan_logs_the_explained_statement_and_its_plan_nodes`: proves `ipc:db_explain` as **partial**. The Output tab logs `… explain select name from users` and `N plan node(s)` (only `Answer::Plan` writes those), and no `retrieved` line shows. It stays partial because the Query Plan tab is not drawn (`ResultsUpdate::Plan` only logs in `workbench/database.rs`).

### `workbench/tests/proofs_services.rs` (1 test)

- `run_all_and_stop_all_drive_a_type_group_and_a_started_servers_port_shows`. It proves:
  - `action:(dynamic) src/features/run/services.tsx:620` **partial**: `run.services.runAll` and `run.services.stopAll` on `run:type:program` start and end both configurations, and the rows say `:8123`/`Running`, then `Stopped`. `restartAll` and `openBrowser` are not run.
  - `action:open-browser` **partial**: the toolbar's `open-browser` is enabled while the server runs with a port and disabled after Stop. Opening the URL is not observable, and the action is never run so no browser opens.
  - `module:src/platform/serverPort.ts` **done**: `Tomcat started on port 8123 (http)` in a real process's output becomes the row's `:8123` through `run::store::started_port`. Its regex matches the webview's.

### `workbench/tests/proofs_tool_windows.rs` (2 tests)

- `hide_active_tool_window_hides_the_focused_project_view`: proves `action:toolwindow.hideActive` as **done**: the Project view is hidden and `toolWindowFocus` and `toolWindow.project.visible` are off.
- `resize_up_and_down_move_a_bottom_tool_windows_edge`: with `appearance_toggles_hide_the_bars_and_tool_windows_maximize_and_resize` (Left/Right), proves `action:(dynamic) src/workbench/toolwindows/actions.ts:247` as **done**. It also upgrades `boundid:toolwindow.resizeUp` and `boundid:toolwindow.resizeDown`: with the Run tool window focused, `sizes().bottom` grows and comes back.

### `workbench/tests/proofs_quit.rs` (1 test)

- `quit_with_unsaved_changes_asks_and_cancel_keeps_the_window`: would prove `ipc:app_quit` as **partial**: the question shows, Cancel keeps the window and the text. Quitting a clean app is not asserted because what `cx.quit()` does in the test app is unverified. **Expected to fail; see §5.**

### `run/segments/tests/proofs_marks.rs` (3 tests)

- `splits_a_line_around_matches`, `marks_the_current_match_and_keeps_its_span_style` and `splits_matches_inside_links`: the spec's three cases, one each. With the existing `run::segments::tests`, `unit:src/features/run/segments.test.ts` is **done**.

### `find/row_selection/tests/proofs_sections.rs` (1 test)

- `selects_only_options_across_headers_in_either_direction`: the spec's one case, with its six rows. `unit:src/ui/rowSelection.test.ts` and `module:src/ui/rowSelection.ts` become **done** (cite with `find::row_selection::tests`).

### `platform/keymap/tests/proofs_presets.rs` (14 tests) and `platform/dispatcher/tests/proofs_keypad.rs` (2 tests)

- `unit:src/platform/keymaps/eclipse.test.ts` (8 cases):
  - The per-preset cases are each one test looping over both presets: `the_eclipse_presets_load_switch_without_stale_overrides_and_parent_a_custom_keymap`, `the_eclipse_presets_have_valid_bindings_no_duplicates_and_no_unreviewed_new_conflicts` (the spec's `shared` table is ported as `SHARED`), `the_eclipse_presets_bind_eclipses_own_keys`, `the_eclipse_presets_preserve_official_removals_and_platform_specific_bindings` and `the_eclipse_presets_leave_undo_and_redo_to_the_focused_component`.
  - `the_eclipse_presets_use_the_bundled_run_and_debug_choices` covers the run/debug case.
  - The dispatcher's `the_eclipse_presets_tell_keypad_folding_from_font_size_and_block_comments` and `keypad_aliases_keep_working_for_existing_presets` cover the two keypad cases. Their `DispatchEnv` is an allow-list over `KeymapState::effective_bindings()`, as the spec registers only those actions.
  - Status: **done** at module level. Make it **partial** if the D16 caveat (GPUI reports no physical numpad key) is to be repeated here as in `unit:src/platform/keymap.test.ts`.
- `unit:src/platform/keymaps/windows.test.ts` (6 cases): `the_windows_keymaps_cover_the_same_actions_as_their_macos_presets`, `the_windows_keymaps_never_bind_the_windows_key`, `the_idea_windows_preset_binds_ideas_windows_keys`, `the_vscode_windows_preset_reads_cmd_as_ctrl`, `macos_keeps_cmd` and `windows_uses_ctrl_and_spells_shortcuts_out`. **Partial**: the spec's `hasPrimaryModifier` asserts have no public native counterpart (`dispatcher::Mods::command` is private). The rest of each case is ported.
- `unit:src/platform/keymaps/navExtras.test.ts` (2 cases): `shift_cmd_t_is_go_to_test_only_and_reopen_closed_tab_has_no_idea_default` and `the_idea_macos_preset_binds_the_navigation_and_editor_extras` make it **done**.

### `platform/actions/tests/proofs_titles.rs` (1 test)

- `has_a_chinese_entry_for_every_literal_action_title`: proves `unit:src/platform/locales/actionTitles.test.ts` as **done** if it passes. Every manifest action with a literal title (no `title_key`, not `verbatim`, not GraphQL/Vim) has a Chinese title different from the English one. It also checks that the two titles the spec pins are in the manifest.

## 3. Accessors and fixes the tests depend on

None: every accessor used exists in the source as read. Two fixes are suggested by expected failures (§5):

- **`app.quit` from inside a window (hypothesis).** The handler (`workbench/menu.rs`, `registry.handle("app.quit", …)`) calls `window.update(cx, …)` for every window while the action already runs inside the active window's update. GPUI takes the updated window out of its map, so its `update` errs, `unwrap_or(true)` counts it as agreeing, and the app may quit without asking. To check: `proofs_quit` fails on `pending_prompt().expect`. Fix: defer the loop out of the current update:

```rust
    registry.handle("app.quit", |_, _, cx| {
        cx.defer(|cx| {
            // Each window is asked first; one with unsaved changes keeps the app open while it asks.
            let mut all = true;
            for window in cx.windows() {
                let closes = window
                    .update(cx, |_, window, cx| {
                        super::for_window(window, cx).is_none_or(|workbench| {
                            workbench.update(cx, |workbench, cx| workbench.confirm_close(window, cx))
                        })
                    })
                    .unwrap_or(true);
                all &= closes;
            }
            if all {
                cx.quit();
            }
        });
        Ok(())
    });
```

(`cx` there is `&mut App`; `App::defer` takes `FnOnce(&mut App)`.) This touches the app's close flow, which the integrator is changing: apply it there, or leave the test out until then.

## 4. Recipes left out, and why

**Debugger / agents**

- `action:debug.evaluate`: **not built**. It is not in `DEBUG_ACTIONS` (`workbench/debug.rs`, whose comment says Evaluate Expression has no view here), and nothing reads `overlays.evaluate`. The webview opens an Evaluate dialog.
- F2 rest:
  - `agents.unarchiveTasks` and `agents.showToolWindow` are already asserted in `the_agents_follow_notifications_trust_and_task_commands_and_come_back_with_the_project`.
  - `ipc:agent_unarchive` comes with that test.
  - `ipc:agent_providers`, `agent_unqueue`, `agent_send_queued_now`, `agent_set_config`, `agent_stop_background` and `agent_changes` are not written: each needs the fake agent to grow modes, queueing, background tasks or file edits, and a composer accessor I did not verify.

**Database**

- F3 `ipc:db_schema_objects`: `sql::routines` and `sql::sequences` are `None` for SQLite, so nothing visible comes from it without a server.
- F3 `ipc:db_history` and `db_remove_history`: **not built**. `Command::ShowHistory` only logs "the Query History dialog is not available in this window".
- F3 `ipc:db_completion`: the console's `main>` prompt in the existing execute test already comes from the completion answer's default schema. SQL completion items in an editor are not wired to a UI I could assert.
- F4 `ipc:db_set_manual_commit` and `db_finish_transaction`: **not reachable**. They are issued only from `database/console_view.rs` (`ConsoleEvent::SetManualCommit` / `FinishTransaction`), and `ConsoleView` is not mounted anywhere in the workbench.
- F5 `ipc:db_set_password`: needs a source whose connect fails with an auth error, so it needs a server. Saving with `save: true` would reach the keychain, which tests must not touch.

**VCS**

- `ipc:vcs_commits_changes`: **not reachable**. `feed_log_details` (`workbench/log_window.rs`) feeds `log.selected()` only, one commit, so `ChangesPart::Commits` is never asked for. The webview shows the union of the changes of several selected commits. Fix: feed every selected commit of the log (needs a `GitLog` multi-selection accessor).
- `unit:src/features/vcs/__tests__/GitMergeDiff.test.tsx`: **differs from the webview**. `show_log_diff` answers a merge commit's file with the warning notice "The diff of a merge commit cannot be shown". The spec wants three read-only panes (both parents and the result), so no test is written to enshrine the notice.

**Services and run**

- Docker `action:delete` and `action:refresh`: these need a Docker double; there is none in the tree.
- `module:src/platform/runTargetSelection.ts` and `module:src/platform/javaLaunch.ts`: not written (time); javaLaunch needs a JDK.

**Workbench subsections**

- `action:file.openInNewWindow`: left out, since window open/close is the integrator's.
- `ipc:window_toggle_fullscreen` and `ipc:window_open_settings`: the owner's call (no ops function).
- `unit:src/workbench/messages.test.tsx` / `module:src/workbench/messages.tsx`, the merge family `action:(dynamic) src/features/vcs/mergeActions.ts:64`, `module:src/workbench/projectColor.ts`, `module:src/workbench/MainToolbarDialog.tsx`, `unit:src/workbench/lightEdit.test.tsx`, `unit:src/workbench/toolwindows/badges.test.tsx`, `unit:src/features/project-tree/ProjectView.test.tsx`, `ipc:fs_entry_kind` and the 7 module rows (toolwindows/actions.ts, Notifications, EditorArea, OverlayHost, inputDialogs, softWrap, WindowControls): not written, for time. Each needs its spec read case by case.
- `point.registerKeyClaim` rows (run console find bar, diff ⌘W): not written. The diff one is partial by design until ⌘W is claimed.

**Platform**

- `unit:src/platform/keymapMouse.test.ts` (13 cases): not mapped, for time.

**ui**

Of the 22 rows only `rowSelection` (2 rows) is proven. The other 20 are module audits with no single native counterpart:

- `Dialog`, `List`, `Tree`, `Tabs`, `Popup`, `Select`, `Scrollbar` and `Splitter` (module and unit rows): no single native module, so a spec port means choosing a representative view per primitive.
- `ansi`: `run::ansi::tests` exist.
- `ConsoleFindBar`: `ui::console_find` with the run console's find bar.
- `Input`: `ui::text_field`.
- `Tooltip`: `ui::tooltip`.
- `symbolKind`: the symbol-kind icons.

Each needs its TS module read against the native one; none was proven here.

### e2e specs (the 32-row subsection)

None of these was verified step by step against a native test. Every line below is a reading of the spec's test titles (and, for the small ones, its body) against the native test names in the tree: a hypothesis, to check by reading both. No spec is claimed as replayed in full.

- `boot.spec.ts`: missing piece. The splash and module-evaluation diagnostics are webview boot mechanics with no native counterpart; likely N/A.
- `codeInsight.spec.ts`: the editor_intelligence fake-server tests (not mine) cover completion, ⌘B, Find Usages, rename and quick fix piecewise. Not one replay.
- `commentToggle.spec.ts`: missing a JSX (`.tsx`) toggle-comment test; the native comment tests are per language.
- `commit.spec.ts`: closest native test is `commit_takes_exactly_the_included_files_with_the_typed_message` / `commits_the_typed_message_from_the_keyboard`. The spec also asserts "Specify commit message" on an empty message and the unversioned group starting unchecked; check those first.
- `commitInclusion.spec.ts`: closest is the commit test using `assert_only_a_included`; check whether it covers the #1539 case (changes left out stay unchecked after a refresh).
- `compareWith.spec.ts`: missing the browser file picker case (N/A natively). The three menu cases need menu-content accessors for the tree, tab and editor menus.
- `conflictMarkers.spec.ts`: missing a native test typing conflict markers in a `.kts` file and asserting three error diagnostics without a server, then undo clearing them.
- `console-selection.spec.ts`: closest is `select_all_copies_the_whole_buffer_and_the_menu_follows_the_input`; the virtual-window drag and scroll cases are webview virtualization.
- `debug.spec.ts`: `debugging_a_configuration_stops_at_the_breakpoint_steps_over_and_resumes_to_the_end` (fake adapter) covers breakpoint, frames, variables, step and resume. Check that the spec has nothing else. This is the most likely full replay.
- `diff-navigation.spec.ts`: missing the diff preview tab (the native editor has no diff tabs, see `ShownChanges::PreviewTab` in `log_window.rs`). Next/previous file is covered by `show_diff_on_a_change_…` and `proofs_vcs`.
- `diff.spec.ts`: algorithmic cases belong to `vcs_diff` unit tests, not one workbench replay.
- `editor.spec.ts`: `files_open_in_tabs_that_the_keymap_cycles_closes_and_reopens`, `the_keymap_runs_editor_actions_on_the_windows_editor` and the find-bar tests cover most cases. Missing pieces: the breadcrumbs case and "each file keeps its own find bar".
- `gitLogBranchMenu.spec.ts`: missing a test for a commit's branch menu acting in a linked repository.
- `goHighlight.spec.ts`, `kotlinHighlight.spec.ts`, `pythonHighlight.spec.ts`, `httpHighlight.spec.ts` and `usage-highlight-contrast.spec.ts`: highlighting colours, owned by the `editor::view` highlighting group (not mine).
- `keymap.spec.ts`: `settings_keymap_assigns_a_recorded_shortcut_that_the_keymap_then_resolves` and `records_a_shortcut_shows_conflicts_and_applies_it_to_the_keymap_on_ok` cover recording. Missing: the Add Mouse Shortcut menu and dialog flow from a double click or Enter.
- `mergeEditor.spec.ts`: `the_merge_editor_opened_from_the_banner_runs_its_actions_…` covers part. The CRLF, binary, deleted-version and keyboard cases need the merge-family test listed above.
- `newBranch.spec.ts`: `new_branch_rename_and_delete_change_the_repositorys_branches` covers the space-to-separator case and `offers_overwrite`. Missing: the caret after a middle space, paste, and overwrite only after opting in.
- `openProject.spec.ts`: Close Project → Welcome → recent project; this is window open/close (integrator).
- `projectTree.spec.ts`: missing type-ahead, the Git ignore submenu and the drag cases (keyboard navigation is covered).
- `run.spec.ts`: covers the ⌃R and ⌘F2 cases by the run tests (`a_cargo_build_lists_its_error_…` and the RunView test in `mod.rs`). The Go rerun and CMake cases need those toolchains.
- `search.spec.ts`: `search_everywhere_runs_actions_finds_text_and_switches_tabs`, `search_everywhere_gives_the_keyboard_back_…` and `find_in_files_searches_the_project_and_opens_a_match` cover all four titles. It is a likely full replay; check step by step.
- `services.spec.ts`: missing service drag-and-drop ordering for Java and npm.
- `settings.spec.ts`: belongs to the Settings dialog group (not mine).
- `stripeTooltip.spec.ts`: missing tooltip placement and colour assertions (pixel geometry).
- `terminal-clipboard.spec.ts`: missing clipboard media pasted into the terminal as a shell-quoted argument.
- `themes.spec.ts`: `settings_appearance_applies_the_chosen_theme`, `follows_the_os_mode_until_a_theme_is_chosen` and the theme-toggle test cover part. Missing: the preview-under-pointer cases.
- `toolwindows.spec.ts`: `tool_window_actions_show_focus_and_hide_the_project_view` covers ⌘1 and ⌘⇧F12. Missing: Shift+Escape (`toolwindow.hideActive`, proven above by action, not by key), ⌘4, ⌥F12 terminal focus and Split Right in the terminal.
- `workbench.spec.ts`: title bar, stripes and status bar rendering are covered piecewise by the chrome tests; the simulated project-opening progress is webview-only.

## 5. Most likely to fail first

1. **`proofs_quit`**: the hypothesis in §3; `cx.pending_prompt()` may be `None` because the active window cannot be updated from inside its own action.
2. **`proofs_titles`**: the manifest may hold literal titles that the zh catalog lacks or translates identically (product names other than GraphQL/Vim, for instance). The failure lists them one per line.
3. **`proofs_agents::an_agent_that_can_roll_back_…`**:
   - If `ide_agent` sends a request the capable script does not answer (e.g. `session/set_mode` or `session/set_config_option` on start), the error reply may fail the start. Extend the script's handled methods then.
   - If the fork's copy expects `loadSession: true` to show its entries, the branch's conversation may stay empty.
4. **`proofs_debug::force_run_to_cursor_…`**:
   - The adapter's `restartFrame` resets `current` but not `ended`/the stack; the frame should read line 1.
   - If the driver waits for a `continued` event before the stop, a 0.2 s gap is already in `stop()`.
   - `session.adapter` might be the adapter's display name rather than `fake-dap`, which affects the notice title in `reset_frame_tells_…`.
5. **`proofs_vcs::the_log_user_filter_…`**: if `vcs_log_users` returns names without emails, the labels differ. Adjust the two strings to what the first run prints.
6. **`proofs_vcs::deleting_a_branch_checked_out_in_a_worktree_…`**: the branch popup may label a branch held by another worktree differently, or offer no `Delete` row for it; `choose_in_branch_menu` panics with the rows it saw.
7. **`proofs_tool_windows::resize_up_and_down_…`**: if `toolwindow.run` does not give the (empty) Run view the keyboard, `toolWindowFocus` is off and the resize has no active window. Use the Terminal instead (`toolwindow.terminal`, shells are isolated by `start`).
8. **`proofs_presets::the_eclipse_presets_have_valid_bindings_…`**: the native JSON is generated from the TS presets; any binding the generator normalizes differently shows as a new conflict.
