//! The refactoring preview popup: the occurrences an edit touches or leaves, a summary per file,
//! warnings, and the button that carries the edit out.

use crate::editor::buffer;
use ropey::Rope;

/// One occurrence in the preview: its 0-based line, the line's text, and the columns it spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewOccurrence {
    pub line: u32,
    pub text: String,
    pub from: u32,
    pub to: u32,
}

/// The occurrences of a file the preview counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewEntry {
    pub path: String,
    pub edits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewSummary {
    pub files: usize,
    pub occurrences: usize,
    pub has_file_operations: bool,
    pub entries: Vec<PreviewEntry>,
}

/// What the preview popup shows. `title` is in the UI language; `warnings` and `confirm_label`
/// are English source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    pub title: String,
    pub summary: PreviewSummary,
    pub warnings: Vec<String>,
    pub occurrences: Vec<PreviewOccurrence>,
    pub confirm_label: &'static str,
}

/// The preview rows of the spans `from..to` of a document (a span is cut at its line's end).
pub fn occurrences_of(doc: &Rope, spans: &[(u32, u32)]) -> Vec<PreviewOccurrence> {
    spans
        .iter()
        .map(|(from, to)| {
            let (line, _) = buffer::line_col(doc, *from);
            let range = buffer::line_range(doc, line);
            PreviewOccurrence {
                line,
                text: buffer::slice(doc, range.start, range.end),
                from: from - range.start,
                to: (*to).min(range.end) - range.start,
            }
        })
        .collect()
}
