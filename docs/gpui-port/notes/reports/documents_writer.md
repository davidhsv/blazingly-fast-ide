I wrote the draft and its notes, but none of it has been compiled or run, so every test below is unverified until the integrator builds it. I touched no existing file and ran no cargo or git command that changes anything.

**Files written** (untracked; each loses `_pending` when declared)

- `app-gpui/src/editor_model/untitled_pending.rs`: 132 lines. Pure helpers (names, tab input, retargeting tabs, where Save As opens) and 3 unit tests.
- `…/app-gpui/src/editor/view/untitled_pending.rs`: 100 lines. A child of `view`: `Editor::is_untitled`, `can_reload_encoding`, `save_document_as` (runs the save cleanup first), `retarget_document`, `set_bom`.
- `…/app-gpui/src/workbench/untitled_tabs_pending.rs`: 337 lines. The `file.newUntitled` action, the Welcome→LightEdit path, `install_tab_editor` (moved out of `load_tab_probed`), Save As, Save As for every buffer one dialog at a time, and the close question.
- `…/app-gpui/src/workbench/encoding_bom_pending.rs`: 46 lines.
- `…/app-gpui/src/workbench/disk_sync_window_pending.rs`: 117 lines. Wires the existing `DiskSync` model to window activation.
- Tests in `…/app-gpui/src/workbench/tests/`:
  - `proofs48_untitled_pending.rs`: 263 lines, 6 tests
  - `proofs48_save_as_pending.rs`: 187 lines, 5 tests
  - `proofs48_light_edit_untitled_pending.rs`: 51 lines, 1 test
  - `proofs48_bom_pending.rs`: 128 lines, 4 tests
  - `proofs48_disk_sync_pending.rs`: 133 lines, 3 tests
- Notes: `$S/drafts/documents_integration.md` (sections a–g). The hunks come in three pieces, applied in order: untitled + Save As + Welcome, then BOM, then disk sync. Piece 1 includes one `crates/ide-host` seam, `open_light_empty_in`, so no test roots LightEdit at the real home folder.

**List totals:** build 15 parity keys; already there 0 rows (one sub-item, the `{name} BOM` label); needs 0 left (the one need, the seam, is supplied as a hunk); cannot be tested 0 rows (one sub-case, the save dialog's file-type list).

| Row                                                            | State after the slice                              | Tests                                                                                                                                                                                                                                                   |
| -------------------------------------------------------------- | -------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:file.newUntitled`                                      | done                                               | `opens_editable_pathless_buffers_with_incrementing_names`, `new_untitled_file_in_a_window_showing_nothing_turns_it_into_light_edit`                                                                                                                     |
| `ipc:doc_open_untitled`                                        | done                                               | the first test, plus `reuses_the_buffer_for_a_re_opened_untitled_name_and_shares_it_across_tabs`                                                                                                                                                        |
| `ipc:doc_save_as`                                              | done; the file-type list is missing                | `autosave_never_writes_…_retargets_tab_and_model`, `save_as_onto_a_file_open_in_another_tab_is_refused_and_keeps_the_buffer`, `asks_about_a_dirty_untitled_buffer_when_its_last_tab_closes`, `stays_open_when_an_untitled_buffers_save_as_is_cancelled` |
| `ipc:light_edit_open_untitled`                                 | done                                               | the LightEdit test                                                                                                                                                                                                                                      |
| `keymap:vscode-macos` and `-windows`, plus both `module:` rows | done                                               | `the_vs_code_keymaps_open_an_untitled_buffer_with_cmd_n_and_ctrl_n`                                                                                                                                                                                     |
| `unit:src/editor/core/untitled.test.ts`                        | partial, 6 of 7                                    | "offers the registered languages as save-as types" is not covered                                                                                                                                                                                       |
| `ipc:doc_set_bom`                                              | done                                               | `the_encoding_menu_adds_and_removes_the_utf8_bom_and_the_save_writes_it`, `with_autosave_on_adding_the_bom_writes_the_file_at_once`, `the_bom_row_is_only_for_utf8_and_disabled_for_a_read_only_file`                                                   |
| `point.registerStatusItem:editor.encoding`                     | done                                               | the BOM tests, plus `reload_with_encoding_is_disabled_for_a_document_that_cannot_be_read_again`                                                                                                                                                         |
| `ipc:doc_set_encoding`                                         | partial; only the virtual-document re-read remains | the same tests                                                                                                                                                                                                                                          |
| `ipc:doc_sync_from_disk`, `module:src/editor/core/diskSync.ts` | done                                               | `a_file_changed_outside_the_project_is_read_again_when_the_window_is_active_again`, `a_changed_file_with_unsaved_edits_keeps_them_and_a_warning_says_so`                                                                                                |
| `unit:src/editor/core/diskSync.test.ts`                        | done, 7 of 7                                       | `controller_preserves_the_unchanged_documents_selection_and_scroll_then_stops_checking`                                                                                                                                                                 |
| `closeGuard.test.ts:285` / `windowClose.test.ts:118`           | half / covered                                     | `close_project_counts_…`, `stays_open_…`                                                                                                                                                                                                                |

**Not built:**

- When another project opens in this window, the webview carries untitled buffers into it. Natively they are counted and asked about like any unsaved file instead, so no text is lost.
- The tab limit is not applied to a new untitled tab.
- A buffer saved as `.md` or `.sql` does not get its Markdown layouts or SQL console until its tab is reopened.
- Before a Git operation, the webview runs Save As for dirty untitled buffers (`ops.ts:116-132`); natively they are skipped.

**Hypotheses** (each with its check in section d of the notes):

- Hypothesis: the native save dialog takes no file-type list.
- Hypothesis: `simulate_new_path_selection` answers a dialog that is already pending.
- Hypothesis: no watcher sees a file outside the project folder. If one does, the second half of the first disk-sync test fails, which settles it.
- Hypothesis: `activate_window()` in a test fires the activation observer.
- Hypothesis: a file without write permission opens read-only, and a UTF-16LE file with its byte order mark is detected as UTF-16LE.
- Hypothesis: the language services read the editor's path on every request, so they follow Save As.
- Hypothesis: the editor does not take `ctrl-n` on macOS before the keymap does.
- Hypothesis: an orphan untitled buffer can be carried into the next project when one opens in this window.
- Read, not run: idle save opens a Save As sheet for a dirty untitled buffer; the webview does the same (`appearanceRuntime.ts:254-260`).

**One existing test changes:** `an_empty_light_edit_pane_offers_open_file_and_dropping` now gets "New File" first; the replacement is hunk 1.8(d). Also, every window test that activates a window and then advances the clock will now run a disk check followed by `project.refresh` and `vcs.refresh`.

**What turned out wrong:**

- `ipc_rows_read_1.md:637` names a "File › Save As". The webview has no Save As for a file tab; Save As exists only for untitled buffers.
- The brief describes a conflict question and a deleted-file tab for disk sync. The backend only shows a warning for a file with unsaved edits and skips a missing file.
- The parity clause for the encoding item says the "UTF-8 BOM" label is missing; it is already native (`status_items.rs:129`).
- `I-UNTITLED.md` uses `act:` keys and line numbers from an older HEAD.
- The rule files point at the cargo registry, which the brief forbids, so the dialog API stays a hypothesis.
