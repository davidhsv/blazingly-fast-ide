//! Declaration templates per language and literal typing for the syntax-only Introduce Variable
//! / Constant. Only what is certain from the text: no type inference beyond literals and plain
//! constructor calls.

use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

const TS_JS: [&str; 4] = ["typescript", "typescriptreact", "javascript", "javascriptreact"];

pub fn is_ts_js(language: &str) -> bool {
    TS_JS.contains(&language)
}

/// Facts about the project and the file a template depends on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeclContext {
    /// Java: language level 10 or later (`var`).
    pub java_var: bool,
    /// C: C23 (`auto`).
    pub c_auto: bool,
    /// TS/JS: whether the file ends statements with `;` (`None`: unknown, as with them).
    pub semicolons: Option<bool>,
    /// An explicit type for languages that need one (Java without `var`, C without `auto`).
    pub type_name: Option<String>,
}

/// A declaration to insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declaration {
    pub text: String,
    /// The UTF-16 offset of the variable's name in `text`.
    pub name_offset: u32,
    /// How occurrences refer to the variable (`$name` in PHP).
    pub reference: String,
    /// The UTF-16 offset of the name in `reference`.
    pub reference_name_offset: u32,
    /// `text` ends with a statement terminator.
    pub terminated: bool,
}

/// A declaration, or why the language cannot have one without a language server.
pub type DeclResult = Result<Declaration, String>;

fn utf16(text: &str) -> u32 {
    text.encode_utf16().count() as u32
}

fn build(prefix: &str, name: &str, middle: &str, suffix: &str, sigil: &str) -> Declaration {
    Declaration {
        text: format!("{prefix}{sigil}{name}{middle}{suffix}"),
        name_offset: utf16(prefix) + utf16(sigil),
        reference: format!("{sigil}{name}"),
        reference_name_offset: utf16(sigil),
        terminated: suffix.ends_with(';'),
    }
}

/// `T name`, with C's `char *name` spacing.
fn typed(type_name: &str) -> String {
    if type_name.ends_with('*') { type_name.to_owned() } else { format!("{type_name} ") }
}

fn semicolon(cx: &DeclContext) -> &'static str {
    if cx.semicolons == Some(false) { "" } else { ";" }
}

/// A variable declaration `name = expr` in the language.
pub fn variable_declaration(language: &str, name: &str, expr: &str, cx: &DeclContext) -> DeclResult {
    let v = |prefix: &str, op: &str, suffix: &str| -> DeclResult {
        Ok(build(prefix, name, &format!(" {op} {expr}"), suffix, ""))
    };
    match language {
        "java" if cx.java_var => v("var ", "=", ";"),
        "java" => match &cx.type_name {
            Some(type_name) => v(&typed(type_name), "=", ";"),
            None => Err("the type of the expression is unknown and `var` needs Java 10+ \
                         (set maven.compiler.release / sourceCompatibility, or wait for the language server)"
                .to_owned()),
        },
        "csharp" => v("var ", "=", ";"),
        "kotlin" | "scala" => v("val ", "=", ""),
        "swift" => v("let ", "=", ""),
        "rust" => v("let ", "=", ";"),
        "go" => v("", ":=", ""),
        "python" | "ruby" | "elixir" => v("", "=", ""),
        "lua" => v("local ", "=", ""),
        "php" => Ok(build("", name, &format!(" = {expr}"), ";", "$")),
        "cpp" => v("auto ", "=", ";"),
        "c" if cx.c_auto => v("auto ", "=", ";"),
        "c" => match &cx.type_name {
            Some(type_name) => v(&typed(type_name), "=", ";"),
            None => Err("the type of the expression is unknown (`auto` needs C23)".to_owned()),
        },
        "dart" => v("final ", "=", ";"),
        "zig" => v("const ", "=", ";"),
        "groovy" => v("def ", "=", ""),
        _ if is_ts_js(language) => v("const ", "=", semicolon(cx)),
        _ => Err(format!("not supported for {language} without the language server")),
    }
}

/// A constant declaration `NAME = expr` in the language.
pub fn constant_declaration(language: &str, name: &str, expr: &str, cx: &DeclContext) -> DeclResult {
    let ok = |prefix: &str, suffix: &str| -> DeclResult { Ok(build(prefix, name, &format!(" = {expr}"), suffix, "")) };
    match language {
        "java" => match &cx.type_name {
            Some(type_name) => ok(&format!("private static final {}", typed(type_name)), ";"),
            None => Err("a constant needs a type; only literals are typed without the language server".to_owned()),
        },
        "python" => ok("", ""),
        "kotlin" => ok("private const val ", ""),
        _ if is_ts_js(language) => ok("const ", semicolon(cx)),
        _ => Err(format!("Introduce Constant is not supported for {language} without the language server")),
    }
}

/// Languages whose block-level expression statements discard their value.
const STATEMENT_VALUE_DISCARDED: [&str; 10] =
    ["java", "csharp", "python", "go", "c", "cpp", "php", "dart", "lua", "zig"];

/// Whether an expression statement that is exactly the extracted expression may be replaced by
/// the declaration (IDEA: `foo();` becomes `var x = foo();`). Not where a trailing expression is
/// the block's value (Rust tail expressions, Kotlin, Scala, Ruby, Elixir, Swift blocks).
pub fn replaces_bare_statement(language: &str, statement_kind: &str) -> bool {
    if language == "rust" {
        return statement_kind == "expression_statement";
    }
    STATEMENT_VALUE_DISCARDED.contains(&language) || is_ts_js(language)
}

static JAVA_INT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(0[xX][0-9a-fA-F_]+|0[bB][01_]+|[0-9][0-9_]*)([lL]?)$"));
static JAVA_FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    pattern(r"^([0-9][0-9_]*\.?[0-9_]*([eE][+-]?[0-9]+)?|\.[0-9][0-9_]*([eE][+-]?[0-9]+)?)([fFdD]?)$")
});
static NEGATION: LazyLock<Regex> = LazyLock::new(|| pattern(r"^-\s*"));
static NEGATIVE_NUMBER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^-\s*[0-9.]"));
static UNARY_KIND: LazyLock<Regex> = LazyLock::new(|| pattern(r"unary|prefix"));
static PYTHON_FORMAT: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(^|["'\s])[rRbBuU]*[fF][rRbB]*["']"#));
static KOTLIN_LITERAL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^(number|float|character|integer|real|hex|bin|long|unsigned)_literal$"));
static C_LONG: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(0[xX][0-9a-fA-F]+|[0-9]+)[uU]?[lL]{1,2}[uU]?$"));
static C_INT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^(0[xX][0-9a-fA-F]+|[0-9]+)$"));

fn numeric_type(text: &str, int_types: [&'static str; 2], float_types: [&'static str; 2]) -> Option<&'static str> {
    let t = NEGATION.replace(text, "");
    if let Some(captures) = JAVA_INT.captures(&t) {
        return Some(if captures[2].is_empty() { int_types[0] } else { int_types[1] });
    }
    if JAVA_FLOAT.is_match(&t) && t.contains(['.', 'e', 'E', 'f', 'F', 'd', 'D']) {
        return Some(if t.ends_with(['f', 'F']) { float_types[1] } else { float_types[0] });
    }
    None
}

/// Whether `kind`/`text` is a literal (or a negated number) whose value depends on nothing in
/// scope. Interpolating strings are not.
pub fn is_literal(language: &str, kind: &str, text: &str) -> bool {
    let t = text.trim();
    let negative_number = NEGATIVE_NUMBER.is_match(t) && UNARY_KIND.is_match(kind);
    match language {
        "java" => {
            negative_number
                || (kind.ends_with("_literal") && kind != "null_literal")
                || kind == "true"
                || kind == "false"
        }
        "python" => {
            negative_number
                || match kind {
                    "string" | "concatenated_string" => !PYTHON_FORMAT.is_match(t),
                    _ => ["integer", "float", "true", "false"].contains(&kind),
                }
        }
        "kotlin" => {
            negative_number
                || match kind {
                    "string_literal" => !t.contains('$'),
                    "identifier" | "boolean_literal" => t == "true" || t == "false",
                    _ => KOTLIN_LITERAL.is_match(kind),
                }
        }
        "c" | "cpp" => {
            negative_number || ["string_literal", "char_literal", "number_literal", "true", "false"].contains(&kind)
        }
        _ if is_ts_js(language) => {
            negative_number
                || match kind {
                    "template_string" => !t.contains("${"),
                    _ => ["string", "number", "true", "false", "regex"].contains(&kind),
                }
        }
        _ => false,
    }
}

/// The Java or C type of a literal, or `None` when it has none (or is not a literal).
pub fn literal_type(language: &str, kind: &str, text: &str) -> Option<&'static str> {
    if !is_literal(language, kind, text) {
        return None;
    }
    let t = text.trim();
    match language {
        "java" => match kind {
            "string_literal" => Some("String"),
            "character_literal" => Some("char"),
            "true" | "false" => Some("boolean"),
            _ => numeric_type(t, ["int", "long"], ["double", "float"]),
        },
        "c" => match kind {
            "string_literal" => Some("const char *"),
            "char_literal" => Some("char"),
            "true" | "false" => None,
            _ => {
                let n = NEGATION.replace(t, "");
                if C_LONG.is_match(&n) {
                    if n.contains(['u', 'U']) {
                        None
                    } else if n.contains("ll") || n.contains("LL") || n.contains("lL") || n.contains("Ll") {
                        Some("long long")
                    } else {
                        Some("long")
                    }
                } else if C_INT.is_match(&n) {
                    Some("int")
                } else {
                    numeric_type(&n, ["int", "long"], ["double", "float"])
                }
            }
        },
        _ => None,
    }
}

static JAVA_ARRAY: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^new\s+([A-Za-z0-9_$.]+)\s*((?:\[[^\]]*\]\s*)+)(\{(?s:.)*\})?$"));
static JAVA_OBJECT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^new\s+([A-Za-z0-9_$.]+(?:\s*<[A-Za-z0-9_$.,\s?<>\[\]]+>)?)\s*\("));
static WHITESPACE: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s+"));

/// The Java type of a plain constructor call or array creation (`new Foo(…)`, `new int[3]`);
/// `None` with `<>`, a class body or a chained call.
pub fn java_constructed_type(text: &str) -> Option<String> {
    let t = text.trim();
    if let Some(captures) = JAVA_ARRAY.captures(t) {
        return Some(format!("{}{}", &captures[1], "[]".repeat(captures[2].matches('[').count())));
    }
    let captures = JAVA_OBJECT.captures(t)?;
    if !t.ends_with(')') {
        return None;
    }
    // The argument list must be the one closing the expression.
    let mut depth = 0i32;
    let start = captures.get(0).map_or(0, |whole| whole.end()) - 1;
    for (i, byte) in t.bytes().enumerate().skip(start) {
        if byte == b'(' {
            depth += 1;
        } else if byte == b')' {
            depth -= 1;
            if depth == 0 && i != t.len() - 1 {
                return None;
            }
        }
    }
    Some(WHITESPACE.replace_all(&captures[1], "").into_owned())
}

static HOVER_CODE: LazyLock<Regex> = LazyLock::new(|| pattern(r"```(?:java)?\n((?s:.)*?)```"));
static OWNER: LazyLock<Regex> = LazyLock::new(|| pattern(r"\s*[A-Za-z0-9_$.]+\.$"));
static MODIFIERS: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^((public|protected|private|static|final|abstract|synchronized|native|default)\s+)+"));
static TYPE_PARAMETERS: LazyLock<Regex> = LazyLock::new(|| pattern(r"^<[^>]*>\s*"));
static JAVA_TYPE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^[A-Za-z0-9_$.]+(\s*<[A-Za-z0-9_$.,\s?<>\[\]]*>)?(\[\])*$"));

/// The type of a Java expression from a jdtls hover (```` ```java\nString java.lang.String.trim()\n``` ````
/// or `String name`), for a call or name ending in `name`. `None` when unclear.
pub fn java_type_from_hover(hover: &str, name: &str) -> Option<String> {
    let code = HOVER_CODE.captures(hover).map_or(hover, |captures| captures.get(1).map_or(hover, |m| m.as_str()));
    let line = code.split('\n').find(|line| !line.trim().is_empty())?.trim();
    let at = pattern(&format!(r"(^|[\s.]){}\s*(\(|$|\s|-)", regex::escape(name)));
    let found = at.captures(line)?;
    let end = found.get(0).map_or(0, |whole| whole.start()) + found.get(1).map_or(0, |prefix| prefix.len());
    let before = line[..end].trim();
    let before = OWNER.replace(before, "");
    let before = before.trim();
    let before = MODIFIERS.replace(before, "");
    let before = TYPE_PARAMETERS.replace(&before, "").into_owned();
    if before.is_empty() || before == "void" {
        return None;
    }
    JAVA_TYPE.is_match(&before).then_some(before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(language: &str, cx: DeclContext) -> String {
        variable_declaration(language, "n", "e()", &cx).map_or_else(|reason| reason, |decl| decl.text)
    }

    fn constant(language: &str, cx: DeclContext) -> String {
        constant_declaration(language, "N", "1", &cx).map_or_else(|reason| reason, |decl| decl.text)
    }

    fn typed_cx(type_name: &str) -> DeclContext {
        DeclContext { type_name: Some(type_name.to_owned()), ..DeclContext::default() }
    }

    #[test]
    fn declares_variables_per_language() {
        let none = DeclContext::default;
        assert_eq!(decl("java", DeclContext { java_var: true, ..none() }), "var n = e();");
        assert_eq!(decl("java", typed_cx("String")), "String n = e();");
        assert!(decl("java", none()).contains("Java 10"));
        assert_eq!(decl("csharp", none()), "var n = e();");
        assert_eq!(decl("kotlin", none()), "val n = e()");
        assert_eq!(decl("typescript", DeclContext { semicolons: Some(true), ..none() }), "const n = e();");
        assert_eq!(decl("javascript", DeclContext { semicolons: Some(false), ..none() }), "const n = e()");
        assert_eq!(decl("python", none()), "n = e()");
        assert_eq!(decl("rust", none()), "let n = e();");
        assert_eq!(decl("go", none()), "n := e()");
        assert_eq!(decl("swift", none()), "let n = e()");
        assert_eq!(decl("scala", none()), "val n = e()");
        assert_eq!(decl("ruby", none()), "n = e()");
        assert_eq!(decl("lua", none()), "local n = e()");
        assert_eq!(decl("elixir", none()), "n = e()");
        assert_eq!(decl("cpp", none()), "auto n = e();");
        assert!(decl("c", none()).contains("C23"));
        assert_eq!(decl("c", DeclContext { c_auto: true, ..none() }), "auto n = e();");
        assert_eq!(decl("c", typed_cx("const char *")), "const char *n = e();");
        assert_eq!(decl("dart", none()), "final n = e();");
        assert!(decl("haskell", none()).contains("not supported"));
        let php = variable_declaration("php", "n", "e()", &none()).expect("PHP declares variables");
        assert_eq!(
            (php.text.as_str(), php.name_offset, php.reference.as_str(), php.reference_name_offset),
            ("$n = e();", 1, "$n", 1)
        );
    }

    #[test]
    fn declares_constants_per_language() {
        assert_eq!(constant("java", typed_cx("int")), "private static final int N = 1;");
        assert!(constant("java", DeclContext::default()).contains("needs a type"));
        assert_eq!(constant("typescript", DeclContext::default()), "const N = 1;");
        assert_eq!(constant("python", DeclContext::default()), "N = 1");
        assert_eq!(constant("kotlin", DeclContext::default()), "private const val N = 1");
        assert!(constant("go", DeclContext::default()).contains("not supported"));
    }

    #[test]
    fn replaces_bare_statements_only_where_they_discard_their_value() {
        assert!(replaces_bare_statement("java", "expression_statement"));
        assert!(replaces_bare_statement("python", "expression_statement"));
        assert!(replaces_bare_statement("rust", "expression_statement"));
        assert!(!replaces_bare_statement("rust", "call_expression"));
        assert!(!replaces_bare_statement("kotlin", "call_expression"));
        assert!(!replaces_bare_statement("ruby", "call"));
    }

    #[test]
    fn types_literals_and_plain_constructions() {
        assert_eq!(literal_type("java", "string_literal", "\"x\""), Some("String"));
        assert_eq!(literal_type("java", "decimal_integer_literal", "10L"), Some("long"));
        assert_eq!(literal_type("java", "hex_integer_literal", "0xFF"), Some("int"));
        assert_eq!(literal_type("java", "decimal_floating_point_literal", "1.5f"), Some("float"));
        assert_eq!(literal_type("java", "decimal_floating_point_literal", "1e3"), Some("double"));
        assert_eq!(literal_type("java", "unary_expression", "-1"), Some("int"));
        assert_eq!(literal_type("java", "character_literal", "'c'"), Some("char"));
        assert_eq!(literal_type("java", "true", "true"), Some("boolean"));
        assert_eq!(literal_type("java", "null_literal", "null"), None);
        assert_eq!(literal_type("java", "binary_expression", "\"a\" + \"b\""), None);
        assert_eq!(literal_type("c", "string_literal", "\"x\""), Some("const char *"));
        assert_eq!(literal_type("c", "number_literal", "3UL"), None);
        assert_eq!(java_constructed_type("new StringBuilder(\"a\")").as_deref(), Some("StringBuilder"));
        assert_eq!(java_constructed_type("new HashMap<String, Integer>()").as_deref(), Some("HashMap<String,Integer>"));
        assert_eq!(java_constructed_type("new ArrayList<>()"), None);
        assert_eq!(java_constructed_type("new Runnable() { public void run() {} }"), None);
        assert_eq!(java_constructed_type("new Foo().bar()"), None);
        assert_eq!(java_constructed_type("new int[3]").as_deref(), Some("int[]"));
    }

    #[test]
    fn recognises_literals_that_do_not_depend_on_scope() {
        assert!(!is_literal("typescript", "template_string", "`a${b}`"));
        assert!(is_literal("typescript", "template_string", "`ab`"));
        assert!(!is_literal("typescript", "identifier", "x"));
        assert!(!is_literal("python", "string", "f\"{x}\""));
        assert!(is_literal("python", "string", "\"x\""));
        assert!(is_literal("python", "unary_operator", "-1"));
        assert!(!is_literal("kotlin", "string_literal", "\"a$b\""));
        assert!(is_literal("kotlin", "identifier", "true"));
    }

    #[test]
    fn reads_a_java_type_from_a_jdtls_hover() {
        assert_eq!(
            java_type_from_hover("```java\nString java.lang.String.trim()\n```", "trim").as_deref(),
            Some("String")
        );
        assert_eq!(
            java_type_from_hover("```java\njava.util.List<java.lang.String> a.B.names()\n```", "names").as_deref(),
            Some("java.util.List<java.lang.String>")
        );
        assert_eq!(java_type_from_hover("```java\nint count - a.B.f()\n```", "count").as_deref(), Some("int"));
        assert_eq!(java_type_from_hover("```java\nvoid a.B.run()\n```", "run"), None);
        assert_eq!(java_type_from_hover("something else", "x"), None);
    }
}
