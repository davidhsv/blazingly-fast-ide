//! `dotnet test`: the console tells little about single tests, so results are read from the TRX reports
//! the run is asked to write. [`DotnetReports`] adds the report logger to the launch and collects the
//! report paths the run announces; the caller reads those files and gets their events here.

use crate::run::configs::param;
use crate::run::links::Resolve;
use crate::run_testing::types::{TestCaseStatus, TestEvent, file_location};
use ide_run::RunConfig;
use std::collections::HashMap;

// An absolute report path at the end of a line, independently of VSTest's UI language.
pattern!(REPORT_PATH, r"(?i)(?:[A-Za-z]:[\\/]|/).*\.trx\s*$");
pattern!(DURATION, r"^([0-9]+):([0-9]+):([0-9]+(?:\.[0-9]+)?)$");
pattern!(SOURCE, r"(?-u:\b)in (.+\.(?:cs|fs|vb)):(?:line )?([0-9]+)");

/// Elements nested deeper than this make a report invalid.
const MAX_DEPTH: usize = 64;
/// Lines of a message, stack trace or captured stream that are kept per test.
const MAX_TEXT_LINES: usize = 200;

/// The reports of one `dotnet test` invocation, including each target framework's report.
#[derive(Clone, Debug)]
pub struct DotnetReports {
    prefix: String,
    paths: Vec<String>,
    mtp: bool,
}

impl DotnetReports {
    /// `id` is unique to the invocation: only reports named after it are read.
    pub fn new(id: &str) -> Self {
        Self { prefix: format!("openidea-{id}"), paths: Vec::new(), mtp: false }
    }

    /// The file name prefix of this invocation's VSTest reports.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// `config` with what makes the run write TRX reports: VSTest's logger ahead of the run settings
    /// (`--`), or the testing platform's `--report-trx`.
    pub fn launch(&mut self, config: &RunConfig) -> RunConfig {
        let runner = param(config, "runner");
        self.mtp = matches!(runner, Some("mtp" | "mtp-legacy"));
        let mut launched = config.clone();
        let has = |args: &[String], flag: &str| args.iter().any(|arg| arg == flag);
        if self.mtp {
            if param(config, "trx") == Some("false") {
                return launched;
            }
            if runner == Some("mtp-legacy") && !has(&launched.args, "--") {
                launched.args.push("--".to_string());
            }
            if !has(&launched.args, "--report-trx") {
                launched.args.push("--report-trx".to_string());
            }
            return launched;
        }
        let at = config.args.iter().position(|arg| arg == "--").unwrap_or(config.args.len());
        launched.args.truncate(at);
        launched.args.extend(["--logger".to_string(), format!("trx;LogFilePrefix={}", self.prefix)]);
        launched.args.extend_from_slice(&config.args[at..]);
        launched
    }

    /// Notes the report path a line of the run's output announces.
    pub fn feed(&mut self, line: &str) {
        let Some(path) = REPORT_PATH.find(line).map(|path| path.as_str().trim()) else { return };
        let own = || path.rsplit(['/', '\\']).next().is_some_and(|name| name.starts_with(&self.prefix));
        if (self.mtp || own()) && !self.paths.iter().any(|known| known == path) {
            self.paths.push(path.to_string());
        }
    }

    /// The reports to read once the run has ended, in the order they were announced.
    pub fn paths(&self) -> &[String] {
        &self.paths
    }
}

/// The events of the reports whose texts are `reports`.
pub fn report_events<S: AsRef<str>>(reports: &[S]) -> Result<Vec<TestEvent>, InvalidTrx> {
    let mut events = Vec::new();
    for report in reports {
        events.extend(trx_events(report.as_ref())?);
    }
    Ok(events)
}

/// The text is not a complete TRX report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTrx;

impl std::fmt::Display for InvalidTrx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid TRX report")
    }
}

impl std::error::Error for InvalidTrx {}

/// What an element holds, in document order.
#[derive(Debug)]
enum Node {
    Element(Element),
    Text(String),
}

/// An element of a report. Its name is the local one: reports differ in how they spell the namespace.
#[derive(Debug, Default)]
struct Element {
    name: String,
    attrs: Vec<(String, String)>,
    content: Vec<Node>,
}

impl Element {
    /// The value of an attribute; empty when the element does not have it.
    fn attr(&self, name: &str) -> &str {
        self.attrs.iter().find(|(known, _)| known == name).map_or("", |(_, value)| value.as_str())
    }

    /// The elements named `name` below this one, in document order.
    fn descendants<'a>(&'a self, name: &str, out: &mut Vec<&'a Element>) {
        for node in &self.content {
            if let Node::Element(element) = node {
                if element.name == name {
                    out.push(element);
                }
                element.descendants(name, out);
            }
        }
    }

    fn all(&self, name: &str) -> Vec<&Element> {
        let mut out = Vec::new();
        self.descendants(name, &mut out);
        out
    }

    fn text(&self, out: &mut String) {
        for node in &self.content {
            match node {
                Node::Element(element) => element.text(out),
                Node::Text(text) => out.push_str(text),
            }
        }
    }
}

/// Character and predefined entity references resolved; anything else stays as written.
fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find(';');
        let decoded = end.and_then(|end| match &rest[1..end] {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            reference => {
                let code = reference.strip_prefix('#')?;
                let code = match code.strip_prefix('x') {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => code.parse().ok()?,
                };
                char::from_u32(code)
            }
        });
        match (decoded, end) {
            (Some(decoded), Some(end)) => {
                out.push(decoded);
                rest = &rest[end + 1..];
            }
            _ => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A reader of the XML that test reports are written in: elements, attributes, text, CDATA sections
/// and references. Comments, processing instructions and a document type are skipped.
struct Reader<'a> {
    rest: &'a str,
}

impl<'a> Reader<'a> {
    /// Moves past the next `end`; gives the text before it.
    fn skip_past(&mut self, end: &str) -> Option<&'a str> {
        let rest = self.rest;
        let at = rest.find(end)?;
        self.rest = &rest[at + end.len()..];
        Some(&rest[..at])
    }

    /// Skips what may stand around the root element.
    fn skip_misc(&mut self) -> Option<()> {
        loop {
            self.rest = self.rest.trim_start();
            if self.rest.starts_with("<?") {
                self.skip_past("?>")?;
            } else if self.rest.starts_with("<!--") {
                self.skip_past("-->")?;
            } else if self.rest.starts_with("<!") {
                self.skip_past(">")?;
            } else {
                return Some(());
            }
        }
    }

    /// The element that starts here, with everything in it; `None` when it is malformed or not closed.
    fn element(&mut self, depth: usize) -> Option<Element> {
        if depth > MAX_DEPTH {
            return None;
        }
        let rest = self.rest.strip_prefix('<')?;
        let name_end = rest.find(|c: char| c.is_whitespace() || c == '/' || c == '>')?;
        let qualified = &rest[..name_end];
        if qualified.is_empty() {
            return None;
        }
        self.rest = &rest[name_end..];
        let local = qualified.rsplit(':').next().unwrap_or(qualified);
        let mut element = Element { name: local.to_string(), ..Element::default() };
        loop {
            self.rest = self.rest.trim_start();
            if let Some(rest) = self.rest.strip_prefix("/>") {
                self.rest = rest;
                return Some(element);
            }
            if let Some(rest) = self.rest.strip_prefix('>') {
                self.rest = rest;
                break;
            }
            let name = self.skip_past("=")?.trim_end();
            if name.is_empty() || name.contains(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'')) {
                return None;
            }
            self.rest = self.rest.trim_start();
            let quote = if self.rest.starts_with('"') { "\"" } else { "'" };
            self.rest = self.rest.strip_prefix(quote)?;
            let value = self.skip_past(quote)?;
            element.attrs.push((name.to_string(), decode(value)));
        }
        loop {
            if let Some(rest) = self.rest.strip_prefix("</") {
                let end = rest.find('>')?;
                if rest[..end].trim_end() != qualified {
                    return None;
                }
                self.rest = &rest[end + 1..];
                return Some(element);
            }
            if self.rest.starts_with("<!--") {
                self.skip_past("-->")?;
            } else if let Some(rest) = self.rest.strip_prefix("<![CDATA[") {
                self.rest = rest;
                let text = self.skip_past("]]>")?;
                element.content.push(Node::Text(text.to_string()));
            } else if self.rest.starts_with("<?") {
                self.skip_past("?>")?;
            } else if self.rest.starts_with('<') {
                element.content.push(Node::Element(self.element(depth + 1)?));
            } else {
                let rest = self.rest;
                let end = rest.find('<')?;
                element.content.push(Node::Text(decode(&rest[..end])));
                self.rest = &rest[end..];
            }
        }
    }
}

/// The root element of a well-formed document.
fn parse(xml: &str) -> Option<Element> {
    let mut reader = Reader { rest: xml.trim_start_matches('\u{feff}') };
    reader.skip_misc()?;
    let root = reader.element(0)?;
    reader.skip_misc()?;
    reader.rest.is_empty().then_some(root)
}

/// The results of a TRX report as test events. Tests are `[assembly, class, method]`, and a
/// parameterized case is a test under its method, named as the report displays it.
pub fn trx_events(xml: &str) -> Result<Vec<TestEvent>, InvalidTrx> {
    let root = parse(xml).filter(|root| root.name == "TestRun").ok_or(InvalidTrx)?;
    let mut definitions: HashMap<&str, &Element> = HashMap::new();
    for test in root.all("UnitTest") {
        let method = test.all("TestMethod").first().copied();
        match method {
            Some(method) => definitions.insert(test.attr("id"), method),
            None => definitions.remove(test.attr("id")),
        };
    }
    let mut events = Vec::new();
    for result in root.all("UnitTestResult") {
        let Some(method) = definitions.get(result.attr("testId")) else { continue };
        let class_name = method.attr("className").split(',').next().unwrap_or_default();
        let name = method.attr("name").split('(').next().unwrap_or_default().trim();
        let source = method.attr("codeBase").replace('\\', "/");
        if class_name.is_empty() || name.is_empty() || source.is_empty() {
            continue;
        }
        let shown = Some(result.attr("testName")).filter(|shown| !shown.is_empty()).unwrap_or(name);
        let display = shown.strip_prefix(class_name).and_then(|rest| rest.strip_prefix('.')).unwrap_or(shown);
        let mut path = vec![source, class_name.to_string(), name.to_string()];
        if display != name {
            path.push(display.to_string());
        }
        let status = match result.attr("outcome") {
            "Passed" => TestCaseStatus::Passed,
            "NotExecuted" | "Inconclusive" | "Pending" => TestCaseStatus::Skipped,
            _ => TestCaseStatus::Failed,
        };
        let duration_ms = DURATION.captures(result.attr("duration")).and_then(|duration| {
            let part = |group: usize| duration[group].parse::<f64>().ok();
            Some((part(1)? * 3600.0 + part(2)? * 60.0 + part(3)?) * 1000.0)
        });
        events.push(TestEvent::result(&path, status, duration_ms));
        for tag in ["Message", "StackTrace", "StdOut", "StdErr"] {
            for node in result.all(tag) {
                let mut text = String::new();
                node.text(&mut text);
                let lines = text.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line));
                for line in lines.take(MAX_TEXT_LINES) {
                    events.push(TestEvent::output(&path, line));
                    if let Some(found) = SOURCE.captures(line) {
                        let location = file_location(&found[1], found[2].parse().ok(), None, Resolve::Path);
                        events.push(TestEvent::location(&path, location));
                    }
                }
            }
        }
    }
    Ok(events)
}

/// The `--filter` expression that runs the methods of the tests at `paths`. A parameterized case
/// reruns its method, using adapter-independent TRX method identities.
pub fn dotnet_filter(paths: &[Vec<String>]) -> Option<String> {
    if paths.iter().any(|path| path.len() < 3) {
        return None;
    }
    let mut names: Vec<String> = Vec::new();
    for path in paths {
        let name = format!("{}.{}", path[1], path[2]);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return None;
    }
    let escaped = |name: &String| {
        let mut out = String::from("FullyQualifiedName=");
        for c in name.chars() {
            if "\\()&|=!~".contains(c) {
                out.push('\\');
            }
            out.push(c);
        }
        out
    };
    Some(names.iter().map(escaped).collect::<Vec<_>>().join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::command_line::parse_args;
    use crate::run::configs::blank_config;
    use crate::run_testing::parsers::{Framework, framework_for};
    use crate::run_testing::rerun::{failed_test_paths, rerun_failed_config};
    use crate::run_testing::results::{TestCounts, TestResults};

    const XUNIT: &str = include_str!("../../../src/features/run/testing/fixtures/xunit.trx");
    const NUNIT: &str = include_str!("../../../src/features/run/testing/fixtures/nunit.trx");
    const MSTEST: &str = include_str!("../../../src/features/run/testing/fixtures/mstest.trx");
    const MTP: &str = include_str!("../../../src/features/run/testing/fixtures/mtp.trx");

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn config() -> RunConfig {
        let mut config = blank_config("dotnet:tests", "Tests", "dotnet-test");
        config.command = "dotnet".to_string();
        config.args = strings(&["test", "Tests.csproj"]);
        config.detected = true;
        config.is_test = true;
        let params = [
            ("project", "Tests.csproj"),
            ("framework", "net9.0"),
            ("programArgs", "--filter Category=Unit --no-restore -- RunConfiguration.MaxCpuCount=1"),
        ];
        config.extras.params = params.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        config
    }

    fn report(adapter: &str, source: &str) -> String {
        format!(
            r#"<TestRun xmlns="http://microsoft.com/schemas/VisualStudio/TeamTest/2010">
    <Results>
      <UnitTestResult testId="a" testName="adds (1)" outcome="Passed" duration="00:00:00.0015"/>
      <UnitTestResult testId="b" testName="adds (2)" outcome="Failed" duration="00:01:02.5"><Output><ErrorInfo>
        <Message>Expected &lt;3&gt;</Message><StackTrace>at Sample.Tests.Adds(Int32 value) in C:\My Project\Tests.cs:line 12</StackTrace>
      </ErrorInfo><StdOut>captured output</StdOut></Output></UnitTestResult>
      <UnitTestResult testId="c" testName="skipped" outcome="NotExecuted"/>
    </Results>
    <TestDefinitions>
      <UnitTest id="a"><TestMethod codeBase="{source}" adapterTypeName="{adapter}" className="Sample.Tests" name="Adds"/></UnitTest>
      <UnitTest id="b"><TestMethod codeBase="{source}" adapterTypeName="{adapter}" className="Sample.Tests" name="Adds"/></UnitTest>
      <UnitTest id="c"><TestMethod codeBase="{source}" adapterTypeName="{adapter}" className="Sample.Tests" name="Skipped"/></UnitTest>
    </TestDefinitions>
  </TestRun>"#
        )
    }

    const SOURCE_DLL: &str = "C:/My Project/bin/net9.0/Tests.dll";

    fn results_of(xml: &str) -> TestResults {
        let mut results = TestResults::new();
        results.apply(&trx_events(xml).expect("the report is valid"));
        results
    }

    /// The configuration that runs only the failed tests of `results`.
    fn rerun_failed(results: &TestResults) -> RunConfig {
        let selected = rerun_failed_config(&config(), &failed_test_paths(results));
        selected.ready().flatten().expect("the failed tests can be selected")
    }

    #[test]
    fn finds_source_locations_in_mtp_stack_traces_without_the_vstest_line_prefix() {
        let expected = file_location(r"C:\Fixture\Native\Test1.cs", Some(5), None, Resolve::Path);
        let events = trx_events(MTP).expect("the report is valid");
        let mut locations = events.iter().filter_map(|event| match event {
            TestEvent::Location { location, .. } => Some(location),
            _ => None,
        });
        assert!(locations.any(|location| *location == expected));
    }

    // The spec asserts through the `dotnet test` configuration type's command line, which this crate
    // does not have: the filter is read from the rerun's own arguments.
    #[test]
    fn reads_actual_vstest_reports_and_selects_failed_methods() {
        for xml in [XUNIT, NUNIT, MSTEST, MTP] {
            let results = results_of(xml);
            assert_eq!(results.counts(), TestCounts { passed: 2, failed: 2, skipped: 1, running: 0, total: 5 });
            let rerun = rerun_failed(&results);
            let at = rerun.args.iter().position(|arg| arg == "--filter").expect("the rerun has a filter");
            let mut selected: Vec<&str> = rerun.args[at + 1].split('|').collect();
            selected.sort_unstable();
            let methods = [
                "FullyQualifiedName=Sample.CalculatorTests.Fails",
                "FullyQualifiedName=Sample.CalculatorTests.Parameterized",
            ];
            assert_eq!(selected, methods);
            let stored: Vec<String> =
                serde_json::from_str(&rerun.extras.params["testMethods"]).expect("the methods are a JSON list");
            let mut stored: Vec<&str> = stored.iter().map(String::as_str).collect();
            stored.sort_unstable();
            assert_eq!(stored, ["Sample.CalculatorTests.Fails", "Sample.CalculatorTests.Parameterized"]);
        }
    }

    #[test]
    fn reads_outcomes_parameterized_names_duration_and_source_from_the_adapter() {
        let adapters = [
            "executor://xunit/VsTestRunner2/netcoreapp",
            "executor://nunit3testexecutor/",
            "executor://mstestadapter/v2",
        ];
        for adapter in adapters {
            let results = results_of(&report(adapter, SOURCE_DLL));
            assert_eq!(results.counts(), TestCounts { passed: 1, failed: 1, skipped: 1, running: 0, total: 3 });
            let failed = results.first_failure().expect("a test failed");
            assert_eq!(failed.name, "adds (2)");
            assert_eq!(failed.duration_ms, Some(62500.0));
            assert!(failed.output.iter().any(|line| line == "Expected <3>"));
            assert!(failed.output.iter().any(|line| line == "captured output"));
            let location = file_location(r"C:\My Project\Tests.cs", Some(12), None, Resolve::Path);
            assert_eq!(failed.location, Some(location));
        }
    }

    #[test]
    fn keeps_assemblies_and_target_frameworks_separate() {
        let mut results = TestResults::new();
        for source in ["/a/bin/net9.0/Tests.dll", "/a/bin/net8.0/Tests.dll", "/b/bin/net9.0/Tests.dll"] {
            results.apply(&trx_events(&report("adapter", source)).expect("the report is valid"));
        }
        assert_eq!(results.roots().len(), 3);
        assert_eq!(results.counts().total, 9);
    }

    #[test]
    fn rejects_incomplete_reports() {
        assert_eq!(trx_events("<TestRun>"), Err(InvalidTrx));
        assert_eq!(trx_events("<html/>"), Err(InvalidTrx));
        assert!(InvalidTrx.to_string().contains("Invalid TRX"));
    }

    // The spec's reader of report files is the caller here: the paths to read are asserted instead.
    #[test]
    fn inserts_the_logger_before_runsettings_and_reads_only_this_invocations_announced_paths() {
        let mut reports = DotnetReports::new("4f1c");
        let mut base = config();
        base.args = strings(&["test", "Tests.csproj", "--", "x=y"]);
        let launched = reports.launch(&base);
        let logger = format!("trx;LogFilePrefix={}", reports.prefix());
        assert_eq!(launched.args, ["test", "Tests.csproj", "--logger", logger.as_str(), "--", "x=y"]);
        let windows = format!(r"C:\My Project\TestResults\{}_net9.0.trx", reports.prefix());
        let unix = format!("/tmp/My Project/{}_net8.0.trx", reports.prefix());
        reports.feed(r"Results File: C:\old.trx");
        reports.feed(&format!("结果文件: {windows}"));
        reports.feed(&format!("Results File: {windows}"));
        reports.feed(&format!("Results File: {unix}"));
        assert_eq!(reports.paths(), [windows, unix]);
        let texts = vec![report("adapter", SOURCE_DLL); reports.paths().len()];
        assert!(!report_events(&texts).expect("the reports are valid").is_empty());
    }

    #[test]
    fn detects_typed_configurations_and_direct_dotnet_test_commands() {
        assert_eq!(framework_for(&config()), Framework::Dotnet);
        let mut direct = config();
        direct.kind = "shell".to_string();
        direct.command = r"C:\Program Files\dotnet\dotnet.exe".to_string();
        assert_eq!(framework_for(&direct), Framework::Dotnet);
        let mut build = config();
        build.kind = "shell".to_string();
        build.args = strings(&["build"]);
        assert_eq!(framework_for(&build), Framework::Auto);
    }

    // The spec regenerates the command line through the `dotnet test` configuration type, which this
    // crate does not have: the regenerated parts are asserted where the rerun stores them.
    #[test]
    fn keeps_a_method_filter_through_net_command_regeneration_and_preserves_runsettings() {
        let base = config();
        let selected = rerun_failed(&results_of(&report("adapter", SOURCE_DLL)));
        let filter = "FullyQualifiedName=Sample.Tests.Adds";
        assert_eq!(selected.args, ["test", "Tests.csproj", "--filter", filter]);
        assert_eq!(selected.extras.params["testMethods"], r#"["Sample.Tests.Adds"]"#);
        assert_eq!(
            parse_args(&selected.extras.params["programArgs"], false),
            ["--no-restore", "--filter", filter, "--", "RunConfiguration.MaxCpuCount=1"]
        );
        assert_eq!(selected.extras.params["framework"], "net9.0");
        assert!(base.extras.params["programArgs"].contains("Category=Unit"));
    }
}
