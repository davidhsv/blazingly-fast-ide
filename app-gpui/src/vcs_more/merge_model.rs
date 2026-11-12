//! The merge editor's model, after IntelliJ's `MergeConflictModel`: the result starts as the base,
//! and every change the sides made to it waits there to be accepted from either side, ignored or
//! resolved. A change is settled once both sides are; a conflict can take one side, then the other
//! (which is appended), as in IntelliJ.
//!
//! Operations do not change the model: they return the [`MergeTransaction`] that does, or `None`
//! when there is nothing to do, and [`MergeModel::dispatch`] carries it out as one undo step.
//! Offsets are UTF-16 code units.

use super::merge_diff3::merge_changes;
use crate::vcs_diff::doc::{Doc, utf16_len};
use crate::vcs_diff::merge_gutters::{Done, MergeChange, MergeKind, MergeSide};

/// The result's starting text, the base, and the changes waiting in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeStart {
    pub text: String,
    pub changes: Vec<MergeChange>,
}

/// `text` with every line break as one `\n`, which is how the result counts its offsets.
fn normalize_breaks(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Compares the three versions: the result starts as the base, with the changes of both sides
/// pending in it.
pub fn start_merge(left: &str, base: &str, right: &str) -> MergeStart {
    let left = normalize_breaks(left);
    let base = normalize_breaks(base);
    let right = normalize_breaks(right);
    let changes = match merge_changes(&left, &base, &right) {
        Some(found) => found
            .into_iter()
            .map(|c| MergeChange {
                from: c.base_from,
                to: c.base_to,
                kind: c.kind,
                left: c.left,
                base: c.base,
                right: c.right,
                left_range: (c.left_from, c.left_to),
                right_range: (c.right_from, c.right_to),
                auto: c.auto,
                done: Done::default(),
                oneside: false,
            })
            .collect(),
        // Too different to compare line by line: the whole file is one change.
        None => whole_file(&left, &base, &right).into_iter().collect(),
    };
    MergeStart { text: base, changes }
}

/// The one change of versions that cannot be compared line by line; none when they are equal.
fn whole_file(left: &str, base: &str, right: &str) -> Option<MergeChange> {
    if left == base && right == base {
        return None;
    }
    let kind = if left == right {
        MergeKind::Both
    } else if left == base {
        MergeKind::Right
    } else if right == base {
        MergeKind::Left
    } else {
        MergeKind::Conflict
    };
    Some(MergeChange {
        from: 0,
        to: utf16_len(base),
        kind,
        left: left.to_owned(),
        base: base.to_owned(),
        right: right.to_owned(),
        left_range: (0, utf16_len(left)),
        right_range: (0, utf16_len(right)),
        auto: None,
        done: Done::default(),
        oneside: false,
    })
}

/// Whose changes an automatic resolution takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AutoSide {
    Side(MergeSide),
    /// Whatever fits: the changes of either side, and the word-level merge of a conflict.
    Base,
}

/// What the user can do with the changes in the merge editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MergeAction {
    /// Take the side's text; with `resolve`, settle the change with it.
    Accept { side: MergeSide, resolve: bool },
    /// Leave the side's change out; without a side, both sides'.
    Ignore { side: Option<MergeSide> },
    /// Resolve without a decision.
    Auto { side: AutoSide },
}

/// One replacement of the result's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeEdit {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

/// A step of the merge: edits of the result and the new state of some changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MergeTransaction {
    /// Ordered and apart from each other, in offsets of the document before the step.
    pub edits: Vec<MergeEdit>,
    /// The changes to put in the model by index, in offsets of the document after the step.
    pub changes: Vec<(usize, MergeChange)>,
}

impl MergeTransaction {
    /// An edit of the result by the user.
    pub fn edit(from: usize, to: usize, insert: &str) -> Self {
        Self { edits: vec![MergeEdit { from, to, insert: insert.to_owned() }], changes: Vec::new() }
    }
}

/// Which side of an insertion a position at it stays on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Assoc {
    Before,
    After,
}

/// Where `pos` of the document before `edits` is after them. A position inside replaced text moves
/// to the edge `assoc` names; one at the start of replaced text stays before the replacement.
fn map_pos(edits: &[MergeEdit], pos: usize, assoc: Assoc) -> usize {
    let mut delta = 0_isize;
    for edit in edits {
        if edit.from > pos {
            break;
        }
        let inserted = utf16_len(&edit.insert);
        let removed = edit.to.saturating_sub(edit.from);
        let insertion_before = edit.to == pos && assoc == Assoc::Before && removed == 0;
        if edit.to > pos || insertion_before {
            let start = edit.from.saturating_add_signed(delta);
            return if pos == edit.from || assoc == Assoc::Before { start } else { start + inserted };
        }
        delta += inserted as isize - removed as isize;
    }
    pos.saturating_add_signed(delta)
}

fn done_on(c: &MergeChange, side: MergeSide) -> bool {
    match side {
        MergeSide::Left => c.done.left,
        MergeSide::Right => c.done.right,
    }
}

fn with_done(c: &MergeChange, side: MergeSide) -> MergeChange {
    let done = match side {
        MergeSide::Left => Done { left: true, ..c.done },
        MergeSide::Right => Done { right: true, ..c.done },
    };
    MergeChange { done, ..c.clone() }
}

fn text_of(c: &MergeChange, side: MergeSide) -> &str {
    match side {
        MergeSide::Left => &c.left,
        MergeSide::Right => &c.right,
    }
}

const fn opposite(side: MergeSide) -> MergeSide {
    match side {
        MergeSide::Left => MergeSide::Right,
        MergeSide::Right => MergeSide::Left,
    }
}

fn settled(c: &MergeChange) -> MergeChange {
    MergeChange { done: Done { left: true, right: true }, ..c.clone() }
}

/// A change's new state, and the edit of the result that goes with it.
struct Plan {
    index: usize,
    next: MergeChange,
    edit: Option<MergeEdit>,
}

fn replace(c: &MergeChange, insert: &str) -> Option<MergeEdit> {
    Some(MergeEdit { from: c.from, to: c.to, insert: insert.to_owned() })
}

/// The transaction applying `plans`, or `None` when there is nothing to do.
fn commit(plans: Vec<Plan>) -> Option<MergeTransaction> {
    if plans.is_empty() {
        return None;
    }
    let mut edits: Vec<MergeEdit> = plans.iter().filter_map(|plan| plan.edit.clone()).collect();
    edits.sort_by_key(|edit| edit.from);
    let changes = plans
        .into_iter()
        .map(|plan| {
            let from = map_pos(&edits, plan.next.from, Assoc::Before);
            let to = map_pos(&edits, plan.next.to, Assoc::After);
            (plan.index, MergeChange { from, to, ..plan.next })
        })
        .collect();
    Some(MergeTransaction { edits, changes })
}

/// Takes the text of `side` for the change (IntelliJ's `replaceChange`). A conflict stays open for
/// the other side unless `resolve` settles it, or the other side has nothing to add.
fn accept(doc: &Doc, c: &MergeChange, index: usize, side: MergeSide, resolve: bool) -> Option<Plan> {
    if done_on(c, side) {
        return None;
    }
    if !c.changes_side(side) {
        return Some(Plan { index, next: settled(c), edit: None });
    }
    let text = text_of(c, side);
    if !c.is_conflict() {
        return Some(Plan { index, next: settled(c), edit: replace(c, text) });
    }
    if !c.oneside {
        let partial = !resolve && !text_of(c, opposite(side)).is_empty();
        let next = if partial { MergeChange { oneside: true, ..with_done(c, side) } } else { settled(c) };
        return Some(Plan { index, next, edit: replace(c, text) });
    }
    // The other side's text is already there; this one follows it, on a line of its own.
    let there = doc.slice_string(c.from, c.to);
    let separator = if !there.is_empty() && !there.ends_with('\n') { "\n" } else { "" };
    Some(Plan {
        index,
        next: if resolve { settled(c) } else { with_done(c, side) },
        edit: Some(MergeEdit { from: c.to, to: c.to, insert: format!("{separator}{text}") }),
    })
}

/// Ignores the change of `side` (IntelliJ's `ignoreChange`): the result keeps what it has.
fn ignore(c: &MergeChange, index: usize, side: Option<MergeSide>) -> Plan {
    let next = match side {
        Some(side) if c.is_conflict() => with_done(c, side),
        _ => settled(c),
    };
    Plan { index, next, edit: None }
}

/// Whether the user edited the change's text in the result: it is then left alone by automatic
/// resolving.
fn edited(doc: &Doc, c: &MergeChange) -> bool {
    doc.slice_string(c.from, c.to) != c.base
}

/// Whether the change can be resolved without a decision (IntelliJ's
/// `canResolveChangeAutomatically`).
pub fn can_resolve_automatically(doc: &Doc, c: &MergeChange, side: AutoSide) -> bool {
    if edited(doc, c) {
        return false;
    }
    if c.is_conflict() {
        return side == AutoSide::Base && c.auto.is_some() && !c.done.left && !c.done.right;
    }
    !c.is_resolved()
        && match side {
            AutoSide::Base => true,
            AutoSide::Side(side) => c.changes_side(side),
        }
}

fn resolve_automatically(doc: &Doc, c: &MergeChange, index: usize, side: AutoSide) -> Option<Plan> {
    if !can_resolve_automatically(doc, c, side) {
        return None;
    }
    if c.is_conflict() {
        let merged = c.auto.as_deref()?;
        return Some(Plan { index, next: settled(c), edit: replace(c, merged) });
    }
    let from = match side {
        AutoSide::Side(side) => side,
        AutoSide::Base if c.changes_side(MergeSide::Left) => MergeSide::Left,
        AutoSide::Base => MergeSide::Right,
    };
    Some(Plan { index, next: settled(c), edit: replace(c, text_of(c, from)) })
}

/// Whether `action` applies to the change now (the enabled state of IntelliJ's actions).
pub fn can_apply(doc: &Doc, c: &MergeChange, action: MergeAction) -> bool {
    match action {
        MergeAction::Accept { side, .. } | MergeAction::Ignore { side: Some(side) } => !done_on(c, side),
        MergeAction::Ignore { side: None } => !c.is_resolved(),
        MergeAction::Auto { side } => can_resolve_automatically(doc, c, side),
    }
}

fn plan(doc: &Doc, c: &MergeChange, index: usize, action: MergeAction) -> Option<Plan> {
    match action {
        MergeAction::Accept { side, resolve } => accept(doc, c, index, side, resolve),
        MergeAction::Ignore { side } => Some(ignore(c, index, side)),
        MergeAction::Auto { side } => resolve_automatically(doc, c, index, side),
    }
}

/// What the toolbar needs to know: how much is left, and which of its commands do something.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MergeSummary {
    pub pending: usize,
    pub conflicts: usize,
    /// Apply Non-Conflicting Changes from the left side, from either side, from the right side.
    pub left: bool,
    pub all: bool,
    pub right: bool,
    /// Resolve Simple Conflicts.
    pub simple: bool,
    /// Revert Conflict Resolution.
    pub revert: bool,
}

/// The result of a merge and the changes in it, in document order. Typing at the edge of a change
/// stays inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeState {
    pub doc: Doc,
    pub changes: Vec<MergeChange>,
}

impl MergeState {
    pub fn new(start: MergeStart) -> Self {
        Self { doc: Doc::new(&start.text), changes: start.changes }
    }

    /// Applies `action` to the changes at `indexes` that it applies to.
    pub fn apply_to(&self, indexes: &[usize], action: MergeAction) -> Option<MergeTransaction> {
        let plans = indexes
            .iter()
            .filter_map(|index| {
                let c = self.changes.get(*index)?;
                if can_apply(&self.doc, c, action) { plan(&self.doc, c, *index, action) } else { None }
            })
            .collect();
        commit(plans)
    }

    /// The indexes of the changes whose text, `range(change)` in `doc`, lies in the lines of
    /// `from`–`to`. A change that adds lines has an empty range: it is where its offset is. A caret
    /// at the start of the line after a change is not in the change.
    fn selected(
        &self,
        doc: &Doc,
        from: usize,
        to: usize,
        range: impl Fn(&MergeChange) -> Option<(usize, usize)>,
    ) -> Vec<usize> {
        let start = doc.line_at(from).from;
        let end = doc.len().min(doc.line_at(to).to + 1);
        self.changes
            .iter()
            .enumerate()
            .filter_map(|(index, c)| {
                let (a, b) = range(c)?;
                let hit = if a == b { a >= start && (a < end || a == doc.len()) } else { a < end && b > start };
                hit.then_some(index)
            })
            .collect()
    }

    /// The changes whose text lies in the lines of `from`–`to` of the result.
    pub fn changes_in(&self, from: usize, to: usize) -> Vec<usize> {
        self.selected(&self.doc, from, to, |c| Some((c.from, c.to)))
    }

    /// The changes of a side pane's version, `doc`, that lie in the lines of `from`–`to` of that
    /// pane.
    pub fn changes_in_side(&self, side: MergeSide, doc: &Doc, from: usize, to: usize) -> Vec<usize> {
        self.selected(doc, from, to, |c| {
            c.changes_side(side).then_some(match side {
                MergeSide::Left => c.left_range,
                MergeSide::Right => c.right_range,
            })
        })
    }

    fn all_indexes(&self) -> Vec<usize> {
        (0..self.changes.len()).collect()
    }

    /// Apply Non-Conflicting Changes from `side` ([`AutoSide::Base`]: from either).
    pub fn apply_non_conflicting(&self, side: AutoSide) -> Option<MergeTransaction> {
        let open = |index: &usize| !self.changes[*index].is_conflict();
        let indexes: Vec<usize> = self.all_indexes().into_iter().filter(open).collect();
        self.apply_to(&indexes, MergeAction::Auto { side })
    }

    pub fn has_non_conflicting(&self, side: AutoSide) -> bool {
        self.changes.iter().any(|c| !c.is_conflict() && can_resolve_automatically(&self.doc, c, side))
    }

    /// Resolve Simple Conflicts: every change that can be resolved without a decision.
    pub fn resolve_simple(&self) -> Option<MergeTransaction> {
        self.apply_to(&self.all_indexes(), MergeAction::Auto { side: AutoSide::Base })
    }

    pub fn has_simple(&self) -> bool {
        self.changes.iter().any(|c| can_resolve_automatically(&self.doc, c, AutoSide::Base))
    }

    /// Revert Conflict Resolution: the settled changes go back to the base.
    pub fn revert_resolved(&self) -> Option<MergeTransaction> {
        let plans = self
            .changes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.is_resolved())
            .map(|(index, c)| Plan {
                index,
                edit: replace(c, &c.base),
                next: MergeChange { oneside: false, done: Done::default(), ..c.clone() },
            })
            .collect();
        commit(plans)
    }

    /// The changes still to be settled.
    pub fn unresolved(&self) -> Vec<&MergeChange> {
        self.changes.iter().filter(|c| !c.is_resolved()).collect()
    }

    pub fn summarize(&self) -> MergeSummary {
        let open = self.unresolved();
        MergeSummary {
            pending: open.len(),
            conflicts: open.iter().filter(|c| c.is_conflict()).count(),
            left: self.has_non_conflicting(AutoSide::Side(MergeSide::Left)),
            all: self.has_non_conflicting(AutoSide::Base),
            right: self.has_non_conflicting(AutoSide::Side(MergeSide::Right)),
            simple: self.has_simple(),
            revert: self.changes.iter().any(MergeChange::is_resolved),
        }
    }

    /// Carries `transaction` out and returns the one that undoes it: the inverse edits, and the
    /// state the changes it sets had before, so an undone resolution can be made again.
    pub fn apply(&mut self, transaction: &MergeTransaction) -> MergeTransaction {
        let before = transaction
            .changes
            .iter()
            .filter_map(|(index, _)| Some((*index, self.changes.get(*index)?.clone())))
            .collect();
        let mut inverse = Vec::with_capacity(transaction.edits.len());
        if !transaction.edits.is_empty() {
            let old = self.doc.units();
            let mut units = Vec::with_capacity(old.len());
            let mut pos = 0;
            for edit in &transaction.edits {
                let from = edit.from.clamp(pos, old.len());
                let to = edit.to.clamp(from, old.len());
                units.extend_from_slice(&old[pos..from]);
                let start = units.len();
                units.extend(edit.insert.encode_utf16());
                let removed = String::from_utf16_lossy(&old[from..to]);
                inverse.push(MergeEdit { from: start, to: units.len(), insert: removed });
                pos = to;
            }
            units.extend_from_slice(&old[pos..]);
            for c in &mut self.changes {
                c.from = map_pos(&transaction.edits, c.from, Assoc::Before);
                c.to = map_pos(&transaction.edits, c.to, Assoc::After);
            }
            self.doc = Doc::from_units(units);
        }
        for (index, change) in &transaction.changes {
            if let Some(slot) = self.changes.get_mut(*index) {
                *slot = change.clone();
            }
        }
        MergeTransaction { edits: inverse, changes: before }
    }
}

/// A merge with its history: every transaction is one step to undo, and undoing brings back the
/// state of the changes with the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeModel {
    state: MergeState,
    /// What undoes each step made, the last one on top.
    done: Vec<MergeTransaction>,
    /// What makes each undone step again.
    undone: Vec<MergeTransaction>,
}

impl MergeModel {
    /// The merge of three versions of a file, nothing decided yet.
    pub fn start(left: &str, base: &str, right: &str) -> Self {
        Self { state: MergeState::new(start_merge(left, base, right)), done: Vec::new(), undone: Vec::new() }
    }

    pub fn state(&self) -> &MergeState {
        &self.state
    }

    pub fn doc(&self) -> &Doc {
        &self.state.doc
    }

    pub fn changes(&self) -> &[MergeChange] {
        &self.state.changes
    }

    /// The result's text.
    pub fn text(&self) -> String {
        self.state.doc.to_string()
    }

    /// Carries `transaction` out as a step of its own.
    pub fn dispatch(&mut self, transaction: &MergeTransaction) {
        let inverse = self.state.apply(transaction);
        self.done.push(inverse);
        self.undone.clear();
    }

    /// The user replaced `[from, to)` of the result with `insert`.
    pub fn edit(&mut self, from: usize, to: usize, insert: &str) {
        self.dispatch(&MergeTransaction::edit(from, to, insert));
    }

    /// Applies `action` to the changes at `indexes`; whether there was anything to do.
    pub fn act(&mut self, indexes: &[usize], action: MergeAction) -> bool {
        let Some(transaction) = self.state.apply_to(indexes, action) else {
            return false;
        };
        self.dispatch(&transaction);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    /// Takes the last step back; `false` when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.done.pop() else {
            return false;
        };
        let inverse = self.state.apply(&step);
        self.undone.push(inverse);
        true
    }

    /// Makes the last undone step again; `false` when there is none.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.undone.pop() else {
            return false;
        };
        let inverse = self.state.apply(&step);
        self.done.push(inverse);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::super::merge_diff3::test_support::lines;
    use super::*;

    const LEFT: MergeSide = MergeSide::Left;
    const RIGHT: MergeSide = MergeSide::Right;

    fn accept(side: MergeSide, resolve: bool) -> MergeAction {
        MergeAction::Accept { side, resolve }
    }

    fn ignore(side: Option<MergeSide>) -> MergeAction {
        MergeAction::Ignore { side }
    }

    const AUTO: MergeAction = MergeAction::Auto { side: AutoSide::Base };

    /// Carries out a transaction that must exist.
    fn run(model: &mut MergeModel, transaction: Option<MergeTransaction>) {
        model.dispatch(&transaction.expect("there is something to do"));
    }

    /// Applies an action that must have something to do.
    fn act(model: &mut MergeModel, indexes: &[usize], action: MergeAction) {
        assert!(model.act(indexes, action), "{action:?} applies to {indexes:?}");
    }

    fn base() -> String {
        lines(&["a", "b", "c", "d", "e", "f", "g"])
    }

    fn left() -> String {
        lines(&["a", "L", "c", "d", "e", "f", "G"])
    }

    fn right() -> String {
        lines(&["a", "R", "c", "D", "e", "f", "G"])
    }

    /// Changes: 0 conflict (b / L / R), 1 right-only (d), 2 identical on both (g).
    fn merge() -> MergeModel {
        MergeModel::start(&left(), &base(), &right())
    }

    fn resolved(model: &MergeModel) -> Vec<bool> {
        model.changes().iter().map(MergeChange::is_resolved).collect()
    }

    fn range(model: &MergeModel, index: usize) -> (usize, usize) {
        (model.changes()[index].from, model.changes()[index].to)
    }

    /// More lines than can be compared, none of which occurs under another prefix.
    fn many(prefix: &str) -> String {
        (0..70_000).map(|i| format!("{prefix}{i}")).collect::<Vec<_>>().join("\n")
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › keeps CRLF changes in CodeMirror coordinates after earlier lines
    #[test]
    fn keeps_crlf_changes_in_document_coordinates_after_earlier_lines() {
        let base = "标题😀\r\nunchanged\r\nbase\r\ntail\r\n";
        let left = base.replace("base", "ours中文");
        let right = base.replace("base", "theirs中文");
        let mut m = MergeModel::start(&left, base, &right);
        let (from, to) = range(&m, 0);
        assert_eq!(m.doc().slice_string(from, to), "base\n");
        act(&mut m, &[0], accept(LEFT, true));
        assert_eq!(m.text().replace('\n', "\r\n"), left);
        assert!(m.undo());
        assert_eq!(m.state().unresolved().len(), 1);
        assert_eq!(m.text().replace('\n', "\r\n"), base);
        act(&mut m, &[0], accept(RIGHT, true));
        assert_eq!(m.text().replace('\n', "\r\n"), right);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › preserves a missing final newline when resolving after non-ASCII text
    #[test]
    fn preserves_a_missing_final_newline_when_resolving_after_non_ascii_text() {
        let base = "标题😀\nunchanged\nbase";
        let mut m = MergeModel::start(&base.replace("base", "左侧🚀"), base, &base.replace("base", "右侧🧪"));
        act(&mut m, &[0], accept(LEFT, true));
        assert_eq!(m.text(), "标题😀\nunchanged\n左侧🚀");
        m.undo();
        act(&mut m, &[0], accept(RIGHT, true));
        assert_eq!(m.text(), "标题😀\nunchanged\n右侧🧪");
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › starts from the base with every change pending
    #[test]
    fn starts_from_the_base_with_every_change_pending() {
        let m = merge();
        assert_eq!(m.text(), base());
        let kinds: Vec<MergeKind> = m.changes().iter().map(|c| c.kind).collect();
        assert_eq!(kinds, [MergeKind::Conflict, MergeKind::Right, MergeKind::Both]);
        assert_eq!(m.state().unresolved().len(), 3);
        assert_eq!(range(&m, 0), (2, 4));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › has no changes for identical versions, and one whole-file conflict for files too different to compare
    #[test]
    fn has_no_changes_for_identical_versions_and_one_whole_file_conflict_for_files_too_different_to_compare() {
        assert!(MergeModel::start(&base(), &base(), &base()).changes().is_empty());
        let big = MergeModel::start(&many("l"), &many("b"), &many("r"));
        assert_eq!(big.changes().len(), 1);
        assert_eq!((big.changes()[0].kind, big.changes()[0].from), (MergeKind::Conflict, 0));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › keeps what both sides did when the file is too different to compare and they agree
    #[test]
    fn keeps_what_both_sides_did_when_the_file_is_too_different_to_compare_and_they_agree() {
        let (b, same, other) = (many("b"), many("s"), many("o"));
        let mut agreed = MergeModel::start(&same, &b, &same);
        assert_eq!(agreed.changes().len(), 1);
        assert_eq!(agreed.changes()[0].kind, MergeKind::Both);
        act(&mut agreed, &[0], accept(LEFT, false));
        assert_eq!(agreed.text(), same);
        // One side left the file as it was: the other's version is the change.
        assert_eq!(MergeModel::start(&b, &b, &other).changes()[0].kind, MergeKind::Right);
        assert!(MergeModel::start(&b, &b, &b).changes().is_empty());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › the model › gives the position of a change in each version
    #[test]
    fn gives_the_position_of_a_change_in_each_version() {
        let m = merge();
        let c = &m.changes()[0];
        assert_eq!(Doc::new(&left()).slice_string(c.left_range.0, c.left_range.1), "L\n");
        assert_eq!(Doc::new(&right()).slice_string(c.right_range.0, c.right_range.1), "R\n");
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › takes the text, and leaves a conflict open for the other side
    #[test]
    fn takes_the_text_and_leaves_a_conflict_open_for_the_other_side() {
        let mut m = merge();
        act(&mut m, &[0], accept(LEFT, false));
        assert_eq!(m.text(), lines(&["a", "L", "c", "d", "e", "f", "g"]));
        let c = &m.changes()[0];
        assert!(c.oneside);
        assert_eq!(c.done, Done { left: true, right: false });
        assert!(!c.is_resolved());
        assert!(!can_apply(m.doc(), c, accept(LEFT, false)));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › appends the other side after the first, in either order
    #[test]
    fn appends_the_other_side_after_the_first_in_either_order() {
        let mut m = merge();
        act(&mut m, &[0], accept(RIGHT, false));
        act(&mut m, &[0], accept(LEFT, false));
        assert_eq!(m.text(), lines(&["a", "R", "L", "c", "d", "e", "f", "g"]));
        assert!(m.changes()[0].is_resolved());
        assert_eq!(range(&m, 0), (2, 6));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › starts a line of its own when the first side did not end its last line
    #[test]
    fn starts_a_line_of_its_own_when_the_first_side_did_not_end_its_last_line() {
        let mut m = MergeModel::start("a\nL", "a\nb", "a\nR");
        act(&mut m, &[0], accept(LEFT, false));
        act(&mut m, &[0], accept(RIGHT, false));
        assert_eq!(m.text(), "a\nL\nR");
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › settles the change at once when resolving using a side
    #[test]
    fn settles_the_change_at_once_when_resolving_using_a_side() {
        let mut m = merge();
        act(&mut m, &[0], accept(LEFT, true));
        assert_eq!(m.text(), lines(&["a", "L", "c", "d", "e", "f", "g"]));
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › settles a conflict whose other side deleted the lines, there being nothing to append
    #[test]
    fn settles_a_conflict_whose_other_side_deleted_the_lines_there_being_nothing_to_append() {
        let mut m = MergeModel::start(&lines(&["a", "c"]), &lines(&["a", "b", "c"]), &lines(&["a", "B", "c"]));
        act(&mut m, &[0], accept(RIGHT, false));
        assert_eq!(m.text(), lines(&["a", "B", "c"]));
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › applies a one-sided change from its side, and settles it from the other without a text change
    #[test]
    fn applies_a_one_sided_change_from_its_side_and_settles_it_from_the_other_without_a_text_change() {
        let mut m = merge();
        act(&mut m, &[1], accept(RIGHT, false));
        assert_eq!(m.text(), lines(&["a", "b", "c", "D", "e", "f", "g"]));
        let mut n = merge();
        act(&mut n, &[1], accept(LEFT, false));
        assert_eq!(n.text(), base());
        assert!(n.changes()[1].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › accepting from a side › keeps the other changes where they are as the text grows and shrinks
    #[test]
    fn keeps_the_other_changes_where_they_are_as_the_text_grows_and_shrinks() {
        let mut m = merge();
        act(&mut m, &[0], accept(RIGHT, false));
        act(&mut m, &[0], accept(LEFT, false));
        act(&mut m, &[1], accept(RIGHT, false));
        act(&mut m, &[2], accept(LEFT, false));
        assert_eq!(m.text(), lines(&["a", "R", "L", "c", "D", "e", "f", "G"]));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › ignoring › settles one side of a conflict and leaves the text; the other side then replaces it
    #[test]
    fn settles_one_side_of_a_conflict_and_leaves_the_text_the_other_side_then_replaces_it() {
        let mut m = merge();
        act(&mut m, &[0], ignore(Some(LEFT)));
        assert_eq!(m.text(), base());
        assert_eq!(m.changes()[0].done, Done { left: true, right: false });
        act(&mut m, &[0], accept(RIGHT, false));
        assert_eq!(m.text(), lines(&["a", "R", "c", "d", "e", "f", "g"]));
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › ignoring › settles a conflict with the base when ignoring both sides
    #[test]
    fn settles_a_conflict_with_the_base_when_ignoring_both_sides() {
        let mut m = merge();
        act(&mut m, &[0], ignore(None));
        assert_eq!(m.text(), base());
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › ignoring › settles a one-sided change whichever side ignores it
    #[test]
    fn settles_a_one_sided_change_whichever_side_ignores_it() {
        let mut m = merge();
        act(&mut m, &[1], ignore(Some(RIGHT)));
        assert!(m.changes()[1].is_resolved());
    }

    const WORDS_BASE: &str = "let a = 1 + b;\n";
    const WORDS_LEFT: &str = "let x = 1 + b;\n";
    const WORDS_RIGHT: &str = "let a = 1 + 2;\n";

    // src/features/vcs/__tests__/mergeModel.test.ts › resolving automatically › merges a conflict whose sides changed different words
    #[test]
    fn merges_a_conflict_whose_sides_changed_different_words() {
        let mut m = MergeModel::start(WORDS_LEFT, WORDS_BASE, WORDS_RIGHT);
        assert!(m.state().has_simple());
        act(&mut m, &[0], AUTO);
        assert_eq!(m.text(), "let x = 1 + 2;\n");
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › resolving automatically › does not touch a conflict the user edited, or one already partly decided
    #[test]
    fn does_not_touch_a_conflict_the_user_edited_or_one_already_partly_decided() {
        let mut m = MergeModel::start(WORDS_LEFT, WORDS_BASE, WORDS_RIGHT);
        m.edit(0, 0, "// ");
        assert!(!can_apply(m.doc(), &m.changes()[0], AUTO));
        assert!(!m.state().has_simple());
        let mut n = MergeModel::start(WORDS_LEFT, WORDS_BASE, WORDS_RIGHT);
        act(&mut n, &[0], ignore(Some(LEFT)));
        assert!(!n.state().has_simple());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › resolving automatically › does not offer it for a conflict of the same word
    #[test]
    fn does_not_offer_it_for_a_conflict_of_the_same_word() {
        let m = MergeModel::start("let x = 1;\n", "let a = 1;\n", "let y = 1;\n");
        assert!(!m.state().has_simple());
        assert_eq!(m.state().apply_to(&[0], AUTO), None);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › resolving automatically › resolves the simple conflicts and the non-conflicting changes together, and no more
    #[test]
    fn resolves_the_simple_conflicts_and_the_non_conflicting_changes_together_and_no_more() {
        let pad = |mid: &str, end: &str| lines(&["1", "2", "3", "4", mid, "5", "6", "7", end]);
        // 0: one side changed a line; 1: a real conflict.
        let mut m = MergeModel::start(&pad("x", "L"), &pad("a", "b"), &pad("a", "R"));
        let simple = m.state().resolve_simple();
        run(&mut m, simple);
        assert_eq!(m.text(), pad("x", "b"));
        assert_eq!(resolved(&m), [true, false]);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › applying the non-conflicting changes › applies those of the chosen side only
    #[test]
    fn applies_those_of_the_chosen_side_only() {
        let mut m = merge();
        assert!(m.state().has_non_conflicting(AutoSide::Side(LEFT)));
        let from_left = m.state().apply_non_conflicting(AutoSide::Side(LEFT));
        run(&mut m, from_left);
        // The right-only change is not the left's to apply; the identical one is.
        assert_eq!(m.text(), lines(&["a", "b", "c", "d", "e", "f", "G"]));
        assert_eq!(resolved(&m), [false, false, true]);
        assert!(!m.state().has_non_conflicting(AutoSide::Side(LEFT)));
        assert!(m.state().has_non_conflicting(AutoSide::Side(RIGHT)));
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › applying the non-conflicting changes › applies all of them from the base, and leaves conflicts
    #[test]
    fn applies_all_of_them_from_the_base_and_leaves_conflicts() {
        let mut m = merge();
        let all = m.state().apply_non_conflicting(AutoSide::Base);
        run(&mut m, all);
        assert_eq!(m.text(), lines(&["a", "b", "c", "D", "e", "f", "G"]));
        assert_eq!(resolved(&m), [false, true, true]);
        assert_eq!(m.state().apply_non_conflicting(AutoSide::Base), None);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › history › undoes and redoes a resolution with the state of the change
    #[test]
    fn undoes_and_redoes_a_resolution_with_the_state_of_the_change() {
        let mut m = merge();
        act(&mut m, &[0], accept(LEFT, false));
        act(&mut m, &[0], accept(RIGHT, false));
        assert!(m.changes()[0].is_resolved());

        m.undo();
        assert_eq!(m.text(), lines(&["a", "L", "c", "d", "e", "f", "g"]));
        let c = &m.changes()[0];
        assert_eq!((c.oneside, c.done, c.from, c.to), (true, Done { left: true, right: false }, 2, 4));
        m.undo();
        assert_eq!(m.text(), base());
        let c = &m.changes()[0];
        assert_eq!((c.oneside, c.done, c.from, c.to), (false, Done::default(), 2, 4));

        act(&mut m, &[0], accept(RIGHT, true));
        assert_eq!(m.text(), lines(&["a", "R", "c", "d", "e", "f", "g"]));
        m.undo();
        m.redo();
        assert!(m.changes()[0].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › history › undoes an ignore, which changes no text
    #[test]
    fn undoes_an_ignore_which_changes_no_text() {
        let mut m = merge();
        act(&mut m, &[1], ignore(Some(RIGHT)));
        m.undo();
        assert!(!m.changes()[1].is_resolved());
        m.redo();
        assert!(m.changes()[1].is_resolved());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › history › undoes a bulk resolution of several changes in one step
    #[test]
    fn undoes_a_bulk_resolution_of_several_changes_in_one_step() {
        let mut m = merge();
        let all = m.state().apply_non_conflicting(AutoSide::Base);
        run(&mut m, all);
        m.undo();
        assert_eq!(m.text(), base());
        assert_eq!(resolved(&m), [false, false, false]);
        let starts: Vec<usize> = m.changes().iter().map(|c| c.from).collect();
        assert_eq!(starts, [2, 6, 12]);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › reverting › sends the settled changes back to the base and leaves the pending ones
    #[test]
    fn sends_the_settled_changes_back_to_the_base_and_leaves_the_pending_ones() {
        let mut m = merge();
        act(&mut m, &[0], accept(LEFT, true));
        act(&mut m, &[2], accept(LEFT, false));
        act(&mut m, &[1], accept(RIGHT, false));
        let revert = m.state().revert_resolved();
        run(&mut m, revert);
        assert_eq!(m.text(), base());
        assert_eq!(resolved(&m), [false, false, false]);
        assert_eq!(m.state().revert_resolved(), None);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › reverting › does not revert a conflict that is only half decided
    #[test]
    fn does_not_revert_a_conflict_that_is_only_half_decided() {
        let mut m = merge();
        act(&mut m, &[0], accept(LEFT, false));
        assert_eq!(m.state().revert_resolved(), None);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › editing the result › keeps typing at the edge of a change inside it, and shifts the changes below
    #[test]
    fn keeps_typing_at_the_edge_of_a_change_inside_it_and_shifts_the_changes_below() {
        let mut m = merge();
        m.edit(0, 0, "top\n");
        assert_eq!(range(&m, 0), (6, 8));
        m.edit(8, 8, "x");
        assert_eq!(range(&m, 0), (6, 9));
        // "c\n" lies between the first two changes.
        assert_eq!(m.changes()[1].from, m.changes()[0].to + 2);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › selection › finds the changes in the selected lines of the result
    #[test]
    fn finds_the_changes_in_the_selected_lines_of_the_result() {
        let m = merge();
        let state = m.state();
        assert_eq!(state.changes_in(2, 2), [0]);
        assert!(state.changes_in(0, 0).is_empty());
        // A caret at the start of the line after a change is not in the change.
        assert!(state.changes_in(4, 4).is_empty());
        assert_eq!(state.changes_in(0, state.doc.len()), [0, 1, 2]);
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › selection › finds the changes in the selected lines of a side
    #[test]
    fn finds_the_changes_in_the_selected_lines_of_a_side() {
        let m = merge();
        let side_doc = Doc::new(&left());
        assert_eq!(m.state().changes_in_side(LEFT, &side_doc, 2, 2), [0]);
        // The left side did not change the second change, so it has nothing there.
        assert!(m.state().changes_in_side(LEFT, &side_doc, 6, 6).is_empty());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › selection › finds an insertion at the point where it sits
    #[test]
    fn finds_an_insertion_at_the_point_where_it_sits() {
        let m = MergeModel::start(&lines(&["a", "N", "b"]), &lines(&["a", "b"]), &lines(&["a", "b"]));
        assert_eq!(m.state().changes_in(2, 2), [0]);
        assert!(m.state().changes_in(0, 0).is_empty());
    }

    // src/features/vcs/__tests__/mergeModel.test.ts › summary › counts what is left and which toolbar commands have something to do (simple conflicts include the non-conflicting changes, as in IntelliJ)
    #[test]
    fn counts_what_is_left_and_which_toolbar_commands_have_something_to_do() {
        let mut m = merge();
        let all_open =
            MergeSummary { pending: 3, conflicts: 1, left: true, all: true, right: true, simple: true, revert: false };
        assert_eq!(m.state().summarize(), all_open);
        let all = m.state().apply_non_conflicting(AutoSide::Base);
        run(&mut m, all);
        let conflict_left = MergeSummary {
            pending: 1,
            conflicts: 1,
            left: false,
            all: false,
            right: false,
            simple: false,
            revert: true,
        };
        assert_eq!(m.state().summarize(), conflict_left);
    }
}
