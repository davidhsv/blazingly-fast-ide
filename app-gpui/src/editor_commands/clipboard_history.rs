//! The clipboard history behind Paste from History: text copied or cut, newest first, each text
//! once, with whether it was a line-wise copy. The latest entry also says whether the clipboard
//! holds a line-wise copy.

use super::utf16_len;
use crate::editor::buffer::byte_of_col;

/// How many entries the history keeps.
pub const MAX_CLIPBOARD_ENTRIES: usize = 30;

/// One copied text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardEntry {
    pub text: String,
    /// The copy took the caret lines (nothing was selected).
    pub linewise: bool,
}

/// The copied texts, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipboardHistory {
    entries: Vec<ClipboardEntry>,
}

impl ClipboardHistory {
    pub fn entries(&self) -> &[ClipboardEntry] {
        &self.entries
    }

    /// Puts `text` on top; an equal entry moves up (taking the new line-wise flag) instead of
    /// repeating, and the oldest entries past the cap drop. Empty text that is not a line-wise
    /// copy is not recorded.
    pub fn record(&mut self, text: &str, linewise: bool) {
        if text.is_empty() && !linewise {
            return;
        }
        self.entries.retain(|entry| entry.text != text);
        self.entries.insert(0, ClipboardEntry { text: text.to_owned(), linewise });
        self.entries.truncate(MAX_CLIPBOARD_ENTRIES);
    }

    /// The clipboard holds a line-wise copy of exactly `text`: only the latest copy counts.
    pub fn is_linewise_text(&self, text: &str) -> bool {
        self.entries.first().is_some_and(|entry| entry.linewise && entry.text == text)
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drops the entries holding any of `texts`, keeping the others in order.
    pub fn remove(&mut self, texts: &[&str]) {
        self.entries.retain(|entry| !texts.contains(&entry.text.as_str()));
    }
}

/// How a history row reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipLabel {
    /// The first non-blank line, trimmed and cut to the row's width with an ellipsis; `None`
    /// when the text is only whitespace.
    pub label: Option<String>,
    /// The number of lines when there are several (a trailing line break ends the last line
    /// rather than starting one).
    pub lines: Option<usize>,
}

/// The label of a history row for `text`, at most `max` UTF-16 code units long.
pub fn clip_label(text: &str, max: u32) -> ClipLabel {
    let unix = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = unix.split('\n').collect();
    let first = lines.iter().map(|line| line.trim()).find(|line| !line.is_empty()).unwrap_or_default();
    let label = if utf16_len(first) > max {
        format!("{}…", &first[..byte_of_col(first, max.saturating_sub(1))])
    } else {
        first.to_owned()
    };
    let count = if text.ends_with(['\r', '\n']) { lines.len() - 1 } else { lines.len() };
    ClipLabel { label: (!label.is_empty()).then_some(label), lines: (count > 1).then_some(count) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::clipboard::copy;
    use crate::editor_commands::{EditState, caret};

    fn entry(text: &str, linewise: bool) -> ClipboardEntry {
        ClipboardEntry { text: text.to_owned(), linewise }
    }

    #[test]
    fn keeps_entries_newest_first_without_duplicates_capped() {
        let mut history = ClipboardHistory::default();
        history.record("a", false);
        history.record("b", false);
        history.record("a", false);
        history.record("", false);
        assert_eq!(history.entries(), [entry("a", false), entry("b", false)]);
        for index in 0..MAX_CLIPBOARD_ENTRIES + 5 {
            history.record(&format!("x{index}"), false);
        }
        assert_eq!(history.entries().len(), MAX_CLIPBOARD_ENTRIES);
        assert_eq!(history.entries()[0].text, format!("x{}", MAX_CLIPBOARD_ENTRIES + 4));
    }

    #[test]
    fn copies_the_selections_or_whole_caret_lines_without_a_selection() {
        let doc = "one\ntwo\nthree";
        let selected = EditState::new(doc).with_selections(vec![(0, 3), (8, 13)], 0);
        assert_eq!(copy(&selected).text, "one\nthree");
        let carets = EditState::new(doc).with_selections(vec![caret(1), caret(2), caret(5)], 0);
        assert_eq!(copy(&carets).text, "one\ntwo");
    }

    #[test]
    fn keeps_a_line_wise_flag_with_older_history_entries_and_restores_it_when_promoted() {
        let mut history = ClipboardHistory::default();
        history.record("one", true);
        history.record("two", false);
        assert_eq!(history.entries(), [entry("two", false), entry("one", true)]);
        assert!(!history.is_linewise_text("two"));
        assert!(!history.is_linewise_text("one"));
        let promoted = history.entries()[1].clone();
        history.record(&promoted.text, promoted.linewise);
        assert!(history.is_linewise_text("one"));
        assert!(!history.is_linewise_text("one\n"));
        assert!(!history.is_linewise_text("other app"));
    }

    #[test]
    fn replaces_the_flag_when_the_same_text_is_copied_as_a_selection() {
        let mut history = ClipboardHistory::default();
        history.record("one", true);
        history.record("one", false);
        assert_eq!(history.entries(), [entry("one", false)]);
        assert!(!history.is_linewise_text("one"));
    }

    #[test]
    fn clears_remembered_line_wise_text_with_the_history() {
        let mut history = ClipboardHistory::default();
        history.record("one", true);
        history.clear();
        assert!(!history.is_linewise_text("one"));
    }

    #[test]
    fn removes_entries_by_content_without_disturbing_the_remaining_history() {
        let mut history = ClipboardHistory::default();
        history.record("oldest", false);
        history.record("middle", false);
        history.record("newest", false);
        history.remove(&["oldest", "newest", "missing", "newest"]);
        let texts = |history: &ClipboardHistory| -> Vec<String> {
            history.entries().iter().map(|entry| entry.text.clone()).collect()
        };
        assert_eq!(texts(&history), ["middle"]);
        history.remove(&[]);
        assert_eq!(texts(&history), ["middle"]);
        history.record("newest", false);
        assert_eq!(texts(&history), ["newest", "middle"]);
        let all = texts(&history);
        history.remove(&all.iter().map(String::as_str).collect::<Vec<_>>());
        assert!(history.entries().is_empty());
    }

    #[test]
    fn labels_rows_by_their_first_non_blank_line_and_line_count() {
        assert_eq!(clip_label("\n  foo()\nbar\n", 80), ClipLabel { label: Some("foo()".to_owned()), lines: Some(3) });
        assert_eq!(clip_label(&"x".repeat(100), 10).label, Some(format!("{}…", "x".repeat(9))));
        assert_eq!(clip_label("   ", 80).label, None);
        assert_eq!(clip_label("first\rsecond\r", 80).lines, Some(2));
    }
}
