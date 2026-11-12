//! Where a Minitest test is defined. Minitest's output names `Class#test_name` and no file for tests
//! that pass, so the tree finds the source by searching the project: the class, then the `def` in its
//! file. Spec-style tests (`describe "Thing" do it "does x"`) are reported as `Thing#test_0001_does_x`.
//!
//! [`RubyTestFinder`] does no I/O: it hands out the searches to run and continues with their hits.

/// A regular-expression search over the project's Ruby files, or over `files` when given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubySearch {
    pub pattern: String,
    pub files: Option<Vec<String>>,
}

/// A file a search matched in, with the 0-based lines of its matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileHit {
    pub path: String,
    pub lines: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RubyTestSite {
    pub path: String,
    /// 1-based.
    pub line: u32,
}

/// `text` as a literal in a regular expression.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if ".*+?^${}()|[]\\".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The description of a spec-style test: `test_0001_does_x` is `does_x`.
fn spec_description(test: &str) -> Option<&str> {
    let numbered = test.strip_prefix("test_")?;
    let digits = numbered.len() - numbered.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    numbered[digits..].strip_prefix('_').filter(|description| digits > 0 && !description.is_empty())
}

/// The first match in `file` (in the first file with a match line when no file is named).
fn first(hits: &[FileHit], file: Option<&str>) -> Option<RubyTestSite> {
    let hit = match file {
        Some(file) => hits.iter().find(|hit| hit.path == file),
        None => hits.first(),
    }?;
    Some(RubyTestSite { path: hit.path.clone(), line: hit.lines.first()? + 1 })
}

/// Finds the source of the test `test` of `class_name`.
#[derive(Clone, Debug)]
pub struct RubyTestFinder {
    /// The pattern of the test's own declaration.
    body: String,
    owners: Vec<FileHit>,
}

impl RubyTestFinder {
    /// The first search is for the declaration of the class (or the `describe` block); answer with
    /// [`Self::owners_found`].
    pub fn start(class_name: &str, test: &str) -> (Self, RubySearch) {
        let spec = spec_description(test);
        // `Outer::InnerTest` is declared as `class InnerTest` inside `module Outer`, or as written.
        let last = class_name.rsplit("::").next().unwrap_or(class_name);
        let declared = match spec {
            Some(_) => format!(r#"^\s*(describe|context)\s*\(?\s*(["']){}\2"#, escape(class_name)),
            None => format!(r"^\s*(class|module)\s+([A-Z]\w*::)*{}\b", escape(last)),
        };
        let body = match spec {
            // Spec descriptions lose their spaces to underscores in the method name.
            Some(description) => format!(r#"^\s*it\s*\(?\s*(["']){}\1"#, escape(description).replace('_', "[ _]")),
            None => format!(r"^\s*def\s+{}\b", escape(test)),
        };
        (Self { body, owners: Vec::new() }, RubySearch { pattern: declared, files: None })
    }

    /// The files declaring the class were found: the next search is for the test in them (in the
    /// whole project when none declares it); answer with [`Self::test_found`].
    pub fn owners_found(&mut self, owners: Vec<FileHit>) -> RubySearch {
        let files: Vec<String> = owners.iter().map(|owner| owner.path.clone()).collect();
        self.owners = owners;
        RubySearch { pattern: self.body.clone(), files: (!files.is_empty()).then_some(files) }
    }

    /// The test's declaration; the class when the method is not found, and `None` when nothing is.
    pub fn test_found(&self, hits: &[FileHit]) -> Option<RubyTestSite> {
        // The same method name is common (`test_initialize`): prefer the file that declares the class.
        let mut declaring = self.owners.iter().map(|owner| owner.path.as_str());
        let owner = declaring.find(|file| hits.iter().any(|hit| hit.path == *file));
        first(hits, owner).or_else(|| first(hits, None)).or_else(|| first(&self.owners, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, lines: &[u32]) -> FileHit {
        FileHit { path: path.to_string(), lines: lines.to_vec() }
    }

    fn site(path: &str, line: u32) -> Option<RubyTestSite> {
        Some(RubyTestSite { path: path.to_string(), line })
    }

    /// Finds a test over fixed hits (the first matching pattern fragment wins). Returns the site and
    /// the searches made.
    fn find(hits: &[(&str, Vec<FileHit>)], class_name: &str, test: &str) -> (Option<RubyTestSite>, Vec<RubySearch>) {
        let answer = |search: &RubySearch| {
            let found = hits.iter().find(|(fragment, _)| search.pattern.contains(fragment));
            let found = found.map(|(_, found)| found.clone()).unwrap_or_default();
            match &search.files {
                Some(files) => found.into_iter().filter(|hit| files.contains(&hit.path)).collect(),
                None => found,
            }
        };
        let (mut finder, declared) = RubyTestFinder::start(class_name, test);
        let body = finder.owners_found(answer(&declared));
        let found = finder.test_found(&answer(&body));
        (found, vec![declared, body])
    }

    #[test]
    fn the_def_inside_the_file_that_declares_the_class() {
        let defs = vec![file("/p/test/a_test.rb", &[11]), file("/p/test/b_test.rb", &[4])];
        let both = vec![file("/p/test/a_test.rb", &[0]), file("/p/test/b_test.rb", &[0])];
        let search = [("(class|module)", both), ("def\\s+test_init", defs.clone())];
        // Only b declares TestB: its def wins even though a's file sorts first.
        let owners = [("(class|module)", vec![file("/p/test/b_test.rb", &[0])]), ("def\\s+test_init", defs)];
        assert_eq!(find(&owners, "TestB", "test_initialize").0, site("/p/test/b_test.rb", 5));
        assert_eq!(find(&search, "TestA", "test_initialize").0, site("/p/test/a_test.rb", 12));
    }

    #[test]
    fn looks_for_the_last_segment_of_a_namespaced_class() {
        let search = [
            ("InnerTest", vec![file("/p/test/n_test.rb", &[3])]),
            ("def\\s+test_nested", vec![file("/p/test/n_test.rb", &[4])]),
        ];
        let (found, searches) = find(&search, "Outer::InnerTest", "test_nested");
        assert_eq!(found, site("/p/test/n_test.rb", 5));
        assert!(!searches[0].pattern.contains("Outer"));
    }

    #[test]
    fn spec_style_tests_the_it_block_of_the_describe_underscores_for_spaces() {
        let search = [
            ("describe", vec![file("/p/spec/x_test.rb", &[0])]),
            ("does[ _]a[ _]thing", vec![file("/p/spec/x_test.rb", &[6])]),
        ];
        assert_eq!(find(&search, "Spec style", "test_0001_does_a_thing").0, site("/p/spec/x_test.rb", 7));
    }

    #[test]
    fn falls_back_to_the_class_when_the_method_is_not_found_and_to_nothing_when_nothing_is() {
        let only_class = [("(class|module)", vec![file("/p/test/a_test.rb", &[2])])];
        assert_eq!(find(&only_class, "TestA", "test_dynamic").0, site("/p/test/a_test.rb", 3));
        assert_eq!(find(&[], "TestA", "test_x").0, None);
    }
}
