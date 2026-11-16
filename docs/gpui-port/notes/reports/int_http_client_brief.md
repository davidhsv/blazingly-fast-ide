# Integrator brief: the HTTP Client (requests in `.http` files, the responses view, the Spring request generators), drafted without a compiler

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dhttpi/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## The slice

Notes (the `mod` lines, every hunk anchored on quoted text, the rows expected, what is left out, the hypotheses, the tests most likely to fail): `$S/drafts/http_client_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Writer's report: `docs/gpui-port/notes/reports/http_client_writer.md`. Draft files (committed, undeclared): `app-gpui/src/http_client/{mod,store,marks,request_toolbar,response_view,service_tabs}.rs`, `app-gpui/src/workbench/http_client.rs`, `app-gpui/src/workbench/http_client/tests.rs`.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/http-client/`, its `*.test.ts`, its CSS module, `src/workbench/endpointProviders.ts`). Re-anchor every hunk from its quoted text and read the place before you edit it: `workbench/mod.rs`, `workbench/context_menus.rs` and `workbench/tab_views.rs` changed after the notes were written. Apply one item at a time (the store and the responses view in the Services tool window; the request toolbar and the run marks; the actions; the Spring generators), compile, run that item's tests by filter, fix, next.

Settle before building on them:

- **No request leaves the loopback interface, and no proxy is used.** The writer's hypothesis is that the backend's proxy setting is process-wide. This suite runs every test in ONE process (`cargo test --lib`), so no test may set or depend on a process-wide proxy, and a test's request to `127.0.0.1` must reach the test's own listener whatever another test did. Read how `ide-http` builds its client and how `Host::new` applies the proxy (`crates/ide-host/src/lib.rs`), and prove with a test that a request to the test's listener arrives while the proxy setting of the test's home names a dead address, or change the test set-up so the question cannot arise. Do not change anything under `crates/` without saying so first in your report's opening lines: a change there makes me rerun the outer workspace's suite.
- The `.http` file's language id, the `Send` bound of the generator, the scratch path against the editor's path, the `use<>` return types.
- The run marks sit at the same x as plugin gutter icons: read how the plugin gutter and the run producers' marks share a line (`app-gpui/src/run_producers/`, the plugin gutter in `workbench/`), and place the marks so two on one line do not overlap; prove it with a test on a line that has both, or report that no line can have both.
- The token of the private environment file is in no `Debug` output, no log line and no notification (the draft asserts two of these: check the third by reading every `log::` and `tracing::` call the slice adds).

The Services tool window gets a tab per view (Services, HTTP Client): check every existing test that reads the Services tool window's header or content, and that the window with only the Services view looks as before.

After it passes, look at your own binary's window (launcher `$S/insp8/run.sh` only: it gives the binary a short `PATH` and a temp `HOME`; steps `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md`; a temp project with one `.http` file holding two requests to `http://127.0.0.1:9/…`, and a Java file with a Spring mapping): the `.http` editor with its toolbar and run marks; the environment chooser open; the HTTP Client tab of the Services tool window, empty. Send NO request in the real binary (press no run mark, run no action that sends), and do not open the Agents tool window. Look at each shot: nothing cut or overlapping, popups inside the window.

## Changed since the notes were written

The notes were written on the tree at about commit 20185bd1. Of the existing files the notes name, these changed since (measured: the notes' file names against `git diff --name-only 20185bd1 5714c388`): `app-gpui/src/lib.rs` (new `mod` lines and the colour scheme's `init` call), `app-gpui/src/workbench/mod.rs` (the diff tab's fields and register calls; the Terminal draws its own header; the waits of its tests print the language servers when they run out) and `app-gpui/src/editor/view.rs` (the right margin guides, the doc-comment tags, the gliding caret, token styles from the colour scheme, the middle-button column drag: the paint and prepaint of the gutter and the rows moved, so read the gutter's paint again before placing the run marks). If the Deployment slice was integrated before you start, `lib.rs`, `workbench/mod.rs`, `workbench/context_menus.rs` and the tool-window registrations changed again: read them as they are. Re-anchor every hunk from its quoted text and read the place before you edit it. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name (the seven `action:http.*` rows, the `unit:` rows of `src/features/http-client/` and `src/workbench/endpointProviders.test.ts`, the `ipc:` rows `http_requests`, `http_environments`, `http_execute`, `http_history`, `http_history_get`, `http_history_clear`, `spring_route_sources`, `plugin_endpoints`, the `point.` and `feature:` rows) with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, for a `unit:` row "N of M cases" by your own count of the spec's cases; for each action whether its `when` clause is read on and off by a test; every behaviour of the webview's HTTP Client you saw missing natively; whether anything under `crates/` changed; the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src`: `app-gpui/src/deployment/`, `workbench/deployment.rs`, `platform/error_log_sink.rs`, `terminal/panel/groups.rs`, `terminal/view/clipboard.rs`, `workbench/terminal_groups.rs`, `editor/view/{column_gestures,right_margin,string_literals,doc_tags,caret_glide,token_style}.rs`, `editor/paste_history_view/preview.rs`, `theme/scheme_runtime.rs` (those of them still undeclared when you start), every `*_pending.rs`, and every undeclared `proofs*` test file this brief does not hand you. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
