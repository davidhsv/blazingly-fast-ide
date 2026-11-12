//! What a task's conversation draws: its rows (a run of reasoning and finished tool calls folds into one),
//! where an author heading starts a run of the agent's work, the folds and what they render (all of it
//! while Find in Conversation is open), the actions of each message, the subagent conversation shown,
//! whether the Changes pane is open and the meters above the composer.

use crate::agents::composer::{LimitLabel, MeterLabel, cost_label, limit_label, usage_label, work_time};
use crate::agents::conversation::format_duration;
use crate::agents::model::{ReplyTurn, entries_key, is_busy, reply_operations};
use crate::agents::store::{AgentEffect, AgentsStore};
use crate::platform::i18n::I18n;
use ide_agent::{AgentChange, AgentEntry, AgentLimit, AgentStatus, AgentTask, AgentToolStatus};
use ide_core::settings::AgentSettings;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Entries rendered at a time when a conversation opens at its end; the earlier ones follow in batches.
pub const ENTRY_BATCH: usize = 40;

/// A tool call still at work: it stays on its own row with its output expanded.
pub fn is_live_tool(entry: &AgentEntry) -> bool {
    matches!(entry, AgentEntry::Tool { status: AgentToolStatus::Running | AgentToolStatus::Pending, .. })
}

/// Reasoning and finished tool calls fold together.
pub fn is_process_entry(entry: &AgentEntry) -> bool {
    matches!(entry, AgentEntry::Thought { .. }) || (matches!(entry, AgentEntry::Tool { .. }) && !is_live_tool(entry))
}

/// A row of the conversation: an entry, or a run of reasoning and tool calls folded together. Indexes are
/// those of the whole conversation, not of the painted tail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConversationRow {
    Entry { index: usize },
    Process { index: usize, entries: Vec<usize> },
}

impl ConversationRow {
    /// The row's key: its first entry's index.
    pub fn index(&self) -> usize {
        match self {
            Self::Entry { index } | Self::Process { index, .. } => *index,
        }
    }
}

/// The conversation's rows from `from`: a run of more than one reasoning or finished tool call entry
/// becomes one folded row (the work behind a decision), so messages and decisions stay readable.
pub fn conversation_rows(entries: &[AgentEntry], from: usize) -> Vec<ConversationRow> {
    fn flush(rows: &mut Vec<ConversationRow>, run: &mut Vec<usize>) {
        match run.len() {
            0 => {}
            1 => rows.push(ConversationRow::Entry { index: run[0] }),
            _ => rows.push(ConversationRow::Process { index: run[0], entries: std::mem::take(run) }),
        }
        run.clear();
    }
    let mut rows = Vec::new();
    let mut run = Vec::new();
    for (index, entry) in entries.iter().enumerate().skip(from) {
        if is_process_entry(entry) {
            run.push(index);
        } else {
            flush(&mut rows, &mut run);
            rows.push(ConversationRow::Entry { index });
        }
    }
    flush(&mut rows, &mut run);
    rows
}

/// The first entry painted when the last `shown` entries are (all of them while finding). A run of process
/// entries is never split, so prepending earlier entries never replaces the row the view is anchored to.
pub fn rows_start(entries: &[AgentEntry], shown: usize, finding: bool) -> usize {
    let mut start = if finding { 0 } else { entries.len().saturating_sub(shown) };
    while start > 0 && is_process_entry(&entries[start]) && is_process_entry(&entries[start - 1]) {
        start -= 1;
    }
    start
}

fn by_agent(entries: &[AgentEntry], row: &ConversationRow) -> bool {
    match row {
        ConversationRow::Process { .. } => true,
        ConversationRow::Entry { index } => !matches!(
            entries.get(*index),
            Some(AgentEntry::User { .. } | AgentEntry::Subagent { .. } | AgentEntry::Message { incoming: true, .. })
        ),
    }
}

/// The rows (by index) that start a run of the agent's work and carry its author heading. Messages,
/// subagents and the parent agent's requests end a run.
pub fn author_headers(entries: &[AgentEntry], rows: &[ConversationRow]) -> BTreeSet<usize> {
    let mut headers = BTreeSet::new();
    let mut previous = false;
    for row in rows {
        let agent = by_agent(entries, row);
        if agent && !previous {
            headers.insert(row.index());
        }
        previous = agent;
    }
    headers
}

/// The summary line of a folded run: "Reasoning · 2 tool calls", the lines its edits added and removed,
/// and whether the agent is still at it (its last entry is the conversation's last while the agent works).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSummary {
    pub label: String,
    pub added: u32,
    pub removed: u32,
    pub working: bool,
}

/// `working`: the index of the entry the agent is at work on (the last one, while it works).
pub fn process_summary(entries: &[AgentEntry], run: &[usize], working: Option<usize>, i18n: &I18n) -> ProcessSummary {
    let mut tools = 0;
    let mut reasoning = false;
    let (mut added, mut removed) = (0, 0);
    for entry in run.iter().filter_map(|i| entries.get(*i)) {
        match entry {
            AgentEntry::Tool { diffs, .. } => {
                tools += 1;
                added += diffs.iter().map(|d| d.added).sum::<u32>();
                removed += diffs.iter().map(|d| d.removed).sum::<u32>();
            }
            AgentEntry::Thought { .. } => reasoning = true,
            _ => {}
        }
    }
    let mut parts = Vec::new();
    if reasoning {
        parts.push(i18n.tx("Reasoning"));
    }
    if tools > 0 {
        parts.push(format!("{tools} {}", i18n.tx(if tools == 1 { "tool call" } else { "tool calls" })));
    }
    ProcessSummary { label: parts.join(" · "), added, removed, working: working.is_some_and(|w| run.contains(&w)) }
}

/// The heading of a reasoning entry.
pub fn thought_label(working: bool, i18n: &I18n) -> String {
    i18n.tx(if working { "Thinking" } else { "Reasoning" })
}

/// A collapsible section of the conversation, by kind and the index of its entry (a folded run shares
/// its first entry's index).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fold {
    Process(usize),
    Thought(usize),
    /// A tool call with output.
    Tool(usize),
}

/// Which folds the user opened. A fold renders its content only once opened, so most of a long
/// conversation stays out of the view; Find in Conversation renders all of it without opening any.
#[derive(Clone, Debug, Default)]
pub struct Folds {
    open: HashSet<Fold>,
}

impl Folds {
    /// Whether the fold shows open: the user opened it, or it holds a tool still at work.
    pub fn is_open(&self, fold: Fold, entries: &[AgentEntry]) -> bool {
        self.open.contains(&fold) || matches!(fold, Fold::Tool(i) if entries.get(i).is_some_and(is_live_tool))
    }

    /// The user opened or closed the fold.
    pub fn toggled(&mut self, fold: Fold, open: bool) {
        if open {
            self.open.insert(fold);
        } else {
            self.open.remove(&fold);
        }
    }

    /// Whether the fold's content is rendered.
    pub fn renders(&self, fold: Fold, entries: &[AgentEntry], finding: bool) -> bool {
        finding || self.is_open(fold, entries)
    }
}

/// The text the rows render, in order: what Find in Conversation searches (with `find_in_runs`). Closed
/// folds contribute their summary only; `working` as for [`process_summary`].
pub fn rendered_texts(
    entries: &[AgentEntry],
    rows: &[ConversationRow],
    folds: &Folds,
    finding: bool,
    working: Option<usize>,
    i18n: &I18n,
) -> Vec<String> {
    let mut out = Vec::new();
    for row in rows {
        match row {
            ConversationRow::Entry { index } => entry_texts(entries, *index, folds, finding, working, i18n, &mut out),
            ConversationRow::Process { index, entries: run } => {
                out.push(process_summary(entries, run, working, i18n).label);
                if folds.renders(Fold::Process(*index), entries, finding) {
                    for i in run {
                        entry_texts(entries, *i, folds, finding, working, i18n, &mut out);
                    }
                }
            }
        }
    }
    out
}

fn entry_texts(
    entries: &[AgentEntry],
    index: usize,
    folds: &Folds,
    finding: bool,
    working: Option<usize>,
    i18n: &I18n,
    out: &mut Vec<String>,
) {
    let Some(entry) = entries.get(index) else { return };
    match entry {
        AgentEntry::User { text, .. }
        | AgentEntry::Agent { text }
        | AgentEntry::Message { text, .. }
        | AgentEntry::Error { message: text } => out.push(text.clone()),
        AgentEntry::Thought { text } => {
            out.push(thought_label(working == Some(index), i18n));
            if folds.renders(Fold::Thought(index), entries, finding) {
                out.push(text.clone());
            }
        }
        AgentEntry::Tool { title, kind, output, .. } => {
            out.push(if title.is_empty() { kind.clone() } else { title.clone() });
            if let Some(output) = output
                && folds.renders(Fold::Tool(index), entries, finding)
            {
                out.push(output.clone());
            }
        }
        AgentEntry::Plan { items } => {
            out.push(i18n.tx("Plan"));
            out.extend(items.iter().map(|item| item.content.clone()));
        }
        AgentEntry::Subagent { title, description, .. } => {
            out.extend(title.iter().chain(description).cloned());
        }
        AgentEntry::Permission { title, .. } => out.push(title.clone()),
    }
}

/// When a message was sent (or the turn behind a reply finished) and how long the turn took, "12s". The
/// view writes `at_ms` as a local time.
#[derive(Clone, Debug, PartialEq)]
pub struct MessageTime {
    pub at_ms: f64,
    pub took: Option<String>,
}

/// `at_done`: replies show when their turn finished; messages show when they were sent.
pub fn message_time(sent_ms: Option<f64>, done_ms: Option<f64>, at_done: bool) -> Option<MessageTime> {
    let at_ms = if at_done { done_ms } else { sent_ms }?;
    let took = sent_ms.zip(done_ms).map(|(sent, done)| format_duration((done - sent).max(0.0)));
    Some(MessageTime { at_ms, took })
}

/// The hover actions of an agent reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReplyButton {
    /// "Copy Markdown" ("Copied" just after).
    Copy,
    QuoteReply,
    Regenerate,
    /// "Branch Conversation from Here".
    Branch,
}

pub fn reply_buttons(task: &AgentTask, reply: Option<&ReplyTurn>, turn_count: usize) -> Vec<ReplyButton> {
    let operations = reply_operations(task, reply, turn_count);
    let mut out = vec![ReplyButton::Copy];
    if task.status != AgentStatus::Archived {
        out.push(ReplyButton::QuoteReply);
    }
    if operations.regenerate {
        out.push(ReplyButton::Regenerate);
    }
    if operations.fork {
        out.push(ReplyButton::Branch);
    }
    out
}

/// The hover actions of a message the user sent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserButton {
    Copy,
    Edit,
    /// "Retract Message".
    Retract,
    RunAgainFromHere,
    SendAgain,
    /// "Copy to a New Task From Here".
    Fork,
}

/// `turn`: the turn the message started (`None` for a message steered into a running turn). Editing and
/// running again in place need an agent that can drop turns; a copy up to an earlier turn drops the later
/// ones from the copied session.
pub fn user_buttons(
    task: &AgentTask,
    text: &str,
    attachments: usize,
    turn: Option<usize>,
    turn_count: usize,
) -> Vec<UserButton> {
    let archived = task.status == AgentStatus::Archived;
    let idle = !archived && !is_busy(task.status) && turn.is_some();
    let capabilities = task.capabilities;
    let in_place = !archived && capabilities.rollback && turn.is_some();
    let latest = turn.is_some_and(|t| t + 1 == turn_count);
    let mut out = vec![UserButton::Copy];
    if in_place && !text.is_empty() {
        out.push(UserButton::Edit);
    }
    if in_place {
        out.push(UserButton::Retract);
    }
    if !archived && !text.is_empty() && attachments == 0 {
        out.push(if in_place { UserButton::RunAgainFromHere } else { UserButton::SendAgain });
    }
    if idle && capabilities.fork && (capabilities.rollback || latest) {
        out.push(UserButton::Fork);
    }
    out
}

/// The files of a user message: its attachments without the images, which are listed last and shown as
/// thumbnails instead.
pub fn user_files(attachments: &[String], images: usize) -> &[String] {
    &attachments[..attachments.len().saturating_sub(images)]
}

/// The conversation a session shows: the task's own, or a subagent's, with the subagents that lead to it.
/// Another task always starts at its own conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionNav {
    task: String,
    path: Vec<String>,
}

impl SessionNav {
    pub fn new(task: &str) -> Self {
        Self { task: task.to_owned(), path: Vec::new() }
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    /// The session now shows `task` (the docked task changed): another task opens at its own conversation.
    pub fn show_task(&mut self, task: &str) {
        if self.task != task {
            *self = Self::new(task);
        }
    }

    /// The subagents leading to the conversation shown; empty for the task's own.
    pub fn path(&self) -> &[String] {
        &self.path
    }

    /// The subagent whose conversation is shown.
    pub fn subagent(&self) -> Option<&str> {
        self.path.last().map(String::as_str)
    }

    /// Opens a subagent of the conversation shown; returns the read of its conversation, unless loaded.
    pub fn open_subagent(&mut self, id: &str, store: &AgentsStore) -> Option<AgentEffect> {
        self.path.push(id.to_owned());
        store.open_conversation(&self.task, Some(id))
    }

    /// Back to the conversation that started the one shown.
    pub fn back(&mut self) {
        self.path.pop();
    }

    /// A breadcrumb: the conversation `depth` subagents deep (0 is the task's own).
    pub fn navigate(&mut self, depth: usize) {
        self.path.truncate(depth);
    }

    /// The key of the conversation shown, for its entries and its scroll position.
    pub fn entries_key(&self) -> String {
        entries_key(&self.task, self.subagent())
    }

    /// Only the task's own conversation takes messages (and shows its question card): subagents take
    /// their instructions from the agent that started them.
    pub fn shows_composer(&self) -> bool {
        self.path.is_empty()
    }
}

pub const CHANGES_OPEN_KEY: &str = "openidea.agents.changesOpen.v1";

/// The saved Changes pane choices by task (JSON as stored; anything unreadable is none).
fn saved_changes_open(saved: Option<&str>) -> BTreeMap<String, bool> {
    saved.and_then(|json| serde_json::from_str(json).ok()).unwrap_or_default()
}

/// Whether a session shows its Changes pane: the user's choice once made, else while the task has
/// changes. A docked session stays in the conversation until the user opens the changes, and its choice
/// is not saved: navigating there doesn't change the editor's preference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangesOpen {
    docked: bool,
    chosen: Option<bool>,
}

impl ChangesOpen {
    /// `saved`: the value of [`CHANGES_OPEN_KEY`].
    pub fn new(task: &str, docked: bool, saved: Option<&str>) -> Self {
        let chosen = if docked { Some(false) } else { saved_changes_open(saved).get(task).copied() };
        Self { docked, chosen }
    }

    pub fn is_open(&self, changes: Option<&[AgentChange]>) -> bool {
        self.chosen.unwrap_or_else(|| changes.is_some_and(|c| !c.is_empty()))
    }

    /// The user showed or hid the pane; an editor session saves the choice (`saved` as for [`Self::new`]).
    pub fn set(&mut self, task: &str, open: bool, saved: Option<&str>) -> Option<AgentEffect> {
        self.chosen = Some(open);
        if self.docked {
            return None;
        }
        let mut all = saved_changes_open(saved);
        all.insert(task.to_owned(), open);
        Some(AgentEffect::Save { key: CHANGES_OPEN_KEY, value: serde_json::to_string(&all).unwrap_or_default() })
    }
}

/// Whether the conversation counts as read: its tab is shown and the docked Changes pane does not cover
/// it. The view calls [`AgentsStore::show_session`] when this becomes true and `hide_session` when false.
pub fn reads_conversation(shown: bool, docked: bool, changes_open: bool) -> bool {
    shown && !(docked && changes_open)
}

/// The header's Changes button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangesToggle {
    pub enabled: bool,
    pub pressed: bool,
    pub label: String,
    /// Changed files, shown when there are any.
    pub count: usize,
}

pub fn changes_toggle(changes: Option<&[AgentChange]>, open: bool, i18n: &I18n) -> ChangesToggle {
    let count = changes.map_or(0, <[AgentChange]>::len);
    ChangesToggle {
        enabled: count > 0 || open,
        pressed: open,
        label: i18n.tx(if open { "Hide changes" } else { "Show changes" }),
        count,
    }
}

/// A meter above the composer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Meter {
    /// Context window use, "12%".
    Context(MeterLabel),
    /// A usage-limit window, "5h 19%".
    Limit(LimitLabel),
    /// What the session has cost so far, "$0.42".
    Cost(String),
    /// Time the agent spent on the task.
    WorkTime(String),
}

/// How often the meters update: every second while the agent works.
pub fn meters_interval_ms(task: &AgentTask) -> f64 {
    if is_busy(task.status) { 1000.0 } else { 60_000.0 }
}

/// The meters as Settings › Tools › Agents › Chat allows: context use, the provider's usage limits and
/// the work time by default, the cost when asked for.
pub fn meters(
    task: &AgentTask,
    entries: Option<&[AgentEntry]>,
    limits: Option<&[AgentLimit]>,
    settings: &AgentSettings,
    now: f64,
    i18n: &mut I18n,
) -> Vec<Meter> {
    let mut out = Vec::new();
    if settings.show_context_usage
        && let Some(usage) = &task.usage
    {
        out.push(Meter::Context(usage_label(usage, i18n)));
    }
    if settings.show_usage_limits {
        out.extend(limits.unwrap_or_default().iter().map(|l| Meter::Limit(limit_label(l, now, i18n))));
    }
    if settings.show_cost
        && let Some(cost) = task.usage.as_ref().and_then(|u| u.cost.as_deref())
    {
        out.push(Meter::Cost(cost_label(cost)));
    }
    if settings.show_work_time
        && let Some(entries) = entries
    {
        let work = work_time(entries, is_busy(task.status).then_some(now));
        if work >= 1000.0 {
            out.push(Meter::WorkTime(format_duration(work)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
