use super::*;
use crate::agents::store::ScrollPosition;

/// The rows the conversation draws.
fn rows(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> Vec<RowModel> {
    view.update(cx, |view, cx| view.snapshot(cx).rows)
}

/// The permission requests the conversation draws.
fn permission_lines(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> Vec<(u32, PermissionLine)> {
    rows(view, cx)
        .into_iter()
        .filter_map(|row| match row.kind {
            RowKind::Permission { id, line } => Some((id, *line)),
            _ => None,
        })
        .collect()
}

fn geometry(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> Geometry {
    view.read_with(cx, |view, _| view.geometry()).expect("the conversation is laid out")
}

fn at_end(g: Geometry) -> bool {
    g.height - g.top - g.viewport <= 1.0
}

/// Moves the conversation's offset to `top` without user input, as dragging the scrollbar or a layout does.
fn set_top(view: &Entity<SessionView>, top: f64, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.scroll.set_offset(point(px(0.0), px(-(top as f32))));
        cx.notify();
    });
    cx.run_until_parked();
}

fn follows(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.follows())
}

fn jump_shown(view: &Entity<SessionView>, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, cx| view.shows_jump_to_latest(cx))
}

/// The conversation of `task` in `store`, in a window of its own.
fn open_store<'a>(
    cx: &'a mut TestAppContext,
    store: AgentsStore,
    task: &str,
) -> (Entity<SessionView>, Entity<AgentsStore>, Events, &'a mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let store = cx.new(|_| store);
    let (shared, task) = (store.clone(), task.to_owned());
    let (view, cx) = cx.add_window_view(move |_, cx| SessionView::new(shared, &task, false, cx).with_clock(clock));
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &SessionEvent, _| seen.borrow_mut().push(event.clone())).detach();
    });
    cx.run_until_parked();
    (view, store, events, cx)
}

// ----- message tags -----

/// A sent message shows its `@file` reference and its `/skill` as tags of two kinds.
#[gpui_kit::test]
async fn shows_file_references_and_skills_as_distinct_tags(cx: &mut TestAppContext) {
    let message = AgentEntry::User {
        text: "fix @src/a.ts and run /triage".to_owned(),
        attachments: vec!["a.ts".to_owned()],
        images: Vec::new(),
        sent_ms: Some(1.0),
        done_ms: Some(2.0),
        steered: false,
    };
    let (view, _, _, cx) = open(cx, vec![local_task("A")], vec![message]);
    let drawn = rows(&view, cx);
    let Some(RowKind::User(user)) = drawn.first().map(|row| &row.kind) else {
        panic!("the message is drawn: {drawn:?}")
    };
    let tags: Vec<(&str, TokenKind)> = user
        .tokens
        .iter()
        .filter(|token| token.kind != TokenKind::Text)
        .map(|token| (token.value.as_str(), token.kind))
        .collect();
    assert_eq!(tags, [("@src/a.ts", TokenKind::File), ("/triage", TokenKind::Command)]);
    assert!(lines(&view, cx)[0].starts_with("you: fix @src/a.ts and run /triage [files: a.ts]"));
}

// ----- questions and permission requests -----

/// A question asked as a permission request shows its whole text once, under the "Allow tool call?" heading, with
/// a button per answer named as the agent named it; a click sends that answer's id.
#[gpui_kit::test]
async fn shows_the_whole_question_once_inline_with_every_answer_and_sends_the_chosen_one(cx: &mut TestAppContext) {
    let question = "Scenario 4 (\"follows the TR\") still leaves the \"About your request\" section empty after two \
                    rewrites.\nHow should we continue?";
    let options = vec![
        permission_option("a", "Ship it as is and record it (Recommended)", "allow_once"),
        permission_option("b", "Detect the request in code", "allow_once"),
        permission_option("c", "Skip", "reject_once"),
    ];
    let (view, _, events, cx) = open(cx, vec![local_task("A")], vec![permission(7, question, options)]);
    let requests = permission_lines(&view, cx);
    assert_eq!(requests.len(), 1, "one card: {requests:?}");
    let (_, line) = &requests[0];
    assert_eq!(line.title, "Allow tool call?");
    assert_eq!(line.command, None);
    assert_eq!(line.detail.replace('\n', " "), question.replace('\n', " "), "the whole question");
    let labels: Vec<&str> = line.answers.iter().map(|answer| answer.label.as_str()).collect();
    assert_eq!(labels, ["Ship it as is and record it (Recommended)", "Detect the request in code", "Skip"]);
    assert!(!lines(&view, cx).iter().any(|shown| shown.contains("How should we continue?")), "not repeated elsewhere");
    click(cx, "session-permission-7-1");
    assert_eq!(
        effect_calls(&events),
        [AgentCall::Respond { task: "A".to_owned(), permission: 7, option: Some("b".to_owned()) }]
    );
}

/// Two questions worded alike but with their own answer keys (Codex) keep their own answers, sent under those keys.
#[gpui_kit::test]
async fn keeps_separate_answers_for_codex_question_ids_with_identical_wording(cx: &mut TestAppContext) {
    let questions = [("first", "One"), ("second", "Two")]
        .into_iter()
        .map(|(key, label)| {
            let mut asked = question(key, "Which?", false, &[(label, "", None)]);
            asked.answer_key = Some(key.to_owned());
            asked
        })
        .collect();
    let entry = AgentEntry::Permission {
        id: 11,
        title: "Which?".to_owned(),
        options: Vec::new(),
        questions,
        chosen: None,
        command: None,
        form: None,
    };
    let (view, _, events, cx) = open(cx, vec![local_task("A")], vec![entry]);
    let card = view.read_with(cx, |view, cx| view.card(cx)).expect("the question card shows");
    assert!(card.contains(&"first: Which?".to_owned()) && card.contains(&"second: Which?".to_owned()), "{card:?}");
    click(cx, "session-question-0-1");
    click(cx, "session-question-1-1");
    click(cx, "session-question-submit");
    let calls = effect_calls(&events);
    let [AgentCall::Respond { task, permission: 11, option: Some(answer) }] = calls.as_slice() else {
        panic!("one answer: {calls:?}")
    };
    assert_eq!(task, "A");
    let answer: serde_json::Value = serde_json::from_str(answer).expect("the answer is JSON");
    assert_eq!(answer, serde_json::json!({ "first": ["One"], "second": ["Two"] }));
}

/// A command the agent asks to run while one of its read tools is still at work shows in one card with the whole
/// command, short answer buttons (each option's own name as its tooltip) and no error, beside the running tool.
#[gpui_kit::test]
async fn shows_the_requested_command_while_another_read_tool_is_pending_with_one_card_and_short_buttons(
    cx: &mut TestAppContext,
) {
    let command = "python3 -c 'from pathlib import Path\np = Path(\"note.txt\")\np.write_text(\"one\\ntwo\")'";
    let always = format!("Always Allow `{command}`");
    let options = vec![
        permission_option("opaque-once", "Allow Once", "allow_once"),
        permission_option("opaque-policy", &always, "allow_always"),
        permission_option("opaque-no", "Decline", "reject_once"),
        permission_option("opaque-stop", "Decline and Stop", "reject_always"),
    ];
    let entries =
        vec![tool_with("List .", AgentToolStatus::Running, None), permission(12, &format!("Run {command}"), options)];
    let (view, _, _, cx) = open(cx, vec![local_task("A")], entries);
    let requests = permission_lines(&view, cx);
    assert_eq!(requests.len(), 1, "one card: {requests:?}");
    let (id, line) = &requests[0];
    assert_eq!(*id, 12);
    assert_eq!(line.title, "codex wants to run a command");
    assert_eq!(line.command.as_deref(), Some(command));
    let labels: Vec<&str> = line.answers.iter().map(|answer| answer.label.as_str()).collect();
    assert_eq!(labels, ["Allow Once", "Always Allow", "Decline", "Decline and Stop"]);
    assert_eq!(line.answers[1].title, always);
    for n in 0..4 {
        let selector: &'static str = Box::leak(format!("session-permission-12-{n}").into_boxed_str());
        assert!(cx.debug_bounds(selector).is_some(), "{selector} is drawn");
    }
    let shown = lines(&view, cx);
    assert!(shown.iter().any(|row| row == "tool: List ."), "the running tool keeps its row: {shown:?}");
    assert!(!shown.iter().any(|row| row.starts_with("error: ")), "{shown:?}");
}

// ----- scrolling -----

/// A scroll that no layout caused (dragging the scrollbar thumb up) leaves the end: the offset stays where it was
/// moved and Jump to Latest shows.
#[gpui_kit::test]
async fn leaves_the_end_for_a_scroll_that_no_layout_caused_as_dragging_the_scrollbar_thumb(cx: &mut TestAppContext) {
    let (view, _, _, cx) = open(cx, vec![local_task("A")], long_conversation(60));
    let g = geometry(&view, cx);
    assert!(at_end(g) && g.height > 2.0 * g.viewport, "{g:?}");
    // The thumb is taken a little above the end first: still the end.
    set_top(&view, g.height - g.viewport - 10.0, cx);
    assert!(follows(&view, cx));
    set_top(&view, 0.0, cx);
    assert!(geometry(&view, cx).top.abs() <= 1.0, "the offset stays: {:?}", geometry(&view, cx));
    assert!(!follows(&view, cx));
    assert!(jump_shown(&view, cx));
    assert!(cx.debug_bounds("session-jump-to-latest").is_some());
}

/// The conversation grows after it was scrolled to its end, and the offset moves back while the rows lay out,
/// without the user scrolling: it keeps following the end. A wheel scroll up then leaves the end.
#[gpui_kit::test]
async fn keeps_following_the_end_when_the_offset_moves_back_without_the_user_scrolling(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, vec![local_task("A")], long_conversation(60));
    let before = geometry(&view, cx);
    let back = before.height - before.viewport - 10.0;
    set_top(&view, back, cx);
    assert!(follows(&view, cx));
    let mut grown = long_conversation(60);
    grown.push(agent("A reply that came later\n\nwith more text\n\nand more"));
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, grown);
        cx.notify();
    });
    cx.run_until_parked();
    let after = geometry(&view, cx);
    assert!(after.height > before.height, "{before:?} {after:?}");
    set_top(&view, back, cx);
    assert!(follows(&view, cx));
    assert!(at_end(geometry(&view, cx)), "back at the end: {:?}", geometry(&view, cx));
    assert!(!jump_shown(&view, cx));
    scroll_up(cx, 400.0);
    assert!(!follows(&view, cx));
    assert!(jump_shown(&view, cx));
}

/// Only the last entries of a long conversation are painted at first, each row keeping the index of its entry.
#[gpui_kit::test]
async fn builds_only_the_requested_tail_with_absolute_indexes(cx: &mut TestAppContext) {
    let (store, _) = connected_store(vec![local_task("A")]);
    let (view, store, _, cx) = open_store(cx, store, "A");
    // Held on the scrollbar, the earlier entries wait.
    let press = bounds_of(cx, "session-scroll").center();
    cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let entries: Vec<AgentEntry> = (0..45).map(|i| agent(&format!("reply {i}"))).collect();
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, entries);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.first_painted()), Some(5));
    assert!(cx.debug_bounds("session-row-4").is_none());
    assert!(cx.debug_bounds("session-row-5").is_some() && cx.debug_bounds("session-row-44").is_some());
    assert_eq!(lines(&view, cx).iter().filter(|line| line.starts_with("agent: ")).count(), 40);
    cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::default());
}

// ----- scroll positions across hosts -----

/// The offset and whether the conversation follows its end are kept when it moves from an editor tab to the
/// tool window (a smaller viewport) and back; new output keeps a conversation that follows at its end and leaves
/// one scrolled away where it was.
#[gpui_kit::test]
async fn retains_scrolling_and_follow_output_when_moving_in_both_directions(cx: &mut TestAppContext) {
    for stick in [false, true] {
        let (view, store, _, editor) = open(cx, vec![local_task("A")], long_conversation(60));
        scroll_up(editor, 400.0);
        if stick {
            // Back down to the end with the wheel.
            scroll_up(editor, -10_000.0);
        }
        let kept = geometry(&view, editor).top;
        let saved = store.read_with(editor, |store, _| store.scroll_position("A")).expect("the position is kept");
        assert_eq!(saved.stick, stick);
        if !stick {
            assert!((saved.top - kept).abs() <= 1.0, "{saved:?} at {kept}");
        }
        drop(view);
        editor.update(|window, _| window.remove_window());
        editor.run_until_parked();

        let shared = store.clone();
        let (dock, docked) =
            editor.add_window_view(move |_, cx| SessionView::new(shared, "A", true, cx).with_clock(clock));
        docked.simulate_resize(gpui_kit::Size { width: px(600.0), height: px(300.0) });
        docked.run_until_parked();
        let check = |g: Geometry, host: &str| {
            if stick {
                assert!(at_end(g), "{host}: at the end: {g:?}");
            } else {
                assert!((g.top - kept).abs() <= 1.0, "{host}: at {kept}: {g:?}");
            }
        };
        check(geometry(&dock, docked), "the tool window");
        assert_eq!(dock.read_with(docked, |view, _| view.follows()), stick);
        let mut more = long_conversation(60);
        more.push(agent("More output"));
        store.update(docked, |store, cx| {
            store.entries_loaded("A", None, more);
            cx.notify();
        });
        docked.run_until_parked();
        check(geometry(&dock, docked), "the tool window with more output");
        drop(dock);
        docked.update(|window, _| window.remove_window());
        docked.run_until_parked();

        let shared = store.clone();
        let (again, editor) =
            docked.add_window_view(move |_, cx| SessionView::new(shared, "A", false, cx).with_clock(clock));
        check(geometry(&again, editor), "the editor tab again");
    }
}

/// Each conversation (a task's own, a subagent's, another task's) opens at its own kept offset.
#[gpui_kit::test]
async fn keeps_task_and_subagent_scroll_positions_separate(cx: &mut TestAppContext) {
    let (mut store, _) = connected_store(vec![local_task("A"), local_task("B")]);
    let mut own = vec![subagent("c1", "Explore")];
    own.extend(long_conversation(60));
    store.entries_loaded("A", None, own);
    store.entries_loaded("A", Some("c1"), long_conversation(60));
    store.entries_loaded("B", None, long_conversation(60));
    for (key, top) in [("A", 120.0), ("A/c1", 70.0), ("B", 340.0)] {
        store.set_scroll_position(key, ScrollPosition { top, stick: false });
    }
    let (view, _, _, cx) = open_store(cx, store, "A");
    let top = |view: &Entity<SessionView>, cx: &mut VisualTestContext| geometry(view, cx).top;
    assert!((top(&view, cx) - 120.0).abs() <= 1.0, "{}", top(&view, cx));
    view.update(cx, |view, cx| view.open_subagent("c1", cx));
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.path().to_vec()), ["c1"]);
    assert!((top(&view, cx) - 70.0).abs() <= 1.0, "{}", top(&view, cx));
    view.update(cx, |view, cx| view.show_task("B", cx));
    cx.run_until_parked();
    assert!((top(&view, cx) - 340.0).abs() <= 1.0, "{}", top(&view, cx));
    view.update(cx, |view, cx| view.show_task("A", cx));
    cx.run_until_parked();
    assert!((top(&view, cx) - 120.0).abs() <= 1.0, "{}", top(&view, cx));
}

/// A kept offset is applied once the conversation's entries have loaded, and the project's disconnection forgets
/// every kept position.
#[gpui_kit::test]
async fn restores_after_entries_load_and_clears_positions_on_project_disconnect(cx: &mut TestAppContext) {
    let (mut store, _) = connected_store(vec![local_task("A")]);
    store.set_scroll_position("A", ScrollPosition { top: 230.0, stick: false });
    let (view, store, _, cx) = open_store(cx, store, "A");
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, long_conversation(60));
        cx.notify();
    });
    cx.run_until_parked();
    assert!((geometry(&view, cx).top - 230.0).abs() <= 1.0, "{:?}", geometry(&view, cx));
    store.update(cx, |store, cx| {
        store.disconnect();
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(store.read_with(cx, |store, _| store.scroll_positions()), 0);
}

/// In a question card, a number key picks that answer and its preview shows (a choice without one says so);
/// hovering another answer previews it instead; picking Other previews nothing; Escape skips the request.
#[gpui_kit::test]
async fn picks_options_with_number_keys_previews_the_picked_or_hovered_one_and_skips_with_escape(
    cx: &mut TestAppContext,
) {
    let ask = AgentEntry::Permission {
        id: 10,
        title: "Which layout?".to_owned(),
        options: vec![permission_option("reject", "Skip", "reject_once")],
        questions: vec![question("", "Which layout?", false, &[("Table", "", Some("| a | b |")), ("List", "", None)])],
        chosen: None,
        command: None,
        form: None,
    };
    let (view, _, events, cx) = open(cx, vec![local_task("A")], vec![ask]);
    let focus = view.read_with(cx, |view, _| view.question.as_ref().map(|q| q.focus.clone())).expect("a card");
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.run_until_parked();
    let preview = |view: &Entity<SessionView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let question = view.question.as_ref()?;
            let preview = question.form.preview(0, &I18n::default())?;
            Some((preview.caption, preview.body))
        })
    };
    cx.simulate_keystrokes("2");
    cx.run_until_parked();
    let card = view.read_with(cx, |view, cx| view.card(cx)).expect("a card");
    assert!(card.contains(&"(x) List".to_owned()), "{card:?}");
    assert!(card.contains(&"preview: List".to_owned()), "{card:?}");
    assert_eq!(preview(&view, cx), Some(("List".to_owned(), None)), "drawn as No preview");

    let table = bounds_of(cx, "session-question-0-1").center();
    cx.simulate_mouse_move(table, None, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(preview(&view, cx), Some(("Table".to_owned(), Some("| a | b |".to_owned()))), "the hovered one");
    let away = bounds_of(cx, "session-question-skip").center();
    cx.simulate_mouse_move(away, None, Modifiers::default());
    cx.run_until_parked();

    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("3");
    cx.run_until_parked();
    assert_eq!(preview(&view, cx), Some(("Other".to_owned(), None)));
    cx.update(|window, cx| window.focus(&focus, cx));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert_eq!(
        effect_calls(&events),
        [AgentCall::Respond { task: "A".to_owned(), permission: 10, option: Some("reject".to_owned()) }]
    );
}
