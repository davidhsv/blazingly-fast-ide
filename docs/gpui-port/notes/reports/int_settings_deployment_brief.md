# Integrator brief: the Deployment servers page of Settings (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dseti/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

## The slice

Notes (three hunks with quoted anchors, the rows expected with every test's full name, each behaviour of the webview's page with `file:line`, the hypotheses, the tests most likely to fail, what the real window must be checked for): `$S/drafts/settings_pages_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Writer's report: `docs/gpui-port/notes/reports/settings_pages_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/settings_view/pages/build_deployment.rs`, `app-gpui/src/settings_view/pages/build_deployment/{connection,paths,proofs28_deployment_view}.rs`, `app-gpui/src/workbench/tests/proofs28_settings_deployment.rs`.

The webview's page: `src/features/settings/pages/deployment.tsx` (755 lines), its model `src/features/deployment/` and tests. The notes are hypotheses; where a note and the webview source disagree, the webview source wins. The writer says it did not read `app-gpui/src/deployment/secret_dialog.rs`, `app-gpui/src/features_b/deployment/store.rs`, the tests of `app-gpui/src/workbench/deployment.rs` and `app-gpui/src/settings_view/pages/languages_sdk.rs`: read them, and where the page duplicates what one of them already has (the secret question, the server store's types, a path field with Browse), use the existing one.

## Safety comes first (hard)

- Network: no test opens a connection to anything but a listener it opened itself on the loopback address. Before you run `proofs28_settings_deployment.rs` at all, read every test in it and in `proofs28_deployment_view.rs` and write down, for each Test Connection a test presses, where the backend would connect: a local-folder server connects nowhere; for the SFTP case read `ide_host::ops::deploy::deploy_test_connection` and the SSH manager down to the socket and say what the configuration `box` of the test resolves to. If any path can reach a host that is not the loopback address, change the test's configuration to `127.0.0.1` on a port the test holds closed (bind a listener, take its port, drop it) so the failure is a refused connection, and assert the address the backend was given. A test must not depend on DNS.
- Secrets: the password typed in a test is kept only where a test build keeps secrets (the vault file under the test's temp home). Prove by reading and by one assertion placed before any secret is typed that the system keychain cannot be reached in a test build (`ide_ssh::secrets::secrets_in_files()` and how `Backend::init` decides it: the writer's hypothesis is that a temp home counts as isolated). The password is never drawn (bullets only: assert the painted text), never in a settings file (assert the files' content after Apply), never logged, never in a `Debug` output (the draft uses `SecretText` from `app-gpui/src/database/store.rs`: check its `Debug`), never left in memory of a closed page longer than the webview keeps it (read `deployment.tsx:75` and around).
- The user's SSH configuration, keys, known hosts and agent are never read by a test: read where the page's SSH configuration list comes from (the global `ssh.configs` setting, by the notes) and whether anything on that path falls back to `~/.ssh/config`; the trust-host-key step in tests goes to a known-hosts file under the temp home (assert the path), never the user's.
- What the user types (host, port, paths) is validated where the webview validates it, with its messages, and never becomes part of a shell command.
- The real binary: only through `$S/insp8/run.sh` (short `PATH`, temp `HOME`, `ZDOTDIR`, `OPENIDEA_GPUI_HOME`), with a temp project under a neutral path (`/private/tmp/openidea-inspect-<random>/`, removed afterwards). In it add only a server of type local folder under that temp path; Test Connection may be pressed for that local server only; never choose SFTP's Test Connection, never type a password, never open the Agents tool window or the Services window. Shots: the page empty, with a local server (Connection tab, Mappings tab, Excluded Paths), the result of Test Connection on the local folder, a validation message, light and dark.

## What the draft leaves out: close or list

- The remote folder chooser of Root path and Deployment path (`deployment.tsx:101-237`): neither Browse button is drawn. The Remote Host tool window already lists a server's folders (`app-gpui/src/deployment/remote_host_view.rs`): if a chooser can be built from it in under about 300 lines with tests on a local-folder server, build it; otherwise leave the row partial and say what it takes.
- The local chooser's title and start folder, the password field's focus when it appears, a busy mark: fix what is small.
- The page's strings are not in `src/platform/locales/text/en.json` by the writer's word (English is the source text): check how the webview localizes this page and that the native page shows the same text in English and goes through the same lookup the other native pages use for other languages.

## Rows

`settings:build.deployment` and `ipc:deploy_test_connection`, `ipc:ssh_set_secret`, `ipc:ssh_trust_host`: check which keys exist in `docs/gpui-port/parity.json` and how the neighbours of the same sections are named (the Settings pages section's keys: read three). A page row is done only when tests in a project window open Settings on the page, change each kind of control with the mouse or the keyboard, Apply, and assert what was written (global or project, as the webview writes it), and Reset and Cancel discard. An `ipc:` row is done only when a test asserts state that can only come from that backend function called with the arguments the Tauri adapter passes; a test against a fake proves the view, not the row.

## Changed since the notes were written

BEFORE LAUNCH (for the manager; delete this paragraph then): the notes were written on the tree at 54bae391 while the syntax tree integration was in flight; the hunks anchor on `settings_view/pages/mod.rs`, `settings_view/custom_controls.rs` and `workbench/mod.rs`. Say here which changed.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. A test that is unstable under the loop is not done: find the cause and fix the wait, not the assertion. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: the safety section first, one line per bullet above with how it was shown (the test, or the code read with `file:line`); the rows table with the proving tests by full name and for partial what is missing; which way each hypothesis went; the shots' absolute paths and what you saw in each.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
