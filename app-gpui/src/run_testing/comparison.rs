//! Expected / actual values of an assertion failure, read from a test's captured output, for the test
//! tree's "Show Diff" (as in IDEA's comparison failure diff).

use regex::Regex;

/// Side labels when the framework doesn't say which side is expected (Rust `left` / `right`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SideLabels {
    pub expected: &'static str,
    pub actual: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comparison {
    pub expected: String,
    pub actual: String,
    pub labels: Option<SideLabels>,
}

impl Comparison {
    fn new(expected: &str, actual: &str) -> Self {
        Self { expected: expected.to_string(), actual: actual.to_string(), labels: None }
    }
}

// JUnit / opentest4j / AssertJ / Hamcrest-style: `expected: <x> but was: <y>`.
pattern!(JUNIT, r"expected:\s*(<(?s:.*?)>|\S[^\n]*?)\s*but was:\s*(<(?s:.*?)>|[^\n]*)");
// TestNG `Assert.assertEquals`: `expected [x] but found [y]`.
pattern!(TESTNG, r"expected \[((?s:.*?))\] but found \[((?s:.*?))\]");
// kotlin.test: `Expected <x>, actual <y>.`
pattern!(KOTLIN, r"Expected <((?s:.*?))>, actual <((?s:.*?))>");
// Rust `assert_eq!`: `  left: x` / ` right: y` (the macro doesn't say which one is expected).
pattern!(LEFT, r"^\s*left:\s+");
pattern!(RIGHT, r"^\s*right:\s+");
// Jest / Vitest: `Expected: x` then `Received: y`.
pattern!(JEST_EXPECTED, r"^\s*Expected:\s+");
pattern!(JEST_RECEIVED, r"^\s*Received:\s+");
// Go testify: `expected: x` then `actual  : y`.
pattern!(TESTIFY_EXPECTED, r"^\s*(?:Error:\s+Not equal:\s*)?expected:\s+");
pattern!(TESTIFY_ACTUAL, r"^\s*actual\s*:\s+");
// pytest: `assert actual == expected`.
pattern!(PYTEST, r"(?m)^(?:E\s+)?(?:AssertionError:\s*)?assert\s+(.+?)\s+==\s+(.+)$");

/// Strips the angle brackets frameworks wrap values in (`<1>`; `"a"` stays as written).
fn unwrap(value: &str) -> &str {
    let value = value.trim();
    value.strip_prefix('<').and_then(|inner| inner.strip_suffix('>')).unwrap_or(value)
}

/// The value a line gives after its label, when the line starts with the label.
fn labelled<'a>(label: &Regex, line: &'a str) -> Option<&'a str> {
    Some(&line[label.find(line)?.end()..])
}

/// The values of the first line labelled `first` and of the first later line labelled `second`.
fn labelled_pair<'a>(lines: &'a [String], first: &Regex, second: &Regex) -> Option<(&'a str, &'a str)> {
    let at = lines.iter().position(|line| first.is_match(line))?;
    let other = lines[at + 1..].iter().find_map(|line| labelled(second, line))?;
    Some((labelled(first, &lines[at])?, other))
}

/// The comparison a failure's output reports; `None` for failures that compare nothing. Output lines
/// reach the parsers with escape sequences already removed.
pub fn parse_comparison(output: &[String]) -> Option<Comparison> {
    let text = output.join("\n");
    if let Some(found) = JUNIT.captures(&text) {
        return Some(Comparison::new(unwrap(&found[1]), unwrap(&found[2])));
    }
    if let Some(found) = TESTNG.captures(&text).or_else(|| KOTLIN.captures(&text)) {
        return Some(Comparison::new(&found[1], &found[2]));
    }
    if let Some(left) = output.iter().position(|line| LEFT.is_match(line))
        && let Some(right) = output.get(left + 1).and_then(|line| labelled(&RIGHT, line))
    {
        let labels = Some(SideLabels { expected: "Left", actual: "Right" });
        return Some(Comparison { labels, ..Comparison::new(labelled(&LEFT, &output[left])?, right) });
    }
    if let Some((expected, actual)) = labelled_pair(output, &JEST_EXPECTED, &JEST_RECEIVED) {
        return Some(Comparison::new(expected, actual));
    }
    if let Some((expected, actual)) = labelled_pair(output, &TESTIFY_EXPECTED, &TESTIFY_ACTUAL) {
        return Some(Comparison::new(expected, actual));
    }
    let found = PYTEST.captures(&text)?;
    Some(Comparison::new(found[2].trim(), found[1].trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(output: &[&str]) -> Option<Comparison> {
        parse_comparison(&output.iter().map(|line| line.to_string()).collect::<Vec<_>>())
    }

    fn values(expected: &str, actual: &str) -> Option<Comparison> {
        Some(Comparison::new(expected, actual))
    }

    #[test]
    fn junit_opentest4j() {
        let output = ["org.opentest4j.AssertionFailedError: expected: <3> but was: <4>"];
        assert_eq!(compare(&output), values("3", "4"));
        assert_eq!(compare(&[r#"expected: "a b""#, r#" but was: "a c""#]), values(r#""a b""#, r#""a c""#));
    }

    #[test]
    fn testng() {
        let output = ["java.lang.AssertionError: total expected [3] but found [2]"];
        assert_eq!(compare(&output), values("3", "2"));
    }

    #[test]
    fn kotlin_test() {
        assert_eq!(compare(&["java.lang.AssertionError: Expected <1>, actual <2>."]), values("1", "2"));
    }

    #[test]
    fn jest_vitest() {
        let output =
            ["    expect(received).toBe(expected) // Object.is equality", "", "    Expected: 3", "    Received: 4"];
        assert_eq!(compare(&output), values("3", "4"));
    }

    #[test]
    fn rust_assert_eq_keeps_left_right() {
        let expected = Comparison {
            expected: "3".to_string(),
            actual: "4".to_string(),
            labels: Some(SideLabels { expected: "Left", actual: "Right" }),
        };
        assert_eq!(compare(&["assertion `left == right` failed", "  left: 3", " right: 4"]), Some(expected));
    }

    #[test]
    fn go_testify_and_pytest() {
        let testify = ["\tError:      \tNot equal: ", "\t            \texpected: 3", "\t            \tactual  : 4"];
        assert_eq!(compare(&testify), values("3", "4"));
        let pytest = [">       assert add(1, 2) == 4", "E       assert 3 == 4"];
        assert_eq!(compare(&pytest), values("4", "3"));
    }

    #[test]
    fn no_comparison_in_other_failures() {
        assert_eq!(compare(&["java.lang.IllegalStateException: boom", "\tat com.acme.A.b(A.java:3)"]), None);
    }
}
