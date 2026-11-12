//! Jest and Vitest reporters (default + verbose).

use crate::find::utf16;
use crate::run::links::{FileTarget, Resolve};
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestSummary, count, file_location, parse_duration,
};
use regex::Captures;

const PASS_MARKS: &str = "✓✔√";
const FAIL_MARKS: &str = "✕×✗✖";
const SKIP_MARKS: &str = "○↓⊘";
const TODO_MARKS: &str = "✎";

const TEST_FILE: &str = r"[^\s>()]+\.(?:[cm]?[jt]sx?|vue|svelte|astro)";

pattern!(JEST_FILE, &format!(r"^\s*(PASS|FAIL)\s+({TEST_FILE})(?:\s+\(.*\))?\s*$"));
pattern!(VITEST_FAIL_HEADER, &format!(r"^\s*FAIL\s+({TEST_FILE})\s+>\s+(.+?)\s*$"));
pattern!(JEST_FAIL_HEADER, r"^\s*● (.+)$");
pattern!(LOCATION, &format!(r"(?:\(|\s|^)(?:file://)?((?:/|\.{{0,2}}/?)?{TEST_FILE}):([0-9]+):([0-9]+)\)?"));
pattern!(JEST_SUMMARY, r"^Tests:\s+(.+?)\s*$");
pattern!(VITEST_SUMMARY, r"^\s*Tests\s+(.+?)\s+\(([0-9]+)\)\s*$");
pattern!(DURATION_SUMMARY, r"^\s*(?:Time:|Duration)\s+([0-9.]+\s*m?s)");
pattern!(TRAILING_DURATION, r"\s+\(?([0-9]+(?:\.[0-9]+)?\s?m?s)\)?\s*$");
pattern!(FILE_COUNT, r"\s+\(([0-9]+) tests?(?:\s*\|[^)]*)?\)");
pattern!(COUNT, r"([0-9]+) (passed|failed|skipped|todo|pending)");
// A test file at the start of a line's text.
pattern!(FILE_START, &format!(r"^({TEST_FILE})"));
// A test file that is a whole word at the start of a line's text.
pattern!(FILE_WORD, &format!(r"^({TEST_FILE})(?:\s|$)"));
pattern!(SUB_TEST, r"^\s+>\s+(.+)$");
pattern!(VITEST_SEPARATOR, r"\s+>\s+");
pattern!(NAME_SEPARATOR, r"\s+(?:>|›)\s+");
pattern!(JEST_SEPARATOR, r"\s+›\s+");
pattern!(AT, r"(?-u:\b)at(?-u:\b)");

fn mark_status(mark: char) -> Option<TestCaseStatus> {
    if PASS_MARKS.contains(mark) {
        Some(TestCaseStatus::Passed)
    } else if FAIL_MARKS.contains(mark) {
        Some(TestCaseStatus::Failed)
    } else if SKIP_MARKS.contains(mark) || TODO_MARKS.contains(mark) {
        Some(TestCaseStatus::Skipped)
    } else {
        None
    }
}

/// The counts of a `Tests` summary: todo and pending tests count as skipped.
fn counts(text: &str) -> TestSummary {
    let mut summary = TestSummary::default();
    for found in COUNT.captures_iter(text) {
        let n = count(&found[1]);
        match &found[2] {
            "passed" => summary.passed = Some(n),
            "failed" => summary.failed = Some(n),
            _ => summary.skipped = Some(summary.skipped.unwrap_or(0) + n),
        }
    }
    summary
}

fn trailing_duration(body: &str) -> Option<f64> {
    parse_duration(&TRAILING_DURATION.captures(body)?[1])
}

fn location_of(found: &Captures) -> FileTarget {
    file_location(&found[1], found[2].parse().ok(), found[3].parse().ok(), Resolve::Path)
}

fn split(separator: &regex::Regex, names: &str) -> Vec<String> {
    separator.split(names).map(str::to_string).collect()
}

/// Reports tests as `[file, ...describe blocks, name]`.
#[derive(Clone, Debug, Default)]
pub struct JsParser {
    file: Option<String>,
    /// Jest verbose describe blocks: indentation and name.
    describes: Vec<(u32, String)>,
    in_verbose_block: bool,
    failure: Option<Vec<String>>,
    location_found: bool,
}

impl JsParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn start_failure(&mut self, path: Vec<String>) {
        self.failure = Some(path);
        self.location_found = false;
    }

    /// Leaves the describe blocks that are not above a line indented by `indent`.
    fn leave_describes(&mut self, indent: u32) {
        while self.describes.last().is_some_and(|(open, _)| *open >= indent) {
            self.describes.pop();
        }
    }
}

impl TestOutputParser for JsParser {
    fn feed(&mut self, raw: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        let line = raw.trim_end();

        if let Some(found) = JEST_FILE.captures(line) {
            let file = found[2].to_string();
            self.describes.clear();
            self.in_verbose_block = true;
            self.failure = None;
            let status = if &found[1] == "PASS" { TestCaseStatus::Passed } else { TestCaseStatus::Failed };
            events.push(TestEvent::result(std::slice::from_ref(&file), status, None));
            self.file = Some(file);
            return events;
        }
        if let Some(found) = VITEST_FAIL_HEADER.captures(line) {
            let mut path = vec![found[1].to_string()];
            path.extend(split(&VITEST_SEPARATOR, &found[2]));
            events.push(TestEvent::result(&path, TestCaseStatus::Failed, None));
            self.start_failure(path);
            return events;
        }
        if let Some(found) = JEST_SUMMARY.captures(line).or_else(|| VITEST_SUMMARY.captures(line)) {
            events.push(TestEvent::Summary(counts(&found[1])));
            self.failure = None;
            return events;
        }
        if let Some(found) = DURATION_SUMMARY.captures(line) {
            let duration_ms = parse_duration(&found[1]);
            events.push(TestEvent::Summary(TestSummary { duration_ms, ..TestSummary::default() }));
            return events;
        }

        let trimmed = line.trim_start();
        let indent = utf16::len(line) - utf16::len(trimmed);
        let mut after_mark = trimmed.chars();
        let mark = after_mark.next();
        let spaced = after_mark.next() == Some(' ');
        // The text after the mark and the character that follows it.
        let marked = after_mark.as_str();
        let status = mark.filter(|_| spaced).and_then(mark_status);

        // Vitest: ` ❯ src/a.test.ts:5:13` (failure location) or ` ❯ src/a.test.ts (2 tests | 1 failed)`.
        if matches!(mark, Some('❯' | '→')) {
            if let Some(found) = LOCATION.captures(marked)
                && let Some(failure) = &self.failure
                && !self.location_found
            {
                self.location_found = true;
                events.push(TestEvent::location(failure, location_of(&found)));
                return events;
            }
            if let Some(found) = FILE_START.captures(marked)
                && FILE_COUNT.is_match(marked)
            {
                let file = found[1].to_string();
                let failed = TestCaseStatus::Failed;
                events.push(TestEvent::result(std::slice::from_ref(&file), failed, trailing_duration(marked)));
                self.file = Some(file);
                return events;
            }
            if let Some(failure) = &self.failure {
                events.push(TestEvent::output(failure, raw));
            }
            return events;
        }

        if let Some(status) = status {
            let duration_ms = trailing_duration(marked);
            let body = TRAILING_DURATION.replace(marked, "");
            if let Some(found) = FILE_WORD.captures(&body) {
                let file = found[1].to_string();
                if let Some(sub) = SUB_TEST.captures(&body[file.len()..]) {
                    // Vitest verbose (flat): `✓ src/a.test.ts > suite > test 2ms`.
                    let mut path = vec![file];
                    path.extend(split(&VITEST_SEPARATOR, &sub[1]));
                    events.push(TestEvent::result(&path, status, duration_ms));
                } else {
                    // Vitest file line: `✓ src/a.test.ts (3 tests) 5ms`.
                    self.describes.clear();
                    events.push(TestEvent::result(std::slice::from_ref(&file), status, duration_ms));
                    self.file = Some(file);
                }
                return events;
            }
            let body = FILE_COUNT.replace(&body, "");
            self.leave_describes(indent);
            let mut path: Vec<String> = self.file.iter().cloned().collect();
            path.extend(self.describes.iter().map(|(_, name)| name.clone()));
            path.extend(split(&NAME_SEPARATOR, &body));
            events.push(TestEvent::result(&path, status, duration_ms));
            return events;
        }

        if let Some(found) = JEST_FAIL_HEADER.captures(line) {
            let names = split(&JEST_SEPARATOR, &found[1]);
            if names.first().is_some_and(|name| name == "Test suite failed to run") {
                if let Some(file) = self.file.clone() {
                    self.start_failure(vec![file]);
                }
            } else {
                let mut path: Vec<String> = self.file.iter().cloned().collect();
                path.extend(names);
                events.push(TestEvent::result(&path, TestCaseStatus::Failed, None));
                self.start_failure(path);
            }
            self.in_verbose_block = false;
            return events;
        }

        let indented = !trimmed.is_empty() && indent > 0;
        if self.in_verbose_block && self.file.is_some() && indented && self.failure.is_none() {
            // Jest verbose describe header (indented, no mark).
            self.leave_describes(indent);
            self.describes.push((indent, trimmed.to_string()));
            return events;
        }
        if self.in_verbose_block && trimmed.is_empty() {
            self.in_verbose_block = false;
        }

        if let Some(failure) = &self.failure {
            events.push(TestEvent::output(failure, raw));
            if !self.location_found && !line.contains("node_modules") {
                let framed = AT.is_match(line) || trimmed.starts_with('❯');
                if let Some(found) = LOCATION.captures(line).filter(|_| framed) {
                    self.location_found = true;
                    events.push(TestEvent::location(failure, location_of(&found)));
                }
            }
        }
        events
    }
}
