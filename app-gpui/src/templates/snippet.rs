//! Snippet templates and their fields.
//!
//! A template such as `for (let ${1:i} = 0; ${1:i} < ${2}; ${1:i}++) {\n\t${0}\n}` is text with
//! placeholders: `${name}` or `#{name}` (the name is the field's default content), numbered
//! `${1}` / `${1:default}` to set the tab order, and `${0}` for where the caret ends up. A brace
//! with a backslash in front is literal. Inserted lines take the indentation of the line the
//! snippet starts on, plus one indent unit per leading tab.
//!
//! Inserting selects every occurrence of the first field; [`ActiveSnippet`] then carries the
//! fields through edits and moves between them until the last one is reached or the selection
//! leaves the active field.

use crate::editor::buffer::{self, Assoc};
use crate::find::utf16;
use ide_core::TextChange;
use regex::Regex;
use ropey::Rope;
use std::ops::Range;
use std::sync::LazyLock;

/// The tab order number `${0}` stands for: after every other stop.
const FINAL_STOP: u64 = 1_000_000_000;

/// One occurrence of a field in the document (UTF-16 offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRange {
    /// The field's place in the tab order, from 0.
    pub field: usize,
    pub from: u32,
    pub to: u32,
}

/// One occurrence of a field in a template line (UTF-16 columns once parsed).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FieldPos {
    field: usize,
    line: usize,
    from: usize,
    to: usize,
}

/// A parsed template: its lines without the placeholders' markup, and where the fields are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    lines: Vec<String>,
    positions: Vec<FieldPos>,
}

/// The lines a snippet inserts at a position and its fields there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub text: Vec<String>,
    pub ranges: Vec<FieldRange>,
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

fn unescape_braces(text: &str) -> String {
    text.replace("\\{", "{").replace("\\}", "}")
}

impl Snippet {
    pub fn parse(template: &str) -> Self {
        static FIELD: LazyLock<Regex> =
            LazyLock::new(|| pattern(r"[#$]\{(?:([0-9]+)(?::([^{}]*))?|((?:\\[{}]|[^{}])*))\}"));
        static ESCAPED_BRACE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\\([{}])"));
        // The tab order: (number, name) of each field. Positions are in bytes until a line is done.
        let mut fields: Vec<(Option<u64>, String)> = Vec::new();
        let mut lines: Vec<String> = Vec::new();
        let mut positions: Vec<FieldPos> = Vec::new();
        let normalized = template.replace("\r\n", "\n").replace('\r', "\n");
        for source in normalized.split('\n') {
            let index = lines.len();
            let mut line = source.to_owned();
            while let Some(found) = FIELD.captures(&line) {
                let whole = found.get(0).map_or(0..0, |m| m.range());
                let digits = found.get(1).map_or("", |m| m.as_str());
                let default = found.get(2).map_or("", |m| m.as_str());
                let raw_name = if default.is_empty() { found.get(3).map_or("", |m| m.as_str()) } else { default };
                let raw_name = raw_name.to_owned();
                let seq = (!digits.is_empty())
                    .then(|| digits.parse::<u64>().unwrap_or(u64::MAX))
                    .map(|seq| if seq == 0 { FINAL_STOP } else { seq });
                let name = unescape_braces(&raw_name);
                let known = fields.iter().rposition(|(field_seq, field_name)| match seq {
                    Some(_) => *field_seq == seq,
                    None => !name.is_empty() && *field_name == name,
                });
                let field = known.unwrap_or_else(|| {
                    // A numbered field goes before the first one that is unnumbered or numbered
                    // at least as high; an unnumbered one goes last.
                    let at = fields
                        .iter()
                        .position(|(field_seq, _)| seq.is_some_and(|seq| field_seq.is_none_or(|other| other >= seq)))
                        .unwrap_or(fields.len());
                    fields.insert(at, (seq, name.clone()));
                    for pos in &mut positions {
                        if pos.field >= at {
                            pos.field += 1;
                        }
                    }
                    at
                });
                let markup = if default.is_empty() { 2 } else { 3 + digits.len() };
                for pos in &mut positions {
                    if pos.line == index && pos.from > whole.start {
                        pos.from = pos.from.saturating_sub(markup);
                        pos.to = pos.to.saturating_sub(markup);
                    }
                }
                positions.push(FieldPos { field, line: index, from: whole.start, to: whole.start + name.len() });
                line.replace_range(whole, &raw_name);
            }
            let mut plain = String::with_capacity(line.len());
            let mut copied = 0;
            for escape in ESCAPED_BRACE.find_iter(&line) {
                for pos in &mut positions {
                    if pos.line == index && pos.from > escape.start() {
                        pos.from -= 1;
                        pos.to = pos.to.saturating_sub(1);
                    }
                }
                plain.push_str(&line[copied..escape.start()]);
                plain.push_str(&line[escape.start() + 1..escape.end()]);
                copied = escape.end();
            }
            plain.push_str(&line[copied..]);
            for pos in positions.iter_mut().filter(|pos| pos.line == index) {
                pos.from = buffer::col_of_byte(&plain, pos.from) as usize;
                pos.to = buffer::col_of_byte(&plain, pos.to) as usize;
            }
            lines.push(plain);
        }
        Self { lines, positions }
    }

    /// The snippet as inserted at `pos`, on a line whose text is `line_text`.
    pub fn instantiate(&self, line_text: &str, pos: u32, indent_unit: &str) -> Instance {
        let base_indent: String = line_text.chars().take_while(|c| c.is_whitespace()).collect();
        let mut text: Vec<String> = Vec::new();
        let mut line_starts: Vec<i64> = vec![i64::from(pos)];
        let mut pos = i64::from(pos);
        for line in &self.lines {
            let mut line = line.clone();
            if !text.is_empty() {
                let tabs = line.chars().take_while(|&c| c == '\t').count();
                let indent = format!("{base_indent}{}", indent_unit.repeat(tabs));
                line_starts.push(pos + i64::from(utf16::len(&indent)) - tabs as i64);
                line = format!("{indent}{}", &line[tabs..]);
            }
            pos += i64::from(utf16::len(&line)) + 1;
            text.push(line);
        }
        let ranges = self
            .positions
            .iter()
            .map(|field| {
                let start = line_starts.get(field.line).copied().unwrap_or(0);
                FieldRange {
                    field: field.field,
                    from: (start + field.from as i64).max(0) as u32,
                    to: (start + field.to as i64).max(0) as u32,
                }
            })
            .collect();
        Instance { text, ranges }
    }
}

fn field_selection(ranges: &[FieldRange], field: usize) -> Vec<Range<u32>> {
    ranges.iter().filter(|range| range.field == field).map(|range| range.from..range.to).collect()
}

/// The fields of an inserted snippet and the one the caret is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveSnippet {
    pub ranges: Vec<FieldRange>,
    pub active: usize,
}

/// A move to another field: what to select, and the fields that stay active (none after the
/// last one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldMove {
    pub selection: Vec<Range<u32>>,
    pub active: Option<ActiveSnippet>,
}

impl ActiveSnippet {
    /// The fields after `changes` (ordered, non-overlapping, in the coordinates of the document
    /// before them); `None` when a deletion went across a field's edge.
    pub fn map(&self, changes: &[TextChange]) -> Option<Self> {
        let deleted = |pos: u32| changes.iter().any(|change| change.from < pos && pos < change.to);
        let mut ranges = Vec::with_capacity(self.ranges.len());
        for range in &self.ranges {
            if deleted(range.from) || deleted(range.to) {
                return None;
            }
            ranges.push(FieldRange {
                field: range.field,
                from: buffer::map_offset(range.from, changes, Assoc::Before),
                to: buffer::map_offset(range.to, changes, Assoc::After),
            });
        }
        Some(Self { ranges, active: self.active })
    }

    /// Whether every selection range lies inside an occurrence of the active field.
    pub fn selection_inside_field(&self, selection: &[Range<u32>]) -> bool {
        selection.iter().all(|selected| {
            self.ranges
                .iter()
                .any(|range| range.field == self.active && range.from <= selected.start && range.to >= selected.end)
        })
    }

    /// The fields after an edit or a selection change of the user: carried through the changes,
    /// and dropped once the selection is outside the active field.
    pub fn after_update(&self, changes: &[TextChange], selection: Option<&[Range<u32>]>) -> Option<Self> {
        let mapped = if changes.is_empty() { self.clone() } else { self.map(changes)? };
        match selection {
            Some(selection) if !mapped.selection_inside_field(selection) => None,
            _ => Some(mapped),
        }
    }

    pub fn has_next_field(&self) -> bool {
        self.ranges.iter().any(|range| range.field == self.active + 1)
    }

    pub fn has_prev_field(&self) -> bool {
        self.active > 0
    }

    /// The move to the next field. Reaching the last one ends the snippet.
    pub fn next_field(&self) -> FieldMove {
        let next = self.active + 1;
        let last = !self.ranges.iter().any(|range| range.field == next + 1);
        FieldMove {
            selection: field_selection(&self.ranges, next),
            active: (!last).then(|| Self { ranges: self.ranges.clone(), active: next }),
        }
    }

    /// The move to the previous field; `None` on the first one.
    pub fn prev_field(&self) -> Option<FieldMove> {
        let prev = self.active.checked_sub(1)?;
        Some(FieldMove {
            selection: field_selection(&self.ranges, prev),
            active: Some(Self { ranges: self.ranges.clone(), active: prev }),
        })
    }
}

/// A snippet ready to go into a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnippetInsertion {
    /// The range to replace.
    pub from: u32,
    pub to: u32,
    pub insert: String,
    /// What to select once inserted: every occurrence of the first field. Empty for a snippet
    /// without fields, whose caret just moves with the edit.
    pub selection: Vec<Range<u32>>,
    /// The fields to keep active; `None` for a snippet with a single field.
    pub active: Option<ActiveSnippet>,
}

impl SnippetInsertion {
    /// The insertion as a document change.
    pub fn change(&self) -> TextChange {
        TextChange { from: self.from, to: self.to, insert: self.insert.clone() }
    }
}

/// `template` replacing `from..to` of `doc`. When `to` is where the main selection starts, the
/// whole selection is replaced. `indent_unit` is what one leading tab of a template line becomes.
pub fn apply_snippet(
    template: &str,
    doc: &Rope,
    main: Range<u32>,
    from: u32,
    to: u32,
    indent_unit: &str,
) -> SnippetInsertion {
    let (line, _) = buffer::line_col(doc, from);
    let line_text = buffer::line_text(doc, line);
    let Instance { text, ranges } = Snippet::parse(template).instantiate(&line_text, from, indent_unit);
    let several = ranges.iter().any(|range| range.field > 0);
    let active = several.then(|| ActiveSnippet { ranges: ranges.clone(), active: 0 });
    SnippetInsertion {
        from,
        to: if to == main.start { main.end } else { to },
        insert: text.join("\n"),
        selection: field_selection(&ranges, 0),
        active,
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;

    /// The indent unit of the reference editor's tests.
    pub(crate) const INDENT: &str = "  ";

    /// Replaces `from..to` (UTF-16 offsets) of `rope` with `insert`.
    pub(crate) fn replace(rope: &mut Rope, from: u32, to: u32, insert: &str) {
        let start = rope.utf16_cu_to_char(from as usize);
        let end = rope.utf16_cu_to_char(to as usize);
        rope.remove(start..end);
        rope.insert(start, insert);
    }

    /// Puts `insertion` into `rope`.
    pub(crate) fn insert(rope: &mut Rope, insertion: &SnippetInsertion) {
        replace(rope, insertion.from, insertion.to, &insertion.insert);
    }

    /// The text and the selection after `template` replaced `from..to` of `doc`, the caret being
    /// at the start of the document.
    pub(crate) fn applied(doc: &str, template: &str, from: u32, to: u32) -> (String, Vec<(u32, u32)>) {
        let mut rope = Rope::from_str(doc);
        let insertion = apply_snippet(template, &rope, 0..0, from, to, INDENT);
        insert(&mut rope, &insertion);
        (rope.to_string(), insertion.selection.iter().map(|range| (range.start, range.end)).collect())
    }
}

#[cfg(test)]
#[expect(clippy::single_range_in_vec_init, reason = "a single selection is the case under test")]
mod tests {
    use super::testing::applied;
    use super::*;

    fn range(field: usize, from: u32, to: u32) -> FieldRange {
        FieldRange { field, from, to }
    }

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.to_owned() }
    }

    // Not from a spec: named fields are linked and ordered as written, numbered ones by number,
    // and `${0}` comes last wherever it is written.
    #[test]
    fn orders_fields_by_number_then_by_appearance() {
        let instance = Snippet::parse("${0}${b} ${2:x} #{a} ${1} ${b}").instantiate("", 0, "  ");
        assert_eq!(instance.text, ["b x a  b"]);
        assert_eq!(
            instance.ranges,
            [range(2, 0, 0), range(3, 0, 1), range(1, 2, 3), range(4, 4, 5), range(0, 6, 6), range(3, 7, 8)]
        );
    }

    // Not from a spec: escaped braces are text, inside a field's name too, and positions after
    // them shift; columns are UTF-16.
    #[test]
    fn unescapes_braces_and_counts_columns_in_utf16() {
        let instance = Snippet::parse(r"\{😀\} ${a\{b} ${1:é}").instantiate("", 10, "  ");
        assert_eq!(instance.text, ["{😀} a{b é"]);
        assert_eq!(instance.ranges, [range(1, 15, 18), range(0, 19, 20)]);
    }

    // Not from a spec: later lines take the start line's indentation and one unit per leading tab.
    #[test]
    fn indents_lines_after_the_first() {
        let (text, selection) = applied("  x", "a {\n\t${1:b}\n\t\tc\r\n}${0}", 2, 3);
        assert_eq!(text, "  a {\n    b\n      c\n  }");
        assert_eq!(selection, [(10, 11)]);
        // A snippet without fields selects nothing, and has no fields to walk.
        let rope = Rope::from_str("p");
        let insertion = apply_snippet("<p>x#\\{y}</p>", &rope, 1..1, 0, 1, "  ");
        assert_eq!(insertion.insert, "<p>x#{y}</p>");
        assert!(insertion.selection.is_empty() && insertion.active.is_none());
        assert_eq!(insertion.change(), change(0, 1, "<p>x#{y}</p>"));
        // Ending where the selection starts replaces the selection too.
        assert_eq!(apply_snippet("x", &Rope::from_str("abcd"), 1..3, 0, 1, "  ").to, 3);
    }

    // Not from a spec: Tab walks the fields in order and ends the snippet on the last one;
    // Shift-Tab walks back.
    #[test]
    fn moves_between_fields_until_the_last() {
        let rope = Rope::from_str("");
        let insertion = apply_snippet("${1:a} ${2:b} ${1:a}${0}", &rope, 0..0, 0, 0, "  ");
        assert_eq!(insertion.selection, [0..1, 4..5]);
        let first = insertion.active.expect("several fields stay active");
        assert!(first.has_next_field() && !first.has_prev_field());
        assert!(first.prev_field().is_none());
        let second = first.next_field();
        assert_eq!(second.selection, [2..3]);
        let second = second.active.expect("the final stop is still ahead");
        assert_eq!(second.prev_field().map(|back| back.selection), Some(vec![0..1, 4..5]));
        let last = second.next_field();
        assert_eq!(last.selection, [5..5]);
        assert_eq!(last.active, None);
    }

    // Not from a spec: fields follow edits, grow with typing at their edges, and end when the
    // selection leaves the active field or a deletion crosses an edge.
    #[test]
    fn fields_follow_edits_and_end_outside_the_active_field() {
        let active = ActiveSnippet { ranges: vec![range(0, 2, 3), range(1, 6, 6)], active: 0 };
        let typed = active.after_update(&[change(2, 3, "xy")], Some(&[4..4])).expect("typing stays inside");
        assert_eq!(typed.ranges, [range(0, 2, 4), range(1, 7, 7)]);
        assert_eq!(active.after_update(&[], Some(&[5..5])), None);
        assert_eq!(active.after_update(&[], Some(&[2..3])), Some(active.clone()));
        assert_eq!(active.map(&[change(1, 5, "")]), None);
        let shifted = active.map(&[change(0, 2, "")]).map(|mapped| mapped.ranges);
        assert_eq!(shifted, Some(vec![range(0, 0, 1), range(1, 4, 4)]));
    }
}
