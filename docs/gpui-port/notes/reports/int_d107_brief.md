# Integrator brief: the `proofs15` tests with three small fixes, then the Project view's options and UI zoom

Read first: `$S/int_rules.md` (hard rules and report format), `$S/tests_brief.md`, `$S/view_rules.md`, `$S/wiring_brief.md`. Your own scratch subfolder: `$S/d107/`.

Format only files you edited, taken from `git diff --name-only` plus the draft files this brief hands you by name. Never build a file list from `git status`.

Do not stop on your own estimate of elapsed time: finish Part A, then Part B. Stop early only if a part is blocked, and say what blocks it. Work one file at a time (declare, compile, run that module's tests by filter, fix, next).

## Part A: `proofs15` (12 tests written without a compiler, plus 4 pending) and the behaviour three of the pending tests wait for

Notes: `docs/gpui-port/notes/integration/proofs15_missing_tests.md` (one line per inventory row: what was found, the test, the state the row reaches; the `mod` lines with their anchors); writer's report: `docs/gpui-port/notes/reports/draft_proofs15_missing_tests.md`. Files, all under `app-gpui/src/workbench/`: `editor_extras/tests/proofs15_{folding,paste_history,reformat}.rs`, `editor_language/tests/proofs15_implement.rs`, `vcs_operations/tests/proofs15_reload_all.rs`, `github/tests/proofs15_github_refresh.rs`, `plugins_window/tests/proofs15_plugins.rs`, `terminal_actions/tests/proofs15_terminal.rs` (the first child module of that `mod tests`), `tests/proofs15_{full_screen,merge_resolve}.rs`; pending: `editor_extras/tests/proofs15_{folding,paste_history}_pending.rs`, `editor_language/tests/proofs15_implement_pending.rs`.

- A test that fails because the TEST is wrong is fixed in the test, without weakening what it checks. A test that fails because the APP differs from the webview: read the webview source it cites; fix the app when the fix is small and local, otherwise leave the test in a `_pending` file, undeclared. No `#[ignore]`, no real sleeps.
- Build these three, each proven by its pending test (move the test into the declared file once it passes), after reading the webview source the notes cite:
  1. Settings › Editor › General › Code Folding's "fold by default" choices (file header, imports, documentation comments) are applied when a file opens. (Needs the test accessor `Editor::painted_fold_markers` the notes describe, or an existing accessor that says which regions are folded.)
  2. Implement Methods, Generate and Override re-indent the lines the server inserted (the helpers exist in `app-gpui/src/editor_lsp/intentions.rs` and nothing calls them).
  3. Paste from History takes its keys from the keymap's bindings of its actions (`editor.pasteHistory.paste`, `.remove`) instead of fixed Enter, Delete and Backspace.
- Not here: Reformat File… hiding Optimize imports for a plugin formatter, Refresh Pull Request reloading the timeline, `run.updatableRunning` for debug sessions.

## Part B: the Project view's options and panes, and UI zoom

Notes: `docs/gpui-port/notes/integration/project_view_options_integration.md`; writer's report: `docs/gpui-port/notes/reports/draft_project_view_options.md` (what is built, what is not, the hypotheses to check first, the tests likely to fail). Draft files (committed, undeclared): `app-gpui/src/project_view/{tree_clicks,pane}.rs`, `app-gpui/src/theme/zoom.rs`, `app-gpui/src/workbench/{project_view_options,zoom_actions}.rs`. The notes are days old and the tree has moved (the Project view now colours rows by VCS status through `set_decorations`, the workbench root sets the interface font, `theme/appearance.rs::apply` sets `--font-ui` and the font sizes from the appearance options, popups have a shared frame): read the current code before applying each hunk and re-anchor it; where a note and the webview source disagree, the webview source wins. Never run `cargo fmt`, whatever the notes say.

It registers `project.option.{previewTab,singleClickOpen,singleClickExpand,autoscrollFromSource,autoscrollToSource}`, `project.viewMode.{project,projectFiles,openFiles,changedFiles}`, `project.{findUsages,reformatCode,optimizeImports,reloadFromDisk}`, `view.{zoomIn,zoomOut,resetZoom,quickSwitchScheme}`, and adds the Project tool window's options menu.

Zoom: the draft scales the theme's length tokens only. Before accepting it, read what the webview's zoom does (`src/features/settings/appearanceRuntime.ts:96-110` and where `appearance.zoom` is applied: a CSS zoom of the whole document scales everything) and measure in your own binary's window (inspection script: `$S/insp/USAGE.md`; launcher to copy: `docs/gpui-port/notes/inspections/2026-11-14-c/run.sh`; temp `OPENIDEA_GPUI_HOME`, `HOME`, `ZDOTDIR`; never open the Services tool window in a shot) what does and does not scale at 150 %: take a shot at 100 % and one at 150 % and look. If GPUI can scale the whole window (look for a window or root scale in `gpui-pre-0.3.12`, e.g. the rem size that `rems()` lengths follow, or a scale factor override), prefer that to scaling tokens; otherwise list precisely what stays unscaled (literal pixel sizes in views) with counts by directory (`grep -rn "px(" app-gpui/src | wc -l` is the upper bound), and keep the rows partial.

## Gates (they replace rule 12 of `$S/int_rules.md`)

The whole suite twice to files; every test you added, declared or changed twenty times by filter (report passes of twenty and every test that failed at least once); `cargo test --test web_view` once; clippy; `cargo build` with 0 warnings. If `workbench::tests::proofs13_agents_failed_send::recovers_images_after_failed_regeneration_with_a_new_draft` fails in a suite run (it did once under load), re-run it alone five times and report both.

## Report

Besides the format of `$S/int_rules.md`: one table of every `parity.json` key the notes name, with the state it reaches (done or partial), the proving tests by full name and, for partial, what is missing. List every test left in a `_pending` file with the behaviour it waits for, and every app fix with the webview `file:line` it follows. For zoom: the two screenshots' paths and what you saw.

## Do not touch

Other agents' drafts, committed but undeclared: `app-gpui/src/workbench/{build_tools,database_extras,run_context,debug_extras}.rs`, `app-gpui/src/debugger/evaluate_dialog.rs`, `app-gpui/src/build_tools/`, `app-gpui/src/database/{dialog_parts,history_view,plan_view,modify_table_dialog,truncate_dialog,import_dialog}.rs`, `app-gpui/src/platform/error_log_sink.rs`, every `_pending` test file this brief does not hand you, and every undeclared `proofs*` test file this brief does not hand you.
