## Report

### 1. Suite, clippy, compile errors, scope

- I ran the whole suite five times on the final tree. Every run gave 6728 passed, 0 failed, 0 ignored, in 93–95 s each.
- `cargo clippy --all-targets -- -D warnings` is clean. No line I added is over 120 columns.
- Compile errors:
  - Part 0: 3, all the same kind (`AppContext` was not imported in a test).
  - Part 1: none. The drafts compiled once the hunks were applied.
- Formatting: I ran rustfmt only on the files I edited. It also follows `mod` lines, so it reached the modules those files declare. I checked: `git status` lists the same 30 files I edited (plus the new `theme/installed_fonts.rs`) and no other tracked file.
- Untracked files I did not create appeared in the worktree between 02:53 and 03:09. Another writer is probably active there. I left them alone:
  - `workbench/{app_actions,plugin_runtime,plugins_window}/tests/`
  - `settings_view/custom_controls/tests/`
  - `workbench/tests/proofs6_{agents,attach,database,workbench}.rs`
  - `workbench/debug/tests/proofs6_debug.rs`
- Nothing was edited under `docs/`, `src/`, `src-tauri/` or `crates/`.

### 2. Part 0: installed fonts

**The 13 call sites before the change:**

Read on every frame (6):

- `editor/view.rs:639` `Editor::code_font`, from prepaint and `text_style`. Only the tests ever set `InstalledFontNames`, so in the app this asked the system on every frame.
- `vcs_diff/view.rs:93`, from render and prepaint.
- `vcs/merge_view.rs:383`, from render and prepaint.
- `terminal/view.rs:473` `TerminalView::font`, from prepaint.
- `code_insight/usages_popup.rs:310`, through `preview_pane` in render.
- `code_insight/usages_view.rs:832`, through `tree` in render.

Read from render, but not compiled:

- `database/dialog_parts.rs:25` `sql_preview`, used by the undeclared drafts.

Read once, when the view is built (5):

- `everywhere/bookmarks_popup.rs:205`
- `project_view/file_dialogs.rs:234` (two constructors)
- `plugin_models/plugins_page.rs:102` (two constructors)
- `vcs/prompt_dialogs.rs:150` (two constructors, plus `git_form_dialog.rs:90`)
- `vcs/branches_popup.rs:329`

Read once per picker load, off the window's thread:

- `settings_view/custom_controls.rs:371`

**After the change:**

- The one source is the `InstalledFontNames` global, now in `theme/installed_fonts.rs`.
- `load(cx, refresh, then)` reads it once on the background executor; `openidea_gpui::init` starts that read. A second request waits for the read already running.
- The Settings refresh button reads again, as the webview's `loadSystemFonts(true)` does. `FontCatalog` is now built from this list.
- Every helper uses `mono_family` or `first_installed`. Until the list arrives they use the theme's first `--font-mono` family, and the windows repaint when it arrives.
- `grep all_font_names` finds one call: `installed_fonts.rs:50`.

**Guard and test:**

- `system_font_names` is the only function that calls `all_font_names`. It counts its calls and panics in test builds; in test builds `load` uses what the test set.
- Test: `theme::installed_fonts::tests::drawing_the_editor_a_diff_and_a_terminal_reads_the_system_fonts_at_most_once`.
  - It draws the three views for 5 frames and checks no list read began. It then reads the list once, draws again, and checks the count is 1, the system count is 0, and the editor's family changed.
  - The system counter is 0 by construction in test builds. The real proof is the panic guard plus the list-read counter.
- The existing font tests pass.

**Caveats and other per-frame reads:**

- Dialogs built before the list arrives keep the theme's default family.
- `TerminalView::font` (prepaint), `shows_status_bar` and `shows_tool_window_bars` (render) call `settings::global(cx)`. Each call copies the whole `Settings` struct under a read lock. Hypothesis: this costs something on every frame. A profile of a frame would confirm it.

### 3. Part 1: actions

All tests are in `workbench::editor_extras::tests` unless marked otherwise.

| Action                                                   | Test                                                                                                                                                                           | IPC                                               | Status                                                                                        |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `code.expandToLevel1..5` and `code.expandAllToLevel1..5` | `expand_to_level_folds_the_regions_nested_deeper_than_the_level`                                                                                                               | `doc_folding` (doc.rs)                            | done                                                                                          |
| `editor.contextInfo`                                     | `context_info_shows_the_header_lines_of_the_code_around_the_caret`                                                                                                             | `doc_folding`                                     | partial: no syntax-tree fallback                                                              |
| `navigate.customRegion`                                  | `custom_folding_lists_the_regions_and_moves_the_caret_to_the_chosen_one`                                                                                                       | none                                              | done                                                                                          |
| `code.unwrap`                                            | `unwrap_replaces_the_statement_around_the_caret_by_its_body`                                                                                                                   | `syntax_support` and `syntax_node_at` (syntax.rs) | done                                                                                          |
| `code.expressionTypeInfo`                                | `expression_type_info_shows_the_type_of_the_expression_at_the_caret`                                                                                                           | `lsp_hover`                                       | partial: an editor hint, not the doc popup                                                    |
| `code.reformatDialog`                                    | `reformat_file_reformats_the_scope_chosen_in_the_dialog`, plus `editor_lsp::reformat_dialog::tests::*`                                                                         | `lsp_format`, `lsp_code_actions`                  | done                                                                                          |
| `find.rerun`                                             | `rerun_search_finds_what_was_written_since_the_search`                                                                                                                         | `search_start`                                    | done                                                                                          |
| `navigate.nextOccurrence` and `prevOccurrence`           | `next_occurrence_steps_through_the_results_of_the_tool_window`, `next_occurrence_says_which_of_how_many_in_the_status_bar`, `the_occurrence_keys_do_nothing_without_a_project` | none                                              | partial: the Find window does not step                                                        |
| `editor.toggleRainbowBrackets`                           | `rainbow_brackets_colour_the_brackets_by_their_depth`, plus `editor::extras::tests::*`                                                                                         | `settings_set`                                    | done: `syn-bracket-1..3` by depth, `syn-invalid` when unmatched, strings and comments skipped |
| `project.refresh`                                        | `reload_all_from_disk_reads_the_project_tree_again`                                                                                                                            | `fs_read_dir`, same as `reloadAll`                | unproven, see below                                                                           |

In every row the native call is the same backend operation the adapter calls. The `project.refresh` id was not registered anywhere else, so it is registered once.

**Not registered:**

- `editor.toggleSemanticHighlighting`: the native build has no semantic-token pipeline, so it is not the one hunk the notes designed.
- `editor.addOrRemoveCaret`: registering it made the window's mouse capture run the empty handler and swallow the ⌥⇧-click. The measured result was `|ab ab` instead of `|ab a|b` in `editor::command_actions::tests::caret_and_occurrence_commands_run_through_the_registry`. The fix would be in `workbench/mod.rs`, which always passes `in_editor_text=false` to `input::mouse_down`.

**The six fixes, Zen mode and the click tests:**

- **Highlighted Usage fallback:** ⌃⌥↓ and ⌃⌥↑ step the tool window's results, proven in `next_occurrence_steps…`.
- **Usages count and status message:** `the_status_bar_shows_the_highlighted_usages_count` and `next_occurrence_says_which_of_how_many…` ("1 of 3", gone after 8 s).
- **Reload All from Disk:** reaches `project.refresh`. Three test variants (a plain file, a folder linked from outside, a git-ignored folder) showed the new file before the reload ran. The file watcher refreshes the tree anyway, so no test can tell the reload apart from it.
- **`pasteHistoryListFocus`:** `the_paste_history_keys_are_enabled_while_its_dialog_has_the_keyboard`. Backspace with text in the filter edits the filter; on an empty filter it deletes the chosen entry; Delete and Enter work: `the_paste_history_keys_edit_the_filter_or_act_on_the_chosen_entry`. Hypothesis: those keys may go through the dialog's own key claim rather than the context key. Removing the `publish_context` hunk and rerunning that test would confirm it.
- **Fold chevrons:** `a_click_on_the_fold_chevron_folds_and_unfolds_its_region` shows the caret, selection and breakpoints stay. `a_click_in_the_breakpoint_area_folds_nothing`. `plugin_contributions::tests::a_click_on_the_fold_chevron_runs_no_gutter_action_and_one_on_the_icon_folds_nothing` uses the plugin's own fold and icon on one line.
- **Create New Test…:** `go_to_test_offers_create_new_test_only_with_the_file_templates`.
- **Zen mode:** `distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text`. The tab strip, gutter and plugin icons are hidden, and the text is centred at the configured width × the width of "0", plus 22 px.

**Shortcuts (positive and negative tests in the same tests):**

- ⌘\* 1 and ⌘⇧\* 1 both reach their chords.
- ⌃⇧Q, ⌥⌘. and ⇧⌘⌦ (negative tested with the Find bar focused).
- ⌃⇧P and ⌥⇧⌘L.
- ⌘R picks `find.rerun` while the Find window is active and does nothing in the editor.
- ⌥⌘↓ and ⌥⌘↑ (negative in a window without a project).
- ⌃⌥↓ and ⌃⌥↑.
- Paste history: Enter, Backspace, Delete.

**Row lists:** none changed. `ROOT`, `GO_TO`, `GIT`, the tabs-menu test and the `missing` list all pass unchanged. The registered ids are saved in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/registered_ids.txt` (570 ids).

### 4. Strings, product fixes, changed tests, gaps

- **Invented text:** "No tests found" is removed. The webview (`gotoTest.ts`) opens "Choose Test for X (0 found)" with only the Create row. `templates.newFromTemplate` is not registered natively, so that row would do nothing; a file without tests now opens no chooser. That is partial.
- **Other strings:** every other user-visible string in the three drafted files is in the webview source or the text catalog.
- **Product changes beyond the notes:**
  - The gutter gets a 16 px fold column, the rightmost one, following `theme.ts:256` and `folding.ts:395`.
  - Presses in that column never set a breakpoint, following `breakpointGutter.ts:133`.
  - Plugin icons sit left of the fold column.
  - The column follows `folding.showOutline` (not tested when off).
  - Open chevrons show only while the pointer is over the gutter (not tested).
  - Zen mode follows `distractionFree.ts`, `EditorArea.tsx:305` and `theme.ts:174-179`. A right-click in the centring margin still opens the gutter menu (`context_menus.rs:676`).
- **Existing tests changed:** press points were moved left of the new fold column in `debug.rs` (`clicking_the_gutter_toggles_the_lines_breakpoint`, `alt_and_shift_in_the_gutter_…`) and `context_menus.rs` (`the_gutters_menu_adds_a_bookmark_on_the_clicked_line`). The drafted tests that read notifications now read the editor hint, which is where `editor.showHint` puts the text.
- **Notes that were false:**
  - The fold column as a 10 px strip inside the gutter overlapped the plugin icons.
  - Defects 1 and 2 needed no Usages test.
  - Assumption 8 was confirmed: the file watcher refreshes the tree on its own.
- **Designed but not built:** soft wrap, inline-completion ghost text, Recent Locations, pasted imports, semantic highlighting paint, the Find-window occurrence navigator, and the syntax-tree fallback for Context Info.
