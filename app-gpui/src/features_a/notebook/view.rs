//! What the notebook's output and Markdown views resolve before drawing: image MIME bundles and
//! `attachment:` images of Markdown cells. Only embedded images resolve; an SVG goes through the
//! sanitizer of the view that draws it.

use super::outputs::mime_text;
use regex::Regex;
use serde_json::{Map, Value};
use std::sync::LazyLock;

static BASE64: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z0-9+/=]+$").expect("valid regex"));

/// An image to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputImage {
    /// A `data:` URL of a PNG or JPEG.
    Raster(String),
    /// SVG source, to sanitize before drawing.
    Svg(String),
}

/// The image of a MIME bundle: PNG, then JPEG (valid base64 only), then SVG.
pub fn bundle_image(bundle: &Map<String, Value>) -> Option<OutputImage> {
    for mime in ["image/png", "image/jpeg"] {
        let value: String = mime_text(bundle.get(mime)).chars().filter(|c| !c.is_whitespace()).collect();
        if !value.is_empty() && BASE64.is_match(&value) {
            return Some(OutputImage::Raster(format!("data:{mime};base64,{value}")));
        }
    }
    let svg = mime_text(bundle.get("image/svg+xml"));
    (!svg.is_empty()).then_some(OutputImage::Svg(svg))
}

fn hex(b: u8) -> Option<u8> {
    char::from(b).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

/// `%XX` escapes decoded; `None` for a malformed escape or bytes that are not UTF-8.
fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let high = hex(*bytes.get(i + 1)?)?;
            let low = hex(*bytes.get(i + 2)?)?;
            out.push(high * 16 + low);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// What an image `src` of a Markdown cell names: `None` when it is not an attachment,
/// `Some(None)` when the name cannot be decoded (the image is dropped).
pub fn attachment_name(src: &str) -> Option<Option<String>> {
    src.strip_prefix("attachment:").map(percent_decode)
}

/// The image an attachment resolves to, from the cell's `attachments`. Notebook keys such as
/// `__proto__` or `constructor` name data like any other key.
pub fn attachment_image(attachments: &Value, name: &str) -> Option<OutputImage> {
    let bundle = attachments.as_object()?.get(name)?.as_object()?;
    bundle_image(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const PNG: &str = "iVBORw0KGgo=";

    #[test]
    fn resolves_an_attachment_named_like_an_inherited_member_from_parsed_notebook_json() {
        for name in ["__proto__", "constructor"] {
            let attachments: Value =
                serde_json::from_str(&format!("{{{}:{{\"image/png\":\"{PNG}\"}}}}", json!(name))).expect("valid JSON");
            let src = format!("attachment:{name}");
            let decoded = attachment_name(&src).flatten().expect("an attachment");
            assert_eq!(
                attachment_image(&attachments, &decoded),
                Some(OutputImage::Raster(format!("data:image/png;base64,{PNG}"))),
                "{name}"
            );
        }
    }

    #[test]
    fn does_not_resolve_inherited_members_as_attachments() {
        assert_eq!(attachment_image(&json!({}), "constructor"), None);
    }

    #[test]
    fn decodes_attachment_names_and_drops_malformed_ones() {
        assert_eq!(attachment_name("attachment:a%20b.png"), Some(Some("a b.png".to_owned())));
        assert_eq!(attachment_name("attachment:%E0%A4"), Some(None));
        assert_eq!(attachment_name("https://example.com/a.png"), None);
    }

    #[test]
    fn prefers_embedded_raster_images_and_leaves_svg_to_the_sanitizer() {
        let bundle = |v: Value| v.as_object().cloned().unwrap_or_default();
        assert_eq!(
            bundle_image(&bundle(json!({ "image/png": ["iVBO", "Rw0KGgo=\n"], "image/svg+xml": "<svg/>" }))),
            Some(OutputImage::Raster(format!("data:image/png;base64,{PNG}")))
        );
        assert_eq!(
            bundle_image(&bundle(json!({ "image/png": "not base64!", "image/svg+xml": "<svg/>" }))),
            Some(OutputImage::Svg("<svg/>".to_owned()))
        );
        assert_eq!(bundle_image(&bundle(json!({ "text/plain": "x" }))), None);
    }
}
