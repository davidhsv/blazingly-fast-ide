//! The preview of Paste from History: the active row's text drawn as the target editor draws code,
//! in its font, size and line height and in the colours of its language's grammar. The text is
//! highlighted off the window's thread whenever the active row changes.

use super::PasteHistoryDialog;
use crate::editor::buffer::byte_of_col;
use crate::editor::view::{Editor, HighlightSpan, syntax_token};
use crate::theme;
use gpui_kit::{
    AnyElement, App, Context, Font, Hsla, IntoElement, Pixels, SharedString, StyledText, TextRun, div, prelude::*,
};
use ide_core::LanguageId;
use ide_language::Syntax;
use std::sync::Arc;

/// How many lines the preview shows at most: it is a fixed height, so more would be cut anyway.
const MAX_LINES: usize = 40;

/// How the target editor draws code: what the preview draws with.
#[derive(Clone)]
pub struct PreviewLook {
    pub language: LanguageId,
    pub font: Font,
    pub font_size: Pixels,
    pub line_height: Pixels,
    /// The grammars the text is highlighted with.
    pub syntax: Arc<Syntax>,
}

impl PreviewLook {
    /// The look of `editor`'s code, highlighted with the backend's grammars.
    pub fn of(editor: &Editor, cx: &App) -> Self {
        let (font, font_size, line_height) = editor.text_style(cx);
        let syntax = cx.global::<crate::backend::Backend>().host.syntax();
        Self { language: editor.language().clone(), font, font_size, line_height, syntax }
    }
}

/// The highlights of the text the preview shows, and the text being highlighted now.
#[derive(Default)]
pub(super) struct PreviewHighlights {
    text: String,
    spans: Vec<HighlightSpan>,
    asked: Option<String>,
}

/// The highlight spans of `text` in `language`; none without a grammar for it.
fn highlight(syntax: &Syntax, language: &LanguageId, text: &str) -> Vec<HighlightSpan> {
    let Some(highlights) = syntax.highlights(language, text) else { return Vec::new() };
    let (triples, _) = highlights.data.as_chunks::<3>();
    triples
        .iter()
        .filter_map(|&[from, to, kind]| Some(HighlightSpan { from, to, kind: *highlights.kinds.get(kind as usize)? }))
        .collect()
}

/// The colour runs (UTF-8 lengths) of each line of `text`, from its sorted `spans`, `plain` between them.
fn line_runs(text: &str, spans: &[HighlightSpan], plain: Hsla, cx: &App) -> Vec<(String, Vec<(usize, Hsla)>)> {
    let mut out = Vec::new();
    let mut line_start = 0u32;
    for piece in text.split_inclusive('\n').take(MAX_LINES) {
        let line = piece.strip_suffix('\n').unwrap_or(piece);
        let line = line.strip_suffix('\r').unwrap_or(line);
        let line_end = line_start + crate::editor_commands::utf16_len(line);
        let mut runs = Vec::new();
        let mut at = 0usize;
        let first = spans.partition_point(|span| span.to <= line_start);
        for span in spans[first..].iter().take_while(|span| span.from < line_end) {
            let from = byte_of_col(line, span.from.saturating_sub(line_start)).max(at);
            let to = byte_of_col(line, span.to.min(line_end) - line_start);
            if from > at {
                runs.push((from - at, plain));
            }
            if to > from {
                runs.push((to - from, theme::color(cx, syntax_token(span.kind))));
                at = to;
            }
        }
        if at < line.len() {
            runs.push((line.len() - at, plain));
        }
        // The next line starts after this one's line break, one or two code units.
        line_start += crate::editor_commands::utf16_len(piece);
        out.push((line.to_owned(), runs));
    }
    out
}

impl PasteHistoryDialog {
    /// Draws the preview as the target editor draws its code: `look` gives the font and the grammar.
    pub fn set_preview_look(&mut self, look: PreviewLook, cx: &mut Context<Self>) {
        self.preview_look = Some(look);
        self.preview_spans = PreviewHighlights::default();
        cx.notify();
    }

    /// The font, size and line height the preview draws with, once the target's look is known.
    pub fn preview_style(&self) -> Option<(Font, Pixels, Pixels)> {
        self.preview_look.as_ref().map(|look| (look.font.clone(), look.font_size, look.line_height))
    }

    /// The preview's lines with their colour runs (UTF-8 lengths), as the preview draws them now.
    pub fn preview_colours(&self, cx: &App) -> Vec<(String, Vec<(usize, Hsla)>)> {
        let spans = self.shown_spans();
        line_runs(self.preview(), spans, theme::color(cx, "fg-editor"), cx)
    }

    /// The spans of the preview's text when they are its own, else none (plain text until they come).
    fn shown_spans(&self) -> &[HighlightSpan] {
        if self.preview_spans.text == self.preview() { &self.preview_spans.spans } else { &[] }
    }

    /// Asks for the highlights of the preview's text unless they are known or on their way.
    fn request_preview_highlights(&mut self, cx: &mut Context<Self>) {
        let Some(look) = self.preview_look.clone() else { return };
        let text = self.preview().to_owned();
        if self.preview_spans.text == text || self.preview_spans.asked.as_deref() == Some(text.as_str()) {
            return;
        }
        self.preview_spans.asked = Some(text.clone());
        let work = cx.background_executor().spawn({
            let text = text.clone();
            async move { highlight(&look.syntax, &look.language, &text) }
        });
        cx.spawn(async move |dialog, cx| {
            let spans = work.await;
            let _ = dialog.update(cx, |dialog: &mut Self, cx| {
                // A later row may have been asked for meanwhile: only its own spans are kept.
                if dialog.preview_spans.asked.as_deref() == Some(text.as_str()) {
                    dialog.preview_spans = PreviewHighlights { text, spans, asked: None };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The preview's lines, drawn in the target's font and colours; plain lines before the look is set.
    pub(super) fn preview_element(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.request_preview_highlights(cx);
        let plain = theme::color(cx, "fg-editor");
        let lines = self.preview_colours(cx);
        let Some(look) = self.preview_look.clone() else {
            return div()
                .children(lines.into_iter().map(|(text, _)| div().child(SharedString::from(text))))
                .into_any_element();
        };
        let rows = lines.into_iter().map(|(text, colours)| {
            let runs: Vec<TextRun> = colours
                .into_iter()
                .map(|(len, color)| TextRun {
                    len,
                    font: look.font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            let row = div().h(look.line_height).text_size(look.font_size).line_height(look.line_height);
            if text.is_empty() {
                row.child(SharedString::from(" ")).text_color(plain).into_any_element()
            } else {
                row.child(StyledText::new(text).with_runs(runs)).into_any_element()
            }
        });
        div().children(rows).into_any_element()
    }
}
