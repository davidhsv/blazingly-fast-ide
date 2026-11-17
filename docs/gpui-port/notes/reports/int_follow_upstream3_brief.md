# Integrator brief: native calls of the backend that no longer pass or do what the webview's do since the merge (small fixes; one concerns secrets)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`. Your own scratch subfolder: `$S/dfol3/`.

Format only files you edited, taken from `git diff --name-only`. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Use `git diff -U3` when you read a diff.

## What this is

The native app calls the backend's functions directly where the webview goes through an adapter in `src-tauri/src/commands/`; it must pass what the adapter passes and treat the answer as the webview does. A reader compared the adapters and backend functions that changed in the merge of upstream v0.1.46 with the native calls (`docs/gpui-port/notes/RETRIAGE_IPC.md`: one section per command, with `file:line` on both sides; its list "native differences" at the top). Its findings are hypotheses: read the adapter at HEAD, the backend function, the webview's caller and the native call before changing anything. This slice takes the differences that are a few lines each; the features that list names (the agents' model catalog, permission forms, per-session MCP servers, the plugin details sections, plugin events, the sbt tool window, Stash Selected Files, FTP fields, closing tabs with the window) are not yours.

## Part 0: Export Settings and Import Settings and the MCP servers' secret values (first)

The adapters of `settings_export` and `settings_import` call `ide_host::ops::agent::move_user_mcp_values(&state.host)` before they read or after they write (`src-tauri/src/commands/app.rs:289`, `:307`: read both and the function, `crates/ide-host/src/ops/agent.rs:421`): MCP servers' environment values and headers written into the user settings are moved to the secret store, so an exported bundle holds only their names. The native Export and Import (`app-gpui/src/workbench/app_actions.rs`, near lines 896 and 949 at the reader's HEAD) do not call it. First a test that fails today, on the real backend in a temp home: a user settings file with an MCP server whose environment has a marker value (not a real key format), Export Settings to a file in the temp folder, and the exported file does not contain the marker while the secret store does (assert `ide_ssh::secrets::secrets_in_files()` first, as `app-gpui/src/workbench/tests/proofs29_settings_ssh.rs:38` does; never print the value on failure: compare, then fail with a fixed message). The same for Import: a bundle holding such a value is imported and afterwards the user settings file does not hold the marker. Then the calls, in the same order as the adapters. Say whether anything else of the native app writes or copies the user settings whole (settings sync, a backup, a "copy settings" action: grep) and would carry such a value.

## The items

1. Reload with Encoding (`app-gpui/src/editor/view.rs` near 1289) calls `Documents::set_encoding` directly; the backend's `ops::doc::doc_set_encoding` now also re-reads a virtual document (a library source in an archive, a language server's document, a Git revision) from its origin in that encoding, and its answer carries the byte-order-mark state the status bar shows (`crates/ide-host/src/ops/doc/commands.rs:53-63`; webview `src/editor/core/view/StatusItems.tsx:368-372`). Call the backend's function as the adapter does; tests: a file on disk (existing), a document opened from an archive in the temp project (build a small zip or jar in the test with the standard library or a crate already in the lock file; no toolchain), and the BOM state shown.
2. Completion: an explicit request (Ctrl+Space) bypasses the back-off after "no server here", and a successful answer clears it (`app-gpui/src/editor_lsp/completion.rs` near 131 and 143; webview `src/editor/lsp/completion.ts:514`, `:527`); the list's type column shows `label_description` when the item has one (`completion_view.rs` near 89: read which wins in the webview when both a detail and a description exist).
3. Stop while a run is starting cannot cancel: the native start passes no request id (`app-gpui/src/run/driver.rs` near 1096; adapter `run_start`, backend `ops::run::start_request`; webview `src/features/run/store.ts:1023`, `:1336`). Pass a request id and cancel it on Stop as the webview does; test with the stand-in runner: Stop during a start that waits leaves no process and the configuration is not shown running.
4. Closing the last project window: `general.closeWithoutWelcome` is ignored and the Welcome window always opens (`app-gpui/src/app_models/close_guard.rs` near 415; webview `src/features/app/closeGuard.ts:52`, `:227`). Follow the setting; `general.closeTabsWithWindow` is a larger change: read it and list it.
5. Recent projects show the stored branch where the webview shows the live one (`app-gpui/src/workbench/mod.rs` near 3186 and 3246; read what the adapter of `recent_projects` returns now and what the Welcome window and the Recent Projects popup show). No test reads a repository outside the temp folder or the user's git configuration.
6. The marketplace shows `publisher` where the webview shows `publisherName` (`app-gpui/src/plugin_models/plugins_page.rs` near 1442).
7. A plugin's network question (`net:ask`) is answered Deny natively without asking (`app-gpui/src/workbench/plugin_runtime.rs` near 1037; webview `src/features/plugins/netAsk.tsx:46-48`). Read what the webview asks, what each answer stores and for how long; build the question as the native app's other confirmation dialogs are built. The default on Escape or on closing the dialog is Deny; a test asserts that no request leaves before Allow (a loopback listener the test opens, which must see no connection until the answer), that Deny is what a closed dialog gives, and that the answer is remembered exactly as the webview remembers it. If this does not converge, the native app keeps denying: that is the safe state.

Not followed, recorded only (the manager's decision): the backend's new `OPENIDEA_DATA_DIR` and sibling folder settings; the native app's folders stay as they are.

For each item: a test that fails first, then the change. No assertion is loosened.

## Rows

Say for each of `ipc:settings_export`, `ipc:doc_set_encoding`, `ipc:lsp_completion`, `ipc:run_start`, `ipc:window_close_context`, `ipc:recent_projects`, `ipc:plugin_market`, `ipc:plugin_answer_prompt` which of the missing clauses in its proof (`docs/gpui-port/parity.json`) is now proven, by which test. Do not write to `parity.json`.

## Changed since

BEFORE LAUNCH (for the manager; delete this paragraph then): name the undeclared drafts of other agents then in the tree and whether the line numbers above moved.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&`, in a script or from the shell, and do not leave a polling loop behind. If any test fails in a whole-suite run, keep that run's log and quote the failure whole (never a line that holds a secret value).

## Report

Besides the format of `$S/int_rules.md`: Part 0 first (the tests, whether each failed before, what else copies the user settings whole); one section per item (the adapter and webview with `file:line`, the native change, the test and whether it failed first); what is left.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`. The real binary is not needed; if you run it, only through `$S/insp8/run.sh` with a temp home and project under a neutral path `/private/tmp/openidea-inspect-<random>/`; never the Plugins marketplace (it fetches), the Agents tool window, the Agents pages of Settings, the Services window's Services tab or a debug session.
