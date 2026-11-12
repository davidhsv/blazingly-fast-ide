//! `cargo test` (libtest) and `cargo nextest` output.

use crate::run::links::Resolve;
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestSummary, count, file_location, parse_duration,
};

pattern!(RUNNING, r"^\s+Running (?:unittests )?(\S+) \(.+\)$");
pattern!(DOCTESTS, r"^\s*Doc-tests (\S+)");
pattern!(TEST, r"^test (.+?) \.\.\. (ok|FAILED|ignored|bench)(?:,.*)?(?:\s+<([0-9.]+s)>)?$");
pattern!(FAILURE_HEADER, r"^---- (.+?) (?:stdout|stderr) ----$");
// Rust ≥ 1.73: `thread 'x' panicked at src/lib.rs:10:5:`; older: `panicked at 'msg', src/lib.rs:10:5`.
pattern!(PANIC, r#"panicked at (?:'.*', |".*", )?([^\s:'"][^:'"]*):([0-9]+):([0-9]+)"#);
pattern!(
    SUMMARY,
    r"^test result: (?:ok|FAILED)\. ([0-9]+) passed; ([0-9]+) failed; ([0-9]+) ignored;.*?(?:finished in ([0-9.]+s))?$"
);
pattern!(NEXTEST, r"^\s*(PASS|FAIL|SKIP|SIGSEGV|SIGABRT|TIMEOUT|LEAK)\s+\[\s*([0-9.]+s)\]\s+(\S+)\s+(.+)$");
pattern!(
    NEXTEST_SUMMARY,
    concat!(
        r"^\s*Summary \[\s*([0-9.]+s)\]\s+([0-9]+) tests? run: ([0-9]+) passed",
        r"(?:.*?([0-9]+) failed)?(?:.*?([0-9]+) skipped)?",
    )
);

/// Reports tests as `[binary, ...module path, name]`; the totals add up over the test binaries.
#[derive(Clone, Debug, Default)]
pub struct CargoParser {
    /// The test binary whose tests are being printed (`src/lib.rs`, `tests/api.rs`, `Doc-tests app`).
    suite: Option<String>,
    /// The test whose captured output is being printed.
    failure: Option<Vec<String>>,
    passed: u32,
    failed: u32,
    skipped: u32,
    duration_ms: f64,
}

impl CargoParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn path_of(&self, name: &str) -> Vec<String> {
        self.suite.iter().cloned().chain(name.split("::").map(str::to_string)).collect()
    }
}

impl TestOutputParser for CargoParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        if let Some(found) = RUNNING.captures(line) {
            self.suite = Some(found[1].to_string());
            self.failure = None;
        } else if let Some(found) = DOCTESTS.captures(line) {
            self.suite = Some(format!("Doc-tests {}", &found[1]));
            self.failure = None;
        } else if let Some(found) = TEST.captures(line) {
            let status = match &found[2] {
                "ok" | "bench" => TestCaseStatus::Passed,
                "FAILED" => TestCaseStatus::Failed,
                _ => TestCaseStatus::Skipped,
            };
            let duration = found.get(3).and_then(|duration| parse_duration(duration.as_str()));
            events.push(TestEvent::result(&self.path_of(&found[1]), status, duration));
        } else if let Some(found) = FAILURE_HEADER.captures(line) {
            self.failure = Some(self.path_of(&found[1]));
        } else if let Some(found) = SUMMARY.captures(line) {
            self.failure = None;
            self.passed += count(&found[1]);
            self.failed += count(&found[2]);
            self.skipped += count(&found[3]);
            self.duration_ms += found.get(4).and_then(|duration| parse_duration(duration.as_str())).unwrap_or(0.0);
            events.push(TestEvent::Summary(TestSummary {
                passed: Some(self.passed),
                failed: Some(self.failed),
                skipped: Some(self.skipped),
                duration_ms: Some(self.duration_ms),
            }));
        } else if let Some(found) = NEXTEST.captures(line) {
            let status = match &found[1] {
                "PASS" => TestCaseStatus::Passed,
                "SKIP" => TestCaseStatus::Skipped,
                _ => TestCaseStatus::Failed,
            };
            let path: Vec<String> =
                std::iter::once(&found[3]).chain(found[4].split("::")).map(str::to_string).collect();
            events.push(TestEvent::result(&path, status, parse_duration(&found[2])));
        } else if let Some(found) = NEXTEST_SUMMARY.captures(line) {
            let optional = |group: usize| found.get(group).map_or(0, |digits| count(digits.as_str()));
            events.push(TestEvent::Summary(TestSummary {
                passed: Some(count(&found[3])),
                failed: Some(optional(4)),
                skipped: Some(optional(5)),
                duration_ms: parse_duration(&found[1]),
            }));
        } else if let Some(failure) = &self.failure {
            if line.starts_with("failures:") {
                self.failure = None;
            } else {
                events.push(TestEvent::output(failure, line));
                if let Some(panic) = PANIC.captures(line) {
                    let (line, col) = (panic[2].parse().ok(), panic[3].parse().ok());
                    events.push(TestEvent::location(failure, file_location(&panic[1], line, col, Resolve::Path)));
                }
            }
        }
        events
    }
}
