//! Edit › Paste from History and Paste as Plain Text: the parts that are not the chooser popup or
//! the system clipboard.

use super::clipboard::paste;
use super::clipboard_history::ClipboardHistory;
use super::{Edit, EditState};

/// The hint Paste from History shows instead of its chooser: the history is empty.
pub fn paste_from_history_hint(history: &ClipboardHistory) -> Option<&'static str> {
    history.entries().is_empty().then_some("Nothing to paste: the clipboard history is empty")
}

/// Pastes the text chosen in Paste from History. The choice goes on top of the history (and the
/// caller puts it on the system clipboard), then pastes as a paste of the clipboard would: a
/// line-wise copy above the caret lines.
pub fn paste_chosen(state: &EditState, history: &mut ClipboardHistory, text: &str, linewise: bool) -> Option<Edit> {
    history.record(text, linewise);
    paste(state, text, history.is_linewise_text(text))
}

/// Paste as Plain Text: the clipboard's `text` exactly as copied, without Reformat on Paste. A
/// line-wise copy still goes above the caret line; empty text pastes only as a line-wise copy.
pub fn paste_as_plain_text(state: &EditState, history: &ClipboardHistory, text: &str) -> Option<Edit> {
    paste(state, text, history.is_linewise_text(text))
}

#[cfg(test)]
mod tests {
    use super::super::clipboard_history::ClipboardEntry;
    use super::super::testing::run;
    use super::super::{SelectionExt, apply, caret};
    use super::*;

    fn target() -> EditState {
        EditState::new("target")
    }

    #[test]
    fn opens_the_chooser_and_pastes_its_result_through_the_existing_paste_path() {
        let mut history = ClipboardHistory::default();
        history.record("older", false);
        history.record("newer\nsecond line", false);
        assert_eq!(paste_from_history_hint(&history), None);
        let state = target().with_selections(vec![(0, 6)], 0);
        let joined = "newer\nsecond line\nolder";
        let edit = paste_chosen(&state, &mut history, joined, false).expect("the choice pastes");
        assert_eq!(apply(&state, &edit).text(), joined);
        assert_eq!(history.entries().first(), Some(&ClipboardEntry { text: joined.to_owned(), linewise: false }));
    }

    #[test]
    fn pastes_a_chosen_line_wise_entry_above_the_caret_line_and_keeps_its_flag() {
        let mut history = ClipboardHistory::default();
        history.record("one", true);
        history.record("selected text", false);
        let state = target().with_selections(vec![caret(3)], 0);
        let after = apply(&state, &paste_chosen(&state, &mut history, "one", true).expect("the choice pastes"));
        assert_eq!(after.text(), "one\ntarget");
        assert_eq!(after.main().head(), 7);
        assert_eq!(history.entries().first(), Some(&ClipboardEntry { text: "one".to_owned(), linewise: true }));
    }

    #[test]
    fn keeps_the_existing_empty_history_hint() {
        let history = ClipboardHistory::default();
        assert_eq!(paste_from_history_hint(&history), Some("Nothing to paste: the clipboard history is empty"));
    }

    #[test]
    fn pastes_line_wise_as_plain_text_without_reformatting() {
        let text = "      a();\n          b();";
        let mut history = ClipboardHistory::default();
        history.record(text, true);
        let state = EditState::new("if {\n  target\n}").with_selections(vec![caret(10)], 0);
        let after = apply(&state, &paste_as_plain_text(&state, &history, text).expect("the text pastes"));
        assert_eq!(after.text(), format!("if {{\n{text}\n  target\n}}"));
        assert_eq!(after.main().head(), 10 + text.len() as u32 + 1);
    }

    #[test]
    fn handles_an_empty_plain_text_clipboard_with_line_wise_flag() {
        for linewise in [true, false] {
            let mut history = ClipboardHistory::default();
            if linewise {
                history.record("", true);
            }
            let state = target().with_selections(vec![caret(3)], 0);
            let after = run(&state, paste_as_plain_text(&state, &history, ""));
            assert_eq!(after.text(), if linewise { "\ntarget" } else { "target" }, "{linewise}");
            assert_eq!(after.main().head(), if linewise { 4 } else { 3 }, "{linewise}");
        }
    }
}
