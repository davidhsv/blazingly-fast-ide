//! Loading the Git log: pages of commits read from every repository a log shows and merged into one
//! list, refreshed without losing what is loaded, and the navigation that pages until a commit is
//! there (Go to Hash/Branch/Tag, Go to Parent / Child Commit).
//!
//! Nothing here calls the backend. A load is a [`PageLoad`] that says what to ask and takes the
//! answers; a newer load of the same log makes the answers of an older one void, and the requests
//! of the older one are handed back to be cancelled. Request ids are passed in.

use super::log_presentation::{ALL_MERGES, LogFragments, PresentedLog, present_log_graph};
use super::multi_root::merge_repo_logs;
use crate::vcs::log_options::{LogOptions, LogSort};
use crate::vcs_ops::log_store::{
    GitLog, LogFilters, LogTab, LogTabKind, LogView, ME, RepoCommit, has_filters, resolve_authors,
};
use crate::vcs_ops::store::{RepoHead, VcsState};
use ide_core::{IpcPath, RequestId};
use ide_vcs::{Branch, Commit, LogOrder, LogQuery};
use regex::Regex;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

/// Commits read per page.
pub const PAGE: usize = 300;

/// The commit a reload jumps to: the page read is the window of the log around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Seek {
    pub repo: Option<String>,
    pub id: String,
}

/// The Git user of a repository (`user.email` and `user.name`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RepoUser {
    pub email: Option<String>,
    pub name: Option<String>,
}

impl RepoUser {
    /// What the "me" filter stands for: the email, else the name.
    pub fn user(&self) -> Option<&str> {
        fn filled(value: &Option<String>) -> Option<&str> {
            value.as_deref().filter(|value| !value.is_empty())
        }
        filled(&self.email).or_else(|| filled(&self.name))
    }
}

/// What a load reads besides the log's own state.
#[derive(Clone, Copy, Debug)]
pub struct LoadEnv<'a> {
    pub vcs: &'a VcsState,
    /// The view options of the log being loaded.
    pub options: &'a LogOptions,
    /// Settings › Version Control › Limit history to N rows (file / folder history); `None` when
    /// the limit is off.
    pub history_limit: Option<usize>,
}

/// What a log keeps while loading, next to its view.
#[derive(Clone, Debug, Default)]
struct Scope {
    /// Bumped by every reload; a load whose generation is no longer the log's is void.
    generation: u64,
    seek: Option<Seek>,
    fragments: LogFragments,
    /// Page requests in flight.
    requests: Vec<RequestId>,
    /// A page load is in flight: another one does not start.
    loading: bool,
    /// The Git user of each repository read ("" = the selected one, in a log of it alone).
    users: BTreeMap<String, RepoUser>,
}

/// The loads of every log, by log id.
#[derive(Clone, Debug, Default)]
pub struct LogLoads {
    scopes: HashMap<String, Scope>,
    sequence: u64,
}

/// A reload that started.
#[derive(Debug)]
pub struct Reload {
    /// Requests of the load it supersedes, to cancel.
    pub cancel: Vec<RequestId>,
    pub generation: u64,
    /// The page to read; `None` when there is nothing to read.
    pub page: Option<PageLoad>,
}

/// A query to send for one repository of a load.
#[derive(Clone, Debug)]
pub struct PageRequest {
    /// Index of the repository in [`PageLoad::roots`].
    pub root: usize,
    pub repo: Option<String>,
    pub query: LogQuery,
}

/// What a repository of a load needs next.
#[derive(Clone, Debug)]
pub enum RootNext {
    /// Its commits are read (or it failed).
    Settled,
    /// Ask for the names of every reference by commit, then call [`PageLoad::decorated`].
    Decorations { root: usize, repo: Option<String> },
    /// Send the query, then call [`PageLoad::fetched`].
    Fetch(Box<PageRequest>),
}

/// What a repository's query is built from.
#[derive(Clone, Debug)]
struct RootContext {
    branches: Vec<String>,
    authors: Vec<String>,
}

#[derive(Clone, Debug)]
enum RootState {
    Pending,
    Fetching { boundary: Option<String>, request: RequestId, context: RootContext },
    Decorating { page: Vec<Commit>, context: RootContext },
    Settled(Result<Vec<RepoCommit>, String>),
}

/// One page of a log being read from its repositories.
#[derive(Clone, Debug)]
pub struct PageLoad {
    /// The log being loaded.
    pub id: String,
    generation: u64,
    /// The page replaces what is loaded (a reload) instead of following it.
    replace: bool,
    /// The page precedes what is loaded (after a jump into the middle of the log).
    before: bool,
    filters: LogFilters,
    /// What the log held when the load started.
    previous: Vec<RepoCommit>,
    was_done: bool,
    seek: Option<Seek>,
    tab: Option<LogTab>,
    order: LogOrder,
    first_parent: bool,
    no_merges: bool,
    roots: Vec<Option<String>>,
    tab_roots: Vec<String>,
    vcs_roots: Vec<String>,
    project_root: Option<String>,
    cap: usize,
    loaded: usize,
    limit: usize,
    refs: HashMap<String, Vec<Branch>>,
    users: Vec<RepoUser>,
    states: Vec<RootState>,
    /// The new commits were joined to the loaded ones instead of replacing them.
    joined: bool,
}

fn tab_scope(tab: Option<&LogTab>) -> Vec<String> {
    let Some(tab) = tab else { return Vec::new() };
    let listed = tab.repositories.clone().filter(|repositories| !repositories.is_empty());
    listed.unwrap_or_else(|| tab.repo.clone().filter(|repo| !repo.is_empty()).into_iter().collect())
}

/// The repository a filtered path belongs to: the innermost of the tab's own, else of the project's.
fn owner_of(path: &str, tab_roots: &[String], vcs_roots: &[String]) -> Option<String> {
    let below = |repo: &&String| {
        path == repo.as_str() || path.strip_prefix(repo.as_str()).is_some_and(|rest| rest.starts_with('/'))
    };
    tab_roots
        .iter()
        .filter(below)
        .max_by_key(|repo| repo.len())
        .cloned()
        .or_else(|| crate::vcs::changes::root_of(path, vcs_roots).map(str::to_owned))
}

fn of_repo(commits: &[RepoCommit], repo: &Option<String>) -> Vec<RepoCommit> {
    commits.iter().filter(|c| c.repo == *repo).cloned().collect()
}

const fn log_order(sort: LogSort) -> LogOrder {
    match sort {
        LogSort::Date => LogOrder::Date,
        LogSort::Topo => LogOrder::Topo,
        LogSort::Standard => LogOrder::Standard,
    }
}

fn narrow(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl LogLoads {
    /// The generation of log `id`: what a load compares its own with before it changes the log.
    pub fn generation(&self, id: &str) -> u64 {
        self.scopes.get(id).map_or(0, |scope| scope.generation)
    }

    pub fn seek(&self, id: &str) -> Option<&Seek> {
        self.scopes.get(id)?.seek.as_ref()
    }

    /// Sets where the next reload of log `id` jumps to. Changing the filters clears it.
    pub fn set_seek(&mut self, id: &str, seek: Option<Seek>) {
        self.scopes.entry(id.to_owned()).or_default().seek = seek;
    }

    pub fn fragments(&self, id: &str) -> LogFragments {
        self.scopes.get(id).map(|scope| scope.fragments.clone()).unwrap_or_default()
    }

    pub fn set_fragments(&mut self, id: &str, fragments: LogFragments) {
        self.scopes.entry(id.to_owned()).or_default().fragments = fragments;
    }

    /// The Git user of the repository `commit` was read from, for Highlight › My Commits.
    pub fn user_of(&self, id: &str, commit: &RepoCommit) -> Option<&RepoUser> {
        self.scopes.get(id)?.users.get(commit.repo.as_deref().unwrap_or_default())
    }

    /// The page requests in flight of log `id` (`None`: of every log), which are to be cancelled.
    /// Their loads may be started again.
    pub fn cancel(&mut self, id: Option<&str>) -> Vec<RequestId> {
        let mut cancelled = Vec::new();
        for (log, scope) in &mut self.scopes {
            if id.is_some_and(|id| id != log.as_str()) || scope.requests.is_empty() {
                continue;
            }
            cancelled.append(&mut scope.requests);
            scope.loading = false;
        }
        cancelled
    }

    /// Log tab `id` was closed: its requests to cancel.
    pub fn closed(&mut self, id: &str) -> Vec<RequestId> {
        let cancelled = self.cancel(Some(id));
        self.scopes.remove(id);
        cancelled
    }

    /// Another project: every log starts over. Returns the requests to cancel.
    pub fn reset(&mut self) -> Vec<RequestId> {
        let cancelled = self.cancel(None);
        self.scopes.clear();
        cancelled
    }

    fn request_done(&mut self, id: &str, request: RequestId) {
        if let Some(scope) = self.scopes.get_mut(id) {
            scope.requests.retain(|known| *known != request);
        }
    }

    /// Reloads the history of log `id`; with `preserve` the current rows stay until their
    /// replacement arrives.
    pub fn reload(&mut self, log: &mut GitLog, id: &str, preserve: bool, env: LoadEnv) -> Reload {
        let cancel = self.cancel(Some(id));
        self.sequence += 1;
        let generation = self.sequence;
        let scope = self.scopes.entry(id.to_owned()).or_default();
        scope.generation = generation;
        scope.loading = false;
        if let Some(view) = log.view_of_mut(id) {
            if !preserve {
                view.commits.clear();
                view.done = false;
            }
            view.loading = true;
            view.error = None;
        }
        let page = self.load_page(log, id, true, false, env);
        Reload { cancel, generation, page }
    }

    /// The next page of older commits; `None` when everything is loaded or a page is on its way.
    pub fn load_more(&mut self, log: &mut GitLog, id: &str, env: LoadEnv) -> Option<PageLoad> {
        self.load_page(log, id, false, false, env)
    }

    /// The page of newer commits above a window the log jumped into.
    pub fn load_newer(&mut self, log: &mut GitLog, id: &str, env: LoadEnv) -> Option<PageLoad> {
        self.load_page(log, id, false, true, env)
    }

    fn load_page(&mut self, log: &mut GitLog, id: &str, replace: bool, before: bool, env: LoadEnv) -> Option<PageLoad> {
        let view = log.view_of(id)?;
        if !replace && !before && view.done {
            return None;
        }
        if self.scopes.get(id).is_some_and(|scope| scope.loading) {
            return None;
        }
        let filters = view.filters.clone();
        let previous = view.commits.clone();
        let was_done = view.done;
        let paths = filters.paths.clone().unwrap_or_default();
        let path = filters.path.clone().filter(|path| !path.is_empty());
        let limited = path.is_some() || !paths.is_empty();
        let cap = env.history_limit.filter(|_| limited).unwrap_or(usize::MAX);
        let loaded = if replace { 0 } else { previous.len() };
        let view = log.view_of_mut(id)?;
        if loaded >= cap {
            view.done = true;
            view.loading = false;
            return None;
        }
        view.loading = true;
        let scope = self.scopes.entry(id.to_owned()).or_default();
        scope.loading = true;

        let tab = log.log_tabs.iter().find(|tab| tab.id == id).cloned();
        let tab_roots = tab_scope(tab.as_ref());
        let vcs_roots = env.vcs.roots.clone();
        let owner = |path: &String| owner_of(path, &tab_roots, &vcs_roots);
        let path_roots: Option<Vec<Option<String>>> = match &path {
            Some(path) => Some(vec![owner(path)]),
            None if !paths.is_empty() => {
                let mut owners = Vec::new();
                for found in paths.iter().map(owner) {
                    if !owners.contains(&found) {
                        owners.push(found);
                    }
                }
                Some(owners)
            }
            None => None,
        };
        let selected = filters.repositories.clone().unwrap_or_default();
        let listed = |repos: &[String]| repos.iter().cloned().map(Some).collect::<Vec<_>>();
        let candidates = if !tab_roots.is_empty() {
            listed(&tab_roots)
        } else if let Some(owners) = &path_roots {
            owners.clone()
        } else if !selected.is_empty() {
            listed(&selected)
        } else if !vcs_roots.is_empty() {
            listed(&vcs_roots)
        } else {
            vec![env.vcs.repo.clone()]
        };
        let roots: Vec<Option<String>> = candidates
            .into_iter()
            .filter(|repo| {
                path_roots.as_ref().is_none_or(|owners| owners.contains(repo))
                    && (selected.is_empty() || repo.as_ref().is_some_and(|repo| selected.contains(repo)))
            })
            .collect();
        let limit = (if replace { PAGE.max(previous.len()) } else { PAGE }).min(cap - loaded);
        Some(PageLoad {
            id: id.to_owned(),
            generation: scope.generation,
            replace,
            before,
            filters,
            previous,
            was_done,
            seek: scope.seek.clone(),
            tab,
            order: log_order(env.options.sort),
            first_parent: env.options.first_parent,
            no_merges: env.options.no_merges,
            states: vec![RootState::Pending; roots.len()],
            users: vec![RepoUser::default(); roots.len()],
            roots,
            tab_roots,
            vcs_roots,
            project_root: env.vcs.root.clone(),
            cap,
            loaded,
            limit,
            refs: HashMap::new(),
            joined: false,
        })
    }

    /// The load could not start reading (a repository outside the project did not open).
    pub fn fail(&mut self, log: &mut GitLog, page: &PageLoad, error: &str) {
        if self.generation(&page.id) != page.generation {
            return;
        }
        if let Some(view) = log.view_of_mut(&page.id) {
            view.error = Some(error.to_owned());
            view.done = if page.replace && !page.previous.is_empty() { page.was_done } else { true };
            view.loading = false;
        }
        if let Some(scope) = self.scopes.get_mut(&page.id) {
            scope.loading = false;
        }
    }

    /// Puts what `page` read into its log, unless a newer load took over meanwhile. Every
    /// repository of the page has to be settled.
    pub fn finish(&mut self, log: &mut GitLog, page: PageLoad) {
        let PageLoad { id, generation, replace, before, previous, roots, limit, cap, loaded, .. } = &page;
        let (replace, before, limit, cap, loaded) = (*replace, *before, *limit, *cap, *loaded);
        let mut failures = Vec::new();
        let mut streams = Vec::new();
        for (state, repo) in page.states.iter().zip(roots) {
            match state {
                RootState::Settled(Ok(commits)) => streams.push(commits.clone()),
                RootState::Settled(Err(error)) => {
                    let name = match repo.as_deref().filter(|repo| !repo.is_empty()) {
                        Some(repo) => crate::vcs::changes::repo_label(repo, page.project_root.as_deref()),
                        None => "Git",
                    };
                    failures.push(format!("{name}: {error}"));
                    streams.push(if replace { of_repo(previous, repo) } else { Vec::new() });
                }
                _ => streams.push(Vec::new()),
            }
        }
        let whole = before || page.joined || page.seek.is_some() || (replace && !previous.is_empty());
        let merged: Vec<Vec<RepoCommit>> = if before {
            let above = |(stream, repo): (&Vec<RepoCommit>, &Option<String>)| {
                [stream.clone(), of_repo(previous, repo)].concat()
            };
            streams.iter().zip(roots).map(above).collect()
        } else {
            streams.clone()
        };
        let mut rows = merge_repo_logs(&merged, if whole { usize::MAX } else { limit });
        if replace && roots.len() > 1 && !previous.is_empty() && page.seek.is_none() {
            // The loaded window is kept through its last surviving commit. Older candidates of each
            // repository are read again when paging.
            let loaded_keys: HashSet<String> = previous.iter().map(RepoCommit::key).collect();
            let last = rows.iter().rposition(|c| loaded_keys.contains(&c.key()));
            rows.truncate(limit.max(last.map_or(0, |last| last + 1)));
        }
        if self.generation(id) != *generation {
            return;
        }
        let Some(view) = log.view_of_mut(id) else { return };
        if let Some(scope) = self.scopes.get_mut(id) {
            scope.loading = false;
            for (repo, user) in roots.iter().zip(&page.users) {
                scope.users.insert(repo.clone().unwrap_or_default(), user.clone());
            }
        }
        view.loading = false;
        if replace && !roots.is_empty() && failures.len() == roots.len() && !view.commits.is_empty() {
            view.error = Some(failures.join("\n"));
            return;
        }
        let commits = if replace || before {
            rows.clone()
        } else {
            let grown: Vec<Vec<RepoCommit>> =
                roots.iter().map(|repo| [of_repo(&view.commits, repo), of_repo(&rows, repo)].concat()).collect();
            merge_repo_logs(&grown, usize::MAX)
        };
        let counted = !rows.is_empty() && rows.iter().all(|c| c.commit.log_total.is_some());
        view.done = if before {
            view.done
        } else if commits.len() >= cap {
            true
        } else if counted {
            // Every commit read from a repository has to be shown before the repository counts as
            // read to its end.
            roots.iter().zip(&streams).all(|(repo, stream)| {
                let Some(total) = stream.last().and_then(|c| c.commit.log_total) else {
                    return stream.is_empty();
                };
                let last = commits.iter().rfind(|c| c.repo == *repo).and_then(|c| c.commit.log_position);
                last.is_some_and(|position| position + 1 >= total)
            })
        } else if page.joined {
            page.was_done
        } else {
            rows.len() < limit || loaded.saturating_add(rows.len()) >= cap
        };
        view.commits = commits;
        view.error = (!failures.is_empty()).then(|| failures.join("\n"));
    }
}

impl PageLoad {
    /// Repositories outside the project to open for reading before anything else is asked.
    pub fn external_logs(&self) -> &[String] {
        match &self.tab {
            Some(tab) if tab.read_only => &self.tab_roots,
            _ => &[],
        }
    }

    /// The repositories read; `None` is the selected one, in a project without a list of them. The
    /// load needs the branches of each named one and the Git user of each.
    pub fn roots(&self) -> &[Option<String>] {
        &self.roots
    }

    pub fn is_settled(&self) -> bool {
        self.states.iter().all(|state| matches!(state, RootState::Settled(_)))
    }

    fn previous_of(&self, root: usize) -> Vec<&RepoCommit> {
        self.previous.iter().filter(|c| c.repo == self.roots[root]).collect()
    }

    fn query(&self, root: usize, context: &RootContext, stop_at: Option<String>, request: RequestId) -> LogQuery {
        let repo = &self.roots[root];
        let f = &self.filters;
        let previous = self.previous_of(root);
        let first_position = previous.first().and_then(|c| c.commit.log_position).unwrap_or(0) as usize;
        let skip = if self.before {
            first_position.saturating_sub(PAGE)
        } else if self.replace {
            0
        } else {
            match previous.last().and_then(|c| c.commit.log_position) {
                Some(position) => position as usize + 1,
                None => previous.len(),
            }
        };
        let own = |path: &&String| owner_of(path, &self.tab_roots, &self.vcs_roots) == *repo;
        let text = f.text.trim();
        LogQuery {
            branches: context.branches.clone(),
            authors: context.authors.clone(),
            authors_exact: true,
            path: f.path.as_ref().filter(|path| !path.is_empty()).map(|path| IpcPath(PathBuf::from(path))),
            paths: f.paths.iter().flatten().filter(own).map(|path| IpcPath(PathBuf::from(path))).collect(),
            follow: self.tab.as_ref().is_some_and(|tab| tab.kind == LogTabKind::History),
            text: (!text.is_empty()).then(|| text.to_owned()),
            text_regex: f.regex,
            text_match_case: f.match_case,
            ranges: f.range.iter().flat_map(|range| range.split_whitespace()).map(str::to_owned).collect(),
            request: Some(request),
            summary_only: true,
            stop_at,
            retain_until: previous.last().filter(|_| self.replace).map(|c| c.commit.id.clone()),
            skip: narrow(skip),
            seek: self.seek.as_ref().filter(|seek| self.replace && seek.repo == *repo).map(|seek| seek.id.clone()),
            limit: narrow(if self.before { PAGE.min(first_position) } else { self.limit }),
            order: self.order,
            first_parent: self.first_parent,
            no_merges: self.no_merges,
            since_ms: f.date.as_ref().and_then(|date| date.after),
            until_ms: f.date.as_ref().and_then(|date| date.before),
            ..LogQuery::default()
        }
    }

    fn fetch(
        &mut self,
        loads: &mut LogLoads,
        root: usize,
        context: RootContext,
        boundary: Option<String>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> PageRequest {
        let request = next_request();
        loads.scopes.entry(self.id.clone()).or_default().requests.push(request);
        let query = self.query(root, &context, boundary.clone(), request);
        self.states[root] = RootState::Fetching { boundary, request, context };
        PageRequest { root, repo: self.roots[root].clone(), query }
    }

    /// The branches of the named repositories (`refs`, by root; a repository whose branches could
    /// not be listed has none) and the Git user of each repository (`users`, in the order of
    /// [`PageLoad::roots`]) are known: the queries to send.
    pub fn prepared(
        &mut self,
        loads: &mut LogLoads,
        refs: HashMap<String, Vec<Branch>>,
        users: Vec<RepoUser>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> Vec<PageRequest> {
        self.refs = refs;
        self.users = (0..self.roots.len()).map(|root| users.get(root).cloned().unwrap_or_default()).collect();
        let current = loads.generation(&self.id) == self.generation;
        let mut requests = Vec::new();
        for root in 0..self.roots.len() {
            let Some((context, boundary)) = self.context(root).filter(|_| current) else {
                self.states[root] = RootState::Settled(Ok(Vec::new()));
                continue;
            };
            requests.push(self.fetch(loads, root, context, boundary, next_request));
        }
        requests
    }

    /// What repository `root` is asked, and the commit a refresh may stop at; `None` when the
    /// filters leave nothing of it to show.
    fn context(&self, root: usize) -> Option<(RootContext, Option<String>)> {
        let f = &self.filters;
        let repo = self.roots[root].as_deref();
        let known = |name: &str| self.refs.values().flatten().any(|branch| branch.name == name);
        let own = |name: &str| {
            let listed = repo.and_then(|repo| self.refs.get(repo));
            listed.is_some_and(|branches| branches.iter().any(|branch| branch.name == name))
        };
        let branches: Vec<String> = f.branches.iter().filter(|name| !known(name) || own(name)).cloned().collect();
        if !f.branches.is_empty() && branches.is_empty() {
            return None;
        }
        let user = &self.users[root];
        let mut authors = resolve_authors(&f.authors, user.user());
        // The user's own commits are found by name as well as by email.
        let includes_me = f.authors.iter().any(|author| author == ME);
        let name = user.name.as_deref().filter(|name| includes_me && !name.is_empty());
        if let Some(name) = name.filter(|name: &&str| !authors.iter().any(|known| known.as_str() == *name)) {
            authors.push(name.to_owned());
        }
        if !f.authors.is_empty() && authors.is_empty() {
            return None;
        }
        let previous = self.previous_of(root);
        if self.before && previous.first().and_then(|c| c.commit.log_position).unwrap_or(0) == 0 {
            return None;
        }
        // A refresh of the current branch's plain history reads only what is new, down to the
        // newest commit already loaded.
        let plain = f.text.is_empty()
            && f.authors.is_empty()
            && f.path.as_deref().is_none_or(str::is_empty)
            && f.paths.as_ref().is_none_or(Vec::is_empty)
            && f.range.as_deref().is_none_or(str::is_empty)
            && f.date.is_none()
            && !self.no_merges
            && !self.first_parent;
        let boundary = previous
            .first()
            .filter(|_| self.replace && self.roots.len() == 1 && branches == ["HEAD"] && plain)
            .map(|c| c.commit.id.clone());
        Some((RootContext { branches, authors }, boundary))
    }

    /// The query of repository `root` was answered.
    pub fn fetched(
        &mut self,
        loads: &mut LogLoads,
        root: usize,
        result: Result<Vec<Commit>, String>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> RootNext {
        let (boundary, request, context) = match std::mem::replace(&mut self.states[root], RootState::Pending) {
            RootState::Fetching { boundary, request, context } => (boundary, request, context),
            other => {
                self.states[root] = other;
                return RootNext::Settled;
            }
        };
        loads.request_done(&self.id, request);
        let page = match result {
            Ok(page) => page,
            Err(error) => {
                self.states[root] = RootState::Settled(Err(error));
                return RootNext::Settled;
            }
        };
        let reached = boundary.is_some_and(|boundary| page.last().is_some_and(|c| c.id == boundary));
        if !reached {
            self.settle(root, page);
            return RootNext::Settled;
        }
        // Only a straight line down to the loaded commits can be joined to them.
        let linear = page.windows(2).all(|pair| pair[0].parents.len() == 1 && pair[0].parents[0] == pair[1].id);
        if linear {
            self.states[root] = RootState::Decorating { page, context };
            return RootNext::Decorations { root, repo: self.roots[root].clone() };
        }
        RootNext::Fetch(Box::new(self.fetch(loads, root, context, None, next_request)))
    }

    /// The references of repository `root` by name (`None`: they could not be read), after
    /// [`RootNext::Decorations`].
    pub fn decorated(
        &mut self,
        loads: &mut LogLoads,
        root: usize,
        decorations: Option<&BTreeMap<String, String>>,
        next_request: &mut impl FnMut() -> RequestId,
    ) -> RootNext {
        let (mut page, context) = match std::mem::replace(&mut self.states[root], RootState::Pending) {
            RootState::Decorating { page, context } => (page, context),
            other => {
                self.states[root] = other;
                return RootNext::Settled;
            }
        };
        let Some(decorations) = decorations else {
            return RootNext::Fetch(Box::new(self.fetch(loads, root, context, None, next_request)));
        };
        let mut labels: HashMap<&str, Vec<&String>> = HashMap::new();
        for (name, id) in decorations.iter().filter(|(_, id)| !id.is_empty()) {
            labels.entry(id.as_str()).or_default().push(name);
        }
        // The loaded commits stay, with the references as they are now.
        let old = self.previous_of(root).into_iter().map(|c| {
            let refs = labels.get(c.commit.id.as_str()).map(Vec::as_slice).unwrap_or_default();
            let same = refs.len() == c.commit.refs.len() && refs.iter().all(|&name| c.commit.refs.contains(name));
            let refs = if same { c.commit.refs.clone() } else { refs.iter().map(|&name| name.clone()).collect() };
            Commit { refs, ..c.commit.clone() }
        });
        let old: Vec<Commit> = old.collect();
        page.pop();
        page.extend(old);
        self.joined = true;
        self.settle(root, page);
        RootNext::Settled
    }

    fn settle(&mut self, root: usize, page: Vec<Commit>) {
        let repo = self.roots[root].clone();
        let branches = repo.as_deref().and_then(|repo| self.refs.get(repo)).map(Vec::as_slice).unwrap_or_default();
        let read_only = self.tab.as_ref().is_some_and(|tab| tab.read_only);
        let commits = page
            .into_iter()
            .map(|commit| {
                let at_tip = |branch: &&Branch| branch.commit == commit.id && commit.refs.contains(&branch.name);
                let ref_branches = branches.iter().filter(at_tip).cloned().collect();
                RepoCommit { commit, repo: repo.clone(), ref_branches, read_only }
            })
            .collect();
        self.states[root] = RootState::Settled(Ok(commits));
    }
}

/// The query asking a repository for the commit `rev` names.
pub fn lookup_query(rev: &str) -> LogQuery {
    LogQuery { branch: Some(rev.trim().to_owned()), limit: 1, ..LogQuery::default() }
}

/// The query for the commits of the current branch that are not on its upstream, which the log
/// marks as not pushed yet.
pub fn unpushed_query() -> LogQuery {
    LogQuery { branch: Some("HEAD".to_owned()), range: Some("@{u}..HEAD".to_owned()), ..LogQuery::default() }
}

static NOT_ALPHANUMERIC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]").expect("the pattern is valid"));

/// Highlight › My Commits: the commit is authored by `user` (an email, else a name), whose Git
/// name is `user_name`.
pub fn is_mine(commit: &Commit, user: Option<&str>, user_name: Option<&str>) -> bool {
    let Some(user) = user.filter(|user| !user.is_empty()) else { return false };
    let normalize = |value: &str| NOT_ALPHANUMERIC.replace_all(&value.to_lowercase(), "").into_owned();
    let local = |address: &str| normalize(address.split('@').next().unwrap_or_default());
    commit.email.to_lowercase() == user.to_lowercase()
        || normalize(&commit.author) == normalize(user_name.unwrap_or(user))
        || local(&commit.email) == local(user)
}

/// The log as displayed: the commits that stay visible are the selected and the bookmarked ones
/// and the head of each repository.
pub fn displayed_log_graph<'a>(
    view: &LogView,
    options: &LogOptions,
    fragments: &LogFragments,
    bookmarks: impl IntoIterator<Item = &'a str>,
    heads: &BTreeMap<String, RepoHead>,
) -> PresentedLog {
    let is_head = |c: &&RepoCommit| {
        let head = heads.get(c.repo.as_deref().unwrap_or_default()).and_then(|head| head.head_commit.as_deref());
        c.commit.refs.iter().any(|name| name == "HEAD") || head == Some(c.commit.id.as_str())
    };
    let keep: HashSet<String> = view
        .selection
        .iter()
        .cloned()
        .chain(bookmarks.into_iter().map(str::to_owned))
        .chain(view.commits.iter().filter(is_head).map(RepoCommit::key))
        .collect();
    present_log_graph(&view.commits, options, fragments, &keep)
}

/// A graph option that folds or unfolds the whole log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphOption {
    CollapseLinear(bool),
    LinearizeMerges(bool),
}

/// Sets a graph option, forgetting the fragments folded one by one. While merges are linearized,
/// Collapse Linear Branches folds and unfolds the merges instead. Returns whether the log has to be
/// reloaded.
pub fn set_graph_option(fragments: &mut LogFragments, options: &mut LogOptions, option: GraphOption) -> bool {
    let before = options.clone();
    let merges = |collapse: bool| LogFragments {
        merges: if collapse { HashSet::new() } else { HashSet::from([ALL_MERGES.to_owned()]) },
        ..LogFragments::default()
    };
    match option {
        GraphOption::CollapseLinear(collapse) => {
            *fragments = merges(collapse);
            options.collapse_linear = collapse && !options.linearize_merges;
        }
        GraphOption::LinearizeMerges(linearize) => {
            *fragments = LogFragments::default();
            options.linearize_merges = linearize;
        }
    }
    options.changes_query(&before)
}

/// Where a navigation in the log leads.
#[derive(Clone, Debug)]
pub enum Navigation {
    /// There is nowhere to go.
    Nothing,
    /// The commits looked for are not all loaded: load the next page and ask again.
    LoadMore,
    /// Ask which commit (the dialog's English title), then select the chosen one.
    Choose { title: &'static str, candidates: Vec<RepoCommit> },
    /// Select the commit with this key and scroll to it.
    Select(String),
}

fn pick(title: &'static str, mut candidates: Vec<RepoCommit>) -> Navigation {
    match candidates.len() {
        0 => Navigation::Nothing,
        1 => Navigation::Select(candidates.remove(0).key()),
        _ => Navigation::Choose { title, candidates },
    }
}

/// Go to Parent Commit: the displayed parents of the commit with `key`, offering a choice at a
/// merge. `can_page` says whether more of the log can be loaded.
pub fn go_to_parent(view: &LogView, displayed: &PresentedLog, key: &str, can_page: bool) -> Navigation {
    let Some(row) = displayed.graph.iter().find(|row| row.id == key) else { return Navigation::Nothing };
    if row.parents.is_empty() {
        return Navigation::Nothing;
    }
    let candidates: Vec<RepoCommit> = displayed
        .commits
        .iter()
        .filter_map(|index| view.commits.get(*index))
        .filter(|c| row.parents.contains(&c.key()))
        .cloned()
        .collect();
    if candidates.len() != row.parents.len() && can_page {
        return Navigation::LoadMore;
    }
    pick("Select Parent Commit", candidates)
}

/// Go to Child Commit: the loaded commits displayed with the commit with `key` as a parent.
pub fn go_to_child(view: &LogView, displayed: &PresentedLog, key: &str) -> Navigation {
    let is_child = |row: &&crate::vcs::graph::GraphCommit| row.parents.iter().any(|parent| parent == key);
    let children: HashSet<&str> = displayed.graph.iter().filter(is_child).map(|row| row.id.as_str()).collect();
    let candidates = view.commits.iter().filter(|c| children.contains(c.key().as_str())).cloned().collect();
    pick("Select Child Commit", candidates)
}

/// Whether more of the shown log can be paged in while looking for a commit: not once everything is
/// loaded, nor after a page that added nothing (`grew` is false).
pub fn can_page(view: &LogView, grew: bool) -> bool {
    !view.done && grew
}

/// What Go to Hash/Branch/Tag needs next.
#[derive(Debug)]
pub enum GoToRefStep {
    /// Send each repository its query and answer with [`GoToRef::found`], the results in the same
    /// order.
    Lookup(Vec<(Option<String>, LogQuery)>),
    /// Several repositories have such a commit: ask which one, and answer with
    /// [`GoToRef::chosen`].
    Choose { title: &'static str, candidates: Vec<RepoCommit> },
    /// The shown log is being reloaded: carry the reload out, then call [`GoToRef::reloaded`].
    Reload(Box<Reload>),
    /// Whether the commit was found and selected.
    Done(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Asking every repository of the log which of them has the commit.
    Candidates,
    /// Asking the one repository for the commit's id.
    Lookup,
    /// Loading a log that had no commits.
    FirstLoad,
    /// Jumping to the window of the log around the commit.
    Seeking,
    /// The same without the filters that hid it.
    Unfiltered,
    /// Showing the commit's own history.
    Pinned,
}

/// Go to Hash/Branch/Tag: selects the commit a revision names, jumping to its place in the log and
/// clearing the filters when they hide it.
#[derive(Clone, Debug)]
pub struct GoToRef {
    rev: String,
    shown_log: String,
    generation: u64,
    repos: Vec<Option<String>>,
    repo: Option<String>,
    target: String,
    stage: Stage,
}

impl GoToRef {
    /// Starts looking for `rev` in `repository` (`None`: in the repositories the shown log reads).
    pub fn begin(
        loads: &LogLoads,
        log: &GitLog,
        vcs: &VcsState,
        rev: &str,
        repository: Option<Option<String>>,
    ) -> (Self, GoToRefStep) {
        let tab = log.log_tabs.iter().find(|tab| tab.id == log.shown_log);
        let tab_roots = tab_scope(tab);
        let selected = log.view.filters.repositories.clone().unwrap_or_default();
        let named = |repos: &[String]| repos.iter().cloned().map(Some).collect::<Vec<_>>();
        let repos = match repository {
            Some(repository) => vec![repository],
            None if !tab_roots.is_empty() => named(&tab_roots),
            None if !selected.is_empty() => named(&selected),
            None if !vcs.roots.is_empty() => named(&vcs.roots),
            None => vec![vcs.repo.clone()],
        };
        let mut go = Self {
            rev: rev.to_owned(),
            shown_log: log.shown_log.clone(),
            generation: loads.generation(&log.shown_log),
            repos,
            repo: None,
            target: String::new(),
            stage: Stage::Candidates,
        };
        if go.repos.len() > 1 {
            let lookups = go.repos.iter().map(|repo| (repo.clone(), lookup_query(rev))).collect();
            return (go, GoToRefStep::Lookup(lookups));
        }
        let repo = go.repos.first().cloned().flatten();
        let step = go.single(loads, log, repo);
        (go, step)
    }

    /// The lookup still stands: no reload superseded it and the same log is shown.
    fn valid(&self, loads: &LogLoads, log: &GitLog) -> bool {
        loads.generation(&log.shown_log) == self.generation && log.shown_log == self.shown_log
    }

    fn single(&mut self, loads: &LogLoads, log: &GitLog, repo: Option<String>) -> GoToRefStep {
        self.generation = loads.generation(&log.shown_log);
        self.shown_log.clone_from(&log.shown_log);
        self.repo = repo;
        self.stage = Stage::Lookup;
        if self.rev.trim().is_empty() {
            return GoToRefStep::Done(false);
        }
        GoToRefStep::Lookup(vec![(self.repo.clone(), lookup_query(&self.rev))])
    }

    fn hit<'a>(&self, view: &'a LogView) -> Option<&'a RepoCommit> {
        view.commits.iter().find(|c| c.commit.id == self.target && (c.repo.is_none() || c.repo == self.repo))
    }

    /// The lookups were answered (a failed one found nothing).
    pub fn found(
        &mut self,
        loads: &mut LogLoads,
        log: &mut GitLog,
        vcs: &VcsState,
        results: Vec<Option<Commit>>,
        env: LoadEnv,
    ) -> GoToRefStep {
        if self.stage == Stage::Candidates {
            let candidates: Vec<RepoCommit> = self
                .repos
                .iter()
                .zip(results)
                .filter_map(|(repo, commit)| Some(RepoCommit { repo: repo.clone(), ..RepoCommit::from(commit?) }))
                .collect();
            if !self.valid(loads, log) {
                return GoToRefStep::Done(false);
            }
            if candidates.len() > 1 {
                // Each one is told apart by its repository's name.
                let named = candidates.into_iter().map(|mut c| {
                    let repo = c.repo.as_deref().unwrap_or_default();
                    c.commit.summary = format!("{} · {}", vcs.repo_name(repo), c.commit.summary);
                    c
                });
                return GoToRefStep::Choose { title: "Select Repository Commit", candidates: named.collect() };
            }
            return self.chosen(loads, log, candidates.into_iter().next());
        }
        let Some(commit) = results.into_iter().next().flatten().filter(|_| self.valid(loads, log)) else {
            return GoToRefStep::Done(false);
        };
        self.target = commit.id;
        self.advance(loads, log, env)
    }

    /// The repository's commit was chosen (`None`: none was).
    pub fn chosen(&mut self, loads: &LogLoads, log: &GitLog, commit: Option<RepoCommit>) -> GoToRefStep {
        match commit.filter(|_| self.valid(loads, log)) {
            Some(commit) => self.single(loads, log, commit.repo),
            None => GoToRefStep::Done(false),
        }
    }

    /// The reload asked for by [`GoToRefStep::Reload`] is over.
    pub fn reloaded(&mut self, loads: &mut LogLoads, log: &mut GitLog, env: LoadEnv) -> GoToRefStep {
        self.advance(loads, log, env)
    }

    fn reload(&mut self, loads: &mut LogLoads, log: &mut GitLog, stage: Stage, env: LoadEnv) -> GoToRefStep {
        self.stage = stage;
        let id = log.shown_log.clone();
        let reload = loads.reload(log, &id, false, env);
        self.generation = reload.generation;
        GoToRefStep::Reload(Box::new(reload))
    }

    fn advance(&mut self, loads: &mut LogLoads, log: &mut GitLog, env: LoadEnv) -> GoToRefStep {
        let seek = Seek { repo: self.repo.clone(), id: self.target.clone() };
        if self.stage == Stage::Lookup {
            if log.view.commits.is_empty() {
                return self.reload(loads, log, Stage::FirstLoad, env);
            }
            self.stage = Stage::FirstLoad;
        }
        if !self.valid(loads, log) {
            return GoToRefStep::Done(false);
        }
        if self.stage == Stage::FirstLoad {
            if self.hit(&log.view).is_none() {
                loads.set_seek(&self.shown_log, Some(seek));
                return self.reload(loads, log, Stage::Seeking, env);
            }
            self.stage = Stage::Seeking;
        }
        if self.stage == Stage::Seeking {
            if self.hit(&log.view).is_none() && has_filters(&log.view.filters) {
                log.view.filters = LogFilters::default();
                loads.set_seek(&self.shown_log, Some(seek));
                return self.reload(loads, log, Stage::Unfiltered, env);
            }
            self.stage = Stage::Unfiltered;
        }
        if self.stage == Stage::Unfiltered {
            if self.hit(&log.view).is_none() && log.view.error.is_none() {
                log.view.filters = LogFilters {
                    branches: vec![self.target.clone()],
                    repositories: self.repo.clone().map(|repo| vec![repo]),
                    ..LogFilters::default()
                };
                return self.reload(loads, log, Stage::Pinned, env);
            }
            self.stage = Stage::Pinned;
        }
        let Some(key) = self.hit(&log.view).map(RepoCommit::key) else {
            return GoToRefStep::Done(false);
        };
        log.select_commit(Some(&key), true);
        GoToRefStep::Done(true)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    /// A commit with `id` (also its summary) and `parents`, authored at time 0, in its wire form.
    pub(crate) fn commit(id: &str, parents: &[&str], refs: &[&str]) -> Commit {
        let wire = json!({
            "id": id,
            "shortId": id,
            "parents": parents,
            "refs": refs,
            "author": "A",
            "email": "a@example.com",
            "timeMs": 0,
            "summary": id,
            "message": id,
        });
        serde_json::from_value(wire).expect("a commit in its wire form")
    }

    pub(crate) fn repo_commit(commit: Commit, repo: &str) -> RepoCommit {
        RepoCommit { repo: Some(repo.to_owned()), ..RepoCommit::from(commit) }
    }

    /// What the log command answers for a repository and a query.
    pub(crate) type LogAnswer = Box<dyn FnMut(Option<&str>, &LogQuery) -> Result<Vec<Commit>, String>>;

    /// A page whose queries are sent but not answered yet.
    pub(crate) struct Pending {
        pub(crate) page: PageLoad,
        pub(crate) requests: Vec<PageRequest>,
    }

    /// A log with its loads and a backend that answers from closures.
    pub(crate) struct Harness {
        pub(crate) log: GitLog,
        pub(crate) loads: LogLoads,
        pub(crate) vcs: VcsState,
        pub(crate) options: LogOptions,
        pub(crate) history_limit: Option<usize>,
        pub(crate) answer: LogAnswer,
        /// What the references command answers; `None` fails it.
        pub(crate) refs: Option<BTreeMap<String, String>>,
        pub(crate) branches: HashMap<String, Vec<Branch>>,
        pub(crate) user: RepoUser,
        /// Every log query sent, with its repository.
        pub(crate) calls: Vec<(Option<String>, LogQuery)>,
        pub(crate) cancelled: Vec<RequestId>,
        next_request: u32,
    }

    impl Harness {
        /// A project whose only repository is `/repo`, with an empty history.
        pub(crate) fn new() -> Self {
            let vcs = VcsState {
                root: Some("/repo".to_owned()),
                repo: Some("/repo".to_owned()),
                roots: vec!["/repo".to_owned()],
                ..VcsState::default()
            };
            Self {
                log: GitLog::default(),
                loads: LogLoads::default(),
                vcs,
                options: LogOptions::default(),
                history_limit: None,
                answer: Box::new(|_, _| Ok(Vec::new())),
                refs: Some(BTreeMap::new()),
                branches: HashMap::new(),
                user: RepoUser::default(),
                calls: Vec::new(),
                cancelled: Vec::new(),
                next_request: 0,
            }
        }

        /// Answers every query with `commits`.
        pub(crate) fn answer_with(&mut self, commits: Vec<Commit>) {
            self.answer = Box::new(move |_, _| Ok(commits.clone()));
        }

        /// Answers a query with the rows of `history` it asks for.
        pub(crate) fn answer_from(&mut self, history: HashMap<String, Vec<Commit>>) {
            self.answer = Box::new(move |repo, query| {
                let rows = history.get(repo.unwrap_or_default()).map(Vec::as_slice).unwrap_or_default();
                Ok(rows.iter().skip(query.skip as usize).take(query.limit as usize).cloned().collect())
            });
        }

        pub(crate) fn shown(&self) -> String {
            self.log.shown_log.clone()
        }

        pub(crate) fn ids(&self) -> Vec<&str> {
            self.log.view.commits.iter().map(|c| c.commit.id.as_str()).collect()
        }

        pub(crate) fn last_query(&self) -> &LogQuery {
            &self.calls.last().expect("a query was sent").1
        }

        fn ask(&mut self, repo: Option<&str>, query: &LogQuery) -> Result<Vec<Commit>, String> {
            self.calls.push((repo.map(str::to_owned), query.clone()));
            (self.answer)(repo, query)
        }

        /// Sends the queries of a page without answering them.
        pub(crate) fn send(&mut self, mut page: PageLoad) -> Pending {
            let users = vec![self.user.clone(); page.roots().len()];
            let next_request = &mut self.next_request;
            let mut allocate = || {
                *next_request += 1;
                RequestId(*next_request)
            };
            let requests = page.prepared(&mut self.loads, self.branches.clone(), users, &mut allocate);
            Pending { page, requests }
        }

        /// Answers the queries of a page, each with `answers`' next entry when there is one (else
        /// with the backend's answer), and puts the page into its log.
        pub(crate) fn settle(&mut self, pending: Pending, answers: Vec<Vec<Commit>>) {
            let Pending { mut page, requests } = pending;
            let mut answers = VecDeque::from(answers);
            let mut queue = VecDeque::from(requests);
            while let Some(request) = queue.pop_front() {
                let result = match answers.pop_front() {
                    Some(answer) => {
                        self.calls.push((request.repo.clone(), request.query.clone()));
                        Ok(answer)
                    }
                    None => self.ask(request.repo.as_deref(), &request.query),
                };
                let next_request = &mut self.next_request;
                let mut allocate = || {
                    *next_request += 1;
                    RequestId(*next_request)
                };
                let mut next = page.fetched(&mut self.loads, request.root, result, &mut allocate);
                if let RootNext::Decorations { root, .. } = next {
                    next = page.decorated(&mut self.loads, root, self.refs.as_ref(), &mut allocate);
                }
                if let RootNext::Fetch(again) = next {
                    queue.push_back(*again);
                }
            }
            assert!(page.is_settled());
            self.loads.finish(&mut self.log, page);
        }

        /// Starts a reload of log `id` and sends its queries.
        pub(crate) fn start_reload(&mut self, id: &str, preserve: bool) -> Option<Pending> {
            let env = LoadEnv { vcs: &self.vcs, options: &self.options, history_limit: self.history_limit };
            let reload = self.loads.reload(&mut self.log, id, preserve, env);
            self.cancelled.extend(reload.cancel);
            reload.page.map(|page| self.send(page))
        }

        /// Reloads the shown log to the end.
        pub(crate) fn reload(&mut self, preserve: bool) {
            let id = self.shown();
            if let Some(pending) = self.start_reload(&id, preserve) {
                self.settle(pending, Vec::new());
            }
        }

        /// Loads the next page of the shown log.
        pub(crate) fn load_more(&mut self) {
            let id = self.shown();
            let env = LoadEnv { vcs: &self.vcs, options: &self.options, history_limit: self.history_limit };
            if let Some(page) = self.loads.load_more(&mut self.log, &id, env) {
                let pending = self.send(page);
                self.settle(pending, Vec::new());
            }
        }

        /// Runs Go to Hash/Branch/Tag to its end, choosing a candidate with `choose` when asked.
        pub(crate) fn go_to_ref(
            &mut self,
            rev: &str,
            mut choose: impl FnMut(&[RepoCommit]) -> Option<RepoCommit>,
        ) -> (bool, Vec<Vec<RepoCommit>>) {
            let mut offered = Vec::new();
            let (mut go, mut step) = GoToRef::begin(&self.loads, &self.log, &self.vcs, rev, None);
            loop {
                step = match step {
                    GoToRefStep::Done(found) => return (found, offered),
                    GoToRefStep::Lookup(lookups) => {
                        let results = self.look_up(&lookups);
                        let (vcs, options) = (&self.vcs, &self.options);
                        let env = LoadEnv { vcs, options, history_limit: self.history_limit };
                        go.found(&mut self.loads, &mut self.log, vcs, results, env)
                    }
                    GoToRefStep::Choose { candidates, .. } => {
                        let chosen = choose(&candidates);
                        offered.push(candidates);
                        go.chosen(&self.loads, &self.log, chosen)
                    }
                    GoToRefStep::Reload(reload) => {
                        let Reload { cancel, page, .. } = *reload;
                        self.cancelled.extend(cancel);
                        if let Some(page) = page {
                            let pending = self.send(page);
                            self.settle(pending, Vec::new());
                        }
                        let (vcs, options) = (&self.vcs, &self.options);
                        let env = LoadEnv { vcs, options, history_limit: self.history_limit };
                        go.reloaded(&mut self.loads, &mut self.log, env)
                    }
                };
            }
        }

        /// Answers lookups: the first commit of each query's answer.
        fn look_up(&mut self, lookups: &[(Option<String>, LogQuery)]) -> Vec<Option<Commit>> {
            lookups
                .iter()
                .map(|(repo, query)| self.ask(repo.as_deref(), query).ok().and_then(|found| found.into_iter().next()))
                .collect()
        }

        /// The displayed graph of the shown log.
        pub(crate) fn displayed(&self) -> PresentedLog {
            let fragments = self.loads.fragments(&self.log.shown_log);
            displayed_log_graph(&self.log.view, &self.options, &fragments, [], &self.vcs.heads)
        }

        pub(crate) fn set_graph_option(&mut self, option: GraphOption) {
            let id = self.shown();
            let mut fragments = self.loads.fragments(&id);
            set_graph_option(&mut fragments, &mut self.options, option);
            self.loads.set_fragments(&id, fragments);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{Harness, commit, repo_commit};
    use super::*;
    use crate::vcs_ops::log_store::{LogTabLocation, MAIN_LOG, commit_key};

    fn head_filter() -> LogFilters {
        LogFilters { branches: vec!["HEAD".to_owned()], ..LogFilters::default() }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn paths(query: &LogQuery) -> Vec<String> {
        query.paths.iter().map(|path| path.0.to_string_lossy().into_owned()).collect()
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › lets an inactive visible scope finish its own request without cancelling the active scope
    #[test]
    fn lets_an_inactive_visible_scope_finish_its_own_request_without_cancelling_the_active_scope() {
        let mut h = Harness::new();
        let main = h.start_reload(MAIN_LOG, false).expect("the main log is read");
        let request = main.requests[0].query.request.expect("the query can be cancelled");
        let opened = h.log.open_log_tab(LogTabKind::Log, "Other", LogTabLocation::ToolWindow, None, &h.vcs);
        assert!(opened.reload);
        h.answer_with(vec![commit("other", &[], &[])]);
        h.reload(false);
        h.reload(false);
        h.settle(main, vec![vec![commit("main", &[], &[])]]);
        assert!(!h.cancelled.contains(&request));
        assert_eq!(h.log.view_of(MAIN_LOG).expect("the main log is parked").commits[0].commit.id, "main");
        assert_eq!(h.log.view_of(&opened.id).expect("the tab is shown").commits[0].commit.id, "other");
        h.log.select_git_tab(MAIN_LOG);
        assert_eq!(h.ids()[0], "main");
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › joins a proven linear prefix and refreshes decorations without dropping loaded history
    #[test]
    fn joins_a_proven_linear_prefix_and_refreshes_decorations_without_dropping_loaded_history() {
        let mut h = Harness::new();
        h.log.view.filters = head_filter();
        h.answer_with(vec![commit("old", &["base"], &["HEAD", "main"]), commit("base", &[], &[])]);
        h.reload(false);
        h.answer_with(vec![commit("new", &["old"], &["HEAD", "main"]), commit("old", &["base"], &[])]);
        let decorations = [("HEAD", "new"), ("main", "new"), ("tag: v1", "base")];
        h.refs = Some(decorations.into_iter().map(|(name, id)| (name.to_owned(), id.to_owned())).collect());
        h.reload(true);
        assert_eq!((h.last_query().stop_at.as_deref(), h.last_query().skip), (Some("old"), 0));
        let rows: Vec<(&str, Vec<&str>)> = h
            .log
            .view
            .commits
            .iter()
            .map(|c| (c.commit.id.as_str(), c.commit.refs.iter().map(String::as_str).collect()))
            .collect();
        assert_eq!(rows, [("new", vec!["HEAD", "main"]), ("old", vec![]), ("base", vec!["tag: v1"])]);
        assert!(h.log.view.done);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › fetches a full replacement when the prefix introduces a merge
    #[test]
    fn fetches_a_full_replacement_when_the_prefix_introduces_a_merge() {
        let mut h = Harness::new();
        h.log.view.filters = head_filter();
        h.answer_with(vec![commit("old", &[], &[]), commit("stale", &[], &[])]);
        h.reload(false);
        let replacement =
            vec![commit("merge", &["old", "side"], &[]), commit("side", &["old"], &[]), commit("old", &[], &[])];
        h.answer_with(replacement);
        h.reload(true);
        let last_two = &h.calls[h.calls.len() - 2..];
        let stops: Vec<Option<&str>> = last_two.iter().map(|call| call.1.stop_at.as_deref()).collect();
        assert_eq!(stops, [Some("old"), None]);
        assert_eq!(h.ids(), ["merge", "side", "old"]);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › intersects repository and path filters and clears rows for an empty intersection
    #[test]
    fn intersects_repository_and_path_filters_and_clears_rows_for_an_empty_intersection() {
        let mut h = Harness::new();
        h.vcs.roots = strings(&["/repo/front", "/repo/back"]);
        h.log.view.filters = LogFilters {
            paths: Some(strings(&["/repo/front/a", "/repo/back/b"])),
            repositories: Some(strings(&["/repo/back"])),
            ..LogFilters::default()
        };
        h.answer_with(vec![commit("back", &[], &[])]);
        h.reload(false);
        let (repo, query) = h.calls.last().expect("the repository is read");
        assert_eq!(repo.as_deref(), Some("/repo/back"));
        assert_eq!(paths(query), ["/repo/back/b"]);
        assert!(query.authors_exact);
        h.calls.clear();
        h.log.view.filters = LogFilters {
            path: Some("/repo/front/a".to_owned()),
            repositories: Some(strings(&["/repo/back"])),
            ..LogFilters::default()
        };
        h.reload(true);
        assert!(h.calls.is_empty());
        assert!(h.log.view.commits.is_empty());
        assert_eq!(h.log.view.error, None);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › keeps multi-path filters scoped to their owning external repositories
    #[test]
    fn keeps_multi_path_filters_scoped_to_their_owning_external_repositories() {
        let mut h = Harness::new();
        h.vcs = VcsState::default();
        let repositories = Some(strings(&["/a", "/b", "/c"]));
        h.log.open_log_tab(LogTabKind::External, "/a", LogTabLocation::ToolWindow, repositories, &h.vcs);
        h.log.view.filters = LogFilters { paths: Some(strings(&["/a/src", "/b/test"])), ..LogFilters::default() };
        h.reload(false);
        let sent: Vec<(Option<&str>, Vec<String>)> =
            h.calls.iter().map(|(repo, query)| (repo.as_deref(), paths(query))).collect();
        assert_eq!(sent, [(Some("/a"), strings(&["/a/src"])), (Some("/b"), strings(&["/b/test"]))]);
        h.calls.clear();
        h.log.view.filters = LogFilters { path: Some("/outside/file".to_owned()), ..LogFilters::default() };
        h.reload(false);
        assert!(h.calls.is_empty());
        assert!(h.log.view.commits.is_empty());
    }

    // src/features/vcs/__tests__/logLoading.test.ts › incremental log refresh › cancels a superseded page and ignores its later response
    #[test]
    fn cancels_a_superseded_page_and_ignores_its_later_response() {
        let mut h = Harness::new();
        let old = h.start_reload(MAIN_LOG, false).expect("the log is read");
        let request = old.requests[0].query.request.expect("the query can be cancelled");
        h.answer_with(vec![commit("fresh", &[], &[])]);
        h.reload(false);
        assert!(h.cancelled.contains(&request));
        h.settle(old, vec![vec![commit("stale", &[], &[])]]);
        assert_eq!(h.ids(), ["fresh"]);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › resolves references in the repositories of a standalone multi-root log
    #[test]
    fn resolves_references_in_the_repositories_of_a_standalone_multi_root_log() {
        let mut h = Harness::new();
        h.vcs = VcsState::default();
        let repositories = Some(strings(&["/a", "/b"]));
        h.log.open_log_tab(LogTabKind::External, "/a", LogTabLocation::ToolWindow, repositories, &h.vcs);
        h.answer = Box::new(|repo, _| Ok(repo.map(|_| commit("target", &[], &[])).into_iter().collect()));
        h.reload(false);
        let (found, offered) = h.go_to_ref("main", |candidates| candidates.get(1).cloned());
        assert!(found);
        let repos: Vec<Option<&str>> = offered[0].iter().map(|c| c.repo.as_deref()).collect();
        assert_eq!(repos, [Some("/a"), Some("/b")]);
        assert_eq!(h.log.view.selected, Some(commit_key("target", Some("/b"))));
        assert!(h.calls.iter().all(|(repo, _)| matches!(repo.as_deref(), Some("/a" | "/b"))));
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › jumps to an indexed window beyond thirty pages without paging through the prefix
    #[test]
    fn jumps_to_an_indexed_window_beyond_thirty_pages_without_paging_through_the_prefix() {
        let mut h = Harness::new();
        let history: Vec<Commit> = (0..PAGE * 31 + 7).map(|i| commit(&format!("commit{i}"), &[], &[])).collect();
        let target = history.last().expect("the history has commits").clone();
        let wanted = target.id.clone();
        h.answer = Box::new(move |_, query| {
            let wanted = Some(wanted.as_str());
            let jump = query.branch.as_deref() == wanted || query.seek.as_deref() == wanted;
            Ok(if jump {
                vec![target.clone()]
            } else {
                history.iter().skip(query.skip as usize).take(query.limit as usize).cloned().collect()
            })
        });
        let last = format!("commit{}", PAGE * 31 + 6);
        let (found, _) = h.go_to_ref(&last, |_| None);
        assert!(found);
        assert_eq!(h.log.view.selected, Some(commit_key(&last, Some("/repo"))));
        assert!(h.calls.iter().any(|(_, query)| query.seek.as_deref() == Some(last.as_str())));
        assert!(h.calls.len() < 5);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › navigates the displayed collapsed adjacency in both directions
    #[test]
    fn navigates_the_displayed_collapsed_adjacency_in_both_directions() {
        let mut h = Harness::new();
        let rows = [commit("tip", &["middle"], &[]), commit("middle", &["base"], &[]), commit("base", &[], &[])];
        h.log.view.commits = rows.into_iter().map(RepoCommit::from).collect();
        h.log.view.done = true;
        h.set_graph_option(GraphOption::CollapseLinear(true));
        let Navigation::Select(parent) = go_to_parent(&h.log.view, &h.displayed(), "tip", false) else {
            panic!("the tip has one displayed parent");
        };
        h.log.select_commit(Some(&parent), true);
        assert_eq!(h.log.view.selected.as_deref(), Some("base"));
        let Navigation::Select(child) = go_to_child(&h.log.view, &h.displayed(), "base") else {
            panic!("the base has one displayed child");
        };
        h.log.select_commit(Some(&child), true);
        assert_eq!(h.log.view.selected.as_deref(), Some("tip"));
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › expands merges in linearized mode and retains expansion across new pages
    #[test]
    fn expands_merges_in_linearized_mode_and_retains_expansion_across_new_pages() {
        let mut h = Harness::new();
        let rows = [
            commit("merge", &["main", "side"], &[]),
            commit("side", &["base"], &[]),
            commit("main", &["base"], &[]),
            commit("base", &[], &[]),
        ];
        h.log.view.commits = rows.into_iter().map(RepoCommit::from).collect();
        h.options.linearize_merges = true;
        assert_eq!(h.displayed().graph[0].parents, ["side"]);
        h.set_graph_option(GraphOption::CollapseLinear(false));
        assert_eq!(h.displayed().graph[0].parents, ["main", "side"]);
        h.set_graph_option(GraphOption::CollapseLinear(true));
        assert_eq!(h.displayed().graph[0].parents, ["side"]);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › keeps selections and ranges spanning repository roots
    #[test]
    fn keeps_selections_and_ranges_spanning_repository_roots() {
        let mut h = Harness::new();
        let rows = vec![repo_commit(commit("a", &[], &[]), "/a"), repo_commit(commit("b", &[], &[]), "/b")];
        let keys: Vec<String> = rows.iter().map(RepoCommit::key).collect();
        h.log.view.commits.clone_from(&rows);
        h.log.view.selection = vec![keys[0].clone()];
        h.log.toggle_commit_selection(&keys[1]);
        assert_eq!(h.log.view.selection, keys);
        h.log.extend_commit_selection(&keys[1], &rows);
        assert_eq!(h.log.view.selection, keys);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › sends branches and independent ranges without dropping either
    #[test]
    fn sends_branches_and_independent_ranges_without_dropping_either() {
        use super::super::log_branch_memory::set_log_branches;
        use crate::platform::storage::UiStorage;

        let mut h = Harness::new();
        let mut scope = None;
        let mut storage = UiStorage::default();
        let chosen = strings(&["main", "a..b", "c..d"]);
        set_log_branches(&mut h.log, &mut h.loads, &mut scope, &h.vcs, &mut storage, &chosen);
        h.reload(false);
        let query = h.last_query();
        assert_eq!(query.branches, ["main"]);
        assert_eq!(query.range, None);
        assert_eq!(query.ranges, ["a..b", "c..d"]);
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › offers both merge parents and selects the chosen repository-scoped commit
    #[test]
    fn offers_both_merge_parents_and_selects_the_chosen_repository_scoped_commit() {
        let mut h = Harness::new();
        let rows = [commit("merge", &["main", "side"], &[]), commit("main", &[], &[]), commit("side", &[], &[])];
        h.log.view.commits = rows.into_iter().map(|c| repo_commit(c, "/repo")).collect();
        h.log.view.done = true;
        let merge = h.log.view.commits[0].key();
        let Navigation::Choose { candidates, .. } = go_to_parent(&h.log.view, &h.displayed(), &merge, false) else {
            panic!("a merge offers its parents");
        };
        let offered: Vec<String> = candidates.iter().map(RepoCommit::key).collect();
        let parents: Vec<String> = h.log.view.commits[1..].iter().map(RepoCommit::key).collect();
        assert_eq!(offered, parents);
        h.log.select_commit(Some(&offered[1]), true);
        assert_eq!(h.log.view.selected, Some(commit_key("side", Some("/repo"))));
    }

    // src/features/vcs/__tests__/logLoading.test.ts › commit navigation › does not navigate after a reference lookup is superseded by a filter change
    #[test]
    fn does_not_navigate_after_a_reference_lookup_is_superseded_by_a_filter_change() {
        let mut h = Harness::new();
        let (mut go, step) = GoToRef::begin(&h.loads, &h.log, &h.vcs, "target", None);
        assert!(matches!(step, GoToRefStep::Lookup(_)));
        h.answer_with(vec![commit("filtered", &[], &[])]);
        h.reload(false);
        let vcs = h.vcs.clone();
        let options = h.options.clone();
        let env = LoadEnv { vcs: &vcs, options: &options, history_limit: None };
        let step = go.found(&mut h.loads, &mut h.log, &vcs, vec![Some(commit("target", &[], &[]))], env);
        assert!(matches!(step, GoToRefStep::Done(false)));
        assert_eq!(h.log.view.selected, None);
        assert_eq!(h.ids()[0], "filtered");
    }

    // Not from a spec: commits are the user's own by email, by name or by the name part of the email.
    #[test]
    fn tells_the_users_own_commits() {
        let mut mine = commit("a", &[], &[]);
        mine.author = "Ada Lovelace".to_owned();
        mine.email = "Ada@Corp.test".to_owned();
        assert!(is_mine(&mine, Some("ada@corp.test"), None));
        assert!(is_mine(&mine, Some("other@corp.test"), Some("ada-lovelace")));
        assert!(is_mine(&mine, Some("a.d.a@elsewhere.test"), Some("Someone")));
        assert!(!is_mine(&mine, Some("bob@corp.test"), Some("Bob")));
        assert!(!is_mine(&mine, None, None));
    }
}
