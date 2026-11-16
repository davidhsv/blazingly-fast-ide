use super::*;
use crate::platform::settings::{SettingsWrite, set_global};

/// A file of each language whose plain string literals copy and paste escaped, and a line of it with
/// `|` where the caret goes, inside an empty string literal. Kotlin is not among them: its highlights
/// carry no string spans, so the editor finds no literal to escape for.
const MODES: [(&str, &str); 8] = [
    ("A.java", "class A { String value = \"|\"; }"),
    ("a.js", "value = \"|\";"),
    ("a.ts", "let value = \"|\";"),
    ("a.py", "value = \"|\""),
    ("a.rs", "fn f() { let value = \"|\"; }"),
    ("a.go", "package a\n\nvar value = \"|\""),
    ("a.c", "char *value = \"|\";"),
    ("a.cpp", "const char *value = \"|\";"),
];

/// Opens `name`, written into the fixture's folder, in the fixture's window and answers its editor
/// once it has the keyboard.
fn open_file(f: &Fixture, cx: &mut VisualTestContext, name: &str) -> Entity<Editor> {
    let file = f._folders.1.path().canonicalize().expect("the folder exists").join(name);
    std::fs::write(&file, "").expect("the file is written");
    let workbench = cx.update(|window, _| window.root::<Workbench>().flatten()).expect("the window's workbench");
    workbench.update_in(cx, |workbench, window, cx| workbench.open_file(file.clone(), window, cx));
    settle(cx, |cx| {
        cx.update(|window, cx| {
            workbench.read(cx).editor().is_some_and(|editor| {
                editor.read(cx).path() == file.as_path() && editor.focus_handle(cx).is_focused(window)
            })
        })
    });
    workbench.read_with(cx, |workbench, _| workbench.editor().cloned()).expect("an editor")
}

/// Makes the text of `editor` the text of `marked` with its carets and selections, and waits for the
/// highlights of that text, which tell its string literals.
fn put(editor: &Entity<Editor>, cx: &mut VisualTestContext, marked: &str) {
    let wanted = state(marked);
    editor.update(cx, |editor, cx| {
        let len = utf16(&editor.text());
        if editor.text() != wanted.text() {
            assert!(editor.apply_edit(&[change(0, len, wanted.text())], Vec::new(), 0, cx), "{marked}");
        }
        editor.set_selections(wanted.selections.clone(), wanted.main, cx);
    });
    settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.literal_tree().is_empty()));
}

fn text_of(editor: &Entity<Editor>, cx: &mut VisualTestContext) -> String {
    editor.read_with(cx, |editor, _| editor.text())
}

fn clipboard(cx: &mut VisualTestContext) -> Option<String> {
    cx.read_from_clipboard().and_then(|item| item.text())
}

fn paste(cx: &mut VisualTestContext, text: &str) {
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
    cx.simulate_keystrokes("cmd-v");
}

/// `marked` with its caret marks removed: the text it stands for.
fn plain(marked: &str) -> String {
    state(marked).text()
}

/// In every language with plain string literals, ⌘V into one escapes the quotes, backslashes and line
/// breaks of the pasted text.
#[gpui_kit::test]
async fn escapes_ordinary_pastes_into_a_string(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    for (name, marked) in MODES {
        let editor = open_file(&f, cx, name);
        put(&editor, cx, marked);
        paste(cx, "say \"hi\"\\path\nnext");
        let expected = plain(marked).replace("\"\"", "\"say \\\"hi\\\"\\\\path\\nnext\"");
        assert_eq!(text_of(&editor, cx), expected, "{name}");
    }
    close(cx);
}

/// In every language with plain string literals, ⌘C of a selection inside one puts the text it stands
/// for on the clipboard.
#[gpui_kit::test]
async fn unescapes_copied_string_contents(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    for (name, marked) in MODES {
        let editor = open_file(&f, cx, name);
        put(&editor, cx, &marked.replace('|', "[say \\\"hi\\\"\\\\path\\nnext\\t!]"));
        cx.simulate_keystrokes("cmd-c");
        assert_eq!(clipboard(cx).as_deref(), Some("say \"hi\"\\path\nnext\t!"), "{name}");
    }
    close(cx);
}

/// Control characters, line and paragraph separators and characters outside the BMP pasted into a
/// string stay on its line and copy back as they were pasted.
#[gpui_kit::test]
async fn round_trips_pasted_control_characters_and_unicode(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "value = \"|\"");
    let pasted = "😀\0\u{1}\u{8}\t\n\u{c}\u{b}\u{2028}\u{2029}$";
    paste(cx, pasted);
    let text = text_of(&editor, cx);
    assert_eq!(text.lines().count(), 1, "{text:?}");
    settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.literal_tree().is_empty()));
    let end = utf16(&text) - 1;
    editor.update(cx, |editor, cx| editor.select(9, end, cx));
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some(pasted));
    close(cx);
}

/// In JavaScript, TypeScript and Python a single-quoted string escapes its own quote only.
#[gpui_kit::test]
async fn escapes_only_the_single_quoted_strings_delimiter(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    for name in ["a.js", "a.ts", "a.py"] {
        let editor = open_file(&f, cx, name);
        put(&editor, cx, "value = '|'");
        paste(cx, "say 'hi' and \"bye\"");
        assert_eq!(text_of(&editor, cx), "value = 'say \\'hi\\' and \"bye\"'", "{name}");
    }
    close(cx);
}

/// Paste as Plain Text and typing put the text into a string as it is.
#[gpui_kit::test]
async fn leaves_paste_as_plain_text_and_typing_unchanged(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "value = \"|\"");
    cx.write_to_clipboard(ClipboardItem::new_string("\"\\\n".to_owned()));
    run(cx, "code.pasteAsPlainText");
    assert_eq!(text_of(&editor, cx), "value = \"\"\\\n\"");
    put(&editor, cx, "value = \"|\"");
    cx.simulate_input("\\n");
    assert_eq!(text_of(&editor, cx), "value = \"\\n\"");
    close(cx);
}

/// Smart Keys › Escape text on paste off: copy and paste leave the text as it is.
#[gpui_kit::test]
async fn disables_both_escaping_and_unescaping_with_the_setting(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "A.java");
    assert!(editor.read_with(cx, |editor, _| editor.facts().escape_on_paste), "on by default");
    let write = [
        SettingsWrite { key: "editor.smartKeys.escapeOnPaste", value: serde_json::json!(false) },
        SettingsWrite { key: "editor.smartKeys.reformatOnPaste", value: serde_json::json!("none") },
    ];
    cx.update(|_, cx| set_global("w1", &write, cx)).expect("the settings are stored");
    settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.facts().escape_on_paste));
    put(&editor, cx, "class A { String value = \"[a\\nb]\"; }");
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some("a\\nb"));
    paste(cx, "\"\\\n");
    assert_eq!(text_of(&editor, cx), "class A { String value = \"\"\\\n\"; }");
    close(cx);
}

/// With one caret outside a string literal nothing is escaped.
#[gpui_kit::test]
async fn requires_every_caret_to_be_inside_a_plain_string(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "value = \"|\"; other = |");
    paste(cx, "\"");
    assert_eq!(text_of(&editor, cx), "value = \"\"\"; other = \"");
    close(cx);
}

/// Each caret's line of a multi-line paste is escaped for the literal of that caret, and every caret
/// ends before its literal's closing quote.
#[gpui_kit::test]
async fn escapes_per_caret_inserts_with_each_literals_delimiter_and_maps_all_carets(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "a = \"|\"; b = '[old]'; c = \"|\";");
    paste(cx, "one\"\ntwo'\nthree\\");
    let text = text_of(&editor, cx);
    assert_eq!(text, "a = \"one\\\"\"; b = 'two\\''; c = \"three\\\\\";");
    let (selections, rope) = editor.read_with(cx, |editor, _| (editor.selections().to_vec(), editor.rope().clone()));
    for (anchor, head) in selections {
        assert_eq!(anchor, head);
        let next = crate::editor::buffer::slice(&rope, head, head + 1);
        assert!(next == "\"" || next == "'", "{text:?} at {head}");
    }
    close(cx);
}

/// Several selections inside string literals copy as their decoded texts joined by line breaks; once a
/// selection leaves its literal the copy is the source text.
#[gpui_kit::test]
async fn copies_multiple_string_selections_as_decoded_fragments(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "a = \"[one\\n]\"; b = '[two\\t]'");
    cx.simulate_keystrokes("cmd-c");
    assert_eq!(clipboard(cx).as_deref(), Some("one\n\ntwo\t"));
    let len = utf16(&text_of(&editor, cx));
    editor.update(cx, |editor, cx| editor.set_selections(vec![(5, 10), (18, len)], 0, cx));
    cx.simulate_keystrokes("cmd-c");
    let text = text_of(&editor, cx);
    let expected = format!("{}\n{}", &text[5..10], &text[18..]);
    assert_eq!(clipboard(cx), Some(expected));
    close(cx);
}

/// A paste over a part of a string's text is escaped, leaves the caret after it, and is one undo step.
#[gpui_kit::test]
async fn replaces_a_substring_and_preserves_the_caret_undo_and_redo(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "value = \"before [old] after\"");
    paste(cx, "😀\"\nnext");
    let text = text_of(&editor, cx);
    assert_eq!(text, "value = \"before 😀\\\"\\nnext after\"");
    let after = utf16(&text[..text.find(" after").expect("the rest of the string")]);
    assert_eq!(editor.read_with(cx, |editor, _| editor.caret()), after);
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(text_of(&editor, cx), "value = \"before old after\"");
    cx.simulate_keystrokes("cmd-shift-z");
    assert_eq!(text_of(&editor, cx), "value = \"before 😀\\\"\\nnext after\"");
    close(cx);
}

/// ⌘V, ⌘C and ⌘X agree: the paste is escaped, the copy and the cut put the unescaped text on the
/// clipboard, and the cut empties the string.
#[gpui_kit::test]
async fn handles_native_paste_copy_cut_and_rich_copy_consistently(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "a.js");
    put(&editor, cx, "value = \"|\"");
    paste(cx, "say \"hi\"\nnext");
    let text = text_of(&editor, cx);
    assert_eq!(text, "value = \"say \\\"hi\\\"\\nnext\"");
    for keys in ["cmd-c", "cmd-x"] {
        settle(cx, |cx| editor.read_with(cx, |editor, _| !editor.literal_tree().is_empty()));
        editor.update(cx, |editor, cx| editor.select(9, utf16(&text) - 1, cx));
        cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
        cx.simulate_keystrokes(keys);
        assert_eq!(clipboard(cx).as_deref(), Some("say \"hi\"\nnext"), "{keys}");
    }
    assert_eq!(text_of(&editor, cx), "value = \"\"");
    close(cx);
}

/// An entry chosen in Paste from History is escaped for the string literal it goes into, as ⌘V is.
#[gpui_kit::test]
async fn escapes_an_entry_pasted_from_the_history_into_a_string(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "notes.txt", "").await;
    let editor = open_file(&f, cx, "A.java");
    put(&editor, cx, "class A { String value = \"|\"; }");
    cx.update(|_, cx| cx.default_global::<crate::editor::view::EditorClipboard>().0.record("say \"hi\"", false));
    run(cx, "code.pasteFromHistory");
    let workbench = cx.update(|window, _| window.root::<Workbench>().flatten()).expect("the window's workbench");
    assert!(workbench.read_with(cx, |w, _| w.paste_history_dialog().is_some()), "the dialog is open");
    run(cx, "editor.pasteHistory.paste");
    assert_eq!(text_of(&editor, cx), "class A { String value = \"say \\\"hi\\\"\"; }");
    close(cx);
}
