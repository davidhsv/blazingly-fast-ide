//! Context menu of a branch, in the Branches popup, the Branches pane and the Git Log.

use super::branch_ops::branch_ref;
use super::log_pane_actions::BranchTarget;
use super::menu::MenuItem;
use crate::platform::i18n::{I18n, Param};
use ide_host::ops::vcs::RemoteOp;
use ide_vcs::{Branch, MergeOptions};
use serde_json::{Value, json};

/// What choosing a row of a branch menu does.
#[derive(Clone, Debug)]
pub enum BranchCommand {
    /// New Branch from `from` (`None`: from HEAD).
    NewBranch {
        from: Option<String>,
    },
    /// Run a registered action.
    Action {
        id: &'static str,
        arg: Option<Value>,
    },
    Rename(String),
    /// Add the branch (by name) to the favorites, or remove it.
    ToggleFavorite(String),
    Checkout(String),
    /// Check the branch out, then rebase it onto the branch that was current.
    CheckoutAndRebase(String),
    /// Compare the branch with the current one.
    Compare(String),
    ShowDiffWithWorkingTree(String),
    /// Rebase the current branch onto `branch`, given as the full ref `reference`.
    RebaseOnto {
        branch: String,
        reference: String,
    },
    /// Merge `branch`, given as the full ref `reference`, into the current branch.
    Merge {
        branch: String,
        reference: String,
    },
    /// `git pull <remote> <branch>` into the current branch ([`pull_into`]).
    PullInto {
        remote_branch: String,
        rebase: bool,
    },
    /// Fast-forward the branch to its upstream.
    Update(String),
    /// Delete the Branches pane's whole selection.
    DeleteSelection(Vec<BranchTarget>),
    DeleteRemote(String),
    Delete(String),
}

/// The pull of `remote_branch` (`origin/feat/x` → origin, feat/x) into the current branch
/// (`current`; `None` on a detached HEAD): the network operation and its detail text. `None` when
/// the name has no remote.
pub fn pull_into(remote_branch: &str, rebase: bool, current: Option<&str>) -> Option<(RemoteOp, String)> {
    let (remote, branch) = remote_branch.split_once('/').filter(|(remote, _)| !remote.is_empty())?;
    let op = RemoteOp::PullFrom {
        remote: remote.to_owned(),
        branches: vec![branch.to_owned()],
        rebase,
        options: MergeOptions::default(),
    };
    Some((op, format!("{} ← {remote_branch}", current.unwrap_or("HEAD"))))
}

type Item = MenuItem<BranchCommand>;

fn quoted(i18n: &mut I18n, before: &str, name: &str, after: &str) -> String {
    i18n.tx_built(&[before, after], &[Param::from(name)])
}

fn run_action(id: &'static str, arg: Option<Value>) -> BranchCommand {
    BranchCommand::Action { id, arg }
}

/// The menu of branch `branch`. `current`: the current branch of its repository (`None` on a
/// detached HEAD); `selected`: the Branches pane's multi-selection, which Delete acts on as a
/// whole; `favorite`: the branch is a favorite.
pub fn branch_menu(
    branch: &Branch,
    current: Option<&str>,
    selected: &[BranchTarget],
    favorite: bool,
    i18n: &mut I18n,
) -> Vec<Item> {
    let name = branch.name.as_str();
    let owned = || name.to_owned();
    let head = current.unwrap_or("HEAD");
    let favorite_label = if favorite { "Remove from Favorites" } else { "Add to Favorites" };
    let favorite_item = Item::item(favorite_label, BranchCommand::ToggleFavorite(owned())).icon("star");
    let history =
        Item::item("Show History", run_action("vcs.showLog", Some(json!({ "branch": name })))).icon("history");
    let new_branch = quoted(i18n, "New Branch from '", name, "'…");
    if branch.current {
        return vec![
            Item::item(new_branch, BranchCommand::NewBranch { from: None }).icon("branch"),
            Item::Separator,
            Item::item("Update", run_action("vcs.update", None)).icon("git-pull"),
            Item::item("Push…", run_action("vcs.push", None)).icon("git-push"),
            Item::Separator,
            Item::item("Rename…", BranchCommand::Rename(owned())),
            favorite_item,
            history,
        ];
    }
    let reference = branch_ref(name, branch.remote);
    let mut items = vec![
        Item::item("Checkout", BranchCommand::Checkout(owned())),
        Item::item(new_branch, BranchCommand::NewBranch { from: Some(owned()) }).icon("branch"),
    ];
    if current.is_some() {
        let label = quoted(i18n, "Checkout and Rebase onto '", head, "'");
        items.push(Item::item(label, BranchCommand::CheckoutAndRebase(owned())));
    }
    let both = [Param::from(head), Param::from(name)];
    items.extend([
        Item::Separator,
        Item::item(quoted(i18n, "Compare with '", head, "'"), BranchCommand::Compare(owned())).icon("diff"),
        Item::item("Show Diff with Working Tree", BranchCommand::ShowDiffWithWorkingTree(owned())),
        history,
        Item::Separator,
        Item::item(
            i18n.tx_built(&["Rebase '", "' onto '", "'"], &both),
            BranchCommand::RebaseOnto { branch: owned(), reference: reference.clone() },
        ),
        Item::item(
            i18n.tx_built(&["Merge '", "' into '", "'"], &[Param::from(name), Param::from(head)]),
            BranchCommand::Merge { branch: owned(), reference },
        ),
    ]);
    if branch.remote {
        let pull = |rebase: bool| BranchCommand::PullInto { remote_branch: name.to_owned(), rebase };
        items.push(Item::item(quoted(i18n, "Pull into '", head, "' Using Rebase"), pull(true)));
        items.push(Item::item(quoted(i18n, "Pull into '", head, "' Using Merge"), pull(false)));
    } else {
        let tracked = branch.upstream.as_deref().is_some_and(|upstream| !upstream.is_empty());
        items.extend([
            Item::Separator,
            Item::item("Update", BranchCommand::Update(owned())).icon("git-pull").disabled(!tracked),
            Item::item("Push…", run_action("vcs.push", Some(json!({ "branch": name })))).icon("git-push"),
        ]);
    }
    items.push(Item::Separator);
    if !branch.remote {
        items.push(Item::item("Rename…", BranchCommand::Rename(owned())));
    }
    items.push(favorite_item);
    let delete = if !selected.is_empty() {
        BranchCommand::DeleteSelection(selected.to_vec())
    } else if branch.remote {
        BranchCommand::DeleteRemote(owned())
    } else {
        BranchCommand::Delete(owned())
    };
    items.push(Item::item("Delete", delete).icon("trash").danger());
    items
}

#[cfg(test)]
mod tests {
    use super::super::test_support::branch;
    use super::*;

    fn labels(items: &[Item]) -> Vec<&str> {
        items.iter().map(|item| item.label().unwrap_or("-")).collect()
    }

    // Not from a spec: the rows of a local, a remote and the current branch.
    #[test]
    fn lists_the_rows_of_a_branch() {
        let mut i18n = I18n::default();
        let local = branch("topic");
        let menu = branch_menu(&local, Some("main"), &[], false, &mut i18n);
        assert_eq!(
            labels(&menu),
            [
                "Checkout",
                "New Branch from 'topic'…",
                "Checkout and Rebase onto 'main'",
                "-",
                "Compare with 'main'",
                "Show Diff with Working Tree",
                "Show History",
                "-",
                "Rebase 'main' onto 'topic'",
                "Merge 'topic' into 'main'",
                "-",
                "Update",
                "Push…",
                "-",
                "Rename…",
                "Add to Favorites",
                "Delete",
            ]
        );
        let update = menu.iter().find(|item| item.label() == Some("Update")).unwrap();
        assert!(update.is_disabled());
        let delete = menu.last().and_then(Item::command);
        assert!(matches!(delete, Some(BranchCommand::Delete(name)) if name == "topic"));
        let merges = |item: &Item| {
            matches!(
                item.command(),
                Some(BranchCommand::Merge { branch, reference }) if branch == "topic" && reference == "refs/heads/topic"
            )
        };
        assert!(menu.iter().any(merges));

        let remote = Branch { remote: true, ..branch("origin/topic") };
        let menu = branch_menu(&remote, None, &[], true, &mut i18n);
        let shown = labels(&menu);
        assert!(shown.contains(&"Pull into 'HEAD' Using Rebase") && shown.contains(&"Remove from Favorites"));
        assert!(!shown.contains(&"Rename…") && !shown.contains(&"Checkout and Rebase onto 'HEAD'"));
        let delete = menu.last().and_then(Item::command);
        assert!(matches!(delete, Some(BranchCommand::DeleteRemote(name)) if name == "origin/topic"));
        let selection = [BranchTarget { ref_name: "origin/topic".to_owned(), ..BranchTarget::default() }];
        let menu = branch_menu(&remote, None, &selection, true, &mut i18n);
        let delete = menu.last().and_then(Item::command);
        assert!(matches!(delete, Some(BranchCommand::DeleteSelection(targets)) if targets.len() == 1));

        let current = Branch { current: true, ..branch("main") };
        let menu = branch_menu(&current, Some("main"), &[], false, &mut i18n);
        assert_eq!(
            labels(&menu),
            ["New Branch from 'main'…", "-", "Update", "Push…", "-", "Rename…", "Add to Favorites", "Show History"]
        );
    }

    // Not from a spec: a remote branch is pulled from its remote, by its name there.
    #[test]
    fn pulls_a_remote_branch_into_the_current_one() {
        let (op, detail) = pull_into("origin/feat/x", true, Some("main")).unwrap();
        let RemoteOp::PullFrom { remote, branches, rebase, options } = op else { panic!("expected a pull") };
        assert_eq!((remote.as_str(), rebase), ("origin", true));
        assert_eq!(branches, ["feat/x"]);
        assert_eq!(options, MergeOptions::default());
        assert_eq!(detail, "main ← origin/feat/x");
        assert!(pull_into("local", false, None).is_none());
    }
}
