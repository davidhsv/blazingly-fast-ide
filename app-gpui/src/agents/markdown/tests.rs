use super::*;

const PIXEL: &str = "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
const DIAGRAM: &str = "flowchart TD\n    A-->B";

fn fence() -> String {
    format!("```mermaid\n{DIAGRAM}\n```\n")
}

/// Every piece of text the blocks show.
fn all_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    for block in blocks {
        match block {
            Block::Paragraph(content) | Block::Heading { content, .. } => out.push_str(&plain_text(content)),
            Block::Code { text, .. } => out.push_str(text),
            Block::Mermaid { source } => out.push_str(source),
            Block::Quote(inner) => out.push_str(&all_text(inner)),
            Block::List { items, .. } => items.iter().for_each(|item| out.push_str(&all_text(item))),
            Block::Table { header, rows } => {
                header.iter().chain(rows.iter().flatten()).for_each(|cell| out.push_str(&plain_text(cell)));
            }
            Block::Rule => {}
        }
        out.push('\n');
    }
    out
}

fn runs(blocks: &[Block]) -> Vec<&Run> {
    let mut out = Vec::new();
    walk(blocks, &mut |block| {
        if let Block::Paragraph(content) | Block::Heading { content, .. } = block {
            out.extend(content.iter().filter_map(|i| match i {
                Inline::Text(run) => Some(run),
                Inline::Image(_) => None,
            }));
        }
    });
    out
}

fn link_of(text: &str) -> String {
    let blocks = parse(text);
    runs(&blocks).iter().find_map(|r| r.style.link.clone()).expect("a link")
}

fn text_run(text: &str, style: Style) -> Inline {
    Inline::Text(Run { text: text.to_owned(), style })
}

fn paragraph(text: &str) -> Vec<Block> {
    vec![Block::Paragraph(vec![text_run(text, Style::default())])]
}

// ----- agents.test.ts -----

#[test]
fn renders_agent_markdown_without_scripts() {
    let blocks = parse("**bold** <img src=x onerror=alert(1)><script>alert(1)</script>");
    let Some(Block::Paragraph(content)) = blocks.first() else { panic!("a paragraph") };
    assert_eq!(content[0], text_run("bold", Style { bold: true, ..Style::default() }));
    assert!(content.contains(&Inline::Image(Image { alt: String::new(), source: "x".into() })));
    assert!(!all_text(&blocks).contains("alert"));
}

#[test]
fn gives_the_text_of_a_list_item_with_a_nested_list_its_own_block() {
    let blocks = parse("- **parent** text:\n  - child\n- plain");
    let bold = Style { bold: true, ..Style::default() };
    let expected = vec![Block::List {
        ordered: None,
        items: vec![
            vec![
                Block::Paragraph(vec![text_run("parent", bold), text_run(" text:", Style::default())]),
                Block::List { ordered: None, items: vec![paragraph("child")] },
            ],
            paragraph("plain"),
        ],
    }];
    assert_eq!(blocks, expected);
}

// ----- Markdown.test.tsx -----

#[test]
fn keeps_one_copy_control_outside_each_code_block_when_effects_restart() {
    let blocks = parse("Before\n\n```text\nrust/\n└── src/main.rs\n```\n\n```ts\nconst a = 1;\n```\n\nAfter");
    assert_eq!(copy_texts(&blocks), vec!["rust/\n└── src/main.rs", "const a = 1;"]);
}

#[test]
fn keeps_raw_markup_from_reaching_the_page() {
    for raw in [
        "<style>body{outline:7px solid red}</style>",
        r#"<form action="https://example.com/x"><button>go</button></form>"#,
        r#"<div style="position:fixed">x</div>"#,
        r#"<link rel="stylesheet" href="https://example.com/x.css">"#,
        r#"<meta http-equiv="refresh" content="0;url=https://example.com">"#,
        r#"<base href="https://example.com/">"#,
        r#"<iframe src="https://example.com"></iframe>"#,
        r#"<button formaction="https://example.com/x">go</button>"#,
    ] {
        let text = all_text(&parse(&format!("text\n\n{raw}")));
        assert!(!text.contains("outline"), "{raw}");
        assert!(!text.contains('<') && !text.contains("example.com"), "{raw}: {text}");
    }
}

#[test]
fn ignores_data_attributes_written_in_a_reply_keeping_its_own() {
    let blocks = parse(
        "<button data-copy=\"evil\">x</button>\n\n<code data-path=\"/etc/passwd\">y</code>\n\n```ts\nconst a = 1;\n```",
    );
    let code = runs(&blocks).into_iter().find(|r| r.text == "y").expect("the code span");
    assert!(code.style.code);
    assert_eq!(code_link(code, "/repo"), None);
    assert_eq!(copy_texts(&blocks), vec!["const a = 1;"]);
}

#[test]
fn opens_a_markdown_file_link() {
    for (href, path, line, col) in [
        ("/Users/thomas/vmos-shots/tiktok-home.jpg", "/Users/thomas/vmos-shots/tiktok-home.jpg", None, None),
        ("images/home.jpg", "/repo/images/home.jpg", None, None),
        ("/tmp/截图%20one.jpg", "/tmp/截图 one.jpg", None, None),
        ("/repo/main.rs:12:5", "/repo/main.rs", Some(11), Some(4)),
        // An editor opened at a line without a column puts the caret at its start.
        ("/repo/main.rs#L7", "/repo/main.rs", Some(6), None),
    ] {
        let href_written = link_of(&format!("[查看文件]({href})"));
        assert_eq!(
            link_action(&href_written, "/repo"),
            LinkAction::File(PathLink { path: path.into(), line, col }),
            "{href}"
        );
    }
}

#[test]
fn keeps_web_links_external() {
    let href = link_of("[Website](https://example.com/image.jpg)");
    assert_eq!(link_action(&href, "/repo"), LinkAction::External("https://example.com/image.jpg".into()));
}

#[test]
fn does_not_open_unsupported_links_as_files() {
    for href in ["#section", "//example.com/image.jpg", "javascript:alert(1)", "/tmp/%ZZ.jpg"] {
        let written = link_of(&format!("[Link]({href})"));
        assert_eq!(link_action(&written, "/repo"), LinkAction::None, "{href}");
    }
}

fn shown(images: &MessageImages, blocks: &[Block], cwd: &str) -> ImageView {
    let image = super::images(blocks).into_iter().next().expect("an image").clone();
    images.view(&image, cwd, &mut I18n::default())
}

#[test]
fn loads_a_reply_image_from_the_project_host() {
    for (src, cwd, path) in [
        ("/Users/thomas/vmos-shots/tiktok-home.jpg", "/repo", "/Users/thomas/vmos-shots/tiktok-home.jpg"),
        ("images/home.jpg", "/repo", "/repo/images/home.jpg"),
        ("/tmp/截图%20one.jpg", "/repo", "/tmp/截图 one.jpg"),
        ("file:///tmp/auth%20code.png", "/repo", "/tmp/auth code.png"),
        ("C:/shots/code.png", "C:/repo", "C:/shots/code.png"),
        ("file:///C:/shots/code.png", "C:/repo", "C:/shots/code.png"),
    ] {
        let blocks = parse(&format!("![QR code]({src})"));
        let mut images = MessageImages::default();
        assert_eq!(images.render(&blocks, cwd), vec![path.to_owned()], "{src}");
        assert_eq!(shown(&images, &blocks, cwd), ImageView::Loading);
        assert!(images.read(path, Ok(PIXEL.into())));
        assert_eq!(shown(&images, &blocks, cwd), ImageView::Source(PIXEL.into()));
    }
}

#[test]
fn loads_raw_html_images_without_allowing_alternate_sources_or_forged_paths() {
    let blocks = parse(concat!(
        r#"<picture><source srcset="/wrong.png"><img alt="QR" src="/tmp/code.png" srcset="/wrong.png 2x" "#,
        r#"data-image-src="/forged.png" onerror="alert(1)"></picture>"#
    ));
    let mut images = MessageImages::default();
    assert_eq!(images.render(&blocks, "/repo"), vec!["/tmp/code.png".to_owned()]);
    images.read("/tmp/code.png", Ok(PIXEL.into()));
    assert_eq!(shown(&images, &blocks, "/repo"), ImageView::Source(PIXEL.into()));
}

#[test]
fn keeps_an_inline_or_web_image() {
    for src in [PIXEL, "https://example.com/code.png", "http://localhost:9000/code.png"] {
        let blocks = parse(&format!("![QR]({src})"));
        let mut images = MessageImages::default();
        assert!(images.render(&blocks, "/repo").is_empty(), "{src}");
        assert_eq!(shown(&images, &blocks, "/repo"), ImageView::Source(src.into()));
    }
}

#[test]
fn reuses_image_reads_while_a_reply_streams_but_reads_again_for_another_message() {
    let mut images = MessageImages::default();
    assert_eq!(images.render(&parse("![QR](/tmp/stream.png)"), "/repo").len(), 1);
    images.read("/tmp/stream.png", Ok(PIXEL.into()));
    let streamed = parse("![QR](/tmp/stream.png)\n\nScan to authorize.");
    assert!(images.render(&streamed, "/repo").is_empty());
    assert_eq!(shown(&images, &streamed, "/repo"), ImageView::Source(PIXEL.into()));
    assert_eq!(MessageImages::default().render(&parse("![QR](/tmp/stream.png)"), "/repo").len(), 1);
}

#[test]
fn reports_read_failure_and_retries_on_the_next_render() {
    let mut images = MessageImages::default();
    let blocks = parse("![QR](/tmp/missing.png)");
    images.render(&blocks, "/repo");
    images.read("/tmp/missing.png", Err("missing".into()));
    assert_eq!(shown(&images, &blocks, "/repo"), ImageView::Failed("Unable to load image: /tmp/missing.png".into()));
    let saved = parse("![QR](/tmp/missing.png)\n\nNow saved.");
    assert_eq!(images.render(&saved, "/repo"), vec!["/tmp/missing.png".to_owned()]);
    images.read("/tmp/missing.png", Ok(PIXEL.into()));
    assert_eq!(shown(&images, &saved, "/repo"), ImageView::Source(PIXEL.into()));
}

#[test]
fn does_not_update_detached_images_after_a_streamed_reply_replaces_them() {
    let mut images = MessageImages::default();
    images.render(&parse("![QR](/tmp/pending.png)"), "/repo");
    images.render(&parse("Image removed."), "/repo");
    assert!(!images.read("/tmp/pending.png", Ok(PIXEL.into())));
}

#[test]
fn does_not_load_unsafe_or_malformed_image_sources() {
    for src in ["javascript:alert(1)", "data:text/html,test", "/tmp/%ZZ.png", "/tmp/code%00.png"] {
        let blocks = parse(&format!("<img alt=\"QR\" src=\"{src}\">"));
        let mut images = MessageImages::default();
        assert!(images.render(&blocks, "/repo").is_empty(), "{src}");
        assert!(matches!(shown(&images, &blocks, "/repo"), ImageView::Failed(_)), "{src}");
    }
}

#[test]
fn reports_network_or_decode_failures_without_leaving_a_broken_source() {
    let blocks = parse("![QR](https://example.com/gone.png)");
    let mut images = MessageImages::default();
    images.render(&blocks, "/repo");
    images.web_failed("https://example.com/gone.png");
    assert_eq!(
        shown(&images, &blocks, "/repo"),
        ImageView::Failed("Unable to load image: https://example.com/gone.png".into())
    );
}

#[test]
fn renders_the_fence_as_a_placeholder_holding_the_source_then_draws_the_diagram() {
    let blocks = parse(&format!("Before\n\n{}\n\n```ts\nconst a = 1;\n```", fence()));
    assert!(blocks.contains(&Block::Mermaid { source: DIAGRAM.into() }));
    assert!(blocks.contains(&Block::Code { language: Some("ts".into()), text: "const a = 1;".into() }));
    let mut drawings = Diagrams::default();
    assert_eq!(drawings.render(&diagrams(&blocks)), vec![DIAGRAM.to_owned()]);
    assert!(!drawings.is_drawn(DIAGRAM));
    drawings.drawn(DIAGRAM);
    assert!(drawings.is_drawn(DIAGRAM));
}

#[test]
fn draws_two_equal_diagrams_with_unique_svg_ids_so_markers_of_a_hidden_copy_stay_live() {
    // The ids of each copy are the view's; the model draws an equal diagram once.
    let mut drawings = Diagrams::default();
    let blocks = parse(&fence());
    assert_eq!(drawings.render(&diagrams(&blocks)).len(), 1);
    drawings.drawn(DIAGRAM);
    assert!(drawings.render(&diagrams(&parse(&fence()))).is_empty());
}

#[test]
fn keeps_the_source_copyable_on_the_block_once_the_diagram_is_drawn() {
    assert_eq!(copy_texts(&parse(&fence())), vec![DIAGRAM]);
}

#[test]
fn tells_the_conversation_to_follow_when_a_diagram_draws_after_the_message_rendered() {
    let mut drawings = Diagrams::default();
    assert_eq!(drawings.render(&diagrams(&parse(&fence()))).len(), 1);
    assert!(drawings.drawn(DIAGRAM));
    // The next streamed chunk finds it drawn: no draw, so no further follow.
    assert!(drawings.render(&diagrams(&parse(&format!("{}\n\nDone.", fence())))).is_empty());
}

// Not from a spec: the reply AgentReply.test.tsx copies reads as the blocks its rendering checks.
#[test]
fn reads_headings_lists_links_code_and_tables() {
    let reply = "**OpenIDEA v0.1.38 is out!**\n\n## Highlights\n\n- **Docker & Compose** run targets.\n\
                 - [Release notes](https://example.com/releases)\n\n```sh\npnpm tauri dev\n```\n\n\
                 | Feature | Status |\n| --- | --- |\n| 中文 | Ready |\n";
    let blocks = parse(reply);
    assert!(matches!(&blocks[1], Block::Heading { level: 2, content } if plain_text(content) == "Highlights"));
    assert!(matches!(&blocks[2], Block::List { items, .. } if items.len() == 2));
    assert_eq!(link_of(reply), "https://example.com/releases");
    assert_eq!(copy_texts(&blocks), vec!["pnpm tauri dev"]);
    let Some(Block::Table { header, rows }) = blocks.last() else { panic!("a table") };
    assert_eq!(header.iter().map(|c| plain_text(c)).collect::<Vec<_>>(), vec!["Feature", "Status"]);
    assert_eq!(rows[0].iter().map(|c| plain_text(c)).collect::<Vec<_>>(), vec!["中文", "Ready"]);
}
