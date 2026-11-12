//! Textual candidates for "replace all occurrences": spans of a text equal to an expression up to
//! the whitespace between its tokens. Callers confirm each span with the syntax tree (same node
//! kind, same resolution) before using it.

use super::text::slice16;

/// A word character of an expression: ASCII letters, digits, `_` and `$`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

/// The end (exclusive, possibly past the text) of the literal opened by the quote at `i`.
fn literal_end(chars: &[char], i: usize) -> usize {
    let quote = chars[i];
    let mut j = i + 1;
    while j < chars.len() && chars[j] != quote {
        j += if chars[j] == '\\' { 2 } else { 1 };
    }
    j + 1
}

/// Collapses the whitespace between tokens (one space stays between two word characters),
/// leaving string and character literals untouched.
pub fn normalize_expression(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut pending_space = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_quote(c) {
            if pending_space && out.chars().next_back().is_some_and(is_word) {
                out.push(' ');
            }
            pending_space = false;
            let end = literal_end(&chars, i).min(chars.len());
            out.extend(&chars[i..end]);
            i = end;
            continue;
        }
        if c.is_whitespace() {
            pending_space = true;
            i += 1;
            continue;
        }
        if pending_space && out.chars().next_back().is_some_and(is_word) && is_word(c) {
            out.push(' ');
        }
        pending_space = false;
        out.push(c);
        i += 1;
    }
    out
}

/// One token of an expression's pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// Whitespace that must be there (between two words).
    Space,
    Literal(Vec<char>),
}

fn tokens(expr: &str) -> (Vec<Token>, bool) {
    let norm: Vec<char> = normalize_expression(expr.trim()).chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < norm.len() {
        let c = norm[i];
        let end = if c == ' ' {
            out.push(Token::Space);
            i + 1
        } else if is_quote(c) {
            literal_end(&norm, i).min(norm.len())
        } else if is_word(c) {
            i + norm[i..].iter().take_while(|c| is_word(**c)).count()
        } else {
            i + 1
        };
        if c != ' ' {
            out.push(Token::Literal(norm[i..end].to_vec()));
        }
        i = end;
    }
    (out, norm.last().copied().is_some_and(is_word))
}

/// The end of a match of `tokens` at `start` of `chars`: whitespace may separate any two
/// tokens, and a match ending in a word must not run on into another word.
fn match_at(chars: &[char], start: usize, tokens: &[Token], word_tail: bool) -> Option<usize> {
    let skip_space = |at: usize| at + chars[at..].iter().take_while(|c| c.is_whitespace()).count();
    let mut at = start;
    for (k, token) in tokens.iter().enumerate() {
        match token {
            Token::Space => {
                let end = skip_space(at);
                if end == at {
                    return None;
                }
                at = end;
            }
            Token::Literal(literal) => {
                if k > 0 && tokens[k - 1] != Token::Space {
                    at = skip_space(at);
                }
                if !chars[at..].starts_with(literal) {
                    return None;
                }
                at += literal.len();
            }
        }
    }
    if word_tail && chars.get(at).copied().is_some_and(is_word) {
        return None;
    }
    Some(at)
}

/// Spans of `text[from..to]` matching `expr` (absolute UTF-16 offsets), at most `limit`.
pub fn find_text_occurrences(text: &str, from: u32, to: u32, expr: &str, limit: usize) -> Vec<(u32, u32)> {
    let scope: Vec<char> = slice16(text, from, to).chars().collect();
    let mut offsets = Vec::with_capacity(scope.len() + 1);
    let mut unit = from;
    offsets.push(unit);
    for c in &scope {
        unit += c.len_utf16() as u32;
        offsets.push(unit);
    }
    let (tokens, word_tail) = tokens(expr);
    let want = normalize_expression(expr.trim());
    let word_start = want.chars().next().is_some_and(is_word);
    let mut out = Vec::new();
    let mut last = 0;
    while last <= scope.len() {
        let Some((start, end)) =
            (last..=scope.len()).find_map(|start| match_at(&scope, start, &tokens, word_tail).map(|end| (start, end)))
        else {
            break;
        };
        let next = if end == start { start + 1 } else { end };
        if word_start && start > 0 && is_word(scope[start - 1]) {
            last = start + 1;
            continue;
        }
        last = next;
        if normalize_expression(&scope[start..end].iter().collect::<String>()) != want {
            continue;
        }
        out.push((offsets[start], offsets[end]));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// How many occurrences a search returns at most by default.
pub const OCCURRENCE_LIMIT: usize = 100;

/// An identifier-like word of an expression outside literals, with its UTF-16 offset in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionWord {
    pub word: String,
    pub offset: u32,
}

/// The identifier-like words of an expression outside its literals.
pub fn expression_words(expr: &str) -> Vec<ExpressionWord> {
    let chars: Vec<char> = expr.chars().collect();
    let mut out = Vec::new();
    let mut offset = 0u32;
    let mut i = 0;
    let units = |slice: &[char]| slice.iter().map(|c| c.len_utf16() as u32).sum::<u32>();
    while i < chars.len() {
        let c = chars[i];
        let next = if is_quote(c) {
            literal_end(&chars, i).min(chars.len())
        } else if (c.is_ascii_alphabetic() || c == '_' || c == '$') && (i == 0 || !is_word(chars[i - 1])) {
            let end = i + chars[i..].iter().take_while(|c| is_word(**c)).count();
            out.push(ExpressionWord { word: chars[i..end].iter().collect(), offset });
            end
        } else {
            i + 1
        };
        offset += units(&chars[i..next]);
        i = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(text: &str, from: u32, to: u32, expr: &str) -> Vec<(u32, u32)> {
        find_text_occurrences(text, from, to, expr, OCCURRENCE_LIMIT)
    }

    #[test]
    fn normalises_whitespace_between_tokens_but_not_inside_strings() {
        assert_eq!(normalize_expression("a  +\n b"), "a+b");
        assert_eq!(normalize_expression("new   Foo( 1 )"), "new Foo(1)");
        assert_eq!(normalize_expression("f( \"a  b\" )"), "f(\"a  b\")");
    }

    #[test]
    fn finds_spans_equal_up_to_whitespace_whole_words_only() {
        let text = "x = a + b; y = a+b; z = aa + b; w = \"a + b\"; v = a + bc;";
        let length = text.encode_utf16().count() as u32;
        let found: Vec<&str> =
            all(text, 0, length, "a + b").iter().map(|(from, to)| slice16(text, *from, *to)).collect();
        assert_eq!(found, ["a + b", "a+b", "a + b"]);
        assert_eq!(all(text, 10, length, "a+b").len(), 2);
    }

    #[test]
    fn lists_the_names_of_an_expression_outside_literals() {
        let words: Vec<String> = expression_words("foo(bar, \"baz\", qux.x)").into_iter().map(|w| w.word).collect();
        assert_eq!(words, ["foo", "bar", "qux", "x"]);
    }

    #[test]
    fn preserves_whole_word_matching_whitespace_normalization_and_adjacent_matches() {
        let text = "ab + c $b+c b + cd b+c b + c";
        assert_eq!(all(text, 0, 28, "b + c"), [(19, 22), (23, 28)]);
    }

    #[test]
    fn finds_valid_occurrences_overlapping_a_rejected_suffix_match() {
        assert_eq!(all("ab + b + b", 0, 10, "b + b"), [(5, 10)]);
    }

    #[test]
    fn advances_through_zero_length_matches() {
        assert_eq!(all("ab", 0, 2, ""), [(0, 0), (1, 1), (2, 2)]);
    }

    #[test]
    fn keeps_punctuation_led_expressions_and_scope_relative_boundaries() {
        assert_eq!(all("x(a) x(a)", 0, 9, "(a)"), [(1, 4), (6, 9)]);
        assert_eq!(all("ab", 1, 2, "b"), [(1, 2)]);
    }
}
