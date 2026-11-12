//! IDEA puts a new line breakpoint only where there is code: a gutter click or Toggle Line
//! Breakpoint on a blank or comment-only line does nothing. Comments come from the syntax tree; in
//! a file without a parser only blank lines are skipped.

use crate::editor::buffer;
use ropey::Rope;
use std::ops::Range;

/// Whether `line` (0-based) has anything besides whitespace and comments. `comments`: the
/// document's comment nodes as UTF-16 offset ranges (none for a file without a parser).
pub fn line_has_code(doc: &Rope, line: u32, comments: &[Range<u32>]) -> bool {
    if line >= buffer::line_count(doc) {
        return false;
    }
    let start = buffer::line_range(doc, line).start;
    let text = buffer::line_text(doc, line);
    // Characters before this offset are inside a comment already seen.
    let mut skip_to = start;
    let mut pos = start;
    for c in text.chars() {
        let at = pos;
        pos += c.len_utf16() as u32;
        if at < skip_to || c.is_whitespace() {
            continue;
        }
        match comments.iter().find(|comment| comment.start <= at && at < comment.end) {
            Some(comment) => skip_to = comment.end,
            None => return true,
        }
    }
    false
}

#[cfg(test)]
pub(crate) mod testing {
    use std::ops::Range;

    /// The `//` and `/* */` comments of ASCII source text, as a syntax tree would report them.
    pub fn c_comments(text: &str) -> Vec<Range<u32>> {
        let bytes = text.as_bytes();
        let mut comments = Vec::new();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let end = match &bytes[i..i + 2] {
                b"//" => text[i..].find('\n').map_or(text.len(), |eol| i + eol),
                b"/*" => text[i + 2..].find("*/").map_or(text.len(), |close| i + 2 + close + 2),
                _ => {
                    i += 1;
                    continue;
                }
            };
            comments.push(i as u32..end as u32);
            i = end;
        }
        comments
    }
}

#[cfg(test)]
mod tests {
    use super::testing::c_comments;
    use super::*;

    fn lines(text: &str, comments: &[Range<u32>]) -> Vec<bool> {
        let doc = Rope::from_str(text);
        (0..buffer::line_count(&doc)).map(|line| line_has_code(&doc, line, comments)).collect()
    }

    #[test]
    fn skips_blank_and_comment_only_lines() {
        let text = [
            "class A {",
            "",
            "   \t",
            "  // note",
            "  /** doc",
            "   * more",
            "   */",
            "  int f() { return 1; } // trailing",
            "  /* a */ int x = 1;",
            "  /* a */ /* b */",
            "}",
        ]
        .join("\n");
        assert_eq!(
            lines(&text, &c_comments(&text)),
            [true, false, false, false, false, false, false, true, true, false, true],
        );
    }

    #[test]
    fn only_skips_blank_lines_in_a_file_without_a_parser() {
        assert_eq!(lines("x\n\n  \n// y", &[]), [true, false, false, true]);
    }

    // Not from a spec: a line past the end of the document has no code.
    #[test]
    fn a_line_past_the_end_has_no_code() {
        let doc = Rope::from_str("x");
        assert!(!line_has_code(&doc, 1, &[]));
    }
}
