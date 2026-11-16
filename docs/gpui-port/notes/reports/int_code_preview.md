I finished every part of the brief. Part 0 and the data-loss proofs landed before any place got an editable preview, then Find in Files, Bookmarks and Usages. All gates pass on the final code. Nothing is committed. Shared undo between a tab and a preview is not built, as the brief said; cases 5 and 6 stay pending.

## Part 0: Save All

- **Every unsaved tab is saved, whichever is in front.** Proved by `workbench::tests::proofs26_save_all::save_all_writes_every_unsaved_tab_not_only_the_active_one`. Failed before the fix: yes, "both files to be written did not happen within thirty seconds" (`$S/dprevi/p0_before.log`).
- **A failed save tells the user and leaves that tab unsaved.** The notice is titled "Could not save" with the reason as its body, as `src/editor/core/model/document.ts:801-813` does; the other tab is written. Proved by `…proofs26_save_all::save_all_tells_the_user_when_a_file_cannot_be_saved_and_keeps_that_tab_unsaved`. Failed before: yes.
- **With a notebook tab in front, the text tabs are saved too.** Proved by `workbench::tests::proofs24_e2e_notebook::save_all_with_a_notebook_in_front_also_writes_the_unsaved_text_tabs`. Failed before: yes. `save_all_writes_the_unsaved_notebook` still passes.
- **How:** the `file.save` handler now calls `Workbench::save_all_documents`. It goes through the same `save_documents` path that the existing `save_all` uses; no fourth path was added. The notebook wrapper saves the notebooks, then always calls that handler.
- **One existing assertion flipped.** `the_keymap_runs_editor_actions_on_the_windows_editor` expected `file.save` to fail with no editor open; it now expects success, because Save All with nothing to save succeeds in the webview.

## Data loss

"Failed without the code" comes from `$S/dprevi/dataloss_without_2.log`: the code turned off by `$S/dprevi/toggle.py`, then turned back on.

- **Preview edit on a file with no tab, saved by Save All:** `workbench::tests::proofs26_code_preview::save_all_writes_an_edit_made_in_a_preview_of_a_file_without_a_tab`. Failed without: yes ("counted while it shows, left 0"). It also checks that the document closes after the save.
- **Autosave on choosing another result and on closing the popup:** `…::autosave_writes_a_preview_edit_when_another_result_is_chosen_or_the_popup_closes`. Failed without: yes.
- **Counted by the close question, Save All writes it:** `…::the_close_question_counts_a_preview_edit_and_save_all_writes_it`. Failed without: yes (no question was asked).
- **Don't Save leaves the file on disk as it was, and nothing stays counted:** `…::dont_save_on_the_close_question_leaves_the_file_and_lets_the_preview_go`. Failed without: yes.
- **Closing the dialog keeps the edits and their undo:** `…::keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`. Failed without: no; that holding is the draft's code, which was not turned off.
- **Replace on a file the preview holds:**
  - The document is replaced and counted unsaved, the disk unchanged, until Save All writes it: `…::replace_on_the_previewed_file_leaves_it_unsaved_and_counted`. Failed without: yes.
  - With autosave on, disk and document agree once the popup closes: `…::replace_on_the_previewed_file_ends_written_with_autosave_on`. Failed without: yes.
  - This follows the webview: `FindInFiles.tsx:340-348` bumps the preview version, the preview reopens, and releasing it autosaves.
- **File changed on disk under an edited preview:** the user gets the backend's "a.rs changed on disk" notice, the edits stay, and the disk keeps the outside write. Proved by `…::a_file_changed_on_disk_under_an_edited_preview_is_told_not_overwritten`. Failed without: only on its unsaved-count assertion; the notice and keeping the text are the backend's behaviour, shared with tabs.
- **Documents a preview opened are closed:**
  - `…::a_todo_item_passed_over_before_its_file_opens_leaves_no_document_open` (the `preview_todo` leak, fixed). Failed without: yes.
  - `…::the_documents_a_preview_opened_are_closed_when_it_lets_them_go`. Failed without: no; the editor's own release already closes it.
- **Two data-loss races found by the existing popup Replace tests, and fixed:**
  - A late open answer that the selection had left closed a document that a Replace had just changed. The change was lost. Fix: `let_go` keeps such a document as a held preview. `find::popup_view::tests::replace_replaces_the_selected_row_and_moves_on` failed before the fix (`$S/dprevi/on_4.log`) and passes after (`$S/dprevi/on_5.log`).
  - An editor created after its document became unsaved showed it as saved, so it was neither counted nor kept. `Editor::with_text` now reads the backend's state. Evidence: 2 failures in 20 runs before (quoted under Gates), 0 in 20 after, and 0 in 200 runs under a full-suite load (`$S/dprevi/stress/summary.txt`). Hypothesis: that was the cause; no run captured the failing state.
  - Checking the backend's dirty state, not only the editor's flag, when a preview lets go has no failing-before evidence.
- **Not proven by a test:**
  - Quitting (`app.quit`) and closing a window that is not the last one. Hypothesis: both use the same `unsaved_count` and `save_all` (check: run `app.quit` with a held preview).
  - The stale-answer close inside the shared preview's open; only the TODO version is proven.
  - Hypothesis: the TODO preview's stale close could still drop a document that a Replace changed meanwhile (check: replace a file while its TODO preview is opening).

## Gates (final code)

- **Whole suite run 1** (`$S/dprevi/suite_1.log`, uptime written before and after): `test result: ok. 7889 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 202.03s`
- **Whole suite run 2** (`$S/dprevi/suite_2.log`): `test result: ok. 7889 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 267.13s`
- **Twenty runs during run 2** (`$S/dprevi/loop20/summary.txt`, 80 tests by filter): 20 of 20 passed.
- **Earlier pair, before the `Editor::with_text` fix** (`$S/dprevi/suite_a1.log`, `$S/dprevi/suite_a2.log`): both 7889 passed. Its twenty-run loop (`$S/dprevi/loop20_a/`) passed 18 of 20. The two failures, whole:
  - `thread 'find::popup_view::tests::replace_all_in_one_file_does_not_ask' (286878896) panicked at src/find/popup_view.rs:2804:13: the files were not replaced within thirty seconds: [("a.txt", "bar\nother\nbar\n")]`
  - `thread 'find::popup_view::tests::replace_all_asks_first_when_several_files_change' (286948099) panicked at src/find/popup_view.rs:2804:13: the files were not replaced within thirty seconds: [("a.txt", "bar\n"), ("b.txt", "bar bar\n")]`
- **`cargo test --test web_view`** (`$S/dprevi/web_view.log`): `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.76s`
- **clippy `--all-targets -- -D warnings`:** exit 0 (`$S/dprevi/clippy_4.log`).
- **`cargo build`:** exit 0, 0 warnings (`$S/dprevi/build_2.log`).
- **Format and processes:** a check-only rustfmt pass over every file I touched is clean, and no cargo, test or app process of mine is left.

## Rows (parity keys; nothing written to `parity.json`)

| Key                                                       | State                                 | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Missing                                                                                                                                                     |
| --------------------------------------------------------- | ------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:file.save`                                        | done                                  | `workbench::tests::proofs26_save_all::save_all_writes_every_unsaved_tab_not_only_the_active_one`, `…::save_all_tells_the_user_when_a_file_cannot_be_saved_and_keeps_that_tab_unsaved`, `workbench::tests::proofs24_e2e_notebook::save_all_with_a_notebook_in_front_also_writes_the_unsaved_text_tabs`                                                                                                                                                                                                                                                                                                                                                                                                                                                 | —                                                                                                                                                           |
| `unit:src/features/search/__tests__/codePreview.test.tsx` | partial, 4 of 8 cases, 2 more in part | Case 1: `ui::code_preview::proofs26_code_preview::marks_matches_the_current_lines_as_current_and_the_current_line` (computed on a plain text, covers the view's marks, not the backend). Cases 2, 4 and 8 through the backend: `workbench::tests::proofs26_code_preview::shows_the_file_editable_in_an_editor_view_with_the_matches_highlighted`, `…::keeps_dirty_text_and_undo_when_the_preview_closes_and_reopens_without_a_tab`, `…::enter_inserts_a_newline_editing_keys_stay_out_of_the_results_escape_bubbles`. Case 3 in part (shared edits and save, text kept identical both ways): `…::an_edit_in_the_preview_reaches_the_open_tab_and_the_save_shortcut_writes_it`. Case 7 in part (`file` kind only): `…::keeps_file_documents_read_only` | Undo shared per document (case 3's undo, cases 5 and 6, still in `proofs26_code_preview_pending.rs`); case 7 for `library`, `archive`, `binary` and `large` |
| `unit:src/features/code-insight/usagePreview.test.tsx`    | done for the view, 3 of 3             | `code_insight::usages_view::tests::proofs26_usage_preview::colours_the_usage_line_with_its_file_as_context`, `…::keeps_comment_colours_outside_the_match_and_the_search_foreground_inside_it` (both fill `LineTokens::store` directly: view, not the backend read), `code_insight::usages_view::tests::shows_a_plain_count_for_unclassified_searches_after_a_java_access_mode_was_selected`                                                                                                                                                                                                                                                                                                                                                           | The backend read (`line_tokens::request`) is seen only in the window, not in a test                                                                         |
| `point.registerOverlay:search.findInFiles`                | partial                               | The Find in Files tests above, plus `…::a_result_passed_over_before_its_file_opens_is_dropped`, `…::editing_shortcuts_stay_in_the_preview_and_the_others_still_run`, `…::a_large_file_previews_while_typing_in_the_query_still_lands` and the data-loss tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Shared undo; the other gaps the notes list                                                                                                                  |
| `action:find.inPath`                                      | partial                               | same                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | same                                                                                                                                                        |
| `action:find.replaceInPath`                               | partial                               | `…::replace_on_the_previewed_file_leaves_it_unsaved_and_counted`, `…::replace_on_the_previewed_file_ends_written_with_autosave_on`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | as for find.inPath                                                                                                                                          |
| `point.registerOverlay:search.bookmarks`                  | done                                  | `…::the_bookmarks_popup_previews_the_bookmarked_line_in_an_editor`, `…::an_empty_previewed_line_is_tinted`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            | —                                                                                                                                                           |
| `action:navigate.showBookmarks`                           | done                                  | same                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  | —                                                                                                                                                           |
| `point.registerToolWindow:usages`                         | partial                               | `…::the_usages_preview_marks_every_usage_of_the_file_read_only`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       | as the notes say                                                                                                                                            |
| `action:find.usages`                                      | partial                               | same, plus the usage-row tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | The UI's own syntax fallback is not exercised                                                                                                               |
| `feature:search`                                          | partial                               | all of the above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | The Find tool window's and Search Everywhere's previews are not on the shared preview                                                                       |

## Differences from the webview, closed

- **Shortcuts:** only shortcuts bound to actions whose condition names `editorTextFocus` stay in the preview, as the webview's key claim does; other shortcuts still run. Proved by `…::editing_shortcuts_stay_in_the_preview_and_the_others_still_run` (Duplicate Line leaves the tab alone, ⌘S runs Save All).
- **⌘Y in the preview:** it is bound to Quick Definition, whose condition does not name `editorTextFocus`, so it runs on the tab behind the popup. The webview does the same, read from `parts.tsx:182-188`.
- **Gutter:** a preview shows line numbers only (no fold chevrons, icon columns or change bars). Asserted in the "shows the file editable" test and seen in the window.
- **Empty line:** an empty previewed line is now tinted. Side effect: an empty breakpoint or execution line is now tinted too.
- **Row colours:** a row whose own text is unchanged but whose context changed (a comment opened above it) keeps its old colours. Not fixed.
- **Show Usages popup:** its preview is unchanged, as the brief said.

## Hypotheses from the notes

1. A preview edit reaches the tab: confirmed (`an_edit_in_the_preview…` passes).
2. Keys reach the preview editor: confirmed (`enter_inserts…` passes).
3. ⌘S is handled by the preview itself: changed. ⌘S is now Save All through the keymap; the preview's own ⌘S handling only runs when ⌘S is not bound.
4. Typing lands in the preview: confirmed.
5. A click keeps the keyboard in the preview: confirmed in the window (shot `dprev3-04`).
6. The line is centred: the line is in view; centring itself was not observed, because the results shot sat at the file's end, where the scroll stops. Hypothesis: it centres mid-file (check: re-shoot a result in the middle of a long file).
7. The webview's preview marks the file recent: not settled. Hypothesis (check: read `src/editor/core/model/attachment.ts`).
8. A line inside a block comment is coloured as a comment: confirmed by test, and the Rust rows are coloured in the window.
9. No language server starts in the tests: consistent, since the tests pass; not measured on its own.
10. The usages pane downcasts to the shared preview: confirmed.
11. Enter, Backspace and Escape behave in the popup: confirmed.

## Cost

- **Per frame:** the editor shapes and paints only the visible rows. The preview's own render walks nothing; a usage row does one lookup per visible row (read in code, not profiled).
- **The 80 ms debounce** covers opening a new file or a new version. Moving to another line of the shown file applies at once.
- **The window's thread** takes a copy of the text and creates the editor. Reading and parsing run off it.
- **Measured once, 5 MB file** (`$S/dprevi/cost.log`): the preview was not shown before typing; typing three keys in the query took 21.3 ms and they landed; the preview showed the file's last line by the end of the typing. The "longest turn" figure measured nothing. Hypothesis: the 21.3 ms includes creating the editor for the 5 MB document.

## Shots (all under `$S/insp8/shots/`)

- `dprev4-01-find-theme1-1.png` (dark) and `dprev4-02-find-theme2-1.png` (light): Find in Files on the final binary; preview in colour and the editor font, match marked, line 429 tinted and in view, line numbers, no chevrons.
- `dprev4-04-bookmarks-theme2-1.png` (light): bookmark on line 428 tinted, editor colours. I did not open its dark counterpart.
- `dprev4-06-usages-theme1-1.png` (dark): usage rows coloured (`fn`, `mut`, `Vec`, `u32`), usage marked.
- `dprev5-02-usages-theme1-1.png` (dark) and `dprev5-03-usages-theme2-1.png` (light): usage on line 429 selected; preview read-only, coloured, line tinted.
- `dprev2-03-usages-sel-1.png` (dark) and `dprev2-04-usages-sel-other-1.png` (light): text files, both usages of `b.txt` marked.
- `dprev3-03-clicked-1.png` and `dprev3-04-typed-1.png` (light, binary before the last three fixes): after a click and one typed `x`, the popup stays open and the caret line is tinted. Nothing in the dialog shows the unsaved state, the same as the webview (`parts.tsx` has none). With autosave on, `x` was written to disk when the popup closed.
- Shots taken at a mark are captured a little later; `dprev2-01`, `dprev5-01` and `dprev1-*` show later states and prove nothing.
- I did not open the Agents tool window or the Services window. Both temp projects were removed.

## False or stale in the notes and drafts

- `view.rs` hunk 2 had moved. The nine accessor hunks for the tests were anchored on text an earlier hunk changes.
- The draft's late-answer close could drop a document a Replace had changed (fixed, above).
- The draft's tests kept popup handles alive, which kept the previews alive.
- `Editor::with_text` took its unsaved flag from the open-time info, which can be stale (fixed).
- The popup Replace All tests now accept "document replaced and held unsaved" in place of the disk, because in those window-less tests nothing closes the popup.

## Files edited

- `app-gpui/src/code_insight/usages_popup.rs`
- `app-gpui/src/code_insight/usages_view.rs`
- `app-gpui/src/editor/actions.rs`
- `app-gpui/src/editor/marks.rs`
- `app-gpui/src/editor/view.rs`
- `app-gpui/src/everywhere/bookmarks_popup.rs`
- `app-gpui/src/find/popup_view.rs`
- `app-gpui/src/platform/input.rs`
- `app-gpui/src/ui/code_preview.rs`
- `app-gpui/src/ui/mod.rs`
- `app-gpui/src/workbench/code_insight.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/notebook.rs`
- `app-gpui/src/workbench/todo_window.rs`
- `app-gpui/src/workbench/tests/proofs12_find_popup.rs`
- `app-gpui/src/workbench/tests/proofs24_e2e_notebook.rs`
- `app-gpui/src/workbench/tests/proofs26_code_preview.rs`
- `app-gpui/src/workbench/tests/proofs_editor_search_find.rs`
- New: `app-gpui/src/workbench/tests/proofs26_save_all.rs`
- Declared, unchanged: `ui/code_preview/line_tokens.rs`, `ui/code_preview/proofs26_code_preview.rs`, `code_insight/usages_view/tests/proofs26_usage_preview.rs`
- Left undeclared: `workbench/tests/proofs26_code_preview_pending.rs`
