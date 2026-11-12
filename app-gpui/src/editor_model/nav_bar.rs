//! The navigation bar popup (⌘↑): the active file's path as segments (project › folders › file
//! › symbols at the caret) and the children listed under a segment.

use ide_core::OutlineItem;
use ide_core::ignored::IgnoredNames;
use ide_vfs::{EntryKind, FileEntry};

/// What a segment or a listed child is.
#[derive(Debug, Clone, PartialEq)]
pub enum NavKind {
    /// A folder; `root` for the project's own.
    Dir {
        root: bool,
    },
    File,
    Symbol(Box<OutlineItem>),
}

/// A segment of the bar, or a child listed under one.
#[derive(Debug, Clone, PartialEq)]
pub struct NavItem {
    pub kind: NavKind,
    pub path: String,
    pub label: String,
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) if at > 0 => &path[..at],
        _ => "/",
    }
}

fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// The segments of `path`: the folders from the project root (`root`, shown as `name`), or only
/// the parent folder outside the project (scratches), then the file and the outline chain at the
/// caret, leaving out a first symbol named like the file (`Foo.java › Foo`).
pub fn nav_segments(project: Option<(&str, &str)>, path: &str, chain: &[OutlineItem]) -> Vec<NavItem> {
    let mut out = Vec::new();
    let parent = parent_of(path);
    let inside = project
        .filter(|(root, _)| parent == *root || parent.strip_prefix(root).is_some_and(|rest| rest.starts_with('/')));
    match inside {
        Some((root, name)) => {
            out.push(NavItem { kind: NavKind::Dir { root: true }, path: root.to_owned(), label: name.to_owned() });
            let relative = parent.get(root.len() + 1..).unwrap_or_default();
            let mut dir = root.to_owned();
            for part in relative.split('/').filter(|part| !part.is_empty()) {
                dir = format!("{dir}/{part}");
                out.push(NavItem { kind: NavKind::Dir { root: false }, path: dir.clone(), label: part.to_owned() });
            }
        }
        None => {
            let name = name_of(parent);
            let label = if name.is_empty() { parent } else { name };
            out.push(NavItem { kind: NavKind::Dir { root: false }, path: parent.to_owned(), label: label.to_owned() });
        }
    }
    let file = name_of(path);
    out.push(NavItem { kind: NavKind::File, path: path.to_owned(), label: file.to_owned() });
    let stem = match file.rfind('.') {
        Some(at) if at > 0 => &file[..at],
        _ => file,
    };
    for (index, item) in chain.iter().enumerate() {
        if index == 0 && item.name == stem {
            continue;
        }
        out.push(symbol(path, item));
    }
    out
}

fn symbol(path: &str, item: &OutlineItem) -> NavItem {
    NavItem { kind: NavKind::Symbol(Box::new(item.clone())), path: path.to_owned(), label: item.name.clone() }
}

/// A folder's children without the names hidden everywhere (`files.ignoredNames`). A symlink of
/// unknown size counts as a folder.
pub fn dir_children(entries: &[FileEntry], ignored: &IgnoredNames) -> Vec<NavItem> {
    entries
        .iter()
        .filter(|entry| !ignored.is_ignored_name(&entry.name))
        .map(|entry| {
            let dir = entry.kind == EntryKind::Dir || (entry.kind == EntryKind::Symlink && entry.size == 0);
            NavItem {
                kind: if dir { NavKind::Dir { root: false } } else { NavKind::File },
                path: entry.path.to_string_lossy().into_owned(),
                label: entry.name.clone(),
            }
        })
        .collect()
}

/// The children of a file or symbol segment: its outline items, one level.
pub fn symbol_children(path: &str, items: &[OutlineItem]) -> Vec<NavItem> {
    items.iter().map(|item| symbol(path, item)).collect()
}

/// The next active segment for ← / →, clamped at the ends.
pub fn step_segment(index: usize, count: usize, delta: isize) -> usize {
    index.saturating_add_signed(delta).min(count.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::{Pos, Range, SymbolKind};

    fn sym(name: &str, children: Vec<OutlineItem>) -> OutlineItem {
        let range = Range { start: Pos { line: 0, col: 0 }, end: Pos { line: 9, col: 0 } };
        OutlineItem {
            name: name.to_owned(),
            detail: None,
            kind: SymbolKind::Class,
            range,
            selection_range: range,
            children,
            visibility: None,
        }
    }

    fn described(items: &[NavItem]) -> Vec<String> {
        items
            .iter()
            .map(|item| {
                let kind = match item.kind {
                    NavKind::Dir { .. } => "dir",
                    NavKind::File => "file",
                    NavKind::Symbol(_) => "symbol",
                };
                format!("{kind}:{}", item.label)
            })
            .collect()
    }

    #[test]
    fn lists_project_dirs_file_symbols_skipping_the_class_named_like_its_file() {
        let segments =
            nav_segments(Some(("/p", "proj")), "/p/src/a/Foo.java", &[sym("Foo", Vec::new()), sym("run", Vec::new())]);
        assert_eq!(described(&segments), ["dir:proj", "dir:src", "dir:a", "file:Foo.java", "symbol:run"]);
        assert_eq!(segments[2].path, "/p/src/a");
    }

    #[test]
    fn shows_only_the_parent_folder_outside_the_project() {
        let segments = nav_segments(Some(("/p", "proj")), "/data/scratches/scratch.py", &[]);
        let labels: Vec<&str> = segments.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["scratches", "scratch.py"]);
    }

    #[test]
    fn lists_folder_children_without_ide_hidden_names_and_symbol_children() {
        let entry = |name: &str, kind: EntryKind| FileEntry {
            name: name.to_owned(),
            path: format!("/p/{name}").into(),
            kind,
            size: 0,
            mtime_ms: 0.0,
            ignored: false,
            excluded: false,
            hidden: false,
            target_kind: None,
        };
        let entries = [entry(".git", EntryKind::Dir), entry("src", EntryKind::Dir), entry("a.ts", EntryKind::File)];
        let names: Vec<String> =
            ide_core::ignored::DEFAULT_IGNORED_NAMES.iter().map(|name| (*name).to_owned()).collect();
        let ignored = IgnoredNames::new(&names);
        assert_eq!(described(&dir_children(&entries, &ignored)), ["dir:src", "file:a.ts"]);
        let children = symbol_children("/p/a.ts", &[sym("A", vec![sym("b", Vec::new())])]);
        let labels: Vec<&str> = children.iter().map(|item| item.label.as_str()).collect();
        assert_eq!(labels, ["A"]);
    }

    #[test]
    fn clamps_left_and_right_at_the_ends() {
        assert_eq!(step_segment(0, 3, -1), 0);
        assert_eq!(step_segment(1, 3, 1), 2);
        assert_eq!(step_segment(2, 3, 1), 2);
    }
}
