//! Picks the test output parser for a run configuration, and the launch arguments that make a test
//! run report every test.

use crate::run_testing::cargo::CargoParser;
use crate::run_testing::go::GoParser;
use crate::run_testing::js::JsParser;
use crate::run_testing::jvm::JvmParser;
use crate::run_testing::phpunit::{PhpunitParser, with_teamcity};
use crate::run_testing::pytest::PytestParser;
use crate::run_testing::ruby::RubyParser;
use crate::run_testing::types::{TestEvent, TestOutputParser};
use crate::run_testing::unittest::UnittestParser;
use ide_run::RunConfig;

/// The test framework whose output a configuration prints; `Auto` when the command does not tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Framework {
    Cargo,
    Js,
    Pytest,
    Unittest,
    Phpunit,
    Go,
    Jvm,
    Ruby,
    Dotnet,
    Auto,
}

pattern!(MANAGE_TEST, r"(?-u:\b)manage\.py test(?-u:\b)");
pattern!(DOTNET_COMMAND, r"(?i)(^|[/\\])dotnet(?:\.exe)?$");
pattern!(CARGO_WORD, r"(?-u:\b)cargo(?-u:\b)");
pattern!(JS_WORD, r"(?-u:\b)(jest|vitest|npm|pnpm|yarn|bun|npx|deno)(?-u:\b)");
pattern!(PYTEST_WORD, r"(?-u:\b)pytest(?-u:\b)");
pattern!(PHPUNIT_WORD, r"(^|[/\\\s])(phpunit|pest)(\.phar|\.bat)?(\s|$)");
pattern!(GO_TEST, r"(?-u:\b)go\s+test(?-u:\b)");
pattern!(JVM_WORD, r"(?-u:\b)(gradlew?|mvnw?)(?-u:\b)");
pattern!(UNITTEST_VERBOSITY, r"^(-v|--verbosity)(=?[0-9])?$");
pattern!(VERBOSE_OPTION, r"(^|\s)(-v|--verbose)(\s|$)");
pattern!(PYTEST_COMMAND, r"(^|[/\\])pytest(\.exe)?$");
pattern!(PYTEST_VERBOSITY, r"^-[vq]+$|^--(verbose|quiet)$|^--verbosity");

pub fn framework_for(config: &RunConfig) -> Framework {
    let command_line = std::iter::once(&config.command).chain(&config.args).cloned().collect::<Vec<_>>().join(" ");
    match config.kind.as_str() {
        "dotnet-test" => return Framework::Dotnet,
        "cargo" => return Framework::Cargo,
        "npm" | "deno" | "bun" => return Framework::Js,
        "pytest" => return Framework::Pytest,
        // `manage.py test` made as a Python configuration before the Django tests type existed.
        "python" if MANAGE_TEST.is_match(&command_line) => return Framework::Unittest,
        "python" => return Framework::Pytest,
        "django-test" => return Framework::Unittest,
        "phpunit" => return Framework::Phpunit,
        "go" => return Framework::Go,
        "gradle" | "maven" => return Framework::Jvm,
        "ruby" | "rake" | "rspec" => return Framework::Ruby,
        _ => {}
    }
    if DOTNET_COMMAND.is_match(&config.command) && config.args.first().is_some_and(|arg| arg == "test") {
        Framework::Dotnet
    } else if CARGO_WORD.is_match(&command_line) {
        Framework::Cargo
    } else if JS_WORD.is_match(&command_line) {
        Framework::Js
    } else if PYTEST_WORD.is_match(&command_line) {
        Framework::Pytest
    } else if MANAGE_TEST.is_match(&command_line) {
        Framework::Unittest
    } else if PHPUNIT_WORD.is_match(&command_line) {
        Framework::Phpunit
    } else if GO_TEST.is_match(&command_line) {
        Framework::Go
    } else if JVM_WORD.is_match(&command_line) {
        Framework::Jvm
    } else {
        Framework::Auto
    }
}

fn with_args(config: &RunConfig, args: Vec<String>) -> RunConfig {
    RunConfig { args, ..config.clone() }
}

fn with_arg(config: &RunConfig, extra: &[&str]) -> RunConfig {
    let mut args = config.args.clone();
    args.extend(extra.iter().map(|arg| (*arg).to_string()));
    with_args(config, args)
}

/// `TESTOPTS=-v` / a trailing `-v`: Minitest prints a line per test only when verbose.
fn ruby_verbose(config: &RunConfig) -> RunConfig {
    match config.kind.as_str() {
        "rspec" => config.clone(),
        "rake" => {
            let Some(at) = config.args.iter().position(|arg| arg.starts_with("TESTOPTS=")) else {
                return with_arg(config, &["TESTOPTS=-v"]);
            };
            // Options already given (`--name=Class#test_x`): add ours unless verbosity was chosen.
            let mut args = config.args.clone();
            if !VERBOSE_OPTION.is_match(&args[at]) {
                args[at].push_str(" -v");
            }
            with_args(config, args)
        }
        _ if config.args.iter().any(|arg| arg == "-v" || arg == "--verbose") => config.clone(),
        _ => with_arg(config, &["-v"]),
    }
}

/// Launch arguments that make a test run reportable per test: pytest and Django print only progress
/// dots by default, so passed tests would never reach the tree. `-v` (Django: `-v 2`) adds one line
/// per test unless the user chose a verbosity already; Go and PHPUnit print machine-readable records.
pub fn with_test_reporting(config: &RunConfig) -> RunConfig {
    match framework_for(config) {
        Framework::Go if config.args.first().is_some_and(|arg| arg == "test") => {
            let boundary = config.args.iter().position(|arg| arg == "-args" || arg == "--");
            let (tool, program) = config.args.split_at(boundary.unwrap_or(config.args.len()));
            let kept = tool.iter().skip(1).filter(|arg| *arg != "-json" && !arg.starts_with("-json="));
            let mut args = vec!["test".to_string(), "-json".to_string()];
            args.extend(kept.cloned());
            args.extend_from_slice(program);
            with_args(config, args)
        }
        Framework::Phpunit => with_args(config, with_teamcity(&config.args)),
        Framework::Unittest if config.args.iter().any(|arg| UNITTEST_VERBOSITY.is_match(arg)) => config.clone(),
        Framework::Unittest => with_arg(config, &["-v", "2"]),
        Framework::Ruby => ruby_verbose(config),
        Framework::Pytest => {
            let command_line: Vec<&String> = std::iter::once(&config.command).chain(&config.args).collect();
            let Some(pytest) = command_line.iter().position(|arg| PYTEST_COMMAND.is_match(arg)) else {
                return config.clone();
            };
            if command_line[pytest + 1..].iter().any(|arg| PYTEST_VERBOSITY.is_match(arg)) {
                config.clone()
            } else {
                with_arg(config, &["-v"])
            }
        }
        _ => config.clone(),
    }
}

/// Runs several parsers side by side (unknown tool: e.g. `make test`).
pub struct CombinedParser {
    parsers: Vec<Box<dyn TestOutputParser>>,
}

impl CombinedParser {
    pub fn new(parsers: Vec<Box<dyn TestOutputParser>>) -> Self {
        Self { parsers }
    }
}

impl TestOutputParser for CombinedParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        self.parsers.iter_mut().flat_map(|parser| parser.feed(line)).collect()
    }

    fn end(&mut self) -> Vec<TestEvent> {
        self.parsers.iter_mut().flat_map(|parser| parser.end()).collect()
    }
}

/// The output of a run whose results are read from report files.
struct NoOutputParser;

impl TestOutputParser for NoOutputParser {
    fn feed(&mut self, _line: &str) -> Vec<TestEvent> {
        Vec::new()
    }
}

pub fn create_test_parser(config: &RunConfig) -> Box<dyn TestOutputParser> {
    match framework_for(config) {
        Framework::Dotnet => Box::new(NoOutputParser),
        Framework::Cargo => Box::new(CargoParser::new()),
        Framework::Js => Box::new(JsParser::new()),
        Framework::Pytest => Box::new(PytestParser::new()),
        Framework::Unittest => Box::new(UnittestParser::new()),
        Framework::Phpunit => Box::new(PhpunitParser::new()),
        Framework::Go => Box::new(GoParser::new()),
        Framework::Jvm => Box::new(JvmParser::new()),
        Framework::Ruby => Box::new(RubyParser::new()),
        Framework::Auto => Box::new(CombinedParser::new(vec![
            Box::new(CargoParser::new()),
            Box::new(JsParser::new()),
            Box::new(PytestParser::new()),
            Box::new(UnittestParser::new()),
            Box::new(PhpunitParser::new()),
            Box::new(GoParser::new()),
            Box::new(JvmParser::new()),
            Box::new(RubyParser::new()),
        ])),
    }
}

/// The events of a whole output, for batch parsing.
pub fn parse_all(parser: &mut dyn TestOutputParser, text: &str) -> Vec<TestEvent> {
    let mut events = Vec::new();
    for line in text.split('\n') {
        events.extend(parser.feed(line.strip_suffix('\r').unwrap_or(line)));
    }
    events.extend(parser.end());
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run::links::{FileTarget, Resolve};
    use crate::run_testing::jvm::move_to_reported_classes;
    use crate::run_testing::results::{TestCounts, TestNode, TestResults};
    use crate::run_testing::types::{
        TestCaseStatus, TestStatus, TestSummary, file_location, parse_duration, test_path,
    };

    fn results(parser: &mut dyn TestOutputParser, text: &str) -> TestResults {
        let mut results = TestResults::new();
        results.apply(&parse_all(parser, text));
        results
    }

    fn leaf<'a>(results: &'a TestResults, path: &[&str]) -> Option<&'a TestNode> {
        results.node_at(path)
    }

    fn own(results: &TestResults, path: &[&str]) -> Option<TestStatus> {
        leaf(results, path)?.own
    }

    fn status(results: &TestResults, path: &[&str]) -> Option<TestStatus> {
        results.status(leaf(results, path)?)
    }

    fn location(results: &TestResults, path: &[&str]) -> Option<FileTarget> {
        leaf(results, path)?.location.clone()
    }

    fn root_names(results: &TestResults) -> Vec<&str> {
        results.roots().into_iter().map(|root| root.name.as_str()).collect()
    }

    fn cfg(kind: &str, command: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(format!("{kind}:test"), format!("{kind} test"), kind);
        config.command = command.to_string();
        config.args = test_path(args);
        config
    }

    fn passed(path: &[&str]) -> TestEvent {
        TestEvent::result(&test_path(path), TestCaseStatus::Passed, None)
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse_duration("(3 ms)"), Some(3.0));
        assert_eq!(parse_duration("1.2 s"), Some(1200.0));
        assert_eq!(parse_duration("0.00s"), Some(0.0));
        assert_eq!(parse_duration("12ms"), Some(12.0));
        assert_eq!(parse_duration("1m 3s"), Some(63_000.0));
        assert_eq!(parse_duration("0.05 sec"), Some(50.0));
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("soon"), None);
    }

    const CARGO: &str = r"   Compiling app v0.1.0 (/p/app)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.52s
     Running unittests src/lib.rs (target/debug/deps/app-1a2b3c)

running 3 tests
test tests::adds ... ok
test tests::slow ... ignored, needs network
test tests::subtracts ... FAILED

failures:

---- tests::subtracts stdout ----

thread 'tests::subtracts' panicked at src/lib.rs:20:9:
assertion `left == right` failed
  left: 1
 right: 2
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace


failures:
    tests::subtracts

test result: FAILED. 1 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s

     Running tests/api.rs (target/debug/deps/api-9f8e7d)

running 1 test
test health ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
";

    #[test]
    fn builds_a_tree_per_test_binary_and_module() {
        let r = results(&mut CargoParser::new(), CARGO);
        assert_eq!(root_names(&r), ["src/lib.rs", "tests/api.rs"]);
        assert_eq!(own(&r, &["src/lib.rs", "tests", "adds"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["src/lib.rs", "tests", "slow"]), Some(TestStatus::Skipped));
        assert_eq!(own(&r, &["tests/api.rs", "health"]), Some(TestStatus::Passed));
        assert_eq!(status(&r, &["src/lib.rs"]), Some(TestStatus::Failed));
        assert_eq!(status(&r, &["tests/api.rs"]), Some(TestStatus::Passed));
    }

    #[test]
    fn captures_failure_output_and_panic_location() {
        let r = results(&mut CargoParser::new(), CARGO);
        let failed = leaf(&r, &["src/lib.rs", "tests", "subtracts"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.location, Some(file_location("src/lib.rs", Some(20), Some(9), Resolve::Path)));
        assert!(failed.output.join("\n").contains("left: 1"));
        assert_eq!(r.first_failure().map(|node| node.id.as_str()), Some(failed.id.as_str()));
    }

    #[test]
    fn accumulates_summaries_across_binaries() {
        let r = results(&mut CargoParser::new(), CARGO);
        let summary = TestSummary { passed: Some(2), failed: Some(1), skipped: Some(1), duration_ms: Some(10.0) };
        assert_eq!(r.summary, summary);
        assert_eq!(r.counts(), TestCounts { passed: 2, failed: 1, skipped: 1, running: 0, total: 4 });
    }

    #[test]
    fn supports_the_pre_1_73_panic_format() {
        let output = "test it_fails ... FAILED\n---- it_fails stdout ----\n\
                      thread 'it_fails' panicked at 'boom', src/main.rs:3:5\n";
        let r = results(&mut CargoParser::new(), output);
        let expected = file_location("src/main.rs", Some(3), Some(5), Resolve::Path);
        assert_eq!(location(&r, &["it_fails"]), Some(expected));
    }

    #[test]
    fn parses_nextest() {
        let output = "        PASS [   0.004s] app tests::adds
        FAIL [   0.120s] app tests::subtracts
     Summary [   0.130s] 2 tests run: 1 passed, 1 failed, 0 skipped";
        let r = results(&mut CargoParser::new(), output);
        let adds = leaf(&r, &["app", "tests", "adds"]).expect("the test is reported");
        assert_eq!((adds.own, adds.duration_ms), (Some(TestStatus::Passed), Some(4.0)));
        assert_eq!(own(&r, &["app", "tests", "subtracts"]), Some(TestStatus::Failed));
        let summary = TestSummary { passed: Some(1), failed: Some(1), skipped: Some(0), duration_ms: Some(130.0) };
        assert_eq!(r.summary, summary);
    }

    const JEST: &str = "PASS src/math.test.ts
  math
    ✓ adds (2 ms)
    nested
      ✓ deep (1 ms)
  ✓ top level

FAIL src/order.test.ts (5.2 s)
  orders
    ✓ lists (3 ms)
    ✕ creates (12 ms)
    ○ skipped later

  ● orders › creates

    expect(received).toBe(expected) // Object.is equality

    Expected: 2
    Received: 1

      at Object.<anonymous> (node_modules/expect/build/index.js:1:1)
      at Object.<anonymous> (src/order.test.ts:14:21)

Test Suites: 1 failed, 1 passed, 2 total
Tests:       1 failed, 1 skipped, 4 passed, 6 total
Snapshots:   0 total
Time:        6.1 s
";

    #[test]
    fn nests_describe_blocks_by_indentation() {
        let r = results(&mut JsParser::new(), JEST);
        let adds = leaf(&r, &["src/math.test.ts", "math", "adds"]).expect("the test is reported");
        assert_eq!((adds.own, adds.duration_ms), (Some(TestStatus::Passed), Some(2.0)));
        assert_eq!(own(&r, &["src/math.test.ts", "math", "nested", "deep"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["src/math.test.ts", "top level"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["src/order.test.ts", "orders", "skipped later"]), Some(TestStatus::Skipped));
        assert_eq!(status(&r, &["src/order.test.ts"]), Some(TestStatus::Failed));
    }

    #[test]
    fn links_failures_to_the_first_project_stack_frame() {
        let r = results(&mut JsParser::new(), JEST);
        let failed = leaf(&r, &["src/order.test.ts", "orders", "creates"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.location, Some(file_location("src/order.test.ts", Some(14), Some(21), Resolve::Path)));
        assert!(failed.output.iter().any(|line| line.contains("Expected: 2")));
    }

    #[test]
    fn reads_the_summary() {
        let r = results(&mut JsParser::new(), JEST);
        let summary = TestSummary { passed: Some(4), failed: Some(1), skipped: Some(1), duration_ms: Some(6100.0) };
        assert_eq!(r.summary, summary);
    }

    #[test]
    fn creates_failed_tests_from_non_verbose_output() {
        let output = "PASS src/a.test.js
FAIL src/b.test.js
  ● suite › broken

    boom

      at Object.<anonymous> (src/b.test.js:3:9)
";
        let r = results(&mut JsParser::new(), output);
        assert_eq!(own(&r, &["src/a.test.js"]), Some(TestStatus::Passed));
        let broken = location(&r, &["src/b.test.js", "suite", "broken"]).expect("the failure is located");
        assert_eq!(broken.line, Some(3));
    }

    const VITEST: &str = r#" RUN  v3.2.4 /p/web

 ✓ src/a.test.ts (2 tests) 3ms
 ❯ src/b.test.ts (2 tests | 1 failed) 7ms
   ✓ cart > adds item 1ms
   × cart > removes item 4ms
     → expected 1 to be 0

⎯⎯⎯⎯⎯⎯⎯ Failed Tests 1 ⎯⎯⎯⎯⎯⎯⎯

 FAIL  src/b.test.ts > cart > removes item
AssertionError: expected 1 to be 0 // Object.is equality
 ❯ src/b.test.ts:12:17
     10|   it("removes item", () => {

 Test Files  1 failed | 1 passed (2)
      Tests  1 failed | 3 passed (4)
   Start at  10:00:00
   Duration  412ms
"#;

    #[test]
    fn builds_file_and_test_nodes() {
        let r = results(&mut JsParser::new(), VITEST);
        let file = leaf(&r, &["src/a.test.ts"]).expect("the file is reported");
        assert_eq!((file.own, file.duration_ms), (Some(TestStatus::Passed), Some(3.0)));
        assert_eq!(own(&r, &["src/b.test.ts", "cart", "adds item"]), Some(TestStatus::Passed));
        let removes = leaf(&r, &["src/b.test.ts", "cart", "removes item"]).expect("the test is reported");
        assert_eq!((removes.own, removes.duration_ms), (Some(TestStatus::Failed), Some(4.0)));
        assert_eq!(status(&r, &["src/b.test.ts"]), Some(TestStatus::Failed));
    }

    #[test]
    fn links_the_failure_location() {
        let r = results(&mut JsParser::new(), VITEST);
        let expected = file_location("src/b.test.ts", Some(12), Some(17), Resolve::Path);
        assert_eq!(location(&r, &["src/b.test.ts", "cart", "removes item"]), Some(expected));
    }

    #[test]
    fn reads_counts_and_duration() {
        let r = results(&mut JsParser::new(), VITEST);
        let summary = TestSummary { passed: Some(3), failed: Some(1), skipped: None, duration_ms: Some(412.0) };
        assert_eq!(r.summary, summary);
        let counts = r.counts();
        assert_eq!((counts.passed, counts.failed), (3, 1));
    }

    #[test]
    fn handles_flat_verbose_lines() {
        let output = " ✓ src/x.test.ts > group > works 2ms\n ↓ src/x.test.ts > group > later";
        let r = results(&mut JsParser::new(), output);
        assert_eq!(own(&r, &["src/x.test.ts", "group", "works"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["src/x.test.ts", "group", "later"]), Some(TestStatus::Skipped));
    }

    const PYTEST: &str = "============================= test session starts ==============================
platform darwin -- Python 3.12.1, pytest-8.3.2
collected 4 items

tests/test_math.py::test_add PASSED                                      [ 25%]
tests/test_math.py::test_sub FAILED                                      [ 50%]
tests/test_math.py::TestDiv::test_zero[1] SKIPPED (no zero)              [ 75%]
tests/test_math.py::TestDiv::test_ok PASSED                              [100%]

=================================== FAILURES ===================================
___________________________________ test_sub ___________________________________

    def test_sub():
>       assert 1 - 1 == 1
E       assert 0 == 1

tests/test_math.py:7: AssertionError
=========================== short test summary info ============================
FAILED tests/test_math.py::test_sub - assert 0 == 1
=============== 1 failed, 2 passed, 1 skipped in 0.12s ===============
";

    #[test]
    fn parses_verbose_node_ids_into_a_tree() {
        let r = results(&mut PytestParser::new(), PYTEST);
        assert_eq!(own(&r, &["tests/test_math.py", "test_add"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["tests/test_math.py", "TestDiv", "test_zero[1]"]), Some(TestStatus::Skipped));
        assert_eq!(own(&r, &["tests/test_math.py", "TestDiv", "test_ok"]), Some(TestStatus::Passed));
        assert_eq!(status(&r, &["tests/test_math.py"]), Some(TestStatus::Failed));
    }

    #[test]
    fn attaches_failure_sections_and_locations() {
        let r = results(&mut PytestParser::new(), PYTEST);
        let failed = leaf(&r, &["tests/test_math.py", "test_sub"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.location, Some(file_location("tests/test_math.py", Some(7), None, Resolve::Path)));
        assert!(failed.output.iter().any(|line| line.contains("assert 0 == 1")));
    }

    #[test]
    fn reads_the_final_summary() {
        let r = results(&mut PytestParser::new(), PYTEST);
        let summary = TestSummary { passed: Some(2), failed: Some(1), skipped: Some(1), duration_ms: Some(120.0) };
        assert_eq!(r.summary, summary);
    }

    #[test]
    fn resolves_non_verbose_failures_from_the_short_summary() {
        let plain = "tests/test_math.py .F.                                                   [100%]

=================================== FAILURES ===================================
_______________________________ TestDiv.test_ok ________________________________

tests/test_math.py:21: ZeroDivisionError
=========================== short test summary info ============================
FAILED tests/test_math.py::TestDiv::test_ok - ZeroDivisionError
========================= 1 failed, 2 passed in 0.05s =========================
";
        let r = results(&mut PytestParser::new(), plain);
        let failed = leaf(&r, &["tests/test_math.py", "TestDiv", "test_ok"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.location.as_ref().and_then(|location| location.line), Some(21));
        assert_eq!(root_names(&r), ["tests/test_math.py"]);
        let counts = r.counts();
        assert_eq!((counts.passed, counts.failed), (2, 1));
    }

    const GO: &str = "=== RUN   TestAdd
--- PASS: TestAdd (0.00s)
=== RUN   TestSub
    math_test.go:12: expected 0, got 1
--- FAIL: TestSub (0.01s)
=== RUN   TestTable
=== RUN   TestTable/zero
=== RUN   TestTable/one
--- PASS: TestTable (0.00s)
    --- PASS: TestTable/zero (0.00s)
    --- SKIP: TestTable/one (0.00s)
FAIL
FAIL\texample.com/calc\t0.004s
ok  \texample.com/util\t(cached)
?   \texample.com/cmd\t[no test files]
";

    #[test]
    fn groups_tests_and_subtests_under_their_package() {
        let r = results(&mut GoParser::new(), GO);
        assert_eq!(root_names(&r), ["example.com/calc", "example.com/util"]);
        assert_eq!(own(&r, &["example.com/calc", "TestAdd"]), Some(TestStatus::Passed));
        let sub = leaf(&r, &["example.com/calc", "TestSub"]).expect("the test is reported");
        assert_eq!((sub.own, sub.duration_ms), (Some(TestStatus::Failed), Some(10.0)));
        assert_eq!(own(&r, &["example.com/calc", "TestTable", "zero"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["example.com/calc", "TestTable", "one"]), Some(TestStatus::Skipped));
        assert_eq!(own(&r, &["example.com/util"]), Some(TestStatus::Passed));
    }

    #[test]
    fn captures_the_failure_location() {
        let r = results(&mut GoParser::new(), GO);
        let expected = file_location("math_test.go", Some(12), None, Resolve::Name);
        assert_eq!(location(&r, &["example.com/calc", "TestSub"]), Some(expected));
    }

    #[test]
    fn handles_non_verbose_output_failure_lines_after_the_result() {
        let output = "--- FAIL: TestSub (0.00s)\n    math_test.go:9: nope\nFAIL\nFAIL\texample.com/calc\t0.002s\n";
        let r = results(&mut GoParser::new(), output);
        let located = location(&r, &["example.com/calc", "TestSub"]).expect("the failure is located");
        assert_eq!(located.line, Some(9));
    }

    #[test]
    fn parses_gradle_test_logging() {
        let output = "> Task :test

OrderControllerTest > shouldListOrders() PASSED

OrderControllerTest > shouldCreateOrder() FAILED
    org.opentest4j.AssertionFailedError at OrderControllerTest.kt:22

OrderControllerTest > Paging > firstPage() SKIPPED

3 tests completed, 1 failed, 1 skipped
";
        let r = results(&mut JvmParser::new(), output);
        assert_eq!(own(&r, &["OrderControllerTest", "shouldListOrders"]), Some(TestStatus::Passed));
        let expected = file_location("OrderControllerTest.kt", Some(22), None, Resolve::Name);
        assert_eq!(location(&r, &["OrderControllerTest", "shouldCreateOrder"]), Some(expected));
        assert_eq!(own(&r, &["OrderControllerTest", "Paging", "firstPage"]), Some(TestStatus::Skipped));
        let summary = TestSummary { passed: Some(1), failed: Some(1), skipped: Some(1), duration_ms: None };
        assert_eq!(r.summary, summary);
    }

    #[test]
    fn moves_gradles_console_results_to_the_reported_classes() {
        let output = "OrderTest > creates order() FAILED
    org.opentest4j.AssertionFailedError at OrderTest.kt:22
";
        let mut r = results(&mut JvmParser::new(), output);
        r.apply(&[
            passed(&["com.acme.OrderTest", "lists"]),
            TestEvent::result(&test_path(&["com.acme.OrderTest", "creates order"]), TestCaseStatus::Failed, None),
            passed(&["com.acme.UserTest", "loads"]),
            passed(&["com.other.UserTest", "loads"]),
        ]);
        let reported = test_path(&["com.acme.OrderTest", "com.acme.UserTest", "com.other.UserTest"]);
        move_to_reported_classes(&mut r, &reported);
        assert_eq!(root_names(&r), ["com.acme.OrderTest", "com.acme.UserTest", "com.other.UserTest"]);
        let failed = leaf(&r, &["com.acme.OrderTest", "creates order"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.location, Some(file_location("OrderTest.kt", Some(22), None, Resolve::Name)));
        assert!(!failed.output.is_empty());
        let counts = r.counts();
        assert_eq!((counts.passed, counts.failed), (3, 1));
        assert!(r.get("OrderTest").is_none());
    }

    #[test]
    fn reports_each_test_from_the_gradle_init_scripts_service_messages() {
        let output = "> Task :test
##teamcity[testStarted name='addsZero()' className='demo.CalcTest']
##teamcity[testStdOut name='addsZero()' className='demo.CalcTest' out='adding zero|n']
##teamcity[testFinished name='addsZero()' className='demo.CalcTest' duration='11']
##teamcity[testStarted name='fails()' className='demo.CalcTest']
##teamcity[testFailed name='fails()' className='demo.CalcTest' message='boom' details='org.opentest4j.AssertionFailedError: boom|n\tat app//org.junit.jupiter.api.AssertEquals.failNotEqual(AssertEquals.java:197)|n\tat app//demo.CalcTest.fails(CalcTest.java:21)|n']
##teamcity[testFinished name='fails()' className='demo.CalcTest' duration='4']
CalcTest > fails() FAILED
    org.opentest4j.AssertionFailedError at CalcTest.java:21
##teamcity[testStarted name='add(int)|[1|]' className='demo.ParamTest']
##teamcity[testFailed name='add(int)|[1|]' className='demo.ParamTest' message='no' details='no']
##teamcity[testFinished name='add(int)|[1|]' className='demo.ParamTest' duration='1']
##teamcity[testStarted name='add(int)|[2|]' className='demo.ParamTest']
##teamcity[testFinished name='add(int)|[2|]' className='demo.ParamTest' duration='1']
##teamcity[testStarted name='later()' className='demo.ParamTest']
##teamcity[testIgnored name='later()' className='demo.ParamTest']
##teamcity[testFinished name='later()' className='demo.ParamTest' duration='0']
";
        let r = results(&mut JvmParser::new(), output);
        // Named like the XML reports, and not repeated under Gradle's simple class name.
        assert_eq!(root_names(&r), ["demo.CalcTest", "demo.ParamTest"]);
        let adds = leaf(&r, &["demo.CalcTest", "addsZero"]).expect("the test is reported");
        assert_eq!((adds.own, adds.duration_ms), (Some(TestStatus::Passed), Some(11.0)));
        assert_eq!(adds.output, ["adding zero"]);
        let failed = leaf(&r, &["demo.CalcTest", "fails"]).expect("the test is reported");
        assert_eq!(failed.own, Some(TestStatus::Failed));
        assert_eq!(failed.output[0], "org.opentest4j.AssertionFailedError: boom");
        let located = failed.location.as_ref().expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line), ("demo/CalcTest.java", Some(21)));
        assert_eq!(located.class_name.as_deref(), Some("demo.CalcTest"));
        assert_eq!(own(&r, &["demo.ParamTest", "add"]), Some(TestStatus::Failed));
        assert_eq!(own(&r, &["demo.ParamTest", "later"]), Some(TestStatus::Skipped));
        assert!(r.has_result(&["demo.CalcTest", "fails"]));
        assert!(!r.has_result(&["demo.CalcTest", "other"]));
    }

    #[test]
    fn shows_a_test_as_running_from_its_start() {
        let mut r = TestResults::new();
        r.apply(&JvmParser::new().feed("##teamcity[testStarted name='slow()' className='demo.SlowTest']"));
        assert_eq!(own(&r, &["demo.SlowTest", "slow"]), Some(TestStatus::Running));
        assert!(!r.has_result(&["demo.SlowTest", "slow"]));
    }

    #[test]
    fn parses_surefire_reports() {
        let output = "[INFO] Running com.acme.OrderTest
[ERROR] Tests run: 2, Failures: 1, Errors: 0, Skipped: 0, Time elapsed: 0.05 s <<< FAILURE! -- in com.acme.OrderTest
[ERROR] com.acme.OrderTest.shouldTotal -- Time elapsed: 0.01 s <<< FAILURE!
org.opentest4j.AssertionFailedError: expected: <3> but was: <2>
\tat org.junit.jupiter.api.AssertionUtils.fail(AssertionUtils.java:151)
\tat com.acme.OrderTest.shouldTotal(OrderTest.java:31)

[INFO] Tests run: 1, Failures: 0, Errors: 0, Skipped: 0, Time elapsed: 0.01 s -- in com.acme.UserTest
[ERROR] Tests run: 3, Failures: 1, Errors: 0, Skipped: 0
";
        let r = results(&mut JvmParser::new(), output);
        assert_eq!(status(&r, &["com.acme.OrderTest"]), Some(TestStatus::Failed));
        // Resolved by the frame's class (package directory + file), not by a bare file name.
        let expected = FileTarget {
            path: "com/acme/OrderTest.java".to_string(),
            line: Some(31),
            col: None,
            resolve: Resolve::Name,
            class_name: Some("com.acme.OrderTest".to_string()),
        };
        assert_eq!(location(&r, &["com.acme.OrderTest", "shouldTotal"]), Some(expected));
        assert_eq!(own(&r, &["com.acme.UserTest"]), Some(TestStatus::Passed));
        let summary = TestSummary { passed: Some(2), failed: Some(1), skipped: Some(0), duration_ms: None };
        assert_eq!(r.summary, summary);
    }

    #[test]
    fn locates_a_testng_failure_in_the_test_past_testngs_own_frames() {
        let output = "[INFO] Running com.acme.PriceTest
[ERROR] Tests run: 1, Failures: 1, Errors: 0, Skipped: 0, Time elapsed: 0.2 s <<< FAILURE! -- in com.acme.PriceTest
[ERROR] com.acme.PriceTest.rounds -- Time elapsed: 0.01 s <<< FAILURE!
java.lang.AssertionError: expected [3] but found [2]
\tat org.testng.Assert.fail(Assert.java:111)
\tat org.testng.Assert.assertEquals(Assert.java:1080)
\tat com.acme.PriceTest.rounds(PriceTest.java:12)
";
        let r = results(&mut JvmParser::new(), output);
        let located = location(&r, &["com.acme.PriceTest", "rounds"]).expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line), ("com/acme/PriceTest.java", Some(12)));
    }

    #[test]
    fn maps_config_kinds_and_commands_to_frameworks() {
        assert_eq!(framework_for(&cfg("cargo", "cargo", &["test"])), Framework::Cargo);
        assert_eq!(framework_for(&cfg("npm", "pnpm", &["run", "test"])), Framework::Js);
        assert_eq!(framework_for(&cfg("pytest", "uv", &["run", "pytest"])), Framework::Pytest);
        let manage = ["manage.py", "test"];
        assert_eq!(framework_for(&cfg("django-test", ".venv/bin/python", &manage)), Framework::Unittest);
        assert_eq!(framework_for(&cfg("shell", "python3 manage.py test polls", &[])), Framework::Unittest);
        assert_eq!(framework_for(&cfg("python", ".venv/bin/python", &manage)), Framework::Unittest);
        assert_eq!(framework_for(&cfg("python", ".venv/bin/python", &["-m", "pytest"])), Framework::Pytest);
        assert_eq!(framework_for(&cfg("go", "go", &["test", "./..."])), Framework::Go);
        assert_eq!(framework_for(&cfg("gradle", "./gradlew", &["test"])), Framework::Jvm);
        assert_eq!(framework_for(&cfg("shell", "npx vitest run", &[])), Framework::Js);
        assert_eq!(framework_for(&cfg("make", "make", &["test"])), Framework::Auto);
    }

    #[test]
    fn the_combined_parser_understands_any_supported_format() {
        let mut parser = create_test_parser(&cfg("make", "make", &["test"]));
        let output = "test a ... ok\n--- PASS: TestB (0.00s)\nok  \tpkg\t0.1s\n\
                      test_c (app.tests.CTests.test_c) ... ok\nFinished (0.5s)\ncompile (gcc.exe)";
        let r = results(parser.as_mut(), output);
        assert_eq!(own(&r, &["a"]), Some(TestStatus::Passed));
        assert_eq!(own(&r, &["app.tests", "CTests", "test_c"]), Some(TestStatus::Passed));
        // Lines of other tools that only look like a unittest header make no tests.
        assert!(!root_names(&r).contains(&"0"));
        assert!(!root_names(&r).contains(&"gcc"));
        assert_eq!(own(&r, &["pkg", "TestB"]), Some(TestStatus::Passed));
    }

    #[test]
    fn marks_unfinished_tests_when_the_process_ends() {
        let mut r = TestResults::new();
        r.apply(&[TestEvent::start(&test_path(&["s", "a"]))]);
        assert_eq!(status(&r, &["s"]), Some(TestStatus::Running));
        r.finish(Some(1));
        assert_eq!(r.get("s\0a").and_then(|node| node.own), Some(TestStatus::Skipped));
    }
}
