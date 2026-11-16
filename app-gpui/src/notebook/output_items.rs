//! What a code cell's outputs show, decided from the notebook's data alone.
//!
//! A notebook comes from anywhere, so nothing in it runs, loads or navigates: streams, errors and
//! plain results are text with their ANSI styles; a PNG or JPEG is decoded from the base64 bytes
//! in the file; HTML is shown as its text (scripts, styles, frames, forms and every URL left out),
//! JavaScript as inert source under a notice; SVG, whose references a decoder could follow, shows
//! the bundle's plain text instead. A type the editor does not show says so and keeps the data.

use super::ansi::{AnsiRun, ansi_runs};
use crate::features_a::notebook::outputs::{mime_text, object};
use crate::features_b::viewers::hex::decode_base64;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

/// How much of an output's text is shown; the rest stays in the notebook.
pub const PREVIEW_LIMIT: usize = 200_000;

/// A raster image format the editor decodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RasterFormat {
    Png,
    Jpeg,
}

/// One thing an output shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputItem {
    /// Text with its ANSI styles; `error` draws it in the error colour; `truncated` when only the
    /// first [`PREVIEW_LIMIT`] characters are shown.
    Text { runs: Vec<AnsiRun>, error: bool, truncated: bool },
    /// An image decoded from the notebook's own bytes, with its alternative text.
    Image { format: RasterFormat, base64: String, alt: String },
    /// Static HTML as its text.
    Html { text: String },
    /// JavaScript, never run: the notice and its source.
    InertJavaScript { source: String },
    /// An SVG image, not drawn: its alternative text.
    Svg { alt: String },
    /// A notice with what it is about (the MIME types, the output type).
    Notice { text: &'static str, detail: String },
}

fn text_item(text: &str, error: bool) -> OutputItem {
    let truncated = text.chars().count() > PREVIEW_LIMIT;
    let shown: String = if truncated { text.chars().take(PREVIEW_LIMIT).collect() } else { text.to_owned() };
    OutputItem::Text { runs: ansi_runs(&shown), error, truncated }
}

static BASE64: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z0-9+/=]+$").expect("valid regex"));

/// The raster image of a bundle: PNG, then JPEG, valid base64 only.
pub fn raster(bundle: &Map<String, Value>) -> Option<(RasterFormat, String)> {
    for (mime, format) in [("image/png", RasterFormat::Png), ("image/jpeg", RasterFormat::Jpeg)] {
        let value: String = mime_text(bundle.get(mime)).chars().filter(|c| !c.is_whitespace()).collect();
        if !value.is_empty() && BASE64.is_match(&value) {
            return Some((format, value));
        }
    }
    None
}

/// The bytes of a base64 image; `None` when they do not decode.
pub fn image_bytes(base64: &str) -> Option<Vec<u8>> {
    decode_base64(base64)
}

/// Elements dropped with everything inside them.
const DROPPED_CONTENT: [&str; 12] = [
    "script", "style", "iframe", "object", "embed", "noscript", "template", "svg", "math", "head", "title", "textarea",
];

/// The text an HTML fragment shows: no element, attribute or URL of it survives. Block elements
/// and table cells keep their text apart.
pub fn html_text(html: &str) -> String {
    static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?(?:-->|$)").expect("valid regex"));
    static TAG: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"<(/?)([a-zA-Z][\w-]*)(?:"[^"]*"|'[^']*'|[^'">])*>"#).expect("valid regex"));
    let text = COMMENT.replace_all(html, "");
    let mut out = String::new();
    let mut at = 0;
    let lower = text.to_ascii_lowercase();
    while let Some(found) = TAG.captures_at(&text, at) {
        let Some(whole) = found.get(0) else { break };
        out.push_str(&text[at..whole.start()]);
        at = whole.end();
        let closing = found.get(1).is_some_and(|m| !m.as_str().is_empty());
        let name = found.get(2).map_or("", |m| m.as_str()).to_ascii_lowercase();
        if !closing && DROPPED_CONTENT.contains(&name.as_str()) {
            let close = format!("</{name}");
            at = match lower[at..].find(&close) {
                Some(found) => {
                    let from = at + found;
                    lower[from..].find('>').map_or(text.len(), |gt| from + gt + 1)
                }
                None => text.len(),
            };
            continue;
        }
        match name.as_str() {
            "br" | "p" | "div" | "tr" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "table" | "pre"
            | "blockquote" => {
                if !out.ends_with('\n') && !out.is_empty() {
                    out.push('\n');
                }
            }
            "td" | "th" if closing => out.push('\t'),
            _ => {}
        }
    }
    out.push_str(&text[at..]);
    let decoded = decode_entities(&out);
    decoded.lines().map(|line| line.trim_end_matches(['\t', ' '])).collect::<Vec<_>>().join("\n").trim().to_owned()
}

/// HTML character references decoded: numeric ones and the common named ones.
pub fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let end = after.find(';').filter(|end| *end <= 10);
        let decoded = end.and_then(|end| {
            let name = &after[..end];
            let code = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(decimal) = name.strip_prefix('#') {
                decimal.parse::<u32>().ok()
            } else {
                match name {
                    "amp" => Some(u32::from('&')),
                    "lt" => Some(u32::from('<')),
                    "gt" => Some(u32::from('>')),
                    "quot" => Some(u32::from('"')),
                    "apos" => Some(u32::from('\'')),
                    "nbsp" => Some(0xa0),
                    "colon" => Some(u32::from(':')),
                    _ => None,
                }
            };
            code.and_then(char::from_u32).map(|c| (c, end))
        });
        match decoded {
            Some((c, end)) => {
                out.push(c);
                rest = &after[end + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// What a MIME bundle (`display_data`, `execute_result`) shows: the first of image, HTML and plain
/// text it has, then inert JavaScript, or a notice naming its types.
fn bundle_items(bundle: &Map<String, Value>, out: &mut Vec<OutputItem>) {
    let plain = mime_text(bundle.get("text/plain"));
    let html = mime_text(bundle.get("text/html"));
    let svg = mime_text(bundle.get("image/svg+xml"));
    let javascript = Some(mime_text(bundle.get("application/javascript")))
        .filter(|js| !js.is_empty())
        .unwrap_or_else(|| mime_text(bundle.get("text/javascript")));
    let image = raster(bundle);
    let alt = if plain.is_empty() { "Notebook image output".to_owned() } else { plain.clone() };
    let supported = image.is_some() || !svg.is_empty() || !html.is_empty() || !plain.is_empty();
    if let Some((format, base64)) = image {
        out.push(OutputItem::Image { format, base64, alt });
    } else if !svg.is_empty() {
        out.push(OutputItem::Svg { alt });
    } else if !html.is_empty() {
        out.push(OutputItem::Html { text: html_text(&html) });
    } else if !plain.is_empty() {
        out.push(text_item(&plain, false));
    }
    if !javascript.is_empty() {
        out.push(OutputItem::InertJavaScript { source: javascript });
    }
    if !supported && javascript.is_empty() {
        let types: Vec<&str> = bundle.keys().map(String::as_str).collect();
        out.push(OutputItem::Notice {
            text: "Unsupported output MIME type; original data is preserved.",
            detail: types.join(", "),
        });
    }
}

/// What the outputs of a code cell show, in order.
pub fn output_items(outputs: &[Value]) -> Vec<OutputItem> {
    let mut items = Vec::new();
    for value in outputs {
        let output = object(value);
        match output.get("output_type").and_then(Value::as_str) {
            Some("stream") => {
                let error = output.get("name").and_then(Value::as_str) == Some("stderr");
                items.push(text_item(&mime_text(output.get("text")), error));
            }
            Some("error") => {
                let text = match output.get("traceback") {
                    Some(Value::Array(lines)) => {
                        lines.iter().map(|line| mime_text(Some(line))).collect::<Vec<_>>().join("\n")
                    }
                    _ => format!("{}: {}", mime_text(output.get("ename")), mime_text(output.get("evalue"))),
                };
                items.push(text_item(&text, true));
            }
            Some("display_data" | "execute_result") => {
                bundle_items(&object(output.get("data").unwrap_or(&Value::Null)), &mut items);
            }
            other => items.push(OutputItem::Notice {
                text: "Unsupported output type; original data is preserved.",
                detail: match other {
                    Some(kind) => kind.to_owned(),
                    None => match output.get("output_type") {
                        Some(Value::Null) | None => "unknown".to_owned(),
                        Some(value) => value.to_string(),
                    },
                },
            }),
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notebook::ansi::plain_text;
    use serde_json::json;

    fn texts(items: &[OutputItem]) -> Vec<(String, bool)> {
        items
            .iter()
            .filter_map(|item| match item {
                OutputItem::Text { runs, error, .. } => Some((plain_text(runs), *error)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn streams_errors_and_plain_results_are_text_and_stderr_and_tracebacks_are_errors() {
        let outputs = [
            json!({ "output_type": "stream", "name": "stdout", "text": ["a\n", "b\n"] }),
            json!({ "output_type": "stream", "name": "stderr", "text": "warn\n" }),
            json!({ "output_type": "error", "ename": "E", "evalue": "v", "traceback": ["\u{1b}[31mE\u{1b}[0m", "v"] }),
            json!({ "output_type": "error", "ename": "KeyError", "evalue": "'k'" }),
            json!({ "output_type": "execute_result", "data": { "text/plain": "42" }, "metadata": {} }),
        ];
        assert_eq!(
            texts(&output_items(&outputs)),
            [
                ("a\nb\n".to_owned(), false),
                ("warn\n".to_owned(), true),
                ("E\nv".to_owned(), true),
                ("KeyError: 'k'".to_owned(), true),
                ("42".to_owned(), false)
            ]
        );
    }

    #[test]
    fn a_long_output_shows_its_first_part_and_says_it_is_truncated() {
        let long = "x".repeat(PREVIEW_LIMIT + 5);
        let items = output_items(&[json!({ "output_type": "stream", "name": "stdout", "text": long })]);
        let OutputItem::Text { runs, truncated, .. } = &items[0] else { panic!("text") };
        assert!(*truncated);
        assert_eq!(plain_text(runs).len(), PREVIEW_LIMIT);
    }

    #[test]
    fn images_come_from_embedded_base64_only_and_svg_html_and_javascript_stay_inert() {
        let png =
            json!({ "output_type": "display_data", "data": { "image/png": "iVBO\nRw0KGgo=", "text/plain": "fig" } });
        let svg = json!({ "output_type": "display_data", "data": { "image/svg+xml": "<svg><image href='https://x.invalid/a.png'/></svg>" } });
        let html = json!({ "output_type": "display_data", "data": {
            "text/html": "<script>alert(1)</script><table><tr><td>alpha</td><td>42</td></tr></table><a href='https://x.invalid'>link</a>"
        } });
        let js = json!({ "output_type": "display_data", "data": { "application/javascript": "fetch('https://x.invalid')" } });
        let items = output_items(&[png, svg, html, js]);
        assert_eq!(
            items[0],
            OutputItem::Image { format: RasterFormat::Png, base64: "iVBORw0KGgo=".to_owned(), alt: "fig".to_owned() }
        );
        assert_eq!(items[1], OutputItem::Svg { alt: "Notebook image output".to_owned() });
        assert_eq!(items[2], OutputItem::Html { text: "alpha\t42\nlink".to_owned() });
        assert_eq!(items[3], OutputItem::InertJavaScript { source: "fetch('https://x.invalid')".to_owned() });
    }

    #[test]
    fn unsupported_types_say_so_and_name_what_they_hold() {
        let items = output_items(&[
            json!({ "output_type": "display_data", "data": { "application/vnd.custom+json": {} } }),
            json!({ "output_type": "future_kind" }),
            json!({}),
        ]);
        assert_eq!(
            items,
            [
                OutputItem::Notice {
                    text: "Unsupported output MIME type; original data is preserved.",
                    detail: "application/vnd.custom+json".to_owned()
                },
                OutputItem::Notice {
                    text: "Unsupported output type; original data is preserved.",
                    detail: "future_kind".to_owned()
                },
                OutputItem::Notice {
                    text: "Unsupported output type; original data is preserved.",
                    detail: "unknown".to_owned()
                },
            ]
        );
    }

    #[test]
    fn entities_decode_and_unknown_ones_stay() {
        assert_eq!(decode_entities("attachment&#58;a&#x3A;b &amp; &bogus; &"), "attachment:a:b & &bogus; &");
    }
}
