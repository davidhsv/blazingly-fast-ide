//! Emmet, as in IDEA's Settings › Editor › Emmet: where abbreviations expand (`ul>li.item*3` + Tab
//! in HTML-like and stylesheet files) and "Update Tag with Emmet" on the enclosing tag. Functions
//! over document text; offsets are UTF-16.
//!
//! Expanding an abbreviation takes an [`EmmetEngine`], which holds the element and property
//! tables; [`update_tag_edits`] reads the single-element abbreviations it needs by itself.

use crate::editor::buffer::{byte_of_col, col_of_byte};
use ide_core::TextChange;
use regex::Regex;
use std::sync::LazyLock;

/// What kind of abbreviations expand at a position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmmetType {
    Markup,
    Stylesheet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmmetSyntax {
    pub kind: EmmetType,
    /// `html`, or the stylesheet dialect: `css`, `scss`, `less`, `sass`.
    pub syntax: &'static str,
}

const MARKUP: EmmetSyntax = EmmetSyntax { kind: EmmetType::Markup, syntax: "html" };

/// Languages with markup expansion; their `<style>` and `<script>` regions are told apart.
const MARKUP_LANGUAGES: [&str; 3] = ["html", "vue", "svelte"];

/// The stylesheet dialect of a language id.
fn stylesheet_syntax(language: &str) -> Option<&'static str> {
    ["css", "scss", "less", "sass"].into_iter().find(|&dialect| dialect == language)
}

/// Whether `language` files expand Emmet abbreviations.
pub fn has_emmet(language: &str) -> bool {
    MARKUP_LANGUAGES.contains(&language) || stylesheet_syntax(language).is_some()
}

/// Characters before the caret that are scanned for context.
const CONTEXT_LIMIT: usize = 200_000;

fn pattern(source: &str) -> Regex {
    Regex::new(source).expect("the pattern is valid")
}

/// Byte offset of the last `<tag` (or `</tag` when `closing`) of `text`, whatever its case.
fn last_tag(text: &str, tag: &str, closing: bool) -> Option<usize> {
    let lead = if closing { "</" } else { "<" };
    text.rmatch_indices(lead).map(|(at, _)| at).find(|&at| {
        let rest = &text[at + lead.len()..];
        rest.get(..tag.len()).is_some_and(|name| name.eq_ignore_ascii_case(tag))
            && !rest[tag.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// The start tag of the `tag` element `before` ends inside of, as `(tag text, attribute text)`.
/// None when the last such element is closed, or its start tag is not complete.
fn open_region<'t>(before: &'t str, tag: &str) -> Option<(&'t str, &'t str)> {
    static HEAD: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"^<[A-Za-z0-9_]+(?-u:\b)((?:[^>"']|"[^"]*"|'[^']*')*)>"#));
    let open = last_tag(before, tag, false)?;
    if last_tag(before, tag, true).is_some_and(|close| open < close) {
        return None;
    }
    let head = HEAD.captures(&before[open..])?;
    Some((head.get(0)?.as_str(), head.get(1).map_or("", |attrs| attrs.as_str())))
}

/// Whether `before` ends inside a rule and outside a comment: where a declaration may start.
fn stylesheet_context(before: &str) -> bool {
    static STRINGS: LazyLock<Regex> = LazyLock::new(|| pattern(r#""[^"\n]*"|'[^'\n]*'"#));
    if before.rfind("/*").is_some_and(|open| before.rfind("*/").is_none_or(|close| open > close)) {
        return false;
    }
    let mut depth = 0u32;
    for c in STRINGS.replace_all(before, "").chars() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    depth > 0
}

/// The Emmet syntax at the end of `before` (the text up to the caret) in a `language` file, or
/// `None` where nothing expands: inside tags, comments and scripts, and outside stylesheet rules.
pub fn syntax_at(language: &str, before: &str) -> Option<EmmetSyntax> {
    static LANG: LazyLock<Regex> = LazyLock::new(|| pattern(r#"(?i)(?-u:\b)lang\s*=\s*["']?([A-Za-z0-9_]+)"#));
    let text = match before.char_indices().rev().nth(CONTEXT_LIMIT - 1) {
        Some((start, _)) => &before[start..],
        None => before,
    };
    if let Some(dialect) = stylesheet_syntax(language) {
        return stylesheet_context(text).then_some(EmmetSyntax { kind: EmmetType::Stylesheet, syntax: dialect });
    }
    if !MARKUP_LANGUAGES.contains(&language) {
        return None;
    }
    let after = |open: Option<usize>, close: Option<usize>| open.is_some_and(|o| close.is_none_or(|c| o > c));
    if after(text.rfind("<!--"), text.rfind("-->")) || open_region(text, "script").is_some() {
        return None;
    }
    if let Some((head, attrs)) = open_region(text, "style") {
        let lang = LANG.captures(attrs).and_then(|found| found.get(1)).map(|lang| lang.as_str().to_lowercase());
        let inner = last_tag(text, "style", false).map_or("", |open| &text[open + head.len()..]);
        let syntax = lang.as_deref().and_then(stylesheet_syntax).unwrap_or("css");
        return stylesheet_context(inner).then_some(EmmetSyntax { kind: EmmetType::Stylesheet, syntax });
    }
    // Not inside a tag (`<div cla|`).
    if after(text.rfind('<'), text.rfind('>')) {
        return None;
    }
    Some(MARKUP)
}

/// A snippet field (`${1:name}`) for one of Emmet's tab stops.
pub fn field(index: u32, placeholder: &str) -> String {
    let mut escaped = String::with_capacity(placeholder.len());
    for c in placeholder.chars() {
        if matches!(c, '{' | '}' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    if escaped.is_empty() { format!("${{{index}}}") } else { format!("${{{index}:{escaped}}}") }
}

/// `text` escaped so that the snippet parser leaves it as it is: `${` and `#{` do not start a field.
pub fn escape_snippet(text: &str) -> String {
    text.replace("${", "$\\{").replace("#{", "#\\{")
}

/// An abbreviation found in a line (UTF-16 columns).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedAbbreviation {
    pub start: u32,
    pub end: u32,
    pub abbreviation: String,
}

/// What expands abbreviations: Emmet's parser with its element and property tables.
pub trait EmmetEngine {
    /// The abbreviation ending at `col` of `line`.
    fn extract(&self, line: &str, col: u32, kind: EmmetType) -> Option<ExtractedAbbreviation>;

    /// `abbreviation` as a snippet template: tab stops written with [`field`], text passed
    /// through [`escape_snippet`], one tab per indent level. `None` for what it cannot expand.
    fn expand(&self, abbreviation: &str, syntax: EmmetSyntax) -> Option<String>;
}

/// An expanded abbreviation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmmetExpansion {
    /// Columns of the abbreviation in the line.
    pub from: u32,
    pub to: u32,
    /// The snippet template to put in its place.
    pub snippet: String,
}

/// The abbreviation ending at `col` of `line` expanded, or `None` when there is none.
pub fn expand_at(engine: &dyn EmmetEngine, line: &str, col: u32, syntax: EmmetSyntax) -> Option<EmmetExpansion> {
    let found = engine.extract(line, col, syntax.kind)?;
    let abbreviation = found.abbreviation.as_str();
    // A bare number or a closing-tag fragment is never meant as an abbreviation.
    if abbreviation.trim().is_empty() || abbreviation.starts_with(|c: char| c.is_ascii_digit() || c == '/') {
        return None;
    }
    let snippet = engine.expand(abbreviation, syntax).filter(|snippet| !snippet.is_empty())?;
    // Stylesheets: unknown words expand to themselves (`foo` → `foo: ;`) or to nothing.
    if syntax.kind == EmmetType::Stylesheet && !snippet.contains([':', ';']) {
        return None;
    }
    Some(EmmetExpansion { from: found.start, to: found.end, snippet })
}

// ---------------------------------------------------------------------------
// Update Tag with Emmet

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    pub name: String,
    /// `None` for a boolean attribute.
    pub value: Option<String>,
}

/// An element's start tag and where its end tag is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRange {
    pub name: String,
    /// Offsets of the start tag `<…>`.
    pub from: u32,
    pub to: u32,
    pub attrs: Vec<Attr>,
    pub self_closing: bool,
    /// Offsets of the name in the matching end tag.
    pub close: Option<(u32, u32)>,
}

const VOID: [&str; 14] =
    ["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];
/// Elements whose content is text, whatever it looks like.
const RAW_TEXT: [&str; 4] = ["script", "style", "textarea", "title"];

/// The attributes written in `text`, the inside of a start tag after its name.
pub fn parse_attrs(text: &str) -> Vec<Attr> {
    static ATTR: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"([^\s=/>"']+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?"#));
    ATTR.captures_iter(text)
        .map(|found| Attr {
            name: found[1].to_owned(),
            value: (2..=4).find_map(|group| found.get(group)).map(|value| value.as_str().to_owned()),
        })
        .collect()
}

/// A tag while the document is scanned: byte offsets.
struct OpenTag {
    name: String,
    from: usize,
    to: usize,
    attrs: Vec<Attr>,
    self_closing: bool,
    close: Option<(usize, usize)>,
}

/// The innermost element whose start tag, content or end tag contains `pos`.
pub fn enclosing_tag(text: &str, pos: u32) -> Option<TagRange> {
    static TAG: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r#"(?s:<!--.*?-->)|<(/?)([A-Za-z][A-Za-z0-9_:.-]*)((?:[^>"']|"[^"]*"|'[^']*')*?)(/?)>"#)
    });
    let pos = byte_of_col(text, pos);
    let mut stack: Vec<OpenTag> = Vec::new();
    let mut best: Option<OpenTag> = None;
    let mut finish = |tag: OpenTag, end: usize| {
        if tag.from <= pos && pos <= end && best.as_ref().is_none_or(|best| tag.from > best.from) {
            best = Some(tag);
        }
    };
    let mut at = 0;
    while let Some(found) = TAG.captures_at(text, at) {
        let whole = found.get(0)?;
        let (from, to) = (whole.start(), whole.end());
        at = to;
        let Some(name) = found.get(2).map(|name| name.as_str()) else { continue };
        if !found[1].is_empty() {
            let Some(i) = stack.iter().rposition(|open| open.name.eq_ignore_ascii_case(name)) else { continue };
            // Unclosed elements end where their parent does.
            for open in stack.drain(i + 1..) {
                finish(open, from);
            }
            if let Some(mut open) = stack.pop() {
                open.close = Some((from + 2, from + 2 + name.len()));
                finish(open, to);
            }
            continue;
        }
        if from > pos && !stack.iter().any(|open| open.from <= pos) {
            break;
        }
        let tag = OpenTag {
            name: name.to_owned(),
            from,
            to,
            attrs: parse_attrs(&found[3]),
            self_closing: !found[4].is_empty(),
            close: None,
        };
        if tag.self_closing || VOID.iter().any(|void| void.eq_ignore_ascii_case(name)) {
            finish(tag, to);
            continue;
        }
        stack.push(tag);
        // Raw text: skip to the end tag.
        if RAW_TEXT.iter().any(|raw| raw.eq_ignore_ascii_case(name))
            && let Some(end) = find_end_tag(&text[to..], name)
        {
            at = to + end;
        }
    }
    for open in stack {
        finish(open, text.len());
    }
    let tag = best?;
    let col = |byte: usize| col_of_byte(text, byte);
    Some(TagRange {
        name: tag.name,
        from: col(tag.from),
        to: col(tag.to),
        attrs: tag.attrs,
        self_closing: tag.self_closing,
        close: tag.close.map(|(from, to)| (col(from), col(to))),
    })
}

/// Byte offset of the first `</name` in `text`, whatever its case.
fn find_end_tag(text: &str, name: &str) -> Option<usize> {
    text.match_indices("</").map(|(at, _)| at).find(|&at| {
        let rest = &text[at + 2..];
        rest.get(..name.len()).is_some_and(|tag| tag.eq_ignore_ascii_case(name))
            && !rest[name.len()..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// A single-element abbreviation: `name.class#id[attr=value]`.
#[derive(Debug, Default, PartialEq, Eq)]
struct ElementAbbreviation {
    name: Option<String>,
    /// In the order written; classes are joined into one `class`.
    attrs: Vec<Attr>,
}

impl ElementAbbreviation {
    fn set(&mut self, name: &str, value: Option<String>) {
        match self.attrs.iter_mut().find(|attr| attr.name == name) {
            Some(attr) => attr.value = value,
            None => self.attrs.push(Attr { name: name.to_owned(), value }),
        }
    }

    fn add_class(&mut self, class: &str) {
        match self.attrs.iter_mut().find(|attr| attr.name == "class") {
            Some(Attr { value: Some(classes), .. }) => {
                classes.push(' ');
                classes.push_str(class);
            }
            Some(attr) => attr.value = Some(class.to_owned()),
            None => self.attrs.push(Attr { name: "class".to_owned(), value: Some(class.to_owned()) }),
        }
    }
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '$' | '@')
}

/// The attributes inside `[…]`: `a=v b="x y" c` (an attribute without a value is empty, one
/// written `c.` is boolean).
fn bracket_attrs(inside: &str, out: &mut ElementAbbreviation) -> Option<()> {
    let chars: Vec<char> = inside.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if name.is_empty() {
            return None;
        }
        if chars.get(i) != Some(&'=') {
            match name.strip_suffix('.') {
                Some(boolean) if !boolean.is_empty() => out.set(boolean, None),
                _ => out.set(&name, Some(String::new())),
            }
            continue;
        }
        i += 1;
        let value: String = match chars.get(i) {
            Some(&quote @ ('"' | '\'')) => {
                let close = chars[i + 1..].iter().position(|&c| c == quote)? + i + 1;
                let value = chars[i + 1..close].iter().collect();
                i = close + 1;
                value
            }
            _ => {
                let start = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                chars[start..i].iter().collect()
            }
        };
        out.set(&name, Some(value));
    }
    Some(())
}

/// Reads `abbreviation` as one element; `None` when it is not one.
fn parse_element(abbreviation: &str) -> Option<ElementAbbreviation> {
    let mut out = ElementAbbreviation::default();
    let name_len = abbreviation.find(|c: char| !is_name_char(c)).unwrap_or(abbreviation.len());
    if name_len > 0 {
        out.name = Some(abbreviation[..name_len].to_owned());
    }
    let mut rest = &abbreviation[name_len..];
    while let Some(sign) = rest.chars().next() {
        rest = &rest[1..];
        match sign {
            '.' | '#' => {
                let len = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
                let (word, after) = rest.split_at(len);
                if word.is_empty() {
                    return None;
                }
                if sign == '.' {
                    out.add_class(word);
                } else {
                    out.set("id", Some(word.to_owned()));
                }
                rest = after;
            }
            '[' => {
                let (inside, after) = rest.split_once(']')?;
                bracket_attrs(inside, &mut out)?;
                rest = after;
            }
            _ => return None,
        }
    }
    Some(out)
}

/// Whether `abbreviation` builds more than one element, or gives it content.
fn is_structural(abbreviation: &str) -> bool {
    let mut in_brackets = false;
    abbreviation.chars().any(|c| {
        match c {
            '[' => in_brackets = true,
            ']' => in_brackets = false,
            _ => {}
        }
        !in_brackets && matches!(c, '>' | '+' | '^' | '*' | '{')
    })
}

/// Edits applying `abbreviation` to `tag`: a leading name renames the element (start and end tag),
/// `.class` adds classes, `#id` and `[attr=value]` set attributes. `None` when the abbreviation is
/// not a single element. Attributes Emmet's own tables would add to a named element (`a` gets
/// `href`) are not added.
pub fn update_tag_edits(tag: &TagRange, abbreviation: &str) -> Option<Vec<TextChange>> {
    let abbreviation = abbreviation.trim();
    if abbreviation.is_empty() || is_structural(abbreviation) {
        return None;
    }
    let element = parse_element(abbreviation)?;
    let renames = abbreviation.starts_with(|c: char| c.is_ascii_alphabetic());
    let name = element.name.filter(|_| renames).unwrap_or_else(|| tag.name.clone());
    let mut attrs = tag.attrs.clone();
    for new in element.attrs {
        let Some(old) = attrs.iter_mut().find(|old| old.name.eq_ignore_ascii_case(&new.name)) else {
            attrs.push(new);
            continue;
        };
        let is_class = new.name.eq_ignore_ascii_case("class");
        old.value = match old.value.take() {
            // Classes are added to the ones the tag has.
            Some(classes) if is_class && !classes.is_empty() => {
                let mut have: Vec<&str> = classes.split_whitespace().collect();
                for class in new.value.as_deref().unwrap_or_default().split_whitespace() {
                    if !have.contains(&class) {
                        have.push(class);
                    }
                }
                Some(have.join(" "))
            }
            _ => new.value,
        };
    }
    let mut start_tag = format!("<{name}");
    for attr in &attrs {
        start_tag.push(' ');
        start_tag.push_str(&attr.name);
        if let Some(value) = &attr.value {
            let quote = if value.contains('"') { '\'' } else { '"' };
            start_tag.push_str(&format!("={quote}{value}{quote}"));
        }
    }
    start_tag.push_str(if tag.self_closing { " />" } else { ">" });
    let mut edits = vec![TextChange { from: tag.from, to: tag.to, insert: start_tag }];
    if let Some((from, to)) = tag.close
        && name != tag.name
    {
        edits.push(TextChange { from, to, insert: name });
    }
    Some(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    const HTML: Option<EmmetSyntax> = Some(MARKUP);

    fn stylesheet(syntax: &'static str) -> Option<EmmetSyntax> {
        Some(EmmetSyntax { kind: EmmetType::Stylesheet, syntax })
    }

    /// `text` with `edits` applied (ASCII documents: offsets are bytes).
    fn apply(text: &str, edits: &[TextChange]) -> String {
        let mut sorted: Vec<&TextChange> = edits.iter().collect();
        sorted.sort_by_key(|edit| std::cmp::Reverse(edit.from));
        let mut out = text.to_owned();
        for edit in sorted {
            out.replace_range(edit.from as usize..edit.to as usize, &edit.insert);
        }
        out
    }

    fn tag_name(text: &str, pos: usize) -> Option<String> {
        enclosing_tag(text, pos as u32).map(|tag| tag.name)
    }

    // emmet context › expands markup in HTML text, not inside tags, comments or scripts
    #[test]
    fn expands_markup_in_html_text_not_inside_tags_comments_or_scripts() {
        assert_eq!(syntax_at("html", "<body>\n  ul>li"), HTML);
        assert_eq!(syntax_at("html", "<div cla"), None);
        assert_eq!(syntax_at("html", "<!-- div"), None);
        assert_eq!(syntax_at("html", "<script>\n  foo"), None);
        assert_eq!(syntax_at("html", "<script></script>\ndiv"), HTML);
        assert_eq!(syntax_at("typescript", "div"), None);
    }

    // emmet context › uses stylesheet syntax inside rules and <style>
    #[test]
    fn uses_stylesheet_syntax_inside_rules_and_style() {
        assert_eq!(syntax_at("css", ".a {\n  m10"), stylesheet("css"));
        assert_eq!(syntax_at("css", "m10"), None);
        assert_eq!(syntax_at("css", ".a { /* m10"), None);
        assert_eq!(syntax_at("html", "<style lang=\"scss\">\n.a { p5"), stylesheet("scss"));
        assert_eq!(syntax_at("html", "<style>\n.a"), None);
    }

    const DOC: &str = "<div class=\"a\" id=\"x\">\n  <p>hi</p>\n</div>";

    // update tag with emmet › finds the innermost element around the caret
    #[test]
    fn finds_the_innermost_element_around_the_caret() {
        let at = |needle: &str| DOC.find(needle).expect("the text is in the document");
        assert_eq!(tag_name(DOC, at("hi")).as_deref(), Some("p"));
        assert_eq!(tag_name(DOC, at("  <p>")).as_deref(), Some("div"));
        assert_eq!(tag_name("<ul><li>a<li>b</ul>", 10).as_deref(), Some("li"));
        assert_eq!(tag_name("<br><span>x</span>", 2).as_deref(), Some("br"));
    }

    // update tag with emmet › renames, adds classes and sets attributes
    #[test]
    fn renames_adds_classes_and_sets_attributes() {
        let tag = enclosing_tag(DOC, 3).expect("the div");
        let edits = update_tag_edits(&tag, "section.b#y[data-k=v]").expect("a single element");
        assert_eq!(apply(DOC, &edits), "<section class=\"a b\" id=\"y\" data-k=\"v\">\n  <p>hi</p>\n</section>");
        let edits = update_tag_edits(&tag, ".c").expect("a single element");
        assert_eq!(apply(DOC, &edits), "<div class=\"a c\" id=\"x\">\n  <p>hi</p>\n</div>");
        assert_eq!(update_tag_edits(&tag, "ul>li"), None);
    }

    /// An engine that knows two abbreviations and takes the word before the column as one.
    struct TwoWords;

    impl EmmetEngine for TwoWords {
        fn extract(&self, line: &str, col: u32, _: EmmetType) -> Option<ExtractedAbbreviation> {
            let before = &line[..byte_of_col(line, col)];
            let word = before.rsplit(' ').next().unwrap_or_default();
            let start = col - word.encode_utf16().count() as u32;
            Some(ExtractedAbbreviation { start, end: col, abbreviation: word.to_owned() })
        }

        fn expand(&self, abbreviation: &str, _: EmmetSyntax) -> Option<String> {
            match abbreviation {
                "a" => Some(format!("<a href=\"{}\">{}</a>", field(1, ""), field(2, ""))),
                "m10" => Some("margin: 10px;".to_owned()),
                "foo" => Some("foo".to_owned()),
                _ => None,
            }
        }
    }

    // Not from a spec: what an engine expands replaces the abbreviation; numbers, closing-tag
    // fragments, unknown words and stylesheet words that expand to themselves do not expand.
    #[test]
    fn expands_what_the_engine_knows_and_nothing_else() {
        let css = EmmetSyntax { kind: EmmetType::Stylesheet, syntax: "css" };
        let expanded = expand_at(&TwoWords, "  m10", 5, css).expect("a known property");
        assert_eq!(expanded, EmmetExpansion { from: 2, to: 5, snippet: "margin: 10px;".to_owned() });
        let link = expand_at(&TwoWords, "a", 1, MARKUP).map(|expansion| expansion.snippet);
        assert_eq!(link.as_deref(), Some("<a href=\"${1}\">${2}</a>"));
        assert_eq!(expand_at(&TwoWords, "  123", 5, css), None);
        assert_eq!(expand_at(&TwoWords, "  /div", 6, MARKUP), None);
        assert_eq!(expand_at(&TwoWords, "  foo", 5, css), None);
        assert_eq!(expand_at(&TwoWords, "  bar", 5, MARKUP), None);
        assert_eq!(expand_at(&TwoWords, "x ", 2, MARKUP), None);
        assert_eq!(field(3, "a{b}"), "${3:a\\{b\\}}");
        assert_eq!(escape_snippet("x#{y} ${z}"), "x#\\{y} $\\{z}");
    }

    // Not from a spec: attributes are read with any quoting, raw text hides tags, a self-closing
    // tag stays one, and attribute values and offsets survive text outside ASCII.
    #[test]
    fn reads_attributes_raw_text_and_self_closing_tags() {
        let attrs = parse_attrs(" a=\"1 2\" b='x' c=y disabled");
        let pairs: Vec<(&str, Option<&str>)> = attrs.iter().map(|a| (a.name.as_str(), a.value.as_deref())).collect();
        assert_eq!(pairs, [("a", Some("1 2")), ("b", Some("x")), ("c", Some("y")), ("disabled", None)]);
        let script = "<div><script>if (a < b) { x = '<p>'; }</script><i>é</i></div>";
        assert_eq!(tag_name(script, script.find("x =").unwrap_or_default()).as_deref(), Some("script"));
        let img = "é <img src=\"a.png\" /> é";
        let tag = enclosing_tag(img, 4).expect("the image");
        assert_eq!((tag.from, tag.to, tag.self_closing, tag.close), (2, 21, true, None));
        let edits = update_tag_edits(&tag, "[alt='the \"best\"' hidden. title]").expect("a single element");
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].insert, "<img src=\"a.png\" alt='the \"best\"' hidden title=\"\" />");
        assert_eq!(update_tag_edits(&tag, "img[data-x=a>b]").map(|edits| edits.len()), Some(1));
        assert_eq!(update_tag_edits(&tag, "img{text}"), None);
        assert_eq!(update_tag_edits(&tag, "  "), None);
        assert!(has_emmet("vue") && has_emmet("scss") && !has_emmet("rust"));
    }
}
