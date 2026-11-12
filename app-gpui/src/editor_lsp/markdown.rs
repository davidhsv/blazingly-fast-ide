//! Documentation markup (hover, completion docs, signature docs) as blocks of styled runs.
//!
//! Markdown is read into paragraphs, headings, code blocks, lists, quotes and tables whose text is
//! split into runs carrying bold, italic, code, strike-through and a link target. Documentation
//! comes from anyone: raw HTML never reaches the page. Its tags are dropped (a few inline ones
//! become styles), the contents of `<style>`, `<script>` and the like are dropped with them, and
//! a link keeps its target only for the schemes documentation legitimately uses.

use crate::find::utf16;
use ide_lsp::types::Markup;
use regex::Regex;
use std::sync::LazyLock;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// How a run of text is shown.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strike: bool,
    /// The link target; `None` for plain text.
    pub link: Option<String>,
}

/// Text of one style. A `"\n"` inside is a hard line break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Run>),
    Heading {
        level: u8,
        content: Vec<Run>,
    },
    /// A fenced or indented code block; `language` is the fence's info string.
    Code {
        language: Option<String>,
        text: String,
    },
    Quote(Vec<Block>),
    List {
        /// The number of the first item of an ordered list.
        ordered: Option<u64>,
        items: Vec<Vec<Block>>,
    },
    Table {
        header: Vec<Vec<Run>>,
        rows: Vec<Vec<Vec<Run>>>,
    },
    Rule,
    /// Markup that is not Markdown, shown as written.
    Plain(String),
}

pub fn is_empty_markup(markup: Option<&Markup>) -> bool {
    markup.is_none_or(|markup| markup.value.trim().is_empty())
}

/// The blocks of `markup`: parsed Markdown, or the text as written for any other kind.
pub fn markup_blocks(markup: &Markup) -> Vec<Block> {
    if markup.kind == "markdown" { parse_markdown(&markup.value) } else { vec![Block::Plain(markup.value.clone())] }
}

/// The text of `blocks` without styles, blocks separated by line breaks.
pub fn blocks_text(blocks: &[Block]) -> String {
    fn runs_text(runs: &[Run]) -> String {
        runs.iter().map(|run| run.text.as_str()).collect()
    }
    let mut parts = Vec::new();
    for block in blocks {
        parts.push(match block {
            Block::Paragraph(runs) | Block::Heading { content: runs, .. } => runs_text(runs),
            Block::Code { text, .. } | Block::Plain(text) => text.clone(),
            Block::Quote(inner) => blocks_text(inner),
            Block::List { items, .. } => items.iter().map(|item| blocks_text(item)).collect::<Vec<_>>().join("\n"),
            Block::Table { header, rows } => std::iter::once(header)
                .chain(rows)
                .map(|row| row.iter().map(|cell| runs_text(cell)).collect::<Vec<_>>().join("\t"))
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Rule => String::new(),
        });
    }
    parts.join("\n")
}

/// Every link target in `blocks`, in order.
pub fn block_links(blocks: &[Block]) -> Vec<String> {
    fn of_runs(runs: &[Run], out: &mut Vec<String>) {
        for run in runs {
            if let Some(link) = &run.style.link
                && out.last() != Some(link)
            {
                out.push(link.clone());
            }
        }
    }
    fn walk(blocks: &[Block], out: &mut Vec<String>) {
        for block in blocks {
            match block {
                Block::Paragraph(runs) | Block::Heading { content: runs, .. } => of_runs(runs, out),
                Block::Quote(inner) => walk(inner, out),
                Block::List { items, .. } => items.iter().for_each(|item| walk(item, out)),
                Block::Table { header, rows } => {
                    std::iter::once(header).chain(rows).flatten().for_each(|cell| of_runs(cell, out));
                }
                Block::Code { .. } | Block::Rule | Block::Plain(_) => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, &mut out);
    out
}

// ---------------------------------------------------------------------------
// Blocks

static FENCE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}(`{3,}|~{3,})[ \t]*([^\s`]*)[^\n]*$"));
static HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$"));
static RULE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^ {0,3}(?:(?:\*[ \t]*){3,}|(?:-[ \t]*){3,}|(?:_[ \t]*){3,})$"));
static LIST_ITEM: LazyLock<Regex> = LazyLock::new(|| pattern(r"^( *)([-*+]|[0-9]{1,9}[.)])(?:[ \t]+(.*))?$"));
static TABLE_RULE: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$"));

fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Whether `line` starts a block that interrupts a paragraph.
fn starts_block(line: &str) -> bool {
    FENCE.is_match(line)
        || HEADING.is_match(line)
        || RULE.is_match(line)
        || line.trim_start().starts_with('>')
        || LIST_ITEM.captures(line).is_some_and(|found| found.get(3).is_some())
}

fn table_cells(line: &str) -> Vec<String> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let mut cells = Vec::new();
    let mut cell = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cell.push('|');
            chars.next();
        } else if c == '|' {
            cells.push(std::mem::take(&mut cell).trim().to_owned());
        } else {
            cell.push(c);
        }
    }
    if !cell.trim().is_empty() {
        cells.push(cell.trim().to_owned());
    }
    cells
}

/// Markdown as blocks.
pub fn parse_markdown(source: &str) -> Vec<Block> {
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    parse_blocks(&lines)
}

fn parse_blocks(lines: &[&str]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_blank(line) {
            i += 1;
            continue;
        }
        if let Some(found) = FENCE.captures(line) {
            let fence = found.get(1).map_or("", |m| m.as_str());
            let marker = fence.chars().next().unwrap_or('`');
            let language = found.get(2).map(|m| m.as_str()).filter(|info| !info.is_empty()).map(str::to_owned);
            let indent = indent_of(line);
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() {
                let closing = lines[i].trim();
                if closing.len() >= fence.len() && closing.chars().all(|c| c == marker) && indent_of(lines[i]) <= 3 {
                    i += 1;
                    break;
                }
                let strip = indent_of(lines[i]).min(indent);
                body.push(&lines[i][strip..]);
                i += 1;
            }
            out.push(Block::Code { language, text: body.join("\n") });
            continue;
        }
        if let Some(found) = HEADING.captures(line) {
            let level = found.get(1).map_or(1, |m| m.as_str().len()) as u8;
            out.push(Block::Heading { level, content: parse_inline(found.get(2).map_or("", |m| m.as_str())) });
            i += 1;
            continue;
        }
        if RULE.is_match(line) {
            out.push(Block::Rule);
            i += 1;
            continue;
        }
        if line.trim_start().starts_with('>') {
            let mut inner = Vec::new();
            while i < lines.len() && !is_blank(lines[i]) {
                let text = lines[i].trim_start();
                match text.strip_prefix('>') {
                    Some(rest) => inner.push(rest.strip_prefix(' ').unwrap_or(rest)),
                    // A lazy continuation line of the quoted paragraph.
                    None if !starts_block(lines[i]) => inner.push(text),
                    None => break,
                }
                i += 1;
            }
            out.push(Block::Quote(parse_blocks(&inner)));
            continue;
        }
        if let Some(found) = LIST_ITEM.captures(line)
            && found.get(3).is_some()
        {
            let (list, next) = parse_list(lines, i);
            out.push(list);
            i = next;
            continue;
        }
        if indent_of(line) >= 4 {
            let mut body = Vec::new();
            while i < lines.len() && (indent_of(lines[i]) >= 4 || is_blank(lines[i])) {
                body.push(lines[i].get(4..).unwrap_or(""));
                i += 1;
            }
            while body.last().is_some_and(|last| is_blank(last)) {
                body.pop();
            }
            out.push(Block::Code { language: None, text: body.join("\n") });
            continue;
        }
        if line.contains('|') && lines.get(i + 1).is_some_and(|next| TABLE_RULE.is_match(next) && next.contains('-')) {
            let header: Vec<Vec<Run>> = table_cells(line).iter().map(|cell| parse_inline(cell)).collect();
            if table_cells(lines[i + 1]).len() == header.len() {
                i += 2;
                let mut rows = Vec::new();
                while i < lines.len() && !is_blank(lines[i]) && lines[i].contains('|') {
                    let mut row: Vec<Vec<Run>> = table_cells(lines[i]).iter().map(|cell| parse_inline(cell)).collect();
                    row.resize(header.len(), Vec::new());
                    rows.push(row);
                    i += 1;
                }
                out.push(Block::Table { header, rows });
                continue;
            }
        }
        let mut paragraph = vec![line.trim_start()];
        i += 1;
        while i < lines.len() && !is_blank(lines[i]) && !starts_block(lines[i]) {
            paragraph.push(lines[i].trim_start());
            i += 1;
        }
        let runs = parse_inline(&paragraph.join("\n"));
        if !runs.is_empty() {
            out.push(Block::Paragraph(runs));
        }
    }
    out
}

/// The list starting at line `start`, and the line after it.
fn parse_list(lines: &[&str], start: usize) -> (Block, usize) {
    fn marker(line: &str) -> Option<(usize, String, String)> {
        let found = LIST_ITEM.captures(line)?;
        let indent = found.get(1).map_or(0, |m| m.as_str().len());
        let marker = found.get(2).map_or("", |m| m.as_str()).to_owned();
        Some((indent, marker, found.get(3).map_or("", |m| m.as_str()).to_owned()))
    }
    let (base, first, _) = marker(lines[start]).unwrap_or_default();
    let numbered = first.starts_with(|c: char| c.is_ascii_digit());
    let ordered = numbered.then(|| first.trim_end_matches(['.', ')']).parse().unwrap_or(1));
    let mut items: Vec<Vec<Block>> = Vec::new();
    let mut i = start;
    while i < lines.len() {
        let Some((indent, mark, rest)) = marker(lines[i]) else { break };
        if indent != base || mark.starts_with(|c: char| c.is_ascii_digit()) != numbered {
            break;
        }
        let width = indent + mark.len() + 1;
        let mut body: Vec<String> = vec![rest];
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
            } else if marker(line).is_some_and(|(indent, _, _)| indent <= base) || starts_block(line) {
                break;
            } else {
                body.push(line.trim_start().to_owned());
            }
            i += 1;
        }
        let body: Vec<&str> = body.iter().map(String::as_str).collect();
        items.push(parse_blocks(&body));
        // Blank lines between items do not end the list.
        let mut next = i;
        while next < lines.len() && is_blank(lines[next]) {
            next += 1;
        }
        if next < lines.len() && marker(lines[next]).is_some_and(|(indent, _, _)| indent == base) {
            i = next;
        }
    }
    (Block::List { ordered, items }, i)
}

// ---------------------------------------------------------------------------
// Inline text

/// The link schemes documentation uses, `file:` and library-class `jdt:` links included; a
/// relative target has no scheme and passes.
static ALLOWED_URI: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)^(?:(?:(?:f|ht)tps?|mailto|tel|file|jdt):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))"));
static AUTOLINK: LazyLock<Regex> = LazyLock::new(|| pattern(r"^<([A-Za-z][A-Za-z0-9+.\-]{1,31}:[^\s<>]*)>"));
static BARE_URL: LazyLock<Regex> = LazyLock::new(|| pattern(r"^https?://[^\s<]+"));
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        r#"^<(/?)([A-Za-z][A-Za-z0-9-]*)((?:\s+[^\s"'=<>`/]+(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?)*)\s*(/?)>"#,
    )
});
static HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| pattern(r"^<!--(?s:.*?)-->"));
static HREF: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(?i)\shref\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#));
static ENTITY: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^&(#[0-9]{1,7}|#[xX][0-9a-fA-F]{1,6}|[A-Za-z][A-Za-z0-9]{1,31});"));

/// Elements whose contents are dropped with their tags.
const DROPPED_CONTENT: [&str; 9] =
    ["style", "script", "iframe", "noscript", "template", "textarea", "title", "object", "head"];

fn allowed_link(target: &str) -> Option<String> {
    let target = target.trim();
    (!target.is_empty() && ALLOWED_URI.is_match(target)).then(|| target.to_owned())
}

fn entity(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(decimal) = name.strip_prefix('#') {
        return decimal.parse().ok().and_then(char::from_u32);
    }
    Some(match name {
        "lt" => '<',
        "gt" => '>',
        "amp" => '&',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{a0}',
        "copy" => '©',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "rarr" => '→',
        "larr" => '←',
        _ => return None,
    })
}

struct Inline {
    runs: Vec<Run>,
}

impl Inline {
    fn push(&mut self, text: &str, style: &Style) {
        if text.is_empty() {
            return;
        }
        match self.runs.last_mut() {
            Some(last) if last.style == *style => last.text.push_str(text),
            _ => self.runs.push(Run { text: text.to_owned(), style: style.clone() }),
        }
    }
}

/// The byte index of the closing `delimiter` of an emphasis opened just before `text`.
fn closing(text: &str, delimiter: &str) -> Option<usize> {
    let mut at = 0;
    while let Some(found) = text[at..].find(delimiter) {
        let index = at + found;
        let before = text[..index].chars().next_back();
        let escaped = before == Some('\\');
        let after = text[index + delimiter.len()..].chars().next();
        let word_inside = delimiter.starts_with('_') && after.is_some_and(|c| c.is_alphanumeric());
        // A longer run of the same character is another delimiter (`**` inside `*…*`).
        let longer = delimiter.len() == 1 && after.is_some_and(|c| delimiter.starts_with(c));
        if index > 0 && !escaped && before.is_some_and(|c| !c.is_whitespace()) && !word_inside && !longer {
            return Some(index);
        }
        at = index + delimiter.len() + usize::from(longer);
        if at > text.len() || !text.is_char_boundary(at) {
            return None;
        }
    }
    None
}

/// The parts of `[text](target "title")` at the start of `text`: the label, the target and the
/// length of the whole link in bytes.
fn link_at(text: &str) -> Option<(&str, &str, usize)> {
    let mut depth = 0;
    let mut close = None;
    let mut chars = text.char_indices();
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
    let rest = text[close + 1..].strip_prefix('(')?;
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
            '\n' => return None,
            _ => {}
        }
    }
    let end = end?;
    let inside = rest[..end].trim();
    let target = match inside.strip_prefix('<') {
        Some(angled) => angled.split('>').next().unwrap_or(""),
        None => inside.split([' ', '\t']).next().unwrap_or(""),
    };
    Some((&text[1..close], target, close + 2 + end + 1))
}

fn parse_inline(text: &str) -> Vec<Run> {
    let mut out = Inline { runs: Vec::new() };
    inline_into(text, &Style::default(), &mut out);
    // Line ends carry no spaces of their own.
    if let Some(last) = out.runs.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
    }
    out.runs.retain(|run| !run.text.is_empty());
    out.runs
}

fn inline_into(text: &str, style: &Style, out: &mut Inline) {
    let mut plain = String::new();
    let mut i = 0;
    // HTML elements open at this point: the styles to go back to when they close.
    let mut html: Vec<(String, Style)> = Vec::new();
    let mut style = style.clone();
    while i < text.len() {
        let rest = &text[i..];
        let Some(c) = rest.chars().next() else { break };
        let previous = text[..i].chars().next_back();
        macro_rules! flush {
            () => {
                out.push(&plain, &style);
                plain.clear();
            };
        }
        match c {
            '\\' => {
                let next = rest[1..].chars().next();
                if next == Some('\n') {
                    plain.push('\n');
                    i += 2;
                } else if let Some(next) = next.filter(char::is_ascii_punctuation) {
                    plain.push(next);
                    i += 1 + next.len_utf8();
                } else {
                    plain.push('\\');
                    i += 1;
                }
            }
            '`' => {
                let ticks = rest.chars().take_while(|c| *c == '`').count();
                let fence = &rest[..ticks];
                let body = &rest[ticks..];
                let mut close = None;
                let mut at = 0;
                while let Some(found) = body[at..].find(fence) {
                    let index = at + found;
                    let run = body[index..].chars().take_while(|c| *c == '`').count();
                    if run == ticks {
                        close = Some(index);
                        break;
                    }
                    at = index + run;
                }
                match close {
                    Some(close) => {
                        flush!();
                        let mut code = body[..close].replace('\n', " ");
                        if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') && !code.trim().is_empty() {
                            code = code[1..code.len() - 1].to_owned();
                        }
                        out.push(&code, &Style { code: true, ..style.clone() });
                        i += ticks + close + ticks;
                    }
                    None => {
                        plain.push_str(fence);
                        i += ticks;
                    }
                }
            }
            '*' | '_' | '~' => {
                let run = rest.chars().take_while(|other| *other == c).count();
                // `***` opens bold and italic at once.
                let size = if c == '~' { 2 } else { run.min(3) };
                let delimiter = &rest[..size.min(run)];
                let body = &rest[delimiter.len()..];
                let opens = run >= size
                    && body.chars().next().is_some_and(|next| !next.is_whitespace())
                    && (c != '_' || previous.is_none_or(|p| !p.is_alphanumeric()));
                match opens.then(|| closing(body, delimiter)).flatten() {
                    Some(close) => {
                        flush!();
                        let inner = Style {
                            bold: style.bold || (c != '~' && size >= 2),
                            italic: style.italic || (c != '~' && size != 2),
                            strike: style.strike || c == '~',
                            ..style.clone()
                        };
                        inline_into(&body[..close], &inner, out);
                        i += delimiter.len() + close + delimiter.len();
                    }
                    None => {
                        plain.push_str(delimiter);
                        i += delimiter.len();
                    }
                }
            }
            '!' if rest[1..].starts_with('[') => match link_at(&rest[1..]) {
                // An image shows as its alternative text.
                Some((label, _, len)) => {
                    plain.push_str(label);
                    i += 1 + len;
                }
                None => {
                    plain.push('!');
                    i += 1;
                }
            },
            '[' => match link_at(rest) {
                Some((label, target, len)) => {
                    flush!();
                    inline_into(label, &Style { link: allowed_link(target), ..style.clone() }, out);
                    i += len;
                }
                None => {
                    plain.push('[');
                    i += 1;
                }
            },
            '<' => {
                if let Some(found) = AUTOLINK.captures(rest) {
                    flush!();
                    let target = found.get(1).map_or("", |m| m.as_str());
                    out.push(target, &Style { link: allowed_link(target), ..style.clone() });
                    i += found.get(0).map_or(1, |m| m.end());
                } else if let Some(found) = HTML_COMMENT.find(rest) {
                    i += found.end();
                } else if let Some(found) = HTML_TAG.captures(rest) {
                    flush!();
                    let closes = found.get(1).is_some_and(|m| !m.as_str().is_empty());
                    let name = found.get(2).map_or("", |m| m.as_str()).to_ascii_lowercase();
                    let attributes = found.get(3).map_or("", |m| m.as_str());
                    let empty = found.get(4).is_some_and(|m| !m.as_str().is_empty());
                    i += found.get(0).map_or(1, |m| m.end());
                    if closes {
                        if let Some(open) = html.iter().rposition(|(open, _)| *open == name) {
                            style = html[open].1.clone();
                            html.truncate(open);
                        }
                    } else if DROPPED_CONTENT.contains(&name.as_str()) {
                        // Everything up to the closing tag (or the end) goes with the element.
                        let lower = text[i..].to_ascii_lowercase();
                        let end = lower
                            .find(&format!("</{name}"))
                            .map(|at| at + lower[at..].find('>').map_or(lower.len() - at, |close| close + 1));
                        i += end.unwrap_or(lower.len());
                    } else if name == "br" {
                        plain.push('\n');
                    } else if !empty {
                        let next = match name.as_str() {
                            "b" | "strong" => Some(Style { bold: true, ..style.clone() }),
                            "i" | "em" => Some(Style { italic: true, ..style.clone() }),
                            "code" | "tt" => Some(Style { code: true, ..style.clone() }),
                            "s" | "del" | "strike" => Some(Style { strike: true, ..style.clone() }),
                            "a" => {
                                let target = HREF.captures(attributes).and_then(|href| {
                                    href.get(1).or_else(|| href.get(2)).or_else(|| href.get(3)).map(|m| m.as_str())
                                });
                                Some(Style { link: target.and_then(allowed_link), ..style.clone() })
                            }
                            _ => None,
                        };
                        if let Some(next) = next {
                            html.push((name, std::mem::replace(&mut style, next)));
                        }
                    }
                } else {
                    plain.push('<');
                    i += 1;
                }
            }
            '&' => match ENTITY.captures(rest) {
                Some(found) => match entity(found.get(1).map_or("", |m| m.as_str())) {
                    Some(decoded) => {
                        plain.push(decoded);
                        i += found.get(0).map_or(1, |m| m.end());
                    }
                    None => {
                        plain.push('&');
                        i += 1;
                    }
                },
                None => {
                    plain.push('&');
                    i += 1;
                }
            },
            'h' if style.link.is_none() && previous.is_none_or(|p| !p.is_alphanumeric()) && BARE_URL.is_match(rest) => {
                flush!();
                let found = BARE_URL.find(rest).map_or("", |m| m.as_str());
                let target = found.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', '\'', '"']);
                out.push(target, &Style { link: allowed_link(target), ..style.clone() });
                i += target.len();
            }
            '\n' => {
                // Two spaces before a line end are a hard break; a single line end is a space.
                if plain.ends_with("  ") {
                    let trimmed = plain.trim_end_matches(' ').len();
                    plain.truncate(trimmed);
                    plain.push('\n');
                } else {
                    let trimmed = plain.trim_end_matches(' ').len();
                    plain.truncate(trimmed);
                    if !plain.ends_with('\n') {
                        plain.push(' ');
                    }
                }
                i += 1;
            }
            _ => {
                plain.push(c);
                i += c.len_utf8();
            }
        }
    }
    out.push(&plain, &style);
}

// ---------------------------------------------------------------------------
// Summaries and links

/// The declaration shown by the brief hover of a navigation key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationSummary {
    Code { code: String, language: Option<String> },
    Text(String),
}

/// The declaration line(s) of hover documentation: the first fenced code block (at most
/// `max_lines` of it), else the first paragraph.
pub fn declaration_summary(markup: &Markup, max_lines: usize) -> Option<DeclarationSummary> {
    static BREAK: LazyLock<Regex> = LazyLock::new(|| pattern(r"\n\s*\n"));
    if markup.kind == "markdown" {
        let lines: Vec<&str> = markup.value.split('\n').collect();
        let fence_of = |line: &str| ["```", "~~~"].into_iter().find(|fence| line.trim_start().starts_with(fence));
        if let Some((open, fence)) = lines.iter().enumerate().find_map(|(i, line)| Some((i, fence_of(line)?)))
            && let Some(len) = lines[open + 1..].iter().position(|line| line.trim_start().starts_with(fence))
        {
            let body = lines[open + 1..open + 1 + len].join("\n");
            if !body.trim().is_empty() {
                let info = lines[open].trim_start()[fence.len()..].trim_start_matches([' ', '\t']);
                let language: String =
                    info.chars().take_while(|c| c.is_ascii_alphanumeric() || "_+#.-".contains(*c)).collect();
                let body_lines: Vec<&str> = body.trim_end().split('\n').collect();
                let mut code = body_lines[..body_lines.len().min(max_lines)].join("\n");
                if body_lines.len() > max_lines {
                    code.push_str("\n…");
                }
                return Some(DeclarationSummary::Code { code, language: (!language.is_empty()).then_some(language) });
            }
        }
    }
    let trimmed = markup.value.trim();
    let paragraph = BREAK.split(trimmed).next().unwrap_or("").trim();
    if paragraph.is_empty() {
        return None;
    }
    Some(DeclarationSummary::Text(if utf16::len(paragraph) > 200 {
        format!("{}…", utf16::slice(paragraph, 0, 199))
    } else {
        paragraph.to_owned()
    }))
}

/// A documentation link to a source position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLink {
    pub path: String,
    pub line: u32,
    pub col: u32,
}

/// `text` with `%XX` escapes decoded (UTF-8, invalid sequences replaced).
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%').then(|| text.get(i + 1..i + 3)).flatten();
        match hex.and_then(|hex| u8::from_str_radix(hex, 16).ok()) {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The 1-based number at the start of `text`, as a 0-based coordinate.
fn zero_based(text: &str) -> Option<u32> {
    let digits: String = text.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| digits.parse::<u32>().unwrap_or(u32::MAX).saturating_sub(1))
}

/// Where a documentation link points in the sources (`file://…#L12,5`, `jdt://…#L42`); `None` for
/// a web link.
pub fn source_link(href: &str) -> Option<SourceLink> {
    if let Some(rest) = href.strip_prefix("file://") {
        let (location, hash) = rest.split_once('#').unwrap_or((rest, ""));
        let location = location.split('?').next().unwrap_or("");
        let path = location.find('/').map_or("/", |slash| &location[slash..]);
        let fragment = hash.strip_prefix('#').unwrap_or(hash);
        let fragment = fragment.strip_prefix('L').unwrap_or(fragment);
        let line = zero_based(fragment);
        let digits = fragment.chars().take_while(char::is_ascii_digit).count();
        let col = line.and_then(|_| fragment[digits..].strip_prefix([',', ':'])).and_then(zero_based);
        return Some(SourceLink { path: percent_decode(path), line: line.unwrap_or(0), col: col.unwrap_or(0) });
    }
    let lower = href.to_ascii_lowercase();
    let web = lower.starts_with("http:") || lower.starts_with("https:") || lower.starts_with("mailto:");
    if ide_core::is_virtual_uri(href) && !web {
        // Library class links (`jdt://contents/…#L42`).
        let (path, hash) = href.split_once('#').map_or((href, None), |(path, hash)| (path, Some(hash)));
        let line = hash.and_then(|hash| zero_based(hash.strip_prefix('L').unwrap_or(hash)));
        return Some(SourceLink { path: path.to_owned(), line: line.unwrap_or(0), col: 0 });
    }
    None
}

/// What opening a documentation link does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocLink {
    /// Open the file in the editor at the position.
    Source(SourceLink),
    /// Open the address in the browser.
    Web(String),
    Ignored,
}

pub fn doc_link(href: &str) -> DocLink {
    if let Some(target) = source_link(href) {
        DocLink::Source(target)
    } else if href.starts_with("http://") || href.starts_with("https://") {
        DocLink::Web(href.to_owned())
    } else {
        DocLink::Ignored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markdown(value: &str) -> Markup {
        Markup { kind: "markdown".into(), value: value.into() }
    }

    fn has_link(blocks: &[Block]) -> bool {
        !block_links(blocks).is_empty()
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
            let blocks = markup_blocks(&markdown(&format!("text\n\n{raw}\n")));
            let text = blocks_text(&blocks);
            assert!(text.starts_with("text"), "{raw}");
            assert!(!text.contains("outline"), "{raw}");
            assert!(!text.contains('<') && !text.contains("example.com"), "{raw}: {text}");
            assert!(!has_link(&blocks), "{raw}");
            assert!(blocks.iter().all(|block| matches!(block, Block::Paragraph(_))), "{raw}");
        }
    }

    #[test]
    fn keeps_the_links_and_tables_comments_and_docs_rely_on() {
        let blocks = markup_blocks(&markdown(
            "[doc](jdt://contents/a.class) [web](https://example.com)\n\n| A |\n| - |\n| 1 |\n",
        ));
        assert_eq!(block_links(&blocks), ["jdt://contents/a.class", "https://example.com"]);
        let Some(Block::Table { header, rows }) = blocks.get(1) else { panic!("no table: {blocks:?}") };
        assert_eq!(header[0][0].text, "A");
        assert_eq!(rows[0][0][0].text, "1");
    }

    #[test]
    fn shows_plain_text_until_the_parser_loads_then_the_rendered_markup() {
        let blocks = markup_blocks(&markdown("**bold** <img src=x onerror=alert(1)>"));
        let [Block::Paragraph(runs)] = blocks.as_slice() else { panic!("one paragraph: {blocks:?}") };
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "bold");
        assert!(runs[0].style.bold);
        assert!(!blocks_text(&blocks).contains("onerror"));
    }

    #[test]
    fn renders_synchronously_once_loaded() {
        let blocks = markup_blocks(&markdown("`x`"));
        assert_eq!(
            blocks,
            [Block::Paragraph(vec![Run { text: "x".into(), style: Style { code: true, ..Style::default() } }])]
        );
        let plain = markup_blocks(&Markup { kind: "plaintext".into(), value: "<b>".into() });
        assert_eq!(plain, [Block::Plain("<b>".into())]);
    }

    // Not from a spec: headings, paragraphs, fences, lists and quotes; unsafe link targets go.
    #[test]
    fn reads_the_blocks_documentation_is_written_in() {
        let blocks = parse_markdown(
            "# Title\n\nSome *text* with `code`,\nwrapped.\n\n```rust\nfn f() {}\n```\n\n- one\n- two\n  more\n\n> quoted\n\n1. first\n",
        );
        assert_eq!(blocks.len(), 6, "{blocks:?}");
        assert!(matches!(&blocks[0], Block::Heading { level: 1, .. }));
        assert_eq!(blocks_text(&blocks[1..2]), "Some text with code, wrapped.");
        assert_eq!(blocks[2], Block::Code { language: Some("rust".into()), text: "fn f() {}".into() });
        let Block::List { ordered: None, items } = &blocks[3] else { panic!("a bullet list: {:?}", blocks[3]) };
        assert_eq!(items.len(), 2);
        assert_eq!(blocks_text(&items[1]), "two more");
        assert!(matches!(&blocks[4], Block::Quote(_)));
        assert!(matches!(&blocks[5], Block::List { ordered: Some(1), .. }));
        // A scheme documentation has no use for loses its target, not its text.
        let unsafe_link = parse_markdown("[x](javascript:alert(1))");
        assert_eq!(blocks_text(&unsafe_link), "x");
        assert!(!has_link(&unsafe_link));
    }

    #[test]
    fn takes_the_first_code_block_of_hover_markdown() {
        assert_eq!(
            declaration_summary(&markdown("```ts\nconst x: number\n```\n---\nDocs"), 6),
            Some(DeclarationSummary::Code { code: "const x: number".into(), language: Some("ts".into()) })
        );
    }

    #[test]
    fn falls_back_to_the_first_paragraph() {
        let plain = Markup { kind: "plaintext".into(), value: "fn main()\n\nmore".into() };
        assert_eq!(declaration_summary(&plain, 6), Some(DeclarationSummary::Text("fn main()".into())));
        assert_eq!(declaration_summary(&markdown("  "), 6), None);
    }

    // Not from a spec: source links open in the editor, web links in the browser.
    #[test]
    fn finds_where_documentation_links_point() {
        assert_eq!(
            source_link("file:///p/src/a%20b.rs#L12,5"),
            Some(SourceLink { path: "/p/src/a b.rs".into(), line: 11, col: 4 })
        );
        assert_eq!(
            source_link("jdt://contents/rt.jar/java.lang/String.class#L42"),
            Some(SourceLink { path: "jdt://contents/rt.jar/java.lang/String.class".into(), line: 41, col: 0 })
        );
        assert_eq!(source_link("https://example.com/#L3"), None);
        assert_eq!(doc_link("https://example.com"), DocLink::Web("https://example.com".into()));
        assert_eq!(doc_link("mailto:a@b.c"), DocLink::Ignored);
    }
}
