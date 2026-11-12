//! Name suggestions for Introduce Variable / Constant without a language server, after IDEA:
//! from the callee (`getUser()` is `user`), the constructed type (`new StringBuilder()` is
//! `stringBuilder`, `builder`), the accessed property, the literal kind (`s`, `i`, `b`…). Purely
//! textual; callers drop names already used in the file ([`unique_names`]).

use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStyle {
    Camel,
    Snake,
    /// UPPER_SNAKE_CASE.
    Constant,
}

/// How a language spells variable names.
pub fn name_style(language: &str) -> NameStyle {
    match language {
        "python" | "ruby" | "rust" | "elixir" | "c" | "zig" | "lua" => NameStyle::Snake,
        _ => NameStyle::Camel,
    }
}

const KEYWORDS: &str = "abstract and as assert async await begin bool boolean break byte case catch char class const \
continue def default defer del delete do double elif else elsif end ensure enum except export extends extern false \
final finally float fn for from func function global go goto if impl import in instanceof int interface is lambda \
let local long loop match mod module mut native new nil none not null of or package pass private protected public \
raise rescue return self short static struct super switch synchronized then this throw throws trait true try typeof \
undefined unless until use val var void volatile when where while with yield";

const STRIP_PREFIXES: [&str; 13] =
    ["get", "to", "as", "create", "new", "make", "build", "find", "fetch", "load", "read", "compute", "calculate"];
const BOOL_PREFIXES: [&str; 4] = ["is", "has", "can", "should"];

static LEADING_SIGILS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[$@]+"));
static LOWER_UPPER: LazyLock<Regex> = LazyLock::new(|| pattern(r"([a-z0-9])([A-Z])"));
static ACRONYM: LazyLock<Regex> = LazyLock::new(|| pattern(r"([A-Z]+)([A-Z][a-z])"));
static WORD_BREAK: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\s_-]+"));

/// `getUserName`, `get_user_name` and `GetUserName` are `["get", "user", "name"]`.
pub fn split_words(identifier: &str) -> Vec<String> {
    let text = LEADING_SIGILS.replace(identifier, "");
    let text = LOWER_UPPER.replace_all(&text, "${1} ${2}");
    let text = ACRONYM.replace_all(&text, "${1} ${2}");
    WORD_BREAK.split(&text).filter(|word| !word.is_empty()).map(str::to_lowercase).collect()
}

fn join(words: &[String], style: NameStyle) -> String {
    match style {
        NameStyle::Constant => words.iter().map(|word| word.to_uppercase()).collect::<Vec<_>>().join("_"),
        NameStyle::Snake => words.join("_"),
        NameStyle::Camel => words
            .iter()
            .enumerate()
            .map(|(i, word)| {
                if i == 0 {
                    return word.clone();
                }
                let mut chars = word.chars();
                chars.next().map(|first| first.to_uppercase().chain(chars).collect()).unwrap_or_default()
            })
            .collect(),
    }
}

/// Every suffix of the word list, longest first: `[string, builder]` gives `stringBuilder`,
/// `builder`.
fn suffixes(words: &[String]) -> Vec<Vec<String>> {
    (0..words.len()).map(|i| words[i..].to_vec()).collect()
}

/// The start of the balanced bracket group that ends right before byte `end`, if any.
fn opening_bracket(text: &str, end: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0i32;
    for (i, byte) in text.as_bytes()[..end].iter().enumerate().rev() {
        if *byte == close {
            depth += 1;
        } else if *byte == open {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
    }
    None
}

static TURBOFISH_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"::<[^>]*>\s*$"));
static GENERICS_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"<[^<>()]*>\s*$"));
static LAST_SEGMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"([A-Za-z_$][A-Za-z0-9_$]*)\s*$"));

/// The last identifier of a callee or member chain: `a.b::c->d` is `d`.
fn last_segment(path: &str) -> Option<String> {
    let path = TURBOFISH_END.replace(path, "");
    let path = GENERICS_END.replace(&path, "");
    LAST_SEGMENT.captures(&path).map(|captures| captures[1].to_owned())
}

fn singular(word: &str) -> Option<String> {
    if let Some(stem) = word.strip_suffix("ies") {
        return Some(format!("{stem}y"));
    }
    if word.ends_with("ss") || word.ends_with("us") {
        return None;
    }
    for ending in ["shes", "ches", "xes"] {
        if word.ends_with(ending) {
            return Some(word[..word.len() - 2].to_owned());
        }
    }
    if word.ends_with('s') && word.chars().count() > 2 {
        return Some(word[..word.len() - 1].to_owned());
    }
    None
}

/// The expression a name is suggested for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuggestInput<'a> {
    /// The syntax node kind of the expression.
    pub kind: &'a str,
    pub text: &'a str,
    pub language: &'a str,
    pub constant: bool,
}

static UNWRAP_KEYWORD: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(await|try[!?]?)\s+"));
static UNWRAP_OPERATOR: LazyLock<Regex> = LazyLock::new(|| pattern(r"(\?|!!|!)$"));
static UNWRAP_CALL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"\.(unwrap|expect|unwrap_or_default|clone|to_owned)\([^()]*\)$"));
static AT_STRING: LazyLock<Regex> = LazyLock::new(|| pattern(r#"^@?"((?s:.)*)"$"#));
static CHAR_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"char"));
static CHAR_TEXT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^'.'$|^'\\.'$"));
static STRING_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"string|encapsed|template|heredoc"));
static LITERAL_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"literal|string"));
static WORDY: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Za-z][A-Za-z0-9 _-]{0,40}$"));
static INTEGER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^-?\s*([0-9][0-9_]*|0[xXbBoO][0-9a-fA-F_]+)[lLuU]*$"));
static FLOAT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^-?\s*([0-9][0-9_]*)?\.?[0-9][0-9_]*([eE][+-]?[0-9]+)?[fFdDmM]?$"));
static BOOLEAN: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(true|false|True|False|YES|NO)$"));
static NULL: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(null|nil|None|undefined|nullptr)$"));
static LIST: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\[(?s:.)*\]$"));
static LIST_CALL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(vec!|listOf|arrayOf|mutableListOf|List\.of|Arrays\.asList)"));
static OBJECT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\{(?s:.)*\}$"));
static OBJECT_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"object|dictionary|hash|map"));
static FUNCTION_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"lambda|arrow|closure|function|anonymous"));
static NEW: LazyLock<Regex> = LazyLock::new(|| pattern(r"^new\s+([A-Za-z0-9_$.]+)"));
static CONSTRUCTOR: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"^&?(?:mut\s+)?([A-Z][A-Za-z0-9_$]*)",
        r"(?:::(?:new|default|from|with_[A-Za-z0-9_]+|create|of))?\s*(?:::<[^>]*>)?\s*[({]",
    ))
});
static FACTORY: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"^([A-Za-z0-9_$.]*\.)?([A-Z][A-Za-z0-9_$]*)\.(?:of|from|valueOf|newInstance|create|getInstance)\s*\(")
});
static ACCESS_END: LazyLock<Regex> = LazyLock::new(|| pattern(r"(\.|::|->|\?\.)\s*$"));
static UPPER_START: LazyLock<Regex> = LazyLock::new(|| pattern(r"^[A-Z]"));
static CONDITION_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"comparison|boolean_operator|not_operator|logical"));
static CONDITION_TEXT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(==|!=|<=|>=|&&|\|\||\s<\s|\s>\s|\bnot\b|\band\b|\bor\b|^!)"));
static NOT_CONDITION: LazyLock<Regex> = LazyLock::new(|| pattern(r"[=!]~|=>"));
static MEMBER: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?:^|\.|::|->|\?\.)\s*\$?([A-Za-z_][A-Za-z0-9_]*)$"));
static ACCESSOR: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s*(\.|::|->|\?\.)\s*"));
static OPERATOR_CHARS: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\s+\-*/%^|&(),]"));
static QUOTE_START: LazyLock<Regex> = LazyLock::new(|| pattern(r#"^["'`]"#));
static CONCATENATION: LazyLock<Regex> = LazyLock::new(|| pattern(r#"\+\s*["'`]|["'`]\s*\+"#));

/// The content of a quoted string literal (with up to two prefix letters, `r"…"`, `f'…'`).
fn quoted_content(text: &str) -> Option<String> {
    const PREFIX: &str = "rbuRBUfF";
    let chars: Vec<char> = text.chars().collect();
    for prefix in (0..=2usize).rev() {
        if chars.len() < prefix + 2 || !chars[..prefix].iter().all(|c| PREFIX.contains(*c)) {
            continue;
        }
        let quote = chars[prefix];
        if matches!(quote, '"' | '\'' | '`') && chars[chars.len() - 1] == quote {
            return Some(chars[prefix + 1..chars.len() - 1].iter().collect());
        }
    }
    AT_STRING.captures(text).map(|captures| captures[1].to_owned())
}

/// Languages where `'x'` is a string rather than a character.
const QUOTE_STRINGS: [&str; 10] = [
    "python",
    "javascript",
    "typescript",
    "javascriptreact",
    "typescriptreact",
    "ruby",
    "php",
    "lua",
    "elixir",
    "dart",
];

fn words(list: &[&str]) -> Vec<String> {
    list.iter().map(|word| (*word).to_owned()).collect()
}

/// Word lists, best first.
fn word_candidates(kind: &str, raw: &str, language: &str) -> Vec<Vec<String>> {
    let mut text = raw.trim().to_owned();
    // Unwrap what does not change the value's nature.
    loop {
        let before = text.clone();
        text = UNWRAP_KEYWORD.replace(&text, "").into_owned();
        text = UNWRAP_OPERATOR.replace(&text, "").into_owned();
        text = UNWRAP_CALL.replace(&text, "").trim().to_owned();
        if text.starts_with('(') && opening_bracket(&text, text.len(), b'(', b')') == Some(0) {
            text = text[1..text.len() - 1].trim().to_owned();
        }
        if text == before {
            break;
        }
    }
    let text = text.as_str();

    // Literals.
    let quoted = quoted_content(text);
    if CHAR_KIND.is_match(kind) || (CHAR_TEXT.is_match(text) && !QUOTE_STRINGS.contains(&language)) {
        return vec![words(&["c"]), words(&["ch"])];
    }
    if STRING_KIND.is_match(kind) || (quoted.is_some() && LITERAL_KIND.is_match(kind)) {
        let content = quoted.unwrap_or_default();
        let content = content.trim();
        let found = if WORDY.is_match(content) { split_words(content) } else { Vec::new() };
        let mut out = Vec::new();
        if !found.is_empty() && found.len() <= 3 {
            out.push(found);
        }
        out.extend([words(&["s"]), words(&["text"])]);
        return out;
    }
    if INTEGER.is_match(text) {
        return vec![words(&["i"]), words(&["value"])];
    }
    if FLOAT.is_match(text) {
        return vec![words(&["value"]), words(&["v"])];
    }
    if BOOLEAN.is_match(text) {
        return vec![words(&["b"]), words(&["flag"])];
    }
    if NULL.is_match(text) {
        return vec![words(&["value"])];
    }
    if LIST.is_match(text) || LIST_CALL.is_match(text) {
        return vec![words(&["list"]), words(&["items"])];
    }
    if OBJECT.is_match(text) && OBJECT_KIND.is_match(kind) {
        return if language == "python" {
            vec![words(&["data"]), words(&["mapping"])]
        } else {
            vec![words(&["obj"]), words(&["data"])]
        };
    }
    if FUNCTION_KIND.is_match(kind) {
        return vec![words(&["fn"]), words(&["callback"])];
    }

    // Construction: `new Foo<T>(…)`, `Foo(…)`, `Foo::new(…)`, `&Foo { … }`, `Foo.of(…)`.
    let constructed = NEW
        .captures(text)
        .or_else(|| CONSTRUCTOR.captures(text))
        .map(|captures| captures[1].to_owned())
        .or_else(|| FACTORY.captures(text).map(|captures| captures[2].to_owned()));
    if let Some(constructed) = constructed {
        let type_name = last_segment(&constructed).unwrap_or_default();
        let found: Vec<String> = split_words(&type_name).into_iter().filter(|word| word != "new").collect();
        if !found.is_empty() {
            return suffixes(&found);
        }
    }

    // Calls: the last top-level argument list.
    if text.ends_with(')')
        && let Some(open) = opening_bracket(text, text.len(), b'(', b')').filter(|open| *open > 0)
    {
        let callee = text[..open].trim();
        if let Some(name) = last_segment(callee) {
            let found = split_words(&name);
            let first = found.first().map(String::as_str).unwrap_or_default();
            let mut out = Vec::new();
            if found.len() > 1 && BOOL_PREFIXES.contains(&first) {
                out.extend(suffixes(&found[1..]));
                out.push(found);
            } else if found.len() > 1 && STRIP_PREFIXES.contains(&first) {
                out.extend(suffixes(&found[1..]));
            } else if found.len() == 1 && (STRIP_PREFIXES.contains(&first) || first == "new") {
                // `map.get(k)`, `Foo.create()`: named after the receiver.
                let before = callee.get(..callee.len().saturating_sub(name.len())).unwrap_or_default();
                let receiver = last_segment(&ACCESS_END.replace(before, ""));
                let single = receiver
                    .as_deref()
                    .and_then(|receiver| singular(split_words(receiver).last().map_or("", String::as_str)));
                if let Some(single) = single {
                    out.push(vec![single]);
                } else if let Some(receiver) = receiver.filter(|receiver| UPPER_START.is_match(receiver)) {
                    out.extend(suffixes(&split_words(&receiver)));
                } else if !["get", "new", "to", "as"].contains(&first) {
                    out.push(found);
                }
            } else {
                out.extend(suffixes(&found));
            }
            out.push(words(&["value"]));
            return out;
        }
    }

    // Index access: `items[i]` is an `item`.
    if text.ends_with(']')
        && let Some(open) = opening_bracket(text, text.len(), b'[', b']').filter(|open| *open > 0)
    {
        let single =
            last_segment(&text[..open]).and_then(|base| split_words(&base).pop()).and_then(|last| singular(&last));
        let mut out: Vec<Vec<String>> = single.map(|single| vec![single]).into_iter().collect();
        out.extend([words(&["item"]), words(&["element"])]);
        return out;
    }

    // Comparisons and boolean operators.
    if (CONDITION_KIND.is_match(kind) || CONDITION_TEXT.is_match(text)) && !NOT_CONDITION.is_match(text) {
        return vec![words(&["b"]), words(&["flag"])];
    }

    // Member access and plain identifiers: `a.b.userName` is `userName`, `name`.
    if let Some(captures) = MEMBER.captures(text)
        && !OPERATOR_CHARS.is_match(&ACCESSOR.replace_all(text, "."))
    {
        let found = split_words(&captures[1]);
        let mut out = match found.first() {
            Some(first) if found.len() > 1 && STRIP_PREFIXES.contains(&first.as_str()) => suffixes(&found[1..]),
            _ => suffixes(&found),
        };
        out.push(words(&["value"]));
        return out;
    }
    if QUOTE_START.is_match(text) || CONCATENATION.is_match(text) {
        return vec![words(&["s"]), words(&["text"])];
    }
    vec![words(&["value"])]
}

/// Suggested names, best first and distinct; at least one. Keywords and non-identifiers are
/// dropped; a constant gets UPPER_SNAKE_CASE.
pub fn suggest_names(input: &SuggestInput<'_>) -> Vec<String> {
    let style = if input.constant { NameStyle::Constant } else { name_style(input.language) };
    let mut out: Vec<String> = Vec::new();
    for candidate in word_candidates(input.kind, input.text, input.language) {
        if candidate.is_empty() {
            continue;
        }
        let name = join(&candidate, style);
        if !is_identifier(&name) || is_keyword(&name) || out.contains(&name) {
            continue;
        }
        out.push(name);
    }
    if out.is_empty() {
        out.push(if input.constant { "VALUE" } else { "value" }.to_owned());
    }
    out
}

pub fn is_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn is_keyword(name: &str) -> bool {
    let lower = name.to_lowercase();
    KEYWORDS.split(' ').any(|keyword| keyword == lower)
}

/// Drops the names `taken` reports as used in the file (the caller answers from the file's
/// unqualified identifier tokens; a name it cannot check counts as taken). When every
/// suggestion is taken the first one is numbered: `user1`, `user2`…
pub fn unique_names(names: &[String], taken: impl Fn(&str) -> bool) -> Vec<String> {
    let free: Vec<String> = names.iter().filter(|name| !taken(name)).cloned().collect();
    if !free.is_empty() {
        return free;
    }
    let Some(base) = names.first() else { return Vec::new() };
    let numbered = (1..100).map(|i| format!("{base}{i}")).find(|name| !taken(name));
    vec![numbered.unwrap_or_else(|| base.clone())]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str, kind: &str, language: &str, constant: bool) -> Vec<String> {
        suggest_names(&SuggestInput { kind, text, language, constant })
    }

    fn java(text: &str, kind: &str) -> Vec<String> {
        s(text, kind, "java", false)
    }

    #[test]
    fn names_calls_after_the_callee_without_get_to_create_prefixes() {
        assert_eq!(java("getUser()", "call_expression"), ["user", "value"]);
        assert_eq!(java("repo.getUserName(id)", "call_expression"), ["userName", "name", "value"]);
        assert_eq!(s("self.get_user_name()", "call", "python", false), ["user_name", "name", "value"]);
        assert_eq!(java("list.isEmpty()", "call_expression"), ["empty", "isEmpty", "value"]);
        assert_eq!(s("await fetchOrders()", "await_expression", "typescript", false), ["orders", "value"]);
        assert_eq!(java("users.get(0)", "call_expression"), ["user", "value"]);
        assert_eq!(java("compute(a + b, c)", "call_expression"), ["compute", "value"]);
    }

    #[test]
    fn names_constructions_after_the_type() {
        assert_eq!(java("new StringBuilder()", "object_creation_expression"), ["stringBuilder", "builder"]);
        assert_eq!(java("new java.util.ArrayList<String>()", "object_creation_expression"), ["arrayList", "list"]);
        assert_eq!(s("StringBuilder()", "call_expression", "kotlin", false), ["stringBuilder", "builder"]);
        assert_eq!(s("HashMap::new()", "call_expression", "rust", false), ["hash_map", "map"]);
    }

    #[test]
    fn names_literals_members_indexing_and_conditions() {
        assert_eq!(java("\"hello world\"", "string_literal"), ["helloWorld", "s", "text"]);
        assert_eq!(java("\"a/b?c\"", "string"), ["s", "text"]);
        assert_eq!(java("42", "decimal_integer_literal"), ["i", "value"]);
        assert_eq!(java("1.5", "float"), ["value", "v"]);
        assert_eq!(java("true", "true"), ["b", "flag"]);
        assert_eq!(java("order.totalPrice", "field_access"), ["totalPrice", "price", "value"]);
        assert_eq!(s("$o->name", "member_access_expression", "php", false), ["name", "value"]);
        assert_eq!(java("items[i]", "array_access"), ["item", "element"]);
        assert_eq!(java("a > b && c", "binary_expression"), ["b", "flag"]);
        assert_eq!(java("(a + b)", "parenthesized_expression"), ["value"]);
    }

    #[test]
    fn gives_upper_snake_case_constants_and_drops_keywords() {
        assert_eq!(s("\"max retries\"", "string_literal", "java", true), ["MAX_RETRIES", "S", "TEXT"]);
        assert_eq!(s("100", "integer", "python", true), ["I", "VALUE"]);
        assert_eq!(java("new Object()", "object_creation_expression"), ["object"]);
        assert_eq!(java("getClass()", "call_expression"), ["value"]);
    }

    #[test]
    fn drops_taken_names_and_numbers_the_best_one_when_all_are_taken() {
        let taken = |name: &str| ["user", "value", "user1"].contains(&name);
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect::<Vec<_>>();
        assert_eq!(unique_names(&names(&["user", "name", "value"]), taken), ["name"]);
        assert_eq!(unique_names(&names(&["user", "value"]), taken), ["user2"]);
    }
}
