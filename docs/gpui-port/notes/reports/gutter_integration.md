# Gutter: change bars, hunk popup, bookmarks, run marks, caret row — integration notes

Files written (all new, untracked, none declared yet):

- `app-gpui/src/editor/view/gutter_marks.rs` (292 lines): child module of `editor/view.rs`. `LaneMark`, `GutterMarkEvent` (`LanePressed { line, position }`, `BookmarkToggled { line }`, a second `EventEmitter` of `Editor` like `AnnotationEvent`), `Editor::{set_gutter_marks_enabled, gutter_marks_enabled, set_lane_marks, lane_marks, lane_mark_of_row, change_lane}`, the paint helpers `GutterRows`/`GutterRow`/`paint_gutter_row`/`lane_bounds`/`current_number_color`, the press hook `press_gutter_marks`, test accessor `painted_gutter_rows`. 1 model test.
- `app-gpui/src/vcs/hunk_popup.rs` (377 lines): `HunkPopup` view (`new(HunkPopupData, cx)`, events `HunkPopupEvent::{Previous, Next, Rollback, ShowDiff, Close}`, accessors `data`, `title`, `lines`, `toolbar`, `is_copied`, `press`), `HunkPopupData::of`, `kind_title`. 3 view tests.
- `app-gpui/src/workbench/editor_gutter.rs` (786 lines): `EditorGutterSlot`, following editors, change bars (fetch / map / drop stale), popup wiring, bookmarks (draw, follow edits, store moved lines), the overlay layer. 1 model test.
- `app-gpui/src/workbench/editor_gutter/run_marks.rs` (418 lines): child of `editor_gutter.rs` (`mod run_marks;` is already inside it). Producers' targets and `package.json` scripts: detection off the window's thread, the menus, their commands. 1 model test.
- `app-gpui/src/workbench/tests/proofs27_gutter.rs` (397 lines): 14 end-to-end tests in a project window.

No new action is registered: every action these tests drive already exists (`navigate.toggleBookmark`, `navigate.toggleBookmarkWithMnemonic`, `vcs.showDiff`, `run.runContext`, `run.debugContext`, `run.modifyContext`, `run.runConfig`, `run.debugConfig`).

## (a) Hunks for existing files, in order

Anchor = the quoted lines just before the insertion, copied from the file. `view.rs` is anchored on lines inside `Editor::with_text`, `Editor::mouse_down`, `EditorElement::prepaint` and `EditorElement::paint`, which the syntax tree commands and code preview integrations do not rewrite (they add fields, methods and actions elsewhere); if one of them reorders the field list, put the new field anywhere in `struct Editor` and its initializer anywhere in the `Self { … }` literal of `with_text`.

### Group 1: columns and the caret row (view.rs; compiles alone with `gutter_marks.rs`)

1. `app-gpui/src/editor/view.rs`, module list. After:
   ```rust
   pub mod annotations;
   ```
   add:
   ```rust
   pub mod gutter_marks;
   ```
2. `struct Editor`, after:
   ```rust
       /// Blame annotations shown in the gutter's first column ([`annotations`]).
       annotations: Option<annotations::Annotations>,
   ```
   add:
   ```rust
       /// The change bars and the caret's gutter tint the workbench turns on ([`gutter_marks`]).
       gutter_marks: gutter_marks::GutterMarks,
   ```
3. `Editor::with_text`, struct literal, after (unique in the file):
   ```rust
               annotations: None,
   ```
   add:
   ```rust
               gutter_marks: Default::default(),
   ```
4. `Editor::mouse_down`, after (the pair is unique; the second line alone occurs twice):
   ```rust
           if let Some(line) = self.gutter_line_at(event.position) {
               (self.drag_anchor, self.adding) = (None, None);
   ```
   add (it must come before `press_annotation` and `in_fold_column`: a press on the lane reaches 3 px into the chevrons, and ⌘-click on the blame column toggles a bookmark):
   ```rust
               // A press on a change bar opens its change; ⌘-click toggles the line's bookmark.
               if self.press_gutter_marks(event, line, cx) {
                   return;
               }
   ```
5. `EditorElement::prepaint`, after:
   ```rust
           let marker_color = theme::color(cx, "fg-fold-marker");
   ```
   add:
   ```rust
           // The numbers of the lines the ranges' moving ends are on are drawn in the stronger colour.
           let caret_lines = editor.gutter_caret_lines();
           let current_number_color = gutter_marks::current_number_color(cx);
   ```
6. `EditorElement::prepaint`, replace:
   ```rust
               let label = (line + 1).to_string();
               numbers.push(window.text_system().shape_line(
                   label.clone().into(),
                   font_size,
                   &[run(label.len(), number_color)],
                   None,
               ));
   ```
   with:
   ```rust
               let label = (line + 1).to_string();
               let color = if caret_lines.binary_search(&line).is_ok() { current_number_color } else { number_color };
               numbers.push(window.text_system().shape_line(
                   label.clone().into(),
                   font_size,
                   &[run(label.len(), color)],
                   None,
               ));
   ```
7. `EditorElement::paint`, after:
   ```rust
           let focused = focus.is_focused(window);
   ```
   add:
   ```rust
           // The lane mark of each painted row: one lookup per row.
           let gutter_rows = self.editor.read(cx).gutter_rows(&layout.line_of);
   ```
8. `EditorElement::paint`, after:
   ```rust
           #[cfg(test)]
           let mut paint_order = Vec::new();
   ```
   add:
   ```rust
           #[cfg(test)]
           let mut painted_gutter = Vec::new();
   ```
9. `EditorElement::paint`, first row loop, after the breakpoint tints:
   ```rust
                   for kind in [MarkKind::BreakpointLine, MarkKind::UnverifiedBreakpoint, MarkKind::ExecutionLine] {
                       if marks.iter().any(|mark| mark.kind == kind && touches(mark.range.start, mark.range.end)) {
                           window.paint_quad(fill(row_area, mark_color(cx, kind)));
                           #[cfg(test)]
                           painted_marks.push((kind, row_area));
                       }
                   }
   ```
   add (still inside the `for (index, line) in layout.line_of…` loop, before its closing brace):
   ```rust
                   // The caret's tint across the gutter and the change bar, under the numbers and icons.
                   if gutter_rows.enabled {
                       let row = gutter_marks::GutterRow {
                           left: bounds.left(),
                           gutter: *gutter,
                           fold_column,
                           top,
                           line_height,
                           caret_row: heads.iter().any(|head| head.0 == line),
                           lane: gutter_rows.lane.get(index).copied().flatten(),
                       };
                       gutter_marks::paint_gutter_row(&row, window, cx);
                       #[cfg(test)]
                       painted_gutter
                           .push((line, row.caret_row, row.lane.map(|lane| (lane, gutter_marks::lane_bounds(&row, cx)))));
                   }
   ```
10. `EditorElement::paint`, replace the last statement:
    ```rust
            self.editor.update(cx, |editor, _| editor.layout = Some(layout));
    ```
    with:
    ```rust
            self.editor.update(cx, |editor, _| {
                editor.layout = Some(layout);
                #[cfg(test)]
                {
                    editor.gutter_marks.painted = painted_gutter;
                }
            });
    ```

Per frame: hunk 5 walks the selections once (sorted, deduplicated); hunk 6 is one binary search per painted row; hunk 7 is one `partition_point` over the lane marks per painted row (plus one `line_of_row` for the row's end); hunk 9 walks `heads` per painted row (as the text tint already does). The workbench's overlay layer (hunk 15, `editor_gutter_layer`) runs two `partition_point`s over the active editor's bookmarks and two over its run marks (both kept sorted by line), then visits only the marks of the painted lines. Nothing walks the document, the hunks or the whole bookmark list; nothing asks the backend from paint or render.

### Group 2: change bars (workbench)

11. `app-gpui/src/workbench/mod.rs`, module list. After:
    ```rust
    mod editor_find;
    ```
    add:
    ```rust
    mod editor_gutter;
    ```
12. `struct Workbench`, after:
    ```rust
        bookmarks: BookmarkStore,
    ```
    add:
    ```rust
        /// What feeds the editors' gutters: change bars, bookmarks, run marks ([`editor_gutter`]).
        editor_gutter: editor_gutter::EditorGutterSlot,
    ```
13. `Workbench::new` initializer, after:
    ```rust
                bookmarks: BookmarkStore::default(),
    ```
    add:
    ```rust
                editor_gutter: Default::default(),
    ```
14. The block that hands a newly opened editor to the features (`watch_debug_editor`, `watch_plugin_gutter`, `http_editor_opened`, inside `if still_open {`), after:
    ```rust
                                workbench.http_editor_opened(&editor, window, cx);
    ```
    add:
    ```rust
                                workbench.editor_gutter_opened(&editor, window, cx);
    ```
15. `Render for Workbench`, after:
    ```rust
                .children(self.http_gutter_layer(cx))
    ```
    add:
    ```rust
                .children(self.editor_gutter_layer(cx))
    ```
16. `app-gpui/src/workbench/branches.rs`, `vcs_refreshed`, after (unique):
    ```rust
            self.changelists_follow_status(cx);
    ```
    add:
    ```rust
            self.editor_gutter_vcs_changed(cx);
    ```
17. `app-gpui/src/workbench/mod.rs`, `IdeEvent::SettingsChanged` arm, after:
    ```rust
                    self.deployment_settings_changed(cx);
    ```
    add:
    ```rust
                    self.editor_gutter_settings_changed(cx);
    ```

### Group 3: the hunk popup

18. `app-gpui/src/vcs/mod.rs`, after:
    ```rust
    pub mod graph;
    ```
    add:
    ```rust
    pub mod hunk_popup;
    ```

(Groups 2 and 3 must land together: `editor_gutter.rs` uses the popup. To land the bars without the popup, declare `hunk_popup` anyway; it has no other dependency.)

### Group 4: bookmarks

19. `app-gpui/src/workbench/bookmarks_window.rs`, `sync_bookmarks_window`, after:
    ```rust
        pub(super) fn sync_bookmarks_window(&mut self, cx: &mut Context<Self>) {
    ```
    add (first line of the body, before the early return when the tool window is not shown; every change made in `mod.rs` — Toggle Bookmark, the digits, file bookmarks, the Bookmarks popup, the mnemonic chooser — calls this):
    ```rust
            self.editor_gutter_bookmarks_changed(cx);
    ```
20. Same file, `BookmarksWindowEvent::Changed` arm, after:
    ```rust
                    self.bookmarks = view.read(cx).store().clone();
                    Self::save_bookmark_writes(writes, cx);
    ```
    add:
    ```rust
                    self.editor_gutter_bookmarks_changed(cx);
    ```

### Group 5: run marks

21. `app-gpui/src/workbench/run_context.rs`, make the launch reachable from the gutter's choice rows. Replace:
    ```rust
        fn launch_production(
    ```
    with:
    ```rust
        pub(super) fn launch_production(
    ```

### Tests

22. `app-gpui/src/workbench/mod.rs`, `mod tests`, after:
    ```rust
        mod proofs23_e2e_terminal_groups;
    ```
    add:
    ```rust
        mod proofs27_gutter;
    ```

### Click targets after these hunks (x from the editor's left edge, left to right)

| x range                                                                                                                                                              | what a left press does                                                                                                                                |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------- |
| blame column (when shown)                                                                                                                                            | ⌘/Ctrl: toggles the line's bookmark (new); otherwise as today: the blame column's own press                                                           |
| numbers column                                                                                                                                                       | ⌘/Ctrl without Shift/Alt: toggles the line's bookmark (new); otherwise as today: `GutterPressed` → set / clear a breakpoint                           |
| bookmarks column, 15 px (order 50)                                                                                                                                   | on a drawn bookmark: ⌘/Ctrl toggles it, a plain press does nothing (overlay takes it); empty cell: as the numbers column                              |
| run column, 14 px (order 55, sources the producers know)                                                                                                             | on a drawn mark: opens its menu at the mark's bottom-left + 2 px (overlay); empty: as the numbers column                                              |
| HTTP column 14 px (56/57), plugin icons 14 px (58, set first when an editor opens), npm scripts column 14 px (58, set after the plugins' so it stands right of them) | unchanged; an npm mark opens its menu                                                                                                                 |
| last pixel of the icon columns + lane 4 px + first 3 px of the chevrons                                                                                              | plain press on a row with a lane mark: opens the change's popup (new); otherwise unchanged (breakpoint left of the chevrons, fold toggle inside them) |
| fold chevrons, 16 px                                                                                                                                                 | ⌘/Ctrl falls through (no bookmark); plain: toggles the fold, except its first 3 px on a row with a lane mark                                          |

## (b) Parity rows

| Key                                                  | Expected                   | Proving test(s)                                                                                                                                                                                                                                                                                                                             | What is missing                                                                                                                                                                                                   |
| ---------------------------------------------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `action:navigate.toggleBookmark`                     | done                       | `workbench::tests::proofs27_gutter::toggle_bookmark_draws_the_bookmark_icon_on_the_carets_line_in_the_bookmarks_column` (with the existing `go_to_line_and_bookmarks_move_the_caret_through_the_project`)                                                                                                                                   | —                                                                                                                                                                                                                 |
| `action:navigate.toggleBookmarkWithMnemonic`         | done                       | `workbench::tests::proofs27_gutter::toggle_bookmark_with_mnemonic_draws_the_chosen_digit_on_the_carets_line` (with `proofs5_navigation::toggle_bookmark_with_mnemonic_gives_the_caret_line_the_typed_digit`)                                                                                                                                | —                                                                                                                                                                                                                 |
| `ipc:vcs_line_hunks`                                 | done                       | `workbench::tests::proofs27_gutter::the_change_lane_draws_bars_on_added_and_modified_lines_and_a_notch_where_lines_were_deleted`, `…::the_bars_follow_an_edit_at_once_and_the_backends_next_answer_adds_the_typed_line`, `…::an_untracked_file_is_all_added_lines`                                                                          | — (the bars can only come from `ide_host::ops::vcs::vcs_line_hunks(&host, label, path)`, which `src-tauri/src/commands/vcs.rs:38` calls with the same arguments)                                                  |
| `point.registerEditorContribution:vcs.changeMarkers` | partial                    | the three above, `…::a_press_on_a_bar_opens_its_changes_popup_and_rollback_puts_the_original_line_back`, `…::next_change_in_the_popup_moves_the_caret_and_shows_the_next_changes_popup`, `…::a_file_outside_any_repository_has_no_bars`                                                                                                     | the overview lane's hunk marks (`scrollMarks`), the bars' tooltips and pointer cursor, the inline blame of the same contribution (`inlineBlame`, `blameSlot`), Show Diff and Copy from the popup in a window test |
| `point.registerEditorContribution:search.bookmarks`  | partial                    | `…::toggle_bookmark_draws_…`, `…::toggle_bookmark_with_mnemonic_draws_…`, `…::a_primary_click_in_the_gutter_toggles_the_lines_bookmark`, `…::a_bookmark_follows_its_line_when_a_line_is_inserted_above_it`, `…::with_gutter_icons_off_the_bookmarks_and_run_columns_take_no_room`                                                           | the icons are drawn for the active editor only: an editor of a second split pane keeps the column but shows no icon (see (c))                                                                                     |
| `point.registerEditorContribution:run.targets`       | partial                    | `…::a_main_gets_a_run_mark_whose_menu_offers_to_run_and_to_modify_its_configuration`                                                                                                                                                                                                                                                        | test-result icons (passed / failed), inherited `main`s (language server), Go module preload, the executors' rows, a launch from the menu in a window test                                                         |
| `point.registerEditorContribution:run.npmScripts`    | partial                    | `…::a_package_json_script_gets_a_run_mark_whose_menu_runs_it_with_the_lockfiles_manager`                                                                                                                                                                                                                                                    | choosing Run / Debug in a window test (it would start `pnpm`)                                                                                                                                                     |
| `unit:src/features/run/npmGutter.test.ts`            | done (31 of 31; was `[m]`) | the existing `run_producers::npm_gutter::tests::*`, one per case (mapping below), now reachable from the app through `editor_gutter/run_marks.rs` (`script_entries`, `ScriptMenuFlow`, `ScriptMenu::items`); reachability proven by `…::a_package_json_script_gets_a_run_mark_…`, which replays the last `scriptMenuItems` case in a window | —                                                                                                                                                                                                                 |
| `point.registerMenuGroup:vcs.gutter`                 | partial (unchanged)        | —                                                                                                                                                                                                                                                                                                                                           | the Annotate with Git Blame label while no annotations are shown; not this feature                                                                                                                                |
| `module:src/editor/core/gutterMenu.ts`               | partial (unchanged)        | —                                                                                                                                                                                                                                                                                                                                           | the soft-wrap row, the switches' writes and the Configure rows; not this feature                                                                                                                                  |

npmGutter.test.ts cases → native tests (all in `run_producers::npm_gutter::tests`, counted from the spec file: 14 + 3 + 7 + 7 = 31): scriptEntries: lists scripts with their lines in document order → `lists_scripts_with_their_lines_in_document_order`; parses names with escapes and values containing braces or colons → `parses_names_with_escapes_and_values_containing_braces_or_colons`; ignores scripts nested in other objects → `ignores_scripts_nested_in_other_objects`; tracks lines through object, array and scalar values → `tracks_lines_through_object_array_and_scalar_values`; returns nothing without a scripts key → `returns_nothing_without_a_scripts_key`; returns an empty scripts object as no entries → `returns_an_empty_scripts_object_as_no_entries`; ignores a scripts value that is not an object → `ignores_a_scripts_value_that_is_not_an_object`; keeps entries parsed before malformed text → `keeps_entries_parsed_before_malformed_text`; tolerates a trailing comma → `tolerates_a_trailing_comma`; skips a BOM → `skips_a_bom`; tracks CRLF lines after non-ASCII and astral characters → `tracks_crlf_lines_after_non_ascii_and_astral_characters`; decodes escaped unicode and backslashes in keys and values → `decodes_escaped_unicode_and_backslashes_in_keys_and_values`; produces no entries for scripts nested in an object or array → `produces_no_entries_for_scripts_nested_in_an_object_or_array`; skips long numeric metadata before scripts → `skips_long_numeric_metadata_before_scripts`. scriptPackageManager: prefers the declared %s over lockfiles (it.each of 4) → `prefers_the_declared_manager_over_lockfiles` (loops over the same 4, read); uses %s when no manager is declared (it.each of 5) → `uses_the_lockfile_when_no_manager_is_declared` (loops over the same 5 pairs, both assertions, read); uses backend lockfile precedence and defaults to npm → `uses_backend_lockfile_precedence_and_defaults_to_npm`. scriptMenuItems: uses the package lockfile for an undetected script → `uses_the_package_lockfile_for_an_undetected_script`; inherits the backend's workspace manager from a sibling script → `inherits_the_backends_workspace_manager_from_a_sibling_script`; does not inherit an unrelated package's manager or a custom executable → `does_not_inherit_an_unrelated_packages_manager_or_a_custom_executable`; uses root detection for a workspace member's first script → `uses_root_detection_for_a_workspace_members_first_script`; uses local detection for a polyglot package without a root manifest → `uses_local_detection_for_a_polyglot_package_without_a_root_manifest`; preserves a matching user configuration → `preserves_a_matching_user_configuration`; renders a native button and opens its menu on keyboard-style activation after Unicode text → `renders_a_native_button_and_opens_its_menu_on_keyboard_style_activation_after_unicode_text` (model) and `workbench::tests::proofs27_gutter::a_package_json_script_gets_a_run_mark_whose_menu_runs_it_with_the_lockfiles_manager` (window). scriptConfigFor: matches the root package's detected id → `matches_the_root_packages_detected_id`; matches a workspace member by its package name or directory → `matches_a_workspace_member_by_its_package_name_or_directory`; falls back to npm run args in the same directory → `falls_back_to_npm_run_args_in_the_same_directory`; matches an unnamed nested member's overridden config by its full relative directory → `matches_an_unnamed_nested_members_overridden_config_by_its_full_relative_directory`; prefers an id match over an args match → `prefers_an_id_match_over_an_args_match`; finds nothing in another directory → `finds_nothing_in_another_directory`; derives the backend configuration ids → `derives_the_backend_configuration_ids`. The bodies of the model tests were not compared case by case against the spec beyond the two `it.each` loops; Hypothesis: they check the same things (they are the row's existing proof).

The inventory has no `module:` rows for `src/features/vcs/gutter.ts`, `HunkPopup.tsx`, `src/features/search/bookmarks/gutter.ts`, `src/features/run/runGutter.ts` or `src/editor/core/cm/lanes.ts`: their rows are the four `registerEditorContribution` rows above. `unit:src/features/vcs/__tests__/hunks.test.ts` and `unit:src/features/search/__tests__/bookmarks.test.ts` are already done.

## (c) Behaviours of the webview, built / left out

Columns and caret row

- `src/editor/core/cm/setup.ts:357-360` order numbers | contributed icon gutters | VCS lane | fold chevrons, `highlightActiveLineGutter()`: built (the lane already sat there; contributions order by their `order`: bookmarks 50, run 55, HTTP 56/57, npm 58, plugins 58).
- `src/editor/core/cm/theme.ts:232-233` caret row's gutter tint `--editor-caret-row` and the number in `--fg-line-number-current` (fallback `--fg-muted`): built; it follows every range's head, selected or not (`@codemirror/view` `activeLineGutterHighlighter`, read in `node_modules`), and only in editors the workbench follows.
- `theme.ts:234-255` lane 4 px, bar 3 px with right corners rounded 1 px, deleted notch a 6×6 triangle centred on the line's top: built (corners not rounded: the bar is a plain quad).
- `theme.ts:166` `hideGutter-run` / `hideGutter-bookmarks` (`prefs.ts:439-546`: Show gutter icons, `gutterIconsDisabled`): built (width 0); `inheritors`/`supers`/`colors` groups: left out (their gutters do not exist natively).
- `theme.ts:216-222` the gutter's background `--bg-editor-gutter` and the 1 px `--border-gutter` hairline: left out (existing native behaviour, not asked; adding the hairline moves every x the existing tests measure). The numbers use `fg-gutter`, the webview `--fg-line-number`: left as it is for the same reason; flagged.
- `src/editor/core/cm/lanes.ts:10-13` `changeLane` gutter from the `gutterLaneMarkers` facet: built in the editor (`set_lane_marks`). `lanes.ts:15-327` the overview lane over the scrollbar: not this feature, left out.

Change bars (`src/features/vcs/gutter.ts`)

- `:29-34` hunks to offsets, `:54-69` markers per line, deleted as notch: native `HunkState::set_hunks` / `build_markers`, used.
- `:36-46` colours `--gutter-added/modified/deleted`, titles "… lines (click to show)": colours built; titles (tooltips) left out (no tooltip on painted quads).
- `:71-88` mapping through edits: built (`map_through` on every `EditorEvent::Edited`; the `Doc` of the new text is built only while the file has hunks).
- `:94-136` a folded line shows the marker of a line it hides, and its click opens that hunk: built (the row's mark is the first among the offsets it shows; the click uses `hunk_at_pos` with the editor's folds).
- `:155-208` fetch 30 ms after open, 60 ms after the VCS store's version / availability changes, 300 ms after an edit; stale answers dropped; unavailable → clear; errors keep the markers: built with `HunkFetcher` (`VcsState.version` read in `vcs_refreshed`).
- `:210-217` overview marks: left out.
- `:228-242` lane click: left button, no modifier, x within lane −1…+3, a hunk on that line: built.
- `:255` virtual documents get none: built. Large files: the webview has no limit (`gutter.ts` none, `ide-vcs` `line_hunks` none: read `crates/ide-vcs/src/lib.rs:806-831`); none added. Untracked: the backend answers all-added unless ignored: shown as answered.
- `:267-318` Next/Previous Change and Rollback Lines: existing native actions (`vcs_text_actions.rs`, `vcs_operations.rs`), untouched; they still ask the backend on each run (they cannot reuse the gutter's state without editing them; their behaviour is the same).

Hunk popup (`HunkPopup.tsx`, `HunkPopup.module.css`)

- `:42-51` anchored at the lane's right edge, below the hunk's last line (top of the line for a deletion), `bottom-start` + 2 px: built.
- `:56-58` closes when that editor's document changes: built.
- `:87-96` Rollback replaces the live hunk by its old text, closes, focuses the editor: built (carets mapped through the change).
- `:98-102` Previous/Next: `gotoChange` then reopen on the next frame, close without a target: built; the popup reopens at once, its place computed from the scroll the move left (`painted_bounds().top() + line_height × row − scroll_top`), so no frame is waited for.
- `:115-152` buttons and titles (arrow-up/down with the `vcs.prevChange`/`vcs.nextChange` shortcuts in the tooltip, undo, diff, copy→check, disabled without old text, the title with "n of m" only when >1, close): built.
- `:129` `data-autofocus` on Rollback: built as Enter = Rollback, Escape = Close.
- `:136` Show Diff runs `vcs.showDiff { path, line }`: built, not asserted by a window test.
- `:154-172` body `-` / `+` lines, tints 14 % / 16 % of the gutter colours, mono font, max height 280: built.

Bookmarks (`src/features/search/bookmarks/gutter.ts`, `index.ts:188-193`)

- `:15-50` the icon (11 px, accent stroke) or the mnemonic chip (min 11×12, accent border, `--accent-soft`, 9 px semibold), spacer keeps the column 15 px wide without bookmarks: built (icon `bookmark` from `assets/gen/icons.json`).
- `:59-66` marks of lines past the end dropped, one per line: built.
- `:81-126` ⌘/Ctrl-click without Shift/Alt anywhere in the gutter but the fold column toggles via `navigate.toggleBookmark { path, line }`: built (the toggle itself, `Workbench::toggle_bookmark`, as the action does).
- `:93,127-146` marks follow edits (`MapMode.TrackBefore`: a mark whose preceding character is deleted goes) and the moved lines are written back with `setFileLines`: built (`mapped_line_start(…, true)`, `set_file_lines`).
- `:104-115` follow changes made elsewhere: built through `sync_bookmarks_window` / the Bookmarks tool window's `Changed`.

Run marks (`src/features/run/runGutter.ts`, `npmGutter.ts`)

- `runGutter.ts:31-32,199` files with gutter targets, a project root, not virtual: built. `:200-201` Go modules preload: left out. `:250-261` inherited Java `main`s through the language server: left out (needs the LSP plumbing, `ContextInput.inherited` stays `None`).
- `:204-208` JVM tests left to a non-Maven/Gradle build: built. `:225-227` re-detect when the configurations load or change: built (observer, compared by ids).
- `:43-61` targets follow edits, one per line: built. `:195,233-235` 350 ms debounce: built.
- `:97-128` menu: Run/Debug (submenus per configuration when several), executors, separator, Modify Run Configuration…: built through `target_menu_items` with no executors (none is registered natively). `:67-83` a launch with nothing produced says "Nothing to run here" with body "No run or test configuration of the project matches this file.": native `run_context` says the same title with Run Context's body ("The active file has no runnable element, test or run configuration."): difference left, see (e).
- `:138-147` test-state icons (passed/failed): left out (no `TestStates` global is fed in the workbench); `:143-147` suite icon `gutter-run-all`: built.
- `:168-169` tooltip "Run 'name'" / "Run test 'name'"…: built.
- `npmGutter.ts:233-293` the menu flow (sibling manager, root manifest, lockfile, labels, `run.runConfig` / `run.debugConfig`): built on the existing `ScriptMenuFlow`, I/O through `fs_read_text` / `fs_read_dir` off the thread. `:369-371` 300 ms debounce: built.

Which editors get these columns (`src/editor/core/cm/preview.ts:19-42`): the Find in Files / Bookmarks / code previews install only line numbers and highlighting, no contributions, no active-line gutter. Native: only editors opened as tabs (`editor_gutter_opened` from the editor-opened path) turn the marks on; `ui/code_preview.rs` editors never call it, so they keep a plain gutter (they still reserve the existing 4 px lane, as before).

The icons (bookmarks, run marks) are drawn over the window for the active editor only, as the HTTP and plugin marks already are; a second split pane's editor shows its bars and caret tint (painted by the editor) but not its icons. Hypothesis: acceptable for now; settling it needs icons painted inside `EditorElement` (no SVG is painted inside an element in this crate yet).

## (d) Hypotheses (not settled without a compiler)

- Hypothesis: `subscribe_in` with two event types of the same `Editor` entity infers each from the closure's annotation (as `annotate.rs` does with `AnnotationEvent`). Settle: compile.
- Hypothesis: `let Some(x) = self.map.get(..).method() else { return self.mutating(..) };` compiles (the else branch holds no borrow). Used in `step_hunk`, `roll_back_hunk`. Settle: compile; if refused, compute an `Option<(u32, u32)>` first.
- Hypothesis: `editor.folded_regions()` (in `editor/core_actions.rs`) is a `pub fn` on `Editor` returning offsets `(from, to)` of the hidden text, the same convention `HunkState::hunk_at_pos` takes. Settle: read `editor/core_actions.rs:88-92`, `editor_commands/folding.rs:17`.
- Hypothesis: the editor's rope is LF-only, so `Doc::new(&editor.text())` offsets equal the rope's UTF-16 offsets. Settle: `buffer` normalises line endings on load (`line_ending` field) — check `ide-text`.
- Hypothesis: `standalone_targets` gives `hello.rs` (`fn main` outside Cargo) a `Main` target on line 0 and `target_menu_items` puts "Run '…'" first and "Modify Run Configuration…" last. Settle: run the test.
- Hypothesis: the backend's detection names the root script `pnpm run dev` (`crates/ide-run/src/detect.rs:238`) so the menu reads the same whether the store matched it or the lockfile decided. Settle: run the test.
- Hypothesis: `theme::icon` elements are found by `debug_bounds("icon:bookmark")` (its `debug_selector`). Settle: run `toggle_bookmark_draws_…`; if `None`, drop that assertion (the cell's bounds already prove the place).
- Hypothesis: right after `scroll_caret_to_center`, `scroll_top()` already holds the new scroll while the layout is the last frame's; the popup's y is computed from it and the x from the last frame's text edge. Settle: `next_change_in_the_popup_…` and a look at the window.
- Hypothesis: the run store's entity changes when another project opens in the window, so comparing its `EntityId` re-subscribes the run marks. Settle: read where `Content::Project { run, .. }` is built.
- Hypothesis: `Theme::color(name)` returns `Option<Color>` with `r, g, b, a` (as `theme::color` uses it). Settle: compile.
- Hypothesis: `gpui_kit::Focusable as _` next to `prelude::*` does not clash. Settle: compile; drop it if the prelude exports it.
- Hypothesis: `launch_production` was made `pub(super)` (hunk 21) and has no other caller whose privacy matters.
- Hypothesis: `RunMarksInput::find` (regexes over at most `MAX_SCAN`) runs on `background_executor().spawn`: all its data is owned and `Send`. Settle: compile.

## (e) Tests most likely to fail first, and why

0. Existing tests that assert the text's left edge as numbers + lane + chevrons will move, because every tab editor now has the 15 px bookmarks column (and a source the producers know a 14 px run column), as the webview's editors have: `workbench::editor_extras::tests::proofs15_folding::the_fold_column_is_drawn_only_while_show_code_folding_outline_is_on` (`src/main.rs`: expects `32 + 4 + fold`, gets `32 + 15 + 14 + 4 + fold`) and `workbench::editor_extras::tests::distraction_free_mode_hides_the_tabs_and_the_gutter_and_centres_the_text` (`notes.txt`, line 1249 and 1268 of `editor_extras.rs`: expects `32 + 4 (+ FOLD_GUTTER)`, gets 15 more). Fix: add the columns' widths to the expected values (read them with `gutter_column("bookmarks"/"run", cx)`); the click at `bounds.left() + 8` in the second test still lands on the numbers. Hypothesis: no other existing test measures the gutter's x (grep of `fold_column_width`, `gutter_column(`, `painted_line_numbers` in `workbench/` found only these and the HTTP Client's, whose relative checks still hold). Presses in existing tests are plain or with Shift/Alt (`debug.rs:1890`), which the new hook lets through.
1. `the_change_lane_draws_…`: `painted_gutter_rows` is filled only after a frame that ran with the marks set. Hypothesis: `run_until_parked` under `wait_long` draws the window, as the view tests that read `painted_marks` suggest; if not, the wait runs out.
2. `the_bars_follow_an_edit_at_once_…`: Enter at the start of the document may auto-indent or the 300 ms timer may already have fired inside `run_until_parked` (the test clock does not advance without `advance_clock`, so it should not). If the backend answer arrived before the assert, the expected `[2, 4, 6]` becomes `[0, 2, 4, 6]`: read only `lane_marks` right after the keystroke.
3. `next_change_in_the_popup_…`: the reopened popup's place comes from the scroll the move left (see (d)).
4. `a_primary_click_in_the_gutter_…`: the workbench's `capture_any_mouse_down` runs `input::mouse_down` first; a user keymap binding `Meta+Button1` would swallow it (none by default: read `src/platform/keymaps/*`).
5. `a_main_gets_a_run_mark_…`: depends on the producers' standalone Rust detection and on the run store being listed; a `Debug` row may be missing, which the test tolerates.
6. `a_package_json_script_…`: if the backend already detected `npm:dev`, the flow needs no read; otherwise it lists the project root with `fs_read_dir`. Both give the same labels.
7. Known difference (test written to the webview): none of the tests asserts the "Nothing to run here" body of a gutter launch; native `run_context` uses Run Context's body. Fix when wanted: give `run_context` an optional notification and pass `context_run::nothing_to_run()` from `run_gutter_command`.

## (f) What a look at the real window must check

- A modified file: green / blue / grey bars right of the numbers and left of the chevrons, 3 px wide, the notch pointing right on the line after a deletion; the text does not move when bars come and go.
- Typing above a bar: the bar moves at once; a third of a second later the typed line gets its own bar.
- Clicking a bar: the popup opens right of the lane under the change; the toolbar's icons are legible in light and dark; Rollback, Next, Previous, Copy (check mark), Close, Escape, Enter.
- The caret's row: the gutter tint lines up with the text row's tint; the caret line's number is brighter; with the editor unfocused the gutter keeps its tint (the text row loses it).
- Bookmarks: the outline icon centred in a 15 px column; the mnemonic chip readable at 9 px; ⌘-click on a number toggles a bookmark and does not set a breakpoint.
- A Rust / Python / JS test file: run icons in their own 14 px column left of the HTTP marks; the menu's labels and shortcuts.
- `package.json`: one icon per script line, after Unicode text.
- Settings › Editor › Show gutter icons off: the bookmarks and run columns collapse, the text moves left by their width.
- Distraction Free Mode: no gutter, no icons, no bars.

## (g) The other gutters (read only, for a later slice)

| Source                                                     | Draws                                                                                                             | Contribution / order                                 | Width                                                                                                           | Lines |
| ---------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- | ----- |
| `src/editor/navigation/implementationGutter.ts`            | inheritance icons (implemented / overridden ↓, implementing / overriding ↑), click navigates or opens the chooser | `lsp.lineMarkers`, 56 (`src/editor/lsp/index.ts:80`) | the shared line-markers gutter (`cm-openidea-gutter-lineMarkers`, hidden by the `inheritors`/`supers` switches) | 452   |
| `src/features/color-preview/gutter.ts`                     | a colour swatch on the first colour literal of a line; click opens the colour picker in editable files            | `colorPreview.gutter`, 59                            | `cm-openidea-gutter-colors` (switch `colors`)                                                                   | 130   |
| `src/features/services/dockerGutter.ts`                    | ▶ on Dockerfile `FROM` and Compose services, temporary Docker run configurations via `run.runConfig`              | `services.dockerGutter`, 56                          | shares the run gutter's class (14 px, switch `run`)                                                             | 121   |
| `src/features/vcs/reviewGutter.ts`                         | code review: comment icon on threaded lines, "+" on the hovered commentable line, range outline                   | review mode only                                     | 16 px                                                                                                           | 364   |
| `src/features/vcs/hunkInclusion.ts`                        | a 12 px checkbox per changed block in a local-changes diff (partial commit)                                       | diff viewer only                                     | `cm-include-gutter`                                                                                             | 162   |
| `src/features/vcs/mergeGutters.ts`                         | merge editor: accept arrow, ignore cross, magic wand                                                              | merge editor only                                    | fixed width (`cm-merge-gutter`)                                                                                 | 258   |
| `src/features/http-client/endpoints.ts` (column at `:133`) | endpoint icons in the run gutter's class                                                                          | `http.endpoints`, 58                                 | 14 px (`cm-openidea-gutter-endpoints`)                                                                          | 163   |

## Things in the brief or the native code that turned out different

- The brief says the native gutter "reserves none of the webview's other gutter columns": it already reserves the 4 px change lane (`CHANGE_LANE`, `view.rs:131`), painted empty.
- The brief says `unit:src/features/run/npmGutter.test.ts` is model only: `INVENTORY.md:3094` marks it `[m]`, but `parity.json` already says `done`; the integrator should make them agree (done once this lands).
- The icon set is not `app-gpui/src/ui/icons`: icons come from `theme::icon(cx, name, size, color)` (`app-gpui/src/theme/mod.rs:126`) over `assets/gen/icons.json`, as `http_client.rs:1270` uses it.
- Neither `vcs_text_actions.rs` nor `vcs_operations.rs` keeps hunk state: both ask `vcs_line_hunks` afresh per action and build a throwaway `HunkState`; there was nothing to share, so the gutter keeps its own per editor.
- Native numbers use the `fg-gutter` token where the webview uses `--fg-line-number` (fallback `--fg-gutter`); the gutter hairline (`--border-gutter`) and background (`--bg-editor-gutter`) are not drawn natively.
