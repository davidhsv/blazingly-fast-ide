//! Move / rename of files and folders as a refactoring: the language servers' reference updates
//! merged with the textual occurrences, whether the result needs a preview, where open editors go,
//! and what the user is told once it is done or undone.

use crate::platform::i18n::{I18n, Param};
use crate::project_tree_paths::{is_same_or_inside, name_of, parent_of};
use crate::project_view::operations::RefactorLabel;
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_core::ipc_path::to_ipc;
use ide_core::text::{FileEdit, Range, TextEdit};
use ide_core::{IpcPath, WorkspaceEdit};
use ide_workspace::FileMove;
use std::collections::BTreeMap;

/// The action that undoes the last file refactoring.
pub const UNDO_MOVE_ACTION: &str = "project.undoMove";
/// The notification of a finished (or undone) refactoring: a later one replaces it.
pub const DONE_KEY: &str = "project.refactor.done";
const ERRORS_KEY: &str = "project.refactor.errors";
const UNDO_KEY: &str = "project.refactor.undo";

pub fn label_text(label: RefactorLabel) -> &'static str {
    match label {
        RefactorLabel::Move => "Move",
        RefactorLabel::Rename => "Rename",
    }
}

fn overlaps(a: &Range, b: &Range) -> bool {
    !(a.end <= b.start) && !(b.end <= a.start)
}

/// The servers' edits plus the textual occurrences no server edit already covers. Only text edits
/// are kept: the move itself is the refactoring's own file operation. A file the servers edited
/// under two versions keeps an empty guard of the first one.
pub fn merge_edits(lsp: &WorkspaceEdit, text: &WorkspaceEdit) -> WorkspaceEdit {
    let mut versions: BTreeMap<String, Option<i32>> = BTreeMap::new();
    let mut guards: Vec<FileEdit> = Vec::new();
    let mut order: Vec<String> = Vec::new();
    let mut by_path: BTreeMap<String, (IpcPath, Vec<TextEdit>)> = BTreeMap::new();
    for file in &lsp.files {
        let FileEdit::Edit { path, edits, version } = file else { continue };
        let key = to_ipc(path);
        let previous = versions.get(&key).copied().flatten();
        if let (Some(previous), Some(version)) = (previous, version)
            && previous != *version
        {
            guards.push(FileEdit::Edit { path: path.clone(), edits: Vec::new(), version: Some(previous) });
        }
        versions.insert(key.clone(), version.or(previous));
        if !by_path.contains_key(&key) {
            order.push(key.clone());
        }
        by_path.entry(key).or_insert_with(|| (path.clone(), Vec::new())).1.extend(edits.iter().cloned());
    }
    for file in &text.files {
        let FileEdit::Edit { path, edits, .. } = file else { continue };
        let key = to_ipc(path);
        let existing = by_path.get(&key).map(|(_, edits)| edits.as_slice()).unwrap_or_default();
        let extra: Vec<TextEdit> =
            edits.iter().filter(|edit| !existing.iter().any(|at| overlaps(&at.range, &edit.range))).cloned().collect();
        if extra.is_empty() {
            continue;
        }
        if !by_path.contains_key(&key) {
            order.push(key.clone());
        }
        by_path.entry(key).or_insert_with(|| (path.clone(), Vec::new())).1.extend(extra);
    }
    let mut files = guards;
    for key in order {
        let Some((path, edits)) = by_path.remove(&key) else { continue };
        if !edits.is_empty() {
            files.push(FileEdit::Edit { path, edits, version: versions.get(&key).copied().flatten() });
        }
    }
    let mut preconditions = lsp.preconditions.clone();
    preconditions.extend(text.preconditions.iter().cloned());
    WorkspaceEdit { files, preconditions }
}

/// Whether `edit` changes files other than the moved ones: such a refactoring is previewed first.
pub fn touches_other_files(edit: &WorkspaceEdit, moves: &[FileMove]) -> bool {
    edit.files.iter().any(|file| match file {
        FileEdit::Edit { path, edits, .. } => {
            let path = to_ipc(path);
            !edits.is_empty() && !moves.iter().any(|moved| is_same_or_inside(&path, &to_ipc(&moved.from)))
        }
        _ => false,
    })
}

/// Where `path` is after `moves`.
pub fn moved_path(path: &str, moves: &[FileMove]) -> String {
    let mut at = path.to_owned();
    for moved in moves {
        let (from, to) = (to_ipc(&moved.from), to_ipc(&moved.to));
        if is_same_or_inside(&at, &from) {
            at = format!("{to}{}", &at[from.len()..]);
        }
    }
    at
}

/// The refactoring's title: "Rename 'a' to 'b'", "Move 'a' to 'dir'", "Move 3 items".
pub fn describe_moves(moves: &[FileMove], label: RefactorLabel) -> String {
    let [only] = moves else { return format!("{} {} items", label_text(label), moves.len()) };
    let (from, to) = (to_ipc(&only.from), to_ipc(&only.to));
    if parent_of(&from) == parent_of(&to) {
        format!("Rename '{}' to '{}'", name_of(&from), name_of(&to))
    } else {
        format!("Move '{}' to '{}'", name_of(&from), name_of(parent_of(&to)))
    }
}

/// The move itself failed.
pub fn failed_notification(moves: &[FileMove], label: RefactorLabel, message: &str) -> NotificationRequest {
    let verb = match label {
        RefactorLabel::Move => "Cannot move",
        RefactorLabel::Rename => "Cannot rename",
    };
    let what = match moves {
        [only] => name_of(&to_ipc(&only.from)).to_owned(),
        _ => format!("{} items", moves.len()),
    };
    NotificationRequest {
        body: Some(message.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Error, format!("{verb} {what}"))
    }
}

/// Servers that could not answer: their references may be stale.
pub fn errors_notification(errors: &[String]) -> Option<NotificationRequest> {
    (!errors.is_empty()).then(|| NotificationRequest {
        body: Some(errors.join("\n")),
        key: Some(ERRORS_KEY.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Warning, "Some references may not have been updated")
    })
}

/// The refactoring `title` is done, having edited `edited` files; its Undo undoes it.
pub fn done_notification(i18n: &mut I18n, title: &str, edited: usize) -> NotificationRequest {
    let body = match edited {
        0 => None,
        1 => Some(i18n.tx_built(&["Updated references in ", " file"], &[Param::from(edited)])),
        _ => Some(i18n.tx_built(&["Updated references in ", " files"], &[Param::from(edited)])),
    };
    let undo = NotificationAction { label: i18n.tx("Undo"), command: UNDO_MOVE_ACTION.to_owned() };
    NotificationRequest {
        body,
        actions: vec![undo],
        key: Some(DONE_KEY.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Info, title)
    }
}

pub fn undo_failed_notification(message: &str) -> NotificationRequest {
    NotificationRequest {
        body: Some(message.to_owned()),
        key: Some(UNDO_KEY.to_owned()),
        ..NotificationRequest::new(NotificationLevel::Error, "Cannot undo")
    }
}

/// The refactoring was undone; `conflicts` are the files changed since, whose edits were kept.
pub fn undone_notification(i18n: &mut I18n, conflicts: &[IpcPath]) -> NotificationRequest {
    let names: Vec<String> = conflicts.iter().map(|path| name_of(&to_ipc(path)).to_owned()).collect();
    let (level, body) = if names.is_empty() {
        (NotificationLevel::Info, None)
    } else {
        let body =
            i18n.tx_built(&["Changed since the refactoring, not reverted: ", ""], &[Param::from(names.join(", "))]);
        (NotificationLevel::Warning, Some(body))
    };
    NotificationRequest {
        body,
        key: Some(DONE_KEY.to_owned()),
        ..NotificationRequest::new(level, "Refactoring undone")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::text::{Pos, WorkspaceEditPrecondition};

    fn path(text: &str) -> IpcPath {
        text.to_owned().into()
    }

    fn edit(line: u32, from: u32, to: u32, text: &str) -> TextEdit {
        let range = Range { start: Pos { line, col: from }, end: Pos { line, col: to } };
        TextEdit { range, new_text: text.to_owned() }
    }

    fn file(at: &str, edits: Vec<TextEdit>, version: Option<i32>) -> FileEdit {
        FileEdit::Edit { path: path(at), edits, version }
    }

    fn moved(from: &str, to: &str) -> FileMove {
        FileMove { from: path(from), to: path(to) }
    }

    fn precondition(at: &str, hash: &str) -> WorkspaceEditPrecondition {
        WorkspaceEditPrecondition { path: path(at), hash: hash.to_owned() }
    }

    // src/features/project-tree/fileRefactor.test.tsx › file refactoring edits › retains version guards and both
    // sources' preview fingerprints
    #[test]
    fn retains_version_guards_and_both_sources_preview_fingerprints() {
        let lsp = WorkspaceEdit {
            files: vec![file("/p/main.ts", vec![edit(0, 0, 3, "new")], Some(7))],
            preconditions: vec![precondition("/p/main.ts", "source")],
        };
        let text = WorkspaceEdit { files: Vec::new(), preconditions: vec![precondition("/p/README.md", "text")] };
        let merged = merge_edits(&lsp, &text);
        assert!(matches!(&merged.files[0], FileEdit::Edit { version: Some(7), .. }));
        assert_eq!(merged.preconditions, [precondition("/p/main.ts", "source"), precondition("/p/README.md", "text")]);
    }

    // src/features/project-tree/fileRefactor.test.tsx › file refactoring edits › merges text occurrences that no
    // server edit covers and drops file operations
    #[test]
    fn merges_text_occurrences_that_no_server_edit_covers_and_drops_file_operations() {
        let lsp = WorkspaceEdit {
            files: vec![
                file("/p/main.ts", vec![edit(0, 20, 26, "./b/util")], None),
                FileEdit::Rename { from: path("/p/a"), to: path("/p/b") },
            ],
            ..Default::default()
        };
        let text = WorkspaceEdit {
            files: vec![
                file("/p/main.ts", vec![edit(0, 22, 26, "b/util"), edit(3, 3, 10, "b/util.ts")], None),
                file("/p/README.md", vec![edit(1, 0, 9, "b/util.ts")], None),
            ],
            ..Default::default()
        };
        let merged = merge_edits(&lsp, &text);
        let counts: Vec<(String, usize)> = merged
            .files
            .iter()
            .filter_map(|file| match file {
                FileEdit::Edit { path, edits, .. } => Some((to_ipc(path), edits.len())),
                _ => None,
            })
            .collect();
        assert_eq!(counts, [("/p/main.ts".to_owned(), 2), ("/p/README.md".to_owned(), 1)]);
        assert_eq!(merged.files.len(), 2);
    }

    // src/features/project-tree/fileRefactor.test.tsx › file refactoring edits › knows when other files change and
    // where paths move
    #[test]
    fn knows_when_other_files_change_and_where_paths_move() {
        let moves = [moved("/p/a", "/p/b/a")];
        let inside =
            WorkspaceEdit { files: vec![file("/p/a/x.ts", vec![edit(0, 0, 1, "y")], None)], ..Default::default() };
        assert!(!touches_other_files(&inside, &moves));
        let other =
            WorkspaceEdit { files: vec![file("/p/main.ts", vec![edit(0, 0, 1, "y")], None)], ..Default::default() };
        assert!(touches_other_files(&other, &moves));
        assert_eq!(moved_path("/p/a/x.ts", &moves), "/p/b/a/x.ts");
        assert_eq!(moved_path("/p/ab.ts", &moves), "/p/ab.ts");
    }

    // Not from a spec: a file the servers edited under two versions keeps an empty guard of the first; a file
    // without edits is left out.
    #[test]
    fn guards_a_file_the_servers_edited_under_two_versions() {
        let lsp = WorkspaceEdit {
            files: vec![
                file("/p/a.ts", vec![edit(0, 0, 1, "x")], Some(1)),
                file("/p/a.ts", vec![edit(2, 0, 1, "y")], Some(2)),
                file("/p/b.ts", Vec::new(), None),
            ],
            ..Default::default()
        };
        let merged = merge_edits(&lsp, &WorkspaceEdit::default());
        assert_eq!(
            merged.files,
            [
                file("/p/a.ts", Vec::new(), Some(1)),
                file("/p/a.ts", vec![edit(0, 0, 1, "x"), edit(2, 0, 1, "y")], Some(2))
            ]
        );
    }

    // Not from a spec: the titles and notifications of a run.
    #[test]
    fn describes_the_refactoring_and_what_it_did() {
        let rename = [moved("/p/a.ts", "/p/b.ts")];
        let to_dir = [moved("/p/a.ts", "/p/lib/a.ts")];
        assert_eq!(describe_moves(&rename, RefactorLabel::Rename), "Rename 'a.ts' to 'b.ts'");
        assert_eq!(describe_moves(&to_dir, RefactorLabel::Move), "Move 'a.ts' to 'lib'");
        let both = [rename[0].clone(), to_dir[0].clone()];
        assert_eq!(describe_moves(&both, RefactorLabel::Move), "Move 2 items");
        assert_eq!(failed_notification(&rename, RefactorLabel::Rename, "busy").title, "Cannot rename a.ts");
        assert_eq!(failed_notification(&both, RefactorLabel::Move, "busy").title, "Cannot move 2 items");
        let mut i18n = I18n::default();
        let done = done_notification(&mut i18n, "Rename 'a.ts' to 'b.ts'", 2);
        assert_eq!(done.body.as_deref(), Some("Updated references in 2 files"));
        assert_eq!(done.actions[0].command, UNDO_MOVE_ACTION);
        assert_eq!(done_notification(&mut i18n, "t", 1).body.as_deref(), Some("Updated references in 1 file"));
        assert_eq!(done_notification(&mut i18n, "t", 0).body, None);
        assert!(errors_notification(&[]).is_none());
        let undone = undone_notification(&mut i18n, &[path("/p/b.ts")]);
        let expected = Some("Changed since the refactoring, not reverted: b.ts");
        assert_eq!((undone.level, undone.body.as_deref()), (NotificationLevel::Warning, expected));
    }
}
