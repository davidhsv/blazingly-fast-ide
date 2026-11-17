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

/// The popup's width: 760 px, less in a window that leaves under 24 px on each side of it.
#[test]
fn the_popup_is_760_wide_and_leaves_24_px_beside_it_in_a_narrow_window() {
    assert!((parts::popup_width(1000.0) - 760.0).abs() < f32::EPSILON);
    assert!((parts::popup_width(720.0) - 672.0).abs() < f32::EPSILON);
    assert!((parts::popup_width(520.0) - 472.0).abs() < f32::EPSILON);
    assert!(parts::popup_width(20.0).abs() < f32::EPSILON);
}
