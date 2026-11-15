I wrote 19 tests in 8 new files; one of them is meant to fail until a field is fixed. Nothing was compiled or run: each file only passes `rustfmt --check`, and every expected state below assumes the tests pass. The notes are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs20_b.md`, with the 7 `mod` lines (each after a quoted anchor), two hunks, the full table and the failure risks.

**Files** (all under `app-gpui/src/`)

- `workbench/tests/proofs20_shell.rs`: 5 tests
- `workbench/app_actions/tests/proofs20_layouts.rs`: 3
- `workbench/project_view_actions/tests/proofs20_tree.rs`: 2
- `workbench/editor_language/tests/proofs20_generate.rs`: 5
- `workbench/editor_language/tests/proofs20_mcp_problems.rs`: 1
- `platform/keymap/tests/proofs20_new_element.rs`: 1 (plain `#[test]`)
- `workbench/zoom_actions/tests/proofs20_quick_switch.rs`: 1 (new `tests/` folder)
- `workbench/github/tests/proofs20_remote_branch.rs`: 1

**Two hunks the tests need**

- **A:** an accessor `TitleBar::project_avatar_fill` in `title_bar.rs`, for the avatar test.
- **B:** a dev-dependency `ide-mcp = { path = "../crates/ide-mcp" }` in `app-gpui/Cargo.toml`, for the MCP test. Without it, leave out that test's `mod` line.

**Row → test → state if it passes**

- `unit:actions.test.ts` and `module:actions.ts` → `proofs20_shell::propagates_host_launch_errors_…` and `…listeners_hear_the_action_the_user_runs_and_not_the_ones_it_runs_itself` → done (1 of 1). The spec's "logged once" clause is not asserted, because nothing in the crate captures log output.
- `context:popupOpen` → `…a_menu_over_the_window_sets_popup_open_until_escape_closes_it` → done.
- `unit:store.test.ts` → `…registers_toolwindow_id_and_generic_actions`, which runs all 19 tool windows → done (26 of 26).
- `unit:layouts.test.ts` → `proofs20_layouts::{saves_and_restores_…, replaces_…, persists_…}` → done (5 of 5 through the window).
- `module:quickSwitch.ts` → `proofs20_quick_switch::…` → partial: the theme preview on hover is not built.
- `module:TitleBar.tsx` → `…the_title_bars_avatar_is_drawn_on_the_colours_of_the_projects_slot` → done with hunk A. It reads the colours handed to the renderer, not painted pixels.
- `action:project.compareWith` → `proofs20_tree::compare_with_hands_two_files_…` → done.
- `point.registerUndoHandler` → `…undo_in_the_project_view_takes_back_its_last_move_and_rename` → partial: undo runs through `edit.undo`, but no test presses ⌘Z in the tree. `feature:project-tree` stays partial.
- `action:code.generate`, `code.overrideMethods`, `code.applyQuickFix` → the three `…indents_the_inserted_…` tests, each also checking the action is refused with the Project view focused → done.
- `unit:newElement.test.ts` → `binds_new_in_this_directory_like_idea` and `lets_generate_win_the_new_key_only_where_it_applies` → done (2 of 2).
- `module:workspaceEdit.ts` → `…the_rename_preview_says_how_many_occurrences_…` → partial: the intention-preview diff is not checked in a window.
- `ipc:mcp_report_problems` → `proofs20_mcp_problems::…` → done with hunk B. The native and Tauri code both call `mcp_reports.report_problems`.
- `unit:RemoteBranchDialog.test.tsx` → `…turns_spaces_typed_in_the_remote_branch_name_into_separators` → expected to fail. The webview cleans the name as it is typed (a space becomes `-`). The native prompt doesn't, and on OK turns the space into `_`. The fix is to clean the field as it is typed with `clean_up_branch_name_on_typing`.

**Rows that are not test-only after all**

- **`ipc:agent_unqueue`, `agent_send_queued_now`, `agent_stop_background`:** no view draws the message queue or the background tasks. Two of the calls are never built and the third has no caller.
- **`point.registerMenuGroup:agents.send`:** the Project view allows only one selection, so a user cannot send two files.
- **`ipc:workspace_attach`:** no user can reach it. The open question drops its Attach button, and the "attach" setting falls back to that question. The Project view does not show attached folders either.

**Left out, already proven:** `module:tasks.ts` is covered by the existing `proofs13_tasks::backend_tasks_cancellable_ones_…`, so it only needs recording. `create.test.ts` is already marked done in `parity.json`.

**Riskiest tests**

1. **`proofs20_remote_branch`:** it fails on purpose. Even after the field fix, the push relies on a second local repository set as `origin`'s push URL, which I have not checked git uses.
2. **`proofs20_mcp_problems`:** it needs hunk B, and it waits on the MCP backend from the test thread while the problems report comes on a timer.
3. **`lets_generate_win_the_new_key_only_where_it_applies`:** it assumes ⌘N reaches Generate for a `.java` file, and that `.py` is not one of Generate's languages.
