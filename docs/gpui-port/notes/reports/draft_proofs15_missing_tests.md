I wrote 13 new test files (639 lines): 12 tests to compile and run now, and 4 more in three `*_pending.rs` files that expect behaviour the app doesn't have yet. Nothing was compiled or run, and no existing file was touched. If they all pass, 7 of the 33 rows become done and 16 more get closer but stay partial.

Notes, with the `mod` lines and their anchor lines: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs15_missing_tests.md`

**Files** (all under `app-gpui/src/workbench/`)

- `editor_extras/tests/`: `proofs15_folding.rs` (62), `proofs15_paste_history.rs` (91, 2 tests), `proofs15_reformat.rs` (51); pending: `proofs15_folding_pending.rs` (69, 2 tests), `proofs15_paste_history_pending.rs` (45)
- `editor_language/tests/`: `proofs15_implement.rs` (50); pending: `proofs15_implement_pending.rs` (19)
- `vcs_operations/tests/proofs15_reload_all.rs` (35)
- `github/tests/proofs15_github_refresh.rs` (48)
- `plugins_window/tests/proofs15_plugins.rs` (36)
- `terminal_actions/tests/proofs15_terminal.rs` (75, 2 tests). This is the first child module under that `mod tests`, so its `mod` line goes after the last `use` inside it.
- `tests/proofs15_full_screen.rs` (24), `tests/proofs15_merge_resolve.rs` (34)

**Rows that become done:** `action:file.reloadAllFromDisk`, `action:plugins.reloadAll`, `action:terminal.splitAndMoveRight`, `action:window.toggleFullScreen`, `ipc:term_is_busy` (both call sites quoted in the notes), `point.registerTitleItem:vcs.mergeRebase`, and `action:plugins.installFromDisk`.

**Rows that move but stay partial:** the nine `code.*` fold rows, `code.implementMethods`, `code.reformatDialog`, `editor.pasteHistory.paste` / `.remove`, `github.pullRequest.refresh`, and the `editor.pasteHistory` overlay.

**Recorded gaps that are out of date:**

- **Install from disk:** installing a package file is already proven by `workbench::plugins_window::tests::proofs6_plugins::installing_a_package_file_from_disk_unpacks_it_and_installs_its_plugin`. The row stays partial only for the picker opened without an argument, which no test can drive.
- **Full screen:** the test window GPUI uses does keep a full-screen flag (read in its source), so the "no full screen on the test platform" note is wrong.
- **Paste from History mouse:** clicks and double clicks are handled, contrary to "the mouse does nothing".

**Hunks the tests depend on:** none of the 12 runnable tests needs one. One pending test needs a test accessor `Editor::painted_fold_markers` in `editor/view.rs`; the notes describe it, and I left it to you because the other agent is editing that file.

**Behaviour that differs from the webview** (each pending test is expected to fail until fixed):

- Settings › Code Folding's fold by default (file header, imports, doc comments) is not applied when a file opens.
- Implement, Generate and Override don't re-indent the lines the server inserted. The helpers for it already exist in `editor_lsp/intentions.rs` but nothing calls them.
- Paste from History hard-codes Enter, Delete and Backspace instead of following the keymap, so rebinding its actions has no effect.
- Reformat File… always offers Optimize imports; the webview leaves it out when a plugin's formatter formats the file. No test, because it needs a plugin formatter fixture.
- Refresh Pull Request doesn't reload the timeline or review threads. No test, because the timeline fixture wasn't built.
- `run.updatableRunning` ignores debug sessions. As a hypothesis to confirm, `run.updateApplication` may have no handler: grep finds it only in `menu_spec.rs`.

**Left out:**

- `context:run.updatableRunning` and `ipc:java_resolve_launch`: both need a real Tomcat or the Java language server, and the editor's fake server only answers `*.mock` files.
- `ipc:plugin_grant`, `ipc:plugin_list`, `ipc:plugin_run_command`: I only read the code for these, no tests.
- Not reached: points `pluginLog`, `agents.send`, the `plugins` menu group, `search.bookmarks`, `plugins.consent`, and the `run` tool window.

**Most likely to fail on first run:**

- **`proofs15_reload_all`:** it only tells the action apart from the file watcher if the repository read itself needs no executor timer. The watcher's re-read does wait on one, which only fires when the test clock moves. The test uses its own polling loop that never moves the clock.
- **`proofs15_terminal` (busy shell):** it relies on `set -m` making `sleep` the foreground process group inside the plain test shell.
- **`proofs15_reformat`:** the editor's lightbulb may send `codeAction` requests of its own. The test checks only the organize-imports request.
- **`proofs15_paste_history`:** the order of the chosen rows after ⌘-click and ⇧-click is assumed.
