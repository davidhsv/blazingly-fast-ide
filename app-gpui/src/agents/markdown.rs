//! Agent replies as Markdown (GitHub flavoured, a single line break is a hard break): blocks of styled
//! runs and images, with no rendering. Replies can quote repository content, so raw HTML never reaches
//! the page: its tags are dropped (a few inline ones become styles, `<img>` an image, `<br>` a break),
//! and the contents of `<style>`, `<script>`, `<iframe>` and the like are dropped with them.
//!
//! Beside the blocks: what a link click does, the Copy controls of code blocks and diagrams, the
//! reads of a message's host images, and the Mermaid drawings shared by every message.

use crate::agents::conversation::{PathLink, file_href, path_link};
use crate::agents::markdown_images::{ImageTarget, image_target};
use crate::platform::i18n::{I18n, Param};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// How a run of text is shown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// The link target as written; see [`link_action`].
    pub link: Option<String>,
}

/// Text of one style; a `"\n"` inside is a hard line break.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

/// An image of a reply: its alternative text and its source as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub alt: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    Text(Run),
    Image(Image),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    /// A fenced or indented code block; `language` is the first word of the fence's info string. Its
    /// text has no trailing line breaks: it is what Copy Code copies.
    Code {
        language: Option<String>,
        text: String,
    },
    /// A `mermaid` fence: drawn as a diagram, its source stays copyable.
    Mermaid {
        source: String,
    },
    Quote(Vec<Block>),
    List {
        /// The number of the first item of an ordered list.
        ordered: Option<u64>,
        /// Each item's blocks: its text is a paragraph of its own, before any nested list.
        items: Vec<Vec<Block>>,
    },
    Table {
        header: Vec<Vec<Inline>>,
        rows: Vec<Vec<Vec<Inline>>>,
    },
    Rule,
}

/// Rendered messages by text, least recently used first: reopening a conversation does not parse its
/// replies again.
#[derive(Debug, Default)]
pub struct MarkdownCache {
    parsed: HashMap<String, (u64, std::sync::Arc<Vec<Block>>)>,
    clock: u64,
}

const CACHE_MAX: usize = 500;

impl MarkdownCache {
    pub fn blocks(&mut self, text: &str) -> std::sync::Arc<Vec<Block>> {
        self.clock += 1;
        let clock = self.clock;
        if let Some((used, blocks)) = self.parsed.get_mut(text) {
            *used = clock;
            return blocks.clone();
        }
        if self.parsed.len() >= CACHE_MAX
            && let Some(oldest) = self.parsed.iter().min_by_key(|(_, (used, _))| *used).map(|(k, _)| k.clone())
        {
            self.parsed.remove(&oldest);
        }
        let blocks = std::sync::Arc::new(parse(text));
        self.parsed.insert(text.to_owned(), (clock, blocks.clone()));
        blocks
    }
}

// ----- blocks -----

/// Leading spaces of a line (a tab counts as four).
fn indent(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// `line` without its first `n` columns of indentation.
fn dedent(line: &str, n: usize) -> &str {
    let mut cols = 0;
    for (i, c) in line.char_indices() {
        if cols >= n || (c != ' ' && c != '\t') {
            return &line[i..];
        }
        cols += if c == '\t' { 4 } else { 1 };
    }
    ""
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

struct Fence {
    marker: char,
    len: usize,
    info: String,
}

fn fence_open(line: &str) -> Option<Fence> {
    if indent(line) > 3 {
        return None;
    }
    let t = line.trim_start();
    let marker = t.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = t.chars().take_while(|c| *c == marker).count();
    let info = t[len..].trim();
    (len >= 3 && !(marker == '`' && info.contains('`'))).then(|| Fence { marker, len, info: info.to_owned() })
}

fn fence_closes(line: &str, fence: &Fence) -> bool {
    let t = line.trim();
    indent(line) <= 3 && t.chars().count() >= fence.len && t.chars().all(|c| c == fence.marker)
}

fn heading(line: &str) -> Option<(u8, &str)> {
    static ATX: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$"));
    let m = ATX.captures(line)?;
    let level = u8::try_from(m.get(1)?.as_str().len()).ok()?;
    Some((level, m.get(2).map_or("", |g| g.as_str())))
}

fn rule(line: &str) -> bool {
    static RULE: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^ {0,3}(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$"));
    RULE.is_match(line)
}

/// A list item's marker: whether it is ordered (with its number), and the column its content starts at.
fn list_marker(line: &str) -> Option<(Option<u64>, usize)> {
    static MARKER: LazyLock<Regex> = LazyLock::new(|| pattern(r"^( {0,3})([-*+]|([0-9]{1,9})[.)])([ \t]+|$)"));
    let m = MARKER.captures(line)?;
    let number = m.get(3).and_then(|n| n.as_str().parse().ok());
    let spaces = m.get(4).map_or(0, |g| g.as_str().len());
    // A marker followed by five or more spaces starts its content after one.
    let gap = if spaces > 4 || spaces == 0 { 1 } else { spaces };
    Some((number, m.get(1).map_or(0, |g| g.as_str().len()) + m.get(2).map_or(0, |g| g.as_str().len()) + gap))
}

fn table_delimiter(line: &str) -> bool {
    static DELIMITER: LazyLock<Regex> =
        LazyLock::new(|| pattern(r"^ {0,3}\|?[ \t]*:?-+:?[ \t]*(\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$"));
    line.contains('|') && DELIMITER.is_match(line)
}

fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').filter(|rest| !rest.ends_with('\\')).unwrap_or(t);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cell.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cell).trim().to_owned()),
            _ => cell.push(c),
        }
    }
    cells.push(cell.trim().to_owned());
    cells
}

const RAW_TEXT_TAGS: [&str; 4] = ["script", "pre", "style", "textarea"];

/// Whether `line` starts a block of raw HTML, and whether that block ends at its closing tag (rather
/// than at a blank line).
fn html_block_start(line: &str) -> Option<Option<&'static str>> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}<(/?)([A-Za-z][A-Za-z0-9-]*)(\s|/?>|$)"));
    let t = line.trim_start();
    if t.starts_with("<!--") {
        return Some(Some("-->"));
    }
    let m = TAG.captures(line)?;
    let opening = m.get(1).is_some_and(|g| g.as_str().is_empty());
    let name = m.get(2)?.as_str().to_lowercase();
    if !opening || !RAW_TEXT_TAGS.contains(&name.as_str()) {
        return Some(None);
    }
    Some(Some(match name.as_str() {
        "script" => "</script>",
        "pre" => "</pre>",
        "style" => "</style>",
        _ => "</textarea>",
    }))
}

/// The rest of `line` after its first `col` columns, whatever they hold (a list item's marker).
fn columns_from(line: &str, col: usize) -> &str {
    let mut cols = 0;
    for (i, c) in line.char_indices() {
        if cols >= col {
            return &line[i..];
        }
        cols += if c == '\t' { 4 } else { 1 };
    }
    ""
}

/// Whether `line` begins a block that interrupts a paragraph.
fn interrupts(line: &str) -> bool {
    fence_open(line).is_some()
        || heading(line).is_some()
        || rule(line)
        || line.trim_start().starts_with('>')
        // An empty item, or a numbered one not starting at 1, does not interrupt a paragraph.
        || list_marker(line).is_some_and(|(n, col)| n.is_none_or(|n| n == 1) && !blank(columns_from(line, col)))
        || html_block_start(line).is_some()
}

/// Parses a reply into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    parse_lines(&lines)
}

fn parse_lines(lines: &[&str]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if blank(line) {
            i += 1;
            continue;
        }
        if let Some(fence) = fence_open(line) {
            let start = i + 1;
            let mut end = start;
            while end < lines.len() && !fence_closes(lines[end], &fence) {
                end += 1;
            }
            let body: Vec<&str> = lines[start..end].iter().map(|l| dedent(l, indent(line).min(3))).collect();
            let text = body.join("\n").trim_end_matches('\n').to_owned();
            let language = fence.info.split_whitespace().next().map(str::to_owned);
            blocks.push(if language.as_deref().is_some_and(|l| l.eq_ignore_ascii_case("mermaid")) {
                Block::Mermaid { source: text }
            } else {
                Block::Code { language, text }
            });
            i = end + 1;
            continue;
        }
        if let Some((level, content)) = heading(line) {
            blocks.push(Block::Heading { level, content: inlines(content) });
            i += 1;
            continue;
        }
        if rule(line) {
            blocks.push(Block::Rule);
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut quoted = Vec::new();
            while i < lines.len() && !blank(lines[i]) {
                let t = lines[i].trim_start();
                let Some(rest) = t.strip_prefix('>') else {
                    // A lazy continuation line of the quoted paragraph.
                    quoted.push(t);
                    i += 1;
                    continue;
                };
                quoted.push(rest.strip_prefix(' ').unwrap_or(rest));
                i += 1;
            }
            blocks.push(Block::Quote(parse_lines(&quoted)));
            continue;
        }
        if let Some((number, _)) = list_marker(line) {
            let (block, next) = list(lines, i, number);
            blocks.push(block);
            i = next;
            continue;
        }
        if let Some(end_tag) = html_block_start(line) {
            let start = i;
            match end_tag {
                Some(end_tag) => {
                    while i < lines.len() && !lines[i].to_lowercase().contains(end_tag) {
                        i += 1;
                    }
                    i = (i + 1).min(lines.len());
                }
                None => {
                    while i < lines.len() && !blank(lines[i]) {
                        i += 1;
                    }
                }
            }
            let content = inlines(&lines[start..i].join("\n"));
            if !content.is_empty() {
                blocks.push(Block::Paragraph(content));
            }
            continue;
        }
        if indent(line) >= 4 {
            let start = i;
            while i < lines.len() && (indent(lines[i]) >= 4 || blank(lines[i])) {
                i += 1;
            }
            let body: Vec<&str> = lines[start..i].iter().map(|l| dedent(l, 4)).collect();
            blocks.push(Block::Code { language: None, text: body.join("\n").trim_end_matches('\n').to_owned() });
            continue;
        }
        if line.contains('|') && i + 1 < lines.len() && table_delimiter(lines[i + 1]) {
            let header = table_cells(line);
            let width = header.len();
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() && !blank(lines[i]) && !interrupts(lines[i]) {
                let mut cells = table_cells(lines[i]);
                cells.resize(width, String::new());
                rows.push(cells.iter().map(|c| inlines(c)).collect());
                i += 1;
            }
            blocks.push(Block::Table { header: header.iter().map(|c| inlines(c)).collect(), rows });
            continue;
        }
        // A paragraph, possibly turned into a heading by an underline.
        let start = i;
        i += 1;
        let mut underline = None;
        while i < lines.len() && !blank(lines[i]) {
            let t = lines[i].trim();
            if indent(lines[i]) <= 3 && !t.is_empty() && (t.chars().all(|c| c == '=') || t.chars().all(|c| c == '-')) {
                underline = Some(if t.starts_with('=') { 1 } else { 2 });
                break;
            }
            if interrupts(lines[i]) {
                break;
            }
            i += 1;
        }
        let text: Vec<&str> = lines[start..i].iter().map(|l| l.trim_start()).collect();
        let content = inlines(text.join("\n").trim_end());
        match underline {
            Some(level) => {
                blocks.push(Block::Heading { level, content });
                i += 1;
            }
            None => blocks.push(Block::Paragraph(content)),
        }
    }
    blocks
}

/// The list starting at line `start`; returns it and the line after it.
fn list(lines: &[&str], start: usize, first: Option<u64>) -> (Block, usize) {
    let ordered = first.is_some();
    let mut items = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some((number, content_col)) = list_marker(lines[i]) else { break };
        if number.is_some() != ordered {
            break;
        }
        let mut item: Vec<&str> = vec![columns_from(lines[i], content_col)];
        i += 1;
        while i < lines.len() {
            let line = lines[i];
            if blank(line) {
                // A blank line stays in the item when the item goes on after it.
                if lines.get(i + 1).is_some_and(|next| !blank(next) && indent(next) >= content_col) {
                    item.push("");
                    i += 1;
                    continue;
                }
                break;
            }
            if indent(line) >= content_col {
                item.push(dedent(line, content_col));
            } else if list_marker(line).is_some() || interrupts(line) {
                break;
            } else {
                // A lazy continuation of the item's paragraph.
                item.push(line.trim_start());
            }
            i += 1;
        }
        items.push(parse_lines(&item));
        // Blank lines between items keep the list going.
        let mut next = i;
        while next < lines.len() && blank(lines[next]) {
            next += 1;
        }
        if next < lines.len() && list_marker(lines[next]).is_some_and(|(n, _)| n.is_some() == ordered) {
            i = next;
        } else {
            break;
        }
    }
    (Block::List { ordered: first, items }, i)
}

// ----- inlines -----

/// Tags whose contents are dropped along with them.
const DROPPED: [&str; 12] = [
    "script", "style", "iframe", "textarea", "title", "noscript", "template", "noembed", "noframes", "xmp", "object",
    "select",
];

#[derive(Default)]
struct HtmlStyle {
    bold: usize,
    italic: usize,
    code: usize,
    strike: usize,
}

struct Inlines {
    out: Vec<Inline>,
    html: HtmlStyle,
}

impl Inlines {
    fn push_text(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        let style = Style {
            bold: style.bold || self.html.bold > 0,
            italic: style.italic || self.html.italic > 0,
            code: style.code || self.html.code > 0,
            strike: style.strike || self.html.strike > 0,
            link: style.link.clone(),
        };
        if let Some(Inline::Text(last)) = self.out.last_mut()
            && last.style == style
        {
            last.text.push_str(text);
            return;
        }
        self.out.push(Inline::Text(Run { text: text.to_owned(), style }));
    }
}

/// The inline content of `text`: emphasis, code spans, links, images, breaks and sanitized HTML.
pub fn inlines(text: &str) -> Vec<Inline> {
    let mut state = Inlines { out: Vec::new(), html: HtmlStyle::default() };
    parse_inline(text, &Style::default(), &mut state);
    state.out
}

fn decode_entities(text: &str) -> String {
    static ENTITY: LazyLock<Regex> = LazyLock::new(|| pattern(r"&(#[0-9]{1,7}|#[xX][0-9A-Fa-f]{1,6}|[A-Za-z]{2,8});"));
    ENTITY
        .replace_all(text, |m: &regex::Captures<'_>| {
            let name = &m[1];
            let decoded = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32).map(String::from)
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse().ok().and_then(char::from_u32).map(String::from)
            } else {
                match name {
                    "amp" => Some("&".to_owned()),
                    "lt" => Some("<".to_owned()),
                    "gt" => Some(">".to_owned()),
                    "quot" => Some("\"".to_owned()),
                    "apos" => Some("'".to_owned()),
                    "nbsp" => Some("\u{a0}".to_owned()),
                    _ => None,
                }
            };
            decoded.unwrap_or_else(|| m[0].to_owned())
        })
        .into_owned()
}

/// The `(destination "title")` after a link's text: the destination and the byte length consumed.
fn link_destination(rest: &str) -> Option<(String, usize)> {
    let inner = rest.strip_prefix('(')?;
    let trimmed = inner.trim_start();
    let lead = inner.len() - trimmed.len();
    let (dest, used) = if let Some(angle) = trimmed.strip_prefix('<') {
        let end = angle.find(['>', '\n'])?;
        (angle[..end].to_owned(), end + 2)
    } else {
        let mut depth = 0usize;
        let mut end = trimmed.len();
        for (i, c) in trimmed.char_indices() {
            match c {
                '(' => depth += 1,
                ')' if depth == 0 => {
                    end = i;
                    break;
                }
                ')' => depth -= 1,
                c if c.is_whitespace() => {
                    end = i;
                    break;
                }
                _ => {}
            }
        }
        (trimmed[..end].to_owned(), end)
    };
    let after = &trimmed[used..];
    let title_rest = after.trim_start();
    let mut close = after.len() - title_rest.len();
    if let Some(quote) = title_rest.chars().next().filter(|c| matches!(c, '"' | '\'' | '(')) {
        let closing = if quote == '(' { ')' } else { quote };
        let end = title_rest[1..].find(closing)? + 2;
        close += end;
        close += after[close..].len() - after[close..].trim_start().len();
    }
    after[close..].starts_with(')').then(|| (decode_entities(&dest), 1 + lead + used + close + 1))
}

/// The end of the bracketed text that starts at `open` (`[`), nested brackets balanced.
fn closing_bracket(text: &str, open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut chars = text[open..].char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '`' => {}
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// A delimiter run of emphasis that can open (followed by a non-space) and the position of its closer.
fn emphasis_close(text: &str, at: usize, delim: &str) -> Option<usize> {
    let after = &text[at + delim.len()..];
    if after.starts_with(char::is_whitespace) || after.is_empty() {
        return None;
    }
    let underscore = delim.starts_with('_');
    if underscore && text[..at].chars().next_back().is_some_and(char::is_alphanumeric) {
        return None;
    }
    let single = delim.len() == 1;
    let bytes = text.as_bytes();
    let mut search = at + delim.len();
    while let Some(found) = text[search..].find(delim) {
        let pos = search + found;
        let before_ws = text[..pos].chars().next_back().is_some_and(char::is_whitespace);
        let marker = delim.as_bytes().first();
        let previous = pos.checked_sub(1).and_then(|p| bytes.get(p));
        let doubled = single && (bytes.get(pos + 1) == marker || previous == marker);
        let followed = underscore && text[pos + delim.len()..].chars().next().is_some_and(char::is_alphanumeric);
        if pos > at + delim.len() && !before_ws && !doubled && !followed {
            return Some(pos);
        }
        search = pos + delim.len();
    }
    None
}

/// A raw HTML tag at the start of `text`: closing, name, attributes and the byte length.
fn html_tag(text: &str) -> Option<(bool, String, String, usize)> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r#"^<(/?)([A-Za-z][A-Za-z0-9-]*)((?:\s+[^\s"'>/=]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?)*)\s*/?>"#,
        )
    });
    let m = TAG.captures(text)?;
    let closing = m.get(1).is_some_and(|g| !g.as_str().is_empty());
    Some((closing, m[2].to_lowercase(), m.get(3).map_or("", |g| g.as_str()).to_owned(), m[0].len()))
}

/// The value of attribute `name` in an HTML tag's attribute text.
fn attribute(attrs: &str, name: &str) -> Option<String> {
    static ATTR: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"([^\s"'>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?"#));
    ATTR.captures_iter(attrs)
        .find(|m| m[1].eq_ignore_ascii_case(name))
        .map(|m| decode_entities(m.get(2).or_else(|| m.get(3)).or_else(|| m.get(4)).map_or("", |g| g.as_str())))
}

fn parse_inline(text: &str, style: &Style, state: &mut Inlines) {
    let mut plain = String::new();
    let flush = |plain: &mut String, state: &mut Inlines| {
        let decoded = decode_entities(plain);
        state.push_text(&decoded, style);
        plain.clear();
    };
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let c = rest.chars().next().unwrap_or_default();
        match c {
            '\\' => {
                let next = rest[1..].chars().next();
                match next {
                    Some('\n') => {
                        plain.push('\n');
                        i += 2;
                    }
                    Some(n) if n.is_ascii_punctuation() => {
                        flush(&mut plain, state);
                        state.push_text(&n.to_string(), style);
                        i += 1 + n.len_utf8();
                    }
                    _ => {
                        plain.push('\\');
                        i += 1;
                    }
                }
            }
            '`' => {
                let run = rest.chars().take_while(|c| *c == '`').count();
                let after = &rest[run..];
                let mut search = 0;
                let mut closed = None;
                while let Some(found) = after[search..].find('`') {
                    let pos = search + found;
                    let len = after[pos..].chars().take_while(|c| *c == '`').count();
                    if len == run {
                        closed = Some(pos);
                        break;
                    }
                    search = pos + len;
                }
                match closed {
                    Some(pos) => {
                        flush(&mut plain, state);
                        let mut code = after[..pos].replace('\n', " ");
                        if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                            code = code[1..code.len() - 1].to_owned();
                        }
                        state.push_text(&code, &Style { code: true, ..style.clone() });
                        i += run + pos + run;
                    }
                    None => {
                        plain.push_str(&rest[..run]);
                        i += run;
                    }
                }
            }
            '!' if rest[1..].starts_with('[') => {
                let open = i + 1;
                if let Some(close) = closing_bracket(text, open)
                    && let Some((source, used)) = link_destination(&text[close + 1..])
                {
                    flush(&mut plain, state);
                    let alt = plain_text(&inlines(&text[open + 1..close]));
                    state.out.push(Inline::Image(Image { alt, source }));
                    i = close + 1 + used;
                } else {
                    plain.push('!');
                    i += 1;
                }
            }
            '[' => {
                if let Some(close) = closing_bracket(text, i)
                    && let Some((href, used)) = link_destination(&text[close + 1..])
                {
                    flush(&mut plain, state);
                    let link = Style { link: Some(href), ..style.clone() };
                    parse_inline(&text[i + 1..close], &link, state);
                    i = close + 1 + used;
                } else {
                    plain.push('[');
                    i += 1;
                }
            }
            '<' => {
                static AUTOLINK: LazyLock<Regex> =
                    LazyLock::new(|| pattern(r"^<([A-Za-z][A-Za-z0-9+.\-]{1,31}:[^\s<>]*)>"));
                if let Some(m) = AUTOLINK.captures(rest) {
                    flush(&mut plain, state);
                    let url = m[1].to_owned();
                    state.push_text(&url, &Style { link: Some(url.clone()), ..style.clone() });
                    i += m[0].len();
                } else if rest.starts_with("<!--") {
                    flush(&mut plain, state);
                    i += rest.find("-->").map_or(rest.len(), |end| end + 3);
                } else if let Some((closing, name, attrs, len)) = html_tag(rest) {
                    flush(&mut plain, state);
                    i += len;
                    if !closing && DROPPED.contains(&name.as_str()) {
                        let end = format!("</{name}");
                        // ASCII lowercasing keeps byte offsets.
                        let lower = text[i..].to_ascii_lowercase();
                        i = match lower.find(&end) {
                            Some(at) => text[i + at..].find('>').map_or(text.len(), |gt| i + at + gt + 1),
                            None => text.len(),
                        };
                        continue;
                    }
                    html_effect(closing, &name, &attrs, style, state);
                } else {
                    plain.push('<');
                    i += 1;
                }
            }
            '*' | '_' | '~' => {
                let doubled = rest[1..].starts_with(c);
                let delim = if doubled { &rest[..2] } else { &rest[..1] };
                let close = if c == '~' && !doubled { None } else { emphasis_close(text, i, delim) };
                match close {
                    Some(close) => {
                        flush(&mut plain, state);
                        let inner = Style {
                            bold: style.bold || (doubled && c != '~'),
                            italic: style.italic || (!doubled && c != '~'),
                            strike: style.strike || c == '~',
                            ..style.clone()
                        };
                        parse_inline(&text[i + delim.len()..close], &inner, state);
                        i = close + delim.len();
                    }
                    None => {
                        plain.push_str(delim);
                        i += delim.len();
                    }
                }
            }
            _ => {
                plain.push(c);
                i += c.len_utf8();
            }
        }
    }
    flush(&mut plain, state);
}

fn html_effect(closing: bool, name: &str, attrs: &str, style: &Style, state: &mut Inlines) {
    let delta = |count: &mut usize| {
        if closing {
            *count = count.saturating_sub(1);
        } else {
            *count += 1;
        }
    };
    match name {
        "b" | "strong" => delta(&mut state.html.bold),
        "i" | "em" => delta(&mut state.html.italic),
        "code" | "kbd" | "samp" | "tt" => delta(&mut state.html.code),
        "s" | "del" | "strike" => delta(&mut state.html.strike),
        "br" if !closing => state.push_text("\n", style),
        "img" if !closing => {
            // Only `src` names the image: `srcset`, `<source>` and data attributes are ignored.
            let source = attribute(attrs, "src").unwrap_or_default();
            let alt = attribute(attrs, "alt").unwrap_or_default();
            state.out.push(Inline::Image(Image { alt, source }));
        }
        _ => {}
    }
}

/// The text of inline content, without its styles.
pub fn plain_text(content: &[Inline]) -> String {
    content
        .iter()
        .map(|i| match i {
            Inline::Text(run) => run.text.as_str(),
            Inline::Image(image) => image.alt.as_str(),
        })
        .collect()
}

// ----- what the blocks offer -----

/// What clicking a link does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkAction {
    /// Opens the URL in the browser.
    External(String),
    /// Opens the file in an editor, at the line and column when given.
    File(PathLink),
    /// Nothing: an anchor, a protocol-relative or unsupported URL, a malformed path.
    None,
}

pub fn link_action(href: &str, cwd: &str) -> LinkAction {
    let web = href.get(..5).is_some_and(|h| h.eq_ignore_ascii_case("http:"))
        || href.get(..6).is_some_and(|h| h.eq_ignore_ascii_case("https:"));
    if web {
        return LinkAction::External(href.to_owned());
    }
    file_href(href, cwd).map_or(LinkAction::None, LinkAction::File)
}

/// The file an inline code span names, when it names one (shown as a link once the file is known to
/// exist; see [`FileProbes`]).
pub fn code_link(run: &Run, cwd: &str) -> Option<PathLink> {
    (run.style.code && run.style.link.is_none()).then(|| path_link(&run.text, cwd)).flatten()
}

/// Whether paths named in replies exist, by path: agents name the same files again and again.
#[derive(Debug, Default)]
pub struct FileProbes {
    known: HashMap<String, Option<bool>>,
}

impl FileProbes {
    /// Whether `path` exists: `None` while unknown; the first ask returns `true` in `probe` and the
    /// caller probes the file, then calls [`FileProbes::answered`].
    pub fn exists(&mut self, path: &str) -> (Option<bool>, bool) {
        match self.known.get(path) {
            Some(known) => (*known, false),
            None => {
                self.known.insert(path.to_owned(), None);
                (None, true)
            }
        }
    }

    pub fn answered(&mut self, path: &str, exists: bool) {
        self.known.insert(path.to_owned(), Some(exists));
    }
}

fn walk<'a>(blocks: &'a [Block], visit: &mut impl FnMut(&'a Block)) {
    for block in blocks {
        visit(block);
        match block {
            Block::Quote(inner) => walk(inner, visit),
            Block::List { items, .. } => items.iter().for_each(|item| walk(item, visit)),
            _ => {}
        }
    }
}

/// The text of each Copy control, in order: one per code block, and one per diagram (whose drawing hides
/// its code block).
pub fn copy_texts(blocks: &[Block]) -> Vec<&str> {
    let mut out = Vec::new();
    walk(blocks, &mut |block| match block {
        Block::Code { text, .. } => out.push(text.as_str()),
        Block::Mermaid { source } => out.push(source.as_str()),
        _ => {}
    });
    out
}

fn inline_images<'a>(content: &'a [Inline], out: &mut Vec<&'a Image>) {
    out.extend(content.iter().filter_map(|i| match i {
        Inline::Image(image) => Some(image),
        Inline::Text(_) => None,
    }));
}

/// The images of a message, in order.
pub fn images(blocks: &[Block]) -> Vec<&Image> {
    let mut out = Vec::new();
    walk(blocks, &mut |block| match block {
        Block::Paragraph(content) | Block::Heading { content, .. } => inline_images(content, &mut out),
        Block::Table { header, rows } => {
            header.iter().chain(rows.iter().flatten()).for_each(|cell| inline_images(cell, &mut out));
        }
        _ => {}
    });
    out
}

/// The sources of the diagrams of a message, in order.
pub fn diagrams(blocks: &[Block]) -> Vec<&str> {
    let mut out = Vec::new();
    walk(blocks, &mut |block| {
        if let Block::Mermaid { source } = block {
            out.push(source.as_str());
        }
    });
    out
}

// ----- images -----

/// How an image of a message is shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageView {
    /// Shown from this URL (`data:` or web; the request sends no referrer).
    Source(String),
    /// Its host file is being read.
    Loading,
    /// It could not be loaded: no source, and this alternative text.
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Read {
    Pending,
    Done(String),
}

/// The host images one message reads. Reads are reused while the reply streams (the same path is read
/// once); a failed read is reported and tried again on the next render; another message reads again.
#[derive(Debug, Default)]
pub struct MessageImages {
    reads: HashMap<String, Read>,
    /// Sources that failed during the current render.
    failed: HashSet<String>,
    /// Host paths of the images on show.
    shown: HashSet<String>,
}

impl MessageImages {
    /// A render of the message's `blocks`: returns the host files to read (each once), then calls
    /// [`MessageImages::read`] with each answer.
    pub fn render(&mut self, blocks: &[Block], cwd: &str) -> Vec<String> {
        self.failed.clear();
        self.shown.clear();
        let mut out = Vec::new();
        for image in images(blocks) {
            if let Some(ImageTarget::Path(path)) = image_target(&image.source, cwd) {
                self.shown.insert(path.clone());
                if !self.reads.contains_key(&path) {
                    self.reads.insert(path.clone(), Read::Pending);
                    out.push(path);
                }
            }
        }
        out
    }

    /// The answer of a read (a `data:` URL, or the error). Returns whether an image on show changed: an
    /// answer for an image a streamed reply has replaced meanwhile changes nothing.
    pub fn read(&mut self, path: &str, answer: Result<String, String>) -> bool {
        if !self.reads.contains_key(path) {
            return false;
        }
        match answer {
            Ok(url) => {
                self.reads.insert(path.to_owned(), Read::Done(url));
            }
            Err(_) => {
                self.reads.remove(path);
                self.failed.insert(path.to_owned());
            }
        }
        self.shown.contains(path)
    }

    /// A web image failed to load or decode.
    pub fn web_failed(&mut self, source: &str) {
        self.failed.insert(source.to_owned());
    }

    pub fn view(&self, image: &Image, cwd: &str, i18n: &mut I18n) -> ImageView {
        let failed = |i18n: &mut I18n| {
            ImageView::Failed(i18n.tx_built(&["Unable to load image: ", ""], &[Param::from(image.source.as_str())]))
        };
        match image_target(&image.source, cwd) {
            None => failed(i18n),
            Some(ImageTarget::Url(url)) if self.failed.contains(&url) => failed(i18n),
            Some(ImageTarget::Url(url)) => ImageView::Source(url),
            Some(ImageTarget::Path(path)) if self.failed.contains(&path) => failed(i18n),
            Some(ImageTarget::Path(path)) => match self.reads.get(&path) {
                Some(Read::Done(url)) => ImageView::Source(url.clone()),
                _ => ImageView::Loading,
            },
        }
    }
}

// ----- diagrams -----

/// Mermaid drawings, shared by every message: an equal diagram is drawn once.
#[derive(Debug, Default)]
pub struct Diagrams {
    drawn: HashSet<String>,
    drawing: HashSet<String>,
}

impl Diagrams {
    /// A render of a message's diagrams: those not drawn yet are returned, to draw and then report with
    /// [`Diagrams::drawn`]. Drawn ones are in place before the message shows.
    pub fn render(&mut self, sources: &[&str]) -> Vec<String> {
        sources
            .iter()
            .filter(|s| !self.drawn.contains(**s) && self.drawing.insert((**s).to_owned()))
            .map(|s| (*s).to_owned())
            .collect()
    }

    pub fn is_drawn(&self, source: &str) -> bool {
        self.drawn.contains(source)
    }

    /// A drawing finished after the message rendered: returns `true`, telling the conversation it may
    /// follow the end (the message grew).
    pub fn drawn(&mut self, source: &str) -> bool {
        self.drawing.remove(source);
        self.drawn.insert(source.to_owned())
    }
}

#[cfg(test)]
mod tests;
