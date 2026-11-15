# Integrator brief: the Terminal's split groups, the tab close question, file drops and the paste plan (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dterm/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## Part 0: an unstable refactoring test (do this first)

`workbench::refactor_actions::tests::apply_the_single_matching_action_or_offer_a_choice` failed once in three runs of the whole suite at commit 87438b22 (load average about 100): `refactor.extractField did not happen within thirty seconds` (`app-gpui/src/workbench/refactor_actions.rs`, the loop over `MENU_CASES`; log `$S/verify_ddfg_suite2.log`). It passed ten of ten alone and in the two other runs. The test runs eight refactorings one after another on one document: each selects, runs the action, and waits until the text holds the action's edit.

Hypothesis (read in the code, not reproduced): the answer of the code-action query is dropped as stale when the document's version or the selection changed between the question and the answer (`RefactorMenu::query_answered` takes the version and the selection), and something of the previous action still changes one of them after its text landed, which the test's wait does not see. Find the cause by measurement, not by reading alone: make the wait's failure say what the window holds (the document's version and selection at the question and at the answer, what the step machine returned, the requests the stand-in server got), loop the test beside two whole-suite runs until it fails (the earlier one-off failures of this suite needed about 100 to 200 runs under load), and read the report. Then fix the cause: if a late change of the previous action is what drops the answer, the test waits for that action to have ended (a state the app exposes, not a sleep); if the app drops an answer it should keep, or leaves the user with nothing shown after a dropped answer where the webview shows something (`src/editor/lsp/actions.ts`, `src/editor/refactor/`), fix the app with a unit test that fails without the fix. Report the runs of the loop before and after, and say plainly if the failure never reproduced.

## The slice

Notes (hunks in the order to apply them, `mod` lines, the rows expected, the webview lines each behaviour follows, what is left out, the tests most likely to fail): `docs/gpui-port/notes/integration/terminal_splits_integration.md`. Writer's report: `docs/gpui-port/notes/reports/terminal_splits_writer.md`. Draft files (committed, undeclared): `app-gpui/src/terminal/panel/groups.rs`, `app-gpui/src/terminal/view/clipboard.rs`, `app-gpui/src/workbench/terminal_groups.rs`, tests `app-gpui/src/terminal/panel/tests/proofs23_groups.rs`, `app-gpui/src/terminal/view/tests/proofs23_paste.rs`, `app-gpui/src/workbench/tests/proofs23_e2e_terminal_groups.rs`.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/terminal/`, its `*.test.ts(x)`, its CSS module, `e2e/toolwindows.spec.ts`). The hunks were anchored on the tree at commit c0e0e25a; re-anchor from the quoted text where a file moved. Apply one item at a time (groups and headers; closing; drops; paste), compile, run that item's tests by filter, fix, next.

Settle the writer's four hypotheses by test before building on them: the divider's grab area lies over both neighbouring terminals (a press there drags, it does not select text); the drop handler fires for a file drop simulated in a test; the close and paste keys reach the terminal; New Tab's key opens a tab while the terminal has the keyboard.

- The two tests of `app-gpui/src/workbench/tests/proofs18_e2e_toolwindows_pending.rs` named after the same two e2e cases are superseded: remove those two tests from that file (it stays undeclared), and say so.
- Every terminal in a test uses the test build's plain shell (read how the existing terminal tests start one): never a login shell, the user's shell files, history or home. A "still running" command is the existing `sleep` setup inside the temp folder.
- The workbench stops drawing its own header for the Terminal window: check every existing test that clicks `more:terminal` or `hide:terminal`, and that the Terminal's header in an unsplit window looks as the webview's (title, tabs, +, chevron, Options, Hide).
- A copied picture is stored through the backend and its path pasted: the test must keep the stored file inside the temp home; a failure shows a notification and pastes nothing.
- After it passes, look at your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, temp project; launcher `$S/insp8/run.sh`, steps `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md`): the Terminal unsplit; split right; split down; a tab's context menu open. The terminal's prompt of the real shell must not show the machine's user or host in a shot you keep: with the temp `HOME` and `ZDOTDIR` the prompt is the shell's default; if it shows the user or host name, crop it or do not keep the shot, and say so. Type nothing into the real shell but `echo ok`.

## Changed since the notes were written

The notes were anchored on the tree at commit c0e0e25a. Of the existing files their hunks anchor on, only `app-gpui/src/workbench/mod.rs` changed since (measured with `git diff --name-only c0e0e25a`): the Agents New Task page, the refactorings, the Git text actions and the diff tab added fields, `mod` lines and register calls there. Re-anchor every hunk from its quoted text and read the place before you edit it. Read the scratch copy of the notes, `$S/drafts/terminal_splits_integration.md`: the copy under `docs/` was reformatted by the commit hook.

Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name (`action:terminal.splitRight`, `terminal.splitDown`, `terminal.splitAndMoveDown`, `terminal.unsplit`, `terminal.closeSession`, `point.registerToolWindow:terminal`, `feature:terminal`, `unit:src/features/terminal/drop.test.ts`, `unit:src/features/terminal/paste.test.ts`, `e2e:e2e/toolwindows.spec.ts`) with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, for a `unit:` row "N of M cases", for the e2e row "N of 12 flows"; every behaviour of the webview's Terminal you saw missing natively; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src`: `app-gpui/src/deployment/`, `workbench/deployment.rs`, `app-gpui/src/http_client/`, `workbench/http_client.rs`, `workbench/http_client/`, `platform/error_log_sink.rs`, `editor/view/{column_gestures,right_margin,string_literals,doc_tags,caret_glide,token_style}.rs`, `editor/paste_history_view/preview.rs`, `theme/scheme_runtime.rs`, the undeclared `proofs23_*` editor test files, every `*_pending.rs` this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.

The real binary is launched only through `$S/insp8/run.sh`, which gives it a short `PATH` and a temp `HOME`. Do not open the Agents tool window or the Services window in it.
