I've drafted the diff tools and Local History: three new view/workbench files plus the notes file. None of it has been compiled or run. Stages 1 and 2 are built, stage 3 partly, and Annotate and Compare Directories are not.

**Files written**

- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/diff_tools.rs` (1162 lines): action registration, handlers, backend calls, and 7 end-to-end tests.
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/vcs/local_history_view.rs` (925 lines): the Local History tab (revision list, Cmd/Shift multi-select, embedded diff, Revert/Restore) and its models. Built by `LocalHistoryView::new(path, options, cx)`; emits `Load`, `LoadDiff`, `Revert`, `Notify`.
- `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/vcs/revision_chooser.rs` (530 lines): the branch/tag/revision chooser with filter-as-you-type. Built by `RevisionChooser::new(title, cx)`; emits `Chosen` or `Cancelled`.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/diff_tools_integration.md` (326 lines): the hunks, assumptions, risks and the action → test → IPC → status table.

**Actions registered → end-to-end test → IPC**

| Action                                                                       | Test                                                                                                                                                | IPC                                                               | Status                   |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------- | ------------------------ |
| `diff.compareFiles`, `diff.applyLeftSide`, `diff.applyRightSide`             | `compare_files_shows_both_files_and_accept_left_side_writes_the_change_into_the_right_one`; also checks both keys do nothing from the editor        | diff loads only; the apply writes through the project's documents | done / partial / partial |
| `diff.compareWith`                                                           | `compare_with_asks_for_the_other_file_and_shows_both`                                                                                               | `fs_entry_kind`                                                   | partial                  |
| `diff.compareWithClipboard`, `diff.compareTexts`                             | `compare_with_clipboard_compares_the_file_or_the_selection_and_compare_texts_its_argument`                                                          | —                                                                 | done                     |
| `vcs.localHistory`                                                           | `show_history_diffs_a_saved_revision_and_revert_writes_it_back_once_confirmed`                                                                      | `history_list_dir`, `history_content`, `history_restore`          | done                     |
| `localHistory.putLabel`, `project.showHistory`, `localHistory.recentChanges` | `put_label_names_the_projects_state_and_shows_in_the_histories_and_recent_changes`; for ⌥⇧C, `the_recent_changes_key_runs_only_with_a_project_open` | `history_put_label`, `history_list_dir`                           | done / done / partial    |
| `vcs.showHistory`                                                            | `show_history_for_file_filters_the_git_log_to_the_file`                                                                                             | `vcs_log` with a path filter                                      | partial                  |
| `vcs.compareWithBranch`                                                      | `compare_with_branch_diffs_the_file_against_the_branch_chosen`                                                                                      | `vcs_compare_scope`, `vcs_branches`, `vcs_tags`, `vcs_file_at`    | partial                  |

The revert test asserts a single "Revert" confirmation. Escape leaves both the file and the unsaved editor unchanged; confirming writes the revision to disk and the editor ends up saved with it. That matches what the webview does.

The brief said `history_*` had no native caller. That's out of date: `history_content` is already called in `diff_window.rs`, and `history_list_dir`/`history_restore` in `project_files.rs`. Only `history_put_label` had no caller.

**Left out**

- **`diff.compareDirectories`:** the backend exists but there's no native view. `diff.compareWith` on a folder says "Compare Directories is not available" — I made that text up.
- **`vcs.annotate`:** the editor gutter has no seam for an extra column, and I did not draft the `view.rs` hunks the assignment asked for. That part is not delivered.
- **`vcs.showHistoryForSelection`, `vcs.log.openExternal`:** their views aren't built.
- **Compare with Branch on a folder or several paths:** there's no comparison view. It says "Comparing folders with a branch is not available", also made up.
- **Behaviour that differs from the webview:**
  - Show History for File filters the main Git log instead of opening its own tab.
  - Accept Left/Right only works in diffs opened by the compare actions.
  - The Local History tab isn't restored with the session.
  - Recent Changes has no time column.

**Hunks needed in compiled files** (exact text in the notes)

- **Module lines:** `pub mod local_history_view;` and `pub mod revision_chooser;` in `vcs/mod.rs`. In `workbench/mod.rs`, `mod diff_tools;` and a call to `diff_tools::register_actions(registry);`.
- **`tab_views.rs`:** one new entry in the tab-kind list (`TAB_KINDS`) for Local History.
- **`diff_window.rs`:** make `load_text` and `open_texts` `pub(super)`, and forward the viewer's Apply event to `apply_diff_edit`.
- **`project_view_actions.rs`** (another agent's file, so coordinate first): one test now also sees `diff.compareWith` run and must wait for the file prompt.
- **`context_menus.rs` tests:** the expected row lists `ROOT` (10 → 13) and `GIT` (11 → 13), with before/after.

**Check these first**

1. **Accept key:** the diff viewer may swallow ⌃⌘← itself, which would make that half of the compare-files test fail.
2. **Revision order:** the revert test assumes each save through the app records exactly one "Saved" revision and nothing else in between.
3. **Method names:** I added about 20 methods to `Workbench` and grepped that none existed. Re-check after merging other agents' drafts.
4. **`project.showHistory` collision:** the agent editing `project_view_actions.rs` may register it too. It must be registered once.
