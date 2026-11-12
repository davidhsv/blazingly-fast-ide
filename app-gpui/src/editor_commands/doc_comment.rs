//! Enter right after a `/**` that opens a new comment closes it, with the caret on the summary
//! line. In PHP the stub of a function lists its parameters (`@param`) and its return type
//! (`@return`).

use super::{Edit, EditState, LanguageFacts, SelectionExt, caret, insertion, leading_blank, utf16_len};
use crate::editor::buffer::byte_of_col;
use regex::Regex;
use std::sync::LazyLock;

/// How far below the `/**` the declaration it documents is read, in UTF-16 code units.
const DECLARATION_WINDOW: u32 = 4000;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// A function declaration up to the parenthesis that opens its parameters, after its attributes
/// and modifiers. The group is the function's name.
static FUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^\s*(?:#\[[^\]]*\]\s*)*(?:(?:abstract|final|public|protected|private|static)\s+)*",
        r"function\s+&?\s*([0-9A-Za-z_]+)\s*\(",
    ))
});

/// One parameter after its attributes and promotion modifiers: its type (with the `&` of a
/// parameter passed by reference), the `...` of a variadic one, its `$name`.
static PARAMETER: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^(?:#\[[^\]]*\]\s*)*(?:(?:public|protected|private|readonly)\s+)*",
        r"([^$=.]*?)\s*(\.\.\.)?\s*(\$[0-9A-Za-z_]+)",
    ))
});

/// The return type after the parameter list, up to the body or the end of the declaration.
static RETURN_TYPE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*:\s*([?0-9A-Za-z_\\|&() ]+?)\s*(?:\{|;|$)"));

/// The index after the bracket that closes the one opened just before `open`; `None` when it
/// stays open.
fn closing(text: &str, open: usize) -> Option<usize> {
    let mut depth = 1usize;
    for (index, byte) in text.bytes().enumerate().skip(open) {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Some(index + 1);
        }
    }
    None
}

/// The parameters of a parameter list: split at the commas outside brackets (default values may
/// hold commas), blank pieces dropped.
fn split_top_level(params: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (index, byte) in params.bytes().enumerate() {
        match byte {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                out.push(&params[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    out.push(&params[start..]);
    out.retain(|param| !param.trim().is_empty());
    out
}

/// The `@param` line of one parameter; `None` when it names no variable.
fn param_tag(param: &str) -> Option<String> {
    let found = PARAMETER.captures(param.trim())?;
    let group = |index: usize| found.get(index).map_or("", |group| group.as_str());
    // `&$x` passes by reference: the `&` belongs to no type (`Foo&Bar` is an intersection).
    let declared = group(1).trim_end();
    let declared = declared.strip_suffix('&').unwrap_or(declared);
    let type_name = declared.split_whitespace().collect::<Vec<_>>().join(" ");
    let name = format!("{}{}", group(2), group(3));
    Some(if type_name.is_empty() { format!("@param {name}") } else { format!("@param {type_name} {name}") })
}

/// The `@param` and `@return` lines for the PHP function declared at the start of `text` (the
/// code below the `/**`); none when `text` starts with anything else.
pub fn php_doc_tags(text: &str) -> Vec<String> {
    let Some(function) = FUNCTION.captures(text) else {
        return Vec::new();
    };
    let (declaration, [name]) = function.extract();
    let open = declaration.len();
    let Some(end) = closing(text, open) else {
        return Vec::new();
    };
    let mut tags: Vec<String> = split_top_level(&text[open..end - 1]).into_iter().filter_map(param_tag).collect();
    let returned =
        RETURN_TYPE.captures(&text[end..]).and_then(|found| found.get(1)).map_or("", |found| found.as_str().trim());
    if !returned.is_empty() && returned != "void" && name != "__construct" {
        tags.push(format!("@return {returned}"));
    }
    tags
}

/// Enter with the caret right after a `/**` that is alone on its line and opens a new comment:
/// inserts the summary line, the tags of the function below (in PHP) and the closing `*/`, all
/// at the indentation of the `/**`, and puts the caret on the summary line.
///
/// `language` holds the facts in force at the caret: the stub is for `/* */` comments only.
/// Does not apply with a selection or several carets, after other text, or when the next line
/// already continues the comment (Enter then only breaks the line).
pub fn enter_in_doc_comment(state: &EditState, language: &LanguageFacts) -> Option<Edit> {
    let [range] = state.selections.as_slice() else {
        return None;
    };
    if !range.is_caret() {
        return None;
    }
    let head = range.head();
    let (open, close) = language.comments.block.as_ref()?;
    if open != "/*" || close != "*/" {
        return None;
    }
    let line = state.line_at(head);
    let (before, after) = line.text.split_at(byte_of_col(&line.text, head.saturating_sub(line.from)));
    if before.trim() != "/**" || !after.trim().is_empty() {
        return None;
    }
    let next = state.next_line_start(&line).map(|start| state.line_at(start));
    if next.as_ref().is_some_and(|next| next.text.trim_start().starts_with('*')) {
        return None;
    }
    let tags = match next {
        Some(next) if language.id == "php" => {
            php_doc_tags(&state.slice(next.from, next.from.saturating_add(DECLARATION_WINDOW)))
        }
        _ => Vec::new(),
    };
    let indent = leading_blank(before);
    let line_start = format!("{}{indent} *", state.line_break);
    let summary = format!("{line_start} ");
    let mut insert = summary.clone();
    for tag in &tags {
        insert.push_str(&summary);
        insert.push_str(tag);
    }
    insert.push_str(&line_start);
    insert.push('/');
    Some(Edit::new(vec![insertion(head, insert)], vec![caret(head + utf16_len(&summary))], 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor_commands::apply;
    use crate::editor_commands::testing::{show, state_carets};

    fn slash_star(id: &str) -> LanguageFacts {
        LanguageFacts::new(id).with_comments(Some("//"), Some(("/*", "*/")))
    }

    /// The text after Enter at the `|` of `marked`, with the caret marked; `None` when the
    /// command does not apply.
    fn enter(marked: &str, language: &LanguageFacts) -> Option<String> {
        let before = state_carets(marked);
        enter_in_doc_comment(&before, language).map(|edit| show(&apply(&before, &edit)))
    }

    #[test]
    fn lists_a_php_functions_parameters_and_return_type_caret_on_the_summary_line() {
        let before = state_carets("/**|\nfunction f(int $a, $b): string {}\n/**\n * old\n */\nfunction g() {}");
        let edit = enter_in_doc_comment(&before, &slash_star("php")).expect("the stub is inserted");
        assert_eq!(
            show(&apply(&before, &edit)),
            "/**\n * |\n * @param int $a\n * @param $b\n * @return string\n */\n\
             function f(int $a, $b): string {}\n/**\n * old\n */\nfunction g() {}"
        );
        // One insertion at the caret: undoing it gives the text back.
        assert_eq!(edit.changes, [insertion(3, "\n * \n * @param int $a\n * @param $b\n * @return string\n */")]);
    }

    #[test]
    fn uses_the_indentation_of_the_doc_comment_line_inside_a_class() {
        let marked =
            "class A {\n    /**|\n    public static function make(?array $o = [], string ...$rest): ?static {}\n}";
        assert_eq!(
            enter(marked, &slash_star("php")).as_deref(),
            Some(
                "class A {\n    /**\n     * |\n     * @param ?array $o\n     * @param string ...$rest\n     \
                 * @return ?static\n     */\n    \
                 public static function make(?array $o = [], string ...$rest): ?static {}\n}"
            )
        );
    }

    #[test]
    fn gives_constructors_and_void_methods_no_return_promoted_parameters_keep_their_type() {
        assert_eq!(
            php_doc_tags("public function __construct(private int $a, #[Attr] readonly Foo&Bar $b) {}"),
            ["@param int $a", "@param Foo&Bar $b"]
        );
        assert_eq!(
            php_doc_tags("function run(array &$x = array(1, 2), &$y): void {}"),
            ["@param array $x", "@param $y"]
        );
        assert_eq!(
            php_doc_tags("abstract protected function find(int|string $id): ?Model;"),
            ["@param int|string $id", "@return ?Model"]
        );
    }

    #[test]
    fn closes_the_comment_with_an_empty_summary_above_anything_else_and_in_other_slash_star_languages() {
        assert_eq!(enter("/**|\nclass A {}", &slash_star("php")).as_deref(), Some("/**\n * |\n */\nclass A {}"));
        assert_eq!(
            enter("  /**|\n\nconst x = 1;", &slash_star("javascript")).as_deref(),
            Some("  /**\n   * |\n   */\n\nconst x = 1;")
        );
    }

    #[test]
    fn only_breaks_the_line_in_an_existing_doc_block_after_other_text_or_without_slash_star_comments() {
        let php = slash_star("php");
        assert_eq!(enter("/**|\n * old\n */\nfunction f() {}", &php), None);
        assert_eq!(enter("$a = 1; /**|", &php), None);
        assert_eq!(enter("/** x|", &php), None);
        assert_eq!(enter("/**|", &LanguageFacts::new("python").with_comments(Some("#"), None)), None);
    }

    #[test]
    fn keeps_the_line_break_of_the_document() {
        assert_eq!(
            enter("/**|\r\nfunction f(): int {}\r\n", &slash_star("php")).as_deref(),
            Some("/**\r\n * |\r\n * @return int\r\n */\r\nfunction f(): int {}\r\n")
        );
    }
}
