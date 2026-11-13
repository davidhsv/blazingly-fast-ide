//! Repository state shared by the version control views: working tree status, HEAD and upstream
//! info, and branches.
//!
//! A project may hold several repositories (nested clones, submodules). Status covers all of them;
//! branch info and branches are those of the selected repository, which branch, log and remote
//! operations act on. Nothing here talks to the backend: a refresh is started by the caller, which
//! hands its result to [`VcsState::finish_refresh`], and [`RefreshQueue`] says when to start one.

use crate::vcs::changes::{EffectiveKind, FileStatus, effective_kind, repo_label, root_of, strip_slash};
use ide_core::ipc_path::to_ipc;
use ide_host::ops::vcs::VcsRoot;
use ide_vcs::{Branch, BranchInfo};
use std::collections::BTreeMap;

/// Current branch (`None` when detached) and HEAD commit of a repository.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoHead {
    pub head: Option<String>,
    pub head_commit: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct VcsState {
    /// The open project is a git repository.
    pub available: bool,
    pub root: Option<String>,
    /// Repository roots, the project's own repository first.
    pub roots: Vec<String>,
    /// Root → merge / rebase / cherry-pick / revert in progress there.
    pub operations: BTreeMap<String, String>,
    pub heads: BTreeMap<String, RepoHead>,
    /// The selected repository's root; `None` = the project's primary one.
    pub repo: Option<String>,
    pub status: Vec<FileStatus>,
    /// `status` reflects the repository (false until the first refresh).
    pub status_loaded: bool,
    pub branch: Option<BranchInfo>,
    pub branches: Vec<Branch>,
    pub branches_loaded: bool,
    pub stashes_loaded: bool,
    /// Bumped after each refresh; gutters query their hunks again when it changes.
    pub version: u64,
    pub loading: bool,
    pub error: Option<String>,
    /// A network operation (fetch / pull / push) is running: its progress title.
    pub remote_busy: Option<String>,
}

/// The files with unresolved conflicts.
pub fn conflicted_files(status: &[FileStatus]) -> Vec<&FileStatus> {
    status.iter().filter(|f| effective_kind(f) == Some(EffectiveKind::Conflicted)).collect()
}

/// A repository that just stopped in a merge / rebase / cherry-pick / revert (e.g. on conflicts
/// after Update Project) while `shown` has none in progress: it is selected so the banner, the
/// branch widget and Continue / Abort act on it. `now` is in the order the backend lists roots.
pub fn attention_repo(shown: Option<&str>, before: &BTreeMap<String, String>, now: &[VcsRoot]) -> Option<String> {
    let stopped: Vec<String> = now.iter().filter(|root| in_operation(root).is_some()).map(root_path).collect();
    if shown.is_some_and(|shown| stopped.iter().any(|root| root == shown)) {
        return None;
    }
    stopped.into_iter().find(|root| !before.contains_key(root))
}

/// The root of a repository, as the UI holds paths.
fn root_path(root: &VcsRoot) -> String {
    to_ipc(&root.root)
}

/// The operation a repository is stopped in.
fn in_operation(root: &VcsRoot) -> Option<&str> {
    root.operation.as_deref().filter(|operation| !operation.is_empty())
}

/// Whether a file system event concerns the working tree. Changes of `.git` internals alone do
/// not: refreshing the status on them rewrites `.git/index` and triggers the event again.
pub fn touches_worktree<S: AsRef<str>>(paths: &[S]) -> bool {
    paths.iter().any(|path| !path.as_ref().split(['/', '\\']).any(|segment| segment == ".git"))
}

/// What a refresh found out about the conflicted files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConflictChange {
    Unchanged,
    /// The set of conflicted files became non-empty.
    Appeared(Vec<FileStatus>),
    /// The last conflicted file was resolved (or the merge aborted).
    Cleared,
}

/// What the state held when a refresh started: what its result is compared against.
#[derive(Clone, Debug)]
pub struct RefreshStart {
    repo: Option<String>,
    roots: Vec<String>,
    operations: BTreeMap<String, String>,
    branches: Vec<Branch>,
    /// The refresh also lists the selected repository's branches.
    pub branches_loaded: bool,
    /// The refresh also lists the selected repository's stashes.
    pub stashes_loaded: bool,
}

impl RefreshStart {
    /// The repository whose branch info, branches and stashes the refresh reads.
    pub fn repo(&self) -> Option<&str> {
        self.repo.as_deref()
    }
}

/// Branch info and branches of the repository a refresh read them for.
#[derive(Clone, Debug)]
pub struct SelectedRepo {
    pub branch: BranchInfo,
    /// `None` when the branches were not listed ([`RefreshStart::branches_loaded`] was off).
    pub branches: Option<Vec<Branch>>,
}

/// What a refresh read from the backend.
pub struct RefreshData {
    pub roots: Vec<VcsRoot>,
    pub status: Vec<FileStatus>,
    /// The selected repository's part, or why it could not be read.
    pub selected: Result<SelectedRepo, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// Branch info and branches are of another repository than the one now selected: refresh
    /// again.
    pub again: bool,
    pub conflicts: ConflictChange,
}

impl VcsState {
    /// The state of a project just opened: nothing loaded yet.
    fn for_project(root: Option<&str>, available: bool, version: u64) -> Self {
        Self { available, root: root.map(str::to_owned), version, ..Self::default() }
    }

    /// Follows the open project. Returns whether the state was reset (another project, or the same
    /// one whose being a repository changed).
    pub fn reset_for_project(&mut self, root: Option<&str>, available: bool) -> bool {
        if self.root.as_deref() == root && self.available == available {
            return false;
        }
        *self = Self::for_project(root, available, self.version + 1);
        true
    }

    /// Root of the selected repository, which its log's and diffs' repository-relative paths are
    /// relative to.
    pub fn current_repo_root(&self) -> Option<&str> {
        self.repo.as_deref().or(self.roots.first().map(String::as_str)).or(self.root.as_deref())
    }

    /// Root of the innermost repository containing `path`.
    pub fn repo_of(&self, path: &str) -> Option<&str> {
        root_of(path, &self.roots)
    }

    /// Repository name for display.
    pub fn repo_name<'a>(&self, root: &'a str) -> &'a str {
        repo_label(root, self.root.as_deref())
    }

    pub fn conflicted_files(&self) -> Vec<&FileStatus> {
        conflicted_files(&self.status)
    }

    /// A git command is running (`git_work` of them) or an operation is stopped in some
    /// repository: local changes may be put aside for the time being (merge, rebase, update), so
    /// files that look unchanged now may come back.
    pub fn local_changes_may_be_aside(&self, git_work: usize) -> bool {
        let operation = self.branch.as_ref().and_then(|branch| branch.operation.as_deref());
        let stopped = operation.is_some_and(|op| !op.is_empty());
        git_work > 0 || self.remote_busy.is_some() || !self.operations.is_empty() || stopped
    }

    /// The status of the file or folder at `path`.
    pub fn status_of(&self, path: &str) -> Option<&FileStatus> {
        let path = strip_slash(path);
        self.status.iter().find(|file| strip_slash(&file.path) == path)
    }

    /// Selects the repository branch, log and remote operations act on. Returns whether it
    /// changed, in which case the caller refreshes.
    pub fn select_repo(&mut self, root: &str) -> bool {
        if self.repo.as_deref() == Some(root) {
            return false;
        }
        self.repo = Some(root.to_owned());
        self.branches.clear();
        self.branches_loaded = false;
        self.stashes_loaded = false;
        true
    }

    /// Stores the branches listed for `repo`, unless another repository was selected meanwhile.
    pub fn set_branches(&mut self, repo: Option<&str>, branches: Vec<Branch>) {
        if repo == self.repo.as_deref() {
            self.branches = branches;
            self.branches_loaded = true;
        }
    }

    /// Stores a status and the branch info read with it.
    pub fn apply_status(&mut self, status: Vec<FileStatus>, branch: Option<BranchInfo>) -> ConflictChange {
        let before = self.conflicted_files().len();
        self.status = status;
        self.branch = branch;
        self.status_loaded = true;
        let conflicts = self.conflicted_files();
        match (before, conflicts.len()) {
            (0, now) if now > 0 => ConflictChange::Appeared(conflicts.into_iter().cloned().collect()),
            (before, 0) if before > 0 => ConflictChange::Cleared,
            _ => ConflictChange::Unchanged,
        }
    }

    /// Marks a refresh as running and remembers what its result is compared against.
    pub fn begin_refresh(&mut self) -> RefreshStart {
        self.loading = true;
        RefreshStart {
            repo: self.repo.clone(),
            roots: self.roots.clone(),
            operations: self.operations.clone(),
            branches: self.branches.clone(),
            branches_loaded: self.branches_loaded,
            stashes_loaded: self.stashes_loaded,
        }
    }

    /// Stores what a refresh read (`Err`: why it failed as a whole).
    pub fn finish_refresh(&mut self, started: &RefreshStart, data: Result<RefreshData, String>) -> RefreshOutcome {
        let mut outcome = RefreshOutcome { again: false, conflicts: ConflictChange::Unchanged };
        match data {
            Err(error) => self.error = Some(error),
            Ok(data) => {
                let roots: Vec<String> = data.roots.iter().map(root_path).collect();
                match data.selected {
                    // The selected repository is gone (removed since the last scan): fall back to
                    // the primary one.
                    Err(_) if started.repo.as_ref().is_some_and(|repo| !roots.contains(repo)) => {
                        self.roots = roots;
                        self.repo = None;
                        self.branches_loaded = false;
                        self.stashes_loaded = false;
                        outcome.again = true;
                    }
                    Err(error) => self.error = Some(error),
                    Ok(selected) => outcome = self.apply_refresh(started, roots, data.roots, data.status, selected),
                }
            }
        }
        self.loading = false;
        self.version += 1;
        outcome
    }

    fn apply_refresh(
        &mut self,
        started: &RefreshStart,
        roots: Vec<String>,
        listed: Vec<VcsRoot>,
        status: Vec<FileStatus>,
        selected: SelectedRepo,
    ) -> RefreshOutcome {
        // No selection (before the first refresh) is the primary repository, listed first.
        let shown = started.repo.clone().filter(|repo| roots.contains(repo)).or_else(|| roots.first().cloned());
        let stopped = attention_repo(shown.as_deref(), &started.operations, &listed);
        let repo = stopped.or_else(|| shown.clone());
        self.operations =
            listed.iter().filter_map(|root| Some((root_path(root), in_operation(root)?.to_owned()))).collect();
        self.heads = listed
            .into_iter()
            .map(|root| (root_path(&root), RepoHead { head: root.head, head_commit: root.head_commit }))
            .collect();
        if repo != started.repo || roots != started.roots {
            self.roots = roots;
            self.repo.clone_from(&repo);
        }
        let conflicts = self.apply_status(status, Some(selected.branch));
        self.branches = selected.branches.unwrap_or_else(|| started.branches.clone());
        self.error = None;
        let again = repo != shown || (started.repo.is_some() && repo != started.repo);
        if again {
            self.branches_loaded = false;
            self.stashes_loaded = false;
        }
        RefreshOutcome { again, conflicts }
    }
}

/// What asking for a refresh came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshRequest {
    /// There is no repository to refresh: the request is settled at once.
    Settled,
    /// The request is settled when run `ticket` finishes; with `start`, the caller starts that run
    /// now.
    Run { ticket: u64, start: bool },
}

/// What the end of a refresh run came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RefreshDone {
    /// Every request with a ticket up to this one is settled.
    pub settled: u64,
    /// A request arrived during the run: the caller starts the next run now.
    pub start: bool,
}

/// Coalesces refresh requests: one run at a time, and a request made while a run is in flight is
/// settled by a run that starts after it, so that whoever waits for it reads the state its own
/// operation produced. All requests made during one run share the same follow-up run.
#[derive(Debug, Default)]
pub struct RefreshQueue {
    run: u64,
    inflight: bool,
    queued: bool,
}

impl RefreshQueue {
    /// A run is in flight, or asked for and about to start.
    pub fn in_flight(&self) -> bool {
        self.inflight
    }

    pub fn request(&mut self, available: bool) -> RefreshRequest {
        if !available {
            return RefreshRequest::Settled;
        }
        if self.inflight {
            self.queued = true;
            return RefreshRequest::Run { ticket: self.run + 1, start: false };
        }
        self.run += 1;
        self.inflight = true;
        RefreshRequest::Run { ticket: self.run, start: true }
    }

    /// The run in flight finished. `available`: the project still is a repository.
    pub fn finished(&mut self, available: bool) -> RefreshDone {
        self.inflight = false;
        if !std::mem::take(&mut self.queued) {
            return RefreshDone { settled: self.run, start: false };
        }
        self.run += 1;
        if available {
            self.inflight = true;
            RefreshDone { settled: self.run - 1, start: true }
        } else {
            // Nothing left to refresh: the queued requests are settled without a run.
            RefreshDone { settled: self.run, start: false }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::branch_info;
    use super::*;
    use crate::vcs::changes::ChangeKind;

    fn modified(rel: &str) -> FileStatus {
        FileStatus {
            path: format!("/p/{rel}"),
            rel_path: rel.to_owned(),
            staged: None,
            unstaged: Some(ChangeKind::Modified),
            renamed_from: None,
        }
    }

    fn root(path: &str, operation: Option<&str>) -> VcsRoot {
        VcsRoot { root: path.into(), operation: operation.map(str::to_owned), head: None, head_commit: None }
    }

    fn data(rel: &str) -> RefreshData {
        RefreshData {
            roots: vec![root("/p", None)],
            status: vec![modified(rel)],
            selected: Ok(SelectedRepo {
                branch: BranchInfo { head: Some("main".to_owned()), ..branch_info() },
                branches: None,
            }),
        }
    }

    // src/features/vcs/__tests__/gitOps.test.ts › status refresh › a refresh requested while one is running resolves only after a newer refresh
    #[test]
    fn a_refresh_requested_while_one_is_running_resolves_only_after_a_newer_refresh() {
        let mut state = VcsState { available: true, ..VcsState::default() };
        let mut queue = RefreshQueue::default();
        let mut runs = 0;

        let first = queue.request(state.available);
        assert_eq!(first, RefreshRequest::Run { ticket: 1, start: true });
        let started = state.begin_refresh();
        runs += 1;
        // An operation finished while the first refresh was in flight.
        let second = queue.request(state.available);
        let third = queue.request(state.available);
        assert_eq!(second, RefreshRequest::Run { ticket: 2, start: false });
        assert_eq!(third, second);

        state.finish_refresh(&started, Ok(data("before.ts")));
        let done = queue.finished(state.available);
        assert_eq!(done, RefreshDone { settled: 1, start: true });
        let started = state.begin_refresh();
        runs += 1;
        state.finish_refresh(&started, Ok(data("after.ts")));
        assert_eq!(queue.finished(state.available), RefreshDone { settled: 2, start: false });

        let shown: Vec<&str> = state.status.iter().map(|f| f.rel_path.as_str()).collect();
        assert_eq!(shown, ["after.ts"]);
        assert_eq!(runs, 2);
        assert!(!state.loading);
    }

    // Not from a spec: a project that is not a repository settles a request without a run.
    #[test]
    fn a_request_without_a_repository_is_settled_at_once() {
        let mut queue = RefreshQueue::default();
        assert_eq!(queue.request(false), RefreshRequest::Settled);
        assert_eq!(queue.request(true), RefreshRequest::Run { ticket: 1, start: true });
        assert_eq!(queue.request(true), RefreshRequest::Run { ticket: 2, start: false });
        assert_eq!(queue.finished(false), RefreshDone { settled: 2, start: false });
    }

    // Not from a spec: a repository that stopped in an operation is selected, and read again.
    #[test]
    fn selects_the_repository_that_just_stopped_in_an_operation() {
        let mut state = VcsState { available: true, ..VcsState::default() };
        let started = state.begin_refresh();
        let mut found = data("a.ts");
        found.roots.push(root("/p/sub", Some("rebase")));
        let outcome = state.finish_refresh(&started, Ok(found));
        assert!(outcome.again);
        assert_eq!(state.repo.as_deref(), Some("/p/sub"));
        assert_eq!(state.roots, ["/p", "/p/sub"]);
        assert_eq!(state.operations.get("/p/sub").map(String::as_str), Some("rebase"));
        assert!(touches_worktree(&["/p/.git/index", "/p/src/a.ts"]));
        assert!(!touches_worktree(&["/p/.git/index", "C:\\p\\.git"]));
    }
}
