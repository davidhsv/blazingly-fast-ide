//! The Git log's changed-files pane: a file of the selected commits with the revisions holding its
//! content before and after the change, and where its diffs open.

use crate::editor::tabs::EditorInput;
use crate::vcs_diff::diff_input::{DiffData, DiffSide, DiffSurface, repository_path, vcs_diff_surface, with_chain};
use ide_vcs::ChangeKind;
use serde_json::json;

/// Git's empty tree: a file added in a root commit is diffed against this.
pub const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// The editor kind of a merge commit's file shown against both parents.
pub const MERGE_DIFF_KIND: &str = "git-merge-diff";

/// The file as one parent of a merge commit has it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeParent {
    pub rev: String,
    pub path: String,
}

/// A file of the pane.
#[derive(Clone, Debug, PartialEq)]
pub struct LogChange {
    /// Repository-relative path (after the change).
    pub path: String,
    pub repo: Option<String>,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
    /// `None`: the file did not exist before.
    pub before: Option<String>,
    /// `None`: the change deleted the file.
    pub after: Option<String>,
    /// Revisions whose diff holds this file's line counts. For one commit that is its first parent
    /// (or the empty tree) and the commit itself, added files included.
    pub stats_from: String,
    pub stats_to: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
    pub diff: DiffData,
    /// A file of a repository opened for reading only.
    pub read_only: bool,
    pub group: Option<String>,
    /// The file in both parents of a merge commit.
    pub merge_parents: Vec<MergeParent>,
}

/// Where the diff of the pane's files opens.
#[derive(Clone, Debug, PartialEq)]
pub enum ShownChanges {
    /// A merge commit's file against both parents, in the window in front of the commit dialog.
    MergeWindow(EditorInput),
    /// The same in an editor tab.
    MergeTab { input: EditorInput, preview: bool, focus: bool },
    /// In the editor's preview tab, without taking focus (the selection in the pane).
    PreviewTab(Box<DiffData>),
    /// Where the diffs the user opens go.
    Diff { data: Box<DiffData>, surface: DiffSurface },
}

/// Show Diff: the first of `changes`, with every file of the pane (`all`) as its chain. With
/// `preview` it shows in the editor's preview tab whatever the commit interface; `non_modal` is the
/// setting "Use non-modal commit interface", `current_root` the selected repository's root.
pub fn show_changes_diff(
    changes: &[LogChange],
    all: &[LogChange],
    preview: bool,
    non_modal: bool,
    current_root: Option<&str>,
) -> Option<ShownChanges> {
    let first = changes.first()?;
    if let (Some(revision), [_, _]) = (first.after.as_deref(), first.merge_parents.as_slice()) {
        let parents: Vec<_> = first.merge_parents.iter().map(|p| json!({ "rev": p.rev, "path": p.path })).collect();
        let repo = first.repo.as_deref();
        let input = EditorInput {
            kind: MERGE_DIFF_KIND.to_owned(),
            key: format!("{MERGE_DIFF_KIND}:{}:{revision}:{}", repo.unwrap_or("null"), first.path),
            path: None,
            data: Some(json!({
                "repo": repo.or(current_root),
                "revision": revision,
                "path": first.path,
                "parents": parents,
            })),
        };
        let front = !preview && vcs_diff_surface(non_modal) == DiffSurface::FrontWindow;
        return Some(if front {
            ShownChanges::MergeWindow(input)
        } else {
            ShownChanges::MergeTab { input, preview, focus: !preview }
        });
    }
    let chain = if all.contains(first) { all } else { changes };
    let index = chain.iter().position(|change| change == first).unwrap_or(0);
    let diffs: Vec<DiffData> = chain.iter().map(|change| change.diff.clone()).collect();
    let data = Box::new(with_chain(&diffs, index)?);
    // A selection's preview stays a preview tab even under the modal interface; an explicit diff
    // opens where the commit interface puts it.
    Some(if preview {
        ShownChanges::PreviewTab(data)
    } else {
        ShownChanges::Diff { data, surface: vcs_diff_surface(non_modal) }
    })
}

/// Which side of a change Compare with Local takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeSide {
    After,
    Before,
}

/// Compare with Local / Compare Before with Local: the revision against the working file. `None`
/// when the change has no such revision or its repository is opened for reading only.
pub fn compare_change_with_local(change: &LogChange, side: ChangeSide, current_root: Option<&str>) -> Option<DiffData> {
    if change.read_only {
        return None;
    }
    let rev = match side {
        ChangeSide::After => change.after.as_deref(),
        ChangeSide::Before => change.before.as_deref(),
    };
    let rev = rev.filter(|rev| !rev.is_empty())?;
    let root = change.repo.as_deref().or(current_root);
    let old_path = change.old_path.as_deref().filter(|old| side == ChangeSide::Before && !old.is_empty());
    let left = DiffSide::Rev {
        rev: rev.to_owned(),
        path: old_path.map(|old| repository_path(root, old)),
        label: Some(rev.chars().take(8).collect()),
    };
    Some(DiffData::new(&repository_path(root, &change.path), left, DiffSide::worktree()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcs_diff::compare::{CompareOutcome, compare_texts};

    fn change(path: &str, before: Option<&str>, after: Option<&str>) -> LogChange {
        let left = before.map_or_else(|| DiffSide::text("", "(does not exist)"), DiffSide::rev);
        let right = after.map_or_else(|| DiffSide::text("", "(deleted)"), DiffSide::rev);
        LogChange {
            path: path.to_owned(),
            repo: Some("/p".to_owned()),
            old_path: None,
            kind: ChangeKind::Modified,
            before: before.map(str::to_owned),
            after: after.map(str::to_owned),
            stats_from: before.unwrap_or(EMPTY_TREE).to_owned(),
            stats_to: after.unwrap_or_default().to_owned(),
            added: None,
            removed: None,
            diff: DiffData::new(&format!("/p/{path}"), left, right),
            read_only: false,
            group: None,
            merge_parents: Vec::new(),
        }
    }

    /// A file of a merge commit, changed against both parents.
    fn merge() -> LogChange {
        let parent = |rev: &str| MergeParent { rev: rev.to_owned(), path: "a.ts".to_owned() };
        LogChange { merge_parents: vec![parent("p1"), parent("p2")], ..change("a.ts", Some("p1"), Some("m")) }
    }

    // src/features/vcs/__tests__/modalDiffWindow.test.ts › opens Compare actions in the front window under the modal interface
    #[test]
    fn opens_compare_actions_in_the_front_window_under_the_modal_interface() {
        let texts = json!({
            "path": "/p/a.ts",
            "left": { "text": "a", "label": "Expected" },
            "right": { "text": "b", "label": "Actual" },
        });
        let CompareOutcome::OpenDiff(data) = compare_texts(Some(&texts)) else {
            panic!("the texts are compared");
        };
        assert_eq!(data.path, "/p/a.ts");
        assert_eq!(vcs_diff_surface(false), DiffSurface::FrontWindow);
    }

    // src/features/vcs/__tests__/modalDiffWindow.test.ts › keeps Compare actions in editor tabs with the non-modal interface
    #[test]
    fn keeps_compare_actions_in_editor_tabs_with_the_non_modal_interface() {
        assert_eq!(vcs_diff_surface(true), DiffSurface::EditorTab);
    }

    // src/features/vcs/__tests__/modalDiffWindow.test.ts › opens a merge commit's file in the front window under the modal interface
    #[test]
    fn opens_a_merge_commits_file_in_the_front_window_under_the_modal_interface() {
        let shown = show_changes_diff(&[merge()], &[], false, false, None);
        let Some(ShownChanges::MergeWindow(input)) = shown else {
            panic!("the merge diff opens in the front window");
        };
        assert_eq!(input.kind, "git-merge-diff");
        assert_eq!(input.key, "git-merge-diff:/p:m:a.ts");
    }

    // src/features/vcs/__tests__/modalDiffWindow.test.ts › keeps a merge commit's file in an editor tab with the non-modal interface, and previews too
    #[test]
    fn keeps_a_merge_commits_file_in_an_editor_tab_with_the_non_modal_interface_and_previews_too() {
        let shown = show_changes_diff(&[merge()], &[], false, true, None);
        let Some(ShownChanges::MergeTab { input, preview, focus }) = shown else {
            panic!("the merge diff opens in an editor tab");
        };
        assert_eq!((input.kind.as_str(), preview, focus), ("git-merge-diff", false, true));
        // A preview stays in the editor under the modal interface.
        let previewed = show_changes_diff(&[merge()], &[], true, false, None);
        assert!(matches!(previewed, Some(ShownChanges::MergeTab { preview: true, focus: false, .. })));
    }

    // src/features/vcs/__tests__/show-diff-chain.test.ts › Compare with Local opens the front window under the modal interface
    #[test]
    fn compare_with_local_opens_the_front_window_under_the_modal_interface() {
        let mut modified = change("b.ts", None, Some("abc123"));
        modified.repo = Some("/repo".to_owned());
        let data = compare_change_with_local(&modified, ChangeSide::After, None).expect("the file has a revision");
        assert_eq!(data.path, "/repo/b.ts");
        assert_eq!(data.right, DiffSide::worktree());
        assert!(matches!(&data.left, DiffSide::Rev { rev, .. } if rev == "abc123"));
        assert_eq!(vcs_diff_surface(false), DiffSurface::FrontWindow);
    }

    // Not from a spec: the pane's files are the chain of the diff, a preview stays a preview tab, and
    // Compare Before with Local reads the file at its old path.
    #[test]
    fn chains_the_files_of_the_pane_and_reads_a_renamed_file_at_its_old_path() {
        let all = [change("a.ts", Some("p"), Some("c")), change("b.ts", Some("p"), Some("c"))];
        let second = std::slice::from_ref(&all[1]);
        let Some(ShownChanges::Diff { data, surface }) = show_changes_diff(second, &all, false, true, None) else {
            panic!("the diff opens");
        };
        assert_eq!((data.path.as_str(), surface), ("/p/b.ts", DiffSurface::EditorTab));
        let chain: Vec<&str> = data.chain.iter().flatten().map(|item| item.path.as_str()).collect();
        assert_eq!(chain, ["/p/a.ts", "/p/b.ts"]);
        assert!(matches!(show_changes_diff(second, &all, true, false, None), Some(ShownChanges::PreviewTab(_))));
        assert_eq!(show_changes_diff(&[], &all, false, true, None), None);

        let renamed = LogChange { old_path: Some("old.ts".to_owned()), ..change("new.ts", Some("p"), Some("c")) };
        let before = compare_change_with_local(&renamed, ChangeSide::Before, None).expect("the file existed before");
        let old_path = Some("/p/old.ts".to_owned());
        assert_eq!(before.left, DiffSide::Rev { rev: "p".to_owned(), path: old_path, label: Some("p".to_owned()) });
        let read_only = LogChange { read_only: true, ..renamed };
        assert_eq!(compare_change_with_local(&read_only, ChangeSide::After, None), None);
    }
}
