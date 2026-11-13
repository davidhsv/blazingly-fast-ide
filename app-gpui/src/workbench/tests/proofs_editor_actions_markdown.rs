use super::*;
use crate::features_b::viewers::markdown_view::MarkdownView;
use crate::features_b::viewers::preview::MarkdownMode;

/// Where `preview` shows source `line` at its top, from its layout.
fn top_of(preview: &Entity<MarkdownView>, line: u32, cx: &mut VisualTestContext) -> f64 {
    preview.read_with(cx, |preview, _| {
        let (blocks, _) = preview.layout();
        blocks.iter().find(|block| block.line == f64::from(line)).map_or(0.0, |block| block.top)
    })
}

fn scroll_top(preview: &Entity<MarkdownView>, cx: &mut VisualTestContext) -> f64 {
    preview.read_with(cx, |preview, _| preview.scroll_top())
}

/// A Markdown tab's editor drives its preview: side by side the preview follows the editor's
/// scrolling, and in Preview Only the hidden editor's caret sent to a far line scrolls the
/// preview there.
#[gpui_kit::test]
async fn a_markdown_preview_follows_the_editors_scrolling_and_the_hidden_editors_caret(cx: &mut TestAppContext) {
    let _home = start(cx);
    // Paragraph n is source line 2n; the document is many screens long.
    let text: String = (0..1000).map(|n| format!("Paragraph {n}\n\n")).collect();
    let (_folder, root, workbench, cx) = trusted_project(cx, &[("NOTES.md", text.as_str())]);
    let notes = root.join("NOTES.md");
    workbench.update_in(cx, |w, window, cx| w.open_project_file(notes.clone(), window, cx));
    settle(cx, |cx| workbench.read_with(cx, |w, _| w.editor().is_some() && w.active_markdown_host().is_some()));
    let editor = workbench.read_with(cx, |w, _| w.editor().cloned()).expect("a text editor");
    let host = workbench.read_with(cx, |w, _| w.active_markdown_host()).expect("the Markdown layouts");
    let preview = |cx: &mut VisualTestContext| host.read_with(cx, |host, _| host.preview().cloned());

    // Side by side, scrolling the editor scrolls the preview to the same source line.
    run_action(cx, "markdown.editorAndPreview", None);
    let shown = preview(cx).expect("the preview shows");
    editor.update(cx, |editor, cx| editor.scroll_to_line(40, cx));
    cx.run_until_parked();
    assert_eq!(editor.read_with(cx, |editor, cx| editor.top_line(cx)), 40);
    let target = top_of(&shown, 40, cx);
    assert!(target > 0.0);
    settle(cx, |cx| (scroll_top(&shown, cx) - target).abs() < 1.0);

    // Preview Only: the hidden editor's caret moved far down scrolls the preview to its line.
    run_action(cx, "markdown.previewOnly", None);
    assert_eq!(host.read_with(cx, |host, _| host.mode()), MarkdownMode::Preview);
    let shown = preview(cx).expect("the preview shows");
    editor.update(cx, |editor, cx| {
        let at = editor.offset_of(1600, 0);
        editor.select(at, at, cx);
    });
    cx.run_until_parked();
    let target = top_of(&shown, 1600, cx);
    assert!(target > 0.0, "line 1600 is far below the first screen");
    settle(cx, |cx| (scroll_top(&shown, cx) - target).abs() < 1.0);
    close_backend_project(cx);
}
