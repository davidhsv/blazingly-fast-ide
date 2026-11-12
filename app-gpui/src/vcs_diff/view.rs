//! The diff viewer: two read-only panes side by side, or one unified pane with the old lines of each
//! change above its new ones, drawn from the chunks of the comparison ([`idea_chunks`]).
//!
//! Each pane numbers its lines and paints the changed ones in the colour of their change (inserted,
//! deleted, modified, lighter when its words were compared) with the changed words over them, the
//! number gutter tinted alike. Runs of unchanged lines collapse into a placeholder that expands on a
//! click, both panes together. The side-by-side panes keep their own heights and scroll together
//! through [`SyncScroll`]; the divider between them joins each change of the left pane to the right
//! one and carries the ">>" and "<<" arrows toward the editable sides. Previous / Next Difference
//! move the caret from change to change, and past the last one tell the window to move to another
//! file of the diff once pressed again.
//!
//! [`DiffView`] owns no document. It takes the two texts (as stored: `\r\n` and `\r` are shown as
//! line breaks), and reports what needs a document or the window as [`DiffViewEvent`]s: an edit
//! (an arrow, Revert) is a [`TextChange`] in the coordinates of the text with `\n` line breaks,
//! which the window applies before handing the new texts back through [`DiffView::set_texts`].
//!
//! Offsets are UTF-16 code units; lines are 0-based.

use super::cm::{
    ChangedLine, DiffKind, FINE_GUTTER_TINT, FINE_LINE_TINT, LINE_TINT, WORD_TINT, WordMark, changed_gutter_lines,
    word_marks,
};
use super::diff_controls::{
    ArrowDir, ArrowSides, HoverRevert, REVERT_TEXT, apply_chunk, chunk_at, revert_unified_chunk,
};
use super::diff_input::{Reveal, line_ending_badge};
use super::diff_options::{Chunk, DiffChunks, DiffMode, DiffViewOptions, HighlightPolicy, Side, idea_chunks};
use super::diff_scroll::{
    Band, Divider, DividerView, PaneLayout, PaneScroll, SyncScroll, band_at, divider, extent, gap_marks,
};
use super::doc::{Direction, Doc};
use super::toolbar_view::{DiffCommand, DiffSummary, ToolbarItem, ToolbarState, counter_text, toolbar_items};
use crate::editor::buffer::{byte_of_col, col_of_byte};
use crate::editor::view::{HighlightSpan, syntax_token};
use crate::find::utf16;
use crate::platform::i18n::Localization;
use crate::platform::input;
use crate::theme;
use gpui_kit::{
    AnyElement, App, Bounds, ClipboardItem, Context, DispatchPhase, Div, Element, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, Font, GlobalElementId, Hsla, InspectorElementId, IntoElement, Keystroke, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, Render, ScrollEvent,
    ShapedLine, SharedString, Stateful, Style, Task, TextAlign, TextRun, Window, div, fill, font, point, prelude::*,
    px, relative, size,
};
use ide_core::TextChange;
use ide_language::SyntaxHighlightKind;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};

/// Unchanged lines kept in view above and below a collapsed run.
const MARGIN: usize = 3;
/// The fewest unchanged lines that collapse.
const MIN_COLLAPSED: usize = 6;
/// Width of the divider between the side-by-side panes.
const DIVIDER_WIDTH: f32 = 30.0;
/// The height a pane is taken to have before it is first drawn.
const DEFAULT_HEIGHT: f32 = 600.0;
/// How long the hint to press again for the next file stays.
const HINT_DURATION: Duration = Duration::from_millis(2500);
/// Points along each edge of a divider band.
const BAND_STEPS: usize = 16;

fn ix(side: Side) -> usize {
    match side {
        Side::A => 0,
        Side::B => 1,
    }
}

/// `text` with every line break as `\n`, as the panes show it.
fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// UI text in the active language; `text` may be built from a template of the catalog.
fn tx(cx: &App, text: &str) -> String {
    cx.try_global::<Localization>().map_or_else(|| text.to_owned(), |localization| localization.0.tx_template(text))
}

/// The height of a line of the panes.
fn line_height(cx: &App) -> Pixels {
    let height = theme::length(cx, "lh-code");
    if height > px(0.0) { height } else { px(20.0) }
}

/// The first font of the theme's `--font-mono` list that is installed.
fn code_font(cx: &App) -> Font {
    let families =
        cx.try_global::<theme::ActiveTheme>().map(|t| t.theme.font_families("font-mono")).unwrap_or_default();
    let installed = cx.text_system().all_font_names();
    let family =
        families.into_iter().find(|f| installed.iter().any(|name| name == f)).unwrap_or_else(|| "Menlo".to_owned());
    font(SharedString::from(family))
}

/// The lines (0-based, end exclusive) chunk `chunk` covers on `side`. A side without lines is the
/// point where they would be: before the line it starts, or after the last line when it lies past
/// the end of a document without a final line break.
pub fn chunk_lines(chunk: &Chunk, side: Side, doc: &Doc) -> Range<usize> {
    let (from, to) = (chunk.from(side), chunk.to(side));
    if from > doc.len() {
        return doc.lines()..doc.lines();
    }
    let line = doc.line_at(from);
    if from == to {
        let at = if line.from == from { line.number - 1 } else { line.number };
        return at..at;
    }
    (line.number - 1)..doc.line_at((to - 1).min(doc.len())).number
}

/// The unchanged lines of `side` around the chunks: before the first, between two, after the last.
fn gap_ranges(chunks: &[Chunk], side: Side, doc: &Doc) -> Vec<Range<usize>> {
    let mut out = Vec::with_capacity(chunks.len() + 1);
    let mut start = 0;
    for chunk in chunks {
        let lines = chunk_lines(chunk, side, doc);
        out.push(start..lines.start.max(start));
        start = lines.end.max(start);
    }
    out.push(start..doc.lines().max(start));
    out
}

/// A row of a pane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// Line `line` of `side`; `other` is the line it matches on the other side when it is unchanged.
    Line { side: Side, line: usize, other: Option<usize> },
    /// Unchanged lines `from..to` of `side` behind a placeholder; `gap` is the stretch they are in
    /// (0 before the first chunk), which expands in every pane at once.
    Collapsed { side: Side, gap: usize, from: usize, to: usize },
    /// Room left on the side lacking lines, when changes are aligned.
    Filler,
}

/// A pane of the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    Left,
    Right,
    Unified,
}

impl Pane {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
            Self::Unified => 2,
        }
    }

    /// The side whose text the pane shows (the unified pane edits the right one).
    pub fn side(self) -> Side {
        match self {
            Self::Left => Side::A,
            Self::Right | Self::Unified => Side::B,
        }
    }

    fn of(side: Side) -> Self {
        match side {
            Side::A => Self::Left,
            Side::B => Self::Right,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Unified => "unified",
        }
    }
}

/// A line of a side as the viewer shows it: its number (1-based), text and the type of its change
/// (none under "Do not highlight").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneLine {
    pub number: usize,
    pub text: String,
    pub kind: Option<DiffKind>,
}

/// Selected text of a pane, from `anchor` to `head`, each a row and a UTF-16 column of its text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextSelection {
    pub pane: Pane,
    pub anchor: (usize, u32),
    pub head: (usize, u32),
}

impl TextSelection {
    fn ordered(&self) -> RowSpan {
        if self.anchor <= self.head { (self.anchor, self.head) } else { (self.head, self.anchor) }
    }
}

/// What the viewer asks of its window.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffViewEvent {
    /// Apply `change` to the text of `target` (an arrow, Revert), then hand both texts back.
    Apply { target: Side, change: TextChange },
    /// Open the file of `side` at this position.
    JumpToSource { side: Side, line: u32, col: u32 },
    /// Show the file of the diff's chain in this direction ([`super::diff_navigation::ChainSearch`]).
    OpenFile(Direction),
    /// Load both texts again.
    Refresh,
    /// The user changed the viewer settings: store them under [`super::diff_options::DIFF_OPTIONS_KEY`].
    OptionsChanged(DiffViewOptions),
    /// Show the context menu of a pane here, for the line under the pointer.
    ShowMenu { side: Side, line: Option<u32>, position: Point<Pixels> },
}

/// A drawn row's text and its shaping; `None` for a placeholder or a filler.
type PaintedLine = Option<(String, ShapedLine)>;
/// A row's text, shaped, and where it is painted.
type ShapedRow = Option<(String, ShapedLine, Point<Pixels>)>;
/// By side, the gaps at the start of a line: the type of the lines the other side has there, and
/// whether the gap lies below the line (past the end of a document without a final line break).
type GapMarks = [HashMap<usize, (DiffKind, bool)>; 2];
/// A stretch of rows from one (row, column) to another.
type RowSpan = ((usize, u32), (usize, u32));
/// The outline of a divider band and its colour.
type BandShape = (Vec<Point<Pixels>>, Hsla);

/// What a pane drew last, for the pointer.
struct Painted {
    bounds: Bounds<Pixels>,
    text_left: Pixels,
    line_height: Pixels,
    first_row: usize,
    /// The text of each drawn row from `first_row`, shaped; `None` for a placeholder or a filler.
    lines: Vec<PaintedLine>,
}

#[derive(Default)]
struct PaneState {
    scroll_top: Pixels,
    height: Option<Pixels>,
    painted: Option<Painted>,
}

/// The rows of the panes and where each line is among them.
#[derive(Default)]
struct Layout {
    /// Left, right and unified.
    rows: [Vec<Row>; 3],
    /// The row of each line of a side in its side-by-side pane.
    index: [Vec<usize>; 2],
    /// The first unified row of each chunk.
    unified_chunk_rows: Vec<usize>,
}

/// The row of each line of `side` among `rows`, a placeholder's for the lines it hides; a line that
/// is not among them takes the row before it.
fn row_index(rows: &[Row], side: Side, lines: usize) -> Vec<usize> {
    let mut index = vec![usize::MAX; lines];
    let mut set = |line: usize, row: usize| {
        if let Some(slot) = index.get_mut(line) {
            *slot = row;
        }
    };
    for (row, entry) in rows.iter().enumerate() {
        match *entry {
            Row::Line { side: shown, line, .. } if shown == side => set(line, row),
            Row::Collapsed { side: shown, from, to, .. } if shown == side => (from..to).for_each(|line| set(line, row)),
            Row::Line { .. } | Row::Collapsed { .. } | Row::Filler => {}
        }
    }
    let mut last = 0;
    for slot in &mut index {
        if *slot == usize::MAX {
            *slot = last;
        } else {
            last = *slot;
        }
    }
    index
}

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

/// The band of both panes around `y` of side `from`.
fn band_between(chunks: &[Chunk], a: &Geometry<'_>, b: &Geometry<'_>, y: f64, from: Side) -> Band {
    band_at(chunks, |c| extent(a, c.from_a, c.to_a), |c| extent(b, c.from_b, c.to_b), (a.height(), b.height()), y, from)
}

/// What a line row paints, read from the viewer before drawing.
struct RowPaint {
    row: usize,
    text: String,
    /// Syntax colours as UTF-16 columns of the line.
    spans: Vec<(u32, u32, SyntaxHighlightKind)>,
    change: Option<ChangedLine>,
    /// Changed words as columns; an end past the text runs over the line break.
    words: Vec<(u32, u32, DiffKind)>,
    selected: Option<(u32, u32)>,
    /// The line belongs to the current difference.
    current: bool,
    /// The other side has lines above (below) this one that this side lacks.
    gap_above: Option<DiffKind>,
    gap_below: Option<DiffKind>,
    /// The old and the new line number (the side-by-side panes show the first only).
    numbers: [Option<usize>; 2],
}

struct PaneSnapshot {
    scroll_top: Pixels,
    first_row: usize,
    /// The rows in view from `first_row`; `None` for a placeholder or a filler.
    rows: Vec<Option<RowPaint>>,
}

pub struct DiffView {
    focus: FocusHandle,
    /// The texts as given, and as shown.
    raw: [String; 2],
    docs: [Doc; 2],
    diff: DiffChunks,
    /// The change of each line of a side.
    kinds: [Vec<Option<ChangedLine>>; 2],
    /// The chunk each line of a side belongs to.
    chunk_of_line: [Vec<Option<usize>>; 2],
    words: [Vec<WordMark>; 2],
    options: DiffViewOptions,
    editable: ArrowSides,
    highlights: [Vec<HighlightSpan>; 2],
    labels: [String; 2],
    binary: bool,
    has_source: bool,
    /// The diff's chain has a file before (after) this one.
    neighbours: [bool; 2],
    /// Stretches of unchanged lines the user expanded.
    expanded: HashSet<usize>,
    layout: Layout,
    panes: [PaneState; 3],
    sync: SyncScroll,
    /// The difference at the caret.
    current: Option<usize>,
    caret: Option<(Side, usize)>,
    /// The side-by-side pane the user worked in last.
    focused: Side,
    selection: Option<TextSelection>,
    dragging: bool,
    hover: HoverRevert,
    hover_side: Option<Side>,
    /// Pressed past the last (first) difference once: pressing again moves to the next file.
    pending_file: Option<Direction>,
    hint: Option<Direction>,
    _hint_timer: Option<Task<()>>,
    epoch: Instant,
    key_claim: u64,
}

impl EventEmitter<DiffViewEvent> for DiffView {}

impl DiffView {
    /// A viewer of `left` against `right` under `options` (a diff that asks for a viewer kind of its
    /// own passes it in `options`; nothing is stored until the user changes a setting).
    pub fn new(left: &str, right: &str, options: DiffViewOptions, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let weak = cx.weak_entity();
        let key_claim = input::claim_keys(
            focus.clone(),
            move |keystroke, _, cx| weak.update(cx, |view, cx| view.key(keystroke, cx)).unwrap_or(false),
            cx,
        );
        cx.on_release(|view, cx| input::release_keys(view.key_claim, cx)).detach();
        let mut view = Self {
            focus,
            raw: [left.to_owned(), right.to_owned()],
            docs: [Doc::new(""), Doc::new("")],
            diff: DiffChunks::default(),
            kinds: [Vec::new(), Vec::new()],
            chunk_of_line: [Vec::new(), Vec::new()],
            words: [Vec::new(), Vec::new()],
            options,
            editable: ArrowSides { to_a: false, to_b: false },
            highlights: [Vec::new(), Vec::new()],
            labels: [String::new(), String::new()],
            binary: false,
            has_source: false,
            neighbours: [false, false],
            expanded: HashSet::new(),
            layout: Layout::default(),
            panes: Default::default(),
            sync: SyncScroll::new(options.sync_scroll),
            current: None,
            caret: None,
            focused: Side::B,
            selection: None,
            dragging: false,
            hover: HoverRevert::default(),
            hover_side: None,
            pending_file: None,
            hint: None,
            _hint_timer: None,
            epoch: Instant::now(),
            key_claim,
        };
        view.rebuild(cx);
        view
    }

    // ---------------------------------------------------------------------------------------------
    // What the window tells the viewer

    /// Shows new texts (after an edit was applied, or a reload).
    pub fn set_texts(&mut self, left: &str, right: &str, cx: &mut Context<Self>) {
        if self.raw[0] == left && self.raw[1] == right {
            return;
        }
        self.raw = [left.to_owned(), right.to_owned()];
        self.expanded.clear();
        self.rebuild(cx);
        cx.notify();
    }

    /// The sides the arrows and Revert may write to.
    pub fn set_editable(&mut self, editable: ArrowSides, cx: &mut Context<Self>) {
        self.editable = editable;
        cx.notify();
    }

    /// Syntax colours of a side's text, sorted by their start.
    pub fn set_highlights(&mut self, side: Side, mut spans: Vec<HighlightSpan>, cx: &mut Context<Self>) {
        spans.sort_by_key(|span| span.from);
        self.highlights[ix(side)] = spans;
        cx.notify();
    }

    /// The labels over the panes (the revisions shown).
    pub fn set_labels(&mut self, left: String, right: String, cx: &mut Context<Self>) {
        self.labels = [left, right];
        cx.notify();
    }

    /// A side is binary: the panes give way to a notice.
    pub fn set_binary(&mut self, binary: bool, cx: &mut Context<Self>) {
        self.binary = binary;
        cx.notify();
    }

    /// The diff has a file to jump to: the toolbar offers Jump to Source.
    pub fn set_has_source(&mut self, has_source: bool, cx: &mut Context<Self>) {
        self.has_source = has_source;
        cx.notify();
    }

    /// Whether the diff's chain has a file before and after this one.
    pub fn set_neighbours(&mut self, previous: bool, next: bool, cx: &mut Context<Self>) {
        self.neighbours = [previous, next];
        self.pending_file = None;
        self.hint = None;
        cx.notify();
    }

    /// Replaces the settings without reporting them as the user's.
    pub fn set_options(&mut self, options: DiffViewOptions, cx: &mut Context<Self>) {
        self.apply_options(options, cx);
    }

    // ---------------------------------------------------------------------------------------------
    // What the viewer shows

    pub fn options(&self) -> DiffViewOptions {
        self.options
    }

    pub fn difference_count(&self) -> usize {
        self.diff.chunks.len()
    }

    /// The difference at the caret.
    pub fn current_difference(&self) -> Option<usize> {
        self.current
    }

    pub fn chunks(&self) -> &[Chunk] {
        &self.diff.chunks
    }

    fn lines_of(&self, side: Side) -> Vec<PaneLine> {
        let i = ix(side);
        let doc = &self.docs[i];
        let shown = self.options.highlight != HighlightPolicy::None;
        (0..doc.lines())
            .map(|line| PaneLine {
                number: line + 1,
                text: doc.line_text(doc.line(line + 1)),
                kind: self.kinds[i].get(line).copied().flatten().filter(|_| shown).map(|changed| changed.kind),
            })
            .collect()
    }

    /// Every line of the left side with the type of its change.
    pub fn left_lines(&self) -> Vec<PaneLine> {
        self.lines_of(Side::A)
    }

    pub fn right_lines(&self) -> Vec<PaneLine> {
        self.lines_of(Side::B)
    }

    /// The rows of a pane, placeholders and fillers included.
    pub fn rows(&self, pane: Pane) -> &[Row] {
        &self.layout.rows[pane.index()]
    }

    /// The unified pane's lines in order, each with its side and the type it is drawn in: the old
    /// lines of a change deleted, above its new lines inserted.
    pub fn unified_lines(&self) -> Vec<(Side, PaneLine)> {
        let shown = self.options.highlight != HighlightPolicy::None;
        let line = |side: Side, line: usize| {
            let doc = &self.docs[ix(side)];
            let changed = self.kinds[ix(side)].get(line).copied().flatten().filter(|_| shown);
            let kind = changed.map(|_| if side == Side::A { DiffKind::Deleted } else { DiffKind::Inserted });
            (side, PaneLine { number: line + 1, text: doc.line_text(doc.line(line + 1)), kind })
        };
        self.layout.rows[Pane::Unified.index()]
            .iter()
            .filter_map(|row| match *row {
                Row::Line { side, line: n, .. } => Some(line(side, n)),
                Row::Collapsed { .. } | Row::Filler => None,
            })
            .collect()
    }

    /// The unchanged lines of a pane's side hidden behind placeholders.
    pub fn collapsed_ranges(&self, pane: Pane) -> Vec<Range<usize>> {
        self.layout.rows[pane.index()]
            .iter()
            .filter_map(|row| match *row {
                Row::Collapsed { from, to, .. } => Some(from..to),
                Row::Line { .. } | Row::Filler => None,
            })
            .collect()
    }

    /// The changed words of a side under "Highlight words".
    pub fn inner_highlights(&self, side: Side) -> &[WordMark] {
        &self.words[ix(side)]
    }

    pub fn scroll_top(&self, pane: Pane) -> Pixels {
        self.panes[pane.index()].scroll_top
    }

    pub fn selection(&self) -> Option<TextSelection> {
        self.selection
    }

    /// The hint shown past the last (first) difference while the chain has another file.
    pub fn nav_hint(&self) -> Option<&'static str> {
        self.hint.map(|direction| match direction {
            Direction::Next => "Press again to go to the next file",
            Direction::Previous => "Press again to go to the previous file",
        })
    }

    /// How the two texts compare.
    pub fn summary(&self) -> DiffSummary {
        let count = self.diff.chunks.len();
        DiffSummary {
            binary: self.binary,
            count,
            current: self.current,
            moves: self.diff.moves.len(),
            whitespace_only: self.diff.whitespace_only,
            identical: count == 0 && self.raw[0] == self.raw[1],
            eol_only: self.raw[0] != self.raw[1] && self.docs[0].units() == self.docs[1].units(),
        }
    }

    /// The toolbar's entries for the viewer as it stands.
    pub fn toolbar(&self, cx: &App) -> Vec<ToolbarItem> {
        toolbar_items(&ToolbarState {
            options: self.options,
            difference_count: self.diff.chunks.len(),
            editable_right: self.editable.to_b && !self.binary,
            has_source: self.has_source,
            counter: counter_text(&self.summary(), cx),
        })
    }

    /// The line of the other side matching `line` of `from`: lines before a change keep their place,
    /// a line inside one keeps its offset into it (at most its last line), lines past it shift.
    pub fn map_line(&self, from: Side, line: usize) -> usize {
        let (src, dst) = (&self.docs[ix(from)], &self.docs[ix(from.other())]);
        let span = |start: usize, end: usize, doc: &Doc| {
            let first = doc.line_at(start.min(doc.len())).number - 1;
            let last = if end > start { doc.line_at((end - 1).min(doc.len())).number } else { first };
            (first, last)
        };
        let mut shift = 0isize;
        for chunk in &self.diff.chunks {
            let (from_src, to_src) = span(chunk.from(from), chunk.to(from), src);
            if line < from_src {
                break;
            }
            let (from_dst, to_dst) = span(chunk.from(from.other()), chunk.to(from.other()), dst);
            if line < to_src {
                return (from_dst + (line - from_src)).min(from_dst.max(to_dst.saturating_sub(1)));
            }
            shift = to_dst as isize - to_src as isize;
        }
        (line as isize + shift).clamp(0, dst.lines() as isize - 1) as usize
    }

    fn unified(&self) -> bool {
        self.options.mode == DiffMode::Unified
    }

    fn line_text(&self, side: Side, line: usize) -> String {
        let doc = &self.docs[ix(side)];
        if line < doc.lines() { doc.line_text(doc.line(line + 1)) } else { String::new() }
    }

    // ---------------------------------------------------------------------------------------------
    // Comparing and laying out

    /// Compares the texts again under the current settings.
    fn rebuild(&mut self, cx: &App) {
        self.docs = [Doc::new(&normalized(&self.raw[0])), Doc::new(&normalized(&self.raw[1]))];
        self.diff = idea_chunks(&self.docs[0], &self.docs[1], self.options.ignore, self.options.highlight);
        for side in [Side::A, Side::B] {
            let (i, doc) = (ix(side), &self.docs[ix(side)]);
            let mut kinds = vec![None; doc.lines()];
            for changed in changed_gutter_lines(&self.diff.chunks, side, doc) {
                if let Some(slot) = kinds.get_mut(changed.line.number - 1) {
                    *slot = Some(changed);
                }
            }
            let mut chunk_of_line = vec![None; doc.lines()];
            for (index, chunk) in self.diff.chunks.iter().enumerate() {
                for line in chunk_lines(chunk, side, doc) {
                    if let Some(slot) = chunk_of_line.get_mut(line) {
                        *slot = Some(index);
                    }
                }
            }
            self.kinds[i] = kinds;
            self.chunk_of_line[i] = chunk_of_line;
            self.words[i] = if self.options.highlight == HighlightPolicy::Words {
                word_marks(&self.diff, side, 0..doc.len())
            } else {
                Vec::new()
            };
        }
        let count = self.diff.chunks.len();
        self.hover.chunks_changed(count);
        self.current = self.current.filter(|current| *current < count);
        self.caret = self.caret.map(|(side, pos)| (side, pos.min(self.docs[ix(side)].len())));
        self.relayout(cx);
    }

    /// The part of stretch `gap` (`range`) that collapses: a margin stays in view around it, except
    /// at the ends of the document.
    fn folded(&self, gap: usize, range: &Range<usize>) -> Option<Range<usize>> {
        if !self.options.collapse || self.expanded.contains(&gap) {
            return None;
        }
        let from = if gap > 0 { range.start + MARGIN } else { range.start };
        let to = if gap < self.diff.chunks.len() { range.end.saturating_sub(MARGIN) } else { range.end };
        (to >= from + MIN_COLLAPSED).then_some(from..to)
    }

    /// The rows of `side` in segments that line up with the other side's: for each stretch of
    /// unchanged lines the lines above its collapsed part, the collapsed part and the lines below it,
    /// then the chunk after it.
    fn segments(&self, side: Side, gaps: &[Vec<Range<usize>>; 2]) -> Vec<Vec<Row>> {
        let (own, other) = (&gaps[ix(side)], &gaps[ix(side.other())]);
        let doc = &self.docs[ix(side)];
        let mut out = Vec::with_capacity(own.len() * 4);
        for (gap, range) in own.iter().enumerate() {
            let paired = |line: usize| {
                let matched = other.get(gap).map(|r| (r.start + (line - range.start), r.end));
                matched.filter(|(at, end)| at < end).map(|(at, _)| at)
            };
            let line_row = |line: usize| Row::Line { side, line, other: paired(line) };
            match self.folded(gap, range) {
                Some(fold) => {
                    out.push((range.start..fold.start).map(&line_row).collect());
                    out.push(vec![Row::Collapsed { side, gap, from: fold.start, to: fold.end }]);
                    out.push((fold.end..range.end).map(&line_row).collect());
                }
                None => {
                    out.push(range.clone().map(&line_row).collect());
                    out.push(Vec::new());
                    out.push(Vec::new());
                }
            }
            if let Some(chunk) = self.diff.chunks.get(gap) {
                out.push(chunk_lines(chunk, side, doc).map(|line| Row::Line { side, line, other: None }).collect());
            }
        }
        out
    }

    /// Lays the rows out again (another viewer kind, a fragment expanded, new chunks).
    fn relayout(&mut self, cx: &App) {
        let gaps = [Side::A, Side::B].map(|side| gap_ranges(&self.diff.chunks, side, &self.docs[ix(side)]));
        let (a, b) = (self.segments(Side::A, &gaps), self.segments(Side::B, &gaps));
        let (mut left, mut right, mut unified) = (Vec::new(), Vec::new(), Vec::new());
        let mut unified_chunk_rows = Vec::with_capacity(self.diff.chunks.len());
        for (i, (a, b)) in a.into_iter().zip(b).enumerate() {
            let chunk = i % 4 == 3;
            if chunk {
                unified_chunk_rows.push(unified.len());
                unified.extend(a.iter().copied());
            }
            unified.extend(b.iter().copied());
            let padded = a.len().max(b.len());
            let (a_len, b_len) = (a.len(), b.len());
            left.extend(a);
            right.extend(b);
            if self.options.align_changes {
                left.extend(std::iter::repeat_n(Row::Filler, padded - a_len));
                right.extend(std::iter::repeat_n(Row::Filler, padded - b_len));
            }
        }
        let lines = [self.docs[0].lines(), self.docs[1].lines()];
        self.layout = Layout {
            index: [row_index(&left, Side::A, lines[0]), row_index(&right, Side::B, lines[1])],
            rows: [left, right, unified],
            unified_chunk_rows,
        };
        self.selection = None;
        for pane in [Pane::Left, Pane::Right, Pane::Unified] {
            let top = self.panes[pane.index()].scroll_top;
            self.set_scroll(pane, top, cx);
        }
    }

    /// Expands the unchanged lines of stretch `gap` in every pane.
    pub fn expand(&mut self, gap: usize, cx: &mut Context<Self>) {
        if self.expanded.insert(gap) {
            self.relayout(cx);
            cx.notify();
        }
    }

    // ---------------------------------------------------------------------------------------------
    // Settings and commands

    fn apply_options(&mut self, options: DiffViewOptions, cx: &mut Context<Self>) -> bool {
        if options == self.options {
            return false;
        }
        let compare = options.ignore != self.options.ignore || options.highlight != self.options.highlight;
        self.options = options;
        if compare {
            self.rebuild(cx);
        } else {
            self.relayout(cx);
        }
        if self.sync.set_sync(options.sync_scroll).is_some() && options.sync_scroll {
            self.resync(cx);
        }
        cx.notify();
        true
    }

    /// Changes the settings as the user asked, and reports them.
    fn change_options(&mut self, change: impl FnOnce(&mut DiffViewOptions), cx: &mut Context<Self>) {
        let mut options = self.options;
        change(&mut options);
        if self.apply_options(options, cx) {
            cx.emit(DiffViewEvent::OptionsChanged(options));
        }
    }

    /// Carries out a command of the toolbar or its menus.
    pub fn run_command(&mut self, command: DiffCommand, cx: &mut Context<Self>) {
        match command {
            DiffCommand::PreviousDifference => self.step(Direction::Previous, cx),
            DiffCommand::NextDifference => self.step(Direction::Next, cx),
            DiffCommand::JumpToSource => self.jump_to_source(cx),
            DiffCommand::SetMode(mode) => self.change_options(|o| o.mode = mode, cx),
            DiffCommand::SetIgnore(ignore) => self.change_options(|o| o.ignore = ignore, cx),
            DiffCommand::SetHighlight(highlight) => self.change_options(|o| o.highlight = highlight, cx),
            DiffCommand::ToggleCollapse => self.change_options(|o| o.collapse = !o.collapse, cx),
            DiffCommand::ToggleSyncScroll => self.change_options(|o| o.sync_scroll = !o.sync_scroll, cx),
            DiffCommand::ToggleAlignChanges => self.change_options(|o| o.align_changes = !o.align_changes, cx),
            DiffCommand::RevertChange => self.revert_current(cx),
            DiffCommand::Refresh => cx.emit(DiffViewEvent::Refresh),
        }
    }

    /// The side the caret moves on: the focused side-by-side pane's, the right one when unified.
    fn caret_side(&self) -> Side {
        if self.unified() { Side::B } else { self.focused }
    }

    pub fn next_difference(&mut self, cx: &mut Context<Self>) {
        self.step(Direction::Next, cx);
    }

    pub fn previous_difference(&mut self, cx: &mut Context<Self>) {
        self.step(Direction::Previous, cx);
    }

    /// Moves to the next (previous) difference after the caret; past the last (first) one, hints at
    /// the next file of the chain, and moves there when pressed again.
    fn step(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let side = self.caret_side();
        let head = self.caret.filter(|(caret_side, _)| *caret_side == side).map(|(_, pos)| pos);
        let chunks = &self.diff.chunks;
        let found = match direction {
            Direction::Next => chunks.iter().position(|c| head.is_none_or(|head| c.from(side) > head)),
            Direction::Previous => head.and_then(|head| chunks.iter().rposition(|c| c.from(side) < head)),
        };
        let Some(index) = found else { return self.boundary(direction, cx) };
        (self.pending_file, self.hint, self._hint_timer) = (None, None, None);
        self.go_to(index, side, cx);
    }

    fn boundary(&mut self, direction: Direction, cx: &mut Context<Self>) {
        let has_file = match direction {
            Direction::Previous => self.neighbours[0],
            Direction::Next => self.neighbours[1],
        };
        if !has_file {
            return;
        }
        if self.pending_file == Some(direction) {
            (self.pending_file, self.hint, self._hint_timer) = (None, None, None);
            cx.emit(DiffViewEvent::OpenFile(direction));
        } else {
            self.pending_file = Some(direction);
            self.hint = Some(direction);
            self._hint_timer = Some(cx.spawn(async move |view, cx| {
                cx.background_executor().timer(HINT_DURATION).await;
                let _ = view.update(cx, |view: &mut DiffView, cx| {
                    view.hint = None;
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    /// Puts the caret at the start of chunk `index` on `side` and scrolls it a third down its pane.
    pub fn go_to(&mut self, index: usize, side: Side, cx: &mut Context<Self>) {
        let Some(chunk) = self.diff.chunks.get(index) else { return };
        let doc = &self.docs[ix(side)];
        let pos = chunk.from(side).min(doc.len());
        let line = doc.line_at(pos).number - 1;
        (self.current, self.caret, self.selection) = (Some(index), Some((side, pos)), None);
        let (pane, row) = if self.unified() {
            (Pane::Unified, self.layout.unified_chunk_rows.get(index).copied().unwrap_or(0))
        } else {
            (Pane::of(side), self.layout.index[ix(side)].get(line).copied().unwrap_or(0))
        };
        let top = line_height(cx) * row as f32 - self.height(pane) * (1.0 / 3.0);
        let now = self.now_ms();
        self.sync.scroll_to(side, false, now);
        self.scrolled(pane, top, cx);
    }

    /// Selects the first (last) difference, as a diff reached along its chain asks.
    pub fn reveal(&mut self, reveal: Reveal, cx: &mut Context<Self>) {
        let count = self.diff.chunks.len();
        let index = match reveal {
            Reveal::First => 0,
            Reveal::Last => count.saturating_sub(1),
        };
        if index < count {
            self.go_to(index, self.caret_side(), cx);
        }
    }

    /// Puts the caret at the start of `line` of `side` and scrolls it to the middle of its pane,
    /// expanding the unchanged lines around it when they are collapsed.
    pub fn reveal_line(&mut self, side: Side, line: usize, cx: &mut Context<Self>) {
        let line = line.min(self.docs[ix(side)].lines().saturating_sub(1));
        let chunk = self.chunk_of_line[ix(side)].get(line).copied().flatten();
        // The unified pane shows the left side's lines of the changes only: an unchanged one is shown
        // as the right side's line it matches.
        let (pane, side, line) = match (self.unified(), side, chunk) {
            (true, Side::A, None) => (Pane::Unified, Side::B, self.map_line(Side::A, line)),
            (true, _, _) => (Pane::Unified, side, line),
            (false, _, _) => (Pane::of(side), side, line),
        };
        let folded = self.layout.rows[pane.index()].iter().find_map(|row| match *row {
            Row::Collapsed { side: shown, gap, from, to } if shown == side && (from..to).contains(&line) => Some(gap),
            Row::Line { .. } | Row::Collapsed { .. } | Row::Filler => None,
        });
        if let Some(gap) = folded {
            self.expanded.insert(gap);
            self.relayout(cx);
        }
        let is_row = |row: &Row| matches!(*row, Row::Line { side: shown, line: at, .. } if shown == side && at == line);
        let row = self.layout.rows[pane.index()].iter().position(is_row).unwrap_or(0);
        let pos = self.docs[ix(side)].line(line + 1).from;
        self.current = self.chunk_of_line[ix(side)].get(line).copied().flatten();
        self.caret = match (pane, side, self.current) {
            (Pane::Unified, Side::A, Some(index)) => self.diff.chunks.get(index).map(|c| (Side::B, c.from_b)),
            _ => Some((side, pos)),
        };
        if pane != Pane::Unified {
            self.focused = side;
        }
        let top = line_height(cx) * row as f32 - self.height(pane) * 0.5;
        self.scrolled(pane, top, cx);
    }

    fn jump_to_source(&mut self, cx: &mut Context<Self>) {
        let side = self.caret_side();
        let doc = &self.docs[ix(side)];
        let pos = self.caret.filter(|(caret_side, _)| *caret_side == side).map_or(0, |(_, pos)| pos.min(doc.len()));
        let line = doc.line_at(pos);
        cx.emit(DiffViewEvent::JumpToSource { side, line: (line.number - 1) as u32, col: (pos - line.from) as u32 });
    }

    fn writable(&self, side: Side) -> bool {
        match side {
            Side::A => self.editable.to_a,
            Side::B => self.editable.to_b,
        }
    }

    /// Copies chunk `index` from `source` over the other side (an arrow, the hover Revert).
    pub fn apply_change(&mut self, index: usize, source: Side, cx: &mut Context<Self>) {
        if !self.writable(source.other()) {
            return;
        }
        if let Some((target, change)) = apply_chunk(&self.diff.chunks, index, source, &self.docs[0], &self.docs[1]) {
            cx.emit(DiffViewEvent::Apply { target, change });
        }
    }

    /// Reverts chunk `index` of the unified pane to the left side's text.
    pub fn revert_unified(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(pos) = self.diff.chunks.get(index).map(|chunk| chunk.from_b) else { return };
        if !self.editable.to_b {
            return;
        }
        if let Some(change) = revert_unified_chunk(&self.diff.chunks, pos, &self.docs[0], &self.docs[1]) {
            cx.emit(DiffViewEvent::Apply { target: Side::B, change });
        }
    }

    /// Revert Change: the current difference (else the first) of the right side.
    fn revert_current(&mut self, cx: &mut Context<Self>) {
        if self.diff.chunks.is_empty() {
            return;
        }
        let index = self.current.unwrap_or(0);
        if self.unified() { self.revert_unified(index, cx) } else { self.apply_change(index, Side::A, cx) }
    }

    // ---------------------------------------------------------------------------------------------
    // Scrolling

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn height(&self, pane: Pane) -> Pixels {
        self.panes[pane.index()].height.unwrap_or(px(DEFAULT_HEIGHT))
    }

    fn max_scroll(&self, pane: Pane, cx: &App) -> Pixels {
        let content = line_height(cx) * self.layout.rows[pane.index()].len() as f32;
        (content - self.height(pane)).max(px(0.0))
    }

    fn set_scroll(&mut self, pane: Pane, top: Pixels, cx: &App) {
        let max = self.max_scroll(pane, cx);
        self.panes[pane.index()].scroll_top = top.clamp(px(0.0), max);
    }

    fn pane_scroll(&self, pane: Pane, cx: &App) -> PaneScroll {
        let line_height = f64::from(line_height(cx));
        PaneScroll {
            top: f64::from(self.panes[pane.index()].scroll_top),
            left: 0.0,
            client_height: f64::from(self.height(pane)),
            scroll_height: self.layout.rows[pane.index()].len() as f64 * line_height,
            max_left: 0.0,
            doc_offset: 0.0,
        }
    }

    /// Scrolls `pane` by `delta` (positive: further down).
    pub fn scroll_by(&mut self, pane: Pane, delta: Pixels, cx: &mut Context<Self>) {
        self.sync.interrupt();
        let top = self.panes[pane.index()].scroll_top + delta;
        self.scrolled(pane, top, cx);
    }

    /// Scrolls `pane` to `top`; the other side-by-side pane follows while scrolling is synchronized.
    fn scrolled(&mut self, pane: Pane, top: Pixels, cx: &mut Context<Self>) {
        self.set_scroll(pane, top, cx);
        if pane != Pane::Unified {
            self.follow(pane.side(), cx);
        }
        cx.notify();
    }

    /// Brings the left pane in line with the right one, as synchronization turns on.
    fn resync(&mut self, cx: &App) {
        if self.unified() {
            return;
        }
        self.follow(Side::B, cx);
    }

    /// The other side-by-side pane follows pane `side`.
    fn follow(&mut self, side: Side, cx: &App) {
        if self.options.align_changes {
            if self.sync.sync() {
                let top = self.panes[Pane::of(side).index()].scroll_top;
                self.set_scroll(Pane::of(side.other()), top, cx);
            }
            return;
        }
        let line_height = f64::from(line_height(cx));
        let now = self.now_ms();
        let (a, b) = (self.pane_scroll(Pane::Left, cx), self.pane_scroll(Pane::Right, cx));
        let follow = {
            let ga = geometry(&self.docs, &self.layout, Side::A, line_height);
            let gb = geometry(&self.docs, &self.layout, Side::B, line_height);
            let chunks = &self.diff.chunks;
            self.sync.scrolled(side, &a, &b, |y, from| band_between(chunks, &ga, &gb, y, from), now)
        };
        let Some(follow) = follow else { return };
        if let Some(top) = follow.top {
            self.set_scroll(Pane::of(follow.side), Pixels::from(top), cx);
        }
        // The follower's own scroll, which the model tells from the user's.
        let (a, b) = (self.pane_scroll(Pane::Left, cx), self.pane_scroll(Pane::Right, cx));
        let ga = geometry(&self.docs, &self.layout, Side::A, line_height);
        let gb = geometry(&self.docs, &self.layout, Side::B, line_height);
        let chunks = &self.diff.chunks;
        self.sync.scrolled(follow.side, &a, &b, |y, from| band_between(chunks, &ga, &gb, y, from), now);
    }

    // ---------------------------------------------------------------------------------------------
    // Keyboard, pointer, clipboard

    /// F7 / Shift+F7 move between differences, ⌘C copies, ⌘A selects the pane's text, the arrows,
    /// Page Up / Down, Home and End scroll the focused pane.
    fn key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let modifiers = &keystroke.modifiers;
        let primary = if cfg!(target_os = "macos") { modifiers.platform } else { modifiers.control };
        let plain = !modifiers.control && !modifiers.platform && !modifiers.alt;
        let pane = self.focused_pane();
        let line_height = line_height(cx);
        match keystroke.key.as_str() {
            "f7" if plain && modifiers.shift => self.step(Direction::Previous, cx),
            "f7" if plain => self.step(Direction::Next, cx),
            "c" if primary && !modifiers.alt && !modifiers.shift => return self.copy(cx),
            "a" if primary && !modifiers.alt && !modifiers.shift => self.select_all(cx),
            "up" if plain && !modifiers.shift => self.scroll_by(pane, -line_height, cx),
            "down" if plain && !modifiers.shift => self.scroll_by(pane, line_height, cx),
            "pageup" if plain => self.scroll_by(pane, -self.height(pane), cx),
            "pagedown" if plain => self.scroll_by(pane, self.height(pane), cx),
            "home" if plain || primary => self.scrolled(pane, px(0.0), cx),
            "end" if plain || primary => {
                let bottom = self.max_scroll(pane, cx);
                self.scrolled(pane, bottom, cx);
            }
            _ => return false,
        }
        true
    }

    fn focused_pane(&self) -> Pane {
        if self.unified() { Pane::Unified } else { Pane::of(self.focused) }
    }

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

    fn mouse_down(&mut self, pane: Pane, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        self.sync.interrupt();
        let Some((row, col)) = self.position_at(pane, event.position) else { return };
        self.pending_file = None;
        match self.layout.rows[pane.index()].get(row).copied() {
            Some(Row::Line { side, line, .. }) => {
                let doc = &self.docs[ix(side)];
                let at = doc.line(line + 1);
                let pos = at.from + (col as usize).min(at.to - at.from);
                if pane == Pane::Unified {
                    let chunk = self.chunk_of_line[ix(side)].get(line).copied().flatten();
                    self.current = chunk;
                    self.caret = match (side, chunk) {
                        (Side::A, Some(index)) => self.diff.chunks.get(index).map(|c| (Side::B, c.from_b)),
                        (Side::A, None) => self.caret,
                        (Side::B, _) => Some((Side::B, pos)),
                    };
                } else {
                    self.focused = side;
                    self.current = chunk_at(&self.diff.chunks, side, at.from, doc.len());
                    self.caret = Some((side, pos));
                }
            }
            // The placeholder expands on its own click.
            Some(Row::Collapsed { .. }) => return cx.notify(),
            Some(Row::Filler) | None => {}
        }
        let anchor = match self.selection {
            Some(selection) if event.modifiers.shift && selection.pane == pane => selection.anchor,
            _ => (row, col),
        };
        self.selection = Some(TextSelection { pane, anchor, head: (row, col) });
        self.dragging = true;
        cx.notify();
    }

    fn mouse_move(&mut self, pane: Pane, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button == Some(MouseButton::Left) && self.dragging {
            let Some(head) = self.position_at(pane, event.position) else { return };
            if let Some(selection) = self.selection.as_mut().filter(|selection| selection.pane == pane)
                && selection.head != head
            {
                selection.head = head;
                cx.notify();
            }
            return;
        }
        self.dragging = false;
        let side = pane.side();
        if pane == Pane::Unified || !self.writable(side) {
            if self.hover.pointer_left() {
                self.hover_side = None;
                cx.notify();
            }
            return;
        }
        let Some((row, _)) = self.position_at(pane, event.position) else { return };
        let line_from = match self.layout.rows[pane.index()].get(row) {
            Some(Row::Line { side, line, .. }) => self.docs[ix(*side)].line(line + 1).from,
            _ => usize::MAX,
        };
        let doc_len = self.docs[ix(side)].len();
        let changed = if line_from == usize::MAX {
            self.hover.pointer_left()
        } else {
            self.hover.pointer_moved(&self.diff.chunks, side, line_from, doc_len)
        };
        if changed {
            self.hover_side = self.hover.index().map(|_| side);
            cx.notify();
        }
    }

    fn context_menu(&mut self, pane: Pane, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let at = self.position_at(pane, event.position);
        let (side, line) = match at.and_then(|(row, _)| self.layout.rows[pane.index()].get(row).copied()) {
            Some(Row::Line { side, line, .. }) => (side, Some(line as u32)),
            _ => (pane.side(), None),
        };
        cx.emit(DiffViewEvent::ShowMenu { side, line, position: event.position });
    }

    /// The text of a row: a line, or the lines a placeholder hides.
    fn row_text(&self, row: &Row) -> String {
        match *row {
            Row::Line { side, line, .. } => self.line_text(side, line),
            Row::Collapsed { side, from, to, .. } => {
                (from..to).map(|line| self.line_text(side, line)).collect::<Vec<_>>().join("\n")
            }
            Row::Filler => String::new(),
        }
    }

    /// The selected text, its lines joined by `\n`.
    pub fn selected_text(&self) -> Option<String> {
        let selection = self.selection?;
        let (start, end) = selection.ordered();
        if start == end {
            return None;
        }
        let rows = &self.layout.rows[selection.pane.index()];
        let mut parts = Vec::new();
        for (index, row) in rows.iter().enumerate().take(end.0 + 1).skip(start.0) {
            if *row == Row::Filler {
                continue;
            }
            let text = self.row_text(row);
            if let Row::Collapsed { .. } = row {
                parts.push(text);
                continue;
            }
            let from = if index == start.0 { start.1 } else { 0 };
            let to = if index == end.0 { end.1 } else { utf16::len(&text) };
            parts.push(utf16::slice(&text, from, to.max(from)).to_owned());
        }
        Some(parts.join("\n"))
    }

    /// Copies the selection; whether there was one.
    pub fn copy(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(text) = self.selected_text() else { return false };
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        true
    }

    /// Selects the whole text of the focused pane.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let pane = self.focused_pane();
        let rows = &self.layout.rows[pane.index()];
        let Some(last) = rows.len().checked_sub(1) else { return };
        let end = utf16::len(&self.row_text(&rows[last]));
        self.selection = Some(TextSelection { pane, anchor: (0, 0), head: (last, end) });
        cx.notify();
    }

    // ---------------------------------------------------------------------------------------------
    // Drawing

    fn painted(&mut self, pane: Pane, painted: Painted) {
        let state = &mut self.panes[pane.index()];
        state.height = Some(painted.bounds.size.height);
        state.painted = Some(painted);
    }

    /// The rows of `pane` in view.
    fn visible_rows(&self, pane: Pane, cx: &App) -> Range<usize> {
        let line_height = line_height(cx);
        let rows = self.layout.rows[pane.index()].len();
        let first = ((self.panes[pane.index()].scroll_top / line_height).floor().max(0.0) as usize).min(rows);
        let count = (self.height(pane) / line_height).ceil().max(0.0) as usize + 1;
        first..(first + count).min(rows)
    }

    /// What `pane` paints in a pane `height` tall.
    fn snapshot(&self, pane: Pane, height: Pixels, line_height: Pixels) -> PaneSnapshot {
        let state = &self.panes[pane.index()];
        let rows = &self.layout.rows[pane.index()];
        let first = ((state.scroll_top / line_height).floor().max(0.0) as usize).min(rows.len());
        let count = (height / line_height).ceil().max(0.0) as usize + 1;
        let last = (first + count).min(rows.len());
        let unified = pane == Pane::Unified;
        let marks_gaps = !unified && !self.options.align_changes && self.options.highlight != HighlightPolicy::None;
        let gaps: GapMarks = [Side::A, Side::B].map(|side| {
            let doc = &self.docs[ix(side)];
            if !marks_gaps {
                return HashMap::new();
            }
            gap_marks(&self.diff.chunks, side, doc, 0..doc.len() + 1)
                .into_iter()
                .map(|mark| (mark.line_from, (mark.kind, mark.end)))
                .collect()
        });
        let selection = self.selection.filter(|selection| selection.pane == pane).map(|s| s.ordered());
        let paint_row = |row: usize| match rows[row] {
            Row::Line { side, line, other } => {
                Some(self.row_paint(row, (side, line, other), unified, selection, &gaps))
            }
            Row::Collapsed { .. } | Row::Filler => None,
        };
        PaneSnapshot { scroll_top: state.scroll_top, first_row: first, rows: (first..last).map(paint_row).collect() }
    }

    /// What row `row`, line `line` of `side` (matching `other`), paints.
    fn row_paint(
        &self,
        row: usize,
        (side, line, other): (Side, usize, Option<usize>),
        unified: bool,
        selection: Option<RowSpan>,
        gaps: &GapMarks,
    ) -> RowPaint {
        let i = ix(side);
        let doc = &self.docs[i];
        let at = doc.line(line + 1);
        let text = doc.line_text(at);
        let highlight = self.options.highlight;
        let change = if highlight == HighlightPolicy::None {
            None
        } else {
            self.kinds[i].get(line).copied().flatten().map(|mut changed| {
                if unified {
                    changed.kind = if side == Side::A { DiffKind::Deleted } else { DiffKind::Inserted };
                }
                changed
            })
        };
        let col = |pos: usize| (pos.clamp(at.from, at.to + 1) - at.from) as u32;
        let marks = &self.words[i];
        let start = marks.partition_point(|mark| mark.to < at.from);
        let words = marks[start..]
            .iter()
            .take_while(|mark| mark.from <= at.to)
            .filter(|mark| mark.is_empty() || mark.to > at.from)
            .map(|mark| (col(mark.from), col(mark.to), mark.kind))
            .collect();
        let spans_of = &self.highlights[i];
        let first_span = spans_of.partition_point(|span| span.to as usize <= at.from);
        let spans = spans_of[first_span..]
            .iter()
            .take_while(|span| (span.from as usize) < at.to)
            .map(|span| (col(span.from as usize), col((span.to as usize).min(at.to)), span.kind))
            .collect();
        let selected = selection.and_then(|(start, end)| {
            if row < start.0 || row > end.0 {
                return None;
            }
            let from = if row == start.0 { start.1 } else { 0 };
            let to = if row == end.0 { end.1 } else { u32::MAX };
            (to > from).then_some((from, to))
        });
        let current = self.current.is_some() && self.chunk_of_line[i].get(line).copied().flatten() == self.current;
        let gap = gaps[i].get(&at.from).copied();
        let numbers = match (unified, side) {
            (true, Side::B) => [other.map(|other| other + 1), Some(line + 1)],
            _ => [Some(line + 1), None],
        };
        RowPaint {
            row,
            text,
            spans,
            change,
            words,
            selected,
            current,
            gap_above: gap.filter(|(_, end)| !end).map(|(kind, _)| kind),
            gap_below: gap.filter(|(_, end)| *end).map(|(kind, _)| kind),
            numbers,
        }
    }

    /// The divider's bands (with their extents relative to its top) and arrows.
    fn divider_shapes(&self, line_height: f64) -> (Divider, Vec<[f64; 4]>) {
        let ga = geometry(&self.docs, &self.layout, Side::A, line_height);
        let gb = geometry(&self.docs, &self.layout, Side::B, line_height);
        let extent_a = |c: &Chunk| extent(&ga, c.from_a, c.to_a);
        let extent_b = |c: &Chunk| extent(&gb, c.from_b, c.to_b);
        let view = DividerView {
            off_a: -f64::from(self.panes[Pane::Left.index()].scroll_top),
            off_b: -f64::from(self.panes[Pane::Right.index()].scroll_top),
            width: f64::from(DIVIDER_WIDTH),
            height: f64::from(self.height(Pane::Left)),
            line_height,
        };
        let arrows = (self.editable.to_a || self.editable.to_b).then_some(self.editable);
        let drawn = divider(&self.diff.chunks, extent_a, extent_b, view, arrows);
        let extents = drawn
            .bands
            .iter()
            .map(|band| {
                let chunk = &self.diff.chunks[band.index];
                let ((a0, a1), (b0, b1)) = (extent_a(chunk), extent_b(chunk));
                [a0 + view.off_a, a1 + view.off_a, b0 + view.off_b, b1 + view.off_b]
            })
            .collect();
        (drawn, extents)
    }

    fn render_labels(&self, cx: &mut Context<Self>) -> Div {
        let summary = self.summary();
        let badges = [line_ending_badge(&self.raw[0]), line_ending_badge(&self.raw[1])];
        let eol_differs = !self.binary && (badges[0] != badges[1] || summary.eol_only);
        let badge = |text: String| {
            div()
                .flex_none()
                .px(px(5.0))
                .rounded(theme::length(cx, "radius-small"))
                .bg(theme::color(cx, "accent-soft"))
                .text_color(theme::color(cx, "accent"))
                .text_size(theme::length(cx, "fs-tiny"))
                .child(SharedString::from(text))
        };
        let label = |side: Side| {
            let i = ix(side);
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(8.0))
                .px(px(12.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .child(SharedString::from(self.labels[i].clone()))
                .children(eol_differs.then(|| badge(badges[i].clone())))
                .children(self.writable(side).then(|| badge(tx(cx, "editable"))))
        };
        let row = div()
            .debug_selector(|| "diff-labels".to_owned())
            .h(theme::length(cx, "h-row"))
            .flex_none()
            .flex()
            .border_b_1()
            .border_color(theme::color(cx, "border"))
            .text_color(theme::color(cx, "fg-secondary"))
            .text_size(theme::length(cx, "fs-small"));
        if self.unified() {
            let arrow = format!("{} → {}", self.labels[0], self.labels[1]);
            return row
                .child(div().flex_1().min_w_0().flex().items_center().px(px(12.0)).child(SharedString::from(arrow)));
        }
        row.child(label(Side::A)).child(div().w(px(1.0)).h_full().bg(theme::color(cx, "border"))).child(label(Side::B))
    }

    /// A button over a pane's row (`top`), at its right end.
    fn row_button(
        &self,
        selector: String,
        top: Pixels,
        cx: &mut Context<Self>,
        press: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Stateful<Div> {
        let line_height = line_height(cx);
        let id = selector.clone();
        div()
            .id(SharedString::from(id))
            .debug_selector(move || selector.clone())
            .absolute()
            .top(top + px(2.0))
            .right(px(17.0))
            .h(line_height - px(4.0))
            .px(px(6.0))
            .flex()
            .items_center()
            .rounded(theme::length(cx, "radius-small"))
            .border_1()
            .border_color(theme::color(cx, "border-strong"))
            .bg(theme::color(cx, "bg-elev"))
            .text_color(theme::color(cx, "fg-secondary"))
            .text_size(theme::length(cx, "fs-tiny"))
            .hover(|el| el.bg(theme::color(cx, "bg-hover")))
            .child(SharedString::from(tx(cx, REVERT_TEXT)))
            .on_click(cx.listener(move |view, _, _, cx| press(view, cx)))
    }

    fn render_pane(&self, pane: Pane, cx: &mut Context<Self>) -> Div {
        let line_height = line_height(cx);
        let scroll_top = self.panes[pane.index()].scroll_top;
        let visible = self.visible_rows(pane, cx);
        let top_of = |row: usize| line_height * row as f32 - scroll_top;
        let mut overlays: Vec<AnyElement> = Vec::new();
        let rows: Vec<(usize, Row)> = visible.clone().map(|row| (row, self.layout.rows[pane.index()][row])).collect();
        for (row, entry) in rows {
            let Row::Collapsed { gap, from, to, .. } = entry else { continue };
            let selector = format!("diff-collapsed-{}-{gap}", pane.name());
            let label = tx(cx, &format!("{} unchanged lines", to - from));
            let placeholder = div()
                .id(SharedString::from(selector.clone()))
                .debug_selector(move || selector.clone())
                .absolute()
                .top(top_of(row))
                .left(px(0.0))
                .w_full()
                .h(line_height)
                .flex()
                .items_center()
                .px(px(10.0))
                .cursor_pointer()
                .bg(theme::color(cx, "bg-elev"))
                .text_color(theme::color(cx, "fg-subtle"))
                .text_size(theme::length(cx, "fs-small"))
                .hover(|el| el.text_color(theme::color(cx, "fg")))
                .child(SharedString::from(label))
                .on_click(cx.listener(move |view, _, _, cx| view.expand(gap, cx)));
            overlays.push(placeholder.into_any_element());
        }
        if pane == Pane::Unified && self.editable.to_b {
            let starts: Vec<(usize, usize)> = self.layout.unified_chunk_rows.iter().copied().enumerate().collect();
            for (index, row) in starts.into_iter().filter(|(_, row)| visible.contains(row)) {
                let button =
                    self.row_button(format!("diff-revert-{index}"), top_of(row), cx, move |view: &mut Self, cx| {
                        view.revert_unified(index, cx)
                    });
                overlays.push(button.into_any_element());
            }
        }
        let side = pane.side();
        if let (false, Some(index), Some(hovered)) = (pane == Pane::Unified, self.hover.index(), self.hover_side)
            && hovered == side
            && self.writable(side)
            && let Some(chunk) = self.diff.chunks.get(index)
        {
            let doc = &self.docs[ix(side)];
            let line = doc.line_at(chunk.from(side).min(doc.len())).number - 1;
            let row = self.layout.index[ix(side)].get(line).copied().unwrap_or(0);
            if visible.contains(&row) {
                let button = self.row_button(
                    format!("diff-hover-revert-{index}"),
                    top_of(row),
                    cx,
                    move |view: &mut Self, cx| view.apply_change(index, side.other(), cx),
                );
                overlays.push(button.into_any_element());
            }
        }
        let selector = format!("diff-pane-{}", pane.name());
        div()
            .debug_selector(move || selector.clone())
            .relative()
            .flex_1()
            .min_w_0()
            .h_full()
            .overflow_hidden()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.mouse_down(pane, event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |view, event: &MouseDownEvent, window, cx| view.context_menu(pane, event, window, cx)),
            )
            .on_mouse_move(cx.listener(move |view, event: &MouseMoveEvent, _, cx| view.mouse_move(pane, event, cx)))
            .on_mouse_up(MouseButton::Left, cx.listener(|view, _: &MouseUpEvent, _, _| view.dragging = false))
            .child(PaneElement { view: cx.entity(), pane })
            .children(overlays)
    }

    fn render_divider(&self, cx: &mut Context<Self>) -> Div {
        let (drawn, _) = self.divider_shapes(f64::from(line_height(cx)));
        let mut column = div()
            .debug_selector(|| "diff-divider".to_owned())
            .relative()
            .flex_none()
            .w(px(DIVIDER_WIDTH))
            .h_full()
            .overflow_hidden()
            .bg(theme::color(cx, "bg-app"))
            .border_l_1()
            .border_r_1()
            .border_color(theme::color(cx, "border"))
            .child(DividerElement { view: cx.entity() });
        for arrow in drawn.arrows {
            let (index, dir) = (arrow.index, arrow.dir);
            let name = match dir {
                ArrowDir::AToB => "a-to-b",
                ArrowDir::BToA => "b-to-a",
            };
            let selector = format!("diff-arrow-{name}-{index}");
            column = column.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .absolute()
                    .top(px(arrow.top as f32))
                    .left(px(if dir == ArrowDir::AToB { 0.0 } else { 14.0 }))
                    .w(px(14.0))
                    .h(px(16.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(theme::length(cx, "radius-small"))
                    .cursor_pointer()
                    .hover(|el| el.bg(theme::color(cx, "bg-hover")))
                    .child(theme::icon(cx, dir.icon(), 12.0, theme::color(cx, "fg-muted")))
                    .on_click(cx.listener(move |view, _, _, cx| view.apply_change(index, dir.source(), cx))),
            );
        }
        column
    }
}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DiffView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .debug_selector(|| "diff-view".to_owned())
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .track_focus(&self.focus)
            .bg(theme::color(cx, "bg-app"))
            .text_size(theme::length(cx, "fs-ui"))
            .child(self.render_labels(cx));
        if self.binary {
            return root.child(
                div()
                    .debug_selector(|| "diff-binary".to_owned())
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(24.0))
                    .text_color(theme::color(cx, "fg-subtle"))
                    .child(SharedString::from(tx(cx, "Binary files cannot be shown as text"))),
            );
        }
        let body =
            div().flex_1().min_h_0().flex().font_family(code_font(cx).family).text_size(theme::length(cx, "fs-code"));
        let body = if self.unified() {
            body.child(self.render_pane(Pane::Unified, cx))
        } else {
            body.child(self.render_pane(Pane::Left, cx))
                .child(self.render_divider(cx))
                .child(self.render_pane(Pane::Right, cx))
        };
        let hint = self.nav_hint().map(|hint| {
            div()
                .debug_selector(|| "diff-nav-hint".to_owned())
                .absolute()
                .bottom(px(40.0))
                .left(px(0.0))
                .w_full()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .rounded(px(6.0))
                        .border_1()
                        .border_color(theme::color(cx, "border-tooltip"))
                        .bg(theme::color(cx, "bg-tooltip"))
                        .text_color(theme::color(cx, "fg-tooltip"))
                        .text_size(theme::length(cx, "fs-small"))
                        .child(SharedString::from(tx(cx, hint))),
                )
        });
        root.child(body).children(hint)
    }
}

/// The colours of a change, as the panes paint it.
fn change_colors(cx: &App, changed: ChangedLine) -> (Hsla, Hsla) {
    let color = theme::color(cx, changed.kind.color_token());
    if changed.fine {
        let line = color.opacity(f32::from(FINE_LINE_TINT) / 100.0);
        let gutter = theme::color(cx, "bg-app").blend(color.opacity(f32::from(FINE_GUTTER_TINT) / 100.0));
        (line, gutter)
    } else {
        (color.opacity(f32::from(LINE_TINT) / 100.0), color)
    }
}

/// Paints the rows of a pane in view.
struct PaneElement {
    view: Entity<DiffView>,
    pane: Pane,
}

impl IntoElement for PaneElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

struct PanePaint {
    line_height: Pixels,
    text_left: Pixels,
    first_row: usize,
    /// Backgrounds, below the text.
    fills: Vec<(Bounds<Pixels>, Hsla)>,
    /// Each drawn row's text, shaped, and where it goes.
    lines: Vec<ShapedRow>,
    numbers: Vec<(Point<Pixels>, ShapedLine)>,
    /// Marks over the text.
    overs: Vec<(Bounds<Pixels>, Hsla)>,
}

impl Element for PaneElement {
    type RequestLayoutState = ();
    type PrepaintState = PanePaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PanePaint {
        let font_size = theme::length(cx, "fs-code");
        let line_height = line_height(cx);
        let code_font = code_font(cx);
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

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepared: &mut PanePaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (view, pane) = (self.view.clone(), self.pane);
        window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble && bounds.contains(&event.position) {
                let delta = event.delta.pixel_delta(line_height(cx)).y;
                view.update(cx, |view, cx| view.scroll_by(pane, -delta, cx));
                window.refresh();
            }
        });
        let line_height = prepared.line_height;
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, theme::color(cx, "bg-editor")));
            for (area, color) in &prepared.fills {
                window.paint_quad(fill(*area, *color));
            }
            for (_, shaped, origin) in prepared.lines.iter().flatten() {
                let _ = shaped.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
            for (origin, number) in &prepared.numbers {
                let _ = number.paint(*origin, line_height, TextAlign::Left, None, window, cx);
            }
            for (area, color) in &prepared.overs {
                window.paint_quad(fill(*area, *color));
            }
        });
        let painted = Painted {
            bounds,
            text_left: prepared.text_left,
            line_height,
            first_row: prepared.first_row,
            lines: std::mem::take(&mut prepared.lines)
                .into_iter()
                .map(|line| line.map(|(text, shaped, _)| (text, shaped)))
                .collect(),
        };
        self.view.update(cx, |view, _| view.painted(pane, painted));
    }
}

/// Paints the divider's bands: each change of the left pane joined to the right one.
struct DividerElement {
    view: Entity<DiffView>,
}

impl IntoElement for DividerElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// Where `y` of a band edge from `from` (left) to `to` (right) is at `t` along it, as the cubic
/// curve with its control points at 30% and 70% of the width draws it.
fn curve(width: f32, from: f64, to: f64, t: f64) -> (f32, f32) {
    let u = 1.0 - t;
    let w = f64::from(width);
    let x = 3.0 * u * u * t * 0.3 * w + 3.0 * u * t * t * 0.7 * w + t * t * t * w;
    let y = u * u * u * from + 3.0 * u * u * t * from + 3.0 * u * t * t * to + t * t * t * to;
    (x as f32, y as f32)
}

/// The outline of a band between `[a0, a1]` on the left and `[b0, b1]` on the right; an empty side
/// is a 2px sliver on its boundary.
fn band_outline([a0, a1, b0, b1]: [f64; 4], width: f32, origin: Point<Pixels>) -> Vec<Point<Pixels>> {
    let sliver = |y0: f64, y1: f64| if y1 - y0 < 2.0 { (y0 - 1.0, y0 + 1.0) } else { (y0, y1) };
    let ((a0, a1), (b0, b1)) = (sliver(a0, a1), sliver(b0, b1));
    let at = |(x, y): (f32, f32)| point(origin.x + px(x), origin.y + px(y));
    let steps = (0..=BAND_STEPS).map(|step| step as f64 / BAND_STEPS as f64);
    let top = steps.clone().map(|t| at(curve(width, a0, b0, t)));
    let bottom = steps.rev().map(|t| at(curve(width, a1, b1, t)));
    top.chain(bottom).collect()
}

impl Element for DividerElement {
    type RequestLayoutState = ();
    type PrepaintState = Vec<BandShape>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        cx: &mut App,
    ) -> Vec<BandShape> {
        let line_height = f64::from(line_height(cx));
        let view = self.view.read(cx);
        if view.options.highlight == HighlightPolicy::None {
            return Vec::new();
        }
        let (drawn, extents) = view.divider_shapes(line_height);
        let width = f32::from(bounds.size.width);
        drawn
            .bands
            .iter()
            .zip(extents)
            .map(|(band, extents)| {
                let color = theme::color(cx, band.kind.color_token()).opacity(f32::from(LINE_TINT) / 100.0);
                (band_outline(extents, width, bounds.origin), color)
            })
            .collect()
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        bands: &mut Vec<BandShape>,
        window: &mut Window,
        _: &mut App,
    ) {
        window.with_content_mask(Some(gpui_kit::ContentMask { bounds }), |window| {
            for (outline, color) in bands.iter() {
                let Some((first, rest)) = outline.split_first() else { continue };
                let mut path = PathBuilder::fill();
                path.move_to(*first);
                for point in rest {
                    path.line_to(*point);
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, *color);
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::diff_options::IgnorePolicy;
    use gpui_kit::{Modifiers, ScrollDelta, TestAppContext, VisualTestContext};
    use std::cell::RefCell;
    use std::rc::Rc;

    type Events = Rc<RefCell<Vec<DiffViewEvent>>>;

    /// The viewer of `left` against `right` in a window of its own, and what it asked of the window.
    fn open<'a>(
        cx: &'a mut TestAppContext,
        left: &str,
        right: &str,
        options: DiffViewOptions,
    ) -> (Entity<DiffView>, Events, &'a mut VisualTestContext) {
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            input::init(cx);
        });
        let (left, right) = (left.to_owned(), right.to_owned());
        let (view, cx) = cx.add_window_view(|_, cx| DiffView::new(&left, &right, options, cx));
        let events: Events = Rc::default();
        let seen = Rc::clone(&events);
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, event: &DiffViewEvent, _| seen.borrow_mut().push(event.clone())).detach();
        });
        cx.run_until_parked();
        (view, events, cx)
    }

    fn split() -> DiffViewOptions {
        DiffViewOptions::default()
    }

    fn unified() -> DiffViewOptions {
        DiffViewOptions { mode: DiffMode::Unified, collapse: false, ..DiffViewOptions::default() }
    }

    fn click(cx: &mut VisualTestContext, selector: &'static str) {
        let centre = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
        cx.simulate_click(centre, Modifiers::default());
        cx.run_until_parked();
    }

    fn focus(view: &Entity<DiffView>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let focus = view.focus_handle(cx);
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
    }

    fn counter(view: &Entity<DiffView>, cx: &mut VisualTestContext) -> String {
        view.read_with(cx, |view, cx| counter_text(&view.summary(), cx))
    }

    fn kinds(lines: &[PaneLine]) -> Vec<Option<DiffKind>> {
        lines.iter().map(|line| line.kind).collect()
    }

    fn applied(events: &Events) -> Vec<(Side, TextChange)> {
        events
            .borrow()
            .iter()
            .filter_map(|event| match event {
                DiffViewEvent::Apply { target, change } => Some((*target, change.clone())),
                _ => None,
            })
            .collect()
    }

    /// `count` numbered lines.
    fn numbered(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("line {i}")).collect()
    }

    // Both panes number their lines and give each changed line the type of its change, with the changed
    // words of a modified line over it.
    #[gpui_kit::test]
    async fn draws_both_sides_with_their_change_types_and_changed_words(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "a\nb\nc", "a\nB\nc\nd", split());
        assert!(cx.debug_bounds("diff-pane-left").is_some() && cx.debug_bounds("diff-pane-right").is_some());
        assert!(cx.debug_bounds("diff-divider").is_some());
        let (left, right) = view.read_with(cx, |view, _| (view.left_lines(), view.right_lines()));
        assert_eq!(left.iter().map(|line| line.number).collect::<Vec<_>>(), [1, 2, 3]);
        assert_eq!(kinds(&left), [None, Some(DiffKind::Modified), None]);
        assert_eq!(kinds(&right), [None, Some(DiffKind::Modified), None, Some(DiffKind::Inserted)]);
        let words = view.read_with(cx, |view, _| view.inner_highlights(Side::A).to_vec());
        assert_eq!(words.iter().map(|word| (word.from, word.to)).collect::<Vec<_>>(), [(2, 3)]);
        assert_eq!(view.read_with(cx, |view, _| view.difference_count()), 2);
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › can start an embedded comparison side by
    // side without overwriting saved viewer preferences
    #[gpui_kit::test]
    async fn can_start_an_embedded_comparison_side_by_side_without_overwriting_saved_viewer_preferences(
        cx: &mut TestAppContext,
    ) {
        let (view, events, cx) = open(cx, "a\n", "b\n", split());
        view.update(cx, |view, cx| view.set_options(DiffViewOptions { ignore: IgnorePolicy::Trim, ..split() }, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("diff-pane-left").is_some() && cx.debug_bounds("diff-pane-right").is_some());
        assert!(events.borrow().is_empty(), "nothing is stored: {:?}", events.borrow());
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › shows a binary notice for disk comparisons
    // instead of identical empty text
    #[gpui_kit::test]
    async fn shows_a_binary_notice_for_disk_comparisons_instead_of_identical_empty_text(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "", "", split());
        view.update(cx, |view, cx| view.set_binary(true, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("diff-binary").is_some());
        assert!(cx.debug_bounds("diff-pane-left").is_none());
        assert_eq!(counter(&view, cx), "Binary content");
        view.update(cx, |view, cx| view.set_binary(false, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("diff-binary").is_none());
        assert_eq!(counter(&view, cx), "Contents are identical");
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › reports %s line-separator-only differences
    #[gpui_kit::test]
    async fn reports_line_separator_only_differences(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "", "", split());
        for (a, b) in [("a\r\n", "a\n"), ("a\r", "a\n"), ("a\r\nb\nc\n", "a\nb\nc\n"), ("a\r\nb\r\nc\n", "a\nb\nc\n")] {
            view.update(cx, |view, cx| view.set_texts(a, b, cx));
            cx.run_until_parked();
            assert_eq!(counter(&view, cx), "Contents differ only in line separators", "{a:?} against {b:?}");
        }
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › recognizes identical contents %j
    #[gpui_kit::test]
    async fn recognizes_identical_contents(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "x", "y", split());
        for text in ["a\n", "a\r\n", "a\r\nb\n", ""] {
            view.update(cx, |view, cx| view.set_texts(text, text, cx));
            cx.run_until_parked();
            assert_eq!(counter(&view, cx), "Contents are identical", "{text:?}");
        }
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › does not describe ignored whitespace as only
    // line separators
    #[gpui_kit::test]
    async fn does_not_describe_ignored_whitespace_as_only_line_separators(cx: &mut TestAppContext) {
        let options = DiffViewOptions { ignore: IgnorePolicy::Whitespace, ..split() };
        let (view, _, cx) = open(cx, "a \r\n", "a\n", options);
        assert_ne!(counter(&view, cx), "Contents differ only in line separators");
        assert_eq!(counter(&view, cx), "Whitespace-only changes");
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Diff toolbar › compares a normalized live CRLF document with
    // a raw CRLF revision
    #[gpui_kit::test]
    async fn compares_a_normalized_live_crlf_document_with_a_raw_crlf_revision(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "a\r\n", "a\r\n", split());
        assert_eq!(counter(&view, cx), "Contents are identical");
    }

    // src/features/vcs/__tests__/movedCode.test.tsx › moved code in the diff viewer › explains ignored whitespace
    // rather than displaying No differences
    #[gpui_kit::test]
    async fn explains_ignored_whitespace_rather_than_displaying_no_differences(cx: &mut TestAppContext) {
        let options = DiffViewOptions { ignore: IgnorePolicy::Whitespace, ..split() };
        let (view, _, cx) = open(cx, "let a = 1;\n", "  let a = 1;\n", options);
        assert_eq!(counter(&view, cx), "Whitespace-only changes");
    }

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

    // Next and Previous Difference move the caret from change to change, the counter following it, in the
    // focused pane; F7 and Shift+F7 do the same.
    #[gpui_kit::test]
    async fn moves_between_differences_with_the_toolbar_and_f7(cx: &mut TestAppContext) {
        let a = numbered(300);
        let mut b = a.clone();
        b[5] = "changed 5".into();
        b[250] = "changed 250".into();
        let options = DiffViewOptions { collapse: false, ..split() };
        let (view, _, cx) = open(cx, &a.join("\n"), &b.join("\n"), options);
        assert_eq!(view.read_with(cx, |view, _| view.current_difference()), None);
        assert_eq!(counter(&view, cx), "2 differences");
        view.update(cx, |view, cx| view.run_command(DiffCommand::NextDifference, cx));
        assert_eq!(view.read_with(cx, |view, _| view.current_difference()), Some(0));
        assert_eq!(counter(&view, cx), "1 of 2 differences");
        focus(&view, cx);
        cx.simulate_keystrokes("f7");
        assert_eq!(view.read_with(cx, |view, _| view.current_difference()), Some(1));
        // The change is scrolled into view, a third down the pane.
        assert!(view.read_with(cx, |view, _| view.scroll_top(Pane::Right)) > px(0.0));
        cx.simulate_keystrokes("shift-f7");
        assert_eq!(view.read_with(cx, |view, _| view.current_difference()), Some(0));
    }

    /// Steps `direction` with the toolbar command or the key.
    fn press(view: &Entity<DiffView>, cx: &mut VisualTestContext, direction: Direction) {
        let command = match direction {
            Direction::Next => DiffCommand::NextDifference,
            Direction::Previous => DiffCommand::PreviousDifference,
        };
        view.update(cx, |view, cx| view.run_command(command, cx));
        cx.run_until_parked();
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › moves across files after the $button boundary hint
    #[gpui_kit::test]
    async fn moves_across_files_after_the_boundary_hint(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "same\nold\n", "same\nnew\n", split());
        for (direction, hint) in [
            (Direction::Next, "Press again to go to the next file"),
            (Direction::Previous, "Press again to go to the previous file"),
        ] {
            events.borrow_mut().clear();
            view.update(cx, |view, cx| view.set_neighbours(true, true, cx));
            // The caret on the only change.
            view.update(cx, |view, cx| view.go_to(0, Side::B, cx));
            press(&view, cx, direction);
            assert_eq!(view.read_with(cx, |view, _| view.nav_hint()), Some(hint));
            assert!(cx.debug_bounds("diff-nav-hint").is_some());
            assert!(events.borrow().iter().all(|event| !matches!(event, DiffViewEvent::OpenFile(_))));
            press(&view, cx, direction);
            assert_eq!(events.borrow().last(), Some(&DiffViewEvent::OpenFile(direction)));
            assert_eq!(view.read_with(cx, |view, _| view.nav_hint()), None);
        }
    }

    // Without another file in the chain the last difference stays put and no hint shows.
    #[gpui_kit::test]
    async fn stays_at_the_last_difference_of_a_single_file(cx: &mut TestAppContext) {
        let (view, events, cx) = open(cx, "same\nold\n", "same\nnew\n", split());
        press(&view, cx, Direction::Next);
        press(&view, cx, Direction::Next);
        press(&view, cx, Direction::Next);
        assert_eq!(view.read_with(cx, |view, _| (view.current_difference(), view.nav_hint())), (Some(0), None));
        assert!(events.borrow().is_empty());
    }

    // Unchanged runs collapse behind a placeholder, keeping three lines around each change, and a click on it
    // expands the run in both panes.
    #[gpui_kit::test]
    async fn collapses_unchanged_fragments_and_expands_them_on_click(cx: &mut TestAppContext) {
        let a = numbered(40);
        let mut b = a.clone();
        b[20] = "changed".into();
        let (view, _, cx) = open(cx, &a.join("\n"), &b.join("\n"), split());
        for pane in [Pane::Left, Pane::Right] {
            assert_eq!(view.read_with(cx, |view, _| view.collapsed_ranges(pane)), [0..17, 24..40]);
        }
        assert!(cx.debug_bounds("diff-collapsed-left-0").is_some());
        click(cx, "diff-collapsed-left-0");
        for pane in [Pane::Left, Pane::Right] {
            assert_eq!(view.read_with(cx, |view, _| view.collapsed_ranges(pane)), [Range { start: 24, end: 40 }]);
        }
        view.update(cx, |view, cx| view.run_command(DiffCommand::ToggleCollapse, cx));
        assert!(view.read_with(cx, |view, _| view.collapsed_ranges(Pane::Right)).is_empty());
        assert_eq!(view.read_with(cx, |view, _| view.rows(Pane::Right).len()), 40);
    }

    // A line the diff is opened at is revealed, the unchanged lines around it expanded; the first and the last
    // difference are revealed on request.
    #[gpui_kit::test]
    async fn reveals_a_line_inside_collapsed_lines_and_the_last_difference(cx: &mut TestAppContext) {
        let a = numbered(40);
        let mut b = a.clone();
        b[20] = "changed".into();
        b[30] = "changed too".into();
        let (view, _, cx) = open(cx, &a.join("\n"), &b.join("\n"), split());
        assert_eq!(view.read_with(cx, |view, _| view.collapsed_ranges(Pane::Right)), [0..17, 34..40]);
        view.update(cx, |view, cx| view.reveal_line(Side::B, 5, cx));
        assert_eq!(view.read_with(cx, |view, _| view.collapsed_ranges(Pane::Right)), [Range { start: 34, end: 40 }]);
        view.update(cx, |view, cx| view.reveal(Reveal::Last, cx));
        assert_eq!(view.read_with(cx, |view, _| view.current_difference()), Some(1));
    }

    // Scrolling one pane carries the other along past an insertion; with Synchronize Scrolling off it stays.
    #[gpui_kit::test]
    async fn scrolls_the_panes_together_through_an_insertion(cx: &mut TestAppContext) {
        let a = numbered(300);
        let mut b = a.clone();
        for line in ["x", "y", "z"] {
            b.insert(10, line.to_owned());
        }
        let options = DiffViewOptions { collapse: false, ..split() };
        let (view, _, cx) = open(cx, &a.join("\n"), &b.join("\n"), options);
        let scroll = |cx: &mut VisualTestContext, by: f32| {
            let centre = cx.debug_bounds("diff-pane-left").expect("the left pane is drawn").center();
            cx.simulate_event(ScrollEvent {
                position: centre,
                delta: ScrollDelta::Pixels(point(px(0.0), px(-by))),
                ..Default::default()
            });
            cx.run_until_parked();
        };
        scroll(cx, 1200.0);
        let line_height = cx.update(|_, cx| line_height(cx));
        let (left, right) = view.read_with(cx, |view, _| (view.scroll_top(Pane::Left), view.scroll_top(Pane::Right)));
        assert_eq!(left, px(1200.0));
        assert!(f32::from(right - left - line_height * 3.0).abs() <= 1.0, "{left:?} {right:?}");
        view.update(cx, |view, cx| view.run_command(DiffCommand::ToggleSyncScroll, cx));
        scroll(cx, 200.0);
        let (left, right_after) =
            view.read_with(cx, |view, _| (view.scroll_top(Pane::Left), view.scroll_top(Pane::Right)));
        assert_eq!(left, px(1400.0));
        assert_eq!(right_after, right);
    }

    // The divider's arrows copy a change toward the editable side, as an edit for the window to apply.
    #[gpui_kit::test]
    async fn applies_a_change_through_the_divider_arrow(cx: &mut TestAppContext) {
        let (left, right) = ("1\nleft\n3\n", "1\nright\n3\n");
        let (view, events, cx) = open(cx, left, right, split());
        assert!(cx.debug_bounds("diff-arrow-a-to-b-0").is_none());
        view.update(cx, |view, cx| view.set_editable(ArrowSides { to_a: false, to_b: true }, cx));
        cx.run_until_parked();
        assert!(cx.debug_bounds("diff-arrow-b-to-a-0").is_none());
        click(cx, "diff-arrow-a-to-b-0");
        let edits = applied(&events);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].0, Side::B);
        assert_eq!(Doc::new(right).apply(&edits[0].1).to_string(), left);
    }

    /// Reverts the only change of `changed` against `original` in the unified viewer, by the toolbar or by
    /// the change's own button, and checks that the edit restores the original.
    fn unified_revert(cx: &mut TestAppContext, toolbar: bool) {
        let (view, events, cx) = open(cx, "", "", unified());
        view.update(cx, |view, cx| view.set_editable(ArrowSides { to_a: false, to_b: true }, cx));
        let cases = [("a\nb", "a"), ("same", "same\n"), ("same\n", "same"), ("", "added"), ("deleted", "")];
        for (original, changed) in cases {
            events.borrow_mut().clear();
            view.update(cx, |view, cx| view.set_texts(original, changed, cx));
            cx.run_until_parked();
            if toolbar {
                let buttons = view.read_with(cx, |view, cx| {
                    view.toolbar(cx).iter().filter_map(ToolbarItem::button).cloned().collect::<Vec<_>>()
                });
                let revert = buttons.into_iter().find(|button| button.id == "diff-revert");
                assert_eq!(revert.map(|button| button.enabled), Some(true));
                view.update(cx, |view, cx| view.run_command(DiffCommand::RevertChange, cx));
            } else {
                click(cx, "diff-revert-0");
            }
            let edits = applied(&events);
            assert_eq!(edits.len(), 1, "{original:?} from {changed:?}");
            assert_eq!(Doc::new(changed).apply(&edits[0].1).to_string(), original, "{original:?} from {changed:?}");
        }
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Unified Revert › toolbar restores %j from %j
    #[gpui_kit::test]
    async fn unified_revert_from_the_toolbar_restores_the_original(cx: &mut TestAppContext) {
        unified_revert(cx, true);
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › Unified Revert › chunk restores %j from %j
    #[gpui_kit::test]
    async fn unified_revert_from_the_chunk_restores_the_original(cx: &mut TestAppContext) {
        unified_revert(cx, false);
    }

    // The unified viewer shows the old lines of a change as deleted above its new lines as inserted, with both
    // line numbers for unchanged lines.
    #[gpui_kit::test]
    async fn shows_the_old_lines_above_the_new_ones_when_unified(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "a\nold\nc", "a\nnew\nc", unified());
        assert!(cx.debug_bounds("diff-pane-unified").is_some());
        assert!(cx.debug_bounds("diff-divider").is_none());
        let lines: Vec<(Side, String, Option<DiffKind>)> = view
            .read_with(cx, |view, _| view.unified_lines())
            .into_iter()
            .map(|(side, line)| (side, line.text, line.kind))
            .collect();
        assert_eq!(
            lines,
            [
                (Side::B, "a".to_owned(), None),
                (Side::A, "old".to_owned(), Some(DiffKind::Deleted)),
                (Side::B, "new".to_owned(), Some(DiffKind::Inserted)),
                (Side::B, "c".to_owned(), None),
            ]
        );
        assert_eq!(
            view.read_with(cx, |view, _| view.rows(Pane::Unified)[0]),
            Row::Line { side: Side::B, line: 0, other: Some(0) }
        );
    }

    // ⌘A selects the focused pane's text and ⌘C copies it; a drag selects part of it.
    #[gpui_kit::test]
    async fn selects_and_copies_the_text_of_a_pane(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, "one\ntwo", "one\nthree", split());
        focus(&view, cx);
        cx.simulate_keystrokes("cmd-a cmd-c");
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("one\nthree"));
        let pane = cx.debug_bounds("diff-pane-left").expect("the left pane is drawn");
        let line_height = cx.update(|_, cx| line_height(cx));
        cx.simulate_click(point(pane.right() - px(4.0), pane.top() + line_height * 1.5), Modifiers::default());
        cx.run_until_parked();
        let selection = view.read_with(cx, |view, _| view.selection());
        assert_eq!(selection.map(|selection| (selection.pane, selection.head.0)), Some((Pane::Left, 1)));
        view.update(cx, |view, _| {
            view.selection = Some(TextSelection { pane: Pane::Left, anchor: (0, 1), head: (1, 2) });
        });
        assert_eq!(view.read_with(cx, |view, _| view.selected_text()).as_deref(), Some("ne\ntw"));
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › mapLine › shifts lines past an insertion and keeps lines
    // before it
    #[gpui_kit::test]
    async fn map_line_shifts_lines_past_an_insertion_and_keeps_lines_before_it(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, MAP_FROM, MAP_TO, split());
        let map = |line| view.read_with(cx, |view, _| view.map_line(Side::A, line));
        assert_eq!([map(0), map(1), map(2)], [0, 3, 4]);
    }

    const MAP_FROM: &str = "a\nb\nc\nd\ne\nf\ng\n";
    const MAP_TO: &str = "a\nX\nY\nb\nc\ne\nF1\nF2\ng\n";

    // src/features/vcs/__tests__/diffToolbar.test.tsx › mapLine › lands on the line after a removed block
    #[gpui_kit::test]
    async fn map_line_lands_on_the_line_after_a_removed_block(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, MAP_FROM, MAP_TO, split());
        let map = |line| view.read_with(cx, |view, _| view.map_line(Side::A, line));
        assert_eq!([map(3), map(4)], [5, 5]);
    }

    // src/features/vcs/__tests__/diffToolbar.test.tsx › mapLine › keeps the offset inside a changed block and
    // shifts past it
    #[gpui_kit::test]
    async fn map_line_keeps_the_offset_inside_a_changed_block_and_shifts_past_it(cx: &mut TestAppContext) {
        let (view, _, cx) = open(cx, MAP_FROM, MAP_TO, split());
        let map = |line| view.read_with(cx, |view, _| view.map_line(Side::A, line));
        assert_eq!([map(5), map(6)], [6, 8]);
    }

    // Not from a spec: an empty side lies after the last line of a document without a final line break.
    #[test]
    fn places_an_empty_side_after_an_unterminated_last_line() {
        let (a, b) = (Doc::new("a\nb"), Doc::new("a"));
        let chunks = idea_chunks(&a, &b, IgnorePolicy::None, HighlightPolicy::Words).chunks;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunk_lines(&chunks[0], Side::A, &a), 1..2);
        assert_eq!(chunk_lines(&chunks[0], Side::B, &b), 1..1);
        assert_eq!(gap_ranges(&chunks, Side::B, &b), [0..1, 1..1]);
    }
}
