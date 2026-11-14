# Integrating Annotate with Git Blame into the editor and the workbench

New files, not compiled yet:

- `app-gpui/src/editor/view/annotations.rs`: the model of the column (`Annotations`, `AnnotationRow`, `short_author`, `age_ranks`, `rank_share`), its shaping and painting (`PreparedColumn`), the `AnnotationEvent` the editor emits, and the `impl Editor` block the view and the workbench call. It is a child module of `editor::view`, so it reads `Layout` and the editor's private fields.
- `app-gpui/src/workbench/annotate.rs`: the `vcs.annotate` action, the backend calls (`vcs_blame`), the per-editor state (`AnnotateSlot`), the column's menu, and six end-to-end tests.

Every anchor below is quoted from the file as it is now. No line numbers are given. Run `cargo fmt` after pasting.

IPC commands the action stands for: `vcs_blame` (first blame, the 800 ms re-blame after an edit, the 100 ms re-blame after the head commit moves). The column's click and its Show Commit row run the registered action `vcs.showCommit`, which is already native. `vcs_revision_files`, `vcs_revision_children` and `vcs_block_history` are not used by the column. Their webview callers are `RevisionBrowser.tsx`, `PathFilterDialog.tsx` and `SelectionHistory.tsx`.

## 1. `app-gpui/src/editor/view.rs`

### 1a. The module

After

```rust
use std::rc::Rc;
use std::sync::Arc;
```

add

```rust

pub mod annotations;
```

It must be `pub mod`: the workbench uses `crate::editor::view::annotations::{AnnotationEvent, Annotations, AnnotationRow}`. Being public also keeps the test accessors free of dead-code warnings.

### 1b. `Layout` records the column's width

In `struct Layout`, after

```rust
    /// Width of the fold chevrons' column, the gutter's last.
    fold_column: Pixels,
```

add

```rust
    /// Width of the blame annotations' column, the gutter's first; none while it is not shown.
    annotation_column: Pixels,
```

### 1c. The editor's field

In `pub struct Editor`, replace

```rust
    /// Folding, Extend / Shrink Selection and the font zoom ([`super::core_actions`]).
    pub(super) core: super::core_actions::CoreState,
}
```

with

```rust
    /// Folding, Extend / Shrink Selection and the font zoom ([`super::core_actions`]).
    pub(super) core: super::core_actions::CoreState,
    /// Blame annotations shown in the gutter's first column ([`annotations`]).
    annotations: Option<annotations::Annotations>,
}
```

and in `Editor::with_text`, replace

```rust
            core: Default::default(),
        };
        editor.refresh_highlights(cx);
```

with

```rust
            core: Default::default(),
            annotations: None,
        };
        editor.refresh_highlights(cx);
```

### 1d. The entries follow the edits

In `fn follow(`, after

```rust
        for marks in self.marks.values_mut() {
            *marks = map_marks(marks, &changes);
        }
        self.marks.retain(|_, marks| !marks.is_empty());
```

add

```rust
        if let Some(annotations) = self.annotations.as_mut() {
            annotations.map(&old, &changes);
        }
```

`old` is the rope before the changes. It is still alive at that point, since `history.record(.., invert(&old, &changes), ..)` uses it further down.

### 1e. A press on the column is the column's

In `fn mouse_down`, replace

```rust
        if let Some(line) = self.gutter_line_at(event.position) {
            (self.drag_anchor, self.adding) = (None, None);
            // The chevrons' column opens and closes the line's fold and takes no breakpoint.
```

with

```rust
        if let Some(line) = self.gutter_line_at(event.position) {
            (self.drag_anchor, self.adding) = (None, None);
            // The blame annotations' column answers its own presses: no breakpoint toggles there.
            if self.press_annotation(event.position, cx) {
                return;
            }
            // The chevrons' column opens and closes the line's fold and takes no breakpoint.
```

### 1f. Hover

In `fn mouse_move`, replace

```rust
            let over_gutter = self.gutter_line_at(event.position).is_some();
            if over_gutter != self.gutter_hovered {
                self.gutter_hovered = over_gutter;
                cx.notify();
            }
```

with

```rust
            let over_gutter = self.gutter_line_at(event.position).is_some();
            if over_gutter != self.gutter_hovered {
                self.gutter_hovered = over_gutter;
                cx.notify();
            }
            self.hover_annotation(Some(event.position), cx);
```

In `impl Render for Editor`, replace

```rust
                if !hovered {
                    editor.hover(None, Modifiers::default(), cx);
                }
```

with

```rust
                if !hovered {
                    editor.hover(None, Modifiers::default(), cx);
                    editor.hover_annotation(None, cx);
                }
```

### 1f'. The hovered entry's tooltip

In `impl Render for Editor`, replace

```rust
            .child(EditorElement { editor: cx.entity() })
```

with

```rust
            .child(EditorElement { editor: cx.entity() })
            .children(self.annotation_hover_target())
```

`annotation_hover_target` returns a `Stateful<Div>`. It is positioned absolutely over the hovered entry (left 0, the row's top, the column's width, one line high) and carries a `tooltip_lines(title, summary)` tooltip, or "Not committed yet". The tooltip shows after `TOOLTIP_DELAY`. It has no mouse listeners and does not `occlude()`, so the root's `on_mouse_down` and `on_mouse_move` still receive the presses (Hypothesis: GPUI's hit test keeps the parent hovered under a plain child hitbox).

### 1g. `Prepared` carries the shaped column

In `struct Prepared`, after

```rust
    /// Per painted row: the chevron of a folded line in the gutter.
    markers: Vec<Option<ShapedLine>>,
```

add

```rust
    /// The blame annotations' column, when it is shown.
    annotations: Option<annotations::PreparedColumn>,
```

### 1h. `prepaint`: the column widens the gutter

Replace

```rust
        // Distraction Free Mode centres a column of its width in characters (and the caret's room).
        let text_left = if distraction_free(cx) {
```

with

```rust
        // The blame annotations' column, when shown, is the gutter's first and widens it.
        let column = editor.prepare_annotations(&painted, gutter > px(0.0), window, cx);
        let annotation_column = column.as_ref().map_or(px(0.0), annotations::PreparedColumn::width);
        let gutter = gutter + annotation_column;
        // Distraction Free Mode centres a column of its width in characters (and the caret's room).
        let text_left = if distraction_free(cx) {
```

The widened `gutter` then feeds `text_left`, `Layout.gutter` (hit-testing) and `Prepared.gutter` (numbers, chevrons, the current-line tint). `metrics()`' gutter value is read only here (`grep -n "metrics(cx)" app-gpui/src/editor/view.rs`), so nothing else needs it. Then, in the `Prepared { layout: Layout { … } … }` literal, replace

```rust
            layout: Layout {
                bounds,
                gutter,
                fold_column,
                text_left,
```

with

```rust
            layout: Layout {
                bounds,
                gutter,
                fold_column,
                annotation_column,
                text_left,
```

and replace

```rust
            numbers,
            gutter,
            marks,
            placeholders,
            markers,
        }
    }
```

with

```rust
            numbers,
            gutter,
            marks,
            placeholders,
            markers,
            annotations: column,
        }
    }
```

### 1i. `paint`

1. Replace

```rust
        let Prepared { layout, numbers, gutter, marks, placeholders, markers } = prepared;
        let (line_height, text_left, fold_column) = (layout.line_height, layout.text_left, layout.fold_column);
```

with

```rust
        let Prepared { layout, numbers, gutter, marks, placeholders, markers, annotations } = prepared;
        let (line_height, text_left, fold_column, annotation_column) =
            (layout.line_height, layout.text_left, layout.fold_column, layout.annotation_column);
```

2. After the line numbers' block

```rust
                if let Some(number) = numbers.get(index).filter(|_| *gutter > px(0.0)) {
                    let x = bounds.left() + *gutter - fold_column - number.width - px(12.0);
                    let _ =
                        number.paint(point(x.max(bounds.left()), top), line_height, TextAlign::Left, None, window, cx);
                }
```

add

```rust
                if let Some(column) = annotations.as_ref() {
                    column.paint_row(index, bounds.left(), top, line_height, window, cx);
                }
```

3. The breakpoint dot moves right of the column. Replace

```rust
                        point(bounds.left() + px(4.0), top + (line_height - diameter) / 2.0),
```

with

```rust
                        point(bounds.left() + annotation_column + px(4.0), top + (line_height - diameter) / 2.0),
```

4. In the `std::mem::replace(layout, Layout { … })` at the end, replace

```rust
            Layout {
                bounds,
                gutter: *gutter,
                fold_column,
                text_left,
```

with

```rust
            Layout {
                bounds,
                gutter: *gutter,
                fold_column,
                annotation_column,
                text_left,
```

`annotation_column` is the copy taken in step 1. Reading `layout.annotation_column` inside the `replace` call's arguments would not compile, because `layout` is already mutably borrowed there.

## 2. `app-gpui/src/workbench/mod.rs`

- After `mod agents;` add `mod annotate;`.
- In `pub struct Workbench`, after

```rust
    /// The Git menu's repository operations: Show Commit in Git Log waiting for the log.
    vcs_operations: vcs_operations::VcsOperationsSlot,
```

add

```rust
    /// The editors that show blame annotations, and the head commit they were blamed on.
    annotate: annotate::AnnotateSlot,
```

- In `Workbench::new`, after `            vcs_operations: Default::default(),` add `            annotate: Default::default(),`.
- In `pub fn register_actions`, after `    vcs_operations::register_actions(registry);` add `    annotate::register_actions(registry);`.
- In `fn publish_context`, after `        self.publish_vcs_context(window, cx);` add `        self.publish_annotate_context(window, cx);`.

## 3. `app-gpui/src/workbench/branches.rs`

In `fn vcs_state_changed`, after `        self.github_vcs_changed(cx);` add

```rust
        self.annotations_vcs_changed(cx);
```

## 4. `app-gpui/src/workbench/context_menus.rs`

### 4a. The column's own menu

In `pub(super) fn open_editor_context_menu`, replace

```rust
        window.focus(&editor.focus_handle(cx), cx);
        if let Some(line) = gutter_line_at(editor.read(cx), position) {
```

with

```rust
        window.focus(&editor.focus_handle(cx), cx);
        if let Some(line) = editor.read(cx).annotation_line_at(position) {
            return self.show_annotation_menu(editor, line, position, window, cx);
        }
        if let Some(line) = gutter_line_at(editor.read(cx), position) {
```

The column lies left of the text, so without this hunk the file's `gutter_line_at` would open the gutter's menu.

### 4b. The gutter's menu says Close Annotations

In `fn group_facts`, replace

```rust
            // No blame annotations are drawn in this window's editors.
            annotations_shown: false,
```

with

```rust
            annotations_shown: ctx.paths.first().is_some_and(|path| self.annotations_shown_for(path, cx)),
```

### 4c. An existing test: the editor menu's Git rows

Registering `vcs.annotate` adds its row to the editor menu's Git submenu (`git_menu` has `action_with("vcs.annotate", …)`; rows of unregistered actions are left out). In `mod tests`, replace

```rust
    const GIT: [&str; 13] = [
        "Commit File…",
        "Add",
        "-",
        "Show Diff",
```

with

```rust
    const GIT: [&str; 14] = [
        "Commit File…",
        "Add",
        "-",
        "Annotate with Git Blame",
        "Show Diff",
```

Otherwise `in_a_git_repository_the_editor_menus_git_rows_are_enabled` fails.

## 5. Menu entries

Nothing to add. `vcs.annotate` is already listed in `menu_spec.rs` (Git › "Annotate with Git Blame"), the VCS Operations popup (`OPERATIONS`), the Project view's and editor's Git submenus, the change and shelf menus, and the gutter's `vcs.gutter` group. Each becomes live once the action is registered. `the_operations_popup_lists_the_operations_in_order_and_runs_the_chosen_one` builds its expectation from the registry, so it needs no edit. The toggle reads a window context key, not the workbench, so building that popup inside the workbench's update does not panic.

## 6. What is not built, and why

- **The tooltip's date.** The webview's tooltip uses `toLocaleString`. No Rust equivalent was found (Hypothesis, from a grep of `app-gpui/src`). The tooltip writes the date the way the list timestamps write a plain one (`format_day(.., false)` + `format_clock`, with the Date Formats settings).
- **The error path.** "Cannot annotate" (a failed `vcs_blame`) has no end-to-end test: no deterministic way was found to make `vcs_blame` fail on a file of the open repository while `vcsAvailable` holds.
- **Annotate in the diff and merge viewers**:
  - The diff pane's context-menu row uses `diff_blame_target`.
  - `merge.annotate` still answers `false` in `vcs/merge_view.rs` (`MergeCommand::Annotate => false`).
  - Both panes paint their own text, not an `Editor`, so a column there means editing those painters.
  - `vcs_more/diff_blame.rs` stays unused. `workbench/tests/proofs_merge_annotate.rs` is another agent's file and is not compiled.
- **Inline blame of the caret line**:
  - The `vcs_diff/inline_blame.rs` model is not wired.
  - It is off by default (`vcs.git.inlineBlame` defaults to false).
  - It needs a hint painted after the caret line's end, a press on that hint, and timers on the caret.
- **The author lens** (`src/editor/lsp/authorLens.ts`) also calls `vcs_blame`. It is a different feature.
- **The small VCS commands**, not built:
  - `vcs_stash_drop`: the Commit window's Stashes list (Drop Stash button and row menu, `CommitView.tsx`) does not exist natively; only the `drop_stash` model does.
  - `vcs_delete_tag` / `vcs_restore_tag`: the Branches popup's tag submenu (`branchMenu.ts` `tagMenu` › Delete, Restore in the notice) does not exist natively. A tag row does nothing on Enter, and the log's `ShowTagMenu` is only logged.
  - `vcs_stage_hunks`: the diff toolbar's "Stage Change (git add hunk)" (`DiffEditor.tsx` `stageChunk`). The toolbar exists, but the button needs a `DiffCommand` variant and a `can_stage` field in the compiled `vcs_diff/toolbar_view.rs` and `vcs_diff/view.rs`, plus a workbench handler. `hunks_overlapping`, `chunk_lines` and `is_index` already exist.
  - `vcs_log_refs`: the log's ref-change detection and its decoration refresh (`logStore.ts`, `GitLog.tsx`).
  - `vcs_log_details_batch`: batched commit details (`logDetails.ts`). The native log reads `vcs_log_details` one commit at a time.
  - `vcs_open_external_log`: read-only logs of other repositories (`RevisionBrowser.tsx`, `historyActions.tsx`); the native log has none.
- **`history_put_label`**: already wired (`workbench/diff_tools.rs`, tested by `put_label_names_the_projects_state_and_shows_in_the_histories_and_recent_changes`).

## 7. Assumptions to check

1. **The editor's second event type.** `Editor` implements both `EventEmitter<EditorEvent>` and `EventEmitter<AnnotationEvent>`. `ProjectTree` does the same with two types, and `cx.subscribe` / `subscribe_in` pick the type from the closure's annotation. No existing `match` on `EditorEvent` changes.
2. **`publish_context` runs when the active tab changes**, so the toggle state follows the active editor. Toggling also calls `publish_context` itself.
3. **The polling after Annotate opens a file.** When the argument names a file that is not active, the action calls `open_project_file`. It then polls `editor()` every 50 ms for up to 10 s, which handles both a new tab and an existing one. In a LightEdit window `open_project_file` does nothing, so the poll runs out and the active editor is annotated.
4. **The column's font.** It uses `ui_family` at `fs-small`, with the colour `fg-secondary` mixed into `fg-gutter` by age and `fg-strong` while hovered. `theme::length(cx, "fs-small")` exists (`everywhere/bookmarks_popup.rs`).
5. **Edit mapping between blames.** It is a line-level approximation: the first line of a change keeps its entry, joined lines lose theirs, added lines have none. The re-blame 800 ms later replaces it.

## 8. Tests most likely to fail first

- `a_click_on_an_entry_shows_its_commit_in_the_git_log`: it depends on the click reaching `Editor::mouse_down` (as the gutter test's left press does), then `run_action_later("vcs.showCommit")`, then the log being read.
- `the_columns_menu_copies_the_revision_and_closes_the_annotations`: typing needs the editor focused, and the re-blame sees the unsaved text through `documents.text_of`.
- `a_new_head_commit_blames_the_file_again`: `git commit --amend --author --date` must move HEAD without touching the file, and `refresh_vcs` must deliver the new `head_commit`.
- `annotate_shows_…`: expected dates come from `format_date` and `utc_offset_minutes` on `git log --format=%at`, never literals. The text's x position is compared to within 0.01 px.
- `resting_on_an_entry_shows_its_commit_author_and_date_in_a_tooltip`: the hover area appears one frame after the first move. The tooltip timer starts on the second move over it, and the test then advances the clock by `TOOLTIP_DELAY`. If GPUI starts the timer only on entering the hitbox, move off the area and back onto it.
