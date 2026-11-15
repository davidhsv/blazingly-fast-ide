Both parts are applied and every gate passed, but the "clean tree at `e7390d85`" was no longer true. Partway through, someone else committed `a26801f5` (docs only) on top of it. Its `parity.json` and `INVENTORY.md` were already in the tree before I edited anything, and both still match the checksums I took at 17:11. Nothing is committed.

## 1. Gates (from `app-gpui/`)

| Gate                                        | Result                                                                                                                     |
| ------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Suite run 1 (nothing else running)          | `test result: ok. 7603 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 185.41s` (`$S/drv/suite2.txt`) |
| Suite run 2                                 | `test result: ok. 7603 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 216.42s` (`$S/drv/suite3.txt`) |
| Twenty times during run 2                   | 42 tests, each `ok` 20/20; `grep -c FAILED` over all 40 logs prints `0`                                                    |
| `cargo test --test web_view`                | `test result: ok. 1 passed; 0 failed; ... finished in 4.23s`                                                               |
| `cargo clippy --all-targets -- -D warnings` | exit 0, `Finished \`dev\` profile`                                                                                         |
| `cargo build`                               | exit 0, `grep -c '^warning'` = 0                                                                                           |

- **Twenty-run loops:** they ran the built test binary, not cargo. The 39 slice tests ran 17:42:36–17:44:06 and the 3 changed menu tests 17:42:39–17:43:27; suite run 2 started about 17:42:30 and its log was written at 17:46:04. So all 20 runs of both loops overlapped the suite.
- **Clippy and build:** re-run last, after the final edit (`$S/drv/clippy3.txt`, `$S/drv/build_final.txt`).

**Failures seen along the way:**

- **First full suite (`$S/drv/suite1.txt`): 7600 passed, 3 failed.** The failing tests were `workbench::context_menus::tests::{a_plugins_editor_menu_entries_appear_under_their_submenu_and_run, right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret, in_a_git_repository_the_editor_menus_git_rows_are_enabled}`.
  - Cause: their expected lists assumed the refactor actions were unregistered. Once they are, the editor menu shows "Refactor" after "Rename…", and Go To shows "Super Method".
  - After I fixed the root list, the git-repository test failed again: Git › now shows "Show History for Selection".
  - Both match the webview (`src/editor/core/controller.ts:777`, `:798-815`, `:1943`).
- **First clippy run: 2 errors.**
  - `needless_lifetimes` at `vcs/selection_history_view.rs:520`.
  - `cloned_ref_to_slice_refs` at `workbench/vcs_text_actions.rs:793`.

Every draft test passed on its first run.

## 2. What I applied, and what I changed beyond the notes

**Part A (Refactor actions).** All hunks from the notes are applied: the `mod` line, the field and its initialiser, the register call, and `apply_code_action` in `editor_language.rs`. Beyond them:

- I removed the draft's private `hint` copy. It now calls `editor_language::hint` (which became `pub(super)`) through a small wrapper. The mouse-shortcut commit changed nothing the draft calls; I read the plain diff of `65d5e6da`.
- **New test** `workbench::refactor_actions::tests::refactor_this_is_not_available_once_the_project_is_closed`. It reads Refactor This's `projectOpen` condition on and then off; no draft test read it off.

**Part B (Git actions on the editor's text).** All hunks are applied: the `pub mod` lines in `vcs/mod.rs` and `vcs_more/mod.rs`, the workbench `mod` line and register call, and the two tab kinds in `tab_views.rs`. Beyond them:

- I kept `let_settle`, a short fixed wait, in its three places (an unchanged file, the diff viewer, a cancelled folder question). Each asserts that nothing happened, and none of them leaves a state to wait on; its comment now says why.
- I fixed the two clippy errors above.

**Other edits.** In `context_menus.rs` I updated the expected root, Go To and Git lists to the webview's, and added a `REFACTOR` check. That check expects Pull Members Up / Push Members Down to be missing, following the file's rule of listing only registered actions; the webview shows them.

**Webview sources checked:**

- `refactorThis.ts:309-317` (Nothing here)
- `navigation/index.ts:~95` ("No super method found" / "No super class found")
- `conflictsUi.tsx:172-208` (Accept at caret and conflict moves, notice "The caret is not inside a conflict")
- `selectionHistoryKind.ts:37-47`
- `RevisionBrowser.tsx:39-53` (error notice "Could not open Git log", body = the error)
- `actions.tsx:430-455, 540-548, 605-640`

Two differences I found there:

- The webview's folder prompt is titled "Open Git Log"; the native one has no title.
- The webview opens all picked folders at once; the native code opens them one after another and stops at the first failure. The user sees the same result.

**Left out (as in the brief and notes):**

- Pull Members Up / Push Members Down.
- Renaming the introduced name in place after an extract.
- The syntax-only Introduce Variable.
- The highlight while a chooser moves.
- The Safe Delete preview, which is reachable but shown as a confirm dialog and untested.
- Popups are not anchored at the caret.
- The external log is a plain commit list.

## 3. `parity.json` keys

| Key                                                                                                                                    | State           | Proving tests (full names)                                                                                                                                                                                                                                                                                                                                                          | Missing for partial                                                                            |
| -------------------------------------------------------------------------------------------------------------------------------------- | --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `action:refactor.extractVariable`, `action:refactor.extractConstant`                                                                   | partial         | `workbench::refactor_actions::tests::apply_the_single_matching_action_or_offer_a_choice`, `…::give_idea_s_caret_reason_the_server_s_disabled_reason_or_the_missing_server`, `…::offer_idea_s_occurrences_choice_when_the_server_has_both_variants` (extractVariable)                                                                                                                | In-place naming, the syntax fallback, choosing the expression first                            |
| `action:refactor.extractMethod`, `action:refactor.extractField`, `action:refactor.extractParameter`                                    | partial         | the first two above                                                                                                                                                                                                                                                                                                                                                                 | In-place naming                                                                                |
| `action:refactor.move`                                                                                                                 | done            | the first two above, plus `…::say_the_server_doesn_t_support_it_when_its_advertised_kinds_can_t_produce_it`                                                                                                                                                                                                                                                                         | —                                                                                              |
| `action:refactor.changeSignature`, `action:refactor.inline`                                                                            | done            | the first two above; inline also `…::offer_idea_s_occurrences_choice_when_the_server_has_both_variants`                                                                                                                                                                                                                                                                             | —                                                                                              |
| `action:refactor.thisMenu`                                                                                                             | partial         | `…::lists_only_the_refactorings_applicable_at_the_caret_in_idea_order`, `…::says_so_when_nothing_applies`, `…::refactor_this_is_not_available_once_the_project_is_closed`                                                                                                                                                                                                           | Fallback and Pull/Push rows hidden; Rename / Safe Delete rows not run from the popup           |
| `action:refactor.safeDelete`                                                                                                           | partial         | `…::confirms_first_delete_function_and_searches_nothing_when_cancelled`, `…::deletes_an_unused_declaration_as_one_undo_step_ignoring_recursive_calls`, `…::asks_when_usages_exist_view_usages_delete_anyway_cancel`                                                                                                                                                                 | Preview, no-server paths, another file's declaration                                           |
| `action:navigate.superMethod`                                                                                                          | partial         | `…::looks_for_the_method_in_supertypes_level_by_level`, `…::opens_the_super_method_in_another_file_or_says_there_is_none`                                                                                                                                                                                                                                                           | One level only; the chooser for several targets is plain and untested                          |
| `action:refactor.pullMembersUp`, `action:refactor.pushMembersDown`                                                                     | none            | —                                                                                                                                                                                                                                                                                                                                                                                   | No model or dialog                                                                             |
| `action:vcs.acceptYours`, `action:vcs.acceptTheirs`, `action:vcs.acceptBoth`                                                           | done            | `workbench::vcs_text_actions::tests::accept_yours_theirs_and_both_at_the_caret_replace_that_conflicts_block_and_leave_the_others`, `…::with_the_caret_outside_a_conflict_accept_changes_nothing_and_says_so` (Yours), `…::accept_yours_during_a_rebase_takes_the_commit_being_replayed` (Yours/Theirs), `…::the_conflict_actions_are_enabled_only_while_an_editor_has_the_keyboard` | —                                                                                              |
| `action:vcs.nextConflict`, `action:vcs.prevConflict`                                                                                   | done            | `…::next_and_previous_conflict_marker_move_the_caret_and_wrap`, the keyboard test above                                                                                                                                                                                                                                                                                             | Scrolling into view is not asserted (`Editor::select` → `set_selections` calls `reveal_caret`) |
| `action:vcs.nextChange`, `action:vcs.prevChange`                                                                                       | done            | `…::next_and_previous_change_move_through_the_editors_changed_ranges_and_wrap`, `…::next_change_moves_between_the_differences_of_the_focused_diff_viewer`, `…::the_change_actions_are_enabled_only_in_a_project_under_version_control`, `…::show_history_for_selection_is_enabled_only_with_a_text_editor_active`                                                                   | —                                                                                              |
| `action:vcs.showHistoryForSelection`                                                                                                   | partial         | `…::show_history_for_selection_lists_the_commits_that_changed_the_selected_lines`, `…::show_history_for_selection_is_enabled_only_with_a_text_editor_active`, `…::the_change_actions_are_enabled_only_in_a_project_under_version_control`                                                                                                                                           | Untested: asking again for the same lines, the virtual-document notice, cancel on close        |
| `action:vcs.log.openExternal`                                                                                                          | partial         | `…::open_external_git_log_shows_the_log_of_the_picked_repository`                                                                                                                                                                                                                                                                                                                   | Commit list only (no graph, paging, filters, details, restore, toolbar entry)                  |
| `point.registerEditorKind:selectionHistory`                                                                                            | partial         | `…::show_history_for_selection_lists_the_commits_that_changed_the_selected_lines`                                                                                                                                                                                                                                                                                                   | Cancel on close untested; no syntax colours                                                    |
| `point.registerEditorKind:git-log`                                                                                                     | partial         | `…::open_external_git_log_shows_the_log_of_the_picked_repository`                                                                                                                                                                                                                                                                                                                   | External logs only                                                                             |
| `unit:src/features/vcs/__tests__/selectionHistory.test.tsx` (6 cases)                                                                  | partial (5/6)   | `vcs::selection_history_view::tests::{selected_lines_of_a_text, cuts_the_block_out_of_the_text_as_git_prints_it, lists_local_changes_only_when_the_block_differs_from_head, asks_for_the_selected_lines_of_the_editors_text_and_diffs_each_commits_block, says_why_there_is_nothing_to_show, shows_gits_error_and_cancels_a_request_still_running_when_closed}`                     | The "cancels when closed" half (the window's release hook is untested)                         |
| `module:src/editor/refactor/refactorThis.ts`, `module:src/editor/refactor/catalog.ts`, `module:src/editor/refactor/serverIntroduce.ts` | partial         | the Part A tests                                                                                                                                                                                                                                                                                                                                                                    | As for thisMenu and extract*                                                                   |
| `module:src/editor/refactor/safeDelete.ts` (already partial)                                                                           | partial         | adds the three Safe Delete tests                                                                                                                                                                                                                                                                                                                                                    | Preview, no-server paths                                                                       |
| `module:src/editor/navigation/structureNav.ts` (already partial)                                                                       | partial         | adds the two Super Method tests                                                                                                                                                                                                                                                                                                                                                     | Several levels, overloads, the chooser                                                         |
| `unit:…/refactorThis.test.ts` (19 cases by my count)                                                                                   | done (models)   | the window tests now prove 7: lists only…, says so…, apply single/choice, doesn't support, caret/disabled reason, never fall back to unrelated actions, occurrences choice. Plus 1 in part: runs the mapped action (Inline row only)                                                                                                                                                | —                                                                                              |
| `unit:…/safeDelete.test.ts` (26)                                                                                                       | done (models)   | the window tests prove 3: deletes unused, asks when usages exist, confirms first                                                                                                                                                                                                                                                                                                    | —                                                                                              |
| `unit:…/structureNav.test.ts` (5)                                                                                                      | done (models)   | the window tests prove 1 case in part: one level, same file and another file                                                                                                                                                                                                                                                                                                        | —                                                                                              |
| `unit:…/serverIntroduce.test.ts` (5), `…/occurrences.test.ts` (4), `…/refactor.test.ts` (18): model-only rows that are now reachable   | stay model-only | No window test asserts their cases (the occurrence pairing is reached through the occurrences-choice test)                                                                                                                                                                                                                                                                          | —                                                                                              |

**`ipc:` rows where the asserted state can only come from that backend function:**

| Key                                  | Native call site                | Webview call site         | Proving test                         |
| ------------------------------------ | ------------------------------- | ------------------------- | ------------------------------------ |
| `ipc:vcs_block_history`              | `vcs_text_actions.rs:321`       | `SelectionHistory.tsx:66` | the Show History rows test           |
| `ipc:vcs_open_external_log`          | `vcs_text_actions.rs:177`       | `RevisionBrowser.tsx:48`  | the external log test                |
| `ipc:lsp_code_action_query`          | `refactor_actions.rs:275, :322` | `refactor/query.ts:39`    | `say_the_server_doesn_t_support_it…` |
| `ipc:vcs_line_hunks` (stays partial) | `vcs_text_actions.rs:255`       | `gutter.ts:193`           | Next / Previous Change caret moves   |

- **Adapters:** each Tauri adapter (`src-tauri/src/commands/vcs.rs:38, 57, 113`; `lsp.rs:209`) calls the same `ide_host::ops` function with the same arguments.
- **Why `say_the_server_doesn_t_support_it…` proves `lsp_code_action_query`:** "doesn't support Move" can only come from `CodeActionQuery.advertised_kinds`.
- **Already done:** `ipc:lsp_type_hierarchy`. It was proven only on subtypes; the Super Method tests are the first to ask for supertypes.

## 4. What was stale or false in the notes and the brief

- The refactor draft has 12 tests, not 13.
- The inventory says `refactorThis.test.ts` has 17 cases; I count 19 `it(` cases.
- The notes say "run `cargo fmt`", which the rules forbid; I used `$S/fmt.sh`.
- The notes say the drafts are "not compiled yet", but both compiled with no change.
- The notes say the brief's commit `e7390d85` is HEAD; it is `a26801f5` now.
- The draft's test comment for the selection history "cancels a request … when closed" claims more than the test checks.
- The spec-path comments in `selection_history_view.rs` follow a convention many declared files share, so I kept them.

## 5. Shots (`$S/insp8/shots/`)

The run went through `$S/insp8/run.sh` with script `rvs`, on a temp project in `$S/drv/shotproj` with `main.rs` committed and then changed in two places. The log had no `fail` or `note` lines.

- **`rvs-01-selection-history-1.png`:** the "History: main.rs:2-4" tab. Rows: Local Changes / Not Committed Yet, "Bump b", "add main". HEAD (`a = 1`, `c = 3`) against Local Changes (`a = 10`, `c = 30`). Nothing is cut.
- **`rvs-02-refactor-this-nothing-here-1.png`:** the "Refactor This" popup with one greyed "Nothing here" row, inside the window, opened from the Project view. The row's highlight covers only the label's width, not the popup's.

## 6. Files I changed (`git diff --name-only`)

- `app-gpui/src/vcs/mod.rs`
- `app-gpui/src/vcs/selection_history_view.rs`
- `app-gpui/src/vcs_more/mod.rs`
- `app-gpui/src/workbench/context_menus.rs`
- `app-gpui/src/workbench/editor_language.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/refactor_actions.rs`
- `app-gpui/src/workbench/tab_views.rs`
- `app-gpui/src/workbench/vcs_text_actions.rs`

I created no untracked files in the tree; everything else I made is under `$S/drv/` and `$S/insp8/` (rvs script, log, home, shots).
