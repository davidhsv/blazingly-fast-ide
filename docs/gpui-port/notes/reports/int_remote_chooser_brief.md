# Integrator brief: the remote folder chooser of the Deployment servers page, shared with Run Targets (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format; the two other briefs it names no longer exist: skip them) and `$S/view_rules.md`. Your own scratch subfolder: `$S/drfci/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

This is a small slice; it may be launched together with another one. If so, the other brief says which lands first.

## After the merge of upstream v0.1.46 (binding, read first)

Upstream added FTP and FTPS server types to deployment. `$S/drafts/recheck_after_merge.md`, section 3: the draft is valid with changes and does not compile at HEAD as written: five exhaustive matches need an FTP arm, and three FTP rules must be followed (the `ftp:<id>` password key, the reachability check, the FTP rule in `settings_model/ssh_connection.rs`); read that section and the webview at HEAD (`src/features/settings/pages/deployment.tsx`) before applying a hunk. Section 6 of the same note lists eight mismatches of the compiled Deployment servers page against the webview at HEAD (no FTP type choice or fields, server groups, the FTP password in the connection settings, and the rest): they are this slice's too where they are on the path of the chooser; the others are listed in your report as not done. Tests never open an FTP connection: a stand-in or a refused loopback port only.

## The slice

Notes (hunks A1 to A15 that put the chooser on the Deployment page, B1 that replaces `build_run_targets/folder.rs` with a thin use of the shared browser, B2 for four lines of a test; the rows; behaviours with `file:line`; hypotheses; the tests most likely to fail): `$S/drafts/remote_chooser_integration.md`; the new content of `folder.rs` for B1: `$S/drafts/remote_chooser_run_targets_folder.rs`. Report: `docs/gpui-port/notes/reports/remote_chooser_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/settings_view/pages/remote_folder_browser.rs`, `remote_folder_browser/{panel,proofs37_remote_folder_browser}.rs`, `build_deployment/remote_folder.rs`, `build_deployment/proofs37_deployment_chooser_view.rs`, `app-gpui/src/workbench/tests/proofs37_settings_deployment_chooser.rs`.

The webview's chooser: `src/features/settings/pages/deployment.tsx:40-237` (and Run Targets' `runTargets.tsx` browser, which differs: the draft keeps the differences in a per-page setting; check each difference it lists against the two webview components). The notes are hypotheses; where a note and the webview source disagree, the webview source wins. Land A (Deployment) first with its tests; then B (Run Targets on the shared browser), after which every existing Run Targets test passes unchanged except the four lines of B2, and the old browser is gone. If B does not converge, leave Run Targets on its own browser, say why, and keep A.

## Points to settle by measurement, not by the notes

- Safety of listing: a test build lists nothing unless a test installs a source; the end-to-end tests install the real backend's listing wrapped so it answers only absolute local paths under the test's server folder (assert that a path outside is refused before the backend is reached, and that no test path can name an SFTP server). No network.
- The password step of the chooser hands the password with the state of its own Save checkbox (`deployment.tsx:75`, unticked at first), unlike Run Targets' `false`: assert both in view tests with the stand-in setter; the password is drawn as bullets only, cleared when a new listing starts and when the chooser closes (assert), never staged, never in a debug output (two types of the draft derive `Debug` near the state: check none of them can hold the secret).
- Nothing lists or sorts on the window's thread; a listing that arrives late is dropped; 100,000 entries: measure the sort and the first frame in a debug build and say the numbers; only rows in view are drawn.
- The chooser is drawn under its field, not as a modal, where the webview's is a dialog: say what a user sees differently (what stays clickable behind it) and that the chooser closes when the server, tab, page, type or SSH configuration changes or a mapping is added or removed (tests for each).
- Keys: Up, Down, Enter, Backspace and Escape go to the chooser's list while it has the keyboard and to the Settings dialog otherwise (Escape with the chooser open closes the chooser, not the dialog: check against the webview).
- The existing Deployment tests (`proofs28_*`) and Run Targets tests (`proofs30_*`) pass by filter before and after; the page's `lines()` text for Root path and the mapping rows is unchanged.
- The theme tokens `bg-tree-hover` and `h-list-row` exist (grep the theme); use the tokens the other lists of Settings pages use.

## Safety

- The real binary: only through `$S/insp8/run.sh` with a temp `HOME`, `OPENIDEA_GPUI_HOME` and project under a neutral path `/private/tmp/openidea-inspect-<random>/` (never under the scratch folder: pages draw paths). Only a Deployment server of type local folder under that path; never an SFTP server, never Test Connection on anything but that local folder, never a password typed, never Browse… on a Run Targets SSH target; never the Agents tool window, the Services window or the Agents page of Settings. Shots, light and dark: the chooser open under Root path with a subfolder selected; the mapping's chooser; the field after Select.

## Rows

`settings:build.deployment`: done if the chooser lands with the two end-to-end tests; what stays unbuilt (the local chooser's title and start folder, which the platform's prompt does not take; a busy mark) is named in the row's text as not built without keeping the row partial only if the webview's own page has nothing a user can do that the native page cannot: say which it is. `ipc:remote_list` (already done: add the proof). `settings:build.runTargets` unchanged.

## Changed since the notes were written

HEAD is `c3d522d56`. The notes were written on the tree at `927cef99`. Of the existing files the hunks anchor on, measured with `git diff --shortstat 927cef99 HEAD`: unchanged are `settings_view/pages/mod.rs`, `build_deployment/paths.rs`, `build_run_targets.rs`, `build_run_targets/folder.rs` and `build_run_targets/proofs30_run_targets_view.rs`; changed are `settings_view/pages/build_deployment.rs` (two commits, 12 lines in, 9 out), `build_deployment/connection.rs` (one commit, 2 in, 1 out) and `workbench/mod.rs` (fifteen commits, 477 in, 175 out: the test module list and the tab strip moved, so re-anchor every hunk on it by name or quoted text). Read `git diff 927cef99 HEAD -- <file>` for the three changed files before applying a hunk on them.

The other agents' undeclared drafts now in the tree are listed in `$S/undeclared_now.txt`: leave alone every file named there that this brief does not hand you. Eight tests that wait for a run or debug process to stop timed out once under a machine load of 80 to 130 (`workbench::debug::tests::proofs40_debug_guards::*`, `workbench::tests::proofs18_e2e_run::cmd_f2_stops_a_running_process`, `workbench::tests::proofs40_run_start::*`) and passed in clean runs: if they fail in your whole-suite runs, quote them and go on. The repository this is pushed to is public: your report and files hold no name, path, branch, author or content of any project of the owner other than this repository.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside either (write `uptime` before and after each run into the same file); after the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Your final message must be the full report (no short closing line after it). Besides the format of `$S/int_rules.md`: the safety points one by one with their tests; whether B landed; the measurements; the rows; which way each hypothesis went; the shots' absolute paths and what you saw in each.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
