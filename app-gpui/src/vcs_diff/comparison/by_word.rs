//! Word-level comparison of a changed block, as the IntelliJ Platform does it
//! (`ByWordRt.compareAndSplit` with `LineFragmentSplitter`; Apache-2.0, Copyright JetBrains s.r.o. and
//! contributors): words and line breaks are diffed, the block is split into "logically different" line
//! blocks, the punctuation between matched words is matched by character, and whitespace by the policy.

use super::by_char::compare_punctuation;
use super::chunk_optimizer::optimize_word_chunks;
use super::iterables::{ChangeBuilder, DiffIterable, diff_sequences, sub_iterable, unchanged};
use super::lcs::{DiffTooBig, Range};
use super::text::{
    ComparisonPolicy, NEWLINE, char_count, code_point_at, expand_whitespaces, expand_whitespaces_backward,
    expand_whitespaces_forward, is_alpha, is_continuous_script, is_equals_ignore_whitespaces_range, is_equals_range,
    is_space, slice, trim_end, trim_range, trim_start,
};

/// A word or a line break of a text, at `[offset1, offset2)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InlineChunk<'a> {
    pub(crate) offset1: usize,
    pub(crate) offset2: usize,
    pub(crate) newline: bool,
    /// Equal keys are equal chunks: the word itself, or the line break.
    pub(crate) key: &'a [u16],
}

/// The words of a text (ByWordRt.getInlineChunks): runs of letters and digits (anything but ASCII
/// whitespace and punctuation), single characters of the scripts written without spaces, and line
/// breaks. Punctuation is matched separately.
pub(crate) fn get_inline_chunks(text: &[u16]) -> Vec<InlineChunk<'_>> {
    let mut chunks = Vec::new();
    let mut word_start: Option<usize> = None;
    let mut offset = 0;
    while offset < text.len() {
        let ch = code_point_at(text, offset);
        let count = char_count(ch);
        let alpha = is_alpha(ch);
        if alpha && !is_continuous_script(ch) {
            if word_start.is_none() {
                word_start = Some(offset);
            }
        } else {
            if let Some(start) = word_start.take() {
                chunks.push(chunk(text, start, offset, false));
            }
            if alpha {
                chunks.push(chunk(text, offset, offset + count, false));
            } else if ch == u32::from(NEWLINE) {
                chunks.push(chunk(text, offset, offset + 1, true));
            }
        }
        offset += count;
    }
    if let Some(start) = word_start {
        chunks.push(chunk(text, start, text.len(), false));
    }
    chunks
}

fn chunk(text: &[u16], from: usize, to: usize, newline: bool) -> InlineChunk<'_> {
    InlineChunk { offset1: from, offset2: to, newline, key: slice(text, from, to) }
}

/// The changes between the words of two texts, moved to where a reader expects them.
fn compare_chunks(
    words1: &[InlineChunk<'_>],
    words2: &[InlineChunk<'_>],
    text1: &[u16],
    text2: &[u16],
) -> Result<DiffIterable, DiffTooBig> {
    let keys1: Vec<&[u16]> = words1.iter().map(|word| word.key).collect();
    let keys2: Vec<&[u16]> = words2.iter().map(|word| word.key).collect();
    let changes = diff_sequences(&keys1, &keys2)?;
    Ok(optimize_word_chunks(words1, words2, text1, text2, &changes))
}

/// Word changes of two texts (ByWordRt.compare).
pub fn compare_words(text1: &[u16], text2: &[u16], policy: ComparisonPolicy) -> Result<Vec<Range>, DiffTooBig> {
    let words1 = get_inline_chunks(text1);
    let words2 = get_inline_chunks(text2);
    let word_changes = compare_chunks(&words1, &words2, text1, text2)?;
    let side1 = BlockSide { text: text1, words: &words1, shift: 0 };
    let side2 = BlockSide { text: text2, words: &words2, shift: 0 };
    let delimiters = match_punctuation(side1, side2, &word_changes)?;
    Ok(match_whitespaces(text1, text2, &delimiters, policy).changes)
}

/// A line block of a changed block, with its word changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineBlock {
    /// Changed ranges of the block's texts, relative to `offsets`.
    pub(crate) fragments: Vec<Range>,
    pub(crate) offsets: Range,
    pub(crate) newlines1: usize,
    pub(crate) newlines2: usize,
}

/// Word changes of a changed line block, split into line blocks (ByWordRt.compareAndSplit).
pub(crate) fn compare_and_split(
    text1: &[u16],
    text2: &[u16],
    policy: ComparisonPolicy,
) -> Result<Vec<LineBlock>, DiffTooBig> {
    let words1 = get_inline_chunks(text1);
    let words2 = get_inline_chunks(text2);
    let word_changes = compare_chunks(&words1, &words2, text1, text2)?;
    let word_blocks = split_line_fragments(text1, text2, &words1, &words2, &word_changes);

    let changed = word_changes.changes.as_slice();
    let mut index = 0;
    let mut blocks = Vec::with_capacity(word_blocks.len());
    for WordBlock { words, offsets } in word_blocks {
        while let Some(r) = changed.get(index)
            && (r.end1 < words.start1 || r.end2 < words.start2)
        {
            index += 1;
        }
        let sub = sub_iterable(changed, words, index);
        let subtext1 = slice(text1, offsets.start1, offsets.end1);
        let subtext2 = slice(text2, offsets.start2, offsets.end2);
        let subwords1 = part(&words1, words.start1, words.end1);
        let subwords2 = part(&words2, words.start2, words.end2);
        let side1 = BlockSide { text: subtext1, words: subwords1, shift: offsets.start1 };
        let side2 = BlockSide { text: subtext2, words: subwords2, shift: offsets.start2 };
        let delimiters = match_punctuation(side1, side2, &sub)?;
        blocks.push(LineBlock {
            fragments: match_whitespaces(subtext1, subtext2, &delimiters, policy).changes,
            offsets,
            newlines1: subwords1.iter().filter(|word| word.newline).count(),
            newlines2: subwords2.iter().filter(|word| word.newline).count(),
        });
    }
    Ok(blocks)
}

/// The items in `[from, to)`; nothing when the span is not inside the slice.
fn part<T>(items: &[T], from: usize, to: usize) -> &[T] {
    items.get(from..to).unwrap_or_default()
}

/// A block of a changed block: its words (as indexes) and its text (as offsets), on both sides.
#[derive(Clone, Copy)]
struct WordBlock {
    words: Range,
    offsets: Range,
}

#[derive(Clone, Copy)]
struct PendingChunk {
    block: WordBlock,
    has_equal_words: bool,
    has_words_inside: bool,
    is_equal_ignore_whitespaces: bool,
}

/// Splits a changed block at the matched line breaks and at the matched first words of lines
/// (LineFragmentSplitter).
fn split_line_fragments(
    text1: &[u16],
    text2: &[u16],
    words1: &[InlineChunk<'_>],
    words2: &[InlineChunk<'_>],
    iterable: &DiffIterable,
) -> Vec<WordBlock> {
    let mut splitter =
        LineSplitter { text1, text2, words1, words2, result: Vec::new(), last1: -1, last2: -1, pending: None };
    let mut has_equal_words = false;
    for r in unchanged(iterable) {
        for (index1, index2) in r.pairs() {
            if words1[index1].newline && words2[index2].newline {
                // Split at the matched line breaks.
                splitter.add_line_chunk(index1 as isize, index2 as isize, has_equal_words);
                has_equal_words = false;
            } else {
                if first_in_line(words1, index1) && first_in_line(words2, index2) {
                    // Split at the matched first words of the lines.
                    splitter.add_line_chunk(index1 as isize - 1, index2 as isize - 1, has_equal_words);
                }
                has_equal_words = true;
            }
        }
    }
    splitter.add_line_chunk(words1.len() as isize, words2.len() as isize, has_equal_words);
    splitter.result.extend(splitter.pending.map(|pending| pending.block));
    splitter.result
}

fn first_in_line(words: &[InlineChunk<'_>], index: usize) -> bool {
    index == 0 || words[index - 1].newline
}

/// Where the text up to word `index` ends: -1 stands for the start of the text and `words.len()` for
/// its end.
fn word_end(words: &[InlineChunk<'_>], text: &[u16], index: isize) -> usize {
    if index < 0 { 0 } else { words.get(index as usize).map_or(text.len(), |word| word.offset2) }
}

struct LineSplitter<'a> {
    text1: &'a [u16],
    text2: &'a [u16],
    words1: &'a [InlineChunk<'a>],
    words2: &'a [InlineChunk<'a>],
    result: Vec<WordBlock>,
    /// The word the last block ends with on each side; -1 before the first block.
    last1: isize,
    last2: isize,
    /// The block that later ones may still merge into.
    pending: Option<PendingChunk>,
}

impl LineSplitter<'_> {
    /// Ends a block with words `end1` and `end2`.
    fn add_line_chunk(&mut self, end1: isize, end2: isize, has_equal_words: bool) {
        if self.last1 > end1 || self.last2 > end2 {
            return;
        }
        let chunk = self.create_chunk(end1, end2, has_equal_words);
        if chunk.block.offsets.is_empty() {
            return;
        }
        self.pending = Some(match self.pending {
            Some(pending) if should_merge(&pending, &chunk) => merge(&pending, &chunk),
            pending => {
                self.result.extend(pending.map(|pending| pending.block));
                chunk
            }
        });
        self.last1 = end1;
        self.last2 = end2;
    }

    /// The block from the words after the last block's end to words `end1` and `end2`.
    fn create_chunk(&self, end1: isize, end2: isize, has_equal_words: bool) -> PendingChunk {
        let offsets = Range::new(
            word_end(self.words1, self.text1, self.last1),
            word_end(self.words1, self.text1, end1),
            word_end(self.words2, self.text2, self.last2),
            word_end(self.words2, self.text2, end2),
        );
        let words = Range::new(
            (self.last1 + 1).max(0) as usize,
            ((end1 + 1).max(0) as usize).min(self.words1.len()),
            (self.last2 + 1).max(0) as usize,
            ((end2 + 1).max(0) as usize).min(self.words2.len()),
        );
        let has_words_inside = part(self.words1, words.start1, words.end1).iter().any(|word| !word.newline)
            || part(self.words2, words.start2, words.end2).iter().any(|word| !word.newline);
        PendingChunk {
            block: WordBlock { words, offsets },
            has_equal_words,
            has_words_inside,
            is_equal_ignore_whitespaces: is_equals_ignore_whitespaces_range(self.text1, self.text2, offsets),
        }
    }
}

fn should_merge(chunk1: &PendingChunk, chunk2: &PendingChunk) -> bool {
    // Lines matched only by their line breaks.
    (!chunk1.has_equal_words && !chunk2.has_equal_words)
        // Lines whose changes are whitespace only.
        || (chunk1.is_equal_ignore_whitespaces && chunk2.is_equal_ignore_whitespaces)
        // A block without words in it.
        || !chunk1.has_words_inside
        || !chunk2.has_words_inside
}

fn merge(chunk1: &PendingChunk, chunk2: &PendingChunk) -> PendingChunk {
    let (words1, words2) = (chunk1.block.words, chunk2.block.words);
    let (offsets1, offsets2) = (chunk1.block.offsets, chunk2.block.offsets);
    PendingChunk {
        block: WordBlock {
            words: Range::new(words1.start1, words2.end1, words1.start2, words2.end2),
            offsets: Range::new(offsets1.start1, offsets2.end1, offsets1.start2, offsets2.end2),
        },
        has_equal_words: chunk1.has_equal_words || chunk2.has_equal_words,
        has_words_inside: chunk1.has_words_inside || chunk2.has_words_inside,
        is_equal_ignore_whitespaces: chunk1.is_equal_ignore_whitespaces && chunk2.is_equal_ignore_whitespaces,
    }
}

/// One side of a block being compared: its text, its words, and where the text starts in the one the
/// words' offsets count from.
#[derive(Clone, Copy)]
struct BlockSide<'a> {
    text: &'a [u16],
    words: &'a [InlineChunk<'a>],
    shift: usize,
}

impl BlockSide<'_> {
    fn start_offset(&self, index: usize) -> usize {
        self.words[index].offset1 - self.shift
    }

    fn end_offset(&self, index: usize) -> usize {
        self.words[index].offset2 - self.shift
    }

    /// The text between word `index - 1` and word `index`, the ends of the text standing for the words
    /// that are not there.
    fn gap_before(&self, index: usize) -> (usize, usize) {
        let start = if index == 0 { 0 } else { self.end_offset(index - 1) };
        let end = if index == self.words.len() { self.text.len() } else { self.start_offset(index) };
        (start, end)
    }
}

/// Matches the punctuation between words (AdjustmentPunctuationMatcher): the separators around each
/// pair of matched words are compared by character, so "[ X { A ! B } Y ]" against "( X ... Y )"
/// compares "[" with "(", "{" + "}" with "..." and "]" with ")".
fn match_punctuation(
    side1: BlockSide<'_>,
    side2: BlockSide<'_>,
    changes: &DiffIterable,
) -> Result<DiffIterable, DiffTooBig> {
    let builder = ChangeBuilder::new(side1.text.len(), side2.text.len());
    let mut matcher = PunctuationMatcher { side1, side2, builder };
    // The separators after the last pair of matched words.
    let mut last = matcher.gaps_before(0, 0);
    for r in unchanged(changes) {
        for (index1, index2) in r.pairs() {
            matcher.match_backward(last, index1, index2)?;
            matcher.builder.mark_equal_range(
                side1.start_offset(index1),
                side2.start_offset(index2),
                side1.end_offset(index1),
                side2.end_offset(index2),
            );
            last = matcher.gaps_before(index1 + 1, index2 + 1);
        }
    }
    matcher.match_backward(last, side1.words.len(), side2.words.len())?;
    Ok(matcher.builder.finish())
}

struct PunctuationMatcher<'a> {
    side1: BlockSide<'a>,
    side2: BlockSide<'a>,
    builder: ChangeBuilder<'a>,
}

impl PunctuationMatcher<'_> {
    /// The separators before words `index1` and `index2`.
    fn gaps_before(&self, index1: usize, index2: usize) -> Range {
        let (start1, end1) = self.side1.gap_before(index1);
        let (start2, end2) = self.side2.gap_before(index2);
        Range::new(start1, end1, start2, end2)
    }

    /// Matches the separators between the last pair of matched words, which `last` follows, and the
    /// pair `index1` and `index2`.
    fn match_backward(&mut self, last: Range, index1: usize, index2: usize) -> Result<(), DiffTooBig> {
        let current = self.gaps_before(index1, index2);
        if last.start1 == current.start1 && last.start2 == current.start2 {
            // Adjacent matched words: match the gap between them ("A B" - "A B").
            self.match_range(current)
        } else if last.start1 < current.start1 && last.start2 < current.start2 {
            // Matched words with unmatched ones between ("A X B" - "A Y B").
            self.match_range(last)?;
            self.match_range(current)
        } else {
            // One side adjacent, the other with unmatched words between ("A B" - "A Y B").
            self.match_complex_range(last, current)
        }
    }

    fn match_range(&mut self, r: Range) -> Result<(), DiffTooBig> {
        if r.is_empty() {
            return Ok(());
        }
        let text1 = slice(self.side1.text, r.start1, r.end1);
        let text2 = slice(self.side2.text, r.start2, r.end2);
        for equal in unchanged(&compare_punctuation(text1, text2)?) {
            self.builder.mark_equal_range(
                r.start1 + equal.start1,
                r.start2 + equal.start2,
                r.start1 + equal.end1,
                r.start2 + equal.end2,
            );
        }
        Ok(())
    }

    /// Matches a separator of one side with the two separators the other side has around its unmatched
    /// words.
    fn match_complex_range(&mut self, last: Range, current: Range) -> Result<(), DiffTooBig> {
        let (text1, text2) = (self.side1.text, self.side2.text);
        let shared1 = last.start1 == current.start1 && last.end1 == current.end1;
        let shared2 = last.start2 == current.start2 && last.end2 == current.end2;
        debug_assert!(shared1 || shared2, "one side has a single separator between the matched words");
        if shared1 {
            let (first, second) = compare_punctuation_2_side(
                slice(text1, last.start1, last.end1),
                slice(text2, last.start2, last.end2),
                slice(text2, current.start2, current.end2),
            )?;
            for r in first {
                let (start1, start2) = (last.start1, last.start2);
                self.builder.mark_equal_range(start1 + r.start1, start2 + r.start2, start1 + r.end1, start2 + r.end2);
            }
            for r in second {
                let (start1, start2) = (last.start1, current.start2);
                self.builder.mark_equal_range(start1 + r.start1, start2 + r.start2, start1 + r.end1, start2 + r.end2);
            }
        } else if shared2 {
            // Mirrored: two separators of the first side against one of the second side.
            let (first, second) = compare_punctuation_2_side(
                slice(text2, last.start2, last.end2),
                slice(text1, last.start1, last.end1),
                slice(text1, current.start1, current.end1),
            )?;
            for r in first {
                let (start1, start2) = (last.start1, last.start2);
                self.builder.mark_equal_range(start1 + r.start2, start2 + r.start1, start1 + r.end2, start2 + r.end1);
            }
            for r in second {
                let (start1, start2) = (current.start1, last.start2);
                self.builder.mark_equal_range(start1 + r.start2, start2 + r.start1, start1 + r.end2, start2 + r.end1);
            }
        }
        Ok(())
    }
}

/// Compares the punctuation of one text with that of two others as if they were one
/// (comparePunctuation2Side): the equal ranges against the first one, and those against the second.
fn compare_punctuation_2_side(
    text: &[u16],
    text21: &[u16],
    text22: &[u16],
) -> Result<(Vec<Range>, Vec<Range>), DiffTooBig> {
    let joined = [text21, text22].concat();
    let changes = compare_punctuation(text, &joined)?;
    let split = text21.len();
    let mut ranges1 = Vec::new();
    let mut ranges2 = Vec::new();
    for r in unchanged(&changes) {
        if r.end2 <= split {
            ranges1.push(r);
        } else if r.start2 >= split {
            ranges2.push(Range::new(r.start1, r.end1, r.start2 - split, r.end2 - split));
        } else {
            let len = split - r.start2;
            ranges1.push(Range::new(r.start1, r.start1 + len, r.start2, split));
            ranges2.push(Range::new(r.start1 + len, r.end1, 0, r.end2 - split));
        }
    }
    Ok((ranges1, ranges2))
}

/// Whitespace handling of the changes by policy (ByWordRt.matchAdjustmentWhitespaces).
fn match_whitespaces(text1: &[u16], text2: &[u16], iterable: &DiffIterable, policy: ComparisonPolicy) -> DiffIterable {
    match policy {
        ComparisonPolicy::Ignore => ignore_spaces(iterable, text1, text2),
        ComparisonPolicy::Default => default_spaces(iterable, text1, text2),
        ComparisonPolicy::Trim => trim_spaces(&default_spaces(iterable, text1, text2), text1, text2),
    }
}

/// Equal whitespace at the edges of a change is unchanged (DefaultCorrector).
fn default_spaces(iterable: &DiffIterable, text1: &[u16], text2: &[u16]) -> DiffIterable {
    let mut changes = Vec::new();
    for r in &iterable.changes {
        let end_cut = expand_whitespaces_backward(text1, text2, *r);
        let cut = Range::new(r.start1, r.end1 - end_cut, r.start2, r.end2 - end_cut);
        let start_cut = expand_whitespaces_forward(text1, text2, cut);
        let expanded = Range::new(cut.start1 + start_cut, cut.end1, cut.start2 + start_cut, cut.end2);
        if !expanded.is_empty() {
            changes.push(expanded);
        }
    }
    DiffIterable::new(changes, text1.len(), text2.len())
}

/// Whitespace is matched where possible and ignored where not (IgnoreSpacesCorrector).
fn ignore_spaces(iterable: &DiffIterable, text1: &[u16], text2: &[u16]) -> DiffIterable {
    let mut changes = Vec::new();
    for r in &iterable.changes {
        let trimmed = trim_range(text1, text2, expand_whitespaces(text1, text2, *r));
        if !trimmed.is_empty() && !is_equals_ignore_whitespaces_range(text1, text2, trimmed) {
            changes.push(trimmed);
        }
    }
    DiffIterable::new(changes, text1.len(), text2.len())
}

/// Whether the whitespace at `start` is at the start of its line.
fn is_leading_space(text: &[u16], start: usize) -> bool {
    if text.get(start).is_none_or(|unit| !is_space(*unit)) {
        return false;
    }
    for unit in text[..start].iter().rev() {
        if *unit == NEWLINE {
            return true;
        }
        if !is_space(*unit) {
            return false;
        }
    }
    true
}

/// Whether the whitespace at `end` runs to the end of its line; the last line has no trailing space
/// unless the text ends with it.
fn is_trailing_space(text: &[u16], end: usize) -> bool {
    if text.get(end).is_none_or(|unit| !is_space(*unit)) {
        return false;
    }
    for unit in &text[end..] {
        if *unit == NEWLINE {
            return true;
        }
        if !is_space(*unit) {
            return false;
        }
    }
    true
}

fn is_leading_trailing_space(text: &[u16], index: usize) -> bool {
    is_leading_space(text, index) || is_trailing_space(text, index)
}

/// Leading and trailing whitespace of lines is not a change (TrimSpacesCorrector).
fn trim_spaces(iterable: &DiffIterable, text1: &[u16], text2: &[u16]) -> DiffIterable {
    let mut changes = Vec::new();
    for r in &iterable.changes {
        let Range { mut start1, mut end1, mut start2, mut end2 } = *r;
        if is_leading_trailing_space(text1, start1) {
            start1 = trim_start(text1, start1, end1);
        }
        if end1 > 0 && is_leading_trailing_space(text1, end1 - 1) {
            end1 = trim_end(text1, start1, end1);
        }
        if is_leading_trailing_space(text2, start2) {
            start2 = trim_start(text2, start2, end2);
        }
        if end2 > 0 && is_leading_trailing_space(text2, end2 - 1) {
            end2 = trim_end(text2, start2, end2);
        }
        let trimmed = Range::new(start1, end1, start2, end2);
        if !trimmed.is_empty() && !is_equals_range(text1, text2, trimmed) {
            changes.push(trimmed);
        }
    }
    DiffIterable::new(changes, text1.len(), text2.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    /// The offsets of each chunk, and whether it is a line break.
    fn spans(text: &str) -> Vec<(usize, usize, bool)> {
        get_inline_chunks(&units(text)).iter().map(|word| (word.offset1, word.offset2, word.newline)).collect()
    }

    // Not from a spec: words are runs of letters, characters of spaceless scripts stand alone, and a
    // character outside the BMP takes two code units.
    #[test]
    fn splits_a_text_into_words_and_line_breaks() {
        assert_eq!(spans("ab 漢c\n"), [(0, 2, false), (3, 4, false), (4, 5, false), (5, 6, true)]);
        assert_eq!(spans("😂x_1, y"), [(0, 2, false), (2, 5, false), (7, 8, false)]);
        assert!(spans(" \t.;").is_empty());
    }

    // Not from a spec: the trailing whitespace of the last line only counts when the text ends with a
    // line break or with that whitespace.
    #[test]
    fn tells_leading_and_trailing_whitespace_of_lines() {
        let text = units("  a b \n c");
        assert!(is_leading_space(&text, 0) && is_leading_space(&text, 1) && !is_leading_space(&text, 3));
        assert!(is_trailing_space(&text, 5) && is_trailing_space(&text, 6) && !is_trailing_space(&text, 3));
        assert!(is_leading_space(&text, 7) && !is_leading_space(&text, 8) && !is_leading_space(&text, 9));
        assert!(is_trailing_space(&units("a "), 1) && !is_trailing_space(&units("a b"), 1));
    }
}
