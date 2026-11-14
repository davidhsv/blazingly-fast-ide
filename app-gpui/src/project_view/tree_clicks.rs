//! What a click on a row of the Project view and a move of its selection do, by the view options.
//!
//! A folder opens or closes on a double click, or on a single one with Expand Directories with
//! Single Click. A file opens with the keyboard on a double click; with Open Files with Single
//! Click or Enable Preview Tab a single click opens it (in the preview tab with the latter) and the
//! keyboard stays in the tree. With Autoscroll to Source the file the selection moves to opens the
//! same way. A click with a modifier only changes the selection.

use crate::project_tree::{ProjectTree, ViewOptions};
use crate::project_tree_model::ProjectNode;
use gpui_kit::EventEmitter;
use std::path::PathBuf;

/// The tree asks for a file to be opened in an editor tab: as the pane's preview tab or as a
/// regular one, and with or without giving the editor the keyboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub preview: bool,
    pub focus: bool,
}

impl EventEmitter<OpenRequest> for ProjectTree {}

/// What a click on a row does besides selecting it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowClick {
    /// Nothing more: the row is selected.
    Select,
    /// The folder opens or closes.
    Toggle,
    /// The file opens.
    Open { preview: bool, focus: bool },
}

/// Whether `node` is a folder of the tree, which a click opens or closes.
fn is_folder(node: &ProjectNode) -> bool {
    node.dir && node.placeholder.is_none()
}

/// Whether `node` is a file an editor can open. The rows of "Open Files" and "All Changed Files"
/// have no listing entry behind them, and open all the same.
fn is_file(node: &ProjectNode) -> bool {
    !node.dir && node.placeholder.is_none() && node.member.is_none() && !node.group
}

/// What the `count`th click of a burst on `node` does; `plain` when no modifier is held.
///
/// The clicks of a burst count up (1, 2, 3, …). A double click is every second one, so a folder
/// toggles once per pair of clicks; with single-click expansion it toggles on every odd click
/// instead, so the second click of a double click does not close it again.
pub fn row_click(options: &ViewOptions, node: &ProjectNode, count: usize, plain: bool) -> RowClick {
    if !plain || count == 0 {
        return RowClick::Select;
    }
    if is_folder(node) {
        let toggles = if options.single_click_expand { count % 2 == 1 } else { count % 2 == 0 };
        return if toggles { RowClick::Toggle } else { RowClick::Select };
    }
    if !is_file(node) {
        return RowClick::Select;
    }
    match count {
        2 => RowClick::Open { preview: false, focus: true },
        1 if options.single_click_open || options.preview_tab => {
            RowClick::Open { preview: !options.single_click_open, focus: false }
        }
        _ => RowClick::Select,
    }
}

/// What opens when the selection moves to `node` (a click, the arrow keys): with Autoscroll to
/// Source the file, in the preview tab with Enable Preview Tab, the keyboard staying in the tree.
pub fn selection_open(options: &ViewOptions, node: &ProjectNode) -> Option<OpenRequest> {
    (options.autoscroll_to_source && is_file(node)).then(|| OpenRequest {
        path: PathBuf::from(&node.path),
        preview: options.preview_tab,
        focus: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_vfs::{EntryKind, FileEntry};

    fn entry(path: &str, kind: EntryKind) -> FileEntry {
        FileEntry {
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            path: path.into(),
            kind,
            size: 0,
            mtime_ms: 1.0,
            ignored: false,
            excluded: false,
            hidden: false,
            target_kind: None,
        }
    }

    fn file(path: &str) -> ProjectNode {
        ProjectNode {
            key: path.to_owned(),
            path: path.to_owned(),
            label: path.rsplit('/').next().unwrap_or(path).to_owned(),
            entry: Some(entry(path, EntryKind::File)),
            ..ProjectNode::default()
        }
    }

    fn folder(path: &str) -> ProjectNode {
        ProjectNode { dir: true, entry: Some(entry(path, EntryKind::Dir)), ..file(path) }
    }

    fn options(change: impl FnOnce(&mut ViewOptions)) -> ViewOptions {
        let mut options = ViewOptions::default();
        change(&mut options);
        options
    }

    // src/features/project-tree/ProjectView.test.tsx › opens files on double click and previews on
    // single click only when enabled
    #[test]
    fn opens_files_on_double_click_and_previews_on_single_click_only_when_enabled() {
        let readme = file("/w/acme/README.md");
        let plain = ViewOptions::default();
        assert_eq!(row_click(&plain, &readme, 1, true), RowClick::Select);
        assert_eq!(row_click(&plain, &readme, 2, true), RowClick::Open { preview: false, focus: true });
        let preview = options(|options| options.preview_tab = true);
        assert_eq!(row_click(&preview, &readme, 1, true), RowClick::Open { preview: true, focus: false });
        assert_eq!(row_click(&preview, &readme, 3, true), RowClick::Select);
        let single = options(|options| options.single_click_open = true);
        assert_eq!(row_click(&single, &readme, 1, true), RowClick::Open { preview: false, focus: false });
        // A click with a modifier only changes the selection.
        assert_eq!(row_click(&single, &readme, 1, false), RowClick::Select);
    }

    // src/features/project-tree/ProjectView.test.tsx › expands and collapses folders on single
    // click only when enabled
    #[test]
    fn expands_and_collapses_folders_on_single_click_only_when_enabled() {
        let src = folder("/w/acme/src");
        assert_eq!(row_click(&ViewOptions::default(), &src, 1, true), RowClick::Select);
        let single = options(|options| options.single_click_expand = true);
        assert_eq!(row_click(&single, &src, 1, true), RowClick::Toggle);
        // A double click toggles once: its second click is left alone.
        assert_eq!(row_click(&single, &src, 2, true), RowClick::Select);
        assert_eq!(row_click(&single, &src, 1, false), RowClick::Select, "a modifier click only selects");
    }

    // src/features/project-tree/ProjectView.test.tsx › toggles folders once for each rapid
    // double-click pair
    #[test]
    fn toggles_folders_once_for_each_rapid_double_click_pair() {
        let src = folder("/w/acme/src");
        for single_click_expand in [false, true] {
            let options = options(|options| options.single_click_expand = single_click_expand);
            for clicks in [2usize, 4, 6, 3, 5] {
                let toggles = (1..=clicks).filter(|count| row_click(&options, &src, *count, true) == RowClick::Toggle);
                let expected = if single_click_expand { clicks.div_ceil(2) } else { clicks / 2 };
                assert_eq!(toggles.count(), expected, "{clicks} clicks, single click: {single_click_expand}");
            }
        }
    }

    // Not from a spec: Autoscroll to Source opens the file the selection moves to, in the preview
    // tab with Enable Preview Tab, and never a folder.
    #[test]
    fn autoscroll_to_source_opens_the_selected_file_without_the_keyboard() {
        let readme = file("/w/acme/README.md");
        assert_eq!(selection_open(&ViewOptions::default(), &readme), None);
        let autoscroll = options(|options| options.autoscroll_to_source = true);
        let opened = selection_open(&autoscroll, &readme).expect("the file opens");
        assert_eq!((opened.path, opened.preview, opened.focus), (PathBuf::from("/w/acme/README.md"), false, false));
        let previewed = options(|options| {
            options.autoscroll_to_source = true;
            options.preview_tab = true;
        });
        assert_eq!(selection_open(&previewed, &readme).map(|opened| opened.preview), Some(true));
        assert_eq!(selection_open(&autoscroll, &folder("/w/acme/src")), None);
        // A row of "Open Files" has no listing entry behind it and opens all the same.
        let listed = ProjectNode { entry: None, ..file("/w/acme/src/App.kt") };
        assert!(selection_open(&autoscroll, &listed).is_some());
        assert_eq!(row_click(&autoscroll, &listed, 2, true), RowClick::Open { preview: false, focus: true });
    }
}
