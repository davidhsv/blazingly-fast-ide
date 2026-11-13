Both parts are integrated, with the end-to-end tests the notes asked for plus four I added. The whole suite passed five runs in a row and clippy is clean. Two drafted bugs mattered most: tree shortcuts (F4, Delete, ⌘R…) ran Database actions in every window, and the file probe made two editor tests flaky. Both are fixed. One way a password can still leak sits in `crates/`, which I couldn't edit (end of §4). No existing test was changed or ignored.

## 1. Per step

**A1: the four viewer modules**

- **13 compile errors, 3 kinds:**
  - 10 borrow errors in `hex_view.rs`: under the 2024 rules the `impl IntoElement` return held on to `cx`. Fixed with a plain loop and `+ use<>` on `render_row`, `render_header` and `render_scrollbar`.
  - 1 missing lifetime in a `markdown_view.rs` test helper.
  - 2 double mutable borrows in a `preview_host.rs` test.
- **Results:** `image_view` 6 passed, `hex_view` 4, `markdown_view` 5, `preview_host` 7; 0 failed.
- **Bug fixed, `preview_host.rs` `apply_layout`:** switching to Editor Only dropped the preview and its subscription before the preview's last `ShownLine(None)` event was delivered, and GPUI drops events from released entities. The drop is now deferred until after delivery, and the spec test passes.

**A2: `tab_views.rs` and the `mod.rs` / `project_files.rs` changes**

- 0 compile errors.
- **I also changed:**
  - The conflict banner in `pane_element` shows only over a text editor.
  - `chrome.rs` gets a `Content::Viewer` arm, and the status bar is in light-edit mode for a viewer.
- **Bug fixed: the probe made two editor tests flaky** (3 of 6 failures each):
  - The tests are `typing_completes_from_the_server_and_enter_inserts_the_item_with_its_import` and `an_answer_overtaken_by_a_newer_request_is_dropped`. Traces showed the language server attaching while `obj.` was being typed.
  - With the probe off: 0 of 12 module runs failed.
  - Fix: the probe and the document open now happen in one backend call (`tab_views::open_tab_file`, `Workbench::load_tab_probed`), and loose files use it too. The shared-probe machinery (`FileProbes` and waiters) is gone. A failed probe falls back to the text editor, as `resolveFileEditor` does.
  - After the fix: 1 failure in the first 10 module runs, then 0 in the next 20.

**A3/A4: end-to-end tests and bars**

- The notes' four tests passed as written.
- I added a status/title bar test and a smoke test for the rest of the view-tab wiring.

**B1: the four database modules**

- **6 compile errors:**
  - 1 borrow in `source_dialog.rs` `set_field`.
  - 1 borrow in `store.rs` `remove_source`.
  - 3 borrows from the same capture issue in `table_editor_view.rs` (fixed with `+ use<>`).
  - 1 missing lifetime in a `console_view.rs` test.
- **Results:** `store` 31 passed, `source_dialog` 14 (12 at this step; I added 2 security tests later), `console_view` 8, `table_editor_view` 7.

**B2/B3: wiring and end-to-end tests**

- 0 compile errors. Clippy needed 2 fixes: `is_multiple_of` in `console_view.rs` and an unused `let _ =` in `database.rs`.
- **Bug fixed: tree shortcuts fired everywhere.**
  - `assets/gen/actions.json` has no `when` / `keyWhen` for the tree actions. The webview sets them through a helper the generator does not read.
  - So F4, ⌘R, ⌘D, ⌘B, Delete, ⌘⌫, ⇧F6, ⇧Enter, ⌘F2 and ⌘⌥⇧C ran Database actions in any window. This broke `the_git_tool_window_shows_the_projects_log` (F4 in the Commit window).
  - Fix: the 10 tree actions now need an open project, and their shortcuts work only while the tree has the keyboard (a new `databaseViewFocus` context key). The tree's filter box is outside the tree's focus area, so Delete while filtering does not drop a source.
- The notes' five tests passed as written.
- **My smoke test failed in 2 of 5 suite runs.** Its rename step waited for the backend, not for the store to re-read the sources. Duplicate then built on the old list and undid the rename. The webview's `renameSource` has the same gap, so I made the test wait for the tree.

**Notes statements that were false**

- `view_tabs` §3.5: "there is no conflict banner in `pane_element`" (there is one).
- `view_tabs` §3.19: "`chrome.rs` is not declared" (it is).
- `view_tabs` §6(e): the probe's extra round trip is harmless (it made two editor tests flaky).
- `view_tabs` §8: `ImageView` decoding a PNG was an open risk. It does decode.
- The database report's "store requests are logged by name only, nothing logs a password" holds. But the secrets were in derived `Debug` output, and the notes never noticed the shortcut gap.
- The claim that Open as Text opens the text tab beside the viewer is true: `controller.ts` calls `admitInput(fileInput(path))` with a new key and closes nothing.

## 2. Actions, tool windows, tab kinds, context keys

| Action                                                                                                           | What it does                                         | Test that runs it             |
| ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | ----------------------------- |
| `image.zoomIn`, `zoomOut`, `actualSize`, `fitToWindow`, `toggleChessboard`                                       | act on the active image tab (chessboard is a toggle) | (a), view-tabs smoke          |
| `markdown.editorOnly`, `editorAndPreview`, `previewOnly`                                                         | set the file's layout                                | (c), view-tabs smoke          |
| `db.showToolWindow`                                                                                              | shows the Database window                            | (b), database smoke           |
| `db.newDataSource`                                                                                               | opens the Data Source dialog                         | (a)                           |
| `db.openConsole`                                                                                                 | opens the source's console file                      | database smoke                |
| `db.properties`, `refresh`, `disconnect`, `goToDdl`, `duplicateSource`, `rename`, `copyReference`, `collapseAll` | tree actions on a node                               | database smoke                |
| `db.editData`                                                                                                    | opens the table editor                               | (c); F4 in the database smoke |
| `db.drop`                                                                                                        | removes a source after asking                        | (e)                           |
| `db.submitChanges`, `revertChanges`                                                                              | table editor edits                                   | (c)                           |
| `db.execute`                                                                                                     | runs the statement of the active `.sql` editor       | (d), database smoke           |
| `db.explainPlan`                                                                                                 | runs Explain; the plan is only logged                | database smoke                |

- **Tool window added:** `database` (`TOOL_WINDOWS` is now 16).
- **Tab kinds added:** `image` and `binary`, neither restored with the session, plus `Content::Viewer` for a loose file.
- **Context keys:** `imageViewerActive`, `imageViewerFocus`, `databaseViewFocus`, `dbTableEditorFocus`, `dbTableEditorChanges`. `lightEdit` now also holds for a loose file in a viewer.
- **Still not wired (view tabs, §9):** clicks in Preview Only don't make that pane active; image and hex tabs keep their old path after a rename; dirty view tabs are not counted when closing the window; plugin editor wrappers; the preview's code colouring and font zoom; `ShownLine` does not reach Structure.
- **Still not wired (Database, §5):**
  - `ConsoleView` and its toolbar.
  - With two or more sources, Execute in a plain `.sql` file only says "choose a data source".
  - Query History and the Query Plan tab.
  - The schema-object dialogs; SSH configurations.
  - The table editor's Import, export Browse… and `db.goToColumn`.
  - The Database window does not count as focused while the keyboard is in its lower half.
  - Never registered: `db.jumpToConsole`, `db.quickDoc`, `db.truncate`, `db.modifyTable`, `db.new`, `db.newTable`, `db.newColumn`, `db.importData`, `db.ddlToClipboard`, `db.ddlToConsole`, `db.runSqlScript`, `db.sqlScript`, `db.goToReferencedTable`, `db.separateProceduresAndFunctions`, `db.queryHistory`, `db.pluginConnect`, `db.goToColumn`.
- **The table editor stays in the panel's lower half.** Moving it to an editor tab means building it from the tab input with access to the store, routing its events, closing it with the tab, and changing test (c)'s check of `panel.lower()`. That is not cheap.

## 3. What a user sees in Part A

- **Image:** it opens in an image tab with the zoom controls and a label like "2×2 · 74 B · 100%". The tab has no text editor, so text actions are off. The test platform does decode a PNG from a file (test (a) waits for it).
- **Binary file:** it opens in a read-only hex view, and no document is ever opened for it. Open as Text adds a read-only text tab beside it; opening the file again goes to that text tab.
- **Markdown:** it stays a text tab, with the Editor / Split / Preview buttons in the tab strip and a live preview that updates once typing pauses.
- **Status and title bars:**
  - On a view tab the editor's status items (caret, line separator, encoding, indent, read-only) are hidden, and the window title ends with the viewer's file name.
  - A loose file in a viewer gets the light-edit title bar showing the file name.

## 4. Part B: driver and security

- **Driver:** SQLite on an empty `app.db` in the test project. It needs no server or network.
- **What I changed:**
  - A `SecretText` type whose `Debug` prints `<redacted>`. It now carries the password and SSH secret in `DbOp::SetPassword`/`SetSshSecret`, `PromptAnswer::Secret`, `SecretChange::Set` and `SourceDialogEvent::Test`.
  - Hand-written `Debug` for `SourceForm` and `SourceDialogEvent` that hides the typed secrets and the password inside a URL (`redacted_url`), including the URL being typed.
  - `PasswordField::painted()` is now the only text the field draws.
- **Tests, all passing:**
  - `a_typed_password_is_never_painted_copied_or_printed`: only bullets painted and handed to the input method, ⌘C and ⌘X leave the clipboard unchanged, and no event or form `Debug` contains the password.
  - `a_password_typed_in_the_url_is_never_printed`: default and URL-only modes.
  - `redacts_the_password_of_a_url`.
  - `requests_and_answers_that_carry_a_secret_never_print_it`: the store's commands and requests.
  - `the_password_a_connection_asks_for_is_masked_and_answers_the_store`: the connect-time password question.
- **`tracing` check:** 12 calls in `workbench/database.rs`, none in `store.rs`. They print a console path, a count of marks, a job id and ok flag, a request name, a file-picker error, an action title, and a plan id with node count. None prints a secret.
- **Leak left (read from the code, not run):** `DataSource` in `crates/ide-db/src/source.rs:157` derives `Debug` including `url`. In URL-only mode that URL keeps `user:password@` as typed. So `Debug` of `DbOp::SaveSources`, `Prompt::Password`/`SshSecret`, `TreeUpdate::Sources` and `Answer::Sources` would print it. Nothing prints these today. The fix belongs in `ide-db`.

## 5. End-to-end tests, as they stand

- **Part A, from the notes:**
  - `a_png_opened_from_the_project_view_shows_in_an_image_tab_that_zooms_and_closes_without_asking`: opened from the tree with the keyboard; context keys; zoom; closes without asking.
  - `a_file_with_nul_bytes_opens_in_the_hex_view_and_open_as_text_gives_it_a_text_editor`: exact hex row; no document opened; Open as Text gives a read-only editor; reopening goes to the text tab.
  - `a_markdown_file_opens_as_text_with_its_layouts_and_a_preview_that_follows_the_typing`: language key; layout buttons in the strip; preview updates after the typing pause; Preview Only.
  - `reopening_a_project_restores_its_text_and_markdown_tabs_and_not_its_image_tab`.
- **Part A, added:**
  - `the_status_and_title_bars_name_the_viewers_file_and_leave_out_the_editors_items`.
  - `view_tabs_run_their_actions_split_reopen_and_route_their_views_requests`: every image action, split, reopen, hex Open as Text, the preview's image and link, and Markdown tab release.
- **Part B, from the notes:**
  - `new_data_source_tests_and_saves_a_sqlite_file_that_then_shows_in_the_tree`.
  - `expanding_a_data_source_lists_its_tables_and_a_tables_columns`.
  - `edit_data_shows_the_rows_submit_writes_an_edit_and_revert_discards_one`.
  - `execute_shows_a_querys_rows_and_its_output_lines_and_a_failing_statement_its_error`.
  - `dropping_a_data_source_asks_and_removes_it_from_the_project_and_the_tree`.
- **Part B, added:**
  - `the_database_trees_actions_and_shortcuts_reach_the_store_and_the_backend`: F4 does nothing in an editor and opens the table in the tree, plus all the tree actions, the console and Explain.
  - `the_password_a_connection_asks_for_is_masked_and_answers_the_store`.

## 6. Final state

- **Five whole-suite runs:** each `test result: ok. 6019 passed; 0 failed; 1 ignored` (the ignored one is in `plugins.rs` and was there before).
- **Failures seen along the way:** those listed in §1, all fixed.
- **Clippy:** `cargo clippy --all-targets -- -D warnings` clean.
- **Formatting:** all touched files pass rustfmt, `workbench/mod.rs` included.
- **Out of scope:**
  - The action manifest generator drops `when`/`keyWhen` set through a helper. I fixed the tree actions in `database.rs`; any other feature built the same way has the same gap.
  - Also out of scope: the `DataSource` `Debug` leak in §4.
