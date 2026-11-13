use super::*;
use crate::run::ansi::AnsiColor;

fn plain(text: &str) -> Segment<'static> {
    Segment { text: text.into(), style: SpanStyle::default(), link: None, mark: false, current: false }
}

// src/features/run/segments.test.ts › segmentLine marks › splits a line around matches
/// Two find matches split a plain line into the matches and the text between them.
#[test]
fn splits_a_line_around_matches() {
    let spans = [Span::plain("hello world")];
    let marks = [SegmentMark { start: 0, end: 5, current: false }, SegmentMark { start: 6, end: 11, current: false }];
    let marked = |text: &str| Segment { mark: true, ..plain(text) };
    assert_eq!(segment_line(&spans, &[], &marks), [marked("hello"), plain(" "), marked("world")]);
}

// src/features/run/segments.test.ts › segmentLine marks › marks the current match and keeps its span style
/// The current match keeps the colour of the span it lies in.
#[test]
fn marks_the_current_match_and_keeps_its_span_style() {
    let red = SpanStyle { fg: Some(AnsiColor::Named(1)), ..SpanStyle::default() };
    let spans = [Span { text: "red".into(), style: red }];
    let marks = [SegmentMark { start: 0, end: 3, current: true }];
    let expected = Segment { text: "red".into(), style: red, link: None, mark: true, current: true };
    assert_eq!(segment_line(&spans, &[], &marks), [expected]);
}

// src/features/run/segments.test.ts › segmentLine marks › splits matches inside links
/// A match inside a link splits the link into a plain part and a marked part, both linked.
#[test]
fn splits_matches_inside_links() {
    let links = [OutputLink::Url { start: 4, end: 12, url: "http://x".into() }];
    let spans = [Span::plain("see http://x")];
    let marks = [SegmentMark { start: 8, end: 12, current: false }];
    let segments = segment_line(&spans, &links, &marks);
    let linked = |text: &str, mark: bool| Segment { link: Some(&links[0]), mark, ..plain(text) };
    assert_eq!(segments, [plain("see "), linked("http", false), linked("://x", true)]);
}
