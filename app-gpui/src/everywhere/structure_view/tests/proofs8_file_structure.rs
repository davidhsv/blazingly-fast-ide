use super::*;

/// Typing narrows the tree to the matches and what holds them; unticking Narrow down on typing with
/// the filter still typed brings every symbol back, the matches then only highlighted.
#[gpui_kit::test]
async fn narrows_down_on_typing_keeping_ancestors_or_only_highlights_when_that_is_off(cx: &mut TestAppContext) {
    let (popup, _, cx) = open(cx, outline(), None);
    assert!(popup.read_with(cx, |popup, _| popup.options().narrow_down));
    cx.simulate_input("creat");
    assert_eq!(lines(&popup, cx), ["OrderController", "  create"]);
    click(cx, "file-structure-narrow", 1);
    cx.run_until_parked();
    assert!(!popup.read_with(cx, |popup, _| popup.options().narrow_down));
    assert_eq!(lines(&popup, cx), ["OrderController", "  list", "  create", "  list", "helper"]);
    assert_eq!(popup.read_with(cx, |popup, _| popup.matched()), ["create"]);
}
