//! "Rerun Failed Tests" and running tests selected in the test tree: derives a run configuration
//! that runs only some tests of a previous run, using each framework's own filter syntax. A plugin's
//! test framework selects its tests itself: the caller asks the plugin and continues with its answer.

use crate::platform::command_line::{format_args, parse_args};
use crate::run::command_name::append_script_args;
use crate::run::configs::{param, to_cwd};
use crate::run::django_types::django_tests_of;
use crate::run::go_arguments::{go_test_parts, without_go_selection};
use crate::run_testing::dotnet::dotnet_filter;
use crate::run_testing::parsers::{Framework, framework_for};
use crate::run_testing::phpunit::{phpunit_args, phpunit_filter};
use crate::run_testing::results::{TestNode, TestResults};
use crate::run_testing::types::TestStatus;
use ide_run::RunConfig;
use std::collections::HashSet;

pattern!(PYTHON_FILE, r"\.py(::|$)");
pattern!(TESTS_DIR, r"^tests?/?$");
pattern!(MAVEN_COMMAND, r"(?-u:\b)mvnw?(?-u:\b)");

fn collect_failed(results: &TestResults, node: &TestNode, out: &mut Vec<Vec<String>>) {
    let failed = |node: &&TestNode| results.status(node) == Some(TestStatus::Failed);
    let failed_children: Vec<&TestNode> = results.children(node).into_iter().filter(failed).collect();
    if failed_children.is_empty() {
        if node.own == Some(TestStatus::Failed) {
            out.push(node.path.clone());
        }
        return;
    }
    for child in failed_children {
        collect_failed(results, child, out);
    }
}

/// Paths of failed tests: failed leaves, plus failed suites without any failed children reported.
pub fn failed_test_paths(results: &TestResults) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for root in results.roots() {
        collect_failed(results, root, &mut out);
    }
    out
}

/// `text` as a literal in a regular expression.
pub fn escape_reg_exp(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The items, each once, in the order they first occur.
fn unique(items: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Splits `args` at the first `--` (args for the test binary / runner).
fn split_dash_dash(args: &[String]) -> (&[String], Option<&[String]>) {
    match args.iter().position(|arg| arg == "--") {
        Some(at) => (&args[..at], Some(&args[at + 1..])),
        None => (args, None),
    }
}

/// Removes `flag value` / `flag=value` pairs.
fn drop_flag(args: &[String], flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if flags.contains(&arg) {
            i += 2;
            continue;
        }
        let attached = |flag: &&str| arg.strip_prefix(*flag).is_some_and(|rest| rest.starts_with('='));
        if !flags.iter().any(attached) {
            out.push(arg.to_string());
        }
        i += 1;
    }
    out
}

fn cargo_args(base: &[String], paths: &[Vec<String>]) -> Option<Vec<String>> {
    // Paths are [suite (src/lib.rs, tests/x.rs or a nextest binary id), ...module::name parts].
    let tests = paths.iter().filter(|path| !path.first().is_some_and(|suite| suite.starts_with("Doc-tests ")));
    let names = unique(tests.map(|path| if path.len() > 1 { path[1..].join("::") } else { path.join("::") }));
    if names.is_empty() {
        return None;
    }
    let (before, after) = split_dash_dash(base);
    let rest = after.unwrap_or_default().iter().filter(|arg| *arg != "--exact").cloned();
    let mut args = before.to_vec();
    args.push("--".to_string());
    args.extend(rest);
    args.extend(names);
    args.push("--exact".to_string());
    Some(args)
}

fn dotnet_args(base: &[String], filter: &str) -> Vec<String> {
    let (before, after) = split_dash_dash(base);
    let mut args = drop_flag(before, &["--filter"]);
    args.extend(["--filter".to_string(), filter.to_string()]);
    if let Some(after) = after {
        args.push("--".to_string());
        args.extend_from_slice(after);
    }
    args
}

fn js_args(config: &RunConfig, paths: &[Vec<String>]) -> Option<Vec<String>> {
    let files = unique(paths.iter().filter_map(|path| path.first()).filter(|file| !file.is_empty()).cloned());
    let names = unique(paths.iter().filter(|path| path.len() > 1).map(|path| path[1..].join(" ")));
    if files.is_empty() {
        return None;
    }
    let (before, after) = split_dash_dash(&config.args);
    let flags = ["-t", "--testNamePattern"];
    let args = match after {
        None => drop_flag(before, &flags),
        Some(after) => {
            let mut args = before.to_vec();
            args.push("--".to_string());
            args.extend(drop_flag(after, &flags));
            args
        }
    };
    let mut extra = files;
    if !names.is_empty() {
        let pattern = names.iter().map(|name| escape_reg_exp(name)).collect::<Vec<_>>().join("|");
        extra.extend(["-t".to_string(), format!("^(?:{pattern})$")]);
    }
    Some(append_script_args(&config.command, &args, &extra))
}

fn pytest_args(base: &[String], paths: &[Vec<String>]) -> Option<Vec<String>> {
    let ids = unique(paths.iter().map(|path| path.join("::")));
    if ids.is_empty() {
        return None;
    }
    // Drop explicit test paths / -k expressions of the original run; keep options and `-m pytest` prefixes.
    let mut args = drop_flag(base, &["-k"]);
    args.retain(|arg| !PYTHON_FILE.is_match(arg) && !TESTS_DIR.is_match(arg));
    args.extend(ids);
    Some(args)
}

/// `-run` / `-bench` pattern of tests: each name anchored, subtests after `/`.
fn go_pattern(selected: &[&Vec<String>]) -> String {
    let exact = |path: &&Vec<String>| {
        path[1..].iter().map(|name| format!("^{}$", escape_reg_exp(name))).collect::<Vec<_>>().join("/")
    };
    unique(selected.iter().map(exact)).join("|")
}

fn go_args(base: &[String], paths: &[Vec<String>], file: Option<&str>) -> Option<Vec<String>> {
    let named = paths.iter().filter_map(|path| path.first()).filter(|package| !package.is_empty());
    let packages = unique(named.cloned());
    if packages.len() != 1 {
        return None;
    }
    let parts = go_test_parts(base);
    let targets = match file {
        Some(file) => vec![file.to_string()],
        None if packages[0] == "command-line-arguments" => parts.targets.clone(),
        None => packages,
    };
    let mut args = vec!["test".to_string()];
    if paths.iter().any(|path| path.len() == 1) {
        args.extend(parts.flags);
        args.extend(targets);
        args.extend(parts.program);
        return Some(args);
    }
    let is_benchmark = |path: &&Vec<String>| path.get(1).is_some_and(|name| name.starts_with("Benchmark"));
    let (benchmarks, tests): (Vec<&Vec<String>>, Vec<&Vec<String>>) = paths.iter().partition(is_benchmark);
    args.extend(without_go_selection(&parts.flags));
    args.push("-run".to_string());
    args.push(if tests.is_empty() { "^$".to_string() } else { go_pattern(&tests) });
    if !benchmarks.is_empty() {
        args.extend(["-bench".to_string(), go_pattern(&benchmarks)]);
    }
    args.extend(targets);
    args.extend(without_go_selection(&parts.program));
    Some(args)
}

fn jvm_args(config: &RunConfig, paths: &[Vec<String>]) -> Option<Vec<String>> {
    // A test is its class and method; the method is the last part (nested classes are in between).
    let method = |path: &Vec<String>| {
        let last = path.last().map_or("", String::as_str);
        (path[0].clone(), last.strip_suffix("()").unwrap_or(last).to_string())
    };
    let tests: Vec<(String, String)> = paths.iter().filter(|path| path.len() >= 2).map(method).collect();
    let classes = unique(paths.iter().filter(|path| path.len() == 1).map(|path| path[0].clone()));
    if tests.is_empty() && classes.is_empty() {
        return None;
    }
    if config.kind == "maven" || MAVEN_COMMAND.is_match(&config.command) {
        let mut by_class: Vec<(String, Vec<String>)> = Vec::new();
        for (class, method) in tests {
            match by_class.iter_mut().find(|(known, _)| *known == class) {
                Some((_, methods)) => methods.push(method),
                None => by_class.push((class, vec![method])),
            }
        }
        let alone = classes.iter().filter(|class| !by_class.iter().any(|(known, _)| known == *class)).cloned();
        let alone: Vec<String> = alone.collect();
        let mut specs: Vec<String> =
            by_class.into_iter().map(|(class, methods)| format!("{class}#{}", unique(methods).join("+"))).collect();
        specs.extend(alone);
        let mut args = config.args.clone();
        args.retain(|arg| !arg.starts_with("-Dtest="));
        args.push(format!("-Dtest={}", specs.join(",")));
        return Some(args);
    }
    let mut filters = unique(tests.into_iter().map(|(class, method)| format!("{class}.{method}")));
    filters.extend(classes);
    let mut args = drop_flag(&config.args, &["--tests"]);
    args.extend(filters.into_iter().flat_map(|filter| ["--tests".to_string(), filter]));
    Some(args)
}

fn collect_leaves(results: &TestResults, node: &TestNode, seen: &mut HashSet<String>, out: &mut Vec<Vec<String>>) {
    if node.is_leaf() {
        if seen.insert(node.id.clone()) {
            out.push(node.path.clone());
        }
        return;
    }
    for child in results.children(node) {
        collect_leaves(results, child, seen, out);
    }
}

/// Leaf test paths under the given nodes (a node without reported children stands for itself).
pub fn leaf_test_paths(results: &TestResults, nodes: &[&TestNode]) -> Vec<Vec<String>> {
    let (mut seen, mut out) = (HashSet::new(), Vec::new());
    for node in nodes {
        collect_leaves(results, node, &mut seen, &mut out);
    }
    out
}

/// A selection of tests as run configurations: ready, or for a plugin's framework to make.
#[derive(Clone, Debug, PartialEq)]
pub enum Selected<T> {
    Ready(T),
    /// Ask the plugin for the arguments that run the tests, then call [`PluginSelection::selected`].
    Plugin(Box<PluginSelection>),
}

impl<T> Selected<T> {
    /// The selection when no plugin has to be asked.
    pub fn ready(self) -> Option<T> {
        match self {
            Self::Ready(ready) => Some(ready),
            Self::Plugin(_) => None,
        }
    }
}

/// Tests of a plugin's framework to select.
#[derive(Clone, Debug, PartialEq)]
pub struct PluginSelection {
    config: RunConfig,
    paths: Vec<Vec<String>>,
    name: String,
}

impl PluginSelection {
    /// The configuration whose plugin selects the tests.
    pub fn config_id(&self) -> &str {
        &self.config.id
    }

    /// The tests to select, as their name paths in the test tree.
    pub fn paths(&self) -> &[Vec<String>] {
        &self.paths
    }

    /// The configuration running the tests with the arguments the plugin gave; `None` when it gave
    /// none (no selector, a failure, or tests it cannot select).
    pub fn selected(&self, args: Option<Vec<String>>) -> Option<RunConfig> {
        Some(RunConfig { args: args?, name: self.name.clone(), detected: true, ..self.config.clone() })
    }
}

fn plugin_selection(config: &RunConfig, paths: &[Vec<String>], name: &str) -> Option<PluginSelection> {
    let plugin = param(config, "framework") == Some("plugin");
    plugin.then(|| PluginSelection { config: config.clone(), paths: paths.to_vec(), name: name.to_string() })
}

/// The name of the run of a configuration's failed tests.
fn failed_name(config: &RunConfig) -> String {
    format!("Failed: {}", config.name.strip_prefix("Failed: ").unwrap_or(&config.name))
}

/// Config running only `paths` (failed test paths from the results tree); `None` when unsupported, or
/// when the tests need several runs.
pub fn rerun_failed_config(config: &RunConfig, paths: &[Vec<String>]) -> Selected<Option<RunConfig>> {
    match rerun_failed_configs(config, paths) {
        Selected::Ready(mut configs) => Selected::Ready(if configs.len() == 1 { configs.pop() } else { None }),
        Selected::Plugin(selection) => Selected::Plugin(selection),
    }
}

/// Configs that together run only `paths`; none when unsupported.
pub fn rerun_failed_configs(config: &RunConfig, paths: &[Vec<String>]) -> Selected<Vec<RunConfig>> {
    tests_configs(config, paths, &failed_name(config))
}

/// Configs that together run the tests at `paths`, named `name`. Go filters apply to every package
/// in an invocation, so each package keeps its own selection.
pub fn tests_configs(config: &RunConfig, paths: &[Vec<String>], name: &str) -> Selected<Vec<RunConfig>> {
    if paths.is_empty() {
        return Selected::Ready(Vec::new());
    }
    if let Some(selection) = plugin_selection(config, paths, name) {
        return Selected::Plugin(Box::new(selection));
    }
    let by_package = framework_for(config) == Framework::Go;
    let mut groups: Vec<(&str, Vec<Vec<String>>)> = Vec::new();
    for path in paths {
        let key = path.first().filter(|_| by_package).map_or("", String::as_str);
        match groups.iter_mut().find(|(known, _)| *known == key) {
            Some((_, group)) => group.push(path.clone()),
            None => groups.push((key, vec![path.clone()])),
        }
    }
    let several = groups.len() > 1;
    let mut configs = Vec::new();
    for (package, selected) in &groups {
        let name = if several { format!("{name} ({package})") } else { name.to_string() };
        let Some(mut derived) = builtin_tests_config(config, selected, &name) else { continue };
        if several {
            derived.id = format!("context:tests:{}:{package}", config.id);
        }
        configs.push(derived);
    }
    Selected::Ready(configs)
}

/// Config running only the tests at `paths` (test tree paths), named `name`; `None` when unsupported.
pub fn tests_config(config: &RunConfig, paths: &[Vec<String>], name: &str) -> Selected<Option<RunConfig>> {
    if paths.is_empty() {
        return Selected::Ready(None);
    }
    match plugin_selection(config, paths, name) {
        Some(selection) => Selected::Plugin(Box::new(selection)),
        None => Selected::Ready(builtin_tests_config(config, paths, name)),
    }
}

fn builtin_tests_config(config: &RunConfig, paths: &[Vec<String>], name: &str) -> Option<RunConfig> {
    if paths.is_empty() {
        return None;
    }
    let framework = framework_for(config);
    let mut derived = RunConfig { name: name.to_string(), detected: true, ..config.clone() };
    derived.args = match framework {
        Framework::Dotnet => {
            let filter = dotnet_filter(paths)?;
            if config.kind == "dotnet-test" {
                let methods = unique(paths.iter().map(|path| format!("{}.{}", path[1], path[2])));
                let program = parse_args(param(config, "programArgs").unwrap_or_default(), false);
                let methods = serde_json::to_string(&methods).ok()?;
                derived.extras.params.insert("testMethods".to_string(), methods);
                let program = format_args(&dotnet_args(&program, &filter));
                derived.extras.params.insert("programArgs".to_string(), program);
            }
            dotnet_args(&config.args, &filter)
        }
        Framework::Cargo => cargo_args(&config.args, paths)?,
        Framework::Js => js_args(config, paths)?,
        Framework::Pytest => pytest_args(&config.args, paths)?,
        Framework::Unittest => {
            // Django: dotted labels (`polls.tests.QuestionTests.test_x`) in place of the run's own target.
            let labels = unique(paths.iter().map(|path| path.join(".")));
            let (args, params) = django_tests_of(config, &labels)?;
            if let Some(params) = params {
                derived.extras.params = params;
            }
            args
        }
        Framework::Phpunit => phpunit_args(&config.args, None, Some(&phpunit_filter(paths))),
        Framework::Go => {
            let file_scope = param(config, "scope") == Some("file");
            let file = param(config, "target").filter(|target| file_scope && !target.is_empty());
            go_args(&config.args, paths, file)?
        }
        Framework::Jvm => jvm_args(config, paths)?,
        Framework::Ruby | Framework::Auto => return None,
    };
    if framework == Framework::Go && param(config, "goKind").is_some_and(|kind| !kind.is_empty()) {
        if let Some(module) = param(config, "module").filter(|module| !module.is_empty()) {
            derived.cwd = to_cwd(Some(module));
        }
        if param(config, "scope") == Some("file") {
            let parts = go_test_parts(&derived.args);
            let run = derived.args.iter().position(|arg| arg == "-run");
            let pattern = run.and_then(|at| derived.args.get(at + 1)).cloned().unwrap_or_default();
            let tool_args = format_args(&drop_flag(&parts.flags, &["-run", "-test.run"]));
            let program_args = format_args(parts.program.get(1..).unwrap_or_default());
            let typed = [("pattern", pattern), ("toolArgs", tool_args), ("programArgs", program_args)];
            derived.extras.params.extend(typed.map(|(key, value)| (key.to_string(), value)));
        } else {
            let kept = |key: &str, default: &str| (key.to_string(), param(config, key).unwrap_or(default).to_string());
            derived.extras.params = [kept("module", ""), kept("useTags", "true")].into_iter().collect();
        }
    }
    Some(derived)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::configs::blank_config;
    use crate::run_testing::parsers::with_test_reporting;
    use crate::run_testing::types::{TestCaseStatus, TestEvent};

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    fn paths(items: &[&[&str]]) -> Vec<Vec<String>> {
        items.iter().map(|path| strings(path)).collect()
    }

    fn cfg(kind: &str, command: &str, args: &[&str]) -> RunConfig {
        let mut config = blank_config(format!("{kind}:test"), format!("{kind} test"), kind);
        config.command = command.to_string();
        config.args = strings(args);
        config.detected = true;
        config.is_test = true;
        config
    }

    fn with_params(mut config: RunConfig, params: &[(&str, &str)]) -> RunConfig {
        config.extras.params = params.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
        config
    }

    fn result(path: &[&str], status: TestCaseStatus) -> TestEvent {
        TestEvent::result(&strings(path), status, None)
    }

    /// The single configuration that reruns the failed tests at `failed`.
    fn rerun(config: &RunConfig, failed: &[&[&str]]) -> RunConfig {
        rerun_failed_config(config, &paths(failed)).ready().flatten().expect("the tests can be selected")
    }

    /// The configurations of a selection that needs no plugin.
    fn ready(selection: Selected<Vec<RunConfig>>) -> Vec<RunConfig> {
        selection.ready().expect("no plugin is asked")
    }

    /// The request to a plugin's framework.
    fn plugin<T>(selection: Selected<T>) -> PluginSelection {
        match selection {
            Selected::Plugin(selection) => *selection,
            Selected::Ready(_) => panic!("the plugin selects the tests"),
        }
    }

    const PASSED: TestCaseStatus = TestCaseStatus::Passed;
    const FAILED: TestCaseStatus = TestCaseStatus::Failed;

    #[test]
    fn collects_failed_leaves_and_failed_suites_without_failed_children() {
        let mut r = TestResults::new();
        r.apply(&[
            result(&["a.test.ts", "adds"], PASSED),
            result(&["a.test.ts", "subs"], FAILED),
            result(&["b.test.ts"], FAILED),
            result(&["b.test.ts", "ok"], PASSED),
        ]);
        assert_eq!(failed_test_paths(&r), paths(&[&["a.test.ts", "subs"], &["b.test.ts"]]));
    }

    #[test]
    fn expands_suites_to_their_tests_a_suite_without_reported_tests_stands_for_itself() {
        let mut r = TestResults::new();
        r.apply(&[
            result(&["com.acme.ATest", "one()"], PASSED),
            result(&["com.acme.ATest", "two()"], FAILED),
            result(&["com.acme.BTest"], PASSED),
        ]);
        let node = |path: &[&str]| r.node_at(path).expect("the node is reported");
        let nodes = [node(&["com.acme.ATest"]), node(&["com.acme.ATest", "one()"]), node(&["com.acme.BTest"])];
        let leaves = leaf_test_paths(&r, &nodes);
        let expected: [&[&str]; 3] = [&["com.acme.ATest", "one()"], &["com.acme.ATest", "two()"], &["com.acme.BTest"]];
        assert_eq!(leaves, paths(&expected));
        let selected = tests_config(&cfg("gradle", "./gradlew", &["test"]), &leaves, "3 tests");
        let selected = selected.ready().flatten().expect("the tests can be selected");
        assert_eq!(selected.name, "3 tests");
        assert_eq!(
            selected.args,
            ["test", "--tests", "com.acme.ATest.one", "--tests", "com.acme.ATest.two", "--tests", "com.acme.BTest"]
        );
    }

    #[test]
    fn cargo_exact_names_after_double_dash() {
        let failed: [&[&str]; 3] = [
            &["src/lib.rs", "tests", "adds"],
            &["tests/it.rs", "smoke"],
            &["Doc-tests app", "src/lib.rs - foo (line 3)"],
        ];
        let c = rerun(&cfg("cargo", "cargo", &["test", "--workspace"]), &failed);
        assert_eq!(c.args, ["test", "--workspace", "--", "tests::adds", "smoke", "--exact"]);
        assert_eq!(c.name, "Failed: cargo test");
    }

    #[test]
    fn cargo_merges_an_existing_double_dash_section_without_duplicating_exact() {
        let base = cfg("cargo", "cargo", &["test", "--", "--nocapture", "--exact", "old"]);
        let c = rerun(&base, &[&["src/lib.rs", "a"]]);
        assert_eq!(c.args, ["test", "--", "--nocapture", "old", "a", "--exact"]);
    }

    #[test]
    fn jest_vitest_files_and_t_pattern() {
        let commands: [(&str, &[&str]); 7] = [
            ("npm", &["--"]),
            ("pnpm", &[]),
            ("/opt/bin/pnpm", &[]),
            (r"C:\tools\pnpm.CMD", &[]),
            ("yarn", &["--"]),
            ("bun", &["--"]),
            ("npx", &[]),
        ];
        for (command, separator) in commands {
            let failed: [&[&str]; 2] = [&["src/a.test.ts", "math", "adds (1+1)"], &["src/b.test.ts"]];
            let c = rerun(&cfg("npm", command, &["run", "test"]), &failed);
            let mut expected = strings(&["run", "test"]);
            expected.extend(strings(separator));
            expected.extend(strings(&["src/a.test.ts", "src/b.test.ts", "-t", r"^(?:math adds \(1\+1\))$"]));
            assert_eq!(c.args, expected, "{command}");
        }
    }

    #[test]
    fn replaces_an_existing_pnpm_test_filter() {
        let bases: [&[&str]; 2] = [&["run", "test", "-t", "old"], &["run", "test", "--", "--testNamePattern=old"]];
        for args in bases {
            let c = rerun(&cfg("npm", "pnpm", args), &[&["src/a.test.ts", "new"]]);
            let mut expected = strings(&["run", "test"]);
            if args.contains(&"--") {
                expected.push("--".to_string());
            }
            expected.extend(strings(&["src/a.test.ts", "-t", "^(?:new)$"]));
            assert_eq!(c.args, expected);
        }
    }

    #[test]
    fn pytest_node_ids_replace_explicit_paths() {
        let base = cfg("pytest", ".venv/bin/pytest", &["-q", "tests/", "-k", "slow"]);
        let failed: [&[&str]; 2] = [&["tests/test_x.py", "TestC", "test_y[1]"], &["tests/test_z.py", "test_a"]];
        let c = rerun(&base, &failed);
        assert_eq!(c.args, ["-q", "tests/test_x.py::TestC::test_y[1]", "tests/test_z.py::test_a"]);
    }

    #[test]
    fn django_dotted_labels_in_place_of_the_runs_target_options_kept() {
        let base = cfg("django-test", ".venv/bin/python", &["manage.py", "test", "polls", "--keepdb", "-v", "2"]);
        let failed: [&[&str]; 3] =
            [&["polls.tests", "QuestionTests", "test_a"], &["polls.tests", "BrokenSetupTests"], &["broken.tests"]];
        let c = rerun(&base, &failed);
        let expected = [
            "manage.py",
            "test",
            "polls.tests.QuestionTests.test_a",
            "polls.tests.BrokenSetupTests",
            "broken.tests",
            "--keepdb",
            "-v",
            "2",
        ];
        assert_eq!(c.args, expected);
        // A configuration made in the form keeps its Target field in step.
        let form = with_params(
            cfg("django-test", "uv", &["run", "python", "manage.py", "test", "polls"]),
            &[("command", "uv"), ("runnerArgs", "run python"), ("target", "polls"), ("settings", ""), ("options", "")],
        );
        let c = rerun(&form, &[&["polls.tests", "QuestionTests", "test_a"]]);
        assert_eq!(c.args, ["run", "python", "manage.py", "test", "polls.tests.QuestionTests.test_a"]);
        assert_eq!(c.extras.params["target"], "polls.tests.QuestionTests.test_a");
    }

    #[test]
    fn go_keeps_full_subtest_paths_separate_for_each_package() {
        let failed: [&[&str]; 3] = [
            &["example.com/m/pkg", "TestAdd"],
            &["example.com/m/pkg", "TestSub", "case_1"],
            &["example.com/m/other", "TestX"],
        ];
        let configs = ready(rerun_failed_configs(&cfg("go", "go", &["test", "-v", "./..."]), &paths(&failed)));
        assert_eq!(configs.len(), 2);
        assert_eq!(configs[0].args, ["test", "-v", "-run", "^TestAdd$|^TestSub$/^case_1$", "example.com/m/pkg"]);
        assert_eq!(configs[1].args, ["test", "-v", "-run", "^TestX$", "example.com/m/other"]);
        assert_ne!(configs[0].id, configs[1].id);
    }

    #[test]
    fn gradle_and_maven_filters() {
        let gradle = rerun(&cfg("gradle", "./gradlew", &["test"]), &[&["com.a.FooTest", "bar()"]]);
        assert_eq!(gradle.args, ["test", "--tests", "com.a.FooTest.bar"]);
        let failed: [&[&str]; 3] = [&["com.a.FooTest", "bar"], &["com.a.FooTest", "baz"], &["com.a.BarTest"]];
        let maven = rerun(&cfg("maven", "mvn", &["test"]), &failed);
        assert_eq!(maven.args, ["test", "-Dtest=com.a.FooTest#bar+baz,com.a.BarTest"]);
    }

    #[test]
    fn returns_none_when_nothing_failed_or_the_framework_is_unknown() {
        assert_eq!(rerun_failed_config(&cfg("cargo", "cargo", &["test"]), &[]), Selected::Ready(None));
        let unknown = rerun_failed_config(&cfg("make", "make", &["test"]), &paths(&[&["x"]]));
        assert_eq!(unknown, Selected::Ready(None));
    }

    #[test]
    fn asks_pytest_for_one_line_per_test_unless_a_verbosity_was_chosen() {
        let reported = |kind: &str, command: &str, args: &[&str]| with_test_reporting(&cfg(kind, command, args)).args;
        assert_eq!(reported("pytest", "python3", &["-m", "pytest", "t.py"]), ["-m", "pytest", "t.py", "-v"]);
        assert_eq!(reported("pytest", "uv", &["run", "pytest"]), ["run", "pytest", "-v"]);
        assert_eq!(reported("pytest", ".venv/bin/pytest", &["-q"]), ["-q"]);
        assert_eq!(reported("pytest", "pytest", &["-vv"]), ["-vv"]);
        // Django prints one line per test from verbosity 2.
        assert_eq!(
            reported("django-test", "python3", &["manage.py", "test", "--keepdb"]),
            ["manage.py", "test", "--keepdb", "-v", "2"]
        );
        assert_eq!(
            reported("django-test", "python3", &["manage.py", "test", "-v", "3"]),
            ["manage.py", "test", "-v", "3"]
        );
        assert_eq!(
            reported("shell", "python3", &["manage.py", "test", "--verbosity=0"]),
            ["manage.py", "test", "--verbosity=0"]
        );
        // A plain script run (no pytest) and other frameworks are left alone.
        assert_eq!(reported("python", "python3", &["main.py"]), ["main.py"]);
        assert_eq!(reported("cargo", "cargo", &["test"]), ["test"]);
    }

    #[test]
    fn does_not_retain_package_targets_after_the_boolean_flag() {
        for flag in ["-test.v", "-test.short", "-test.benchmem", "-test.failfast", "-test.fullpath"] {
            let config = cfg("go", "go", &["test", flag, "./..."]);
            let c = rerun(&config, &[&["example.com/selected", "TestOnly"]]);
            assert_eq!(c.args, ["test", flag, "-run", "^TestOnly$", "example.com/selected"]);
        }
    }

    #[test]
    fn replaces_import_path_targets_without_losing_flag_values_or_binary_arguments() {
        let args = ["test", "-tags", "demo", "-run", "old", "example.com/old", "-args", "-test.short"];
        let c = rerun(&cfg("go", "go", &args), &[&["example.com/new", "TestChild", "case"]]);
        let expected =
            ["test", "-tags", "demo", "-run", "^TestChild$/^case$", "example.com/new", "-args", "-test.short"];
        assert_eq!(c.args, expected);
    }

    #[test]
    fn retains_file_scoped_preparation() {
        for package in ["command-line-arguments", "example.com/app"] {
            let params = [
                ("goKind", "test"),
                ("scope", "file"),
                ("target", "main_test.go"),
                ("module", "app"),
                ("pattern", ""),
                ("useTags", "true"),
            ];
            let config = with_params(cfg("go", "go", &["test", "main_test.go"]), &params);
            let c = rerun(&config, &[&[package, "TestFail"]]);
            let param = |key: &str| c.extras.params.get(key).map(String::as_str);
            assert_eq!(param("goKind"), Some("test"));
            assert_eq!(param("scope"), Some("file"));
            assert_eq!(param("target"), Some("main_test.go"));
            assert_eq!(param("pattern"), Some("^TestFail$"));
            assert_eq!(c.args, ["test", "-run", "^TestFail$", "main_test.go"]);
        }
    }

    #[test]
    fn keeps_benchmark_selectors_and_removes_stale_selectors_from_binary_arguments() {
        let args = [
            "test",
            "-tags=demo",
            "-run=Old",
            "-bench",
            "Old",
            "./...",
            "-args",
            "-test.run",
            "Old",
            "-test.bench=Old",
            "-test.skip=New",
            "-custom",
            "keep",
        ];
        let selected = paths(&[&["example.com/p", "BenchmarkNew", "case"]]);
        let configs = ready(tests_configs(&cfg("go", "go", &args), &selected, "selected"));
        let expected = [
            "test",
            "-tags=demo",
            "-run",
            "^$",
            "-bench",
            "^BenchmarkNew$/^case$",
            "example.com/p",
            "-args",
            "-custom",
            "keep",
        ];
        assert_eq!(configs[0].args, expected);
    }

    #[test]
    fn keeps_independent_alternatives_for_tests_of_different_depths() {
        let selected: [&[&str]; 3] =
            [&["p", "TestA", "one"], &["p", "TestB", "two", "leaf.with.dot"], &["p", "BenchmarkC"]];
        let configs = ready(tests_configs(&cfg("go", "go", &["test", "."]), &paths(&selected), "selected"));
        let run = r"^TestA$/^one$|^TestB$/^two$/^leaf\.with\.dot$";
        assert_eq!(configs[0].args, ["test", "-run", run, "-bench", "^BenchmarkC$", "p"]);
    }

    #[test]
    fn keeps_typed_file_benchmark_flags_and_clears_overridden_binary_selectors() {
        let params = [
            ("goKind", "test"),
            ("scope", "file"),
            ("target", "chosen_test.go"),
            ("module", "app"),
            ("programArgs", "-test.run=Old -custom keep"),
        ];
        let args = ["test", "chosen_test.go", "-args", "-test.run=Old", "-custom", "keep"];
        let base = with_params(cfg("go", "go", &args), &params);
        let selected = paths(&[&["example.com/p", "BenchmarkNew"]]);
        let configs = ready(tests_configs(&base, &selected, "selected"));
        let param = |key: &str| configs[0].extras.params.get(key).map(String::as_str);
        assert_eq!(param("target"), Some("chosen_test.go"));
        assert_eq!(param("pattern"), Some("^$"));
        assert_eq!(param("toolArgs"), Some("-bench '^BenchmarkNew$'"));
        assert_eq!(param("programArgs"), Some("-custom keep"));
        assert!(configs[0].args.iter().any(|arg| arg == "chosen_test.go"));
    }

    #[test]
    fn reruns_an_entire_failed_package_with_its_original_runner_options() {
        let base = cfg("go", "go", &["test", "-race", "-bench", ".", "./..."]);
        let configs = ready(tests_configs(&base, &paths(&[&["example.com/p"]]), "selected"));
        assert_eq!(configs[0].args, ["test", "-race", "-bench", ".", "example.com/p"]);
    }

    #[test]
    fn keeps_inherited_go_build_tags_enabled_when_deriving_a_typed_package_rerun() {
        let params = [("goKind", "test"), ("scope", "package"), ("target", "./...")];
        let config = with_params(cfg("go", "go", &["test", "./..."]), &params);
        let c = rerun(&config, &[&["example.com/p", "TestChosen"]]);
        assert_eq!(c.extras.params.get("useTags").map(String::as_str), Some("true"));
    }

    fn plugin_config() -> RunConfig {
        let mut config = with_params(cfg("plugin", "runner", &["test", "--old-filter"]), &[("framework", "plugin")]);
        config.id = "plugin:example:tests".to_string();
        config.env.insert("EXAMPLE".to_string(), "yes".to_string());
        config.cwd = to_cwd(Some("/project"));
        config.extras.target = Some("plugin:example:container".to_string());
        config
    }

    // The spec's selector is the caller here: what it would be called with is the request, and its
    // answer is passed to the request.
    #[test]
    fn passes_the_owning_config_and_exact_failed_name_paths_replacing_argv_and_preserving_launch_settings() {
        let base = plugin_config();
        let mut results = TestResults::new();
        results.apply(&[result(&["suite", "fails"], FAILED), result(&["suite", "passes"], PASSED)]);
        let request = plugin(rerun_failed_configs(&base, &failed_test_paths(&results)));
        assert_eq!(request.config_id(), base.id);
        assert_eq!(request.paths(), paths(&[&["suite", "fails"]]));
        let args = strings(&["test", "--only", "suite::fails"]);
        let expected = RunConfig { name: "Failed: plugin test".to_string(), args: args.clone(), ..base.clone() };
        assert_eq!(request.selected(Some(args)), Some(expected));
        assert_eq!(base.args, ["test", "--old-filter"]);
    }

    #[test]
    fn uses_the_same_selection_for_test_tree_suites_and_accepts_empty_replacement_argv() {
        let base = plugin_config();
        let selected = paths(&[&["suite", "one"], &["suite", "two"]]);
        let request = plugin(tests_config(&base, &selected, "suite"));
        let expected = RunConfig { name: "suite".to_string(), args: Vec::new(), ..base.clone() };
        assert_eq!(request.selected(Some(Vec::new())), Some(expected));
        let request = plugin(tests_configs(&base, &selected, "suite"));
        assert_eq!((request.config_id(), request.paths()), (base.id.as_str(), selected.as_slice()));
    }

    // No selector, a selector that selects nothing and one that fails all reach the request as no
    // arguments.
    #[test]
    fn returns_unsupported_for_no_provider_none_errors_and_empty_selection() {
        let base = plugin_config();
        let one = paths(&[&["test"]]);
        assert_eq!(plugin(tests_config(&base, &one, "selected")).selected(None), None);
        assert_eq!(plugin(rerun_failed_configs(&base, &one)).selected(None), None);
        assert_eq!(plugin(tests_configs(&base, &one, "selected")).selected(None), None);
        assert_eq!(tests_configs(&base, &[], "selected"), Selected::Ready(Vec::new()));
    }

    #[test]
    fn keeps_built_in_selection_synchronous_without_calling_plugins() {
        let base = cfg("cargo", "cargo", &["test"]);
        let one = paths(&[&["src/lib.rs", "test"]]);
        assert!(matches!(tests_config(&base, &one, "test"), Selected::Ready(Some(_))));
        assert_eq!(ready(rerun_failed_configs(&base, &one)).len(), 1);
    }
}
