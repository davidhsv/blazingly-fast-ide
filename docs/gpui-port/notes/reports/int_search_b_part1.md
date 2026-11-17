# Search Everywhere scope/history, Recent Files edited files: stopped, the disk is full

The task is not finished: the disk filled up partway through. The tree is left in a broken state. Eight temporary "behaviour off" edits are still in the source, so the app does not behave as written. They must be reverted before anything else runs.

From the run-B compile onwards, every write and every shell command failed with `ENOSPC`. The run-B compile itself died with `rustc-LLVM ERROR: IO failure on output stream: No space left on device` (`$S/dsbi/failfirst_B.log:68`). Even the Edit tool gets `ENOSPC` creating its temp file. Freeing space means deleting something (at least this worktree's `app-gpui/target`), and I am not allowed to delete files, so you need to decide that. `S` below is `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`.

## The tree right now

These eight edits are each marked `/* OFF */`. `python3 -I $S/dsbi/switch.py B on` puts every one back; `grep -rn "OFF \*/" app-gpui/src` must then print nothing.

1. `app-gpui/src/everywhere/view/memory.rs`, `recent_action_ids`: reads key `""`. It should read `RECENT_ACTIONS_KEY`.
2. `memory.rs`, `browse_history`: an early `return;`.
3. `memory.rs`, `remember_history`: an early `return;`.
4. `memory.rs`, `assign_shortcut`: a `return;` before `let arg = Value::String(...)`.
5. `app-gpui/src/workbench/mod.rs`, `leave`: `let _ = &self.editor_core.edited;` stands where `self.editor_core.edited.clear();` belongs.
6. `app-gpui/src/everywhere/view/parts.rs`, `is_narrow`: compares with `0.0` instead of `NARROW_WIDTH`.
7. `parts.rs`, `icon_slot`: the `.size(px(ICON))` call is missing.
8. `app-gpui/src/everywhere/view.rs`, `answered`: an extra `let ticket = /* OFF */ Ticket { generation: self.controller.snapshot().generation, ..ticket };` line.

Run A's switches were put back before run B started.

## Gates

- **HEAD baseline:** the related tests before any edit: `test result: ok. 464 passed; 0 failed; 0 ignored; 0 measured; 7809 filtered out; finished in 6.75s` (`$S/dsbi/filtered_before.log`, filters `everywhere search find_action go_to_file symbol recent switcher`).
- **With the code on:** the build had 0 errors and 0 warnings (`$S/dsbi/build1.log`). The new tests and `find_action_says_what_to_do_when_it_has_nothing_to_list` gave `test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 8273 filtered out; finished in 2.30s` (`$S/dsbi/new_on2.log`).
  - That run predates the last test I added (`a_toggled_action_run_from_the_popup_remembers_the_query_while_it_stays_open`), so that one has never compiled or run.
  - The first such run (`$S/dsbi/new_on1.log`) had 3 failures, all in the draft tests, all fixed:
    - `alt_up…`: the history was not saved on close. Fixed by remembering in `Workbench::close_popup` and dropping the test's handle.
    - `tab_cycles…`: its query "kept" matches nothing, so the popup widened by itself. It now types "alpha", a project file.
    - `mixed…rows…`: the action's title is "Find Action…", with the ellipsis.
- **Not run:** the "after" filtered run, both whole-suite runs, the 20× repeats, `web_view`, clippy and `cargo build`.
- **Processes:** I stopped the waiting loop `bzmu370mg` with TaskStop. I could not check by PID that nothing of mine is left. Hypothesis: nothing is; check with `ps -ax | grep openidea_gpui-dce820e51898f091`.

## Piece 1: Search Everywhere

**Code** (`everywhere/view.rs`, `view/memory.rs`, `view/parts.rs`, `workbench/mod.rs`):

- **One place for tab and scope:** the view's own `tab` field is gone; the tab and the scope live only in `SearchEverywhereState`. The query is the field's text while the popup is open and is copied into the state on close. History and recent actions live only in `UiStorage`, the app-wide store, read and written through `store.rs`.
- **Tab switching:** a tab switch (`setTab`, `SearchEverywhere.tsx:282`) and a command row reset the history walk.
- **Saving history:** the query is saved when a result is opened or run (`SearchEverywhere.tsx:301`), and on close (`store.ts`, `hideSearchEverywhere`), at most once per popup. Close covers `close_popup` and the popup being released.
- **⌘↵:** on Text it runs `find.inPath` with the query (`:404`). On any other tab it acts as Enter (`:405-407`); before this change it did nothing.
- **⌥↵:** runs `settings.assignShortcut` on action rows only (`:362`, `:366`). The menu for other rows is not built.
- **Scope check box:** drawn only on All and Actions (`:530`). The file-set chooser of the other four tabs is missing.
- **Widening:** before the deferred wider search runs, the popup checks that the query is still the one that found nothing.
- **Edited test expectation:** in `workbench::tests::find_action_says_what_to_do_when_it_has_nothing_to_list`, "Nothing found in project files" becomes "Nothing found" (`SearchEverywhere.tsx:451`, `:496`).
- **Height:** the fixed 440 px height is gone. The list is `min(672, max(240 while a query is typed, rows×24)) + 8` (`SearchEverywhere.tsx:89-91`, `:443`), under `max-height: 80vh` (`.module.css:4`).
- **Rows and frame** (numbers from the CSS module unless noted):

| Element                     | Native now                                                         | Webview source                              |
| --------------------------- | ------------------------------------------------------------------ | ------------------------------------------- |
| Popup width                 | 760, less in a narrow window                                       | `.tsx:506`                                  |
| Space beside a narrow popup | 24 px each side                                                    | `Dialog.module.css:9`                       |
| Header                      | 44 px                                                              | line 21                                     |
| Tabs                        | 26 px high, padding 0 9px, selected border                         | lines 38-39                                 |
| Header wrap                 | at 680 px inside the 1 px frame                                    | line 333; `box-sizing` from `global.css:20` |
| Query field                 | 32 px                                                              | line 84                                     |
| Icon slot                   | 16 px                                                              | line 169                                    |
| Footer                      | 30 px                                                              | line 250                                    |
| Rows                        | 6 px inset, padding 0 8px, 5 px radius                             | `List.module.css:38-50`                     |
| Symbol rows                 | kind icon; on the right the folder for classes, else the file name |                                             |
| Command rows                | search icon                                                        |                                             |
| "more" row                  | `… more <section>`                                                 |                                             |

**Fail-first** (`$S/dsbi/failfirst_A.log`, run A switched off 12 behaviours together; each failure below is at that test's own assertion):

| Test                                                                                                                                                             | Failed at      |
| ---------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------- |
| `everywhere::view::proofs41_stale::a_late_answer_of_an_old_query_is_not_shown`                                                                                   | `:47`          |
| `everywhere::view::proofs41_stale::the_popup_is_760_wide_and_leaves_24_px_beside_it_in_a_narrow_window`                                                          | `:86`          |
| `everywhere::view::proofs41_stale::the_list_is_as_high_as_its_rows_between_ten_rows_and_672_px`                                                                  | `:96`          |
| `workbench::tests::proofs41_e2e_search_everywhere::alt_up_and_alt_down_browse_the_tab_s_search_history_and_restore_the_draft` (starts with the tab's last query) | `:53`          |
| `…::tab_cycles_scopes_and_repeating_the_scope_action_toggles_non_project_items`                                                                                  | `:98`          |
| `…::labels_the_actions_tab_s_checkbox_and_assigns_a_shortcut_with_alt_enter` (the label)                                                                         | `:123`         |
| `…::searches_everywhere_when_nothing_is_found_in_the_project`                                                                                                    | `:144`         |
| `…::cmd_enter_on_text_opens_find_in_files_with_the_query`                                                                                                        | `:207`         |
| `…::cmd_enter_off_the_text_tab_opens_the_selection`                                                                                                              | `:228`         |
| `…::actions_run_from_the_popup_are_recent_and_the_footer_describes_the_selection` (its "the tab's last query comes back" check)                                  | `:180`         |
| `workbench::tests::proofs41_e2e_search_layout::tabs_and_query_fit_narrow_windows` (width)                                                                        | `:90`          |
| `…proofs41_e2e_search_layout::without_the_preview_the_results_list_shows_its_rows`                                                                               | `:71`          |
| `workbench::tests::find_action_says_what_to_do_when_it_has_nothing_to_list`                                                                                      | `mod.rs:10842` |

Three of these failed, but not at their own behaviour:

- `…proofs41_e2e_search_layout::mixed_file_and_action_rows_align_icons_and_names_and_keep_shortcuts_readable` failed only because the list was 0 px high, not on alignment.
- `…proofs41_stale::a_late_wide_answer_of_an_old_query_is_not_shown_after_a_new_query` failed at the widening, not the stale-answer guard.
- Remembering the query on activation was hidden by remembering on close.

**Never seen failing**, because run B died before it ran:

- browse ⌥↑/⌥↓;
- ⌥↵ running `settings.assignShortcut`;
- recent actions listed first;
- the header wrapping;
- the 16 px icon slot (alignment);
- the late wide answer;
- remembering on activation (the toggled-action test);
- remembering on close;
- the shortcut drawn beside an action.

**Left out:**

- preview;
- funnel filters and the file-set scope chooser;
- the Recent Searches menu;
- multi-selection;
- opening to the side;
- the ⌥↵ menu on rows that are not actions;
- the Open in Find Tool Window button;
- language-server symbols and endpoints;
- refresh when indexing ends;
- toggle switches in action rows;
- syntax colours of text matches;
- folder results.

## Piece 2: edited files in Recent Files

`workbench/editor_core.rs`: every edit except `External` puts the file's path in `edited`. `workbench/mod.rs`: `show_recent_files` passes it on, and `leave` clears it (`src/features/search/index.ts:544`, `resetTracking`). The second half goes further than the notes: their hypothesis 7 was wrong, `open_project` reuses the same window.

| Test                                                                                                                      | Failed before the code                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `workbench::tests::proofs41_recent_edited::recent_files_asked_for_again_lists_only_the_files_edited_in_this_window`       | yes, `:30`, run A                                                                                           |
| `workbench::tests::proofs41_recent_edited::the_edited_files_of_the_project_before_are_not_listed_after_another_is_opened` | not on its own behaviour: run A stopped it at `:59` (edits not recorded); the clear-on-leave half never ran |

## Piece 3: tests

Declared in `workbench/mod.rs`: `proofs41_e2e_search_everywhere`, `proofs41_e2e_search_layout`, `proofs41_recent_edited`. `view.rs` declares `memory`, `parts` and `proofs41_stale`.

Added:

- the late-wide-answer test;
- the list-height test;
- ⌘↵ off Text;
- the toggled-action test;
- the project-switch test.

The checkbox assertions now check the drawn labels: `search-everywhere-scope:<label>` is present for one label and absent for the other.

## Rows and spec cases

Not done. There were no gates and no `parity.json` check. The draft's mapping of the cases to native tests is in `$S/drafts/search_b_integration.md` §(c), and it is unverified.

## Hypotheses

**Measured:**

- The index finds `FindAction.kt` for "Find Action" (draft hypothesis 9).
- `Localization::set_locale("zh")` switches the labels and the narrow test passes in zh (hypothesis 10).
- `cmd-enter` is the primary modifier on this Mac (hypothesis 8).

**Read from the source, not measured:**

- History and recent actions are app-wide, not per window or per project, on both sides: `localStorage` in the webview, the `UiStorage` global natively. Tests get an in-memory `UiStorage::default()` from `start`.
- Hypothesis: Tauri windows share one `localStorage`. That is why I wrote no test of two windows not seeing each other's history.

## Not done at all

The shots in the real window, the gates, the report on rows and specs.

## Files edited

- `app-gpui/src/everywhere/view.rs`
- `app-gpui/src/everywhere/view/memory.rs`
- `app-gpui/src/everywhere/view/parts.rs`
- `app-gpui/src/everywhere/view/proofs41_stale.rs`
- `app-gpui/src/everywhere/recent_view.rs` (`marked_text` made `pub(crate)`)
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/editor_core.rs`
- `app-gpui/src/workbench/tests/proofs41_e2e_search_everywhere.rs`
- `app-gpui/src/workbench/tests/proofs41_e2e_search_layout.rs`
- `app-gpui/src/workbench/tests/proofs41_recent_edited.rs`

New scratch files: `$S/dsbi/switch.py` and the logs in `$S/dsbi/`. Files added since the last formatting: the toggled-action and project-switch tests are not formatted yet.

## To resume

1. You free disk space.
2. `python3 -I $S/dsbi/switch.py B on`.
3. `python3 -I $S/dsbi/switch.py B off`, then rebuild and run `cargo test --lib -- proofs41`, then `B on`.
4. The same with `C`.
5. Format the edited files.
6. The gates, the "after" filtered run and the shots.
