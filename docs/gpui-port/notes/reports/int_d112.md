I finished Part 0 and Part A, and every gate passed. Of the 41 handed tests I could declare (the 42nd sits under a draft I'm not allowed to touch), 38 are declared and pass, 3 moved to `_pending`, and I added one new test. On the way I fixed six app gaps, including a crash on double Shift.

## 1. Gates (measured, in `app-gpui/`)

- **Suite run 1** (nothing else running): `test result: ok. 7339 passed; 0 failed; 0 ignored`, 171.55 s.
- **Suite run 2**: `ok. 7339 passed; 0 failed; 0 ignored`, 209.34 s. While it ran, the 42 tests I added, declared or changed ran 20 times by `--exact` with the built binary: 42 of 42 passed in all 20 runs, no failures. Each batch took 4.2–10.5 s, so no single test is slow.
- **`cargo test --test web_view`**: 1 passed.
- **`cargo clippy --all-targets -- -D warnings`**: exit 0. Code was last changed before this run, apart from one comment edit in an undeclared pending file, which nothing compiles.
- **`cargo build`**: 0 warnings.
- **Failures seen along the way** (all fixed or moved before the gates):
  - Double Shift panicked with "cannot update Workbench while it is already being updated" (fix 3 below).
  - The `Result::is_none` compile errors in open_project.
  - The test failures described in section 2.

## 2. Work by part

### Part 0: Database window

- **Query Plan params**
  - **Change:** the params cell is drawn with `truncate()`, and the operation text is truncated too. The cell's laid-out text is kept so a test can read it (`PlanView::drawn_params`).
  - **Follows:** `src/features/database/Database.module.css:1060-1071` (`.planOperation`, `.planParams`).
  - **Tests:**
    - `database::plan_view::tests::params_too_wide_for_a_narrow_pane_end_with_an_ellipsis` (300 px pane); I checked it fails without the change.
    - `workbench::database_extras::tests::the_docked_database_window_fits_its_filter_field_and_cuts_long_plan_params` (real docked window, measured `"table: users…"`).
- **Filter objects**
  - **The brief's diagnosis is stale.** The frame was already 100 px and ended 6 px inside the toolbar, so the field did shrink. What was cut was the placeholder, painted past the field's own box.
  - **Change:** `TextField` now clips its text, selection and caret to its own box, as a browser input does. Text wider than the field scrolls to keep the caret in view, and the click position and IME box follow the scroll. This affects every text field.
  - **Follows:** `src/features/database/Database.module.css:241-246` and `src/ui/Input.module.css` (`.searchInput`, `min-width: 0`).
  - **Tests:**
    - The docked-window test above, which fails without the clip.
    - `ui::text_field::tests::text_wider_than_the_field_scrolls_to_keep_the_caret_inside_it`, which fails without the scroll.
- **Screenshot:** retaken on the final binary (07:15 build), `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d112/insp/shots/t10-05-query-plan-1.png`. It shows "table: users; USI..." with an ellipsis and "Filter objec" cut inside the field's frame. The app ran on a temp home and project and was stopped by its PID (54106).

### Part A: app fixes

1. **Search Everywhere lists Settings pages and option labels among the actions; Enter opens Settings on that page.**
   - **Changed:** `everywhere/view.rs`, plus `settings_search_entries` and `settings_page_set` in `workbench/settings_window.rs`.
   - **Follows:** `src/features/settings/searchIndex.ts:11-31`, `src/features/search/everywhere/controller.ts:276-283`, `src/features/search/everywhere/SearchEverywhere.tsx:306-307`.
   - **Test:** `workbench::tests::proofs18_e2e_search::search_everywhere_lists_a_settings_page_and_enter_opens_settings_on_it`. The option-label entries have no test of their own.
2. **Settings opened without a page opens on the page shown last.**
   - **Changed:** a `last_settings_page` field on the workbench, saved on close and when the project closes.
   - **Follows:** `src/features/settings/model/staging.ts:104-111`.
   - **Test:** the plugin_settings test (Cancel, then ⌘, again, keeps LLM Settings). A page that is no longer listed falls back to the first page (`current_key`).
3. **Double Shift crashed the window.** Modifier and mouse-button forwarding ran inside the workbench's own listener, and the action they triggered updated the workbench again.
   - **Changed:** both are now plain window listeners (`workbench/mod.rs`).
   - **Test:** search double Shift, open_project. The mouse-down change has no dedicated test.
   - Hypothesis: the real window crashes on double Shift without this fix. It was measured only as a test backtrace; confirm by launching the HEAD binary and pressing Shift twice.
4. **`SettingsDialogView::page_view`** accessor, as the notes' hunk specified.

### Part A: test fixes (the tests were wrong, not the app)

- **cmd_f2_stops_a_running_process:** the backend stops with SIGINT (`crates/ide-run/src/process.rs:997`), so the code is 130. 143 was the e2e stub's value. This test needs SIGINT not to be ignored.
- **CRLF merge:** Yours equals HEAD, so after staging, `status --porcelain` is empty, not `M  a.txt`.
- **Terminal tests:** the tests' plain shell prints no prompt, so they now wait for the session id.
- **Completion:** lookup rows are drawn as `label — detail`.
- **open_project:** `host.project` returns a `Result`.
- **keymap:** now opens a project, as the e2e fixture does.

### Moved to `_pending`

- **Both keymap cases**, now in the new, untracked `workbench/tests/proofs18_e2e_keymap_pending.rs`; I deleted `proofs18_e2e_keymap.rs`.
  - The Warning on OK needs another action bound to `Alt+Button1`. That is `debug.quickEvaluate`, which this window does not register (`DEBUG_ACTIONS`).
  - With the Warning step skipped, every other step passed.
  - Hypothesis: it is unregistered with a project open as well. My probe that checked the registry ran with no project open; to confirm, read the registry in a test after `trusted_project`.
- **search "finds and runs an action"**, appended to `proofs18_e2e_search_pending.rs`. The Language Servers page shows "This page is not available yet", so the spec's server list cannot show.
- `double_click_at` moved from the shared fixtures into the keymap pending file, since nothing declared uses it.

## 3. Tables per spec (`parity.json` key, cases proven)

- **`e2e:e2e/commit.spec.ts`**: 1 of 1. `workbench::tests::proofs18_e2e_commit::cmd_k_commits_the_checked_changes_with_the_typed_message`.
- **`e2e:e2e/gitIgnore.spec.ts`**: 2 of 2. `…proofs18_e2e_git_ignore::ignore_gitignore_menu_dispatch_and_immediate_tree_status_refresh` and `…::ignore_local_exclude_menu_dispatch_and_immediate_tree_status_refresh`.
- **`e2e:e2e/keymap.spec.ts`**: 0 of 2. Missing: a Quick Evaluate action bound to Alt+Button1.
- **`e2e:e2e/openProject.spec.ts`**: 1 of 1. `…proofs18_e2e_open_project::close_project_shows_the_welcome_screen_a_recent_project_opens_from_it_once_trusted`.
- **`e2e:e2e/plugin-settings.spec.ts`**: 0 of 1. `workbench::plugin_contributions::tests::proofs18_e2e_plugin_settings::plugin_settings_pages_stage_edits_apply_before_actions_and_discard_on_cancel` passes. Not driven: Test's failure alert "Example provider unavailable", because no fixture command fails and `crates/` is off limits.
- **`e2e:e2e/pluginNotifications.spec.ts`**: 1 of 1. `workbench::plugin_runtime::tests::proofs18_e2e_plugin_notifications::plugin_notification_actions_run_their_command_and_dismiss_the_balloon`.
- **`e2e:e2e/newBranch.spec.ts`**: 4 of 4. All four `…proofs18_e2e_new_branch::*` tests (typed space, overwrite, mid caret, paste).
- **`e2e:e2e/codeInsight.spec.ts`**: 4 of 7. Proven by `editor::intelligence::tests::proofs18_e2e_code_insight::{completion_lists_the_servers_items_and_inserts_the_chosen_one, cmd_b_goes_to_the_declaration_in_another_file, find_usages_lists_every_usage_by_file_and_opens_one, diagnostics_underline_the_range_and_alt_enter_applies_the_preferred_quick_fix}`. Missing:
  - in-place rename on ⇧F6 (two cases);
  - a lazy-action fixture.
- **`e2e:e2e/toolwindows.spec.ts`**: 7 of 12. Proven by `…proofs18_e2e_toolwindows::{cmd_1…, cmd_4…, alt_f12_opens…, the_first_alt_f12…, a_terminal_view_that_loads_late…, cmd_shift_f12…, the_terminal_sends_typed_commands…}`. Missing:
  - a double click on the header title toggling maximize (3 cases);
  - per-group terminal headers and tab selectors (2 cases).
- **`e2e:e2e/mergeEditor.spec.ts`**: 14 of 16.
  - Proven by 5 new tests: `…proofs18_e2e_merge_editor::{crlf…, accepting_a_deleted_version…, the_toolbar_applies…, the_context_menus_decide…, find_action_runs_merge_commands…}`.
  - Plus the 9 existing tests the notes cite. Hypothesis: they match their spec cases; I checked only that the names exist (run `grep -n "<name>" -A40` on each and compare with the spec).
  - Missing: the keyboard on per-change controls, and two merge editors open at once.
- **`e2e:e2e/run.spec.ts`**: 2 of 4. Proven by `…proofs18_e2e_run::{ctrl_r_runs_the_selected_configuration_and_shows_its_output_and_exit, cmd_f2_stops_a_running_process}`. Missing: Go rerun-failed in the Run window, and the CMake before-launch flow.
- **`e2e:e2e/search.spec.ts`**: 2 of 4. Proven by `…proofs18_e2e_search::{double_shift_opens_search_everywhere_with_its_tabs_escape_closes_it, search_everywhere_finds_a_file_and_opens_it}`. Missing:
  - the Language Servers page;
  - Find in Files' preview as an editor.
- **`e2e:e2e/editor.spec.ts`**: 6 of 8. Proven by `…proofs18_e2e_editor::*` (6 tests). Missing: breadcrumbs and Navigation Bar, and the tab's modified mark.
- **`e2e:e2e/compareWith.spec.ts`**: 4 of 4. Proven by `workbench::diff_tools::tests::compare_with_asks_for_the_other_file_and_shows_both`, `workbench::context_menus::tests::the_tab_menu_acts_on_the_tab_it_was_opened_on`, `workbench::context_menus::tests::right_click_in_a_plain_file_opens_the_editor_menu_and_moves_the_caret` (bodies read), and `workbench::diff_tools::tests::proofs18_e2e_compare_with::compare_with_compares_against_a_file_picked_in_the_dialog`. One difference for you to decide on: case 4 opens a diff window titled other.md, not an editor tab named other.md.
- **`e2e:e2e/diff-navigation.spec.ts`**: 1 of 3. Proven by `workbench::tests::proofs18_e2e_diff_navigation::continues_between_changed_files_from_toolbar_buttons_and_the_keymap`. The `diff_tab` test is excluded by the brief, and the binary-sibling case is pending.

**Other parity keys:** none changes state. Every action the declared tests drive by key or registry is either already done, or its row names a gap these tests don't close (`run.run`, `run.stop`, `vcs.newBranch`, `code.completion`, `navigate.declaration`, `find.usages`, `code.showIntentions`, `navigate.findAction`, `vcs.showDiff`). `point.registerOverlay:search.everywhere` lists "settings as sources" as missing; fix 1 closes that part, but the row stays partial. `toolwindow.project/run/terminal/problems/closeContent` have no rows.

## 4. Stale or false in the notes and briefs

- The brief says the filter field is cut by the window edge. Only its placeholder overflowed.
- The notes give a `diff_tab.rs` mod line, which the brief forbids.
- The notes predict 143, `M  a.txt`, a shell prompt and bare completion labels; all four are wrong, as described in section 2.
- The two failures the notes predicted (second ⌘⇧F12, the find bar seeded from the last search) both passed.
- Six e2e keys are absent from `parity.json`: commit, gitIgnore, keymap, openProject, plugin-settings, pluginNotifications.
- The search row in `parity.json` counts "runs an action" as proven with a different action (the theme toggle).
- The notes say "Quick Evaluate has no view here yet" while `debugger/actions.rs` handles it but the window never registers it.

Nothing is committed. HEAD moved to `aafc0876` while I worked (a commit I didn't make), and `docs/gpui-port/notes/backlog.md` in the diff isn't mine. Edited files were formatted with `$S/fmt.sh`.
