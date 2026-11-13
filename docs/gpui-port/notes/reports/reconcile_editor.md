I wrote one notes file, `$S/drafts/editor_all_integration.md` (69,826 bytes), with the two stages in order. Nothing was compiled or run: anchor counts, greps and the keymap/manifest lookups were run here, and everything about what the code and tests will do comes from reading.

**Hunks**

- **Stage 1 (core):** 46 edits in 7 files. Of the 12 old edits outside `view.rs`, 9 apply as written. Old `mod.rs` hunk 4 is dropped: `editor.closeAllTabs` is already in the build. Old hunk 5 and the `tab_views.rs` hunk became one: `install_editor_core` now goes inside `install_intelligence`, away from the `if still_open {` lines the compiling agent and plugin stage 3.4.5 edit. All 34 `view.rs` edits were re-derived against today's file. The whole-function `prepaint` replacement is now four small anchored hunks.
- **Stage 2 (language):** 13 edits in 6 files. Old sections 3b and 4 are dropped: commit `e22ffae7` already runs a quick fix's submenu command and made `apply_inspection` `pub(super)`. One hunk is new: it makes the fake server's capabilities extendable from the test's answers file.
- All 57 anchors occur exactly once in the working tree, re-counted after the last edit. Every text to replace in the notes counts 1 and every text to add counts 0.

**Line↔pixel conversions in `view.rs`:** 34 edit sites (33 in the code plus the view's own test), listed with a hunk each. They cover painting, hit-testing, scrolling, caret movement up/down/page, gutter, breakpoint and execution marks, diagnostics squiggles, selections including the new column selection, and highlight spans. The native editor paints no inlay hints, so there is no site for them. The original draft missed 6:

- `block_point_at` and the ⌥-drag column selection (both new since `e22ffae7`)
- triple-click on a folded line
- the painted row count
- `find_bar.rs` `select_in`
- `plugin_contributions.rs` `plugin_gutter_layer`

**Edits to drafted files** (final copies in `$S/ed/files/app-gpui/src/…`; the notes say to copy from there if `git status` shows a draft unmodified):

- **`editor_core.rs`:** removed `copy_reference` and `project_root`, which would not compile (both duplicate methods already in `project_files.rs` and `mod.rs`). Removed `close_all_tabs` and its test, pruned unused imports, and added negative key presses.
- **`core_actions.rs`:** public `row_of_line` / `line_of_row` / `row_count`, and the scroll is clamped after folding (Collapse All while scrolled down painted nothing).
- **`editor_language.rs`:** removed `run_intention_control`, the duplicate quick-fix-menu test and the test-time rewrite of the server script. Wrapped two lines over 120 columns and added a test that the language keys do nothing while a tool window has the keyboard.

**Removed because it now exists:** Close All Tabs, Copy Reference (the draft's registration, method and test steps) and the quick-fix submenu control.

**Not verified:**

- Whether tree-sitter's Rust fold ranges are what the fold tests expect.
- How GPUI parses keys such as `cmd--` and `alt-shift-.`. The test preset is the macOS one; on other hosts the keys differ.
- That the server sees `didOpen` before the `*File` actions' requests (read in `ide-lsp`, not run).
- That `lsp_prepare_rename` answers `None` for a `null` result.
- Signatures in files I did not re-read (`paste_history_view.rs`, `folds.rs` and the models they call). None of them changed since the drafts were written.
- Webview sources beyond the generated manifest and two excerpts.
- The adapters behind `cancel_request` and `doc_edit`.
- Keys never pressed by a test: ⌃I, ⌃O, ⇧F6, ⌥Space, ⌘Y, ⇧⌘T, ⌃⇧↑, ⇧F2, ⌘⌥Y, ⇧⌘⌫.

**Not applied, offered:** ⌘Z in the editor does not reach the refactoring undo (Edit › Undo does). An optional untested hunk is in section 2.7.

**Anchors the compiling agent is likely to touch:**

- the 11 anchors in `workbench/mod.rs`; re-run `$S/ed/anchors.py` just before pasting
- `view.rs` `Prepared` and `paint` (V8a, V9b–V9d), if their gutter layer lands
- hunk 1.7 in `plugin_contributions.rs`, which plugin stage 3 declares
