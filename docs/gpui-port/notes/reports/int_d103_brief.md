# Integrator brief: the `proofs11` test batches (75 tests written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md` (how these tests are packaged and what a test must prove), `$S/view_rules.md`. Your own scratch subfolder: `$S/d103/`.

Format only files you edited, taken from `git diff --name-only` plus the `proofs11_*.rs` files this brief hands you. Never build a file list from `git status`.

## The two batches

1. Views: notes `docs/gpui-port/notes/integration/proofs11_views.md`, writer's report `docs/gpui-port/notes/reports/draft_proofs11_views.md`. 55 tests in 8 files: `app-gpui/src/workbench/tests/proofs11_agents.rs`, `app-gpui/src/agents/{task_list_view,session_view,markdown_view}/tests/proofs11_*.rs`, `app-gpui/src/database/{grid_view,results_view}/tests/proofs11_*.rs`, `app-gpui/src/github/list_panel/tests/proofs11_list.rs`, `app-gpui/src/workbench/github/tests/proofs11_github.rs`. Needs only the 8 `mod` lines.
2. Windows: notes `docs/gpui-port/notes/integration/proofs11_windows.md`, report `docs/gpui-port/notes/reports/draft_proofs11_windows.md`. 20 tests in 10 files: `app-gpui/src/workbench/tests/proofs11_vcs_windows.rs`, `app-gpui/src/workbench/{shelf,vcs_operations,services_actions,debug,editor_core,plugin_contributions,project_view_actions}/tests/proofs11_*.rs`, `app-gpui/src/debugger/state/tests/proofs11_execution_state.rs`, `app-gpui/src/platform/keymap/tests/proofs11_host_modifier.rs`. Needs the `mod` lines and three hunks: H1 (`workbench/services_window.rs`: two test-only accessors), H2 (`debugger/driver.rs`: a test answer to the hot-swap compile), H3 (`platform/keymap.rs`: `has_primary_modifier(modifiers, mac)`; replace the private copies of that check in views with it).

## How to integrate

- The notes are hypotheses. Declare a file, compile, read every failure. A test that fails because the TEST is wrong (a helper misused, a wrong assumption about the fixture) is fixed in the test. A test that fails because the APP does not do what the webview does is a finding: read the webview source the test cites, and if the test is right, fix the app when the fix is small and local, otherwise leave the test out (do not declare it, or split it into its own undeclared file) and report the missing behaviour. Never weaken an assertion to make a test pass, never add `#[ignore]`.
- Known app differences the tests demand a fix for: after "Add to .git/info/exclude", and after an ignore write that fails, the project tree is reloaded (`Workbench::add_to_ignore` calls the reload on both outcomes; webview: `src/features/vcs/` ignore code and `ignore.test.ts`). The writers also suspect: retracting an agent turn that has later messages asks to confirm; showing the Pull Requests tool window gives it the keyboard; an edited result that ran in another schema asks for that schema's table details. Check each against the webview source before changing the app.
- Tests that need real time (the agents stop-timeout test needs about 15 s of wall clock): keep them only if they do not slow the suite by more than a few seconds each; otherwise make the timeout injectable in the test build and say so.
- No Docker daemon, no real `gh`, no JVM, no network may be reached: the Docker tests feed a made-up snapshot, the GitHub tests rewrite the fake `gh` in the temp folder. Confirm that by reading each fixture before running it.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every `proofs11` module twenty times by filter (`cargo test --lib -- proofs11 > file`, twenty times; report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the test or tests that prove it by full name, and for partial what is missing. List every test left undeclared, with the reason and the missing behaviour.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{annotate,project_view_options,zoom_actions,build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/editor/view/annotations.rs`, `app-gpui/src/project_view/{pane,tree_clicks}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, any draft the previous integration left undeclared, and every undeclared `proofs*` test file that is not a `proofs11_*.rs` of this brief.
