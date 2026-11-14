I wrote 9 new test files with 11 tests in total. None of it was compiled or run; one test is expected to fail until a fix lands. The notes, with the `mod` lines, proof texts and hunks, are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs9.md`.

**Files** (paths under `app-gpui/src/`):

- `editor/intelligence/tests/`:
  - `proofs9_resolve.rs` (41 lines, 1 test)
  - `proofs9_syntax_usages.rs` (150 lines, 2 tests)
  - `proofs9_cancel_task.rs` (~96 lines, 1 test)
- `workbench/github/tests/proofs9_github.rs` (72 lines, 1 test)
- `workbench/plugin_tool_windows/tests/proofs9_view_icon.rs` (~25 lines, 1 test)
- `workbench/plugin_runtime/tests/proofs9_status_snapshot.rs` (28 lines, 1 test)
- `workbench/plugins_window/tests/proofs9_settings_plugins.rs` (~41 lines, 1 test)
- `workbench/tests/`:
  - `proofs9_run_chooser.rs` (93 lines, 2 tests)
  - `proofs9_log_users.rs` (~40 lines, 1 test, expected to fail)

The tests need no accessor hunk. The `mod` lines go in `intelligence.rs`, `github.rs`, `plugin_tool_windows.rs`, `plugin_runtime.rs`, `plugins_window.rs` and `workbench/mod.rs`.

**Rows:** 9 would become done and 2 stay partial.

- Done: `ipc:lsp_resolve_completion`, `ipc:syntax_identifiers`, `ipc:syntax_identifiers_in_files`, `ipc:cancel_task`, `ipc:gh_pr_prepare_diff`, `ipc:plugin_assets`, `ipc:plugin_status_items`, `action:plugins.settings`, and `ipc:vcs_log_users` once its fix lands.
- Partial: `settings:plugins`, and `chooserPinned.test.tsx` (the debug-session half of "marks recent" is missing).
- Two more rows can be recorded done with existing tests, no new test needed: `ipc:history_content` and `unit:…/Grid.test.tsx`.

**A, per command:**

- **With a test (8):** the 8 `ipc:` rows above.
- **Not issued natively (8):**
  - `plugin_terminal_session_close`: the cited call site is a different action with a similar name, and nothing in the app calls the plugin host's close.
  - `vcs_commits_changes`: the Git log has a single selection.
  - The five `gh_pr_*` commands and `fs_rename`: re-checked; nothing changed.
- **Issued only in a narrow case, not reached:** `doc_open_virtual`.
- **No native counterpart:** `project_current`.
- **Differ from the webview, no test:** `vcs_restore_head`, `vcs_config_get`.
- **Missing piece not built, nothing to test:** `vcs_diff_with_worktree`, `vcs_log`, `settings_set`, `settings_import`, `plugin_activation_event`.
- **Not reached, recipes in the notes:** `plugin_run_command`, `debug_redefine_classes`.
- **Blocked:** `ssh_set_secret` (SSH; I did not check the secret store).

**B, per group:**

| group                         | covered                         | result                                                  |
| ----------------------------- | ------------------------------- | ------------------------------------------------------- |
| 68 Settings › Plugins         | 1/1                             | written                                                 |
| 32 Run chooser and removal    | chooserPinned's 2 missing cases | removal's 4 cases not examined                          |
| 20 Database results           | 1/2                             | Grid row recorded; `results-view.test.tsx` not examined |
| 26 Hot swap                   | 0/2                             | the code contradicts the group                          |
| 67 Plugin debug preparation   | 0/1                             | the code contradicts the group; not written             |
| 28 Project-tree backend tests | 0/2                             | not examined                                            |

- **Group 26:** both actions build through the Java language server first, and the driver has no test hook for that build. The notes give the hunk that would add one.
- **Group 67:** the preparation is done by the plugin host, not by plugin code. So no plugin fixture is needed, and the notes give the recipe.

**Differs from the webview:** the Git log's User filter reads only the default repository, while the webview reads every repository root. `proofs9_log_users` is written to the webview's behaviour, and the fix hunk for `log_window.rs` is in the notes.

**Most likely to fail first:**

- `proofs9_log_users`, by design. It may also stop earlier if the root scan does not find a nested repository.
- `proofs9_cancel_task`: assumes the background tasks popup lists rows in the same order as the window's task list.
- `proofs9_github`: assumes the changed file's row selector and that a double click on it opens the diff.
- `proofs9_syntax_usages`: I confirmed in the code that a server error comes back as a protocol error, so the syntax fallback should start. Whether the project-wide path passes the error through is still a hypothesis.
