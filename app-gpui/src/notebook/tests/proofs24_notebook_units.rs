//! What Markdown cells and outputs show, decided from the notebook's own data: attachments that
//! resolve and those that do not, previews kept across fresh snapshots, and the markup, HTML and
//! SVG of an untrusted notebook kept from loading, running or navigating anything.

use crate::editor_lsp::markdown::Run;
use crate::features_b::viewers::render::{Element, Image, Inline, LinkTarget, Node, PreviewBlock, resolve_link};
use crate::notebook::markdown_cell::{MarkdownCache, markdown_blocks};
use crate::notebook::output_items::{OutputItem, html_text, output_items};
use serde_json::{Value, json};
use std::rc::Rc;

const PNG: &str = "iVBORw0KGgo=";

/// Every node of the blocks, nested ones included, in document order.
fn nodes(blocks: &[PreviewBlock]) -> Vec<Node> {
    fn walk(nodes: &[Node], out: &mut Vec<Node>) {
        for node in nodes {
            out.push(node.clone());
            match node {
                Node::Quote(inner) => walk(inner, out),
                Node::List { items, .. } => items.iter().for_each(|item| walk(&item.content, out)),
                Node::Element(element) => walk(&element.children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for block in blocks {
        walk(&block.nodes, &mut out);
    }
    out
}

fn inlines(blocks: &[PreviewBlock]) -> Vec<Inline> {
    nodes(blocks)
        .into_iter()
        .flat_map(|node| match node {
            Node::Paragraph(content) | Node::Heading { content, .. } => content,
            Node::Image(image) => vec![Inline::Image(image)],
            _ => Vec::new(),
        })
        .collect()
}

fn images(blocks: &[PreviewBlock]) -> Vec<Image> {
    inlines(blocks)
        .into_iter()
        .filter_map(|inline| match inline {
            Inline::Image(image) => Some(image),
            Inline::Text(_) => None,
        })
        .collect()
}

fn runs(blocks: &[PreviewBlock]) -> Vec<Run> {
    inlines(blocks)
        .into_iter()
        .filter_map(|inline| match inline {
            Inline::Text(run) => Some(run),
            Inline::Image(_) => None,
        })
        .collect()
}

fn text(blocks: &[PreviewBlock]) -> String {
    nodes(blocks).iter().filter(|node| !matches!(node, Node::Element(_))).map(Node::text).collect()
}

fn elements(blocks: &[PreviewBlock]) -> Vec<Element> {
    nodes(blocks)
        .into_iter()
        .filter_map(|node| match node {
            Node::Element(element) => Some(element),
            _ => None,
        })
        .collect()
}

fn attachments(name: &str, data: &str) -> Value {
    serde_json::from_str(&format!("{{{}:{{\"image/png\":\"{data}\"}}}}", json!(name))).expect("valid JSON")
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › leaves native interactions such as
// <details> alone and only claims links
#[test]
fn leaves_native_interactions_such_as_details_alone_and_only_claims_links() {
    let blocks = markdown_blocks("<details><summary>More</summary>body</details>", &json!({}));
    let tags: Vec<String> = elements(&blocks).into_iter().map(|element| element.tag).collect();
    assert!(tags.contains(&"details".to_owned()) && tags.contains(&"summary".to_owned()), "{tags:?}");
    assert!(runs(&blocks).iter().all(|run| run.style.link.is_none()), "only links are claimed");
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › keeps the rendered preview across
// snapshots with equal attachments
#[test]
fn keeps_the_rendered_preview_across_snapshots_with_equal_attachments() {
    let mut cache = MarkdownCache::default();
    let source = "text ![a](attachment:a.png)";
    let first = cache.blocks("m1", source, &attachments("a.png", PNG));
    let again = cache.blocks("m1", source, &attachments("a.png", PNG));
    assert!(Rc::ptr_eq(&first, &again));
    let changed = cache.blocks("m1", source, &attachments("a.png", "AAAA"));
    assert!(!Rc::ptr_eq(&first, &changed));
    assert_eq!(images(&changed)[0].source, "data:image/png;base64,AAAA");
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › resolves an entity-encoded attachment
// in a %s (it.each: Markdown destination, reference definition, raw HTML)
#[test]
fn resolves_an_entity_encoded_attachment_in_a_markdown_destination_a_reference_definition_and_raw_html() {
    let cases = [
        ("Markdown destination", "![a](attachment&#58;a.png)"),
        ("reference definition", "![a][r]\n\n[r]: attachment&#58;a.png"),
        ("raw HTML", "<img src=\"attachment&#x3A;a.png\" alt=\"a\">"),
    ];
    for (kind, source) in cases {
        let blocks = markdown_blocks(source, &attachments("a.png", PNG));
        let sources: Vec<String> = images(&blocks).into_iter().map(|image| image.source).collect();
        assert_eq!(sources, [format!("data:image/png;base64,{PNG}")], "{kind}");
    }
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › resolves an attachment named %s from
// parsed notebook JSON (it.each: __proto__, constructor)
#[test]
fn resolves_an_attachment_named_proto_or_constructor_from_parsed_notebook_json() {
    for name in ["__proto__", "constructor"] {
        let blocks = markdown_blocks(&format!("![a](attachment:{name})"), &attachments(name, PNG));
        let sources: Vec<String> = images(&blocks).into_iter().map(|image| image.source).collect();
        assert_eq!(sources, [format!("data:image/png;base64,{PNG}")], "{name}");
    }
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › does not resolve inherited members as
// attachments
#[test]
fn does_not_resolve_inherited_members_as_attachments() {
    let blocks = markdown_blocks("![a](attachment:constructor)", &json!({}));
    assert!(images(&blocks).is_empty());
    assert_eq!(text(&blocks).trim(), "a");
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › does not rebuild when only an
// unreferenced attachment changes
#[test]
fn does_not_rebuild_when_only_an_unreferenced_attachment_changes() {
    let mut cache = MarkdownCache::default();
    let source = "![a](attachment:a.png)";
    let first = cache.blocks("m1", source, &json!({ "a.png": { "image/png": PNG } }));
    let more = json!({ "a.png": { "image/png": PNG }, "b.png": { "image/png": "AAAA" } });
    let again = cache.blocks("m1", source, &more);
    assert!(Rc::ptr_eq(&first, &again));
    assert_eq!(cache.built(), 1);
}

// src/features/notebook/OutputView.test.tsx › notebook Markdown preview › ignores attachment identity when the
// source references none
#[test]
fn ignores_attachment_identity_when_the_source_references_none() {
    let mut cache = MarkdownCache::default();
    let first = cache.blocks("m1", "plain", &json!({}));
    let again = cache.blocks("m1", "plain", &json!({ "unrelated": {} }));
    assert!(Rc::ptr_eq(&first, &again));
    assert_eq!(cache.built(), 1);
}

// src/features/notebook/security.test.ts › static notebook output sanitization › keeps raw Markdown markup from
// reaching the page: %s (it.each over eight kinds of markup)
#[test]
fn keeps_raw_markdown_markup_from_reaching_the_page() {
    let markup = [
        "<style>body{outline:7px solid red}</style>",
        "<form action=\"https://example.com/x\"><button>go</button></form>",
        "<div style=\"position:fixed\">x</div>",
        "<link rel=\"stylesheet\" href=\"https://example.com/x.css\">",
        "<meta http-equiv=\"refresh\" content=\"0;url=https://example.com\">",
        "<base href=\"https://example.com/\">",
        "<iframe src=\"https://example.com\"></iframe>",
        "<button formaction=\"https://example.com/x\">go</button>",
    ];
    for raw in markup {
        let blocks = markdown_blocks(&format!("text\n\n{raw}\n"), &json!({}));
        for element in elements(&blocks) {
            assert!(
                !["style", "form", "link", "meta", "base", "iframe", "button"].contains(&element.tag.as_str()),
                "{raw}: {}",
                element.tag
            );
            assert!(
                element.attributes.iter().all(|(name, _)| name != "style" && name != "formaction"),
                "{raw}: {:?}",
                element.attributes
            );
        }
        assert!(!text(&blocks).contains("outline"), "{raw}");
        assert!(runs(&blocks).iter().all(|run| run.style.link.is_none()), "{raw}: no link is made of markup");
    }
}

// src/features/notebook/security.test.ts › static notebook output sanitization › retains table content while
// removing executable, navigation and external-resource surfaces
#[test]
fn retains_table_content_while_removing_executable_navigation_and_external_resource_surfaces() {
    let html = r#"
      <meta http-equiv="refresh" content="0;url=https://attacker.invalid">
      <base href="https://attacker.invalid"><script>window.__TAURI_INTERNALS__.invoke('x')</script>
      <form action="https://attacker.invalid"><input name="secret"><button>submit</button></form>
      <iframe src="https://attacker.invalid"></iframe><object data="https://attacker.invalid"></object>
      <a href="https://attacker.invalid" target="_top" ping="https://attacker.invalid">link</a>
      <img src="https://attacker.invalid/a.png" srcset="https://attacker.invalid/b.png 2x" onerror="alert(1)">
      <div style="background-image:image-set('https://attacker.invalid/x')">safe text</div>
      <table><thead><tr><th>name</th><th>value</th></tr></thead><tbody><tr><td>alpha</td><td>42</td></tr></tbody></table>
    "#;
    let shown = html_text(html);
    assert!(shown.lines().any(|line| line == "alpha\t42"), "{shown}");
    assert!(shown.contains("safe text") && shown.contains("link"), "{shown}");
    for gone in ["attacker.invalid", "__TAURI", "alert", "secret", "<", "image-set"] {
        assert!(!shown.contains(gone), "{gone} in {shown}");
    }
    let items = output_items(&[json!({ "output_type": "display_data", "data": { "text/html": html } })]);
    assert!(matches!(&items[..], [OutputItem::Html { text }] if *text == shown));
}

// src/features/notebook/security.test.ts › static notebook output sanitization › retains safe SVG chart paint,
// local definitions and raster images but removes active/external SVG
//
// No SVG is drawn: an SVG output shows its plain text, so neither its active parts nor its external references
// reach a decoder.
#[test]
fn retains_safe_svg_chart_paint_local_definitions_and_raster_images_but_removes_active_external_svg() {
    let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><script>alert(1)</script>
      <use href="https://attacker.invalid/a.svg#x"/><image href="data:image/png;base64,aGVsbG8="/></svg>"##;
    let items = output_items(&[json!({
        "output_type": "display_data",
        "data": { "image/svg+xml": svg, "text/plain": "<Figure size 10x10>" }
    })]);
    assert_eq!(items, [OutputItem::Svg { alt: "<Figure size 10x10>".to_owned() }]);
}

// src/features/notebook/security.test.ts › static notebook output sanitization › resolves only embedded image
// attachments and makes Markdown file links editor-controlled
#[test]
fn resolves_only_embedded_image_attachments_and_makes_markdown_file_links_editor_controlled() {
    let source = "![chart](attachment:chart.png)\n\n![remote](https://attacker.invalid/a.png)\n\n[local](./other.py#L8)\n\n<script>alert(1)</script><div style=\"position:fixed\">text</div>";
    let blocks = markdown_blocks(source, &json!({ "chart.png": { "image/png": "aGVsbG8=" } }));
    let sources: Vec<String> = images(&blocks).into_iter().map(|image| image.source).collect();
    assert_eq!(sources, ["data:image/png;base64,aGVsbG8="], "the remote image is not loaded");
    assert!(text(&blocks).contains("remote"), "the remote image shows its text");
    let links: Vec<String> = runs(&blocks).into_iter().filter_map(|run| run.style.link).collect();
    assert_eq!(links, ["./other.py#L8"]);
    assert!(matches!(
        resolve_link("/p/a.ipynb", &links[0]),
        Some(LinkTarget::File { path, line: Some(7), .. }) if path == "/p/other.py"
    ));
    assert!(!text(&blocks).contains("alert"));
    assert!(elements(&blocks).iter().all(|element| element.attributes.iter().all(|(name, _)| name != "style")));
}
