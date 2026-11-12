//! Enter with indentation, and the indentation a language without a parser gets: the previous
//! line's indentation, one unit deeper after a block opener (`:`, `{`, `(`, `[` or a keyword),
//! one unit shallower after `pass` / `return` in the languages whose blocks are indentation.

use super::{
    Edit, EditState, IndentContext, Indenter, RangeEdit, SelectionExt, caret, change, change_by_range, leading_space,
    utf16_len,
};
use regex::Regex;
use std::sync::LazyLock;

/// Languages whose blocks open with a trailing colon.
const COLON_INDENT_LANGS: [&str; 2] = ["gdscript", "python"];
/// Languages whose blocks open with a keyword and close with `end`.
const KEYWORD_OPEN_LANGS: [&str; 3] = ["elixir", "ruby", "lua"];
/// Languages that are prose or data: no indentation is computed for them.
pub const NO_INDENT_LANGS: [&str; 7] = ["plaintext", "markdown", "asciidoc", "log", "diff", "ignore", "gitattributes"];

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// A statement after which the block it ends is left.
static DEDENT_STATEMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*(?:pass|break|continue|return)(?-u:\b)"));
/// A line that opens a keyword block (Ruby, Lua, Elixir).
static KEYWORD_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"(?-u:\b)(?:do(?:\s*\|[^|]*\|)?|then|->)$|",
        r"^\s*(?:def|defp|defmodule|class|module|function|local\s+function|if|unless|elsif|elseif|else|while|until|",
        r"for|case|begin|cond|with|receive|try)(?-u:\b)",
    ))
});
static ENDS_WITH_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u:\b)end$"));
static ELIXIR_OPEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?-u:\b)do$|->$"));

fn opens_with_bracket(code: &str) -> bool {
    code.ends_with(['{', '(', '['])
}

fn opens_block(code: &str, language: &str) -> bool {
    if code.is_empty() {
        return false;
    }
    if opens_with_bracket(code) {
        return true;
    }
    if COLON_INDENT_LANGS.contains(&language) && code.ends_with(':') {
        return true;
    }
    if language == "elixir" {
        return ELIXIR_OPEN.is_match(code);
    }
    KEYWORD_OPEN_LANGS.contains(&language) && !ENDS_WITH_END.is_match(code) && KEYWORD_OPEN.is_match(code)
}

fn dedents_after(code: &str, language: &str) -> bool {
    COLON_INDENT_LANGS.contains(&language) && DEDENT_STATEMENT.is_match(code) && !code.ends_with([',', '\\'])
}

/// The code of a line for reading block syntax: a string literal becomes one `x` (its quotes and
/// brackets do not count), a trailing line comment is dropped, and so are trailing blanks.
fn indentation_code(text: &str, line_comment: Option<&str>) -> String {
    let mut code = String::with_capacity(text.len());
    let mut quote: Option<char> = None;
    let mut chars = text.char_indices();
    while let Some((at, c)) = chars.next() {
        if let Some(open) = quote {
            if c == '\\' {
                chars.next();
            } else if c == open {
                quote = None;
            }
        } else if matches!(c, '"' | '\'' | '`') {
            quote = Some(c);
            code.push('x');
        } else if line_comment.is_some_and(|token| !token.is_empty() && text[at..].starts_with(token)) {
            break;
        } else {
            code.push(c);
        }
    }
    code.truncate(code.trim_end().len());
    code
}

/// The indentation column from the code of the line before (`code`, at column `base`), for the
/// line that follows it; `closing` when that line starts with a closing bracket.
fn indent_after(code: &str, base: u32, closing: bool, language: &str, unit: u32) -> u32 {
    if closing {
        return if opens_with_bracket(code) { base } else { base.saturating_sub(unit) };
    }
    if opens_block(code, language) {
        return base + unit;
    }
    if dedents_after(code, language) {
        return base.saturating_sub(unit);
    }
    base
}

/// The indentation of a language without a parser (GDScript, Zig, Elixir, HCL…) for the line at
/// `cx.pos`: after a simulated line break, from the code before the break; otherwise from the
/// closest non-blank line above. `language` is the backend's language id, `line_comment` the
/// line comment token in force there. `None` for the languages that are not indented.
pub fn fallback_indent(cx: &IndentContext<'_>, language: &str, line_comment: Option<&str>) -> Option<u32> {
    if NO_INDENT_LANGS.contains(&language) {
        return None;
    }
    let state = cx.state;
    let pos = cx.pos;
    let closing = cx.text_after_pos(pos).trim_start().starts_with(['}', ']', ')']);
    if cx.simulated_break.is_some() {
        let line = state.line_at(pos);
        let before = &line.text[..crate::editor::buffer::byte_of_col(&line.text, pos - line.from)];
        let base = state.count_column(leading_space(before));
        if before.trim().is_empty() {
            return Some(base);
        }
        let code = indentation_code(before, line_comment);
        return Some(indent_after(&code, base, closing, language, cx.unit()));
    }
    let current = state.line_at(pos);
    for number in (0..current.number).rev() {
        let previous = state.line(number);
        if previous.text.trim().is_empty() {
            continue;
        }
        let base = state.count_column(leading_space(&previous.text));
        let code = indentation_code(&previous.text, line_comment);
        return Some(indent_after(&code, base, closing, language, cx.unit()));
    }
    Some(0)
}

/// The caret sits between an opening bracket and its closer (`(|)`, `[|]`, `{|}`): Enter there
/// puts the closer on a line of its own.
pub fn is_between_brackets(state: &EditState, pos: u32) -> Option<(u32, u32)> {
    let around = state.slice(pos.saturating_sub(1), pos + 1);
    matches!(around.as_str(), "()" | "[]" | "{}").then_some((pos, pos))
}

/// UTF-16 length of the whitespace `text` starts with.
fn leading_whitespace_units(text: &str) -> u32 {
    utf16_len(leading_space(text))
}

/// Enter: replaces each selection with a line break and the indentation the language computes
/// for the new line (the current line's indentation without one). Whitespace after the caret
/// goes, and so does the whitespace of a line holding nothing else; between a pair of brackets
/// the closer moves to a line of its own at the opener line's indentation.
pub fn insert_newline_and_indent(state: &EditState, indenter: Option<Indenter<'_>>) -> Option<Edit> {
    let br = state.line_break.as_str();
    Some(change_by_range(state, |_, range| {
        let (mut from, mut to) = (range.start(), range.end());
        let line = state.line_at(from);
        let explode = if range.is_caret() { is_between_brackets(state, from) } else { None };
        let cx = IndentContext {
            simulated_break: Some(from),
            double_break: explode.is_some(),
            ..IndentContext::new(state, from)
        };
        let indent = indenter
            .and_then(|indenter| indenter(&cx))
            .unwrap_or_else(|| state.count_column(leading_space(&line.text)));
        if to < line.to {
            to += leading_whitespace_units(&state.slice(to, line.to));
        }
        if let Some((open, close)) = explode {
            (from, to) = (open, close);
        } else if from > line.from && from < line.from + 100 && state.slice(line.from, from).trim().is_empty() {
            from = line.from;
        }
        let indent_text = state.indent_string(indent);
        let mut insert = format!("{br}{indent_text}");
        if explode.is_some() {
            insert.push_str(br);
            insert.push_str(&state.indent_string(cx.line_indent(line.from, -1)));
        }
        let range = caret(from + utf16_len(br) + utf16_len(&indent_text));
        RangeEdit { changes: vec![change(from, to, insert)], range }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::testing::{shown, state, styled};

    fn gdscript(cx: &IndentContext<'_>) -> Option<u32> {
        fallback_indent(cx, "gdscript", Some("#"))
    }

    fn enter(state: &EditState, indenter: Indenter<'_>) -> String {
        shown(state, insert_newline_and_indent(state, Some(indenter)))
    }

    #[test]
    fn indents_with_a_tab_after_a_function_header_ending_in_a_colon() {
        assert_eq!(enter(&styled("func test() -> void:|", 4, "\t"), &gdscript), "func test() -> void:\n\t|");
    }

    #[test]
    fn indents_nested_blocks_and_ignores_trailing_comments() {
        let before = styled("func test() -> void:\n\tif x > 0: # check|", 4, "\t");
        assert_eq!(enter(&before, &gdscript), "func test() -> void:\n\tif x > 0: # check\n\t\t|");
    }

    #[test]
    fn does_not_indent_after_a_comment_ending_in_a_colon() {
        assert_eq!(enter(&styled("\t# note:|", 4, "\t"), &gdscript), "\t# note:\n\t|");
    }

    #[test]
    fn dedents_after_pass_return_break_and_continue() {
        let pass = styled("func test() -> void:\n\tpass|", 4, "\t");
        assert_eq!(enter(&pass, &gdscript), "func test() -> void:\n\tpass\n|");
        let nested = styled("func test() -> int:\n\tif ready:\n\t\treturn 1|", 4, "\t");
        assert_eq!(enter(&nested, &gdscript), "func test() -> int:\n\tif ready:\n\t\treturn 1\n\t|");
    }

    #[test]
    fn handles_language_input() {
        let cases = [
            ("gdscript", "var items = {|}", "var items = {\n  |\n}"),
            ("gdscript", "var text = \"unfinished:|", "var text = \"unfinished:\n|"),
            ("gdscript", "var text = \"unfinished {|", "var text = \"unfinished {\n|"),
            ("gdscript", "var text = \"#\" # comment:|", "var text = \"#\" # comment:\n|"),
            ("gdscript", "if ready: # note|", "if ready: # note\n  |"),
            ("elixir", "if ready, do: value|", "if ready, do: value\n|"),
            ("lua", "if ready then work() end|", "if ready then work() end\n|"),
        ];
        for (language, input, expected) in cases {
            let indenter = |cx: &IndentContext<'_>| fallback_indent(cx, language, Some("#"));
            assert_eq!(enter(&state(input), &indenter), expected, "{language}: {input}");
        }
    }

    #[test]
    fn keeps_the_line_indentation_without_a_language_and_the_document_line_break() {
        let before = state("  a|\r\nb");
        assert_eq!(shown(&before, insert_newline_and_indent(&before, None)), "  a\r\n  |\r\nb");
    }
}
