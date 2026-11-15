# Integrator brief: the agents' setting pickers and the New Task page (reconciled drafts)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dagv/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

## Part 0 (first, small): 24 tests for unit rows of the built Settings pages, written without a compiler

Notes: `docs/gpui-port/notes/integration/proofs22_settings.md` (three `mod` lines, no accessor; per spec file the table of cases; the tests most likely to fail). Files (committed, undeclared): `app-gpui/src/workbench/tests/proofs22_settings_units.rs`, `app-gpui/src/settings_view/pages/languages_controls/tests/proofs22_language_controls.rs`, `app-gpui/src/settings_view/pages/font_picker/tests/proofs22_font_picker.rs`. Proposed rows: `docs/gpui-port/notes/integration/proofs22_settings_rows.json`.

- Declare one file at a time, compile, run that file's tests by filter, read every failure. A test wrong about the app is fixed against the spec case it follows (`src/features/settings/__tests__/*.test.tsx`, `src/features/settings/components/FontPicker.test.tsx`), never by weakening an assertion. A test right about a behaviour the app lacks: fix the app if the fix is small and local (the test is its proof), otherwise move the test, unchanged, into a `proofs22_<topic>_pending.rs` file beside it (undeclared).
- Toolchain answers come only through `SdkDetection` (`settings_view/pages/languages_sdk.rs`; it now also carries `formatters`). No test may reach the backend's detection.
- The writer's own doubts to settle by running: the two Maven search tests (does the dialog stay on Maven?), the UI fallback font test (a click on the disabled picker, Escape after typing), the proxy test (does Reset drop edits staged on the model?), the CMake round trip of an empty argument and a `C:\…` path.
- Report for this part: per spec file "N of M cases proven" with each case's test by full name, the state the row reaches, and every case not proven with what is missing.

## Part 0b (before Part A): three agent tests that never got their reply on a loaded machine

In one full run of the suite on 2026-11-15, while another process tree of the machine's owner was running browser tests (load averages about 30; not ours, never touch it), three tests failed the same way: `workbench::tests::proofs11_agents::archives_only_eligible_tasks_in_a_mixed_selection`, `workbench::tests::proofs11_agents::deletes_the_right_clicked_selection_after_one_confirmation_excluding_group_headers`, `workbench::tests::proofs13_agents_config::shows_allow_all_as_a_switch_and_sends_on_off_values`: "the reply did not happen within thirty seconds (ran out after 41 s; reached later: None)" and "agent: Answer 1 did not happen …". The reply never came, even with thirty more seconds: this is not slowness of the wait. Log: `$S/verify_d122_suite2.log` (search `FAILED`). The other 38 agent tests of that run passed.

1. The failure report of `wait_long` prints the Git state only, which says nothing here. Make the agent tests' waits report the agent side when they run out (the session's state as the store has it, whether the fake agent's process was started and is alive, its last lines of output or error), without printing any secret.
2. Find the cause by measurement: read how the fake agent is started and what can end a turn or a start without a reply (a start-up or handshake time limit in `crates/ide-agent`, a probe, a dropped event, an exit of the fake agent); loop the three tests beside two full suite runs of your own to load the machine (never start anything outside the temp folders), and read what the new report says when one fails. If it does not fail in a hundred loops, say so and leave the cause as a hypothesis with the check.
3. Fix the cause (in the test's fixture if the test is wrong about timing; in the app if a user on a slow machine would lose a reply the same way: then say what the webview does in that case, from its source).

## Part A: the slice

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

## Changed since the notes were written

The last recorded decision is D123. Since the reconciled notes were written, these files changed and anchors in them may have moved: `app-gpui/src/workbench/mod.rs` (new `mod` lines for proofs20, proofs21, Settings pages, a database test), `app-gpui/src/workbench/build_tools.rs`, `app-gpui/src/workbench/editor_extras.rs`, `app-gpui/src/settings_view/` (27 pages; `SettingsDialogView::page_view` exists), `app-gpui/src/inspect.rs` (steps `mods`, `no-toolchains`). Run the anchors script first and re-anchor from the quoted text.
