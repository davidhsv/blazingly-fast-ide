//! Diff viewer settings (the diff toolbar): viewer kind, whitespace policy, highlighting, collapsed
//! unchanged fragments and synchronized scrolling, together with the chunks of two documents as
//! IntelliJ's comparison finds them and the edit that copies a chunk from one side to the other.
//!
//! Offsets are UTF-16 code units. A chunk's `to` is one past the end of its last line, past the line
//! break, so for the last line of a document it is one more than the document's length.

use super::comparison::moves::{ChunkSpan, MovedBlock, find_moved_blocks};
use super::comparison::text::strip_spaces;
use super::comparison::{CompareOptions, ComparisonPolicy, LineFragment, compare_texts};
use super::doc::Doc;
use ide_core::TextChange;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A side of a diff: `A` is the old (left) document, `B` the new (right) one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    A,
    B,
}

impl Side {
    pub fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffMode {
    #[default]
    Split,
    Unified,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum IgnorePolicy {
    #[default]
    None,
    Trim,
    Whitespace,
    WhitespaceAndLines,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HighlightPolicy {
    #[default]
    Words,
    Lines,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffViewOptions {
    pub mode: DiffMode,
    pub ignore: IgnorePolicy,
    pub highlight: HighlightPolicy,
    pub collapse: bool,
    /// Side-by-side panes scroll together.
    pub sync_scroll: bool,
    /// Side-by-side panes are padded so that unchanged lines line up. Off, each pane keeps its own
    /// height and a curved divider joins the changes.
    pub align_changes: bool,
    /// Long lines wrap; `None` follows the editor's soft-wrap setting for the file.
    pub soft_wrap: Option<bool>,
}

/// The whitespace policies with their English menu labels.
pub const IGNORE_OPTIONS: [(IgnorePolicy, &str); 4] = [
    (IgnorePolicy::None, "Do not ignore"),
    (IgnorePolicy::Trim, "Trim whitespaces"),
    (IgnorePolicy::Whitespace, "Ignore whitespaces"),
    (IgnorePolicy::WhitespaceAndLines, "Ignore whitespaces and empty lines"),
];

/// The highlighting policies with their English menu labels.
pub const HIGHLIGHT_OPTIONS: [(HighlightPolicy, &str); 3] = [
    (HighlightPolicy::Words, "Highlight words"),
    (HighlightPolicy::Lines, "Highlight lines"),
    (HighlightPolicy::None, "Do not highlight"),
];

pub const DEFAULT_DIFF_OPTIONS: DiffViewOptions = DiffViewOptions {
    mode: DiffMode::Split,
    ignore: IgnorePolicy::None,
    highlight: HighlightPolicy::Words,
    collapse: true,
    sync_scroll: true,
    align_changes: false,
    soft_wrap: None,
};

impl Default for DiffViewOptions {
    fn default() -> Self {
        DEFAULT_DIFF_OPTIONS
    }
}

/// The key the options are stored under.
pub const DIFF_OPTIONS_KEY: &str = "openidea.vcs.diffOptions";

/// Parses stored options, falling back to the default field by field.
pub fn parse_diff_options(raw: Option<&Value>) -> DiffViewOptions {
    let d = DEFAULT_DIFF_OPTIONS;
    let field = |name: &str| raw.and_then(Value::as_object).and_then(|o| o.get(name));
    let one_of = |name: &str| field(name).filter(|v| v.is_string()).cloned();
    let flag = |name: &str, fallback: bool| field(name).and_then(Value::as_bool).unwrap_or(fallback);
    let unified = field("mode").and_then(Value::as_str) == Some("unified");
    DiffViewOptions {
        mode: if unified { DiffMode::Unified } else { DiffMode::Split },
        ignore: one_of("ignore").and_then(|v| serde_json::from_value(v).ok()).unwrap_or(d.ignore),
        highlight: one_of("highlight").and_then(|v| serde_json::from_value(v).ok()).unwrap_or(d.highlight),
        collapse: flag("collapse", d.collapse),
        sync_scroll: flag("syncScroll", d.sync_scroll),
        align_changes: flag("alignChanges", d.align_changes),
        soft_wrap: field("softWrap").and_then(Value::as_bool).or(d.soft_wrap),
    }
}

/// The options held in `stored`, the text saved under [`DIFF_OPTIONS_KEY`]; the defaults when there
/// is none or it does not parse.
pub fn load_diff_options(stored: Option<&str>) -> DiffViewOptions {
    match stored.filter(|stored| !stored.is_empty()) {
        None => DEFAULT_DIFF_OPTIONS,
        Some(stored) => match serde_json::from_str::<Value>(stored) {
            Ok(raw) => parse_diff_options(Some(&raw)),
            Err(_) => DEFAULT_DIFF_OPTIONS,
        },
    }
}

/// The text to save under [`DIFF_OPTIONS_KEY`].
pub fn save_diff_options(options: &DiffViewOptions) -> serde_json::Result<String> {
    serde_json::to_string(options)
}

/// How two documents are compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffConfig {
    pub policy: ComparisonPolicy,
    /// Changed blocks are compared by words.
    pub words: bool,
    /// Whitespace-only blocks at the edges of a change are dropped.
    pub trim_chunks: bool,
}

/// The comparison the diff viewer runs for its whitespace and highlighting settings: lines matched
/// as IntelliJ does, word fragments by its rules, no merging of nearby changes.
pub fn idea_diff_config(ignore: IgnorePolicy, highlight: HighlightPolicy) -> DiffConfig {
    DiffConfig {
        policy: match ignore {
            IgnorePolicy::None => ComparisonPolicy::Default,
            IgnorePolicy::Trim => ComparisonPolicy::Trim,
            IgnorePolicy::Whitespace | IgnorePolicy::WhitespaceAndLines => ComparisonPolicy::Ignore,
        },
        words: highlight == HighlightPolicy::Words,
        trim_chunks: ignore == IgnorePolicy::WhitespaceAndLines,
    }
}

/// A changed range inside a chunk, relative to the chunk's start on each side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Change {
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
}

/// A changed block of lines: `[from_a, to_a)` of the old document against `[from_b, to_b)` of the
/// new one. A side without lines has `from == to`, where the lines would be.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    /// The changed ranges inside the chunk: its word fragments, or the whole block.
    pub changes: Vec<Change>,
    pub from_a: usize,
    pub to_a: usize,
    pub from_b: usize,
    pub to_b: usize,
    /// The line fragment the chunk was built from.
    fragment: Option<LineFragment>,
}

impl Chunk {
    /// A chunk given by its offsets alone.
    pub fn new(changes: Vec<Change>, from_a: usize, to_a: usize, from_b: usize, to_b: usize) -> Self {
        Self { changes, from_a, to_a, from_b, to_b, fragment: None }
    }

    fn of_fragment(f: LineFragment, a: &Doc, b: &Doc) -> Self {
        // `to` is one past the last line's end, past the document for its last line.
        let to = |start_line: usize, end_line: usize, start: usize, end: usize, doc: &Doc| {
            if start_line == end_line {
                start
            } else if end_line == doc.lines() {
                doc.len() + 1
            } else {
                end
            }
        };
        let to_a = to(f.start_line1, f.end_line1, f.start_offset1, f.end_offset1, a);
        let to_b = to(f.start_line2, f.end_line2, f.start_offset2, f.end_offset2, b);
        let changes = match &f.inner {
            Some(inner) => inner
                .iter()
                .map(|r| Change { from_a: r.start1, to_a: r.end1, from_b: r.start2, to_b: r.end2 })
                .collect(),
            None => vec![Change {
                from_a: 0,
                to_a: f.end_offset1 - f.start_offset1,
                from_b: 0,
                to_b: f.end_offset2 - f.start_offset2,
            }],
        };
        Self { changes, from_a: f.start_offset1, to_a, from_b: f.start_offset2, to_b, fragment: Some(f) }
    }

    /// Start of the chunk on `side`.
    pub fn from(&self, side: Side) -> usize {
        match side {
            Side::A => self.from_a,
            Side::B => self.from_b,
        }
    }

    /// One past the end of the chunk's last line on `side`.
    pub fn to(&self, side: Side) -> usize {
        match side {
            Side::A => self.to_a,
            Side::B => self.to_b,
        }
    }

    /// End of the chunk's text on `side`, before the last line break; its start for an empty side.
    pub fn end(&self, side: Side) -> usize {
        self.from(side).max(self.to(side).saturating_sub(1))
    }

    /// Whether the chunk has lines on `side`.
    pub fn has_lines(&self, side: Side) -> bool {
        self.from(side) != self.to(side)
    }

    /// Whether the chunk's words were compared: such a chunk gets the lighter line background with
    /// its `changes` in full colour.
    pub fn has_inner_fragments(&self) -> bool {
        self.fragment.as_ref().is_some_and(|f| f.inner.is_some())
    }

    fn span(&self) -> ChunkSpan {
        ChunkSpan { from_a: self.from_a, to_a: self.to_a, from_b: self.from_b, to_b: self.to_b }
    }
}

/// The chunks of two documents and what else the comparison found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiffChunks {
    pub chunks: Vec<Chunk>,
    /// Blocks of lines that moved from one place to another.
    pub moves: Vec<MovedBlock>,
    /// The documents differ in whitespace only.
    pub whitespace_only: bool,
}

/// Offsets of the lines `[start, end)` (0-based) of `doc`, the last line break included.
fn line_offsets(doc: &Doc, start: usize, end: usize) -> (usize, usize) {
    let line_end = |line: usize| if line + 1 == doc.lines() { doc.len() } else { doc.line(line + 2).from };
    if start == end {
        let offset = if start < doc.lines() { doc.line(start + 1).from } else { doc.len() };
        return (offset, offset);
    }
    (doc.line(start + 1).from, line_end(end - 1))
}

/// A bounded comparison for documents the line comparison gives up on (long runs of repeated
/// lines): everything between the lines the documents share at their start and at their end is one
/// change.
fn coarse_fragments(a: &Doc, b: &Doc) -> Vec<LineFragment> {
    let same = |line1: usize, line2: usize| {
        let (l1, l2) = (a.line(line1 + 1), b.line(line2 + 1));
        a.slice(l1.from, l1.to) == b.slice(l2.from, l2.to)
    };
    let shortest = a.lines().min(b.lines());
    let prefix = (0..shortest).take_while(|&i| same(i, i)).count();
    let suffix = (0..shortest - prefix).take_while(|&i| same(a.lines() - 1 - i, b.lines() - 1 - i)).count();
    let (start1, end1) = (prefix, a.lines() - suffix);
    let (start2, end2) = (prefix, b.lines() - suffix);
    if start1 == end1 && start2 == end2 {
        return Vec::new();
    }
    let (start_offset1, end_offset1) = line_offsets(a, start1, end1);
    let (start_offset2, end_offset2) = line_offsets(b, start2, end2);
    vec![LineFragment {
        start_line1: start1,
        end_line1: end1,
        start_line2: start2,
        end_line2: end2,
        start_offset1,
        end_offset1,
        start_offset2,
        end_offset2,
        inner: None,
    }]
}

/// The chunks of `a` against `b` under `config`.
pub fn build_chunks(a: &Doc, b: &Doc, config: DiffConfig) -> DiffChunks {
    let options =
        CompareOptions { policy: config.policy, words: config.words, trim_chunks: config.trim_chunks, split: false };
    // Repeated lines can exhaust the search: the viewer stays usable with a bounded, coarser diff.
    let compared = compare_texts(a.units(), b.units(), options).unwrap_or_else(|_| coarse_fragments(a, b));
    let chunks: Vec<Chunk> = compared.into_iter().map(|f| Chunk::of_fragment(f, a, b)).collect();
    let spans: Vec<ChunkSpan> = chunks.iter().map(Chunk::span).collect();
    DiffChunks {
        moves: find_moved_blocks(a, b, &spans),
        whitespace_only: a.units() != b.units() && strip_spaces(a.units()) == strip_spaces(b.units()),
        chunks,
    }
}

/// The chunks of `a` against `b` for the viewer's whitespace and highlighting settings.
pub fn idea_chunks(a: &Doc, b: &Doc, ignore: IgnorePolicy, highlight: HighlightPolicy) -> DiffChunks {
    build_chunks(a, b, idea_diff_config(ignore, highlight))
}

/// Text of the lines `[from, to)` (0-based), without the last line break.
fn lines_content(doc: &Doc, from: usize, to: usize) -> String {
    doc.slice_string(doc.line(from + 1).from, doc.line(to).to)
}

fn edit(from: usize, to: usize, insert: String) -> TextChange {
    TextChange { from: from as u32, to: to as u32, insert }
}

/// The change that copies a chunk's content from `source` to the other side (">>", "<<" and
/// Revert), in the destination's coordinates. Whole lines are replaced, so a missing final line
/// break stays missing.
pub fn transfer_chunk(chunk: &Chunk, source: Side, src: &Doc, dst: &Doc) -> TextChange {
    let Some(f) = &chunk.fragment else { return transfer_by_offsets(chunk, source, src, dst) };
    let (src_from, src_to, dst_from, dst_to) = match source {
        Side::A => (f.start_line1, f.end_line1, f.start_line2, f.end_line2),
        Side::B => (f.start_line2, f.end_line2, f.start_line1, f.end_line1),
    };
    if dst_from == dst_to {
        let text = lines_content(src, src_from, src_to);
        if dst_from == dst.lines() {
            return edit(dst.len(), dst.len(), format!("\n{text}"));
        }
        let at = dst.line(dst_from + 1).from;
        return edit(at, at, format!("{text}\n"));
    }
    let mut from = dst.line(dst_from + 1).from;
    let mut to = dst.line(dst_to).to;
    if src_from == src_to {
        // Lines removed with a line break: the one before them, or else the one after.
        if from > 0 {
            from -= 1;
        } else if to < dst.len() {
            to += 1;
        }
        return edit(from, to, String::new());
    }
    edit(from, to, lines_content(src, src_from, src_to))
}

/// [`transfer_chunk`] for a chunk that was not built from a line fragment.
fn transfer_by_offsets(chunk: &Chunk, source: Side, src: &Doc, dst: &Doc) -> TextChange {
    let target = source.other();
    let (src_from, src_to) = (chunk.from(source), chunk.to(source));
    let (dst_from, dst_to) = (chunk.from(target), chunk.to(target));
    let mut insert = src.slice_string(src_from, chunk.end(source));
    if src_from != src_to && dst_to <= dst.len() {
        insert.push('\n');
    }
    let to = dst.len().min(dst_to);
    edit(dst_from.min(to), to, insert)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn chunks(a: &str, b: &str, ignore: IgnorePolicy) -> Vec<Chunk> {
        idea_chunks(&Doc::new(a), &Doc::new(b), ignore, HighlightPolicy::Words).chunks
    }

    /// `target` with every chunk copied over from `source`, last chunk first.
    fn restore(chunks: &[Chunk], source: &Doc, target: &Doc) -> String {
        let mut restored = target.clone();
        for chunk in chunks.iter().rev() {
            restored = restored.apply(&transfer_chunk(chunk, Side::A, source, &restored));
        }
        restored.to_string()
    }

    #[test]
    fn parses_stored_options_with_per_field_defaults() {
        assert_eq!(parse_diff_options(None), DEFAULT_DIFF_OPTIONS);
        let stored = json!({
            "mode": "split",
            "ignore": "trim",
            "highlight": "lines",
            "collapse": false,
            "syncScroll": false,
            "alignChanges": false,
            "softWrap": true,
        });
        let expected = DiffViewOptions {
            mode: DiffMode::Split,
            ignore: IgnorePolicy::Trim,
            highlight: HighlightPolicy::Lines,
            collapse: false,
            sync_scroll: false,
            align_changes: false,
            soft_wrap: Some(true),
        };
        assert_eq!(parse_diff_options(Some(&stored)), expected);
        let bogus = json!({ "ignore": "bogus", "highlight": 3, "collapse": "x", "softWrap": "yes" });
        assert_eq!(parse_diff_options(Some(&bogus)), DEFAULT_DIFF_OPTIONS);
        // Options saved before soft wraps existed follow the editor's setting.
        assert_eq!(parse_diff_options(Some(&json!({ "mode": "unified" }))).soft_wrap, None);
    }

    #[test]
    fn round_trips_the_soft_wrap_override() {
        for soft_wrap in [Some(true), Some(false), None] {
            let options = DiffViewOptions { soft_wrap, ..DEFAULT_DIFF_OPTIONS };
            let saved = save_diff_options(&options).expect("the options are plain JSON");
            assert_eq!(load_diff_options(Some(&saved)), options);
        }
    }

    #[test]
    fn ignore_whitespace_policies() {
        let a = "fn main() {\n  let x = 1;\n}\n";
        let indented = "fn main() {\n    let x = 1;   \n}\n";
        let inner = "fn main() {\n  let x=1;\n}\n";
        let blank = "fn main() {\n\n  let x = 1;\n}\n";
        assert_eq!(chunks(a, indented, IgnorePolicy::None).len(), 1);
        assert!(chunks(a, indented, IgnorePolicy::Trim).is_empty());
        assert_eq!(chunks(a, inner, IgnorePolicy::Trim).len(), 1);
        assert!(chunks(a, inner, IgnorePolicy::Whitespace).is_empty());
        assert_eq!(chunks(a, blank, IgnorePolicy::Whitespace).len(), 1);
        assert!(chunks(a, blank, IgnorePolicy::WhitespaceAndLines).is_empty());
    }

    #[test]
    fn keeps_real_changes_with_word_level_ranges_when_ignoring_whitespace() {
        let a = "x\nlet total = compute(first);\ny\n";
        let b = "x\n   let total = compute(second);\ny\n";
        let found = chunks(a, b, IgnorePolicy::Whitespace);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].from_a, found[0].from_b), (2, 2));
        // Inner changes are character-precise, not whole lines.
        assert!(found[0].changes.iter().all(|change| change.to_b - change.from_b < 20));
    }

    #[test]
    fn keeps_distant_insertions_separate_in_a_long_file() {
        let lines: Vec<String> = (0..1200).map(|i| format!("line_{i}: {}{i}", "content ".repeat(8))).collect();
        let mut changed = lines.clone();
        for i in (100..=1000).rev().step_by(100) {
            changed.insert(i, format!("insert_{i} {}", "x".repeat(90)));
        }
        let (a, b) = (lines.join("\n"), changed.join("\n"));
        let result = chunks(&a, &b, IgnorePolicy::None);
        assert_eq!(result.len(), 10);
        assert!(result.iter().all(|c| c.from_a == c.to_a && c.to_b - c.from_b < 120));
        assert_eq!(restore(&result, &Doc::new(&a), &Doc::new(&b)), a);
    }

    #[test]
    fn keeps_distant_edits_separate_in_a_very_long_file() {
        let lines: Vec<String> = (0..50_000).map(|i| format!("const v{i} = {};", i * 7)).collect();
        let mut changed = lines.clone();
        for i in (1..=49_000).rev().step_by(2500) {
            changed[i] = format!("{} // edited", changed[i]);
            changed.insert(i, format!("inserted {i}"));
        }
        let result = chunks(&lines.join("\n"), &changed.join("\n"), IgnorePolicy::None);
        assert_eq!(result.len(), 20);
        assert!(result.iter().all(|c| c.to_b - c.from_b < 60));
    }

    #[test]
    fn matches_unchanged_lines_between_many_small_edits() {
        let lines: Vec<String> = (0..4000).map(|i| format!("line {i}")).collect();
        let a = lines.join("\n");
        let edit = |(i, line): (usize, &String)| if i % 2 == 1 { format!("{line} changed") } else { line.clone() };
        let edited: Vec<String> = lines.iter().enumerate().map(edit).collect();
        let b = edited.join("\n");
        assert_eq!(chunks(&a, &b, IgnorePolicy::None).len(), 2000);
        assert_eq!(chunks(&a, &b, IgnorePolicy::Whitespace).len(), 2000);
    }

    #[test]
    fn handles_empty_documents() {
        assert!(chunks("", "", IgnorePolicy::None).is_empty());
        let inserted = chunks("", "one\ntwo\n", IgnorePolicy::None);
        assert_eq!((inserted[0].from_a, inserted[0].to_a, inserted[0].from_b), (0, 0, 0));
        let deleted = chunks("one\ntwo\n", "", IgnorePolicy::Whitespace);
        assert_eq!((deleted[0].from_a, deleted[0].from_b, deleted[0].to_b), (0, 0, 0));
    }

    #[test]
    fn detects_a_missing_final_newline() {
        for (a, b) in [("same\n", "same"), ("same", "same\n")] {
            let found = chunks(a, b, IgnorePolicy::None);
            assert!(!found.is_empty(), "{a:?} against {b:?}");
            let edit = transfer_chunk(&found[0], Side::A, &Doc::new(a), &Doc::new(b));
            assert_eq!(Doc::new(b).apply(&edit).to_string(), a);
        }
    }

    #[test]
    fn diffs_changed_blocks_by_words() {
        let pairs = |a: &str, b: &str| -> Vec<(String, String)> {
            let (doc_a, doc_b) = (Doc::new(a), Doc::new(b));
            let mut out = Vec::new();
            for chunk in chunks(a, b, IgnorePolicy::None) {
                for c in &chunk.changes {
                    out.push((
                        doc_a.slice_string(chunk.from_a + c.from_a, chunk.from_a + c.to_a),
                        doc_b.slice_string(chunk.from_b + c.from_b, chunk.from_b + c.to_b),
                    ));
                }
            }
            out
        };
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        // An edited identifier is changed whole.
        assert_eq!(pairs("  syncScroll: boolean;", "  syncScrolling: boolean;"), [pair("syncScroll", "syncScrolling")]);
        // Punctuation is matched between matched words only: two changed words around ":" are one
        // fragment.
        assert_eq!(
            pairs("  syncScroll: true,", "  syncScrolling: false,"),
            [pair("syncScroll: true", "syncScrolling: false")]
        );
        assert_eq!(pairs("f(a, b)", "f(a, x, b)"), [pair("", " x,")]);
        // Each character of a script written without spaces is a word.
        assert_eq!(pairs("你好世界", "你们世界"), [pair("好", "们")]);
        assert_eq!(pairs("a  b", "a b"), [pair(" ", "")]);
        // A 10,000-line block edited on every line still pairs up its words.
        let big = |keyword: &str| -> String {
            (0..10_000).map(|i| format!("export {keyword} value{i} = compute({i});\n")).collect()
        };
        let changes = pairs(&big("const"), &big("let"));
        assert_eq!(changes.len(), 10_000);
        assert!(changes.iter().all(|(a, b)| a == "const" && b == "let"));
    }

    #[test]
    fn transfers_a_chunk_between_sides() {
        let a = Doc::new("x\nold\ny\n");
        let b = Doc::new("x\nnew\nnewer\ny\n");
        // A chunk known by its offsets only: lines 2 of `a` against lines 2 and 3 of `b`.
        let chunk = Chunk::new(vec![Change { from_a: 0, to_a: 3, from_b: 0, to_b: 9 }], 2, 6, 2, 12);
        let to_b = transfer_chunk(&chunk, Side::A, &a, &b);
        assert_eq!(b.apply(&to_b).to_string(), "x\nold\ny\n");
        let to_a = transfer_chunk(&chunk, Side::B, &b, &a);
        assert_eq!(a.apply(&to_a).to_string(), "x\nnew\nnewer\ny\n");
        // Not from a spec: the chunk the comparison builds for the same documents transfers alike.
        let built = idea_chunks(&a, &b, IgnorePolicy::None, HighlightPolicy::Words).chunks;
        assert_eq!((built[0].from_a, built[0].to_a, built[0].from_b, built[0].to_b), (2, 6, 2, 12));
        assert_eq!(b.apply(&transfer_chunk(&built[0], Side::A, &a, &b)).to_string(), "x\nold\ny\n");
        assert_eq!(a.apply(&transfer_chunk(&built[0], Side::B, &b, &a)).to_string(), "x\nnew\nnewer\ny\n");
    }

    #[test]
    fn falls_back_to_transferable_chunks_on_initial_comparison_and_either_side_edits() {
        let a = Doc::new(&("alpha\n".repeat(12_000) + &"beta\n".repeat(12_000)));
        let b = Doc::new(&("beta\n".repeat(12_000) + &"alpha\n".repeat(12_000)));
        let next_b = b.replace(0, 0, "header\n");
        let next_a = a.replace(0, 0, "other\n");
        for (source, target) in [(&a, &b), (&a, &next_b), (&next_a, &b)] {
            let found = idea_chunks(source, target, IgnorePolicy::None, HighlightPolicy::Words).chunks;
            assert!(!found.is_empty());
            assert!(restore(&found, source, target) == source.to_string());
        }
    }

    // Not from a spec: the stored text keeps the key names and writes an unset soft wrap as null.
    #[test]
    fn saves_the_stored_shape() {
        let text = save_diff_options(&DEFAULT_DIFF_OPTIONS).expect("the options are plain JSON");
        assert!(text.starts_with(r#"{"mode":"split","ignore":"none","highlight":"words","collapse":true,"#));
        let saved: Value = serde_json::from_str(&text).expect("it is JSON");
        let expected = json!({
            "mode": "split",
            "ignore": "none",
            "highlight": "words",
            "collapse": true,
            "syncScroll": true,
            "alignChanges": false,
            "softWrap": null,
        });
        assert_eq!(saved, expected);
        assert_eq!(serde_json::to_value(DEFAULT_DIFF_OPTIONS).expect("it is JSON"), expected);
        assert_eq!(load_diff_options(Some("{not json")), DEFAULT_DIFF_OPTIONS);
        assert_eq!(load_diff_options(None), DEFAULT_DIFF_OPTIONS);
        let both = json!({ "ignore": "whitespaceAndLines" });
        assert_eq!(parse_diff_options(Some(&both)).ignore, IgnorePolicy::WhitespaceAndLines);
    }

    // Not from a spec: the bounded comparison keeps the lines shared at both ends out of its change.
    #[test]
    fn the_coarse_comparison_trims_shared_lines() {
        let (a, b) = (Doc::new("keep\nold\nsame\ntail"), Doc::new("keep\nnew\nmore\nsame\ntail"));
        let fragments = coarse_fragments(&a, &b);
        assert_eq!(fragments.len(), 1);
        let f = &fragments[0];
        assert_eq!((f.start_line1, f.end_line1, f.start_line2, f.end_line2), (1, 2, 1, 3));
        assert_eq!((f.start_offset1, f.end_offset1, f.start_offset2, f.end_offset2), (5, 9, 5, 14));
        assert!(coarse_fragments(&a, &a).is_empty());
        assert_eq!(coarse_fragments(&Doc::new("a\n"), &Doc::new("a")).len(), 1);
    }
}
