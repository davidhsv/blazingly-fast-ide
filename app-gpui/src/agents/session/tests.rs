use super::*;
use crate::agents::composer::{TokenKind, message_tokens};
use crate::agents::conversation::find_in_runs;
use crate::agents::fixtures::{
    agent, connected_store, local_task, path, permission, subagent, thought, tool, tool_with,
};
use crate::agents::model::{reply_times, reply_turns, turn_numbers};
use ide_agent::{AgentCost, AgentEvent, AgentPlanItem, AgentToolDiff, AgentUsage};

fn user(text: &str, sent_ms: Option<f64>, done_ms: Option<f64>) -> AgentEntry {
    let text = text.to_owned();
    AgentEntry::User { text, attachments: Vec::new(), images: Vec::new(), sent_ms, done_ms, steered: false }
}

fn edit(id: &str) -> AgentEntry {
    let AgentEntry::Tool { id, title, status, locations, output, .. } = tool(id) else { unreachable!() };
    let diffs = vec![AgentToolDiff { path: path("/project/a.ts"), added: 2, removed: 1 }];
    AgentEntry::Tool { id, title, kind: "edit".to_owned(), status, locations, output, diffs }
}

fn process_rows(rows: &[ConversationRow]) -> Vec<(usize, Vec<usize>)> {
    rows.iter()
        .filter_map(|row| match row {
            ConversationRow::Process { index, entries } => Some((*index, entries.clone())),
            ConversationRow::Entry { .. } => None,
        })
        .collect()
}

fn row_after(rows: &[ConversationRow], index: usize) -> Option<&ConversationRow> {
    let at = rows.iter().position(|row| row.index() == index)?;
    rows.get(at + 1)
}

#[test]
fn keeps_authors_above_messages_and_their_folded_work_with_matching_icon_only_actions() {
    let entries = vec![
        user("First question", None, None),
        agent("First reply"),
        tool("read"),
        agent("More detail"),
        user("Next question", None, None),
        thought("Considering the next question"),
        tool("next"),
        agent("Next reply"),
    ];
    let rows = conversation_rows(&entries, 0);
    let headers = author_headers(&entries, &rows);
    assert_eq!(headers, BTreeSet::from([1, 5]));
    // The second author heads the folded work, and the reply follows it.
    assert_eq!(process_rows(&rows), vec![(5, vec![5, 6])]);
    assert_eq!(row_after(&rows, 5), Some(&ConversationRow::Entry { index: 7 }));
    let task = local_task("A");
    let turns = turn_numbers(&entries);
    let replies = reply_turns(&entries);
    let mut quotes = 0;
    for (i, entry) in entries.iter().enumerate() {
        match entry {
            AgentEntry::Agent { .. } => {
                let buttons = reply_buttons(&task, replies.get(&i), turns.len());
                assert_eq!(buttons, vec![ReplyButton::Copy, ReplyButton::QuoteReply]);
                quotes += 1;
            }
            AgentEntry::User { text, attachments, .. } => {
                let buttons = user_buttons(&task, text, attachments.len(), turns.get(&i).copied(), turns.len());
                assert_eq!(buttons.first(), Some(&UserButton::Copy));
            }
            _ => {}
        }
    }
    assert_eq!(quotes, 3);
}

#[test]
fn keeps_tool_reasoning_plan_and_approval_rows_in_one_author_run() {
    for chosen in [None, Some("allow")] {
        let AgentEntry::Permission { id, title, options, questions, .. } = permission(1, "Run python3", Vec::new())
        else {
            unreachable!()
        };
        let request = AgentEntry::Permission { id, title, options, questions, chosen: chosen.map(str::to_owned) };
        let plan = AgentEntry::Plan {
            items: vec![AgentPlanItem { content: "Check files".to_owned(), status: "in_progress".to_owned() }],
        };
        let entries = vec![
            agent("I’ll read package.json…"),
            tool("one"),
            tool("two"),
            agent("More detail"),
            tool_with("List .", AgentToolStatus::Running, None),
            request,
            plan,
            thought("Consider the result"),
            agent("Done"),
        ];
        let rows = conversation_rows(&entries, 0);
        assert_eq!(author_headers(&entries, &rows), BTreeSet::from([0]), "{chosen:?}");
        assert_eq!(rows[0].index(), 0);
    }
}

#[test]
fn starts_another_author_run_after_type_entries() {
    let request = AgentEntry::Message { incoming: true, text: "Parent request".to_owned() };
    let others = [subagent("child", "Explore task A"), request];
    for other in others {
        let entries =
            vec![agent("Before the switch"), other, thought("After the switch"), tool("read"), agent("After the work")];
        let rows = conversation_rows(&entries, 0);
        let headers = author_headers(&entries, &rows);
        assert_eq!(headers, BTreeSet::from([0, 2]));
        assert!(!headers.contains(&rows[1].index()));
        assert_eq!(process_rows(&rows), vec![(2, vec![2, 3])]);
        assert_eq!(row_after(&rows, 2), Some(&ConversationRow::Entry { index: 4 }));
    }
}

#[test]
fn anchors_every_agent_footer_with_its_time_before_the_hover_actions() {
    let entries =
        vec![user("Question", Some(1000.0), Some(13_000.0)), agent("First reply"), tool("read"), agent("Final reply")];
    let times = reply_times(&entries);
    let replies: Vec<usize> =
        entries.iter().enumerate().filter(|(_, e)| matches!(e, AgentEntry::Agent { .. })).map(|(i, _)| i).collect();
    assert_eq!(replies, vec![1, 3]);
    for i in replies {
        let time = times.get(&i).expect("the turn's times");
        let footer = message_time(time.sent_ms, time.done_ms, true).expect("a footer time");
        assert_eq!(footer, MessageTime { at_ms: 13_000.0, took: Some("12s".to_owned()) });
    }
    let sent = message_time(Some(1000.0), Some(13_000.0), false).expect("the user message's time");
    assert_eq!(sent.at_ms, 1000.0);
}

#[test]
fn builds_only_the_requested_tail_with_absolute_indexes() {
    let entries: Vec<AgentEntry> = (0..45).map(|i| agent(&format!("reply {i}"))).collect();
    let rows = conversation_rows(&entries, 40);
    assert_eq!(rows.iter().map(ConversationRow::index).collect::<Vec<_>>(), vec![40, 41, 42, 43, 44]);
}

#[test]
fn folds_reasoning_and_tool_calls_into_one_block() {
    let i18n = I18n::default();
    let entries = vec![thought("think one"), tool("one"), thought("think two"), tool("two"), agent("Answer A")];
    let rows = conversation_rows(&entries, 0);
    assert_eq!(process_rows(&rows), vec![(0, vec![0, 1, 2, 3])]);
    assert_eq!(process_summary(&entries, &[0, 1, 2, 3], None, &i18n).label, "Reasoning · 2 tool calls");
    let mut folds = Folds::default();
    assert!(!folds.is_open(Fold::Process(0), &entries));
    let texts = |folds: &Folds| rendered_texts(&entries, &rows, folds, false, None, &i18n);
    // Folded content is rendered once opened, a level at a time.
    assert!(!texts(&folds).iter().any(|t| t == "Reasoning"));
    folds.toggled(Fold::Process(0), true);
    assert!(texts(&folds).iter().any(|t| t == "Reasoning"));
    assert!(!texts(&folds).iter().any(|t| t == "think one"));
    folds.toggled(Fold::Thought(0), true);
    assert!(texts(&folds).iter().any(|t| t == "think one"));
}

#[test]
fn is_rendered_for_find_in_conversation_so_its_text_is_found() {
    let i18n = I18n::default();
    let entries = vec![
        tool_with("one", AgentToolStatus::Completed, Some("needle in a tool output")),
        tool("two"),
        agent("Answer A"),
    ];
    let rows = conversation_rows(&entries, 0);
    assert_eq!(process_summary(&entries, &[0, 1], None, &i18n).label, "2 tool calls");
    let folds = Folds::default();
    let closed = rendered_texts(&entries, &rows, &folds, false, None, &i18n);
    assert!(find_in_runs(&closed, "needle").is_empty());
    let finding = rendered_texts(&entries, &rows, &folds, true, None, &i18n);
    assert_eq!(find_in_runs(&finding, "needle").len(), 1);
    assert!(!folds.is_open(Fold::Process(0), &entries));
}

#[test]
fn shows_file_references_and_skills_as_distinct_tags() {
    let tokens = message_tokens("fix @src/a.ts and run /triage");
    let tags: Vec<(TokenKind, &str)> =
        tokens.iter().filter(|t| t.kind != TokenKind::Text).map(|t| (t.kind, t.value.as_str())).collect();
    assert_eq!(tags, vec![(TokenKind::File, "@src/a.ts"), (TokenKind::Command, "/triage")]);
    assert_eq!(user_files(&["a.ts".to_owned()], 0), ["a.ts".to_owned()]);
}

#[test]
fn keeps_status_tools_outside_folds_with_their_output_expanded() {
    let i18n = I18n::default();
    for status in [AgentToolStatus::Pending, AgentToolStatus::Running] {
        let entries = vec![tool("done one"), tool("done two"), tool_with("live", status, Some("Reading files…"))];
        let rows = conversation_rows(&entries, 0);
        let live = ConversationRow::Entry { index: 2 };
        assert_eq!(rows, vec![ConversationRow::Process { index: 0, entries: vec![0, 1] }, live]);
        let folds = Folds::default();
        assert!(folds.is_open(Fold::Tool(2), &entries));
        assert!(!folds.is_open(Fold::Process(0), &entries));
        assert_eq!(process_summary(&entries, &[0, 1], None, &i18n).label, "2 tool calls");
    }
}

#[test]
fn folds_consecutive_finished_tool_calls_into_one_collapsed_row() {
    let i18n = I18n::default();
    let entries = vec![edit("one"), edit("two"), agent("Answer A")];
    let rows = conversation_rows(&entries, 0);
    assert_eq!(process_rows(&rows), vec![(0, vec![0, 1])]);
    assert!(!Folds::default().is_open(Fold::Process(0), &entries));
    let summary = process_summary(&entries, &[0, 1], None, &i18n);
    assert_eq!(summary, ProcessSummary { label: "2 tool calls".to_owned(), added: 4, removed: 2, working: false });
}

fn reply(store: &mut AgentsStore, generation: u64, task: &str) {
    let i18n = I18n::default();
    let event = AgentEvent::Entry { task: task.to_owned(), subagent: None, index: 0, entry: agent("Done") };
    store.on_event(generation, event, &i18n);
}

#[test]
fn leaves_a_task_unread_while_its_kept_alive_tab_is_hidden_and_reads_it_once_shown() {
    let (mut store, generation) = connected_store(vec![local_task("A")]);
    reply(&mut store, generation, "A");
    assert!(store.is_unread("A"));
    // The kept-alive tab is mounted behind the shown one: it isn't read there.
    if reads_conversation(false, false, false) {
        store.show_session("A");
    }
    assert!(store.is_unread("A"));
    if reads_conversation(true, false, false) {
        store.show_session("A");
    }
    assert!(!store.is_unread("A"));
}

#[test]
fn returns_to_the_task_conversation_when_the_docked_task_changes_after_opening_a_subagent() {
    let (mut store, _) = connected_store(vec![local_task("A"), local_task("B")]);
    store.entries_loaded("A", None, vec![agent("Answer A"), subagent("child", "Explore task A")]);
    store.entries_loaded("B", None, vec![agent("Answer B")]);
    store.dock_task("A", &Default::default());
    let mut nav = SessionNav::new(store.docked.as_deref().expect("a docked task"));
    let load = nav.open_subagent("child", &store);
    assert!(matches!(load, Some(AgentEffect::LoadEntries { subagent: Some(ref s), .. }) if s == "child"));
    store.entries_loaded("A", Some("child"), vec![agent("Child answer")]);
    assert_eq!(nav.entries_key(), entries_key("A", Some("child")));
    assert!(!nav.shows_composer());
    store.open_task("B");
    nav.show_task(store.docked.as_deref().expect("a docked task"));
    assert_eq!(nav.subagent(), None);
    assert_eq!(nav.entries_key(), "B");
    assert!(nav.shows_composer());
}

#[test]
fn keeps_docked_changes_opt_in_preserves_the_draft_and_returns_to_the_same_conversation() {
    let i18n = I18n::default();
    let saved = r#"{"A":true}"#;
    let changes = [AgentChange {
        path: path("/project/very-long-file-name.ts"),
        rel_path: "very-long-file-name.ts".to_owned(),
        kind: ide_vcs::ChangeKind::Modified,
        added: 8,
        removed: 2,
    }];
    let (mut store, generation) = connected_store(vec![local_task("A")]);
    let mut open = ChangesOpen::new("A", true, Some(saved));
    assert!(!open.is_open(Some(&changes)));
    let toggle = changes_toggle(Some(&changes), false, &i18n);
    assert!(toggle.enabled);
    assert_eq!(toggle.label, "Show changes");
    store.show_session("A");
    store.update_composer("A", |c| c.text = "Draft to keep".to_owned());
    // Showing the changes covers the conversation: it isn't read meanwhile.
    assert!(open.set("A", true, Some(saved)).is_none());
    assert!(open.is_open(Some(&changes)));
    assert!(!reads_conversation(true, true, open.is_open(Some(&changes))));
    store.hide_session("A");
    reply(&mut store, generation, "A");
    store.set_window_active(true);
    assert!(store.is_unread("A"));
    // Back to the conversation.
    assert!(open.set("A", false, Some(saved)).is_none());
    assert!(reads_conversation(true, true, open.is_open(Some(&changes))));
    store.show_session("A");
    assert!(!store.is_unread("A"));
    assert_eq!(store.composer("A").text, "Draft to keep");
    // The editor's saved choice is untouched.
    assert!(ChangesOpen::new("A", false, Some(saved)).is_open(Some(&[])));
}

#[test]
fn shows_the_providers_usage_limits_by_default_and_each_meter_as_settings_tools_agents_chat_allows() {
    let mut i18n = I18n::default();
    let mut task = local_task("A");
    task.usage = Some(AgentUsage {
        used: 24_000,
        size: 200_000,
        cost: Some(Box::new(AgentCost { amount: 0.42, currency: "USD".to_owned() })),
    });
    let limits = [
        AgentLimit { window: "five_hour".to_owned(), used_percent: 19.0, resets_ms: None },
        AgentLimit { window: "seven_day".to_owned(), used_percent: 9.4, resets_ms: None },
    ];
    let entries = [agent("Answer A")];
    let shown = |meters: Vec<Meter>| -> Vec<String> {
        meters
            .into_iter()
            .map(|m| match m {
                Meter::Context(l) => l.short,
                Meter::Limit(l) => l.short,
                Meter::Cost(c) | Meter::WorkTime(c) => c,
            })
            .collect()
    };
    let defaults = AgentSettings::default();
    let all = meters(&task, Some(&entries), Some(&limits), &defaults, 0.0, &mut i18n);
    assert_eq!(shown(all), vec!["12%", "5h 19%", "7d 9%"]);
    let settings = AgentSettings { show_context_usage: false, show_usage_limits: false, show_cost: true, ..defaults };
    let cost = meters(&task, Some(&entries), Some(&limits), &settings, 0.0, &mut i18n);
    assert_eq!(shown(cost), vec!["$0.42"]);
}
