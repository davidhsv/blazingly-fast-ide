# Editor core and language actions: one integration in two stages

Nothing here was compiled or run. Measured in this session (commands run, output read): every anchor below occurs exactly once in its file in the working tree as it is now (57 anchors, counted by `$S/ed/anchors.py`, re-run after the last edit; `workbench/mod.rs` was being edited by the compiling agent while this was written), and every fenced block of this file was extracted and counted in the target files (each text to replace counts 1, each text to add counts 0); which compiled files changed since the draft commits `8f6cf8d2` / `598adc00` (`git diff --stat`, `git show -U3 e22ffae7`); that no Workbench method of the drafts other than the two named in "Edits" collides with one of another `impl Workbench` (script over `workbench/*.rs`); that every `use crate::…` name in the six drafted files exists (script); the action ids, `when`, `keyWhen` and IntelliJ macOS keys from `app-gpui/assets/gen/actions.json` / `keymaps.json`; that the context keys the conditions read are published by `publish_context`; the fake server's script text. Read, not measured: the signatures the drafts call (spot-checked, listed under "Compile risks"), and everything about what the tests will do.

`$S` = `/private/tmp/claude-502/-Users-davidvieira--claude-personal-spares-spare-BDrKLZ/ef45d052-1f0c-4be6-a975-4f7e63cf4971/scratchpad`.

Order: after the three plugin stages (`docs/gpui-port/notes/integration/plugins_all_integration.md`) and after the context menus (`docs/gpui-port/notes/integration/context_menus_integration.md`). Neither touches an anchor used here (grepped); hunk 1.7 edits `workbench/plugin_contributions.rs`, which plugin stage 3 declares, so it needs that stage first. Each stage below leaves the build green on its own.

## 0. The drafted files

The six drafted files are committed but undeclared: `app-gpui/src/editor/{folds,core_actions,paste_history_view}.rs`, `app-gpui/src/workbench/{editor_core,editor_language}.rs`, `app-gpui/src/editor_lsp/definition_view.rs`. Three were edited in this reconciliation (section 4); the final versions are kept byte-identical under `$S/ed/files/app-gpui/src/…`. Before declaring anything run `git status --short app-gpui/src/editor app-gpui/src/workbench app-gpui/src/editor_lsp`: if `workbench/editor_core.rs`, `workbench/editor_language.rs` or `editor/core_actions.rs` is NOT shown as modified, a restore put back the committed draft; copy the three files from `$S/ed/files/app-gpui/src/` over them (and `cmp` all six) before going on.

## Stage 1: editor core actions

Declares `editor/folds.rs`, `editor/core_actions.rs`, `editor/paste_history_view.rs`, `workbench/editor_core.rs`. Hunks: 46 edit sites in 7 files (`editor/mod.rs` 3, `editor/actions.rs` 1, `workbench/mod.rs` 5 with its `mod` line, `workbench/editor_intelligence.rs` 1, `editor/view.rs` 34, `editor/find_bar.rs` 1, `workbench/plugin_contributions.rs` 1). Of the original notes' 12 edits outside `view.rs`: 9 apply as written (re-counted: the 4 `mod` lines, `actions.rs`, `mod.rs` hunks 1, 2, 3 and 6), 1 is dropped (old hunk 4, removing `editor.closeAllTabs` from `mod.rs`: the build already has the new behaviour, and the draft no longer registers it), 2 are replaced by one (old hunks 5 and the `tab_views.rs` one, which put `install_editor_core` next to the two `install_intelligence` calls: it goes inside `install_intelligence` instead, one site, away from the `if still_open {` lines the compiling agent and plugin stage 3.4.5 edit). The `view.rs` hunks V1–V10 are all re-derived against today's file (34 edit sites, section 3); 6 conversion sites are new, which the draft missed: 4 in `view.rs` (`block_point_at` and the ⌥-drag column selection, both from `e22ffae7`; the triple click; the painted row count) and 2 outside (`find_bar.rs`, `plugin_contributions.rs`). Two more fixes are in the drafted `core_actions.rs` (the scroll clamp after folding, the public row accessors). V8 is no longer a whole-function replacement: it is four anchored hunks, so an unrelated edit inside `prepaint` does not break it.

### 1.1 `mod` lines

`app-gpui/src/editor/mod.rs`, after `pub mod command_actions;`:

```rust
pub mod core_actions;
```

after `pub mod find_model;`:

```rust
pub mod folds;
```

after `pub mod nav_history;`:

```rust
pub mod paste_history_view;
```

`app-gpui/src/workbench/mod.rs`, after `mod editor_intelligence;`:

```rust
mod editor_core;
```

### 1.2 `app-gpui/src/editor/actions.rs`

In `pub fn register`, after `    super::command_actions::register(cx);`:

```rust
    super::core_actions::register(cx);
```

### 1.3 `app-gpui/src/workbench/mod.rs`

1. Field. After `    light_find_bar: Option<TabFindBar>,` (struct `Workbench`):

```rust
    /// Where the caret has been in the window's editors: Back / Forward and the edit locations.
    editor_core: editor_core::EditorCore,
```

2. Initialiser. In `Workbench::new`, after `            light_find_bar: None,`:

```rust
            editor_core: Default::default(),
```

3. Registration. In `pub fn register_actions`, after `    project_files::register_actions(registry);`:

```rust
    editor_core::register_actions(registry);
```

`editor_core::register_actions` no longer registers `editor.closeAllTabs` (the `mod.rs` one stays, with its test) nor `editor.copyReference` (the `project_files.rs` one stays). It registers `editor.showHint`, which nothing registers today: `project_files.rs` `run_flow_action` then routes the Copy Path popup's and Copy Reference's hint through it (a notification keyed `editor.hint`, same title) instead of its fallback (a notification keyed `project.copyPath`). The titles the existing tests read are unchanged.

4. Context key of the Paste from History dialog (`when: pasteHistoryOpen`). In `fn publish_context`, after `        set_context(window, cx, "findBarHasResults", focused_bar.is_some_and(|bar| bar.read(cx).has_results()));`:

```rust
        set_context(window, cx, "pasteHistoryOpen", self.paste_history_dialog().is_some());
```

`pasteHistoryListFocus` (the `keyWhen` of `editor.pasteHistory.paste` / `.remove`, keys Enter / Delete / Backspace) is deliberately never set: the keyboard stays in the dialog's filter field, which claims Enter, Delete and an empty-filter Backspace itself (`input::claim_keys`). With the key on, Backspace in the filter would delete an entry; `paste_from_history_pastes_the_entry_chosen_in_the_dialog` checks it does not.

### 1.4 `app-gpui/src/workbench/editor_intelligence.rs` (follow every editor for the histories)

In `install_intelligence`, after `        let intelligence = EditorIntelligence::install(editor, cx);`:

```rust
        self.install_editor_core(editor, window, cx);
```

`install_intelligence` is called for every editor the window makes: a project tab (`workbench/mod.rs`, the file-opening task) and a single-file window (`workbench/tab_views.rs`, `open_loose_file`); grepped, no other caller.

### 1.5 `app-gpui/src/editor/view.rs`

These give the editor its `core` state, make every line↔pixel/row conversion go through the fold row map, paint the fold placeholder and the closed-fold chevron, and make the metrics follow the font zoom. A row is a line no fold hides; with nothing folded `row_of_line` and `line_of_row` are the identity, `row_count` is `line_count`, and every hunk computes what the code computes today.

#### V1 Field and initialiser

After

```rust
    /// Counts edits, so that a highlight request an edit has overtaken is dropped.
    generation: u64,
    created: std::time::Instant,
}
```

replace the last `}` so that it reads

```rust
    /// Counts edits, so that a highlight request an edit has overtaken is dropped.
    generation: u64,
    created: std::time::Instant,
    /// Folding, Extend / Shrink Selection and the font zoom ([`super::core_actions`]).
    pub(super) core: super::core_actions::CoreState,
}
```

After `            created: std::time::Instant::now(),` add

```rust
            core: Default::default(),
```

#### V2 `Layout`: painted rows know their line

Replace

```rust
    line_height: Pixels,
    first_line: u32,
    lines: Vec<(String, ShapedLine)>,
}
```

with

```rust
    line_height: Pixels,
    /// The first painted row (a row is a line no fold hides).
    first_line: u32,
    lines: Vec<(String, ShapedLine)>,
    /// The document line of each painted row.
    line_of: Vec<u32>,
}
```

#### V3 Metrics follow the font zoom; the editor's new methods

Replace

```rust
    fn metrics(cx: &App) -> (Pixels, Pixels, Pixels) {
        (theme::length(cx, "fs-code"), theme::length(cx, "lh-code"), theme::length(cx, "w-gutter"))
    }
```

with

```rust
    /// The font size, line height and gutter width the editor paints with: the theme's, the font
    /// and the line height scaled by the editor's font zoom.
    fn metrics(&self, cx: &App) -> (Pixels, Pixels, Pixels) {
        let (size, line) = (theme::length(cx, "fs-code"), theme::length(cx, "lh-code"));
        let zoomed = (size + px(self.core.font_zoom as f32)).max(px(1.0));
        (zoomed, line * (zoomed / size), theme::length(cx, "w-gutter"))
    }

    /// The font size the editor paints with.
    pub fn font_size(&self, cx: &App) -> Pixels {
        self.metrics(cx).0
    }

    /// Makes the editor accept edits or refuse them, as its file now is.
    pub fn set_read_only(&mut self, read_only: bool, cx: &mut Context<Self>) {
        if self.read_only != read_only {
            self.read_only = read_only;
            cx.notify();
        }
    }

    /// Scroll to Center: the main caret's row goes to the middle of the view.
    pub fn scroll_caret_to_center(&mut self, cx: &mut Context<Self>) {
        let line_height = self.metrics(cx).1;
        let viewport = self.layout.as_ref().map_or(px(0.0), |layout| layout.bounds.size.height);
        let row = self.core.folds.row_of_line(self.caret_position().0);
        let content = line_height * self.core.folds.row_count(self.line_count()) as f32;
        let top = line_height * row as f32 + line_height / 2.0 - viewport / 2.0;
        self.scroll_top = top.clamp(px(0.0), (content - viewport).max(px(0.0)));
        cx.notify();
    }
```

`Self::metrics(cx)` occurs 6 times today and `Editor::metrics(cx)` 3 times (2 in `prepaint`/`paint`, 1 in a test); each is rewritten by a hunk below (V4a, V4b, V4d, V4e, V4f, V4g, V8b, V9a, V10). After stage 1, `grep -n "::metrics(" app-gpui/src/editor/view.rs` must print nothing.

#### V4 Rows in the scroll arithmetic

V4a `top_line`: replace `        (self.scroll_top / Self::metrics(cx).1).floor() as u32` with

```rust
        self.core.folds.line_of_row((self.scroll_top / self.metrics(cx).1).floor() as u32)
```

V4b `scroll_to_line`: replace `        self.scroll_top = Self::metrics(cx).1 * line as f32;` with

```rust
        self.scroll_top = self.metrics(cx).1 * self.core.folds.row_of_line(line) as f32;
```

V4c `painted_lines`, and the painted row count. Replace

```rust
    /// The lines the last frame shaped: the first one's index and how many.
    pub fn painted_lines(&self) -> Option<Range<u32>> {
        self.layout.as_ref().map(|l| l.first_line..l.first_line + l.lines.len() as u32)
    }
```

with

```rust
    /// The document lines the last frame painted, from the first to the last (the lines a fold
    /// hides between them included).
    pub fn painted_lines(&self) -> Option<Range<u32>> {
        let layout = self.layout.as_ref()?;
        Some(layout.line_of.first().copied().unwrap_or(0)..layout.line_of.last().map_or(0, |last| last + 1))
    }

    /// How many rows the last frame painted.
    pub fn painted_rows(&self) -> u32 {
        self.layout.as_ref().map_or(0, |layout| layout.lines.len() as u32)
    }
```

V4d `line_height`: replace

```rust
    pub fn line_height(&self, cx: &App) -> Pixels {
        Self::metrics(cx).1
    }
```

with

```rust
    pub fn line_height(&self, cx: &App) -> Pixels {
        self.metrics(cx).1
    }
```

V4e `reveal_caret`: replace

```rust
        let (_, line_height, _) = Self::metrics(cx);
        let line_top = line_height * self.caret_position().0 as f32;
```

with

```rust
        let (_, line_height, _) = self.metrics(cx);
        let line_top = line_height * self.core.folds.row_of_line(self.caret_position().0) as f32;
```

V4f `scroll_by`: replace

```rust
        let (_, line_height, _) = Self::metrics(cx);
        let viewport = self.layout.as_ref().map_or(px(0.0), |l| l.bounds.size.height);
        let content = line_height * self.line_count() as f32;
```

with

```rust
        let (_, line_height, _) = self.metrics(cx);
        let viewport = self.layout.as_ref().map_or(px(0.0), |l| l.bounds.size.height);
        let content = line_height * self.core.folds.row_count(self.line_count()) as f32;
```

V4g `page_lines` (Page Up / Page Down move by a viewport of rows; V6b skips the hidden lines): replace `        (((viewport / Self::metrics(cx).1).floor() as i64) - 1).max(1)` with

```rust
        (((viewport / self.metrics(cx).1).floor() as i64) - 1).max(1)
```

#### V5 Selections open folds, edits move them, and the folds are asked again

V5a `set_selections`: replace

```rust
        self.put_selections(ranges, main);
        self.history.seal();
```

with

```rust
        self.put_selections(ranges, main);
        // A caret inside a fold opens it: no caret is hidden.
        let heads: Vec<u32> = self.selections.iter().map(|range| range.head()).collect();
        let state = self.edit_state();
        self.core.folds.reveal(&state, &heads);
        self.history.seal();
```

V5b `follow` (every change of the text goes through it: typing, undo, the backend's edits, Reload): replace

```rust
        self.put_selections(ranges, main);
        self.marked = None;
```

with

```rust
        self.put_selections(ranges, main);
        let after = self.edit_state();
        self.core.folds.map(&after, &changes);
        let heads: Vec<u32> = self.selections.iter().map(|range| range.head()).collect();
        self.core.folds.reveal(&after, &heads);
        self.marked = None;
```

V5c `refresh_highlights`: replace

```rust
        if self.large {
            self.highlights.clear();
            return;
        }
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
```

with

```rust
        if self.large {
            self.highlights.clear();
            return;
        }
        super::core_actions::refresh_folds(self, cx);
        let (host, window, doc) = (self.host.clone(), self.window_label.clone(), self.doc);
```

`refresh_highlights` runs when the editor is made and after every followed change. `refresh_folds` waits 40 ms of stillness and drops an answer a newer request or a newer text overtook. A large document gets no folds (as it gets no highlights).

#### V6 Caret movement

V6a `key_down`, ← / → step over a folded region as one character. Replace

```rust
            "left" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() { range.start() } else { editor.previous_offset(range.head()) }
            }),
            "right" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() { range.end() } else { editor.next_offset(range.head()) }
            }),
```

with

```rust
            "left" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() {
                    range.start()
                } else {
                    editor.core.folds.step(range.head(), editor.previous_offset(range.head()))
                }
            }),
            "right" => self.move_carets(extend, cx, |editor, range| {
                if !extend && !range.is_caret() {
                    range.end()
                } else {
                    editor.core.folds.step(range.head(), editor.next_offset(range.head()))
                }
            }),
```

V6b `vertical` (↑ / ↓ and Page Up / Page Down), the main caret skips hidden lines. Replace

```rust
        for _ in 0..lines.unsigned_abs() {
            let Some(motion) = step(&state, &goals) else { break };
            state = commands::apply(&state, &motion.edit);
            goals = motion.goals;
            moved = true;
        }
```

with

```rust
        for _ in 0..lines.unsigned_abs() {
            let Some(motion) = step(&state, &goals) else { break };
            state = commands::apply(&state, &motion.edit);
            goals = motion.goals;
            moved = true;
            // A line inside a fold is not on screen: the caret goes on to the next line shown.
            while self.core.folds.is_hidden(state.line_at(state.main().head()).number) {
                let before = state.main().head();
                let Some(motion) = step(&state, &goals) else { break };
                state = commands::apply(&state, &motion.edit);
                goals = motion.goals;
                if state.main().head() == before {
                    break;
                }
            }
        }
```

V6c `mouse_down`, the triple click selects the row: a folded header's hidden lines go with it (ending at `line + 1` would put the head inside the fold and V5a would open it). Replace

```rust
                // The whole line with its break, so that typing replaces the line.
                let end = if line + 1 < self.line_count() {
                    buffer::line_range(&self.rope, line + 1).start
                } else {
                    range.end
                };
```

with

```rust
                // The whole line with its break, so that typing replaces the line; the lines a fold
                // hides go with their header.
                let next = self.core.folds.line_of_row(self.core.folds.row_of_line(line) + 1);
                let end = if next < self.line_count() { buffer::line_range(&self.rope, next).start } else { range.end };
```

#### V7 Hit-testing and positions by row

V7a `position_of` (the caret's pixel position: input-method bounds `bounds_for_range`, every popup anchored at an offset, the context menu's gutter test, the plugin gutter layer). Replace the whole function

```rust
    pub fn position_of(&self, offset: u32) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let (line, col) = buffer::line_col(&self.rope, offset);
        let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
        let x = shaped.x_for_index(buffer::byte_of_col(text, col));
        let y = layout.line_height * line as f32 - self.scroll_top;
        Some(point(layout.bounds.left() + layout.text_left + x, layout.bounds.top() + y))
    }
```

with

```rust
    pub fn position_of(&self, offset: u32) -> Option<Point<Pixels>> {
        let layout = self.layout.as_ref()?;
        let (line, col) = buffer::line_col(&self.rope, offset);
        let row = self.core.folds.row_of_line(line);
        let index = row.checked_sub(layout.first_line)? as usize;
        // A hidden line is not painted.
        if layout.line_of.get(index) != Some(&line) {
            return None;
        }
        let (text, shaped) = layout.lines.get(index)?;
        let x = shaped.x_for_index(buffer::byte_of_col(text, col));
        let y = layout.line_height * row as f32 - self.scroll_top;
        Some(point(layout.bounds.left() + layout.text_left + x, layout.bounds.top() + y))
    }
```

V7b `offset_at` (clicks, drags, `character_index_for_point`, `block_point_at`). Replace

```rust
        let line = ((y / layout.line_height) as u32).min(self.line_count().saturating_sub(1));
        let x = position.x - layout.bounds.left() - layout.text_left;
        let col = match layout.lines.get(line.checked_sub(layout.first_line)? as usize) {
```

with

```rust
        let rows = self.core.folds.row_count(self.line_count());
        let row = ((y / layout.line_height) as u32).min(rows.saturating_sub(1));
        let line = self.core.folds.line_of_row(row);
        let x = position.x - layout.bounds.left() - layout.text_left;
        let col = match layout.lines.get(row.checked_sub(layout.first_line)? as usize) {
```

V7c `gutter_line_at` (gutter presses: breakpoints, `EditorEvent::GutterPressed`). Replace

```rust
        let line = (y / layout.line_height) as u32;
        (line < self.line_count()).then_some(line)
```

with

```rust
        let line = self.core.folds.line_of_row((y / layout.line_height) as u32);
        (line < self.line_count()).then_some(line)
```

V7d the ⌥-drag column selection in `mouse_move`: a line a fold hides gets no range. Replace

```rust
            let ranges = commands::column_mode::rectangle_ranges(&self.edit_state(), start, head);
```

with

```rust
            let mut ranges = commands::column_mode::rectangle_ranges(&self.edit_state(), start, head);
            // The block covers the rows on screen: a range on a line a fold hides would open the fold.
            ranges.retain(|range| !self.core.folds.is_hidden(buffer::line_col(&self.rope, range.head()).0));
```

V7e `block_point_at` (the painted row of the block's line, for the columns right of its end). Replace

```rust
        let painted = line.checked_sub(layout.first_line).and_then(|index| layout.lines.get(index as usize));
```

with

```rust
        let painted = layout.line_of.iter().position(|shown| *shown == line).and_then(|index| layout.lines.get(index));
```

V7f `text_offset_at` (hover, the navigation modifier). Replace

```rust
        let line = (y / layout.line_height) as u32;
        let (text, shaped) = layout.lines.get(line.checked_sub(layout.first_line)? as usize)?;
```

with

```rust
        let row = (y / layout.line_height) as u32;
        let index = row.checked_sub(layout.first_line)? as usize;
        let (text, shaped) = layout.lines.get(index)?;
        let line = *layout.line_of.get(index)?;
```

#### V8 `Prepared` and `prepaint` (four hunks)

V8a Replace

```rust
struct Prepared {
    layout: Layout,
    numbers: Vec<ShapedLine>,
    gutter: Pixels,
    /// The marks that touch the painted lines.
    marks: Vec<Mark>,
}
```

with

```rust
struct Prepared {
    layout: Layout,
    numbers: Vec<ShapedLine>,
    gutter: Pixels,
    /// The marks that touch the painted lines.
    marks: Vec<Mark>,
    /// Per painted row: the placeholder of the fold its line starts, and the text after the fold.
    placeholders: Vec<Option<(ShapedLine, ShapedLine)>>,
    /// Per painted row: the chevron of a folded line in the gutter.
    markers: Vec<Option<ShapedLine>>,
}
```

V8b The rows in view. Replace

```rust
        let (font_size, line_height, gutter) = Editor::metrics(cx);
        let code_font = Editor::code_font(cx);
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let match_color = theme::color(cx, "fg-search-match");
        let editor = self.editor.read(cx);
        let total = editor.line_count();
        let first_line = ((editor.scroll_top / line_height).floor() as u32).min(total.saturating_sub(1));
        let visible = (bounds.size.height / line_height).ceil() as u32 + 1;
        let last_line = (first_line + visible).min(total);
```

with

```rust
        let editor = self.editor.read(cx);
        let (font_size, line_height, gutter) = editor.metrics(cx);
        let code_font = Editor::code_font(cx);
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let match_color = theme::color(cx, "fg-search-match");
        let placeholder_color = theme::color(cx, "fg-fold-placeholder");
        let marker_color = theme::color(cx, "fg-fold-marker");
        let total = editor.line_count();
        // The frame paints the rows in view; a row is a line no fold hides.
        let folds = &editor.core.folds;
        let rows = folds.row_count(total);
        let first_row = ((editor.scroll_top / line_height).floor() as u32).min(rows.saturating_sub(1));
        let visible = (bounds.size.height / line_height).ceil() as u32 + 1;
        let last_row = (first_row + visible).min(rows);
        let painted: Vec<u32> =
            (first_row..last_row).map(|row| folds.line_of_row(row)).filter(|line| *line < total).collect();
        let first_line = painted.first().copied().unwrap_or(0);
        let last_line = painted.last().map_or(0, |line| line + 1);
        // A folded header's placeholder reads the text and its comments; with nothing folded
        // neither is needed.
        let state = (!folds.folded().is_empty()).then(|| editor.edit_state());
```

V8c The marks stay as they are (they are filtered by the offsets of the first and last painted line, hidden lines between included); the highlight spans are looked up per row. Replace

```rust
        let mut span = editor.highlights.partition_point(|s| s.to <= first_offset);
        let mut lines = Vec::with_capacity((last_line - first_line) as usize);
        let mut numbers = Vec::with_capacity(lines.capacity());
        for line in first_line..last_line {
            let range = buffer::line_range(&editor.rope, line);
            let text = buffer::slice(&editor.rope, range.start, range.end);
            let mut runs = Vec::new();
            let mut at = 0usize;
```

with

```rust
        let mut lines = Vec::with_capacity(painted.len());
        let mut numbers = Vec::with_capacity(painted.len());
        let mut placeholders = Vec::with_capacity(painted.len());
        let mut markers = Vec::with_capacity(painted.len());
        for &line in &painted {
            let range = buffer::line_range(&editor.rope, line);
            let text = buffer::slice(&editor.rope, range.start, range.end);
            let mut runs = Vec::new();
            let mut at = 0usize;
            // Spans are sorted; the lines between two rows can be hidden, so each row looks its
            // first span up.
            let mut span = editor.highlights.partition_point(|s| s.to <= range.start);
```

and replace the comment line `        // Spans are sorted: skip to the first one that can touch the first visible line.` (just above `let first_offset`) with

```rust
        // The marks that can touch the painted lines.
```

V8d The placeholder, the chevron and the layout. Replace

```rust
                &[run(label.len(), number_color)],
                None,
            ));
        }
        Prepared {
            layout: Layout { bounds, text_left: gutter, line_height, first_line, lines },
            numbers,
            gutter,
            marks,
        }
```

with

```rust
                &[run(label.len(), number_color)],
                None,
            ));
            // A folded header shows a placeholder (a custom region's name), then what of the fold's
            // last line comes after it; its gutter shows the closed fold's chevron.
            let folded = state.as_ref().and_then(|state| folds.folded_on(state, line).map(|region| (state, region)));
            placeholders.push(folded.map(|(state, (from, to))| {
                let comments = |pos: u32| editor.facts().comments_at(pos);
                let name = crate::editor_commands::regions::region_name_at(state, &comments, from)
                    .unwrap_or_else(|| super::folds::PLACEHOLDER.to_owned());
                let tail_end = buffer::line_range(&editor.rope, buffer::line_col(&editor.rope, to).0).end;
                let tail = buffer::slice(&editor.rope, to, tail_end);
                let shape = |text: String, color: Hsla| {
                    let len = text.len();
                    window.text_system().shape_line(text.into(), font_size, &[run(len, color)], None)
                };
                (shape(name, placeholder_color), shape(tail, plain))
            }));
            markers.push(folded.map(|_| {
                let chevron = "›";
                window.text_system().shape_line(chevron.into(), font_size, &[run(chevron.len(), marker_color)], None)
            }));
        }
        Prepared {
            layout: Layout { bounds, text_left: gutter, line_height, first_line: first_row, lines, line_of: painted },
            numbers,
            gutter,
            marks,
            placeholders,
            markers,
        }
```

`region_name_at(state: &EditState, comments: CommentTokensAt<'_>, pos: u32)` with `CommentTokensAt<'a> = &'a dyn Fn(u32) -> Vec<CommentTokens>` (`editor_commands/mod.rs:478`): `&comments` coerces. `folded` is `Option<(&EditState, (u32, u32))>`, which is `Copy`, so it is read twice.

#### V9 `paint`

V9a The scroll listener. Replace `                let line_height = Editor::metrics(cx).1;` with

```rust
                let line_height = editor.read(cx).metrics(cx).1;
```

V9b The destructuring. Replace `        let Prepared { layout, numbers, gutter, marks } = prepared;` with

```rust
        let Prepared { layout, numbers, gutter, marks, placeholders, markers } = prepared;
```

V9c The row's top and its line. This one change moves, with the row, everything the loop paints per line: the current-line band, the breakpoint and execution-line tints, the find / usage / problem backgrounds, the selections (column selections included: one range per line), the input-method underline, the text, the diagnostics' wavy underlines, the struck-out excluded occurrences, the line number, the breakpoint dot and the carets. Replace

```rust
                let line = layout.first_line + index as u32;
                let top = bounds.top() + line_height * line as f32 - scroll_top;
```

with

```rust
                let line = layout.line_of[index];
                let row = layout.first_line + index as u32;
                let top = bounds.top() + line_height * row as f32 - scroll_top;
```

V9d The placeholder and the chevron. After `                let _ = shaped.paint(text_origin, line_height, TextAlign::Left, None, window, cx);` add

```rust
                if let Some((placeholder, tail)) = placeholders.get(index).and_then(Option::as_ref) {
                    let origin = point(text_origin.x + shaped.width + px(4.0), top);
                    let chip = Bounds::new(origin, size(placeholder.width + px(8.0), line_height));
                    let background = theme::color(cx, "bg-fold-placeholder");
                    let solid = gpui_kit::BorderStyle::Solid;
                    window.paint_quad(quad(chip, px(3.0), background, px(0.0), background, solid));
                    let at = point(origin.x + px(4.0), top);
                    let _ = placeholder.paint(at, line_height, TextAlign::Left, None, window, cx);
                    let after = point(origin.x + chip.size.width + px(2.0), top);
                    let _ = tail.paint(after, line_height, TextAlign::Left, None, window, cx);
                }
                if let Some(marker) = markers.get(index).and_then(Option::as_ref) {
                    let x = bounds.left() + *gutter - px(10.0);
                    let _ = marker.paint(point(x, top), line_height, TextAlign::Left, None, window, cx);
                }
```

V9e The layout kept for hit-testing. Replace `            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new() },` with

```rust
            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new(), line_of: Vec::new() },
```

#### V10 The view's own test

In `mod tests` of `view.rs` (`a_long_file_costs_a_viewport_per_frame`), replace `(layout.bounds.size.height / Editor::metrics(cx).1).ceil() as u32)` with `(layout.bounds.size.height / e.line_height(cx)).ceil() as u32)`.

### 1.6 `app-gpui/src/editor/find_bar.rs` (`select_in`: Find centres its match by rows)

Replace

```rust
    let Some((line, painted)) = center.zip(editor.painted_lines()) else { return };
    let visible = painted.end.saturating_sub(painted.start);
    let last_top = editor.line_count().saturating_sub(visible.saturating_sub(1).max(1));
    editor.scroll_to_line(line.saturating_sub(visible / 2).min(last_top), cx);
```

with

```rust
    let Some((line, _)) = center.zip(editor.painted_lines()) else { return };
    // In rows: the lines a fold hides take none.
    let visible = editor.painted_rows();
    let last_top = editor.row_count().saturating_sub(visible.saturating_sub(1).max(1));
    let top = editor.row_of_line(line).saturating_sub(visible / 2).min(last_top);
    editor.scroll_to_line(editor.line_of_row(top), cx);
```

`row_of_line`, `line_of_row`, `row_count` are the public `Editor` methods in `core_actions.rs`; `painted_rows` is V4c. Without folds this is today's arithmetic.

### 1.7 `app-gpui/src/workbench/plugin_contributions.rs` (`plugin_gutter_layer`, declared by plugin stage 3)

Replace

```rust
            // The editor's top: where its first painted line would start unscrolled.
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * first as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * (painted.end - painted.start).saturating_sub(1) as f32);
```

with

```rust
            // The editor's top: where its first painted row would start unscrolled.
            let top = editor
                .position_of(editor.offset_of(first, 0))
                .map(|at| at.y - line_height * editor.row_of_line(first) as f32 + editor.scroll_top());
            let bottom = top.map(|top| top + line_height * editor.painted_rows().saturating_sub(1) as f32);
```

The markers themselves are placed with `position_of`, which answers `None` for a hidden line (V7a), so a plugin's gutter icon on a folded line is not drawn. If plugin stage 3 changed these lines, re-derive the same two substitutions (row of the first painted line, painted row count).

## Stage 2: editor language actions

Declares `workbench/editor_language.rs` and `editor_lsp/definition_view.rs`. Hunks: 13 edit sites in 6 files (`editor_lsp/mod.rs` 1, `workbench/mod.rs` 6 with its `mod` line, `workbench/editor_intelligence.rs` 1, `editor/intelligence.rs` 3, `editor/marks.rs` 1, `editor/actions.rs` 1), plus one optional `view.rs` hunk not applied (2.7). Of the original notes' sections: 1, 2a–2e, 5, 6 and 7 apply (re-counted; 2a's `mod` line is now anchored on stage 1's `mod editor_core;`, and 3a moves next to stage 1's line in `install_intelligence`); 3b (a quick fix's → menu control runs) and 4 (`apply_inspection` made `pub(super)`) are dropped because commit `e22ffae7` put both in the build (`editor_intelligence.rs` now calls `workbench.apply_inspection(command, …)` for a control with a `run`, and `code_insight.rs` has `pub(super) fn apply_inspection`); the draft's `run_intention_control` went with them. One hunk is new: the fake language server's capabilities (2.6), replacing the draft's test-time rewrite of the server script.

### 2.1 `mod` lines

`app-gpui/src/editor_lsp/mod.rs`, after `pub mod configuration_navigation;`:

```rust
pub mod definition_view;
```

`app-gpui/src/workbench/mod.rs`, after stage 1's `mod editor_core;`:

```rust
mod editor_language;
```

### 2.2 `app-gpui/src/workbench/mod.rs`

1. The Edit › Undo entry points. After

```rust
pub fn active_editor(window: &Window, cx: &App) -> Option<Entity<Editor>> {
    for_window(window, cx)?.read(cx).editor().cloned()
}
```

add

```rust

pub(crate) use editor_language::{redo_refactoring, undo_refactoring};
```

2. Field. In `pub struct Workbench`, after

```rust
    /// The dialog a Git operation waits on.
    vcs_dialog: Option<branches::VcsDialog>,
```

add

```rust
    /// The editor language actions: the refactorings undoable as one step, the highlighted usages,
    /// the rename waiting for its edit.
    language: editor_language::EditorLanguage,
```

3. Initialiser. In `Workbench::new`, after `            vcs_dialog: None,`:

```rust
            language: Default::default(),
```

4. Registration. In `pub fn register_actions`, after `    code_insight::register_actions(registry);`:

```rust
    editor_language::register_actions(registry);
```

5. The refactoring notification's Undo / Redo button. In `fn run_notification_action`, before `            EntryAction::Own(own) if own.command.starts_with("vcs.deleted.") => {` add

```rust
            EntryAction::Own(own) if own.command == editor_language::WORKSPACE_UNDO_COMMAND => {
                self.run_language_notification(&own.command, cx);
            }
```

### 2.3 `app-gpui/src/workbench/editor_intelligence.rs`

In `install_intelligence`, after stage 1's `        self.install_editor_core(editor, window, cx);`:

```rust
        self.watch_workspace_edits(cx);
```

### 2.4 `app-gpui/src/editor/intelligence.rs`: hints with a place, a tone and a timeout

After the method `    fn show_hint_text(&mut self, text: &str, cx: &mut Context<Self>) { … }` (it ends `        self.hint_timer(timer, cx);\n    }`) add

```rust

    /// Shows `text` (English source text) at `pos`, the caret when `None`, with `tone`, for
    /// `timeout_ms` (the hint's usual time when `None`).
    pub fn show_hint_at(
        &mut self,
        text: &str,
        pos: Option<u32>,
        tone: Tone,
        timeout_ms: Option<u64>,
        cx: &mut Context<Self>,
    ) {
        let caret = self.editor.upgrade().map_or(0, |editor| editor.read(cx).caret());
        let timer = self.hints.show(text, pos.unwrap_or(caret), tone, timeout_ms);
        self.hint_timer(timer, cx);
    }
```

`Tone` is imported there (`use crate::editor_lsp::hint::{HintState, HintTimer, PENDING_DELAY_MS, Tone};`); `HintState::show(&mut self, text: &str, pos: u32, tone: Tone, timeout_ms: Option<u64>) -> HintTimer` (`editor_lsp/hint.rs:81`).

### 2.5 `app-gpui/src/editor/marks.rs`

In `pub enum MarkLayer`, after

```rust
    /// The line where the debugger is suspended.
    Execution,
```

add

```rust
    /// The usages Highlight Usages in File marks, until Escape.
    Usages,
```

No `match` over `MarkLayer` exists (grepped); the painter draws every layer (`editor.marks.values().flatten()`), and the usages are `MarkKind::FindOccurrence` marks.

### 2.6 `app-gpui/src/editor/intelligence.rs`: the fake server offers what a test's answers add (test fixture)

In `mod fake_server`, in `SERVER`, after `            'codeActionProvider': {'resolveProvider': True},` add

```python
            **script().get('capabilities', {}),
```

and replace the doc lines

```rust
        /// (by the request's position); `delays`: method → seconds per call; `diagnostics`: published
        /// on every change.
```

with

```rust
        /// (by the request's position); `delays`: method → seconds per call; `diagnostics`: published
        /// on every change; `capabilities`: added to the server's own when it starts.
```

No existing answers file has a `capabilities` key (grepped: only `editor_language.rs` writes one), so every existing test's server is unchanged. `script()` is defined above the loop, and `{…, **mapping,}` is a Python 3.5+ dict display. The capability is read at `initialize`, so a test sets `capabilities` in the answers it gives `open` (the draft's tests do).

### 2.7 `app-gpui/src/editor/actions.rs`: one Undo takes a refactoring back

Replace

```rust
    registry.handle("edit.undo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
```

with

```rust
    registry.handle("edit.undo", |_, window, cx| {
        // With nothing of its own left to undo, the editor undoes the refactoring of its file.
        if workbench::undo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.undo(cx);
        })
    });
    registry.handle("edit.redo", |_, window, cx| {
        if workbench::redo_refactoring(window, cx) {
            return Ok(());
        }
        on_editor(window, cx, |editor, cx| {
            editor.redo(cx);
        })
    });
```

A workspace edit reaches an open editor as `DocPatched { workspace_edit: true }`, and `Editor::remote_patch` clears the editor's history, so Undo after a rename had nothing to undo. The webview undoes the whole edit through `undo_workspace_edit` from any affected editor (`src/editor/core/model/workspaceHistory.ts`).

Not applied, offered: ⌘Z / ⇧⌘Z are not in the keymap (`edit.undo` has no binding in `keymaps.json`); the editor's own `key_down` runs `self.undo(cx)` and stops there, so the key does not reach a refactoring. To make it, replace in `view.rs` `key_down`

```rust
                ("z", false) => Some(self.undo(cx)),
                ("z", true) => Some(self.redo(cx)),
```

with

```rust
                ("z", redo) => {
                    let done = if redo { self.redo(cx) } else { self.undo(cx) };
                    // With nothing of its own left, the window's Undo / Redo takes the refactoring back.
                    if !done {
                        let id = if redo { "edit.redo" } else { "edit.undo" };
                        window.defer(cx, move |window, cx| {
                            crate::platform::actions::execute_action(id, None, window, cx);
                        });
                    }
                    Some(done)
                }
```

It needs a test that presses `cmd-z` in `rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each` instead of running `edit.undo`; it is left out of the stage because that test is the one most likely to fail first.

## 3. Every line↔pixel/row conversion in `editor/view.rs` (34 sites)

Measured by `grep -n "first_line\|layout.lines\|metrics(cx)\|line_height \*\|line_count()\|scroll_top\|Layout {"` over the non-test part and by reading every function that reads `layout`. "Draft" says whether the original V1–V10 covered it.

| #   | Site                                            | What converts                                                                                                                                                                                                                                    | Hunk                          | Draft                |
| --- | ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------- | -------------------- |
| 1   | `metrics`                                       | line height, font size (zoom)                                                                                                                                                                                                                    | V3                            | yes                  |
| 2   | `top_line`                                      | scroll → first line                                                                                                                                                                                                                              | V4a                           | yes                  |
| 3   | `scroll_to_line`                                | line → scroll                                                                                                                                                                                                                                    | V4b                           | yes                  |
| 4   | `painted_lines`                                 | layout → line range                                                                                                                                                                                                                              | V4c                           | yes                  |
| 5   | `painted_rows` (new)                            | layout → row count                                                                                                                                                                                                                               | V4c                           | no                   |
| 6   | `line_height`                                   | metrics                                                                                                                                                                                                                                          | V4d                           | yes                  |
| 7   | `reveal_caret`                                  | caret line → scroll                                                                                                                                                                                                                              | V4e                           | yes                  |
| 8   | `scroll_by`                                     | content height in rows                                                                                                                                                                                                                           | V4f                           | yes                  |
| 9   | `page_lines`                                    | viewport → rows                                                                                                                                                                                                                                  | V4g                           | yes                  |
| 10  | `scroll_caret_to_center` (new)                  | caret row → scroll                                                                                                                                                                                                                               | V3                            | yes                  |
| 11  | `set_selections`                                | a caret in a fold opens it                                                                                                                                                                                                                       | V5a                           | yes                  |
| 12  | `follow`                                        | folds follow every edit (the highlight spans already do, `e22ffae7`)                                                                                                                                                                             | V5b                           | yes                  |
| 13  | `refresh_highlights`                            | folds asked again                                                                                                                                                                                                                                | V5c                           | yes                  |
| 14  | ← / → in `key_down`                             | step over a fold                                                                                                                                                                                                                                 | V6a                           | yes                  |
| 15  | `vertical` (↑ ↓ PgUp PgDn)                      | skip hidden lines                                                                                                                                                                                                                                | V6b                           | yes                  |
| 16  | triple click in `mouse_down`                    | next shown line                                                                                                                                                                                                                                  | V6c                           | no                   |
| 17  | `position_of`                                   | offset → pixel                                                                                                                                                                                                                                   | V7a                           | yes                  |
| 18  | `offset_at`                                     | pixel → offset                                                                                                                                                                                                                                   | V7b                           | yes                  |
| 19  | `gutter_line_at`                                | pixel → gutter line                                                                                                                                                                                                                              | V7c                           | yes                  |
| 20  | ⌥-drag in `mouse_move` (`rectangle_ranges`)     | block over rows                                                                                                                                                                                                                                  | V7d                           | no                   |
| 21  | `block_point_at`                                | the block line's painted row                                                                                                                                                                                                                     | V7e                           | no                   |
| 22  | `text_offset_at`                                | pixel → offset (hover)                                                                                                                                                                                                                           | V7f                           | yes                  |
| 23  | `Prepared`                                      | placeholders, chevrons                                                                                                                                                                                                                           | V8a                           | yes                  |
| 24  | `prepaint` rows                                 | scroll → painted rows                                                                                                                                                                                                                            | V8b                           | yes (whole function) |
| 25  | `prepaint` highlight spans                      | per-row span lookup                                                                                                                                                                                                                              | V8c                           | yes (whole function) |
| 26  | `prepaint` marks filter                         | breakpoints, execution line, problems, find, usages in the painted offsets                                                                                                                                                                       | V8c (comment), unchanged code | yes                  |
| 27  | `prepaint` placeholder / layout                 | fold tail, chevron                                                                                                                                                                                                                               | V8d                           | yes                  |
| 28  | `paint` scroll listener                         | wheel → scroll                                                                                                                                                                                                                                   | V9a                           | yes                  |
| 29  | `paint` destructure                             |                                                                                                                                                                                                                                                  | V9b                           | yes                  |
| 30  | `paint` row top                                 | every per-line paint: current line, breakpoint / execution tint, find / usage / problem backgrounds, selections (column selection included), IME underline, text, diagnostics squiggles, struck occurrences, line number, breakpoint dot, carets | V9c                           | yes                  |
| 31  | `paint` placeholder / chevron                   |                                                                                                                                                                                                                                                  | V9d                           | yes                  |
| 32  | `paint` layout reset                            |                                                                                                                                                                                                                                                  | V9e                           | yes                  |
| 33  | `bounds_for_range`, `character_index_for_point` | through 17 / 18                                                                                                                                                                                                                                  | none needed                   | yes                  |
| 34  | the view's test                                 | metrics                                                                                                                                                                                                                                          | V10                           | yes                  |

Outside `view.rs`: `editor/find_bar.rs` `select_in` (1.6, missed by the draft), `workbench/plugin_contributions.rs` `plugin_gutter_layer` (1.7; the draft said it "stays true", it does not under folds), `workbench/context_menus.rs` `gutter_line_at` and `editor/intelligence.rs` popup anchors (through `offset_at` / `position_of`, nothing to change). Inlay hints: the native editor paints none (`grep -n inlay editor/view.rs` prints nothing), so there is no site. `scroll_to_line(line.saturating_sub(5))` callers (Problems, TODO, debugger, usages) give 5 document lines of context, fewer rows when a fold is above: accepted.

## 4. Edits to the drafted files (all in this session; copies in `$S/ed/files/app-gpui/src/`)

`workbench/editor_core.rs` (898 → 792 lines):

- `copy_reference` deleted (duplicate method: `project_files.rs` defines `Workbench::copy_reference`, error E0592) with its `editor.copyReference` registration and the imports only it used (`crate::editor::buffer`, `ReferenceInput`, `declared_name`, `qualified_reference`, `reference_offset`, `declaration_chain`, `ide_core::{Pos, SymbolKind}`, `REFERENCE`).
- `project_root` deleted (duplicate: `mod.rs:1081` `pub fn project_root(&self, cx: &App) -> Option<PathBuf>`, which `editor_language.rs` also calls); `path_with_line` uses that one (`.map(|root| root.to_string_lossy().into_owned())`).
- `close_all_tabs` and the `editor.closeAllTabs` registration deleted (`mod.rs` registers it after `project_files`, closes every pane and clears the reopen history, and has its test).
- Module doc no longer names Copy Reference and Close All Tabs.
- Tests: `close_all_tabs_closes_the_tabs_of_every_pane` deleted; `the_copy_path_actions_copy_the_forms_of_the_edited_file` keeps `editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber` (caret on line 3, history asserted as the three copies, newest first) and drops the Copy Reference steps; `the_actions_need_what_their_when_clauses_name` no longer lists `editor.closeAllTabs` / `editor.copyReference`, and in the single-file window presses ⌥⇧. twice and asserts the font zoom stays 0 (`projectOpen` false); `the_font_size_steps_in_the_active_editor_and_resets_in_every_editor` presses ⌥⇧. for the second step (the key in a project window); `the_editor_actions_do_nothing_outside_the_editor` also presses ⌥↑ ⌘. ⌘⌥- ⌃⌥I ⌃L ⇧⌘V with the Project view focused and asserts text, selections, scroll, shown lines and the absent dialog are unchanged.

`editor/core_actions.rs` (677 → 694 lines):

- `impl Editor` gains `row_of_line`, `line_of_row`, `row_count` (public, for `find_bar.rs` and `plugin_contributions.rs`).
- `run_fold` calls `editor.scroll_by(px(0.0), cx)` when the folds changed: fewer rows would otherwise leave the view scrolled past the end (Collapse All while scrolled down painted nothing). Import `px`.
- The registry test's comment names the real test (`the_font_size_steps_in_the_active_editor_and_resets_in_every_editor`).

`workbench/editor_language.rs` (2283 → 2264 lines):

- `run_intention_control` and the `ControlItem` import deleted (the build runs the control itself; the function would be dead code under `-D warnings`).
- Test `opens_a_rows_menu_with_the_right_arrow_and_runs_its_items` deleted: `editor/intelligence/tests/proofs_editor_lsp.rs` `a_quick_fixs_arrow_menu_opens_with_the_right_arrow_and_runs_its_controls` covers it in the build, and it ran `code.showIntentions`, not an action of this module.
- `install_with_capabilities` (rewrote the server script at test time) deleted; the fixture uses `FakeServer::install` with hunk 2.6.
- Two closures over 120 columns (lines 2000 and 2189 of the draft) wrapped.
- New test `the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard`.

`editor/folds.rs`, `editor/paste_history_view.rs`, `editor_lsp/definition_view.rs`: unchanged.

All six: no line over 120 columns, no forbidden word (`grep -n -i -w "port\|webview\|typescript\|react\|parity\|codemirror"` prints nothing).

## 5. Removed because it now exists

- `editor.closeAllTabs` (draft registration, `close_all_tabs`, its test, old `mod.rs` hunk 4): `workbench/mod.rs` `register_actions`, proven by `close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen` (`workbench/tests/proofs_editor_actions_tabs.rs`).
- `editor.copyReference` (draft registration and `copy_reference`): `workbench/project_files.rs` (copies, records the clipboard history, shows the hint through `editor.showHint`), proven by `the_copy_reference_action_copies_the_qualified_name_without_a_popup`.
- A quick fix's → menu running its control, and `apply_inspection` being `pub(super)` (language sections 3b and 4, `run_intention_control`, the duplicate test).

## 6. Action ids, conditions, keys

Every id below is in `assets/gen/actions.json` (measured); its `when` / `keyWhen` and title come from there at run time (the registry reads them; the drafts set none of their own), and its keys from `keymaps.json` (measured). The manifest is generated from the webview; of the webview sources only `src/editor/core/actions.ts` (`editor.copyPath`, `editor.copyPathWithLineNumber`) and `src/editor/lsp/actions.ts` (`code.overrideMethods`, `editor.showHint`) were read in this session, the rest rely on the manifest's `source` lines. IntelliJ macOS keys (the test preset on macOS):

| Id                                                                                                                                                                                                                                                                                                                                                                                                      | when / keyWhen                                                           | Key                                                                                    | Negative key test                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `code.fold`                                                                                                                                                                                                                                                                                                                                                                                             | `editorTextFocus \|\| textEditorActive && popupOpen`                     | ⌘-                                                                                     | `the_editor_actions_do_nothing_outside_the_editor` (and positive)                                                                                                                                                                                                                                                                                                                                                                                      |
| `code.unfold` / `code.foldAll` / `code.unfoldAll`                                                                                                                                                                                                                                                                                                                                                       | same                                                                     | ⌘= / ⌘⇧- / ⌘⇧=                                                                         | via `code.fold` (same condition); not pressed                                                                                                                                                                                                                                                                                                                                                                                                          |
| `code.collapseRecursively` / `code.expandRecursively`                                                                                                                                                                                                                                                                                                                                                   | same                                                                     | ⌘⌥- / ⌘⌥=                                                                              | ⌘⌥- in the same test                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `code.foldSelection` / `code.collapseBlock`                                                                                                                                                                                                                                                                                                                                                             | same                                                                     | ⌘. / ⌘⇧.                                                                               | ⌘. in the same test                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `code.foldToggle`                                                                                                                                                                                                                                                                                                                                                                                       | same                                                                     | none                                                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `code.extendSelection` / `code.shrinkSelection`                                                                                                                                                                                                                                                                                                                                                         | same                                                                     | ⌥↑ / ⌥↓                                                                                | ⌥↑                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `code.autoIndentLines`                                                                                                                                                                                                                                                                                                                                                                                  | same                                                                     | ⌃⌥I                                                                                    | ⌃⌥I                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `editor.scrollToCenter`                                                                                                                                                                                                                                                                                                                                                                                 | same                                                                     | ⌃L                                                                                     | ⌃L                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `code.pasteFromHistory`                                                                                                                                                                                                                                                                                                                                                                                 | same                                                                     | ⇧⌘V                                                                                    | ⇧⌘V                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `editor.copyPathWithLineNumber`                                                                                                                                                                                                                                                                                                                                                                         | same                                                                     | none                                                                                   | registry refusal only                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `editor.copyPath`                                                                                                                                                                                                                                                                                                                                                                                       | `textEditorActive` / `editorFocus \|\| !toolWindowFocus`                 | none                                                                                   | registry refusal                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `file.toggleReadOnly`                                                                                                                                                                                                                                                                                                                                                                                   | `textEditorActive`                                                       | none                                                                                   | registry refusal                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `editor.increaseFontSize` / `editor.decreaseFontSize`                                                                                                                                                                                                                                                                                                                                                   | `projectOpen`                                                            | ⌥⇧. / ⌥⇧,                                                                              | ⌥⇧. in the single-file window (and positive in a project)                                                                                                                                                                                                                                                                                                                                                                                              |
| `editor.resetFontSize`, `file.reloadAllFromDisk`                                                                                                                                                                                                                                                                                                                                                        | `projectOpen`                                                            | none                                                                                   | registry refusal                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `file.reloadFromDisk`                                                                                                                                                                                                                                                                                                                                                                                   | `projectOpen`                                                            | ⌘⌥Y                                                                                    | registry refusal only                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `navigate.lastEditLocation` / `navigate.nextEditLocation`                                                                                                                                                                                                                                                                                                                                               | `projectOpen`                                                            | ⇧⌘⌫ / none                                                                             | registry refusal only                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `navigate.back` / `navigate.forward`                                                                                                                                                                                                                                                                                                                                                                    | none                                                                     | ⌘[ ⌘⌥← / ⌘] ⌘⌥→                                                                        | no condition                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `editor.showHint`                                                                                                                                                                                                                                                                                                                                                                                       | none                                                                     | none                                                                                   | no condition                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `editor.pasteHistory.paste` / `.remove`                                                                                                                                                                                                                                                                                                                                                                 | `pasteHistoryOpen` / `pasteHistoryListFocus`                             | Enter / Delete, Backspace                                                              | `paste_from_history_pastes_the_entry_chosen_in_the_dialog` (Backspace in the filter removes nothing) and registry refusal                                                                                                                                                                                                                                                                                                                              |
| `code.reformat`, `code.reformatWith{Prettier,Oxfmt,Ruff}`, `code.optimizeImports`, `code.implementMethods`, `code.overrideMethods`, `refactor.rename`, `code.applyQuickFix`, `code.quickDefinition`, `code.quickTypeDefinition`, `navigate.gotoTest`, `navigate.nextMethod`, `navigate.previousMethod`, `navigate.nextError`, `navigate.prevError`, `code.showErrorDescription`, `find.highlightUsages` | `textEditorActive && (editorFocus \|\| popupOpen \|\| !toolWindowFocus)` | ⌥⌘L, none×3, ⌃⌥O, ⌃I, ⌃O, ⇧F6, ⌥⇧↵, ⌥Space ⌘Y, none, ⇧⌘T, ⌃⇧↓, ⌃⇧↑, F2, ⇧F2, ⌘F1, ⇧⌘F7 | `the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard` presses F2 ⌘F1 ⇧⌘F7 ⌃⇧↓ ⌃⌥↓ ⌃⌥O ⌥⇧↵ ⌥⌘L with the Project view focused (same editor active, no hint; its text, caret and usages unchanged), then F2 in the editor (the hint). Not pressed there: ⌃I, ⌃O, ⇧F6, ⌥Space, ⌘Y, ⇧⌘T, ⌃⇧↑, ⇧F2 (⇧F6 and ⌘N-like keys may be the Project view's own and open a dialog that swallows the rest; the others share the pressed keys' condition) |
| `code.generate`                                                                                                                                                                                                                                                                                                                                                                                         | same / `editorLangId == java \|\| … \|\| php`                            | ⌘N                                                                                     | `cmd_n_in_a_markdown_file_opens_the_new_popup`                                                                                                                                                                                                                                                                                                                                                                                                         |
| `navigate.nextHighlightedUsage` / `prev…`                                                                                                                                                                                                                                                                                                                                                               | same / `editorTextFocus`                                                 | ⌃⌥↓ / ⌃⌥↑                                                                              | ⌃⌥↓ in the language key test                                                                                                                                                                                                                                                                                                                                                                                                                           |
| `code.reformatFile`, `code.optimizeImportsFile`, `code.cleanupFile`                                                                                                                                                                                                                                                                                                                                     | `projectOpen`                                                            | none                                                                                   | —                                                                                                                                                                                                                                                                                                                                                                                                                                                      |

The context keys these read (`editorTextFocus`, `editorFocus`, `textEditorActive`, `popupOpen`, `toolWindowFocus`, `projectOpen`, `editorLangId`) are set in `publish_context` (grepped); `pasteHistoryOpen` by hunk 1.3.4. `pasteHistoryListFocus` is never set (by design, 1.3.4).

## 7. The fake language server against the language stage's tests

The fixture (`editor/intelligence.rs` `mod fake_server`) answers any request with `answers[method]` (or `null`), `initialize` with fixed capabilities, publishes `answers["diagnostics"]` on `didOpen` / `didChange`, and logs every message. With 2.6:

- `textDocument/prepareRename`, `textDocument/rename` (rename test): answered from the answers; the server advertises `renameProvider.prepareProvider` from `answers.capabilities`. The last step's `prepareRename: null` relies on `lsp_prepare_rename` answering `Ok(None)` for `null` (assumption 3).
- `textDocument/formatting` (reformat, cleanup): answered; `documentFormattingProvider` from the capabilities.
- `textDocument/codeAction` (optimize imports, cleanup, apply quick fix, generate): answered; `codeActionProvider` is built in. `codeAction/resolve` is answered `null`, but `lsp_apply_code_action` resolves only an action with neither `edit` nor `command` (`crates/ide-host/src/ops/lsp.rs:985`), and every action the tests offer has an `edit`. `asked[0]` is deterministic: the only other `codeAction` request is `show_intentions`, which runs only on Show Context Actions (`intelligence.rs:1205`).
- `textDocument/documentHighlight`: answered; `documentHighlightProvider` from the capabilities.
- `textDocument/definition`, `textDocument/typeDefinition`: built-in capabilities, answered.
- `textDocument/documentSymbol` (`moves_by_line`): answered; `documentSymbolProvider` from the capabilities.
- Diagnostics (next error, apply quick fix): published from the answers on `didOpen`.

Waiting for the document to be open: `open` waits for the opened file's `didOpen` in the server's log (`wait_opened`) before returning, as the fixture's own tests learned to. The three `*File` steps (`code.reformatFile`, `code.optimizeImportsFile`, `code.cleanupFile`) open a second file and ask about it at once without that wait. Read, not run: when a document is attached to a running server, `ide-lsp` sends its `didOpen` under the documents lock at attach time (`crates/ide-lsp/src/lib.rs:606`), and every later request about it goes through the same peer (`call`, `:1258`), so `didOpen` precedes the request on the pipe; the server is already running there (it has `a.mock` open). If a run shows the request first (`f.requests` order in the failure output), add `f.wait_opened(cx, "b.mock")` after the editor of `b.mock` shows and run the action again.

## 8. Assumptions to check, in order

1. `git status` shows the three edited drafts modified (section 0); else copy from `$S/ed/files`.
2. Stage 1 compiles without stage 2: nothing of stage 1 names `editor_language`, `show_hint_at`, `MarkLayer::Usages` (grepped the stage-1 drafts).
3. `lsp_prepare_rename` answers `Ok(None)` for a `null` result (rename test, last step).
4. Tree-sitter's Rust fold ranges for the test text are `(0,5)` and `(1,3)`, the block's end line starting with `}` (all fold tests).
5. Keystroke parsing: `cmd--`, `cmd-alt--`, `cmd-.`, `alt-shift-.`, `ctrl-alt-i`, `alt-shift-enter`, `cmd-shift-f7` parse to the keymap's `Meta+-`, `Meta+Alt+-`, `Meta+.`, `Alt+Shift+.`, `Ctrl+Alt+I`, `Alt+Shift+Enter`, `Meta+Shift+F7` (`input::keystroke` / `key_name`). On a non-macOS host the default preset is `intellij-windows` and these tests' keys differ.
6. `BackendEvent.window` of `WorkspaceEditApplied` is the project's label or `None`; `paths` are file paths equal to `to_ipc(editor.path())`.
7. `Editor::set_encoding(current, true)` reloads synchronously (Reload from Disk).
8. `ide_vfs::set_writable` changes permission bits in a macOS / Linux temp dir.
9. `ide.mode = "light"` keeps every server off (Python and Markdown tests).
10. A formatter path in `languages.<tool>.path` that is not a `.js` file is run directly (`reformat_with_a_named_formatter_runs_that_formatter`).
11. `ide_lsp` turns a `DocumentSymbol` of kind 12 into a `SymbolKind::Function` outline item (`moves_by_line`).

## 9. Compile risks

- `view.rs` V8d: `shape` captures `window` (shared) and `run`; `region_name_at(state, &comments, from)` relies on the closure-to-`&dyn Fn` coercion. If either fails, inline the closure (`&|pos: u32| editor.facts().comments_at(pos)`) or shape the two lines without the helper.
- `view.rs` V5a/V5b borrow `self.core.folds` mutably after `self.edit_state()` (a value), fine; V7d's `retain` closure borrows `self.core.folds` and `self.rope` immutably while `ranges` is a local, fine.
- `Pixels / Pixels = f32`, `Pixels * f32 = Pixels` (V3) are in `gpui-pre-0.3.12/src/geometry.rs:2738`, `:2766` (read).
- `editor_core.rs`: `self.project_root(cx)` is now `mod.rs`'s (`Option<PathBuf>`); if the compiling agent renamed it, adapt `path_with_line`.
- `editor_language.rs`: `#[expect(clippy::too_many_arguments)]` on `choose_action` (8 inputs with `self`); `basename(found) == name` compares `Cow<str>` with `String`; `on_language_editor` clones an `AsyncWindowContext` (`Clone` in gpui-pre-0.3.12).
- The `unavailable(cx, &editor_actions)` call passes `&[&str; 5]` to a closure parameter typed `&[&str]` (the draft already did this with array literals).
- Not re-read in this session beyond the import check: the bodies of `paste_history_view.rs`, `folds.rs`, the `file_attributes`, `nav_history`, `folding`, `extend_selection` and `goto_test` APIs. The drafts were written against them 3–4 hours ago and none of those files changed since (`git diff --stat 598adc00 HEAD` lists none of them).

## 10. Tests most likely to fail first, and why

1. `the_folding_commands_hide_and_show_the_lines_of_their_regions`, `the_caret_skips_the_lines_a_fold_hides`, `a_fold_follows_the_edits_above_it`: assumption 4 (the fold ranges); the second also expects ↓ from a header to land on line 4 through V6b.
2. `rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each`: assumptions 3 and 6, and the preview dialog's title and buttons.
3. `optimize_imports_and_code_cleanup_apply_the_servers_source_actions` and `reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines`: the `*File` steps ask a file the action has just opened (section 7); the fake answers every `codeAction` with the same list, so Cleanup's second request gets the actions of the first text (the test keeps the edits on separate lines).
4. `the_editor_actions_do_nothing_outside_the_editor` and `the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard`: assumption 5 (key parsing); a key the Project view itself handles is fine, the assertions are about the editor.
5. `extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`: the first step expects the word `one`; a wider innermost backend range shows in the first assertion.
6. `back_and_forward_move_between_the_places_of_two_files`: focus-in may arrive before `self.editor()` names the new tab; then record the place in `editor_took_focus` instead.
7. `scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view`: needs a frame painted between the action and the read of `painted_lines`.
8. Existing tests after stage 1: `copy_reference_copies_the_qualified_name_of_the_declaration_at_the_caret` and `copy_path_reference_lists_the_forms_of_the_edited_file_and_copies_one` now get their hint through the registered `editor.showHint`, run in `window.defer`; they read the notification titles after `run_until_parked`, which runs deferred work, and the title is the same text. `a_long_file_costs_a_viewport_per_frame` (V10).
9. Existing tests after stage 2: every workspace edit the backend applies now raises "Refactoring applied"; a test that runs `edit.undo` after a code action now reverts it (none does, by grep).
10. Tests whose outcome depends on these ids being unregistered: grepped every id of both stages over `app-gpui/src` (tests included) for registrations and for checks of registration. None registers them; the checks found are dynamic (`context_menus.rs` `the_editor_menu_leaves_out_the_actions_the_window_does_not_register` compares against the registry at run time; `project_view/context_menu.rs` adds Reformat Code / Optimize Imports rows when `code.reformatFile` / `code.optimizeImportsFile` are registered, and its tests give `ctx.actions` by hand; `chrome.rs`'s status-click test uses its own registry). After stage 1 the status bar's read-only lock becomes clickable (`file.toggleReadOnly`) and the editor menu gains Paste from History; after stage 2 the Project view gains Reformat Code / Optimize Imports and the editor menu Generate, Rename and Test: no test asserts a fixed list of those rows (grepped).

## 11. Action → test → IPC → status

"IPC" is the `src-tauri/src/commands/` adapter and the `ide_host` function it calls: measured by grep of the adapter's body for `doc_folding`, `doc_selection_ranges`, `doc_set_encoding`, `doc_outline`, `fs_set_writable`, `lsp_prepare_rename`, `lsp_rename`, `apply_workspace_edit`, `undo_workspace_edit`, `redo_workspace_edit`, `lsp_format`, `lsp_code_actions`, `lsp_apply_code_action`, `lsp_document_highlights`, `lsp_goto`, `project_roots`, `find_files`; `cancel_request` and `doc_edit` were not looked up (named from the original drafts). "Partial" names what the webview does that this does not.

| Action                                                                                                                                                                            | End-to-end test                                                                                                                                   | IPC (adapter → backend)                                                                                                                                                                                                                                                                         | Status                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `code.fold`, `code.unfold`, `code.foldToggle`, `code.foldAll`, `code.unfoldAll`, `code.collapseRecursively`, `code.expandRecursively`, `code.foldSelection`, `code.collapseBlock` | `the_folding_commands_hide_and_show_the_lines_of_their_regions` (+ `the_caret_skips_the_lines_a_fold_hides`, `a_fold_follows_the_edits_above_it`) | `doc_folding` (`commands/doc.rs`) → `ide_host::ops::doc::doc_folding`                                                                                                                                                                                                                           | partial: no chevrons on gutter hover, a click on a chevron or placeholder does not unfold, a caret at a fold's end (or in its tail) is on a hidden line and is neither painted nor given a position (`position_of` answers `None`, so the input method and a popup at that caret have no anchor), a caret a new fold hides goes to the fold's start, folds are not saved with the session, Shift+arrow column mode into a fold opens it |
| `code.extendSelection`, `code.shrinkSelection`                                                                                                                                    | `extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`                                                                             | `doc_selection_ranges` → `ide_host::ops::doc::doc_selection_ranges`                                                                                                                                                                                                                             | partial: text candidates without a syntax tree when the backend answers nothing; Shrink does not wait for queued Extends                                                                                                                                                                                                                                                                                                                |
| `code.autoIndentLines`                                                                                                                                                            | `auto_indent_lines_reindents_the_selected_lines`                                                                                                  | `doc_edit` → `Documents::edit` (the editor's edit path)                                                                                                                                                                                                                                         | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `editor.scrollToCenter`                                                                                                                                                           | `scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view`                                                                                  | none                                                                                                                                                                                                                                                                                            | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `editor.increaseFontSize`, `editor.decreaseFontSize`, `editor.resetFontSize`                                                                                                      | `the_font_size_steps_in_the_active_editor_and_resets_in_every_editor`                                                                             | none                                                                                                                                                                                                                                                                                            | partial: always the active editor (the webview steps the focused surface first: terminal, plugin view); the zoom is not published to a Markdown preview                                                                                                                                                                                                                                                                                 |
| `navigate.back`, `navigate.forward`                                                                                                                                               | `back_and_forward_move_between_the_places_of_two_files`                                                                                           | none                                                                                                                                                                                                                                                                                            | partial: only a file switch leaves a back entry; in-file jumps (click, Go to Line, find) are not marked as navigations                                                                                                                                                                                                                                                                                                                  |
| `navigate.lastEditLocation`, `navigate.nextEditLocation`                                                                                                                          | `last_and_next_edit_location_walk_through_the_edits`                                                                                              | none                                                                                                                                                                                                                                                                                            | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `editor.showHint`                                                                                                                                                                 | `the_copy_path_actions_copy_the_forms_of_the_edited_file`                                                                                         | none                                                                                                                                                                                                                                                                                            | partial: a notification keyed `editor.hint`, not a hint at the caret with a tone (kept so the Copy Reference proof tests' notification stays)                                                                                                                                                                                                                                                                                           |
| `editor.copyPath`, `editor.copyPathWithLineNumber`                                                                                                                                | `the_copy_path_actions_copy_the_forms_of_the_edited_file`                                                                                         | none (clipboard)                                                                                                                                                                                                                                                                                | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `file.reloadFromDisk`, `file.reloadAllFromDisk`                                                                                                                                   | `reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes`                                                                             | `doc_set_encoding` with `reload: true` → `Documents::set_encoding` (through `Editor::set_encoding`, as `ide_host::ops::doc::doc_set_encoding` does)                                                                                                                                             | partial: Reload All runs `project.refresh` / `vcs.refresh` only where registered (`project.refresh` is not)                                                                                                                                                                                                                                                                                                                             |
| `file.toggleReadOnly`                                                                                                                                                             | `toggle_read_only_asks_then_locks_the_file_and_its_editor`                                                                                        | `fs_set_writable` (`commands/fs.rs`) → `ide_host::ops::fs::set_writable`                                                                                                                                                                                                                        | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `code.pasteFromHistory`, `editor.pasteHistory.paste`, `editor.pasteHistory.remove`                                                                                                | `paste_from_history_pastes_the_entry_chosen_in_the_dialog`                                                                                        | `doc_edit` (the paste)                                                                                                                                                                                                                                                                          | partial: plain-text preview, the active row is not scrolled into view, no Reformat on Paste                                                                                                                                                                                                                                                                                                                                             |
| `refactor.rename`                                                                                                                                                                 | `rename_applies_the_servers_edit_to_both_files_and_one_undo_restores_each`                                                                        | `lsp_prepare_rename`, `lsp_rename`, `cancel_request`, `apply_workspace_edit`, `undo_workspace_edit`, `redo_workspace_edit` (`commands/lsp.rs`) → `ide_host::ops::lsp::{lsp_prepare_rename, lsp_rename, apply_workspace_edit, undo_workspace_edit, redo_workspace_edit}`, `Host::cancel_request` | partial: always the dialog (no in-place rename), no syntax-tree rename without a server, ⌘Z in the editor does not reach the refactoring undo (Edit › Undo does; optional hunk in 2.7)                                                                                                                                                                                                                                                  |
| `code.reformat`, `code.reformatFile`                                                                                                                                              | `reformat_code_applies_the_servers_edits_and_reports_the_formatted_lines`                                                                         | `lsp_format` → `ide_host::ops::lsp::lsp_format`                                                                                                                                                                                                                                                 | partial: no per-language Code Style indent override, contributed formatters (SQL) not consulted, no "Show the Reformat dialog" link                                                                                                                                                                                                                                                                                                     |
| `code.reformatWithPrettier`, `code.reformatWithOxfmt`, `code.reformatWithRuff`                                                                                                    | `reformat_with_a_named_formatter_runs_that_formatter`                                                                                             | `lsp_format` (with `tool`) → `ide_host::ops::lsp::lsp_format`                                                                                                                                                                                                                                   | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `code.optimizeImports`, `code.optimizeImportsFile`                                                                                                                                | `optimize_imports_and_code_cleanup_apply_the_servers_source_actions`                                                                              | `lsp_code_actions`, `lsp_apply_code_action` → `ide_host::ops::lsp::{lsp_code_actions, lsp_apply_code_action}`                                                                                                                                                                                   | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `code.cleanupFile`                                                                                                                                                                | same                                                                                                                                              | `lsp_code_actions` ×2, `lsp_apply_code_action`, `lsp_format`                                                                                                                                                                                                                                    | partial: generated code is not re-indented                                                                                                                                                                                                                                                                                                                                                                                              |
| `code.generate`, `code.implementMethods`, `code.overrideMethods`                                                                                                                  | `generate_lists_the_generate_actions_and_applies_the_chosen_one` (+ `cmd_n_in_a_markdown_file_opens_the_new_popup` for the key)                   | `lsp_code_actions`, `lsp_apply_code_action`                                                                                                                                                                                                                                                     | partial: no syntax-based Implement fallback, interactive commands not built, no re-indent of generated code                                                                                                                                                                                                                                                                                                                             |
| `code.applyQuickFix`                                                                                                                                                              | `apply_quick_fix_applies_the_fix_of_the_problem_at_the_caret`                                                                                     | `lsp_code_actions` (`only: ["quickfix"]`), `lsp_apply_code_action`                                                                                                                                                                                                                              | partial: contributed local quick fixes (spelling) not consulted                                                                                                                                                                                                                                                                                                                                                                         |
| `navigate.nextError`, `navigate.prevError`, `code.showErrorDescription`                                                                                                           | `next_error_moves_the_caret_to_the_next_problem_and_shows_its_message` (+ `the_language_keys_do_nothing_while_a_tool_window_has_the_keyboard`)    | none (the problems the editor shows)                                                                                                                                                                                                                                                            | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `find.highlightUsages`, `navigate.nextHighlightedUsage`, `navigate.prevHighlightedUsage`                                                                                          | `highlight_usages_marks_the_occurrences_and_the_caret_moves_through_them`                                                                         | `lsp_document_highlights` → `ide_host::ops::lsp::lsp_document_highlights`                                                                                                                                                                                                                       | partial: no status bar item draws the text, read / write usages share one colour, no automatic highlight at the caret, no syntax fallback, the moves' fallback `navigate.nextOccurrence` is not registered natively                                                                                                                                                                                                                     |
| `code.quickDefinition`, `code.quickTypeDefinition`                                                                                                                                | `quick_definition_shows_the_definitions_lines_and_f4_opens_it`                                                                                    | `lsp_goto` → `ide_host::ops::lsp::lsp_goto`                                                                                                                                                                                                                                                     | partial: a window of lines around the name (not the declaration's extent), no syntax colours                                                                                                                                                                                                                                                                                                                                            |
| `navigate.gotoTest`                                                                                                                                                               | `opens_the_only_test_of_a_class`                                                                                                                  | `project_roots` (`commands/app.rs`) → `ide_host::ops::project::roots`; `find_files` (`commands/nav.rs`) → `ide_host::ops::nav::find_files`                                                                                                                                                      | partial: Create New Test… runs `templates.newFromTemplate`, not registered natively; the Rust test module insertion is untested                                                                                                                                                                                                                                                                                                         |
| `navigate.nextMethod`, `navigate.previousMethod`                                                                                                                                  | `moves_by_line`                                                                                                                                   | `doc_outline` → `ide_host::ops::doc::doc_outline`                                                                                                                                                                                                                                               | done                                                                                                                                                                                                                                                                                                                                                                                                                                    |
