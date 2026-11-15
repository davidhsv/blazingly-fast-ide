# Integrator brief: the `proofs14` tests, then Annotate

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d106/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish Part A, then Part B. Stop early only if a part is blocked, and say what blocks it. Work one file at a time (declare, compile, run that module's tests by filter, fix, next).

## Part A: `proofs14` (20 tests written without a compiler, plus 3 pending)

Notes: `docs/gpui-port/notes/integration/proofs14_github_plugins.md` (per spec file every case with its test or the reason; the 5 `mod` lines; no hunks); writer's report: `docs/gpui-port/notes/reports/draft_proofs14_github_plugins.md`. Files to declare: `app-gpui/src/workbench/github/tests/proofs14_github_window.rs`, `app-gpui/src/github/list_panel/tests/proofs14_ui.rs`, `app-gpui/src/workbench/plugin_runtime/tests/proofs14_plugins.rs`, `app-gpui/src/plugin_models/plugins_page/tests/proofs14_page.rs`, `app-gpui/src/terminal/view/tests/proofs14_ime.rs`. The two `_pending` files stay undeclared unless you build what they wait for.

- A test that fails because the TEST is wrong is fixed in the test, without weakening what it checks. A test that fails because the APP differs from the webview: read the webview source it cites; fix the app when the fix is small and local, otherwise move the test unchanged into a `_pending` file and leave it undeclared. No `#[ignore]`.
- One test waits about 0.6 s of real time after a plugin trap: replace the wait with a state it can wait for (or a seam), no real sleeps.
- Small behaviour to build here (its test is in `proofs14_plugins_pending.rs`): the plugin update notification's Update All button runs `plugins.updateAll`, which nothing registers; register it and have it call the existing `PluginsPage::update_all` (webview: find `plugins.updateAll` under `src/features/plugins/`).
- The IME test may expose a real defect (a printable key delivered both as a key press and through the input handler, so the text arrives twice in the terminal): if it fails for that reason, fix the terminal view, not the test.
- No real `gh`, no network, no login or interactive shell, nothing from the user's plugin folder.

## Part B: Annotate

Notes: `docs/gpui-port/notes/integration/annotate_integration.md`; report of the writer: `docs/gpui-port/notes/reports/draft_annotate.md` (what is built, what is not, which tests are likely to fail first). Draft files (committed, undeclared): `app-gpui/src/editor/view/annotations.rs` (a child module of `editor::view`), `app-gpui/src/workbench/annotate.rs`. The notes were written days ago; the tree has moved (the editor's font resolver is now `code_font_of`, popups have a shared frame, tooltips note presses): read the current code before applying each hunk and re-anchor it; where a note and the webview source disagree, the webview source wins (`src/features/vcs/`).

It registers `vcs.annotate` (IPC `vcs_blame`): the annotation column in the editor's gutter (date and author per line, age colours, hover tooltip, click shows the commit, the column's menu, re-blame after an edit and after the head moves). Check first the three hypotheses its report names (two `EventEmitter` impls on `Editor`; `publish_context` on tab change; the hover child not swallowing the editor's mouse events). After it passes, take a screenshot of the column in your own binary's window (inspection script: `$S/insp/USAGE.md`; launcher to copy: `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; a temp Git project with two commits by two authors made with `git -c user.name=… -c user.email=…@example.invalid`) and look at it: the column must not overlap line numbers, breakpoints or the fold column, and its text is in the interface font at the webview's size.

Not part of this brief: Annotate inside the diff and merge viewers, inline blame.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name, "N of M cases" for spec rows and, for partial, what is missing. List every test left in a `_pending` file with the behaviour it waits for, and every app fix with the webview `file:line` it follows.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{project_view_options,zoom_actions,build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/project_view/{pane,tree_clicks}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you.
