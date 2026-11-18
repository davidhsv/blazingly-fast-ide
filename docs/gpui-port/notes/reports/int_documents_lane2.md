## Report: untitled documents draft in `gpui-lane2`

All three pieces landed: untitled buffers with Save As and the Welcome path, the byte order mark, and files changed on disk. Every gate passed. The whole suite was not run, as the brief says; the manager runs it after taking the diff.

### Gates (all measured, from `app-gpui/`)

- **Clippy:** `cargo clippy --all-targets -j 5 -- -D warnings` exits 0, no diagnostics.
- **Build:** `cargo build -j 5` exits 0 with 0 warnings (4m 56s).
- **Added or changed tests, twenty times:** `cargo test --lib -j 5 -- proofs48_ editor_model::untitled:: an_empty_light_edit_pane_offers_new_file` passed 20 of 20, 25 tests each run. No test failed in any run.
- **Test modules of the files I edited, once:**
  - `editor_model::untitled:: editor_model::disk_sync:: workbench::light_edit_tabs:: app_models::close_guard:: workbench::editor_core:: workbench::tab_views:: editor::view::tests:: editor::view::editing_tests::`: 103 passed, 0 failed.
- **Window tests that activate a window, plus the encoding, tab and save tests:** 254 passed, 0 failed. These are:
  - the modules `proofs13_close_guard`, `proofs18_e2e_toolwindows`, `database_extras`, `plugins_window`, `keymap_reference`, `windows`, `app_actions`, `proofs31_settings_feature_pages`, `proofs43_database_results`, `proofs3_run`, `proofs_editor_copy_reference`, `proofs44_attach_trust`, `proofs40_run_start`, `proofs26_code_preview`, `proofs13_shell`, `github`, `proofs13_light_edit_close`, `proofs26_save_all`, `proofs44_editor_tabs`;
  - the 31 tests in `workbench/mod.rs` that call `activate_window` and the status-bar encoding tests, each by name.
- **Not run:**
  - `node docs/gpui-port/gen-data.mjs --check`: I touched nothing it reads, and this worktree has no `node_modules`.
  - The crate gates of int_rules rule 3 (`cargo test -p ide-host`, `cargo clippy -p ide-host`, `export_bindings`): the brief allows cargo only inside `app-gpui/`. The new function compiles and runs through app-gpui's build and tests. `git status --short src/ src-tauri/ docs/ e2e/` prints nothing; under `crates/` only `crates/ide-host/src/project.rs` changed, and only by adding `open_light_empty_in`.
- **Mutations:** each was one script run with the file restored in an EXIT trap and its hash compared afterwards. Every run printed `restored: yes`. The script is `$S/ddoc2/mut.sh`; the logs are in `$S/ddoc2/mut/`.

### Rows

Most of these keys (`action:file.newUntitled`, `ipc:doc_open_untitled`, `ipc:doc_save_as`, `ipc:light_edit_open_untitled`, `ipc:doc_set_bom`, `ipc:doc_sync_from_disk`, `module:src/editor/core/diskSync.ts`, `unit:src/editor/core/untitled.test.ts`) have no entry in `docs/gpui-port/parity.json` at HEAD. Test paths below are `workbench::tests::proofs48_untitled::` (U), `proofs48_save_as::` (S), `proofs48_light_edit_untitled::` (L), `proofs48_bom::` (B) and `proofs48_disk_sync::` (D).

| Key                                                                      | State    | Tests (full names)                                                                                                                                                                                                                                                                                                                                                                        | Mutation that fails them                                                                                                                     | What remains, and why                                                                                                                                                          |
| ------------------------------------------------------------------------ | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:file.newUntitled`                                                | raised   | U `opens_editable_pathless_buffers_with_incrementing_names`, L `new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit`                                                                                                                                                                                                                                                   | tab title back to `rsplit` (U fails); Welcome branch → `return` (L fails)                                                                    | none                                                                                                                                                                           |
| `ipc:doc_open_untitled`                                                  | raised   | U `opens_editable_pathless_buffers_with_incrementing_names`, U `reuses_the_buffer_for_a_re_opened_untitled_name_and_shares_it_across_tabs`, U `reopening_a_closed_untitled_tab_gives_an_empty_buffer_of_that_name` (new)                                                                                                                                                                  | `open_tab_file`'s untitled branch → `if false` fails the reopen test                                                                         | none                                                                                                                                                                           |
| `ipc:doc_save_as`                                                        | raised   | S `autosave_never_writes_an_untitled_buffer_explicit_save_asks_for_a_path_and_retargets_tab_and_model`, S `keeps_the_buffer_open_and_dirty_when_the_save_dialog_is_cancelled`, S `save_as_onto_a_file_open_in_another_tab_is_refused_and_keeps_the_buffer`, S `stays_open_when_an_untitled_buffers_save_as_is_cancelled`, U `asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes` | `retarget_tabs` call removed; a cancelled dialog saves anyway; refusal not notified; `save_cancelled` → `saved(Ok)` (fails both close tests) | the dialog's file-type list: the toolkit call takes no filter (hypothesis 1)                                                                                                   |
| `ipc:light_edit_open_untitled`                                           | raised   | L `new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit`                                                                                                                                                                                                                                                                                                                | Welcome branch off                                                                                                                           | none; rooted at the test's home through `open_light_empty_in`                                                                                                                  |
| `unit:src/editor/core/untitled.test.ts`                                  | 6 of 7   | U `opens_editable…`, S `autosave_never_writes…`, S `keeps_the_buffer_open…`, U `asks_about_a_dirty…`, U `closes_an_untitled_buffer_without_saving_when_the_user_declines`, U `reuses_the_buffer…`                                                                                                                                                                                         | untitled close question off (fails asks, closes, closing-one-of-two and reopen)                                                              | "offers the registered languages as save-as types" cannot be built (no filter parameter)                                                                                       |
| `keymap:vscode-macos`, `module:src/platform/keymaps/vscode-macos.ts`     | raised   | U `the_vs_code_keymaps_open_an_untitled_buffer_with_cmd_n_and_ctrl_n`                                                                                                                                                                                                                                                                                                                     | action handler → no-op                                                                                                                       | none: each row's only Missing clause was this binding                                                                                                                          |
| `keymap:vscode-windows`, `module:src/platform/keymaps/vscode-windows.ts` | raised   | the same test                                                                                                                                                                                                                                                                                                                                                                             | the same mutation                                                                                                                            | none: same check                                                                                                                                                               |
| `ipc:doc_set_bom`                                                        | raised   | B `the_encoding_menu_adds_and_removes_the_utf8_bom_and_the_save_writes_it`, B `with_autosave_on_adding_the_bom_writes_the_file_at_once`, B `the_bom_row_is_only_for_utf8_and_disabled_for_a_read_only_file`                                                                                                                                                                               | no BOM rows; no autosave after the change; row enabled when read-only                                                                        | none                                                                                                                                                                           |
| `point.registerStatusItem:editor.encoding`                               | raised   | the three B tests plus B `reload_with_encoding_is_disabled_for_a_document_that_cannot_be_read_again`                                                                                                                                                                                                                                                                                      | Reload's `!can_reload_encoding()` dropped                                                                                                    | none (the `{name} BOM` label was already native)                                                                                                                               |
| `ipc:doc_set_encoding`                                                   | narrowed | B `reload_with_encoding_is_disabled…` and the BOM tests                                                                                                                                                                                                                                                                                                                                   | as above                                                                                                                                     | "not proven: the re-read of a virtual document (no native code opens a library source, a server's document or a revision as an editor tab…)" — no native code opens such a tab |
| `ipc:doc_sync_from_disk`                                                 | raised   | D `a_file_changed_outside_the_project_is_read_again_when_the_window_is_active_again`, D `a_changed_file_with_unsaved_edits_keeps_them_and_a_warning_says_so`                                                                                                                                                                                                                              | the activation call removed (all three D fail); `sync_on_focus` forced to true (first D fails)                                               | none                                                                                                                                                                           |
| `module:src/editor/core/diskSync.ts`                                     | raised   | `editor_model::disk_sync::tests` (6) plus the 3 D tests                                                                                                                                                                                                                                                                                                                                   | as above, plus `light_edit: true` forced (third D fails)                                                                                     | none                                                                                                                                                                           |
| `unit:src/editor/core/diskSync.test.ts`                                  | 7 of 7   | the 6 model tests plus D `controller_preserves_the_unchanged_documents_selection_and_scroll_then_stops_checking`                                                                                                                                                                                                                                                                          | `light_edit` forced; activation call removed                                                                                                 | none                                                                                                                                                                           |

`closeGuard.test.ts:285` is half covered by S `close_project_counts_an_untitled_buffer_and_stays_when_its_save_as_is_cancelled`; untitled buffers are not carried into another project opened in place. `windowClose.test.ts:118` is covered by S `stays_open…`. Other tests: U `the_session_never_holds_an_untitled_buffer` (fails when the untitled tab kind becomes "file"), U `closing_one_of_two_tabs_of_an_untitled_buffer_asks_nothing` (fails when the "another tab shows it" check is forced to true), and model tests `editor_model::untitled::tests::*` (3).

### Hypotheses and how each was settled

1. **Confirmed, by reading** `gpui-pre-0.3.12/src/app.rs:2045`: `prompt_for_new_path(directory, suggested_name)` takes no file-type list.
2. **False.** `simulate_new_path_selection` panics when no dialog is pending ("no pending new path prompt", `platform/test/platform.rs:284`), and the dialog opens only after backend work. The tests now use `answer_save_dialog`, which waits for `did_prompt_for_new_path()` and also checks the folder the dialog opens in.
3. **Confirmed by D.** The text stays `one two\n` with the setting off, so no watcher saw the file outside the project.
4. **Confirmed.** The D tests pass, and removing the activation call fails all three.
5. **Confirmed.** B read-only passes (`documents.rs:396,432`), and UTF-16LE with its mark is detected.
6. **Not settled.** Hypothesis: the language services follow the new path. Check: Save As to `.rs` with the fake-server fixture.
7. **Confirmed.** S asserts `rust` after Save As.
8. **Not settled.** Hypothesis: the generic file icon in both apps. Natively `file_icon_name` returns `file-text` for a name without a dot (`theme/icons.rs:135`, read); the webview side was not checked.
9. **Confirmed.** The keymap test passes for both `cmd-n` and `ctrl-n`.
10. **Confirmed.** The `await` inside the match arm compiles.
11. **Read, not run.**
12. **Read, not run.** `open_light_in` carries untitled buffers into the next project (`project.rs:733-734`).
13. **Not tested.**

### Differences from the webview

Fixed:

- **When the cleanup runs on Save As.** The webview runs the save cleanup before an untitled buffer's dialog (`document.ts:652-673`, before `isUntitledUri` at `:675`). The draft ran it after the dialog. It now runs first through the new `Editor::clean_up_document`, so a cancelled or refused Save As leaves `draft\n`.
- **Autosave of an untitled buffer.** The webview cleans it on "auto" without writing (`document.ts:675-679`; triggers at `controller.ts:457`, `:534-540`). The draft's autosave skip (hunk 1.5f) prevented that and is removed. The backend's `save` already does nothing for an untitled buffer (`documents.rs:796`).
- **Folder of the Save As dialog.** The draft looked at the active pane first. The webview takes panes in their own order (`controller.ts:1253-1269`), and the native code now does the same.

Not fixed:

- Without a project or a file tab, the webview gives the dialog a bare name; the native dialog needs a folder, so it opens at the home folder.
- Git operations: the webview's `saveAll()` opens Save As for untitled buffers (`controller.ts:2038`, `ops.ts:116-132`). Natively `save_all` only runs the cleanup; no dialog opens.
- The close question's buttons: the webview has Don't Save and Save, with dismissal as cancel (`controller.ts:1624-1631`). Natively it is Save / Don't Save / Cancel, as the existing native close questions are.
- The menu: the webview has File › New › Untitled File (`src-tauri/src/menu.rs:229`); natively it is a flat "New Untitled File" row.
- New Untitled File while a project is opening: the webview still runs `openLightEditUntitled` (`windows.ts:505-506`); natively it is ignored.
- Not built, as the notes list: the tab limit on a new untitled tab, the Markdown and SQL layouts after Save As, carrying untitled buffers into another project, the breadcrumbs of an untitled tab, and the dialog's file-type list.

### Draft tests that failed first, and which side was wrong

- **S `save_as_onto…`** expected `draft`. The test was wrong: the cleanup runs before the backend answers, as in the webview. It now expects `draft\n` and checks the buffer is still unsaved.
- **S `autosave_never_writes…` and L `new_untitled_file…`.** The tests were wrong: they waited only for the file and the clean flag, but the tab is retargeted later, when the answer reaches the window. They now also wait for the tab to become the file's, and L compares canonical paths.
- **After the cleanup fix**, U `closing_one_of_two…`, S `keeps_the_buffer…` and S `stays_open…` changed to `draft\n`. This is the webview's autosave and explicit-save cleanup.
- **Four mutations survived at first:**
  - **The reopen-by-key mutation.** For a live buffer, the backend's `open` already finds the existing key. The new test U `reopening_a_closed…` covers the case that needs the branch: a closed buffer reopened by its key.
  - **The two session skips** (hunks 1.5l and 1.8c). They are redundant, because the session keeps views only of the tabs it saves (`editor/session.rs:147-149`). I deleted both and moved the mutation to the tab kind.
  - **The autosave skip.** I deleted it (see above).
  - **The cancelled-dialog mutation.** The test did not wait long enough. It now saves again and checks that only the second file is written.
- The B draft selected rows by typing `a` and `r`. Typing `r` would select Reload with Encoding first, since two rows start with r. The tests now click `menu-row-0-3`.

### Existing tests whose expectations changed

- `workbench::light_edit_tabs::tests::an_empty_light_edit_pane_offers_open_file_and_dropping` is renamed `…_offers_new_file_open_file_and_dropping`, with New File first (`src/workbench/EditorArea.tsx:21`).
- No other existing assertion changed. The 254 activation-sensitive tests pass with the disk check on activation.

### Mouse paths not tested

- The LightEdit empty pane's "New File" hint (`empty-editor:file.newUntitled`). Its row and action are asserted; the click goes through the existing deferred handler (`empty_editor.rs:95-99`).
- The File menu row "New Untitled File" (`menu_spec.rs`, native menu bar).

### Files, relative to `$L`

- **Edited:**
  - `crates/ide-host/src/project.rs`
  - `app-gpui/src/app_models/close_guard.rs`
  - `app-gpui/src/editor/actions.rs`
  - `app-gpui/src/editor/view.rs`
  - `app-gpui/src/editor/view/save_cleanup.rs` (`clean_up_before_save` and `document_editors` are now `pub(super)`)
  - `app-gpui/src/editor_model/mod.rs`
  - `app-gpui/src/workbench/app_shell.rs`
  - `app-gpui/src/workbench/editor_core.rs`
  - `app-gpui/src/workbench/empty_editor.rs`
  - `app-gpui/src/workbench/light_edit_tabs.rs`
  - `app-gpui/src/workbench/menu_spec.rs`
  - `app-gpui/src/workbench/mod.rs`
  - `app-gpui/src/workbench/tab_views.rs`
- **Renamed (old → new), then edited:**
  - `app-gpui/src/editor_model/untitled_pending.rs` → `untitled.rs`
  - `app-gpui/src/editor/view/untitled_pending.rs` → `untitled.rs`
  - `app-gpui/src/workbench/untitled_tabs_pending.rs` → `untitled_tabs.rs`
  - `app-gpui/src/workbench/encoding_bom_pending.rs` → `encoding_bom.rs`
  - `app-gpui/src/workbench/disk_sync_window_pending.rs` → `disk_sync_window.rs`
  - `app-gpui/src/workbench/tests/proofs48_{untitled,save_as,light_edit_untitled,bom,disk_sync}_pending.rs` → `proofs48_{untitled,save_as,light_edit_untitled,bom,disk_sync}.rs`
- **Created or deleted:** none.

### What in the notes turned out false

- **Hypothesis 2.** The test seam needs a pending dialog, so the tests have to wait for it.
- **Hunks 1.5(f), 1.5(g), 1.5(l) and 1.8(c).** (f) is behind the webview, and the other three are redundant; all four are removed.
- **Hunk 1.3 and the draft's `save_document_as`** cleaned up after the dialog, not before it.
- **Smaller points:**
  - `save_as_start` searched the active pane first.
  - The draft's `editors_of` duplicated the existing `document_editors`.
  - The draft's waits covered less than the assertions after them.
- **Stale files:**
  - `$S/fmt.sh` changes directory into the main worktree. It worked here only because it is given absolute paths.
  - Most of the slice's rows are not keys in `docs/gpui-port/parity.json`.
