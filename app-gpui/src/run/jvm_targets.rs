//! Java / Kotlin run targets: `main` methods (the class to launch) and JUnit 4/5 / TestNG test methods
//! and classes, with their binary class names (`pkg.Outer$Inner`).
//!
//! A small token scanner rather than per-line patterns, so annotations on earlier lines (with
//! arguments), multi-line signatures, nested and `@Nested` classes, Kotlin objects / companions and
//! comments or strings are handled. Lines are 0-based, columns UTF-16.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JvmTargetKind {
    Main,
    Test,
    Suite,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmTarget {
    pub line: u32,
    pub kind: JvmTargetKind,
    /// Binary name of the class (`com.acme.Outer$Inner`).
    pub class_name: String,
    /// Test method name (tests only).
    pub method: Option<String>,
    /// The class is a `@SpringBootApplication`.
    pub spring: bool,
    /// Java 25 instance `main` (JEP 512): a subclass cannot inherit it on older JDKs.
    pub instance: bool,
}

/// A class with `extends X` and no `main` of its own: it may inherit one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmMainCandidate {
    /// Position of the class name.
    pub line: u32,
    pub col: u32,
    pub class_name: String,
    /// The superclass as written (`a.b.Base`), so an answer lapses when the `extends` clause changes.
    pub base: String,
    pub spring: bool,
}

#[derive(Clone, Debug)]
struct Token {
    text: String,
    line: u32,
    col: u32,
}

/// Blanks comments, strings, text blocks and char literals, keeping every line and column where it is
/// (an unterminated literal swallows its line break, as the string it would be does).
pub fn strip_comments_and_strings(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let n = chars.len();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, from: usize, to: usize| {
        for c in chars.get(from..to).unwrap_or_default() {
            if *c == '\n' {
                out.push('\n');
            } else {
                out.extend(std::iter::repeat_n(' ', c.len_utf16()));
            }
        }
    };
    let find = |needle: &[char], from: usize| {
        (from..n.saturating_sub(needle.len() - 1)).find(|at| chars[*at..].starts_with(needle))
    };
    let mut i = 0;
    while i < n {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            let stop = find(&['\n'], i).unwrap_or(n);
            blank(&mut out, i, stop);
            i = stop;
        } else if c == '/' && next == Some('*') {
            let stop = find(&['*', '/'], i + 2).map_or(n, |end| end + 2);
            blank(&mut out, i, stop);
            i = stop;
        } else if c == '"' && chars[i..].starts_with(&['"', '"', '"']) {
            let stop = find(&['"', '"', '"'], i + 3).map_or(n, |end| end + 3);
            out.push_str("\"\"");
            blank(&mut out, i + 2, stop);
            i = stop;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            while j < n && chars[j] != c && chars[j] != '\n' {
                j += if chars[j] == '\\' { 2 } else { 1 };
            }
            let stop = n.min(j + 1);
            out.push(c);
            blank(&mut out, i + 1, stop - 1);
            out.push(if chars[stop - 1] == c { c } else { ' ' });
            i = stop;
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

fn tokenize(code: &str) -> Vec<Token> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"`[^`\n]+`|[A-Za-z_$][A-Za-z0-9_$]*|[0-9][A-Za-z0-9_.]*|\S").expect("the token pattern is valid")
    });
    let mut tokens = Vec::new();
    for (line, text) in code.split('\n').enumerate() {
        let (mut byte, mut col) = (0, 0u32);
        for found in TOKEN.find_iter(text) {
            col += text[byte..found.start()].chars().map(|c| c.len_utf16() as u32).sum::<u32>();
            byte = found.start();
            tokens.push(Token { text: found.as_str().to_string(), line: line as u32, col });
        }
    }
    tokens
}

const TEST_ANNOTATIONS: [&str; 5] = ["Test", "ParameterizedTest", "RepeatedTest", "TestFactory", "TestTemplate"];
const TYPE_KEYWORDS: [&str; 5] = ["class", "interface", "enum", "record", "object"];
const NOT_TYPES: [&str; 10] = ["new", "return", "throw", "else", "case", "assert", "yield", "await", "this", "super"];
/// Tokens a Kotlin declaration starts with: a class without a body ends where the next one starts.
const KOTLIN_DECL: [&str; 15] = [
    "fun",
    "val",
    "var",
    "class",
    "object",
    "interface",
    "enum",
    "data",
    "sealed",
    "abstract",
    "open",
    "private",
    "public",
    "internal",
    "@",
];
const MEMBER_END: [&str; 4] = [";", "{", "}", ")"];

fn is_ident(token: Option<&str>) -> bool {
    token.and_then(|t| t.chars().next()).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '$' | '`'))
}

fn unquote(token: &str) -> &str {
    let token = token.strip_prefix('`').unwrap_or(token);
    token.strip_suffix('`').unwrap_or(token)
}

/// TestNG methods that set up or feed tests; not tests themselves under a class-level `@Test`.
fn testng_not_test(annotation: &str) -> bool {
    let word = |rest: &str| rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    annotation.strip_prefix("Before").or_else(|| annotation.strip_prefix("After")).is_some_and(word)
        || matches!(annotation, "DataProvider" | "Factory")
}

#[derive(Clone, Debug)]
struct ClassFrame {
    name: String,
    /// Brace depth inside the class body.
    depth: usize,
    line: u32,
    annotations: Vec<String>,
    kind: String,
    companion: bool,
    /// Java `class … extends X` that may inherit a `main` (top-level or `static` nested): the column
    /// of the class name and the superclass.
    inherits_from: Option<(u32, String)>,
}

impl ClassFrame {
    fn has(&self, annotation: &str) -> bool {
        self.annotations.iter().any(|own| own == annotation)
    }
}

/// Targets of a Java (`kotlin = false`) or Kotlin source file.
pub fn scan_jvm_targets(src: &str, rel_path: &str, kotlin: bool) -> Vec<JvmTarget> {
    scan(src, rel_path, kotlin).0
}

/// Java classes that extend another class but declare no `main`.
pub fn jvm_main_candidates(src: &str, rel_path: &str) -> Vec<JvmMainCandidate> {
    scan(src, rel_path, false).1
}

fn package_of(code: &str) -> &str {
    static PACKAGE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^\s*package\s+([A-Za-z0-9_.]+)").expect("the package pattern is valid"));
    PACKAGE.captures(code).and_then(|found| found.get(1)).map_or("", |package| package.as_str())
}

fn binary_name(frames: &[ClassFrame]) -> String {
    frames.iter().map(|frame| frame.name.as_str()).collect::<Vec<_>>().join("$")
}

fn scan(src: &str, rel_path: &str, kotlin: bool) -> (Vec<JvmTarget>, Vec<JvmMainCandidate>) {
    static STRING_ARGS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(java\s*\.\s*lang\s*\.\s*)?String\s*(\[\s*\]|\.\s*\.\s*\.)").expect("the main pattern is valid")
    });
    static KOTLIN_ARGS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^[A-Za-z0-9_]+\s*:\s*Array\s*<\s*(out\s+)?String\s*>$").expect("the main pattern is valid")
    });
    static JVM_NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"@file:JvmName\(\s*"([^"]+)"\s*\)"#).expect("the facade name pattern is valid"));
    let code = strip_comments_and_strings(src);
    let package = package_of(&code);
    let qualify = |binary: String| if package.is_empty() { binary } else { format!("{package}.{binary}") };
    let tokens = tokenize(&code);
    let text = |at: usize| tokens.get(at).map(|token| token.text.as_str());
    let mut out: Vec<JvmTarget> = Vec::new();
    let mut candidates: Vec<JvmMainCandidate> = Vec::new();
    let mut classes: Vec<ClassFrame> = Vec::new();
    // A class header seen, waiting for its `{`.
    let mut pending: Option<ClassFrame> = None;
    let mut annotations: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut parens = 0usize;
    // Test classes in the order their first test was found, with the line of the class.
    let mut test_classes: Vec<(String, u32)> = Vec::new();

    let mut i = 0;
    while i < tokens.len() {
        'token: {
            let t = tokens[i].text.as_str();
            let prev = i.checked_sub(1).and_then(text);
            if t == "(" {
                parens += 1;
            } else if t == ")" {
                parens = parens.saturating_sub(1);
            }
            if parens > 0 && t != "(" {
                break 'token;
            }
            // A Kotlin class without a body ends where the next declaration starts.
            let later_line = pending.as_ref().is_some_and(|class| tokens[i].line > class.line);
            if kotlin && later_line && KOTLIN_DECL.contains(&t) {
                pending = None;
            }

            if t == "@" {
                let name = text(i + 1);
                // `@interface Foo` declares an annotation type.
                if name == Some("interface") {
                    break 'token;
                }
                // Qualified annotation (`@org.junit.Test`, `@file:JvmName`): keep the last segment.
                let mut j = i + 1;
                let mut last = name.unwrap_or_default();
                while matches!(text(j + 1), Some("." | ":")) {
                    last = text(j + 2).unwrap_or(last);
                    j += 2;
                }
                annotations.push(last.to_string());
                i = j;
                if text(i + 1) == Some("(") {
                    // Skip the arguments.
                    let mut nesting = 0usize;
                    i += 1;
                    while i < tokens.len() {
                        match tokens[i].text.as_str() {
                            "(" => nesting += 1,
                            ")" => {
                                nesting = nesting.saturating_sub(1);
                                if nesting == 0 {
                                    break;
                                }
                            }
                            _ => {}
                        }
                        i += 1;
                    }
                }
                break 'token;
            }

            if TYPE_KEYWORDS.contains(&t) && prev != Some(".") {
                // `companion object [Name]`, `object Foo`, `class Foo`, `enum class Foo`.
                let companion = t == "object" && prev == Some("companion");
                let next = text(i + 1);
                let name = match next {
                    Some(next) if is_ident(Some(next)) && !TYPE_KEYWORDS.contains(&next) => Some(unquote(next)),
                    _ if companion => Some("Companion"),
                    _ => None,
                };
                if let Some(name) = name {
                    let base = if !kotlin && t == "class" { extends_clause(&tokens, i) } else { None };
                    let inherits = base.filter(|_| classes.is_empty() || has_modifier(&tokens, i, "static"));
                    pending = Some(ClassFrame {
                        name: name.to_string(),
                        depth: 0,
                        line: tokens[i].line,
                        annotations: std::mem::take(&mut annotations),
                        kind: t.to_string(),
                        companion,
                        inherits_from: inherits.map(|base| (tokens.get(i + 1).map_or(0, |token| token.col), base)),
                    });
                }
                break 'token;
            }

            if t == "{" {
                if let Some(mut frame) = pending.take() {
                    frame.depth = depth + 1;
                    classes.push(frame);
                    if let Some(frame) = classes.last()
                        && let Some((col, base)) = &frame.inherits_from
                    {
                        candidates.push(JvmMainCandidate {
                            line: frame.line,
                            col: *col,
                            class_name: qualify(binary_name(&classes)),
                            base: base.clone(),
                            spring: frame.has("SpringBootApplication"),
                        });
                    }
                }
                depth += 1;
                annotations.clear();
                break 'token;
            }
            if t == "}" {
                depth = depth.saturating_sub(1);
                while classes.last().is_some_and(|class| class.depth > depth) {
                    classes.pop();
                }
                annotations.clear();
                pending = None;
                break 'token;
            }
            if t == ";" {
                annotations.clear();
                if !kotlin {
                    pending = None;
                }
                break 'token;
            }

            // Method declarations: `<type> name(` (Java) or `fun [<T>] [Receiver.]name(` (Kotlin),
            // directly in a class body (or at file level for Kotlin top-level functions).
            let at_member_level = classes.last().map_or(depth == 0, |class| depth == class.depth);
            if text(i + 1) != Some("(") || !is_ident(Some(t)) || !at_member_level || pending.is_some() {
                break 'token;
            }
            let is_method = if kotlin {
                let mut found = false;
                for k in (i.saturating_sub(8)..i).rev() {
                    let before = tokens[k].text.as_str();
                    if before == "fun" {
                        found = true;
                        break;
                    }
                    if !matches!(before, "." | ">" | "<" | ",") && !is_ident(Some(before)) {
                        break;
                    }
                }
                found
            } else {
                let type_name = |prev: &str| is_ident(Some(prev)) && !NOT_TYPES.contains(&prev);
                prev.is_some_and(|prev| matches!(prev, ">" | "]") || type_name(prev))
            };
            if !is_method {
                break 'token;
            }
            let name = unquote(t);
            // Parameters (until the matching `)`).
            let mut params: Vec<&str> = Vec::new();
            let mut nesting = 0usize;
            for token in &tokens[i + 1..] {
                match token.text.as_str() {
                    "(" => nesting += 1,
                    ")" if nesting <= 1 => break,
                    ")" => {
                        nesting -= 1;
                        params.push(")");
                    }
                    other => params.push(other),
                }
            }
            let modifiers: Vec<&str> = tokens[..i]
                .iter()
                .rev()
                .map(|token| token.text.as_str())
                .take_while(|token| !MEMBER_END.contains(token))
                .collect();
            let owner = classes.last();
            // TestNG `@Test` on a class (JUnit's is method-only): its public `void` methods are tests.
            let class_level_test = !kotlin
                && owner.is_some_and(|owner| owner.kind == "class" && owner.has("Test"))
                && prev == Some("void")
                && modifiers.contains(&"public")
                && !modifiers.contains(&"static")
                && !annotations.iter().any(|annotation| testng_not_test(annotation));
            let annotated_test = annotations.iter().any(|annotation| TEST_ANNOTATIONS.contains(&annotation.as_str()));
            if (annotated_test || class_level_test) && owner.is_some() {
                out.push(JvmTarget {
                    line: tokens[i].line,
                    kind: JvmTargetKind::Test,
                    class_name: qualify(binary_name(&classes)),
                    method: Some(name.to_string()),
                    spring: false,
                    instance: false,
                });
                for count in (1..=classes.len()).rev() {
                    let class_name = qualify(binary_name(&classes[..count]));
                    if !test_classes.iter().any(|(known, _)| *known == class_name) {
                        test_classes.push((class_name, classes[count - 1].line));
                    }
                    // Outer classes only run nested ones when those are `@Nested`.
                    if !classes[count - 1].has("Nested") {
                        break;
                    }
                }
            } else if name == "main" {
                params.retain(|param| *param != "final");
                let param_text = params.join(" ");
                let string_args = param_text.is_empty() || STRING_ARGS.is_match(&param_text);
                let kotlin_args = param_text.is_empty() || KOTLIN_ARGS.is_match(&param_text);
                let main = |class_name: String, spring: bool, instance: bool| JvmTarget {
                    line: tokens[i].line,
                    kind: JvmTargetKind::Main,
                    class_name,
                    method: None,
                    spring,
                    instance,
                };
                if kotlin && kotlin_args {
                    match owner {
                        None => {
                            let facade = match JVM_NAME.captures(src).and_then(|found| found.get(1)) {
                                Some(name) => name.as_str().to_string(),
                                None => facade_name(rel_path),
                            };
                            out.push(main(qualify(facade), false, false));
                        }
                        Some(owner) => {
                            let jvm_static = annotations.iter().any(|annotation| annotation == "JvmStatic");
                            // Companion members become statics of the enclosing class.
                            let launch = if owner.companion { &classes[..classes.len() - 1] } else { &classes[..] };
                            if (owner.kind == "object" || owner.companion)
                                && jvm_static
                                && let Some(class) = launch.last()
                            {
                                let spring = class.has("SpringBootApplication");
                                out.push(main(qualify(binary_name(launch)), spring, false));
                            }
                        }
                    }
                } else if !kotlin
                    && let Some(owner) = owner
                    && prev == Some("void")
                    && string_args
                {
                    let is_static = modifiers.contains(&"static");
                    // Java 25 instance main methods (JEP 512) need no `static` / arguments.
                    if is_static || owner.kind == "class" || owner.kind == "record" {
                        let spring = classes.iter().any(|class| class.has("SpringBootApplication"));
                        out.push(main(qualify(binary_name(&classes)), spring, !is_static));
                    }
                }
            }
            annotations.clear();
        }
        i += 1;
    }

    let suites = test_classes.into_iter().map(|(class_name, line)| JvmTarget {
        line,
        kind: JvmTargetKind::Suite,
        class_name,
        method: None,
        spring: false,
        instance: false,
    });
    // One main per class is enough (overloads).
    let mut mains: HashSet<String> = HashSet::new();
    let unique: Vec<JvmTarget> =
        out.into_iter().filter(|t| t.kind != JvmTargetKind::Main || mains.insert(t.class_name.clone())).collect();
    let mut targets: Vec<JvmTarget> = suites.chain(unique).collect();
    targets.sort_by_key(|target| target.line);
    candidates.retain(|candidate| !mains.contains(&candidate.class_name));
    (targets, candidates)
}

fn has_modifier(tokens: &[Token], at: usize, modifier: &str) -> bool {
    let before = tokens[..at].iter().rev().map(|token| token.text.as_str());
    before.take_while(|token| !MEMBER_END.contains(token)).any(|token| token == modifier)
}

/// The class a Kotlin file's top-level functions compile to: `server.kt` → `ServerKt`.
fn facade_name(rel_path: &str) -> String {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = file.strip_suffix(".kts").or_else(|| file.strip_suffix(".kt")).unwrap_or(file);
    let mut chars = stem.chars();
    let first: String = chars.next().into_iter().flat_map(char::to_uppercase).collect();
    format!("{first}{}Kt", chars.as_str())
}

/// The superclass named by the `extends` clause of the `class` at `at` (not a type-parameter bound).
fn extends_clause(tokens: &[Token], at: usize) -> Option<String> {
    let mut angle = 0i32;
    let in_header = |token: &Token| !matches!(token.text.as_str(), "{" | ";");
    for (k, token) in tokens.iter().enumerate().skip(at + 2).take_while(|(_, token)| in_header(token)) {
        match token.text.as_str() {
            "<" => angle += 1,
            ">" => angle -= 1,
            "extends" if angle == 0 => {
                let base: String = tokens[k + 1..]
                    .iter()
                    .map(|token| token.text.as_str())
                    .take_while(|token| !matches!(*token, "<" | "{" | "implements" | "permits"))
                    .collect();
                return (!base.is_empty()).then_some(base);
            }
            _ => {}
        }
    }
    None
}

/// Binary name of a Java file's first top-level type: the class the source launcher (`java Hi.java`) runs.
pub fn first_top_level_type(src: &str) -> Option<String> {
    let code = strip_comments_and_strings(src);
    let package = package_of(&code);
    let tokens = tokenize(&code);
    let mut depth = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        match token.text.as_str() {
            "{" => depth += 1,
            "}" => depth = depth.saturating_sub(1),
            keyword if depth == 0 && TYPE_KEYWORDS.contains(&keyword) => {
                let after_dot = i.checked_sub(1).is_some_and(|before| tokens[before].text == ".");
                let name = tokens.get(i + 1).map(|next| next.text.as_str());
                if !after_dot && is_ident(name) {
                    let name = name.unwrap_or_default();
                    return Some(if package.is_empty() { name.to_string() } else { format!("{package}.{name}") });
                }
            }
            _ => {}
        }
    }
    None
}

/// Simple name of a binary class name (`a.b.Outer$Inner` → `Inner`).
pub fn simple_class_name(class_name: &str) -> &str {
    class_name.rsplit(['.', '$']).next().unwrap_or(class_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Picked<'a> = (u32, JvmTargetKind, &'a str, Option<&'a str>, bool);

    fn pick(targets: &[JvmTarget]) -> Vec<Picked<'_>> {
        targets.iter().map(|t| (t.line, t.kind, t.class_name.as_str(), t.method.as_deref(), t.spring)).collect()
    }

    use JvmTargetKind::{Main, Suite, Test};

    #[test]
    fn finds_a_spring_boot_main_with_annotations_on_earlier_lines() {
        let src = [
            "package com.example.demo;",
            "",
            "import org.springframework.boot.SpringApplication;",
            "",
            "@SpringBootApplication(",
            "    scanBasePackages = \"com.example\" // not a ) here",
            ")",
            "public class SpringDemoApplication {",
            "  /* public static void main(String[] args) in a comment */",
            "  static String s = \"void main(String[] a) {\";",
            "  public static void",
            "  main(",
            "      final String... args) {",
            "    SpringApplication.run(SpringDemoApplication.class, args);",
            "  }",
            "}",
        ]
        .join("\n");
        let targets = scan_jvm_targets(&src, "src/main/java/com/example/demo/SpringDemoApplication.java", false);
        assert_eq!(pick(&targets), [(11, Main, "com.example.demo.SpringDemoApplication", None, true)]);
    }

    #[test]
    fn ignores_non_main_methods_named_main_and_java_25_instance_mains_count() {
        let src = "class A {\n  int main(String[] a) { return 0; }\n  static void main(int x) {}\n}";
        assert!(scan_jvm_targets(src, "A.java", false).is_empty());
        let targets = scan_jvm_targets("class B {\n  void main() {}\n}", "B.java", false);
        assert_eq!(pick(&targets), [(1, Main, "B", None, false)]);
    }

    #[test]
    fn finds_junit_tests_nested_classes_and_multi_line_annotations() {
        let src = [
            "package a.b;",
            "class OrderTest {",
            "  @Test",
            "  void plain() {}",
            "  @ParameterizedTest(",
            "      name = \"{0}\")",
            "  @ValueSource(ints = {1, 2})",
            "  void param(int x) {",
            "    Runnable r = new Runnable() { public void run() {} };",
            "  }",
            "  void helper() {}",
            "  @Nested",
            "  class WhenEmpty {",
            "    @org.junit.jupiter.api.Test void fails() {}",
            "  }",
            "  static class Fixture {",
            "    @Test void inner() {}",
            "  }",
            "}",
        ]
        .join("\n");
        let targets = scan_jvm_targets(&src, "src/test/java/a/b/OrderTest.java", false);
        assert_eq!(
            pick(&targets),
            [
                (1, Suite, "a.b.OrderTest", None, false),
                (3, Test, "a.b.OrderTest", Some("plain"), false),
                (7, Test, "a.b.OrderTest", Some("param"), false),
                (12, Suite, "a.b.OrderTest$WhenEmpty", None, false),
                (13, Test, "a.b.OrderTest$WhenEmpty", Some("fails"), false),
                (15, Suite, "a.b.OrderTest$Fixture", None, false),
                (16, Test, "a.b.OrderTest$Fixture", Some("inner"), false),
            ]
        );
    }

    #[test]
    fn treats_the_public_void_methods_of_a_testng_class_level_test_as_tests() {
        let src = [
            "package a.b;",
            "import org.testng.annotations.*;",
            "@Test(groups = \"unit\")",
            "public class PriceTest {",
            "  @BeforeMethod public void setUp() {}",
            "  public void rounds() {}",
            "  @Test(dataProvider = \"rows\")",
            "  public void parses(String row) {}",
            "  @DataProvider public Object[][] rows() { return null; }",
            "  @DataProvider(name = \"more\") public void notATest() {}",
            "  void packagePrivate() {}",
            "  private void util() {}",
            "  public static void shared() {}",
            "  public int value() { return 1; }",
            "}",
        ]
        .join("\n");
        let targets = scan_jvm_targets(&src, "src/test/java/a/b/PriceTest.java", false);
        assert_eq!(
            pick(&targets),
            [
                (3, Suite, "a.b.PriceTest", None, false),
                (5, Test, "a.b.PriceTest", Some("rounds"), false),
                (7, Test, "a.b.PriceTest", Some("parses"), false),
            ]
        );
    }

    #[test]
    fn is_the_first_top_level_class_interface_enum_record_or_annotation_with_the_package() {
        let src = [
            "package demo;",
            "import java.util.List; // class Commented {}",
            "/* enum Old {} */ @Tag(type = List.class, name = \"class Str\")",
            "interface Shape { class Nested {} }",
            "public class Test { public static void main(String[] args) {} }",
        ]
        .join("\n");
        assert_eq!(first_top_level_type(&src).as_deref(), Some("demo.Shape"));
        assert_eq!(first_top_level_type("record Point(int x) {}\nclass Test {}").as_deref(), Some("Point"));
        assert_eq!(first_top_level_type("@interface Marker {}\nclass Test {}").as_deref(), Some("Marker"));
        assert_eq!(first_top_level_type("void main() {\n  IO.println(1);\n}\n"), None);
    }

    #[test]
    fn top_level_main_runs_the_file_facade_objects_and_companions_with_jvm_static() {
        let server = "package app\n\nfun main(args: Array<String>) {}";
        let targets = scan_jvm_targets(server, "src/main/kotlin/app/server.kt", true);
        assert_eq!(pick(&targets), [(2, Main, "app.ServerKt", None, false)]);
        let named = "@file:JvmName(\"Boot\")\npackage app\nfun main() {}";
        let targets = scan_jvm_targets(named, "src/main/kotlin/app/x.kt", true);
        assert_eq!(pick(&targets), [(2, Main, "app.Boot", None, false)]);
        let companion = [
            "package app",
            "@SpringBootApplication",
            "class Demo",
            "class Tool {",
            "  companion object {",
            "    @JvmStatic",
            "    fun main(args: Array<String>) {}",
            "  }",
            "}",
            "object Cli { @JvmStatic fun main(args: Array<String>) {} }",
        ]
        .join("\n");
        let targets = scan_jvm_targets(&companion, "src/main/kotlin/app/Tool.kt", true);
        assert_eq!(pick(&targets), [(6, Main, "app.Tool", None, false), (9, Main, "app.Cli", None, false)]);
    }

    #[test]
    fn finds_backticked_test_names() {
        let kotlin = "class CalcTest {\n  @Test\n  fun `adds numbers`() {}\n}";
        let targets = scan_jvm_targets(kotlin, "src/test/kotlin/CalcTest.kt", true);
        assert_eq!(
            pick(&targets),
            [(0, Suite, "CalcTest", None, false), (2, Test, "CalcTest", Some("adds numbers"), false)]
        );
    }

    fn candidate_names(src: &str) -> Vec<String> {
        jvm_main_candidates(src, "a/B.java").into_iter().map(|candidate| candidate.class_name).collect()
    }

    #[test]
    fn finds_the_class_from_the_report_at_its_name() {
        let src = "package demo;\n\n@SpringBootApplication\npublic class AiChatApplication extends AtomStarter {}\n";
        let expected = JvmMainCandidate {
            line: 3,
            col: 13,
            class_name: "demo.AiChatApplication".into(),
            base: "AtomStarter".into(),
            spring: true,
        };
        assert_eq!(jvm_main_candidates(src, "a/B.java"), [expected]);
    }

    #[test]
    fn main_candidates_by_class_shape() {
        let cases: [(&str, &str, &[&str]); 10] = [
            ("static nested class", "class A { static class B extends Base {} }", &["A$B"]),
            ("inner (non-static) class", "class A { class B extends Base {} }", &[]),
            ("abstract class", "abstract class A extends Base {}", &["A"]),
            ("own main", "class A extends Base { public static void main(String[] a) {} }", &[]),
            ("implements only", "class A implements Runnable {}", &[]),
            ("interface extends", "interface A extends Base {}", &[]),
            ("enum and record", "enum E implements I {}\nrecord R() implements I {}", &[]),
            ("multi-line generic header", "class A<T extends Number>\n  extends Base<T>\n{}", &["A"]),
            ("type bound only", "class A<T extends Number> {}", &[]),
            (
                "extends in comment and string",
                "// class A extends B\nclass C { String s = \"class D extends E {\"; }",
                &[],
            ),
        ];
        for (case, src, expected) in cases {
            assert_eq!(candidate_names(src), expected, "{case}");
        }
    }

    #[test]
    fn accepts_java_lang_string_and_flags_instance_main() {
        let src = "class A { public static void main(java.lang.String[] a) {} }";
        let qualified = scan_jvm_targets(src, "A.java", false);
        assert_eq!((qualified[0].kind, qualified[0].class_name.as_str(), qualified[0].instance), (Main, "A", false));
        let instance = scan_jvm_targets("class A { void main() {} }", "A.java", false);
        assert_eq!((instance[0].kind, instance[0].instance), (Main, true));
    }

    // Not from a spec: blanking keeps columns, also after characters outside the basic plane.
    #[test]
    fn stripping_keeps_lines_and_utf16_columns() {
        let expected = format!("a{}b{}\n\"{}\" ' '", " ".repeat(10), " ".repeat(5), " ".repeat(4));
        assert_eq!(strip_comments_and_strings("a /* 😀 */ b // c\n\"x\\\"y\" 'c'"), expected);
        assert_eq!(simple_class_name("a.b.Outer$Inner"), "Inner");
    }
}
