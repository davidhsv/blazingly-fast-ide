//! Find in console output (Run, Debug): every occurrence in every line, with the Match Case / Words / Regex
//! options. Matches carry the line's stable id so the find bar can keep the current match while output keeps
//! arriving.

use std::ops::Range;

/// The find bar's toggles; all off by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub case_sensitive: bool,
    pub whole_word: bool,
    pub regex: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConsoleMatch {
    /// Index into the console's current lines.
    pub line: usize,
    /// Stable line id (survives trimming of older lines).
    pub line_id: u64,
    /// Byte offsets into the line's text, on character boundaries.
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FindResult {
    pub matches: Vec<ConsoleMatch>,
    /// More matches exist than were collected.
    pub overflow: bool,
    /// The query is an invalid regular expression.
    pub invalid: bool,
}

/// Matches collected per search; the counter shows `N+` beyond this.
pub const MAX_MATCHES: usize = 2000;

/// A compiled regular expression: every match in one line of text, in order and without overlaps, as byte
/// ranges on character boundaries. It may report empty matches; the search drops them.
pub type LineMatcher = Box<dyn Fn(&str) -> Vec<Range<usize>>>;

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Whether the characters around `start..end` leave it standing as a word of its own.
fn is_whole_word(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    before.is_none_or(|c| !is_word_char(c)) && after.is_none_or(|c| !is_word_char(c))
}

/// The key two characters share when they differ only by case, one character for one character: `ſ`
/// compares equal to `s`, `ß` only to `ẞ`.
fn fold_case(c: char) -> char {
    // Dotless ı pairs with I only under Turkic rules, so it stays apart from i.
    if c == 'ı' {
        return c;
    }
    let upper = single(c.to_uppercase()).unwrap_or(c);
    single(upper.to_lowercase()).unwrap_or(upper)
}

fn single(mut chars: impl Iterator<Item = char>) -> Option<char> {
    let first = chars.next()?;
    chars.next().is_none().then_some(first)
}

/// Byte length of the start of `text` that equals the folded `needle` when case is ignored.
fn folded_prefix(text: &str, needle: &[char]) -> Option<usize> {
    let mut chars = text.chars();
    for &want in needle {
        if fold_case(chars.next()?) != want {
            return None;
        }
    }
    Some(text.len() - chars.as_str().len())
}

/// Occurrences of the folded, non-empty `needle` in `text` ignoring case, left to right without overlaps.
fn folded_matches(text: &str, needle: &[char]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let at = text.len() - rest.len();
        let step = match folded_prefix(rest, needle) {
            Some(len) => {
                out.push(at..at + len);
                len
            }
            None => c.len_utf8(),
        };
        rest = &rest[step..];
    }
    out
}

/// All matches of `query` in `lines` (stable id and text of each), in document order, stopping at `limit`.
///
/// With the Regex option the query is handed to `compile_regex` together with whether case matters; it
/// returns `None` for an invalid regular expression. It is not called otherwise.
pub fn find_in_lines<'a>(
    lines: impl IntoIterator<Item = (u64, &'a str)>,
    query: &str,
    options: FindOptions,
    limit: usize,
    compile_regex: &dyn Fn(&str, bool) -> Option<LineMatcher>,
) -> FindResult {
    if query.is_empty() {
        return FindResult::default();
    }
    let matcher: LineMatcher = if options.regex {
        let Some(matcher) = compile_regex(query, options.case_sensitive) else {
            return FindResult { invalid: true, ..FindResult::default() };
        };
        matcher
    } else if options.case_sensitive {
        let needle = query.to_owned();
        Box::new(move |text: &str| text.match_indices(needle.as_str()).map(|(at, hit)| at..at + hit.len()).collect())
    } else {
        let needle: Vec<char> = query.chars().map(fold_case).collect();
        Box::new(move |text: &str| folded_matches(text, &needle))
    };
    let mut matches = Vec::new();
    for (line, (line_id, text)) in lines.into_iter().enumerate() {
        for Range { start, end } in matcher(text) {
            // An empty match is no occurrence, and offsets off a character boundary cannot be highlighted.
            if text.get(start..end).is_none_or(str::is_empty) {
                continue;
            }
            if options.whole_word && !is_whole_word(text, start, end) {
                continue;
            }
            if matches.len() >= limit {
                return FindResult { matches, overflow: true, invalid: false };
            }
            matches.push(ConsoleMatch { line, line_id, start, end });
        }
    }
    FindResult { matches, overflow: false, invalid: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stands in for a regular expression engine on the patterns used below: `(` does not compile, `x*`
    /// finds an empty match at every position of text without an `x`, and anything else is literal
    /// characters with `.` for any one character. The fixtures are ASCII, so characters are bytes.
    fn compile(pattern: &str, _case_sensitive: bool) -> Option<LineMatcher> {
        match pattern {
            "(" => None,
            "x*" => Some(Box::new(|text: &str| (0..=text.len()).map(|at| at..at).collect())),
            _ => {
                let pattern = pattern.as_bytes().to_vec();
                Some(Box::new(move |text: &str| {
                    let mut out = Vec::new();
                    let mut at = 0;
                    while let Some(window) = text.as_bytes().get(at..at + pattern.len()) {
                        if pattern.iter().zip(window).all(|(p, c)| *p == b'.' || p == c) {
                            out.push(at..at + pattern.len());
                            at += pattern.len();
                        } else {
                            at += 1;
                        }
                    }
                    out
                }))
            }
        }
    }

    fn find(texts: &[&str], query: &str, options: FindOptions, limit: usize) -> FindResult {
        let lines = texts.iter().zip(1..).map(|(text, id)| (id, *text));
        find_in_lines(lines, query, options, limit, &compile)
    }

    fn hit(line: usize, line_id: u64, start: usize, end: usize) -> ConsoleMatch {
        ConsoleMatch { line, line_id, start, end }
    }

    const CASE_SENSITIVE: FindOptions = FindOptions { case_sensitive: true, whole_word: false, regex: false };
    const WHOLE_WORD: FindOptions = FindOptions { case_sensitive: false, whole_word: true, regex: false };
    const REGEX: FindOptions = FindOptions { case_sensitive: false, whole_word: false, regex: true };
    const REGEX_WHOLE_WORD: FindOptions = FindOptions { case_sensitive: false, whole_word: true, regex: true };

    // src/ui/consoleFind.test.ts › findInLines › finds every occurrence on every line, ignoring case by default
    #[test]
    fn finds_every_occurrence_on_every_line_ignoring_case_by_default() {
        let texts = ["alpha beta", "gamma Beta", "BETA again"];
        let result = find(&texts, "beta", FindOptions::default(), MAX_MATCHES);
        assert_eq!(result.matches, vec![hit(0, 1, 6, 10), hit(1, 2, 6, 10), hit(2, 3, 0, 4)]);
    }

    // src/ui/consoleFind.test.ts › findInLines › matches case only when asked
    #[test]
    fn matches_case_only_when_asked() {
        let result = find(&["beta Beta BETA"], "Beta", CASE_SENSITIVE, MAX_MATCHES);
        assert_eq!(result.matches, vec![hit(0, 1, 5, 9)]);
    }

    // src/ui/consoleFind.test.ts › findInLines › keeps whole words only
    #[test]
    fn keeps_whole_words_only() {
        let result = find(&["cat concatenate cat."], "cat", WHOLE_WORD, MAX_MATCHES);
        assert_eq!(result.matches, vec![hit(0, 1, 0, 3), hit(0, 1, 16, 19)]);
    }

    // src/ui/consoleFind.test.ts › findInLines › treats a literal query literally
    #[test]
    fn treats_a_literal_query_literally() {
        let result = find(&["a.b axb a.b"], "a.b", FindOptions::default(), MAX_MATCHES);
        assert_eq!(result.matches.iter().map(|m| m.start).collect::<Vec<_>>(), [0, 8]);
    }

    // src/ui/consoleFind.test.ts › findInLines › uses a regular expression when asked
    #[test]
    fn uses_a_regular_expression_when_asked() {
        assert_eq!(find(&["beta betta b3ta"], "b.ta", REGEX, MAX_MATCHES).matches.len(), 2);
    }

    // src/ui/consoleFind.test.ts › findInLines › applies whole words to regular expression matches
    #[test]
    fn applies_whole_words_to_regular_expression_matches() {
        let result = find(&["cat concatenate cat"], "c.t", REGEX_WHOLE_WORD, MAX_MATCHES);
        assert_eq!(result.matches, vec![hit(0, 1, 0, 3), hit(0, 1, 16, 19)]);
    }

    // src/ui/consoleFind.test.ts › findInLines › reports an invalid regular expression instead of throwing
    #[test]
    fn reports_an_invalid_regular_expression() {
        let result = find(&["anything"], "(", REGEX, MAX_MATCHES);
        assert_eq!(result, FindResult { matches: vec![], overflow: false, invalid: true });
    }

    // src/ui/consoleFind.test.ts › findInLines › skips empty regular expression matches
    #[test]
    fn skips_empty_regular_expression_matches() {
        assert!(find(&["abc"], "x*", REGEX, MAX_MATCHES).matches.is_empty());
    }

    // src/ui/consoleFind.test.ts › findInLines › caps the matches and reports the overflow
    #[test]
    fn caps_the_matches_and_reports_the_overflow() {
        let result = find(&["x x x x"], "x", FindOptions::default(), 2);
        assert_eq!(result.matches.len(), 2);
        assert!(result.overflow);
    }

    // src/ui/consoleFind.test.ts › findInLines › has no overflow when the cap is exact
    #[test]
    fn has_no_overflow_when_the_cap_is_exact() {
        assert!(!find(&["x x"], "x", FindOptions::default(), 2).overflow);
        assert_eq!(find(&["x x"], "x", FindOptions::default(), MAX_MATCHES).matches.len(), 2);
    }

    // src/ui/consoleFind.test.ts › findInLines › returns nothing for an empty query
    #[test]
    fn returns_nothing_for_an_empty_query() {
        assert_eq!(find(&["a b"], " ", FindOptions::default(), MAX_MATCHES).matches.len(), 1);
        assert!(find(&["text"], "", FindOptions::default(), MAX_MATCHES).matches.is_empty());
    }

    // Offsets count bytes ("Ü" and "ï" take two each), and letters outside ASCII fold and bound words too.
    #[test]
    fn handles_text_outside_ascii() {
        let result = find(&["Ünïcode ünÏCODE"], "ünïcode", FindOptions::default(), MAX_MATCHES);
        assert_eq!(result.matches, vec![hit(0, 1, 0, 9), hit(0, 1, 10, 19)]);
        assert_eq!(find(&["Ünïcode ünÏCODE"], "ünïcode", CASE_SENSITIVE, MAX_MATCHES).matches, vec![]);
        assert_eq!(find(&["naïve na"], "na", WHOLE_WORD, MAX_MATCHES).matches, vec![hit(0, 1, 7, 9)]);
    }
}
