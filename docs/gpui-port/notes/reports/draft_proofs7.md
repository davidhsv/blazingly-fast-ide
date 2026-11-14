I wrote 15 new test files (824 lines, 28 tests) and one notes file. Nothing was compiled or run. Every runtime claim below is a hypothesis until you compile and run the tests. I created no existing-file edits; the `mod` lines (with anchors) and the hunks are in the notes. I stopped before step 7: groups 20, 26, 28, 67 and 68 were not started.

If everything passes, 39 rows become done, 41 once the GitHub fix below lands. 4 more get closer but stay partial.

| Group                      | Covered / in group       | Notes                                                                                                                                                                                                                                                                                                                                                                                               |
| -------------------------- | ------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1 Bookmark digits          | 18/18                    | Each digit's key from the active keymap, and the registry, asserted inside the loop. The rows are `boundid:` keys; the digit ids are not in `actions.json`.                                                                                                                                                                                                                                         |
| 7 Project view toggles     | 4/4                      |                                                                                                                                                                                                                                                                                                                                                                                                     |
| 3 Spec cases not found     | 4/6                      | Two cases already had tests under other names (`retains_locals_whatever_their_names`, `applies_settings_with_validation_and_custom_keymaps`). New tests cover the Java separator, `relative_dir`, the VS Code preset and dispatch suspended while a shortcut is recorded. `windows.test` stays partial: there is no native `hasPrimaryModifier`. `findPopup`'s 25-case matching was time-boxed out. |
| 10 Context menu groups     | 1 done + 1 partial / 3   | Send to Agent from the editor menu and the Project view, and Add Bookmark on a folder row. The tree only selects one path, so "several paths" can't be tested.                                                                                                                                                                                                                                      |
| 11 Toolbar slots           | 0 done, 2 partial / 3    | `mainToolbar` and `commit.message` buttons are drawn and clicked. The plugin's own icon is product work (sketched in the notes).                                                                                                                                                                                                                                                                    |
| 12 Docker snapshot         | 0/3, skipped             | See the contradicted claims below.                                                                                                                                                                                                                                                                                                                                                                  |
| 13 Extend Selection        | 3/3                      | One step must be exactly the call, which only the backend's syntax ranges give.                                                                                                                                                                                                                                                                                                                     |
| 14 GitHub URLs             | 1/3 now, 3/3 after a fix | The "not open, not listed" test is written to the webview's behaviour and is expected to fail today.                                                                                                                                                                                                                                                                                                |
| 25 Hint at the caret       | 2/2                      | Needs one accessor hunk.                                                                                                                                                                                                                                                                                                                                                                            |
| 29 Quit                    | 2/2                      | The ⌘Q test is macOS-only. `ipc:app_quit` is done only if you accept "every window closes"; the process exit can't be observed in a test.                                                                                                                                                                                                                                                           |
| 30 Read-only toggle        | 2/2                      |                                                                                                                                                                                                                                                                                                                                                                                                     |
| 33 Services Move Up/Down   | 2/2                      |                                                                                                                                                                                                                                                                                                                                                                                                     |
| 32 Run chooser and removal | 0/2                      | Not written; the existing tests that cover some cases are listed in the notes.                                                                                                                                                                                                                                                                                                                      |
| 16, 37                     | skipped                  | 16 is M product work; 37 is already covered by `editor_extras_integration.md`.                                                                                                                                                                                                                                                                                                                      |

**Hunks the tests depend on:**

1. `EditorIntelligence::hint_view(&self) -> &Entity<HintView>` in `app-gpui/src/editor/intelligence.rs`.
2. `Workbench::pull_request_url` should load the pull request's details from `gh` instead of building a github.com URL, as the webview's `prUrl` does. Without it the GitHub test fails, as intended.

**Group claims the code contradicts:**

- **Group 10:** the webview's bookmark rows don't "always run". `runAction` refuses them when `projectOpen` is false too. Only the greyed-out look differs, so this needs an owner decision, not a test.
- **Group 12:** `snapshot_answered` is not a usable seam. Any snapshot needs a connection, and its refresh runs `ide_docker::snapshot`, which looks up and runs `docker`. The missing seam is a replaceable snapshot source in `carry_out_docker`.
- **Group 13:** the ranges come from the backend's syntax tree, so the fake language server isn't needed.
- **Group 3:** two of the "not found" cases already had tests.

**Most likely to fail first:**

- The GitHub URL test (expected; it takes about a minute to report).
- Bookmark keys, if `ctrl-shift-<digit>` arrives in tests as a shifted character.
- Show Excluded, if `.gitignore` isn't honoured without a Git repository.
- Services in the second window, if the `program` type group isn't stored per project.
- The read-only failure test, which makes `set_writable` fail by deleting the file.

Files are in `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src`:

- `workbench/tests/proofs7_bookmark_digits.rs`
- `workbench/tests/proofs7_quit.rs`
- `workbench/project_view_actions/tests/proofs7_tree_options.rs`
- `workbench/project_view_actions/tests/proofs7_project_menu.rs`
- `workbench/context_menus/tests/proofs7_send_to_agent.rs`
- `workbench/context_menus/tests/proofs7_toolbar_slots.rs`
- `workbench/github/tests/proofs7_github_urls.rs`
- `workbench/editor_language/tests/proofs7_hint.rs`
- `workbench/editor_core/tests/proofs7_read_only.rs`
- `workbench/services_actions/tests/proofs7_move.rs`
- `editor/core_actions/tests/proofs7_extend_selection.rs`
- `run/java_launch/tests/proofs7_separator.rs`
- `platform/virtual_docs/tests/proofs7_relative_dir.rs`
- `platform/keymap/tests/proofs7_vscode_preset.rs`
- `platform/input/tests/proofs7_suspend.rs`

Notes: /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs7.md
