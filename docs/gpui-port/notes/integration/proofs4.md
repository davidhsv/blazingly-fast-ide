# proofs4: tests for the partial rows whose only recorded gap is a missing test (ipc, unit, module, point)

Nothing was compiled or run. Every claim about behaviour below comes from reading the code, so it is a hypothesis unless it says otherwise. The check that confirms each one is running the named test.

## `mod` lines to add

1. `app-gpui/src/workbench/mod.rs`, inside `mod tests {` (line 3998), after the anchor line `    mod proofs_vcs;` (cargo fmt sorts the block):

```rust
    mod proofs4_run;
    mod proofs4_shell;
    mod proofs4_vcs;
```

2. `app-gpui/src/ui/tooltip.rs`, inside the second `#[cfg(test)]` item, `mod tests {` (line 278), before the anchor line `    const ACTION: &str = "navigate.searchEverywhere";`:

```rust
    mod proofs4_tooltip;

```

3. `app-gpui/src/vcs_diff/model.rs`, inside `mod tests {` (line 315), after the anchor line `    use crate::vcs::changes::ChangeKind;`:

```rust

    mod proofs4_remote_urls;
```

## Files

- `app-gpui/src/workbench/tests/proofs4_run.rs` (3 tests)
- `app-gpui/src/workbench/tests/proofs4_shell.rs` (3 tests)
- `app-gpui/src/workbench/tests/proofs4_vcs.rs` (3 tests)
- `app-gpui/src/ui/tooltip/tests/proofs4_tooltip.rs` (1 test; `tooltip.rs` has no child test module yet)
- `app-gpui/src/vcs_diff/model/tests/proofs4_remote_urls.rs` (1 test; `model.rs` has no child test module yet)

That is 11 tests. No accessor had to be added. Everything they use was checked to exist: `Workbench::{run_driver, tool_window_view, editor, welcome, close_project, project_root, vcs_state, log_details, project_tree, open_project}`, `RunStore::{config, temporary, sessions, loaded}`, `RunSession::{is_live, status, exit_code}`, `RunView::lines`, `TerminalPanel::active_view`, `Welcome::{run, lines}`, `WelcomeCommand::NewGroup`, `MenuBar::shown`, `menu::sync`, `MenuCommand`, `MenuNode::{Submenu, Item}`, `GitLog::{commits, filter_bar, filters, options}`, `LogFilterBar::{open, menu_lines, choose}`, `LogDetailsPane::{lines, is_loading, message_lines}`, `LogDetailsEvent::SetOptions`, `parse_temporary`, `TEMPORARY_KEY`, `UiStorage::set`, `RecentProjects::{load, groups}`, `VisualTestContext::deactivate_window` (gpui-pre 0.3.12 `test_context.rs:1075`), and the test helpers `start`, `settle`, `wait_long`, `run_action`, `run_project`, `trusted_project`, `select_in_chooser`, `close_backend_project`, `git_project`, `init_repo`, `git_in`, plus the tooltip tests' `host`, `hover` and `shown`. Helpers private to sibling files (`proofs_vcs::git_log`, `proofs3_run::shell`) are defined again in each file.

## Per test → rows

### `workbench::tests::proofs4_run`

- `kept_temporary_configurations_take_todays_settings_and_are_stored_without_them`
  - `ipc:run_apply_settings` → **done**. Proposed proof: "`workbench::tests::proofs4_run::kept_temporary_configurations_take_todays_settings_and_are_stored_without_them`; two temporary Go configurations kept in the UI storage, neither naming `GOPATH`, are listed when the project opens with the `languages.go.gopath` of the project's `.openidea/settings.toml` in their environment. Only the backend's settings can put it there. The native load calls `ide_host::ops::run::apply_settings` (`run/driver.rs:298`), as `src-tauri/src/commands/run.rs::run_apply_settings` does."
  - `ipc:run_without_derived` → **done**. Proposed proof: "same test; deleting the other temporary configuration in the chooser stores the remaining one again, and the stored copy has lost `GOPATH` but keeps its own `FOO`, while the listed one keeps `GOPATH`. The test waits for the rewrite, because the first write is unstripped. Native path: `carry_out_persist` → `ide_host::ops::run::without_derived` (`run/driver.rs:238`), as `run_without_derived` does."
- `a_location_printed_in_the_output_opens_the_file_that_exists_there`
  - `ipc:fs_entry_kind` → **done**. Proposed proof: "`workbench::tests::proofs4_run::a_location_printed_in_the_output_opens_the_file_that_exists_there`; a click on `notes.txt:2`, printed by a process running in `server/`, opens `server/notes.txt` at line 2. The project root also has a `notes.txt`, so the run directory's copy wins only because the backend said that path is a file. A click on `docs.txt:1`, where `server/docs.txt` is a folder, opens the root's `docs.txt`. The native path calls `ide_host::ops::fs::entry_kind` (`run/driver.rs:1371`), as `src-tauri/src/commands/fs.rs::fs_entry_kind` does."
- `a_gradle_boot_run_with_vm_options_is_launched_with_the_init_scripts_written_for_it`
  - `ipc:run_gradle_jvm_args_script` → **done**. Proposed proof: "`workbench::tests::proofs4_run::a_gradle_boot_run_with_vm_options_is_launched_with_the_init_scripts_written_for_it`; a Gradle BootRun with VM options `-Ddemo.flag=on` runs a stand-in `gradlew`. It is given an `--init-script` named `jvm-args-*` that holds the option, which only the backend writes. Native path: `prepare_type` → `ide_host::ops::run::run_gradle_jvm_args_script` (`run/driver.rs:1030`), as the IPC adapter does."
  - `ipc:run_gradle_init_script` → **done**. Proof: "same test; it is also given the `openidea-gradle-boot-*` script, whose text carries `openidea.run.debug`. Native path: `finalize_launch` → `ide_host::ops::run::run_gradle_init_script` (`run/driver.rs:1045`), as the IPC adapter does." The scripts land in the shared system temp directory (`std::env::temp_dir()/openidea-gradle`, `ide_run::gradle_jvm::default_dir`). The test reads them only through what the stand-in prints, so they are not cleaned up; they are named by their content and shared by design.

### `workbench::tests::proofs4_shell`

- `closing_a_terminal_session_ends_its_shell`
  - `ipc:term_kill` → **done**. Proposed proof: "`workbench::tests::proofs4_shell::closing_a_terminal_session_ends_its_shell`; the shell of a New Terminal Tab writes its own process id, runs while the tab is open, and is gone (not even a zombie, per `ps -o stat=`) once Close Terminal Session releases the view. The view's release calls `ide_host::ops::term::kill` (`terminal/view.rs:177`), which is `host.terminals.kill`. The IPC adapter `term_kill` calls `state.terminals.kill` on the same `Terminals` type." The adapter does not go through `ide_host::ops::term::kill`, but both call `Terminals::kill`.
- `a_group_named_on_the_welcome_screen_is_stored_with_its_project`
  - `ipc:set_recent_project_groups` → **done**. Proposed proof: "`workbench::tests::proofs4_shell::a_group_named_on_the_welcome_screen_is_stored_with_its_project` and `workbench::welcome::tests::a_group_is_named_in_a_form_above_the_list`; New Group… on the Welcome screen's recent project, named Work, is in the recent projects read again from their file (`RecentProjects::load(config).groups()`) with the project in it, and the list shows `[Work 1]` above the project. The workbench calls `ide_workspace::RecentProjects::set_groups` (`workbench/mod.rs:2880`). The IPC adapter calls `state.recent.set_groups`, the same function; no `ide_host::ops` wrapper exists on either side."
- `open_recent_lists_a_project_once_opened_and_choosing_it_opens_it`
  - `ipc:menu_refresh` → **done** (the recorded gap is stale, see below). Proposed proof: "`workbench::tests::proofs4_shell::open_recent_lists_a_project_once_opened_and_choosing_it_opens_it` and `workbench::menu::tests`; Open Recent is empty before anything is opened. Once a project has been opened, it lists that project with its path as the item's argument and its name first in the label. Choosing it (`file.openRecentProject`) opens the project in the window. The native refresh (`menu::refresh_recent`, called on every project open, `workbench/mod.rs:2986`) reads `ide_workspace::RecentProjects::list`, the list the adapter `menu_refresh` passes to `menu::install` (`state.recent.list()`)."

### `workbench::tests::proofs4_vcs`

- `a_selected_merge_commit_lists_its_changes_against_its_parents`
  - `ipc:vcs_diff_revisions` → **done**. Proposed proof: "existing proof + `workbench::tests::proofs4_vcs::a_selected_merge_commit_lists_its_changes_against_its_parents`; a selected merge commit lists only `a.txt`, the file both sides changed and the merge resolved anew. It does not list `f.txt` or `m.txt`, which came from one side each: that is the intersection of the backend's diffs against both parents. With Show Changes from Parents on, `f.txt` (against the first parent), `m.txt` (against the second) and `a.txt` twice are listed. Native path: `ChangesPart::Parent` → `git::vcs_diff_revisions` (`workbench/log_window.rs:74`), the same `ide_host::ops::vcs` function as the adapter."
- `the_log_date_filter_keeps_the_commits_of_the_chosen_period`
  - `ipc:vcs_log` → stays **partial**. Proposed proof: "existing proof + `workbench::tests::proofs4_vcs::the_log_date_filter_keeps_the_commits_of_the_chosen_period`; the Date filter's Last 7 Days leaves out a 2020 commit on another branch and keeps the recent ones (`since_ms` through `ide_host::ops::vcs::vcs_log`, `vcs/log_view.rs:293`). Not built: a path filter and a typed date range. The filter bar's Select Folders… (`LogFilterEvent::ChoosePaths`) and Select… date range (`ChooseDateRange`) fall into `log_filter_event`'s 'is not available' arm (`workbench/log_window.rs:274`). `vcs.showLog` with a `path` shows the main log (`vcs_operations.rs:974`), and `vcs.showHistory` has no handler, so nothing sets `filters.path` / `filters.paths` except a recent value that nothing can store."
- `routes_all_paths_to_gitignore_then_refreshes_status_and_tree`
  - `unit:src/features/vcs/__tests__/ignore.test.ts` → stays **partial**. Proposed proof: "`vcs_ops::ignore::tests`, `workbench::vcs_operations::tests::add_to_vcs_stages_an_unversioned_file_and_the_ignore_actions_write_their_own_file`, `workbench::tests::proofs4_vcs::routes_all_paths_to_gitignore_then_refreshes_status_and_tree`; all 7 cases on the model. Through the window: Add to .gitignore on a file and a folder writes both rules, and the window's status no longer lists them. Not asserted: the tree reload, the refresh after the local-exclude write, and the refresh after a failed write." The status refresh comes from `run_op` → `refresh` (`workbench/branches.rs:506`), which runs after success and failure alike. A watcher on `.gitignore` could also refresh the status, so this does not prove that the operation's refresh is the cause.

### `ui::tooltip::tests::proofs4_tooltip`

- `keeps_dismissal_behavior_for_a_key_press_and_the_window_losing_the_focus`
  - `unit:src/ui/Tooltip.test.tsx` → stays **partial**, and the test is **expected to fail**. The spec has 4 cases:
    1. "returns empty object when text is missing": `returns_no_tooltip_when_text_is_missing`.
    2. "sets data-tooltip and optional shortcut": `the_view_shows_its_text_shortcut_and_detail_lines`.
    3. "shows tooltip text with styles.text class and shortcut after delay": `shows_tooltip_text_and_shortcut_after_delay`.
    4. "keeps dismissal behavior for %s tooltips" (pointer out, pointer down, Escape, scroll, wheel, window blur): pointer out, pointer down and wheel are covered by `shows_tooltip_text_and_shortcut_after_delay` and `keeps_dismissal_behavior_for_pointer_down_and_wheel`; Escape and window blur are covered by the new test, which is expected to fail (see "Behaviours that differ").

    3/4 now; with the fix: 4/4, **done**, proof "`ui::tooltip::tests` and `ui::tooltip::tests::proofs4_tooltip::keeps_dismissal_behavior_for_a_key_press_and_the_window_losing_the_focus`". (The stripe class of the spec's case 4 is the webview's CSS; the native stripe tooltip is a separate element.)

### `vcs_diff::model::tests::proofs4_remote_urls`

- `maps_ssh_scp_like_and_https_remotes_to_their_web_page`
  - `unit:src/features/vcs/__tests__/branches.test.ts` → **done**, 13/13. This is a `.test.ts` spec of pure functions, so a model test is the case. Proposed proof: "`vcs::branches::tests`, `vcs_diff::model::tests::reads_progress_and_remote_urls`, `vcs_diff::model::tests::proofs4_remote_urls::maps_ssh_scp_like_and_https_remotes_to_their_web_page`, `workbench::log_actions::tests::view_in_browser_opens_the_commit_page_of_the_remote`; all 13 cases. The remote web URL case takes its exact inputs: scp-like, ssh with a port and a nested group path (`group/sub/app`), https with `user:token@` stripped, a local path (none), and the GitHub and Bitbucket commit pages."

## Tests most likely to fail first

1. `keeps_dismissal_behavior_for_a_key_press_and_the_window_losing_the_focus`: expected to fail on both halves. GPUI clears a tooltip only on mouse move out, mouse down and scroll (`gpui-pre-0.3.12/src/elements/div.rs`, around lines 3786–3862); there is no key-down or deactivation handler, and `ui/tooltip.rs` adds none.
2. `a_gradle_boot_run_with_vm_options_…`: whether the `gradle` start flow keeps the stand-in command and the `bootRun` argument. The type may rebuild the command line from `params` (`wrapper: true` was set so that it would resolve to the project's `gradlew` anyway). Also whether a Gradle configuration with no linked build passes the start's checks, and whether `gradle_build_env` needs a JDK.
3. `a_location_printed_in_the_output_…`: the click must land in the link's text range of `InteractiveText`. It clicks 16px into row `run-line-<index>`, where `<index>` is the position in `RunView::lines` (equal to the console's line index only without folds). It also depends on an absolute `cwd` passing through `run_directory` unchanged (`join_path` keeps an absolute path, read).
4. `kept_temporary_configurations_…`: whether the chooser lists temporary configurations by name and Delete removes one (precedent only for a saved one). It also relies on the stored key being `TEMPORARY_KEY + root.to_string_lossy()` of the canonical root (`mod.rs:2995` passes `root.to_string_lossy()`), and on `languages.go.gopath` being read from the project layer of a trusted project. If `goroot` finds a Go on the machine, `GOROOT` and `PATH` are also derived; the test does not assert them.
5. `closing_a_terminal_session_ends_its_shell`: whether anything besides the panel holds the `TerminalView` (focus restore, the workbench), which would keep it from being released. It also assumes the isolated shell expands `$$` to its own pid.
6. `a_selected_merge_commit_…`: the row format of `LogDetailsPane::lines` with Show Changes from Parents. The test trims each line and expects `a.txt` twice, but group header rows may read differently.
7. `open_recent_lists_…`: whether `OpenFlow::with_policy` in an empty window opens in place without asking ("Open project in" default).
8. `the_log_date_filter_…`: git's `--since` with several branch tips; the old commit is on its own branch, so the date-ordered walk reaches it last.

## Behaviours that differ from the webview (with fixes)

### Tooltips stay up on a key press and when the window is deactivated

The webview hides the tooltip on `keydown` (Escape) and on `window` blur (`src/ui/Tooltip.tsx`, the case "keeps dismissal behavior"). Natively `WithTooltip` hands the tooltip to GPUI's `.tooltip(…)`, which clears it only on mouse events (read in `gpui-pre` `div.rs`). No exact hunk: GPUI 0.3.12 exposes no public call that clears the active tooltip of an element (`clear_active_tooltip` is `pub(crate)`). Possible fixes, neither verified:

- (a) Have the workbench, on a key down it sees first (`input::claim_keys` or the dispatcher's pre-hook) and on `observe_window_activation`, force the element under the pointer to drop its tooltip, for example by moving the hover state with `window.refresh()` plus a synthetic `MouseMoveEvent` outside the bounds.
- (b) Draw tooltips as the app's own overlay instead of GPUI's.

The test stays as the webview demands.

## Rows left out, with why

- `ipc:history_list_dir`, `ipc:history_restore`: reached only through Undo Delete (`project_view/operations.rs:1345`), which needs a real delete first. `ide_host::ops::fs::delete` → `ide_vfs::delete` → `ops::delete(path, !cfg!(test))`, and `cfg!(test)` is false in `ide-vfs` built as a dependency of `app-gpui`'s tests, so the file goes to the system Trash. A failed delete (the route `a_confirmed_delete_searches_deletes_…` uses) records no `last_delete`, so there is nothing to undo. Fixture that would unblock them: a test hook making `ide_vfs::delete` permanent (for example an env var or a `Host` flag read by `ops::fs::delete`), or a Local History view (see below) that lists and restores without deleting.
- Rows already settled by `proofs3.md`: `ipc:run_stdin` and `ipc:run_close_stdin` (tested there), `ipc:java_resolve_launch` (left out there: the Java language server), `module:src/workbench/quickSwitch.ts` and `unit:src/features/app/__tests__/lightEditClose.test.ts` (not built, per proofs3).

## Rows that are really "not built"

- `ipc:run_resize`: `RunStore::resize_terminal` (`run/store.rs:1259`) has no caller outside the store, so `terminal_size` is never set and `StartEffect::Resize` (`run/store.rs:1934`), the only path to `ide_host::ops::run::resize` (`run/driver.rs:829`), never fires. Missing: the Run console of a run with "Emulate terminal in output console" reporting its size in cells (on layout and on resize) to `resize_terminal`, plus the resize of an already-running process.
- `ipc:history_content`: `SideLoad::History` (`workbench/diff_window.rs:78`) is reached only from a `DiffSide::History`, which nothing native builds. `vcs.localHistory` (Show History, in the Project view's and editor's menus, `menu_spec.rs:660`, `context_menus.rs:408`) has no handler (searched `app-gpui/src` for a `handle("vcs.localHistory"`), and `history_list` is never called. Missing: the Local History view (the revisions of a file from `history_list`, opening the diff of one against the current text). `vcs.openDiff` with a hand-built `DiffData` holding a History side would reach it, but no native caller produces such a side or a snapshot id.
- `ipc:vcs_log` (part): the path filter and a typed date range, see the proof above.

## Rows whose recorded gap is stale

- `ipc:menu_refresh`: "its items have no handler yet (file.openRecentProject)". `file.openRecentProject` is registered (`workbench/windows.rs:204`), as `proofs3.md` also noted. The new proof is above.
- `unit:src/features/vcs/__tests__/branches.test.ts`: the two "not asserted" inputs now are asserted (above).
- `unit:src/features/database/Grid.test.tsx`: "'preserves short values and truncates only beyond the preview' only on the model". That spec case renders nothing: it calls `cellPreview` only (`Grid.test.tsx:151–155`). `database::grid::tests::preserves_short_values_and_truncates_only_beyond_the_preview_limit` asserts the same inputs on the same function, so the case is covered as the spec writes it. Proposed: **done**, 12/12, with the existing proof text minus the "only on the model" clause. Read in this session: the Rust test (`app-gpui/src/database/grid.rs:545`) uses the spec's three short values, the long value, and the `999 × "a" + emoji` value.
- `module:src/workbench/projectColor.ts` ("no window uses it yet"): `workbench/welcome.rs:9` and `workbench/title_bar.rs:549` use it (grep, this session). No test was written here.
- `unit:src/features/app/__tests__/windows.test.ts`: `proofs3.md` reports `app_shell.rs` now drives `OpenFlow` / `LooseFiles` (`open_project_with_policy` → `OpenFlow::with_policy`, `app_shell.rs:673`, confirmed by reading). Not verified case by case here.

## Rows not reached in this pass

These were looked at only as far as the gap text, with no test and no verdict:

- Modules: `module:src/editor/core/copyReference.ts`, `module:src/editor/lsp/workspaceEdit.ts`, `module:src/platform/navigationModifier.ts`, `module:src/workbench/toolwindows/ToolWindows.tsx`, `module:src/workbench/toolwindows/content.ts`, `module:src/workbench/toolwindows/layouts.ts` (and `projectColor.ts` beyond the stale note).
- Points: `point.registerStatusItem:code-insight.servers`, `point.registerTitleItem:vcs.mergeRebase`.
- Unit specs: `structural.test.ts`, `Popups.test.tsx`, `AgentReply.test.tsx`, `AgentsView.test.tsx`, `Markdown.test.tsx`, `messageEditing.test.tsx`, `sessionState.test.tsx`, `windows.test.ts` (beyond the stale note), `results-view.test.tsx`, `fs.test.ts`, `unityRefresh.test.ts`, `runConfigTypes.test.tsx`, `session.test.tsx`, `behavior.test.ts`, `dialog.test.tsx`, `keymap.test.ts`, `mouseShortcut.test.tsx`, `pages.test.ts`, `diffScroll.test.ts`, `logOptions.test.ts`, `logUiFeatures.test.tsx`, `worktreesPanel.test.tsx`, `previewOnly.test.tsx`, `Workbench.test.tsx`.

## Count (of the 56)

- With a test here: 13 rows. Would become done: 10 (`ipc:run_apply_settings`, `ipc:run_without_derived`, `ipc:fs_entry_kind`, `ipc:run_gradle_jvm_args_script`, `ipc:run_gradle_init_script`, `ipc:term_kill`, `ipc:set_recent_project_groups`, `ipc:menu_refresh`, `ipc:vcs_diff_revisions`, `unit:…/branches.test.ts`). Stay partial: 3 (`ipc:vcs_log`, `unit:…/ignore.test.ts`, `unit:src/ui/Tooltip.test.tsx`, the last until the fix).
- Done without a new test (stale gap): 1 (`unit:…/Grid.test.tsx`).
- Really "not built": 2 whole rows (`ipc:run_resize`, `ipc:history_content`), plus part of `ipc:vcs_log`.
- Left out: 2 (`ipc:history_list_dir`, `ipc:history_restore`).
- Settled by `proofs3.md`: 5 (`ipc:run_stdin`, `ipc:run_close_stdin`, `ipc:java_resolve_launch`, `quickSwitch.ts`, `lightEditClose.test.ts`).
- Not reached: 33 (7 modules, `projectColor.ts` included; 2 points; 24 unit specs).
- Stale gaps noted: `ipc:menu_refresh`, `branches.test.ts`, `Grid.test.tsx`, `projectColor.ts`, `windows.test.ts`.
