//! Longest common subsequence as the IntelliJ Platform computes it (`com.intellij.util.diff`: Diff,
//! Reindexer, MyersLCS, PatienceIntLCS, UniqueLCS; Apache-2.0, Copyright JetBrains s.r.o. and
//! contributors): the common prefix and suffix are trimmed, the elements unique to one side discarded,
//! then Myers' O(ND) diff runs bounded by a threshold, falling back to patience diff when the sequences
//! differ too much.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;

/// Half-open index ranges `[start1, end1)` and `[start2, end2)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Range {
    pub start1: usize,
    pub end1: usize,
    pub start2: usize,
    pub end2: usize,
}

impl Range {
    pub const fn new(start1: usize, end1: usize, start2: usize, end2: usize) -> Self {
        Self { start1, end1, start2, end2 }
    }

    /// Whether both sides are empty.
    pub const fn is_empty(&self) -> bool {
        self.start1 == self.end1 && self.start2 == self.end2
    }

    /// The index pairs of a range whose sides have the same length, in order.
    pub(crate) fn pairs(&self) -> impl Iterator<Item = (usize, usize)> + use<> {
        (self.start1..self.end1).zip(self.start2..)
    }
}

/// The sequences differ too much to be compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiffTooBig;

impl fmt::Display for DiffTooBig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Diff is too big")
    }
}

impl std::error::Error for DiffTooBig {}

/// A bounded search inside the LCS gave up.
struct FilesTooBig;

/// Changed ranges between two sequences of keys, equal keys being equal elements.
pub(crate) fn diff_keys<T: Eq + Hash>(a: &[T], b: &[T]) -> Result<Vec<Range>, DiffTooBig> {
    let mut numbers = HashMap::new();
    let ints1 = enumerate(&mut numbers, a);
    let ints2 = enumerate(&mut numbers, b);
    diff_ints(&ints1, &ints2)
}

/// The elements as numbers, one per distinct key.
fn enumerate<'a, T: Eq + Hash>(numbers: &mut HashMap<&'a T, u32>, items: &'a [T]) -> Vec<u32> {
    items
        .iter()
        .map(|item| {
            let next = numbers.len() as u32 + 1;
            *numbers.entry(item).or_insert(next)
        })
        .collect()
}

/// Changed ranges between two sequences of numbers.
fn diff_ints(a: &[u32], b: &[u32]) -> Result<Vec<Range>, DiffTooBig> {
    let n1 = a.len();
    let n2 = b.len();
    let shortest = n1.min(n2);
    let mut start_shift = 0;
    while start_shift < shortest && a[start_shift] == b[start_shift] {
        start_shift += 1;
    }
    let mut end_cut = 0;
    while end_cut < shortest - start_shift && a[n1 - end_cut - 1] == b[n2 - end_cut - 1] {
        end_cut += 1;
    }
    let len1 = n1 - start_shift - end_cut;
    let len2 = n2 - start_shift - end_cut;
    if len1 == 0 || len2 == 0 {
        if len1 == 0 && len2 == 0 {
            return Ok(Vec::new());
        }
        return Ok(vec![Range::new(start_shift, start_shift + len1, start_shift, start_shift + len2)]);
    }
    build_changes(&a[start_shift..start_shift + len1], &b[start_shift..start_shift + len2], start_shift)
        .map_err(|FilesTooBig| DiffTooBig)
}

fn build_changes(ints1: &[u32], ints2: &[u32], start_shift: usize) -> Result<Vec<Range>, FilesTooBig> {
    let mut builder = ChangeCollector { changes: Vec::new(), index1: start_shift, index2: start_shift };
    let (reindexer, discarded1, discarded2) = Reindexer::discard_unique(ints1, ints2);
    if discarded1.is_empty() && discarded2.is_empty() {
        builder.add_change(ints1.len(), ints2.len());
        return Ok(builder.changes);
    }
    let mut changes1 = vec![false; discarded1.len()];
    let mut changes2 = vec![false; discarded2.len()];
    let first = Side { ints: &discarded1, start: 0, count: discarded1.len(), changes: &mut changes1 };
    let second = Side { ints: &discarded2, start: 0, count: discarded2.len(), changes: &mut changes2 };
    let bounded = MyersLcs::new(first, second).execute_with_threshold();
    if bounded.is_err() {
        let mut patience = PatienceLcs::new(&discarded1, &discarded2);
        patience.execute(true)?;
        changes1 = patience.changes1;
        changes2 = patience.changes2;
    }
    reindexer.reindex(&changes1, &changes2, &mut builder);
    Ok(builder.changes)
}

/// Collects the changes of an LCS walked in order, shifted by the trimmed prefix.
struct ChangeCollector {
    changes: Vec<Range>,
    index1: usize,
    index2: usize,
}

impl ChangeCollector {
    fn add_equal(&mut self, length: usize) {
        self.index1 += length;
        self.index2 += length;
    }

    fn add_change(&mut self, first: usize, second: usize) {
        self.changes.push(Range::new(self.index1, self.index1 + first, self.index2, self.index2 + second));
        self.index1 += first;
        self.index2 += second;
    }
}

/// Sets `bits[from..to]`, an empty or reversed span being nothing and the end clamped to the slice.
fn fill(bits: &mut [bool], from: usize, to: usize, value: bool) {
    let to = to.min(bits.len());
    if to > from {
        bits[from..to].fill(value);
    }
}

fn at(values: &[isize], index: isize) -> isize {
    values[index as usize]
}

/// One side of a comparison: its elements, the window `[start, start + count)` of them being compared,
/// and the flags of its unmatched elements.
struct Side<'a> {
    ints: &'a [u32],
    start: usize,
    count: usize,
    changes: &'a mut [bool],
}

const DELTA_THRESHOLD_SIZE: isize = 20_000;

/// Myers' O(ND) diff with linear space (MyersLCS); the `changes` of both sides flag the unmatched
/// elements. Indexes are signed: diagonals and the ends of the backward search go below zero.
struct MyersLcs<'a> {
    first: &'a [u32],
    second: &'a [u32],
    start1: isize,
    count1: isize,
    start2: isize,
    count2: isize,
    changes1: &'a mut [bool],
    changes2: &'a mut [bool],
    v_forward: Vec<isize>,
    v_backward: Vec<isize>,
}

impl<'a> MyersLcs<'a> {
    fn new(first: Side<'a>, second: Side<'a>) -> Self {
        fill(first.changes, first.start, first.start + first.count, true);
        fill(second.changes, second.start, second.start + second.count, true);
        let diagonals = first.count + second.count + 1;
        Self {
            first: first.ints,
            second: second.ints,
            start1: first.start as isize,
            count1: first.count as isize,
            start2: second.start as isize,
            count2: second.count as isize,
            changes1: first.changes,
            changes2: second.changes,
            v_forward: vec![0; diagonals],
            v_backward: vec![0; diagonals],
        }
    }

    /// Bounds D by A + B·√N: expected O(N) under common assumptions.
    fn linear_threshold(&self) -> isize {
        20_000 + 10 * (self.count1 + self.count2).isqrt()
    }

    /// Matches what a bounded search finds and leaves the rest unmatched.
    fn execute_linear(&mut self) -> Result<(), FilesTooBig> {
        self.run(self.linear_threshold(), false)
    }

    /// Fails when the sequences differ by more than the threshold.
    fn execute_with_threshold(&mut self) -> Result<(), FilesTooBig> {
        self.run(self.linear_threshold().max(DELTA_THRESHOLD_SIZE), true)
    }

    fn run(&mut self, threshold: isize, fail_on_excess: bool) -> Result<(), FilesTooBig> {
        if self.count1 == 0 || self.count2 == 0 {
            return Ok(());
        }
        let estimate = threshold.min(self.count1 + self.count2);
        self.execute(0, self.count1, 0, self.count2, estimate, fail_on_excess)
    }

    /// LCS of `old[old_start, old_end)` and `new[new_start, new_end)`.
    fn execute(
        &mut self,
        old_start: isize,
        old_end: isize,
        new_start: isize,
        new_end: isize,
        difference_estimate: isize,
        fail_on_excess: bool,
    ) -> Result<(), FilesTooBig> {
        if old_start >= old_end || new_start >= new_end {
            return Ok(());
        }
        let old_length = old_end - old_start;
        let new_length = new_end - new_start;
        self.v_forward[(new_length + 1) as usize] = 0;
        self.v_backward[(new_length + 1) as usize] = 0;
        let half_d = (difference_estimate + 1) >> 1;
        let odd_delta = (old_length - new_length) % 2 != 0;
        let mut xx = -1_isize;
        let mut kk = -1_isize;
        let mut td = -1_isize;

        let mut d = 0_isize;
        'search: while d <= half_d {
            let left = new_length + (-d).max(-new_length + ((d ^ new_length) & 1));
            let right = new_length + d.min(old_length - ((d ^ old_length) & 1));

            let mut k = left;
            while k <= right {
                let forward = &self.v_forward;
                let mut x = if k == left || (k != right && at(forward, k - 1) < at(forward, k + 1)) {
                    at(forward, k + 1)
                } else {
                    at(forward, k - 1) + 1
                };
                let y = x - k + new_length;
                let rest = (old_end - old_start - x).min(new_end - new_start - y);
                x += self.common_forward(old_start + x, new_start + y, rest);
                self.v_forward[k as usize] = x;
                k += 2;
            }

            if odd_delta {
                let mut k = left;
                while k <= right {
                    if (old_length - (d - 1)..=old_length + (d - 1)).contains(&k)
                        && at(&self.v_forward, k) + at(&self.v_backward, new_length + old_length - k) >= old_length
                    {
                        xx = at(&self.v_forward, k);
                        kk = k;
                        td = 2 * d - 1;
                        break 'search;
                    }
                    k += 2;
                }
            }

            let mut k = left;
            while k <= right {
                let backward = &self.v_backward;
                let mut x = if k == left || (k != right && at(backward, k - 1) < at(backward, k + 1)) {
                    at(backward, k + 1)
                } else {
                    at(backward, k - 1) + 1
                };
                let y = x - k + new_length;
                let rest = (old_end - old_start - x).min(new_end - new_start - y);
                x += self.common_backward(old_end - 1 - x, new_end - 1 - y, rest);
                self.v_backward[k as usize] = x;
                k += 2;
            }

            if !odd_delta {
                let mut k = left;
                while k <= right {
                    if (old_length - d..=old_length + d).contains(&k)
                        && at(&self.v_forward, old_length + new_length - k) + at(&self.v_backward, k) >= old_length
                    {
                        xx = old_length - at(&self.v_backward, k);
                        kk = old_length + new_length - k;
                        td = 2 * d;
                        break 'search;
                    }
                    k += 2;
                }
            }

            d += 1;
        }

        if td > 1 {
            let yy = xx - kk + new_length;
            let old_diff = (td + 1) >> 1;
            if 0 < xx && 0 < yy {
                self.execute(old_start, old_start + xx, new_start, new_start + yy, old_diff, fail_on_excess)?;
            }
            if old_start + xx < old_end && new_start + yy < new_end {
                self.execute(old_start + xx, old_end, new_start + yy, new_end, td - old_diff, fail_on_excess)?;
            }
        } else if td >= 0 {
            let mut x = old_start;
            let mut y = new_start;
            while x < old_end && y < new_end {
                let common = self.common_forward(x, y, (old_end - x).min(new_end - y));
                if common > 0 {
                    let from1 = (self.start1 + x) as usize;
                    let from2 = (self.start2 + y) as usize;
                    fill(self.changes1, from1, from1 + common as usize, false);
                    fill(self.changes2, from2, from2 + common as usize, false);
                    x += common;
                    y += common;
                } else if old_end - old_start > new_end - new_start {
                    x += 1;
                } else {
                    y += 1;
                }
            }
        } else if fail_on_excess {
            // The difference is more than the given estimate.
            return Err(FilesTooBig);
        }
        Ok(())
    }

    fn equal_at(&self, x: isize, y: isize) -> bool {
        self.first[(self.start1 + x) as usize] == self.second[(self.start2 + y) as usize]
    }

    fn common_forward(&self, old_index: isize, new_index: isize, max_length: isize) -> isize {
        let max_length = max_length.min(self.count1 - old_index).min(self.count2 - new_index);
        let mut x = old_index;
        let mut y = new_index;
        while x - old_index < max_length && self.equal_at(x, y) {
            x += 1;
            y += 1;
        }
        x - old_index
    }

    fn common_backward(&self, old_index: isize, new_index: isize, max_length: isize) -> isize {
        let max_length = max_length.min(old_index.min(new_index) + 1);
        let mut x = old_index;
        let mut y = new_index;
        while old_index - x < max_length && self.equal_at(x, y) {
            x -= 1;
            y -= 1;
        }
        old_index - x
    }
}

/// Patience diff (PatienceIntLCS): the elements that are unique on both sides anchor the match, Myers
/// fills the gaps between them.
struct PatienceLcs<'a> {
    first: &'a [u32],
    second: &'a [u32],
    changes1: Vec<bool>,
    changes2: Vec<bool>,
}

impl<'a> PatienceLcs<'a> {
    fn new(first: &'a [u32], second: &'a [u32]) -> Self {
        Self { first, second, changes1: vec![false; first.len()], changes2: vec![false; second.len()] }
    }

    /// `fail_on_small_reduction` gives up when the anchors leave most of both sequences in one gap.
    fn execute(&mut self, fail_on_small_reduction: bool) -> Result<(), FilesTooBig> {
        let threshold_check_counter = if fail_on_small_reduction { 2 } else { -1 };
        self.run(0, self.first.len(), 0, self.second.len(), threshold_check_counter)
    }

    fn run(
        &mut self,
        mut start1: usize,
        mut count1: usize,
        mut start2: usize,
        mut count2: usize,
        mut threshold_check_counter: i32,
    ) -> Result<(), FilesTooBig> {
        if count1 == 0 && count2 == 0 {
            return Ok(());
        }
        if count1 == 0 || count2 == 0 {
            self.add_change(start1, count1, start2, count2);
            return Ok(());
        }

        let mut start_offset = 0;
        while start_offset < count1.min(count2)
            && self.first[start1 + start_offset] == self.second[start2 + start_offset]
        {
            start_offset += 1;
        }
        start1 += start_offset;
        start2 += start_offset;
        count1 -= start_offset;
        count2 -= start_offset;
        let mut end_offset = 0;
        while end_offset < count1.min(count2)
            && self.first[start1 + count1 - end_offset - 1] == self.second[start2 + count2 - end_offset - 1]
        {
            end_offset += 1;
        }
        count1 -= end_offset;
        count2 -= end_offset;

        if count1 == 0 || count2 == 0 {
            self.add_change(start1, count1, start2, count2);
            return Ok(());
        }

        if threshold_check_counter == 0 {
            self.check_reduction(count1, count2)?;
        }
        threshold_check_counter = (threshold_check_counter - 1).max(-1);

        let Some((matches1, matches2)) = unique_lcs(self.first, self.second, start1, count1, start2, count2) else {
            if threshold_check_counter >= 0 {
                self.check_reduction(count1, count2)?;
            }
            let first = Side { ints: self.first, start: start1, count: count1, changes: &mut self.changes1 };
            let second = Side { ints: self.second, start: start2, count: count2, changes: &mut self.changes2 };
            return MyersLcs::new(first, second).execute_linear();
        };

        let matched = matches1.len();
        self.run(start1, matches1[0], start2, matches2[0], threshold_check_counter)?;
        let mut i = 1;
        while i < matched {
            let s1 = matches1[i - 1] + 1;
            let s2 = matches2[i - 1] + 1;
            let c1 = matches1[i] - s1;
            let c2 = matches2[i] - s2;
            if c1 > 0 || c2 > 0 {
                self.run(start1 + s1, c1, start2 + s2, c2, threshold_check_counter)?;
            }
            i += 1;
        }
        let last1 = matches1[matched - 1];
        let last2 = matches2[matched - 1];
        let (s1, c1) = if last1 == count1 - 1 { (count1 - 1, 0) } else { (last1 + 1, count1 - last1 - 1) };
        let (s2, c2) = if last2 == count2 - 1 { (count2 - 1, 0) } else { (last2 + 1, count2 - last2 - 1) };
        self.run(start1 + s1, c1, start2 + s2, c2, threshold_check_counter)
    }

    fn add_change(&mut self, start1: usize, count1: usize, start2: usize, count2: usize) {
        fill(&mut self.changes1, start1, start1 + count1, true);
        fill(&mut self.changes2, start2, start2 + count2, true);
    }

    /// Fails unless the part left to compare is under half of one of the sequences.
    fn check_reduction(&self, count1: usize, count2: usize) -> Result<(), FilesTooBig> {
        if count1 * 2 < self.first.len() || count2 * 2 < self.second.len() {
            return Ok(());
        }
        Err(FilesTooBig)
    }
}

/// The longest increasing run of the elements unique on both sides (UniqueLCS), as indexes relative to
/// the starts; `None` when no element is unique on both sides.
fn unique_lcs(
    first: &[u32],
    second: &[u32],
    start1: usize,
    count1: usize,
    start2: usize,
    count2: usize,
) -> Option<(Vec<usize>, Vec<usize>)> {
    // Key -> its offset on the first side, `None` once repeated on either side.
    let mut offsets: HashMap<u32, Option<usize>> = HashMap::new();
    for (i, key) in first[start1..start1 + count1].iter().enumerate() {
        offsets.entry(*key).and_modify(|offset| *offset = None).or_insert(Some(i));
    }
    // Offset on the first side -> offset on the second side plus one, zero when unmatched.
    let mut matches = vec![0_usize; count1];
    let mut count = 0_usize;
    for (i, key) in second[start2..start2 + count2].iter().enumerate() {
        let Some(Some(offset1)) = offsets.get(key).copied() else { continue };
        if matches[offset1] == 0 {
            matches[offset1] = i + 1;
            count += 1;
        } else {
            matches[offset1] = 0;
            offsets.insert(*key, None);
            count -= 1;
        }
    }
    if count == 0 {
        return None;
    }

    let mut sequence = vec![0_usize; count];
    let mut last_element = vec![0_usize; count];
    let mut predecessor: Vec<Option<usize>> = vec![None; count1];
    let mut length = 0;
    for (i, matching) in matches.iter().copied().enumerate() {
        if matching == 0 {
            continue;
        }
        // The first position holding a greater element (the elements are distinct).
        let position = sequence[..length].partition_point(|element| *element < matching);
        sequence[position] = matching;
        last_element[position] = i;
        predecessor[i] = if position > 0 { Some(last_element[position - 1]) } else { None };
        if position == length {
            length += 1;
        }
    }

    let mut result1 = vec![0; length];
    let mut result2 = vec![0; length];
    let mut current = Some(last_element[length - 1]);
    let mut i = length;
    while let Some(offset1) = current
        && i > 0
    {
        i -= 1;
        result1[i] = offset1;
        result2[i] = matches[offset1] - 1;
        current = predecessor[offset1];
    }
    Some((result1, result2))
}

/// Drops the elements that cannot be matched and maps the result back (Reindexer).
struct Reindexer {
    /// For each side, the original index of every element kept.
    old_indices: [Vec<usize>; 2],
    original_lengths: [usize; 2],
}

impl Reindexer {
    /// Both sequences without the elements the other one lacks, and the way back to the originals.
    fn discard_unique(ints1: &[u32], ints2: &[u32]) -> (Self, Vec<u32>, Vec<u32>) {
        let (discarded1, old1) = discard(ints2, ints1);
        let (discarded2, old2) = discard(&discarded1, ints2);
        (Self { old_indices: [old1, old2], original_lengths: [ints1.len(), ints2.len()] }, discarded1, discarded2)
    }

    /// Reports the equal and changed runs of the original sequences, given the unmatched flags of the
    /// discarded ones.
    fn reindex(&self, discarded1: &[bool], discarded2: &[bool], builder: &mut ChangeCollector) {
        let [orig1, orig2] = self.original_lengths;
        let [old1, old2] = &self.old_indices;
        let expanded;
        let (changes1, changes2) = if old1.len() == orig1 && old2.len() == orig2 {
            (discarded1, discarded2)
        } else {
            expanded = self.expand(discarded1, discarded2);
            (expanded.0.as_slice(), expanded.1.as_slice())
        };

        let mut x = 0;
        let mut y = 0;
        while x < orig1 && y < orig2 {
            let start_x = x;
            while x < orig1 && y < orig2 && !changes1[x] && !changes2[y] {
                x += 1;
                y += 1;
            }
            if x > start_x {
                builder.add_equal(x - start_x);
            }
            let mut dx = 0;
            let mut dy = 0;
            while x < orig1 && changes1[x] {
                dx += 1;
                x += 1;
            }
            while y < orig2 && changes2[y] {
                dy += 1;
                y += 1;
            }
            if dx != 0 || dy != 0 {
                builder.add_change(dx, dy);
            }
        }
        if x != orig1 || y != orig2 {
            builder.add_change(orig1 - x, orig2 - y);
        }
    }

    /// The unmatched flags of the original sequences: those of the kept elements, plus every discarded
    /// element.
    fn expand(&self, discarded1: &[bool], discarded2: &[bool]) -> (Vec<bool>, Vec<bool>) {
        let [orig1, orig2] = self.original_lengths;
        let [old1, old2] = &self.old_indices;
        let disc1 = old1.len();
        let disc2 = old2.len();
        let mut changes1 = vec![false; orig1];
        let mut changes2 = vec![false; orig2];
        let changed = |flags: &[bool], index: usize| flags.get(index).copied().unwrap_or(false);
        let mut x = 0;
        let mut y = 0;
        while x < disc1 || y < disc2 {
            if x < disc1 && y < disc2 && !discarded1[x] && !discarded2[y] {
                x = increment(old1, x, &mut changes1, orig1);
                y = increment(old2, y, &mut changes2, orig2);
            } else if changed(discarded1, x) {
                changes1[old1[x]] = true;
                x = increment(old1, x, &mut changes1, orig1);
            } else if changed(discarded2, y) {
                changes2[old2[y]] = true;
                y = increment(old2, y, &mut changes2, orig2);
            } else {
                // An LCS matches as many elements on each side, so one of the cases above applies.
                break;
            }
        }
        fill(&mut changes1, 0, if disc1 == 0 { orig1 } else { old1[0] }, true);
        fill(&mut changes2, 0, if disc2 == 0 { orig2 } else { old2[0] }, true);
        (changes1, changes2)
    }
}

/// The elements of `to_discard` that `needed` has too, and their indexes.
fn discard(needed: &[u32], to_discard: &[u32]) -> (Vec<u32>, Vec<usize>) {
    let present: HashSet<u32> = needed.iter().copied().collect();
    to_discard.iter().enumerate().filter(|(_, value)| present.contains(*value)).map(|(i, value)| (*value, i)).unzip()
}

/// Flags the discarded elements that follow kept element `i`; returns the next kept element.
fn increment(indexes: &[usize], i: usize, set: &mut [bool], length: usize) -> usize {
    let next = indexes.get(i + 1).copied().unwrap_or(length);
    fill(set, indexes[i] + 1, next, true);
    i + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: a sequence that only loses elements reports them, shifted by the common prefix.
    #[test]
    fn reports_a_deletion_between_a_common_prefix_and_suffix() {
        assert_eq!(diff_keys(&[1, 2, 3], &[1, 3]), Ok(vec![Range::new(1, 2, 1, 1)]));
        assert_eq!(diff_keys(&["a", "b"], &["a", "b"]), Ok(Vec::new()));
        assert_eq!(diff_keys::<u8>(&[], &[7]), Ok(vec![Range::new(0, 0, 0, 1)]));
    }

    // Not from a spec: whatever the changes, the elements between them are equal pairs.
    #[test]
    fn leaves_equal_elements_between_the_changes() {
        let mut seed = 0x2545_f491_u32;
        let mut next = |modulo: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % modulo
        };
        for round in 0..200 {
            let a: Vec<u32> = (0..next(40)).map(|_| next(6)).collect();
            let b: Vec<u32> = (0..next(40)).map(|_| next(6)).collect();
            let changes = diff_keys(&a, &b).expect("small sequences are always compared");
            let mut last1 = 0;
            let mut last2 = 0;
            for change in changes.iter().copied().chain([Range::new(a.len(), a.len(), b.len(), b.len())]) {
                assert!(last1 <= change.start1 && last2 <= change.start2, "round {round}: {changes:?}");
                assert_eq!(a[last1..change.start1], b[last2..change.start2], "round {round}: {a:?} - {b:?}");
                last1 = change.end1;
                last2 = change.end2;
            }
            assert!(changes.iter().all(|change| !change.is_empty()), "round {round}: {changes:?}");
        }
    }

    // Not from a spec: the error reads as a sentence.
    #[test]
    fn describes_itself() {
        assert_eq!(DiffTooBig.to_string(), "Diff is too big");
    }
}
