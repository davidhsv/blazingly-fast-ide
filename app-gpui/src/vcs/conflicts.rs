//! Git conflict markers: finding the conflict blocks of a text and the text that resolves them, the
//! titles of the merge editor's panes, and the pass that moves the boundaries of a line diff to where
//! a reader expects them.
//!
//! Offsets are byte offsets into the text, always on a character boundary.

use serde::{Deserialize, Serialize};
use std::ops::Range;

/// Who stands on each side of the conflicts of the operation in progress, as the backend reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeSides {
    /// `merge`, `rebase`, `cherry-pick`, `unstash`…; `None` outside an operation.
    pub operation: Option<String>,
    /// The current branch (for a rebase: the branch being rebased).
    pub branch: Option<String>,
    /// Short id of the commit being applied or merged.
    pub commit: Option<String>,
    /// The other branch: the one being merged, or the one rebased onto.
    pub other_branch: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictBlock {
    /// Offset of the `<<<<<<<` line start.
    pub from: usize,
    /// Offset just past the `>>>>>>>` line (including its terminator).
    pub to: usize,
    pub ours: String,
    /// diff3 base section, when present.
    pub base: Option<String>,
    pub theirs: String,
    pub ours_label: String,
    pub theirs_label: String,
    /// Offsets of the two sections (for decorations).
    pub ours_from: usize,
    pub ours_to: usize,
    pub theirs_from: usize,
    pub theirs_to: usize,
    /// The marker lines (`<<<<<<<`, `|||||||` when there is a base, `=======`, `>>>>>>>`), without their
    /// terminators.
    pub markers: Vec<Range<usize>>,
}

const START: &str = "<<<<<<<";
const BASE: &str = "|||||||";
const SEP: &str = "=======";
const END: &str = ">>>>>>>";

/// A line of the text with its terminator, and where it starts.
struct Line<'a> {
    from: usize,
    text: &'a str,
}

impl Line<'_> {
    fn end(&self) -> usize {
        self.from + self.text.len()
    }
}

fn is_marker(line: &str, marker: &str) -> bool {
    line.strip_prefix(marker).is_some_and(|rest| matches!(rest.bytes().next(), None | Some(b' ' | b'\r' | b'\n')))
}

/// Finds well-formed conflict blocks in `text`.
pub fn parse_conflicts(text: &str) -> Vec<ConflictBlock> {
    let mut lines: Vec<Line> = Vec::new();
    let mut pos = 0usize;
    for line in text.split_inclusive('\n') {
        lines.push(Line { from: pos, text: line });
        pos += line.len();
    }
    // Lines are contiguous, so the lines `a..b` are the text between the starts of `a` and `b`.
    let slice = |a: usize, b: usize| if a >= b { String::new() } else { text[lines[a].from..lines[b].from].to_owned() };
    let label = |line: &Line, marker: &str| line.text[marker.len()..].trim().to_owned();
    let marker_range = |line: &Line| {
        let shown = line.text.strip_suffix('\n').map_or(line.text, |t| t.strip_suffix('\r').unwrap_or(t));
        line.from..line.from + shown.len()
    };

    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let start = &lines[i];
        if !is_marker(start.text, START) {
            i += 1;
            continue;
        }
        let mut base_at: Option<usize> = None;
        let mut sep_at: Option<usize> = None;
        let mut end_at: Option<usize> = None;
        for (j, line) in lines.iter().enumerate().skip(i + 1) {
            if is_marker(line.text, START) {
                break;
            }
            if sep_at.is_none() && base_at.is_none() && is_marker(line.text, BASE) {
                base_at = Some(j);
            } else if sep_at.is_none() && is_marker(line.text, SEP) {
                sep_at = Some(j);
            } else if sep_at.is_some() && is_marker(line.text, END) {
                end_at = Some(j);
                break;
            }
        }
        let (Some(sep_at), Some(end_at)) = (sep_at, end_at) else {
            i += 1;
            continue;
        };
        let ours_end = base_at.unwrap_or(sep_at);
        out.push(ConflictBlock {
            from: start.from,
            to: lines[end_at].end(),
            ours: slice(i + 1, ours_end),
            base: base_at.map(|base_at| slice(base_at + 1, sep_at)),
            theirs: slice(sep_at + 1, end_at),
            ours_label: label(start, START),
            theirs_label: label(&lines[end_at], END),
            ours_from: lines[i + 1].from,
            ours_to: lines[ours_end].from,
            theirs_from: lines[sep_at].end(),
            theirs_to: lines[end_at].from,
            markers: [Some(i), base_at, Some(sep_at), Some(end_at)]
                .into_iter()
                .flatten()
                .map(|j| marker_range(&lines[j]))
                .collect(),
        });
        i = end_at + 1;
    }
    out
}

/// A choice in user terms: "ours" is the user's work ("Accept Yours").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    Both,
    Base,
}

/// Text replacing `block`. With `reversed` (a rebase, where git writes the upstream as the `<<<<<<<`
/// section and the user's commit as `>>>>>>>`) the sides swap, as IDEA does.
pub fn resolution_text(block: &ConflictBlock, how: Resolution, reversed: bool) -> String {
    let (yours, theirs) = if reversed { (&block.theirs, &block.ours) } else { (&block.ours, &block.theirs) };
    match how {
        Resolution::Ours => yours.clone(),
        Resolution::Theirs => theirs.clone(),
        Resolution::Base => block.base.clone().unwrap_or_default(),
        Resolution::Both if !yours.is_empty() && !yours.ends_with('\n') => format!("{yours}\n{theirs}"),
        Resolution::Both => format!("{yours}{theirs}"),
    }
}

/// Resolves every block the same way.
pub fn resolve_all(text: &str, how: Resolution, reversed: bool) -> String {
    let mut out = String::new();
    let mut pos = 0usize;
    for block in parse_conflicts(text) {
        out.push_str(&text[pos..block.from]);
        out.push_str(&resolution_text(&block, how, reversed));
        pos = block.to;
    }
    out.push_str(&text[pos..]);
    out
}

/// The block holding `offset`.
pub fn block_at(blocks: &[ConflictBlock], offset: usize) -> Option<&ConflictBlock> {
    blocks.iter().find(|b| (b.from..b.to.max(b.from + 1)).contains(&offset))
}

/// A value the backend sent, unless it is missing or empty.
fn given(value: Option<&String>) -> Option<&str> {
    value.map(String::as_str).filter(|v| !v.is_empty())
}

/// Titles of the merge editor's two side panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeTitles {
    pub yours: String,
    pub theirs: String,
}

/// Merge editor pane titles for the operation in progress (as IDEA labels them). English source text,
/// to be translated as a template by whoever shows it.
pub fn merge_titles(sides: Option<&MergeSides>) -> MergeTitles {
    let branch = given(sides.and_then(|s| s.branch.as_ref()));
    let commit = given(sides.and_then(|s| s.commit.as_ref()));
    let other = given(sides.and_then(|s| s.other_branch.as_ref()));
    let (yours, theirs) = match sides.and_then(|s| s.operation.as_deref()) {
        Some("rebase") => (
            match (commit, branch) {
                (Some(commit), Some(branch)) => format!("Rebasing {commit} from {branch}"),
                _ => "Your changes".to_owned(),
            },
            match other {
                Some(other) => format!("Already rebased commits and commits from {other}"),
                None => "Already rebased commits".to_owned(),
            },
        ),
        Some("merge") => (
            branch.map_or_else(|| "Your version".to_owned(), |branch| format!("Changes from {branch}")),
            match (other, commit) {
                (Some(from), _) | (None, Some(from)) => format!("Changes from {from}"),
                (None, None) => "Changes from diverging branches".to_owned(),
            },
        ),
        Some("unstash") => (
            "Your uncommitted changes".to_owned(),
            branch.map_or_else(|| "Changes from remote".to_owned(), |branch| format!("Changes from {branch}")),
        ),
        Some("cherry-pick") => (
            "Local Changes".to_owned(),
            commit.map_or_else(
                || "Changes from cherry-pick".to_owned(),
                |commit| format!("Changes from cherry-pick {commit}"),
            ),
        ),
        _ => (
            branch.map_or_else(|| "Your version".to_owned(), |branch| format!("Your version, branch {branch}")),
            commit.map_or_else(|| "Theirs (incoming)".to_owned(), |commit| format!("Changes from {commit}")),
        ),
    };
    MergeTitles { yours, theirs }
}

/// An unchanged stretch of a line diff: lines `s1`–`e1` of the first text are lines `s2`–`e2` of the
/// second.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unchanged {
    pub s1: usize,
    pub e1: usize,
    pub s2: usize,
    pub e2: usize,
}

/// Lines with this many characters or fewer, apart from blanks, count as unimportant (`{`, `}`, `);`).
const UNIMPORTANT: usize = 3;

/// The unchanged stretches between `changes` (each `[from1, to1, from2, to2]`) of two texts of the given
/// lengths.
pub fn unchanged_between(changes: &[[usize; 4]], length1: usize, length2: usize) -> Vec<Unchanged> {
    let mut out = Vec::new();
    let (mut p1, mut p2) = (0usize, 0usize);
    for &[f1, t1, f2, t2] in changes {
        if f1 > p1 {
            out.push(Unchanged { s1: p1, e1: f1, s2: p2, e2: f2 });
        }
        (p1, p2) = (t1, t2);
    }
    if length1 > p1 {
        out.push(Unchanged { s1: p1, e1: length1, s2: p2, e2: length2 });
    }
    out
}

/// The changes (each `[from1, to1, from2, to2]`) between `unchanged` stretches of two texts of the given
/// lengths.
pub fn changes_between(unchanged: &[Unchanged], length1: usize, length2: usize) -> Vec<[usize; 4]> {
    let mut out = Vec::new();
    let (mut p1, mut p2) = (0usize, 0usize);
    for u in unchanged {
        if u.s1 > p1 || u.s2 > p2 {
            out.push([p1, u.s1, p2, u.s2]);
        }
        (p1, p2) = (u.e1, u.e2);
    }
    if length1 > p1 || length2 > p2 {
        out.push([p1, length1, p2, length2]);
    }
    out
}

/// How many lines from (`i1`, `i2`) on are equal, at most `max`.
fn equal_forward<T: PartialEq>(a: &[T], b: &[T], i1: usize, i2: usize, max: usize) -> usize {
    a.iter().skip(i1).zip(b.iter().skip(i2)).take(max).take_while(|(x, y)| x == y).count()
}

/// How many lines before (`i1`, `i2`) are equal, at most `max`.
fn equal_backward<T: PartialEq>(a: &[T], b: &[T], i1: usize, i2: usize, max: usize) -> usize {
    let (before_a, before_b) = (&a[..i1.min(a.len())], &b[..i2.min(b.len())]);
    before_a.iter().rev().zip(before_b.iter().rev()).take(max).take_while(|(x, y)| x == y).count()
}

/// Distance from `offset` forward to the first of `count` lines that weighs `limit` or less.
fn next_unimportant(weights: &[usize], offset: usize, count: usize, limit: usize) -> Option<usize> {
    (0..count).find(|i| weights.get(offset + i).is_some_and(|&weight| weight <= limit))
}

/// Distance from `last` backward to the first of `count` lines that weighs `limit` or less. `last` is
/// `None` when there is no line before the place looked from.
fn previous_unimportant(weights: &[usize], last: Option<usize>, count: usize, limit: usize) -> Option<usize> {
    let last = last?;
    (0..count).find(|&i| last.checked_sub(i).and_then(|at| weights.get(at)).is_some_and(|&weight| weight <= limit))
}

/// A stretch already at a boundary stays; else forward if one is found there, else backward.
fn pick_shift(forward: Option<usize>, backward: Option<usize>) -> Option<isize> {
    match (forward, backward) {
        (None, None) => None,
        (Some(0), _) | (_, Some(0)) => Some(0),
        (Some(forward), _) => Some(forward as isize),
        (None, Some(backward)) => Some(-(backward as isize)),
    }
}

/// How far the change between `r1` and `r2` moves, given that it can move `forward` lines down or
/// `backward` lines up. `touch_a` when the lines of `a` touch (the change only adds to `b`).
fn shift_of(
    touch_a: bool,
    (forward, backward): (usize, usize),
    (r1, r2): (Unchanged, Unchanged),
    weight_a: &[usize],
    weight_b: &[usize],
) -> isize {
    let (touch, other) = if touch_a { (weight_a, weight_b) } else { (weight_b, weight_a) };
    let touch_start = if touch_a { r2.s1 } else { r2.s2 };
    let change_start = if touch_a { r1.e2 } else { r1.e1 };
    let change_end = if touch_a { r2.s2 } else { r2.s1 };
    let at_touch = |limit: usize| {
        pick_shift(
            next_unimportant(touch, touch_start, forward + 1, limit),
            previous_unimportant(touch, touch_start.checked_sub(1), backward + 1, limit),
        )
    };
    let at_change = |limit: usize| {
        pick_shift(
            next_unimportant(other, change_start, forward + 1, limit),
            previous_unimportant(other, change_end.checked_sub(1), backward + 1, limit),
        )
    };
    at_touch(0)
        .or_else(|| at_change(0))
        .or_else(|| at_touch(UNIMPORTANT))
        .or_else(|| at_change(UNIMPORTANT))
        .unwrap_or(0)
}

/// Moves the change between the last two stretches of `ranges` to a better place, merging the two when
/// the change slides all the way into one of them (and then looking at the new last two).
fn settle_last<T: PartialEq>(ranges: &mut Vec<Unchanged>, a: &[T], b: &[T], weight_a: &[usize], weight_b: &[usize]) {
    loop {
        let len = ranges.len();
        if len < 2 {
            return;
        }
        let (r1, r2) = (ranges[len - 2], ranges[len - 1]);
        // Only a change that adds or removes lines (touching on one side) can move.
        if r1.e1 != r2.s1 && r1.e2 != r2.s2 {
            return;
        }
        let (count1, count2) = (r1.e1 - r1.s1, r2.e1 - r2.s1);
        let forward = equal_forward(a, b, r1.e1, r1.e2, count2);
        let backward = equal_backward(a, b, r2.s1, r2.s2, count1);
        if forward == 0 && backward == 0 {
            return;
        }
        let merged = if forward == count2 {
            // The change slides all the way to the next stretch, and disappears into it.
            Unchanged { s1: r1.s1, e1: r1.e1 + count2, s2: r1.s2, e2: r1.e2 + count2 }
        } else if backward == count1 {
            Unchanged { s1: r2.s1 - count1, e1: r2.e1, s2: r2.s2 - count1, e2: r2.e2 }
        } else {
            let shift = shift_of(r1.e1 == r2.s1, (forward, backward), (r1, r2), weight_a, weight_b);
            let moved = |at: usize| at.saturating_add_signed(shift);
            ranges[len - 2] = Unchanged { s1: r1.s1, e1: moved(r1.e1), s2: r1.s2, e2: moved(r1.e2) };
            ranges[len - 1] = Unchanged { s1: moved(r2.s1), e1: r2.e1, s2: moved(r2.s2), e2: r2.e2 };
            return;
        };
        ranges.truncate(len - 2);
        ranges.push(merged);
    }
}

/// Optimizes the changes between two texts, after IntelliJ's `ChunkOptimizer.LineChunkOptimizer`. A
/// diff has several equally short answers when an inserted or deleted block repeats the lines around
/// it (an added method can be said to start at its first line or to end at its closing brace); the
/// shortest diff does not say which, and this picks the one that starts or ends at a blank or
/// near-blank line.
///
/// `a` and `b` hold one value per line (equal values are equal lines); `weight_a` and `weight_b` the
/// number of characters other than whitespace of each line.
pub fn optimize_line_chunks<T: PartialEq>(
    a: &[T],
    b: &[T],
    weight_a: &[usize],
    weight_b: &[usize],
    unchanged: &[Unchanged],
) -> Vec<Unchanged> {
    let mut ranges: Vec<Unchanged> = Vec::with_capacity(unchanged.len());
    for &u in unchanged {
        ranges.push(u);
        settle_last(&mut ranges, a, b, weight_a, weight_b);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = concat!(
        "keep\n",
        "<<<<<<< HEAD\n",
        "ours 1\n",
        "ours 2\n",
        "||||||| base\n",
        "base\n",
        "=======\n",
        "theirs\n",
        ">>>>>>> feature\n",
        "tail\n",
        "<<<<<<< HEAD\n",
        "=======\n",
        "only theirs\n",
        ">>>>>>> feature\n",
    );

    fn marker_lines<'a>(text: &'a str, block: &ConflictBlock) -> Vec<&'a str> {
        block.markers.iter().map(|m| &text[m.clone()]).collect()
    }

    // src/features/vcs/__tests__/conflicts.test.ts › parseConflicts › parses diff3 and plain blocks
    #[test]
    fn parses_diff3_and_plain_blocks() {
        let blocks = parse_conflicts(TEXT);
        assert_eq!(blocks.len(), 2);
        let (a, b) = (&blocks[0], &blocks[1]);
        assert_eq!(a.ours, "ours 1\nours 2\n");
        assert_eq!(a.base.as_deref(), Some("base\n"));
        assert_eq!(a.theirs, "theirs\n");
        assert_eq!(a.ours_label, "HEAD");
        assert_eq!(a.theirs_label, "feature");
        let whole = &TEXT[a.from..a.to];
        assert!(whole.starts_with("<<<<<<< HEAD\n") && whole.ends_with(">>>>>>> feature\n"));
        assert_eq!(b.ours, "");
        assert_eq!(b.base, None);
        assert_eq!(b.theirs, "only theirs\n");
        let inside = TEXT.find("theirs\n").unwrap();
        assert!(block_at(&blocks, inside).is_some_and(|found| std::ptr::eq(found, a)));
        assert_eq!(block_at(&blocks, 0), None);
    }

    // src/features/vcs/__tests__/conflicts.test.ts › parseConflicts › locates the marker lines, without their terminators
    #[test]
    fn locates_the_marker_lines_without_their_terminators() {
        let blocks = parse_conflicts(TEXT);
        assert_eq!(marker_lines(TEXT, &blocks[0]), ["<<<<<<< HEAD", "||||||| base", "=======", ">>>>>>> feature"]);
        assert_eq!(marker_lines(TEXT, &blocks[1]), ["<<<<<<< HEAD", "=======", ">>>>>>> feature"]);
        let crlf = "<<<<<<< a\r\nx\r\n=======\r\ny\r\n>>>>>>> b\r\n";
        assert_eq!(marker_lines(crlf, &parse_conflicts(crlf)[0]), ["<<<<<<< a", "=======", ">>>>>>> b"]);
    }

    // src/features/vcs/__tests__/conflicts.test.ts › parseConflicts › ignores unterminated markers
    #[test]
    fn ignores_unterminated_markers() {
        assert!(parse_conflicts("<<<<<<< HEAD\nx\n=======\ny\n").is_empty());
    }

    // src/features/vcs/__tests__/conflicts.test.ts › parseConflicts › resolves blocks
    #[test]
    fn resolves_blocks() {
        assert_eq!(resolve_all(TEXT, Resolution::Ours, false), "keep\nours 1\nours 2\ntail\n");
        assert_eq!(resolve_all(TEXT, Resolution::Theirs, false), "keep\ntheirs\ntail\nonly theirs\n");
        let blocks = parse_conflicts(TEXT);
        assert_eq!(resolution_text(&blocks[0], Resolution::Both, false), "ours 1\nours 2\ntheirs\n");
        assert_eq!(resolution_text(&blocks[0], Resolution::Base, false), "base\n");
    }

    // src/features/vcs/__tests__/conflicts.test.ts › parseConflicts › swaps sides during a rebase, where the user's commit is the >>>>>>> section
    #[test]
    fn swaps_sides_during_a_rebase() {
        assert_eq!(resolve_all(TEXT, Resolution::Ours, true), "keep\ntheirs\ntail\nonly theirs\n");
        assert_eq!(resolve_all(TEXT, Resolution::Theirs, true), "keep\nours 1\nours 2\ntail\n");
        let blocks = parse_conflicts(TEXT);
        assert_eq!(resolution_text(&blocks[0], Resolution::Both, true), "theirs\nours 1\nours 2\n");
        assert_eq!(resolution_text(&blocks[0], Resolution::Base, true), "base\n");
    }

    fn sides(operation: &str) -> MergeSides {
        MergeSides {
            operation: Some(operation.to_owned()),
            branch: Some("feature".to_owned()),
            commit: Some("abc12345".to_owned()),
            other_branch: Some("main".to_owned()),
        }
    }

    fn titles(yours: &str, theirs: &str) -> MergeTitles {
        MergeTitles { yours: yours.to_owned(), theirs: theirs.to_owned() }
    }

    // src/features/vcs/__tests__/conflicts.test.ts › mergeTitles › names the sides like IDEA for each operation
    #[test]
    fn names_the_sides_like_idea_for_each_operation() {
        assert_eq!(
            merge_titles(Some(&sides("rebase"))),
            titles("Rebasing abc12345 from feature", "Already rebased commits and commits from main")
        );
        let onto_unknown = MergeSides { other_branch: None, ..sides("rebase") };
        assert_eq!(merge_titles(Some(&onto_unknown)).theirs, "Already rebased commits");
        assert_eq!(merge_titles(Some(&sides("merge"))), titles("Changes from feature", "Changes from main"));
        assert_eq!(
            merge_titles(Some(&sides("cherry-pick"))),
            titles("Local Changes", "Changes from cherry-pick abc12345")
        );
        assert_eq!(merge_titles(Some(&sides("unstash"))), titles("Your uncommitted changes", "Changes from feature"));
        assert_eq!(merge_titles(None), titles("Your version", "Theirs (incoming)"));
    }

    const ALL: [&str; 6] = ["if (a) {", "  x();", "}", "if (b) {", "  y();", ""];
    const BLOCK: [&str; 3] = ["if (a) {", "  x();", "}"];

    /// One value per distinct line, and the weight (characters other than blanks) of each.
    fn encode(lines: &[&str]) -> (Vec<usize>, Vec<usize>) {
        let ids = lines.iter().map(|line| ALL.iter().position(|known| known == line).unwrap()).collect();
        let weights = lines.iter().map(|line| line.chars().filter(|c| !c.is_whitespace()).count()).collect();
        (ids, weights)
    }

    // src/features/vcs/__tests__/chunkOptimizer.test.ts › optimizeLineChunks › moves a block that starts in the middle of a copy to where the copy starts
    #[test]
    fn moves_a_block_that_starts_in_the_middle_of_a_copy_to_where_the_copy_starts() {
        // The base has the block `if (a) {…}` then `if (b) {…}`; the left repeats the first block.
        // A diff may say the lines "}", "if (a) {", "  x();" were inserted after the first two.
        let other = ["if (b) {", "  y();", "}"];
        let base: Vec<&str> = BLOCK.iter().chain(&other).copied().collect();
        let left: Vec<&str> = BLOCK.iter().chain(&BLOCK).chain(&other).copied().collect();
        let (a, weight_a) = encode(&base);
        let (b, weight_b) = encode(&left);
        let misaligned = [Unchanged { s1: 0, e1: 2, s2: 0, e2: 2 }, Unchanged { s1: 2, e1: 6, s2: 5, e2: 9 }];
        let out = optimize_line_chunks(&a, &b, &weight_a, &weight_b, &misaligned);
        // The inserted lines become a whole block: lines 0–3 of the left.
        assert_eq!(changes_between(&out, base.len(), left.len()), [[0, 0, 0, 3]]);
    }

    // src/features/vcs/__tests__/chunkOptimizer.test.ts › optimizeLineChunks › leaves a change that already starts and ends at a blank line alone
    #[test]
    fn leaves_a_change_that_already_starts_and_ends_at_a_blank_line_alone() {
        let (a, weight_a) = encode(&["if (a) {", "", "  x();"]);
        let (b, weight_b) = encode(&["if (a) {", "", "  y();", "", "  x();"]);
        let found = [Unchanged { s1: 0, e1: 2, s2: 0, e2: 2 }, Unchanged { s1: 2, e1: 3, s2: 4, e2: 5 }];
        assert_eq!(optimize_line_chunks(&a, &b, &weight_a, &weight_b, &found), found);
    }
}
