//! The worktrees of the project's repositories: which one a window works in, and one group per
//! repository.

use ide_vcs::Worktree;
use std::collections::HashMap;

/// Comparison key of a path: `/` separators, no trailing slash, the drive letter in lower case
/// (Git prints `C:/…` with the drive in either case).
fn path_key(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let trimmed = slashed.trim_end_matches('/');
    // A path of slashes only is the root.
    let kept = if trimmed.is_empty() { &slashed[..slashed.len().min(1)] } else { trimmed };
    let bytes = kept.as_bytes();
    let drive = bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/';
    if drive { kept.to_lowercase() } else { kept.to_owned() }
}

fn worktree_key(worktree: &Worktree) -> String {
    path_key(&worktree.path.0.to_string_lossy())
}

/// Whether worktree `path` (as `git worktree list` prints it) is the repository root `root` this
/// window works in. A trailing slash does not count.
pub fn is_current_worktree(path: &str, root: Option<&str>) -> bool {
    root.is_some_and(|root| !root.is_empty() && path_key(path) == path_key(root))
}

/// Whether deleting this worktree would also delete the window's project folder.
pub fn worktree_contains_project(path: &str, project_root: Option<&str>) -> bool {
    let Some(project) = project_root.filter(|root| !root.is_empty()).map(path_key) else { return false };
    let worktree = path_key(path);
    let below = if worktree.ends_with('/') { worktree.clone() } else { format!("{worktree}/") };
    project == worktree || project.starts_with(&below)
}

/// The worktrees of one repository, loaded from one of its roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoWorktrees {
    /// A repository root of the project the query ran from.
    pub root: String,
    pub worktrees: Vec<Worktree>,
    /// Why the query failed; the repository is still listed.
    pub error: Option<String>,
}

/// One group per repository. A project holds a linked worktree as a root of its own, and every
/// root of a repository lists all of its worktrees: their duplicate lists are merged, the first
/// root (in root order) staying the group's.
pub fn group_worktrees(lists: &[RepoWorktrees]) -> Vec<RepoWorktrees> {
    // Loaded lists first: a failed query knows only its own root, which a loaded list can claim as
    // one of the repository's worktrees.
    let mut ordered: Vec<&RepoWorktrees> = lists.iter().collect();
    ordered.sort_by_key(|list| list.worktrees.is_empty());
    let mut groups: Vec<RepoWorktrees> = Vec::new();
    let mut by_main: HashMap<String, usize> = HashMap::new();
    let mut by_worktree: HashMap<String, usize> = HashMap::new();
    for list in ordered {
        let main = list.worktrees.iter().find(|worktree| worktree.main).map(worktree_key);
        let root = path_key(&list.root);
        let known = main.as_ref().and_then(|main| by_main.get(main)).or_else(|| by_worktree.get(&root)).copied();
        let index = match known {
            Some(index) => {
                let group = &mut groups[index];
                // The group is named after the repository's main folder when its root is one of them.
                if main.as_deref() == Some(root.as_str()) {
                    group.root.clone_from(&list.root);
                }
                if group.worktrees.is_empty() && !list.worktrees.is_empty() {
                    group.worktrees.clone_from(&list.worktrees);
                    group.error.clone_from(&list.error);
                    by_main.extend(main.map(|main| (main, index)));
                }
                index
            }
            None => {
                groups.push(list.clone());
                let index = groups.len() - 1;
                by_main.extend(main.map(|main| (main, index)));
                index
            }
        };
        for worktree in &list.worktrees {
            by_worktree.insert(worktree_key(worktree), index);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::IpcPath;
    use std::path::PathBuf;

    fn worktree(path: &str, main: bool) -> Worktree {
        Worktree {
            path: IpcPath(PathBuf::from(path)),
            head: Some("0123456789abcdef".to_owned()),
            branch: path.rsplit('/').next().map(str::to_owned),
            main,
            locked: false,
            prunable: false,
        }
    }

    fn loaded(root: &str, worktrees: &[Worktree]) -> RepoWorktrees {
        RepoWorktrees { root: root.to_owned(), worktrees: worktrees.to_vec(), error: None }
    }

    fn failed(root: &str, error: &str) -> RepoWorktrees {
        RepoWorktrees { root: root.to_owned(), worktrees: Vec::new(), error: Some(error.to_owned()) }
    }

    // src/features/vcs/worktrees.test.ts › isCurrentWorktree › matches the repository root this window works in
    #[test]
    fn matches_the_repository_root_this_window_works_in() {
        assert!(is_current_worktree("/work/app", Some("/work/app")));
        assert!(!is_current_worktree("/work/app-wt", Some("/work/app")));
        assert!(is_current_worktree("/work/app/", Some("/work/app")));
        assert!(!is_current_worktree("/work/app", None));
    }

    // src/features/vcs/worktrees.test.ts › isCurrentWorktree › ignores the drive letter's case and separators on Windows
    #[test]
    fn ignores_the_drive_letters_case_and_separators_on_windows() {
        assert!(is_current_worktree("c:/Work/app", Some("C:/Work/app")));
        assert!(is_current_worktree(r"C:\Work\app", Some("C:/Work/app")));
        // Only drive paths: elsewhere the case is part of the name.
        assert!(!is_current_worktree("/Work/App", Some("/work/app")));
    }

    // src/features/vcs/worktrees.test.ts › groupWorktrees › keeps one group per repository
    #[test]
    fn keeps_one_group_per_repository() {
        let groups = group_worktrees(&[
            loaded("/work/app", &[worktree("/work/app", true)]),
            loaded("/work/web", &[worktree("/work/web", true)]),
        ]);
        let roots: Vec<&str> = groups.iter().map(|group| group.root.as_str()).collect();
        assert_eq!(roots, ["/work/app", "/work/web"]);
    }

    // src/features/vcs/worktrees.test.ts › groupWorktrees › merges the roots of a linked worktree that list the same repository
    #[test]
    fn merges_the_roots_of_a_linked_worktree_that_list_the_same_repository() {
        let worktrees = [worktree("/work/app", true), worktree("/work/app-feature", false)];
        let groups = group_worktrees(&[loaded("/work/app-feature", &worktrees), loaded("/work/app", &worktrees)]);
        // The main folder names the group even when the linked root came first.
        assert_eq!(groups, [loaded("/work/app", &worktrees)]);
    }

    // src/features/vcs/worktrees.test.ts › groupWorktrees › matches the main worktree case-insensitively on Windows
    #[test]
    fn matches_the_main_worktree_case_insensitively_on_windows() {
        let worktrees = [worktree("C:/Work/app", true), worktree("C:/Work/app-feature", false)];
        let groups = group_worktrees(&[loaded("c:/work/app", &worktrees), loaded("C:/Work/app-feature", &worktrees)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].root, "c:/work/app");
    }

    // src/features/vcs/worktrees.test.ts › groupWorktrees › files a root that failed under the repository that lists it
    #[test]
    fn files_a_root_that_failed_under_the_repository_that_lists_it() {
        let broken = failed("/work/app-feature", "not a git repository");
        let listed = loaded("/work/app", &[worktree("/work/app", true), worktree("/work/app-feature", false)]);
        let groups = group_worktrees(&[broken, listed.clone()]);
        assert_eq!(groups.len(), 1);
        // The group keeps the root that loaded, so its actions keep working.
        assert_eq!(groups[0].root, "/work/app");
        assert_eq!(groups[0].worktrees, listed.worktrees);
        // There is no error rather than the stale message.
        assert_eq!(groups[0].error, None);
    }

    // src/features/vcs/worktrees.test.ts › groupWorktrees › keeps errors from roots whose repository was not listed by another root
    #[test]
    fn keeps_errors_from_roots_whose_repository_was_not_listed_by_another_root() {
        let broken = failed("/work/other-feature", "permission denied");
        let listed = loaded("/work/app", &[worktree("/work/app", true), worktree("/work/app-feature", false)]);
        assert_eq!(group_worktrees(&[broken.clone(), listed.clone()]), [listed, broken.clone()]);
        assert_eq!(group_worktrees(std::slice::from_ref(&broken)), [broken]);
    }

    // src/features/vcs/worktrees.test.ts › worktreeContainsProject › compares %s with project %s
    #[test]
    fn compares_a_worktree_with_the_project() {
        let cases = [
            ("/p/front", Some("/p/front"), true),
            ("/p/front/", Some("/p/front/src"), true),
            ("/p/front", Some("/p/front-wt"), false),
            ("/p/front/linked", Some("/p/front"), false),
            ("/", Some("/p/front"), true),
            (r"C:\Work\App", Some("c:/work/app/src"), true),
            ("C:/Work/App/", Some("c:/work/app-wt"), false),
            ("/Work/App", Some("/work/app"), false),
            ("/p/front", None, false),
        ];
        for (path, root, expected) in cases {
            assert_eq!(worktree_contains_project(path, root), expected, "{path} with project {root:?}");
        }
    }
}
