use super::*;
use crate::github::data_model::{SearchReviewState, SearchSort, SearchState};
use crate::github::types::{GhReview, ReviewState, ReviewerState};
use gpui_kit::{ScrollDelta, ScrollEvent, point};

/// The overlay rows the panel draws, by index.
const OVERLAY: [&str; 8] = [
    "gh-overlay-0",
    "gh-overlay-1",
    "gh-overlay-2",
    "gh-overlay-3",
    "gh-overlay-4",
    "gh-overlay-5",
    "gh-overlay-6",
    "gh-overlay-7",
];

fn open_quick_filters(view: &Entity<PrListPanel>, cx: &mut VisualTestContext) -> Vec<String> {
    click(cx, "gh-quick-filters");
    view.read_with(cx, |view, _| view.menu_lines()).expect("the Quick Filters are open")
}

fn close_overlay(view: &Entity<PrListPanel>, cx: &mut VisualTestContext) {
    click(cx, "gh-overlay-backdrop");
    assert_eq!(view.read_with(cx, |view, _| view.menu_lines()), None);
}

/// Opens `chip`'s chooser and chooses the row reading `text`.
fn choose(view: &Entity<PrListPanel>, chip: Chip, text: &str, cx: &mut VisualTestContext) {
    view.update_in(cx, |view, window, cx| view.open_chooser(chip, window, cx));
    cx.run_until_parked();
    let (open, rows) = view.read_with(cx, |view, _| view.chooser_rows()).expect("the chooser is open");
    assert_eq!(open, chip);
    let index = rows.iter().position(|row| row == text).unwrap_or_else(|| panic!("{text} in {rows:?}"));
    click(cx, OVERLAY[index]);
}

fn last_searches_saved(events: &Events) -> usize {
    events.borrow().iter().filter(|e| matches!(e, ListPanelEvent::SaveLastSearch(_))).count()
}

/// The Quick Filters are IntelliJ's four; one is checked only while the search is exactly its own, so adding a
/// sort to "Assigned to you" leaves none checked.
#[gpui_kit::test]
async fn offers_intellijs_quick_filters_checked_only_on_an_exact_match(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    fetches(&events);
    let lines = open_quick_filters(&view, cx);
    let titles: Vec<&str> =
        lines[1..5].iter().map(|line| line.trim_start_matches("> ").trim_start_matches("✓ ")).collect();
    assert_eq!(titles, ["Open", "Your pull requests", "Assigned to you", "Review requests"]);
    click(cx, OVERLAY[2]);
    let query = query_of(&only_fetch(&events));
    assert!(query.contains("is:open") && query.contains("author:me"), "{query}");
    let lines = open_quick_filters(&view, cx);
    assert!(lines[2].ends_with("✓ Your pull requests"), "{lines:?}");
    click(cx, OVERLAY[3]);
    fetches(&events);
    let lines = open_quick_filters(&view, cx);
    assert!(lines[3].ends_with("✓ Assigned to you"), "{lines:?}");
    close_overlay(&view, cx);
    choose(&view, Chip::Sort, "Newest", cx);
    assert!(query_of(&only_fetch(&events)).contains("assignee:me"));
    let lines = open_quick_filters(&view, cx);
    assert!(!lines.iter().any(|line| line.contains('✓')), "{lines:?}");
    close_overlay(&view, cx);
}

/// The history drops down each saved search as IntelliJ's short text: the text quoted, then each filter.
#[gpui_kit::test]
async fn writes_the_history_text_like_get_short_text(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    fetches(&events);
    let full = SearchValue {
        search_query: Some("foo".into()),
        state: Some(SearchState::Open),
        label: Some("bug".into()),
        assignee: Some("me".into()),
        review_state: Some(SearchReviewState::AwaitingReview),
        author: Some("x".into()),
        sort: Some(SearchSort::Newest),
    };
    let reviewed = SearchValue { review_state: Some(SearchReviewState::ReviewedByMe), ..SearchValue::default() };
    view.update(cx, |view, cx| view.connect(&connection(), initial_search(None), vec![full, reviewed], cx));
    cx.run_until_parked();
    click(cx, "gh-search-history");
    let lines = view.read_with(cx, |view, _| view.menu_lines()).expect("the history is open");
    let texts: Vec<&str> = lines.iter().map(|line| line.trim_start_matches("> ")).collect();
    let full = r#""foo" state:"Open" label:bug assignee:me reviewState:"Awaiting review" author:x sort:"Newest""#;
    assert_eq!(texts, [full, r#"reviewState:"Reviewed""#]);
}

/// Text applied with Enter joins the search; emptied and applied again, it leaves the search.
#[gpui_kit::test]
async fn sets_or_clears_the_text(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    fetches(&events);
    focus_field(&view, cx);
    cx.simulate_input("fix");
    cx.simulate_keystrokes("enter");
    assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app (fix) is:open");
    let search = view.read_with(cx, |view, _| view.search().clone());
    assert_eq!(search, SearchValue { search_query: Some("fix".into()), ..SearchValue::default_search() });
    focus_field(&view, cx);
    let field = view.read_with(cx, |view, _| view.field.clone());
    field.update(cx, |field, cx| field.select_all(cx));
    cx.simulate_keystrokes("backspace enter");
    assert_eq!(query_of(&only_fetch(&events)), "type:pr repo:org/app is:open");
    assert_eq!(view.read_with(cx, |view, _| view.search().clone()), SearchValue::default_search());
}

/// A search with new text joins the history at once; chip-only changes join it once they have stood ten seconds,
/// a new change starting the wait again; the empty search and the default one never join it.
#[gpui_kit::test]
async fn saves_searches_by_intellijs_rules_waiting_10_seconds_for_chip_only_changes(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    fetches(&events);
    assert_eq!(last_searches_saved(&events), 1);
    focus_field(&view, cx);
    cx.simulate_input("a");
    cx.simulate_keystrokes("enter");
    let text = SearchValue { search_query: Some("a".into()), ..SearchValue::default_search() };
    assert_eq!(saved_histories(&events), [vec![text.clone()]]);

    choose(&view, Chip::State, "Closed", cx);
    cx.executor().advance_clock(Duration::from_millis(5_000));
    cx.run_until_parked();
    choose(&view, Chip::State, "Merged", cx);
    cx.executor().advance_clock(Duration::from_millis(CHIP_ONLY_SAVE_DELAY_MS as u64 - 1));
    cx.run_until_parked();
    assert_eq!(saved_histories(&events).len(), 1, "the wait started again");
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    let merged = SearchValue { state: Some(SearchState::Merged), ..text };
    let histories = saved_histories(&events);
    assert_eq!(histories.len(), 2);
    assert_eq!(histories.last().and_then(|history| history.first()), Some(&merged));

    // Clear filters (the empty search), then the default search: neither joins the history.
    let last = fetches(&events).pop().expect("the merged search is asked for");
    answer(&view, &last, Ok(page(Vec::new())), cx);
    click(cx, "gh-list-clear-filters");
    assert_eq!(view.read_with(cx, |view, _| view.search().clone()), SearchValue::default());
    open_quick_filters(&view, cx);
    click(cx, OVERLAY[1]);
    assert_eq!(view.read_with(cx, |view, _| view.search().clone()), SearchValue::default_search());
    cx.executor().advance_clock(Duration::from_millis(20_000));
    cx.run_until_parked();
    assert_eq!(saved_histories(&events).len(), 2);
    assert_eq!(last_searches_saved(&events), 6);
}

/// A row draws its reviewers approved first, then changes requested, commented, re-requested and still to
/// review, cut to the avatars it has room for.
#[gpui_kit::test]
async fn orders_reviewers_approved_changes_requested_commented_re_requested_needs_review(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let review = |author: GhUser, state: ReviewState, at: u32| GhReview {
        id: format!("{}{at}", author.login),
        author: Some(author),
        state,
        created_at: format!("2020-01-01T00:00:0{at}Z"),
        ..GhReview::default()
    };
    let reviewed = GhPullRequestShort {
        reviews: Nodes::of(vec![
            review(user("c"), ReviewState::Commented, 1),
            review(user_named("a", Some("Ada")), ReviewState::Approved, 2),
            review(user("r"), ReviewState::Commented, 3),
            review(user("x"), ReviewState::ChangesRequested, 4),
        ]),
        review_requests: Nodes::of(vec![
            ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(user("n"))) },
            ReviewRequest { requested_reviewer: Some(GhReviewer::Actor(user("r"))) },
        ]),
        ..pr(1)
    };
    answer(&view, &only_fetch(&events), Ok(page(vec![reviewed])), cx);
    let rows = view.update(cx, |view, cx| view.rows(cx));
    let shown: Vec<(String, ReviewerState)> = rows[0].row.reviewers.iter().map(|r| (r.name.clone(), r.state)).collect();
    let all = [
        ("Ada", ReviewerState::Approved),
        ("x", ReviewerState::ChangesRequested),
        ("c", ReviewerState::Commented),
        ("r", ReviewerState::ReRequested),
        ("n", ReviewerState::NeedsReview),
    ]
    .map(|(name, state)| (name.to_owned(), state));
    assert_eq!(shown, all[..AVATAR_STACK_MAX]);
    assert!(cx.debug_bounds("gh-pr-row-1").is_some());
}

/// A row's info line names its number, its creation date and its author (none without one); the comment count's
/// tooltip says how many comments are unresolved.
#[gpui_kit::test]
async fn formats_the_info_line_and_the_comment_tooltip(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let unresolved = |n: usize| Nodes::of(vec![ThreadStatus { is_resolved: false, is_outdated: false }; n]);
    let pulls = vec![
        GhPullRequestShort { review_threads: unresolved(0), ..pr(12) },
        GhPullRequestShort { author: None, review_threads: unresolved(1), ..pr(13) },
        GhPullRequestShort { review_threads: unresolved(3), ..pr(14) },
    ];
    answer(&view, &only_fetch(&events), Ok(page(pulls)), cx);
    let rows = view.update(cx, |view, cx| view.rows(cx));
    let info: Vec<&str> = rows.iter().map(|r| r.row.info.as_str()).collect();
    assert!(info[0].starts_with("#12 · created ") && info[0].ends_with(", by ann"), "{info:?}");
    assert!(info[1].starts_with("#13 · created ") && !info[1].contains(", by"), "{info:?}");
    let tooltips: Vec<&str> = rows.iter().map(|r| r.row.comments_tooltip.as_str()).collect();
    assert_eq!(tooltips, ["No unresolved comments", "1 unresolved comment", "3 unresolved comments"]);
}

/// While the rows fill the list nothing more is asked for; scrolled past 70% of it, the list asks for the next
/// page from the last cursor.
#[gpui_kit::test]
async fn requests_the_next_page_past_70_percent_of_the_scroll(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let first = Connection {
        page_info: PageInfo { has_next_page: true, end_cursor: Some("c1".into()), ..PageInfo::default() },
        nodes: (1..=200).map(pr).collect(),
    };
    answer(&view, &only_fetch(&events), Ok(first), cx);
    assert!(fetches(&events).is_empty(), "the first rows fill the list");
    let at = bounds_of(cx, "gh-pr-row-1").center();
    let delta = ScrollDelta::Pixels(point(px(0.0), px(-20_000.0)));
    cx.simulate_event(ScrollEvent { position: at, delta, ..Default::default() });
    cx.run_until_parked();
    let next = only_fetch(&events);
    assert_eq!(variables(&next.request)["cursor"], "c1");
}
