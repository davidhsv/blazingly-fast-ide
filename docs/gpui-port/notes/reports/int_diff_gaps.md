I landed all three parts and the default now follows the webview: under the non-modal commit interface (on by default), Show Diff and the Commit window open the diff in an editor tab. Every gate passed. Nothing is committed or pushed.

## 1. Gates (measured, from `app-gpui/`)

| Gate                                        | Result                                                                                                           |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------------- |
| Baseline at clean HEAD, before any edit     | `test result: ok. 7603 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 184.34s`             |
| Suite run 1 (nothing else running)          | `test result: ok. 7630 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 187.03s`             |
| Suite run 2                                 | `test result: ok. 7630 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 219.56s`             |
| 20× loop during run 2                       | 20 of 20 runs passed, each `45 passed; 0 failed`; tally shows all 45 tests at 20 each; no test failed in any run |
| `cargo test --test web_view`                | `test result: ok. 1 passed; 0 failed; …`                                                                         |
| `cargo clippy --all-targets -- -D warnings` | exit 0, `Finished`, no warning or error lines                                                                    |
| `cargo build`                               | the full rebuild (`Finished … in 2m 29s`) printed no warning lines                                               |

- **How the loop ran:** I called the built test binary (`target/debug/deps/openidea_gpui-b0f788e1630dc362`) directly with 14 filters, using `$S/ddfg/loop.sh`. It ran 19:15:48–19:17:43, inside suite run 2, which started about 19:15:45 and ended 19:19:25.
- **What the loop covered:**
  - wrap tests, soft-wrap proofs and moved-code tests;
  - `tab_view` and `diff_tab` tests, and `proofs12_diff_tabs`;
  - the pinned tests: `show_diff_on_a_change_of_the_commit_window…`, `proofs_vcs::compare_previous_file…`, all of `proofs12_diff_window`, the window flow of `proofs18_e2e_diff_navigation`, and `vcs_operations::…the_diff_draws_with_the_editors_font_settings…`;
  - the T4 toolbar test and `tab_views::every_kind_is_registered_once`.
- **Suite after each part:** part 1 gave 7611 passed, part 2 gave 7615 passed.
- **Ordering caveat:** I wrote part 2 and D1–D30 to disk while the previous part's suite was still running on its already-built binary. Each suite tested the binary of its own part.

Failures seen along the way, with their cause:

- **First full suite after part 3:** `workbench::tab_views::tests::every_kind_is_registered_once` failed on `assertion failed: kind("diff").is_none()`. The test asserted that no `diff` tab kind exists, and D22 registers one on purpose. I changed it to `assert!(kind("diff").is_some_and(|kind| !kind.restorable && kind.focus_key.is_none()));`.
- **Pins before the flip:** this evidence is a `diff`-filtered run, not a full suite. With D1–D30 in and D31 out, it gave 372 passed and 9 failed. All 9 open the diff through Show Diff and need D31 (`proofs12_diff_tabs::opens_an_editor_tab…`, 6 draft `diff_tab` tests, my nav, Jump to Source and close tests). Every pinned modal test passed.
- **My first two-file test project** made Show Diff build a chain of one, because the files changed after the window opened and the Commit window was not open. The helper now prepares the repository before opening and waits for the Commit window to list `["Changes (2)", "  a.txt", "  b.txt"]`.
- **My first next-file wait** checked the counter for "1 difference". A file reached through the chain opens at its first difference, so the counter reads "1 of 1 differences". I fixed the wait, not the assertion.
- **The draft Jump to Source test** failed with `the diff tab stays open  left: 0  right: 1`: jumping from the single-file diff, which is a preview tab, closes that tab, because a new regular editor replaces the preview tab natively (`editor/tabs.rs`).
  - Hypothesis: the webview does the same (`src/editor/core/model/tabs.ts:207`, read only). The webview e2e run in a tab would settle it.
  - The test now uses Show Diff of two files, which opens regular tabs, and asserts both stay open.

## 2. Per part

**Part 1 (soft wraps):** hunks S1, S2, V1–V31, T1–T4, G1, G2, W1, W2 applied as written. `proofs12_diff_soft_wraps_pending.rs` renamed with `mv`. Nothing changed beyond the notes. `rustfmt` changed nothing.

**Part 2 (moved code):** hunks M1–M8 applied as written. `proofs12_moved_code_view_pending.rs` renamed with `mv`.

**Part 3 (diff as an editor tab):** D1–D32 applied, D31 and D32 last. `proofs12_diff_tabs_pending.rs` renamed with `mv`; the superseded `proofs18_e2e_diff_navigation_pending.rs` removed with `rm`. Beyond the notes I added:

- **`vcs_diff/tab_view.rs`:** the tab measures its toolbar with a `canvas` (the pattern from `agents/new_task_view.rs:746`). New `settings_menu_position()` returns the toolbar's left + 8 and its bottom, as `DiffEditor.tsx:941-943` places the menu when it is not opened from the gear.
- **`workbench/diff_tab.rs`:** `active_diff_tab`, `step_diff_tab_file` (Compare Next/Previous File in the active tab goes straight to the chain search; the preview tab takes the result, per `DiffEditor.tsx:686-696` and `file(dir)`) and `show_diff_tab_settings` (the menu with Use Soft Wraps checked as the panes wrap). Also four tests and helpers (`non_modal_repository`, `two_files_project`, `focus_viewer`, `edited_at`).
- **`workbench/diff_window.rs`:** `step_diff_file` now takes `window` and falls back to the active tab; `show_diff_settings` falls back to the tab when no diff window is shown.
- **`workbench/tab_views.rs`:** the test assertion above.

`editor.toggleSoftWrap` is not registered. The native editor has no soft wrap at all: `grep wrap src/editor/view.rs` finds none, and the only editor-side code is the pattern matcher in `editor_model/editor_prefs.rs`. Supporting it for the editor is not small. The diff-only route would also need the `softWrapSurfaceFocus` context key.

## 3. The default flipped: tests proving the tab

All of these run through the keymap with the tab's viewer focused, under `workbench::diff_tab::tests::`:

| What                                                                                                                                                 | Test                                                                | Red without the fix (seen)                                                                                                                                                                                     |
| ---------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Next/previous file: `ctrl-shift-right` / `ctrl-shift-left` (`diff.nextFile` / `diff.prevFile`); also reads `diffEditorFocus` on, then off after blur | `compares_the_next_and_previous_file_in_a_diff_tab_from_the_keymap` | with `step_diff_file`'s fallback disabled: "b.txt's diff did not happen within thirty seconds"                                                                                                                 |
| Settings menu: `cmd-shift-d` (`diff.showSettings`) opens the menu under the toolbar; clicking Use Soft Wraps flips the tab's viewer                  | `diff_settings_opens_the_settings_menu_under_the_diff_tabs_toolbar` | with the fallback disabled: panic at `diff_tab.rs:563:81`, inside the menu read. Hypothesis: it is the `.expect("the settings menu is open")`; the line number was not checked against the file as it was then |
| Jump to Source: `f4` opens `b.txt` at line 1, the diff tabs stay                                                                                     | `jump_to_source_opens_the_file_at_the_caret_and_keeps_the_diff_tab` | with D19's tab fallback disabled: "b.txt in the editor did not happen within thirty seconds"                                                                                                                   |
| Close shortcut: `cmd-w` closes the tab                                                                                                               | `the_close_shortcut_closes_a_diff_tab_that_has_the_keyboard`        | needed nothing beyond D31; I did not see it fail for a reason other than D31                                                                                                                                   |

The brief's "toolbar buttons" for next/previous file are stale: a grep of `src/features/vcs/` finds `diff.nextFile` / `diff.prevFile` only as actions (`compare.tsx:178-191`), with no button in any tab or dialog toolbar.

## 4. `parity.json` keys (I did not write `parity.json`)

The totals M come from the notes. My own regex count of top-level `it(` calls (20 / 14 / 8) does not expand `it.each` rows, so it neither confirms nor refutes them. Hypothesis: counting the expanded rows of each spec settles it.

| Key                                                     | State reached                | Proving tests (full names)                                                                                                                                                                                                                                                                                                                                                                                                                                                                 | Missing                                                       |
| ------------------------------------------------------- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| `unit:src/features/vcs/__tests__/diffToolbar.test.tsx`  | done, 25 of 25 per the notes | `workbench::tests::proofs12_diff_soft_wraps::{soft_wraps_follow_the_editors_setting, soft_wraps_reconfigure_without_losing_the_selection, soft_wraps_turn_on_from_the_settings_menu_and_stay_on}`; `workbench::diff_tab::tests::maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor`; `workbench::diff_tab::tests::restores_the_shared_layout_after_the_diff_tab_is_remounted`; `vcs_diff::tab_view::tests::does_not_offer_editor_maximization_in_an_embedded_diff` | —                                                             |
| `unit:src/features/vcs/__tests__/movedCode.test.tsx`    | done, 15 of 15 per the notes | `vcs_diff::view::tests::proofs12_moved_code_view::{labels_edited_moves_and_highlights_only_their_internal_word_edits, shows_native_move_links_follows_them_in_both_directions_and_updates_after_editing, omits_inner_edits_in_highlight_lines_mode, respects_do_not_highlight}`; `vcs_diff::view::moved::tests::{labels_the_links_by_direction_and_edits, places_the_edits_of_a_move_on_their_lines}`                                                                                      | —                                                             |
| `unit:src/features/vcs/__tests__/diffDialog.test.tsx`   | done, 8 of 8 per the notes   | `workbench::tests::proofs12_diff_tabs::opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen`                                                                                                                                                                                                                                                                                                                                                                                 | —                                                             |
| `e2e:e2e/diff-navigation.spec.ts`                       | done, 3 of 3 flows           | `workbench::diff_tab::tests::proofs18_e2e_diff_navigation::{maximizes_a_diff_preview_tab_and_restores_its_previous_layout, continues_between_changed_files_from_toolbar_buttons_and_the_keymap, skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files}`                                                                                                                                                                                                            | the spec's `toolbar toBeHidden` is not asserted               |
| `point.registerEditorKind:diff`                         | partial                      | `workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab`, `vcs_diff::tab_view::tests::*` (3)                                                                                                                                                                                                                                                                                                                                                               | not restored with the session; working-copy side not editable |
| `action:vcs.showDiff`                                   | partial                      | adds `workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab`                                                                                                                                                                                                                                                                                                                                                                                              | folders, editable panes                                       |
| `action:diff.nextFile`, `action:diff.prevFile`          | done, now in a tab too       | add `workbench::diff_tab::tests::compares_the_next_and_previous_file_in_a_diff_tab_from_the_keymap`                                                                                                                                                                                                                                                                                                                                                                                        | —                                                             |
| `action:diff.showSettings`                              | done, now in a tab too       | add `workbench::diff_tab::tests::diff_settings_opens_the_settings_menu_under_the_diff_tabs_toolbar`                                                                                                                                                                                                                                                                                                                                                                                        | —                                                             |
| `action:diff.jumpToSource`                              | done, now in a tab too       | add `workbench::diff_tab::tests::jump_to_source_opens_the_file_at_the_caret_and_keeps_the_diff_tab`                                                                                                                                                                                                                                                                                                                                                                                        | —                                                             |
| `action:diff.nextDifference`, `context:diffEditorFocus` | done                         | add the e2e tests above; the context is read on and off in the next/previous-file test                                                                                                                                                                                                                                                                                                                                                                                                     | —                                                             |
| `point.registerSoftWrapSurface`                         | stays 0 of 1                 | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | needs `editor.toggleSoftWrap` natively                        |
| `e2e:e2e/compareWith.spec.ts`                           | stays partial                | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | the compare actions keep the diff window, per the brief       |

## 5. Stale or false in the notes and brief

- The notes say `diff.nextFile`, `diff.prevFile` and `diff.showSettings` "do nothing in a tab". That is no longer true; the rows can name the tests above.
- The notes say the tab "stays open" after Jump to Source. That holds for regular tabs only; a preview diff tab is replaced (measured natively).
- The notes' file moves say `git mv` / `git rm`; I used `mv` and `rm` as you asked.
- The notes did not list `tab_views::tests::every_kind_is_registered_once` among the tests D22 breaks.
- The brief's "toolbar buttons" for next/previous file have no webview counterpart.
- The draft test `diff_settings…` assumed the panes do not wrap at first. `a.txt` matches the default Soft-wrap files (`*.txt`) and `editor.softWrap` defaults to true, so they do wrap; the test now asserts the setting flips.

## 6. Shots

All are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/`. The binary ran through `run.sh` on temp projects `projtab` and `projmodal`. No `fail` line was logged, and no shot shows a user or host name.

- **`ddfgtab-01-tab-wrapped-1.png`:** preview tab "notes.txt (HEAD ↔ Your version)". The long lines wrap, line numbers sit on the first visual row only, the word marks hold "long" and "LONG and edited", and both sides line up. Maximize Editor sits before Settings.
- **`ddfgtab-02-tab-maximized-1.png`:** the Project view is hidden, the text rewraps wider, and the button shows Restore Size pressed.
- **`ddfgtab-03-tab-restored-1.png`:** the Project view is back, rewrapped at the narrower width, still level.
- **`ddfgtab-04-tab-moved-1.png`:** `code.js`, counter "2 differences · 1 moved block". Accent stripes run along the moved lines, the pills read "To L7 (edited)" and "From L1 (edited)", and 'first' / 'changed' are tinted.
- **`ddfgtab-05-tab-settings-menu-1.png`:** the Diff Settings action opens the menu under the toolbar's start, with the Use Soft Wraps row.
- **`ddfgmodal-01-modal-wrapped-1.png`:** the diff window under the modal interface (from `.openidea/settings.toml`), wrapped, with no Maximize Editor.
  - With Align Changes off, changed row 2 takes 6 visual lines on the left and 7 on the right, so the rows below sit one line lower on the right. Unchanged rows match their counterpart's height, and the divider band joins the rows.
  - Hypothesis: the webview behaves the same with Align Changes off; viewing the same diff there would settle it.
- **`ddfgalign-01-modal-aligned-1.png`:** after clicking Align Changes, both sides are level (line 3 at the same height on both).
- **`ddfgmenu-01-modal-menu-1.png`:** the window's menu with Use Soft Wraps checked while the panes wrap.
- **`ddfgmodal-02-modal-moved-1.png`:** moved-code marks and links in the window.

Not looked at: unified mode with wraps, zoom, and clicking a move link in the real window.

## 7. Files

`git diff --name-only` (modified):

- `app-gpui/src/vcs_diff/dialog_view.rs`
- `app-gpui/src/vcs_diff/mod.rs`
- `app-gpui/src/vcs_diff/tab_view.rs`
- `app-gpui/src/vcs_diff/toolbar_view.rs`
- `app-gpui/src/vcs_diff/view.rs`
- `app-gpui/src/workbench/diff_tab.rs`
- `app-gpui/src/workbench/diff_window.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/tab_views.rs`
- `app-gpui/src/workbench/tests/proofs12_diff_window.rs`
- `app-gpui/src/workbench/tests/proofs18_e2e_diff_navigation.rs`
- `app-gpui/src/workbench/tests/proofs_vcs.rs`
- `app-gpui/src/workbench/tool_windows.rs`
- `app-gpui/src/workbench/vcs_operations.rs`

Deleted, shown by `git diff` as removed:

- `app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view_pending.rs` (renamed)
- `app-gpui/src/workbench/tests/proofs12_diff_soft_wraps_pending.rs` (renamed)
- `app-gpui/src/workbench/tests/proofs12_diff_tabs_pending.rs` (renamed)
- `app-gpui/src/workbench/tests/proofs18_e2e_diff_navigation_pending.rs` (removed with `rm`)

Untracked, the renamed targets:

- `app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view.rs`
- `app-gpui/src/workbench/tests/proofs12_diff_soft_wraps.rs`
- `app-gpui/src/workbench/tests/proofs12_diff_tabs.rs`

Now declared (committed drafts, unchanged by me): `app-gpui/src/vcs_diff/wrap.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs`.

Scratch only (under `$S`): `ddfg/apply.py`, `ddfg/loop.sh`, the suite logs and the two temp projects in `ddfg/`, and the `insp8/ddfg*.script` files.
