# Integrator brief: Deployment (the Remote Host tool window, Sync with Deployed, uploads and downloads), drafted without a compiler

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/ddepi/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing.

## Part 0: a secret in a debug output (do this first)

`app-gpui/src/features_b/deployment/store.rs` declares `SecretAnswer` and `SecretForm` with a derived `Debug` over the plain SSH password or passphrase. Read both types and every type that holds them. Give them a `Debug` that prints the secret's presence, never its text (copy how the proxy password of the Settings model and the other redacted types of this crate do it: grep `fmt::Debug for` with "redact" or `••`), and add a test that formats each type holding a known secret and asserts the secret's text is absent. Then grep the whole slice you are about to land for the same mistake: no type of `app-gpui/src/deployment/` or `workbench/deployment.rs` that can hold a secret may derive `Debug` over it, and no `log::`/`tracing::` call may take one.

## The slice

Notes (the `mod` lines, every hunk anchored on quoted text, the rows expected, what is left out, the hypotheses with their checks, the tests most likely to fail): `$S/drafts/deployment_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Writer's report: `docs/gpui-port/notes/reports/deployment_writer.md`. Draft files (committed, undeclared): `app-gpui/src/deployment/{mod,remote_host_view,sync_dialog,sync_preview,secret_dialog}.rs`, `app-gpui/src/workbench/deployment.rs`.

The notes are hypotheses; where a note and the webview source disagree, the webview source wins (`src/features/deployment/`, its `*.test.ts(x)`, its CSS module, `src/platform/deploymentSettings.ts`). Re-anchor every hunk from its quoted text and read the place before you edit it: `workbench/mod.rs` changed after the notes were written (the tool windows list and `mount_panels` among others). Apply one item at a time (settings and the server chooser; the Remote Host window; uploads and downloads; Sync with Deployed and its preview; the remaining actions), compile, run that item's tests by filter, fix, next.

Settle first, because every end-to-end test depends on it: the project settings accept the keys `deployment.servers` and `deployment.defaultServer` as the webview writes them (`src/platform/deploymentSettings.ts`), and a server of the local-folder kind reaches `ide_host::ops::deploy` without any SSH code running.

## Tests: nothing leaves the machine, nothing of the user is read

Every server in a test is the local-folder kind pointing into the test's temp folder. No test connects anywhere, starts an SSH client or server, or reads SSH configuration, keys, known hosts, an agent socket, the keychain, or anything under the user's home. Before you run a test that touches `host.ssh` (the trust-host and password paths of `workbench/deployment.rs`), read what that call reads on this machine; if it can read a file of the user's home or ask the keychain, the test must not reach it: prove the dialog's own behaviour on the view instead and report the path as untested. Open in Browser is asserted on the URL the test context recorded (`cx.opened_url()`): nothing is opened.

The tool windows list grows by one (`remoteHost`): check every existing test that counts or lists tool windows, the stripe's order against the webview's registration (`src/features/deployment/index.ts`: anchor, order, icon, whether it is shown by default), and the persisted layout of a home that has none.

After it passes, look at your own binary's window (launcher `$S/insp8/run.sh` only: it gives the binary a short `PATH` and a temp `HOME`; steps `docs/gpui-port/notes/inspections/INSPECT_SCRIPT.md`; a temp project whose `.openidea` settings name one local-folder server pointing at a second temp folder with a few files): the Remote Host tool window with its tree and toolbar; a row's context menu; the Sync dialog with one differing file selected and its preview. Do not open the Agents tool window, and configure no SFTP server. Look at each shot: nothing cut or overlapping, popups inside the window.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion.

## Report

Besides the format of `$S/int_rules.md`: Part 0's types and test; one table of every `parity.json` key the notes name (the eleven `action:deployment.*` rows, the `unit:` rows of `src/features/deployment/` and `src/platform/deploymentSettings.test.ts`, the `ipc:` rows `deploy_*` and `remote_*`, `point.registerToolWindow:remoteHost`, `feature:deployment`, any `settings:` row) with the state it reaches (done or partial), the proving tests by full name, for partial what is missing, for a `unit:` row "N of M cases" by your own count of the spec's cases and whether each case is proven on a model, a view or through a project window; for each action whether its `when` clause is read on and off by a test; every behaviour of the webview's Deployment you saw missing natively (the notes list the context-menu group, the separate comparison window, the editable local side, speed search, SFTP end to end); whether anything under `crates/` changed (say so in the report's opening lines: it makes me rerun the outer workspace's suite); the shots' paths and what you saw.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src`: `app-gpui/src/http_client/`, `workbench/http_client.rs`, `workbench/http_client/`, `platform/error_log_sink.rs`, `terminal/panel/groups.rs`, `terminal/view/clipboard.rs`, `workbench/terminal_groups.rs`, `editor/view/{column_gestures,right_margin,string_literals,doc_tags,caret_glide,token_style}.rs`, `editor/paste_history_view/preview.rs`, `theme/scheme_runtime.rs` (those of them still undeclared when you start), every `*_pending.rs`, and every undeclared `proofs*` test file this brief does not hand you. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
