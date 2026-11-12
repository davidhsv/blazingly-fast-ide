//! PHPUnit (and Pest, which runs on it) through `--teamcity`: one `##teamcity[…]` service message per
//! suite and test event, with the exact `Class::method` names and failure details. Tests are reported as
//! `[...suites, test]`: the configured suite (`Unit`), the class, then the method (with its data set).

use crate::run::links::Resolve;
use crate::run_testing::teamcity::service_message;
use crate::run_testing::types::{TestCaseStatus, TestEvent, TestOutputParser, file_location};
use std::collections::HashMap;

// `php_qn:///app/tests/FooTest.php::\App\FooTest::testBar` → the file.
pattern!(HINT_FILE, r"^(?:php|pest)_qn://(.+?\.php)(?:::|$)");
pattern!(FRAME, r"(?:^|\s)((?:[A-Za-z]:)?[^\s:]+\.php):([0-9]+)\s*$");
pattern!(VENDOR, r"[/\\]vendor[/\\]");
pattern!(RUNNER, r"(^|[/\\])(phpunit|pest)(\.phar|\.bat)?$");
pattern!(OWN_FORMAT, r"^--(testdox|log-|no-output)");
pattern!(CLASS_NAME, r"^\\?[A-Z][A-Za-z0-9_]*(?:\\[A-Za-z0-9_]+)*$");
pattern!(METHOD_NAME, r"^[a-z_][A-Za-z0-9_]*$");
pattern!(DATA_SET, r" with data set .*$");

fn hint_file(hint: &str) -> Option<String> {
    Some(HINT_FILE.captures(hint)?[1].to_string())
}

/// First stack frame inside the project (not `vendor/`): `/app/tests/FooTest.php:12`.
fn failure_location(details: &str) -> Option<(String, Option<u32>)> {
    details.split('\n').find_map(|line| {
        let frame = FRAME.captures(line.trim()).filter(|frame| !VENDOR.is_match(&frame[1]))?;
        Some((frame[1].to_string(), frame[2].parse().ok()))
    })
}

/// A started test.
#[derive(Clone, Debug)]
struct OpenTest {
    path: Vec<String>,
    file: Option<String>,
    /// It failed or was ignored: it has its result.
    done: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PhpunitParser {
    /// Open suites, outermost first (the data-provider suite `Class::method` is one of them).
    suites: Vec<String>,
    open: HashMap<String, OpenTest>,
}

impl PhpunitParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// A data-provider suite is named `Class::method`; its data sets belong under the method (the
    /// class is open already).
    fn suite_path(&self) -> Vec<String> {
        let name = |suite: &String| match suite.rfind("::") {
            Some(at) if at > 0 => suite[at + 2..].to_string(),
            _ => suite.clone(),
        };
        self.suites.iter().map(name).collect()
    }
}

impl TestOutputParser for PhpunitParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let Some(message) = service_message(line) else { return Vec::new() };
        let name = message.attr("name");
        let mut events = Vec::new();
        match message.kind.as_str() {
            "testSuiteStarted" => self.suites.push(name.to_string()),
            "testSuiteFinished" => {
                if self.suites.last().is_some_and(|open| open == name) {
                    self.suites.pop();
                }
            }
            "testStarted" => {
                let mut path = self.suite_path();
                path.push(name.to_string());
                events.push(TestEvent::start(&path));
                let file = hint_file(message.attr("locationHint"));
                self.open.insert(name.to_string(), OpenTest { path, file, done: false });
            }
            kind @ ("testFailed" | "testIgnored") => {
                let Some(test) = self.open.get_mut(name) else { return events };
                let details = message.attr("details");
                let parts = [message.attr("message"), details];
                let text = parts.into_iter().filter(|part| !part.is_empty()).collect::<Vec<_>>().join("\n");
                events.extend(text.split('\n').map(|line| TestEvent::output(&test.path, line)));
                let at = failure_location(details).or_else(|| test.file.clone().map(|file| (file, None)));
                if let Some((file, line)) = at {
                    let line = line.filter(|line| *line > 0);
                    events.push(TestEvent::location(&test.path, file_location(&file, line, None, Resolve::Path)));
                }
                let status = if kind == "testIgnored" { TestCaseStatus::Skipped } else { TestCaseStatus::Failed };
                events.push(TestEvent::result(&test.path, status, None));
                test.done = true;
            }
            "testFinished" => {
                let Some(test) = self.open.remove(name) else { return events };
                // A failed or ignored test already has its result; the finish only reports a pass.
                if !test.done {
                    let duration = Some(message.attr("duration")).filter(|duration| !duration.is_empty());
                    let duration_ms = duration.and_then(|duration| duration.parse().ok());
                    events.push(TestEvent::result(&test.path, TestCaseStatus::Passed, duration_ms));
                }
            }
            _ => {}
        }
        events
    }
}

/// Options that take a value in a separate argument.
const VALUE_OPTIONS: [&str; 10] = [
    "-c",
    "--configuration",
    "--bootstrap",
    "--filter",
    "--testsuite",
    "--group",
    "--exclude-group",
    "--covers",
    "--uses",
    "--cache-directory",
];

/// Number of leading runner arguments (`vendor/bin/phpunit` under `php`); PHPUnit's own options follow.
fn runner_length(args: &[String]) -> usize {
    usize::from(args.first().is_some_and(|first| RUNNER.is_match(first)))
}

/// Splits the arguments after the runner into options and positional targets.
fn split_args(rest: &[String], drop_filter: bool) -> (Vec<String>, Vec<String>) {
    let (mut options, mut targets) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < rest.len() {
        let arg = &rest[i];
        if !arg.starts_with('-') {
            targets.push(arg.clone());
        } else if VALUE_OPTIONS.contains(&arg.as_str()) && i + 1 < rest.len() {
            if !(drop_filter && arg == "--filter") {
                options.extend([arg.clone(), rest[i + 1].clone()]);
            }
            i += 1;
        } else if !(drop_filter && arg.starts_with("--filter=")) {
            options.push(arg.clone());
        }
        i += 1;
    }
    (options, targets)
}

/// `args` running `target` (a file or directory) and/or a `--filter` pattern in place of the command's
/// own. PHPUnit stops parsing options at the first path, so everything is placed before it.
pub fn phpunit_args(args: &[String], target: Option<&str>, filter: Option<&str>) -> Vec<String> {
    let runner = runner_length(args);
    let (options, targets) = split_args(&args[runner..], filter.is_some());
    let mut out = args[..runner].to_vec();
    out.extend(options);
    if let Some(filter) = filter {
        out.extend(["--filter".to_string(), filter.to_string()]);
    }
    match target {
        Some(target) => out.push(target.to_string()),
        None => out.extend(targets),
    }
    out
}

/// `--teamcity` ahead of any path, unless the run already picked a result format.
pub fn with_teamcity(args: &[String]) -> Vec<String> {
    if args.iter().any(|arg| arg == "--teamcity" || OWN_FORMAT.is_match(arg)) {
        return args.to_vec();
    }
    let runner = runner_length(args);
    let mut out = args[..runner].to_vec();
    out.push("--teamcity".to_string());
    out.extend_from_slice(&args[runner..]);
    out
}

/// `text` as a literal in a `--filter` pattern (a regular expression between `/`).
fn escape_pattern(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ".*+?^${}()|[]\\/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Whether `name` under `parent` is a test rather than a class: a method name, a Pest description, or
/// anything under a namespaced class.
fn is_test_name(name: &str, parent: &str) -> bool {
    METHOD_NAME.is_match(name) || !CLASS_NAME.is_match(name) || (CLASS_NAME.is_match(parent) && parent.contains('\\'))
}

/// `--filter` pattern selecting the tests at `paths` (`[suite, class, method]`, a data set under its
/// method). PHPUnit matches `Class::method with data set …`; a class or suite node selects by its name.
pub fn phpunit_filter(paths: &[Vec<String>]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for path in paths {
        let last = path.last().map_or("", String::as_str);
        let data_set = DATA_SET.is_match(last);
        let method = DATA_SET.replace(last, "");
        // A data set sits under a node named after its method (the data-provider suite).
        let class = path.len().checked_sub(if data_set { 3 } else { 2 }).and_then(|at| path.get(at));
        let part = match class.filter(|class| !class.is_empty() && is_test_name(&method, class)) {
            Some(class) => format!("{}::{}( with data set .*)?", escape_pattern(class), escape_pattern(&method)),
            None => format!("{}::.*", escape_pattern(last)),
        };
        if !parts.contains(&part) {
            parts.push(part);
        }
    }
    format!("/({})$/", parts.join("|"))
}

/// `--filter` pattern of a Pest test `name` inside its `describes`, or of everything in them.
pub fn pest_filter(describes: &[String], name: Option<&str>) -> String {
    let prefix: String = describes.iter().map(|describe| format!("`{describe}` → ")).collect();
    match name {
        None => format!("/::{}/", escape_pattern(&prefix)),
        Some(name) => format!("/::{}( with data set .*)?$/", escape_pattern(&format!("{prefix}{name}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run_testing::parsers::{Framework, create_test_parser, framework_for, with_test_reporting};
    use crate::run_testing::results::TestResults;
    use crate::run_testing::teamcity::teamcity_output;
    use crate::run_testing::types::test_path;
    use ide_run::RunConfig;

    const SUITE: &str = r"App\Tests\CalcTest";

    fn run(lines: &[&str]) -> Vec<TestEvent> {
        let mut parser = PhpunitParser::new();
        lines.iter().flat_map(|line| parser.feed(line)).collect()
    }

    fn cfg(kind: &str, command: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(format!("{kind}:test"), format!("{kind} test"), kind);
        config.command = command.to_string();
        config.args = test_path(args);
        config
    }

    fn filter(path: &[&str]) -> String {
        phpunit_filter(&[test_path(path)])
    }

    #[test]
    fn reports_passed_failed_and_skipped_tests_with_locations() {
        let suite_started = format!(
            r"##teamcity[testSuiteStarted name='{SUITE}' locationHint='php_qn:///app/tests/CalcTest.php::\{SUITE}' flowId='1']"
        );
        let suite_finished = format!("##teamcity[testSuiteFinished name='{SUITE}' flowId='1']");
        let events = run(&[
            "PHPUnit 11.0.0 by Sebastian Bergmann and contributors.",
            r"##teamcity[testSuiteStarted name='Unit' locationHint='php_qn:///app/phpunit.xml::\Unit' flowId='1']",
            &suite_started,
            r"##teamcity[testStarted name='testAdd' locationHint='php_qn:///app/tests/CalcTest.php::\App\Tests\CalcTest::testAdd' flowId='1']",
            "##teamcity[testFinished name='testAdd' duration='3' flowId='1']",
            r"##teamcity[testStarted name='testSub' locationHint='php_qn:///app/tests/CalcTest.php::\App\Tests\CalcTest::testSub' flowId='1']",
            "##teamcity[testFailed name='testSub' message='Failed asserting that 1 matches expected 2.' details=' /app/vendor/phpunit/Assert.php:100|n /app/tests/CalcTest.php:21|n' flowId='1']",
            "##teamcity[testFinished name='testSub' duration='1' flowId='1']",
            "##teamcity[testStarted name='testSkip' flowId='1']",
            "##teamcity[testIgnored name='testSkip' message='not yet' flowId='1']",
            "##teamcity[testFinished name='testSkip' duration='0' flowId='1']",
            &suite_finished,
            "##teamcity[testSuiteFinished name='Unit' flowId='1']",
        ]);
        let add = test_path(&["Unit", SUITE, "testAdd"]);
        let sub = test_path(&["Unit", SUITE, "testSub"]);
        let skip = test_path(&["Unit", SUITE, "testSkip"]);
        assert!(events.contains(&TestEvent::result(&add, TestCaseStatus::Passed, Some(3.0))));
        assert!(events.contains(&TestEvent::result(&sub, TestCaseStatus::Failed, None)));
        assert!(events.contains(&TestEvent::result(&skip, TestCaseStatus::Skipped, None)));
        // A failure points at the first frame outside vendor/.
        let frame = file_location("/app/tests/CalcTest.php", Some(21), None, Resolve::Path);
        assert!(events.contains(&TestEvent::location(&sub, frame)));
        assert!(events.contains(&TestEvent::output(&sub, "Failed asserting that 1 matches expected 2.")));
        // The finish of a failed test doesn't turn it into a pass.
        let results_of_sub = |event: &&TestEvent| matches!(event, TestEvent::Result { path, .. } if *path == sub);
        assert_eq!(events.iter().filter(results_of_sub).count(), 1);
    }

    #[test]
    fn nests_data_sets_under_their_method() {
        let events = run(&[
            &format!("##teamcity[testSuiteStarted name='{SUITE}']"),
            &format!("##teamcity[testSuiteStarted name='{SUITE}::testAdd']"),
            "##teamcity[testStarted name='testAdd with data set #0']",
            "##teamcity[testFinished name='testAdd with data set #0' duration='1']",
            &format!("##teamcity[testSuiteFinished name='{SUITE}::testAdd']"),
        ]);
        let data_set = test_path(&[SUITE, "testAdd", "testAdd with data set #0"]);
        assert_eq!(events[0], TestEvent::start(&data_set));
    }

    #[test]
    fn reads_pests_names_and_location_hints() {
        let events = run(&[
            "##teamcity[testSuiteStarted name='Unit' locationHint='pest_qn://tests/Unit/MathTest.php' flowId='1']",
            r"##teamcity[testSuiteStarted name='Tests\Unit\MathTest' locationHint='pest_qn://tests/Unit/MathTest.php' flowId='1']",
            "##teamcity[testStarted name='`division` → it divides' locationHint='pest_qn://tests/Unit/MathTest.php::`division` → it divides' flowId='1']",
            "##teamcity[testFailed name='`division` → it divides' message='Failed asserting that 1 is identical to 2.' details='' flowId='1']",
            "##teamcity[testSuiteStarted name='it doubles' locationHint='pest_qn://tests/Unit/MathTest.php::it doubles' flowId='1']",
            r#"##teamcity[testStarted name='it doubles with data set "(1)"' locationHint='pest_qn://tests/Unit/MathTest.php::it doubles with data set "(1)"' flowId='1']"#,
        ]);
        let divides = test_path(&["Unit", r"Tests\Unit\MathTest", "`division` → it divides"]);
        let hinted = file_location("tests/Unit/MathTest.php", None, None, Resolve::Path);
        assert!(events.contains(&TestEvent::location(&divides, hinted)));
        let doubles = test_path(&["Unit", r"Tests\Unit\MathTest", "it doubles", r#"it doubles with data set "(1)""#]);
        assert!(events.contains(&TestEvent::start(&doubles)));
    }

    #[test]
    fn unescapes_teamcity_values() {
        let events = run(&[
            "##teamcity[testSuiteStarted name='S']",
            "##teamcity[testStarted name='t']",
            "##teamcity[testFailed name='t' message='a |'quoted|' |[x|] b' details='']",
        ]);
        assert!(events.contains(&TestEvent::output(&test_path(&["S", "t"]), "a 'quoted' [x] b")));
    }

    #[test]
    fn ignores_everything_else() {
        assert!(run(&["Time: 00:00.012, Memory: 8.00 MB", "OK (1 test, 1 assertion)", ""]).is_empty());
    }

    #[test]
    fn builds_a_tree() {
        let mut results = TestResults::new();
        results.apply(&run(&[
            "##teamcity[testSuiteStarted name='S']",
            "##teamcity[testStarted name='a']",
            "##teamcity[testFinished name='a' duration='1']",
            "##teamcity[testStarted name='b']",
            "##teamcity[testFailed name='b' message='x' details='']",
            "##teamcity[testFinished name='b' duration='1']",
        ]));
        let counts = results.counts();
        assert_eq!((counts.passed, counts.failed), (1, 1));
    }

    #[test]
    fn feeds_every_line_to_the_tree_but_keeps_service_messages_out_of_the_console() {
        let mut results = TestResults::new();
        let mut parser = PhpunitParser::new();
        let mut output = teamcity_output();
        let chunks = [
            "PHPUnit 11.5 by Sebastian Bergmann\n##teamcity[testSuiteStarted name='CalcTest']\n##teamcity[testStar",
            "ted name='testAdd']\r\n##teamcity[testFinished name='testAdd' duration='1']\nOK (1 test",
        ];
        let mut fed: Vec<_> = chunks.iter().map(|chunk| output.feed(chunk)).collect();
        fed.push(output.end());
        let mut shown = String::new();
        for part in fed {
            shown.push_str(&part.text);
            for line in &part.lines {
                results.apply(&parser.feed(line));
            }
        }
        assert_eq!(shown, "PHPUnit 11.5 by Sebastian Bergmann\nOK (1 test\n");
        assert_eq!(results.counts().passed, 1);
    }

    #[test]
    fn puts_teamcity_before_any_path_after_the_runner() {
        let with = |args: &[&str]| with_teamcity(&test_path(args));
        assert_eq!(with(&["tests/FooTest.php"]), ["--teamcity", "tests/FooTest.php"]);
        assert_eq!(
            with(&["vendor/bin/phpunit", "--stop-on-failure", "tests"]),
            ["vendor/bin/phpunit", "--teamcity", "--stop-on-failure", "tests"]
        );
        assert_eq!(with(&["--testdox"]), ["--testdox"]);
        assert_eq!(with(&["--teamcity"]), ["--teamcity"]);
    }

    #[test]
    fn detects_phpunit_configurations() {
        assert_eq!(framework_for(&cfg("phpunit", "vendor/bin/phpunit", &[])), Framework::Phpunit);
        assert_eq!(framework_for(&cfg("composer", "composer", &["run-script", "test"])), Framework::Auto);
        assert_eq!(framework_for(&cfg("shell", "php", &["vendor/bin/phpunit"])), Framework::Phpunit);
        assert_eq!(framework_for(&cfg("shell", "vendor/bin/pest", &["--parallel"])), Framework::Phpunit);
        let mut parser = create_test_parser(&cfg("phpunit", "phpunit", &[]));
        assert_eq!(parser.feed("##teamcity[testStarted name='t']"), [TestEvent::start(&test_path(&["t"]))]);
        let reported = with_test_reporting(&cfg("phpunit", "phpunit", &["tests"]));
        assert_eq!(reported.args, ["--teamcity", "tests"]);
    }

    #[test]
    fn replaces_the_target_and_filter_keeping_options() {
        let args = |args: &[&str], target, filter| phpunit_args(&test_path(args), target, filter);
        assert_eq!(
            args(&["-c", "ci.xml", "tests"], Some("tests/FooTest.php"), None),
            ["-c", "ci.xml", "tests/FooTest.php"]
        );
        assert_eq!(
            args(&["--filter", "old", "--colors=never", "tests"], None, Some("/x/")),
            ["--colors=never", "--filter", "/x/", "tests"]
        );
        assert_eq!(
            args(&["vendor/bin/phpunit", "tests"], Some("a.php"), Some("/x/")),
            ["vendor/bin/phpunit", "--filter", "/x/", "a.php"]
        );
    }

    #[test]
    fn filters_by_class_and_method() {
        let method = r"/(App\\Tests\\CalcTest::testAdd( with data set .*)?)$/";
        assert_eq!(filter(&["Unit", SUITE, "testAdd"]), method);
        assert_eq!(filter(&["Unit", SUITE, "testAdd", "testAdd with data set #0"]), method);
        assert_eq!(filter(&[SUITE]), r"/(App\\Tests\\CalcTest::.*)$/");
    }

    #[test]
    fn filters_pest_tests_data_sets_and_their_class_by_the_names_pest_prints() {
        let class = r"Tests\Unit\MathTest";
        assert_eq!(
            filter(&["Unit", class, "it fails on purpose"]),
            r"/(Tests\\Unit\\MathTest::it fails on purpose( with data set .*)?)$/"
        );
        assert_eq!(
            filter(&["Unit", class, "`division` → `by zero` → Throws"]),
            r"/(Tests\\Unit\\MathTest::`division` → `by zero` → Throws( with data set .*)?)$/"
        );
        assert_eq!(filter(&["Unit", class, "Adds"]), r"/(Tests\\Unit\\MathTest::Adds( with data set .*)?)$/");
        assert_eq!(
            filter(&["Unit", class, "it doubles", r#"it doubles with data set "(2)""#]),
            r"/(Tests\\Unit\\MathTest::it doubles( with data set .*)?)$/"
        );
        assert_eq!(filter(&["Unit", class]), r"/(Tests\\Unit\\MathTest::.*)$/");
        assert_eq!(filter(&["/app/phpunit.xml", "Unit"]), "/(Unit::.*)$/");
    }
}
