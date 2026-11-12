//! Link detection in terminal lines: URLs and `path:line[:col]` file locations.

use regex::Regex;
use std::sync::OnceLock;

/// What a link opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    Url(String),
    /// A file location; `line` and `col` are 1-based, as the program printed them.
    File {
        path: String,
        line: Option<u32>,
        col: Option<u32>,
    },
}

/// A link found in a line: `text` is `line[start..end]` (byte offsets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalLink {
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub target: LinkTarget,
}

impl TerminalLink {
    pub fn is_url(&self) -> bool {
        matches!(self.target, LinkTarget::Url(_))
    }
}

/// `http(s)://` and `file://` URLs up to the next white space, quote or angle bracket. White
/// space is spelled out so that it is the same set in every regex engine.
fn url_regex() -> &'static Regex {
    static URL: OnceLock<Regex> = OnceLock::new();
    URL.get_or_init(|| {
        Regex::new(concat!(
            r"(?-u:\b)(?:https?|file)://",
            r"[^\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}",
            r#"<>"'`]+"#,
        ))
        .expect("the URL pattern is valid")
    })
}

/// `src/main.rs:10:5`, `./a/b.ts:3`, `/abs/x.py:12`, `foo.go(12,3)`, `Foo.kt:22`.
fn file_regex() -> &'static Regex {
    static FILE: OnceLock<Regex> = OnceLock::new();
    FILE.get_or_init(|| {
        Regex::new(concat!(
            r"((?:~|\.{1,2})?/?(?:[A-Za-z0-9_.@+-]+/)*[A-Za-z0-9_@+-][A-Za-z0-9_.@+-]*\.[A-Za-z][A-Za-z0-9_]{0,9})",
            r"(?::([0-9]+)(?::([0-9]+))?|\(([0-9]+)(?:[,:]([0-9]+))?\))",
        ))
        .expect("the file location pattern is valid")
    })
}

/// Strips trailing punctuation that is almost never part of a URL (keeps balanced parens).
pub fn trim_url(url: &str) -> &str {
    let mut out = url;
    loop {
        let Some(last) = out.chars().next_back() else { return out };
        if ".,;:!?'\"".contains(last) {
            out = &out[..out.len() - last.len_utf8()];
            continue;
        }
        if matches!(last, ')' | ']') {
            let open = if last == ')' { '(' } else { '[' };
            if out.matches(last).count() > out.matches(open).count() {
                out = &out[..out.len() - last.len_utf8()];
                continue;
            }
        }
        return out;
    }
}

/// The links of one line, in the order they appear. A file location inside a URL (a port, a
/// path) is part of the URL, and one glued to a longer word or path is not a location.
pub fn find_terminal_links(text: &str) -> Vec<TerminalLink> {
    let mut links: Vec<TerminalLink> = Vec::new();
    for found in url_regex().find_iter(text) {
        let url = trim_url(found.as_str());
        links.push(TerminalLink {
            start: found.start(),
            end: found.start() + url.len(),
            text: url.to_owned(),
            target: LinkTarget::Url(url.to_owned()),
        });
    }
    for captures in file_regex().captures_iter(text) {
        let (Some(whole), Some(path)) = (captures.get(0), captures.get(1)) else { continue };
        let (start, end) = (whole.start(), whole.end());
        let glued = text[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '/' | '.' | '~' | '-'));
        if glued || links.iter().any(|link| start < link.end && end > link.start) {
            continue;
        }
        let number = |a: usize, b: usize| captures.get(a).or_else(|| captures.get(b))?.as_str().parse::<u32>().ok();
        links.push(TerminalLink {
            start,
            end,
            text: whole.as_str().to_owned(),
            target: LinkTarget::File { path: path.as_str().to_owned(), line: number(2, 4), col: number(3, 5) },
        });
    }
    links.sort_by_key(|link| link.start);
    links
}

/// `C:/…`: a path that starts at a Windows drive.
fn has_drive(path: &str) -> bool {
    matches!(path.as_bytes(), [letter, b':', b'/', ..] if letter.is_ascii_alphabetic())
}

/// Resolves `path` against `cwd` (POSIX, or Windows drive paths); `~` expands with `home` when known.
pub fn resolve_path(path: &str, cwd: &str, home: Option<&str>) -> String {
    let mut p = path.replace('\\', "/");
    if p == "~" || p.starts_with("~/") {
        let Some(home) = home.filter(|home| !home.is_empty()) else { return p };
        p = home.replace('\\', "/") + &p[1..];
    } else if !p.starts_with('/') && !has_drive(&p) {
        p = format!("{}/{p}", cwd.replace('\\', "/").trim_end_matches('/'));
    }
    let mut parts: Vec<&str> = Vec::new();
    for segment in p.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(segment),
        }
    }
    let joined = parts.join("/");
    // A Windows drive prefix is its own root; POSIX results keep the leading slash.
    if has_drive(&joined) { joined } else { format!("/{joined}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(link: &TerminalLink) -> Option<(&str, Option<u32>, Option<u32>)> {
        match &link.target {
            LinkTarget::File { path, line, col } => Some((path.as_str(), *line, *col)),
            LinkTarget::Url(_) => None,
        }
    }

    // src/features/terminal/terminal.test.ts › terminal links › finds URLs and file locations
    #[test]
    fn finds_urls_and_file_locations() {
        let line = "see https://example.com/x). error at src/main.rs:10:5 and ./a/b.ts:3";
        let links = find_terminal_links(line);
        let texts: Vec<&str> = links.iter().map(|link| link.text.as_str()).collect();
        assert_eq!(texts, ["https://example.com/x", "src/main.rs:10:5", "./a/b.ts:3"]);
        assert_eq!(file(&links[1]), Some(("src/main.rs", Some(10), Some(5))));
        assert!(links.iter().all(|link| line[link.start..link.end] == link.text));
    }

    // src/features/terminal/terminal.test.ts › terminal links › supports paren locations and absolute paths
    #[test]
    fn supports_paren_locations_and_absolute_paths() {
        let links = find_terminal_links("/Users/me/p/App.tsx(14,3): error");
        assert_eq!(links.first().and_then(file), Some(("/Users/me/p/App.tsx", Some(14), Some(3))));
    }

    // src/features/terminal/terminal.test.ts › terminal links › ignores plain numbers and URL ports
    #[test]
    fn ignores_plain_numbers_and_url_ports() {
        let links = find_terminal_links("12:30 http://localhost:3000");
        assert_eq!(links.len(), 1);
        assert!(links[0].is_url());
    }

    // src/features/terminal/terminal.test.ts › terminal links › trims trailing punctuation from URLs
    #[test]
    fn trims_trailing_punctuation_from_urls() {
        assert_eq!(trim_url("https://a.dev/x."), "https://a.dev/x");
        assert_eq!(trim_url("https://a.dev/(x)"), "https://a.dev/(x)");
        assert_eq!(trim_url("https://a.dev/x)"), "https://a.dev/x");
    }

    // src/features/terminal/terminal.test.ts › terminal links › resolves paths against the cwd
    #[test]
    fn resolves_paths_against_the_cwd() {
        assert_eq!(resolve_path("src/a.ts", "/p", None), "/p/src/a.ts");
        assert_eq!(resolve_path("../b.ts", "/p/web", None), "/p/b.ts");
        assert_eq!(resolve_path("/abs/c.ts", "/p", None), "/abs/c.ts");
        assert_eq!(resolve_path("~/d.ts", "/p", Some("/Users/me")), "/Users/me/d.ts");
    }

    // src/features/terminal/terminal.test.ts › terminal links › resolves paths against a Windows cwd (#705)
    #[test]
    fn resolves_paths_against_a_windows_cwd() {
        assert_eq!(resolve_path("a/main.php", r"C:\Users\me\proj", None), "C:/Users/me/proj/a/main.php");
        assert_eq!(resolve_path(r"a\main.php", "C:/Users/me/proj", None), "C:/Users/me/proj/a/main.php");
        assert_eq!(resolve_path(r"C:\abs\d.ts", "C:/p", None), "C:/abs/d.ts");
        assert_eq!(resolve_path("~/e.ts", "C:/p", Some(r"C:\Users\me")), "C:/Users/me/e.ts");
        assert_eq!(resolve_path("../f.ts", "C:/p/web", None), "C:/p/f.ts");
    }
}
