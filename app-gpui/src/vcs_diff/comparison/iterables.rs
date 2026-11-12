//! Changed and unchanged ranges between two sequences, as the IntelliJ Platform keeps them
//! (`DiffIterable` and `DiffIterableUtil`; Apache-2.0, Copyright JetBrains s.r.o. and contributors).
//! The changes are a sorted list; "fair" iterables, whose unchanged ranges have equal lengths on both
//! sides, use the same shape.

use super::lcs::{DiffTooBig, Range, diff_keys};
use std::hash::Hash;

/// The changes between two sequences of the given lengths, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffIterable {
    pub(crate) length1: usize,
    pub(crate) length2: usize,
    pub(crate) changes: Vec<Range>,
}

impl DiffIterable {
    pub(crate) fn new(changes: Vec<Range>, length1: usize, length2: usize) -> Self {
        Self { length1, length2, changes }
    }
}

/// The ranges between the changes (ChangeDiffIterableBase.UnchangedIterator).
pub(crate) fn unchanged(iterable: &DiffIterable) -> Vec<Range> {
    let mut out = Vec::new();
    let mut last1 = 0;
    let mut last2 = 0;
    let mut changes = iterable.changes.as_slice();
    if let Some(first) = changes.first()
        && first.start1 == 0
        && first.start2 == 0
    {
        last1 = first.end1;
        last2 = first.end2;
        changes = &changes[1..];
    }
    for change in changes {
        out.push(Range::new(last1, change.start1, last2, change.start2));
        last1 = change.end1;
        last2 = change.end2;
    }
    if last1 != iterable.length1 || last2 != iterable.length2 {
        out.push(Range::new(last1, iterable.length1, last2, iterable.length2));
    }
    out
}

/// The changes of two sequences whose unchanged ranges are given.
pub(crate) fn create_unchanged(ranges: Vec<Range>, length1: usize, length2: usize) -> DiffIterable {
    DiffIterable::new(unchanged(&DiffIterable::new(ranges, length1, length2)), length1, length2)
}

/// The changes of two sequences of keys compared by equality (DiffIterableUtil.diff).
pub(crate) fn diff_sequences<T: Eq + Hash>(a: &[T], b: &[T]) -> Result<DiffIterable, DiffTooBig> {
    Ok(DiffIterable::new(diff_keys(a, b)?, a.len(), b.len()))
}

/// The part of `changed` inside `bounds`, relative to its starts (SubiterableDiffIterable). The changes
/// before `first_index` are known to end before the bounds.
pub(crate) fn sub_iterable(changed: &[Range], bounds: Range, first_index: usize) -> DiffIterable {
    let Range { start1, end1, start2, end2 } = bounds;
    let mut out = Vec::new();
    for r in changed.iter().skip(first_index) {
        if r.end1 < start1 || r.end2 < start2 {
            continue;
        }
        if r.start1 > end1 || r.start2 > end2 {
            break;
        }
        let sub = Range::new(
            start1.max(r.start1) - start1,
            end1.min(r.end1) - start1,
            start2.max(r.start2) - start2,
            end2.min(r.end2) - start2,
        );
        if !sub.is_empty() {
            out.push(sub);
        }
    }
    DiffIterable::new(out, end1 - start1, end2 - start2)
}

/// Shrinks a change before it is recorded.
type Shrink<'a> = Box<dyn Fn(Range) -> Range + 'a>;

/// Collects the changes from the equal ranges marked in order (DiffIterableUtil.ChangeBuilder). An
/// expanding builder first shrinks each change by its equal ends and drops what is left empty
/// (ExpandChangeBuilder).
pub(crate) struct ChangeBuilder<'a> {
    index1: usize,
    index2: usize,
    length1: usize,
    length2: usize,
    changes: Vec<Range>,
    shrink: Option<Shrink<'a>>,
}

impl<'a> ChangeBuilder<'a> {
    pub(crate) fn new(length1: usize, length2: usize) -> Self {
        Self { index1: 0, index2: 0, length1, length2, changes: Vec::new(), shrink: None }
    }

    /// A builder for two sequences of objects that keeps the equal ends of a change out of it.
    pub(crate) fn expanding<T: PartialEq>(objects1: &'a [T], objects2: &'a [T]) -> Self {
        let shrink: Shrink<'a> =
            Box::new(move |r: Range| expand_equal(objects1, objects2, r.start1, r.start2, r.end1, r.end2));
        Self { shrink: Some(shrink), ..Self::new(objects1.len(), objects2.len()) }
    }

    /// Where the last equal range marked ends on the first side.
    pub(crate) fn index1(&self) -> usize {
        self.index1
    }

    /// Where the last equal range marked ends on the second side.
    pub(crate) fn index2(&self) -> usize {
        self.index2
    }

    /// Marks one pair of elements equal.
    pub(crate) fn mark_equal(&mut self, index1: usize, index2: usize) {
        self.mark_equal_range(index1, index2, index1 + 1, index2 + 1);
    }

    /// Marks `[index1, end1)` and `[index2, end2)` equal; what lies before them, since the last equal
    /// range, is a change.
    pub(crate) fn mark_equal_range(&mut self, index1: usize, index2: usize, end1: usize, end2: usize) {
        if index1 == end1 && index2 == end2 {
            return;
        }
        if self.index1 != index1 || self.index2 != index2 {
            self.add_change(self.index1, self.index2, index1, index2);
        }
        self.index1 = end1;
        self.index2 = end2;
    }

    pub(crate) fn mark_equal_count(&mut self, index1: usize, index2: usize, count: usize) {
        self.mark_equal_range(index1, index2, index1 + count, index2 + count);
    }

    pub(crate) fn finish(mut self) -> DiffIterable {
        if self.length1 != self.index1 || self.length2 != self.index2 {
            self.add_change(self.index1, self.index2, self.length1, self.length2);
        }
        DiffIterable::new(self.changes, self.length1, self.length2)
    }

    fn add_change(&mut self, start1: usize, start2: usize, end1: usize, end2: usize) {
        let change = Range::new(start1, end1, start2, end2);
        match &self.shrink {
            None => self.changes.push(change),
            Some(shrink) => {
                let shrunk = shrink(change);
                if !shrunk.is_empty() {
                    self.changes.push(shrunk);
                }
            }
        }
    }
}

/// Shrinks `[start, end)` by the equal elements at both ends (TrimUtil.expand).
pub(crate) fn expand_equal<T: PartialEq>(
    a: &[T],
    b: &[T],
    start1: usize,
    start2: usize,
    end1: usize,
    end2: usize,
) -> Range {
    let forward = expand_forward(a, b, start1, start2, end1, end2);
    let start1 = start1 + forward;
    let start2 = start2 + forward;
    let backward = expand_backward(a, b, start1, start2, end1, end2);
    Range::new(start1, end1 - backward, start2, end2 - backward)
}

/// How many elements are equal from the starts on.
pub(crate) fn expand_forward<T: PartialEq>(
    a: &[T],
    b: &[T],
    mut start1: usize,
    mut start2: usize,
    end1: usize,
    end2: usize,
) -> usize {
    let old = start1;
    while start1 < end1 && start2 < end2 && a[start1] == b[start2] {
        start1 += 1;
        start2 += 1;
    }
    start1 - old
}

/// How many elements are equal from the ends back.
pub(crate) fn expand_backward<T: PartialEq>(
    a: &[T],
    b: &[T],
    start1: usize,
    start2: usize,
    mut end1: usize,
    mut end2: usize,
) -> usize {
    let old = end1;
    while start1 < end1 && start2 < end2 && a[end1 - 1] == b[end2 - 1] {
        end1 -= 1;
        end2 -= 1;
    }
    old - end1
}

#[cfg(test)]
mod tests {
    use super::*;

    // Not from a spec: the unchanged ranges are the gaps around the changes, a leading change leaving none.
    #[test]
    fn lists_the_ranges_between_the_changes() {
        let leading = DiffIterable::new(vec![Range::new(0, 1, 0, 0), Range::new(3, 3, 2, 4)], 5, 6);
        assert_eq!(unchanged(&leading), [Range::new(1, 3, 0, 2), Range::new(3, 5, 4, 6)]);
        let inner = DiffIterable::new(vec![Range::new(2, 3, 2, 2)], 3, 2);
        assert_eq!(unchanged(&inner), [Range::new(0, 2, 0, 2)]);
        assert_eq!(create_unchanged(unchanged(&leading), 5, 6), leading);
    }

    // Not from a spec: an expanding builder keeps the equal ends of a change out of it.
    #[test]
    fn shrinks_changes_by_their_equal_ends() {
        let a = ["x", "a", "b", "y"];
        let b = ["x", "a", "c", "b", "y"];
        let mut plain = ChangeBuilder::new(a.len(), b.len());
        plain.mark_equal(0, 0);
        plain.mark_equal(3, 4);
        assert_eq!(plain.finish().changes, [Range::new(1, 3, 1, 4)]);
        let mut expanding = ChangeBuilder::expanding(&a, &b);
        expanding.mark_equal(0, 0);
        expanding.mark_equal(3, 4);
        assert_eq!(expanding.finish().changes, [Range::new(2, 2, 2, 3)]);
    }
}
