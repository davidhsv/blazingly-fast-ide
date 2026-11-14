# proofs7 — test-only groups of the gap index

Nothing was compiled or run (no cargo). Statements about runtime behaviour are hypotheses until the integrator runs the tests; statements about code were read in this session.

## Files written (all new)

| file                                                                        | tests              | parent `mod tests`                                                                                       |
| --------------------------------------------------------------------------- | ------------------ | -------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/workbench/tests/proofs7_bookmark_digits.rs`                   | 2                  | `workbench::tests` (inline in `workbench/mod.rs`)                                                        |
| `app-gpui/src/workbench/project_view_actions/tests/proofs7_tree_options.rs` | 4                  | `workbench::project_view_actions::tests` (inline)                                                        |
| `app-gpui/src/workbench/project_view_actions/tests/proofs7_project_menu.rs` | 2                  | `workbench::project_view_actions::tests` (inline)                                                        |
| `app-gpui/src/workbench/context_menus/tests/proofs7_send_to_agent.rs`       | 1                  | `workbench::context_menus::tests` (inline)                                                               |
| `app-gpui/src/workbench/context_menus/tests/proofs7_toolbar_slots.rs`       | 2                  | `workbench::context_menus::tests` (inline)                                                               |
| `app-gpui/src/run/java_launch/tests/proofs7_separator.rs`                   | 1                  | `run::java_launch::tests` (inline)                                                                       |
| `app-gpui/src/platform/virtual_docs/tests/proofs7_relative_dir.rs`          | 3                  | `platform::virtual_docs::tests` (inline)                                                                 |
| `app-gpui/src/platform/keymap/tests/proofs7_vscode_preset.rs`               | 1                  | `platform::keymap::tests` (inline; `keymap/tests/` already holds `proofs_presets.rs`)                    |
| `app-gpui/src/platform/input/tests/proofs7_suspend.rs`                      | 1                  | `platform::input::tests` (inline)                                                                        |
| `app-gpui/src/editor/core_actions/tests/proofs7_extend_selection.rs`        | 1                  | `editor::core_actions::tests` (inline)                                                                   |
| `app-gpui/src/workbench/github/tests/proofs7_github_urls.rs`                | 2                  | `workbench::github::tests` (inline)                                                                      |
| `app-gpui/src/workbench/editor_language/tests/proofs7_hint.rs`              | 3                  | `workbench::editor_language::tests` (inline; `editor_language/tests/` already holds `proofs5_editor.rs`) |
| `app-gpui/src/workbench/tests/proofs7_quit.rs`                              | 2 (one macOS-only) | `workbench::tests`                                                                                       |
| `app-gpui/src/workbench/editor_core/tests/proofs7_read_only.rs`             | 2                  | `workbench::editor_core::tests` (inline)                                                                 |
| `app-gpui/src/workbench/services_actions/tests/proofs7_move.rs`             | 1                  | `workbench::services_actions::tests` (inline)                                                            |

Total: 15 files, 28 tests.

## mod lines to add

1. `app-gpui/src/workbench/mod.rs`, inside `mod tests {`, after the anchor `    mod proofs5_run;`:
   ```rust
       mod proofs7_bookmark_digits;
       mod proofs7_quit;
   ```
2. `app-gpui/src/workbench/project_view_actions.rs`, inside `mod tests {`, after the anchor `    use std::sync::Mutex;`:
   ```rust
       mod proofs7_project_menu;
       mod proofs7_tree_options;
   ```
3. `app-gpui/src/workbench/context_menus.rs`, inside `mod tests {`, after the anchor `    use std::path::PathBuf;` (the last `use` of that block, before `    const LABEL: &str = "w1";`):
   ```rust
       mod proofs7_send_to_agent;
       mod proofs7_toolbar_slots;
   ```
4. `app-gpui/src/run/java_launch.rs`, inside `mod tests {`, after the anchor `    use ide_lsp::jdtls::JavaCompileError;`: `    mod proofs7_separator;`
5. `app-gpui/src/platform/virtual_docs.rs`, inside `mod tests {`, after the anchor `    use super::*;` (the first line of that block): `    mod proofs7_relative_dir;`
6. `app-gpui/src/platform/keymap.rs`, inside `mod tests {`, after the anchor `    mod proofs_presets;`: `    mod proofs7_vscode_preset;`
7. `app-gpui/src/platform/input.rs`, inside `mod tests {`, after the anchor `    use std::rc::Rc;` (before `    struct Empty;`): `    mod proofs7_suspend;`
8. `app-gpui/src/editor/core_actions.rs`, inside `mod tests {`, after the anchor `    use std::time::Duration;` (before `    struct Fixture {`): `    mod proofs7_extend_selection;`
9. `app-gpui/src/workbench/github.rs`, inside `mod tests {`, after the anchor `    use std::cell::{Cell, RefCell};`: `    mod proofs7_github_urls;`
10. `app-gpui/src/workbench/editor_language.rs`, inside `mod tests {`, after the anchor `    mod proofs5_editor;`: `    mod proofs7_hint;`
11. `app-gpui/src/workbench/editor_core.rs`, inside `mod tests {`, after the anchor `    use std::time::Duration;` (before `    fn start(cx: &mut TestAppContext)`): `    mod proofs7_read_only;`
12. `app-gpui/src/workbench/services_actions.rs`, inside `mod tests {`, after the anchor `    use std::path::PathBuf;`: `    mod proofs7_move;`

One accessor hunk is needed (`EditorIntelligence::hint_view`, group 25 below); every other accessor used exists (read in this session): `Workbench::{bookmarks, editor, open_project_file, project_tree, agents_store (pub(super) in workbench::agents), open_menu, vcs_state}`, `BookmarkStore::data().{items,paths}`, `Editor::{offset_of, select, caret_position, text, path}`, `KeymapState::primary_key_binding`, `menu::gpui_keystroke` (pub), `ProjectTree::{toggle, rows}`, `ViewOptions` via the registry, `action_toggled`, `AgentsStore::composer` and `agents::store::HOME`, `context_menus::tests::{open, Fixture (private fields, reachable from a child module), Setup, at_offset, right_click, choose, click, disabled, HELLO, LABEL}`, `plugins_window::test_support::{start_with_plugins, plugin_copy, link}`, `project_view_actions::tests::{trusted_project, select_in_tree, right_click_row, choose_in_menu, project_tree_of, tree_labels, text_of, close_project}`, `platform::input::{suspend, resume}` and its tests' `setup`, `run::java_launch::tests::{config, strings}`.

## Per group

### Group 1 — Bookmark digit actions (18 rows; 18 covered)

The rows are `boundid:` keys in `parity.json` (not `action:`): the digit ids are registered in a loop in `workbench/mod.rs` and are not in `assets/gen/actions.json` (the manifest has a dynamic entry for them, `src/features/search/index.ts:476/486`); their keys are in `assets/gen/keymaps.json` (`Ctrl+Shift+<d>` toggle, `Ctrl+<d>` go to, on both IDEA presets, no other action on those keys — checked by script).

- `workbench::tests::proofs7_bookmark_digits::toggle_bookmark_with_a_digit_marks_the_caret_line_with_that_digit_for_every_digit` → `boundid:navigate.toggleBookmark0`…`boundid:navigate.toggleBookmark9` (except 7, already done) → **done**. Proof text: "`workbench::tests::proofs7_bookmark_digits::toggle_bookmark_with_a_digit_marks_the_caret_line_with_that_digit_for_every_digit`; for each digit, its key from the active keymap pressed on its own line gives that line a bookmark with the digit and types nothing; run through the registry on the same line it removes it and again puts it back".
- `workbench::tests::proofs7_bookmark_digits::go_to_bookmark_with_a_digit_puts_the_caret_on_that_bookmarks_line_for_every_digit` → `boundid:navigate.gotoBookmark0`…`9` (except 7) → **done**. Proof text: "`workbench::tests::proofs7_bookmark_digits::go_to_bookmark_with_a_digit_puts_the_caret_on_that_bookmarks_line_for_every_digit`; for each digit, its key pressed from another file and the action run through the registry from another line of the file put the caret at the start of the line of that digit's bookmark".
- Digit 7 rows may cite both tests too.

### Group 7 — Project view option and sort toggles (4 rows; 4 covered)

All four are toggles with no `when` in the webview (`src/features/project-tree/actions.ts:167`, `:189`) and in the native registration (`workbench/project_view_actions.rs::register_computed`); defaults match (`model.ts`: compact, foldersFirst, showExcluded on; sort by name). Modification order is newest first in both (`model.ts:129`, `project_tree_model.rs::sort_in_place`).

- `…::proofs7_tree_options::compact_middle_packages_joins_a_chain_of_single_folders_into_one_row` → `action:project.option.compact` → **done**: the toggle reads on/off/on and the tree's rows go `docs/guide/intro` / `docs`, `guide` / joined again.
- `…::sort_folders_first_lists_folders_before_files_and_off_sorts_them_together` → `action:project.option.foldersFirst` → **done**: `b, a.txt, c.txt` ↔ `a.txt, b, c.txt`.
- `…::show_excluded_files_lists_ignored_entries_and_off_leaves_them_out` → `action:project.option.showExcluded` → **done**: the `.gitignore`d folder shows dimmed, goes, comes back.
- `…::sort_by_modification_time_lists_the_newest_file_first` → `action:project.sortBy.modified` → **done**: mtimes set before the project opens; `b.txt, c.md, a.md` (neither name nor type order); toggled states of modified/name.

(`…` = `workbench::project_view_actions::tests`.)

### Group 3 — Spec cases not found by name (6 rows + 1 closer)

- `unit:src/features/debugger/__tests__/executionState.test.ts` → **done with an EXISTING test**, no new test: `debugger::state::tests::retains_locals_whatever_their_names` (state.rs ~2370) feeds locals named `constructor`, `hasOwnProperty`, `toString`, `valueOf`, `__proto__`, `isPrototypeOf` and asserts each is kept. Proof text: "`debugger::state::tests`, `debugger::actions::tests::each_resuming_action_clears_the_old_execution_point_before_its_request`; ten of ten cases, the one about locals named like Object.prototype properties is `debugger::state::tests::retains_locals_whatever_their_names`".
- `unit:src/features/debugger/__tests__/javaSeparator.test.ts` → `run::java_launch::tests::proofs7_separator::joins_the_classpath_and_module_paths_using_the_host_separator` → **done**: the five hosts of the spec (`:` with a `;` inside a path, `;` with Windows paths), both `${classpath}` and `${testClasspath}`, args equal `--module-path <joined> -cp <joined> Main`. `resolve_java_command` is reached (`JavaPreparation`, java_launch.rs ~500, used by `debugger::java`). The split case was already `debugger::java::tests::splits_explicit_classpaths_without_damaging_paths`. Proof text: "`debugger::java::tests`, `run::java_launch::tests::proofs7_separator::joins_the_classpath_and_module_paths_using_the_host_separator`".
- `unit:src/platform/project.test.ts` → `platform::virtual_docs::tests::proofs7_relative_dir::{is_empty_for_files_directly_in_the_project_root, is_project_relative_for_nested_files, keeps_the_absolute_directory_outside_the_project}` → **done** (with `platform_more::project::tests` for the other cases). `relative_dir` is reached from `code_insight::{problems_view, usages_view}`.
- `unit:src/features/settings/__tests__/keymap.test.ts` → three cases were missing:
  - "applies a user keymap as its parent preset plus its overrides": EXISTING `platform::keymap::tests::applies_settings_with_validation_and_custom_keymaps` (user keymap `mine` on parent `eclipse-macos`; the passed overrides are ignored, the keymap's own apply) — same rule as the spec's.
  - "ships the VS Code preset for every IntelliJ action id": new `platform::keymap::tests::proofs7_vscode_preset::ships_the_vs_code_preset_for_every_intellij_action_id` (checked by script on `keymaps.json`: 0 missing of 363).
  - "suspends the dispatcher while recording": new `platform::input::tests::proofs7_suspend::a_suspended_window_runs_no_shortcut_until_it_is_resumed` (a key bound in the window runs nothing while `input::suspend` holds, nests, runs once after the resumes). The recorder that calls it is `settings_view/keymap.rs` (~404/411/430).
    → **done**. Proof text: append the three tests above to the current proof and drop "were not found and are not verified".
- `unit:src/platform/keymaps/windows.test.ts` → **stays partial**, no test written: there is no native `hasPrimaryModifier`; each view has its own `primary(&Modifiers)` (`debugger/variables_view.rs:1084`, `run/view.rs:123`, `database/grid_view.rs:171`). Proving the two asserts needs a shared helper first (e.g. `platform::keymap::has_primary_modifier(mods: Mods, mac: bool) -> bool { if mac { mods.meta } else { mods.ctrl } }`, used by those views) — product work, not written.
- `unit:src/features/search/__tests__/findPopup.test.tsx` → **not done** (time-boxed out): 25+ spec cases (with `it.each`) against the 23 window tests of `find::popup_view::tests`; matching them one by one is its own task.
- Closer `unit:src/features/settings/__tests__/languages.test.ts`: not attempted (needs malformed-value normalisation, product).

### Group 10 — Context menu group rows (3 rows + 1 closer)

- `workbench::context_menus::tests::proofs7_send_to_agent::send_to_agent_chosen_in_the_editors_menu_mentions_the_selected_lines_in_the_new_tasks_prompt` and `workbench::project_view_actions::tests::proofs7_project_menu::send_to_agent_in_a_folders_project_view_menu_mentions_the_folder_in_the_new_tasks_prompt` → `point.registerMenuGroup:agents.send` → **done** (the "virtual document left out only as data" clause stays a note: `agents_send` returns nothing for a virtual URI, as the webview). Asserted: the row chosen from each menu puts `@a.txt:1-2` / `@src` and the file reference in the New Task composer (`AgentsStore::composer(HOME)`; no task exists, so `add_references` targets the home composer), and the menu closes. No agent process starts.
- `workbench::project_view_actions::tests::proofs7_project_menu::add_bookmark_in_a_folders_project_view_menu_bookmarks_the_folder` → `point.registerMenuGroup:search.bookmarks.files` → **stays partial**: proves the Project view's row on a folder (bookmarked with `dir: true`, then Remove Bookmark). Several paths at once cannot be tested: the native tree has a single selection (`ProjectTree::{select, selected_path}`).
- `point.registerMenuGroup:search.bookmarks` → **no test written; the group's claim is contradicted by the webview**. The webview rows (`src/features/search/index.ts:209-245`) run `executeAction("navigate.toggleBookmark" | "bookmarks.toggleFileBookmark")`, and `runAction` (`src/platform/actions.ts:179-186`) returns false when the action's `when` (`projectOpen` on both, `index.ts:419`, `:438`) fails. So without a project the webview's row is clickable but does nothing; the native row is greyed out and does nothing. The only difference is the look. Proposed: record the row done with the reason "the row is greyed out where the webview's runs nothing; same outcome" (owner decision), or, to match the look exactly, build these two rows with `disabled: false` in `line_bookmark` / `file_bookmark` (`workbench/context_menus.rs`) — the action still refuses to run.
- Closer `point.registerMenuGroup:plugins`: not attempted (editor tab and Problems menus).

### Group 11 — Contributed toolbar slots and plugin icons (3 rows)

- `workbench::context_menus::tests::proofs7_toolbar_slots::a_plugins_main_toolbar_entry_is_a_title_bar_button_that_runs_its_command` and `…::a_plugins_commit_message_entry_is_a_button_of_the_commit_message_toolbar` → `module:src/workbench/ContributedToolbar.tsx` and `point.registerMenuSlotItem:(at) src/features/plugins/uiPoints.tsx:182` → **stay partial**: the two slots are now drawn and clicked (the button `slot:mainToolbar:<action>` in the title bar, `slot:commit.message:<action>` once the Commit window shows, each inserting the date into the editor). Still missing: the plugin's own icon for a button whose action has none.
- `unit:src/features/plugins/__tests__/pluginIcon.test.tsx` → not written. The piece is product work, not a small hunk. `plugin_contributions::plugin_slot_items` sets `icon` only from the action, and `context_menus::slot_buttons` falls back to `"plugin"`. The webview (`uiPoints.tsx:185`) uses `getAction(action)?.icon ?? pluginIconName(pluginId, plugin.icon)`. Doing the same natively means reading the listed plugin's icon markup, naming it through a `PluginIcons` kept per app (`plugin_models/plugin_icon.rs::PluginIcons::name`), and registering the returned SVGs with `theme::icons::register_external_icons`, as `workbench/plugin_tool_windows.rs:~408` does for tool windows. Once that lands, the test can read `plugin_slot_items("mainToolbar", cx)[0].icon` = `ext:pluginIcon/<id>/<hash>` for a fixture plugin with an icon.

### Group 12 — Docker filters and tree from an injected snapshot (3 rows): SKIPPED, the claim is contradicted

`DockerStore::snapshot_answered` (services/docker.rs:1201) is not a usable seam. It drops any answer whose ticket was not handed out by `refresh(conn)` for a known connection (`has_connection`). With a connection in `docker.json`, the window carries out the `DockerRequest::Snapshot` itself (`workbench/services_window.rs::carry_out_docker`, ~723) through `ide_docker::snapshot`, which calls `docker_executable()` (a lookup of the `docker` program) and runs `docker ps/image ls/...` (crates/ide-docker/src/lib.rs:127-148). Missing seam: a test-replaceable snapshot source in `carry_out_docker`, e.g. a global `DockerSnapshotSource(Rc<dyn Fn(&DockerEndpoint) -> DockerSnapshot>)` that defaults to `ide_docker::snapshot`. Test builds would set a fake one, and the connection would also need an endpoint that never runs `docker`. No test written.

### Group 13 — Extend Selection: a step only the backend gives (3 rows; 3 covered)

- `editor::core_actions::tests::proofs7_extend_selection::extend_selection_takes_the_call_around_its_arguments_from_the_syntax_tree` → `action:code.extendSelection`, `action:code.shrinkSelection`, `ipc:doc_selection_ranges` → **done**. Asserted: the first step is `[one]`, each step selects strictly more, one step (found within six) is exactly `[call(one, two)]`, and Shrink walks every step back to the caret. `text_candidates` (core_actions.rs:215) gives only the word, the literal tree's bracket and string nodes (with and without their delimiters), the line from its indentation, the whole line and the document. So `call(one, two)` can only come from `ide_host::ops::doc::doc_selection_ranges` (core_actions.rs:273), the same op the adapter uses. The fake LSP is not needed, because the ranges come from the backend's syntax tree.

### Group 14 — GitHub URL cases with the fake gh (3 rows)

- `workbench::github::tests::proofs7_github_urls::open_on_github_without_a_pull_request_in_context_opens_the_repositorys_page` → `action:github.openInBrowser` → **done**: list shown, no tab open; it opens `https://github.com/org/app` (the connection's repository URL, as `runtime.ts:52-56`).
- `workbench::github::tests::proofs7_github_urls::the_url_of_a_pull_request_neither_open_nor_listed_is_the_one_github_gives` → `action:github.pullRequest.copyUrl`, `action:github.pullRequest.openInBrowser` → **done once the fix lands; EXPECTED TO FAIL now**. The webview's `prUrl` (`runtime.ts:37`) is `cachedDetails(n)?.url ?? (await loadPullRequestData(n, "details")).url`. The native `Workbench::pull_request_url` (workbench/github.rs ~769) builds `https://github.com/<owner/name>/pull/<n>` instead, so the test gets `https://github.com/org/app/pull/7` where gh answers `https://github.com/org/app-renamed/pull/7`. Fix: when neither the tab's details nor the list has the number, load the details the way `load_pull_request(number, cx)` does, then copy or open `details.short.url`. The two handlers become async: the URL is copied or opened once the details arrive, falling back to the built URL only when gh fails.

### Group 16 — Edit actions on text fields and lists: SKIPPED

This is M product work: `app_models::edit_command` is unreached, and `edit.copy/cut/paste/undo/redo/selectAll` do not route to a focused text field or list. No test was written.

### Group 25 — Hint at the caret (2 rows; 2 covered, depends on a hunk)

- `workbench::editor_language::tests::proofs7_hint::show_editor_hint_draws_the_hint_in_the_tone_it_is_given`, `…::a_hint_at_the_caret_goes_away_after_four_seconds`, `…::an_edit_a_caret_move_or_escape_dismisses_the_hint_at_the_caret` → `action:editor.showHint`, `module:src/editor/lsp/hint.ts` → **done**. Asserted: the tone the drawn `HintView` has (Info/Warning/Error), still shown at 3 s and gone after 4.1 s on the executor's clock (HINT_TIMEOUT_MS = 4000, as the webview's 4000), and dismissed by typing, by → and by Escape.
- **Hunk** (`app-gpui/src/editor/intelligence.rs`, beside `pub fn hover(&self) -> &Entity<HoverView>`):
  ```rust
      /// The view drawing the hint at the caret.
      pub fn hint_view(&self) -> &Entity<HintView> {
          &self.hint_view
      }
  ```
  (`HintView` is already imported there, line 24; `HintView::tone()` exists in editor_lsp/hover_view.rs ~805.)

### Group 29 — Quit sequence (2 rows)

- `workbench::tests::proofs7_quit::cmd_q_from_the_menu_bar_confirms_the_exit_and_closes_every_window` (`#[cfg(target_os = "macos")]`) → `action:app.quit` (the macOS menu key path) and `ipc:app_quit` (every window closed) → **done on macOS**. No keymap binds `app.quit` (checked: no preset has it). ⌘Q is the menu bar's native shortcut (`menu_spec.rs:177`, `native_shortcut: true`), bound by `menu::sync` and run by `menu::run` even while a key is in flight. The app's own exit after the last window closes (`cx.quit()`) is not observable in a GPUI test: the proof asserts that every window closed. The adapter `src-tauri/src/commands/window.rs::app_quit` closes the project windows one by one (`windows::continue_quit`), then calls `app.exit(0)`. The native `app_shell::quit` is a window flow, not an `ide_host::ops` call. So `ipc:app_quit` becomes **done with a limit** ("every window closes; the process exit is not observable in a test") if the owner accepts that; otherwise it stays partial. The Terminate test below asserts the same closing and is not macOS-only, so cite it for `ipc:app_quit` too. The webview's `quitApp` (`src/features/app/closeGuard.ts:245`) confirms the exit and then lets "each window ask about its own processes as the backend closes it", which matches.
- `workbench::tests::proofs7_quit::terminate_chosen_from_quit_stops_the_running_program_and_closes_the_window` → `action:app.quit` (Terminate from Quit) → **done**. It relies on the run store's last notification before it is released with the window, recorded through `cx.observe`.

### Group 30 — Read-only toggle (2 rows; 2 covered)

- `workbench::editor_core::tests::proofs7_read_only::a_click_on_the_status_bars_lock_asks_then_toggles_the_files_read_only_attribute` → `point.registerStatusItem:editor.readOnly` → **done**: click `status:editor.readOnly` → "Make File Read-only" → Enter → read-only on disk and in the editor; click again → "Make File Writable" → writable.
- `…::a_read_only_toggle_that_fails_says_it_cannot_make_the_file_read_only` → `module:src/editor/core/fileAttributes.ts` → **done**: the file is removed after it opens, so `ide_host::ops::fs::set_writable` fails; an Error notification "Cannot make notes.txt read-only" shows and the editor stays writable.

### Group 32 — Run chooser and removal spec cases: NOT WRITTEN (stopped here for this group)

The existing tests that cover the missing cases, found by reading:

- chooserPinned "lists the configurations started last after the pinned ones, newest first" = `run::config_chooser::tests::lists_pinned_then_recent_then_groups_by_kind` (same `build_rows` level as the spec's `buildRows`).
- "keeps pins per project across reloads" is half-covered by `run::driver::tests::selecting_pinning_and_deleting_are_stored_and_saved`, which stores and unpins but has no reload and no other project.
- "marks a configuration recent when a run or debug session starts" = `run::driver` line ~823 calls `mark_recent` and `run::store` tests ~2196 cover it on the store; no window test was found.
- removal.test 103 (it.each over 8 kinds), 116, 123, 133 were not checked.

### Group 33 — Services Move Up / Move Down (2 rows + closer)

- `workbench::services_actions::tests::proofs7_move::move_up_and_down_are_greyed_out_at_the_ends_of_a_group_and_a_later_window_keeps_the_order` → `action:moveUp`, `action:moveDown` → **done**: Move Up disabled on the first row, Move Down on the last (the toolbar's `BatchAction::disabled`); after Move Down, a second window `w2` on the project lists the swapped order. The closer `unit:src/features/services/ServicesTree.reorder.test.tsx` stays partial (row dragging).

### Group 37 — pasteHistoryListFocus: SKIPPED

`docs/gpui-port/notes/integration/editor_extras_integration.md` §2d already covers it: `set_context(... "pasteHistoryListFocus" ...)` in workbench/mod.rs ~1989, with the test `the_paste_history_keys_are_enabled_while_its_dialog_has_the_keyboard`.

## Product hunks the tests depend on

1. `hint_view()` accessor (group 25, above).
2. Group 14's second test needs the `pull_request_url` fix; without it the test fails, as the webview's behaviour demands.

## Tests most likely to fail on first run

1. `proofs7_github_urls::the_url_of_a_pull_request_neither_open_nor_listed_is_the_one_github_gives`: expected to fail until the fix lands (above).
2. `proofs7_bookmark_digits::*`: the GPUI test platform must deliver `ctrl-shift-<digit>` as key `<digit>` with Shift, so that the dispatcher reads `Ctrl+Shift+<digit>`. `ctrl-shift-q` and `ctrl-shift-p` already work in `editor_extras` tests. If a digit with Shift arrives as its shifted character, the key half fails: compare with `input::key_name`.
3. `proofs7_tree_options::show_excluded_files_lists_ignored_entries_and_off_leaves_them_out`: it assumes `.gitignore` is honoured without a Git repository, as `project_tree::tests::hides_excluded_and_ignored_entries_by_option` shows for the bare tree. `compact_…` assumes the chain label is `docs/guide/intro` and that compact off leaves `docs` open with `guide` closed, mirroring `compacts_chains_of_single_folders`.
4. `proofs7_move`: the later window may not list the `program` type group unless `run.services.addType` is stored per project. If the rows never show, run `run.services.addType` with `"program"` in `w2` too, before the wait.
5. `proofs7_read_only::a_read_only_toggle_that_fails_…`: if the editor reacts to the removed file by refusing the toggle (no question), use another failing `set_writable` instead. On macOS, `chflags uchg` on the file makes chmod fail with EPERM.
6. `proofs7_toolbar_slots::a_plugins_commit_message_entry_…`: Insert Date must act on the last active editor while the click lands in the Commit window.
7. `proofs7_quit::terminate_…`: the dialog after Exit must be the same "Process 'nap' Is Running" question the window close asks.
8. `proofs7_hint::a_hint_at_the_caret_goes_away_after_four_seconds`: it assumes the hint's timer runs on the GPUI executor clock (`advance_clock`). `wait_hint` also advances that clock by 50 ms per poll, so it leaves margins of 3.0 s and 4.1 s. `an_edit_…`: Escape must reach `EditorIntelligence::escape` while the editor has the keyboard.
9. Group 14's expected failure takes about a minute to report: `wait_long` polls for 30 s, then 30 s more, before it panics.

Webview behaviour checked for groups 13 (`src/editor/core/actions.ts`), 29 (`closeGuard.ts:245`, `window.rs:239`), 30 (`fileAttributes.ts:26-28`: error level, title `Cannot make <name> read-only|writable`; the status item is `editor.readOnly` in `src/editor/core/index.ts:114`) and 33 (`ServicesTree.tsx:124-127`: Move Up/Down are greyed out, not hidden, when there is no neighbour to swap with). All match the tests.
For group 14, `targetPullRequest` (`runtime.ts:27`) is the timeline's pull request, else the context's. Natively `GhToolWindowTabs::context_pull_request` (github/toolwindow.rs:90) is the active `#N` tab, else the list's selection, which is set only when a row is chosen (`list_panel.rs:945`). The test asserts that no pull request is in context before the action runs.

## Groups whose claim the code contradicts

- Group 10 (bookmark rows "always run" in the webview): they do not. The action's `projectOpen` gate stops them there too. Only the greyed-out look differs.
- Group 12 ("snapshot_answered is a seam, so no daemon is needed"): any answered snapshot needs a connection, whose refresh looks up and runs `docker`.
- Group 3 / executionState ("not found"): the case exists as `debugger::state::tests::retains_locals_whatever_their_names`. Group 3 / keymap.test "user keymap on its parent preset": it exists in `platform::keymap::tests::applies_settings_with_validation_and_custom_keymaps`.
- Group 13's piece names a fake LSP, but the selection ranges come from the backend's syntax tree (`doc_selection_ranges`), not from a language server.

## Where I stopped

Groups 1, 7, 3 (except findPopup and windows.test), 10, 11 (except the icon), 13, 14, 25, 29, 30 and 33 are written. Groups 12, 16 and 37 are skipped for the reasons above. Group 32 was not written: its existing proofs are listed above. Step 7 (other S, test-only groups in ranking order: 20 Database results spec cases, 26 Hot swap with a fake adapter, 28 Project-tree backend-driven tests, 67 Plugin debug preparation asserted, 68 Settings > Plugins with a plugin host) was not started.
