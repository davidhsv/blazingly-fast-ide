All three stages are integrated and the suite passes: the last five full runs gave `6222 passed; 0 failed; 0 ignored` each, and clippy with `-D warnings` reports nothing. There are no `#[ignore]`s. Several deviations from the webview remain (listed below), and a few things are not isolated or not tested.

The suite now takes 24–26 s instead of 17–18 s. `log_reset_dialog.rs` and `log_tag_dialog.rs` are still on disk and not declared.

## Stage 1: repository operations

- **Compile errors (11):**
  - 10 in `git_forms.rs`: it used `ide_vcs::Branch` where `sort_local`/`sort_remote` take `vcs::branches::Branch`. I added a `sortable()` conversion.
  - 1 missing `AppContext` import in `vcs_operations.rs`.
  - Clippy: 3 explicit-deref warnings in `git_form_dialog.rs`.
- **Tests:** `vcs_operations git_form` 33 passed. Whole suite and clippy clean after the stage.
- **Existing tests changed:**
  - The title-bar test (`the_title_bars_menus_open_and_the_context_menu_hides_and_restores_an_entry`): it expected no "Version Control" fallback. With `vcs.showLog` now registered, the webview draws it (`TitleBar.tsx:96-107`), so the test now asserts the fallback and its Show Git Log row. The notes did not mention this.
  - The two `commit_view` tests now set `status.showUntrackedFiles=normal` in their own repositories (see git config below).
  - Test helpers `start`, `search_project` and `type_query` became `pub(super)`, beyond the notes' list.
- **Drafted tests fixed:**
  - The Update test checked the second update before the first had finished: it read `git log`, the first update was still running, and the second was refused with "Another remote operation is in progress". It now waits for the "updated" notice first.
  - The Rollback Lines test I added did not wait for the status read.
- **Bugs fixed:**
  - `vcs.rollback` in a focused editor now does Rollback Lines, as the webview does. It uses `vcs_line_hunks` with the existing `HunkState` model and asks again if the text changed while waiting.
  - Remote URLs could show credentials in the Remotes list, the Pull remote choice and Edit Remote; I mask them there (details under Security).
  - The index-lock race (see "The two timeouts").
- **Notes that were false:**
  - The `Branch` types: the notes assumed one type.
  - "every type is pub, `pub(super)` is enough": true.
  - Missing `init_repo` keys: `tag.forceSignAnnotated`, `rerere.enabled`, `diff.context` (1000 on this machine), `diff.noprefix`, `diff.mnemonicPrefix`, `status.showUntrackedFiles`, `pull.rebase`, `push.default`, `core.excludesFile`.

## Stage 2: shelf, change lists, patches, before-commit checks

- **Compile errors:** none.
- **Tests:** `shelf change_dialogs patch_dialogs commit_view commit_box` 59 passed.
- **Bugs fixed:**
  - `commit_view.rs` did not update on working-tree edits: the backend only emits `VcsChanged` for `.git` changes. I added `CommitPanel::set_status`, fed from `changelists_follow_status` on every repository read. Measured: the tree stayed stale for 30 s before the fix.
  - The tree's checkboxes ignored change lists. They now use `with_list_defaults`, as `CommitView.tsx:422,608` does, so only the active list is included by default.
  - Analyze code waited the full 2 s for a file a language server had reported clean. An empty publication removes the file from `ProblemsMap`, whereas the webview counts any event as analysed (`commitChecks.tsx:34`). I added `ShelfSlot.analyzed`, filled from `diagnostics_published` (one line in `code_insight.rs`).
- **Drafted tests fixed:** two waits read a file that `git apply` briefly removes; they now tolerate a missing file.
- **Commit timing.** The 2 s wait is the webview's (`commitChecks.tsx:315`, `waitForAnalysis(…, 2000)`). Measured per test:

| Test                                                                         | Before | After  |
| ---------------------------------------------------------------------------- | ------ | ------ |
| `commit_takes_exactly_the_included_files_with_the_typed_message` (3 commits) | 1.13 s | 5.48 s |
| `a_refused_commit_keeps_its_message_and_the_options_reach_the_commit`        | 0.73 s | 2.22 s |
| `amend_loads_the_last_message_and_amends_the_commit`                         | 0.50 s | 1.98 s |

The first is over your "a second or two". Its files are `.txt`, which no server reports on, so the app's wait does apply. I left the tests unchanged.

- **Notes that were true:** `committed()` clears `check_problems` (`change_ops.rs:112`); "Changes (2)" holds with only the default list.

## Stage 3: Git log actions

- **Compile errors (1):** a missing lifetime in the `log_message_dialog.rs` test helper.
- **Tests:** `log_actions interactive_rebase log_message_dialog log_view log_window prompt_dialogs` 44 tests; `proofs_vcs` and `proofs_update` 5 passed.
- **Drafted tests fixed:** the commit menu marks the active row with `"> "`, which the draft's row lookups did not expect.
- **Bugs fixed:**
  - Revert Selected Changes always said "Nothing to revert". `git diff -R` writes headers as `b/… a/…`; `patch_header_paths` now accepts both orders, with a unit test. The webview's `logChanges.ts:188` regex has the same bug: I inferred that from the regex and did not run it.
  - Undo Commit now asks "Select Target Changelist" and moves the undone files there, as `rebase.tsx:69-81` does.
  - Commit actions given a commit id now read it from the repository when the log has not loaded it, or was never shown.
- **History-rewriting rows for pushed commits:** I checked against `logStore.ts:757-793` and `GitLog.tsx:1150-1214`; they match (no upstream means not pushed).

## Registered actions

All are run by at least one test. Rows not marked partial are done.

| Action                                                | Test                                                                | Status                                                                      |
| ----------------------------------------------------- | ------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `vcs.pull`                                            | `pull_brings_in_a_commit_from_the_remote`                           | done                                                                        |
| `vcs.update`                                          | `update_project_pulls_…`, `proofs_update`                           | done                                                                        |
| `vcs.merge`, `vcs.merge.abort`                        | `merge_merges_a_branch_and_abort_merge_…`                           | done                                                                        |
| `vcs.rebase`, `vcs.rebase.continue/abort/skip`        | `rebase_onto_a_branch_…`, `a_stopped_rebase_is_aborted_and_skipped` | done                                                                        |
| `vcs.cherryPick.*`, `vcs.revert.*`, `vcs.refresh`     | `a_stopped_cherry_pick_and_revert_…`                                | done                                                                        |
| `vcs.stash`, `vcs.unstash`                            | `stash_then_unstash_restores_the_change`                            | done                                                                        |
| `vcs.rollback`                                        | `rollback_restores_…`, `rollback_in_an_editor_…`                    | done                                                                        |
| `vcs.addToVcs`, `vcs.ignoreShared`, `vcs.ignoreLocal` | `add_to_vcs_stages_…`                                               | done                                                                        |
| `vcs.resetHead`                                       | `reset_head_moves_the_branch_in_each_mode`                          | done                                                                        |
| `vcs.newTag`                                          | `new_tag_creates_…`                                                 | partial: the message is a one-line field (the webview has a 4-row textarea) |
| `vcs.manageRemotes`                                   | `the_remotes_dialog_…`                                              | partial: credentials masked, which the webview shows in clear               |
| `vcs.newWorktree`, `vcs.worktrees`                    | `new_worktree_creates_…`                                            | done                                                                        |
| `vcs.operations`                                      | the operations popup test                                           | done                                                                        |
| `vcs.showLog`                                         | `show_log_shows_…`                                                  | partial: a branch or path argument opens the main log                       |
| `vcs.showCommit`                                      | `show_log_shows_…`                                                  | partial: selects only commits the log has read                              |
| `vcs.toggleAmend`                                     | `amend_commit_toggles_…`, the shortcut test                         | done                                                                        |
| `vcs.openDiff`                                        | `open_diff_shows_…`                                                 | done                                                                        |
| `vcs.compareWithBranch`                               | —                                                                   | not registered                                                              |

| Stage 2 action                                                                                                      | Test                                                            | Status                                                                                                          |
| ------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `vcs.changelist.new/edit/setActive`                                                                                 | `new_changelists_…`                                             | done                                                                                                            |
| `vcs.changelist.move/delete`                                                                                        | `moving_changes_…`, `a_change_dragged_…`                        | done                                                                                                            |
| `vcs.changelist.commit`                                                                                             | `commit_changelist_commits_only_its_files`                      | done                                                                                                            |
| `vcs.shelve`, `vcs.showShelf`, `vcs.shelf.refresh`, `vcs.unshelve`, `vcs.shelf.rename/delete`                       | the shelve, unshelve and rename tests                           | partial: shelf rows show no time                                                                                |
| `vcs.shelf.showDiff`, `vcs.shelf.compareLocal`                                                                      | `show_diff_of_a_shelved_file_…`, `show_diff_in_the_shelf_tab_…` | done                                                                                                            |
| `vcs.shelf.showDiffNewTab`                                                                                          | the same tests                                                  | partial: opens the diff window, not a tab                                                                       |
| `vcs.shelf.copyPatch/createPatch/importPatches`, `vcs.createPatch`, `vcs.applyPatch`, `vcs.applyPatchFromClipboard` | their tests                                                     | done                                                                                                            |
| Commit checks                                                                                                       | the check tests                                                 | partial: Reformat, Optimize imports and Cleanup are disabled with a reason; TODO hits are plain text, not links |

| Stage 3 action                                                                    | Status                                                                                     |
| --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| `vcs.log.compareWithLocal`, `vcs.log.compareVersions`, `vcs.log.changes.compare*` | partial: open the diff window instead of the "Changes between" list                        |
| `vcs.log.goToRef`                                                                 | partial: finds any ref but selects only loaded commits (no paging or filter reset)         |
| Commit menu                                                                       | partial: no multi-selection, so the many-commit menu is reachable only through an argument |
| `vcs.log.collapseLinear`, `vcs.log.expandLinear`                                  | partial: stored option and model only; the collapsed graph is not drawn                    |
| `vcs.push` up to here                                                             | partial: the dialog still lists every outgoing commit                                      |
| All other `vcs.log.*` commit, branches-pane and changed-files actions             | done                                                                                       |
| `vcs.log.openExternal`                                                            | not registered                                                                             |

Also missing: there is no log toolbar.

## Shortcuts

The conditions come from the manifest (`is_action_key_enabled`); nothing hand-written contradicts it.

| Key          | Condition                                                | Test                                                                                                |
| ------------ | -------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| ⌘T, ⌥⌘Z, ⌥⌘A | `vcsAvailable`: inert without a repository, run with one | `the_git_shortcuts_run_only_where_their_conditions_hold`, `in_a_repository_the_git_shortcuts_run_…` |
| ⌃V           | `projectOpen`                                            | same first test                                                                                     |
| ⌃⌥M          | Commit window active                                     | `in_a_repository_…`, plus Find Action toggling with no re-entrancy panic                            |
| F2, ⇧⌘M      | Commit window active                                     | `the_change_list_keys_work_only_in_the_commit_window`                                               |
| ⌥⇧⌘C, ⌘F     | Git window or `vcsLogFocus`; in an editor neither runs   | `the_logs_keys_work_while_the_log_has_the_keyboard`                                                 |
| Eclipse ⌥⇧R  | not default                                              | not tested                                                                                          |

## Context keys

- **`vcsLogFocus`:** published in `publish_log_context` (focus in the log or one of its panes, like `[data-git-log]`). Read on and off by `the_logs_keys_work_while_the_log_has_the_keyboard`.
- **`shelfFocus`:** read on and off by `show_diff_in_the_shelf_tab_shows_the_selected_shelved_file`.

## IPC commands the tests prove

| Test                               | Command                                                  |
| ---------------------------------- | -------------------------------------------------------- |
| `proofs_update`                    | `vcs_merge_base`                                         |
| `rollback_in_an_editor_…`          | `vcs_line_hunks`                                         |
| Undo Commit                        | `vcs_commit_files`, `vcs_undo_commit`                    |
| `the_changed_files_actions_…`      | `vcs_create_patch` (commits, reverse), `vcs_apply_patch` |
| `commit_and_rebase_after_fixup_…`  | `vcs_operation` (interactive rebase)                     |
| `push_all_up_to_here_…`            | `vcs_remote` push with revision                          |
| `the_commit_menu_disables_…`       | `vcs_log` range `@{u}..HEAD`                             |
| `a_commit_action_reads_a_commit_…` | `vcs_log` by id                                          |

The drafts' per-test tables for the other commands still apply.

## Git config isolation

I measured this by running the whole binary with `GIT_CONFIG_GLOBAL` pointing at a hostile config (signing on, autocrlf, a bogus hooks path, `pull.rebase`, `merge.ff=false`, `diff3`, `noprefix`, autosquash/autostash/updateRefs, `rerere`, an excludes file, `showUntrackedFiles=no`):

- **Without the excludes file:** 6222 passed after the `init_repo` and `commit_view` additions. Before them, `diff.noprefix` broke 5 shelf and patch tests and `status.showUntrackedFiles` broke 3.
- **Cannot be isolated this way:** with a global `core.excludesFile` matching `*.txt`, 4 tests never see their file change, even though the repo sets `core.excludesFile=/dev/null` and plain `git status` honours that (measured). Hypothesis: the IDE's own change detection reads the global excludes file. To confirm, find which reader in `ide-host`/`ide-vcs` ignores the repository's setting.
- **This machine:** its own `~/.gitignore_global` does not hit the test files (the suite is green here).

## The two timeouts

- **Measured:** I ran both tests in 10 parallel processes, 150 runs: 3 failed. One failure's Git console shows `git merge --abort` failing with `index.lock: File exists`.
- **Hypothesis for the lock holder:** a debounced repository read whose status refresh writes the index back (`ide-vcs/src/status.rs` `write_changes`). The CLI retries only switch/add/rm/reset. The rebase test's cause is the same hypothesis.
- **Fix in `branches.rs`:**
  - no debounced read starts while `remote_busy` is set;
  - `run_git_operation` and `run_remote` wait for any read in progress before running.
- **After the fix:** 0 failures in 150 runs, and 0 in 400 runs on the final binary.
- **Not covered:** operations that run through `run_op` (stash, rollback, unshelve, apply patch, merge and rebase from the dialogs). The backend should also serialise its index write-back with the CLI operations.

## Security grep

- **`tracing::`:** every call in the new files logs error text or ids, none a URL or credential.
- **Masking:** `https://user:pw@…` shows as `user:***`, and a bare user info over HTTP(S) shows as `***`. This covers the Remotes list, Pull's remote choice and Edit Remote's field. Editing the URL while leaving the mask keeps the stored password; a new user info replaces it. Tested in `a_remotes_password_is_never_shown_and_survives_an_edit`.
- **`Debug`:** `RemotesCommand` and `GitFormEvent` have redacting `Debug` impls, and `FormAnswer` has none.
- **Remaining exposure:** `FormRow`, `FormSpec` and `FormInput` still derive `Debug` and can carry a URL the user is typing.
- **Other paths:** View in Browser drops the user info (`remote_web_url`). Rollback deletes through `git clean`, not the system Trash.

## Out of scope, found wrong

- The webview's Revert Selected Changes header regex (above).
- The backend's patch creation breaks under `diff.noprefix`: it does not pin prefixes.
- The backend's index write-back is not serialised with operations; the CLI retries only some commands.
- `core.excludesFile` is read from the global config despite the repository's override.

Scratch files are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`, including the hostile config `hostile.gitconfig`.
