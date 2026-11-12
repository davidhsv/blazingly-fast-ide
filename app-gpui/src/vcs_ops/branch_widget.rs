//! The branch widget: how it names HEAD, and what the Branches popup puts on top while an operation
//! is stopped.

use super::operations::InProgress;
use super::store::RepoHead;
use crate::platform::i18n::I18n;
use crate::vcs::branches::PopupAction;

/// The widget's text for a repository's HEAD: its branch, else the short commit.
pub fn head_label(branch: Option<&RepoHead>, i18n: &I18n) -> String {
    let Some(branch) = branch else { return String::new() };
    let given = |value: &Option<String>| value.clone().filter(|value| !value.is_empty());
    given(&branch.head)
        .or_else(|| given(&branch.head_commit).map(|commit| commit.chars().take(8).collect()))
        .unwrap_or_else(|| i18n.tx("No commits"))
}

fn action(id: &str, label: &str) -> PopupAction {
    PopupAction { id: id.to_owned(), label: label.to_owned(), ..PopupAction::default() }
}

/// Entries at the top of the Branches popup while an operation is stopped, or while `conflicts`
/// files are left to resolve.
pub fn operation_actions(op: Option<InProgress>, conflicts: usize) -> Vec<PopupAction> {
    let mut out = Vec::new();
    // Conflicts are resolved whether or not an operation is stopped: applying a stash, or the
    // autostash of a pull, leaves them with none in progress.
    if conflicts > 0 {
        let resolve = action("vcs.resolveConflicts", "Resolve Conflicts…");
        out.push(PopupAction { icon: Some("warning".to_owned()), ..resolve });
    }
    let controls: &[(&str, &str)] = match op {
        None | Some(InProgress::Am) => &[],
        Some(InProgress::Rebase) => &[
            ("vcs.rebase.continue", "Continue Rebase"),
            ("vcs.rebase.skip", "Skip Commit"),
            ("vcs.rebase.abort", "Abort Rebase"),
        ],
        Some(InProgress::Merge) => &[("vcs.merge.abort", "Abort Merge")],
        Some(InProgress::CherryPick) => {
            &[("vcs.cherryPick.continue", "Continue Cherry-Pick"), ("vcs.cherryPick.abort", "Abort Cherry-Pick")]
        }
        Some(InProgress::Revert) => &[("vcs.revert.continue", "Continue Revert"), ("vcs.revert.abort", "Abort Revert")],
    };
    out.extend(controls.iter().map(|(id, label)| action(id, label)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(actions: &[PopupAction]) -> Vec<&str> {
        actions.iter().map(|action| action.label.as_str()).collect()
    }

    fn ids(actions: &[PopupAction]) -> Vec<&str> {
        actions.iter().map(|action| action.id.as_str()).collect()
    }

    // src/features/vcs/__tests__/historyOps.test.ts › operations in progress › puts the controls at the top of the Branches popup
    #[test]
    fn puts_the_controls_at_the_top_of_the_branches_popup() {
        assert!(operation_actions(None, 0).is_empty());
        assert_eq!(
            labels(&operation_actions(Some(InProgress::Rebase), 2)),
            ["Resolve Conflicts…", "Continue Rebase", "Skip Commit", "Abort Rebase"]
        );
        assert_eq!(ids(&operation_actions(Some(InProgress::Merge), 0)), ["vcs.merge.abort"]);
    }

    // src/features/vcs/__tests__/historyOps.test.ts › operations in progress › offers Resolve Conflicts when conflicts remain with no operation in progress
    #[test]
    fn offers_resolve_conflicts_when_conflicts_remain_with_no_operation_in_progress() {
        // Applying a stash, or the autostash of a pull, leaves conflicts and no merge or rebase.
        assert_eq!(ids(&operation_actions(None, 1)), ["vcs.resolveConflicts"]);
    }

    // Not from a spec: the widget names a detached HEAD by its short commit.
    #[test]
    fn names_head_by_branch_then_commit() {
        let i18n = I18n::default();
        let head = |head: Option<&str>, commit: Option<&str>| RepoHead {
            head: head.map(str::to_owned),
            head_commit: commit.map(str::to_owned),
        };
        assert_eq!(head_label(Some(&head(Some("main"), Some("0123456789ab"))), &i18n), "main");
        assert_eq!(head_label(Some(&head(None, Some("0123456789ab"))), &i18n), "01234567");
        assert_eq!(head_label(Some(&head(None, None)), &i18n), "No commits");
        assert_eq!(head_label(None, &i18n), "");
    }
}
