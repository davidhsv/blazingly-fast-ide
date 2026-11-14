# D102 integrator report: Part A done, Part B not integrated

Part A is done and every gate passed. None of Part B is integrated: I stopped adding items at the two-hour point. Nothing is committed (rule 2).

## 1. Gates (measured, from `app-gpui/`)

- **Suite run 1:** `test result: ok. 6908 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.96s`
- **Suite run 2:** `test result: ok. 6908 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 133.68s`
- **Each added or changed test, 20 runs by filter:**
  - `draws_with_the_editors`: 20/20 (2 tests)
  - `the_local_history_diff_is_coloured`: 20/20 (1 test)
  - `viewer_look`: 20/20 (2 tests)
- **`cargo test --test web_view`:** `ok. 1 passed`.
- **`cargo clippy --all-targets -- -D warnings`:** clean.
- **`cargo build`:** 0 warnings.
- **Failures seen during the slice:**
  - My first version of `the_local_history_diff_is_coloured_and_drawn_as_the_editor_draws` failed once. Cause: the test typed `x` before `fn`, so neither side contained a keyword. That was a fault in the test data, not in the code; I fixed the data.
  - Nothing else failed.
- **Formatting:** only the files listed by `git diff --name-only`, through `$S/fmt.sh`.
  - Early on, I ran plain `rustfmt` once on files that declare child modules. `git status` afterwards showed only my 9 files modified. No undeclared draft is reachable through a `mod` line, so none could be changed.
- **Binary launches:** each one used temp `OPENIDEA_GPUI_HOME`, `HOME` and `ZDOTDIR`, ran on a temp project in the background, and was stopped by PID. I checked afterwards that it is stopped.

## 2. Part A: the diff and merge viewers draw code as the editor does

**One font resolver.** I moved the body of `Editor::code_font` into `pub fn code_font_of(families, ligatures, weight, cx)` in `app-gpui/src/editor/view.rs`. The editor and all the viewers call it, so there is no second copy of the font logic.

**The model.** `ViewerFont` in `app-gpui/src/vcs_more/viewer_look.rs` gains `weight`. In the webview the viewer's weight comes from `--fw-code`, `src/features/vcs/cm.ts:325`.

**Diff view (`app-gpui/src/vcs_diff/view.rs`).**

- New `CodeStyle::of`, `viewer_line_height`, `default_look`, `listen_to_editor_settings` and `stop_listening`.
- `DiffView` has a `look` and `follow_editor_settings(window)`. The window label is needed because the view reads `platform::settings::effective` for that window.
- It re-reads on `IdeEvent::SettingsChanged` and repaints only when `ViewerLook::follow` reports a change.
- Every former `line_height(cx)`, `code_font(cx)` and `fs-code` site now uses the look, including scrolling, the divider bands and the wheel handler.
- What a pane actually painted with is recorded and exposed as `painted_style(pane)`.

**Merge editor (`app-gpui/src/vcs/merge_view.rs`).**

- Same plumbing as the diff view.
- With "Use block caret" on, the caret is one character wide at 55% of the caret colour. This follows `src/editor/core/cm/theme.ts:45-50`, which `viewerTheme` includes through `caretStyles`.

**Where the look is fed in.**

- `workbench/diff_window.rs::open_diff`
- `workbench/conflicts.rs::open_merge_editor`
- `workbench/diff_tools.rs::build_local_history_tab`, deferred because the workbench label is not readable inside the builder.

**Local History colours.** `workbench/diff_tools.rs::load_history_diff` now runs `side_colours` under `spawn_blocking`, as the diff window does. It sets the colours after the texts, and only while `shown_diff()` still matches. `shown_diff()` is a new accessor in `vcs/local_history_view.rs`.

**Webview sources followed.**

- `src/features/vcs/cm.ts:429-470` (`viewerEditorLook`, `viewerBase`)
- `src/editor/core/cm/setup.ts:178-200` (`fontExtension`)
- `src/features/vcs/cm.ts:323-329` (ligatures off unless `cm-openidea-ligatures`)
- `src/editor/core/cm/prefs.ts:505-516` (caret)

**Tests.**

- `app-gpui/src/workbench/vcs_operations.rs::tests::the_diff_draws_with_the_editors_font_settings_and_follows_them` runs `vcs.showDiff` through the registry on a modified file in a temp Git repo. It asserts that the painted font of each pane has ligatures off, then on with size 17 after the settings are written, then off again.
- `app-gpui/src/workbench/mod.rs::tests::the_merge_editor_draws_with_the_editors_font_and_caret_settings` checks the ligatures painted in all three panes and the block caret look, both following the settings.
- `app-gpui/src/workbench/diff_tools.rs::tests::the_local_history_diff_is_coloured_and_drawn_as_the_editor_draws` checks keyword colours on both sides and that the painted ligatures follow the setting.

**Screenshots of my own binary's window**, in `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/d102/insp/shots/`:

- `off-02-diff-1.png` (default settings): `->`, `+=`, `!=`, `>=` and `&&` are drawn as separate characters.
- `on-02-diff-1.png` (home seeded with `editor.ligatures = true`): `->` becomes an arrow, `!=` becomes `≠`, `>=` becomes `≥`.

**Left out.**

- **Caret blink and smooth motion.** Nothing under `app-gpui/src/editor` reads the caret appearance settings (grep `block_caret|caret_blink` finds none); the editor paints a fixed 2px bar.
- **A caret in the diff panes.** `DiffView` has never painted one. The block-caret look is wired only where a caret is drawn, which is the merge editor.
- **The block caret's painted width.** The merge test asserts `look().caret.block`, not the painted width.
- **Hover and completion popups.** Not changed; see section 5.
- **Terminal.** Not changed: `src/features/terminal/theme.ts` has no ligature handling.

## 2. Part B: edit targets, popup placement, error log sink — not integrated

All five drafts are left undeclared.

- **Edit targets** (`platform/edit_target.rs`, `workbench/edit_actions.rs`) and **popup placement** (`ui/popup_anchor.rs`, `workbench/popup_placement.rs`): out of time. The notes ask for hunks in `actions.rs`, `menu.rs`, `text_field.rs`, `text_area.rs`, `secret_field.rs`, `project_tree.rs`, `services/view.rs`, `workbench/mod.rs`, `code_insight.rs`, `project_view_actions.rs` and `services_window.rs`.
- **Edit menu greying, for whoever integrates it.** `src-tauri/src/menu.rs:1034-1052` greys an item only through `item_enabled(id, disabled)`, the when-clause set. Nothing there depends on the edit target. So the draft's `edit_item_disabled` hunk (notes section A4) should be dropped, not restricted.
- **Error log sink** (`platform/error_log_sink.rs`): no native event matches anything that feeds the webview's error log.
  - The webview's feeders are window `error` and `unhandledrejection` events, `logError` from the React render error boundary (`src/main.tsx:96`), settings page registration and startup (`src/features/settings/index.ts:248,260`), and document sync (`src/editor/core/model/document.ts:271,278`).
  - The notes propose the failed action handler as the counterpart. The webview does not send that to the error log: `src/platform/actions.ts:203-206` sends it to `console.error`.
  - I found no native "Editor resynchronised" or sync-error path either (grep is empty).
  - Declaring the sink would ship code that nothing calls, so I left it undeclared. Wiring it is the owner's decision.

## 3. Inventory rows

- `unit:src/features/vcs/__tests__/viewerFont.test.ts`: stays **done**. Its proof should add `workbench::vcs_operations::tests::the_diff_draws_with_the_editors_font_settings_and_follows_them`, `workbench::tests::the_merge_editor_draws_with_the_editors_font_and_caret_settings` and `workbench::diff_tools::tests::the_local_history_diff_is_coloured_and_drawn_as_the_editor_draws`.
- `unit:src/features/vcs/__tests__/viewerCaret.test.ts`: **partial**, with `workbench::tests::the_merge_editor_draws_with_the_editors_font_and_caret_settings`. Only the block caret is drawn; there is no blink or motion, and the diff panes have no caret.
- `module:src/platform/errorLog.ts` and `unit:src/platform/errorLog.test.ts`: unchanged, **partial**.
- My grep found no inventory rows for `DiffEditor.tsx`, `MergeEditor.tsx`, `LocalHistory.tsx` or `vcs/cm.ts`.

## 4. Hypotheses

- **Hover and completion code text in the webview** probably draws with `--font-mono` and the browser's default ligatures, whatever the setting.
  - Reason: tooltips are parented to `document.body` (`src/editor/core/cm/setup.ts:346-348`). That is outside `.cm-scroller`, where `fontVariantLigatures: none` lives, and outside the element that carries `--openidea-editor-font` and `cm-openidea-ligatures`. `editor_lsp/hover_view.rs::code_font` already draws that way, so I did not change it.
  - To confirm: in the webview, turn ligatures off, hover a symbol whose docs contain `->`, and run `getComputedStyle(document.querySelector('.cm-tooltip code')).fontVariantLigatures`.
- **Before this slice, the diff and merge panes drew at the theme's `fs-code` and `lh-code`.** They now draw at Editor › Font's size and line height, like the editor. On default settings the screenshots show larger text than the d101 diff shot. To confirm, compare `$S/d101/shots/tour-05-diff-1.png` with `$S/d102/insp/shots/off-02-diff-1.png`.

## 5. False or stale notes and briefs

- In `docs/gpui-port/parity.json`, the `viewerFont` and `viewerCaret` rows were marked **done** with only `vcs_more::viewer_look::tests` as proof. Before this slice nothing in the app called `viewer_look`.
- Notes section C3 proposes feeding the sink from `execute_action`'s failure arm. That does not match the webview (`src/platform/actions.ts:203-206`).
- Notes section A4's greying contradicts `src-tauri/src/menu.rs`, which greys by when-clauses only.
- The brief asks for the popups to follow the ligature setting. That conflicts with the webview source (see section 4); I followed the webview.
