I've written the draft and the integration notes. Nothing was compiled or run (no cargo), so every test below is unverified until the integrator builds and runs it.

## Files written

All are new, untracked and declared nowhere. On integration, drop `_pending` from each name.

| File                                                                        | Lines | Contents                                                                                                                                                                          |
| --------------------------------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/vcs_ops/git_ops_pending.rs`                                   | 265   | The tag menu, the "Deleted tag" notice with its buttons, Create Git Repository's and Stash Selected's notices, where Move Worktree puts a worktree; 4 tests                       |
| `app-gpui/src/vcs/clone_form_pending.rs`                                    | 299   | The Clone Repository dialog's form, drawn by the existing Git form dialog, plus remembered URLs; 4 tests                                                                          |
| `app-gpui/src/workbench/vcs_operations/menu_ops_pending.rs`                 | 469   | Registers `vcs.createRepository`, `vcs.clone`, `vcs.stashSelected`; the tag menu's commands and Restore / Delete on Remote; the clone run with progress and cancel; Move Worktree |
| `app-gpui/src/workbench/vcs_operations/tests/proofs49_git_ops_pending.rs`   | 288   | 6 window tests                                                                                                                                                                    |
| `app-gpui/src/workbench/vcs_operations/tests/proofs49_clone_pending.rs`     | 75    | 1 window test                                                                                                                                                                     |
| `app-gpui/src/workbench/annotate/tests/proofs49_ignore_commit_pending.rs`   | 29    | 1 window test                                                                                                                                                                     |
| `app-gpui/src/vcs_ops/commit_view/tests/proofs49_stash_selected_pending.rs` | 17    | 2 spec tests                                                                                                                                                                      |
| `$S/drafts/git_ops_integration.md`                                          | ~350  | Notes sections (a)–(g)                                                                                                                                                            |

The workbench code is a child of `vcs_operations`, not a separate `workbench/*.rs` module, so it can use that module's private helpers. For the same reason the window tests sit under `vcs_operations/tests/` rather than `workbench/tests/`, as the existing `proofs11_worktrees.rs` does.

The notes hold 10 groups of hunks for existing files (B0–B9), each with an anchor copied from HEAD. B0 goes first because every Git test in the slice depends on it.

## The list, totals per verdict

- **build:** 15 rows (including three found on the way: `context:projectLocal`, the title bar's fallback entry, the changes-tree row).
- **already there:** 1 (`proofs6_app_actions.rs`). Declaring it needs only `mod proofs6_app_actions;` in `app_actions.rs` tests, plus B0.
- **needs:** 8:
  - `vcs_stage_hunks`: per-change controls in the diff viewer.
  - `vcs_file_mode`: side modes and a "File mode changed" header in the diff.
  - `vcs_revision_files`: the log's path filter dialog, the revision browser, a file-of-revision chooser.
  - `vcs_revision_children`: the revision browser.
  - `vcs_compare_paths`: the path comparison view.
  - `vcs_stashes`: the Commit window's stash list (also where its Drop would live).
  - The Branches popup's tag submenu.
  - The Unstash dialog's Show Changes.
- **cannot be tested:** 0, on hypothesis H1 below.

## Rows after the slice

| Row                                                                   | State                                                                                                                          | Tests                                                                                                                                                                                                |
| --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:vcs.createRepository`, `context:projectLocal`, `ipc:vcs_init` | done                                                                                                                           | `proofs49_git_ops::create_git_repository_initializes_the_folder_chosen_and_the_window_reads_it` (+ `proofs6_app_actions::new_project_with_git_creates_a_repository_in_the_new_folder` once declared) |
| `ipc:scaffold_create`                                                 | partial (generators, steps view)                                                                                               | proofs6 test above                                                                                                                                                                                   |
| `action:vcs.clone`                                                    | partial: no progress bar, Cancel closes the dialog instead of keeping it open with "Clone cancelled", no clone-sources sidebar | `proofs49_clone::clone_clones_into_the_folder_chosen_and_opens_the_clone`                                                                                                                            |
| `ipc:vcs_clone`, `unit:…/clone.test.ts`                               | done                                                                                                                           | same test; the spec's cases were already model tests                                                                                                                                                 |
| `ipc:vcs_delete_tag`, `ipc:vcs_restore_tag`                           | done for the Git log's Branches pane; popup still missing                                                                      | `deleting_a_tag_from_its_menu_offers_restore_which_puts_it_back`, `a_tag_is_pushed_from_its_menu_and_deleted_on_the_remote_from_the_notification`                                                    |
| `ipc:vcs_stash_drop`                                                  | partial: Commit view's stash list, Show Changes                                                                                | `drop_stash_asks_then_drops_the_chosen_stash_and_lists_the_rest`                                                                                                                                     |
| `action:vcs.stashSelected`, `ipc:vcs_stash_push`                      | done                                                                                                                           | `stash_selected_files_stashes_only_the_files_given`                                                                                                                                                  |
| `unit:…/stashSelectedFiles.test.ts`                                   | done (2/2)                                                                                                                     | `targets_the_selected_files`, `is_disabled_when_no_file_is_selected`                                                                                                                                 |
| `ipc:vcs_move_worktree`                                               | done (the folder chooser can't be told where to start)                                                                         | `move_worktree_moves_a_linked_worktree_into_the_folder_chosen`                                                                                                                                       |
| `ipc:vcs_ignore_blame_revision`                                       | done                                                                                                                           | `ignore_this_commit_blames_its_lines_on_the_commit_before`                                                                                                                                           |
| `ipc:vcs_stashes` and the 5 other needs rows                          | unchanged                                                                                                                      | —                                                                                                                                                                                                    |

One existing test changes its expectation: `annotate::tests::the_columns_menu_copies_the_revision_and_closes_the_annotations` (`annotate.rs:574`) now also lists "Ignore This Commit" (webview `blame.ts:286`).

## Hypotheses

- **H1:** the backend's git can be kept off the machine's configuration without `unsafe`. All its git child processes go through `crates/ide-vcs/src/cli.rs:252-281`, which adds the `ide_vcs::set_askpass_env` list to each one (`:277`). B0 sets `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` to `/dev/null` through that list, in `start_in`. Nothing in `app-gpui/src` or `crates/` calls it; only `src-tauri/src/state.rs:213` does. Hypothesis: no other test depends on the global configuration. Check: run the workbench suite under both nextest and `cargo test`.
- **H2:** `prompt_for_paths` answers `Result<anyhow::Result<Option<Vec<PathBuf>>>, _>`. Check: compile.
- **H3:** the backend's blame honours `blame.ignoreRevsFile`. Check: the Ignore This Commit test.
- **H4:** apart from `worktrees` (patched in B8), no exhaustive `match` covers `FormAnswer` or `WorktreesCommand`. Check: compile.
- **H5:** after `git init`, the watcher's rescan emits `VcsChanged` (`crates/ide-host/src/project.rs:447-466`, `:916-921`), and that makes `VcsState.available` true. Check: the Create Git Repository test's second wait.
- **H6:** the Branches pane's search makes a tag row visible. Check: the tag tests' "the tag is listed" wait.
- **H7:** `ShowTagMenu` reaches the workbench through `log_branches_event`. Check: the tag tests' menu wait.
- **H8:** the stash message is git's reflog subject (`On main: …`). Check: `crates/ide-vcs/src/refs.rs:161`.
- **H9:** Escape on the confirmation answers Cancel. Check: the Drop Stash test.
- **H10:** `file.openProjectPath` asks about trust when it opens a new clone in a window with no project. Check: the clone test.

## What turned out wrong

- **`projectLocal` is never published natively.** `git grep -i projectlocal HEAD -- app-gpui/src` finds only `app_models/windows.rs:977`, so `vcs.createRepository` (`when: projectOpen && projectLocal`) could never run. `project.attach` names the same key; Hypothesis: it is unavailable too. B3 publishes the key.
- **The Restore button of a deleted tag had no route.** Webview keys start with `vcs.deletedTag.`, but the native routing (`mod.rs:1424`) matches `vcs.deleted.`, which does not prefix it. B5 adds a route.
- **The existing Git tests do not keep the backend's git off the machine's configuration.** Only their own `git_in` calls are isolated; the backend's git reads the machine config, which `app_actions.rs:1663` says outright. This is why the proofs6 test turned Git off.
- **Brief vs. source:**
  - The annotate row is labelled "Ignore This Commit", not "Ignore Revision".
  - Git log tag menus arrive as `ShowTagMenu`, which the workbench only logs (`log_window.rs:407`).
  - The Branches popup's tag rows do nothing (`branches_popup.rs:798`).
  - `docs/gpui-port/notes/designs/ipc_rows_read_3.md` is right on every Git command except that its `vcs_init` entry misses the `projectLocal` gap.
- The sample notes the wiring brief points to (`drafts/conflicts_integration.md`) does not exist; the notes use the brief's own (a)–(g) layout instead.
