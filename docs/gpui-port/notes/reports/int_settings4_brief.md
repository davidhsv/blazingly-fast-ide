# Integrator brief: the Vim page of Settings (drafts written without a compiler)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/dset6i/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`. Never restore a file from a backup copy or with `git checkout`: undo your own edit by editing. Leave no polling loop behind (a loop on `pgrep -f "cargo test"` matches its own command line and never ends).

This is a small slice; it may be launched together with another small one. If so, land this one first.

## The slice

Notes (five hunks with quoted anchors, the rows with every test's full name, behaviours with `file:line`, hypotheses, the tests most likely to fail): `$S/drafts/settings_pages6_integration.md` (read this scratch copy: the copy under `docs/` was reformatted by the commit hook). Report: `docs/gpui-port/notes/reports/settings_pages6_writer.md`. Draft files (committed, undeclared, never compiled): `app-gpui/src/settings_view/pages/editor_vim.rs`, `app-gpui/src/workbench/settings_vim_page.rs`, `app-gpui/src/workbench/tests/proofs35_settings_vim.rs`.

The webview: `src/features/vim/VimSettingsPage.tsx` (144 lines), `src/features/vim/settings.ts`, `rcFile.ts`, `index.ts` (the `vim.settings` action and the page's registration). The notes are hypotheses; where a note and the webview source disagree, the webview source wins.

## Points to settle by measurement, not by the notes

- The page applies at once (check it in the webview): mount it as the other pages of that kind are mounted (`app-gpui/src/workbench/settings_feature_pages.rs`: `mount_page`, `set_page_labels`, the helper `assert_nothing_staged` of its tests); the draft has a module of its own (`settings_vim_page.rs`): fold the mounting into the shared module if that is a few lines, keep the rc reading and the action where they fit.
- The rc file. In the real app the page reads `.openideavimrc`, else `.ideavimrc`, in the user's home, and up to 32 files their `source` lines name, through the backend's text reader, only while the dialog shows the page. Prove by test: under a backend with a home of its own (every test) the home is that folder (the draft's `rc_home`; assert the path once, before any read); a `source` line cannot make the page read outside that home in a test, and in the real app say what bounds it (a `source` of an absolute path, of `~/.ssh/config`, of a 1 GB file, of a file that sources itself: read the webview's `rcFile.ts` and the native `features_a/vim/rc.rs`, do what the webview does, never read more than its limits, never block the window's thread, and never draw a sourced file's text on the page beyond what the webview shows: say what it shows). No test reads the user's real home.
- Nothing native reads the Vim setting yet, by the writer's reading (`VimMode` is built only in tests, no key dispatcher asks the conflicts table, `vim.toggle` and `vim.reloadRc` are not registered): confirm by grep and say it in the row's text; the page row stays partial for that reason and you do not wire the engine here.
- The handler drop-downs take the mouse only in the draft: give them the keyboard if the native drop-down the other pages use already has it (use that control instead of a new one).
- The page is taller than the dialog: the tests scroll; check each click target is inside the dialog's visible part when clicked (a click outside must fail the test, not pass by accident).

## Safety

- No test uses the network, a toolchain or anything under the user's real home.
- The real binary: only through `$S/insp8/run.sh` (short `PATH`, temp `HOME`, `ZDOTDIR`, `OPENIDEA_GPUI_HOME`, so the rc file looked for is under the temp home; put a small `.ideavimrc` with a `source` line there to see the status row), with a temp project under a neutral path. Never open the Agents tool window or the Services window, and never the Agents page of Settings (it would fetch a registry from the network). Shots, light and dark: the page with Vim off, with Vim on and the conflicts table, the rc status row.

## Rows

`settings:editor.vim` (partial: nothing reads the setting), `action:vim.settings`. Check which keys exist in `docs/gpui-port/parity.json`.

## Changed since the notes were written

BEFORE LAUNCH (for the manager; delete this paragraph then): the notes were written on the tree at bbb49e1c while the templates pages integration was in flight; say here which of the existing files the five hunks anchor on changed (`settings_view/pages/mod.rs`, `workbench/mod.rs`, `workbench/settings_window.rs`, and where `register_actions` calls are listed).

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, nothing else running beside the first (write `uptime` before and after each run into the same file); during the second, every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once, with its failure); `cargo test --test web_view` once; clippy `--all-targets -- -D warnings`; `cargo build` with 0 warnings. Never pipe a test run into `head`. Start no test run with `&` inside a script. If any test fails in a whole-suite run, keep that run's log and quote the failure whole.

## Report

Besides the format of `$S/int_rules.md`: the rc file section first (where it is read from in a test and in the real app, the bounds on `source`, the tests); the rows table with the proving tests by full name; which way each hypothesis went; the shots' absolute paths and what you saw in each.

## Do not touch

Every undeclared draft of other agents under `app-gpui/src` and every undeclared `proofs*` or `*_pending.rs` test file this brief does not hand you. No edits under `crates/`, `docs/`, `src/`, `src-tauri/`, `e2e/`. Do not write to `DECISIONS.md`, `parity.json` or `INVENTORY.md`.
