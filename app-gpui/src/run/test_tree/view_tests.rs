use super::model::RunTests;
use super::view::{TestTreeCommand, TestTreeEvent, TestTreeView, TreeSession};
use crate::platform::storage::UiStorage;
use crate::platform_more::occurrences::{Direction, OccurrenceStep};
use crate::run::configs::blank_config;
use crate::run::console::{ChunkKind, CompletedLine};
use crate::theme;
use gpui_kit::{
    AppContext as _, Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, TestAppContext, VisualTestContext,
};
use ide_run::RunConfig;
use std::cell::RefCell;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<TestTreeEvent>>>;

/// `adds` passes, `subtracts` fails an `assert_eq!` at `src/lib.rs:9:5`.
const MATH: &str = "running 2 tests
test math::adds ... ok
test math::subtracts ... FAILED

failures:

---- math::subtracts stdout ----
thread 'math::subtracts' panicked at src/lib.rs:9:5:
assertion `left == right` failed
  left: 1
 right: 2

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s";

/// Failures in a nested module and between passing tests; only `db::writes` is located.
const NESTED: &str = "test api::lists ... ok
test api::auth::rejects ... FAILED
test api::auth::accepts ... ok
test db::reads ... FAILED
test db::skips ... ignored
test db::writes ... FAILED

failures:

---- db::writes stdout ----
thread 'db::writes' panicked at src/db.rs:20:5:
boom
";

/// A failure whose output names the expected and the actual value.
const JUNIT_STYLE: &str = "test math::subs ... FAILED

failures:

---- math::subs stdout ----
org.opentest4j.AssertionFailedError: expected: <1> but was: <2>
";

const PASSING: &str = "test api::lists ... ok\ntest db::reads ... ok\n";

/// A failure deep in one suite, passing tests in two others.
const DEEP: &str = "test api::auth::tokens::refreshes ... FAILED
test api::users::lists ... ok
test db::pool::reuses ... ok
";

fn config() -> RunConfig {
    let config = blank_config("cargo:test", "cargo test", "cargo");
    RunConfig { command: "cargo".into(), args: vec!["test".into()], is_test: true, ..config }
}

fn lines(text: &str) -> Vec<CompletedLine> {
    text.lines().map(|line| CompletedLine { text: line.to_owned(), kind: ChunkKind::Stdout }).collect()
}

/// A finished run of `output` in session `s1`, shown in a tree.
type Opened<'a> = (Entity<TestTreeView>, Events, &'a mut VisualTestContext);

fn open<'a>(cx: &'a mut TestAppContext, output: &str, code: i32) -> Opened<'a> {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        if !cx.has_global::<UiStorage>() {
            cx.set_global(UiStorage::default());
        }
    });
    let tests = cx.new(|_| RunTests::new());
    tests.update(cx, |tests, _| {
        tests.started("s1", 1, &config());
        tests.feed("s1", 1, &lines(output));
        tests.finish("s1", 1, Some(code));
    });
    let (view, cx) = cx.add_window_view(|_, cx| TestTreeView::new(tests, cx));
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|window, cx| {
        window.activate_window();
        cx.subscribe(&view, move |_, event: &TestTreeEvent, _| seen.borrow_mut().push(event.clone())).detach();
    });
    let session = TreeSession { id: "s1".into(), running: false, cwd: "/p".into(), base_config: config() };
    view.update(cx, |view, cx| view.show(Some(session), cx));
    cx.run_until_parked();
    (view, events, cx)
}

fn rows(view: &Entity<TestTreeView>, cx: &mut VisualTestContext) -> Vec<String> {
    view.read_with(cx, |view, _| view.rows())
}

fn click(cx: &mut VisualTestContext, selector: &'static str, count: usize) {
    cx.run_until_parked();
    let position = cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn")).center();
    let (modifiers, button) = (Modifiers::default(), MouseButton::Left);
    cx.simulate_event(MouseDownEvent { position, modifiers, button, click_count: count, first_mouse: false });
    cx.simulate_event(MouseUpEvent { position, modifiers, button, click_count: count });
    cx.run_until_parked();
}

fn opened(events: &Events) -> Vec<(String, Option<u32>, bool)> {
    events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            TestTreeEvent::OpenLocation { target, focus, cwd } => {
                assert_eq!(cwd, "/p", "locations resolve against the run directory");
                Some((target.path.clone(), target.line, *focus))
            }
            _ => None,
        })
        .collect()
}

fn step(view: &Entity<TestTreeView>, forward: bool, focus: bool, cx: &mut VisualTestContext) -> Option<OccurrenceStep> {
    let direction = if forward { Direction::Next } else { Direction::Previous };
    view.update(cx, |view, cx| view.step_failed(direction, focus, cx))
}

fn selected_name(view: &Entity<TestTreeView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.selected().map(|id| id.rsplit('\0').next().unwrap_or(id).to_owned()))
}

fn enabled(view: &Entity<TestTreeView>, id: &str, cx: &mut VisualTestContext) -> bool {
    view.read_with(cx, |view, _| view.header_buttons().iter().any(|button| button.id == id && button.enabled))
}

#[gpui_kit::test]
async fn hides_passed_tests_by_default_and_remembers_the_toggle(cx: &mut TestAppContext) {
    let (view, _, vcx) = open(cx, MATH, 101);
    assert_eq!(rows(&view, vcx), ["math", "subtracts"]);
    click(vcx, "test-show-passed", 1);
    assert_eq!(rows(&view, vcx), ["math", "adds", "subtracts"]);
    let (again, _, vcx) = open(cx, MATH, 101);
    assert_eq!(rows(&again, vcx), ["math", "adds", "subtracts"]);
    assert!(again.read_with(vcx, |view, _| view.filters().show_passed));
}

// Not from a spec: Show Ignored hides the ignored tests and shows them again.
#[gpui_kit::test]
async fn show_ignored_hides_and_shows_the_ignored_tests(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx, NESTED, 101);
    assert!(rows(&view, cx).contains(&"skips".to_owned()));
    click(cx, "test-show-ignored", 1);
    assert!(!rows(&view, cx).contains(&"skips".to_owned()));
    assert!(!view.read_with(cx, |view, _| view.filters().show_ignored));
    click(cx, "test-show-ignored", 1);
    assert!(rows(&view, cx).contains(&"skips".to_owned()));
}

#[gpui_kit::test]
async fn a_click_selects_without_navigating_double_click_jumps_to_the_source(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx, MATH, 101);
    click(cx, "test-row:math/subtracts", 1);
    assert_eq!(selected_name(&view, cx).as_deref(), Some("subtracts"));
    assert!(opened(&events).is_empty());
    let output = view.read_with(cx, |view, cx| view.selected_output(cx)).expect("the test printed its failure");
    assert!(output.iter().any(|line| line.contains("left: 1")), "{output:?}");
    assert!(events.borrow().iter().any(|event| matches!(event, TestTreeEvent::Selected { test: Some(_), .. })));
    click(cx, "test-row:math/subtracts", 2);
    assert_eq!(opened(&events), [("src/lib.rs".to_owned(), Some(9), true)]);
}

#[gpui_kit::test]
async fn context_menu_run_debug_the_test_and_show_diff_of_an_assertion_failure(app: &mut TestAppContext) {
    let (view, events, cx) = open(app, MATH, 101);
    let at = cx.debug_bounds("test-row:math/subtracts").expect("drawn").center();
    cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::default());
    cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::default());
    cx.run_until_parked();
    assert!(matches!(events.borrow().last(), Some(TestTreeEvent::ShowMenu(_))));
    let items = view.update(cx, |view, cx| view.context_menu(cx));
    let labels: Vec<&str> = items.iter().filter_map(|item| item.label()).collect();
    assert_eq!(labels, ["Run 'subtracts'", "Debug 'subtracts'", "Show Diff", "Jump to Source"]);
    assert!(!items[1].is_disabled(), "cargo can run a single test");
    let debug = items[1].command().cloned().expect("a command");
    view.update(cx, |view, cx| view.run_command(debug, cx));
    let id = "math\0subtracts".to_owned();
    assert_eq!(
        events.borrow().last(),
        Some(&TestTreeEvent::RunTests { session: "s1".into(), ids: vec![id.clone()], debug: true })
    );
    view.update(cx, |view, cx| view.run_command(TestTreeCommand::ShowDiff(id), cx));
    let Some(TestTreeEvent::ShowDiff(arg)) = events.borrow().last().cloned() else { panic!("a diff is asked for") };
    assert_eq!(arg["left"], serde_json::json!({ "text": "1", "label": "Left" }));
    assert_eq!(arg["right"], serde_json::json!({ "text": "2", "label": "Right" }));
    assert_eq!(arg["path"], "math/subtracts.txt");
    assert_eq!(arg["title"], "Comparison Failure: subtracts");

    // A framework that says which side is expected: Expected on the left, Actual on the right.
    let (view, events, cx) = open(app, JUNIT_STYLE, 101);
    view.update(cx, |view, cx| view.run_command(TestTreeCommand::ShowDiff("math\0subs".to_owned()), cx));
    let Some(TestTreeEvent::ShowDiff(arg)) = events.borrow().last().cloned() else { panic!("a diff is asked for") };
    assert_eq!(arg["left"], serde_json::json!({ "text": "1", "label": "Expected" }));
    assert_eq!(arg["right"], serde_json::json!({ "text": "2", "label": "Actual" }));
}

#[gpui_kit::test]
async fn steps_through_the_failed_tests_in_tree_order_and_stops_at_both_ends(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx, NESTED, 101);
    assert!(enabled(&view, "test-prev-failed", cx) && enabled(&view, "test-next-failed", cx));
    let place = |s: Option<OccurrenceStep>| s.map(|s| (s.index, s.total));
    assert_eq!(place(step(&view, true, false, cx)), Some((1, 3)));
    assert_eq!(selected_name(&view, cx).as_deref(), Some("rejects"));
    assert_eq!(place(step(&view, true, false, cx)), Some((2, 3)));
    assert_eq!(selected_name(&view, cx).as_deref(), Some("reads"));
    assert_eq!(place(step(&view, true, false, cx)), Some((3, 3)));
    assert_eq!(selected_name(&view, cx).as_deref(), Some("writes"));
    assert!(!enabled(&view, "test-next-failed", cx));
    assert!(enabled(&view, "test-prev-failed", cx));
    assert_eq!(step(&view, true, false, cx), None);
    assert_eq!(selected_name(&view, cx).as_deref(), Some("writes"));
    assert_eq!(place(step(&view, false, false, cx)), Some((2, 3)));
    assert_eq!(place(step(&view, false, false, cx)), Some((1, 3)));
    assert!(!enabled(&view, "test-prev-failed", cx));
    assert_eq!(step(&view, false, false, cx), None);
    assert_eq!(selected_name(&view, cx).as_deref(), Some("rejects"));
}

#[gpui_kit::test]
async fn expands_the_collapsed_suites_of_the_failure_it_selects(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx, NESTED, 101);
    click(cx, "test-toggle:api", 1);
    assert!(!rows(&view, cx).contains(&"rejects".to_owned()));
    step(&view, true, false, cx);
    assert_eq!(selected_name(&view, cx).as_deref(), Some("rejects"));
    assert!(rows(&view, cx).contains(&"rejects".to_owned()));
}

#[gpui_kit::test]
async fn opens_the_failures_source_focusing_the_editor_only_when_asked(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx, NESTED, 101);
    step(&view, false, false, cx);
    assert_eq!(opened(&events), [("src/db.rs".to_owned(), Some(20), false)]);
    step(&view, false, true, cx);
    assert_eq!(opened(&events).len(), 1, "reads has no location: nothing to open");
}

#[gpui_kit::test]
async fn is_unavailable_when_no_test_failed(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx, PASSING, 0);
    assert!(!enabled(&view, "test-prev-failed", cx) && !enabled(&view, "test-next-failed", cx));
    assert_eq!(step(&view, true, false, cx), None);
    assert_eq!(step(&view, false, false, cx), None);
    assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("Passed tests are hidden"));
}

#[gpui_kit::test]
async fn runs_previous_next_occurrence_so_cmd_alt_up_down_do_the_same(cx: &mut TestAppContext) {
    let (_, events, cx) = open(cx, NESTED, 101);
    click(cx, "test-next-failed", 1);
    click(cx, "test-prev-failed", 1);
    let actions: Vec<&str> = events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            TestTreeEvent::RunAction(id) => Some(*id),
            _ => None,
        })
        .collect();
    assert_eq!(actions, ["navigate.nextOccurrence", "navigate.prevOccurrence"]);
}

#[gpui_kit::test]
async fn opens_every_suite_including_those_of_hidden_passed_tests_and_folds_back_to_the_top_level_suites(
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        let mut storage = UiStorage::default();
        storage.set("openidea.run.testFilters", r#"{"showPassed":true}"#.to_owned());
        cx.set_global(storage);
    });
    let (view, _, cx) = open(cx, DEEP, 101);
    assert_eq!(rows(&view, cx), ["api", "auth", "tokens", "refreshes", "users", "db", "pool"]);
    view.update(cx, |view, cx| view.expand_all(cx));
    assert_eq!(rows(&view, cx), ["api", "auth", "tokens", "refreshes", "users", "lists", "db", "pool", "reuses"]);
    view.update(cx, |view, cx| view.collapse_all(cx));
    assert_eq!(rows(&view, cx), ["api", "db"]);
    view.update(cx, |view, cx| view.expand_all(cx));
    click(cx, "test-show-passed", 1);
    assert_eq!(rows(&view, cx), ["api", "auth", "tokens", "refreshes"]);
    click(cx, "test-show-passed", 1);
    assert!(rows(&view, cx).contains(&"reuses".to_owned()));
}

#[gpui_kit::test]
async fn the_buttons_run_the_actions_and_are_unavailable_without_suites(cx: &mut TestAppContext) {
    let (view, events, vcx) = open(cx, DEEP, 101);
    click(vcx, "test-expand-all", 1);
    click(vcx, "test-collapse-all", 1);
    let last_two: Vec<TestTreeEvent> = events.borrow().iter().rev().take(2).rev().cloned().collect();
    assert_eq!(
        last_two,
        [TestTreeEvent::RunAction("run.expandAllTests"), TestTreeEvent::RunAction("run.collapseAllTests")]
    );
    assert!(enabled(&view, "test-expand-all", vcx));
    let (flat, _, vcx) = open(cx, "test main_test ... FAILED\n", 101);
    assert!(!enabled(&flat, "test-expand-all", vcx));
    assert!(!enabled(&flat, "test-collapse-all", vcx));
}

#[gpui_kit::test]
async fn the_header_shows_the_runs_status_counts_and_duration_and_rows_their_status(cx: &mut TestAppContext) {
    let (view, _, vcx) = open(cx, MATH, 101);
    assert_eq!(view.update(vcx, |view, cx| view.header(cx)), ["1 failing", "1 passed", "10 ms"]);
    assert_eq!(view.read_with(vcx, |view, _| view.header_icon()), "test-failed");
    let marks = view.read_with(vcx, |view, _| view.marks());
    assert_eq!(marks, [("math".to_owned(), "test-failed"), ("subtracts".to_owned(), "test-failed")]);
    let (passing, _, vcx) = open(cx, PASSING, 0);
    passing.update(vcx, |view, cx| view.toggle_show_passed(cx));
    assert_eq!(passing.read_with(vcx, |view, _| view.header_icon()), "test-passed");
    assert!(passing.read_with(vcx, |view, _| view.marks()).iter().all(|(_, icon)| *icon == "test-passed"));
}

#[gpui_kit::test]
async fn arrows_move_and_fold_and_enter_and_f4_jump_to_the_source(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx, NESTED, 101);
    click(cx, "test-row:api", 1);
    cx.simulate_keystrokes("left");
    assert_eq!(rows(&view, cx), ["api", "db", "reads", "skips", "writes"]);
    cx.simulate_keystrokes("right right");
    assert_eq!(selected_name(&view, cx).as_deref(), Some("auth"));
    cx.simulate_keystrokes("end");
    assert_eq!(selected_name(&view, cx).as_deref(), Some("writes"));
    cx.simulate_keystrokes("enter");
    cx.simulate_keystrokes("f4");
    cx.simulate_keystrokes("cmd-down");
    assert_eq!(opened(&events).len(), 3);
    cx.simulate_keystrokes("left");
    assert_eq!(selected_name(&view, cx).as_deref(), Some("db"));
    cx.simulate_keystrokes("enter");
    let last = opened(&events).last().map(|(path, _, _)| path.clone());
    assert_eq!(last.as_deref(), Some("src/db.rs"), "a suite opens the source of its located failure");
}

#[gpui_kit::test]
async fn a_running_session_without_results_waits_for_them(cx: &mut TestAppContext) {
    let (view, _, cx) = open(cx, "", 0);
    let running = TreeSession { id: "s2".into(), running: true, cwd: "/p".into(), base_config: config() };
    view.update(cx, |view, cx| view.show(Some(running), cx));
    assert_eq!(view.read_with(cx, |view, cx| view.empty_text(cx)).as_deref(), Some("Waiting for test results…"));
    assert_eq!(view.update(cx, |view, cx| view.header(cx)), ["Running…"]);
    assert_eq!(view.read_with(cx, |view, _| view.header_icon()), "test-pending");
}
