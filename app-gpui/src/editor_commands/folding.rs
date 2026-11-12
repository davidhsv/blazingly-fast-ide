//! Folding: the backend's fold ranges as offsets, and the Folding menu's commands (Collapse,
//! Expand, Collapse All, Expand to Level, Collapse/Expand Recursively, Fold Selection).
//!
//! The fold state is plain data the caller keeps: the folded ranges, and for every line the range
//! that line folds, if any (the backend's folds through [`foldable_lines`], merged with whatever
//! the syntax offers). A command answers the folds to add and remove, in order, and where the
//! selection goes when a caret would otherwise sit inside a new fold.

use super::regions::{RegionMarker, region_marker_at};
use super::{
    CommentTokensAt, Edit, EditState, Line, SelectionExt, caret, leading_space, map_after, map_before, utf16_len,
};
use ide_core::TextChange;
use ide_language::{FoldKind, FoldRange};

/// A foldable or folded range of text, `from` and `to` in UTF-16 offsets.
pub type FoldRegion = (u32, u32);

/// A backend fold range as offsets in the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackendFold {
    /// Where the fold starts: the end of its header line; for a custom region, its start marker.
    pub from: u32,
    pub to: u32,
    pub kind: FoldKind,
    /// A custom region (`//region` … `//endregion`): it folds from its start marker, which the
    /// placeholder names.
    pub custom: bool,
}

/// One change to the fold state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldEffect {
    Fold(FoldRegion),
    Unfold(FoldRegion),
}

/// What a folding command answers: the fold changes in order, and the selection when it moves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoldEdit {
    pub effects: Vec<FoldEffect>,
    pub selection: Option<Edit>,
}

impl FoldEdit {
    /// The effects alone; `None` when there are none (the command does not apply).
    fn effects(effects: Vec<FoldEffect>) -> Option<Self> {
        (!effects.is_empty()).then_some(Self { effects, selection: None })
    }

    /// The regions this edit folds, in order.
    pub fn folds(&self) -> impl Iterator<Item = FoldRegion> + '_ {
        self.effects.iter().filter_map(|effect| match effect {
            FoldEffect::Fold(region) => Some(*region),
            FoldEffect::Unfold(_) => None,
        })
    }
}

/// The fold state a command reads.
#[derive(Debug, Clone, Copy)]
pub struct FoldContext<'a> {
    /// For every 0-based line, the region folding that line hides, if it has one.
    pub foldable: &'a [Option<FoldRegion>],
    /// The folded ranges, in document order.
    pub folded: &'a [FoldRegion],
}

impl FoldContext<'_> {
    /// The region line `number` folds.
    pub fn foldable_at(&self, number: u32) -> Option<FoldRegion> {
        self.foldable.get(number as usize).copied().flatten()
    }

    /// `region` is folded as it is (not merely inside or around another fold).
    pub fn is_folded(&self, region: FoldRegion) -> bool {
        self.folded.contains(&region)
    }
}

/// A block whose `{` sits alone on its line (Allman style: C#, some Java and C) folds from the
/// declaration line above it, so the fold and the sticky line show `namespace Demo` or
/// `void F()` rather than a bare brace.
fn allman_header(state: &EditState, line: Line) -> Line {
    if line.text.trim() != "{" || line.number == 0 {
        return line;
    }
    let previous = state.line(line.number - 1);
    let code = previous.text.trim();
    let is_header = !code.is_empty()
        && !code.ends_with([';', '{', '}', ','])
        && !["//", "#", "*", "/*", "--"].into_iter().any(|prefix| code.starts_with(prefix));
    if is_header { previous } else { line }
}

/// The backend's line ranges as offsets in `state`, sorted by start (the wider first). A fold
/// starts at the end of its header line, and a `}`, `)` or `]` opening its last line stays
/// visible after the placeholder; a custom region folds from its start marker to the end of its
/// end marker.
pub fn folds_from_ranges(state: &EditState, ranges: &[FoldRange], comments: CommentTokensAt<'_>) -> Vec<BackendFold> {
    let lines = state.line_count();
    let mut out = Vec::new();
    for range in ranges {
        if range.start_line >= lines || range.end_line >= lines || range.end_line <= range.start_line {
            continue;
        }
        let first = state.line(range.start_line);
        let end = state.line(range.end_line);
        if matches!(region_marker_at(&first, comments), Some(RegionMarker::Start { .. })) {
            let from = first.from + utf16_len(leading_space(&first.text));
            if end.to > from {
                out.push(BackendFold { from, to: end.to, kind: range.kind, custom: true });
            }
            continue;
        }
        let start = allman_header(state, first);
        let trimmed = end.text.trim_start();
        let to =
            if trimmed.starts_with(['}', ')', ']']) { end.from + utf16_len(leading_space(&end.text)) } else { end.to };
        if to > start.to {
            out.push(BackendFold { from: start.to, to, kind: range.kind, custom: false });
        }
    }
    out.sort_by(|a, b| a.from.cmp(&b.from).then(b.to.cmp(&a.to)));
    out
}

/// Backend folds after `changes`, until the backend answers for the new text: a fold keeps
/// covering its text and is dropped when nothing of it is left.
pub fn map_backend_folds(folds: &[BackendFold], changes: &[TextChange]) -> Vec<BackendFold> {
    folds
        .iter()
        .filter_map(|fold| {
            let (from, to) = (map_before(fold.from, changes), map_after(fold.to, changes));
            (to > from).then_some(BackendFold { from, to, ..*fold })
        })
        .collect()
}

/// The widest of the backend folds (sorted by start) that line `from..=to` folds: one starting
/// at the end of the line, or a custom region starting on it.
fn fold_at_line(folds: &[BackendFold], from: u32, to: u32) -> Option<FoldRegion> {
    let first = folds.partition_point(|fold| fold.from < from);
    let mut best: Option<&BackendFold> = None;
    for fold in folds[first..].iter().take_while(|fold| fold.from <= to) {
        if (fold.from == to || fold.custom) && best.is_none_or(|best| fold.to > best.to) {
            best = Some(fold);
        }
    }
    best.map(|fold| (fold.from, fold.to))
}

/// For every line, the backend fold it offers (see [`FoldContext::foldable`]). `folds` are
/// sorted by start, as [`folds_from_ranges`] answers them.
pub fn foldable_lines(state: &EditState, folds: &[BackendFold]) -> Vec<Option<FoldRegion>> {
    (0..state.line_count())
        .map(|number| {
            let line = state.line(number);
            fold_at_line(folds, line.from, line.to)
        })
        .collect()
}

/// Lines scanned upwards from the caret when looking for the enclosing fold region.
const MAX_SCAN_LINES: usize = 2000;

/// The innermost region not folded yet that holds `pos`: the region of its own line first, then
/// the ones of the lines above that contain it (Collapse).
pub fn enclosing_fold_range(state: &EditState, cx: FoldContext<'_>, pos: u32) -> Option<FoldRegion> {
    let own = state.line_at(pos).number;
    (0..=own).rev().take(MAX_SCAN_LINES).find_map(|number| {
        cx.foldable_at(number)
            .filter(|region| number == own || (region.0 <= pos && pos <= region.1))
            .filter(|region| !cx.is_folded(*region))
    })
}

/// Collapse: folds the region around each caret.
pub fn fold_at_caret(state: &EditState, cx: FoldContext<'_>) -> Option<FoldEdit> {
    let effects = state
        .selections
        .iter()
        .filter_map(|range| enclosing_fold_range(state, cx, range.head()))
        .map(FoldEffect::Fold)
        .collect();
    FoldEdit::effects(effects)
}

/// The visible fold at `head` (Expand): the widest fold containing it, as the inner ones are
/// hidden inside it; else the widest fold starting on its line.
pub fn fold_to_expand(state: &EditState, folded: &[FoldRegion], head: u32) -> Option<FoldRegion> {
    let line = state.line_at(head);
    let width = |region: &FoldRegion| region.1 - region.0;
    let mut containing: Option<FoldRegion> = None;
    let mut on_line: Option<FoldRegion> = None;
    for region in folded {
        let slot = if region.0 <= head && head <= region.1 {
            &mut containing
        } else if region.0 >= line.from && region.0 <= line.to {
            &mut on_line
        } else {
            continue;
        };
        if slot.is_none_or(|best| width(region) > width(&best)) {
            *slot = Some(*region);
        }
    }
    containing.or(on_line)
}

/// Expand: unfolds the visible fold at each caret.
pub fn unfold_at_caret(state: &EditState, folded: &[FoldRegion]) -> Option<FoldEdit> {
    let effects = state
        .selections
        .iter()
        .filter_map(|range| fold_to_expand(state, folded, range.head()))
        .map(FoldEffect::Unfold)
        .collect();
    FoldEdit::effects(effects)
}

/// The end of a selection without the line break it ends after.
fn without_trailing_break(state: &EditState, to: u32) -> u32 {
    let line = state.line_at(to);
    if line.from == to { state.previous_line_end(&line).unwrap_or(to) } else { to }
}

/// Fold Selection / Remove Region: folds each selection (without a line break it ends with);
/// at a caret, unfolds the fold there or else folds the region around it. After a new fold of a
/// selection the caret goes to its end.
pub fn fold_selection(state: &EditState, cx: FoldContext<'_>) -> Option<FoldEdit> {
    let mut effects = Vec::new();
    for range in &state.selections {
        if !range.is_caret() {
            let to = without_trailing_break(state, range.end());
            if to > range.start() {
                effects.push(FoldEffect::Fold((range.start(), to)));
            }
        } else if let Some(folded) = fold_to_expand(state, cx.folded, range.head()) {
            effects.push(FoldEffect::Unfold(folded));
        } else if let Some(region) = enclosing_fold_range(state, cx, range.head()) {
            effects.push(FoldEffect::Fold(region));
        }
    }
    let mut edit = FoldEdit::effects(effects)?;
    let last = edit.folds().last();
    if state.selections.iter().any(|range| !range.is_caret())
        && let Some(last) = last
    {
        edit.selection = Some(Edit::select(vec![caret(last.1)], 0));
    }
    Some(edit)
}

/// Every foldable region (nested ones included) of the lines in `from..=to` that lies within
/// `from..=to`, in line order.
pub fn foldable_regions(state: &EditState, cx: FoldContext<'_>, from: u32, to: u32) -> Vec<FoldRegion> {
    let (first, last) = (state.line_at(from).number, state.line_at(to).number);
    (first..=last)
        .filter_map(|number| cx.foldable_at(number))
        .filter(|region| region.0 >= from && region.1 <= to)
        .collect()
}

/// The main selection, when it holds the regions to work on; `None` for the whole document.
fn region_scope(state: &EditState) -> Option<FoldRegion> {
    let main = state.main();
    (!main.is_caret()).then_some((main.start(), main.end()))
}

/// Collapse All: folds every region, nested ones included; with a selection, the regions inside
/// it (all of them when it holds none).
pub fn collapse_all(state: &EditState, cx: FoldContext<'_>) -> Option<FoldEdit> {
    let scoped = region_scope(state).map(|(from, to)| foldable_regions(state, cx, from, to)).unwrap_or_default();
    let regions = if scoped.is_empty() { foldable_regions(state, cx, 0, state.doc_len()) } else { scoped };
    FoldEdit::effects(regions.into_iter().filter(|region| !cx.is_folded(*region)).map(FoldEffect::Fold).collect())
}

/// Expand All: unfolds every folded region, or those inside the selection when it holds any.
pub fn expand_all(state: &EditState, folded: &[FoldRegion]) -> Option<FoldEdit> {
    let inside: Vec<FoldRegion> = region_scope(state)
        .map(|(from, to)| folded.iter().copied().filter(|region| region.0 >= from && region.1 <= to).collect())
        .unwrap_or_default();
    let regions = if inside.is_empty() { folded.to_vec() } else { inside };
    FoldEdit::effects(regions.into_iter().map(FoldEffect::Unfold).collect())
}

/// Expand to Level `level` (of the innermost region around the main caret) or, with `all`,
/// Expand All to Level `level` (of the file): the regions nested at most `level` deep are
/// unfolded, deeper ones folded. A caret a new fold would hide moves to the start of the first
/// such fold.
pub fn expand_to_level(state: &EditState, cx: FoldContext<'_>, level: u32, all: bool) -> Option<FoldEdit> {
    let regions = foldable_regions(state, cx, 0, state.doc_len());
    let head = state.main().head();
    let scope = if all {
        regions
    } else {
        let width = |region: &FoldRegion| region.1 - region.0;
        let mut root: Option<FoldRegion> = None;
        for region in regions.iter().filter(|region| region.0 <= head && head <= region.1) {
            if root.is_none_or(|best| width(region) < width(&best)) {
                root = Some(*region);
            }
        }
        let root = root?;
        regions.into_iter().filter(|region| region.0 >= root.0 && region.1 <= root.1).collect()
    };
    let mut effects = Vec::new();
    for (index, region) in scope.iter().enumerate() {
        let around = scope
            .iter()
            .enumerate()
            .filter(|(other, outer)| *other != index && outer.0 <= region.0 && region.1 <= outer.1)
            .count();
        let depth = around as u32 + 1;
        let folded = cx.is_folded(*region);
        if depth <= level && folded {
            effects.push(FoldEffect::Unfold(*region));
        } else if depth > level && !folded {
            effects.push(FoldEffect::Fold(*region));
        }
    }
    let mut edit = FoldEdit::effects(effects)?;
    let hiding = edit.folds().filter(|region| region.0 < head && head < region.1).min_by_key(|region| region.0);
    edit.selection = hiding.map(|region| Edit::select(vec![caret(region.0)], 0));
    Some(edit)
}

/// The folds Collapse Recursively makes at `head`: the region of its line (else the innermost
/// one around it) and every region inside it.
fn collapse_recursively_at(state: &EditState, cx: FoldContext<'_>, head: u32) -> Vec<FoldEffect> {
    let own = cx.foldable_at(state.line_at(head).number);
    let root = match own {
        Some(region) if !cx.is_folded(region) => Some(region),
        _ => enclosing_fold_range(state, cx, head),
    };
    let Some(root) = root else {
        return Vec::new();
    };
    let mut inner: Vec<FoldRegion> =
        foldable_regions(state, cx, root.0, root.1).into_iter().filter(|region| !cx.is_folded(*region)).collect();
    if !cx.is_folded(root) && !inner.contains(&root) {
        inner.push(root);
    }
    inner.into_iter().map(FoldEffect::Fold).collect()
}

/// Collapse Recursively: at each caret, the region of its line (else the innermost one around it)
/// and every region inside it.
pub fn collapse_recursively(state: &EditState, cx: FoldContext<'_>) -> Option<FoldEdit> {
    let effects = state.selections.iter().flat_map(|range| collapse_recursively_at(state, cx, range.head())).collect();
    FoldEdit::effects(effects)
}

/// The folds Expand Recursively removes at `head`: the visible fold there (else the region of its
/// line) and every fold inside it.
fn expand_recursively_at(state: &EditState, cx: FoldContext<'_>, head: u32) -> Vec<FoldEffect> {
    let root = fold_to_expand(state, cx.folded, head).or_else(|| cx.foldable_at(state.line_at(head).number));
    let Some(root) = root else {
        return Vec::new();
    };
    cx.folded
        .iter()
        .filter(|region| region.0 >= root.0 && region.1 <= root.1)
        .map(|region| FoldEffect::Unfold(*region))
        .collect()
}

/// Expand Recursively: at each caret, the fold there and every fold inside it.
pub fn expand_recursively(state: &EditState, cx: FoldContext<'_>) -> Option<FoldEdit> {
    let effects = state.selections.iter().flat_map(|range| expand_recursively_at(state, cx, range.head())).collect();
    FoldEdit::effects(effects)
}

/// The folded ranges after `effects`, in document order: a fold is added unless it is there
/// already, an unfold removes the fold with exactly that range.
pub fn apply_fold_effects(folded: &[FoldRegion], effects: &[FoldEffect]) -> Vec<FoldRegion> {
    let mut out = folded.to_vec();
    for effect in effects {
        match effect {
            FoldEffect::Fold(region) => {
                if !out.contains(region) {
                    out.push(*region);
                }
            }
            FoldEffect::Unfold(region) => out.retain(|folded| folded != region),
        }
    }
    out.sort_unstable();
    out
}

/// The folded ranges once the main caret moves to `head`: a fold with the caret strictly inside
/// it opens, so the caret is never hidden.
pub fn unfold_around_head(folded: &[FoldRegion], head: u32) -> Vec<FoldRegion> {
    folded.iter().copied().filter(|region| !(region.0 < head && head < region.1)).collect()
}

#[cfg(test)]
mod tests {
    use super::super::CommentTokens;
    use super::super::testing::state;
    use super::*;

    /// What folds `{ … }` blocks: a line ending with `{` folds from its end to the matching `}`.
    fn braces(state: &EditState) -> Vec<Option<FoldRegion>> {
        let units = state.units();
        (0..state.line_count())
            .map(|number| {
                let line = state.line(number);
                if !line.text.trim_end().ends_with('{') {
                    return None;
                }
                let mut depth = 0;
                for (pos, unit) in units.iter().enumerate().skip(line.to.saturating_sub(1) as usize) {
                    if *unit == u16::from(b'{') {
                        depth += 1;
                    } else if *unit == u16::from(b'}') {
                        depth -= 1;
                        if depth == 0 {
                            return Some((line.to, pos as u32));
                        }
                    }
                }
                None
            })
            .collect()
    }

    const TEXT: &str = "fn a() {\n    if x {\n        y();\n    }\n    z();\n}\n";

    fn make() -> EditState {
        EditState::new(TEXT)
    }

    /// Offset `column` of 0-based line `number`.
    fn at(state: &EditState, number: u32, column: u32) -> u32 {
        state.line(number).from + column
    }

    fn line_of(state: &EditState, region: Option<FoldRegion>) -> Option<u32> {
        region.map(|region| state.line_at(region.0).number)
    }

    fn effects(edit: Option<FoldEdit>) -> Vec<FoldEffect> {
        edit.map(|edit| edit.effects).unwrap_or_default()
    }

    /// The 0-based lines folds start on.
    fn folded_lines(state: &EditState, folded: &[FoldRegion]) -> Vec<u32> {
        folded.iter().map(|region| state.line_at(region.0).number).collect()
    }

    fn fold(start_line: u32, end_line: u32) -> FoldRange {
        FoldRange { start_line, end_line, kind: FoldKind::Region }
    }

    #[test]
    fn folds_the_caret_lines_own_region_first() {
        let s = make();
        let foldable = braces(&s);
        let cx = FoldContext { foldable: &foldable, folded: &[] };
        assert_eq!(line_of(&s, enclosing_fold_range(&s, cx, at(&s, 1, 2))), Some(1));
    }

    #[test]
    fn folds_the_innermost_enclosing_region_from_inside_a_body() {
        let s = make();
        let foldable = braces(&s);
        let cx = FoldContext { foldable: &foldable, folded: &[] };
        assert_eq!(line_of(&s, enclosing_fold_range(&s, cx, at(&s, 2, 9))), Some(1));
        assert_eq!(line_of(&s, enclosing_fold_range(&s, cx, at(&s, 4, 4))), Some(0));
    }

    #[test]
    fn moves_outward_once_the_inner_region_is_folded() {
        let s = make();
        let foldable = braces(&s);
        let inner = enclosing_fold_range(&s, FoldContext { foldable: &foldable, folded: &[] }, at(&s, 2, 9));
        let folded: Vec<FoldRegion> = inner.into_iter().collect();
        let cx = FoldContext { foldable: &foldable, folded: &folded };
        assert_eq!(line_of(&s, enclosing_fold_range(&s, cx, at(&s, 1, 2))), Some(0));
    }

    #[test]
    fn expands_the_outermost_fold_hiding_the_caret_first() {
        let s = make();
        let foldable = braces(&s);
        let pos = at(&s, 2, 9);
        let inner = enclosing_fold_range(&s, FoldContext { foldable: &foldable, folded: &[] }, pos).expect("inner");
        let folded = [inner];
        let outer = enclosing_fold_range(&s, FoldContext { foldable: &foldable, folded: &folded }, pos).expect("outer");
        let folded = apply_fold_effects(&folded, &[FoldEffect::Fold(outer)]);
        assert_eq!(fold_to_expand(&s, &folded, pos), Some(outer));
    }

    #[test]
    fn expands_a_fold_on_the_caret_line_when_the_caret_is_on_its_header() {
        let s = make();
        let foldable = braces(&s);
        let pos = at(&s, 1, 2);
        let inner = enclosing_fold_range(&s, FoldContext { foldable: &foldable, folded: &[] }, pos).expect("inner");
        let folded = [inner];
        assert_eq!(fold_to_expand(&s, &folded, pos), Some(inner));
        assert_eq!(fold_to_expand(&s, &folded, at(&s, 4, 2)), None);
    }

    #[test]
    fn starts_an_allman_blocks_fold_on_its_declaration_line() {
        let s = EditState::new("namespace Demo\n{\n    class A\n    {\n    }\n}\n");
        let folds = folds_from_ranges(&s, &[fold(1, 5), fold(3, 4)], &|_| Vec::new());
        let shown: Vec<(u32, String)> =
            folds.iter().map(|fold| (s.line_at(fold.from).number, s.slice(fold.to, fold.to + 1))).collect();
        assert_eq!(shown, [(0, "}".to_owned()), (2, "}".to_owned())]);
    }

    #[test]
    fn folds_a_custom_region_from_its_start_marker_through_its_end_marker() {
        let s = EditState::new("class A {\n  //region Fields\n  int a;\n  //endregion\n}\n");
        let tokens = CommentTokens::new(Some("//"), None);
        let folds = folds_from_ranges(&s, &[fold(1, 3)], &|_| vec![tokens.clone()]);
        let expected = BackendFold { from: s.line(1).from + 2, to: s.line(3).to, kind: FoldKind::Region, custom: true };
        assert_eq!(folds.first(), Some(&expected));
    }

    #[test]
    fn keeps_a_brace_line_that_follows_a_statement_or_comment() {
        let s = EditState::new("x();\n{\n    y();\n}\n// c\n{\n}\n");
        let folds = folds_from_ranges(&s, &[fold(1, 3), fold(5, 6)], &|_| Vec::new());
        let lines: Vec<u32> = folds.iter().map(|fold| s.line_at(fold.from).number).collect();
        assert_eq!(lines, [1, 5]);
    }

    #[test]
    fn folds_the_selection_and_expands_it_again_from_the_caret() {
        let s = EditState::new(TEXT).with_selections(vec![(9, 30)], 0);
        let cx = FoldContext { foldable: &[], folded: &[] };
        let edit = fold_selection(&s, cx).expect("the selection folds");
        let folded = apply_fold_effects(&[], &edit.effects);
        assert_eq!(folded, [(9, 30)]);
        assert_eq!(edit.selection.as_ref().map(|selection| selection.selections.clone()), Some(vec![caret(30)]));
        let s = s.with_selections(vec![caret(30)], 0);
        let folded = unfold_around_head(&folded, 30);
        let edit = fold_selection(&s, FoldContext { foldable: &[], folded: &folded }).expect("the fold opens");
        assert!(apply_fold_effects(&folded, &edit.effects).is_empty());
    }

    #[test]
    fn expand_all_to_level_1_keeps_top_level_regions_open_and_folds_nested_ones() {
        let s = make();
        let foldable = braces(&s);
        let edit = expand_to_level(&s, FoldContext { foldable: &foldable, folded: &[] }, 1, true);
        let folded = apply_fold_effects(&[], &effects(edit));
        assert_eq!(folded_lines(&s, &folded), [1]);
        let edit = expand_to_level(&s, FoldContext { foldable: &foldable, folded: &folded }, 2, true);
        assert!(apply_fold_effects(&folded, &effects(edit)).is_empty());
    }

    #[test]
    fn expand_to_level_counts_from_the_region_around_the_caret() {
        let s = make().with_selections(vec![caret(TEXT.find("y()").expect("y") as u32)], 0);
        let foldable = braces(&s);
        let edit = expand_to_level(&s, FoldContext { foldable: &foldable, folded: &[] }, 1, true);
        let folded = apply_fold_effects(&[], &effects(edit));
        let z = TEXT.find("z()").expect("z") as u32;
        let s = s.with_selections(vec![caret(z)], 0);
        let folded = unfold_around_head(&folded, z);
        let edit = expand_to_level(&s, FoldContext { foldable: &foldable, folded: &folded }, 2, false);
        assert!(apply_fold_effects(&folded, &effects(edit)).is_empty());
    }

    const ACTIONS_TEXT: &str = "a {\n  b {\n    c;\n  }\n}\nd {\n}\n";

    #[test]
    fn collapse_all_folds_nested_regions_too_with_a_selection_only_those_inside_it() {
        let s = EditState::new(ACTIONS_TEXT);
        let foldable = braces(&s);
        let cx = FoldContext { foldable: &foldable, folded: &[] };
        let folded = apply_fold_effects(&[], &effects(collapse_all(&s, cx)));
        assert_eq!(folded_lines(&s, &folded), [0, 1, 5]);
        let s = s.with_selections(vec![(4, 21)], 0);
        let folded = apply_fold_effects(&[], &effects(collapse_all(&s, cx)));
        assert_eq!(folded_lines(&s, &folded), [1]);
    }

    #[test]
    fn collapses_and_expands_recursively() {
        let s = EditState::new(ACTIONS_TEXT).with_selections(vec![caret(1)], 0);
        let foldable = braces(&s);
        let edit = collapse_recursively(&s, FoldContext { foldable: &foldable, folded: &[] });
        let folded = apply_fold_effects(&[], &effects(edit));
        assert_eq!(folded_lines(&s, &folded), [0, 1]);
        let edit = expand_recursively(&s, FoldContext { foldable: &foldable, folded: &folded });
        assert!(apply_fold_effects(&folded, &effects(edit)).is_empty());
    }

    #[test]
    fn cmd_period_folds_a_selection_without_its_trailing_line_break_and_toggles_the_region_at_a_caret() {
        let s = EditState::new(ACTIONS_TEXT).with_selections(vec![(4, 23)], 0);
        let foldable = braces(&s);
        let cx = FoldContext { foldable: &foldable, folded: &[] };
        assert_eq!(effects(fold_selection(&s, cx)).first(), Some(&FoldEffect::Fold((4, 22))));
        let s = s.with_selections(vec![caret(12)], 0);
        let folded = apply_fold_effects(&[], &effects(fold_selection(&s, cx)));
        assert_eq!(folded_lines(&s, &folded), [1]);
        let edit = fold_selection(&s, FoldContext { foldable: &foldable, folded: &folded });
        assert!(apply_fold_effects(&folded, &effects(edit)).is_empty());
    }

    #[test]
    fn a_selection_ending_after_a_crlf_break_folds_without_it() {
        let s = state("a\r\nb\r\nc").with_selections(vec![(0, 6)], 0);
        let edit = fold_selection(&s, FoldContext { foldable: &[], folded: &[] });
        assert_eq!(effects(edit), [FoldEffect::Fold((0, 4))]);
    }

    #[test]
    fn backend_folds_follow_edits_and_give_each_line_its_widest_fold() {
        let s = EditState::new("a {\n  b\n}\n");
        let folds = [
            BackendFold { from: 3, to: 9, kind: FoldKind::Region, custom: false },
            BackendFold { from: 3, to: 7, kind: FoldKind::Comment, custom: false },
        ];
        assert_eq!(foldable_lines(&s, &folds), [Some((3, 9)), None, None, None]);
        let mapped = map_backend_folds(&folds, &[TextChange { from: 0, to: 0, insert: "xx".to_owned() }]);
        assert_eq!(mapped.iter().map(|fold| (fold.from, fold.to)).collect::<Vec<_>>(), [(5, 11), (5, 9)]);
        assert!(map_backend_folds(&folds, &[TextChange { from: 2, to: 10, insert: String::new() }]).is_empty());
    }
}
