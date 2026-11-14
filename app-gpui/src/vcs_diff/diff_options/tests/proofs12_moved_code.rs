use super::*;

const FIRST: &str = "function first() {\n  return 'first';\n}\n";
const SECOND: &str = "function second() {\n  return 'second';\n}\n";
const THIRD: &str = "function third() {\n  return 'third';\n}\n";
const FOURTH: &str = "function fourth() {\n  return 'fourth';\n}\n";
const FIFTH: &str = "function fifth() {\n  return 'fifth';\n}\n";
const ORIGINAL: &str =
    "function compute() {\n  const value = calculate(input);\n  record(value);\n  return value + 1;\n}\n";

fn compare(a: &str, b: &str) -> DiffChunks {
    compare_ignoring(a, b, IgnorePolicy::None)
}

fn compare_ignoring(a: &str, b: &str, ignore: IgnorePolicy) -> DiffChunks {
    idea_chunks(&Doc::new(a), &Doc::new(b), ignore, HighlightPolicy::Words)
}

/// The old and the new text of a moved block, by its UTF-16 offsets.
fn moved_texts(moved: &MovedBlock, a: &str, b: &str) -> (String, String) {
    (Doc::new(a).slice_string(moved.from_a, moved.to_a), Doc::new(b).slice_string(moved.from_b, moved.to_b))
}

/// The word edits of a moved block, as the old and the new text of each, by their offsets into the block.
fn edits(moved: &MovedBlock, a: &str, b: &str) -> Vec<(String, String)> {
    let (old, new) = moved_texts(moved, a, b);
    let (old, new) = (Doc::new(&old), Doc::new(&new));
    moved.changes.iter().map(|c| (old.slice_string(c.start1, c.end1), new.slice_string(c.start2, c.end2))).collect()
}

/// `b` with every chunk copied back from `a`, last chunk first: the text Revert of every change gives.
fn reverted(compared: &DiffChunks, a: &str, b: &str) -> String {
    restore(&compared.chunks, &Doc::new(a), &Doc::new(b))
}

fn indented(text: &str, by: &str) -> String {
    let lines: Vec<String> =
        text.split('\n').map(|line| if line.is_empty() { String::new() } else { format!("{by}{line}") }).collect();
    lines.join("\n")
}

// src/features/vcs/__tests__/movedCode.test.tsx › built-in moved block detection › pairs a moved function while
// preserving all transferable line hunks
#[test]
fn pairs_a_moved_function_while_preserving_all_transferable_line_hunks() {
    let (a, b) = (format!("{FIRST}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{FIRST}"));
    let compared = compare(&a, &b);
    assert_eq!(compared.moves.len(), 1);
    let first = FIRST.trim_end().to_owned();
    assert_eq!(moved_texts(&compared.moves[0], &a, &b), (first.clone(), first));
    assert_eq!(reverted(&compared, &a, &b), a);
}

// src/features/vcs/__tests__/movedCode.test.tsx › built-in moved block detection › recognizes reindented moved code
// and records token changes separately
#[test]
fn recognizes_reindented_moved_code_and_records_token_changes_separately() {
    let before = format!("{FIRST}{SECOND}{THIRD}");
    let reindented = format!("{SECOND}{THIRD}{}", indented(FIRST, "    "));
    let compared = compare(&before, &reindented);
    assert_eq!(compared.moves.len(), 1);
    assert!(compared.moves[0].changes.is_empty(), "indentation is not an edit");
    let changed = FIRST.replace("'first'", "'changed'");
    let after = format!("{SECOND}{THIRD}{changed}");
    let compared = compare(&before, &after);
    assert_eq!(compared.moves.len(), 1);
    assert_eq!(edits(&compared.moves[0], &before, &after), [("first".to_owned(), "changed".to_owned())]);
}

// src/features/vcs/__tests__/movedCode.test.tsx › built-in moved block detection › does not label copied code,
// repeated blocks, braces or indentation-only edits as moves
#[test]
fn does_not_label_copied_code_repeated_blocks_braces_or_indentation_only_edits_as_moves() {
    let before = format!("{FIRST}{SECOND}{THIRD}");
    assert!(compare(&before, &format!("{before}{FIRST}")).moves.is_empty(), "copied code");
    let repeated = (format!("{FIRST}{FIRST}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{FIRST}{FIRST}"));
    assert!(compare(&repeated.0, &repeated.1).moves.is_empty(), "repeated blocks");
    assert!(compare("{\n}\nkeep\n", "keep\n{\n}\n").moves.is_empty(), "braces");
    assert!(compare(FIRST, &FIRST.replace("  return", "    return")).moves.is_empty(), "indentation only");
}

// src/features/vcs/__tests__/movedCode.test.tsx › built-in moved block detection › handles Unicode, no final newline
// and multiple independently moved blocks
#[test]
fn handles_unicode_no_final_newline_and_multiple_independently_moved_blocks() {
    let unicode = "function привет() {\n  return '你好 😀';\n}\n";
    let a = format!("{unicode}{SECOND}{THIRD}");
    let b = format!("{SECOND}{THIRD}{unicode}").trim_end().to_owned();
    let compared = compare(&a, &b);
    assert_eq!(compared.moves.len(), 1);
    let block = unicode.trim_end().to_owned();
    assert_eq!(moved_texts(&compared.moves[0], &a, &b), (block.clone(), block));
    let five = format!("{FIRST}{SECOND}{THIRD}{FOURTH}{FIFTH}");
    let shuffled = format!("{SECOND}{THIRD}{FIRST}{FIFTH}{FOURTH}");
    let compared = compare(&five, &shuffled);
    assert_eq!(compared.moves.len(), 2);
    assert!(compared.moves.iter().all(|moved| moved.changes.is_empty()));
}

// src/features/vcs/__tests__/movedCode.test.tsx › built-in moved block detection › classifies whitespace-only
// differences under every ignore policy
#[test]
fn classifies_whitespace_only_differences_under_every_ignore_policy() {
    for (ignore, _) in IGNORE_OPTIONS {
        assert!(compare_ignoring("let a = 1;\n", "  let a = 1;\n\n", ignore).whitespace_only, "{ignore:?}");
        assert!(!compare_ignoring("let a = 1;\n", "let a = 2;\n", ignore).whitespace_only, "{ignore:?}");
        assert!(!compare_ignoring("same", "same", ignore).whitespace_only, "{ignore:?}");
    }
}

// src/features/vcs/__tests__/movedCode.test.tsx › edited moves › pairs a moved block with a %s and preserves revert
#[test]
fn pairs_a_moved_block_with_an_edit_and_preserves_revert() {
    let cases = [
        ("body edit", ORIGINAL.replace("+ 1", "+ 2")),
        ("added line", ORIGINAL.replace("  return", "  validate(value);\n  return")),
        ("deleted line", ORIGINAL.replace("  record(value);\n", "")),
        ("renamed function", ORIGINAL.replace("compute", "renamed")),
        ("reindent and edit", indented(&ORIGINAL.replace("+ 1", "+ 2"), "  ")),
    ];
    for (name, changed) in cases {
        let (a, b) = (format!("{ORIGINAL}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{changed}"));
        let compared = compare(&a, &b);
        assert_eq!(compared.moves.len(), 1, "{name}");
        let moved = &compared.moves[0];
        let expected = (ORIGINAL.trim_end().to_owned(), changed.trim_end().to_owned());
        assert_eq!(moved_texts(moved, &a, &b), expected, "{name}");
        assert!(!moved.changes.is_empty(), "{name}: the edit is recorded");
        assert_eq!(reverted(&compared, &a, &b), a, "{name}");
    }
}

// src/features/vcs/__tests__/movedCode.test.tsx › edited moves › does not classify an ordinary edit shifted by nearby
// insertions as a move
#[test]
fn does_not_classify_an_ordinary_edit_shifted_by_nearby_insertions_as_a_move() {
    let a = format!("{ORIGINAL}{SECOND}{THIRD}");
    let b = format!("// new header\n{}{SECOND}{THIRD}", ORIGINAL.replace("+ 1", "+ 2"));
    assert!(compare(&a, &b).moves.is_empty());
}

// src/features/vcs/__tests__/movedCode.test.tsx › edited moves › rejects major rewrites, repeated alternatives and
// weak matches between unrelated functions
#[test]
fn rejects_major_rewrites_repeated_alternatives_and_weak_matches_between_unrelated_functions() {
    let rewrite = "function compute() {\n  throw new Error('not implemented');\n}\n";
    let a = format!("{ORIGINAL}{SECOND}{THIRD}");
    assert!(compare(&a, &format!("{SECOND}{THIRD}{rewrite}")).moves.is_empty(), "a major rewrite");
    let unrelated = FIRST.replace("function first", "function unrelated");
    let tiny = (format!("{FIRST}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{unrelated}"));
    assert!(compare(&tiny.0, &tiny.1).moves.is_empty(), "a weak match");
    let changed = ORIGINAL.replace("+ 1", "+ 2");
    let repeated = (format!("{ORIGINAL}{ORIGINAL}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{changed}{changed}"));
    assert!(compare(&repeated.0, &repeated.1).moves.iter().all(|moved| moved.changes.is_empty()), "alternatives");
}

// src/features/vcs/__tests__/movedCode.test.tsx › edited moves › keeps UTF-16 edit offsets correct for Unicode and a
// missing final newline
#[test]
fn keeps_utf16_edit_offsets_correct_for_unicode_and_a_missing_final_newline() {
    let a_block = ORIGINAL.replace("calculate(input)", "calculate('你好 😀')");
    let b_block = a_block.replace("+ 1", "+ 2").trim_end().to_owned();
    let (a, b) = (format!("{a_block}{SECOND}{THIRD}"), format!("{SECOND}{THIRD}{b_block}"));
    let compared = compare(&a, &b);
    assert_eq!(compared.moves.len(), 1);
    assert_eq!(edits(&compared.moves[0], &a, &b), [("1".to_owned(), "2".to_owned())]);
}

// src/features/vcs/__tests__/movedCode.test.tsx › edited moves › leaves large approximate comparisons usable as
// ordinary line hunks
#[test]
fn leaves_large_approximate_comparisons_usable_as_ordinary_line_hunks() {
    let calls: String = (0..300).map(|i| format!("  call{i}();\n")).collect();
    let large = format!("function large() {{\n{calls}}}\n");
    let a = format!("{large}{SECOND}{THIRD}");
    let b = format!("{SECOND}{THIRD}{}", large.replace("call150()", "changed150()"));
    let compared = compare(&a, &b);
    assert!(compared.moves.iter().all(|moved| moved.changes.is_empty()));
    assert!(!compared.chunks.is_empty());
}
