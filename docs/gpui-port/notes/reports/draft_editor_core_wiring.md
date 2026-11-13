I wrote everything the brief asks for, but none of it has been compiled or run. Fifteen of the brief's ~70 actions are registered, plus fifteen of mine at the window level. Every registered id appears in `assets/gen/actions.json` (I checked).

## 1. Files written

| File                                                                                                                                                                | Lines |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----- |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/editor/folds.rs`                                       | 334   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/editor/core_actions.rs`                                | 669   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/editor/paste_history_view.rs`                          | 438   |
| `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/workbench/editor_core.rs`                              | 886   |
| `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/editor_core_integration.md` | notes |

- **`folds.rs`**: the `Folds` model. It holds the backend's fold ranges, the folded regions and the hidden lines, and maps rows to lines. It also finds the placeholder, steps the caret over a fold, follows edits, and holds `toggle_fold`.
- **`core_actions.rs`**: the per-editor `CoreState`, `refresh_folds`, and the commands below. It adds accessors on `Editor`: `shown_lines`, `folded_regions`, `folds_are_current`, `font_zoom`, `set_font_zoom`.
- **The dialog**: `PasteHistoryDialog::new(entries, read_only, cx)`.
  - Events: `PasteHistoryEvent::{Paste{text, linewise}, Remove(Vec<String>), Cancelled}`.
  - Accessors: `rows`, `chosen`, `preview`, `field`, `can_paste`, `empty_text`, `paste(only)`, `remove`.
- **`editor_core.rs`**: the window's back/forward and edit-location history, `install_editor_core`, and the window-level actions.

The notes file has every hunk, quoted against existing text:

- **`mod` lines**: three in `editor/mod.rs`, one in `workbench/mod.rs`.
- **`editor/actions.rs`**: one line to register the new commands.
- **`workbench/mod.rs`**: the field and its initialiser, the registration, deleting the old `editor.closeAllTabs`, `install_editor_core` in `load_tab_probed`, and the `pasteHistoryOpen` context key.
- **`tab_views.rs`**: `install_editor_core` for the single-file window.
- **`view.rs` (V1–V10)**:
  - the `core` field;
  - `Layout.line_of`;
  - metrics that follow the font zoom;
  - `font_size`, `set_read_only` and `scroll_caret_to_center`;
  - the row map in every line↔pixel conversion (grepped; all sites listed);
  - folds that open around a caret and follow edits;
  - `refresh_folds` after highlighting;
  - the caret stepping over folds and skipping hidden lines;
  - a new prepaint, and paint of the placeholder and the closed-fold chevron.

## 2. Actions registered

- **The nine folding actions** (`code.fold`, `unfold`, `foldToggle`, `foldAll`, `unfoldAll`, `collapseRecursively`, `expandRecursively`, `foldSelection`, `collapseBlock`): built on `editor_commands::folding` and `collapse_block`.
  - Test: `the_folding_commands_hide_and_show_the_lines_of_their_regions`. Also `the_caret_skips_the_lines_a_fold_hides`, `a_fold_follows_the_edits_above_it`, and a key test in `the_editor_actions_do_nothing_outside_the_editor`.
  - IPC: `doc_folding`.
- **`code.extendSelection` / `shrinkSelection`**: `SelectionStack` plus `BackendChain`; presses wait for the backend in order.
  - Test: `extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`. IPC: `doc_selection_ranges`.
- **`code.autoIndentLines`**: test `auto_indent_lines_reindents_the_selected_lines`.
- **`editor.scrollToCenter`**: test `scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view`.
- **`editor.increaseFontSize` / `decreaseFontSize` / `resetFontSize`**: test `the_font_size_steps_in_the_active_editor_and_resets_in_every_editor`.
- **`navigate.back` / `forward`**: test `back_and_forward_move_between_the_places_of_two_files`.
- **`navigate.lastEditLocation` / `nextEditLocation`**: test `last_and_next_edit_location_walk_through_the_edits`.
- **`editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber`, `editor.copyReference`**: test `the_copy_path_actions_copy_the_forms_of_the_edited_file`. IPC for Copy Reference: `doc_outline`.
- **`file.reloadFromDisk` / `reloadAllFromDisk`**: test `reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes`. IPC: `doc_set_encoding` with reload, via `Editor::set_encoding`.
- **`file.toggleReadOnly`**: a confirm dialog, then the writable bit. Test `toggle_read_only_asks_then_locks_the_file_and_its_editor`. IPC: `fs_set_writable`.
- **`code.pasteFromHistory`, `editor.pasteHistory.paste` / `remove`**: test `paste_from_history_pastes_the_entry_chosen_in_the_dialog`.
- **`editor.closeAllTabs`**: every pane, and the reopen history cleared. Test `close_all_tabs_closes_the_tabs_of_every_pane`.
- **Confinement**: `the_actions_need_what_their_when_clauses_name` covers `projectOpen`, `textEditorActive`, `editorTabsOpen` and `pasteHistoryOpen`. `the_editor_actions_do_nothing_outside_the_editor` covers the editor-focus actions and ⌘-.

**Not registered:**

- `code.unwrap`: no backend op gives the syntax-node chain.
- `editor.toggleSoftWrap`: the view has no wrapped layout.
- `toggleRainbowBrackets` / `toggleSemanticHighlighting`: nothing paints either, so a toggle would change nothing visible.
- `editor.contextInfo`: no model for the enclosing header lines.
- `navigate.customRegion` / `navigate.navigationBar`: no popup views.
- The `~` rows that need a syntax tree: comment, move element/statement, toggle case, complete statement.
- The two hint `~` rows: an optional hunk in `command_actions.rs` would show their hint through `editor.showHint`; not applied.
- The other `~` rows are outside these models. The ⌥-drag column selection stays unbuilt.

**Proof tests this wiring turns green:** `close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen` (once the mod.rs deletion hunk is applied) and `the_copy_reference_action_copies_the_qualified_name_without_a_popup`. `restores_alt_drag_column_selection_once_the_caret_gesture_is_removed` still fails.

## 3. Spec files covered

| Spec                          | Covered | Not covered                                                                                                                         |
| ----------------------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------- |
| `PasteHistoryDialog.test.tsx` | 6/11    | ⌘-click paste in list order, ⇧-click in the list, double-click paste, reopening resets the chooser, the grammar-highlighted preview |
| `extras.test.ts`              | 2/3     | the line-wise entry pasted above the caret line                                                                                     |
| `extendSelection.test.ts`     | —       | already covered by the model tests                                                                                                  |
| `folding.test.ts`             | —       | already covered by the model tests; mine are window-level only                                                                      |

## 4. Not built, assumptions, likely failures

**Not built:**

- Clicking a fold chevron or placeholder does nothing, and the open chevrons are not drawn.
- Expand to Level is not registered.
- A caret at a fold's end, which sits on a hidden line, is not painted.
- Only file switches leave Back entries; mouse clicks and Go to Line do not.
- Font zoom does not go to a focused terminal first.
- Copy Reference does not ask the language server when the outline has nothing at the caret.
- The hint is a notification, not a balloon over the editor.

**Assumptions for the integrator to check first:**

- The tree-sitter folds for the Rust fixture are lines 0–5 and 1–3.
- `cmd--` reaches `code.fold` in the IntelliJ macOS keymap.
- `Pixels / Pixels` gives `f32`.
- Enter answers `PromptDialog::confirm` with OK.
- `pasteHistoryListFocus` is deliberately never published: when the dialog declines Backspace, the keymap runs next, which would delete an entry while the user edits the filter.

**Most likely to fail on first run:**

- The fold tests, if the Rust fold ranges differ from the assumption.
- The first extend step, if the backend's innermost range is wider than the word.
- Back/Forward, if focus-in fires before `editor()` names the new tab.
- Scroll to Center, if no frame is painted before it reads the layout.
