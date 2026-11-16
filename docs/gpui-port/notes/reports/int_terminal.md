# Report: Terminal split groups, closing, drops and paste, plus Part 0

All four items are in and every gate passed after my last change. Part 0's failure never reproduced, so it has no fix, only a better failure message. I found and fixed two real bugs in the drafts: a drag of the line between two groups stopped when the pointer left the panel, and files dropped right after typing were ignored. Nothing is committed.

## 1. Gates (final run, after every source and test change)

| Gate                                        | Result line                                                                                                | Load (before → after) |
| ------------------------------------------- | ---------------------------------------------------------------------------------------------------------- | --------------------- |
| Suite 1, alone                              | `test result: ok. 7645 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 190.53s`, rc=0 | 5.68 → 44.64          |
| Suite 2                                     | `test result: ok. 7645 passed; 0 failed; …; finished in 216.64s`, rc=0                                     | 38.16 → 18.29         |
| `cargo test --test web_view`                | `test result: ok. 1 passed; 0 failed`, rc=0                                                                |                       |
| `cargo clippy --all-targets -- -D warnings` | `Finished`, rc=0                                                                                           |                       |
| `cargo build`                               | `Finished`, no warning lines, rc=0                                                                         |                       |

During suite 2, the 24 tests I added, declared or changed ran 20 times by filter: all 24 passed 20 of 20, no failures. Logs are in `$S/dterm/gate2_*.log` and `$S/dterm/loop/`.

An earlier full gate round was also all green (7643 passed twice; 22 tests 20 of 20). Its clippy then failed on `clippy::type_complexity` in `proofs23_paste.rs`; I fixed that with a `Failures` type alias and added two tests, which is why the whole round was rerun.

## 2. Part 0: the unstable refactoring test

**Never reproduced.** The refactoring test (`apply_the_single_matching_action_or_offer_a_choice`), all measured on a binary with temporary probes:

- 360 single-test runs beside 2 whole suites: 0 failures; both suites passed.
- 3 parallel loops (310, 310 and 308 runs, 928 in all) beside 2 more suites, load up to about 90: 0 failures. I meant to run four loops but only three started.
- 14 whole suites in a row, where the original failure happened: this test passed 14 of 14.

So 0 failures in 1,288 single-test runs and 18 whole suites, plus the 4 gate suites since. There are no before/after numbers because there is no fix.

The probe traces of passing runs showed no late change after an edit lands: each run starts at the version the previous edit left, the answer comes back at the same version and selection, and the reformat step finds nothing to do. Hypothesis (not reproduced): the original failure needs another test running in the same process. Running whole suites with the trace below in place would confirm it.

What I did instead, as the brief asks, was make the failure explain itself. The wait's panic in `refactor_actions.rs` now prints the text, version, selections, hint, whether a list is open, the request counts, every run's trace (each run's version and selection at the question and at the answer, and the step it went on to), and the methods the stand-in server received. The trace is a `#[cfg(test)]` field on `RefactorActions`. The temporary probes are removed.

**Two other tests failed once each** in those 14 suites, on the probed binary from before the slice. Cause unknown for both:

- `workbench::refactor_actions::tests::confirms_first_delete_function_and_searches_nothing_when_cancelled`: "the dialog did not happen within thirty seconds" (run 3, `$S/dterm/sl_anyfail3.log`). It is in the same module.
- `workbench::popup_placement::tests::show_usages_opens_under_the_caret`: "the usages popup did not happen within thirty seconds" (run 7, `$S/dterm/sl_anyfail7.log`).

## 3. Per item: what landed, and what I changed beyond the notes

All hunks were applied from their quoted text. The webview lines each piece follows are as the notes give them; for headers and the tab menu I checked `TerminalView.tsx:209-215, 340-484` and `index.ts:64` myself.

**Groups and headers** (P1–P3, P5–P7; W1, W2, W4, and W3 applied by hand).

- I replaced the draft's divider drag in `render_groups` with window-level move and release listeners. The draft's `div` listener only fires while the pointer is over the panel, so a drag past its edge stopped following. Seen failing without the fix: `right group keeps 60 pixels: 859.5`.
- New test `terminal::panel::tests::proofs23_groups::split_down_stacks_a_group_with_its_own_header_under_a_line_dragged_up_and_down`.

**Closing** (T1–T3 and P4) landed as written.

**Drops** (V1–V3, V5).

- The draft's `.on_drop` never fired after typing at the prompt. Measured: the drag was active and consumed, but neither the terminal's mouse-up nor its drop handler ran. With a mouse press first, the drop arrived.
- Cause, read in GPUI: file-drop events don't change the window's last-input type, and `Hitbox::is_hovered` reports nothing under the pointer after keyboard input.
- Fix in `terminal/view/clipboard.rs`: the terminal remembers the dragged files only while the pointer is over it (`on_drag_move`), and a release checked with `is_hovered_at` drops them. Any press or release forgets them, so a later drag of something else never drops stale paths. Seen failing without the fix many times ("the paths are typed: not within thirty seconds").

**Paste** (V1 import, V4, V6) landed as written; I only added the `Failures` alias.

**Tests I added beyond the drafts:**

- The drag test now also presses 2 px either side of the line, over rows full of text, and checks the line moves and no text is selected. A control drag away from the line does select text, so the check can fail.
- New e2e `workbench::tests::proofs23_e2e_terminal_groups::the_terminal_header_s_more_options_and_hide_buttons_act_on_the_window`: the More chevron lists its rows and its Split Right splits; Options lists `Move To, Maximize, -, Hide`; Maximize and Restore Size work; Hide hides the Terminal.

**Superseded pending tests:** I removed the two tests (and the doc lines and imports only they used) from `workbench/tests/proofs18_e2e_toolwindows_pending.rs`, which stays undeclared.

**Existing tests that click `more:terminal` or `hide:terminal`:** none besides that pending file. Both selectors are now drawn by the Terminal's own header.

## 4. The writer's four hypotheses

1. **The divider's grab area lies over both terminals: true.** Shown by the presses 2 px either side of the line in the drag test.
2. **The drop handler fires for a simulated file drop: false after keyboard input, true after a mouse press.** Both measured; the release-based drop above fixes it.
3. **The close and paste keys reach the terminal: true on macOS** (`cmd_w_in_a_terminal_asks_the_window_to_close_its_tab`, the ⌘W e2e test, and the ⌘V paste tests). Hypothesis for Linux and Windows: run the same tests there.
4. **New Tab's key opens a tab while the terminal has the keyboard: true on macOS** (`closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`, ⌘T). Hypothesis for Ctrl+Shift+T elsewhere.

## 5. `parity.json` rows (not written to the file)

| Key                                        | State                   | Proving tests                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Missing                                                                                                                                                                                                                              |
| ------------------------------------------ | ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `action:terminal.splitRight`               | done                    | `workbench::tests::proofs23_e2e_terminal_groups::split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back`, `terminal::panel::tests::proofs23_groups::each_group_has_a_header_whose_tabs_and_new_tab_button_act_on_that_group`, `terminal::panel::tests::proofs23_groups::dragging_the_line_between_two_groups_resizes_them_down_to_sixty_pixels`, `workbench::terminal_actions::tests::the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`                                                                                                                                                                                      | —                                                                                                                                                                                                                                    |
| `action:terminal.splitDown`                | done                    | `terminal::panel::tests::proofs23_groups::split_down_stacks_a_group_with_its_own_header_under_a_line_dragged_up_and_down`, `workbench::terminal_actions::tests::the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`                                                                                                                                                                                                                                                                                                                                                                                                                                           | —                                                                                                                                                                                                                                    |
| `action:terminal.splitAndMoveDown`         | partial                 | `workbench::terminal_actions::tests::the_splits_lay_their_shells_side_by_side_or_stacked_and_unsplit_joins_them`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | no test of the per-group headers after this action (shared code with Split Down)                                                                                                                                                     |
| `action:terminal.unsplit`                  | done                    | `…split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              | —                                                                                                                                                                                                                                    |
| `action:terminal.closeSession`             | done                    | `workbench::tests::proofs23_e2e_terminal_groups::the_close_tab_button_and_cmd_w_ask_before_terminating_a_running_command`, `workbench::tests::proofs23_e2e_terminal_groups::close_other_tabs_asks_about_a_running_program_and_stops_at_cancel`, `…closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`, `terminal::panel::tests::proofs23_groups::cmd_w_in_a_terminal_asks_the_window_to_close_its_tab`, `workbench::terminal_actions::tests::close_session_asks_before_terminating_a_running_command_and_closes_an_idle_shell_at_once`, `workbench::terminal_actions::tests::proofs15_terminal::close_session_asks_before_ending_a_shell_whose_foreground_runs_a_command` | —                                                                                                                                                                                                                                    |
| `point.registerToolWindow:terminal`        | partial                 | `workbench::tests::proofs23_e2e_terminal_groups::the_terminal_header_s_more_options_and_hide_buttons_act_on_the_window`, `…each_group_has_a_header_whose_tabs_and_new_tab_button_act_on_that_group`, `…split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back`                                                                                                                                                                                                                                                                                                                                                                                         | header middle click (hide), double click (maximize), buttons shown only on hover                                                                                                                                                     |
| `feature:terminal`                         | partial                 | all of the above                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      | Rename Session…, tab drag and drop, find bar, pane context menu, shell list and SSH Session… in More, links, mouse reporting, plugin sessions, command titles                                                                        |
| `unit:src/features/terminal/drop.test.ts`  | partial, 4 of 5 cases   | `terminal::drop::tests`; the "under the cursor" case now also through the view by `terminal::panel::tests::proofs23_groups::pastes_only_into_the_terminal_under_the_cursor_without_executing`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | "ignores detached and hidden sessions" (embedded Run console) has no test                                                                                                                                                            |
| `unit:src/features/terminal/paste.test.ts` | partial, 20 of 24 cases | `terminal::paste::tests`, `terminal::encode::tests`, `terminal::view::tests::proofs23_paste::keeps_existing_copied_files_on_their_original_quoted_paths_during_paste`, `…stores_raw_local_clipboard_media_through_paste`, `…rejects_oversized_images_before_reading_or_uploading`, `…ignores_an_image_after_its_target_is_replaced_during_upload`                                                                                                                                                                                                                                                                                                                                     | The last test calls `media_stored` directly with a stale target. "Disposed during upload" is handled in code but no test drives it. The read-phase cases are only argued (the clipboard is read synchronously natively), not tested. |
| `e2e:e2e/toolwindows.spec.ts`              | partial, 9 of 12 flows  | the 7 already listed, plus `…split_right_gives_the_terminal_a_tab_group_with_its_own_header_unsplit_merges_it_back` and `…closing_the_last_tab_of_a_terminal_group_focuses_its_neighbour`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             | the 3 maximize-by-double-click flows                                                                                                                                                                                                 |

**Webview Terminal behaviour still missing natively** (seen in tests or shots):

- **SSH Session…** is not in the More menu, because `ssh.startSession` is not registered natively and the menu leaves it out (measured).
- **The shell list** ("New Predefined Session") is not in the More menu.
- **Rename Session…** is not in the tab menu.
- **Header buttons** are always visible natively; the webview shows them only on hover or focus.
- Tab drag and drop, the find bar, and the pane's context menu are not built.
- Hypothesis: the native terminal anchors a drag selection where the first move lands, not where the press was. One simulated move selected a single cell, `Some("0")`. A slow real drag in the window would settle it.

## 6. Stale or false in the notes and brief

- The "done 5/5" (drop) and "done 24/24" (paste) states rest on arguments, not tests.
- Writer's hypothesis 3 (the drop fires for a simulated drop) was false as written, and the drag draft broke outside the panel.
- The More menu's "SSH Session…" row never appears natively.
- Hunk W3 says to "Run `cargo fmt`", which contradicts rule 1. I used `$S/fmt.sh`.
- The remaining pending maximize test expects `tool-window-title:terminal` on a workbench header that is no longer drawn for the Terminal.
- Not measured on a real OS: a real file drop, a real divider drag in the window, and the keys on Linux and Windows.

## 7. Shots

All from my own binary via `$S/insp8/run.sh`, with a temp project, temp `HOME` and `ZDOTDIR`, the project trusted. I typed only `echo ok`. The real shell's prompt showed the machine's user and host, so I deleted the four full shots and kept only crops with no prompt text in them:

- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dterm/shots/terminal-unsplit-header.png`: one header row: "Terminal", the selected tab "Local" with ✕, +, chevron, and Options and Hide at the right, as the webview has it.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dterm/shots/terminal-split-right-headers.png`: two headers side by side: the title with "Local" in the left one; "Local (2)" with Options and Hide in the right one.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dterm/shots/terminal-split-down-top-headers.png` and `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dterm/shots/terminal-split-down-lower-header.png`: after Split Down on "Local (2)", the right group is stacked with "Local (3)" and its own header below.
- `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/dterm/shots/terminal-tab-menu.png`: the tab menu of "Local" while split: Close Tab, Close Other Tabs (greyed, one tab), a separator, Split Right, Split Down, Unsplit.

## 8. Files

From `git diff --name-only` (I left out `docs/gpui-port/notes/backlog.md`, which is not mine):

- `app-gpui/src/terminal/panel.rs`
- `app-gpui/src/terminal/panel/groups.rs`
- `app-gpui/src/terminal/panel/tests/proofs23_groups.rs`
- `app-gpui/src/terminal/view.rs`
- `app-gpui/src/terminal/view/clipboard.rs`
- `app-gpui/src/terminal/view/tests/proofs23_paste.rs`
- `app-gpui/src/workbench/mod.rs`
- `app-gpui/src/workbench/refactor_actions.rs`
- `app-gpui/src/workbench/terminal_actions.rs`
- `app-gpui/src/workbench/tests/proofs18_e2e_toolwindows_pending.rs`
- `app-gpui/src/workbench/tests/proofs23_e2e_terminal_groups.rs`

`app-gpui/src/workbench/terminal_groups.rs` is now declared but unchanged. I created no untracked files in the tree. Scratch only: `$S/dterm/` (scripts, logs, `proj/`, `shots/`) and `$S/insp8/dterm1.*` and `$S/insp8/dterm2.*`.
