I've written the three new files and the notes, but nothing has been compiled or run, so none of the tests below has passed yet. No existing file was edited.

## 1. Files written

- `app-gpui/src/vcs/git_forms.rs` (1892 lines): the dialog models with no GPUI in them. Each is a `GitForm` (`spec(i18n) -> FormSpec`, `edit(id, FormInput)`, `press(button, i18n) -> Option<FormAnswer>`, `set_error`). The forms are `PullForm`, `UpdateForm`, `MergeForm`, `RebaseForm`, `ResetForm`, `TagForm`, `StashForm`, `UnstashForm`, `RollbackForm`, `RemotesForm`, `RemoteForm`, `WorktreesForm` and `NewWorktreeForm`. Helpers: `disabled_merge_flags`, `merge_options_of`, `remote_branches_of`, `validate_remote_name`, `default_worktree_path`, `reset_mode_name`, `can_press`. It has 11 plain tests.
- `app-gpui/src/vcs/git_form_dialog.rs` (741 lines): one view, `GitFormDialog`, that draws any form.
  - **Constructor:** `new(Box<dyn GitForm>, cx)`.
  - **Events:** `GitFormEvent::{Answer(FormAnswer), Cancelled}`.
  - **Accessors:** `spec`, `title`, `field(id)`, `open_select`, `edit`, `set_text`, `choose`, `toggle`, `toggle_row`, `press`, `set_error`.
  - 4 `#[gpui_kit::test]`s.
- `app-gpui/src/workbench/vcs_operations.rs` (1731 lines): registration, flows and 16 end-to-end tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/vcs_operations_integration.md`.

**`pub mod` lines** (in `vcs/mod.rs`): `pub mod git_form_dialog;` and `pub mod git_forms;`. `workbench/mod.rs` also gets `mod vcs_operations;`.

**Other edits listed as exact hunks in the notes:**

- `prompt_dialogs.rs`: `checkbox`, `button`, `field_frame` and `code_font` become `pub(crate)`.
- `branches.rs`: `i18n`, `run_remote`, `ask`, `confirm` and `run_git_operation` become `pub(super)`. `run_git_operation` takes `progress: &str, success: &str` because Reset's success text is built at run time.
- `commit.rs`: a new `commit_amend(&self, &App)`, and `remember_commit_message` becomes `pub(super)`.
- `mod.rs`: the new field, its initial value, the registration call, and `pub(super)` on 10 test helpers so the tests in my file can use them.
- No menu entries need adding; `menu_spec.rs` and the context menus already name these ids.

## 2. Actions registered (29) → test that runs it → IPC commands

| Action                                                | Test                                                                                | IPC commands                                                         |
| ----------------------------------------------------- | ----------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `vcs.pull`                                            | `pull_brings_in_a_commit_from_the_remote`                                           | `vcsRemotes`, `vcsBranches`, `vcsRemote{pullFrom}`                   |
| `vcs.update`                                          | `update_project_pulls_with_the_method_chosen_and_resets_to_the_remote_branch`       | `settingsSet`, `vcsRemote{pull}`, `vcsRemote{resetToUpstream}`       |
| `vcs.merge`, `vcs.merge.abort`                        | `merge_merges_a_branch_and_abort_merge_undoes_one_stopped_on_conflicts`             | `vcsBranches`, `vcsMergeWith`, `vcsOperation{abort}`                 |
| `vcs.rebase`, `vcs.rebase.continue`                   | `rebase_onto_a_branch_and_continue_a_rebase_that_stopped`                           | `vcsRebaseWith`, `vcsOperation{continue}`                            |
| `vcs.rebase.abort`, `vcs.rebase.skip`                 | `a_stopped_rebase_is_aborted_and_skipped`                                           | `vcsOperation{abort}`, `vcsOperation{skip}`                          |
| `vcs.cherryPick.*`, `vcs.revert.*`, `vcs.refresh`     | `a_stopped_cherry_pick_and_revert_are_aborted_and_continued`                        | `vcsOperation`; refresh is the window's usual read                   |
| `vcs.stash`, `vcs.unstash`                            | `stash_then_unstash_restores_the_change`                                            | `vcsStashPush`, `vcsStashes`, `vcsStashApply`                        |
| `vcs.rollback`                                        | `rollback_restores_a_modified_file_and_deletes_an_added_one_only_when_asked`        | `vcsUnstage`, `vcsDiscard` (git clean, not the system Trash)         |
| `vcs.addToVcs`, `vcs.ignoreShared`, `vcs.ignoreLocal` | `add_to_vcs_stages_an_unversioned_file_and_the_ignore_actions_write_their_own_file` | `vcsStage`, `vcsAddToIgnore`                                         |
| `vcs.resetHead` (soft, mixed, hard)                   | `reset_head_moves_the_branch_in_each_mode`                                          | `vcsOperation{reset}`                                                |
| `vcs.newTag`                                          | `new_tag_creates_a_tag_and_refuses_an_invalid_or_duplicate_name`                    | `vcsTags`, `vcsCreateTag`                                            |
| `vcs.manageRemotes`                                   | `the_remotes_dialog_adds_edits_and_removes_a_remote`                                | `vcsRemotes`, `vcsEditRemote`                                        |
| `vcs.newWorktree`, `vcs.worktrees`                    | `new_worktree_creates_one_that_the_worktrees_dialog_lists_and_removes`              | `vcsBranches`, `vcsAddWorktree`, `vcsWorktrees`, `vcsRemoveWorktree` |
| `vcs.operations`                                      | `the_operations_popup_lists_the_operations_in_order_and_runs_the_chosen_one`        | none                                                                 |
| `vcs.showLog`, `vcs.showCommit`                       | `show_log_shows_the_git_window_and_show_commit_selects_the_commit`                  | none                                                                 |
| `vcs.toggleAmend`                                     | `amend_commit_toggles_amend_with_the_last_commits_message`                          | `vcsLog`                                                             |
| `vcs.openDiff`                                        | `open_diff_shows_the_diff_of_its_argument`                                          | none                                                                 |

**Not registered:** `vcs.compareWithBranch`. It needs the revision chooser popup and the "Changes between" comparison window, and neither has a view yet; a text prompt would be a different dialog.

## 3. Spec coverage

- `worktreesPanel.test.tsx`: 5 of 5 cases, as model tests in `git_forms.rs`. Each `it.each` is a loop inside one test.
- `gitOps`, `ignore`, `rollback`, `historyOps` and `multiRootTags` were already covered by the existing models. My files reuse those models and add no spec cases.
- `compareActions.test.ts` and `compareFlow.test.ts`: not covered, because the compare action is not wired.

## 4. What is not built, assumptions, and the likely first failures

**Not built:**

- `vcs.showLog` with a branch or path argument opens the main log; the Rust log has no tabs.
- `vcs.showCommit` only selects a commit the log has already read. It does not page the log further or clear its filters.
- Rollback Lines (Rollback in a focused editor with the caret on changed lines) is not built.
- The tag message is a one-line field; the webview has a 4-row textarea.
- In the Pull dialog, the remote and branch selects are stacked rather than side by side.
- New Tag and Remotes don't reload the Git log afterwards.
- Add to .gitignore doesn't reload the project tree; it relies on the file watcher.

**Integrator should check first:**

1. `start(...)` takes the flows as `impl AsyncFnOnce(This, &mut AsyncWindowContext)`. The notes give a fallback if rustc refuses it.
2. `git rebase/cherry-pick/revert --continue` need no editor, assuming the backend sets `GIT_EDITOR=true`.
3. `BranchInfo.operation` reads `"cherry-pick"` and `"revert"` while those operations are stopped.
4. The options list of a select is `deferred(priority 2)` inside the dialog layer, the same nesting the Push dialog uses.
5. `TextField` has no `set_placeholder`, so the view rebuilds an empty field when its placeholder changes. The notes suggest adding the method.

**Most likely to fail on first run:**

- **Cherry-pick/revert test:** it depends on 2 and 3, and on git stopping on a modify/delete conflict.
- **Operations popup test:** it asserts every listed row is enabled. If another module registers `vcs.showHistory`, `vcs.annotate` or `vcs.localHistory` with a `when` that is false here, it fails.
- **Worktree test:** it compares the path `git worktree list` prints with the canonicalised temp path (`/private/var` on macOS).
- **Update test:** it assumes a project-scope setting write is visible to `settings::effective("w1")` straight away.
