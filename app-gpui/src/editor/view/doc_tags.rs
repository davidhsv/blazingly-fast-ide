//! The tags of documentation comments (`@param name`, `{@link …}`, HTML markup, KDoc `[links]`),
//! coloured over the comment's own colour in the languages that have them. They are found in the
//! painted lines only, from the comments the highlights mark there, so a frame's cost does not grow
//! with the document; a change of the highlights or of the document's language draws them anew.

use super::{Editor, HighlightSpan};
use crate::editor::buffer;
use crate::editor_commands::doc_comment_tags::{
    DocCommentTagKind, DocCommentTagRange, has_doc_comment_tags, visible_doc_comment_tags,
};
use crate::editor_commands::tree::{SyntaxTree, TreeNode};
use crate::theme;
use gpui_kit::{App, Context, Font, Hsla, TextRun};
use ide_core::LanguageId;
use ide_language::SyntaxHighlightKind;
use std::ops::Range;

/// The colour token of a kind of tag range.
fn tag_token(kind: DocCommentTagKind) -> &'static str {
    match kind {
        DocCommentTagKind::Tag => "syn-doc-comment-tag",
        DocCommentTagKind::Value => "syn-doc-comment-tag-value",
        DocCommentTagKind::Markup => "syn-doc-markup",
    }
}

/// The comments of `highlights` (sorted, not overlapping) that touch `ranges` (sorted), as the
/// nodes of a tree under a node covering `len`: a documentation comment is a `DocComment`, any other
/// comment a `Comment`. Adjacent spans of one kind are one comment. Only the spans touching a range
/// are read.
fn comment_tree(highlights: &[HighlightSpan], ranges: &[Range<u32>], len: u32) -> SyntaxTree {
    let mut comments: Vec<(u32, u32, bool)> = Vec::new();
    // The first span not read yet: ranges are in order, so a span is read once.
    let mut next = 0;
    for range in ranges {
        let mut at = next.max(highlights.partition_point(|span| span.to < range.start));
        while let Some(span) = highlights.get(at).filter(|span| span.from <= range.end) {
            let doc = match span.kind {
                SyntaxHighlightKind::DocComment => Some(true),
                SyntaxHighlightKind::Comment => Some(false),
                _ => None,
            };
            // Highlights of an older text can reach past the end of this one.
            let (from, to) = (span.from.min(len), span.to.min(len));
            if let Some(doc) = doc.filter(|_| from < to) {
                match comments.last_mut() {
                    Some(last) if last.2 == doc && last.1 >= from => last.1 = last.1.max(to),
                    _ => comments.push((from, to, doc)),
                }
            }
            at += 1;
        }
        next = at;
    }
    let mut nodes = Vec::with_capacity(comments.len() + 1);
    nodes.push(TreeNode::new("Document", 0, len));
    nodes.extend(
        comments.into_iter().map(|(from, to, doc)| TreeNode::new(if doc { "DocComment" } else { "Comment" }, from, to)),
    );
    SyntaxTree::new(nodes)
}

/// `rows` (document line ranges in order) merged where one ends right before the next starts.
fn visible_ranges(rows: &[Range<u32>]) -> Vec<Range<u32>> {
    let mut merged: Vec<Range<u32>> = Vec::with_capacity(rows.len());
    for row in rows {
        match merged.last_mut() {
            Some(last) if row.start <= last.end + 2 => last.end = row.end,
            _ => merged.push(row.clone()),
        }
    }
    merged
}

impl Editor {
    /// The tag ranges of the documentation comments on the painted lines `lines` (document lines, in
    /// order), sorted and not overlapping; none in a language without doc comment tags.
    pub(super) fn painted_doc_tags(&self, lines: &[u32]) -> Vec<DocCommentTagRange> {
        if lines.is_empty() || !has_doc_comment_tags(self.language.as_str()) {
            return Vec::new();
        }
        let rows: Vec<Range<u32>> = lines.iter().map(|line| buffer::line_range(&self.rope, *line)).collect();
        let visible = visible_ranges(&rows);
        let tree = comment_tree(&self.highlights, &visible, buffer::len_utf16(&self.rope));
        if tree.len() < 2 {
            return Vec::new();
        }
        let spans: Vec<(u32, u32)> = visible.iter().map(|range| (range.start, range.end)).collect();
        visible_doc_comment_tags(&self.edit_state(), &tree, self.language.as_str(), &spans)
    }

    /// The document's language changed (Override File Type, a file type association): the commands
    /// read its facts, and the highlights and the doc comment tags follow it.
    pub(super) fn change_language(&mut self, language: LanguageId, cx: &mut Context<Self>) {
        if self.language == language {
            return;
        }
        self.language = language;
        self.read_facts();
        self.refresh_highlights(cx);
        cx.notify();
    }
}

/// `runs` (covering the bytes of the line `text`, which starts at offset `line_start`) with the tag
/// ranges of `tags` that fall on it in their colours, upright and in the main weight as the font
/// `font` is.
pub(super) fn colour_doc_tags(
    runs: Vec<TextRun>,
    text: &str,
    line_start: u32,
    tags: &[DocCommentTagRange],
    font: &Font,
    cx: &App,
) -> Vec<TextRun> {
    let line_end = line_start + crate::editor_commands::utf16_len(text);
    let first = tags.partition_point(|tag| tag.to <= line_start);
    let mut runs = runs;
    for tag in tags[first..].iter().take_while(|tag| tag.from < line_end) {
        let from = buffer::byte_of_col(text, tag.from.saturating_sub(line_start));
        let to = buffer::byte_of_col(text, tag.to.min(line_end) - line_start);
        runs = restyle(runs, from, to, theme::color(cx, tag_token(tag.kind)), font);
    }
    runs
}

/// `runs` with the bytes `from..to` in `color` and `font`.
fn restyle(runs: Vec<TextRun>, from: usize, to: usize, color: Hsla, font: &Font) -> Vec<TextRun> {
    let mut out = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let (start, end) = (at, at + run.len);
        at = end;
        let (cut_from, cut_to) = (from.clamp(start, end), to.clamp(start, end));
        if cut_from >= cut_to {
            out.push(run);
            continue;
        }
        for (a, b, styled) in [(start, cut_from, false), (cut_from, cut_to, true), (cut_to, end, false)] {
            if b > a {
                let mut part = TextRun { len: b - a, ..run.clone() };
                if styled {
                    part.color = color;
                    part.font = font.clone();
                }
                out.push(part);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(from: u32, to: u32, kind: SyntaxHighlightKind) -> HighlightSpan {
        HighlightSpan { from, to, kind }
    }

    #[test]
    fn reads_the_comments_touching_the_painted_ranges_and_joins_adjacent_spans() {
        use SyntaxHighlightKind as K;
        let highlights = [
            span(0, 10, K::DocComment),
            span(10, 20, K::DocComment),
            span(21, 25, K::Keyword),
            span(30, 40, K::Comment),
        ];
        let tree = comment_tree(&highlights, std::slice::from_ref(&(0..12)), 50);
        assert_eq!((tree.len(), tree.name(1), tree.node(1).to), (2, "DocComment", 20));
        let tree = comment_tree(&highlights, &[22..23, 35..36], 50);
        assert_eq!((tree.len(), tree.name(1)), (2, "Comment"));
        assert_eq!(comment_tree(&highlights, std::slice::from_ref(&(41..45)), 50).len(), 1);
    }

    #[test]
    fn rows_that_follow_each_other_are_one_range() {
        assert_eq!(visible_ranges(&[0..4, 5..9, 20..25]), [0..9, 20..25]);
        assert_eq!(visible_ranges(&[0..4, 6..9]), std::slice::from_ref(&(0..9)));
    }
}
