use super::*;
use crate::editor_commands::clipboard_history::{ClipboardEntry, MAX_CLIPBOARD_ENTRIES};

fn history(cx: &mut VisualTestContext) -> Vec<ClipboardEntry> {
    cx.update(|_, cx| {
        let history = cx.try_global::<EditorClipboard>();
        history.map(|history| history.0.entries().to_vec()).unwrap_or_default()
    })
}

fn entry(text: &str, linewise: bool) -> ClipboardEntry {
    ClipboardEntry { text: text.to_owned(), linewise }
}

/// Copies and cuts in an editor go into the clipboard history newest first: a selection as it is, the caret
/// line (nothing selected) as a line-wise copy; copying a text again moves it up instead of repeating it.
/// A line-wise copy pastes above the caret's line while it is the latest copy; text put on the clipboard
/// by something else pastes at the caret.
#[gpui_kit::test]
async fn copies_and_cuts_are_recorded_newest_first_and_the_latest_line_copy_pastes_above(cx: &mut TestAppContext) {
    let (o, cx) = open(cx, "alpha beta\ngamma\n").await;
    o.editor.update(cx, |e, cx| e.select(0, 5, cx));
    cx.simulate_keystrokes("cmd-c");
    o.editor.update(cx, |e, cx| e.select(13, 13, cx));
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(history(cx), [entry("gamma", true), entry("alpha", false)]);
    o.editor.update(cx, |e, cx| e.select(0, 5, cx));
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(history(cx), [entry("alpha", false), entry("gamma", true)]);
    o.editor.update(cx, |e, cx| e.select(6, 10, cx));
    cx.simulate_keystrokes("cmd-x");
    assert_eq!(text(&o, cx), "alpha \ngamma\n");
    assert_eq!(history(cx), [entry("beta", false), entry("alpha", false), entry("gamma", true)]);

    // The caret line, copied with nothing selected, goes above the line the caret is on.
    o.editor.update(cx, |e, cx| e.select(9, 9, cx));
    cx.simulate_keystrokes("cmd-c");
    o.editor.update(cx, |e, cx| e.select(2, 2, cx));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(text(&o, cx), "gamma\nalpha \ngamma\n");
    // Text copied elsewhere is not a line copy: it goes in at the caret.
    o.editor.update(cx, |e, cx| e.select(6, 6, cx));
    cx.write_to_clipboard(ClipboardItem::new_string("zeta ".to_owned()));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(text(&o, cx), "gamma\nzeta alpha \ngamma\n");
    o.host.close_project("w1");
}

/// The history keeps the latest [`MAX_CLIPBOARD_ENTRIES`] copies.
#[gpui_kit::test]
async fn the_clipboard_history_keeps_the_latest_thirty_copies(cx: &mut TestAppContext) {
    let words: Vec<String> = (0..MAX_CLIPBOARD_ENTRIES + 5).map(|n| format!("w{n}")).collect();
    let (o, cx) = open(cx, &words.join(" ")).await;
    let mut from = 0;
    for word in &words {
        let to = from + word.len() as u32;
        o.editor.update(cx, |e, cx| e.select(from, to, cx));
        cx.simulate_keystrokes("cmd-c");
        from = to + 1;
    }
    let kept: Vec<String> = history(cx).into_iter().map(|entry| entry.text).collect();
    let newest: Vec<String> = words.iter().rev().take(MAX_CLIPBOARD_ENTRIES).cloned().collect();
    assert_eq!(kept, newest);
    o.host.close_project("w1");
}
