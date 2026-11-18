# Report: wiring batch 1

Seven of the twelve items are wired, each with a window test seen failing before the change and failing again with its guard switched off: 1, 2, 3, 4/5 (one item), 6 and 8. Item 7 stopped because it needs a view. Items 9 to 12 were not started: I reached about two thirds of the budget and ran the gates on what had landed. The one real-binary run drew only the "Opening…" screen, so there is no visual check of the landed items.

## 1. Gates

All of these ran after the last source edit, from `app-gpui/`, with nothing else running beside them.

| Gate                                                         | Result                                                                                               |
| ------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| Whole suite, run 1                                           | **8385 passed, 0 failed** in 222.92 s. uptime before: load 4.35 3.23 3.75; after: 9.39 8.68 6.21     |
| Whole suite, run 2                                           | **8385 passed, 0 failed** in 225.81 s. uptime before: load 7.45 8.29 6.11; after: 61.48 20.05 10.72  |
| 20 runs by filter of the 10 tests added, declared or changed | **20 of 20 runs passed all 10** ("test result: ok. 10 passed" 20 times; no FAILED or panicked lines) |
| `cargo test --test web_view`                                 | 1 passed, 0 failed                                                                                   |
| `cargo clippy --all-targets -- -D warnings`                  | exit 0, no diagnostics                                                                               |
| `cargo build`                                                | exit 0, 0 warnings                                                                                   |

- Disk free before the heavy commands was 305–350 GB (logged in each log).
- A first whole-suite run was stopped by me before it finished, because I then changed items 3, 6 and 8. It is not counted.
- None of the eight slow tests the brief warned about failed in either counted run.

Logs are in `$S/dwb1/`: `suite1.log`, `suite2.log`, `repeat20.log`, `web_view.log`, `clippy.log`, `build.log`, the first-run logs `i*_first.log`, and the mutation logs `m*.log`. Every mutation log ends in "restored: yes" (the file's hash matched after restore).

Files changed (from `git diff --name-only`; I formatted only these, with `$S/fmt.sh`): `debugger/driver.rs`, `editor/intelligence.rs`, `editor/view.rs`, `theme/scheme_runtime.rs`, `workbench/context_menus.rs`, `workbench/debug.rs`, `workbench/editor_core.rs`, `workbench/mod.rs`, `workbench/project_files.rs`, `workbench/tests/proofs16_todo_run_pending.rs`, `workbench/tests/proofs18_e2e_editor.rs`, `workbench/todo_window.rs`, all under `app-gpui/src/`. Nothing under `src/`, `src-tauri/`, `crates/`, `docs/` or `e2e/` changed. Nothing is committed.

## 2. Items

All paths below are under `/Users/davidvieira/code/openidea-bazel-work/openidea/.claude/worktrees/agent-a307dd904b5ae1f01/app-gpui/src/`.

### Item 1: Back/Forward and Last Edit Location forget deleted files — done

- **Webview rule:** when a deleted file's tab closes (only tabs that are not dirty), its path is dropped from both histories (`src/editor/core/controller.ts:1896-1922`, `nav.removePath` and `edits.removePath`).
- **Before the change:** the test failed. Last Edit Location went to the deleted file and left no active editor ("reached later: None").
- **Change:**
  - `workbench/project_files.rs` `close_removed_tabs`: each closed tab's path is forgotten.
  - `workbench/editor_core.rs`: new `EditorCore::forget_path`, which calls `NavigationHistory::remove_path` and `EditHistory::remove_path`.
- **Test:** `workbench/editor_core.rs` `back_and_last_edit_location_skip_a_file_deleted_in_the_project_view`. It edits two files, saves one, deletes it through `project.delete` and the Delete dialog (into the test's own trash folder), then runs Last Edit Location twice, Back twice and Forward twice. The deleted file never opens again, it stays off disk, and the editor stays on the surviving file.
- **Mutation** (forget call removed): fails.
- **Left out:** the webview also does this for files removed outside the app (any file-system removal event). Natively it is reached only from the project view's delete.

### Item 2: selected text stays visible inside a find match — done

- **Webview rule:** the selected ranges are painted again over mark backgrounds (`src/editor/core/cm/selectionMarks.ts:12-35`).
- **Before the change:** the test failed; the paint order had mark backgrounds and no repainted selection.
- **Change:** `editor/view.rs` paint. After the backgrounds drawn over the selection, the ranges from `selection_marks::selected_ranges` (clipped to the painted lines) are painted again. Two test-only layer kinds record the order: `MarkBackgrounds(line)` and `SelectedText(line)`.
- **Test:** `workbench/tests/proofs18_e2e_editor.rs` `the_selection_inside_a_find_match_is_painted_over_the_matchs_background`. ⌘F, type "Order", select "rde" inside a match; the selection is painted after the match background on that line.
- **Mutation** (repaint loop never runs): fails.
- **Left out:**
  - The webview also recolours the selected text (`color: var(--fg-editor-selection)`).
  - The repaint uses the native selection colour `bg-selected`. The webview uses the editor selection colours, active and inactive (`src/editor/core/cm/theme.ts:200-203`). The native base selection layer has the same difference, so I kept the two consistent rather than change the base.

### Item 3: the TODO preview is editable — done

- **Webview rule:**
  - The preview is an editable editor of the item's file (`src/features/todo/PreviewEditor.tsx:1-11`, `:67-84`).
  - Previews with unsaved edits are held and given back by path (`heldDirty`, `:28-40`).
- **Before the change:** the handed-over test, now declared, failed with "a file that can be written is edited".
- **Change** (`workbench/todo_window.rs`, `workbench/mod.rs`):
  - The read-only override is removed; the preview opens with the document's own read-only flag.
  - Choosing another item of the same file moves the shown editor's caret.
  - A replaced preview with unsaved edits is kept, using the existing `HeldPreviews` model for the held paths plus a map from path to editor. It is shown again when its file is previewed again.
  - `document_editors` (what Save All saves) now includes these previews.
- **Why the hold is in this item:** without it, the new editable preview would silently lose edits when the preview moved to another file. The notes had marked the hold as "larger".
- **Tests** (both in `workbench/tests/proofs16_todo_run_pending.rs`, declared in `workbench/mod.rs`):
  - `the_todo_preview_edits_the_file_and_moves_its_caret_to_another_item_of_the_same_file` (the handed-over test).
  - New: `an_edit_in_the_todo_preview_survives_another_file_and_save_all_writes_it`. It types in the preview, moves to another file, runs Save All, checks the edit reached disk, then previews the first file again and sees the edited text.
- **Mutations:**
  - read-only override restored: both tests fail;
  - same-file caret reuse off: the handed-over test fails;
  - holding off: the new test fails;
  - Save All not reaching the previews: the new test fails.

### Items 4 and 5: breakpoints move with edits — done (line breakpoints)

- **Webview rule:** the editor where the change started moves the file's breakpoints and special breakpoints with the lines. For a change the backend applied, only the file's "writer" editor does (`src/features/debugger/editor/breakpointGutter.ts:225-238`).
- **Before the change:** the test failed. The gutter mark moved but the store stayed on line 1 (`[1]` vs `[3]`).
- **Change:**
  - `editor/view.rs`: `EditorEvent::Edited` gains `before: Rope`.
  - `debugger/driver.rs`: new `DebugDriver::document_changed`, which calls `breakpoint_gutter::document_changed` inside `change`.
  - `workbench/debug.rs`: `watch_debug_editor` also handles `Edited`. Edits the backend applied (`EditOrigin::External`) count as remote; the writer is the active editor if it shows the file, otherwise the file's first editor.
  - `editor/intelligence.rs`: its `Edited` pattern now ignores the new field.
- **Test:** `workbench/debug.rs` `a_breakpoint_moves_with_the_lines_typed_or_deleted_above_it`. It toggles a breakpoint on line 2 and types two newlines above it. The breakpoint then shows, is stored for the project (stored text contains `"line":3`), and stays there after `DebugDriver::load` re-reads the storage. Two backspaces move it back. A line inserted by the backend moves it exactly once.
- **Mutations:**
  - store update removed: fails;
  - writer forced off: fails on the backend-inserted line.
- **Left out:** the special-breakpoint half of `document_changed` is reached by the same call but has no window test and no mutation, so it is unproven.

### Item 6: Reload Changed Classes in the editor menu — done (landed after item 8)

- **Webview rule:** the row appears on one `.java` file while a live session can hot swap (`src/features/debugger/index.ts:104-108`, `src/features/debugger/hotswap.ts:44-47`).
- **Before the change:** the test failed with "the Java session can reload classes".
- **Change:** `workbench/context_menus.rs`. A new `GroupFacts::hot_swap` comes from `debugger::hotswap::hot_swap_menu_action(paths, state)`, and the `debugger.hotswap` group gets a rows function.
- **Test:** `the_editor_menu_offers_reload_changed_classes_on_a_java_file_while_a_session_can_reload` (same file). The stand-in session is inserted straight into the debugger state, so no program or JVM is started. The row is absent without a session, present and enabled on the Java file with one, and absent on a `.txt` file.
- **Mutation** (rows empty): fails.

### Item 7: stash rows show the message and the branch apart — stopped, needs a view

- The webview's Unstash dialog draws the raw `stash@{n}: message` (`src/features/vcs/changeOps.tsx:510`), which is what the native Unstash form already shows.
- The message/branch split exists only in the Commit view's stash list (`src/features/vcs/CommitView.tsx:1398-1404`). The app has no rendered stash list: searching for a stash list view found only the Unstash form in `vcs/git_forms.rs`.
- Changing the Unstash form would move it away from the webview. Nothing was changed.

### Item 8: the editor colour scheme follows a light/dark switch — done (landed before item 6)

- **Webview rule:** when the effective theme turns light or dark, a chosen scheme of the other kind is replaced by Theme default in the global settings. A scheme chosen in the same settings change is kept (`src/editor/core/colorSchemeRuntime.ts:32-50`).
- **Before the change:** the test failed (the scheme stayed "light").
- **Change:** `theme/scheme_runtime.rs`. `follow_theme_kind` runs at the start of `apply`, which runs after every theme cascade: the toggle, Sync with OS, and settings edits. It remembers the last kind and scheme. On a kind change, and only if the scheme did not change in the same step, it writes `editor.colorScheme` through `scheme_after_theme_change` and reloads every open project's settings.
- **Test:** `workbench/mod.rs` `the_editor_scheme_follows_the_theme_turning_dark_or_light`:
  - a "light" scheme becomes Theme default when toggling to dark, and the editor background changes;
  - a "dark" scheme stays while the theme stays dark;
  - toggling back to light resets it again;
  - a scheme written in the same change as a dark theme stays.
- **Mutations:**
  - following off: fails;
  - same-change guard off: fails.

### Items 9 to 12 — not started

Hover quick-fix link, Search Everywhere endpoint refresh, Find in Files inline replacement, and Paste following files copied in the file manager. No code was touched for them.

## 3. Existing tests whose expectation changed (3)

| Test                                                                                            | Change                                                                                       | Webview source                                      |
| ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- | --------------------------------------------------- |
| `workbench/mod.rs` `the_todo_window_lists_the_projects_todo_comments_and_follows_what_is_typed` | asserted the preview is read-only; now asserts it is editable                                | `src/features/todo/PreviewEditor.tsx:1-5`, `:72-84` |
| `editor/view.rs` `the_owner_hears_edits_carets_and_the_pointer_and_may_take_a_press`            | each `Edited` event now also states the text before the edit (adds assertions, weakens none) | `breakpointGutter.ts:234-236` (`u.startState.doc`)  |
| `workbench/context_menus.rs` `every_menu_group_is_listed_once`                                  | `debugger.hotswap` removed from the groups without rows                                      | `src/features/debugger/index.ts:104-108`            |

## 4. Shots

The one allowed run of the real binary (`$S/insp8/run.sh`, neutral `INSPECT_BASE`, temp project, removed afterwards; the app was stopped by its PID) never drew past "Opening /private/tmp/openidea-inspect-…/proj…". The runner logged "no frame was drawn within a second: is the window covered?" eight times.

All three shots show only that screen (`$S/insp8/shots/`):

- `dwb1-01-find-selection-light-1.png`
- `dwb1-02-find-selection-dark-1.png`
- `dwb1-03-todo-dark-1.png`

So nothing landed was seen in the real window. The stash list and Find in Files with a replacement could not be shot in any case, because items 7 and 11 did not land.

## 5. Rows

`parity.json` is not written.

| Row                                                        | Clause now proven, and by which test                                                                                                                                                                                                                                | Missing clause still open                                                                          |
| ---------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `unit:src/editor/core/model/editHistory.test.ts`           | "no deletion handler calls NavigationHistory::remove_path / EditHistory::remove_path" is resolved by `back_and_last_edit_location_skip_a_file_deleted_in_the_project_view`                                                                                          | Only the project-view delete reaches it; a removal outside the app does not.                       |
| `unit:src/editor/core/cm/selectionMarks.test.ts`           | "never calls selection_marks::selected_ranges" is resolved by `the_selection_inside_a_find_match_is_painted_over_the_matchs_background`                                                                                                                             | The proof is the recorded paint order, not pixels, and the selected-text colour is not applied.    |
| `unit:src/features/todo/TodoView.test.tsx`                 | "opens the preview with read_only: true" is resolved by `the_todo_preview_edits_the_file_and_moves_its_caret_to_another_item_of_the_same_file`; "never uses HeldPreviews" is resolved by `an_edit_in_the_todo_preview_survives_another_file_and_save_all_writes_it` | Nothing of its Missing text.                                                                       |
| `unit:src/features/todo/PreviewEditor.test.tsx`            | Same two tests                                                                                                                                                                                                                                                      | It has no Missing clause ("the view is not built"); not re-assessed.                               |
| `unit:src/features/debugger/__tests__/breakpoints.test.ts` | "subscribes only to GutterPressed and never calls breakpoint_gutter::document_changed" is resolved for line breakpoints by `a_breakpoint_moves_with_the_lines_typed_or_deleted_above_it`                                                                            | Special breakpoints moving with edits is unproven.                                                 |
| `unit:src/features/debugger/__tests__/hotswap.test.ts`     | "builds the editor menu without it, hot_swap_menu_action has no production caller" is resolved by `the_editor_menu_offers_reload_changed_classes_on_a_java_file_while_a_session_can_reload`                                                                         | Nothing of its Missing text.                                                                       |
| `unit:src/features/vcs/__tests__/decorations.test.ts`      | None                                                                                                                                                                                                                                                                | "vcs_diff::model::stash_label has no production caller" is still open: it needs a stash list view. |
| `module:src/platform/colorScheme.ts`                       | "switching the scheme back when the theme turns light or dark" is resolved by `the_editor_scheme_follows_the_theme_turning_dark_or_light`                                                                                                                           | "token backgrounds and effects", "per-language root tokens"                                        |

## 6. Hypotheses (not measured)

- **Hypothesis:** with a split pane, the mirror pane receives the other pane's edit as `External` and skips as a non-writer, so breakpoints move once. To check: a window test with two panes of one file, typing in one, asserting the stored line once.
- **Hypothesis:** writing `editor.colorScheme` globally with an empty window label reaches every window. The code reloads every open project's settings after the write, but only one window was tested. To check: a two-window test asserting `settings::effective("w2").editor.color_scheme`.

## 7. What in the notes or brief was false

- **Item 7:** the anchor is wrong. The Unstash dialog already matches the webview (`changeOps.tsx:510`), and the split the row names lives in the Commit view's stash list, which has no native view.
- **Item 3:** the dirty-preview hold was called "larger", but leaving it out would make the editable preview lose edits. Done with the `HeldPreviews` model, it took about 40 lines.
- **Item 4:** "adding the previous rope there is one field" holds, but the one place that builds the event is also an existing test (`the_owner_hears_edits…`), which had to change.
- **Item 8:** there is no window at hand in `apply`. Where the notes suggest `set_theme`, wiring it there would miss Sync with OS and settings edits.
- **Item 1:** the native app reaches tab closing on deletion only from the project view's delete; the webview also does it for removals outside the app.
