I wrote all three pieces (soft wraps, moved code, diff tabs) as four new files plus exact hunks. Nothing has been compiled or run: per the brief I ran no cargo, edited no existing file, and the advisor's final review timed out. Statements marked "Hypothesis:" were not checked against the code.

## 1. Files written

- `app-gpui/src/vcs_diff/wrap.rs` (300 lines). Pure wrap geometry with 5 plain `#[test]`s.
  - `RowLines`: the visual-line offsets of each row; without wrapping each row is one line, so the existing pixel positions are unchanged.
  - `LineBreaks::measure`: wraps with GPUI's own `LineWrapper`, taken from `&App`. One set of breaks drives both the row heights and the painting.
  - `pane_lines`: a row is as tall as its counterpart. That is the matched unchanged line, or with aligned changes the row of the same index.
  - Also `pieces`, `piece_starts`, `x_at`, `spans_on_lines` and `editor_wraps`.
- `app-gpui/src/vcs_diff/view/moved.rs` (308 lines). A child module of `view.rs`, declared with `mod moved;`, so it can read `DiffView`'s private fields.
  - `move_edits` feeds the row's word marks.
  - `moved_overlays` draws the stripe `diff-moved-{left|right}-{i}`, the edit tooltips `diff-move-edit-{side}-{i}`, and the links `diff-moved-to-{i}` / `diff-moved-from-{i}` with the inner `diff-moved-edited-{i}`.
  - `follow_move` reveals the other side and stops the pane's own mouse-down.
  - 2 tests.
- `app-gpui/src/vcs_diff/tab_view.rs` (173 lines). `DiffTabView::new(data, options, maximized, cx)`.
  - Events: `DiffTabEvent::{Load, Viewer, Menu}`.
  - Accessors: `data()`, `viewer()`, `title()`, `load()`, `loaded()`.
  - 1 GPUI test.
- `app-gpui/src/workbench/diff_tab.rs` (364 lines).
  - The tab kind's `diff_tab_title`, `diff_tab_icon` and `build_diff_tab`.
  - On `Workbench`: `open_diff_tabs`, `diff_tab_viewer`, `diff_tab_focused`, loading, jump to source, and maximize.
  - 3 end-to-end tests.
- Notes: `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/diff_gaps_integration.md`.
  - It holds the `pub mod` and `mod` lines, the declarations of the three pending tests, and hunks V1–V25, T1–T7, G1–G2, W1–W6, plus one each for `tab_views.rs` and `tool_windows.rs`.
  - Hunks are anchored on quoted text. Long functions (`prepaint`, `position_at`, `visible_rows`, `Geometry`) are replaced whole.
- New on `DiffView`: `soft_wraps()`, `set_path()`, `set_maximized()`, `DiffViewEvent::Maximize`, and the commands `DiffCommand::{ToggleSoftWrap, ToggleMaximize}`.
- New in the toolbar: `viewer_settings_menu(options, soft_wraps)`. "Use Soft Wraps" goes after Collapse, checked when the panes actually wrap. `ToolbarState.maximized` adds the `diff-maximize` button.

## 2. Actions

No new action ids.

- `vcs.showDiff` now passes the real `vcs.commit.nonModal` setting and opens `ShownDiffs::Tabs` as tabs. It goes through the same IPC as the diff window: `vcsFileAt`, `fsReadText`, `fsProbe`.
- `diff.nextDifference`, `diff.prevDifference` and `diff.jumpToSource` reach a focused diff tab (W2, W6).
- Maximize Editor runs `ToolWindowLayout::hide_all`, the same thing `toolwindow.hideAll` runs.

## 3. Spec coverage

- `diffToolbar.test.tsx`: 5 of 26 are new.
  - The 3 soft-wrap cases, through the pending file.
  - Case 15 as `maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor`.
  - Case 17 as `restores_the_shared_layout_after_the_diff_tab_is_remounted`.
  - The rest were already covered.
- `movedCode.test.tsx`: the 2 viewer cases, through the pending file. The link and mark half of "omits inner edits in Highlight lines mode" and "respects Do not highlight" is built but has no test.
- `diffDialog.test.tsx`: "opens an editor tab instead…", through the pending file.

None of the three pending tests needed changing. One existing test does: T7 adds "Use Soft Wraps" to the label list in `turns_on_aligned_changes_from_the_settings_menu_off_by_default`.

## 4. Not built, assumptions, likely first failures

**Not built:**

- Diff tabs:
  - no moving along the chain or Compare Next/Previous File;
  - the `diff.showSettings` action does nothing in a tab;
  - working-copy sides can't be edited;
  - tabs are not restored with the session.
  - The Maximize button only updates when the tab is built or the button is pressed.
- Soft wraps:
  - no hanging indent;
  - only the left pane's width is measured, for both panes;
  - the Local History viewer has no file path.
- Moved code: the link is drawn in the code font.

**Assumptions to check first:**

- Hypothesis: `gpui_kit::{IndentAdjustment, LineFragment}` resolve, and `Boundary::ix` is the byte where the next visual line starts.
- Hypothesis: `ShapedLine::cursor().take_until` rebases each piece to x = 0.
- Hypothesis: a `div` with only a tooltip does not block the pane's mouse-down.
- Hypothesis: a `diff` tab view gets built when `open_tab` activates it, as Local History's does.
- Hypothesis: `editor.closeTab` closes a view tab when run through `run_action`.
- Notifying during prepaint is lost because the window doesn't redraw. This one I checked in the GPUI source, so the re-measure uses `window.on_next_frame` instead.

**Most likely to fail first:**

- `soft_wraps_reconfigure_without_losing_the_selection`. Hypothesis: it needs `SettingsChanged` to reach the viewer for both soft-wrap keys.
- `labels_edited_moves_and_highlights_only_their_internal_word_edits`. Hypothesis: it needs the link and edit overlays to land inside the test window's width.
- The `diff_tab.rs` Maximize tests. Hypothesis: they assume the Project tool window is visible right after the project opens.
