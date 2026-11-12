//! unittest's text runner, as Django's `manage.py test` prints it: verbose result lines (`-v 2`),
//! failure sections and the summary. Tests are reported as `[module, Class, test]`; class and module
//! fixtures (`setUpClass`, a test module that fails to import) as their class or module.

use crate::run::links::{FileTarget, Resolve};
use crate::run_testing::types::{
    TestCaseStatus, TestEvent, TestOutputParser, TestSummary, count, file_location, parse_duration,
};

/// A Python identifier (Unicode letters, not starting with a digit).
const ID: &str = r"[\p{L}_][\p{L}\p{N}_]*";
// `test_x (polls.tests.QuestionTests.test_x)` (Python ≥ 3.11 adds the test name),
// `test_x (polls.tests.QuestionTests)`, then a subtest's `[msg]` / `(i=1)`. The header ends where the
// last group starts: before ` ...`, or at the end of the line.
pattern!(HEADER, &format!(r"^\s*({ID}(?:\.{ID})*) \(({ID}(?:\.{ID})+)\)(?: \[.*?\])?(?: \(.*?\))?( \.\.\.|$)"));
pattern!(OUTCOME, r"^(ok|FAIL|ERROR|skipped(?: .*)?|expected failure|unexpected success)$");
// A docstring's first line follows the name when the test has one: `Checks x. ... ok`.
pattern!(TRAILING_OUTCOME, r" \.\.\. (ok|FAIL|ERROR|skipped(?: .*)?|expected failure|unexpected success)$");
pattern!(SECTION, r"^(FAIL|ERROR|UNEXPECTED SUCCESS): (.+)$");
pattern!(FRAME, r#"^\s*File "([^"]+)", line ([0-9]+), in (\S+)"#);
// Installed packages and the standard library with its unittest (`lib/python3.12/…`, Windows `Lib\…`);
// a project's own `unittest/` folder is not a library.
pattern!(
    LIBRARY,
    concat!(
        r"[/\\](?:site-packages|dist-packages)[/\\]",
        r"|/lib/python[0-9.]*/(?:unittest/|[^/]+\.py$)",
        r"|\\Lib\\(?:unittest\\|[^\\]+\.py$)",
    )
);
pattern!(RAN, r"^Ran ([0-9]+) tests? in ([0-9.]+s)$");
// Python ≥ 3.12 prints `NO TESTS RAN` when nothing matched the labels.
pattern!(RESULT, r"^(OK|FAILED|NO TESTS RAN)(?: \((.*)\))?$");
pattern!(COUNT, r"([a-z ]+)=([0-9]+)");

/// A module that failed to import is reported as a test of this loader class.
const FAILED_LOADER: &str = "unittest.loader._Failed";

/// unittest's `separator1` / `separator2`: 70 times `=` or `-`.
fn is_rule(line: &str, mark: u8) -> bool {
    line.len() == 70 && line.bytes().all(|b| b == mark)
}

fn status(outcome: &str) -> TestCaseStatus {
    if outcome == "ok" || outcome == "expected failure" {
        TestCaseStatus::Passed
    } else if outcome.starts_with("skipped") {
        TestCaseStatus::Skipped
    } else {
        TestCaseStatus::Failed
    }
}

/// Tree path of a reported test: its module, class and name.
fn test_path(name: &str, dotted: &str) -> Vec<String> {
    // A test module that failed to import: `polls.tests (unittest.loader._FailedTest.polls.tests)`.
    if dotted.starts_with(FAILED_LOADER) {
        return vec![name.to_string()];
    }
    if name == "setUpModule" || name == "tearDownModule" {
        return vec![dotted.to_string()];
    }
    let id = dotted.strip_suffix(name).and_then(|id| id.strip_suffix('.')).unwrap_or(dotted);
    let mut parts: Vec<String> = match id.rsplit_once('.') {
        Some((module, class)) => vec![module.to_string(), class.to_string()],
        None => vec![id.to_string()],
    };
    if name != "setUpClass" && name != "tearDownClass" {
        parts.push(name.to_string());
    }
    parts
}

/// A failure section being printed.
#[derive(Clone, Debug)]
struct Section {
    path: Vec<String>,
    /// The function whose frame locates the failure (the test, `setUpClass`, a module's `<module>`).
    name: String,
    ruled: bool,
    location: Option<FileTarget>,
    /// The first project frame, for a failure raised outside the named function.
    fallback: Option<FileTarget>,
}

#[derive(Clone, Debug, Default)]
pub struct UnittestParser {
    /// A test whose outcome hasn't been printed yet (output, a docstring or subtests came first).
    pending: Option<Vec<String>>,
    section: Option<Section>,
    /// The `Ran N tests in …` line: the count and the time.
    ran: Option<(u32, Option<f64>)>,
}

impl UnittestParser {
    pub fn new() -> Self {
        Self::default()
    }

    fn close_section(&mut self) -> Vec<TestEvent> {
        let Some(section) = self.section.take() else { return Vec::new() };
        let location = section.location.or(section.fallback);
        location.into_iter().map(|location| TestEvent::location(&section.path, location)).collect()
    }

    /// Result lines; a line may hold several (Python < 3.11 prints nothing after a test with failed
    /// subtests). Tells whether the line was one.
    fn results(&mut self, line: &str, events: &mut Vec<TestEvent>) -> bool {
        let mut rest = line;
        let mut matched = false;
        while let Some(found) = HEADER.captures(rest) {
            let end = found.get(3).map_or(rest.len(), |tail| tail.start());
            let after = &rest[end..];
            let after = after.strip_prefix(" ...").map_or(after, |after| after.strip_prefix(' ').unwrap_or(after));
            let outcome = OUTCOME.captures(after);
            // A result that comes later (after a docstring or the test's output) only for test methods
            // (unittest's `test` prefix); fixtures and modules print theirs at once. Lines of other tools
            // such as `Finished (0.5s)` or `compile (gcc.exe)` are no tests.
            if outcome.is_none() && !found[1].starts_with("test") {
                break;
            }
            matched = true;
            let path = test_path(&found[1], &found[2]);
            rest = after;
            if let Some(outcome) = outcome {
                events.push(TestEvent::result(&path, status(&outcome[1]), None));
                self.pending = None;
                return true;
            }
            events.push(TestEvent::start(&path));
            self.pending = Some(path);
        }
        matched
    }
}

impl TestOutputParser for UnittestParser {
    fn feed(&mut self, line: &str) -> Vec<TestEvent> {
        let mut events = Vec::new();
        if is_rule(line, b'=') {
            events.extend(self.close_section());
            self.pending = None;
            return events;
        }
        // An unexpected success's section has no traceback: the summary follows its rule.
        if self.section.is_some() && RAN.is_match(line) {
            events.extend(self.close_section());
        }
        if let Some(section) = &mut self.section {
            if is_rule(line, b'-') {
                if section.ruled {
                    events.extend(self.close_section());
                } else {
                    section.ruled = true;
                }
                return events;
            }
            events.push(TestEvent::output(&section.path, line));
            // Library frames never locate a failure, even when named like the test or fixture
            // (Django's own `setUpClass` runs the test's `setUpTestData`).
            if let Some(frame) = FRAME.captures(line).filter(|frame| !LIBRARY.is_match(&frame[1])) {
                let location = file_location(&frame[1], frame[2].parse().ok(), None, Resolve::Path);
                let known = if frame[3] == section.name { &mut section.location } else { &mut section.fallback };
                if known.is_none() {
                    *known = Some(location);
                }
            }
            return events;
        }
        if let Some(found) = SECTION.captures(line)
            && let Some(header) = HEADER.captures(&found[2])
        {
            let path = test_path(&header[1], &header[2]);
            let name = if header[2].starts_with(FAILED_LOADER) { "<module>" } else { &header[1] };
            events.push(TestEvent::result(&path, TestCaseStatus::Failed, None));
            events.push(TestEvent::output(&path, line));
            let name = name.to_string();
            self.section = Some(Section { path, name, ruled: false, location: None, fallback: None });
            return events;
        }
        if let Some(found) = RAN.captures(line) {
            self.ran = Some((count(&found[1]), parse_duration(&found[2])));
            return events;
        }
        if let Some((ran, duration_ms)) = self.ran
            && let Some(found) = RESULT.captures(line)
        {
            let counts = found.get(2).map_or("", |counts| counts.as_str());
            let (mut failed, mut skipped) = (0, 0);
            for counted in COUNT.captures_iter(counts) {
                match counted[1].trim() {
                    "failures" | "errors" | "unexpected successes" => failed += count(&counted[2]),
                    "skipped" => skipped = count(&counted[2]),
                    _ => {}
                }
            }
            events.push(TestEvent::Summary(TestSummary {
                passed: Some(ran.saturating_sub(failed + skipped)),
                failed: Some(failed),
                skipped: Some(skipped),
                duration_ms,
            }));
            self.ran = None;
            return events;
        }
        if self.results(line, &mut events) {
            return events;
        }
        if let Some(pending) = &self.pending
            && let Some(outcome) = OUTCOME.captures(line).or_else(|| TRAILING_OUTCOME.captures(line))
        {
            events.push(TestEvent::result(pending, status(&outcome[1]), None));
            self.pending = None;
        }
        events
    }

    fn end(&mut self) -> Vec<TestEvent> {
        self.close_section()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run_testing::parsers::parse_all;
    use crate::run_testing::results::{TestCounts, TestNode, TestResults};
    use crate::run_testing::types::TestStatus;

    // Captured from `manage.py test` (Django 6.1 on Python 3.12) as the Run console splits it into lines:
    // stdout and stderr on two pipes, never sharing a line.
    const PY312: &str = r#"System check identified no issues (0 silenced).
test_documented (polls.tests.QuestionModelTests.test_documented)
Explains what is checked. ... ok
test_errors (polls.tests.QuestionModelTests.test_errors) ... ERROR
test_expected_failure (polls.tests.QuestionModelTests.test_expected_failure) ... expected failure
test_fails (polls.tests.QuestionModelTests.test_fails) ... FAIL
test_passes (polls.tests.QuestionModelTests.test_passes) ... ok
test_prints (polls.tests.QuestionModelTests.test_prints) ... hello from stderr
ok
hello from stdout
test_skipped (polls.tests.QuestionModelTests.test_skipped) ... skipped 'not ready'
test_subtests (polls.tests.QuestionModelTests.test_subtests) ... 
  test_subtests (polls.tests.QuestionModelTests.test_subtests) (i=1) ... FAIL
test_unexpected_success (polls.tests.QuestionModelTests.test_unexpected_success) ... unexpected success
test_ok (polls.more.test_views.ViewTests.test_ok) ... ok
setUpClass (polls.tests.BrokenSetupTests) ... ERROR

======================================================================
ERROR: test_errors (polls.tests.QuestionModelTests.test_errors)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 15, in test_errors
    raise ValueError("boom")
ValueError: boom

======================================================================
ERROR: setUpClass (polls.tests.BrokenSetupTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 50, in setUpClass
    raise RuntimeError("setup broke")
RuntimeError: setup broke

======================================================================
FAIL: test_fails (polls.tests.QuestionModelTests.test_fails)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 12, in test_fails
    self.assertEqual("left", "right")
AssertionError: 'left' != 'right'
- left
+ right


======================================================================
FAIL: test_subtests (polls.tests.QuestionModelTests.test_subtests) (i=1)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 43, in test_subtests
    self.assertNotEqual(i, 1)
AssertionError: 1 == 1

======================================================================
UNEXPECTED SUCCESS: test_unexpected_success (polls.tests.QuestionModelTests.test_unexpected_success)
----------------------------------------------------------------------
Ran 10 tests in 0.001s

FAILED (failures=2, errors=2, skipped=1, expected failures=1, unexpected successes=1)
Destroying test database for alias 'default' ('file:memorydb_default?mode=memory&cache=shared')..."#;

    // The same run on Django 4.2 and Python 3.10.
    const PY310: &str = r#"System check identified no issues (0 silenced).
test_documented (polls.tests.QuestionModelTests)
Explains what is checked. ... ok
test_errors (polls.tests.QuestionModelTests) ... ERROR
test_expected_failure (polls.tests.QuestionModelTests) ... expected failure
test_fails (polls.tests.QuestionModelTests) ... FAIL
test_passes (polls.tests.QuestionModelTests) ... ok
test_prints (polls.tests.QuestionModelTests) ... hello from stderr
ok
test_skipped (polls.tests.QuestionModelTests) ... skipped 'not ready'
test_subtests (polls.tests.QuestionModelTests) ... test_unexpected_success (polls.tests.QuestionModelTests) ... unexpected success
test_ok (polls.more.test_views.ViewTests) ... ok
ERROR

======================================================================
ERROR: test_errors (polls.tests.QuestionModelTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 15, in test_errors
    raise ValueError("boom")
ValueError: boom

======================================================================
ERROR: setUpClass (polls.tests.BrokenSetupTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 50, in setUpClass
    raise RuntimeError("setup broke")
RuntimeError: setup broke

======================================================================
FAIL: test_fails (polls.tests.QuestionModelTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 12, in test_fails
    self.assertEqual("left", "right")
AssertionError: 'left' != 'right'
- left
+ right


======================================================================
FAIL: test_subtests (polls.tests.QuestionModelTests) (i=1)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 43, in test_subtests
    self.assertNotEqual(i, 1)
AssertionError: 1 == 1

----------------------------------------------------------------------
Ran 10 tests in 0.001s

FAILED (failures=2, errors=2, skipped=1, expected failures=1, unexpected successes=1)
Destroying test database for alias 'default' ('file:memorydb_default?mode=memory&cache=shared')...
hello from stdout"#;

    // Default verbosity: progress characters only, failures in the sections.
    const PY310_DOTS: &str = r#"System check identified no issues (0 silenced).
.ExF.
hello from stdout
hello from stderr
.su.E
======================================================================
ERROR: test_errors (polls.tests.QuestionModelTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 15, in test_errors
    raise ValueError("boom")
ValueError: boom

======================================================================
ERROR: setUpClass (polls.tests.BrokenSetupTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 50, in setUpClass
    raise RuntimeError("setup broke")
RuntimeError: setup broke

======================================================================
FAIL: test_fails (polls.tests.QuestionModelTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 12, in test_fails
    self.assertEqual("left", "right")
AssertionError: 'left' != 'right'
- left
+ right


======================================================================
FAIL: test_subtests (polls.tests.QuestionModelTests) (i=1)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/tests.py", line 43, in test_subtests
    self.assertNotEqual(i, 1)
AssertionError: 1 == 1

----------------------------------------------------------------------
Ran 10 tests in 0.001s

FAILED (failures=2, errors=2, skipped=1, expected failures=1, unexpected successes=1)
Destroying test database for alias 'default'..."#;

    // `--parallel`: tracebacks rebuilt by tblib start in unittest's own frames.
    const PARALLEL: &str = r#"======================================================================
FAIL: test_fails (polls.tests.QuestionModelTests.test_fails)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 58, in testPartExecutor
    yield
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 634, in run
    self._callTestMethod(testMethod)
    ^^^^^^^^^^^^^^^^^
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 589, in _callTestMethod
    if method() is not None:
    ^^^^^^^^^^^^^^^^^
  File "/e2e/proj/polls/tests.py", line 12, in test_fails
    self.assertEqual("left", "right")
    ^^^^^^^^^^^^^^^^^
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 885, in assertEqual
    assertion_func(first, second, msg=msg)
    ^^^^^^^^^^^^^^^^^
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 1251, in assertMultiLineEqual
    self.fail(self._formatMessage(msg, standardMsg))
    ^^^^^^^^^^^^^^^^^
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/case.py", line 715, in fail
    raise self.failureException(msg)
    ^^^^^^^^^^^^^^^^^
AssertionError: 'left' != 'right'
- left
+ right"#;

    // Django 4.2 / Python 3.10: `setUpTestData` fails inside Django's own `setUpClass`; a non-ASCII test name.
    const SETUP_TEST_DATA: &str = r#"System check identified no issues (0 silenced).
ERROR
test_città (polls.test_extra.UnicodeTests) ... FAIL

======================================================================
ERROR: setUpClass (polls.test_extra.DataTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/venvs/py310/lib/python3.10/site-packages/django/test/testcases.py", line 1466, in setUpClass
    cls.setUpTestData()
  File "/e2e/proj/polls/test_extra.py", line 7, in setUpTestData
    raise ValueError("bad data")
ValueError: bad data

======================================================================
FAIL: test_città (polls.test_extra.UnicodeTests)
----------------------------------------------------------------------
Traceback (most recent call last):
  File "/e2e/proj/polls/test_extra.py", line 15, in test_città
    self.assertEqual(1, 2)
AssertionError: 1 != 2

----------------------------------------------------------------------
Ran 1 test in 0.001s

FAILED (failures=1, errors=1)
Destroying test database for alias 'default' ('file:memorydb_default?mode=memory&cache=shared')..."#;

    // Python 3.12: no test matched (`-k nomatch`).
    const NO_TESTS: &str = r#"System check identified no issues (0 silenced).

----------------------------------------------------------------------
Ran 0 tests in 0.000s

NO TESTS RAN"#;

    const IMPORT_ERROR: &str = r#"broken.tests (unittest.loader._FailedTest.broken.tests) ... ERROR
test_ok (polls.more.test_views.ViewTests.test_ok) ... ok

======================================================================
ERROR: broken.tests (unittest.loader._FailedTest.broken.tests)
----------------------------------------------------------------------
ImportError: Failed to import test module: broken.tests
Traceback (most recent call last):
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/loader.py", line 396, in _find_test_path
    module = self._get_module_from_name(name)
             ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  File "/venvs/pythons/cpython-3.12.16-linux-aarch64-gnu/lib/python3.12/unittest/loader.py", line 339, in _get_module_from_name
    __import__(name)
  File "/e2e/proj/broken/tests.py", line 1, in <module>
    import does_not_exist
ModuleNotFoundError: No module named 'does_not_exist'


----------------------------------------------------------------------
Ran 2 tests in 0.000s

FAILED (errors=1)"#;

    fn tree(text: &str) -> TestResults {
        let mut results = TestResults::new();
        results.apply(&parse_all(&mut UnittestParser::new(), text));
        results.finish(Some(1));
        results
    }

    fn node<'a>(results: &'a TestResults, path: &[&str]) -> &'a TestNode {
        results.node_at(path).expect("the test is reported")
    }

    fn status(results: &TestResults, path: &[&str]) -> Option<TestStatus> {
        results.status(results.node_at(path)?)
    }

    /// The status of a test of the fixtures' main class.
    fn question(results: &TestResults, test: &str) -> Option<TestStatus> {
        status(results, &["polls.tests", "QuestionModelTests", test])
    }

    fn root_names(results: &TestResults) -> Vec<&str> {
        results.roots().into_iter().map(|root| root.name.as_str()).collect()
    }

    const PASSED: Option<TestStatus> = Some(TestStatus::Passed);
    const FAILED: Option<TestStatus> = Some(TestStatus::Failed);

    #[test]
    fn reports_every_test_of_a_verbose_run_under_its_module_and_class() {
        for text in [PY312, PY310] {
            let results = tree(text);
            assert_eq!(root_names(&results), ["polls.tests", "polls.more.test_views"]);
            assert_eq!(question(&results, "test_documented"), PASSED);
            assert_eq!(question(&results, "test_errors"), FAILED);
            assert_eq!(question(&results, "test_expected_failure"), PASSED);
            assert_eq!(question(&results, "test_fails"), FAILED);
            assert_eq!(question(&results, "test_passes"), PASSED);
            // Its output split the result line in two.
            assert_eq!(question(&results, "test_prints"), PASSED);
            assert_eq!(question(&results, "test_skipped"), Some(TestStatus::Skipped));
            assert_eq!(question(&results, "test_subtests"), FAILED);
            assert_eq!(question(&results, "test_unexpected_success"), FAILED);
            assert_eq!(status(&results, &["polls.more.test_views", "ViewTests", "test_ok"]), PASSED);
            // `setUpClass` failed: the class itself.
            assert_eq!(status(&results, &["polls.tests", "BrokenSetupTests"]), FAILED);
            assert_eq!(results.counts(), TestCounts { passed: 5, failed: 5, skipped: 1, running: 0, total: 11 });
            let summary = results.summary;
            assert_eq!((summary.failed, summary.skipped, summary.duration_ms), (Some(5), Some(1), Some(1.0)));

            let fails = node(&results, &["polls.tests", "QuestionModelTests", "test_fails"]);
            let expected = file_location("/e2e/proj/polls/tests.py", Some(12), None, Resolve::Path);
            assert_eq!(fails.location, Some(expected));
            assert!(fails.output.iter().any(|line| line == "AssertionError: 'left' != 'right'"));
            let broken = node(&results, &["polls.tests", "BrokenSetupTests"]);
            assert_eq!(broken.location.as_ref().and_then(|location| location.line), Some(50));
            let subtests = node(&results, &["polls.tests", "QuestionModelTests", "test_subtests"]);
            assert!(subtests.output.iter().any(|line| line == "AssertionError: 1 == 1"));
        }
    }

    #[test]
    fn reports_failures_of_a_run_without_v_2_from_their_sections() {
        let results = tree(PY310_DOTS);
        assert_eq!(question(&results, "test_fails"), FAILED);
        assert_eq!(question(&results, "test_errors"), FAILED);
        assert_eq!(question(&results, "test_subtests"), FAILED);
        assert_eq!(status(&results, &["polls.tests", "BrokenSetupTests"]), FAILED);
        // The summary line counts the unexpected success, which has no section in Python 3.10.
        let counts = results.counts();
        assert_eq!((counts.passed, counts.failed, counts.skipped), (4, 5, 1));
    }

    #[test]
    fn locates_a_failure_at_the_tests_own_frame_past_unittests_frames() {
        let results = tree(PARALLEL);
        let located = node(&results, &["polls.tests", "QuestionModelTests", "test_fails"]).location.as_ref();
        let located = located.expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line), ("/e2e/proj/polls/tests.py", Some(12)));
    }

    #[test]
    fn locates_fixture_failures_in_the_project_past_djangos_frames_and_reads_non_ascii_names() {
        let results = tree(SETUP_TEST_DATA);
        assert_eq!(status(&results, &["polls.test_extra", "DataTests"]), FAILED);
        let expected = file_location("/e2e/proj/polls/test_extra.py", Some(7), None, Resolve::Path);
        assert_eq!(node(&results, &["polls.test_extra", "DataTests"]).location, Some(expected));
        let unicode = ["polls.test_extra", "UnicodeTests", "test_città"];
        assert_eq!(status(&results, &unicode), FAILED);
        assert_eq!(node(&results, &unicode).location.as_ref().and_then(|location| location.line), Some(15));
    }

    #[test]
    fn keeps_a_projects_own_unittest_folder_as_a_location() {
        let text = [
            "=".repeat(70).as_str(),
            "FAIL: test_a (tests.unittest.test_x.C.test_a)",
            "-".repeat(70).as_str(),
            "Traceback (most recent call last):",
            r#"  File "/e2e/proj/tests/unittest/test_x.py", line 9, in test_a"#,
            "    self.assertTrue(False)",
            "AssertionError: False is not true",
            "",
        ]
        .join("\n");
        let results = tree(&text);
        let located = node(&results, &["tests.unittest.test_x", "C", "test_a"]).location.as_ref();
        let located = located.expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line), ("/e2e/proj/tests/unittest/test_x.py", Some(9)));
    }

    #[test]
    fn reports_a_run_where_no_test_matched() {
        let results = tree(NO_TESTS);
        assert!(results.is_empty());
        let summary = results.summary;
        assert_eq!((summary.passed, summary.failed, summary.skipped), (Some(0), Some(0), Some(0)));
    }

    #[test]
    fn ignores_other_tools_lines_that_look_like_a_name_and_a_dotted_word() {
        let results = tree("Finished (0.5s)\ncompile (gcc.exe)\nbuilding (app.main) ... done");
        assert!(results.is_empty());
    }

    #[test]
    fn reports_a_test_module_that_fails_to_import_as_the_module() {
        let results = tree(IMPORT_ERROR);
        assert_eq!(status(&results, &["broken.tests"]), FAILED);
        let located = node(&results, &["broken.tests"]).location.as_ref().expect("the failure is located");
        assert_eq!((located.path.as_str(), located.line), ("/e2e/proj/broken/tests.py", Some(1)));
        assert_eq!(status(&results, &["polls.more.test_views", "ViewTests", "test_ok"]), PASSED);
    }
}
