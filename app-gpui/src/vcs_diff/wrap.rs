//! Soft wraps of the diff viewer's panes: where each line goes on to a new visual line to fit the
//! width of its pane, and how many visual lines every row then takes.
//!
//! A row of a side-by-side pane is as tall as its counterpart in the other pane, so that a wrapped
//! line on one side keeps the panes level: an unchanged line and the line it matches, and with
//! aligned changes every row and the row of the same index. Scrolling, the pointer, the overlays
//! and the divider's bands measure rows by these heights ([`RowLines`]); without wrapping every row
//! takes one line, and the positions are those of rows of one line height.
//!
//! The same breaks ([`LineBreaks`]) decide the heights and where the painter cuts a shaped line
//! ([`pieces`]), so that the text drawn always fills the rows laid out for it.

use super::diff_options::Side;
use super::doc::Doc;
use super::view::{CodeStyle, Row};
use crate::editor::buffer::{byte_of_col, col_of_byte};
use crate::editor_model::editor_prefs::matches_file_patterns;
use gpui_kit::{App, IndentAdjustment, LineFragment, Pixels, ShapedLine, px};

/// Whether the editor soft-wraps the file at `path`: Soft-wrap files is on and its file patterns
/// match the file's name (every file when there are no patterns).
pub fn editor_wraps(soft_wrap: bool, files: &str, path: &str) -> bool {
    soft_wrap && matches_file_patterns(if files.is_empty() { "*" } else { files }, path)
}

/// Where every row of a pane starts, counted in visual lines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RowLines {
    /// The first visual line of each row, then the number of visual lines in all.
    starts: Vec<usize>,
}

impl RowLines {
    /// `rows` rows of one line each.
    pub fn uniform(rows: usize) -> Self {
        Self { starts: (0..=rows).collect() }
    }

    /// Rows of `counts` lines each; a row takes one line at least.
    pub fn from_counts(counts: impl IntoIterator<Item = usize>) -> Self {
        let mut starts = vec![0];
        let mut total = 0;
        for count in counts {
            total += count.max(1);
            starts.push(total);
        }
        Self { starts }
    }

    pub fn rows(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    /// The visual lines of all rows.
    pub fn total(&self) -> usize {
        self.starts.last().copied().unwrap_or(0)
    }

    /// The first visual line of `row`; the total for a row past the last.
    pub fn start(&self, row: usize) -> usize {
        self.starts.get(row).copied().unwrap_or_else(|| self.total())
    }

    /// The visual lines `row` takes; none past the last row.
    pub fn count(&self, row: usize) -> usize {
        self.start(row + 1) - self.start(row)
    }

    /// The row visual line `line` belongs to: the last row for a line past the end, 0 without rows.
    pub fn row_at(&self, line: usize) -> usize {
        let Some(last) = self.rows().checked_sub(1) else { return 0 };
        self.starts.partition_point(|start| *start <= line).saturating_sub(1).min(last)
    }
}

/// Where each line of both sides of a diff goes on to a new visual line, for one text width and
/// one code style.
#[derive(Clone, Debug, PartialEq)]
pub struct LineBreaks {
    pub width: Pixels,
    pub style: CodeStyle,
    /// By side, for each line, the byte offsets into its text at which a new visual line starts.
    breaks: [Vec<Vec<usize>>; 2],
}

impl LineBreaks {
    /// Breaks every line of `docs` to fit `width` in `style`, at word boundaries where it can, as
    /// the text system wraps text.
    pub fn measure(docs: [&Doc; 2], width: Pixels, style: &CodeStyle, cx: &App) -> Self {
        let mut wrapper = cx.text_system().line_wrapper(style.font.clone(), style.size);
        let breaks = docs.map(|doc| {
            (0..doc.lines())
                .map(|line| {
                    let text = doc.line_text(doc.line(line + 1));
                    if text.is_empty() {
                        return Vec::new();
                    }
                    let fragments = [LineFragment::text(&text)];
                    wrapper
                        .wrap_line(&fragments, width, IndentAdjustment::NoIndent)
                        .map(|boundary| boundary.ix)
                        .filter(|at| *at > 0 && *at < text.len())
                        .collect()
                })
                .collect()
        });
        Self { width, style: style.clone(), breaks }
    }

    /// Breaks given as they are, by side and line.
    pub fn from_breaks(width: Pixels, style: CodeStyle, breaks: [Vec<Vec<usize>>; 2]) -> Self {
        Self { width, style, breaks }
    }

    /// The byte offsets at which line `line` (0-based) of `side` goes on to a new visual line.
    pub fn of(&self, side: Side, line: usize) -> &[usize] {
        let side = match side {
            Side::A => 0,
            Side::B => 1,
        };
        self.breaks[side].get(line).map_or(&[], Vec::as_slice)
    }

    /// The visual lines line `line` of `side` takes.
    pub fn lines(&self, side: Side, line: usize) -> usize {
        self.of(side, line).len() + 1
    }
}

/// The visual lines every row of the left, right and unified panes takes when lines break as
/// `breaks` says, one each without breaks. A line takes as many as it wraps into, a placeholder
/// and a filler one; a side-by-side row then takes as many as its counterpart: with `aligned`
/// the other pane's row of the same index, otherwise the row of the unchanged line it matches.
/// `index` holds the row of each line of a side in its side-by-side pane.
pub fn pane_lines(
    rows: [&[Row]; 3],
    index: [&[usize]; 2],
    aligned: bool,
    breaks: Option<&LineBreaks>,
) -> [RowLines; 3] {
    let Some(breaks) = breaks else { return rows.map(|rows| RowLines::uniform(rows.len())) };
    let own = |row: &Row| match *row {
        Row::Line { side, line, .. } => breaks.lines(side, line),
        Row::Collapsed { .. } | Row::Filler => 1,
    };
    let [left, right, unified] = rows;
    let mut a: Vec<usize> = left.iter().map(own).collect();
    let mut b: Vec<usize> = right.iter().map(own).collect();
    if aligned {
        for (left, right) in a.iter_mut().zip(b.iter_mut()) {
            let tallest = (*left).max(*right);
            (*left, *right) = (tallest, tallest);
        }
    } else {
        for (row, entry) in left.iter().enumerate() {
            let Row::Line { other: Some(other), .. } = *entry else { continue };
            let Some(&matched) = index[1].get(other) else { continue };
            if matched < b.len() {
                let tallest = a[row].max(b[matched]);
                (a[row], b[matched]) = (tallest, tallest);
            }
        }
    }
    [RowLines::from_counts(a), RowLines::from_counts(b), RowLines::from_counts(unified.iter().map(own))]
}

/// `shaped` cut at `breaks` (byte offsets into its text, increasing): each visual line's first
/// byte and its glyphs, placed from x = 0.
pub fn pieces(shaped: &ShapedLine, breaks: &[usize]) -> Vec<(usize, ShapedLine)> {
    if breaks.is_empty() {
        return vec![(0, shaped.clone())];
    }
    let len = shaped.len();
    let mut cursor = shaped.cursor();
    let mut out = Vec::with_capacity(breaks.len() + 1);
    let mut start = 0;
    for &at in breaks {
        if at <= start || at >= len || !shaped.text.is_char_boundary(at) {
            continue;
        }
        out.push((start, cursor.take_until(at)));
        start = at;
    }
    out.push((start, cursor.take_until(len)));
    out
}

/// The column (UTF-16) each visual line of `text` starts at, given its `pieces`.
pub fn piece_starts(text: &str, pieces: &[(usize, ShapedLine)]) -> Vec<u32> {
    pieces.iter().map(|(start, _)| col_of_byte(text, *start)).collect()
}

/// Where column `col` of `text` lies along visual line `line` of its `pieces`.
pub fn x_at(text: &str, pieces: &[(usize, ShapedLine)], line: usize, col: u32) -> Pixels {
    let Some((start, piece)) = pieces.get(line) else { return px(0.0) };
    piece.x_for_index(byte_of_col(text, col).saturating_sub(*start))
}

/// The part of columns `from..to` of a line on each of its visual lines, which start at columns
/// `starts`: `(visual line, from, to)`, in the line's columns. An end past the text (the line
/// break) stays on the last visual line; an empty range is a point on the visual line holding it.
pub fn spans_on_lines(starts: &[u32], from: u32, to: u32) -> Vec<(usize, u32, u32)> {
    let mut out = Vec::new();
    for (line, &start) in starts.iter().enumerate() {
        let end = starts.get(line + 1).copied().unwrap_or(u32::MAX);
        if from == to {
            if from >= start && from < end {
                out.push((line, from, to));
                break;
            }
            continue;
        }
        let (a, b) = (from.max(start), to.min(end));
        if a < b {
            out.push((line, a, b));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::font;

    fn style() -> CodeStyle {
        CodeStyle { font: font("Mono"), size: px(13.0), line_height: px(20.0) }
    }

    fn line(side: Side, line: usize, other: Option<usize>) -> Row {
        Row::Line { side, line, other }
    }

    #[test]
    fn rows_of_one_line_start_at_their_index() {
        let lines = RowLines::uniform(3);
        assert_eq!((lines.rows(), lines.total()), (3, 3));
        assert_eq!([lines.start(0), lines.start(2), lines.start(3), lines.start(9)], [0, 2, 3, 3]);
        assert_eq!([lines.row_at(0), lines.row_at(2), lines.row_at(7)], [0, 2, 2]);
        assert_eq!(RowLines::uniform(0).row_at(4), 0);
    }

    #[test]
    fn a_wrapped_row_takes_its_lines() {
        let lines = RowLines::from_counts([1, 3, 0, 2]);
        assert_eq!(lines.total(), 7);
        assert_eq!([lines.start(1), lines.start(2), lines.start(3)], [1, 4, 5]);
        assert_eq!([lines.count(1), lines.count(2), lines.count(4)], [3, 1, 0]);
        let rows: Vec<usize> = [0, 1, 3, 4, 6].into_iter().map(|line| lines.row_at(line)).collect();
        assert_eq!(rows, [0, 1, 1, 2, 3]);
    }

    #[test]
    fn follows_the_editors_soft_wrap_files() {
        assert!(editor_wraps(true, "*.ts", "/p/a.ts"));
        assert!(!editor_wraps(true, "*.md; *.txt; *.rst; *.adoc", "/p/a.ts"));
        assert!(!editor_wraps(false, "*.ts", "/p/a.ts"));
        assert!(editor_wraps(true, "", "/p/a.ts"), "no patterns: every file");
    }

    // An unchanged line wrapped on one side makes the line it matches as tall; a changed line keeps its
    // own height unless changes are aligned.
    #[test]
    fn keeps_the_side_by_side_rows_level() {
        // Left: line 0 unchanged, line 1 changed; right: line 0 unchanged, lines 1 and 2 changed.
        let left = [line(Side::A, 0, Some(0)), line(Side::A, 1, None)];
        let right = [line(Side::B, 0, Some(0)), line(Side::B, 1, None), line(Side::B, 2, None)];
        let unified = [line(Side::B, 0, Some(0)), line(Side::A, 1, None), line(Side::B, 1, None)];
        let breaks =
            LineBreaks::from_breaks(px(100.0), style(), [vec![vec![4, 8], vec![]], vec![vec![], vec![3], vec![]]]);
        let rows = [&left[..], &right[..], &unified[..]];
        let index = [&[0, 1][..], &[0, 1, 2][..]];
        let [a, b, u] = pane_lines(rows, index, false, Some(&breaks));
        assert_eq!([a.count(0), a.count(1)], [3, 1]);
        assert_eq!([b.count(0), b.count(1), b.count(2)], [3, 2, 1]);
        assert_eq!([u.count(0), u.count(1), u.count(2)], [1, 1, 2]);
        // Aligned, the rows of one index share the taller height, the filler included.
        let left = [line(Side::A, 0, Some(0)), line(Side::A, 1, None), Row::Filler];
        let rows = [&left[..], &right[..], &unified[..]];
        let [a, b, _] = pane_lines(rows, index, true, Some(&breaks));
        assert_eq!((a.total(), b.total()), (6, 6));
        assert_eq!([a.count(1), b.count(1)], [2, 2]);
        // Without breaks every row takes one line.
        let [a, b, u] = pane_lines(rows, index, true, None);
        assert_eq!((a, b, u), (RowLines::uniform(3), RowLines::uniform(3), RowLines::uniform(3)));
    }

    #[test]
    fn splits_a_span_over_the_visual_lines_it_crosses() {
        let starts = [0, 5, 9];
        assert_eq!(spans_on_lines(&starts, 2, 7), [(0, 2, 5), (1, 5, 7)]);
        assert_eq!(spans_on_lines(&starts, 6, u32::MAX), [(1, 6, 9), (2, 9, u32::MAX)]);
        assert_eq!(spans_on_lines(&starts, 5, 5), [(1, 5, 5)]);
        assert_eq!(spans_on_lines(&[0], 3, 12), [(0, 3, 12)]);
        assert!(spans_on_lines(&starts, 4, 4).iter().all(|(line, ..)| *line == 0));
    }
}
