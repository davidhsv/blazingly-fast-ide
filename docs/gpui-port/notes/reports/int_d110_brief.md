# Integrator brief: the app's own interrupt handler, then tabs in a single-file window (alone)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d110/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: do the parts in order and finish them. Stop early only if a part is blocked, and say what blocks it. Send one final report only, after every gate has finished.

## Part A: processes the app starts get the default interrupt disposition

Measured by the previous integration (`docs/gpui-port/notes/reports/int_d109.md`, A1): when the IDE's own process has SIGINT ignored (started by a script with `&`, by `nohup`, by some launchers), every process it starts inherits that, and Stop, which sends SIGINT first (`crates/ide-run/src/process.rs`, read only), never stops a run. A process that has a HANDLER for a signal passes the DEFAULT disposition to what it executes; only "ignored" is inherited.

So: the native app installs a handler for SIGINT at start, in `app-gpui/src/main.rs` or `lib.rs` `init`, and on SIGINT quits the application the way Quit does (the close guards included, so an unsaved file is asked about; if that is not reachable from a signal, say what happens instead and why). Use a safe API already in the dependency graph: `tokio::signal::unix::signal(SignalKind::interrupt())` by adding the `signal` feature to the existing `tokio` dependency in `app-gpui/Cargo.toml` (a feature of a dependency that is already there, not a new dependency; say what it adds to `Cargo.lock`), or another safe API that is already locked. No `unsafe`.

Proof: a test that starts a run of a shell script through the app in a test process whose SIGINT is ignored and shows that Stop stops it (the existing Stop tests in `workbench/mod.rs` show the pattern; the previous integration reproduced the ignored state with `trap '' INT` around the test command: do the equivalent, and say exactly how the test gets a process with SIGINT ignored; if a `#[gpui_kit::test]` cannot be given that state, write the proof as a script in `$S/d110/` that runs the debug binary under `trap '' INT` with temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`, drives Run and Stop with the inspection script (`$S/insp/USAGE.md`), and checks the process is gone, and put its output in the report).

## Part B: tabs in a single-file window, and nothing else

Notes: `docs/gpui-port/notes/integration/keymap_reference_light_edit_tabs_integration.md`, ONLY the part on tabs in a single-file window (the Keymap Reference part is already integrated); writer's report: `docs/gpui-port/notes/reports/draft_keymap_reference_light_edit_tabs.md`. Draft file (committed, undeclared): `app-gpui/src/workbench/light_edit_tabs.rs`. Waiting tests: `app-gpui/src/workbench/tests/proofs13_windows_pending.rs` (two tests: they must pass and are then declared, moved into `proofs13_windows.rs`).

What it does: `Content::Editor` becomes a variant with the tab fields of `Content::Project` (about 45 places that match on it today become or-patterns; the notes list every one with `file:line`: re-check that list against the current code with `grep -n "Content::Editor" -r app-gpui/src`, the tree has moved), so a window that edits loose files has the tab strip, Close Tab and Close Other Tabs, the tab menu, splits, a find bar per tab, the session of its tabs, the two hints when its last tab is closing, the title bar and status bar following the active tab; a second loose file opens as a tab of that window instead of a new window. It removes `light_find_bar` and `light_editor_element` (the previous integrations changed `light_editor_element` to open the editor's and gutter's menus for a lone file: the shared pane must keep that; the test `workbench::context_menus::tests::proofs16_menus::the_editors_menu_without_a_project_opens_on_the_text_and_copies` must still pass, unchanged).

How to work: this touches `workbench/mod.rs` widely. Apply it in steps that each compile (the variant and the or-patterns first, with behaviour unchanged; then the tab strip; then routing a second file; then Close Tab and the window's close; then the session), running the single-file-window tests after each step: `cargo test --lib -- light_edit proofs13_light_edit_close proofs13_windows single_file loose_file` and whatever else `grep -rln "Target::File\|open_file(" app-gpui/src --include='*.rs'` shows to be tests of such windows. Where a note and the webview source disagree, the webview source wins (`src/features/app/windows.ts`, `src/features/app/__tests__/windows.test.ts` cases 8 and 11, `src/features/app/__tests__/lightEditClose.test.ts`, `src/workbench/EditorArea.tsx:19-23`).

Gaps the writer named, to settle against the webview: a window whose first file opens in a viewer (an image) takes no tabs; a tab that fails to open is only logged where the webview notifies (`openFailed`): do the notification if it is small.

After it passes, look at it in your own binary's window (inspection script; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; launch the binary with a FILE argument, then `open⇥<second file>`; never open the Services tool window): shots of the window with two tabs, of the tab's menu (`rclick` on a tab), and after closing one tab. The tab strip must look like a project window's.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, with nothing else running beside it; every test you added, declared or changed twenty times by filter; `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings; `cargo check --release` once (Part A touches start-up).

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name, "N of M cases" for spec rows and, for partial, what is missing. List every test left in a `_pending` file with the behaviour it waits for, and every app fix with the webview `file:line` it follows.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,database_extras,diff_tab,agents_views}.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/vcs_diff/{wrap,tab_view}.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/agents/{config_pickers_view,new_task_view}.rs`, `app-gpui/src/agents/composer_view/`, `app-gpui/src/agents/new_task_view/`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you.
