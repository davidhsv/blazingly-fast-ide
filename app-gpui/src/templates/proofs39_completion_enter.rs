//! Enter in the fields being filled in: a live template's fields take it, a completion item's
//! snippet leaves it to the editor.

use super::editor::{FieldKey, TemplateAction};
use super::editor_context::{CommentTokens, TreeNode};
use super::expand::{EditorSnapshot, TabPress, TemplateEdit, TemplateSession, TemplateStores, tab_press};
use super::snippet::apply_snippet;
use ropey::Rope;

fn len(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

// template fields › leaves Enter to the editor in a completion's snippet: the fields of a snippet a
// completion item put in walk with Tab only, while a template's walk with Enter too.
#[test]
#[expect(clippy::single_range_in_vec_init, reason = "a caret is a selection of one range")]
fn leaves_enter_to_the_editor_in_a_completions_snippet() {
    // An LSP tag completion `<dependency>$1</dependency>$0` accepted on the typed `<dep`.
    let doc = Rope::from_str("<dep");
    let insertion = apply_snippet("<dependency>${1}</dependency>${0}", &doc, 4..4, 0, 4, "  ");
    assert_eq!(insertion.insert, "<dependency></dependency>");
    let caret = len("<dependency>");
    assert_eq!(insertion.selection, [caret..caret]);
    let session = TemplateSession::from_fields(insertion.active.clone().expect("a field comes before the end"));
    assert_eq!(session.step(FieldKey::Enter, false), None, "Enter is the editor's");
    let step = session.step(FieldKey::Tab, false).expect("Tab moves to the snippet's end");
    let end = len("<dependency></dependency>");
    assert_eq!(step.selections, [(end, end)]);
    assert_eq!(step.session, None, "the end of the snippet ends it");

    // A live template's fields take Enter, but for the open lookup's.
    let text = "fori";
    let fori = EditorSnapshot {
        doc: Rope::from_str(text),
        selections: vec![(len(text), len(text))],
        main: 0,
        path: "/p/src/main.ts".to_owned(),
        language: "typescript".to_owned(),
        ancestors: vec![TreeNode::new("Document", 0, len(text))],
        comment_tokens: CommentTokens { line: Some("//".to_owned()), block: None },
        project_root: "/p".to_owned(),
        indent_unit: "  ".to_owned(),
    };
    let expansion = match tab_press(&fori, None, false, false, false, &TemplateStores::default()) {
        TabPress::Template(action) => match *action {
            TemplateAction::Expand(expansion) => expansion,
            other => panic!("expected an expansion, got {other:?}"),
        },
        other => panic!("expected a template, got {other:?}"),
    };
    let edit = TemplateEdit::new(expansion.apply(&fori.template_editor(), None));
    let template = edit.session.expect("the template has several fields");
    assert!(template.step(FieldKey::Enter, false).is_some(), "Enter moves to a template's next field");
    assert_eq!(template.step(FieldKey::Enter, true), None, "the open lookup takes Enter");
}
