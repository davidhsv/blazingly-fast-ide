use super::*;
use crate::agents::fixtures::{agent, local_task, option, path, user};
use ide_agent::{AgentCommandReadOnlyReason, AgentSource, AgentToolDiff};

struct Harness {
    store: AgentsStore,
    generation: u64,
    i18n: I18n,
    effects: Vec<AgentEffect>,
}

fn running(id: &str) -> AgentTask {
    let mut t = local_task(id);
    t.status = AgentStatus::Running;
    t
}

fn with_status(id: &str, status: AgentStatus) -> AgentTask {
    let mut t = local_task(id);
    t.status = status;
    t
}

fn generation_of(effects: &[AgentEffect]) -> u64 {
    effects
        .iter()
        .find_map(|e| match e {
            AgentEffect::Subscribe { generation, .. } => Some(*generation),
            _ => None,
        })
        .expect("a subscription")
}

impl Harness {
    fn new(tasks: Vec<AgentTask>) -> Self {
        let mut store = AgentsStore::new(Saved::default());
        let generation = generation_of(&store.connect(true));
        let effects = store.connected(generation, Ok((tasks, Vec::new())), &Editors::default());
        Self { store, generation, i18n: I18n::default(), effects }
    }

    fn emit(&mut self, event: AgentEvent) -> &[AgentEffect] {
        self.effects = self.store.on_event(self.generation, event, &self.i18n);
        &self.effects
    }

    fn task(&mut self, task: AgentTask) -> &[AgentEffect] {
        self.emit(AgentEvent::Task { task })
    }

    fn reply(&mut self, id: &str, text: &str) {
        self.emit(AgentEvent::Entry { task: id.into(), subagent: None, index: 0, entry: agent(text) });
    }

    fn unread(&self) -> Vec<&str> {
        self.store.unread().iter().map(String::as_str).collect()
    }
}

fn notifications(effects: &[AgentEffect]) -> usize {
    effects.iter().filter(|e| matches!(e, AgentEffect::Notify(_))).count()
}

fn titles_changed(effects: &[AgentEffect]) -> usize {
    effects.iter().filter(|e| matches!(e, AgentEffect::TabTitlesChanged)).count()
}

fn closed(effects: &[AgentEffect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|e| match e {
            AgentEffect::CloseTab { key } => Some(key.as_str()),
            _ => None,
        })
        .collect()
}

fn opened(effects: &[AgentEffect]) -> Vec<&str> {
    effects
        .iter()
        .filter_map(|e| match e {
            AgentEffect::OpenSession { task, .. } => Some(task.as_str()),
            _ => None,
        })
        .collect()
}

fn shows_tool_window(effects: &[AgentEffect]) -> bool {
    effects.iter().any(|e| matches!(e, AgentEffect::ShowToolWindow { id: TOOL_WINDOW }))
}

/// The calls sent, with their tickets.
fn calls(effects: &[AgentEffect]) -> Vec<(Option<Ticket>, AgentCall)> {
    effects
        .iter()
        .filter_map(|e| match e {
            AgentEffect::Call { ticket, call } => Some((*ticket, call.clone())),
            _ => None,
        })
        .collect()
}

fn call_ticket(effects: &[AgentEffect], matches: impl Fn(&AgentCall) -> bool) -> Ticket {
    calls(effects).into_iter().find(|(_, c)| matches(c)).and_then(|(t, _)| t).expect("the call with a ticket")
}

fn is_cancel(call: &AgentCall) -> bool {
    matches!(call, AgentCall::Cancel { .. })
}

fn is_rollback(call: &AgentCall) -> bool {
    matches!(call, AgentCall::Rollback { .. })
}

fn is_prompt(call: &AgentCall) -> bool {
    matches!(call, AgentCall::Prompt { .. })
}

fn outcome(effects: &[AgentEffect], ticket: Ticket) -> Option<Outcome> {
    effects.iter().find_map(|e| match e {
        AgentEffect::Finished { ticket: t, outcome } if *t == ticket => Some(outcome.clone()),
        _ => None,
    })
}

fn image(data: &str, name: &str) -> AgentImage {
    AgentImage { mime_type: "image/png".into(), data: data.into(), name: Some(name.into()) }
}

// ----- draft.test.ts -----

#[test]
fn runs_tasks_in_the_local_checkout_unless_the_user_picked_a_worktree() {
    assert_eq!(saved_draft(Some("{}")).isolation, Isolation::Local);
    // Saved before the choice was recorded: `worktree` was only the old default.
    let old = saved_draft(Some(r#"{"provider":"codex","isolation":"worktree"}"#));
    assert_eq!((old.provider.as_str(), old.isolation), ("codex", Isolation::Local));
    assert_eq!(saved_draft(Some(r#"{"isolation":"worktree","isolationChosen":true}"#)).isolation, Isolation::Worktree);
    assert_eq!(saved_draft(Some(r#"{"isolation":"local","isolationChosen":true}"#)).isolation, Isolation::Local);
}

// ----- unread.test.tsx -----

fn unread_harness() -> Harness {
    Harness::new(vec![running("one"), with_status("two", AgentStatus::Ready)])
}

#[test]
fn refreshes_restored_tab_titles_when_tasks_finish_loading() {
    let mut h = unread_harness();
    h.store.disconnect();
    let mut renamed = running("one");
    renamed.title = "Saved rename".into();
    let generation = generation_of(&h.store.connect(true));
    let effects = h.store.connected(generation, Ok((vec![renamed], Vec::new())), &Editors::default());
    assert_eq!(h.store.task("one").map(|t| t.title.as_str()), Some("Saved rename"));
    assert_eq!(titles_changed(&effects), 1);
}

#[test]
fn refreshes_titles_for_user_and_provider_renames_but_not_unchanged_task_updates() {
    let mut h = unread_harness();
    let mut renamed = running("one");
    renamed.title = "User rename".into();
    assert_eq!(titles_changed(h.task(renamed.clone())), 1);
    renamed.updated_ms = 1.0;
    assert_eq!(titles_changed(h.task(renamed)), 0);
    let mut provider = with_status("two", AgentStatus::Ready);
    provider.title = "Provider title".into();
    assert_eq!(titles_changed(h.task(provider)), 1);
}

#[test]
fn does_not_mark_historical_tasks_unread_but_tracks_replies_even_without_loaded_entries() {
    let mut h = unread_harness();
    assert!(h.unread().is_empty());
    h.reply("one", "New reply");
    h.reply("one", "New reply, streamed further");
    h.reply("two", "New reply");
    assert_eq!(h.unread(), vec!["one", "two"]);
    assert_eq!(h.store.loaded_conversations(), 0);
    assert_eq!(notifications(&h.effects), 0);
}

#[test]
fn marks_background_status_transitions_unread() {
    for status in [AgentStatus::Ready, AgentStatus::NeedsInput, AgentStatus::Failed, AgentStatus::Limited] {
        let mut h = unread_harness();
        assert_eq!(notifications(h.task(with_status("one", status))), 1, "{status:?}");
        assert!(h.store.is_unread("one"));
        h.store.show_session("one");
        assert_eq!(notifications(h.task(with_status("one", status))), 0);
        assert!(h.unread().is_empty());
    }
}

#[test]
fn keeps_the_latest_usage_limits_per_provider_without_marking_anything_unread() {
    let mut h = unread_harness();
    let five_hour = |used_percent| vec![AgentLimit { window: "five_hour".into(), used_percent, resets_ms: None }];
    h.emit(AgentEvent::Limits { provider: "claude".into(), limits: five_hour(10.0) });
    h.emit(AgentEvent::Limits { provider: "codex".into(), limits: five_hour(50.0) });
    h.emit(AgentEvent::Limits { provider: "claude".into(), limits: five_hour(12.0) });
    assert_eq!(h.store.limits("claude"), Some(five_hour(12.0).as_slice()));
    assert_eq!(h.store.limits("codex"), Some(five_hour(50.0).as_slice()));
    assert!(h.unread().is_empty());
}

#[test]
fn ignores_user_messages_thoughts_empty_replies_and_subagent_streaming() {
    let mut h = unread_harness();
    for entry in [user("Do this", false), AgentEntry::Thought { text: "Thinking".into() }, agent(" ")] {
        h.emit(AgentEvent::Entry { task: "one".into(), subagent: None, index: 0, entry });
    }
    h.emit(AgentEvent::Entry { task: "one".into(), subagent: Some("child".into()), index: 0, entry: agent("Working") });
    let mut renamed = running("one");
    renamed.title = "Renamed".into();
    h.task(renamed);
    assert!(h.unread().is_empty());
}

#[test]
fn clears_only_the_viewed_task_and_keeps_reading_while_either_split_shows_it() {
    let mut h = unread_harness();
    h.reply("one", "New reply");
    h.reply("two", "New reply");
    h.store.show_session("one");
    h.store.show_session("one");
    h.store.hide_session("one");
    h.reply("one", "New reply");
    assert_eq!(notifications(h.task(with_status("one", AgentStatus::Ready))), 0);
    assert_eq!(h.unread(), vec!["two"]);
}

#[test]
fn keeps_a_visible_session_unread_while_the_window_is_unfocused_then_clears_on_focus() {
    let mut h = unread_harness();
    h.store.set_window_active(false);
    h.reply("one", "New reply");
    h.store.show_session("one");
    assert!(h.store.is_unread("one"));
    h.store.set_window_active(true);
    assert!(h.unread().is_empty());
    h.reply("one", "New reply");
    assert!(h.unread().is_empty());
}

#[test]
fn does_not_read_hidden_windows_and_removes_focus_listeners_when_the_session_closes() {
    let mut h = unread_harness();
    // Hidden: focusing it does not make it active.
    h.store.set_window_active(false);
    h.store.show_session("one");
    h.reply("one", "New reply");
    assert!(h.store.is_unread("one"));
    h.store.set_window_active(true);
    assert!(h.unread().is_empty());
    h.store.hide_session("one");
    h.reply("one", "New reply");
    h.store.set_window_active(true);
    assert!(h.store.is_unread("one"));
}

#[test]
fn cleans_up_archived_and_deleted_tasks_ignores_late_replies_and_resets_on_project_switch() {
    let mut h = unread_harness();
    h.reply("one", "New reply");
    h.reply("two", "New reply");
    h.task(with_status("one", AgentStatus::Archived));
    h.reply("one", "New reply");
    assert_eq!(h.unread(), vec!["two"]);
    h.emit(AgentEvent::Removed { id: "two".into() });
    h.reply("two", "New reply");
    assert!(h.unread().is_empty());
    h.task(running("one"));
    h.reply("one", "New reply");
    let old = h.generation;
    h.store.disconnect();
    let generation = generation_of(&h.store.connect(true));
    h.store.connected(generation, Ok((vec![running("other")], Vec::new())), &Editors::default());
    h.store.on_event(old, AgentEvent::Task { task: with_status("one", AgentStatus::Ready) }, &h.i18n);
    assert!(h.unread().is_empty());
    assert_eq!(h.store.tasks().iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["other"]);
}

#[test]
fn shows_a_dot_on_the_task_and_a_clickable_status_count_even_when_all_tasks_are_idle() {
    let mut h = unread_harness();
    h.task(with_status("one", AgentStatus::Ready));
    assert!(h.store.is_unread("one"));
    let item = h.store.status_item().expect("a status item");
    assert_eq!((item.unread, item.target), (1, Some("one")));
    assert_eq!(opened(&h.store.open_task("one")), vec!["one"]);
    h.store.show_session("one");
    assert!(!h.store.is_unread("one"));
    assert_eq!(h.store.status_item(), None);
}

#[test]
fn counts_unread_and_needs_input_tasks_once_each_in_the_sidebar_badge() {
    let mut h = Harness::new(vec![with_status("one", AgentStatus::NeedsInput), with_status("two", AgentStatus::Ready)]);
    h.reply("one", "New reply");
    h.reply("two", "New reply");
    assert_eq!(h.store.badge(), Some(2));
    h.store.show_session("two");
    assert_eq!(h.store.badge(), Some(1));
    h.store.show_session("one");
    assert_eq!(h.store.badge(), Some(1));
}

fn sessions(ids: &[&str]) -> Vec<AgentTab> {
    ids.iter().map(|id| AgentTab::Session((*id).into())).collect()
}

#[test]
fn shows_the_task_in_the_tool_window_closing_its_editor_tab_and_undocks_back_to_one() {
    let mut h = unread_harness();
    let effects = h.store.dock_task("one", &Editors::default());
    assert_eq!(h.store.docked.as_deref(), Some("one"));
    assert!(shows_tool_window(&effects));
    assert!(closed(&effects).contains(&"agent:one"));
    let effects = h.store.undock_task(&Editors::default());
    assert_eq!(h.store.docked, None);
    assert_eq!(opened(&effects), vec!["one"]);
}

#[test]
fn undocks_a_removed_task() {
    let mut h = unread_harness();
    h.store.dock_task("one", &Editors::default());
    h.emit(AgentEvent::Removed { id: "one".into() });
    assert_eq!(h.store.docked, None);
}

#[test]
fn drops_a_dragged_task_into_the_editor_undocking_when_docked_opening_a_tab_otherwise() {
    let mut h = unread_harness();
    let effects = h.store.open_task_in_editor("two", &Editors::default());
    assert_eq!(h.store.docked, None);
    assert_eq!(opened(&effects), vec!["two"]);
    h.store.dock_task("one", &Editors::default());
    let effects = h.store.open_task_in_editor("one", &Editors::default());
    assert_eq!(h.store.docked, None);
    assert_eq!(opened(&effects), vec!["one"]);
}

#[test]
fn moves_the_active_session_and_applies_the_header_choice_to_every_subsequent_session() {
    let mut h = unread_harness();
    let editors = Editors { active: Some(AgentTab::Session("two".into())), open: sessions(&["one", "two"]) };
    let effects = h.store.set_session_location(SessionLocation::ToolWindow, &editors);
    assert!(closed(&effects).contains(&"agent:one"));
    assert!(closed(&effects).contains(&"agent:two"));
    assert_eq!(h.store.docked.as_deref(), Some("two"));
    assert!(opened(&effects).is_empty());
    let effects = h.store.open_task("one");
    assert_eq!(h.store.docked.as_deref(), Some("one"));
    assert!(opened(&effects).is_empty());
    assert_eq!(h.store.session_location, SessionLocation::ToolWindow);
    let effects = h.store.set_session_location(SessionLocation::Editor, &Editors::default());
    let saved =
        |e: &AgentEffect| matches!(e, AgentEffect::Save { key: SESSION_LOCATION_KEY, value } if value == "\"editor\"");
    assert!(effects.iter().any(saved));
    assert_eq!(h.store.docked, None);
    assert_eq!(opened(&effects), vec!["one"]);
    assert_eq!(opened(&h.store.open_task("two")), vec!["two"]);
}

#[test]
fn opens_new_tasks_in_the_selected_location_and_moves_an_unfinished_draft_with_the_layout() {
    let mut h = unread_harness();
    h.store.set_session_location(SessionLocation::ToolWindow, &Editors::default());
    let effects = h.store.open_home(Some("Keep this draft"), None);
    assert!(h.store.home_open);
    assert_eq!(h.store.docked, None);
    assert!(!effects.iter().any(|e| matches!(e, AgentEffect::OpenHome | AgentEffect::OpenSession { .. })));
    let effects = h.store.set_session_location(SessionLocation::Editor, &Editors::default());
    assert!(!h.store.home_open);
    assert!(effects.iter().any(|e| matches!(e, AgentEffect::OpenHome)));
    assert_eq!(h.store.composer(HOME).text, "Keep this draft");
}

#[test]
fn drops_the_dock_when_the_project_changes() {
    let mut h = unread_harness();
    h.store.dock_task("one", &Editors::default());
    h.store.disconnect();
    assert_eq!(h.store.docked, None);
    assert_eq!(h.store.session_location, SessionLocation::ToolWindow);
}

// ----- sessionState.test.tsx (store parts) -----

#[test]
fn fetches_a_conversation_once_and_shows_it_again_from_the_store_when_its_tab_reopens() {
    let mut h = unread_harness();
    assert!(matches!(h.store.open_conversation("one", None), Some(AgentEffect::LoadEntries { .. })));
    h.store.entries_loaded("one", None, vec![agent("Answer A")]);
    assert!(h.store.open_conversation("one", None).is_none());
    assert_eq!(h.store.entries("one", None), Some([agent("Answer A")].as_slice()));
}

#[test]
fn restores_after_entries_load_and_clears_positions_on_project_disconnect() {
    let mut h = unread_harness();
    h.store.set_scroll_position("one", ScrollPosition { top: 230.0, stick: false });
    h.store.entries_loaded("one", None, vec![agent("Answer A")]);
    assert_eq!(h.store.scroll_position("one"), Some(ScrollPosition { top: 230.0, stick: false }));
    h.store.disconnect();
    assert_eq!(h.store.scroll_positions(), 0);
}

#[test]
fn keeps_task_and_subagent_scroll_positions_separate() {
    let mut h = unread_harness();
    h.store.set_scroll_position(&entries_key("one", None), ScrollPosition { top: 120.0, stick: false });
    h.store.set_scroll_position(&entries_key("one", Some("child")), ScrollPosition { top: 70.0, stick: false });
    assert_eq!(h.store.scroll_position(&entries_key("one", None)).map(|p| p.top), Some(120.0));
    assert_eq!(h.store.scroll_position(&entries_key("one", Some("child"))).map(|p| p.top), Some(70.0));
}

#[test]
fn reads_changes_again_once_after_a_burst_of_edits() {
    let mut h = unread_harness();
    h.store.entries_loaded("one", None, Vec::new());
    let edit = |index| AgentEvent::Entry {
        task: "one".into(),
        subagent: None,
        index,
        entry: AgentEntry::Tool {
            id: "t".into(),
            title: "Edit".into(),
            kind: "edit".into(),
            status: AgentToolStatus::Completed,
            locations: Vec::new(),
            output: None,
            diffs: vec![AgentToolDiff { path: path("/project/a.rs"), added: 1, removed: 0 }],
        },
    };
    let scheduled = |e: &[AgentEffect]| e.iter().filter(|e| matches!(e, AgentEffect::ScheduleChanges { .. })).count();
    assert_eq!(scheduled(h.emit(edit(0))), 1);
    assert_eq!(scheduled(h.emit(edit(1))), 0);
    assert!(matches!(h.store.changes_due("one"), Some(AgentEffect::RefreshChanges { .. })));
    assert!(h.store.changes_due("one").is_none());
    // A gap in the conversation reads it again.
    assert!(h.emit(edit(5)).iter().any(|e| matches!(e, AgentEffect::LoadEntries { .. })));
}

// ----- messageEditing.test.tsx -----

fn editing_harness(status: AgentStatus) -> Harness {
    let mut t = with_status("editing", status);
    t.capabilities.fork = true;
    t.capabilities.rollback = true;
    let mut h = Harness::new(vec![t]);
    h.store.entries_loaded("editing", None, vec![user("Original message", false), agent("Original answer")]);
    h
}

#[test]
fn keeps_the_transcript_and_running_session_when_a_fresh_session_rerun_is_cancelled() {
    let mut h = editing_harness(AgentStatus::Running);
    let (ticket, effects) = h.store.rerun_from("editing", 0, "Replacement", &[], 0.0, &h.i18n);
    let request = effects.iter().find_map(|e| match e {
        AgentEffect::Confirm { request, .. } => Some(request.clone()),
        _ => None,
    });
    assert!(request.expect("a confirmation").message.contains("clears the entire conversation"));
    let effects = h.store.confirmed(ticket, false, 0.0);
    assert!(calls(&effects).is_empty());
    assert_eq!(outcome(&effects, ticket), Some(Ok(false)));
    assert_eq!(h.store.entries("editing", None).map(<[AgentEntry]>::len), Some(2));
}

#[test]
fn stops_before_rollback_and_restores_text_images_without_sending() {
    for turn in [0, 1] {
        let mut h = editing_harness(AgentStatus::Running);
        let shot = image("AAA", "shot.png");
        h.store.update_composer("editing", |c| c.text = "Draft".into());
        let (ticket, effects) = h.store.retract_from("editing", turn, "Original", std::slice::from_ref(&shot), 0.0);
        let cancel = call_ticket(&effects, is_cancel);
        assert!(!calls(&effects).iter().any(|(_, c)| is_rollback(c)));
        assert!(h.store.call_answered(cancel, Ok(CallAnswer::Done)).is_empty());
        let effects = h.task(with_status("editing", AgentStatus::Ready)).to_vec();
        let sent = calls(&effects);
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].1, AgentCall::Rollback { task: "editing".into(), turns: turn });
        let effects = h.store.call_answered(sent[0].0.expect("a ticket"), Ok(CallAnswer::RolledBack(None)));
        assert!(!calls(&effects).iter().any(|(_, c)| is_prompt(c)));
        assert_eq!(outcome(&effects, ticket), Some(Ok(true)));
        assert_eq!(
            h.store.composer("editing"),
            ComposerDraft { text: "Original\nDraft".into(), files: Vec::new(), images: vec![shot] }
        );
        assert_eq!(h.store.focus().map(|f| f.key), Some("editing"));
    }
}

#[test]
fn keeps_the_draft_and_avoids_rollback_when_the_agent_does_not_stop_in_time() {
    let mut h = editing_harness(AgentStatus::Running);
    let (ticket, effects) = h.store.retract_from("editing", 1, "Original", &[], 0.0);
    let wake = effects.iter().find_map(|e| match e {
        AgentEffect::WakeAt { at_ms } => Some(*at_ms),
        _ => None,
    });
    assert_eq!(wake, Some(STOP_TIMEOUT_MS));
    assert!(h.store.tick(STOP_TIMEOUT_MS - 1.0, &h.i18n).is_empty());
    let effects = h.store.tick(STOP_TIMEOUT_MS, &h.i18n);
    let error = outcome(&effects, ticket).and_then(Result::err).expect("a failure");
    assert!(error.starts_with("The agent is still stopping"));
    assert!(calls(&effects).is_empty());
    assert_eq!(h.store.composer("editing").text, "Original");
    // The task stopping later no longer rolls anything back.
    assert!(calls(h.task(with_status("editing", AgentStatus::Ready))).iter().all(|(_, c)| !is_rollback(c)));
}

#[test]
fn recovers_the_original_prompt_when_rollback_fails() {
    let mut h = editing_harness(AgentStatus::Ready);
    let (ticket, effects) = h.store.retract_from("editing", 1, "Original", &[], 0.0);
    let rollback = call_ticket(&effects, is_rollback);
    let effects = h.store.call_answered(rollback, Err("Rollback failed".into()));
    assert_eq!(outcome(&effects, ticket), Some(Err("Rollback failed".into())));
    assert_eq!(h.store.composer("editing").text, "Original");
    assert!(calls(&effects).is_empty());
}

/// Runs a rerun of turn 0 through its confirmation; the effects then hold the rollback.
fn confirmed_rerun(h: &mut Harness, text: &str, images: &[AgentImage]) -> (Ticket, Vec<AgentEffect>) {
    let (ticket, _) = h.store.rerun_from("editing", 0, text, images, 0.0, &h.i18n);
    (ticket, h.store.confirmed(ticket, true, 0.0))
}

#[test]
fn recovers_images_after_failed_regeneration() {
    for draft in [false, true] {
        let mut h = editing_harness(AgentStatus::Ready);
        let shot = image("AAA", "shot.png");
        let added = image("BBB", "added.png");
        let (ticket, effects) = confirmed_rerun(&mut h, "Original message", std::slice::from_ref(&shot));
        let rollback = call_ticket(&effects, is_rollback);
        h.emit(AgentEvent::Truncated { task: "editing".into(), length: 0 });
        let effects = h.store.call_answered(rollback, Ok(CallAnswer::RolledBack(None)));
        let send = call_ticket(&effects, is_prompt);
        let files = if draft { vec![FileRef { path: "/project/a.ts".into(), rel: "a.ts".into() }] } else { vec![] };
        if draft {
            let (files, added) = (files.clone(), added.clone());
            h.store.update_composer("editing", |c| {
                *c = ComposerDraft { text: "Another draft @a.ts".into(), files, images: vec![added] }
            });
        }
        let effects = h.store.call_answered(send, Err("Send failed".into()));
        assert_eq!(outcome(&effects, ticket), Some(Err("Send failed".into())));
        let expected = ComposerDraft {
            text: if draft { "Original message\nAnother draft @a.ts".into() } else { "Original message".into() },
            files: files.clone(),
            images: if draft { vec![shot.clone(), added] } else { vec![shot.clone()] },
        };
        assert_eq!(h.store.composer("editing"), expected);
        assert_eq!(h.store.focus().map(|f| f.key), Some("editing"));
        let resend = h.store.send_message("editing", false).expect("a message to send");
        let AgentEffect::Call { call: AgentCall::Prompt { prompt, now, .. }, .. } = resend else { panic!("a prompt") };
        assert!(!now);
        assert_eq!(prompt.text, expected.text);
        assert_eq!(prompt.images, expected.images);
        assert_eq!(prompt.files.len(), files.len());
    }
}

#[test]
fn keeps_the_draft_and_shows_a_rollback_failure_beside_it() {
    let mut h = editing_harness(AgentStatus::Ready);
    let (ticket, effects) = confirmed_rerun(&mut h, "Edited message", &[]);
    let effects = h.store.call_answered(call_ticket(&effects, is_rollback), Err("Rollback failed".into()));
    assert_eq!(outcome(&effects, ticket), Some(Err("Rollback failed".into())));
    assert!(!calls(&effects).iter().any(|(_, c)| is_prompt(c)));
    assert!(!h.store.editing("editing"));
    let (_, effects) = confirmed_rerun(&mut h, "Edited message", &[]);
    let effects = h.store.call_answered(call_ticket(&effects, is_rollback), Ok(CallAnswer::RolledBack(None)));
    assert!(calls(&effects).iter().any(|(_, c)| is_prompt(c)));
}

#[test]
fn rolls_back_before_sending_the_edited_text_when_send_is_clicked() {
    let mut h = editing_harness(AgentStatus::Ready);
    let (ticket, effects) = confirmed_rerun(&mut h, "Edited message", &[]);
    let calls_now = calls(&effects);
    assert_eq!(calls_now.len(), 1);
    assert_eq!(calls_now[0].1, AgentCall::Rollback { task: "editing".into(), turns: 0 });
    assert!(h.store.editing("editing"));
    let effects = h.store.call_answered(calls_now[0].0.expect("a ticket"), Ok(CallAnswer::RolledBack(None)));
    let sent = calls(&effects);
    let prompt = AgentPrompt { text: "Edited message".into(), files: Vec::new(), images: Vec::new() };
    assert_eq!(sent[0].1, AgentCall::Prompt { task: "editing".into(), prompt, now: false });
    let effects = h.store.call_answered(sent[0].0.expect("a ticket"), Ok(CallAnswer::Done));
    assert_eq!(outcome(&effects, ticket), Some(Ok(true)));
}

#[test]
fn recovers_the_edit_after_rollback_if_sending_fails() {
    for draft in ["", "Another draft"] {
        let mut h = editing_harness(AgentStatus::Ready);
        h.store.update_composer("editing", |c| c.text = draft.into());
        let (_, effects) = confirmed_rerun(&mut h, "Edited message", &[]);
        let effects = h.store.call_answered(call_ticket(&effects, is_rollback), Ok(CallAnswer::RolledBack(None)));
        h.store.call_answered(call_ticket(&effects, is_prompt), Err("Send failed".into()));
        let expected = if draft.is_empty() { "Edited message".to_owned() } else { format!("Edited message\n{draft}") };
        assert_eq!(h.store.composer("editing").text, expected);
        assert_eq!(h.store.focus().map(|f| f.key), Some("editing"));
        assert!(h.store.send_message("editing", false).is_some());
    }
}

#[test]
fn waits_for_a_running_turn_to_stop_before_rolling_it_back() {
    let mut h = editing_harness(AgentStatus::Running);
    let (_, effects) = confirmed_rerun(&mut h, "Edited message", &[]);
    let sent = calls(&effects);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].1, AgentCall::Cancel { task: "editing".into(), subagent: None });
    let effects = h.task(with_status("editing", AgentStatus::Ready)).to_vec();
    let effects = h.store.call_answered(call_ticket(&effects, is_rollback), Ok(CallAnswer::RolledBack(None)));
    assert!(calls(&effects).iter().any(|(_, c)| is_prompt(c)));
}

#[test]
fn keeps_an_accessible_icon_only_send_action_with_submission() {
    let mut h = editing_harness(AgentStatus::Ready);
    assert!(h.store.send_message("editing", false).is_none());
    h.store.update_composer("editing", |c| c.text = "Follow up".into());
    let effect = h.store.send_message("editing", false).expect("a message to send");
    let sent = calls(std::slice::from_ref(&effect));
    let prompt = AgentPrompt { text: "Follow up".into(), files: Vec::new(), images: Vec::new() };
    assert_eq!(sent[0].1, AgentCall::Prompt { task: "editing".into(), prompt, now: false });
    assert_eq!(h.store.composer("editing").text, "Follow up");
    h.store.call_answered(sent[0].0.expect("a ticket"), Ok(CallAnswer::Done));
    assert_eq!(h.store.composer("editing").text, "");
}

// ----- HomeEditor.test.tsx -----

fn level(id: &str, category: &str, values: &[&str], current: &str) -> AgentConfigOption {
    let mut o = option(category, values, Some(current));
    o.id = id.into();
    o
}

fn fake_provider() -> AgentProvider {
    AgentProvider {
        id: "fake".into(),
        name: "Fake".into(),
        command: "fake".into(),
        program: "fake".into(),
        args: Vec::new(),
        default_program: None,
        default_args: Vec::new(),
        login_hint: None,
        command_overridden: false,
        command_editable: true,
        command_read_only_reason: None::<AgentCommandReadOnlyReason>,
        installed: true,
        env: BTreeMap::new(),
        config: vec![level("model", "model", &["big", "small", "plain", "unknown"], "big")],
        thought_levels: BTreeMap::from([
            ("big".to_owned(), vec![level("effort", "thought_level", &["low", "high"], "high")]),
            ("small".to_owned(), vec![level("effort", "thought_level", &["minimal", "medium", "max"], "minimal")]),
            ("plain".to_owned(), Vec::new()),
        ]),
        commands: Vec::new(),
        source: AgentSource::Custom,
    }
}

fn home_store(draft: &str) -> AgentsStore {
    let mut store = AgentsStore::new(Saved { draft: Some(draft), ..Saved::default() });
    store.providers = vec![fake_provider()];
    store.update_composer(HOME, |c| c.text = "Check the project".into());
    store
}

fn sent_config(store: &mut AgentsStore) -> Vec<(String, String)> {
    let Some(AgentEffect::Create { request, .. }) = store.start_task(None, None) else { panic!("a task to create") };
    request.config.into_iter().map(|c| (c.id, c.value)).collect()
}

fn pair(id: &str, value: &str) -> (String, String) {
    (id.to_owned(), value.to_owned())
}

/// The model button's text: the model and, when it has one, its effort.
fn model_button(store: &AgentsStore) -> String {
    store.draft_config().iter().filter_map(|o| o.current.clone()).collect()
}

#[test]
fn switches_model_specific_levels_and_restores_each_draft_effort_before_starting() {
    let mut store = home_store(r#"{"provider":"fake","config":{}}"#);
    assert_eq!(model_button(&store), "bighigh");
    store.set_draft_option("model", "small");
    let effort = store.draft_config().into_iter().find(|o| o.id == "effort").expect("an effort setting");
    assert_eq!((effort.values.len(), effort.current.as_deref()), (3, Some("minimal")));
    store.set_draft_option("effort", "medium");
    store.set_draft_option("model", "big");
    assert_eq!(model_button(&store), "bighigh");
    store.set_draft_option("model", "small");
    assert_eq!(model_button(&store), "smallmedium");
    assert_eq!(sent_config(&mut store), vec![pair("model", "small"), pair("effort", "medium")]);
}

#[test]
fn hides_effort_for_models_and_sends_no_stale_effort() {
    for model in ["plain", "unknown"] {
        let mut store = home_store(r#"{"provider":"fake","config":{}}"#);
        store.set_draft_option("model", model);
        assert!(store.draft_config().iter().all(|o| o.category.as_deref() != Some("thought_level")));
        assert_eq!(sent_config(&mut store), vec![pair("model", model)]);
    }
}

#[test]
fn sends_the_displayed_remembered_model_and_effort_even_when_neither_is_changed() {
    let mut store = home_store(r#"{"provider":"fake","config":{}}"#);
    assert_eq!(sent_config(&mut store), vec![pair("model", "big"), pair("effort", "high")]);
}

#[test]
fn ignores_old_provider_wide_effort_choices_and_refreshes_unsupported_per_model_choices() {
    let mut store =
        home_store(r#"{"provider":"fake","config":{"effort":"low"},"thoughtLevels":{"big":{"effort":"removed"}}}"#);
    assert_eq!(model_button(&store), "bighigh");
    assert!(sent_config(&mut store).contains(&pair("effort", "high")));
}

// ----- composerCommands.test.tsx (workspace task project) -----

#[test]
fn defaults_to_the_active_member_last_used_member_then_first_member() {
    let members = vec!["/w/a".to_owned(), "/w/b".to_owned()];
    let projects = |active_path| TaskProjects { root: Some("/w"), members: &members, active_path };
    assert_eq!(default_task_project(projects(Some("/w/b/src/x.rs")), Some("/w/a")).as_deref(), Some("/w/b"));
    assert_eq!(default_task_project(projects(None), Some("/w/b")).as_deref(), Some("/w/b"));
    assert_eq!(default_task_project(projects(Some("/elsewhere")), Some("/gone")).as_deref(), Some("/w/a"));
}

#[test]
fn sends_the_selected_project_with_isolation() {
    for isolation in [Isolation::Local, Isolation::Worktree] {
        let mut store = home_store(r#"{"provider":"fake","base":"main"}"#);
        let Some(AgentEffect::Create { ticket, request }) = store.start_task(Some(isolation), Some("/w/b")) else {
            panic!("a task to create")
        };
        assert_eq!(request.project_root.as_deref(), Some(std::path::Path::new("/w/b")));
        assert_eq!(request.isolation, isolation);
        let base = if isolation == Isolation::Worktree { Some("main") } else { None };
        assert_eq!(request.base.as_deref(), base);
        let mut created = local_task("new");
        created.isolation = isolation;
        let effects = store.task_created(ticket, Ok(created));
        assert_eq!(store.draft.project_root.as_deref(), Some("/w/b"));
        assert_eq!(store.history.first().map(String::as_str), Some("Check the project"));
        assert_eq!(store.composer(HOME), ComposerDraft::default());
        assert!(closed(&effects).contains(&HOME_KEY));
        assert_eq!(opened(&effects), vec!["new"]);
    }
}

#[test]
fn debug_output_names_an_agents_environment_but_never_its_values() {
    let mut store = AgentsStore::new(Saved::default());
    let mut provider = fake_provider();
    provider.env.insert("ANTHROPIC_API_KEY".to_owned(), "sk-ant-secret-value".to_owned());
    store.providers = vec![provider];
    let printed = format!("{store:?}");
    assert!(!printed.contains("sk-ant-secret-value"), "{printed}");
    assert!(printed.contains("ANTHROPIC_API_KEY"), "{printed}");
    assert!(printed.contains("\"fake\""), "{printed}");
}
