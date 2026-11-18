I finished the slice and every gate passes: both whole-suite runs (8,327 of 8,327 each), 20 of 20 repeats of all 16 slice tests, `web_view`, clippy and a build with 0 warnings. The real-window shots are taken. Eight existing tests changed, as you accepted; nothing is committed. This report replaces the interim one.

## 1. Gates

| Gate                                                       | Result                                                                   |
| ---------------------------------------------------------- | ------------------------------------------------------------------------ |
| Whole suite, run A (`cargo test --lib`, nothing beside it) | 8,327 passed, 0 failed, 224.8 s. Load 5.56 before, 84.97 after.          |
| Whole suite, run B                                         | 8,327 passed, 0 failed, 230.7 s. Load 67.41 before, 118.57 after.        |
| Twenty-fold repeats (started after run B ended)            | 20 of 20 runs: all 16 tests passed every time. No test failed even once. |
| `cargo test --test web_view`                               | 1 passed                                                                 |
| `cargo clippy --all-targets -- -D warnings`                | Exit 0                                                                   |
| `cargo build`                                              | Exit 0, 0 warnings                                                       |

Free disk space stayed at or above 322 GB throughout.

The 16 repeated tests:

- **New:** the 6 in `workbench::tests::proofs42_gitlog_real_repo_pending`, plus `vcs::log_view::tests::the_graph_of_three_thousand_commits_is_laid_out_in_one_pass` and `workbench::log_actions::tests::copy_commit_message_copies_the_whole_message_of_a_commit_listed_by_its_subject`.
- **Changed:** the 8 existing tests in section 5.

The one failure seen at any time was the interim whole-suite run, before the last two tests were fixed. It failed `the_branches_pane_actions_act_on_the_selected_branch` and `proofs4_vcs::the_log_date_filter_keeps_the_commits_of_the_chosen_period`, both on "the log is read did not happen within thirty seconds": each waited for commits that HEAD does not hold. None of the eight load-sensitive run/debug tests failed in any run.

Logs are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dglog/`: `suiteA.log`, `suiteB.log`, `repeats.log`, `web_view.log`, `clippy2.log`, `build2.log`.

## 2. Each test failed before its fix

Run 1 had only H1's selectors applied (`run1_before.log` in the same folder). All five tests failed, each for the notes' stated reason:

| Test                                                                        | Failure                                              |
| --------------------------------------------------------------------------- | ---------------------------------------------------- |
| `the_log_opens_on_the_current_branch_and_its_branch_chip_reads_head`        | Chip `Some((Branch, None))`, expected `Some("HEAD")` |
| `with_every_branch_shown_the_newest_rows_draw_their_node_and_their_subject` | `graph()[0].lane` was 40, expected 0                 |
| `the_log_reads_a_page_ahead_and_page_down_moves_twenty_rows`                | Timed out on "a second page is read ahead"           |
| `showing_the_git_window_gives_the_keyboard_to_the_branch_filter_field`      | "the branch filter has the keyboard" failed          |
| `a_hidden_log_reads_the_repository_again_only_when_it_shows`                | 4 commits, expected 3: the log re-read while hidden  |

I replaced test 5 with `the_log_is_read_again_when_a_reference_moves_and_not_on_other_changes`, because the webview does re-read a hidden log (section 3, H6).

Tests that did not exist in run 1 were proven by switching one behaviour off per run. The script copies the file aside and restores it on exit (`mut/mutate.sh`); the files' hashes were identical before and after every run. The script's own "restored:" line never printed, because the trap referred to `$5`, so the hash comparison is the proof.

| Test                               | Behaviour switched off        | Failure                                                     |
| ---------------------------------- | ----------------------------- | ----------------------------------------------------------- |
| Replacement test 5                 | Reload on every VCS sync      | "no reference moved: the log is not read again" (reads = 2) |
| Replacement test 5                 | Never reload                  | Timed out on "the log lists the new commit"                 |
| Copy-message test                  | The whole-message read        | Clipboard held `Some("Second")`                             |
| Branch-memory test (section 3, H6) | Remembering the branch choice | Timed out on "the log is read with the remembered filter"   |

## 3. Per hunk

Files changed, all under `app-gpui/src/`: `vcs/log_view.rs`, `workbench/log_window.rs`, `workbench/log_actions.rs`, `vcs_more/log_branch_memory.rs`, `workbench/mod.rs`, `workbench/tests/proofs42_gitlog_real_repo_pending.rs` and `workbench/tests/proofs4_vcs.rs`. The test file is declared after `mod proofs41_recent_edited;`. Only these files were formatted, with `$S/fmt.sh`.

- **H1:** the `git-log-graph-{index}` and `git-log-subject-{index}` selectors, plus `SUBJECT_MIN = 120` (from `GitLog.tsx:1389-1396`), as the notes say.
- **H2:** as the notes, except the pipeline is a free function `present_graph(&[GraphCommit], long_edges)` so it can be timed in a unit test.
  - The native code has no port of the webview's `pruneFilteredEdges` step (`logPresentation.ts:33`, `graph.ts:273`). Neither the notes nor `vcs_more::present_log_graph` include it.
- **H3:** as the notes: graph cell at its row's width, date 140 px, details 28%. Two additions:
  - `flex_shrink(1.0)`, since GPUI's `flex_shrink` takes an argument.
  - Each reference label also shrinks to an ellipsis (`GitLog.module.css:306-318`).
- **H4:** as the notes.
- **H5:** the log reads with `summary_only: true` (`logStore.ts:950`).
  - Copy Commit Message runs `copy_commit_message` in the window, which reads `vcs_log_details` when `message_truncated` (`GitLog.tsx:1248-1251`).
- **H6, built per the webview's source:** the log is read again on a ref move only, with no "while shown" condition.
  - The webview keeps a shown tool window mounted while hidden (`toolwindows/store.ts:90`, `ToolWindows.tsx:1103`). Its log's `active` means the log tab, not visibility (`GitLog.tsx:231`). Its reload effects (`GitLog.tsx:244-264`, `:312-321`) never check visibility.
  - `LogSlot` gains `branch_scope` and `refs`. `refs` holds the refs signature, the repositories' heads and the tags.
  - `settle_git_log` runs first in `sync_log_panes`.
  - Branch choices from the filter bar and the Branches pane are remembered per repository (`logStore.ts:255-271`).
  - `GitLog` drops its `VcsChanged` listener and gains a `reads()` counter.
  - The log starts on `["HEAD"]`. The webview's first read uses `[]` (`logStore.ts:111`) before settling; both end on the same filter.
- **H6, regression fixed:** with the log on HEAD, Navigate to Selected Branch and Go to Hash could no longer find a commit on another branch.
  - `go_to_ref` now retries with no filters, then with `branches: [id]`, as the webview's `goToRef` does (`logStore.ts:1355-1366`).
  - This adds `GitLog::replace_filters` and `Workbench::replace_log_filters`.
  - The webview's seek step (`logStore.ts:1351-1354`) has no native counterpart.
- **H6, new test as you asked:** `the_branch_chosen_for_the_log_is_remembered_when_the_project_is_opened_again`.
  - It chooses `feature` through the Branch chip's chooser, then runs Close Project and opens the project again in the same test home. The chip reads `feature` and the log lists `feature work, two, one`.
  - Then it clears the chip with its close button and reopens again. The chip is empty and all 4 commits are listed.
- **H7, applied:** on opening, the Git window gives the keyboard to the Branches pane's field, or to the filter bar's field when the pane is hidden.
  - Settled by reading the source: the header tabs sit outside `[data-toolwindow-body]` (`ToolWindows.tsx:890-899`). Buttons are skipped, and the first remaining focusable element is the branch search (`ToolWindows.tsx:841-851`, `GitLog.tsx:333-339`, `:544-552`).
  - No webview test asserts this.
- **Test 2** now clears the Branch chip by clicking its close button, not by calling `clear`.

## 4. Measurements (item 4)

- **Timing:** `present_graph` over 3,000 commits took 263.1 ms in a debug build (`the_graph_of_three_thousand_commits_is_laid_out_in_one_pass`). That is above the 10 ms threshold.
  - The webview does not lay the graph out incrementally or off its main thread. It recomputes `presentLogGraph` over everything loaded, synchronously, in a `useMemo` (`GitLog.tsx:1355`, `logPresentation.ts:14-39`). So there was no webview approach to copy, and I restructured nothing.
  - Hypothesis: the release-build time is much lower. Check: `cargo test --release --lib the_graph_of_three_thousand -- --nocapture`. Not run, as you instructed.
- **Lane stability:** in the same test, 32 of the first page's 300 rows change their graph once the second page is read.
  - Hypothesis: the webview moves the same rows, since it runs the same layout over everything loaded. Check: run `presentLogGraph` on the same fixture in vitest.

## 5. Existing tests changed

No assertion is weakened.

| Test                                                                                                                      | Change                                                                                                                                                                                                             | Webview line that requires it                       |
| ------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------- |
| `vcs::log_view::tests::shows_a_repositorys_commits_with_their_graph`                                                      | Calls `reload` instead of the removed `backend_event`; expectations unchanged                                                                                                                                      | `GitLog.tsx:312-321`                                |
| `workbench::tests::the_git_tool_window_shows_the_projects_log`                                                            | Keyboard goes to the branch filter field                                                                                                                                                                           | `ToolWindows.tsx:841-851`, `GitLog.tsx:544-552`     |
| `workbench::tests::the_log_filters_by_text_and_by_the_branch_chosen_in_the_branches_pane`                                 | Opens on HEAD's 3 commits with filter `["HEAD"]`; the "cleared" wait is 3                                                                                                                                          | `GitLog.tsx:299-310`, `logStore.ts:208-213,240-252` |
| `workbench::log_actions::tests::cherry_pick_applies_a_commit_of_another_branch_and_a_conflicting_one_opens_the_conflicts` | `show_log` 4 → 2                                                                                                                                                                                                   | `logStore.ts:240-252`                               |
| `workbench::log_actions::tests::new_branch_new_tag_rebase_onto_and_checkout_at_a_commit`                                  | "the log follows" is `["two", "first"]`                                                                                                                                                                            | Same, plus `GitLog.tsx:312-321`                     |
| `workbench::log_actions::tests::the_commit_context_menu_offers_the_commit_actions_and_runs_the_chosen_one`                | Waits for the clipboard and asserts the whole `"Second\n\nWith a body"` (stronger than before)                                                                                                                     | `GitLog.tsx:1248-1251`                              |
| `workbench::log_actions::tests::the_branches_pane_actions_act_on_the_selected_branch`                                     | `show_log` 2 → 1. Every later assertion is unchanged and still exercises the selected-branch actions, Navigate included (through the `go_to_ref` fix). No assertion needed the other branch's commits in the list. | `logStore.ts:240-252`, `:1355-1366`                 |
| `workbench::tests::proofs4_vcs::the_log_date_filter_keeps_the_commits_of_the_chosen_period`                               | Waits for the log to open on HEAD, then clicks `log-filter-clear-branch`. Its wait for 3 commits and everything after it are unchanged.                                                                            | `logStore.ts:240-252`, `GitLog.tsx:906`             |

## 6. Shots

Taken with `$S/insp8/run.sh` on the 140-commit fixture. The fixture was rebuilt from the test's own stream (`dglog/fixture.py`: 140 commits, 60 on HEAD, HEAD at "stable 10") under a neutral `/private/tmp/openidea-inspect-*` folder, which was removed afterwards.

All are in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/insp8/shots/`:

- **`dglog1-01-glog-open-a-1.png` (dark) and `dglog1-02-glog-open-b-1.png` (light), window 1536 wide:** the log on opening. The chip reads "Branch: HEAD", HEAD's node is a ring, subjects run "stable 10…", and the caret is in the Branches pane's field.
- **`dglog2-01-cleared-dark-1.png`:** the Branch chip cleared by a click on its ×. The newest rows "topic 39 2", "topic 39 1", "topic 38 2"… have nodes on packed lanes, subjects and `origin/topic-NN` labels.
- **`dglog2-02-cleared-light-1.png`:** this was meant to be light but came out dark; the theme toggle had not taken effect by that mark. The cleared state in light is shown by the next shot.
- **`dglog2-03-narrow-light-1.png` and `dglog2-04-narrow-dark-1.png`, window 900 wide, chip cleared:** nodes and subjects are drawn, and the reference labels shrink to their icon.

The narrow shots show a remaining difference from the webview, measured visually and not fixed:

- In rows with a reference, the author and date columns shift right by about 20 px: the labels' 11 px minimum plus the gap.
- The date column is clipped at the details pane's edge.
- Cause: the webview puts the refs inside the subject cell and scrolls the list horizontally, while the native row keeps a 120 px subject minimum with the refs beside it.

## 7. Rows

Nothing is written to `parity.json`.

| Key                                                            | State                                      | What is now proven, and by which test                                                                                                                                                                                                                                                                   |
| -------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ipc:vcs_log`                                                  | Stays partial                              | The `summary_only` read and drawn rows: tests 1-3 and the copy-message test. Multi-root, seek and `stopAt` remain.                                                                                                                                                                                      |
| `unit:src/features/vcs/__tests__/logBranchMemory.test.ts`      | Stays partial                              | The app settles on HEAD (test 1), remembers a cleared chip (test 2), and the choice and the cleared chip survive reopening the project (`the_branch_chosen_for_the_log_is_remembered_when_the_project_is_opened_again`). The multi-root and deleted-branch cases are not yet driven through the window. |
| `boundid:toolwindow.git`                                       | Done, focus clause now matches the webview | `showing_the_git_window_gives_the_keyboard_to_the_branch_filter_field`                                                                                                                                                                                                                                  |
| `unit:src/features/vcs/__tests__/graph.test.ts`                | Stays partial                              | Packed lanes in the drawn log: tests 1-2                                                                                                                                                                                                                                                                |
| `unit:src/features/vcs/__tests__/logGraphCorrectness.test.tsx` | Stays partial                              | The node drawn inside its cell: tests 1-2. Keyboard navigation remains.                                                                                                                                                                                                                                 |
| `unit:src/features/vcs/__tests__/logUiFeatures.test.tsx`       | Stays partial                              | The subject keeps 120 px: test 2                                                                                                                                                                                                                                                                        |
| `unit:src/features/vcs/__tests__/logLoading.test.ts`           | Stays partial                              | The page read ahead: test 3                                                                                                                                                                                                                                                                             |
| `feature:vcs`                                                  | Stays partial                              | Add the six proofs42 tests and the copy-message test                                                                                                                                                                                                                                                    |

## 8. False or stale in the notes and brief

- **H6's "only while shown"** contradicts the webview's source (section 3).
- **The tests the notes expected to change:**
  - They predicted `collapse_and_expand_linear_branches_…` would hang; it passed unchanged.
  - The three tests the notes listed as a hypothesis (`…history_rewriting…`, `refuses_every_rewrite…`, `asks_before_rewriting…`) also passed unchanged.
  - The notes missed four affected tests: the context-menu copy, new-branch, Branches pane and date-filter tests.
  - They also missed the Navigate / Go to Hash regression.
- **The brief's premise** that the webview lays the graph out incrementally or off its main thread is false (section 4).
- **Disk-check command:** the brief and `int_rules.md` give different commands (`df -g /` against `df -k /System/Volumes/Data`). I logged both.
