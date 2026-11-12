//! Reformat Code for SQL, in IDEA's default SQL code style: clauses on their own lines, select /
//! set / values items one per line aligned after their keyword, joins indented under FROM, AND / OR
//! of WHERE right-aligned, subqueries aligned after their parenthesis, CREATE TABLE bodies one
//! definition per line. Keyword case and comments are kept; each statement is replaced on its own,
//! so text between statements is left alone.

use super::dialect::is_reserved;
use super::lexer::{TokenKind, len16, lex, slice16, utf16};
use ide_db::DbDriver;

/// A replacement of the UTF-16 range `from..to`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlEdit {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    Token(TokenKind),
    Comment,
}

#[derive(Clone, Debug)]
struct Item {
    kind: ItemKind,
    text: String,
    upper: String,
    from: usize,
    to: usize,
}

impl Item {
    fn is_word(&self) -> bool {
        self.kind == ItemKind::Token(TokenKind::Word)
    }
}

fn is_p(t: Option<&Item>, p: &str) -> bool {
    t.is_some_and(|t| t.kind == ItemKind::Token(TokenKind::Punct) && t.text == p)
}

/// Edits formatting every statement (or those overlapping `range`, a UTF-16 `(from, to)`).
pub fn format_sql(text: &str, driver: DbDriver, range: Option<(usize, usize)>) -> Vec<SqlEdit> {
    let units = utf16(text);
    let lexed = lex(text, driver);
    let mut items: Vec<Item> = lexed
        .tokens
        .into_iter()
        .map(|t| Item { kind: ItemKind::Token(t.kind), text: t.text, upper: t.upper, from: t.from, to: t.to })
        .chain(lexed.comments.iter().map(|c| Item {
            kind: ItemKind::Comment,
            text: slice16(&units, c.from, c.to),
            upper: String::new(),
            from: c.from,
            to: c.to,
        }))
        .collect();
    items.sort_by_key(|it| it.from);

    let mut edits = Vec::new();
    let flush = |start: usize, end: usize, edits: &mut Vec<SqlEdit>| {
        // Comments before a statement stay where they are.
        let mut a = start;
        while a < end && items[a].kind == ItemKind::Comment {
            a += 1;
        }
        if a >= end {
            return;
        }
        let first = &items[a];
        let last = &items[end - 1];
        if range.is_some_and(|(from, to)| last.to < from || first.from > to) {
            return;
        }
        let line = units[..first.from].iter().rposition(|&u| u == u16::from(b'\n')).map_or(0, |p| p + 1);
        let lead = slice16(&units, line, first.from);
        let indent = if lead.chars().all(|c| c == ' ' || c == '\t') { lead } else { String::new() };
        edits.push(SqlEdit { from: first.from, to: last.to, insert: format_statement(&items[a..end], &indent) });
    };
    let mut start = 0;
    let mut depth = 0usize;
    for (i, it) in items.iter().enumerate() {
        if is_p(Some(it), "(") {
            depth += 1;
        } else if is_p(Some(it), ")") {
            depth = depth.saturating_sub(1);
        } else if is_p(Some(it), ";") && depth == 0 {
            flush(start, i + 1, &mut edits);
            start = i + 1;
        }
    }
    flush(start, items.len(), &mut edits);
    edits
}

const NEWLINE_CLAUSES: [&str; 15] = [
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "HAVING",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "WINDOW",
    "RETURNING",
    "VALUES",
    "SET",
    "UNION",
    "INTERSECT",
    "EXCEPT",
];
const JOIN_START: [&str; 8] = ["JOIN", "LEFT", "RIGHT", "FULL", "INNER", "CROSS", "NATURAL", "STRAIGHT_JOIN"];
const JOIN_MODIFIERS: [&str; 7] = ["LEFT", "RIGHT", "FULL", "INNER", "CROSS", "NATURAL", "OUTER"];
/// Keywords written like functions: `cast(x as int)`.
const CALL_KEYWORDS: [&str; 1] = ["CAST"];

/// Where a list item continues: the width of `select `, `from `, `set `, `values `.
fn item_column(clause: &str) -> Option<usize> {
    match clause {
        "SELECT" | "VALUES" => Some(7),
        "FROM" => Some(5),
        "SET" => Some(4),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrameKind {
    Query,
    Create,
    Plain,
}

#[derive(Clone, Debug)]
struct Frame {
    kind: FrameKind,
    /// Column where the block's clauses start.
    indent: usize,
    clause: String,
    /// Tokens written in this frame so far.
    count: usize,
    /// A BETWEEN waits for its AND.
    between: bool,
}

impl Frame {
    fn new(kind: FrameKind, indent: usize) -> Self {
        Self { kind, indent, clause: String::new(), count: 0, between: false }
    }
}

#[derive(Default)]
struct Writer {
    lines: Vec<String>,
    line: String,
}

impl Writer {
    fn newline(&mut self, column: usize) {
        self.lines.push(self.line.trim_end().to_string());
        self.line = " ".repeat(column);
    }

    fn at_line_start(&self) -> bool {
        self.line.trim().is_empty()
    }
}

fn format_statement(items: &[Item], base: &str) -> String {
    let mut w = Writer::default();
    let create = is_create_table(items);
    let mut frames = vec![Frame::new(FrameKind::Query, 0)];
    let mut prev: Option<&Item> = None;
    let mut unary = false;

    for (i, it) in items.iter().enumerate() {
        let fi = frames.len() - 1;
        let word = if it.is_word() { it.upper.as_str() } else { "" };
        let is_query = frames[fi].kind == FrameKind::Query;

        // Line breaks before the token.
        let after_line_comment =
            prev.is_some_and(|p| p.kind == ItemKind::Comment && (p.text.starts_with("--") || p.text.starts_with('#')));
        if after_line_comment {
            let f = &frames[fi];
            let column = match item_column(&f.clause) {
                Some(c) if is_query => f.indent + c,
                _ => f.indent,
            };
            w.newline(column);
        } else if is_p(Some(it), ")") && frames[fi].kind == FrameKind::Create {
            w.newline(frames[fi.saturating_sub(1)].indent);
        } else if is_query && !word.is_empty() && frames[fi].count > 0 {
            let frame = &mut frames[fi];
            let prev_upper = prev.map_or("", |p| p.upper.as_str());
            if NEWLINE_CLAUSES.contains(&word) {
                if !w.at_line_start() {
                    w.newline(frame.indent);
                }
            } else if word == "ON" && frame.clause == "VALUES" {
                w.newline(frame.indent);
            } else if JOIN_START.contains(&word) && !JOIN_MODIFIERS.contains(&prev_upper) && frame.clause == "FROM" {
                w.newline(frame.indent + 9);
            } else if (word == "AND" || word == "OR") && (frame.clause == "WHERE" || frame.clause == "HAVING") {
                if word == "AND" && frame.between {
                    frame.between = false;
                } else {
                    w.newline(frame.indent + if word == "AND" { 2 } else { 3 });
                }
            } else if ["INSERT", "UPDATE", "DELETE"].contains(&word) && is_p(prev, ")") {
                // The statement after `with x as (…)`.
                w.newline(frame.indent);
            }
        }

        // The token itself; a comma after a line comment moves before it (`a, -- why`).
        let moved_comma = after_line_comment && is_p(Some(it), ",");
        if moved_comma
            && let (Some(p), Some(last)) = (prev, w.lines.last_mut())
            && let Some(at) = last.rfind(&p.text)
        {
            *last = format!("{}, {}", last[..at].trim_end(), &last[at..]);
        } else if !moved_comma {
            if !w.at_line_start() && space_before(prev, it, i.checked_sub(2).map(|k| &items[k]), unary) {
                w.line.push(' ');
            }
            w.line.push_str(&it.text);
        }
        unary = it.kind == ItemKind::Token(TokenKind::Punct)
            && (it.text == "-" || it.text == "+")
            && prev.is_none_or(is_operator_like);

        // Bookkeeping and breaks after the token.
        let frame = &mut frames[fi];
        frame.count += 1;
        if is_query && !word.is_empty() {
            if NEWLINE_CLAUSES.contains(&word) {
                frame.clause = word.to_string();
            } else if word == "ON" && frame.clause == "VALUES" {
                frame.clause = "CONFLICT".to_string();
            }
            if word == "BETWEEN" {
                frame.between = true;
            }
        }
        let indent = frame.indent;
        if is_p(Some(it), "(") {
            let next = items.get(i + 1);
            if create && frames.len() == 1 && is_create_body_paren(items, i) {
                // `create table t\n(\n    a int,\n    …\n)`.
                w.line.pop();
                w.line.truncate(w.line.trim_end().len());
                w.newline(indent);
                w.line.push('(');
                frames.push(Frame::new(FrameKind::Create, indent + 4));
                w.newline(indent + 4);
            } else if next.is_some_and(|n| n.is_word() && (n.upper == "SELECT" || n.upper == "WITH")) {
                frames.push(Frame::new(FrameKind::Query, len16(&w.line)));
            } else {
                frames.push(Frame::new(FrameKind::Plain, indent));
            }
        } else if is_p(Some(it), ")") && frames.len() > 1 {
            frames.pop();
        } else if is_p(Some(it), ",") {
            let frame = &frames[fi];
            if w.at_line_start() {
                // Already broken after a line comment.
            } else if frame.kind == FrameKind::Create {
                w.newline(frame.indent);
            } else if let Some(c) = item_column(&frame.clause).filter(|_| is_query) {
                w.newline(frame.indent + c);
            }
        }
        prev = Some(it);
    }
    w.lines.push(w.line.trim_end().to_string());
    w.lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if i == 0 {
                l.trim_start().to_string()
            } else if l.is_empty() {
                String::new()
            } else {
                format!("{base}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_operator_like(t: &Item) -> bool {
    match t.kind {
        ItemKind::Token(TokenKind::Punct) => t.text != ")",
        _ => t.is_word() && is_reserved(&t.text),
    }
}

fn space_before(prev: Option<&Item>, it: &Item, before_prev: Option<&Item>, unary: bool) -> bool {
    let Some(prev) = prev else { return false };
    if unary {
        return false;
    }
    if it.kind == ItemKind::Comment || prev.kind == ItemKind::Comment {
        return true;
    }
    let punct = ItemKind::Token(TokenKind::Punct);
    if it.kind == punct && [",", ";", ")", ".", "::"].contains(&it.text.as_str()) {
        return false;
    }
    if prev.kind == punct && ["(", ".", "::"].contains(&prev.text.as_str()) {
        return false;
    }
    if it.kind == punct && it.text == "(" {
        if prev.is_word() && CALL_KEYWORDS.contains(&prev.upper.as_str()) {
            return false;
        }
        if prev.is_word() && !is_reserved(&prev.text) {
            return is_insert_target(before_prev);
        }
        if prev.kind == ItemKind::Token(TokenKind::Quoted) {
            return is_insert_target(before_prev);
        }
        return true;
    }
    !(it.kind == punct && it.text == "*" && prev.kind == punct && prev.text == ".")
}

/// `into users (`: a column list, written with a space.
fn is_insert_target(before_prev: Option<&Item>) -> bool {
    before_prev.is_some_and(|b| b.is_word() && ["INTO", "TABLE", "EXISTS"].contains(&b.upper.as_str()))
}

fn is_create_table(items: &[Item]) -> bool {
    let words: Vec<&str> = items.iter().filter(|t| t.is_word()).take(3).map(|t| t.upper.as_str()).collect();
    words.first() == Some(&"CREATE") && words.contains(&"TABLE")
}

/// The first top-level `(` of `CREATE TABLE name (`, not `AS (select…)`.
fn is_create_body_paren(items: &[Item], i: usize) -> bool {
    if items[..i].iter().any(|t| is_p(Some(t), "(")) {
        return false;
    }
    i.checked_sub(1)
        .map(|p| &items[p])
        .is_some_and(|p| (p.is_word() || p.kind == ItemKind::Token(TokenKind::Quoted)) && p.upper != "AS")
}

/// `text` with every statement formatted.
pub fn formatted(text: &str, driver: DbDriver) -> String {
    apply_edits(text, &format_sql(text, driver, None))
}

/// `text` with non-overlapping `edits` (in text order) applied.
pub fn apply_edits(text: &str, edits: &[SqlEdit]) -> String {
    let mut units = utf16(text);
    for e in edits.iter().rev() {
        units.splice(e.from..e.to, e.insert.encode_utf16());
    }
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(sql: &str) -> String {
        formatted(sql, DbDriver::Sqlite)
    }

    #[test]
    fn lays_out_a_query_like_idea() {
        assert_eq!(
            fmt(
                "SELECT u.id,u.name , count( * ) AS n FROM users u LEFT JOIN orders o ON o.user_id=u.id WHERE u.id>1 AND o.total BETWEEN 1 AND 5 OR o.status='x' GROUP BY u.id ORDER BY n DESC LIMIT 10;"
            ),
            [
                "SELECT u.id,",
                "       u.name,",
                "       count(*) AS n",
                "FROM users u",
                "         LEFT JOIN orders o ON o.user_id = u.id",
                "WHERE u.id > 1",
                "  AND o.total BETWEEN 1 AND 5",
                "   OR o.status = 'x'",
                "GROUP BY u.id",
                "ORDER BY n DESC",
                "LIMIT 10;",
            ]
            .join("\n")
        );
    }

    #[test]
    fn aligns_subqueries_and_ctes_after_their_parenthesis() {
        assert_eq!(
            fmt(
                "with big as (select user_id, total from orders where total > 10) select * from big where exists (select 1 from users u where u.id = big.user_id);"
            ),
            [
                "with big as (select user_id,",
                "                    total",
                "             from orders",
                "             where total > 10)",
                "select *",
                "from big",
                "where exists (select 1",
                "              from users u",
                "              where u.id = big.user_id);",
            ]
            .join("\n")
        );
    }

    #[test]
    fn formats_dml_and_ddl() {
        assert_eq!(
            fmt("insert into users(email,name) values('a', -1),('b',cast(2 as text));"),
            "insert into users (email, name)\nvalues ('a', -1),\n       ('b', cast(2 as text));"
        );
        assert_eq!(
            fmt("update users set name='x', email = 'y' where id=1;"),
            "update users\nset name = 'x',\n    email = 'y'\nwhere id = 1;"
        );
        assert_eq!(fmt("delete from orders where id = 1;"), "delete\nfrom orders\nwhere id = 1;");
        assert_eq!(
            fmt("create table t (id integer primary key, name varchar(10) not null);"),
            "create table t\n(\n    id integer primary key,\n    name varchar(10) not null\n);"
        );
        assert_eq!(fmt("select 1 union all select 2;"), "select 1\nunion all\nselect 2;");
    }

    #[test]
    fn keeps_comments_text_between_statements_and_the_range() {
        let sql = "-- head\nselect a -- why\n, b from t;\n\n\nselect   1;";
        assert_eq!(fmt(sql), "-- head\nselect a, -- why\n       b\nfrom t;\n\n\nselect 1;");
        let edits = format_sql(sql, DbDriver::Sqlite, Some((sql.len() - 2, sql.len() - 1)));
        assert_eq!(
            edits,
            [SqlEdit { from: sql.rfind("select").expect("a select"), to: sql.len(), insert: "select 1;".into() }]
        );
    }
}
