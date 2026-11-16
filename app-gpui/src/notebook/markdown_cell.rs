//! A Markdown cell as its preview shows it: the blocks of the Markdown renderer, with only images
//! the notebook embeds kept.
//!
//! Host Markdown loads nothing: an `attachment:` image resolves to the cell's own attachment (PNG
//! or JPEG bytes in the file), a `data:` PNG or JPEG stays, and every other image (a remote URL, a
//! local path, an SVG) shows its alternative text. The renderer keeps no style attribute and drops
//! scripts, styles, frames and forms with their content. Links are not followed by the preview:
//! the editor resolves a clicked link and opens a file in the editor or a web page in the
//! system's browser.
//!
//! Full snapshots carry fresh attachment objects; [`MarkdownCache`] rebuilds a preview only when
//! its source or the data of the attachments it references changes, so a reader's place in it
//! survives an unrelated edit.

use super::output_items::decode_entities;
use crate::editor_lsp::markdown::{Run, Style};
use crate::features_a::notebook::outputs::object;
use crate::features_a::notebook::view::{OutputImage, attachment_image, attachment_name};
use crate::features_b::viewers::render::{Image, Inline, ListItem, Node, PreviewBlock, render_markdown};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::LazyLock;

static RASTER_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^data:image/(?:png|jpeg);base64,[a-z0-9+/=\s]+$").expect("valid regex"));

/// What an image source of a Markdown cell names, after HTML character references are decoded:
/// `None` when it is not an attachment, `Some(None)` when its name cannot be decoded.
fn source_attachment(source: &str) -> Option<Option<String>> {
    attachment_name(&decode_entities(source.trim()))
}

/// The `data:` URL an image of the cell shows, or `None` when it shows its alternative text.
fn resolved_source(source: &str, attachments: &Value) -> Option<String> {
    match source_attachment(source) {
        Some(Some(name)) => match attachment_image(attachments, &name)? {
            OutputImage::Raster(url) => Some(url),
            OutputImage::Svg(_) => None,
        },
        Some(None) => None,
        None => {
            let decoded = decode_entities(source.trim());
            RASTER_URL.is_match(&decoded).then_some(decoded)
        }
    }
}

/// The text an image shows instead of itself: its alternative text, else the attachment's name.
fn fallback_text(image: &Image) -> String {
    if !image.alt.is_empty() {
        return image.alt.clone();
    }
    source_attachment(&image.source).flatten().unwrap_or_default()
}

fn resolve_image(image: &Image, attachments: &Value) -> Option<Image> {
    resolved_source(&image.source, attachments).map(|source| Image { source, ..image.clone() })
}

fn plain_run(text: String) -> Inline {
    Inline::Text(Run { text, style: Style::default() })
}

fn resolve_inlines(content: &[Inline], attachments: &Value) -> Vec<Inline> {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Image(image) => match resolve_image(image, attachments) {
                Some(image) => Inline::Image(image),
                None => plain_run(fallback_text(image)),
            },
            Inline::Text(run) => Inline::Text(run.clone()),
        })
        .collect()
}

fn resolve_nodes(nodes: &[Node], attachments: &Value) -> Vec<Node> {
    nodes
        .iter()
        .map(|node| match node {
            Node::Paragraph(content) => Node::Paragraph(resolve_inlines(content, attachments)),
            Node::Heading { level, anchor, content } => {
                Node::Heading { level: *level, anchor: anchor.clone(), content: resolve_inlines(content, attachments) }
            }
            Node::Quote(inner) => Node::Quote(resolve_nodes(inner, attachments)),
            Node::List { ordered, items } => Node::List {
                ordered: *ordered,
                items: items
                    .iter()
                    .map(|item| ListItem { checked: item.checked, content: resolve_nodes(&item.content, attachments) })
                    .collect(),
            },
            Node::Table { header, rows } => Node::Table {
                header: header.iter().map(|cell| resolve_inlines(cell, attachments)).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|cell| resolve_inlines(cell, attachments)).collect())
                    .collect(),
            },
            Node::Image(image) => match resolve_image(image, attachments) {
                Some(image) => Node::Image(image),
                None => Node::Paragraph(vec![plain_run(fallback_text(image))]),
            },
            Node::Element(element) => {
                let mut element = element.clone();
                element.children = resolve_nodes(&element.children, attachments);
                Node::Element(element)
            }
            other => other.clone(),
        })
        .collect()
}

/// The image sources the blocks hold, in document order.
fn sources(nodes: &[Node], out: &mut Vec<String>) {
    let inline = |content: &[Inline], out: &mut Vec<String>| {
        for item in content {
            if let Inline::Image(image) = item {
                out.push(image.source.clone());
            }
        }
    };
    for node in nodes {
        match node {
            Node::Paragraph(content) | Node::Heading { content, .. } => inline(content, out),
            Node::Quote(inner) => sources(inner, out),
            Node::List { items, .. } => items.iter().for_each(|item| sources(&item.content, out)),
            Node::Table { header, rows } => {
                header.iter().chain(rows.iter().flatten()).for_each(|cell| inline(cell, out));
            }
            Node::Image(image) => out.push(image.source.clone()),
            Node::Element(element) => sources(&element.children, out),
            _ => {}
        }
    }
}

/// The attachment names the blocks reference, decoded, each once.
pub fn attachment_names(blocks: &[PreviewBlock]) -> Vec<String> {
    let mut found = Vec::new();
    for block in blocks {
        sources(&block.nodes, &mut found);
    }
    let mut names: Vec<String> = Vec::new();
    for name in found.iter().filter_map(|source| source_attachment(source).flatten()) {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

static DEFINITION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^ {0,3}\[([^\]]+)\]:[ \t]*<?([^\s<>()]+)>?(?:[ \t]+.*)?$").expect("valid regex"));
static REFERENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(!?)\[([^\[\]]*)\]\[([^\[\]]*)\]").expect("valid regex"));

/// Reference labels match case-insensitively with runs of white space collapsed.
fn label_key(label: &str) -> String {
    label.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// The source with full and collapsed references (`![alt][label]`, `[text][]`) written inline and
/// their definitions blanked, as the renderer reads only inline destinations. Lines inside code
/// fences stay as they are; a blanked definition keeps its line, so block lines do not move.
fn inline_references(source: &str) -> String {
    enum Line<'a> {
        Definition,
        Code(&'a str),
        Text(&'a str),
    }
    let mut definitions: HashMap<String, String> = HashMap::new();
    let mut fence: Option<&str> = None;
    let mut lines: Vec<Line> = Vec::new();
    for line in source.split('\n') {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"].into_iter().find(|marker| trimmed.starts_with(marker));
        if fence.is_some() || marker.is_some() {
            match (fence, marker) {
                (Some(open), Some(marker)) if open == marker => fence = None,
                (None, Some(marker)) => fence = Some(marker),
                _ => {}
            }
            lines.push(Line::Code(line));
        } else if let Some(found) = DEFINITION.captures(line) {
            definitions.entry(label_key(&found[1])).or_insert_with(|| found[2].to_owned());
            lines.push(Line::Definition);
        } else {
            lines.push(Line::Text(line));
        }
    }
    if definitions.is_empty() {
        return source.to_owned();
    }
    let inline = |found: &regex::Captures| {
        let label = if found[3].is_empty() { &found[2] } else { &found[3] };
        match definitions.get(&label_key(label)) {
            Some(destination) => format!("{}[{}]({destination})", &found[1], &found[2]),
            None => found[0].to_owned(),
        }
    };
    lines
        .into_iter()
        .map(|line| match line {
            Line::Definition => String::new(),
            Line::Code(line) => line.to_owned(),
            Line::Text(line) => REFERENCE.replace_all(line, &inline).into_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rendered(source: &str) -> Vec<PreviewBlock> {
    render_markdown(&inline_references(source))
}

/// The cell's preview: the rendered blocks with their images resolved against `attachments`.
pub fn markdown_blocks(source: &str, attachments: &Value) -> Vec<PreviewBlock> {
    rendered(source)
        .into_iter()
        .map(|block| PreviewBlock { line: block.line, nodes: resolve_nodes(&block.nodes, attachments) })
        .collect()
}

/// The image data of the attachments `names` (an own key of the attachments, or nothing), the only
/// part of them a preview depends on.
fn referenced_images(names: &[String], attachments: &Value) -> String {
    let record = object(attachments);
    let parts: Vec<Value> = names
        .iter()
        .map(|name| {
            let bundle = record.get(name).map(object).unwrap_or_default();
            let field = |mime: &str| bundle.get(mime).cloned().unwrap_or(Value::Null);
            serde_json::json!([name, field("image/png"), field("image/jpeg"), field("image/svg+xml")])
        })
        .collect();
    Value::Array(parts).to_string()
}

struct Cached {
    source: String,
    /// The attachments the source references, read before they were resolved to `data:` URLs.
    names: Vec<String>,
    key: String,
    blocks: Rc<Vec<PreviewBlock>>,
}

/// The previews of a notebook's Markdown cells, by cell id.
#[derive(Default)]
pub struct MarkdownCache {
    cells: HashMap<String, Cached>,
    /// How many previews were built, for the tests of rebuilding.
    built: usize,
}

impl MarkdownCache {
    /// The preview of cell `id`; the one built before when neither the source nor a referenced
    /// attachment's image changed.
    pub fn blocks(&mut self, id: &str, source: &str, attachments: &Value) -> Rc<Vec<PreviewBlock>> {
        let names = match self.cells.get(id) {
            Some(cached) if cached.source == source => cached.names.clone(),
            _ => attachment_names(&rendered(source)),
        };
        let key = if names.is_empty() { String::new() } else { referenced_images(&names, attachments) };
        if let Some(cached) = self.cells.get(id).filter(|cached| cached.source == source && cached.key == key) {
            return cached.blocks.clone();
        }
        let blocks = Rc::new(markdown_blocks(source, attachments));
        self.built += 1;
        self.cells.insert(id.to_owned(), Cached { source: source.to_owned(), names, key, blocks: blocks.clone() });
        blocks
    }

    /// Forgets the cells that are gone.
    pub fn retain(&mut self, live: &[&str]) {
        self.cells.retain(|id, _| live.contains(&id.as_str()));
    }

    pub fn built(&self) -> usize {
        self.built
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn data_raster_images_stay_and_other_images_show_their_text() {
        let blocks = markdown_blocks(
            "![a](data:image/png;base64,iVBORw0KGgo=) ![remote](https://x.invalid/a.png) ![local](./a.png)",
            &json!({}),
        );
        let Node::Paragraph(content) = &blocks[0].nodes[0] else { panic!("a paragraph") };
        let images: Vec<&str> = content
            .iter()
            .filter_map(|inline| match inline {
                Inline::Image(image) => Some(image.source.as_str()),
                Inline::Text(_) => None,
            })
            .collect();
        assert_eq!(images, ["data:image/png;base64,iVBORw0KGgo="]);
        let text: String = content
            .iter()
            .filter_map(|inline| match inline {
                Inline::Text(run) => Some(run.text.as_str()),
                Inline::Image(_) => None,
            })
            .collect();
        assert!(text.contains("remote") && text.contains("local"), "{text}");
    }

    #[test]
    fn references_are_written_inline_and_code_fences_are_left_alone() {
        let source = "See [docs][D] and ![chart][].\n\n[d]: https://example.test/docs \"Docs\"\n[chart]: <attachment:c.png>\n\n```\n[x][d]\n[d]: kept\n```";
        assert_eq!(
            inline_references(source),
            "See [docs](https://example.test/docs) and ![chart](attachment:c.png).\n\n\n\n\n```\n[x][d]\n[d]: kept\n```"
        );
        assert_eq!(inline_references("[a][missing]"), "[a][missing]");
    }
}
