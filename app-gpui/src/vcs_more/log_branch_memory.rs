//! The main Log's branch filter, remembered for each repository: a log opens on the branches last
//! chosen there, else on the current branch.

use super::log_loading::LogLoads;
use crate::platform::storage::UiStorage;
use crate::vcs_ops::log_store::{GitLog, MAIN_LOG, resolve_log_branches};
use crate::vcs_ops::store::VcsState;
use serde_json::{Value, json};

/// Storage key of the branch filter last chosen for `repo`.
pub fn branch_memory_key(repo: &str) -> String {
    format!("openidea.vcs.logBranches:{repo}")
}

/// The main Log's branch filter last chosen for `repo`; `None` when none was.
pub fn remembered_log_branches(storage: &UiStorage, repo: &str) -> Option<Vec<String>> {
    let raw = storage.get(&branch_memory_key(repo)).filter(|raw| !raw.is_empty())?;
    let stored: Value = serde_json::from_str(raw).ok()?;
    Some(stored.as_array()?.iter().filter_map(Value::as_str).map(str::to_owned).collect())
}

fn remember_log_branches(storage: &mut UiStorage, repo: &str, branches: &[String]) {
    storage.set(&branch_memory_key(repo), json!(branches).to_string());
}

/// Settles the main Log's branch filter for the selected repository: its last choice, else the
/// current branch. `scope` is the repository whose filter is settled. Returns `false` when it was
/// settled already, when another log is shown, and in a project of several repositories, whose
/// log shows them all.
pub fn init_log_branch_filter(
    log: &mut GitLog,
    scope: &mut Option<String>,
    vcs: &VcsState,
    storage: &UiStorage,
) -> bool {
    if vcs.roots.len() > 1 || log.shown_log != MAIN_LOG {
        return false;
    }
    let Some(repo) = vcs.current_repo_root().filter(|repo| scope.as_deref() != Some(*repo)) else {
        return false;
    };
    let listed = vcs.branches.iter().filter(|branch| !branch.name.ends_with("/HEAD"));
    let names: Vec<String> = listed.map(|branch| branch.name.clone()).collect();
    log.view.filters.branches = resolve_log_branches(remembered_log_branches(storage, repo).as_deref(), &names);
    *scope = Some(repo.to_owned());
    true
}

/// The Branch filter's setter: entries with `..` are commit ranges, shown next to the branches. A
/// choice in the main Log is remembered for the repository. The shown log then has to be reloaded.
pub fn set_log_branches(
    log: &mut GitLog,
    loads: &mut LogLoads,
    scope: &mut Option<String>,
    vcs: &VcsState,
    storage: &mut UiStorage,
    branches: &[String],
) {
    loads.set_seek(&log.shown_log, None);
    let (ranges, names): (Vec<String>, Vec<String>) = branches.iter().cloned().partition(|name| name.contains(".."));
    log.view.filters.branches = names;
    log.view.filters.range = (!ranges.is_empty()).then(|| ranges.join("\n"));
    let repo = vcs.current_repo_root().filter(|_| log.shown_log == MAIN_LOG);
    if let Some(repo) = repo {
        *scope = Some(repo.to_owned());
        remember_log_branches(storage, repo, branches);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_vcs::Branch;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn branch(name: &str, remote: bool, current: bool) -> Branch {
        let wire = json!({
            "name": name,
            "remote": remote,
            "current": current,
            "ahead": 0,
            "behind": 0,
            "lastCommitMs": 0,
        });
        serde_json::from_value(wire).expect("a branch in its wire form")
    }

    /// A project and its selected repository with the given branches, and a log nothing is settled in.
    fn project(root: &str, repo: &str, branches: Vec<Branch>) -> (VcsState, GitLog, Option<String>) {
        let vcs = VcsState {
            root: Some(root.to_owned()),
            roots: vec![repo.to_owned()],
            repo: Some(repo.to_owned()),
            branches,
            branches_loaded: true,
            ..VcsState::default()
        };
        (vcs, GitLog::default(), None)
    }

    fn main_only() -> Vec<Branch> {
        vec![branch("main", false, true)]
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › resolveLogBranches › defaults to HEAD (the current branch) when nothing was remembered
    #[test]
    fn defaults_to_head_the_current_branch_when_nothing_was_remembered() {
        assert_eq!(resolve_log_branches(None, &strings(&["main", "feature"])), ["HEAD"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › resolveLogBranches › keeps a remembered local or remote branch
    #[test]
    fn keeps_a_remembered_local_or_remote_branch() {
        assert_eq!(resolve_log_branches(Some(&strings(&["feature"])), &strings(&["main", "feature"])), ["feature"]);
        let remembered = strings(&["origin/es"]);
        assert_eq!(resolve_log_branches(Some(&remembered), &strings(&["main", "origin/es"])), ["origin/es"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › resolveLogBranches › falls back to HEAD when every remembered branch is gone
    #[test]
    fn falls_back_to_head_when_every_remembered_branch_is_gone() {
        assert_eq!(resolve_log_branches(Some(&strings(&["deleted"])), &strings(&["main"])), ["HEAD"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › resolveLogBranches › keeps the remembered branches that still exist
    #[test]
    fn keeps_the_remembered_branches_that_still_exist() {
        let remembered = strings(&["deleted", "feature"]);
        assert_eq!(resolve_log_branches(Some(&remembered), &strings(&["main", "feature"])), ["feature"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › resolveLogBranches › keeps an empty list as all branches, and HEAD
    #[test]
    fn keeps_an_empty_list_as_all_branches_and_head() {
        assert!(resolve_log_branches(Some(&[]), &strings(&["main"])).is_empty());
        assert_eq!(resolve_log_branches(Some(&strings(&["HEAD"])), &strings(&["main"])), ["HEAD"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › Git Log branch filter memory › shows the current branch on first open
    #[test]
    fn shows_the_current_branch_on_first_open() {
        let (vcs, mut log, mut scope) = project("/mem-first", "/mem-first", main_only());
        let storage = UiStorage::default();
        assert!(init_log_branch_filter(&mut log, &mut scope, &vcs, &storage));
        assert_eq!(log.view.filters.branches, ["HEAD"]);
        assert!(!init_log_branch_filter(&mut log, &mut scope, &vcs, &storage));
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › Git Log branch filter memory › restores the remembered branch, dropping a deleted one
    #[test]
    fn restores_the_remembered_branch_dropping_a_deleted_one() {
        let mut storage = UiStorage::default();
        remember_log_branches(&mut storage, "/mem-restore", &strings(&["origin/es", "deleted"]));
        let branches = vec![branch("main", false, true), branch("origin/es", true, false)];
        let (vcs, mut log, mut scope) = project("/mem-restore", "/mem-restore", branches);
        init_log_branch_filter(&mut log, &mut scope, &vcs, &storage);
        assert_eq!(log.view.filters.branches, ["origin/es"]);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › Git Log branch filter memory › remembers a choice in the main Log, including all branches
    #[test]
    fn remembers_a_choice_in_the_main_log_including_all_branches() {
        let (vcs, mut log, mut scope) = project("/mem-save", "/mem-save", main_only());
        let mut storage = UiStorage::default();
        let mut loads = LogLoads::default();
        init_log_branch_filter(&mut log, &mut scope, &vcs, &storage);
        set_log_branches(&mut log, &mut loads, &mut scope, &vcs, &mut storage, &strings(&["feature"]));
        assert_eq!(log.view.filters.branches, ["feature"]);
        assert_eq!(remembered_log_branches(&storage, "/mem-save"), Some(strings(&["feature"])));
        set_log_branches(&mut log, &mut loads, &mut scope, &vcs, &mut storage, &[]);
        assert_eq!(remembered_log_branches(&storage, "/mem-save"), Some(Vec::new()));
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › Git Log branch filter memory › does not remember a choice in another log tab
    #[test]
    fn does_not_remember_a_choice_in_another_log_tab() {
        let (vcs, mut log, mut scope) = project("/mem-tab", "/mem-tab", main_only());
        let mut storage = UiStorage::default();
        log.shown_log = "tab-1".to_owned();
        set_log_branches(&mut log, &mut LogLoads::default(), &mut scope, &vcs, &mut storage, &strings(&["feature"]));
        assert_eq!(remembered_log_branches(&storage, "/mem-tab"), None);
    }

    // src/features/vcs/__tests__/logBranchMemory.test.ts › Git Log branch filter memory › keeps a separate choice for every repository
    #[test]
    fn keeps_a_separate_choice_for_every_repository() {
        let mut storage = UiStorage::default();
        remember_log_branches(&mut storage, "/mem-multi", &strings(&["main"]));
        remember_log_branches(&mut storage, "/mem-multi/backend", &strings(&["dev"]));
        let branches = vec![branch("dev", false, true), branch("main", false, false)];
        let (vcs, mut log, mut scope) = project("/mem-multi", "/mem-multi/backend", branches);
        init_log_branch_filter(&mut log, &mut scope, &vcs, &storage);
        assert_eq!(log.view.filters.branches, ["dev"]);
        assert_eq!(scope.as_deref(), Some("/mem-multi/backend"));
    }
}
