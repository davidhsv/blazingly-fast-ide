//! Guarded tab removal: closes exactly the tabs a command selected, deciding first for every
//! custom document that loses its last tab (prepare it, ask about unsaved changes, save or
//! discard it), and re-evaluating which documents lose their last tab after every step, since
//! the tabs may change while a step runs (a split opens, a tab moves, a document becomes dirty).
//!
//! Discards are applied only after every decision is in, so a later Cancel or failed save never
//! drops changes the user agreed to discard for another document. Every prepared document that
//! does not close resumes. Removals run through the [`EditorGuard`]'s serial queue.

use super::documents::{DocumentKinds, closed_documents};
use super::guard::EditorGuard;
use crate::editor::tabs::{ClosingPolicy, EditorInput, Tab, TabsState};
use crate::platform::i18n::I18n;
use std::collections::BTreeSet;

/// A button of a [`Prompt`]; dismissing the prompt answers `cancel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptButton {
    pub id: &'static str,
    pub label: String,
    pub primary: bool,
}

/// A question for the user, shown with a warning icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub title: String,
    pub body: String,
    pub buttons: Vec<PromptButton>,
}

/// The unsaved-changes question for one document losing its last tab.
pub fn unsaved_prompt(input: &EditorInput, i18n: &I18n) -> Prompt {
    let name = input.path.clone().unwrap_or_else(|| input.key.clone());
    Prompt {
        title: i18n.t("app.unsaved.title", &[]),
        body: i18n.t("app.unsaved.body", &[("count", 1usize.into()), ("project", name.into())]),
        buttons: vec![
            PromptButton { id: "discard", label: i18n.t("app.unsaved.discard", &[]), primary: false },
            PromptButton { id: "save", label: i18n.t("app.unsaved.save", &[]), primary: true },
        ],
    }
}

/// Which tabs a removal closes, selected once when the command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseScope {
    Tab {
        pane: String,
        tab: String,
    },
    /// Close Others: pinned tabs stay.
    Others {
        pane: String,
        tab: String,
    },
    /// Close All, or Close All but Pinned.
    All {
        pane: String,
        keep_pinned: bool,
    },
    ToRight {
        pane: String,
        tab: String,
    },
    ToLeft {
        pane: String,
        tab: String,
    },
    /// Close Unmodified: tabs with unsaved changes or a VCS status (`decorated` paths) stay.
    Unmodified {
        pane: String,
        decorated: BTreeSet<String>,
    },
    /// Close Read-Only: the tabs of `read_only` paths go.
    ReadOnly {
        pane: String,
        read_only: BTreeSet<String>,
    },
    /// Every tab of an input key, in every pane.
    Key(String),
    /// Every tab of a path, in every pane.
    Path(String),
    /// Unsplit: a pane with its tabs.
    Pane(String),
    /// Unsplit All but `0`.
    UnsplitAll(String),
    /// Every tab of every pane; Reopen Closed Tab forgets them.
    Everything,
    /// A deleted file's tabs: one with unsaved changes stays, since external deletion never
    /// discards edits.
    Removed(String),
    /// The pane's tab limit: `keep`, `live` keys and custom documents with unsaved changes stay.
    TabLimit {
        pane: String,
        keep: String,
        limit: usize,
        policy: ClosingPolicy,
        live: BTreeSet<String>,
    },
}

/// What a removal step sees: the tabs as they are now, and which documents are unsaved
/// (custom documents and text documents alike; a failing check answers `true`).
pub struct RemovalContext<'a> {
    pub tabs: &'a TabsState,
    pub kinds: &'a DocumentKinds,
    pub dirty: &'a dyn Fn(&EditorInput) -> bool,
    pub i18n: &'a I18n,
}

/// What the caller does next.
#[derive(Debug, Clone, PartialEq)]
pub enum RemovalStep {
    /// Prepare the document for closing (stop editing and executing, flush pending changes),
    /// then report [`Removal::prepared`].
    Prepare(EditorInput),
    /// Ask, then report [`Removal::answered`] with the button's id.
    Ask { input: EditorInput, prompt: Prompt },
    /// Save the document, then report [`Removal::saved`].
    Save(EditorInput),
    /// Discard the document's unsaved changes, then report [`Removal::discarded`].
    Discard(EditorInput),
    /// Run the command's own last step (Open in New Window), then report
    /// [`Removal::before_removed`].
    BeforeRemove,
    /// The removal ended: apply it, then call [`EditorGuard::finish_removal`].
    Done(RemovalEnd),
}

/// How a removal ended.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalEnd {
    pub removed: bool,
    /// The tabs to install (when removed).
    pub tabs: Option<TabsState>,
    /// Prepared documents that stay open: their close is cancelled before the tabs change.
    pub resume: Vec<EditorInput>,
    /// Documents that lost their last tab: released (closed) after the tabs change.
    pub release: Vec<EditorInput>,
    /// Keys of the documents this removal froze.
    pub prepared: Vec<String>,
    /// A failure to report ("Could not save" with this as the body).
    pub failure: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Running,
    Preparing(EditorInput),
    Asking(EditorInput),
    Saving(EditorInput),
    Discarding,
    BeforeRemove,
    Done,
}

/// One guarded removal.
#[derive(Debug, Clone, PartialEq)]
pub struct Removal {
    generation: u64,
    scope: CloseScope,
    targets: BTreeSet<String>,
    empty_panes: Vec<String>,
    clear_history: bool,
    before_remove: bool,
    prepared: Vec<EditorInput>,
    authorized: BTreeSet<String>,
    discards: Vec<EditorInput>,
    phase: Phase,
}

impl Removal {
    /// Selects the tabs `scope` closes in `ctx.tabs` as they are now.
    pub fn plan(guard: &EditorGuard, scope: CloseScope, ctx: &RemovalContext) -> Self {
        let mut planned = ctx.tabs.clone();
        select(&scope, &mut planned, ctx);
        let remaining: BTreeSet<&str> =
            planned.panes.iter().flat_map(|pane| &pane.tabs).map(|tab| tab.id.as_str()).collect();
        let targets = ctx
            .tabs
            .panes
            .iter()
            .flat_map(|pane| &pane.tabs)
            .filter(|tab| !remaining.contains(tab.id.as_str()))
            .map(|tab| tab.id.clone())
            .collect();
        let empty_panes = ctx
            .tabs
            .pane_ids()
            .into_iter()
            .filter(|id| planned.pane(id).is_none() && ctx.tabs.pane(id).is_some_and(|pane| pane.tabs.is_empty()))
            .map(str::to_owned)
            .collect();
        Self {
            generation: guard.project,
            clear_history: scope == CloseScope::Everything,
            scope,
            targets,
            empty_panes,
            before_remove: false,
            prepared: Vec::new(),
            authorized: BTreeSet::new(),
            discards: Vec::new(),
            phase: Phase::Running,
        }
    }

    /// The pane's tab limit after an open: a removal when the pane holds more tabs than `limit`.
    pub fn tab_limit(
        guard: &EditorGuard,
        pane: &str,
        keep: &str,
        prefs: &ide_core::editor_settings::EditorTabsSettings,
        ctx: &RemovalContext,
    ) -> Option<Self> {
        let limit = usize::try_from(prefs.tab_limit).unwrap_or(usize::MAX);
        let scope = CloseScope::TabLimit {
            pane: pane.to_owned(),
            keep: keep.to_owned(),
            limit,
            policy: ClosingPolicy::from_setting(&prefs.closing_policy),
            live: BTreeSet::new(),
        };
        let removal = Self::plan(guard, scope, ctx);
        (!removal.targets.is_empty()).then_some(removal)
    }

    /// The command runs its own last step before the tabs go ([`RemovalStep::BeforeRemove`]).
    pub fn with_before_remove(mut self) -> Self {
        self.before_remove = true;
        self
    }

    /// Whether the removal selected nothing.
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty() && self.empty_panes.is_empty()
    }

    fn protects(&self, tab: &Tab, ctx: &RemovalContext) -> bool {
        let custom_dirty = || ctx.kinds.has_lifecycle(&tab.input) && (ctx.dirty)(&tab.input);
        match &self.scope {
            CloseScope::Others { .. } | CloseScope::ToRight { .. } | CloseScope::ToLeft { .. } => tab.pinned,
            CloseScope::All { keep_pinned, .. } => *keep_pinned && tab.pinned,
            CloseScope::Unmodified { decorated, .. } => tab.pinned || modified(tab, decorated, ctx),
            CloseScope::ReadOnly { read_only, .. } => tab.pinned || writable(tab, read_only),
            CloseScope::Removed(_) => (ctx.dirty)(&tab.input),
            CloseScope::TabLimit { live, .. } => live.contains(&tab.input.key) || custom_dirty(),
            CloseScope::Tab { .. }
            | CloseScope::Key(_)
            | CloseScope::Path(_)
            | CloseScope::Pane(_)
            | CloseScope::UnsplitAll(_)
            | CloseScope::Everything => false,
        }
    }

    /// The selected tabs that are still open and unprotected, closed in `current`.
    fn current_removal(&self, current: &TabsState, ctx: &RemovalContext) -> TabsState {
        let mut next = current.clone();
        for pane in &current.panes {
            for tab in &pane.tabs {
                if self.targets.contains(&tab.id) && next.pane(&pane.id).is_some() && !self.protects(tab, ctx) {
                    next.close_tab(&pane.id, &tab.id);
                }
            }
        }
        for id in &self.empty_panes {
            if next.pane(id).is_some_and(|pane| pane.tabs.is_empty()) && next.pane_ids().len() > 1 {
                next.remove_pane(id);
            }
        }
        next
    }

    fn still_last(&self, guard: &EditorGuard, input: &EditorInput, ctx: &RemovalContext) -> bool {
        self.generation == guard.project
            && ctx.tabs.open_keys().contains(input.key.as_str())
            && !self.current_removal(ctx.tabs, ctx).open_keys().contains(input.key.as_str())
    }

    fn end(&mut self, end: RemovalEnd) -> RemovalStep {
        self.phase = Phase::Done;
        RemovalStep::Done(end)
    }

    /// The project changed: its replacement releases every document, nothing resumes.
    fn aborted(&mut self) -> RemovalStep {
        let prepared = self.prepared.drain(..).map(|input| input.key).collect();
        let resume = Vec::new();
        self.end(RemovalEnd { removed: false, tabs: None, resume, release: Vec::new(), prepared, failure: None })
    }

    /// Cancelled or failed: every prepared document resumes, the tabs stay.
    fn failed(&mut self, guard: &EditorGuard, failure: Option<String>) -> RemovalStep {
        let resume: Vec<EditorInput> =
            if self.generation == guard.project { self.prepared.clone() } else { Vec::new() };
        let prepared = self.prepared.drain(..).map(|input| input.key).collect();
        self.end(RemovalEnd { removed: false, tabs: None, resume, release: Vec::new(), prepared, failure })
    }

    /// Starts the removal when the queue runs it.
    pub fn start(&mut self, guard: &mut EditorGuard, ctx: &RemovalContext) -> RemovalStep {
        if self.generation != guard.project || guard.session.is_some() {
            return self.end(RemovalEnd {
                removed: false,
                tabs: None,
                resume: Vec::new(),
                release: Vec::new(),
                prepared: Vec::new(),
                failure: None,
            });
        }
        self.resume(guard, ctx)
    }

    fn resume(&mut self, guard: &mut EditorGuard, ctx: &RemovalContext) -> RemovalStep {
        self.phase = Phase::Running;
        loop {
            let after = self.current_removal(ctx.tabs, ctx);
            let next = closed_documents(ctx.tabs, &after, ctx.kinds)
                .into_iter()
                .find(|input| !self.authorized.contains(&input.key));
            let Some(input) = next else {
                if let Some(at) = self.discards.iter().position(|input| self.still_last(guard, input, ctx)) {
                    let input = self.discards.remove(at);
                    self.phase = Phase::Discarding;
                    return RemovalStep::Discard(input);
                }
                break;
            };
            if !self.prepared.iter().any(|prepared| prepared.key == input.key) {
                self.prepared.push(input.clone());
                guard.prepared.insert(input.key.clone());
                self.phase = Phase::Preparing(input.clone());
                return RemovalStep::Prepare(input);
            }
            if let Some(step) = self.decide(input, ctx) {
                return step;
            }
        }
        if self.before_remove {
            self.phase = Phase::BeforeRemove;
            return RemovalStep::BeforeRemove;
        }
        self.commit(ctx)
    }

    /// A prepared document that still loses its last tab: asked about when unsaved, else
    /// authorized to close.
    fn decide(&mut self, input: EditorInput, ctx: &RemovalContext) -> Option<RemovalStep> {
        if (ctx.dirty)(&input) {
            let prompt = unsaved_prompt(&input, ctx.i18n);
            self.phase = Phase::Asking(input.clone());
            return Some(RemovalStep::Ask { input, prompt });
        }
        self.authorized.insert(input.key);
        None
    }

    fn commit(&mut self, ctx: &RemovalContext) -> RemovalStep {
        let mut next = self.current_removal(ctx.tabs, ctx);
        let release = closed_documents(ctx.tabs, &next, ctx.kinds);
        let resume = self
            .prepared
            .iter()
            .filter(|input| !release.iter().any(|closed| closed.key == input.key))
            .cloned()
            .collect();
        if self.clear_history {
            next.closed.clear();
        }
        let prepared = self.prepared.drain(..).map(|input| input.key).collect();
        self.end(RemovalEnd { removed: true, tabs: Some(next), resume, release, prepared, failure: None })
    }

    /// The document of the last [`RemovalStep::Prepare`] is prepared, or preparing it failed.
    pub fn prepared(
        &mut self,
        guard: &mut EditorGuard,
        result: Result<(), String>,
        ctx: &RemovalContext,
    ) -> RemovalStep {
        let Phase::Preparing(input) = std::mem::replace(&mut self.phase, Phase::Running) else {
            return self.failed(guard, None);
        };
        if let Err(error) = result {
            return self.failed(guard, Some(error));
        }
        if self.generation != guard.project {
            return self.aborted();
        }
        if !self.still_last(guard, &input, ctx) {
            return self.resume(guard, ctx);
        }
        match self.decide(input, ctx) {
            Some(step) => step,
            None => self.resume(guard, ctx),
        }
    }

    /// The question of the last [`RemovalStep::Ask`] was answered with `button`.
    pub fn answered(&mut self, guard: &mut EditorGuard, button: &str, ctx: &RemovalContext) -> RemovalStep {
        let Phase::Asking(input) = std::mem::replace(&mut self.phase, Phase::Running) else {
            return self.failed(guard, None);
        };
        if self.generation != guard.project {
            return self.aborted();
        }
        if !self.still_last(guard, &input, ctx) {
            return self.resume(guard, ctx);
        }
        match button {
            "save" => {
                self.phase = Phase::Saving(input.clone());
                RemovalStep::Save(input)
            }
            "discard" => {
                self.authorized.insert(input.key.clone());
                self.discards.push(input);
                self.resume(guard, ctx)
            }
            _ => self.failed(guard, None),
        }
    }

    /// The document of the last [`RemovalStep::Save`] was saved, or saving it failed.
    pub fn saved(&mut self, guard: &mut EditorGuard, result: Result<(), String>, ctx: &RemovalContext) -> RemovalStep {
        let Phase::Saving(input) = std::mem::replace(&mut self.phase, Phase::Running) else {
            return self.failed(guard, None);
        };
        if let Err(error) = result {
            return self.failed(guard, Some(error));
        }
        self.authorized.insert(input.key);
        self.resume(guard, ctx)
    }

    /// The document of the last [`RemovalStep::Discard`] was discarded, or discarding it failed.
    pub fn discarded(
        &mut self,
        guard: &mut EditorGuard,
        result: Result<(), String>,
        ctx: &RemovalContext,
    ) -> RemovalStep {
        if self.phase != Phase::Discarding {
            return self.failed(guard, None);
        }
        if let Err(error) = result {
            return self.failed(guard, Some(error));
        }
        if self.generation != guard.project {
            return self.aborted();
        }
        self.resume(guard, ctx)
    }

    /// The command's own last step finished; `ok == false` keeps every tab.
    pub fn before_removed(&mut self, guard: &mut EditorGuard, ok: bool, ctx: &RemovalContext) -> RemovalStep {
        if self.phase != Phase::BeforeRemove || !ok {
            return self.failed(guard, None);
        }
        if self.generation != guard.project {
            return self.aborted();
        }
        self.commit(ctx)
    }
}

/// Close Unmodified: unsaved changes or a VCS status count as modified.
fn modified(tab: &Tab, decorated: &BTreeSet<String>, ctx: &RemovalContext) -> bool {
    (ctx.dirty)(&tab.input) || tab.input.path.as_ref().is_some_and(|path| decorated.contains(path))
}

fn writable(tab: &Tab, read_only: &BTreeSet<String>) -> bool {
    tab.input.path.as_ref().is_none_or(|path| !read_only.contains(path))
}

/// Applies `scope`'s transition to `tabs`, selecting what the removal closes.
fn select(scope: &CloseScope, tabs: &mut TabsState, ctx: &RemovalContext) {
    match scope {
        CloseScope::Tab { pane, tab } => {
            tabs.close_tab(pane, tab);
        }
        CloseScope::Others { pane, tab } => {
            tabs.close_others(pane, tab);
        }
        CloseScope::All { pane, keep_pinned } => {
            tabs.close_all(pane, *keep_pinned);
        }
        CloseScope::ToRight { pane, tab } => {
            tabs.close_to_right(pane, tab);
        }
        CloseScope::ToLeft { pane, tab } => {
            tabs.close_to_left(pane, tab);
        }
        CloseScope::Unmodified { pane, decorated } => {
            tabs.close_unmodified(pane, |tab| modified(tab, decorated, ctx));
        }
        CloseScope::ReadOnly { pane, read_only } => {
            tabs.close_unmodified(pane, |tab| writable(tab, read_only));
        }
        CloseScope::Key(key) | CloseScope::Removed(key) => {
            tabs.close_key(key);
        }
        CloseScope::Path(path) => {
            let keys: BTreeSet<String> = tabs
                .panes
                .iter()
                .flat_map(|pane| &pane.tabs)
                .filter(|tab| tab.input.path.as_deref() == Some(path.as_str()))
                .map(|tab| tab.input.key.clone())
                .collect();
            for key in keys {
                tabs.close_key(&key);
            }
        }
        CloseScope::Pane(pane) => {
            tabs.remove_pane(pane);
        }
        CloseScope::UnsplitAll(pane) => {
            tabs.unsplit_all(pane);
        }
        CloseScope::Everything => {
            let panes: Vec<String> = tabs.pane_ids().into_iter().map(str::to_owned).collect();
            for pane in panes {
                if tabs.pane(&pane).is_some() {
                    tabs.close_all(&pane, false);
                }
            }
        }
        CloseScope::TabLimit { pane, keep, limit, policy, live } => {
            let protect = |tab: &Tab| {
                live.contains(&tab.input.key) || (ctx.kinds.has_lifecycle(&tab.input) && (ctx.dirty)(&tab.input))
            };
            tabs.enforce_tab_limit(pane, *limit, *policy, |tab| (ctx.dirty)(&tab.input), Some(keep.as_str()), protect);
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::editor::tabs::SplitDir;
    use crate::editor_model::documents::DocumentFailures;
    use crate::editor_model::guard::{
        Admission, CloseSession, EditorError, Job, OpenOptions, OpenRequest, PrepareStep,
    };
    use ide_core::editor_settings::EditorTabsSettings;
    use std::collections::{BTreeMap, VecDeque};

    const KIND: &str = "lifecycle-test";

    /// A custom document: `text` is what the editor shows, `pending` what it has not flushed yet,
    /// `running` an execution whose final output arrives when it is drained.
    #[derive(Debug, Clone, PartialEq)]
    pub(crate) struct Doc {
        pub saved: String,
        pub text: String,
        pub pending: String,
        pub running: bool,
        pub frozen: bool,
        pub closed: bool,
        pub saves: u32,
        pub releases: u32,
        pub save_error: Option<String>,
        pub save_throws: Option<String>,
        pub prepare_error: Option<String>,
    }

    impl Default for Doc {
        fn default() -> Self {
            Self {
                saved: "original".into(),
                text: "edited".into(),
                pending: String::new(),
                running: false,
                frozen: false,
                closed: false,
                saves: 0,
                releases: 0,
                save_error: None,
                save_throws: None,
                prepare_error: None,
            }
        }
    }

    impl Doc {
        pub fn is_dirty(&self) -> bool {
            self.text != self.saved || !self.pending.is_empty()
        }
        fn flush(&mut self) {
            let pending = std::mem::take(&mut self.pending);
            self.text.push_str(&pending);
        }
        pub fn save(&mut self) -> Result<(), String> {
            if let Some(error) = &self.save_throws {
                return Err(error.clone());
            }
            self.flush();
            if let Some(error) = &self.save_error {
                return Err(error.clone());
            }
            self.saves += 1;
            self.saved.clone_from(&self.text);
            Ok(())
        }
        pub fn freeze(&mut self) {
            self.frozen = true;
            self.flush();
        }
        pub fn drain(&mut self) -> Result<(), String> {
            if self.running {
                self.text.push_str("\nfinal output");
            }
            self.running = false;
            self.prepare_error.clone().map_or(Ok(()), Err)
        }
        pub fn prepare_close(&mut self) -> Result<(), String> {
            self.freeze();
            self.drain()
        }
        pub fn discard(&mut self) {
            self.pending.clear();
            self.text.clone_from(&self.saved);
        }
        pub fn close(&mut self) {
            self.closed = true;
            self.running = false;
            self.releases += 1;
        }
    }

    pub(crate) fn input(name: &str) -> EditorInput {
        let path = Some(format!("/p/{name}.ipynb"));
        EditorInput { kind: KIND.into(), key: format!("custom:{name}"), path, data: None }
    }

    pub(crate) fn marker(key: &str) -> EditorInput {
        EditorInput { kind: "read-only-test".into(), key: key.into(), path: None, data: None }
    }

    /// Whether a tab's document is dirty, as the documents are now.
    fn dirty_of(docs: &BTreeMap<String, Doc>) -> impl Fn(&EditorInput) -> bool + use<> {
        let dirty: std::collections::HashSet<String> =
            docs.iter().filter(|(_, doc)| doc.is_dirty()).map(|(key, _)| key.clone()).collect();
        move |input| dirty.contains(&input.key)
    }

    type Hook = Box<dyn FnMut(&mut Ide)>;
    type BeforeRemove = Box<dyn FnMut(&mut Ide) -> bool>;

    fn background() -> OpenOptions {
        OpenOptions { focus: false, ..OpenOptions::default() }
    }

    /// The editor's owner, carrying out every step the guard and its removals ask for.
    pub(crate) struct Ide {
        pub tabs: TabsState,
        pub guard: EditorGuard,
        pub docs: BTreeMap<String, Doc>,
        pub kinds: DocumentKinds,
        pub i18n: I18n,
        pub prefs: EditorTabsSettings,
        pub autosave: bool,
        pub answers: VecDeque<&'static str>,
        pub asked: usize,
        pub on_ask: Option<Hook>,
        pub before_remove: Option<BeforeRemove>,
        pub failures: Vec<String>,
        /// The lifecycle calls made, in order.
        pub events: Vec<&'static str>,
    }

    impl Ide {
        pub fn new() -> Self {
            Self {
                tabs: TabsState::new(),
                guard: EditorGuard::new(),
                docs: BTreeMap::new(),
                kinds: DocumentKinds::new([KIND]),
                i18n: I18n::default(),
                prefs: EditorTabsSettings::default(),
                autosave: false,
                answers: VecDeque::new(),
                asked: 0,
                on_ask: None,
                before_remove: None,
                failures: Vec::new(),
                events: Vec::new(),
            }
        }

        pub fn doc(&mut self, name: &str) -> &mut Doc {
            self.docs.get_mut(&input(name).key).expect("an open document")
        }

        pub fn has(&self, key: &str) -> bool {
            self.tabs.find_tab(key, None).is_some()
        }

        pub fn tab_id(&self, key: &str, pane: Option<&str>) -> String {
            self.tabs.find_tab(key, pane).map(|(_, tab)| tab.id.clone()).expect("an open tab")
        }

        pub fn split(&mut self, pane: &str) -> String {
            self.tabs.split_pane(pane, SplitDir::Row).expect("a split")
        }

        fn with_ctx<R>(&mut self, f: impl FnOnce(&mut EditorGuard, &RemovalContext) -> R) -> R {
            let dirty = dirty_of(&self.docs);
            let ctx = RemovalContext { tabs: &self.tabs, kinds: &self.kinds, dirty: &dirty, i18n: &self.i18n };
            f(&mut self.guard, &ctx)
        }

        fn apply_resumed(&mut self) {
            for resumed in self.guard.take_resumed() {
                if let Some(doc) = self.docs.get_mut(&resumed.key) {
                    doc.frozen = false;
                }
            }
        }

        pub fn open_input(&mut self, opened: EditorInput, opts: OpenOptions) -> Result<(), EditorError> {
            let request = self.guard.begin_open(&self.tabs, &opts);
            self.apply_resumed();
            let request = request?;
            self.admit_request(&request, opened, &opts).map(|_| ())
        }

        /// Admits a started open, then applies the pane's tab limit.
        pub fn admit_request(
            &mut self,
            request: &OpenRequest,
            opened: EditorInput,
            opts: &OpenOptions,
        ) -> Result<Admission, EditorError> {
            let admission = self.guard.admit(request, opened, opts, &mut self.tabs, &self.prefs, &self.kinds);
            self.apply_resumed();
            if let Ok(Admission::Opened { pane, keep, .. }) = &admission {
                self.guard.note_tabs(&self.tabs);
                let prefs = self.prefs.clone();
                let limit = self.with_ctx(|guard, ctx| Removal::tab_limit(guard, pane, keep, &prefs, ctx));
                if let Some(removal) = limit {
                    self.queue(removal);
                }
            }
            admission
        }

        pub fn begin(&mut self, opts: &OpenOptions) -> OpenRequest {
            let request = self.guard.begin_open(&self.tabs, opts);
            self.apply_resumed();
            request.expect("the open starts")
        }

        /// The path of the active pane's active tab.
        pub fn active_path(&self) -> Option<String> {
            let pane = self.tabs.pane(&self.tabs.active_pane)?;
            pane.active_tab().and_then(|tab| tab.input.path.clone())
        }

        /// Open paths in visual order, each once.
        pub fn open_paths(&self) -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for pane in self.tabs.pane_ids() {
                for tab in self.tabs.pane(pane).map(|pane| pane.tabs.as_slice()).unwrap_or_default() {
                    if let Some(path) = &tab.input.path
                        && !out.contains(path)
                    {
                        out.push(path.clone());
                    }
                }
            }
            out
        }

        pub fn open(&mut self, name: &str) -> Result<(), EditorError> {
            self.docs.insert(input(name).key, Doc::default());
            self.open_input(input(name), OpenOptions { preview: true, ..background() })
        }

        pub fn open_marker(&mut self, key: &str, opts: OpenOptions) {
            self.open_input(marker(key), opts).expect("the marker opens");
        }

        pub fn queue(&mut self, removal: Removal) -> Option<bool> {
            let job = self.guard.enqueue(Job::Removal(Box::new(removal)))?;
            Some(self.run_job(job))
        }

        pub fn plan(&mut self, scope: CloseScope) -> Removal {
            self.with_ctx(|guard, ctx| Removal::plan(guard, scope, ctx))
        }

        pub fn close(&mut self, scope: CloseScope) -> bool {
            let removal = self.plan(scope);
            self.queue(removal).unwrap_or(false)
        }

        fn run_job(&mut self, job: Job) -> bool {
            match job {
                Job::Removal(mut removal) => {
                    let step = self.with_ctx(|guard, ctx| removal.start(guard, ctx));
                    self.drive(*removal, step)
                }
                Job::PrepareClose => self.run_preparation().is_ok(),
            }
        }

        pub fn drive(&mut self, mut removal: Removal, mut step: RemovalStep) -> bool {
            loop {
                step = match step {
                    RemovalStep::Prepare(prepared) => {
                        self.events.push("prepare");
                        let result = self.docs.get_mut(&prepared.key).expect("a document").prepare_close();
                        self.with_ctx(|guard, ctx| removal.prepared(guard, result, ctx))
                    }
                    RemovalStep::Ask { .. } => {
                        self.asked += 1;
                        self.events.push("ask");
                        if let Some(mut hook) = self.on_ask.take() {
                            hook(self);
                        }
                        let button = self.answers.pop_front().unwrap_or("cancel");
                        self.with_ctx(|guard, ctx| removal.answered(guard, button, ctx))
                    }
                    RemovalStep::Save(saved) => {
                        self.events.push("save");
                        let result = self.docs.get_mut(&saved.key).expect("a document").save();
                        self.with_ctx(|guard, ctx| removal.saved(guard, result, ctx))
                    }
                    RemovalStep::Discard(discarded) => {
                        self.events.push("discard");
                        self.docs.get_mut(&discarded.key).expect("a document").discard();
                        self.with_ctx(|guard, ctx| removal.discarded(guard, Ok(()), ctx))
                    }
                    RemovalStep::BeforeRemove => {
                        let ok = match self.before_remove.take() {
                            Some(mut before_remove) => before_remove(self),
                            None => true,
                        };
                        self.with_ctx(|guard, ctx| removal.before_removed(guard, ok, ctx))
                    }
                    RemovalStep::Done(end) => return self.finish(end),
                };
            }
        }

        fn finish(&mut self, end: RemovalEnd) -> bool {
            for resumed in &end.resume {
                self.events.push("cancelClose");
                if let Some(doc) = self.docs.get_mut(&resumed.key) {
                    doc.frozen = false;
                }
            }
            if let Some(tabs) = end.tabs.clone() {
                let before = std::mem::replace(&mut self.tabs, tabs);
                if self.autosave {
                    let dirty = dirty_of(&self.docs);
                    let targets = self.guard.switch_autosave(&before, &self.tabs, &self.kinds, &dirty);
                    for target in targets {
                        let _ = self.docs.get_mut(&target.key).map(Doc::save);
                    }
                }
                self.guard.note_tabs(&self.tabs);
            }
            if let Some(failure) = &end.failure {
                self.failures.push(failure.clone());
            }
            let next = self.guard.finish_removal(&end);
            for released in &end.release {
                self.events.push("close");
                if let Some(doc) = self.docs.get_mut(&released.key) {
                    doc.close();
                }
                self.guard.released(&released.key);
            }
            if let Some(job) = next {
                self.run_job(job);
            }
            end.removed
        }

        pub fn prepare_close(&mut self) -> Result<CloseSession, EditorError> {
            match self.guard.request_close() {
                Some(_) => self.run_preparation(),
                None => Err(EditorError::Closing),
            }
        }

        fn run_preparation(&mut self) -> Result<CloseSession, EditorError> {
            let dirty = dirty_of(&self.docs);
            let (mut preparation, mut step) = self.guard.start_preparation(&self.tabs, &self.kinds, &dirty);
            loop {
                match step {
                    PrepareStep::Prepare(prepared) => {
                        let result = self.docs.get_mut(&prepared.key).expect("a document").prepare_close();
                        let preparation = preparation.as_mut().expect("a preparation");
                        let dirty = dirty_of(&self.docs);
                        step = self.guard.document_prepared(preparation, result, &dirty);
                    }
                    PrepareStep::Ready(session) => {
                        self.after_job();
                        return Ok(session);
                    }
                    PrepareStep::Failed(error) => {
                        self.after_job();
                        return Err(error);
                    }
                }
            }
        }

        fn after_job(&mut self) {
            self.apply_resumed();
            if let Some(job) = self.guard.job_finished() {
                self.run_job(job);
            }
        }

        pub fn cancel_session(&mut self, id: u64) {
            self.guard.cancel_session(id);
            self.apply_resumed();
        }

        pub fn cancel_close(&mut self) {
            self.guard.cancel_close();
            self.apply_resumed();
        }

        pub fn discard_session(&mut self, id: u64) {
            let dirty = dirty_of(&self.docs);
            let targets = self.guard.discard_session(id, &dirty).expect("a current session");
            for target in targets {
                self.docs.get_mut(&target.key).expect("a document").discard();
            }
        }

        /// The project was replaced: the tabs reset and the old documents are released.
        pub fn replace_project(&mut self) {
            let released = self.guard.project_replaced(&self.tabs, &self.kinds);
            self.tabs = TabsState::new();
            for document in released {
                if let Some(doc) = self.docs.get_mut(&document.key) {
                    doc.close();
                }
                self.guard.released(&document.key);
            }
        }

        pub fn save_all(&mut self) -> Result<(), DocumentFailures> {
            let dirty = dirty_of(&self.docs);
            let targets = self.guard.save_targets(&self.tabs, &self.kinds, &dirty);
            let results: Vec<Result<(), String>> =
                targets.iter().map(|target| self.docs.get_mut(&target.key).expect("a document").save()).collect();
            DocumentFailures::collect(results)
        }

        pub fn active_pane(&self) -> String {
            self.tabs.active_pane.clone()
        }
    }

    #[test]
    fn rejects_a_delayed_virtual_document_from_the_previous_project() {
        let mut ide = Ide::new();
        let request = ide.guard.begin_open(&ide.tabs, &background()).expect("the open starts");
        ide.replace_project();
        assert_eq!(ide.guard.check_opening(&request), Err(EditorError::ProjectChanged));
        let virtual_input = EditorInput {
            kind: "virtual".into(),
            key: "virtual:jdt://old/Class".into(),
            path: Some("jdt://old/Class".into()),
            data: None,
        };
        let admitted = ide.guard.admit(&request, virtual_input, &background(), &mut ide.tabs, &ide.prefs, &ide.kinds);
        assert_eq!(admitted, Err(EditorError::ProjectChanged));
        assert!(!ide.has("virtual:jdt://old/Class"));
    }

    #[test]
    fn rolls_back_delayed_provider_acquisition_after_a_project_close() {
        for route in ["pending", "cancelled", "replaced"] {
            let mut ide = Ide::new();
            ide.docs.insert(input("a").key, Doc::default());
            let request = ide.guard.begin_open(&ide.tabs, &background()).expect("the open starts");
            let session = ide.prepare_close().expect("a session");
            match route {
                "cancelled" => ide.cancel_session(session.id),
                "replaced" => {
                    ide.guard.commit_session(session.id).expect("the session commits");
                    ide.replace_project();
                }
                _ => {}
            }
            // The provider resolved: its admission is refused, so the caller rolls it back.
            let admitted = ide.guard.admit(&request, input("a"), &background(), &mut ide.tabs, &ide.prefs, &ide.kinds);
            let expected = if route == "replaced" { EditorError::ProjectChanged } else { EditorError::Closing };
            assert_eq!(admitted, Err(expected), "{route}");
            ide.cancel_session(session.id);
            assert!(!ide.has(&input("a").key), "{route}");
        }
    }

    #[test]
    fn rejects_a_new_open_before_invoking_a_provider_while_close_admission_is_pending() {
        let mut ide = Ide::new();
        let job = ide.guard.request_close();
        assert_eq!(job, Some(Job::PrepareClose));
        assert_eq!(ide.guard.begin_open(&ide.tabs, &background()), Err(EditorError::Closing));
        let dirty = |_: &EditorInput| false;
        let (_, step) = ide.guard.start_preparation(&ide.tabs, &ide.kinds, &dirty);
        let PrepareStep::Ready(session) = step else { panic!("an empty editor prepares at once") };
        ide.guard.job_finished();
        ide.cancel_session(session.id);
        assert!(ide.guard.begin_open(&ide.tabs, &background()).is_ok());
    }

    #[test]
    fn retains_pending_changes_and_the_last_tab_on_cancel() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.doc("a").pending = " pending".into();
        assert!(!ide.close(CloseScope::Key(input("a").key)));
        assert!(ide.has(&input("a").key));
        let doc = ide.doc("a").clone();
        assert_eq!(doc.text, "edited pending");
        assert_eq!(doc.saved, "original");
        assert!(!doc.frozen);
        assert!(!doc.closed);
    }

    #[test]
    fn saves_a_drained_execution_checkpoint_before_removing_the_last_tab() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.doc("a").running = true;
        ide.answers.push_back("save");
        assert!(ide.close(CloseScope::Path("/p/a.ipynb".into())));
        let doc = ide.doc("a").clone();
        assert_eq!(doc.saved, "edited\nfinal output");
        assert!(!doc.running);
        assert!(doc.closed);
        assert!(!ide.has(&input("a").key));
    }

    #[test]
    fn discards_without_a_removal_triggered_autosave() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.autosave = true;
        ide.doc("a").pending = " unsaved".into();
        ide.answers.push_back("discard");
        assert!(ide.close(CloseScope::Everything));
        let doc = ide.doc("a").clone();
        assert_eq!(doc.saved, "original");
        assert_eq!(doc.text, "original");
        assert_eq!(doc.saves, 0);
        assert!(doc.closed);
    }

    #[test]
    fn keeps_dirty_state_and_tabs_when_save_fails() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.doc("a").save_error = Some("external modification conflict".into());
        ide.answers.push_back("save");
        let pane = ide.active_pane();
        let tab = ide.tab_id(&input("a").key, Some(pane.as_str()));
        assert!(!ide.close(CloseScope::Tab { pane, tab }));
        assert!(ide.has(&input("a").key));
        let doc = ide.doc("a").clone();
        assert!(doc.is_dirty());
        assert!(!doc.frozen);
        assert!(!doc.closed);
        assert_eq!(ide.failures, ["external modification conflict"]);
    }

    #[test]
    fn closing_one_split_neither_prompts_nor_stops_its_shared_execution() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.doc("a").running = true;
        let first = ide.active_pane();
        ide.split(&first);
        assert!(ide.close(CloseScope::Pane(first)));
        let doc = ide.doc("a").clone();
        assert!(doc.running);
        assert!(!doc.closed);
        assert!(!doc.frozen);
        assert_eq!(ide.asked, 0);
        ide.answers.push_back("discard");
        assert!(ide.close(CloseScope::Key(input("a").key)));
        assert_eq!(ide.doc("a").releases, 1);
        assert!(!ide.doc("a").running);
    }

    #[test]
    fn does_not_discard_shared_state_when_a_split_opens_while_a_close_prompt_awaits() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let first = ide.active_pane();
        let split_from = first.clone();
        ide.on_ask = Some(Box::new(move |ide: &mut Ide| {
            ide.split(&split_from);
        }));
        ide.answers.push_back("discard");
        ide.close(CloseScope::All { pane: first, keep_pinned: false });
        assert_eq!(ide.tabs.pane_ids().len(), 1);
        assert!(ide.has(&input("a").key));
        let doc = ide.doc("a").clone();
        assert_eq!(doc.text, "edited");
        assert!(!doc.closed);
        assert!(!doc.frozen);
    }

    #[test]
    fn saves_a_split_document_once_and_reports_failures_after_trying_every_document() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let pane = ide.active_pane();
        ide.split(&pane);
        ide.open("b").expect("opens");
        ide.doc("a").save_error = Some("disk full".into());
        assert_eq!(ide.save_all().map_err(|failures| failures.message().to_owned()), Err("disk full".to_owned()));
        assert_eq!(ide.doc("a").saved, "original");
        assert!(ide.doc("a").is_dirty());
        assert_eq!(ide.doc("b").saved, "edited");
        ide.doc("a").save_error = None;
        assert!(ide.save_all().is_ok());
        assert_eq!(ide.doc("a").saves, 1);
        assert_eq!(ide.doc("b").saves, 1);
    }

    #[test]
    fn never_previews_or_evicts_a_dirty_custom_editor_under_lru_pressure() {
        let mut ide = Ide::new();
        ide.prefs.tab_limit = 1;
        ide.prefs.closing_policy = "lru".into();
        ide.open("a").expect("opens");
        ide.open_marker("other", OpenOptions { preview: true, ..background() });
        let (_, tab) = ide.tabs.find_tab(&input("a").key, None).expect("the custom tab");
        assert!(!tab.preview);
        assert!(!ide.doc("a").closed);
        assert_eq!(ide.doc("a").text, "edited");
        assert!(ide.has("other"));
    }

    #[test]
    fn close_unmodified_retains_pending_custom_changes() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let doc = ide.doc("a");
        doc.text.clone_from(&doc.saved);
        doc.pending = "new typing".into();
        ide.open_marker("other", background());
        let pane = ide.active_pane();
        ide.close(CloseScope::Unmodified { pane, decorated: BTreeSet::new() });
        assert!(ide.has(&input("a").key));
        assert!(!ide.has("other"));
        assert_eq!(ide.doc("a").pending, "new typing");
    }

    #[test]
    fn close_read_only_retains_a_dirty_custom_document_on_cancel_and_releases_it_after_discard() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let pane = ide.active_pane();
        let read_only = BTreeSet::from(["/p/a.ipynb".to_owned()]);
        let cancelled = ide.close(CloseScope::ReadOnly { pane: pane.clone(), read_only: read_only.clone() });
        assert!(ide.has(&input("a").key));
        assert_eq!(ide.doc("a").text, "edited");
        assert!(!ide.doc("a").closed);
        assert!(!cancelled);
        ide.answers.push_back("discard");
        assert!(ide.close(CloseScope::ReadOnly { pane, read_only }));
        assert!(!ide.has(&input("a").key));
        assert_eq!(ide.doc("a").saved, "original");
        assert!(ide.doc("a").closed);
    }

    #[test]
    fn unsplit_all_preserves_cancelled_documents_and_only_releases_their_last_removed_view() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let keep = ide.active_pane();
        ide.split(&keep);
        let removed_pane = ide.active_pane();
        ide.open("removed").expect("opens");
        let cancelled = ide.close(CloseScope::UnsplitAll(keep.clone()));
        assert!(ide.tabs.pane_ids().contains(&removed_pane.as_str()));
        assert!(ide.has(&input("removed").key));
        assert_eq!(ide.doc("removed").text, "edited");
        assert!(!ide.doc("removed").closed);
        assert!(!cancelled);
        ide.answers.push_back("discard");
        assert!(ide.close(CloseScope::UnsplitAll(keep.clone())));
        assert_eq!(ide.tabs.pane_ids(), [keep.as_str()]);
        assert_eq!(ide.doc("removed").saved, "original");
        assert!(ide.doc("removed").closed);
        assert_eq!(ide.doc("a").text, "edited");
        assert!(!ide.doc("a").closed);
    }

    #[test]
    fn prepares_hidden_documents_deduplicates_split_state_and_resumes_after_a_failed_project_transition() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let pane = ide.active_pane();
        ide.split(&pane);
        ide.open("b").expect("opens");
        ide.doc("b").running = true;
        let session = ide.prepare_close().expect("a session");
        assert_eq!(session.dirty, 2);
        assert!(ide.doc("a").frozen);
        assert!(!ide.doc("b").running);
        assert!(ide.save_all().is_ok());
        assert_eq!(ide.doc("b").saved, "edited\nfinal output");
        ide.guard.commit_session(session.id).expect("the session commits");
        ide.cancel_close();
        assert!(!ide.doc("a").frozen);
        assert!(!ide.doc("b").frozen);
        assert!(!ide.doc("a").closed);
        assert!(ide.has(&input("a").key));
    }

    #[test]
    fn project_replacement_releases_old_split_documents_once_without_saving_discarded_changes() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let pane = ide.active_pane();
        ide.split(&pane);
        let session = ide.prepare_close().expect("a session");
        ide.discard_session(session.id);
        ide.guard.commit_session(session.id).expect("the session commits");
        ide.replace_project();
        ide.open_marker("new-project-tab", background());
        let doc = ide.doc("a").clone();
        assert_eq!(doc.saved, "original");
        assert_eq!(doc.saves, 0);
        assert_eq!(doc.releases, 1);
        assert!(!ide.has(&input("a").key));
        assert!(ide.has("new-project-tab"));
    }

    #[test]
    fn respects_a_last_document_cancel_without_removing_other_tabs() {
        for route in ["all", "others", "left", "right", "unsplit"] {
            let mut ide = Ide::new();
            if route != "left" {
                ide.open_marker("marker", background());
            }
            ide.open("a").expect("opens");
            let pane = ide.active_pane();
            if route == "left" {
                ide.open_marker("marker", background());
            }
            if route == "unsplit" {
                ide.open_marker("marker", OpenOptions { side: true, ..background() });
            }
            let marker_tab = ide.tab_id("marker", None);
            let scope = match route {
                "all" => CloseScope::All { pane, keep_pinned: false },
                "others" => CloseScope::Others { pane, tab: marker_tab },
                "left" => CloseScope::ToLeft { pane, tab: marker_tab },
                "right" => CloseScope::ToRight { pane, tab: marker_tab },
                _ => CloseScope::Pane(pane),
            };
            assert!(!ide.close(scope), "{route}");
            assert!(ide.has(&input("a").key), "{route}");
            assert!(ide.has("marker"), "{route}");
            let doc = ide.doc("a").clone();
            assert_eq!(doc.text, "edited", "{route}");
            assert!(!doc.closed, "{route}");
            assert!(!doc.frozen, "{route}");
        }
    }

    #[test]
    fn keeps_newly_opened_tabs_when_a_bulk_close_completes() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.on_ask = Some(Box::new(|ide: &mut Ide| {
            ide.open_marker("new-tab", background());
            assert!(ide.has("new-tab"));
        }));
        ide.answers.push_back("discard");
        assert!(ide.close(CloseScope::Everything));
        assert!(ide.has("new-tab"));
        assert!(!ide.has(&input("a").key));
    }

    #[test]
    fn autosaves_when_the_application_is_deactivated_while_focus_is_in_a_frame() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.autosave = true;
        let dirty = dirty_of(&ide.docs);
        let targets = ide.guard.blur_autosave(&ide.tabs, &ide.kinds, &dirty);
        assert_eq!(targets, [input("a")]);
        ide.doc("a").save().expect("saves");
        assert_eq!(ide.doc("a").saved, "edited");
    }

    #[test]
    fn does_not_prompt_or_blur_autosave_a_document_until_execution_has_drained() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.autosave = true;
        ide.doc("a").running = true;
        let mut removal = ide.with_ctx(|guard, ctx| Removal::plan(guard, CloseScope::Key(input("a").key), ctx));
        assert!(ide.guard.enqueue(Job::Removal(Box::new(removal.clone()))).is_some());
        let step = ide.with_ctx(|guard, ctx| removal.start(guard, ctx));
        assert_eq!(step, RemovalStep::Prepare(input("a")));
        ide.doc("a").freeze();
        assert!(ide.doc("a").frozen);
        let dirty = dirty_of(&ide.docs);
        assert!(ide.guard.blur_autosave(&ide.tabs, &ide.kinds, &dirty).is_empty());
        assert_eq!(ide.doc("a").saved, "original");
        assert_eq!(ide.asked, 0);
        let drained = ide.doc("a").drain();
        let step = ide.with_ctx(|guard, ctx| removal.prepared(guard, drained, ctx));
        ide.drive(removal, step);
        let doc = ide.doc("a").clone();
        assert_eq!(doc.text, "edited\nfinal output");
        assert_eq!(doc.saved, "original");
        assert!(!doc.frozen);
        assert!(!doc.closed);
    }

    #[test]
    fn unfreezes_every_prepared_document_when_another_preparation_fails() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.open("b").expect("opens");
        ide.doc("b").prepare_error = Some("pending source could not be flushed".into());
        let failed = ide.prepare_close();
        assert_eq!(failed, Err(EditorError::Document("pending source could not be flushed".into())));
        assert!(!ide.doc("a").frozen);
        assert!(!ide.doc("b").frozen);
        assert_eq!(ide.doc("a").text, "edited");
        assert_eq!(ide.doc("b").text, "edited");
        assert!(ide.has(&input("a").key));
        assert!(ide.has(&input("b").key));
    }

    #[test]
    fn evicts_a_clean_custom_document_through_its_last_reference_lifecycle() {
        let mut ide = Ide::new();
        ide.prefs.tab_limit = 1;
        ide.open("a").expect("opens");
        let doc = ide.doc("a");
        doc.text.clone_from(&doc.saved);
        ide.open_marker("new-tab", background());
        assert!(!ide.has(&input("a").key));
        assert!(ide.has("new-tab"));
        assert_eq!(ide.doc("a").releases, 1);
        assert_eq!(ide.doc("a").saved, "original");
        assert_eq!(ide.asked, 0);
    }

    #[test]
    fn releases_old_documents_when_a_replacement_project_has_the_same_root() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let session = ide.prepare_close().expect("a session");
        ide.discard_session(session.id);
        ide.guard.commit_session(session.id).expect("the session commits");
        ide.guard.set_opening_project(true);
        ide.replace_project();
        ide.guard.set_opening_project(false);
        ide.open_marker("replacement-tab", background());
        assert_eq!(ide.doc("a").releases, 1);
        assert!(!ide.has(&input("a").key));
        assert!(ide.has("replacement-tab"));
    }

    #[test]
    fn does_not_apply_an_earlier_bulk_discard_when_a_later_cancel_or_save_aborts_closing() {
        for button in ["cancel", "save"] {
            let mut ide = Ide::new();
            ide.open("a").expect("opens");
            ide.open("b").expect("opens");
            ide.doc("b").save_error = Some("disk full".into());
            ide.answers.extend(["discard", button]);
            assert!(!ide.close(CloseScope::Everything), "{button}");
            for name in ["a", "b"] {
                let doc = ide.doc(name).clone();
                assert_eq!(doc.text, "edited", "{button} {name}");
                assert_eq!(doc.saved, "original", "{button} {name}");
                assert!(!doc.frozen, "{button} {name}");
                assert!(!doc.closed, "{button} {name}");
                assert!(ide.has(&input(name).key), "{button} {name}");
            }
        }
    }

    #[test]
    fn guards_a_document_that_becomes_the_last_split_reference_during_another_close_prompt() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let first = ide.active_pane();
        let first_tab = ide.tab_id(&input("a").key, Some(first.as_str()));
        ide.split(&first);
        let second = ide.active_pane();
        ide.open("b").expect("opens");
        ide.open_marker("marker", background());
        let marker_tab = ide.tab_id("marker", None);
        let (from, to) = (first.clone(), second.clone());
        // Moving onto the existing split merges its duplicate, leaving the originally targeted
        // tab as the only remaining reference.
        ide.on_ask = Some(Box::new(move |ide: &mut Ide| {
            ide.tabs.move_tab(&from, &first_tab, &to, 0, None);
        }));
        ide.answers.push_back("discard");
        assert!(!ide.close(CloseScope::Others { pane: second, tab: marker_tab }));
        for name in ["a", "b"] {
            let doc = ide.doc(name).clone();
            assert_eq!(doc.text, "edited", "{name}");
            assert!(!doc.closed, "{name}");
            assert!(!doc.frozen, "{name}");
        }
        assert!(ide.has(&input("a").key));
    }

    #[test]
    fn retains_a_tab_that_becomes_dirty_while_a_tab_limit_close_drains_execution() {
        let mut ide = Ide::new();
        ide.prefs.tab_limit = 1;
        ide.open("a").expect("opens");
        let doc = ide.doc("a");
        doc.text.clone_from(&doc.saved);
        doc.running = true;
        ide.open_marker("other", background());
        let doc = ide.doc("a").clone();
        assert_eq!(doc.text, "original\nfinal output");
        assert_eq!(doc.saved, "original");
        assert!(!doc.closed);
        assert!(!doc.frozen);
        assert!(ide.has(&input("a").key));
        assert_eq!(ide.asked, 0);
    }

    #[test]
    fn attempts_remaining_saves_when_a_lifecycle_callback_throws_synchronously() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        ide.open("b").expect("opens");
        ide.doc("a").save_throws = Some("session unavailable".into());
        let failed = ide.save_all().map_err(|failures| failures.message().to_owned());
        assert_eq!(failed, Err("session unavailable".to_owned()));
        assert_eq!(ide.doc("a").saved, "original");
        assert_eq!(ide.doc("a").text, "edited");
        assert_eq!(ide.doc("b").saved, "edited");
    }

    #[test]
    fn releases_an_authorized_decision_to_leave_when_the_project_never_changed() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let session = ide.prepare_close().expect("a session");
        ide.guard.commit_session(session.id).expect("the session commits");
        assert!(ide.doc("a").frozen);
        // The window stayed with its project: files open again, and it can be left again.
        ide.open("b").expect("opens");
        assert!(ide.has(&input("b").key));
        assert!(!ide.doc("a").frozen);
        let again = ide.prepare_close().expect("another session");
        assert_eq!(ide.guard.commit_session(session.id), Err(EditorError::NotCurrent));
        ide.cancel_session(again.id);
    }

    #[test]
    fn keeps_an_authorized_decision_while_the_project_it_leaves_for_is_opening() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let session = ide.prepare_close().expect("a session");
        ide.guard.commit_session(session.id).expect("the session commits");
        ide.guard.set_opening_project(true);
        assert_eq!(ide.open("b"), Err(EditorError::Closing));
        assert_eq!(EditorError::Closing.message(), "The project is closing");
        assert_eq!(ide.prepare_close(), Err(EditorError::AlreadyPending));
        ide.guard.set_opening_project(false);
        ide.apply_resumed();
        assert!(!ide.guard.close_pending());
    }

    #[test]
    fn rejects_authorization_from_a_cancelled_project_close_session() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        let session = ide.prepare_close().expect("a session");
        ide.cancel_session(session.id);
        assert_eq!(ide.guard.commit_session(session.id), Err(EditorError::NotCurrent));
        assert!(!ide.doc("a").frozen);
        assert_eq!(ide.doc("a").text, "edited");
        assert!(ide.has(&input("a").key));
    }

    #[test]
    fn does_not_return_an_authorized_session_after_preparation_was_cancelled_in_flight() {
        let mut ide = Ide::new();
        ide.open("a").expect("opens");
        assert_eq!(ide.guard.request_close(), Some(Job::PrepareClose));
        let dirty = dirty_of(&ide.docs);
        let (preparation, step) = ide.guard.start_preparation(&ide.tabs, &ide.kinds, &dirty);
        let mut preparation = preparation.expect("a preparation");
        assert_eq!(step, PrepareStep::Prepare(input("a")));
        ide.doc("a").freeze();
        ide.cancel_close();
        let drained = ide.doc("a").drain();
        let dirty = dirty_of(&ide.docs);
        let step = ide.guard.document_prepared(&mut preparation, drained, &dirty);
        assert_eq!(step, PrepareStep::Failed(EditorError::Cancelled));
        ide.after_job();
        let doc = ide.doc("a").clone();
        assert!(!doc.frozen);
        assert_eq!(doc.text, "edited");
        assert!(!doc.closed);
        assert!(ide.has(&input("a").key));
    }
}
