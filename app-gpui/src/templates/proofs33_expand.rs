//! What a key press does with templates, on the models: the stores read from the UI storage's
//! text, the literal at the caret from the highlights, and a template filled in field by field.

use super::editor::TemplateAction;
use super::editor_context::{CommentTokens, TreeNode};
use super::expand::*;
use super::live_store::{LIVE_TEMPLATE_OPTIONS_KEY, LIVE_TEMPLATES_KEY};
use super::postfix_store::POSTFIX_KEY;
use super::snippet::testing::replace;
use crate::editor::buffer;
use crate::editor::view::HighlightSpan;
use ide_language::SyntaxHighlightKind;
use ropey::Rope;
use std::collections::HashMap;

/// An editor of `language` on `text` with its caret at the end.
fn snapshot(language: &str, text: &str) -> EditorSnapshot {
    let doc = Rope::from_str(text);
    let end = buffer::len_utf16(&doc);
    EditorSnapshot {
        doc,
        selections: vec![(end, end)],
        main: 0,
        path: "/p/src/main.ts".to_owned(),
        language: language.to_owned(),
        ancestors: vec![TreeNode::new("Document", 0, end)],
        comment_tokens: CommentTokens { line: Some("//".to_owned()), block: None },
        project_root: "/home/ann/p".to_owned(),
        indent_unit: "  ".to_owned(),
    }
}

/// Stores that read `stored` as the UI storage.
fn stores(stored: &[(&str, &str)]) -> TemplateStores {
    let map: HashMap<&str, &str> = stored.iter().copied().collect();
    let mut stores = TemplateStores::default();
    stores.sync(|key| map.get(key).copied());
    stores
}

/// The edit Tab makes in `snapshot`, which must expand a template.
fn tab_edit(snapshot: &EditorSnapshot, stores: &TemplateStores) -> TemplateEdit {
    match tab_press(snapshot, None, false, false, false, stores) {
        TabPress::Template(action) => match *action {
            TemplateAction::Expand(expansion) => TemplateEdit::new(expansion.apply(&snapshot.template_editor(), None)),
            other => panic!("expected an expansion, got {other:?}"),
        },
        other => panic!("expected a template, got {other:?}"),
    }
}

/// `snapshot` after `edit`, its selection the edit's.
fn applied(snapshot: &EditorSnapshot, edit: &TemplateEdit) -> EditorSnapshot {
    let mut doc = snapshot.doc.clone();
    for change in edit.changes.iter().rev() {
        replace(&mut doc, change.from, change.to, &change.insert);
    }
    EditorSnapshot { doc, selections: edit.selections.clone(), main: 0, ..snapshot.clone() }
}

fn text_of(snapshot: &EditorSnapshot, (from, to): (u32, u32)) -> String {
    buffer::slice(&snapshot.doc, from.min(to), from.max(to))
}

// Tab on `fori` expands it with its first field selected; Tab and Shift+Tab walk the fields, and the
// last one ends the template.
#[test]
fn tab_expands_an_abbreviation_and_walks_its_fields_until_the_last() {
    let stores = TemplateStores::default();
    let before = snapshot("typescript", "fori");
    let edit = tab_edit(&before, &stores);
    let after = applied(&before, &edit);
    assert_eq!(after.doc.to_string(), "for (let i = 0; i < .length; i++) {\n  \n}");
    // The linked occurrences of `i` are all selected.
    assert_eq!(edit.selections.len(), 3);
    assert!(edit.selections.iter().all(|&range| text_of(&after, range) == "i"));
    let session = edit.session.clone().expect("the template has several fields");
    assert_eq!(session.active_field(), 0);
    assert!(matches!(tab_press(&after, Some(&session), true, false, false, &stores), TabPress::Editor));

    let TabPress::Field(second) = tab_press(&after, Some(&session), false, false, false, &stores) else {
        panic!("Tab moves to the next field")
    };
    let caret = "for (let i = 0; i < ".len() as u32;
    assert_eq!(second.selections, [(caret, caret)]);
    let second_session = second.session.clone().expect("the end is still ahead");
    assert_eq!(second_session.active_field(), 1);
    let TabPress::Field(back) = tab_press(&after, Some(&second_session), true, false, false, &stores) else {
        panic!("Shift+Tab moves back")
    };
    assert_eq!(back.selections.len(), 3);

    let TabPress::Field(last) = tab_press(&after, Some(&second_session), false, false, false, &stores) else {
        panic!("Tab moves to the end")
    };
    let end = "for (let i = 0; i < .length; i++) {\n  ".len() as u32;
    assert_eq!(last.selections, [(end, end)]);
    assert_eq!(last.session, None, "the last field ends the template");
    // While the lookup is open Tab and Enter are its own.
    assert_eq!(second_session.step(super::editor::FieldKey::Enter, true), None);
}

// What the Settings pages store is what the next key press uses: a template added, an option turned
// off, a postfix template disabled; a key whose text did not change is not read again.
#[test]
fn the_stores_follow_what_the_ui_storage_holds_under_their_keys() {
    let added = r#"{"templates":{"user.zz":{"id":"user.zz","group":"User","abbrev":"zz","body":"zebra($END$)","contexts":["typescript"]}},"deleted":[]}"#;
    let with_template = stores(&[(LIVE_TEMPLATES_KEY, added)]);
    assert!(with_template.template("user.zz").is_some());
    let edit = tab_edit(&snapshot("typescript", "zz"), &with_template);
    assert_eq!(applied(&snapshot("typescript", "zz"), &edit).doc.to_string(), "zebra()");

    let no_tab = stores(&[(LIVE_TEMPLATE_OPTIONS_KEY, r#"{"expandWithTab":false,"showInCompletion":true}"#)]);
    assert!(!no_tab.options().expand_with_tab);
    assert!(matches!(tab_press(&snapshot("typescript", "log"), None, false, false, false, &no_tab), TabPress::Editor));

    let postfix_off = stores(&[(POSTFIX_KEY, r#"{"enabled":true,"expandWithTab":true,"disabled":["ts.not"]}"#)]);
    assert_eq!(postfix_off.postfix().disabled, ["ts.not"]);
    assert!(matches!(
        tab_press(&snapshot("typescript", "x.not"), None, false, false, false, &postfix_off),
        TabPress::Editor
    ));
    let postfix_on = TemplateStores::default();
    let edit = tab_edit(&snapshot("typescript", "x.not"), &postfix_on);
    assert_eq!(applied(&snapshot("typescript", "x.not"), &edit).doc.to_string(), "!x");

    // Removing the key brings the defaults back.
    let mut changing = stores(&[(LIVE_TEMPLATE_OPTIONS_KEY, r#"{"expandWithTab":false}"#)]);
    changing.sync(|_| None);
    assert!(changing.options().expand_with_tab);
    // A read-only editor and an open lookup keep Tab.
    assert!(matches!(
        tab_press(&snapshot("typescript", "log"), None, false, true, false, &postfix_on),
        TabPress::Editor
    ));
    assert!(matches!(
        tab_press(&snapshot("typescript", "log"), None, false, false, true, &postfix_on),
        TabPress::Editor
    ));
}

// The literal at the caret is the run of touching highlights of its kind: a string split by an
// escape is one string, a comment ends where its highlight does, code is in neither.
#[test]
fn the_comment_or_string_at_the_caret_comes_from_the_highlights_around_it() {
    use SyntaxHighlightKind as K;
    let span = |from: u32, to: u32, kind: K| HighlightSpan { from, to, kind };
    let spans = [span(0, 3, K::Keyword), span(4, 7, K::String), span(7, 9, K::Escape), span(9, 12, K::String)];
    let names = |pos: u32| -> Vec<(String, u32, u32)> {
        literal_ancestors(&spans, pos, 20).into_iter().map(|node| (node.kind, node.from, node.to)).collect()
    };
    assert_eq!(names(8), [("String".to_owned(), 4, 12), ("Document".to_owned(), 0, 20)]);
    assert_eq!(names(2), [("Document".to_owned(), 0, 20)]);
    assert_eq!(names(15), [("Document".to_owned(), 0, 20)]);
    let comment = [span(0, 6, K::Comment)];
    let at_end = literal_ancestors(&comment, 6, 6);
    assert_eq!(at_end[0].kind, "Comment");
    assert!(super::editor_context::in_comment_or_string(&at_end, 6));
    assert!(literal_ancestors(&[], 0, 0).iter().all(|node| node.kind == "Document"));
}

// The expansion's own edit, reported after the template started, leaves the fields where they are;
// typing in a field moves the ones after it; the caret leaving the field ends the template.
#[test]
fn a_template_follows_typing_in_its_field_and_ends_when_the_caret_leaves_it() {
    let stores = TemplateStores::default();
    let before = snapshot("typescript", "fori");
    let edit = tab_edit(&before, &stores);
    let session = edit.session.clone().expect("several fields");
    let selection: Vec<std::ops::Range<u32>> = edit.selections.iter().map(|&(a, h)| a..h).collect();
    let own = session.edited(&edit.changes, &selection).expect("the expansion's report keeps it");
    assert_eq!(own.field_ranges(), session.field_ranges());

    // `j` typed over each selected `i`: every occurrence follows.
    let typed: Vec<ide_core::TextChange> =
        edit.selections.iter().map(|&(from, to)| ide_core::TextChange { from, to, insert: "j".to_owned() }).collect();
    let carets: Vec<std::ops::Range<u32>> = selection.iter().map(|range| range.end..range.end).collect();
    let followed = own.edited(&typed, &carets).expect("typing stays in the field");
    assert_eq!(followed.field_ranges(), own.field_ranges());
    assert!(followed.selection_moved(std::slice::from_ref(&(0..0))).is_none(), "the caret left the field");
}

// Escape in a template first drops the field's selection, then leaves the template; outside one it is
// the editor's.
#[test]
fn escape_drops_the_selection_then_leaves_the_template() {
    let stores = TemplateStores::default();
    let edit = tab_edit(&snapshot("typescript", "fori"), &stores);
    let session = edit.session.clone().expect("several fields");
    let heads: Vec<(u32, u32)> = edit.selections.iter().map(|&(_, head)| (head, head)).collect();
    assert_eq!(escape_press(&edit.selections, Some(&session), false), EscapePress::DropSelection(heads.clone()));
    assert_eq!(escape_press(&heads, Some(&session), false), EscapePress::EndSession);
    assert_eq!(escape_press(&heads, None, false), EscapePress::Editor);
    assert_eq!(escape_press(&heads, Some(&session), true), EscapePress::Editor);
}

// Insert Live Template lists the templates that apply at the caret for the word before it, an exact
// abbreviation first; after a dot there is no word, and the caret is where the template goes.
#[test]
fn insert_live_template_lists_the_templates_for_the_word_before_the_caret() {
    let stores = TemplateStores::default();
    let typed = snapshot("typescript", "function f() {\n  lo");
    let rows = insert_live_template(&typed, &stores);
    assert_eq!((rows.from, rows.typed.as_str()), ("function f() {\n  ".len() as u32, "lo"));
    assert!(rows.rows.iter().any(|row| row.abbrev == "log" && row.boost == -5 && row.description == "console.log"));
    let exact = insert_live_template(&snapshot("typescript", "log"), &stores);
    assert!(exact.rows.iter().any(|row| row.abbrev == "log" && row.boost == 5));
    let after_dot = insert_live_template(&snapshot("typescript", "x."), &stores);
    assert_eq!((after_dot.from, after_dot.typed.as_str()), (2, ""));
    assert!(lookup_keeps_rows("fo-r!") && !lookup_keeps_rows("a.b"));
}
