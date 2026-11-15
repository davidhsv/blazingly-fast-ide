use super::*;
use crate::editor::view::code_font_of;
use gpui_kit::Hsla;
use std::sync::Arc;

/// The look of a JavaScript editor whose code is drawn at 17 pixels on 22-pixel lines.
fn javascript_look(cx: &mut VisualTestContext) -> PreviewLook {
    let font = cx.update(|_, cx| code_font_of(&[], false, 400, cx));
    PreviewLook {
        language: ide_core::LanguageId::new("javascript"),
        font,
        font_size: px(17.0),
        line_height: px(22.0),
        syntax: Arc::new(ide_language::Syntax::new()),
    }
}

/// The colour the preview draws the first character of its line `line` in.
fn first_colour(dialog: &Entity<PasteHistoryDialog>, cx: &mut VisualTestContext, line: usize) -> Option<Hsla> {
    dialog.read_with(cx, |dialog, cx| dialog.preview_colours(cx).get(line)?.1.first().map(|(_, colour)| *colour))
}

/// Waits for the preview of the active row to be highlighted.
fn highlighted(dialog: &Entity<PasteHistoryDialog>, cx: &mut VisualTestContext, plain: Hsla) {
    for _ in 0..200 {
        cx.run_until_parked();
        if first_colour(dialog, cx, 0).is_some_and(|colour| colour != plain) {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("the preview was not highlighted");
}

/// The rows are the copies newest first; the preview draws the active row's whole text in the target
/// editor's font, size and line height and in the colours of its language's grammar, and follows the
/// active row.
#[gpui_kit::test]
async fn shows_newest_first_and_a_read_only_preview_with_the_target_grammar_and_editor_font(cx: &mut TestAppContext) {
    let (dialog, _, cx) = open(cx, vec![entry("const newest = 1;\n// detail", false), entry("older", false)]);
    assert_eq!(texts(&dialog, cx), ["const newest = 1;\n// detail", "older"]);
    let look = javascript_look(cx);
    let font = look.font.clone();
    dialog.update(cx, |dialog, cx| dialog.set_preview_look(look, cx));
    let (plain, keyword, comment) = cx.update(|_, cx| {
        (theme::color(cx, "fg-editor"), theme::color(cx, "syn-keyword"), theme::color(cx, "syn-comment"))
    });
    highlighted(&dialog, cx, plain);
    let lines = dialog.read_with(cx, |dialog, cx| dialog.preview_colours(cx));
    let texts_shown: Vec<&str> = lines.iter().map(|(text, _)| text.as_str()).collect();
    assert_eq!(texts_shown, ["const newest = 1;", "// detail"]);
    assert_eq!(lines[0].1.first(), Some(&(5, keyword)), "{lines:?}");
    assert_eq!(first_colour(&dialog, cx, 1), Some(comment));
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview_style()), Some((font, px(17.0), px(22.0))));
    cx.simulate_keystrokes("down");
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview().to_owned()), "older");
    let lines = dialog.read_with(cx, |dialog, cx| dialog.preview_colours(cx));
    assert_eq!(lines.iter().map(|(text, _)| text.as_str()).collect::<Vec<_>>(), ["older"]);
}

/// Without the target's look the preview is the active row's text in plain lines.
#[gpui_kit::test]
async fn the_preview_is_plain_text_until_the_target_editors_look_is_known(cx: &mut TestAppContext) {
    let (dialog, _, cx) = open(cx, vec![entry("const a = 1;", false)]);
    let plain = cx.update(|_, cx| theme::color(cx, "fg-editor"));
    assert_eq!(dialog.read_with(cx, |dialog, _| dialog.preview_style()), None);
    let lines = dialog.read_with(cx, |dialog, cx| dialog.preview_colours(cx));
    assert_eq!(lines, [("const a = 1;".to_owned(), vec![(12, plain)])]);
}
