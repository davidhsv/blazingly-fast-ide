# Integrator brief: tests that exist and never run, and native code no test drives (tests only; no new feature)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`. Your own scratch subfolder: `$S/dtests/`.

Format only files you edited, taken from `git diff --name-only` plus the files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff. Leave no polling loop behind.

## Part 1: thirteen test files no module declares

A census found source files under `app-gpui/src` that no `mod` declaration reaches, so their tests never compile or run (its script parsed text, not the compiler: confirm each by finding no `mod <name>` for it). Besides the queued drafts of other agents (listed under "Do not touch"), thirteen are test files written in earlier slices and never declared:

`app-gpui/src/editor/view/editing_tests/proofs23_middle_column.rs`, `proofs_editor_clipboard_history.rs`, `proofs_editor_multi_caret.rs`; `app-gpui/src/editor/view/tests/proofs_editor_highlight_cmake_queries.rs`, `proofs_editor_highlight_interpolation_templates.rs`, `proofs_editor_highlight_shell.rs`; `app-gpui/src/workbench/app_actions/tests/proofs6_app_actions.rs`; `app-gpui/src/workbench/plugin_runtime/tests/proofs10_locale.rs`, `proofs10_unsubscribe.rs`; `app-gpui/src/workbench/plugins_window/tests/proofs10_lifecycle.rs`; `app-gpui/src/workbench/tests/proofs6_attach.rs`, `proofs_merge_annotate.rs`, `proofs_quit.rs`.

For each file: read it and `git log --oneline -- <file>` (why it was written and whether the same tests were later written elsewhere under other names: grep each test's name and what it asserts); then one of: (a) declare it, make it compile against the code at HEAD by changing the test (never by weakening what it asserts), and run it; a test that fails shows either a defect of the app (then: find the cause; fix it if it is contained and a few lines, with the test as the proof; otherwise leave the test in a file named `*_pending.rs` undeclared and report the defect whole, with the failure quoted) or an expectation the merged webview no longer has (then the webview's `file:line` decides: rewrite to it, or drop the test and say why); (b) it duplicates tests that run: say which, and list the file for the manager to delete (you do not delete). One file at a time, each landing alone.

## Part 2: native code that no test drives

`$S/dcensus/code_no_test.md` lists, by native file, inventory rows whose native code exists and that no test reaches through the app (22 rows: backend calls, toolbar items of the Services window, three backend events, three menu groups), each with the native `file:line`, the webview's source and what a test would have to drive and assert. For each: read both sides, then write the test that drives it through the app (a window, an action through the registry, a view's event) and asserts the effect; where the native code turns out not to do what the webview does, do not bend the test: report the difference (and fix it only if it is a few lines and contained). Its notes on nine status items built and never put in the status bar, run types nothing registers and Docker operations shown and not carried out are features, not tests: list them, do not build them.

## Hard rules

- Tests start no program of the machine beyond what today's tests of the same module start (read them: stand-in scripts in the test's temp folder, the fake debug adapter under `python3`, the stand-in language server); no Docker, no network, no agent (`spawn_guard` stays in force), no toolchain, nothing under the user's home; deletes never reach the system Trash; a test that would attach to a process attaches only to one the test started.
- `proofs_quit.rs` and anything else that quits or closes the app's windows runs only inside the test application; nothing sends a quit to a process you did not start.
- No assertion is weakened to make an old test pass. No new dependencies, no `unsafe`, no stubs, no `#[ignore]`.

## Rows

For each test that now runs, say which inventory row it proves (key as in `docs/gpui-port/parity.json` or, for a row with no record, the key's form of its section) and whether the row is then done or partial with what is missing. Do not write to `parity.json`.

## If the slice is larger than one pass

Part 1 first, file by file, then Part 2 by native file. When about two thirds of your budget is used, finish the item in hand, run the gates on what has landed, and report what is done and what is not started.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): HEAD, what landed since the census (read at 21685f123), and the other agents' undeclared drafts then in the tree.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you declared, added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: Part 1, one section per file (declared and passing; changed how; what failed and why; or duplicate of which tests); defects of the app found, each with the failure quoted; Part 2, one line per row (the test, what it asserts, the row's state); files for the manager to delete.

## Do not touch

The queued drafts of other agents: `app-gpui/src/settings_view/pages/tools_agents.rs`, `tools_agents/`, `workbench/settings_agents.rs`, `settings_view/pages/agents_list.rs`, `agents_list/`, `agents/agent_list.rs`, `agents/agent_list/`, `workbench/agent_list_backend.rs`, `workbench/settings_agents_list.rs`, `settings_view/pages/remote_folder_browser.rs`, `remote_folder_browser/`, `build_deployment/remote_folder.rs`, every `proofs36_*`, `proofs37_*`, `proofs38_*` and `proofs40_*` file, `workbench/debug_guards.rs`, every `*_pending.rs`, `platform/error_log_sink.rs`. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
