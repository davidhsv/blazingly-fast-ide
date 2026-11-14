I integrated the compare tools and Local History, and wrote the Stop test without changing `crates/ide-run`. The whole suite passed five times on the final code, two of those runs while a second suite ran beside them, and clippy is clean. No webview text was invented in the end; two error titles I added are borrowed from other webview paths, so judge them (section 4).

## 1. Suite, clippy, scope

- **Suite on the final code:** 5 × `test result: ok. 6858 passed; 0 failed; 0 ignored`. Two of the five ran as a pair (cargo beside the test binary run through `$S/diff/dfl.py`), and I ran a second pair the same way. `$S/dfl.py` did not exist, so I wrote `$S/diff/dfl.py`.
- **Earlier flake:** in a parallel pair before the last edits, both binaries failed `put_label_names_…` (left `"Saved"`, right `"Before refactoring"`). The fix is in section 4. Four parallel runs and one foreground run have passed since.
- **Clippy:** `cargo clippy --all-targets -- -D warnings` exits 0.
- **Compile errors:** Part 0 had none. Part 1 had 7 on the first build: 2 removed-constant/pattern errors from my own edits, 2 `Branch` type mismatches (`ide_vcs::Branch` against `vcs::branches::Branch`), 2 `PickDirectory` patterns missing the field `picker`, and 1 capture/borrow error in `render_row`. Clippy then found 1 needless lifetime.
- **Scope:** `rustfmt` ran only on files I edited (`revision_chooser.rs`, `diff_tools.rs`, `project_view_actions.rs`), and `--check` first showed it would touch nothing else. I briefly wrote and deleted `/tmp/x` (outside the scratch folder). Nothing under `crates/`, `src/`, `src-tauri/` or `docs/` changed, and I touched no undeclared draft. `$S/registered_ids.txt` holds 582 ids, all 12 of mine among them.

## 2. Part 0: Stop on a process that ignores the interrupt

- **Backend** (`crates/ide-run/src/process.rs`): the first Stop sends SIGINT and then waits with no time limit (`:732-737`). A Stop while it is already stopping sends SIGKILL to the whole process group (`:703-707`, `kill_tree` `:995-1008`). The backend never escalates on its own. The comments at `:23` (`STOP_GRACE`) and `:686` ("SIGKILL after a timeout") are out of date.
- **Webview:** the button becomes "Kill process" while stopping (`RunWidget.tsx:170-180`); the tooltip reads "Stopping… (click to kill)" (`RunView.tsx:203`); the store says the first Stop waits and a second one kills (`store.ts:777`, `:1173-1175`).
- **Native app:** already does the same, so I made no product change. The pipe spawns (`process.rs:419`, `:822`) would need `pre_exec` (unsafe) to reset signal dispositions, so I did not add it. The PTY path already resets SIGINT and the other stop signals (`portable-pty-0.9.0/src/unix.rs:238-251`).
- **Test:** `workbench::tests::proofs3_run::a_second_stop_kills_a_process_that_ignores_the_interrupt`.
  - It runs `trap '' INT; …tick loop` as a run configuration and clicks `run-stop`.
  - After the first click: status is Stopping, the tooltip is "Stopping… (click to kill)", 5 more ticks print, and the process group is alive (`/bin/kill -0 -- -pgid`).
  - After the second click: the session ends as stopped and the group is gone.
- **Measured:** the test also passes when the test binary itself starts with SIGINT ignored (python `SIG_IGN` then exec), which is the inherited case from the brief.

## 3. Part 1: what is wired

Every test is under `workbench::diff_tools::tests::`. The native code calls the same `ide_host::ops` function as each Tauri adapter.

| Action                                                                       | Test                                                                                                                                            | IPC                                                                 | Status                                           |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------ |
| `diff.compareFiles`, `diff.applyLeftSide`, `diff.applyRightSide`             | `compare_files_shows_both_files_and_accept_left_side_writes_the_change_into_the_right_one`                                                      | `fs_read_text` / `fs_probe`; the apply writes through the documents | done / partial / partial                         |
| `diff.compareWith`                                                           | `compare_with_asks_for_the_other_file_and_shows_both`                                                                                           | `fs_entry_kind`                                                     | partial: no folders                              |
| `diff.compareWithClipboard`, `diff.compareTexts`                             | `compare_with_clipboard_compares_the_file_or_the_selection_and_compare_texts_its_argument`                                                      | none                                                                | done                                             |
| `vcs.localHistory`                                                           | `show_history_diffs_a_saved_revision_and_revert_writes_it_back_once_confirmed`                                                                  | `history_list_dir`, `history_content`, `history_restore`            | done                                             |
| `localHistory.putLabel`, `project.showHistory`, `localHistory.recentChanges` | `put_label_names_the_projects_state_and_shows_in_the_histories_and_recent_changes`; key: `the_recent_changes_key_runs_only_with_a_project_open` | `history_put_label`, `history_list_dir`                             | done / done / partial (no time column)           |
| `vcs.showHistory`                                                            | `show_history_for_file_filters_the_git_log_to_the_file`                                                                                         | `vcs_log`                                                           | partial: filters the main log, no tab of its own |
| `vcs.compareWithBranch`                                                      | `compare_with_branch_diffs_the_file_against_the_branch_chosen`                                                                                  | `vcs_compare_scope`, `vcs_branches`, `vcs_tags`, `vcs_file_at`      | partial: file only                               |

- **Context keys:**
  - `diffEditorFocus`: on in the diff window, off in the editor.
  - `projectOpen`: on with a project, off in an empty window.
  - `vcsAvailable`: on in the Git tests, off in the Restricted Mode test (a project without Git).
- **Shortcuts:**
  - ⌃⌘→: in the diff window it runs Accept Left Side and the file is written; in the editor it does nothing.
  - ⌃⌘←: in the diff window it runs Accept Right Side; in the editor it does nothing.
  - ⌥⇧C: runs Recent Changes with a project open; does nothing in an empty window.
- **Restricted Mode:** the desktop's command guard (`trust.rs:206-409`) allows every one of these commands: the `history_`/`fs_` prefixes, and `vcs_*` by name. So no command exists for the required refusal test, and I added no native guard; whether to call `authorize_command` anyway is your call. `local_history_and_the_compare_tools_work_in_restricted_mode` asserts `authorize_command` accepts the 11 names and refuses `run_start`, then puts a label and reads the history in an untrusted window.
- **Revert from Local History:** the test checks the file and the unsaved editor ("xyzone\n") before, one "Revert" question with the buttons Revert and Cancel, Escape leaving both unchanged, and no second question. Confirming writes "xone\n" to the file and the editor ends saved with it, as the webview does through the same backend restore.
- **Git gate:** Revert now holds `git_command_starts`/`git_command_ended` around the restore, then logs to the console and refreshes, as the webview's `runOp` does. The reads (`vcs_compare_scope`, `vcs_branches`, `vcs_tags`) follow `branches_of` and do not hold the gate. Hypothesis: none of them touches the index (from reading `compare.rs:62`, `refs.rs:91`, `ops.rs:458`); a run with a hanging Git command in the window would confirm it.
- **Literal row lists (before → after):**
  - `ROOT`: 11 → 14, adding Local History, Compare With… and Compare with Clipboard.
  - `GIT`: 11 → 13, adding Compare with Branch… and Show History.
  - The plugin test's root list and the tab menus in `the_tab_menu_acts_on_the_tab_it_was_opened_on` and `a_tabs_menu_closes_pins_and_splits`: each adds Local History, Compare With… and Compare with Clipboard.
  - `GO_TO` and `missing` are unchanged.

## 4. Strings, product fixes, test changes, gaps

- **Strings that were not the webview's:**
  - "Compare Directories is not available" and "Comparing folders with a branch is not available": removed. The action now does nothing on a folder (logged), and the row is hidden.
  - "Compare with Branch or Revision failed": now only logged, as the webview's action registry only logs a failure.
  - "Cannot load the diff": now shown in place of the diff, as the webview does. This added `LocalHistoryView::diff_failed`, tested by `a_diff_that_cannot_be_read_shows_why_in_its_place`.
  - "Cannot apply the change" → "Could not save". This is my mapping from the webview's document save error (`document.ts:812`); the webview has no error for this path.
  - The loading "…" → empty, since the webview shows a spinner and the native app has none.
  - "Could not save" and "Cannot read Local History" are the two titles above taken from other webview code paths.
- **Product fixes beyond the notes:**
  - The Project view menu hides Compare With… on a folder unless `diff.compareDirectories` is registered (the webview's `delegated` hides a row whose action is missing).
  - A new `MenuContext.compares_paths` (false in the native app) limits Compare with Branch… to a single file. Proven on the model by `folders_are_offered_no_comparison_the_window_cannot_show` and through the tree's menu in the branch test.
  - ⌘D on two files now goes to `diff.compareFiles`, as `actions.ts:466-478` does.
- **Existing test changed:** `compare_with_shows_two_files_side_by_side_and_asks_for_the_second_of_one` now expects `["project.compareWith","diff.compareWith"]` and waits for the file prompt, as the notes said.
- **The flake:** the backend writes the file, runs the syntax and language-server observers, and only then records the revision (`documents.rs:649-670`, `project.rs:347-349`). The tests waited only for the disk content. Hypothesis: under load the label was put before the save was recorded. A new `saved` helper now waits for the recorded revision, and the assertion prints every entry if it recurs.
- **Notes that turned out false:**
  - "Open In is absent": it is shown, so `ROOT` went 11 → 14, not 10 → 13.
  - "Local History tab not restored" as a gap: the webview's kind is not restorable either.
  - "history_list_dir has no caller": out of date.
  - All anchors were still there. No drafted test was left out.
- **Still unbuilt:** Annotate, Compare Directories, the path comparison of folders against a revision, Show History for Selection, Show History in a tab of its own, the time column in Recent Changes, and Accept Left/Right in Show Diff.

Everything is in the worktree uncommitted (12 modified files); my logs are in `$S/diff/`.
