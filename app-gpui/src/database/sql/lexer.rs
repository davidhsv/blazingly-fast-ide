//! SQL tokens for completion and resolution: words, quoted identifiers, string literals, numbers and
//! punctuation, with comments and whitespace dropped. Dialect-aware (MySQL backticks / `#` comments /
//! backslash escapes, PostgreSQL dollar quoting, ClickHouse backslash escapes). Offsets are UTF-16
//! code units, like the editor's; the text is scanned one code unit at a time, so a character
//! outside the Basic Multilingual Plane is never part of a word.

use ide_db::DbDriver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenKind {
    Word,
    Quoted,
    Str,
    Number,
    Param,
    Punct,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    /// Source text (quotes included for `Quoted` / `Str`).
    pub text: String,
    pub from: usize,
    pub to: usize,
    /// Upper-cased `text` of words (the text itself otherwise), for keyword checks.
    pub upper: String,
    /// The unescaped name of a ClickHouse quoted identifier.
    pub value: Option<String>,
}

/// A comment; a caret at `to` is still inside line comments and unterminated block comments.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comment {
    pub from: usize,
    pub to: usize,
    pub open: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lexed {
    pub tokens: Vec<Token>,
    pub comments: Vec<Comment>,
}

/// The UTF-16 code units of `text`.
pub fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// The text of code units `from..to` (clamped to the text).
pub fn slice16(units: &[u16], from: usize, to: usize) -> String {
    let to = to.min(units.len());
    let from = from.min(to);
    String::from_utf16_lossy(&units[from..to])
}

/// Length of `text` in UTF-16 code units.
pub fn len16(text: &str) -> usize {
    text.encode_utf16().count()
}

fn char_of(unit: u16) -> Option<char> {
    char::from_u32(u32::from(unit))
}

/// A letter or `_`: a word starts here.
pub fn is_word_start(unit: u16) -> bool {
    char_of(unit).is_some_and(|c| c == '_' || c.is_alphabetic())
}

/// A letter, digit, `_` or `$`: a word goes on here.
pub fn is_word_part(unit: u16) -> bool {
    char_of(unit).is_some_and(|c| c == '_' || c == '$' || c.is_alphanumeric())
}

fn is_digit(unit: u16) -> bool {
    (u16::from(b'0')..=u16::from(b'9')).contains(&unit)
}

fn is_number_part(unit: u16) -> bool {
    char_of(unit).is_some_and(|c| c.is_ascii_hexdigit() || matches!(c, '.' | '_' | 'x' | 'X'))
}

const TWO_CHAR: [&str; 9] = ["::", "<=", ">=", "<>", "!=", "||", "->", "=>", ":="];

fn unit(c: char) -> u16 {
    c as u16
}

/// Index of the first occurrence of `needle` in `hay` at or after `start`.
fn find_units(hay: &[u16], needle: &[u16], start: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(start.min(hay.len()));
    }
    (start..hay.len().saturating_sub(needle.len() - 1)).find(|&i| hay[i..].starts_with(needle))
}

/// The unescaped body of a ClickHouse quoted identifier (`\x` stands for `x` for quotes and `\`).
fn clickhouse_value(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let body: &[char] = match chars.as_slice() {
        [] => &[],
        [_] => &[],
        [first, inner @ .., last] if first == last => inner,
        [_, rest @ ..] => rest,
    };
    let mut out = String::new();
    let mut i = 0;
    while i < body.len() {
        if body[i] == '\\' && body.get(i + 1).is_some_and(|c| matches!(c, '\\' | '\'' | '"' | '`')) {
            out.push(body[i + 1]);
            i += 2;
        } else {
            out.push(body[i]);
            i += 1;
        }
    }
    out
}

pub fn lex(text: &str, driver: DbDriver) -> Lexed {
    let units = utf16(text);
    let mysql = driver == DbDriver::Mysql;
    let clickhouse = driver == DbDriver::Clickhouse;
    let n = units.len();
    let mut tokens = Vec::new();
    let mut comments = Vec::new();
    let at = |i: usize| units.get(i).copied();
    let push = |tokens: &mut Vec<Token>, kind: TokenKind, from: usize, to: usize| {
        let t = slice16(&units, from, to);
        let upper = if kind == TokenKind::Word { t.to_uppercase() } else { t.clone() };
        let value = (kind == TokenKind::Quoted && clickhouse).then(|| clickhouse_value(&t));
        tokens.push(Token { kind, text: t, from, to, upper, value });
    };
    // End of a quoted run starting at `start` (the closing char doubled = escaped).
    let quoted = |start: usize, close: u16, backslash: bool| -> usize {
        let mut j = start + 1;
        while j < n {
            let c = units[j];
            if backslash && c == unit('\\') {
                j += 2;
            } else if c == close {
                if at(j + 1) == Some(close) {
                    j += 2;
                } else {
                    return j + 1;
                }
            } else {
                j += 1;
            }
        }
        n
    };
    let mut i = 0;
    while i < n {
        let c = units[i];
        let next = at(i + 1);
        if [unit(' '), unit('\t'), unit('\n'), unit('\r')].contains(&c) {
            i += 1;
        } else if (c == unit('-') && next == Some(unit('-'))) || (c == unit('#') && mysql) {
            let to = find_units(&units, &[unit('\n')], i).unwrap_or(n);
            comments.push(Comment { from: i, to, open: true });
            i = to;
        } else if c == unit('/') && next == Some(unit('*')) {
            let end = find_units(&units, &[unit('*'), unit('/')], i + 2);
            let to = end.map_or(n, |e| e + 2);
            comments.push(Comment { from: i, to, open: end.is_none() });
            i = to;
        } else if c == unit('\'') {
            let to = quoted(i, c, mysql || clickhouse);
            push(&mut tokens, TokenKind::Str, i, to);
            i = to;
        } else if c == unit('"') {
            let to = quoted(i, c, clickhouse);
            push(&mut tokens, if mysql { TokenKind::Str } else { TokenKind::Quoted }, i, to);
            i = to;
        } else if c == unit('`') && (mysql || clickhouse) {
            let to = quoted(i, c, clickhouse);
            push(&mut tokens, TokenKind::Quoted, i, to);
            i = to;
        } else if c == unit('$') && driver == DbDriver::Postgres {
            if let Some(tag_end) = dollar_tag(&units, i) {
                let tag = &units[i..tag_end];
                let to = find_units(&units, tag, tag_end).map_or(n, |close| close + tag.len());
                push(&mut tokens, TokenKind::Str, i, to);
                i = to;
            } else {
                let mut j = i + 1;
                while j < n && is_digit(units[j]) {
                    j += 1;
                }
                push(&mut tokens, TokenKind::Param, i, j);
                i = j;
            }
        } else if is_digit(c) || (c == unit('.') && next.is_some_and(is_digit)) {
            let mut j = i + 1;
            while j < n && is_number_part(units[j]) {
                j += 1;
            }
            push(&mut tokens, TokenKind::Number, i, j);
            i = j;
        } else if is_word_start(c) {
            let mut j = i + 1;
            while j < n && is_word_part(units[j]) {
                j += 1;
            }
            push(&mut tokens, TokenKind::Word, i, j);
            i = j;
        } else if [unit('?'), unit(':'), unit('@')].contains(&c)
            && next.is_some_and(is_word_start)
            && !(c == unit(':') && i > 0 && units[i - 1] == unit(':'))
        {
            let mut j = i + 1;
            while j < n && is_word_part(units[j]) {
                j += 1;
            }
            push(&mut tokens, TokenKind::Param, i, j);
            i = j;
        } else {
            let two = slice16(&units, i, i + 2);
            let len = if TWO_CHAR.contains(&two.as_str()) { 2 } else { 1 };
            push(&mut tokens, TokenKind::Punct, i, i + len);
            i += len;
        }
    }
    Lexed { tokens, comments }
}

/// End of a PostgreSQL dollar-quote tag (`$$`, `$tag$`) starting at `i`, within 64 code units.
fn dollar_tag(units: &[u16], i: usize) -> Option<usize> {
    let limit = (i + 64).min(units.len());
    let mut j = i + 1;
    if j < limit && is_word_start(units[j]) {
        j += 1;
        while j < limit && char_of(units[j]).is_some_and(|c| c == '_' || c.is_alphanumeric()) {
            j += 1;
        }
    }
    (j < limit && units[j] == unit('$')).then_some(j + 1)
}

/// What a caret at `pos` lies inside of (not at its edge).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Literal {
    Str,
    Quoted,
    Comment,
}

/// Whether `pos` lies inside a string literal, quoted identifier or comment (not at its edge).
pub fn in_literal(tokens: &[Token], comments: &[Comment], pos: usize) -> Option<Literal> {
    if comments.iter().any(|c| pos > c.from && (pos < c.to || (c.open && pos == c.to))) {
        return Some(Literal::Comment);
    }
    for t in tokens {
        if t.from >= pos {
            break;
        }
        if matches!(t.kind, TokenKind::Str | TokenKind::Quoted) && pos > t.from && (pos < t.to || !closed(t)) {
            return Some(if t.kind == TokenKind::Str { Literal::Str } else { Literal::Quoted });
        }
    }
    None
}

/// [`in_literal`] over a whole lexed text.
pub fn in_lexed_literal(lexed: &Lexed, pos: usize) -> Option<Literal> {
    in_literal(&lexed.tokens, &lexed.comments, pos)
}

/// Whether a quoted token has its closing quote.
fn closed(t: &Token) -> bool {
    let units = utf16(&t.text);
    if units.len() < 2 {
        return false;
    }
    let open = units[0];
    if open == unit('$') {
        let mut j = 1;
        while j < units.len() && char_of(units[j]).is_some_and(|c| c == '_' || c.is_alphanumeric()) {
            j += 1;
        }
        let tag_len = if units.get(j) == Some(&unit('$')) { j + 1 } else { 1 };
        return units.len() >= tag_len * 2 && units.ends_with(&units[..tag_len]);
    }
    units.ends_with(&[open])
}

/// The identifier a word / quoted token names (quotes removed, doubled quotes collapsed).
pub fn ident_name(t: &Token) -> String {
    if t.kind != TokenKind::Quoted {
        return t.text.clone();
    }
    if let Some(value) = &t.value {
        return value.clone();
    }
    let Some(q) = t.text.chars().next() else { return String::new() };
    let rest = &t.text[q.len_utf8()..];
    let body = if t.text.chars().count() > 1 { rest.strip_suffix(q).unwrap_or(rest) } else { rest };
    body.replace(&format!("{q}{q}"), &q.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::params::find_params;
    use super::*;

    #[test]
    fn reads_clickhouse_backticks_and_escaped_quotes_without_exposing_literal_parameters() {
        let sql = r#"select "a\\b\"c", `my column`, 'it\'s ?; text', ?"#;
        let tokens = lex(sql, DbDriver::Clickhouse).tokens;
        assert_eq!(ident_name(&tokens[1]), r#"a\b"c"#);
        let quoted: Vec<String> = tokens.iter().filter(|t| t.kind == TokenKind::Quoted).map(ident_name).collect();
        assert_eq!(quoted, [r#"a\b"c"#, "my column"]);
        let names: Vec<String> = find_params(sql, DbDriver::Clickhouse, 0).into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["?1"]);
    }

    #[test]
    fn skips_comments_and_knows_literals() {
        let l = lex("select 'a;b', \"Col\" -- c\nfrom t /* x */ where a::int = $1", DbDriver::Postgres);
        let texts: Vec<&str> = l.tokens.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["select", "'a;b'", ",", "\"Col\"", "from", "t", "where", "a", "::", "int", "=", "$1"]);
        assert_eq!(in_lexed_literal(&l, 9), Some(Literal::Str));
        assert_eq!(in_lexed_literal(&l, 23), Some(Literal::Comment));
        assert_eq!(in_lexed_literal(&l, 0), None);
    }

    #[test]
    fn handles_dialect_quoting() {
        let kinds = |sql: &str, driver| lex(sql, driver).tokens.iter().map(|t| t.kind).collect::<Vec<_>>();
        assert_eq!(kinds("select `a b` # c\n", DbDriver::Mysql), [TokenKind::Word, TokenKind::Quoted]);
        assert_eq!(kinds("do $x$ begin; end $x$", DbDriver::Postgres), [TokenKind::Word, TokenKind::Str]);
        assert_eq!(in_lexed_literal(&lex("select 'abc", DbDriver::Sqlite), 11), Some(Literal::Str));
    }
}
