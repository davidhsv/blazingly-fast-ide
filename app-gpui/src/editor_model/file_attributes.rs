//! File › File Properties › Make File Read-only / Writable (IDEA's Toggle Read-only Attribute,
//! also the status bar lock): asks, then the caller flips the file's writable bit and the open
//! document's editability.

use crate::platform::i18n::{I18n, format_message};
use crate::platform::virtual_docs::basename;

/// The confirmation asked before toggling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOnlyQuestion {
    pub title: String,
    pub message: String,
    pub confirm: String,
}

pub fn read_only_question(name: &str, read_only: bool, i18n: &I18n) -> ReadOnlyQuestion {
    let text = |source: &str| format_message(&i18n.tx(source), &[("name", name.into())], i18n.locale());
    if read_only {
        ReadOnlyQuestion {
            title: i18n.tx("Make File Writable"),
            message: text("'{name}' is read-only. Make it writable?"),
            confirm: i18n.tx("Make Writable"),
        }
    } else {
        ReadOnlyQuestion {
            title: i18n.tx("Make File Read-only"),
            message: text("Make '{name}' read-only?"),
            confirm: i18n.tx("Make Read-only"),
        }
    }
}

/// What the document shows instead of its text, when anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Banner {
    ReadOnly,
    Binary,
    TooLarge,
}

/// The document the toggle acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToggleTarget<'a> {
    pub path: &'a str,
    /// A library class or other server-provided document.
    pub virtual_document: bool,
    pub read_only: bool,
    pub banner: Option<Banner>,
}

/// The question to ask before toggling, then [`confirmed`] with the answer. Library, binary and
/// too-large documents have no such attribute: nothing is asked.
pub fn toggle_read_only(target: &ToggleTarget, i18n: &I18n) -> Option<ReadOnlyQuestion> {
    if target.virtual_document || matches!(target.banner, Some(Banner::Binary | Banner::TooLarge)) {
        return None;
    }
    Some(read_only_question(&basename(target.path), target.read_only, i18n))
}

/// The answer to [`toggle_read_only`]'s question: `Some(writable)` to set on the file, `None` on
/// Cancel.
pub fn confirmed(target: &ToggleTarget, ok: bool) -> Option<bool> {
    ok.then_some(target.read_only)
}

/// The error title when setting the attribute failed.
pub fn toggle_failure_title(path: &str, writable: bool) -> String {
    format!("Cannot make {} {}", basename(path), if writable { "writable" } else { "read-only" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(read_only: bool) -> ToggleTarget<'static> {
        ToggleTarget {
            path: "/p/a.txt",
            virtual_document: false,
            read_only,
            banner: read_only.then_some(Banner::ReadOnly),
        }
    }

    #[test]
    fn asks_before_making_a_read_only_file_writable_then_toggles() {
        let i18n = I18n::default();
        let file = target(true);
        let question = toggle_read_only(&file, &i18n).expect("asks");
        assert_eq!(question.title, "Make File Writable");
        assert_eq!(question.confirm, "Make Writable");
        assert_eq!(question.message, "'a.txt' is read-only. Make it writable?");
        assert_eq!(confirmed(&file, true), Some(true));
    }

    #[test]
    fn cancel_leaves_the_file_alone() {
        let i18n = I18n::default();
        let file = target(false);
        let question = toggle_read_only(&file, &i18n).expect("asks");
        assert_eq!(question.title, "Make File Read-only");
        assert_eq!(confirmed(&file, false), None);
        assert_eq!(toggle_failure_title(file.path, false), "Cannot make a.txt read-only");
    }

    #[test]
    fn does_nothing_for_library_and_binary_documents() {
        let i18n = I18n::default();
        let library = ToggleTarget { virtual_document: true, ..target(true) };
        let binary = ToggleTarget { banner: Some(Banner::Binary), ..target(true) };
        assert_eq!(toggle_read_only(&library, &i18n), None);
        assert_eq!(toggle_read_only(&binary, &i18n), None);
        assert_eq!(read_only_question("a.txt", false, &i18n).confirm, "Make Read-only");
    }
}
