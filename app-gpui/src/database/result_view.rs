//! The result views' data shaping: the local filter and client-side sort of loaded rows, search in
//! the grid (Match Case / Words / Regex, n/m, Filter rows), paging of console results, the
//! Aggregates of a selection, and the series and ticks of the chart.

use super::grid::is_plain_number;
use super::model::{Cell, js_number, js_number_string};
use crate::platform::collate;
use crate::platform::i18n::I18n;
use regex::{Regex, RegexBuilder};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashSet};

/// Column index → the values still shown (the local filter: checked values, in first-seen order).
pub type LocalFilter = BTreeMap<usize, Vec<Cell>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalSort {
    pub column: usize,
    pub descending: bool,
}

fn number_of(text: &str) -> f64 {
    js_number(text).unwrap_or(f64::NAN)
}

/// Numbers by value, then text (by UTF-16 code units); NULL first (as most databases sort ascending).
pub fn compare_cells(a: &Cell, b: &Cell) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
        (Some(a), Some(b)) if a == b => Ordering::Equal,
        (Some(a), Some(b)) if is_plain_number(a) && is_plain_number(b) => {
            number_of(a).partial_cmp(&number_of(b)).unwrap_or(Ordering::Equal)
        }
        (Some(a), Some(b)) => match collate::compare_code_units(a, b) {
            Ordering::Equal => Ordering::Greater,
            o => o,
        },
    }
}

/// The loaded rows to show, in order: those passing every column filter, sorted by `sort` (stable,
/// original order otherwise). Rows from `fixed` on (the table editor's added rows) always stay, last.
pub fn visible_rows(
    row_count: usize,
    value: impl Fn(usize, usize) -> Cell,
    filter: &LocalFilter,
    sort: Option<LocalSort>,
    fixed: Option<usize>,
) -> Vec<usize> {
    let fixed = fixed.unwrap_or(row_count).min(row_count);
    let allowed: Vec<(usize, HashSet<&Cell>)> =
        filter.iter().map(|(&c, values)| (c, values.iter().collect())).collect();
    let mut rows: Vec<usize> =
        (0..fixed).filter(|&r| allowed.iter().all(|(col, values)| values.contains(&value(r, *col)))).collect();
    if let Some(sort) = sort {
        rows.sort_by(|&a, &b| {
            let o = compare_cells(&value(a, sort.column), &value(b, sort.column));
            let o = if sort.descending { o.reverse() } else { o };
            o.then(a.cmp(&b))
        });
    }
    rows.extend(fixed..row_count);
    rows
}

/// A distinct value of a column with how often it occurs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DistinctValue {
    pub value: Cell,
    pub count: usize,
}

/// Distinct values of a column with their counts, sorted (the filter popup's list).
pub fn distinct_values(
    row_count: usize,
    value: impl Fn(usize, usize) -> Cell,
    col: usize,
    limit: usize,
) -> Vec<DistinctValue> {
    let mut counts: Vec<DistinctValue> = Vec::new();
    let mut index: std::collections::HashMap<Cell, usize> = std::collections::HashMap::new();
    for r in 0..row_count {
        let v = value(r, col);
        match index.get(&v) {
            Some(&i) => counts[i].count += 1,
            None => {
                index.insert(v.clone(), counts.len());
                counts.push(DistinctValue { value: v, count: 1 });
            }
        }
    }
    counts.sort_by(|a, b| compare_cells(&a.value, &b.value));
    counts.truncate(limit);
    counts
}

/// The filter popup's default list size.
pub const DISTINCT_LIMIT: usize = 1000;

/// The new filter after (un)checking `v` of column `col` (all checked again: no filter).
pub fn toggle_filter_value(filter: &LocalFilter, col: usize, all: &[Cell], v: &Cell, checked: bool) -> LocalFilter {
    let mut next = filter.clone();
    let mut current: Vec<Cell> = filter.get(&col).cloned().unwrap_or_else(|| {
        let mut seen = Vec::new();
        for x in all {
            if !seen.contains(x) {
                seen.push(x.clone());
            }
        }
        seen
    });
    if checked {
        if !current.contains(v) {
            current.push(v.clone());
        }
    } else {
        current.retain(|x| x != v);
    }
    if all.iter().all(|x| current.contains(x)) {
        next.remove(&col);
    } else {
        next.insert(col, current);
    }
    next
}

/// The Aggregates view of the selected cells.
#[derive(Clone, Debug, PartialEq)]
pub struct Aggregates {
    pub count: usize,
    pub distinct: usize,
    pub nulls: usize,
    /// When every non-NULL value is a number; `None` otherwise.
    pub sum: Option<f64>,
    pub avg: Option<f64>,
    pub min: Cell,
    pub max: Cell,
    pub median: Option<f64>,
}

pub fn aggregates(cells: &[Cell]) -> Aggregates {
    let present: Vec<&String> = cells.iter().flatten().collect();
    let numbers: Vec<f64> = present.iter().filter(|c| is_plain_number(c)).map(|c| number_of(c)).collect();
    let numeric = !numbers.is_empty() && numbers.len() == present.len();
    let mut sorted: Vec<Cell> = present.iter().map(|c| Some((*c).clone())).collect();
    sorted.sort_by(compare_cells);
    let median = numeric.then(|| {
        let mut n = numbers.clone();
        n.sort_by(f64::total_cmp);
        let mid = n.len() / 2;
        if n.len().is_multiple_of(2) { (n[mid - 1] + n[mid]) / 2.0 } else { n[mid] }
    });
    let sum = numeric.then(|| numbers.iter().sum::<f64>());
    let distinct = cells.iter().collect::<HashSet<_>>().len();
    Aggregates {
        count: cells.len(),
        distinct,
        nulls: cells.len() - present.len(),
        sum,
        avg: sum.map(|s| s / numbers.len() as f64),
        min: sorted.first().cloned().flatten(),
        max: sorted.last().cloned().flatten(),
        median,
    }
}

/// `n` rounded to 12 significant digits (JavaScript's `Number(n.toPrecision(12))`).
fn precision12(n: f64) -> f64 {
    if !n.is_finite() || n == 0.0 {
        return n;
    }
    format!("{n:.11e}").parse().unwrap_or(n)
}

/// A number for display: integers as is, others to 12 significant digits.
pub fn format_number(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 {
        return js_number_string(n);
    }
    js_number_string(precision12(n))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartAggregate {
    Sum,
    Avg,
    Count,
    Min,
    Max,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChartPoint {
    pub label: String,
    pub value: f64,
}

/// The chart's series: `y` per distinct `x` (in first-seen order), combined with `agg`; `None`
/// plots every row as is. Non-numeric `y` values are skipped (except for `Count`).
pub fn chart_series(rows: &[Vec<Cell>], x: usize, y: usize, agg: ChartAggregate) -> Vec<ChartPoint> {
    let cell = |r: &Vec<Cell>, c: usize| r.get(c).cloned().flatten();
    let label_of = |r: &Vec<Cell>| cell(r, x).unwrap_or_else(|| "<null>".to_string());
    if agg == ChartAggregate::None {
        return rows
            .iter()
            .filter_map(|r| {
                let v = cell(r, y).filter(|v| is_plain_number(v))?;
                Some(ChartPoint { label: label_of(r), value: number_of(&v) })
            })
            .collect();
    }
    let mut groups: Vec<(String, Vec<f64>)> = Vec::new();
    for r in rows {
        let label = label_of(r);
        let i = match groups.iter().position(|(l, _)| *l == label) {
            Some(i) => i,
            None => {
                groups.push((label, Vec::new()));
                groups.len() - 1
            }
        };
        if agg == ChartAggregate::Count {
            groups[i].1.push(1.0);
        } else if let Some(v) = cell(r, y).filter(|v| is_plain_number(v)) {
            groups[i].1.push(number_of(&v));
        }
    }
    let combine = |v: &[f64]| -> f64 {
        if v.is_empty() {
            return 0.0;
        }
        match agg {
            ChartAggregate::Sum | ChartAggregate::Count | ChartAggregate::None => v.iter().sum(),
            ChartAggregate::Avg => v.iter().sum::<f64>() / v.len() as f64,
            ChartAggregate::Min => v.iter().copied().fold(f64::INFINITY, f64::min),
            ChartAggregate::Max => v.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        }
    };
    groups.into_iter().map(|(label, v)| ChartPoint { value: combine(&v), label }).collect()
}

/// Round axis ticks from 0 (or the minimum, when negative) to `max`, about `count` of them.
pub fn nice_ticks(min: f64, max: f64, count: usize) -> Vec<f64> {
    let lo = min.min(0.0);
    let hi = max.max(0.0);
    if hi == lo {
        return vec![lo];
    }
    let raw = (hi - lo) / count.max(1) as f64;
    let mag = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * mag).find(|&s| s >= raw).unwrap_or(raw);
    let mut out = Vec::new();
    let mut t = (lo / step).floor() * step;
    while t <= hi + step * 1e-9 {
        out.push(precision12(t));
        t += step;
    }
    if let Some(&last) = out.last()
        && last < hi
    {
        out.push(precision12(last + step));
    }
    out
}

/// The search options of the grid's search bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

/// A test of a cell's text against a query.
#[derive(Clone, Debug)]
pub struct CellMatcher {
    re: Regex,
    whole_word: bool,
}

fn is_word_char(c: char) -> bool {
    c == '_' || c == '$' || c.is_alphanumeric()
}

impl CellMatcher {
    pub fn test(&self, text: &str) -> bool {
        // Words: a match with no word character on either side.
        self.re.find_iter(text).any(|m| {
            if m.is_empty() {
                return !self.whole_word;
            }
            !self.whole_word
                || (!text[..m.start()].chars().next_back().is_some_and(is_word_char)
                    && !text[m.end()..].chars().next().is_some_and(is_word_char))
        })
    }
}

/// What a search query makes.
#[derive(Clone, Debug)]
pub enum Matcher {
    /// No query: nothing is searched.
    NoQuery,
    /// The query is not a valid regular expression.
    Invalid,
    Test(CellMatcher),
}

impl Matcher {
    pub fn test(&self) -> Option<&CellMatcher> {
        match self {
            Matcher::Test(m) => Some(m),
            _ => None,
        }
    }
}

/// The test of a cell's text against `query`. Regex mode takes the `regex` crate's syntax (no
/// look-around or back-references).
pub fn cell_matcher(query: &str, options: SearchOptions) -> Matcher {
    if query.is_empty() {
        return Matcher::NoQuery;
    }
    let source = if options.regex { query.to_string() } else { regex::escape(query) };
    match RegexBuilder::new(&source).case_insensitive(!options.case_sensitive).build() {
        Ok(re) => Matcher::Test(CellMatcher { re, whole_word: options.whole_word }),
        Err(_) => Matcher::Invalid,
    }
}

/// A cell position: `row` is the position in the display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPos {
    pub row: usize,
    pub col: usize,
}

/// The cells matching `test`, row by row; `row` is the position in `rows` (display order). NULL
/// never matches.
pub fn find_cells(
    rows: &[usize],
    col_count: usize,
    value: impl Fn(usize, usize) -> Cell,
    test: &CellMatcher,
) -> Vec<CellPos> {
    let mut out = Vec::new();
    for (row, &r) in rows.iter().enumerate() {
        for col in 0..col_count {
            if value(r, col).is_some_and(|v| test.test(&v)) {
                out.push(CellPos { row, col });
            }
        }
    }
    out
}

/// Whether any cell of loaded row `r` matches `test`.
pub fn row_matches(r: usize, col_count: usize, value: impl Fn(usize, usize) -> Cell, test: &CellMatcher) -> bool {
    (0..col_count).any(|col| value(r, col).is_some_and(|v| test.test(&v)))
}

/// The search bar over a data view: it covers the loaded rows only (the page shown).
#[derive(Clone, Debug, Default)]
pub struct ResultSearch {
    pub open: bool,
    pub query: String,
    pub options: SearchOptions,
    /// Show only the rows with a match (Filter rows).
    pub filter_rows: bool,
    /// Index of the current match.
    pub current: usize,
}

impl ResultSearch {
    /// The query, an option or Filter rows changed: the first match is current again.
    pub fn set_query(&mut self, query: &str) {
        self.query = query.to_string();
        self.current = 0;
    }

    pub fn set_options(&mut self, options: SearchOptions) {
        self.options = options;
        self.current = 0;
    }

    pub fn set_filter_rows(&mut self, on: bool) {
        self.filter_rows = on;
        self.current = 0;
    }

    pub fn matcher(&self) -> Matcher {
        if self.open { cell_matcher(&self.query, self.options) } else { Matcher::NoQuery }
    }

    /// Display row → loaded row after Filter rows (`shaped`: the order of the local filter and
    /// sort, `None` for the identity); `None` keeps the identity.
    pub fn order(
        &self,
        shaped: Option<&[usize]>,
        row_count: usize,
        col_count: usize,
        value: impl Fn(usize, usize) -> Cell,
    ) -> Option<Vec<usize>> {
        let matcher = self.matcher();
        let test = matcher.test().filter(|_| self.filter_rows);
        let Some(test) = test else { return shaped.map(<[usize]>::to_vec) };
        let base: Vec<usize> = shaped.map_or_else(|| (0..row_count).collect(), <[usize]>::to_vec);
        Some(base.into_iter().filter(|&r| row_matches(r, col_count, &value, test)).collect())
    }

    /// The matching cells over the display order.
    pub fn matches(
        &self,
        order: Option<&[usize]>,
        row_count: usize,
        col_count: usize,
        value: impl Fn(usize, usize) -> Cell,
    ) -> Vec<CellPos> {
        let matcher = self.matcher();
        let Some(test) = matcher.test() else { return Vec::new() };
        let rows: Vec<usize> = order.map_or_else(|| (0..row_count).collect(), <[usize]>::to_vec);
        find_cells(&rows, col_count, value, test)
    }

    /// The current match of `count` (clamped), if any.
    pub fn active(&self, count: usize) -> Option<usize> {
        (count > 0).then(|| self.current.min(count - 1))
    }

    /// Enter / ⇧Enter: the next / previous match, wrapping around.
    pub fn step(&mut self, forward: bool, count: usize) {
        if let Some(active) = self.active(count) {
            self.current = if forward { (active + 1) % count } else { (active + count - 1) % count };
        }
    }

    /// The counter next to the field: `n/m`, "0 results", "Bad regex", or nothing without a query.
    pub fn counter(&self, count: usize, i18n: &I18n) -> String {
        if matches!(self.matcher(), Matcher::Invalid) {
            return i18n.tx("Bad regex");
        }
        if self.query.is_empty() {
            return String::new();
        }
        match self.active(count) {
            None => i18n.tx("0 results"),
            Some(active) => format!("{}/{count}", active + 1),
        }
    }
}

/// The paging state of a console result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageState {
    /// Zero-based page shown.
    pub page: u64,
    pub page_size: u64,
    /// Rows on the page.
    pub rows: u64,
    /// More rows follow the page.
    pub has_more: bool,
    /// Counted rows of the whole query, when known.
    pub total: Option<u64>,
}

/// What the pager shows ("1-500 of 501+").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageWindow {
    /// One-based numbers of the first and last row shown (0 and 0 on an empty page).
    pub first: u64,
    pub last: u64,
    /// Rows of the whole query: known once counted or on reaching the end.
    pub total: Option<u64>,
    /// Zero-based last page, when the total is known.
    pub last_page: Option<u64>,
}

pub fn page_window(s: PageState) -> PageWindow {
    let offset = s.page * s.page_size;
    let known = s.total.or((!s.has_more).then_some(offset + s.rows));
    PageWindow {
        first: if s.rows > 0 { offset + 1 } else { 0 },
        last: offset + s.rows,
        total: known,
        last_page: known.map(|k| k.div_ceil(s.page_size.max(1)).saturating_sub(1)),
    }
}

/// A page is read by running the query again: the backend reads past the rows before it and
/// returns only the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageFetch {
    pub max_rows: u64,
    pub skip: u64,
}

pub fn page_fetch(page: u64, page_size: u64) -> PageFetch {
    PageFetch { max_rows: page_size, skip: page * page_size }
}

#[cfg(test)]
mod tests {
    use super::super::export::{ExportColumn, ExportData, ExportFormat, ExportOptions, export_data};
    use super::super::model::{
        format_order_by, guess_separator, import_edits, parse_delimited, parse_order_by, passes_object_filter,
    };
    use super::*;
    use ide_db::{CellValue, DbDriver, DbRowEdit, SortKey};

    fn s(v: &str) -> Cell {
        Some(v.to_string())
    }

    fn rows() -> Vec<Vec<Cell>> {
        vec![
            vec![s("1"), s("Shanghai"), s("10")],
            vec![s("2"), s("Beijing"), None],
            vec![s("3"), s("Shanghai"), s("2.5")],
            vec![s("4"), None, s("7")],
        ]
    }

    fn value_of(rows: &[Vec<Cell>]) -> impl Fn(usize, usize) -> Cell + '_ {
        |r, c| rows.get(r).and_then(|row| row.get(c)).cloned().flatten()
    }

    #[test]
    fn compares_numbers_by_value_null_first() {
        let mut v = vec![s("10"), s("9"), None, s("abc")];
        v.sort_by(compare_cells);
        assert_eq!(v, [None, s("9"), s("10"), s("abc")]);
    }

    #[test]
    fn keeps_the_rows_whose_values_are_checked_then_sorts_them() {
        let rows = rows();
        let value = value_of(&rows);
        let filter = LocalFilter::from([(1, vec![s("Shanghai"), None])]);
        assert_eq!(visible_rows(4, &value, &filter, None, None), [0, 2, 3]);
        assert_eq!(visible_rows(4, &value, &filter, Some(LocalSort { column: 2, descending: true }), None), [0, 3, 2]);
        let sort = Some(LocalSort { column: 2, descending: false });
        assert_eq!(visible_rows(4, &value, &LocalFilter::new(), sort, None), [1, 2, 3, 0]);
    }

    #[test]
    fn leaves_rows_past_fixed_added_rows_last_and_unfiltered() {
        let rows = rows();
        let filter = LocalFilter::from([(1, vec![s("Beijing")])]);
        assert_eq!(visible_rows(4, value_of(&rows), &filter, None, Some(3)), [1, 3]);
    }

    #[test]
    fn lists_distinct_values_with_counts_and_toggles_them() {
        let rows = rows();
        let d = distinct_values(4, value_of(&rows), 1, DISTINCT_LIMIT);
        assert_eq!(
            d,
            [
                DistinctValue { value: None, count: 1 },
                DistinctValue { value: s("Beijing"), count: 1 },
                DistinctValue { value: s("Shanghai"), count: 2 },
            ]
        );
        let all: Vec<Cell> = d.into_iter().map(|x| x.value).collect();
        let f = toggle_filter_value(&LocalFilter::new(), 1, &all, &s("Beijing"), false);
        assert_eq!(f.get(&1), Some(&vec![None, s("Shanghai")]));
        let f = toggle_filter_value(&f, 1, &all, &s("Beijing"), true);
        assert!(!f.contains_key(&1));
    }

    #[test]
    fn sums_numbers_and_reports_text_min_max() {
        let a = aggregates(&[s("10"), None, s("2.5"), s("7")]);
        assert_eq!((a.count, a.distinct, a.nulls), (4, 4, 1));
        assert_eq!((a.sum, a.avg, a.median), (Some(19.5), Some(6.5), Some(7.0)));
        assert_eq!((a.min, a.max), (s("2.5"), s("10")));
        let t = aggregates(&[s("b"), s("a"), s("b")]);
        assert_eq!((t.distinct, t.sum, t.avg, t.min, t.max), (2, None, None, s("a"), s("b")));
    }

    #[test]
    fn combines_y_per_x() {
        let rows = rows();
        let point = |label: &str, value: f64| ChartPoint { label: label.into(), value };
        assert_eq!(
            chart_series(&rows, 1, 2, ChartAggregate::Sum),
            [point("Shanghai", 12.5), point("Beijing", 0.0), point("<null>", 7.0)]
        );
        let counts: Vec<f64> = chart_series(&rows, 1, 2, ChartAggregate::Count).iter().map(|p| p.value).collect();
        assert_eq!(counts, [2.0, 1.0, 1.0]);
        let labels: Vec<String> =
            chart_series(&rows, 0, 2, ChartAggregate::None).into_iter().map(|p| p.label).collect();
        assert_eq!(labels, ["1", "3", "4"]);
    }

    #[test]
    fn picks_round_ticks_from_zero() {
        assert_eq!(nice_ticks(0.0, 3100.0, 5), [0.0, 1000.0, 2000.0, 3000.0, 4000.0]);
        assert_eq!(nice_ticks(-5.0, 5.0, 5), [-6.0, -4.0, -2.0, 0.0, 2.0, 4.0, 6.0]);
    }

    #[test]
    fn leaves_the_header_out_adds_row_numbers_and_transposes() {
        let data = ExportData {
            columns: vec![
                ExportColumn { name: "id".into(), type_name: "INTEGER".into() },
                ExportColumn { name: "city".into(), type_name: "TEXT".into() },
            ],
            rows: vec![vec![s("1"), s("Shanghai")], vec![s("2"), None]],
            table: "customers".into(),
            driver: DbDriver::Sqlite,
        };
        let opts = ExportOptions { column_header: false, row_header: false, transpose: false };
        assert_eq!(export_data(ExportFormat::Csv, &data, opts), "1,Shanghai\r\n2,");
        let numbered = ExportOptions { row_header: true, column_header: true, ..opts };
        assert_eq!(export_data(ExportFormat::Csv, &data, numbered), "#,id,city\r\n1,1,Shanghai\r\n2,2,");
        let transposed = ExportOptions { column_header: true, transpose: true, ..opts };
        assert_eq!(export_data(ExportFormat::Tsv, &data, transposed), "id\t1\t2\ncity\tShanghai\t");
        assert_eq!(export_data(ExportFormat::Csv, &data, ExportOptions::default()), "id,city\r\n1,Shanghai\r\n2,");
        assert_eq!(export_data(ExportFormat::OneRow, &data, ExportOptions::default()), "1, 'Shanghai', 2, NULL");
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_quoted_csv_and_guesses_the_separator() {
        assert_eq!(
            parse_delimited("\u{feff}a,\"b \"\"q\"\"\",c\r\n1,\"x,y\",\n", ','),
            [strings(&["a", "b \"q\"", "c"]), strings(&["1", "x,y", ""]),]
        );
        assert_eq!(guess_separator("a\tb\tc\n1\t2\t3"), '\t');
        assert_eq!(guess_separator("a;b;c"), ';');
    }

    #[test]
    fn maps_columns_by_header_name_or_position() {
        let parsed = vec![
            strings(&["CITY", "nope", "id"]),
            strings(&["Shanghai", "x", "1"]),
            strings(&["", "", ""]),
            strings(&["", "y", "2"]),
        ];
        let by_name = import_edits(&parsed, &strings(&["id", "city"]), true, true);
        assert_eq!(by_name.mapped, strings(&["city", "id"]));
        let cell =
            |column: &str, value: Option<&str>| CellValue { column: column.into(), value: value.map(str::to_string) };
        assert_eq!(
            by_name.edits,
            [
                DbRowEdit::Insert { values: vec![cell("city", Some("Shanghai")), cell("id", Some("1"))] },
                DbRowEdit::Insert { values: vec![cell("city", None), cell("id", Some("2"))] },
            ]
        );
        let by_position = import_edits(&[strings(&["1", "a"])], &strings(&["id", "city", "extra"]), false, false);
        assert_eq!(
            by_position.edits,
            [DbRowEdit::Insert { values: vec![cell("id", Some("1")), cell("city", Some("a"))] }]
        );
    }

    #[test]
    fn round_trips_the_sort_and_rejects_unknown_columns() {
        let cols = strings(&["id", "Name"]);
        let sort = vec![
            SortKey { column: "Name".into(), descending: true },
            SortKey { column: "id".into(), descending: false },
        ];
        assert_eq!(parse_order_by("name desc, \"id\" ASC", &cols), Some(sort.clone()));
        assert_eq!(format_order_by(&sort), "Name DESC, id");
        assert_eq!(parse_order_by("", &cols), Some(Vec::new()));
        assert_eq!(parse_order_by("nope", &cols), None);
    }

    #[test]
    fn matches_any_of_its_patterns_ignoring_case() {
        assert!(passes_object_filter("orders", Some("")));
        assert!(passes_object_filter("Order_Items", Some("order*, customers")));
        assert!(passes_object_filter("customers", Some("order* customers")));
        assert!(!passes_object_filter("products", Some("order*,customers")));
    }

    fn test_of(query: &str, options: SearchOptions) -> CellMatcher {
        match cell_matcher(query, options) {
            Matcher::Test(m) => m,
            other => panic!("{query}: {other:?}"),
        }
    }

    #[test]
    fn matches_cell_text_with_match_case_words_and_regex() {
        let opts = SearchOptions::default();
        let m = test_of("shang", opts);
        assert!(m.test("Shanghai"));
        assert!(!m.test("Beijing"));
        assert!(!test_of("shang", SearchOptions { case_sensitive: true, ..opts }).test("Shanghai"));
        let word = test_of("23", SearchOptions { whole_word: true, ..opts });
        assert!(word.test("23"));
        assert!(!word.test("123"));
        assert!(word.test("a 23 b"));
        let re = test_of(r"^\d\.\d$", SearchOptions { regex: true, ..opts });
        assert!(re.test("2.5"));
        assert!(!re.test("10"));
        // A dot is literal without Regex.
        assert!(!test_of("2.5", opts).test("215"));
    }

    #[test]
    fn reports_no_query_and_a_bad_regex() {
        let opts = SearchOptions::default();
        assert!(matches!(cell_matcher("", opts), Matcher::NoQuery));
        assert!(matches!(cell_matcher("(", SearchOptions { regex: true, ..opts }), Matcher::Invalid));
    }

    #[test]
    fn steps_over_astral_characters_on_empty_matches_in_words_mode() {
        for query in ["x*", "|", "^", r"\d*", r"\b"] {
            let m = test_of(query, SearchOptions { regex: true, whole_word: true, case_sensitive: false });
            // Terminates on astral text; empty matches never count as words.
            assert!(!m.test("a😀b"));
            assert!(!m.test("😀"));
        }
    }

    #[test]
    fn finds_the_matching_cells_row_by_row_skipping_null() {
        let rows = rows();
        let m = test_of("i", SearchOptions::default());
        let pos = |row, col| CellPos { row, col };
        assert_eq!(find_cells(&[0, 1, 2, 3], 3, value_of(&rows), &m), [pos(0, 1), pos(1, 1), pos(2, 1)]);
        // Display rows are positions in `rows`.
        assert_eq!(find_cells(&[3, 1], 3, value_of(&rows), &m), [pos(1, 1)]);
    }

    #[test]
    fn describes_the_rows_of_a_page_idea_style() {
        let state = |page, page_size, rows, has_more, total| PageState { page, page_size, rows, has_more, total };
        let window = |first, last, total, last_page| PageWindow { first, last, total, last_page };
        assert_eq!(page_window(state(0, 500, 500, true, None)), window(1, 500, None, None));
        assert_eq!(page_window(state(1, 500, 1, false, None)), window(501, 501, Some(501), Some(1)));
        let counted = page_window(state(0, 100, 100, true, Some(1234)));
        assert_eq!((counted.total, counted.last_page), (Some(1234), Some(12)));
        assert_eq!(page_window(state(0, 100, 0, false, None)), window(0, 0, Some(0), Some(0)));
    }

    #[test]
    fn slices_the_rows_of_a_page_from_a_fetch_of_every_row_up_to_it() {
        assert_eq!(page_fetch(0, 500), PageFetch { max_rows: 500, skip: 0 });
        assert_eq!(page_fetch(2, 100), PageFetch { max_rows: 100, skip: 200 });
    }

    #[test]
    fn opens_the_search_bar_on_ctrl_f_and_highlights_the_matches_of_the_page() {
        let data = [["Shanghai", "1"], ["Beijing", "2"], ["Shenzhen", "3"], ["Hangzhou", "4"]];
        let value = |r: usize, c: usize| data.get(r).map(|row| row[c].to_string());
        let i18n = I18n::default();
        let mut search = ResultSearch { open: true, ..ResultSearch::default() };
        search.set_query("sh");
        let marked = |s: &ResultSearch| {
            let order = s.order(None, 4, 2, value);
            s.matches(order.as_deref(), 4, 2, value)
                .iter()
                .map(|m| {
                    let r = order.as_ref().map_or(m.row, |o| o[m.row]);
                    data[r][m.col]
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(marked(&search), ["Shanghai", "Shenzhen"]);
        assert_eq!(search.counter(2, &i18n), "1/2");
        search.step(true, 2);
        assert_eq!(search.counter(2, &i18n), "2/2");

        // Match Case: "sh" no longer matches "Sh…".
        search.set_options(SearchOptions { case_sensitive: true, ..search.options });
        assert!(marked(&search).is_empty());
        assert_eq!(search.counter(0, &i18n), "0 results");
        search.set_options(SearchOptions { case_sensitive: false, ..search.options });

        // Filter rows keeps only the rows with a match.
        search.set_filter_rows(true);
        assert_eq!(search.order(None, 4, 2, value).map(|o| o.len()), Some(2));

        search.open = false;
        assert_eq!(search.order(None, 4, 2, value), None);
    }
}
