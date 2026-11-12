//! Clickable locations in run output: compiler diagnostics and stack traces for common toolchains,
//! plus URLs. Positions are 1-based as printed; consumers convert to the IDE's 0-based lines.

use crate::editor::buffer::col_of_byte;
use crate::find::utf16;
use crate::terminal::links::trim_url;
use regex::{Captures, Regex};
use std::sync::LazyLock;

/// How a printed location is turned into a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resolve {
    /// Resolve the path against the run directory.
    Path,
    /// A file name from a JVM stack frame: look it up in the project (the path then is the file under
    /// its package directory, `com/acme/OrderController.kt`, when the frame names the class).
    Name,
    /// A Minitest test with no file in its output: `class_name` is its class, `path` its method name,
    /// found by searching the project.
    RubyTest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTarget {
    /// As printed (relative, absolute or a bare file name).
    pub path: String,
    pub line: Option<u32>,
    pub col: Option<u32>,
    pub resolve: Resolve,
    /// JVM frames: the class's binary name (`com.acme.OrderController$Inner`).
    pub class_name: Option<String>,
}

/// The target of a JVM stack frame `at com.acme.Foo.bar(Foo.kt:22)`: the file under its package
/// directory, so frames resolve by fully qualified class name (as in IDEA), not by a bare file name
/// that several packages may share.
pub fn jvm_frame_target(class_name: Option<&str>, file: &str, line: Option<u32>) -> FileTarget {
    let class_name = class_name.filter(|name| !name.is_empty());
    let package = class_name.and_then(|name| name.rsplit_once('.')).map(|(package, _)| package.replace('.', "/"));
    FileTarget {
        path: match package.filter(|package| !package.is_empty()) {
            Some(package) => format!("{package}/{file}"),
            None => file.to_string(),
        },
        line,
        col: None,
        resolve: Resolve::Name,
        class_name: class_name.map(str::to_string),
    }
}

/// A link in a line of output; `start` and `end` are UTF-16 columns of the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputLink {
    Url { start: u32, end: u32, url: String },
    File { start: u32, end: u32, target: FileTarget },
}

impl OutputLink {
    pub fn start(&self) -> u32 {
        match self {
            Self::Url { start, .. } | Self::File { start, .. } => *start,
        }
    }

    pub fn end(&self) -> u32 {
        match self {
            Self::Url { end, .. } | Self::File { end, .. } => *end,
        }
    }

    pub fn target(&self) -> Option<&FileTarget> {
        match self {
            Self::File { target, .. } => Some(target),
            Self::Url { .. } => None,
        }
    }
}

/// What a link opens, before its columns are known.
enum Found {
    Url(String),
    File(FileTarget),
}

/// A file path as tools print it: an optional drive, home or relative prefix, directories, and a
/// name with an extension.
const PATH: &str = concat!(
    r"(?:[A-Za-z]:[\\/]|~?/|\.{1,2}/)?(?:[A-Za-z0-9_.@+\-]+[\\/])*",
    r"[A-Za-z0-9_@+\-][A-Za-z0-9_.@+\-]*\.[A-Za-z][A-Za-z0-9_]{0,9}",
);

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the output link patterns are valid")
}

/// Python: `File "app/main.py", line 12, in <module>`.
static PYTHON: LazyLock<Regex> = LazyLock::new(|| pattern(r#"File "([^"]+)", line ([0-9]+)"#));
/// JVM: `at com.acme.OrderController.list(OrderController.kt:22)`.
static JVM: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"(?:([A-Za-z0-9_$]+(?:\.[A-Za-z0-9_$]+)*)\.[A-Za-z0-9_$<>-]+)?",
        r"\(([A-Za-z0-9_$.-]+\.(?:java|kt|kts|scala|groovy|clj)):([0-9]+)\)",
    ))
});
/// JS / Node: `at foo (src/a.ts:3:7)`, `at file:///abs/a.mjs:3:7`, `at /abs/a.js:3:7`.
static SCRIPT: LazyLock<Regex> = LazyLock::new(|| pattern(&format!(r"(?:file://)?({PATH}):([0-9]+):([0-9]+)")));
/// Generic `path:line` (Go `file.go:12`, Rust `--> src/main.rs:10`, gcc, eslint…), `path(line,col)`
/// (tsc) and Maven's `path:[line,col]` (`[ERROR] /p/A.java:[12,5] msg`).
static GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    pattern(&format!(r"({PATH})(?::\[?([0-9]+)(?::([0-9]+)|,([0-9]+)\])?|\(([0-9]+)(?:,([0-9]+))?\))"))
});
/// `http(s)://` URLs up to the next white space, quote or angle bracket.
static URL: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r"(?-u:\b)https?://",
        r"[^\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}",
        r#"<>"'`]+"#,
    ))
});

fn number(found: &Captures, groups: &[usize]) -> Option<u32> {
    groups.iter().find_map(|group| found.get(*group))?.as_str().parse().ok()
}

fn path_target(found: &Captures, line: &[usize], col: &[usize]) -> FileTarget {
    FileTarget {
        path: found[1].to_string(),
        line: number(found, line),
        col: number(found, col),
        resolve: Resolve::Path,
        class_name: None,
    }
}

/// Finds non-overlapping links in `text`; URLs first, then the more specific patterns win.
pub fn find_output_links(text: &str) -> Vec<OutputLink> {
    if utf16::len(text) > 4000 {
        return Vec::new();
    }
    // Byte ranges while matching; columns once the set is final.
    let mut links: Vec<(usize, usize, Found)> = Vec::new();
    for found in URL.find_iter(text) {
        let url = trim_url(found.as_str());
        links.push((found.start(), found.start() + url.len(), Found::Url(url.to_string())));
    }
    let mut add = |start: usize, end: usize, target: FileTarget| {
        if !links.iter().any(|(from, to, _)| start < *to && end > *from) {
            links.push((start, end, Found::File(target)));
        }
    };
    for found in PYTHON.captures_iter(text) {
        let whole = found.get(0).map_or(0..0, |whole| whole.range());
        add(whole.start, whole.end, path_target(&found, &[2], &[]));
    }
    for found in JVM.captures_iter(text) {
        let (Some(file), Some(line)) = (found.get(2), found.get(3)) else { continue };
        let class_name = found.get(1).map(|class_name| class_name.as_str());
        add(file.start(), line.end(), jvm_frame_target(class_name, file.as_str(), line.as_str().parse().ok()));
    }
    for found in SCRIPT.captures_iter(text) {
        let whole = found.get(0).map_or(0..0, |whole| whole.range());
        add(whole.start, whole.end, path_target(&found, &[2], &[3]));
    }
    for found in GENERIC.captures_iter(text) {
        let whole = found.get(0).map_or(0..0, |whole| whole.range());
        // A location glued to a longer word or path is not one.
        let glued = text[..whole.start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '~' | '-'));
        if !glued {
            add(whole.start, whole.end, path_target(&found, &[2, 5], &[3, 4, 6]));
        }
    }
    links.sort_by_key(|(start, _, _)| *start);
    links
        .into_iter()
        .map(|(start, end, found)| {
            let (start, end) = (col_of_byte(text, start), col_of_byte(text, end));
            match found {
                Found::Url(url) => OutputLink::Url { start, end, url },
                Found::File(target) => OutputLink::File { start, end, target },
            }
        })
        .collect()
}

/// Joins `path` onto `base` (POSIX), normalising `.`/`..`. Absolute paths are kept.
pub fn join_path(base: &str, path: &str) -> String {
    let path = path.replace('\\', "/");
    let path = path.strip_prefix("file://").unwrap_or(&path);
    let has_drive = |p: &str| matches!(p.as_bytes(), [letter, b':', b'/', ..] if letter.is_ascii_alphabetic());
    let full = if path.starts_with('/') || has_drive(path) {
        path.to_string()
    } else {
        format!("{}/{path}", base.trim_end_matches('/'))
    };
    let mut out: Vec<&str> = Vec::new();
    for segment in full.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            _ => out.push(segment),
        }
    }
    let drive = out.first().is_some_and(|first| matches!(first.as_bytes(), [l, b':'] if l.is_ascii_alphabetic()));
    format!("{}{}", if drive { "" } else { "/" }, out.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file link as the cases compare it: its target and the linked text.
    #[derive(Debug, PartialEq)]
    struct Linked {
        path: String,
        line: Option<u32>,
        col: Option<u32>,
        resolve: Resolve,
        class_name: Option<String>,
        text: String,
    }

    fn files(text: &str) -> Vec<Linked> {
        find_output_links(text)
            .into_iter()
            .filter_map(|link| match link {
                OutputLink::File { start, end, target } => Some(Linked {
                    path: target.path,
                    line: target.line,
                    col: target.col,
                    resolve: target.resolve,
                    class_name: target.class_name,
                    text: utf16::slice(text, start, end).to_string(),
                }),
                OutputLink::Url { .. } => None,
            })
            .collect()
    }

    fn linked(
        path: &str,
        line: u32,
        col: Option<u32>,
        resolve: Resolve,
        class_name: Option<&str>,
        text: &str,
    ) -> Linked {
        Linked {
            path: path.into(),
            line: Some(line),
            col,
            resolve,
            class_name: class_name.map(str::to_string),
            text: text.into(),
        }
    }

    /// The first file link's path, line and column.
    fn first(text: &str) -> (String, Option<u32>, Option<u32>) {
        let link = files(text).into_iter().next().expect("a file link");
        (link.path, link.line, link.col)
    }

    #[test]
    fn rust_diagnostics_and_panics() {
        let expected = linked("src/main.rs", 10, Some(5), Resolve::Path, None, "src/main.rs:10:5");
        assert_eq!(files("  --> src/main.rs:10:5"), [expected]);
        assert_eq!(
            first("thread 'main' panicked at crates/app/src/lib.rs:42:9:"),
            ("crates/app/src/lib.rs".to_string(), Some(42), Some(9))
        );
    }

    #[test]
    fn node_v8_stack_frames() {
        let expected = linked("src/server.ts", 3, Some(7), Resolve::Path, None, "src/server.ts:3:7");
        assert_eq!(files("    at foo (src/server.ts:3:7)"), [expected]);
        let (path, line, _) = first("    at file:///Users/me/app/index.mjs:12:1");
        assert_eq!((path.as_str(), line), ("/Users/me/app/index.mjs", Some(12)));
        assert_eq!(first("    at Object.<anonymous> (/abs/dir/x.js:1:2)").0, "/abs/dir/x.js");
    }

    #[test]
    fn python_tracebacks() {
        let found = files("  File \"app/main.py\", line 3, in <module>");
        assert_eq!(found, [linked("app/main.py", 3, None, Resolve::Path, None, "File \"app/main.py\", line 3")]);
    }

    #[test]
    fn java_kotlin_frames_resolve_by_fully_qualified_class_name() {
        let found = files("\tat com.acme.orders.OrderController.list(OrderController.kt:22)");
        let class_name = Some("com.acme.orders.OrderController");
        let path = "com/acme/orders/OrderController.kt";
        assert_eq!(found, [linked(path, 22, None, Resolve::Name, class_name, "OrderController.kt:22")]);
        // Nested classes and lambdas live in the outer class's file; JDK module prefixes are skipped.
        let frame = |text: &str| {
            let link = files(text).remove(0);
            (link.path, link.class_name.unwrap_or_default())
        };
        let expected = |path: &str, class_name: &str| (path.to_string(), class_name.to_string());
        assert_eq!(
            frame("\tat com.acme.Outer$Inner.lambda$run$0(Outer.java:7)"),
            expected("com/acme/Outer.java", "com.acme.Outer$Inner")
        );
        assert_eq!(
            frame("\tat java.base/java.util.ArrayList.get(ArrayList.java:427)"),
            expected("java/util/ArrayList.java", "java.util.ArrayList")
        );
        // The default package, and a bare `(File.java:3)` without a frame.
        assert_eq!(frame("\tat Main.main(Main.java:3)"), expected("Main.java", "Main"));
        assert_eq!(files("(Main.java:3)"), [linked("Main.java", 3, None, Resolve::Name, None, "Main.java:3")]);
    }

    #[test]
    fn go_and_generic_path_line() {
        let found = |path: &str, line: u32, col: Option<u32>| (path.to_string(), Some(line), col);
        assert_eq!(first("    math_test.go:12: expected 1"), found("math_test.go", 12, None));
        assert_eq!(first("./cmd/server/main.go:8:2: undefined: x"), found("./cmd/server/main.go", 8, Some(2)));
        assert_eq!(first("src/App.tsx(14,3): error TS2322"), found("src/App.tsx", 14, Some(3)));
        // Go module cache paths with @version and stack frames.
        assert_eq!(
            first("        /Users/user/go/pkg/mod/github.com/spf13/cobra@v1.10.2/command.go:1071"),
            found("/Users/user/go/pkg/mod/github.com/spf13/cobra@v1.10.2/command.go", 1071, None)
        );
        // Go stack frames with trailing colon.
        assert_eq!(
            first("        /Users/user/projects/demo/pkg/api/server.go:42:"),
            found("/Users/user/projects/demo/pkg/api/server.go", 42, None)
        );
        // Relative paths in loggers (e.g. zap/zerolog/slog).
        assert_eq!(
            first("2026-01-01T12:00:00.000+0000\tDEBUG\tpkg/store/db.go:15\tfailed to connect"),
            found("pkg/store/db.go", 15, None)
        );
    }

    #[test]
    fn maven_diagnostics_error_path_line_col() {
        let text = "/p/src/main/java/a/A.java:[12,5]";
        assert_eq!(
            files("[ERROR] /p/src/main/java/a/A.java:[12,5] cannot find symbol"),
            [linked("/p/src/main/java/a/A.java", 12, Some(5), Resolve::Path, None, text)]
        );
        assert_eq!(
            first("[WARNING] src/main/java/B.java:[3,8] [deprecation] old() has been deprecated"),
            ("src/main/java/B.java".to_string(), Some(3), Some(8))
        );
        let windows = first("[ERROR] C:\\proj\\A.java:[7,1] error: x");
        assert_eq!(windows, ("C:\\proj\\A.java".to_string(), Some(7), Some(1)));
    }

    #[test]
    fn urls_trimming_trailing_punctuation() {
        let links = find_output_links("Listening on http://localhost:8080/api. (see https://x.dev/a_(b))");
        let urls: Vec<&str> = links
            .iter()
            .filter_map(|link| match link {
                OutputLink::Url { url, .. } => Some(url.as_str()),
                OutputLink::File { .. } => None,
            })
            .collect();
        assert_eq!(urls, ["http://localhost:8080/api", "https://x.dev/a_(b)"]);
    }

    #[test]
    fn does_not_link_url_ports_or_versions_as_files() {
        assert!(files("Started at http://127.0.0.1:3000").is_empty());
        assert!(files("version 1.2.3:4").is_empty());
        assert!(files("plain text 12:30").is_empty());
    }

    #[test]
    fn returns_links_sorted_and_non_overlapping() {
        let links = find_output_links("a.rs:1 then b.ts:2:3");
        assert_eq!(links.iter().map(OutputLink::start).collect::<Vec<_>>(), [0, 12]);
    }

    #[test]
    fn joins_and_normalises() {
        assert_eq!(join_path("/p/app", "src/main.rs"), "/p/app/src/main.rs");
        assert_eq!(join_path("/p/app", "./a/../b.ts"), "/p/app/b.ts");
        assert_eq!(join_path("/p/app", "/abs/x.py"), "/abs/x.py");
        assert_eq!(join_path("/p/app/", "file:///abs/y.js"), "/abs/y.js");
    }

    // Not from a spec: link columns count UTF-16 code units, and a drive path keeps its drive.
    #[test]
    fn columns_are_utf16_and_drive_paths_stay_absolute() {
        let links = find_output_links("😀 src/main.rs:3");
        assert_eq!((links[0].start(), links[0].end()), (3, 16));
        assert_eq!(links[0].target().map(|target| target.path.as_str()), Some("src/main.rs"));
        assert_eq!(join_path("/p", "C:\\proj\\..\\A.java"), "C:/A.java");
    }
}
