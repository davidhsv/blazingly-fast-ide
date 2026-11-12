//! Reports the conflict markers a merge left in a file as errors, whatever its language and whether
//! or not a language server runs for it: without a server (Groovy, plain text…) nothing else would
//! say the file is unresolved.

use crate::platform::i18n::I18n;
use crate::vcs::conflicts::parse_conflicts;
use ide_core::{Diagnostic, Pos, Range, Severity};
use std::collections::HashMap;

/// Larger documents (in UTF-16 code units) are not scanned.
pub const MAX_LENGTH: usize = 2_000_000;
/// How long a document is left alone after an edit before it is inspected again, in milliseconds.
pub const DELAY_MS: u64 = 250;
/// The inspection the diagnostics are published under.
pub const CONFLICT_MARKERS_PRODUCER: &str = "merge-conflict-markers";

/// Whether `text` is longer than [`MAX_LENGTH`], counting only when its bytes leave a doubt.
fn oversized(text: &str) -> bool {
    text.len() > MAX_LENGTH && text.encode_utf16().count() > MAX_LENGTH
}

/// Line and UTF-16 column of the byte offsets in `offsets`, which must be ascending.
fn positions(text: &str, offsets: &[usize]) -> Vec<Pos> {
    let mut out = Vec::with_capacity(offsets.len());
    let (mut line, mut line_start, mut scanned) = (0u32, 0usize, 0usize);
    for &offset in offsets {
        let offset = offset.min(text.len());
        for (i, byte) in text.as_bytes()[scanned..offset].iter().enumerate() {
            if *byte == b'\n' {
                line += 1;
                line_start = scanned + i + 1;
            }
        }
        scanned = offset;
        out.push(Pos::new(line, text[line_start..offset].encode_utf16().count() as u32));
    }
    out
}

/// An error on every marker line of the complete conflict blocks in `text`.
pub fn marker_diagnostics(text: &str, i18n: &I18n) -> Vec<Diagnostic> {
    // A cheap test first: most files have no marker at all.
    if oversized(text) || !text.contains("<<<<<<<") {
        return Vec::new();
    }
    let markers: Vec<std::ops::Range<usize>> =
        parse_conflicts(text).into_iter().flat_map(|block| block.markers).collect();
    let offsets: Vec<usize> = markers.iter().flat_map(|marker| [marker.start, marker.end]).collect();
    let message = i18n.tx("Unresolved merge conflict marker");
    positions(text, &offsets)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|ends| Diagnostic {
            range: Range::new(ends[0], ends[1]),
            severity: Severity::Error,
            message: message.clone(),
            source: Some("Git".to_owned()),
            code: None,
            unnecessary: false,
            deprecated: false,
            server: None,
        })
        .collect()
}

/// How many editors show each file: its diagnostics go away with the last one.
#[derive(Clone, Debug, Default)]
pub struct MarkerViewers {
    viewers: HashMap<String, usize>,
}

impl MarkerViewers {
    /// An editor opened `path`; its markers are published at once.
    pub fn open(&mut self, path: &str) {
        *self.viewers.entry(path.to_owned()).or_insert(0) += 1;
    }

    /// An editor of `path` closed. Returns whether it was the last one, whose diagnostics are to be
    /// withdrawn.
    pub fn close(&mut self, path: &str) -> bool {
        match self.viewers.get_mut(path) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            _ => {
                self.viewers.remove(path);
                true
            }
        }
    }
}

/// When an editor inspects its document again: once it has been left alone for [`DELAY_MS`], so a
/// burst of edits costs one inspection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MarkerInspection {
    due_at: Option<u64>,
}

impl MarkerInspection {
    /// The document changed at `now_ms`.
    pub fn doc_changed(&mut self, now_ms: u64) {
        self.due_at = Some(now_ms + DELAY_MS);
    }

    /// When the next inspection is due, if one is.
    pub fn due_at(&self) -> Option<u64> {
        self.due_at
    }

    /// Whether to inspect the document now; true once per quiet period.
    pub fn take_due(&mut self, now_ms: u64) -> bool {
        if self.due_at.is_some_and(|due| due <= now_ms) {
            self.due_at = None;
            return true;
        }
        false
    }

    /// The editor closed: nothing is pending any more.
    pub fn cancel(&mut self) {
        self.due_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<Diagnostic> {
        marker_diagnostics(text, &I18n::default())
    }

    fn range(start: (u32, u32), end: (u32, u32)) -> Range {
        Range::new(Pos::new(start.0, start.1), Pos::new(end.0, end.1))
    }

    #[test]
    fn reports_an_error_on_every_marker_line_of_a_conflict_whatever_the_language() {
        let text = concat!(
            "def allServices = [\n<<<<<<< Updated upstream\n  'a',\n",
            "=======\n  'b',\n>>>>>>> Stashed changes\n]\n"
        );
        let found = found(text);
        let lines: Vec<(u32, u32, Severity)> =
            found.iter().map(|d| (d.range.start.line, d.range.end.line, d.severity)).collect();
        assert_eq!(lines, [(1, 1, Severity::Error), (3, 3, Severity::Error), (5, 5, Severity::Error)]);
        assert_eq!(found[0].range.end.col as usize, "<<<<<<< Updated upstream".len());
        assert!(
            found.iter().all(|d| d.message == "Unresolved merge conflict marker" && d.source.as_deref() == Some("Git"))
        );
    }

    #[test]
    fn includes_the_base_marker_of_a_diff3_conflict() {
        let text = "<<<<<<< a\nx\n||||||| base\nb\n=======\ny\n>>>>>>> c\n";
        assert_eq!(found(text).len(), 4);
    }

    #[test]
    fn uses_utf16_columns_and_excludes_crlf_terminators() {
        let found = found("😀\r\n<<<<<<< ours 😀\r\nx\r\n=======\r\ny\r\n>>>>>>> theirs\r\n");
        assert_eq!(found[0].range, range((1, 0), (1, 15)));
        assert_eq!(found[1].range, range((3, 0), (3, 7)));
    }

    #[test]
    fn skips_oversized_documents() {
        assert!(found(&"x".repeat(2_000_001)).is_empty());
    }

    #[test]
    fn reports_nothing_for_text_without_conflicts_or_a_block_that_was_never_closed() {
        assert!(found("a\nb\n").is_empty());
        assert!(found("<<<<<<< a\nx\n=======\ny\n").is_empty());
        // A heading underline of seven equals signs is not a conflict.
        assert!(found("Title\n=======\ntext\n").is_empty());
    }

    #[test]
    fn coalesces_a_burst_of_edits_in_a_large_document_into_one_inspection() {
        let mut text = "plain text\n".repeat(100_000);
        let mut inspection = MarkerInspection::default();
        let mut published: Vec<usize> = Vec::new();
        let mut now = 0u64;
        for insert in ["<<<<<<< ours\n", "a\n=======\n", "b\n>>>>>>> theirs\n"] {
            text.push_str(insert);
            inspection.doc_changed(now);
            now += 100;
            if inspection.take_due(now) {
                published.push(found(&text).len());
            }
        }
        assert!(published.is_empty());
        now += 250;
        if inspection.take_due(now) {
            published.push(found(&text).len());
        }
        assert!(!inspection.take_due(now + 1000));
        assert_eq!(published, [3]);
    }

    // Not from a spec: a file's diagnostics are withdrawn with its last editor.
    #[test]
    fn withdraws_diagnostics_with_the_last_viewer() {
        let mut viewers = MarkerViewers::default();
        viewers.open("/a");
        viewers.open("/a");
        assert!(!viewers.close("/a"));
        assert!(viewers.close("/a"));
    }
}
