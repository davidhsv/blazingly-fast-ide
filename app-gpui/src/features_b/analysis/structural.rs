//! Search / Replace Structurally: matches go to the Find Usages tool window; replacements are previewed,
//! each match included or excluded, and applied as one workspace edit.

use crate::find::utf16;
use crate::platform::i18n::{I18n, Param};
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::{FileEdit, IpcPath, Location, Range, TextEdit, WorkspaceEdit};
use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;
use std::sync::LazyLock;

/// A match of a structural search (the backend's `StructuralMatch`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralMatch {
    pub range: Range,
    /// The matched code (capped).
    pub text: String,
    /// The first line of the match, for result rows.
    pub line_text: String,
    /// Set when the query has a replacement template.
    pub replacement: Option<String>,
}

/// All matches in one file (the backend's `StructuralFile`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralFile {
    pub path: String,
    pub rel_path: String,
    pub matches: Vec<StructuralMatch>,
}

/// The end of a search (the backend's `StructuralSummary`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructuralSummary {
    pub files: u32,
    pub matches: u32,
    pub limit_hit: bool,
    pub cancelled: bool,
    pub elapsed_ms: u32,
}

/// Key of one match, for including / excluding it in the replace preview.
pub fn match_key(path: &str, index: usize) -> String {
    format!("{path}\u{0}{index}")
}

/// Locations of all matches (for the Find Usages view).
pub fn match_locations(files: &[StructuralFile]) -> Vec<Location> {
    files
        .iter()
        .flat_map(|file| {
            file.matches.iter().map(|m| Location { path: IpcPath::from(file.path.as_str()), range: m.range, uri: None })
        })
        .collect()
}

/// One text edit per included match with a replacement; files without one are left out.
pub fn build_replace_edit(files: &[StructuralFile], excluded: &HashSet<String>) -> WorkspaceEdit {
    let files = files
        .iter()
        .filter_map(|file| {
            let edits: Vec<TextEdit> = file
                .matches
                .iter()
                .enumerate()
                .filter(|(i, _)| !excluded.contains(&match_key(&file.path, *i)))
                .filter_map(|(_, m)| m.replacement.clone().map(|new_text| TextEdit { range: m.range, new_text }))
                .collect();
            let path = IpcPath::from(file.path.as_str());
            (!edits.is_empty()).then_some(FileEdit::Edit { path, edits, version: None })
        })
        .collect();
    WorkspaceEdit { files, preconditions: Vec::new() }
}

/// The search's files by relative path, as results are listed.
pub fn sort_files(files: &mut [StructuralFile]) {
    files.sort_by(|a, b| crate::platform::collate::compare(&a.rel_path, &b.rel_path));
}

static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("the pattern is valid"));

/// The Find Usages title of a search: the pattern on one line, at most 60 characters.
pub fn usage_symbol(pattern: &str) -> String {
    let label = WHITESPACE.replace_all(pattern, " ");
    let label = label.trim();
    if utf16::len(label) > 60 { format!("{}…", utf16::slice(label, 0, 59)) } else { label.to_owned() }
}

fn analysis_notification(level: NotificationLevel, title: &str, body: String) -> NotificationRequest {
    let mut request = NotificationRequest::new(level, title);
    request.group = Some("analysis".to_owned());
    request.body = Some(body);
    request
}

/// What showing a finished search does.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownResults {
    /// Run `find.showUsageResults` with this symbol and these locations.
    pub usages: Option<(String, Vec<Location>)>,
    pub notification: Option<NotificationRequest>,
}

/// The results of a finished search: in the Find Usages tool window, with a warning when the match limit
/// stopped it, or a notification that nothing was found.
pub fn show_results(
    pattern: &str,
    files: &[StructuralFile],
    summary: &StructuralSummary,
    i18n: &mut I18n,
) -> ShownResults {
    if files.is_empty() {
        let notification =
            analysis_notification(NotificationLevel::Info, "Search Structurally", "No occurrences found".to_owned());
        return ShownResults { usages: None, notification: Some(notification) };
    }
    let notification = summary.limit_hit.then(|| {
        let body = i18n.tx_built(&["Stopped after ", " matches"], &[Param::from(f64::from(summary.matches))]);
        analysis_notification(NotificationLevel::Warning, "Search Structurally", body)
    });
    ShownResults { usages: Some((usage_symbol(pattern), match_locations(files))), notification }
}

/// The search or the replacement failed.
pub fn failed(title: &str, message: String) -> NotificationRequest {
    analysis_notification(NotificationLevel::Error, title, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Pos;

    fn found(line: u32, from: u32, to: u32, text: &str, replacement: &str) -> StructuralMatch {
        StructuralMatch {
            range: Range::new(Pos::new(line, from), Pos::new(line, to)),
            text: text.to_owned(),
            line_text: String::new(),
            replacement: Some(replacement.to_owned()),
        }
    }

    fn files() -> Vec<StructuralFile> {
        vec![
            StructuralFile {
                path: "/p/a.rs".to_owned(),
                rel_path: "a.rs".to_owned(),
                matches: vec![found(1, 4, 14, "x.unwrap()", "x?"), found(2, 4, 14, "y.unwrap()", "y?")],
            },
            StructuralFile {
                path: "/p/b.rs".to_owned(),
                rel_path: "b.rs".to_owned(),
                matches: vec![found(0, 0, 10, "z.unwrap()", "z?")],
            },
        ]
    }

    #[test]
    fn turns_included_matches_into_one_workspace_edit() {
        let files = files();
        assert_eq!(build_replace_edit(&files, &HashSet::new()).files.len(), 2);
        let excluded = HashSet::from([match_key("/p/a.rs", 0), match_key("/p/b.rs", 0)]);
        assert_eq!(
            build_replace_edit(&files, &excluded),
            WorkspaceEdit {
                files: vec![FileEdit::Edit {
                    path: IpcPath::from("/p/a.rs"),
                    edits: vec![TextEdit { range: files[0].matches[1].range, new_text: "y?".to_owned() }],
                    version: None,
                }],
                preconditions: Vec::new(),
            }
        );
    }

    #[test]
    fn lists_match_locations_for_the_usages_view() {
        let locations: Vec<(String, u32)> = match_locations(&files())
            .iter()
            .map(|l| (ide_core::ipc_path::to_ipc(&l.path), l.range.start.line))
            .collect();
        assert_eq!(locations, [("/p/a.rs".to_owned(), 1), ("/p/a.rs".to_owned(), 2), ("/p/b.rs".to_owned(), 0)]);
    }

    #[test]
    fn shows_matches_as_usages_and_warns_when_the_limit_stopped_the_search() {
        let mut i18n = I18n::default();
        let summary = StructuralSummary { matches: 3, limit_hit: true, ..StructuralSummary::default() };
        let shown = show_results("foo(\n   $x$ )", &files(), &summary, &mut i18n);
        assert_eq!(shown.usages.as_ref().map(|(symbol, l)| (symbol.as_str(), l.len())), Some(("foo( $x$ )", 3)));
        assert_eq!(shown.notification.and_then(|n| n.body).as_deref(), Some("Stopped after 3 matches"));
        let none = show_results("x", &[], &StructuralSummary::default(), &mut i18n);
        assert_eq!(none.notification.and_then(|n| n.body).as_deref(), Some("No occurrences found"));
        assert_eq!(usage_symbol(&"a".repeat(70)), format!("{}…", "a".repeat(59)));
    }
}
