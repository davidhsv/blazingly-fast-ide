//! Language levels read from build files, for templates that depend on them: Java `var` (10+)
//! from Maven or Gradle, C `auto` (C23) from CMake or compiler flags. Unknown means "not known to
//! be supported".
//!
//! The build files are read through a callback (`None`: the file does not exist or cannot be
//! read), so the caller decides where the reads run; [`LevelCache`] keeps answers for a while.

use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

static JAVA_VERSION: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\s*(?:1\.)?([0-9]+)"));

/// `1.8` is 8, `17` is 17.
pub fn parse_java_version(version: Option<&str>) -> Option<u32> {
    JAVA_VERSION.captures(version?).and_then(|captures| captures[1].parse().ok())
}

static XML_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"<!--(?s:.)*?-->"));
static PROPERTIES: LazyLock<Regex> = LazyLock::new(|| pattern(r"<properties>((?s:.)*?)</properties>"));
static PROPERTY: LazyLock<Regex> = LazyLock::new(|| pattern(r"<([A-Za-z0-9_.-]+)>\s*([^<]*?)\s*</([A-Za-z0-9_.-]+)>"));
static PROPERTY_REFERENCE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^\$\{([A-Za-z0-9_.-]+)\}$"));
static COMPILER_PLUGIN: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"<artifactId>\s*maven-compiler-plugin\s*</artifactId>((?s:.)*?)</plugin>"));

fn pom_properties(pom: &str) -> HashMap<String, String> {
    let section = PROPERTIES.captures(pom).map_or("", |captures| captures.get(1).map_or("", |m| m.as_str()));
    PROPERTY
        .captures_iter(section)
        .filter(|captures| captures[1] == captures[3])
        .map(|captures| (captures[1].to_owned(), captures[2].to_owned()))
        .collect()
}

fn resolve(value: Option<String>, properties: &HashMap<String, String>, depth: u32) -> Option<String> {
    let value = value?;
    if depth > 5 {
        return Some(value);
    }
    match PROPERTY_REFERENCE.captures(value.trim()) {
        Some(captures) => resolve(properties.get(&captures[1]).cloned(), properties, depth + 1),
        None => Some(value),
    }
}

/// The Java language level a pom.xml declares, if any.
pub fn java_level_from_pom(pom: &str) -> Option<u32> {
    let clean = XML_COMMENT.replace_all(pom, "");
    let properties = pom_properties(&clean);
    let plugin = COMPILER_PLUGIN.captures(&clean).and_then(|captures| captures.get(1)).map_or("", |m| m.as_str());
    let tag = |name: &str| {
        let tag = pattern(&format!(r"<{name}>\s*([^<]+?)\s*</{name}>", name = regex::escape(name)));
        tag.captures(plugin).map(|captures| captures[1].to_owned())
    };
    let property = |name: &str| properties.get(name).cloned();
    let candidates = [
        tag("release"),
        property("maven.compiler.release"),
        tag("source"),
        property("maven.compiler.source"),
        property("java.version"),
    ];
    candidates.into_iter().find_map(|candidate| parse_java_version(resolve(candidate, &properties, 0).as_deref()))
}

static BLOCK_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"/\*(?s:.)*?\*/"));
static LINE_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?m)//.*$"));
static GRADLE_LEVELS: LazyLock<[Regex; 5]> = LazyLock::new(|| {
    [
        pattern(r"options\.release(?:\.set\s*\(|\s*=)\s*([0-9]+)"),
        pattern(r#"JavaLanguageVersion\.of\s*\(\s*["']?([0-9]+)"#),
        pattern(r"jvmToolchain\s*\(\s*([0-9]+)"),
        pattern(
            r#"sourceCompatibility\s*(?:=|\s)\s*(?:JavaVersion\.VERSION_(?:1_)?([0-9]+)|["']?((?:1\.)?[0-9]+)["']?)"#,
        ),
        pattern(r"JavaVersion\.VERSION_(?:1_)?([0-9]+)"),
    ]
});

/// The Java language level a build.gradle(.kts) declares, if any.
pub fn java_level_from_gradle(gradle: &str) -> Option<u32> {
    let clean = BLOCK_COMMENT.replace_all(gradle, "");
    let clean = LINE_COMMENT.replace_all(&clean, "");
    GRADLE_LEVELS.iter().find_map(|level| {
        level.captures(&clean).map(|captures| {
            let version = captures.get(1).or_else(|| captures.get(2)).map(|m| m.as_str());
            parse_java_version(version)
        })
    })?
}

static CMAKE_STANDARD: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)C_STANDARD\s+(23|2x)\b"));
static STD_FLAG: LazyLock<Regex> = LazyLock::new(|| pattern(r"-std=(c|gnu)(23|2x)\b"));

/// Whether CMake or the compiler flags in `file` ask for C23 (`auto` type inference).
pub fn c_standard_is_23(file: &str, text: &str) -> bool {
    (file.ends_with("CMakeLists.txt") && CMAKE_STANDARD.is_match(text)) || STD_FLAG.is_match(text)
}

fn parent_dir(path: &str) -> Option<&str> {
    let trimmed = path.trim_end_matches('/');
    trimmed.rfind('/').filter(|i| *i > 0).map(|i| &trimmed[..i])
}

/// The directories from `file`'s up to `root` (inclusive), nearest first.
pub fn ancestor_dirs(file: &str, root: Option<&str>) -> Vec<String> {
    let stop = root.map(|root| root.trim_end_matches('/'));
    let mut out = Vec::new();
    let mut dir = parent_dir(file);
    while let Some(current) = dir {
        out.push(current.to_owned());
        match stop {
            Some(stop) if current != stop && current.starts_with(stop) => dir = parent_dir(current),
            _ => break,
        }
    }
    out
}

/// Reads the Java level a build file declares.
type LevelParser = fn(&str) -> Option<u32>;

/// The nearest Java level declared for `file`, walking up to `root`; `read` returns a build
/// file's text.
pub fn java_language_level(file: &str, root: Option<&str>, read: impl Fn(&str) -> Option<String>) -> Option<u32> {
    let parsers: [(&str, LevelParser); 3] = [
        ("pom.xml", java_level_from_pom),
        ("build.gradle", java_level_from_gradle),
        ("build.gradle.kts", java_level_from_gradle),
    ];
    ancestor_dirs(file, root).iter().find_map(|dir| {
        parsers.iter().find_map(|(name, parse)| read(&format!("{dir}/{name}")).and_then(|text| parse(&text)))
    })
}

/// Whether the C file is built as C23, per the nearest build configuration found.
pub fn c_is_c23(file: &str, root: Option<&str>, read: impl Fn(&str) -> Option<String>) -> bool {
    const FILES: [&str; 5] = ["CMakeLists.txt", "compile_flags.txt", "compile_commands.json", ".clangd", "Makefile"];
    ancestor_dirs(file, root).iter().any(|dir| {
        FILES.iter().any(|name| read(&format!("{dir}/{name}")).is_some_and(|text| c_standard_is_23(name, &text)))
    })
}

/// How long a level stays known.
pub const CACHE_MS: u64 = 30_000;

/// Language levels per file, kept for [`CACHE_MS`] so that a run of refactorings reads the build
/// files once.
#[derive(Debug, Clone, Default)]
pub struct LevelCache {
    java: HashMap<String, (u64, Option<u32>)>,
    c23: HashMap<String, (u64, bool)>,
}

impl LevelCache {
    /// [`java_language_level`], from the cache while it is fresh at `now_ms`.
    pub fn java(
        &mut self,
        file: &str,
        root: Option<&str>,
        now_ms: u64,
        read: impl Fn(&str) -> Option<String>,
    ) -> Option<u32> {
        if let Some((at, level)) = self.java.get(file)
            && now_ms.saturating_sub(*at) < CACHE_MS
        {
            return *level;
        }
        let level = java_language_level(file, root, read);
        self.java.insert(file.to_owned(), (now_ms, level));
        level
    }

    /// [`c_is_c23`], from the cache while it is fresh at `now_ms`.
    pub fn c23(&mut self, file: &str, root: Option<&str>, now_ms: u64, read: impl Fn(&str) -> Option<String>) -> bool {
        if let Some((at, c23)) = self.c23.get(file)
            && now_ms.saturating_sub(*at) < CACHE_MS
        {
            return *c23;
        }
        let c23 = c_is_c23(file, root, read);
        self.c23.insert(file.to_owned(), (now_ms, c23));
        c23
    }

    pub fn clear(&mut self) {
        self.java.clear();
        self.c23.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_maven_poms() {
        let release = "<properties><maven.compiler.release>17</maven.compiler.release></properties>";
        assert_eq!(java_level_from_pom(release), Some(17));
        let source = "<properties><maven.compiler.source>1.8</maven.compiler.source></properties>";
        assert_eq!(java_level_from_pom(source), Some(8));
        assert_eq!(java_level_from_pom("<properties><java.version>21</java.version></properties>"), Some(21));
        assert_eq!(
            java_level_from_pom(
                "<properties><jdk>11</jdk></properties><plugin><artifactId>maven-compiler-plugin</artifactId>\
                 <configuration><release>${jdk}</release></configuration></plugin>"
            ),
            Some(11)
        );
        assert_eq!(java_level_from_pom("<!-- <java.version>17</java.version> --><project/>"), None);
    }

    #[test]
    fn reads_gradle_builds() {
        assert_eq!(java_level_from_gradle("java { sourceCompatibility = JavaVersion.VERSION_1_8 }"), Some(8));
        assert_eq!(java_level_from_gradle("sourceCompatibility = '11'"), Some(11));
        assert_eq!(
            java_level_from_gradle("java { toolchain { languageVersion = JavaLanguageVersion.of(21) } }"),
            Some(21)
        );
        assert_eq!(java_level_from_gradle("kotlin { jvmToolchain(17) }"), Some(17));
        assert_eq!(java_level_from_gradle("// sourceCompatibility = 17\nplugins {}"), None);
    }

    #[test]
    fn finds_the_nearest_build_file_below_the_project_root() {
        assert_eq!(ancestor_dirs("/p/a/src/A.java", Some("/p")), ["/p/a/src", "/p/a", "/p"]);
        let files: HashMap<&str, &str> = HashMap::from([
            ("/p/a/build.gradle", "plugins { id 'java' }"),
            ("/p/pom.xml", "<properties><maven.compiler.release>17</maven.compiler.release></properties>"),
        ]);
        let read = |path: &str| files.get(path).map(|text| (*text).to_owned());
        assert_eq!(java_language_level("/p/a/src/Main.java", Some("/p"), read), Some(17));
        assert_eq!(java_language_level("/q/Other.java", Some("/q"), read), None);
    }

    #[test]
    fn detects_c23() {
        assert!(c_standard_is_23("CMakeLists.txt", "set(CMAKE_C_STANDARD 23)"));
        assert!(c_standard_is_23("compile_flags.txt", "-std=c2x\n-Wall"));
        assert!(!c_standard_is_23("Makefile", "CFLAGS=-std=c11"));
    }
}
