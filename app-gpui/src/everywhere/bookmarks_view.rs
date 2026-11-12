//! The tree the Bookmarks view shows: the bookmark lists, in each its bookmarked folders and files, and
//! under a file its line bookmarks.

use super::bookmarks_store::{Bookmark, BookmarkData, BookmarkList, PathBookmark};
use crate::platform::collate;
use crate::platform::virtual_docs::{basename, relative_path};
use std::borrow::Cow;

/// A row of the Bookmarks tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BookmarkNode {
    List {
        key: String,
        list: BookmarkList,
        is_default: bool,
        /// Bookmarks in the list: lines, files and folders.
        count: usize,
        children: Vec<BookmarkNode>,
    },
    File {
        key: String,
        /// Id of the list the row is under.
        list: String,
        path: String,
        name: String,
        dir: String,
        /// The file itself is bookmarked (not only lines in it).
        bookmarked: bool,
        children: Vec<BookmarkNode>,
    },
    Folder {
        key: String,
        list: String,
        path: String,
        name: String,
        dir: String,
    },
    Mark {
        key: String,
        bookmark: Bookmark,
    },
}

impl BookmarkNode {
    pub fn key(&self) -> &str {
        match self {
            Self::List { key, .. } | Self::File { key, .. } | Self::Folder { key, .. } | Self::Mark { key, .. } => key,
        }
    }

    /// The rows under this one; none for folders and line bookmarks.
    pub fn children(&self) -> &[BookmarkNode] {
        match self {
            Self::List { children, .. } | Self::File { children, .. } => children,
            Self::Folder { .. } | Self::Mark { .. } => &[],
        }
    }
}

/// The name of `path` and the folder it is shown in, relative to the project at `root`.
fn location(path: &str, root: Option<&str>) -> (String, String) {
    let rel = match root.filter(|root| !root.is_empty()) {
        Some(root) => relative_path(root, path),
        None => Cow::Borrowed(path),
    };
    let name = basename(&rel);
    let dir = rel.strip_suffix(&*name).map_or("", |dir| dir.strip_suffix('/').unwrap_or(dir));
    (name.to_string(), dir.to_owned())
}

/// A file of a list while its bookmarks are collected.
struct FileGroup<'a> {
    path: &'a str,
    bookmarked: bool,
    marks: Vec<&'a Bookmark>,
}

/// The group of `path`, added at the end when the file has none yet.
fn group_of<'g, 'a>(files: &'g mut Vec<FileGroup<'a>>, path: &'a str) -> &'g mut FileGroup<'a> {
    let at = files.iter().position(|file| file.path == path).unwrap_or_else(|| {
        files.push(FileGroup { path, bookmarked: false, marks: Vec::new() });
        files.len() - 1
    });
    &mut files[at]
}

/// One list's nodes: its folders, then its files (bookmarked files and files with line bookmarks), each
/// by path; under a file its line bookmarks in the order given. `list_id` goes into the keys, so the
/// same file can show under several lists.
pub fn group_bookmarks<'a>(
    items: impl IntoIterator<Item = &'a Bookmark>,
    paths: impl IntoIterator<Item = &'a PathBookmark>,
    list_id: &str,
    root: Option<&str>,
) -> Vec<BookmarkNode> {
    let mut folders: Vec<&PathBookmark> = Vec::new();
    let mut files: Vec<FileGroup<'a>> = Vec::new();
    for bookmark in paths {
        if bookmark.dir {
            folders.push(bookmark);
        } else {
            group_of(&mut files, &bookmark.path).bookmarked = true;
        }
    }
    for bookmark in items {
        group_of(&mut files, &bookmark.path).marks.push(bookmark);
    }
    folders.sort_by(|a, b| collate::compare(&a.path, &b.path));
    files.sort_by(|a, b| collate::compare(a.path, b.path));

    let folders = folders.into_iter().map(|folder| {
        let (name, dir) = location(&folder.path, root);
        BookmarkNode::Folder {
            key: format!("d:{list_id}:{}", folder.path),
            list: list_id.to_owned(),
            path: folder.path.clone(),
            name,
            dir,
        }
    });
    let files = files.into_iter().map(|file| {
        let (name, dir) = location(file.path, root);
        let marks = file.marks.into_iter().map(|bookmark| BookmarkNode::Mark {
            key: format!("b:{}:{}", bookmark.path, bookmark.line),
            bookmark: bookmark.clone(),
        });
        BookmarkNode::File {
            key: format!("f:{list_id}:{}", file.path),
            list: list_id.to_owned(),
            path: file.path.to_owned(),
            name,
            dir,
            bookmarked: file.bookmarked,
            children: marks.collect(),
        }
    });
    folders.chain(files).collect()
}

/// The whole tree: one node per bookmark list, in the lists' order, with the bookmarks kept in it.
pub fn build_bookmark_tree(data: &BookmarkData, root: Option<&str>) -> Vec<BookmarkNode> {
    data.lists
        .iter()
        .map(|list| {
            let items: Vec<&Bookmark> =
                data.items.iter().filter(|bookmark| bookmark.list.as_deref() == Some(list.id.as_str())).collect();
            let paths: Vec<&PathBookmark> = data.paths.iter().filter(|bookmark| bookmark.list == list.id).collect();
            BookmarkNode::List {
                key: format!("l:{}", list.id),
                list: list.clone(),
                is_default: list.id == data.default_list,
                count: items.len() + paths.len(),
                children: group_bookmarks(items, paths, &list.id, root),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::bookmarks_store::{
        BookmarkRef, DEFAULT_LIST_ID, create_list, move_bookmarks, toggle_bookmark, toggle_path_bookmarks,
    };
    use super::*;

    fn data() -> BookmarkData {
        let items = toggle_bookmark(&[], "/p/a.ts", 1, "a", 1, None);
        let items = items.into_iter().map(|item| Bookmark { list: Some(DEFAULT_LIST_ID.to_owned()), ..item });
        BookmarkData { items: items.collect(), ..BookmarkData::empty("p") }
    }

    fn kinds(nodes: &[BookmarkNode]) -> Vec<&'static str> {
        nodes
            .iter()
            .map(|node| match node {
                BookmarkNode::List { .. } => "list",
                BookmarkNode::File { .. } => "file",
                BookmarkNode::Folder { .. } => "folder",
                BookmarkNode::Mark { .. } => "mark",
            })
            .collect()
    }

    #[test]
    fn groups_a_lists_folders_bookmarked_files_and_line_bookmarks() {
        let data = toggle_path_bookmarks(&data(), &[("/p/a.ts", false), ("/p/lib", true)], 1);
        let tree = build_bookmark_tree(&data, None);
        let BookmarkNode::List { is_default, count, children, .. } = &tree[0] else { panic!("lists come first") };
        assert!(*is_default);
        assert_eq!(*count, 3);
        assert_eq!(kinds(children), ["folder", "file"]);
        assert!(matches!(&children[1], BookmarkNode::File { bookmarked: true, .. }));
    }

    // Not from a spec: keys carry the list, names and folders are relative to the project, files sort by
    // path and each list shows only its own bookmarks.
    #[test]
    fn lists_show_their_own_bookmarks_relative_to_the_project() {
        let (data, review) = create_list(&data(), "Review");
        let data = BookmarkData { items: toggle_bookmark(&data.items, "/p/src/z.ts", 4, "z", 2, None), ..data };
        let data = BookmarkData { items: toggle_bookmark(&data.items, "/p/src/b.ts", 9, "b", 3, None), ..data };
        let moved = [BookmarkRef { path: "/p/src/z.ts".to_owned(), line: Some(4) }];
        let data = move_bookmarks(&data, &moved, &review);
        let tree = build_bookmark_tree(&data, Some("/p"));
        assert_eq!(tree.iter().map(BookmarkNode::key).collect::<Vec<_>>(), ["l:default", "l:list2"]);
        // The bookmark added last has no list yet and shows under none.
        let default = tree[0].children();
        assert_eq!(default.iter().map(BookmarkNode::key).collect::<Vec<_>>(), ["f:default:/p/a.ts"]);
        assert_eq!(default[0].children()[0].key(), "b:/p/a.ts:1");
        let BookmarkNode::List { count, children, is_default, .. } = &tree[1] else { panic!("a list") };
        assert_eq!((*count, *is_default), (1, false));
        let BookmarkNode::File { name, dir, bookmarked, list, .. } = &children[0] else { panic!("a file") };
        assert_eq!((name.as_str(), dir.as_str(), *bookmarked, list.as_str()), ("z.ts", "src", false, "list2"));
        let top = group_bookmarks(&data.items, &data.paths, "", None);
        let keys: Vec<&str> = top.iter().map(BookmarkNode::key).collect();
        assert_eq!(keys, ["f::/p/a.ts", "f::/p/src/b.ts", "f::/p/src/z.ts"]);
        let BookmarkNode::File { name, dir, .. } = &top[0] else { panic!("a file") };
        assert_eq!((name.as_str(), dir.as_str()), ("a.ts", "/p"));
    }
}
