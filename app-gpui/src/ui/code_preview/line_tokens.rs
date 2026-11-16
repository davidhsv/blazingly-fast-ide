//! Syntax colours of result lines (the rows of Find Usages and Show Usages): each file is parsed whole
//! once, off the window's thread, so a line inside a block comment or a multi-line string is coloured as
//! such. A line whose file no longer holds its text there (it changed since the search), or a file too
//! large to parse, is coloured on its own. Rows only look the colours up while they are drawn.

use crate::backend::Backend;
use crate::code_insight::usages::{Usage, UsageGroup, trim_preview};
use crate::editor::view::{HighlightSpan, syntax_token};
use crate::find::utf16;
use crate::theme;
use futures::StreamExt as _;
use futures::channel::mpsc::unbounded;
use gpui_kit::{App, Global, Hsla};
use ide_core::LanguageId;
use ide_language::Syntax;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Files longer than this (UTF-16 code units) are not parsed whole: their lines are coloured on their own.
pub const MAX_PARSE: u32 = 500_000;

/// Where a result line's `text` sits in its file: on the 0-based `line`, from the UTF-16 column `col`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LineSource {
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub text: String,
}

/// A file parsed whole: its lines, where each starts (UTF-16) and the highlights of the whole text.
pub struct ParsedFile {
    lines: Vec<String>,
    line_starts: Vec<u32>,
    spans: Vec<HighlightSpan>,
}

impl ParsedFile {
    /// `text` (lines broken by `\n`) parsed with the grammar of `language`; none without a grammar for it
    /// or when the text is longer than [`MAX_PARSE`].
    pub fn parse(syntax: &Syntax, language: &LanguageId, text: &str) -> Option<Self> {
        if utf16::len(text) > MAX_PARSE {
            return None;
        }
        let spans = highlight(syntax, language, text)?;
        // Splitting on `\n` alone keeps a `\r` in its line, so the offsets stay those of `text`.
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let mut line_starts = Vec::with_capacity(lines.len());
        let mut at = 0u32;
        for line in &lines {
            line_starts.push(at);
            at += utf16::len(line) + 1;
        }
        Some(Self { lines, line_starts, spans })
    }

    /// The highlights of `text` at column `col` of the 0-based `line`, in columns of `text`; none when the
    /// file does not hold `text` there.
    fn tokens_at(&self, line: u32, col: u32, text: &str) -> Option<Vec<HighlightSpan>> {
        let (line_text, start) = (self.lines.get(line as usize)?, *self.line_starts.get(line as usize)?);
        let len = utf16::len(text);
        if utf16::slice(line_text, col, col + len) != text {
            return None;
        }
        let (from, to) = (start + col, start + col + len);
        let first = self.spans.partition_point(|span| span.to <= from);
        let tokens = self.spans[first..]
            .iter()
            .take_while(|span| span.from < to)
            .map(|span| HighlightSpan { from: span.from.max(from) - from, to: span.to.min(to) - from, kind: span.kind })
            .filter(|span| span.to > span.from)
            .collect();
        Some(tokens)
    }
}

/// The highlight spans (UTF-16 offsets, sorted) of `text` in `language`; none without a grammar for it.
fn highlight(syntax: &Syntax, language: &LanguageId, text: &str) -> Option<Vec<HighlightSpan>> {
    let highlights = syntax.highlights(language, text)?;
    let (triples, _) = highlights.data.as_chunks::<3>();
    let spans = triples
        .iter()
        .filter_map(|&[from, to, kind]| Some(HighlightSpan { from, to, kind: *highlights.kinds.get(kind as usize)? }))
        .collect();
    Some(spans)
}

/// The tokens of `text`, the line `source` names, in columns of `text`: from the whole file when `parsed`
/// holds that text there, else from `text` parsed on its own.
pub fn line_tokens(
    syntax: &Syntax,
    language: &LanguageId,
    parsed: Option<&ParsedFile>,
    source: &LineSource,
) -> Vec<HighlightSpan> {
    if source.text.is_empty() {
        return Vec::new();
    }
    let whole = parsed.and_then(|parsed| parsed.tokens_at(source.line, source.col, &source.text));
    whole.unwrap_or_else(|| highlight(syntax, language, &source.text).unwrap_or_default())
}

/// The colour runs of `text` with `from..to` (UTF-16 columns) marked: `(piece, colour, marked)`. The pieces
/// outside the mark take their tokens' colours, `plain` between them; the marked piece is drawn whole in
/// `marked`, the search match's foreground, whatever its tokens are.
pub fn marked_runs(
    text: &str,
    from: u32,
    to: u32,
    tokens: &[HighlightSpan],
    plain: Hsla,
    marked: Hsla,
    cx: &App,
) -> Vec<(String, Hsla, bool)> {
    let len = utf16::len(text);
    let (from, to) = (from.min(len), to.min(len).max(from.min(len)));
    let mut runs = Vec::new();
    let coloured = |runs: &mut Vec<(String, Hsla, bool)>, start: u32, end: u32| {
        let mut at = start;
        for token in tokens.iter().filter(|token| token.to > start && token.from < end) {
            let (s, e) = (token.from.max(at), token.to.min(end));
            if s > at {
                runs.push((utf16::slice(text, at, s).to_owned(), plain, false));
            }
            if e > s {
                runs.push((utf16::slice(text, s, e).to_owned(), theme::color(cx, syntax_token(token.kind)), false));
                at = e;
            }
        }
        if at < end {
            runs.push((utf16::slice(text, at, end).to_owned(), plain, false));
        }
    };
    coloured(&mut runs, 0, from);
    if to > from {
        runs.push((utf16::slice(text, from, to).to_owned(), marked, true));
    }
    coloured(&mut runs, to, len);
    runs
}

/// Where the trimmed line a row shows of `usage` sits in its file.
pub fn usage_source(usage: &Usage) -> LineSource {
    let trimmed = trim_preview(usage);
    LineSource { path: usage.path.clone(), line: usage.line, col: trimmed.text_col, text: trimmed.text }
}

/// The lines the rows of `groups` show.
pub fn group_sources(groups: &[UsageGroup]) -> Vec<LineSource> {
    groups.iter().flat_map(|group| group.usages.iter().map(usage_source)).collect()
}

/// The colour runs of the row of `usage`: its trimmed line in its file's colours, `fg` where no token
/// colours it, the usage itself in the search match's foreground.
pub fn usage_runs(usage: &Usage, cx: &App) -> Vec<(String, Hsla, bool)> {
    let trimmed = trim_preview(usage);
    let source = usage_source(usage);
    let tokens = LineTokens::tokens(cx, &source).unwrap_or_default();
    let (plain, marked) = (theme::color(cx, "fg"), theme::color(cx, "fg-search-match"));
    marked_runs(&trimmed.text, trimmed.from, trimmed.to, &tokens, plain, marked, cx)
}

/// The colours known for result lines, by line, and the files asked for. Only the tokens of the lines asked
/// for are kept, never a file's text or parse; a line is known by its text too, so a line that changed is
/// asked for again. A new project starts afresh ([`LineTokens::clear`]).
#[derive(Default)]
pub struct LineTokens {
    lines: HashMap<LineSource, Vec<HighlightSpan>>,
    /// The files being read and parsed now.
    asked: HashSet<String>,
}

impl Global for LineTokens {}

impl LineTokens {
    /// The tokens of the line `source` names, once they are known. Reading does not change the global, so
    /// a row can ask while it is drawn.
    pub fn tokens(cx: &App, source: &LineSource) -> Option<Vec<HighlightSpan>> {
        cx.try_global::<Self>()?.lines.get(source).cloned()
    }

    /// Keeps the tokens of `lines`, all of the file `path`, with what is known of its other lines.
    pub fn store(cx: &mut App, path: &str, lines: Vec<(LineSource, Vec<HighlightSpan>)>) {
        let known = cx.default_global::<Self>();
        known.asked.remove(path);
        known.lines.extend(lines);
    }

    /// Forgets every line: they are read again, as they are now.
    pub fn clear(cx: &mut App) {
        if let Some(known) = cx.try_global::<Self>()
            && known.lines.is_empty()
            && known.asked.is_empty()
        {
            return;
        }
        cx.set_global(Self::default());
    }
}

/// Reads and parses, off the window's thread, the files of `lines` the window `label` shows that are not
/// known or asked for yet: the text an open editor has, else the file on disk. The files are read one after
/// the other in one backend task, and each file's rows are drawn again once its colours are kept. A file
/// that cannot be read keeps its lines plain.
pub fn request(label: &str, lines: Vec<LineSource>, cx: &mut App) {
    let Some(backend) = cx.try_global::<Backend>() else { return };
    let known = cx.try_global::<LineTokens>();
    let mut by_file: HashMap<String, Vec<LineSource>> = HashMap::new();
    for source in lines {
        let skip = known.is_some_and(|known| known.lines.contains_key(&source) || known.asked.contains(&source.path));
        if !skip {
            by_file.entry(source.path.clone()).or_default().push(source);
        }
    }
    if by_file.is_empty() {
        return;
    }
    let paths: Vec<String> = by_file.keys().cloned().collect();
    let window = label.to_owned();
    let (sender, mut parsed) = unbounded();
    let work = backend.with_host(move |host| async move {
        let syntax = host.syntax();
        for (path, sources) in by_file {
            let (project, file) = (host.project(&window), path.clone());
            let syntax = syntax.clone();
            let lines = match project {
                Ok(project) => {
                    host.blocking(move || {
                        let at = Path::new(&file);
                        let text = project.text_of(at)?;
                        let language = project.languages.detect(at, text.lines().next());
                        let parsed = ParsedFile::parse(&syntax, &language, &text);
                        let tokens = |source: LineSource| {
                            let tokens = line_tokens(&syntax, &language, parsed.as_ref(), &source);
                            (source, tokens)
                        };
                        Ok(sources.into_iter().map(tokens).collect::<Vec<_>>())
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            if sender.unbounded_send((path, lines)).is_err() {
                break;
            }
        }
    });
    cx.default_global::<LineTokens>().asked.extend(paths.iter().cloned());
    cx.spawn(async move |cx| {
        while let Some((path, lines)) = parsed.next().await {
            let lines = lines.unwrap_or_else(|error| {
                tracing::debug!("no colours for the lines of {path}: {}", error.message);
                Vec::new()
            });
            cx.update(|cx| LineTokens::store(cx, &path, lines));
        }
        // The backend task ended (or could not run): what it did not answer is asked for again next time.
        if let Err(error) = work.await {
            tracing::debug!("no colours for result lines: {error}");
        }
        cx.update(|cx| cx.default_global::<LineTokens>().asked.retain(|path| !paths.contains(path)));
    })
    .detach();
}
