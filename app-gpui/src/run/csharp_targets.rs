//! C# test targets: methods carrying an xUnit, NUnit or MSTest attribute, and the classes holding them,
//! named as the test runner names them (`Namespace.Outer+Nested.Method`).

use crate::run::targets::{RunTarget, TargetKind};
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the C# patterns are valid")
}

/// Comments and literals.
static MASKED: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"(?s:/\*.*?\*/)|//[^\n]*|@"(?:""|[^"])*"|"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'"#));
/// An attribute list naming a test attribute.
static TEST_ATTRIBUTE: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"(?:^|[.,\s])(?:Fact|Theory|Test|TestCase|TestCaseSource|TestMethod|DataTestMethod)",
        r"(?:Attribute)?(?-u:\b)",
    ))
});

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// What the scanner recognises at a position of the masked source.
enum Token<'a> {
    /// `namespace A.B;` (file-scoped) or `namespace A.B {`.
    Namespace {
        name: &'a str,
        file_scoped: bool,
    },
    /// `class C … {` (also `record`, `struct`).
    Class(&'a str),
    /// `[…]`: the text between the brackets.
    Attributes(&'a str),
    /// `Name(…)` followed by a body (`{` or `=>`), which is not part of the token.
    Method(&'a str),
    Open,
    Close,
    Semicolon,
}

/// The length of the leading run of `rest` whose characters satisfy `keep`.
fn run(rest: &str, keep: impl Fn(char) -> bool) -> usize {
    rest.find(|c: char| !keep(c)).unwrap_or(rest.len())
}

/// The token starting at the beginning of `rest` and its length, if one does. `boundary`: a word may
/// start here (the character before is not part of one).
fn token_at(rest: &str, boundary: bool) -> Option<(Token<'_>, usize)> {
    let first = rest.chars().next()?;
    match first {
        '{' => return Some((Token::Open, 1)),
        '}' => return Some((Token::Close, 1)),
        ';' => return Some((Token::Semicolon, 1)),
        '[' => {
            let end = rest.find(']')?;
            return Some((Token::Attributes(&rest[1..end]), end + 1));
        }
        _ if !boundary || !is_word(first) => return None,
        _ => {}
    }
    let word = run(rest, is_word);
    let after = &rest[word..];
    let space = run(after, char::is_whitespace);
    if &rest[..word] == "namespace" && space > 0 {
        let name = &after[space..];
        let length = run(name, |c| is_word(c) || c == '.');
        let tail = &name[length..];
        let end = run(tail, char::is_whitespace);
        if length > 0
            && let Some(close @ (';' | '{')) = tail[end..].chars().next()
        {
            let token = Token::Namespace { name: &name[..length], file_scoped: close == ';' };
            return Some((token, word + space + length + end + 1));
        }
    }
    if matches!(&rest[..word], "class" | "record" | "struct") && space > 0 {
        let name = &after[space..];
        let length = run(name, is_word);
        // The header runs to the first brace or semicolon; only an opening brace makes it a type.
        if length > 0
            && let Some(stop) = name[length..].find([';', '{', '}'])
            && name[length + stop..].starts_with('{')
        {
            return Some((Token::Class(&name[..length]), word + space + length + stop + 1));
        }
    }
    let parameters = after[space..].strip_prefix('(')?;
    let span = parameters.find([';', '{', '}']).unwrap_or(parameters.len());
    // The last closing parenthesis of the span that a body follows.
    let end = parameters[..span].rmatch_indices(')').find_map(|(close, _)| {
        let tail = &parameters[close + 1..];
        let body = &tail[run(tail, char::is_whitespace)..];
        (body.starts_with('{') || body.starts_with("=>")).then(|| close + 1 + run(tail, char::is_whitespace))
    })?;
    Some((Token::Method(&rest[..word]), word + space + 1 + end))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    Namespace,
    Class,
}

struct Scope<'a> {
    name: &'a str,
    depth: i32,
    line: u32,
    kind: ScopeKind,
}

/// The test methods of a C# source and their classes, in source order.
pub fn csharp_targets(text: &str) -> Vec<RunTarget> {
    // Comments and literals are blanked, keeping every line break.
    let code = MASKED.replace_all(text, |masked: &regex::Captures| {
        masked[0].chars().map(|c| if matches!(c, '\r' | '\n') { c } else { ' ' }).collect::<String>()
    });
    let mut scopes: Vec<Scope> = Vec::new();
    let mut out: Vec<RunTarget> = Vec::new();
    let mut suites: Vec<String> = Vec::new();
    let mut depth = 0i32;
    let mut line = 0u32;
    let mut file_namespace = "";
    let mut test = false;
    let mut boundary = true;
    let mut at = 0;
    while at < code.len() {
        let rest = &code[at..];
        let Some((token, length)) = token_at(rest, boundary) else {
            let c = rest.chars().next().unwrap_or(' ');
            line += u32::from(c == '\n');
            boundary = !is_word(c);
            at += c.len_utf8();
            continue;
        };
        match token {
            Token::Namespace { name, file_scoped: true } => file_namespace = name,
            Token::Namespace { name, file_scoped: false } => {
                depth += 1;
                scopes.push(Scope { name, depth, line, kind: ScopeKind::Namespace });
            }
            Token::Class(name) => {
                depth += 1;
                scopes.push(Scope { name, depth, line, kind: ScopeKind::Class });
                test = false;
            }
            Token::Attributes(attributes) => test = test || TEST_ATTRIBUTE.is_match(attributes),
            Token::Method(name) => {
                let classes: Vec<&Scope> = scopes.iter().filter(|scope| scope.kind == ScopeKind::Class).collect();
                if test && let Some(owner) = classes.last() {
                    let namespaces = scopes.iter().filter(|scope| scope.kind == ScopeKind::Namespace);
                    let nested = classes.iter().map(|scope| scope.name).collect::<Vec<_>>().join("+");
                    let class_name = std::iter::once(file_namespace)
                        .chain(namespaces.map(|scope| scope.name))
                        .chain([nested.as_str()])
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(".");
                    if !suites.contains(&class_name) {
                        suites.push(class_name.clone());
                        out.push(RunTarget::new(owner.line, TargetKind::Suite, owner.name, vec![class_name.clone()]));
                    }
                    out.push(RunTarget::new(line, TargetKind::Test, name, vec![class_name, name.to_string()]));
                }
                test = false;
            }
            Token::Open => depth += 1,
            Token::Close => {
                depth -= 1;
                while scopes.last().is_some_and(|scope| scope.depth > depth) {
                    scopes.pop();
                }
                test = false;
            }
            Token::Semicolon => test = false,
        }
        let consumed = &rest[..length];
        line += consumed.matches('\n').count() as u32;
        boundary = !consumed.chars().next_back().is_some_and(is_word);
        at += length;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "namespace Demo.Tests;
public class Calculator {
 // [Fact] public void NotATest() {}
 [Xunit.Fact]
 public void Adds() { var s = \"}\"; }
 [Theory, InlineData(1)]
 public async Task Values(int n) { }
 public class Nested {
   [TestMethod] public void Child() {}
 }
}";

    fn lines_and_paths(targets: &[RunTarget]) -> Vec<(u32, Vec<&str>)> {
        targets.iter().map(|t| (t.line, t.path.iter().map(String::as_str).collect())).collect()
    }

    #[test]
    fn tracks_lines_across_multiline_tokens_and_separate_test_classes() {
        let text = "namespace Demo {
/* a comment
   across lines */
class First {
  [Fact]
  void One(
  ) {}
}
class Second {
  [TestCase(
    1)]
  void Two(int value) {}
}
}";
        assert_eq!(
            lines_and_paths(&csharp_targets(text)),
            [
                (3, vec!["Demo.First"]),
                (5, vec!["Demo.First", "One"]),
                (8, vec!["Demo.Second"]),
                (11, vec!["Demo.Second", "Two"]),
            ]
        );
    }

    #[test]
    fn finds_qualified_and_multiline_attributes_nested_types_and_skips_literals_and_comments() {
        let targets = csharp_targets(SOURCE);
        let tests: Vec<RunTarget> = targets.into_iter().filter(|t| t.kind == TargetKind::Test).collect();
        assert_eq!(
            lines_and_paths(&tests),
            [
                (4, vec!["Demo.Tests.Calculator", "Adds"]),
                (6, vec!["Demo.Tests.Calculator", "Values"]),
                (8, vec!["Demo.Tests.Calculator+Nested", "Child"]),
            ]
        );
        let nested = csharp_targets("namespace A { namespace B { class C { [Test] void T() {} } } }");
        assert_eq!(nested.last().map(|t| t.path.clone()), Some(vec!["A.B.C".to_string(), "T".to_string()]));
    }

    // Not from a spec: expression-bodied tests count, calls and declarations without a body do not.
    #[test]
    fn a_method_needs_a_body() {
        let text = "class T {\n [Fact] void A() => Run(1);\n [Fact] abstract void B();\n void C() { [Fact] D(); }\n}";
        let names: Vec<String> = csharp_targets(text).into_iter().map(|t| t.name).collect();
        assert_eq!(names, ["T", "A"]);
    }
}
