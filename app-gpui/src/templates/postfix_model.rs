//! Postfix completion (`expr.if`, `.not`, `.var` …): finding the expression before the dot and the
//! per-language template set. Template bodies use the live-template syntax with `$EXPR$` bound to
//! the expression text.

use super::editor_context::{TreeNode, pascal_kind};
use super::live_builtins::TS_LANGS;
use super::live_model::TemplateVariable;
use crate::editor::buffer;
use regex::Regex;
use ropey::Rope;
use std::collections::BTreeSet;
use std::sync::LazyLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixTemplate {
    /// Stable id: `<language group>.<key>`.
    pub id: String,
    pub key: String,
    pub description: String,
    pub langs: Vec<String>,
    pub body: String,
    pub variables: Vec<TemplateVariable>,
    /// Only offered when the expression starts its line (statement position).
    pub statement: bool,
    /// Wrap compound expressions (`a + b`) in parentheses for `$EXPR$`.
    pub wrap_compound: bool,
}

/// A template before its set gives it an id and languages.
struct Def {
    key: &'static str,
    description: &'static str,
    body: &'static str,
    variables: Vec<TemplateVariable>,
    statement: bool,
    wrap_compound: bool,
}

/// A template offered for any expression.
fn expr(key: &'static str, description: &'static str, body: &'static str) -> Def {
    Def { key, description, body, variables: Vec::new(), statement: false, wrap_compound: false }
}

/// A template offered only for an expression that starts its line.
fn stmt(key: &'static str, description: &'static str, body: &'static str) -> Def {
    Def { statement: true, ..expr(key, description, body) }
}

impl Def {
    fn wrap(mut self) -> Self {
        self.wrap_compound = true;
        self
    }

    fn var(mut self, variable: TemplateVariable) -> Self {
        self.variables.push(variable);
        self
    }
}

/// The name of the variable the expression is assigned to.
fn name() -> TemplateVariable {
    TemplateVariable::new("NAME").expression("suggestVariableName()").default_value("\"value\"")
}

fn item() -> TemplateVariable {
    TemplateVariable::new("ITEM").default_value("\"item\"")
}

fn index() -> TemplateVariable {
    TemplateVariable::new("I").default_value("\"i\"")
}

fn set(key: &str, langs: &[&str], defs: Vec<Def>) -> Vec<PostfixTemplate> {
    defs.into_iter()
        .map(|d| PostfixTemplate {
            id: format!("{key}.{}", d.key),
            key: d.key.to_owned(),
            description: d.description.to_owned(),
            langs: langs.iter().map(|&lang| lang.to_owned()).collect(),
            body: d.body.to_owned(),
            variables: d.variables,
            statement: d.statement,
            wrap_compound: d.wrap_compound,
        })
        .collect()
}

fn ts_like() -> Vec<PostfixTemplate> {
    set(
        "ts",
        &TS_LANGS,
        vec![
            stmt("if", "if (expr)", "if ($EXPR$) {\n\t$END$\n}"),
            stmt("else", "if (!expr)", "if (!$EXPR$) {\n\t$END$\n}").wrap(),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("const", "const name = expr", "const $NAME$ = $EXPR$;$END$").var(name()),
            stmt("var", "const name = expr", "const $NAME$ = $EXPR$;$END$").var(name()),
            stmt("let", "let name = expr", "let $NAME$ = $EXPR$;$END$").var(name()),
            stmt("return", "return expr", "return $EXPR$;$END$"),
            stmt("log", "console.log(expr)", "console.log($EXPR$);$END$"),
            expr("await", "await expr", "await $EXPR$$END$").wrap(),
            expr("par", "(expr)", "($EXPR$)$END$"),
            stmt("for", "for (const item of expr)", "for (const $ITEM$ of $EXPR$) {\n\t$END$\n}").var(item()),
            stmt("forin", "for (const key in expr)", "for (const $KEY$ in $EXPR$) {\n\t$END$\n}")
                .var(TemplateVariable::new("KEY").default_value("\"key\"")),
            stmt("fori", "for (let i = 0; i < expr; i++)", "for (let $I$ = 0; $I$ < $EXPR$; $I$++) {\n\t$END$\n}")
                .var(index()),
            stmt("nn", "if (expr != null)", "if ($EXPR$ != null) {\n\t$END$\n}"),
            stmt("notnull", "if (expr != null)", "if ($EXPR$ != null) {\n\t$END$\n}"),
            stmt("null", "if (expr == null)", "if ($EXPR$ == null) {\n\t$END$\n}"),
            stmt("try", "try { expr } catch", "try {\n\t$EXPR$;$END$\n} catch (e) {\n\t\n}"),
            expr("cast", "(expr as Type)", "($EXPR$ as $TYPE$)$END$"),
            expr("typeof", "typeof expr", "typeof $EXPR$$END$").wrap(),
            stmt("throw", "throw expr", "throw $EXPR$;$END$"),
        ],
    )
}

fn rust() -> Vec<PostfixTemplate> {
    set(
        "rust",
        &["rust"],
        vec![
            stmt("if", "if expr", "if $EXPR$ {\n\t$END$\n}"),
            stmt("else", "if !expr", "if !$EXPR$ {\n\t$END$\n}").wrap(),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("let", "let name = expr;", "let $NAME$ = $EXPR$;$END$").var(name()),
            stmt("letm", "let mut name = expr;", "let mut $NAME$ = $EXPR$;$END$").var(name()),
            stmt("match", "match expr", "match $EXPR$ {\n\t$PAT$ => $END$,\n}"),
            stmt("while", "while expr", "while $EXPR$ {\n\t$END$\n}"),
            stmt("for", "for item in expr", "for $ITEM$ in $EXPR$ {\n\t$END$\n}").var(item()),
            stmt("return", "return expr;", "return $EXPR$;$END$"),
            stmt("println", "println!(\"{:?}\", expr)", "println!(\"{:?}\", $EXPR$);$END$"),
            expr("dbg", "dbg!(expr)", "dbg!($EXPR$)$END$"),
            expr("ok", "Ok(expr)", "Ok($EXPR$)$END$"),
            expr("err", "Err(expr)", "Err($EXPR$)$END$"),
            expr("some", "Some(expr)", "Some($EXPR$)$END$"),
            expr("box", "Box::new(expr)", "Box::new($EXPR$)$END$"),
            expr("ref", "&expr", "&$EXPR$$END$").wrap(),
            expr("refm", "&mut expr", "&mut $EXPR$$END$").wrap(),
            expr("par", "(expr)", "($EXPR$)$END$"),
            stmt("iflet", "if let Some(x) = expr", "if let Some($ITEM$) = $EXPR$ {\n\t$END$\n}").var(item()),
        ],
    )
}

fn python() -> Vec<PostfixTemplate> {
    set(
        "py",
        &["python"],
        vec![
            stmt("if", "if expr:", "if $EXPR$:\n\t$END$"),
            stmt("ifn", "if expr is None:", "if $EXPR$ is None:\n\t$END$"),
            stmt("ifnn", "if expr is not None:", "if $EXPR$ is not None:\n\t$END$"),
            stmt("nn", "if expr is not None:", "if $EXPR$ is not None:\n\t$END$"),
            stmt("null", "if expr is None:", "if $EXPR$ is None:\n\t$END$"),
            expr("not", "not expr", "not $EXPR$$END$").wrap(),
            stmt("var", "name = expr", "$NAME$ = $EXPR$$END$").var(name()),
            stmt("return", "return expr", "return $EXPR$$END$"),
            stmt("print", "print(expr)", "print($EXPR$)$END$"),
            stmt("for", "for item in expr:", "for $ITEM$ in $EXPR$:\n\t$END$").var(item()),
            stmt("fori", "for i in range(expr):", "for $I$ in range($EXPR$):\n\t$END$").var(index()),
            stmt("while", "while expr:", "while $EXPR$:\n\t$END$"),
            expr("len", "len(expr)", "len($EXPR$)$END$"),
            expr("par", "(expr)", "($EXPR$)$END$"),
            expr("await", "await expr", "await $EXPR$$END$").wrap(),
            stmt("try", "try: expr except", "try:\n\t$EXPR$$END$\nexcept Exception as e:\n\traise"),
        ],
    )
}

fn go() -> Vec<PostfixTemplate> {
    set(
        "go",
        &["go"],
        vec![
            stmt("if", "if expr {}", "if $EXPR$ {\n\t$END$\n}"),
            stmt("else", "if !expr {}", "if !$EXPR$ {\n\t$END$\n}").wrap(),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("var", "name := expr", "$NAME$ := $EXPR$$END$").var(name()),
            stmt("return", "return expr", "return $EXPR$$END$"),
            stmt("print", "fmt.Println(expr)", "fmt.Println($EXPR$)$END$"),
            stmt("for", "for _, v := range expr", "for _, $ITEM$ := range $EXPR$ {\n\t$END$\n}")
                .var(TemplateVariable::new("ITEM").default_value("\"v\"")),
            stmt("fori", "for i := 0; i < expr; i++", "for $I$ := 0; $I$ < $EXPR$; $I$++ {\n\t$END$\n}").var(index()),
            stmt("nn", "if expr != nil", "if $EXPR$ != nil {\n\t$END$\n}"),
            stmt("null", "if expr == nil", "if $EXPR$ == nil {\n\t$END$\n}"),
            expr("par", "(expr)", "($EXPR$)$END$"),
            expr("len", "len(expr)", "len($EXPR$)$END$"),
        ],
    )
}

fn java() -> Vec<PostfixTemplate> {
    set(
        "java",
        &["java"],
        vec![
            stmt("if", "if (expr)", "if ($EXPR$) {\n\t$END$\n}"),
            stmt("else", "if (!expr)", "if (!$EXPR$) {\n\t$END$\n}").wrap(),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("var", "var name = expr;", "var $NAME$ = $EXPR$;$END$").var(name()),
            stmt("return", "return expr;", "return $EXPR$;$END$"),
            stmt("sout", "System.out.println(expr)", "System.out.println($EXPR$);$END$"),
            stmt("for", "for (var item : expr)", "for (var $ITEM$ : $EXPR$) {\n\t$END$\n}").var(item()),
            stmt("fori", "for (int i = 0; i < expr; i++)", "for (int $I$ = 0; $I$ < $EXPR$; $I$++) {\n\t$END$\n}")
                .var(index()),
            stmt("nn", "if (expr != null)", "if ($EXPR$ != null) {\n\t$END$\n}"),
            stmt("null", "if (expr == null)", "if ($EXPR$ == null) {\n\t$END$\n}"),
            stmt("while", "while (expr)", "while ($EXPR$) {\n\t$END$\n}"),
            expr("par", "(expr)", "($EXPR$)$END$"),
            expr("cast", "((Type) expr)", "(($TYPE$) $EXPR$)$END$").wrap(),
            stmt(
                "try",
                "try { expr } catch",
                "try {\n\t$EXPR$;$END$\n} catch (Exception e) {\n\tthrow new RuntimeException(e);\n}",
            ),
            stmt("throw", "throw expr;", "throw $EXPR$;$END$"),
        ],
    )
}

fn kotlin() -> Vec<PostfixTemplate> {
    set(
        "kt",
        &["kotlin"],
        vec![
            stmt("if", "if (expr)", "if ($EXPR$) {\n\t$END$\n}"),
            stmt("else", "if (!expr)", "if (!$EXPR$) {\n\t$END$\n}").wrap(),
            expr("not", "!expr", "!$EXPR$$END$").wrap(),
            stmt("val", "val name = expr", "val $NAME$ = $EXPR$$END$").var(name()),
            stmt("var", "var name = expr", "var $NAME$ = $EXPR$$END$").var(name()),
            stmt("return", "return expr", "return $EXPR$$END$"),
            stmt("sout", "println(expr)", "println($EXPR$)$END$"),
            stmt("for", "for (item in expr)", "for ($ITEM$ in $EXPR$) {\n\t$END$\n}").var(item()),
            stmt("fori", "for (i in 0 until expr)", "for ($I$ in 0 until $EXPR$) {\n\t$END$\n}").var(index()),
            stmt("nn", "if (expr != null)", "if ($EXPR$ != null) {\n\t$END$\n}"),
            stmt("null", "if (expr == null)", "if ($EXPR$ == null) {\n\t$END$\n}"),
            expr("let", "expr?.let { }", "$EXPR$?.let {\n\t$END$\n}").wrap(),
            stmt("while", "while (expr)", "while ($EXPR$) {\n\t$END$\n}"),
            expr("par", "(expr)", "($EXPR$)$END$"),
            expr("cast", "(expr as Type)", "($EXPR$ as $TYPE$)$END$"),
            stmt("try", "try { expr } catch", "try {\n\t$EXPR$$END$\n} catch (e: Exception) {\n\tthrow e\n}"),
            stmt("when", "when (expr)", "when ($EXPR$) {\n\t$END$\n}"),
        ],
    )
}

/// Every postfix template, grouped by language set.
pub fn postfix_templates() -> &'static [PostfixTemplate] {
    static TEMPLATES: LazyLock<Vec<PostfixTemplate>> =
        LazyLock::new(|| [ts_like(), rust(), python(), go(), java(), kotlin()].concat());
    &TEMPLATES
}

/// The language sets, as the settings page titles them: `(id prefix, title)`.
pub const POSTFIX_GROUPS: [(&str, &str); 6] = [
    ("ts", "JavaScript / TypeScript"),
    ("rust", "Rust"),
    ("py", "Python"),
    ("go", "Go"),
    ("java", "Java"),
    ("kt", "Kotlin"),
];

/// The templates of `language` the user has not disabled.
pub fn postfix_templates_for(language: &str, disabled: &BTreeSet<String>) -> Vec<&'static PostfixTemplate> {
    postfix_templates()
        .iter()
        .filter(|t| t.langs.iter().any(|lang| lang == language) && !disabled.contains(&t.id))
        .collect()
}

// ---------------------------------------------------------------------------
// Expression detection

/// An expression a postfix template may wrap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixCandidate {
    /// UTF-16 offset the expression starts at; it ends at the dot.
    pub from: u32,
    pub text: String,
    /// Only blanks come before it on its line.
    pub statement: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostfixTarget {
    /// Offset of the `.` before the key.
    pub dot: u32,
    /// End of the typed key (the caret).
    pub end: u32,
    pub key: String,
    /// Candidate expressions, innermost first; each ends at `dot`.
    pub candidates: Vec<PostfixCandidate>,
}

/// A letter, a digit, `_` or `$`.
fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

fn is_ascii_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_quote(c: char) -> bool {
    matches!(c, '"' | '\'' | '`')
}

fn opener(close: char) -> Option<char> {
    match close {
        ')' => Some('('),
        ']' => Some('['),
        '}' => Some('{'),
        _ => None,
    }
}

fn has_generics(language: &str) -> bool {
    matches!(language, "java" | "kotlin" | "rust" | "csharp" | "scala" | "swift") || TS_LANGS.contains(&language)
}

/// The operators a language writes after an expression (`foo?`, `foo!!`).
fn postfix_ops(language: &str) -> &'static str {
    match language {
        "rust" => "?",
        "kotlin" | "typescript" | "typescriptreact" => "!",
        "swift" => "!?",
        _ => "",
    }
}

/// Index of the bracket opening the one that closes at `close - 1`.
fn match_open(text: &[char], close: usize) -> Option<usize> {
    let mut stack: Vec<char> = Vec::new();
    let mut i = close;
    while i > 0 {
        i -= 1;
        let c = text[i];
        if is_quote(c) {
            i = skip_string_back(text, i + 1)?;
        } else if let Some(open) = opener(c) {
            stack.push(open);
        } else if matches!(c, '(' | '[' | '{') {
            if stack.pop() != Some(c) {
                return None;
            }
            if stack.is_empty() {
                return Some(i);
            }
        }
    }
    None
}

/// Start of the string literal ending (with its quote) at `end`.
fn skip_string_back(text: &[char], end: usize) -> Option<usize> {
    let is_prefix = |c: char| matches!(c, 'r' | 'b' | 'f' | 'u' | 'R' | 'B' | 'F' | 'U');
    let quote = *text.get(end.checked_sub(1)?)?;
    let mut i = end - 1;
    while i > 0 {
        i -= 1;
        if text[i] == '\n' && quote != '`' {
            return None;
        }
        if text[i] != quote {
            continue;
        }
        let backslashes = text[..i].iter().rev().take_while(|&&c| c == '\\').count();
        if backslashes % 2 == 0 {
            // Rust raw / byte strings and Python prefixes (r"", f"", b"") belong to the literal,
            // unless the letters are the end of an identifier.
            let mut k = i;
            while k > 0
                && (is_prefix(text[k - 1]) || text[k - 1] == '#')
                && !(k > 1 && is_ident(text[k - 2]) && !is_prefix(text[k - 2]))
            {
                k -= 1;
            }
            return Some(k);
        }
    }
    None
}

/// Start of a `<...>` generic argument list closing at `close - 1`.
fn match_generic(text: &[char], close: usize) -> Option<usize> {
    let mut depth = 0i32;
    let mut i = close;
    while i > 0 && close - (i - 1) < 200 {
        i -= 1;
        let c = text[i];
        if c == '>' {
            depth += 1;
        } else if c == '<' {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        } else if !(is_ascii_word(c) || c.is_whitespace() || ",.?:&[]$".contains(c)) {
            return None;
        }
    }
    None
}

fn scan_start(text: &[char], end: usize, language: &str) -> usize {
    let ops = postfix_ops(language);
    let at = |i: usize| i.checked_sub(1).and_then(|before| text.get(before)).copied();
    let mut i = end;
    loop {
        let start = i;
        // One primary, with its trailing calls, indexing and postfix operators.
        while let Some(c) = at(i) {
            if opener(c).is_some() {
                match match_open(text, i) {
                    Some(open) => {
                        i = open;
                        continue;
                    }
                    None => return if start == end { end } else { start },
                }
            }
            if is_quote(c) {
                if let Some(string) = skip_string_back(text, i) {
                    i = string;
                }
                break;
            }
            // `foo?` / `foo!!` / `foo!` and Rust macro calls `vec![…]`.
            if ops.contains(c) || (c == '!' && language == "rust" && at(i - 1).is_some_and(is_ident)) {
                i -= 1;
                continue;
            }
            if c == '>' && has_generics(language) && i != end {
                let generic = match_generic(text, i).filter(|&g| {
                    g > 0 && (is_ident(text[g - 1]) || (g >= 2 && text[g - 2] == ':' && text[g - 1] == ':'))
                });
                match generic {
                    Some(g) => {
                        i = g;
                        continue;
                    }
                    None => break,
                }
            }
            // Lifetimes and labels are not expressions; numbers are fine.
            while at(i).is_some_and(is_ident) {
                i -= 1;
            }
            break;
        }
        if i == start {
            break;
        }
        // A link to the previous part of the chain.
        let before = (at(i.saturating_sub(1)).filter(|_| i >= 2), at(i));
        i -= match before {
            (Some('?'), Some('.')) | (Some(':'), Some(':')) | (Some('-'), Some('>')) => 2,
            (Some('.'), Some('.')) => break,
            (_, Some('.')) => 1,
            _ => break,
        };
        if i == 0 {
            break;
        }
    }
    if i == end {
        return end;
    }
    // Skip a leading link that has nothing before it.
    while matches!(text.get(i), Some('.' | ':')) {
        i += 1;
    }
    // `new Foo()` (Java, TypeScript / JavaScript, C#).
    if !matches!(language, "kotlin" | "rust" | "go" | "python") {
        let window = &text[i.saturating_sub(12)..i];
        let blanks = window.iter().rev().take_while(|c| c.is_whitespace()).count();
        let word = &window[..window.len() - blanks];
        let bounded = word.len().checked_sub(4).is_none_or(|before| !is_ascii_word(word[before]));
        if blanks > 0 && word.ends_with(&['n', 'e', 'w']) && bounded {
            i -= blanks + 3;
        }
    }
    i
}

fn utf16_len(chars: &[char]) -> u32 {
    chars.iter().map(|c| c.len_utf16() as u32).sum()
}

/// Start of the call / member chain of `text` ending at `end` (exclusive; UTF-16 offsets):
/// identifiers, calls, indexing, literals and `.` / `::` / `?.` / `->` links, plus a leading `new`
/// for constructor calls. `end` itself when no expression ends there.
pub fn scan_expression_start(text: &str, end: u32, language: &str) -> u32 {
    let mut units = 0u32;
    let chars: Vec<char> = text
        .chars()
        .take_while(|c| {
            units += c.len_utf16() as u32;
            units <= end
        })
        .collect();
    utf16_len(&chars[..scan_start(&chars, chars.len(), language)])
}

/// Node kinds an expression does not reach beyond.
static STOP_NODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        "Statement|Block|Body|Program|Script|SourceFile|Module|Declaration|Definition|Item|Clause|List|Arg|Param",
        "|Template|Element|Document|Lambda|Arrow|Closure|Function|Macro|Attribute|Decorator|Annotation|Import",
        "|Export|Pattern|Spec|Case|Arm|Decl|⚠",
    ))
    .expect("the pattern is valid")
});

/// Whether `text` starts with a keyword that makes it more than an expression.
fn starts_with_keyword(text: &str) -> bool {
    const KEYWORDS: &[&str] = &[
        "return", "let", "const", "var", "val", "throw", "yield", "else", "case", "await", "async", "if", "for",
        "while", "in", "of",
    ];
    let word_ends = |rest: &str| !rest.starts_with(is_ascii_word);
    text == "new" || KEYWORDS.iter().any(|kw| text.strip_prefix(kw).is_some_and(word_ends))
}

/// `text` without its string literals (a literal ends on the line it starts on).
fn without_strings(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_quote(c) {
            let mut j = i + 1;
            let mut closed = None;
            while j < chars.len() && chars[j] != '\n' {
                if chars[j] == '\\' && chars.get(j + 1).is_some_and(|&next| next != '\n') {
                    j += 2;
                } else if chars[j] == c {
                    closed = Some(j);
                    break;
                } else {
                    j += 1;
                }
            }
            if let Some(close) = closed {
                i = close + 1;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn line_is_blank_before(doc: &Rope, pos: u32) -> bool {
    let (line, _) = buffer::line_col(doc, pos);
    buffer::slice(doc, buffer::line_range(doc, line).start, pos).trim().is_empty()
}

/// The postfix key typed before `pos` (possibly empty) and the offset of the single `.` in front
/// of it. `None` when the text before `pos` on its line does not end in `.key`.
pub fn postfix_key_at(doc: &Rope, pos: u32) -> Option<(u32, String)> {
    let (line, _) = buffer::line_col(doc, pos);
    let line_from = buffer::line_range(doc, line).start;
    let before = buffer::slice(doc, line_from, pos);
    // The key: the word the line ends with, without the digits it may start with.
    let word = before.len() - before.trim_end_matches(is_ascii_word).len();
    let key = before[before.len() - word..].trim_start_matches(|c: char| c.is_ascii_digit());
    let dot = pos.checked_sub(key.len() as u32 + 1).filter(|&dot| dot >= line_from)?;
    if buffer::slice(doc, dot, dot + 1) != "." || (dot > 0 && buffer::slice(doc, dot - 1, dot) == ".") {
        return None;
    }
    Some((dot, key.to_owned()))
}

/// Finds the postfix key and the candidate expressions before the caret at `pos`:
/// `foo.bar().if|` gives the key `if` and the expression `foo.bar()`. Wider expressions (`a + b`)
/// come from `ancestors`, the syntax nodes around the dot (see [`postfix_key_at`]), innermost
/// first. `None` when there is no `.key` after an expression.
pub fn find_postfix_target(doc: &Rope, pos: u32, language: &str, ancestors: &[TreeNode]) -> Option<PostfixTarget> {
    let (dot, key) = postfix_key_at(doc, pos)?;
    let window_from = dot.saturating_sub(4000);
    let window = buffer::slice(doc, window_from, dot);
    let start = window_from + scan_expression_start(&window, dot - window_from, language);
    if start >= dot {
        return None;
    }
    let inner = buffer::slice(doc, start, dot);
    // `1.` is a number literal being typed.
    if inner.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut candidates =
        vec![PostfixCandidate { from: start, statement: line_is_blank_before(doc, start), text: inner }];

    // Widen through the syntax tree: the ancestors that end at the key (or the dot).
    let covering = ancestors.iter().position(|node| node.from <= start && node.to >= dot);
    let first = covering.unwrap_or(ancestors.len().saturating_sub(1));
    for node in ancestors.iter().skip(first) {
        if STOP_NODE.is_match(&pascal_kind(&node.kind)) || node.error || node.to < dot || node.to > pos {
            break;
        }
        if node.from >= start {
            continue;
        }
        let text = buffer::slice(doc, node.from, dot);
        let text = text.trim();
        if text.is_empty() || starts_with_keyword(text) || text.ends_with('.') {
            break;
        }
        if without_strings(text).contains([';', '{', '}']) {
            break;
        }
        if candidates.iter().any(|candidate| candidate.text == text) {
            continue;
        }
        let statement = line_is_blank_before(doc, node.from);
        candidates.push(PostfixCandidate { from: node.from, text: text.to_owned(), statement });
    }
    Some(PostfixTarget { dot, end: pos, key, candidates })
}

/// Whether `expr` needs parentheses in front of a prefix operator: `a + b` does.
pub fn is_compound(expr: &str) -> bool {
    let chars: Vec<char> = expr.chars().collect();
    let mut depth = 0i32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if is_quote(c) {
            match chars[i + 1..].iter().position(|&next| next == c) {
                Some(close) => i += close + 1,
                None => return true,
            }
        } else if matches!(c, '(' | '[' | '{') {
            depth += 1;
        } else if matches!(c, ')' | ']' | '}') {
            depth -= 1;
        } else if depth == 0
            && (c.is_whitespace() || "+-*/%<>=&|^?:,".contains(c))
            && !(c == ':' && chars.get(i + 1) == Some(&':'))
            && !(c == ':' && i > 0 && chars[i - 1] == ':')
            && !(c == '?' && chars.get(i + 1) == Some(&'.'))
        {
            return true;
        }
        i += 1;
    }
    false
}

/// The candidates `template` may use: a statement template needs a statement position.
pub fn candidates_for<'t>(template: &PostfixTemplate, target: &'t PostfixTarget) -> Vec<&'t PostfixCandidate> {
    target.candidates.iter().filter(|candidate| candidate.statement || !template.statement).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::live_model::{ExpansionEnv, TemplateFunctions, expand_template};
    use std::collections::BTreeMap;

    fn scan<'t>(text: &'t str, language: &str) -> &'t str {
        let end = text.encode_utf16().count() as u32;
        &text[buffer::byte_of_col(text, scan_expression_start(text, end, language))..]
    }

    fn target(language: &str, doc: &str, ancestors: &[TreeNode]) -> Option<PostfixTarget> {
        find_postfix_target(&Rope::from_str(doc), doc.encode_utf16().count() as u32, language, ancestors)
    }

    fn texts(target: &PostfixTarget) -> Vec<&str> {
        target.candidates.iter().map(|candidate| candidate.text.as_str()).collect()
    }

    fn template(language: &str, key: &str) -> &'static PostfixTemplate {
        let all = postfix_templates_for(language, &BTreeSet::new());
        all.into_iter().find(|t| t.key == key).expect("a template of the language")
    }

    // expression scanning › handles call chains, indexing, strings and generics
    #[test]
    fn handles_call_chains_indexing_strings_and_generics() {
        assert_eq!(scan("const x = foo.bar(1, (2)).baz[0]", "typescript"), "foo.bar(1, (2)).baz[0]");
        assert_eq!(scan("return \"a.b\".length", "typescript"), "\"a.b\".length");
        assert_eq!(scan("let v = vec![1, 2].iter().map(|x| x * 2)", "rust"), "vec![1, 2].iter().map(|x| x * 2)");
        assert_eq!(scan("    file.read()?", "rust"), "file.read()?");
        assert_eq!(scan("x = Foo::<u8>::new()", "rust"), "Foo::<u8>::new()");
        assert_eq!(scan("var a = new ArrayList<String>()", "java"), "new ArrayList<String>()");
        assert_eq!(scan("val n = user?.name!!", "kotlin"), "user?.name!!");
        assert_eq!(scan("print(len(xs))", "python"), "print(len(xs))");
        assert_eq!(scan("a + b", "go"), "b");
        assert_eq!(scan("  f(", "go"), "");
    }

    // expression scanning › detects compound expressions
    #[test]
    fn detects_compound_expressions() {
        assert!(is_compound("a + b"));
        assert!(!is_compound("a.b(c + d)"));
        assert!(!is_compound("Foo::new()"));
        assert!(!is_compound("user?.name"));
        assert!(is_compound("x > 0"));
    }

    // postfix target › TypeScript: call chain and wider binary expression
    //
    // The syntax chains of this and the following cases are written by hand, as a tree-sitter
    // grammar shapes the documents.
    #[test]
    fn typescript_call_chain_and_wider_binary_expression() {
        // items.filter(Boolean).for
        let chain = [
            TreeNode::new("call_expression", 0, 21),
            TreeNode::new("member_expression", 0, 25),
            TreeNode::new("expression_statement", 0, 25),
            TreeNode::new("program", 0, 25),
        ];
        let t = target("typescript", "items.filter(Boolean).for", &chain).expect("a target");
        assert_eq!(t.key, "for");
        assert_eq!(texts(&t), ["items.filter(Boolean)"]);
        assert!(t.candidates[0].statement);

        // a + b.if
        let binary = [
            TreeNode::new("identifier", 4, 5),
            TreeNode::new("member_expression", 4, 8),
            TreeNode::new("binary_expression", 0, 8),
            TreeNode::new("expression_statement", 0, 8),
            TreeNode::new("program", 0, 8),
        ];
        let b = target("typescript", "a + b.if", &binary).expect("a target");
        assert_eq!(texts(&b), ["b", "a + b"]);
        let usable: Vec<&str> =
            candidates_for(template("typescript", "if"), &b).iter().map(|c| c.text.as_str()).collect();
        assert_eq!(usable, ["a + b"]);
    }

    // postfix target › Rust: method chain with ?
    #[test]
    fn rust_method_chain_with_question_mark() {
        let t = target("rust", "fn f() {\n    std::fs::read(path)?.len().let", &[]).expect("a target");
        assert_eq!(t.key, "let");
        assert_eq!(t.candidates[0].text, "std::fs::read(path)?.len()");
    }

    // postfix target › Python: not a statement inside a call
    #[test]
    fn python_not_a_statement_inside_a_call() {
        // print(xs.len
        let chain = [
            TreeNode::new("identifier", 6, 8),
            TreeNode::new("attribute", 6, 12),
            TreeNode::new("argument_list", 5, 12),
            TreeNode::new("call", 0, 12),
        ];
        let t = target("python", "print(xs.len", &chain).expect("a target");
        assert_eq!(t.candidates[0].text, "xs");
        assert!(!t.candidates[0].statement);
        assert!(candidates_for(template("python", "if"), &t).is_empty());
    }

    // postfix target › Go, Java, Kotlin
    #[test]
    fn go_java_kotlin() {
        let first = |language: &str, doc: &str| target(language, doc, &[]).map(|t| t.candidates[0].text.clone());
        assert_eq!(first("go", "func f() {\n\tos.Open(name).var").as_deref(), Some("os.Open(name)"));
        assert_eq!(first("java", "class A { void f() {\n  new Foo(1).var").as_deref(), Some("new Foo(1)"));
        assert_eq!(first("kotlin", "fun f() {\n  user?.name.nn").as_deref(), Some("user?.name"));
    }

    // postfix target › returns null without a key position or expression
    #[test]
    fn returns_none_without_a_key_position_or_expression() {
        assert_eq!(target("typescript", "foo", &[]), None);
        assert_eq!(target("typescript", "  .if", &[]), None);
        assert_eq!(target("typescript", "x = 1.", &[]), None);
    }

    // postfix target › expands with $EXPR$ and a suggested name
    #[test]
    fn expands_with_expr_and_a_suggested_name() {
        let tpl = template("typescript", "const");
        let env = ExpansionEnv {
            predefined: BTreeMap::from([("EXPR".to_owned(), "user.getName()".to_owned())]),
            fns: TemplateFunctions::new().with("suggestVariableName", |_| "name".to_owned()),
            ..ExpansionEnv::default()
        };
        let e = expand_template(&tpl.body, &tpl.variables, &env);
        assert_eq!(e.snippet, "const ${1:name} = user.getName();${0}");
    }

    // postfix target › every language has the core keys
    #[test]
    fn every_language_has_the_core_keys() {
        for lang in ["typescript", "rust", "python", "go", "java", "kotlin"] {
            let keys: Vec<&str> =
                postfix_templates_for(lang, &BTreeSet::new()).iter().map(|t| t.key.as_str()).collect();
            for key in ["if", "not", "return", "par", "nn"] {
                if lang == "rust" && key == "nn" {
                    continue;
                }
                assert!(keys.contains(&key), "{lang} {key}");
            }
        }
        let disabled = BTreeSet::from(["ts.if".to_owned()]);
        assert!(!postfix_templates_for("typescript", &disabled).iter().any(|t| t.key == "if"));
    }

    // Not from a spec: a wider expression stops at statements, errors, keywords and braces, and a
    // key may follow digits of a longer word.
    #[test]
    fn widening_stops_at_what_is_not_an_expression() {
        // return a + b.not
        let chain = [
            TreeNode::new("identifier", 11, 12),
            TreeNode::new("member_expression", 11, 16),
            TreeNode::new("binary_expression", 7, 16),
            TreeNode::new("return_statement", 0, 16),
        ];
        let t = target("typescript", "return a + b.not", &chain).expect("a target");
        assert_eq!(texts(&t), ["b", "a + b"]);
        assert!(!t.candidates[1].statement);
        let broken = [TreeNode::new("identifier", 11, 12), TreeNode { error: true, ..TreeNode::new("ERROR", 7, 16) }];
        assert_eq!(texts(&target("typescript", "return a + b.not", &broken).expect("a target")), ["b"]);
        assert!(starts_with_keyword("return x") && starts_with_keyword("new") && !starts_with_keyword("newest"));
        assert_eq!(without_strings("f(\"a;b\", '{') + `}` + \"open"), "f(, ) +  + \"open");
        assert_eq!(scan("x = \"😀\" + sé.len()", "rust"), "sé.len()");
        assert_eq!(postfix_templates().len(), 99);
    }
}
