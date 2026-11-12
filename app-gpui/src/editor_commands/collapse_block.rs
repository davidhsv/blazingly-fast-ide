//! Fold Code Block: folds the inside of the `{…}` block around the caret whether or not the
//! language offers a fold there; an already folded block passes on to the one enclosing it.

use super::caret_commands::enclosing_code_block;
use super::folding::{FoldEdit, FoldEffect, FoldRegion};
use super::tree::SyntaxTree;
use super::{Edit, EditState, Selection, SelectionExt, caret};

/// The blocks Fold Code Block folds: the one around each caret, or, when that one is folded
/// already, the closest enclosing one that is not.
pub fn collapse_block_folds(state: &EditState, tree: &SyntaxTree, folded: &[FoldRegion]) -> Vec<FoldRegion> {
    let mut out = Vec::new();
    for range in &state.selections {
        let mut block = enclosing_code_block(state, tree, range.head());
        while let Some(found) = block.filter(|found| folded.contains(found)) {
            block = enclosing_code_block(state, tree, found.0.saturating_sub(1));
        }
        if let Some(found) = block.filter(|found| found.1 > found.0) {
            out.push(found);
        }
    }
    out
}

/// Fold Code Block. A caret the new fold would hide moves onto the fold's start, since a caret
/// inside a fold opens it again.
pub fn collapse_block(state: &EditState, tree: &SyntaxTree, folded: &[FoldRegion]) -> Option<FoldEdit> {
    let folds = collapse_block_folds(state, tree, folded);
    if folds.is_empty() {
        return None;
    }
    let ranges: Vec<Selection> = state
        .selections
        .iter()
        .map(|range| {
            let hiding = folds.iter().find(|fold| fold.0 < range.head() && range.head() < fold.1);
            hiding.map_or(*range, |fold| caret(fold.0))
        })
        .collect();
    Some(FoldEdit {
        effects: folds.into_iter().map(FoldEffect::Fold).collect(),
        selection: Some(Edit::select(ranges, state.main)),
    })
}

#[cfg(test)]
mod tests {
    use super::super::caret_commands::{select_code_block_end, select_code_block_start};
    use super::super::folding::{apply_fold_effects, unfold_around_head};
    use super::*;

    const TEXT: &str = "fn a() {\n  if x { y(); }\n  z();\n}\n";

    fn at(needle: &str) -> u32 {
        TEXT.find(needle).expect("the needle is in the text") as u32
    }

    fn last_brace() -> u32 {
        TEXT.rfind('}').expect("a closing brace") as u32
    }

    fn state_at(pos: u32) -> EditState {
        EditState::new(TEXT).with_selections(vec![caret(pos)], 0)
    }

    fn no_tree() -> SyntaxTree {
        SyntaxTree::default()
    }

    #[test]
    fn is_the_inside_of_the_innermost_block() {
        let block = enclosing_code_block(&state_at(0), &no_tree(), at("y()"));
        assert_eq!(block, Some((at("{ y") + 1, at("}\n  z"))));
    }

    #[test]
    fn after_a_closing_brace_is_the_block_it_closes() {
        let block = enclosing_code_block(&state_at(0), &no_tree(), at("}\n  z") + 1);
        assert_eq!(block, Some((at("{ y") + 1, at("}\n  z"))));
    }

    #[test]
    fn is_null_outside_any_block() {
        assert_eq!(enclosing_code_block(&state_at(0), &no_tree(), 0), None);
    }

    #[test]
    fn folds_the_block_around_the_caret_then_the_enclosing_one() {
        let s = state_at(at("y()"));
        let first = collapse_block(&s, &no_tree(), &[]).expect("a block folds");
        assert_eq!(first.effects, [FoldEffect::Fold((at("{ y") + 1, at("}\n  z")))]);
        let folded = apply_fold_effects(&[], &first.effects);
        let second = collapse_block(&s, &no_tree(), &folded).expect("the enclosing block folds");
        assert_eq!(second.effects, [FoldEffect::Fold((at("{\n") + 1, last_brace()))]);
    }

    #[test]
    fn extends_the_selection_to_the_block_start_and_end() {
        let s = state_at(at("z()"));
        let end = select_code_block_end(&s, &no_tree()).expect("the block has an end");
        assert_eq!(end.selections, [(at("z()"), last_brace())]);
        let start = select_code_block_start(&s, &no_tree()).expect("the block has a start");
        assert_eq!(start.selections, [(at("z()"), at("{\n") + 1)]);
    }

    #[test]
    fn keeps_the_fold_by_moving_a_caret_inside_it_onto_the_fold() {
        let s = state_at(at("y()"));
        let edit = collapse_block(&s, &no_tree(), &[]).expect("a block folds");
        let selection = edit.selection.expect("the caret moves");
        let head = selection.selections[selection.main].head();
        let folded = unfold_around_head(&apply_fold_effects(&[], &edit.effects), head);
        assert_eq!(folded.len(), 1);
        assert_eq!(head, at("{ y") + 1);
    }
}
