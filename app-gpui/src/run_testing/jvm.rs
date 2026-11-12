//! Gradle test logging (`Class > method() PASSED`), the Gradle test init script's service messages and
//! Maven Surefire reports.

use crate::run::links::{FileTarget, Resolve, jvm_frame_target};
use crate::run_testing::results::{TestResults, node_id};
use crate::run_testing::teamcity::{ServiceMessage, service_message};
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestSummary, count, file_location, parse_duration,
};
use std::collections::HashMap;

pattern!(GRADLE, r"^(\S[^>]*?(?:\s>\s[^>]+?)+)\s+(PASSED|FAILED|SKIPPED)(?:\s+\(([0-9.]+\s*m?s)\))?$");
pattern!(GRADLE_SUMMARY, r"^([0-9]+) tests? completed(?:, ([0-9]+) failed)?(?:, ([0-9]+) skipped)?");
pattern!(GRADLE_SEPARATOR, r"\s+>\s+");
pattern!(
    FRAME,
    concat!(
        r"(?:([A-Za-z0-9_$]+(?:\.[A-Za-z0-9_$]+)*)\.[A-Za-z0-9_$<>-]+)?",
        r"\(([A-Za-z0-9_$.-]+\.(?:java|kt|kts|scala|groovy)):([0-9]+)\)",
        r"|(?-u:\b)at ([A-Za-z0-9_$.-]+\.(?:java|kt|kts|scala|groovy)):([0-9]+)",
    )
);
// [ERROR] com.acme.OrderTest.shouldX -- Time elapsed: 0.01 s <<< FAILURE!   (Surefire 3)
pattern!(
    SUREFIRE3,
    concat!(
        r"^(?:\[[A-Za-z0-9_]+\]\s+)?([A-Za-z0-9_$.]+)\.([A-Za-z0-9_$]+)(?:\(\))?\s+--\s+",
        r"Time elapsed: ([0-9.]+ s(?:ec)?)\s+<<<\s+(FAILURE|ERROR)!",
    )
);
// shouldX(com.acme.OrderTest)  Time elapsed: 0.01 sec  <<< FAILURE!          (Surefire 2)
pattern!(
    SUREFIRE2,
    concat!(
        r"^(?:\[[A-Za-z0-9_]+\]\s+)?([A-Za-z0-9_$]+)\(([A-Za-z0-9_$.]+)\)\s+",
        r"Time elapsed: ([0-9.]+ s(?:ec)?)\s+<<<\s+(FAILURE|ERROR)!",
    )
);
pattern!(
    SUREFIRE_CLASS,
    concat!(
        r"^(?:\[[A-Za-z0-9_]+\]\s+)?Tests run: ([0-9]+), Failures: ([0-9]+), Errors: ([0-9]+), Skipped: ([0-9]+), ",
        r"Time elapsed: ([0-9.]+ s(?:ec)?).*?(?:-- in |- in )([A-Za-z0-9_$.]+)\s*$",
    )
);
pattern!(
    SUREFIRE_TOTAL,
    r"^(?:\[[A-Za-z0-9_]+\]\s+)?Tests run: ([0-9]+), Failures: ([0-9]+), Errors: ([0-9]+), Skipped: ([0-9]+)\s*$"
);
// `at org.junit…`, also behind a class loader or module (`at app//org.junit…`, `at java.base@21/java.lang…`).
pattern!(
    LIBRARY_FRAME,
    concat!(
        r"^\s*at\s+(?:\S*/)?",
        r"(?:org\.junit|junit\.|org\.opentest4j|org\.testng|kotlin\.|jdk\.internal|java\.|javax\.|sun\.|org\.gradle)",
    )
);
pattern!(MAVEN_LOG, r"^\[(INFO|WARNING)\]");

/// The source position of a stack frame in the project's code, `None` for library frames.
fn frame_location(line: &str) -> Option<FileTarget> {
    let frame = FRAME.captures(line)?;
    if LIBRARY_FRAME.is_match(line) {
        return None;
    }
    Some(match frame.get(2) {
        Some(file) => jvm_frame_target(frame.get(1).map(|class| class.as_str()), file.as_str(), frame[3].parse().ok()),
        None => file_location(&frame[4], frame[5].parse().ok(), None, Resolve::Name),
    })
}

/// Lines of a text that may end with one line break, which is not a line of its own.
fn text_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line))
}

/// Test events of the init script's messages. Tests are `[class, method]`, named as in the XML reports
/// (`add(int, int)[1]` → `add`); `outcomes` keeps each test's failure or skip until it finishes.
fn init_script_events(message: &ServiceMessage, outcomes: &mut HashMap<String, TestCaseStatus>) -> Vec<TestEvent> {
    let (class_name, name) = (message.attr("className"), message.attr("name"));
    if class_name.is_empty() || name.is_empty() {
        return Vec::new();
    }
    let method = Some(name.split('(').next().unwrap_or(name).trim()).filter(|method| !method.is_empty());
    let path = vec![class_name.to_string(), method.unwrap_or(name).to_string()];
    let key = node_id(&path);
    // A parameterized method's invocations share its node: one failure fails the method.
    let failed = outcomes.get(&key) == Some(&TestCaseStatus::Failed);
    match message.kind.as_str() {
        "testStarted" => {
            if !failed {
                outcomes.remove(&key);
            }
            vec![TestEvent::start(&path)]
        }
        "testStdOut" | "testStdErr" => {
            let out = message.attr("out");
            let text = out.strip_suffix('\n').map_or(out, |text| text.strip_suffix('\r').unwrap_or(text));
            if text.is_empty() {
                return Vec::new();
            }
            text_lines(text).map(|line| TestEvent::output(&path, line)).collect()
        }
        "testFailed" => {
            outcomes.insert(key, TestCaseStatus::Failed);
            let details = Some(message.attr("details")).filter(|details| !details.is_empty());
            let lines: Vec<&str> = details.unwrap_or(message.attr("message")).split('\n').collect();
            let mut events: Vec<TestEvent> = lines.iter().map(|line| TestEvent::output(&path, line)).collect();
            if let Some(location) = lines.iter().find_map(|line| frame_location(line)) {
                events.push(TestEvent::location(&path, location));
            }
            events
        }
        "testIgnored" => {
            if !failed {
                outcomes.insert(key, TestCaseStatus::Skipped);
            }
            Vec::new()
        }
        "testFinished" => {
            let status = outcomes.get(&key).copied().unwrap_or(TestCaseStatus::Passed);
            vec![TestEvent::result(&path, status, message.attr("duration").parse().ok())]
        }
        _ => Vec::new(),
    }
}

/// Reports tests as `[class, method]` (Gradle's console names nested classes on the way).
#[derive(Clone, Debug, Default)]
pub struct JvmParser {
    failure: Option<Vec<String>>,
    located: bool,
    /// The init script reports every test: Gradle's own test logging would repeat them by simple class name.
    reported: bool,
    outcomes: HashMap<String, TestCaseStatus>,
}

impl JvmParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn start_failure(&mut self, path: Vec<String>) {
        self.failure = Some(path);
        self.located = false;
    }
}

impl TestOutputParser for JvmParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        if let Some(message) = service_message(line) {
            self.reported = true;
            self.failure = None;
            return init_script_events(&message, &mut self.outcomes);
        }
        if let Some(found) = GRADLE.captures(line.trim()).filter(|_| !line.starts_with(' ')) {
            if self.reported {
                self.failure = None;
                return events;
            }
            // `method()` is a JUnit 5 display name; the reports name the method.
            let mut path: Vec<String> = GRADLE_SEPARATOR.split(&found[1]).map(str::to_string).collect();
            if let Some(method) = path.last_mut()
                && let Some(name) = method.strip_suffix("()")
            {
                *method = name.to_string();
            }
            let status = match &found[2] {
                "PASSED" => TestCaseStatus::Passed,
                "FAILED" => TestCaseStatus::Failed,
                _ => TestCaseStatus::Skipped,
            };
            let duration = found.get(3).and_then(|duration| parse_duration(duration.as_str()));
            events.push(TestEvent::result(&path, status, duration));
            if status == TestCaseStatus::Failed {
                self.start_failure(path);
            } else {
                self.failure = None;
            }
            return events;
        }
        if let Some(found) = GRADLE_SUMMARY.captures(line) {
            let optional = |group: usize| found.get(group).map_or(0, |digits| count(digits.as_str()));
            let (failed, skipped) = (optional(2), optional(3));
            events.push(summary(count(&found[1]), failed, skipped));
            self.failure = None;
            return events;
        }
        let surefire3 = SUREFIRE3.captures(line);
        let is_surefire3 = surefire3.is_some();
        if let Some(found) = surefire3.or_else(|| SUREFIRE2.captures(line)) {
            let (class, method) = if is_surefire3 { (&found[1], &found[2]) } else { (&found[2], &found[1]) };
            let path = vec![class.to_string(), method.to_string()];
            events.push(TestEvent::result(&path, TestCaseStatus::Failed, parse_duration(&found[3])));
            self.start_failure(path);
            return events;
        }
        if let Some(found) = SUREFIRE_CLASS.captures(line) {
            let failed = count(&found[2]) + count(&found[3]);
            let status = if failed > 0 {
                TestCaseStatus::Failed
            } else if count(&found[4]) == count(&found[1]) {
                TestCaseStatus::Skipped
            } else {
                TestCaseStatus::Passed
            };
            events.push(TestEvent::result(&[found[6].to_string()], status, parse_duration(&found[5])));
            self.failure = None;
            return events;
        }
        if let Some(found) = SUREFIRE_TOTAL.captures(line) {
            let failed = count(&found[2]) + count(&found[3]);
            events.push(summary(count(&found[1]), failed, count(&found[4])));
            self.failure = None;
            return events;
        }
        if let Some(failure) = &self.failure {
            if line.trim().is_empty() {
                return events;
            }
            if MAVEN_LOG.is_match(line) {
                self.failure = None;
                return events;
            }
            events.push(TestEvent::output(failure, line));
            if !self.located
                && let Some(location) = frame_location(line)
            {
                self.located = true;
                events.push(TestEvent::location(failure, location));
            }
        }
        events
    }
}

/// The summary of `run` tests of which `failed` failed and `skipped` were skipped.
fn summary(run: u32, failed: u32, skipped: u32) -> TestEvent {
    TestEvent::Summary(TestSummary {
        passed: Some(run.saturating_sub(failed + skipped)),
        failed: Some(failed),
        skipped: Some(skipped),
        duration_ms: None,
    })
}

/// Gradle's console names a test class by its simple name and its XML reports by the qualified one:
/// moves the console's results to the reported class. A simple name that several reported classes
/// share stays where it is.
pub fn move_to_reported_classes(tests: &mut TestResults, class_names: &[String]) {
    let mut by_simple_name: HashMap<&str, Option<&str>> = HashMap::new();
    let mut seen: Vec<&str> = Vec::new();
    for class in class_names.iter().map(String::as_str) {
        if seen.contains(&class) {
            continue;
        }
        seen.push(class);
        if let Some((_, simple)) = class.rsplit_once('.') {
            let unique = !by_simple_name.contains_key(simple);
            by_simple_name.insert(simple, unique.then_some(class));
        }
    }
    let roots: Vec<String> = tests.roots().into_iter().map(|root| root.name.clone()).collect();
    for root in roots {
        if let Some(Some(qualified)) = by_simple_name.get(root.as_str()) {
            tests.move_node(std::slice::from_ref(&root), &[(*qualified).to_string()]);
        }
    }
}
