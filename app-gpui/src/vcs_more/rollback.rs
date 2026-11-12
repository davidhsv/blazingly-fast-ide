//! Rollback in a project whose repository holds another one (a submodule): what the action offers.
//! The candidates themselves are chosen by [`crate::vcs_ops::change_ops::rollback_candidates`].

#[cfg(test)]
mod tests {
    use crate::vcs::changes::{ChangeKind, FileStatus};
    use crate::vcs_ops::change_ops::{is_nested_repo_entry, rollback_candidates};

    fn fs(rel: &str, unstaged: ChangeKind) -> FileStatus {
        FileStatus {
            path: format!("/r/{rel}"),
            rel_path: rel.to_owned(),
            staged: None,
            unstaged: Some(unstaged),
            renamed_from: None,
        }
    }

    fn roots() -> Vec<String> {
        vec!["/r".to_owned(), "/r/libs/core".to_owned()]
    }

    fn status() -> Vec<FileStatus> {
        vec![
            fs("app.txt", ChangeKind::Modified),
            fs("libs/core", ChangeKind::Modified),
            fs("libs/core/core.txt", ChangeKind::Modified),
            fs("libs/core/wip.txt", ChangeKind::Untracked),
        ]
    }

    /// The paths Rollback offers for a selection (`None`: for every change).
    fn offered(selection: Option<&[&str]>) -> Vec<String> {
        let chosen: Option<Vec<String>> = selection.map(|paths| paths.iter().map(|path| (*path).to_owned()).collect());
        let status = status();
        rollback_candidates(chosen.as_deref(), &status, &roots()).into_iter().map(|file| file.path.clone()).collect()
    }

    // src/features/vcs/__tests__/rollback.test.ts › rollback candidates with a submodule › never offers the submodule's own entry
    #[test]
    fn never_offers_the_submodules_own_entry() {
        assert!(is_nested_repo_entry("/r/libs/core", &roots()));
        assert!(!is_nested_repo_entry("/r", &roots()));
        assert_eq!(offered(None), ["/r/app.txt", "/r/libs/core/core.txt"]);
    }

    // src/features/vcs/__tests__/rollback.test.ts › rollback candidates with a submodule › takes a selected change alone, not the submodule's changes below its path
    #[test]
    fn takes_a_selected_change_alone_not_the_submodules_changes_below_its_path() {
        assert!(offered(Some(&["/r/libs/core"])).is_empty());
        assert_eq!(offered(Some(&["/r/app.txt", "/r/libs/core"])), ["/r/app.txt"]);
    }

    // src/features/vcs/__tests__/rollback.test.ts › rollback candidates with a submodule › takes the changes below a selected folder
    #[test]
    fn takes_the_changes_below_a_selected_folder() {
        assert_eq!(offered(Some(&["/r/libs"])), ["/r/libs/core/core.txt"]);
    }
}
