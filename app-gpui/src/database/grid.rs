//! The result / table data grid's state: rectangular cell selection (click, ⇧-click, drag, arrows,
//! select all), copy as TSV / CSV / One-Row, pasting a rectangle as edits, the inline and
//! multi-line cell editors, and how a cell's text is shown (`<null>`, line-break markers, numbers,
//! a bounded preview of large values).
//!
//! Cell values are read through a `value(row, col)` function, so the grid works the same over a
//! query result and over a table page with pending edits applied.

use super::model::{Cell, parse_delimited, to_csv, to_one_row, to_tsv};
use regex::Regex;
use std::sync::LazyLock;

/// Longest text (UTF-16 code units) a cell lays out; the value itself stays whole.
pub const CELL_PREVIEW_LIMIT: usize = 1000;

/// Text of a cell, bounded to [`CELL_PREVIEW_LIMIT`] code units plus `…`, without splitting a
/// surrogate pair.
pub fn cell_preview(value: &str) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    if units.len() <= CELL_PREVIEW_LIMIT {
        return value.to_string();
    }
    let mut end = CELL_PREVIEW_LIMIT;
    if (0xd800..=0xdbff).contains(&units[end - 1]) {
        end -= 1;
    }
    let mut out = String::from_utf16_lossy(&units[..end]);
    out.push('…');
    out
}

/// Whether the value spans several lines (a single-line field would drop its line breaks).
pub fn has_line_break(value: &str) -> bool {
    value.contains(['\r', '\n'])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridColumn {
    pub name: String,
    pub type_name: String,
    pub primary_key: bool,
    pub foreign_key: bool,
}

impl GridColumn {
    pub fn new(name: impl Into<String>, type_name: impl Into<String>) -> Self {
        Self { name: name.into(), type_name: type_name.into(), primary_key: false, foreign_key: false }
    }
}

static NUMERIC_TYPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)int|num|dec|real|float|double|money|serial|^bit$").expect("the pattern is valid")
});
static NOT_NUMERIC_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)point|interval").expect("the pattern is valid"));
static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^-?[0-9]+(\.[0-9]+)?(e[+-]?[0-9]+)?$").expect("the pattern is valid"));

/// Whether `text` is a plain decimal number (`-12`, `3.5`, `1e9`).
pub fn is_plain_number(text: &str) -> bool {
    NUMBER.is_match(text)
}

/// Numbers are right-aligned: by type, else when every sampled value is one.
pub fn is_numeric_column(type_name: &str, sample: &[Cell]) -> bool {
    let unknown =
        type_name.is_empty() || type_name.eq_ignore_ascii_case("null") || type_name.eq_ignore_ascii_case("unknown");
    if !unknown {
        return NUMERIC_TYPE.is_match(type_name) && !NOT_NUMERIC_TYPE.is_match(type_name);
    }
    let values: Vec<&str> = sample.iter().flatten().map(String::as_str).collect();
    !values.is_empty() && values.iter().all(|v| is_plain_number(v))
}

/// SQLite renders whole REALs as `3108.0`; the grid shows `3108`.
pub fn displayed(value: &str, numeric: bool) -> &str {
    let Some(whole) = value.strip_suffix(".0") else { return value };
    let digits = whole.strip_prefix('-').unwrap_or(whole);
    if numeric && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) { whole } else { value }
}

/// A piece of a cell's one-line text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellPart {
    Text(String),
    /// A line break, shown as a muted `↵`.
    LineBreak,
    /// SQL NULL, shown as a muted `<null>`.
    Null,
}

/// What a cell shows: `<null>`, or its (bounded) text on one line with each line break as a marker.
pub fn cell_parts(value: Option<&str>, numeric: bool) -> Vec<CellPart> {
    let Some(value) = value else { return vec![CellPart::Null] };
    let preview = cell_preview(displayed(value, numeric));
    let mut out = Vec::new();
    for (i, line) in split_lines(&preview).into_iter().enumerate() {
        if i > 0 {
            out.push(CellPart::LineBreak);
        }
        out.push(CellPart::Text(line.to_string()));
    }
    out
}

/// `text` split at `\r\n`, `\r` and `\n`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' => {
                out.push(&text[start..i]);
                i += if bytes.get(i + 1) == Some(&b'\n') { 2 } else { 1 };
                start = i;
            }
            b'\n' => {
                out.push(&text[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push(&text[start..]);
    out
}

/// The plain text of [`cell_parts`] (`↵` for line breaks, `<null>` for NULL).
pub fn cell_text(value: Option<&str>, numeric: bool) -> String {
    cell_parts(value, numeric)
        .into_iter()
        .map(|p| match p {
            CellPart::Text(t) => t,
            CellPart::LineBreak => "↵".to_string(),
            CellPart::Null => "<null>".to_string(),
        })
        .collect()
}

/// The tooltip of a cell: long values (over 40 code units) show their first 2000.
pub fn cell_tooltip(value: Option<&str>) -> Option<String> {
    let units: Vec<u16> = value?.encode_utf16().collect();
    (units.len() > 40).then(|| String::from_utf16_lossy(&units[..units.len().min(2000)]))
}

/// The grid's own copy formats (the extractor drop-down chooses one for plain ⌘C).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CopyFormat {
    #[default]
    Tsv,
    Csv,
    OneRow,
}

/// A selection: the anchor (`r0`, `c0`) and the lead (`r1`, `c1`), which moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sel {
    pub r0: usize,
    pub c0: usize,
    pub r1: usize,
    pub c1: usize,
}

impl Sel {
    pub fn cell(row: usize, col: usize) -> Self {
        Self { r0: row, c0: col, r1: row, c1: col }
    }
}

/// The selected rectangle, inclusive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridRange {
    pub top: usize,
    pub bottom: usize,
    pub left: usize,
    pub right: usize,
}

impl From<Sel> for GridRange {
    fn from(s: Sel) -> Self {
        Self { top: s.r0.min(s.r1), bottom: s.r0.max(s.r1), left: s.c0.min(s.c1), right: s.c0.max(s.c1) }
    }
}

/// The editor of a cell: the inline field, or the multi-line dialog (`dialog`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellEditor {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub dialog: bool,
}

/// A change the grid asks its owner to make: `value` of `row`, `col` (`None` sets NULL).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellEdit {
    pub row: usize,
    pub col: usize,
    pub value: Cell,
}

/// Row state of the table editor's grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Deleted,
    Inserted,
}

/// Arrow-key moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
}

/// The grid's interaction state over `row_count` rows of `columns`.
#[derive(Clone, Debug, Default)]
pub struct GridState {
    pub columns: Vec<GridColumn>,
    pub row_count: usize,
    pub numeric: Vec<bool>,
    pub sel: Option<Sel>,
    pub editing: Option<CellEditor>,
    pub editable: bool,
    pub copy_format: CopyFormat,
    dragging: bool,
}

impl GridState {
    /// A grid over new columns: numbers detected from the first 50 rows, selection and editor reset.
    pub fn new(columns: Vec<GridColumn>, row_count: usize, value: impl Fn(usize, usize) -> Cell) -> Self {
        let sample_rows = row_count.min(50);
        let numeric = columns
            .iter()
            .enumerate()
            .map(|(ci, c)| {
                let sample: Vec<Cell> = (0..sample_rows).map(|r| value(r, ci)).collect();
                is_numeric_column(&c.type_name, &sample)
            })
            .collect();
        Self { columns, row_count, numeric, ..Self::default() }
    }

    pub fn range(&self) -> Option<GridRange> {
        self.sel.map(GridRange::from)
    }

    pub fn is_selected(&self, r: usize, c: usize) -> bool {
        self.range().is_some_and(|n| (n.top..=n.bottom).contains(&r) && (n.left..=n.right).contains(&c))
    }

    pub fn selected_rows(&self) -> Vec<usize> {
        self.range().map(|n| (n.top..=n.bottom).collect()).unwrap_or_default()
    }

    /// Left button down on a cell: starts a selection (⇧ extends it) and a drag. Any other
    /// button selects the cell unless it is already selected. Returns the edit of an open
    /// inline editor, which the click commits.
    pub fn mouse_down(
        &mut self,
        r: usize,
        c: usize,
        left: bool,
        shift: bool,
        value: impl Fn(usize, usize) -> Cell,
    ) -> Option<CellEdit> {
        if !left {
            if !self.is_selected(r, c) {
                self.sel = Some(Sel::cell(r, c));
            }
            return None;
        }
        let committed = if self.editing.is_some() { self.commit(value) } else { None };
        self.dragging = true;
        self.sel = Some(match self.sel {
            Some(s) if shift => Sel { r1: r, c1: c, ..s },
            _ => Sel::cell(r, c),
        });
        committed
    }

    /// The pointer entered a cell: a drag extends the selection to it.
    pub fn mouse_enter(&mut self, r: usize, c: usize) {
        if self.dragging
            && let Some(s) = self.sel.as_mut()
        {
            s.r1 = r;
            s.c1 = c;
        }
    }

    pub fn mouse_up(&mut self) {
        self.dragging = false;
    }

    /// Row number clicked: selects the whole row (⇧ extends).
    pub fn row_number_down(&mut self, r: usize, left: bool, shift: bool) {
        if !left && self.sel.is_some() && self.is_selected(r, 0) {
            return;
        }
        let last = self.columns.len().saturating_sub(1);
        self.sel = Some(match self.sel {
            Some(s) if shift => Sel { r1: r, c1: last, ..s },
            _ => Sel { r0: r, c0: 0, r1: r, c1: last },
        });
    }

    /// ⌘A.
    pub fn select_all(&mut self) {
        if self.row_count > 0 && !self.columns.is_empty() {
            self.sel = Some(Sel { r0: 0, c0: 0, r1: self.row_count - 1, c1: self.columns.len() - 1 });
        }
    }

    /// An arrow key: moves the lead (⇧ extends the selection). Returns the cell to scroll to.
    pub fn move_lead(&mut self, m: Move, shift: bool) -> Option<(usize, usize)> {
        let s = self.sel?;
        let (dr, dc): (isize, isize) = match m {
            Move::Up => (-1, 0),
            Move::Down => (1, 0),
            Move::Left => (0, -1),
            Move::Right => (0, 1),
        };
        let clamp = |v: usize, d: isize, len: usize| v.saturating_add_signed(d).min(len.saturating_sub(1));
        let r = clamp(s.r1, dr, self.row_count);
        let c = clamp(s.c1, dc, self.columns.len());
        self.sel = Some(if shift { Sel { r1: r, c1: c, ..s } } else { Sel::cell(r, c) });
        Some((r, c))
    }

    /// The text copying the selection puts on the clipboard (`None` without a selection):
    /// `header` adds the column names, `whole_rows` copies every column of the selected rows.
    pub fn copy(
        &self,
        format: CopyFormat,
        header: bool,
        whole_rows: bool,
        value: impl Fn(usize, usize) -> Cell,
    ) -> Option<String> {
        let n = self.range()?;
        let cols: Vec<usize> =
            if whole_rows { (0..self.columns.len()).collect() } else { (n.left..=n.right).collect() };
        let rows: Vec<Vec<Cell>> =
            self.selected_rows().iter().map(|&r| cols.iter().map(|&c| value(r, c)).collect()).collect();
        let head: Option<Vec<String>> = header.then(|| cols.iter().map(|&c| self.columns[c].name.clone()).collect());
        Some(match format {
            CopyFormat::OneRow => {
                to_one_row(&rows, |c| cols.get(c).is_some_and(|&col| self.numeric.get(col) == Some(&true)))
            }
            CopyFormat::Tsv => to_tsv(head.as_deref(), &rows),
            CopyFormat::Csv => to_csv(head.as_deref(), &rows),
        })
    }

    /// Pasting tab-separated `text` at the selection's top-left corner: the edits to make (rows
    /// past the end, columns past the last and deleted rows are skipped); the selection grows to
    /// cover the pasted cells.
    pub fn paste(&mut self, text: &str, row_state: impl Fn(usize) -> Option<RowState>) -> Vec<CellEdit> {
        if self.editing.is_some() || !self.editable || text.is_empty() {
            return Vec::new();
        }
        let Some(n) = self.range() else { return Vec::new() };
        let (mut bottom, mut right) = (n.top, n.left);
        let mut edits = Vec::new();
        for (dr, cells) in parse_delimited(text, '\t').into_iter().enumerate() {
            for (dc, cell) in cells.into_iter().enumerate() {
                let (row, col) = (n.top + dr, n.left + dc);
                if row >= self.row_count || col >= self.columns.len() || row_state(row) == Some(RowState::Deleted) {
                    continue;
                }
                edits.push(CellEdit { row, col, value: Some(cell) });
                bottom = bottom.max(row);
                right = right.max(col);
            }
        }
        self.sel = Some(Sel { r0: n.top, c0: n.left, r1: bottom, c1: right });
        edits
    }

    /// Set NULL on every selected cell.
    pub fn set_null(&self) -> Vec<CellEdit> {
        let Some(n) = self.range().filter(|_| self.editable) else { return Vec::new() };
        (n.top..=n.bottom)
            .flat_map(|row| (n.left..=n.right).map(move |col| CellEdit { row, col, value: None }))
            .collect()
    }

    /// Opens the editor of a cell (double-click, Enter, F2): the multi-line one when `maximized`
    /// (⇧Enter) or the value is long or spans lines. Deleted rows are not edited.
    pub fn start_edit(
        &mut self,
        row: usize,
        col: usize,
        maximized: bool,
        value: Option<&str>,
        state: Option<RowState>,
    ) {
        if !self.editable || state == Some(RowState::Deleted) {
            return;
        }
        let text = value.unwrap_or("").to_string();
        let dialog = maximized || text.encode_utf16().count() > CELL_PREVIEW_LIMIT || has_line_break(&text);
        self.editing = Some(CellEditor { row, col, text, dialog });
    }

    /// Enter / F2 on the grid: edits the lead cell (⇧Enter in the multi-line editor).
    pub fn edit_lead(
        &mut self,
        maximized: bool,
        value: impl Fn(usize, usize) -> Cell,
        state: impl Fn(usize) -> Option<RowState>,
    ) {
        if let Some(s) = self.sel {
            let v = value(s.r1, s.c1);
            self.start_edit(s.r1, s.c1, maximized, v.as_deref(), state(s.r1));
        }
    }

    /// The editor's text changed.
    pub fn set_editor_text(&mut self, text: &str) {
        if let Some(e) = self.editing.as_mut() {
            e.text = text.to_string();
        }
    }

    /// ⇧Enter in the inline editor: continues in the multi-line editor with the text typed so far
    /// (the inline field's blur then commits nothing).
    pub fn maximize(&mut self) {
        if let Some(e) = self.editing.as_mut() {
            e.dialog = true;
        }
    }

    /// Escape / Cancel.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
    }

    /// Enter / Apply / blur: closes the editor; the edit to make when the value changed. A value
    /// that used CRLF throughout keeps CRLF line breaks (the multi-line editor types LF), and
    /// leaving a NULL cell empty keeps it NULL.
    pub fn commit(&mut self, value: impl Fn(usize, usize) -> Cell) -> Option<CellEdit> {
        let editing = self.editing.take()?;
        let prev = value(editing.row, editing.col);
        let crlf = prev.as_deref().is_some_and(|p| p.contains("\r\n") && !has_bare_lf(p));
        let text = if crlf { editing.text.replace("\r\n", "\n").replace('\n', "\r\n") } else { editing.text };
        let changed = prev.as_deref() != Some(text.as_str());
        (changed && !(prev.is_none() && text.is_empty())).then_some(CellEdit {
            row: editing.row,
            col: editing.col,
            value: Some(text),
        })
    }
}

/// Whether `text` has a `\n` not preceded by `\r`.
fn has_bare_lf(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().any(|(i, &b)| b == b'\n' && (i == 0 || bytes[i - 1] != b'\r'))
}

#[cfg(test)]
mod tests {
    use super::super::export::{ExportColumn, ExportData, ExportFormat, ExportOptions, export_data};
    use super::*;
    use ide_db::DbDriver;

    fn table<'a>(data: &'a [&'a [Option<&'a str>]]) -> impl Fn(usize, usize) -> Cell + 'a {
        |r, c| data.get(r).and_then(|row| row.get(c)).copied().flatten().map(str::to_string)
    }

    fn drag(g: &mut GridState, from: (usize, usize), to: (usize, usize)) {
        g.mouse_down(from.0, from.1, true, false, |_, _| None);
        g.mouse_enter(to.0, to.1);
        g.mouse_up();
    }

    fn text_grid() -> Vec<GridColumn> {
        vec![GridColumn::new("a", "text"), GridColumn::new("b", "text")]
    }

    #[test]
    fn drags_a_rectangle_and_copies_it_as_tsv_through_the_windows_dispatcher() {
        let data: &[&[Option<&str>]] = &[&[Some("one"), Some("two")], &[Some("three"), Some("four")]];
        let mut g = GridState::new(text_grid(), 2, table(data));
        drag(&mut g, (0, 0), (1, 1));
        assert_eq!(g.copy(g.copy_format, false, false, table(data)).as_deref(), Some("one\ttwo\nthree\tfour"));
    }

    #[test]
    fn copies_the_selection_in_the_extractor_s_format_for_plain_cmd_c() {
        let data: &[&[Option<&str>]] =
            &[&[Some("1"), Some("Shanghai")], &[Some("2"), Some("Bei'jing")], &[Some("3"), None]];
        let columns = vec![GridColumn::new("id", "INTEGER"), GridColumn::new("city", "TEXT")];
        let mut g = GridState { copy_format: CopyFormat::OneRow, ..GridState::new(columns, 3, table(data)) };
        // A column of three cells: the values on one line.
        drag(&mut g, (0, 0), (2, 0));
        assert_eq!(g.copy(g.copy_format, false, false, table(data)).as_deref(), Some("1, 2, 3"));
        // Text values are single-quoted with `'` doubled; nulls are NULL.
        drag(&mut g, (0, 1), (2, 1));
        assert_eq!(g.copy(g.copy_format, false, false, table(data)).as_deref(), Some("'Shanghai', 'Bei''jing', NULL"));
        // Several columns join in reading order, all on one line.
        drag(&mut g, (0, 0), (1, 1));
        assert_eq!(g.copy(g.copy_format, false, false, table(data)).as_deref(), Some("1, 'Shanghai', 2, 'Bei''jing'"));
    }

    #[test]
    fn copies_a_selection_as_csv_with_cmd_c_when_the_extractor_is_csv() {
        let data: &[&[Option<&str>]] = &[&[Some("one"), Some("two")], &[Some("three"), Some("four")]];
        let mut g = GridState { copy_format: CopyFormat::Csv, ..GridState::new(text_grid(), 2, table(data)) };
        drag(&mut g, (0, 0), (1, 1));
        assert_eq!(g.copy(g.copy_format, false, false, table(data)).as_deref(), Some("one,two\r\nthree,four"));
    }

    #[test]
    fn pastes_a_rectangle_as_pending_edits_and_skips_deleted_rows() {
        let mut g = GridState { editable: true, ..GridState::new(text_grid(), 2, |_, _| Some("old".into())) };
        g.mouse_down(0, 0, true, false, |_, _| None);
        let edits = g.paste("new\tother\nignored\tignored", |r| (r == 1).then_some(RowState::Deleted));
        assert_eq!(
            edits,
            [
                CellEdit { row: 0, col: 0, value: Some("new".into()) },
                CellEdit { row: 0, col: 1, value: Some("other".into()) },
            ]
        );
    }

    fn full() -> String {
        let text = "\u{8bf4}\u{660e} text \u{6df7}\u{5408} ".repeat(200_000);
        serde_json::json!({ "text": text, "tail": "complete" }).to_string()
    }

    fn prefix16(text: &str, n: usize) -> String {
        let units: Vec<u16> = text.encode_utf16().take(n).collect();
        String::from_utf16_lossy(&units)
    }

    #[test]
    fn preserves_short_values_and_truncates_only_beyond_the_preview_limit() {
        for value in [String::new(), "hello".to_string(), "\u{4e2d}".repeat(CELL_PREVIEW_LIMIT)] {
            assert_eq!(cell_preview(&value), value);
        }
        let full = full();
        assert_eq!(cell_preview(&full), format!("{}…", prefix16(&full, CELL_PREVIEW_LIMIT)));
        assert_eq!(cell_preview(&format!("{}😀tail", "a".repeat(999))), format!("{}…", "a".repeat(999)));
    }

    #[test]
    fn bounds_rendered_text_while_copying_and_exporting_the_complete_value() {
        let full = full();
        let value = |_: usize, _: usize| Some(full.clone());
        let mut g = GridState::new(vec![GridColumn::new("payload", "jsonb")], 1, value);
        assert_eq!(cell_text(Some(&full), g.numeric[0]), cell_preview(&full));
        assert_eq!(cell_tooltip(Some(&full)), Some(prefix16(&full, 2000)));
        g.mouse_down(0, 0, true, false, value);
        assert_eq!(g.copy(g.copy_format, false, false, value), Some(full.clone()));
        let d = ExportData {
            columns: vec![ExportColumn { name: "payload".into(), type_name: "jsonb".into() }],
            rows: vec![vec![Some(full.clone())]],
            table: "big".into(),
            driver: DbDriver::Postgres,
        };
        let out: serde_json::Value =
            serde_json::from_str(&export_data(ExportFormat::Json, &d, ExportOptions::default())).expect("JSON");
        assert_eq!(out, serde_json::json!([{ "payload": full }]));
    }

    #[test]
    fn edits_the_full_large_value_in_a_dialog_and_supports_cancel_and_apply() {
        let full = full();
        let value = |_: usize, _: usize| Some(full.clone());
        let mut g = GridState { editable: true, ..GridState::new(vec![GridColumn::new("payload", "jsonb")], 1, value) };
        g.start_edit(0, 0, false, Some(&full), None);
        let editor = g.editing.clone().expect("an editor");
        assert!(editor.dialog);
        assert_eq!(editor.text, full);
        g.set_editor_text(&format!("{full}changed"));
        g.cancel_edit();
        assert_eq!(g.editing, None);
        g.start_edit(0, 0, false, Some(&full), None);
        g.set_editor_text(&format!("{full}changed"));
        assert_eq!(g.commit(value), Some(CellEdit { row: 0, col: 0, value: Some(format!("{full}changed")) }));
        assert_eq!(g.editing, None);
    }

    #[test]
    fn keeps_short_values_in_the_inline_editor_and_preserves_null_display() {
        let value = |_: usize, _: usize| Some("short".to_string());
        let mut g = GridState { editable: true, ..GridState::new(vec![GridColumn::new("payload", "jsonb")], 1, value) };
        g.start_edit(0, 0, false, Some("short"), None);
        assert_eq!(g.editing.as_ref().map(|e| e.dialog), Some(false));
        g.set_editor_text("updated");
        assert_eq!(g.commit(value), Some(CellEdit { row: 0, col: 0, value: Some("updated".into()) }));
        assert_eq!(cell_text(None, false), "<null>");
    }

    #[test]
    fn shows_line_breaks_as_markers_on_one_line() {
        assert_eq!(cell_text(Some("first line\nsecond line\r\nthird"), false), "first line↵second line↵third");
    }

    #[test]
    fn edits_a_multi_line_value_in_the_multi_line_editor_and_keeps_its_line_breaks() {
        for (value, saved) in [("a\nb", "a\nb\nc"), ("a\r\nb", "a\r\nb\r\nc")] {
            let read = |_: usize, _: usize| Some(value.to_string());
            let mut g = GridState { editable: true, ..GridState::new(vec![GridColumn::new("note", "text")], 1, read) };
            g.start_edit(0, 0, false, Some(value), None);
            assert_eq!(g.editing.as_ref().map(|e| e.dialog), Some(true));
            g.set_editor_text("a\nb\nc");
            assert_eq!(g.commit(read), Some(CellEdit { row: 0, col: 0, value: Some(saved.into()) }));
        }
    }

    #[test]
    fn opens_the_multi_line_editor_with_shift_enter_from_the_grid_and_from_the_inline_editor() {
        let read = |_: usize, _: usize| Some("one".to_string());
        let mut g = GridState { editable: true, ..GridState::new(vec![GridColumn::new("note", "text")], 1, read) };
        g.mouse_down(0, 0, true, false, read);
        g.mouse_up();
        g.edit_lead(true, read, |_| None);
        assert_eq!(g.editing.as_ref().map(|e| e.dialog), Some(true));
        g.cancel_edit();

        g.start_edit(0, 0, false, Some("one"), None);
        assert_eq!(g.editing.as_ref().map(|e| e.dialog), Some(false));
        g.set_editor_text("one two");
        g.maximize();
        let editor = g.editing.clone().expect("the multi-line editor");
        assert!(editor.dialog);
        assert_eq!(editor.text, "one two");
    }
}
