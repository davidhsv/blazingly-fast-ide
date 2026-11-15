# Integrator brief: three small things seen in the real window, then the database extras (reconciled notes)

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d111/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish it. Stop early only if blocked, and say what blocks you. Send one final report only, after every gate has finished.

## Part A: three small things first

1. **A tooltip still covers the popup its target opened** when the pointer rests on the target after the click. Seen in the real window (`docs/gpui-port/notes/inspections/2026-11-15-b/03-project-widget.png` and `05-gear-menu.png`: look at them): `click` on the title bar's project widget or on the gear, `wait 1200`, and the target's tooltip is drawn over the popup's first rows. The earlier fix (`ui/tooltip.rs` `note_press`, test `workbench::tests::a_pressed_widgets_tooltip_stays_off_the_menu_it_opened`) covers a press made before the tooltip's delay ends. Hypothesis: opening the popup makes the target count as left and entered again (or the popup's backdrop does not stop the hover), so the delay starts anew. Reproduce it first in your own binary's window with the inspection script, find the cause in the code, and fix it in the shared tooltip so that a tooltip never shows while a popup or menu opened from its target is open (webview: `src/ui/Tooltip.tsx`, what hides a tooltip and what keeps it hidden while a menu is open). The test must reproduce the real flow: click, let more than the tooltip's delay pass on the test clock with the pointer unmoved, and assert the tooltip is not drawn and the popup's first row is.
2. **Settings → Appearance**: "Fallback font::" and "Size::" end in two colons (`…/2026-11-15-b/01-settings-appearance.png`). The shared form label adds a colon to a label that already ends in one: fix it in the helper (`settings_view/controls.rs` `form_label`), check every page that passes a label ending in a colon, and assert it in a test of the helper and one of the Appearance page's rows. Also "UI font:" sits lower than its field there: align the label with the field's first line as the other rows are.
3. **A test whose state never came**, once, in a suite run: `editor::intelligence::tests::proofs_editor_lsp::a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls` ("the problems did not happen within thirty seconds … reached later: None"). The two races found before were a click or an index read from a layout the app was still going to change. Loop it beside a running suite (at least 200 runs) until it fails, read which state did not come and why, and fix the cause (the test's order of waits, or the app if the fake server's diagnostics can really be lost).

## Part B: the database extras

Notes, reconciled against the tree a few hours ago: `docs/gpui-port/notes/integration/database_extras_reconciled.md` (37 hunk sites in 4 files, the `mod` lines, the 13 actions with their tests and IPC call sites, what is not built, the risks, and the order to apply). The older notes `database_extras_integration.md` are superseded by it. A script that checks every anchor occurs exactly once is `$S/dbx/anchors.py`: run it before applying (another integration changed `workbench/mod.rs` and `tab_views.rs` since) and re-anchor what no longer matches.

Draft files (committed, undeclared; the reconciler edited them to use the shared dialog helpers, the shared tooltip and `wait_long`): `app-gpui/src/workbench/database_extras.rs`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, and the test draft `app-gpui/src/workbench/tests/proofs6_database_console.rs`.

Apply in the notes' order: the six `pub mod` lines of `database/mod.rs` alone first (the views compile on their own), then the rest in one step, `database.rs` first. Settle the hypotheses the notes list as you go (the borrow checks, SQLite's EXPLAIN rows, the tree refresh after a successful `run_sql`, the chooser clicks in the console tests).

Rules specific to this feature: tests use SQLite in a temp folder only (no server, no network); a data source's password or URL userinfo is never drawn, logged, copied or part of a `Debug` output (the notes say the drafts were checked: check the code you add); a destructive statement (truncate, drop, modify table) runs only on the temp SQLite file. Never click or index from a layout read before the app's last update: wait for the final state, then read the bounds.

Differences from the webview the notes name (settle each against the webview source and fix it if it is small): Browse Query History falls back to the tree's selection rather than the selected source; Run SQL Script's file dialog has no `.sql` filter.

After it passes, look at it in your own binary's window (inspection script: `$S/insp/USAGE.md`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; a temp project holding a small SQLite file made with `sqlite3` or by the app; never open the Services tool window): shots of the Database tool window with the source's tree, the Modify Table dialog, the query history, a query plan tab, the Import dialog. Each must use the shared dialog frame, sit inside the window and show no cut text.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files, with nothing else beside it except: during the SECOND suite run, loop every test you added, declared or changed twenty times by filter (both real races found so far showed only beside a suite); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings. Say how each run was started (a command a script starts with `&` runs with SIGINT ignored).

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name (the 13 `action:db.*` rows, their `ipc:` rows, `point.*` and `unit:` rows), with the state it reaches (done or partial), the proving tests by full name and, for partial, what is missing; for each `ipc:` row both call sites (the adapter in `src-tauri/src/commands/` and the native call). The screenshots' paths and what you saw. Every app fix with the webview `file:line` it follows.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,diff_tab,agents_views}.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/vcs_diff/{wrap,tab_view}.rs`, `app-gpui/src/vcs_diff/view/moved.rs`, `app-gpui/src/agents/{config_pickers_view,new_task_view}.rs`, `app-gpui/src/agents/composer_view/`, `app-gpui/src/agents/new_task_view/`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file, and every undeclared `proofs*` test file this brief does not hand you.
