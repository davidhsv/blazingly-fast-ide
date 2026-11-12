//! The merge editor's per-change controls: an Accept arrow and an Ignore cross beside each change in
//! the side panes, a magic wand beside a conflict of the result that needs no decision, the changes
//! tinted in the result, and the open changes on each pane's overview lane.
//!
//! Offsets are UTF-16 code units.

use super::doc::Doc;
use super::marks::{MarkKind, ScrollbarMark};

/// A side pane of the merge editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergeSide {
    Left,
    Right,
}

/// A pane of the merge editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergePane {
    Side(MergeSide),
    Result,
}

/// Who changed a block of the base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergeKind {
    Left,
    Right,
    /// Both sides made the same change.
    Both,
    /// The sides changed it differently.
    Conflict,
}

/// The sides of a change that were accepted or ignored.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Done {
    pub left: bool,
    pub right: bool,
}

/// A change waiting in the result of a merge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeChange {
    /// Where the change sits in the result; follows the document as it is edited.
    pub from: usize,
    pub to: usize,
    pub kind: MergeKind,
    pub left: String,
    pub base: String,
    pub right: String,
    /// Where the change is in the left and the right version, for the side panes.
    pub left_range: (usize, usize),
    pub right_range: (usize, usize),
    /// The word-level merge of a conflict whose sides changed different words.
    pub auto: Option<String>,
    pub done: Done,
    /// One side of a conflict was accepted and the other is still to be decided.
    pub oneside: bool,
}

impl MergeChange {
    /// Both sides are settled.
    pub fn is_resolved(&self) -> bool {
        self.done.left && self.done.right
    }

    pub fn is_conflict(&self) -> bool {
        self.kind == MergeKind::Conflict
    }

    /// Whether `side` differs from the base here.
    pub fn changes_side(&self, side: MergeSide) -> bool {
        match self.kind {
            MergeKind::Both | MergeKind::Conflict => true,
            MergeKind::Left => side == MergeSide::Left,
            MergeKind::Right => side == MergeSide::Right,
        }
    }

    fn done_on(&self, side: MergeSide) -> bool {
        match side {
            MergeSide::Left => self.done.left,
            MergeSide::Right => self.done.right,
        }
    }

    fn range(&self, side: MergeSide) -> (usize, usize) {
        match side {
            MergeSide::Left => self.left_range,
            MergeSide::Right => self.right_range,
        }
    }

    /// Whether the user edited the change's text in the result: automatic resolving then leaves it
    /// alone.
    fn edited(&self, doc: &Doc) -> bool {
        doc.slice_string(self.from, self.to) != self.base
    }

    /// Whether the conflict can be resolved without a decision: its sides changed different words
    /// and neither was accepted or ignored yet.
    pub fn resolves_automatically(&self, doc: &Doc) -> bool {
        !self.edited(doc) && self.is_conflict() && self.auto.is_some() && !self.done.left && !self.done.right
    }
}

/// What a press on a control asks the merge editor to do; `resolve` is set when the platform's
/// modifier key is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeControl {
    /// Take the side's text; with `resolve`, settle the change with it.
    Accept {
        index: usize,
        side: MergeSide,
        resolve: bool,
    },
    /// Leave the side's text out; with `resolve`, both sides'.
    Ignore {
        index: usize,
        side: MergeSide,
        resolve: bool,
    },
    ResolveAutomatically {
        index: usize,
    },
}

/// The Accept and Ignore controls beside a change of a side pane that is still to be decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideMarker {
    pub index: usize,
    pub side: MergeSide,
    /// The other side of the conflict is in the result already: this one goes below it.
    pub oneside: bool,
}

impl SideMarker {
    pub const ACCEPT_TITLE: &'static str = "Accept (hold the modifier key to resolve using this side)";
    pub const IGNORE_TITLE: &'static str = "Ignore (hold the modifier key to ignore both sides)";
    pub const IGNORE_ICON: &'static str = "diff-remove";

    /// The Accept arrow, pointing toward the result.
    pub fn accept_icon(&self) -> &'static str {
        match (self.side, self.oneside) {
            (MergeSide::Left, false) => "diff-arrow-right",
            (MergeSide::Left, true) => "diff-arrow-right-down",
            (MergeSide::Right, false) => "diff-arrow-left",
            (MergeSide::Right, true) => "diff-arrow-left-down",
        }
    }

    pub fn accept(&self, resolve: bool) -> MergeControl {
        MergeControl::Accept { index: self.index, side: self.side, resolve }
    }

    pub fn ignore(&self, resolve: bool) -> MergeControl {
        MergeControl::Ignore { index: self.index, side: self.side, resolve }
    }
}

/// The controls on the line starting at `line_from` of the pane `side`, whose document is `doc`.
pub fn side_marker(changes: &[MergeChange], side: MergeSide, doc: &Doc, line_from: usize) -> Option<SideMarker> {
    let index = changes.iter().position(|c| {
        c.changes_side(side) && !c.done_on(side) && doc.line_at(c.range(side).0.min(doc.len())).from == line_from
    })?;
    Some(SideMarker { index, side, oneside: changes[index].oneside })
}

/// English title of the wand.
pub const WAND_TITLE: &str = "Resolve Automatically";
pub const WAND_ICON: &str = "diff-magic";

/// The conflict the wand on the line starting at `line_from` of the result resolves.
pub fn wand_marker(changes: &[MergeChange], doc: &Doc, line_from: usize) -> Option<MergeControl> {
    let index = changes
        .iter()
        .position(|c| c.resolves_automatically(doc) && doc.line_at(c.from.min(doc.len())).from == line_from)?;
    Some(MergeControl::ResolveAutomatically { index })
}

/// How a change is tinted in the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeTint {
    /// Settled: the text is dimmed.
    Resolved,
    Conflict,
    Change,
}

impl MergeTint {
    fn of(change: &MergeChange) -> Self {
        if change.is_resolved() {
            Self::Resolved
        } else if change.is_conflict() {
            Self::Conflict
        } else {
            Self::Change
        }
    }

    /// Name of the theme token the tint takes its colour from; a settled change has none.
    pub fn color_token(self) -> Option<&'static str> {
        match self {
            Self::Resolved => None,
            Self::Conflict => Some("vcs-conflict"),
            Self::Change => Some("vcs-modified"),
        }
    }
}

/// A tinted place of the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResultMark {
    /// The line starting at `from`.
    Line { from: usize, tint: MergeTint },
    /// A change that adds lines has no text in the result yet: a thin line above `at` shows where
    /// it goes.
    Point { at: usize, tint: MergeTint },
}

/// The changes tinted in the result: conflicts, the other changes and the settled ones.
pub fn result_marks(changes: &[MergeChange], doc: &Doc) -> Vec<ResultMark> {
    let mut out = Vec::new();
    for change in changes {
        let tint = MergeTint::of(change);
        if change.to > change.from {
            let (first, last) = (doc.line_at(change.from).number, doc.line_at(change.to - 1).number);
            out.extend((first..=last).map(|number| ResultMark::Line { from: doc.line(number).from, tint }));
        } else if !change.is_resolved() {
            out.push(ResultMark::Point { at: change.from, tint });
        }
    }
    out
}

/// Name of the theme token of a change's type on the overview lane: a conflict, else how the
/// changed side differs from the base.
fn stripe_token(change: &MergeChange) -> &'static str {
    if change.is_conflict() {
        return "diff-conflict-stripe";
    }
    let changed = if change.kind == MergeKind::Right { &change.right } else { &change.left };
    if change.base.is_empty() {
        "diff-inserted-stripe"
    } else if changed.is_empty() {
        "diff-deleted-stripe"
    } else {
        "diff-modified-stripe"
    }
}

fn stripe_mark(change: &MergeChange, from: usize, to: usize) -> ScrollbarMark {
    ScrollbarMark {
        from,
        to: Some(if to > from { to - 1 } else { from }),
        color: stripe_token(change),
        kind: MarkKind::Full,
        tooltip: None,
        priority: u32::from(change.is_conflict()),
    }
}

/// The open changes on a pane's overview lane: the result marks each change until it is resolved, a
/// side pane the changes of that side until that side is accepted or ignored. Conflicts lie on top.
pub fn merge_stripe(pane: MergePane, changes: &[MergeChange]) -> Vec<ScrollbarMark> {
    changes
        .iter()
        .filter_map(|change| match pane {
            MergePane::Result => (!change.is_resolved()).then(|| stripe_mark(change, change.from, change.to)),
            MergePane::Side(side) => (change.changes_side(side) && !change.done_on(side)).then(|| {
                let (from, to) = change.range(side);
                stripe_mark(change, from, to)
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Line 2: both sides changed it differently (conflict); line 4: left only; line 6: right deletes it.
    const BASE: &str = "a\nb\nc\nd\ne\nf\ng";
    const LEFT: &str = "a\nB-left\nc\nD\ne\nf\ng";
    const RIGHT: &str = "a\nB-right\nc\nd\ne\ng";

    fn change(
        kind: MergeKind,
        range: (usize, usize),
        texts: [&str; 3],
        left_range: (usize, usize),
        right_range: (usize, usize),
    ) -> MergeChange {
        MergeChange {
            from: range.0,
            to: range.1,
            kind,
            left: texts[0].to_owned(),
            base: texts[1].to_owned(),
            right: texts[2].to_owned(),
            left_range,
            right_range,
            auto: None,
            done: Done::default(),
            oneside: false,
        }
    }

    /// The changes of the three texts above as the merge starts: the result is the base.
    fn start() -> Vec<MergeChange> {
        vec![
            change(MergeKind::Conflict, (2, 4), ["B-left\n", "b\n", "B-right\n"], (2, 9), (2, 10)),
            change(MergeKind::Left, (6, 8), ["D\n", "d\n", "d\n"], (11, 13), (12, 14)),
            change(MergeKind::Right, (10, 12), ["f\n", "f\n", ""], (15, 17), (16, 16)),
        ]
    }

    /// Offset of the start of the 1-based line `n` of `text`.
    fn line(text: &str, n: usize) -> usize {
        Doc::new(text).line(n).from
    }

    fn colors(marks: &[ScrollbarMark]) -> Vec<&'static str> {
        marks.iter().map(|mark| mark.color).collect()
    }

    fn placed(marks: &[ScrollbarMark]) -> Vec<(usize, &'static str)> {
        marks.iter().map(|mark| (mark.from, mark.color)).collect()
    }

    #[test]
    fn marks_every_open_change_of_the_result_by_its_type_conflicts_on_top() {
        let marks = merge_stripe(MergePane::Result, &start());
        assert_eq!(colors(&marks), ["diff-conflict-stripe", "diff-modified-stripe", "diff-deleted-stripe"]);
        assert_eq!(marks[0].priority, 1);
    }

    #[test]
    fn marks_on_each_side_only_the_changes_that_side_made() {
        let left = merge_stripe(MergePane::Side(MergeSide::Left), &start());
        let expected_left = [(line(LEFT, 2), "diff-conflict-stripe"), (line(LEFT, 4), "diff-modified-stripe")];
        assert_eq!(placed(&left), expected_left);
        let right = merge_stripe(MergePane::Side(MergeSide::Right), &start());
        let expected_right = [(line(RIGHT, 2), "diff-conflict-stripe"), (line(RIGHT, 6), "diff-deleted-stripe")];
        assert_eq!(placed(&right), expected_right);
    }

    #[test]
    fn drops_a_change_once_it_is_resolved_and_a_sides_mark_once_that_side_is_done() {
        // The left side's change of line 4 is accepted, which settles it.
        let mut changes = start();
        changes[1].done = Done { left: true, right: true };
        let result = merge_stripe(MergePane::Result, &changes);
        assert_eq!(colors(&result), ["diff-conflict-stripe", "diff-deleted-stripe"]);
        let left = merge_stripe(MergePane::Side(MergeSide::Left), &changes);
        assert_eq!(colors(&left), ["diff-conflict-stripe"]);
    }

    // Not from a spec: the side panes offer Accept and Ignore on the first line of each open change of
    // their side, the result a wand on a conflict that merges by words, and the result tints its
    // changes.
    #[test]
    fn places_the_controls_and_tints_the_result() {
        let (left, base) = (Doc::new(LEFT), Doc::new(BASE));
        let mut changes = start();
        let marker = side_marker(&changes, MergeSide::Left, &left, line(LEFT, 2)).expect("the conflict is open");
        assert_eq!((marker.index, marker.accept_icon()), (0, "diff-arrow-right"));
        assert_eq!(marker.accept(true), MergeControl::Accept { index: 0, side: MergeSide::Left, resolve: true });
        assert_eq!(side_marker(&changes, MergeSide::Left, &left, line(LEFT, 6)), None);
        assert_eq!(side_marker(&changes, MergeSide::Right, &Doc::new(RIGHT), line(RIGHT, 4)), None);
        assert_eq!(wand_marker(&changes, &base, line(BASE, 2)), None);
        changes[0].auto = Some("B-both\n".to_owned());
        let wand = wand_marker(&changes, &base, line(BASE, 2));
        assert_eq!(wand, Some(MergeControl::ResolveAutomatically { index: 0 }));
        // An edited conflict is left to the user.
        assert_eq!(wand_marker(&changes, &base.replace(2, 3, "x"), line(BASE, 2)), None);
        changes[1].done = Done { left: true, right: true };
        let expected = [
            ResultMark::Line { from: 2, tint: MergeTint::Conflict },
            ResultMark::Line { from: 6, tint: MergeTint::Resolved },
            ResultMark::Line { from: 10, tint: MergeTint::Change },
        ];
        assert_eq!(result_marks(&changes, &base), expected);
        let added = change(MergeKind::Left, (4, 4), ["new\n", "", ""], (4, 8), (4, 4));
        assert_eq!(
            result_marks(std::slice::from_ref(&added), &base),
            [ResultMark::Point { at: 4, tint: MergeTint::Change }]
        );
        assert_eq!(colors(&merge_stripe(MergePane::Result, &[added])), ["diff-inserted-stripe"]);
    }
}
