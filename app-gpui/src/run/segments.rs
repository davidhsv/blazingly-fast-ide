//! Splits a console line's styled spans at link and find-match boundaries for rendering.

use crate::find::utf16;
use crate::run::ansi::{Span, SpanStyle};
use crate::run::links::OutputLink;

/// A find match on one line, in UTF-16 columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SegmentMark {
    pub start: u32,
    pub end: u32,
    /// The match the find bar is on.
    pub current: bool,
}

/// A piece of a line drawn in one way: one style, at most one link, inside or outside a find match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: String,
    pub style: SpanStyle,
    pub link: Option<&'a OutputLink>,
    pub mark: bool,
    pub current: bool,
}

/// The segments of a line. `links` and `marks` are sorted by start and do not overlap among themselves.
pub fn segment_line<'a>(spans: &[Span], links: &'a [OutputLink], marks: &[SegmentMark]) -> Vec<Segment<'a>> {
    let mut out: Vec<Segment<'a>> = Vec::new();
    // The link of the last segment, by index: two segments join only inside the same link.
    let mut last_link: Option<usize> = None;
    let (mut pos, mut li, mut mi) = (0u32, 0usize, 0usize);
    for span in spans {
        let length = utf16::len(&span.text);
        let mut offset = 0u32;
        while offset < length {
            let abs = pos + offset;
            while links.get(li).is_some_and(|link| link.end() <= abs) {
                li += 1;
            }
            while marks.get(mi).is_some_and(|mark| mark.end <= abs) {
                mi += 1;
            }
            let mut take = length - offset;
            let mut link = None;
            if let Some(next) = links.get(li) {
                if next.start() <= abs {
                    link = Some(li);
                    take = take.min(next.end() - abs);
                } else {
                    take = take.min(next.start() - abs);
                }
            }
            let mut mark = None;
            if let Some(next) = marks.get(mi) {
                if next.start <= abs {
                    mark = Some(*next);
                    take = take.min(next.end - abs);
                } else {
                    take = take.min(next.start - abs);
                }
            }
            let piece = utf16::slice(&span.text, offset, offset + take);
            let current = mark.is_some_and(|mark| mark.current);
            match out.last_mut() {
                Some(previous)
                    if last_link == link
                        && previous.mark == mark.is_some()
                        && previous.current == current
                        && previous.style == span.style =>
                {
                    previous.text.push_str(piece);
                }
                _ => {
                    out.push(Segment {
                        text: piece.to_string(),
                        style: span.style,
                        link: link.map(|index| &links[index]),
                        mark: mark.is_some(),
                        current,
                    });
                    last_link = link;
                }
            }
            offset += take.max(1);
        }
        pos += length;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::ansi::AnsiColor;
    use crate::run::console::{ChunkKind, ConsoleModel};
    use crate::run::links::find_output_links;
    mod proofs_marks;

    #[test]
    fn splits_styled_spans_at_link_boundaries() {
        let mut model = ConsoleModel::default();
        model.append(ChunkKind::Stderr, "error at \x1b[1msrc/ma\x1b[0min.rs:3:1 here\n");
        let line = &model.lines()[0];
        let links = find_output_links(&line.text);
        let segments = segment_line(&line.spans, &links, &[]);
        let shape: Vec<(&str, bool, bool)> =
            segments.iter().map(|s| (s.text.as_str(), s.link.is_some(), s.style.bold)).collect();
        assert_eq!(
            shape,
            [("error at ", false, false), ("src/ma", true, true), ("in.rs:3:1", true, false), (" here", false, false)]
        );
        assert_eq!(segments.iter().map(|s| s.text.as_str()).collect::<String>(), line.text);
    }

    #[test]
    fn returns_spans_unchanged_without_links() {
        let coloured = SpanStyle { fg: Some(AnsiColor::Named(1)), ..SpanStyle::default() };
        let spans = [Span { text: "a".into(), style: coloured }, Span::plain("b")];
        let segment = |text: &str, style: SpanStyle| {
            let (link, mark, current) = (None, false, false);
            Segment { text: text.into(), style, link, mark, current }
        };
        assert_eq!(segment_line(&spans, &[], &[]), [segment("a", coloured), segment("b", SpanStyle::default())]);
    }

    // Not from a spec: a find match splits a span, and the current match is told apart from the others.
    #[test]
    fn splits_at_find_matches_and_flags_the_current_one() {
        let spans = [Span::plain("one two two")];
        let marks =
            [SegmentMark { start: 4, end: 7, current: false }, SegmentMark { start: 8, end: 11, current: true }];
        let segments = segment_line(&spans, &[], &marks);
        let shape: Vec<(&str, bool, bool)> = segments.iter().map(|s| (s.text.as_str(), s.mark, s.current)).collect();
        assert_eq!(shape, [("one ", false, false), ("two", true, false), (" ", false, false), ("two", true, true)]);
    }
}
