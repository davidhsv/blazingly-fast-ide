# Diff viewer gaps: soft wraps, moved code, the diff editor tab (reconciled)

Self-contained integration notes for the draft files below. Every hunk is anchored on quoted text that occurs exactly once in its file at the moment it is applied (hunks are applied in the order written; `dfg/anchors.py` replays them in memory and checks every count). No line numbers. Land the three parts one at a time, in order: part 2 needs part 1 (`row_top`, `wraps`, `wrap.rs`), part 3 needs parts 1 and 2 only through `view.rs`/`toolbar_view.rs` anchors part 1 creates.

Draft files (committed, undeclared until the `mod` lines below):

- `app-gpui/src/vcs_diff/wrap.rs` (part 1): soft-wrap geometry `RowLines`, `LineBreaks`, `pane_lines`, `pieces`, `piece_starts`, `spans_on_lines`, `x_at`, `editor_wraps`; plain `#[test]`s.
- `app-gpui/src/vcs_diff/view/moved.rs` (part 2): a child module of `view.rs` (`mod moved;`, hunk M1); `move_edits`, `moved_overlays`, `follow_move`, `link_text`, `link_tooltip`, `edits_on_line`; plain `#[test]`s.
- `app-gpui/src/vcs_diff/tab_view.rs` (part 3): `DiffTabView` (`new(data, options, maximized, cx)`, `data()`, `viewer()`, `title()`, `set_maximized(bool)`, `load()`, `loaded(left, right, binary)`), event `DiffTabEvent::{Load, Viewer, Menu}`; three `#[gpui_kit::test]`s.
- `app-gpui/src/workbench/diff_tab.rs` (part 3): the `diff` tab kind (`diff_tab_title`, `diff_tab_icon`, `build_diff_tab`), `open_diff_tabs`, `diff_tab_viewer`, `diff_tab_focused`, `sync_diff_tabs_maximized`, loading, chain navigation (`find_diff_tab_file`), Jump to Source; end-to-end tests.
- `app-gpui/src/workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs` (part 3): the three flows of `e2e/diff-navigation.spec.ts` in diff tabs; declared by `diff_tab.rs` itself (`mod proofs18_e2e_diff_navigation;` inside its `mod tests`).

File moves the integrator makes (git, not edits):

- part 1: `git mv app-gpui/src/workbench/tests/proofs12_diff_soft_wraps_pending.rs app-gpui/src/workbench/tests/proofs12_diff_soft_wraps.rs`
- part 2: `git mv app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view_pending.rs app-gpui/src/vcs_diff/view/tests/proofs12_moved_code_view.rs`
- part 3: `git mv app-gpui/src/workbench/tests/proofs12_diff_tabs_pending.rs app-gpui/src/workbench/tests/proofs12_diff_tabs.rs`; `git rm app-gpui/src/workbench/tests/proofs18_e2e_diff_navigation_pending.rs` (its test now lives, rewritten, in the draft `diff_tab/tests/proofs18_e2e_diff_navigation.rs`).

`mod` lines (they are hunks S1, S2, M1, M7, D1, D2, D3; listed here for reading): `pub mod wrap;` after `pub mod view;` and `pub mod tab_view;` after `pub mod model;` in `vcs_diff/mod.rs`; `mod moved;` in `vcs_diff/view.rs`; `mod diff_tab;` before `mod diff_tools;` in `workbench/mod.rs`; `mod proofs12_diff_soft_wraps;` and `mod proofs12_diff_tabs;` before `mod proofs12_diff_window;` in `workbench/mod.rs` `mod tests`; `mod proofs12_moved_code_view;` in `vcs_diff/view.rs` `mod tests`. Each goes beside its alphabetical neighbour (the workbench's `mod` list is not sorted as a whole: `use` lines sit between its `mod` lines).

## When the webview shows a diff in a tab and when in a dialog

Two entry points, decided per caller, with one setting, `vcs.commit.nonModal` (Settings › Version Control › Commit › Use non-modal commit interface), default **on**: `src/platform/workflowSettings.ts:207` (`nonModal: true`), `crates/ide-core/src/workflow_settings.rs:123`.

- `openDiff` (`src/features/vcs/diffInput.ts:133-135`) always opens an editor tab (`editorService().openInput(diffInput(data), …)`). Callers: `vcs.openDiff` (`compare.tsx:140-149`, pull-request files and plugins), the tab's own chain navigation (`DiffEditor.tsx:686-696`, `openDiff(neighbour, { preview: true })`), Show Diff in a New Tab of the Shelf (`shelf.tsx:380`), passive previews (`logChanges.ts:251`), the GitHub create panel, and `e2e/diff-navigation.spec.ts` itself (`openChain` calls `openDiff(withChain(files, 0), { preview: true })`).
- `openVcsDiff` (`diffInput.ts:151-155`) opens the front diff dialog only when `vcsDiffInFront()` (`diffInput.ts:142`, `!workflowSettings().vcs.commit.nonModal`), else `openDiff`. Callers: Show Diff (`actions.tsx:367-412`: modal → one front window with the group as chain; non-modal → `openDiff` per path, at most 5, `preview` when there is one), activating a change in the Commit window (`CommitView.tsx:204-210`, `{ preview: true }`), Compare With… / Compare Files / Compare with Clipboard (`compare.tsx:241-243, 274, 295, 307`), a log entry's file (`logChanges.ts:239`), directory compare (`DirectoryCompare.tsx:205, 282`).

So with the default settings Show Diff, the Commit window and Compare With… open editor tabs; the diff window is the modal interface's. `e2e/compareWith.spec.ts` (last case: `tabs.getByRole("tab", { name: /other\.md/ })`) and all three flows of `e2e/diff-navigation.spec.ts` therefore need the diff as an editor tab.

Native today: `workbench/diff_window.rs` `show_diff` passes `false` for `non_modal` to `show_diff_files` and always opens the diff window; `vcs.openDiff` (`vcs_operations.rs`, `open_diff`) and the compare actions (`diff_tools.rs` `open_compared_diff`) always open the diff window. After part 3: Show Diff and the Commit window follow the setting (hunk D31); `vcs.openDiff` and the compare actions still open the diff window (not built, see below).

## Edits made to the draft files

- `vcs_diff/tab_view.rs`: Maximize Editor is offered unless `data.embedded` (`diff-editor-tab.tsx:10`); new `set_maximized(bool)`; the viewer gets its chain neighbours (`chain_neighbour`) so F7 past the last difference hints and then asks for the next file; tests for the embedded case and the chain request added.
- `workbench/diff_tab.rs`: `DiffViewEvent::OpenFile` walks the chain with the diff window's `find_file` and opens the neighbour in the preview tab (`DiffEditor.tsx:686-696`); `sync_diff_tabs_maximized` (called from `with_layout`, D24) replaces the draft's one-off update in `maximize_from_diff_tab`; `build_diff_tab` sets the button through `DiffTabView::set_maximized`; tests: the maximize test runs both rows of the spec's `it.each([null, "bottom"])`, the remount test uses a maximized bottom side as the spec does, a test that the button follows Hide All Tool Windows; the e2e child module is declared.
- `workbench/diff_tab/tests/proofs18_e2e_diff_navigation.rs`: rewritten to the spec's flows: chains of text sides opened in the preview tab as `openChain` does, the existing `click_on` helper, all three flows.
- `vcs_diff/wrap.rs`, `vcs_diff/view/moved.rs`: no change needed (every API they call exists with that signature: `CodeStyle` and `Row` are `pub` in `view.rs`, `matches_file_patterns`, `byte_of_col`/`col_of_byte`, `WithTooltip::with_tooltip`, `MovedBlock`, `lcs::Range::new`, theme tokens `radius-small`, `fs-small`, `link`, `bg-hover`, `border`).
- Pending files: `proofs12_diff_soft_wraps_pending.rs` pins the modal interface (it is about the diff window) and waits for both editor settings to read back; `proofs12_moved_code_view_pending.rs` gains the two spec cases on moved code (Highlight lines, Do not highlight); `proofs12_diff_tabs_pending.rs` unchanged; `proofs18_e2e_diff_navigation_pending.rs` superseded (git rm).

## Part 1: soft wraps (with the Use Soft Wraps menu item)

#### S1 · `app-gpui/src/vcs_diff/mod.rs` · after

Insert the text right after the anchor. Declares the soft-wrap geometry.

Anchor:

```text
pub mod view;
```

Text:

```rust

pub mod wrap;
```

#### S2 · `app-gpui/src/workbench/mod.rs` · before

Insert the text right before the anchor. Declares the soft-wrap tests (after `git mv` of the pending file, see the file moves).

Anchor:

```text
    mod proofs12_diff_window;
```

Text:

```rust
    mod proofs12_diff_soft_wraps;

```

#### V1 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
use super::toolbar_view::{DiffCommand, DiffSummary, ToolbarItem, ToolbarState, counter_text, toolbar_items};
```

Text:

```rust

use super::wrap::{LineBreaks, RowLines, editor_wraps, pane_lines, piece_starts, pieces, spans_on_lines, x_at};
```

#### V2 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
const BAND_STEPS: usize = 16;
```

Text:

```rust

/// Room kept free at the right of a wrapped pane's text.
const WRAP_MARGIN: f32 = 8.0;
```

#### V3 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
/// A drawn row's text and its shaping; `None` for a placeholder or a filler.
type PaintedLine = Option<(String, ShapedLine)>;
/// A row's text, shaped, and where it is painted.
type ShapedRow = Option<(String, ShapedLine, Point<Pixels>)>;
```

Text:

```rust
/// A drawn row's text and its visual lines, each with its first byte and its shaping; `None` for a
/// placeholder or a filler.
type PaintedLine = Option<(String, Vec<(usize, ShapedLine)>)>;
/// A row's text and its visual lines, each with its first byte, its shaping and where it is painted.
type ShapedRow = Option<(String, Vec<(usize, ShapedLine, Point<Pixels>)>)>;
```

#### V4 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// The first unified row of each chunk.
    unified_chunk_rows: Vec<usize>,
}
```

Text:

```rust
    /// The first unified row of each chunk.
    unified_chunk_rows: Vec<usize>,
    /// The visual lines each row of the left, right and unified pane takes.
    lines: [RowLines; 3],
}
```

#### V5 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. Bands, scrolling and extents measure rows by their visual lines.

Anchor:

```text
/// A side-by-side pane as [`diff_scroll`](super::diff_scroll) measures it: rows of one height.
struct Geometry<'a> {
    doc: &'a Doc,
    index: &'a [usize],
    rows: usize,
    line_height: f64,
}

impl Geometry<'_> {
    fn row(&self, pos: usize) -> usize {
        let line = self.doc.line_at(pos).number - 1;
        self.index.get(line).copied().unwrap_or(0)
    }

    fn height(&self) -> f64 {
        self.rows as f64 * self.line_height
    }
}

impl PaneLayout for Geometry<'_> {
    fn doc_len(&self) -> usize {
        self.doc.len()
    }

    fn line_top(&self, pos: usize) -> f64 {
        self.row(pos) as f64 * self.line_height
    }

    fn line_bottom(&self, pos: usize) -> f64 {
        (self.row(pos) + 1) as f64 * self.line_height
    }
}

/// The side-by-side pane of `side` as `layout` lays it out.
fn geometry<'a>(docs: &'a [Doc; 2], layout: &'a Layout, side: Side, line_height: f64) -> Geometry<'a> {
    Geometry {
        doc: &docs[ix(side)],
        index: &layout.index[ix(side)],
        rows: layout.rows[Pane::of(side).index()].len(),
        line_height,
    }
}
```

Text:

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

#### V6 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. The painter places rows by `top` now; `row` would be a field never read (`-D warnings`).

Anchor:

```text
struct RowPaint {
    row: usize,
    text: String,
```

Text:

```rust
struct RowPaint {
    text: String,
```

#### V7 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// The old and the new line number (the side-by-side panes show the first only).
    numbers: [Option<usize>; 2],
}
```

Text:

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

#### V8 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// Keeps `look` in step with the settings, once a window's settings are followed.
    settings_listener: Option<u64>,
}
```

Text:

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
}
```

#### V9 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        .detach();
        let mut view = Self {
            focus,
```

Text:

```rust
        .detach();
        let defaults = EditorSettings::default();
        let mut view = Self {
            focus,
```

#### V10 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            look: default_look(),
            settings_listener: None,
        };
```

Text:

```rust
            look: default_look(),
            settings_listener: None,
            editor_soft_wrap: defaults.soft_wrap,
            soft_wrap_files: defaults.general.soft_wrap_files,
            path: String::new(),
            wrap_widths: [None, None],
            wraps: None,
        };
```

#### V11 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    fn editor_settings_changed(&mut self, editor: &EditorSettings, cx: &mut Context<Self>) {
        if let Some(look) = self.look.follow(editor) {
            self.look = look;
            cx.notify();
        }
    }
```

Text:

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
```

#### V12 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        self.caret = self.caret.map(|(side, pos)| (side, pos.min(self.docs[ix(side)].len())));
        self.relayout(cx);
```

Text:

```rust
        self.caret = self.caret.map(|(side, pos)| (side, pos.min(self.docs[ix(side)].len())));
        // Breaks of the old texts would fall on other lines.
        self.wraps = None;
        self.relayout(cx);
```

#### V13 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            rows: [left, right, unified],
            unified_chunk_rows,
        };
        self.selection = None;
```

Text:

```rust
            rows: [left, right, unified],
            unified_chunk_rows,
            lines: Default::default(),
        };
        self.measure(cx);
        self.selection = None;
```

#### V14 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. A change of Use Soft Wraps keeps the rows, the selection and the caret.

Anchor:

```text
        let compare = options.ignore != self.options.ignore || options.highlight != self.options.highlight;
        self.options = options;
        if compare {
            self.rebuild(cx);
        } else {
            self.relayout(cx);
        }
```

Text:

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

#### V15 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
            DiffCommand::ToggleAlignChanges => self.change_options(|o| o.align_changes = !o.align_changes, cx),
```

Text:

```rust

            DiffCommand::ToggleSoftWrap => {
                let wraps = self.soft_wraps();
                self.change_options(|o| o.soft_wrap = Some(!wraps), cx);
            }
```

#### V16 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let top = self.line_height(cx) * row as f32 - self.height(pane) * (1.0 / 3.0);
```

Text:

```rust
        let top = self.row_top(pane, row, self.line_height(cx)) - self.height(pane) * (1.0 / 3.0);
```

#### V17 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let top = self.line_height(cx) * row as f32 - self.height(pane) * 0.5;
```

Text:

```rust
        let top = self.row_top(pane, row, self.line_height(cx)) - self.height(pane) * 0.5;
```

#### V18 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let content = self.line_height(cx) * self.layout.rows[pane.index()].len() as f32;
```

Text:

```rust
        let content = self.line_height(cx) * self.layout.lines[pane.index()].total() as f32;
```

#### V19 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            scroll_height: self.layout.rows[pane.index()].len() as f64 * line_height,
```

Text:

```rust
            scroll_height: self.layout.lines[pane.index()].total() as f64 * line_height,
```

#### V20 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
        self.panes[pane.index()].scroll_top = top.clamp(px(0.0), max);
    }
```

Text:

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
        let slot = usize::from(self.unified());
        let width = self.wrap_widths[slot].filter(|width| *width > px(0.0) && self.soft_wraps());
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
    /// width stands for both side-by-side panes). A zoom changes the code style without a settings
    /// change, so breaks measured in another style are measured again. Whether the rows were
    /// measured again.
    fn wrap_to(&mut self, pane: Pane, width: Pixels, cx: &App) -> bool {
        if pane == Pane::Right {
            return false;
        }
        let slot = usize::from(pane == Pane::Unified);
        let restyled = self.wraps.as_ref().is_some_and(|wraps| wraps.style != self.code_style(cx));
        if self.wrap_widths[slot] == Some(width) && !restyled {
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

#### V21 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. The pointer finds the visual line, then the column on that piece.

Anchor:

```text
    /// The row and column under `position` in `pane`, as it was drawn last.
    fn position_at(&self, pane: Pane, position: Point<Pixels>) -> Option<(usize, u32)> {
        let painted = self.panes[pane.index()].painted.as_ref()?;
        let rows = &self.layout.rows[pane.index()];
        let last = rows.len().checked_sub(1)?;
        let y = position.y - painted.bounds.top() + self.panes[pane.index()].scroll_top;
        let row = ((y / painted.line_height).floor().max(0.0) as usize).min(last);
        let x = position.x - painted.bounds.left() - painted.text_left;
        let col = row
            .checked_sub(painted.first_row)
            .and_then(|i| painted.lines.get(i))
            .and_then(Option::as_ref)
            .map_or(0, |(text, shaped)| col_of_byte(text, shaped.closest_index_for_x(x)));
        Some((row, col))
    }
```

Text:

```rust
    /// The row and column under `position` in `pane`, as it was drawn last.
    fn position_at(&self, pane: Pane, position: Point<Pixels>) -> Option<(usize, u32)> {
        let painted = self.panes[pane.index()].painted.as_ref()?;
        if self.layout.rows[pane.index()].is_empty() {
            return None;
        }
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

#### V22 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// The rows of `pane` in view.
    fn visible_rows(&self, pane: Pane, cx: &App) -> Range<usize> {
        let line_height = self.line_height(cx);
        let rows = self.layout.rows[pane.index()].len();
        let first = ((self.panes[pane.index()].scroll_top / line_height).floor().max(0.0) as usize).min(rows);
        let count = (self.height(pane) / line_height).ceil().max(0.0) as usize + 1;
        first..(first + count).min(rows)
    }
```

Text:

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
        let last = if first_line + count > lines.total() { rows } else { lines.row_at(first_line + count - 1) + 1 };
        first..last.clamp(first, rows)
    }
```

#### V23 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let state = &self.panes[pane.index()];
        let rows = &self.layout.rows[pane.index()];
        let first = ((state.scroll_top / line_height).floor().max(0.0) as usize).min(rows.len());
        let count = (height / line_height).ceil().max(0.0) as usize + 1;
        let last = (first + count).min(rows.len());
        let unified = pane == Pane::Unified;
```

Text:

```rust
        let state = &self.panes[pane.index()];
        let rows = &self.layout.rows[pane.index()];
        let lines = &self.layout.lines[pane.index()];
        let Range { start: first, end: last } = self.rows_in_view(pane, height, line_height);
        let unified = pane == Pane::Unified;
```

#### V24 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            Row::Line { side, line, other } => {
                Some(self.row_paint(row, (side, line, other), unified, selection, &gaps))
            }
```

Text:

```rust
            Row::Line { side, line, other } => {
                let at = (line_height * lines.start(row) as f32, line_height * lines.count(row) as f32);
                Some(self.row_paint(row, (side, line, other), unified, selection, &gaps, at))
            }
```

#### V25 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        selection: Option<RowSpan>,
        gaps: &GapMarks,
    ) -> RowPaint {
```

Text:

```rust
        selection: Option<RowSpan>,
        gaps: &GapMarks,
        (top, height): (Pixels, Pixels),
    ) -> RowPaint {
```

#### V26 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. Goes with V6.

Anchor:

```text
        RowPaint {
            row,
            text,
```

Text:

```rust
        RowPaint {
            text,
```

#### V27 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            gap_below: gap.filter(|(_, end)| *end).map(|(kind, _)| kind),
            numbers,
        }
```

Text:

```rust
            gap_below: gap.filter(|(_, end)| *end).map(|(kind, _)| kind),
            numbers,
            top,
            height,
            breaks: self.wraps.as_ref().map(|wraps| wraps.of(side, line).to_vec()).unwrap_or_default(),
        }
```

#### V28 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let top_of = |row: usize| line_height * row as f32 - scroll_top;
```

Text:

```rust
        let top_of = |row: usize| self.row_top(pane, row, line_height) - scroll_top;
```

#### V29 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. The painter cuts each shaped line at its breaks and paints the pieces one visual line apart.

Anchor:

```text
    ) -> PanePaint {
        let style = self.view.read(cx).code_style(cx);
        let (font_size, line_height, code_font) = (style.size, style.line_height, style.font.clone());
        let plain = theme::color(cx, "fg-editor");
        let number_color = theme::color(cx, "fg-gutter");
        let selection_color = theme::color(cx, "bg-editor-selection");
        let accent = theme::color(cx, "accent");
        let unified = self.pane == Pane::Unified;
        let gutter = theme::length(cx, "w-gutter") * if unified { 2.0 } else { 1.0 };
        let text_left = gutter + px(6.0);
        let snapshot = self.view.read(cx).snapshot(self.pane, bounds.size.height, line_height);
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
            let top = bounds.top() + line_height * row.row as f32 - snapshot.scroll_top;
            if let Some(changed) = row.change {
                let (line_color, gutter_color) = change_colors(cx, changed);
                let width = bounds.size.width - gutter;
                let line = Bounds::new(point(bounds.left() + gutter, top), size(width, line_height));
                paint.fills.push((line, line_color));
                paint.fills.push((Bounds::new(point(bounds.left(), top), size(gutter, line_height)), gutter_color));
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
            let origin = point(bounds.left() + text_left, top);
            let length = utf16::len(&row.text);
            let x = |col: u32| shaped.x_for_index(byte_of_col(&row.text, col.min(length)));
            let span = |from: u32, to: u32| {
                let left = x(from);
                let right = if to > length { x(length) + px(6.0) } else { x(to) };
                Bounds::new(point(origin.x + left, top), size((right - left).max(px(0.0)), line_height))
            };
            for (from, to, kind) in &row.words {
                let color = theme::color(cx, kind.color_token()).opacity(f32::from(WORD_TINT) / 100.0);
                let area = if from == to {
                    Bounds::new(point(origin.x + x(*from) - px(1.0), top), size(px(2.0), line_height))
                } else {
                    span(*from, *to)
                };
                paint.fills.push((area, color));
            }
            if let Some((from, to)) = row.selected {
                paint.fills.push((span(from, to), selection_color));
            }
            if row.current {
                let mark = Bounds::new(point(bounds.left() + gutter - px(3.0), top), size(px(2.0), line_height));
                paint.overs.push((mark, accent));
            }
            for (kind, y) in [(row.gap_above, top - px(1.0)), (row.gap_below, top + line_height - px(1.0))] {
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
            paint.lines.push(Some((row.text, shaped, origin)));
        }
        paint
    }
```

Text:

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

#### V30 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            for (_, shaped, origin) in prepared.lines.iter().flatten() {
                let _ = shaped.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
```

Text:

```rust
            for (_, pieces) in prepared.lines.iter().flatten() {
                for (_, piece, origin) in pieces {
                    let _ = piece.paint(*origin, line_height, TextAlign::Left, None, window, cx);
                }
            }
```

#### V31 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
                .map(|line| line.map(|(text, shaped, _)| (text, shaped)))
```

Text:

```rust
                .map(|line| {
                    line.map(|(text, pieces)| {
                        (text, pieces.into_iter().map(|(start, piece, _)| (start, piece)).collect())
                    })
                })
```

#### T1 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    ToggleAlignChanges,
    /// Reverts the current change of the right side to the left side's text.
```

Text:

```rust
    ToggleAlignChanges,
    /// Use Soft Wraps: the viewer's own choice, stored, in place of the editor's setting.
    ToggleSoftWrap,
    /// Reverts the current change of the right side to the left side's text.
```

#### T2 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text. Use Soft Wraps after Collapse Unchanged Fragments, as DiffEditor.tsx:980-990 orders it.

Anchor:

```text
/// The viewer settings: the viewer kind, the comparison submenus, collapsing, and the scrolling and
/// alignment of side-by-side panes (disabled in the unified viewer).
pub fn settings_menu(options: &DiffViewOptions) -> Vec<MenuItem<DiffCommand>> {
    let split = options.mode == DiffMode::Split;
    vec![
        checked(MenuItem::item("Side-by-side Viewer", DiffCommand::SetMode(DiffMode::Split)), split),
        checked(MenuItem::item("Unified Viewer", DiffCommand::SetMode(DiffMode::Unified)), !split),
        MenuItem::Separator,
        MenuItem::submenu("Ignore Whitespace", ignore_menu(options)),
        MenuItem::submenu("Highlighting", highlight_menu(options)),
        MenuItem::Separator,
        checked(MenuItem::item("Collapse Unchanged Fragments", DiffCommand::ToggleCollapse), options.collapse),
        checked(MenuItem::item("Synchronize Scrolling", DiffCommand::ToggleSyncScroll), options.sync_scroll)
            .disabled(!split),
        checked(MenuItem::item("Align Changes", DiffCommand::ToggleAlignChanges), options.align_changes)
            .disabled(!split),
    ]
}
```

Text:

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

#### T3 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text. The toolbar's settings button checks Use Soft Wraps as the panes wrap (`checked: softWrap`).

Anchor:

```text
            ToolbarAction::Open(kind) => {
                let items = menu(kind, &self.viewer.read(cx).options());
```

Text:

```rust
            ToolbarAction::Open(kind) => {
                let viewer = self.viewer.read(cx);
                let items = match kind {
                    DiffMenu::Settings => viewer_settings_menu(&viewer.options(), viewer.soft_wraps()),
                    DiffMenu::Ignore | DiffMenu::Highlight => menu(kind, &viewer.options()),
                };
```

#### T4 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text. The existing settings-menu test lists the new row.

Anchor:

```text
                "Collapse Unchanged Fragments ✓",
                "Synchronize Scrolling",
```

Text:

```rust
                "Collapse Unchanged Fragments ✓",
                "Use Soft Wraps",
                "Synchronize Scrolling",
```

#### G1 · `app-gpui/src/vcs_diff/dialog_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let viewer = cx.new(|cx| DiffView::new("", "", options, cx));
```

Text:

```rust
        let viewer = cx.new(|cx| {
            let mut viewer = DiffView::new("", "", options, cx);
            viewer.set_path(&data.path, cx);
            viewer
        });
```

#### G2 · `app-gpui/src/vcs_diff/dialog_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        self.files = file_rows(&data);
        self.data = data;
```

Text:

```rust
        self.files = file_rows(&data);
        self.data = data;
        let path = self.data.path.clone();
        self.viewer.update(cx, |viewer, cx| viewer.set_path(&path, cx));
```

#### W1 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text.

Anchor:

```text
use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, settings_menu};
```

Text:

```rust
use crate::vcs_diff::toolbar_view::{DiffCommand, DiffToolbarEvent, viewer_settings_menu};
```

#### W2 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. Diff Settings (the action) checks Use Soft Wraps as the viewer wraps.

Anchor:

```text
        let items = settings_menu(&viewer.read(cx).options());
```

Text:

```rust
        let items = {
            let viewer = viewer.read(cx);
            viewer_settings_menu(&viewer.options(), viewer.soft_wraps())
        };
```

## Part 2: moved code (marks, word edits, links, "(edited)")

#### M1 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor. `moved.rs` is a child of `view.rs`: it reads the viewer's private fields.

Anchor:

```text
use std::time::{Duration, Instant};
```

Text:

```rust


mod moved;
```

#### M2 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. The words edited inside a move are painted as word marks.

Anchor:

```text
        let words = marks[start..]
            .iter()
            .take_while(|mark| mark.from <= at.to)
            .filter(|mark| mark.is_empty() || mark.to > at.from)
            .map(|mark| (col(mark.from), col(mark.to), mark.kind))
            .collect();
```

Text:

```rust
        let mut words: Vec<(u32, u32, DiffKind)> = marks[start..]
            .iter()
            .take_while(|mark| mark.from <= at.to)
            .filter(|mark| mark.is_empty() || mark.to > at.from)
            .map(|mark| (col(mark.from), col(mark.to), mark.kind))
            .collect();
        words.extend(self.move_edits(side, line));
```

#### M3 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    fn render_pane(&self, pane: Pane, cx: &mut Context<Self>) -> Div {
```

Text:

```rust
    fn render_pane(&self, pane: Pane, window: &mut Window, cx: &mut Context<Self>) -> Div {
```

#### M4 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let selector = format!("diff-pane-{}", pane.name());
```

Text:

```rust
        overlays.extend(self.moved_overlays(pane, window, cx));
        let selector = format!("diff-pane-{}", pane.name());
```

#### M5 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. `impl Render for DiffView`.

Anchor:

```text
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
```

Text:

```rust
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
```

#### M6 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
        let body = if self.unified() {
            body.child(self.render_pane(Pane::Unified, cx))
        } else {
            body.child(self.render_pane(Pane::Left, cx))
                .child(self.render_divider(cx))
                .child(self.render_pane(Pane::Right, cx))
        };
```

Text:

```rust
        let body = if self.unified() {
            body.child(self.render_pane(Pane::Unified, window, cx))
        } else {
            let (left, right) = (self.render_pane(Pane::Left, window, cx), self.render_pane(Pane::Right, window, cx));
            body.child(left).child(self.render_divider(cx)).child(right)
        };
```

#### M7 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. Declares the moved-code view tests (after `git mv` of the pending file).

Anchor:

```text
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffViewEvent>>>;
```

Text:

```rust
    use std::rc::Rc;

    mod proofs12_moved_code_view;

    type Events = Rc<RefCell<Vec<DiffViewEvent>>>;
```

#### M8 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text. The two cases move to `proofs12_moved_code_view.rs`, where they are asserted on moved code as the spec does.

Anchor:

```text
    // src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › omits inner edits in Highlight
    // lines mode
    #[gpui_kit::test]
    async fn omits_inner_edits_in_highlight_lines_mode(cx: &mut TestAppContext) {
        let options = DiffViewOptions { highlight: HighlightPolicy::Lines, ..split() };
        let (view, _, cx) = open(cx, "let a = 'first';\n", "let a = 'changed';\n", options);
        assert!(view.read_with(cx, |view, _| view.inner_highlights(Side::B).is_empty()));
        assert_eq!(kinds(&view.read_with(cx, |view, _| view.right_lines()))[0], Some(DiffKind::Modified));
    }

    // src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › respects Do not highlight
    #[gpui_kit::test]
    async fn respects_do_not_highlight(cx: &mut TestAppContext) {
        let options = DiffViewOptions { highlight: HighlightPolicy::None, ..split() };
        let (view, _, cx) = open(cx, "a\nb\n", "a\nc\n", options);
        assert_eq!(view.read_with(cx, |view, _| view.difference_count()), 1);
        assert!(kinds(&view.read_with(cx, |view, _| view.right_lines())).iter().all(Option::is_none));
        assert!(view.read_with(cx, |view, _| view.inner_highlights(Side::A).is_empty()));
    }


```

Text:

```rust

```

## Part 3: the diff as an editor tab (Maximize Editor / Restore Size, chain navigation, Show Diff into tabs)

Apply D25-D30 before D31: D31 makes Show Diff open tabs under the default setting.

#### D1 · `app-gpui/src/vcs_diff/mod.rs` · after

Insert the text right after the anchor.

Anchor:

```text
pub mod model;
```

Text:

```rust

pub mod tab_view;
```

#### D2 · `app-gpui/src/workbench/mod.rs` · before

Insert the text right before the anchor.

Anchor:

```text

mod diff_tools;
```

Text:

```rust

mod diff_tab;
```

#### D3 · `app-gpui/src/workbench/mod.rs` · before

Insert the text right before the anchor. Declares the editor-tab test (after `git mv` of the pending file).

Anchor:

```text
    mod proofs12_diff_window;
```

Text:

```rust
    mod proofs12_diff_tabs;

```

#### D4 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// Where the lines break while the panes wrap.
    wraps: Option<LineBreaks>,
}
```

Text:

```rust
    /// Where the lines break while the panes wrap.
    wraps: Option<LineBreaks>,
    /// Maximize Editor in the toolbar: `Some(true)` while the tool windows are hidden; `None`
    /// where the viewer is not an editor tab.
    maximized: Option<bool>,
}
```

#### D5 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            wrap_widths: [None, None],
            wraps: None,
        };
```

Text:

```rust
            wrap_widths: [None, None],
            wraps: None,
            maximized: None,
        };
```

#### D6 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
        self.options.soft_wrap.unwrap_or_else(|| editor_wraps(self.editor_soft_wrap, &self.soft_wrap_files, &self.path))
    }
```

Text:

```rust


    /// Offers Maximize Editor in the toolbar (`Some`), showing Restore Size while `Some(true)`.
    pub fn set_maximized(&mut self, maximized: Option<bool>, cx: &mut Context<Self>) {
        if self.maximized != maximized {
            self.maximized = maximized;
            cx.notify();
        }
    }
```

#### D7 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
            has_source: self.has_source,
            counter: counter_text(&self.summary(), cx),
```

Text:

```rust
            has_source: self.has_source,
            maximized: self.maximized,
            counter: counter_text(&self.summary(), cx),
```

#### D8 · `app-gpui/src/vcs_diff/view.rs` · after

Insert the text right after the anchor.

Anchor:

```text
                self.change_options(|o| o.soft_wrap = Some(!wraps), cx);
            }
```

Text:

```rust

            DiffCommand::ToggleMaximize => cx.emit(DiffViewEvent::Maximize),
```

#### D9 · `app-gpui/src/vcs_diff/view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// Show the context menu of a pane here, for the line under the pointer.
    ShowMenu { side: Side, line: Option<u32>, position: Point<Pixels> },
}
```

Text:

```rust
    /// Show the context menu of a pane here, for the line under the pointer.
    ShowMenu { side: Side, line: Option<u32>, position: Point<Pixels> },
    /// Hide every tool window, or bring back the ones hidden (Maximize Editor, Restore Size).
    Maximize,
}
```

#### D10 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    ToggleSoftWrap,
    /// Reverts the current change of the right side to the left side's text.
```

Text:

```rust
    ToggleSoftWrap,
    /// Maximize Editor / Restore Size of a diff tab: hides every tool window, or brings them back.
    ToggleMaximize,
    /// Reverts the current change of the right side to the left side's text.
```

#### D11 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    /// The diff has a file to jump to.
    pub has_source: bool,
    pub counter: String,
```

Text:

```rust
    /// The diff has a file to jump to.
    pub has_source: bool,
    /// Maximize Editor is offered (an editor tab), checked while the tool windows are hidden.
    pub maximized: Option<bool>,
    pub counter: String,
```

#### D12 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text. Before the settings button, as diff-editor-tab.tsx:14-23 and DiffEditor.tsx:1227 place it.

Anchor:

```text
        ToolbarItem::Button(button("diff-refresh", "refresh", "Refresh", DiffCommand::Refresh)),
        ToolbarItem::Counter(state.counter.clone()),
        ToolbarItem::Button(ToolbarButton {
            id: "diff-settings",
            icon: "settings",
            label: "Settings",
            enabled: true,
            checked: false,
            action: ToolbarAction::Open(DiffMenu::Settings),
        }),
    ]);
    items
}
```

Text:

```rust
        ToolbarItem::Button(button("diff-refresh", "refresh", "Refresh", DiffCommand::Refresh)),
        ToolbarItem::Counter(state.counter.clone()),
    ]);
    if let Some(maximized) = state.maximized {
        let (icon, label) = if maximized { ("minimize", "Restore Size") } else { ("maximize", "Maximize Editor") };
        items.push(toggle(button("diff-maximize", icon, label, DiffCommand::ToggleMaximize), maximized));
    }
    items.push(ToolbarItem::Button(ToolbarButton {
        id: "diff-settings",
        icon: "settings",
        label: "Settings",
        enabled: true,
        checked: false,
        action: ToolbarAction::Open(DiffMenu::Settings),
    }));
    items
}
```

#### D13 · `app-gpui/src/vcs_diff/toolbar_view.rs` · replace

Replace the anchor with the text.

Anchor:

```text
/// Revert Change when the right side is editable, Refresh, the counter and the settings menu.
```

Text:

```rust
/// Revert Change when the right side is editable, Refresh, the counter, Maximize Editor in an editor tab and the
/// settings menu.
```

#### D14 · `app-gpui/src/vcs_diff/toolbar_view.rs` · after

Insert the text right after the anchor. Its tooltip shows the shortcut of toolwindow.hideAll (diff-editor-tab.tsx:18).

Anchor:

```text
        ToolbarAction::Run(DiffCommand::JumpToSource) => Some("diff.jumpToSource"),
```

Text:

```rust

        ToolbarAction::Run(DiffCommand::ToggleMaximize) => Some("toolwindow.hideAll"),
```

#### D15 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text.

Anchor:

```text
struct LoadedSides {
    left: String,
    right: String,
    /// A side on disk is not text.
    binary: bool,
    /// Neither side is a file, and the diff's path is one in the working tree.
    working_copy: bool,
}
```

Text:

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

#### D16 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text.

Anchor:

```text
async fn load_sides(
```

Text:

```rust
pub(super) async fn load_sides(
```

#### D17 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. A diff tab walks its chain with the diff window's search.

Anchor:

```text
async fn find_file(
```

Text:

```rust
pub(super) async fn find_file(
```

#### D18 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text.

Anchor:

```text
    fn jump_to_working_copy(
```

Text:

```rust
    pub(super) fn jump_to_working_copy(
```

#### D19 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. diff.nextDifference / prevDifference / jumpToSource reach the viewer of the active diff tab.

Anchor:

```text
    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone())
    }
```

Text:

```rust
    /// The viewer of the diff window, else of the active diff tab.
    fn diff_viewer(&self, cx: &App) -> Option<Entity<DiffView>> {
        self.diff_dialog().map(|dialog| dialog.read(cx).viewer().clone()).or_else(|| self.diff_tab_viewer(cx))
    }
```

#### D20 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text.

Anchor:

```text
                // The panes have no menu of their own.
                DiffViewEvent::ShowMenu { .. } => {}
```

Text:

```rust
                // The panes have no menu of their own, and the diff window offers no Maximize Editor.
                DiffViewEvent::ShowMenu { .. } | DiffViewEvent::Maximize => {}
```

#### D21 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. `diffEditorFocus` (compare.tsx:157) while a diff tab has the keyboard; the tab kind has no focus_key.

Anchor:

```text
                || self.merge_editor_focused(window, cx);
        set_context(window, cx, "diffEditorFocus", focused);
```

Text:

```rust
                || self.merge_editor_focused(window, cx)
                || self.diff_tab_focused(window, cx);
        set_context(window, cx, "diffEditorFocus", focused);
```

#### D22 · `app-gpui/src/workbench/tab_views.rs` · after

Insert the text right after the anchor. The editor kind `diff` (vcs/index.ts:154-163). If the other slice inserts after the same anchor first, the anchor is still unique: insert after it all the same.

Anchor:

```text
        build: super::diff_tools::build_local_history_tab,
    },
```

Text:

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

#### D23 · `app-gpui/src/workbench/tool_windows.rs` · replace

Replace the anchor with the text. Restore Size while `!!stashed && !visible.some(Boolean)` (diff-editor-tab.tsx:9).

Anchor:

```text
            self.maximized = self.stashed_maximized.take();
            Effects::PERSIST
        } else {
            Effects::NONE
        }
    }

```

Text:

```rust
            self.maximized = self.stashed_maximized.take();
            Effects::PERSIST
        } else {
            Effects::NONE
        }
    }

    /// Hide All Tool Windows hid every window and brings them back when run again: the editor is
    /// maximized.
    pub fn all_hidden(&self) -> bool {
        self.stashed.is_some() && self.visible.is_empty()
    }

```

#### D24 · `app-gpui/src/workbench/mod.rs` · replace

Replace the anchor with the text. End of `with_layout`: every diff tab's Maximize Editor follows the layout however it changed.

Anchor:

```text
        // The Notifications window may have opened or closed, or just got its view.
        self.sync_notifications(cx);
        self.sync_run_window(cx);
    }

```

Text:

```rust
        // The Notifications window may have opened or closed, or just got its view.
        self.sync_notifications(cx);
        self.sync_run_window(cx);
        self.sync_diff_tabs_maximized(cx);
    }

```

#### D25 · `app-gpui/src/workbench/mod.rs` · before

Insert the text right before the anchor. Test helper for the tests below that are about the diff window.

Anchor:

```text
    pub(super) fn run_action(cx: &mut VisualTestContext, id: &str, arg: Option<serde_json::Value>) {
```

Text:

```rust
    /// Settings › Version Control › Commit › Use non-modal commit interface off: Show Diff opens the
    /// diff window in front, not an editor tab.
    pub(super) fn modal_commit(workbench: &Entity<Workbench>, cx: &mut VisualTestContext) {
        use ide_workspace::settings::SettingsScope;
        let label = workbench.read_with(cx, |w, _| w.label.clone());
        let off = serde_json::json!(false);
        cx.update(|_, cx| {
            crate::platform::settings::set(&label, SettingsScope::Global, "vcs.commit.nonModal", &off, cx)
        })
        .expect("the setting is written");
        wait_long(cx, "the modal commit interface", |cx| {
            cx.update(|_, cx| !crate::platform::settings::effective(&label, cx).vcs.commit.non_modal)
        });
    }


```

#### D26 · `app-gpui/src/workbench/mod.rs` · before

Insert the text right before the anchor. Pins `show_diff_on_a_change_of_the_commit_window_shows_and_walks_its_diff` to the diff window.

Anchor:

```text
        cx.update(|window, cx| execute_action_or_throw("toolwindow.commit", None, window, cx)).expect("it runs");
        let changes = workbench
            .read_with(cx, |w, _| w.tool_window_view(COMMIT_TOOL_WINDOW))
            .and_then(|view| view.downcast::<CommitPanel>().ok())
            .expect("the Commit window holds the changes");
        settle(cx, |cx| changes.read_with(cx, |changes, _| changes.lines().len() == 3));
        assert_eq!(changes.read_with(cx, |changes, _| changes.lines()), ["Changes (2)", "  a.txt", "  b.txt"]);
```

Text:

```rust
        modal_commit(&workbench, cx);

```

#### D27 · `app-gpui/src/workbench/tests/proofs_vcs.rs` · replace

Replace the anchor with the text. Pins `compare_previous_file_walks_back_and_the_diff_settings_menu_opens`.

Anchor:

```text
    std::fs::write(root.join("b.txt"), "x\nY\nz\n").expect("the file is written");
    run_action(cx, "toolwindow.commit", None);
```

Text:

```rust
    std::fs::write(root.join("b.txt"), "x\nY\nz\n").expect("the file is written");
    modal_commit(&workbench, cx);
    run_action(cx, "toolwindow.commit", None);
```

#### D28 · `app-gpui/src/workbench/tests/proofs12_diff_window.rs` · replace

Replace the anchor with the text. Pins `remembers_the_resized_size_for_the_next_opening`, `owns_the_close_shortcut_with_body_or_titlebar_focus`, `does_not_offer_editor_maximization_in_an_embedded_diff` (the two front-dialog tests set it again, harmlessly).

Anchor:

```text
/// A repository whose `a.txt` was changed after its first commit.
fn changed_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
    })
}
```

Text:

```rust
/// A repository whose `a.txt` was changed after its first commit, under the modal commit interface: Show
/// Diff opens the diff window.
fn changed_project(
    cx: &mut TestAppContext,
) -> (Entity<Workbench>, &mut VisualTestContext, [tempfile::TempDir; 2], PathBuf) {
    let (workbench, cx, folders, root) = git_project(cx, |root| {
        init_repo(root);
        std::fs::write(root.join("a.txt"), "two\n").expect("the file is written");
    });
    modal_commit(&workbench, cx);
    (workbench, cx, folders, root)
}
```

#### D29 · `app-gpui/src/workbench/tests/proofs18_e2e_diff_navigation.rs` · before

Insert the text right before the anchor. Pins the diff-window flow `continues_between_changed_files_from_toolbar_buttons_and_the_keymap`.

Anchor:

```text
    for name in ["a.ts", "b.ts"] {
        std::fs::write(root.join(name), "same\nafter\ntail\n").expect("the file is written");
    }
```

Text:

```rust
    modal_commit(&workbench, cx);

```

#### D30 · `app-gpui/src/workbench/vcs_operations.rs` · after

Insert the text right after the anchor. Pins `the_diff_draws_with_the_editors_font_settings_and_follows_them`.

Anchor:

```text
        let (workbench, cx, _folders, root) = git_project(cx, init_repo);
        let path = root.join("a.txt");
        std::fs::write(&path, "one -> two\nx += 1\n").expect("the file is written");
```

Text:

```rust

        super::super::tests::modal_commit(&workbench, cx);
```

#### D31 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. LAST. Show Diff honours Use non-modal commit interface (actions.tsx:395-412, diffInput.ts:151-155). With the default (on) it opens tabs: D25-D30 must be in first.

Anchor:

```text
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

Text:

```rust
        let non_modal = crate::platform::settings::effective(&self.label, cx).vcs.commit.non_modal;
        let i18n = &mut cx.default_global::<Localization>().0;
        let data = match show_diff_files(arg, &paths, view, context, non_modal, i18n) {
            Some(ShownDiffs::FrontWindow(data)) => *data,
            Some(ShownDiffs::Tabs(tabs)) => return self.open_diff_tabs(tabs, window, cx),
            None => return,
        };
```

#### D32 · `app-gpui/src/workbench/diff_window.rs` · replace

Replace the anchor with the text. Doc comment of `show_diff` (wording only).

Anchor:

```text
    /// tool window, else the Project view's selection, else the active file. The diff opens in front of
    /// the window with the other files of the change's group as its chain.
```

Text:

```rust
    /// tool window, else the Project view's selection, else the active file. Under the modal commit
    /// interface the diff opens in front of the window with the other files of the change's group as its
    /// chain; under the non-modal one each file opens in an editor tab, the preview tab for a single one.
```

## Action ids, settings keys and what proves them

No new action ids are registered. Existing ones that change behaviour:

| Id / key                                             | Webview source                                                                                                                | What changes                                                                                                 | Test that proves it                                                                                                                                                                                                                  |
| ---------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `vcs.showDiff`                                       | `src/features/vcs/actions.tsx:367-412`                                                                                        | Under `vcs.commit.nonModal` (default on) each file opens in an editor tab, the preview tab for one (D31)     | `workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab`; the Commit window's activation: `workbench::tests::proofs12_diff_tabs::opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen` |
| `diff.nextDifference` / `diff.prevDifference`        | `compare.tsx:153-168` (`when: "diffEditorFocus"`)                                                                             | Reach the active diff tab's viewer (D19), past the last/first difference walk the chain into the preview tab | `workbench::diff_tab::tests::proofs18_e2e_diff_navigation::continues_between_changed_files_from_toolbar_buttons_and_the_keymap`, `…::skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files`                  |
| `diff.jumpToSource`                                  | `compare.tsx:212`                                                                                                             | Reaches the active diff tab (D19); the tab stays open                                                        | not proved in a tab (the window path keeps its proofs)                                                                                                                                                                               |
| context `diffEditorFocus`                            | `compare.tsx:157`                                                                                                             | Also true while a diff tab has the keyboard (D21)                                                            | the F7 steps of the two tests above                                                                                                                                                                                                  |
| `toolwindow.hideAll`                                 | `src/workbench/toolwindows/actions.ts:181`; the tab button's tooltip shortcut `diff-editor-tab.tsx:18`                        | Maximize Editor runs `ToolWindowLayout::hide_all`; the button follows the layout (D24)                       | `workbench::diff_tab::tests::the_maximize_button_follows_hide_all_tool_windows`, `…::maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor`                                                                     |
| `editor.softWrap`, `editor.general.softWrapFiles`    | `DiffEditor.tsx:372-374` with `softWrapActive` (`src/editor/core/cm/prefs.ts:77-79`, `softWrapFiles \|\| "*"`)                | The panes wrap as the editor would wrap the file, and follow changes (V11)                                   | `workbench::tests::proofs12_diff_soft_wraps::soft_wraps_follow_the_editors_setting`, `…::soft_wraps_reconfigure_without_losing_the_selection`; `vcs_diff::wrap::tests::follows_the_editors_soft_wrap_files`                          |
| `openidea.vcs.diffOptions` › `softWrap` (UI storage) | `diffOptions.ts:30, 53, 72`; menu row `DiffEditor.tsx:985-990` (`checked: softWrap`, `run: setOpts({ softWrap: !softWrap })`) | Use Soft Wraps in the viewer's settings menu, stored, outlasts the editor's setting (V15, T1-T4, W2)         | `workbench::tests::proofs12_diff_soft_wraps::soft_wraps_turn_on_from_the_settings_menu_and_stay_on`; `vcs_diff::toolbar_view::tests::turns_on_aligned_changes_from_the_settings_menu_off_by_default` (row order)                     |
| `vcs.commit.nonModal`                                | `workflowSettings.ts:207`, `diffInput.ts:142`                                                                                 | Tab or diff window for Show Diff (D31)                                                                       | `proofs12_diff_tabs::opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen`; `proofs12_diff_window::opens_the_diff_in_a_front_dialog_not_in_an_editor_tab` (modal)                                                      |

IPC commands the tab path calls: the same as the diff window (`load_sides`: `vcsFileAt` / `fsReadText` through `load_text`, `fsProbe` through `ide_host::ops::bytes::fs_probe`; the chain search `find_file`: `fsProbe` then the sides' texts).

Not registered: `editor.toggleSoftWrap` (`src/editor/core/actions.ts:737-748`, `when: "textEditorActive || softWrapSurfaceFocus"`): it has no native handler at all yet (only a menu row, `menu_spec.rs` `act("editor.toggleSoftWrap", "Soft-Wrap")`), so the diff's soft-wrap surface (`registerSoftWrapSurface`, `DiffEditor.tsx:384`, context key `softWrapSurfaceFocus`) cannot be reached by it; see "Not built".

## parity.json keys this slice should earn

Current state read from `docs/gpui-port/parity.json` and `docs/gpui-port/INVENTORY.md` (2026-11-15).

| Key                                                                                                                                                             | Now                                                                              | After                                              | Proof                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `unit:src/features/vcs/__tests__/diffToolbar.test.tsx`                                                                                                          | partial, 20/25 (missing: 3 soft-wrap cases, 2 diff-tab cases)                    | done, 25/25                                        | the three `workbench::tests::proofs12_diff_soft_wraps::*`; `workbench::diff_tab::tests::maximizes_a_diff_tab_and_restores_the_layout_without_rebuilding_its_editor` (both `%s` rows: no side and the bottom side maximized), `…::restores_the_shared_layout_after_the_diff_tab_is_remounted`; `vcs_diff::tab_view::tests::does_not_offer_editor_maximization_in_an_embedded_diff` (tab form of a case already proved in the window)                                                                                                                                                                                              |
| `unit:src/features/vcs/__tests__/movedCode.test.tsx`                                                                                                            | partial, 13/15                                                                   | done, 15/15                                        | `vcs_diff::view::tests::proofs12_moved_code_view::{labels_edited_moves_and_highlights_only_their_internal_word_edits, shows_native_move_links_follows_them_in_both_directions_and_updates_after_editing, omits_inner_edits_in_highlight_lines_mode, respects_do_not_highlight}` (the last two now on moved code, M8), `vcs_diff::view::moved::tests`                                                                                                                                                                                                                                                                             |
| `unit:src/features/vcs/__tests__/diffDialog.test.tsx`                                                                                                           | partial, 7/8 (missing: the editor tab under non-modal)                           | done, 8/8                                          | `workbench::tests::proofs12_diff_tabs::opens_an_editor_tab_instead_once_the_non_modal_interface_is_chosen`                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `point.registerEditorKind:diff` (INVENTORY "Editor kinds", `src/features/vcs/index.ts:156`)                                                                     | not at parity                                                                    | partial                                            | `workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab`, `vcs_diff::tab_view::tests::*`; partial because the tab is not restored with the session (`restorable: false`) and its working-copy side is not editable                                                                                                                                                                                                                                                                                                                                                                               |
| `e2e:e2e/diff-navigation.spec.ts`                                                                                                                               | partial, 1/3 flows (window)                                                      | done, 3/3                                          | `workbench::diff_tab::tests::proofs18_e2e_diff_navigation::{maximizes_a_diff_preview_tab_and_restores_its_previous_layout, continues_between_changed_files_from_toolbar_buttons_and_the_keymap, skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files}`; the third flow asserts the restored layout (left side maximized, Project view visible) but not the spec's `toolbar toBeHidden` (whether a maximized side covers the editor's toolbar natively is not checked). The window flow `workbench::tests::proofs18_e2e_diff_navigation::continues_between_…` stays, pinned to the modal interface (D29) |
| `action:vcs.showDiff`                                                                                                                                           | partial ("the editor-tab form is not built")                                     | partial (folders and editable panes still missing) | add `workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab` and drop "the editor-tab form" from the missing list                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `point.registerSoftWrapSurface` (`DiffEditor.tsx:384`)                                                                                                          | 0/1                                                                              | stays 0/1                                          | needs `editor.toggleSoftWrap` registered natively first (not in this slice)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `e2e:e2e/compareWith.spec.ts`                                                                                                                                   | partial (diff window, the spec expects an editor tab named after the other file) | stays partial                                      | the compare actions keep the diff window: in a tab the sides are not editable yet (`diff.applyLeftSide`/`applyRightSide` would regress), see "Not built"                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `action:diff.nextDifference`, `action:diff.nextFile`, `action:diff.jumpToSource`, `context:diffEditorFocus`, `action:diff.prevFile`, `action:diff.showSettings` | done                                                                             | done                                               | their proofs (`show_diff_on_a_change_of_the_commit_window_shows_and_walks_its_diff`, `proofs_vcs::compare_previous_file_walks_back_and_the_diff_settings_menu_opens`) now run only on the modal path (D26, D27). The tab path of `diff.nextDifference` / `diffEditorFocus` is proved by the diff-navigation tests above; `diff.nextFile` / `diff.prevFile` / `diff.showSettings` do nothing in a tab (`step_diff_file` and `show_diff_settings` read the diff window only): note it in their proofs                                                                                                                              |

Rows that change wording only: `action:vcs.log.changes.compareWithLocal`, `action:vcs.shelf.showDiffNewTab` keep "opens the diff window, not an editor tab" (true still).

## Not built

- Compare With… / Compare Files / Compare with Clipboard / `vcs.openDiff` / Shelf › Show Diff in a New Tab / a log entry's file still open the diff window where the webview opens a tab by default (`openVcsDiff` / `openDiff`, see above). Routing them to `open_diff_tabs` needs editable working-copy sides in a tab first (`DiffViewEvent::Apply` is ignored by `diff_tab_event`, and `set_editable` is never called on a tab's viewer): `diff_tools.rs` `open_compared_diff` would lose Accept Left/Right Side. This is what keeps `e2e:e2e/compareWith.spec.ts` partial.
- Diff tabs: `diff.nextFile` / `diff.prevFile` (Compare Next/Previous File) and `diff.showSettings` (the action; the toolbar's Settings button works) do nothing in a tab; editing a working-copy side in a tab (arrows, Revert, hover Revert); restoring diff tabs with the session (`restorable: false`); the webview's one-search-at-a-time guard (`navigation.current`, `DiffEditor.tsx:686-698`) is replaced by checking that the tab still shows the diff the search started from.
- Chain navigation in tabs and the button that follows the layout (D24, `sync_diff_tabs_maximized`) were not in the original draft; they are added here because with the default setting Show Diff now opens tabs, and a tab without them would lose what the diff window does today.
- Soft wraps: no hanging indent for wrapped parts (`IndentAdjustment::NoIndent`; `editor.general.softWrapIndent` is not followed). A `LocalHistoryView` viewer gets no path (`set_path` is not called), so it follows the editor's patterns for an empty name (never wraps unless Use Soft Wraps). Only the left pane's width is measured for both side-by-side panes. `editor.toggleSoftWrap` does not reach the diff (no native handler, no `softWrapSurfaceFocus`).
- Moved code: the link is drawn in the UI font size `fs-small` but not the UI font family (`font-ui`); its tooltip is the `aria-label` text ("Go to moved code at line N") when the move has no edits, where the webview has no tooltip; the link is not focusable (`:focus-visible` outline).

## Hypotheses to settle first (each with what settles it)

1. Hypothesis: a `TabView` of kind `diff` is built (its `build` runs and `defer_in` fires) during `with_tabs` → `tabs_changed` for the activated preview tab, as for Local History, and the new view gets the keyboard. Settles it: `cargo nextest run -p openidea-gpui workbench::diff_tab::tests::show_diff_opens_a_titled_diff_tab_that_closes_like_any_tab` and `…::proofs18_e2e_diff_navigation::skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files` (the third F7 is pressed without refocusing).
2. Hypothesis: `editor.closeTab` closes a view tab (its `when` holds with a diff tab active). Settles it: the first test above (`diff_tabs(...).is_empty()` after it).
3. Hypothesis: the GPUI test window is wide enough that hiding the Project view widens `diff-view` by more than 100 px. Settles it: `…::proofs18_e2e_diff_navigation::maximizes_a_diff_preview_tab_and_restores_its_previous_layout`; if it fails on the width only, print `viewer_width` before/after and the window's `viewport_size()`.
4. Hypothesis: a plain `div().id(..).with_tooltip(..)` over a pane (the `diff-move-edit-*` marks) does not swallow the pane's mouse-down (GPUI normal hitbox), and the link's `cx.stop_propagation()` keeps the pane from moving the caret. Settles it: `vcs_diff::view::tests::proofs12_moved_code_view::shows_native_move_links_follows_them_in_both_directions_and_updates_after_editing` plus a click on a marked word in the real window.
5. Hypothesis: the settings listener delivers `SettingsChanged` for both `editor.softWrap` and `editor.general.softWrapFiles`, with `settings::effective(..).editor` already updated when it fires. Settles it: `workbench::tests::proofs12_diff_soft_wraps::soft_wraps_reconfigure_without_losing_the_selection` (it waits on `editor_wraps_set` before asserting).
6. Settled by reading (not hypotheses): `ShapedLineCursor::take_until` rebases each piece's glyph indices and x to 0 (`gpui-pre-0.3.12/src/text_system/line.rs:282-379`), so `x_at`'s `- start` and V21's `start +` are right; `Boundary::ix` is the byte where the next visual line starts (`line_wrapper.rs:942-957`); `App::text_system()` returns `&Arc<TextSystem>` and `line_wrapper(self: &Arc<Self>, ..)` takes it (`app.rs:2492`, `text_system.rs:515`); `ShapedLine::cursor()` asserts `len() == text.len()`, which holds for lines shaped from the row's own text; `gpui_kit::{IndentAdjustment, LineFragment}` come from `pub use line_wrapper::*` (`text_system.rs:11`); icons `maximize`, `minimize`, `diff` exist in `assets/gen/icons.json`; the colour token `link` exists (`tokens.css:63`).
7. Zoom: `theme::zoom` changes `CodeStyle::of` (`theme::zoomed`, `theme::length("fs-code")`) without a settings change; `wrap_to` (V20) therefore re-measures when the stored breaks' style differs from the current one, not only when the width changes. Check in the real window with View › Zoom In on a wrapped diff.

## Tests most likely to fail first

- Existing tests that read `diff_dialog()` after Show Diff under the default settings, if D31 lands without D25-D30: `show_diff_on_a_change_of_the_commit_window_shows_and_walks_its_diff`, `proofs_vcs::compare_previous_file_walks_back_and_the_diff_settings_menu_opens`, `proofs12_diff_window::{remembers_the_resized_size_for_the_next_opening, owns_the_close_shortcut_with_body_or_titlebar_focus, does_not_offer_editor_maximization_in_an_embedded_diff}`, `proofs18_e2e_diff_navigation::continues_between_changed_files_from_toolbar_buttons_and_the_keymap` (window), `vcs_operations::tests::the_diff_draws_with_the_editors_font_settings_and_follows_them`. What was searched: every test that reads `diff_dialog()` (22 tests in `workbench/`, listed by `grep -rln 'diff_dialog()' app-gpui/src/workbench` and traced one by one) and every `"vcs.showDiff"` literal; the others open the diff through `vcs.openDiff`, `open_diff`, the log, the merge editor, the Shelf or the compare actions, which D31 does not change. Keystroke-driven Show Diff (`cmd-d`/`ctrl-d`) reaches a diff window only in `proofs11_shelf::routes_the_configured_show_diff_shortcut_to_the_focused_shelf_selection`, which takes the Shelf path before `show_diff_files`.
- `skips_binary_and_unchanged_siblings_and_restores_after_changing_preview_files`: the keyboard must follow the new preview tab after each chain step (hypothesis 1), and the third F7 must stay on z.ts.
- `labels_edited_moves_and_highlights_only_their_internal_word_edits`: `diff-move-edit-*` exist only when the edit offsets fall on a visible line, and `diff-moved-to-0` sits right of line 1's text inside the default test window.
- `soft_wraps_reconfigure_without_losing_the_selection` (hypothesis 5).
- `maximizes_a_diff_preview_tab_and_restores_its_previous_layout` (hypothesis 3).
- Regression on the default surface (not a test failure): after D31, `diff.nextFile`, `diff.prevFile` (Compare Next/Previous File) and `diff.showSettings` do nothing when Show Diff opened a tab, because `step_diff_file` and `show_diff_settings` read the diff window only. Their "done" rows stay true on the modal path only; extend both to the active diff tab (`find_diff_tab_file`, and a menu position from the tab's `diff-settings` button) in a follow-up, or say it in the rows.
- Compile: V29 is a long replacement; `cut` moves into `placed` after the `x`/`span` closures' last use (NLL); `row.text` moves out of `row` last. V20's `measure` takes `&self.layout` for `rows`/`index` and assigns `self.layout.lines` after the call returns (the borrows end with the call).

## What to look at in the real window

Screenshots after each part (`openidea:verify`, Tauri MCP not involved: the GPUI app):

- Part 1: a diff of a file with lines longer than the pane, Use Soft Wraps on, side by side, aligned off and on, and unified. Wrapped rows stay level across the two panes (an unchanged line and its counterpart take the same height); the divider's bands meet the rows they join; change backgrounds and word marks cover every visual line of a wrapped row; the selection spans visual lines; the caret line marker and line numbers sit on the row's first visual line; scrolling and the scrollbar reach the last row; resizing the window rewraps without jumping. Zoom in and out once.
- Part 2: a moved function (`function first() {…}` moved below two others), Highlight words: the accent stripe along the moved lines' left edge in both panes, the edited word tinted on both sides with its "Edited within moved code" tooltip, the link "To L7 (edited)" / "From L1 (edited)" after the first line's text in a bordered pill; clicking it moves the caret and focus to the other pane at that line, expanding collapsed lines. Highlight lines: no word tint, link kept. Do not highlight: no stripe, no link.
- Part 3: Show Diff on a changed file with default settings opens a preview editor tab titled "a.txt (HEAD ↔ Your version)"; the toolbar has Maximize Editor before Settings (tooltip with the Hide All Tool Windows shortcut); pressing it hides every tool window and the button shows Restore Size pressed; pressing again brings the same windows back, a maximized bottom window included, without reloading the diff (the caret stays). Hide All Tool Windows from the menu flips the button too. F7 past the last difference shows the hint, then the next changed file replaces the preview tab.

## Reconciliation of the original notes (`docs/gpui-port/notes/integration/diff_gaps_integration.md`)

Every anchor of the original notes was counted in the current tree (`dfg/probe.py`): all 63 anchor fragments still occur exactly once. Only `workbench/mod.rs` and `workbench/tab_views.rs` changed after the draft commit (`55c9a09c`); `vcs_diff/view.rs`, `toolbar_view.rs`, `dialog_view.rs`, `diff_window.rs`, `tool_windows.rs` and `viewer_look.rs` did not (`git log -1` on each). `tab_views.rs` changed only by adding `Content::Editor` to its matches; `TabView`, `TabKind`, `open_view_tab`, `tab_view` and `active_tab_view` keep their signatures.

| Original         | Now                                                 | Kind                                                                                                                                                                                                         |
| ---------------- | --------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| 0 (mod lines)    | S1, S2, M1, M7, D1, D2, D3                          | rewritten: rustfmt order (`mod diff_tab;` before `mod diff_tools;`, test mods before `proofs12_diff_window`), and the pending files are `git mv`'d to their final names                                      |
| V1               | V1 (wrap import, part 1), M1 (`mod moved;`, part 2) | kept, split                                                                                                                                                                                                  |
| V2, V3, V4, V5   | V2, V3, V4, V5                                      | kept (V5 now quotes the whole old block instead of a start line and an end marker that was not unique)                                                                                                       |
| V6               | V7, plus V6 and V26                                 | rewritten: `RowPaint.row` is dropped (its only reader is the line V29 replaces; it would be a field never read under `-D warnings`)                                                                          |
| V7               | V8 (soft-wrap fields), D4 (`maximized`)             | kept, split by part                                                                                                                                                                                          |
| V8               | V9, V10 (part 1), D5 (part 3)                       | kept, split                                                                                                                                                                                                  |
| V9               | V11 (part 1), D6 (part 3)                           | kept, split; `set_maximized` now notifies only on a change (it runs on every layout change)                                                                                                                  |
| V10              | D7                                                  | kept (part 3)                                                                                                                                                                                                |
| V11, V12, V13    | V12, V13, V14                                       | kept                                                                                                                                                                                                         |
| V14              | V15 (ToggleSoftWrap), D8 (ToggleMaximize)           | kept, split                                                                                                                                                                                                  |
| V15, V16         | V16-V19                                             | kept                                                                                                                                                                                                         |
| V17              | V20                                                 | rewritten: `wrap_to` re-measures when the zoom changed the code style (not only the width); `measure`'s long line split                                                                                      |
| V18              | V21                                                 | rewritten: the whole function quoted; `checked_sub(1)?;` statement replaced by an `is_empty` check                                                                                                           |
| V19, V20         | V22, V23, V24                                       | kept                                                                                                                                                                                                         |
| V21              | V25, V27 (part 1), M2 (part 2)                      | kept, split                                                                                                                                                                                                  |
| V22              | V28 (part 1), M3-M6 (part 2)                        | kept, split: the `window` parameter lands with its first use (else an unused parameter in part 1)                                                                                                            |
| V23              | V29, V30, V31                                       | kept (whole old body quoted)                                                                                                                                                                                 |
| V24              | M7                                                  | rewritten: declares `proofs12_moved_code_view` (renamed file)                                                                                                                                                |
| V25              | D9                                                  | kept                                                                                                                                                                                                         |
| T1               | T1 (part 1), D10 (part 3)                           | kept, split                                                                                                                                                                                                  |
| T2, T3, T4       | D11, D12, D13, D14                                  | kept                                                                                                                                                                                                         |
| T5, T6, T7       | T2, T3, T4                                          | kept (T5 now quotes the whole old function)                                                                                                                                                                  |
| G1, G2           | G1, G2                                              | kept                                                                                                                                                                                                         |
| W1               | D15, D16, D18, plus D17 (`find_file`)               | kept, extended                                                                                                                                                                                               |
| W2               | D19                                                 | kept                                                                                                                                                                                                         |
| W3               | W1, W2                                              | kept                                                                                                                                                                                                         |
| W4               | D31 (last), plus D25-D30, D32                       | rewritten: last hunk of the slice, after the modal-interface pins of the tests that are about the diff window                                                                                                |
| W5, W6           | D20, D21                                            | kept                                                                                                                                                                                                         |
| 5 (`TAB_KINDS`)  | D22                                                 | kept; the anchor (end of the Local History entry) exists today, so the other slice's insertion after the same anchor does not clash                                                                          |
| 6 (`all_hidden`) | D23                                                 | re-anchored: the original end marker (`} else {\n Effects::NONE\n }\n }`) is not unique in `tool_windows.rs`; the new anchor is the end of `hide_all` from `self.maximized = self.stashed_maximized.take();` |
| —                | D24                                                 | new: the button follows the layout (`sync_diff_tabs_maximized` at the end of `with_layout`)                                                                                                                  |
| —                | M8                                                  | new: the two existing moved-code view tests asserted without moved code move to `proofs12_moved_code_view.rs`                                                                                                |

Dropped: none (no other slice has built any of it: `grep -rn "soft_wraps\|DiffTabView\|moved_overlays\|all_hidden" app-gpui/src` finds only the drafts, plus the unrelated test name `keeps_an_empty_default_group_whose_configurations_are_all_hidden`).

Counts: of the original notes' 43 hunks (40 `###` hunks plus sections 0, 5 and 6), 36 kept (8 of them split across parts: V1, V7, V8, V9, V14, V21, V22, T1), 1 re-anchored (6), 6 rewritten (0, V6, V17, V18, V24, W4), 0 dropped. New here: V6/V26 (field removal), D17, D24, D25-D30 (test pins), M8, D32; and M6 is written so that rustfmt leaves it as it is.
