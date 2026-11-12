//! Markdown files as a preview model: top-level blocks tagged with their 0-based source line (for
//! scroll sync), holding styled runs, images, diagrams and the HTML containers READMEs rely on.
//!
//! The text inside a block is read by the documentation parser ([`crate::editor_lsp::markdown`]).
//! This module splits the file into top-level blocks first, reads YAML front matter, keeps image
//! sources (the documentation parser shows only their alternative text), task list checkboxes,
//! GitHub-style heading anchors and Mermaid fences. Raw HTML never reaches the page: a fixed set
//! of elements and attributes is kept as [`Element`]s, `<style>`, `<script>`, `<iframe>` and the
//! like are dropped with their content, every other tag is dropped and its text kept. Blocks
//! inside an HTML container left open (`<details>`, `<div>`…) belong to it up to its close: they
//! share its block, so the container does not end early and spill its content out.

use crate::editor_lsp::markdown::{self as docs, Block, Run, Style};
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// Heading anchors carry GitHub's prefix, so links copied from GitHub resolve.
pub const ANCHOR_PREFIX: &str = "user-content-";

/// An image of the preview: its source as written (loaded by the preview, local files through
/// the backend) and the size attributes of an HTML `<img>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Image {
    pub alt: String,
    pub source: String,
    pub width: Option<String>,
    pub height: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inline {
    Text(Run),
    Image(Image),
}

/// A front matter value: one line as written, or a nested mapping or list shown in its YAML layout
/// (without the indentation its lines share).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldValue {
    Text(String),
    Nested(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatterRow {
    pub key: String,
    pub value: FieldValue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// A task list item's checkbox: `Some(true)` for `- [x]`.
    pub checked: Option<bool>,
    pub content: Vec<Node>,
}

/// An HTML element kept from the file, with the attributes that are safe to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Lower case.
    pub tag: String,
    pub attributes: Vec<(String, String)>,
    pub children: Vec<Node>,
}

impl Element {
    pub fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    /// Whether a `<details>` is written open.
    pub fn is_open(&self) -> bool {
        self.attribute("open").is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// The YAML front matter: a table of its top-level fields.
    FrontMatter(Vec<FrontMatterRow>),
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        /// `user-content-<slug>`, deduplicated; `None` for headings inside lists and quotes.
        anchor: Option<String>,
        content: Vec<Inline>,
    },
    Code {
        language: Option<String>,
        text: String,
    },
    /// A ```` ```mermaid ```` fence: drawn by [`super::mermaid`], its source shown until then.
    Mermaid {
        source: String,
    },
    Quote(Vec<Node>),
    List {
        ordered: Option<u64>,
        items: Vec<ListItem>,
    },
    Table {
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
    /// An HTML `<img>` outside running text.
    Image(Image),
    Element(Element),
}

impl Node {
    /// The HTML element name the node stands for, in upper case (`H1`, `P`, `DETAILS`).
    pub fn tag(&self) -> String {
        match self {
            Node::FrontMatter(_) | Node::Table { .. } => "TABLE".to_owned(),
            Node::Paragraph(_) => "P".to_owned(),
            Node::Heading { level, .. } => format!("H{level}"),
            Node::Code { .. } => "PRE".to_owned(),
            Node::Mermaid { .. } => "DIV".to_owned(),
            Node::Quote(_) => "BLOCKQUOTE".to_owned(),
            Node::List { ordered: Some(_), .. } => "OL".to_owned(),
            Node::List { ordered: None, .. } => "UL".to_owned(),
            Node::Rule => "HR".to_owned(),
            Node::Image(_) => "IMG".to_owned(),
            Node::Element(element) => element.tag.to_ascii_uppercase(),
        }
    }

    /// The text the node shows, without styles.
    pub fn text(&self) -> String {
        match self {
            Node::FrontMatter(rows) => rows
                .iter()
                .map(|row| match &row.value {
                    FieldValue::Text(value) | FieldValue::Nested(value) => format!("{}{value}", row.key),
                })
                .collect(),
            Node::Paragraph(content) | Node::Heading { content, .. } => inline_text(content),
            Node::Code { text, .. } | Node::Mermaid { source: text } => text.clone(),
            Node::Quote(nodes) => nodes_text(nodes),
            Node::List { items, .. } => items.iter().map(|item| nodes_text(&item.content)).collect(),
            Node::Table { header, rows } => {
                std::iter::once(header).chain(rows).flatten().map(|cell| inline_text(cell)).collect()
            }
            Node::Rule | Node::Image(_) => String::new(),
            Node::Element(element) => nodes_text(&element.children),
        }
    }
}

/// The text of `nodes`, concatenated.
pub fn nodes_text(nodes: &[Node]) -> String {
    nodes.iter().map(Node::text).collect()
}

/// The text of inline content, images left out.
pub fn inline_text(content: &[Inline]) -> String {
    content
        .iter()
        .filter_map(|inline| match inline {
            Inline::Text(run) => Some(run.text.as_str()),
            Inline::Image(_) => None,
        })
        .collect()
}

/// A top-level block of the preview and the 0-based source line it starts at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewBlock {
    pub line: u32,
    pub nodes: Vec<Node>,
}

/// GitHub-style heading anchor: lower case, tags and punctuation dropped, each space a dash.
pub fn slugify(text: &str) -> String {
    static TAGS: LazyLock<Regex> = LazyLock::new(|| pattern(r"<[^>]*>"));
    let lower = text.trim().to_lowercase();
    TAGS.replace_all(&lower, "")
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '_' || *c == '-')
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}

fn count_lines(text: &str) -> usize {
    text.matches('\n').count()
}

/// The YAML front matter at the start of a file: its top-level fields and how many lines it spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontMatter {
    pub fields: Vec<(String, String)>,
    pub lines: usize,
}

pub fn front_matter(text: &str) -> Option<FrontMatter> {
    static BLOCK: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^---[ \t]*\r?\n((?s:.*?))\r?\n(?:---|\.\.\.)[ \t]*(?:\r?\n|$)"));
    static FIELD: LazyLock<Regex> = LazyLock::new(|| pattern(r"^([^\s#:-][^:]*?):(?:[ \t]+(.*))?$"));
    let found = BLOCK.captures(text)?;
    let whole = found.get(0)?.as_str();
    let body = found.get(1).map_or("", |m| m.as_str());
    let mut fields: Vec<(String, String)> = Vec::new();
    for raw in body.split('\n').map(|line| line.strip_suffix('\r').unwrap_or(line)) {
        if let Some(field) = FIELD.captures(raw) {
            let key = field.get(1).map_or("", |m| m.as_str()).trim().to_owned();
            fields.push((key, field.get(2).map_or("", |m| m.as_str()).trim().to_owned()));
        } else if !raw.trim().is_empty() && !raw.trim_start().starts_with('#') {
            // A nested mapping or a list belongs to the field above it; anything else isn't front matter.
            let nested = raw.starts_with(|c: char| c.is_whitespace() || c == '-');
            let last = fields.last_mut().filter(|_| nested)?;
            last.1 = if last.1.is_empty() { raw.to_owned() } else { format!("{}\n{raw}", last.1) };
        }
    }
    if fields.is_empty() {
        return None;
    }
    Some(FrontMatter { fields, lines: count_lines(whole) + usize::from(!whole.ends_with('\n')) })
}

/// `text` without the indentation all its non-blank lines share.
fn dedent(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let indent = lines.iter().filter(|line| !line.trim().is_empty()).map(|line| indent_of(line)).min();
    match indent {
        Some(indent) => lines.iter().map(|line| line.get(indent..).unwrap_or("")).collect::<Vec<_>>().join("\n"),
        None => lines.iter().map(|_| "").collect::<Vec<_>>().join("\n"),
    }
}

fn front_matter_rows(fields: &[(String, String)]) -> Vec<FrontMatterRow> {
    fields
        .iter()
        .map(|(key, value)| FrontMatterRow {
            key: key.clone(),
            value: if value.contains('\n') {
                FieldValue::Nested(dedent(value.trim_start_matches('\n')))
            } else {
                FieldValue::Text(value.clone())
            },
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Top-level blocks

static FENCE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}(`{3,}|~{3,})"));
static HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}#{1,6}(?:[ \t]|$)"));
static RULE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^ {0,3}(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$"));
static SETEXT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}(=+|-+)[ \t]*$"));
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| pattern(r"^( *)([-*+]|[0-9]{1,9}[.)])(?:[ \t]+(.*))?$"));
static TABLE_RULE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$"));
static HTML_RAW: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^ {0,3}<(script|pre|style|textarea)(?:[ \t>]|$)"));
static HTML_BLOCK_TAG: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^ {0,3}</?([a-z][a-z0-9-]*)(?:[ \t>]|/>|$)"));
static HTML_COMPLETE_TAG: LazyLock<Regex> = LazyLock::new(|| {
    pattern(concat!(
        r#"^ {0,3}(?:<[A-Za-z][A-Za-z0-9-]*(?:\s+[A-Za-z_:][\w.:-]*(?:\s*=\s*(?:[^\s"'=<>`]+|'[^']*'|"[^"]*"))?)*"#,
        r"\s*/?>|</[A-Za-z][A-Za-z0-9-]*\s*>)[ \t]*$",
    ))
});

/// Tag names that start an HTML block (CommonMark's sixth kind): it ends at a blank line.
const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "base",
    "basefont",
    "blockquote",
    "body",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "frame",
    "frameset",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "head",
    "header",
    "hr",
    "html",
    "iframe",
    "legend",
    "li",
    "link",
    "main",
    "menu",
    "menuitem",
    "nav",
    "noframes",
    "ol",
    "optgroup",
    "option",
    "p",
    "param",
    "search",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "title",
    "tr",
    "track",
    "ul",
];

/// Containers whose open and close tags may sit in separate HTML blocks with Markdown between them.
const CONTAINER_TAGS: &[&str] = &[
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "div",
    "dl",
    "figure",
    "footer",
    "header",
    "main",
    "nav",
    "ol",
    "section",
    "table",
    "ul",
];

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// How an HTML block ends.
enum HtmlEnd {
    Blank,
    /// At the line containing this text (lower case), that line included.
    Contains(&'static str),
}

/// Whether `line` starts an HTML block; inside a paragraph only the kinds that interrupt one count.
fn html_start(line: &str, in_paragraph: bool) -> Option<HtmlEnd> {
    if let Some(found) = HTML_RAW.captures(line) {
        return Some(HtmlEnd::Contains(match found.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase().as_str() {
            "script" => "</script>",
            "pre" => "</pre>",
            "style" => "</style>",
            _ => "</textarea>",
        }));
    }
    if indent_of(line) <= 3 && line.trim_start().starts_with("<!--") {
        return Some(HtmlEnd::Contains("-->"));
    }
    if let Some(found) = HTML_BLOCK_TAG.captures(line)
        && BLOCK_TAGS.contains(&found.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase().as_str())
    {
        return Some(HtmlEnd::Blank);
    }
    (!in_paragraph && HTML_COMPLETE_TAG.is_match(line)).then_some(HtmlEnd::Blank)
}

fn is_table_rule(line: &str) -> bool {
    TABLE_RULE.is_match(line) && line.contains('-')
}

fn starts_list_item(line: &str) -> bool {
    LIST_ITEM.captures(line).is_some_and(|found| found.get(3).is_some())
}

/// Whether `line` ends a paragraph and starts another block.
fn interrupts(line: &str) -> bool {
    FENCE.is_match(line)
        || HEADING.is_match(line)
        || RULE.is_match(line)
        || line.trim_start().starts_with('>')
        || starts_list_item(line)
        || html_start(line, true).is_some()
}

/// The line after the fence opened at `start`.
fn fence_end(lines: &[&str], start: usize, fence: &str) -> usize {
    let marker = fence.chars().next().unwrap_or('`');
    let mut i = start + 1;
    while i < lines.len() {
        let closing = lines[i].trim();
        i += 1;
        if closing.len() >= fence.len() && closing.chars().all(|c| c == marker) && indent_of(lines[i - 1]) <= 3 {
            break;
        }
    }
    i
}

/// A list's items, each as its lines without the marker and indentation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ListParts {
    ordered: Option<u64>,
    items: Vec<Vec<String>>,
}

/// The list starting at line `start`, and the line after it.
fn parse_list(lines: &[&str], start: usize) -> (ListParts, usize) {
    fn marker(line: &str) -> Option<(usize, String, String)> {
        let found = LIST_ITEM.captures(line)?;
        let indent = found.get(1).map_or(0, |m| m.as_str().len());
        let marker = found.get(2).map_or("", |m| m.as_str()).to_owned();
        Some((indent, marker, found.get(3).map_or("", |m| m.as_str()).to_owned()))
    }
    let (base, first, _) = marker(lines[start]).unwrap_or_default();
    let numbered = first.starts_with(|c: char| c.is_ascii_digit());
    let ordered = numbered.then(|| first.trim_end_matches(['.', ')']).parse().unwrap_or(1));
    let mut items = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some((indent, mark, rest)) = marker(lines[i]) else { break };
        if indent != base || mark.starts_with(|c: char| c.is_ascii_digit()) != numbered {
            break;
        }
        let width = indent + mark.len() + 1;
        let mut body = vec![rest];
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if is_blank(line) {
                // A blank line stays in the item only when indented text follows it.
                let more = lines[i + 1..].iter().find(|next| !is_blank(next));
                if more.is_some_and(|next| indent_of(next) >= width) {
                    body.push(String::new());
                    i += 1;
                    continue;
                }
                break;
            }
            if indent_of(line) >= width {
                body.push(line[width..].to_owned());
            } else if marker(line).is_some_and(|(indent, _, _)| indent <= base) || interrupts(line) {
                break;
            } else {
                body.push(line.trim_start().to_owned());
            }
            i += 1;
        }
        items.push(body);
        // Blank lines between items do not end the list.
        let mut next = i;
        while next < lines.len() && is_blank(lines[next]) {
            next += 1;
        }
        if next < lines.len() && marker(lines[next]).is_some_and(|(indent, _, _)| indent == base) {
            i = next;
        }
    }
    (ListParts { ordered, items }, i)
}

enum Kind {
    /// Read by the documentation parser; a setext heading's level when its last line underlines it.
    Markdown {
        setext: Option<u8>,
    },
    Html,
    List(ListParts),
}

struct Chunk {
    start: usize,
    end: usize,
    kind: Kind,
}

/// The top-level blocks of `lines`, blank lines between them left out.
fn chunks(lines: &[&str]) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_blank(line) {
            i += 1;
            continue;
        }
        let start = i;
        let mut kind = Kind::Markdown { setext: None };
        if let Some(found) = FENCE.captures(line) {
            i = fence_end(lines, i, found.get(1).map_or("```", |m| m.as_str()));
        } else if HEADING.is_match(line) || RULE.is_match(line) {
            i += 1;
        } else if let Some(end) = html_start(line, false) {
            kind = Kind::Html;
            match end {
                HtmlEnd::Blank => {
                    while i < lines.len() && !is_blank(lines[i]) {
                        i += 1;
                    }
                }
                HtmlEnd::Contains(close) => {
                    while i < lines.len() && !lines[i].to_ascii_lowercase().contains(close) {
                        i += 1;
                    }
                    i = (i + 1).min(lines.len());
                }
            }
        } else if line.trim_start().starts_with('>') {
            while i < lines.len() && !is_blank(lines[i]) {
                i += 1;
            }
        } else if starts_list_item(line) {
            let (parts, next) = parse_list(lines, i);
            kind = Kind::List(parts);
            i = next;
        } else if indent_of(line) >= 4 {
            while i < lines.len() && (indent_of(lines[i]) >= 4 || is_blank(lines[i])) {
                i += 1;
            }
            while i > start + 1 && is_blank(lines[i - 1]) {
                i -= 1;
            }
        } else if line.contains('|') && lines.get(i + 1).is_some_and(|next| is_table_rule(next)) {
            i += 2;
            while i < lines.len() && !is_blank(lines[i]) && lines[i].contains('|') {
                i += 1;
            }
        } else {
            i += 1;
            while i < lines.len() && !is_blank(lines[i]) {
                if let Some(found) = SETEXT.captures(lines[i]) {
                    let equals = found.get(1).is_some_and(|m| m.as_str().starts_with('='));
                    kind = Kind::Markdown { setext: Some(if equals { 1 } else { 2 }) };
                    i += 1;
                    break;
                }
                if interrupts(lines[i]) {
                    break;
                }
                i += 1;
            }
        }
        out.push(Chunk { start, end: i, kind });
    }
    out
}

// ---------------------------------------------------------------------------
// Images inside Markdown text

/// Images stand in the text given to the documentation parser as one private-use character each.
const PLACEHOLDER: u32 = 0xE000;
const PLACEHOLDERS: usize = 6400;

static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r#"([A-Za-z_:][\w.:-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?"#));
static IMG_TAG: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(?i)^<img\b((?:[^>"']|"[^"]*"|'[^']*')*)>"#));

/// The attributes of a tag, names in lower case.
fn attributes(text: &str) -> Vec<(String, String)> {
    ATTRIBUTE
        .captures_iter(text)
        .map(|found| {
            let value = found.get(2).or_else(|| found.get(3)).or_else(|| found.get(4)).map_or("", |m| m.as_str());
            (found.get(1).map_or("", |m| m.as_str()).to_ascii_lowercase(), value.to_owned())
        })
        .collect()
}

fn image_of(attributes_text: &str) -> Image {
    let mut image = Image::default();
    for (name, value) in attributes(attributes_text) {
        match name.as_str() {
            "src" => image.source = value,
            "alt" => image.alt = value,
            "width" => image.width = Some(value),
            "height" => image.height = Some(value),
            _ => {}
        }
    }
    image
}

/// The parts of `![alt](target "title")` at the start of `text`: the alternative text, the
/// target and the length in bytes.
fn markdown_image(text: &str) -> Option<(&str, &str, usize)> {
    let label = text.strip_prefix("![")?;
    let mut depth = 1;
    let mut close = None;
    let mut chars = label.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let close = close?;
    let rest = label[close + 1..].strip_prefix('(')?;
    let mut depth = 1;
    let mut end = None;
    for (i, c) in rest.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end?;
    let inside = rest[..end].trim();
    let target = match inside.strip_prefix('<') {
        Some(angled) => angled.split('>').next().unwrap_or(""),
        None => inside.split([' ', '\t']).next().unwrap_or(""),
    };
    Some((&label[..close], target, 2 + close + 2 + end + 1))
}

/// `line` with its images replaced by placeholders (code spans left alone).
fn mark_line(line: &str, out: &mut String, images: &mut Vec<Image>) {
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        let Some(c) = rest.chars().next() else { break };
        if c == '`' {
            let ticks = rest.chars().take_while(|c| *c == '`').count();
            let span = rest[ticks..].find(&rest[..ticks]).map_or(ticks, |close| ticks + close + ticks);
            out.push_str(&rest[..span]);
            i += span;
            continue;
        }
        let found = if rest.starts_with("![") {
            markdown_image(rest).map(|(alt, target, len)| {
                (Image { alt: alt.to_owned(), source: target.to_owned(), ..Image::default() }, len)
            })
        } else if c == '<' {
            IMG_TAG
                .captures(rest)
                .map(|tag| (image_of(tag.get(1).map_or("", |m| m.as_str())), tag.get(0).map_or(1, |m| m.end())))
        } else {
            None
        };
        match found.filter(|_| images.len() < PLACEHOLDERS) {
            Some((image, len)) => {
                out.extend(char::from_u32(PLACEHOLDER + images.len() as u32));
                images.push(image);
                i += len;
            }
            None => {
                out.push(c);
                i += c.len_utf8();
            }
        }
    }
}

/// `text` with the images outside code replaced by placeholders, and the images.
fn mark_images(text: &str) -> (String, Vec<Image>) {
    let mut out = String::with_capacity(text.len());
    let mut images = Vec::new();
    let mut fence: Option<String> = None;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        if let Some(open) = &fence {
            let closing = line.trim();
            let marker = open.chars().next().unwrap_or('`');
            if closing.len() >= open.len() && closing.chars().all(|c| c == marker) {
                fence = None;
            }
            out.push_str(line);
        } else if let Some(found) = FENCE.captures(line) {
            fence = found.get(1).map(|m| m.as_str().to_owned());
            out.push_str(line);
        } else if indent_of(line) >= 4 {
            out.push_str(line);
        } else {
            mark_line(line, &mut out, &mut images);
        }
    }
    (out, images)
}

fn placeholder(c: char) -> Option<usize> {
    let code = u32::from(c);
    (PLACEHOLDER..PLACEHOLDER + PLACEHOLDERS as u32).contains(&code).then(|| (code - PLACEHOLDER) as usize)
}

/// `runs` with the placeholders turned back into images.
fn with_images(runs: Vec<Run>, images: &[Image]) -> Vec<Inline> {
    let mut out = Vec::new();
    for run in runs {
        let mut text = String::new();
        for c in run.text.chars() {
            match placeholder(c).and_then(|index| images.get(index)) {
                Some(image) => {
                    if !text.is_empty() {
                        out.push(Inline::Text(Run { text: std::mem::take(&mut text), style: run.style.clone() }));
                    }
                    out.push(Inline::Image(image.clone()));
                }
                None => text.push(c),
            }
        }
        if !text.is_empty() {
            out.push(Inline::Text(Run { text, style: run.style }));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Nodes

/// Heading anchors already given in the document.
#[derive(Default)]
struct Anchors {
    seen: HashMap<String, usize>,
}

impl Anchors {
    fn next(&mut self, base: String) -> String {
        let n = self.seen.entry(base.clone()).or_insert(0);
        let anchor = if *n == 0 { format!("{ANCHOR_PREFIX}{base}") } else { format!("{ANCHOR_PREFIX}{base}-{n}") };
        *n += 1;
        anchor
    }
}

fn is_mermaid(language: Option<&str>) -> bool {
    language.and_then(|info| info.split_whitespace().next()).is_some_and(|word| word.eq_ignore_ascii_case("mermaid"))
}

/// A list item, its checkbox read off a leading `[ ]` / `[x]`.
fn task_item(mut content: Vec<Node>) -> ListItem {
    let mut checked = None;
    if let Some(Node::Paragraph(inlines)) = content.first_mut()
        && let Some(Inline::Text(run)) = inlines.first_mut()
        && !run.style.code
    {
        for (prefix, state) in [("[ ]", false), ("[x]", true), ("[X]", true)] {
            if let Some(rest) = run.text.strip_prefix(prefix)
                && (rest.is_empty() || rest.starts_with(' '))
            {
                checked = Some(state);
                run.text = rest.trim_start_matches(' ').to_owned();
                break;
            }
        }
        if run.text.is_empty() {
            inlines.remove(0);
        }
    }
    ListItem { checked, content }
}

fn convert(block: Block, images: &[Image], anchors: Option<&mut Anchors>) -> Node {
    let cells = |cells: Vec<Vec<Run>>| -> Vec<Vec<Inline>> {
        cells.into_iter().map(|cell| with_images(cell, images)).collect()
    };
    match block {
        Block::Paragraph(runs) => Node::Paragraph(with_images(runs, images)),
        Block::Heading { level, content } => {
            let content = with_images(content, images);
            let anchor = anchors.map(|anchors| anchors.next(slugify(&inline_text(&content))));
            Node::Heading { level, anchor, content }
        }
        Block::Code { language, text } if is_mermaid(language.as_deref()) => {
            Node::Mermaid { source: text.trim_end_matches('\n').to_owned() }
        }
        Block::Code { language, text } => Node::Code { language, text },
        Block::Quote(inner) => Node::Quote(inner.into_iter().map(|block| convert(block, images, None)).collect()),
        Block::List { ordered, items } => Node::List {
            ordered,
            items: items
                .into_iter()
                .map(|item| task_item(item.into_iter().map(|block| convert(block, images, None)).collect()))
                .collect(),
        },
        Block::Table { header, rows } => {
            Node::Table { header: cells(header), rows: rows.into_iter().map(cells).collect() }
        }
        Block::Rule => Node::Rule,
        Block::Plain(text) => Node::Paragraph(vec![Inline::Text(Run { text, style: Style::default() })]),
    }
}

/// The nodes of Markdown `text`; headings get anchors when `anchors` is given.
fn markdown_nodes(text: &str, setext: Option<u8>, mut anchors: Option<&mut Anchors>) -> Vec<Node> {
    let source = match setext {
        Some(level) => {
            let lines: Vec<&str> = text.split('\n').collect();
            let content: Vec<&str> = lines[..lines.len().saturating_sub(1)].iter().map(|line| line.trim()).collect();
            format!("{} {}", "#".repeat(usize::from(level)), content.join(" "))
        }
        None => text.to_owned(),
    };
    let (marked, images) = mark_images(&source);
    docs::parse_markdown(&marked).into_iter().map(|block| convert(block, &images, anchors.as_deref_mut())).collect()
}

/// Elements whose content is dropped with them.
const DROPPED_CONTENT: &[&str] = &[
    "style", "script", "iframe", "noscript", "template", "textarea", "title", "object", "head", "svg", "math",
    "frameset", "noembed", "xmp",
];
/// Elements dropped on their own.
const DROPPED: &[&str] = &["link", "meta", "base", "embed", "input", "frame"];
/// Elements kept, with [`KEPT_ATTRIBUTES`]; others leave their text to the documentation parser.
const KEPT: &[&str] = &[
    "article",
    "aside",
    "blockquote",
    "center",
    "details",
    "summary",
    "div",
    "dl",
    "dt",
    "dd",
    "figure",
    "figcaption",
    "footer",
    "header",
    "main",
    "nav",
    "ol",
    "ul",
    "li",
    "section",
    "table",
    "thead",
    "tbody",
    "tfoot",
    "tr",
    "th",
    "td",
    "p",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "pre",
    "hr",
    "caption",
];
const KEPT_ATTRIBUTES: &[&str] = &["align", "open", "colspan", "rowspan"];
const VOID: &[&str] = &["hr"];

/// The preview being built: the current block's nodes and the elements open in it.
#[derive(Default)]
struct Tree {
    line: Option<u32>,
    nodes: Vec<Node>,
    open: Vec<Element>,
}

impl Tree {
    fn push(&mut self, node: Node) {
        match self.open.last_mut() {
            Some(top) => top.children.push(node),
            None => self.nodes.push(node),
        }
    }

    /// Closes the innermost open `tag` and the elements inside it; a stray close is ignored.
    fn close(&mut self, tag: &str) {
        if let Some(at) = self.open.iter().rposition(|element| element.tag == tag) {
            while self.open.len() > at {
                if let Some(element) = self.open.pop() {
                    self.push(Node::Element(element));
                }
            }
        }
    }

    /// The end of an HTML block: only containers stay open across blocks.
    fn end_chunk(&mut self) {
        while self.open.last().is_some_and(|element| !CONTAINER_TAGS.contains(&element.tag.as_str())) {
            if let Some(element) = self.open.pop() {
                self.push(Node::Element(element));
            }
        }
    }

    fn close_all(&mut self) {
        while let Some(element) = self.open.pop() {
            self.push(Node::Element(element));
        }
    }

    fn flush(&mut self) -> PreviewBlock {
        PreviewBlock { line: self.line.take().unwrap_or(0), nodes: std::mem::take(&mut self.nodes) }
    }

    /// Text between kept tags, read as Markdown (HTML text has no indented code).
    fn text(&mut self, pending: &mut String, anchors: &mut Anchors) {
        if !pending.trim().is_empty() {
            let text = pending.split('\n').map(str::trim_start).collect::<Vec<_>>().join("\n");
            for node in markdown_nodes(&text, None, Some(anchors)) {
                self.push(node);
            }
        }
        pending.clear();
    }

    fn html(&mut self, text: &str, anchors: &mut Anchors) {
        static COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?s)<!--.*?(?:-->|$)"));
        static TAG: LazyLock<Regex> =
            LazyLock::new(|| pattern(r#"<(/?)([a-zA-Z][\w-]*)((?:"[^"]*"|'[^']*'|[^'">])*?)(/?)>"#));
        let text = COMMENT.replace_all(text, "");
        let mut pending = String::new();
        let mut at = 0;
        for found in TAG.captures_iter(&text) {
            let Some(whole) = found.get(0) else { continue };
            if whole.start() < at {
                continue;
            }
            pending.push_str(&text[at..whole.start()]);
            at = whole.end();
            let closing = found.get(1).is_some_and(|m| !m.as_str().is_empty());
            let name = found.get(2).map_or("", |m| m.as_str()).to_ascii_lowercase();
            let attributes_text = found.get(3).map_or("", |m| m.as_str());
            let self_closing = found.get(4).is_some_and(|m| !m.as_str().is_empty());
            if DROPPED_CONTENT.contains(&name.as_str()) {
                if !closing && !self_closing {
                    // Everything up to the closing tag (or the end) goes with the element.
                    let lower = text[at..].to_ascii_lowercase();
                    let end = lower
                        .find(&format!("</{name}"))
                        .map(|close| close + lower[close..].find('>').map_or(lower.len() - close, |gt| gt + 1));
                    at += end.unwrap_or(lower.len());
                }
            } else if DROPPED.contains(&name.as_str()) {
                // A tag without content worth keeping (`<link>`, `<meta>`, `<base>`…).
                continue;
            } else if name == "img" {
                self.text(&mut pending, anchors);
                if !closing {
                    self.push(Node::Image(image_of(attributes_text)));
                }
            } else if KEPT.contains(&name.as_str()) {
                self.text(&mut pending, anchors);
                if closing {
                    self.close(&name);
                } else {
                    let element = Element {
                        attributes: attributes(attributes_text)
                            .into_iter()
                            .filter(|(key, _)| KEPT_ATTRIBUTES.contains(&key.as_str()))
                            .collect(),
                        tag: name,
                        children: Vec::new(),
                    };
                    if self_closing || VOID.contains(&element.tag.as_str()) {
                        self.push(Node::Element(element));
                    } else {
                        self.open.push(element);
                    }
                }
            } else {
                pending.push_str(whole.as_str());
            }
        }
        pending.push_str(text.get(at..).unwrap_or(""));
        self.text(&mut pending, anchors);
    }
}

/// The blocks of `lines`, the first at source line `offset`.
fn build(lines: &[&str], offset: u32, anchors: &mut Anchors) -> Vec<PreviewBlock> {
    let mut out = Vec::new();
    let mut tree = Tree::default();
    for chunk in chunks(lines) {
        tree.line.get_or_insert(offset + chunk.start as u32);
        match chunk.kind {
            Kind::Html => tree.html(&lines[chunk.start..chunk.end].join("\n"), anchors),
            Kind::List(parts) => tree.push(list_node(parts)),
            Kind::Markdown { setext } => {
                let text = lines[chunk.start..chunk.end].join("\n");
                for node in markdown_nodes(&text, setext, Some(&mut *anchors)) {
                    tree.push(node);
                }
            }
        }
        tree.end_chunk();
        if tree.open.is_empty() {
            out.push(tree.flush());
        }
    }
    if tree.line.is_some() {
        tree.close_all();
        out.push(tree.flush());
    }
    out
}

fn list_node(parts: ListParts) -> Node {
    let items = parts
        .items
        .iter()
        .map(|body| {
            let lines: Vec<&str> = body.iter().map(String::as_str).collect();
            // Only top-level headings get anchors.
            let mut own = Anchors::default();
            let nodes = build(&lines, 0, &mut own).into_iter().flat_map(|block| block.nodes).collect();
            task_item(strip_anchors(nodes))
        })
        .collect();
    Node::List { ordered: parts.ordered, items }
}

fn strip_anchors(nodes: Vec<Node>) -> Vec<Node> {
    nodes
        .into_iter()
        .map(|node| match node {
            Node::Heading { level, content, .. } => Node::Heading { level, anchor: None, content },
            other => other,
        })
        .collect()
}

/// The preview of a Markdown file.
pub fn render_markdown(text: &str) -> Vec<PreviewBlock> {
    let text = text.replace("\r\n", "\n");
    let front = front_matter(&text);
    let lines: Vec<&str> = text.split('\n').collect();
    let skip = front.as_ref().map_or(0, |front| front.lines).min(lines.len());
    let mut blocks = Vec::new();
    if let Some(front) = &front {
        blocks.push(PreviewBlock { line: 0, nodes: vec![Node::FrontMatter(front_matter_rows(&front.fields))] });
    }
    blocks.extend(build(&lines[skip..], skip as u32, &mut Anchors::default()));
    blocks
}

fn walk<'a>(nodes: &'a [Node], visit: &mut impl FnMut(&'a Node)) {
    for node in nodes {
        visit(node);
        match node {
            Node::Quote(inner) => walk(inner, visit),
            Node::List { items, .. } => items.iter().for_each(|item| walk(&item.content, visit)),
            Node::Element(element) => walk(&element.children, visit),
            _ => {}
        }
    }
}

/// Every node of the preview, in document order.
pub fn all_nodes(blocks: &[PreviewBlock]) -> Vec<&Node> {
    let mut out = Vec::new();
    for block in blocks {
        walk(&block.nodes, &mut |node| out.push(node));
    }
    out
}

/// The Mermaid sources of the preview, in document order.
pub fn mermaid_sources(blocks: &[PreviewBlock]) -> Vec<&str> {
    all_nodes(blocks)
        .into_iter()
        .filter_map(|node| match node {
            Node::Mermaid { source } => Some(source.as_str()),
            _ => None,
        })
        .collect()
}

pub fn has_mermaid(blocks: &[PreviewBlock]) -> bool {
    !mermaid_sources(blocks).is_empty()
}

/// Whether each `<details>` of the preview is written open, in document order.
pub fn details_written_open(blocks: &[PreviewBlock]) -> Vec<bool> {
    all_nodes(blocks)
        .into_iter()
        .filter_map(|node| match node {
            Node::Element(element) if element.tag == "details" => Some(element.is_open()),
            _ => None,
        })
        .collect()
}

/// The line of the block holding the heading a link's `#id` names.
pub fn anchor_line(blocks: &[PreviewBlock], id: &str) -> Option<u32> {
    let wanted = format!("{ANCHOR_PREFIX}{}", slugify(id));
    blocks.iter().find_map(|block| {
        let mut found = false;
        walk(&block.nodes, &mut |node| {
            if let Node::Heading { anchor: Some(anchor), .. } = node {
                found |= *anchor == wanted;
            }
        });
        found.then_some(block.line)
    })
}

// ---------------------------------------------------------------------------
// Links

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    External { url: String },
    Anchor { id: String },
    File { path: String, anchor: Option<String>, line: Option<u32> },
}

/// The directory of `file` (forward slashes).
fn dir_of(file: &str) -> &str {
    &file[..file.rfind('/').unwrap_or(0)]
}

/// Joins `rel` onto `dir`, resolving `.` and `..`; an absolute `rel` (leading `/`) is kept.
pub fn join_path(dir: &str, rel: &str) -> String {
    let joined = if rel.starts_with('/') { rel.to_owned() } else { format!("{dir}/{rel}") };
    let mut out: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        if part == ".." {
            if out.len() > 1 {
                out.pop();
            }
        } else if part != "." && (!part.is_empty() || out.is_empty()) {
            out.push(part);
        }
    }
    let path = out.join("/");
    if path.is_empty() { "/".to_owned() } else { path }
}

/// Where a link in the Markdown file `file` points; `None` for schemes the preview does not open.
pub fn resolve_link(file: &str, href: &str) -> Option<LinkTarget> {
    static EXTERNAL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^(?:https?|mailto):"));
    static FILE_URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^file://"));
    static SCHEME: LazyLock<Regex> = LazyLock::new(|| pattern(r"(?i)^[a-z][a-z0-9+.-]*:"));
    static LINE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^L(\d+)"));
    let href = href.trim();
    if href.is_empty() {
        return None;
    }
    if EXTERNAL.is_match(href) {
        return Some(LinkTarget::External { url: href.to_owned() });
    }
    if let Some(id) = href.strip_prefix('#') {
        return Some(LinkTarget::Anchor { id: docs::percent_decode(id) });
    }
    let rest = match FILE_URL.find(href) {
        Some(found) => &href[found.end()..],
        None if SCHEME.is_match(href) => return None,
        None => href,
    };
    let (path_part, fragment) = match rest.split_once('#') {
        Some((path, fragment)) => (path, Some(docs::percent_decode(fragment))),
        None => (rest, None),
    };
    let path_part = docs::percent_decode(path_part.split('?').next().unwrap_or(""));
    let path = if path_part.is_empty() { file.to_owned() } else { join_path(dir_of(file), &path_part) };
    if let Some(found) = fragment.as_deref().and_then(|fragment| LINE.captures(fragment)) {
        let number: u32 = found.get(1).map_or("", |m| m.as_str()).parse().unwrap_or(u32::MAX);
        return Some(LinkTarget::File { path, anchor: None, line: Some(number.saturating_sub(1)) });
    }
    Some(LinkTarget::File { path, anchor: fragment.filter(|fragment| !fragment.is_empty()), line: None })
}

// ---------------------------------------------------------------------------
// Scroll sync

/// A top-level block's source line and its offset from the top of the preview.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockTop {
    pub line: f64,
    pub top: f64,
}

/// The preview scroll offset that shows source `line` at the top: interpolated between the blocks
/// around it (`blocks` sorted by line); `end` is the preview's full height.
pub fn preview_offset_for_line(blocks: &[BlockTop], line: f64, end: f64) -> f64 {
    if blocks.is_empty() {
        return 0.0;
    }
    let mut i = 0;
    while i + 1 < blocks.len() && blocks[i + 1].line <= line {
        i += 1;
    }
    let a = &blocks[i];
    if line <= a.line {
        return a.top;
    }
    let b = blocks.get(i + 1);
    let next_line = b.map_or(a.line + 1.0, |b| b.line);
    let next_top = b.map_or(end, |b| b.top);
    let t = ((line - a.line) / (next_line - a.line).max(1.0)).min(1.0);
    a.top + t * (next_top - a.top)
}

/// The source line of the block at the top of a preview scrolled to `offset`: the first block's
/// when none is above it.
pub fn preview_line_at_offset(blocks: &[BlockTop], offset: f64) -> f64 {
    let mut line = blocks.first().map_or(0.0, |block| block.line);
    for block in blocks {
        if block.top > offset + 1.0 {
            break;
        }
        line = block.line;
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(md: &str) -> Vec<(u32, Option<String>)> {
        render_markdown(md).iter().map(|block| (block.line, block.nodes.first().map(Node::tag))).collect()
    }

    fn line_numbers(md: &str) -> Vec<u32> {
        render_markdown(md).iter().map(|block| block.line).collect()
    }

    fn tagged(tags: &[(u32, &str)]) -> Vec<(u32, Option<String>)> {
        tags.iter().map(|(line, tag)| (*line, Some((*tag).to_owned()))).collect()
    }

    fn elements<'a>(nodes: &'a [Node], tag: &str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        walk(nodes, &mut |node| {
            if let Node::Element(element) = node
                && element.tag == tag
            {
                out.push(element);
            }
        });
        out
    }

    fn block_nodes(blocks: &[PreviewBlock]) -> Vec<Node> {
        blocks.iter().flat_map(|block| block.nodes.clone()).collect()
    }

    fn child_text(element: &Element, tag: &str) -> Option<String> {
        element.children.iter().find(|node| node.tag() == tag).map(Node::text)
    }

    #[test]
    fn shows_every_top_level_field_above_the_body_nested_values_as_written() {
        let skill = concat!(
            "---\nname: review\ndescription: Reviews a diff: bugs first\ntools:\n  - Read\n  - Grep\n",
            "meta:\n  owner: team\n---\n\n# Review\n\nBody\n",
        );
        let fields = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
        };
        assert_eq!(
            front_matter(skill),
            Some(FrontMatter {
                fields: fields(&[
                    ("name", "review"),
                    ("description", "Reviews a diff: bugs first"),
                    ("tools", "  - Read\n  - Grep"),
                    ("meta", "  owner: team"),
                ]),
                lines: 9,
            })
        );
        assert_eq!(lines(skill), tagged(&[(0, "TABLE"), (10, "H1"), (12, "P")]));
        let blocks = render_markdown(skill);
        let Some(Node::FrontMatter(rows)) = blocks[0].nodes.first() else { panic!("no front matter: {blocks:?}") };
        assert_eq!(rows[1].key, "description");
        assert_eq!(rows[1].value, FieldValue::Text("Reviews a diff: bugs first".into()));
        assert_eq!(rows[2].value, FieldValue::Nested("- Read\n- Grep".into()));
    }

    #[test]
    fn leaves_a_document_without_front_matter_or_a_thematic_break_alone() {
        assert_eq!(front_matter("# Title\n---\nx: y\n---\n"), None);
        assert_eq!(front_matter("---\nJust a paragraph between rules.\n---\n"), None);
        assert_eq!(front_matter("---\n---\n"), None);
    }

    #[test]
    fn tags_blocks_with_their_source_line() {
        let md = "# Title\n\nPara one\nstill one\n\n- a\n- b\n";
        assert_eq!(lines(md), tagged(&[(0, "H1"), (2, "P"), (5, "UL")]));
        let blocks = render_markdown(md);
        let Some(Node::Heading { anchor, .. }) = blocks[0].nodes.first() else { panic!("no heading") };
        assert_eq!(anchor.as_deref(), Some("user-content-title"));
    }

    #[test]
    fn sanitizes_and_defers_image_loading() {
        let blocks =
            render_markdown("<script>alert(1)</script>\n\n![x](img/a.png) <a href=\"javascript:alert(1)\">j</a>\n");
        let nodes = block_nodes(&blocks);
        assert!(!nodes_text(&nodes).contains("alert"), "{nodes:?}");
        let Some(Node::Paragraph(content)) = nodes.first() else { panic!("no paragraph: {nodes:?}") };
        let images: Vec<&Image> = content
            .iter()
            .filter_map(|inline| match inline {
                Inline::Image(image) => Some(image),
                Inline::Text(_) => None,
            })
            .collect();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].source, "img/a.png");
        assert_eq!(images[0].alt, "x");
        assert!(content.iter().all(|inline| !matches!(inline, Inline::Text(run) if run.style.link.is_some())));
        assert!(inline_text(content).contains('j'));
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
            let nodes = block_nodes(&render_markdown(&format!("text\n\n{raw}\n")));
            let text = nodes_text(&nodes);
            assert!(text.starts_with("text"), "{raw}");
            assert!(!text.contains("outline") && !text.contains('<') && !text.contains("example.com"), "{raw}: {text}");
            let mut kept = Vec::new();
            walk(&nodes, &mut |node| {
                if let Node::Element(element) = node {
                    kept.push(element.clone());
                }
            });
            for element in kept {
                assert!(!["style", "form", "link", "meta", "base", "iframe"].contains(&element.tag.as_str()), "{raw}");
                assert!(element.attributes.iter().all(|(key, _)| key != "style" && key != "formaction"), "{raw}");
            }
        }
    }

    #[test]
    fn keeps_the_raw_html_readmes_rely_on() {
        let nodes = block_nodes(&render_markdown(concat!(
            "<div align=\"center\"><img src=\"a.png\" width=\"20\"></div>\n\n",
            "<details><summary>More</summary>x</details>\n\n- [x] done\n",
        )));
        let div = elements(&nodes, "div");
        assert_eq!(div[0].attribute("align"), Some("center"));
        let Some(Node::Image(image)) = div[0].children.first() else { panic!("no image: {div:?}") };
        assert_eq!(image.width.as_deref(), Some("20"));
        assert_eq!(image.source, "a.png");
        assert!(!elements(&elements(&nodes, "details")[0].children, "summary").is_empty());
        let list = nodes.iter().find_map(|node| match node {
            Node::List { items, .. } => Some(items),
            _ => None,
        });
        let items = list.expect("a task list");
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(nodes_text(&items[0].content), "done");
    }

    #[test]
    fn keeps_markdown_between_a_containers_tags_inside_it() {
        let md = "<details>\n<summary>Summary</summary>\n\nContent\n\n</details>\n\nAfter\n";
        let blocks = render_markdown(md);
        let details = elements(&blocks[0].nodes, "details");
        assert!(!details[0].is_open());
        assert_eq!(child_text(details[0], "SUMMARY").as_deref(), Some("Summary"));
        assert_eq!(child_text(details[0], "P").as_deref(), Some("Content"));
        assert_eq!(blocks[1].nodes[0].tag(), "P");
        assert_eq!(blocks[1].nodes[0].text(), "After");
        assert_eq!(lines(md), tagged(&[(0, "DETAILS"), (7, "P")]));
    }

    #[test]
    fn renders_the_content_of_a_container() {
        type Check = fn(&[Node]) -> bool;
        let cases: [(&str, &str, Check); 6] = [
            ("close right after the text", "<details>\n<summary>S</summary>\n\nContent\n</details>\n", |nodes| {
                elements(nodes, "details").iter().any(|d| child_text(d, "P").is_some())
            }),
            ("kept open", "<details open>\n<summary>S</summary>\n\n- a\n- b\n\n</details>\n", |nodes| {
                elements(nodes, "details").iter().any(|d| {
                    d.is_open() && d.children.iter().any(|n| matches!(n, Node::List { items, .. } if !items.is_empty()))
                })
            }),
            ("a centred README header", "<div align=\"center\">\n\n# Title\n\n</div>\n", |nodes| {
                elements(nodes, "div")
                    .iter()
                    .any(|d| d.attribute("align") == Some("center") && child_text(d, "H1").is_some())
            }),
            ("a section", "<section>\n\n```js\nx\n```\n\n</section>\n", |nodes| {
                elements(nodes, "section").iter().any(|s| s.children.iter().any(|n| matches!(n, Node::Code { .. })))
            }),
            ("upper case", "<DETAILS>\n<SUMMARY>S</SUMMARY>\n\nContent\n\n</DETAILS>\n", |nodes| {
                elements(nodes, "details").iter().any(|d| child_text(d, "P").is_some())
            }),
            ("a list item", "- item\n  <details>\n  <summary>S</summary>\n\n  body\n\n  </details>\n", |nodes| {
                nodes.iter().any(|node| match node {
                    Node::List { items, .. } => items.iter().any(|item| !elements(&item.content, "details").is_empty()),
                    _ => false,
                })
            }),
        ];
        for (name, md, check) in cases {
            let nodes = block_nodes(&render_markdown(md));
            assert!(check(&nodes), "{name}: {nodes:?}");
        }
    }

    #[test]
    fn nests_containers_and_ends_each_at_its_own_close() {
        let md = concat!(
            "<details>\n<summary>A</summary>\n\n<details>\n<summary>B</summary>\n\ninner\n\n</details>\n\n",
            "outer\n\n</details>\n\nafter\n",
        );
        let blocks = render_markdown(md);
        let outer = elements(&blocks[0].nodes, "details")[0];
        let inner = elements(&outer.children, "details");
        assert_eq!(inner.len(), 1);
        assert_eq!(child_text(inner[0], "P").as_deref(), Some("inner"));
        assert_eq!(child_text(outer, "P").as_deref(), Some("outer"));
        assert_eq!(lines(md), tagged(&[(0, "DETAILS"), (14, "P")]));
    }

    #[test]
    fn leaves_blocks_alone_when_the_container_is_closed_in_its_own_block_or_only_mentioned() {
        assert_eq!(lines("<details><summary>S</summary>x</details>\n\nNext\n"), tagged(&[(0, "DETAILS"), (2, "P")]));
        assert_eq!(line_numbers("<!-- <details> -->\n\nOne\n\nTwo\n"), [0, 2, 4]);
        assert_eq!(line_numbers("```html\n<details>\n```\n\nOne\n\n`<div>` code\n\nTwo\n"), [0, 4, 6, 8]);
        assert_eq!(line_numbers("<img src=\"a.png\" alt=\"<div>\">\n\n<br/>\n\nOne\n"), [0, 2, 4]);
    }

    #[test]
    fn keeps_everything_after_a_container_thats_never_closed_inside_it() {
        let md = "Intro\n\n<details>\n<summary>S</summary>\n\nOne\n\nTwo\n";
        let blocks = render_markdown(md);
        let details = elements(&blocks[1].nodes, "details");
        let paragraphs: Vec<String> =
            details[0].children.iter().filter(|node| node.tag() == "P").map(Node::text).collect();
        assert_eq!(paragraphs, ["One", "Two"]);
        assert_eq!(lines(md), tagged(&[(0, "P"), (2, "DETAILS")]));
    }

    #[test]
    fn ignores_a_stray_close() {
        assert_eq!(line_numbers("One\n\n</div>\n\nTwo\n"), [0, 2, 4]);
        let md = "</details>\n\n<details>\n<summary>S</summary>\n\nIn\n\n</details>\n\nOut\n";
        assert_eq!(lines(md), [(0, None), (2, Some("DETAILS".to_owned())), (9, Some("P".to_owned()))]);
    }

    #[test]
    fn dedupes_heading_anchors() {
        let anchors: Vec<Option<String>> = block_nodes(&render_markdown("## Same\n\n## Same\n"))
            .into_iter()
            .filter_map(|node| match node {
                Node::Heading { anchor, .. } => Some(anchor),
                _ => None,
            })
            .collect();
        assert_eq!(anchors, [Some("user-content-same".to_owned()), Some("user-content-same-1".to_owned())]);
        assert_eq!(slugify("Hello, World! v2"), "hello-world-v2");
    }

    #[test]
    fn resolves_links_against_the_file() {
        let file = "/repo/docs/guide.md";
        assert_eq!(resolve_link(file, "https://x.dev"), Some(LinkTarget::External { url: "https://x.dev".into() }));
        assert_eq!(resolve_link(file, "#setup"), Some(LinkTarget::Anchor { id: "setup".into() }));
        assert_eq!(
            resolve_link(file, "../src/main.rs#L12"),
            Some(LinkTarget::File { path: "/repo/src/main.rs".into(), anchor: None, line: Some(11) })
        );
        assert_eq!(
            resolve_link(file, "other%20file.md#intro"),
            Some(LinkTarget::File {
                path: "/repo/docs/other file.md".into(),
                anchor: Some("intro".into()),
                line: None
            })
        );
        assert_eq!(resolve_link(file, "javascript:alert(1)"), None);
        assert_eq!(join_path("C:/repo/docs", "../a.md"), "C:/repo/a.md");
        assert_eq!(join_path("/repo", "../../a.md"), "/a.md");
    }

    #[test]
    fn interpolates_the_preview_offset_between_blocks() {
        let blocks = [BlockTop { line: 0.0, top: 0.0 }, BlockTop { line: 10.0, top: 100.0 }];
        assert_eq!(preview_offset_for_line(&blocks, 5.0, 400.0), 50.0);
        assert_eq!(preview_offset_for_line(&blocks, 10.0, 400.0), 100.0);
        assert_eq!(preview_offset_for_line(&blocks, 10.5, 400.0), 250.0);
        assert_eq!(preview_offset_for_line(&[], 3.0, 400.0), 0.0);
    }

    #[test]
    fn finds_the_block_at_the_top_of_a_scrolled_preview() {
        let blocks = [
            BlockTop { line: 2.0, top: 0.0 },
            BlockTop { line: 10.0, top: 100.0 },
            BlockTop { line: 20.0, top: 300.0 },
        ];
        assert_eq!(preview_line_at_offset(&blocks, 0.0), 2.0);
        assert_eq!(preview_line_at_offset(&blocks, 99.0), 10.0);
        assert_eq!(preview_line_at_offset(&blocks, 299.0), 20.0);
        assert_eq!(preview_line_at_offset(&blocks, 250.0), 10.0);
        assert_eq!(preview_line_at_offset(&blocks, 5000.0), 20.0);
        assert_eq!(preview_line_at_offset(&[], 50.0), 0.0);
    }
}
