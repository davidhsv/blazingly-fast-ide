use super::*;

/// The reply's Copy Markdown copies the text as it is when clicked: a reply still streaming is
/// copied as far as it has come; once more text arrives the button no longer says Copied, and a
/// second click copies the whole reply.
#[gpui_kit::test]
async fn copies_the_latest_streamed_text_and_clears_feedback_when_content_changes(cx: &mut TestAppContext) {
    let (view, store, _, cx) = open(cx, vec![local_task("A")], vec![user("Question", false), agent("**Partial**")]);
    click(cx, "session-reply-1-copy");
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("**Partial**"));
    assert_eq!(buttons(&view, 1, cx).first().map(String::as_str), Some("Copied"));

    let whole = "**Partial** and the rest of the reply.";
    store.update(cx, |store, cx| {
        store.entries_loaded("A", None, vec![user("Question", false), agent(whole)]);
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(buttons(&view, 1, cx).first().map(String::as_str), Some("Copy Markdown"));
    click(cx, "session-reply-1-copy");
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(whole));
    assert_eq!(buttons(&view, 1, cx).first().map(String::as_str), Some("Copied"));
}
