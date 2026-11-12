//! Compare with Branch or Revision… and Show Diff on a selection of files and folders.
//!
//! The caller asks the backend for the selection's [`CompareScope`], then: [`compare_paths`] says
//! whether to report, ask for a revision or go on with a given one, and [`compare_target`] what to
//! open for the revision chosen.

use super::operations::{Comparison, path_comparison};
use super::ops::vcs_notice;
use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::ipc_path::to_ipc;
use ide_vcs::CompareScope;

/// Project-relative path for labels.
pub fn display_path<'a>(path: &'a str, root: Option<&str>) -> &'a str {
    root.filter(|root| !root.is_empty())
        .and_then(|root| path.strip_prefix(root))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(path)
}

/// The revision to compare with and the repositories it was chosen for (`None`: all of them).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevisionChoice {
    pub rev: String,
    pub repositories: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareStep {
    /// The selection lies outside every repository: report it and stop.
    Notify(NotificationRequest),
    /// Ask for a branch or revision of `repositories` (tags included with `tags`).
    Choose { title: String, repositories: Vec<String>, tags: bool },
    /// The revision was given: go on with it.
    Chosen(RevisionChoice),
}

fn outside_repositories(scope: &CompareScope, project_root: Option<&str>) -> NotificationRequest {
    let mut notice = vcs_notice(NotificationLevel::Info, "Selection is outside Git repositories");
    let skipped: Vec<String> =
        scope.skipped_paths.iter().map(|path| display_path(&to_ipc(path), project_root).to_owned()).collect();
    notice.body = Some(skipped.join("\n"));
    notice
}

/// First step of comparing a selection with `revision` (asked for when `None`).
pub fn compare_paths(
    scope: &CompareScope,
    revision: Option<&str>,
    project_root: Option<&str>,
    i18n: &I18n,
) -> CompareStep {
    if scope.repositories.is_empty() {
        return CompareStep::Notify(outside_repositories(scope, project_root));
    }
    match revision.filter(|revision| !revision.is_empty()) {
        Some(revision) => CompareStep::Chosen(RevisionChoice { rev: revision.to_owned(), repositories: None }),
        None => CompareStep::Choose {
            title: i18n.tx("Compare with Branch or Revision"),
            repositories: scope.repositories.iter().map(|root| to_ipc(root)).collect(),
            tags: true,
        },
    }
}

/// What a comparison of a selection opens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompareTarget {
    /// One file's diff: revision `rev` (labelled `label`) on the left, the working tree on the right.
    FileDiff { path: String, rev: String, label: String },
    /// The tree of files that differ.
    Paths(Comparison),
}

/// What to open for `choice`. `asked`: the revision was chosen in the picker (a single file then
/// keeps its direct diff).
pub fn compare_target(paths: &[String], scope: &CompareScope, choice: &RevisionChoice, asked: bool) -> CompareTarget {
    if let Some(path) = paths.first().filter(|_| scope.single_file && asked) {
        return CompareTarget::FileDiff { path: path.clone(), rev: choice.rev.clone(), label: choice.rev.clone() };
    }
    // A shared branch reviews the selection, including explicit errors for roots lacking it.
    let repositories = choice.repositories.clone().filter(|repositories| repositories.len() == 1);
    CompareTarget::Paths(path_comparison(paths, &choice.rev, repositories))
}

/// What Show Diff does on a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DirectoryDiff {
    /// A single file: its own Show Diff behaviour is retained.
    SingleFile,
    Notify(NotificationRequest),
    /// The tree of files that differ from HEAD.
    Paths(Comparison),
}

pub fn directory_working_diff(paths: &[String], scope: &CompareScope, project_root: Option<&str>) -> DirectoryDiff {
    if scope.single_file {
        DirectoryDiff::SingleFile
    } else if scope.repositories.is_empty() {
        DirectoryDiff::Notify(outside_repositories(scope, project_root))
    } else {
        DirectoryDiff::Paths(path_comparison(paths, "HEAD", None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn scope(repositories: &[&str], single_file: bool, skipped: &[&str]) -> CompareScope {
        fn paths(values: &[&str]) -> Vec<ide_core::IpcPath> {
            values.iter().map(|value| ide_core::IpcPath::from(*value)).collect()
        }
        CompareScope { repositories: paths(repositories), single_file, skipped_paths: paths(skipped) }
    }

    fn choice(rev: &str, repositories: &[&str]) -> RevisionChoice {
        RevisionChoice { rev: rev.to_owned(), repositories: Some(strings(repositories)) }
    }

    // src/features/vcs/__tests__/compareFlow.test.ts › comparison flow › keeps a single file's direct revision → worktree diff and uses its own repository's picker
    #[test]
    fn keeps_a_single_files_direct_diff_and_uses_its_own_repositorys_picker() {
        let scope = scope(&["/server"], true, &[]);
        let paths = strings(&["/server/a.ts"]);
        assert_eq!(
            compare_paths(&scope, None, None, &I18n::default()),
            CompareStep::Choose {
                title: "Compare with Branch or Revision".to_owned(),
                repositories: strings(&["/server"]),
                tags: true,
            }
        );
        let main = "main".to_owned();
        assert_eq!(
            compare_target(&paths, &scope, &choice("main", &["/server"]), true),
            CompareTarget::FileDiff { path: "/server/a.ts".to_owned(), rev: main.clone(), label: main }
        );
    }

    // src/features/vcs/__tests__/compareFlow.test.ts › comparison flow › applies shared revisions across the selection so missing revisions are reported, and preserves repo-specific choices
    #[test]
    fn applies_shared_revisions_across_the_selection_and_preserves_repo_specific_choices() {
        let scope = scope(&["/client", "/server", "/admin"], false, &[]);
        let paths = strings(&["/workspace"]);
        assert_eq!(
            compare_target(&paths, &scope, &choice("main", &["/client", "/server"]), true),
            CompareTarget::Paths(path_comparison(&paths, "main", None))
        );
        assert_eq!(
            compare_target(&paths, &scope, &choice("feature", &["/server"]), true),
            CompareTarget::Paths(path_comparison(&paths, "feature", Some(strings(&["/server"]))))
        );
    }

    // src/features/vcs/__tests__/compareFlow.test.ts › comparison flow › reports non-repository selections before opening a picker
    #[test]
    fn reports_non_repository_selections_before_opening_a_picker() {
        let scope = scope(&[], false, &["/workspace/loose/"]);
        let CompareStep::Notify(notice) = compare_paths(&scope, None, Some("/workspace"), &I18n::default()) else {
            panic!("expected a notification");
        };
        assert_eq!(notice.title, "Selection is outside Git repositories");
        assert_eq!(notice.body.as_deref(), Some("loose/"));
    }

    // src/features/vcs/__tests__/compareFlow.test.ts › comparison flow › reuses the tree against HEAD for Show Diff on folders
    #[test]
    fn reuses_the_tree_against_head_for_show_diff_on_folders() {
        let paths = strings(&["/server/src"]);
        assert_eq!(
            directory_working_diff(&paths, &scope(&["/server"], false, &[]), None),
            DirectoryDiff::Paths(path_comparison(&paths, "HEAD", None))
        );
    }

    // Not from a spec: a given revision skips the picker and compares every repository.
    #[test]
    fn a_given_revision_skips_the_picker() {
        let scope = scope(&["/server"], true, &[]);
        let step = compare_paths(&scope, Some("v1"), None, &I18n::default());
        let CompareStep::Chosen(chosen) = step else { panic!("expected the given revision") };
        let paths = strings(&["/server/a.ts"]);
        assert_eq!(
            compare_target(&paths, &scope, &chosen, false),
            CompareTarget::Paths(path_comparison(&paths, "v1", None))
        );
        assert_eq!(directory_working_diff(&paths, &scope, None), DirectoryDiff::SingleFile);
    }
}
