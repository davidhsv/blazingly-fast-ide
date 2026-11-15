# Integrator brief: fifteen tests of the build tools and the shell, and a test that failed once under load

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/d122/`.

Format only files you edited, taken from `git diff --name-only` plus the test files this brief hands you by name. Never build a file list from `git status`.

## Part A: fifteen tests of the build tools and the shell written without a compiler

Notes: `docs/gpui-port/notes/integration/proofs21.md` (three `mod` lines, no accessor; the table of rows; one four-line hunk for an app defect). Files: `app-gpui/src/workbench/build_tools/tests/{proofs21_execute_popup,proofs21_build_windows}.rs`, `app-gpui/src/workbench/tests/proofs21_shell.rs`. Declare them, make them pass without weakening an assertion (the spec files they follow are `src/features/build-tools/{ExecutePopup,actions,linkage}.test.tsx`). One test fails by design until the app is fixed: a Gradle build picked in the file dialog after the window changed projects must be linked nowhere (the webview drops the answer; the notes give the hunk; read the webview source first). If a press inside the Execute popup takes the keyboard from its command line so that Tab no longer completes, that is an app defect to fix in the popup. The build tools' fixtures are the only launchers allowed (the test build refuses any other `mvn`, `gradle`, `java`). Report, per row of the notes' table, the state reached and the proving tests by full name.

## Part B: a test that failed once under load

`workbench::editor_extras::tests::next_occurrence_steps_through_the_results_of_the_tool_window` failed once in a suite run that had a test loop beside it (`app-gpui/src/workbench/editor_extras.rs`, "the caret in b at 0:13 did not happen within thirty seconds … reached later: None"); it passed 20 of 20 alone and 15 of 15 beside a suite. The last integrator's hypothesis: under load the highlighted usages arrive before the key press, which then steps to a usage instead of the next occurrence. Find the cause by measurement (log the state the key press meets; run the test in a loop beside a full suite AND a second test loop until it fails or two hundred runs pass), then fix the wait in the test, or the app if the app is wrong (the webview's rule: `src/editor/` next occurrence against highlighted usages). Say what you measured and what stays a hypothesis.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script.

## Report

Besides the format of `$S/int_rules.md`: the table of the notes' rows with the state each reaches and the proving tests by full name; every app fix with the webview source it follows; for Part 0b what was measured, the cause found or what stays a hypothesis, and the fix.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (Settings pages `settings_view/pages/languages_*.rs`, `workbench/{refactor_actions,vcs_text_actions,agents_views,diff_tab}.rs`, `agents/{config_pickers_view,new_task_view}.rs`, `agents/composer_view/pickers.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `vcs_more/caret_conflicts.rs`, `vcs/{selection_history_view,external_log_view}.rs`, `platform/error_log_sink.rs`) and every undeclared `proofs*` test file this brief does not hand you.
