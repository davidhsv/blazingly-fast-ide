# Editor core actions: integration notes

Nothing here was compiled or run. No existing file was edited. Every hunk below is anchored on quoted text, never on line numbers.

## New files

| File                                        | What                                                                                                                                                                                                                                                                                                            |
| ------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `app-gpui/src/editor/folds.rs`              | `Folds`: the backend's fold ranges, the folded regions, the hidden lines and the row map (row ↔ line), placeholder lookup, caret stepping over a fold, `toggle_fold`. 6 plain `#[test]`s.                                                                                                                       |
| `app-gpui/src/editor/core_actions.rs`       | `CoreState` (kept on every `Editor`), `refresh_folds`, the 9 folding actions, Extend / Shrink Selection, Auto-Indent Lines, Scroll to Center, Increase / Decrease Font Size. `impl Editor { shown_lines, folded_regions, folds_are_current, font_zoom, set_font_zoom }`. 6 `#[gpui_kit::test]`s + 2 `#[test]`s. |
| `app-gpui/src/editor/paste_history_view.rs` | `PasteHistoryDialog` (Paste from History). 6 `#[gpui_kit::test]`s.                                                                                                                                                                                                                                              |
| `app-gpui/src/workbench/editor_core.rs`     | `EditorCore` (Back / Forward and edit locations), `install_editor_core`, the window-level actions, 9 end-to-end `#[gpui_kit::test]`s.                                                                                                                                                                           |

## `mod` lines

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

## Hunks: `app-gpui/src/editor/actions.rs`

In `pub fn register`, after `    super::command_actions::register(cx);`:

```rust
    super::core_actions::register(cx);
```

## Hunks: `app-gpui/src/workbench/mod.rs`

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

4. Remove the old Close All Tabs (it runs after the line above and would replace the new one, which closes every pane and clears the reopen history). Delete:

```rust
    registry
        .handle("editor.closeAllTabs", |_, window, cx| on_tabs(window, cx, |tabs, pane| tabs.close_all(pane, false)));
```

The fix hunk in `docs/gpui-port/notes/integration/proofs_editor.md` (Part B) is the same behaviour; apply this one instead of that one, not both.

5. History recording for project tabs. In `load_tab_probed`, replace

```rust
                        if still_open {
                            workbench.install_intelligence(&editor, window, cx);
                        }
```

with

```rust
                        if still_open {
                            workbench.install_intelligence(&editor, window, cx);
                            workbench.install_editor_core(&editor, window, cx);
                        }
```

6. Context key of the Paste from History dialog (`when: pasteHistoryOpen` in `assets/gen/actions.json`). In `fn publish_context`, after the line `        set_context(window, cx, "findBarHasResults", focused_bar.is_some_and(|bar| bar.read(cx).has_results()));`:

```rust
        set_context(window, cx, "pasteHistoryOpen", self.paste_history_dialog().is_some());
```

`pasteHistoryListFocus` (the `keyWhen` of the two dialog actions) is deliberately never set. The dialog has no list focus of its own, because the keyboard stays in its filter field. It claims Enter, Delete and an empty-filter Backspace itself (`input::claim_keys`). A key it declines goes on to the keymap (`platform/input.rs` `init`). If the key were on, the keymap's Backspace → `editor.pasteHistory.remove` would delete an entry while the user edits the filter. `paste_from_history_pastes_the_entry_chosen_in_the_dialog` checks this.

## Hunks: `app-gpui/src/workbench/tab_views.rs`

In `open_loose_file`, after `                                workbench.install_intelligence(&editor, window, cx);`:

```rust
                                workbench.install_editor_core(&editor, window, cx);
```

## Hunks: `app-gpui/src/editor/view.rs`

These hunks do four things. They give the editor its `core` state. They make every line↔pixel conversion go through the fold row map. They paint the fold placeholder and the closed-fold chevron. They make the metrics follow the editor's font zoom.

### V1 Field and initialiser

After

```rust
    /// Counts edits, so that a highlight request an edit has overtaken is dropped.
    generation: u64,
    created: std::time::Instant,
```

add

```rust
    /// Folding, Extend / Shrink Selection and the font zoom ([`super::core_actions`]).
    pub(super) core: super::core_actions::CoreState,
```

After `            created: std::time::Instant::now(),` add

```rust
            core: Default::default(),
```

### V2 `Layout`: painted rows know their line

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

### V3 Metrics follow the font zoom

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

    /// Scroll to Center: the main caret's line goes to the middle of the view.
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

Then replace every `Self::metrics(cx)` in the file with `self.metrics(cx)`. The sites are `top_line`, `scroll_to_line`, `line_height`, `reveal_caret`, `scroll_by` and `page_lines`. The other three `Editor::metrics(cx)` are rewritten by V8, V9 and V10.

### V4 Rows in the scroll and caret arithmetic

`top_line`, replace the body line `        (self.scroll_top / Self::metrics(cx).1).floor() as u32` with

```rust
        self.core.folds.line_of_row((self.scroll_top / self.metrics(cx).1).floor() as u32)
```

`scroll_to_line`, replace `        self.scroll_top = Self::metrics(cx).1 * line as f32;` with

```rust
        self.scroll_top = self.metrics(cx).1 * self.core.folds.row_of_line(line) as f32;
```

`reveal_caret`, replace

```rust
        let (_, line_height, _) = Self::metrics(cx);
        let line_top = line_height * self.caret_position().0 as f32;
```

with

```rust
        let (_, line_height, _) = self.metrics(cx);
        let line_top = line_height * self.core.folds.row_of_line(self.caret_position().0) as f32;
```

`scroll_by`, replace `        let content = line_height * self.line_count() as f32;` with

```rust
        let content = line_height * self.core.folds.row_count(self.line_count()) as f32;
```

`painted_lines`, replace `        self.layout.as_ref().map(|l| l.first_line..l.first_line + l.lines.len() as u32)` with

```rust
        let layout = self.layout.as_ref()?;
        Some(layout.line_of.first().copied().unwrap_or(0)..layout.line_of.last().map_or(0, |last| last + 1))
```

(The signature returns `Option<Range<u32>>`, so the body becomes these two lines.)

### V5 Selections open folds, edits move them, and the folds are asked again

In `set_selections`, replace

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

In `follow`, replace

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

In `refresh_highlights`, replace

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

`refresh_highlights` runs once when the editor is made and after every followed change, which is when the fold ranges change. `refresh_folds` waits 40 ms of stillness (`FOLD_DELAY`) and drops a request that a newer one overtook, as the highlights do.

### V6 The caret steps over folds and skips hidden lines

In `key_down`, replace

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

In `vertical`, replace

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

### V7 Hit-testing by row

Replace the whole of `position_of` with

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

In `offset_at`, replace

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

In `text_offset_at`, replace

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

In `gutter_line_at`, replace

```rust
        let line = (y / layout.line_height) as u32;
        (line < self.line_count()).then_some(line)
```

with

```rust
        let line = self.core.folds.line_of_row((y / layout.line_height) as u32);
        (line < self.line_count()).then_some(line)
```

### V8 `Prepared` and `prepaint`

Replace

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

Replace the whole `fn prepaint(…) -> Prepared { … }` of `impl Element for EditorElement` with:

```rust
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepared {
        use crate::editor_commands::regions::region_name_at;
        let editor = self.editor.read(cx);
        let (font_size, line_height, gutter) = editor.metrics(cx);
        let code_font = Editor::code_font(cx);
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let match_color = theme::color(cx, "fg-search-match");
        let placeholder_color = theme::color(cx, "fg-fold-placeholder");
        let marker_color = theme::color(cx, "fg-fold-marker");
        let total = editor.line_count();
        let folds = &editor.core.folds;
        let rows = folds.row_count(total);
        let first_row = ((editor.scroll_top / line_height).floor() as u32).min(rows.saturating_sub(1));
        let visible = (bounds.size.height / line_height).ceil() as u32 + 1;
        let last_row = (first_row + visible).min(rows);
        let painted: Vec<u32> =
            (first_row..last_row).map(|row| folds.line_of_row(row)).filter(|line| *line < total).collect();
        let first_line = painted.first().copied().unwrap_or(0);
        let last_line = painted.last().map_or(0, |line| line + 1);
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: code_font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let state = editor.edit_state();
        let comments = |pos: u32| editor.facts().comments_at(pos);

        let first_offset = buffer::line_range(&editor.rope, first_line).start;
        let last_offset = buffer::line_range(&editor.rope, last_line.saturating_sub(1)).end;
        let marks: Vec<Mark> = editor
            .marks
            .values()
            .flatten()
            .filter(|mark| mark.range.end >= first_offset && mark.range.start <= last_offset)
            .cloned()
            .collect();
        let mut lines = Vec::with_capacity(painted.len());
        let mut numbers = Vec::with_capacity(painted.len());
        let mut placeholders = Vec::with_capacity(painted.len());
        let mut markers = Vec::with_capacity(painted.len());
        for &line in &painted {
            let range = buffer::line_range(&editor.rope, line);
            let text = buffer::slice(&editor.rope, range.start, range.end);
            let mut runs = Vec::new();
            let mut at = 0usize;
            // Lines between two painted rows can be hidden: the spans are looked up per line.
            let mut span = editor.highlights.partition_point(|s| s.to <= range.start);
            while let Some(s) = editor.highlights.get(span).filter(|s| s.from < range.end) {
                let from = buffer::byte_of_col(&text, s.from.saturating_sub(range.start)).max(at);
                let to = buffer::byte_of_col(&text, s.to.min(range.end) - range.start);
                if from > at {
                    runs.push(run(from - at, plain));
                }
                if to > from {
                    runs.push(run(to - from, theme::color(cx, syntax_token(s.kind))));
                    at = to;
                }
                if s.to > range.end {
                    break; // continues on the next line
                }
                span += 1;
            }
            if at < text.len() || runs.is_empty() {
                runs.push(run(text.len() - at, plain));
            }
            for mark in marks.iter().filter(|mark| mark.kind.recolors_text()) {
                if mark.range.end > range.start && mark.range.start < range.end {
                    let from = buffer::byte_of_col(&text, mark.range.start.saturating_sub(range.start));
                    let to = buffer::byte_of_col(&text, mark.range.end.min(range.end) - range.start);
                    runs = recolor(runs, from, to, match_color);
                }
            }
            let shaped = window.text_system().shape_line(text.clone().into(), font_size, &runs, None);
            lines.push((text, shaped));
            let label = (line + 1).to_string();
            numbers.push(window.text_system().shape_line(
                label.clone().into(),
                font_size,
                &[run(label.len(), number_color)],
                None,
            ));
            // A folded header shows a placeholder (a custom region's name), then what of the
            // fold's last line comes after it.
            let folded = folds.folded_on(&state, line);
            placeholders.push(folded.map(|(from, to)| {
                let name =
                    region_name_at(&state, &comments, from).unwrap_or_else(|| super::folds::PLACEHOLDER.to_owned());
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
    }
```

Check `region_name_at`'s exact signature (`editor_commands/regions.rs:161`: `(state: &EditState, comments: CommentTokensAt<'_>, pos: u32) -> Option<String>`). `&comments` is a `&impl Fn(u32) -> Vec<CommentTokens>`, which coerces to `CommentTokensAt`. If it does not, write `&|pos: u32| editor.facts().comments_at(pos)` inline.

### V9 `paint`

The scroll listener. Replace `                let line_height = Editor::metrics(cx).1;` with

```rust
                let line_height = editor.read(cx).metrics(cx).1;
```

The destructuring. Replace `        let Prepared { layout, numbers, gutter, marks } = prepared;` with

```rust
        let Prepared { layout, numbers, gutter, marks, placeholders, markers } = prepared;
```

The row's top. Replace

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

The placeholder and the chevron. After `                let _ = shaped.paint(text_origin, line_height, TextAlign::Left, None, window, cx);` add

```rust
                if let Some((placeholder, tail)) = placeholders.get(index).and_then(Option::as_ref) {
                    let origin = point(text_origin.x + shaped.width + px(4.0), top);
                    let chip = Bounds::new(origin, size(placeholder.width + px(8.0), line_height));
                    let background = theme::color(cx, "bg-fold-placeholder");
                    let solid = gpui_kit::BorderStyle::Solid;
                    window.paint_quad(quad(chip, px(3.0), background, px(0.0), background, solid));
                    let _ = placeholder.paint(
                        point(origin.x + px(4.0), top),
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    );
                    let after = point(origin.x + chip.size.width + px(2.0), top);
                    let _ = tail.paint(after, line_height, TextAlign::Left, None, window, cx);
                }
                if let Some(marker) = markers.get(index).and_then(Option::as_ref) {
                    let x = bounds.left() + *gutter - px(10.0);
                    let _ = marker.paint(point(x, top), line_height, TextAlign::Left, None, window, cx);
                }
```

The layout kept for hit-testing. Replace

```rust
            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new() },
```

with

```rust
            Layout { bounds, text_left: *gutter, line_height, first_line: 0, lines: Vec::new(), line_of: Vec::new() },
```

### Sites checked

These sites were grepped in `view.rs` for `first_line`, `layout.lines`, `Self::metrics`, `Editor::metrics`, `Layout {` and `line_height *`, and they are all above:

- `top_line`, `scroll_to_line`, `painted_lines` and `line_height`
- `reveal_caret`, `scroll_by` and `page_lines`
- `position_of`, `offset_at`, `text_offset_at` and `gutter_line_at`
- `prepaint`, `paint` and the view's test (V10)

`bounds_for_range` and `character_index_for_point` go through `position_of` / `offset_at`. `editor/view/` holds only the two test directories, and neither names these. Outside `view.rs`, `workbench/plugin_contributions.rs` reads `painted_lines()` as a line range. That stays true with V4.

### V10 The view's own test

In `mod tests` of `view.rs`, the line containing `(layout.bounds.size.height / Editor::metrics(cx).1).ceil() as u32)`: replace `Editor::metrics(cx).1` with `e.line_height(cx)` (the editor bound in that closure is `e`).

## Menu entries

Nothing new: `menu_spec.rs` already lists `code.fold`, `code.unfold`, `code.foldAll`, `code.unfoldAll`, `code.collapseBlock`, `code.foldSelection` (Code › Folding), `code.extendSelection`, `code.autoIndentLines`, `code.unwrap` (Code), `editor.copyPath`, `editor.copyReference`, `code.pasteFromHistory` (Edit), `editor.resetFontSize` (View), `navigate.back`, `navigate.forward`, `navigate.lastEditLocation`, `navigate.nextEditLocation` (Navigate), `file.reloadAllFromDisk` and `file.toggleReadOnly` (File). `context_menus.rs` already has the editor menu's `code.pasteFromHistory` and `editor.copyReference`. Not checked here: whether `menu_spec.rs` has `code.foldToggle`, `code.collapseRecursively`, `code.expandRecursively`, `code.shrinkSelection`, `editor.increaseFontSize`, `editor.decreaseFontSize`, `editor.scrollToCenter`, `file.reloadFromDisk` and `editor.copyPathWithLineNumber` where the webview's menus have them. Grep `src/workbench/menus*` before adding.

## IPC commands each action stands for

| Action                                                                                                                                                                                                                        | IPC (src-tauri adapter → op)                                                                                               |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| the 9 folding actions                                                                                                                                                                                                         | `doc_folding` → `ide_host::ops::doc::doc_folding` (asked after every change, `core_actions::refresh_folds`)                |
| `code.extendSelection`                                                                                                                                                                                                        | `doc_selection_ranges` → `ide_host::ops::doc::doc_selection_ranges`                                                        |
| `code.shrinkSelection`, `code.autoIndentLines`, `editor.scrollToCenter`, font size ×3, `navigate.*` ×4, `editor.copyPath`, `editor.copyPathWithLineNumber`, `editor.showHint`, `editor.closeAllTabs`, `editor.pasteHistory.*` | none of their own; edits go through `doc_edit` (`Documents::edit`, as every editor edit)                                   |
| `editor.copyReference`                                                                                                                                                                                                        | `doc_outline` → `ide_host::ops::doc::doc_outline`                                                                          |
| `file.reloadFromDisk`, `file.reloadAllFromDisk`                                                                                                                                                                               | `doc_set_encoding` with `reload: true` (through `Editor::set_encoding` → `Documents::set_encoding`, not the `ops` wrapper) |
| `file.toggleReadOnly`                                                                                                                                                                                                         | `fs_set_writable` → `ide_host::ops::fs::set_writable`                                                                      |
| `code.pasteFromHistory`                                                                                                                                                                                                       | `doc_edit` (the paste)                                                                                                     |

## Actions registered

`code.fold`, `code.unfold`, `code.foldToggle`, `code.foldAll`, `code.unfoldAll`, `code.collapseRecursively`, `code.expandRecursively`, `code.foldSelection`, `code.collapseBlock`, `code.extendSelection`, `code.shrinkSelection`, `code.autoIndentLines`, `editor.scrollToCenter`, `editor.increaseFontSize`, `editor.decreaseFontSize` (editor/core_actions.rs). Also `navigate.back`, `navigate.forward`, `navigate.lastEditLocation`, `navigate.nextEditLocation`, `editor.showHint`, `editor.copyPath`, `editor.copyPathWithLineNumber`, `editor.copyReference`, `file.reloadFromDisk`, `file.reloadAllFromDisk`, `file.toggleReadOnly`, `editor.resetFontSize`, `code.pasteFromHistory`, `editor.pasteHistory.paste`, `editor.pasteHistory.remove`, `editor.closeAllTabs` (workbench/editor_core.rs). Every id is in `assets/gen/actions.json` (checked).

## Not registered, and why

- `code.unwrap`: `editor_commands::unwrap::unwrap_options` needs the chain of syntax nodes around the caret, with kinds and field names. No backend op answers that (`syntax_structural_ranges` gives ranges without kinds). The chooser popup is not built either.
- `editor.toggleSoftWrap`: the editor paints one row per line. Soft wrap needs a wrapped layout (rows from shaped widths) through every row conversion of V4–V9.
- `editor.toggleRainbowBrackets`, `editor.toggleSemanticHighlighting`: the editor paints neither rainbow brackets nor semantic tokens, so the toggle would change nothing visible. They are client settings (`platform_more/client_settings.rs`).
- `editor.contextInfo`: needs the enclosing header lines (`enclosingHeaders` of sticky scroll). No native model computes them.
- `navigate.customRegion`: `regions::custom_regions` exists, but a list popup anchored at the caret is not built here.
- `navigate.navigationBar`: `editor_model::nav_bar` has no popup view.
- The `~` rows that need a syntax tree of the document stay as they are, since the backend gives the editor none: `code.commentBlock`, `code.commentLine`, `code.moveElementLeft/Right`, `code.moveStatementUp/Down`, `code.toggleCase`, `code.completeStatement` (structural answers). Their ids stay in `command_actions::COMMANDS` and are not registered again.
- `~code.addCaretNextOccurrence`, `~code.cyclicExpandWord(Backward)`: the hint, once logged only, can now show through `editor.showHint`. The change is in `command_actions.rs`, an existing file, so it is given as optional hunks and not made:
  - in `next_occurrence`, replace `tracing::debug!("no further occurrence of the selection");` with a deferred `execute_action("editor.showHint", Some(&serde_json::json!({ "text": "Not found, press again to search from the top" })), window, cx)`. `COMMANDS` entries get no `window`, so this needs `on_editor` to pass it. Left out.
- `~editor.closeTab`, `~editor.pinTab`, `~editor.unsplit(All)`, `~editor.toggleColumnSelection`, `~find.openInWindow`, `~find.scrollToResults`, `~navigate.gotoLine`, `~editor.copyPathReference`: what each row says is missing is out of this assignment's models. For `toggleColumnSelection`, the ⌥-drag is a mouse gesture in `view.rs`; the expected failure `restores_alt_drag_column_selection_once_the_caret_gesture_is_removed` stays failing.

## Proof tests in `proofs_editor.md` this wiring turns green

- `close_all_tabs_closes_every_pane_and_leaves_nothing_to_reopen`: green with hunk 4 (the new `editor.closeAllTabs` closes every pane and clears `TabsState::closed`).
- `the_copy_reference_action_copies_the_qualified_name_without_a_popup`: green (`editor.copyReference` registered; same outline → `declaration_chain` → `qualified_reference` path as the popup).
- `restores_alt_drag_column_selection_once_the_caret_gesture_is_removed`: still fails (not built).

My `close_all_tabs_closes_the_tabs_of_every_pane` overlaps the first. It is kept because the registration rule asks for a test of my own per action.

## Behaviour of the webview not built

- Folding:
  - The open chevrons that appear on gutter hover are not drawn.
  - A click on a chevron or a placeholder does not unfold.
  - Expand to Level (`code.expandToLevel1..5`, `code.expandAllToLevel1..5`) is not registered, although `folding::expand_to_level` exists. It is outside the list of this assignment.
  - A caret that a new fold hides goes to the fold's start, where the webview leaves it inside and shows it at the fold.
  - Folds are not saved with the session.
- Extend Selection's text candidates have no syntax-tree nodes when the backend answers nothing: word, string/comment, line, document.
- Shrink Selection does not wait for queued extends.
- Back / Forward: in-file navigations (mouse click, Go to Line, find) are not marked as navigations, because `EditorEvent::SelectionMoved` carries no user event. Only a file switch leaves a back entry. Within one file, the place is updated, not pushed.
- Font size:
  - The webview steps the focused surface (terminal, plugin view) first; here it is always the active editor.
  - The zoom is not published to a Markdown preview (`ZoomEffect::Editor`).
- Copy Reference does not ask the language server for a definition when the outline has no symbol at the caret (`lspGoto` fallback).
- Reload All from Disk runs `project.refresh` and `vcs.refresh` only when registered. `project.refresh` is not registered natively.
- Paste from History:
  - The preview is plain text without the target's grammar.
  - The list does not scroll its active row into view.
  - The pasted text skips Reformat on Paste.
- The editor hint is a notification with key `editor.hint`, not a balloon over the editor.
- A caret at a fold's end, or in the text after it on the fold's last line, is on a hidden line. It is neither painted nor clickable there; the webview shows it after the placeholder. Painting it on the header row needs the x of the tail, which V9 knows (`after`), plus a caret pass over `placeholders`. That pass is not written.
- `Folds::map` rebuilds the per-line `foldable` table on every edit (O(lines)). Without that, the commands would act on stale offsets until the backend answers.

## Assumptions to check first

1. Tree-sitter's Rust fold ranges for `fn a() {…}` / `if x {…}` are `(0,5)` and `(1,3)`, and the block's end line starts with `}`. All fold tests read the ranges through `folds_from_ranges`.
2. `cmd--` reaches `code.fold` through the IntelliJ macOS preset (`Meta+-`). GPUI parses `"cmd--"` as ⌘ + `-` (checked in `Keystroke::parse`).
3. `Pixels / Pixels` gives `f32` and `Pixels * f32` gives `Pixels` (V3).
4. `Editor::set_encoding(current, true)` reloads synchronously on the main thread, as Reload with Encoding already does. That matches the webview's `setEncoding(doc, encoding, true)`.
5. `ide_vfs::set_writable` makes the file's permissions read-only on macOS and Linux temp dirs.
6. `PromptDialog::confirm` answers Enter with OK.
7. `editor_commands::caret_commands::enclosing_code_block` answers the inside of the braces, ending at the `}` (the `code.collapseBlock` case expects lines 2–3 hidden).
8. `Documents::set_writable` sets `read_only = !observed || !writable`, so making a file writable unlocks its editor only for an observed (file-backed) document. A tab of a project file is one.
9. `open_project_file_at` on an open tab focuses its editor (focus-in records the place) before or after the reveal sets the caret. Either order gives the same Back entry.

## Tests most likely to fail first

1. `the_folding_commands_hide_and_show_the_lines_of_their_regions` and `the_caret_skips_the_lines_a_fold_hides`, if assumption 1 is off. The table then shows which lines hide.
2. `extend_and_shrink_selection_walk_the_syntax_ranges_around_the_caret`: the first step expects the word `one`. If the backend's innermost range is wider (the argument list), the first assertion shows it.
3. `back_and_forward_move_between_the_places_of_two_files`, if focus-in arrives for the new tab before `self.editor()` names it (then the switch is not recorded). Fix: record the place in `editor_took_focus` instead.
4. `scroll_to_center_puts_the_caret_line_in_the_middle_of_the_view`, if no frame is painted between `scrollToCenter` and the read of `painted_lines` (then the tolerance check reads the old layout).
5. `reload_from_disk_reads_the_file_again_unless_it_has_unsaved_changes`: the file watcher may reload the clean document by itself. The test still passes, but then it does not prove the action.
6. `toggle_read_only_asks_then_locks_the_file_and_its_editor`, on a file system ignoring permission bits.
