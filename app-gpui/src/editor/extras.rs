//! Editor commands over the folds and the custom regions: Expand to Level N / Expand All to Level
//! N, the fold chevrons of the gutter, the enclosing header lines Context Info shows and the custom
//! folding regions Navigate › Custom Folding lists.
//!
//! They read the fold state [`super::core_actions::CoreState`] keeps on the editor, so they act on
//! the backend's fold ranges of the current text (mapped through the edits until it answers again).

use super::core_actions::apply_fold_edit;
use super::view::Editor;
use crate::editor_commands::folding::{self, FoldEdit, FoldEffect};
use crate::editor_commands::regions::{CustomRegion, custom_regions};
use crate::editor_commands::{CommentTokens, leading_space, utf16_len};
use gpui_kit::Context;
use ide_language::FoldKind;

/// How many lines above the target Navigate › Custom Folding leaves on screen.
const REGION_TOP_MARGIN: u32 = 2;

/// What the gutter shows for a line: the chevron of a fold it starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldMarker {
    /// The line folds a region that is open.
    Open,
    /// The line is the header of a folded region.
    Closed,
}

impl Editor {
    /// Folding › Expand to Level `level` (the innermost region around the caret) or, with `all`,
    /// Expand All to Level `level` (the whole file): the regions nested at most `level` deep open,
    /// the deeper ones fold. Answers whether a fold changed.
    pub fn expand_to_level(&mut self, level: u32, all: bool, cx: &mut Context<Self>) -> bool {
        let state = self.edit_state();
        let Some(edit) = folding::expand_to_level(&state, self.core.folds.context(), level, all) else {
            return false;
        };
        apply_fold_edit(self, &state, edit, cx);
        true
    }

    /// The chevron the gutter shows on line `line`, if the line starts a fold.
    pub fn fold_marker(&self, line: u32) -> Option<FoldMarker> {
        let state = self.edit_state();
        if line >= state.line_count() {
            return None;
        }
        if self.core.folds.folded_on(&state, line).is_some() {
            Some(FoldMarker::Closed)
        } else if self.core.folds.is_foldable(line) {
            Some(FoldMarker::Open)
        } else {
            None
        }
    }

    /// A click on the chevron of line `line`: its folded region opens, else the region the line
    /// starts folds. Answers whether the line has a fold to toggle.
    pub fn toggle_fold_at_line(&mut self, line: u32, cx: &mut Context<Self>) -> bool {
        let state = self.edit_state();
        if line >= state.line_count() {
            return false;
        }
        let effect = match self.core.folds.folded_on(&state, line) {
            Some(region) => FoldEffect::Unfold(region),
            None => match self.core.folds.context().foldable_at(line) {
                Some(region) => FoldEffect::Fold(region),
                None => return false,
            },
        };
        apply_fold_edit(self, &state, FoldEdit { effects: vec![effect], selection: None }, cx);
        true
    }

    /// View › Context Info: the 0-based header lines of the code regions around the caret's line
    /// that start above it (class, method, block headers), outermost first, at most `max` of them
    /// (the innermost ones).
    pub fn context_header_lines(&self, max: u32) -> Vec<u32> {
        let state = self.edit_state();
        let caret_line = self.caret_position().0;
        let line_start = state.line(caret_line).from;
        let mut headers: Vec<u32> = self
            .core
            .folds
            .backend()
            .iter()
            .take_while(|fold| fold.from < line_start)
            .filter(|fold| fold.kind == FoldKind::Region && fold.to >= line_start)
            .map(|fold| state.line_at(fold.from).number)
            .filter(|&line| line < caret_line)
            .collect();
        headers.sort_unstable();
        headers.dedup();
        let keep = headers.len().saturating_sub(max.max(1) as usize);
        headers.split_off(keep)
    }

    /// The custom folding regions of the file (`//region` … `//endregion` and the like), in order.
    pub fn custom_regions(&self) -> Vec<CustomRegion> {
        let state = self.edit_state();
        let comments = |pos: u32| self.facts().comments_at(pos);
        custom_regions(&state, &comments)
    }

    /// Moves the caret to the first non-blank character of line `line` and scrolls the line near
    /// the top, as choosing a custom region does.
    pub fn go_to_line_text(&mut self, line: u32, cx: &mut Context<Self>) {
        let line = line.min(self.line_count().saturating_sub(1));
        let col = utf16_len(leading_space(&self.line_text(line)));
        let offset = self.offset_of(line, col);
        self.select(offset, offset, cx);
        self.scroll_to_line(line.saturating_sub(REGION_TOP_MARGIN), cx);
    }
}

/// A bracket of the text and its nesting depth from 0; `None`: it has no partner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BracketMark {
    /// Its UTF-16 offset.
    pub at: u32,
    pub depth: Option<u32>,
}

/// How a scan for brackets steps over strings and comments: the UTF-16 ranges the highlighter
/// found, sorted; or, without highlights, the language's comment tokens and quotes.
pub enum Literals<'a> {
    Ranges(&'a [(u32, u32)]),
    Lexical(&'a CommentTokens),
}

/// The brackets kept for the text version and highlights they were found in.
#[derive(Debug, Default)]
pub struct BracketCache {
    key: Option<(u32, u32)>,
    marks: std::rc::Rc<Vec<BracketMark>>,
}

/// What the scanner is inside of, without highlights.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lexical {
    Code,
    LineComment,
    BlockComment,
    Quoted(char),
}

/// The brackets `()`, `[]` and `{}` of `text` outside strings and comments, each with its depth;
/// one without a partner (a closer that closes nothing, an opener never closed) has none.
pub fn bracket_marks(text: &str, literals: Literals<'_>) -> Vec<BracketMark> {
    let mut out: Vec<BracketMark> = Vec::new();
    // Each open bracket: its closer and its index in `out`.
    let mut open: Vec<(char, usize)> = Vec::new();
    let (mut i, mut at) = (0usize, 0u32);
    let mut skip = 0usize;
    let mut mode = Lexical::Code;
    while let Some(ch) = text[i..].chars().next() {
        let step = |i: &mut usize, at: &mut u32, ch: char| {
            *i += ch.len_utf8();
            *at += ch.len_utf16() as u32;
        };
        match &literals {
            Literals::Ranges(ranges) => {
                while ranges.get(skip).is_some_and(|&(_, to)| to <= at) {
                    skip += 1;
                }
                if ranges.get(skip).is_some_and(|&(from, _)| from <= at) {
                    step(&mut i, &mut at, ch);
                    continue;
                }
            }
            Literals::Lexical(comments) => {
                let rest = &text[i..];
                match mode {
                    Lexical::Code => {
                        let line = comments.line.as_deref().filter(|token| rest.starts_with(token));
                        let block = comments.block.as_ref().filter(|(token, _)| rest.starts_with(token.as_str()));
                        let entered = if let Some(token) = line {
                            Some((Lexical::LineComment, token.len()))
                        } else if let Some((token, _)) = block {
                            Some((Lexical::BlockComment, token.len()))
                        } else if ch == '"' || ch == '`' || (ch == '\'' && quote_starts_string(text, i)) {
                            Some((Lexical::Quoted(ch), 1))
                        } else {
                            None
                        };
                        if let Some((next, len)) = entered {
                            mode = next;
                            at += text[i..i + len].encode_utf16().count() as u32;
                            i += len;
                            continue;
                        }
                    }
                    Lexical::LineComment | Lexical::BlockComment => {
                        let close = match (mode, &comments.block) {
                            (Lexical::BlockComment, Some((_, close))) => close.as_str(),
                            _ => "\n",
                        };
                        if rest.starts_with(close) {
                            mode = Lexical::Code;
                            at += close.encode_utf16().count() as u32;
                            i += close.len();
                        } else {
                            step(&mut i, &mut at, ch);
                        }
                        continue;
                    }
                    Lexical::Quoted(quote) => {
                        step(&mut i, &mut at, ch);
                        if ch == '\\' {
                            if let Some(escaped) = text[i..].chars().next() {
                                step(&mut i, &mut at, escaped);
                            }
                        } else if ch == quote || (ch == '\n' && quote != '`') {
                            mode = Lexical::Code;
                        }
                        continue;
                    }
                }
            }
        }
        match ch {
            '(' | '[' | '{' => {
                let closer = match ch {
                    '(' => ')',
                    '[' => ']',
                    _ => '}',
                };
                out.push(BracketMark { at, depth: Some(open.len() as u32) });
                open.push((closer, out.len() - 1));
            }
            ')' | ']' | '}' => {
                if open.last().is_some_and(|&(closer, _)| closer == ch) {
                    open.pop();
                    out.push(BracketMark { at, depth: Some(open.len() as u32) });
                } else {
                    out.push(BracketMark { at, depth: None });
                }
            }
            _ => {}
        }
        step(&mut i, &mut at, ch);
    }
    for (_, index) in open {
        out[index].depth = None;
    }
    out
}

/// Whether the `'` at byte `i` starts a string: not when it ends a word (Haskell's `x'`) or has no
/// partner on its line (Rust's lifetimes).
fn quote_starts_string(text: &str, i: usize) -> bool {
    if text[..i].chars().next_back().is_some_and(|before| before.is_alphanumeric() || before == '_') {
        return false;
    }
    let line = text[i + 1..].split('\n').next().unwrap_or("");
    line.contains('\'')
}

impl Editor {
    /// The brackets of the text with their depths, found again only when the text or its
    /// highlights changed. Strings and comments are skipped: by the highlights of the current
    /// text when there are, else by the language's comment tokens and quotes.
    pub fn bracket_marks(&self) -> std::rc::Rc<Vec<BracketMark>> {
        use ide_language::SyntaxHighlightKind as K;
        let key = (self.version(), self.highlights_version);
        let mut cache = self.brackets.borrow_mut();
        if cache.key != Some(key) {
            let text = self.text();
            let current = self.highlights_version == self.version() && !self.highlights.is_empty();
            let marks = if current {
                let literal = |kind: K| {
                    matches!(kind, K::Comment | K::DocComment | K::String | K::StringSpecial | K::Escape | K::Regexp)
                };
                let mut ranges: Vec<(u32, u32)> = Vec::new();
                for span in self.highlights.iter().filter(|span| literal(span.kind)) {
                    match ranges.last_mut() {
                        Some(last) if span.from <= last.1 => last.1 = last.1.max(span.to),
                        _ => ranges.push((span.from, span.to)),
                    }
                }
                bracket_marks(&text, Literals::Ranges(&ranges))
            } else {
                bracket_marks(&text, Literals::Lexical(&self.facts().language.comments))
            };
            *cache = BracketCache { key: Some(key), marks: std::rc::Rc::new(marks) };
        }
        cache.marks.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn depths(text: &str, literals: Literals<'_>) -> Vec<(u32, Option<u32>)> {
        bracket_marks(text, literals).into_iter().map(|mark| (mark.at, mark.depth)).collect()
    }

    // Brackets nest by depth; a closer without its opener and an opener never closed have none.
    #[test]
    fn brackets_are_coloured_by_depth_and_unmatched_ones_are_marked() {
        let none = CommentTokens::default();
        assert_eq!(
            depths("f(a[b{c}])", Literals::Lexical(&none)),
            [(1, Some(0)), (3, Some(1)), (5, Some(2)), (7, Some(2)), (8, Some(1)), (9, Some(0))]
        );
        assert_eq!(depths("(]", Literals::Lexical(&none)), [(0, None), (1, None)]);
    }

    // Without highlights strings and comments are skipped by the language's tokens and quotes; a
    // lifetime's quote starts no string.
    #[test]
    fn strings_and_comments_are_skipped_lexically() {
        let rust = CommentTokens { line: Some("//".to_owned()), block: Some(("/*".to_owned(), "*/".to_owned())) };
        let text = "(\"(\" // (\n/* { */ 'a)";
        assert_eq!(depths(text, Literals::Lexical(&rust)), [(0, Some(0)), (20, Some(0))]);
        // The ranges the highlighter found are skipped as given.
        assert_eq!(depths("(x)(y)", Literals::Ranges(&[(0, 3)])), [(3, Some(0)), (5, Some(0))]);
    }
}
