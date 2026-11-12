//! Reformat Code (⌥⌘L): what to ask the server for, and what its edits do to the document. An
//! explicit reformat reports "Formatted N lines" with a link to the Reformat File dialog; files
//! excluded by Do not format and text between formatter markers stay as they are.

use super::format_guard::{FormatGuardOptions, format_excluded, guard_format_changes};
use super::lsp::error_hint;
use super::positions::{apply_changes, text_edits_to_changes};
use crate::editor::buffer;
use crate::find::utf16;
use crate::platform::i18n::I18n;
use ide_core::error::{ErrorData, ErrorKind};
use ide_core::settings::Settings;
use ide_core::{IdeError, TextChange, TextEdit};
use ropey::Rope;
use std::collections::BTreeSet;

/// An indentation: the editor's, or a Code Style override.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentStyle {
    pub tab_size: u32,
    pub insert_spaces: bool,
    /// The width of one indent level with spaces; the tab size when unset.
    pub indent_size: Option<u32>,
}

/// The indentation of language-server `FormattingOptions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormattingIndent {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

/// With spaces the indent width is the indent unit, not the tab size. An explicit Code Style
/// override replaces the editor's indent; the server's formatter profile carries the same
/// override, so both agree.
pub fn format_options(editor: IndentStyle, code_style: Option<IndentStyle>) -> FormattingIndent {
    if let Some(style) = code_style {
        return if style.insert_spaces {
            FormattingIndent { tab_size: style.indent_size.unwrap_or(style.tab_size), insert_spaces: true }
        } else {
            FormattingIndent { tab_size: style.tab_size, insert_spaces: false }
        };
    }
    if editor.insert_spaces {
        let unit = editor.indent_size.unwrap_or(editor.tab_size);
        FormattingIndent { tab_size: if unit == 0 { editor.tab_size } else { unit }, insert_spaces: true }
    } else {
        FormattingIndent { tab_size: editor.tab_size, insert_spaces: false }
    }
}

/// The lines of the new text that `changes` (against `doc`) touched, as "Formatted N lines"
/// counts them.
pub fn formatted_lines(doc: &Rope, changes: &[TextChange]) -> usize {
    let mut sorted: Vec<&TextChange> = changes.iter().collect();
    sorted.sort_by_key(|change| change.from);
    let mut touched = BTreeSet::new();
    // The line shift of the changes so far, to number lines as in the new text.
    let mut shift: i64 = 0;
    for change in sorted {
        let first = i64::from(buffer::line_col(doc, change.from).0);
        let last = i64::from(buffer::line_col(doc, change.to).0);
        let inserted = change.insert.matches('\n').count() as i64;
        let start = first + shift;
        touched.extend(start..=start + inserted);
        shift += inserted - (last - first);
    }
    touched.len()
}

/// The hint of an explicit reformat that changed something.
pub fn formatted_text(i18n: &I18n, lines: usize) -> String {
    i18n.tx_template(&format!("Formatted {lines} {}", if lines == 1 { "line" } else { "lines" }))
}

/// The link of that hint, then what it opens (followed by the dialog's shortcut).
pub fn formatted_link(i18n: &I18n, shortcut: Option<&str>) -> (String, String) {
    let rest = i18n.tx(" the Reformat dialog");
    let rest = match shortcut {
        Some(keys) => format!("{rest}: {keys}"),
        None => rest,
    };
    (i18n.tx("Show"), rest)
}

pub const EXCLUDED: &str = "Formatting is turned off for this file (Code Style › Formatter › Do not format)";
pub const CHANGED_MEANWHILE: &str = "The file changed while formatting; try again";
pub const ALREADY_FORMATTED: &str = "No lines changed: content is already properly formatted";

/// How a reformat was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatAsk<'a> {
    /// ⌥⌘L, the dialog: report the outcome.
    pub explicit: bool,
    pub on_save: bool,
    /// Reformat with a named external formatter, whatever the settings select.
    pub tool: bool,
    pub path: &'a str,
    pub root: Option<&'a str>,
    pub read_only: bool,
    /// Another feature formats this language itself (SQL).
    pub contributed: bool,
    /// Settings › Editor › Format on save.
    pub format_on_save: bool,
}

/// What a reformat does first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatStart {
    /// Nothing; the hint to show, if any.
    Nothing(Option<&'static str>),
    /// Run the contributed formatter and take its changes with [`contributed_changes`].
    Contributed,
    /// Ask the server (once pending edits reached it), then [`format_answer`].
    Request,
}

pub fn start_format(ask: &FormatAsk<'_>, guard: &FormatGuardOptions) -> FormatStart {
    if format_excluded(ask.path, ask.root, &guard.do_not_format) {
        return FormatStart::Nothing(ask.explicit.then_some(EXCLUDED));
    }
    if ask.contributed && !ask.tool {
        // Only the editor's own Format on save runs the contributed formatter on save.
        if ask.on_save && !ask.format_on_save {
            return FormatStart::Nothing(None);
        }
        return FormatStart::Contributed;
    }
    if ask.read_only {
        return FormatStart::Nothing(None);
    }
    FormatStart::Request
}

/// The outcome of a reformat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Formatted {
    /// The changes to apply as one undo step; empty when nothing changes.
    pub changes: Vec<TextChange>,
    /// [`formatted_lines`] of them.
    pub lines: usize,
    /// The hint to show (English, translated when shown).
    pub hint: Option<String>,
}

impl Formatted {
    fn nothing(hint: Option<&str>) -> Self {
        Self { changes: Vec::new(), lines: 0, hint: hint.map(str::to_owned) }
    }

    fn of(doc: &Rope, changes: Vec<TextChange>, explicit: bool) -> Self {
        if changes.is_empty() {
            return Self::nothing(explicit.then_some(ALREADY_FORMATTED));
        }
        let lines = formatted_lines(doc, &changes);
        Self { changes, lines, hint: None }
    }
}

/// The server's edits for `doc` (the text formatting was asked for); `unchanged` is whether the
/// document is still that text.
pub fn format_answer(
    doc: &Rope,
    unchanged: bool,
    edits: &[TextEdit],
    guard: &FormatGuardOptions,
    explicit: bool,
) -> Formatted {
    if !unchanged {
        return Formatted::nothing(explicit.then_some(CHANGED_MEANWHILE));
    }
    let changes = guard_format_changes(&doc.to_string(), &text_edits_to_changes(doc, edits), guard);
    Formatted::of(doc, changes, explicit)
}

/// The changes of a contributed formatter for `doc`.
pub fn contributed_changes(
    doc: &Rope,
    changes: Vec<TextChange>,
    guard: &FormatGuardOptions,
    explicit: bool,
) -> Formatted {
    let mut changes: Vec<TextChange> =
        changes.into_iter().filter(|change| buffer::slice(doc, change.from, change.to) != change.insert).collect();
    changes.sort_by_key(|change| change.from);
    Formatted::of(doc, guard_format_changes(&doc.to_string(), &changes, guard), explicit)
}

/// What a failed reformat shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatFailure {
    Silent,
    /// The external formatter is not installed: offer to install it (`sticky` unless on save).
    PromptInstall {
        tool: String,
        sticky: bool,
    },
    /// A notification: its title, the body (the formatter's output, or that it timed out) and its
    /// key, so that a later failure replaces it.
    Notify {
        title: String,
        body: String,
        key: String,
    },
    Hint(String),
}

fn formatter_titles(tool: &str) -> Option<(&'static str, &'static str)> {
    Some(match tool {
        "prettier" => ("Prettier formatting failed", "Prettier formatting timed out"),
        "oxfmt" => ("oxfmt formatting failed", "oxfmt formatting timed out"),
        "ruff" => ("Ruff formatting failed", "Ruff formatting timed out"),
        _ => return None,
    })
}

pub fn format_failure(i18n: &I18n, error: &IdeError, path: &str, explicit: bool, on_save: bool) -> FormatFailure {
    if let Some(ErrorData::Tool { tool, missing }) = &error.data
        && let Some((failed, timed_out)) = formatter_titles(tool)
    {
        if error.kind == ErrorKind::NotFound && *missing {
            return FormatFailure::PromptInstall { tool: tool.clone(), sticky: !on_save };
        }
        if error.kind == ErrorKind::Cancelled {
            return FormatFailure::Silent;
        }
        let body = if error.kind == ErrorKind::TimedOut { i18n.tx(timed_out) } else { error.message.clone() };
        return FormatFailure::Notify { title: i18n.tx(failed), body, key: format!("{tool}-format-error:{path}") };
    }
    error_hint(error, explicit).map_or(FormatFailure::Silent, FormatFailure::Hint)
}

/// Whether `after` has a non-blank line at column 0 where all of `before` was indented.
pub fn dedents_to_column_zero(before: &str, after: &str) -> bool {
    fn min_indent(text: &str) -> usize {
        text.split('\n')
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
            .min()
            .unwrap_or(usize::MAX)
    }
    min_indent(before) > 0 && min_indent(after) == 0
}

/// The server's edits for lines `from..to` of generated code (quick fixes, Generate), reformatted
/// silently: only the edits inside the range. `None` when the server formatted the range out of
/// context, moving indented code to column 0; its answer is ignored then.
pub fn range_format_changes(doc: &Rope, edits: &[TextEdit], from: u32, to: u32) -> Option<Vec<TextChange>> {
    let changes: Vec<TextChange> =
        text_edits_to_changes(doc, edits).into_iter().filter(|change| change.from >= from && change.to <= to).collect();
    if changes.is_empty() {
        return Some(changes);
    }
    let delta: i64 =
        changes.iter().map(|change| i64::from(utf16::len(&change.insert)) - i64::from(change.to - change.from)).sum();
    let next = apply_changes(doc, &changes);
    let after = buffer::slice(&next, from, (i64::from(to) + delta).max(0) as u32);
    (!dedents_to_column_zero(&buffer::slice(doc, from, to), &after)).then_some(changes)
}

/// Whether saving a file of `language` formats it: the editor's Format on save, or the Run on
/// save of an external formatter covering the language.
pub fn formats_on_save(settings: Option<&Settings>, language: &str) -> bool {
    let Some(settings) = settings else { return false };
    let languages = &settings.languages;
    settings.editor.format_on_save
        || [&languages.prettier, &languages.oxfmt, &languages.ruff]
            .iter()
            .any(|formatter| formatter.run_on_save && formatter.languages.iter().any(|l| l == language))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indent(tab_size: u32, insert_spaces: bool, indent_size: Option<u32>) -> IndentStyle {
        IndentStyle { tab_size, insert_spaces, indent_size }
    }

    fn formatting(tab_size: u32, insert_spaces: bool) -> FormattingIndent {
        FormattingIndent { tab_size, insert_spaces }
    }

    #[test]
    fn asks_for_the_code_style_override_not_the_indent_detected_in_the_file() {
        // Code Style › Java: tab character, tab size 1; the file was indented with 4 spaces.
        assert_eq!(format_options(indent(4, true, None), Some(indent(1, false, Some(1)))), formatting(1, false));
    }

    #[test]
    fn sends_the_tab_size_with_tabs_and_the_indent_size_with_spaces() {
        let editor = indent(8, true, None);
        assert_eq!(format_options(editor, Some(indent(4, false, Some(2)))), formatting(4, false));
        assert_eq!(format_options(editor, Some(indent(4, true, Some(2)))), formatting(2, true));
    }

    #[test]
    fn keeps_the_editors_indent_without_an_override() {
        assert_eq!(format_options(indent(4, true, Some(2)), None), formatting(2, true));
        assert_eq!(format_options(indent(3, false, None), None), formatting(3, false));
    }

    fn lines_doc() -> Rope {
        Rope::from_str("fn  main(){\nlet x=1;\n}\n\nfn other() {}")
    }

    fn line_start(doc: &Rope, line: u32) -> u32 {
        buffer::line_range(doc, line - 1).start
    }

    fn change(from: u32, to: u32, insert: &str) -> TextChange {
        TextChange { from, to, insert: insert.into() }
    }

    #[test]
    fn counts_the_lines_of_the_result_the_changes_touched() {
        let doc = lines_doc();
        let (one, two) = (line_start(&doc, 1), line_start(&doc, 2));
        let changes = [change(one + 2, one + 4, " "), change(one + 10, one + 10, " "), change(two, two, "    ")];
        assert_eq!(formatted_lines(&doc, &changes), 2);
    }

    #[test]
    fn numbers_lines_as_in_the_new_text_when_a_change_adds_lines() {
        let doc = lines_doc();
        let (one, five) = (line_start(&doc, 1), line_start(&doc, 5));
        // `{}` split over two lines, then a change on (old) line 5 = new line 6.
        assert_eq!(formatted_lines(&doc, &[change(five + 11, five + 12, "{\n"), change(one, one + 1, "f")]), 3);
    }

    #[test]
    fn spots_range_formatting_done_without_the_surrounding_code() {
        assert!(dedents_to_column_zero("\n    override fun f() { }\n", "\noverride fun f() {}\n"));
        assert!(!dedents_to_column_zero("\n\tfun f() {\n\t}\n", "\n    fun f() {\n    }\n"));
        assert!(!dedents_to_column_zero("fun f() {}\n", "fun f() { }\n"));
    }

    fn guard() -> FormatGuardOptions {
        FormatGuardOptions {
            do_not_format: Vec::new(),
            formatter_tags_enabled: false,
            formatter_off_tag: "@formatter:off".into(),
            formatter_on_tag: "@formatter:on".into(),
            formatter_tags_regexp: false,
        }
    }

    #[test]
    fn does_not_enable_contributed_formatters_when_only_prettier_save_is_enabled() {
        let ask = FormatAsk {
            explicit: false,
            on_save: true,
            tool: false,
            path: "/p/a.sql",
            root: Some("/p"),
            read_only: false,
            contributed: true,
            format_on_save: false,
        };
        assert_eq!(start_format(&ask, &guard()), FormatStart::Nothing(None));
        assert_eq!(start_format(&FormatAsk { format_on_save: true, ..ask }, &guard()), FormatStart::Contributed);
        let doc = Rope::from_str("select");
        let formatted = contributed_changes(&doc, vec![change(0, 6, "SELECT")], &guard(), false);
        assert_eq!(apply_changes(&doc, &formatted.changes).to_string(), "SELECT");
    }

    #[test]
    fn formats_on_save_for_the_languages_whose_prettier_or_oxfmt_run_on_save_is_on() {
        let settings = |edit: &dyn Fn(&mut Settings)| {
            let mut settings = Settings::default();
            edit(&mut settings);
            settings
        };
        let oxfmt = settings(&|s| {
            s.languages.oxfmt.run_on_save = true;
            s.languages.oxfmt.languages = vec!["typescript".into()];
        });
        assert!(formats_on_save(Some(&oxfmt), "typescript"));
        assert!(!formats_on_save(Some(&oxfmt), "markdown"));
        assert!(formats_on_save(Some(&settings(&|s| s.languages.prettier.run_on_save = true)), "markdown"));
        let ruff = settings(&|s| s.languages.ruff.run_on_save = true);
        assert!(formats_on_save(Some(&ruff), "python"));
        assert!(!formats_on_save(Some(&ruff), "typescript"));
        assert!(!formats_on_save(Some(&settings(&|_| {})), "typescript"));
        assert!(formats_on_save(Some(&settings(&|s| s.editor.format_on_save = true)), "rust"));
        assert!(!formats_on_save(None, "typescript"));
    }
}
