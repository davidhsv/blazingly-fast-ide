# D103 integration report: the `proofs11` batches (75 tests, 18 files)

Both batches are integrated: all 75 tests are declared, they pass, and none was left out. Getting them to pass took seven small app fixes (part 2). Five agents tests had wrong assumptions and were corrected in the test, without weakening what they check (part 2a).

## 1. Gates (measured on the final tree)

| Gate                                        | Result                                                                                      |
| ------------------------------------------- | ------------------------------------------------------------------------------------------- |
| `cargo test --lib`, run 1                   | `ok. 6983 passed; 0 failed; 0 ignored` (134.96 s)                                           |
| `cargo test --lib`, run 2                   | `ok. 6983 passed; 0 failed; 0 ignored` (135.84 s)                                           |
| `cargo test --lib -- proofs11`, ×20         | 20 of 20 `ok. 75 passed; 0 failed; 0 ignored; 6908 filtered out`; no test failed in any run |
| `cargo test --test web_view`                | `ok. 1 passed`                                                                              |
| `cargo clippy --all-targets -- -D warnings` | exit 0                                                                                      |
| `cargo build`                               | exit 0, 0 warnings                                                                          |

6983 = 6908 + 75. The 20 `proofs11` runs took 3.4–3.9 s each.

**Failures seen during integration, all fixed:**

- **GitHub:** `view_pull_requests_shows_the_tool_window_and_gives_it_the_keyboard` failed (`activeToolWindow` was None). App fix 1.
- **Agents, six tests:** the two `stops_before_rollback…turn_0`, `confirms_before_retracting…`, `rolls_back_before_sending…` and `waits_for_a_running_turn…` failures were test errors (2a). `applies_worktree_cleanup_only_to_worktree_tasks` failed because the checkbox started unticked (app fix 3). `keeps_the_draft_and_avoids_rollback…` timed out (app fix 4).
- **VCS:** `add_to_local_exclude_refreshes_the_status_and_the_project_tree` and `refreshes_after_a_failed_or_partially_completed_multi_repository_write` timed out at "the tree to be read again". App fix 2.
- **Hot swap:** `cancel_hot_swap_build_aborts_the_reload_before_any_class_is_redefined` timed out at "the compile". App fix 5.
- **Small ones:**
  - a compile error (`Focusable` not in scope in `workbench/github.rs`);
  - an unused `prelude::*` import in `proofs11_task_list.rs`;
  - a clippy `cloned_ref_to_slice_refs` in `proofs11_agents.rs:218`, changed to `std::slice::from_ref`.

**What the tests reach, measured:**

- **Docker:** no read is carried out. A temporary `eprintln` in the `DockerRequest::Snapshot` arm of `carry_out_docker` printed nothing over the 3 tests, and I removed it.
- **Hot swap:** the attach never reaches `JavaResolveLaunch`. I checked that the same way; the hash of `git diff` was identical before and after (`625e6fbe…`).
  - Hypothesis: no JVM or jdtls starts. The other two Java calls, the debug server and the compile, are answered by the test seams.
  - What the hot swap tests do open: a loopback listener on 127.0.0.1, and each connection spawns `python3 java_dap.py` from the temp folder.
- **GitHub:** the tests rewrite the fake `gh` inside the temp `.git/fake-gh`.
- **Clipboards:** the shelf and import tests use GPUI's test clipboard.
- **50 MiB file:** it is made with `set_len`, so it is sparse.

## 2. What changed

**`mod` lines:** all 18 files, added to the inline `mod tests` the notes name.

**Brief hunks:**

- **H1** (`workbench/services_window.rs`): `state()` and `docker_mut()` added next to the existing `detail()`, not replacing it. Proven by the 3 `proofs11_docker` tests.
- **H2** (`debugger/driver.rs`):
  - adds a test-only `JavaBuildAnswer` global;
  - in the test build, the `JavaBuildWorkspace` arm answers after a delay on the test clock and then pumps the driver;
  - the `#[cfg(test)] if let … return;` form compiles; it is the pattern the file already uses for `JavaDebugServerPort`;
  - proven by the 2 `proofs11_hotswap` tests.
- **H3** (`platform/keymap.rs`): adds `has_primary_modifier`, now used by the four exact private copies:
  - `run/view.rs::primary`
  - `debugger/variables_view.rs::primary`
  - `vcs_diff/view.rs:1201`
  - `editor_lsp/hover_view.rs:635` (the notes missed this one)
  - Proven by the 2 `proofs11_host_modifier` tests.
  - I left alone the copies that accept ⌘ or Ctrl on purpose: `database/grid_view.rs::primary` and `run/edit_configs_view.rs:1179`.
  - I kept the two old `proofs_presets` tests that the new ones repeat.

**App fixes, each with the webview source it follows and the test that proves it:**

1. **Pull Requests keeps the keyboard after it connects.** Webview `src/features/github/toolwindow/open.ts:11-12`: `show(WINDOW, focus = true)`.
   - Cause: the keyboard went to the window's outer handle before it connected; once connected, the window hands focus to the list's handle, which does not contain the outer one.
   - Fix: `github/window_view.rs` adds `PullRequestsEvent::FocusTargetChanged`, emitted when the connection changes, plus `holds_focus()`. `workbench/github.rs` then gives the keyboard to the new target if the window held it.
   - Test: `workbench::github::tests::proofs11_github::view_pull_requests_shows_the_tool_window_and_gives_it_the_keyboard`. All 245 `github` tests passed afterwards.
2. **The project tree is read again after Add to .gitignore / .git/info/exclude, whether the write worked or not.** Webview `src/features/vcs/ignore.ts:17`.
   - Fix: `Workbench::add_to_ignore` (`workbench/vcs_operations.rs`) calls `reload_all`.
   - Tests: the 3 `workbench::tests::proofs11_vcs_windows::*` tests.
3. **The worktree-cleanup checkbox starts ticked.** Webview `src/features/agents/taskActions.ts:59-64` (`checked: worktree`).
   - Cause: the native `confirm_batch` dropped `request.checked`.
   - Fix: added `PromptDialog::with_checked` (`vcs/prompt_dialogs.rs`), used in `workbench/agents.rs`.
   - Test: `workbench::tests::proofs11_agents::applies_worktree_cleanup_only_to_worktree_tasks`.
4. **The agents stop timeout.**
   - Cause: the deadline is checked against the wall clock, but the timer ran on the test clock and fired early. Nothing re-armed it, so the stop never timed out.
   - Fix: `workbench/agents.rs::agents_wake_at` waits out the rest of an early wake-up. As the brief asks, the timeout is made shorter in the test build: `AgentsStore::set_stop_timeout_ms`, test-only, with a `stop_timeout_ms` field that defaults to `STOP_TIMEOUT_MS`. The test sets it to 1 s.
   - So the production 15 s path is proven only by the existing store unit test, not through the window.
   - Test: `workbench::tests::proofs11_agents::keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time`, now about 1 s.
5. **`debugHotSwapBuilding` is now published.** Natively nothing set it before. Webview `src/features/debugger/hotswap.ts:129,145`.
   - Fix: `workbench/debug.rs::publish_debug_context`.
   - Test: `workbench::debug::tests::proofs11_hotswap::cancel_hot_swap_build_aborts_the_reload_before_any_class_is_redefined` reads it ON while compiling and OFF after.

**2a. Agents tests corrected (the test was wrong, not the app):**

- In the backend (`crates/ide-agent/src/lib.rs`), `rollback` to `turns == 0` drops the session and starts a fresh one, so the agent never receives `_openidea/rollback` for the first turn.
- Editing the first message asks "Start a Fresh Agent Session" in both apps: webview `rerunFrom`, whose spec mocks the confirm as yes.
- What I changed:
  - `rolls_back_before_sending_the_edited_text_when_send_is_clicked` and `waits_for_a_running_turn_to_stop_before_rolling_it_back` now confirm that dialog. They prove the rollback by a second `session/new` in the agent's log, before the edited prompt and after `session/cancel`. They also check that only the edited turn and one answer remain.
  - `stops_before_rollback_and_restores_text_without_sending_turn_0` and `confirms_before_retracting_a_turn_with_later_messages_and_guards_repeated_clicks` now assert 0 `_openidea/rollback` (the session is dropped), the emptied conversation, the restored draft and no further prompt.

**Writers' suspicions that turned out false:**

- Retract already asks first when later messages exist (`reply_actions.rs::retract`).
- The execution-schema results test passed with no change.

**Left undeclared:** none.

## 3. Inventory rows (`parity.json` is under `docs/`, so I did not edit it; all keys below are `partial` there today)

The "done" verdicts rely on the writers' mapping of spec cases to tests. I measured only the raw case counts (`grep -cE "^\s*(it|test)(\.each…)?\(" <spec>`): Grid 11 (writer says 12), list 15, worktreesPanel 5, executionState 10, windows.test 5 (writer says 6), ignore 7. Hypothesis: each case maps to a named test; check by matching each `it(` against the citations in the tests' doc comments.

| Key                                                                                           | State reached                                       | Proving tests (module `app-gpui/src/...`)                                                                                                                      | Missing                                                                                       |
| --------------------------------------------------------------------------------------------- | --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `unit:src/features/database/Grid.test.tsx`                                                    | done (hypothesis, as above)                         | `database::grid_view::tests::proofs11_grid::preserves_short_values_and_truncates_only_beyond_the_preview_limit`                                                |                                                                                               |
| `unit:src/features/github/list/list.test.tsx`                                                 | done (hypothesis)                                   | the 7 `github::list_panel::tests::proofs11_list::*` tests                                                                                                      |                                                                                               |
| `action:github.pullRequests`                                                                  | done                                                | the 3 `workbench::github::tests::proofs11_github::*` tests                                                                                                     |                                                                                               |
| `unit:src/features/vcs/__tests__/ignore.test.ts`                                              | done (hypothesis)                                   | the 3 `workbench::tests::proofs11_vcs_windows::*` tests                                                                                                        |                                                                                               |
| `unit:src/features/vcs/__tests__/worktreesPanel.test.tsx`                                     | done (hypothesis)                                   | the 3 `workbench::vcs_operations::tests::proofs11_worktrees::*` tests                                                                                          |                                                                                               |
| `action:docker.refresh`                                                                       | done                                                | `…proofs11_docker::refresh_docker_reads_the_connection_again_and_the_tree_shows_what_it_found`                                                                 |                                                                                               |
| `action:docker.toggleStoppedContainers`                                                       | done                                                | `…proofs11_docker::show_stopped_containers_leaves_the_stopped_container_out_of_the_tree`                                                                       |                                                                                               |
| `action:docker.toggleUntaggedImages`                                                          | done                                                | `…proofs11_docker::show_untagged_images_leaves_the_untagged_image_out_of_the_tree`                                                                             |                                                                                               |
| `action:debug.reloadChangedClasses`                                                           | done                                                | `workbench::debug::tests::proofs11_hotswap::reload_changed_classes_compiles_then_reloads_the_classes_into_the_java_session`                                    |                                                                                               |
| `action:debug.cancelHotSwap`                                                                  | done                                                | `…proofs11_hotswap::cancel_hot_swap_build_aborts_the_reload_before_any_class_is_redefined`                                                                     |                                                                                               |
| `unit:src/features/debugger/__tests__/executionState.test.ts`                                 | done (hypothesis)                                   | `debugger::state::tests::proofs11_execution_state::retains_locals_whose_names_are_object_prototype_properties`                                                 |                                                                                               |
| `unit:src/platform/keymaps/windows.test.ts`                                                   | done (hypothesis)                                   | `platform::keymap::tests::proofs11_host_modifier::{macos_keeps_cmd, windows_uses_ctrl_and_spells_shortcuts_out}`                                               |                                                                                               |
| `unit:src/features/agents/AgentsView.test.tsx`                                                | partial                                             | the 5 bulk tests of `workbench::tests::proofs11_agents`; the 7 docked/menu tests of `agents::task_list_view::tests::proofs11_task_list`                        | "continues after an error and reports the failed task" (needs a delete that fails on purpose) |
| `unit:src/features/agents/messageEditing.test.tsx`                                            | partial                                             | the 8 editing/retract tests of `workbench::tests::proofs11_agents`                                                                                             | 4 cases about images or a failed send                                                         |
| `unit:src/features/agents/sessionState.test.tsx`                                              | partial                                             | 4 `agents::session_view::tests::proofs11_session` tests; `proofs11_task_list::leaves_a_task_unread_while_its_kept_alive_tab_is_hidden_and_reads_it_once_shown` | scroll and follow geometry, tag kinds, question cards, docked changes                         |
| `unit:src/features/agents/unread.test.tsx`                                                    | partial                                             | the 5 unread tests of `proofs11_task_list`; `proofs11_session::keeps_the_latest_usage_limits_per_provider_without_marking_anything_unread`                     | status-bar count and tool-window badge (no caller), editor-tab conversations, window focus    |
| `unit:src/features/agents/Markdown.test.tsx`                                                  | partial                                             | the 4 `agents::markdown_view::tests::proofs11_markdown::*` tests                                                                                               | images and diagrams: the view lacks the behaviour                                             |
| `unit:src/features/database/results-view.test.tsx`                                            | partial                                             | the 9 `database::results_view::tests::proofs11_results::*` tests                                                                                               | "records the selected schema or connection default" (the console's job)                       |
| `unit:src/features/vcs/__tests__/shelf.test.tsx`                                              | partial                                             | the 2 `workbench::shelf::tests::proofs11_shelf::*` tests                                                                                                       | Compare with Local skipping files; cross-root Unshelve                                        |
| `module:src/editor/core/fileAttributes.ts`                                                    | partial                                             | the 2 `workbench::editor_core::tests::proofs11_read_only::*` tests                                                                                             | the virtual-document case (needs jdtls; natively passed as `virtual_document: false`)         |
| `action:debug.pluginStart`                                                                    | partial                                             | `workbench::plugin_contributions::tests::proofs11_plugin_debug::the_debug_preparation_changes_the_program_and_the_session_launches_the_changed_one`            | the Java attach from the configuration chooser                                                |
| `unit:src/features/project-tree/fs.test.ts`                                                   | partial                                             | `workbench::project_view_actions::tests::proofs11_import::uses_project_copy_for_a_local_destination`                                                           | the 3 remote cases                                                                            |
| `context:debugCanHotSwap`, `context:debugHotSwapBuilding`                                     | no entry in `parity.json` (checked with `key in d`) | read ON and OFF by the two hotswap tests                                                                                                                       | the rows need adding                                                                          |
| `unit:src/features/project-tree/unityRefresh.test.ts`, `findPopup.test.tsx`, the 3 diff specs | unchanged                                           | none in this slice                                                                                                                                             | no tests were written                                                                         |

## 4. Hypotheses still open

- **No JVM or jdtls runs in the hot swap tests:** see part 1. To confirm, add an `eprintln` in every `BackendCall::Java*` arm of `driver.rs` and run `cargo test --lib -- proofs11_hotswap --nocapture`.
- **Spec-case coverage of the "done" rows:** see the note above the table in part 3.

## 5. Stale or false in the notes and briefs

- The views notes say no accessor or fix is needed; three app fixes were (fixes 1, 3 and 4).
- The windows notes call H3's replacement of the private copies optional, but the brief requires it, and the notes list only two of the four copies.
- `shelf.rs` has one `mod tests`, not a second one; the same goes for `debugger/state.rs`.
- The line counts in the notes differ slightly from the files (for example `proofs11_grid.rs` is 25 lines, not 27).
- The notes miss that `debugHotSwapBuilding` was never published natively.
- The notes miss that a rollback to turn 0 never reaches the agent.

## Tree state

- 29 files modified in `app-gpui/`, nothing committed.
- Formatting was done with `fmt.sh`, on the files from `git diff --name-only` plus the 18 brief files.
- `git status --short src/ src-tauri/ docs/ e2e/ crates/` prints nothing.
- Five untracked items from another agent appeared and were not touched: `app-gpui/src/workbench/tests/proofs12_{diff_soft_wraps,diff_window,find_popup}.rs`, and the folders `app-gpui/src/vcs_diff/diff_options/` and `app-gpui/src/vcs_diff/view/`.
- Gate outputs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d103/` (`gates2.log`, `suite2_1.txt`, `suite2_2.txt`, `p11b_1.txt` … `p11b_20.txt`, `webview2.txt`, `clippy2.txt`, `build2.txt`).
