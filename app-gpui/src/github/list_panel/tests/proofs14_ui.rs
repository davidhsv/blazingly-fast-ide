use super::*;
use crate::platform::local_time::now_ms;

/// A GitHub timestamp `ago_ms` before now.
fn created_ago(ago_ms: i64) -> String {
    let at = now_ms() as i64 - ago_ms;
    chrono::DateTime::from_timestamp_millis(at).expect("a time the calendar holds").to_rfc3339()
}

/// The Author chooser open with `users` loaded.
fn author_chooser_with(view: &Entity<PrListPanel>, users: Vec<ChooserItem>, cx: &mut VisualTestContext) {
    click(cx, "gh-chip-author");
    view.update(cx, |view, cx| view.set_chooser_items(Chip::Author, users, false, None, cx));
    cx.run_until_parked();
}

fn users() -> Vec<ChooserItem> {
    user_items(&[user_named("ann", Some("Ann Lee")), user_named("bob", Some("Robert")), user("cy")])
}

/// The rows write each pull request's creation as GitHub does: just now (also for a time slightly ahead),
/// minutes, hours, yesterday, days, then the date itself.
#[gpui_kit::test]
async fn formats_relative_dates_like_github(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    const MINUTE: i64 = 60_000;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let ages = [
        20_000,
        -20_000,
        90_000,
        42 * MINUTE + 30_000,
        HOUR + 30 * MINUTE,
        8 * HOUR + 30 * MINUTE,
        DAY + 12 * HOUR,
        9 * DAY + 12 * HOUR,
        400 * DAY,
    ];
    let pulls: Vec<GhPullRequestShort> = ages
        .iter()
        .zip(1..)
        .map(|(ago, number)| GhPullRequestShort { created_at: created_ago(*ago), ..pr(number) })
        .collect();
    answer(&view, &only_fetch(&events), Ok(page(pulls)), cx);
    let rows = view.update(cx, |view, cx| view.rows(cx));
    let created: Vec<String> = rows
        .iter()
        .map(|row| {
            let info = &row.row.info;
            let start = info.find("created ").map_or(0, |at| at + "created ".len());
            let end = info.rfind(", by ").unwrap_or(info.len());
            info[start..end].to_owned()
        })
        .collect();
    assert_eq!(
        created[..8],
        [
            "just now",
            "just now",
            "1 minute ago",
            "42 minutes ago",
            "1 hour ago",
            "8 hours ago",
            "yesterday",
            "9 days ago",
        ]
    );
    let year = (chrono::Utc::now() - chrono::Duration::days(400)).format("%Y").to_string();
    assert!(created[8].starts_with("on ") && created[8].contains(&year), "{:?}", created[8]);
    assert!(cx.debug_bounds("gh-pr-row-9").is_some());
}

/// The chooser of a chip with a value lists that value first and does not list it again among the loaded ones.
#[gpui_kit::test]
async fn lists_the_chosen_items_first_without_duplicates(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    let cy = SearchValue { author: Some("cy".into()), ..SearchValue::default_search() };
    view.update(cx, |view, cx| view.connect(&connection(), cy, Vec::new(), cx));
    cx.run_until_parked();
    fetches(&events);
    author_chooser_with(&view, users(), cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.chooser_rows()),
        Some((Chip::Author, vec!["cy".to_owned(), "ann Ann Lee".to_owned(), "bob Robert".to_owned()]))
    );
    assert!(cx.debug_bounds("gh-overlay-2").is_some() && cx.debug_bounds("gh-overlay-3").is_none());
}

/// Typing in the chooser's filter keeps the rows whose login or name match; Enter chooses the first of them
/// and closes the chooser, and the chip takes the value.
#[gpui_kit::test]
async fn chooses_one_item_and_closes_single(cx: &mut TestAppContext) {
    let (view, events, cx) = open(cx);
    fetches(&events);
    author_chooser_with(&view, users(), cx);
    cx.simulate_input("rob");
    assert_eq!(view.read_with(cx, |view, _| view.chooser_rows()), Some((Chip::Author, vec!["bob Robert".to_owned()])));
    cx.simulate_keystrokes("enter");
    assert_eq!(view.read_with(cx, |view, _| view.chooser_rows()), None);
    assert!(cx.debug_bounds("gh-chooser-filter").is_none(), "the chooser is closed");
    assert!(query_of(&only_fetch(&events)).contains("author:bob"));
    assert_eq!(view.update(cx, |view, cx| view.chips(cx))[1], ("bob".to_owned(), true));
}
