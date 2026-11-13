I wrote 11 tests in 5 new files (481 lines), and the notes file. Nothing was compiled or run, and no existing file was edited. The three `mod` lines to add, with their anchor lines, are at the top of the notes.

**Files:**

- `app-gpui/src/workbench/tests/proofs4_run.rs` (182 lines, 3 tests)
- `app-gpui/src/workbench/tests/proofs4_shell.rs` (124 lines, 3 tests)
- `app-gpui/src/workbench/tests/proofs4_vcs.rs` (136 lines, 3 tests)
- `app-gpui/src/ui/tooltip/tests/proofs4_tooltip.rs` (19 lines, 1 test)
- `app-gpui/src/vcs_diff/model/tests/proofs4_remote_urls.rs` (20 lines, 1 test)
- Notes: /private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/proofs4.md

No accessor hunks are needed; every accessor and helper used was checked to exist.

**Of the 56 rows:**

- **13 get tests; 10 would become done:** `ipc:run_apply_settings`, `ipc:run_without_derived`, `ipc:fs_entry_kind`, `ipc:run_gradle_jvm_args_script`, `ipc:run_gradle_init_script`, `ipc:term_kill`, `ipc:set_recent_project_groups`, `ipc:menu_refresh`, `ipc:vcs_diff_revisions` and `branches.test.ts` (13/13).
- **3 of those stay partial:**
  - `ipc:vcs_log`: the date preset is tested; the path filter and a typed date range are not built.
  - `ignore.test.ts`: the status refresh is asserted; the tree reload and the refresh after a failure are not.
  - `Tooltip.test.tsx`: 3/4 until the fix below.
- **2 are really "not built":**
  - `ipc:run_resize`: nothing calls `resize_terminal`, so the resize never fires.
  - `ipc:history_content`: `vcs.localHistory` has no handler and nothing builds a Local History diff side.
- **5 have stale gaps:**
  - `menu_refresh`: `file.openRecentProject` is now registered.
  - `branches.test.ts`: the two inputs called unasserted are asserted now.
  - `Grid.test.tsx`: the case only calls the preview function, and the Rust test asserts the same inputs, so it can be marked done 12/12 with no new test.
  - `projectColor.ts`: the Welcome screen and the title bar use it.
  - `windows.test.ts`: as `proofs3.md` reported.
- **2 left out:** `history_list_dir` and `history_restore`. They are only reached through Undo Delete, and the delete first sends the file to the system Trash.
- **5 skipped as settled by `proofs3.md`:** `run_stdin`, `run_close_stdin`, `java_resolve_launch`, `quickSwitch.ts` and `lightEditClose.test.ts`.
- **33 not reached:** 7 modules, 2 points and 24 unit specs, all listed in the notes.

**Behaviour that differs from the webview:** the webview hides a tooltip on a key press and when the window loses focus. GPUI only clears it on mouse move out, mouse down and scroll, and the app adds nothing. The tooltip test therefore expects that behaviour and should fail as things stand. I have no exact fix: GPUI's function that clears a tooltip is not public. The notes give two possible approaches.

**Most likely to fail first:**

1. The tooltip test (expected to fail, above).
2. The Gradle test: the start flow may rebuild the stand-in command, or need a linked build or a JDK.
3. The run-output link test: the click has to land inside the link's text range.
4. The temporary-configuration test: it assumes the chooser can delete a temporary configuration.
5. The terminal test: something besides the panel may still hold the terminal view, so it is never released.
6. The merge-commit test: the format of the rows under Show Changes from Parents.
