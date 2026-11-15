//! What the HTTP Client of a project keeps: the environment chosen per request file and the ones
//! each file offers, the response history (newest first) with the exchanges loaded so far, the
//! selected entry and the request being sent.
//!
//! The store does no I/O: the window loads environments and history, sends requests and hands the
//! answers back. Exchanges carry the request as sent, headers included, so nothing here derives
//! `Debug`: an authorization value never reaches a log through it.

use ide_http::{HistoryEntry, HttpExchange};
use std::collections::{BTreeMap, HashMap};

/// The Services view the responses are shown in.
pub const SERVICE_VIEW: &str = "httpClient";

/// UI storage key of the environment chosen per request file (a JSON object, path to name).
pub const ENV_KEY: &str = "openidea.httpClient.environments";

/// The request being sent: from which file (`None` for a buffer without one), which of its
/// requests (0-based) and the window's number for it, which Stop cancels.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Running<'a> {
    pub path: Option<&'a str>,
    pub index: u32,
    pub request: u64,
}

struct RunningRequest {
    path: Option<String>,
    index: u32,
    request: u64,
}

/// The HTTP Client state of one project.
#[derive(Default)]
pub struct HttpClientStore {
    env_by_file: BTreeMap<String, String>,
    environments: HashMap<String, Vec<String>>,
    history: Vec<HistoryEntry>,
    exchanges: HashMap<String, HttpExchange>,
    selected: Option<String>,
    running: Option<RunningRequest>,
    requests: u64,
}

impl HttpClientStore {
    /// A store with the environment choices kept in UI storage (`stored`, the value of [`ENV_KEY`]);
    /// unreadable choices are dropped.
    pub fn new(stored: Option<&str>) -> Self {
        let env_by_file = stored.and_then(|raw| serde_json::from_str(raw).ok()).unwrap_or_default();
        Self { env_by_file, ..Self::default() }
    }

    /// The environment choices as they are kept in UI storage.
    pub fn stored_choices(&self) -> String {
        serde_json::to_string(&self.env_by_file).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Chooses `env` for the request file `path` (`None`: no environment). Returns the choices to
    /// keep in UI storage.
    pub fn set_environment(&mut self, path: &str, env: Option<&str>) -> String {
        match env {
            Some(env) if !env.is_empty() => {
                self.env_by_file.insert(path.to_owned(), env.to_owned());
            }
            _ => {
                self.env_by_file.remove(path);
            }
        }
        self.stored_choices()
    }

    /// The environments the files of `path` declare, as last loaded.
    pub fn set_environments(&mut self, path: &str, names: Vec<String>) {
        self.environments.insert(path.to_owned(), names);
    }

    /// The environments of `path`, `None` until they were loaded.
    pub fn environments(&self, path: &str) -> Option<&[String]> {
        self.environments.get(path).map(Vec::as_slice)
    }

    /// The environment chosen for `path`, whether or not it still exists.
    pub fn chosen(&self, path: &str) -> Option<&str> {
        self.env_by_file.get(path).map(String::as_str)
    }

    /// The environment a run of `path` uses: the chosen one while it still exists (or while the
    /// environments are not loaded yet).
    pub fn effective_environment(&self, path: Option<&str>) -> Option<String> {
        let path = path?;
        let chosen = self.env_by_file.get(path)?;
        match self.environments.get(path) {
            Some(available) if !available.contains(chosen) => None,
            _ => Some(chosen.clone()),
        }
    }

    /// The environment the toolbar shows for `path`: the chosen one when the file offers it.
    pub fn shown_environment(&self, path: &str) -> Option<&str> {
        let chosen = self.chosen(path)?;
        self.environments(path).is_some_and(|names| names.iter().any(|name| name == chosen)).then_some(chosen)
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// The history as read from the project. An entry whose exchange is held here but that the read
    /// missed (answered while it was under way) stays first.
    pub fn set_history(&mut self, history: Vec<HistoryEntry>) {
        let kept: Vec<HistoryEntry> = self
            .history
            .iter()
            .filter(|entry| self.exchanges.contains_key(&entry.id) && !history.iter().any(|read| read.id == entry.id))
            .cloned()
            .collect();
        self.history = kept.into_iter().chain(history).collect();
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The exchange of the selected entry, once loaded.
    pub fn selected_exchange(&self) -> Option<&HttpExchange> {
        self.exchanges.get(self.selected.as_deref()?)
    }

    /// Selects history entry `id`. Returns whether its exchange still has to be loaded.
    pub fn select(&mut self, id: &str) -> bool {
        self.selected = Some(id.to_owned());
        !self.exchanges.contains_key(id)
    }

    /// The exchange of entry `id` was loaded.
    pub fn exchange_loaded(&mut self, id: &str, exchange: HttpExchange) {
        self.exchanges.insert(id.to_owned(), exchange);
    }

    /// The history was cleared: nothing is listed, loaded or selected.
    pub fn clear_history(&mut self) {
        self.history.clear();
        self.exchanges.clear();
        self.selected = None;
    }

    pub fn running(&self) -> Option<Running<'_>> {
        self.running.as_ref().map(|r| Running { path: r.path.as_deref(), index: r.index, request: r.request })
    }

    /// Whether a request of `path` is being sent.
    pub fn is_running_in(&self, path: Option<&str>) -> bool {
        self.running.as_ref().is_some_and(|r| r.path.as_deref() == path)
    }

    /// Starts sending request `index` of `path`. Returns its number and the number of the request
    /// it replaces, which is to be cancelled.
    pub fn begin_run(&mut self, path: Option<&str>, index: u32) -> (u64, Option<u64>) {
        let busy = self.running.as_ref().map(|r| r.request);
        self.requests += 1;
        let request = self.requests;
        self.running = Some(RunningRequest { path: path.map(str::to_owned), index, request });
        (request, busy)
    }

    /// Request `request` answered: its entry goes first in the history (once) and is selected.
    pub fn run_answered(&mut self, request: u64, exchange: HttpExchange) {
        let id = exchange.entry.id.clone();
        self.history.retain(|entry| entry.id != id);
        self.history.insert(0, exchange.entry.clone());
        self.exchanges.insert(id.clone(), exchange);
        self.selected = Some(id);
        self.run_ended(request);
    }

    /// Request `request` ended (answered, failed or cancelled): it no longer runs, unless another
    /// request took its place meanwhile.
    pub fn run_ended(&mut self, request: u64) {
        if self.running.as_ref().is_some_and(|r| r.request == request) {
            self.running = None;
        }
    }

    /// The window switched projects: the history, responses, environments and the running request
    /// belonged to the previous one. Returns the request to cancel.
    pub fn reset(&mut self) -> Option<u64> {
        let running = self.running.take().map(|r| r.request);
        self.environments.clear();
        self.clear_history();
        running
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use ide_http::PreparedRequest;

    pub(crate) fn exchange(id: &str, status: Option<u16>) -> HttpExchange {
        HttpExchange {
            entry: HistoryEntry {
                id: id.to_owned(),
                time_ms: 0.0,
                name: None,
                method: "GET".to_owned(),
                url: format!("http://127.0.0.1/{id}"),
                status,
                total_ms: Some(1.0),
                file: None,
                error: None,
            },
            request: PreparedRequest {
                name: None,
                method: "GET".to_owned(),
                url: format!("http://127.0.0.1/{id}"),
                headers: Vec::new(),
                body: None,
            },
            response: None,
        }
    }

    /// A choice survives in UI storage, is dropped by choosing no environment, and unreadable stored
    /// choices are ignored.
    #[test]
    fn keeps_the_chosen_environment_per_file_in_ui_storage() {
        let mut store = HttpClientStore::new(Some("not json"));
        assert_eq!(store.chosen("/p/a.http"), None);
        let stored = store.set_environment("/p/a.http", Some("dev"));
        let reopened = HttpClientStore::new(Some(&stored));
        assert_eq!(reopened.chosen("/p/a.http"), Some("dev"));
        let stored = store.set_environment("/p/a.http", None);
        assert_eq!(stored, "{}");
    }

    /// A run uses the chosen environment while the file still offers it, and none once it is gone.
    #[test]
    fn a_run_uses_the_chosen_environment_only_while_it_exists() {
        let mut store = HttpClientStore::new(None);
        store.set_environment("/p/a.http", Some("dev"));
        assert_eq!(store.effective_environment(Some("/p/a.http")).as_deref(), Some("dev"), "not loaded yet");
        assert_eq!(store.shown_environment("/p/a.http"), None);
        store.set_environments("/p/a.http", vec!["dev".to_owned(), "prod".to_owned()]);
        assert_eq!(store.effective_environment(Some("/p/a.http")).as_deref(), Some("dev"));
        assert_eq!(store.shown_environment("/p/a.http"), Some("dev"));
        store.set_environments("/p/a.http", vec!["prod".to_owned()]);
        assert_eq!(store.effective_environment(Some("/p/a.http")), None);
        assert_eq!(store.effective_environment(None), None, "a buffer without a file has none");
    }

    /// A new run replaces the busy one, which is cancelled; the replaced run ending leaves the new one
    /// running.
    #[test]
    fn a_new_run_replaces_the_busy_one() {
        let mut store = HttpClientStore::new(None);
        let (first, busy) = store.begin_run(Some("/p/a.http"), 0);
        assert_eq!(busy, None);
        let (second, busy) = store.begin_run(Some("/p/a.http"), 1);
        assert_eq!(busy, Some(first));
        store.run_ended(first);
        let running = store.running().map(|r| (r.index, r.request));
        assert_eq!(running, Some((1, second)));
        assert!(store.is_running_in(Some("/p/a.http")));
        assert!(!store.is_running_in(Some("/p/b.http")));
        store.run_answered(second, exchange("e1", Some(200)));
        assert!(store.running().is_none());
    }

    /// An answer goes first in the history once, is selected and needs no load; another entry does.
    #[test]
    fn an_answer_goes_first_in_the_history_and_is_selected() {
        let mut store = HttpClientStore::new(None);
        store.set_history(vec![exchange("old", Some(404)).entry, exchange("e1", Some(200)).entry]);
        let (request, _) = store.begin_run(None, 0);
        store.run_answered(request, exchange("e1", Some(201)));
        let ids: Vec<&str> = store.history().iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["e1", "old"]);
        assert_eq!(store.selected(), Some("e1"));
        assert_eq!(store.selected_exchange().and_then(|ex| ex.entry.status), Some(201));
        assert!(!store.select("e1"));
        assert!(store.select("old"));
        assert!(store.selected_exchange().is_none());
        store.exchange_loaded("old", exchange("old", Some(404)));
        assert_eq!(store.selected_exchange().and_then(|ex| ex.entry.status), Some(404));
    }

    /// A history read that missed an answer of this session keeps that answer first.
    #[test]
    fn a_history_read_keeps_an_answer_it_missed() {
        let mut store = HttpClientStore::new(None);
        let (request, _) = store.begin_run(None, 0);
        store.run_answered(request, exchange("new", Some(200)));
        store.set_history(vec![exchange("old", Some(200)).entry]);
        let ids: Vec<&str> = store.history().iter().map(|entry| entry.id.as_str()).collect();
        assert_eq!(ids, ["new", "old"]);
        store.set_history(vec![exchange("new", Some(200)).entry, exchange("old", Some(200)).entry]);
        assert_eq!(store.history().len(), 2, "listed once");
    }

    /// Clearing the history and switching projects leave nothing listed, loaded or selected; the
    /// switch also gives back the running request to cancel and forgets the environments.
    #[test]
    fn clearing_and_switching_projects_forget_the_responses() {
        let mut store = HttpClientStore::new(None);
        store.set_environments("/p/a.http", vec!["dev".to_owned()]);
        let (request, _) = store.begin_run(None, 0);
        store.run_answered(request, exchange("e1", Some(200)));
        store.clear_history();
        assert!(store.history().is_empty() && store.selected().is_none() && store.selected_exchange().is_none());
        let (request, _) = store.begin_run(Some("/p/a.http"), 2);
        assert_eq!(store.reset(), Some(request));
        assert!(store.running().is_none());
        assert_eq!(store.environments("/p/a.http"), None);
    }
}
