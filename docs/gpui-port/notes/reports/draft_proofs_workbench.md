I wrote 39 tests in 13 new files and edited no existing file. None of them has been compiled or run. Each file needs one `mod proofs_*;` line inside an existing `mod tests`. The notes file lists every line with its anchor, maps each test to its `parity.json` keys, and covers the gaps, the one fix the tests depend on, and the likely first failures: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs_workbench.md`

**Files (approximate line counts)**

- `app-gpui/src/workbench/debug/tests/proofs_debug.rs` (~130, 4 tests)
- `app-gpui/src/workbench/tests/proofs_agents.rs` (~190, 3 tests)
- `app-gpui/src/workbench/tests/proofs_vcs.rs` (~170, 4 tests)
- `app-gpui/src/workbench/vcs_operations/tests/proofs_update.rs` (~20, 1 test)
- `app-gpui/src/workbench/tests/proofs_database.rs` (~70, 2 tests)
- `app-gpui/src/workbench/tests/proofs_services.rs` (~55, 1 test)
- `app-gpui/src/workbench/tests/proofs_tool_windows.rs` (~40, 2 tests)
- `app-gpui/src/workbench/tests/proofs_quit.rs` (~20, 1 test)
- `app-gpui/src/run/segments/tests/proofs_marks.rs` (3 tests)
- `app-gpui/src/find/row_selection/tests/proofs_sections.rs` (1 test)
- `app-gpui/src/platform/keymap/tests/proofs_presets.rs` (~300, 14 tests)
- `app-gpui/src/platform/dispatcher/tests/proofs_keypad.rs` (2 tests)
- `app-gpui/src/platform/actions/tests/proofs_titles.rs` (1 test)

**Rows:** about 34 would become done and 6 partial, if the tests pass.

- **Debugger:** the four session context keys, each read on and off, plus `debug.startConfig`, `forceRunToCursor`, `forceStepInto` and `resetFrame`.
- **Agents:** `regenerateReply`, `forkReply`, `retractMessage`, `forkTask` and `addAgent`, the `agent_fork` and `agent_rollback` IPC rows, and `agentSessionFocus` on and off.
- **Git and diff:** `diff.prevFile`, `diff.showSettings`, and the `vcs_log_users`, `vcs_diff_numstat`, `vcs_remove_worktree` and `vcs_merge_base` IPC rows.
- **Database:** `db_object_ddl` is done; `db_explain` stays partial because the Query Plan tab is not drawn.
- **Services:** `runAll`/`stopAll` and `open-browser` stay partial; `serverPort.ts` is done.
- **Tool windows:** `toolwindow.hideActive` and resize up/down are done.
- **Units:** run segments, row selection, the eclipse, windows and navExtras keymap specs, and action titles. The windows keymap spec stays partial because its primary-modifier helper has no public native counterpart.

**Test fakes extended inside the test files**

- The debug adapter is given restart-frame support for `resetFrame`.
- A second fake agent that can copy and roll back its session, needed for regenerate, branch and retract.

**No accessor needed.** One suggested fix is written out as a code hunk in the notes. It is a hypothesis I could not run: the `app.quit` handler updates every window while it is itself running inside the active window. That window's update probably fails and counts as "may close", so quitting from inside a window would skip its own unsaved-changes question. The fix is to defer the loop with `cx.defer`. `proofs_quit` should fail until it is applied, and it touches the close flow you are changing.

**Where the app differs from the webview, so no test was written**

- `debug.evaluate` is not registered and has no dialog.
- The Git log feeds only one selected commit to its details pane, so `vcs_commits_changes` can never be reached.
- A merge commit's diff shows a "cannot be shown" warning instead of both parents and the result.
- Manual commit (`db_set_manual_commit` and `db_finish_transaction`) is unreachable because the console view that issues it is never mounted.
- The Query History dialog is not available.

**Left out**

- Needs a server or daemon: the database password prompt and the Docker delete/refresh buttons.
- Window open/close, yours: `file.openInNewWindow`.
- Not reached in the time:
  - the workbench module rows: messages, the merge-editor actions, project colour, toolbar dialog, LightEdit, badges, ProjectView and `fs_entry_kind`;
  - the 13 mouse-binding keymap cases;
  - the run target and Java launch rows;
  - the key-claim rows;
  - 20 of the 22 ui rows.

**The 32 e2e specs:** each has a one-line verdict in the notes. They come from reading spec titles against native test names, not a step-by-step comparison. No spec is claimed as fully replayed. `debug.spec` and `search.spec` are the most likely full replays and should be checked first.

**Most likely first failures**

- `proofs_quit`, for the reason above.
- `proofs_titles`, if some manifest titles have no Chinese entry; the failure lists them.
- The capable-agent test, if the agent library sends a request the new fake agent doesn't answer.
- The log user filter, if the authors come back without email addresses.
