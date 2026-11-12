//! Reading an agent conversation: quoting, file links, turn times and finding text.

use crate::find::utf16;
use regex::Regex;
use std::sync::LazyLock;

/// `text` as a Markdown quote for the composer, followed by an empty line to type in.
pub fn quote_text(text: &str) -> String {
    let lines: Vec<String> =
        text.trim_end().split('\n').map(|l| if l.is_empty() { ">".to_owned() } else { format!("> {l}") }).collect();
    format!("{}\n\n", lines.join("\n"))
}

/// Appends a quote to a draft, on its own paragraph.
pub fn with_quote(draft: &str, text: &str) -> String {
    if draft.trim().is_empty() {
        return quote_text(text);
    }
    format!("{}\n\n{}", draft.trim_end(), quote_text(text))
}

/// A file a conversation names.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathLink {
    /// Absolute path.
    pub path: String,
    /// 0-based line and UTF-16 column, when the text names them.
    pub line: Option<u32>,
    pub col: Option<u32>,
}

/// A 1-based number as written → 0-based.
fn zero_based(digits: &str) -> u32 {
    digits.parse::<u32>().unwrap_or(u32::MAX).saturating_sub(1)
}

/// `%XX` escapes decoded as UTF-8; `None` for a malformed escape or bytes that are not UTF-8.
pub(crate) fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Whether `text` starts with a URL scheme (`https:`, `javascript:`…).
pub(crate) fn has_scheme(text: &str) -> bool {
    static SCHEME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9+.\-]*:").expect("the pattern is valid"));
    SCHEME.is_match(text)
}

fn joined(cwd: &str, rel: &str) -> String {
    format!("{}/{rel}", cwd.strip_suffix('/').unwrap_or(cwd))
}

/// The file an explicit Markdown link names: such links can also name files with spaces or non-ASCII
/// names. `None` for anchors, protocol-relative and scheme URLs, malformed escapes and control characters.
pub fn file_href(href: &str, cwd: &str) -> Option<PathLink> {
    static SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^((?s:.)*?)(?::([0-9]+)(?::([0-9]+))?|#L([0-9]+))?$").expect("the pattern is valid")
    });
    if href.is_empty() || href.starts_with('#') || href.starts_with("//") || has_scheme(href) {
        return None;
    }
    let decoded = decode_uri_component(href)?;
    if decoded.chars().any(|c| (c as u32) < 32) {
        return None;
    }
    let m = SUFFIX.captures(&decoded)?;
    let rel = m.get(1).map(|g| g.as_str()).filter(|rel| !rel.is_empty())?;
    let path = if rel.starts_with('/') { rel.to_owned() } else { joined(cwd, rel) };
    let line = m.get(2).or_else(|| m.get(4)).map(|g| zero_based(g.as_str()));
    let col = m.get(3).map(|g| zero_based(g.as_str()));
    Some(PathLink { path, line, col })
}

/// The file an inline code span names (`src/lib.rs`, `src/main.rs:12`, `a/b.ts:3:5`, `/abs/x.py#L7`),
/// resolved against `cwd`; `None` for text that isn't a file path (`foo()`, `cargo test`, URLs, `1.5`,
/// folders, words without an extension).
pub fn path_link(text: &str, cwd: &str) -> Option<PathLink> {
    static PATH: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"^(?:\./)?([A-Za-z0-9_.@\-]+(?:/[A-Za-z0-9_.@\-]+)*\.[A-Za-z][A-Za-z0-9]*)",
            r"(?::([0-9]+)(?::([0-9]+))?|#L([0-9]+))?$",
        ))
        .expect("the pattern is valid")
    });
    let t = text.trim();
    if t.is_empty() || t.chars().any(char::is_whitespace) || t.contains("://") {
        return None;
    }
    let absolute = t.strip_prefix('/');
    let m = PATH.captures(absolute.unwrap_or(t))?;
    let rel = m.get(1)?.as_str();
    let path = if absolute.is_some() { format!("/{rel}") } else { joined(cwd, rel) };
    let line = m.get(2).or_else(|| m.get(4)).map(|g| zero_based(g.as_str()));
    let col = m.get(3).map(|g| zero_based(g.as_str()));
    Some(PathLink { path, line, col })
}

/// `850ms`, `12s`, `1m 05s`, `2h 03m`.
pub fn format_duration(ms: f64) -> String {
    if ms < 1000.0 {
        return format!("{}ms", ms.max(0.0).round());
    }
    let s = (ms / 1000.0).floor() as u64;
    if s < 60 {
        return format!("{s}s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("{m}m {:02}s", s % 60);
    }
    format!("{}h {:02}m", m / 60, m % 60)
}

/// UTF-16 start offsets of `query` in `text`, ignoring case; empty for an empty query.
pub fn find_all(text: &str, query: &str) -> Vec<u32> {
    if query.is_empty() {
        return Vec::new();
    }
    let hay = text.to_lowercase();
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        out.push(utf16::len(&hay[..start]));
        from = start + needle.len();
    }
    out
}

/// A match of Find in Conversation: the text run it lies in and its UTF-16 range there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunMatch {
    pub run: usize,
    pub start: u32,
    pub end: u32,
}

/// The matches of `query` in the text runs of a rendered message; a match never spans two runs.
pub fn find_in_runs<S: AsRef<str>>(runs: &[S], query: &str) -> Vec<RunMatch> {
    let len = utf16::len(query);
    runs.iter()
        .enumerate()
        .flat_map(|(run, text)| {
            find_all(text.as_ref(), query).into_iter().map(move |start| RunMatch { run, start, end: start + len })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_each_line_and_leaves_a_line_to_type_on() {
        assert_eq!(quote_text("one\n\ntwo\n"), "> one\n>\n> two\n\n");
    }

    #[test]
    fn adds_the_quote_after_a_draft() {
        assert_eq!(with_quote("", "x"), "> x\n\n");
        assert_eq!(with_quote("Why?  ", "x"), "Why?\n\n> x\n\n");
    }

    fn link(path: &str, line: Option<u32>, col: Option<u32>) -> Option<PathLink> {
        Some(PathLink { path: path.to_owned(), line, col })
    }

    #[test]
    fn resolves_relative_paths_with_a_line_and_column() {
        let cwd = "/repo";
        assert_eq!(path_link("src/lib.rs", cwd), link("/repo/src/lib.rs", None, None));
        assert_eq!(path_link("./src/main.rs:12", cwd), link("/repo/src/main.rs", Some(11), None));
        assert_eq!(path_link("a/b.ts:3:5", cwd), link("/repo/a/b.ts", Some(2), Some(4)));
        assert_eq!(path_link("Cargo.toml", cwd), link("/repo/Cargo.toml", None, None));
        assert_eq!(path_link("/abs/x.py#L7", cwd), link("/abs/x.py", Some(6), None));
    }

    #[test]
    fn ignores_code_that_isnt_a_path() {
        let texts = ["foo()", "cargo test", "https://x.dev/a.md", "inventory", "1.5", "v2.0", "a.b()", "failing-tests"];
        for t in texts.into_iter().chain(["tests/"]) {
            assert_eq!(path_link(t, "/repo"), None, "{t}");
        }
    }

    #[test]
    fn scales_the_unit() {
        assert_eq!(format_duration(850.0), "850ms");
        assert_eq!(format_duration(12_400.0), "12s");
        assert_eq!(format_duration(65_000.0), "1m 05s");
        assert_eq!(format_duration(2.0 * 3_600_000.0 + 3.0 * 60_000.0), "2h 03m");
    }

    #[test]
    fn finds_every_occurrence_ignoring_case() {
        assert_eq!(find_all("Abc abc ABC", "abc"), vec![0, 4, 8]);
        assert!(find_all("aaa", "").is_empty());
    }

    #[test]
    fn maps_matches_to_dom_ranges() {
        // The text runs of `<p>one <code>two</code> one</p>`.
        let runs = ["one ", "two", " one"];
        let found = find_in_runs(&runs, "one");
        let texts: Vec<&str> = found.iter().map(|m| utf16::slice(runs[m.run], m.start, m.end)).collect();
        assert_eq!(texts, vec!["one", "one"]);
        assert_eq!(find_in_runs(&runs, "TWO").len(), 1);
    }
}
