//! Copy and paste inside string literals: text pasted into a plain string literal is escaped for
//! it (quotes, backslashes, line breaks, control characters), and text copied from inside one is
//! unescaped, so a round trip through the clipboard gives back the text.
//!
//! A plain literal is a complete one-line string in double quotes (or single quotes in
//! JavaScript, TypeScript and Python) with no prefix: raw strings, text blocks, templates and
//! interpolated strings are left alone, as is anything a selection boundary would split (an
//! escape sequence, two literals).

use super::tree::{SyntaxTree, pascal_name};
use super::{Edit, EditState, SelectionExt, change, map_selection, utf16_len};
use crate::editor::buffer::{Assoc, col_of_byte};
use regex::Regex;
use std::sync::LazyLock;

/// The languages whose string literals are escaped on paste and unescaped on copy.
const LANGUAGES: [&str; 9] = ["java", "kotlin", "javascript", "typescript", "python", "rust", "go", "c", "cpp"];

fn is_js(language: &str) -> bool {
    matches!(language, "javascript" | "typescript")
}

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

static RUST_UNICODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^u\{([0-9a-fA-F][0-9a-fA-F_]{0,6})\}"));
static BRACED_UNICODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^u\{([0-9a-fA-F]+)\}"));
/// Java allows several `u`s in a Unicode escape.
static JAVA_UNICODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^u+([0-9a-fA-F]{4})"));
static UNICODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^u([0-9a-fA-F]{4})"));
static LONG_UNICODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^U([0-9a-fA-F]{8})"));
static HEX: LazyLock<Regex> = LazyLock::new(|| pattern(r"^x([0-9a-fA-F]+)"));
static BYTE_HEX: LazyLock<Regex> = LazyLock::new(|| pattern(r"^x([0-9a-fA-F]{2})"));
static OCTAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"^([0-7]{1,3})"));
/// Java octal escapes stop at `\377`.
static JAVA_OCTAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"^([0-3][0-7]{0,2}|[4-7][0-7]?)"));
static GO_OCTAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"^([0-7]{3})"));
/// A Rust line continuation skips the whitespace starting the next line.
static RUST_CONTINUATION: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\n\s*"));

/// What an escape sequence stands for.
#[derive(Debug, Clone, Copy)]
enum Decoded<'t> {
    Text(&'t str),
    /// A code point, possibly a surrogate (half of a pair written as two escapes).
    Code(u32),
}

/// An escape sequence: the byte index after it and what it stands for.
struct Escape<'t> {
    end: usize,
    value: Decoded<'t>,
}

/// The one-character escapes of `language`.
fn basic_escape(c: char, language: &str) -> Option<&'static str> {
    Some(match c {
        'n' => "\n",
        'r' => "\r",
        't' => "\t",
        '\\' => "\\",
        '"' => "\"",
        '\'' => "'",
        'b' if language != "rust" => "\u{8}",
        'f' if !matches!(language, "kotlin" | "rust") => "\u{c}",
        'a' if matches!(language, "python" | "go" | "c" | "cpp") => "\u{7}",
        'v' if !matches!(language, "java" | "kotlin" | "rust") => "\u{b}",
        '$' if language == "kotlin" => "$",
        's' if language == "java" => " ",
        '?' if matches!(language, "c" | "cpp") => "?",
        _ => return None,
    })
}

/// The digits `pattern` captures at the start of `rest`, and the length of the whole match.
fn capture<'t>(pattern: &Regex, rest: &'t str) -> Option<(&'t str, usize)> {
    let found = pattern.captures(rest)?;
    Some((found.get(1)?.as_str(), found.get(0)?.end()))
}

/// The escape sequence of `language` at byte `at` of `text` (a backslash). One that is not
/// valid stands for itself: the backslash and the character after it.
fn read_escape<'t>(text: &'t str, at: usize, language: &str) -> Escape<'t> {
    let rest = &text[at + 1..];
    let Some(c) = rest.chars().next() else {
        return Escape { end: text.len(), value: Decoded::Text(&text[at..]) };
    };
    if let Some(value) = basic_escape(c, language) {
        return Escape { end: at + 2, value: Decoded::Text(value) };
    }
    let js = is_js(language);
    let mut radix = 16;
    let numeric = match c {
        'u' => {
            let braced = match language {
                "rust" => capture(&RUST_UNICODE, rest),
                _ if js => capture(&BRACED_UNICODE, rest),
                _ => None,
            };
            if braced.is_none() && language != "rust" {
                capture(if language == "java" { &JAVA_UNICODE } else { &UNICODE }, rest)
            } else {
                braced
            }
        }
        'U' if matches!(language, "python" | "go" | "c" | "cpp") => capture(&LONG_UNICODE, rest),
        'x' if !matches!(language, "java" | "kotlin") => {
            capture(if matches!(language, "c" | "cpp") { &HEX } else { &BYTE_HEX }, rest)
        }
        '0'..='7' => {
            radix = 8;
            match language {
                "python" | "c" | "cpp" => capture(&OCTAL, rest),
                "java" => capture(&JAVA_OCTAL, rest),
                "go" => capture(&GO_OCTAL, rest),
                _ => {
                    let digit_after = rest[1..].starts_with(|next: char| next.is_ascii_digit());
                    if c == '0' && (language == "rust" || (js && !digit_after)) {
                        return Escape { end: at + 2, value: Decoded::Code(0) };
                    }
                    None
                }
            }
        }
        _ => None,
    };
    if let Some((digits, length)) = numeric {
        let value = u64::from_str_radix(&digits.replace('_', ""), radix).unwrap_or(u64::MAX);
        let scalar = !(0xd800..=0xdfff).contains(&value);
        let valid = value <= 0x10ffff
            && (scalar || !matches!(language, "rust" | "go"))
            && !(language == "rust" && c == 'x' && value > 0x7f)
            && !(language == "go" && radix == 8 && value > 0xff);
        if valid {
            return Escape { end: at + 1 + length, value: Decoded::Code(value as u32) };
        }
    }
    if c == '\n' && !matches!(language, "java" | "kotlin" | "go") {
        let length = if language == "rust" { RUST_CONTINUATION.find(rest).map_or(1, |found| found.end()) } else { 1 };
        return Escape { end: at + 1 + length, value: Decoded::Text("") };
    }
    let end = at + 1 + c.len_utf8();
    Escape { end, value: Decoded::Text(&text[at..end]) }
}

/// A string node, in the spellings of the parsers.
fn is_string_node(name: &str) -> bool {
    matches!(pascal_name(name).as_str(), "String" | "StringLiteral" | "string")
}

/// The quote of the plain string literal `from..to` lies inside (between its quotes, splitting
/// none of its escape sequences); `None` when there is none.
fn plain_string_at(state: &EditState, tree: &SyntaxTree, from: u32, to: u32, language: &str) -> Option<char> {
    let inner = tree.resolve_inner(from, 1)?;
    let index = tree.ancestors(inner).find(|index| is_string_node(tree.name(*index)))?;
    let node = tree.node(index);
    let text = state.slice(node.from, node.to);
    let quote = text.chars().next()?;
    let allowed = quote == '"' || (quote == '\'' && (is_js(language) || language == "python"));
    if !allowed
        || utf16_len(&text) < 2
        || !text.ends_with(quote)
        || text.starts_with(&quote.to_string().repeat(3))
        || from < node.from + 1
        || to > node.to.saturating_sub(1)
    {
        return None;
    }
    // A parser may merge adjacent tokens: only one complete literal counts, and no selection
    // boundary may split an escape sequence.
    let last = text.len() - 1;
    let bytes = text.as_bytes();
    let mut at = 1;
    while at < last {
        if bytes[at] == quote as u8 {
            return None;
        }
        if bytes[at] != b'\\' {
            at += 1;
            continue;
        }
        let end = read_escape(&text, at, language).end;
        if end > last {
            return None;
        }
        let (start, stop) = (node.from + col_of_byte(&text, at), node.from + col_of_byte(&text, end));
        if [from, to].iter().any(|pos| *pos > start && *pos < stop) {
            return None;
        }
        at = end;
    }
    Some(quote)
}

/// `text` escaped for a literal delimited by `quote`: backslashes, the quote (and `$` in
/// Kotlin), line breaks, tabs and control characters (and the line and paragraph separators in
/// JavaScript).
fn escape_text(text: &str, quote: char, language: &str) -> String {
    let js = is_js(language);
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        let code = u32::from(c);
        match c {
            '\\' => out.push_str("\\\\"),
            _ if c == quote || (c == '$' && language == "kotlin") => {
                out.push('\\');
                out.push(c);
            }
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ if code < 32 || (js && matches!(c, '\u{2028}' | '\u{2029}')) => out.push_str(&match language {
                "rust" => format!("\\u{{{code:x}}}"),
                "c" | "cpp" => format!("\\{code:03o}"),
                _ => format!("\\u{code:04x}"),
            }),
            _ => out.push(c),
        }
    }
    out
}

/// Writes out a high surrogate still waiting for its low half as the escape it came from.
fn flush_high(out: &mut String, high: &mut Option<(u32, &str)>) {
    if let Some((_, source)) = high.take() {
        out.push_str(source);
    }
}

/// `text` with the escape sequences of `language` decoded. Two escapes that are the halves of a
/// surrogate pair decode to their character; an unpaired half stays as written.
fn unescape_text(text: &str, language: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut high: Option<(u32, &str)> = None;
    let mut at = 0;
    while let Some(c) = text[at..].chars().next() {
        if c != '\\' {
            flush_high(&mut out, &mut high);
            out.push(c);
            at += c.len_utf8();
            continue;
        }
        let escape = read_escape(text, at, language);
        let source = &text[at..escape.end];
        at = escape.end;
        match escape.value {
            Decoded::Text(value) => {
                flush_high(&mut out, &mut high);
                out.push_str(value);
            }
            Decoded::Code(code) => match char::from_u32(code) {
                Some(decoded) => {
                    flush_high(&mut out, &mut high);
                    out.push(decoded);
                }
                None if (0xdc00..=0xdfff).contains(&code) => match high.take() {
                    Some((first, first_source)) => {
                        match char::from_u32(0x10000 + ((first - 0xd800) << 10) + (code - 0xdc00)) {
                            Some(pair) => out.push(pair),
                            None => {
                                out.push_str(first_source);
                                out.push_str(source);
                            }
                        }
                    }
                    None => out.push_str(source),
                },
                None => {
                    flush_high(&mut out, &mut high);
                    high = Some((code, source));
                }
            },
        }
    }
    flush_high(&mut out, &mut high);
    out
}

/// The paste `paste` (applied to `state`) with each change escaped for the plain string literal
/// its selection range is in. The paste comes back unchanged when `enabled` is off (Settings ›
/// Smart Keys › escape on paste), the language has no such literals, a range is not inside one,
/// a change does not replace exactly one range (a line-wise paste) or nothing needs escaping.
pub fn escape_paste(state: &EditState, tree: &SyntaxTree, language: &str, enabled: bool, paste: &Edit) -> Edit {
    if !enabled || !LANGUAGES.contains(&language) || paste.changes.is_empty() {
        return paste.clone();
    }
    let quotes: Option<Vec<char>> = state
        .selections
        .iter()
        .map(|range| plain_string_at(state, tree, range.start(), range.end(), language))
        .collect();
    let Some(quotes) = quotes else {
        return paste.clone();
    };
    let mut changes = Vec::with_capacity(paste.changes.len());
    let mut corrections = Vec::new();
    let mut delta: i64 = 0;
    for pasted in &paste.changes {
        let replaced = |range: &(u32, u32)| range.start() == pasted.from && range.end() == pasted.to;
        let Some(index) = state.selections.iter().position(replaced) else {
            return paste.clone();
        };
        let inserted = utf16_len(&pasted.insert);
        let from_after = (i64::from(pasted.from) + delta).max(0) as u32;
        delta += i64::from(inserted) - i64::from(pasted.to - pasted.from);
        let escaped = escape_text(&pasted.insert, quotes[index], language);
        if escaped != pasted.insert {
            corrections.push(change(from_after, from_after + inserted, escaped.clone()));
        }
        changes.push(change(pasted.from, pasted.to, escaped));
    }
    if corrections.is_empty() {
        return paste.clone();
    }
    let ranges = paste.selections.iter().map(|range| map_selection(*range, &corrections, Assoc::After)).collect();
    Edit::new(changes, ranges, paste.main)
}

/// The text a copy puts on the clipboard (`text`, as copied), unescaped when every selection
/// range lies inside a plain string literal and `text` is exactly the selected text.
pub fn unescape_copied(state: &EditState, tree: &SyntaxTree, language: &str, enabled: bool, text: &str) -> String {
    if !enabled || !LANGUAGES.contains(&language) {
        return text.to_owned();
    }
    let ranges = &state.selections;
    let in_plain_string =
        |range: &(u32, u32)| plain_string_at(state, tree, range.start(), range.end(), language).is_some();
    if ranges.iter().any(|range| range.is_caret() || !in_plain_string(range)) {
        return text.to_owned();
    }
    let selected: Vec<String> = ranges.iter().map(|range| state.slice(range.start(), range.end())).collect();
    if text != selected.join(state.line_break.as_str()) {
        return text.to_owned();
    }
    let decoded: Vec<String> = selected.iter().map(|part| unescape_text(part, language)).collect();
    decoded.join(state.line_break.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::clipboard::{copy, cut, paste};
    use crate::editor_commands::testing::{state, state_guillemets};
    use crate::editor_commands::tree::TreeNode;
    use crate::editor_commands::{apply, caret};

    const MODES: [&str; 9] = ["java", "kotlin", "javascript", "typescript", "python", "rust", "go", "c", "cpp"];

    /// The name the language's parser gives a string literal.
    fn string_node(language: &str) -> &'static str {
        match language {
            "java" => "StringLiteral",
            "kotlin" => "string",
            _ => "String",
        }
    }

    /// A tree with a node per `(name, from, to)` under the document node.
    fn tree_of(state: &EditState, nodes: &[(&str, u32, u32)]) -> SyntaxTree {
        let mut all = vec![TreeNode::new("Document", 0, state.doc_len())];
        all.extend(nodes.iter().map(|(name, from, to)| TreeNode::new(name, *from, *to)));
        SyntaxTree::new(all)
    }

    /// A tree with one `name` node over the first occurrence of `node_text`.
    fn node_over(state: &EditState, name: &str, node_text: &str) -> SyntaxTree {
        let text = state.text();
        let at = text.find(node_text).expect("the node's text is in the document");
        let from = utf16_len(&text[..at]);
        tree_of(state, &[(name, from, from + utf16_len(node_text))])
    }

    /// A tree with one `name` node from the first `quote` to the last.
    fn quoted(state: &EditState, name: &str, quote: char) -> SyntaxTree {
        let text = state.text();
        let (first, last) = (text.find(quote).expect("a quote"), text.rfind(quote).expect("a quote"));
        node_over(state, name, &text[first..=last])
    }

    /// Pastes `text` with the string escaping: the state after.
    fn paste_into(before: &EditState, tree: &SyntaxTree, language: &str, enabled: bool, text: &str) -> EditState {
        let raw = paste(before, text, false).expect("the paste applies");
        apply(before, &escape_paste(before, tree, language, enabled, &raw))
    }

    fn copied(state: &EditState, tree: &SyntaxTree, language: &str, enabled: bool) -> String {
        unescape_copied(state, tree, language, enabled, &copy(state).text)
    }

    /// The state with the inside of its string literal (after `value = "` or the like) selected.
    fn select_inside(state: &EditState, from: u32) -> EditState {
        state.clone().with_selections(vec![(from, state.doc_len() - 1)], 0)
    }

    #[test]
    fn escapes_ordinary_pastes_into_a_string() {
        for language in MODES {
            let before = state("value = \"|\"");
            let tree = quoted(&before, string_node(language), '"');
            let after = paste_into(&before, &tree, language, true, "say \"hi\"\\path\nnext");
            assert_eq!(after.text(), r#"value = "say \"hi\"\\path\nnext""#, "{language}");
        }
    }

    #[test]
    fn unescapes_copied_string_contents() {
        for language in MODES {
            let before = state_guillemets(r#"value = "«say \"hi\"\\path\nnext\t!»""#);
            let tree = quoted(&before, string_node(language), '"');
            assert_eq!(copied(&before, &tree, language, true), "say \"hi\"\\path\nnext\t!", "{language}");
        }
    }

    #[test]
    fn round_trips_pasted_control_characters_and_unicode() {
        let text = "😀\0\u{1}\u{8}\t\n\u{c}\u{b}\u{2028}\u{2029}$";
        for language in MODES {
            let before = state("value = \"|\"");
            let tree = quoted(&before, string_node(language), '"');
            let after = paste_into(&before, &tree, language, true, text);
            // The literal stays on one line (the rope also counts U+2028, U+2029, VT and FF as
            // line breaks, so the check is on the characters that break lines in the source).
            assert!(!after.text().contains(['\n', '\r']), "{language}: {}", after.text());
            let selected = select_inside(&after, 9);
            let tree = quoted(&selected, string_node(language), '"');
            assert_eq!(copied(&selected, &tree, language, true), text, "{language}");
        }
    }

    #[test]
    fn escapes_only_the_single_quoted_strings_delimiter() {
        for language in ["javascript", "typescript", "python"] {
            let before = state("value = '|'");
            let tree = quoted(&before, "String", '\'');
            let after = paste_into(&before, &tree, language, true, "say 'hi' and \"bye\"");
            assert_eq!(after.text(), r#"value = 'say \'hi\' and "bye"'"#, "{language}");
        }
    }

    #[test]
    fn disables_both_escaping_and_unescaping_with_the_setting() {
        let before = state_guillemets(r#"value = "«a\nb»""#);
        let tree = quoted(&before, "StringLiteral", '"');
        assert_eq!(copied(&before, &tree, "java", false), r"a\nb");
        let after = paste_into(&before, &tree, "java", false, "\"\\\n");
        assert_eq!(after.text(), "value = \"\"\\\n\"");
    }

    #[test]
    fn escapes_kotlin_dollars_and_unescapes_them_on_copy() {
        let before = state("val s = \"|\"");
        let tree = quoted(&before, "string", '"');
        let after = paste_into(&before, &tree, "kotlin", true, "$name ${call()}");
        assert_eq!(after.text(), r#"val s = "\$name \${call()}""#);
        let selected = select_inside(&after, 9);
        let tree = quoted(&selected, "string", '"');
        assert_eq!(copied(&selected, &tree, "kotlin", true), "$name ${call()}");
    }

    #[test]
    fn leaves_unsupported_literals_and_code_unchanged() {
        // The node each language's parser gives the literal around the caret: text blocks, raw,
        // template and format strings and characters have their own names; prefixed strings and
        // unfinished ones keep their prefix or miss their closing quote; Kotlin strings stop at
        // an interpolation, and a Kotlin multi-line string is one node.
        let cases: [(&str, &str, &str, &str); 26] = [
            ("java", "value = \"\"\"\n|\n\"\"\"", "TextBlock", "\"\"\"\n\n\"\"\""),
            ("kotlin", "val s = \"\"\"\n\"|\"\n\"\"\"", "string", "\"\"\"\n\"\"\n\"\"\""),
            ("kotlin", "val s = \"\"\"|\"\"\"", "string", "\"\"\"\"\"\""),
            ("kotlin", "val s = \"before |${name} after\"", "string", "\"before "),
            ("kotlin", "val s = \"before ${name} |after\"", "string", " after\""),
            ("javascript", "value = `|`", "TemplateString", "``"),
            ("typescript", "value = `hello |${name}`", "TemplateString", "`hello ${name}`"),
            ("python", "value = r\"|\"", "String", "r\"\""),
            ("python", "value = f\"|{name}\"", "FormatString", "f\"{name}\""),
            ("python", "value = b\"|\"", "String", "b\"\""),
            ("python", "value = '''|'''", "String", "''''''"),
            ("python", "value = \"\"\"\n\"|\"\n\"\"\"", "String", "\"\"\"\n\"\"\n\"\"\""),
            ("rust", "let s = r#\"|\"#;", "RawString", "r#\"\"#"),
            ("rust", "let s = r\"|\";", "RawString", "r\"\""),
            ("rust", "let s = b\"|\";", "String", "b\"\""),
            ("go", "s := `\"|\"`", "String", "`\"\"`"),
            ("cpp", "s = R\"(\"|\n\")\";", "RawString", "R\"(\"\n\")\""),
            ("java", "value = '|'", "CharacterLiteral", "''"),
            ("go", "s := '|'", "Rune", "''"),
            ("javascript", "// \"|\"", "LineComment", "// \"\""),
            ("kotlin", "/* \"|\" */", "comment", "/* \"\" */"),
            ("javascript", "value = \"unfinished|", "String", "\"unfinished"),
            ("javascript", "value = |\"text\"", "String", "\"text\""),
            ("javascript", "value = \"text\"|", "String", "\"text\""),
            ("javascript", "value = «\"text»\"", "String", "\"text\""),
            ("plaintext", "value = \"|\"", "String", "\"\""),
        ];
        let text = "\"\\\n";
        for (language, marked, name, node_text) in cases {
            let before = state_guillemets(marked);
            let tree = node_over(&before, name, node_text);
            let main = before.main();
            let expected =
                format!("{}{text}{}", before.slice(0, main.start()), before.slice(main.end(), before.doc_len()));
            assert_eq!(paste_into(&before, &tree, language, true, text).text(), expected, "{language}: {marked}");
        }
    }

    #[test]
    fn preserves_copied_source_outside_one_plain_string() {
        let cases: [(&str, &str, &str, &str); 8] = [
            ("javascript", r#"value = «"a\nb"»"#, "String", r#""a\nb""#),
            ("javascript", r#"value = "«a\nb" + "c»""#, "String", r#""a\nb""#),
            ("javascript", r"value = `«a\nb»`", "TemplateString", r"`a\nb`"),
            ("kotlin", "val s = \"\"\"\n\"«a\\nb»\"\n\"\"\"", "string", "\"\"\"\n\"a\\nb\"\n\"\"\""),
            ("python", r#"value = r"«a\nb»""#, "String", r#"r"a\nb""#),
            ("python", r#"value = f"«a\nb»{name}""#, "FormatString", r#"f"a\nb{name}""#),
            ("rust", r##"let s = r#"«a\nb»"#;"##, "RawString", r##"r#"a\nb"#"##),
            ("go", r"s := `«a\nb»`", "String", r"`a\nb`"),
        ];
        for (language, marked, name, node_text) in cases {
            let before = state_guillemets(marked);
            let tree = node_over(&before, name, node_text);
            let main = before.main();
            assert_eq!(copied(&before, &tree, language, true), before.slice(main.start(), main.end()), "{marked}");
        }
    }

    #[test]
    fn requires_every_caret_to_be_inside_a_plain_string() {
        let before = state("value = \"|\"; other = |");
        let tree = node_over(&before, "String", "\"\"");
        assert_eq!(paste_into(&before, &tree, "javascript", true, "\"").text(), "value = \"\"\"; other = \"");
    }

    #[test]
    fn escapes_per_caret_inserts_with_each_literals_delimiter_and_maps_all_carets() {
        let before = state_guillemets("a = \"|\"; b = '«old»'; c = \"|\";");
        let tree = tree_of(&before, &[("String", 4, 6), ("String", 12, 17), ("String", 23, 25)]);
        let after = paste_into(&before, &tree, "javascript", true, "one\"\ntwo'\nthree\\");
        assert_eq!(after.text(), r#"a = "one\""; b = 'two\''; c = "three\\";"#);
        for range in &after.selections {
            assert!(range.is_caret());
            assert!(matches!(after.slice(range.head(), range.head() + 1).as_str(), "\"" | "'"));
        }
    }

    #[test]
    fn copies_multiple_string_selections_as_decoded_fragments() {
        let before = state_guillemets(r#"a = "«one\n»"; b = '«two\t»'"#);
        let nodes = |state: &EditState| {
            let text = state.text();
            let first = utf16_len(&text[..text.find('"').expect("a quote")]);
            let second = utf16_len(&text[..text.find('\'').expect("a quote")]);
            tree_of(state, &[("String", first, first + 7), ("String", second, second + 7)])
        };
        assert_eq!(copied(&before, &nodes(&before), "javascript", true), "one\n\ntwo\t");
        let mixed = before.clone().with_selections(vec![(5, 10), (18, before.doc_len())], 0);
        let expected = format!("{}\n{}", mixed.slice(5, 10), mixed.slice(18, mixed.doc_len()));
        assert_eq!(copied(&mixed, &nodes(&mixed), "javascript", true), expected);
    }

    #[test]
    fn replaces_a_substring_and_preserves_the_caret_undo_and_redo() {
        let before = state_guillemets("value = \"before «old» after\"");
        let tree = quoted(&before, "String", '"');
        let raw = paste(&before, "😀\"\nnext", false).expect("the paste applies");
        let edit = escape_paste(&before, &tree, "javascript", true, &raw);
        let after = apply(&before, &edit);
        let expected = r#"value = "before 😀\"\nnext after""#;
        assert_eq!(after.text(), expected);
        let after_text = utf16_len(&expected[..expected.find(" after").expect("the rest of the literal")]);
        assert_eq!(after.main().head(), after_text);
        // One edit replacing the selection: undoing it gives the text back.
        assert_eq!(edit.changes, [change(16, 19, r#"😀\"\nnext"#)]);
    }

    #[test]
    fn preserves_edits_that_split_an_existing_escape() {
        for marked in [r#"value = "a\|nb""#, r#"value = "a«\»nb""#, r#"value = "a\u12|34b""#] {
            let before = state_guillemets(marked);
            let tree = quoted(&before, "String", '"');
            let main = before.main();
            let expected = format!("{}\"{}", before.slice(0, main.start()), before.slice(main.end(), before.doc_len()));
            assert_eq!(paste_into(&before, &tree, "javascript", true, "\"").text(), expected, "{marked}");
        }
    }

    #[test]
    fn decodes_supported_escapes_and_preserves_invalid_escapes() {
        let cases = [
            ("java", r"\u4f60\u597d\141", "你好a"),
            ("kotlin", r"\u4f60\u597d", "你好"),
            ("javascript", r"\u{1f600}\x21", "😀!"),
            ("python", r"\U0001f600\x21\141", "😀!a"),
            ("rust", r"\u{1f600}\x21", "😀!"),
            ("go", r"\U0001f600\x21\141", "😀!a"),
            ("javascript", r"\u{bad_name}\xZZ", r"\u{bad_name}\xZZ"),
            ("rust", r"\xFF\u{d800}", r"\xFF\u{d800}"),
        ];
        for (language, text, expected) in cases {
            let before = state_guillemets(&format!("value = \"«{text}»\""));
            let tree = quoted(&before, string_node(language), '"');
            assert_eq!(copied(&before, &tree, language, true), expected, "{language}: {text}");
        }
    }

    #[test]
    fn handles_native_paste_copy_cut_and_rich_copy_consistently() {
        let before = state("value = \"|\"");
        let tree = quoted(&before, "String", '"');
        let after = paste_into(&before, &tree, "javascript", true, "say \"hi\"\nnext");
        assert_eq!(after.text(), r#"value = "say \"hi\"\nnext""#);
        let selected = select_inside(&after, 9);
        let tree = quoted(&selected, "String", '"');
        assert_eq!(copied(&selected, &tree, "javascript", true), "say \"hi\"\nnext");
        let (cut_text, edit) = cut(&selected);
        assert_eq!(unescape_copied(&selected, &tree, "javascript", true, &cut_text.text), "say \"hi\"\nnext");
        assert_eq!(apply(&selected, &edit).text(), "value = \"\"");
        assert_eq!(apply(&selected, &edit).main(), caret(9));
    }
}
