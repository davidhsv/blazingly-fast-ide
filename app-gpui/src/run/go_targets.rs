//! Go run targets: `func main` of package main, `Test…` / `Benchmark…` / `Example…` / `Fuzz…`
//! functions with their `t.Run` subtests, and `//go:generate` directives. Comments and literal
//! contents are ignored when matching declaration and callback boundaries.

use crate::run::targets::{RunTarget, TargetKind};
use regex::{Captures, Regex};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

struct Token {
    text: String,
    line: u32,
    /// The value of a string literal.
    literal: Option<String>,
}

/// A subtest's name as `go test -run` matches it.
fn go_subtest_name(name: &str) -> String {
    name.chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect()
}

/// The value of a Go string literal (raw or interpreted), given with its quotes.
fn string_value(text: &str) -> String {
    static ESCAPE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?s)\\(U[0-9a-fA-F]{8}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|[0-7]{3}|.)")
            .expect("the escape pattern is valid")
    });
    let mut chars = text.chars();
    let quote = chars.next();
    chars.next_back();
    let inner = chars.as_str();
    if quote == Some('`') {
        return inner.replace('\r', "");
    }
    ESCAPE
        .replace_all(inner, |found: &Captures| {
            let escape = &found[1];
            let code = |radix| u32::from_str_radix(&escape[1..], radix).ok().and_then(char::from_u32);
            match escape.as_bytes()[0] {
                b'U' | b'u' | b'x' if escape.len() > 1 => code(16).map_or(escape.to_string(), String::from),
                b'0'..=b'7' if escape.len() == 3 => {
                    let octal = u32::from_str_radix(escape, 8).ok().and_then(char::from_u32);
                    octal.map_or(escape.to_string(), String::from)
                }
                b'a' => "\x07".to_string(),
                b'b' => "\x08".to_string(),
                b'f' => "\x0c".to_string(),
                b'n' => "\n".to_string(),
                b'r' => "\r".to_string(),
                b't' => "\t".to_string(),
                b'v' => "\x0b".to_string(),
                _ => escape.to_string(),
            }
        })
        .into_owned()
}

struct Scanner {
    tokens: Vec<Token>,
    /// Opening bracket → its closing bracket, by token index.
    pairs: HashMap<usize, usize>,
    /// Names the `testing` package is imported as (`.` for a dot import).
    aliases: HashSet<String>,
    out: Vec<RunTarget>,
}

impl Scanner {
    fn text(&self, at: usize) -> Option<&str> {
        self.tokens.get(at).map(|token| token.text.as_str())
    }

    /// The name of the `*testing.<type>` parameter between the parentheses at `start` and `end`
    /// (empty when it is unnamed); `None` when the parameters are something else.
    fn parameter(&self, start: usize, end: usize, type_name: &str) -> Option<String> {
        let params: Vec<&Token> =
            self.tokens.get(start + 1..end).unwrap_or_default().iter().filter(|token| token.text != ",").collect();
        let star = params.iter().position(|token| token.text == "*").filter(|star| *star <= 1)?;
        let tail: Vec<&str> = params[star + 1..].iter().map(|token| token.text.as_str()).collect();
        let qualified = tail.len() == 3 && self.aliases.contains(tail[0]) && tail[1] == "." && tail[2] == type_name;
        let dot_import = self.aliases.contains(".") && tail.len() == 1 && tail[0] == type_name;
        (qualified || dot_import).then(|| if star == 1 { params[0].text.clone() } else { String::new() })
    }

    fn add(&mut self, line: u32, path: Vec<String>) {
        if !self.out.iter().any(|target| target.line == line && target.path == path) {
            self.out.push(RunTarget::new(line, TargetKind::Test, path.join("/"), path));
        }
    }

    /// The `receiver.Run(name, func…)` subtests between the tokens `start` and `end`, under `path`.
    fn subtests(&mut self, start: usize, end: usize, path: &[String], receiver: &str, type_name: &str) {
        if receiver.is_empty() {
            return;
        }
        let mut i = start;
        while i < end {
            let run_call = self.text(i) == Some(receiver)
                && self.text(i + 1) == Some(".")
                && self.text(i + 2) == Some("Run")
                && self.text(i + 3) == Some("(");
            let (Some(close), Some(arg), true) = (self.pairs.get(&(i + 3)).copied(), self.tokens.get(i + 4), run_call)
            else {
                i += 1;
                continue;
            };
            let mut names: Vec<(u32, String)> = Vec::new();
            if let Some(literal) = &arg.literal {
                names.push((self.tokens[i].line, literal.clone()));
            } else {
                // A table of cases: named by a field (`tc.name`), or keyed by name (`for name := range`).
                let field = if self.text(i + 5) == Some(".") { self.text(i + 6) } else { None };
                let before = &self.tokens[start.min(i)..i];
                let loop_variable = |n: usize| before.get(n + 1).is_some_and(|next| next.text == arg.text);
                let keyed = field.is_none()
                    && before.iter().enumerate().any(|(n, token)| token.text == "for" && loop_variable(n));
                for n in start..end.min(self.tokens.len()) {
                    let value = &self.tokens[n];
                    let colon = self.text(n + 1) == Some(":");
                    let named = self.tokens.get(n + 2).and_then(|next| next.literal.as_ref());
                    if let (Some(field), true, Some(name)) = (field, colon, named)
                        && value.text == field
                    {
                        names.push((value.line, name.clone()));
                    }
                    if let (true, Some(name), true) = (keyed && colon, &value.literal, self.text(n + 2) == Some("{")) {
                        names.push((value.line, name.clone()));
                    }
                }
            }
            let mut paths: Vec<Vec<String>> = Vec::new();
            for (line, name) in names {
                let mut child = path.to_vec();
                child.extend(go_subtest_name(&name).split('/').map(str::to_string));
                self.add(line, child.clone());
                paths.push(child);
            }
            let callback = (i + 5..close).find(|at| self.text(*at) == Some("func"));
            let params_end = callback.and_then(|callback| self.pairs.get(&(callback + 1)).copied());
            let body_end = params_end.and_then(|params_end| self.pairs.get(&(params_end + 1)).copied());
            if let (Some(callback), Some(params_end), Some(body_end)) = (callback, params_end, body_end)
                && let Some(child_receiver) = self.parameter(callback + 1, params_end, type_name)
            {
                for child in &paths {
                    self.subtests(params_end + 2, body_end, child, &child_receiver, type_name);
                }
            }
            i = close + 1;
        }
    }
}

/// The run targets of a Go source, by line.
pub fn go_targets(text: &str) -> Vec<RunTarget> {
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r#"\s+|//[^\n]*|(?s:/\*.*?\*/)|"(?:[^"\\]|\\.)*"|`[^`]*`|'(?:[^'\\]|\\.)*'"#,
            r"|[\p{L}_][\p{L}\p{N}_]*|:=|\S",
        ))
        .expect("the Go token pattern is valid")
    });
    let mut scanner = Scanner { tokens: Vec::new(), pairs: HashMap::new(), aliases: HashSet::new(), out: Vec::new() };
    let mut line = 0u32;
    for found in TOKEN.find_iter(text) {
        let word = found.as_str();
        let directive = word.strip_prefix("//go:generate").filter(|rest| {
            let command = rest.trim_start_matches([' ', '\t']);
            command.len() < rest.len() && command.chars().next().is_some_and(|c| !c.is_whitespace())
        });
        let line_start = found.start() == 0 || text[..found.start()].ends_with('\n');
        if let Some(command) = directive.filter(|_| line_start) {
            scanner.out.push(RunTarget::new(line, TargetKind::Generate, command.trim(), vec![line.to_string()]));
        }
        let skipped = word.starts_with("//") || word.starts_with("/*") || word.starts_with(char::is_whitespace);
        if !skipped {
            let literal = (word.starts_with('"') || word.starts_with('`')).then(|| string_value(word));
            scanner.tokens.push(Token { text: word.to_string(), line, literal });
        }
        line += word.matches('\n').count() as u32;
    }
    let mut stack: Vec<usize> = Vec::new();
    for (i, token) in scanner.tokens.iter().enumerate() {
        match token.text.as_str() {
            "(" | "{" | "[" => stack.push(i),
            ")" | "}" | "]" => {
                if let Some(start) = stack.pop() {
                    scanner.pairs.insert(start, i);
                }
            }
            _ => {}
        }
    }
    scanner.aliases.insert("testing".to_string());
    for (i, token) in scanner.tokens.iter().enumerate() {
        if token.literal.as_deref() == Some("testing")
            && let Some(previous) = i.checked_sub(1).map(|before| scanner.tokens[before].text.as_str())
            && !matches!(previous, "import" | "(")
        {
            scanner.aliases.insert(previous.to_string());
        }
    }
    let main = scanner.tokens.windows(2).any(|pair| pair[0].text == "package" && pair[1].text == "main");
    let mut i = 0;
    while i < scanner.tokens.len() {
        let declaration = (scanner.text(i) == Some("func") && scanner.text(i + 2) == Some("("))
            .then(|| scanner.pairs.get(&(i + 2)).copied())
            .flatten()
            .filter(|params_end| scanner.text(params_end + 1) == Some("{"))
            .and_then(|params_end| Some((params_end, scanner.pairs.get(&(params_end + 1)).copied()?)));
        let Some((params_end, end)) = declaration else {
            i += 1;
            continue;
        };
        let name = scanner.tokens[i + 1].text.clone();
        let line = scanner.tokens[i].line;
        let prefix = ["Test", "Benchmark", "Example", "Fuzz"]
            .into_iter()
            .find_map(|prefix| Some((prefix, name.strip_prefix(prefix)?)))
            .filter(|(_, rest)| name != "TestMain" && !rest.chars().next().is_some_and(char::is_lowercase));
        if let Some((prefix, _)) = prefix {
            let type_name = match prefix {
                "Benchmark" => "B",
                "Fuzz" => "F",
                _ => "T",
            };
            let receiver = scanner.parameter(i + 2, params_end, type_name);
            let declared = if prefix == "Example" { params_end == i + 3 } else { receiver.is_some() };
            if declared {
                scanner.add(line, vec![name.clone()]);
                if matches!(prefix, "Test" | "Benchmark") {
                    scanner.subtests(params_end + 2, end, &[name], &receiver.unwrap_or_default(), type_name);
                }
            }
        } else if main && name == "main" && params_end == i + 3 {
            scanner.out.push(RunTarget::new(line, TargetKind::Main, "main", Vec::new()));
        }
        i = end + 1;
    }
    scanner.out.sort_by_key(|target| target.line);
    scanner.out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tests(lines: &[&str]) -> Vec<(u32, String)> {
        let targets = go_targets(&lines.join("\n"));
        targets.into_iter().filter(|t| t.kind == TargetKind::Test).map(|t| (t.line, t.name)).collect()
    }

    fn named(found: &[(u32, &str)]) -> Vec<(u32, String)> {
        found.iter().map(|(line, name)| (*line, name.to_string())).collect()
    }

    // Not from a spec: test functions need the testing parameter, and literal subtests nest by name.
    #[test]
    fn finds_tests_benchmarks_examples_and_literal_subtests() {
        let found = tests(&[
            "package calc",
            "import \"testing\"",
            "func TestAdd(t *testing.T) {",
            "\tt.Run(\"small numbers\", func(t *testing.T) {",
            "\t\tt.Run(\"a/b\", func(t *testing.T) {})",
            "\t})",
            "}",
            "func Testhelper(t *testing.T) {}",
            "func TestMain(m *testing.M) {}",
            "func BenchmarkAdd(b *testing.B) {}",
            "func ExampleAdd() {}",
            "func TestNoParam() {}",
        ]);
        let expected = [
            (2, "TestAdd"),
            (3, "TestAdd/small_numbers"),
            (4, "TestAdd/small_numbers/a/b"),
            (9, "BenchmarkAdd"),
            (10, "ExampleAdd"),
        ];
        assert_eq!(found, named(&expected));
    }

    // Not from a spec: table-driven subtests are named by the field the Run call reads, or by the map key.
    #[test]
    fn finds_table_driven_subtests() {
        let by_field = tests(&[
            "package calc",
            "import tt \"testing\"",
            "func TestTable(t *tt.T) {",
            "\tcases := []struct{ name string }{",
            "\t\t{name: \"one\"},",
            "\t\t{name: `two`},",
            "\t}",
            "\tfor _, tc := range cases {",
            "\t\tt.Run(tc.name, func(t *tt.T) {})",
            "\t}",
            "}",
        ]);
        assert_eq!(by_field, named(&[(2, "TestTable"), (4, "TestTable/one"), (5, "TestTable/two")]));
        let by_key = tests(&[
            "package calc",
            "import \"testing\"",
            "func TestKeys(t *testing.T) {",
            "\tcases := map[string]struct{}{",
            "\t\t\"first\": {},",
            "\t}",
            "\tfor name := range cases {",
            "\t\tt.Run(name, func(t *testing.T) {})",
            "\t}",
            "}",
        ]);
        assert_eq!(by_key, named(&[(2, "TestKeys"), (4, "TestKeys/first")]));
    }

    // Not from a spec: main needs package main; a directive needs a command and the start of a line.
    #[test]
    fn finds_main_and_generate_directives() {
        let text = [
            "package main",
            "//go:generate stringer -type=Pill",
            "// go:generate ignored",
            "//go:generate   ",
            "func main() {",
            "\ts := \"func main() {\"",
            "}",
        ]
        .join("\n");
        let found: Vec<(u32, TargetKind, String)> =
            go_targets(&text).into_iter().map(|t| (t.line, t.kind, t.name)).collect();
        let expected =
            [(1, TargetKind::Generate, "stringer -type=Pill".to_string()), (4, TargetKind::Main, "main".to_string())];
        assert_eq!(found, expected);
        assert!(go_targets("package tools\nfunc main() {}\n").is_empty());
        assert_eq!(string_value("\"a\\tb\\u00e9\\x41\\101\\q\""), "a\tbéAAq");
    }
}
