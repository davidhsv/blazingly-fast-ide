# Integrator brief: the agents' setting pickers and the New Task page (reconciled drafts)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dagv/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## The slice

Reconciled notes (self-contained): `docs/gpui-port/notes/integration/agents_views_reconciled.md`: 42 hunks in the order to apply them, the `mod` lines, the rows expected, the hypotheses to settle first, the tests most likely to fail. A script re-checks every anchor against the tree: `python3 -I $S/agv/anchors.py <worktree>` (it must print `0 failed` before you apply anything; where an anchor moved since the notes were written, re-anchor it yourself from the quoted text; `--out DIR` previews the result; do not use `--apply` blindly: apply hunk by hunk or review its whole output with `git diff` before compiling). Draft files (committed, undeclared): `app-gpui/src/agents/config_pickers_view.rs`, `app-gpui/src/agents/composer_view/pickers.rs`, `app-gpui/src/agents/new_task_view.rs`, `app-gpui/src/agents/new_task_view/tests.rs`, `app-gpui/src/workbench/agents_views.rs`, `app-gpui/src/workbench/tests/proofs14_agents_views.rs`; pending tests the notes move in: `app-gpui/src/agents/session_view/tests/proofs13_agents_session_pending.rs`, `app-gpui/src/workbench/tests/proofs13_agents_pending.rs`.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/agents/` pickers, composer, New Task page and their `*.test.tsx`). Settle the notes' five risks by test before building on them (whether creating a task sends the model picked on the New Task page; whether the backend remembers a session's offered commands for the project).

## Rules for this slice

- No test uses the network, the user's agent binaries, a login shell, the keychain or anything under the user's home: only the fake agents the existing tests install (`proofs11_agents`, `proofs13_agents_fixtures`). A built-in agent found on the machine's search path must not be started: read how the list of agents is built in a test and make sure a test never runs one.
- A secret (API key, token) is never drawn in clear, logged, copied or part of a `Debug` output.
- Popups open inside the window (the notes say the New Task popups open downwards and can fall below a short tool window: fix the placement with the shared popup anchor, do not shrink the test).
- After it passes, take screenshots in your own binary's window (temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, temp project; launcher `$S/insp8/run.sh`, steps `$S/insp/USAGE.md`): the Agents tool window's New Task page, the agent chooser open, the Model & effort popup open. Start no agent in the real binary (no send, no task created): with the temp home no agent is configured; if a built-in agent of the machine would be started by opening the page, do not open it and say so. Look at each shot: nothing cut, popups inside the window with their shadow.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, and for a `unit:` row "N of M cases"; every test left pending with what it waits for; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (`workbench/{refactor_actions,vcs_text_actions,diff_tab}.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `vcs_more/caret_conflicts.rs`, `vcs/{selection_history_view,external_log_view}.rs`, `platform/error_log_sink.rs`) and every undeclared `proofs*` test file this brief does not hand you.
