use super::*;

/// A read-only editor takes no paste: ⌘V leaves it as it is; once writable again the same paste goes in.
#[gpui_kit::test]
async fn does_not_insert_into_a_read_only_editor(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    f.set(cx, "ab|c");
    f.editor.update(cx, |editor, cx| editor.set_read_only(true, cx));
    cx.write_to_clipboard(ClipboardItem::new_string("text".to_owned()));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(f.shown(cx), "ab|c");
    assert!(!f.editor.update(cx, |editor, cx| editor.paste(cx)), "the paste is refused");
    assert_eq!(f.shown(cx), "ab|c");
    f.editor.update(cx, |editor, cx| editor.set_read_only(false, cx));
    cx.simulate_keystrokes("cmd-v");
    assert_eq!(f.shown(cx), "abtext|c");
    close(cx);
}
