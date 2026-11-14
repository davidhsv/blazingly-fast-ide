//! Virtual documents: read-only contents a language server provides for a non-`file:` URI
//! (library classes from jdtls' `jdt://contents/…`, decompiled code). Locations into them carry
//! the URI as their path; the editor opens them as virtual editor inputs instead of files.
//!
//! The path helpers here take such a URI wherever a file path is expected.

use std::borrow::Cow;

/// Compiled Java classes on disk. They are resolved before the binary viewer, at every opening
/// entry; a class of a `jdt:` library is a document of the language server instead.
pub fn is_class_file(path: &str) -> bool {
    !path.starts_with("jdt:")
        && path.as_bytes().last_chunk::<6>().is_some_and(|tail| tail.eq_ignore_ascii_case(b".class"))
}

/// Why no breakpoint can be set in `path`, when it is a compiled class.
pub fn class_file_breakpoint_reason(path: &str) -> Option<&'static str> {
    is_class_file(path).then_some("Breakpoints are not supported in output classes without source code.")
}

/// Gradle scripts and static inputs whose edits can change the build model or libraries.
pub fn is_gradle_build_input(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let named = |suffix: &str| name.strip_suffix(suffix).is_some_and(|stem| !stem.is_empty());
    named(".gradle")
        || named(".gradle.kts")
        || named(".versions.toml")
        || matches!(name, "gradle.properties" | "gradle-wrapper.properties")
}

/// Server document URIs, including decompiled disk classes; drive letters remain paths.
pub fn is_virtual_uri(s: &str) -> bool {
    let Some((scheme, _)) = s.split_once(':') else { return false };
    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && scheme.len() >= 2
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && (!scheme.eq_ignore_ascii_case("file") || is_class_file(s))
}

/// `uri` without its query and fragment.
fn without_query(uri: &str) -> &str {
    uri.split(['?', '#']).next().unwrap_or(uri)
}

/// What follows `scheme:` and an optional `//`; all of `uri` when it has no scheme.
fn after_scheme(uri: &str) -> &str {
    match uri.split_once(':') {
        Some((scheme, rest)) if !scheme.is_empty() => rest.strip_prefix("//").unwrap_or(rest),
        _ => uri,
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    char::from(byte).to_digit(16).and_then(|digit| u8::try_from(digit).ok())
}

/// `segment` with its `%XX` escapes decoded; `None` when an escape is malformed or the decoded
/// bytes are not UTF-8.
fn percent_decoded(segment: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(segment.len());
    let mut rest = segment.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let (high, low) = (hex_value(*tail.first()?)?, hex_value(*tail.get(1)?)?);
            bytes.push(high * 16 + low);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

/// A URI segment as shown to the user: decoded, or as written when it does not decode.
fn decoded(segment: &str) -> String {
    percent_decoded(segment).unwrap_or_else(|| segment.to_owned())
}

/// Last path segment of a URI (without query / fragment), e.g. `SpringApplication.class`.
pub fn virtual_name(uri: &str) -> String {
    let path = without_query(after_scheme(uri));
    decoded(path.rsplit('/').find(|segment| !segment.is_empty()).unwrap_or(uri))
}

/// The URI's path without its last segment, e.g. `jdt://contents/x.jar/org.example`.
pub fn virtual_parent(uri: &str) -> &str {
    let base = without_query(uri);
    let Some(slash) = base.rfind('/') else { return base };
    let below_authority = match base.find("://") {
        Some(scheme_end) => slash > scheme_end + 2,
        // Without an authority, a slash within the first two UTF-16 units separates nothing.
        None => base[..slash].encode_utf16().nth(1).is_some(),
    };
    if below_authority { &base[..slash] } else { base }
}

/// The archive path and the entry path of a `jar:` URI without query:
/// `jar:file:///m2/a.jar!/org/x/Y.class` → (`/m2/a.jar`, `org/x/Y.class`).
fn jar_parts(uri: &str) -> Option<(&str, &str)> {
    let rest = uri.strip_prefix("jar:")?;
    let rest = rest.strip_prefix("file:").unwrap_or(rest);
    let rest = rest.strip_prefix("//").unwrap_or(rest);
    if rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    rest.split_once("!/")
}

/// Short human location of a virtual document, e.g. `spring-boot-4.1.1.jar › org.springframework.boot`
/// for `jdt://contents/spring-boot-4.1.1.jar/org.springframework.boot/SpringApplication.class?=…`.
pub fn virtual_container(uri: &str) -> String {
    let scheme = uri.split_once(':').map_or("", |(scheme, _)| scheme);
    // `jar:///…/a-1.0.jar!/org/example/X.class` → `a-1.0.jar › org.example`
    if scheme == "jar"
        && let Some((archive, entry)) = jar_parts(without_query(uri))
    {
        let archive = decoded(archive.rsplit('/').next().unwrap_or(archive));
        let mut package: Vec<String> = entry.split('/').map(decoded).collect();
        package.pop();
        let package = package.join(".");
        return if package.is_empty() { archive } else { format!("{archive} › {package}") };
    }
    let path = without_query(after_scheme(uri));
    let mut segments: Vec<String> = path.split('/').filter(|segment| !segment.is_empty()).map(decoded).collect();
    segments.pop();
    if scheme == "jdt" && segments.first().is_some_and(|first| first == "contents") {
        segments.remove(0);
    }
    segments.join(" › ")
}

/// The name of a file, or of the document a URI stands for.
pub fn basename(path: &str) -> Cow<'_, str> {
    if is_virtual_uri(path) {
        return Cow::Owned(virtual_name(path));
    }
    Cow::Borrowed(path.rsplit_once('/').map_or(path, |(_, name)| name))
}

/// The folder of a file, or the parent of a URI.
pub fn dirname(path: &str) -> &str {
    if is_virtual_uri(path) {
        return virtual_parent(path);
    }
    match path.rfind('/') {
        Some(slash) if slash > 0 => &path[..slash],
        _ => "/",
    }
}

/// `path` relative to `root` when it lies below it, else as it is; for a URI, its container and
/// name.
pub fn relative_path<'a>(root: &str, path: &'a str) -> Cow<'a, str> {
    if is_virtual_uri(path) {
        let parts = [virtual_container(path), virtual_name(path)];
        let shown: Vec<&str> = parts.iter().map(String::as_str).filter(|part| !part.is_empty()).collect();
        return Cow::Owned(shown.join(" › "));
    }
    Cow::Borrowed(path.strip_prefix(root).and_then(|rest| rest.strip_prefix('/')).unwrap_or(path))
}

/// Project-relative directory of `path`; "" for files directly in `root`.
pub fn relative_dir<'a>(root: &str, path: &'a str) -> Cow<'a, str> {
    if is_virtual_uri(path) {
        return Cow::Owned(virtual_container(path));
    }
    let dir = dirname(path);
    if dir == root { Cow::Borrowed("") } else { relative_path(root, dir) }
}

#[cfg(test)]
mod tests {
    use super::*;
    mod proofs7_relative_dir;

    const JDT: &str = concat!(
        "jdt://contents/spring-boot-4.1.1.jar/org.springframework.boot/SpringApplication.class",
        "?=demo/%5C/m2%5C/spring-boot-4.1.1.jar%3Corg.springframework.boot%28SpringApplication.class"
    );

    // src/platform/virtualDocs.test.ts › tells URIs from file paths
    #[test]
    fn tells_uris_from_file_paths() {
        assert!(is_virtual_uri(JDT));
        assert!(is_virtual_uri("untitled:Untitled-1"));
        assert!(!is_virtual_uri("/Users/me/a.java"));
        assert!(!is_virtual_uri(r"C:\src\a.java"));
        assert!(!is_virtual_uri("file:///a.java"));
    }

    // src/platform/virtualDocs.test.ts › names and locates library classes
    #[test]
    fn names_and_locates_library_classes() {
        assert_eq!(virtual_name(JDT), "SpringApplication.class");
        assert_eq!(virtual_parent(JDT), "jdt://contents/spring-boot-4.1.1.jar/org.springframework.boot");
        assert_eq!(virtual_container(JDT), "spring-boot-4.1.1.jar › org.springframework.boot");
        // `jar:` targets (official Kotlin LSP, sources jars), in both URI forms.
        const ENTRY: &str = "org/springframework/boot/SpringApplicationExtensions.kt";
        for prefix in ["jar:///", "jar:file:///"] {
            let uri = format!("{prefix}home/me/.m2/repository/spring-boot-3.5.16-sources.jar!/{ENTRY}");
            assert_eq!(virtual_container(&uri), "spring-boot-3.5.16-sources.jar › org.springframework.boot");
        }
        assert_eq!(virtual_container("jar:///tmp/my%20libs/a.jar!/Root.class"), "a.jar");
        assert_eq!(virtual_name("jar:///tmp/a.jar!/org/x/Y.class"), "Y.class");
    }

    // src/platform/virtualDocs.test.ts › makes path helpers URI-aware
    #[test]
    fn makes_path_helpers_uri_aware() {
        assert_eq!(basename(JDT), "SpringApplication.class");
        assert_eq!(basename(dirname(JDT)), "org.springframework.boot");
        assert_eq!(
            relative_path("/p", JDT),
            "spring-boot-4.1.1.jar › org.springframework.boot › SpringApplication.class"
        );
        assert_eq!(relative_dir("/p", JDT), "spring-boot-4.1.1.jar › org.springframework.boot");
        assert_eq!(relative_path("/p", "/p/src/A.java"), "src/A.java");
    }

    // src/platform/classFiles.test.ts › disables output breakpoints even with any-file enabled: %s
    #[test]
    fn disables_output_breakpoints_even_with_any_file_enabled() {
        for path in [
            "/p/target/classes/p/C.class",
            "/p/build/classes/java/main/p/C$1.class",
            r"C:\p\out\C.class",
            "file:///p/out/C.class",
        ] {
            assert!(is_class_file(path), "{path}");
            assert!(class_file_breakpoint_reason(path).is_some_and(|reason| !reason.is_empty()), "{path}");
        }
    }

    // src/platform/classFiles.test.ts › keeps source and JDT breakpoints enabled: %s
    #[test]
    fn keeps_source_and_jdt_breakpoints_enabled() {
        for path in [
            "jdt://contents/a.jar/p/C.class?=p/a.jar%3Cp%28C.class",
            "jdt://contents/a.jar/p/C.java?=p/a.jar%3Cp%28C.class",
            "/p/src/main/java/p/C.java",
        ] {
            assert!(!is_class_file(path), "{path}");
            assert_eq!(class_file_breakpoint_reason(path), None, "{path}");
        }
    }

    // src/platform/classFiles.test.ts › treats disk class URIs as read-only server documents
    #[test]
    fn treats_disk_class_uris_as_read_only_server_documents() {
        assert!(is_virtual_uri("file:///p/out/C.class"));
        assert!(!is_virtual_uri("file:///p/src/C.java"));
        assert!(is_class_file("/m2/a.jar!/p/C.class"));
    }

    // src/platform/buildFiles.test.ts › recognizes applied scripts and Gradle properties for build and library refresh
    #[test]
    fn recognizes_applied_scripts_and_gradle_properties_for_build_and_library_refresh() {
        for path in [
            "/p/gradle/dependencies.gradle",
            "/p/gradle/dependencies.gradle.kts",
            r"C:\p\gradle\dependencies.gradle",
            "/p/gradle.properties",
            "/p/gradle/wrapper/gradle-wrapper.properties",
            "/p/gradle/libs.versions.toml",
        ] {
            assert!(is_gradle_build_input(path), "{path}");
        }
        for path in ["/p/dependencies.gradle.bak", "/p/src/main.kt", "/p/README.md"] {
            assert!(!is_gradle_build_input(path), "{path}");
        }
    }
}
