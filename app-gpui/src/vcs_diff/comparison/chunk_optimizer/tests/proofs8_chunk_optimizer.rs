use super::*;

const BLOCK: [&str; 3] = ["if (a) {", "  x();", "}"];

/// Each line's text as the key, and its count of characters other than white space as the weight.
fn weighed(lines: &[Vec<u16>]) -> Vec<LineInfo<'_>> {
    lines
        .iter()
        .map(|line| {
            let blanks = line.iter().filter(|unit| char::from_u32(u32::from(**unit)).is_some_and(char::is_whitespace));
            LineInfo { key: line, non_space_chars: line.len() - blanks.count() }
        })
        .collect()
}

/// A second copy of a block, reported as an insertion that starts in the middle of the first copy,
/// becomes the whole first block: lines 0–3 of the new text.
#[test]
fn moves_a_block_that_starts_in_the_middle_of_a_copy_to_where_the_copy_starts() {
    let other = ["if (b) {", "  y();", "}"];
    let base: Vec<&str> = BLOCK.iter().chain(&other).copied().collect();
    let left: Vec<&str> = BLOCK.iter().chain(&BLOCK).chain(&other).copied().collect();
    let (base, left) = (units(&base), units(&left));
    // The diff kept lines 0–2 and 2–6 of the base as lines 0–2 and 5–9 of the left.
    let misaligned = DiffIterable::new(vec![Range::new(2, 2, 2, 5)], base.len(), left.len());
    let optimized = optimize_line_chunks(&weighed(&base), &weighed(&left), &misaligned);
    assert_eq!(optimized.changes, [Range::new(0, 0, 0, 3)]);
}

/// An insertion that already starts after a blank line and ends with one stays where it is.
#[test]
fn leaves_a_change_that_already_starts_and_ends_at_a_blank_line_alone() {
    let base = units(&["if (a) {", "", "  x();"]);
    let left = units(&["if (a) {", "", "  y();", "", "  x();"]);
    let found = DiffIterable::new(vec![Range::new(2, 2, 2, 4)], base.len(), left.len());
    let optimized = optimize_line_chunks(&weighed(&base), &weighed(&left), &found);
    assert_eq!(optimized.changes, [Range::new(2, 2, 2, 4)]);
}
