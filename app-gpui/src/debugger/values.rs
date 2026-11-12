//! Presentation helpers for debugger values.

use crate::find::utf16;
use crate::platform::collate;
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// What a value string looks like, for colouring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Number,
    String,
    Keyword,
    Object,
    Plain,
}

const KEYWORDS: [&str; 13] = [
    "null",
    "nil",
    "None",
    "undefined",
    "true",
    "false",
    "True",
    "False",
    "NaN",
    "Infinity",
    "-Infinity",
    "nullptr",
    "<nil>",
];

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the value pattern is valid")
}

static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^[-+]?(?:0[xX][0-9a-fA-F]+|0[bB][01]+|0[oO][0-7]+",
        r"|(?:[0-9][0-9_]*\.?[0-9_]*|\.[0-9]+)(?:[eE][-+]?[0-9]+)?)(?:[A-Za-z][0-9A-Za-z_]*)?$",
    ))
});
static OBJECT_HEAD: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[0-9A-Za-z_$.:<>]+\s*[{(@]"));
static STRING_TYPE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u:\b)(?:str|string|char|&str)(?-u:\b)"));
static LINE_BREAK: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s*\n\s*"));
static IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| pattern(r"[A-Za-z_$][0-9A-Za-z_$]*"));
static INDEX: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\[?[0-9]+\]?$"));

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Whether `v` reads as a quoted string: the same quote at both ends, or a Python-style prefixed
/// literal (`b'…'`).
fn is_quoted(v: &str) -> bool {
    let mut chars = v.chars();
    let (first, last) = (chars.next(), chars.next_back());
    if matches!((first, last), (Some(open), Some(close)) if open == close && "'\"`".contains(open)) {
        return true;
    }
    let body = v.strip_prefix(['b', 'r', 'u']).unwrap_or(v);
    let opens = body.starts_with(['\'', '"']) || v.starts_with(['\'', '"']);
    opens && v.ends_with(['\'', '"'])
}

/// Classifies a value string: numbers, strings, keywords (null/true…), objects (`{…}`, `[…]`).
/// `type_name` settles what the text leaves open.
pub fn classify_value(value: &str, type_name: Option<&str>) -> ValueKind {
    let v = value.trim();
    if v.is_empty() {
        return ValueKind::Plain;
    }
    if KEYWORDS.contains(&v) {
        return ValueKind::Keyword;
    }
    if is_quoted(v) {
        return ValueKind::String;
    }
    if NUMBER.is_match(v) {
        return ValueKind::Number;
    }
    let bracketed = v.starts_with('<') && v.ends_with('>') && v.len() > 1 && !v.contains(is_line_terminator);
    if v.starts_with(['{', '[', '(']) || OBJECT_HEAD.is_match(v) || bracketed {
        return ValueKind::Object;
    }
    let type_name = type_name.unwrap_or_default().to_lowercase();
    if STRING_TYPE.is_match(&type_name) { ValueKind::String } else { ValueKind::Plain }
}

/// The value on one line: a multi-line rendering (a pandas Series, a pretty-printed object) joined
/// with spaces.
pub fn one_line(value: &str) -> String {
    LINE_BREAK.replace_all(value, " ").into_owned()
}

/// Default length of an inline value, in UTF-16 code units.
pub const INLINE_VALUE_MAX: u32 = 60;

/// Single-line, length-limited value for inline display (`max` in UTF-16 code units).
pub fn inline_value(value: &str, max: u32) -> String {
    let flat = one_line(value);
    if utf16::len(&flat) > max { format!("{}…", utf16::slice(&flat, 0, max.saturating_sub(1))) } else { flat }
}

/// Where the string literal opening at `open` ends (the index after its closing quote), if it
/// closes on the same line. An escaped quote closes it only when no later quote does.
fn literal_end(chars: &[char], open: usize) -> Option<usize> {
    let quote = chars[open];
    let mut ends: Vec<Option<usize>> = vec![None; chars.len() + 2];
    for at in (open + 1..chars.len()).rev() {
        let c = chars[at];
        ends[at] = if c == '\\' {
            let escaped = chars.get(at + 1).is_some_and(|next| !is_line_terminator(*next));
            escaped.then(|| ends[at + 2]).flatten().or(ends[at + 1])
        } else if c == quote {
            Some(at + 1)
        } else if is_line_terminator(c) {
            None
        } else {
            ends[at + 1]
        };
    }
    ends[open + 1]
}

/// Identifiers in a line of code, in order and once each (for matching inline values). The
/// contents of string literals are skipped, so `"total"` doesn't count.
pub fn identifiers_in(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut code = String::with_capacity(text.len());
    let mut at = 0;
    while at < chars.len() {
        let literal = if "\"'`".contains(chars[at]) { literal_end(&chars, at) } else { None };
        match literal {
            Some(end) => {
                code.push(' ');
                at = end;
            }
            None => {
                code.push(chars[at]);
                at += 1;
            }
        }
    }
    let mut seen = HashSet::new();
    IDENTIFIER.find_iter(&code).map(|m| m.as_str()).filter(|id| seen.insert(*id)).map(str::to_owned).collect()
}

/// Data Views › "Sort values alphabetically": children by name (numbers in names compared
/// numerically). Array / list elements keep their index order.
pub fn sort_variables<T: Clone>(vars: &[T], name: impl Fn(&T) -> &str) -> Vec<T> {
    let mut sorted = vars.to_vec();
    if !vars.iter().all(|v| INDEX.is_match(name(v))) {
        sorted.sort_by(|a, b| collate::compare_natural(name(a), name(b)));
    }
    sorted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_classification() {
        let cases = [
            ("42", ValueKind::Number),
            ("-3.5e10", ValueKind::Number),
            ("0x1F", ValueKind::Number),
            ("297.00", ValueKind::Number),
            ("10u32", ValueKind::Number),
            ("\"hello\"", ValueKind::String),
            ("'x'", ValueKind::String),
            ("b'bytes'", ValueKind::String),
            ("null", ValueKind::Keyword),
            ("None", ValueKind::Keyword),
            ("true", ValueKind::Keyword),
            ("undefined", ValueKind::Keyword),
            ("{CreateOrderRequest@4127}", ValueKind::Object),
            ("CreateOrderRequest@4127", ValueKind::Object),
            ("[1, 2, 3]", ValueKind::Object),
            ("Vec(3)", ValueKind::Object),
            ("<function main at 0x10>", ValueKind::Object),
            ("some text", ValueKind::Plain),
            ("", ValueKind::Plain),
        ];
        for (value, kind) in cases {
            assert_eq!(classify_value(value, None), kind, "{value}");
        }
    }

    #[test]
    fn uses_the_type_name_as_a_fallback_for_strings() {
        assert_eq!(classify_value("hello", Some("str")), ValueKind::String);
        assert_eq!(classify_value("hello", Some("Order")), ValueKind::Plain);
    }

    #[test]
    fn flattens_and_truncates_inline_values() {
        assert_eq!(inline_value("a\n  b", INLINE_VALUE_MAX), "a b");
        assert_eq!(inline_value(&"x".repeat(80), 10), "xxxxxxxxx…");
    }

    #[test]
    fn finds_identifiers_outside_string_literals() {
        assert_eq!(identifiers_in("log.info(\"total \" + total)"), ["log", "info", "total"]);
    }

    // Not from a spec: an unterminated literal hides nothing, and an escaped quote stays inside.
    #[test]
    fn reads_string_literals_as_the_source_line_has_them() {
        assert_eq!(identifiers_in("a = \"open"), ["a", "open"]);
        assert_eq!(identifiers_in(r#"say("he said \"hi\" twice", name)"#), ["say", "name"]);
        assert_eq!(identifiers_in(r#"x = "tail\" + y"#), ["x", "y"]);
    }

    #[test]
    fn sorts_by_name_case_insensitively_and_with_numbers_compared_numerically() {
        let names = ["b", "item10", "A", "item2"];
        assert_eq!(sort_variables(&names, |name| *name), ["A", "b", "item2", "item10"]);
    }

    #[test]
    fn keeps_array_elements_in_index_order() {
        let items = ["[0]", "[1]", "[10]", "[2]"];
        assert_eq!(sort_variables(&items, |name| *name), items);
    }
}
