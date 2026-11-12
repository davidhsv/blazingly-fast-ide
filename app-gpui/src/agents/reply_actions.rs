//! The actions of a reply in the conversation: copy its Markdown, quote it, regenerate it, branch the
//! conversation from it, and retract a sent message. Only one operation that changes a task's
//! conversation runs at a time; a failure is reported as a notification and can be retried.

use crate::agents::model::{TurnPrompt, reply_operations, reply_turns, turn_numbers};
use crate::agents::store::{AgentEffect, AgentsStore, ConfirmRequest, Outcome, Ticket};
use crate::platform::i18n::I18n;
use crate::workbench::notifications::{NotificationLevel, NotificationRequest};
use ide_agent::{AgentImage, AgentStatus};
use std::collections::{HashMap, HashSet};

/// How long a Copy button says "Copied".
pub const COPIED_MS: f64 = 2000.0;

/// The reply a Regenerate, Branch or Retract action is for: a task and the number of a turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplyTarget {
    pub task: String,
    pub turn: usize,
}

fn error(title: &str, body: &str, i18n: &I18n) -> AgentEffect {
    let mut request = NotificationRequest::new(NotificationLevel::Error, i18n.tx(title));
    request.body = Some(body.to_owned());
    AgentEffect::Notify(Box::new(request))
}

/// A Copy Markdown button: what it writes to the clipboard and whether it says "Copied".
#[derive(Clone, Debug, Default)]
pub struct CopyFeedback {
    copied: Option<(String, f64)>,
}

impl CopyFeedback {
    /// The text to write to the clipboard; `None` for an empty reply, which offers no copying.
    pub fn copy(text: &str) -> Option<&str> {
        (!text.trim().is_empty()).then_some(text)
    }

    /// The clipboard's answer for `text`: success is confirmed briefly, a failure is reported.
    pub fn copied(&mut self, text: &str, result: Result<(), String>, now: f64, i18n: &I18n) -> Option<AgentEffect> {
        match result {
            Ok(()) => {
                self.copied = Some((text.to_owned(), now));
                None
            }
            Err(e) => Some(error("Couldn't copy the reply", &e, i18n)),
        }
    }

    /// Whether the button shows "Copied" for the reply now showing `text` (a reply that streamed on
    /// since does not).
    pub fn shows_copied(&self, text: &str, now: f64) -> bool {
        self.copied.as_ref().is_some_and(|(copied, at)| copied == text && now - at < COPIED_MS)
    }
}

#[derive(Clone, Debug)]
struct Running {
    task: String,
    title: &'static str,
}

/// A retraction waiting for its confirmation.
#[derive(Clone, Debug)]
struct Retraction {
    task: String,
    turn: u32,
    text: String,
    images: Vec<AgentImage>,
}

/// The reply operations under way.
#[derive(Debug, Default)]
pub struct ReplyActions {
    /// Tasks with an operation under way: further clicks are ignored.
    busy: HashSet<String>,
    running: HashMap<Ticket, Running>,
    confirming: HashMap<Ticket, Retraction>,
}

fn turn_u32(turn: usize) -> u32 {
    u32::try_from(turn).unwrap_or(u32::MAX)
}

impl ReplyActions {
    pub fn is_busy(&self, task: &str) -> bool {
        self.busy.contains(task)
    }

    fn start(&mut self, ticket: Ticket, task: &str, title: &'static str) {
        self.busy.insert(task.to_owned());
        self.running.insert(ticket, Running { task: task.to_owned(), title });
    }

    /// Quote Reply: the raw Markdown goes into the task's composer; nothing is sent. Archived tasks
    /// take no quotes.
    pub fn quote(&self, store: &mut AgentsStore, task: &str, text: &str) -> bool {
        let open = store.task(task).is_some_and(|t| t.status != AgentStatus::Archived);
        if !open || text.trim().is_empty() {
            return false;
        }
        store.quote_in_composer(task, text);
        true
    }

    /// Regenerate Response: runs the reply's turn again with its original message and images.
    pub fn regenerate(
        &mut self,
        store: &mut AgentsStore,
        target: &ReplyTarget,
        now: f64,
        i18n: &I18n,
    ) -> Vec<AgentEffect> {
        if self.is_busy(&target.task) {
            return Vec::new();
        }
        let Some((text, images)) = resolve(store, target, |o| o.regenerate) else { return Vec::new() };
        let (ticket, effects) = store.rerun_from(&target.task, turn_u32(target.turn), &text, &images, now, i18n);
        self.start(ticket, &target.task, "Couldn't regenerate the response");
        effects
    }

    /// Branch Conversation from Here: copies the conversation up to and including the reply's turn.
    pub fn fork(&mut self, store: &mut AgentsStore, target: &ReplyTarget) -> Vec<AgentEffect> {
        if self.is_busy(&target.task) || resolve(store, target, |o| o.fork).is_none() {
            return Vec::new();
        }
        let (ticket, effect) = store.fork_task(&target.task, Some(turn_u32(target.turn + 1)));
        self.start(ticket, &target.task, "Couldn't copy the task");
        vec![effect]
    }

    /// Retract Message: drops the turn's message (and the later turns, after a confirmation) and puts
    /// it back in the composer. Needs an agent that can roll back.
    pub fn retract(
        &mut self,
        store: &mut AgentsStore,
        target: &ReplyTarget,
        now: f64,
        i18n: &I18n,
    ) -> Vec<AgentEffect> {
        let Some(task) = store.task(&target.task) else { return Vec::new() };
        if task.status == AgentStatus::Archived || !task.capabilities.rollback || self.is_busy(&target.task) {
            return Vec::new();
        }
        let Some(entries) = store.entries(&target.task, None) else { return Vec::new() };
        let turns = turn_numbers(entries);
        let Some(index) = turns.iter().find(|(_, turn)| **turn == target.turn).map(|(index, _)| *index) else {
            return Vec::new();
        };
        let Some(prompt) = TurnPrompt::at(entries, index) else { return Vec::new() };
        let retraction = Retraction {
            task: target.task.clone(),
            turn: turn_u32(target.turn),
            text: prompt.text.to_owned(),
            images: prompt.images.to_vec(),
        };
        let later = target.turn + 1 < turns.len();
        if later {
            let ticket = store.new_ticket();
            self.busy.insert(target.task.clone());
            self.confirming.insert(ticket, retraction);
            let request = ConfirmRequest {
                title: i18n.tx("Retract Message"),
                message: i18n.tx("The messages after this one are replaced. Files the agent changed stay as they are."),
                ok: i18n.tx("Retract"),
            };
            return vec![AgentEffect::Confirm { ticket, request }];
        }
        self.retract_now(store, retraction, now)
    }

    fn retract_now(&mut self, store: &mut AgentsStore, r: Retraction, now: f64) -> Vec<AgentEffect> {
        let (ticket, effects) = store.retract_from(&r.task, r.turn, &r.text, &r.images, now);
        self.start(ticket, &r.task, "Couldn't retract the message");
        effects
    }

    /// The answer of a confirmation this asked for; `None` when the ticket is not its own.
    pub fn confirmed(
        &mut self,
        store: &mut AgentsStore,
        ticket: Ticket,
        ok: bool,
        now: f64,
    ) -> Option<Vec<AgentEffect>> {
        let retraction = self.confirming.remove(&ticket)?;
        if !ok {
            self.busy.remove(&retraction.task);
            return Some(Vec::new());
        }
        Some(self.retract_now(store, retraction, now))
    }

    /// A request ended ([`AgentEffect::Finished`]): the task takes reply operations again, and a failure
    /// of one of them is reported.
    pub fn finished(&mut self, ticket: Ticket, outcome: &Outcome, i18n: &I18n) -> Option<AgentEffect> {
        let running = self.running.remove(&ticket)?;
        self.busy.remove(&running.task);
        outcome.as_ref().err().map(|e| error(running.title, e, i18n))
    }
}

/// The message of the reply's turn, when `allowed` grants the operation on it.
fn resolve(
    store: &AgentsStore,
    target: &ReplyTarget,
    allowed: impl Fn(&crate::agents::model::ReplyOperations) -> bool,
) -> Option<(String, Vec<AgentImage>)> {
    let task = store.task(&target.task)?;
    let entries = store.entries(&target.task, None)?;
    let replies = reply_turns(entries);
    let reply = replies.values().find(|r| r.turn == target.turn)?;
    let operations = reply_operations(task, Some(reply), turn_numbers(entries).len());
    allowed(&operations).then(|| (reply.prompt.text.to_owned(), reply.prompt.images.to_vec()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::fixtures::{agent, local_task, user};
    use crate::agents::store::{AgentCall, CallAnswer, Editors, Saved};
    use ide_agent::{AgentEntry, AgentTask};

    fn task() -> AgentTask {
        let mut t = local_task("t1");
        t.capabilities.fork = true;
        t.capabilities.rollback = true;
        t.capabilities.steer = true;
        t.capabilities.images = true;
        t
    }

    fn store_with(task: AgentTask, entries: Vec<AgentEntry>) -> AgentsStore {
        let mut store = AgentsStore::new(Saved::default());
        let effects = store.connect(true);
        let Some(AgentEffect::Subscribe { generation, .. }) = effects.first() else { panic!("a subscription") };
        store.connected(*generation, Ok((vec![task], Vec::new())), &Editors::default());
        store.entries_loaded("t1", None, entries);
        store
    }

    fn conversation() -> Vec<AgentEntry> {
        vec![user("Original request", false), agent("**Answer**")]
    }

    fn target(turn: usize) -> ReplyTarget {
        ReplyTarget { task: "t1".into(), turn }
    }

    fn calls(effects: &[AgentEffect]) -> Vec<(Option<Ticket>, AgentCall)> {
        effects
            .iter()
            .filter_map(|e| match e {
                AgentEffect::Call { ticket, call } => Some((*ticket, call.clone())),
                _ => None,
            })
            .collect()
    }

    /// Confirms the fresh-session rerun a regeneration of the first turn asks for.
    fn regenerate(actions: &mut ReplyActions, store: &mut AgentsStore) -> Vec<AgentEffect> {
        let i18n = I18n::default();
        let effects = actions.regenerate(store, &target(0), 0.0, &i18n);
        match effects.first() {
            Some(AgentEffect::Confirm { ticket, .. }) => store.confirmed(*ticket, true, 0.0),
            _ => effects,
        }
    }

    fn rollback(effects: &[AgentEffect]) -> Option<u32> {
        calls(effects).into_iter().find_map(|(_, c)| match c {
            AgentCall::Rollback { turns, .. } => Some(turns),
            _ => None,
        })
    }

    #[test]
    fn quotes_raw_markdown_without_sending_a_prompt() {
        let mut store = store_with(task(), conversation());
        let actions = ReplyActions::default();
        assert!(actions.quote(&mut store, "t1", "**Answer**"));
        assert_eq!(store.composer("t1").text, "> **Answer**\n\n");
        let mut archived = task();
        archived.status = AgentStatus::Archived;
        let mut store = store_with(archived, conversation());
        assert!(!actions.quote(&mut store, "t1", "More"));
        assert_eq!(store.composer("t1").text, "");
    }

    #[test]
    fn regenerates_using_the_original_prompt_and_its_images() {
        let image = AgentImage { mime_type: "image/png".into(), data: "AAA".into(), name: Some("shot.png".into()) };
        let prompt = AgentEntry::User {
            text: "Original request".into(),
            attachments: vec!["shot.png".into()],
            images: vec![image.clone()],
            sent_ms: Some(0.0),
            done_ms: Some(1.0),
            steered: false,
        };
        let mut store = store_with(task(), vec![prompt, agent("**Answer**")]);
        let mut actions = ReplyActions::default();
        let effects = regenerate(&mut actions, &mut store);
        assert_eq!(rollback(&effects), Some(0));
        let (ticket, _) = calls(&effects)[0].clone();
        let effects = store.call_answered(ticket.expect("a ticket"), Ok(CallAnswer::RolledBack(None)));
        let sent = calls(&effects);
        let AgentCall::Prompt { prompt, .. } = &sent[0].1 else { panic!("a prompt") };
        assert_eq!((prompt.text.as_str(), prompt.images.clone()), ("Original request", vec![image]));
    }

    #[test]
    fn does_not_replay_an_invalid_request() {
        let i18n = I18n::default();
        let files = AgentEntry::User {
            text: "Original request".into(),
            attachments: vec!["main.rs".into()],
            images: Vec::new(),
            sent_ms: Some(0.0),
            done_ms: Some(1.0),
            steered: false,
        };
        let mut busy = task();
        busy.status = AgentStatus::Running;
        let mut unsupported = task();
        unsupported.capabilities = Default::default();
        let cases: Vec<(&str, AgentTask, Vec<AgentEntry>)> = vec![
            ("files", task(), vec![files, agent("**Answer**")]),
            ("steered", task(), [conversation(), vec![user("Original request", true)]].concat()),
            ("earlier", task(), [conversation(), conversation()].concat()),
            ("busy", busy, conversation()),
            ("unsupported", unsupported, conversation()),
            ("subagent", task(), vec![agent("Child reply")]),
        ];
        for (kind, task, entries) in cases {
            let mut store = store_with(task, entries);
            let effects = ReplyActions::default().regenerate(&mut store, &target(0), 0.0, &i18n);
            assert!(effects.is_empty(), "{kind}");
        }
    }

    #[test]
    fn branches_after_the_reply_s_turn() {
        let mut store = store_with(task(), conversation());
        let effects = ReplyActions::default().fork(&mut store, &target(0));
        assert_eq!(calls(&effects)[0].1, AgentCall::Fork { task: "t1".into(), turns: Some(1) });
    }

    #[test]
    fn reports_regeneration_errors_and_allows_retry() {
        let i18n = I18n::default();
        let mut store = store_with(task(), conversation());
        let mut actions = ReplyActions::default();
        let effects = regenerate(&mut actions, &mut store);
        let ticket = calls(&effects)[0].0.expect("a ticket");
        let effects = store.call_answered(ticket, Err("Unavailable".into()));
        let finished = effects.iter().find_map(|e| match e {
            AgentEffect::Finished { ticket, outcome } => Some((*ticket, outcome.clone())),
            _ => None,
        });
        let (ticket, outcome) = finished.expect("the regeneration ended");
        let Some(AgentEffect::Notify(request)) = actions.finished(ticket, &outcome, &i18n) else {
            panic!("an error notification")
        };
        let expected = (NotificationLevel::Error, "Couldn't regenerate the response");
        assert_eq!((request.level, request.title.as_str()), expected);
        assert_eq!(rollback(&regenerate(&mut actions, &mut store)), Some(0));
    }

    #[test]
    fn ignores_duplicate_clicks_while_regeneration_is_pending() {
        let i18n = I18n::default();
        let mut store = store_with(task(), conversation());
        let mut actions = ReplyActions::default();
        assert!(!regenerate(&mut actions, &mut store).is_empty());
        assert!(actions.regenerate(&mut store, &target(0), 0.0, &i18n).is_empty());
    }

    #[test]
    fn confirms_before_retracting_a_turn_with_later_messages_and_guards_repeated_clicks() {
        let i18n = I18n::default();
        let transcript = vec![user("Original message", false), agent("a"), user("Follow up", false), agent("b")];
        let mut store = store_with(task(), transcript.clone());
        let mut actions = ReplyActions::default();
        let effects = actions.retract(&mut store, &target(0), 0.0, &i18n);
        let Some(AgentEffect::Confirm { ticket, .. }) = effects.first() else { panic!("a confirmation") };
        let ticket = *ticket;
        assert!(actions.retract(&mut store, &target(0), 0.0, &i18n).is_empty());
        assert_eq!(actions.confirmed(&mut store, ticket, false, 0.0).map(|e| e.len()), Some(0));
        assert_eq!(store.entries("t1", None), Some(transcript.as_slice()));
        let effects = actions.retract(&mut store, &target(0), 0.0, &i18n);
        let Some(AgentEffect::Confirm { ticket, .. }) = effects.first() else { panic!("a confirmation") };
        let effects = actions.confirmed(&mut store, *ticket, true, 0.0).expect("its own confirmation");
        assert_eq!(rollback(&effects), Some(0));
        let ticket = calls(&effects)[0].0.expect("a ticket");
        store.call_answered(ticket, Ok(CallAnswer::RolledBack(None)));
        assert_eq!(store.composer("t1").text, "Original message");
    }

    #[test]
    fn copies_the_exact_markdown_source_and_briefly_confirms_success() {
        let i18n = I18n::default();
        let reply = "**OpenIDEA v0.1.38 is out!**\n\n## Highlights\n\n```sh\npnpm tauri dev\n```\n";
        let mut button = CopyFeedback::default();
        assert_eq!(CopyFeedback::copy(reply), Some(reply));
        assert!(button.copied(reply, Ok(()), 0.0, &i18n).is_none());
        assert!(button.shows_copied(reply, 1999.0));
        assert!(!button.shows_copied(reply, COPIED_MS));
    }

    #[test]
    fn copies_the_latest_streamed_text_and_clears_feedback_when_content_changes() {
        let i18n = I18n::default();
        let mut button = CopyFeedback::default();
        button.copied("**Partial**", Ok(()), 0.0, &i18n);
        assert!(!button.shows_copied("**Partial** and more", 10.0));
        assert_eq!(CopyFeedback::copy("**Partial** and more"), Some("**Partial** and more"));
    }

    #[test]
    fn reports_a_clipboard_failure_without_claiming_success_and_allows_retry() {
        let i18n = I18n::default();
        let mut button = CopyFeedback::default();
        let Some(AgentEffect::Notify(request)) = button.copied("x", Err("denied".into()), 0.0, &i18n) else {
            panic!("an error notification")
        };
        assert_eq!((request.level, request.title.as_str()), (NotificationLevel::Error, "Couldn't copy the reply"));
        assert!(!button.shows_copied("x", 1.0));
        button.copied("x", Ok(()), 2.0, &i18n);
        assert!(button.shows_copied("x", 3.0));
    }

    #[test]
    fn does_not_offer_copying_an_empty_reply() {
        for text in ["", " \n\t"] {
            assert_eq!(CopyFeedback::copy(text), None, "{text:?}");
        }
    }
}
