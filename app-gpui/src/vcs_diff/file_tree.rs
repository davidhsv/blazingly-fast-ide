//! Directory tree of a list of files (the changes views' "Group by Directory"): directories first, then
//! files, each by name, with chains of directories that hold nothing but one directory merged into
//! one node.

use crate::platform::collate;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeNode<F> {
    /// `d:<path>` for a directory, `f:<path>` for a file.
    pub key: String,
    /// Directory label (`src/app`) or file name.
    pub name: String,
    /// The file; `None` for a directory.
    pub file: Option<F>,
    /// The entry of a directory that was listed itself, which an empty directory always is.
    pub directory: Option<F>,
    pub children: Vec<FileTreeNode<F>>,
    /// Number of files below (1 for a file).
    pub count: usize,
}

struct FileEntry<F> {
    name: String,
    rel: String,
    file: F,
}

struct Dir<F> {
    /// Subdirectories in the order they were first seen.
    dirs: Vec<(String, Dir<F>)>,
    index: HashMap<String, usize>,
    files: Vec<FileEntry<F>>,
    entry: Option<F>,
}

impl<F> Dir<F> {
    fn new() -> Self {
        Self { dirs: Vec::new(), index: HashMap::new(), files: Vec::new(), entry: None }
    }

    fn child(&mut self, name: &str) -> &mut Dir<F> {
        let at = match self.index.get(name) {
            Some(at) => *at,
            None => {
                self.index.insert(name.to_owned(), self.dirs.len());
                self.dirs.push((name.to_owned(), Dir::new()));
                self.dirs.len() - 1
            }
        };
        &mut self.dirs[at].1
    }
}

fn convert<F>(dir: Dir<F>, prefix: &str) -> Vec<FileTreeNode<F>> {
    let Dir { mut dirs, mut files, .. } = dir;
    let mut out = Vec::with_capacity(dirs.len() + files.len());
    dirs.sort_by(|(a, _), (b, _)| collate::compare(a, b));
    for (name, sub) in dirs {
        let mut path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let mut label = name;
        let mut current = sub;
        while current.entry.is_none() && current.files.is_empty() && current.dirs.len() == 1 {
            let Some((only_name, only)) = current.dirs.pop() else { break };
            label.push('/');
            label.push_str(&only_name);
            path.push('/');
            path.push_str(&only_name);
            current = only;
        }
        let directory = current.entry.take();
        let children = convert(current, &path);
        let count = children.iter().map(|child| child.count).sum();
        out.push(FileTreeNode { key: format!("d:{path}"), name: label, file: None, directory, children, count });
    }
    files.sort_by(|a, b| collate::compare(&a.name, &b.name));
    out.extend(files.into_iter().map(|f| FileTreeNode {
        key: format!("f:{}", f.rel),
        name: f.name,
        file: Some(f.file),
        directory: None,
        children: Vec::new(),
        count: 1,
    }));
    out
}

/// The tree of `files`. `rel_path` gives each one's `/`-separated path; `is_directory` tells the
/// entries that are directories themselves, which stay in the tree even when empty.
pub fn build_file_tree<F>(
    files: impl IntoIterator<Item = F>,
    rel_path: impl Fn(&F) -> String,
    is_directory: impl Fn(&F) -> bool,
) -> Vec<FileTreeNode<F>> {
    let mut root = Dir::new();
    for file in files {
        let rel = rel_path(&file);
        let mut parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
        let name = parts.pop().unwrap_or(&rel).to_owned();
        let mut dir = &mut root;
        for part in parts {
            dir = dir.child(part);
        }
        if is_directory(&file) {
            dir.child(&name).entry = Some(file);
        } else {
            dir.files.push(FileEntry { name, rel, file });
        }
    }
    convert(root, "")
}

/// Keys of every directory node, parents before their children (what "expand all" opens).
pub fn dir_keys<F>(nodes: &[FileTreeNode<F>]) -> Vec<String> {
    fn collect<F>(nodes: &[FileTreeNode<F>], out: &mut Vec<String>) {
        for node in nodes.iter().filter(|node| node.file.is_none()) {
            out.push(node.key.clone());
            collect(&node.children, out);
        }
    }
    let mut out = Vec::new();
    collect(nodes, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names<F>(nodes: &[FileTreeNode<F>]) -> Vec<&str> {
        nodes.iter().map(|node| node.name.as_str()).collect()
    }

    #[test]
    fn groups_by_directory_merges_single_child_chains_and_counts_files() {
        let files = ["src/app/a.ts", "src/app/b.ts", "README.md", "docs/x/y/z.md"];
        let tree = build_file_tree(files, |path| (*path).to_owned(), |_| false);
        assert_eq!(names(&tree), ["docs/x/y", "src/app", "README.md"]);
        assert_eq!(tree[1].count, 2);
        assert_eq!(names(&tree[1].children), ["a.ts", "b.ts"]);
        assert_eq!(dir_keys(&tree), ["d:docs/x/y", "d:src/app"]);
    }

    #[test]
    fn retains_explicit_empty_directories_and_their_entries_for_subtree_operations() {
        let entries = [("empty", true), ("src", true), ("src/nested", true), ("src/nested/a", false)];
        let tree = build_file_tree(entries, |(path, _)| (*path).to_owned(), |(_, dir)| *dir);
        assert_eq!(tree[0].name, "empty");
        assert_eq!(tree[0].directory, Some(entries[0]));
        assert!(tree[0].children.is_empty());
        assert_eq!(tree[0].count, 0);
        assert_eq!(tree[1].name, "src");
        assert_eq!(tree[1].directory, Some(entries[1]));
        assert_eq!(tree[1].children[0].name, "nested");
        assert_eq!(tree[1].children[0].directory, Some(entries[2]));
    }
}
