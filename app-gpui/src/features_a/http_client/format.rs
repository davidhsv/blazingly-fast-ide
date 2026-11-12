//! Response body presentation: the kind of a body (from its content type, else its text) and
//! pretty-printing of JSON, XML and HTML, plus the size and duration labels of a response.

use regex::Regex;
use std::sync::LazyLock;

/// How a response body is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    Json,
    Xml,
    Html,
    Text,
}

static HTML_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^<!doctype html|^<html[\s>]").expect("valid regex"));
static XML_START: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<[A-Za-z][A-Za-z0-9_:.-]*[\s>/]").expect("valid regex"));
static MARKUP_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->|<!\[CDATA\[.*?\]\]>|<[^>]+>|[^<]+|<").expect("valid regex"));
static TAG_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<\s*([A-Za-z0-9_:.-]+)").expect("valid regex"));
static IS_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^<[!?/A-Za-z0-9_]").expect("valid regex"));

const VOID_HTML: [&str; 14] =
    ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// The kind of a body: the content type decides when it names one, else the text's start.
pub fn body_kind(content_type: Option<&str>, body: &str) -> BodyKind {
    let ct = content_type.unwrap_or_default().to_lowercase();
    if ct.contains("json") {
        return BodyKind::Json;
    }
    if ct.contains("html") {
        return BodyKind::Html;
    }
    if ct.contains("xml") {
        return BodyKind::Xml;
    }
    let t = body.trim_start();
    if t.starts_with('{') || t.starts_with('[') {
        return if serde_json::from_str::<serde_json::Value>(body).is_ok() { BodyKind::Json } else { BodyKind::Text };
    }
    if HTML_START.is_match(t) {
        return BodyKind::Html;
    }
    if t.starts_with("<?xml") || XML_START.is_match(t) {
        return BodyKind::Xml;
    }
    BodyKind::Text
}

/// `body` re-serialized with two-space indentation; invalid JSON is returned as it is.
pub fn pretty_json(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| body.to_owned())
}

fn tag_name(token: &str) -> Option<String> {
    TAG_NAME.captures(token).and_then(|c| c.get(1)).map(|m| m.as_str().to_ascii_lowercase())
}

fn is_raw(name: &str) -> bool {
    matches!(name, "script" | "style" | "pre")
}

fn markup_tokens(body: &str, html: bool) -> Vec<&str> {
    let lower = body.to_ascii_lowercase();
    let mut tokens = Vec::new();
    let mut at = 0;
    while let Some(m) = MARKUP_TOKEN.find_at(body, at) {
        let token = m.as_str();
        if html
            && let Some(name) = tag_name(token)
            && is_raw(&name)
            && !token.ends_with("/>")
        {
            // A raw element is one token up to and including its closing tag.
            let end = match lower[m.end()..].find(&format!("</{name}")) {
                None => body.len(),
                Some(close) => body[m.end() + close..].find('>').map_or(body.len(), |gt| m.end() + close + gt + 1),
            };
            tokens.push(&body[m.start()..end]);
            at = end;
            continue;
        }
        tokens.push(token);
        at = m.end();
    }
    tokens
}

/// Re-indents XML / HTML by tag nesting. Text-only elements stay on one line; `<script>`,
/// `<style>` and `<pre>` contents and comments are kept as written.
pub fn pretty_markup(body: &str, html: bool) -> String {
    const INDENT: &str = "  ";
    let tokens = markup_tokens(body, html);
    if tokens.is_empty() {
        return body.to_owned();
    }
    let is_tag = |t: Option<&&str>| t.is_some_and(|t| IS_TAG.is_match(t));
    let mut out: Vec<String> = Vec::new();
    let mut depth: i64 = 0;
    let pad = |depth: i64| INDENT.repeat(usize::try_from(depth.max(0)).unwrap_or_default());
    let mut i = 0;
    while i < tokens.len() {
        let tok = tokens[i];
        i += 1;
        if !is_tag(Some(&tok)) {
            let text = tok.trim();
            if !text.is_empty() {
                out.push(pad(depth) + text);
            }
            continue;
        }
        if tok.starts_with("<!") || tok.starts_with("<?") {
            out.push(pad(depth) + tok);
            continue;
        }
        if tok.starts_with("</") {
            depth -= 1;
            out.push(pad(depth) + tok);
            continue;
        }
        let name = tag_name(tok).unwrap_or_default();
        if (html && is_raw(&name)) || tok.ends_with("/>") || (html && VOID_HTML.contains(&name.as_str())) {
            out.push(pad(depth) + tok);
            continue;
        }
        let next = tokens.get(i);
        let after = tokens.get(i + 1);
        if let (Some(next), Some(after)) = (next, after)
            && !is_tag(Some(next))
            && after.starts_with("</")
        {
            out.push(format!("{}{tok}{}{after}", pad(depth), next.trim()));
            i += 2;
            continue;
        }
        if let Some(next) = next
            && next.starts_with("</")
        {
            out.push(format!("{}{tok}{next}", pad(depth)));
            i += 1;
            continue;
        }
        out.push(pad(depth) + tok);
        depth += 1;
    }
    out.join("\n")
}

/// The body as shown for its kind.
pub fn pretty_body(kind: BodyKind, body: &str) -> String {
    match kind {
        BodyKind::Json => pretty_json(body),
        BodyKind::Xml => pretty_markup(body, false),
        BodyKind::Html => pretty_markup(body, true),
        BodyKind::Text => body.to_owned(),
    }
}

/// `512 B`, `2.0 KB`, `1.5 MB`.
pub fn format_size(value: Option<u64>) -> String {
    let bytes = value.unwrap_or_default();
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let bytes = bytes as f64;
    if bytes < 1024.0 * 1024.0 {
        return format!("{:.1} KB", bytes / 1024.0);
    }
    format!("{:.1} MB", bytes / 1024.0 / 1024.0)
}

/// `12 ms` below a second, else `1.50 s`.
pub fn format_ms(value: Option<f64>) -> String {
    let ms = value.unwrap_or_default();
    if ms < 1000.0 { format!("{} ms", ms.round()) } else { format!("{:.2} s", ms / 1000.0) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_kind_from_the_content_type_else_the_content() {
        assert_eq!(body_kind(Some("application/json; charset=utf-8"), "x"), BodyKind::Json);
        assert_eq!(body_kind(Some("application/problem+json"), "x"), BodyKind::Json);
        assert_eq!(body_kind(Some("text/html"), ""), BodyKind::Html);
        assert_eq!(body_kind(Some("application/xml"), ""), BodyKind::Xml);
        assert_eq!(body_kind(None, " {\"a\":1}"), BodyKind::Json);
        assert_eq!(body_kind(None, "{not json"), BodyKind::Text);
        assert_eq!(body_kind(None, "<!DOCTYPE html><html></html>"), BodyKind::Html);
        assert_eq!(body_kind(None, "<?xml version='1.0'?><a/>"), BodyKind::Xml);
        assert_eq!(body_kind(None, "plain"), BodyKind::Text);
    }

    #[test]
    fn pretty_prints_json_and_leaves_invalid_json_alone() {
        assert_eq!(
            pretty_body(BodyKind::Json, r#"{"a":[1,2],"b":null}"#),
            "{\n  \"a\": [\n    1,\n    2\n  ],\n  \"b\": null\n}"
        );
        assert_eq!(pretty_body(BodyKind::Json, "{oops"), "{oops");
    }

    #[test]
    fn re_indents_xml_keeping_text_only_elements_on_one_line() {
        assert_eq!(
            pretty_markup(r#"<?xml version="1.0"?><a><b>1</b><c/><d><e>x</e></d><f></f></a>"#, false),
            [
                r#"<?xml version="1.0"?>"#,
                "<a>",
                "  <b>1</b>",
                "  <c/>",
                "  <d>",
                "    <e>x</e>",
                "  </d>",
                "  <f></f>",
                "</a>",
            ]
            .join("\n")
        );
    }

    #[test]
    fn re_indents_html_with_void_elements_and_raw_script_contents() {
        let html = "<html><head><meta charset=utf-8><script>if (a < b) { x(); }</script></head><body><p>Hi</p><br>\
                    </body></html>";
        assert_eq!(
            pretty_markup(html, true),
            [
                "<html>",
                "  <head>",
                "    <meta charset=utf-8>",
                "    <script>if (a < b) { x(); }</script>",
                "  </head>",
                "  <body>",
                "    <p>Hi</p>",
                "    <br>",
                "  </body>",
                "</html>",
            ]
            .join("\n")
        );
    }

    #[test]
    fn formats_sizes_and_durations() {
        assert_eq!(format_size(Some(512)), "512 B");
        assert_eq!(format_size(Some(2048)), "2.0 KB");
        assert_eq!(format_size(None), "0 B");
        assert_eq!(format_ms(Some(12.4)), "12 ms");
        assert_eq!(format_ms(Some(1500.0)), "1.50 s");
    }
}
