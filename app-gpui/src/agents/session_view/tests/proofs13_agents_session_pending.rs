use super::*;
use std::cell::Cell;

thread_local! {
    /// The clock of the conversations of this file: it moves only when a test lets time pass.
    static NOW: Cell<f64> = const { Cell::new(1_000_000.0) };
}

fn now() -> f64 {
    NOW.with(Cell::get)
}

/// Lets `ms` pass on the view's clock and the executor's.
fn pass(ms: u64, cx: &mut VisualTestContext) {
    NOW.with(|clock| clock.set(clock.get() + ms as f64));
    cx.executor().advance_clock(Duration::from_millis(ms));
    cx.run_until_parked();
}

fn row_top(cx: &mut VisualTestContext, selector: &'static str) -> f32 {
    f32::from(bounds_of(cx, selector).top())
}

/// While the pointer holds the conversation (its scrollbar), earlier entries are not prepended, even scrolled to
/// the top of what is painted; released, they are prepended once scrolling is still, and the row that was first
/// stays where it was on screen.
#[gpui_kit::test]
async fn defers_prepending_during_a_pointer_drag_and_preserves_the_first_rows_viewport_position(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<input::Keymap>() {
            input::init(cx);
        }
    });
    let (store, _) = connected_store(vec![local_task("A")]);
    let store = cx.new(|_| store);
    let shared = store.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| SessionView::new(shared, "A", false, cx).with_clock(now));
    cx.run_until_parked();
    let press = bounds_of(cx, "session-scroll").center();
    cx.simulate_mouse_down(press, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    // A history, then a new tail, arrive while the scrollbar is held.
    let tall = |text: String| agent(&format!("{text}\n\nwith a second paragraph\n\nand a third"));
    let mut entries: Vec<AgentEntry> = (0..90).map(|i| tall(format!("History {i}"))).collect();
    entries.extend((0..90).map(|i| tall(format!("New {i}"))));
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, entries);
        cx.notify();
    });
    cx.run_until_parked();
    let first = view.read_with(cx, |view, _| view.first_painted());
    assert_eq!(first, Some(140), "the last entries are painted");
    view.update(cx, |view, cx| {
        view.scroll.set_offset(point(px(0.0), px(-20.0)));
        cx.notify();
    });
    cx.run_until_parked();
    pass(180, cx);
    assert_eq!(view.read_with(cx, |view, _| view.first_painted()), first, "nothing is prepended during the drag");

    let before = row_top(cx, "session-row-140");
    cx.simulate_mouse_up(press, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    pass(200, cx);
    let painted = view.read_with(cx, |view, _| view.first_painted());
    assert!(painted.is_some_and(|index| index < 140), "earlier entries are prepended: {painted:?}");
    let after = row_top(cx, "session-row-140");
    assert!((after - before).abs() <= 1.0, "the first row stays on screen where it was: {before} → {after}");
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
