//! References Go to Declaration and ⌘-click follow where the language server has no definition,
//! in every language: the server's document links (HTML `href`, CSS `url()`, Markdown links,
//! `#include`, JSON `$schema`…), else URLs in the text and quoted strings naming an existing file
//! (`open("data/x.csv")`, `include_str!("a.txt")`). Offsets are UTF-16.

use super::markdown::percent_decode;
use super::positions::range_to_offsets;
use crate::editor::buffer;
use ide_core::ipc_path::to_ipc;
use ide_core::{Pos, is_virtual_uri};
use ide_lsp::types::DocumentLink;
use regex::Regex;
use ropey::Rope;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// A reference in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reference {
    File {
        from: u32,
        to: u32,
        path: String,
        fragment: Option<String>,
    },
    Url {
        from: u32,
        to: u32,
        url: String,
    },
    /// A quoted path still to be resolved against the file system.
    Path {
        from: u32,
        to: u32,
        value: String,
    },
    /// A folder: revealed in the project view.
    Folder {
        from: u32,
        to: u32,
        path: String,
    },
    /// A link whose file does not exist: reported, never opened.
    Missing {
        from: u32,
        to: u32,
        value: String,
    },
}

/// The server link around `offset`.
pub fn link_reference(doc: &Rope, links: &[DocumentLink], offset: u32) -> Option<Reference> {
    links.iter().find_map(|link| {
        let range = range_to_offsets(doc, link.range);
        if offset < range.start || offset > range.end {
            return None;
        }
        if let Some(path) = &link.path {
            return Some(Reference::File {
                from: range.start,
                to: range.end,
                path: to_ipc(path),
                fragment: link.fragment.clone(),
            });
        }
        link.url.as_ref().map(|url| Reference::Url { from: range.start, to: range.end, url: url.clone() })
    })
}

static URL: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"\b(?:https?://|mailto:)[^\s"'`<>()\[\]{}]+[^\s"'`<>()\[\]{},.;:!?]"#));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| pattern(r#""([^"\\\n]*)"|'([^'\\\n]*)'|`([^`\\\n]*)`"#));
/// Unquoted targets: Markdown `[x](path)` / `![x](path "title")`, CSS `url(path)`.
static BARE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"\]\(([^()\s]+)(?:\s+"[^"]*")?\)|\burl\(\s*([^()'"\s]+)\s*\)"#));
const MAX_PATH: usize = 260;

/// Whether a string literal looks like a file path rather than prose or an identifier.
pub fn looks_like_path(value: &str) -> bool {
    static SCHEME: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^[a-z][A-Za-z0-9_+.\-]*:"));
    static FORBIDDEN: LazyLock<Regex> = LazyLock::new(|| pattern(r"[\s<>|*?]"));
    static FILE_NAME: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^\.{0,2}[A-Za-z0-9_@~\-][A-Za-z0-9_.@~\-]*\.[A-Za-z0-9]{1,8}$"));
    if value.is_empty() || utf16_len(value) > MAX_PATH || SCHEME.is_match(value) || FORBIDDEN.is_match(value) {
        return false;
    }
    let bare = value.split(['?', '#']).next().unwrap_or_default();
    bare.contains('/') || FILE_NAME.is_match(bare)
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// A URL or path-like string at `offset`, found on its line.
pub fn text_reference(doc: &Rope, offset: u32) -> Option<Reference> {
    static HREF_BEFORE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)\bhref\s*=\s*$"));
    let (line, col) = buffer::line_col(doc, offset);
    let start = buffer::line_range(doc, line).start;
    let text = buffer::line_text(doc, line);
    let col_of = |byte: usize| buffer::col_of_byte(&text, byte);
    for found in URL.find_iter(&text) {
        let (from, to) = (col_of(found.start()), col_of(found.end()));
        if col >= from && col <= to {
            return Some(Reference::Url { from: start + from, to: start + to, url: found.as_str().to_owned() });
        }
    }
    // (value, its start in bytes, whether a bare `#fragment` counts)
    let mut strings: Vec<(&str, usize, bool)> = Vec::new();
    for found in QUOTED.captures_iter(&text) {
        let Some(value) = found.get(1).or_else(|| found.get(2)).or_else(|| found.get(3)) else { continue };
        let whole = found.get(0).map_or(0, |m| m.start());
        strings.push((value.as_str(), value.start(), HREF_BEFORE.is_match(&text[..whole])));
    }
    for found in BARE.captures_iter(&text) {
        if let Some(value) = found.get(1) {
            strings.push((value.as_str(), value.start(), true));
        } else if let Some(value) = found.get(2) {
            strings.push((value.as_str(), value.start(), false));
        }
    }
    for (value, byte, fragment) in strings {
        let from = col_of(byte);
        let to = from + utf16_len(value) as u32;
        let fragment_link =
            fragment && value.len() > 1 && value.starts_with('#') && !value.contains(char::is_whitespace);
        if col < from || col > to || (!looks_like_path(value) && !fragment_link) {
            continue;
        }
        return Some(Reference::Path { from: start + from, to: start + to, value: value.to_owned() });
    }
    None
}

/// How a `Path` reference resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Done(Option<Reference>),
    /// Ask the backend to resolve `value` like a file reference of the file at `from`
    /// (`fsResolveReference`), and whether the result is a file; then [`resolved_path`].
    Ask {
        from: String,
        value: String,
    },
}

/// Resolves a `Path` reference: a local `#fragment` at once, anything else in the backend.
pub fn resolve_path(reference: Reference, from: &str) -> Resolution {
    let Reference::Path { from: start, to, value } = &reference else { return Resolution::Done(Some(reference)) };
    if let Some(fragment) = value.strip_prefix('#') {
        let decoded = decode_component(fragment);
        return Resolution::Done(decoded.map(|fragment| Reference::File {
            from: *start,
            to: *to,
            path: from.to_owned(),
            fragment: Some(fragment),
        }));
    }
    Resolution::Ask { from: from.to_owned(), value: value.clone() }
}

/// `decodeURIComponent`: `None` for a malformed escape.
fn decode_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let malformed = bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'%' && !(text.get(i + 1..i + 3).is_some_and(|hex| u8::from_str_radix(hex, 16).is_ok())));
    if malformed {
        return None;
    }
    let decoded = percent_decode(text);
    (!decoded.contains('\u{fffd}') || text.contains('\u{fffd}')).then_some(decoded)
}

/// The backend's answer for a `Path` reference: what exists there, if anything.
pub fn resolved_path(reference: &Reference, resolved: Option<String>, is_file: bool) -> Option<Reference> {
    let Reference::Path { from, to, value } = reference else { return Some(reference.clone()) };
    let path = resolved?;
    Some(if is_file {
        Reference::File { from: *from, to: *to, path, fragment: value.split_once('#').map(|(_, f)| f.to_owned()) }
    } else {
        Reference::Folder { from: *from, to: *to, path }
    })
}

/// What finding the reference at an offset does next, given the server link there and the
/// reference in the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceStep {
    Done(Option<Reference>),
    /// Resolve the text's path ([`resolve_path`]).
    Resolve(Reference),
    /// The link names a file: use it if it exists (a virtual document always does), else the
    /// text's own path resolved, else report it [`missing`].
    CheckLink {
        link: Reference,
        own: Option<Reference>,
    },
}

pub fn reference_step(link: Option<Reference>, text: Option<Reference>, virtual_doc: bool) -> ReferenceStep {
    match link {
        Some(link @ Reference::File { .. }) => {
            if let Reference::File { path, .. } = &link
                && is_virtual_uri(path)
            {
                return ReferenceStep::Done(Some(link));
            }
            ReferenceStep::CheckLink { link, own: text.filter(|_| !virtual_doc) }
        }
        Some(link) => ReferenceStep::Done(Some(link)),
        None => match text {
            Some(url @ Reference::Url { .. }) => ReferenceStep::Done(Some(url)),
            Some(text) if !virtual_doc => ReferenceStep::Resolve(text),
            _ => ReferenceStep::Done(None),
        },
    }
}

/// A link to a file that does not exist, shown as its text.
pub fn missing(link: &Reference, doc: &Rope) -> Option<Reference> {
    let Reference::File { from, to, .. } = link else { return None };
    Some(Reference::Missing { from: *from, to: *to, value: buffer::slice(doc, *from, *to) })
}

/// What following a reference does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Follow {
    /// Show this (English) error.
    Error(String),
    /// Select the folder in the project view.
    RevealFolder(String),
    OpenUrl(String),
    /// Read the file (a virtual document's content from its server) to place the caret at the
    /// fragment with [`fragment_position`], then open it.
    ReadForFragment {
        path: String,
        virtual_doc: bool,
        fragment: String,
    },
    Open(String),
}

pub fn follow_reference(reference: &Reference) -> Option<Follow> {
    Some(match reference {
        Reference::Missing { value, .. } => Follow::Error(format!("Cannot find file '{value}'")),
        Reference::Folder { path, .. } => Follow::RevealFolder(path.clone()),
        Reference::Url { url, .. } => Follow::OpenUrl(url.clone()),
        Reference::File { path, fragment: Some(fragment), .. } if !fragment.is_empty() => Follow::ReadForFragment {
            path: path.clone(),
            virtual_doc: is_virtual_uri(path),
            fragment: fragment.clone(),
        },
        Reference::File { path, .. } => Follow::Open(path.clone()),
        Reference::Path { .. } => return None,
    })
}

fn slug(text: &str) -> String {
    let kept: String = text
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphabetic() || c.is_numeric() || c.is_whitespace() || *c == '-')
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join("-")
}

/// The UTF-16 offset a `#fragment` names in `text` of `path`: `L12[,5]`, an element id or name, or
/// a Markdown heading.
pub fn fragment_offset(text: &str, path: &str, fragment: &str) -> Option<u32> {
    static LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^L([0-9]+)(?:[,:C]([0-9]+))?$"));
    static MARKDOWN: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)\.(md|markdown|mdx)$"));
    static HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^#{1,6}\s+(.*?)\s*#*$"));
    let lines: Vec<&str> = text.split('\n').collect();
    let line_offset = |n: usize| lines[..n].iter().map(|line| utf16_len(line) as u32 + 1).sum::<u32>();
    if let Some(found) = LINE.captures(fragment) {
        let number: usize = found.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
        let n = number.checked_sub(1)?.min(lines.len() - 1);
        let col: usize = found.get(2).and_then(|m| m.as_str().parse().ok()).unwrap_or(1);
        let col = col.saturating_sub(1).min(utf16_len(lines[n]));
        return Some(line_offset(n) + col as u32);
    }
    if MARKDOWN.is_match(path) {
        let wanted = fragment.to_lowercase();
        for (n, line) in lines.iter().enumerate() {
            if HEADING.captures(line).and_then(|found| found.get(1)).is_some_and(|title| slug(title.as_str()) == wanted)
            {
                return Some(line_offset(n));
            }
        }
    }
    let attribute = pattern(&format!(r#"\b(?:id|name)\s*=\s*["']?{}"#, regex::escape(fragment)));
    let ends_value = |c: char| c == '"' || c == '\'' || c == '>' || c.is_whitespace();
    attribute
        .find_iter(text)
        .find(|found| text[found.end()..].chars().next().is_some_and(ends_value))
        .map(|found| buffer::col_of_byte(text, found.start()))
}

/// Where the caret goes for a fragment of `text`.
pub fn fragment_position(text: &str, path: &str, fragment: &str) -> Option<Pos> {
    let offset = fragment_offset(text, path, fragment)?;
    let before = &text[..buffer::byte_of_col(text, offset)];
    let line = before.matches('\n').count() as u32;
    let col = utf16_len(before.rsplit('\n').next().unwrap_or_default()) as u32;
    Some(Pos::new(line, col))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_core::Range;

    /// The document of `source` without its `|`, and the offset of the `|`.
    fn at(source: &str) -> (Rope, u32) {
        let byte = source.find('|').expect("the source marks the offset");
        (Rope::from_str(&source.replacen('|', "", 1)), utf16_len(&source[..byte]) as u32)
    }

    fn value_at(source: &str) -> Option<String> {
        let (doc, offset) = at(source);
        match text_reference(&doc, offset)? {
            Reference::Path { value, .. } => Some(value),
            Reference::Url { url, .. } => Some(url),
            _ => None,
        }
    }

    #[test]
    fn finds_urls_trimming_trailing_punctuation() {
        let (doc, offset) = at("// see https://exa|mple.com/a?b=1.");
        let found = text_reference(&doc, offset);
        assert!(matches!(found, Some(Reference::Url { url, .. }) if url == "https://example.com/a?b=1"));
    }

    #[test]
    fn finds_quoted_paths_in_any_language() {
        assert_eq!(value_at("df = open(\"data/x|.csv\")").as_deref(), Some("data/x.csv"));
        assert_eq!(value_at("<link href='sty|le.css'>").as_deref(), Some("style.css"));
    }

    #[test]
    fn finds_markdown_link_targets_and_unquoted_css_urls() {
        assert_eq!(value_at("See [theme](css/the|me.css#L1) now").as_deref(), Some("css/theme.css#L1"));
        assert_eq!(value_at(".a { background: url(img/lo|go.svg); }").as_deref(), Some("img/logo.svg"));
    }

    #[test]
    fn finds_fragment_only_link_targets() {
        for source in ["[artist::order](#Artist-or|der)", "<a href=\"#Artist-or|der\">order</a>"] {
            assert_eq!(value_at(source).as_deref(), Some("#Artist-order"), "{source}");
        }
    }

    #[test]
    fn does_not_treat_arbitrary_fragment_strings_as_links() {
        for source in ["const color = \"#ff|f\"", "<a id=\"Artist-or|der\">", "url(#ic|on)"] {
            assert_eq!(value_at(source), None, "{source}");
        }
    }

    #[test]
    fn ignores_prose_and_identifiers() {
        assert!(!looks_like_path("hello world.txt"));
        assert!(!looks_like_path("utf-8"));
        assert!(!looks_like_path("main"));
        assert!(!looks_like_path("http://x"));
        assert!(looks_like_path("../a/b"));
        assert!(looks_like_path("logo.svg#icon"));
        assert_eq!(value_at("log(\"do|ne\")"), None);
    }

    #[test]
    fn resolves_local_fragments_without_probing_the_filesystem() {
        let reference = Reference::Path { from: 5, to: 20, value: "#Artist%2Dorder".into() };
        assert_eq!(
            resolve_path(reference, "/p/notes.md"),
            Resolution::Done(Some(Reference::File {
                from: 5,
                to: 20,
                path: "/p/notes.md".into(),
                fragment: Some("Artist-order".into()),
            }))
        );
        let bad = Reference::Path { from: 5, to: 20, value: "#bad%".into() };
        assert_eq!(resolve_path(bad, "/p/notes.md"), Resolution::Done(None));
    }

    #[test]
    fn picks_the_server_link_around_the_offset() {
        let doc = Rope::from_str("<a href=\"b.html#top\">");
        let link = DocumentLink {
            range: Range::new(Pos::new(0, 9), Pos::new(0, 19)),
            path: Some("/p/b.html".into()),
            fragment: Some("top".into()),
            url: None,
            tooltip: None,
        };
        assert_eq!(
            link_reference(&doc, std::slice::from_ref(&link), 12),
            Some(Reference::File { from: 9, to: 19, path: "/p/b.html".into(), fragment: Some("top".into()) })
        );
        assert_eq!(link_reference(&doc, &[link], 2), None);
    }

    #[test]
    fn locates_line_element_id_and_markdown_heading_fragments() {
        assert_eq!(fragment_offset("a\nbc\nd", "/x.ts", "L2"), Some(2));
        assert_eq!(fragment_offset("a\nbc\nd", "/x.ts", "L2,2"), Some(3));
        let html = "<p>\n<div class=\"x\" id=\"main\">";
        assert_eq!(fragment_offset(html, "/i.html", "main"), html.find("id=").map(|at| at as u32));
        assert_eq!(fragment_offset("# Intro\n\n## Getting Started!\n", "/r.md", "getting-started"), Some(9));
        for path in ["/r.md", "/r.markdown", "/r.mdx"] {
            for attribute in ["id", "name"] {
                let source = format!("[artist::order](#Artist-order)\n\n<a {attribute}=\"Artist-order\"></a>");
                let expected = source.find(&format!("{attribute}=")).map(|at| at as u32);
                assert_eq!(fragment_offset(&source, path, "Artist-order"), expected, "{path} {attribute}");
            }
        }
        assert_eq!(fragment_offset("x", "/i.html", "nope"), None);
    }

    #[test]
    fn opens_a_container_document_link_in_the_editor_and_resolves_its_fragment_through_virtual_content() {
        let uri = "openidea-target://target/7/usr/lib/library.php";
        let link = Reference::File { from: 0, to: 1, path: uri.into(), fragment: Some("L2".into()) };
        assert_eq!(
            follow_reference(&link),
            Some(Follow::ReadForFragment { path: uri.into(), virtual_doc: true, fragment: "L2".into() })
        );
        let text = "<?php\nfunction library() {}\n";
        assert_eq!(fragment_position(text, uri, "L2"), Some(Pos::new(1, 0)));
    }
}
