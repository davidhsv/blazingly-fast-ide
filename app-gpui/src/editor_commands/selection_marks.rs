//! The selected text to repaint over other backgrounds: a mark with a background of its own
//! (a warning, a find match, a usage, a semantic background) would otherwise hide the selection
//! painted behind the text.

use super::{EditState, SelectionExt};

/// The non-empty selection ranges clipped to the visible ranges, in the order the visible ranges
/// come and, within one, the order of the selection.
pub fn selected_ranges(state: &EditState, visible: &[(u32, u32)]) -> Vec<(u32, u32)> {
    let ranges: Vec<(u32, u32)> =
        state.selections.iter().filter(|range| !range.is_caret()).map(|range| (range.start(), range.end())).collect();
    visible
        .iter()
        .flat_map(|(visible_from, visible_to)| {
            ranges.iter().filter_map(move |(from, to)| {
                let (from, to) = ((*from).max(*visible_from), (*to).min(*visible_to));
                (from < to).then_some((from, to))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::caret;
    use super::*;

    const DOC: &str = "return request.app.state.credits";

    fn everything(state: &EditState) -> [(u32, u32); 1] {
        [(0, state.doc_len())]
    }

    #[test]
    fn paints_selected_text_inside_a_background_mark() {
        let state = EditState::new(DOC).with_selections(vec![(7, DOC.len() as u32)], 0);
        assert_eq!(selected_ranges(&state, &everything(&state)), [(7, DOC.len() as u32)]);
    }

    #[test]
    fn covers_every_range_of_a_multi_selection_partially_overlapping_marks_too() {
        let state = EditState::new(DOC).with_selections(vec![(0, 3), (10, 20)], 1);
        assert_eq!(selected_ranges(&state, &everything(&state)), [(0, 3), (10, 20)]);
        // A range half outside the visible text is cut at its edge.
        assert_eq!(selected_ranges(&state, &[(15, 24)]), [(15, 20)]);
    }

    #[test]
    fn adds_nothing_for_an_empty_selection_and_clears_when_the_selection_collapses() {
        let state = EditState::new(DOC).with_selections(vec![(0, 6)], 0);
        assert_eq!(selected_ranges(&state, &everything(&state)), [(0, 6)]);
        let collapsed = state.with_selections(vec![caret(3)], 0);
        assert!(selected_ranges(&collapsed, &everything(&collapsed)).is_empty());
    }
}
