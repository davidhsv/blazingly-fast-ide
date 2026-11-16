## Report: ten visual defects in the GPUI editor UI

Seven of the ten defects are fixed. Item 5 (the search field caret) is reported with numbers and not changed. Item 8 already matched the webview, so it only got a test. Item 10 is left as it is. All gates pass on the final code. Nothing is committed.

### 1. Gates (final code, all measured)

**Whole suite, first run.** Log: `$S/dvis/suiteC.log`

- Before: `3:43  up 94 days, 13:13, 5 users, load averages: 8.36 7.31 10.14`
- `test result: ok. 7769 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 195.38s`
- After: `3:46  up 94 days, 13:17, 5 users, load averages: 41.76 21.88 15.59`

**Whole suite, second run, with the twenty reruns beside it.** Log: `.../scratchpad/dvis/suiteD.log`

- Before: `3:47  up 94 days, 13:17, 5 users, load averages: 33.87 21.10 15.43`
- `test result: ok. 7769 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 202.51s`
- After: `3:50  up 94 days, 13:20, 5 users, load averages: 67.05 32.02 20.69`
- The load averages show other work running on the machine. None of it was mine besides these commands.

**Twenty reruns by filter during the second run.** Each test was run with `--exact` on the test binary. Every one passed 20/20 and none failed once. Log: `.../scratchpad/dvis/stress2.log`

- `editor::view::tests::a_breakpoint_is_drawn_in_place_of_its_line_number_centred_on_the_numbers_column`
- `editor::view::tests::the_line_numbers_column_widens_to_the_widest_number_and_its_padding`
- `workbench::http_client::tests::a_run_mark_stands_in_its_own_gutter_column_right_of_the_line_number`
- `workbench::http_client::tests::a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap` (changed)
- `workbench::editor_extras::tests::distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text` (changed)
- `workbench::editor_extras::tests::proofs15_folding::the_fold_column_is_drawn_only_while_show_code_folding_outline_is_on` (changed)
- `vcs_diff::view::tests::a_long_side_label_is_cut_at_its_end_inside_its_half_of_the_header`
- `settings_view::pages::tests::a_help_line_starts_where_its_rows_label_starts`
- `settings_view::pages::languages_sdk::tests::a_path_fields_help_line_starts_where_its_label_starts`
- `workbench::popup_placement::tests::a_menu_and_a_popup_at_the_right_edge_keep_the_window_margin`
- `vcs::selection_history_view::tests::the_revision_rows_are_inset_and_their_lines_keep_apart`
- `workbench::refactor_actions::tests::says_so_when_nothing_applies` (changed)
- `workbench::tests::proofs23_e2e_terminal_groups::the_terminal_window_buttons_show_on_hover_or_keyboard_focus_only`
- `deployment::remote_host_view::tests::a_right_click_selects_the_row_before_its_menu_opens`
- `editor::command_actions::tests::proofs23_right_margin::the_right_margin_is_drawn_over_the_caret_rows_tint`

**Other gates**

- `cargo test --test web_view`: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.13s`
- `cargo clippy --all-targets -- -D warnings`: exit 0, `Finished`.
- `cargo build`: exit 0 with 0 warning lines.

**One failure I caused and fixed.** An earlier whole-suite run (log kept at `.../scratchpad/dvis/suite1-kept.log`) ended `test result: FAILED. 7767 passed; 1 failed`:
`workbench::tests::the_appearance_pages_font_labels_end_in_one_colon_on_their_fields_line` panicked at `src/workbench/mod.rs:8050:9`: `the Appearance page shows its font rows did not happen within thirty seconds (ran out after 43.393423334s; reached later: None)`

- Cause (measured): I had added `.debug_selector("settings-label-…")` to `row_label`, which replaced its existing `form-label:…` selector. GPUI keeps only one selector per element.
- I removed my selector and made my tests use the existing `form-label:` one. The two later full runs pass.

**How each "fails without the fix" was produced.** For each test I put the old code back by an edit (scripts in `$S/dvis/old*.py`), ran the test and saw it fail, then restored by an edit. The failure messages are quoted per item below.

### 2. Per item

**Item 1: gutter icons drawn over the line numbers. Fixed.**

- Webview numbers:
  - Column order is numbers, then contributed icon columns by `order`, then the change lane, then fold (`src/editor/core/cm/setup.ts:357-359`; the lane and fold are `Prec.lowest`).
  - The numbers' cell is `minWidth: 32px`, padding `0 8px 0 10px`, right-aligned (`src/editor/core/cm/theme.ts:226-231`).
  - CodeMirror's `.cm-gutterElement` is `boxSizing: border-box` (`node_modules/@codemirror/view/dist/index.js:7002`), so the column is max(32, widest number + 18). The widest number is all 9s for the line count (`index.js:11869`).
  - Lane 4px (`theme.ts:234`), fold 16px (`theme.ts:256`).
  - HTTP and Spring mark columns are fixed `width:14px` (`src/features/http-client/HttpClient.module.css:2-4`), at orders 56 and 57 (`src/features/http-client/index.ts:123-125`).
  - The plugin actions column is order 58 (`src/features/plugins/index.ts:163`) and sized by its content, a 14px marker (`src/editor/navigation/LineMarkers.module.css`).
  - A breakpoint hides its number (`color: transparent`) and draws a 1em dot centred at `left: calc(50% + 1px)` (`src/features/debugger/editor/editor.module.css:5-22`).
- Native before:
  - The gutter was `w-gutter` 52 plus fold 16. Numbers ended 12px before the fold column.
  - HTTP marks sat at the fold column minus 14, plugin icons at minus 13, so they overlapped the numbers.
  - The breakpoint was a 10px dot at left+4 with the number still drawn.
- Native after:
  - The editor owns ordered icon columns: `Editor::set_gutter_column(id, order, width)` and `Editor::gutter_column(id)`.
  - Numbers column is max(32, digits+18), then the icon columns, a 4px lane, then fold.
  - The HTTP/Spring column (14, order 56/57) is reserved as soon as such an editor opens. The plugin column (order 58) is 14 while the editor has a marker and 0 otherwise.
  - The breakpoint now replaces the number: a 1em dot centred a pixel right of the column's middle.
  - Paint work is unchanged and still covers only the visible rows.
- Files: `app-gpui/src/editor/view.rs`, `app-gpui/src/workbench/http_client.rs` (the "move one icon left" shift is removed), `app-gpui/src/workbench/plugin_contributions.rs`.
- Tests:
  - New: `workbench::http_client::tests::a_run_mark_stands_in_its_own_gutter_column_right_of_the_line_number`. Without the fix it failed: `the mark … x: 324px … is over the number … x: 318.2px … 7.8px`.
  - Tightened: `…a_generate_mark_and_a_plugin_gutter_icon_on_one_line_do_not_overlap` now also checks icon.left − mark.right = 1 and that the mark is not over the number. Without the fix it failed with `left: 3px right: 1px`.
  - New: `editor::view::tests::a_breakpoint_is_drawn_in_place_of_its_line_number_centred_on_the_numbers_column`. Without the fix it failed: `the breakpoint's line shows no number: [0, 1, 2, 3]`.
  - New: `editor::view::tests::the_line_numbers_column_widens_to_the_widest_number_and_its_padding`. Without the fix it failed: `… x: 32.2px … from 0px`.
  - Updated pins: `editor_extras.rs`, the distraction-free test (2 asserts) and `proofs15_folding.rs` changed from `w-gutter` (52) to 32+4 (+fold). This is the webview's number, not a loosening.
- Shots (after the final fix), all under `$S/insp8/shots/`: `dvis4-01-http-light-1.png`, `dvis4-02-spring-light-1.png`, `dvis4-03-gutter-light-1.png`, `dvis4-04-gutter-dark-1.png`, `dvis4-05-http-dark-1.png`. Zoomed crops: `$S/dvis/z4-*.png`.
  - Run and generate marks sit right of each number, and every digit is visible.
  - The breakpoint dot replaces "2".
- Not done:
  - Bookmark icons, mnemonics, VCS change bars and run-producer run marks are not drawn in the native editor at all. Adding them would be a new feature.
  - Their webview columns are not reserved: bookmarks (order 50, 11px + 2+2, always, `src/features/search/index.ts:188-193`, `bookmarks/gutter.ts:157-172`), endpoints (14px in every non-virtual non-http file, `http-client/endpoints.ts:93-134`), run targets (14px, `run/runGutter.ts:197`), colours and LSP markers. So native text still starts left of the webview's in those files.
  - Hypothesis: a fixed `width:14px` column stays at 14 with no markers. Confirm by opening a `.http` file without requests in the webview.
  - The webview tints the caret row across the gutter and shows its number in `--fg-line-number-current`; native does neither (not changed).
  - The plugin column drops to 0 on each edit and comes back after the debounce, because `LanguageVisuals::doc_changed` clears the markers. Hypothesis: the webview does the same (`languageVisuals.ts`: `tr.docChanged ? [] : v`).
  - The execution point was not checked in a shot.

**Item 2: diff side headers cut without an ellipsis. Fixed.**

- Webview: `.label` has `overflow:hidden; text-overflow:ellipsis; white-space:nowrap; font-family:var(--font-mono)`, padding 0 12, gap 8 (`src/features/vcs/DiffEditor.module.css:59-70`). The end is cut. `title=` holds the full label (`DiffEditor.tsx:1238-1256`). Badges use the UI font (`:74-82`).
- Hypothesis: because `.label` is `display:flex` with a bare text node, WebKit may not actually draw the ellipsis there. Confirm by opening a long-path diff in the webview.
- Native before: plain text in an `overflow_hidden` div, UI font, so it was clipped.
- Native after: the text sits in its own `min_w_0().truncate()` child (end ellipsis), the row is mono, badges are UI font, and the label has a tooltip. Same for the unified label. This is the one `DiffView` used by the diff window, the diff tab, embedded diffs and `deployment/sync_preview.rs:99`.
- File: `app-gpui/src/vcs_diff/view.rs`.
- Test: `vcs_diff::view::tests::a_long_side_label_is_cut_at_its_end_inside_its_half_of_the_header`. Without truncation it failed: `diff-label-text-b … 4680px … keeps the padding of … 959.5px`.
- Shot: `$S/insp8/shots/dvis2-08-diff-1.png`. Both headers end in "…" before their "editable" badge, in the mono font.

**Item 3: the help line under a text field starts left of the drop-down's. Fixed.**

- Webview: rows are a grid, `padding:2px 6px; margin:0 -6px` (`src/features/settings/components/Form.module.css:30-39`). `.hint` is `grid-column: 1 / -1`, so it starts at the label (`:66-72`, `Form.tsx:69-83`).
- Native before:
  - Field-row hint at indent 0 while the label sits at the row's 4px padding.
  - Drop-down hint at 4, but 4 even when the row is indented 20.
  - `PathFieldView` (PHPStan file) hint at 0.
- Native after: every hint starts at the label (4, or 20 when indented), using a new `ROW_PADDING` constant and `label_start`.
- Files: `app-gpui/src/settings_view/pages/mod.rs`, `app-gpui/src/settings_view/pages/languages_sdk.rs`.
- Tests:
  - `settings_view::pages::tests::a_help_line_starts_where_its_rows_label_starts`. Without the fix: `left: 0px right: 4px`.
  - `settings_view::pages::languages_sdk::tests::a_path_fields_help_line_starts_where_its_label_starts`. Without the fix: `left: 0px right: 4px`.
- Shot: `$S/insp8/shots/dvis3-05-settings-php-1.png` (crop: `$S/dvis/z-settings.png`). The language-level, include-paths, stubs and PHPStan help lines all start at their label's x.
- Not changed: `settings_view/controls.rs` `draw_option` puts text and drop-down hints at 28, under the label's text rather than at the label. Hypothesis that this differs from the webview the same way.

**Item 4: popups flush at the window's right edge. Fixed for every `PopupMenu` and the anchored popup slot.**

- Webview: `flip({padding:6})`, `shift({padding:6})`, `size({padding:6})` (`src/ui/Popup.tsx:95-98`).
- Native before: `snap_to_window()`, margin 0.
- Native after: `snap_to_window_with_margin(6)` via `WINDOW_MARGIN` in `app-gpui/src/ui/popup_menu.rs` and `app-gpui/src/workbench/popup_placement.rs`. The Agents New Task agent menu goes through `workbench.show_menu` (`workbench/agents_views.rs:137`), so it is covered.
- Test: `workbench::popup_placement::tests::a_menu_and_a_popup_at_the_right_edge_keep_the_window_margin`. Without the fix the menu part failed: `the menu … x: 1519px … 401px … keeps 0px from the right edge`. The popup assertion comes later in the test and was not reached in that run.
- Shot: `$S/insp8/shots/dvis3-02-toolbar-menu-1.png` (main toolbar ⋮ menu). Its right edge is about 6px from the window edge.
- Not changed: `agents/config_pickers_view.rs:834` keeps its own 4px margin. The Agents tool window was not opened.

**Item 5: search field caret over the placeholder. Not changed (no measured geometric difference).**

- Native, from `app-gpui/src/ui/text_field.rs:595-628`: with no text, caret x = placeholder x = the field's text-box left; caret width `px(1.5)`. The crop `$S/dvis/z-search.png` (Search Everywhere) shows the caret touching the left of "T".
- Webview: caret and placeholder start at the content-box left by CSS. The input has no `caret-color`, so the caret is `currentColor` (`--fg-strong`), and the placeholder is `--fg-faint` (`src/ui/Input.module.css:2-19`).
- Hypothesis: WebKit's caret is 1px wide, not 1.5. Confirm with a zoomed shot of a focused empty `Input` in the webview.
- Differences found but not fixed (outside the x/width/x comparison): native placeholder is `fg-muted` versus webview `fg-faint`; native caret uses the `caret` token versus `currentColor`.

**Item 6: History for Selection rows touching; "Nothing here" highlight only label-wide. Fixed.**

- Webview: `rowHeight={38}` (`SelectionHistory.tsx:120`). Rows inset by `--list-inset` 6, `padding:0 8px`, radius `--radius-selection` 5 (`src/ui/List.module.css:38-53`). Entry `gap:2px` (`LocalHistory.module.css:32-37`). Details at `--fs-small` (`SelectionHistory.module.css:15-18`). Body `line-height:1.4` (`src/styles/global.css:33`).
- Native before: no inset, padding 10, both lines at fs-ui with GPUI's default line height. Measured: 21 + 19.5 overflowed the 38px row, so the next row's text started above this row's bottom.
- Native after: inset 6, padding 8, radius-selection, gap 2, details at fs-small, line height 1.4.
- `ListPopup` rows are now `w_full` inside a 4px inset wrapper (the 4px inset is unchanged).
- Files: `app-gpui/src/vcs/selection_history_view.rs`, `app-gpui/src/editor_lsp/intentions_view.rs`.
- Tests:
  - `vcs::selection_history_view::tests::the_revision_rows_are_inset_and_their_lines_keep_apart` (a bare view). Without the fix: `row 0 … x: 0px … is inset in … left: 0px`.
  - `workbench::refactor_actions::tests::says_so_when_nothing_applies` (assertion added). Without the fix: `the row … 140px × 24px … spans the popup … 322px`.
- Shots: `$S/insp8/shots/dvis3-03-history-1.png` (inset rounded rows, smaller details, text inside each row) and `dvis3-04-refactor-nothing-1.png` ("Nothing here" highlight spans the popup).
- Not changed: native colours `fg`/`fg-muted`/`bg-selected` versus the webview's `fg-emphasis`/`fg-subtle`/`bg-tree-selected(-inactive)`.

**Item 7: Terminal header buttons always shown. Fixed.**

- Webview: `.headerActions` (the window's own actions, ⋮ Options, — Hide) has `opacity:0`. It shows on `.window:hover`, `.window:focus-within`, or while its menu is open (`:has([aria-expanded="true"])`) (`src/workbench/toolwindows/ToolWindows.module.css:411-422`, `ToolWindows.tsx:637-690`).
  - The rule is pointer or keyboard anywhere in the tool window, not only its header.
  - Split: only the corner group shows them (`TerminalView.tsx:212`).
  - New Tab and More Terminal Options next to the tabs always show (`TerminalView.tsx:451-478`).
- Native after: `TerminalPanel` tracks hover with `on_hover`. Options and Hide render only while it is hovered or `focus.contains_focused`, in the top-right group's header. New Tab is unchanged.
- Files: `app-gpui/src/terminal/panel.rs`, `app-gpui/src/terminal/panel/groups.rs`.
- Test: `workbench::tests::proofs23_e2e_terminal_groups::the_terminal_window_buttons_show_on_hover_or_keyboard_focus_only` (project window). With the buttons forced on it failed: `neither hovered nor focused  left: (true, true) right: (false, false)`.
- Shots: only header crops are kept, `$S/dvis/dvis3-06-term-away-header.png` (no buttons) and `$S/dvis/dvis3-07-term-over-header.png` (⋮ and — shown). In the shell I typed `PS1='$ '; clear` before the marks and deleted the full shots.
- Not done:
  - "While its menu is open" is not implemented. The buttons may disappear while Options is open if the pointer leaves the tool window.
  - The generic tool window header (`workbench/mod.rs` ~4154) still shows ⋮ and — always, which is the same defect for every other tool window.

**Item 8: right-clicked Remote Host row shows a hover tint. No change needed.**

- Webview: a right mousedown selects the row if it is not already selected (`src/ui/Tree.tsx:579-583`). With focus in the menu the tree shows `--bg-tree-selected-inactive` (`Tree.module.css:72-79`; light `#e9eaee`).
- Native: Remote Host has its own tree, not a shared one. `RemoteHostView::context_click` already selects the row alone (`deployment/remote_host_view.rs:351-363`), and the row uses `bg-tree-selected-inactive` when unfocused. No other tree was changed.
- Hypothesis: the tint seen in note n is that inactive-selection colour (light theme), as in the webview. Confirm by opening a menu on a Remote Host row in each app.
- Test added: `deployment::remote_host_view::tests::a_right_click_selects_the_row_before_its_menu_opens` (a bare view). There was no fix, so it could not fail. No shot: Remote Host needs a server.

**Item 9: the right margin disappears on the caret row. Fixed.**

- Webview: guides are a `background-image` on `.cm-content` (`src/editor/core/cm/prefs.ts:161-171`). The caret row and line tints are a `::before` at `z-index:-1000` (`theme.ts:185-194`), inside `.cm-scroller`'s stacking context (`zIndex:0`, `codemirror index.js:6856-6864`). So the guide paints over the row tints.
- Native before: guides were painted, then the current-line and breakpoint/execution row fills on top.
- Native after: all row tints are painted first, then the guides, then the rest. The paint order is recorded for tests.
- File: `app-gpui/src/editor/view.rs`.
- Test: `editor::command_actions::tests::proofs23_right_margin::the_right_margin_is_drawn_over_the_caret_rows_tint`. Without the fix: `left: [Guides, CaretRow] right: [CaretRow, Guides]`.
- Shots: `$S/insp8/shots/dvis2-03-gutter-light-1.png`, `dvis2-04-gutter-dark-1.png` (crop `$S/dvis/z-margin-04-gutter-dark.png`). The guide is continuous through the caret row and the breakpoint row in dark; I opened the dark crop only, not the light one.

**Item 10: Services tool window tabs on a second row. Not moved.**

- Webview: `headerContent: ServicesTabs` (`src/features/services/index.ts:62`).
- The generic native tool window header (title, spacer, ⋮, —; `workbench/mod.rs` ~4154-4213) has no slot for content between the title and the buttons. The Terminal avoids it by being excluded (`id != TERMINAL_TOOL_WINDOW`) and drawing its own header.
- What it would take, either:
  - a `header_content` view on the tool window declaration that the generic header renders between the title and the buttons, or
  - excluding Services the same way and having its view draw title + tabs + Options/Hide, wired to the existing options and hide events.
- The Services tool window was not opened.

### 3. Inventory rows

None. I did not write to `parity.json`, `INVENTORY.md` or `DECISIONS.md`, and none of these items is a parity row I can name with certainty.

### 4. Hypotheses (with the check that would confirm)

- Fixed-width webview columns keep 14px when empty: inspect a `.http` file without requests in the webview.
- The webview's ellipsis on `.label`: open a long-path diff in the webview.
- WebKit caret width is 1px: zoomed shot of a focused empty `Input`.
- The webview's plugin column collapses on edit: type in a file with plugin icons in the webview.
- The item 8 tint is the inactive selection: right-click a Remote Host row in both apps.
- `controls.rs` 28px hints differ from the webview: compare a schema page in both apps.

### 5. Stale or false in notes or briefs

- `src/editor/api.ts:13-14` says contribution gutters render right of the lane and fold. `setup.ts:357-359` (`Prec.lowest`) puts them left. Unconfirmed: I did not check it in CodeMirror's gutter code.
- Rule 12 of `int_rules.md` (five suite runs) is replaced by this brief's Gates, which I followed.
- HEAD moved during my run from `ae5e0e96` to `40d8c447` (the manager's commit; no `app-gpui` changes). `docs/gpui-port/parity.json` no longer shows as modified. I touched neither.

**Edited files (19, uncommitted):** `deployment/remote_host_view.rs`, `editor/command_actions/tests/proofs23_right_margin.rs`, `editor/view.rs`, `editor_lsp/intentions_view.rs`, `settings_view/pages/languages_sdk.rs`, `settings_view/pages/mod.rs`, `terminal/panel.rs`, `terminal/panel/groups.rs`, `ui/popup_menu.rs`, `vcs/selection_history_view.rs`, `vcs_diff/view.rs`, `workbench/editor_extras.rs`, `workbench/editor_extras/tests/proofs15_folding.rs`, `workbench/http_client.rs`, `workbench/http_client/tests.rs`, `workbench/plugin_contributions.rs`, `workbench/popup_placement.rs`, `workbench/refactor_actions.rs`, `workbench/tests/proofs23_e2e_terminal_groups.rs`, all under `app-gpui/src/`. They were formatted with `$S/fmt.sh` only.

**Shot runs.** The temp projects under `/private/tmp/openidea-inspect-*` are removed. For the history shot that project was given a local git repo (commits by "Test", no global config) so History for Selection had commits. The `run.sh` homes `home-dvis1` to `home-dvis4` are removed. I deleted the shots captured a step late (`dvis1-*`), those blocked by the diff dialog (`dvis2-10…13`), the `dvis3-08` diff shot and the full Terminal shots. All windows were stopped by `run.sh`; no `openidea-gpui` process remains.
