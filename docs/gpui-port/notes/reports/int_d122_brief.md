# Integrator brief: fifteen tests of the build tools and the shell, and a test that failed once under load

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`. Your own scratch subfolder: `$S/d122/`.

Format only files you edited, taken from `git diff --name-only` plus the test files this brief hands you by name. Never build a file list from `git status`.

## Part A: fifteen tests of the build tools and the shell written without a compiler

Notes: `docs/gpui-port/notes/integration/proofs21.md` (three `mod` lines, no accessor; the table of rows; one four-line hunk for an app defect). Files: `app-gpui/src/workbench/build_tools/tests/{proofs21_execute_popup,proofs21_build_windows}.rs`, `app-gpui/src/workbench/tests/proofs21_shell.rs`. Declare them, make them pass without weakening an assertion (the spec files they follow are `src/features/build-tools/{ExecutePopup,actions,linkage}.test.tsx`). One test fails by design until the app is fixed: a Gradle build picked in the file dialog after the window changed projects must be linked nowhere (the webview drops the answer; the notes give the hunk; read the webview source first). If a press inside the Execute popup takes the keyboard from its command line so that Tab no longer completes, that is an app defect to fix in the popup. The build tools' fixtures are the only launchers allowed (the test build refuses any other `mvn`, `gradle`, `java`). Report, per row of the notes' table, the state reached and the proving tests by full name.

## Part B: a test that failed once under load

`workbench::editor_extras::tests::next_occurrence_steps_through_the_results_of_the_tool_window` failed once in a suite run that had a test loop beside it (`app-gpui/src/workbench/editor_extras.rs`, "the caret in b at 0:13 did not happen within thirty seconds … reached later: None"); it passed 20 of 20 alone and 15 of 15 beside a suite. The last integrator's hypothesis: under load the highlighted usages arrive before the key press, which then steps to a usage instead of the next occurrence. Find the cause by measurement (log the state the key press meets; run the test in a loop beside a full suite AND a second test loop until it fails or two hundred runs pass), then fix the wait in the test, or the app if the app is wrong (the webview's rule: `src/editor/` next occurrence against highlighted usages). Say what you measured and what stays a hypothesis.

## Part C: the Plugin Formatters page reads a process-wide list in tests

`app-gpui/src/settings_view/pages/languages_sdk.rs` answers `LanguageAsk::PluginFormatters` with `ide_host::ops::lsp::plugin_formatters()`, which reads the plugin host of the whole process. In a test build that list depends on which other tests have plugins loaded at that moment, and the last integrator loosened `workbench::tests::proofs19_settings_languages::plugin_formatters_format_on_save_applies` because of it (it now fixes only the first and last line), without having seen it fail. A loosened assertion is not acceptable.

1. Put the formatters behind the same seam as the toolchains (`SdkDetection`, same file): with answers handed over, those; in a test build without them, none, at once; otherwise the backend. Extend `a_test_build_without_a_detection_of_its_own_finds_nothing` to this fifth question.
2. Restore the strict assertion of that test (`["— Plugin Formatters —", "[ ] Format on save"]`).
3. Add one test in `proofs19_settings_languages.rs` that hands two formatters over through the seam (one covering two languages), opens the page in a project window, asserts every line of the page, picks a formatter for one language with a click, applies, and asserts the stored value and that Reset discards a second pick. Follow `src/features/settings/languages/PluginFormattersPage.tsx` for what the page shows and stores.
4. Report whether `parity.json` `settings:languages.pluginFormatters` can be done after it, and what is still missing if not.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script.

## Report

Besides the format of `$S/int_rules.md`: the table of the notes' rows with the state each reaches and the proving tests by full name; every app fix with the webview source it follows; for Part B what was measured, the cause found or what stays a hypothesis, and the fix.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` (`workbench/{refactor_actions,vcs_text_actions,agents_views,diff_tab}.rs`, `agents/{config_pickers_view,new_task_view}.rs`, `agents/composer_view/pickers.rs`, `vcs_diff/{wrap,tab_view}.rs`, `vcs_diff/view/moved.rs`, `vcs_more/caret_conflicts.rs`, `vcs/{selection_history_view,external_log_view}.rs`, `platform/error_log_sink.rs`) and every undeclared `proofs*` test file this brief does not hand you.

## Changed since the notes were written

The language Settings pages are now compiled and live. The toolchain drop-down of the Gradle and Maven pages was replaced by the shared one: `settings_view/pages/build_sdk_field.rs` no longer exists, the field is `SdkFieldView` in `settings_view/pages/languages_sdk.rs` and the test seam is `SdkDetection` there. Decision numbers: the last recorded decision is D122; do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
