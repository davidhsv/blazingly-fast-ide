//! Summaries of workspace edits for rename and refactoring previews, and the line diff an
//! intention preview shows.

use super::positions::{apply_changes, text_edits_to_changes};
use crate::editor::buffer::{self, Assoc};
use crate::platform::collate;
use ide_core::ipc_path::to_ipc;
use ide_core::{FileEdit, TextEdit, WorkspaceEdit};
use ropey::Rope;
use std::collections::HashMap;

/// A file operation of a workspace edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileOperation {
    Create,
    Rename,
    Delete,
}

/// What a workspace edit does to one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSummary {
    pub path: String,
    /// Text edits in the file.
    pub edits: usize,
    pub operation: Option<FileOperation>,
    /// The new path of a renamed file.
    pub to: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEditSummary {
    /// Distinct paths touched by text edits or file operations.
    pub files: usize,
    /// Text edits in all.
    pub occurrences: usize,
    /// Whether the edit creates, renames or deletes files.
    pub has_file_operations: bool,
    /// By path.
    pub entries: Vec<FileSummary>,
}

/// The summary of `path`, added when it is new.
fn entry<'a>(
    entries: &'a mut Vec<FileSummary>,
    by_path: &mut HashMap<String, usize>,
    path: String,
) -> &'a mut FileSummary {
    let index = *by_path.entry(path.clone()).or_insert_with(|| {
        entries.push(FileSummary { path, edits: 0, operation: None, to: None });
        entries.len() - 1
    });
    &mut entries[index]
}

pub fn summarize_workspace_edit(edit: &WorkspaceEdit) -> WorkspaceEditSummary {
    let mut entries: Vec<FileSummary> = Vec::new();
    let mut by_path: HashMap<String, usize> = HashMap::new();
    let mut occurrences = 0;
    let mut has_file_operations = false;
    for file in &edit.files {
        match file {
            FileEdit::Edit { path, edits, .. } => {
                entry(&mut entries, &mut by_path, to_ipc(path)).edits += edits.len();
                occurrences += edits.len();
            }
            FileEdit::Create { path, .. } => {
                has_file_operations = true;
                entry(&mut entries, &mut by_path, to_ipc(path)).operation = Some(FileOperation::Create);
            }
            FileEdit::Delete { path, .. } => {
                has_file_operations = true;
                entry(&mut entries, &mut by_path, to_ipc(path)).operation = Some(FileOperation::Delete);
            }
            FileEdit::Rename { from, to } => {
                has_file_operations = true;
                let summary = entry(&mut entries, &mut by_path, to_ipc(from));
                summary.operation = Some(FileOperation::Rename);
                summary.to = Some(to_ipc(to));
            }
        }
    }
    entries.retain(|entry| entry.edits > 0 || entry.operation.is_some());
    entries.sort_by(|a, b| collate::compare(&a.path, &b.path));
    WorkspaceEditSummary { files: entries.len(), occurrences, has_file_operations, entries }
}

/// "3 occurrences in 2 files".
pub fn describe_summary(summary: &WorkspaceEditSummary) -> String {
    let occurrences = if summary.occurrences == 1 { "occurrence" } else { "occurrences" };
    let files = if summary.files == 1 { "file" } else { "files" };
    format!("{} {occurrences} in {} {files}", summary.occurrences, summary.files)
}

/// Whether applying asks for confirmation first: a refactoring of several files, or one that
/// creates, renames or deletes files.
pub fn needs_preview(summary: &WorkspaceEditSummary) -> bool {
    summary.files > 1 || summary.has_file_operations
}

/// A line of a preview diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
    /// Unchanged lines left out between hunks.
    Gap,
}

/// Line diff of `a` to `b` by longest common subsequence (both are the changed lines of one
/// hunk, so short; past a size the old lines are listed removed and the new ones added).
fn line_diff(a: &[String], b: &[String]) -> Vec<DiffLine> {
    let (n, m) = (a.len(), b.len());
    if n * m > 250_000 {
        return a.iter().cloned().map(DiffLine::Removed).chain(b.iter().cloned().map(DiffLine::Added)).collect();
    }
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push(DiffLine::Context(a[i].clone()));
            i += 1;
            j += 1;
        } else if i < n && (j >= m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            out.push(DiffLine::Removed(a[i].clone()));
            i += 1;
        } else {
            out.push(DiffLine::Added(b[j].clone()));
            j += 1;
        }
    }
    out
}

fn lines(doc: &Rope, first: u32, last: u32) -> Vec<String> {
    (first..=last).map(|line| buffer::line_text(doc, line)).collect()
}

/// The preview of `edits` to `doc`: the changed lines with `context` unchanged lines around each
/// hunk, hunks separated by a gap, at most `max_lines` lines (then a gap).
pub fn edit_preview_lines(doc: &Rope, edits: &[TextEdit], context: u32, max_lines: usize) -> Vec<DiffLine> {
    let changes = text_edits_to_changes(doc, edits);
    if changes.is_empty() {
        return Vec::new();
    }
    let after = apply_changes(doc, &changes);
    // Hunks: changed old-line ranges, merged when their contexts touch.
    let mut hunks: Vec<(u32, u32)> = Vec::new();
    for change in &changes {
        let first = buffer::line_col(doc, change.from).0;
        let last = buffer::line_col(doc, change.to).0;
        match hunks.last_mut() {
            Some(hunk) if first.saturating_sub(hunk.1) <= 2 * context + 1 => hunk.1 = hunk.1.max(last),
            _ => hunks.push((first, last)),
        }
    }
    let line_count = buffer::line_count(doc);
    let mut out = Vec::new();
    for (first, last) in hunks {
        if !out.is_empty() {
            out.push(DiffLine::Gap);
        }
        let from = buffer::line_range(doc, first).start;
        let to = buffer::line_range(doc, last).end;
        let new_first = buffer::line_col(&after, buffer::map_offset(from, &changes, Assoc::Before)).0;
        let new_last = buffer::line_col(&after, buffer::map_offset(to, &changes, Assoc::After)).0;
        let context_first = first.saturating_sub(context);
        let context_last = (last + context).min(line_count - 1);
        if context_first < first {
            out.extend(lines(doc, context_first, first - 1).into_iter().map(DiffLine::Context));
        }
        out.extend(line_diff(&lines(doc, first, last), &lines(&after, new_first, new_last)));
        if last < context_last {
            out.extend(lines(doc, last + 1, context_last).into_iter().map(DiffLine::Context));
        }
        if out.len() >= max_lines {
            break;
        }
    }
    if out.len() > max_lines {
        out.truncate(max_lines);
        out.push(DiffLine::Gap);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Pos, Range};

    fn edit(line: u32) -> TextEdit {
        TextEdit { range: Range::new(Pos::new(line, 0), Pos::new(line, 3)), new_text: "new".into() }
    }

    fn file(path: &str, edits: Vec<TextEdit>) -> FileEdit {
        FileEdit::Edit { path: path.into(), edits, version: None }
    }

    fn workspace(files: Vec<FileEdit>) -> WorkspaceEdit {
        WorkspaceEdit { files, preconditions: Vec::new() }
    }

    #[test]
    fn counts_files_and_occurrences() {
        let summary = summarize_workspace_edit(&workspace(vec![
            file("/p/b.rs", vec![edit(1), edit(2)]),
            file("/p/a.rs", vec![edit(4)]),
            file("/p/b.rs", vec![edit(9)]),
            file("/p/empty.rs", Vec::new()),
        ]));
        assert_eq!(summary.files, 2);
        assert_eq!(summary.occurrences, 4);
        assert!(!summary.has_file_operations);
        let entries: Vec<(&str, usize)> = summary.entries.iter().map(|e| (e.path.as_str(), e.edits)).collect();
        assert_eq!(entries, [("/p/a.rs", 1), ("/p/b.rs", 3)]);
        assert_eq!(describe_summary(&summary), "4 occurrences in 2 files");
        assert!(needs_preview(&summary));
    }

    #[test]
    fn previews_file_operations_even_in_a_single_file() {
        let summary = summarize_workspace_edit(&workspace(vec![
            file("/p/a.ts", vec![edit(0)]),
            FileEdit::Rename { from: "/p/a.ts".into(), to: "/p/b.ts".into() },
        ]));
        assert_eq!(summary.files, 1);
        let entry = &summary.entries[0];
        assert_eq!(
            (entry.operation, entry.to.as_deref(), entry.edits),
            (Some(FileOperation::Rename), Some("/p/b.ts"), 1)
        );
        assert!(needs_preview(&summary));
        assert_eq!(describe_summary(&summary), "1 occurrence in 1 file");
    }

    #[test]
    fn applies_single_file_renames_directly() {
        let summary = summarize_workspace_edit(&workspace(vec![file("/p/a.ts", vec![edit(0), edit(3)])]));
        assert!(!needs_preview(&summary));
    }

    #[test]
    fn handles_empty_edits() {
        let summary = summarize_workspace_edit(&workspace(Vec::new()));
        assert_eq!((summary.files, summary.occurrences), (0, 0));
    }

    fn preview_doc() -> Rope {
        Rope::from_str("function f() {\n  return price * 2;\n}\n\nconst a = 1;\nconst b = 2;\nconst c = 3;")
    }

    fn at(line: u32, col: u32) -> Pos {
        Pos::new(line, col)
    }

    #[test]
    fn shows_changed_lines_with_one_line_of_context_hunks_separated() {
        let lines = edit_preview_lines(
            &preview_doc(),
            &[
                TextEdit { range: Range::new(at(1, 2), at(1, 2)), new_text: "const x = price * 2;\n  ".into() },
                TextEdit { range: Range::new(at(1, 9), at(1, 18)), new_text: "x".into() },
                TextEdit { range: Range::new(at(6, 10), at(6, 11)), new_text: "4".into() },
            ],
            1,
            60,
        );
        let text = |s: &str| s.to_owned();
        assert_eq!(
            lines,
            [
                DiffLine::Context(text("function f() {")),
                DiffLine::Removed(text("  return price * 2;")),
                DiffLine::Added(text("  const x = price * 2;")),
                DiffLine::Added(text("  return x;")),
                DiffLine::Context(text("}")),
                DiffLine::Gap,
                DiffLine::Context(text("const b = 2;")),
                DiffLine::Removed(text("const c = 3;")),
                DiffLine::Added(text("const c = 4;")),
            ]
        );
    }

    #[test]
    fn is_empty_when_nothing_changes() {
        let unchanged = TextEdit { range: Range::new(at(0, 0), at(0, 8)), new_text: "function".into() };
        assert!(edit_preview_lines(&preview_doc(), &[unchanged], 1, 60).is_empty());
    }
}
