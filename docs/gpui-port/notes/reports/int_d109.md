I finished Parts A, B and C, and every gate passed.

- **Part A:** I found a measured cause for each of the four tests and fixed three of them (the fourth was already fixed). Each fixed test then passed 100 of 100 runs beside a suite.
- **Part B:** All three actions are built. The tab test passes after one app fix, and it now closes the tab from the tab's menu. The registration test is unchanged in the pending file.
- **Part C:** The dialog now opens near the top of the window and closes when the window loses focus, as the webview does. I checked it in a screenshot of my own window.

## 1. Gates (measured, from `app-gpui/`, outputs in `$S/d109/`)

| Gate                                                                                | Result                                                                                                                      |
| ----------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| Suite 1, nothing beside it (`final_suite1.txt`)                                     | ok: 7240 passed, 0 failed, 0 ignored, 159.99 s                                                                              |
| Suite 2, nothing beside it (`final_suite2.txt`)                                     | ok: 7240 passed, 0 failed, 0 ignored, 160.34 s                                                                              |
| Every test I added, declared or changed: 12 filters, 198 tests, 20 runs (`twenty/`) | 20 of 20 runs: 198 passed, 0 failed                                                                                         |
| `cargo test --test web_view`                                                        | ok, 1 passed                                                                                                                |
| `cargo clippy --all-targets -- -D warnings`                                         | exit 0                                                                                                                      |
| `cargo build`                                                                       | 0 warnings                                                                                                                  |
| Part A tests after the fixes, beside back-to-back suites (`r8/`)                    | A1 100/100 (with SIGINT ignored, the condition that failed), A2 100/100, A4 100/100; both suites alongside: ok, 7240 passed |

Failures I saw along the way, all before the fixes:

- `r1` suite: the java stop test and the A4 test failed.
- `r3` suite: the java stop test failed.
- `r6` suite: `opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab` failed.

The causes are below. `git status --short src/ src-tauri/ docs/ e2e/ crates/` prints nothing. No `#[ignore]`, no new dependency, no new `unsafe`, and no wait was made longer. I formatted the 34 files in `$S/d109/fmt_list.txt` with `fmt.sh`: `git diff --name-only` plus `create_view.rs` and `github_part2.rs`.

## 2. Parts

### A1: `plugin_java_debug::tests::stop_ends_the_process_and_the_session`

- **Reproduced:** it failed 4 of 4 times in a loop beside a suite, each time reached later at about 72 s.
- **Cause (measured):** the test process had SIGINT ignored.
  - A script that runs a command with `&` gives that command SIGINT ignored. The d108 loops and suites, and my first runs, were started that way.
  - Every process the app starts inherits that.
  - Stop only sends SIGINT (`crates/ide-run/src/process.rs:694-741` and `:996-1008`).
  - A shell cannot trap a signal it was started with ignored. So the fake JVM ignored Stop and exited on its own minute-long time-out (1200 × `sleep 0.05`, about 72–74 s). That is the "74 s" figure.
  - The test alone with `trap '' INT` reproduces it every time: no signal logged, exit code 4 at 72.7 s, run marked "Stopping, stopped=true".
- **This is a real defect, but not in `app-gpui`:** if the IDE itself is started with SIGINT ignored, the first Stop never stops a run. The fix belongs in `ide-run`'s spawn (reset SIGINT to the default in the child). The brief does not name `crates/`, so I did not touch it.
  - Hypothesis: a real JVM also keeps an inherited ignored SIGINT. Check: start `java` from `zsh -c 'trap "" INT; java …' &` and send it SIGINT.
- **Fix (test fixture):** the fake JVM is now Python. It installs its own SIGINT handler, which works even when SIGINT was ignored at start. With SIGINT ignored it now exits 130 within about 30 ms, 5 of 5 times.
- **File:** `app-gpui/src/workbench/plugin_java_debug.rs` (`JVM`, `jvm()`).

### A2: `workbench::tests::proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft`

- **Reproduced:** 2 failures in 121 runs, then 2 in 60, beside suites.
- **Instrumentation:** the failure message now gives the button and time bounds, the busy flag, the task status, the regenerate permission and the events the session view emitted (new test-only accessor `Workbench::agent_reply_busy`).
- **Cause (measured):** before the click the button was at x=1640 and the reply's time was not drawn. After the click the time was drawn (94 px wide) and the button was at 1740. Nothing was emitted, while busy=false, status=Ready and regenerate=true.
  - The reply's end time is written to the store after the task is already Ready and the answer is shown. It arrived between the mouse down and the mouse up, the button moved 100 px, and the click was dropped.
- **Fix (test):** `wait_for_answer` also waits for the turn's end time (new `turn_ended`). I made this change in both copies, `proofs13_agents_fixtures.rs` and `proofs11_agents.rs`, because the user-message buttons move the same way.
  - Hypothesis: a real click lasting about 100 ms during that redraw could be dropped too. I left that alone.

### A3: `context_menus::tests::move_to_in_a_tool_windows_options_moves_it`

This was not an instability. The d107 failure (written at 23:38) was the test's old exact assertion `["Move To", "Maximize", "-", "Hide"]` against a menu that had just gained the Project view's options. Commit `bc143b90` (23:54) rewrote the assertion to `ends_with`. It passed in every suite since.

### A4: `editor::intelligence::tests::proofs9_cancel_task::cancelling_…`

- **Named waits:** every `settle` in `intelligence.rs`, `editor_extras.rs` and `editor_language.rs` (plus their child test files) now takes a name and panics in the form of `wait_long`, with `#[track_caller]`. That is 202 call sites. In `workbench/mod.rs` I only added `#[track_caller]` to its `settle`, so its panic points at the caller line; I did not name its many call sites.
- **Reproduced:** 1 in 10, 2 in 59 and 2 in 23 runs beside suites. The wait that ran out is "the cancel button is drawn".
- **Cause (measured):** the project's own "Indexing" task was still listed above the work, so the test built the selector `task-cancel:1`. Then Indexing ended, the work's row moved to index 0, and `task-cancel:1` never existed again ("reached later: None").
- **Fix (test):** wait until the work is the only task listed, then use row 0.

**Common cause of A2 and A4:** both read a layout or an index before an update the app had still to make, so a click or selector pointed at something that then moved. The backend's worker threads were not blocked and no answer was lost. A1 is unrelated.

### Part B: GitHub part 2

- **Files:**
  - `github/mod.rs`, `branches.rs` (`host_call` made `pub(super)`).
  - `workbench/github.rs`: every hunk of the notes; `part2` declared with `#[path]`; 11 test helpers made `pub(super)`.
  - Declared: `github/create_view.rs`, `workbench/github_part2.rs`.
- **Tab test:** `opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab` is moved into `workbench/github/tests/proofs14_github_window.rs`.
  - It failed first because the tab strip overflowed the narrow tool window: the right-hand tabs and the + button lay outside it and could not be clicked.
  - App fix (`github/window_view.rs`): the strip now scrolls sideways and brings the active tab into view, as the webview's compact tabs do (`src/ui/Tabs.module.css:17-24`, `src/ui/Tabs.tsx:501-502`, `PullRequestsToolWindow.tsx:246`).
  - The New Pull Request tab is wider than the test's strip, so the test now closes it from its own menu ("Close Tab"). It also asserts the scrolling.
- **Pending file:** `proofs14_github_window_pending.rs` still holds the registration test, unchanged and undeclared. It waits for `github.pullRequest.showTimeline`, `github.pullRequest.timeline.refresh`, `github.selectOpenedPullRequest` and `github.pullRequest.submitReview`, which need the timeline editor and the Submit Review popup.
- **Tests:** the four end-to-end tests in `workbench::github::part2::tests` pass, along with the `create_view` tests.
- **Left out:** everything the writer's report lists as not built (the changes panel, reviewers/assignees/labels, search in the branch pickers, the Details popup, Settings → GitHub).
- **No screenshot:** Create Pull Request… opens the tab only once the repository is connected (`github_part2.rs`, `create_pull_request`), and that needs a signed-in `gh` (read from the code, not run). I did not open the Pull Requests window, so the real `gh` never ran.

### Part C: Keymap Reference

- **Files:**
  - `app_views/mod.rs`, `workbench/mod.rs` (module and registration).
  - Declared: `keymap_reference_dialog.rs`, `keymap_reference.rs`.
  - `proofs13_shell_pending.rs`: its only test moved into `proofs13_shell.rs`, and the file was removed.
- **Placement:** the webview opens it with `position="top"` (`KeymapReference.tsx:98`): a tenth of the window height from the top, centred across (`Dialog.module.css:12-15`).
  - The draft centred it and kept it open on deactivation on purpose; I changed both.
  - The dialog slot can now place a dialog near the top (`branches.rs`, `place_vcs_dialog_at_top`).
- **Closing:** the webview closes it on window deactivation (`Dialog.tsx:237`, `:266`). The dialog now emits Closed when the window is deactivated.
- **Test:** `workbench::keymap_reference::tests::sits_near_the_top_and_closes_when_the_window_is_left`.
- **Screenshot:** taken of my own window only, with a temp home, `HOME`, `ZDOTDIR` and project, stopped by its PID. The dialog sits near the top, centred, and lists 319 shortcuts; the filter "search" leaves 3; Escape closes it. Files: `$S/d109/insp/shots/keymap-*.png`.
- **Left out:** closing on a click outside the dialog, and refreshing while it is open after a keymap change (it shows a snapshot).

## 3. Rows that changed state

I made no edits under `docs/`.

| Key                                                       | State           | Proving tests                                                                                                                                                                                                                                                                                                                    |
| --------------------------------------------------------- | --------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:github.createPullRequest`                         | done            | `workbench::github::part2::tests::create_pull_request_creates_the_current_branchs_pull_request_and_opens_its_tab`, `…::create_pull_request_before_the_window_connects_opens_the_tab_once_connected`, `workbench::github::tests::proofs14_github_window::opens_the_new_pull_request_tab_once_and_closes_back_to_the_previous_tab` |
| `action:github.pullRequest.toggleViewed`                  | done            | `workbench::github::part2::tests::toggle_viewed_marks_the_selected_file_viewed_then_not_viewed`                                                                                                                                                                                                                                  |
| `action:github.pullRequest.openInWorktree`                | done            | `workbench::github::part2::tests::open_in_worktree_adds_the_pull_requests_head_as_a_worktree_and_opens_it`                                                                                                                                                                                                                       |
| `action:help.keymapReference`                             | done            | `workbench::keymap_reference::tests::{renders_the_overlay, filters_by_title_id_or_shortcut, opens_in_a_window_that_edits_a_loose_file, sits_near_the_top_and_closes_when_the_window_is_left}`, `workbench::tests::proofs13_shell::renders_the_overlay`                                                                           |
| `point.registerOverlay:app.keymapReference`               | partial         | same tests. Missing: closing on a click outside, refreshing while open                                                                                                                                                                                                                                                           |
| `unit:src/features/github/toolwindow/toolwindow.test.tsx` | partial, 6 of 8 | the registration case still lacks 4 actions (it previously lacked 7)                                                                                                                                                                                                                                                             |

The notes also name IPC commands these actions call (`vcs_branches`, `vcs_branch_info`, `gh_api`, `vcs_log`, `vcs_remote`, `gh_pr_prepare_diff`, `vcs_add_worktree`). I did not check their rows. Hypothesis: they are reached by the part2 tests above. Check: compare each row in `parity.json` with the calls in `github_part2.rs`.

## 4. Hypotheses (not measured)

- A real JVM keeps an inherited ignored SIGINT. Check: start `java` from `zsh -c 'trap "" INT; java …' &` and send it SIGINT.
- A user's click during the reply-time redraw can be dropped. Check: in the real window, click Regenerate just as a turn ends.
- The webview would also hide the New Pull Request tab's close button at this tool-window width. Check: the webview at a tool-window width of about 280 px.

## 5. False or stale in notes and briefs

- **int_d108 report:** its hypothesis that load pushes Stop past 30 s is false. The cause is SIGINT ignored in the test process, plus the fake JVM's own one-minute time-out. The brief's "fixed time-out on the path" was that fake JVM's loop, not the debug driver.
- **Keymap integration notes:** "centred, stays open on deactivation, on purpose" contradicts the webview (`KeymapReference.tsx:98`, `Dialog.tsx:266`). Both are fixed.
- **GitHub part 2 notes:** they said the tab test "should pass"; it failed on the overflowing tab strip. Their suggestion to mark the registration test `#[ignore]` was not followed: it stays undeclared in the pending file.
- **The main session's suite log** (`$S/suite_d108_1.log`): its A4 failure was the task-row index (A4), not the backend. It does not depend on SIGINT; A4 failed with SIGINT both ignored and at default.
