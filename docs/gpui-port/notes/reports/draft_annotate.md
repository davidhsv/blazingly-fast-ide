I wrote the editor's Annotate column and the `vcs.annotate` wiring, but that covers only one of the four parts of item 1, and none of the part-2 commands got built. Nothing has been compiled or run. Every hunk for existing files is in the notes, which also hold the reasons for what I left out.

### 1. Files written

- **`app-gpui/src/editor/view/annotations.rs`** (452 lines): a child module of `editor::view`, so `view.rs` must declare it as `pub mod annotations;`.
  - **Model:** `short_author`, `age_ranks`, `rank_share`, `Annotations::new(&[BlameLine], &DateFormatOptions)`, `Annotations::row`, `Annotations::map(&old_rope, &changes)`, `AnnotationRow { commit, uncommitted, date, author, rank, title, summary }`.
  - **Painting:** `PreparedColumn` (`width`, `paint_row`).
  - **Event:** `AnnotationEvent::Pressed { line }`. `Editor` gets a second `EventEmitter` this way, so no existing `match` on `EditorEvent` changes.
  - **Editor methods:** `annotations_shown`, `set_annotations`, `annotation(line)`, `painted_annotations()`, `annotation_column_width()`, `annotation_line_at(point)`. Hooks for `view.rs`: `press_annotation`, `hover_annotation`, `prepare_annotations`, `annotation_hover_target` (the tooltip area).
  - Dates use `vcs_diff::model::format_date` with the user's Date Formats setting and time zone.
- **`app-gpui/src/workbench/annotate.rs`** (638 lines): `AnnotateSlot` (state per editor, keyed by `EntityId`), `register_actions`, the column's menu (`AnnotationCommand`), re-blame 800 ms after an edit and 100 ms after the head commit moves, and 7 end-to-end tests on a temp repository with two authors and fixed author dates.
- **Notes** (exact hunks, assumptions, risks): `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad/drafts/annotate_integration.md`
  - `editor/view.rs`: the module line, a `Layout` field, an `Editor` field, mapping in `follow()`, `mouse_down`, `mouse_move`/`on_hover`, the hover area in `render`, `Prepared`, `prepaint` and `paint` (the breakpoint dot moves right by the column's width).
  - `workbench/mod.rs`: 5 one-line edits.
  - `branches.rs`: 1 line.
  - `context_menus.rs`: the column's menu intercept, `annotations_shown` (today hard-coded to `false`), and an existing test's `GIT` list that gains "Annotate with Git Blame".

### 2. Actions registered

- **`vcs.annotate`** (toggle): opens the argument's file if it isn't active, then shows or closes the column. Without a file it notifies "Open a file to annotate it"; a failed blame notifies "Cannot annotate" with the backend's message.
  - IPC: `vcs_blame`. A click and the Show Commit row run `vcs.showCommit`, which is already native.
  - Tests: `annotate_shows_the_date_and_author_of_each_line_and_closes_again`, `annotate_without_a_file_asks_for_one_and_with_a_path_opens_and_annotates_it`, `the_gutters_menu_closes_the_annotations`.
  - The column itself is covered by `a_click_on_an_entry_shows_its_commit_in_the_git_log`, `the_columns_menu_copies_the_revision_and_closes_the_annotations` (includes the re-blame after typing), `a_new_head_commit_blames_the_file_again` and `resting_on_an_entry_shows_its_commit_author_and_date_in_a_tooltip`.
  - The toggle state comes from a window context key (`vcsAnnotationsShown`), not from the workbench. The VCS Operations popup is built inside the workbench's own update, so reading the workbench there would panic.
- **Not registered:** `merge.annotate`.

### 3. Spec files

- `diffBlame.test.ts`: 3/3 are covered by the existing model tests only. The diff viewer never calls `diff_blame_target`.
- `inlineBlame.test.ts`: covered only by the existing model tests. Nothing in the app reaches `inline_blame.rs`.
- The column itself has no webview spec. I added 4 model `#[test]`s.

### 4. What is not built, assumptions, likely failures

**Not built:**

- **Annotate in the diff and merge viewers:** both panes paint their own text rather than using an `Editor`, so a column means editing those painters. `merge_view.rs` still has `MergeCommand::Annotate => false`.
- **Inline blame of the caret line:** it needs a hint painted at the line's end, a press on it, and timers on the caret. Off by default (`vcs.git.inlineBlame`).
- **The error path:** "Cannot annotate" has no test; I found no deterministic way to make `vcs_blame` fail while the repository stays available.
- **Tooltip date:** the webview uses `toLocaleString`. Hypothesis: no Rust equivalent exists (from a grep, not a read). I used `format_day` plus `format_clock`, as the list timestamps do.
- **Not part of the column:** `vcs_revision_files` and `vcs_revision_children` (`RevisionBrowser.tsx`, `PathFilterDialog.tsx`), `vcs_block_history` (`SelectionHistory.tsx`).
- **Part 2:**
  - `history_put_label`: already wired natively in `workbench/diff_tools.rs`, with a test.
  - `vcs_stash_drop`: its view, the Commit window's Stashes list, doesn't exist natively.
  - `vcs_delete_tag` / `vcs_restore_tag`: no tag menu natively (a tag row does nothing; `log_window.rs` only logs `ShowTagMenu`).
  - `vcs_stage_hunks`: **this one I should have built and didn't.** Its view exists (the diff toolbar), and the helpers it needs exist (`hunks_overlapping`, `chunk_lines`, `is_index`). It still needs a `DiffCommand` variant and a `can_stage` field in the compiled `toolbar_view.rs` and `view.rs`, plus a workbench handler. I ran out of room for those.
  - Hypothesis (from greps, not reads): `vcs_log_refs`, `vcs_log_details_batch` and `vcs_open_external_log` belong to log internals and read-only external logs that the native log doesn't have. It reads `vcs_log_details` one commit at a time.

**Check first:**

- Hypothesis: `cx.subscribe` and `subscribe_in` pick `AnnotationEvent` from the closure's annotation, as with `ProjectTree`'s two event types.
- Hypothesis: `publish_context` runs when the active tab changes, so the toggle state follows the active editor.
- Hypothesis: the hover area (a plain child element with an id) doesn't stop the editor root from receiving mouse presses and moves.
- Opening a file that isn't active is followed by polling `editor()` every 50 ms for up to 10 s.
- Between blames, entries follow edits line by line: the first line of a change keeps its entry, joined lines lose theirs, added lines get none.

**Likely to fail first:**

- `a_click_on_an_entry…`: depends on the left press reaching `Editor::mouse_down` and on the log being read.
- The tooltip test: depends on when GPUI starts the tooltip timer.
- `a_new_head_commit…`: `git commit --amend --author --date` must move HEAD without touching the file, and `refresh_vcs` must report the new head.
