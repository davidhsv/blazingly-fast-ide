I've written the four files and the notes, but none of it has been compiled or run, since I had no cargo. Eight of the nine actions should be done once integrated; `vcs.log.openExternal` is partial (a commit list, not a full Git log).

## 1. Files written

- `app-gpui/src/vcs_more/caret_conflicts.rs` (129 lines). A model with two functions, `resolve_at_caret` and `conflict_target`. The existing parser in `vcs/conflicts.rs` counts in bytes and the editor counts in UTF-16, so this file converts between them. 5 `#[test]`s.
- `app-gpui/src/vcs/selection_history_view.rs` (665 lines), the History for Selection tab, kind `selectionHistory`.
  - Constructor: `SelectionHistoryView::new(data, DiffViewOptions, cx)`.
  - Event: `SelectionHistoryEvent::Load(Box<SelectionHistoryData>)`.
  - Calls in: `load`, `reload`, `started(RequestId)`, `set_history(RequestId, Result<BlockHistory, String>)`, `select`.
  - Accessors: `data`, `range`, `is_loading`, `error`, `rows`, `empty_message`, `shown`, `active`, `pending_request`, `diff_viewer`.
  - Helpers: `selected_lines`, `block_of`, `history_rows`, plus input, title and icon functions. 3 view tests and 4 plain tests.
- `app-gpui/src/vcs/external_log_view.rs` (369 lines), the external log tab, kind `git-log`.
  - Constructor: `ExternalLogView::new(roots, cx)`.
  - Event: `ExternalLogEvent::Load(Vec<String>)`.
  - Calls in: `load`, `set_commits`, `select`.
  - Accessors: `roots`, `is_loading`, `error`, `rows`. 2 view tests and 1 plain test.
- `app-gpui/src/workbench/vcs_text_actions.rs` (805 lines): `register_actions`, the handlers, `build_selection_history_tab`, `build_external_log_tab`, the two accessors `selection_history_view` and `external_log_view`, and 11 end-to-end tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/vcs_text_actions_integration.md`.
  - `pub mod` lines for `vcs/mod.rs` and `vcs_more/mod.rs`.
  - `mod vcs_text_actions;` and its `register_actions` call in `workbench/mod.rs`.
  - Two `TabKind` entries for `TAB_KINDS` in `tab_views.rs`, inserted after the Local History entry.
  - All edits are insertions after quoted anchors. No `Workbench` field and no menu rows are needed: Show History for Selection is already in both menus.

## 2. Actions registered

All nine go through `registry.handle`, so titles, `when` clauses, icons and shortcuts come from `assets/gen/actions.json` and the generated keymaps.

- **`vcs.acceptYours`, `vcs.acceptTheirs`, `vcs.acceptBoth`** (no IPC beyond the editor's own `doc_edit`):
  - Replace the conflict block at the caret as an unsaved editor edit; the selections move with the edit.
  - During a rebase the sides swap.
  - Outside a block they show the notice "The caret is not inside a conflict".
  - Tests: `accept_yours_theirs_and_both_at_the_caret_replace_that_conflicts_block_and_leave_the_others` (real `git merge` with three conflicts), `with_the_caret_outside_a_conflict_accept_changes_nothing_and_says_so`, `accept_yours_during_a_rebase_takes_the_commit_being_replayed`, `the_conflict_actions_are_enabled_only_while_an_editor_has_the_keyboard` (ON, and OFF once the Project view has the keyboard).
- **`vcs.nextConflict`, `vcs.prevConflict`** (no IPC):
  - Move the caret to the next or previous block and wrap.
  - Do nothing when the file has no conflicts.
  - Tests: `next_and_previous_conflict_marker_move_the_caret_and_wrap` and the ON/OFF test above.
- **`vcs.nextChange`, `vcs.prevChange`** (`vcs_line_hunks`):
  - When a diff viewer or the merge editor has the keyboard, they run `diff.nextDifference` / `diff.prevDifference`.
  - Otherwise they read the changed ranges fresh for each move, unsaved edits included. They then move the caret, wrap, centre it and focus the editor.
  - Tests: `next_and_previous_change_move_through_the_editors_changed_ranges_and_wrap` (wrapping, an unsaved typed line, an unchanged file), `next_change_moves_between_the_differences_of_the_focused_diff_viewer` (next, next, previous), `the_change_actions_are_enabled_only_in_a_project_under_version_control` (OFF), `show_history_for_selection_is_enabled_only_with_a_text_editor_active` (ON).
- **`vcs.showHistoryForSelection`** (`vcs_block_history`, `cancel_request`):
  - Opens the tab with the selected lines and the editor's current text.
  - A virtual document or no editor gives "Open a file to show the history of its selection".
  - The request is cancelled if the tab closes first. Asking again after the text changed reloads the open tab.
  - Tests: `show_history_for_selection_lists_the_commits_that_changed_the_selected_lines` (Local Changes row, then commits, and the diff sides), the two enable tests, and OFF on the view tab.
- **`vcs.log.openExternal`** (`vcs_open_external_log`, `vcs_log`):
  - Asks for folders (several allowed) and opens the log tab of their repositories.
  - A failure shows the error notice "Could not open Git log" with the message as body; cancelling does nothing.
  - Test: `open_external_git_log_shows_the_log_of_the_picked_repository` (a repository, a plain folder, a cancel). It has no `when` clause, so there is no ON/OFF test.

## 3. Spec files

- `selectionHistory.test.tsx`: 6 of 6. Closing the tab to cancel the request belongs to the window and has no unit test.
- `conflicts.test.ts` and the hunk navigation cases of `hunks.test.ts`: already covered by the existing tests in `vcs/conflicts.rs` and `vcs_diff/gutter.rs`. The caret cases (non-ASCII text, wrapping, the rebase swap) are new in `caret_conflicts.rs`.

## 4. Not built, assumptions, likely first failures

- **External log is partial.** It is a commit list (summary, author, time, short id), up to 300 commits per repository. It has no graph, paging, filters, details pane or context menu, and the tab is not restored with the session. The native Git log view only reads the project's own log, and the log-tab models have no view. Only external logs use the `git-log` tab kind; whoever wires the other In Editor log tabs must share it.
- **Not wired:**
  - The Git log toolbar's "External Repository…" entry: its menu event has no handler yet.
  - Syntax colours in the History for Selection diff.
- **Check first:** child modules can see the `pub(super)` helpers in `branches.rs` and the test helpers; `DiffDialog` is `Focusable`; the external log tab's `Load` arrives after the tab's subscription is in place.
- **Tests most likely to fail on first run:**
  - The focused-diff test: the dialog may not publish `diffEditorFocus`.
  - The rebase test: it needs the backend to report the branch operation as "rebase".
  - The selection-history rows: it depends on which commits `git log -L` returns.
  - The plain-folder half of the external-log test: it assumes the temp folder is not inside a Git worktree.
  - The editor-keyboard ON/OFF test: it relies on the Project view taking the keyboard.

## Expected state after integration

- `vcs.acceptYours`: done
- `vcs.acceptTheirs`: done
- `vcs.acceptBoth`: done
- `vcs.nextConflict`: done
- `vcs.prevConflict`: done
- `vcs.nextChange`: done
- `vcs.prevChange`: done
- `vcs.showHistoryForSelection`: done (the diff has no syntax colours)
- `vcs.log.openExternal`: partial (commit list only: no graph, paging, filters, details, context menu or restore, and no toolbar menu entry)
