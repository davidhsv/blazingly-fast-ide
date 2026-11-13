//! Git Log state: filters, the selection, and the log tabs of their own (History: file,
//! Log: branch) next to the main Log.
//!
//! Loading commits is the caller's: the methods that change what a log shows return whether its
//! commits have to be loaded.

use super::store::VcsState;
use crate::platform::collate;
use crate::vcs::changes::strip_slash;
use ide_vcs::{Branch, BranchInfo, Commit};
use std::collections::{HashMap, HashSet};

/// A commit of the log with the repository it was read from (several repositories share one log).
#[derive(Clone, Debug)]
pub struct RepoCommit {
    pub commit: Commit,
    /// Root of its repository; `None` in a log of the selected repository alone.
    pub repo: Option<String>,
    /// The branches pointing at the commit.
    pub ref_branches: Vec<Branch>,
    /// A commit of a repository opened for reading only (no history rewriting).
    pub read_only: bool,
}

impl From<Commit> for RepoCommit {
    fn from(commit: Commit) -> Self {
        Self { commit, repo: None, ref_branches: Vec::new(), read_only: false }
    }
}

/// Identity of a commit in a log of several repositories.
pub fn commit_key(id: &str, repo: Option<&str>) -> String {
    match repo.filter(|repo| !repo.is_empty()) {
        Some(repo) => format!("{repo}\0{id}"),
        None => id.to_owned(),
    }
}

impl RepoCommit {
    pub fn key(&self) -> String {
        commit_key(&self.commit.id, self.repo.as_deref())
    }
}

/// Date filter: commits made after / before the given times (milliseconds since the epoch).
#[derive(Clone, Debug, PartialEq)]
pub struct LogDateFilter {
    pub after: Option<f64>,
    pub before: Option<f64>,
    pub label: String,
}

/// User filter entry standing for the current Git user (`user.name` / `user.email`).
pub const ME: &str = "me";

/// What a log shows; the default shows everything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogFilters {
    /// Branches / revisions to show; empty = all.
    pub branches: Vec<String>,
    pub repositories: Option<Vec<String>>,
    /// Author names or emails (any matches); [`ME`] = the current user.
    pub authors: Vec<String>,
    pub text: String,
    /// `text` is a regular expression.
    pub regex: bool,
    /// `text` matches case-sensitively.
    pub match_case: bool,
    /// Absolute path (file or directory).
    pub path: Option<String>,
    pub paths: Option<Vec<String>>,
    pub range: Option<String>,
    pub date: Option<LogDateFilter>,
}

/// The filters to change; `None` leaves a filter as it is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LogFiltersPatch {
    pub branches: Option<Vec<String>>,
    pub repositories: Option<Option<Vec<String>>>,
    pub authors: Option<Vec<String>>,
    pub text: Option<String>,
    pub regex: Option<bool>,
    pub match_case: Option<bool>,
    pub path: Option<Option<String>>,
    pub paths: Option<Option<Vec<String>>>,
    pub range: Option<Option<String>>,
    pub date: Option<Option<LogDateFilter>>,
}

impl LogFilters {
    /// Changes the filters `patch` sets; a new branch choice drops the commit range picked for the
    /// former branches.
    pub fn apply(&mut self, patch: LogFiltersPatch) {
        fn set<T>(field: &mut T, value: Option<T>) {
            if let Some(value) = value {
                *field = value;
            }
        }
        // Other branches: a commit range picked for the former ones no longer applies.
        if patch.branches.is_some() {
            self.range = None;
        }
        set(&mut self.branches, patch.branches);
        set(&mut self.repositories, patch.repositories);
        set(&mut self.authors, patch.authors);
        set(&mut self.text, patch.text);
        set(&mut self.regex, patch.regex);
        set(&mut self.match_case, patch.match_case);
        set(&mut self.path, patch.path);
        set(&mut self.paths, patch.paths);
        set(&mut self.range, patch.range);
        set(&mut self.date, patch.date);
    }
}

/// The Repository filter after clicking `repo`: added when unchecked, removed when checked
/// (none = all).
pub fn toggle_repository(selected: Option<&[String]>, repo: &str) -> Vec<String> {
    let current = selected.unwrap_or_default();
    if current.iter().any(|known| known == repo) {
        current.iter().filter(|known| *known != repo).cloned().collect()
    } else {
        current.iter().cloned().chain([repo.to_owned()]).collect()
    }
}

/// Any filter other than the text field's toggles is set.
pub fn has_filters(filters: &LogFilters) -> bool {
    let listed = |list: &Option<Vec<String>>| list.as_ref().is_some_and(|list| !list.is_empty());
    listed(&filters.repositories)
        || !filters.branches.is_empty()
        || !filters.authors.is_empty()
        || !filters.text.is_empty()
        || filters.path.as_deref().is_some_and(|path| !path.is_empty())
        || listed(&filters.paths)
        || filters.range.as_deref().is_some_and(|range| !range.is_empty())
        || filters.date.is_some()
}

/// [`ME`] replaced by the current user's email (or name); duplicates and blanks dropped.
pub fn resolve_authors(authors: &[String], me: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for author in authors {
        let author = if author == ME { me.unwrap_or_default() } else { author.as_str() }.trim();
        if !author.is_empty() && !out.iter().any(|known| known == author) {
            out.push(author.to_owned());
        }
    }
    out
}

/// The commit's time falls inside the date filter (`before` is exclusive).
pub fn in_date_range(time_ms: f64, filter: Option<&LogDateFilter>) -> bool {
    filter.is_none_or(|filter| {
        filter.after.is_none_or(|after| time_ms >= after) && filter.before.is_none_or(|before| time_ms < before)
    })
}

/// The refs the log shows: the remembered ones that still exist, else `HEAD` (the current branch,
/// followed across checkouts). An empty list is a real choice: all branches.
pub fn resolve_log_branches(remembered: Option<&[String]>, branches: &[String]) -> Vec<String> {
    let head = || vec!["HEAD".to_owned()];
    let Some(remembered) = remembered else { return head() };
    if remembered.is_empty() {
        return Vec::new();
    }
    let valid: Vec<String> =
        remembered.iter().filter(|name| *name == "HEAD" || branches.contains(name)).cloned().collect();
    if valid.is_empty() { head() } else { valid }
}

/// What the log shows depends on: HEAD and every branch tip. When this changes (commit, undo,
/// reset, rebase, fetch, branch created / deleted) the log is stale.
pub fn refs_signature(head: Option<&BranchInfo>, branches: &[Branch]) -> String {
    let mut tips: Vec<String> = branches
        .iter()
        .map(|branch| format!("{}:{}@{}", if branch.remote { "r" } else { "l" }, branch.name, branch.commit))
        .collect();
    tips.sort_by(|a, b| collate::compare_code_units(a, b));
    let name = head.and_then(|head| head.head.as_deref()).unwrap_or_default();
    let commit = head.and_then(|head| head.head_commit.as_deref()).unwrap_or_default();
    let mut lines = vec![format!("HEAD:{name}@{commit}")];
    lines.extend(tips);
    lines.join("\n")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogTabKind {
    /// History of a file or folder.
    History,
    /// Log of a branch.
    Branch,
    /// Another Log with the filters of the one it was opened from.
    Log,
    /// Log of a repository outside the project.
    External,
}

impl LogTabKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::History => "history",
            Self::Branch => "branch",
            Self::Log => "log",
            Self::External => "external",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogTabLocation {
    #[default]
    ToolWindow,
    Editor,
}

/// A log tab of its own: its own filters and view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogTab {
    pub id: String,
    pub kind: LogTabKind,
    /// What it shows (the file path or the branch); opening the same again selects the tab.
    pub target: String,
    pub repo: Option<String>,
    pub repositories: Option<Vec<String>>,
    pub location: LogTabLocation,
    pub read_only: bool,
}

impl LogTab {
    /// The repositories the tab's log reads.
    fn scope(&self) -> Vec<String> {
        self.repositories.clone().unwrap_or_else(|| self.repo.clone().into_iter().collect())
    }
}

/// What a log tab shows: swapped in and out when another log tab is selected.
#[derive(Clone, Debug, Default)]
pub struct LogView {
    pub filters: LogFilters,
    pub commits: Vec<RepoCommit>,
    pub loading: bool,
    /// Every commit matching the filters is loaded.
    pub done: bool,
    pub error: Option<String>,
    /// Lead selection.
    pub selected: Option<String>,
    /// Key of every selected commit (includes `selected`), in selection order.
    pub selection: Vec<String>,
    /// Bumped to ask the list to scroll the selection into view.
    pub reveal_tick: u64,
}

/// Id of the main Log, as a content tab and as a log.
pub const MAIN_LOG: &str = "log";

/// What opening a log tab came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenedLogTab {
    pub id: String,
    /// The shown log has no commits yet: load them.
    pub reload: bool,
    /// The tab lives in the editor area: open its editor.
    pub editor: bool,
}

#[derive(Clone, Debug)]
pub struct GitLog {
    /// The view of the log shown ([`GitLog::shown_log`]).
    pub view: LogView,
    /// Commits of HEAD not on its upstream (`@{u}..HEAD`); `None` without an upstream.
    pub outgoing: Option<HashSet<String>>,
    /// Repositories whose current branch has an upstream ("" = the selected one, in a log of it alone).
    pub tracked: HashSet<String>,
    /// Content tab of the Git tool window (a tab id, or the id of a log tab).
    pub tab: String,
    /// Closable log tabs next to the main Log.
    pub log_tabs: Vec<LogTab>,
    /// The log whose commits, filters and selection `view` holds ([`MAIN_LOG`] or a `log_tabs` id).
    pub shown_log: String,
    /// Views of the log tabs not shown now.
    parked: HashMap<String, LogView>,
    tab_seq: u64,
}

impl Default for GitLog {
    fn default() -> Self {
        Self {
            view: LogView::default(),
            outgoing: None,
            tracked: HashSet::new(),
            tab: MAIN_LOG.to_owned(),
            log_tabs: Vec::new(),
            shown_log: MAIN_LOG.to_owned(),
            parked: HashMap::new(),
            tab_seq: 0,
        }
    }
}

impl GitLog {
    pub fn is_log_tab(&self, id: &str) -> bool {
        id == MAIN_LOG || self.log_tabs.iter().any(|tab| tab.id == id)
    }

    /// The view of log `id`, shown or not.
    pub fn view_of(&self, id: &str) -> Option<&LogView> {
        if id == self.shown_log { Some(&self.view) } else { self.parked.get(id) }
    }

    /// The view of log `id`, shown or not, to change it: a load of a log that is not shown
    /// finishes into its parked view.
    pub fn view_of_mut(&mut self, id: &str) -> Option<&mut LogView> {
        if id == self.shown_log { Some(&mut self.view) } else { self.parked.get_mut(id) }
    }

    /// Puts the view of log `id` in place, parking the one shown. Returns whether its commits have
    /// to be loaded.
    fn show_log_view(&mut self, id: &str) -> bool {
        if self.shown_log == id {
            return false;
        }
        let view = self.parked.remove(id).unwrap_or_default();
        let parked = std::mem::replace(&mut self.view, view);
        self.parked.insert(std::mem::replace(&mut self.shown_log, id.to_owned()), parked);
        self.view.commits.is_empty()
    }

    /// Selects a content tab of the Git tool window; a log tab brings its own filters and commits.
    /// Returns whether the shown log's commits have to be loaded.
    pub fn select_git_tab(&mut self, id: &str) -> bool {
        let reload = self.is_log_tab(id) && self.show_log_view(id);
        self.tab = id.to_owned();
        reload
    }

    /// Opens (or selects) a log tab of its own, leaving the main Log and its filters as they are:
    /// Show History for a file, Show Git Log for a branch.
    pub fn open_log_tab(
        &mut self,
        kind: LogTabKind,
        target: &str,
        location: LogTabLocation,
        repositories: Option<Vec<String>>,
        vcs: &VcsState,
    ) -> OpenedLogTab {
        let active = self.log_tabs.iter().find(|tab| tab.id == self.shown_log);
        let inherits = matches!(kind, LogTabKind::Log | LogTabKind::Branch) && active.is_some_and(|tab| tab.read_only);
        let repositories =
            repositories.or_else(|| active.filter(|_| inherits).map(LogTab::scope).filter(|scope| !scope.is_empty()));
        let below = |repo: &str| target.strip_prefix(repo).is_some_and(|rest| rest.starts_with('/'));
        let repo = match kind {
            LogTabKind::External => Some(target.to_owned()),
            LogTabKind::History => history_repo(target, vcs)
                .or_else(|| {
                    let listed = active.and_then(|tab| tab.repositories.as_ref());
                    listed.and_then(|repos| repos.iter().find(|repo| target == *repo || below(repo)).cloned())
                })
                .or_else(|| active.and_then(|tab| tab.repo.clone()).filter(|repo| !repo.is_empty() && below(repo))),
            LogTabKind::Log => active.and_then(|tab| tab.repo.clone()),
            LogTabKind::Branch => {
                active.and_then(|tab| tab.repo.clone()).or_else(|| vcs.current_repo_root().map(str::to_owned))
            }
        };
        let read_only = kind == LogTabKind::External || active.is_some_and(|tab| tab.read_only);
        let scoped = repositories.as_ref().is_some_and(|repositories| !repositories.is_empty());
        let same = |tab: &&LogTab| {
            if tab.kind != kind {
                false
            } else if kind == LogTabKind::External && scoped {
                let sorted = |mut list: Vec<String>| {
                    list.sort();
                    list
                };
                sorted(tab.scope()) == sorted(repositories.clone().unwrap_or_default())
            } else {
                tab.target == target && tab.repo == repo
            }
        };
        if let Some(existing) = self.log_tabs.iter().find(same).filter(|_| kind != LogTabKind::Log) {
            let id = existing.id.clone();
            let reload = self.select_git_tab(&id);
            return OpenedLogTab { id, reload, editor: false };
        }
        self.tab_seq += 1;
        let id = format!("{}:{}", kind.id(), self.tab_seq);
        let filters = match kind {
            LogTabKind::Log => self.view.filters.clone(),
            LogTabKind::History => LogFilters { path: Some(target.to_owned()), ..LogFilters::default() },
            LogTabKind::Branch => LogFilters { branches: vec![target.to_owned()], ..LogFilters::default() },
            LogTabKind::External => LogFilters::default(),
        };
        self.parked.insert(id.clone(), LogView { filters, ..LogView::default() });
        self.log_tabs.push(LogTab {
            id: id.clone(),
            kind,
            target: target.to_owned(),
            repo: if scoped { None } else { repo },
            repositories,
            location,
            read_only,
        });
        let editor = location == LogTabLocation::Editor;
        let reload = if editor { self.show_log_view(&id) } else { self.select_git_tab(&id) };
        OpenedLogTab { id, reload, editor }
    }

    /// Closes a log tab; when it was shown, the main Log comes back. Returns whether the main Log's
    /// commits have to be loaded.
    pub fn close_log_tab(&mut self, id: &str) -> bool {
        if !self.log_tabs.iter().any(|tab| tab.id == id) {
            return false;
        }
        let reload = self.shown_log == id && self.show_log_view(MAIN_LOG);
        self.parked.remove(id);
        self.log_tabs.retain(|tab| tab.id != id);
        if self.tab == id {
            self.tab = MAIN_LOG.to_owned();
        }
        reload
    }

    /// Changes the shown log's filters; its commits then have to be loaded again.
    pub fn set_log_filters(&mut self, patch: LogFiltersPatch) {
        self.view.filters.apply(patch);
    }

    pub fn select_commit(&mut self, key: Option<&str>, reveal: bool) {
        self.view.selected = key.map(str::to_owned);
        self.view.selection = key.map(str::to_owned).into_iter().collect();
        if reveal {
            self.view.reveal_tick += 1;
        }
    }

    /// Cmd/Ctrl-click: toggles the commit in the selection.
    pub fn toggle_commit_selection(&mut self, key: &str) {
        let view = &mut self.view;
        if view.selection.iter().any(|known| known == key) {
            view.selection.retain(|known| known != key);
            view.selected = view.selection.last().cloned();
        } else {
            view.selection.push(key.to_owned());
            view.selected = Some(key.to_owned());
        }
    }

    /// Shift-click / Shift+arrows: selects the rows between the anchor (lead) and the commit within
    /// `visible`.
    pub fn extend_commit_selection(&mut self, key: &str, visible: &[RepoCommit]) {
        let view = &mut self.view;
        let anchor = view.selection.first().cloned().or_else(|| view.selected.clone());
        let index_of = |wanted: &str| visible.iter().position(|commit| commit.key() == wanted);
        view.selected = Some(key.to_owned());
        let (Some(anchor), Some(to)) = (anchor, index_of(key)) else {
            view.selection = vec![key.to_owned()];
            return;
        };
        let Some(from) = index_of(&anchor) else {
            view.selection = vec![key.to_owned()];
            return;
        };
        let range = &visible[from.min(to)..=from.max(to)];
        // The anchor stays first so further extensions pivot on it.
        view.selection = vec![anchor.clone()];
        view.selection.extend(range.iter().map(RepoCommit::key).filter(|key| *key != anchor));
    }

    /// Selected commits, newest first (log order).
    pub fn selected_commits(&self) -> Vec<&RepoCommit> {
        let keys: HashSet<&str> = self.view.selection.iter().map(String::as_str).collect();
        self.view.commits.iter().filter(|commit| keys.contains(commit.key().as_str())).collect()
    }

    /// The commit is on the upstream of the current branch (rewriting it needs a force push).
    pub fn is_pushed(&self, key: &str) -> bool {
        let Some(commit) = self.view.commits.iter().find(|commit| commit.key() == key) else { return false };
        self.tracked.contains(commit.repo.as_deref().unwrap_or_default())
            && self.outgoing.as_ref().is_some_and(|outgoing| !outgoing.contains(key))
    }
}

/// The repository whose history Show History lists: a repository's own root (a submodule folder
/// included) is its own history, not the folder's history in the enclosing repository; any other
/// path belongs to the innermost repository containing it.
fn history_repo(target: &str, vcs: &VcsState) -> Option<String> {
    let path = strip_slash(target);
    let own = vcs.roots.iter().find(|root| root.as_str() == path).map(String::as_str);
    own.or_else(|| vcs.repo_of(target)).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::super::test_support;
    use super::*;

    fn info(head_commit: &str) -> BranchInfo {
        let head = Some("main".to_owned());
        BranchInfo { head, head_commit: Some(head_commit.to_owned()), ..test_support::branch_info() }
    }

    fn branch(name: &str, commit: &str, remote: bool) -> Branch {
        Branch { remote, commit: commit.to_owned(), ..test_support::branch(name) }
    }

    fn commit(summary: &str) -> RepoCommit {
        RepoCommit::from(test_support::commit(&summary.repeat(40), &[], summary))
    }

    fn summaries(log: &GitLog) -> Vec<&str> {
        log.selected_commits().iter().map(|commit| commit.commit.summary.as_str()).collect()
    }

    fn vcs() -> VcsState {
        VcsState {
            root: Some("/p".to_owned()),
            repo: Some("/p".to_owned()),
            roots: vec!["/p".to_owned()],
            ..VcsState::default()
        }
    }

    fn open(log: &mut GitLog, kind: LogTabKind, target: &str) -> String {
        log.open_log_tab(kind, target, LogTabLocation::ToolWindow, None, &vcs()).id
    }

    // src/features/vcs/__tests__/gitOps.test.ts › log ordering and staleness › changes the refs signature when HEAD or any branch tip moves
    #[test]
    fn changes_the_refs_signature_when_head_or_any_branch_tip_moves() {
        let head = info("c3");
        let base = refs_signature(Some(&head), &[branch("main", "c3", false), branch("origin/main", "c3", true)]);
        let reordered = [branch("origin/main", "c3", true), branch("main", "c3", false)];
        assert_eq!(refs_signature(Some(&head), &reordered), base);
        let moved = [branch("main", "c2", false), branch("origin/main", "c3", true)];
        assert_ne!(refs_signature(Some(&info("c2")), &moved), base);
        let fetched = [branch("main", "c3", false), branch("origin/main", "c4", true)];
        assert_ne!(refs_signature(Some(&head), &fetched), base);
        assert_ne!(refs_signature(Some(&head), &[branch("main", "c3", false)]), base);
    }

    // src/features/vcs/__tests__/historyOps.test.ts › Git Log commit actions › extends and toggles the log selection
    #[test]
    fn extends_and_toggles_the_log_selection() {
        let list: Vec<RepoCommit> = ["1", "2", "3", "4"].into_iter().map(commit).collect();
        let key = |index: usize| list[index].key();
        let mut log = GitLog::default();
        log.view.commits = list.clone();
        log.select_commit(Some(&key(1)), false);
        log.extend_commit_selection(&key(3), &list);
        assert_eq!(summaries(&log), ["2", "3", "4"]);
        log.extend_commit_selection(&key(0), &list);
        assert_eq!(summaries(&log), ["1", "2"]);
        log.toggle_commit_selection(&key(3));
        assert_eq!(summaries(&log), ["1", "2", "4"]);
        log.toggle_commit_selection(&key(3));
        assert_eq!(log.view.selected, Some(key(0)));
    }

    // src/features/vcs/__tests__/commitMode.test.ts › log tabs of their own › opens Show History in a closable tab and leaves the main Log's filters alone
    #[test]
    fn opens_show_history_in_a_closable_tab_and_leaves_the_main_logs_filters_alone() {
        let mut log = GitLog::default();
        log.set_log_filters(LogFiltersPatch { authors: Some(vec!["bob".to_owned()]), ..LogFiltersPatch::default() });
        let id = open(&mut log, LogTabKind::History, "/p/src/a.ts");
        assert_eq!((log.tab.as_str(), log.shown_log.as_str()), (id.as_str(), id.as_str()));
        assert_eq!(log.view.filters.path.as_deref(), Some("/p/src/a.ts"));
        assert!(log.view.filters.authors.is_empty());
        // The same file again: the same tab.
        log.select_git_tab(MAIN_LOG);
        assert_eq!(log.view.filters.authors, ["bob"]);
        assert_eq!(open(&mut log, LogTabKind::History, "/p/src/a.ts"), id);
        assert_eq!(log.log_tabs.len(), 1);

        log.close_log_tab(&id);
        assert_eq!((log.tab.as_str(), log.shown_log.as_str()), (MAIN_LOG, MAIN_LOG));
        assert!(log.log_tabs.is_empty());
        assert_eq!(log.view.filters.authors, ["bob"]);
        assert_eq!(log.view.filters.path, None);
    }

    // src/features/vcs/__tests__/commitMode.test.ts › log tabs of their own › keeps a log tab's view while another content tab is shown
    #[test]
    fn keeps_a_log_tabs_view_while_another_content_tab_is_shown() {
        let mut log = GitLog::default();
        let id = open(&mut log, LogTabKind::Branch, "feature");
        log.select_git_tab("console");
        assert_eq!((log.tab.as_str(), log.shown_log.as_str()), ("console", id.as_str()));
        log.select_git_tab(&id);
        assert_eq!(log.view.filters.branches, ["feature"]);
        log.close_log_tab(&id);
    }

    // Not from a spec: the pure filter helpers.
    #[test]
    fn resolves_filters() {
        let strings = |values: &[&str]| values.iter().map(|value| (*value).to_owned()).collect::<Vec<String>>();
        assert_eq!(toggle_repository(None, "/a"), ["/a"]);
        assert!(toggle_repository(Some(&strings(&["/a"])), "/a").is_empty());
        assert!(!has_filters(&LogFilters { regex: true, ..LogFilters::default() }));
        assert!(has_filters(&LogFilters { text: "x".to_owned(), ..LogFilters::default() }));
        assert_eq!(resolve_authors(&strings(&[ME, " bob ", "bob", ""]), Some("me@x.org")), ["me@x.org", "bob"]);
        assert_eq!(resolve_authors(&strings(&[ME]), None), Vec::<String>::new());
        let filter = LogDateFilter { after: Some(10.0), before: Some(20.0), label: String::new() };
        assert!(in_date_range(10.0, Some(&filter)) && !in_date_range(20.0, Some(&filter)));
        let branches = strings(&["main"]);
        assert_eq!(resolve_log_branches(None, &branches), ["HEAD"]);
        assert!(resolve_log_branches(Some(&[]), &branches).is_empty());
        assert_eq!(resolve_log_branches(Some(&strings(&["gone"])), &branches), ["HEAD"]);
        assert_eq!(resolve_log_branches(Some(&strings(&["main", "gone"])), &branches), ["main"]);
    }

    // Not from a spec: a commit counts as pushed only in a repository whose branch has an upstream.
    #[test]
    fn tells_pushed_commits() {
        let mut log = GitLog::default();
        log.view.commits = vec![commit("1"), commit("2")];
        let (local, pushed) = (log.view.commits[0].key(), log.view.commits[1].key());
        log.outgoing = Some(HashSet::from([local.clone()]));
        assert!(!log.is_pushed(&pushed));
        log.tracked.insert(String::new());
        assert!(log.is_pushed(&pushed));
        assert!(!log.is_pushed(&local));
    }
}
