# Integrating the diff viewer's soft wraps, moved code and diff tabs

New files, not compiled yet:

- `app-gpui/src/vcs_diff/wrap.rs` (soft-wrap geometry: `RowLines`, `LineBreaks`, `pane_lines`, `pieces`, `spans_on_lines`, `x_at`, `editor_wraps`; plain `#[test]`s)
- `app-gpui/src/vcs_diff/view/moved.rs` (a CHILD module of `view.rs`: `mod moved;` in `view.rs`, see V1; it reads `DiffView`'s private fields)
- `app-gpui/src/vcs_diff/tab_view.rs` (`DiffTabView`, the view of a diff editor tab)
- `app-gpui/src/workbench/diff_tab.rs` (the diff tab kind, Show Diff into tabs, loading, Maximize Editor; end-to-end tests)

Every anchor is quoted from the file as it is now; no line numbers. Sections are in the order the pieces were built: soft wraps (V*, T*, G*, W*), moved code (V-M*), diff tabs (the rest). Soft wraps alone compile once V1–V24, T1–T5, T7, G1–G2, W3, W5 are applied; but T1 adds `ToggleMaximize` too, so apply V25–V27 with them (they are small).

## 0. Module lines

`app-gpui/src/vcs_diff/mod.rs`: after `pub mod model;` add `pub mod tab_view;`; after `pub mod view;` add `pub mod wrap;`.

`app-gpui/src/workbench/mod.rs`: after `mod diff_window;` add `mod diff_tab;`. Inside `mod tests`, after `    mod proofs12_diff_window;` add:

```rust
    mod proofs12_diff_soft_wraps_pending;
    mod proofs12_diff_tabs_pending;
```

`app-gpui/src/vcs_diff/view.rs` `mod tests`: see V24 (declares `proofs12_moved_code_view_pending`).

The three pending test files are used unchanged.

## 1. `app-gpui/src/vcs_diff/view.rs`

### V1. Imports and the child module

After

```rust
use super::toolbar_view::{DiffCommand, DiffSummary, ToolbarItem, ToolbarState, counter_text, toolbar_items};
```

add

```rust
use super::wrap::{LineBreaks, RowLines, editor_wraps, pane_lines, piece_starts, pieces, spans_on_lines, x_at};
```

After `use std::time::{Duration, Instant};` add

```rust

mod moved;
```

### V2. Constant

After `const BAND_STEPS: usize = 16;` add

```rust
/// Room kept free at the right of a wrapped pane's text.
const WRAP_MARGIN: f32 = 8.0;
```

### V3. Painted line types

Replace

```rust
/// A drawn row's text and its shaping; `None` for a placeholder or a filler.
type PaintedLine = Option<(String, ShapedLine)>;
/// A row's text, shaped, and where it is painted.
type ShapedRow = Option<(String, ShapedLine, Point<Pixels>)>;
```

with

```rust
/// A drawn row's text and its visual lines, each with its first byte and its shaping; `None` for a
/// placeholder or a filler.
type PaintedLine = Option<(String, Vec<(usize, ShapedLine)>)>;
/// A row's text and its visual lines, each with its first byte, its shaping and where it is painted.
type ShapedRow = Option<(String, Vec<(usize, ShapedLine, Point<Pixels>)>)>;
```

### V4. `Layout` keeps the rows' heights

Replace

```rust
    /// The first unified row of each chunk.
    unified_chunk_rows: Vec<usize>,
}
```

with

```rust
    /// The first unified row of each chunk.
    unified_chunk_rows: Vec<usize>,
    /// The visual lines each row of the left, right and unified pane takes.
    lines: [RowLines; 3],
}
```

### V5. `Geometry` measures rows by their heights

Replace everything from

```rust
/// A side-by-side pane as [`diff_scroll`](super::diff_scroll) measures it: rows of one height.
```

through the end of `fn geometry` (its last lines are `        line_height,\n    }\n}`), with

```rust
/// A side-by-side pane as [`diff_scroll`](super::diff_scroll) measures it: rows as tall as the
/// visual lines they take.
struct Geometry<'a> {
    doc: &'a Doc,
    index: &'a [usize],
    lines: &'a RowLines,
    line_height: f64,
}

impl Geometry<'_> {
    fn row(&self, pos: usize) -> usize {
        let line = self.doc.line_at(pos).number - 1;
        self.index.get(line).copied().unwrap_or(0)
    }

    fn height(&self) -> f64 {
        self.lines.total() as f64 * self.line_height
    }
}

impl PaneLayout for Geometry<'_> {
    fn doc_len(&self) -> usize {
        self.doc.len()
    }

    fn line_top(&self, pos: usize) -> f64 {
        self.lines.start(self.row(pos)) as f64 * self.line_height
    }

    fn line_bottom(&self, pos: usize) -> f64 {
        self.lines.start(self.row(pos) + 1) as f64 * self.line_height
    }
}

/// The side-by-side pane of `side` as `layout` lays it out.
fn geometry<'a>(docs: &'a [Doc; 2], layout: &'a Layout, side: Side, line_height: f64) -> Geometry<'a> {
    Geometry {
        doc: &docs[ix(side)],
        index: &layout.index[ix(side)],
        lines: &layout.lines[Pane::of(side).index()],
        line_height,
    }
}
```

### V6. `RowPaint` fields

Replace

```rust
    /// The old and the new line number (the side-by-side panes show the first only).
    numbers: [Option<usize>; 2],
}
```

with

```rust
    /// The old and the new line number (the side-by-side panes show the first only).
    numbers: [Option<usize>; 2],
    /// Where the row starts below the top of the pane's content, and how tall it is.
    top: Pixels,
    height: Pixels,
    /// The byte offsets of the text at which a new visual line starts.
    breaks: Vec<usize>,
}
```

### V7. `DiffView` fields

Replace

```rust
    /// Keeps `look` in step with the settings, once a window's settings are followed.
    settings_listener: Option<u64>,
}
```

with

```rust
    /// Keeps `look` in step with the settings, once a window's settings are followed.
    settings_listener: Option<u64>,
    /// The editor's Soft-wrap files and its file patterns, and the file the diff shows: the panes
    /// wrap as the editor would wrap the file while the viewer has no choice of its own.
    editor_soft_wrap: bool,
    soft_wrap_files: String,
    path: String,
    /// The text width of the side-by-side panes and of the unified pane, as last drawn.
    wrap_widths: [Option<Pixels>; 2],
    /// Where the lines break while the panes wrap.
    wraps: Option<LineBreaks>,
    /// Maximize Editor in the toolbar: `Some(true)` while the tool windows are hidden; `None`
    /// where the viewer is not an editor tab.
    maximized: Option<bool>,
}
```

### V8. Constructor

Replace

```rust
        .detach();
        let mut view = Self {
            focus,
```

with

```rust
        .detach();
        let defaults = EditorSettings::default();
        let mut view = Self {
            focus,
```

and replace

```rust
            look: default_look(),
            settings_listener: None,
        };
```

with

```rust
            look: default_look(),
            settings_listener: None,
            editor_soft_wrap: defaults.soft_wrap,
            soft_wrap_files: defaults.general.soft_wrap_files,
            path: String::new(),
            wrap_widths: [None, None],
            wraps: None,
            maximized: None,
        };
```

### V9. Following the editor's soft wraps; the path; Maximize Editor

Replace

```rust
    fn editor_settings_changed(&mut self, editor: &EditorSettings, cx: &mut Context<Self>) {
        if let Some(look) = self.look.follow(editor) {
            self.look = look;
            cx.notify();
        }
    }
```

with

```rust
    fn editor_settings_changed(&mut self, editor: &EditorSettings, cx: &mut Context<Self>) {
        let wrapped = self.soft_wraps();
        self.editor_soft_wrap = editor.soft_wrap;
        self.soft_wrap_files.clone_from(&editor.general.soft_wrap_files);
        let restyled = match self.look.follow(editor) {
            Some(look) => {
                self.look = look;
                true
            }
            None => false,
        };
        if restyled || wrapped != self.soft_wraps() {
            self.rewrap(cx);
            cx.notify();
        }
    }

    /// The file the diff shows: whether the editor soft-wraps it decides whether the panes wrap,
    /// until Use Soft Wraps is chosen in the viewer.
    pub fn set_path(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.path == path {
            return;
        }
        let wrapped = self.soft_wraps();
        path.clone_into(&mut self.path);
        if wrapped != self.soft_wraps() {
            self.rewrap(cx);
        }
        cx.notify();
    }

    /// Whether the panes wrap long lines: as Use Soft Wraps says once chosen, else as the editor
    /// would wrap the file.
    pub fn soft_wraps(&self) -> bool {
        self.options.soft_wrap.unwrap_or_else(|| editor_wraps(self.editor_soft_wrap, &self.soft_wrap_files, &self.path))
    }

    /// Offers Maximize Editor in the toolbar (`Some`), showing Restore Size while `Some(true)`.
    pub fn set_maximized(&mut self, maximized: Option<bool>, cx: &mut Context<Self>) {
        self.maximized = maximized;
        cx.notify();
    }
```

### V10. `toolbar()` passes Maximize Editor

Replace

```rust
            has_source: self.has_source,
            counter: counter_text(&self.summary(), cx),
```

with

```rust
            has_source: self.has_source,
            maximized: self.maximized,
            counter: counter_text(&self.summary(), cx),
```

### V11. `rebuild` drops the breaks of the old texts

Replace

```rust
        self.caret = self.caret.map(|(side, pos)| (side, pos.min(self.docs[ix(side)].len())));
        self.relayout(cx);
```

with

```rust
        self.caret = self.caret.map(|(side, pos)| (side, pos.min(self.docs[ix(side)].len())));
        // Breaks of the old texts would fall on other lines.
        self.wraps = None;
        self.relayout(cx);
```

### V12. `relayout` measures the rows

Replace

```rust
            rows: [left, right, unified],
            unified_chunk_rows,
        };
        self.selection = None;
```

with

```rust
            rows: [left, right, unified],
            unified_chunk_rows,
            lines: Default::default(),
        };
        self.measure(cx);
        self.selection = None;
```

### V13. `apply_options`: a change of wrapping or scrolling keeps the rows (and the selection)

Replace

```rust
        let compare = options.ignore != self.options.ignore || options.highlight != self.options.highlight;
        self.options = options;
        if compare {
            self.rebuild(cx);
        } else {
            self.relayout(cx);
        }
```

with

```rust
        let compare = options.ignore != self.options.ignore || options.highlight != self.options.highlight;
        // The rows stay what they are when only the scrolling or the wrapping changes.
        let rows = options.mode != self.options.mode
            || options.collapse != self.options.collapse
            || options.align_changes != self.options.align_changes;
        self.options = options;
        if compare {
            self.rebuild(cx);
        } else if rows {
            self.relayout(cx);
        } else {
            self.rewrap(cx);
        }
```

### V14. `run_command`

After

```rust
            DiffCommand::ToggleAlignChanges => self.change_options(|o| o.align_changes = !o.align_changes, cx),
```

add

```rust
            DiffCommand::ToggleSoftWrap => {
                let wraps = self.soft_wraps();
                self.change_options(|o| o.soft_wrap = Some(!wraps), cx);
            }
            DiffCommand::ToggleMaximize => cx.emit(DiffViewEvent::Maximize),
```

### V15. `go_to` and `reveal_line` place rows by their tops

Replace `        let top = self.line_height(cx) * row as f32 - self.height(pane) * (1.0 / 3.0);` with

```rust
        let top = self.row_top(pane, row, self.line_height(cx)) - self.height(pane) * (1.0 / 3.0);
```

Replace `        let top = self.line_height(cx) * row as f32 - self.height(pane) * 0.5;` with

```rust
        let top = self.row_top(pane, row, self.line_height(cx)) - self.height(pane) * 0.5;
```

### V16. Content heights

Replace `        let content = self.line_height(cx) * self.layout.rows[pane.index()].len() as f32;` with

```rust
        let content = self.line_height(cx) * self.layout.lines[pane.index()].total() as f32;
```

Replace `            scroll_height: self.layout.rows[pane.index()].len() as f64 * line_height,` with

```rust
            scroll_height: self.layout.lines[pane.index()].total() as f64 * line_height,
```

### V17. Measuring (new methods)

After `fn set_scroll` (ends `        self.panes[pane.index()].scroll_top = top.clamp(px(0.0), max);\n    }`) add

```rust

    /// Where row `row` of `pane` starts below the top of the pane's content; past the last row,
    /// the height of the content.
    fn row_top(&self, pane: Pane, row: usize, line_height: Pixels) -> Pixels {
        line_height * self.layout.lines[pane.index()].start(row) as f32
    }

    /// Measures the rows: a line takes as many visual lines as it wraps into at the width its pane
    /// was drawn at (one while the panes do not wrap, or before they are drawn), and a row as many
    /// as its counterpart in the other pane.
    fn measure(&mut self, cx: &App) {
        let width = self.wrap_widths[usize::from(self.unified())].filter(|width| *width > px(0.0) && self.soft_wraps());
        match width {
            None => self.wraps = None,
            Some(width) => {
                let style = self.code_style(cx);
                if self.wraps.as_ref().is_none_or(|wraps| wraps.width != width || wraps.style != style) {
                    self.wraps = Some(LineBreaks::measure([&self.docs[0], &self.docs[1]], width, &style, cx));
                }
            }
        }
        let layout = &self.layout;
        let rows = [&layout.rows[0][..], &layout.rows[1][..], &layout.rows[2][..]];
        let index = [&layout.index[0][..], &layout.index[1][..]];
        self.layout.lines = pane_lines(rows, index, self.options.align_changes, self.wraps.as_ref());
    }

    /// Measures the rows again after the wrapping, the font or the panes' width changed, keeping
    /// the row at the top of each pane where it is.
    fn rewrap(&mut self, cx: &App) {
        let line_height = self.line_height(cx);
        let panes = [Pane::Left, Pane::Right, Pane::Unified];
        let anchors = panes.map(|pane| {
            let top = self.panes[pane.index()].scroll_top;
            let row = self.layout.lines[pane.index()].row_at((top / line_height).floor().max(0.0) as usize);
            (row, top - self.row_top(pane, row, line_height))
        });
        self.measure(cx);
        for (pane, (row, offset)) in panes.into_iter().zip(anchors) {
            let height = line_height * self.layout.lines[pane.index()].count(row) as f32;
            let top = self.row_top(pane, row, line_height) + offset.min(height);
            self.set_scroll(pane, top, cx);
        }
    }

    /// The text of `pane` is drawn `width` wide: the panes wrap their lines to it (the left pane's
    /// width stands for both side-by-side panes). Whether the rows were measured again.
    fn wrap_to(&mut self, pane: Pane, width: Pixels, cx: &App) -> bool {
        if pane == Pane::Right {
            return false;
        }
        let slot = usize::from(pane == Pane::Unified);
        if self.wrap_widths[slot] == Some(width) {
            return false;
        }
        self.wrap_widths[slot] = Some(width);
        if !self.soft_wraps() || (pane == Pane::Unified) != self.unified() {
            return false;
        }
        self.rewrap(cx);
        true
    }
```

### V18. `position_at` finds the visual line under the pointer

Replace the whole function, from `   /// The row and column under`position`in`pane`, as it was drawn last.` through its closing `    }` (it ends `        Some((row, col))\n    }`), with

```rust
    /// The row and column under `position` in `pane`, as it was drawn last.
    fn position_at(&self, pane: Pane, position: Point<Pixels>) -> Option<(usize, u32)> {
        let painted = self.panes[pane.index()].painted.as_ref()?;
        self.layout.rows[pane.index()].len().checked_sub(1)?;
        let lines = &self.layout.lines[pane.index()];
        let y = position.y - painted.bounds.top() + self.panes[pane.index()].scroll_top;
        let visual = (y / painted.line_height).floor().max(0.0) as usize;
        let row = lines.row_at(visual);
        let x = position.x - painted.bounds.left() - painted.text_left;
        let col = row
            .checked_sub(painted.first_row)
            .and_then(|i| painted.lines.get(i))
            .and_then(Option::as_ref)
            .map_or(0, |(text, pieces)| {
                let at = visual.saturating_sub(lines.start(row)).min(pieces.len().saturating_sub(1));
                pieces.get(at).map_or(0, |(start, shaped)| col_of_byte(text, start + shaped.closest_index_for_x(x)))
            });
        Some((row, col))
    }
```

### V19. `visible_rows` by row heights

Replace

```rust
    /// The rows of `pane` in view.
    fn visible_rows(&self, pane: Pane, cx: &App) -> Range<usize> {
        let line_height = self.line_height(cx);
        let rows = self.layout.rows[pane.index()].len();
        let first = ((self.panes[pane.index()].scroll_top / line_height).floor().max(0.0) as usize).min(rows);
        let count = (self.height(pane) / line_height).ceil().max(0.0) as usize + 1;
        first..(first + count).min(rows)
    }
```

with

```rust
    /// The rows of `pane` in view.
    fn visible_rows(&self, pane: Pane, cx: &App) -> Range<usize> {
        self.rows_in_view(pane, self.height(pane), self.line_height(cx))
    }

    /// The rows of `pane` in view of a pane `height` tall at its scroll position: those that start
    /// above its bottom and end below its top.
    fn rows_in_view(&self, pane: Pane, height: Pixels, line_height: Pixels) -> Range<usize> {
        let lines = &self.layout.lines[pane.index()];
        let rows = self.layout.rows[pane.index()].len();
        let first_line = (self.panes[pane.index()].scroll_top / line_height).floor().max(0.0) as usize;
        let count = (height / line_height).ceil().max(0.0) as usize + 1;
        let first = if first_line >= lines.total() { rows } else { lines.row_at(first_line) };
        let last =
            if first_line + count > lines.total() { rows } else { lines.row_at(first_line + count - 1) + 1 };
        first..last.clamp(first, rows)
    }
```

### V20. `snapshot`

Replace

```rust
        let state = &self.panes[pane.index()];
        let rows = &self.layout.rows[pane.index()];
        let first = ((state.scroll_top / line_height).floor().max(0.0) as usize).min(rows.len());
        let count = (height / line_height).ceil().max(0.0) as usize + 1;
        let last = (first + count).min(rows.len());
        let unified = pane == Pane::Unified;
```

with

```rust
        let state = &self.panes[pane.index()];
        let rows = &self.layout.rows[pane.index()];
        let lines = &self.layout.lines[pane.index()];
        let Range { start: first, end: last } = self.rows_in_view(pane, height, line_height);
        let unified = pane == Pane::Unified;
```

and replace

```rust
            Row::Line { side, line, other } => {
                Some(self.row_paint(row, (side, line, other), unified, selection, &gaps))
            }
```

with

```rust
            Row::Line { side, line, other } => {
                let at = (line_height * lines.start(row) as f32, line_height * lines.count(row) as f32);
                Some(self.row_paint(row, (side, line, other), unified, selection, &gaps, at))
            }
```

### V21. `row_paint`: its place, its breaks, the edits of moves

Replace

```rust
        selection: Option<RowSpan>,
        gaps: &GapMarks,
    ) -> RowPaint {
```

with

```rust
        selection: Option<RowSpan>,
        gaps: &GapMarks,
        (top, height): (Pixels, Pixels),
    ) -> RowPaint {
```

Replace

```rust
        let words = marks[start..]
            .iter()
            .take_while(|mark| mark.from <= at.to)
            .filter(|mark| mark.is_empty() || mark.to > at.from)
            .map(|mark| (col(mark.from), col(mark.to), mark.kind))
            .collect();
```

with

```rust
        let mut words: Vec<(u32, u32, DiffKind)> = marks[start..]
            .iter()
            .take_while(|mark| mark.from <= at.to)
            .filter(|mark| mark.is_empty() || mark.to > at.from)
            .map(|mark| (col(mark.from), col(mark.to), mark.kind))
            .collect();
        words.extend(self.move_edits(side, line));
```

Replace

```rust
            gap_below: gap.filter(|(_, end)| *end).map(|(kind, _)| kind),
            numbers,
        }
```

with

```rust
            gap_below: gap.filter(|(_, end)| *end).map(|(kind, _)| kind),
            numbers,
            top,
            height,
            breaks: self.wraps.as_ref().map(|wraps| wraps.of(side, line).to_vec()).unwrap_or_default(),
        }
```

### V22. `render_pane`: row tops, the window, moved code

Replace `    fn render_pane(&self, pane: Pane, cx: &mut Context<Self>) -> Div {` with

```rust
    fn render_pane(&self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) -> Div {
```

Replace `        let top_of = |row: usize| line_height * row as f32 - scroll_top;` with

```rust
        let top_of = |row: usize| self.row_top(pane, row, line_height) - scroll_top;
```

Replace `        let selector = format!("diff-pane-{}", pane.name());` with

```rust
        overlays.extend(self.moved_overlays(pane, window, cx));
        let selector = format!("diff-pane-{}", pane.name());
```

In `impl Render for DiffView`, replace `    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {` with `    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {`, and replace

```rust
            body.child(self.render_pane(Pane::Unified, cx))
        } else {
            body.child(self.render_pane(Pane::Left, cx))
                .child(self.render_divider(cx))
                .child(self.render_pane(Pane::Right, cx))
```

with

```rust
            body.child(self.render_pane(Pane::Unified, window, cx))
        } else {
            body.child(self.render_pane(Pane::Left, window, cx))
                .child(self.render_divider(cx))
                .child(self.render_pane(Pane::Right, window, cx))
```

### V23. `PaneElement`: wrapped painting

In `impl Element for PaneElement`, replace the body of `fn prepaint` — everything from its line `    ) -> PanePaint {` through the line `        paint` and the `    }` closing the function (the last statements before it are `            paint.lines.push(Some((row.text, shaped, origin)));\n        }\n        paint\n    }`) — with:

```rust
    ) -> PanePaint {
        let style = self.view.read(cx).code_style(cx);
        let (font_size, line_height, code_font) = (style.size, style.line_height, style.font.clone());
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let selection_color = theme::color(cx, "bg-editor-selection");
        let accent = theme::color(cx, "accent");
        let pane = self.pane;
        let unified = pane == Pane::Unified;
        let gutter = theme::length(cx, "w-gutter") * if unified { 2.0 } else { 1.0 };
        let text_left = gutter + px(6.0);
        let wrap_width = bounds.size.width - text_left - px(WRAP_MARGIN);
        if self.view.update(cx, |view, cx| view.wrap_to(pane, wrap_width, cx)) {
            // The placeholders, buttons and links over the rows were placed for the old heights.
            let view = self.view.clone();
            window.on_next_frame(move |_, cx| view.update(cx, |_, cx| cx.notify()));
        }
        let snapshot = self.view.read(cx).snapshot(pane, bounds.size.height, line_height);
        let run = |len: usize, color: Hsla| TextRun {
            len,
            font: code_font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let mut paint = PanePaint {
            line_height,
            style,
            text_left,
            first_row: snapshot.first_row,
            fills: Vec::new(),
            lines: Vec::with_capacity(snapshot.rows.len()),
            numbers: Vec::new(),
            overs: Vec::new(),
        };
        for row in snapshot.rows {
            let Some(row) = row else {
                paint.lines.push(None);
                continue;
            };
            let top = bounds.top() + row.top - snapshot.scroll_top;
            if let Some(changed) = row.change {
                let (line_color, gutter_color) = change_colors(cx, changed);
                let width = bounds.size.width - gutter;
                let line = Bounds::new(point(bounds.left() + gutter, top), size(width, row.height));
                paint.fills.push((line, line_color));
                paint.fills.push((Bounds::new(point(bounds.left(), top), size(gutter, row.height)), gutter_color));
            }
            let mut runs = Vec::new();
            let mut at = 0usize;
            for (from, to, kind) in &row.spans {
                let from = byte_of_col(&row.text, *from).max(at);
                let to = byte_of_col(&row.text, *to);
                if from > at {
                    runs.push(run(from - at, plain));
                }
                if to > from {
                    runs.push(run(to - from, theme::color(cx, syntax_token(*kind))));
                    at = to;
                }
            }
            if at < row.text.len() || runs.is_empty() {
                runs.push(run(row.text.len() - at, plain));
            }
            let shaped = window.text_system().shape_line(row.text.clone().into(), font_size, &runs, None);
            let cut = pieces(&shaped, &row.breaks);
            let starts = piece_starts(&row.text, &cut);
            let length = utf16::len(&row.text);
            let left = bounds.left() + text_left;
            let line_top = |line: usize| top + line_height * line as f32;
            let x = |line: usize, col: u32| x_at(&row.text, &cut, line, col.min(length));
            // A span over the visual lines it crosses; an end past the text runs over the line break.
            let span = |from: u32, to: u32| -> Vec<Bounds<Pixels>> {
                spans_on_lines(&starts, from, to)
                    .into_iter()
                    .map(|(line, from, to)| {
                        let start = x(line, from);
                        let end = if to > length { x(line, length) + px(6.0) } else { x(line, to) };
                        let width = (end - start).max(px(0.0));
                        Bounds::new(point(left + start, line_top(line)), size(width, line_height))
                    })
                    .collect()
            };
            for (from, to, kind) in &row.words {
                let color = theme::color(cx, kind.color_token()).opacity(f32::from(WORD_TINT) / 100.0);
                if from == to {
                    for (line, col, _) in spans_on_lines(&starts, *from, *to) {
                        let origin = point(left + x(line, col) - px(1.0), line_top(line));
                        paint.fills.push((Bounds::new(origin, size(px(2.0), line_height)), color));
                    }
                } else {
                    paint.fills.extend(span(*from, *to).into_iter().map(|area| (area, color)));
                }
            }
            if let Some((from, to)) = row.selected {
                paint.fills.extend(span(from, to).into_iter().map(|area| (area, selection_color)));
            }
            if row.current {
                let mark = Bounds::new(point(bounds.left() + gutter - px(3.0), top), size(px(2.0), row.height));
                paint.overs.push((mark, accent));
            }
            for (kind, y) in [(row.gap_above, top - px(1.0)), (row.gap_below, top + row.height - px(1.0))] {
                if let Some(kind) = kind {
                    let mark = Bounds::new(point(bounds.left() + gutter, y), size(bounds.size.width - gutter, px(2.0)));
                    paint.overs.push((mark, theme::color(cx, kind.stripe_token())));
                }
            }
            // Side by side: one column of numbers; unified: the old and the new number.
            let column_right = |column: usize| {
                let right = if unified && column == 0 { gutter * 0.5 - px(6.0) } else { gutter - px(8.0) };
                bounds.left() + right
            };
            for (column, number) in row.numbers.iter().enumerate() {
                let Some(number) = number else { continue };
                let label = number.to_string();
                let runs = [run(label.len(), number_color)];
                let shaped_number = window.text_system().shape_line(label.clone().into(), font_size, &runs, None);
                let x = (column_right(column) - shaped_number.width).max(bounds.left());
                paint.numbers.push((point(x, top), shaped_number));
            }
            let placed =
                cut.into_iter().enumerate().map(|(line, (start, piece))| (start, piece, point(left, line_top(line))));
            paint.lines.push(Some((row.text, placed.collect())));
        }
        paint
    }
```

In `fn paint` of the same impl, replace

```rust
            for (_, shaped, origin) in prepared.lines.iter().flatten() {
                let _ = shaped.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
```

with

```rust
            for (_, pieces) in prepared.lines.iter().flatten() {
                for (_, piece, origin) in pieces {
                    let _ = piece.paint(*origin, line_height, TextAlign::Left, None, window, cx);
                }
            }
```

and replace

```rust
                .map(|line| line.map(|(text, shaped, _)| (text, shaped)))
```

with

```rust
                .map(|line| {
                    line.map(|(text, pieces)| {
                        (text, pieces.into_iter().map(|(start, piece, _)| (start, piece)).collect())
                    })
                })
```

### V24. The pending moved-code tests

In `mod tests`, replace

```rust
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffViewEvent>>>;
```

with

```rust
    use std::rc::Rc;

    mod proofs12_moved_code_view_pending;

    type Events = Rc<RefCell<Vec<DiffViewEvent>>>;
```

### V25. `DiffViewEvent::Maximize`

Replace

```rust
    /// Show the context menu of a pane here, for the line under the pointer.
    ShowMenu { side: Side, line: Option<u32>, position: Point<Pixels> },
}
```

with

```rust
    /// Show the context menu of a pane here, for the line under the pointer.
    ShowMenu { side: Side, line: Option<u32>, position: Point<Pixels> },
    /// Hide every tool window, or bring back the ones hidden (Maximize Editor, Restore Size).
    Maximize,
}
```

## 2. `app-gpui/src/vcs_diff/toolbar_view.rs`

### T1. Commands

Replace

```rust
    ToggleAlignChanges,
    /// Reverts the current change of the right side to the left side's text.
```

with

```rust
    ToggleAlignChanges,
    /// Use Soft Wraps: the viewer's own choice, stored, in place of the editor's setting.
    ToggleSoftWrap,
    /// Maximize Editor / Restore Size of a diff tab: hides every tool window, or brings them back.
    ToggleMaximize,
    /// Reverts the current change of the right side to the left side's text.
```

### T2. `ToolbarState`

Replace

```rust
    /// The diff has a file to jump to.
    pub has_source: bool,
    pub counter: String,
```

with

```rust
    /// The diff has a file to jump to.
    pub has_source: bool,
    /// Maximize Editor is offered (an editor tab), checked while the tool windows are hidden.
    pub maximized: Option<bool>,
    pub counter: String,
```

### T3. Maximize Editor before the settings button

Replace

```rust
        ToolbarItem::Button(button("diff-refresh", "refresh", "Refresh", DiffCommand::Refresh)),
        ToolbarItem::Counter(state.counter.clone()),
        ToolbarItem::Button(ToolbarButton {
```

with

```rust
        ToolbarItem::Button(button("diff-refresh", "refresh", "Refresh", DiffCommand::Refresh)),
        ToolbarItem::Counter(state.counter.clone()),
    ]);
    if let Some(maximized) = state.maximized {
        let (icon, label) = if maximized { ("minimize", "Restore Size") } else { ("maximize", "Maximize Editor") };
        items.push(toggle(button("diff-maximize", icon, label, DiffCommand::ToggleMaximize), maximized));
    }
    items.extend([
        ToolbarItem::Button(ToolbarButton {
```

In the doc comment of `toolbar_items`, replace `/// Revert Change when the right side is editable, Refresh, the counter and the settings menu.` with `/// Revert Change when the right side is editable, Refresh, the counter, Maximize Editor in an editor tab and the\n/// settings menu.` (wording only).

### T4. Its shortcut

After `        ToolbarAction::Run(DiffCommand::JumpToSource) => Some("diff.jumpToSource"),` add

```rust
        ToolbarAction::Run(DiffCommand::ToggleMaximize) => Some("toolwindow.hideAll"),
```

### T5. Use Soft Wraps in the settings menu

Replace the whole `settings_menu` function (from `/// The viewer settings: the viewer kind, the comparison submenus, collapsing, and the scrolling and` through its closing `}` after `            .disabled(!split),\n    ]`) with

```rust
/// The viewer settings as `options` alone tell them: Use Soft Wraps is checked when the viewer's
/// own choice turned it on.
pub fn settings_menu(options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    viewer_settings_menu(options, options.soft_wrap == Some(true))
}

/// The viewer settings: the viewer kind, the comparison submenus, collapsing, Use Soft Wraps
/// (checked while the panes wrap, `soft_wraps`), and the scrolling and alignment of side-by-side
/// panes (disabled in the unified viewer).
pub fn viewer_settings_menu(options: &DiffViewOptions, soft_wraps: bool) -> Vec<MenuItem<DiffCommand>> {
    let split = options.mode == DiffMode::Split;
    vec![
        checked(MenuItem::item("Side-by-side Viewer", DiffCommand::SetMode(DiffMode::Split)), split),
        checked(MenuItem::item("Unified Viewer", DiffCommand::SetMode(DiffMode::Unified)), !split),
        MenuItem::Separator,
        MenuItem::submenu("Ignore Whitespace", ignore_menu(options)),
        MenuItem::submenu("Highlighting", highlight_menu(options)),
        MenuItem::Separator,
        checked(MenuItem::item("Collapse Unchanged Fragments", DiffCommand::ToggleCollapse), options.collapse),
        checked(MenuItem::item("Use Soft Wraps", DiffCommand::ToggleSoftWrap), soft_wraps),
        checked(MenuItem::item("Synchronize Scrolling", DiffCommand::ToggleSyncScroll), options.sync_scroll)
            .disabled(!split),
        checked(MenuItem::item("Align Changes", DiffCommand::ToggleAlignChanges), options.align_changes)
            .disabled(!split),
    ]
}
```

### T6. The toolbar's settings button checks the wrapping in effect

In `DiffToolbar::press`, replace

```rust
            ToolbarAction::Open(kind) => {
                let items = menu(kind, &self.viewer.read(cx).options());
```

with

```rust
            ToolbarAction::Open(kind) => {
                let viewer = self.viewer.read(cx);
                let items = match kind {
                    DiffMenu::Settings => viewer_settings_menu(&viewer.options(), viewer.soft_wraps()),
                    DiffMenu::Ignore | DiffMenu::Highlight => menu(kind, &viewer.options()),
                };
```

### T7. The existing settings-menu test lists the new row

In `turns_on_aligned_changes_from_the_settings_menu_off_by_default`, replace

```rust
                "Collapse Unchanged Fragments ✓",
                "Synchronize Scrolling",
```

with

```rust
                "Collapse Unchanged Fragments ✓",
                "Use Soft Wraps",
                "Synchronize Scrolling",
```

## 3. `app-gpui/src/vcs_diff/dialog_view.rs`

### G1. The viewer knows the file (soft wraps follow the editor's patterns for it)

Replace `        let viewer = cx.new(|cx| DiffView::new("", "", options, cx));` (in `DiffDialog::new`) with

```rust
        let viewer = cx.new(|cx| {
            let mut viewer = DiffView::new("", "", options, cx);
            viewer.set_path(&data.path, cx);
            viewer
        });
```

### G2. Another file of the chain

In `DiffDialog::show`, replace

```rust
        self.files = file_rows(&data);
        self.data = data;
```

with

```rust
        self.files = file_rows(&data);
        self.data = data;
        let path = self.data.path.clone();
        self.viewer.update(cx, |viewer, cx| viewer.set_path(&path, cx));
```

## 4. `app-gpui/src/workbench/diff_window.rs`

### W1. What `diff_tab.rs` reuses

Replace

```rust
struct LoadedSides {
    left: String,
    right: String,
    /// A side on disk is not text.
    binary: bool,
    /// Neither side is a file, and the diff's path is one in the working tree.
    working_copy: bool,
}
```

with

```rust
pub(super) struct LoadedSides {
    pub(super) left: String,
    pub(super) right: String,
    /// A side on disk is not text.
    pub(super) binary: bool,
    /// Neither side is a file, and the diff's path is one in the working tree.
    pub(super) working_copy: bool,
}
```

Replace `async fn load_sides(` with `pub(super) async fn load_sides(`. Replace `    fn jump_to_working_copy(` with `    pub(super) fn jump_to_working_copy(`.

### W2. The diff actions reach the viewer of the active diff tab

Replace

```rust
    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone())
    }
```

with

```rust
    /// The viewer of the diff window, else of the active diff tab.
    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone()).or_else(|| self.diff_tab_viewer(cx))
    }
```

### W3. Use Soft Wraps checked as the viewer wraps (the Diff Settings action)

Replace

```rust
        let items = settings_menu(&viewer.read(cx).options());
```

with

```rust
        let items = {
            let viewer = viewer.read(cx);
            viewer_settings_menu(&viewer.options(), viewer.soft_wraps())
        };
```

and in the imports replace `use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, settings_menu};` with `use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, viewer_settings_menu};`.

### W4. Show Diff honours the non-modal commit interface (IPC: the same `vcsFileAt` / `fsReadText` / `fsProbe` loads as the diff window)

Replace

```rust
        let i18n = &mut cx.default_global::<Localization>().0;
        let data = match show_diff_files(arg, &paths, view, context, false, i18n) {
            Some(ShownDiffs::FrontWindow(data)) => *data,
            Some(ShownDiffs::Tabs(tabs)) => match tabs.into_iter().next() {
                Some(tab) => tab.data,
                None => return,
            },
            None => return,
        };
```

with

```rust
        let non_modal = crate::platform::settings::effective(&self.label, cx).vcs.commit.non_modal;
        let i18n = &mut cx.default_global::<Localization>().0;
        let data = match show_diff_files(arg, &paths, view, context, non_modal, i18n) {
            Some(ShownDiffs::FrontWindow(data)) => *data,
            Some(ShownDiffs::Tabs(tabs)) => return self.open_diff_tabs(tabs, window, cx),
            None => return,
        };
```

Also update the doc comment of `show_diff`: "The diff opens in front of the window with the other files of its group as its chain; under the non-modal commit interface each file opens in an editor tab instead."

### W5. Exhaustive match on `DiffViewEvent`

Replace

```rust
                // The panes have no menu of their own.
                DiffViewEvent::ShowMenu { .. } => {}
```

with

```rust
                // The panes have no menu of their own, and the diff window offers no Maximize Editor.
                DiffViewEvent::ShowMenu { .. } | DiffViewEvent::Maximize => {}
```

### W6. `diffEditorFocus` while a diff tab has the keyboard

Replace

```rust
                || self.merge_editor_focused(window, cx);
        set_context(window, cx, "diffEditorFocus", focused);
```

with

```rust
                || self.merge_editor_focused(window, cx)
                || self.diff_tab_focused(window, cx);
        set_context(window, cx, "diffEditorFocus", focused);
```

## 5. `app-gpui/src/workbench/tab_views.rs`

At the end of `pub const TAB_KINDS: &[TabKind] = &[`, after the `LOCAL_HISTORY_KIND` entry (it ends `        build: super::diff_tools::build_local_history_tab,\n    },`), add

```rust
    TabKind {
        id: crate::vcs_diff::diff_input::DIFF_KIND,
        restorable: false,
        active_key: None,
        focus_key: None,
        title: super::diff_tab::diff_tab_title,
        icon: super::diff_tab::diff_tab_icon,
        build: super::diff_tab::build_diff_tab,
    },
```

No `focus_key`: `publish_diff_context` owns `diffEditorFocus` (W6).

## 6. `app-gpui/src/workbench/tool_windows.rs`

After `pub fn hide_all(&mut self) -> Effects {` ... its closing `    }` (the function ends `        } else {\n            Effects::NONE\n        }\n    }`), add

```rust

    /// Hide All Tool Windows hid every window and brings them back when run again: the editor is
    /// maximized.
    pub fn all_hidden(&self) -> bool {
        self.stashed.is_some() && self.visible.is_empty()
    }
```

## Actions

No new action ids. `vcs.showDiff` (registered already) now opens tabs under `vcs.commit.nonModal`; `diff.nextDifference` / `diff.prevDifference` / `diff.jumpToSource` reach a focused diff tab through W2 and W6. Maximize Editor runs `ToolWindowLayout::hide_all`, what `toolwindow.hideAll` runs; its tooltip shows that action's shortcut. No menu_spec entries.

## Not built

- Diff tabs: moving to the next/previous file of the chain (no neighbours set, so no hint), Compare Next/Previous File and Diff Settings (the action) in a tab, editing a working-copy side in a tab (arrows/Revert), restoring diff tabs with the session (`restorable: false`). The Maximize Editor button of a tab follows the layout when the tab is built and when it is pressed, not when tool windows are toggled elsewhere.
- Soft wraps: wrapped parts keep no hanging indent (`IndentAdjustment::NoIndent`; the editor's "Use original line's indent" is not followed). A `LocalHistoryView` viewer gets no path, so it follows the editor's patterns for an empty name. Only the left pane's width is measured for both side-by-side panes.
- Moved code: the link uses the code font, not the UI font; its tooltip is the aria-label text when the move has no edits.

## Assumptions to check first (Hypothesis)

- `gpui_kit::{IndentAdjustment, LineFragment}` resolve (gpui `pub use line_wrapper::*`), and `cx.text_system().line_wrapper(font, size)` works from `&App` (signature `self: &Arc<Self>`; `App::text_system` returns `&Arc<TextSystem>`). `Boundary::ix` is the byte where the next visual line starts.
- `ShapedLine::cursor().take_until(byte)` rebases each piece to x = 0 (gpui docs say so).
- A plain `div().id(..).with_tooltip(..)` over a pane does not stop the pane's own mouse-down (normal hitbox).
- `TabView` of kind `diff` is built when `open_view_tab`/`open_tab` activates it, as for Local History.
- `run_action(cx, "editor.closeTab", None)` closes a view tab.

## Compile risks

- V23 is a long replacement; `cut` is moved after the `x`/`span` closures that borrow it (their last use is before).
- `measure`'s first line is ~118 columns; `soft_wraps`' body line ~118.
- `rows_in_view` uses `last.clamp(first, rows)`: `first <= rows` always holds.
- `wrap.rs` imports `super::view::{CodeStyle, Row}` while `view.rs` imports `wrap` (a cycle of `use`s, fine in Rust).

## Tests most likely to fail first

- `soft_wraps_reconfigure_without_losing_the_selection`: depends on the backend listener delivering `SettingsChanged` for both `editor.softWrap` and `editor.general.softWrapFiles` (Hypothesis).
- `labels_edited_moves_and_highlights_only_their_internal_word_edits`: `diff-move-edit-*` only exists when the shaped line and the move's offsets agree on the line; and `diff-moved-to-0` lies past the end of line 1's text, inside the default test window width (Hypothesis).
- The `diff_tab.rs` Maximize tests: they assume a tool window is visible right after the project opens (the Project view).
