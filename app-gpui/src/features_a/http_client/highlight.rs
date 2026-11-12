//! Highlighting of `.http` / `.rest` request files: `###` separators, comments and `# @name`,
//! `@var = value`, request lines, headers, `{{variables}}`, `< file` includes, `> {% %}` handler
//! blocks and JSON bodies.
//!
//! A line-by-line tokenizer: each line is read with a state carried from the previous one, and
//! adjacent tokens of the same style merge into one span.

use crate::everywhere::open::utf16_len;
use regex::Regex;
use std::sync::LazyLock;

/// The request methods a request line starts with.
pub const METHODS: [&str; 9] = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE", "CONNECT"];

/// The style of a highlighted span.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpStyle {
    Heading,
    Comment,
    Meta,
    /// `{{name}}`.
    Variable,
    /// `@name` of `@name = value`.
    VariableDefinition,
    Keyword,
    Link,
    Operator,
    String,
    PropertyName,
    Punctuation,
    Number,
    Atom,
    Brace,
    SquareBracket,
    Separator,
    Bool,
    Null,
}

impl HttpStyle {
    /// The highlight tag name of the style.
    pub fn name(self) -> &'static str {
        match self {
            Self::Heading => "heading",
            Self::Comment => "comment",
            Self::Meta => "meta",
            Self::Variable => "variableName.special",
            Self::VariableDefinition => "variableName.definition",
            Self::Keyword => "keyword",
            Self::Link => "link",
            Self::Operator => "operator",
            Self::String => "string",
            Self::PropertyName => "propertyName",
            Self::Punctuation => "punctuation",
            Self::Number => "number",
            Self::Atom => "atom",
            Self::Brace => "brace",
            Self::SquareBracket => "squareBracket",
            Self::Separator => "separator",
            Self::Bool => "bool",
            Self::Null => "null",
        }
    }
}

/// A highlighted span: UTF-16 offsets into the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HttpToken {
    pub style: HttpStyle,
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Preamble,
    Headers,
    Body,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Start,
    Url,
    HeaderValue,
    Rest,
}

#[derive(Clone, Copy, Debug)]
struct State {
    phase: Phase,
    /// Inside a `> {% … %}` handler script.
    handler: bool,
    /// Whether the body is JSON; `None` until the content type or the body's start tells.
    json: Option<bool>,
    /// Inside a JSON string: the style of that string.
    string: Option<HttpStyle>,
    /// Position within the current line's structure.
    part: Part,
}

impl Default for State {
    fn default() -> Self {
        Self { phase: Phase::Preamble, handler: false, json: None, string: None, part: Part::Start }
    }
}

macro_rules! re {
    ($name:ident, $pattern:expr) => {
        static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($pattern).expect("valid regex"));
    };
}

re!(SEPARATOR, r"^\s*###");
re!(HANDLER_END, r"^.*%\}");
re!(NOT_BRACE, r"^[^}]*");
re!(NAME_META, r"^(#|//)\s*@name(?-u:\b)");
re!(COMMENT, r"^(#|//)");
re!(DEFINITION, r"^@[A-Za-z0-9_.-]+");
re!(METHOD, r"^(GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS|TRACE|CONNECT)");
re!(OTHER_METHOD, r"^[A-Z][A-Z0-9_-]*");
re!(OTHER_METHOD_TARGET, r"^\s+(?:[A-Za-z0-9_+.-]+://|[A-Za-z0-9_.-]+:[0-9]+(?:[/?#]|\s|$)|/|\{\{)");
re!(URL_START, r"^(https?://|/)");
re!(HTTP_VERSION, r"^\s+HTTP/[0-9.]+\s*$");
re!(URL_PART, r"^[^\s{]+");
re!(ASSIGN, r"^\s*=\s*");
re!(NOT_VARIABLE, r"^[^{]+");
re!(CONTINUATION, r"^\s+[?&/]");
re!(CONTENT_TYPE, r"(?i)^Content-Type:\s*([^;\s]+)");
re!(JSON_TYPE, r"(?i)(?:/|\+)json$");
re!(HEADER_NAME, r"^[^:\s]+");
re!(HANDLER_START, r"^\s*>\s*\{%");
re!(HANDLER_LINE_END, r"^.*%\}\s*$");
re!(REDIRECT, r"^\s*(<>|>>|>|<)\s");
re!(JSON_START, r#"^(?:\[|\{\s*(?:"|\}|$)|"|(?:true|false|null|-?[0-9][0-9.eE+-]*)\s*$)"#);
re!(QUOTED, r#"^"(?:[^"\\]|\\.)*""#);
re!(PROPERTY_KEY, r#"^"(?:[^"\\]|\\.)*"\s*:"#);
re!(NUMBER, r"^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?(?-u:\b)");
re!(ATOM, r"^(true|false|null)(?-u:\b)");
re!(PLAIN_TEXT, r#"^[^"{0-9\-tfn]+"#);
re!(BRACE, r"^[{}]");
re!(BRACKET, r"^[\[\]]");
re!(SEPARATOR_CHAR, r"^[,:]");
re!(BOOL, r"^(true|false)(?-u:\b)");
re!(NULL, r"^null(?-u:\b)");
re!(JSON_TEXT, r#"^[^"{}\[\],:0-9\-tfn]+"#);

/// A cursor over one line.
struct Stream<'a> {
    line: &'a str,
    pos: usize,
    start: usize,
}

impl<'a> Stream<'a> {
    fn rest(&self) -> &'a str {
        &self.line[self.pos..]
    }

    fn eol(&self) -> bool {
        self.pos >= self.line.len()
    }

    /// Consumes a match of `re` (anchored at the cursor).
    fn eat_re(&mut self, re: &Regex) -> bool {
        match re.find(self.rest()) {
            Some(m) if m.start() == 0 => {
                self.pos += m.end();
                true
            }
            _ => false,
        }
    }

    fn sees(&self, re: &Regex) -> bool {
        re.find(self.rest()).is_some_and(|m| m.start() == 0)
    }

    fn eat_str(&mut self, s: &str) -> bool {
        let found = self.rest().starts_with(s);
        if found {
            self.pos += s.len();
        }
        found
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn next(&mut self) {
        if let Some(c) = self.peek() {
            self.pos += c.len_utf8();
        }
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        if found {
            self.pos += c.len_utf8();
        }
        found
    }

    fn eat_space(&mut self) -> bool {
        let start = self.pos;
        while let Some(c) = self.peek().filter(|c| c.is_whitespace()) {
            self.pos += c.len_utf8();
        }
        self.pos > start
    }

    fn skip_to_end(&mut self) {
        self.pos = self.line.len();
    }

    /// A request method followed by whitespace, or an upper-case word followed by a request target.
    fn eat_method(&mut self) -> bool {
        let rest = self.rest();
        if let Some(m) = METHOD.find(rest)
            && rest[m.end()..].chars().next().is_some_and(char::is_whitespace)
        {
            self.pos += m.end();
            return true;
        }
        if let Some(m) = OTHER_METHOD.find(rest)
            && OTHER_METHOD_TARGET.is_match(&rest[m.end()..])
        {
            self.pos += m.end();
            return true;
        }
        false
    }

    /// A header name directly followed by `:`.
    fn eat_header_name(&mut self) -> bool {
        let rest = self.rest();
        match HEADER_NAME.find(rest) {
            Some(m) if rest[m.end()..].starts_with(':') => {
                self.pos += m.end();
                true
            }
            _ => false,
        }
    }

    /// The inside of a JSON string up to its closing quote, a `{{variable}}` or the line's end.
    fn eat_string_body(&mut self) {
        let bytes = self.line.as_bytes();
        while let Some(&b) = bytes.get(self.pos) {
            match b {
                b'"' => break,
                b'\\' if self.pos + 1 < bytes.len() => {
                    self.pos += 1;
                    self.next();
                }
                b'\\' => break,
                b'{' if bytes.get(self.pos + 1) == Some(&b'{') => break,
                _ => self.next(),
            }
        }
    }
}

fn variable(stream: &mut Stream) -> Option<HttpStyle> {
    if stream.eat_str("{{") {
        stream.eat_re(&NOT_BRACE);
        stream.eat_str("}}");
        return Some(HttpStyle::Variable);
    }
    None
}

fn blank_line(state: &mut State) {
    if state.phase == Phase::Headers || (state.phase == Phase::Preamble && state.part == Part::Url) {
        state.phase = Phase::Body;
    }
}

fn token(stream: &mut Stream, state: &mut State) -> Option<HttpStyle> {
    if stream.pos == 0 {
        // The line after the request line starts the headers.
        if state.phase == Phase::Preamble && state.part == Part::Url {
            state.phase = Phase::Headers;
        }
        state.part = Part::Start;
        state.string = None;
        if stream.eat_re(&SEPARATOR) {
            stream.skip_to_end();
            state.phase = Phase::Preamble;
            state.handler = false;
            state.json = None;
            return Some(HttpStyle::Heading);
        }
    }
    if state.handler {
        if stream.eat_re(&HANDLER_END) {
            state.handler = false;
            return Some(HttpStyle::Meta);
        }
        stream.skip_to_end();
        return Some(HttpStyle::Comment);
    }
    if let Some(style) = variable(stream) {
        if state.phase == Phase::Preamble && state.part == Part::Start {
            state.part = Part::Url;
        }
        return Some(style);
    }
    match state.phase {
        Phase::Preamble => preamble(stream, state),
        Phase::Headers => headers(stream, state),
        Phase::Body => body(stream, state),
    }
}

fn preamble(stream: &mut Stream, state: &mut State) -> Option<HttpStyle> {
    if state.part == Part::Start {
        if stream.eat_space() {
            return None;
        }
        if stream.eat_re(&NAME_META) {
            state.part = Part::Rest;
            return Some(HttpStyle::Meta);
        }
        if stream.eat_re(&COMMENT) {
            stream.skip_to_end();
            return Some(HttpStyle::Comment);
        }
        if stream.eat_re(&DEFINITION) {
            state.part = Part::Rest;
            return Some(HttpStyle::VariableDefinition);
        }
        if stream.eat_method() {
            state.part = Part::Url;
            return Some(HttpStyle::Keyword);
        }
        if stream.sees(&URL_START) {
            state.part = Part::Url;
        } else {
            stream.skip_to_end();
            return None;
        }
    }
    if state.part == Part::Url {
        if stream.eat_re(&HTTP_VERSION) {
            return Some(HttpStyle::Meta);
        }
        if stream.eat_space() {
            return None;
        }
        if !stream.eat_re(&URL_PART) {
            stream.next();
        }
        return Some(HttpStyle::Link);
    }
    // The rest of `# @name …` / `@var = …`.
    if stream.eat_re(&ASSIGN) {
        return Some(HttpStyle::Operator);
    }
    if !stream.eat_re(&NOT_VARIABLE) {
        stream.next();
    }
    Some(HttpStyle::String)
}

fn headers(stream: &mut Stream, state: &mut State) -> Option<HttpStyle> {
    if state.part == Part::Start {
        if stream.eat_re(&CONTINUATION) {
            state.part = Part::Url;
            return Some(HttpStyle::Link);
        }
        if stream.eat_re(&COMMENT) {
            stream.skip_to_end();
            return Some(HttpStyle::Comment);
        }
        if let Some(media) = CONTENT_TYPE.captures(stream.rest()).and_then(|c| c.get(1)) {
            state.json = Some(JSON_TYPE.is_match(media.as_str()));
        }
        if stream.eat_header_name() {
            state.part = Part::HeaderValue;
            return Some(HttpStyle::PropertyName);
        }
        stream.skip_to_end();
        return None;
    }
    if state.part == Part::Url {
        if !stream.eat_re(&URL_PART) {
            stream.next();
        }
        return Some(HttpStyle::Link);
    }
    if stream.eat(':') {
        return Some(HttpStyle::Punctuation);
    }
    if !stream.eat_re(&NOT_VARIABLE) {
        stream.next();
    }
    Some(HttpStyle::String)
}

fn body(stream: &mut Stream, state: &mut State) -> Option<HttpStyle> {
    if state.part == Part::Start {
        state.part = Part::Rest;
        if stream.eat_re(&HANDLER_START) {
            state.handler = !stream.eat_re(&HANDLER_LINE_END);
            stream.skip_to_end();
            return Some(HttpStyle::Meta);
        }
        if stream.eat_re(&REDIRECT) {
            stream.skip_to_end();
            return Some(HttpStyle::Meta);
        }
    }
    if state.json.is_none() {
        if stream.eat_space() {
            return None;
        }
        state.json = Some(JSON_START.is_match(stream.rest()));
    }
    if state.json == Some(false) {
        if stream.eat_re(&QUOTED) {
            return Some(HttpStyle::String);
        }
        if stream.eat_re(&NUMBER) {
            return Some(HttpStyle::Number);
        }
        if stream.eat_re(&ATOM) {
            return Some(HttpStyle::Atom);
        }
        if !stream.eat_re(&PLAIN_TEXT) {
            stream.next();
        }
        return None;
    }
    if state.string.is_none() && stream.peek() == Some('"') {
        state.string = Some(if stream.sees(&PROPERTY_KEY) { HttpStyle::PropertyName } else { HttpStyle::String });
        stream.next();
    }
    if let Some(style) = state.string {
        stream.eat_string_body();
        if stream.eat('"') {
            state.string = None;
        }
        if stream.pos == stream.start {
            stream.next();
        }
        return Some(style);
    }
    let style = [
        (&BRACE, HttpStyle::Brace),
        (&BRACKET, HttpStyle::SquareBracket),
        (&SEPARATOR_CHAR, HttpStyle::Separator),
        (&NUMBER, HttpStyle::Number),
        (&BOOL, HttpStyle::Bool),
        (&NULL, HttpStyle::Null),
    ]
    .into_iter()
    .find_map(|(re, style)| stream.eat_re(re).then_some(style));
    if style.is_none() && !stream.eat_re(&JSON_TEXT) {
        stream.next();
    }
    style
}

/// Reads one token; a tokenizer that does not advance gets a few more tries, then the character
/// is skipped unstyled.
fn read_token(stream: &mut Stream, state: &mut State) -> Option<HttpStyle> {
    for _ in 0..10 {
        let style = token(stream, state);
        if stream.pos > stream.start {
            return style;
        }
    }
    stream.next();
    None
}

/// The highlighted spans of an `.http` document, in order. A `\r` before a line break belongs
/// to no span.
pub fn highlight_http(doc: &str) -> Vec<HttpToken> {
    let mut state = State::default();
    let mut out: Vec<HttpToken> = Vec::new();
    let mut line_start = 0;
    for raw in doc.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.is_empty() {
            blank_line(&mut state);
        } else {
            let mut stream = Stream { line, pos: 0, start: 0 };
            while !stream.eol() {
                stream.start = stream.pos;
                let Some(style) = read_token(&mut stream, &mut state) else { continue };
                let from = line_start + utf16_len(&line[..stream.start]);
                let to = line_start + utf16_len(&line[..stream.pos]);
                match out.last_mut() {
                    Some(last) if last.style == style && last.to == from => last.to = to,
                    _ => out.push(HttpToken { style, from, to }),
                }
            }
        }
        line_start += utf16_len(raw) + 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(text: &str) -> Vec<(&'static str, String)> {
        let units: Vec<u16> = text.encode_utf16().collect();
        highlight_http(text)
            .into_iter()
            .filter(|t| t.from < t.to)
            .map(|t| (t.style.name(), String::from_utf16_lossy(&units[t.from..t.to])))
            .collect()
    }

    fn has(t: &[(&'static str, String)], name: &str, text: &str) -> bool {
        t.iter().any(|(n, s)| *n == name && s == text)
    }

    fn json_punctuation(t: &[(&'static str, String)]) -> Vec<(&'static str, String)> {
        t.iter().filter(|(n, _)| ["brace", "squareBracket", "separator"].contains(n)).cloned().collect()
    }

    #[test]
    fn highlights_request_lines_and_their_json_bodies() {
        for (method, url) in [
            ("GRPC", "localhost:9003/example.Greeter/SayHello"),
            ("WEBSOCKET", "ws://localhost:8080/events"),
            ("PROPFIND", "https://example.test/collection"),
            ("M-SEARCH", "http://example.test/devices"),
        ] {
            let t = tokens(&format!(
                "### Request with body\n{method} {url}\nContent-Type: application/json\n\n\
                 {{\"name\": \"World\", \"count\": 12, \"enabled\": true}}\n"
            ));
            assert!(has(&t, "keyword", method), "{method}");
            assert!(has(&t, "link", url), "{url}");
            assert!(has(&t, "propertyName", "Content-Type"));
            assert!(has(&t, "propertyName", "\"name\""));
            assert!(has(&t, "string", "\"World\""));
            assert!(has(&t, "number", "12"));
            assert!(has(&t, "bool", "true"));
        }
    }

    #[test]
    fn recognizes_a_bare_request_target_before_entering_the_body() {
        for url in ["https://example.test/items", "/items", "{{endpoint}}", "{{host}}/items"] {
            let t = tokens(&format!("{url}\n\n{{\"count\": 12, \"enabled\": false}}\n"));
            let expected = if url.starts_with("{{") { "variableName" } else { "link" };
            assert!(t[0].0.contains(expected), "{url}: {:?}", t[0]);
            assert!(has(&t, "propertyName", "\"count\""));
            assert!(has(&t, "number", "12"));
            assert!(has(&t, "bool", "false"));
        }
    }

    #[test]
    fn resets_request_state_across_protocol_changes_and_keeps_variable_declarations() {
        let t = tokens(
            "@host = localhost\n### RPC\nGRPC {{host}}:9003/example.Greeter/SayHello\n\n{\"count\": 1}\n### HTTP\n\
             POST https://example.test/messages\n\n{\"count\": 2}\n### Socket\nWEBSOCKET ws://{{host}}/events\n\n\
             {\"count\": 3}",
        );
        let texts = |name: &str| t.iter().filter(|(n, _)| *n == name).map(|(_, s)| s.as_str()).collect::<Vec<_>>();
        assert_eq!(texts("keyword"), ["GRPC", "POST", "WEBSOCKET"]);
        assert_eq!(texts("number"), ["1", "2", "3"]);
        assert!(t.iter().any(|(n, s)| n.contains("variableName") && s == "@host"));
    }

    #[test]
    fn distinguishes_json_keys_values_arrays_and_primitive_literals() {
        let t = tokens(
            "POST https://example.test/items\n\n{\"name\": \"World\", \"fraction\": -1.25e+3, \"enabled\": true, \
             \"empty\": null, \"items\": [1, false, {\"nested\": \"yes\"}]}",
        );
        assert!(has(&t, "propertyName", "\"name\""));
        assert!(has(&t, "string", "\"World\""));
        assert!(has(&t, "propertyName", "\"nested\""));
        assert!(has(&t, "number", "-1.25e+3"));
        assert!(has(&t, "bool", "true"));
        assert!(has(&t, "bool", "false"));
        assert!(has(&t, "null", "null"));
        assert!(t.iter().any(|(n, s)| *n == "brace" && s.contains('{')));
        assert!(has(&t, "squareBracket", "["));
        assert!(t.iter().any(|(n, s)| *n == "separator" && s.contains(':')));
    }

    #[test]
    fn ignores_non_request_preamble_lines() {
        for preamble in ["TODO something", "HOST: x", "WORD", "TODO", "TODO ", "TODO something\nHOST: x"] {
            let t = tokens(&format!(
                "{preamble}\nPOST https://example.test/items HTTP/1.1\nContent-Type: application/json\n\n\
                 {{\"ok\": true}}"
            ));
            assert!(has(&t, "keyword", "POST"), "{preamble}");
            assert!(!has(&t, "keyword", "TODO"), "{preamble}");
            assert!(has(&t, "meta", " HTTP/1.1"), "{preamble}");
            assert!(has(&t, "propertyName", "Content-Type"), "{preamble}");
            assert!(has(&t, "propertyName", "\"ok\""), "{preamble}");
        }
    }

    #[test]
    fn keeps_comments_continued_queries_and_separator_resets() {
        let t = tokens(
            "TODO something\n### Request\n# comment\n// comment\n{{host}}/items HTTP/1.1\n  ?q={{query}}\n  &limit=12\n\
             # header comment\nAccept: application/json\n\n{}\n### Next\nGET /next\n\n[]",
        );
        assert!(has(&t, "meta", " HTTP/1.1"));
        assert!(has(&t, "link", "/items"));
        assert!(t.iter().any(|(n, s)| *n == "link" && s.contains("&limit=12")));
        assert!(has(&t, "comment", "# header comment"));
        assert!(has(&t, "propertyName", "Accept"));
        assert!(has(&t, "keyword", "GET"));
        assert!(has(&t, "link", "/next"));
    }

    #[test]
    fn highlights_variables_inside_json_strings_and_unquoted_values() {
        let t = tokens("POST /items\n\n{\"name\": \"Hello {{name}}!\", \"count\": {{count}}, \"escaped\": \"a\\\"b\"}");
        for name in ["name", "count"] {
            assert!(has(&t, "variableName.special", &format!("{{{{{name}}}}}")), "{name}");
        }
        assert!(has(&t, "string", "!\""));
        assert!(has(&t, "propertyName", "\"escaped\""));
    }

    #[test]
    fn keeps_advancing_while_a_json_string_ends_with_an_unfinished_escape() {
        let t = tokens("POST /items\n\n{\"name\": \"unfinished\\");
        assert!(has(&t, "propertyName", "\"name\""));
    }

    #[test]
    fn keeps_non_json_bodies_out_of_json_punctuation_highlighting() {
        for (kind, body) in [
            ("application/x-www-form-urlencoded", "name=World&items=[one,two]&enabled=true"),
            ("application/xml", "<item name=\"World\">text: [one,two]</item>"),
            ("text/plain", "text: [one,two]"),
            ("application/graphql", "{\n  user(id: 12) { name }\n}"),
            ("", "query { user(id: 12) { name } }"),
        ] {
            let headers = if kind.is_empty() { String::new() } else { format!("Content-Type: {kind}\n") };
            let t = tokens(&format!("POST /items\n{headers}\n{body}"));
            assert!(json_punctuation(&t).is_empty(), "{kind}");
        }
    }

    #[test]
    fn preserves_standard_request_targets() {
        for request in ["GET relative/path", "OPTIONS *", "CONNECT example.test:443"] {
            assert!(tokens(&format!("{request}\n\n{{}}")).iter().any(|(n, _)| *n == "keyword"), "{request}");
        }
    }

    #[test]
    fn recognizes_json_content_type_and_resets_it_at_separators() {
        for kind in ["application/json", "application/problem+json; charset=utf-8"] {
            let t = tokens(&format!(
                "POST /one\ncontent-type: {kind}\n\ntrue\n### Two\nPOST /two\nContent-Type: text/plain\n\n\
                 text: [one,two]"
            ));
            assert!(has(&t, "bool", "true"), "{kind}");
            assert!(json_punctuation(&t).is_empty(), "{kind}");
        }
    }

    #[test]
    fn keeps_input_files_and_response_scripts_out_of_json_highlighting() {
        let t = tokens(
            "POST /items\n\n< ./payload.json\n> {%\n\
             client.test(\"status\", function () { client.assert(true); });\n%}\n\
             ### Next\nPOST /next\n\n{\"ok\": true}",
        );
        assert!(has(&t, "meta", "< ./payload.json"));
        assert!(has(&t, "meta", "> {%"));
        assert!(has(&t, "comment", "client.test(\"status\", function () { client.assert(true); });"));
        assert!(has(&t, "meta", "%}"));
        assert!(has(&t, "propertyName", "\"ok\""));
    }

    #[test]
    fn tags_separators_methods_urls_headers_variables_and_comments() {
        let t = tokens("### Users\n# @name list\nGET https://{{host}}/users\nAccept: json\n\n{\"a\": 1}\n");
        let contains = |name: &str, text: &str| t.iter().any(|(n, s)| n.contains(name) && s == text);
        assert!(contains("heading", "### Users"));
        assert!(contains("keyword", "GET"));
        assert!(contains("link", "https://"));
        assert!(contains("variableName", "{{host}}"));
        assert!(contains("propertyName", "Accept"));
        assert!(contains("string", " json"));
        assert!(contains("propertyName", "\"a\""));
        assert!(contains("number", "1"));
        assert!(contains("meta", "# @name"));
    }
}
