Both parts are done: all 12 `proofs15` tests run and pass, the three behaviours their pending tests waited for are built, and the Project view's options, panes and UI zoom are wired. The last gate run is clean except one failure in suite run 1, in `app_actions.rs`, a file I did not touch; it passed in suite run 2 and five times alone. Zoom does not scale the whole window, so its rows stay partial.

## 1. Gates (final tree)

| Gate                                           | Result                                             |
| ---------------------------------------------- | -------------------------------------------------- |
| clippy (`--all-targets -D warnings`)           | exit 0                                             |
| `cargo build`                                  | exit 0, 0 warnings                                 |
| Suite run 1                                    | FAILED: 7176 passed, 1 failed, 0 ignored, 152.27 s |
| Suite run 2                                    | ok: 7177 passed, 0 failed, 0 ignored, 153.60 s     |
| `cargo test --test web_view`                   | ok: 1 passed                                       |
| 47 added, declared or changed tests, by filter | 20 of 20 runs: 47 passed, 0 failed                 |

Failures seen at any time:

- **Final suite run 1:** `workbench::app_actions::tests::updates_the_running_projects_banner_after_a_backend_trust_transition` panicked at `app_actions.rs:1224`, "the banner is gone". I did not touch this test. It passed in suite run 2 and five times alone.
  - Hypothesis: the assertion reads `debug_bounds` right after `run_until_parked`, before a frame is drawn again, so a late frame under load fails it. To confirm, add `window.refresh()` before the assertion and rerun the suite.
- **First gate attempt (stopped and restarted):** `workbench::context_menus::tests::move_to_in_a_tool_windows_options_moves_it` failed. It asserted that the Project tool window's Options menu was exactly `[Move To, Maximize, -, Hide]`, and the new options rows now come first. I changed the test to check that the menu starts with "Appearance" and still ends with `-, Move To, Maximize, -, Hide`. It is in the 20× filter.
- `proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft` did not fail in any run.

## 2. Part A: `proofs15`

I declared the 10 files where the notes say. All 12 tests passed on their first run, without changes.

**Paste from History follows the keymap.**

- Change: `app-gpui/src/editor/paste_history_view.rs` `claim_key` now compares the keystroke with `bindings_for("editor.pasteHistory.paste" / ".remove")` instead of fixed Enter, Delete and Backspace. Backspace still edits a non-empty filter. The arrow and page keys stay.
- Webview source: `src/editor/core/view/PasteHistoryDialog.tsx:127-139`.
- Test: `workbench::editor_extras::tests::proofs15_paste_history::the_paste_history_keys_are_the_ones_the_keymap_binds`.

**Implement, Generate and Override re-indent what they insert.**

- Change: `app-gpui/src/workbench/editor_language.rs`. `apply_action` now runs `reformat_generated` after every action except `source.organizeImports*` and `source.fixAll*`. It waits for the edit, then takes the changed spans bottom up and skips one-line spans. A span under a `{` is shifted with `indent_under_brace`; any other span is range-formatted by the new `format_lines`. `format_lines` skips read-only and do-not-format files, keeps only edits inside the span, rejects a dedent to column 0 and stops if the text moved meanwhile. The format options are now shared through `editor_format_options`.
- Webview source: `src/editor/lsp/intentions.ts:182-219` and `src/editor/lsp/format.ts:201-229`.
- Second fix, in `app-gpui/src/editor_lsp/intentions.rs` `changed_lines`: it now drops the lines both texts start and end with before its line diff, as the webview's diff does. Before, a block ending in `}` inserted above the class's `}` gave the wrong span (1,2 instead of 1,3), so the inserted `}` was not indented. Webview source: `intentions.ts:256-271`.
- Tests:
  - `workbench::editor_language::tests::proofs15_implement::implement_methods_indents_the_inserted_members_under_the_brace`
  - `workbench::editor_language::tests::proofs15_implement::implement_methods_range_formats_inserted_lines_that_are_not_under_a_brace` (new)
  - `editor_lsp::intentions::tests::spans_an_inserted_block_whose_last_line_equals_the_line_below_it` (new)
- Difference left: native waits up to 25×20 ms for the edit to arrive (the existing `edited_since`); the webview waits up to 100×20 ms (`intentions.ts:207`).

**Fold by default.**

- Change: `app-gpui/src/editor/folds.rs` gains `Folds::folded_by_default`: imports; a comment that starts at or above the first line of code is the file header (a `#!` line is not code); other comments are documentation comments. `app-gpui/src/editor/core_actions.rs` applies it once, on the editor's first fold answer, reading the settings at that moment. When the caret is outside the folded regions, the view scrolls by the number of rows folded above it, so the line the file was opened at stays in view.
- Webview source: `src/editor/core/model/document.ts:555-598`.
- Test accessor `Editor::painted_fold_markers`, test builds only, in `app-gpui/src/editor/view.rs`.
- Tests:
  - `workbench::editor_extras::tests::proofs15_folding::a_file_opens_with_the_regions_fold_by_default_names_folded`
  - `workbench::editor_extras::tests::proofs15_folding::open_chevrons_show_only_while_the_pointer_is_over_the_gutter`
  - `workbench::editor_extras::tests::proofs15_folding::folding_by_default_keeps_the_line_a_file_is_opened_at_in_view` (new; it fails with the scroll correction removed)
  - `editor::folds::tests::fold_by_default_names_the_header_the_imports_or_the_documentation_comments` (new)
- Fixture change: measured, the backend offers no fold for a run of Rust `///` lines, so the test now uses a `/** … */` block as its documentation comment. It still checks that documentation comments fold by default.
- Differences left:
  - The webview scrolls the caret to the centre; native keeps the caret's row where it was.
  - Hypothesis: a caret inside a default-folded region moves to the fold's start (`apply_fold_edit`), where the webview leaves it in place. Untested; a test opening at line 1 of a folded header would confirm it.

No `_pending` file is left. I moved the three pending tests into their declared files and deleted `proofs15_folding_pending.rs`, `proofs15_paste_history_pending.rs` and `proofs15_implement_pending.rs`.

## 2b. Part B: Project view options, panes, UI zoom

Applied, re-anchored on the current code:

- Module lines in `project_view/mod.rs`, `theme/mod.rs` and `workbench/mod.rs`.
- `theme/appearance.rs`: `apply_zoom` runs after the custom font sizes, so they scale too.
- All `project_tree.rs` hunks (§3a–3k).
- In `workbench/mod.rs`: the field, the registration calls, `watch_tree_requests` and `project_view_tabs_changed`.
- `branches.rs`: `feed_changed_files`, after `annotations_vcs_changed`.
- `context_menus.rs`: the Options menu rows.
- `tree_clicks.rs`: one clippy fix (`is_multiple_of`).
- Two test fixes in `project_view_options.rs`:
  - Each test drops its own handle on the old tree before `reopen`. Holding it kept the tree alive, so its expansion was never written.
  - `reopen` waits for the root listing only in the Project and Project Files panes.

The Options menu follows `src/features/project-tree/HeaderActions.tsx:26-90`, minus Show Members and the Unity rows.

Proving tests, all in `workbench::project_view_options::tests::`: `opens_files_on_double_click_and_previews_on_single_click_only_when_enabled`, `expands_and_collapses_folders_on_single_click_only_when_enabled`, `follows_tab_switches_with_always_select_opened_file`, `autoscroll_to_source_opens_the_file_the_selection_moves_to`, `the_options_menu_lists_the_view_options_and_they_survive_reopening_the_project`, `switches_panes_and_remembers_the_choice`, `find_usages_of_file_searches_the_selected_file`, `reformat_and_optimize_imports_in_the_tree_change_the_selected_file`, `reload_from_disk_reads_the_selected_file_again`. Zoom: `workbench::zoom_actions::tests::the_zoom_actions_step_the_ide_zoom_and_the_window_draws_at_it` and `…::quick_switch_scheme_switches_the_theme_and_the_editor_color_scheme`.

Not built:

- `project.option.showMembers`, `project.renameInline`, `project.viewMode.{scratches,unity}`.
- The "Project ▾" pane switcher.
- The theme preview on hover in Quick Switch.

Each actionable `view.*` id has exactly one `registry.handle`; `pub mod quick_switch` does not collide. ⌥⌘Y: `project.reloadFromDisk` has `keyWhen: projectViewFocus` (`assets/gen/actions.json`), and native applies `keyWhen` (`platform/actions.rs:254-256`). Hypothesis: ⌥⌘Y in the editor still runs `file.reloadFromDisk`; untested.

**Zoom, measured in my own binary's window:**

- Screenshots: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d107/insp/shots/zoom-z100-1.png` and `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d107/insp/shots/zoom-z150-1.png`.
- **Scaled at 150 %:** the UI text (title bar, tool window title, tree rows, tab, status bar), row and header heights, the stripe width, the gutter width.
- **Not scaled:**
  - The editor's text and line numbers. By reading the code: `Editor::metrics` uses the Editor › Font size in pixels (`facts.font.size`), not a theme token.
  - Every icon.
  - Literal pixel sizes in views: 3007 `px(` occurrences (upper bound). Most by top-level directory: vcs 442, database 342, workbench 310, settings_view 168, run 152, agents 147, github 135, editor 130, everywhere 128, editor_lsp 119.
- GPUI has no window-wide scale: `set_scale_factor` exists only in test builds, and `rem_size` affects only `rems()`, which the crate never uses (0 occurrences). The webview applies a whole-document zoom (`appearanceRuntime.ts:58-67`), so the zoom rows stay partial.

## 3. Inventory rows (`parity.json`; I did not edit `docs/`)

| Key                                                                                                                                                                                                                                 | State                  | Proving test(s), or what is missing                                                                                                                       |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:code.{collapseBlock,collapseRecursively,expandRecursively,fold,foldAll,foldSelection,foldToggle,unfold,unfoldAll}`                                                                                                          | done                   | the four `proofs15_folding` tests                                                                                                                         |
| `action:code.implementMethods`                                                                                                                                                                                                      | done                   | the three `proofs15_implement` tests                                                                                                                      |
| `action:code.reformatDialog`                                                                                                                                                                                                        | partial                | `proofs15_reformat::reformat_file_with_optimize_imports_organizes_the_imports_before_formatting`; missing: hiding Optimize imports for a plugin formatter |
| `action:editor.pasteHistory.paste`, `action:editor.pasteHistory.remove`                                                                                                                                                             | done                   | `proofs15_paste_history::the_paste_history_keys_are_the_ones_the_keymap_binds`                                                                            |
| `point.registerOverlay:editor.pasteHistory`                                                                                                                                                                                         | partial                | the three `proofs15_paste_history` tests; missing: the preview's grammar and font                                                                         |
| `action:file.reloadAllFromDisk`                                                                                                                                                                                                     | done                   | `proofs15_reload_all::reload_all_from_disk_reads_the_repository_again`                                                                                    |
| `action:github.pullRequest.refresh`                                                                                                                                                                                                 | partial                | `proofs15_github_refresh::refresh_pull_request_replaces_what_the_tab_shows_by_the_new_answer`; missing: timeline and review-thread reload                 |
| `action:plugins.reloadAll`                                                                                                                                                                                                          | done                   | `proofs15_plugins::reload_plugins_reports_the_plugin_it_could_not_reload`                                                                                 |
| `action:terminal.splitAndMoveRight`                                                                                                                                                                                                 | done                   | `proofs15_terminal::split_and_move_right_moves_the_tab_into_a_new_group_beside_its_own`                                                                   |
| `ipc:term_is_busy`                                                                                                                                                                                                                  | done                   | `proofs15_terminal::close_session_asks_before_ending_a_shell_whose_foreground_runs_a_command`                                                             |
| `action:window.toggleFullScreen`                                                                                                                                                                                                    | done                   | `proofs15_full_screen::toggle_full_screen_puts_the_window_in_full_screen_and_takes_it_out`                                                                |
| `point.registerTitleItem:vcs.mergeRebase`                                                                                                                                                                                           | done                   | `proofs15_merge_resolve::resolve_conflicts_on_the_title_bar_opens_the_conflicts_dialog_again`                                                             |
| `action:plugins.installFromDisk`, `context:run.updatableRunning`, `ipc:java_resolve_launch`, `ipc:plugin_grant`, `ipc:plugin_list`, `ipc:plugin_run_command`                                                                        | partial, unchanged     | as in the notes                                                                                                                                           |
| `point.registerEditorKind:pluginLog`, `point.registerMenuGroup:agents.send`, `point.registerMenuGroup:plugins`, `point.registerMenuGroup:search.bookmarks`, `point.registerOverlay:plugins.consent`, `point.registerToolWindow:run` | not reached, unchanged | as in the notes                                                                                                                                           |
| `action:project.option.{previewTab,singleClickOpen,singleClickExpand,autoscrollFromSource,autoscrollToSource}`                                                                                                                      | done                   | the click, follow, autoscroll and Options-menu tests above                                                                                                |
| `action:project.viewMode.{project,projectFiles,openFiles,changedFiles}`                                                                                                                                                             | done                   | `switches_panes_and_remembers_the_choice`; the "Project ▾" switcher is not built                                                                          |
| `action:project.findUsages`, `action:project.reformatCode`, `action:project.optimizeImports`, `action:project.reloadFromDisk`                                                                                                       | done                   | the matching `project_view_options` tests                                                                                                                 |
| `action:view.zoomIn`, `action:view.zoomOut`, `action:view.resetZoom`                                                                                                                                                                | partial                | `the_zoom_actions_step_the_ide_zoom_and_the_window_draws_at_it`; missing: editor text, icons and literal pixel sizes                                      |
| `action:view.quickSwitchScheme`                                                                                                                                                                                                     | partial                | `quick_switch_scheme_switches_the_theme_and_the_editor_color_scheme`; missing: the theme preview on hover                                                 |

All test names in this table are under `workbench::<module>::tests::`.

## 4. Hypotheses

- Rust `///` runs get no fold because the walk in `crates/ide-language/src/syntax/folding.rs:110-118` also visits each comment's `doc_comment` child. That child is not on its own line, so it flushes the run, and the webview would have the same gap since it calls the same backend. To confirm, a unit test in `ide-language` that folds `"/// a\n/// b\nfn f() {}\n"` and expects a Comment fold for lines 0–1.
- The trust banner failure is a missed frame (command in §1).

## 5. False or stale in the notes and briefs

- The Part B notes say to run `cargo fmt`; I did not.
- The Part B anchor in `branches.rs` was stale: the code is now followed by `annotations_vcs_changed`.
- The `proofs15` folding fixture assumed `///` folds; measured, it does not.
- `changed_lines` gave the wrong span for a block ending in `}` above a `}` (fixed).
- Part B "tests likely to fail" says the reopen needs the expansion written on release. That code already existed; the tests held the old tree, which kept it alive.
- The Part B draft's zoom test still passes, although editor text does not scale.

`git status --short docs/ src/ src-tauri/ e2e/ crates/` prints nothing. The `proofs16_*` drafts are untouched and undeclared. Nothing is committed.
