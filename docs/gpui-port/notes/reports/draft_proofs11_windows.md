I wrote 20 tests in 10 new files (831 lines) plus the notes file. Nothing has been compiled or run. Groups 1, 3, 4 and 6 are written; for group 5 I wrote the two small spec cases and only mapped findPopup. Group 2 (the diff view cases) was not started and none of the missing findPopup cases were written. I edited no existing file.

**Files** (under `app-gpui/src/` in the worktree):

- `workbench/tests/proofs11_vcs_windows.rs` (91 lines, 3 tests): ignore, the tree reload and the refresh after a local-exclude write and after a failed write.
- `workbench/shelf/tests/proofs11_shelf.rs` (108 lines, 2): Copy as Patch of one file, from the file's context menu; the ⌘D/Ctrl+D shortcut in the Shelf tab and its refusal in an editor.
- `workbench/vcs_operations/tests/proofs11_worktrees.rs` (98 lines, 3): disabled Remove with its tooltip, protected main worktree, two roots grouped once.
- `workbench/services_actions/tests/proofs11_docker.rs` (201 lines, 3): `docker.refresh` and the two toggles, from made-up snapshots.
- `workbench/debug/tests/proofs11_hotswap.rs` (126 lines, 2): reload and cancel through a fake java-debug adapter.
- `workbench/editor_core/tests/proofs11_read_only.rs` (63 lines, 2): binary and too-large files are left alone.
- `workbench/plugin_contributions/tests/proofs11_plugin_debug.rs` (44 lines, 1): the session uses the program the plugin's preparation put in.
- `workbench/project_view_actions/tests/proofs11_import.rs` (37 lines, 1): the local-copy case of `fs.test.ts`.
- `debugger/state/tests/proofs11_execution_state.rs` (32 lines, 1): the `Object.prototype` locals case.
- `platform/keymap/tests/proofs11_host_modifier.rs` (31 lines, 2): the two host-modifier cases with their primary-modifier asserts.

**Rows, if the tests pass:**

- **Done (8):** `ignore.test.ts`, `worktreesPanel.test.tsx`, `docker.refresh`, `docker.toggleStoppedContainers`, `docker.toggleUntaggedImages`, `debug.reloadChangedClasses`, `debug.cancelHotSwap`, `executionState.test.ts`. `keymaps/windows.test.ts` also becomes done once H3 is in.
- **Still partial (4):**
  - `shelf.test.tsx`: Compare with Local and the cross-root Unshelve are still only on the model.
  - `fileAttributes.ts`: the virtual-document case can't be reached without jdtls.
  - `debug.pluginStart`: the Java configuration-chooser attach is not covered.
  - `fs.test.ts`: three of its four cases need a remote window.

**Hunks the tests depend on** (exact code in the notes):

- **H1**, `workbench/services_window.rs`: two test-only accessors for the Services state and its Docker store. The 3 Docker tests need it.
- **H2**, `debugger/driver.rs`: a test answer to the hot swap compile, given after a delay, used instead of jdtls. The 2 hot swap tests need it.
- **H3**, `platform/keymap.rs`: a new `has_primary_modifier(modifiers, mac)`. No such host-aware helper exists today, only private copies in a few views. The 2 keymap tests need it.

**Left out:**

- `unityRefresh.test.ts`: the window has no Unity view, and nothing in the window calls `UnityRefresh`.
- The remote cases of `fs.test.ts`.
- The virtual document case of `fileAttributes.ts`.
- Group 2: the notes list which diff cases have no spec citation in the Rust tests, by name only.
- findPopup: a 25-row table matching spec cases to existing tests, read from doc comments only. 8 cases look missing and 1 behaviour (a resizable popup) seems absent from the code.

**Behaviours that differ from the webview:**

- After Add to .git/info/exclude, or after an ignore write that fails, the project tree is not reloaded. Two of the three ignore tests should fail until `Workbench::add_to_ignore` calls `reload_all` on both outcomes; the fix is in the notes.
- The read-only toggle never treats a document as virtual (`virtual_document: false`, `banner: None` in `editor_core.rs`). Binary and too-large files are still left alone, because they open in the hex viewer.
- The two-roots worktrees test may fail if a linked worktree is not found as a second repository root.

The notes cover the mod lines with their anchors, the row for each test, the hunks and the failure risks: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs11_windows.md`
