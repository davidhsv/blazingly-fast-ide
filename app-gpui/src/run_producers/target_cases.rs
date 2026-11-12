//! The run targets of each language, and the configurations narrowed to them: the detection and
//! narrowing of [`crate::run::targets`] held to what the gutter shows and runs.

use crate::run::configs::{blank_config, param, to_cwd};
use crate::run::context::RunEnv;
use crate::run::targets::{
    Lang, RunTarget, TargetKind, detect_run_targets, js_name_pattern, target_config, target_language,
};
use ide_run::RunConfig;

type Picked = (u32, TargetKind, String);

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| item.to_string()).collect()
}

/// A detected test configuration named by its id.
fn cfg(id: &str, kind: &str, command: &str, args: &[&str]) -> RunConfig {
    let base = blank_config(id, id, kind);
    RunConfig { command: command.to_string(), args: strings(args), detected: true, is_test: true, ..base }
}

fn detect(text: &str, rel_path: &str) -> Vec<RunTarget> {
    detect_run_targets(text, rel_path, false)
}

/// Each target's line, kind and path (joined with `/`).
fn pick(targets: &[RunTarget]) -> Vec<Picked> {
    targets.iter().map(|target| (target.line, target.kind, target.path.join("/"))).collect()
}

fn picked(expected: &[(u32, TargetKind, &str)]) -> Vec<Picked> {
    expected.iter().map(|(line, kind, path)| (*line, *kind, path.to_string())).collect()
}

fn paths(targets: &[RunTarget]) -> Vec<Vec<String>> {
    targets.iter().map(|target| target.path.clone()).collect()
}

/// The configuration of `target`, narrowed from the closest of `configs`.
fn narrowed(target: &RunTarget, rel_path: &str, configs: &[RunConfig], content: Option<&str>) -> Option<RunConfig> {
    target_config(target, rel_path, configs, content, None, RunEnv::default())
}

fn args_of(config: Option<RunConfig>) -> Vec<String> {
    config.map(|config| config.args).unwrap_or_default()
}

const MAIN: TargetKind = TargetKind::Main;
const TEST: TargetKind = TargetKind::Test;
const SUITE: TargetKind = TargetKind::Suite;

#[test]
fn rust_main_test_fns_with_module_path_test_modules() {
    let source = [
        "fn main() {}",
        "#[cfg(test)]",
        "mod tests {",
        "    use super::*;",
        "    #[test]",
        "    fn adds() { let s = \"}\"; }",
        "    #[tokio::test]",
        "    #[ignore]",
        "    async fn async_one() {}",
        "    fn helper() {}",
        "}",
        "fn after() {}",
    ]
    .join("\n");
    let expected = [(0, MAIN, ""), (2, SUITE, "tests"), (5, TEST, "tests/adds"), (8, TEST, "tests/async_one")];
    assert_eq!(pick(&detect(&source, "src/main.rs")), picked(&expected));
    let nested = detect("#[test]\nfn x() {}", "crates/a/src/foo/bar.rs");
    assert_eq!(pick(&nested), picked(&[(1, TEST, "foo/bar/x")]));
    assert!(detect("fn main() {}", "src/lib.rs").is_empty());
}

#[test]
fn js_and_ts_describe_chains_and_test_names() {
    let source = [
        "describe('math', () => {",
        "  it('adds', () => {});",
        "  describe(\"nested\", () => {",
        "    test.each([[1]])('n=%i', () => {});",
        "  });",
        "  test.skip(`subs`, () => {});",
        "});",
        "const item = items.find((x) => x);",
        "test('top', () => {});",
    ]
    .join("\n");
    let targets = detect(&source, "src/math.test.ts");
    let expected = [
        (0, SUITE, "math"),
        (1, TEST, "math/adds"),
        (2, SUITE, "math/nested"),
        (3, TEST, "math/nested/n=%i"),
        (5, TEST, "math/subs"),
        (8, TEST, "top"),
    ];
    assert_eq!(pick(&targets), picked(&expected));
    assert!(targets[3].parameterized);
    assert_eq!(js_name_pattern(&targets[1]), "^math adds$");
    assert_eq!(js_name_pattern(&targets[3]), "^math nested n=.*$");
    assert_eq!(js_name_pattern(&targets[0]), "^math");
    assert!(detect("test('x', () => {})", "src/math.ts").is_empty());
}

#[test]
fn python_test_functions_test_classes_main() {
    let source = [
        "class TestCalc:",
        "    def test_add(self):",
        "        pass",
        "def test_top():",
        "    pass",
        "class Helper:",
        "    def test_no(self): pass",
        "if __name__ == \"__main__\":",
    ]
    .join("\n");
    let expected = [(0, SUITE, "TestCalc"), (1, TEST, "TestCalc/test_add"), (3, TEST, "test_top"), (7, MAIN, "")];
    assert_eq!(pick(&detect(&source, "tests/test_calc.py")), picked(&expected));
}

#[test]
fn python_unittest_testcase_subclasses_in_tests_py_djangos_default_test_module() {
    let source = [
        "from django.test import TestCase",
        "class QuestionTests(TestCase):",
        "    def test_recent(self):",
        "        pass",
        "class Helper(Mixin):",
        "    def test_no(self): pass",
    ]
    .join("\n");
    let expected = [(1, SUITE, "QuestionTests"), (2, TEST, "QuestionTests/test_recent")];
    assert_eq!(pick(&detect_run_targets(&source, "polls/tests.py", true)), picked(&expected));
    assert!(detect_run_targets(&source, "polls/models.py", true).is_empty());
    // Outside Django projects `tests.py` is a script: no test targets.
    assert!(detect(&source, "polls/tests.py").is_empty());
}

#[test]
fn go_and_jvm() {
    let test = detect("package x\n\nfunc TestAdd(t *testing.T) {}\nfunc helper() {}", "pkg/add_test.go");
    assert_eq!(pick(&test), picked(&[(2, TEST, "TestAdd")]));
    let main = detect("package main\nfunc main() {}", "cmd/app/main.go");
    assert_eq!(pick(&main), picked(&[(1, MAIN, "")]));
    // Subtests: literal names, map-keyed tables; a one-line test has none.
    let go = [
        "package x",
        "func TestOne(t *testing.T) {}",
        "func TestMap(t *testing.T) {",
        "\tcases := map[string]struct{ in int }{",
        "\t\t\"small n\": {in: 1},",
        "\t}",
        "\tfor name, tc := range cases {",
        "\t\tt.Run(name, func(t *testing.T) { _ = tc })",
        "\t}",
        "\tt.Run(\"a\\\"b\", nil)",
        "}",
    ]
    .join("\n");
    let expected =
        [(1, TEST, "TestOne"), (2, TEST, "TestMap"), (4, TEST, "TestMap/small_n"), (9, TEST, "TestMap/a\"b")];
    assert_eq!(pick(&detect(&go, "x_test.go")), picked(&expected));
    let kotlin = ["class CalcTest {", "  @Test", "  fun `adds numbers`() {}", "  @Test fun subs() {}", "}"].join("\n");
    let expected = [(0, SUITE, "CalcTest"), (2, TEST, "CalcTest/adds numbers"), (3, TEST, "CalcTest/subs")];
    assert_eq!(pick(&detect(&kotlin, "src/test/kotlin/CalcTest.kt")), picked(&expected));
}

fn project_configs() -> [RunConfig; 5] {
    [
        cfg("cargo:test", "cargo", "cargo", &["test"]),
        cfg("npm:test", "npm", "pnpm", &["run", "test"]),
        cfg("pytest", "pytest", "pytest", &[]),
        cfg("go:test", "go", "go", &["test", "./..."]),
        cfg("gradle:test", "gradle", "./gradlew", &["test"]),
    ]
}

/// The configuration of the first test of `source`.
fn first(source: &str, rel_path: &str) -> RunConfig {
    let targets = detect(source, rel_path);
    let test = targets.iter().find(|target| target.kind != SUITE).expect("a test");
    narrowed(test, rel_path, &project_configs(), None).expect("a configuration")
}

#[test]
fn derives_per_framework_filters() {
    let unit = first("mod tests {\n#[test]\nfn adds() {}\n}", "src/lib.rs");
    assert_eq!(unit.args, ["test", "--", "tests::adds", "--exact"]);
    let integration = first("#[test]\nfn it_works() {}", "tests/api.rs");
    assert_eq!(integration.args, ["test", "--test", "api", "--", "it_works", "--exact"]);
    let js = first("describe('m', () => {\n  it('adds', () => {});\n});", "src/a.test.ts");
    assert_eq!(js.args, ["run", "test", "src/a.test.ts", "-t", "^m adds$"]);
    let pytest = first("class TestC:\n    def test_a(self): pass", "tests/test_c.py");
    assert_eq!(pytest.args, ["tests/test_c.py::TestC::test_a"]);
    // pytest also collects TestCase subclasses.
    let unittest = "class QTests(TestCase):\n    def test_a(self): pass";
    assert_eq!(first(unittest, "app/test_q.py").args, ["app/test_q.py::QTests::test_a"]);
    let django = [cfg("django:test", "django-test", "python3", &["manage.py", "test"])];
    let method = detect_run_targets(unittest, "polls/tests.py", true).remove(1);
    let labelled = narrowed(&method, "polls/tests.py", &django, None);
    assert_eq!(args_of(labelled), ["manage.py", "test", "polls.tests.QTests.test_a"]);
    // A module-level function or a plain class: unittest's loader would call it, so no Django run.
    let function = detect_run_targets("def test_top():\n    pass", "polls/tests.py", true).remove(0);
    assert_eq!(narrowed(&function, "polls/tests.py", &django, None), None);
    // Relative to the working directory, where manage.py is.
    let [django] = django;
    let nested = [RunConfig { cwd: to_cwd(Some("backend")), ..django }];
    let labelled = narrowed(&method, "backend/polls/tests/test_q.py", &nested, None);
    assert_eq!(args_of(labelled), ["manage.py", "test", "polls.tests.test_q.QTests.test_a"]);
    let go = first("func TestAdd(t *testing.T) {}", "pkg/add_test.go");
    assert_eq!(go.args, ["test", "./pkg", "-run", "^TestAdd$"]);
    let java = first("class FooTest {\n  @Test\n  void bar() {}\n}", "src/test/java/FooTest.java");
    assert_eq!(java.args, ["test", "--tests", "FooTest.bar"]);
}

#[test]
fn names_configs_after_the_target_and_keeps_ids_stable() {
    let config = first("mod tests {\n#[test]\nfn adds() {}\n}", "src/lib.rs");
    assert_eq!(config.name, "adds");
    assert_eq!(config.id, "context:target:cargo:test:src/lib.rs:tests/adds");
}

#[test]
fn detects_benchmark_fuzz_and_example_functions_and_runs_a_benchmark_without_tests() {
    let text = concat!(
        "package main\nfunc main() {}\nfunc BenchmarkWork(b *testing.B) {}\nfunc FuzzInput(f *testing.F) {}\n",
        "func Example() {}\n// func TestComment(t *testing.T) {}",
    );
    let targets = detect(text, "work_test.go");
    let names: Vec<&str> = targets.iter().map(|target| target.name.as_str()).collect();
    assert_eq!(names, ["main", "BenchmarkWork", "FuzzInput", "Example"]);
    let base = cfg("go:test", "go", "go", &["test", "-tags", "demo", "./...", "-args", "-test.short"]);
    let config = narrowed(&targets[1], "work_test.go", &[base], Some(text));
    let expected = ["test", "-tags", "demo", ".", "-run", "^$", "-bench", "^BenchmarkWork$", "-args", "-test.short"];
    assert_eq!(args_of(config), expected);
}

#[test]
fn uses_the_module_and_overrides_a_saved_filter_before_binary_arguments() {
    let program = ["-args", "-test.run=Old", "-test.bench", "OldBench", "-test.short"];
    let saved = [&["test", "-tags", "demo", "-run", "Old", "."][..], &program].concat();
    let mut base = RunConfig { cwd: to_cwd(Some("runtime")), ..cfg("go:typed", "go", "go", &saved) };
    let form = [
        ("goKind", "test"),
        ("module", "app"),
        ("scope", "package"),
        ("target", "."),
        ("pattern", "Old"),
        ("programArgs", "-test.run=Old -test.bench OldBench -test.short"),
    ];
    base.extras.params = form.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect();
    let text = "package calc\nfunc TestNew(t *testing.T) {\n t.Run(\"child case\", func(t *testing.T) {})\n}";
    let targets = detect(text, "app/calc_test.go");
    let configs = [base.clone()];
    let config = target_config(&targets[1], "app/calc_test.go", &configs, Some(text), Some(&base), RunEnv::default());
    let config = config.expect("a configuration");
    assert_eq!(config.args, ["test", "-tags", "demo", ".", "-run", "^TestNew$/^child_case$", "-args", "-test.short"]);
    let stored = ["module", "scope", "target", "pattern", "programArgs"].map(|key| param(&config, key));
    let expected = [Some("app"), Some("directory"), Some("."), Some("^TestNew$/^child_case$"), Some("-test.short")];
    assert_eq!(stored, expected);
}

fn phpunit_source() -> String {
    [
        "<?php",
        "namespace App\\Tests;",
        "use PHPUnit\\Framework\\TestCase;",
        "final class CalcTest extends TestCase",
        "{",
        "    public function testAdd(): void {}",
        "    #[Test]",
        "    public function subtracts(): void {}",
        "    /** @test */",
        "    public function multiplies(): void {}",
        "    public function helper(): void {}",
        "    private function testHidden(): void {}",
        "}",
        "class Helper {}",
        "    public function testNotInTestClass() {}",
    ]
    .join("\n")
}

#[test]
fn finds_test_classes_and_test_methods() {
    let expected = [
        (3, SUITE, "CalcTest"),
        (5, TEST, "CalcTest/testAdd"),
        (7, TEST, "CalcTest/subtracts"),
        (9, TEST, "CalcTest/multiplies"),
    ];
    assert_eq!(pick(&detect(&phpunit_source(), "tests/CalcTest.php")), picked(&expected));
}

#[test]
fn ignores_files_that_are_not_phpunit_tests() {
    assert!(detect(&phpunit_source(), "src/Calc.php").is_empty());
}

#[test]
fn runs_a_method_with_filter_ahead_of_the_file() {
    let (source, phpunit) = (phpunit_source(), cfg("phpunit", "phpunit", "vendor/bin/phpunit", &[]));
    let target = detect(&source, "tests/CalcTest.php").remove(1);
    let run = narrowed(&target, "tests/CalcTest.php", &[phpunit], Some(&source)).expect("a configuration");
    assert_eq!(run.args, ["--filter", "/(CalcTest::testAdd( with data set .*)?)$/", "tests/CalcTest.php"]);
    assert_eq!(run.command, "vendor/bin/phpunit");
}

const PEST_FILE: &str = "tests/Unit/MathTest.php";

fn pest_source() -> String {
    [
        "<?php",
        "",
        "test('adds numbers', function () {",
        "    expect(1 + 1)->toBe(2);",
        "});",
        "",
        "it(\"handles \\\"quoted\\\" names\", fn () => true);",
        "",
        "describe('division', function () {",
        "    it('divides', function () {",
        "        expect(4 / 2)->toBe(2);",
        "    });",
        "",
        "    describe('by zero', function () {",
        "        test('throws', fn () => true);",
        "    });",
        "});",
        "",
        "it('doubles', function (int $n) {",
        "    expect($n * 2)->toBe($n + $n);",
        "})->with([1, 2, 3]);",
        "// it('is commented out')",
    ]
    .join("\n")
}

/// The Pest run of the target declared at `line`.
fn pest_run(line: u32) -> RunConfig {
    let (source, pest) = (pest_source(), cfg("phpunit", "phpunit", "vendor/bin/pest", &[]));
    let targets = detect(&source, PEST_FILE);
    let target = targets.iter().find(|target| target.line == line).expect("a target on the line");
    narrowed(target, PEST_FILE, &[pest], Some(&source)).expect("a configuration")
}

#[test]
fn finds_tests_and_describe_blocks_with_the_names_pest_gives_them() {
    let targets = detect(&pest_source(), PEST_FILE);
    let expected = [
        (2, TEST, "adds numbers"),
        (6, TEST, "it handles \"quoted\" names"),
        (8, SUITE, "division"),
        (9, TEST, "division/it divides"),
        (13, SUITE, "division/by zero"),
        (14, TEST, "division/by zero/throws"),
        (18, TEST, "it doubles"),
    ];
    assert_eq!(pick(&targets), picked(&expected));
    assert_eq!((targets[3].name.as_str(), targets[3].pest), ("division → it divides", true));
}

#[test]
fn ignores_files_that_are_not_pest_tests() {
    assert!(detect(&pest_source(), "src/Math.php").is_empty());
}

#[test]
fn runs_one_test_by_the_name_pest_matches_data_sets_included() {
    assert_eq!(pest_run(2).args, ["--filter", "/::adds numbers( with data set .*)?$/", PEST_FILE]);
    let nested = "/::`division` → `by zero` → throws( with data set .*)?$/";
    assert_eq!(pest_run(14).args, ["--filter", nested, PEST_FILE]);
    assert_eq!(pest_run(18).args[1], "/::it doubles( with data set .*)?$/");
    assert_eq!(pest_run(18).command, "vendor/bin/pest");
}

#[test]
fn runs_every_test_of_a_describe_block() {
    assert_eq!(pest_run(8).args[1], "/::`division` → /");
    assert_eq!(pest_run(13).args[1], "/::`division` → `by zero` → /");
}

#[test]
fn keeps_phpunit_classes_in_the_same_file() {
    let mixed = [
        "<?php",
        "final class CalcTest extends TestCase",
        "{",
        "    public function testAdd() {}",
        "}",
        "test('x', fn () => 1);",
    ]
    .join("\n");
    let expected = [(1, SUITE, "CalcTest"), (3, TEST, "CalcTest/testAdd"), (5, TEST, "x")];
    assert_eq!(pick(&detect(&mixed, "tests/CalcTest.php")), picked(&expected));
}

#[test]
fn finds_aliased_dot_imported_and_multiline_testing_declarations() {
    let text = [
        "package p",
        "import check \"testing\"",
        "func TestAlias(",
        "  subject *check.T,",
        ") {",
        "  subject.Run(\"outer\", func(child *check.T) {",
        "    child.Run(`nested/leaf case`, func(last *check.T) {})",
        "  })",
        "  subject.Run(\"sibling\", func(child *check.T) {})",
        "}",
        "func BenchmarkAlias(b *check.B) {}",
        "func FuzzAlias(f *check.F) {}",
        "func Example() {}",
        "func TestMain(m *check.M) {}",
        "func Testlower(t *check.T) {}",
        "func (s Suite) TestMethod(t *check.T) {}",
    ]
    .join("\n");
    let expected: [&[&str]; 7] = [
        &["TestAlias"],
        &["TestAlias", "outer"],
        &["TestAlias", "outer", "nested", "leaf_case"],
        &["TestAlias", "sibling"],
        &["BenchmarkAlias"],
        &["FuzzAlias"],
        &["Example"],
    ];
    assert_eq!(paths(&detect(&text, "alias_test.go")), expected.map(strings));
    let dot = detect("package p\nimport . \"testing\"\nfunc TestDot(t *T) {}", "dot_test.go");
    assert_eq!(paths(&dot), [strings(&["TestDot"])]);
}

#[test]
fn ignores_comments_strings_methods_and_unrelated_run_receivers() {
    let text = [
        "package p",
        "/* func TestComment(t *testing.T) {} */",
        "var fake = `",
        "func TestString(t *testing.T) {}",
        "//go:generate fake",
        "`",
        "func TestReal(t *testing.T) {",
        "  // t.Run(\"comment\", nil)",
        "  _ = `t.Run(\"string\", nil) }`",
        "  other.Run(\"unrelated\", nil)",
        "  t.Run(\"a\\x20b\", func(sub *testing.T) {",
        "    sub.Run(\"quote\\\"brace}\", nil)",
        "  })",
        "}",
        "//go:generate real",
    ]
    .join("\n");
    let targets = detect(&text, "real_test.go");
    let found: Vec<(u32, Vec<String>)> = targets.iter().map(|target| (target.line, target.path.clone())).collect();
    let expected = [
        (6, strings(&["TestReal"])),
        (10, strings(&["TestReal", "a_b"])),
        (11, strings(&["TestReal", "a_b", "quote\"brace}"])),
        (14, strings(&["14"])),
    ];
    assert_eq!(found, expected);
}

#[test]
fn keeps_nested_benchmark_paths_and_tolerates_incomplete_source() {
    let text = concat!(
        "package p\nfunc BenchmarkWork(b *testing.B) { b.Run(\"group\", func(inner *testing.B) ",
        "{ inner.Run(\"leaf\", nil) }) }",
    );
    let expected: [&[&str]; 3] = [&["BenchmarkWork"], &["BenchmarkWork", "group"], &["BenchmarkWork", "group", "leaf"]];
    assert_eq!(paths(&detect(text, "bench_test.go")), expected.map(strings));
    // An escape that names no character is left as written instead of failing.
    let broken = detect("func TestBroken(t *testing.T) { t.Run(\"\\UFFFFFFFF\", nil) }", "broken_test.go");
    assert_eq!(broken.first().map(|target| target.path.clone()), Some(strings(&["TestBroken"])));
}

// ---------------------------------------------------------------------------------------------------
// Ruby

const RUBY_FILE: &str = "test/unit/test_outer.rb";
const RUBY_SOURCE: &str = "require_relative \"../test_helper\"

module Outer
  class InnerTest < Minitest::Test
    def test_one = assert true

    def test_two?
      assert true
    end

    def helper = 1
  end

  class Helper
    def test_not_a_test_class = 1
  end
end

class TopTest < ActiveSupport::TestCase
  test \"does a thing\" do
    assert true
  end
end

module Empty
  class Plain
  end
end
";

fn ruby_target(name: &str) -> RunTarget {
    let targets = detect(RUBY_SOURCE, RUBY_FILE);
    targets.into_iter().find(|target| target.name == name).expect("a target of that name")
}

/// The arguments of the Ruby run of the target named `name`, narrowed from `base`.
fn ruby_run(name: &str, base: RunConfig) -> Vec<String> {
    args_of(narrowed(&ruby_target(name), RUBY_FILE, &[base], Some(RUBY_SOURCE)))
}

fn rake() -> RunConfig {
    cfg("rake:test", "rake", "bundle", &["exec", "rake", "test"])
}

fn minitest() -> RunConfig {
    cfg("minitest", "ruby", "ruby", &["-Ilib", "-e", "RUNNER", "test"])
}

#[test]
fn is_a_target_language() {
    assert_eq!(target_language("a.rb"), Some(Lang::Ruby));
}

#[test]
fn finds_test_classes_the_modules_holding_them_and_their_test_methods() {
    let targets = detect(RUBY_SOURCE, RUBY_FILE);
    let found: Vec<(TargetKind, &str, u32)> =
        targets.iter().map(|target| (target.kind, target.name.as_str(), target.line)).collect();
    let expected = [
        (SUITE, "Outer", 2),
        (SUITE, "Outer::InnerTest", 3),
        (TEST, "Outer::InnerTest#test_one", 4),
        (TEST, "Outer::InnerTest#test_two?", 6),
        (SUITE, "TopTest", 18),
        (TEST, "TopTest#test_does_a_thing", 19),
    ];
    assert_eq!(found, expected);
    assert!(ruby_target("Outer").ruby_module);
    assert_eq!(ruby_target("Outer::InnerTest").path, ["Outer::InnerTest"]);
    assert_eq!(ruby_target("Outer::InnerTest#test_one").path, ["Outer::InnerTest", "test_one"]);
}

#[test]
fn leaves_modules_without_test_classes_non_test_classes_and_non_test_files_alone() {
    let targets = detect(RUBY_SOURCE, RUBY_FILE);
    assert!(!targets.iter().any(|target| target.name == "Empty" || target.name.contains("Helper")));
    assert!(detect(RUBY_SOURCE, "lib/outer.rb").is_empty());
    assert!(detect(RUBY_SOURCE, "spec/outer_spec.rb").is_empty());
}

#[test]
fn one_test_by_its_exact_name_through_rake_and_through_the_minitest_runner() {
    let through_rake =
        ["exec", "rake", "test", "TEST=test/unit/test_outer.rb", "TESTOPTS=--name=Outer::InnerTest#test_one"];
    assert_eq!(ruby_run("Outer::InnerTest#test_one", rake()), through_rake);
    let through_runner = ["-Ilib", "-e", "RUNNER", "test/unit/test_outer.rb", "--name=Outer::InnerTest#test_one"];
    assert_eq!(ruby_run("Outer::InnerTest#test_one", minitest()), through_runner);
}

#[test]
fn a_class_runs_its_own_tests_a_module_the_tests_of_the_classes_inside_it() {
    let last = |name: &str, base: RunConfig| ruby_run(name, base).pop();
    assert_eq!(last("Outer::InnerTest", minitest()).as_deref(), Some("--name=/^Outer::InnerTest#/"));
    assert_eq!(last("Outer", minitest()).as_deref(), Some("--name=/^Outer::/"));
    assert_eq!(last("Outer", rake()).as_deref(), Some("TESTOPTS=--name=/^Outer::/"));
}
