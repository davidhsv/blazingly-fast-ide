//! The Find tool window's results tree: files with their matching lines, flat or grouped by directory.

use super::ipc::{FileMatches, Pos};
use super::replace_flow::UsageGroup;
use super::row_selection::Keyed;
use super::session::{count_file_matches, line_preview};
use crate::everywhere::open::rel_dir;
use crate::platform::collate;
use crate::platform::virtual_docs::basename;
use std::collections::{BTreeSet, HashMap};

/// A row of the results tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Dir {
        key: String,
        /// The directory relative to the project; `/` for the project root.
        label: String,
        /// Matches in the files below.
        count: u32,
        children: Vec<Node>,
    },
    File {
        key: String,
        path: String,
        name: String,
        /// Directory of the file relative to the project ("" for the root).
        dir: String,
        count: u32,
        children: Vec<Node>,
    },
    Match {
        key: String,
        path: String,
        line: u32,
        /// First match column in the real line.
        col: u32,
        end_col: u32,
        /// The line's text, white space trimmed at both ends.
        text: String,
        /// Column of `text` in the real line.
        text_col: u32,
        /// Highlight ranges within `text`.
        ranges: Vec<(u32, u32)>,
        /// Start of every match on the line.
        starts: Vec<Pos>,
    },
}

impl Node {
    pub fn children(&self) -> &[Node] {
        match self {
            Self::Dir { children, .. } | Self::File { children, .. } => children,
            Self::Match { .. } => &[],
        }
    }

    /// Matches below a directory or file node.
    pub fn count(&self) -> Option<u32> {
        match self {
            Self::Dir { count, .. } | Self::File { count, .. } => Some(*count),
            Self::Match { .. } => None,
        }
    }
}

impl Keyed for Node {
    fn key(&self) -> &str {
        match self {
            Self::Dir { key, .. } | Self::File { key, .. } | Self::Match { key, .. } => key,
        }
    }
}

fn file_node(file: &FileMatches) -> Node {
    let children = file.matches.iter().map(|line| {
        let preview = line_preview(line);
        Node::Match {
            key: format!("m:{}:{}", file.path, line.line),
            path: file.path.clone(),
            line: line.line,
            col: preview.col,
            end_col: preview.end_col,
            text: preview.text,
            text_col: preview.text_col,
            ranges: preview.ranges,
            starts: preview.starts,
        }
    });
    Node::File {
        key: format!("f:{}", file.path),
        path: file.path.clone(),
        name: basename(&file.rel_path).into_owned(),
        dir: rel_dir(&file.rel_path).to_owned(),
        count: count_file_matches(file),
        children: children.collect(),
    }
}

/// Streamed file matches as file → line nodes: flat and sorted by path, or under their directory with
/// `group_by_directory`.
pub fn build_find_tree(files: &[FileMatches], group_by_directory: bool) -> Vec<Node> {
    if !group_by_directory {
        let mut flat: Vec<(String, Node)> = files
            .iter()
            .map(|file| {
                let (dir, name) = (rel_dir(&file.rel_path), basename(&file.rel_path));
                (if dir.is_empty() { name.into_owned() } else { format!("{dir}/{name}") }, file_node(file))
            })
            .collect();
        flat.sort_by(|a, b| collate::compare(&a.0, &b.0));
        return flat.into_iter().map(|(_, node)| node).collect();
    }
    // Directories in the order their first file arrived, before they are sorted by label.
    let mut dirs: Vec<(String, u32, Vec<Node>)> = Vec::new();
    let mut index: HashMap<&str, usize> = HashMap::new();
    for file in files {
        let dir = rel_dir(&file.rel_path);
        let at = *index.entry(dir).or_insert_with(|| {
            dirs.push((dir.to_owned(), 0, Vec::new()));
            dirs.len() - 1
        });
        dirs[at].1 += count_file_matches(file);
        dirs[at].2.push(file_node(file));
    }
    dirs.sort_by(|a, b| collate::compare(dir_label(&a.0), dir_label(&b.0)));
    dirs.into_iter()
        .map(|(dir, count, children)| Node::Dir {
            key: format!("d:{dir}"),
            label: dir_label(&dir).to_owned(),
            count,
            children,
        })
        .collect()
}

/// The name a directory node shows: its path relative to the project, `/` for the project root.
fn dir_label(dir: &str) -> &str {
    if dir.is_empty() { "/" } else { dir }
}

/// What the preview pane shows for a node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewTarget<'a> {
    pub path: &'a str,
    pub line: u32,
}

/// Preview target of a node: a match, or a file's first match.
pub fn preview_of(node: Option<&Node>) -> Option<PreviewTarget<'_>> {
    match node? {
        Node::Dir { .. } => None,
        Node::Match { path, line, .. } => Some(PreviewTarget { path, line: *line }),
        Node::File { path, children, .. } => match children.first() {
            Some(Node::Match { line, .. }) => Some(PreviewTarget { path, line: *line }),
            _ => None,
        },
    }
}

/// The usages under a node (itself for a match), one group per matching line.
pub fn node_groups(node: &Node) -> Vec<UsageGroup> {
    match node {
        Node::Match { key, path, starts, .. } => {
            vec![UsageGroup { key: key.clone(), path: path.clone(), starts: starts.clone() }]
        }
        Node::Dir { children, .. } | Node::File { children, .. } => children.iter().flat_map(node_groups).collect(),
    }
}

/// Keys of every node that can be expanded or collapsed.
pub fn all_expandable_keys(nodes: &[Node]) -> BTreeSet<String> {
    fn collect(nodes: &[Node], out: &mut BTreeSet<String>) {
        for node in nodes.iter().filter(|node| !matches!(node, Node::Match { .. })) {
            out.insert(node.key().to_owned());
            collect(node.children(), out);
        }
    }
    let mut out = BTreeSet::new();
    collect(nodes, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::super::ipc::LineMatch;
    use super::*;

    fn file(rel: &str) -> FileMatches {
        let line = LineMatch { line: 0, text: "foo".to_owned(), ranges: vec![(0, 3)], text_offset: 0 };
        FileMatches { path: format!("/p/{rel}"), rel_path: rel.to_owned(), matches: vec![line] }
    }

    fn files() -> Vec<FileMatches> {
        vec![file("src/b.ts"), file("a.ts"), file("src/a.ts")]
    }

    // src/features/search/__tests__/findWindow.test.ts › buildFindTree › lists files flat by path by default
    #[test]
    fn lists_files_flat_by_path_by_default() {
        let roots = build_find_tree(&files(), false);
        assert!(roots.iter().all(|node| matches!(node, Node::File { .. })));
        let keys: Vec<&str> = roots.iter().map(Keyed::key).collect();
        assert_eq!(keys, ["f:/p/a.ts", "f:/p/src/a.ts", "f:/p/src/b.ts"]);
    }

    // src/features/search/__tests__/findWindow.test.ts › buildFindTree › groups files by directory on request
    #[test]
    fn groups_files_by_directory_on_request() {
        let roots = build_find_tree(&files(), true);
        let dirs: Vec<(&str, u32)> = roots
            .iter()
            .map(|node| match node {
                Node::Dir { label, count, .. } => (label.as_str(), *count),
                other => panic!("a directory node, not {other:?}"),
            })
            .collect();
        assert_eq!(dirs, [("/", 1), ("src", 2)]);
    }

    // Not from a spec: what a node previews, which nodes expand and the usages below a directory.
    #[test]
    fn nodes_know_their_preview_usages_and_expandable_keys() {
        let roots = build_find_tree(&files(), true);
        let src = &roots[1];
        assert_eq!(preview_of(Some(src)), None);
        assert_eq!(preview_of(None), None);
        let first = &src.children()[0];
        assert_eq!(preview_of(Some(first)), Some(PreviewTarget { path: "/p/src/b.ts", line: 0 }));
        assert_eq!(preview_of(first.children().first()), Some(PreviewTarget { path: "/p/src/b.ts", line: 0 }));
        let groups = node_groups(src);
        let group_keys: Vec<&str> = groups.iter().map(|group| group.key.as_str()).collect();
        assert_eq!(group_keys, ["m:/p/src/b.ts:0", "m:/p/src/a.ts:0"]);
        assert_eq!(groups[0].starts, [Pos::new(0, 0)]);
        let expandable: Vec<String> = all_expandable_keys(&roots).into_iter().collect();
        assert_eq!(expandable, ["d:", "d:src", "f:/p/a.ts", "f:/p/src/a.ts", "f:/p/src/b.ts"]);
        assert_eq!((src.count(), first.children()[0].count()), (Some(2), None));
    }
}
