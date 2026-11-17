use super::*;
use ide_agent::{AgentCost, AgentEvent, AgentLimit, AgentPlanItem, AgentUsage};

/// The conversation of the first of `tasks` in a window of its own, without its entries loaded, and the store's
/// subscription generation for its events.
fn open_unloaded(
    cx: &mut TestAppContext,
    tasks: Vec<AgentTask>,
) -> (Entity<SessionView>, Entity<AgentsStore>, Events, u64, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let id = tasks[0].id.clone();
    let (store, generation) = connected_store(tasks);
    let store = cx.new(|_| store);
    let shared = store.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| SessionView::new(shared, &id, false, cx).with_clock(clock));
    let events = subscribe(&view, cx);
    (view, store, events, generation, cx)
}

fn subscribe(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> Events {
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|_, cx| {
        cx.subscribe(view, move |_, event: &SessionEvent, _| seen.borrow_mut().push(event.clone())).detach();
    });
    cx.run_until_parked();
    events
}

fn loads(events: &Events) -> usize {
    events
        .borrow()
        .iter()
        .map(|event| match event {
            SessionEvent::Effects(effects) => {
                effects.iter().filter(|effect| matches!(effect, AgentEffect::LoadEntries { .. })).count()
            }
            _ => 0,
        })
        .sum()
}

fn limits(provider: &str, five_hour: f64, seven_day: f64) -> AgentEvent {
    let limit = |window: &str, used_percent| AgentLimit { window: window.to_owned(), used_percent, resets_ms: None };
    AgentEvent::Limits {
        provider: provider.to_owned(),
        limits: vec![limit("five_hour", five_hour), limit("seven_day", seven_day)],
    }
}

fn emit(store: &Entity<AgentsStore>, generation: u64, event: AgentEvent, cx: &mut VisualTestContext) {
    store.update(cx, |store, cx| {
        store.on_event(generation, event, &I18n::default());
        cx.notify();
    });
    cx.run_until_parked();
}

/// A reply's work (tool calls, a running tool, a permission request, a plan and reasoning) stays under the one
/// author that started it, whether the request is open or answered.
#[gpui_kit::test]
async fn keeps_tool_reasoning_plan_and_approval_rows_in_one_author_run(cx: &mut TestAppContext) {
    for chosen in [None, Some("allow")] {
        let request = AgentEntry::Permission {
            id: 1,
            title: "Run python3".to_owned(),
            options: Vec::new(),
            questions: Vec::new(),
            chosen: chosen.map(str::to_owned),
            command: None,
            form: None,
        };
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
        let (view, _, _, cx) = open(cx, vec![local_task("A")], entries);
        let shown = lines(&view, cx);
        assert_eq!(shown.iter().filter(|line| line.starts_with("author: ")).count(), 1, "{chosen:?}: {shown:?}");
        assert_eq!(shown.first().map(String::as_str), Some("author: codex"), "{chosen:?}: {shown:?}");
        assert!(shown.iter().any(|line| line.starts_with("plan: ")), "{shown:?}");
        assert!(shown.iter().any(|line| line.starts_with("permission: ")), "{shown:?}");
    }
}

/// Each answer of a pending request sends the option id the agent gave it; once the agent reports the choice,
/// the request keeps a short record without its answers or its whole command.
#[gpui_kit::test]
async fn sends_the_original_option_id_then_collapses_the_resolved_card(cx: &mut TestAppContext) {
    const SELECTORS: [&str; 4] =
        ["session-permission-12-0", "session-permission-12-1", "session-permission-12-2", "session-permission-12-3"];
    let command = "python3 -c 'from pathlib import Path\np = Path(\"note.txt\")\np.write_text(\"one\\ntwo\")'";
    let options = vec![
        permission_option("opaque-once", "Allow Once", "allow_once"),
        permission_option("opaque-policy", &format!("Always Allow `{command}`"), "allow_always"),
        permission_option("opaque-no", "Decline", "reject_once"),
        permission_option("opaque-stop", "Decline and Stop", "reject_always"),
    ];
    for (index, option) in options.iter().enumerate() {
        let pending = |chosen: Option<String>| AgentEntry::Permission {
            id: 12,
            title: format!("Run {command}"),
            options: options.clone(),
            questions: Vec::new(),
            chosen,
            command: None,
            form: None,
        };
        let (view, store, events, cx) = open(cx, vec![local_task("A")], vec![pending(None)]);
        let shown = lines(&view, cx);
        let request = shown.iter().find(|line| line.starts_with("permission: ")).expect("the request is listed");
        assert!(request.contains("write_text") && request.contains('['), "{request}");
        click(cx, SELECTORS[index]);
        let respond = AgentCall::Respond { task: "A".to_owned(), permission: 12, option: Some(option.id.clone()) };
        assert_eq!(effect_calls(&events), [respond], "{}", option.kind);
        store.update(cx, |store, cx| {
            store.entries_loaded("A", None, vec![pending(Some(option.name.clone()))]);
            cx.notify();
        });
        cx.run_until_parked();
        let shown = lines(&view, cx);
        let record = shown.iter().find(|line| line.starts_with("permission: ")).expect("the record is listed");
        assert!(!record.contains('[') && !record.contains("write_text"), "{}: {record}", option.kind);
        assert!(record.contains("python3"), "{record}");
        assert!(cx.debug_bounds(SELECTORS[index]).is_none(), "the answers are gone");
    }
}

/// The header shows the context use and the provider's usage limits by default; with Settings › Tools › Agents ›
/// Chat showing only the cost, it shows the cost alone.
#[gpui_kit::test]
async fn shows_the_providers_usage_limits_by_default_and_each_meter_as_settings_tools_agents_chat_allows(
    cx: &mut TestAppContext,
) {
    let mut task = local_task("A");
    let cost = AgentCost { amount: 0.42, currency: "USD".to_owned() };
    task.usage = Some(AgentUsage { used: 24_000, size: 200_000, cost: Some(Box::new(cost)) });
    let (view, store, _, generation, cx) = open_unloaded(cx, vec![task]);
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, vec![agent("Answer A")]);
        cx.notify();
    });
    emit(&store, generation, limits("codex", 19.0, 9.4), cx);
    assert_eq!(view.update(cx, |view, cx| view.meters(cx)), ["12%", "5h 19%", "7d 9%"]);
    let defaults = AgentSettings::default();
    let settings = AgentSettings { show_context_usage: false, show_usage_limits: false, show_cost: true, ..defaults };
    view.update(cx, |view, cx| view.set_agent_settings(settings, cx));
    assert_eq!(view.update(cx, |view, cx| view.meters(cx)), ["$0.42"]);
}

/// The header follows the latest limits of the conversation's own provider, not another provider's, and limits
/// leave the task read.
#[gpui_kit::test]
async fn keeps_the_latest_usage_limits_per_provider_without_marking_anything_unread(cx: &mut TestAppContext) {
    let (view, store, _, generation, cx) = open_unloaded(cx, vec![local_task("A")]);
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, vec![agent("Answer A")]);
        cx.notify();
    });
    emit(&store, generation, limits("codex", 10.0, 1.0), cx);
    emit(&store, generation, limits("claude", 50.0, 5.0), cx);
    emit(&store, generation, limits("codex", 12.0, 2.0), cx);
    let meters = view.update(cx, |view, cx| view.meters(cx));
    assert!(meters.contains(&"5h 12%".to_owned()) && meters.contains(&"7d 2%".to_owned()), "{meters:?}");
    assert!(!meters.iter().any(|meter| meter.contains("50%") || meter.contains("10%")), "{meters:?}");
    assert!(!store.read_with(cx, |store, _| store.is_unread("A")));
}

/// A conversation is read from the backend once; another view of it opened later shows it from the store
/// without asking again.
#[gpui_kit::test]
async fn fetches_a_conversation_once_and_shows_it_again_from_the_store_when_its_tab_reopens(cx: &mut TestAppContext) {
    let (view, store, events, _, cx) = open_unloaded(cx, vec![local_task("A")]);
    view.update(cx, |view, cx| view.load(cx));
    assert_eq!(loads(&events), 1);
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, vec![agent("Answer A")]);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(lines(&view, cx), ["author: codex", "agent: Answer A"]);

    let shared = store.clone();
    let again = cx.update(|_, cx| cx.new(|cx| SessionView::new(shared, "A", false, cx).with_clock(clock)));
    let reopened = subscribe(&again, cx);
    again.update(cx, |view, cx| view.load(cx));
    cx.run_until_parked();
    assert_eq!(loads(&reopened), 0, "the store has the conversation");
    assert_eq!(lines(&again, cx), ["author: codex", "agent: Answer A"]);
}
