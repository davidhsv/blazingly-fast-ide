# Integrator brief: edit targets, popup placement, the error log sink; then Annotate

Read first: `$S/int_rules.md` (hard rules and report format), `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d102/`. The inspection script's steps are in `$S/insp/USAGE.md`; a launcher that captures the window at each mark is `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh` (copy and adapt it; always `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR` on temp folders; `click` coordinates are window points: a saved shot's pixel times 1.024).

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`: it contains other agents' untracked drafts.

The drafts below were written without a compiler, some days ago; the tree has moved since. Their notes are hypotheses: read the current code before applying a hunk, re-anchor it yourself, and where a note and the webview source disagree, the webview source wins (`src/…`). For each draft: declare it, make it compile, make its tests pass, keep what the webview does and drop what it does not. Every action registered is run through the registry by a test that asserts what the window shows.

When about two and a half hours are used, stop adding items and report what is left.

## Part A: edit targets, popup placement, the error log sink

Notes: `docs/gpui-port/notes/integration/edit_targets_popups_integration.md`. Draft files (committed, undeclared): `app-gpui/src/platform/{edit_target,error_log_sink}.rs`, `app-gpui/src/workbench/{edit_actions,popup_placement}.rs`, `app-gpui/src/ui/popup_anchor.rs`. (`app-gpui/src/vcs/shelf_time.rs` of the same notes is already integrated.)

Decisions already taken, which override the drafts:

- The greying of Edit menu items follows the Tauri menu and the webview's menu exactly: read `src-tauri/src/menu.rs` and the webview's menu code; if they never grey an item, neither does the native menu (the draft greys on every platform: restrict or remove it).
- The error log sink is fed by what the webview's error log is fed by (read the source under `src/platform/` that the webview's error log reads: `errorLog.ts` and its callers), not by a guess. No log file and no `help.showLog` (they would need new dependencies).
- The webview's Go to Line has no range hint; do not add one.
- A popup anchored at its button must agree with the shared tooltip: a press on a target keeps its tooltip off the popup the press opened (`ui/tooltip.rs` `note_press`, test `workbench::tests::a_pressed_widgets_tooltip_stays_off_the_menu_it_opened`).

## Part B: one defect seen in the real window

The diff viewer draws font ligatures (`->` as an arrow, `+=` joined) while the editor does not and Settings → Editor → Font has "Enable ligatures" off (shot `$S/d101/shots/tour-05-diff-1.png`). Find how the editor turns ligatures off (`editor/view.rs` `code_font` and its font features) and make every surface that draws code with the editor's font follow the same setting: the diff sides, the merge editor, Local History's diff, hover and completion popups, the terminal if the webview's terminal follows the setting too (read `src/features/terminal/theme.ts`). Test on the font value each surface draws with (the features it carries), with the setting off and on.

## Part C: Annotate

Notes: `docs/gpui-port/notes/integration/annotate_integration.md`; report of the writer: `docs/gpui-port/notes/reports/draft_annotate.md` (what is built, what is not, which tests are likely to fail first). Draft files (committed, undeclared): `app-gpui/src/editor/view/annotations.rs` (a child module of `editor::view`), `app-gpui/src/workbench/annotate.rs`.

It registers `vcs.annotate` (IPC `vcs_blame`): the annotation column in the editor's gutter (date and author per line, age colours, hover tooltip, click shows the commit, the column's menu, re-blame after an edit and after the head moves). Check first the three hypotheses its report names (two `EventEmitter` impls on `Editor`; `publish_context` on tab change; the hover child not swallowing the editor's mouse events). After it passes, take a screenshot of the column in your own binary's window (a temp Git project with two commits by two authors, `git -c user.name=… -c user.email=…@example.invalid`) and look at it: the column must not overlap line numbers, breakpoints or the fold column.

Not part of this brief: Annotate inside the diff and merge viewers, inline blame.

## Do not touch

Other agents' drafts, committed but undeclared or untracked: `app-gpui/src/workbench/{project_view_options,zoom_actions,build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/project_view/{pane,tree_clicks}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, any other new file under `app-gpui/src` that you did not create, and every undeclared `proofs*` test file.
