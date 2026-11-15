## D110 report: Part A (SIGINT handler) and Part B (tabs in a single-file window) are done, and every gate except the first suite run passed

Both parts are complete and the tree is formatted. One full-suite run had one failure, in a code-intelligence test that neither part touches. It passed in the second suite and 20 of 20 times alone. Nothing is committed.

### 1. Gates (measured)

| Gate                                                | Result                                                 |
| --------------------------------------------------- | ------------------------------------------------------ |
| `cargo clippy --all-targets -- -D warnings`         | exit 0                                                 |
| Suite run 1 (`cargo test --lib`)                    | **FAILED. 7252 passed; 1 failed; 0 ignored**, 163.48 s |
| Suite run 2                                         | ok. 7253 passed; 0 failed; 0 ignored, 161.98 s         |
| Twenty runs of every test I added, moved or touched | 20 of 20 "ok. 22 passed; 0 failed"                     |
| `cargo test --test web_view`                        | ok. 1 passed                                           |
| `cargo build`                                       | exit 0, 0 warnings                                     |
| `cargo check --release`                             | exit 0, 0 warnings                                     |

- **The suite-1 failure:** `editor::intelligence::tests::proofs_editor_lsp::a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls`. Message: "the problems did not happen within thirty seconds (ran out after 40.6 s; reached later: None)". It opens a project window, which reaches my change only through the unchanged `Content::Project` branches. Hypothesis: the fake language server's diagnostics arrived late under suite load. To confirm, loop it beside a running suite.
- **The twenty-times filter (22 tests):** `workbench::tests::proofs17_interrupt`, `workbench::tests::proofs13_windows::`, `workbench::light_edit_tabs`, `the_status_and_title_bars_name_the_viewers_file_and_leave_out_the_editors_items` and `the_editors_menu_without_a_project_opens_on_the_text_and_copies`.
- **How each run was started:** every cargo run went through the Bash tool, in the foreground or its background mode, never with `&` in a script. The gates were run in order by `$S/d110/gates/run.sh`, with nothing else running beside them; outputs are in `$S/d110/gates/`.
- **The one exception:** the app was started with `&` inside `$S/d110/insp/run.sh`, on purpose, so that it had SIGINT ignored.
- **Failures earlier, during the steps:**
  - Steps 1–4: the draft's `light_edit_tabs` tests failed while the steps they need were not applied yet (7, 7, 4 and 2 failures), as planned.
  - After step 6: `light_edit_tabs::tests::closing_the_last_tab_leaves_no_tabs_to_reopen` failed in the suite and every time alone. That was a real bug, fixed below.
- **Formatting:** only the files in `$S/d110/fmt_list.txt`, through `fmt.sh` (that is `git diff --name-only` plus `proofs17_interrupt.rs`). Only `app_shell.rs`, `mod.rs` and `proofs17_interrupt.rs` changed.
  - That list also picked up the database drafts another agent is editing. `fmt.sh` changed none of them; their diff is not mine.
- **Other checks:** `git status --short src/ src-tauri/ e2e/ crates/ docs/` prints nothing. There is no new `unsafe`, no new dependency and no ignored test.

### 2. Parts

#### Part A: the app's own interrupt handler

- **Files:**
  - `app-gpui/Cargo.toml`: the `signal` feature on the existing `tokio`. `Cargo.lock` is unchanged (`git diff --stat` is empty), because tokio's `signal` feature was already enabled by another crate in the graph.
  - `app-gpui/src/workbench/app_shell.rs`: `quit_on_interrupt` and `quit_on_signal`. The signal is registered on the backend's tokio runtime before `init` returns, so a program started right after already gets the default disposition. Each SIGINT runs the same `quit()` as File › Exit, asked from the active window: Confirm Exit, then each window's close guards. With no window left the app just quits.
  - `app-gpui/src/workbench/mod.rs`: the re-export.
  - `app-gpui/src/lib.rs`: `init` installs the handler; a failure is logged.
- **Where the webview stands:** it has no equivalent. The disposition follows `crates/ide-run/src/process.rs:686-741`: the first Stop sends SIGINT to the process group.
- **How the test gets a process with SIGINT ignored:**
  - The test re-runs itself (`current_exe --exact <name>`) under `/bin/sh -c "trap '' INT; exec …"` with `OPENIDEA_TEST_SIGINT_IGNORED=1`, and asserts that the inner run passed with "1 passed".
  - Inside, before the handler is installed, the test proves the ignored state: a child `sh -c 'kill -INT $$; echo survived'` survives. After installing, a fresh child is killed by the signal.
- **Tests** (`app-gpui/src/workbench/tests/proofs17_interrupt.rs`):
  - `workbench::tests::proofs17_interrupt::stop_ends_a_shell_script_run_by_an_ide_started_with_interrupts_ignored`: `/bin/sh serve.sh` (echo, then `sleep 60`) runs through `run.run` and is stopped by `run.stop`.
    - **Negative control, measured:** without the handler, "the script to end did not happen within thirty seconds (… reached later: Some(60.0 s))".
  - `workbench::tests::proofs17_interrupt::an_interrupt_asks_to_exit_and_about_unsaved_changes_like_quit`: `kill -INT <own pid>` leads to "Confirm Exit", then the "Unsaved Changes" question; Cancel keeps the window and the text.
- **In the real binary (measured):** started with `&` (SIGINT ignored), then `kill -INT <pid>`. The app stayed alive and showed Confirm Exit (`$S/d110/insp/shots/tabs1-sigint-1.png`), so `init` installs the handler.

#### Part B: tabs in a single-file window

I applied it in six steps, each compiled and followed by a full suite. The scripts are `$S/d110/steps/step1.py` to `step6.py`.

1. The struct variant, every or-pattern from the notes, the `light_pending` field, the empty-pane hints, and the module declared.
2. The pane draws the window: `editor_area`. `light_find_bar` and `light_editor_element` are removed, and `editorTabsOpen` counts real tabs.
3. Routing a second file: `open_file`, `light_edit_opened`, `queue_light_tabs`, `take_loose_files`, and `route_files` handing files to the existing window. `window_showing` now uses `light_edit_shows`.
4. Close Tab and the window's close: `closed_tab_settled`, `close_emptied_light_edit`, and `close_light_edit_tab` counting open tabs.
5. The session: `save_session` goes to `save_light_session`.
6. A failed tab is reported: `report_light_tab_failure`.

The `Content::Editor` sites were re-checked with grep against the current tree. Extra `Content::Project` sites that the notes do not list were deliberately left project-only, as before:

- `code_insight.rs` `open_doc_of`
- `todo_window.rs` (two sites)
- `project_view_options.rs`
- `project_files.rs` `files_moved`

**Files:** `workbench/{mod.rs, app_shell.rs, chrome.rs, editor_core.rs, editor_find.rs, editor_intelligence.rs, empty_editor.rs, project_files.rs, tab_views.rs, light_edit_tabs.rs, tests/proofs13_windows.rs}`.

**App fix outside the notes (`workbench/mod.rs`, the `on_release` of `Workbench::new`):**

- **The bug (measured):** a window's label is free as soon as the window is removed, but its workbench can be released later. Its release then closed the project of the new window that had taken the same label. Diagnostic: the next light window showed "Failed: no project is open in this window".
- **The fix:** the release no longer closes the project when a live workbench of another window has that label.
- **Proof:** the test failed every time before (15 runs alone) and passed 20 of 20 after.
- There is no webview counterpart: each webview window is its own process.

**Gaps the writer named, settled:**

- **Failed tab:** the webview notifies through `openFailed` (`src/features/app/windows.ts:43-50, 72-83`), so it is now done: a `Content::Editor` tab that fails to open raises "Cannot open <name>". A restored-session tab whose file is gone is dropped quietly, as `src/editor/core/controller.ts:589` does.
  - New test: `workbench::tests::proofs13_windows::a_loose_file_that_cannot_be_opened_as_a_tab_is_reported` (an unreadable file).
  - Its negative control, measured: without the notice, "the failure to be reported did not happen".
- **A window whose first file opens in a viewer (an image):** it still takes no tabs, as the draft left it. I did not change this.

**Webview sources followed:**

- `src/features/app/windows.ts:86-96`: `openFilesHere`.
- `src/features/app/windows.ts:577-583`: the window closes with its last tab.
- `src/workbench/EditorArea.tsx:19-23`: the two hints.
- `src/editor/core/controller.ts:135, :332-333`: the `<lightEdit>` session.
- `windows.test.ts:207` and `:231`: cases 8 and 11.

**Tests:**

- Moved into `app-gpui/src/workbench/tests/proofs13_windows.rs` (the `_pending` file is deleted):
  - `workbench::tests::proofs13_windows::turns_a_window_without_a_project_into_light_edit_when_no_light_edit_window_exists`
  - `workbench::tests::proofs13_windows::adds_tabs_to_this_light_edit_window`
- Added there: `…::a_loose_file_that_cannot_be_opened_as_a_tab_is_reported`.
- The draft's eight in `app-gpui/src/workbench/light_edit_tabs.rs`, all passing: `closes_when_its_last_editor_tab_closes`, `close_other_tabs_keeps_the_active_tab`, `the_title_and_the_status_bar_follow_the_active_tab`, `leaves_files_to_the_existing_light_edit_window`, `the_session_restores_its_tabs`, `closing_the_last_tab_saves_its_changes_before_the_window_closes`, `closing_the_last_tab_leaves_no_tabs_to_reopen`, `an_empty_light_edit_pane_offers_open_file_and_dropping`.
- `context_menus::tests::proofs16_menus::the_editors_menu_without_a_project_opens_on_the_text_and_copies` passes unchanged.

**Screenshots** (my own window only, temp `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR`, stopped by PID, Services never opened; no `fail` or `note` lines and no error in the app's output):

- `$S/d110/insp/shots/tabs1-two-tabs-1.png`: `a.txt` and `b.md` tabs, the title "b.md", Markdown split view.
- `$S/d110/insp/shots/tabs2-tab-menu-1.png`: the tab menu (Close, Close Other Tabs, …, Configure Editor Tabs…).
- `$S/d110/insp/shots/tabs2-after-close-1.png`: one tab left, the title back to "a.txt".
- I did not shoot a project window side by side. The strip is the same `pane_element` code that draws a project window's.

**Left out:**

- **Dropping files:** there is no drop handler.
- **The light empty pane end to end:** only its rows are tested. The draft test's doc says "Open File runs its action", but the test checks the rows only.
- **No tabs after a viewer:** a window whose first file shows in a viewer still takes no tabs.

### 3. Inventory rows

| Key                                                                              | State          | Proving tests                                                                                                                                                                           | Cases / missing                                                                            |
| -------------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `unit:src/features/app/__tests__/windows.test.ts`                                | partial        | the two moved tests above, plus `light_edit_tabs::tests::leaves_files_to_the_existing_light_edit_window`                                                                                | 13 of 48 (was 11). Missing: drops, launches beyond the first argument, remote, WSL, Attach |
| `unit:src/features/app/__tests__/lightEditClose.test.ts`                         | done (already) | adds `light_edit_tabs::tests::{closes_when_its_last_editor_tab_closes, closing_the_last_tab_saves_its_changes_before_the_window_closes, closing_the_last_tab_leaves_no_tabs_to_reopen}` | 2 of 2                                                                                     |
| `module:src/workbench/EditorArea.tsx`                                            | partial        | `light_edit_tabs::tests::an_empty_light_edit_pane_offers_open_file_and_dropping` (rows only)                                                                                            | the light hints are not tested through the window                                          |
| `action:editor.closeTab`, `action:editor.closeOtherTabs`, `action:file.openFile` | done (already) | adds `light_edit_tabs::tests::{closes_when_its_last_editor_tab_closes, close_other_tabs_keeps_the_active_tab}` and `proofs13_windows::adds_tabs_to_this_light_edit_window`              | —                                                                                          |

There are no `_pending` tests left from this brief.

### 4. Hypotheses

- The suite-1 failure is load-related; see section 1 for how to confirm.
- A user who closes a single-file window and opens a file at once could hit the label-reuse bug in the real app. This depends on something holding the old workbench past its window, which I did not measure. To confirm, close a single-file window's last tab and immediately open another file, in the binary.

### 5. False or stale notes

- **`clippy::large_enum_variant`:** the notes' B1 says `Content` carries `#[expect(clippy::large_enum_variant)]`. It does not.
- **Incomplete site list:** the notes' table of sites misses the `Content::Project` guards listed in Part B. Their line numbers had drifted.
- **Changes beside mine:**
  - HEAD moved from `664c5e51` to `08f53875` (D111, D112) during my run.
  - `docs/gpui-port/INVENTORY.md` and `inventory.mjs` showed as modified mid-run.
  - Another agent edited the do-not-touch drafts `app-gpui/src/database/{dialog_parts,history_view,import_dialog,modify_table_dialog,plan_view}.rs` and `workbench/database_extras.rs` at 03:26. I touched none of them.

`$S` is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`.
