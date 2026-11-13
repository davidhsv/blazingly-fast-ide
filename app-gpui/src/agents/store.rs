//! The agent tasks of the open project, mirrored from `ide-agent` through its event subscription: the
//! tasks and their unread state, the conversations opened in this window, the composers, the New Task
//! settings, and where a conversation is shown (an editor tab or docked in the Agents tool window).
//!
//! [`AgentsStore`] does no I/O. A method that needs the backend, a dialog, a timer, storage or the
//! workbench returns [`AgentEffect`]s for the caller to carry out; answers come back through the
//! `*_answered` / `*_loaded` methods with the [`Ticket`] of the request. An answer of a subscription
//! that was replaced (the project changed) is dropped. Time is passed in.
//!
//! Editing a sent message is a step machine ([`AgentsStore::rerun_from`], [`AgentsStore::retract_from`]):
//! confirm (for the first turn), stop a running turn and wait until it has stopped (or give up after
//! [`STOP_TIMEOUT_MS`]), roll the conversation back, then send. Whatever fails, the message is put back
//! in the composer.

use crate::agents::composer::{ComposerDraft, FileRef, bypass_defaults, can_send, to_prompt};
use crate::agents::conversation::with_quote;
use crate::agents::model::{
    Attention, HOME_KEY, TOOL_WINDOW, attention_change, entries_key, is_busy, session_key, upsert_entry,
};
use crate::platform::i18n::I18n;
use crate::platform::paths::is_same_or_inside;
use crate::workbench::notifications::{NotificationAction, NotificationLevel, NotificationRequest};
use ide_agent::{
    AgentChange, AgentConfigChoice, AgentConfigOption, AgentEntry, AgentEvent, AgentImage, AgentLimit, AgentPrompt,
    AgentProvider, AgentQueued, AgentStatus, AgentTask, AgentToolStatus, Isolation, NewAgentTask,
};
use ide_core::IpcPath;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// Composer key of the New Task page; sessions use their task id.
pub const HOME: &str = "home";
pub const DRAFT_KEY: &str = "openidea.agents.draft.v2";
pub const HISTORY_KEY: &str = "openidea.agents.history.v1";
pub const SESSION_LOCATION_KEY: &str = "openidea.agents.sessionLocation.v1";
const HISTORY_SIZE: usize = 50;
/// How long changes wait after an edit before they are read again (edits come in bursts).
pub const CHANGES_DELAY_MS: f64 = 500.0;
/// How long an edit waits for a running turn to stop.
pub const STOP_TIMEOUT_MS: f64 = 15_000.0;
/// The command of a notification's Open / Review button; the task is the notification's key without
/// its `agent:` prefix.
pub const OPEN_TASK_COMMAND: &str = "agents.openTask";

/// New Task page settings (its text lives in the [`HOME`] composer).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    pub project_root: Option<String>,
    pub provider: String,
    /// Settings chosen for the next task, by option id.
    pub config: BTreeMap<String, String>,
    /// Thinking-level choices by model value for this provider.
    pub thought_levels: BTreeMap<String, BTreeMap<String, String>>,
    pub isolation: Isolation,
    /// The user picked `isolation`: otherwise a task runs in the local checkout.
    pub isolation_chosen: bool,
    /// `None`: the current branch.
    pub base: Option<String>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            project_root: None,
            provider: "claude".to_owned(),
            config: BTreeMap::new(),
            thought_levels: BTreeMap::new(),
            isolation: Isolation::Local,
            isolation_chosen: false,
            base: None,
        }
    }
}

/// The saved New Task settings (JSON as stored; anything unreadable gives the defaults). A worktree
/// is used only when the user picked it: drafts saved before that choice was recorded carried the old
/// default (`worktree`) instead.
pub fn saved_draft(saved: Option<&str>) -> Draft {
    let draft: Draft = saved.and_then(|json| serde_json::from_str(json).ok()).unwrap_or_default();
    if draft.isolation_chosen { draft } else { Draft { isolation: Isolation::Local, ..draft } }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SessionLocation {
    #[default]
    Editor,
    ToolWindow,
}

impl SessionLocation {
    fn stored(self) -> &'static str {
        match self {
            Self::Editor => "\"editor\"",
            Self::ToolWindow => "\"toolWindow\"",
        }
    }
}

/// An editor tab as the Agents feature sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentTab {
    Home,
    Session(String),
    Other,
}

/// The editor area: the active tab and every open one, most recently used first.
#[derive(Clone, Debug, Default)]
pub struct Editors {
    pub active: Option<AgentTab>,
    pub open: Vec<AgentTab>,
}

/// A request's identity: its answer is matched by it, and dropped when nothing waits for it any more.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ticket(pub u64);

/// A dialog asking to go ahead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmRequest {
    pub title: String,
    pub message: String,
    pub ok: String,
}

/// What a finished request came to: `Ok(true)` done, `Ok(false)` given up by the user (a dialog was
/// declined), `Err` the message to show.
pub type Outcome = Result<bool, String>;

/// The backend operations the feature calls.
#[derive(Clone, Debug, PartialEq)]
pub enum AgentCall {
    Prompt { task: String, prompt: AgentPrompt, now: bool },
    Cancel { task: String, subagent: Option<String> },
    Rollback { task: String, turns: u32 },
    Fork { task: String, turns: Option<u32> },
    Unqueue { task: String, queued: String },
    SendQueuedNow { task: String, queued: String },
    SetConfig { task: String, option: String, value: String },
    StopBackground { task: String, background: String },
    Respond { task: String, permission: u32, option: Option<String> },
    Archive { task: String },
    Unarchive { task: String },
    Rename { task: String, title: String },
    Delete { task: String, remove_worktree: bool },
}

#[derive(Clone, Debug)]
pub enum AgentEffect {
    /// Subscribe to the project's agents (`agent_subscribe`) and, when `providers`, list them; then
    /// call [`AgentsStore::connected`] with `generation`.
    Subscribe {
        generation: u64,
        providers: bool,
    },
    /// Editor tab titles of sessions may have changed.
    TabTitlesChanged,
    CloseTab {
        key: String,
    },
    /// Open (or focus) the task's session tab.
    OpenSession {
        task: String,
        title: Option<String>,
    },
    OpenHome,
    ShowToolWindow {
        id: &'static str,
    },
    Notify(Box<NotificationRequest>),
    /// Read a conversation, then call [`AgentsStore::entries_loaded`].
    LoadEntries {
        task: String,
        subagent: Option<String>,
    },
    /// Read the task's changed files, then call [`AgentsStore::changes_loaded`].
    RefreshChanges {
        task: String,
    },
    /// After `delay_ms`, call [`AgentsStore::changes_due`].
    ScheduleChanges {
        task: String,
        delay_ms: f64,
    },
    /// List the agents again, then call [`AgentsStore::providers_loaded`].
    RefreshProviders,
    /// Write `value` (JSON) to UI storage.
    Save {
        key: &'static str,
        value: String,
    },
    /// Create a task, then call [`AgentsStore::task_created`].
    Create {
        ticket: Ticket,
        request: Box<NewAgentTask>,
    },
    /// Run a backend operation; a call with a ticket is answered through [`AgentsStore::call_answered`].
    Call {
        ticket: Option<Ticket>,
        call: AgentCall,
    },
    /// Ask, then call [`AgentsStore::confirmed`].
    Confirm {
        ticket: Ticket,
        request: ConfirmRequest,
    },
    /// At `at_ms`, call [`AgentsStore::tick`].
    WakeAt {
        at_ms: f64,
    },
    /// The request started with `ticket` ended.
    Finished {
        ticket: Ticket,
        outcome: Outcome,
    },
}

/// The answer of an [`AgentCall`] with a ticket.
#[derive(Clone, Debug)]
pub enum CallAnswer {
    Done,
    /// What a rollback hands back: the message of the first dropped turn, when the agent says.
    RolledBack(Option<AgentPrompt>),
    Forked(Box<AgentTask>),
    Unqueued(AgentQueued),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusRequest<'a> {
    pub key: &'a str,
    pub seq: u64,
}

/// The Agents status bar item: counts, and the task a click opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusItem<'a> {
    pub needs_input: usize,
    pub running: usize,
    pub unread: usize,
    pub target: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollPosition {
    pub top: f64,
    /// The conversation follows new output.
    pub stick: bool,
}

/// A reference "Send to Agent" adds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub path: String,
    pub rel: String,
    pub mention: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditKind {
    Rerun,
    Retract,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum EditStage {
    Confirming,
    Stopping { deadline: f64 },
    RollingBack,
    Sending,
}

#[derive(Clone, Debug)]
struct EditFlow {
    kind: EditKind,
    task: String,
    turn: u32,
    text: String,
    images: Vec<AgentImage>,
    stage: EditStage,
    /// The request of the current stage: answers of earlier ones are dropped.
    step: Option<Ticket>,
}

#[derive(Clone, Debug)]
enum Pending {
    Create {
        project_root: Option<String>,
        text: String,
    },
    Send {
        task: String,
    },
    Unqueue {
        task: String,
    },
    Fork,
    /// A step of the edit started with this ticket.
    Edit(Ticket),
}

/// The Agents feature's state for one window. Its `Debug` leaves out the agents' environment, which
/// holds their API keys.
pub struct AgentsStore {
    pub loaded: bool,
    /// Why the tasks could not be loaded.
    pub error: Option<String>,
    /// In the order the backend first reported them.
    tasks: Vec<AgentTask>,
    /// Tasks with unseen replies or status changes, in the order they became unread.
    unread: Vec<String>,
    /// Conversations of the tasks (and subagents) opened in this window, by [`entries_key`].
    entries: HashMap<String, Vec<AgentEntry>>,
    changes: HashMap<String, Vec<AgentChange>>,
    /// Scroll position and follow-output preference by conversation, shared by both hosts.
    scroll_positions: HashMap<String, ScrollPosition>,
    pub providers: Vec<AgentProvider>,
    /// The usage limits each agent last reported for its account, by provider.
    limits: HashMap<String, Vec<AgentLimit>>,
    pub draft: Draft,
    composers: HashMap<String, ComposerDraft>,
    /// The composer typed in last: where "Send to Agent" adds references.
    pub last_composer: String,
    /// First messages of past tasks, newest first (New Task page ↑).
    pub history: Vec<String>,
    focus: Option<(String, u64)>,
    /// Task shown inside the Agents tool window instead of an editor tab.
    pub docked: Option<String>,
    pub session_location: SessionLocation,
    pub last_task: Option<String>,
    pub home_open: bool,
    generation: u64,
    /// Session views on screen by task; a task can be shown in more than one split.
    visible: HashMap<String, usize>,
    /// The window is visible and focused.
    window_active: bool,
    changes_scheduled: HashSet<String>,
    next_ticket: u64,
    pending: HashMap<Ticket, Pending>,
    edits: HashMap<Ticket, EditFlow>,
}

/// The saved state a store starts from: the values of [`DRAFT_KEY`], [`HISTORY_KEY`] and
/// [`SESSION_LOCATION_KEY`] in UI storage.
#[derive(Clone, Copy, Debug, Default)]
pub struct Saved<'a> {
    pub draft: Option<&'a str>,
    pub history: Option<&'a str>,
    pub session_location: Option<&'a str>,
}

fn json<T: Serialize>(value: &T) -> String {
    // Plain strings and string maps always serialize.
    serde_json::to_string(value).unwrap_or_default()
}

fn task_id(tab: &AgentTab) -> Option<&str> {
    match tab {
        AgentTab::Session(task) => Some(task),
        _ => None,
    }
}

/// The agents of `providers` as `Debug` prints them: id, command and whether it is installed, never the
/// values of their environment (API keys, tokens).
pub struct ProvidersDebug<'a>(pub &'a [AgentProvider]);

impl std::fmt::Debug for ProvidersDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_list();
        for p in self.0 {
            let env: Vec<&str> = p.env.keys().map(String::as_str).collect();
            list.entry(&format_args!(
                "{{ id: {:?}, command: {:?}, installed: {}, env: {:?} }}",
                p.id, p.program, p.installed, env
            ));
        }
        list.finish()
    }
}

impl std::fmt::Debug for AgentsStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentsStore")
            .field("loaded", &self.loaded)
            .field("error", &self.error)
            .field("tasks", &self.tasks)
            .field("unread", &self.unread)
            .field("providers", &ProvidersDebug(&self.providers))
            .field("draft", &self.draft)
            .field("docked", &self.docked)
            .field("session_location", &self.session_location)
            .field("home_open", &self.home_open)
            .field("generation", &self.generation)
            .finish_non_exhaustive()
    }
}

impl AgentsStore {
    pub fn new(saved: Saved<'_>) -> Self {
        let location = saved.session_location.and_then(|s| serde_json::from_str::<String>(s).ok());
        Self {
            loaded: false,
            error: None,
            tasks: Vec::new(),
            unread: Vec::new(),
            entries: HashMap::new(),
            changes: HashMap::new(),
            scroll_positions: HashMap::new(),
            providers: Vec::new(),
            limits: HashMap::new(),
            draft: saved_draft(saved.draft),
            composers: HashMap::new(),
            last_composer: HOME.to_owned(),
            history: saved.history.and_then(|s| serde_json::from_str(s).ok()).unwrap_or_default(),
            focus: None,
            docked: None,
            session_location: if location.as_deref() == Some("toolWindow") {
                SessionLocation::ToolWindow
            } else {
                SessionLocation::Editor
            },
            last_task: None,
            home_open: false,
            generation: 0,
            visible: HashMap::new(),
            window_active: true,
            changes_scheduled: HashSet::new(),
            next_ticket: 0,
            pending: HashMap::new(),
            edits: HashMap::new(),
        }
    }

    fn ticket(&mut self) -> Ticket {
        self.next_ticket += 1;
        Ticket(self.next_ticket)
    }

    /// A ticket for a request of another model of this feature (a reply action's confirmation), so that
    /// its answers cannot be mistaken for the store's.
    pub fn new_ticket(&mut self) -> Ticket {
        self.ticket()
    }

    // ----- reading -----

    /// The tasks in the order the backend reported them.
    pub fn tasks(&self) -> &[AgentTask] {
        &self.tasks
    }

    pub fn task(&self, id: &str) -> Option<&AgentTask> {
        self.tasks.iter().find(|t| t.id == id)
    }

    pub fn is_unread(&self, task: &str) -> bool {
        self.unread.iter().any(|t| t == task)
    }

    /// Unread tasks, in the order they became unread.
    pub fn unread(&self) -> &[String] {
        &self.unread
    }

    /// The Agents tool window's badge: tasks that wait for an answer or have unseen activity, each once.
    pub fn badge(&self) -> Option<usize> {
        let n = self.tasks.iter().filter(|t| t.status == AgentStatus::NeedsInput || self.is_unread(&t.id)).count();
        (n > 0).then_some(n)
    }

    /// The status bar item: tasks that wait for an answer, run, or have unseen activity, and the most
    /// urgent one, which a click opens. `None` when there is nothing to show.
    pub fn status_item(&self) -> Option<StatusItem<'_>> {
        let running = |t: &&AgentTask| matches!(t.status, AgentStatus::Running | AgentStatus::Starting);
        let mut unseen: Vec<&AgentTask> = self.tasks.iter().filter(|t| self.is_unread(&t.id)).collect();
        unseen.sort_by(|a, b| b.updated_ms.total_cmp(&a.updated_ms));
        let input: Vec<&AgentTask> = self.tasks.iter().filter(|t| t.status == AgentStatus::NeedsInput).collect();
        let running_count = self.tasks.iter().filter(running).count();
        if input.is_empty() && running_count == 0 && unseen.is_empty() {
            return None;
        }
        let target = input.first().or(unseen.first()).copied().or_else(|| self.tasks.iter().find(running));
        Some(StatusItem {
            needs_input: input.len(),
            running: running_count,
            unread: unseen.len(),
            target: target.map(|t| t.id.as_str()),
        })
    }

    pub fn entries(&self, task: &str, subagent: Option<&str>) -> Option<&[AgentEntry]> {
        self.entries.get(&entries_key(task, subagent)).map(Vec::as_slice)
    }

    pub fn loaded_conversations(&self) -> usize {
        self.entries.len()
    }

    pub fn changes(&self, task: &str) -> Option<&[AgentChange]> {
        self.changes.get(task).map(Vec::as_slice)
    }

    pub fn limits(&self, provider: &str) -> Option<&[AgentLimit]> {
        self.limits.get(provider).map(Vec::as_slice)
    }

    pub fn provider(&self, id: &str) -> Option<&AgentProvider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// The composer asked to take focus; `seq` tells repeated requests apart.
    pub fn focus(&self) -> Option<FocusRequest<'_>> {
        self.focus.as_ref().map(|(key, seq)| FocusRequest { key, seq: *seq })
    }

    pub fn scroll_position(&self, key: &str) -> Option<ScrollPosition> {
        self.scroll_positions.get(key).copied()
    }

    pub fn set_scroll_position(&mut self, key: &str, position: ScrollPosition) {
        self.scroll_positions.insert(key.to_owned(), position);
    }

    pub fn scroll_positions(&self) -> usize {
        self.scroll_positions.len()
    }

    // ----- connection -----

    /// Subscribes this window to the project's agents (again after the project changed). An untrusted
    /// project lists no agents.
    pub fn connect(&mut self, trusted: bool) -> Vec<AgentEffect> {
        self.generation += 1;
        vec![AgentEffect::Subscribe { generation: self.generation, providers: trusted }]
    }

    /// The subscription's answer: the tasks and the agents. Tabs of tasks deleted meanwhile close, like
    /// editors of deleted files.
    pub fn connected(
        &mut self,
        generation: u64,
        answer: Result<(Vec<AgentTask>, Vec<AgentProvider>), String>,
        editors: &Editors,
    ) -> Vec<AgentEffect> {
        if generation != self.generation {
            return Vec::new();
        }
        self.loaded = true;
        let (tasks, providers) = match answer {
            Ok(answer) => answer,
            Err(error) => {
                self.error = Some(error);
                return Vec::new();
            }
        };
        self.error = None;
        self.tasks = tasks;
        self.entries.clear();
        self.changes.clear();
        self.providers = providers;
        let mut effects = vec![AgentEffect::TabTitlesChanged];
        for task in editors.open.iter().filter_map(task_id) {
            if self.task(task).is_none() {
                effects.push(AgentEffect::CloseTab { key: session_key(task) });
            }
        }
        effects
    }

    /// Drops the project's agents: its events and answers are void from now on.
    pub fn disconnect(&mut self) {
        self.generation += 1;
        self.loaded = false;
        self.error = None;
        self.tasks.clear();
        self.unread.clear();
        self.entries.clear();
        self.changes.clear();
        self.docked = None;
        self.last_task = None;
        self.home_open = false;
        self.scroll_positions.clear();
        self.changes_scheduled.clear();
        self.pending.clear();
        self.edits.clear();
    }

    // ----- unread -----

    fn viewing(&self, task: &str) -> bool {
        self.window_active && self.visible.contains_key(task)
    }

    fn set_unread(&mut self, task: &str, value: bool) {
        if self.is_unread(task) == value {
            return;
        }
        if value {
            self.unread.push(task.to_owned());
        } else {
            self.unread.retain(|t| t != task);
        }
    }

    fn mark_unread(&mut self, task: &str) {
        let open = self.task(task).is_some_and(|t| t.status != AgentStatus::Archived);
        if open && !self.viewing(task) {
            self.set_unread(task, true);
        }
    }

    /// A session view of `task` is on screen; [`AgentsStore::hide_session`] when it closes or hides.
    pub fn show_session(&mut self, task: &str) {
        *self.visible.entry(task.to_owned()).or_default() += 1;
        if self.viewing(task) {
            self.set_unread(task, false);
        }
    }

    pub fn hide_session(&mut self, task: &str) {
        if let Some(count) = self.visible.get_mut(task) {
            *count -= 1;
            if *count == 0 {
                self.visible.remove(task);
            }
        }
    }

    /// The window was focused or shown (`true`), or lost focus or was hidden: the sessions on screen are
    /// read once it is both visible and focused.
    pub fn set_window_active(&mut self, active: bool) {
        self.window_active = active;
        if active {
            let shown: Vec<String> = self.visible.keys().cloned().collect();
            for task in shown {
                self.set_unread(&task, false);
            }
        }
    }

    // ----- events -----

    /// An event of the subscription `generation`.
    pub fn on_event(&mut self, generation: u64, event: AgentEvent, i18n: &I18n) -> Vec<AgentEffect> {
        if generation != self.generation {
            return Vec::new();
        }
        let mut effects = Vec::new();
        match event {
            AgentEvent::Task { task } => {
                let id = task.id.clone();
                let prev = self.task(&id).map(|t| (t.status, t.title.clone()));
                if prev.as_ref().is_some_and(|(_, title)| *title != task.title) {
                    effects.push(AgentEffect::TabTitlesChanged);
                }
                let prev_status = prev.map(|(status, _)| status);
                let status = task.status;
                let change = attention_change(prev_status, status);
                match self.tasks.iter_mut().find(|t| t.id == id) {
                    Some(slot) => *slot = task,
                    None => self.tasks.push(task),
                }
                if status == AgentStatus::Archived {
                    self.set_unread(&id, false);
                } else if change.is_some() {
                    self.mark_unread(&id);
                }
                if let Some(change) = change
                    && !self.viewing(&id)
                    && let Some(task) = self.task(&id)
                {
                    effects.push(AgentEffect::Notify(Box::new(notification(task, change, i18n))));
                }
                if prev_status != Some(status) && !is_busy(status) {
                    effects.push(AgentEffect::RefreshChanges { task: id.clone() });
                    effects.push(AgentEffect::RefreshProviders);
                }
                effects.extend(self.advance_stopping(&id));
            }
            AgentEvent::Removed { id } => {
                self.tasks.retain(|t| t.id != id);
                self.composers.remove(&id);
                self.set_unread(&id, false);
                if self.docked.as_deref() == Some(id.as_str()) {
                    self.docked = None;
                }
                effects.push(AgentEffect::CloseTab { key: session_key(&id) });
            }
            AgentEvent::Truncated { task, length } => {
                if let Some(entries) = self.entries.get_mut(&task) {
                    entries.truncate(length as usize);
                }
                effects.extend(self.schedule_changes(&task));
            }
            AgentEvent::Providers => effects.push(AgentEffect::RefreshProviders),
            AgentEvent::Limits { provider, limits } => {
                self.limits.insert(provider, limits);
            }
            AgentEvent::Entry { task, subagent, index, entry } => {
                let noteworthy = match &entry {
                    AgentEntry::Agent { text } => !text.trim().is_empty(),
                    AgentEntry::Error { .. } => true,
                    AgentEntry::Permission { chosen, .. } => chosen.is_none(),
                    _ => false,
                };
                if subagent.is_none() && noteworthy {
                    self.mark_unread(&task);
                }
                let edited = match &entry {
                    AgentEntry::Tool { status, diffs, .. } => {
                        *status == AgentToolStatus::Completed && !diffs.is_empty()
                    }
                    _ => false,
                };
                let key = entries_key(&task, subagent.as_deref());
                let Some(current) = self.entries.get_mut(&key) else { return effects };
                if !upsert_entry(current, index as usize, entry) {
                    effects.push(AgentEffect::LoadEntries { task: task.clone(), subagent });
                }
                if edited {
                    effects.extend(self.schedule_changes(&task));
                }
            }
        }
        effects
    }

    fn schedule_changes(&mut self, task: &str) -> Option<AgentEffect> {
        self.changes_scheduled
            .insert(task.to_owned())
            .then(|| AgentEffect::ScheduleChanges { task: task.to_owned(), delay_ms: CHANGES_DELAY_MS })
    }

    /// The delay of [`AgentEffect::ScheduleChanges`] ran out.
    pub fn changes_due(&mut self, task: &str) -> Option<AgentEffect> {
        self.changes_scheduled.remove(task).then(|| AgentEffect::RefreshChanges { task: task.to_owned() })
    }

    /// The answer of [`AgentEffect::RefreshChanges`]: a failure (the worktree is gone, or it is not a
    /// repository) lists nothing.
    pub fn changes_loaded(&mut self, task: &str, changes: Result<Vec<AgentChange>, String>) {
        self.changes.insert(task.to_owned(), changes.unwrap_or_default());
    }

    /// Opens a conversation: it is read once and shown from the store afterwards.
    pub fn open_conversation(&self, task: &str, subagent: Option<&str>) -> Option<AgentEffect> {
        let loaded = self.entries.contains_key(&entries_key(task, subagent));
        (!loaded).then(|| AgentEffect::LoadEntries { task: task.to_owned(), subagent: subagent.map(str::to_owned) })
    }

    pub fn entries_loaded(&mut self, task: &str, subagent: Option<&str>, entries: Vec<AgentEntry>) {
        self.entries.insert(entries_key(task, subagent), entries);
    }

    /// The answer of [`AgentEffect::RefreshProviders`]; a failed read keeps the list.
    pub fn providers_loaded(&mut self, providers: Option<Vec<AgentProvider>>) {
        if let Some(providers) = providers {
            self.providers = providers;
        }
    }

    // ----- New Task settings -----

    /// Changes the New Task settings; choosing another agent forgets the thinking levels picked.
    pub fn update_draft(&mut self, change: impl FnOnce(&mut Draft)) -> AgentEffect {
        let provider = self.draft.provider.clone();
        change(&mut self.draft);
        if self.draft.provider != provider {
            self.draft.thought_levels.clear();
        }
        AgentEffect::Save { key: DRAFT_KEY, value: json(&self.draft) }
    }

    /// The New Task settings shown for the selected agent and model, with the draft's choices applied.
    pub fn draft_config(&self) -> Vec<AgentConfigOption> {
        draft_config(self.provider(&self.draft.provider), &self.draft)
    }

    /// Picks `value` for setting `id`; a thinking level is remembered for the selected model only.
    pub fn set_draft_option(&mut self, id: &str, value: &str) -> AgentEffect {
        let config = self.draft_config();
        let model = config.iter().find(|o| o.category.as_deref() == Some("model")).and_then(|o| o.current.clone());
        let thought =
            config.iter().find(|o| o.id == id).is_some_and(|o| o.category.as_deref() == Some("thought_level"));
        match model {
            Some(model) if thought => self.update_draft(|d| {
                d.thought_levels.entry(model).or_default().insert(id.to_owned(), value.to_owned());
            }),
            _ => self.update_draft(|d| {
                d.config.insert(id.to_owned(), value.to_owned());
            }),
        }
    }

    // ----- composers -----

    pub fn composer(&self, key: &str) -> ComposerDraft {
        self.composers.get(key).cloned().unwrap_or_default()
    }

    /// Changes composer `key`, which becomes the one typed in last.
    pub fn update_composer(&mut self, key: &str, change: impl FnOnce(&mut ComposerDraft)) {
        change(self.composers.entry(key.to_owned()).or_default());
        self.last_composer = key.to_owned();
    }

    /// Moves the caret to the end of composer `key`.
    pub fn focus_composer(&mut self, key: &str) {
        let seq = self.focus.as_ref().map_or(0, |(_, seq)| *seq) + 1;
        self.focus = Some((key.to_owned(), seq));
    }

    /// Quotes `text` (a selection of the conversation) in the task's composer, to reply to it.
    pub fn quote_in_composer(&mut self, task: &str, text: &str) {
        self.update_composer(task, |c| c.text = with_quote(&c.text, text));
        self.focus_composer(task);
    }

    /// Puts a message back in the task's composer, above anything typed meanwhile.
    fn restore_composer(&mut self, task: &str, text: &str, images: &[AgentImage]) {
        self.update_composer(task, |c| {
            c.text = if c.text.is_empty() { text.to_owned() } else { format!("{text}\n{}", c.text) };
            c.images = images.iter().chain(&c.images).cloned().collect();
        });
        self.focus_composer(task);
    }

    // ----- placement -----

    /// Opens the task's conversation where conversations are shown.
    pub fn open_task(&mut self, task: &str) -> Vec<AgentEffect> {
        if self.session_location == SessionLocation::ToolWindow {
            return self.show_docked_task(task);
        }
        self.last_task = Some(task.to_owned());
        self.home_open = false;
        self.last_composer = task.to_owned();
        // The title rides along so a restored tab has it before the tasks load.
        vec![AgentEffect::OpenSession { task: task.to_owned(), title: self.task(task).map(|t| t.title.clone()) }]
    }

    fn show_docked_task(&mut self, task: &str) -> Vec<AgentEffect> {
        self.docked = Some(task.to_owned());
        self.last_task = Some(task.to_owned());
        self.home_open = false;
        self.last_composer = task.to_owned();
        vec![AgentEffect::ShowToolWindow { id: TOOL_WINDOW }, AgentEffect::CloseTab { key: session_key(task) }]
    }

    fn save_session_location(&mut self, location: SessionLocation, editors: &Editors) -> Vec<AgentEffect> {
        self.session_location = location;
        let mut effects = vec![AgentEffect::Save { key: SESSION_LOCATION_KEY, value: location.stored().to_owned() }];
        if location == SessionLocation::ToolWindow {
            for tab in &editors.open {
                match tab {
                    AgentTab::Session(task) => effects.push(AgentEffect::CloseTab { key: session_key(task) }),
                    AgentTab::Home => effects.push(AgentEffect::CloseTab { key: HOME_KEY.to_owned() }),
                    AgentTab::Other => {}
                }
            }
        }
        effects
    }

    /// Conversation placement is an Agents-wide preference, shared by every task: the conversation on
    /// show (or the New Task page) moves along.
    pub fn set_session_location(&mut self, location: SessionLocation, editors: &Editors) -> Vec<AgentEffect> {
        if self.session_location == location {
            return Vec::new();
        }
        let active_task = editors.active.as_ref().and_then(task_id).map(str::to_owned);
        let home = self.home_open || editors.active == Some(AgentTab::Home);
        let last = self.last_task.clone().filter(|t| self.task(t).is_some());
        let newest = || {
            self.tasks
                .iter()
                .filter(|t| t.status != AgentStatus::Archived)
                .min_by(|a, b| b.updated_ms.total_cmp(&a.updated_ms))
                .map(|t| t.id.clone())
        };
        let task = self.docked.clone().or(active_task).or(last).or_else(newest);
        let mut effects = self.save_session_location(location, editors);
        self.docked = None;
        self.home_open = false;
        if home {
            effects.extend(self.open_home(None, None));
        } else if let Some(task) = task {
            effects.extend(self.open_task(&task));
        } else if location == SessionLocation::ToolWindow {
            effects.extend(self.open_home(None, None));
        }
        effects
    }

    /// Dragging a session into the tool window selects the same placement for other tasks.
    pub fn dock_task(&mut self, task: &str, editors: &Editors) -> Vec<AgentEffect> {
        let mut effects = self.save_session_location(SessionLocation::ToolWindow, editors);
        effects.extend(self.show_docked_task(task));
        effects
    }

    pub fn undock_task(&mut self, editors: &Editors) -> Vec<AgentEffect> {
        self.set_session_location(SessionLocation::Editor, editors)
    }

    /// An explicit editor drop also updates where subsequent sessions open.
    pub fn open_task_in_editor(&mut self, task: &str, editors: &Editors) -> Vec<AgentEffect> {
        let mut effects = self.save_session_location(SessionLocation::Editor, editors);
        self.docked = None;
        self.home_open = false;
        effects.extend(self.open_task(task));
        effects
    }

    /// Opens the New Task page; `prompt` pre-fills it. In a workspace, the task's project defaults to
    /// the active file's member (`workspace`).
    pub fn open_home(&mut self, prompt: Option<&str>, workspace: Option<TaskProjects<'_>>) -> Vec<AgentEffect> {
        let mut effects = Vec::new();
        self.last_composer = HOME.to_owned();
        if let Some(workspace) = workspace {
            let previous = self.draft.project_root.clone();
            let root = default_task_project(workspace, previous.as_deref());
            effects.push(self.update_draft(|d| {
                if d.project_root != root {
                    d.base = None;
                }
                d.project_root = root;
            }));
        }
        if let Some(prompt) = prompt {
            self.update_composer(HOME, |c| c.text = prompt.to_owned());
        }
        if self.session_location == SessionLocation::ToolWindow {
            self.docked = None;
            self.home_open = true;
            effects.push(AgentEffect::ShowToolWindow { id: TOOL_WINDOW });
            effects.push(AgentEffect::CloseTab { key: HOME_KEY.to_owned() });
            return effects;
        }
        effects.push(AgentEffect::OpenHome);
        effects
    }

    // ----- starting and messaging -----

    /// Starts a task with the New Task page's message and settings. `isolation` overrides the draft's (a
    /// project without git can only use the local checkout). `None` when there is nothing to send.
    pub fn start_task(&mut self, isolation: Option<Isolation>, project_root: Option<&str>) -> Option<AgentEffect> {
        let composer = self.composer(HOME);
        if !can_send(&composer) {
            return None;
        }
        let isolation = isolation.unwrap_or(self.draft.isolation);
        let options = self.draft_config();
        let defaults = bypass_defaults(&options);
        let config = options
            .iter()
            .filter_map(|o| {
                let value = match o.category.as_deref() {
                    Some("model" | "thought_level") => o.current.clone(),
                    _ => self.draft.config.get(&o.id).or_else(|| defaults.get(&o.id)).cloned(),
                }?;
                o.values.iter().any(|v| v.value == value).then(|| AgentConfigChoice { id: o.id.clone(), value })
            })
            .collect();
        let request = NewAgentTask {
            project_root: project_root.map(|root| IpcPath(PathBuf::from(root))),
            prompt: to_prompt(&composer),
            provider: self.draft.provider.clone(),
            config,
            isolation,
            base: if isolation == Isolation::Worktree { self.draft.base.clone() } else { None },
        };
        let ticket = self.ticket();
        let text = composer.text.trim().to_owned();
        self.pending.insert(ticket, Pending::Create { project_root: project_root.map(str::to_owned), text });
        Some(AgentEffect::Create { ticket, request: Box::new(request) })
    }

    /// The answer of [`AgentEffect::Create`]: the task opens and the New Task page closes. Its events can
    /// arrive before this answer: the newer state they carried is kept.
    pub fn task_created(&mut self, ticket: Ticket, answer: Result<AgentTask, String>) -> Vec<AgentEffect> {
        let Some(Pending::Create { project_root, text }) = self.pending.remove(&ticket) else { return Vec::new() };
        let task = match answer {
            Ok(task) => task,
            Err(error) => return vec![AgentEffect::Finished { ticket, outcome: Err(error) }],
        };
        let id = task.id.clone();
        if self.task(&id).is_none() {
            self.tasks.push(task);
        }
        self.entries.entry(id.clone()).or_default();
        let mut effects = vec![self.update_draft(|d| d.project_root = project_root)];
        effects.extend(self.remember(&text));
        self.update_composer(HOME, |c| *c = ComposerDraft::default());
        effects.push(AgentEffect::CloseTab { key: HOME_KEY.to_owned() });
        effects.extend(self.open_task(&id));
        effects.push(AgentEffect::Finished { ticket, outcome: Ok(true) });
        effects
    }

    fn remember(&mut self, text: &str) -> Option<AgentEffect> {
        if text.is_empty() {
            return None;
        }
        self.history.retain(|h| h != text);
        self.history.insert(0, text.to_owned());
        self.history.truncate(HISTORY_SIZE);
        Some(AgentEffect::Save { key: HISTORY_KEY, value: json(&self.history) })
    }

    /// Sends the session composer's message: queued while the agent works, or with `now` interrupting it.
    pub fn send_message(&mut self, task: &str, now: bool) -> Option<AgentEffect> {
        let composer = self.composer(task);
        if !can_send(&composer) {
            return None;
        }
        let ticket = self.ticket();
        self.pending.insert(ticket, Pending::Send { task: task.to_owned() });
        let call = AgentCall::Prompt { task: task.to_owned(), prompt: to_prompt(&composer), now };
        Some(AgentEffect::Call { ticket: Some(ticket), call })
    }

    /// Takes a queued message back into the composer to edit it.
    pub fn edit_queued(&mut self, task: &str, queued: &str) -> AgentEffect {
        let ticket = self.ticket();
        self.pending.insert(ticket, Pending::Unqueue { task: task.to_owned() });
        let call = AgentCall::Unqueue { task: task.to_owned(), queued: queued.to_owned() };
        AgentEffect::Call { ticket: Some(ticket), call }
    }

    /// Copies the task's conversation into a new task (up to its first `turns` turns) and opens it.
    pub fn fork_task(&mut self, task: &str, turns: Option<u32>) -> (Ticket, AgentEffect) {
        let ticket = self.ticket();
        self.pending.insert(ticket, Pending::Fork);
        (ticket, AgentEffect::Call { ticket: Some(ticket), call: AgentCall::Fork { task: task.to_owned(), turns } })
    }

    /// The answer of an [`AgentEffect::Call`] with a ticket.
    pub fn call_answered(&mut self, ticket: Ticket, answer: Result<CallAnswer, String>) -> Vec<AgentEffect> {
        let Some(pending) = self.pending.remove(&ticket) else { return Vec::new() };
        let answer = match (pending, answer) {
            (Pending::Edit(flow), answer) => return self.edit_step_answered(flow, ticket, answer),
            (_, Err(error)) => return vec![AgentEffect::Finished { ticket, outcome: Err(error) }],
            (pending, Ok(answer)) => (pending, answer),
        };
        let mut effects = Vec::new();
        match answer {
            (Pending::Send { task }, _) => self.update_composer(&task, |c| *c = ComposerDraft::default()),
            (Pending::Unqueue { task }, CallAnswer::Unqueued(item)) => {
                let cwd = self.task(&task).map(|t| ide_core::ipc_path::to_ipc(&t.cwd)).unwrap_or_default();
                let files: Vec<FileRef> = item
                    .prompt
                    .files
                    .iter()
                    .map(|path| {
                        let path = ide_core::ipc_path::to_ipc(path);
                        let rel = path.strip_prefix(&format!("{cwd}/")).unwrap_or(path.as_str()).to_owned();
                        FileRef { path, rel }
                    })
                    .collect();
                self.update_composer(&task, |c| {
                    let text = &item.prompt.text;
                    c.text = if c.text.is_empty() { text.clone() } else { format!("{text}\n{}", c.text) };
                    c.files = files.into_iter().chain(c.files.drain(..)).collect();
                    c.images = item.prompt.images.iter().cloned().chain(c.images.drain(..)).collect();
                });
            }
            (Pending::Fork, CallAnswer::Forked(copy)) => {
                let id = copy.id.clone();
                if self.task(&id).is_none() {
                    self.tasks.push(*copy);
                }
                effects.extend(self.open_task(&id));
            }
            _ => {}
        }
        effects.push(AgentEffect::Finished { ticket, outcome: Ok(true) });
        effects
    }

    // ----- editing sent messages -----

    /// Runs the conversation again from turn `turn` with `text` as its message: the agent is stopped if it
    /// works, that turn and the later ones are dropped, then `text` is sent. Starting over from the first
    /// turn is confirmed first. Ends with [`AgentEffect::Finished`] for the returned ticket.
    pub fn rerun_from(
        &mut self,
        task: &str,
        turn: u32,
        text: &str,
        images: &[AgentImage],
        now: f64,
        i18n: &I18n,
    ) -> (Ticket, Vec<AgentEffect>) {
        let ticket = self.ticket();
        let flow = EditFlow {
            kind: EditKind::Rerun,
            task: task.to_owned(),
            turn,
            text: text.to_owned(),
            images: images.to_vec(),
            stage: EditStage::Confirming,
            step: None,
        };
        self.edits.insert(ticket, flow);
        if turn == 0 {
            let request = ConfirmRequest {
                title: i18n.tx("Start a Fresh Agent Session"),
                message: i18n
                    .tx("This clears the entire conversation and starts a fresh agent session with this message. \
                     Files the agent changed stay as they are."),
                ok: i18n.tx("Start Fresh"),
            };
            return (ticket, vec![AgentEffect::Confirm { ticket, request }]);
        }
        (ticket, self.stop_for_edit(ticket, now))
    }

    /// Drops `turn` and the later turns, and puts the message back in the composer to edit. Ends with
    /// [`AgentEffect::Finished`] for the returned ticket.
    pub fn retract_from(
        &mut self,
        task: &str,
        turn: u32,
        text: &str,
        images: &[AgentImage],
        now: f64,
    ) -> (Ticket, Vec<AgentEffect>) {
        let ticket = self.ticket();
        let flow = EditFlow {
            kind: EditKind::Retract,
            task: task.to_owned(),
            turn,
            text: text.to_owned(),
            images: images.to_vec(),
            stage: EditStage::Confirming,
            step: None,
        };
        self.edits.insert(ticket, flow);
        (ticket, self.stop_for_edit(ticket, now))
    }

    /// The answer of [`AgentEffect::Confirm`].
    pub fn confirmed(&mut self, ticket: Ticket, ok: bool, now: f64) -> Vec<AgentEffect> {
        if !self.edits.get(&ticket).is_some_and(|f| f.stage == EditStage::Confirming) {
            return Vec::new();
        }
        if !ok {
            self.edits.remove(&ticket);
            return vec![AgentEffect::Finished { ticket, outcome: Ok(false) }];
        }
        self.stop_for_edit(ticket, now)
    }

    fn task_busy(&self, task: &str) -> bool {
        self.task(task).is_some_and(|t| is_busy(t.status))
    }

    fn stop_for_edit(&mut self, ticket: Ticket, now: f64) -> Vec<AgentEffect> {
        let Some(task) = self.edits.get(&ticket).map(|f| f.task.clone()) else { return Vec::new() };
        if !self.task_busy(&task) {
            return self.roll_back(ticket);
        }
        let deadline = now + STOP_TIMEOUT_MS;
        let step = self.edit_step(ticket, EditStage::Stopping { deadline });
        vec![
            AgentEffect::Call { ticket: Some(step), call: AgentCall::Cancel { task, subagent: None } },
            AgentEffect::WakeAt { at_ms: deadline },
        ]
    }

    /// Moves the edit to `stage`, whose request gets the returned ticket.
    fn edit_step(&mut self, ticket: Ticket, stage: EditStage) -> Ticket {
        let step = self.ticket();
        self.pending.insert(step, Pending::Edit(ticket));
        if let Some(flow) = self.edits.get_mut(&ticket) {
            flow.stage = stage;
            flow.step = Some(step);
        }
        step
    }

    fn roll_back(&mut self, ticket: Ticket) -> Vec<AgentEffect> {
        let Some(flow) = self.edits.get(&ticket) else { return Vec::new() };
        let call = AgentCall::Rollback { task: flow.task.clone(), turns: flow.turn };
        let step = self.edit_step(ticket, EditStage::RollingBack);
        vec![AgentEffect::Call { ticket: Some(step), call }]
    }

    /// Edits waiting for `task` to stop go on once it has.
    fn advance_stopping(&mut self, task: &str) -> Vec<AgentEffect> {
        if self.task_busy(task) {
            return Vec::new();
        }
        let waiting: Vec<Ticket> = self
            .edits
            .iter()
            .filter(|(_, f)| f.task == task && matches!(f.stage, EditStage::Stopping { .. }))
            .map(|(ticket, _)| *ticket)
            .collect();
        waiting.into_iter().flat_map(|ticket| self.roll_back(ticket)).collect()
    }

    /// The time of an [`AgentEffect::WakeAt`] came: an edit still waiting for its task to stop gives up.
    pub fn tick(&mut self, now: f64, i18n: &I18n) -> Vec<AgentEffect> {
        let expired: Vec<Ticket> = self
            .edits
            .iter()
            .filter(|(_, f)| matches!(f.stage, EditStage::Stopping { deadline } if deadline <= now))
            .map(|(ticket, _)| *ticket)
            .collect();
        let message = i18n.tx("The agent is still stopping. Try again once it has stopped.");
        expired.into_iter().flat_map(|ticket| self.fail_edit(ticket, message.clone())).collect()
    }

    fn fail_edit(&mut self, ticket: Ticket, error: String) -> Vec<AgentEffect> {
        let Some(flow) = self.edits.remove(&ticket) else { return Vec::new() };
        self.restore_composer(&flow.task, &flow.text, &flow.images);
        vec![AgentEffect::Finished { ticket, outcome: Err(error) }]
    }

    fn edit_step_answered(
        &mut self,
        ticket: Ticket,
        step: Ticket,
        answer: Result<CallAnswer, String>,
    ) -> Vec<AgentEffect> {
        let Some(flow) = self.edits.get(&ticket).filter(|flow| flow.step == Some(step)) else { return Vec::new() };
        let answer = match answer {
            Ok(answer) => answer,
            Err(error) => return self.fail_edit(ticket, error),
        };
        match (flow.stage, flow.kind, answer) {
            // The turn was asked to stop; the task's next status says when it has.
            (EditStage::Stopping { .. }, _, _) => {
                let task = flow.task.clone();
                self.advance_stopping(&task)
            }
            (EditStage::RollingBack, EditKind::Retract, answer) => {
                let Some(flow) = self.edits.remove(&ticket) else { return Vec::new() };
                // The agent only hands back the text; the images come from the message itself.
                let text = match answer {
                    CallAnswer::RolledBack(Some(prompt)) => prompt.text,
                    _ => flow.text.clone(),
                };
                self.restore_composer(&flow.task, &text, &flow.images);
                vec![AgentEffect::Finished { ticket, outcome: Ok(true) }]
            }
            (EditStage::RollingBack, EditKind::Rerun, _) => {
                let prompt = AgentPrompt { text: flow.text.clone(), files: Vec::new(), images: flow.images.clone() };
                let call = AgentCall::Prompt { task: flow.task.clone(), prompt, now: false };
                let step = self.edit_step(ticket, EditStage::Sending);
                vec![AgentEffect::Call { ticket: Some(step), call }]
            }
            (EditStage::Sending, _, _) => {
                self.edits.remove(&ticket);
                vec![AgentEffect::Finished { ticket, outcome: Ok(true) }]
            }
            (EditStage::Confirming, _, _) => Vec::new(),
        }
    }

    /// Whether an edit of `task`'s conversation is under way.
    pub fn editing(&self, task: &str) -> bool {
        self.edits.values().any(|f| f.task == task)
    }

    // ----- Send to Agent -----

    /// The composer the user is looking at: the Agents surface on show, else the one typed in last.
    pub fn active_composer_key(&self, editors: &Editors) -> String {
        // The tool window shows one conversation at a time; its inline New Task page wins.
        if self.session_location == SessionLocation::ToolWindow {
            if self.home_open {
                return HOME.to_owned();
            }
            if let Some(docked) = self.docked.as_ref().filter(|t| self.task(t).is_some()) {
                return docked.clone();
            }
        }
        // No agent tab has focus (the Project view has it): the one used last wins.
        for tab in editors.active.iter().chain(&editors.open) {
            match tab {
                AgentTab::Home => return HOME.to_owned(),
                AgentTab::Session(task) if self.task(task).is_some() => return task.clone(),
                _ => {}
            }
        }
        if self.last_composer != HOME && self.task(&self.last_composer).is_some() {
            self.last_composer.clone()
        } else {
            HOME.to_owned()
        }
    }

    /// Adds `@file` references (with an optional line range) to the composer on show (else the one typed
    /// in last) and shows it: IDEA-style "send this to the chat".
    pub fn add_references(&mut self, refs: &[Reference], editors: &Editors) -> Vec<AgentEffect> {
        let key = self.active_composer_key(editors);
        let task = (key != HOME).then(|| self.task(&key)).flatten();
        // A worktree task sees the same files under its own directory.
        let root = task.filter(|t| t.isolation == Isolation::Worktree).map(|t| ide_core::ipc_path::to_ipc(&t.cwd));
        let task_id = task.map(|t| t.id.clone());
        let files: Vec<FileRef> = refs
            .iter()
            .map(|r| FileRef {
                path: root.as_ref().map_or_else(|| r.path.clone(), |root| format!("{root}/{}", r.rel)),
                rel: r.rel.clone(),
            })
            .collect();
        let mentions = refs.iter().map(|r| r.mention.as_str()).collect::<Vec<_>>().join(" ");
        self.update_composer(&key, |c| {
            c.text = if !c.text.is_empty() && !c.text.ends_with(char::is_whitespace) {
                format!("{} {mentions} ", c.text)
            } else {
                format!("{}{mentions} ", c.text)
            };
            c.files.extend(files);
        });
        match task_id {
            Some(task) => self.open_task(&task),
            None => self.open_home(None, None),
        }
    }
}

/// The members of a workspace a new task can run in.
#[derive(Clone, Copy, Debug)]
pub struct TaskProjects<'a> {
    pub root: Option<&'a str>,
    /// The workspace's member projects, in manifest order.
    pub members: &'a [String],
    /// The active editor's file.
    pub active_path: Option<&'a str>,
}

/// The project a new task runs in: the active file's member, then the last task's member, then the
/// first member in manifest order (the root outside a workspace).
pub fn default_task_project(projects: TaskProjects<'_>, last: Option<&str>) -> Option<String> {
    if projects.members.is_empty() {
        return projects.root.map(str::to_owned);
    }
    let active = projects
        .active_path
        .and_then(|path| projects.members.iter().filter(|m| is_same_or_inside(path, m)).max_by_key(|m| m.len()));
    active
        .or_else(|| last.and_then(|last| projects.members.iter().find(|m| *m == last)))
        .or(projects.members.first())
        .cloned()
}

/// Config for the selected model, with only supported draft choices applied.
pub fn draft_config(provider: Option<&AgentProvider>, draft: &Draft) -> Vec<AgentConfigOption> {
    fn applied(option: &AgentConfigOption, choice: Option<&String>) -> AgentConfigOption {
        let mut option = option.clone();
        if let Some(choice) = choice.filter(|c| option.values.iter().any(|v| v.value == **c)) {
            option.current = Some(choice.clone());
        }
        option
    }
    let Some(provider) = provider else { return Vec::new() };
    let mut config: Vec<AgentConfigOption> = provider
        .config
        .iter()
        .filter(|o| o.category.as_deref() != Some("thought_level"))
        .map(|o| applied(o, draft.config.get(&o.id)))
        .collect();
    let model = config.iter().find(|o| o.category.as_deref() == Some("model")).and_then(|o| o.current.clone());
    if let Some(model) = model {
        let levels = provider.thought_levels.get(&model).map(Vec::as_slice).unwrap_or_default();
        let chosen = draft.thought_levels.get(&model);
        config.extend(levels.iter().map(|o| applied(o, chosen.and_then(|c| c.get(&o.id)))));
    }
    config
}

fn notification(task: &AgentTask, change: Attention, i18n: &I18n) -> NotificationRequest {
    let body = match change {
        Attention::Input => i18n.tx("Waiting for your answer"),
        Attention::Failed => task.error.clone().unwrap_or_else(|| i18n.tx("The agent stopped")),
        Attention::Finished => i18n.tx("Finished"),
    };
    let level = if change == Attention::Failed { NotificationLevel::Error } else { NotificationLevel::Info };
    let label = i18n.tx(if change == Attention::Finished { "Review" } else { "Open" });
    let mut request = NotificationRequest::new(level, task.title.clone());
    request.body = Some(body);
    request.key = Some(session_key(&task.id));
    request.group = Some("tools".to_owned());
    request.sticky = Some(change != Attention::Finished);
    request.verbatim = true;
    request.actions = vec![NotificationAction { label, command: OPEN_TASK_COMMAND.to_owned() }];
    request
}

#[cfg(test)]
mod tests;
