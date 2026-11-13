//! Find Usages (⌥F7, the "usages" tool window) and Show Usages (⌥⌘F7, the popup at the caret).
//!
//! A search is a [`UsageFetch`]: it waits for the index to have the editors' edits, asks the
//! server for the references across the project (those found first stream in while it searches
//! on), falls back to syntax when the server is not ready or fails, and reads the lines of the
//! files the usages are in. Each step is a [`FetchRequest`] for the caller to carry out; answers
//! come back tagged, so that an answer to a request that is no longer current is dropped.
//!
//! [`UsagesTabs`] runs the tool window's searches, one tab each; [`UsagesPopupFlow`] runs the
//! popup's, which opens with the usages found first when the rest take a while, jumps to a single
//! usage, and searches Project and Libraries when invoked again.

use super::syntax_usages::{SyntaxAnswer, SyntaxRequest, SyntaxSearch, SyntaxStep};
use super::usages::{
    Classified, FileLines, ShowUsageResults, UsageGroup, UsageScope, group_usages, paths_to_read, searched_scope,
    usage_count,
};
use crate::editor_lsp::hint::Tone;
use crate::editor_lsp::lsp::{error_hint, is_cancelled, is_unavailable, result_note};
use crate::platform::i18n::I18n;
use ide_core::{DocId, IdeError, Location, Pos, RequestId};
use ide_lsp::types::References;
use std::collections::HashMap;

/// The caret a search starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageCaret {
    pub doc: DocId,
    pub path: String,
    pub pos: Pos,
    /// The identifier at the caret, for titles and notes.
    pub word: Option<String>,
    /// The document's language, for the syntax fallback.
    pub language: Option<String>,
}

/// Usages found, grouped by file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedUsages {
    pub groups: Vec<UsageGroup>,
    pub note: Option<String>,
    /// The dependencies and the JDK were not searched.
    pub libraries_skipped: bool,
}

/// What a search needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchRequest {
    /// Wait until the editors' edits of `path` reached the index, then answer whether the document is
    /// still the one searched from.
    SyncIndexes { path: String },
    /// `lsp_references(request, doc, pos, include_declaration, project_wide, libraries, on_partial)`.
    References {
        request: RequestId,
        doc: DocId,
        pos: Pos,
        include_declaration: bool,
        project_wide: bool,
        libraries: bool,
    },
    /// Read the lines of `paths` (an open editor's text, else the file on disk); answer with `read`.
    ReadLines { read: u64, paths: Vec<String> },
    /// The syntax fallback's next question.
    Syntax(SyntaxRequest),
}

/// What became of a search after an answer.
#[derive(Debug, Clone)]
pub enum FetchStep {
    Send(FetchRequest),
    /// The usages found so far; the search goes on.
    Partial(FetchedUsages),
    Done(FetchedUsages),
    Failed(IdeError),
    /// The answer changes nothing (a late one, or one for another search).
    Nothing,
}

#[derive(Debug, Clone)]
struct PendingRead {
    locations: Vec<Location>,
    classified: Vec<Classified>,
    note: Option<String>,
    libraries_skipped: bool,
    last: bool,
}

/// One usages search.
#[derive(Debug, Clone)]
pub struct UsageFetch {
    caret: Option<UsageCaret>,
    request: Option<RequestId>,
    libraries: bool,
    light_mode: bool,
    /// The server answered (or failed): partial results arriving later are dropped.
    settled: bool,
    reads: u64,
    pending: HashMap<u64, PendingRead>,
    server_error: Option<IdeError>,
    syntax: Option<SyntaxSearch>,
}

impl UsageFetch {
    /// A search from `caret` under backend request `request`; `libraries` also searches the
    /// dependencies and the JDK, `light_mode` is the IDE running without language servers.
    pub fn start(caret: UsageCaret, request: RequestId, libraries: bool, light_mode: bool) -> (Self, FetchRequest) {
        let first = FetchRequest::SyncIndexes { path: caret.path.clone() };
        let fetch = Self {
            caret: Some(caret),
            request: Some(request),
            libraries,
            light_mode,
            settled: false,
            reads: 0,
            pending: HashMap::new(),
            server_error: None,
            syntax: None,
        };
        (fetch, first)
    }

    /// Usages found elsewhere: only their lines are read.
    pub fn given(locations: Vec<Location>, classified: Vec<Classified>, note: Option<String>) -> (Self, FetchRequest) {
        let mut fetch = Self {
            caret: None,
            request: None,
            libraries: false,
            light_mode: false,
            settled: true,
            reads: 0,
            pending: HashMap::new(),
            server_error: None,
            syntax: None,
        };
        let first = fetch.read(PendingRead { locations, classified, note, libraries_skipped: false, last: true });
        (fetch, first)
    }

    pub fn request(&self) -> Option<RequestId> {
        self.request
    }

    fn read(&mut self, pending: PendingRead) -> FetchRequest {
        self.reads += 1;
        let paths = paths_to_read(&pending.locations, &pending.classified);
        self.pending.insert(self.reads, pending);
        FetchRequest::ReadLines { read: self.reads, paths }
    }

    fn note(&self, refs: &References, i18n: &mut I18n) -> Option<String> {
        let word = self.caret.as_ref().and_then(|caret| caret.word.as_deref());
        result_note(i18n, refs.incomplete, refs.approximate, word, self.light_mode)
    }

    fn found(&mut self, refs: &References, last: bool, i18n: &mut I18n) -> FetchStep {
        let note = self.note(refs, i18n);
        let pending = PendingRead {
            locations: refs.locations.clone(),
            classified: refs.classified.iter().map(Classified::from_backend).collect(),
            note,
            libraries_skipped: last && refs.libraries_skipped,
            last,
        };
        FetchStep::Send(self.read(pending))
    }

    /// The index has the edits; `unchanged`: the document is still the one searched from (else the
    /// search is given up, a newer one following).
    pub fn indexes_synced(&mut self, unchanged: bool) -> FetchStep {
        let (Some(caret), Some(request)) = (&self.caret, self.request) else { return FetchStep::Nothing };
        if !unchanged {
            return FetchStep::Failed(IdeError::cancelled());
        }
        FetchStep::Send(FetchRequest::References {
            request,
            doc: caret.doc,
            pos: caret.pos,
            include_declaration: false,
            project_wide: true,
            libraries: self.libraries,
        })
    }

    /// References the server found first, while it searches on.
    pub fn partial(&mut self, refs: &References, i18n: &mut I18n) -> FetchStep {
        if self.settled { FetchStep::Nothing } else { self.found(refs, false, i18n) }
    }

    /// The server's answer. An empty answer is respected; when the server is not ready or fails,
    /// the syntax fallback searches (without one the server's error stands).
    pub fn answered(&mut self, result: Result<References, IdeError>, i18n: &mut I18n) -> FetchStep {
        self.settled = true;
        match result {
            Ok(refs) => self.found(&refs, true, i18n),
            Err(error) if is_cancelled(&error) => FetchStep::Failed(error),
            Err(error) => {
                let Some(caret) = &self.caret else { return FetchStep::Failed(error) };
                let (search, step) = SyntaxSearch::start(
                    caret.doc,
                    &caret.path,
                    caret.pos,
                    caret.word.as_deref(),
                    caret.language.as_deref(),
                );
                self.syntax = Some(search);
                self.server_error = Some(error);
                self.syntax_step(step)
            }
        }
    }

    fn syntax_step(&mut self, step: SyntaxStep) -> FetchStep {
        match step {
            SyntaxStep::Ask(request) => FetchStep::Send(FetchRequest::Syntax(request)),
            SyntaxStep::Found(found) => {
                self.syntax = None;
                let pending = PendingRead {
                    locations: found.locations,
                    classified: Vec::new(),
                    note: Some(found.note),
                    libraries_skipped: false,
                    last: true,
                };
                FetchStep::Send(self.read(pending))
            }
            SyntaxStep::Unsupported => {
                self.syntax = None;
                FetchStep::Failed(self.server_error.take().unwrap_or_else(|| IdeError::internal("No usages found")))
            }
        }
    }

    /// The syntax service's answer to the last [`FetchRequest::Syntax`].
    pub fn syntax_answered(&mut self, answer: SyntaxAnswer) -> FetchStep {
        let Some(search) = self.syntax.as_mut() else { return FetchStep::Nothing };
        let step = search.answer(answer);
        self.syntax_step(step)
    }

    /// The syntax service failed: the server's error stands (a cancellation stays one).
    pub fn syntax_failed(&mut self, error: IdeError) -> FetchStep {
        if self.syntax.take().is_none() {
            return FetchStep::Nothing;
        }
        if is_cancelled(&error) {
            return FetchStep::Failed(error);
        }
        tracing::debug!("syntax usages fallback failed: {error}");
        FetchStep::Failed(self.server_error.take().unwrap_or(error))
    }

    /// The lines of read `read`.
    pub fn lines_read(&mut self, read: u64, files: &FileLines) -> FetchStep {
        let Some(pending) = self.pending.remove(&read) else { return FetchStep::Nothing };
        if !pending.last && self.settled {
            return FetchStep::Nothing;
        }
        let fetched = FetchedUsages {
            groups: group_usages(&pending.locations, files, &pending.classified),
            note: pending.note,
            libraries_skipped: pending.libraries_skipped,
        };
        if pending.last { FetchStep::Done(fetched) } else { FetchStep::Partial(fetched) }
    }
}

/// A hint at the caret (a notification without an editor), in English.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageHint {
    pub text: String,
    pub tone: Tone,
}

/// The hint an error shows; none for a cancellation.
pub fn error_usage_hint(error: &IdeError) -> Option<UsageHint> {
    let text = error_hint(error, true)?;
    Some(UsageHint { text, tone: if is_unavailable(error) { Tone::Info } else { Tone::Error } })
}

fn info(text: impl Into<String>) -> UsageHint {
    UsageHint { text: text.into(), tone: Tone::Info }
}

// ---------------------------------------------------------------------------
// The Find Usages tool window.

/// Most tabs kept; opening another closes the oldest.
pub const MAX_TABS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchSpec {
    caret: UsageCaret,
    libraries: bool,
    /// An earlier run of this search skipped the libraries.
    widenable: bool,
}

/// One tab: a search and its results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageSearch {
    pub id: u64,
    pub symbol: Option<String>,
    pub loading: bool,
    pub groups: Vec<UsageGroup>,
    pub error: Option<String>,
    /// What to say about the results (from syntax, may be incomplete).
    pub note: Option<String>,
    /// Where it searched, when the server can search beyond Project Files.
    pub scope: Option<UsageScope>,
    /// Kept when a new search runs (Pin Tab).
    pub pinned: bool,
    spec: Option<SearchSpec>,
}

impl UsageSearch {
    /// A tab of usages that cannot be searched again.
    pub fn shown(id: u64, symbol: Option<&str>) -> Self {
        Self {
            id,
            symbol: symbol.map(str::to_owned),
            loading: false,
            groups: Vec::new(),
            error: None,
            note: None,
            scope: None,
            pinned: false,
            spec: None,
        }
    }

    /// Rerun searches the same symbol again.
    pub fn can_rerun(&self) -> bool {
        self.spec.is_some()
    }

    /// "Search in Project and Libraries" is offered: it searched Project Files and can widen.
    pub fn offers_libraries(&self) -> bool {
        !self.loading && self.error.is_none() && self.scope == Some(UsageScope::Project) && self.spec.is_some()
    }
}

/// What the tool window needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TabsEffect {
    ShowToolWindow,
    Cancel(RequestId),
    /// A step of tab `search`'s search; answers go to the [`UsagesTabs`] method of the same name.
    Fetch {
        search: u64,
        request: FetchRequest,
    },
    Hint(UsageHint),
}

/// The tool window's tabs, one per search; a new search replaces the selected tab unless it is
/// pinned.
#[derive(Debug, Clone, Default)]
pub struct UsagesTabs {
    tabs: Vec<UsageSearch>,
    active: Option<u64>,
    ids: u64,
    inflight: HashMap<u64, UsageFetch>,
    light_mode: bool,
}

impl UsagesTabs {
    pub fn new(light_mode: bool) -> Self {
        Self { light_mode, ..Self::default() }
    }

    pub fn set_light_mode(&mut self, light_mode: bool) {
        self.light_mode = light_mode;
    }

    pub fn tabs(&self) -> &[UsageSearch] {
        &self.tabs
    }

    /// The selected tab's search (the last tab's when none is selected).
    pub fn active(&self) -> Option<&UsageSearch> {
        self.tabs.iter().find(|tab| Some(tab.id) == self.active).or_else(|| self.tabs.last())
    }

    pub fn select(&mut self, id: u64) {
        if self.tabs.iter().any(|tab| tab.id == id) {
            self.active = Some(id);
        }
    }

    fn next_id(&mut self) -> u64 {
        self.ids += 1;
        self.ids
    }

    /// Shows `search` in a new tab, or in place of the selected one unless that one is pinned.
    pub fn open_tab(&mut self, search: UsageSearch) {
        let id = search.id;
        match self.tabs.iter_mut().find(|tab| Some(tab.id) == self.active && !tab.pinned) {
            Some(reused) => *reused = search,
            None => self.tabs.push(search),
        }
        let excess = self.tabs.len().saturating_sub(MAX_TABS);
        self.tabs.drain(..excess);
        self.active = Some(id);
    }

    /// Replaces tab `id` (a rerun or a finished search), keeping its pin and the selection.
    fn replace_tab(&mut self, id: u64, search: UsageSearch) -> bool {
        let Some(old) = self.tabs.iter_mut().find(|tab| tab.id == id) else { return false };
        let new_id = search.id;
        *old = UsageSearch { pinned: old.pinned, ..search };
        if self.active == Some(id) {
            self.active = Some(new_id);
        }
        true
    }

    pub fn pin(&mut self, id: u64, pinned: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) {
            tab.pinned = pinned;
        }
    }

    fn cancel(&mut self, id: u64) -> Option<TabsEffect> {
        self.inflight.remove(&id)?.request().map(TabsEffect::Cancel)
    }

    /// Closes tab `id`, cancelling its search; the next tab (else the previous) is selected.
    pub fn close(&mut self, id: u64) -> Vec<TabsEffect> {
        let effects = self.cancel(id).into_iter().collect();
        let Some(at) = self.tabs.iter().position(|tab| tab.id == id) else { return effects };
        self.tabs.remove(at);
        if self.active == Some(id) {
            self.active = self.tabs.get(at.min(self.tabs.len().saturating_sub(1))).map(|tab| tab.id);
        }
        effects
    }

    /// The window switched projects: every tab and search goes.
    pub fn reset(&mut self) -> Vec<TabsEffect> {
        let ids: Vec<u64> = self.inflight.keys().copied().collect();
        let effects = ids.into_iter().filter_map(|id| self.cancel(id)).collect();
        self.tabs.clear();
        self.active = None;
        effects
    }

    /// Find Usages of the symbol at `caret` in Project Files, under backend request `request`.
    pub fn find(&mut self, caret: Option<UsageCaret>, request: RequestId) -> Vec<TabsEffect> {
        match caret {
            Some(caret) => self.run(None, SearchSpec { caret, libraries: false, widenable: false }, request),
            None => vec![TabsEffect::Hint(info("Open a file to find usages"))],
        }
    }

    /// Searches tab `id`'s symbol again, in the same scope (⌘R).
    pub fn rerun(&mut self, id: u64, request: RequestId) -> Vec<TabsEffect> {
        let Some(spec) = self.tabs.iter().find(|tab| tab.id == id).and_then(|tab| tab.spec.clone()) else {
            return Vec::new();
        };
        self.run(Some(id), spec, request)
    }

    /// Reruns the selected tab.
    pub fn rerun_active(&mut self, request: RequestId) -> Vec<TabsEffect> {
        match self.active().map(|tab| tab.id) {
            Some(id) => self.rerun(id, request),
            None => Vec::new(),
        }
    }

    /// Searches tab `id`'s symbol again in Project and Libraries.
    pub fn search_libraries(&mut self, id: u64, request: RequestId) -> Vec<TabsEffect> {
        let Some(spec) = self.tabs.iter().find(|tab| tab.id == id).and_then(|tab| tab.spec.clone()) else {
            return Vec::new();
        };
        self.run(Some(id), SearchSpec { libraries: true, widenable: true, ..spec }, request)
    }

    fn run(&mut self, previous: Option<u64>, spec: SearchSpec, request: RequestId) -> Vec<TabsEffect> {
        let id = self.next_id();
        let mut effects: Vec<TabsEffect> = previous.and_then(|previous| self.cancel(previous)).into_iter().collect();
        let loading = UsageSearch {
            loading: true,
            spec: Some(spec.clone()),
            ..UsageSearch::shown(id, spec.caret.word.as_deref())
        };
        if !previous.is_some_and(|previous| self.replace_tab(previous, loading.clone())) {
            self.open_tab(loading);
        }
        effects.push(TabsEffect::ShowToolWindow);
        let (fetch, first) = UsageFetch::start(spec.caret, request, spec.libraries, self.light_mode);
        self.inflight.insert(id, fetch);
        effects.push(TabsEffect::Fetch { search: id, request: first });
        effects
    }

    /// Usages found elsewhere (Safe Delete's View Usages) in a tab of their own.
    pub fn show_results(&mut self, args: ShowUsageResults) -> Vec<TabsEffect> {
        let id = self.next_id();
        self.open_tab(UsageSearch { loading: true, ..UsageSearch::shown(id, args.symbol.as_deref()) });
        let (fetch, first) = UsageFetch::given(args.locations, args.classified, args.note);
        self.inflight.insert(id, fetch);
        vec![TabsEffect::ShowToolWindow, TabsEffect::Fetch { search: id, request: first }]
    }

    fn step(&mut self, search: u64, step: impl FnOnce(&mut UsageFetch) -> FetchStep) -> Vec<TabsEffect> {
        let Some(fetch) = self.inflight.get_mut(&search) else { return Vec::new() };
        let step = step(fetch);
        self.apply(search, step)
    }

    fn apply(&mut self, id: u64, step: FetchStep) -> Vec<TabsEffect> {
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == id) else {
            self.inflight.remove(&id);
            return Vec::new();
        };
        match step {
            FetchStep::Send(request) => vec![TabsEffect::Fetch { search: id, request }],
            FetchStep::Nothing => Vec::new(),
            FetchStep::Partial(found) => {
                (tab.groups, tab.note) = (found.groups, found.note);
                Vec::new()
            }
            FetchStep::Done(found) => {
                self.inflight.remove(&id);
                tab.scope = tab
                    .spec
                    .as_ref()
                    .and_then(|spec| searched_scope(spec.libraries, found.libraries_skipped, spec.widenable));
                (tab.loading, tab.groups, tab.note, tab.error) = (false, found.groups, found.note, None);
                Vec::new()
            }
            FetchStep::Failed(error) => {
                self.inflight.remove(&id);
                (tab.loading, tab.groups, tab.note) = (false, Vec::new(), None);
                if is_cancelled(&error) {
                    return Vec::new();
                }
                tab.error = Some(error.message.clone());
                error_usage_hint(&error).map(TabsEffect::Hint).into_iter().collect()
            }
        }
    }

    pub fn indexes_synced(&mut self, search: u64, unchanged: bool) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.indexes_synced(unchanged))
    }

    pub fn partial(&mut self, search: u64, refs: &References, i18n: &mut I18n) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.partial(refs, i18n))
    }

    pub fn answered(&mut self, search: u64, result: Result<References, IdeError>, i18n: &mut I18n) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.answered(result, i18n))
    }

    pub fn syntax_answered(&mut self, search: u64, answer: SyntaxAnswer) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.syntax_answered(answer))
    }

    pub fn syntax_failed(&mut self, search: u64, error: IdeError) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.syntax_failed(error))
    }

    pub fn lines_read(&mut self, search: u64, read: u64, files: &FileLines) -> Vec<TabsEffect> {
        self.step(search, |fetch| fetch.lines_read(read, files))
    }
}

// ---------------------------------------------------------------------------
// The Show Usages popup.

/// How long Show Usages waits for all usages before it opens with those found first.
pub const EARLY_POPUP_DELAY_MS: u64 = 300;
/// How long the hint after Show Usages stays (invoking it again meanwhile widens its search).
pub const HINT_MS: u64 = 4000;

/// What the popup shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupContent {
    pub open: bool,
    pub symbol: Option<String>,
    pub groups: Vec<UsageGroup>,
    pub note: Option<String>,
    /// More usages may follow: those found so far are shown.
    pub loading: bool,
    /// Where it searched, when the server can search beyond Project Files (the scope chooser).
    pub scope: Option<UsageScope>,
}

/// What the popup needs carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupEffect {
    /// The popup opens below the caret.
    Open,
    Fetch(FetchRequest),
    Cancel(RequestId),
    /// Start a timer of `ms`; when it elapses, call [`UsagesPopupFlow::early_timer`] with `ticket`.
    StartTimer {
        ticket: u64,
        ms: u64,
    },
    /// Open the only usage in its editor.
    OpenLocation {
        path: String,
        line: u32,
        col: u32,
    },
    Hint(UsageHint),
}

#[derive(Debug, Clone)]
struct Target {
    caret: UsageCaret,
    widenable: bool,
    /// Until when invoking Show Usages again widens the search (the hint offering it shows).
    hint_until: u64,
}

#[derive(Debug, Clone)]
struct PopupSearch {
    request: RequestId,
    fetch: UsageFetch,
    caret: UsageCaret,
    libraries: bool,
    widenable: bool,
    /// The popup was open when this search started (a scope change, a widening).
    reopened: bool,
    /// The popup opened with the usages found first.
    early: bool,
    found: Option<FetchedUsages>,
    timer: Option<u64>,
}

/// The Show Usages popup's searches and what it shows.
#[derive(Debug, Clone, Default)]
pub struct UsagesPopupFlow {
    content: PopupContent,
    target: Option<Target>,
    search: Option<PopupSearch>,
    tickets: u64,
    light_mode: bool,
}

impl UsagesPopupFlow {
    pub fn new(light_mode: bool) -> Self {
        Self { light_mode, ..Self::default() }
    }

    pub fn set_light_mode(&mut self, light_mode: bool) {
        self.light_mode = light_mode;
    }

    pub fn content(&self) -> &PopupContent {
        &self.content
    }

    /// Show Usages at `caret` (`now`: milliseconds on any monotonic clock). Invoked again while the
    /// popup shows Project Files, or while the hint offering it shows, it searches the same symbol in
    /// Project and Libraries.
    pub fn show(&mut self, caret: Option<UsageCaret>, now: u64, request: RequestId) -> Vec<PopupEffect> {
        if let Some(target) = &self.target {
            let showing_project = self.content.open && self.content.scope == Some(UsageScope::Project);
            if target.widenable && (showing_project || now < target.hint_until) {
                let caret = target.caret.clone();
                return self.search(caret, true, true, request);
            }
        }
        match caret {
            Some(caret) => self.search(caret, false, false, request),
            None => vec![PopupEffect::Hint(info("Open a file to show usages"))],
        }
    }

    /// The scope chooser: the symbol searched again in `scope`.
    pub fn set_scope(&mut self, scope: UsageScope, request: RequestId) -> Vec<PopupEffect> {
        let Some(target) = &self.target else { return Vec::new() };
        if Some(scope) == self.content.scope {
            return Vec::new();
        }
        let (caret, widenable) = (target.caret.clone(), target.widenable);
        self.search(caret, scope == UsageScope::Libraries, widenable, request)
    }

    /// The popup closed: its search is cancelled.
    pub fn dismiss(&mut self) -> Vec<PopupEffect> {
        self.content.open = false;
        self.search.take().map(|search| PopupEffect::Cancel(search.request)).into_iter().collect()
    }

    /// The window switched projects.
    pub fn reset(&mut self) -> Vec<PopupEffect> {
        let effects = self.dismiss();
        self.content = PopupContent::default();
        self.target = None;
        effects
    }

    fn search(&mut self, caret: UsageCaret, libraries: bool, widenable: bool, request: RequestId) -> Vec<PopupEffect> {
        // Set from the start: the scope chooser can switch while this search goes on.
        self.target = Some(Target { caret: caret.clone(), widenable, hint_until: 0 });
        let reopened = self.content.open;
        if reopened {
            self.content.loading = true;
            self.content.scope = Some(if libraries { UsageScope::Libraries } else { UsageScope::Project });
        }
        let mut effects: Vec<PopupEffect> =
            self.search.take().map(|search| PopupEffect::Cancel(search.request)).into_iter().collect();
        let (fetch, first) = UsageFetch::start(caret.clone(), request, libraries, self.light_mode);
        self.search = Some(PopupSearch {
            request,
            fetch,
            caret,
            libraries,
            widenable,
            reopened,
            early: false,
            found: None,
            timer: None,
        });
        effects.push(PopupEffect::Fetch(first));
        effects
    }

    fn current(&mut self, request: RequestId) -> Option<&mut PopupSearch> {
        self.search.as_mut().filter(|search| search.request == request)
    }

    /// Opens the popup with the usages found so far.
    fn open_early(&mut self) -> Vec<PopupEffect> {
        let Some(search) = self.search.as_mut() else { return Vec::new() };
        let Some(found) = search.found.clone() else { return Vec::new() };
        let mut effects = Vec::new();
        if !search.early && !search.reopened {
            self.content.open = true;
            self.content.symbol = search.caret.word.clone();
            effects.push(PopupEffect::Open);
        }
        search.early = true;
        self.content.groups = found.groups;
        self.content.note = found.note;
        self.content.loading = true;
        self.content.scope = search.widenable.then_some(UsageScope::Libraries);
        effects
    }

    /// The early-open timer of `ticket` elapsed.
    pub fn early_timer(&mut self, ticket: u64) -> Vec<PopupEffect> {
        if self.search.as_ref().and_then(|search| search.timer) != Some(ticket) {
            return Vec::new();
        }
        self.open_early()
    }

    fn apply(&mut self, step: FetchStep, now: u64, again: Option<&str>) -> Vec<PopupEffect> {
        match step {
            FetchStep::Send(request) => vec![PopupEffect::Fetch(request)],
            FetchStep::Nothing => Vec::new(),
            FetchStep::Partial(found) => {
                let Some(search) = self.search.as_mut() else { return Vec::new() };
                if usage_count(&found.groups) == 0 {
                    return Vec::new();
                }
                search.found = Some(found);
                if search.early {
                    return self.open_early();
                }
                if search.timer.is_some() {
                    return Vec::new();
                }
                self.tickets += 1;
                search.timer = Some(self.tickets);
                vec![PopupEffect::StartTimer { ticket: self.tickets, ms: EARLY_POPUP_DELAY_MS }]
            }
            FetchStep::Done(found) => match self.search.take() {
                Some(search) => self.finished(search, found, now, again),
                None => Vec::new(),
            },
            FetchStep::Failed(error) => {
                let Some(search) = self.search.take() else { return Vec::new() };
                if is_cancelled(&error) {
                    return Vec::new();
                }
                if search.early || search.reopened {
                    self.content.loading = false;
                }
                error_usage_hint(&error).map(PopupEffect::Hint).into_iter().collect()
            }
        }
    }

    fn finished(
        &mut self,
        search: PopupSearch,
        found: FetchedUsages,
        now: u64,
        again: Option<&str>,
    ) -> Vec<PopupEffect> {
        let scope = searched_scope(search.libraries, found.libraries_skipped, search.widenable);
        let mut target =
            Target { caret: search.caret.clone(), widenable: search.widenable || scope.is_some(), hint_until: 0 };
        let total = usage_count(&found.groups);
        let mut effects = Vec::new();
        if search.early || search.reopened || total > 1 {
            if !search.early && !search.reopened {
                self.content.open = true;
                self.content.symbol = search.caret.word.clone();
                effects.push(PopupEffect::Open);
            }
            self.content.groups = found.groups;
            self.content.note = found.note;
            self.content.loading = false;
            self.content.scope = scope;
            self.target = Some(target);
            return effects;
        }
        // As in IDEA, the hint offers to widen the search, which Show Usages does while it shows.
        let again = again.filter(|keys| !keys.is_empty());
        let widen = scope == Some(UsageScope::Project) && again.is_some();
        if widen {
            target.hint_until = now + HINT_MS;
        }
        let keys = again.unwrap_or_default();
        let hint = match found.groups.first().and_then(|group| group.usages.first()) {
            None if widen => {
                format!("No usages found in Project Files. Press {keys} again to search in 'Project and Libraries'")
            }
            None if scope == Some(UsageScope::Project) => "No usages found in Project Files".to_owned(),
            None => {
                let none = match &search.caret.word {
                    Some(word) => format!("No usages of '{word}' found"),
                    None => "No usages found".to_owned(),
                };
                match &found.note {
                    Some(note) => format!("{none}. {note}"),
                    None => none,
                }
            }
            // IDEA jumps to the only usage and says so in the target editor.
            Some(usage) => {
                effects.push(PopupEffect::OpenLocation { path: usage.path.clone(), line: usage.line, col: usage.col });
                if widen {
                    format!("Only one usage in Project Files. Press {keys} again to search in 'Project and Libraries'")
                } else if scope == Some(UsageScope::Libraries) {
                    "Only one usage in Project and Libraries".to_owned()
                } else {
                    "Only one usage in Project Files".to_owned()
                }
            }
        };
        self.target = Some(target);
        effects.push(PopupEffect::Hint(info(hint)));
        effects
    }

    fn step(
        &mut self,
        request: RequestId,
        now: u64,
        again: Option<&str>,
        step: impl FnOnce(&mut UsageFetch) -> FetchStep,
    ) -> Vec<PopupEffect> {
        let Some(search) = self.current(request) else { return Vec::new() };
        let step = step(&mut search.fetch);
        self.apply(step, now, again)
    }

    pub fn indexes_synced(&mut self, request: RequestId, unchanged: bool) -> Vec<PopupEffect> {
        self.step(request, 0, None, |fetch| fetch.indexes_synced(unchanged))
    }

    pub fn partial(&mut self, request: RequestId, refs: &References, i18n: &mut I18n) -> Vec<PopupEffect> {
        self.step(request, 0, None, |fetch| fetch.partial(refs, i18n))
    }

    pub fn answered(
        &mut self,
        request: RequestId,
        result: Result<References, IdeError>,
        i18n: &mut I18n,
    ) -> Vec<PopupEffect> {
        self.step(request, 0, None, |fetch| fetch.answered(result, i18n))
    }

    pub fn syntax_answered(&mut self, request: RequestId, answer: SyntaxAnswer) -> Vec<PopupEffect> {
        self.step(request, 0, None, |fetch| fetch.syntax_answered(answer))
    }

    pub fn syntax_failed(&mut self, request: RequestId, error: IdeError) -> Vec<PopupEffect> {
        self.step(request, 0, None, |fetch| fetch.syntax_failed(error))
    }

    /// The lines of read `read`; a search that ends with them decides at `now` how long its hint
    /// offers widening, `again` being the shortcut the hint names.
    pub fn lines_read(
        &mut self,
        request: RequestId,
        read: u64,
        files: &FileLines,
        now: u64,
        again: Option<&str>,
    ) -> Vec<PopupEffect> {
        self.step(request, now, again, |fetch| fetch.lines_read(read, files))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_insight::syntax_usages::{LOCAL_NOTE, TEXT_NOTE};
    use crate::editor_lsp::lsp::incomplete_note;
    use ide_core::error::ErrorKind;
    use ide_core::{IpcPath, Range};
    use ide_lsp::types::ReferencesIncomplete;
    use ide_syntax::StructureSupport;
    use ide_syntax::{FileIdentifiers, IdentifierOccurrence, LocalKind, LocalSymbol, Locals, NotLocalReason};
    use std::path::PathBuf;

    const SHORTCUT: &str = "⌥⌘F7";

    fn r(line: u32, col: u32, end: u32) -> Range {
        Range::new(Pos::new(line, col), Pos::new(line, end))
    }

    fn at(path: &str, line: u32) -> Location {
        Location { path: IpcPath::from(PathBuf::from(path)), range: r(line, 4, 8), uri: None }
    }

    fn refs(locations: Vec<Location>) -> References {
        References { locations, ..References::default() }
    }

    fn skipped(locations: Vec<Location>) -> References {
        References { libraries_skipped: true, ..refs(locations) }
    }

    fn caret() -> UsageCaret {
        UsageCaret {
            doc: DocId(1),
            path: "/p/a.rs".to_owned(),
            pos: Pos::new(0, 0),
            word: Some("used".to_owned()),
            language: Some("rust".to_owned()),
        }
    }

    fn b_lines() -> FileLines {
        let lines = ["", "    used();", "    used();", "    used();", "    used();"];
        FileLines::from([("/p/b.rs".to_owned(), Some(lines.iter().map(|line| (*line).to_owned()).collect()))])
    }

    fn read_in<'a>(mut requests: impl Iterator<Item = &'a FetchRequest>) -> Option<u64> {
        requests.find_map(|request| match request {
            FetchRequest::ReadLines { read, .. } => Some(*read),
            _ => None,
        })
    }

    fn tab_read(effects: &[TabsEffect]) -> Option<u64> {
        read_in(effects.iter().filter_map(|effect| match effect {
            TabsEffect::Fetch { request, .. } => Some(request),
            _ => None,
        }))
    }

    fn popup_read(effects: &[PopupEffect]) -> Option<u64> {
        read_in(effects.iter().filter_map(|effect| match effect {
            PopupEffect::Fetch(request) => Some(request),
            _ => None,
        }))
    }

    fn lines_of(groups: &[UsageGroup]) -> Vec<u32> {
        groups.iter().flat_map(|group| group.usages.iter().map(|usage| usage.line)).collect()
    }

    fn libraries_of(effects: &[TabsEffect]) -> Option<bool> {
        effects.iter().find_map(|effect| match effect {
            TabsEffect::Fetch { request: FetchRequest::References { libraries, .. }, .. } => Some(*libraries),
            _ => None,
        })
    }

    /// Runs tab search `id` from its index barrier to the server's answer and the lines read.
    fn complete_tab(tabs: &mut UsagesTabs, id: u64, answer: References, i18n: &mut I18n) -> Option<bool> {
        let libraries = libraries_of(&tabs.indexes_synced(id, true));
        let effects = tabs.answered(id, Ok(answer), i18n);
        let read = tab_read(&effects).expect("the lines are read");
        tabs.lines_read(id, read, &b_lines());
        libraries
    }

    fn search_id(effects: &[TabsEffect]) -> u64 {
        effects
            .iter()
            .find_map(|effect| match effect {
                TabsEffect::Fetch { search, .. } => Some(*search),
                _ => None,
            })
            .expect("a search starts")
    }

    #[test]
    fn shows_given_usages_in_the_find_usages_tool_window() {
        let mut tabs = UsagesTabs::default();
        let args = ShowUsageResults {
            symbol: Some("used".to_owned()),
            locations: vec![at("/p/b.rs", 1)],
            classified: Vec::new(),
            note: None,
        };
        let effects = tabs.show_results(args);
        assert_eq!(effects[0], TabsEffect::ShowToolWindow);
        let id = search_id(&effects);
        assert!(tabs.active().is_some_and(|tab| tab.loading));
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &b_lines());
        let search = tabs.active().expect("a tab");
        assert_eq!((search.symbol.as_deref(), search.loading, &search.error), (Some("used"), false, &None));
        assert!(!search.can_rerun());
        let usage = &search.groups[0].usages[0];
        assert_eq!(
            (usage.path.as_str(), usage.line, usage.col, usage.end_col, usage.text.as_str()),
            ("/p/b.rs", 1, 4, 8, "    used();")
        );
    }

    #[test]
    fn searches_project_files_then_project_and_libraries_on_request() {
        let mut i18n = I18n::default();
        let mut tabs = UsagesTabs::default();
        let id = search_id(&tabs.find(Some(caret()), RequestId(1)));
        assert_eq!(complete_tab(&mut tabs, id, skipped(vec![at("/p/b.rs", 1)]), &mut i18n), Some(false));
        let project = tabs.active().expect("a tab").clone();
        assert_eq!((project.loading, project.scope), (false, Some(UsageScope::Project)));
        assert!(project.offers_libraries());

        let effects = tabs.search_libraries(project.id, RequestId(2));
        let id = search_id(&effects);
        assert_eq!(complete_tab(&mut tabs, id, refs(vec![at("/p/b.rs", 1)]), &mut i18n), Some(true));
        let all = tabs.active().expect("a tab").clone();
        assert_eq!((all.loading, all.scope), (false, Some(UsageScope::Libraries)));
        assert_eq!(tabs.tabs().len(), 1);

        // Rerun keeps the scope.
        let id = search_id(&tabs.rerun(all.id, RequestId(3)));
        assert_eq!(libraries_of(&tabs.indexes_synced(id, true)), Some(true));
    }

    #[test]
    fn shows_the_usages_found_first_while_it_searches_for_the_rest() {
        let mut i18n = I18n::default();
        let mut tabs = UsagesTabs::default();
        let id = search_id(&tabs.find(Some(caret()), RequestId(1)));
        tabs.indexes_synced(id, true);
        let effects = tabs.partial(id, &refs(vec![at("/p/b.rs", 1)]), &mut i18n);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &b_lines());
        let partial = tabs.active().expect("a tab");
        assert_eq!((partial.groups.len(), partial.loading, &partial.error), (1, true, &None));
        let effects = tabs.answered(id, Ok(refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)])), &mut i18n);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &b_lines());
        let done = tabs.active().expect("a tab");
        assert!(!done.loading);
        assert_eq!(lines_of(&done.groups), [1, 2]);
    }

    #[test]
    fn waits_for_the_contributed_index_barrier_then_preserves_partial_and_final_usage_events() {
        let mut i18n = I18n::default();
        let mut tabs = UsagesTabs::default();
        let effects = tabs.find(Some(caret()), RequestId(1));
        let id = search_id(&effects);
        // Nothing is asked of the server before the index has the edits.
        assert!(effects.contains(&TabsEffect::Fetch {
            search: id,
            request: FetchRequest::SyncIndexes { path: "/p/a.rs".to_owned() }
        }));
        assert_eq!(libraries_of(&effects), None);
        assert_eq!(libraries_of(&tabs.indexes_synced(id, true)), Some(false));
        let effects = tabs.partial(id, &refs(vec![at("/p/b.rs", 1)]), &mut i18n);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &b_lines());
        assert!(tabs.active().is_some_and(|tab| tab.loading && tab.groups.len() == 1));
        let effects = tabs.answered(id, Ok(refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)])), &mut i18n);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &b_lines());
        assert_eq!(lines_of(&tabs.active().expect("a tab").groups), [1, 2]);
    }

    #[test]
    fn replace_the_selected_tab_unless_it_is_pinned() {
        let mut tabs = UsagesTabs::default();
        let ids = |tabs: &UsagesTabs| tabs.tabs().iter().map(|tab| tab.id).collect::<Vec<_>>();
        tabs.open_tab(UsageSearch::shown(101, Some("s101")));
        tabs.open_tab(UsageSearch::shown(102, Some("s102")));
        assert_eq!(ids(&tabs), [102]);
        tabs.pin(102, true);
        tabs.open_tab(UsageSearch::shown(103, Some("s103")));
        assert_eq!(ids(&tabs), [102, 103]);
        assert_eq!(tabs.active().map(|tab| tab.id), Some(103));
        tabs.open_tab(UsageSearch::shown(104, Some("s104")));
        assert_eq!(ids(&tabs), [102, 104]);
    }

    // The syntax fallback, through a tool window search.

    /// A search whose server answered `result`; returns the tabs, the search and the effects.
    fn answered(result: Result<References, IdeError>) -> (UsagesTabs, u64, Vec<TabsEffect>) {
        let mut tabs = UsagesTabs::default();
        let id = search_id(&tabs.find(Some(caret()), RequestId(42)));
        tabs.indexes_synced(id, true);
        let effects = tabs.answered(id, result, &mut I18n::default());
        (tabs, id, effects)
    }

    fn syntax_request(effects: &[TabsEffect]) -> Option<SyntaxRequest> {
        effects.iter().find_map(|effect| match effect {
            TabsEffect::Fetch { request: FetchRequest::Syntax(request), .. } => Some(request.clone()),
            _ => None,
        })
    }

    fn a_lines() -> FileLines {
        FileLines::from([("/p/a.rs".to_owned(), Some(vec!["let x = 1;".to_owned(), "x + y;".to_owned()]))])
    }

    #[test]
    fn uses_the_server_when_it_answers_even_with_nothing() {
        let (mut tabs, id, effects) = answered(Ok(refs(Vec::new())));
        assert_eq!(syntax_request(&effects), None);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &FileLines::new());
        let tab = tabs.active().expect("a tab");
        assert_eq!((tab.groups.is_empty(), &tab.note, tab.loading), (true, &None, false));
    }

    #[test]
    fn keeps_indexed_results_and_names_a_failed_language_server_query_accurately() {
        let incomplete = Some(ReferencesIncomplete::ServerFailed);
        let answer = References { incomplete, ..refs(vec![at("/p/a.rs", 1)]) };
        let (mut tabs, id, effects) = answered(Ok(answer));
        assert_eq!(syntax_request(&effects), None);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &a_lines());
        let tab = tabs.active().expect("a tab");
        assert_eq!(tab.groups[0].usages.len(), 1);
        assert_eq!(tab.note.as_deref(), Some("May be incomplete: the language server could not check this name"));
    }

    #[test]
    fn searches_project_wide_under_the_given_request_and_says_when_results_may_be_incomplete() {
        let mut i18n = I18n::default();
        let mut tabs = UsagesTabs::default();
        let id = search_id(&tabs.find(Some(caret()), RequestId(42)));
        let sent = tabs.indexes_synced(id, true);
        assert_eq!(
            sent,
            [TabsEffect::Fetch {
                search: id,
                request: FetchRequest::References {
                    request: RequestId(42),
                    doc: DocId(1),
                    pos: Pos::new(0, 0),
                    include_declaration: false,
                    project_wide: true,
                    libraries: false,
                },
            }]
        );
        let answer = References { incomplete: Some(ReferencesIncomplete::Indexing), ..refs(vec![at("/p/a.rs", 1)]) };
        let effects = tabs.answered(id, Ok(answer), &mut i18n);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &a_lines());
        let expected = incomplete_note(&mut i18n, ReferencesIncomplete::Indexing, Some("used"));
        assert_eq!(tabs.active().and_then(|tab| tab.note.clone()), Some(expected));
        assert_eq!(
            incomplete_note(&mut i18n, ReferencesIncomplete::TooManyFiles, Some("x")),
            "May be incomplete: too many files mention 'x' to search them all"
        );
        assert_eq!(
            incomplete_note(&mut i18n, ReferencesIncomplete::FilesSkipped, Some("x")),
            "May be incomplete: some files that mention 'x' are too large or unreadable"
        );
    }

    #[test]
    fn lists_a_certain_locals_references_when_the_server_is_not_ready() {
        let unavailable = IdeError::new(ErrorKind::Unavailable, "No language server available");
        let (mut tabs, id, effects) = answered(Err(unavailable));
        assert_eq!(syntax_request(&effects), Some(SyntaxRequest::Support { language: "rust".to_owned() }));
        let support = StructureSupport { tree: true, locals: true };
        let effects = tabs.syntax_answered(id, SyntaxAnswer::Support(support));
        assert_eq!(syntax_request(&effects), Some(SyntaxRequest::Locals { doc: DocId(1), pos: Pos::new(0, 0) }));
        let symbol = LocalSymbol {
            name: "x".to_owned(),
            kind: LocalKind::Var,
            definition: r(0, 4, 5),
            scope: r(0, 0, 20),
            references: vec![r(1, 0, 1)],
            certain: true,
            conflicts: Vec::new(),
        };
        let effects = tabs.syntax_answered(id, SyntaxAnswer::Locals(Locals::Local { symbol }));
        // No project search for a certain local.
        assert_eq!(syntax_request(&effects), None);
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &a_lines());
        let tab = tabs.active().expect("a tab");
        assert_eq!(tab.note.as_deref(), Some(LOCAL_NOTE));
        let usage = &tab.groups[0].usages[0];
        assert_eq!((usage.path.as_str(), usage.line, usage.col, usage.text.as_str()), ("/p/a.rs", 1, 0, "x + y;"));
    }

    #[test]
    fn searches_the_project_for_identifier_occurrences_of_non_locals() {
        let (mut tabs, id, _) = answered(Err(IdeError::internal("internal")));
        tabs.syntax_answered(id, SyntaxAnswer::Support(StructureSupport { tree: true, locals: true }));
        let reason = NotLocalReason::Unresolved;
        let not_local = Locals::NotLocal { name: "y".to_owned(), range: r(1, 4, 5), reason };
        let effects = tabs.syntax_answered(id, SyntaxAnswer::Locals(not_local));
        assert_eq!(syntax_request(&effects), Some(SyntaxRequest::FilesMentioning { name: "y".to_owned() }));
        let mentioning = SyntaxAnswer::FilesMentioning { paths: vec!["/p/b.rs".to_owned()], limit_hit: false };
        let effects = tabs.syntax_answered(id, mentioning);
        assert_eq!(
            syntax_request(&effects),
            Some(SyntaxRequest::IdentifiersInFiles {
                paths: vec!["/p/b.rs".to_owned(), "/p/a.rs".to_owned()],
                name: "y".to_owned()
            })
        );
        let occurrence = |range: Range| IdentifierOccurrence { range, kind: "identifier".to_owned(), qualified: false };
        let file = |path: &str, range: Range| FileIdentifiers {
            path: IpcPath::from(PathBuf::from(path)),
            occurrences: vec![occurrence(range)],
        };
        let files = vec![file("/p/b.rs", r(2, 0, 1)), file("/p/a.rs", r(1, 4, 5))];
        let effects = tabs.syntax_answered(id, SyntaxAnswer::IdentifiersInFiles(files));
        tabs.lines_read(id, tab_read(&effects).expect("the lines are read"), &a_lines());
        let tab = tabs.active().expect("a tab");
        assert_eq!(tab.note.as_deref(), Some(TEXT_NOTE));
        assert_eq!(tab.groups.iter().map(|group| group.path.as_str()).collect::<Vec<_>>(), ["/p/a.rs", "/p/b.rs"]);
    }

    #[test]
    fn keeps_the_server_error_when_the_language_has_no_grammar() {
        let unavailable = IdeError::new(ErrorKind::Unavailable, "No language server available");
        let (mut tabs, id, _) = answered(Err(unavailable));
        let effects = tabs.syntax_answered(id, SyntaxAnswer::Support(StructureSupport { tree: false, locals: false }));
        assert_eq!(
            effects,
            [TabsEffect::Hint(UsageHint { text: "No language server available".to_owned(), tone: Tone::Info })]
        );
        assert_eq!(tabs.active().and_then(|tab| tab.error.clone()).as_deref(), Some("No language server available"));
    }

    // The Show Usages popup.

    struct Popup {
        flow: UsagesPopupFlow,
        i18n: I18n,
        now: u64,
        requests: u32,
    }

    impl Popup {
        fn new() -> Self {
            Self { flow: UsagesPopupFlow::default(), i18n: I18n::default(), now: 0, requests: 0 }
        }

        /// Invokes Show Usages; returns its request and effects.
        fn show(&mut self) -> (RequestId, Vec<PopupEffect>) {
            self.requests += 1;
            let request = RequestId(self.requests);
            let effects = self.flow.show(Some(caret()), self.now, request);
            (request, effects)
        }

        fn scope(&mut self, scope: UsageScope) -> (RequestId, Vec<PopupEffect>) {
            self.requests += 1;
            let request = RequestId(self.requests);
            (request, self.flow.set_scope(scope, request))
        }

        /// Whether `request` searches Project and Libraries; the index barrier passes.
        fn libraries(&mut self, request: RequestId) -> Option<bool> {
            self.flow.indexes_synced(request, true).iter().find_map(|effect| match effect {
                PopupEffect::Fetch(FetchRequest::References { libraries, .. }) => Some(*libraries),
                _ => None,
            })
        }

        fn partial(&mut self, request: RequestId, lines: &[u32]) -> Vec<PopupEffect> {
            let found = refs(lines.iter().map(|line| at("/p/b.rs", *line)).collect());
            let effects = self.flow.partial(request, &found, &mut self.i18n);
            match popup_read(&effects) {
                Some(read) => self.flow.lines_read(request, read, &b_lines(), self.now, Some(SHORTCUT)),
                None => effects,
            }
        }

        fn answer(&mut self, request: RequestId, answer: References) -> Vec<PopupEffect> {
            let effects = self.flow.answered(request, Ok(answer), &mut self.i18n);
            match popup_read(&effects) {
                Some(read) => self.flow.lines_read(request, read, &b_lines(), self.now, Some(SHORTCUT)),
                None => effects,
            }
        }

        /// Show Usages answered at once with `answer`.
        fn quick(&mut self, answer: References) -> (bool, Vec<PopupEffect>) {
            let (request, _) = self.show();
            let libraries = self.libraries(request).expect("the server is asked");
            (libraries, self.answer(request, answer))
        }

        fn lines(&self) -> Vec<u32> {
            lines_of(&self.flow.content().groups)
        }
    }

    fn hint(text: &str) -> PopupEffect {
        PopupEffect::Hint(UsageHint { text: text.to_owned(), tone: Tone::Info })
    }

    fn open_b(line: u32) -> PopupEffect {
        PopupEffect::OpenLocation { path: "/p/b.rs".to_owned(), line, col: 4 }
    }

    const WIDEN: &str = "Press ⌥⌘F7 again to search in 'Project and Libraries'";

    #[test]
    fn a_single_usage_is_opened_with_a_hint_in_its_editor() {
        let mut popup = Popup::new();
        let (_, effects) = popup.quick(refs(vec![at("/p/b.rs", 1)]));
        assert_eq!(effects, [open_b(1), hint("Only one usage in Project Files")]);
        assert!(!popup.flow.content().open);
    }

    #[test]
    fn does_not_reopen_after_dismissal_or_let_a_superseded_response_replace_a_newer_popup() {
        let mut popup = Popup::new();
        let (old, _) = popup.show();
        popup.libraries(old);
        let (fresh, effects) = popup.show();
        assert!(effects.contains(&PopupEffect::Cancel(old)));
        popup.libraries(fresh);
        popup.answer(fresh, refs(vec![at("/p/b.rs", 1)]));
        popup.flow.dismiss();
        assert!(popup.answer(old, refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)])).is_empty());
        assert!(!popup.flow.content().open);
    }

    #[test]
    fn opens_with_the_usages_found_first_and_fills_in_the_rest() {
        let mut popup = Popup::new();
        let (request, _) = popup.show();
        popup.libraries(request);
        let effects = popup.partial(request, &[1]);
        // Not before the rest had a moment to arrive.
        assert!(!popup.flow.content().open);
        let [PopupEffect::StartTimer { ticket, ms: EARLY_POPUP_DELAY_MS }] = effects.as_slice() else {
            panic!("a timer starts: {effects:?}");
        };
        assert_eq!(popup.flow.early_timer(*ticket), [PopupEffect::Open]);
        let content = popup.flow.content();
        assert!(content.open && content.loading);
        assert_eq!(popup.lines(), [1]);
        assert!(popup.partial(request, &[1, 2]).is_empty());
        assert_eq!(popup.lines(), [1, 2]);
        let effects = popup.answer(request, refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2), at("/p/b.rs", 3)]));
        assert!(effects.is_empty(), "no jump and no hint: {effects:?}");
        assert!(popup.flow.content().open && !popup.flow.content().loading);
        assert_eq!(popup.lines(), [1, 2, 3]);
    }

    #[test]
    fn jumps_to_a_single_usage_when_all_arrive_quickly_and_otherwise_stays_open() {
        let mut popup = Popup::new();
        let (request, _) = popup.show();
        popup.libraries(request);
        assert!(matches!(popup.partial(request, &[1]).as_slice(), [PopupEffect::StartTimer { .. }]));
        let effects = popup.answer(request, refs(vec![at("/p/b.rs", 1)]));
        assert_eq!(effects, [open_b(1), hint("Only one usage in Project Files")]);
        assert!(!popup.flow.content().open);
        // The timer of a finished search does nothing.
        assert!(popup.flow.early_timer(1).is_empty());

        let (request, _) = popup.show();
        popup.libraries(request);
        let [PopupEffect::StartTimer { ticket, .. }] = popup.partial(request, &[1]).as_slice() else {
            panic!("a timer starts");
        };
        let ticket = *ticket;
        assert_eq!(popup.flow.early_timer(ticket), [PopupEffect::Open]);
        assert!(popup.answer(request, refs(vec![at("/p/b.rs", 1)])).is_empty());
        let content = popup.flow.content();
        assert!(content.open && !content.loading);
        assert_eq!(popup.lines(), [1]);
    }

    #[test]
    fn searches_project_files_and_project_and_libraries_when_invoked_again() {
        let mut popup = Popup::new();
        // Nothing in Project Files: the hint offers the libraries, searched when invoked again meanwhile.
        let (libraries, effects) = popup.quick(skipped(Vec::new()));
        assert!(!libraries);
        assert_eq!(effects, [hint(&format!("No usages found in Project Files. {WIDEN}"))]);
        let (request, _) = popup.show();
        assert_eq!(popup.libraries(request), Some(true));
        assert_eq!(popup.answer(request, refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)])), [PopupEffect::Open]);
        assert!(popup.flow.content().open);
        assert_eq!(popup.flow.content().scope, Some(UsageScope::Libraries));

        // The scope chooser searches Project Files again.
        let (request, _) = popup.scope(UsageScope::Project);
        assert_eq!(popup.libraries(request), Some(false));
        popup.answer(request, skipped(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)]));
        assert_eq!((popup.flow.content().loading, popup.flow.content().scope), (false, Some(UsageScope::Project)));

        // Switched while Project and Libraries are still searched: that search gives way.
        let (slow, _) = popup.scope(UsageScope::Libraries);
        assert_eq!(popup.libraries(slow), Some(true));
        assert_eq!((popup.flow.content().loading, popup.flow.content().scope), (true, Some(UsageScope::Libraries)));
        let (request, effects) = popup.scope(UsageScope::Project);
        assert!(effects.contains(&PopupEffect::Cancel(slow)));
        assert_eq!(popup.libraries(request), Some(false));
        popup.answer(request, skipped(vec![at("/p/b.rs", 1), at("/p/b.rs", 2)]));
        assert_eq!((popup.flow.content().loading, popup.flow.content().scope), (false, Some(UsageScope::Project)));
        assert!(popup.answer(slow, refs((1..=4).map(|line| at("/p/b.rs", line)).collect())).is_empty());
        assert_eq!(popup.lines(), [1, 2]);

        // Invoked again while it shows Project Files: Project and Libraries.
        let (request, _) = popup.show();
        assert_eq!(popup.libraries(request), Some(true));
        popup.answer(request, refs(vec![at("/p/b.rs", 1), at("/p/b.rs", 2), at("/p/b.rs", 3)]));
        assert_eq!(popup.lines(), [1, 2, 3]);
        popup.flow.dismiss();

        // The only usage in Project Files is opened; the hint offers the libraries too.
        let (_, effects) = popup.quick(skipped(vec![at("/p/b.rs", 1)]));
        assert_eq!(effects, [open_b(1), hint(&format!("Only one usage in Project Files. {WIDEN}"))]);
        let (libraries, effects) = popup.quick(refs(vec![at("/p/b.rs", 1)]));
        assert!(libraries);
        assert_eq!(effects, [open_b(1), hint("Only one usage in Project and Libraries")]);
        // Without a hint showing, it searches the symbol at the caret in Project Files again.
        popup.now += HINT_MS + 1;
        let (libraries, _) = popup.quick(skipped(vec![at("/p/b.rs", 1)]));
        assert!(!libraries);
    }

    #[test]
    fn discards_a_pending_response_after_the_current_popup_is_dismissed() {
        let mut popup = Popup::new();
        let (request, _) = popup.show();
        popup.libraries(request);
        assert_eq!(popup.flow.dismiss(), [PopupEffect::Cancel(request)]);
        let mut other = at("/p/b.rs", 1);
        other.path = IpcPath::from(PathBuf::from("/p/other.rs"));
        assert!(popup.answer(request, refs(vec![at("/p/b.rs", 1), other])).is_empty());
        assert!(!popup.flow.content().open);
    }

    // Not from a spec: without a caret there is nothing to search, and a server error is said.
    #[test]
    fn says_why_nothing_is_searched_or_found() {
        let mut flow = UsagesPopupFlow::default();
        assert_eq!(flow.show(None, 0, RequestId(1)), [hint("Open a file to show usages")]);
        let mut tabs = UsagesTabs::default();
        assert_eq!(tabs.find(None, RequestId(1)), [TabsEffect::Hint(info("Open a file to find usages"))]);

        let mut popup = Popup::new();
        let (request, _) = popup.show();
        popup.libraries(request);
        let effects = popup.answer(request, refs(Vec::new()));
        assert_eq!(effects, [hint("No usages of 'used' found")]);
        let (request, _) = popup.show();
        popup.libraries(request);
        let failed = popup.flow.answered(request, Err(IdeError::cancelled()), &mut I18n::default());
        assert!(failed.is_empty(), "a cancelled search says nothing");

        // Closing a tab cancels its search and selects the next one.
        let first = search_id(&tabs.find(Some(caret()), RequestId(5)));
        tabs.pin(first, true);
        let second = search_id(&tabs.find(Some(caret()), RequestId(6)));
        tabs.select(first);
        assert_eq!(tabs.close(first), [TabsEffect::Cancel(RequestId(5))]);
        assert_eq!(tabs.active().map(|tab| tab.id), Some(second));
        assert_eq!(tabs.reset(), [TabsEffect::Cancel(RequestId(6))]);
        assert!(tabs.tabs().is_empty());
    }
}
