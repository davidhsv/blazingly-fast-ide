# Integrator brief: the `proofs12` tests (diff viewer, Find in Files), then edit targets and popup placement

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d104/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish Part A, then Part B. Stop early only if a part is blocked, and say what blocks it. Work one file at a time (declare, compile, run that module's tests by filter, fix, next).

## Part A: `proofs12` (43 tests written without a compiler)

Notes: `docs/gpui-port/notes/integration/proofs12_diff_find.md` (per spec file, every case with the test that proves it or the reason; the `mod` lines; hunk H1); writer's report: `docs/gpui-port/notes/reports/draft_proofs12_diff_find.md`. Files: `app-gpui/src/workbench/tests/proofs12_{find_popup,diff_window,diff_soft_wraps}.rs`, `app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view.rs`, `app-gpui/src/vcs_diff/diff_options/tests/proofs12_moved_code.rs`.

- The writer says eleven tests are expected to fail because the native app lacks a behaviour of the webview: Jump to Source from a revision-to-revision diff, the non-modal commit-diff setting, soft wraps in the diff (with the "Use Soft Wraps" menu item), moved-code links, marks and the "(edited)" label, a resizable diff window whose size is remembered and that Cmd+W closes, a resizable Find in Files popup, a multi-line query field, underlined scope-tab letters, a browse dialog that starts at the current folder.
- Declare every file; for each failing test read the webview source it cites and decide: the test is wrong (fix the test, never weaken an assertion), or the app lacks the behaviour. Where the missing behaviour is small and local, build it with the test as its proof (candidates: Cmd+W closing the diff window, Jump to Source from a revision diff mapping the line to the working file when it exists, the "Use Soft Wraps" item and the diff reading the stored setting if the diff's painter already supports wrapping; say what you found). Where it is not small, move the failing tests, unchanged, into a file named `proofs12_<topic>_pending.rs` beside the original and leave that file undeclared: it is the specification of a later slice. No `#[ignore]`.
- H1 (`DiffView::soft_wraps()`) only if the soft-wrap tests are declared.

## Part B: edit targets and popup placement

Notes: `docs/gpui-port/notes/integration/edit_targets_popups_integration.md` (written days ago; the tree has moved: read the current code before applying each hunk and re-anchor it; where a note and the webview source disagree, the webview source wins). Draft files (committed, undeclared): `app-gpui/src/platform/edit_target.rs`, `app-gpui/src/workbench/{edit_actions,popup_placement}.rs`, `app-gpui/src/ui/popup_anchor.rs`.

Decisions already taken, which override the drafts:

- Drop the draft's greying of Edit menu items by edit target (notes section A4): the Tauri menu greys an item only by its when-clause (`src-tauri/src/menu.rs:1034-1052`).
- `app-gpui/src/platform/error_log_sink.rs` stays undeclared: nothing native feeds it. Do not touch it.
- The webview's Go to Line has no range hint; do not add one.
- The popup slot changed after the drafts were written: `workbench/mod.rs` wraps every popup in a shadowed, rounded frame aligned at the start, and every dialog layer goes through `workbench::dialog_frame`. Popup placement builds on those.
- A press on a tooltip's target keeps the tooltip off the popup the press opened (`ui/tooltip.rs` `note_press`; test `workbench::tests::a_pressed_widgets_tooltip_stays_off_the_menu_it_opened`): keep it passing.

After Part B passes, take one screenshot per changed popup position in your own binary's window (inspection script, `$S/insp/USAGE.md`; launcher to copy: `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; `click` coordinates are window points: a saved shot's pixel times 1.024) and look at them: a popup anchored at its button or at the caret must sit where the webview puts it and stay inside the window.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name and, for partial, what is missing. For each spec file of Part A, the count "N of M cases proven" from the notes' table after your changes. List every test left in a `_pending` file with the behaviour it waits for.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{annotate,project_view_options,zoom_actions,build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/editor/view/annotations.rs`, `app-gpui/src/project_view/{pane,tree_clicks}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, and every undeclared `proofs*` test file that this brief does not hand you.
