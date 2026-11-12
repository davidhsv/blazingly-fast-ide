//! Path helpers of the Project view, the checks on names typed into its inline editor, and the
//! icon kind of a folder.
//!
//! Backend paths are absolute and `/`-separated on every OS, so everything here is plain string
//! logic on such paths.

pub use crate::platform::paths::is_same_or_inside;
use crate::platform::project_roots::{FolderRole, RootFolder, root_for, root_role};
use std::collections::BTreeMap;

/// Folder names that hold JVM sources: packages below them are dotted, and they are source roots
/// below `main` / `test`.
pub const JVM_SOURCE_DIRS: [&str; 4] = ["java", "kotlin", "scala", "groovy"];

/// `name` inside `dir`; `dir` itself for an empty name.
pub fn join_path(dir: &str, name: &str) -> String {
    if name.is_empty() {
        dir.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

/// The folder holding `path`; `/` for a top-level path and for one without a separator.
pub fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(slash) if slash > 0 => &path[..slash],
        _ => "/",
    }
}

/// The last segment of `path`.
pub fn name_of(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}

/// True when `path` lies below `ancestor` (and is not `ancestor` itself).
pub fn is_inside(path: &str, ancestor: &str) -> bool {
    path != ancestor && is_same_or_inside(path, ancestor)
}

/// Path relative to `root` (`""` for the root itself); `path` unchanged when outside.
pub fn relative_to<'a>(root: &str, path: &'a str) -> &'a str {
    if path == root {
        return "";
    }
    if !is_inside(path, root) {
        return path;
    }
    let start = root.len() + usize::from(!root.ends_with('/'));
    path.get(start..).unwrap_or_default()
}

/// Ancestor directories of `path` from `root` (inclusive) down to the parent of `path`; none when
/// `path` is not below `root`.
pub fn ancestors_within<'a>(root: &str, path: &'a str) -> Vec<&'a str> {
    let mut out = Vec::new();
    if !is_inside(path, root) {
        return out;
    }
    let mut dir = parent_of(path);
    while is_same_or_inside(dir, root) {
        out.push(dir);
        if dir == root || dir == "/" {
            break;
        }
        dir = parent_of(dir);
    }
    out.reverse();
    out
}

/// Replaces the home directory prefix with `~`, as IDEA shows paths.
pub fn tildify(path: &str, home: Option<&str>) -> String {
    let Some(home) = home.filter(|home| !home.is_empty() && *home != "/") else { return path.to_owned() };
    let home = home.strip_suffix('/').unwrap_or(home);
    match path.strip_prefix(home) {
        Some("") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_owned(),
    }
}

/// Splits `name.ext` into `("name", ".ext")`. Dotfiles have no extension: `.gitignore` is
/// `(".gitignore", "")`.
pub fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    }
}

/// Finder-style non-conflicting name: `Foo.kt`, then `Foo copy.kt`, then `Foo copy 2.kt`. A
/// folder keeps its dots in the stem.
pub fn unique_name(name: &str, taken: impl Fn(&str) -> bool, is_dir: bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, extension) = if is_dir { (name, "") } else { split_extension(name) };
    let mut n: u64 = 1;
    loop {
        let candidate = if n > 1 { format!("{stem} copy {n}{extension}") } else { format!("{stem} copy{extension}") };
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// The verdict on a name typed into the inline editor.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameCheck {
    /// Error message, or `None` when the name is acceptable.
    pub error: Option<String>,
    /// Non-fatal hint (leading or trailing spaces).
    pub warning: Option<String>,
}

/// What a typed name is checked against.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidateOptions<'a> {
    /// Names already present in the target directory.
    pub siblings: &'a [&'a str],
    /// Allow `a/b/c` (creates intermediate directories).
    pub allow_nested: bool,
    /// Current name when renaming (renaming to itself or a case variant is fine).
    pub current: Option<&'a str>,
    /// Compare names case-insensitively (macOS / Windows default file systems).
    pub case_insensitive: bool,
    /// Reject the characters and device names Windows reserves.
    pub windows: bool,
}

const WINDOWS_INVALID: [char; 8] = ['<', '>', ':', '"', '|', '?', '*', '\\'];

/// The characters that end a line; a device name followed by an extension holding one of them is
/// not reserved.
const LINE_TERMINATORS: [char; 4] = ['\n', '\r', '\u{2028}', '\u{2029}'];

/// White space in the ECMAScript sense, so that a name gets the same verdict here and in the
/// webview UI. Unlike `char::is_whitespace` it has U+FEFF and lacks U+0085.
fn is_space(c: char) -> bool {
    matches!(c, '\t'..='\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}')
        || matches!(c, '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}')
}

/// Whether `segment` is a Windows device name (`con`, `prn`, `aux`, `nul`, `com0`-`com9`,
/// `lpt0`-`lpt9`, in any case), bare or with an extension.
fn is_windows_reserved(segment: &str) -> bool {
    let (device, extension) = match segment.split_once('.') {
        Some((device, extension)) => (device, extension),
        None => (segment, ""),
    };
    if extension.contains(LINE_TERMINATORS) {
        return false;
    }
    let device = device.to_ascii_lowercase();
    let numbered = |prefix: &str| {
        device.strip_prefix(prefix).is_some_and(|digit| digit.len() == 1 && digit.bytes().all(|b| b.is_ascii_digit()))
    };
    matches!(device.as_str(), "con" | "prn" | "aux" | "nul") || numbered("com") || numbered("lpt")
}

fn name_error(message: impl Into<String>) -> NameCheck {
    NameCheck { error: Some(message.into()), warning: None }
}

/// Validates a file / folder name typed into the inline editor.
pub fn validate_name(raw: &str, opts: &ValidateOptions<'_>) -> NameCheck {
    let trimmed = raw.trim_matches(is_space);
    if trimmed.is_empty() {
        return name_error("Name cannot be empty");
    }
    if raw.contains('\0') {
        return name_error("Name contains an invalid character");
    }
    if raw.starts_with('/') {
        return name_error("Name cannot start with '/'");
    }
    let segments: Vec<&str> = raw.split('/').collect();
    if segments.len() > 1 && !opts.allow_nested {
        return name_error("Name cannot contain '/'");
    }
    for segment in segments.iter().copied() {
        if segment.is_empty() {
            return name_error("Name contains an empty path segment");
        }
        if segment == "." || segment == ".." {
            return name_error(format!("'{segment}' is not a valid name"));
        }
        if opts.windows && (segment.contains(WINDOWS_INVALID) || is_windows_reserved(segment)) {
            return name_error(format!("'{segment}' is not a valid file name"));
        }
        // File systems limit a name to 255 units; the length is counted in UTF-16 ones.
        if segment.encode_utf16().count() > 255 {
            return name_error("Name is too long");
        }
    }
    if opts.current == Some(raw) {
        return NameCheck::default();
    }
    let fold = |name: &str| if opts.case_insensitive { name.to_lowercase() } else { name.to_owned() };
    let first = fold(segments[0]);
    // A nested path may go through an existing directory; the backend reports a real conflict.
    let conflicts = segments.len() == 1
        && opts.current.map(fold).as_ref() != Some(&first)
        && opts.siblings.iter().copied().any(|sibling| fold(sibling) == first);
    if conflicts {
        return name_error(format!("'{}' already exists", segments[0]));
    }
    let warning = (raw != trimmed).then(|| "Name has leading or trailing spaces".to_owned());
    NameCheck { error: None, warning }
}

/// Package flavour of New › Package (JVM, dotted) / New › Python Package (`__init__.py`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageKind {
    Jvm,
    Python,
}

const JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "null",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "true",
    "try",
    "void",
    "volatile",
    "while",
];

const PY_KEYWORDS: &[&str] = &[
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del",
    "elif", "else", "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal",
    "not", "or", "pass", "raise", "return", "try", "while", "with", "yield",
];

/// An ASCII identifier: a letter or `_` (or `$` where `dollar` allows it), then those or digits.
fn is_identifier(segment: &str, dollar: bool) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || (dollar && c == '$');
    segment.chars().next().is_some_and(|first| !first.is_ascii_digit()) && segment.chars().all(word)
}

/// Folder segments of a dotted package name (`com.acme.util` is `com/acme/util`), or the error
/// as IDEA reports it ("Not a valid package name").
pub fn package_segments(raw: &str, kind: PackageKind) -> Result<Vec<&str>, String> {
    let name = raw.trim_matches(is_space);
    if name.is_empty() {
        return Err("Name cannot be empty".to_owned());
    }
    let (keywords, dollar) = match kind {
        PackageKind::Jvm => (JAVA_KEYWORDS, true),
        PackageKind::Python => (PY_KEYWORDS, false),
    };
    let segments: Vec<&str> = name.split('.').collect();
    if segments.iter().all(|segment| is_identifier(segment, dollar) && !keywords.contains(segment)) {
        Ok(segments)
    } else {
        Err(format!("Not a valid package name: '{name}'"))
    }
}

const JVM_BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const PY_BUILD_FILES: [&str; 5] = ["pyproject.toml", "setup.py", "setup.cfg", "Pipfile", "requirements.txt"];

/// What follows `dir/` in `path`, when `path` lies below `dir`.
fn below<'a>(path: &'a str, dir: &str) -> Option<&'a str> {
    path.strip_prefix(dir)?.strip_prefix('/')
}

/// Whether `path` is `dir` or lies below it.
fn at_or_below(path: &str, dir: &str) -> bool {
    path == dir || below(path, dir).is_some()
}

/// Which "New › Package" flavour the project-relative folder `rel` offers: `Jvm` inside a Java /
/// Kotlin source or test root (IDEA shows "Package" there instead of "Directory"), `Python`
/// inside a Python module (PyCharm's "Python Package").
pub fn package_kind_at(rel: &str, roots: &[RootFolder], detected: &[RootFolder]) -> Option<PackageKind> {
    let root = root_for(roots, rel);
    if root.is_some_and(|root| root.kind == FolderRole::Excluded) {
        return None;
    }
    let jvm = root.is_some_and(|root| {
        matches!(root.kind, FolderRole::Source | FolderRole::Test)
            && (root.detected_from.as_deref().is_some_and(|file| JVM_BUILD_FILES.contains(&name_of(file)))
                || JVM_SOURCE_DIRS.contains(&name_of(&root.path)))
    });
    if jvm {
        return Some(PackageKind::Jvm);
    }
    let python = roots.iter().chain(detected).filter_map(|root| root.detected_from.as_deref()).any(|file| {
        let module = file.rsplit_once('/').map_or("", |(dir, _)| dir);
        PY_BUILD_FILES.contains(&name_of(file)) && (module.is_empty() || at_or_below(rel, module))
    });
    python.then_some(PackageKind::Python)
}

/// The icon a folder gets in the project view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FolderKind {
    Plain,
    Source,
    Test,
    Resources,
    TestResources,
    Excluded,
    Library,
    Github,
    Module,
    Package,
    GeneratedSource,
    GeneratedTest,
    Compiled,
    Javadoc,
}

/// The build tool a module folder belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuildTool {
    Maven,
    Gradle,
}

/// The build modules of the project: project-relative module folder (`""` is the project root)
/// to its build tool.
pub type BuildModules = BTreeMap<String, BuildTool>;

fn role_kind(role: FolderRole) -> FolderKind {
    match role {
        FolderRole::Source => FolderKind::Source,
        FolderRole::Test => FolderKind::Test,
        FolderRole::Resources => FolderKind::Resources,
        FolderRole::TestResources => FolderKind::TestResources,
        FolderRole::Excluded => FolderKind::Excluded,
    }
}

/// Icon kind of the folder at the absolute `path`, from the Project Structure marks and the
/// roots detected from build files (`roots` are the effective ones, `root` the project folder).
/// Like IDEA, only excluded folders get the excluded icon; git-ignored ones keep the plain one.
pub fn folder_kind(
    path: &str,
    excluded: bool,
    roots: &[RootFolder],
    root: Option<&str>,
    modules: &BuildModules,
    detected: &[RootFolder],
) -> FolderKind {
    let project = root.filter(|root| !root.is_empty());
    let rel = project.map(|root| if path == root { "" } else { below(path, root).unwrap_or(path) });
    let role = rel.and_then(|rel| root_role(roots, rel));
    let exact = rel.and_then(|rel| roots.iter().find(|root| root.path == rel));
    let marked = exact.is_some_and(|root| root.detected_from.is_none());
    let output = rel.and_then(|rel| output_folder_kind(rel, modules));
    let generated = matches!(output, Some(FolderKind::GeneratedSource | FolderKind::GeneratedTest));
    // A mark of the user, and a root that is generated sources, win over exclusion.
    if let Some(role) = role.filter(|_| marked || generated) {
        return role_kind(role);
    }
    let containing = rel.and_then(|rel| root_for(roots, rel)).map(|root| root.kind);
    if excluded || containing == Some(FolderRole::Excluded) {
        return FolderKind::Excluded;
    }
    if let Some(kind) = role.map(role_kind).or(output) {
        return kind;
    }
    if let Some(rel) = rel.filter(|_| matches!(containing, Some(FolderRole::Source | FolderRole::Test))) {
        let in_module = modules.keys().any(|dir| dir.is_empty() || below(rel, dir).is_some());
        if in_module || package_kind_at(rel, roots, detected) == Some(PackageKind::Jvm) {
            return FolderKind::Package;
        }
    }
    if name_of(path) == ".github" { FolderKind::Github } else { FolderKind::Plain }
}

/// Whether a path below `generated/` has the whole word `test` on its first line, as in
/// `generated/sources/annotationProcessor/java/test`.
fn generated_for_tests(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("generated/") else { return false };
    let line = rest.split(LINE_TERMINATORS).next().unwrap_or_default();
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    line.match_indices("test").any(|(at, found)| {
        let (before, after) = (line[..at].bytes().next_back(), line[at + found.len()..].bytes().next());
        !before.is_some_and(word) && !after.is_some_and(word)
    })
}

/// The role of `path`, a folder relative to a module's output folder (`target`, `build`).
fn output_role(path: &str) -> Option<FolderKind> {
    let within = |dirs: &[&str]| dirs.iter().any(|dir| at_or_below(path, dir));
    if within(&["generated-test-sources"]) || generated_for_tests(path) {
        Some(FolderKind::GeneratedTest)
    } else if within(&["generated-sources", "generated"]) {
        Some(FolderKind::GeneratedSource)
    } else if within(&["classes", "test-classes"]) {
        Some(FolderKind::Compiled)
    } else if within(&["site/apidocs", "apidocs", "docs/javadoc", "javadoc"]) {
        Some(FolderKind::Javadoc)
    } else {
        None
    }
}

/// The role of the project-relative folder `rel` inside a build output folder. Output roles are
/// relative to detected JVM modules (`target` of a Maven one, `build` of a Gradle one), never
/// guessed from arbitrary folder names.
pub fn output_folder_kind(rel: &str, modules: &BuildModules) -> Option<FolderKind> {
    let mut start = 0usize;
    for (slash, _) in rel.match_indices('/') {
        let (module, output, path) = (&rel[..start.saturating_sub(1)], &rel[start..slash], &rel[slash + 1..]);
        start = slash + 1;
        let tool = match output {
            "target" => BuildTool::Maven,
            "build" => BuildTool::Gradle,
            _ => continue,
        };
        if modules.get(module) != Some(&tool) {
            continue;
        }
        if let Some(kind) = output_role(path) {
            return Some(kind);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIBLINGS: &[&str] = &["Order.kt", "model"];

    fn options() -> ValidateOptions<'static> {
        ValidateOptions { siblings: SIBLINGS, ..ValidateOptions::default() }
    }

    fn error_of(name: &str, opts: ValidateOptions<'_>) -> Option<String> {
        validate_name(name, &opts).error
    }

    fn assert_rejected(name: &str, opts: ValidateOptions<'_>, reason: &str) {
        let error = error_of(name, opts);
        assert!(error.as_deref().is_some_and(|message| message.contains(reason)), "{name:?}: {error:?}");
    }

    // src/features/project-tree/paths.test.ts › path helpers › joins, splits and relates paths
    #[test]
    fn joins_splits_and_relates_paths() {
        assert_eq!(join_path("/a", "b"), "/a/b");
        assert_eq!(join_path("/", "b"), "/b");
        assert_eq!(parent_of("/a/b/c.txt"), "/a/b");
        assert_eq!(parent_of("/a"), "/");
        assert_eq!(name_of("/a/b/c.txt"), "c.txt");
        assert!(is_inside("/a/b", "/a"));
        assert!(!is_inside("/ab", "/a"));
        assert!(!is_inside("/a", "/a"));
        assert!(is_same_or_inside("/a", "/a"));
        assert_eq!(relative_to("/r", "/r/src/x.ts"), "src/x.ts");
        assert_eq!(relative_to("/r", "/r"), "");
        assert_eq!(relative_to("/r", "/other"), "/other");
    }

    // src/features/project-tree/paths.test.ts › path helpers › lists ancestors within the root
    #[test]
    fn lists_ancestors_within_the_root() {
        assert_eq!(ancestors_within("/r", "/r/a/b/c.txt"), ["/r", "/r/a", "/r/a/b"]);
        assert_eq!(ancestors_within("/r", "/r/x"), ["/r"]);
        assert!(ancestors_within("/r", "/elsewhere/x").is_empty());
    }

    // src/features/project-tree/paths.test.ts › path helpers › abbreviates the home directory
    #[test]
    fn abbreviates_the_home_directory() {
        assert_eq!(tildify("/Users/me/dev/acme", Some("/Users/me")), "~/dev/acme");
        assert_eq!(tildify("/Users/me", Some("/Users/me/")), "~");
        assert_eq!(tildify("/Users/meow/x", Some("/Users/me")), "/Users/meow/x");
        assert_eq!(tildify("/x", None), "/x");
    }

    // src/features/project-tree/paths.test.ts › path helpers › splits extensions (dotfiles have none)
    #[test]
    fn splits_extensions_dotfiles_have_none() {
        assert_eq!(split_extension("Foo.kt"), ("Foo", ".kt"));
        assert_eq!(split_extension("archive.tar.gz"), ("archive.tar", ".gz"));
        assert_eq!(split_extension(".gitignore"), (".gitignore", ""));
        assert_eq!(split_extension("Makefile"), ("Makefile", ""));
    }

    // src/features/project-tree/paths.test.ts › path helpers › generates Finder-style unique names
    #[test]
    fn generates_finder_style_unique_names() {
        let taken = |name: &str| ["Foo.kt", "Foo copy.kt", "src"].contains(&name);
        assert_eq!(unique_name("Bar.kt", taken, false), "Bar.kt");
        assert_eq!(unique_name("Foo.kt", taken, false), "Foo copy 2.kt");
        assert_eq!(unique_name("src", taken, true), "src copy");
    }

    // src/features/project-tree/paths.test.ts › validateName › rejects empty, invalid and conflicting names
    #[test]
    fn rejects_empty_invalid_and_conflicting_names() {
        assert_rejected("", options(), "empty");
        assert_rejected("   ", options(), "empty");
        assert_rejected("a\0b", options(), "invalid");
        assert_rejected("..", options(), "not a valid");
        assert_rejected("a/b", options(), "cannot contain");
        assert_rejected("Order.kt", options(), "already exists");
        assert_rejected("order.kt", ValidateOptions { case_insensitive: true, ..options() }, "already exists");
        assert_eq!(error_of("order.kt", options()), None);
        assert_rejected("a:b", ValidateOptions { windows: true, ..options() }, "not a valid");
    }

    // src/features/project-tree/paths.test.ts › validateName › allows nested paths for new items and validates each segment
    #[test]
    fn allows_nested_paths_for_new_items_and_validates_each_segment() {
        let nested = ValidateOptions { allow_nested: true, ..options() };
        assert_eq!(error_of("pkg/Sub.kt", nested), None);
        assert_eq!(error_of("model/Sub.kt", nested), None);
        assert_rejected("pkg//Sub.kt", nested, "empty path segment");
        assert_rejected("/abs", nested, "start with");
    }

    // src/features/project-tree/paths.test.ts › validateName › accepts the current name and case-only renames
    #[test]
    fn accepts_the_current_name_and_case_only_renames() {
        let renaming = ValidateOptions { current: Some("Order.kt"), ..options() };
        assert_eq!(error_of("Order.kt", renaming), None);
        assert_eq!(error_of("order.kt", ValidateOptions { case_insensitive: true, ..renaming }), None);
        assert_rejected("model", renaming, "already exists");
    }

    // src/features/project-tree/paths.test.ts › validateName › warns about surrounding spaces
    #[test]
    fn warns_about_surrounding_spaces() {
        assert!(validate_name(" x ", &options()).warning.is_some());
    }

    fn modules() -> BuildModules {
        BTreeMap::from([(String::new(), BuildTool::Maven), ("app".to_owned(), BuildTool::Gradle)])
    }

    fn root(path: &str, kind: FolderRole, detected_from: Option<&str>) -> RootFolder {
        RootFolder { path: path.to_owned(), kind, detected_from: detected_from.map(str::to_owned) }
    }

    fn pom_roots() -> Vec<RootFolder> {
        vec![
            root("src/main/java", FolderRole::Source, Some("pom.xml")),
            root("src/test/java", FolderRole::Test, Some("pom.xml")),
            root("src/main/resources", FolderRole::Resources, Some("pom.xml")),
            root("src/test/resources", FolderRole::TestResources, Some("pom.xml")),
        ]
    }

    fn kind(rel: &str, excluded: bool, roots: &[RootFolder]) -> FolderKind {
        folder_kind(&format!("/p/{rel}"), excluded, roots, Some("/p"), &modules(), &[])
    }

    // src/features/project-tree/folderKind.test.ts › JVM folder presentation › %s: %s (the eight rows of roots, packages and plain folders)
    #[test]
    fn jvm_roots_packages_and_plain_folders() {
        let rows = [
            ("src/main/java", FolderKind::Source),
            ("src/test/java", FolderKind::Test),
            ("src/main/resources", FolderKind::Resources),
            ("src/test/resources", FolderKind::TestResources),
            ("src/main/java/com/example", FolderKind::Package),
            ("src/test/java/com", FolderKind::Package),
            ("src/main/resources/templates", FolderKind::Plain),
            ("plain", FolderKind::Plain),
        ];
        for (path, expected) in rows {
            assert_eq!(kind(path, false, &pom_roots()), expected, "{path}");
        }
    }

    // src/features/project-tree/folderKind.test.ts › JVM folder presentation › %s: %s (the seven rows of build output folders)
    #[test]
    fn build_output_folders() {
        let rows = [
            ("target/generated-sources", FolderKind::GeneratedSource),
            ("target/generated-test-sources", FolderKind::GeneratedTest),
            ("app/build/generated", FolderKind::GeneratedSource),
            ("app/build/classes/kotlin", FolderKind::Compiled),
            ("target/classes", FolderKind::Compiled),
            ("target/site/apidocs", FolderKind::Javadoc),
            ("app/build/docs/javadoc", FolderKind::Javadoc),
        ];
        for (path, expected) in rows {
            assert_eq!(kind(path, false, &pom_roots()), expected, "{path}");
            assert_eq!(kind(path, true, &pom_roots()), FolderKind::Excluded, "{path}, excluded");
            assert_eq!(output_folder_kind(path, &BuildModules::new()), None, "{path}, no modules");
        }
    }

    // src/features/project-tree/folderKind.test.ts › JVM folder presentation › shows packages below custom marked JVM source roots
    #[test]
    fn shows_packages_below_custom_marked_jvm_source_roots() {
        let roots = [root("custom", FolderRole::Source, None)];
        assert_eq!(kind("custom/com/example", false, &roots), FolderKind::Package);
    }

    // src/features/project-tree/folderKind.test.ts › JVM folder presentation › keeps marks and generated source roots ahead of output and exclusion
    #[test]
    fn keeps_marks_and_generated_source_roots_ahead_of_output_and_exclusion() {
        let generated = [root("target/generated-sources", FolderRole::Source, Some("pom.xml"))];
        assert_eq!(kind("target/generated-sources", true, &generated), FolderKind::Source);
        let marked = [root("target/classes", FolderRole::Test, None)];
        assert_eq!(kind("target/classes", false, &marked), FolderKind::Test);
        let mut roots = pom_roots();
        roots.push(root("src/main/java/com", FolderRole::Excluded, None));
        assert_eq!(kind("src/main/java/com", false, &roots), FolderKind::Excluded);
    }

    // src/features/project-tree/folderKind.test.ts › JVM folder presentation › does not guess output roles for %s (four rows)
    #[test]
    fn does_not_guess_output_roles() {
        for path in ["generated-sources", "src/main/java/classes", "other/build/generated", "build/generated"] {
            assert_eq!(output_folder_kind(path, &modules()), None, "{path}");
        }
    }
}
