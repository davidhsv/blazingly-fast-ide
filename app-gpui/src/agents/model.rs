//! Agent tasks as the Agents tool window and the session editor present them: the groups tasks are
//! listed in, labels, the turns of a conversation and what each reply offers.

use ide_agent::{AgentEntry, AgentImage, AgentStatus, AgentSubagentStatus, AgentTask};
use std::collections::BTreeMap;

pub const SESSION_KIND: &str = "agent-session";
pub const HOME_KIND: &str = "agent-home";
pub const HOME_KEY: &str = "agent:new";
pub const TOOL_WINDOW: &str = "agents";
/// MIME of a dragged agent task (a tree row or the docked conversation bar): the payload is the task id.
pub const TASK_DRAG: &str = "application/x-openidea-agent-task";
/// Settings › Tools › Agents: the ACP Registry.
pub const REGISTRY_PAGE: &str = "tools.agents";

/// Editor key of a task's session tab.
pub fn session_key(task: &str) -> String {
    format!("agent:{task}")
}

/// Key of a conversation in the entries store: the task's own, or its subagent `subagent`'s.
pub fn entries_key(task: &str, subagent: Option<&str>) -> String {
    match subagent {
        Some(subagent) if !subagent.is_empty() => format!("{task}/{subagent}"),
        _ => task.to_owned(),
    }
}

/// The card of subagent `id` in its parent's `entries`.
pub fn find_subagent<'a>(entries: Option<&'a [AgentEntry]>, id: &str) -> Option<&'a AgentEntry> {
    entries?.iter().find(|e| matches!(e, AgentEntry::Subagent { id: card, .. } if card == id))
}

pub fn subagent_working(status: AgentSubagentStatus) -> bool {
    matches!(status, AgentSubagentStatus::Running | AgentSubagentStatus::NeedsInput)
}

/// "Explore", else what it was started for; "Subagent" for a card without either.
pub fn subagent_title(entry: &AgentEntry) -> &str {
    match entry {
        AgentEntry::Subagent { title, description, .. } => {
            title.as_deref().or(description.as_deref()).unwrap_or("Subagent")
        }
        _ => "Subagent",
    }
}

/// English text, shown through `I18n::tx`.
pub fn subagent_status_label(status: AgentSubagentStatus) -> &'static str {
    match status {
        AgentSubagentStatus::Running => "Running",
        AgentSubagentStatus::NeedsInput => "Needs input",
        AgentSubagentStatus::Done => "Done",
        AgentSubagentStatus::Cancelled => "Stopped",
        AgentSubagentStatus::Failed => "Failed",
    }
}

const PROVIDER_ICONS: [&str; 12] =
    ["claude", "codex", "gemini", "copilot", "cursor", "opencode", "qwen", "kimi", "goose", "auggie", "kiro", "vibe"];

/// The agent's logo; custom agents get the generic agents icon.
pub fn provider_icon(provider: &str) -> String {
    if PROVIDER_ICONS.contains(&provider) { format!("agent-{provider}") } else { "toolwindow-agents".to_owned() }
}

/// The groups of the Agents tool window, in attention order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupStatus {
    NeedsInput,
    Failed,
    Limited,
    Running,
    Ready,
    Archived,
}

#[derive(Clone, Debug)]
pub struct TaskGroup<'a> {
    pub status: GroupStatus,
    /// English text, shown through `I18n::tx`.
    pub label: &'static str,
    pub tasks: Vec<&'a AgentTask>,
}

/// What waits for the user comes first.
const GROUPS: [(GroupStatus, &str, &[AgentStatus]); 6] = [
    (GroupStatus::NeedsInput, "Needs Input", &[AgentStatus::NeedsInput]),
    (GroupStatus::Failed, "Failed", &[AgentStatus::Failed]),
    (GroupStatus::Limited, "Usage Limit", &[AgentStatus::Limited]),
    (GroupStatus::Running, "Running", &[AgentStatus::Running, AgentStatus::Starting]),
    (GroupStatus::Ready, "Ready to Review", &[AgentStatus::Ready]),
    (GroupStatus::Archived, "Archived", &[AgentStatus::Archived]),
];

/// The non-empty groups, each newest first (ties keep the given order).
pub fn group_tasks<'a>(tasks: impl IntoIterator<Item = &'a AgentTask>) -> Vec<TaskGroup<'a>> {
    let mut sorted: Vec<&AgentTask> = tasks.into_iter().collect();
    sorted.sort_by(|a, b| b.updated_ms.total_cmp(&a.updated_ms));
    GROUPS
        .iter()
        .map(|(status, label, members)| TaskGroup {
            status: *status,
            label,
            tasks: sorted.iter().copied().filter(|t| members.contains(&t.status)).collect(),
        })
        .filter(|g| !g.tasks.is_empty())
        .collect()
}

pub fn is_busy(status: AgentStatus) -> bool {
    matches!(status, AgentStatus::Running | AgentStatus::Starting | AgentStatus::NeedsInput)
}

/// Puts `entry` at `index` (appended when `index` is the length). A gap is refused (`false`): the
/// conversation is reloaded instead.
pub fn upsert_entry(entries: &mut Vec<AgentEntry>, index: usize, entry: AgentEntry) -> bool {
    match index.cmp(&entries.len()) {
        std::cmp::Ordering::Less => entries[index] = entry,
        std::cmp::Ordering::Equal => entries.push(entry),
        std::cmp::Ordering::Greater => return false,
    }
    true
}

/// "agent/fix-lexer" → "fix-lexer".
pub fn branch_label(branch: Option<&str>) -> &str {
    let branch = branch.unwrap_or_default();
    branch.strip_prefix("agent/").unwrap_or(branch)
}

/// English text, shown through `I18n::tx`.
pub fn status_label(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Starting => "Starting",
        AgentStatus::Running => "Running",
        AgentStatus::NeedsInput => "Needs input",
        AgentStatus::Ready => "Ready",
        AgentStatus::Failed => "Failed",
        AgentStatus::Limited => "Usage limit",
        AgentStatus::Archived => "Archived",
    }
}

/// A user message that started a turn (not one steered into a running turn).
fn starts_turn(entry: &AgentEntry) -> bool {
    matches!(entry, AgentEntry::User { steered: false, .. })
}

/// Entry index of each turn-starting message → its turn's number.
pub fn turn_numbers(entries: &[AgentEntry]) -> BTreeMap<usize, usize> {
    let mut out = BTreeMap::new();
    for (i, entry) in entries.iter().enumerate() {
        if starts_turn(entry) {
            let turn = out.len();
            out.insert(i, turn);
        }
    }
    out
}

/// The message that started a turn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TurnPrompt<'a> {
    /// Its index in the conversation.
    pub index: usize,
    pub text: &'a str,
    pub attachments: &'a [String],
    pub images: &'a [AgentImage],
}

impl<'a> TurnPrompt<'a> {
    /// The prompt at `index`, when that entry is a user message.
    pub fn at(entries: &'a [AgentEntry], index: usize) -> Option<Self> {
        match entries.get(index)? {
            AgentEntry::User { text, attachments, images, .. } => Some(Self { index, text, attachments, images }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplyTurn<'a> {
    pub turn: usize,
    pub prompt: TurnPrompt<'a>,
    /// A message was steered into the turn.
    pub steered: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplyTimes {
    pub sent_ms: Option<f64>,
    pub done_ms: Option<f64>,
}

/// When the turn behind each agent reply was asked and finished, by entry index.
pub fn reply_times(entries: &[AgentEntry]) -> BTreeMap<usize, ReplyTimes> {
    let mut out = BTreeMap::new();
    let mut current: Option<ReplyTimes> = None;
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            AgentEntry::User { steered: false, sent_ms, done_ms, .. } => {
                current = Some(ReplyTimes { sent_ms: *sent_ms, done_ms: *done_ms });
            }
            AgentEntry::Agent { .. } => {
                if let Some(times) = current {
                    out.insert(i, times);
                }
            }
            _ => {}
        }
    }
    out
}

/// The last non-empty reply of each turn, by entry index: only it offers actions that change the
/// conversation.
pub fn reply_turns(entries: &[AgentEntry]) -> BTreeMap<usize, ReplyTurn<'_>> {
    let mut turns: Vec<ReplyTurn> = Vec::new();
    let mut last: Vec<Option<usize>> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            AgentEntry::User { steered, .. } => {
                if !steered {
                    if let Some(prompt) = TurnPrompt::at(entries, i) {
                        turns.push(ReplyTurn { turn: turns.len(), prompt, steered: false });
                        last.push(None);
                    }
                } else if let Some(current) = turns.last_mut() {
                    current.steered = true;
                }
            }
            AgentEntry::Agent { text } if !text.trim().is_empty() => {
                if let Some(slot) = last.last_mut() {
                    *slot = Some(i);
                }
            }
            _ => {}
        }
    }
    turns.into_iter().zip(last).filter_map(|(turn, index)| Some((index?, turn))).collect()
}

/// What the actions of a reply may do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReplyOperations {
    /// Run the turn again with the same message.
    pub regenerate: bool,
    /// Copy the conversation up to this turn into a new task.
    pub fork: bool,
}

pub fn reply_operations(task: &AgentTask, reply: Option<&ReplyTurn>, turn_count: usize) -> ReplyOperations {
    let Some(reply) = reply else { return ReplyOperations::default() };
    let idle = task.status != AgentStatus::Archived && !is_busy(task.status);
    let latest = reply.turn + 1 == turn_count;
    let capabilities = task.capabilities;
    ReplyOperations {
        regenerate: idle
            && latest
            && capabilities.rollback
            && !reply.steered
            && !reply.prompt.text.trim().is_empty()
            && reply.prompt.attachments.len() == reply.prompt.images.len(),
        fork: idle && capabilities.fork && (capabilities.rollback || latest),
    }
}

/// "now", "3m", "2h", "5d" since `ms`.
pub fn age(ms: f64, now: f64) -> String {
    let s = ((now - ms) / 1000.0).round().max(0.0);
    if s < 60.0 {
        return "now".to_owned();
    }
    let m = (s / 60.0).round();
    if m < 60.0 {
        return format!("{m}m");
    }
    let h = (m / 60.0).round();
    if h < 24.0 {
        return format!("{h}h");
    }
    format!("{}d", (h / 24.0).round())
}

/// A status change worth a notification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Attention {
    Finished,
    Failed,
    Input,
}

pub fn attention_change(prev: Option<AgentStatus>, next: AgentStatus) -> Option<Attention> {
    let prev = prev.filter(|prev| *prev != next)?;
    if next == AgentStatus::NeedsInput {
        return Some(Attention::Input);
    }
    if !is_busy(prev) {
        return None;
    }
    match next {
        AgentStatus::Ready => Some(Attention::Finished),
        AgentStatus::Failed | AgentStatus::Limited => Some(Attention::Failed),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::fixtures::{agent, task, user};

    #[test]
    fn groups_tasks_by_what_they_need_newest_first() {
        let tasks = [
            task("a", AgentStatus::Ready, 1.0),
            task("b", AgentStatus::NeedsInput, 2.0),
            task("c", AgentStatus::Running, 3.0),
            task("d", AgentStatus::Starting, 4.0),
            task("e", AgentStatus::Ready, 5.0),
        ];
        let groups: Vec<(GroupStatus, Vec<&str>)> = group_tasks(&tasks)
            .into_iter()
            .map(|g| (g.status, g.tasks.iter().map(|t| t.id.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            vec![
                (GroupStatus::NeedsInput, vec!["b"]),
                (GroupStatus::Running, vec!["d", "c"]),
                (GroupStatus::Ready, vec!["e", "a"]),
            ]
        );
    }

    #[test]
    fn upserts_entries_in_place_and_refuses_gaps() {
        let mut entries = vec![agent("a")];
        assert!(upsert_entry(&mut entries, 0, agent("b")));
        assert_eq!(entries, vec![agent("b")]);
        let mut entries = vec![agent("a")];
        assert!(upsert_entry(&mut entries, 1, agent("b")));
        assert_eq!(entries, vec![agent("a"), agent("b")]);
        let mut entries = vec![agent("a")];
        assert!(!upsert_entry(&mut entries, 3, agent("b")));
        assert_eq!(entries, vec![agent("a")]);
    }

    #[test]
    fn notifies_on_finish_failure_and_questions_only() {
        use AgentStatus::*;
        assert_eq!(attention_change(Some(Running), Ready), Some(Attention::Finished));
        assert_eq!(attention_change(Some(Running), Failed), Some(Attention::Failed));
        assert_eq!(attention_change(Some(Running), Limited), Some(Attention::Failed));
        assert_eq!(attention_change(Some(Running), NeedsInput), Some(Attention::Input));
        assert_eq!(attention_change(Some(NeedsInput), Running), None);
        assert_eq!(attention_change(Some(Ready), Archived), None);
        assert_eq!(attention_change(None, Ready), None);
    }

    fn card(title: Option<&str>) -> AgentEntry {
        AgentEntry::Subagent {
            id: "c1".to_owned(),
            title: title.map(str::to_owned),
            description: Some("Find the lexer".to_owned()),
            status: AgentSubagentStatus::Running,
            error: None,
            prompt: None,
            reply: None,
            tools: 0,
            activity: None,
            started_ms: 0.0,
            ended_ms: None,
            cancellable: false,
        }
    }

    #[test]
    fn finds_subagent_cards_and_keys_their_conversations_apart() {
        let entries = [agent("a"), card(None)];
        assert_eq!(find_subagent(Some(entries.as_slice()), "c1"), Some(&entries[1]));
        assert_eq!(find_subagent(None, "c1"), None);
        assert_eq!(subagent_title(&card(None)), "Find the lexer");
        assert_eq!(subagent_title(&card(Some("Explore"))), "Explore");
        assert_eq!(entries_key("t", None), "t");
        assert_ne!(entries_key("t", Some("c1")), entries_key("t", None));
    }

    #[test]
    fn formats_labels() {
        assert_eq!(branch_label(Some("agent/fix-lexer")), "fix-lexer");
        assert_eq!(branch_label(None), "");
        assert_eq!(age(0.0, 30_000.0), "now");
        assert_eq!(age(0.0, 5.0 * 60_000.0), "5m");
        assert_eq!(age(0.0, 3.0 * 3_600_000.0), "3h");
    }

    #[test]
    fn numbers_the_messages_that_start_a_turn_not_those_steered_into_one() {
        let entries = [user("first", false), agent("ok"), user("also", true), user("second", false), agent("done")];
        assert_eq!(turn_numbers(&entries).into_iter().collect::<Vec<_>>(), vec![(0, 0), (3, 1)]);
    }

    #[test]
    fn offers_conversation_actions_only_on_the_final_nonempty_reply_of_each_turn() {
        let entries = [
            agent("Unassociated reply"),
            user("Original request", false),
            agent("Progress"),
            user("Additional instruction", true),
            agent("Done"),
            user("Next request", false),
            agent("Next answer"),
            agent(" "),
        ];
        let result: Vec<(usize, usize, bool)> =
            reply_turns(&entries).into_iter().map(|(index, reply)| (index, reply.turn, reply.steered)).collect();
        assert_eq!(result, vec![(4, 0, true), (6, 1, false)]);
    }

    fn reply_task(status: AgentStatus, fork: bool, rollback: bool) -> AgentTask {
        let mut t = task("t1", status, 0.0);
        t.capabilities.fork = fork;
        t.capabilities.rollback = rollback;
        t.capabilities.steer = true;
        t.capabilities.images = true;
        t
    }

    #[test]
    fn enables_conversation_actions_when_status() {
        let entries = [user("Original request", false)];
        let reply = ReplyTurn { turn: 0, prompt: TurnPrompt::at(&entries, 0).expect("a prompt"), steered: false };
        for (status, enabled) in [
            (AgentStatus::Ready, true),
            (AgentStatus::Running, false),
            (AgentStatus::Starting, false),
            (AgentStatus::NeedsInput, false),
            (AgentStatus::Archived, false),
        ] {
            assert_eq!(
                reply_operations(&reply_task(status, true, true), Some(&reply), 1),
                ReplyOperations { regenerate: enabled, fork: enabled },
                "{status:?}"
            );
        }
    }

    #[test]
    fn requires_rollback_for_regeneration_and_forking_earlier_turns() {
        let entries = [user("Original request", false)];
        let older = ReplyTurn { turn: 0, prompt: TurnPrompt::at(&entries, 0).expect("a prompt"), steered: false };
        let task = reply_task(AgentStatus::Ready, true, true);
        assert_eq!(reply_operations(&task, Some(&older), 2), ReplyOperations { regenerate: false, fork: true });
        let no_rollback = reply_task(AgentStatus::Ready, true, false);
        assert_eq!(reply_operations(&no_rollback, Some(&older), 2), ReplyOperations::default());
        assert_eq!(reply_operations(&no_rollback, Some(&older), 1), ReplyOperations { regenerate: false, fork: true });
    }
}
