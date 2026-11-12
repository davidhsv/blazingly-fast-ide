//! Text comparison as IntelliJ IDEA's diff viewer computes it (`ComparisonManagerImpl.compareLinesInner`
//! and `processBlocks`; Apache-2.0, Copyright JetBrains s.r.o. and contributors): lines are compared,
//! each changed block is compared by words and split into line blocks, then adjoining blocks are
//! squashed back and, when ignoring whitespace and empty lines, whitespace-only blocks trimmed.
//!
//! Texts are UTF-16 code units, and every offset and length counts code units.

mod by_char;
mod by_line;
pub mod by_word;
pub(crate) mod chunk_optimizer;
pub(crate) mod iterables;
pub mod lcs;
pub mod moves;
pub mod text;

pub use by_word::compare_words;
pub use lcs::{DiffTooBig, Range};
pub use text::ComparisonPolicy;

use by_line::compare_lines;
use by_word::compare_and_split;
use text::{NEWLINE, equals_ignore_whitespaces, is_equal_texts, slice};

/// A changed block of lines: lines `[start_line1, end_line1)` of the first text against lines
/// `[start_line2, end_line2)` of the second one, numbered from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineFragment {
    pub start_line1: usize,
    pub end_line1: usize,
    pub start_line2: usize,
    pub end_line2: usize,
    /// Text offsets; the end includes the last line's line break, when it has one.
    pub start_offset1: usize,
    pub end_offset1: usize,
    pub start_offset2: usize,
    pub end_offset2: usize,
    /// Changed ranges inside the fragment, relative to its start offsets; `None` when not computed
    /// (by-line highlighting, or a block too big to compare).
    pub inner: Option<Vec<Range>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompareOptions {
    pub policy: ComparisonPolicy,
    /// Compare changed blocks by words ("Highlight words").
    pub words: bool,
    /// Drop leading / trailing whitespace-only blocks ("Ignore whitespaces and empty lines").
    pub trim_chunks: bool,
    /// Keep the line blocks words split a change into ("Highlight split changes").
    pub split: bool,
}

/// Do not compare by words after that many blocks were too big (DiffConfig.MAX_BAD_LINES).
const MAX_BAD_LINES: usize = 3;

/// Where the lines of a text start; lines break at `\n` only and a text always has one line.
struct LineOffsets<'a> {
    text: &'a [u16],
    starts: Vec<usize>,
}

impl<'a> LineOffsets<'a> {
    fn new(text: &'a [u16]) -> Self {
        let breaks = text.iter().enumerate().filter(|(_, unit)| **unit == NEWLINE).map(|(index, _)| index + 1);
        Self { text, starts: std::iter::once(0).chain(breaks).collect() }
    }

    fn line_count(&self) -> usize {
        self.starts.len()
    }

    fn line_start(&self, line: usize) -> usize {
        self.starts[line]
    }

    fn line_end(&self, line: usize, include_newline: bool) -> usize {
        match self.starts.get(line + 1) {
            Some(next) if include_newline => *next,
            Some(next) => next - 1,
            None => self.text.len(),
        }
    }

    /// The text of every line, without its line break.
    fn lines(&self) -> Vec<&'a [u16]> {
        let text = self.text;
        (0..self.line_count()).map(|line| &text[self.line_start(line)..self.line_end(line, false)]).collect()
    }

    /// Offsets of lines `[start, end)` (ComparisonManagerImpl.getOffsets).
    fn offsets(&self, start: usize, end: usize) -> (usize, usize) {
        if start == end {
            let last = self.line_count() - 1;
            let offset = if start <= last { self.line_start(start) } else { self.line_end(last, true) };
            return (offset, offset);
        }
        (self.line_start(start), self.line_end(end - 1, true))
    }
}

/// Line fragments of two texts with their inner word changes, as IDEA's diff viewer computes them.
pub fn compare_texts(text1: &[u16], text2: &[u16], options: CompareOptions) -> Result<Vec<LineFragment>, DiffTooBig> {
    let policy = options.policy;
    let fragments = if options.words {
        compare_lines_inner(text1, text2, policy)?
    } else {
        compare_line_fragments(text1, text2, policy)?
    };
    Ok(process_blocks(fragments, text1, text2, policy, !options.split, options.trim_chunks))
}

/// Changed lines (ComparisonManagerImpl.compareLines).
pub fn compare_line_fragments(
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
) -> Result<Vec<LineFragment>, DiffTooBig> {
    let offsets1 = LineOffsets::new(text1);
    let offsets2 = LineOffsets::new(text2);
    let changes = compare_lines(&offsets1.lines(), &offsets2.lines(), policy)?.changes;
    let fragment = |r: Range| {
        let (start_offset1, end_offset1) = offsets1.offsets(r.start1, r.end1);
        let (start_offset2, end_offset2) = offsets2.offsets(r.start2, r.end2);
        LineFragment {
            start_line1: r.start1,
            end_line1: r.end1,
            start_line2: r.start2,
            end_line2: r.end2,
            start_offset1,
            end_offset1,
            start_offset2,
            end_offset2,
            inner: None,
        }
    };
    Ok(changes.into_iter().map(fragment).collect())
}

/// Changed lines split into line blocks with their word changes (ComparisonManagerImpl.compareLinesInner).
pub fn compare_lines_inner(
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
) -> Result<Vec<LineFragment>, DiffTooBig> {
    Ok(inner_fragments(compare_line_fragments(text1, text2, policy)?, text1, text2, policy))
}

fn inner_fragments(
    fragments: Vec<LineFragment>,
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
) -> Vec<LineFragment> {
    let mut out = Vec::new();
    let mut too_big = 0;
    for mut f in fragments {
        let sub1 = slice(text1, f.start_offset1, f.end_offset1);
        let sub2 = slice(text2, f.start_offset2, f.end_offset2);
        if f.start_line1 == f.end_line1 || f.start_line2 == f.end_line2 {
            // An insertion or a deletion.
            if is_equal_texts(sub1, sub2, policy) {
                f.inner = Some(Vec::new());
            }
            out.push(f);
            continue;
        }
        if too_big >= MAX_BAD_LINES {
            out.push(f);
            continue;
        }
        let Ok(blocks) = compare_and_split(sub1, sub2, policy) else {
            out.push(f);
            too_big += 1;
            continue;
        };
        let count = blocks.len();
        let mut line1 = f.start_line1;
        let mut line2 = f.start_line2;
        for (i, block) in blocks.into_iter().enumerate() {
            // The last block ends with the fragment, whatever its last line.
            let last = i + 1 == count;
            let end_line1 = if last { f.end_line1 } else { line1 + block.newlines1 };
            let end_line2 = if last { f.end_line2 } else { line2 + block.newlines2 };
            out.push(LineFragment {
                start_line1: line1,
                end_line1,
                start_line2: line2,
                end_line2,
                start_offset1: f.start_offset1 + block.offsets.start1,
                end_offset1: f.start_offset1 + block.offsets.end1,
                start_offset2: f.start_offset2 + block.offsets.start2,
                end_offset2: f.start_offset2 + block.offsets.end2,
                inner: Some(block.fragments),
            });
            line1 = end_line1;
            line2 = end_line2;
        }
    }
    out
}

fn adjoining(before: &LineFragment, after: &LineFragment) -> bool {
    before.end_line1 == after.start_line1
        && before.end_line2 == after.start_line2
        && before.end_offset1 == after.start_offset1
        && before.end_offset2 == after.start_offset2
}

/// Squashes adjoining fragments and trims whitespace-only ones at their edges (processBlocks).
pub fn process_blocks(
    fragments: Vec<LineFragment>,
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
    squash: bool,
    trim: bool,
) -> Vec<LineFragment> {
    if (!squash && !trim) || fragments.is_empty() {
        return fragments;
    }
    let mut out = Vec::new();
    let mut run: Vec<LineFragment> = Vec::new();
    for fragment in fragments {
        if run.last().is_some_and(|last| !adjoining(last, &fragment)) {
            out.extend(process_adjoining(std::mem::take(&mut run), text1, text2, policy, squash, trim));
        }
        run.push(fragment);
    }
    out.extend(process_adjoining(run, text1, text2, policy, squash, trim));
    out
}

fn process_adjoining(
    mut fragments: Vec<LineFragment>,
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
    squash: bool,
    trim: bool,
) -> Vec<LineFragment> {
    let mut start = 0;
    let mut end = fragments.len();
    if trim && policy == ComparisonPolicy::Ignore {
        let ignorable = |f: &LineFragment| {
            f.inner.as_ref().is_some_and(|inner| inner.is_empty())
                || equals_ignore_whitespaces(
                    slice(text1, f.start_offset1, f.end_offset1),
                    slice(text2, f.start_offset2, f.end_offset2),
                )
        };
        while start < end && ignorable(&fragments[start]) {
            start += 1;
        }
        while start < end && ignorable(&fragments[end - 1]) {
            end -= 1;
        }
    }
    if start == end {
        return Vec::new();
    }
    fragments.truncate(end);
    fragments.drain(..start);
    if squash { vec![do_squash(fragments)] } else { fragments }
}

/// One fragment for a run of adjoining ones, its inner changes being theirs with the touching ones
/// joined; a fragment without inner changes counts as changed whole.
fn do_squash(mut fragments: Vec<LineFragment>) -> LineFragment {
    if fragments.len() == 1
        && let Some(only) = fragments.pop()
    {
        return only;
    }
    let first = &fragments[0];
    let last = &fragments[fragments.len() - 1];
    let mut inner: Vec<Range> = Vec::new();
    for f in &fragments {
        let shift1 = f.start_offset1 - first.start_offset1;
        let shift2 = f.start_offset2 - first.start_offset2;
        let whole = [Range::new(0, f.end_offset1 - f.start_offset1, 0, f.end_offset2 - f.start_offset2)];
        for r in f.inner.as_deref().unwrap_or(&whole) {
            let shifted = Range::new(r.start1 + shift1, r.end1 + shift1, r.start2 + shift2, r.end2 + shift2);
            if let Some(previous) = inner.last_mut()
                && previous.end1 == shifted.start1
                && previous.end2 == shifted.start2
            {
                previous.end1 = shifted.end1;
                previous.end2 = shifted.end2;
            } else {
                inner.push(shifted);
            }
        }
    }
    LineFragment {
        start_line1: first.start_line1,
        end_line1: last.end_line1,
        start_line2: first.start_line2,
        end_line2: last.end_line2,
        start_offset1: first.start_offset1,
        end_offset1: last.end_offset1,
        start_offset2: first.start_offset2,
        end_offset2: last.end_offset2,
        inner: Some(inner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::BTreeSet;

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum CaseType {
        Lines,
        LinesInner,
        Words,
        Splitter,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Policy {
        Default,
        Trim,
        Ignore,
    }

    impl Policy {
        fn comparison(self) -> ComparisonPolicy {
            match self {
                Policy::Default => ComparisonPolicy::Default,
                Policy::Trim => ComparisonPolicy::Trim,
                Policy::Ignore => ComparisonPolicy::Ignore,
            }
        }
    }

    /// An expectation per policy; a policy without its own takes that of the stricter one.
    #[derive(Debug, Deserialize)]
    struct PolicyData<T> {
        default: Option<T>,
        trim: Option<T>,
        ignore: Option<T>,
    }

    impl<T> PolicyData<T> {
        /// PolicyData.get: "ignore" falls back to "trim", then "default".
        fn get(&self, policy: Policy) -> Option<&T> {
            match policy {
                Policy::Ignore => self.ignore.as_ref().or(self.trim.as_ref()).or(self.default.as_ref()),
                Policy::Trim => self.trim.as_ref().or(self.default.as_ref()),
                Policy::Default => self.default.as_ref(),
            }
        }
    }

    type Change = [usize; 4];

    /// One case of IntelliJ's own comparison tests (LineComparisonUtilTest, WordComparisonUtilTest,
    /// SplitComparisonUtilTest; Apache-2.0, Copyright JetBrains s.r.o. and contributors).
    #[derive(Debug, Deserialize)]
    struct Case {
        source: String,
        test: String,
        #[serde(rename = "type")]
        kind: CaseType,
        squash: bool,
        trim: bool,
        before: String,
        after: String,
        changes: PolicyData<Vec<Change>>,
        matchings: PolicyData<(String, String)>,
        policies: Vec<Policy>,
    }

    fn cases() -> Vec<Case> {
        serde_json::from_str(include_str!("../../../../src/features/vcs/comparison/__tests__/ideaCases.json"))
            .expect("the fixture parses")
    }

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// Changed character offsets marked by non-space characters, '.' skipped (DiffTestCase.parseMatching).
    fn parse_matching(matching: &str) -> Vec<usize> {
        let marks = matching.chars().filter(|c| *c != '.').enumerate();
        marks.filter(|(_, c)| *c != ' ').map(|(index, _)| index).collect()
    }

    /// Changed lines: lines (split at '_' / '*') with a character other than ' ' and '_'
    /// (parseLineMatching).
    fn parse_line_matching(matching: &str) -> Vec<usize> {
        let chars: Vec<char> = matching.chars().collect();
        let mut out = Vec::new();
        let mut line = 0;
        let mut index = 0;
        while index < chars.len() {
            let separator = chars[index..].iter().position(|c| matches!(c, '_' | '*'));
            let end = separator.map_or(chars.len(), |found| index + found + 1);
            if chars[index..end].iter().any(|c| !matches!(c, ' ' | '_')) {
                out.push(line);
            }
            line += 1;
            index = end;
        }
        out
    }

    /// The indexes the ranges cover on each side, in order.
    fn char_sets(fragments: &[Range]) -> (Vec<usize>, Vec<usize>) {
        let set1: BTreeSet<usize> = fragments.iter().flat_map(|f| f.start1..f.end1).collect();
        let set2: BTreeSet<usize> = fragments.iter().flat_map(|f| f.start2..f.end2).collect();
        (set1.into_iter().collect(), set2.into_iter().collect())
    }

    fn line_ranges(fragments: &[LineFragment]) -> Vec<Range> {
        fragments.iter().map(|f| Range::new(f.start_line1, f.end_line1, f.start_line2, f.end_line2)).collect()
    }

    fn line_changes(fragments: &[LineFragment]) -> Vec<Change> {
        fragments.iter().map(|f| [f.start_line1, f.end_line1, f.start_line2, f.end_line2]).collect()
    }

    fn range_changes(fragments: &[Range]) -> Vec<Change> {
        fragments.iter().map(|f| [f.start1, f.end1, f.start2, f.end2]).collect()
    }

    fn check_words(label: &str, fragments: &[Range], case: &Case, policy: Policy) {
        if let Some(changes) = case.changes.get(policy) {
            assert_eq!(&range_changes(fragments), changes, "{label}");
        }
        if let Some((matching1, matching2)) = case.matchings.get(policy) {
            assert_eq!(char_sets(fragments), (parse_matching(matching1), parse_matching(matching2)), "{label}");
        }
    }

    /// Runs `check` on every case of a type, under each of its policies, with a label naming the case.
    fn run_cases(kind: CaseType, check: impl Fn(&Case, Policy, &str)) {
        let mut ran = 0;
        for (index, case) in cases().iter().enumerate().filter(|(_, case)| case.kind == kind) {
            let mut seen = Vec::new();
            for policy in case.policies.iter().copied() {
                if seen.contains(&policy) {
                    continue;
                }
                seen.push(policy);
                let label = format!(
                    "{}.{} #{index} {kind:?} {:?} - {:?} ({policy:?})",
                    case.source, case.test, case.before, case.after
                );
                assert!(
                    case.changes.get(policy).is_some() || case.matchings.get(policy).is_some(),
                    "{label}: the case has expectations"
                );
                check(case, policy, &label);
                ran += 1;
            }
        }
        assert!(ran > 0, "the fixture has {kind:?} cases");
    }

    // src/features/vcs/comparison/__tests__/idea.test.ts › IntelliJ comparison tests › lines
    #[test]
    fn intellij_comparison_tests_lines() {
        run_cases(CaseType::Lines, |case, policy, label| {
            let (before, after) = (units(&case.before), units(&case.after));
            let fragments = compare_line_fragments(&before, &after, policy.comparison())
                .unwrap_or_else(|error| panic!("{label}: {error}"));
            if let Some(changes) = case.changes.get(policy) {
                assert_eq!(&line_changes(&fragments), changes, "{label}");
            }
            if let Some((matching1, matching2)) = case.matchings.get(policy) {
                let expected = (parse_line_matching(matching1), parse_line_matching(matching2));
                assert_eq!(char_sets(&line_ranges(&fragments)), expected, "{label}");
            }
        });
    }

    // src/features/vcs/comparison/__tests__/idea.test.ts › IntelliJ comparison tests › lines_inner
    #[test]
    fn intellij_comparison_tests_lines_inner() {
        run_cases(CaseType::LinesInner, |case, policy, label| {
            let (before, after) = (units(&case.before), units(&case.after));
            let comparison = policy.comparison();
            let inner =
                compare_lines_inner(&before, &after, comparison).unwrap_or_else(|error| panic!("{label}: {error}"));
            let fragments = process_blocks(inner, &before, &after, comparison, true, false);
            assert_eq!(fragments.len(), 1, "{label}");
            let fragment = &fragments[0];
            assert_eq!(
                [fragment.start_offset1, fragment.end_offset1, fragment.start_offset2, fragment.end_offset2],
                [0, before.len(), 0, after.len()],
                "{label}"
            );
            let inner = fragment.inner.as_deref().unwrap_or_else(|| panic!("{label}: the block has word changes"));
            check_words(label, inner, case, policy);
            let words = compare_words(&before, &after, comparison).unwrap_or_else(|error| panic!("{label}: {error}"));
            check_words(label, &words, case, policy);
        });
    }

    // src/features/vcs/comparison/__tests__/idea.test.ts › IntelliJ comparison tests › words
    #[test]
    fn intellij_comparison_tests_words() {
        run_cases(CaseType::Words, |case, policy, label| {
            let (before, after) = (units(&case.before), units(&case.after));
            let words =
                compare_words(&before, &after, policy.comparison()).unwrap_or_else(|error| panic!("{label}: {error}"));
            check_words(label, &words, case, policy);
        });
    }

    // src/features/vcs/comparison/__tests__/idea.test.ts › IntelliJ comparison tests › splitter
    #[test]
    fn intellij_comparison_tests_splitter() {
        run_cases(CaseType::Splitter, |case, policy, label| {
            let (before, after) = (units(&case.before), units(&case.after));
            let comparison = policy.comparison();
            let inner =
                compare_lines_inner(&before, &after, comparison).unwrap_or_else(|error| panic!("{label}: {error}"));
            let fragments = process_blocks(inner, &before, &after, comparison, case.squash, case.trim);
            let changes = case.changes.get(policy).unwrap_or_else(|| panic!("{label}: the case lists its changes"));
            assert_eq!(&line_changes(&fragments), changes, "{label}");
        });
    }

    // Not from a spec: a changed line comes with its offsets, line break included, and its word changes
    // when words are compared.
    #[test]
    fn reports_a_changed_line_with_its_offsets_and_word_changes() {
        let (before, after) = (units("a\nb\nc"), units("a\nx\nc"));
        let lines = LineFragment {
            start_line1: 1,
            end_line1: 2,
            start_line2: 1,
            end_line2: 2,
            start_offset1: 2,
            end_offset1: 4,
            start_offset2: 2,
            end_offset2: 4,
            inner: None,
        };
        let by_lines =
            CompareOptions { policy: ComparisonPolicy::Default, words: false, trim_chunks: false, split: false };
        assert_eq!(compare_texts(&before, &after, by_lines), Ok(vec![lines.clone()]));
        let words = LineFragment { inner: Some(vec![Range::new(0, 1, 0, 1)]), ..lines };
        let by_words = CompareOptions { words: true, ..by_lines };
        assert_eq!(compare_texts(&before, &after, by_words), Ok(vec![words]));
    }

    // Not from a spec: an insertion after the last line sits at the end of the text, and equal texts
    // have no fragments.
    #[test]
    fn places_an_insertion_after_the_last_line() {
        let (before, after) = (units("a"), units("a\nb"));
        let fragments = compare_line_fragments(&before, &after, ComparisonPolicy::Default).expect("texts compare");
        assert_eq!(line_changes(&fragments), [[1, 1, 1, 2]]);
        assert_eq!((fragments[0].start_offset1, fragments[0].end_offset1), (1, 1));
        assert_eq!((fragments[0].start_offset2, fragments[0].end_offset2), (2, 3));
        assert_eq!(compare_line_fragments(&before, &before, ComparisonPolicy::Trim), Ok(Vec::new()));
    }
}
