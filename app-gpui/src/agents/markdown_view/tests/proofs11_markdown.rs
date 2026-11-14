use super::*;
use crate::agents::conversation::PathLink;

/// The text elements a message draws, in order.
const TEXTS: [&str; 4] = ["md-text-0", "md-text-1", "md-text-2", "md-text-3"];

/// Raw markup in a reply (styles, forms, links to style sheets, meta refreshes, bases, frames, form actions)
/// draws none of its markup, attributes or addresses.
#[gpui_kit::test]
async fn keeps_raw_markup_from_reaching_the_page(cx: &mut TestAppContext) {
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
        let (view, _, cx) = open(cx, &format!("text\n\n{raw}"));
        let shown = lines(&view, cx);
        assert_eq!(shown.first().map(String::as_str), Some("text"), "{raw}");
        for line in &shown {
            let leaked = line.contains("outline") || line.contains('<') || line.contains("example.com");
            assert!(!leaked, "{raw}: {shown:?}");
        }
        assert!(cx.debug_bounds("md-copy-0").is_none(), "{raw}: no code block, no Copy control");
    }
}

/// Data attributes written in a reply give nothing a meaning: the only Copy control is the code block's own,
/// copying its text, and clicking the other text opens nothing.
#[gpui_kit::test]
async fn ignores_data_attributes_written_in_a_reply_keeping_its_own(cx: &mut TestAppContext) {
    let text =
        "<button data-copy=\"evil\">x</button>\n\n<code data-path=\"/etc/passwd\">y</code>\n\n```ts\nconst a = 1;\n```";
    let (_, events, cx) = open(cx, text);
    assert!(cx.debug_bounds("md-copy-0").is_some() && cx.debug_bounds("md-copy-1").is_none());
    for selector in TEXTS {
        if cx.debug_bounds(selector).is_some() {
            click_start(cx, selector);
        }
    }
    assert!(
        !events.borrow().iter().any(|event| matches!(event, MarkdownEvent::Link(LinkAction::File(_)))),
        "{:?}",
        events.borrow()
    );
    events.borrow_mut().clear();
    click(cx, "md-copy-0");
    assert_eq!(*events.borrow(), [MarkdownEvent::Copied("const a = 1;".to_owned())]);
    assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some("const a = 1;"));
}

/// A link to a file (absolute, relative to the conversation's folder, percent-encoded, with a line and column or
/// a `#L` line) opens that file at that place, not in the browser.
#[gpui_kit::test]
async fn opens_a_markdown_file_link(cx: &mut TestAppContext) {
    for (href, path, line, col) in [
        ("/Users/thomas/vmos-shots/tiktok-home.jpg", "/Users/thomas/vmos-shots/tiktok-home.jpg", None, None),
        ("images/home.jpg", "/repo/images/home.jpg", None, None),
        ("/tmp/截图%20one.jpg", "/tmp/截图 one.jpg", None, None),
        ("/repo/main.rs:12:5", "/repo/main.rs", Some(11), Some(4)),
        ("/repo/main.rs#L7", "/repo/main.rs", Some(6), None),
    ] {
        let (_, events, cx) = open(cx, &format!("[查看文件]({href})"));
        click_start(cx, "md-text-0");
        let file = LinkAction::File(PathLink { path: path.to_owned(), line, col });
        assert_eq!(*events.borrow(), [MarkdownEvent::Link(file)], "{href}");
    }
}

/// An anchor, a protocol-relative URL, a script URL and a malformed path open neither a file nor the browser.
#[gpui_kit::test]
async fn does_not_open_unsupported_links_as_files(cx: &mut TestAppContext) {
    for href in ["#section", "//example.com/image.jpg", "javascript:alert(1)", "/tmp/%ZZ.jpg"] {
        let (_, events, cx) = open(cx, &format!("[Link]({href})"));
        click_start(cx, "md-text-0");
        let opened = events
            .borrow()
            .iter()
            .any(|event| matches!(event, MarkdownEvent::Link(LinkAction::File(_) | LinkAction::External(_))));
        assert!(!opened, "{href}: {:?}", events.borrow());
    }
}
