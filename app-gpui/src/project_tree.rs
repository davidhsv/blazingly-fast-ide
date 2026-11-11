//! The Project tool window's file tree: the project folder, listed lazily through the backend.
//!
//! The backend lists a folder with directories first in natural order and marks what
//! `.gitignore` and the exclude settings match; the tree shows those dimmed (Show Excluded Files
//! is on by default) and never lists a folder before it is expanded.

use crate::backend::Backend;
use crate::theme;
use gpui_kit::{
    App, ClickEvent, Context, EventEmitter, FocusHandle, Focusable, IntoElement, KeyDownEvent, Render, SharedString,
    Window, div, prelude::*, px, uniform_list,
};
use ide_vfs::{EntryKind, FileEntry};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Symlinks report the target's metadata; a symlink of size 0 with a directory target is a folder.
fn is_dir(entry: &FileEntry) -> bool {
    match entry.kind {
        EntryKind::Dir => true,
        EntryKind::Symlink => entry.target_kind.as_ref().is_some_and(|kind| matches!(kind, EntryKind::Dir)),
        EntryKind::File => false,
    }
}

/// One visible line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub dir: bool,
    pub expanded: bool,
    /// Matched by `.gitignore` or the exclude settings: drawn dimmed.
    pub ignored: bool,
}

/// What is loaded and expanded.
#[derive(Debug, Clone)]
pub struct TreeModel {
    root: PathBuf,
    listings: HashMap<PathBuf, Vec<FileEntry>>,
    expanded: HashSet<PathBuf>,
    /// Show Excluded Files (on by default): entries the backend marks ignored stay listed.
    pub show_excluded: bool,
}

impl TreeModel {
    /// A tree of `root`, expanded at the root as the Project view opens it.
    pub fn new(root: PathBuf) -> Self {
        Self { expanded: HashSet::from([root.clone()]), root, listings: HashMap::new(), show_excluded: true }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_loaded(&self, dir: &Path) -> bool {
        self.listings.contains_key(dir)
    }

    pub fn is_expanded(&self, dir: &Path) -> bool {
        self.expanded.contains(dir)
    }

    /// The backend's listing of `dir`, in its order.
    pub fn set_listing(&mut self, dir: PathBuf, entries: Vec<FileEntry>) {
        self.listings.insert(dir, entries);
    }

    /// Expands or collapses `dir`; returns whether it is now expanded.
    pub fn toggle(&mut self, dir: &Path) -> bool {
        if self.expanded.remove(dir) {
            return false;
        }
        self.expanded.insert(dir.to_path_buf());
        true
    }

    pub fn set_expanded(&mut self, dir: &Path, expanded: bool) {
        if expanded {
            self.expanded.insert(dir.to_path_buf());
        } else {
            self.expanded.remove(dir);
        }
    }

    /// Expanded folders whose listing has not arrived yet.
    pub fn missing_listings(&self) -> Vec<PathBuf> {
        let mut missing: Vec<PathBuf> = self
            .rows()
            .into_iter()
            .filter(|r| r.dir && r.expanded && !self.is_loaded(&r.path))
            .map(|r| r.path)
            .collect();
        missing.sort();
        missing
    }

    /// The visible lines, top to bottom: the root, then each expanded folder's entries.
    pub fn rows(&self) -> Vec<Row> {
        let name = self
            .root
            .file_name()
            .map_or_else(|| self.root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
        let mut rows = vec![Row {
            path: self.root.clone(),
            name,
            depth: 0,
            dir: true,
            expanded: self.is_expanded(&self.root),
            ignored: false,
        }];
        if self.is_expanded(&self.root) {
            self.push_children(&self.root, 1, &mut rows);
        }
        rows
    }

    fn push_children(&self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        let Some(entries) = self.listings.get(dir) else { return };
        for entry in entries.iter().filter(|e| self.show_excluded || !e.ignored) {
            let path = PathBuf::from(entry.path.clone());
            let dir = is_dir(entry);
            let expanded = dir && self.is_expanded(&path);
            rows.push(Row {
                path: path.clone(),
                name: entry.name.clone(),
                depth,
                dir,
                expanded,
                ignored: entry.ignored,
            });
            if expanded {
                self.push_children(&path, depth + 1, rows);
            }
        }
    }
}

/// The user asked to open a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile(pub PathBuf);

/// The file tree of the project open in a window.
pub struct ProjectTree {
    focus: FocusHandle,
    window_label: String,
    model: TreeModel,
    selected: Option<PathBuf>,
}

impl EventEmitter<OpenFile> for ProjectTree {}

impl ProjectTree {
    pub fn new(window_label: &str, root: PathBuf, cx: &mut Context<Self>) -> Self {
        let mut tree = Self {
            focus: cx.focus_handle(),
            window_label: window_label.to_owned(),
            model: TreeModel::new(root),
            selected: None,
        };
        tree.load_missing(cx);
        tree
    }

    pub fn model(&self) -> &TreeModel {
        &self.model
    }

    pub fn selected(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    /// Asks the backend for the listing of every expanded folder that has none.
    fn load_missing(&mut self, cx: &mut Context<Self>) {
        for dir in self.model.missing_listings() {
            let (window, path) = (self.window_label.clone(), dir.clone());
            let listing = cx
                .global::<Backend>()
                .with_host(move |host| async move { ide_host::ops::fs::read_dir(&host, &window, path).await });
            cx.spawn(async move |tree, cx| {
                let entries = match listing.await {
                    Ok(Ok(entries)) => entries,
                    Ok(Err(error)) => {
                        tracing::warn!("cannot list {}: {}", dir.display(), error.message);
                        Vec::new()
                    }
                    Err(error) => {
                        tracing::warn!("cannot list {}: {error:#}", dir.display());
                        Vec::new()
                    }
                };
                let _ = tree.update(cx, |tree: &mut ProjectTree, cx| {
                    tree.model.set_listing(dir, entries);
                    cx.notify();
                });
            })
            .detach();
        }
    }

    /// Expands or collapses the folder at `path`.
    pub fn toggle(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.model.toggle(path);
        self.load_missing(cx);
        cx.notify();
    }

    pub fn select(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.selected = Some(path.to_path_buf());
        cx.notify();
    }

    /// Opens the file at `path`, or toggles the folder (Enter, double click).
    pub fn activate(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(row) = self.model.rows().into_iter().find(|row| row.path == path) else { return };
        if row.dir {
            self.toggle(path, cx);
        } else {
            cx.emit(OpenFile(row.path));
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let modifiers = event.keystroke.modifiers;
        if modifiers.control || modifiers.alt || modifiers.platform || modifiers.shift {
            return;
        }
        let rows = self.model.rows();
        let current = self.selected.as_ref().and_then(|selected| rows.iter().position(|row| &row.path == selected));
        let row = current.and_then(|index| rows.get(index));
        match (event.keystroke.key.as_str(), row) {
            ("down", _) => {
                let next = current.map_or(0, |index| (index + 1).min(rows.len() - 1));
                self.selected = rows.get(next).map(|row| row.path.clone());
            }
            ("up", _) => {
                let previous = current.map_or(0, |index| index.saturating_sub(1));
                self.selected = rows.get(previous).map(|row| row.path.clone());
            }
            // Right opens a closed folder, then steps into it; Left closes an open folder, else
            // goes to the parent.
            ("right", Some(row)) if row.dir && !row.expanded => self.toggle(&row.path.clone(), cx),
            ("right", Some(row)) if row.dir => {
                self.selected = current.and_then(|index| rows.get(index + 1)).map(|row| row.path.clone())
            }
            ("left", Some(row)) if row.dir && row.expanded => self.toggle(&row.path.clone(), cx),
            ("left", Some(row)) => {
                let parent =
                    current.and_then(|index| rows[..index].iter().rev().find(|candidate| candidate.depth < row.depth));
                if let Some(parent) = parent {
                    self.selected = Some(parent.path.clone());
                }
            }
            ("enter", Some(row)) => self.activate(&row.path.clone(), cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}

impl Focusable for ProjectTree {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ProjectTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.model.rows();
        let focused = self.focus.is_focused(window);
        let (row_height, selected_bg) = (
            theme::length(cx, "h-row"),
            theme::color(cx, if focused { "bg-tree-selected" } else { "bg-tree-selected-inactive" }),
        );
        let (fg, dimmed, chevron_color) =
            (theme::color(cx, "fg-strong"), theme::color(cx, "fg-muted"), theme::color(cx, "fg-icon"));
        let selected = self.selected.clone();
        let list = uniform_list(
            "project-tree",
            rows.len(),
            cx.processor(move |_, range: std::ops::Range<usize>, _, cx| {
                rows[range]
                    .iter()
                    .map(|row| {
                        let chevron = if row.dir {
                            let name = if row.expanded { "chevron-down" } else { "chevron-right" };
                            let toggled = row.path.clone();
                            div()
                                .id(SharedString::from(format!("toggle:{}", row.path.display())))
                                .size(px(16.0))
                                .flex_none()
                                .child(theme::icon(cx, name, 16.0, chevron_color))
                                .on_click(cx.listener(move |tree, _: &ClickEvent, _, cx| {
                                    tree.toggle(&toggled, cx);
                                    cx.stop_propagation();
                                }))
                                .into_any_element()
                        } else {
                            div().size(px(16.0)).flex_none().into_any_element()
                        };
                        let color = if row.ignored { dimmed } else { fg };
                        let clicked = row.path.clone();
                        div()
                            .id(SharedString::from(format!("row:{}", row.path.display())))
                            .h(row_height)
                            .w_full()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .pl(px(4.0 + 20.0 * row.depth as f32))
                            .pr(px(8.0))
                            .text_color(color)
                            .when(selected.as_deref() == Some(row.path.as_path()), |el| el.bg(selected_bg))
                            .child(chevron)
                            .child(theme::icon(cx, if row.dir { "folder" } else { "file" }, 16.0, color))
                            .child(div().whitespace_nowrap().child(SharedString::from(row.name.clone())))
                            .on_click(cx.listener(move |tree, event: &ClickEvent, window, cx| {
                                window.focus(&tree.focus, cx);
                                tree.select(&clicked, cx);
                                if event.click_count() >= 2 {
                                    tree.activate(&clicked, cx);
                                }
                            }))
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .size_full();
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .text_size(theme::length(cx, "fs-ui"))
            .child(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AppDirs;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use std::time::Duration;

    fn entry(dir: &Path, name: &str, kind: EntryKind, ignored: bool) -> FileEntry {
        FileEntry {
            name: name.to_owned(),
            path: dir.join(name).into(),
            kind,
            size: 0,
            mtime_ms: 0.0,
            ignored,
            excluded: false,
            hidden: false,
            target_kind: None,
        }
    }

    #[test]
    fn rows_follow_what_is_expanded_and_loaded() {
        let root = PathBuf::from("/p");
        let mut model = TreeModel::new(root.clone());
        // Before the listing arrives only the root shows, and its listing is asked for.
        assert_eq!(model.rows().iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["p"]);
        assert_eq!(model.missing_listings(), std::slice::from_ref(&root));
        model.set_listing(
            root.clone(),
            vec![
                entry(&root, "src", EntryKind::Dir, false),
                entry(&root, "target", EntryKind::Dir, true),
                entry(&root, "Cargo.toml", EntryKind::File, false),
            ],
        );
        let names = |model: &TreeModel| {
            model.rows().iter().map(|r| format!("{}{}", "  ".repeat(r.depth), r.name)).collect::<Vec<_>>()
        };
        assert_eq!(names(&model), ["p", "  src", "  target", "  Cargo.toml"]);
        assert!(model.missing_listings().is_empty());

        // An expanded folder is listed once its entries are known, in the backend's order.
        assert!(model.toggle(&root.join("src")));
        assert_eq!(model.missing_listings(), [root.join("src")]);
        model.set_listing(root.join("src"), vec![entry(&root.join("src"), "main.rs", EntryKind::File, false)]);
        assert_eq!(names(&model), ["p", "  src", "    main.rs", "  target", "  Cargo.toml"]);
        assert!(model.rows()[1].expanded && model.rows()[3].ignored);

        // Collapsing keeps the listing; Show Excluded Files off hides ignored entries.
        assert!(!model.toggle(&root.join("src")));
        model.show_excluded = false;
        assert_eq!(names(&model), ["p", "  src", "  Cargo.toml"]);
        assert!(!model.toggle(&root));
        assert_eq!(names(&model), ["p"]);
    }

    #[test]
    fn a_symlink_to_a_folder_is_a_folder() {
        let dir = Path::new("/p");
        let mut link = entry(dir, "link", EntryKind::Symlink, false);
        assert!(!is_dir(&link));
        link.target_kind = Some(EntryKind::Dir);
        assert!(is_dir(&link));
    }

    fn settle(cx: &mut VisualTestContext, done: impl Fn(&mut VisualTestContext) -> bool) {
        for _ in 0..300 {
            cx.run_until_parked();
            if done(cx) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the backend did not deliver within three seconds");
    }

    fn names(tree: &Entity<ProjectTree>, cx: &mut VisualTestContext) -> Vec<String> {
        tree.read_with(cx, |tree, _| tree.model.rows().into_iter().map(|row| row.name).collect())
    }

    /// The tree lists a real project through the backend, expands with the keyboard and asks
    /// for a file to be opened.
    #[gpui_kit::test]
    async fn lists_a_project_and_opens_files_from_the_keyboard(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (home, project) = (tempfile::tempdir().expect("a home"), tempfile::tempdir().expect("a project"));
        let root = project.path().canonicalize().expect("the project folder exists");
        std::fs::create_dir(root.join("src")).expect("a folder");
        std::fs::write(root.join("src/main.rs"), "fn main() {}\n").expect("a file");
        std::fs::write(root.join("README.md"), "# readme\n").expect("a file");
        std::fs::write(root.join("a10.txt"), "").expect("a file");
        std::fs::write(root.join("a9.txt"), "").expect("a file");
        cx.update(|cx| {
            cx.set_global(theme::ActiveTheme::default());
            Backend::init(AppDirs::under(home.path()), cx).expect("the backend starts");
        });
        let open_root = root.clone();
        let opened = cx.update(|cx| {
            cx.global::<Backend>().with_host(move |host| async move {
                ide_host::ops::project::open(&host, "w1", &open_root).await.map(|_| ())
            })
        });
        opened.await.expect("the task completes").expect("the project opens");

        let tree_root = root.clone();
        let (tree, cx) = cx.add_window_view(move |_, cx| ProjectTree::new("w1", tree_root, cx));
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.model.is_loaded(tree.model.root())));
        // Folders first, then files in natural order (a9 before a10).
        let root_name = root.file_name().expect("a name").to_string_lossy().into_owned();
        assert_eq!(names(&tree, cx), [root_name.as_str(), "src", "a9.txt", "a10.txt", "README.md"]);

        let opened: std::rc::Rc<std::cell::RefCell<Vec<PathBuf>>> = Default::default();
        let log = opened.clone();
        let _subscription = cx
            .update(|_, cx| cx.subscribe(&tree, move |_, event: &OpenFile, _| log.borrow_mut().push(event.0.clone())));
        cx.update(|window, cx| {
            let focus = tree.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        // Down to the root, down to `src`, Right expands it, Right steps onto main.rs, Enter opens it.
        cx.simulate_keystrokes("down down right");
        settle(cx, |cx| tree.read_with(cx, |tree, _| tree.model.is_loaded(&root.join("src"))));
        cx.simulate_keystrokes("right");
        assert_eq!(
            tree.read_with(cx, |tree, _| tree.selected().map(Path::to_path_buf)),
            Some(root.join("src/main.rs"))
        );
        cx.simulate_keystrokes("enter");
        assert_eq!(*opened.borrow(), [root.join("src/main.rs")]);
        // Left goes to the parent folder, Left again closes it.
        cx.simulate_keystrokes("left left");
        assert_eq!(names(&tree, cx).len(), 5);
        assert_eq!(tree.read_with(cx, |tree, _| tree.selected().map(Path::to_path_buf)), Some(root.join("src")));
        let host = cx.update(|_, cx| cx.global::<Backend>().host.clone());
        host.close_project("w1");
    }
}
