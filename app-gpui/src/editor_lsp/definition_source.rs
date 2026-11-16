//! The source Quick Definition shows: the definition's whole declaration as the syntax tree spans
//! it, read off the window's thread, and drawn in the editor's code font with the colours of the
//! declaration's language.

use super::definition_view::DefinitionTarget;
use super::files::target_label;
use super::hover::definition_span;
use crate::editor::buffer::byte_of_col;
use crate::editor::view::{HighlightSpan, syntax_token};
use crate::editor_commands::utf16_len;
use crate::platform::virtual_docs::basename;
use crate::theme;
use gpui_kit::{AnyElement, App, Font, Hsla, IntoElement, SharedString, StyledText, TextRun, div, prelude::*};
use ide_core::ipc_path::to_ipc;
use ide_core::{IdeResult, LanguageId, Location, Range};
use ide_host::Host;
use ide_language::Syntax;

/// The colour runs of a line: UTF-8 lengths and their colours.
pub type LineRuns = Vec<(usize, Hsla)>;

/// The highlight spans of `text` in `language`; none without a grammar for it.
fn highlight(syntax: &Syntax, language: &LanguageId, text: &str) -> Vec<HighlightSpan> {
    let Some(highlights) = syntax.highlights(language, text) else { return Vec::new() };
    let (triples, _) = highlights.data.as_chunks::<3>();
    triples
        .iter()
        .filter_map(|&[from, to, kind]| Some(HighlightSpan { from, to, kind: *highlights.kinds.get(kind as usize)? }))
        .collect()
}

impl DefinitionTarget {
    /// The definition at `location` in a file of `text`, showing the lines of `declaration` (the
    /// syntax tree's declaration around the location) when it holds the location, else a window
    /// around the name; the lines shown are coloured as `language` colours them. `None` when the
    /// location lies past the text's end.
    pub fn read_declared(
        location: Location,
        text: &str,
        declaration: Option<Range>,
        language: &LanguageId,
        syntax: &Syntax,
    ) -> Option<Self> {
        let lines: Vec<&str> = text.lines().collect();
        let start = location.range.start;
        let line = *lines.get(start.line as usize)?;
        let (first, last) = definition_span(&location, lines.len() as u32, declaration);
        let name = target_label(line, start.col).unwrap_or_else(|| basename(&to_ipc(&location.path)).into_owned());
        let shown: Vec<String> = lines[first as usize..=last as usize].iter().map(|line| (*line).to_owned()).collect();
        let spans = highlight(syntax, language, &shown.join("\n"));
        Some(Self { location, name, first_line: first, lines: shown, spans })
    }
}

/// Reads each of `locations` as Quick Definition shows it: the file's text from its document
/// (opened for the question and closed again), the declaration around the location from the
/// syntax tree, and the colours of the lines shown. A location whose file cannot be read is left
/// out; one whose language has no grammar shows the lines around the name.
pub async fn read_definitions(host: &Host, label: &str, locations: Vec<Location>) -> IdeResult<Vec<DefinitionTarget>> {
    let project = host.project(label)?;
    let syntax = host.syntax();
    let mut targets = Vec::new();
    for location in locations {
        let path = location.path.0.clone();
        let (text, language, declaration) = match ide_host::ops::doc::open(host, label, path.clone()).await {
            Ok(info) => {
                let pos = location.range.start;
                let ranges = ide_host::ops::syntax::syntax_structural_ranges(host, label, info.id, pos, None).await;
                if let Err(error) = ide_host::ops::doc::close(host, label, info.id) {
                    tracing::warn!("the document of {} stays open: {}", path.display(), error.message);
                }
                let declaration = match ranges {
                    Ok(ranges) => ranges.declaration.map(|node| node.range),
                    Err(error) => {
                        tracing::debug!("no declaration around {}: {}", path.display(), error.message);
                        None
                    }
                };
                (info.text, info.language, declaration)
            }
            Err(error) => {
                tracing::debug!("{} is read from disk: {}", path.display(), error.message);
                let Ok(text) = project.text_of(&path) else { continue };
                let language = project.languages.detect(&path, text.lines().next());
                (text, language, None)
            }
        };
        targets.extend(DefinitionTarget::read_declared(location, &text, declaration, &language, &syntax));
    }
    Ok(targets)
}

/// The shown lines of `target` with their colour runs, each after its 1-based line number, the
/// number and the text between the spans in `plain`.
pub fn line_colours(target: &DefinitionTarget, plain: Hsla, cx: &App) -> Vec<(String, LineRuns)> {
    let spans = &target.spans;
    let mut out = Vec::with_capacity(target.lines.len());
    let mut line_start = 0u32;
    for (at, line) in target.lines.iter().enumerate() {
        let number = format!("{}  ", target.first_line + 1 + at as u32);
        let line_end = line_start + utf16_len(line);
        let mut runs = vec![(number.len(), plain)];
        let mut done = 0usize;
        let first = spans.partition_point(|span| span.to <= line_start);
        for span in spans[first..].iter().take_while(|span| span.from < line_end) {
            let from = byte_of_col(line, span.from.saturating_sub(line_start)).max(done);
            let to = byte_of_col(line, span.to.min(line_end) - line_start);
            if from > done {
                runs.push((from - done, plain));
            }
            if to > from {
                runs.push((to - from, theme::color(cx, syntax_token(span.kind))));
                done = to;
            }
        }
        if done < line.len() {
            runs.push((line.len() - done, plain));
        }
        // The lines were joined with one line break.
        line_start = line_end + 1;
        out.push((number + line, runs));
    }
    out
}

/// The rows of the popup's body: the shown lines of `target`, numbered, in `font` and coloured.
pub fn source_rows(target: &DefinitionTarget, font: &Font, cx: &App) -> Vec<AnyElement> {
    let plain = theme::color(cx, "fg-editor");
    line_colours(target, plain, cx)
        .into_iter()
        .map(|(text, colours)| {
            let runs: Vec<TextRun> = colours
                .into_iter()
                .map(|(len, color)| TextRun {
                    len,
                    font: font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                })
                .collect();
            let text = StyledText::new(SharedString::from(text)).with_runs(runs);
            div().whitespace_nowrap().child(text).into_any_element()
        })
        .collect()
}
