//! A declaration-level reader for Java and Kotlin sources: classes, methods, fields and their
//! annotations with unevaluated argument expressions. Enough to read Spring Web mappings (no
//! types, no statements); comments, strings and bracket nesting are handled exactly, the rest is
//! skipped. Offsets are UTF-16 code units.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokKind {
    Id,
    Str,
    Num,
    Punct,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tok {
    pub kind: TokKind,
    pub text: String,
    /// String literals: the contents, escapes decoded.
    pub value: Option<String>,
    pub start: usize,
    pub end: usize,
    /// A line break precedes the token.
    pub nl: bool,
}

impl Tok {
    fn is_punct(&self, text: &str) -> bool {
        self.kind == TokKind::Punct && self.text == text
    }

    fn is_id(&self, text: &str) -> bool {
        self.kind == TokKind::Id && self.text == text
    }
}

/// An unevaluated expression: its tokens.
pub type Expr = Vec<Tok>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Annotation {
    /// Simple name (`GetMapping` of `@org.springframework…GetMapping`).
    pub name: String,
    /// Arguments by name; a positional argument is `value`.
    pub args: BTreeMap<String, Expr>,
}

impl Annotation {
    pub fn arg(&self, name: &str) -> Option<&Expr> {
        self.args.get(name)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    /// Type as written, without spaces (`List<String>`); "" when unknown.
    pub ty: String,
    pub annotations: Vec<Annotation>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub is_static: bool,
    pub init: Option<Expr>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub annotations: Vec<Annotation>,
    pub params: Vec<Param>,
    /// Offset of the name.
    pub name_start: usize,
    /// Whole declaration, annotations included.
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassDecl {
    pub name: String,
    pub annotations: Vec<Annotation>,
    pub name_start: usize,
    /// Whole declaration, annotations included.
    pub start: usize,
    pub end: usize,
    /// Kotlin `object` / companion: its properties are static.
    pub is_object: bool,
    pub methods: Vec<Method>,
    pub fields: Vec<Field>,
    /// Java record components / Kotlin primary constructor parameters.
    pub ctor_params: Vec<Param>,
    pub classes: Vec<ClassDecl>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceFile {
    pub package_name: String,
    pub imports: Vec<String>,
    pub classes: Vec<ClassDecl>,
    /// Top-level properties (Kotlin).
    pub fields: Vec<Field>,
}

fn is_id_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_id_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '$'
}

fn find_seq(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let pat: Vec<char> = pat.chars().collect();
    (from..chars.len()).find(|&i| chars[i..].starts_with(&pat))
}

/// The tokens of a source, comments dropped.
pub fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut units = Vec::with_capacity(chars.len() + 1);
    let mut acc = 0;
    for c in &chars {
        units.push(acc);
        acc += c.len_utf16();
    }
    units.push(acc);
    let slice = |from: usize, to: usize| chars[from..to].iter().collect::<String>();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    let mut nl = false;
    while i < n {
        let c = chars[i];
        if c == '\n' {
            nl = true;
            i += 1;
            continue;
        }
        if matches!(c, ' ' | '\t' | '\r' | '\u{c}') {
            i += 1;
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < n && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let end = find_seq(&chars, i + 2, "*/").map_or(n, |close| close + 2);
            if chars[i..end].contains(&'\n') {
                nl = true;
            }
            i = end;
            continue;
        }
        let start = i;
        let tok = |kind, text: String, value: Option<String>, end: usize, nl: bool| Tok {
            kind,
            text,
            value,
            start: units[start],
            end: units[end],
            nl,
        };
        if c == '"' && chars[i..].starts_with(&['"', '"', '"']) {
            let close = find_seq(&chars, i + 3, "\"\"\"");
            let end = close.map_or(n, |close| close + 3);
            let body = slice(i + 3, close.unwrap_or(n));
            // Java text blocks start after a line break; Kotlin raw strings keep it.
            let trimmed = body.trim_start_matches([' ', '\t']);
            let value = trimmed
                .strip_prefix("\r\n")
                .or_else(|| trimmed.strip_prefix('\n'))
                .map_or_else(|| body.clone(), str::to_owned);
            out.push(tok(TokKind::Str, slice(i, end), Some(value), end, nl));
            i = end;
        } else if c == '"' || c == '\'' {
            let mut j = i + 1;
            let mut value = String::new();
            while j < n && chars[j] != c && chars[j] != '\n' {
                if chars[j] == '\\' && j + 1 < n {
                    value.push(match chars[j + 1] {
                        'n' => '\n',
                        't' => '\t',
                        e => e,
                    });
                    j += 2;
                } else {
                    value.push(chars[j]);
                    j += 1;
                }
            }
            let end = n.min(j + 1);
            out.push(tok(TokKind::Str, slice(i, end), Some(value), end, nl));
            i = end;
        } else if c == '`' {
            let close = (i + 1..n).find(|&k| chars[k] == '`');
            let end = match close {
                Some(close) if !chars[i..close].contains(&'\n') => close + 1,
                _ => i + 1,
            };
            let text = if end > i + 1 { slice(i + 1, end - 1) } else { "`".to_owned() };
            out.push(tok(TokKind::Id, text, None, end, nl));
            i = end;
        } else if is_id_start(c) {
            let mut j = i + 1;
            while j < n && is_id_part(chars[j]) {
                j += 1;
            }
            out.push(tok(TokKind::Id, slice(i, j), None, j, nl));
            i = j;
        } else if c.is_ascii_digit() {
            let mut j = i + 1;
            while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '_' || chars[j] == '.') {
                j += 1;
            }
            out.push(tok(TokKind::Num, slice(i, j), None, j, nl));
            i = j;
        } else {
            out.push(tok(TokKind::Punct, c.to_string(), None, i + 1, nl));
            i += 1;
        }
        nl = false;
    }
    out
}

fn closer(open: &str) -> Option<&'static str> {
    match open {
        "(" => Some(")"),
        "[" => Some("]"),
        "{" => Some("}"),
        _ => None,
    }
}

fn is_opener(t: &Tok) -> bool {
    t.kind == TokKind::Punct && closer(&t.text).is_some()
}

/// Index of the matching closing bracket for every opening one (unbalanced: the end).
fn match_brackets(toks: &[Tok]) -> Vec<usize> {
    let mut matched = vec![usize::MAX; toks.len()];
    let mut stack: Vec<usize> = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        if t.kind != TokKind::Punct {
            continue;
        }
        if closer(&t.text).is_some() {
            stack.push(i);
        } else if matches!(t.text.as_str(), ")" | "]" | "}") {
            // Pop to the matching opener (unclosed ones inside end here too); stray closers are ignored.
            if let Some(k) = stack.iter().rposition(|&o| closer(&toks[o].text) == Some(t.text.as_str())) {
                for o in stack.drain(k..) {
                    matched[o] = i;
                }
            }
        }
    }
    for o in stack {
        matched[o] = toks.len();
    }
    matched
}

const CLASS_KW: [&str; 5] = ["class", "interface", "enum", "record", "object"];
const MODIFIERS: [&str; 31] = [
    "public",
    "private",
    "protected",
    "internal",
    "static",
    "final",
    "abstract",
    "open",
    "override",
    "const",
    "data",
    "sealed",
    "inline",
    "suspend",
    "lateinit",
    "default",
    "synchronized",
    "native",
    "transient",
    "volatile",
    "strictfp",
    "operator",
    "infix",
    "external",
    "annotation",
    "inner",
    "value",
    "companion",
    "fun",
    "val",
    "var",
];
const NOT_METHOD: [&str; 10] =
    ["if", "for", "while", "switch", "catch", "synchronized", "return", "new", "throw", "when"];

fn is_modifier(t: &Tok) -> bool {
    t.kind == TokKind::Id && MODIFIERS.contains(&t.text.as_str())
}

fn joined(toks: &[Tok]) -> String {
    toks.iter().map(|t| t.text.as_str()).collect()
}

struct Reader<'a> {
    toks: &'a [Tok],
    matched: Vec<usize>,
}

/// What a class or member body collects while it reads one declaration.
#[derive(Default)]
struct Pending {
    annotations: Vec<Annotation>,
    header: Vec<Tok>,
    decl_start: Option<usize>,
}

impl<'a> Reader<'a> {
    fn new(toks: &'a [Tok]) -> Self {
        Self { toks, matched: match_brackets(toks) }
    }

    fn tok(&self, i: usize) -> Option<&'a Tok> {
        self.toks.get(i)
    }

    fn is(&self, i: usize, text: &str) -> bool {
        self.tok(i).is_some_and(|t| t.text == text && matches!(t.kind, TokKind::Punct | TokKind::Id))
    }

    fn is_kind(&self, i: usize, kind: TokKind) -> bool {
        self.tok(i).is_some_and(|t| t.kind == kind)
    }

    /// Kotlin: a token on a new line that starts a member ends the previous one.
    fn starts_member(&self, i: usize) -> bool {
        let Some(t) = self.tok(i) else { return false };
        if !t.nl {
            return false;
        }
        t.text == "@"
            || t.text == "}"
            || (t.kind == TokKind::Id && (MODIFIERS.contains(&t.text.as_str()) || CLASS_KW.contains(&t.text.as_str())))
    }

    /// `@Name(args)` at `i` (the `@`); the annotation and the next index.
    fn annotation(&self, i: usize) -> Option<(Annotation, usize)> {
        if !self.is(i, "@") || !self.is_kind(i + 1, TokKind::Id) || self.is(i + 1, "interface") {
            return None;
        }
        let mut j = i + 1;
        let mut name = self.toks[j].text.clone();
        // Kotlin use-site targets: `@field:JsonProperty`.
        if self.is(j + 1, ":") && self.is_kind(j + 2, TokKind::Id) {
            j += 2;
            name.clone_from(&self.toks[j].text);
        }
        while self.is(j + 1, ".") && self.is_kind(j + 2, TokKind::Id) {
            j += 2;
            name.clone_from(&self.toks[j].text);
        }
        j += 1;
        let mut args = BTreeMap::new();
        if self.is(j, "(") && !self.toks[j].nl {
            let close = self.matched[j];
            let mut positional: Vec<Expr> = Vec::new();
            for part in self.split(j + 1, close) {
                if part.is_empty() {
                    continue;
                }
                let named = part[0].kind == TokKind::Id
                    && part.get(1).is_some_and(|t| t.is_punct("="))
                    && part.get(2).is_none_or(|t| t.text != "=");
                if named {
                    args.insert(part[0].text.clone(), part[2..].to_vec());
                } else {
                    positional.push(part);
                }
            }
            // Kotlin `vararg value`: `@GetMapping("/a", "/b")` is an array.
            if positional.len() == 1 {
                args.insert("value".to_owned(), positional.remove(0));
            } else if positional.len() > 1 {
                let synthetic = |text: &str, at: &Tok| Tok {
                    kind: TokKind::Punct,
                    text: text.to_owned(),
                    value: None,
                    start: at.start,
                    end: at.start,
                    nl: false,
                };
                let first = positional[0][0].clone();
                let mut value = vec![synthetic("[", &first)];
                for (k, x) in positional.into_iter().enumerate() {
                    if k > 0 {
                        value.push(synthetic(",", &x[0]));
                    }
                    value.extend(x);
                }
                value.push(synthetic("]", &first));
                args.insert("value".to_owned(), value);
            }
            j = close + 1;
        }
        Some((Annotation { name, args }, j))
    }

    /// Splits `[from, to)` at top-level commas (brackets and generics nest).
    fn split(&self, from: usize, to: usize) -> Vec<Expr> {
        let mut parts: Vec<Expr> = Vec::new();
        let mut cur: Expr = Vec::new();
        let mut angle = 0;
        let mut i = from;
        while i < to.min(self.toks.len()) {
            let t = &self.toks[i];
            if is_opener(t) {
                let close = self.matched[i].min(to - 1).min(self.toks.len() - 1);
                cur.extend_from_slice(&self.toks[i..=close]);
                i = close + 1;
                continue;
            }
            if t.is_punct("<") {
                angle += 1;
            }
            if t.is_punct(">") && angle > 0 {
                angle -= 1;
            }
            if t.is_punct(",") && angle == 0 {
                parts.push(std::mem::take(&mut cur));
                i += 1;
                continue;
            }
            cur.push(t.clone());
            i += 1;
        }
        parts.push(cur);
        let last = parts.len() - 1;
        parts.into_iter().enumerate().filter(|(idx, p)| !p.is_empty() || *idx < last).map(|(_, p)| p).collect()
    }

    fn params(&self, open: usize) -> Vec<Param> {
        let close = self.matched[open];
        let mut out = Vec::new();
        for part in self.split(open + 1, close) {
            let mut annotations = Vec::new();
            let mut rest: Vec<Tok> = Vec::new();
            let sub = Reader::new(&part);
            let mut i = 0;
            while i < part.len() {
                if let Some((a, next)) = sub.annotation(i) {
                    annotations.push(a);
                    i = next;
                } else {
                    rest.push(part[i].clone());
                    i += 1;
                }
            }
            if let Some(colon) = rest.iter().position(|t| t.is_punct(":")) {
                // Kotlin `name: Type = default`.
                let head: Vec<&Tok> = rest[..colon]
                    .iter()
                    .filter(|t| t.kind == TokKind::Id && !is_modifier(t) && t.text != "vararg")
                    .collect();
                let eq = rest.iter().enumerate().position(|(k, t)| k > colon && t.is_punct("="));
                if let Some(name) = head.last() {
                    let ty = joined(&rest[colon + 1..eq.unwrap_or(rest.len())]);
                    out.push(Param { name: name.text.clone(), ty, annotations });
                }
            } else {
                let toks: Vec<Tok> = rest.into_iter().filter(|t| !t.is_id("final")).collect();
                if let Some(last) = toks.last().filter(|t| t.kind == TokKind::Id) {
                    out.push(Param { name: last.text.clone(), ty: joined(&toks[..toks.len() - 1]), annotations });
                }
            }
        }
        out
    }

    fn flush_field(header: &[Tok], into: &mut ClassDecl, init: Option<Expr>) {
        let kw = header.iter().position(|t| t.kind == TokKind::Id && (t.text == "val" || t.text == "var"));
        let is_static =
            into.is_object || header.iter().any(|t| t.kind == TokKind::Id && (t.text == "static" || t.text == "const"));
        if let Some(kw) = kw {
            let colon = header.iter().enumerate().position(|(k, t)| k > kw && t.is_punct(":"));
            if let Some(name) = header.get(kw + 1).filter(|t| t.kind == TokKind::Id) {
                into.fields.push(Field {
                    name: name.text.clone(),
                    ty: colon.map_or_else(String::new, |colon| joined(&header[colon + 1..])),
                    is_static,
                    init,
                });
            }
        } else {
            let type_toks: Vec<Tok> = header.iter().filter(|t| !is_modifier(t)).cloned().collect();
            if type_toks.len() >= 2
                && let Some(name) = type_toks.last().filter(|t| t.kind == TokKind::Id)
            {
                into.fields.push(Field {
                    name: name.text.clone(),
                    ty: joined(&type_toks[..type_toks.len() - 1]),
                    is_static,
                    init,
                });
            }
        }
    }

    fn end_of(&self, i: usize) -> usize {
        self.toks.get(i).map_or(0, |t| t.end)
    }

    /// Members of a body `[from, to)`; nested declarations recurse.
    fn body(&self, from: usize, to: usize, into: &mut ClassDecl) {
        let mut cur = Pending::default();
        let mut i = from;
        while i < to {
            let t = &self.toks[i];
            if !cur.header.is_empty() && self.starts_member(i) && !self.is(i, "}") {
                Self::flush_field(&cur.header, into, None);
                cur = Pending::default();
            }
            if let Some((a, next)) = self.annotation(i) {
                cur.decl_start.get_or_insert(t.start);
                cur.annotations.push(a);
                i = next;
                continue;
            }
            if t.is_punct(";") {
                Self::flush_field(&cur.header, into, None);
                cur = Pending::default();
                i += 1;
                continue;
            }
            let decl_start = *cur.decl_start.get_or_insert(t.start);
            let companion = t.is_id("object") && self.is(i + 1, "{");
            if t.kind == TokKind::Id
                && CLASS_KW.contains(&t.text.as_str())
                && (self.is_kind(i + 1, TokKind::Id) || companion)
            {
                i = self.class(i, to, companion, decl_start, std::mem::take(&mut cur.annotations), into);
                cur = Pending::default();
                continue;
            }
            if t.kind == TokKind::Id
                && self.is(i + 1, "(")
                && !NOT_METHOD.contains(&t.text.as_str())
                && !cur.header.iter().any(|h| h.kind == TokKind::Punct && (h.text == "=" || h.text == "."))
            {
                let params = self.params(i + 1);
                let mut j = self.matched[i + 1].saturating_add(1);
                while j < to {
                    if self.is(j, "{") {
                        j = self.matched[j] + 1;
                        break;
                    }
                    if self.is(j, ";") {
                        j += 1;
                        break;
                    }
                    if self.starts_member(j) {
                        break;
                    }
                    if is_opener(&self.toks[j]) {
                        j = self.matched[j];
                    }
                    j += 1;
                }
                let end = match j.min(self.toks.len()).checked_sub(1) {
                    Some(last) => self.end_of(last),
                    None => t.end,
                };
                into.methods.push(Method {
                    name: t.text.clone(),
                    annotations: std::mem::take(&mut cur.annotations),
                    params,
                    name_start: t.start,
                    start: decl_start,
                    end,
                });
                cur = Pending::default();
                i = j;
                continue;
            }
            if t.is_punct("=") {
                let mut init: Expr = Vec::new();
                let mut j = i + 1;
                while j < to {
                    if self.is(j, ";") || self.is(j, "}") {
                        break;
                    }
                    let prev = &self.toks[j - 1];
                    let operand =
                        prev.kind == TokKind::Punct && matches!(prev.text.as_str(), "+" | "." | "(" | "," | "=");
                    if j > i + 1 && self.starts_member(j) && !operand {
                        break;
                    }
                    let tok = &self.toks[j];
                    if j > i + 1 && tok.nl && !operand && tok.text != "+" && tok.text != "." {
                        break;
                    }
                    if is_opener(tok) {
                        let close = self.matched[j].min(to - 1);
                        init.extend_from_slice(&self.toks[j..=close]);
                        j = close;
                    } else {
                        init.push(tok.clone());
                    }
                    j += 1;
                }
                Self::flush_field(&cur.header, into, Some(init));
                cur = Pending::default();
                i = if self.is(j, ";") { j + 1 } else { j };
                continue;
            }
            if is_opener(t) {
                // Initializer blocks, enum constant arguments…
                i = self.matched[i].saturating_add(1);
                if t.text == "{" {
                    cur = Pending::default();
                }
                continue;
            }
            if t.is_punct(",") {
                // Enum constants / `int a, b;`.
                cur = Pending::default();
                i += 1;
                continue;
            }
            cur.header.push(t.clone());
            i += 1;
        }
        if !cur.header.is_empty() {
            Self::flush_field(&cur.header, into, None);
        }
    }

    /// A class declaration whose keyword is at `i`; returns the index after it.
    fn class(
        &self,
        i: usize,
        to: usize,
        companion: bool,
        start: usize,
        annotations: Vec<Annotation>,
        into: &mut ClassDecl,
    ) -> usize {
        let t = &self.toks[i];
        let name_tok = if companion { t } else { &self.toks[i + 1] };
        let mut cls = ClassDecl {
            name: if companion { "Companion".to_owned() } else { name_tok.text.clone() },
            annotations,
            name_start: name_tok.start,
            start,
            end: name_tok.end,
            is_object: t.text == "object",
            ..ClassDecl::default()
        };
        let mut j = if companion { i + 1 } else { i + 2 };
        if self.is(j, "<") {
            let mut depth = 0;
            while j < to {
                if self.is(j, "<") {
                    depth += 1;
                } else if self.is(j, ">") {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                j += 1;
            }
            j += 1;
        }
        // Kotlin `class A @Inject constructor(…)`.
        while j < to && (self.is(j, "@") || self.is_kind(j, TokKind::Id)) && !self.toks[j].nl {
            if let Some((_, next)) = self.annotation(j) {
                j = next;
            } else if is_modifier(&self.toks[j]) || self.is(j, "constructor") {
                j += 1;
            } else {
                break;
            }
        }
        if self.is(j, "(") {
            cls.ctor_params = self.params(j);
            j = self.matched[j] + 1;
        }
        while j < to {
            if self.is(j, "{") || self.is(j, ";") || self.starts_member(j) {
                break;
            }
            if is_opener(&self.toks[j]) {
                j = self.matched[j];
            }
            j += 1;
        }
        if self.is(j, "{") {
            let close = self.matched[j].min(to);
            self.body(j + 1, close, &mut cls);
            cls.end = self.tok(close).or_else(|| self.tok(to - 1)).map_or(cls.end, |t| t.end);
            j = close + 1;
        } else if let Some(prev) = j.checked_sub(1) {
            cls.end = self.end_of(prev);
        }
        into.classes.push(cls);
        j
    }
}

/// The declarations of a Java or Kotlin source.
pub fn parse_source(text: &str) -> SourceFile {
    let toks = tokenize(text);
    let r = Reader::new(&toks);
    let mut package_name = String::new();
    let mut imports = Vec::new();
    let mut i = 0;
    // `package a.b;` / `import a.b.C;` (Kotlin: no semicolons; `import a.b.C as D`).
    loop {
        let Some(t) = toks.get(i).filter(|t| t.is_id("package") || t.is_id("import")) else {
            if r.is(i, "@")
                && toks.get(i + 1).is_some_and(|t| t.text == "file")
                && let Some((_, next)) = r.annotation(i)
            {
                i = next;
                continue;
            }
            break;
        };
        let mut j = i + 1;
        let mut name = String::new();
        while j < toks.len() && !toks[j].nl && !r.is(j, ";") {
            if toks[j].is_id("as") {
                break;
            }
            name.push_str(&toks[j].text);
            j += 1;
        }
        while j < toks.len() && !toks[j].nl && !r.is(j, ";") {
            j += 1;
        }
        if r.is(j, ";") {
            j += 1;
        }
        if t.text == "package" {
            package_name = name;
        } else {
            imports.push(name.strip_prefix("static").map_or_else(|| name.clone(), str::to_owned));
        }
        i = j;
    }
    let mut root = ClassDecl { is_object: true, end: text.encode_utf16().count(), ..ClassDecl::default() };
    r.body(i, toks.len(), &mut root);
    SourceFile { package_name, imports, classes: root.classes, fields: root.fields }
}

/// Every class of the file, nested ones included, outer ones first.
pub fn all_classes(file: &SourceFile) -> Vec<&ClassDecl> {
    fn walk<'a>(classes: &'a [ClassDecl], out: &mut Vec<&'a ClassDecl>) {
        for c in classes {
            out.push(c);
            walk(&c.classes, out);
        }
    }
    let mut out = Vec::new();
    walk(&file.classes, &mut out);
    out
}
