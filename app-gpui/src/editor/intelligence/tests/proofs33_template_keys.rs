use super::*;
use crate::templates::live_store::LIVE_TEMPLATES_KEY;

/// A template of the `.mock` files whose abbreviation `mk` the server's items also start with.
const MK_TEMPLATE: &str = r#"{"templates":{"user.mk":{"id":"user.mk","group":"User","abbrev":"mk","body":"make($A$, $B$);$END$","contexts":["mock"]}},"deleted":[]}"#;

fn store_mk_template(cx: &mut VisualTestContext) {
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(LIVE_TEMPLATES_KEY, MK_TEMPLATE.to_owned()));
}

/// Opens the lookup on the word before the caret and waits for its rows.
fn complete(f: &Fixture, cx: &mut VisualTestContext) {
    f.run(cx, "code.completion");
    settle(cx, "the lookup's rows", |cx| !f.rows(cx).is_empty());
}

/// While the completion lookup is open, Tab and Enter are its own even after a template's
/// abbreviation: Tab puts the selected item in place of the word and Enter inserts it; Shift+Tab is
/// not the lookup's.
#[gpui_kit::test]
async fn tab_and_enter_go_to_the_open_lookup_before_a_templates_abbreviation(cx: &mut TestAppContext) {
    let answers = json!({ "textDocument/completion": [item("mkdir", 6), item("mktemp", 6)] });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", answers);
    store_mk_template(cx);
    cx.simulate_input("mk");
    complete(&f, cx);
    assert_eq!(f.rows(cx), ["mkdir", "mktemp"]);
    // Shift+Tab is not the lookup's: it reaches the text input as a tab character, which ends the word.
    cx.simulate_keystrokes("shift-tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "mk\t");
    assert!(f.rows(cx).is_empty(), "the lookup closed");
    cx.simulate_keystrokes("backspace");
    complete(&f, cx);
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "mkdir");
    assert!(f.rows(cx).is_empty(), "the lookup closed");

    cx.simulate_input(" mk");
    complete(&f, cx);
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "mkdir mktemp");
    assert!(f.rows(cx).is_empty());
    f.close(cx);
}

/// While a template is filled in, the lookup opened in a field takes Tab and Enter first, as it does
/// outside a template; the template goes on in the field the accepted item filled, and once the
/// lookup is closed Enter and Tab walk the fields again, the last one ending the template.
#[gpui_kit::test]
async fn in_a_template_the_lookup_takes_tab_and_enter_before_the_fields(cx: &mut TestAppContext) {
    let answers = json!({ "textDocument/completion": [item("mkdir", 6), item("mktemp", 6)] });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", answers);
    store_mk_template(cx);
    let editor = f.editor(cx).expect("an editor");
    let active = |cx: &mut VisualTestContext| {
        f.workbench.read_with(cx, |w, _| w.template_session(&editor).map(|session| session.active_field()))
    };
    cx.simulate_input("mk");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "make(, );");
    assert_eq!(active(cx), Some(0));

    cx.simulate_input("mk");
    complete(&f, cx);
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "make(mkdir, );", "Tab accepted the lookup's item");
    assert_eq!(active(cx), Some(0), "the template goes on in the first field");
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(active(cx), Some(1), "with the lookup closed Enter moves to the next field");
    cx.simulate_input("mk");
    complete(&f, cx);
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "make(mkdir, mktemp);", "Enter accepted the lookup's item");
    assert_eq!(active(cx), Some(1));
    // Escape closes an open lookup before it is the template's.
    cx.simulate_input(" ");
    complete(&f, cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(f.rows(cx).is_empty());
    assert_eq!(active(cx), Some(1), "the first Escape closed the lookup only");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(active(cx), None, "the last field ends the template");
    let end = "make(mkdir, mktemp );".encode_utf16().count() as u32;
    assert_eq!(editor.read_with(cx, |e, _| e.caret()), end);
    f.close(cx);
}

/// A server's snippet item accepted from the lookup is filled in like a template: its first
/// placeholder is selected, Tab moves to the next one, and the last ends it with the caret at the
/// snippet's end.
#[gpui_kit::test]
async fn tab_walks_the_placeholders_of_a_servers_snippet_item(cx: &mut TestAppContext) {
    let snippet = json!({ "label": "call", "kind": 15, "insertText": "call(${1:a}, ${2:b})$0", "insertTextFormat": 2 });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", json!({ "textDocument/completion": [snippet] }));
    let editor = f.editor(cx).expect("an editor");
    let selected = |cx: &mut VisualTestContext| editor.read_with(cx, |e, _| e.selected_text());
    cx.simulate_input("ca");
    complete(&f, cx);
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "call(a, b)");
    assert_eq!(selected(cx), "a");
    cx.simulate_input("x");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "call(x, b)");
    assert_eq!(selected(cx), "b", "Tab moved to the second placeholder");
    cx.simulate_keystrokes("tab");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "call(x, b)");
    assert_eq!(editor.read_with(cx, |e, _| e.caret()), "call(x, b)".encode_utf16().count() as u32);
    f.close(cx);
}

/// A field whose variable calls `complete()` opens the lookup as soon as the template puts the caret
/// in it, and again when Enter moves the caret to the next such field.
#[gpui_kit::test]
async fn a_field_that_calls_complete_opens_the_lookup_when_the_caret_reaches_it(cx: &mut TestAppContext) {
    let answers = json!({ "textDocument/completion": [item("mkdir", 6), item("mktemp", 6)] });
    let (f, cx) = open(cx, &[("a.mock", "")], "a.mock", answers);
    let template = r#"{"templates":{"user.uz":{"id":"user.uz","group":"User","abbrev":"uz","body":"use($A$, $B$);$END$","contexts":["mock"],"variables":[{"name":"A","expression":"complete()"},{"name":"B","expression":"complete()"}]}},"deleted":[]}"#;
    cx.update(|_, cx| cx.global_mut::<UiStorage>().set(LIVE_TEMPLATES_KEY, template.to_owned()));
    let editor = f.editor(cx).expect("an editor");
    let active = |cx: &mut VisualTestContext| {
        f.workbench.read_with(cx, |w, _| w.template_session(&editor).map(|session| session.active_field()))
    };
    cx.simulate_input("uz");
    cx.simulate_keystrokes("tab");
    settle(cx, "the lookup's rows in the first field", |cx| !f.rows(cx).is_empty());
    assert_eq!(f.text(cx), "use(, );");
    assert_eq!(active(cx), Some(0));
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(f.rows(cx).is_empty());
    cx.simulate_keystrokes("enter");
    settle(cx, "the lookup's rows in the second field", |cx| !f.rows(cx).is_empty());
    assert_eq!(active(cx), Some(1));
    cx.simulate_keystrokes("enter");
    cx.run_until_parked();
    assert_eq!(f.text(cx), "use(, mkdir);", "Enter accepted the lookup's item");
    f.close(cx);
}
