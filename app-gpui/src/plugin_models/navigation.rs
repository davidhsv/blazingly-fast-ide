//! Background indexing for plugins' document indexers (navigation between files that no language
//! server links, such as a mapper XML and its Java interface). Opening a trusted project lists the
//! files the indexers own once; edits, open editors and file events then queue only the paths they
//! touch, indexed in small batches.
//!
//! [`NavigationIndexer`] calls nothing: every method returns the [`NavEffect`]s to carry out (backend
//! calls with a [`NavTicket`], timers, marker and provider invalidation, a notification) and the
//! answers come back through the `*_answered` methods. Request ids are passed in. A reset (another
//! project, reloaded indexers) starts a new generation, and answers of an older one are dropped.
//!
//! Usages and rename first bring the indexes of the file they start from up to date:
//! [`NavigationIndexer::begin_sync`] and [`NavigationIndexer::poll_sync`].

use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_core::error::ErrorKind;
use ide_core::{IdeError, RequestId};
use ide_host::plugins::{PluginInfo, PluginStatus};
use std::collections::{HashMap, HashSet};

/// How many paths one index call takes.
pub const BATCH: usize = 8;
/// Delay before a batch of queued paths is indexed.
pub const QUEUE_DELAY_MS: u64 = 30;
/// Delay for the paths of open editors, so typing coalesces.
pub const PRIORITY_DELAY_MS: u64 = 150;
/// Delay before the next batch while paths remain queued.
pub const NEXT_BATCH_DELAY_MS: u64 = 20;
/// Delay before an incomplete file listing is asked for again.
pub const DISCOVERY_RETRY_MS: u64 = 1000;

const INDEXERS_POINT: &str = "documentIndexers";

/// Identifies a backend call; an answer whose generation is not current is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NavTicket {
    pub generation: u64,
    pub seq: u64,
}

/// What the caller does for the indexer.
#[derive(Clone, Debug, PartialEq)]
pub enum NavEffect {
    /// `plugin_navigation_files`; answer with [`NavigationIndexer::files_answered`].
    ListFiles(NavTicket),
    /// `plugin_navigation_invalidate(None)` (every plugin index); answer with
    /// [`NavigationIndexer::invalidated`].
    InvalidateAll(NavTicket),
    /// Bring the open editors of `paths` in sync with the backend, then
    /// `plugin_navigation_invalidate(paths)` and `plugin_navigation_index(paths, request)`;
    /// answer with [`NavigationIndexer::index_answered`].
    Index {
        ticket: NavTicket,
        paths: Vec<String>,
        request: RequestId,
    },
    /// `cancel_request`.
    Cancel(RequestId),
    /// Call [`NavigationIndexer::flush`] after this many milliseconds (replacing a pending call).
    ScheduleFlush(u64),
    /// Call [`NavigationIndexer::discover`] after this many milliseconds.
    ScheduleDiscovery(u64),
    /// Drop the gutter markers of these files and of the markers pointing into them; `None`: all.
    InvalidateMarkers(Option<Vec<String>>),
    /// Navigation providers must be asked again (editors, markers).
    ProvidersChanged,
    Notify(NotificationRequest),
}

/// Where a usages or rename request stands with the indexes of its file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SyncStatus {
    /// The indexes are current, or the file is none of the indexers' business.
    Done,
    /// They cannot be brought up to date now.
    Failed(String),
    /// Poll again after the next answer.
    Waiting,
}

/// A usages or rename request waiting for the indexes of `path`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncToken {
    generation: u64,
    path: String,
}

#[derive(Clone, Debug, PartialEq)]
struct Batch {
    ticket: NavTicket,
    paths: Vec<String>,
    request: RequestId,
}

/// The project as the indexer needs it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavProject {
    pub root: String,
    pub trusted: bool,
}

#[derive(Debug, Default)]
pub struct NavigationIndexer {
    generation: u64,
    seq: u64,
    disposed: bool,
    project: Option<NavProject>,
    /// Some plugin with an indexer has its contributions live (active or inactive code).
    indexers: bool,
    signature: String,
    batch: Option<Batch>,
    /// Queued paths, in the order they are indexed.
    pending: Vec<String>,
    /// Paths whose indexes are not current yet.
    dirty: HashSet<String>,
    /// Open editors (by the caller's id) and their files.
    views: HashMap<u64, String>,
    discovered: bool,
    discovering: Option<NavTicket>,
    known: HashSet<String>,
    reported_error: bool,
    extensions: HashSet<String>,
}

/// Active and inactive plugins keep their indexes; a static plugin has no code to index with.
fn indexes(status: PluginStatus) -> bool {
    matches!(status, PluginStatus::Active | PluginStatus::Inactive)
}

/// What decides whether the indexes are still valid: each plugin's version, whether it can index,
/// and what it contributes. An idle instance being reclaimed (active → inactive) changes nothing.
fn signature(plugins: &[PluginInfo]) -> String {
    plugins
        .iter()
        .map(|p| {
            let status = if indexes(p.status) { "live".to_owned() } else { format!("{:?}", p.status) };
            let contributions: Vec<String> =
                p.contributions.iter().map(|c| format!("{}={}", c.point, c.label)).collect();
            format!("{}:{}:{status}:{}", p.id, p.version, contributions.join(","))
        })
        .collect::<Vec<_>>()
        .join("|")
}

impl NavigationIndexer {
    /// An indexer for the window's project and plugins; returns the first file listing to ask for.
    pub fn new(project: Option<NavProject>, plugins: &[PluginInfo]) -> (Self, Vec<NavEffect>) {
        let mut indexer = Self { project, signature: signature(plugins), ..Self::default() };
        indexer.indexers = Self::has_indexers(plugins);
        let effects = indexer.discover();
        (indexer, effects)
    }

    fn has_indexers(plugins: &[PluginInfo]) -> bool {
        plugins.iter().any(|p| indexes(p.status) && p.contributions.iter().any(|c| c.point == INDEXERS_POINT))
    }

    fn ready(&self) -> bool {
        !self.disposed && self.project.as_ref().is_some_and(|p| p.trusted) && self.indexers
    }

    fn eligible(&self, path: &str) -> bool {
        self.extensions.contains(path.rsplit('.').next().unwrap_or_default())
    }

    fn ticket(&mut self) -> NavTicket {
        self.seq += 1;
        NavTicket { generation: self.generation, seq: self.seq }
    }

    /// Paths whose indexes are not current: their markers are hidden and navigation from them waits.
    pub fn is_dirty(&self, path: &str) -> bool {
        self.dirty.contains(path)
    }

    /// Whether a navigation request in `path` goes to the plugins' indexes.
    pub fn navigates(&self, path: &str) -> bool {
        self.ready() && self.eligible(path) && !self.dirty.contains(path)
    }

    fn report(&mut self, error: &IdeError) -> Vec<NavEffect> {
        tracing::warn!("plugin navigation: {}", error.message);
        if self.reported_error {
            return Vec::new();
        }
        self.reported_error = true;
        let request = NotificationRequest {
            group: Some("plugins".to_owned()),
            body: Some(error.message.clone()),
            ..NotificationRequest::new(NotificationLevel::Warning, "Plugin navigation indexing incomplete")
        };
        vec![NavEffect::Notify(request)]
    }

    /// Queues `paths` that an indexer owns; open editors' paths go first.
    pub fn queue(&mut self, paths: &[String], priority: bool) -> Vec<NavEffect> {
        let matching: Vec<String> = paths.iter().filter(|p| self.eligible(p)).cloned().collect();
        if matching.is_empty() || !self.ready() {
            return Vec::new();
        }
        self.dirty.extend(matching.iter().cloned());
        let rest = std::mem::take(&mut self.pending);
        let ordered = if priority { matching.iter().chain(&rest) } else { rest.iter().chain(&matching) };
        for path in ordered {
            if !self.pending.contains(path) {
                self.pending.push(path.clone());
            }
        }
        vec![
            NavEffect::InvalidateMarkers(Some(matching)),
            NavEffect::ProvidersChanged,
            NavEffect::ScheduleFlush(if priority { PRIORITY_DELAY_MS } else { QUEUE_DELAY_MS }),
        ]
    }

    /// Indexes the next batch of queued paths, unless one is in flight.
    pub fn flush(&mut self, request: RequestId) -> Vec<NavEffect> {
        if self.batch.is_some() || !self.ready() || self.pending.is_empty() {
            return Vec::new();
        }
        let paths: Vec<String> = self.pending.drain(..self.pending.len().min(BATCH)).collect();
        let ticket = self.ticket();
        self.batch = Some(Batch { ticket, paths: paths.clone(), request });
        vec![NavEffect::Index { ticket, paths, request }]
    }

    /// The answer to [`NavEffect::Index`].
    pub fn index_answered(&mut self, ticket: NavTicket, result: Result<(), IdeError>) -> Vec<NavEffect> {
        let Some(batch) = self.batch.take_if(|batch| batch.ticket == ticket) else { return Vec::new() };
        let mut effects = Vec::new();
        if ticket.generation == self.generation {
            match result {
                Ok(()) => {
                    for path in &batch.paths {
                        if !self.pending.contains(path) {
                            self.dirty.remove(path);
                        }
                    }
                    effects.push(NavEffect::ProvidersChanged);
                }
                Err(error) if error.kind == ErrorKind::Cancelled => {
                    for path in batch.paths {
                        if !self.pending.contains(&path) {
                            self.pending.push(path);
                        }
                    }
                }
                Err(error) => effects.extend(self.report(&error)),
            }
        }
        if !self.pending.is_empty() && self.ready() {
            effects.push(NavEffect::ScheduleFlush(NEXT_BATCH_DELAY_MS));
        }
        effects
    }

    /// Lists the files the indexers own, once per generation (again while the listing was
    /// incomplete).
    pub fn discover(&mut self) -> Vec<NavEffect> {
        if !self.ready() || self.discovered || self.discovering.is_some() {
            return Vec::new();
        }
        let ticket = self.ticket();
        self.discovering = Some(ticket);
        vec![NavEffect::ListFiles(ticket)]
    }

    /// The answer to [`NavEffect::ListFiles`]: every known path is queued once, open editors first.
    pub fn files_answered(
        &mut self,
        ticket: NavTicket,
        result: Result<ide_host::plugins::index_ops::NavigationFiles, IdeError>,
    ) -> Vec<NavEffect> {
        if ticket.generation != self.generation || self.discovering != Some(ticket) {
            return Vec::new();
        }
        self.discovering = None;
        let mut effects = Vec::new();
        match result {
            Ok(inventory) => {
                self.extensions = inventory.extensions.into_iter().collect();
                self.discovered = !inventory.indexing;
                let paths: Vec<String> = inventory.paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
                let fresh: Vec<String> = paths.iter().filter(|p| !self.known.contains(*p)).cloned().collect();
                self.known.extend(paths);
                effects.extend(self.queue(&fresh, false));
                let open: Vec<String> = self.views.values().cloned().collect();
                effects.extend(self.queue(&open, true));
            }
            Err(error) => {
                self.discovered = false;
                effects.extend(self.report(&error));
            }
        }
        if !self.discovered && self.ready() {
            effects.push(NavEffect::ScheduleDiscovery(DISCOVERY_RETRY_MS));
        }
        effects
    }

    /// The answer to [`NavEffect::InvalidateAll`]: the files are listed again.
    pub fn invalidated(&mut self, ticket: NavTicket) -> Vec<NavEffect> {
        if ticket.generation != self.generation { Vec::new() } else { self.discover() }
    }

    /// Forgets every index and queued path and starts a new generation.
    pub fn reset(&mut self) -> Vec<NavEffect> {
        let mut effects = vec![NavEffect::InvalidateMarkers(None)];
        self.generation += 1;
        if let Some(batch) = self.batch.take() {
            effects.push(NavEffect::Cancel(batch.request));
        }
        self.pending.clear();
        self.dirty.clear();
        self.known.clear();
        self.discovered = false;
        self.discovering = None;
        self.reported_error = false;
        effects.push(NavEffect::ProvidersChanged);
        if self.ready() {
            let ticket = self.ticket();
            effects.push(NavEffect::InvalidateAll(ticket));
        }
        effects
    }

    /// The window's project changed (opened, closed, trusted).
    pub fn set_project(&mut self, project: Option<NavProject>) -> Vec<NavEffect> {
        if project == self.project {
            return Vec::new();
        }
        self.project = project;
        self.reset()
    }

    /// A listing of the plugins arrived; the indexes are reset only when an indexer changed.
    pub fn set_plugins(&mut self, plugins: &[PluginInfo]) -> Vec<NavEffect> {
        self.indexers = Self::has_indexers(plugins);
        let next = signature(plugins);
        if next == self.signature {
            return Vec::new();
        }
        self.signature = next;
        self.reset()
    }

    /// The host dropped the documents' indexes (reload, disable, failure).
    pub fn document_indexes_changed(&mut self) -> Vec<NavEffect> {
        self.reset()
    }

    /// The project's roots changed.
    pub fn project_roots_changed(&mut self) -> Vec<NavEffect> {
        self.reset()
    }

    /// The project's file index finished: an incomplete listing is asked for again.
    pub fn index_state(&mut self, indexing: bool) -> Vec<NavEffect> {
        if indexing { Vec::new() } else { self.discover() }
    }

    /// Files were created, modified or removed.
    pub fn files_changed(&mut self, paths: &[String]) -> Vec<NavEffect> {
        self.queue(paths, false)
    }

    /// An editor of a real file opened.
    pub fn editor_opened(&mut self, view: u64, path: &str) -> Vec<NavEffect> {
        self.views.insert(view, path.to_owned());
        self.queue(&[path.to_owned()], true)
    }

    /// The text of an open editor changed.
    pub fn editor_changed(&mut self, view: u64) -> Vec<NavEffect> {
        let Some(path) = self.views.get(&view).cloned() else { return Vec::new() };
        self.queue(&[path], true)
    }

    /// An editor closed: its file is indexed from disk again.
    pub fn editor_closed(&mut self, view: u64) -> Vec<NavEffect> {
        let Some(path) = self.views.remove(&view) else { return Vec::new() };
        self.queue(&[path], false)
    }

    /// Stops: forgets everything and cancels the batch in flight.
    pub fn dispose(&mut self) -> Vec<NavEffect> {
        self.disposed = true;
        let effects = self.reset();
        self.views.clear();
        effects
    }

    /// Starts bringing the indexes of `path` up to date before usages or rename run from it.
    pub fn begin_sync(
        &mut self,
        path: &str,
        request: RequestId,
        i18n: &I18n,
    ) -> (SyncToken, SyncStatus, Vec<NavEffect>) {
        let token = SyncToken { generation: self.generation, path: path.to_owned() };
        let under_root = self.project.as_ref().is_some_and(|project| {
            let root = if project.root.ends_with('/') { project.root.clone() } else { format!("{}/", project.root) };
            path.starts_with(&root)
        });
        if !self.ready() || !under_root {
            return (token, SyncStatus::Done, Vec::new());
        }
        let mut effects = self.discover();
        let (status, more) = self.poll_sync(&token, request, i18n);
        effects.extend(more);
        (token, status, effects)
    }

    /// Where the request of `token` stands; flushes the queue at once (without the debounce) while
    /// paths wait. Call again after each answer while it is [`SyncStatus::Waiting`].
    pub fn poll_sync(&mut self, token: &SyncToken, request: RequestId, i18n: &I18n) -> (SyncStatus, Vec<NavEffect>) {
        if !self.ready() {
            return (SyncStatus::Done, Vec::new());
        }
        if self.discovering.is_some() {
            return (SyncStatus::Waiting, Vec::new());
        }
        if !self.eligible(&token.path) {
            return (SyncStatus::Done, Vec::new());
        }
        let current = token.generation == self.generation;
        if current && self.batch.is_some() {
            return (SyncStatus::Waiting, Vec::new());
        }
        if current && !self.pending.is_empty() {
            return (SyncStatus::Waiting, self.flush(request));
        }
        if !current || !self.discovered || !self.dirty.is_empty() {
            let message = i18n.tx("Plugin indexes are still updating; try again shortly");
            return (SyncStatus::Failed(message), Vec::new());
        }
        (SyncStatus::Done, Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_host::plugins::index_ops::NavigationFiles;
    use serde_json::json;

    fn plugins(status: &str) -> Vec<PluginInfo> {
        let contributions = json!([{ "point": "documentIndexers", "label": "xml, java" }]);
        let fields = json!({ "status": status, "contributions": contributions });
        vec![crate::plugin_models::test_support::plugin("test", fields)]
    }

    fn project() -> Option<NavProject> {
        Some(NavProject { root: "/project".into(), trusted: true })
    }

    fn inventory() -> NavigationFiles {
        NavigationFiles {
            paths: vec!["/project/A.java".into(), "/project/A.xml".into()],
            indexing: false,
            extensions: vec!["xml".into(), "java".into()],
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn list_ticket(effects: &[NavEffect]) -> Option<NavTicket> {
        effects.iter().find_map(|e| if let NavEffect::ListFiles(t) = e { Some(*t) } else { None })
    }

    fn index(effects: &[NavEffect]) -> Option<(NavTicket, Vec<String>)> {
        effects.iter().find_map(|e| match e {
            NavEffect::Index { ticket, paths, .. } => Some((*ticket, paths.clone())),
            _ => None,
        })
    }

    /// A started indexer whose first listing was answered and first batch indexed.
    fn warmed() -> NavigationIndexer {
        let (mut indexer, effects) = NavigationIndexer::new(project(), &plugins("active"));
        let ticket = list_ticket(&effects).expect("the files are listed");
        indexer.files_answered(ticket, Ok(inventory()));
        let (ticket, paths) = index(&indexer.flush(RequestId(1))).expect("a batch");
        assert_eq!(paths, strings(&["/project/A.java", "/project/A.xml"]));
        indexer.index_answered(ticket, Ok(()));
        indexer
    }

    #[test]
    fn prewarms_once_and_indexes_only_changed_paths_after_file_events() {
        let mut indexer = warmed();
        let queued = indexer.files_changed(&strings(&["/project/A.xml"]));
        assert!(queued.contains(&NavEffect::ScheduleFlush(QUEUE_DELAY_MS)));
        let (_, paths) = index(&indexer.flush(RequestId(2))).expect("a batch");
        assert_eq!(paths, strings(&["/project/A.xml"]));
        assert_eq!(list_ticket(&indexer.index_state(false)), None, "a complete listing is not asked for again");
    }

    #[test]
    fn indexes_unsaved_editor_changes_and_refreshes_after_same_version_plugin_reload() {
        let mut indexer = warmed();
        indexer.editor_opened(7, "/project/A.xml");
        let (ticket, _) = index(&indexer.flush(RequestId(2))).expect("a batch");
        indexer.index_answered(ticket, Ok(()));
        assert!(indexer.editor_changed(7).contains(&NavEffect::ScheduleFlush(PRIORITY_DELAY_MS)));
        let (_, paths) = index(&indexer.flush(RequestId(3))).expect("a batch");
        assert_eq!(paths, strings(&["/project/A.xml"]));
        let reset = indexer.document_indexes_changed();
        let invalidate = reset
            .iter()
            .find_map(|e| if let NavEffect::InvalidateAll(t) = e { Some(*t) } else { None })
            .expect("every index is invalidated");
        assert!(list_ticket(&indexer.invalidated(invalidate)).is_some(), "the files are listed again");
    }

    #[test]
    fn cancels_outstanding_work_when_project_context_changes() {
        let (mut indexer, effects) = NavigationIndexer::new(project(), &plugins("active"));
        indexer.files_answered(list_ticket(&effects).expect("a listing"), Ok(inventory()));
        index(&indexer.flush(RequestId(9))).expect("a batch");
        let effects = indexer.set_project(None);
        assert_eq!(effects.iter().filter(|e| **e == NavEffect::Cancel(RequestId(9))).count(), 1);
    }

    #[test]
    fn keeps_warm_indexes_when_api_1_0_reclaims_an_idle_wasm_instance() {
        let mut indexer = warmed();
        assert_eq!(indexer.set_plugins(&plugins("inactive")), Vec::new());
        assert_eq!(indexer.flush(RequestId(2)), Vec::new());
        assert_eq!(list_ticket(&indexer.discover()), None);
    }

    #[test]
    fn flushes_pending_changes_before_usages_or_rename_without_waiting_for_the_debounce() {
        let mut indexer = warmed();
        indexer.files_changed(&strings(&["/project/A.xml"]));
        let (token, status, effects) = indexer.begin_sync("/project/A.xml", RequestId(5), &I18n::default());
        assert_eq!(status, SyncStatus::Waiting);
        let batches: Vec<_> = effects.iter().filter_map(|e| index(std::slice::from_ref(e))).collect();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].1, strings(&["/project/A.xml"]));
        indexer.index_answered(batches[0].0, Ok(()));
        assert_eq!(indexer.poll_sync(&token, RequestId(6), &I18n::default()).0, SyncStatus::Done);
    }

    #[test]
    fn waits_for_an_active_batch_then_indexes_changes_queued_while_it_was_running() {
        let mut indexer = warmed();
        indexer.files_changed(&strings(&["/project/A.xml"]));
        let (running, _) = index(&indexer.flush(RequestId(2))).expect("a batch");
        indexer.files_changed(&strings(&["/project/A.java"]));
        let (token, status, effects) = indexer.begin_sync("/project/A.xml", RequestId(3), &I18n::default());
        assert_eq!((status, index(&effects)), (SyncStatus::Waiting, None));
        indexer.index_answered(running, Ok(()));
        let (status, effects) = indexer.poll_sync(&token, RequestId(4), &I18n::default());
        assert_eq!(status, SyncStatus::Waiting);
        let (last, paths) = index(&effects).expect("the queued change");
        assert_eq!(paths, strings(&["/project/A.java"]));
        indexer.index_answered(last, Ok(()));
        assert_eq!(indexer.poll_sync(&token, RequestId(5), &I18n::default()).0, SyncStatus::Done);
    }

    #[test]
    fn refuses_refactoring_if_a_queued_index_batch_failed() {
        let mut indexer = warmed();
        indexer.files_changed(&strings(&["/project/A.xml"]));
        let (token, _, effects) = indexer.begin_sync("/project/A.xml", RequestId(3), &I18n::default());
        let (ticket, _) = index(&effects).expect("a batch");
        let answered = indexer.index_answered(ticket, Err(IdeError::unavailable("indexer failed")));
        assert!(answered.iter().any(|e| matches!(e, NavEffect::Notify(_))));
        let (status, _) = indexer.poll_sync(&token, RequestId(4), &I18n::default());
        let SyncStatus::Failed(message) = status else { panic!("refused: {status:?}") };
        assert!(message.contains("Plugin indexes are still updating"));
    }

    #[test]
    fn does_not_block_unowned_path_while_a_plugin_batch_is_running() {
        for path in ["/project/notes.txt", "/external/Other.xml", "jdt://contents/Other.xml"] {
            let mut indexer = warmed();
            indexer.files_changed(&strings(&["/project/A.xml"]));
            index(&indexer.flush(RequestId(2))).expect("a batch");
            assert_eq!(indexer.begin_sync(path, RequestId(3), &I18n::default()).1, SyncStatus::Done, "{path}");
        }
    }

    #[test]
    fn keeps_unrelated_language_server_queries_available_when_plugin_discovery_fails() {
        let (mut indexer, effects) = NavigationIndexer::new(project(), &plugins("active"));
        let failed = indexer.files_answered(
            list_ticket(&effects).expect("a listing"),
            Err(IdeError::unavailable("Plugin file inventory is unavailable")),
        );
        assert!(failed.contains(&NavEffect::ScheduleDiscovery(DISCOVERY_RETRY_MS)));
        let (token, status, effects) = indexer.begin_sync("/project/notes.txt", RequestId(2), &I18n::default());
        assert_eq!(status, SyncStatus::Waiting, "the files are listed again first");
        let again = list_ticket(&effects).expect("a listing");
        indexer.files_answered(again, Err(IdeError::unavailable("Plugin file inventory is unavailable")));
        let (status, effects) = indexer.poll_sync(&token, RequestId(3), &I18n::default());
        assert_eq!(status, SyncStatus::Done);
        assert_eq!(index(&effects), None);
    }
}
