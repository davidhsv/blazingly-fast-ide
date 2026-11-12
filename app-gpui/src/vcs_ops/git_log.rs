//! The Git Log's commit context menu.

use super::branch_menu::{BranchCommand, branch_menu};
use super::log_store::{GitLog, RepoCommit};
use super::menu::MenuItem;
use super::store::VcsState;
use crate::platform::i18n::{I18n, Param};
use crate::vcs::branches::is_favorite_branch;
use serde_json::{Value, json};

/// What choosing a row of the commit menu does. Commits are named by their key in the log.
#[derive(Clone, Debug)]
pub enum LogCommand {
    CopyCommitMessage(String),
    /// Bookmark the commit, or remove its bookmark.
    ToggleBookmark(String),
    /// Open the mnemonic chooser for the commit's bookmark.
    AssignMnemonic(String),
    BrowseRepositoryAt(String),
    /// A row of a branch's menu, to run with repository `repo` selected (`None`: as it is).
    Branch {
        repo: Option<String>,
        command: BranchCommand,
    },
}

type Item = MenuItem<LogCommand>;

/// What the menu reads besides the commits.
#[derive(Clone, Copy, Debug)]
pub struct CommitMenuContext<'a> {
    pub log: &'a GitLog,
    pub vcs: &'a VcsState,
    /// The commit the menu is for is bookmarked.
    pub bookmarked: bool,
    /// The user's favorite branches; `None` while the defaults apply.
    pub favorites: Option<&'a [String]>,
}

/// The argument commit actions are executed with: the commit's key in the log.
pub fn commit_arg(commit: &RepoCommit) -> Value {
    json!(commit.key())
}

/// The argument of an action on several commits: their keys, newest first.
pub fn commits_arg(commits: &[RepoCommit]) -> Value {
    Value::Array(commits.iter().map(commit_arg).collect())
}

fn act(id: &'static str, commit: &RepoCommit) -> Item {
    Item::action_with(id, commit_arg(commit))
}

fn bookmark_item(key: &str, bookmarked: bool) -> Item {
    let label = if bookmarked { "Remove Commit Bookmark" } else { "Bookmark Commit" };
    Item::item(label, LogCommand::ToggleBookmark(key.to_owned()))
}

/// IDEA's Git Log commit context menu for `commit`; `selection` holds all selected commits, newest
/// first (the commit alone when it is not among them).
pub fn commit_menu(
    commit: &RepoCommit,
    selection: &[RepoCommit],
    context: CommitMenuContext,
    i18n: &mut I18n,
) -> Vec<Item> {
    let key = commit.key();
    let copy_message = || Item::item("Copy Commit Message", LogCommand::CopyCommitMessage(key.clone()));
    let mnemonic = || Item::item("Assign Mnemonic", LogCommand::AssignMnemonic(key.clone()));
    let browse = || Item::item("Browse Repository at Revision", LogCommand::BrowseRepositoryAt(key.clone()));
    if commit.read_only {
        return vec![
            act("vcs.log.copyRevision", commit),
            copy_message(),
            bookmark_item(&key, context.bookmarked),
            mnemonic(),
            act("vcs.log.goToParent", commit),
            act("vcs.log.goToChild", commit),
            browse(),
        ];
    }
    let CommitMenuContext { log, vcs, .. } = context;
    // In a log of the selected repository alone, a commit is pushed when it is not among the
    // outgoing ones; in a log of several, when its own repository says so.
    let pushed = |commit: &RepoCommit| match commit.repo {
        None => log.outgoing.as_ref().is_some_and(|outgoing| !outgoing.contains(&commit.commit.id)),
        Some(_) => log.is_pushed(&commit.key()),
    };
    if selection.len() > 1 && selection.iter().any(|selected| selected.key() == key) {
        let any_pushed = selection.iter().any(pushed);
        let all = |id: &'static str| Item::action_with(id, commits_arg(selection));
        let mut items = vec![
            all("vcs.log.copyRevision").labelled("Copy Revision Numbers"),
            all("vcs.log.createPatch"),
            all("vcs.log.cherryPick"),
        ];
        if selection.len() == 2 {
            items.push(all("vcs.log.compareVersions"));
        }
        items.extend([
            Item::Separator,
            all("vcs.log.revert").labelled("Revert Commits"),
            Item::Separator,
            all("vcs.log.squashCommits").disabled(any_pushed),
            all("vcs.log.dropCommit").labelled("Drop Commits").disabled(any_pushed),
        ]);
        return items;
    }
    let repo = commit.repo.as_deref().filter(|repo| !repo.is_empty());
    let head = match repo {
        Some(repo) => vcs.heads.get(repo).and_then(|head| head.head.as_deref()),
        None => vcs.branch.as_ref().and_then(|branch| branch.head.as_deref()),
    };
    let head = head.filter(|head| !head.is_empty());
    let pushed_commit = pushed(commit);
    // Without an upstream there is no outgoing set: pushing up to any commit of HEAD is allowed.
    let tracked = match commit.repo {
        None => log.outgoing.is_some(),
        Some(_) => log.tracked.contains(repo.unwrap_or_default()),
    };
    let outgoing = log.outgoing.as_ref().is_some_and(|outgoing| outgoing.contains(&key));
    let can_push_here = head.is_some() && (!tracked || outgoing);
    let mut branch_items: Vec<Item> = Vec::new();
    for branch in commit.ref_branches.iter().filter(|branch| !branch.current) {
        let favorite = is_favorite_branch(&branch.name, branch.remote, context.favorites);
        let in_repo = |command| LogCommand::Branch { repo: repo.map(str::to_owned), command };
        let rows = branch_menu(branch, head, &[], favorite, i18n);
        let menu = rows.into_iter().map(|item| item.map(&in_repo)).collect();
        let label = i18n.tx_built(&["Branch '", "'"], &[Param::from(branch.name.as_str())]);
        branch_items.push(Item::submenu(label, menu));
    }
    if branch_items.is_empty() {
        let strings = ["Rebase '", "' onto Selected Commit"];
        let label = i18n.tx_built(&strings, &[Param::from(head.unwrap_or("HEAD"))]);
        branch_items.push(act("vcs.log.rebaseOntoCommit", commit).labelled(label).disabled(head.is_none()));
    }
    if branch_items.len() > 2 {
        branch_items = vec![Item::submenu("Branches", branch_items)];
    }
    let mut items = vec![
        act("vcs.log.copyRevision", commit),
        bookmark_item(&key, context.bookmarked),
        copy_message(),
        act("vcs.log.createPatch", commit),
        act("vcs.log.cherryPick", commit),
        Item::Separator,
        act("vcs.log.checkoutRevision", commit),
        browse(),
        act("vcs.log.compareWithLocal", commit),
        Item::Separator,
        act("vcs.log.resetToHere", commit),
        act("vcs.log.revert", commit),
        act("vcs.log.undoCommit", commit),
        Item::Separator,
        act("vcs.log.editMessage", commit).disabled(pushed_commit),
        act("vcs.log.fixup", commit).disabled(pushed_commit),
        act("vcs.log.squashInto", commit).disabled(pushed_commit),
        act("vcs.log.dropCommit", commit).disabled(pushed_commit),
        act("vcs.log.interactiveRebase", commit).disabled(pushed_commit),
        act("vcs.log.pushUpToHere", commit).disabled(!can_push_here),
        Item::Separator,
    ];
    items.extend(branch_items);
    items.extend([
        act("vcs.log.newBranch", commit),
        act("vcs.log.newTag", commit),
        Item::Separator,
        mnemonic(),
        act("vcs.log.goToChild", commit),
        act("vcs.log.goToParent", commit).disabled(commit.commit.parents.is_empty()),
        Item::Separator,
        act("vcs.log.openInBrowser", commit),
    ]);
    items
}

#[cfg(test)]
mod tests {
    use super::super::test_support;
    use super::*;
    use ide_vcs::Branch;
    use std::collections::HashSet;

    fn commit(summary: &str, id: &str) -> RepoCommit {
        RepoCommit::from(test_support::commit(id, &[], summary))
    }

    fn menu(commit: &RepoCommit, selection: &[RepoCommit], log: &GitLog) -> Vec<Item> {
        let vcs = VcsState::default();
        let context = CommitMenuContext { log, vcs: &vcs, bookmarked: false, favorites: None };
        commit_menu(commit, selection, context, &mut I18n::default())
    }

    fn ids(items: &[Item]) -> Vec<&'static str> {
        items.iter().filter_map(Item::action_id).collect()
    }

    fn disabled(items: &[Item]) -> Vec<&'static str> {
        items.iter().filter(|item| item.is_disabled()).filter_map(Item::action_id).collect()
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › lists IDEA's commit context menu actions, all bound to the commit
    #[test]
    fn lists_ideas_commit_context_menu_actions_all_bound_to_the_commit() {
        let c = commit("Fix it", "0123456789abcdef0123456789abcdef01234567");
        let items = menu(&c, std::slice::from_ref(&c), &GitLog::default());
        assert_eq!(
            ids(&items),
            [
                "vcs.log.copyRevision",
                "vcs.log.createPatch",
                "vcs.log.cherryPick",
                "vcs.log.checkoutRevision",
                "vcs.log.compareWithLocal",
                "vcs.log.resetToHere",
                "vcs.log.revert",
                "vcs.log.undoCommit",
                "vcs.log.editMessage",
                "vcs.log.fixup",
                "vcs.log.squashInto",
                "vcs.log.dropCommit",
                "vcs.log.interactiveRebase",
                "vcs.log.pushUpToHere",
                "vcs.log.rebaseOntoCommit",
                "vcs.log.newBranch",
                "vcs.log.newTag",
                "vcs.log.goToChild",
                "vcs.log.goToParent",
                "vcs.log.openInBrowser",
            ]
        );
        let bound = commit_arg(&c);
        assert!(items.iter().filter(|item| item.action_id().is_some()).all(|item| item.arg() == Some(&bound)));
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › disables history rewriting for pushed commits
    #[test]
    fn disables_history_rewriting_for_pushed_commits() {
        let local = commit("Local", &"1".repeat(40));
        let pushed = commit("Pushed", &"2".repeat(40));
        let mut log = GitLog::default();
        log.outgoing.get_or_insert_with(HashSet::new).insert(local.commit.id.clone());
        let off = |commit: &RepoCommit| disabled(&menu(commit, std::slice::from_ref(commit), &log));
        assert!(!off(&local).contains(&"vcs.log.editMessage"));
        let rewriting = [
            "vcs.log.editMessage",
            "vcs.log.fixup",
            "vcs.log.squashInto",
            "vcs.log.dropCommit",
            "vcs.log.interactiveRebase",
            "vcs.log.pushUpToHere",
        ];
        let off = off(&pushed);
        assert!(rewriting.iter().all(|id| off.contains(id)), "{off:?}");
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › offers the multi-commit actions for a multiple selection
    #[test]
    fn offers_the_multi_commit_actions_for_a_multiple_selection() {
        let a = commit("A", &"a".repeat(40));
        let b = commit("B", &"b".repeat(40));
        let c = commit("C", &"c".repeat(40));
        let log = GitLog::default();
        let two = [a.clone(), b.clone()];
        assert_eq!(
            ids(&menu(&a, &two, &log)),
            [
                "vcs.log.copyRevision",
                "vcs.log.createPatch",
                "vcs.log.cherryPick",
                "vcs.log.compareVersions",
                "vcs.log.revert",
                "vcs.log.squashCommits",
                "vcs.log.dropCommit",
            ]
        );
        // Compare Versions needs exactly two commits (IDEA).
        let three = ids(&menu(&a, &[a.clone(), b, c], &log));
        assert!(!three.contains(&"vcs.log.compareVersions"));
        assert!(three.contains(&"vcs.log.cherryPick"));
    }

    // Not from a spec: the branches at a commit bring their own menus, run in the commit's repository.
    #[test]
    fn nests_the_menus_of_the_branches_at_the_commit() {
        let mut c = commit("Tip", &"d".repeat(40));
        c.repo = Some("/p/sub".to_owned());
        let current = Branch { current: true, ..test_support::branch("main") };
        c.ref_branches = vec![current, test_support::branch("topic")];
        let items = menu(&c, std::slice::from_ref(&c), &GitLog::default());
        let Some(Item::Item(branch)) = items.iter().find(|item| item.label() == Some("Branch 'topic'")) else {
            panic!("expected the branch's submenu");
        };
        let Some(LogCommand::Branch { repo, command }) = branch.submenu[0].command() else {
            panic!("expected a row of the branch's menu");
        };
        assert_eq!(repo.as_deref(), Some("/p/sub"));
        assert!(matches!(command, BranchCommand::Checkout(name) if name == "topic"));
        assert!(!ids(&items).contains(&"vcs.log.rebaseOntoCommit"));

        let read_only = RepoCommit { read_only: true, ..c };
        let items = menu(&read_only, std::slice::from_ref(&read_only), &GitLog::default());
        assert_eq!(ids(&items), ["vcs.log.copyRevision", "vcs.log.goToParent", "vcs.log.goToChild"]);
    }
}
