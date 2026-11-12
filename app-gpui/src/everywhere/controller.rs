//! Runs the Search Everywhere sources for a query. Every source fills its own section independently (the
//! first results show as soon as any source answers); a new query supersedes the old one, cancelling the
//! backend requests that can be cancelled. Sections whose inputs did not change are not fetched again
//! ("Show more" on one section).
//!
//! The controller calls nothing: [`SeController::run`] answers what it can at once (actions) and returns a
//! [`SeEffect`] for each source to ask, and the answers come back through [`SeController::answer`] with the
//! [`Ticket`] of the request, which tells an answer to a superseded query from a current one.

use super::filters::{ResolvedFilter, passes_filter};
use super::model::{
    ALL_TAB_LIMIT, ActionItem, ActionMatchOptions, LineMatch, MergeOptions, RECENT_LIMIT, SeItem, SeTab, SectionId,
    SectionState, Sections, SettingsSearchEntry, SymbolHits, SymbolItem, SymbolMatch, TAB_LIMIT, WorkspaceSymbol,
    action_item, file_item, match_actions, match_settings, merge_action_results, merge_symbols, sections_for,
    text_items,
};
use super::open::{rel_to_root, utf16_len};
use super::routes::{EndpointEntry, match_routes};
use crate::find::ipc::{FileMatch, SearchQuery, SearchScope, SymbolFilter as IndexFilter};
use crate::find::query::parse_location_query;
use crate::platform::actions::ActionMeta;
use crate::platform::i18n::I18n;
use crate::platform::project_roots::ProjectFileSet;
use ide_core::{RequestId, SymbolKind};
use std::collections::{BTreeMap, HashMap};

/// The optional sources the app has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Sources {
    /// Language servers answer workspace symbol queries.
    pub workspace_symbols: bool,
    /// The settings search index (settings pages and options listed with actions).
    pub settings: bool,
    /// Endpoint providers, for request-path matches.
    pub routes: bool,
}

/// Whether an action can run where the dialog was opened.
pub type IsEnabled<'a> = &'a dyn Fn(&ActionMeta) -> bool;

/// Whether a path is in a root-based named scope ("Project Test Files" …).
pub type InFileSet<'a> = &'a dyn Fn(ProjectFileSet, &str) -> bool;

/// What the controller reads from the app while it runs a request or takes an answer.
#[derive(Clone, Copy)]
pub struct SeEnv<'a> {
    pub actions: &'a [&'a ActionMeta],
    pub is_enabled: IsEnabled<'a>,
    /// Ids of the recently run actions, most recent first.
    pub recent_actions: &'a [String],
    pub root: Option<&'a str>,
    /// The current time in milliseconds, on any clock that only moves forward.
    pub now: f64,
    /// `None` when the app cannot tell which files a named scope holds: nothing is narrowed then.
    pub in_file_set: Option<InFileSet<'a>>,
    /// Extension → language id ([`super::filters::extension_index`]); `None` while the languages are not
    /// known, and no file is filtered out by language then.
    pub languages: Option<&'a HashMap<String, String>>,
    pub i18n: &'a I18n,
    pub sources: Sources,
}

/// What the dialog asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeRequest {
    pub query: String,
    pub tab: SeTab,
    pub include_non_project: bool,
    /// "Project Production Files" / "Project Test Files"; unset or `All`: every project file.
    pub file_set: Option<ProjectFileSet>,
    /// The tab's funnel filter (file types, languages, kinds, file mask).
    pub filter: Option<ResolvedFilter>,
    /// Actions: `Some(false)` leaves the disabled ones out; unset lists them.
    pub include_disabled: Option<bool>,
    /// Extra rows per section, asked for with "Show more".
    pub extra: BTreeMap<SectionId, usize>,
}

impl SeRequest {
    pub fn new(query: &str, tab: SeTab) -> Self {
        Self {
            query: query.to_owned(),
            tab,
            include_non_project: false,
            file_set: None,
            filter: None,
            include_disabled: None,
            extra: BTreeMap::new(),
        }
    }
}

/// The sections as they are now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeSnapshot {
    /// Counts the requests run so far.
    pub generation: u64,
    pub sections: Sections,
    /// Milliseconds from the start of the query to its latest result; `None` while nothing arrived.
    pub elapsed: Option<u64>,
    /// True while any section is still loading.
    pub loading: bool,
    /// The query and tab these sections answer (results of an older query may still be shown).
    pub query: String,
    pub tab: Option<SeTab>,
}

/// Shortest query the text search runs for, in UTF-16 code units.
pub const TEXT_MIN_QUERY: usize = 2;

/// How many rows a section shows on a tab, with the `extra` ones "Show more" asked for.
pub fn section_limit(tab: SeTab, section: SectionId, extra: usize) -> usize {
    let base = match section {
        SectionId::Recent | SectionId::RecentActions => RECENT_LIMIT,
        _ if tab == SeTab::All => ALL_TAB_LIMIT,
        _ => TAB_LIMIT,
    };
    base + extra
}

/// Identifies what a source was asked: answers to a superseded request are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    pub section: SectionId,
    pub generation: u64,
}

/// A question to a source. Each is answered with the [`SeAnswer`] of the same name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeFetch {
    RecentFiles,
    /// Files by name; `limit` is how many the index is asked for.
    Files {
        query: String,
        limit: usize,
    },
    /// Indexed symbols; `limit` is how many the index is asked for.
    Symbols {
        query: String,
        filter: IndexFilter,
        limit: usize,
    },
    /// Language-server symbols, cancellable through `request`.
    WorkspaceSymbols {
        request: RequestId,
        query: String,
    },
    /// The project's endpoints.
    Routes,
    /// The settings search index.
    Settings,
    /// A streamed text search, cancellable through `request`: every file is answered with
    /// [`SeAnswer::TextFile`], the end with [`SeAnswer::TextDone`].
    Text {
        request: RequestId,
        query: SearchQuery,
    },
}

/// What the controller asks of its driver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeEffect {
    /// Stop this backend request.
    Cancel(RequestId),
    /// Ask a source and hand what it says to [`SeController::answer`] with this ticket.
    Fetch { ticket: Ticket, fetch: SeFetch },
}

/// What a source said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeAnswer {
    RecentFiles(Vec<String>),
    Files(Vec<FileMatch>),
    Symbols(Vec<SymbolMatch>),
    /// `None` when no language server answered (none is running, or the request was cancelled).
    WorkspaceSymbols(Option<Vec<WorkspaceSymbol>>),
    Routes(Vec<EndpointEntry>),
    Settings(Vec<SettingsSearchEntry>),
    /// The matches of one file of the text search.
    TextFile {
        path: String,
        rel_path: String,
        matches: Vec<LineMatch>,
    },
    TextDone,
    /// The source is unavailable (no project, no index yet): it contributes nothing.
    Failed,
}

/// The inputs a section was fetched for; a section is fetched again only when they change.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Signature {
    query: String,
    tab: SeTab,
    include_non_project: bool,
    file_set: ProjectFileSet,
    filter: Option<ResolvedFilter>,
    include_disabled: Option<bool>,
    limit: usize,
}

/// A symbol search under way: the index and the language servers answer separately.
#[derive(Debug)]
struct SymbolSearch {
    text: String,
    limit: usize,
    /// How many symbols the index was asked for.
    fetch: usize,
    types_only: bool,
    exclude_types: bool,
    include_non_project: bool,
    scope: Option<ProjectFileSet>,
    filter: Option<ResolvedFilter>,
    index: Vec<SymbolMatch>,
    lsp: Vec<WorkspaceSymbol>,
    index_done: bool,
    lsp_done: bool,
}

/// What a section waits for, with what its answer is turned into results by.
#[derive(Debug)]
enum Pending {
    Recent {
        limit: usize,
    },
    Files {
        limit: usize,
        line: Option<u32>,
        col: Option<u32>,
        scope: Option<ProjectFileSet>,
        filter: Option<ResolvedFilter>,
    },
    Symbols(SymbolSearch),
    Endpoints {
        text: String,
        limit: usize,
        scope: Option<ProjectFileSet>,
    },
    /// The actions are listed; the settings follow when their index has loaded.
    Actions {
        hits: Vec<ActionItem>,
        text: String,
        limit: usize,
    },
    Text {
        items: Vec<SeItem>,
        has_more: bool,
        limit: usize,
    },
}

/// What a section is started with.
#[derive(Clone, Copy)]
struct Start<'a> {
    /// The query without its location suffix.
    text: &'a str,
    line: Option<u32>,
    col: Option<u32>,
    request: &'a SeRequest,
    limit: usize,
}

/// The root-based scope that narrows a request, when there is one and the app can tell which files are in it.
fn scope_of(request: &SeRequest, env: &SeEnv<'_>) -> Option<ProjectFileSet> {
    let narrowed = !request.include_non_project && env.in_file_set.is_some();
    request.file_set.filter(|set| narrowed && *set != ProjectFileSet::All)
}

fn in_scope(env: &SeEnv<'_>, scope: Option<ProjectFileSet>, path: &str) -> bool {
    match (scope, env.in_file_set) {
        (Some(set), Some(in_file_set)) => in_file_set(set, path),
        _ => true,
    }
}

fn passes(env: &SeEnv<'_>, filter: Option<&ResolvedFilter>, path: &str, kind: Option<SymbolKind>) -> bool {
    filter.is_none_or(|filter| passes_filter(filter, path, kind, env.languages))
}

/// The first `limit` of `items`, and whether there are more.
fn cut<T>(items: Vec<T>, limit: usize, into: impl Fn(T) -> SeItem) -> (Vec<SeItem>, bool) {
    let has_more = items.len() > limit;
    (items.into_iter().take(limit).map(into).collect(), has_more)
}

#[derive(Debug, Default)]
pub struct SeController {
    generation: u64,
    sections: Sections,
    signatures: BTreeMap<SectionId, Signature>,
    /// The generation each loading section was started in.
    live: BTreeMap<SectionId, u64>,
    pending: BTreeMap<SectionId, Pending>,
    /// The cancellable backend requests of each loading section.
    requests: BTreeMap<SectionId, Vec<RequestId>>,
    started: f64,
    query: String,
    tab: Option<SeTab>,
    elapsed: Option<u64>,
    disposed: bool,
}

impl SeController {
    /// Makes the next [`SeController::run`] ask for the endpoints again, whatever the request.
    pub fn invalidate_endpoints(&mut self) {
        self.signatures.remove(&SectionId::Endpoints);
    }

    /// Starts the sections `request` needs and drops those it does not. `next_request_id` allocates the ids
    /// of the cancellable backend requests.
    pub fn run(
        &mut self,
        request: &SeRequest,
        env: &SeEnv<'_>,
        next_request_id: &mut dyn FnMut() -> RequestId,
    ) -> Vec<SeEffect> {
        self.generation += 1;
        let generation = self.generation;
        self.disposed = false;
        self.query = request.query.clone();
        self.tab = Some(request.tab);
        let parsed = parse_location_query(&request.query, env.root);
        let text = parsed.text.trim();
        let wanted = sections_for(request.tab, text.is_empty());
        self.started = env.now;
        self.elapsed = None;

        let mut effects = Vec::new();
        let unwanted: Vec<SectionId> = self.sections.keys().copied().filter(|id| !wanted.contains(id)).collect();
        for section in unwanted {
            self.cancel_section(section, &mut effects);
            self.sections.remove(&section);
            self.signatures.remove(&section);
        }
        for section in wanted {
            let limit = section_limit(request.tab, section, request.extra.get(&section).copied().unwrap_or(0));
            let signature = Signature {
                query: request.query.clone(),
                tab: request.tab,
                include_non_project: request.include_non_project,
                file_set: request.file_set.unwrap_or(ProjectFileSet::All),
                filter: request.filter.clone(),
                include_disabled: request.include_disabled,
                limit,
            };
            if self.signatures.get(&section) == Some(&signature) && self.sections.contains_key(&section) {
                continue;
            }
            self.signatures.insert(section, signature);
            self.cancel_section(section, &mut effects);
            // The rows of the previous query stay until this one answers.
            self.sections.entry(section).or_default().loading = true;
            self.live.insert(section, generation);
            let start = Start { text, line: parsed.line, col: parsed.col, request, limit };
            self.start(Ticket { section, generation }, start, env, next_request_id, &mut effects);
        }
        effects
    }

    /// Cancels everything in flight; a later [`SeController::run`] starts afresh.
    pub fn dispose(&mut self) -> Vec<SeEffect> {
        self.disposed = true;
        let mut effects = Vec::new();
        let sections: Vec<SectionId> = self.sections.keys().copied().collect();
        for section in sections {
            self.cancel_section(section, &mut effects);
        }
        self.sections.clear();
        self.signatures.clear();
        effects
    }

    pub fn sections(&self) -> &Sections {
        &self.sections
    }

    /// True while any section is still loading.
    pub fn is_loading(&self) -> bool {
        self.sections.values().any(|section| section.loading)
    }

    pub fn snapshot(&self) -> SeSnapshot {
        SeSnapshot {
            generation: self.generation,
            sections: self.sections.clone(),
            elapsed: self.elapsed,
            loading: self.is_loading(),
            query: self.query.clone(),
            tab: self.tab,
        }
    }

    fn cancel_section(&mut self, section: SectionId, effects: &mut Vec<SeEffect>) {
        self.live.remove(&section);
        self.pending.remove(&section);
        effects.extend(self.requests.remove(&section).unwrap_or_default().into_iter().map(SeEffect::Cancel));
    }

    fn is_live(&self, ticket: Ticket) -> bool {
        !self.disposed && self.live.get(&ticket.section) == Some(&ticket.generation)
    }

    /// Publishes a section's results; a section that is done waits for nothing more.
    fn set(&mut self, ticket: Ticket, items: Vec<SeItem>, has_more: bool, loading: bool, env: &SeEnv<'_>) {
        if !self.is_live(ticket) {
            return;
        }
        if !items.is_empty() {
            // A clock that went backwards reads as no time passed.
            self.elapsed = Some((env.now - self.started).round().max(0.0) as u64);
        }
        self.sections.insert(ticket.section, SectionState { items, has_more, loading });
        if !loading {
            self.live.remove(&ticket.section);
            self.requests.remove(&ticket.section);
            self.pending.remove(&ticket.section);
        }
    }

    fn track(&mut self, section: SectionId, request: RequestId) -> RequestId {
        self.requests.entry(section).or_default().push(request);
        request
    }

    fn start(
        &mut self,
        ticket: Ticket,
        start: Start<'_>,
        env: &SeEnv<'_>,
        next_request_id: &mut dyn FnMut() -> RequestId,
        effects: &mut Vec<SeEffect>,
    ) {
        let Start { text, line, col, request, limit } = start;
        let section = ticket.section;
        let scope = scope_of(request, env);
        let mut ask = |fetch: SeFetch| effects.push(SeEffect::Fetch { ticket, fetch });
        match section {
            SectionId::Recent => {
                self.pending.insert(section, Pending::Recent { limit });
                ask(SeFetch::RecentFiles);
            }
            SectionId::RecentActions => {
                let recent = env.recent_actions.iter();
                let known = recent.filter_map(|id| env.actions.iter().copied().find(|action| action.id == *id));
                let items = known
                    .filter(|action| !action.hidden)
                    .take(limit)
                    .map(|action| SeItem::Action(action_item(action, (env.is_enabled)(action), env.i18n)))
                    .collect();
                self.set(ticket, items, false, false, env);
            }
            SectionId::Files => {
                let filter = request.filter.clone();
                // Over-fetch so that a narrowed scope still fills the section.
                let fetch = if scope.is_some() || filter.is_some() { limit * 4 + 1 } else { limit + 1 };
                self.pending.insert(section, Pending::Files { limit, line, col, scope, filter });
                ask(SeFetch::Files { query: text.to_owned(), limit: fetch });
            }
            SectionId::Classes | SectionId::Symbols => {
                let types_only = section == SectionId::Classes;
                let exclude_types = !types_only && request.tab == SeTab::All;
                let filter = request.filter.clone();
                // Over-fetch in the All tab (types are filtered out of Symbols there) and under a filter.
                let fetch = match (&filter, exclude_types) {
                    (Some(_), _) => limit * 4 + 1,
                    (None, true) => limit * 2 + 1,
                    (None, false) => limit + 1,
                };
                let with_servers = env.sources.workspace_symbols;
                if with_servers {
                    let id = self.track(section, next_request_id());
                    ask(SeFetch::WorkspaceSymbols { request: id, query: text.to_owned() });
                }
                let search = SymbolSearch {
                    text: text.to_owned(),
                    limit,
                    fetch,
                    types_only,
                    exclude_types,
                    include_non_project: request.include_non_project,
                    scope,
                    filter,
                    index: Vec::new(),
                    lsp: Vec::new(),
                    index_done: false,
                    lsp_done: !with_servers,
                };
                self.pending.insert(section, Pending::Symbols(search));
                let filter = if types_only { IndexFilter::Types } else { IndexFilter::All };
                ask(SeFetch::Symbols { query: text.to_owned(), filter, limit: fetch });
            }
            SectionId::Endpoints => {
                if env.sources.routes {
                    self.pending.insert(section, Pending::Endpoints { text: text.to_owned(), limit, scope });
                    ask(SeFetch::Routes);
                } else {
                    self.set(ticket, Vec::new(), false, false, env);
                }
            }
            SectionId::Actions => {
                let options = ActionMatchOptions {
                    is_enabled: env.is_enabled,
                    recent: env.recent_actions,
                    include_disabled: request.include_disabled.unwrap_or(true),
                };
                let hits = match_actions(env.actions.iter().copied(), text, &options, env.i18n);
                // Actions answer at once; settings follow when their index has loaded.
                let with_settings = env.sources.settings && !text.is_empty();
                let (items, has_more) = cut(hits.clone(), limit, SeItem::Action);
                self.set(ticket, items, has_more, with_settings, env);
                if with_settings {
                    self.pending.insert(section, Pending::Actions { hits, text: text.to_owned(), limit });
                    ask(SeFetch::Settings);
                }
            }
            SectionId::Text => {
                if utf16_len(text) < TEXT_MIN_QUERY {
                    self.set(ticket, Vec::new(), false, false, env);
                    return;
                }
                let mask = request.filter.as_ref().and_then(|filter| filter.mask.clone());
                let query = SearchQuery {
                    pattern: text.to_owned(),
                    include: mask.filter(|mask| !mask.is_empty()).into_iter().collect(),
                    scope: match request.file_set {
                        Some(set) if set != ProjectFileSet::All => SearchScope::FileSet { set },
                        _ => SearchScope::Project,
                    },
                    // One more than is shown tells whether there are more.
                    max_matches: u32::try_from(limit + 1).unwrap_or(u32::MAX),
                    ..SearchQuery::default()
                };
                let id = self.track(section, next_request_id());
                self.pending.insert(section, Pending::Text { items: Vec::new(), has_more: false, limit });
                ask(SeFetch::Text { request: id, query });
            }
        }
    }

    /// Takes what a source said. An answer to a superseded request, or one the section does not wait for,
    /// changes nothing.
    pub fn answer(&mut self, ticket: Ticket, answer: SeAnswer, env: &SeEnv<'_>) {
        if !self.is_live(ticket) {
            return;
        }
        let Some(pending) = self.pending.remove(&ticket.section) else { return };
        match (pending, answer) {
            (Pending::Recent { limit }, SeAnswer::RecentFiles(paths)) => {
                let (items, has_more) = cut(paths, limit, |path| {
                    SeItem::File(file_item(&path, rel_to_root(env.root, &path), &[], None, None))
                });
                self.set(ticket, items, has_more, false, env);
            }
            (Pending::Files { limit, line, col, scope, filter }, SeAnswer::Files(found)) => {
                let filter = filter.as_ref();
                let keep = |path: &str| in_scope(env, scope, path) && passes(env, filter, path, None);
                let kept: Vec<FileMatch> = found.into_iter().filter(|file| keep(&file.path)).collect();
                let (items, has_more) = cut(kept, limit, |file| {
                    SeItem::File(file_item(&file.path, &file.rel_path, &file.indices, line, col))
                });
                self.set(ticket, items, has_more, false, env);
            }
            (Pending::Symbols(mut search), SeAnswer::Symbols(index)) => {
                search.index = index;
                search.index_done = true;
                self.publish_symbols(ticket, search, env);
            }
            // Without the index the language servers' symbols are still listed.
            (Pending::Symbols(mut search), SeAnswer::Failed) => {
                search.index_done = true;
                self.publish_symbols(ticket, search, env);
            }
            (Pending::Symbols(mut search), SeAnswer::WorkspaceSymbols(found)) => {
                search.lsp = found.unwrap_or_default();
                search.lsp_done = true;
                if search.index_done {
                    self.publish_symbols(ticket, search, env);
                } else {
                    self.pending.insert(ticket.section, Pending::Symbols(search));
                }
            }
            (Pending::Endpoints { text, limit, scope }, SeAnswer::Routes(routes)) => {
                let SymbolHits { items, has_more } = match_routes(&routes, &text, limit);
                let kept = items.into_iter().filter(|item| in_scope(env, scope, &item.path));
                self.set(ticket, kept.map(SeItem::Symbol).collect(), has_more, false, env);
            }
            (Pending::Actions { hits, text, limit }, SeAnswer::Settings(entries)) => {
                let all = merge_action_results(hits, match_settings(&entries, &text, env.i18n));
                let (items, has_more) = cut(all, limit, |item| item);
                self.set(ticket, items, has_more, false, env);
            }
            // The actions stay as they are when the settings index does not load.
            (Pending::Actions { hits, limit, .. }, SeAnswer::Failed) => {
                let (items, has_more) = cut(hits, limit, SeItem::Action);
                self.set(ticket, items, has_more, false, env);
            }
            (Pending::Text { mut items, mut has_more, limit }, SeAnswer::TextFile { path, rel_path, matches }) => {
                // Once the limit is reached the files still streaming in add nothing.
                if !has_more {
                    for item in text_items(&path, &rel_path, &matches) {
                        if items.len() >= limit {
                            has_more = true;
                            break;
                        }
                        items.push(SeItem::Text(item));
                    }
                    self.set(ticket, items.clone(), has_more, true, env);
                }
                self.pending.insert(ticket.section, Pending::Text { items, has_more, limit });
            }
            (Pending::Text { items, has_more, .. }, SeAnswer::TextDone) => {
                self.set(ticket, items, has_more, false, env);
            }
            (_, SeAnswer::Failed) => self.set(ticket, Vec::new(), false, false, env),
            // Not what the section waits for: it keeps waiting.
            (pending, _) => {
                self.pending.insert(ticket.section, pending);
            }
        }
    }

    /// Lists the symbols known so far; the section stays loading until both the index and the language
    /// servers have answered.
    fn publish_symbols(&mut self, ticket: Ticket, search: SymbolSearch, env: &SeEnv<'_>) {
        let filtered = search.filter.is_some();
        let options = MergeOptions {
            root: env.root,
            include_non_project: search.include_non_project,
            types_only: search.types_only,
            exclude_types: search.exclude_types,
            // Under a filter everything fetched is merged, and the section is cut after filtering.
            limit: if filtered { search.fetch } else { search.limit },
        };
        let merged = merge_symbols(&search.index, &search.lsp, &search.text, &options);
        let (scope, filter) = (search.scope, search.filter.as_ref());
        let keep = |item: &SymbolItem| {
            in_scope(env, scope, &item.path) && passes(env, filter, &item.path, Some(item.symbol_kind))
        };
        let passed: Vec<SeItem> = merged.items.into_iter().filter(keep).map(SeItem::Symbol).collect();
        let passed_count = passed.len();
        let items: Vec<SeItem> = if filtered { passed.into_iter().take(search.limit).collect() } else { passed };
        let has_more = merged.has_more || search.index.len() >= search.fetch || passed_count > items.len();
        let loading = !(search.index_done && search.lsp_done);
        self.set(ticket, items, has_more, loading, env);
        if loading && self.is_live(ticket) {
            self.pending.insert(ticket.section, Pending::Symbols(search));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::filters::{
        LanguageFilter, SeFilters, SymbolFilter as TabSymbolFilter, TextFilter, extension_index, resolve_filter,
    };
    use super::super::model::{SeRow, build_rows};
    use super::*;
    use ide_core::{IpcPath, Location, Pos, Range};
    use std::path::PathBuf;

    fn enabled(_: &ActionMeta) -> bool {
        true
    }

    fn is_in_file_set(set: ProjectFileSet, path: &str) -> bool {
        match set {
            ProjectFileSet::All => true,
            ProjectFileSet::Test => path.contains("/test/"),
            ProjectFileSet::Production => !path.contains("/test/"),
        }
    }

    /// An app with a project at `/p` and none of the optional sources.
    fn env(i18n: &I18n) -> SeEnv<'_> {
        SeEnv {
            actions: &[],
            is_enabled: &enabled,
            recent_actions: &[],
            root: Some("/p"),
            now: 0.0,
            in_file_set: None,
            languages: None,
            i18n,
            sources: Sources::default(),
        }
    }

    fn request_ids() -> impl FnMut() -> RequestId {
        let mut next = 0;
        move || {
            next += 1;
            RequestId(next)
        }
    }

    fn by_ext() -> HashMap<String, String> {
        extension_index([("typescript", &["ts", "d.ts"][..]), ("rust", &["rs"][..])])
    }

    fn strs(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn fm(rel: &str) -> FileMatch {
        FileMatch { path: format!("/p/{rel}"), rel_path: rel.to_owned(), score: 1, indices: Vec::new() }
    }

    fn sym(name: &str, kind: SymbolKind, rel: &str) -> SymbolMatch {
        SymbolMatch {
            name: name.to_owned(),
            kind,
            container: None,
            path: format!("/p/{rel}"),
            rel_path: rel.to_owned(),
            range: Range::new(Pos::new(0, 0), Pos::new(0, 1)),
            score: 1,
            indices: Vec::new(),
        }
    }

    fn entries() -> Vec<SettingsSearchEntry> {
        let entry = |page_id: &str, title: &str, path: &[&str], keywords: &[&str]| SettingsSearchEntry {
            page_id: page_id.to_owned(),
            title: title.to_owned(),
            path: strs(path),
            keywords: strs(keywords),
        };
        vec![
            entry("editor.general", "General", &["Editor"], &[]),
            entry("editor.general", "Show line numbers", &["Editor", "General"], &[]),
            entry("appearance", "Appearance", &[], &["theme"]),
        ]
    }

    /// The fetches among `effects`.
    fn fetches(effects: &[SeEffect]) -> Vec<&SeFetch> {
        fn fetch(effect: &SeEffect) -> Option<&SeFetch> {
            match effect {
                SeEffect::Fetch { fetch, .. } => Some(fetch),
                SeEffect::Cancel(_) => None,
            }
        }
        effects.iter().filter_map(fetch).collect()
    }

    /// The ticket of the fetch among `effects` that `wanted` picks.
    fn ticket(effects: &[SeEffect], wanted: impl Fn(&SeFetch) -> bool) -> Ticket {
        let found = effects.iter().find_map(|effect| match effect {
            SeEffect::Fetch { ticket, fetch } if wanted(fetch) => Some(*ticket),
            _ => None,
        });
        found.expect("the source is asked")
    }

    fn asks_files(fetch: &SeFetch) -> bool {
        matches!(fetch, SeFetch::Files { .. })
    }

    fn asks_symbols(fetch: &SeFetch) -> bool {
        matches!(fetch, SeFetch::Symbols { .. })
    }

    fn asks_text(fetch: &SeFetch) -> bool {
        matches!(fetch, SeFetch::Text { .. })
    }

    fn keys(controller: &SeController, section: SectionId) -> Vec<&str> {
        controller.sections()[&section].items.iter().map(SeItem::key).collect()
    }

    #[test]
    fn streams_each_section_independently() {
        let i18n = I18n::default();
        let action = ActionMeta::new("x", "Order Things");
        let actions = [&action];
        let env = SeEnv { actions: &actions, ..env(&i18n) };
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("ord", SeTab::All), &env, &mut request_ids());
        // Actions answered at once; files are still pending.
        assert_eq!(controller.sections()[&SectionId::Actions].items.len(), 1);
        assert!(controller.sections()[&SectionId::Files].loading);
        assert!(controller.snapshot().loading);
        controller.answer(ticket(&effects, asks_files), SeAnswer::Files(vec![fm("Order.kt")]), &env);
        assert_eq!(keys(&controller, SectionId::Files), ["f:/p/Order.kt"]);
    }

    #[test]
    fn drops_late_results_of_a_superseded_query_and_cancels_its_requests() {
        let i18n = I18n::default();
        let env = SeEnv { sources: Sources { workspace_symbols: true, ..Sources::default() }, ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let old = controller.run(&SeRequest::new("old", SeTab::Files), &env, &mut ids);
        let new = controller.run(&SeRequest::new("new", SeTab::Files), &env, &mut ids);
        controller.answer(ticket(&old, asks_files), SeAnswer::Files(vec![fm("Old.kt")]), &env);
        controller.answer(ticket(&new, asks_files), SeAnswer::Files(vec![fm("New.kt")]), &env);
        assert_eq!(keys(&controller, SectionId::Files), ["f:/p/New.kt"]);

        let first = controller.run(&SeRequest::new("sym", SeTab::Symbols), &env, &mut ids);
        let second = controller.run(&SeRequest::new("sym2", SeTab::Symbols), &env, &mut ids);
        let asked = fetches(&first);
        let Some(SeFetch::WorkspaceSymbols { request, .. }) = asked.first() else { panic!("{asked:?}") };
        assert!(second.contains(&SeEffect::Cancel(*request)));
    }

    #[test]
    fn parses_file_kt_42_and_opens_files_at_the_line() {
        let i18n = I18n::default();
        let env = env(&i18n);
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("File.kt:42:3", SeTab::Files), &env, &mut request_ids());
        assert_eq!(fetches(&effects), [&SeFetch::Files { query: "File.kt".to_owned(), limit: 51 }]);
        controller.answer(ticket(&effects, asks_files), SeAnswer::Files(vec![fm("File.kt")]), &env);
        let SeItem::File(item) = &controller.sections()[&SectionId::Files].items[0] else { panic!("a file") };
        assert_eq!((item.line, item.col), (Some(41), Some(2)));
    }

    #[test]
    fn shows_recent_files_for_an_empty_query() {
        let i18n = I18n::default();
        let env = env(&i18n);
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("", SeTab::All), &env, &mut request_ids());
        let recent = ticket(&effects, |fetch| *fetch == SeFetch::RecentFiles);
        controller.answer(recent, SeAnswer::RecentFiles(vec!["/p/a/B.kt".to_owned(), "/p/C.kt".to_owned()]), &env);
        let rels: Vec<&str> = controller.sections()[&SectionId::Recent]
            .items
            .iter()
            .map(|item| match item {
                SeItem::File(file) => file.rel.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(rels, ["a/B.kt", "C.kt"]);
    }

    #[test]
    fn re_fetches_only_the_section_whose_limit_changed_show_more() {
        let i18n = I18n::default();
        let env = env(&i18n);
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let first = controller.run(&SeRequest::new("a", SeTab::All), &env, &mut ids);
        controller.answer(ticket(&first, asks_files), SeAnswer::Files(vec![fm("A.kt")]), &env);
        let mut more = SeRequest::new("a", SeTab::All);
        more.extra.insert(SectionId::Files, 15);
        let second = controller.run(&more, &env, &mut ids);
        // The files are asked for twice, the second time for more of them.
        assert_eq!(fetches(&first).into_iter().filter(|fetch| asks_files(fetch)).count(), 1);
        assert_eq!(fetches(&second), [&SeFetch::Files { query: "a".to_owned(), limit: 22 }]);
        // Classes and symbols were asked for once each, by the first run.
        assert_eq!(fetches(&first).into_iter().filter(|fetch| asks_symbols(fetch)).count(), 2);
    }

    #[test]
    fn merges_lsp_symbols_after_the_index_answers_and_ignores_unavailable_servers() {
        let i18n = I18n::default();
        let env = SeEnv { sources: Sources { workspace_symbols: true, ..Sources::default() }, ..env(&i18n) };
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("ord", SeTab::Symbols), &env, &mut request_ids());
        let found = SymbolMatch {
            name: "order".to_owned(),
            kind: SymbolKind::Variable,
            container: None,
            path: "/p/A.kt".to_owned(),
            rel_path: "A.kt".to_owned(),
            range: Range::new(Pos::new(1, 0), Pos::new(1, 5)),
            score: 1,
            indices: vec![0],
        };
        let asked = ticket(&effects, asks_symbols);
        controller.answer(asked, SeAnswer::Symbols(vec![found]), &env);
        assert_eq!(controller.sections()[&SectionId::Symbols].items.len(), 1);
        assert!(controller.sections()[&SectionId::Symbols].loading);
        controller.answer(asked, SeAnswer::WorkspaceSymbols(None), &env);
        assert!(!controller.sections()[&SectionId::Symbols].loading);
        assert_eq!(controller.sections()[&SectionId::Symbols].items.len(), 1);
    }

    #[test]
    fn streams_text_matches_up_to_the_limit() {
        let i18n = I18n::default();
        let env = env(&i18n);
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("order", SeTab::All), &env, &mut request_ids());
        let asked = fetches(&effects).into_iter().find(|fetch| asks_text(fetch));
        let Some(SeFetch::Text { query, .. }) = asked else { panic!("the text search is asked") };
        assert_eq!(query.max_matches, 7);
        let searching = ticket(&effects, asks_text);
        for file in 0..3 {
            let line = |line: u32| LineMatch { line, text: "order".to_owned(), ranges: vec![(0, 5)], text_offset: 0 };
            let found = SeAnswer::TextFile {
                path: format!("/p/{file}.kt"),
                rel_path: format!("{file}.kt"),
                matches: vec![line(1), line(2), line(3)],
            };
            controller.answer(searching, found, &env);
        }
        controller.answer(searching, SeAnswer::TextDone, &env);
        let section = &controller.sections()[&SectionId::Text];
        assert_eq!(section.items.len(), 6);
        assert!(section.has_more);
        assert!(!section.loading);
    }

    #[test]
    fn narrows_files_symbols_text_to_project_production_or_test_files() {
        let i18n = I18n::default();
        let env = SeEnv { in_file_set: Some(&is_in_file_set), ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let found = || SeAnswer::Files(vec![fm("src/main/A.kt"), fm("src/test/ATest.kt")]);
        let scoped = |tab: SeTab, query: &str, file_set: ProjectFileSet, include_non_project: bool| SeRequest {
            file_set: Some(file_set),
            include_non_project,
            ..SeRequest::new(query, tab)
        };

        let effects = controller.run(&scoped(SeTab::Files, "a", ProjectFileSet::Test, false), &env, &mut ids);
        controller.answer(ticket(&effects, asks_files), found(), &env);
        assert_eq!(keys(&controller, SectionId::Files), ["f:/p/src/test/ATest.kt"]);
        assert_eq!(fetches(&effects), [&SeFetch::Files { query: "a".to_owned(), limit: 4 * 50 + 1 }]);

        let effects = controller.run(&scoped(SeTab::Files, "a", ProjectFileSet::Production, false), &env, &mut ids);
        controller.answer(ticket(&effects, asks_files), found(), &env);
        assert_eq!(keys(&controller, SectionId::Files), ["f:/p/src/main/A.kt"]);

        // "All Places" ignores the narrowed scope.
        let effects = controller.run(&scoped(SeTab::Files, "a", ProjectFileSet::Production, true), &env, &mut ids);
        controller.answer(ticket(&effects, asks_files), found(), &env);
        assert_eq!(controller.sections()[&SectionId::Files].items.len(), 2);

        let effects = controller.run(&scoped(SeTab::Text, "needle", ProjectFileSet::Test, false), &env, &mut ids);
        let asked = fetches(&effects);
        let Some(SeFetch::Text { query, .. }) = asked.last() else { panic!("the text search is asked") };
        assert_eq!(query.scope, SearchScope::FileSet { set: ProjectFileSet::Test });
    }

    #[test]
    fn answers_a_request_path_with_the_controller_method_that_serves_it() {
        let route = EndpointEntry {
            method: "POST".to_owned(),
            url: "/deeplink/menu/search".to_owned(),
            controller: "DeepLinkController".to_owned(),
            method_name: "searchMenus".to_owned(),
            path: "/p/tg-tenant-server/src/main/java/DeepLinkController.java".to_owned(),
            rel: "tg-tenant-server/src/main/java/DeepLinkController.java".to_owned(),
            line: 45,
            col: 4,
            ..EndpointEntry::default()
        };
        let i18n = I18n::default();
        let env = SeEnv { sources: Sources { routes: true, ..Sources::default() }, ..env(&i18n) };
        let mut controller = SeController::default();
        let query = "/tg-tenant/deeplink/menu/search";
        let effects = controller.run(&SeRequest::new(query, SeTab::All), &env, &mut request_ids());
        let asked = ticket(&effects, |fetch| *fetch == SeFetch::Routes);
        controller.answer(asked, SeAnswer::Routes(vec![route]), &env);
        let section = &controller.sections()[&SectionId::Endpoints];
        assert_eq!(section.items.len(), 1);
        let SeItem::Symbol(endpoint) = &section.items[0] else { panic!("a symbol") };
        assert_eq!(endpoint.container.as_deref(), Some("DeepLinkController.searchMenus"));
        assert_eq!(endpoint.line, 45);
        // The All tab mixes its sections into one list; an endpoint has to be in it.
        let rows = build_rows(controller.sections(), false, Some(query));
        let listed = rows.iter().any(|row| match row {
            SeRow::Item { item: SeItem::Symbol(symbol), .. } => {
                symbol.container.as_deref() == Some("DeepLinkController.searchMenus")
            }
            _ => false,
        });
        assert!(listed);
    }

    #[test]
    fn filters_files_by_file_type() {
        let i18n = I18n::default();
        let by_ext = by_ext();
        let env = SeEnv { languages: Some(&by_ext), ..env(&i18n) };
        let mut controller = SeController::default();
        let filters = SeFilters { files: LanguageFilter { languages: strs(&["rust"]) }, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Files);
        let request = SeRequest { filter, ..SeRequest::new("a", SeTab::Files) };
        let effects = controller.run(&request, &env, &mut request_ids());
        let found = SeAnswer::Files(vec![fm("a.rs"), fm("b.ts"), fm("c.md")]);
        controller.answer(ticket(&effects, asks_files), found, &env);
        assert_eq!(keys(&controller, SectionId::Files), ["f:/p/b.ts", "f:/p/c.md"]);
    }

    #[test]
    fn filters_symbols_by_kind_and_language() {
        let i18n = I18n::default();
        let by_ext = by_ext();
        let env = SeEnv { languages: Some(&by_ext), ..env(&i18n) };
        let mut controller = SeController::default();
        let excluded = TabSymbolFilter { languages: strs(&["rust"]), kinds: strs(&["variable"]) };
        let filters = SeFilters { symbols: excluded, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Symbols);
        let request = SeRequest { filter, ..SeRequest::new("run", SeTab::Symbols) };
        let effects = controller.run(&request, &env, &mut request_ids());
        let found = vec![
            sym("run", SymbolKind::Function, "a.rs"),
            sym("Run", SymbolKind::Class, "b.ts"),
            sym("runner", SymbolKind::Variable, "c.ts"),
        ];
        controller.answer(ticket(&effects, asks_symbols), SeAnswer::Symbols(found), &env);
        let names: Vec<&str> = controller.sections()[&SectionId::Symbols]
            .items
            .iter()
            .map(|item| match item {
                SeItem::Symbol(symbol) => symbol.name.as_str(),
                _ => "",
            })
            .collect();
        assert_eq!(names, ["Run"]);
        let asked = SeFetch::Symbols { query: "run".to_owned(), filter: IndexFilter::All, limit: 4 * 50 + 1 };
        assert_eq!(fetches(&effects), [&asked]);
    }

    #[test]
    fn passes_the_text_tabs_file_mask_to_the_search() {
        let i18n = I18n::default();
        let env = env(&i18n);
        let mut controller = SeController::default();
        let filters = SeFilters { text: TextFilter { mask: " *.ts, !*.test.* ".to_owned() }, ..SeFilters::default() };
        let filter = resolve_filter(&filters, SeTab::Text);
        let request = SeRequest { filter, ..SeRequest::new("needle", SeTab::Text) };
        let effects = controller.run(&request, &env, &mut request_ids());
        let asked = fetches(&effects);
        let Some(SeFetch::Text { query, .. }) = asked.last() else { panic!("the text search is asked") };
        assert_eq!(query.include, ["*.ts, !*.test.*"]);
    }

    #[test]
    fn hides_disabled_actions_unless_included() {
        let i18n = I18n::default();
        let (on, off) = (ActionMeta::new("a.on", "Reformat Code"), ActionMeta::new("a.off", "Reformat File"));
        let actions = [&on, &off];
        let is_enabled = |action: &ActionMeta| action.id == "a.on";
        let env = SeEnv { actions: &actions, is_enabled: &is_enabled, ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let request = |included: bool| SeRequest {
            include_non_project: included,
            include_disabled: Some(included),
            ..SeRequest::new("reformat", SeTab::Actions)
        };
        controller.run(&request(false), &env, &mut ids);
        assert_eq!(keys(&controller, SectionId::Actions), ["a:a.on"]);
        controller.run(&request(true), &env, &mut ids);
        assert_eq!(keys(&controller, SectionId::Actions), ["a:a.on", "a:a.off"]);
    }

    #[test]
    fn lists_settings_with_actions_once_the_index_answers() {
        let i18n = I18n::default();
        let action = ActionMeta::new("x", "Toggle Line Numbers");
        let actions = [&action];
        let sources = Sources { settings: true, ..Sources::default() };
        let env = SeEnv { actions: &actions, sources, ..env(&i18n) };
        let mut controller = SeController::default();
        let effects = controller.run(&SeRequest::new("line numbers", SeTab::Actions), &env, &mut request_ids());
        let asked = ticket(&effects, |fetch| *fetch == SeFetch::Settings);
        controller.answer(asked, SeAnswer::Settings(entries()), &env);
        let section = &controller.sections()[&SectionId::Actions];
        let kinds: Vec<&str> = section
            .items
            .iter()
            .map(|item| match item {
                SeItem::Action(_) => "action",
                SeItem::Setting(_) => "setting",
                SeItem::File(_) | SeItem::Symbol(_) | SeItem::Text(_) => "other",
            })
            .collect();
        assert_eq!(kinds, ["action", "setting"]);
        assert!(!section.loading);
    }

    // Not from a spec: settings join the actions once their index answers, the time to the latest result is
    // reported, and disposing cancels what is in flight.
    #[test]
    fn reports_the_time_to_the_latest_result_and_cancels_on_dispose() {
        let i18n = I18n::default();
        let action = ActionMeta::new("view.softWraps", "Soft Wraps");
        let actions = [&action];
        let sources = Sources { settings: true, ..Sources::default() };
        let mut env = SeEnv { actions: &actions, sources, ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let effects = controller.run(&SeRequest::new("soft wraps", SeTab::Actions), &env, &mut ids);
        assert_eq!(keys(&controller, SectionId::Actions), ["a:view.softWraps"]);
        assert!(controller.is_loading());
        assert_eq!(controller.snapshot().elapsed, Some(0));
        let entry = SettingsSearchEntry {
            page_id: "editor.general".to_owned(),
            title: "Soft Wraps".to_owned(),
            ..SettingsSearchEntry::default()
        };
        env.now = 12.4;
        let asked = ticket(&effects, |fetch| *fetch == SeFetch::Settings);
        controller.answer(asked, SeAnswer::Settings(vec![entry]), &env);
        assert_eq!(keys(&controller, SectionId::Actions), ["a:view.softWraps", "set:editor.general:Soft Wraps"]);
        let snapshot = controller.snapshot();
        assert_eq!((snapshot.loading, snapshot.elapsed, snapshot.tab), (false, Some(12), Some(SeTab::Actions)));
        assert_eq!((snapshot.generation, snapshot.query.as_str()), (1, "soft wraps"));

        let effects = controller.run(&SeRequest::new("needle", SeTab::Text), &env, &mut ids);
        assert_eq!(effects.len(), 1);
        assert_eq!(controller.dispose(), [SeEffect::Cancel(RequestId(1))]);
        assert!(controller.sections().is_empty());
        controller.answer(ticket(&effects, asks_text), SeAnswer::TextDone, &env);
        assert!(controller.sections().is_empty());
    }

    // Not from a spec: a source that fails contributes nothing, the index failing leaves the language
    // servers' symbols, and endpoints are asked for again once invalidated.
    #[test]
    fn failed_sources_contribute_nothing_and_endpoints_can_be_invalidated() {
        let i18n = I18n::default();
        let sources = Sources { workspace_symbols: true, routes: true, settings: false };
        let env = SeEnv { sources, ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        let request = SeRequest::new("order", SeTab::All);
        let effects = controller.run(&request, &env, &mut ids);
        controller.answer(ticket(&effects, asks_files), SeAnswer::Failed, &env);
        assert_eq!(controller.sections()[&SectionId::Files], SectionState::default());

        let location = Location {
            path: IpcPath(PathBuf::from("/p/A.kt")),
            range: Range::new(Pos::new(3, 0), Pos::new(3, 4)),
            uri: None,
        };
        let symbol =
            WorkspaceSymbol { name: "orderTotal".to_owned(), kind: SymbolKind::Function, container: None, location };
        let asked = Ticket { section: SectionId::Symbols, generation: 1 };
        controller.answer(asked, SeAnswer::WorkspaceSymbols(Some(vec![symbol])), &env);
        assert!(controller.sections()[&SectionId::Symbols].items.is_empty());
        controller.answer(asked, SeAnswer::Failed, &env);
        assert_eq!(keys(&controller, SectionId::Symbols), ["s:/p/A.kt:3:0:orderTotal"]);
        assert!(!controller.sections()[&SectionId::Symbols].loading);

        controller.answer(ticket(&effects, |fetch| *fetch == SeFetch::Routes), SeAnswer::Failed, &env);
        assert!(controller.sections()[&SectionId::Endpoints].items.is_empty());
        assert!(controller.run(&request, &env, &mut ids).is_empty());
        controller.invalidate_endpoints();
        assert_eq!(fetches(&controller.run(&request, &env, &mut ids)), [&SeFetch::Routes]);
        assert_eq!(section_limit(SeTab::All, SectionId::Recent, 5), 20);
        assert_eq!(section_limit(SeTab::Files, SectionId::Files, 0), 50);
    }

    // Not from a spec: the recent actions are the known, visible ones in the order they were run, and a
    // query of one character starts no text search.
    #[test]
    fn lists_recent_actions_and_skips_the_text_search_for_a_short_query() {
        let i18n = I18n::default();
        let shown = ActionMeta::new("a.shown", "Shown");
        let hidden = ActionMeta { hidden: true, ..ActionMeta::new("a.hidden", "Hidden") };
        let actions = [&shown, &hidden];
        let recent = strs(&["a.hidden", "a.gone", "a.shown"]);
        let env = SeEnv { actions: &actions, recent_actions: &recent, ..env(&i18n) };
        let mut controller = SeController::default();
        let mut ids = request_ids();
        assert!(controller.run(&SeRequest::new("", SeTab::Actions), &env, &mut ids).is_empty());
        assert_eq!(keys(&controller, SectionId::RecentActions), ["a:a.shown"]);
        assert!(!controller.is_loading());
        assert!(controller.run(&SeRequest::new("x", SeTab::Text), &env, &mut ids).is_empty());
        assert_eq!(controller.sections()[&SectionId::Text], SectionState::default());
        assert!(!controller.sections().contains_key(&SectionId::RecentActions));
    }
}
