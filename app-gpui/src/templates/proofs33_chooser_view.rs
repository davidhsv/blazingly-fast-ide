//! The template chooser on its own: what it lists for a request, how typing filters it or picks by
//! mnemonic, how the keys move through it, and what it reports.

use super::chooser_view::*;
use super::popup_store::{ChooserItem, ChooserRequest};
use crate::platform::input;
use crate::theme;
use gpui_kit::{Entity, Focusable, TestAppContext, VisualTestContext};
use std::cell::RefCell;
use std::rc::Rc;

type Events = Rc<RefCell<Vec<ChooserEvent>>>;

fn item(key: &str, label: &str, detail: Option<&str>) -> ChooserItem {
    ChooserItem { detail: detail.map(str::to_owned), ..ChooserItem::new(key, label) }
}

fn with_mnemonic(key: &str, label: &str, mnemonic: &str, index: Option<u32>) -> ChooserItem {
    ChooserItem { mnemonic: Some(mnemonic.to_owned()), mnemonic_index: index, ..ChooserItem::new(key, label) }
}

/// The request Choose Template makes for `log`, `fori` and `forof`.
fn templates_request(filter: &str) -> ChooserRequest {
    ChooserRequest {
        title: "Choose Template".to_owned(),
        items: vec![
            ChooserItem { search: Some("console.log".to_owned()), ..item("t.log", "log", Some("console.log")) },
            ChooserItem {
                search: Some("Indexed for loop".to_owned()),
                ..item("t.fori", "fori", Some("Indexed for loop"))
            },
            ChooserItem { search: Some("Iterate".to_owned()), ..item("t.forof", "forof", Some("Iterate")) },
        ],
        filter: filter.to_owned(),
        filterable: true,
        empty_text: Some("No matching templates".to_owned()),
        ..ChooserRequest::default()
    }
}

fn open(cx: &mut TestAppContext, request: ChooserRequest) -> (Entity<TemplateChooser>, Events, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
    });
    let (chooser, cx) = cx.add_window_view(move |_, cx| TemplateChooser::new(request, cx));
    let events: Events = Rc::default();
    let seen = Rc::clone(&events);
    cx.update(|window, cx| {
        cx.subscribe(&chooser, move |_, event: &ChooserEvent, _| seen.borrow_mut().push(event.clone())).detach();
        window.activate_window();
        window.focus(&chooser.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (chooser, events, cx)
}

fn lines(chooser: &Entity<TemplateChooser>, cx: &mut VisualTestContext) -> Vec<String> {
    chooser.read_with(cx, |chooser, _| chooser.lines())
}

fn selected(chooser: &Entity<TemplateChooser>, cx: &mut VisualTestContext) -> Option<String> {
    chooser.read_with(cx, |chooser, _| chooser.selected_key().map(str::to_owned))
}

fn picked(key: &str, filter: &str) -> ChooserEvent {
    ChooserEvent::Picked { key: key.to_owned(), filter: filter.to_owned() }
}

// Not from a spec: a filter keeps the items whose label starts with it first, then the ones it matches
// loosely or whose search text holds it; section rows show only while nothing is typed.
#[test]
fn the_filter_puts_prefix_matches_first_then_loose_and_search_matches() {
    let items = [
        item("a", "forof", None),
        ChooserItem::section("s", "Live templates"),
        item("b", "afor", None),
        ChooserItem { search: Some("Loop over".to_owned()), ..item("c", "it", None) },
        item("d", "fo", None),
    ];
    let keys = |filter: &str| -> Vec<String> {
        filter_chooser_items(&items, filter).into_iter().map(|row| row.item.key).collect()
    };
    assert_eq!(keys(""), ["a", "s", "b", "c", "d"]);
    assert_eq!(keys("fo"), ["a", "d", "b"]);
    assert_eq!(keys("LOOP"), ["c"]);
    assert!(keys("zz").is_empty());
    let rows = filter_chooser_items(&items, "fo");
    assert_eq!((rows[0].matched.len(), rows[0].matched.first()), (1, Some(&(0..2))));
}

// Not from a spec: the arrows wrap around and step over section rows; Home, End and the page keys stop
// at the ends.
#[test]
fn the_keys_move_over_items_and_skip_section_rows() {
    let items =
        [item("a", "a", None), ChooserItem::section("s", "Live templates"), item("b", "b", None), item("c", "c", None)];
    let rows = filter_chooser_items(&items, "");
    assert_eq!(navigate(&rows, 0, "down"), Some(2));
    assert_eq!(navigate(&rows, 2, "up"), Some(0));
    assert_eq!(navigate(&rows, 0, "up"), Some(3));
    assert_eq!(navigate(&rows, 3, "down"), Some(0));
    assert_eq!(navigate(&rows, 2, "home"), Some(0));
    assert_eq!(navigate(&rows, 0, "end"), Some(3));
    assert_eq!(navigate(&rows, 0, "pagedown"), Some(3));
    assert_eq!(navigate(&rows, 3, "pageup"), Some(0));
    assert_eq!(navigate(&rows, 0, "left"), None);
    assert_eq!(navigate(&[], 0, "down"), None);
}

// Choose Template lists the templates with their descriptions; typing filters them, Backspace takes
// the last letter back, the arrows move and Enter picks the selected one.
#[gpui_kit::test]
async fn typing_filters_the_templates_and_enter_picks_the_selected_one(cx: &mut TestAppContext) {
    let (chooser, events, cx) = open(cx, templates_request(""));
    assert_eq!(chooser.read_with(cx, |chooser, cx| chooser.title(cx)), "Choose Template");
    assert_eq!(lines(&chooser, cx), ["log · console.log", "fori · Indexed for loop", "forof · Iterate"]);
    assert_eq!(selected(&chooser, cx).as_deref(), Some("t.log"));
    cx.simulate_keystrokes("f o");
    assert_eq!(chooser.read_with(cx, |chooser, _| chooser.filter().to_owned()), "fo");
    assert_eq!(lines(&chooser, cx), ["fori · Indexed for loop", "forof · Iterate"]);
    cx.simulate_keystrokes("down");
    assert_eq!(selected(&chooser, cx).as_deref(), Some("t.forof"));
    cx.simulate_keystrokes("backspace backspace");
    assert_eq!(lines(&chooser, cx).len(), 3);
    cx.simulate_keystrokes("z");
    assert!(lines(&chooser, cx).is_empty());
    assert_eq!(selected(&chooser, cx), None);
    // Enter with nothing listed picks nothing.
    cx.simulate_keystrokes("enter");
    assert!(events.borrow().is_empty());
    cx.simulate_keystrokes("backspace l");
    cx.simulate_keystrokes("enter");
    assert_eq!(*events.borrow(), [picked("t.log", "l")]);
}

// The initial filter is applied at once; Escape cancels.
#[gpui_kit::test]
async fn the_initial_filter_applies_and_escape_cancels(cx: &mut TestAppContext) {
    let (chooser, events, cx) = open(cx, templates_request("for"));
    assert_eq!(lines(&chooser, cx), ["fori · Indexed for loop", "forof · Iterate"]);
    cx.simulate_keystrokes("escape");
    assert_eq!(*events.borrow(), [ChooserEvent::Cancelled]);
}

// Surround With: the mnemonics show before a surrounder's label and inside a template's abbreviation,
// the section row is not selectable, and typing a mnemonic picks its item whatever its case; Tab picks
// the selected one.
#[gpui_kit::test]
async fn a_mnemonic_picks_its_item_and_section_rows_are_skipped(cx: &mut TestAppContext) {
    let request = ChooserRequest {
        title: "Surround With".to_owned(),
        items: vec![
            with_mnemonic("java.0", "if", "1", None),
            with_mnemonic("java.1", "if / else", "2", None),
            ChooserItem::section("\u{0}templates", "Live templates"),
            ChooserItem {
                detail: Some("Callable".to_owned()),
                ..with_mnemonic("surround.java.callable", "C", "C", Some(0))
            },
            ChooserItem::section("\u{0}separator", ""),
            ChooserItem::new("\u{0}configure", "Configure Live Templates…"),
        ],
        mnemonics: true,
        empty_text: Some("Nothing to surround with here".to_owned()),
        ..ChooserRequest::default()
    };
    let (chooser, events, cx) = open(cx, request);
    assert_eq!(
        lines(&chooser, cx),
        ["1. if", "2. if / else", "-- Live templates", "C · Callable", "--", "Configure Live Templates…"]
    );
    cx.simulate_keystrokes("down down");
    assert_eq!(selected(&chooser, cx).as_deref(), Some("surround.java.callable"));
    cx.simulate_keystrokes("c");
    assert_eq!(*events.borrow(), [picked("surround.java.callable", "")]);
}

// A digit picks the surrounder it numbers.
#[gpui_kit::test]
async fn a_digit_picks_the_surrounder_it_numbers(cx: &mut TestAppContext) {
    let request = ChooserRequest {
        title: "Surround With".to_owned(),
        items: vec![with_mnemonic("java.0", "if", "1", None), with_mnemonic("java.1", "if / else", "2", None)],
        mnemonics: true,
        ..ChooserRequest::default()
    };
    let (_, events, cx) = open(cx, request);
    cx.simulate_keystrokes("2");
    assert_eq!(*events.borrow(), [picked("java.1", "")]);
}

// With nothing to list the chooser says what its request says.
#[gpui_kit::test]
async fn an_empty_chooser_shows_its_empty_text(cx: &mut TestAppContext) {
    let request = ChooserRequest {
        title: "Surround with Live Template".to_owned(),
        mnemonics: true,
        empty_text: Some("No live templates to surround with".to_owned()),
        ..ChooserRequest::default()
    };
    let (chooser, events, cx) = open(cx, request);
    assert!(lines(&chooser, cx).is_empty());
    assert!(cx.debug_bounds("template-chooser-empty").is_some());
    cx.simulate_keystrokes("tab");
    assert!(events.borrow().is_empty());
}
