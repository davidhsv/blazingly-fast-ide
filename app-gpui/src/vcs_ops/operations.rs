//! Operations in progress (merge, rebase, cherry-pick, revert) and Git Log commit actions: their
//! controls and wording, tag names, the order commits are applied in, and what a comparison shows.

use super::branch_ops::has_forbidden_ref_char;
use super::log_store::RepoCommit;
use crate::platform::i18n::{I18n, Param};
use ide_vcs::{Commit, UNSTAGED_CHANGES_PREVENT_REBASE};
use std::cmp::Ordering;
use std::collections::HashMap;

/// An operation git can continue or abort.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InProgress {
    Rebase,
    Merge,
    CherryPick,
    Revert,
    Am,
}

impl InProgress {
    /// The operation as the backend names it.
    pub fn id(self) -> &'static str {
        match self {
            Self::Rebase => "rebase",
            Self::Merge => "merge",
            Self::CherryPick => "cherry-pick",
            Self::Revert => "revert",
            Self::Am => "am",
        }
    }

    /// The operation in the labels of its controls.
    fn noun(self) -> &'static str {
        match self {
            Self::Rebase => "Rebase",
            Self::Merge => "Merge",
            Self::CherryPick => "Cherry-Pick",
            Self::Revert => "Revert",
            Self::Am => "Patch Application",
        }
    }

    /// Banner text while the operation is stopped (English source text).
    pub fn banner(self) -> &'static str {
        match self {
            Self::Rebase => "Rebase in progress",
            Self::Merge => "Merge in progress",
            Self::CherryPick => "Cherry-pick in progress",
            Self::Revert => "Revert in progress",
            Self::Am => "Patch application in progress",
        }
    }
}

/// The operation a repository is stopped in, when it is one git can continue or abort.
pub fn in_progress(op: Option<&str>) -> Option<InProgress> {
    match op? {
        "rebase" => Some(InProgress::Rebase),
        "merge" => Some(InProgress::Merge),
        "cherry-pick" => Some(InProgress::CherryPick),
        "revert" => Some(InProgress::Revert),
        "am" => Some(InProgress::Am),
        _ => None,
    }
}

/// "Rebasing main", "Merging", "Cherry-picking"… for the branch widget and banner.
pub fn operation_title(op: InProgress, head: Option<&str>, i18n: &mut I18n) -> String {
    let Some(head) = head.filter(|head| !head.is_empty()) else {
        return i18n.tx(match op {
            InProgress::Rebase => "Rebasing",
            InProgress::Merge => "Merging",
            InProgress::CherryPick => "Cherry-picking",
            InProgress::Revert => "Reverting",
            InProgress::Am => "Applying patches",
        });
    };
    let prefix = match op {
        InProgress::Rebase => "Rebasing ",
        InProgress::Merge => "Merging ",
        InProgress::CherryPick => "Cherry-picking ",
        InProgress::Revert => "Reverting ",
        InProgress::Am => "Applying patches to ",
    };
    i18n.tx_built(&[prefix, ""], &[Param::from(head)])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Control {
    Continue,
    Skip,
    Abort,
}

/// What a stopped operation offers, in IDEA's order. A merge is concluded by committing.
pub fn operation_controls(op: InProgress) -> &'static [Control] {
    match op {
        InProgress::Merge => &[Control::Abort],
        InProgress::Rebase | InProgress::Am => &[Control::Continue, Control::Skip, Control::Abort],
        InProgress::CherryPick | InProgress::Revert => &[Control::Continue, Control::Abort],
    }
}

/// Label of a control (English source text).
pub fn control_label(op: InProgress, control: Control) -> String {
    match control {
        Control::Skip => "Skip Commit".to_owned(),
        Control::Continue => format!("Continue {}", op.noun()),
        Control::Abort => format!("Abort {}", op.noun()),
    }
}

/// A link of the balloon of an operation that failed and is still stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoppedAction {
    /// Stages the tracked files' changes and continues.
    StageAndRetry,
    /// Shows the local changes.
    ShowFiles,
    Abort,
}

impl StoppedAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::StageAndRetry => "Stage and Retry",
            Self::ShowFiles => "Show Files",
            Self::Abort => "Abort",
        }
    }
}

/// Links of the balloon of operation `op` that failed with `message`: Abort, and for a rebase held
/// up by unstaged changes (the backend's [`UNSTAGED_CHANGES_PREVENT_REBASE`]), Stage and Retry and
/// Show Files. None once nothing is in progress any
/// more (`stopped` is off).
pub fn stopped_actions(op: InProgress, message: &str, stopped: bool) -> Vec<StoppedAction> {
    if !stopped {
        Vec::new()
    } else if op == InProgress::Rebase && message.contains(UNSTAGED_CHANGES_PREVENT_REBASE) {
        vec![StoppedAction::StageAndRetry, StoppedAction::ShowFiles, StoppedAction::Abort]
    } else {
        vec![StoppedAction::Abort]
    }
}

/// Commits newest first in the log's topological order (author dates lie after rebases and
/// cherry-picks). Commits not loaded in `log` come after those that are, newest first by date.
pub fn newest_first(commits: &[RepoCommit], log: &[RepoCommit]) -> Vec<RepoCommit> {
    let position: HashMap<String, usize> = log.iter().enumerate().map(|(i, commit)| (commit.key(), i)).collect();
    let mut out = commits.to_vec();
    out.sort_by(|a, b| match (position.get(&a.key()), position.get(&b.key())) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b.commit.time_ms.total_cmp(&a.commit.time_ms),
    });
    out
}

/// Why `name` cannot name a new tag (`existing`: the tags there are), or `None`.
pub fn validate_tag_name(name: &str, existing: &[String]) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return Some("Tag name must not be empty".to_owned());
    }
    let malformed = has_forbidden_ref_char(name)
        || name.starts_with('-')
        || name.ends_with('.')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("//")
        || name.contains("@{")
        || name == "@";
    if malformed {
        return Some("Invalid tag name".to_owned());
    }
    existing.iter().any(|tag| tag == name).then(|| format!("Tag '{name}' already exists"))
}

/// One side of a comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revision {
    pub rev: String,
    pub label: String,
}

/// The selection a comparison is limited to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComparisonScope {
    pub paths: Vec<String>,
    /// The repositories compared; `None` = every repository the paths lie in.
    pub repositories: Option<Vec<String>>,
}

/// Files that differ between `from` and `to` (`None`: the working tree).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub from: Revision,
    pub to: Option<Revision>,
    pub scope: Option<ComparisonScope>,
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn revision(rev: &str, label: &str) -> Revision {
    Revision { rev: rev.to_owned(), label: label.to_owned() }
}

/// Files that differ between `rev` (a commit or branch) and the working tree.
pub fn diff_with_working_tree(rev: &str, label: &str) -> Comparison {
    Comparison { from: revision(rev, label), to: None, scope: None }
}

/// Compare with Local: files that differ between the commit and the working tree.
pub fn compare_with_local(commit: &Commit) -> Comparison {
    diff_with_working_tree(&commit.id, &short(&commit.id))
}

/// Compare Versions of two log commits: files that differ between them, older first.
pub fn compare_versions(older: &Commit, newer: &Commit) -> Comparison {
    let side = |commit: &Commit| revision(&commit.id, &short(&commit.id));
    Comparison { from: side(older), to: Some(side(newer)), scope: None }
}

/// Files that differ between two revisions (e.g. two branches from Compare with Current).
pub fn compare_revisions(from: &str, to: &str, from_label: &str, to_label: &str) -> Comparison {
    Comparison { from: revision(from, from_label), to: Some(revision(to, to_label)), scope: None }
}

/// The files below `paths` that differ between `rev` and the working tree.
pub fn path_comparison(paths: &[String], rev: &str, repositories: Option<Vec<String>>) -> Comparison {
    let scope = ComparisonScope { paths: paths.to_vec(), repositories };
    Comparison { from: revision(rev, rev), to: None, scope: Some(scope) }
}

/// What a comparison without differences says.
pub fn comparison_empty(revision: &str, repositories: usize, i18n: &mut I18n) -> String {
    if repositories == 1 {
        i18n.tx_built(&["No changes against ", " in 1 repository"], &[Param::from(revision)])
    } else {
        let values = [Param::from(revision), Param::from(repositories)];
        i18n.tx_built(&["No changes against ", " in ", " repositories"], &values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(id: &str, time_ms: f64) -> RepoCommit {
        RepoCommit::from(Commit { time_ms, ..super::super::test_support::commit(id, &[], id) })
    }

    fn ids(commits: &[RepoCommit]) -> Vec<&str> {
        commits.iter().map(|commit| commit.commit.id.as_str()).collect()
    }

    // src/features/vcs/__tests__/historyOps.test.ts › operations in progress › recognises the operations git can continue or abort
    #[test]
    fn recognises_the_operations_git_can_continue_or_abort() {
        assert_eq!(in_progress(Some("rebase")), Some(InProgress::Rebase));
        assert_eq!(in_progress(Some("cherry-pick")), Some(InProgress::CherryPick));
        assert_eq!(in_progress(Some("bisect")), None);
        assert_eq!(in_progress(None), None);
    }

    // src/features/vcs/__tests__/historyOps.test.ts › operations in progress › offers IDEA's controls and wording
    #[test]
    fn offers_ideas_controls_and_wording() {
        let mut i18n = I18n::default();
        assert_eq!(operation_controls(InProgress::Rebase), [Control::Continue, Control::Skip, Control::Abort]);
        assert_eq!(operation_controls(InProgress::Merge), [Control::Abort]);
        assert_eq!(operation_controls(InProgress::CherryPick), [Control::Continue, Control::Abort]);
        assert_eq!(control_label(InProgress::Rebase, Control::Continue), "Continue Rebase");
        assert_eq!(control_label(InProgress::CherryPick, Control::Abort), "Abort Cherry-Pick");
        assert_eq!(control_label(InProgress::Rebase, Control::Skip), "Skip Commit");
        assert_eq!(operation_title(InProgress::Rebase, Some("feature"), &mut i18n), "Rebasing feature");
        assert_eq!(operation_title(InProgress::Merge, None, &mut i18n), "Merging");
    }

    // src/features/vcs/__tests__/gitOps.test.ts › log ordering and staleness › orders commits by their position in the log, not by date
    #[test]
    fn orders_commits_by_their_position_in_the_log_not_by_date() {
        let log = [commit("new", 0.0), commit("mid", 0.0), commit("old", 0.0)];
        let picked = [commit("old", 3.0), commit("new", 1.0)];
        assert_eq!(ids(&newest_first(&picked, &log)), ["new", "old"]);
        assert_eq!(ids(&newest_first(&[commit("a", 0.0), commit("b", 5.0)], &[])), ["b", "a"]);
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › validates tag names like git check-ref-format
    #[test]
    fn validates_tag_names_like_git_check_ref_format() {
        assert_eq!(validate_tag_name("v1.0", &[]), None);
        assert_eq!(validate_tag_name("release/2024.1", &[]), None);
        assert!(validate_tag_name("", &[]).unwrap().contains("empty"));
        assert!(validate_tag_name("bad name", &[]).unwrap().contains("Invalid"));
        assert!(validate_tag_name("a..b", &[]).unwrap().contains("Invalid"));
        assert!(validate_tag_name("-x", &[]).unwrap().contains("Invalid"));
        assert!(validate_tag_name("v1", &["v1".to_owned()]).unwrap().contains("already exists"));
    }

    // Not from a spec: the links of a stopped operation's failure and the comparisons the log opens.
    #[test]
    fn offers_stage_and_retry_for_a_rebase_held_up_by_unstaged_changes() {
        let held = format!("error: {UNSTAGED_CHANGES_PREVENT_REBASE}.");
        let labels: Vec<&str> =
            stopped_actions(InProgress::Rebase, &held, true).iter().map(|action| action.label()).collect();
        assert_eq!(labels, ["Stage and Retry", "Show Files", "Abort"]);
        assert_eq!(stopped_actions(InProgress::Merge, &held, true), [StoppedAction::Abort]);
        assert!(stopped_actions(InProgress::Rebase, &held, false).is_empty());

        let local = compare_with_local(&commit("0123456789abcdef", 0.0).commit);
        assert_eq!(local.from, revision("0123456789abcdef", "01234567"));
        assert_eq!(local.to, None);
        let mut i18n = I18n::default();
        assert_eq!(comparison_empty("main", 1, &mut i18n), "No changes against main in 1 repository");
        assert_eq!(comparison_empty("main", 3, &mut i18n), "No changes against main in 3 repositories");
    }
}
