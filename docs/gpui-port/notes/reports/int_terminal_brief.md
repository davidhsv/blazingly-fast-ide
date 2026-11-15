# Integrator brief: the Terminal's split groups, the tab close question, file drops and the paste plan (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dterm/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## The slice

Notes (hunks in the order to apply them, `mod` lines, the rows expected, the webview lines each behaviour follows, what is left out, the tests most likely to fail): `docs/gpui-port/notes/integration/terminal_splits_integration.md`. Writer's report: `docs/gpui-port/notes/reports/terminal_splits_writer.md`. Draft files (committed, undeclared): `app-gpui/src/terminal/panel/groups.rs`, `app-gpui/src/terminal/view/clipboard.rs`, `app-gpui/src/workbench/terminal_groups.rs`, tests `app-gpui/src/terminal/panel/tests/proofs23_groups.rs`, `app-gpui/src/terminal/view/tests/proofs23_paste.rs`, `app-gpui/src/workbench/tests/proofs23_e2e_terminal_groups.rs`.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/terminal/`, its `*.test.ts(x)`, its CSS module, `e2e/toolwindows.spec.ts`). The hunks were anchored on the tree at commit c0e0e25a; re-anchor from the quoted text where a file moved. Apply one item at a time (groups and headers; closing; drops; paste), compile, run that item's tests by filter, fix, next.

Settle the writer's four hypotheses by test before building on them: the divider's grab area lies over both neighbouring terminals (a press there drags, it does not select text); the drop handler fires for a file drop simulated in a test; the close and paste keys reach the terminal; New Tab's key opens a tab while the terminal has the keyboard.

- The two tests of `app-gpui/src/workbench/tests/proofs18_e2e_toolwindows_pending.rs` named after the same two e2e cases are superseded: remove those two tests from that file (it stays undeclared), and say so.
- Every terminal in a test uses the test build's plain shell (read how the existing terminal tests start one): never a login shell, the user's shell files, history or home. A "still running" command is the existing `sleep` setup inside the temp folder.
- The workbench stops drawing its own header for the Terminal window: check every existing test that clicks `more:terminal` or `hide:terminal`, and that the Terminal's header in an unsplit window looks as the webview's (title, tabs, +, chevron, Options, Hide).
- A copied picture is stored through the backend and its path pasted: the test must keep the stored file inside the temp home; a failure shows a notification and pastes nothing.
- After it passes, look at your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, temp project; launcher `$S/insp8/run.sh`, steps `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md`): the Terminal unsplit; split right; split down; a tab's context menu open. The terminal's prompt of the real shell must not show the machine's user or host in a shot you keep: with the temp `HOME` and `ZDOTDIR` the prompt is the shell's default; if it shows the user or host name, crop it or do not keep the shot, and say so. Type nothing into the real shell but `echo ok`.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name (`action:terminal.splitRight`, `terminal.splitDown`, `terminal.splitAndMoveDown`, `terminal.unsplit`, `terminal.closeSession`, `point.registerToolWindow:terminal`, `feature:terminal`, `unit:src/features/terminal/drop.test.ts`, `unit:src/features/terminal/paste.test.ts`, `e2e:e2e/toolwindows.spec.ts`) with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, for a `unit:` row "N of M cases", for the e2e row "N of 12 flows"; every behaviour of the webview's Terminal you saw missing natively; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (`workbench/{refactor_actions,vcs_text_actions,diff_tab}.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `vcs_more/caret_conflicts.rs`, `vcs/{selection_history_view,external_log_view}.rs`, `platform/error_log_sink.rs`, new files under `app-gpui/src/editor/` and `app-gpui/src/editor_commands/` that are not declared) and every undeclared `proofs*` test file this brief does not hand you. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
