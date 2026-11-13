Of the 81 rows, 18 now have a test, 9 are left out, 2 are really "not built", and 52 were not reached. Nothing has been compiled or run, so everything below comes from reading the code.

**Files** (23 tests, 960 lines; all under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/`):

- `tests/proofs3_run.rs` (243 lines, 5 tests)
- `tests/proofs3_windows.rs` (154 lines, 2 tests)
- `tests/proofs3_rename_lsp.rs` (93 lines, 1 test)
- `tests/proofs3_context.rs` (68 lines, 2 tests)
- `debug/tests/proofs3_debug.rs` (202 lines, 4 tests)
- `editor_language/tests/proofs3_editor_keys.rs` (200 lines, 9 tests, one per editor shortcut)

The notes are at `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs3.md`. They give the three `mod` lines with their anchors (`mod proofs_vcs;` in `mod.rs`, `mod proofs_debug;` in `debug.rs`, `use std::path::Path;` in `editor_language.rs`) and the proposed proof text for each row. No new accessors are needed.

**Rows with a test:**

- **Would become done (15):** `run.runConfig`, `run.stopProcess`, `run.reloadConfigs`, `run`, `stop`, `file.openFile`, `file.openInNewWindow`, `project.rename` (the fake server answers the file rename), `debug.toggleMethodBreakpoint`, `debug.toggleFieldWatchpoint`, `debug.hotReload`, `debug.hotRestart`, `ipc:run_stdin`, `ipc:run_close_stdin`, `context:run.cmakeBuild`.
- **Stay partial (3):** `services.addService`, `app.quit` and `context:focusedDirectory`, for the reasons in the next list.
- **No registry id for `run` / `stop`:** neither UI registers them as actions; they are Services toolbar buttons. The tests click the buttons, and I propose "done" on that basis.
- **No path argument for `file.openFile`:** neither UI's handler takes one, so the test answers the file picker instead.
- **Hot reload fixture:** the test writes a user `dart.toml` that replaces the bundled Dart language, so the fake debug adapter counts as a Dart session.

**Behaviours that differ from the webview:**

- **`app.quit` ignores live debug sessions.** The webview asks to terminate a program running under the debugger; natively only Run processes are asked about. The quit test is written to the webview's behaviour and is expected to fail. The fix is in two parts (the process names, then terminating and waiting); both are in the notes as hunks, and part 2 is uncompiled.
- **`focusedDirectory`:** only the Project view sets it; the webview also sets it from the Bookmarks and Commit trees.
- **`services.addService`:** the menu opens at a fixed point near the top instead of under its button, and the picker for a type with several configurations is not built.

**Most likely to fail first:**

- the quit test (expected);
- ⌃O, ⌃I and ⌃⇧↑, if the editor's own text bindings take them before the IDE keymap;
- the Dart fixture;
- `file.openFile` sending a file to the window that has its project, which the recorded gap says does not happen.

**Left out (9):**

- 3 Docker actions, which need the daemon.
- `project.delete`, which uses the system Trash.
- `window.toggleFullScreen`, which needs real full screen.
- `reloadChangedClasses`, `cancelHotSwap` and `ipc:java_resolve_launch`, which need the Java language server.
- `find.usages`: its syntax fallback needs a fake-server error answer plus a language with a syntax tree, and I could not rule out that Full mode without a server reaches the network. The fixture change needed is in the notes.

**Not built (2):**

- `quickSwitch.ts`: `view.quickSwitchScheme` has no handler.
- `lightEditClose`: `light_edit_should_close` has no caller.

**Not reached (52):** 14 `ipc:` rows, 28 `unit:` rows, 7 `module:` rows, 2 `point.` rows and `context:run.updatableRunning`. A few of their gap texts look out of date: `projectColor`, `windows.test.ts` and `menu_refresh` (`file.openRecentProject` is registered now).
