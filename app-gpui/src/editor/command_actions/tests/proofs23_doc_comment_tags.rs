use super::*;
use crate::platform::settings::{SettingsWrite, set_global};
use crate::theme::tokens::ThemeAttributes;
use gpui_kit::Hsla;

/// The colour the last frame drew the first character of `needle` in (`needle` on a painted line).
fn colour_of(f: &Fixture, cx: &mut VisualTestContext, needle: &str) -> Option<Hsla> {
    cx.run_until_parked();
    f.editor.read_with(cx, |editor, _| {
        let at = editor.text().find(needle).expect("the text is in the document");
        let offset = utf16(&editor.text()[..at]);
        let (line, col) = crate::editor::buffer::line_col(editor.rope(), offset);
        editor.painted_color_at(line, col)
    })
}

fn token(cx: &mut VisualTestContext, name: &str) -> Hsla {
    cx.update(|_, cx| theme::color(cx, name))
}

fn dark(cx: &mut VisualTestContext) {
    cx.update(|_, cx| theme::set_theme(&ThemeAttributes::dark(), cx));
    cx.run_until_parked();
}

/// A JavaDoc comment draws its block tag, the name `@param` takes and its HTML markup in the colours of
/// the doc comment tags, and the rest of the comment in the colour its opening is drawn in.
#[gpui_kit::test]
async fn styles_the_tags_with_the_highlight_styles_classes_inside_the_comments_own(cx: &mut TestAppContext) {
    let (f, cx) =
        open(cx, "A.java", "package a;\n\n/**\n * See <b>this</b>.\n * @param id the id\n */\nclass A {}\n").await;
    f.highlighted(cx);
    dark(cx);
    // The comment's own colour, as its opening is drawn.
    let comment = colour_of(&f, cx, "/**").expect("the comment is painted");
    assert_eq!(colour_of(&f, cx, "See"), Some(comment));
    assert_eq!(colour_of(&f, cx, "@param"), Some(token(cx, "syn-doc-comment-tag")));
    assert_eq!(colour_of(&f, cx, "id the"), Some(token(cx, "syn-doc-comment-tag-value")));
    assert_eq!(colour_of(&f, cx, "<b>"), Some(token(cx, "syn-doc-markup")));
    assert_eq!(colour_of(&f, cx, "the id"), Some(comment));
    assert_ne!(token(cx, "syn-doc-comment-tag"), comment, "the dark theme gives tags their own colour");
    close(cx);
}

/// A new theme draws the tags in its colours; an edit that turns the doc comment into a plain block
/// comment takes the tag colour away once the text is highlighted again.
#[gpui_kit::test]
async fn rebuilds_decorations_when_the_highlighter_is_reconfigured(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "A.java", "/** @return result */\nclass A {}\n").await;
    f.highlighted(cx);
    let light = colour_of(&f, cx, "@return");
    assert_eq!(light, Some(token(cx, "syn-doc-comment-tag")));
    dark(cx);
    assert_eq!(colour_of(&f, cx, "@return"), Some(token(cx, "syn-doc-comment-tag")));
    assert_ne!(colour_of(&f, cx, "@return"), light);
    f.editor.update(cx, |editor, cx| assert!(editor.apply_edit(&[change(2, 3, "")], vec![(2, 2)], 0, cx)));
    f.highlighted(cx);
    settle(cx, |cx| colour_of(&f, cx, "@return") == Some(token(cx, "syn-comment")));
    close(cx);
}

/// A file type override that makes the Java file plain text takes its tags away.
#[gpui_kit::test]
async fn removes_decorations_when_switching_to_an_unsupported_language(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "A.java", "/** @param x the x */\nclass A {}\n").await;
    f.highlighted(cx);
    dark(cx);
    assert_eq!(colour_of(&f, cx, "@param"), Some(token(cx, "syn-doc-comment-tag")));
    let path = f.editor.read_with(cx, |editor, _| editor.path().to_string_lossy().into_owned());
    let overrides = serde_json::Map::from_iter([(path, serde_json::json!("plaintext"))]);
    let write = [SettingsWrite { key: "files.fileTypeOverrides", value: serde_json::Value::Object(overrides) }];
    cx.update(|_, cx| set_global("w1", &write, cx)).expect("the override is stored");
    settle(cx, |cx| f.editor.read_with(cx, |editor, _| editor.language().as_str() == "plaintext"));
    // The tags go with the language at once; the comment's colour goes when the plain text's (empty)
    // highlights arrive.
    settle(cx, |cx| colour_of(&f, cx, "@param") == Some(token(cx, "fg-editor")));
    assert_eq!(colour_of(&f, cx, "@param"), Some(token(cx, "fg-editor")));
    close(cx);
}

/// Other languages draw no tags: a Rust doc comment stays in the comment's colours.
#[gpui_kit::test]
async fn gives_other_languages_nothing(cx: &mut TestAppContext) {
    let (f, cx) = open(cx, "main.rs", "/** @param x */\nfn f() {}\n").await;
    f.highlighted(cx);
    dark(cx);
    let tag = token(cx, "syn-doc-comment-tag");
    assert_ne!(colour_of(&f, cx, "@param"), Some(tag));
    close(cx);
}

/// Scrolled deep into a long doc comment whose `/**` is far above the view, the tags of the painted lines
/// are still drawn.
#[gpui_kit::test]
async fn keeps_tags_deep_inside_a_long_doc_comment(cx: &mut TestAppContext) {
    let body: String = (0..400).map(|n| format!(" * @param item{n} description\n")).collect();
    let (f, cx) = open(cx, "A.java", &format!("package a;\n\n/**\n{body} */\nclass A {{}}\n")).await;
    f.highlighted(cx);
    dark(cx);
    f.editor.update(cx, |editor, cx| editor.scroll_to_line(300, cx));
    cx.run_until_parked();
    let painted = f.editor.read_with(cx, |editor, _| editor.painted_lines().expect("painted"));
    assert!(painted.start >= 300, "{painted:?}");
    assert_eq!(colour_of(&f, cx, "@param item300 "), Some(token(cx, "syn-doc-comment-tag")));
    assert_eq!(colour_of(&f, cx, "item300 "), Some(token(cx, "syn-doc-comment-tag-value")));
    close(cx);
}
