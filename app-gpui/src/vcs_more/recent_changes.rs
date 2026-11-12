//! View › Recent Changes: the project's latest Local History events (labels, system events and
//! saved file revisions) in a popup; choosing one opens its Local History.

use crate::platform::virtual_docs::relative_path;
use ide_workspace::history::{HistoryEntry, HistoryKind};

/// How many rows the popup lists.
pub const MAX_ROWS: usize = 40;

fn path_of(entry: &HistoryEntry) -> String {
    ide_core::ipc_path::to_ipc(&entry.path.0)
}

/// The newest `limit` events, newest first; consecutive revisions of one file with the same label
/// collapse into the newest (typing saves often).
pub fn recent_changes(entries: &[HistoryEntry], limit: usize) -> Vec<HistoryEntry> {
    let mut sorted: Vec<&HistoryEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| b.timestamp_ms.total_cmp(&a.timestamp_ms));
    let mut out: Vec<HistoryEntry> = Vec::new();
    for entry in sorted {
        if out.len() >= limit {
            break;
        }
        let repeats = out.last().is_some_and(|previous| {
            previous.kind != HistoryKind::Label
                && entry.kind != HistoryKind::Label
                && previous.path == entry.path
                && previous.label == entry.label
        });
        if !repeats {
            out.push(entry.clone());
        }
    }
    out
}

/// A row's text: a label as written, a revision as its project-relative path and what made it.
pub fn recent_change_label(entry: &HistoryEntry, root: &str) -> String {
    if entry.kind == HistoryKind::Label {
        return entry.label.clone();
    }
    let path = path_of(entry);
    let relative = relative_path(root, &path);
    if entry.label.is_empty() { relative.into_owned() } else { format!("{relative} — {}", entry.label) }
}

/// What choosing a row opens the Local History of: the project for a label, else the file.
pub fn recent_change_target(entry: &HistoryEntry, root: &str) -> String {
    if entry.kind == HistoryKind::Label { root.to_owned() } else { path_of(entry) }
}

/// The icon of a row.
pub fn recent_change_icon(entry: &HistoryEntry) -> &'static str {
    if entry.kind == HistoryKind::Label { "bookmark" } else { "history" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IpcPath;
    use std::path::PathBuf;

    fn entry(path: &str, time: f64, label: &str, kind: HistoryKind) -> HistoryEntry {
        HistoryEntry {
            id: format!("{path}@{time}"),
            path: IpcPath(PathBuf::from(path)),
            timestamp_ms: time,
            label: label.to_owned(),
            size: 1,
            kind,
        }
    }

    fn saved(path: &str, time: f64) -> HistoryEntry {
        entry(path, time, "Saved", HistoryKind::Snapshot)
    }

    // src/features/vcs/__tests__/recentChanges.test.ts › View › Recent Changes › lists newest first and collapses runs of saves of one file
    #[test]
    fn lists_newest_first_and_collapses_runs_of_saves_of_one_file() {
        let entries = [
            saved("/p/a.ts", 1.0),
            saved("/p/a.ts", 3.0),
            saved("/p/a.ts", 2.0),
            entry("/p", 4.0, "Before Git checkout", HistoryKind::Label),
            saved("/p/b.ts", 5.0),
        ];
        let rows = recent_changes(&entries, MAX_ROWS);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["/p/b.ts@5", "/p@4", "/p/a.ts@3"]);
        assert_eq!(recent_changes(&[saved("/p/a", 1.0), saved("/p/b", 2.0), saved("/p/c", 3.0)], 2).len(), 2);
    }

    // src/features/vcs/__tests__/recentChanges.test.ts › View › Recent Changes › labels revisions by project-relative path
    #[test]
    fn labels_revisions_by_project_relative_path() {
        assert_eq!(recent_change_label(&saved("/p/src/a.ts", 1.0), "/p"), "src/a.ts — Saved");
        let release = entry("/p", 1.0, "Release", HistoryKind::Label);
        assert_eq!(recent_change_label(&release, "/p"), "Release");
        assert_eq!((recent_change_target(&release, "/p"), recent_change_icon(&release)), ("/p".to_owned(), "bookmark"));
    }
}
