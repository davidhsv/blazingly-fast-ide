All five parts are done. The whole suite passed 5 times out of 5 and clippy is clean. The intermittent merge-editor failure was a race in the test itself; a separate `index.lock` failure class in Git operations was a real product race, now fixed.

## 1. Suite, clippy, scope

- **Suite ×5:** each run `ok. 6477 passed; 0 failed; 0 ignored`, about 55.5 s.
- **Clippy:** `cargo clippy --all-targets -- -D warnings` is clean.
- **Scope:** I edited only under `app-gpui/`; nothing under `docs/`, `src/`, `src-tauri/` or `crates/`. I never ran `cargo fmt`, only `rustfmt` on files I edited, and no git command that changes anything. No `proofs3_*` file was touched or declared.
- HEAD moved by two commits from another agent during my run (`ac1c4b5e`, `96387912`, which committed the `proofs3` drafts).

## 2. Part 1: the intermittent failures

**The instrument.** When `wait_long` runs out, it now panics with the Git console, the operation in progress, the read queue, the merge-editor and question state, the notices, and a trace of backend events, debounces, reads and operations. It then keeps polling another 3000 iterations and reports whether the wait would have succeeded later. All of this is `cfg(test)`, in `workbench/mod.rs` and `branches.rs`. A failing wait now takes about 60–80 s instead of 30–40 s.

**Before the fix:** filtered to `workbench::vcs_operations` plus the merge-editor test.

| Run      | Processes × rounds | Failures |
| -------- | ------------------ | -------- |
| before3  | 3 × 5              | 6        |
| instr3   | 3 × 5              | 14       |
| before6  | 6 × 5              | 34       |
| before10 | 10 × 5             | 16       |

All 70 failures, grouped:

| Test                               | Wait label                                   | `index.lock` in console | Count                               |
| ---------------------------------- | -------------------------------------------- | ----------------------- | ----------------------------------- |
| merge editor                       | "the dialog"                                 | no                      | 41 (merge editor gone, no question) |
| merge editor                       | "the Conflicts dialog"                       | yes                     | 1                                   |
| rollback                           | "the local copy is deleted"                  | yes                     | 14                                  |
| rollback                           | "the file is unstaged"                       | yes                     | 5                                   |
| rebase onto a branch               | Conflicts dialog / "Rebased clean onto main" | yes                     | 4                                   |
| stopped rebase aborted and skipped | Conflicts dialog                             | yes                     | 2                                   |
| stash then unstash                 | "Unstashed stash@{0}"                        | yes                     | 1                                   |
| cherry-pick and revert             | "the cherry-pick / revert is read"           | no                      | 2                                   |

Every one of the 70 shows `reached later: None`, so none of them was starvation. The four merge-editor failures in the first 3 × 5 run were all in process 3; the test passed 4 of 4 when run alone.

**Classification.**

- **(a) Operation/read race, 28 failures.** For example `fatal: Unable to create '.../.git/index.lock': File exists` while a window read overlapped the command. The trace in `before3/r4_p2` shows a read running from 800365 to 800457 ms while the failing Rollback ran from 800381 to 800396.
  - Cause: `run_op` (stash, rollback, rebase from the menu and so on) never waited for a read in flight. Reads also came from paths other than the debounce. The backend's status read writes the index back (`crates/ide-vcs/src/status.rs:99-117`).
- **Test race, 41 failures, including the original 1-in-13.** `merge.reload` returns at once and the old versions stay shown, so `merge_view_loaded` returns immediately. The decision made by `apply_control` is then dropped when the reload lands. Cancel finds nothing modified and closes without asking (`merge editor=None question=false`).
  - The webview behaves the same (`MergeEditor.tsx:128-145`), so this is a test bug.
  - Hypothesis: the original suite failure was this race; its wait label was never recorded.
- **Two cherry-pick/revert failures.** Hypothesis: the test's own `git revert`/`cherry-pick` lost `index.lock` to the window's read, and `git_may_fail` counted that as a stop on a conflict. That stderr was never captured; the new assert in `git_may_fail` would print it.

**The fixes.**

- **The Git gate** (`branches.rs`, `RefreshQueue::in_flight` in `vcs_ops/store.rs`): `git_command_starts` waits until no read and no other command is running, then sets `git_running`. `start_vcs_refresh` defers any read while it is set, and `git_command_ended` starts the deferred read and wakes waiters.
  - Used by `run_op`, `run_git_operation` and `run_remote`. It replaces their earlier pre-read.
  - The debounce no longer drops events while `remote_busy` is set; it defers them. Before, a skipped debounce was never re-armed. The skip appeared once in a trace, but no failure was tied to it.
- **The test race:** a `cfg(test)` `MergeView::loads()` counter, and the merge test now waits for it after `merge.reload`.
- **`git_may_fail`** now waits out `index.lock` like `git_in`, and asserts if the lock never clears.

**After the fix:** 0 failures in each run.

| Processes × rounds                 | Tests passed, 0 failed |
| ---------------------------------- | ---------------------- |
| 3 × 5                              | 330                    |
| 6 × 5                              | 660                    |
| 10 × 5                             | 1100                   |
| 10 × 15                            | 3300                   |
| merge proofs + agent guard, 10 × 5 | 950                    |

**Still open, and behaviour changes to review:**

- An external `git` can still fail on `index.lock` against the backend's write-back. The tests absorb it by retrying in `git_in`, `set_stage` (the merge fixtures) and now `git_may_fail`.
- Commit or Stage during a long fetch or pull now waits for it instead of running at once.
- If a gated task were dropped mid-command, the gate would stay closed. By reading the code, every caller's task is detached (unmeasured).
- About the watcher under load: in every trace an fs event arrived and its debounce fired. No event loss was observed.

## 3. Parts 2, 4 and 5

- **Part 2:** `workbench/agents.rs` gains `cfg(test)` `spawn_guard::check` at the two workbench call sites, not in the backend: task create, and the calls that can start an agent (prompt, rollback, fork, send queued now, set config).
  - It refuses unless the agent is a Custom agent with `<config_dir>/agents/<id>.toml`, and `config_dir` is under the temp folder. A refusal returns the error and logs it with `tracing::error!`.
  - `AgentSpawnSeam`, a test global, records requests that pass the guard instead of starting them.
  - Test `a_task_with_the_default_agent_is_refused_in_a_test_build_and_starts_no_process`: `claude` (the default) is refused on the New Task page. The seam saw nothing, and neither the store nor the backend has a task. The fake agent passes the guard and reaches the seam. The production path is unchanged.
- **Part 4:** `workbench::tests::smoke_registered_actions::every_action_registered_in_a_project_window_is_known_to_the_manifest`.
  - 491 registered ids, 0 unknown to the manifest. The list is in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/registered_ids.txt`.
  - Matching rule: each `dynamic` entry, keyed by its `source` line, maps to a prefix plus a tail kind (digits, any, `a.b`, NUL-separated, direction). The table must name exactly the manifest's dynamic sources. `toolwindow.` plus any tail is permissive.
- **Part 5:** the `painted_marks` field, its pushes and its accessor are now `cfg(test)` in `editor/view.rs`. `painted_bounds` stays, because hit-testing uses it.

## 4. Part 3: merge editor proofs

17 of the 18 drafted tests are in the build, plus one new test and one unit test. All pass, including 10 × 5 under load.

| Test (`workbench::tests::…`)                                                                                                | `parity.json` keys                                                                                                                    | Status              |
| --------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------- |
| `proofs_merge_apply::applies_those_of_the_chosen_side_only`                                                                 | `action:merge.applyLeft`, `applyRight`                                                                                                | done                |
| `…::resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more`                                     | `merge.resolveSimple`, `merge.revert`                                                                                                 | done                |
| `…::resolve_simple_conflicts_then_apply_writes_both_sides_words_and_stages_the_file`                                        | `merge.apply`, `merge.resolveSimple`                                                                                                  | done                |
| `…::apply_merge_result_asks_about_unresolved_changes_and_writes_the_result`                                                 | `merge.apply`, `context:mergeEditorFocus`                                                                                             | done                |
| `…::accept_theirs_asks_once_the_result_was_changed_and_writes_their_version`                                                | `merge.acceptTheirs`                                                                                                                  | done                |
| `proofs_merge_change::settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it`                   | `merge.ignore`, `merge.accept`                                                                                                        | done                |
| `…::takes_the_text_and_leaves_a_conflict_open_for_the_other_side`                                                           | `merge.accept`                                                                                                                        | done                |
| `…::settles_the_change_at_once_when_resolving_using_a_side`                                                                 | `merge.resolveUsingYours`, `resolveUsingTheirs`                                                                                       | done                |
| `…::merges_a_conflict_whose_sides_changed_different_words`                                                                  | `merge.resolveAutomatically`, `merge.ignore`                                                                                          | done                |
| `…::resolve_merge_change_does_what_its_argument_says_to_the_change`                                                         | `merge.change`                                                                                                                        | done (needed fix 1) |
| `proofs_merge_edit::select_all_copy_cut_and_paste_edit_the_result`                                                          | `merge.selectAll`, `copy`, `cut`, `paste`                                                                                             | done                |
| `…::a_side_pane_copies_its_version_but_is_not_cut_or_pasted_into`                                                           | same four                                                                                                                             | done                |
| `proofs_merge_compare::each_comparison_shows_its_two_versions_in_the_diff_window`                                           | `merge.compareYoursResult`, `compareResultTheirs`, `compareBaseYours`, `compareBaseResult`, `compareBaseTheirs`, `compareYoursTheirs` | done                |
| `…::automatically_apply_non_conflicting_changes_is_checked_stored_and_starts_the_next_merge`                                | `merge.autoApply`, `merge.settings`, `merge.cancel`                                                                                   | done                |
| `…::reload_from_disk_reads_the_versions_again`                                                                              | `merge.reload`                                                                                                                        | done                |
| `proofs_merge_focus::keeps_merge_commands_registered_and_disabled_outside_a_merge_editor`                                   | `context:mergeEditorFocus`, `context:merge.`, `context:diffEditorFocus`                                                               | done                |
| `…::diff_settings_and_jump_to_source_act_on_the_merge_editor_that_has_the_keyboard`                                         | `action:diff.showSettings`, `diff.jumpToSource`                                                                                       | done (needed fix 2) |
| `…::jump_to_source_from_a_changed_merge_asks_first_and_no_keeps_the_work` (new)                                             | `diff.jumpToSource`                                                                                                                   | done                |
| `workbench::conflicts::tests::the_resolve_merge_change_argument_reads_as_an_action_on_the_changes_and_other_shapes_as_none` | `merge.change`                                                                                                                        | done                |

Unit spec counts are taken from the notes and were not recounted: `unit:…/mergeActions.test.ts` 2/3 and `unit:…/mergeModel.test.ts` 12/32 through the app. Both rows are already done by unit tests.

**Product fixes:**

- **`merge.change` now takes its argument `{indexes, action}`.** Changes: `run_merge_action` and `merge_change` in `conflicts.rs`, and `MergeView::apply_change`. This matches `MergeEditor.tsx:469-472`, including `side: "base"` for auto (`mergeModel.ts:211-214`).
- **Diff Settings and Jump to Source now act on the merge editor that has the keyboard** (`diff_window.rs`, `conflicts.rs`), matching `MergeEditor.tsx:343-346`.
  - As decided, Jump to Source asks the Cancel Merge question first when the result was changed (`MergeQuestion::JumpToSource`). The new test proves it.

**Left out:** `annotate_with_git_blame_is_offered_in_the_side_panes_only`. It is kept undeclared in `app-gpui/src/workbench/tests/proofs_merge_annotate.rs`.

- Blocker: no blame view exists in the native merge editor's side panes, and `MergeView::enabled(Annotate)` is always false.
- Enabling the key alone would ship a command that does nothing. `merge.annotate` stays partial; what is missing is per-line blame drawn in the side panes, plus the toggle.

## 5. Hypotheses, with the command that would confirm them

- **The original 1-in-13 suite failure was the reload race.** Confirm: run the suite repeatedly with `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/bin_instr` and read the `merge editor=` line of any failure.
- **The cherry-pick/revert failures came from the test's own git losing the index lock.** Confirm: run the old `git_may_fail` under 10-process stress and capture its stderr.
- **Every gated caller's task is detached.** This was read from the code, not measured. Confirm: `grep -n "= Some(cx.spawn\|= cx.spawn" app-gpui/src/workbench/*.rs` and check that no stored task reaches `run_op`, `run_git_operation` or `run_remote`.

Nothing is committed.
