All four gates are green and nothing was committed. Part A is done: 36 of the 43 `proofs12` tests are declared and pass; I built three missing behaviours and moved the other seven tests to undeclared `_pending` files. Part B is done, with one bug of my own found in the real window and fixed: the Add Service menu opened in the wrong place.

## 1. Gates

| Gate                                                | Result                                                  |
| --------------------------------------------------- | ------------------------------------------------------- |
| `cargo test --lib`, final run 1                     | 7038 passed; 0 failed; 0 ignored (141.02 s)             |
| `cargo test --lib`, final run 2                     | 7038 passed; 0 failed; 0 ignored (141.63 s)             |
| Added, declared or changed tests, 20 runs by filter | 20 of 20 passed (57 tests per run, no test failed once) |
| `cargo test --test web_view`                        | 1 passed                                                |
| `cargo clippy --all-targets -- -D warnings`         | 0 errors, 0 warnings                                    |
| `cargo build`                                       | 0 warnings                                              |

The 20-run set: every `proofs12_` test, `workbench::edit_actions::tests`, `workbench::popup_placement::tests`, `ui::popup_anchor::tests`, `platform::edit_target::tests`, `workbench::tests::proofs3_run::add_service_opens_its_menu_under_its_button`, `workbench::tests::go_to_line_and_bookmarks_move_the_caret_through_the_project` and `workbench::tests::the_find_bar_marks_its_occurrences_replaces_in_one_step_and_closes_on_escape`.

Failures seen along the way:

- **`the_find_bar_marks_its_occurrences_replaces_in_one_step_and_closes_on_escape`** failed in two earlier full runs (7037 passed, 1 failed each). Cause: text fields now take Edit › Undo. The test ran `edit.undo` while the find field had the keyboard, so Undo took back the field's typing instead of the editor's Replace All. The webview does the same: `editCommand` in `src/features/app/appActions.ts:34-57` finds no editor view for a plain input and lets the input undo. I changed the test to focus the editor before `edit.undo`; the assertion is unchanged. It is in the 20-run set.
- **`show_usages_opens_under_the_caret` and `the_mnemonic_chooser_opens_under_the_caret`** failed first because they compared exact points (455 against 455.4). Layout rounds the popup to whole pixels. They now allow less than 1 px.
- An earlier full run (7019 passed) may have compiled while I was editing Part B files, so it is not counted.

## 2. Part A: `proofs12`

**Declared and passing** (36 tests):

- `vcs_diff/diff_options/tests/proofs12_moved_code.rs`: all 10 passed on the first run.
- `workbench/tests/proofs12_diff_window.rs`: 10.
- `workbench/tests/proofs12_find_popup.rs`: 16.
- The `mod` lines are in `app-gpui/src/vcs_diff/diff_options.rs` and `app-gpui/src/workbench/mod.rs`.

**Behaviour built:**

- **Jump to Source from a diff of two revisions.**
  - Files: `app-gpui/src/workbench/diff_window.rs`; in `app-gpui/src/vcs_diff/view.rs`, `map_line` is now a free function and `side_text` is new.
  - Webview: `src/features/vcs/DiffEditor.tsx:770-812`.
  - Behaviour: when neither side is a file, the loader probes the diff's path; if it exists, the toolbar offers Jump to Source. The jump loads the working text off the UI thread, maps the line through a new diff of the shown pane against it, and keeps the column only when the line reads the same.
  - Tests: `workbench::tests::proofs12_diff_window::{jumps_to_the_working_copy_of_a_file_shown_at_a_revision, maps_a_line_of_the_commit_to_the_working_copy, maps_a_line_of_the_parent_through_the_diff_to_the_working_copy, jumps_on_cmd_down_in_the_intellij_macos_keymap_only_while_a_diff_pane_has_focus}`.
- **The diff window owns the close shortcut.**
  - File: `app-gpui/src/vcs_diff/dialog_view.rs`; the window closes on any binding of `toolwindow.closeContent`.
  - Webview: `src/features/vcs/diffDialog.tsx:83-95`.
  - Test: `…::proofs12_diff_window::owns_the_close_shortcut_with_body_or_titlebar_focus`.
- **The diff window can be resized, and remembers its size.**
  - File: `app-gpui/src/workbench/diff_window.rs`.
  - Webview: `src/features/vcs/diffDialog.tsx:18-35, 75-106`, `src/ui/Dialog.tsx:250-252, 349-382`, `src/ui/Dialog.module.css:41-64`.
  - Size: it opens centred at `min(1100, width−64)` × `min(720, height−64)`, or at the size stored under `openidea.vcs.diffDialogSize`, kept inside the window less 24 px. It used to fill the window less fixed 72/56 px margins.
  - Dragging: the 16 px grip at the bottom right has two painted strokes and the NW-SE cursor (checked against the macOS mapping). The drag keeps the top left corner in place and holds at least 480 × 280. The size is stored on release. The settings menu now opens relative to the window.
  - Test: `…::proofs12_diff_window::remembers_the_resized_size_for_the_next_opening`.

**Moved to undeclared `_pending` files.** The test bodies are unchanged; the two new files also carry the helpers they need. The two pending files I created compiled when I declared them for one run; they failed as expected and I undeclared them again.

| File                                                                                            | Tests                                                                                                                                                    | What it waits for                                                                                                                                          |
| ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/workbench/tests/proofs12_diff_tabs_pending.rs`                                    | `opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen`                                                                                     | A diff editor tab kind. `show_diff` also passes `false` for non-modal.                                                                                     |
| `app-gpui/src/workbench/tests/proofs12_find_popup_pending.rs`                                   | `is_resizable_and_opens_at_the_size_it_was_left_at`                                                                                                      | A resizable Find in Files popup. The webview also keeps a results/preview split (`openidea.search.findPopupSplit.v1`); the native popup has fixed heights. |
| `app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view_pending.rs` (the whole draft, moved) | `labels_edited_moves_and_highlights_only_their_internal_word_edits`, `shows_native_move_links_follows_them_in_both_directions_and_updates_after_editing` | Moved-code marks, move links, the "(edited)" label and move word edits in the painter.                                                                     |
| `app-gpui/src/workbench/tests/proofs12_diff_soft_wraps_pending.rs` (the whole draft, moved)     | `soft_wraps_follow_the_editors_setting`, `soft_wraps_reconfigure_without_losing_the_selection`, `soft_wraps_turn_on_from_the_settings_menu_and_stay_on`  | Line wrapping in the diff painter (rows are fixed single lines with `whitespace_nowrap`), plus the "Use Soft Wraps" item.                                  |

- I did not apply H1 (`DiffView::soft_wraps()`), because the soft-wrap tests are not declared.
- Hypothesis: the soft-wraps pending file does not compile until H1 is applied. Check: declare it in the workbench `mod tests` and run `cargo test --lib --no-run`.
- The two drafts that moved whole show in `git status` as a deletion plus an untracked file.

**Spec counts after the changes:**

- `diffToolbar.test.tsx`: 20 of 25. Missing: 11–13 (pending) and 15 and 17 (no diff tab).
- `movedCode.test.tsx`: 13 of 15. 11 and 13 are pending; 12 and 14 stay weak.
- `diffDialog.test.tsx`: 7 of 8. 8 is pending.
- `findPopup.test.tsx`: 23 of 25, of which 7, 9, 10 and 18 are partial. 17 was never written; 21 is pending.

Cases 1 and 3 of `diffDialog` would pass whatever the non-modal setting is, because `show_diff` ignores it.

## 3. Part B: edit targets and popup placement

**Declared:** `platform::edit_target`, `workbench::edit_actions`, `ui::popup_anchor` and `workbench::popup_placement`.

**Hunks applied, each re-anchored against the current tree** (every anchor still matched once): A1–A3, A5–A9 and B1–B9. I did not declare `error_log_sink`; `shelf_time` was already declared.

**Changed from the drafts:**

- **A4 is dropped, as the brief decided.** I also removed what only fed it: `EditTarget::can_edit` with all its implementations, `edit_target::can_edit`, `edit_item_disabled`, `command_of`, `TextField::can_undo/can_redo`, and the "greyed" assertions in the tests. Where Copy was greyed, the dialog test now asserts that Copy with nothing selected leaves the clipboard alone.
- `EditTarget` needs a `Sized` bound to compile.
- **Popup frame.** `popup_body` builds on the current popup frame (rounded corners and the popup shadow), and only the anchored case wraps it in `anchored().snap_to_window()`. Go to Line goes through `dialog_frame`.
- **Bug found in the real window: Add Service.** Run while the Services window was hidden, its menu opened at the fallback point (100, 102), not under the Add button. `ServicesView::show_add_service_menu` now waits for the button's next drawn frame. The webview waits one animation frame for the same reason. `add_service_opens_its_menu_under_its_button` now also covers the action while the window is hidden.
  - Hypothesis: the test would have failed before this fix. Check: in `app-gpui/src/services/view.rs`, make `show_add_service_menu` emit at `button_menu(None)` instead of waiting, then run `cargo test --lib add_service_opens_its_menu_under_its_button`.

**Webview positions checked:**

- Usages: `src/features/code-insight/ipc.ts:80-89` plus the default offset of 4 in `src/ui/Popup.tsx:76-77`.
- Mnemonic chooser: `src/features/search/index.ts:56-68` (caret bottom +2) and `src/features/search/bookmarks/BookmarkPopups.tsx:101-104`.
- Select In: `src/features/project-tree/actions.ts:149-152`.
- Add Service: `src/features/services/ServicesTree.tsx:63-67`.
- Go to Line: a centred dialog, `src/editor/core/view/GotoLineDialog.tsx:35-43`.

**Tests:**

- `workbench::edit_actions::tests::*` (6).
- `workbench::popup_placement::tests::*` (7).
- `ui::popup_anchor::tests::*` (2).
- `platform::edit_target::tests::*` (2).
- `workbench::tests::proofs3_run::add_service_opens_its_menu_under_its_button` (new).
- `workbench::tests::go_to_line_and_bookmarks_move_the_caret_through_the_project` (switched to `goto_line_dialog()`).
- `workbench::tests::a_pressed_widgets_tooltip_stays_off_the_menu_it_opened` still passes.

**Secret fields:** they refuse Copy and Cut through the actions and the keys, and `TextState` and `TextHistory` print sizes only in `Debug`.

**Screenshots** (debug binary, fresh home, `HOME` and `ZDOTDIR` temp folders, temp git project, stopped by its PID): `$S/d104/insp/shots/tour-0{1..6}-*.png`. I looked at all six:

- Usages: under the caret's line.
- Mnemonic chooser: just under the caret's line.
- Select In: under the caret.
- Go to Line: a centred dialog with title, field, hint and OK/Cancel.
- Diff window: 1100 × 720, centred, with the grip drawn.
- Add Service: under the + button, after the fix.

All stay inside the window.

In the screenshots, the Services window lists the machine's real Docker containers. The app reads the real Docker even with a temp `HOME`; I did nothing to them.

## 4. Inventory rows

Proposed states; I did not edit `docs/`.

| Key                                                                                                     | State   | Tests                                                                                                                      | Missing                                           |
| ------------------------------------------------------------------------------------------------------- | ------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| `unit:…/diffToolbar.test.tsx`                                                                           | partial | as in section 2                                                                                                            | 11–13, 15, 17                                     |
| `unit:…/movedCode.test.tsx`                                                                             | partial | `vcs_diff::diff_options::tests::proofs12_moved_code::*`                                                                    | 11, 13                                            |
| `unit:…/diffDialog.test.tsx`                                                                            | partial | `proofs12_diff_window::*` + existing                                                                                       | 8                                                 |
| `unit:…/findPopup.test.tsx`                                                                             | partial | `proofs12_find_popup::*` + existing                                                                                        | 17, 21                                            |
| `action:edit.copy`, `action:edit.cut`, `action:edit.paste`, `action:edit.redo`, `action:edit.selectAll` | done    | `workbench::edit_actions::tests::*`                                                                                        | —                                                 |
| `action:edit.undo`                                                                                      | partial | `workbench::edit_actions::tests::*`                                                                                        | Project view Undo of a move; ⌘Z in the tree       |
| `unit:src/editor/core/clipboardHistory.test.ts`, `module:src/editor/core/clipboardHistory.ts`           | done    | `…::records_the_selected_text_of_inputs` + existing                                                                        | —                                                 |
| `unit:src/features/app/__tests__/editCommand.test.ts`                                                   | done    | `…::use_the_focused_editors_history_including_programmatic_edits`                                                          | —                                                 |
| `action:navigate.gotoLine`, `point.registerOverlay:editor.gotoLine`                                     | done    | `workbench::popup_placement::tests::go_to_line_*`, `…::escape_closes_go_to_line_…`, `…::the_go_to_line_key_runs_nothing_…` | —                                                 |
| `action:find.showUsages`, `point.registerOverlay:code-insight.usagesPopup`                              | done    | `…::show_usages_opens_under_the_caret`                                                                                     | —                                                 |
| `action:navigate.toggleBookmarkWithMnemonic`                                                            | done    | `…::the_mnemonic_chooser_opens_under_the_caret`                                                                            | —                                                 |
| `action:navigate.selectIn`                                                                              | partial | `…::select_in_opens_its_chooser_under_the_caret_of_the_edited_file`                                                        | contributed targets, Navigation Bar row           |
| `action:services.addService`                                                                            | partial | `proofs3_run::add_service_opens_its_menu_under_its_button`                                                                 | the picker for a type with several configurations |
| `module:src/ui/Menu.tsx`                                                                                | partial | same                                                                                                                       | header rows, check marks, menu aim                |

## 5. Hypotheses

Hypothesis: the native diff window (and the diffs Git Log opens through it) is the only diff host that needs Jump to Source for revision diffs. The other `DiffView` host, `app-gpui/src/vcs/local_history_view.rs:240`, has no Jump to Source wiring. Check: `grep -rn "DiffView::new(" app-gpui/src`, then read whether the Local History webview offers Jump to Source.

## 6. False or stale in the notes and briefs

- The diff window's size key is `openidea.vcs.diffDialogSize`, without `.v1` (`diffDialog.tsx:18`).
- The webview maps a revision line straight to the working text with one diff, not in two steps through the newer revision.
- F7: the webview popup also stores a results/preview split, which the notes do not mention.
- The integration notes say to run `cargo fmt`; I used `fmt.sh` on named files only.
- The B3 anchor quotes `plugin_prompt_layer()`; the current code is `plugin_prompt_layer(cx)`. The placement assumption "exact origin" is off by sub-pixel rounding (now handled in the tests).
- The `gpui-fast` doc comment maps `ResizeUpLeftDownRight` to CSS `nesw-resize`, against its name. The macOS code uses the NW-SE cursor.
- B3f: no dialog context key exists, so none was set for Go to Line.
