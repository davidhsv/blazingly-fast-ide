//! `go test`: its JSON records (`-json`), and the plain and `-v` text. In the text the package name
//! only appears in the trailing `ok`/`FAIL` line, so events are buffered per package and emitted
//! under it once that line arrives.

use crate::run::links::Resolve;
use crate::run_testing::results::node_id;
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestStdout, TestSummary, file_location, parse_duration,
};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

pattern!(RUN, r"^=== (?:RUN|CONT|PAUSE)\s+(\S+)");
pattern!(RESULT, r"^\s*--- (PASS|FAIL|SKIP): (\S+) \(([0-9.]+s)\)");
pattern!(PACKAGE, r"^(ok|FAIL|---)\s+(\S+)\s+(?:\(cached\)|([0-9.]+s)|\[[^\]]+\])");
pattern!(LOCATION, r"^\s+([A-Za-z0-9_.-]+\.go):([0-9]+):");
pattern!(BENCHMARK, r"(?m)^(Benchmark\S*) *\t[ \t]*[0-9]+ *(?:\t|$)");
pattern!(RECORD_LOCATION, r"^\s*(.+\.go):([0-9]+)(?::([0-9]+))?:");
pattern!(CPU_SUFFIX, r"-[0-9]+$");

/// Passed, failed and skipped tests so far.
#[derive(Clone, Copy, Debug, Default)]
struct Totals {
    passed: u32,
    failed: u32,
    skipped: u32,
}

impl Totals {
    fn add(&mut self, status: TestCaseStatus) {
        match status {
            TestCaseStatus::Passed => self.passed += 1,
            TestCaseStatus::Failed => self.failed += 1,
            TestCaseStatus::Skipped => self.skipped += 1,
        }
    }

    fn summary(self) -> TestEvent {
        TestEvent::Summary(TestSummary {
            passed: Some(self.passed),
            failed: Some(self.failed),
            skipped: Some(self.skipped),
            duration_ms: None,
        })
    }
}

fn split(name: &str) -> Vec<String> {
    name.split('/').map(str::to_string).collect()
}

/// The text output: tests are reported once their package's line names the package.
#[derive(Clone, Debug, Default)]
struct PlainGoParser {
    buffer: Vec<TestEvent>,
    current: Option<Vec<String>>,
    totals: Totals,
    located: HashSet<String>,
}

impl PlainGoParser {
    fn flush(&mut self, package: Option<&str>) -> Vec<TestEvent> {
        self.current = None;
        self.located.clear();
        let buffered = std::mem::take(&mut self.buffer).into_iter();
        let Some(package) = package else { return buffered.collect() };
        let under = |path: &[String]| -> Vec<String> {
            std::iter::once(package.to_string()).chain(path.iter().cloned()).collect()
        };
        buffered
            .map(|event| {
                let path = event.path().map(under);
                match path {
                    Some(path) => event.with_path(path),
                    None => event,
                }
            })
            .collect()
    }
}

impl TestOutputParser for PlainGoParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        if let Some(found) = RUN.captures(line) {
            let path = split(&found[1]);
            self.buffer.push(TestEvent::start(&path));
            self.current = Some(path);
            return Vec::new();
        }
        if let Some(found) = RESULT.captures(line) {
            let status = match &found[1] {
                "PASS" => TestCaseStatus::Passed,
                "FAIL" => TestCaseStatus::Failed,
                _ => TestCaseStatus::Skipped,
            };
            let path = split(&found[2]);
            self.buffer.push(TestEvent::result(&path, status, parse_duration(&found[3])));
            if path.len() == 1 {
                self.totals.add(status);
            }
            self.current = Some(path);
            return Vec::new();
        }
        if let Some(found) = PACKAGE.captures(line).filter(|found| &found[1] != "---") {
            let package = &found[2];
            let mut events = self.flush(Some(package));
            // Without -v a passing package prints nothing per test: show the package itself.
            if !events.iter().any(|event| matches!(event, TestEvent::Result { .. })) {
                let status = if &found[1] == "ok" { TestCaseStatus::Passed } else { TestCaseStatus::Failed };
                let duration = found.get(3).and_then(|duration| parse_duration(duration.as_str()));
                events.push(TestEvent::result(&[package.to_string()], status, duration));
            }
            events.push(self.totals.summary());
            return events;
        }
        if let Some(current) = &self.current
            && line.starts_with(char::is_whitespace)
        {
            self.buffer.push(TestEvent::output(current, line));
            let key = current.join("/");
            if let Some(found) = LOCATION.captures(line)
                && self.located.insert(key)
            {
                let location = file_location(&found[1], found[2].parse().ok(), None, Resolve::Name);
                self.buffer.push(TestEvent::location(current, location));
            }
        }
        Vec::new()
    }

    fn end(&mut self) -> Vec<TestEvent> {
        self.flush(None)
    }
}

/// A record of `go test -json` (test2json), or of the build that precedes the tests.
#[derive(Clone, Debug, PartialEq)]
pub struct GoEvent {
    pub action: String,
    pub package: String,
    pub test: Option<String>,
    pub output: Option<String>,
    /// Seconds.
    pub elapsed: Option<f64>,
}

const ACTIONS: [&str; 11] =
    ["start", "run", "pause", "cont", "output", "pass", "bench", "fail", "skip", "build-output", "build-fail"];

/// The record on `line`; `None` for text and for records that are not about a package's tests.
pub fn go_json_event(line: &str) -> Option<GoEvent> {
    if !line.starts_with('{') {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let record = value.as_object()?;
    let action = record.get("Action")?.as_str()?;
    let package = record.get("Package").filter(|package| !package.is_null()).or_else(|| record.get("ImportPath"))?;
    let package = package.as_str().filter(|package| !package.is_empty())?;
    if !ACTIONS.contains(&action) {
        return None;
    }
    // A field that is present must be text.
    let text = |key: &str| match record.get(key) {
        None => Some(None),
        Some(Value::String(text)) => Some(Some(text.clone())),
        Some(_) => None,
    };
    let elapsed = record.get("Elapsed").and_then(Value::as_f64);
    Some(GoEvent {
        action: action.to_string(),
        package: package.to_string(),
        test: text("Test")?,
        output: text("Output")?,
        elapsed,
    })
}

#[derive(Clone, Debug, Default)]
struct GoPackage {
    tests: bool,
    failed: bool,
    /// Benchmark names in the order they started, with whether Go reported a successful measurement
    /// or bench event.
    benchmarks: Vec<(String, bool)>,
}

impl GoPackage {
    fn has_benchmark(&self, name: &str) -> bool {
        self.benchmarks.iter().any(|(known, _)| known == name)
    }

    fn set_benchmark(&mut self, name: &str, measured: bool) {
        match self.benchmarks.iter_mut().find(|(known, _)| known == name) {
            Some(benchmark) => benchmark.1 = measured,
            None => self.benchmarks.push((name.to_string(), measured)),
        }
    }

    /// The started benchmark a reported name stands for: results carry a `-<GOMAXPROCS>` suffix.
    fn benchmark_name(&self, name: &str) -> String {
        if self.has_benchmark(name) {
            return name.to_string();
        }
        let without_cpu = CPU_SUFFIX.replace(name, "");
        if self.has_benchmark(&without_cpu) { without_cpu.into_owned() } else { name.to_string() }
    }
}

/// Reports tests as `[package, test, ...subtests]`, and a package without tests as itself.
#[derive(Clone, Debug, Default)]
pub struct GoParser {
    plain: PlainGoParser,
    located: HashSet<String>,
    finished: HashSet<String>,
    packages: HashMap<String, GoPackage>,
    totals: Totals,
}

impl GoParser {
    pub fn new() -> Self {
        Self::default()
    }
}

/// The result of the test at `path`, once per start.
fn finish(
    finished: &mut HashSet<String>,
    totals: &mut Totals,
    package: &mut GoPackage,
    path: &[String],
    status: TestCaseStatus,
    duration_ms: Option<f64>,
) -> Option<TestEvent> {
    if !finished.insert(node_id(path)) {
        return None;
    }
    let failed = status == TestCaseStatus::Failed;
    if path.len() == 2 {
        totals.add(status);
        package.failed |= failed;
    } else if path.len() == 1 && (!package.tests || (failed && !package.failed)) {
        totals.add(status);
    }
    Some(TestEvent::result(path, status, duration_ms))
}

impl TestOutputParser for GoParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let Some(record) = go_json_event(line) else { return self.plain.feed(line) };
        let Self { located, finished, packages, totals, .. } = self;
        let package = packages.entry(record.package.clone()).or_default();
        let test = record.test.as_deref().filter(|test| !test.is_empty());
        let benchmark = test.filter(|test| test.starts_with("Benchmark"));
        let action = record.action.as_str();
        let name = match benchmark {
            Some(benchmark) if action != "run" => Some(package.benchmark_name(benchmark)),
            _ => test.map(str::to_string),
        };
        let under =
            |name: &str| -> Vec<String> { std::iter::once(record.package.clone()).chain(split(name)).collect() };
        let path = name.as_deref().map_or_else(|| vec![record.package.clone()], under);
        if name.is_some() {
            package.tests = true;
        }
        match action {
            "run" => {
                finished.remove(&node_id(&path));
                if let Some(benchmark) = benchmark {
                    package.set_benchmark(benchmark, false);
                }
                vec![TestEvent::start(&path)]
            }
            "bench" => {
                if let Some(name) = &name {
                    package.set_benchmark(name, true);
                }
                Vec::new()
            }
            "output" | "build-output" => {
                let Some(output) = record.output.as_deref().filter(|output| !output.is_empty()) else {
                    return Vec::new();
                };
                if action == "output"
                    && (name.is_none() || benchmark.is_some())
                    && let Some(measurement) = BENCHMARK.captures(output)
                {
                    let measured = package.benchmark_name(&measurement[1]);
                    if package.has_benchmark(&measured) && name.as_ref().is_none_or(|name| *name == measured) {
                        package.set_benchmark(&measured, true);
                    }
                }
                let lines: Vec<&str> = output.strip_suffix('\n').unwrap_or(output).split('\n').collect();
                let mut events: Vec<TestEvent> = lines.iter().map(|line| TestEvent::output(&path, line)).collect();
                if let Some(found) = lines.iter().find_map(|line| RECORD_LOCATION.captures(line))
                    && located.insert(node_id(&path))
                {
                    let col = found.get(3).and_then(|col| col.as_str().parse().ok());
                    let resolve = if found[1].starts_with('/') { Resolve::Path } else { Resolve::Name };
                    events.push(TestEvent::location(
                        &path,
                        file_location(&found[1], found[2].parse().ok(), col, resolve),
                    ));
                }
                events
            }
            "pass" | "fail" | "skip" | "build-fail" => {
                let status = match action {
                    "pass" => TestCaseStatus::Passed,
                    "skip" => TestCaseStatus::Skipped,
                    _ => TestCaseStatus::Failed,
                };
                let duration_ms = record.elapsed.filter(|elapsed| elapsed.is_finite() && *elapsed >= 0.0);
                let duration_ms = duration_ms.map(|elapsed| elapsed * 1000.0);
                let mut events = Vec::new();
                if name.is_none() {
                    // Quiet benchmarks can emit measurements without a terminal event. Wait for the
                    // package result so a failure from a later CPU setting can take precedence.
                    for (benchmark, measured) in std::mem::take(&mut package.benchmarks) {
                        if measured {
                            let passed = TestCaseStatus::Passed;
                            events.extend(finish(finished, totals, package, &under(&benchmark), passed, None));
                        }
                    }
                }
                events.extend(finish(finished, totals, package, &path, status, duration_ms));
                if !events.is_empty() {
                    events.push(totals.summary());
                }
                events
            }
            _ => Vec::new(),
        }
    }

    fn end(&mut self) -> Vec<TestEvent> {
        self.plain.end()
    }
}

/// Stdout of `go test -json` for the console: a record shows as the output it carries, while the
/// test parser gets the complete, unsplit records.
pub fn go_test_output() -> TestStdout {
    TestStdout::new(|line| match go_json_event(line) {
        Some(record) => record.output.unwrap_or_default(),
        None => format!("{line}\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run::links::find_output_links;
    use crate::run_testing::parsers::with_test_reporting;
    use crate::run_testing::results::{TestCounts, TestResults};
    use crate::run_testing::types::{StdoutText, TestStatus};
    use serde_json::json;

    /// A record about `package`, or about its test `test` unless that is empty, with further fields.
    fn record_with(package: &str, action: &str, test: &str, rest: Value) -> String {
        let mut fields = serde_json::Map::new();
        fields.insert("Package".to_string(), json!(package));
        fields.insert("Action".to_string(), json!(action));
        if !test.is_empty() {
            fields.insert("Test".to_string(), json!(test));
        }
        if let Value::Object(rest) = rest {
            fields.extend(rest);
        }
        Value::Object(fields).to_string()
    }

    fn record(package: &str, action: &str, test: &str) -> String {
        record_with(package, action, test, json!({}))
    }

    fn output(package: &str, test: &str, text: &str) -> String {
        record_with(package, "output", test, json!({ "Output": text }))
    }

    fn feed_all(parser: &mut GoParser, results: &mut TestResults, lines: &[String]) {
        for line in lines {
            results.apply(&parser.feed(line));
        }
    }

    fn own(results: &TestResults, path: &[&str]) -> Option<TestStatus> {
        results.node_at(path)?.own
    }

    fn root_names(results: &TestResults) -> Vec<&str> {
        results.roots().into_iter().map(|root| root.name.as_str()).collect()
    }

    #[test]
    fn streams_interleaved_packages_and_subtests_with_exact_counts_and_locations() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let lines = [
            record("one", "run", "TestPass"),
            record("two", "run", "TestFail"),
            record("one", "run", "TestPass/child"),
            record_with("one", "pass", "TestPass/child", json!({ "Elapsed": 0.002 })),
            output("two", "TestFail", "    nested/main_test.go:12:7: fixture failure\n"),
            record_with("two", "fail", "TestFail", json!({ "Elapsed": 0.01 })),
            record("two", "fail", ""),
            record("one", "pass", "TestPass"),
            record("one", "skip", "TestSkip"),
            record("one", "pass", ""),
        ];
        feed_all(&mut parser, &mut results, &lines);
        results.finish(Some(1));
        assert_eq!(results.counts(), TestCounts { passed: 1, failed: 1, skipped: 1, running: 0, total: 3 });
        assert_eq!(root_names(&results), ["one", "two"]);
        let located = results.node_at(&["two", "TestFail"]).and_then(|node| node.location.clone());
        assert_eq!(located, Some(file_location("nested/main_test.go", Some(12), Some(7), Resolve::Name)));
        let child = results.node_at(&["one", "TestPass", "child"]).expect("the subtest is reported");
        assert_eq!(child.duration_ms, Some(2.0));
        assert!(parser.feed(&record("two", "fail", "TestFail")).is_empty());
    }

    #[test]
    fn reports_compiler_failures_and_packages_without_tests() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let build_output = json!({
            "ImportPath": "broken",
            "Action": "build-output",
            "Output": "./main.go:3:1: undefined: missing\n",
        });
        let lines = [
            build_output.to_string(),
            json!({ "ImportPath": "broken", "Action": "build-fail" }).to_string(),
            record("empty", "skip", ""),
        ];
        feed_all(&mut parser, &mut results, &lines);
        let counts = results.counts();
        assert_eq!((counts.failed, counts.skipped), (1, 1));
        let broken = results.roots()[0];
        assert!(broken.output.iter().any(|line| line == "./main.go:3:1: undefined: missing"));
    }

    #[test]
    fn counts_repeated_test_results_once_per_start_and_preserves_a_failed_attempt() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let lines = [
            record("p", "run", "TestRepeated"),
            record("p", "fail", "TestRepeated"),
            record("p", "run", "TestRepeated"),
            record("p", "pass", "TestRepeated"),
            record("p", "pass", "TestRepeated"),
            record("p", "fail", ""),
        ];
        feed_all(&mut parser, &mut results, &lines);
        assert_eq!(results.counts(), TestCounts { passed: 1, failed: 1, skipped: 0, running: 0, total: 2 });
        assert_eq!(own(&results, &["p", "TestRepeated"]), Some(TestStatus::Failed));
    }

    #[test]
    fn completes_benchmark_measurements_and_explicit_bench_events_in_interleaved_packages() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let lines = [
            record("one", "run", "BenchmarkQuiet"),
            record("two", "run", "BenchmarkLogged"),
            output("one", "BenchmarkQuiet", "BenchmarkQuiet-4  \t  1\t  214 ns/op\n"),
            output("two", "BenchmarkLogged", "    bench_test.go:4: logged\n"),
            record("two", "bench", "BenchmarkLogged-4"),
            output("one", "", "BenchmarkQuiet-8  \t  1\t  210 ns/op\n"),
            record("one", "run", "BenchmarkFails"),
            output("one", "BenchmarkFails", "BenchmarkFails-4  \t  1\t  220 ns/op\n"),
            record("one", "fail", "BenchmarkFails-8"),
            record("one", "fail", ""),
            record("two", "pass", ""),
        ];
        feed_all(&mut parser, &mut results, &lines);
        assert_eq!(results.counts(), TestCounts { passed: 2, failed: 1, skipped: 0, running: 0, total: 3 });
        assert_eq!(own(&results, &["one", "BenchmarkQuiet"]), Some(TestStatus::Passed));
        assert_eq!(own(&results, &["one", "BenchmarkFails"]), Some(TestStatus::Failed));
        assert_eq!(own(&results, &["two", "BenchmarkLogged"]), Some(TestStatus::Passed));
    }

    #[test]
    fn maps_benchmark_measurements_without_a_test_field_to_the_actual_nested_benchmark_name() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let lines = [
            record("p", "run", "BenchmarkNested"),
            record("p", "run", "BenchmarkNested/deep-2"),
            output("p", "", "BenchmarkNested/deep-2  \t  1\t  214 ns/op\n"),
            output("p", "", "BenchmarkNested/deep-2-4  \t  1\t  210 ns/op\n"),
            record("p", "pass", ""),
        ];
        feed_all(&mut parser, &mut results, &lines);
        assert_eq!(results.counts(), TestCounts { passed: 1, failed: 0, skipped: 0, running: 0, total: 1 });
        let nested = results.node_at(&["p", "BenchmarkNested"]).expect("the benchmark is reported");
        let children: Vec<&str> = results.children(nested).into_iter().map(|node| node.name.as_str()).collect();
        assert_eq!(children, ["deep-2"]);
        assert_eq!(own(&results, &["p", "BenchmarkNested", "deep-2"]), Some(TestStatus::Passed));
    }

    #[test]
    fn ignores_malformed_metadata_and_negative_durations_without_throwing() {
        let malformed = [
            "{",
            "null",
            "[]",
            r#"{"Action":"pass","Package":5}"#,
            r#"{"Action":"output","Package":"p","Output":{}}"#,
            r#"{"Action":"unknown","Package":"p"}"#,
        ];
        for line in malformed {
            assert_eq!(go_json_event(line), None);
            assert!(GoParser::new().feed(line).is_empty());
        }
        let negative = record_with("p", "pass", "TestOne", json!({ "Elapsed": -1 }));
        let events = GoParser::new().feed(&negative);
        assert!(matches!(events.first(), Some(TestEvent::Result { duration_ms: None, .. })));
    }

    #[test]
    fn decodes_chunked_and_long_json_output_into_clickable_console_text() {
        let mut parser = GoParser::new();
        let mut results = TestResults::new();
        let mut stdout = go_test_output();
        let mut feed = |fed: StdoutText| {
            for line in &fed.lines {
                results.apply(&parser.feed(line));
            }
            fed.text
        };
        let line = output("p", "TestFail", &format!("    main_test.go:6: failure {}\n", "x".repeat(9000)));
        assert_eq!(feed(stdout.feed(&line[..22])), "");
        let text = feed(stdout.feed(&format!("{}\n{}", &line[22..], record("p", "fail", "TestFail"))));
        assert!(text.contains("main_test.go:6: failure"));
        assert!(!text.contains("\"Action\""));
        assert!(find_output_links(&text[..100]).iter().any(|link| link.target().is_some()));
        assert_eq!(feed(stdout.end()), "");
        assert_eq!(results.counts().failed, 1);
    }

    #[test]
    fn forces_json_before_program_arguments_and_keeps_the_binary_arguments_untouched() {
        let mut config = blank_config("go:test", "go test", "go");
        config.command = "go".to_string();
        let args = ["test", "-json=false", "-tags", "demo", ".", "-args", "-json=false"];
        config.args = args.iter().map(|arg| arg.to_string()).collect();
        let reported = with_test_reporting(&config);
        assert_eq!(reported.args, ["test", "-json", "-tags", "demo", ".", "-args", "-json=false"]);
    }
}
