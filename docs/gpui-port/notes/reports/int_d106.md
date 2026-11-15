## D106 integrator report: `proofs14` (Part A) and Annotate (Part B)

Both parts are done and all the gates you set were run. The only problem: run 1 of the suite had one failure, in a test I did not touch. It passed in run 2 and three times on its own. Nothing is committed.

### 1. Gates (measured, logs in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d106/`)

- **Suite run 1** (`suite_1.txt`): `FAILED. 7138 passed; 1 failed; 0 ignored … finished in 207.61s`.
  - The failure was `workbench::tests::proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft`, at `src/workbench/mod.rs:7767`: "the dialog did not happen within thirty seconds (ran out after 40.41s; reached later: None)".
  - Re-run alone three times: 3 of 3 `ok`. It also passed in run 2.
  - This run took 207 s against 149 s for run 2. Hypothesis: the dialog's wait timed out under load. Confirm with `cargo test --lib proofs13_agents_failed_send` repeated while the full suite runs.
- **Suite run 2** (`suite_2.txt`): `ok. 7139 passed; 0 failed; 0 ignored … 148.76s`.
- **Twenty runs by filter** (`twenty_1..20.txt`): filter `proofs14_ workbench::annotate editor::view::annotations in_a_git_repository_the_editor_menus_git_rows_are_enabled`. Every run was `ok. 34 passed; 0 failed`, so 20 of 20 for each of the 34 tests; none failed even once.
- **`cargo test --test web_view`**: `ok. 1 passed`.
- **`cargo clippy --all-targets -- -D warnings`**: exit 0.
- **`cargo build`**: exit 0, 0 warnings.
- No edits under `src/`, `src-tauri/`, `docs/`, `e2e/` or `crates/`. No `cargo fmt` was run: I formatted only the `git diff --name-only` files plus the drafts handed to me, through `fmt.sh`; only `proofs14_plugins.rs` changed.

### 2. Part A: `proofs14`

**Declared**, each with a `mod` line added at the anchor the notes give:

| File                                     | Tests | Result                  |
| ---------------------------------------- | ----- | ----------------------- |
| `github.rs` → `proofs14_github_window`   | 4     | passed on first compile |
| `list_panel.rs` → `proofs14_ui`          | 3     | passed on first compile |
| `plugin_runtime.rs` → `proofs14_plugins` | 3     | after the changes below |
| `plugins_page.rs` → `proofs14_page`      | 7     | passed on first compile |
| `terminal/view.rs` → `proofs14_ime`      | 4     | passed on first compile |

**Trap test was wrong, not the app:** `reports_command_failures_but_not_twice_for_plugins_that_trapped`.

- The draft assumed one trap leaves a plugin Failed. The host only stops a plugin after `max_failures` (3) crashes in a row (`crates/ide-host/src/plugins/mod.rs:2599`). After one trap the plugin is Inactive, so the app reports "panic failed".
- The webview does the same thing (`src/features/plugins/store.ts:405-411`: it stays quiet only when the status is `failed`), so I fixed the test, not the app.
- The test now uses a misbehave build whose version ends in `-broken`. Its activation fails, so the host disables the plugin and says so ("Plugin “Misbehave” was disabled"). The test asserts the status is Failed and that no "panic failed" notification appears.
- **The 0.6 s real-time wait is gone.** I added a small counter to `src/plugins.rs`: `Plugins::commands_answered()`, incremented in `run_command` when a command's answer comes back, succeeded or failed. The test waits until that count rises.

**Built: `plugins.updateAll`** in `src/workbench/plugins_window.rs::register_actions`.

- It calls `PluginsPage::update_all` on the page the window keeps for actions (`action_plugins_page`), which is never drawn. So, like the webview's `updateAllPlugins` (`src/features/plugins/index.ts:103-111` → `store.ts:251-260`), it does not open Settings.
- I moved its test, `update_all_on_the_announcement_installs_the_updates`, into `proofs14_plugins.rs` because it uses that file's helpers, and deleted `proofs14_plugins_pending.rs`.

**IME:** the four tests pass, so no double delivery happens in GPUI's test platform. Hypothesis: it also holds with a real macOS keyboard. That would need a manual check by typing in the real window; neither the test platform nor the inspection script's `keys` step goes through the OS key path.

**Left pending**, undeclared: `proofs14_github_window_pending.rs`.

- `opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab` waits for `github.createPullRequest` and the New Pull Request view.
- `registers_the_window_the_timeline_editor_and_the_actions` waits for 7 GitHub actions and the timeline editor.

### 3. Part B: Annotate

- **Declared:** `pub mod annotations;` in `src/editor/view.rs` and `mod annotate;` in `src/workbench/mod.rs`. Every hunk from the notes was re-anchored to the current code; all the anchors still matched.
- **Other files touched:** `editor/view.rs` (layout, field, edit mapping, press, hover, tooltip area, prepaint widening the gutter, painting, breakpoint dot moved right of the column), `workbench/mod.rs` (slot, registration, `publish_annotate_context`), `branches.rs` (re-blame when the head moves), and `context_menus.rs` (the column's own menu, `annotations_shown`, and the `GIT` list now 14 rows).
- **Webview it follows:** `src/features/vcs/blame.ts` and `src/features/vcs/actions.tsx:146-158, 456-467`. It calls the same `ide_host::ops::vcs::vcs_blame` as the adapter `src-tauri/src/commands/vcs.rs:70-78`.
- **The three hypotheses from the writer's report, now measured:**
  - **Two `EventEmitter` impls on `Editor`:** it compiles, and the click test proves `AnnotationEvent::Pressed` reaches the workbench.
  - **`publish_context` on tab change:** it runs in `Workbench::render` (`mod.rs:3811`). New test `the_toggle_follows_the_active_editor`: the toggle is off on another tab and on again back on the annotated one.
  - **The hover area does not swallow presses:** `a_click_on_an_entry_shows_its_commit_in_the_git_log` now hovers first, asserts the tooltip area is drawn, then clicks. The commit is selected in the Git log, no breakpoint is set and the caret stays.
- **Tests, all passing, 20 of 20 each:**
  - The 7 drafted end-to-end tests in `workbench::annotate::tests` plus the new one above.
  - The 4 model tests in `editor::view::annotations::tests`.
  - `context_menus` (27) and `operations` (85) also pass.
- **Real window**, my own debug binary only, on fresh temp `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR`, git global/system config off, stopped by PID. The temp repo has two commits, by Ada (2024) and Bob (2025). Screenshots are in `…/scratchpad/d106/insp/shots/`:
  - `a-02-annotated-1.png`: date and author per line, Bob's newer lines in the stronger colour, the column's own border, left of the line numbers.
  - `b-01-breakpoint-and-folds-1-crop.png`: the breakpoint dot on line 3 sits right of the column's border, and the fold chevrons right of the numbers. Nothing overlaps.
  - `b-02-tooltip-1-crop.png`: hovering shows "4c2356d3 Bob Stone Jun 15, 2025 07:00 AM / Change values".
  - The text is a proportional sans, smaller than the code, consistent with the interface font at `fs-small`. That it is exactly `--font-ui` at the webview's size comes from reading `prepare_annotations` (`ui_family`, `theme::length("fs-small")`), not from measuring pixels.
- **Left out, as the brief says:** Annotate in the diff and merge viewers, and inline blame. The "Cannot annotate" error path still has no test: there is no deterministic way to make `vcs_blame` fail.

### 4. Inventory rows

| Key                                                       | State                                                  | Proof                                                                                                                                                                                                                                                                                                                                                                                     |
| --------------------------------------------------------- | ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `unit:src/features/plugins/__tests__/pluginIcon.test.tsx` | done, 7 of 7                                           | `plugin_models::plugins_page::tests::shows_a_plugins_own_icon_in_its_row_and_details` plus the 6 tests in `plugin_models::plugins_page::tests::proofs14_page::*`                                                                                                                                                                                                                          |
| `unit:src/features/github/toolwindow/toolwindow.test.tsx` | partial, 6 of 8                                        | `workbench::github::tests::proofs14_github_window::{opens_each_pull_request_once_and_closes_back_to_the_previous_tab, targets_the_active_pull_request_tab_else_the_list_selection, opens_the_tool_window_for_change_repository_or_account_while_not_connected}` plus existing tests. Missing: the New Pull Request tab, the timeline editor, 7 actions                                    |
| `unit:src/features/github/ui/ui.test.tsx`                 | partial, 5 of 10 (+1 partial)                          | `github::list_panel::tests::proofs14_ui::{formats_relative_dates_like_github, lists_the_chosen_items_first_without_duplicates, chooses_one_item_and_closes_single}`, `workbench::github::tests::proofs14_github_window::shows_the_plain_date_and_time_with_pretty_formatting_off`. Missing: avatar images, label chips, the multi-choice chooser, Markdown links                          |
| `unit:src/features/plugins/__tests__/plugins.test.tsx`    | partial, about 44 of 61 (notes' count, not re-checked) | new: `workbench::plugin_runtime::tests::proofs14_plugins::{reports_command_failures_but_not_twice_for_plugins_that_trapped, announces_updates_with_actions_to_install_them_or_show_the_plugins, update_all_on_the_announcement_installs_the_updates}`, `plugin_models::plugins_page::tests::proofs14_page::filters_by_name_id_author_and_contribution`. Missing: the cases the notes list |
| `unit:src/features/terminal/imeInput.test.ts`             | partial, 9 of 20                                       | `terminal::view::tests::proofs14_ime::*` (4 tests). The other 11 cases test DOM events and have no native counterpart                                                                                                                                                                                                                                                                     |
| `action:plugins.updateAll`                                | done                                                   | `workbench::plugin_runtime::tests::proofs14_plugins::update_all_on_the_announcement_installs_the_updates`                                                                                                                                                                                                                                                                                 |
| `action:vcs.annotate`                                     | done                                                   | `workbench::annotate::tests::annotate_shows_the_date_and_author_of_each_line_and_closes_again`, `…::annotate_without_a_file_asks_for_one_and_with_a_path_opens_and_annotates_it`, `…::the_toggle_follows_the_active_editor`                                                                                                                                                               |
| `ipc:vcs_blame`                                           | done                                                   | `workbench::annotate::tests::annotate_shows_the_date_and_author_of_each_line_and_closes_again`, `…::a_new_head_commit_blames_the_file_again`, `…::the_columns_menu_copies_the_revision_and_closes_the_annotations` (re-blame after an edit sees the unsaved text)                                                                                                                         |

None of the last three keys is in `parity.json` yet; they are unticked in `INVENTORY.md` at lines 596, 758 and 2433. I did not edit either file, because `docs/` is off limits to me.

### 5. False or stale in notes and briefs

- The proofs14 notes' premise that one plugin trap makes the plugin Failed is false (see Part A).
- The notes say Update All should open the Plugins page. The webview does not open it, so mine does not either.
- The annotate notes say "Run `cargo fmt` after pasting", which contradicts the hard rules; I did not run it. Their compile risks did not happen: everything compiled first time, without warnings.
- The writer's report says the tooltip's date may lack a `toLocaleString` equivalent. In the real window it reads "Jun 15, 2025 07:00 AM", in local time.
