//! Line-level comparison, as the IntelliJ Platform does it (`ByLineRt` with
//! `SmartLineChangeCorrector`; Apache-2.0, Copyright JetBrains s.r.o. and contributors): lines are
//! first matched ignoring whitespace and "unimportant" lines (blank, `}`), the gaps are filled, the
//! boundaries optimized, then the whitespace-only matches that the policy does not allow are corrected.

use super::chunk_optimizer::{LineInfo, UNIMPORTANT_LINE_CHAR_COUNT, optimize_line_chunks};
use super::iterables::{ChangeBuilder, DiffIterable, diff_sequences, expand_equal, unchanged};
use super::lcs::{DiffTooBig, Range};
use super::text::{ComparisonPolicy, is_space, strip_spaces, trim_spaces};

/// The keys of a text's lines, equal keys being equal lines.
type Keys<'a> = &'a [&'a [u16]];

/// Line changes between two lists of line texts (without their line breaks).
pub(crate) fn compare_lines(
    lines1: &[&[u16]],
    lines2: &[&[u16]],
    policy: ComparisonPolicy,
) -> Result<DiffIterable, DiffTooBig> {
    let stripped1: Vec<Vec<u16>> = lines1.iter().copied().map(strip_spaces).collect();
    let stripped2: Vec<Vec<u16>> = lines2.iter().copied().map(strip_spaces).collect();
    // The keys ignoring whitespace.
    let iw1: Vec<&[u16]> = stripped1.iter().map(Vec::as_slice).collect();
    let iw2: Vec<&[u16]> = stripped2.iter().map(Vec::as_slice).collect();
    let infos1 = line_infos(lines1, &iw1, policy);
    let infos2 = line_infos(lines2, &iw2, policy);
    let keys1: Vec<&[u16]> = infos1.iter().map(|line| line.key).collect();
    let keys2: Vec<&[u16]> = infos2.iter().map(|line| line.key).collect();

    let smart = compare_smart(&infos1, &infos2, &iw1, &iw2)?;
    let iw_changes = optimize_line_chunks(&infos1, &infos2, &smart);
    if policy == ComparisonPolicy::Ignore {
        let expanded = iw_changes
            .changes
            .iter()
            .map(|c| expand_equal(&keys1, &keys2, c.start1, c.start2, c.end1, c.end2))
            .filter(|r| !r.is_empty())
            .collect();
        return Ok(DiffIterable::new(expanded, lines1.len(), lines2.len()));
    }
    Ok(correct_changes_second_step(&keys1, &keys2, &iw1, &iw2, &iw_changes))
}

/// Each line's identity under the policy (ComparisonUtil.isEquals) and its weight.
fn line_infos<'a>(lines: &[&'a [u16]], iw_keys: &[&'a [u16]], policy: ComparisonPolicy) -> Vec<LineInfo<'a>> {
    lines
        .iter()
        .copied()
        .zip(iw_keys.iter().copied())
        .map(|(content, iw_key)| LineInfo {
            key: match policy {
                ComparisonPolicy::Default => content,
                ComparisonPolicy::Trim => trim_spaces(content),
                ComparisonPolicy::Ignore => iw_key,
            },
            non_space_chars: content.iter().filter(|unit| !is_space(**unit)).count(),
        })
        .collect()
}

/// Matches the lines ignoring the unimportant ones, then compares the gaps between the matches with all
/// their lines (compareSmart with SmartLineChangeCorrector).
fn compare_smart(
    lines1: &[LineInfo<'_>],
    lines2: &[LineInfo<'_>],
    keys1: Keys<'_>,
    keys2: Keys<'_>,
) -> Result<DiffIterable, DiffTooBig> {
    let indexes1 = important(lines1);
    let indexes2 = important(lines2);
    let big1: Vec<&[u16]> = indexes1.iter().map(|index| keys1[*index]).collect();
    let big2: Vec<&[u16]> = indexes2.iter().map(|index| keys2[*index]).collect();
    let changes = diff_sequences(&big1, &big2)?;

    let mut builder = ChangeBuilder::new(lines1.len(), lines2.len());
    let mut last1 = 0;
    let mut last2 = 0;
    for r in unchanged(&changes) {
        for (index1, index2) in r.pairs() {
            let start1 = indexes1[index1];
            let start2 = indexes2[index2];
            match_gap(&mut builder, keys1, keys2, Range::new(last1, start1, last2, start2))?;
            builder.mark_equal(start1, start2);
            last1 = start1 + 1;
            last2 = start2 + 1;
        }
    }
    match_gap(&mut builder, keys1, keys2, Range::new(last1, lines1.len(), last2, lines2.len()))?;
    Ok(builder.finish())
}

/// The indexes of the lines heavy enough to anchor a match.
fn important(lines: &[LineInfo<'_>]) -> Vec<usize> {
    (0..lines.len()).filter(|index| lines[*index].non_space_chars > UNIMPORTANT_LINE_CHAR_COUNT).collect()
}

/// Compares all the lines between two matched important lines: the equal ends of the gap match, the
/// lines between them are compared on their own.
fn match_gap(builder: &mut ChangeBuilder<'_>, keys1: Keys<'_>, keys2: Keys<'_>, gap: Range) -> Result<(), DiffTooBig> {
    let expand = expand_equal(keys1, keys2, gap.start1, gap.start2, gap.end1, gap.end2);
    let inner = diff_sequences(&keys1[expand.start1..expand.end1], &keys2[expand.start2..expand.end2])?;
    builder.mark_equal_range(gap.start1, gap.start2, expand.start1, expand.start2);
    for r in unchanged(&inner) {
        builder.mark_equal_count(expand.start1 + r.start1, expand.start2 + r.start2, r.end1 - r.start1);
    }
    builder.mark_equal_range(expand.end1, expand.end2, gap.end1, gap.end2);
    Ok(())
}

/// Fixes the lines matched ignoring whitespace that are not equal under the policy
/// (ByLineRt.correctChangesSecondStep). Equal lines stay matched and unmatched ones stay unmatched; in
/// a run of lines equal but for their whitespace the alignment with the most equal pairs wins, so
/// ".{ ..{ ...{" against "..{ ...{" matches the last two.
fn correct_changes_second_step(
    keys1: Keys<'_>,
    keys2: Keys<'_>,
    iw1: Keys<'_>,
    iw2: Keys<'_>,
    changes: &DiffIterable,
) -> DiffIterable {
    let builder = ChangeBuilder::expanding(keys1, keys2);
    let mut corrector = Corrector { keys1, keys2, iw1, iw2, builder, sample: None, last1: 0, last2: 0 };
    for r in unchanged(changes) {
        for (index1, index2) in r.pairs() {
            if corrector.sample == Some(iw1[index1]) {
                continue;
            }
            corrector.flush(index1, index2);
            if corrector.equal(index1, index2) {
                corrector.builder.mark_equal(index1, index2);
            } else {
                corrector.sample = Some(iw1[index1]);
            }
        }
    }
    corrector.flush(changes.length1, changes.length2);
    corrector.builder.finish()
}

struct Corrector<'a> {
    keys1: Keys<'a>,
    keys2: Keys<'a>,
    iw1: Keys<'a>,
    iw2: Keys<'a>,
    builder: ChangeBuilder<'a>,
    /// The whitespace-free text of the run of lines being collected, when there is one.
    sample: Option<&'a [u16]>,
    last1: usize,
    last2: usize,
}

impl Corrector<'_> {
    fn equal(&self, index1: usize, index2: usize) -> bool {
        self.keys1[index1] == self.keys2[index2]
    }

    /// Ends the run of lines that read as the sample: those before `line1` and `line2` are aligned.
    fn flush(&mut self, line1: usize, line2: usize) {
        let Some(sample) = self.sample.take() else { return };
        let start1 = self.last1.max(self.builder.index1());
        let start2 = self.last2.max(self.builder.index2());
        let sub1: Vec<usize> = (start1..line1).filter(|index| self.iw1[*index] == sample).collect();
        let sub2: Vec<usize> = (start2..line2).filter(|index| self.iw2[*index] == sample).collect();
        if let Some(last) = sub1.last() {
            self.last1 = last + 1;
        }
        if let Some(last) = sub2.last() {
            self.last2 = last + 1;
        }
        self.align_exact_matching(&sub1, &sub2);
    }

    fn align_exact_matching(&mut self, sub1: &[usize], sub2: &[usize]) {
        let longest = sub1.len().max(sub2.len());
        // Every combination is tried: limit the search space.
        if longest > 10 || sub1.len() == sub2.len() {
            for (index1, index2) in sub1.iter().copied().zip(sub2.iter().copied()) {
                if self.equal(index1, index2) {
                    self.builder.mark_equal(index1, index2);
                }
            }
            return;
        }
        if sub1.len() < sub2.len() {
            let matching = best_matching_alignment(sub1, sub2, |index1, index2| self.equal(index1, index2));
            for (index1, position) in sub1.iter().copied().zip(matching) {
                let index2 = sub2[position];
                if self.equal(index1, index2) {
                    self.builder.mark_equal(index1, index2);
                }
            }
        } else {
            let matching = best_matching_alignment(sub2, sub1, |index2, index1| self.equal(index1, index2));
            for (index2, position) in sub2.iter().copied().zip(matching) {
                let index1 = sub1[position];
                if self.equal(index1, index2) {
                    self.builder.mark_equal(index1, index2);
                }
            }
        }
    }
}

/// For each element of `sub1`, the position in the longer `sub2` it pairs with, in the increasing
/// combination with the most equal pairs (getBestMatchingAlignment); the first one found wins a tie.
fn best_matching_alignment(sub1: &[usize], sub2: &[usize], equal: impl Fn(usize, usize) -> bool) -> Vec<usize> {
    let size = sub1.len();
    let mut search = Alignment { sub1, sub2, equal, comb: vec![0; size], best: (0..size).collect(), best_weight: 0 };
    search.combinations(0, 0);
    search.best
}

struct Alignment<'a, F> {
    sub1: &'a [usize],
    sub2: &'a [usize],
    equal: F,
    /// The combination being built.
    comb: Vec<usize>,
    best: Vec<usize>,
    best_weight: usize,
}

impl<F: Fn(usize, usize) -> bool> Alignment<'_, F> {
    /// Tries every way to complete the combination from its element `k` on, with positions from
    /// `start` on.
    fn combinations(&mut self, start: usize, k: usize) {
        if k == self.sub1.len() {
            let pairs = self.sub1.iter().zip(&self.comb);
            let weight = pairs.filter(|(index1, position)| (self.equal)(**index1, self.sub2[**position])).count();
            if weight > self.best_weight {
                self.best_weight = weight;
                self.best.clone_from(&self.comb);
            }
            return;
        }
        for position in start..self.sub2.len() {
            self.comb[k] = position;
            self.combinations(position + 1, k + 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the alignment with the most equal pairs wins, the identity when none has any.
    #[test]
    fn picks_the_alignment_with_the_most_equal_pairs() {
        assert_eq!(best_matching_alignment(&[0], &[5, 6, 7], |_, index2| index2 == 7), [2]);
        assert_eq!(best_matching_alignment(&[0, 1], &[5, 6, 7], |index1, index2| index1 + 6 == index2), [1, 2]);
        assert_eq!(best_matching_alignment(&[0, 1], &[5, 6, 7], |_, _| false), [0, 1]);
    }
}
