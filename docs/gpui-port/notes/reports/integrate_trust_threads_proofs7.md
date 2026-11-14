I'm finishing with four of five consecutive whole-suite runs green. The fifth failed one VCS test that isn't mine (`rollback_in_an_editor_restores_the_changed_lines_under_the_caret`); I didn't find its cause and didn't fix it. All Part 0a, 0b, 1 and 2 tests pass, clippy is clean, and nothing in `docs/`, `src/`, `src-tauri/` or `crates/` was edited.

## 1. Suite, clippy, scope

- **Suite ×5:** 109, 194 (1 failed), 109, 109 and 109 s, 0 ignored, 6,800 tests. The failing test waits for a file change it never sees ("reached later: None"); its state dump shows only the first status read.
  - The test, its sibling `rollback_restores_a_modified_file_and_deletes_an_added_one_only_when_asked`, and `plugin_java_debug::tests::stop_ends_the_process_and_the_session` fail every time the suite is slowed. Sampling threads with `ps -M` slows it to 274 s and reproduced all three on both sampled runs.
  - The java-debug test is only slow (it reached its state at 75 s). All three pass alone.
  - Hypothesis: the file-watch event for a write made right after the first status read is lost, so the window never re-reads. Confirm by running `$S/i7_sampled.sh` against a clean `ab92c69a` checkout.
- **Clippy:** clean.
- **Compile errors:** Part 1: 0. Part 0a: 0. Part 0b: 3 (missing `Path` import, private `ide_host::project` path twice). Clippy: 2 (`let_unit_value`, `needless_range_loop`).
- **Outside scope:**
  - I created an empty stray `/var/folders/…/T/../ins.py` and deleted it.
  - My first loop overwrote `$S/run1.txt`, `$S/run2.txt` and `$S/summary.txt`, which may have belonged to another session (`$S` is shared).
  - `pgrep` read other sessions' process ids (read only).
  - rustfmt changed no file I hadn't edited.

## 2. Part 0a

- **Rule (`trust.rs` `authorize_command` and the allow-list; `lib.rs:170`):**
  - A project in Restricted Mode is refused with "Project is in Restricted Mode. Trust the project to enable execution."
  - A single-file window is refused too: its project is built with `trusted=false` and `refresh_project_trust` skips it.
  - A window with no project is refused with "no project is open in this window", except for the plugin-management commands (install, enable, uninstall…). This one is read, not tested.
  - Allowed plugin commands: `plugin_subscribe`, `plugin_list`, `plugin_set_active_file`, theme, catalog, assets, market and package commands. Everything else is refused, including run, view render and events, line markers, answers and event forwarding.
- **What the webview shows (read):**
  - a command, menu entry or status-item click: an error notification "<title> failed" with the reason (`store.ts:395-411`); the actions stay shown and enabled;
  - a view: "The plugin could not show this view" or an inline error (`PluginView.tsx:119-149`);
  - a settings action: an inline error under the button (`settings.tsx:250-266`);
  - gutter, events and prompt answers: silent (`languageVisuals.ts:206-210`, `runtimeUi.tsx:309-328`, `:412-421`).
- **Native fix:** only the action path was unguarded. `app-gpui/src/plugins.rs::run_command` (used by actions and menus) now asks `authorize_command(label, "plugin_run_command")`, and the refusal flows into the existing notification. All other entry points already asked the guard.
- **Tests:** `app-gpui/src/workbench/plugin_contributions/tests/untrusted_windows.rs`, 9 tests. Each has a refused case in a restricted window and an allowed one after trusting (or in a trusted window): action, menu, status item, view render, view event, settings action, gutter, saved-file event, prompt answer.
  - The trigger in the gutter and view tests differs: neither re-asks just because trust changed, natively or (hypothesis) in the webview. The gutter's allowed half uses an edit; the view's uses a trusted window.
- **Single-file test:** the guard refuses it, so it is replaced by `plugins::tests::a_plugins_command_is_refused_on_a_file_of_its_own_and_runs_its_wasm_in_a_trusted_project`: refusal notification, text unchanged, plugin not loaded. The positive half runs in a trusted project with its own folder.
  - With the same file open in both windows, the rewrite never arrived within 60 s. Cause not established.
- **Not done:** the Plugins page calls (`plugins_window.rs:430-560, 740`: enable, reload, uninstall, grant, capability, roll back, secrets, install) still reach the host unguarded.

## 3. Part 0b (measured, one backend, child process)

Test: `workbench::tests::project_threads::a_projects_threads_end_when_it_leaves_its_window`. It reads thread names through a test-only, macOS-only libSystem FFI: no new dependency, but the crate's first `unsafe`.

| state                                  | fs-watch / index / db-history | all threads |
| -------------------------------------- | ----------------------------- | ----------- |
| baseline                               | 0 / 0 / 0                     | 4           |
| one project open                       | 2 / 1 / 1                     | 22          |
| after Close Project                    | 0 / 0 / 0                     | 17          |
| after 10 open/close rounds             | 0 / 0 / 0                     | 17          |
| 10 projects opened in turn (last open) | 2 / 1 / 1                     | 23          |
| after last close                       | 0 / 0 / 0                     | 17          |
| after 10 windows closed                | 0 / 0 / 0                     | 17          |

The application releases them; no fix was needed. Hypothesis for the counts in the test binary: tests that never close their projects or drop their backends. Peak in the whole suite: 3,340 threads.

## 4. Part 1 (28 tests, all pass)

| tests                                                                                             | keys                                                                                   | status                             |
| ------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------- |
| `workbench::tests::proofs7_bookmark_digits::{toggle_…,go_to_…}`                                   | `boundid:navigate.toggleBookmark0…9`, `gotoBookmark0…9`                                | done                               |
| `workbench::project_view_actions::tests::proofs7_tree_options::*` (4)                             | `action:project.option.{compact,foldersFirst,showExcluded}`, `project.sortBy.modified` | done                               |
| `run::java_launch::tests::proofs7_separator::*`                                                   | `unit:…/javaSeparator.test.ts`                                                         | done                               |
| `platform::virtual_docs::tests::proofs7_relative_dir::*` (3)                                      | `unit:src/platform/project.test.ts`                                                    | done                               |
| `platform::keymap::tests::proofs7_vscode_preset::*`, `platform::input::tests::proofs7_suspend::*` | `unit:…/keymap.test.ts`                                                                | done                               |
| `proofs7_send_to_agent::*`, `proofs7_project_menu::send_to_agent_…`                               | `point.registerMenuGroup:agents.send`                                                  | done                               |
| `proofs7_project_menu::add_bookmark_…`                                                            | `…:search.bookmarks.files`                                                             | partial                            |
| `workbench::context_menus::tests::proofs7_toolbar_slots::*` (2)                                   | `module:…/ContributedToolbar.tsx`, `registerMenuSlotItem`                              | partial (plugin icon missing)      |
| `editor::core_actions::tests::proofs7_extend_selection::*`                                        | `action:code.{extend,shrink}Selection`, `ipc:doc_selection_ranges`                     | done                               |
| `workbench::github::tests::proofs7_github_urls::*` (2)                                            | `action:github.openInBrowser`, `github.pullRequest.{copyUrl,openInBrowser}`            | done                               |
| `workbench::editor_language::tests::proofs7_hint::*` (3)                                          | `action:editor.showHint`, `module:…/hint.ts`                                           | done                               |
| `workbench::tests::proofs7_quit::*` (2)                                                           | `action:app.quit`, `ipc:app_quit`                                                      | done (process exit not observable) |
| `workbench::editor_core::tests::proofs7_read_only::*` (2)                                         | `point.registerStatusItem:editor.readOnly`, `module:…/fileAttributes.ts`               | done                               |
| `workbench::services_actions::tests::proofs7_move::*`                                             | `action:moveUp`, `action:moveDown`                                                     | done                               |

**Product fixes:**

- **Quit:** `app_shell.rs::quit_then` now closes each window that agreed before quitting, as the Tauri `continue_quit` does (`windows.rs:502-517`; restart goes through the same path, `caches.rs:25`). Without it no window closed in either quit test.
  - Two existing tests changed: `workbench::app_actions::tests::{invalidate_and_restart_…, just_restart_…}` called `close(cx)` on the window the restart now closes; they now assert it closed.
- **GitHub:** `github.rs` loads the pull request's details from gh, as `prUrl` (`runtime.ts:37`). It keeps the list row's URL when the list has it, so no gh call then. On a gh failure nothing opens; the webview's behaviour on that failure is unread (hypothesis).
- **Accessor:** `EditorIntelligence::hint_view` added in `intelligence.rs`.

## 5. Part 2 (`workbench::editor_extras::tests::levels_and_choosers::*`)

- `expand_to_levels_two_to_five_fold_the_regions_nested_deeper`: ⌘⇧*2–5 and ⌘*2–5 on seven nested regions, the shown lines checked after each key; nothing happens while the Project view has the keyboard. Keys `code.expandAllToLevel2–5`, `code.expandToLevel2–5`.
- `unwrap_with_several_statements_around_the_caret_lists_them_innermost_first`: ⇧⌘⌦ lists `Unwrap 'while'` then `Unwrap 'if'`; choosing `if` unwraps it; with one statement there is no chooser. Key `code.unwrap`.
  - Product fix: native rows read `while` where the webview reads `Unwrap 'while'` (`unwrap.ts:162`); `editor_extras.rs::choose_unwrap` now uses the same catalog string.
- `next_and_previous_occurrence_step_the_usages_and_say_which_of_how_many`: ⌥⌘↓/↑ through the Usages window gives "1 of 2", "2 of 2", "1 of 2"; before any search, nothing moves. Keys `navigate.{next,prev}Occurrence`.
- `reload_all_from_disk_re_reads_every_folder_the_project_view_has_read`: a test-only counter in `project_tree.rs` shows exactly 4 more listings, and none without the action. Key `file.reloadAllFromDisk`, run through the registry because it has no key.
- **Reformat:** the premise is false. The webview offers Optimize imports unless a contributed formatter owns the file (`format.ts:283`), which only database consoles have (`console.tsx:578`); then it disables the box rather than hiding it (`Popups.tsx:660`). Native consoles have no contributed SQL formatter, so I left `true` in place, which matches every other file.
- The manifest smoke test passes; its ids are in `$S/registered_ids.txt` (570).

## 6. Notes that turned out false

- "Optimize imports depends on the server" (task), as above.
- The quit drafts' "done on macOS" needed the quit fix.
- "Today natively, plugin views render in a restricted window" (task): not so. Render, events, status, settings, gutter and prompts were already refused; only the action path (actions, menus) ran.

`$S` = `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`
