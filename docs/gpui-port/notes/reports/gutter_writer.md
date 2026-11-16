I've written the gutter feature as new files and an integration plan, but none of it has been compiled or run (no cargo, as the brief required). 14 end-to-end tests and 6 unit tests are written and none has been run. Two existing tests will need their expected values updated once it lands.

## Files written (all new, untracked)

| File                                                | Lines | What it holds                                                                                                                                                                                                                           |
| --------------------------------------------------- | ----- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/editor/view/gutter_marks.rs`          | 292   | Child of `view.rs`. The change bars and the caret row's gutter tint, painted by the editor itself. Presses come back as a separate `GutterMarkEvent`, the way `AnnotationEvent` does it, so no `EditorEvent` match breaks. 1 unit test. |
| `app-gpui/src/vcs/hunk_popup.rs`                    | 377   | The popup a click on a bar opens: Previous, Next, Rollback, Show Diff, Copy Original Text, Close, the "n of m" counter and the -/+ lines. Enter rolls back, Escape closes. 3 view tests.                                                |
| `app-gpui/src/workbench/editor_gutter.rs`           | 809   | The wiring: fetching hunks and dropping stale answers, bars following edits, the popup's actions, bookmark icons and mnemonics, bookmarks following edits and being stored again, the icon layer. 1 unit test.                          |
| `app-gpui/src/workbench/editor_gutter/run_marks.rs` | 427   | Run marks: producers' targets and `package.json` scripts, found off the window's thread, with their menus. 1 unit test.                                                                                                                 |
| `app-gpui/src/workbench/tests/proofs27_gutter.rs`   | 397   | The 14 end-to-end tests.                                                                                                                                                                                                                |
| `$S/drafts/gutter_integration.md`                   | —     | The notes, sections (a) to (g).                                                                                                                                                                                                         |

The notes contain 22 anchored hunks, grouped in landing order: columns and caret row (`view.rs`), change bars, popup, bookmarks, run marks, tests. Besides `view.rs` they touch `mod.rs`, `branches.rs`, `bookmarks_window.rs`, `vcs/mod.rs`, and change `launch_production` in `run_context.rs` to `pub(super)`. No new action is registered. While splitting the wiring file I briefly created two temporary files of my own in the worktree and then deleted them.

## Rows

| Key                                                                          | Expected       | What is missing                                                                                                                         |
| ---------------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------------------------------------------- |
| `action:navigate.toggleBookmark`                                             | done           | —                                                                                                                                       |
| `action:navigate.toggleBookmarkWithMnemonic`                                 | done           | —                                                                                                                                       |
| `ipc:vcs_line_hunks`                                                         | done           | —                                                                                                                                       |
| `unit:src/features/run/npmGutter.test.ts`                                    | done, 31 of 31 | The model tests already existed; the app now uses them. Each case is mapped to its test in the notes.                                   |
| `point.registerEditorContribution:search.bookmarks`                          | partial        | Icons are drawn only for the active editor.                                                                                             |
| `point.registerEditorContribution:vcs.changeMarkers`                         | partial        | Overview-lane marks, bar tooltips and pointer cursor, inline blame. Show Diff and Copy from the popup are not checked in a window test. |
| `point.registerEditorContribution:run.targets`                               | partial        | Passed/failed test icons, inherited Java `main`s, Go module preload, executor rows, launching from the menu in a test.                  |
| `point.registerEditorContribution:run.npmScripts`                            | partial        | Choosing Run or Debug in a test (it would start `pnpm`).                                                                                |
| `point.registerMenuGroup:vcs.gutter`, `module:src/editor/core/gutterMenu.ts` | still partial  | What these rows lack is about the gutter menu, not this feature.                                                                        |

The inventory has no rows for `gutter.ts`, `HunkPopup.tsx`, `bookmarks/gutter.ts`, `runGutter.ts` or `lanes.ts`; the four contribution rows above are theirs. The full test names for each row are in notes (b).

## Left out, and why

- **Icons in non-active editors:** an editor in a second split pane shows its bars and caret tint, but not its bookmark or run icons. The icons are drawn over the window for the active editor only, as the HTTP and plugin marks already are, because nothing in this crate paints an SVG inside the editor element yet.
- **Not built:**
  - overview-lane marks for hunks;
  - bar tooltips and pointer cursor;
  - passed/failed test icons, because no test-state store is kept by the workbench;
  - inherited Java `main`s, which need the language server;
  - Go module preload;
  - executor rows.
- **Not changed (existing native differences):** the gutter hairline and gutter background are not drawn, and numbers use the `fg-gutter` token where the webview uses `fg-line-number`.
- **Previews:** previews keep a plain gutter, as in the webview, because only tab editors turn the marks on.

## Will break on landing

- **Two existing tests:** `proofs15_folding::the_fold_column_is_drawn_only_while_show_code_folding_outline_is_on` and `editor_extras::…distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text` assert the text starts at numbers + lane + chevrons. Every tab editor now also has the 15 px bookmarks column, plus 14 px for run marks on a source file, as in the webview. Their expected values need those widths added.
- **One wording difference:** a gutter launch with nothing to run shows Run Context's message body, not the webview's gutter body. The fix is described in notes (e).

## Hypotheses

- Hypothesis: the code compiles after small fixes. I read every signature I call; the specific risks are in notes (d): two event types subscribed on one editor, let-else borrows, `Focusable as _` next to the prelude, and `Theme::color` field names.
- Hypothesis: the editor's text is LF-only, so the hunk offsets from the vcs_diff `Doc` built on the editor's text match the editor's own offsets.
- Hypothesis: the test window repaints under `wait_long`, so the painted-row assertions see the bars.
- Hypothesis: `debug_bounds("icon:bookmark")` finds the icon.
- Hypothesis: a standalone `hello.rs` gets a `main` run mark.
- Hypothesis: the backend names the detected script `pnpm run dev`.
- Hypothesis: the popup reopened by Next/Previous lands in the right place without waiting for a frame. I checked that the test window only draws frames on request, so I compute the position from the new scroll instead.
- Hypothesis: the run store changes identity when another project opens in the window, so the run marks re-subscribe.

## Wrong in the brief or in the native code

- The brief says the gutter reserves none of the other columns. It already reserves the 4 px change lane (`CHANGE_LANE`); nothing was painted in it.
- The npm spec row is `[m]` in `INVENTORY.md` but `done` in `parity.json`; the two disagree.
- Icons come from `theme::icon` over `assets/gen/icons.json`, which is what the HTTP run mark uses. There is no `app-gpui/src/ui/icons`.
- `vcs_text_actions.rs` and `vcs_operations.rs` keep no hunk state. They ask the backend afresh on each action and build a throwaway `HunkState`, so there was nothing to share. The gutter keeps its own per editor, fed by the repository's state version for refetches.
