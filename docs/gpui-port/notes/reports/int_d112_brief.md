# Integrator brief: two cut texts in the Database window, then the `proofs18` end-to-end flow tests (42 tests written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d112/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish Part 0, then Part A. Stop early only if a part is blocked, and say what blocks it. Work one file at a time (declare, compile, run that module's tests by filter, fix, next).

## Part 0: two texts cut in the Database tool window (seen in the real window, shot `docs/gpui-port/notes/inspections/2026-11-15-c/t8-05-query-plan-1.png`)

1. The Query Plan's Params value runs off the pane's edge with no ellipsis ("table: users; USIN"). The webview ends it with an ellipsis (`src/features/database/Database.module.css`, the plan's cells). Make the cell end with an ellipsis inside the pane, with a test at a narrow width.
2. The tool window's "Filter objects" field is clipped by the window's right edge at the docked width. Read how the webview's toolbar lays the field out (`src/features/database/`, the tool window's toolbar and its CSS) and do the same: the field shrinks, it is not cut. Test at the docked width.

Take one screenshot of the Database window with a plan shown afterwards (the script of the last one is `$S/d111/insp/t8.script`, launcher `$S/d111/insp/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`) and look at it.

## Part A: `proofs18` (one native test per `test(...)` of an e2e spec)

Notes: `docs/gpui-port/notes/integration/proofs18_e2e.md` (the `mod` lines, the one accessor hunk `SettingsDialogView::page_view`, the tests most likely to fail, and per spec every case with its test). Writer's reports: `docs/gpui-port/notes/reports/` (the `proofs18` files), split of the specs: `docs/gpui-port/notes/sweep/e2e_part{A,B}.md`.

Files handed to you (committed, undeclared): every `proofs18_e2e_*.rs` under `app-gpui/src` whose name does not end in `_pending.rs`, EXCEPT `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs`: its parent `workbench/diff_tab.rs` is an undeclared draft of another slice; leave both alone.

- The notes are hypotheses. Declare a file, compile, read every failure. A test that fails because the TEST is wrong (a helper misused, a wrong assumption about a fixture or a selector) is fixed in the test, never by weakening an assertion: the e2e spec it cites (`e2e/<name>.spec.ts`) is the specification, and each native test must drive the whole flow of its `test(...)`, in the same order, asserting what the spec asserts.
- A test that fails because the APP does not do what the webview does is a finding. Read the webview source. If the fix is small and local, build it with the test as its proof. If not, move the failing test, unchanged, into the `proofs18_e2e_<topic>_pending.rs` file beside it (those files stay undeclared; they are the specification of later slices). No `#[ignore]`.
- The `_pending.rs` files stay undeclared. Do not add the accessors they assume.
- No real `gh`, JVM, Maven, Gradle, Docker, network, login shell or anything under the user's home: read each fixture before running it. The terminal test must use the test build's shell seam (read how the existing terminal tests start a shell); if it would start the user's login shell, leave it pending and say so.
- A test that needs more than a few seconds of wall clock is reported with its time.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, with nothing else running beside the first; during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings. Start no test run with `&` inside a script (it would run with SIGINT ignored).

## Report

Besides the format of `$S/int_rules.md`:

- One table per e2e spec: the `parity.json` key (`e2e:e2e/<name>.spec.ts`), "N of M `test(...)` cases proven", the native test of each case by full name, and for each case not proven the behaviour missing. A spec row is done only when every case has one passing native test that drives the whole flow.
- Every other `parity.json` key (action, context, ipc, point) that one of these tests proves through the registry, a key or a click and that the current `docs/gpui-port/parity.json` does not record as done: key, state, test.
- Every app fix, with the webview source it follows.
- Every test left in a `_pending` file with the behaviour it waits for.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,agents_views,diff_tab}.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/agents/{config_pickers_view,new_task_view}.rs`, `app-gpui/src/agents/composer_view/pickers.rs`, `app-gpui/src/agents/new_task_view/`, `app-gpui/src/vcs_diff/{wrap,tab_view}.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/platform/error_log_sink.rs`, and every undeclared `proofs*` test file that this brief does not hand you.
