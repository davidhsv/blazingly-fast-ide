//! Go to Test / Go to Test Subject (⇧⌘T) naming conventions per language: JVM and .NET
//! `FooTest`, `FooTests`, `TestFoo`; Go `foo_test.go`; Python `test_foo.py`, `foo_test.py`;
//! JS/TS `foo.test.ts`, `foo.spec.ts`; Ruby `foo_spec.rb`, `foo_test.rb`; C/C++ `foo_test.cc`,
//! `test_foo.c`; Rust integration tests `tests/foo.rs` (unit tests live in the file's
//! `#[cfg(test)]` module).

use crate::platform::collate::compare_code_units;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

static JS_EXT: LazyLock<Regex> = LazyLock::new(|| pattern(r"\.(?:[cm]?[jt]sx?)$"));
static JVM_EXT: LazyLock<Regex> = LazyLock::new(|| pattern(r"\.(java|kt|kts|scala|groovy|cs|fs|vb|swift|dart|php)$"));
static C_EXT: LazyLock<Regex> = LazyLock::new(|| pattern(r"\.(c|cc|cpp|cxx|h|hpp|m|mm)$"));
static TEST_PREFIX: LazyLock<Regex> = LazyLock::new(|| pattern(r"^Test[A-Z0-9_]"));
static RUST_TEST_MODULE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z0-9_]+"));

/// A file name split at its last dot: `Foo` and `.java`, `foo.test` and `.ts`.
pub fn split_name(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot..]),
        _ => (name, ""),
    }
}

fn is_test_kind(ext: &str) -> bool {
    ext == ".test" || ext == ".spec"
}

/// Whether `name` looks like a test by its name alone (the test roots are checked separately).
pub fn is_test_name(name: &str) -> bool {
    let (stem, ext) = split_name(name);
    if is_test_kind(split_name(stem).1) && JS_EXT.is_match(ext) {
        return true;
    }
    match ext {
        ".go" => stem.ends_with("_test"),
        ".py" => stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest",
        ".rb" => stem.ends_with("_spec") || stem.ends_with("_test"),
        _ if JVM_EXT.is_match(name) => {
            TEST_PREFIX.is_match(stem)
                || ((stem.ends_with("Test") || stem.ends_with("Tests")) && stem != "Test" && stem != "Tests")
        }
        _ if C_EXT.is_match(name) => {
            stem.starts_with("test_") || stem.ends_with("_test") || stem.ends_with("_unittest")
        }
        _ => false,
    }
}

/// The file names a test of `name` may have, the most conventional first.
pub fn test_names_for(name: &str) -> Vec<String> {
    let (stem, ext) = split_name(name);
    if JS_EXT.is_match(name) {
        return vec![format!("{stem}.test{ext}"), format!("{stem}.spec{ext}")];
    }
    match ext {
        ".go" => vec![format!("{stem}_test.go")],
        ".py" => vec![format!("test_{stem}.py"), format!("{stem}_test.py")],
        ".rb" => vec![format!("{stem}_spec.rb"), format!("{stem}_test.rb")],
        ".rs" => vec![format!("{stem}.rs"), format!("{stem}_test.rs"), format!("{stem}_tests.rs")],
        _ if JVM_EXT.is_match(name) => vec![
            format!("{stem}Test{ext}"),
            format!("{stem}Tests{ext}"),
            format!("Test{stem}{ext}"),
            format!("{stem}IT{ext}"),
            format!("{stem}Spec{ext}"),
        ],
        _ if C_EXT.is_match(name) => {
            vec![format!("{stem}_test{ext}"), format!("test_{stem}{ext}"), format!("{stem}_unittest{ext}")]
        }
        _ => vec![format!("{stem}_test{ext}"), format!("test_{stem}{ext}")],
    }
}

fn push_unique(out: &mut Vec<String>, name: String) {
    if !out.contains(&name) {
        out.push(name);
    }
}

/// The file names the subject of the test `name` may have.
pub fn subject_names_for(name: &str) -> Vec<String> {
    let (stem, ext) = split_name(name);
    let (base, inner_ext) = split_name(stem);
    let mut out = Vec::new();
    if JS_EXT.is_match(ext) && is_test_kind(inner_ext) {
        // `foo.test.ts` may test `foo.tsx` or `foo.js` too.
        push_unique(&mut out, format!("{base}{ext}"));
        for other in [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"] {
            push_unique(&mut out, format!("{base}{other}"));
        }
        return out;
    }
    if matches!(ext, ".go" | ".py" | ".rb" | ".rs") || C_EXT.is_match(name) {
        for suffix in ["_test", "_tests", "_spec", "_unittest"] {
            if let Some(subject) = stem.strip_suffix(suffix).filter(|subject| !subject.is_empty()) {
                push_unique(&mut out, format!("{subject}{ext}"));
            }
        }
        if let Some(subject) = stem.strip_prefix("test_").filter(|subject| !subject.is_empty()) {
            push_unique(&mut out, format!("{subject}{ext}"));
        }
        // Rust integration tests (`tests/foo.rs`) are named like their subject.
        if ext == ".rs" {
            push_unique(&mut out, format!("{stem}{ext}"));
        }
        return out;
    }
    for suffix in ["Tests", "Test", "IT", "Spec"] {
        if let Some(subject) = stem.strip_suffix(suffix).filter(|subject| !subject.is_empty()) {
            push_unique(&mut out, format!("{subject}{ext}"));
        }
    }
    if TEST_PREFIX.is_match(stem) {
        push_unique(&mut out, format!("{}{ext}", &stem[4..]));
    }
    out
}

/// The number of leading path segments two `/`-separated paths share.
pub fn shared_segments(a: &str, b: &str) -> usize {
    a.split('/').zip(b.split('/')).take_while(|(x, y)| x == y).count()
}

fn dir_of(rel: &str) -> &str {
    rel.rfind('/').map_or("", |slash| &rel[..slash])
}

/// Orders candidate paths for `from`: the mirrored package (the same folder below its root)
/// first, then the closest in the tree. `below_root` is a path below its innermost root.
pub fn rank_candidates(from: &str, candidates: &[String], below_root: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let own_dir = below_root(from).map(|own| dir_of(&own).to_owned());
    let score = |path: &str| {
        let dir = below_root(path).map(|rel| dir_of(&rel).to_owned());
        let mirrored = own_dir.is_some() && dir == own_dir;
        (if mirrored { 1000 } else { 0 }) + shared_segments(from, path)
    };
    let mut ranked: Vec<(usize, &String)> = candidates.iter().map(|path| (score(path), path)).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| compare_code_units(a.1, b.1)));
    ranked.into_iter().map(|(_, path)| path.clone()).collect()
}

/// The UTF-16 offset of a Rust file's `#[cfg(test)]` module, if it has one.
pub fn rust_test_module_offset(text: &str) -> Option<u32> {
    RUST_TEST_MODULE.find(text).map(|found| text[..found.start()].encode_utf16().count() as u32)
}

/// The name a new test of `name` gets (without directory), by convention.
pub fn new_test_name(name: &str) -> String {
    test_names_for(name).into_iter().next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s_to_tests() {
        let cases: [(&str, &[&str]); 7] = [
            ("Foo.java", &["FooTest.java", "FooTests.java", "TestFoo.java", "FooIT.java", "FooSpec.java"]),
            ("foo.go", &["foo_test.go"]),
            ("foo.py", &["test_foo.py", "foo_test.py"]),
            ("foo.ts", &["foo.test.ts", "foo.spec.ts"]),
            ("Button.tsx", &["Button.test.tsx", "Button.spec.tsx"]),
            ("lexer.rs", &["lexer.rs", "lexer_test.rs", "lexer_tests.rs"]),
            ("user.rb", &["user_spec.rb", "user_test.rb"]),
        ];
        for (name, tests) in cases {
            assert_eq!(test_names_for(name), tests, "{name}");
        }
    }

    #[test]
    fn s_to_subjects() {
        let cases: [(&str, &[&str]); 7] = [
            ("FooTest.java", &["Foo.java"]),
            ("FooTests.kt", &["Foo.kt"]),
            ("TestFoo.java", &["Foo.java"]),
            ("foo_test.go", &["foo.go"]),
            ("test_foo.py", &["foo.py"]),
            ("foo_test.py", &["foo.py"]),
            ("lexer.rs", &["lexer.rs"]),
        ];
        for (name, subjects) in cases {
            assert_eq!(subject_names_for(name), subjects, "{name}");
        }
    }

    #[test]
    fn js_ts_tests_may_test_any_script_extension() {
        assert_eq!(subject_names_for("foo.spec.ts")[..3], ["foo.ts", "foo.tsx", "foo.js"]);
    }

    #[test]
    fn recognises_test_file_names() {
        for name in [
            "FooTest.java",
            "TestFoo.java",
            "FooTests.cs",
            "foo_test.go",
            "test_foo.py",
            "foo.test.tsx",
            "foo.spec.js",
            "foo_spec.rb",
            "bar_test.cc",
        ] {
            assert!(is_test_name(name), "{name}");
        }
        for name in ["Foo.java", "Test.java", "Testing.java", "foo.go", "foo.py", "foo.ts", "contest.py", "latest.ts"] {
            assert!(!is_test_name(name), "{name}");
        }
        assert_eq!(new_test_name("Foo.java"), "FooTest.java");
    }

    #[test]
    fn ranks_the_mirrored_package_first_then_the_closest_file() {
        let below = |path: &str| {
            let prefix = Regex::new(r"^/p/(?:a|b)/src/(?:main|test)/java/").expect("the pattern is valid");
            Some(prefix.replace(path, "").into_owned())
        };
        let candidates = [
            "/p/b/src/test/java/com/y/FooTest.java".to_owned(),
            "/p/a/src/test/java/com/z/FooTest.java".to_owned(),
            "/p/a/src/test/java/com/x/FooTest.java".to_owned(),
        ];
        assert_eq!(
            rank_candidates("/p/a/src/main/java/com/x/Foo.java", &candidates, below),
            [
                "/p/a/src/test/java/com/x/FooTest.java",
                "/p/a/src/test/java/com/z/FooTest.java",
                "/p/b/src/test/java/com/y/FooTest.java",
            ]
        );
    }

    #[test]
    fn finds_a_rust_cfg_test_module() {
        let text = "fn a() {}\n\n#[cfg(test)]\nmod tests {\n}\n";
        assert_eq!(rust_test_module_offset(text), Some(11));
        assert_eq!(rust_test_module_offset("fn a() {}"), None);
    }
}
