//! Method / field declarations on a source line, named the way debug adapters expect them in
//! function breakpoints and data breakpoint ids. Text-based (no syntax tree): good for ordinary
//! declarations, which is what the gutter toggles act on.

use regex::Regex;
use std::sync::LazyLock;

/// A method to break on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodTarget {
    /// Function breakpoint name for the language's adapter.
    pub name: String,
    /// Short label (`Order.total()`).
    pub label: String,
}

/// A field to watch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldTarget {
    /// java-debug data breakpoint id (`pkg.Class#field`).
    pub data_id: String,
    pub label: String,
}

const CONTROL: [&str; 15] = [
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "return",
    "new",
    "throw",
    "else",
    "do",
    "try",
    "synchronized",
    "assert",
    "super",
    "this",
];

/// A pattern in which `\w`, `\d` and `\b` mean ASCII word characters, digits and word boundaries,
/// as source code identifiers are matched here.
fn ascii(source: &str) -> Regex {
    let mut pattern = String::with_capacity(source.len() + 32);
    let mut in_class = false;
    let mut chars = source.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match (chars.next(), in_class) {
                (Some('w'), true) => pattern.push_str("0-9A-Za-z_"),
                (Some('w'), false) => pattern.push_str("[0-9A-Za-z_]"),
                (Some('d'), true) => pattern.push_str("0-9"),
                (Some('d'), false) => pattern.push_str("[0-9]"),
                (Some('b'), false) => pattern.push_str(r"(?-u:\b)"),
                (Some(escaped), _) => {
                    pattern.push('\\');
                    pattern.push(escaped);
                }
                (None, _) => pattern.push('\\'),
            },
            '[' if !in_class => {
                in_class = true;
                pattern.push(c);
            }
            ']' if in_class => {
                in_class = false;
                pattern.push(c);
            }
            _ => pattern.push(c),
        }
    }
    Regex::new(&pattern).expect("the declaration pattern is valid")
}

macro_rules! pattern {
    ($name:ident, $source:expr $(,)?) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| ascii($source));
    };
}

pattern!(PACKAGE, r"^\s*package\s+([\w.]+)");
pattern!(TYPE, r"\b(?:class|interface|enum|record|object)\s+([A-Za-z_$][\w$]*)");
pattern!(JAVA_METHOD, r"^\s*(?:@\w+(?:\([^)]*\))?\s+)*(?:[\w<>,.?\[\]\s]+?\s+)?([A-Za-z_$][\w$]*)\s*\(");
pattern!(NOT_A_DECLARATION, r"[=.]|\breturn\b|\bnew\b");
pattern!(
    JAVA_FIELD,
    concat!(
        r"^\s*(?:@\w+(?:\([^)]*\))?\s+)*((?:(?:public|protected|private|static|final|volatile|transient)\s+)*)",
        r"[\w<>,.?\[\]\s]+?\s+([A-Za-z_$][\w$]*)\s*(?:=|;)",
    )
);
pattern!(RETURN, r"^\s*return\b");
pattern!(GO_FUNC, r"^\s*func\s+(?:\(\s*\w*\s*(\*?)\s*([A-Za-z_]\w*)(?:\[[^\]]*\])?\s*\)\s*)?([A-Za-z_]\w*)\s*[\[(]");
pattern!(GO_PACKAGE, r"^\s*package\s+(\w+)");
pattern!(KOTLIN_FUN, r"^\s*(?:[\w@]+\s+)*fun\s+(?:<[^>]*>\s*)?(?:[\w.]+\.)?([A-Za-z_]\w*)\s*\(");
pattern!(PYTHON_DEF, r"^\s*(?:async\s+)?def\s+([A-Za-z_]\w*)\s*\(");
pattern!(RUST_FN, r#"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe|extern\s+"[^"]*")\s+)*fn\s+([A-Za-z_]\w*)"#);
pattern!(JS_FUNCTION, r"^\s*(?:export\s+)?(?:default\s+)?(?:async\s+)?function\*?\s+([A-Za-z_$][\w$]*)");
pattern!(
    JS_METHOD,
    concat!(
        r"^\s*(?:(?:public|private|protected|static|async|get|set)\s+)*",
        r"([A-Za-z_$][\w$]*)\s*\([^)]*\)\s*(?::[^{]*)?\{",
    )
);
pattern!(
    C_FUNCTION,
    concat!(
        r"^\s*(?:(?:static|inline|extern|virtual|constexpr|const|unsigned|signed|struct|enum)\s+)*",
        r"[A-Za-z_][\w:<>,*&\s]*?[\s*&]([A-Za-z_~][\w:~]*)\s*\([^;]*$",
    )
);
pattern!(SWIFT_FUNC, r"^\s*(?:[\w@]+\s+)*func\s+([A-Za-z_]\w*)");

/// `line` without string / char literals and comments, so braces and keywords inside them don't
/// count. `block`: a block comment is open, across lines.
fn code(line: &str, block: &mut bool) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let (c, next) = (chars[i], chars.get(i + 1).copied());
        if *block {
            if c == '*' && next == Some('/') {
                *block = false;
                i += 1;
            }
            i += 1;
        } else if c == '/' && next == Some('/') {
            break;
        } else if c == '/' && next == Some('*') {
            *block = true;
            i += 2;
        } else if c == '"' || c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != c {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            out.push(c);
            out.push(c);
            i += 1;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

/// The package and the enclosing types of `line` (outermost first) in a brace language.
fn java_context(lines: &[&str], line: usize) -> (String, Vec<String>) {
    let mut package = String::new();
    let mut stack: Vec<(Option<String>, i32)> = Vec::new();
    let mut depth = 0;
    let mut pending_type = None;
    let mut block = false;
    for text in lines.iter().take(line) {
        let text = code(text, &mut block);
        if let Some(found) = PACKAGE.captures(&text) {
            package = found[1].to_owned();
        }
        if let Some(found) = TYPE.captures(&text) {
            pending_type = Some(found[1].to_owned());
        }
        for c in text.chars() {
            if c == '{' {
                depth += 1;
                stack.push((pending_type.take(), depth));
            } else if c == '}' {
                while stack.last().is_some_and(|(_, opened)| *opened >= depth) {
                    stack.pop();
                }
                depth -= 1;
            }
        }
    }
    // A type declared on the line itself (one-line `class A { void f() {} }`) is ignored.
    (package, stack.into_iter().filter_map(|(name, _)| name).collect())
}

fn qualified(package: &str, types: &[String]) -> Option<String> {
    if types.is_empty() {
        return None;
    }
    let prefix = if package.is_empty() { String::new() } else { format!("{package}.") };
    Some(format!("{prefix}{}", types.join("$")))
}

fn line_at<'a>(lines: &[&'a str], line: usize) -> &'a str {
    lines.get(line).copied().unwrap_or_default()
}

fn java_method(lines: &[&str], line: usize) -> Option<MethodTarget> {
    let text = code(line_at(lines, line), &mut false);
    let found = JAVA_METHOD.captures(&text)?;
    let name = &found[1];
    // What precedes the name: a declaration has a type or modifiers there, or is a constructor.
    let before_len = (found[0].chars().count() - 1).saturating_sub(name.chars().count());
    let before: String = text.chars().take(before_len).collect();
    let before = before.trim();
    if CONTROL.contains(&name) || NOT_A_DECLARATION.is_match(before) {
        return None;
    }
    let (package, types) = java_context(lines, line);
    let class = qualified(&package, &types)?;
    let innermost = types.last()?;
    let is_constructor = name == innermost.as_str();
    // `foo(x);` is a call.
    if before.is_empty() && !is_constructor {
        return None;
    }
    let method = if is_constructor { "<init>" } else { name };
    Some(MethodTarget { name: format!("{class}#{method}"), label: format!("{innermost}.{name}()") })
}

/// Whether every open brace before `line` belongs to a type declaration.
fn inside_type_body(lines: &[&str], line: usize, type_depth: usize) -> bool {
    let mut depth: i64 = 0;
    let mut block = false;
    for text in lines.iter().take(line) {
        for c in code(text, &mut block).chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ => {}
            }
        }
    }
    i64::try_from(type_depth).is_ok_and(|types| types == depth)
}

fn java_field(lines: &[&str], line: usize) -> Option<FieldTarget> {
    let text = code(line_at(lines, line), &mut false);
    if text.find('(').is_some_and(|paren| !text[..paren].contains('=')) {
        return None;
    }
    let found = JAVA_FIELD.captures(&text)?;
    let name = &found[2];
    if CONTROL.contains(&name) || RETURN.is_match(&text) {
        return None;
    }
    let (package, types) = java_context(lines, line);
    let class = qualified(&package, &types)?;
    // Only directly inside a type body (a local variable sits inside a method's braces).
    if !inside_type_body(lines, line, types.len()) {
        return None;
    }
    Some(FieldTarget { data_id: format!("{class}#{name}"), label: format!("{}.{name}", types.last()?) })
}

fn go_method(lines: &[&str], line: usize) -> Option<MethodTarget> {
    let found = GO_FUNC.captures(line_at(lines, line))?;
    let package = lines.iter().find_map(|l| GO_PACKAGE.captures(l)).map_or("main".to_owned(), |p| p[1].to_owned());
    let (star, function) = (found.get(1).is_some_and(|star| !star.as_str().is_empty()), &found[3]);
    let target = match found.get(2).map(|receiver| receiver.as_str()) {
        Some(receiver) if star => MethodTarget {
            name: format!("{package}.(*{receiver}).{function}"),
            label: format!("{receiver}.{function}()"),
        },
        Some(receiver) => {
            MethodTarget { name: format!("{package}.{receiver}.{function}"), label: format!("{receiver}.{function}()") }
        }
        None => MethodTarget { name: format!("{package}.{function}"), label: format!("{function}()") },
    };
    Some(target)
}

fn simple(pattern: &Regex, lines: &[&str], line: usize) -> Option<MethodTarget> {
    let found = pattern.captures(line_at(lines, line))?;
    Some(MethodTarget { name: found[1].to_owned(), label: format!("{}()", &found[1]) })
}

/// The method declared on `line` (0-based) of `text`, named for `language`'s debug adapter.
pub fn method_at(language: &str, text: &str, line: u32) -> Option<MethodTarget> {
    let lines: Vec<&str> = text.split('\n').collect();
    let line = line as usize;
    match language {
        "java" => java_method(&lines, line),
        "kotlin" => simple(&KOTLIN_FUN, &lines, line),
        "python" => simple(&PYTHON_DEF, &lines, line),
        "go" => go_method(&lines, line),
        "rust" => simple(&RUST_FN, &lines, line),
        "javascript" | "typescript" | "javascriptreact" | "typescriptreact" => {
            simple(&JS_FUNCTION, &lines, line).or_else(|| simple(&JS_METHOD, &lines, line))
        }
        "c" | "cpp" | "objective-c" | "objective-cpp" => {
            simple(&C_FUNCTION, &lines, line).filter(|target| !CONTROL.contains(&target.name.as_str()))
        }
        "swift" => simple(&SWIFT_FUNC, &lines, line),
        _ => None,
    }
}

/// The field declared on `line` (0-based) of `text` as a persistent watchpoint id (Java only).
pub fn field_at(language: &str, text: &str, line: u32) -> Option<FieldTarget> {
    if language != "java" {
        return None;
    }
    let lines: Vec<&str> = text.split('\n').collect();
    java_field(&lines, line as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    const JAVA: &str = r#"package com.shop;

import java.util.List;

public class Orders {
    private int count = 0;
    static final String NAME = "x"; // not a { brace

    public Orders() {
        count = 1;
    }

    @Override
    public List<String> load(int id) {
        int local = 2;
        helper(local);
        return List.of();
    }

    static class Line {
        long total;
        void add(long n) { total += n; }
    }
}
"#;

    fn method(name: &str, label: &str) -> Option<MethodTarget> {
        Some(MethodTarget { name: name.to_owned(), label: label.to_owned() })
    }

    fn field(data_id: &str, label: &str) -> Option<FieldTarget> {
        Some(FieldTarget { data_id: data_id.to_owned(), label: label.to_owned() })
    }

    fn name_of(language: &str, text: &str, line: u32) -> Option<String> {
        method_at(language, text, line).map(|target| target.name)
    }

    #[test]
    fn names_java_methods_the_way_java_debug_expects() {
        assert_eq!(method_at("java", JAVA, 13), method("com.shop.Orders#load", "Orders.load()"));
        assert_eq!(method_at("java", JAVA, 8), method("com.shop.Orders#<init>", "Orders.Orders()"));
        assert_eq!(method_at("java", JAVA, 21), method("com.shop.Orders$Line#add", "Line.add()"));
        // Calls and statements are not declarations.
        assert_eq!(method_at("java", JAVA, 14), None);
        assert_eq!(method_at("java", JAVA, 15), None);
        assert_eq!(method_at("java", JAVA, 16), None);
    }

    #[test]
    fn finds_java_fields_not_locals_as_watchpoint_ids() {
        assert_eq!(field_at("java", JAVA, 5), field("com.shop.Orders#count", "Orders.count"));
        assert_eq!(field_at("java", JAVA, 6), field("com.shop.Orders#NAME", "Orders.NAME"));
        assert_eq!(field_at("java", JAVA, 20), field("com.shop.Orders$Line#total", "Line.total"));
        assert_eq!(field_at("java", JAVA, 14), None);
        assert_eq!(field_at("java", JAVA, 13), None);
        assert_eq!(field_at("python", "x = 1", 0), None);
    }

    #[test]
    fn names_functions_for_debugpy_delve_lldb_and_js_debug() {
        let python = "class A:\n    async def run(self, n):\n        pass";
        assert_eq!(name_of("python", python, 1).as_deref(), Some("run"));
        let go = concat!(
            "package shop\n\nfunc Parse(s string) int {\n}\nfunc (o *Order) Total() int {\n}\n",
            "func (o Order) Name() string {\n}",
        );
        assert_eq!(name_of("go", go, 2).as_deref(), Some("shop.Parse"));
        assert_eq!(name_of("go", go, 4).as_deref(), Some("shop.(*Order).Total"));
        assert_eq!(name_of("go", go, 6).as_deref(), Some("shop.Order.Name"));
        assert_eq!(name_of("rust", "pub(crate) async fn parse(text: &str) -> i32 {", 0).as_deref(), Some("parse"));
        assert_eq!(name_of("c", "static int parse(const char *text) {", 0).as_deref(), Some("parse"));
        assert_eq!(method_at("c", "    if (x) {", 0), None);
        assert_eq!(method_at("c", "    total += parse(buf);", 0), None);
        let load = "export async function load(id: number) {";
        assert_eq!(name_of("typescript", load, 0).as_deref(), Some("load"));
        assert_eq!(method_at("plaintext", "def x():", 0), None);
    }

    // Not from a spec: comments and literals hide their braces and keywords.
    #[test]
    fn ignores_braces_in_comments_and_literals() {
        let mut block = false;
        assert_eq!(code("a /* { */ b \"}\" // {", &mut block), "a  b \"\" ");
        assert_eq!(code("x /* open", &mut block), "x ");
        assert!(block);
        assert_eq!(code("still */ y '\\''", &mut block), " y ''");
        assert!(!block);
    }
}
