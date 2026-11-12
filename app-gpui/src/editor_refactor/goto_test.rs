//! Navigate › Test / Test Subject (⇧⌘T): from a class to its tests and back, by the naming
//! conventions of [`super::test_naming`] and the project's source and test roots. One match
//! opens; several show a chooser; with none (or always, in the chooser) "Create New Test…"
//! creates one from the file templates.
//!
//! The caller finds the project files named as [`GotoTest::names`] says and hands them to
//! [`GotoTest::found`].

use super::test_naming::{
    is_test_name, new_test_name, rank_candidates, rust_test_module_offset, split_name, subject_names_for,
    test_names_for,
};
use super::text::basename;
use crate::platform::i18n::{I18n, Param};
use crate::platform::project_roots::{FolderRole, RootFolder, is_test_path, root_for};
use regex::Regex;
use std::sync::LazyLock;

static RUST_INTEGRATION_TEST: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/tests/[^/]+\.rs$").expect("the pattern is valid"));

fn relative<'a>(project_root: Option<&str>, path: &'a str) -> Option<&'a str> {
    path.strip_prefix(project_root?)?.strip_prefix('/')
}

fn dirname(path: &str) -> &str {
    path.rfind('/').map_or("", |slash| &path[..slash])
}

/// Whether `path` is on the test side: under a test root, or a test by its name.
pub fn is_test_side(path: &str, project_root: Option<&str>, roots: &[RootFolder]) -> bool {
    relative(project_root, path).is_some_and(|rel| is_test_path(roots, rel))
        || is_test_name(basename(path))
        || RUST_INTEGRATION_TEST.is_match(path)
}

/// The path below its innermost root (`com/x/Foo.java` for `src/main/java/com/x/Foo.java`).
fn below_root(project_root: Option<&str>, roots: &[RootFolder], path: &str) -> Option<String> {
    let rel = relative(project_root, path)?;
    match root_for(roots, rel) {
        Some(root) if !root.path.is_empty() => rel.get(root.path.len() + 1..).map(str::to_owned),
        _ => Some(rel.to_owned()),
    }
}

fn shared(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

/// Where Create New Test… puts the test: the mirrored package under the closest test root, else
/// next to the file.
pub fn test_directory_for(path: &str, project_root: Option<&str>, roots: &[RootFolder]) -> String {
    let dir = dirname(path);
    let Some(project) = project_root else { return dir.to_owned() };
    let own = relative(project_root, path).and_then(|rel| root_for(roots, rel));
    let tests: Vec<&RootFolder> = roots.iter().filter(|root| root.kind == FolderRole::Test).collect();
    let Some(own) = own.filter(|own| own.kind == FolderRole::Source) else { return dir.to_owned() };
    // The test root sharing the most leading segments with the source root (the first listed of
    // equals).
    let Some(best) = tests.iter().copied().fold(None::<&RootFolder>, |best, root| match best {
        Some(best) if shared(&own.path, &best.path) >= shared(&own.path, &root.path) => Some(best),
        _ => Some(root),
    }) else {
        return dir.to_owned();
    };
    let own_base = if own.path.is_empty() { project.to_owned() } else { format!("{project}/{}", own.path) };
    let rel_dir = if dir == own_base { "" } else { dir.get(own_base.len() + 1..).unwrap_or_default() };
    let base = if best.path.is_empty() { project.to_owned() } else { format!("{project}/{}", best.path) };
    if rel_dir.is_empty() { base } else { format!("{base}/{rel_dir}") }
}

/// Create New Test… in `directory` from the file templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateTest {
    pub directory: String,
    /// The new file's name without its extension.
    pub name: String,
    /// Without the dot.
    pub extension: String,
    /// The file name of the class tested.
    pub subject: String,
}

/// What a chooser row (or the only match) does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestRun {
    /// Open the file, at `offset` when given.
    Open {
        path: String,
        offset: Option<u32>,
    },
    CreateTest(CreateTest),
    /// Add a `#[cfg(test)]` module to the Rust file itself.
    CreateRustTestModule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestChoice {
    pub label: String,
    pub detail: Option<String>,
    pub emphasis: bool,
    pub run: TestRun,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GotoTestStep {
    Run(TestRun),
    /// Show this hint (English source text).
    Hint(&'static str),
    /// The chooser; its title is in the UI language.
    Choose {
        title: String,
        items: Vec<TestChoice>,
    },
}

/// One Go to Test / Test Subject from the file `path`.
#[derive(Debug, Clone)]
pub struct GotoTest {
    path: String,
    project_root: Option<String>,
    roots: Vec<RootFolder>,
    test_side: bool,
}

impl GotoTest {
    pub fn new(path: &str, project_root: Option<&str>, roots: Vec<RootFolder>) -> Self {
        let test_side = is_test_side(path, project_root, &roots);
        Self { path: path.to_owned(), project_root: project_root.map(str::to_owned), roots, test_side }
    }

    /// The file names to look for in the project.
    pub fn names(&self) -> Vec<String> {
        let name = basename(&self.path);
        if self.test_side { subject_names_for(name) } else { test_names_for(name) }
    }

    fn detail(&self, path: &str) -> String {
        let dir = dirname(path);
        relative(self.project_root.as_deref(), dir).unwrap_or(dir).to_owned()
    }

    fn open(&self, path: &str, offset: Option<u32>) -> TestChoice {
        TestChoice {
            label: basename(path).to_owned(),
            detail: Some(self.detail(path)),
            emphasis: false,
            run: TestRun::Open { path: path.to_owned(), offset },
        }
    }

    /// The project files named as [`GotoTest::names`] said (any order, duplicates allowed); `text`
    /// is the file's text (for its Rust test module).
    pub fn found(&self, files: Vec<String>, text: &str, i18n: &mut I18n) -> GotoTestStep {
        let root = self.project_root.as_deref();
        let mut unique: Vec<String> = Vec::new();
        for file in files {
            if file != self.path && !unique.contains(&file) {
                unique.push(file);
            }
        }
        let side: Vec<String> =
            unique.into_iter().filter(|file| is_test_side(file, root, &self.roots) != self.test_side).collect();
        let ranked = rank_candidates(&self.path, &side, |path| below_root(root, &self.roots, path));
        let name = basename(&self.path);
        let stem = split_name(name).0;
        if self.test_side {
            return match ranked.as_slice() {
                [] => GotoTestStep::Hint("No test subject found"),
                [one] => GotoTestStep::Run(TestRun::Open { path: one.clone(), offset: None }),
                subjects => GotoTestStep::Choose {
                    title: i18n.tx_built(
                        &["Choose Test Subject for ", " (", " found)"],
                        &[Param::from(stem), Param::from(subjects.len())],
                    ),
                    items: subjects.iter().map(|path| self.open(path, None)).collect(),
                },
            };
        }
        let mut items: Vec<TestChoice> = Vec::new();
        // Rust unit tests live in the file's own `#[cfg(test)]` module.
        if let Some(offset) = self.path.ends_with(".rs").then(|| rust_test_module_offset(text)).flatten() {
            items.push(TestChoice {
                label: "mod tests".to_owned(),
                detail: Some(format!("{name} (#[cfg(test)])")),
                emphasis: false,
                run: TestRun::Open { path: self.path.clone(), offset: Some(offset) },
            });
        }
        items.extend(ranked.iter().map(|path| self.open(path, None)));
        if let [only] = items.as_slice() {
            return GotoTestStep::Run(only.run.clone());
        }
        let create = if self.path.ends_with(".rs") {
            TestRun::CreateRustTestModule
        } else {
            let extension = split_name(name).1;
            TestRun::CreateTest(CreateTest {
                directory: test_directory_for(&self.path, root, &self.roots),
                name: split_name(&new_test_name(name)).0.to_owned(),
                extension: extension.strip_prefix('.').unwrap_or(extension).to_owned(),
                subject: name.to_owned(),
            })
        };
        let title =
            i18n.tx_built(&["Choose Test for ", " (", " found)"], &[Param::from(stem), Param::from(items.len())]);
        let create = TestChoice { label: "Create New Test…".to_owned(), detail: None, emphasis: true, run: create };
        items.insert(0, create);
        GotoTestStep::Choose { title, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(path: &str, kind: FolderRole) -> RootFolder {
        RootFolder { path: path.to_owned(), kind, detected_from: Some("pom.xml".to_owned()) }
    }

    fn roots() -> Vec<RootFolder> {
        vec![root("src/main/java", FolderRole::Source), root("src/test/java", FolderRole::Test)]
    }

    /// Go to Test from `path` among the project's `files`.
    fn goto(path: &str, text: &str, files: &[&str], roots: Vec<RootFolder>) -> GotoTestStep {
        let flow = GotoTest::new(path, Some("/p"), roots);
        let names = flow.names();
        let found = files
            .iter()
            .filter(|file| names.iter().any(|name| file.ends_with(&format!("/{name}"))))
            .map(|file| (*file).to_owned())
            .collect();
        flow.found(found, text, &mut I18n::default())
    }

    fn opens(path: &str) -> GotoTestStep {
        GotoTestStep::Run(TestRun::Open { path: path.to_owned(), offset: None })
    }

    fn chooser(step: &GotoTestStep) -> (&str, Vec<&str>) {
        let GotoTestStep::Choose { title, items } = step else { panic!("expected a chooser, got {step:?}") };
        (title, items.iter().map(|item| item.label.as_str()).collect())
    }

    #[test]
    fn opens_the_only_test_of_a_class() {
        let files = ["/p/src/main/java/a/Foo.java", "/p/src/test/java/a/FooTest.java"];
        assert_eq!(goto(files[0], "", &files, roots()), opens("/p/src/test/java/a/FooTest.java"));
    }

    #[test]
    fn goes_back_from_a_test_to_its_subject() {
        let files = ["/p/src/main/java/a/Foo.java", "/p/src/test/java/a/FooTest.java"];
        assert_eq!(goto(files[1], "", &files, roots()), opens("/p/src/main/java/a/Foo.java"));
    }

    #[test]
    fn shows_a_chooser_with_create_new_test_for_several_tests_mirrored_package_first() {
        let files =
            ["/p/src/test/java/b/FooTest.java", "/p/src/test/java/a/FooTests.java", "/p/src/main/java/a/Foo.java"];
        let step = goto(files[2], "", &files, roots());
        assert_eq!(
            chooser(&step),
            ("Choose Test for Foo (2 found)", vec!["Create New Test…", "FooTests.java", "FooTest.java"])
        );
    }

    #[test]
    fn offers_create_new_test_in_the_mirrored_test_package_when_there_is_none() {
        let files = ["/p/src/main/java/a/Foo.java"];
        let step = goto(files[0], "", &files, roots());
        let GotoTestStep::Choose { items, .. } = &step else { panic!("expected a chooser") };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].run,
            TestRun::CreateTest(CreateTest {
                directory: "/p/src/test/java/a".to_owned(),
                name: "FooTest".to_owned(),
                extension: "java".to_owned(),
                subject: "Foo.java".to_owned(),
            })
        );
    }

    #[test]
    fn does_not_take_a_non_test_file_of_a_test_like_name_as_a_test() {
        // FooTests.java is a test by name, wherever it is.
        let files = ["/p/src/main/java/a/Foo.java", "/p/src/main/java/a/FooTests.java"];
        let step = goto(files[0], "", &files, vec![root("src/main/java", FolderRole::Source)]);
        assert_eq!(step, opens("/p/src/main/java/a/FooTests.java"));
    }

    #[test]
    fn go_python_ts_conventions_next_to_the_file() {
        let files = ["/p/pkg/foo.go", "/p/pkg/foo_test.go", "/p/m/test_bar.py", "/p/web/x.spec.ts"];
        assert_eq!(goto("/p/pkg/foo.go", "", &files, Vec::new()), opens("/p/pkg/foo_test.go"));
        assert_eq!(goto("/p/m/bar.py", "", &files, Vec::new()), opens("/p/m/test_bar.py"));
        assert_eq!(goto("/p/web/x.ts", "", &files, Vec::new()), opens("/p/web/x.spec.ts"));
    }

    #[test]
    fn rust_the_in_file_test_module_and_integration_tests() {
        let text = "pub fn lex() {}\n#[cfg(test)]\nmod tests {}\n";
        let files = ["/p/src/lexer.rs", "/p/tests/lexer.rs"];
        let step = goto(files[0], text, &files, Vec::new());
        assert_eq!(chooser(&step).1, ["Create New Test…", "mod tests", "lexer.rs"]);
        let GotoTestStep::Choose { items, .. } = &step else { panic!("expected a chooser") };
        assert_eq!(items[1].run, TestRun::Open { path: files[0].to_owned(), offset: Some(16) });
        assert_eq!(items[0].run, TestRun::CreateRustTestModule);
        assert_eq!(goto(files[1], "", &files, Vec::new()), opens("/p/src/lexer.rs"));
    }

    #[test]
    fn hints_when_a_test_has_no_subject() {
        let files = ["/p/src/test/java/a/FooTest.java"];
        assert_eq!(goto(files[0], "", &files, roots()), GotoTestStep::Hint("No test subject found"));
    }

    #[test]
    fn puts_new_tests_next_to_the_file_without_source_test_roots() {
        assert_eq!(test_directory_for("/p/pkg/foo.go", Some("/p"), &[]), "/p/pkg");
        assert_eq!(test_directory_for("/p/src/main/java/Foo.java", Some("/p"), &roots()), "/p/src/test/java");
    }
}
