//! Rename: the occurrences an in-place rename edits together.

/// An occurrence (UTF-16 offsets).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: u32,
    pub to: u32,
}

/// The occurrences other than `primary` that `accept` keeps, in order, without overlaps.
pub fn linked_spans(spans: &[Span], primary: Span, accept: impl Fn(&Span) -> bool) -> Vec<Span> {
    let mut sorted = spans.to_vec();
    sorted.sort_by_key(|span| span.from);
    let mut out: Vec<Span> = Vec::new();
    for span in sorted {
        if span.to > primary.from && span.from < primary.to {
            continue;
        }
        if span.from >= span.to || !accept(&span) {
            continue;
        }
        if out.last().is_some_and(|last| span.from < last.to) {
            continue;
        }
        out.push(span);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(from: u32, to: u32) -> Span {
        Span { from, to }
    }

    #[test]
    fn keeps_other_same_name_occurrences_sorted_and_without_the_primary_or_overlaps() {
        let spans = [span(20, 23), span(4, 7), span(10, 13), span(11, 14), span(30, 35)];
        assert_eq!(linked_spans(&spans, span(4, 7), |s| s.to - s.from == 3), [span(10, 13), span(20, 23)]);
    }
}
