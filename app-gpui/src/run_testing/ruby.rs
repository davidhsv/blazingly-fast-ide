//! Minitest's verbose output (`-v`; Test::Unit's minitest shim prints the same): one
//! `Class#test_name = 0.00 s = .` line per test (`.` passed, `F` failed, `E` error, `S` skipped),
//! then the failure sections and the summary. Tests are reported as `[Class, test_name]`.
//!
//! ```text
//! MixedTest#test_prints = hello from test      <- the test's own output comes first,
//! 0.00 s = .                                      the result follows on a line of its own
//!
//!   1) Failure:
//!   MixedTest#test_fail [test/mixed_test.rb:5]:
//!   Expected: 4
//!     Actual: 3
//!
//! 7 runs, 5 assertions, 1 failures, 1 errors, 1 skips
//! ```
//!
//! With the IDE's own Minitest reporter the records it prints are the source of results instead.

use crate::run::links::{FileTarget, Resolve};
use crate::run_testing::results::node_id;
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestStdout, TestSummary, count, file_location, parse_duration,
};
use serde_json::Value;
use std::collections::HashSet;

// A class may be a spec description with spaces (`Spec style#test_0001_works`).
pattern!(RESULT, r"^(.+?)#(\S+) = ([0-9.]+ s) = ([.FES])$");
pattern!(STARTED, r"^(.+?)#(\S+) = (.*)$");
// The result of a test that printed output first.
pattern!(LATE_RESULT, r"^([0-9.]+ s) = ([.FES])$");
pattern!(SECTION, r"^\s*[0-9]+\) (Failure|Error|Skipped):$");
pattern!(SECTION_TEST, r"^(.+?)#(\S+?)(?: \[(.+?):([0-9]+)\])?:$");
pattern!(FRAME, r"^\s+(\S+?):([0-9]+):in ");
pattern!(SUMMARY, r"^([0-9]+) runs, [0-9]+ assertions, ([0-9]+) failures, ([0-9]+) errors, ([0-9]+) skips");
pattern!(FINISHED, r"^Finished in ([0-9.]+s)");
// Gems and the standard library never locate a failure.
pattern!(LIBRARY, r"[/\\](?:gems|rubygems|lib[/\\]ruby)[/\\]");
// A test method, or a spec's numbered example (`test_0001_works`).
pattern!(TEST_NAME, r"^(test|[A-Za-z0-9_]*_[0-9]+)");

fn status(mark: &str) -> TestCaseStatus {
    match mark {
        "." => TestCaseStatus::Passed,
        "S" => TestCaseStatus::Skipped,
        _ => TestCaseStatus::Failed,
    }
}

/// Minitest names no file for a passing test: its location is searched for when the user jumps to it.
fn located(path: &[String]) -> TestEvent {
    let location = FileTarget {
        path: path.get(1).cloned().unwrap_or_default(),
        line: None,
        col: None,
        resolve: Resolve::RubyTest,
        class_name: path.first().cloned(),
    };
    TestEvent::location(path, location)
}

/// Starts a record of the IDE's own Minitest reporter; a JSON object follows.
pub const RECORD_MARK: &str = "@@OPENIDEA@@";

/// A count of the reporter's summary; one it does not give is zero.
fn record_count(record: &Value, key: &str) -> u32 {
    record.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

/// The reporter's time (seconds) in milliseconds, to the microsecond.
fn record_duration(record: &Value) -> Option<f64> {
    let seconds = record.get("d")?.as_f64()?;
    Some((seconds * 1000.0 * 1000.0).round() / 1000.0)
}

/// What a reporter record says: the test's real file and line, its status, time and failure text.
fn from_record(json: &str) -> Vec<TestEvent> {
    let Ok(record) = serde_json::from_str::<Value>(json) else { return Vec::new() };
    let text = |key: &str| record.get(key).and_then(Value::as_str).filter(|text| !text.is_empty());
    let kind = text("t");
    if kind == Some("summary") {
        let failed = record_count(&record, "failures") + record_count(&record, "errors");
        let skipped = record_count(&record, "skips");
        return vec![TestEvent::Summary(TestSummary {
            passed: Some(record_count(&record, "runs").saturating_sub(failed + skipped)),
            failed: Some(failed),
            skipped: Some(skipped),
            duration_ms: record_duration(&record),
        })];
    }
    let (Some(class), Some(name)) = (text("c"), text("n")) else { return Vec::new() };
    let path = vec![class.to_string(), name.to_string()];
    let mut events = Vec::new();
    if kind == Some("start") {
        events.push(TestEvent::start(&path));
    }
    let status = match text("s") {
        Some("passed") => Some(TestCaseStatus::Passed),
        Some("failed") => Some(TestCaseStatus::Failed),
        Some("skipped") => Some(TestCaseStatus::Skipped),
        _ => None,
    };
    if kind == Some("result")
        && let Some(status) = status
    {
        events.push(TestEvent::result(&path, status, record_duration(&record)));
        let message = text("m").unwrap_or_default();
        events.extend(message.split('\n').filter(|line| !line.is_empty()).map(|line| TestEvent::output(&path, line)));
    }
    let line = record.get("l").and_then(Value::as_u64).and_then(|line| u32::try_from(line).ok());
    if let (Some(file), Some(line)) = (text("f"), line.filter(|line| *line > 0)) {
        events.push(TestEvent::location(&path, file_location(file, Some(line), None, Resolve::Path)));
    }
    events
}

/// The console side of the reporter: a record ends with its own newline, so what the line holds before
/// the marker (a progress dot) is all the console shows of it.
pub fn ruby_test_output() -> TestStdout {
    TestStdout::new(|line| match line.find(RECORD_MARK) {
        Some(at) => line[..at].to_string(),
        None => format!("{line}\n"),
    })
}

/// A failure section being printed.
#[derive(Clone, Debug)]
struct Section {
    path: Option<Vec<String>>,
    located: bool,
    /// The next line names the section's test.
    expect_test: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RubyParser {
    /// The reporter's records are the source of results; the text is then ignored (it would repeat them).
    records: bool,
    /// The tests whose own location was reported.
    seen: HashSet<String>,
    /// A test whose result line has not been printed yet: its own output came first.
    pending: Option<Vec<String>>,
    section: Option<Section>,
    /// The time of the `Finished in …` line.
    finished: Option<f64>,
}

impl RubyParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// The test's own location, once.
    fn locate(&mut self, path: &[String]) -> Option<TestEvent> {
        self.seen.insert(node_id(path)).then(|| located(path))
    }
}

impl TestOutputParser for RubyParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        if let Some(mark) = line.find(RECORD_MARK) {
            self.records = true;
            return from_record(&line[mark + RECORD_MARK.len()..]);
        }
        if self.records {
            return Vec::new();
        }
        let mut events = Vec::new();

        if let Some(found) = SUMMARY.captures(line) {
            self.section = None;
            let (runs, failures) = (count(&found[1]), count(&found[2]));
            let (errors, skips) = (count(&found[3]), count(&found[4]));
            events.push(TestEvent::Summary(TestSummary {
                passed: Some(runs.saturating_sub(failures + errors + skips)),
                failed: Some(failures + errors),
                skipped: Some(skips),
                duration_ms: self.finished,
            }));
            return events;
        }
        if let Some(found) = FINISHED.captures(line) {
            self.finished = parse_duration(&found[1]);
            self.pending = None;
            return events;
        }
        if SECTION.is_match(line) {
            self.section = Some(Section { path: None, located: false, expect_test: true });
            return events;
        }
        if let Some(section) = &mut self.section {
            if section.expect_test {
                section.expect_test = false;
                if let Some(test) = SECTION_TEST.captures(line) {
                    let path = vec![test[1].to_string(), test[2].to_string()];
                    events.push(TestEvent::output(&path, line));
                    if let Some(file) = test.get(3) {
                        section.located = true;
                        let location = file_location(file.as_str(), test[4].parse().ok(), None, Resolve::Path);
                        events.push(TestEvent::location(&path, location));
                    }
                    section.path = Some(path);
                    return events;
                }
            }
            if let Some(path) = &section.path {
                events.push(TestEvent::output(path, line));
                // An error has no `[file:line]` in its header: its first frame outside the libraries
                // is the place.
                if !section.located
                    && let Some(frame) = FRAME.captures(line).filter(|frame| !LIBRARY.is_match(&frame[1]))
                {
                    section.located = true;
                    let location = file_location(&frame[1], frame[2].parse().ok(), None, Resolve::Path);
                    events.push(TestEvent::location(path, location));
                }
            }
            return events;
        }

        if let Some(found) = RESULT.captures(line) {
            let path = vec![found[1].to_string(), found[2].to_string()];
            events.push(TestEvent::result(&path, status(&found[4]), parse_duration(&found[3])));
            events.extend(self.locate(&path));
            self.pending = None;
            return events;
        }
        if let Some(pending) = &self.pending
            && let Some(found) = LATE_RESULT.captures(line)
        {
            events.push(TestEvent::result(pending, status(&found[2]), parse_duration(&found[1])));
            self.pending = None;
            return events;
        }
        if let Some(found) = STARTED.captures(line).filter(|found| TEST_NAME.is_match(&found[2])) {
            let path = vec![found[1].to_string(), found[2].to_string()];
            events.push(TestEvent::start(&path));
            events.extend(self.locate(&path));
            if !found[3].is_empty() {
                events.push(TestEvent::output(&path, &found[3]));
            }
            self.pending = Some(path);
            return events;
        }
        if let Some(pending) = &self.pending {
            events.push(TestEvent::output(pending, line));
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run_testing::parsers::{Framework, framework_for, parse_all, with_test_reporting};
    use crate::run_testing::results::{TestNode, TestResults};
    use crate::run_testing::types::{TestStatus, test_path};

    // Captured from `ruby -Ilib test/mixed_test.rb -v --seed 1` (Minitest 6.0.6).
    const MINITEST: &str = r#"Run options: -v --seed 1

# Running:

MixedTest#test_pass = 0.00 s = .
MixedTest#test_fail = 0.00 s = F
MixedTest#test_skip = 0.00 s = S
MixedTest#test_error = 0.00 s = E
MixedTest#test_prints = hello from test
0.00 s = .
Spec style#test_0001_works = 0.00 s = .
Outer::InnerTest#test_nested = 0.00 s = .

Finished in 0.000893s, 7838.7459 runs/s, 5599.1042 assertions/s.

  1) Failure:
MixedTest#test_fail [test/mixed_test.rb:5]:
Expected: 4
  Actual: 3

  2) Skipped:
MixedTest#test_skip [test/mixed_test.rb:7]:
later

  3) Error:
MixedTest#test_error:
RuntimeError: boom
    test/mixed_test.rb:6:in 'MixedTest#test_error'

7 runs, 5 assertions, 1 failures, 1 errors, 1 skips"#;

    fn minitest() -> (Vec<TestEvent>, TestResults) {
        let events = parse_all(&mut RubyParser::new(), MINITEST);
        let mut results = TestResults::new();
        results.apply(&events);
        (events, results)
    }

    fn status(results: &TestResults, class: &str, name: &str) -> Option<TestStatus> {
        results.status(results.node_at(&[class, name])?)
    }

    fn test_named(event: &TestEvent, name: &str) -> bool {
        event.path().and_then(|path| path.get(1)).is_some_and(|test| test == name)
    }

    #[test]
    fn reports_every_test_under_its_class_with_its_status() {
        let (_, results) = minitest();
        let mut roots: Vec<&str> = results.roots().into_iter().map(|root| root.name.as_str()).collect();
        roots.sort_unstable();
        assert_eq!(roots, ["MixedTest", "Outer::InnerTest", "Spec style"]);
        assert_eq!(status(&results, "MixedTest", "test_pass"), Some(TestStatus::Passed));
        assert_eq!(status(&results, "MixedTest", "test_fail"), Some(TestStatus::Failed));
        assert_eq!(status(&results, "MixedTest", "test_error"), Some(TestStatus::Failed));
        assert_eq!(status(&results, "MixedTest", "test_skip"), Some(TestStatus::Skipped));
        assert_eq!(status(&results, "MixedTest", "test_prints"), Some(TestStatus::Passed));
        assert_eq!(status(&results, "Spec style", "test_0001_works"), Some(TestStatus::Passed));
        assert_eq!(status(&results, "Outer::InnerTest", "test_nested"), Some(TestStatus::Passed));
    }

    #[test]
    fn keeps_a_tests_own_output_with_it_and_the_result_printed_after_it() {
        let (events, _) = minitest();
        assert!(events.contains(&TestEvent::output(&test_path(&["MixedTest", "test_prints"]), "hello from test")));
        let result = |event: &&TestEvent| matches!(event, TestEvent::Result { .. }) && test_named(event, "test_prints");
        assert_eq!(events.iter().filter(result).count(), 1);
    }

    #[test]
    fn attaches_the_failure_text_and_locates_it() {
        let (events, _) = minitest();
        let output: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                TestEvent::Output { line, .. } if test_named(event, "test_fail") => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert!(output.contains(&"Expected: 4") && output.contains(&"  Actual: 3"));
        let at = |line: u32| file_location("test/mixed_test.rb", Some(line), None, Resolve::Path);
        assert!(events.contains(&TestEvent::location(&test_path(&["MixedTest", "test_fail"]), at(5))));
        // An error has no location in its header: the first frame gives it.
        assert!(events.contains(&TestEvent::location(&test_path(&["MixedTest", "test_error"]), at(6))));
    }

    #[test]
    fn locates_every_test_by_class_and_name_once_for_jumping_to_its_source() {
        let (events, results) = minitest();
        let searched = |event: &&TestEvent| match event {
            TestEvent::Location { location, .. } => location.resolve == Resolve::RubyTest,
            _ => false,
        };
        let located: Vec<&TestEvent> = events.iter().filter(searched).collect();
        assert_eq!(located.len(), 7);
        let path = test_path(&["MixedTest", "test_pass"]);
        let location = FileTarget {
            path: "test_pass".to_string(),
            line: None,
            col: None,
            resolve: Resolve::RubyTest,
            class_name: Some("MixedTest".to_string()),
        };
        assert!(located.contains(&&TestEvent::location(&path, location)));
        // The first location wins in the tree: passing tests have no other.
        let node = results.node_at(&path).expect("the test is reported");
        assert_eq!(node.location.as_ref().map(|location| location.resolve), Some(Resolve::RubyTest));
    }

    #[test]
    fn summarises_the_run() {
        let (events, _) = minitest();
        let summary = TestSummary { passed: Some(4), failed: Some(2), skipped: Some(1), duration_ms: Some(0.893) };
        assert_eq!(events.last(), Some(&TestEvent::Summary(summary)));
    }

    #[test]
    fn asks_minitest_for_one_line_per_test() {
        let reported = |kind: &str, command: &str, args: &[&str]| {
            let mut config = blank_config(format!("{kind}:test"), format!("{kind} test"), kind);
            config.command = command.to_string();
            config.args = test_path(args);
            with_test_reporting(&config).args
        };
        assert_eq!(reported("ruby", "ruby", &["-Ilib", "test/a_test.rb"]), ["-Ilib", "test/a_test.rb", "-v"]);
        assert_eq!(reported("rake", "bundle", &["exec", "rake", "test"]), ["exec", "rake", "test", "TESTOPTS=-v"]);
        assert_eq!(reported("ruby", "ruby", &["a_test.rb", "-v"]), ["a_test.rb", "-v"]);
        assert_eq!(reported("rake", "rake", &["test", "TESTOPTS=--seed 1"]), ["test", "TESTOPTS=--seed 1 -v"]);
        assert_eq!(
            reported("rake", "rake", &["test", "TESTOPTS=--name=Foo#test_a"]),
            ["test", "TESTOPTS=--name=Foo#test_a -v"]
        );
        // RSpec has no parser yet: untouched.
        assert_eq!(reported("rspec", "rspec", &["spec"]), ["spec"]);
    }

    #[test]
    fn is_the_framework_of_ruby_configurations() {
        let mut ruby = blank_config("ruby:test", "ruby test", "ruby");
        ruby.command = "ruby".to_string();
        assert_eq!(framework_for(&ruby), Framework::Ruby);
        let mut rake = blank_config("rake:test", "rake test", "rake");
        rake.command = "bundle".to_string();
        rake.args = test_path(&["exec", "rake", "test"]);
        assert_eq!(framework_for(&rake), Framework::Ruby);
    }

    // Captured from a run with the IDE's reporter (Minitest 6.0.6): records end with their own newline,
    // so the progress dots stay on one line once they are taken out.
    const REPORTER_STREAM: &str = r#"# Running:

@@OPENIDEA@@{"t":"start","c":"MixedTest","n":"test_pass","f":"/p/test/mixed_test.rb","l":4}
.@@OPENIDEA@@{"t":"result","c":"MixedTest","n":"test_pass","s":"passed","d":0.000063,"f":"/p/test/mixed_test.rb","l":4,"m":""}
@@OPENIDEA@@{"t":"start","c":"MixedTest","n":"test_fail","f":"/p/test/mixed_test.rb","l":5}
F@@OPENIDEA@@{"t":"result","c":"MixedTest","n":"test_fail","s":"failed","d":0.000054,"f":"/p/test/mixed_test.rb","l":5,"m":"Expected: 4\n  Actual: 3"}
@@OPENIDEA@@{"t":"start","c":"MixedTest","n":"test_prints","f":"/p/test/mixed_test.rb","l":8}
hello from test
.@@OPENIDEA@@{"t":"result","c":"MixedTest","n":"test_prints","s":"passed","d":0.000007,"f":"/p/test/mixed_test.rb","l":8,"m":""}
@@OPENIDEA@@{"t":"start","c":"MixedTest","n":"test_skip","f":"/p/test/mixed_test.rb","l":7}
S@@OPENIDEA@@{"t":"result","c":"MixedTest","n":"test_skip","s":"skipped","d":0.000006,"f":"/p/test/mixed_test.rb","l":7,"m":"later"}

@@OPENIDEA@@{"t":"summary","runs":4,"failures":1,"errors":0,"skips":1,"d":0.0011}
Finished in 0.001124s
"#;
    const REPORTED_FILE: &str = "/p/test/mixed_test.rb";

    /// What the console shows of a stream, and the lines the test parser gets.
    #[derive(Debug, PartialEq)]
    struct Collected {
        text: String,
        records: Vec<String>,
        lines: Vec<String>,
    }

    fn collect(chunks: &[&str]) -> Collected {
        let mut output = ruby_test_output();
        let (mut text, mut lines) = (String::new(), Vec::new());
        let mut fed: Vec<_> = chunks.iter().map(|chunk| output.feed(chunk)).collect();
        fed.push(output.end());
        for part in fed {
            text.push_str(&part.text);
            lines.extend(part.lines);
        }
        let records = lines.iter().filter(|line| line.contains(RECORD_MARK)).cloned().collect();
        Collected { text, records, lines }
    }

    #[test]
    fn takes_the_records_out_of_the_console_exactly_as_minitest_prints_them_without_it() {
        let collected = collect(&[REPORTER_STREAM]);
        assert_eq!(collected.text, "# Running:\n\n.Fhello from test\n.S\nFinished in 0.001124s\n");
        assert_eq!(collected.records.len(), 9);
        assert!(collected.records.iter().all(|record| record.contains(RECORD_MARK)));
    }

    #[test]
    fn is_the_same_however_the_output_is_cut_into_chunks() {
        let whole = collect(&[REPORTER_STREAM]);
        let chars: Vec<String> = REPORTER_STREAM.chars().map(String::from).collect();
        let chars: Vec<&str> = chars.iter().map(String::as_str).collect();
        // The stream is ASCII: seven bytes are seven characters.
        let sevens = REPORTER_STREAM.as_bytes().chunks(7);
        let odd: Vec<&str> = sevens.filter_map(|chunk| std::str::from_utf8(chunk).ok()).collect();
        assert_eq!(odd.concat(), REPORTER_STREAM);
        assert_eq!(collect(&chars), whole);
        assert_eq!(collect(&odd), whole);
    }

    #[test]
    fn drops_a_record_cut_off_by_the_end_of_the_process_not_the_text_before_it() {
        let cut = format!(r#"abc{RECORD_MARK}{{"t":"sta"#);
        assert_eq!(collect(&[cut.as_str()]).text, "abc");
        assert_eq!(collect(&["no marker @@OPEN"]).text, "no marker @@OPEN\n");
    }

    /// The tree of the reporter's stream, followed by the console's own text of the same results.
    fn reporter_results() -> TestResults {
        let mut parser = RubyParser::new();
        let mut results = TestResults::new();
        for line in collect(&[REPORTER_STREAM]).lines {
            results.apply(&parser.feed(&line));
        }
        for line in ["MixedTest#test_pass = 0.00 s = .", "1 runs, 1 assertions, 0 failures, 0 errors, 0 skips"] {
            results.apply(&parser.feed(line));
        }
        results
    }

    fn reported<'a>(results: &'a TestResults, name: &str) -> &'a TestNode {
        results.node_at(&["MixedTest", name]).expect("the test is reported")
    }

    #[test]
    fn builds_the_tree_with_statuses_times_and_failure_text() {
        let results = reporter_results();
        assert_eq!(status(&results, "MixedTest", "test_pass"), Some(TestStatus::Passed));
        assert_eq!(status(&results, "MixedTest", "test_fail"), Some(TestStatus::Failed));
        assert_eq!(status(&results, "MixedTest", "test_skip"), Some(TestStatus::Skipped));
        assert_eq!(reported(&results, "test_fail").output, ["Expected: 4", "  Actual: 3"]);
        let duration = reported(&results, "test_pass").duration_ms.expect("the reporter times the test");
        assert!((duration - 0.063).abs() < 0.0005);
    }

    #[test]
    fn gives_every_test_its_real_file_and_line_passed_or_not() {
        let results = reporter_results();
        for (name, line) in [("test_pass", 4), ("test_fail", 5), ("test_prints", 8), ("test_skip", 7)] {
            let expected = file_location(REPORTED_FILE, Some(line), None, Resolve::Path);
            assert_eq!(reported(&results, name).location, Some(expected));
        }
    }

    #[test]
    fn summarises_from_the_reporter_and_ignores_the_repeated_text() {
        let results = reporter_results();
        let summary = results.summary;
        assert_eq!((summary.passed, summary.failed, summary.skipped), (Some(2), Some(1), Some(1)));
        let class = results.node_at(&["MixedTest"]).expect("the class is reported");
        assert_eq!(results.children(class).len(), 4);
    }

    #[test]
    fn a_real_location_replaces_the_place_to_search_for() {
        let mut results = TestResults::new();
        results.apply(&parse_all(&mut RubyParser::new(), "MixedTest#test_pass = 0.00 s = ."));
        let path = test_path(&["MixedTest", "test_pass"]);
        let searched = reported(&results, "test_pass").location.as_ref().map(|location| location.resolve);
        assert_eq!(searched, Some(Resolve::RubyTest));
        let real = file_location(REPORTED_FILE, Some(4), None, Resolve::Path);
        results.apply(&[TestEvent::location(&path, real.clone())]);
        assert_eq!(reported(&results, "test_pass").location, Some(real));
    }
}
