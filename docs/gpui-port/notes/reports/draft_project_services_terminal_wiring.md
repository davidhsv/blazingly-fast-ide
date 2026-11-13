I wrote three new workbench modules and one notes file. None of it has been compiled or run, and no existing file was edited. Five files need edits pasted from the notes first (hunks anchored on quoted text): `mod.rs`, `project_files.rs`, `terminal/panel.rs`, and the test helpers in `mod.rs` `mod tests`.

## 1. Files written

- `app-gpui/src/workbench/project_view_actions.rs`: 903 lines, 11 end-to-end tests.
- `app-gpui/src/workbench/terminal_actions.rs`: 351 lines, 4 end-to-end tests and 1 unit test.
- `app-gpui/src/workbench/services_actions.rs`: 79 lines, tests only, 2 end-to-end tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/project_services_terminal_integration.md`. It has the `mod` lines, every hunk, a section per module, the IPC table, what is not wired and the assumptions.

There are no new view files. Select In and File Path reuse `show_menu` with `ProjectCommand` rows. The terminal panes need hunks to `panel.rs`: `root()`, `change()`, `view_of_tab()`, `panes()`, starting the shell of every group's selected tab, and drawing `GroupLayout` with `terminal-group:{id}` selectors.

## 2. Actions registered

Each is run through the registry or its key by the test named after the arrow.

**Project view**

- `navigate.selectInProjectView` / `project.reveal`: select the file in the tree; reveal leaves the keyboard where it was → `select_in_project_view_selects_the_edited_file_and_reveal_leaves_the_keyboard_where_it_was`. IPC: `fs_read_dir`.
- `navigate.selectIn`: the Select In chooser, or a direct select when given a path → `select_in_lists_the_places_that_show_the_file_and_the_project_view_selects_it`.
- `navigate.showFilePath` → `file_path_lists_the_file_then_every_folder_up_to_the_root`.
- `project.expandAll` / `project.collapseAll`: driven both by key and from the registry, and confined to the tree → `expand_all_and_collapse_all_…_act_in_the_tree_only`. IPC: `fs_read_dir`.
- `project.copyPath`, `copyFileName`, `copyRelativePath`, `copySourceRootPath`, plus Mark Directory as › Sources Root from the context menu → `the_copy_path_forms_copy_the_selection_and_the_source_root_path_follows_mark_directory_as`. IPC: `project_roots`, `settings_set`.
- Mark Directory as › Excluded → `an_excluded_folder_shows_as_excluded_and_the_search_skips_it`, which checks the folder's row and the backend search. IPC: `settings_set`, `fs_read_dir`, `search_start`.
- `project.compareWith`: two files go to the diff window; one file asks for the other; refused when the editor has the keyboard → `compare_with_shows_two_files_side_by_side_and_asks_for_the_second_of_one`. IPC: `fs_read_text`.
- `project.copy` / `cut` / `paste` were already registered. A new test checks the files on disk and the "'a.txt' already exists" refusal → `cut_and_paste_moves_the_file_and_a_copy_onto_a_taken_name_asks_for_another`.
- `project.open` (also F4) → `jump_to_source_opens_the_selected_file`.
- `project.findInFolder` → `find_in_folder_opens_find_in_files_on_the_selected_folder`.
- `project.option.{showHidden, showExcluded, compact, foldersFirst}` and `project.sortBy.{name, type, modified}` → `the_view_options_change_what_the_tree_shows_and_say_whether_they_are_on`.

**Terminal**

- `project.openInTerminal` / `terminal.openAt` → `open_in_terminal_on_a_folder_opens_a_shell_there_named_after_it`. IPC: `term_spawn`.
- `terminal.runCommand` / `terminal.newPredefinedSession` → `run_command_runs_its_program_in_a_tab_of_its_own_and_a_predefined_session_runs_the_shell`.
- `terminal.splitRight`, `splitDown`, `splitAndMoveRight`, `splitAndMoveDown`, `unsplit` → `the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`. It checks pane bounds and per-pane output.
- `the_terminal_actions_need_a_project` checks they are unavailable without a project.

**Services**

- Move Up / Move Down (toolbar) → `move_up_and_move_down_reorder_a_configuration_within_its_type_and_store_the_order`.
- `docker.toggleStoppedContainers` / `toggleUntaggedImages` (already registered) → `the_docker_view_options_flip_and_store_what_the_tree_leaves_out`.

**Not registered, with why**

- **`project.revealInFinder`:** the test platform's `reveal_path` is `unimplemented!()`.
- **`project.refresh`, `reloadFromDisk`:** the file watcher re-reads the same folders, so a test can't tell the action did anything.
- **`project.renameInline`:** the tree has no inline editor.
- **`project.findUsages`:** another agent's unwired draft test already covers it.
- **`project.showHistory`, `reformatCode`, `optimizeImports`:** their target actions aren't registered.
- **`project.viewMode.*` and the other `project.option.*`:** the tree has no panes and ignores those options.
- **`terminal.find`:** there is no find bar view or scrollback search.
- **`terminal.pluginSession`:** no plugin host here hands it requests.
- **`terminal.closeSession` question:** the pieces exist (`term_is_busy`, `terminate_confirmation`), but its handler sits in `mod.rs`, which another agent is editing.
- **Every Docker operation:** `docker_executable()` is a process-wide `OnceLock` resolved from `OPENIDEA_DOCKER`, `PATH` or fixed install paths. A fake binary can't be guaranteed, so a test could reach the real daemon. The workbench also drops these requests today.
- **Delete:** a confirmed delete always goes to the real Trash (`!cfg!(test)` applies only inside ide-vfs's own tests). Neither `ops::fs::delete` nor the dialog has a permanent option. No new delete test; the existing one removes the file first, so the backend refuses before any Trash call.

## 3. Spec cases covered by name

None. My tests are end-to-end proofs, not renamed spec cases:

- `ProjectView.test.tsx`: 0/27.
- `selectIn.test.ts`: still 5/5, as models.
- `ServicesTree.reorder.test.tsx`: case 1 is approximated, but I didn't reopen the window.
- `groups.test.ts`: still covered by the store's tests.

## 4. Not built, assumptions, likely failures

**Not built**

- The Select In chooser opens near the top of the window, not at the caret, and lists no contributed targets (`Contributed` is empty).
- Per-group terminal tab strips.
- "Cancel Exclusion" never appears in Mark Directory as (`with_menu_context` passes no roots).
- The `terminal.runCommand` fallback in `workbench/github.rs` becomes dead code, and "Log in with gh" will really run `gh auth login`. No test clicks it.

**Check first**

- The visibility changes in `project_files.rs` (`with_view`, `start_flow`, `run_project_command`, the `Done` alias, …).
- The `mod tests` helpers made `pub(super)`.
- `layout.show(TERMINAL…)` mounting the panel synchronously.
- `gen` not being a folder the backend already excludes on its own.
- `cmd-=` / `cmd--` spelling; on a Linux CI the keymap uses `ctrl`.

**Shell history**
By reading, not by running:

- Tests use `isolate_shells`, which points `HOME` and `ZDOTDIR` at a temp folder.
- macOS `/etc/zshrc` line 15 sets `HISTFILE` under `ZDOTDIR`, and the generated `.zshrc` moves it to `<tmp>/shell-home/.zsh_history`.
- So no line reaches `~/.zsh_history`.
- Run Command uses `/bin/sh -c` with explicit arguments: no startup files, no history.

**Most likely to fail first**

- The expand/collapse key test: key spelling, and the editor's `code.unfold` taking the same key.
- The split test: pane bounds may need a repaint after each split.
- The copy-path test: walking the context menu with the arrow keys.
- The Select In test: it expects a "Terminal" row, which shows only once `terminal.openAt` is registered.
