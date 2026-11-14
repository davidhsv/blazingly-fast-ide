use super::*;

/// The unchanged stretches between `changes` of a base of `len1` lines and a side of `len2` lines,
/// each as `(from1, from2, length)`; panics when a stretch would differ in length on the two sides.
fn unchanged(changes: &[Range], len1: usize, len2: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let (mut p1, mut p2) = (0, 0);
    for change in changes.iter().chain(std::iter::once(&Range::new(len1, len1, len2, len2))) {
        assert!(change.start1 >= p1 && change.start2 >= p2, "changes are ordered: {changes:?}");
        assert_eq!(change.start1 - p1, change.start2 - p2, "an unchanged stretch has as many lines on each side");
        out.push((p1, p2, change.start1 - p1));
        (p1, p2) = (change.end1, change.end2);
    }
    out
}

/// Over random edits of random code, the line optimizer the diff and the merge use only moves the
/// boundaries of changes: the unchanged stretches stay equal line for line, in order, and cover as
/// many lines as the shortest diff found.
#[test]
fn only_rearranges_the_unchanged_stretches_stay_equal_ordered_and_as_many_lines() {
    let mut random = Random::new(99);
    let vocab = ["{", "}", "", "  a();", "  b();", "if (x) {", "return;", "  );"];
    for round in 0..500 {
        let pick = |random: &mut Random| vocab[random.below(vocab.len())];
        let base: Vec<&str> = (0..random.below(25)).map(|_| pick(&mut random)).collect();
        let mut side = base.clone();
        for _ in 0..1 + random.below(4) {
            let at = random.below(side.len() + 1);
            if random.unit() < 0.5 {
                let end = (at + 1 + random.below(3)).min(side.len());
                side.drain(at..end);
            } else {
                let added: Vec<&str> = (0..1 + random.below(3)).map(|_| pick(&mut random)).collect();
                side.splice(at..at, added);
            }
        }
        let number = |line: &&str| vocab.iter().position(|known| known == line).expect("a known line") as u16;
        let ids1: Vec<u16> = base.iter().map(number).collect();
        let ids2: Vec<u16> = side.iter().map(number).collect();
        let Ok(found) = diff_keys(&ids1, &ids2) else { panic!("round {round}: small texts compare") };
        let optimized = against(&ids1, &ids2, Some((base.as_slice(), side.as_slice()))).expect("small texts compare");
        let before = unchanged(&found, base.len(), side.len());
        let after = unchanged(&optimized, base.len(), side.len());
        let total = |stretches: &[(usize, usize, usize)]| stretches.iter().map(|s| s.2).sum::<usize>();
        assert_eq!(total(&after), total(&before), "round {round}: {base:?} → {side:?}");
        for (from1, from2, length) in after {
            assert_eq!(base[from1..from1 + length], side[from2..from2 + length], "round {round}: {base:?} → {side:?}");
        }
    }
}

/// A function added between two others is one change from a blank line to the closing brace: it
/// adds no base lines and does not start inside the function before it or end inside the one after.
#[test]
fn puts_an_added_function_between_its_neighbours_not_across_their_braces() {
    let base = lines(&["a() {", "  x();", "}", "", "b() {", "  y();", "}", ""]);
    let left = lines(&["a() {", "  x();", "}", "", "c() {", "  z();", "}", "", "b() {", "  y();", "}", ""]);
    let changes = merge_changes(&left, &base, &base).expect("the versions are comparable");
    let added = changes.first().expect("one change");
    assert_eq!(added.base, "");
    assert!(added.left.starts_with("c() {") || added.left.starts_with("\nc() {"), "{:?}", added.left);
    assert!(added.left.trim_end().ends_with('}'), "{:?}", added.left);
}
