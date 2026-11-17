use super::*;
use crate::everywhere::model::SectionId;
use crate::platform::storage::UiStorage;
use gpui_kit::{TestAppContext, VisualTestContext};

/// The popup on Files in a window of its own, with no backend: the test answers its questions itself.
fn files_popup(cx: &mut TestAppContext) -> (Entity<SearchEverywhere>, &mut VisualTestContext) {
    cx.update(|cx| {
        cx.set_global(theme::ActiveTheme::default());
        input::init(cx);
        cx.set_global(Localization::default());
        cx.set_global(ActionRegistry::default());
        cx.set_global(UiStorage::default());
    });
    cx.add_window_view(|_, cx| {
        let context = Rc::new(ContextKeys::new());
        SearchEverywhere::new("w1", SeTab::Files, Some("order"), Some("/p".to_owned()), context, Vec::new(), cx)
    })
}

fn found(rel: &str) -> SeAnswer {
    let file = FileMatch { path: format!("/p/{rel}"), rel_path: rel.to_owned(), score: 1, indices: Vec::new() };
    SeAnswer::Files(vec![file])
}

fn generation(popup: &Entity<SearchEverywhere>, cx: &mut VisualTestContext) -> u64 {
    popup.read_with(cx, |popup, _| popup.controller.snapshot().generation)
}

/// A query typed while an older one is still being answered: the late answer of the old query is not
/// shown, the answer of the new one is.
#[gpui_kit::test]
async fn a_late_answer_of_an_old_query_is_not_shown(cx: &mut TestAppContext) {
    let (popup, cx) = files_popup(cx);
    let old = generation(&popup, cx);
    popup.update(cx, |popup, cx| {
        popup.field.update(cx, |field, cx| field.set_text("ledger", cx));
        popup.searched = "ledger".to_owned();
        popup.run(cx);
    });
    let new = generation(&popup, cx);
    assert_ne!(old, new, "the new query is a new request");

    let ticket = |generation| Ticket { section: SectionId::Files, generation };
    popup.update(cx, |popup, cx| popup.answered(ticket(old), found("src/Order.kt"), cx));
    let lines = popup.read_with(cx, |popup, _| popup.lines());
    assert!(!lines.contains(&"src/Order.kt".to_owned()), "the old query's answer is dropped: {lines:?}");
    popup.update(cx, |popup, cx| popup.answered(ticket(new), found("src/Ledger.kt"), cx));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["src/Ledger.kt"]);
}

/// A query with nothing in the project is searched for everywhere; a new query typed before that wider
/// answer comes is searched in the project again, and the late wide answer of the old query is not shown.
#[gpui_kit::test]
async fn a_late_wide_answer_of_an_old_query_is_not_shown_after_a_new_query(cx: &mut TestAppContext) {
    let (popup, cx) = files_popup(cx);
    let ticket = |generation| Ticket { section: SectionId::Files, generation };
    let first = generation(&popup, cx);
    popup.update(cx, |popup, cx| popup.answered(ticket(first), SeAnswer::Files(Vec::new()), cx));
    cx.run_until_parked();
    let scope = popup.read_with(cx, |popup, _| (popup.include_non_project(), popup.not_found().map(str::to_owned)));
    assert_eq!(scope, (true, Some("order".to_owned())), "nothing in the project: the popup searches everywhere");
    let wide = generation(&popup, cx);
    assert_ne!(first, wide, "the wider search is a new request");

    popup.update(cx, |popup, cx| {
        popup.field.update(cx, |field, cx| field.set_text("ledger", cx));
        popup.query_changed(cx);
    });
    cx.executor().advance_clock(DEBOUNCE);
    cx.run_until_parked();
    let narrowed = generation(&popup, cx);
    assert_ne!(wide, narrowed, "the new query is a new request");
    assert!(!popup.read_with(cx, |popup, _| popup.include_non_project()), "the new query is searched in the project");
    popup.update(cx, |popup, cx| popup.answered(ticket(wide), found("lib/Order.kt"), cx));
    let lines = popup.read_with(cx, |popup, _| popup.lines());
    assert!(!lines.contains(&"lib/Order.kt".to_owned()), "the old query's wide answer is dropped: {lines:?}");
    popup.update(cx, |popup, cx| popup.answered(ticket(narrowed), found("src/Ledger.kt"), cx));
    assert_eq!(popup.read_with(cx, |popup, _| popup.lines()), ["src/Ledger.kt"]);
}

/// The popup's width: 760 px, less in a window that leaves under 24 px on each side of it.
#[test]
fn the_popup_is_760_wide_and_leaves_24_px_beside_it_in_a_narrow_window() {
    assert!((parts::popup_width(1000.0) - 760.0).abs() < f32::EPSILON);
    assert!((parts::popup_width(720.0) - 672.0).abs() < f32::EPSILON);
    assert!((parts::popup_width(520.0) - 472.0).abs() < f32::EPSILON);
    assert!(parts::popup_width(20.0).abs() < f32::EPSILON);
    assert!(!parts::is_narrow(760.0) && parts::is_narrow(682.0) && parts::is_narrow(672.0));
}

/// Without a preview the list is as high as its rows: ten rows at least while a query is typed, 672 px at
/// most.
#[test]
fn the_list_is_as_high_as_its_rows_between_ten_rows_and_672_px() {
    assert!((parts::list_height(2, false) - 48.0).abs() < f32::EPSILON);
    assert!((parts::list_height(2, true) - 240.0).abs() < f32::EPSILON);
    assert!((parts::list_height(24, true) - 576.0).abs() < f32::EPSILON);
    assert!((parts::list_height(100, false) - 672.0).abs() < f32::EPSILON);
}
