//! Annotate with Git Blame: the column at the left edge of the editor's gutter that shows, for each
//! line, the date and the author of the commit that last changed it.
//!
//! The workbench asks the backend for the blame and hands the editor an [`Annotations`]; the editor
//! draws it, keeps it in step with the edits until the next blame arrives, and reports a press on
//! the column as an [`AnnotationEvent`]. A line not committed yet has an entry that draws nothing.
//! The newer a line's commit, the stronger its text: the oldest commit is drawn in the gutter's
//! colour mixed with a quarter of the secondary text colour, the newest in the secondary colour.

use super::Editor;
use crate::editor::buffer;
use crate::platform::date_format::{CivilDateTime, DateFormatOptions, format_clock, format_day};
use crate::platform::local_time::utc_offset_minutes;
use crate::theme;
use crate::ui::tooltip::{TOOLTIP_DELAY, tooltip, tooltip_lines};
use crate::vcs_diff::inline_blame::is_uncommitted;
use crate::vcs_diff::model::format_date;
use gpui_kit::{
    App, Context, Div, EventEmitter, Hsla, Pixels, Point, Rgba, ShapedLine, Stateful, TextAlign, TextRun, Window, div,
    fill, font, point, prelude::*, px, size,
};
use ide_core::TextChange;
use ide_vcs::BlameLine;
use ropey::Rope;

/// Space left of the date.
const PAD_LEFT: f32 = 6.0;
/// Space right of the author, before the column's border.
const PAD_RIGHT: f32 = 8.0;
/// Space between the date and the author.
const GAP: f32 = 8.0;
/// The border between the column and the line numbers.
const BORDER: f32 = 1.0;
/// The narrowest the date and the author are drawn, in widths of the digit zero.
const DATE_MIN_CHARS: f32 = 7.0;
const AUTHOR_MIN_CHARS: f32 = 9.0;
/// The longest a shown author name is, in UTF-16 code units, before it is cut with an ellipsis.
const AUTHOR_MAX: usize = 12;
/// The author the column is at least as wide as.
const SPACER_AUTHOR: &str = "Mmmmmmmm";
/// 2020-12-28 00:00 UTC: the date the column is at least as wide as.
const SPACER_TIME_MS: f64 = 1_609_113_600_000.0;
/// What the tooltip of a line not committed yet says.
pub const UNCOMMITTED_TOOLTIP: &str = "Not committed yet";
/// The debug selector of the area over the hovered entry that holds its tooltip.
pub const HOVER_SELECTOR: &str = "editor-annotation-hover";

/// The first word of `name`, cut to eleven UTF-16 code units and an ellipsis when it is longer than
/// twelve.
pub fn short_author(name: &str) -> String {
    let first = name.split_whitespace().next().unwrap_or("");
    if first.encode_utf16().count() <= AUTHOR_MAX {
        return first.to_owned();
    }
    let mut out = String::new();
    let mut units = 0;
    for c in first.chars() {
        units += c.len_utf16();
        if units > AUTHOR_MAX - 1 {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

/// A commit time that counts: blame gives zero when it has none.
fn known_time(time_ms: f64) -> Option<f64> {
    (time_ms != 0.0 && time_ms.is_finite()).then_some(time_ms)
}

/// How new each line's commit is among the commits of `lines`: 0 for the oldest, 1 for the newest,
/// and 1 for a line without a time, a line not committed yet, or when all the times are the same.
pub fn age_ranks(lines: &[BlameLine]) -> Vec<f32> {
    let times: Vec<f64> =
        lines.iter().filter(|line| !is_uncommitted(line)).filter_map(|line| known_time(line.time_ms)).collect();
    let (Some(min), Some(max)) = (times.iter().copied().reduce(f64::min), times.iter().copied().reduce(f64::max))
    else {
        return vec![1.0; lines.len()];
    };
    lines
        .iter()
        .map(|line| match known_time(line.time_ms) {
            Some(time) if !is_uncommitted(line) && max > min => ((time - min) / (max - min)) as f32,
            _ => 1.0,
        })
        .collect()
}

/// `a` mixed with `b` in sRGB, `share` (0 to 1) of it `a`.
fn mix(a: Hsla, b: Hsla, share: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let blend = |x: f32, y: f32| x * share + y * (1.0 - share);
    Hsla::from(Rgba { r: blend(a.r, b.r), g: blend(a.g, b.g), b: blend(a.b, b.b), a: blend(a.a, b.a) })
}

/// The share of the secondary text colour in a line of age `rank`: 25 % for the oldest commit up to
/// 100 % for the newest, in whole percents.
pub fn rank_share(rank: f32) -> f32 {
    (25.0 + rank * 75.0).round() / 100.0
}

/// What the column shows for one line.
#[derive(Clone, Debug, PartialEq)]
pub struct AnnotationRow {
    /// The full commit id.
    pub commit: String,
    /// The line is not committed yet: nothing is drawn and a press opens nothing.
    pub uncommitted: bool,
    /// The commit's short date, as the Date Formats settings write it.
    pub date: String,
    /// The first word of the author's name.
    pub author: String,
    /// How new the commit is among the file's: 0 (oldest) to 1 (newest).
    pub rank: f32,
    /// The tooltip's first line: the short commit id, the author's full name and the commit's date
    /// and time; empty for a line not committed yet.
    pub title: String,
    /// The tooltip's second line: the commit's summary.
    pub summary: String,
}

/// The tooltip's first line for `line`: eight characters of the commit id, the author and the
/// commit's date and time, two spaces apart.
fn tooltip_title(line: &BlameLine, options: &DateFormatOptions) -> String {
    let id: String = line.commit.chars().take(8).collect();
    let when = known_time(line.time_ms).map_or_else(String::new, |ms| {
        let at = CivilDateTime::from_unix_ms(ms, utc_offset_minutes(ms));
        format!("{} {}", format_day(at, options, false), format_clock(at, options))
    });
    format!("{id}  {}  {when}", line.author)
}

/// The blame of an editor's lines, entry by document line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotations {
    rows: Vec<Option<AnnotationRow>>,
    /// The date the column is at least as wide as, written as the rows' dates are.
    spacer_date: String,
    /// The line whose entry is under the pointer.
    hovered: Option<u32>,
}

impl Annotations {
    /// The column for `lines`, one blame line per document line, the dates written with `options`
    /// in the user's time zone.
    pub fn new(lines: &[BlameLine], options: &DateFormatOptions) -> Self {
        let ranks = age_ranks(lines);
        let date = |ms: f64| format_date(Some(ms), utc_offset_minutes(ms), options);
        let rows = lines
            .iter()
            .zip(ranks)
            .map(|(line, rank)| {
                let uncommitted = is_uncommitted(line);
                Some(AnnotationRow {
                    commit: line.commit.clone(),
                    uncommitted,
                    date: if uncommitted { String::new() } else { date(line.time_ms) },
                    author: if uncommitted { String::new() } else { short_author(&line.author) },
                    rank,
                    title: if uncommitted { String::new() } else { tooltip_title(line, options) },
                    summary: if uncommitted { String::new() } else { line.summary.clone() },
                })
            })
            .collect();
        Self { rows, spacer_date: date(SPACER_TIME_MS), hovered: None }
    }

    /// The entry of the 0-based `line`; none for a line typed since the blame.
    pub fn row(&self, line: u32) -> Option<&AnnotationRow> {
        self.rows.get(line as usize).and_then(Option::as_ref)
    }

    /// Makes one entry per line of a document of `lines` lines: blame lines past its end are
    /// dropped, and lines past the blame's end have none.
    fn fit(&mut self, lines: u32) {
        self.rows.resize(lines as usize, None);
    }

    /// Follows `changes` (in the coordinates of `old`, ordered and apart): the lines a change
    /// joins lose their entries but the first, and the lines it adds have none until the next blame.
    pub fn map(&mut self, old: &Rope, changes: &[TextChange]) {
        for change in changes.iter().rev() {
            let (first, _) = buffer::line_col(old, change.from);
            let (last, _) = buffer::line_col(old, change.to);
            let added = Rope::from_str(&change.insert).len_lines().saturating_sub(1);
            let start = (first as usize + 1).min(self.rows.len());
            let end = (last as usize + 1).clamp(start, self.rows.len());
            self.rows.splice(start..end, std::iter::repeat_n(None, added));
        }
        self.hovered = None;
    }
}

/// What the column tells the editor's owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnotationEvent {
    /// The column was pressed with the left button on the 0-based `line`.
    Pressed { line: u32 },
}

impl EventEmitter<AnnotationEvent> for Editor {}

/// The column as a frame draws it: its width and the shaped entries of the painted rows.
pub(super) struct PreparedColumn {
    width: Pixels,
    date_width: Pixels,
    border: Hsla,
    /// Per painted row: the shaped date and author, if the line has a committed entry.
    rows: Vec<Option<(ShapedLine, ShapedLine)>>,
}

impl PreparedColumn {
    pub(super) fn width(&self) -> Pixels {
        self.width
    }

    /// Paints the entry of the painted row `index`, whose top is at `top`, and the column's border
    /// beside it; `left` is the editor's left edge.
    pub(super) fn paint_row(
        &self,
        index: usize,
        left: Pixels,
        top: Pixels,
        line_height: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) {
        let border = gpui_kit::Bounds::new(point(left + self.width - px(BORDER), top), size(px(BORDER), line_height));
        window.paint_quad(fill(border, self.border));
        if let Some((date, author)) = self.rows.get(index).and_then(Option::as_ref) {
            let at = left + px(PAD_LEFT);
            let _ = date.paint(point(at, top), line_height, TextAlign::Left, None, window, cx);
            let after = at + self.date_width + px(GAP);
            let _ = author.paint(point(after, top), line_height, TextAlign::Left, None, window, cx);
        }
    }
}

impl Editor {
    /// Whether the editor shows blame annotations.
    pub fn annotations_shown(&self) -> bool {
        self.annotations.is_some()
    }

    /// Shows `annotations` in the column, or takes the column away (`None`).
    pub fn set_annotations(&mut self, annotations: Option<Annotations>, cx: &mut Context<Self>) {
        self.annotations = annotations.map(|mut annotations| {
            annotations.fit(self.line_count());
            annotations
        });
        cx.notify();
    }

    /// The column's entry of the 0-based `line`.
    pub fn annotation(&self, line: u32) -> Option<&AnnotationRow> {
        self.annotations.as_ref()?.row(line)
    }

    /// The entries the last frame painted, by document line.
    pub fn painted_annotations(&self) -> Vec<(u32, AnnotationRow)> {
        let (Some(layout), Some(annotations)) = (self.layout.as_ref(), self.annotations.as_ref()) else {
            return Vec::new();
        };
        layout.line_of.iter().filter_map(|&line| Some((line, annotations.row(line)?.clone()))).collect()
    }

    /// The width of the column as the last frame painted it; none while it is not shown.
    pub fn annotation_column_width(&self) -> Pixels {
        self.layout.as_ref().map_or(px(0.0), |layout| layout.annotation_column)
    }

    /// The line whose entry in the column is under `position`.
    pub fn annotation_line_at(&self, position: Point<Pixels>) -> Option<u32> {
        let layout = self.layout.as_ref()?;
        let x = position.x - layout.bounds.left();
        if self.annotations.is_none() || x < px(0.0) || x >= layout.annotation_column {
            return None;
        }
        self.gutter_line_at(position)
    }

    /// A left press at `position`: on the column it is reported for the line and taken (no caret
    /// moves, no breakpoint toggles). Answers whether the column took it.
    pub(super) fn press_annotation(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) -> bool {
        let Some(line) = self.annotation_line_at(position) else { return false };
        cx.emit(AnnotationEvent::Pressed { line });
        true
    }

    /// The pointer moved to `position`: the entry under it is drawn in the strong text colour.
    pub(super) fn hover_annotation(&mut self, position: Option<Point<Pixels>>, cx: &mut Context<Self>) {
        let line = position.and_then(|position| self.annotation_line_at(position));
        if let Some(annotations) = self.annotations.as_mut()
            && annotations.hovered != line
        {
            annotations.hovered = line;
            cx.notify();
        }
    }

    /// The area over the hovered entry, which shows its tooltip once the pointer rests there: the
    /// commit, the author, the date and the summary, or that the line is not committed yet.
    pub(super) fn annotation_hover_target(&self) -> Option<Stateful<Div>> {
        let annotations = self.annotations.as_ref()?;
        let line = annotations.hovered?;
        let row = annotations.row(line)?;
        let layout = self.layout.as_ref()?;
        let top = layout.line_height * self.core.folds.row_of_line(line) as f32 - self.scroll_top;
        let target = div()
            .id(("annotation-hover", line as usize))
            .debug_selector(|| HOVER_SELECTOR.to_owned())
            .absolute()
            .left(px(0.0))
            .top(top)
            .w(layout.annotation_column)
            .h(layout.line_height);
        let target = if row.uncommitted {
            target.tooltip(tooltip(UNCOMMITTED_TOOLTIP))
        } else {
            target.tooltip(tooltip_lines(row.title.clone(), row.summary.clone()))
        };
        Some(target.tooltip_show_delay(TOOLTIP_DELAY))
    }

    /// The column for the painted `lines`, shaped; none while no annotations are shown or the editor
    /// draws no gutter.
    pub(super) fn prepare_annotations(
        &self,
        lines: &[u32],
        gutter_shown: bool,
        window: &mut Window,
        cx: &App,
    ) -> Option<PreparedColumn> {
        let annotations = self.annotations.as_ref().filter(|_| gutter_shown)?;
        let font_size = theme::length(cx, "fs-small");
        let face = font(theme::installed_fonts::ui_family(cx));
        let (secondary, faint, strong) =
            (theme::color(cx, "fg-secondary"), theme::color(cx, "fg-gutter"), theme::color(cx, "fg-strong"));
        let shape = |text: &str, color: Hsla, window: &mut Window| {
            let run = TextRun {
                len: text.len(),
                font: face.clone(),
                color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            window.text_system().shape_line(text.to_owned().into(), font_size, &[run], None)
        };
        let zero = shape("0", secondary, window).width;
        let mut date_width = (zero * DATE_MIN_CHARS).max(shape(&annotations.spacer_date, secondary, window).width);
        let mut author_width = (zero * AUTHOR_MIN_CHARS).max(shape(SPACER_AUTHOR, secondary, window).width);
        let rows: Vec<Option<(ShapedLine, ShapedLine)>> = lines
            .iter()
            .map(|&line| {
                let row = annotations.row(line).filter(|row| !row.uncommitted)?;
                let hovered = annotations.hovered == Some(line);
                let color = if hovered { strong } else { mix(secondary, faint, rank_share(row.rank)) };
                let date = shape(&row.date, color, window);
                let author = shape(&row.author, color, window);
                date_width = date_width.max(date.width);
                author_width = author_width.max(author.width);
                Some((date, author))
            })
            .collect();
        let width = px(PAD_LEFT) + date_width + px(GAP) + author_width + px(PAD_RIGHT) + px(BORDER);
        Some(PreparedColumn { width, date_width, border: theme::color(cx, "border"), rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(commit: &str, author: &str, time_ms: f64) -> BlameLine {
        BlameLine { commit: commit.to_owned(), author: author.to_owned(), time_ms, summary: "summary".to_owned() }
    }

    fn uncommitted() -> BlameLine {
        line(&"0".repeat(40), "Not Committed Yet", 5_000.0)
    }

    #[test]
    fn the_author_is_the_first_word_of_the_name_cut_past_twelve_characters() {
        assert_eq!(short_author("  Ada Lovelace "), "Ada");
        assert_eq!(short_author("Abcdefghijkl Long"), "Abcdefghijkl");
        assert_eq!(short_author("Abcdefghijklm"), "Abcdefghijk…");
        assert_eq!(short_author(""), "");
    }

    #[test]
    fn ranks_go_from_the_oldest_commit_to_the_newest_and_uncommitted_lines_count_as_newest() {
        let lines = [line("a", "Ada", 1_000.0), line("b", "Bob", 3_000.0), line("c", "Cy", 2_000.0), uncommitted()];
        assert_eq!(age_ranks(&lines), [0.0, 1.0, 0.5, 1.0]);
        // One time, or none at all: every line is drawn as the newest.
        assert_eq!(age_ranks(&[line("a", "Ada", 1_000.0), line("a", "Ada", 1_000.0)]), [1.0, 1.0]);
        assert_eq!(age_ranks(&[uncommitted(), line("a", "Ada", 0.0)]), [1.0, 1.0]);
        assert_eq!((rank_share(0.0), rank_share(0.5), rank_share(1.0)), (0.25, 0.63, 1.0));
    }

    #[test]
    fn an_uncommitted_line_has_an_entry_that_shows_nothing() {
        let options = DateFormatOptions::default();
        let annotations = Annotations::new(&[line(&"a".repeat(40), "Ada Lovelace", 1_000.0), uncommitted()], &options);
        let first = annotations.row(0).expect("the committed line has an entry");
        assert_eq!((first.author.as_str(), first.uncommitted), ("Ada", false));
        assert_eq!(first.date, format_date(Some(1_000.0), utc_offset_minutes(1_000.0), &options));
        let at = CivilDateTime::from_unix_ms(1_000.0, utc_offset_minutes(1_000.0));
        let when = format!("{} {}", format_day(at, &options, false), format_clock(at, &options));
        assert_eq!(first.title, format!("aaaaaaaa  Ada Lovelace  {when}"));
        assert_eq!(first.summary, "summary");
        let second = annotations.row(1).expect("the uncommitted line has an entry");
        assert!(second.uncommitted && second.date.is_empty() && second.author.is_empty() && second.title.is_empty());
        assert!(annotations.row(2).is_none());
    }

    #[test]
    fn entries_follow_the_lines_through_edits_until_the_next_blame() {
        let options = DateFormatOptions::default();
        let blame: Vec<BlameLine> =
            ["a", "b", "c", "d"].iter().map(|id| line(&id.repeat(40), "Ada", 1_000.0)).collect();
        let commit = |annotations: &Annotations, at: u32| annotations.row(at).map(|row| row.commit[..1].to_owned());
        let old = Rope::from_str("one\ntwo\nthree\nfour");

        // A line break typed at the end of the first line: the new line has no entry.
        let mut split = Annotations::new(&blame, &options);
        split.map(&old, &[TextChange { from: 3, to: 3, insert: "\nnew".to_owned() }]);
        let shown: Vec<Option<String>> = (0..5).map(|at| commit(&split, at)).collect();
        let expected = [Some("a"), None, Some("b"), Some("c"), Some("d")].map(|c| c.map(str::to_owned));
        assert_eq!(shown, expected);

        // Joining the second and the third line keeps the second's entry.
        let mut joined = Annotations::new(&blame, &options);
        joined.map(&old, &[TextChange { from: 7, to: 8, insert: String::new() }]);
        let shown: Vec<Option<String>> = (0..3).map(|at| commit(&joined, at)).collect();
        assert_eq!(shown, [Some("a"), Some("b"), Some("d")].map(|c| c.map(str::to_owned)));

        // Two changes at once, in the old text's coordinates.
        let mut both = Annotations::new(&blame, &options);
        let changes = [
            TextChange { from: 0, to: 0, insert: "x\n".to_owned() },
            TextChange { from: 14, to: 14, insert: "\ny".to_owned() },
        ];
        both.map(&old, &changes);
        let shown: Vec<Option<String>> = (0..6).map(|at| commit(&both, at)).collect();
        let expected = [Some("a"), None, Some("b"), Some("c"), Some("d"), None].map(|c| c.map(str::to_owned));
        assert_eq!(shown, expected);
    }
}
