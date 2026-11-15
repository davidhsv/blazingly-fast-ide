# Integrator brief: two unstable tests, zoom for the editor and icons, the `proofs16` tests, then Run and Debug actions

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d108/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: do the parts in order and finish them. Stop early only if a part is blocked, and say what blocks it. Work one file at a time. Send one final report only, after every gate has finished.

## Part A: two tests that failed once each under load

1. `workbench::app_actions::tests::updates_the_running_projects_banner_after_a_backend_trust_transition` panicked once at "the banner is gone" (it reads the drawn bounds right after `run_until_parked`; hypothesis: before the next frame is drawn). Read the test and what it waits for; make it wait for the state it asserts (the banner drawn or gone) with the existing wait helpers. Run it fifty times by filter while a full suite runs beside it, and report the count.
2. `workbench::tests::proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft` once did not see its dialog within thirty seconds in a suite run that took 207 s. Read what the dialog waits on (a real timer? a backend thread?) and say what you find; fix the test or the app if there is a real wait on the wall clock; run it fifty times by filter beside a full suite run and report the count.

## Part B: zoom scales the editor, the terminal and icons

Measured in the real window at 150 % (shots `$S/d107/insp/shots/zoom-z100-1.png`, `zoom-z150-1.png`: look at them): interface text and the theme's lengths scale; the editor's text and line numbers, every icon, and sizes written as literal pixels do not. The webview zooms the whole document (`src/features/settings/appearanceRuntime.ts:58-67`), so everything scales together.

- Make the code font size and line height the editor, the diff and merge viewers and the terminal draw with follow the zoom (one place: where the editor's font settings become metrics, `code_font_of` / `Editor::metrics`, `viewer_look`, `terminal_font`), and make icons follow it (find how icons get their size: if one helper draws them, scale there).
- Then count what is left: literal `px(` sizes by directory, and say which shared helpers (`ui/`, the dialog frame, row heights) would cover most of them. Do not sweep 3,000 call sites in this slice; scale the shared helpers that take a literal size if there are a few.
- Test: with zoom at 150 % the editor's painted font size and an icon's painted size are 1.5 times their size at 100 % (through the window, with the existing zoom test as the pattern). Take the two screenshots again in your own binary's window (inspection script: `$S/insp/USAGE.md`; launcher to copy: `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; never open the Services tool window in a shot) and look at them.

## Part C: `proofs16` (13 tests written without a compiler, plus 3 pending)

Notes: `docs/gpui-port/notes/integration/proofs16_leftovers.md` (the `mod` lines, the call sites for the `ipc:` rows, the 61-case table of `plugins.test.tsx`); writer's report: `docs/gpui-port/notes/reports/draft_proofs16_leftovers.md`. Files to declare: `app-gpui/src/workbench/plugins_window/tests/proofs16_plugins.rs`, `app-gpui/src/workbench/plugin_runtime/tests/proofs16_runtime.rs`, `app-gpui/src/plugin_models/plugins_page/tests/proofs16_page.rs`, `app-gpui/src/workbench/context_menus/tests/proofs16_menus.rs`. The two `_pending` files stay undeclared unless you build what they wait for; one is small: the Run stripe button's running-count badge (webview: find the badge of the `run` tool window under `src/features/run/`), with the accessor `Workbench::stripe_badge` its test needs.

A test that fails because the TEST is wrong is fixed in the test, without weakening what it checks; one that fails because the APP differs from the webview is fixed in the app when small and local, otherwise left in a `_pending` file. No `#[ignore]`, no real sleeps. Plugin tests use the checked-in fixture plugins copied into the temp home; nothing from the user's plugin folder, no network.

## Part D: Run and Debug actions

Notes: `docs/gpui-port/notes/integration/run_debug_actions_integration.md`; writer's report: `docs/gpui-port/notes/reports/draft_run_debug_actions.md` (what is built, what is not, the hypotheses to check first, the tests likely to fail). Draft files (committed, undeclared): `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/workbench/{debug_extras,run_context}.rs` (two of them were reformatted by rustfmt after the notes were written: layout only). The notes are days old: re-anchor every hunk on the current code; where a note and the webview source disagree, the webview source wins (`src/features/run/index.ts`, `src/features/debugger/actions.ts`).

It registers `run.runContext`, `run.debugContext`, `run.modifyContext`, `run.chooseDebugConfig`, `debug.evaluate`, `debug.smartStepInto`. Also do what the writer left: after Run with Current File selected, Current File is selected again (the notes describe the fix); and wire the Run group of the editor's, the Project view's and the tab's context menus to these actions as the webview's menus do (`grep -rn "run.runContext\|run.debugContext\|run.modifyContext" src/` for the menu groups), with a test that opens each menu and runs the row. The Evaluate dialog uses the shared dialog frame and the shared tooltip.

No real compiler, debugger, JVM or network in a test: the fake debug adapter and the plain echo configurations the existing run tests use.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every test you added, declared or changed twenty times by filter (the two of Part A fifty times beside a suite run); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name, "N of M cases" for spec rows and, for partial, what is missing. For `plugins.test.tsx` give the final counts (full, in part, page only, model only or missing, no counterpart) from the notes' table after your changes. List every test left in a `_pending` file with the behaviour it waits for, every app fix with the webview `file:line` it follows, and for zoom the screenshots' paths and what you saw.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,database_extras}.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you.
