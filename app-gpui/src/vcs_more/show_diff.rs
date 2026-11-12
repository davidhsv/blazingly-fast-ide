//! Show Diff: which files the action shows for the context it runs in, with what to move along (the
//! other files of a file's group, or the other selected files), and where the diffs open.

use crate::platform::i18n::I18n;
use crate::vcs::changes::{
    ChangeNode, ChangelistState, FileStatus, build_change_tree, change_file_nodes, change_group_paths, strip_slash,
};
use crate::vcs_diff::diff_input::{
    DiffData, DiffSurface, change_node_diff, change_node_diff_data, vcs_diff_surface, with_chain, working_diff_data,
};
use crate::vcs_ops::actions::{PathContext, arg_paths, selected_paths};
use crate::vcs_ops::ops::vcs_notice;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use serde_json::Value;
use std::collections::HashSet;

/// Id of Show Diff.
pub const SHOW_DIFF: &str = "vcs.showDiff";

/// How many editor tabs one Show Diff opens at most.
const MAX_TABS: usize = 5;

/// What Show Diff starts with.
#[derive(Clone, Debug, PartialEq)]
pub enum ShowDiffStart {
    /// The Shelf has focus: it shows the diff of its own selection.
    Shelf,
    /// Nothing is selected: report it.
    Nothing(NotificationRequest),
    /// The paths to show. Folders among them open as a comparison of directories; when they do
    /// not, [`show_diff_files`] says what opens.
    Paths(Vec<String>),
}

/// The line an action argument asks to reveal (`{ "path": …, "line": … }`).
pub fn arg_line(arg: Option<&Value>) -> Option<u32> {
    let line = arg?.as_object()?.get("line")?;
    let number = line.as_u64().or_else(|| line.as_str()?.trim().parse().ok())?;
    u32::try_from(number).ok()
}

/// Show Diff: the argument's paths, else the changes tree's selection while it has focus, else the
/// Project view's, else the active file.
pub fn show_diff_start(arg: Option<&Value>, context: PathContext, shelf_focus: bool) -> ShowDiffStart {
    if shelf_focus && arg.is_none() {
        return ShowDiffStart::Shelf;
    }
    let paths = selected_paths(arg, context);
    if paths.is_empty() {
        let notice = vcs_notice(NotificationLevel::Info, "Select a changed file to show its diff");
        return ShowDiffStart::Nothing(notice);
    }
    ShowDiffStart::Paths(paths)
}

/// What the changes tree shows, and what is selected in it.
#[derive(Clone, Copy, Debug)]
pub struct ChangesView<'a> {
    pub status: &'a [FileStatus],
    pub root: Option<&'a str>,
    /// Repository roots; several group each group's files by repository.
    pub roots: &'a [String],
    pub lists: Option<&'a ChangelistState>,
    pub group_by_dir: bool,
    pub group_by_staging: bool,
    /// Keys of the nodes selected in the tree.
    pub selection_keys: &'a [String],
}

/// A diff to open in an editor tab.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffTab {
    pub data: DiffData,
    /// In the preview tab, which the next preview replaces.
    pub preview: bool,
}

/// What Show Diff opens.
#[derive(Clone, Debug, PartialEq)]
pub enum ShownDiffs {
    /// The modal commit interface has one diff window in front of it: a file with its group as the
    /// chain, or several selected files as the chain.
    FrontWindow(Box<DiffData>),
    /// Each file in a tab of its own, with its group as the chain.
    Tabs(Vec<DiffTab>),
}

/// A file Show Diff shows: a node of the changes tree where the side of the index is known, else a
/// path.
enum Target<'a> {
    Node(&'a ChangeNode),
    Path(&'a str),
}

/// The files of `paths` as tree nodes where the side of the index is known: the tree's selected
/// file nodes (`from_tree`), else the nodes of the paths the tree lists once. A partially staged
/// file listed under both staging groups stays a path: its whole working diff.
fn targets<'a>(
    tree: &'a [ChangeNode],
    paths: &'a [String],
    selection_keys: &[String],
    from_tree: bool,
) -> Vec<Target<'a>> {
    let files = change_file_nodes(tree);
    if from_tree {
        let keys: HashSet<&str> = selection_keys.iter().map(String::as_str).collect();
        let is_selected = |node: &&ChangeNode| keys.contains(node.key.as_str());
        let selected: Vec<&ChangeNode> = files.iter().copied().filter(is_selected).collect();
        if !selected.is_empty() && selected.len() == keys.len() {
            return selected.into_iter().map(Target::Node).collect();
        }
    }
    let node_of = |path: &'a String| {
        let mut listed = files.iter().copied().filter(|node| node.path.as_deref() == Some(path.as_str()));
        match (listed.next(), listed.next()) {
            (Some(node), None) => Target::Node(node),
            _ => Target::Path(path),
        }
    };
    paths.iter().map(node_of).collect()
}

/// The files Show Diff opens for `paths` ([`ShowDiffStart::Paths`]) and where: the front window
/// under the modal commit interface (`non_modal` is the setting "Use non-modal commit interface"),
/// else editor tabs, at most five of them.
pub fn show_diff_files(
    arg: Option<&Value>,
    paths: &[String],
    view: ChangesView,
    context: PathContext,
    non_modal: bool,
    i18n: &mut I18n,
) -> Option<ShownDiffs> {
    let line = arg_line(arg);
    let lists = view.lists;
    let root = view.root.unwrap_or_default();
    let tree = build_change_tree(view.status, root, view.group_by_dir, lists, view.roots, view.group_by_staging, i18n);
    let status_of = |path: &str| view.status.iter().find(|fs| strip_slash(&fs.path) == strip_slash(path));
    // A changed file's working diff, with the other files of its group, in tree order, as the chain.
    let group_diff = |path: &str| {
        let siblings = change_group_paths(&tree, path);
        let index = siblings.iter().position(|sibling| sibling == path).unwrap_or(0);
        let items: Vec<DiffData> =
            siblings.iter().map(|sibling| working_diff_data(sibling, None, None, status_of(sibling))).collect();
        with_chain(&items, index)
    };
    if vcs_diff_surface(non_modal) == DiffSurface::EditorTab {
        let preview = paths.len() == 1;
        let tabs = paths.iter().take(MAX_TABS).filter_map(|path| group_diff(path));
        return Some(ShownDiffs::Tabs(tabs.map(|data| DiffTab { data: DiffData { line, ..data }, preview }).collect()));
    }
    let explicit = arg_paths(arg).is_some_and(|paths| !paths.is_empty());
    let shown = targets(&tree, paths, view.selection_keys, !explicit && context.commit_window_focused);
    let data = match shown.as_slice() {
        [] => None,
        [Target::Path(path)] => group_diff(path),
        [Target::Node(node)] => change_node_diff_data(&tree, node, status_of),
        several => {
            let items: Vec<DiffData> = several
                .iter()
                .filter_map(|target| match target {
                    Target::Path(path) => Some(working_diff_data(path, None, None, status_of(path))),
                    Target::Node(node) => change_node_diff(node, None, status_of),
                })
                .collect();
            with_chain(&items, 0)
        }
    }?;
    Some(ShownDiffs::FrontWindow(Box::new(DiffData { line, ..data })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs::changes::{ChangeKind, Changelist};
    use crate::vcs_diff::diff_input::{DiffSide, Reveal, chain_neighbour};
    use crate::vcs_diff::doc::Direction;
    use crate::vcs_ops::actions::commit_window_focused;
    use crate::vcs_ops::changelists::initial_changelists;
    use serde_json::json;

    fn status(name: &str) -> FileStatus {
        FileStatus {
            path: format!("/repo/{name}"),
            rel_path: name.to_owned(),
            staged: None,
            unstaged: Some(ChangeKind::Modified),
            renamed_from: None,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    /// What the action runs in: the changed files, how they are grouped, and what is selected where.
    struct Fixture {
        files: Vec<FileStatus>,
        roots: Vec<String>,
        lists: ChangelistState,
        group_by_dir: bool,
        group_by_staging: bool,
        /// Paths and node keys selected in the changes tree.
        selection: Vec<String>,
        selection_keys: Vec<String>,
        project_selection: Vec<String>,
        active_tool_window: Option<&'static str>,
        git_tab: &'static str,
        non_modal: bool,
    }

    impl Fixture {
        /// Four changed files, the last one in a change list of its own, under the non-modal
        /// commit interface.
        fn new() -> Self {
            let mut lists = initial_changelists();
            lists.lists.push(Changelist { id: "other".to_owned(), name: "Other".to_owned(), comment: String::new() });
            lists.files.insert("other.ts".to_owned(), "other".to_owned());
            Self {
                files: ["z.ts", "b.ts", "a.ts", "other.ts"].map(status).to_vec(),
                roots: strings(&["/repo"]),
                lists,
                group_by_dir: false,
                group_by_staging: false,
                selection: Vec::new(),
                selection_keys: Vec::new(),
                project_selection: Vec::new(),
                active_tool_window: None,
                git_tab: "log",
                non_modal: true,
            }
        }

        fn modal(mut self) -> Self {
            self.non_modal = false;
            self
        }

        fn files(mut self, names: &[&str]) -> Self {
            self.files = names.iter().map(|name| status(name)).collect();
            self
        }

        /// The changes tree has focus with `paths` selected in it.
        fn selected_in(mut self, window: &'static str, tab: &'static str, paths: &[&str], keys: &[&str]) -> Self {
            self.active_tool_window = Some(window);
            self.git_tab = tab;
            self.selection = strings(paths);
            self.selection_keys = strings(keys);
            self
        }

        fn show(&self, arg: Option<Value>) -> ShownDiffs {
            let context = PathContext {
                commit_selection: &self.selection,
                commit_window_focused: commit_window_focused(self.active_tool_window, self.git_tab),
                project_selection: &self.project_selection,
                active_path: None,
            };
            let ShowDiffStart::Paths(paths) = show_diff_start(arg.as_ref(), context, false) else {
                panic!("there is a file to show");
            };
            let view = ChangesView {
                status: &self.files,
                root: Some("/repo"),
                roots: &self.roots,
                lists: Some(&self.lists),
                group_by_dir: self.group_by_dir,
                group_by_staging: self.group_by_staging,
                selection_keys: &self.selection_keys,
            };
            let mut i18n = I18n::default();
            show_diff_files(arg.as_ref(), &paths, view, context, self.non_modal, &mut i18n).expect("a diff opens")
        }

        /// The first editor tab Show Diff opens.
        fn tab(&self, arg: Option<Value>) -> DiffData {
            match self.show(arg) {
                ShownDiffs::Tabs(tabs) => tabs.into_iter().next().expect("a tab opens").data,
                ShownDiffs::FrontWindow(_) => panic!("the diff opens in an editor tab"),
            }
        }

        /// The diff Show Diff opens in the front window.
        fn front(&self, arg: Option<Value>) -> DiffData {
            match self.show(arg) {
                ShownDiffs::FrontWindow(data) => *data,
                ShownDiffs::Tabs(_) => panic!("the diff opens in the front window"),
            }
        }
    }

    fn chain(data: &DiffData) -> Vec<&str> {
        data.chain.iter().flatten().map(|item| item.path.as_str()).collect()
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › keeps sibling files in tree order when Show Diff opens a changed file
    #[test]
    fn keeps_sibling_files_in_tree_order_when_show_diff_opens_a_changed_file() {
        let data = Fixture::new().tab(Some(json!("/repo/b.ts")));
        assert_eq!(data.path, "/repo/b.ts");
        assert_eq!(chain(&data), ["/repo/a.ts", "/repo/b.ts", "/repo/z.ts"]);
        let next = chain_neighbour(&data, Direction::Next).expect("a file follows");
        assert_eq!((next.path.as_str(), next.reveal), ("/repo/z.ts", Some(Reveal::First)));
        let previous = chain_neighbour(&data, Direction::Previous).expect("a file precedes");
        assert_eq!((previous.path.as_str(), previous.reveal), ("/repo/a.ts", Some(Reveal::Last)));
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › keeps an explicit line on the opened file without carrying it into neighbouring requests
    #[test]
    fn keeps_an_explicit_line_on_the_opened_file_without_carrying_it_into_neighbouring_requests() {
        let data = Fixture::new().tab(Some(json!({ "path": "/repo/b.ts", "line": 12 })));
        assert_eq!(data.line, Some(12));
        let next = chain_neighbour(&data, Direction::Next).expect("a file follows");
        let back = chain_neighbour(&next, Direction::Previous).expect("the opened file precedes it");
        assert_eq!(back.line, None);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › opens a file outside the changes list without unrelated siblings
    #[test]
    fn opens_a_file_outside_the_changes_list_without_unrelated_siblings() {
        let data = Fixture::new().tab(Some(json!("/repo/clean.ts")));
        assert_eq!(data.path, "/repo/clean.ts");
        assert_eq!(data.chain, None);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › follows directory order rather than global filename order
    #[test]
    fn follows_directory_order_rather_than_global_filename_order() {
        let mut fixture = Fixture::new().files(&["a.ts", "src/z.ts", "src/nested/b.ts", "other.ts"]);
        fixture.group_by_dir = true;
        let data = fixture.tab(Some(json!("/repo/src/z.ts")));
        assert_eq!(chain(&data), ["/repo/src/nested/b.ts", "/repo/src/z.ts", "/repo/a.ts"]);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › keeps repository grouping in a changelist's chain
    #[test]
    fn keeps_repository_grouping_in_a_changelists_chain() {
        let mut fixture = Fixture::new().files(&["z.ts", "module/b.ts", "module/a.ts"]);
        fixture.roots = strings(&["/repo", "/repo/module"]);
        let data = fixture.tab(Some(json!("/repo/module/b.ts")));
        assert_eq!(chain(&data), ["/repo/z.ts", "/repo/module/a.ts", "/repo/module/b.ts"]);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › shows the selected change rather than a Project view selection kept from earlier
    #[test]
    fn shows_the_selected_change_rather_than_a_project_view_selection_kept_from_earlier() {
        // The Project view keeps its selection while the Commit window has focus: the file selected
        // in the changes tree opens, not the one still selected there.
        let mut fixture = Fixture::new().selected_in("commit", "log", &["/repo/b.ts"], &[]);
        fixture.project_selection = strings(&["/repo/z.ts"]);
        assert_eq!(fixture.tab(None).path, "/repo/b.ts");
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › shows a Project view selection outside the changes tree
    #[test]
    fn shows_a_project_view_selection_outside_the_changes_tree() {
        let mut fixture = Fixture::new();
        fixture.project_selection = strings(&["/repo/z.ts"]);
        fixture.active_tool_window = Some("project");
        assert_eq!(fixture.tab(None).path, "/repo/z.ts");
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › opens the front diff window from the modal Local Changes selection (⌘D)
    #[test]
    fn opens_the_front_diff_window_from_the_modal_local_changes_selection() {
        let fixture = Fixture::new().modal().selected_in("git", "changes", &["/repo/b.ts"], &[]);
        let data = fixture.front(None);
        assert_eq!(data.path, "/repo/b.ts");
        assert_eq!(chain(&data), ["/repo/a.ts", "/repo/b.ts", "/repo/z.ts"]);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › keeps the editor diff outside the modal interface with the Commit window focused
    #[test]
    fn keeps_the_editor_diff_outside_the_modal_interface_with_the_commit_window_focused() {
        let fixture = Fixture::new().selected_in("commit", "log", &["/repo/b.ts"], &[]);
        assert!(matches!(fixture.show(None), ShownDiffs::Tabs(tabs) if !tabs.is_empty()));
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › openVcsDiff opens the front window under the modal interface
    #[test]
    fn open_vcs_diff_opens_the_front_window_under_the_modal_interface() {
        assert_eq!(vcs_diff_surface(false), DiffSurface::FrontWindow);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › openVcsDiff keeps editor tabs without the modal interface
    #[test]
    fn open_vcs_diff_keeps_editor_tabs_without_the_modal_interface() {
        assert_eq!(vcs_diff_surface(true), DiffSurface::EditorTab);
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › keeps the hunk's line when Show Diff opens the front window (gutter popup)
    #[test]
    fn keeps_the_hunks_line_when_show_diff_opens_the_front_window() {
        let data = Fixture::new().modal().front(Some(json!({ "path": "/repo/b.ts", "line": 42 })));
        assert_eq!((data.path.as_str(), data.line), ("/repo/b.ts", Some(42)));
    }

    /// A partially staged file under the staging groups, with the modal commit interface.
    fn partially_staged() -> Fixture {
        let mut fixture = Fixture::new().modal();
        fixture.files = vec![FileStatus { staged: Some(ChangeKind::Modified), ..status("b.ts") }];
        fixture.group_by_staging = true;
        fixture
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › a partially staged file under the staging groups › opens its whole working diff for a path, which does not tell the side
    #[test]
    fn opens_its_whole_working_diff_for_a_path_which_does_not_tell_the_side() {
        let data = partially_staged().front(Some(json!("/repo/b.ts")));
        assert_eq!((data.left, data.right), (DiffSide::rev("HEAD"), DiffSide::worktree()));
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › a partially staged file under the staging groups › opens the side selected in the changes tree
    #[test]
    fn opens_the_side_selected_in_the_changes_tree() {
        let fixture = partially_staged().selected_in("git", "changes", &["/repo/b.ts"], &["unstaged:/repo/b.ts"]);
        let data = fixture.front(None);
        assert!(matches!(&data.left, DiffSide::Rev { rev, .. } if rev == ":0"));
        assert_eq!(data.right, DiffSide::worktree());
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › chains the selected files in the front window
    #[test]
    fn chains_the_selected_files_in_the_front_window() {
        let fixture = Fixture::new().modal().selected_in("git", "changes", &["/repo/z.ts", "/repo/other.ts"], &[]);
        let data = fixture.front(None);
        assert_eq!(data.path, "/repo/z.ts");
        assert_eq!(chain(&data), ["/repo/z.ts", "/repo/other.ts"]);
    }
}
