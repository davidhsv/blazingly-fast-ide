# Integrator brief: four unstable tests, then GitHub part 2 and the Keymap Reference dialog

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d109/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: do the parts in order and finish them. Stop early only if a part is blocked, and say what blocks it. Work one file at a time. Send one final report only, after every gate has finished.

## Part A: find the cause of four unstable tests (do not paper over them)

The previous integration's report has the details: `docs/gpui-port/notes/reports/int_d108.md` (Gates, Part A2). A loop script for running one test many times beside a suite is in `$S/d108/` (`loop.sh`).

1. `workbench::plugin_java_debug::tests::stop_ends_the_process_and_the_session` failed in two suite runs that had a second test loop beside them: "the process ends did not happen within thirty seconds", reached later at 73.97 s and 74.63 s. The same figure twice suggests a fixed time-out on the path (a disconnect time-out of the debug driver, a kill escalation, a socket read time-out), not load alone. Read the stop path it exercises (`workbench/plugin_java_debug.rs`, `debugger/driver.rs`, `ide-dap`'s session stop: read only under `crates/`) and find where about 74 s (or 60 + something, or 45 + 30) can come from. If it is a real defect of the app (Stop taking over a minute when something does not answer), say so and fix it in `app-gpui` if the fix is there; if it is only reachable in the test, fix the test's fixture. Reproduce first: the test alone in a loop while a full suite runs beside it.
2. `workbench::tests::proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft` failed 1 run in 50 beside a suite: the dialog never came ("reached later: None"), so a longer wait does not help. The paths that can end silently are the two early returns of `ReplyActions::regenerate` (an action already under way, or `resolve` returning `None`) and a click that never lands. Instrument the test's failure message (the reply-actions busy flag, the task's status, the reply's regenerate permission, whether the clicked element's bounds were drawn at the click), reproduce it (loop beside a suite until it fails, at least 200 runs), and fix what it shows: in the app if the app can drop a click on a drawn button, in the test if the test clicks before the state it needs.
3. `workbench::context_menus::tests::move_to_in_a_tool_windows_options_moves_it` failed once in an earlier run (`$S/d107/run1/g_suite1.txt` has the message). Read it and say what it was.

4. `editor::intelligence::tests::proofs9_cancel_task::cancelling_a_language_servers_work_in_background_tasks_asks_the_server_to_stop_it` failed in a suite run with NOTHING beside it (the main session's verification, `$S/suite_d108_1.log`: "the backend did not deliver within thirty seconds" at `src/editor/intelligence.rs:1761`), and once before under load. Its three `settle` calls share one message, so the log does not say which wait ran out: first make every `settle` helper that panics with that sentence take a name for what it waits for and print it (there are a few copies: `editor/intelligence.rs`, `workbench/editor_extras.rs`, `workbench/editor_language.rs`; give them the form of `wait_long`), then reproduce (loop the test, alone and beside a suite, at least 200 runs) and find which wait it is and why the thing never arrives: the fake server's progress report not sent, the task not listed, the cancel click not landing, or the cancel request not reaching the server.

What tests 2 and 4 share is that something never happens (waiting longer does not help). Look for a common cause in how tests drive the app: a click simulated on bounds read from an older frame, a backend answer delivered while no one is listening, the test backend's two worker threads all blocked. Say what you find even if it is "no common cause".

Report for each: reproduced or not (runs, failures), the cause with `file:line`, the fix, and the count after the fix (100 runs beside a suite).

## Part B: GitHub part 2

Notes: `docs/gpui-port/notes/integration/github_part2_integration.md`; writer's report: `docs/gpui-port/notes/reports/draft_github_part2.md` (what is built, what is not, the hypotheses to check first, the tests likely to fail). Draft files (committed, undeclared): `app-gpui/src/github/create_view.rs`, `app-gpui/src/workbench/github_part2.rs` (meant as a child module of `workbench/github.rs`; if the `#[path]` form is awkward, make it an ordinary child module file `workbench/github/part2.rs`). Re-anchor every hunk on the current code; where a note and the webview source disagree, the webview source wins (`src/features/github/`).

It registers `github.createPullRequest` (the New Pull Request tab), `github.pullRequest.toggleViewed`, `github.pullRequest.openInWorktree`. The waiting test file `app-gpui/src/workbench/github/tests/proofs14_github_window_pending.rs` has two tests: the tab one must pass and is then declared (move it into `proofs14_github_window.rs`); the registration one still needs four actions that are not built: leave it, unchanged, in the pending file. No `#[ignore]`.

The fake `gh` in the temp folder answers everything; the real `gh` never runs; a destructive call is asserted on the fake's log of arguments. Take a screenshot of the New Pull Request tab in your own binary's window if the app can show it without a GitHub login (it probably cannot: say so) — do not sign in to anything.

## Part C: the Keymap Reference dialog

Notes: `docs/gpui-port/notes/integration/keymap_reference_light_edit_tabs_integration.md`, ONLY its Keymap Reference part; writer's report: `docs/gpui-port/notes/reports/draft_keymap_reference_light_edit_tabs.md`. Draft files to declare: `app-gpui/src/app_views/keymap_reference_dialog.rs`, `app-gpui/src/workbench/keymap_reference.rs`. Do NOT apply the light-edit tabs part and do not declare `app-gpui/src/workbench/light_edit_tabs.rs` or `proofs13_windows_pending.rs`: that is a slice of its own.

It registers `help.keymapReference`. The waiting test `app-gpui/src/workbench/tests/proofs13_shell_pending.rs` must pass and is then declared (move it into `proofs13_shell.rs`). Make the dialog sit where the webview puts it and close when the window is deactivated, if the webview does (`src/features/app/KeymapReference.tsx`: read its placement and its close conditions). Take a screenshot of it in your own binary's window (inspection script: `$S/insp/USAGE.md`; `action⇥help.keymapReference`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; never open the Services tool window) and look at it.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, with nothing else running beside it; every test you added, declared or changed twenty times by filter; the three tests of Part A as that part says; `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name, "N of M cases" for spec rows and, for partial, what is missing. List every test left in a `_pending` file with the behaviour it waits for, and every app fix with the webview `file:line` it follows.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,database_extras,light_edit_tabs}.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you.
