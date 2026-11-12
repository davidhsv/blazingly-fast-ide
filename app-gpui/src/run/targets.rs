//! Run targets inside a file, for the gutter ▶ icons and "Run test" code lenses: `fn main`, `#[test]`
//! functions and `mod tests`, JS/TS `test`/`it`/`describe`, pytest `def test_*` / `class Test*`,
//! unittest `TestCase` subclasses, Go `func TestX(t *testing.T)`, JUnit `@Test`, Minitest / Test::Unit
//! classes, modules holding them and `def test_*`. Line-based detection (cheap, no parser), plus the
//! run configuration for one target derived from the project's detected test configurations.

use crate::find::utf16;
use crate::platform::command_line::format_args;
use crate::run::application::{application_config, jvm_test_config};
use crate::run::configs::param;
use crate::run::context::{RunEnv, context_config, phpunit_args, rel_to_cwd, test_config_for};
use crate::run::csharp_targets::csharp_targets;
use crate::run::django_types::{django_label, django_tests_of};
use crate::run::go_arguments::{go_test_parts, without_go_selection};
use crate::run::go_generate::escape_reg_exp;
use crate::run::go_targets::go_targets;
use crate::run::jvm_targets::{JvmTargetKind, scan_jvm_targets, simple_class_name};
use ide_run::RunConfig;
use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetKind {
    Main,
    Test,
    Suite,
    Generate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunTarget {
    /// 0-based line of the declaration.
    pub line: u32,
    pub kind: TargetKind,
    /// Display name (`adds_numbers`, `math > adds`).
    pub name: String,
    /// Test path: Rust module path, describe chain, Python class chain… (framework specific).
    pub path: Vec<String>,
    /// Name pattern contains printf placeholders / template parts (`test.each`).
    pub parameterized: bool,
    /// JVM: binary name of the class (main class, test class).
    pub class_name: Option<String>,
    /// JVM: a `@SpringBootApplication` main class.
    pub spring: bool,
    /// Python: a unittest `TestCase` subclass or one of its methods (what Django's runner can run).
    pub unittest: bool,
    /// Ruby: a `module` suite, which runs the tests of every class declared inside it.
    pub ruby_module: bool,
    pub pest: bool,
}

impl RunTarget {
    pub fn new(line: u32, kind: TargetKind, name: impl Into<String>, path: Vec<String>) -> Self {
        Self {
            line,
            kind,
            name: name.into(),
            path,
            parameterized: false,
            class_name: None,
            spring: false,
            unittest: false,
            ruby_module: false,
            pest: false,
        }
    }
}

/// Above this size detection is skipped (generated / minified files), in UTF-16 code units.
pub const MAX_SCAN: u32 = 400_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Js,
    Python,
    Php,
    Go,
    Jvm,
    Ruby,
    Csharp,
}

pattern!(JS_FILE, r"\.[cm]?[jt]sx?$");
pattern!(JVM_FILE, r"\.(java|kt|kts|scala|groovy)$");

pub fn target_language(rel_path: &str) -> Option<Lang> {
    if rel_path.to_ascii_lowercase().ends_with(".cs") {
        Some(Lang::Csharp)
    } else if rel_path.ends_with(".rs") {
        Some(Lang::Rust)
    } else if JS_FILE.is_match(rel_path) {
        Some(Lang::Js)
    } else if rel_path.ends_with(".py") {
        Some(Lang::Python)
    } else if rel_path.ends_with(".php") {
        Some(Lang::Php)
    } else if rel_path.ends_with(".go") {
        Some(Lang::Go)
    } else if JVM_FILE.is_match(rel_path) {
        Some(Lang::Jvm)
    } else if rel_path.ends_with(".rb") {
        Some(Lang::Ruby)
    } else {
        None
    }
}

/// The width of a line's indentation, a tab counting as four columns.
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

pattern!(LINE_COMMENT, r"//.*$");
pattern!(STRING_LITERAL, r#""(?:[^"\\]|\\.)*""#);
pattern!(CHAR_LITERAL, r"'(?:[^'\\]|\\.)'");

/// Strips string literals and line comments (rough) so braces inside them don't count.
fn code_only(line: &str) -> String {
    let line = LINE_COMMENT.replace(line, "");
    let line = STRING_LITERAL.replace_all(&line, "\"\"");
    CHAR_LITERAL.replace_all(&line, "''").into_owned()
}

pattern!(RUST_SOURCE, r"(?:^|/)src/(.+)\.rs$");
pattern!(
    RUST_TEST_ATTRIBUTE,
    concat!(
        r"^#\[(?:[A-Za-z0-9_:]+::)?test(?-u:\b)",
        r"|^#\[(?:tokio|async_std|actix_rt|rstest|test_case)(?:::[A-Za-z0-9_]+)?(?-u:\b)",
    )
);
pattern!(RUST_FN, r"^(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?(?:unsafe\s+)?fn\s+([A-Za-z0-9_]+)");
pattern!(RUST_MOD, r"^(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z0-9_]+)\s*\{");
pattern!(RUST_MAIN_FILE, r"(?:^|/)(?:src/main|src/bin/[^/]+|examples/[^/]+)\.rs$");

fn rust_module_prefix(rel_path: &str) -> Vec<String> {
    let Some(found) = RUST_SOURCE.captures(rel_path) else { return Vec::new() };
    let mut parts: Vec<&str> = found[1].split('/').collect();
    let last = parts.last().copied().unwrap_or_default();
    if (parts.len() == 1 && matches!(last, "lib" | "main")) || parts[0] == "bin" {
        return Vec::new();
    }
    if last == "mod" {
        parts.pop();
    }
    strings(&parts)
}

fn rust_targets(lines: &[&str], rel_path: &str) -> Vec<RunTarget> {
    let mut out: Vec<RunTarget> = Vec::new();
    let prefix = rust_module_prefix(rel_path);
    // Open `mod` blocks with the brace depth they were opened at.
    let mut mods: Vec<(String, i32)> = Vec::new();
    let mut depth = 0i32;
    let (mut pending_test, mut pending_cfg_test) = (false, false);
    for (i, raw) in lines.iter().enumerate() {
        let line = code_only(raw);
        let trimmed = line.trim();
        if RUST_TEST_ATTRIBUTE.is_match(trimmed) {
            pending_test = true;
        }
        if trimmed.starts_with("#[cfg(test)]") {
            pending_cfg_test = true;
        }
        let module_path = |mods: &[(String, i32)]| {
            prefix.iter().cloned().chain(mods.iter().map(|(name, _)| name.clone())).collect::<Vec<_>>()
        };
        if let Some(function) = RUST_FN.captures(trimmed) {
            let name = &function[1];
            if pending_test {
                let mut path = module_path(&mods);
                path.push(name.to_string());
                out.push(RunTarget::new(i as u32, TargetKind::Test, name, path));
            } else if name == "main" && depth == 0 && RUST_MAIN_FILE.is_match(rel_path) {
                out.push(RunTarget::new(i as u32, TargetKind::Main, "main", Vec::new()));
            }
            pending_test = false;
            pending_cfg_test = false;
        } else if let Some(module) = RUST_MOD.captures(trimmed) {
            let name = &module[1];
            if pending_cfg_test || name == "tests" {
                let mut path = module_path(&mods);
                path.push(name.to_string());
                out.push(RunTarget::new(i as u32, TargetKind::Suite, name, path));
            }
            mods.push((name.to_string(), depth));
            pending_test = false;
            pending_cfg_test = false;
        } else if !trimmed.is_empty() && !trimmed.starts_with("#[") && !trimmed.starts_with("//") {
            pending_test = false;
            if !trimmed.starts_with('#') {
                pending_cfg_test = false;
            }
        }
        for c in line.chars() {
            if c == '{' {
                depth += 1;
            } else if c == '}' {
                depth -= 1;
                while mods.last().is_some_and(|(_, opened)| *opened >= depth) {
                    mods.pop();
                }
            }
        }
    }
    // A suite that contains no tests is noise.
    let holds_tests = |suite: &RunTarget| {
        let inside = |test: &RunTarget| test.path.len() > suite.path.len() && test.path.starts_with(&suite.path);
        out.iter().any(|other| other.kind == TargetKind::Test && inside(other))
    };
    out.iter().filter(|target| target.kind != TargetKind::Suite || holds_tests(target)).cloned().collect()
}

/// The body of a quoted string starting at `rest` (just after the opening `quote`): the text up to the
/// closing quote, a backslash escaping the character after it.
fn quoted_body(rest: &str, quote: char) -> Option<&str> {
    let mut chars = rest.char_indices();
    while let Some((at, c)) = chars.next() {
        if c == '\\' {
            chars.next();
        } else if c == quote {
            return Some(&rest[..at]);
        }
    }
    // A trailing backslash before the last quote is a character of the name, not an escape.
    rest.rfind(quote).map(|at| &rest[..at])
}

/// `text` with every backslash escape replaced by the escaped character.
fn unescape(text: &str, escaped: impl Fn(char) -> bool) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek().copied() {
            Some(next) if c == '\\' && escaped(next) => {
                chars.next();
                out.push(next);
            }
            _ => out.push(c),
        }
    }
    out
}

pattern!(
    JS_CALL,
    concat!(
        r"^(\s*)(describe|suite|context|test|it)(?-u:\b)(?:\.(?:only|skip|todo|concurrent|failing|sequential))*",
        r#"(\.each\s*(?:\(.*?\)|`[^`]*`))?\s*\(\s*(['"`])"#,
    )
);
pattern!(JS_PLACEHOLDER, r"%[sdifjo#%p]|\$[A-Za-z0-9_]+");

fn js_targets(lines: &[&str]) -> Vec<RunTarget> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(call) = JS_CALL.captures(line) else { continue };
        let quote = call[4].chars().next().unwrap_or('"');
        let Some(body) = quoted_body(&line[call.get(0).map_or(0, |head| head.end())..], quote) else { continue };
        let indent = indent_of(&call[1]);
        while stack.last().is_some_and(|(open, _)| *open >= indent) {
            stack.pop();
        }
        let name = unescape(body, |_| true);
        let parameterized =
            call.get(3).is_some() || (quote == '`' && name.contains("${")) || JS_PLACEHOLDER.is_match(&name);
        let mut path: Vec<String> = stack.iter().map(|(_, name)| name.clone()).collect();
        path.push(name.clone());
        let suite = matches!(&call[2], "describe" | "suite" | "context");
        let kind = if suite { TargetKind::Suite } else { TargetKind::Test };
        out.push(RunTarget { parameterized, ..RunTarget::new(i as u32, kind, path.join(" > "), path) });
        if suite {
            stack.push((indent, name));
        }
    }
    out
}

pattern!(
    PHP_CLASS,
    r"^\s*(?:final\s+|abstract\s+|readonly\s+)*class\s+([A-Za-z0-9_]+)(?:\s+extends\s+([A-Za-z0-9_\\]+))?"
);
pattern!(PHP_ABSTRACT, r"^\s*abstract(?-u:\b)");
pattern!(PHP_TEST_MARK, r"#\[\s*(?:\\?PHPUnit\\Framework\\Attributes\\)?Test\s*[\],(]|@test(?-u:\b)");
pattern!(PHP_METHOD, r"^\s*public\s+(?:static\s+)?function\s+([A-Za-z0-9_]+)\s*\(");
pattern!(PHP_MEMBER, r"^\s*(?:public|private|protected)(?-u:\b)");

/// PHPUnit: classes named `*Test` / extending `TestCase`, and `test*` methods or `#[Test]` / `@test` ones.
fn php_targets(lines: &[&str]) -> Vec<RunTarget> {
    let mut out = Vec::new();
    let mut class: Option<String> = None;
    let mut marked = false;
    for (i, line) in lines.iter().enumerate() {
        if let Some(declared) = PHP_CLASS.captures(line) {
            let name = &declared[1];
            let parent = declared.get(2).map_or("", |parent| parent.as_str());
            let test = name.ends_with("Test") || name.ends_with("TestCase") || parent.ends_with("TestCase");
            class = (test && !PHP_ABSTRACT.is_match(line)).then(|| name.to_string());
            if let Some(class) = &class {
                out.push(RunTarget::new(i as u32, TargetKind::Suite, class.clone(), vec![class.clone()]));
            }
            marked = false;
            continue;
        }
        let Some(class) = &class else { continue };
        if PHP_TEST_MARK.is_match(line) {
            marked = true;
            continue;
        }
        let method = PHP_METHOD.captures(line);
        if let Some(method) = method.as_ref().map(|method| &method[1])
            && (marked || method.starts_with("test"))
        {
            let path = vec![class.clone(), method.to_string()];
            out.push(RunTarget::new(i as u32, TargetKind::Test, format!("{class}::{method}"), path));
        }
        if method.is_some() || PHP_MEMBER.is_match(line) {
            marked = false;
        }
    }
    out
}

pattern!(PEST_CALL, r#"^(\s*)(describe|test|it)\s*\(\s*(['"])"#);

fn pest_targets(lines: &[&str]) -> Vec<RunTarget> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(call) = PEST_CALL.captures(line) else { continue };
        let quote = call[3].chars().next().unwrap_or('\'');
        let Some(body) = quoted_body(&line[call.get(0).map_or(0, |head| head.end())..], quote) else { continue };
        let indent = indent_of(&call[1]);
        while stack.last().is_some_and(|(open, _)| *open >= indent) {
            stack.pop();
        }
        let text = unescape(body, |c| if quote == '\'' { "'\\".contains(c) } else { "\"\\$".contains(c) });
        let mut path: Vec<String> = stack.iter().map(|(_, name)| name.clone()).collect();
        let kind = if &call[2] == "describe" {
            path.push(text.clone());
            stack.push((indent, text));
            TargetKind::Suite
        } else {
            path.push(if &call[2] == "it" { format!("it {text}") } else { text });
            TargetKind::Test
        };
        out.push(RunTarget { pest: true, ..RunTarget::new(i as u32, kind, path.join(" → "), path) });
    }
    out
}

pattern!(PYTHON_CLASS, r"^\s*class\s+([A-Za-z0-9_]+)\s*(?:\(([^)]*))?");
pattern!(PYTHON_TEST_CASE, r"TestCase(?-u:\b)");
pattern!(PYTHON_TEST_DEF, r"^\s*(?:async\s+)?def\s+(test[A-Za-z0-9_]*)\s*\(");
pattern!(PYTHON_MAIN, r#"^if\s+__name__\s*==\s*['"]__main__['"]\s*:"#);

struct PythonClass {
    indent: usize,
    name: String,
    test: bool,
    unittest: bool,
}

fn python_targets(lines: &[&str]) -> Vec<RunTarget> {
    let mut out = Vec::new();
    let mut classes: Vec<PythonClass> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = indent_of(line);
        while classes.last().is_some_and(|class| class.indent >= indent) {
            classes.pop();
        }
        let names = |classes: &[PythonClass]| classes.iter().map(|class| class.name.clone()).collect::<Vec<_>>();
        if let Some(class) = PYTHON_CLASS.captures(line) {
            let name = &class[1];
            // pytest collects `Test*` classes; unittest (and Django) any `TestCase` subclass.
            let unittest = class.get(2).is_some_and(|bases| PYTHON_TEST_CASE.is_match(bases.as_str()));
            let test = (name.starts_with("Test") || unittest) && classes.iter().all(|class| class.test);
            classes.push(PythonClass { indent, name: name.to_string(), test, unittest });
            if test {
                let suite = RunTarget::new(i as u32, TargetKind::Suite, name, names(&classes));
                out.push(RunTarget { unittest, ..suite });
            }
            continue;
        }
        if let Some(def) = PYTHON_TEST_DEF.captures(line)
            && classes.iter().all(|class| class.test)
        {
            let mut path = names(&classes);
            path.push(def[1].to_string());
            let unittest = classes.last().is_some_and(|class| class.unittest);
            out.push(RunTarget { unittest, ..RunTarget::new(i as u32, TargetKind::Test, path.join("."), path) });
            continue;
        }
        if indent == 0 && PYTHON_MAIN.is_match(line) {
            out.push(RunTarget::new(i as u32, TargetKind::Main, "main", Vec::new()));
        }
    }
    out
}

pattern!(
    RUBY_DECLARATION,
    r"^\s*(class|module)\s+([A-Z][A-Za-z0-9_]*(?:::[A-Z][A-Za-z0-9_]*)*)(?:\s*<\s*([A-Za-z0-9_:]+))?"
);
pattern!(RUBY_TEST_SUPERCLASS, r"(Test|TestCase|Spec)$");
pattern!(RUBY_TEST_CLASS_NAME, r"Tests?(Case)?$");
pattern!(RUBY_TEST_METHOD, r"^\s*def\s+(test_[A-Za-z0-9_]*[?!]?)");
pattern!(RUBY_TEST_BLOCK, r#"^\s*test\s*\(?\s*(["'])"#);

/// A class that holds Minitest / Test::Unit tests: by its superclass or its name.
fn is_ruby_test_class(name: &str, superclass: Option<&str>) -> bool {
    RUBY_TEST_SUPERCLASS.is_match(superclass.unwrap_or_default())
        || RUBY_TEST_CLASS_NAME.is_match(name.rsplit("::").next().unwrap_or(name))
}

/// The name of the test a line declares: `def test_x`, or ActiveSupport's `test "does x" do`
/// (`test_does_x`).
fn ruby_test_method(line: &str) -> Option<String> {
    if let Some(method) = RUBY_TEST_METHOD.captures(line) {
        return Some(method[1].to_string());
    }
    let block = RUBY_TEST_BLOCK.captures(line)?;
    let rest = &line[block.get(0)?.end()..];
    // At least one character, then the same quote again.
    let first = rest.chars().next()?.len_utf8();
    let end = first + rest[first..].find(&block[1])?;
    let mut name = String::from("test_");
    let mut in_gap = false;
    for c in rest[..end].chars() {
        if !c.is_whitespace() {
            name.push(c);
        } else if !in_gap {
            name.push('_');
        }
        in_gap = c.is_whitespace();
    }
    Some(name)
}

struct RubyFrame {
    indent: usize,
    name: String,
    test: bool,
    /// Index of the frame's suite in the output.
    suite: usize,
}

/// Minitest / Test::Unit: test classes, the modules that hold them (run together) and their
/// `def test_*` methods. Frames are tracked by indentation, as Python's are. Results name a class by
/// its full name (`Outer::InnerTest`).
fn ruby_targets(lines: &[&str]) -> Vec<RunTarget> {
    let mut frames: Vec<RubyFrame> = Vec::new();
    // Each target with whether it is kept: a class or module that holds no tests is noise.
    let mut out: Vec<(RunTarget, bool)> = Vec::new();
    let full_name =
        |frames: &[RubyFrame]| frames.iter().map(|frame| frame.name.as_str()).collect::<Vec<_>>().join("::");
    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = indent_of(line);
        while frames.last().is_some_and(|frame| frame.indent >= indent) {
            frames.pop();
        }
        if let Some(declared) = RUBY_DECLARATION.captures(line) {
            let module = &declared[1] == "module";
            let test = !module && is_ruby_test_class(&declared[2], declared.get(3).map(|parent| parent.as_str()));
            frames.push(RubyFrame { indent, name: declared[2].to_string(), test, suite: out.len() });
            let name = full_name(&frames);
            let suite = RunTarget::new(i as u32, TargetKind::Suite, name.clone(), vec![name]);
            out.push((RunTarget { ruby_module: module, ..suite }, false));
            continue;
        }
        if !frames.last().is_some_and(|owner| owner.test) {
            continue;
        }
        let Some(method) = ruby_test_method(line).filter(|method| method != "test_") else { continue };
        for frame in &frames {
            out[frame.suite].1 = true;
        }
        let class = full_name(&frames);
        out.push((RunTarget::new(i as u32, TargetKind::Test, format!("{class}#{method}"), vec![class, method]), true));
    }
    out.into_iter().filter(|(_, kept)| *kept).map(|(target, _)| target).collect()
}

/// Java / Kotlin: classes and methods with binary class names.
fn java_kotlin_targets(text: &str, rel_path: &str) -> Vec<RunTarget> {
    let kotlin = rel_path.ends_with(".kt") || rel_path.ends_with(".kts");
    scan_jvm_targets(text, rel_path, kotlin)
        .into_iter()
        .map(|found| {
            let simple = simple_class_name(&found.class_name).to_string();
            let class_path = vec![found.class_name.clone()];
            let target = match (found.kind, found.method) {
                (JvmTargetKind::Main, _) => RunTarget::new(found.line, TargetKind::Main, simple, class_path),
                (JvmTargetKind::Suite, _) => RunTarget::new(found.line, TargetKind::Suite, simple, class_path),
                (JvmTargetKind::Test, method) => {
                    let method = method.unwrap_or_default();
                    let path = vec![found.class_name.clone(), method.clone()];
                    RunTarget::new(found.line, TargetKind::Test, format!("{simple}.{method}"), path)
                }
            };
            RunTarget { class_name: Some(found.class_name), spring: found.spring, ..target }
        })
        .collect()
}

pattern!(JUNIT_ANNOTATION, r"^@(?:Test|ParameterizedTest|RepeatedTest|TestFactory)(?-u:\b)");
pattern!(DECLARES_METHOD, r"(?-u:\b)fun(?-u:\b)|(?-u:\b)void(?-u:\b)");
pattern!(KOTLIN_FUN, r"(?-u:\b)fun\s+(?:`([^`]+)`|([A-Za-z0-9_]+))\s*\(");
pattern!(VOID_METHOD, r"(?-u:\b)void\s+([A-Za-z0-9_]+)\s*\(");
pattern!(STATIC, r"(?-u:\b)static(?-u:\b)");
pattern!(TOP_LEVEL_MAIN, r"^fun\s+main\s*\(");

/// Scala / Groovy: line-based (JUnit-style annotations only).
fn jvm_line_targets(lines: &[&str], rel_path: &str) -> Vec<RunTarget> {
    let mut out = Vec::new();
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let class = Regex::new(&format!(r"(?-u:\b)(?:class|object)\s+{}(?-u:\b)", regex::escape(stem))).ok();
    let mut pending_test = false;
    let mut class_line: Option<u32> = None;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if JUNIT_ANNOTATION.is_match(trimmed) {
            pending_test = true;
            // Annotation and declaration on one line fall through.
            if !DECLARES_METHOD.is_match(trimmed) {
                continue;
            }
        }
        if class_line.is_none() && class.as_ref().is_some_and(|class| class.is_match(trimmed)) {
            class_line = Some(i as u32);
        }
        let method = KOTLIN_FUN.captures(trimmed).or_else(|| VOID_METHOD.captures(trimmed));
        if let Some(method) = method {
            let name = method.get(1).or_else(|| method.get(2)).map_or("", |name| name.as_str());
            if pending_test {
                let path = vec![stem.to_string(), name.to_string()];
                out.push(RunTarget::new(i as u32, TargetKind::Test, format!("{stem}.{name}"), path));
            } else if name == "main" && (STATIC.is_match(trimmed) || TOP_LEVEL_MAIN.is_match(trimmed)) {
                out.push(RunTarget::new(i as u32, TargetKind::Main, "main", Vec::new()));
            }
            pending_test = false;
        } else if !trimmed.is_empty() && !trimmed.starts_with('@') && !trimmed.starts_with("//") {
            pending_test = false;
        }
    }
    if let Some(line) = class_line.filter(|_| out.iter().any(|target| target.kind == TargetKind::Test)) {
        out.insert(0, RunTarget::new(line, TargetKind::Suite, stem, vec![stem.to_string()]));
    }
    out
}

pattern!(JS_TEST_FILE, r"\.(test|spec)\.[cm]?[jt]sx?$|(^|/)__tests__/");
pattern!(PYTHON_TEST_FILE, r"(^|/)(test_[^/]*|[^/]*_test)\.py$|(^|/)tests?/");
pattern!(DJANGO_TESTS, r"(^|/)tests\.py$");
pattern!(PHP_TEST_FILE, r"Test\.php$|(^|/)tests?/");
pattern!(JAVA_KOTLIN_FILE, r"\.(java|kts?)$");
pattern!(RUBY_TEST_FILE, r"(^|/)test_[^/]*\.rb$|_test\.rb$|(^|/)tests?/");

/// Run targets of a file (relative path picks the language). `django`: a Django project, where
/// `tests.py` (Django's default test module) holds tests too.
pub fn detect_run_targets(text: &str, rel_path: &str, django: bool) -> Vec<RunTarget> {
    if utf16::len(text) > MAX_SCAN {
        return Vec::new();
    }
    let lines: Vec<&str> = text.split('\n').collect();
    match target_language(rel_path) {
        Some(Lang::Csharp) => csharp_targets(text),
        Some(Lang::Rust) => rust_targets(&lines, rel_path),
        Some(Lang::Js) if JS_TEST_FILE.is_match(rel_path) => js_targets(&lines),
        Some(Lang::Python) => {
            let tests = PYTHON_TEST_FILE.is_match(rel_path) || (django && DJANGO_TESTS.is_match(rel_path));
            python_targets(&lines).into_iter().filter(|target| target.kind == TargetKind::Main || tests).collect()
        }
        Some(Lang::Php) if PHP_TEST_FILE.is_match(rel_path) => {
            let mut targets = php_targets(&lines);
            targets.extend(pest_targets(&lines));
            targets.sort_by_key(|target| target.line);
            targets
        }
        Some(Lang::Go) => {
            let tests = rel_path.ends_with("_test.go");
            let kept = |target: &RunTarget| matches!(target.kind, TargetKind::Main | TargetKind::Generate) || tests;
            go_targets(text).into_iter().filter(kept).collect()
        }
        Some(Lang::Jvm) if JAVA_KOTLIN_FILE.is_match(rel_path) => java_kotlin_targets(text, rel_path),
        Some(Lang::Jvm) => jvm_line_targets(&lines, rel_path),
        // RSpec files (`spec/`) are not Minitest.
        Some(Lang::Ruby) if RUBY_TEST_FILE.is_match(rel_path) => ruby_targets(&lines),
        _ => Vec::new(),
    }
}

pattern!(JS_NAME_PLACEHOLDER, r"%[sdifjo#p]|\\\$\\\{[^}]*\\\}|\$[A-Za-z0-9_]+");

/// Jest / Vitest `-t` pattern for a test (full name = describe chain + name, space separated).
pub fn js_name_pattern(target: &RunTarget) -> String {
    let escaped = escape_reg_exp(&target.path.join(" "));
    // `test.each` placeholders (%s, $name, ${…}) match anything.
    let pattern = JS_NAME_PLACEHOLDER.replace_all(&escaped, ".*");
    if target.kind == TargetKind::Suite { format!("^{pattern}") } else { format!("^{pattern}$") }
}

/// `args` with its last argument replaced by what `change` makes of it.
fn with_last(args: &[String], change: impl Fn(&str) -> String) -> Vec<String> {
    let mut out = args.to_vec();
    if let Some(last) = out.last_mut() {
        *last = change(last);
    }
    out
}

/// `text` as a literal in a PHPUnit `--filter` pattern (which is delimited by `/`).
fn escape_filter(text: &str) -> String {
    escape_reg_exp(text).replace('/', "\\/")
}

pattern!(PHP_CLASS_NAME, r"^\\?[A-Z][A-Za-z0-9_]*(?:\\[A-Za-z0-9_]+)*$");
pattern!(PHP_METHOD_NAME, r"^[a-z_][A-Za-z0-9_]*$");
pattern!(DATA_SET, r" with data set .*$");

/// `--filter` pattern selecting the PHPUnit test at `path` (`[class, method]`, or a class).
/// PHPUnit matches `Class::method with data set …`; a class or suite node selects by its name.
fn phpunit_filter(path: &[String]) -> String {
    let last = path.last().map_or("", String::as_str);
    let data_set = DATA_SET.is_match(last);
    let method = DATA_SET.replace(last, "");
    // A data set sits under a node named after its method (the data-provider suite).
    let class = path.len().checked_sub(if data_set { 3 } else { 2 }).and_then(|at| path.get(at));
    let is_test = |class: &String| {
        PHP_METHOD_NAME.is_match(&method)
            || !PHP_CLASS_NAME.is_match(&method)
            || (PHP_CLASS_NAME.is_match(class) && class.contains('\\'))
    };
    match class.filter(|class| !class.is_empty() && is_test(class)) {
        Some(class) => format!("/({}::{}( with data set .*)?)$/", escape_filter(class), escape_filter(&method)),
        None => format!("/({}::.*)$/", escape_filter(last)),
    }
}

/// `--filter` pattern selecting the Pest tests under `describes`, or the one named `name` there.
fn pest_filter(describes: &[String], name: Option<&str>) -> String {
    let prefix: String = describes.iter().map(|describe| format!("`{describe}` → ")).collect();
    match name {
        None => format!("/::{}/", escape_filter(&prefix)),
        Some(name) => format!("/::{}( with data set .*)?$/", escape_filter(&format!("{prefix}{name}"))),
    }
}

/// Run configuration for one target, or `None` when the project has no matching runner. `base`: the
/// test configuration to narrow (one of the file's test bases); the closest by default.
pub fn target_config(
    target: &RunTarget,
    rel_path: &str,
    configs: &[RunConfig],
    content: Option<&str>,
    base: Option<&RunConfig>,
    env: RunEnv,
) -> Option<RunConfig> {
    if target.kind == TargetKind::Main {
        return match &target.class_name {
            Some(class_name) => Some(application_config(class_name, target.spring, rel_path, env.jvm(configs))),
            None => context_config(rel_path, configs, content, env),
        };
    }
    let file = test_config_for(rel_path, configs, base, content, env)?;
    let last = target.path.last().map_or("", String::as_str);
    if let Some(class_name) = &target.class_name
        && matches!(file.kind.as_str(), "maven" | "gradle")
    {
        return jvm_test_config(&file, class_name, (target.kind == TargetKind::Test).then_some(last));
    }
    let short_name = if target.kind == TargetKind::Suite { last } else { target.name.as_str() };
    let named = |args: Vec<String>| RunConfig {
        id: format!("context:target:{}:{rel_path}:{}", file.id, target.path.join("/")),
        name: short_name.to_string(),
        args,
        detected: true,
        ..file.clone()
    };
    let suite = target.kind == TargetKind::Suite;
    match file.kind.as_str() {
        "dotnet-test" => {
            let methods: Vec<String> = match content.filter(|content| suite && !content.is_empty()) {
                Some(content) => csharp_targets(content)
                    .iter()
                    .filter(|test| test.kind == TargetKind::Test && test.path.first() == target.path.first())
                    .map(|test| test.path.join("."))
                    .collect(),
                None => vec![target.path.join(".")],
            };
            let mut config = named(file.args.clone());
            let methods = serde_json::to_string(&methods).unwrap_or_default();
            config.extras.params.insert("testMethods".to_string(), methods);
            Some(config)
        }
        "npm" | "bun" | "deno" => {
            let mut args = file.args.clone();
            args.extend(["-t".to_string(), js_name_pattern(target)]);
            Some(named(args))
        }
        "pytest" | "python" => Some(named(with_last(&file.args, |file| format!("{file}::{}", target.path.join("::"))))),
        "phpunit" => {
            let filter = if !target.pest {
                phpunit_filter(&target.path[..target.path.len().min(2)])
            } else if suite {
                pest_filter(&target.path, None)
            } else {
                pest_filter(&target.path[..target.path.len().saturating_sub(1)], Some(last))
            };
            Some(named(phpunit_args(&file.args, Some(&rel_to_cwd(rel_path, &file)), Some(&filter))))
        }
        "django-test" => {
            // unittest's loader would call a module-level `test_*` function (or a plain class) to build
            // the suite.
            if !target.unittest {
                return None;
            }
            let module = django_label(&rel_to_cwd(rel_path, &file));
            let label = std::iter::once(module).chain(target.path.iter().cloned()).collect::<Vec<_>>().join(".");
            let (args, params) = django_tests_of(&file, &[label])?;
            let mut config = named(args);
            if let Some(params) = params {
                config.extras.params = params;
            }
            Some(config)
        }
        "go" => {
            let anchored: Vec<String> = target.path.iter().map(|part| escape_reg_exp(part)).collect();
            let pattern = format!("^{}$", anchored.join("$/^"));
            let benchmark = target.path.first().is_some_and(|name| name.starts_with("Benchmark"));
            let parts = go_test_parts(&file.args);
            let flags = without_go_selection(&parts.flags);
            let program = without_go_selection(&parts.program);
            let selection = if benchmark {
                strings(&["-run", "^$", "-bench", pattern.as_str()])
            } else {
                strings(&["-run", pattern.as_str()])
            };
            let args = [strings(&["test"]), flags.clone(), parts.targets, selection, program.clone()].concat();
            let mut config = named(args);
            if param(&file, "goKind").is_some_and(|kind| !kind.is_empty()) {
                let mut tool_args = flags;
                if benchmark {
                    tool_args.extend(strings(&["-bench", pattern.as_str()]));
                }
                let params = &mut config.extras.params;
                params.insert("pattern".to_string(), if benchmark { "^$".to_string() } else { pattern });
                params.insert("toolArgs".to_string(), format_args(&tool_args));
                params.insert("programArgs".to_string(), format_args(program.get(1..).unwrap_or_default()));
            }
            Some(config)
        }
        "rake" | "ruby" => {
            // Minitest's `--name=`: `Class#test_x` names one test exactly, a regexp (no `$`, which a
            // shell would eat) the rest. The `=` form is one argument: rake's test loader would
            // `require` the value of `-n x`.
            let full = target.path.first().map_or("", String::as_str);
            let pattern = if target.kind == TargetKind::Test {
                format!("{full}#{}", target.path.get(1).map_or("", String::as_str))
            } else if target.ruby_module {
                format!("/^{full}::/")
            } else {
                format!("/^{full}#/")
            };
            let option = if file.kind == "rake" { "TESTOPTS=--name=" } else { "--name=" };
            let mut args = file.args.clone();
            args.push(format!("{option}{pattern}"));
            Some(named(args))
        }
        "cargo" => {
            let path = target.path.join("::");
            let mut args = file.args.clone();
            args.push("--".to_string());
            if suite {
                args.push(format!("{path}::"));
            } else {
                args.extend([path, "--exact".to_string()]);
            }
            Some(named(args))
        }
        "gradle" => {
            Some(named(with_last(&file.args, |file| if suite { file.to_string() } else { format!("{file}.{last}") })))
        }
        "maven" => {
            Some(named(with_last(&file.args, |file| if suite { file.to_string() } else { format!("{file}#{last}") })))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::{blank_config, cwd_of};

    fn cfg(id: &str, kind: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(id, id, kind);
        config.command = if kind == "maven" { "./mvnw".to_string() } else { kind.to_string() };
        config.args = strings(args);
        config.detected = true;
        config
    }

    #[test]
    fn creates_an_application_or_spring_boot_config_named_after_the_class() {
        let rel = "src/main/java/com/example/demo/SpringDemoApplication.java";
        let src = concat!(
            "package com.example.demo;\n@SpringBootApplication\nclass SpringDemoApplication {\n",
            " public static void main(String[] a) {}\n}",
        );
        let mut maven = cfg("maven:test", "maven", &["test"]);
        maven.is_test = true;
        let targets = detect_run_targets(src, rel, false);
        let main = targets.iter().find(|target| target.kind == TargetKind::Main).expect("a main target");
        let config = target_config(main, rel, &[maven], Some(src), None, RunEnv::default()).expect("a configuration");
        assert_eq!(config.id, "application:com.example.demo.SpringDemoApplication");
        assert_eq!((config.name.as_str(), config.kind.as_str()), ("SpringDemoApplication", "spring-boot"));
        assert_eq!(config.command, "java");
        assert_eq!(config.args, ["-cp", "${classpath}", "com.example.demo.SpringDemoApplication"]);
        assert_eq!((cwd_of(&config), config.is_test), (None, false));
        assert_eq!(config.debug_adapter.as_deref(), Some("java-debug"));
    }

    #[test]
    fn selects_the_owning_project_when_same_named_tests_exist_in_sibling_projects() {
        let source = "namespace Demo.Tests;\npublic class Calculator {\n [Xunit.Fact]\n public void Adds() { }\n}";
        let project = |project: &str| {
            let mut config = cfg(project, "dotnet-test", &["test", project]);
            config.command = "dotnet".into();
            config.is_test = true;
            config.extras.params.insert("project".into(), project.into());
            config.extras.params.insert("framework".into(), "net9.0".into());
            config
        };
        let configs = [project("one/One.csproj"), project("two/Two.csproj")];
        let targets = csharp_targets(source);
        let adds = targets.iter().find(|target| target.name == "Adds").expect("the test");
        let env = RunEnv::default();
        let config = target_config(adds, "two/Tests.cs", &configs, Some(source), None, env).expect("a configuration");
        assert_eq!(param(&config, "project"), Some("two/Two.csproj"));
        assert_eq!(param(&config, "testMethods"), Some("[\"Demo.Tests.Calculator.Adds\"]"));
    }

    #[test]
    fn recognizes_real_go_generate_directives_in_non_main_packages() {
        let content = concat!(
            "package tools\n//go:generate go run gen.go -out=$GOFILE\nfunc helper() {}\n",
            "// go:generate ignored\n//go:generate   \n",
        );
        let targets = detect_run_targets(content, "tools/generate.go", false);
        assert_eq!(targets.len(), 1);
        assert_eq!((targets[0].kind, targets[0].line), (TargetKind::Generate, 1));
    }

    // Not from a spec: describe blocks nest by indentation and `each` or placeholders mark a
    // parameterized test.
    #[test]
    fn finds_nested_js_tests_and_their_name_patterns() {
        let text = [
            "describe('math', () => {",
            "  it('adds', () => {});",
            "  test.each([1, 2])('doubles %i', (n) => {});",
            "  it(`has ${name}`, () => {});",
            "  it('it\\'s fine', () => {});",
            "});",
            "test('alone', () => {});",
        ]
        .join("\n");
        let targets = detect_run_targets(&text, "src/math.test.ts", false);
        let shape: Vec<(u32, TargetKind, &str, bool)> =
            targets.iter().map(|t| (t.line, t.kind, t.name.as_str(), t.parameterized)).collect();
        assert_eq!(
            shape,
            [
                (0, TargetKind::Suite, "math", false),
                (1, TargetKind::Test, "math > adds", false),
                (2, TargetKind::Test, "math > doubles %i", true),
                (3, TargetKind::Test, "math > has ${name}", true),
                (4, TargetKind::Test, "math > it's fine", false),
                (6, TargetKind::Test, "alone", false),
            ]
        );
        assert_eq!(js_name_pattern(&targets[1]), "^math adds$");
        assert_eq!(js_name_pattern(&targets[0]), "^math");
        assert_eq!(js_name_pattern(&targets[2]), "^math doubles .*$");
        assert!(detect_run_targets(&text, "src/math.ts", false).is_empty());
    }

    // Not from a spec: Rust tests carry their module path, and a module without tests is no suite.
    #[test]
    fn finds_rust_tests_suites_and_main() {
        let text = [
            "fn main() {}",
            "#[cfg(test)]",
            "mod tests {",
            "    #[test]",
            "    fn adds() { let s = \"}\"; }",
            "    mod empty {}",
            "    #[tokio::test]",
            "    async fn waits() {}",
            "}",
        ]
        .join("\n");
        let targets = detect_run_targets(&text, "src/bin/tool.rs", false);
        let shape: Vec<(u32, TargetKind, String)> =
            targets.iter().map(|t| (t.line, t.kind, t.path.join("::"))).collect();
        assert_eq!(
            shape,
            [
                (0, TargetKind::Main, String::new()),
                (2, TargetKind::Suite, "tests".to_string()),
                (4, TargetKind::Test, "tests::adds".to_string()),
                (7, TargetKind::Test, "tests::waits".to_string()),
            ]
        );
        let cargo = RunConfig { is_test: true, ..cfg("cargo:test", "cargo", &["test"]) };
        let env = RunEnv::default();
        let test = target_config(&targets[2], "src/bin/tool.rs", std::slice::from_ref(&cargo), None, None, env);
        assert_eq!(test.expect("a configuration").args, ["test", "--", "tests::adds", "--exact"]);
        assert_eq!(rust_module_prefix("crates/app/src/net/mod.rs"), ["net"]);
    }

    // Not from a spec: pytest classes nest, unittest cases are flagged, and a target narrows the file.
    #[test]
    fn finds_python_tests_and_narrows_pytest_to_them() {
        let text = [
            "class TestMath:",
            "    def test_adds(self):",
            "        pass",
            "class Helper:",
            "    def test_not_collected(self):",
            "        pass",
            "class ApiTests(TestCase):",
            "    def test_get(self):",
            "        pass",
            "def test_alone():",
            "    pass",
            "if __name__ == \"__main__\":",
            "    main()",
        ]
        .join("\n");
        let targets = detect_run_targets(&text, "tests/test_math.py", false);
        let shape: Vec<(u32, &str, bool)> = targets.iter().map(|t| (t.line, t.name.as_str(), t.unittest)).collect();
        assert_eq!(
            shape,
            [
                (0, "TestMath", false),
                (1, "TestMath.test_adds", false),
                (6, "ApiTests", true),
                (7, "ApiTests.test_get", true),
                (9, "test_alone", false),
                (11, "main", false),
            ]
        );
        let pytest = RunConfig { is_test: true, ..cfg("pytest", "pytest", &["-q"]) };
        let env = RunEnv::default();
        let config = target_config(&targets[1], "tests/test_math.py", &[pytest], None, None, env);
        assert_eq!(config.expect("a configuration").args, ["-q", "tests/test_math.py::TestMath::test_adds"]);
        assert_eq!(detect_run_targets(&text, "tools/gen.py", false).len(), 1);
    }

    // Not from a spec: Minitest classes and the modules holding them are suites; empty ones are dropped.
    #[test]
    fn finds_minitest_tests_and_names_them_for_the_runner() {
        let text = [
            "module Billing",
            "  class InvoiceTest < Minitest::Test",
            "    def test_total",
            "    end",
            "    test \"rounds  cents\" do",
            "    end",
            "  end",
            "  class Helper",
            "  end",
            "end",
        ]
        .join("\n");
        let targets = detect_run_targets(&text, "test/invoice_test.rb", false);
        let shape: Vec<(u32, &str, bool)> = targets.iter().map(|t| (t.line, t.name.as_str(), t.ruby_module)).collect();
        assert_eq!(
            shape,
            [
                (0, "Billing", true),
                (1, "Billing::InvoiceTest", false),
                (2, "Billing::InvoiceTest#test_total", false),
                (4, "Billing::InvoiceTest#test_rounds_cents", false),
            ]
        );
        let rake = RunConfig { is_test: true, ..cfg("rake:test", "rake", &["exec", "rake", "test"]) };
        let env = RunEnv::default();
        let run = |target: &RunTarget| {
            let config = target_config(target, "test/invoice_test.rb", std::slice::from_ref(&rake), None, None, env);
            config.expect("a configuration").args.last().cloned().unwrap_or_default()
        };
        assert_eq!(run(&targets[0]), "TESTOPTS=--name=/^Billing::/");
        assert_eq!(run(&targets[1]), "TESTOPTS=--name=/^Billing::InvoiceTest#/");
        assert_eq!(run(&targets[2]), "TESTOPTS=--name=Billing::InvoiceTest#test_total");
    }

    // Not from a spec: PHPUnit methods select by class and method, Pest tests by their describe chain.
    #[test]
    fn finds_phpunit_and_pest_tests_and_their_filters() {
        let text = [
            "final class CartTest extends TestCase {",
            "    public function testAdds(): void {}",
            "    #[Test]",
            "    public function removes(): void {}",
            "    public function helper(): void {}",
            "}",
            "describe('cart', function () {",
            "    it('adds', function () {});",
            "});",
        ]
        .join("\n");
        let targets = detect_run_targets(&text, "tests/CartTest.php", false);
        let names: Vec<&str> = targets.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["CartTest", "CartTest::testAdds", "CartTest::removes", "cart", "cart → it adds"]);
        assert_eq!(phpunit_filter(&targets[1].path), "/(CartTest::testAdds( with data set .*)?)$/");
        assert_eq!(phpunit_filter(&targets[0].path), "/(CartTest::.*)$/");
        assert_eq!(pest_filter(&targets[3].path, None), "/::`cart` → /");
        let phpunit = RunConfig { is_test: true, ..cfg("phpunit", "phpunit", &["vendor/bin/phpunit", "--colors"]) };
        let env = RunEnv::default();
        let config = target_config(&targets[4], "tests/CartTest.php", &[phpunit], None, None, env);
        let filter = "/::`cart` → it adds( with data set .*)?$/";
        let expected = ["vendor/bin/phpunit", "--colors", "--filter", filter, "tests/CartTest.php"];
        assert_eq!(config.expect("a configuration").args, expected);
    }

    // Not from a spec: a Go subtest narrows `go test` to its anchored path, dropping the base's selection.
    #[test]
    fn narrows_go_test_to_a_subtest() {
        let target = RunTarget::new(3, TargetKind::Test, "TestAdd/small", strings(&["TestAdd", "small"]));
        let mut go = cfg("go:test", "go", &["test", "-race", "-run", "^Old$", "./..."]);
        go.is_test = true;
        let config = target_config(&target, "calc/calc_test.go", &[go], None, None, RunEnv::default());
        assert_eq!(config.expect("a configuration").args, ["test", "-race", "./calc", "-run", "^TestAdd$/^small$"]);
    }
}
