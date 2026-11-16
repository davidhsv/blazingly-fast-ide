**Gutter integration report**

The gutter now draws change bars with their popup, bookmark icons and mnemonics, run marks, the caret row's tint and its brighter number, a 1 px hairline, and the gutter's own background. All five draft groups compiled; the only build error was a duplicate `run_gutter_command` name. Every gate passed. Nothing is committed or pushed.

## 1. Idle and cost

**What each editor adds (from reading the code; the counts below are measured):**

- Three subscriptions per followed editor: its edits, its gutter presses, and its release.
- One observer per window on the run store.
- Two one-shot timers per editor, never repeating:
  - Hunks are asked 30 ms after the editor opens, 60 ms after the repository's state version changes, and 300 ms after an edit.
  - Run marks are searched 350 ms after an edit (300 ms for `package.json`).
- Only an editor that some pane shows schedules either timer. A hidden one is marked stale and asks once a tab change shows it (`editor_gutter_shown_changed`, called from `tabs_changed`).

**Tests that prove it (all in `workbench::tests::proofs27_gutter`):**

- `an_open_editor_asks_the_backend_nothing_while_nothing_changes`: a quiet window, then 60 × `advance_clock(1 s)`. Hunk requests and run-mark searches stay the same.
  - The first version of this test failed: the project's own `.openidea` folder, created at startup, triggered one repository read and so one extra request (trace measured). The webview also refetches on every version change (`gutter.ts:166`).
  - The test now waits for 50 polls with an unchanged version, then repeats the minute (at most 5 times) until one minute passes with no version change, and asserts on that minute only.
- `an_editor_behind_another_tab_asks_nothing_until_it_is_shown_again`: a commit plus a refresh while the tab is hidden gives zero requests. A press on the `tab:` element then gives at least one request, and the bars clear.
- `an_answer_for_a_text_that_changed_since_is_dropped`: an answer to the pre-edit ticket arrives after an edit and is dropped; the lane stays at `[2,4,6]` until the next real answer gives `[0,2,4,6]`.
  - An answer for a closed tab has nowhere to land, because the entry is removed when the editor is released. Not tested separately.
- Another file's answer cannot reach an editor, because answers are keyed by editor.

**Per frame (read, not measured per call):** one binary search per painted row for the number colour; one `partition_point` per painted row for the lane; two `partition_point`s each over the bookmarks and run marks of each shown editor. Nothing in paint reads files or asks the backend.

**Per edit:** while a file has hunks, each edit builds a `Doc` of the whole text (this is not in paint).

**Measured once, debug build, with a temporary test that I then deleted by editing:**

| File                             | Lane marks | Per keystroke | Per frame |
| -------------------------------- | ---------- | ------------- | --------- |
| 5.3 MB, 100,000 lines, committed | 0          | ~1.29 s       | ~3.2 ms   |
| Same file, untracked             | 100,000    | ~1.44 s       | ~4.0 ms   |
| 20,000 lines with 10,000 hunks   | 10,000     | ~247 ms       | ~2.4 ms   |

So the gutter adds about 150 ms per keystroke on the 5 MB file in debug. The webview has no size limit either (`gutter.ts`, `ide-vcs line_hunks`), so I added none.

**Safety:** `grep Command::new` finds nothing in `crates/ide-run/src/detect.rs` or `app-gpui/src/run_producers/`. Hypothesis (confirm with the same grep): detection for `hello.rs` and `package.json` starts no process. The menu uses `ToolEnv::optimistic`. The script menu reads files through `ide_host::ops::fs`.

## 2. Gates

Logs are under `$S/dguti/`.

| Gate                                        | Log file           | Result                                                                                                                  |
| ------------------------------------------- | ------------------ | ----------------------------------------------------------------------------------------------------------------------- |
| Suite 1 (alone)                             | `gate-suite1.log`  | `test result: ok. 7917 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 207.42s`; load 5.62 → 19.64 |
| Suite 2                                     | `gate-suite2.log`  | `test result: ok. 7917 passed; 0 failed; ... finished in 246.12s`; load 18.46 → 117.09 (the loop ran beside it)         |
| 20× loop of the 38 added or changed tests   | `gate-loop20.log`  | 20 of 20 `test result: ok. 38 passed; 0 failed`; no test failed in any run                                              |
| `cargo test --test web_view`                | `gate-webview.log` | `test result: ok. 1 passed; 0 failed`                                                                                   |
| `cargo clippy --all-targets -- -D warnings` | `clippy2.log`      | exit 0, no findings                                                                                                     |
| `cargo build` (after the last edit)         | `gate-build.log`   | exit 0, 0 warnings                                                                                                      |

Failures seen earlier and fixed:

- In the baseline suite (`s0.log`), 4 existing tests failed on the new widths; they are listed in section 4.
- `proofs27` needed one hairline adjustment.
- The idle test hit the extra request described above.

## 3. Parity rows

None of the four `point.registerEditorContribution` keys below exists in `parity.json` today (measured by reading the file). I did not write to `parity.json`.

| Key                                                  | Today   | Reaches   | Proving tests and what is missing                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ---------------------------------------------------- | ------- | --------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:navigate.toggleBookmark`                     | partial | done      | `proofs27_gutter::toggle_bookmark_draws_the_bookmark_icon_on_the_carets_line_in_the_bookmarks_column`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `action:navigate.toggleBookmarkWithMnemonic`         | partial | done      | `proofs27_gutter::toggle_bookmark_with_mnemonic_draws_the_chosen_digit_on_the_carets_line`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `ipc:vcs_line_hunks`                                 | partial | done      | `proofs27_gutter::the_change_lane_draws_bars_on_added_and_modified_lines_and_a_notch_where_lines_were_deleted`, `…the_bars_follow_an_edit_at_once_and_the_backends_next_answer_adds_the_typed_line`, `…an_untracked_file_is_all_added_lines`                                                                                                                                                                                                                                                                                                                                                                          |
| `point.registerEditorContribution:vcs.changeMarkers` | absent  | partial   | Proven by the three above plus `…a_press_on_a_bar_opens_its_changes_popup_and_rollback_puts_the_original_line_back` (asserts dirty and one undo), `…next_change_in_the_popup_moves_the_caret_and_shows_the_next_changes_popup`, `…a_file_outside_any_repository_has_no_bars`. Missing: overview-lane marks, bar tooltips and pointer cursor, inline blame, and Show Diff and Copy in a window test                                                                                                                                                                                                                    |
| `point.registerEditorContribution:search.bookmarks`  | absent  | partial   | Proven by the two toggle tests, `…a_primary_click_in_the_gutter_toggles_the_lines_bookmark`, `…a_bookmark_follows_its_line_when_a_line_is_inserted_above_it` (with undo), `…a_bookmark_follows_its_line_when_the_file_changes_on_disk`, `…a_moved_bookmark_keeps_its_line_after_the_tab_is_closed_and_opened_again`, `…a_moved_bookmark_keeps_its_stored_line_when_the_tab_closes_without_saving`, `…both_panes_of_a_split_window_draw_their_bookmarks`, `…with_gutter_icons_off_the_bookmarks_and_run_columns_take_no_room`. Missing: the mark's tooltip; the stored line after a whole-line deletion was not tested |
| `point.registerEditorContribution:run.targets`       | absent  | partial   | `…a_main_gets_a_run_mark_whose_menu_offers_to_run_and_to_modify_its_configuration`, `…a_run_mark_launch_with_nothing_to_run_says_that_no_configuration_matches_the_file`. Missing: passed/failed test icons, inherited Java `main`s, Go preload, executor rows                                                                                                                                                                                                                                                                                                                                                        |
| `point.registerEditorContribution:run.npmScripts`    | absent  | partial   | `…a_package_json_script_gets_a_run_mark_whose_menu_runs_it_with_the_lockfiles_manager`. Missing: choosing Run or Debug in a test (it would start `pnpm`)                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `unit:src/features/run/npmGutter.test.ts`            | done    | done      | `run_producers::npm_gutter::tests`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `point.registerMenuGroup:vcs.gutter`                 | partial | unchanged | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `module:src/editor/core/gutterMenu.ts`               | partial | unchanged | —                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

**Click targets as they ended** (x from the editor's left edge):

- **Blame column:** ⌘ toggles a bookmark; otherwise its own press.
- **Numbers:** ⌘ toggles a bookmark; a plain press sets or clears a breakpoint.
- **Bookmarks column (15 px):**
  - On an icon: ⌘ toggles it, a plain press does nothing, a right press opens the gutter menu.
  - Empty cell: as the numbers.
- **Run column (14 px):**
  - On a mark: a plain press opens its menu, ⌘ toggles the bookmark, a right press opens the gutter menu.
  - Empty cell: as the numbers.
- **HTTP and plugin columns:** unchanged. Their marks still swallow ⌘ and right presses (existing behaviour, not changed).
- **Lane slack:** from 1 px left of the 4 px lane to 3 px into the chevrons, a plain press on a marked row opens the popup. This matches the webview's hit area (`gutter.ts:233`: `left - 1` … `right + 3`).
- **Fold chevrons (16 px):** ⌘ falls through; a plain press folds.
- **Hairline (1 px):** neither gutter nor text.

## 4. Hypotheses and other parts

**The draft's hypotheses:**

- Two event types on one editor, the let-else borrows, `Focusable as _`, and `Theme::color` all compiled.
- `hello.rs` gets a mark on line 0, and the menu runs from Run to Modify; `pnpm run dev` is the label.
- `icon:bookmark` is found.
- The popup placed right after Next lands correctly.

All of these held: their tests pass.

**Changed from the draft or the notes:**

- **Bookmark icon:** the asset was a filled orange icon. It is now the webview's accent outline (`bookmarks/gutter.ts:30-40`), drawn on a canvas.
- **Bookmark mapping:** TrackBefore, confirmed in `@codemirror/view` (`GutterMarker.prototype.mapMode`).
- **Rollback:** now shares `vcs_operations::roll_back` with Rollback Lines.
- **"Nothing to run here":** both reachable sites in `run_context.rs` now take the message; a gutter launch says "No run or test configuration of the project matches this file." with key `run-target` (`runGutter.ts:71-78`).
- **Icons in every shown editor:** each pane's active editor draws its icons. Selectors are now `bookmark-gutter-{entity}-{line}`.
- **Light edit:** the first loose file's editor is now followed (`tab_views.rs`), asserted in `light_edit_tabs::tests::the_title_and_the_status_bar_follow_the_active_tab`.
- **Hunk 19:** every bookmark-store write reaches `sync_bookmarks_window` (checked by grep).

**Previews and embedded editors:**

- Asserted plain gutters in `proofs26_code_preview`: `shows_the_file_editable…` (Find in Files), `the_bookmarks_popup_previews…`, `the_usages_preview_marks…`.
- Hypothesis: the TODO preview (`todo_window.rs:363`) and `theme/installed_fonts.rs:316` stay plain, because only `editor_gutter_opened` turns the marks on and it is called from two places only (`grep -n editor_gutter_opened app-gpui/src -r`).
- I did not check whether the diff sides, notebook cells and commit message are `Editor` instances.

**Tokens:**

- Numbers now use `fg-line-number` (falling back to `fg-gutter`).
- The gutter background is `bg-editor-gutter` (falling back to `bg-editor`).
- The hairline is `border-gutter` (falling back to `border`), from `theme.ts:129-131,220-222`.

**The "(empty line)" bookmark defect:** not reproduced. The native toggle with a given place read the line's text only from the active editor, exactly as the webview's `lineText` does (`search/index.ts:93`). A press in another pane's gutter therefore stores empty text. Native now reads the text from any editor of that file, proven by `both_panes_of_a_split_window_draw_their_bookmarks`. Hypothesis: that was the cause in the earlier shot.

**Existing tests whose numbers I changed:**

| Test                                                                                             | Old                   | New                      | Webview source                                                                                                                                               |
| ------------------------------------------------------------------------------------------------ | --------------------- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `proofs15_folding::the_fold_column_is_drawn_only_while_show_code_folding_outline_is_on`          | `32+4`                | `32+15+14+4+1`           | Bookmarks 15 px = 11 px spacer + 2 px padding each side (`bookmarks/gutter.ts:47,167-171`); run 14 px (`Run.module.css:805`); hairline 1 px (`theme.ts:222`) |
| `editor_extras::tests::distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text` | `32+4`                | `32+15+4+1`              | Same sources                                                                                                                                                 |
| `http_client::tests::a_run_mark_stands_in_its_own_gutter_column_right_of_the_line_number`        | `8`; `4+fold`         | `8+15`; `4+fold+1`       | Same sources                                                                                                                                                 |
| `context_menus::tests::the_gutters_menu_adds_a_bookmark_on_the_clicked_line`                     | press at `text−16−12` | press on line 2's number | An icon keeps its own presses (`breakpointGutter.ts:131-137`)                                                                                                |

## 5. Shots

All in `$S/insp8/shots/`. Themes are named as I saw them; the file names are inverted in places.

- **`dguti1-01-main-light-1.png` (dark):** blue modified bar on line 2, grey notch on line 7, green added bar on line 9, chip "5" on line 4, bookmark on line 5 (still the old filled orange icon), green run mark ▷, caret-row tint with a brighter "4", hairline.
- **`dguti1-02-main-dark-1.png` (light):** the same in light.
- **`dguti1-03-package-light-1.png`:** actually shows `api.http` (light): the bookmarks gap, then the HTTP marks.
- **`dguti1-04-http-light-1.png` (dark):** `api.http`.
- **`dguti2-02-popup-dark-1.png` (light):** the popup under line 2, right of the lane: toolbar, "Modified lines 1 of 3", `- hello` / `+ hi`.
- **`dguti2-05-runmenu-light-1.png` (dark):** Run 'main.rs', Debug 'main.rs', Modify Run Configuration… with shortcuts; nothing was chosen.
- **`dguti2-07-package-dark-1.png` (light):** script marks on lines 5 and 6, after the Unicode text.
- **`dguti2-09-guttermenu-dark-1.png` (light):** the gutter menu.
- **`dguti2-10-zen-dark-1.png` (light):** Distraction Free Mode: no gutter, icons or bars. Taken in light only.
- **`dguti4-01-iconsoff-a-1.png` (light):** gutter icons off: the columns collapse and the bars stay.
- **Not looked at:** `dguti2-03`, `-04`, `-06`, `-08`, `dguti4-02`.

No shot shows the user name or host. The temp project was removed, the app's PID is gone, and the one stray wait loop of mine was stopped.

## 6. Left undone, and stale notes

**Undone:**

- HTTP and plugin marks still swallow ⌘ and right presses (existing code).
- The per-edit `Doc` rebuild for large files.
- Gutter icons off in dark, and Distraction Free Mode in dark.
- Toggling Show Gutter Icons from the menu (see the defect below).

**Defect found:** the gutter menu's Appearance › Show Gutter Icons shows unchecked while the icons are shown (seen in `dguti3-01-appearance-1.png`), so clicking it writes `true` again. Hypothesis: the key `editor.showGutterIcons` is missing from the settings JSON that `context_menus::setting_on` reads (`setting_on` sits next to `GUTTER_APPEARANCE`, `context_menus.rs:578`). Confirm by printing `serde_json::to_value(effective(...))` in a test.

**Stale in the notes:**

- The bookmark icon asset is filled, not an outline.
- `run_gutter_command` clashed with an existing method of that name.
- Hunk 9's anchor changed: a `CurrentLine` kind was added.
- Note (e)7 was half wrong: `run_context` already reaches its own message from two sites.
- Only 22 proofs27 tests exist, not "14 + 6".
- Since launch, `docs/gpui-port/parity.json` and `INVENTORY.md` no longer show as changed in `git status`; I never touched them.

**Files I edited** (all under `app-gpui/src/`):

- `editor/view.rs`
- `editor/view/gutter_marks.rs`
- `vcs/mod.rs`
- `workbench/bookmarks_window.rs`
- `workbench/branches.rs`
- `workbench/context_menus.rs`
- `workbench/editor_extras.rs`
- `workbench/editor_extras/tests/proofs15_folding.rs`
- `workbench/editor_gutter.rs`
- `workbench/editor_gutter/run_marks.rs`
- `workbench/http_client/tests.rs`
- `workbench/light_edit_tabs.rs`
- `workbench/mod.rs`
- `workbench/run_context.rs`
- `workbench/tab_views.rs`
- `workbench/tests/proofs26_code_preview.rs`
- `workbench/tests/proofs27_gutter.rs`
- `workbench/vcs_operations.rs`

`vcs/hunk_popup.rs` was declared as handed over and was not changed.
