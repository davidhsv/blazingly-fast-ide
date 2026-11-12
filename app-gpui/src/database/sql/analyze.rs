//! Caret analysis for SQL completion: the statement and query block around the caret, the tables it
//! can see (FROM / JOIN items with aliases, CTEs, derived tables, enclosing queries for correlated
//! subqueries) and what is expected at the caret (a statement, a table, a column expression, a join
//! condition…). Purely lexical: works on half-typed statements a parser would reject.

use super::dialect::{is_clause_word, is_reserved};
use super::lexer::{Comment, Token, TokenKind, ident_name, in_literal, is_word_part, lex, slice16, utf16};
use ide_db::DbDriver;
use std::collections::HashMap;

/// A UTF-16 range of the text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub from: usize,
    pub to: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefKind {
    Table,
    Cte,
    Derived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableRef {
    /// Schema as written (`None`: unqualified).
    pub schema: Option<String>,
    /// Table / CTE name; a derived table's alias.
    pub name: String,
    pub alias: Option<String>,
    /// Offsets of the alias token.
    pub alias_at: Option<Span>,
    pub kind: RefKind,
    /// Columns known from the query text (CTEs, derived tables); `None` for catalog tables.
    pub columns: Option<Vec<String>>,
    /// Offsets of the (last part of the) name.
    pub from: usize,
    pub to: usize,
    /// 0: the caret's query block; 1+: enclosing blocks (correlated references).
    pub depth: usize,
    /// Introduced by JOIN (so `ON` may follow).
    pub via_join: bool,
}

impl TableRef {
    /// How the query names it: the alias, else the name.
    pub fn ref_name(&self) -> &str {
        self.alias.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Clause {
    Select,
    From,
    Where,
    Group,
    Order,
    Having,
    Limit,
    Set,
    Values,
    On,
    Returning,
    Other,
}

/// What is expected at the caret. A table reference is an index into [`Analysis::refs`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlContext {
    None,
    Statement,
    Table {
        join: bool,
    },
    /// Right after a FROM / JOIN item (alias, JOIN, WHERE…).
    AfterTable {
        reference: usize,
    },
    Column {
        clause: Clause,
    },
    JoinCondition {
        reference: usize,
    },
    InsertColumns {
        target: usize,
    },
    AfterInsertColumns,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefix {
    pub from: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Statement {
    pub from: usize,
    pub to: usize,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Analysis {
    pub context: SqlContext,
    /// Identifier chain before the caret word: `s.t.na|` → `["s", "t"]`.
    pub qualifier: Option<Vec<String>>,
    /// The word being typed (start offset and text up to the caret).
    pub prefix: Prefix,
    /// Tables visible at the caret, innermost block first.
    pub refs: Vec<TableRef>,
    pub ctes: Vec<TableRef>,
    pub statement: Statement,
    /// First word of the statement, upper-cased.
    pub verb: String,
    /// Where `FROM t` goes for a SELECT that has no FROM yet (`None`: it has one).
    pub from_insert: Option<usize>,
    /// Keyword case the statement is written in.
    pub lower_keywords: bool,
    /// The token before the caret word (and its qualifier), upper-cased: `(`, `UNION`…
    pub previous: Option<String>,
}

impl Analysis {
    /// The table reference a context names.
    pub fn context_ref(&self) -> Option<&TableRef> {
        match self.context {
            SqlContext::AfterTable { reference }
            | SqlContext::JoinCondition { reference }
            | SqlContext::InsertColumns { target: reference } => self.refs.get(reference),
            _ => None,
        }
    }
}

const TABLE_INTRO: [&str; 8] = ["FROM", "JOIN", "UPDATE", "INTO", "TABLE", "TRUNCATE", "DESCRIBE", "STRAIGHT_JOIN"];
const SET_OPS: [&str; 3] = ["UNION", "INTERSECT", "EXCEPT"];
const QUERY_START: [&str; 4] = ["SELECT", "WITH", "VALUES", "TABLE"];

fn clause_of(upper: &str) -> Option<Clause> {
    Some(match upper {
        "SELECT" => Clause::Select,
        "FROM" | "JOIN" | "STRAIGHT_JOIN" => Clause::From,
        "WHERE" => Clause::Where,
        "GROUP" => Clause::Group,
        "ORDER" => Clause::Order,
        "HAVING" => Clause::Having,
        "LIMIT" | "OFFSET" => Clause::Limit,
        "SET" => Clause::Set,
        "VALUES" => Clause::Values,
        "ON" | "USING" => Clause::On,
        "RETURNING" => Clause::Returning,
        "WINDOW" => Clause::Other,
        _ => return None,
    })
}

/// The token at a signed index (`None` before the start or past the end).
fn tok(st: &[Token], i: isize) -> Option<&Token> {
    usize::try_from(i).ok().and_then(|i| st.get(i))
}

pub(crate) fn is_ident(t: Option<&Token>) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Quoted || (t.kind == TokenKind::Word && !is_clause_word(&t.upper)))
}

pub(crate) fn is_p(t: Option<&Token>, p: &str) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Punct && t.text == p)
}

pub(crate) fn is_w(t: Option<&Token>, words: &[&str]) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Word && words.contains(&t.upper.as_str()))
}

fn is_name(t: Option<&Token>) -> bool {
    t.is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::Quoted))
}

fn is_set_op(t: &Token) -> bool {
    t.kind == TokenKind::Word && SET_OPS.contains(&t.upper.as_str())
}

/// Bracket structure of a token list.
pub(crate) struct Parens {
    /// `(` index → matching `)` index (or the token count when unclosed).
    close: HashMap<usize, usize>,
    /// Token index → index of the innermost enclosing `(` (−1 at top level).
    parent: Vec<isize>,
}

impl Parens {
    pub(crate) fn new(t: &[Token]) -> Self {
        let mut stack: Vec<usize> = Vec::new();
        let mut close = HashMap::new();
        let top = |stack: &[usize]| stack.last().map_or(-1, |&p| p as isize);
        let parent = t
            .iter()
            .enumerate()
            .map(|(i, tok)| {
                let p = top(&stack);
                if is_p(Some(tok), "(") {
                    stack.push(i);
                } else if is_p(Some(tok), ")")
                    && let Some(open) = stack.pop()
                {
                    close.insert(open, i);
                    return top(&stack);
                }
                p
            })
            .collect();
        for open in stack {
            close.insert(open, t.len());
        }
        Self { close, parent }
    }

    fn close_of(&self, open: usize) -> usize {
        self.close.get(&open).copied().unwrap_or(self.parent.len())
    }

    fn parent_of(&self, i: usize) -> isize {
        self.parent.get(i).copied().unwrap_or(-1)
    }

    /// Indices in `a..b` directly inside group `g` (not nested deeper).
    fn level(&self, g: isize, a: usize, b: usize) -> Vec<usize> {
        (a..b.min(self.parent.len())).filter(|&i| self.parent[i] == g).collect()
    }

    /// The `(` closed by token `i`.
    fn opened_by(&self, i: usize) -> Option<usize> {
        self.close.iter().find(|&(_, &c)| c == i).map(|(&open, _)| open)
    }
}

/// Start of group `g`'s contents (the token after its `(`, or 0 at top level).
fn inside(g: isize) -> usize {
    usize::try_from(g + 1).unwrap_or(0)
}

pub fn analyze(text: &str, pos: usize, driver: DbDriver) -> Analysis {
    let lexed = lex(text, driver);
    analyze_tokens(text, pos, driver, &lexed.tokens, &lexed.comments)
}

/// [`analyze`] over tokens already lexed from `text` (possibly those of one statement only).
pub fn analyze_tokens(text: &str, pos: usize, driver: DbDriver, all: &[Token], comments: &[Comment]) -> Analysis {
    let units = utf16(text);
    let pos = pos.min(units.len());
    let mut word_from = pos;
    while word_from > pos.saturating_sub(256) && is_word_part(units[word_from - 1]) {
        word_from -= 1;
    }
    let prefix = Prefix { from: word_from, text: slice16(&units, word_from, pos) };

    // The statement: between the `;` before the caret word and the next one.
    let mut a = 0;
    let mut b = all.len();
    for (i, t) in all.iter().enumerate() {
        if !is_p(Some(t), ";") {
            continue;
        }
        if t.to <= prefix.from {
            a = i + 1;
        } else {
            b = i;
            break;
        }
    }
    let st = &all[a..b];
    let s_from = if a > 0 { all[a - 1].to } else { 0 };
    let s_to = if b < all.len() { all[b].from } else { units.len() };
    let statement = Statement { from: s_from, to: s_to, text: slice16(&units, s_from, s_to) };
    let verb = st.first().filter(|t| t.kind == TokenKind::Word).map(|t| t.upper.clone()).unwrap_or_default();
    let lower_keywords = st
        .iter()
        .find(|t| t.kind == TokenKind::Word && is_clause_word(&t.upper))
        .is_some_and(|k| k.text == k.text.to_lowercase());

    let parens = Parens::new(st);
    // Tokens before the caret word (a word the caret is inside of is the prefix).
    let nb = st.iter().take_while(|t| t.to <= prefix.from).count();
    let before = &st[..nb];

    // Open parens around the caret, innermost last.
    let mut open: Vec<usize> = Vec::new();
    for (i, t) in before.iter().enumerate() {
        if is_p(Some(t), "(") {
            open.push(i);
        } else if is_p(Some(t), ")") {
            open.pop();
        }
    }
    let is_query =
        |p: usize| st.get(p + 1).is_some_and(|t| t.kind == TokenKind::Word && QUERY_START.contains(&t.upper.as_str()));
    let mut blocks: Vec<isize> = open.iter().rev().filter(|&&p| is_query(p)).map(|&p| p as isize).collect();
    blocks.push(-1);

    let ctes = parse_ctes(st, &parens);
    let mut refs: Vec<TableRef> = Vec::new();
    let mut ref_ends: HashMap<usize, usize> = HashMap::new();
    for (depth, &g) in blocks.iter().enumerate() {
        // Within an enclosing block, the "caret" is the paren of the block nested in it.
        let caret = if depth == 0 { nb } else { usize::try_from(blocks[depth - 1]).unwrap_or(0) };
        let (ra, rb) = block_range(st, &parens, g, caret);
        for (r, end) in parse_refs(st, &parens, g, ra, rb, &ctes, depth, driver) {
            refs.push(r);
            if depth == 0 {
                ref_ends.insert(end, refs.len() - 1);
            }
        }
    }

    let literal = in_literal(all, comments, pos);
    let mut qualifier: Vec<String> = Vec::new();
    let mut k = nb as isize - 1;
    while is_p(tok(before, k), ".")
        && is_name(tok(before, k - 1))
        && tok(before, k).map(|t| t.to) == Some(tok(before, k + 1).map_or(prefix.from, |t| t.from))
    {
        if let Some(t) = tok(before, k - 1) {
            qualifier.insert(0, ident_name(t));
        }
        k -= 2;
    }
    let p = tok(before, k);
    let pi = k;
    let block = blocks[0];
    let clause = clause_at(st, &parens, block, open.last().map_or(-1, |&o| o as isize), nb);
    let prev_word = tok(before, pi - 1);
    let p_at = usize::try_from(pi).ok().and_then(|i| before.get(i).map(|t| (i, t)));

    let context = if literal.is_some() {
        SqlContext::None
    } else if let Some((pi, p)) = p_at {
        if p.kind == TokenKind::Word
            && TABLE_INTRO.contains(&p.upper.as_str())
            && !(p.upper == "TABLE" && verb != "TABLE" && !is_w(prev_word, &["ALTER", "DROP", "TRUNCATE", "LOCK"]))
        {
            SqlContext::Table { join: p.upper == "JOIN" || p.upper == "STRAIGHT_JOIN" }
        } else if is_w(Some(p), &["LATERAL", "ONLY"]) && is_w(prev_word, &["JOIN", "FROM"]) {
            SqlContext::Table { join: is_w(prev_word, &["JOIN"]) }
        } else if is_p(Some(p), ",") && clause == Clause::From && parens.parent_of(pi) == block {
            SqlContext::Table { join: false }
        } else if is_w(Some(p), &["AS"]) || is_p(Some(p), "::") {
            SqlContext::None
        } else if let Some(&reference) = ref_ends.get(&pi)
            && qualifier.is_empty()
            && matches!(clause, Clause::From | Clause::Other)
        {
            SqlContext::AfterTable { reference }
        } else if is_w(Some(p), &["ON"]) || (clause == Clause::On && is_w(Some(p), &["AND", "OR"])) {
            match refs.iter().rposition(|r| r.depth == 0 && r.from < p.from) {
                Some(reference) => SqlContext::JoinCondition { reference },
                None => SqlContext::Column { clause: Clause::On },
            }
        } else if let Some(target) = insert_target(st, &parens, open.last().copied(), &ref_ends)
            && (is_p(Some(p), "(") || is_p(Some(p), ","))
        {
            SqlContext::InsertColumns { target }
        } else if is_p(Some(p), ")") && insert_target(st, &parens, parens.opened_by(pi), &ref_ends).is_some() {
            SqlContext::AfterInsertColumns
        } else {
            SqlContext::Column { clause }
        }
    } else if qualifier.is_empty() {
        SqlContext::Statement
    } else {
        SqlContext::Column { clause: Clause::Other }
    };

    Analysis {
        context,
        qualifier: (!qualifier.is_empty()).then_some(qualifier),
        prefix,
        refs,
        ctes,
        statement,
        verb,
        from_insert: from_insert_pos(st, &parens, block, pos),
        lower_keywords,
        previous: p.map(|p| p.upper.clone()),
    }
}

/// Token range of block `g`, cut to the UNION / INTERSECT / EXCEPT branch containing `caret` (a token index).
fn block_range(st: &[Token], parens: &Parens, g: isize, caret: usize) -> (usize, usize) {
    let mut a = inside(g);
    let mut b = if g < 0 { st.len() } else { parens.close_of(inside(g) - 1) };
    for i in parens.level(g, a, b) {
        if !is_set_op(&st[i]) {
            continue;
        }
        if i < caret {
            a = i + 1;
        } else {
            b = i;
            break;
        }
    }
    (a, b)
}

/// The clause the caret is in: the last clause keyword of its block before it.
fn clause_at(st: &[Token], parens: &Parens, block: isize, innermost: isize, caret: usize) -> Clause {
    // Inside a non-query paren (function call, IN list), the clause is the one around that paren.
    let limit = if innermost > block { inside(innermost) - 1 } else { caret };
    let mut clause = Clause::Other;
    for i in parens.level(block, inside(block), limit) {
        let t = &st[i];
        if t.kind != TokenKind::Word {
            continue;
        }
        if is_set_op(t) {
            clause = Clause::Other;
        }
        if let Some(c) = clause_of(&t.upper) {
            clause = c;
        }
    }
    clause
}

fn parse_ctes(st: &[Token], parens: &Parens) -> Vec<TableRef> {
    let mut out = Vec::new();
    if !is_w(st.first(), &["WITH"]) {
        return out;
    }
    let mut i = 1;
    if is_w(st.get(i), &["RECURSIVE"]) {
        i += 1;
    }
    while i < st.len() && is_ident(st.get(i)) {
        let name_tok = &st[i];
        i += 1;
        let mut columns: Option<Vec<String>> = None;
        if is_p(st.get(i), "(") {
            let c = parens.close_of(i);
            columns = Some(
                parens
                    .level(i as isize, i + 1, c)
                    .into_iter()
                    .filter(|&j| is_ident(st.get(j)))
                    .map(|j| ident_name(&st[j]))
                    .collect(),
            );
            i = c + 1;
        }
        if !is_w(st.get(i), &["AS"]) {
            break;
        }
        i += 1;
        while is_w(st.get(i), &["NOT", "MATERIALIZED"]) {
            i += 1;
        }
        if !is_p(st.get(i), "(") {
            break;
        }
        let c = parens.close_of(i);
        let columns = columns.unwrap_or_else(|| output_columns(st, parens, i as isize, i + 1, c));
        out.push(TableRef {
            schema: None,
            name: ident_name(name_tok),
            alias: None,
            alias_at: None,
            kind: RefKind::Cte,
            columns: Some(columns),
            from: name_tok.from,
            to: name_tok.to,
            depth: 0,
            via_join: false,
        });
        i = c + 1;
        if !is_p(st.get(i), ",") {
            break;
        }
        i += 1;
    }
    out
}

/// Output column names of the query in group `g` (`a..b`): aliases and plain column references.
pub(crate) fn output_columns(st: &[Token], parens: &Parens, g: isize, a: usize, b: usize) -> Vec<String> {
    let level = parens.level(g, a, b);
    let Some(&sel) = level.iter().find(|&&i| is_w(st.get(i), &["SELECT"])) else { return Vec::new() };
    let mut out = Vec::new();
    let mut item: Vec<usize> = Vec::new();
    let flush = |item: &mut Vec<usize>, out: &mut Vec<String>| {
        let last = item.last().map(|&i| &st[i]);
        let prev = (item.len() > 1).then(|| &st[item[item.len() - 2]]);
        if let Some(last) = last
            && matches!(last.kind, TokenKind::Word | TokenKind::Quoted)
            && !is_w(Some(last), &["DISTINCT"])
            && (item.len() == 1 || is_w(prev, &["AS"]) || is_p(prev, ".") || is_p(prev, ")") || is_name(prev))
        {
            out.push(ident_name(last));
        }
        item.clear();
    };
    for i in level {
        if i <= sel {
            continue;
        }
        let t = &st[i];
        if t.kind == TokenKind::Word
            && (["FROM", "WHERE", "GROUP", "ORDER", "LIMIT"].contains(&t.upper.as_str()) || is_set_op(t))
        {
            break;
        }
        if is_w(Some(t), &["DISTINCT", "ALL"]) && item.is_empty() {
            continue;
        }
        if is_p(Some(t), ",") {
            flush(&mut item, &mut out);
        } else {
            item.push(i);
        }
    }
    if !item.is_empty() {
        flush(&mut item, &mut out);
    }
    out
}

/// What [`parse_refs`] reads from.
struct RefScan<'a> {
    st: &'a [Token],
    parens: &'a Parens,
    b: usize,
    ctes: &'a [TableRef],
    depth: usize,
    driver: DbDriver,
}

impl RefScan<'_> {
    /// The FROM item starting at `j`, with the index of its last token.
    fn item(&self, mut j: usize, via_join: bool, into: bool) -> Option<(TableRef, usize)> {
        let st = self.st;
        while is_w(st.get(j), &["LATERAL", "ONLY"]) {
            j += 1;
        }
        let t = st.get(j)?;
        if j >= self.b {
            return None;
        }
        if is_p(Some(t), "(") {
            let c = self.parens.close_of(j);
            let columns = output_columns(st, self.parens, j as isize, j + 1, c);
            let al = alias(st, c + 1, self.b)?;
            let at = Span { from: al.tok.from, to: al.tok.to };
            let reference = TableRef {
                schema: None,
                name: al.name.clone(),
                alias: Some(al.name),
                alias_at: Some(at),
                kind: RefKind::Derived,
                columns: Some(columns),
                from: at.from,
                to: at.to,
                depth: self.depth,
                via_join,
            };
            return Some((reference, al.end));
        }
        if !is_ident(Some(t)) {
            return None;
        }
        let mut parts = vec![t];
        let mut k = j + 1;
        while is_p(st.get(k), ".") && is_name(st.get(k + 1)) {
            parts.push(&st[k + 1]);
            k += 2;
        }
        let mut end = k - 1;
        if is_p(st.get(k), "(") && !into {
            // Table function: `generate_series(1, 3) g` (`INSERT INTO t (` is a column list).
            end = self.parens.close.get(&k).copied().unwrap_or(k);
            k = end + 1;
        }
        let last = parts[parts.len() - 1];
        let name = ident_name(last);
        let schema = (parts.len() > 1).then(|| ident_name(parts[parts.len() - 2]));
        let al = alias(st, k, self.b);
        let cte = if schema.is_none() {
            self.ctes.iter().find(|c| {
                if self.driver == DbDriver::Clickhouse {
                    c.name == name
                } else {
                    c.name.to_lowercase() == name.to_lowercase()
                }
            })
        } else {
            None
        };
        let end = al.as_ref().map_or(end, |al| al.end);
        let reference = TableRef {
            schema,
            name,
            alias_at: al.as_ref().map(|al| Span { from: al.tok.from, to: al.tok.to }),
            alias: al.map(|al| al.name),
            kind: if cte.is_some() { RefKind::Cte } else { RefKind::Table },
            columns: cte.and_then(|c| c.columns.clone()),
            from: last.from,
            to: last.to,
            depth: self.depth,
            via_join,
        };
        Some((reference, end))
    }
}

/// FROM / JOIN / UPDATE / INTO items of block `g` within `a..b`, each with the index of its last token.
#[expect(clippy::too_many_arguments, reason = "the scan's inputs are all plain values of one analysis")]
fn parse_refs(
    st: &[Token],
    parens: &Parens,
    g: isize,
    a: usize,
    b: usize,
    ctes: &[TableRef],
    depth: usize,
    driver: DbDriver,
) -> Vec<(TableRef, usize)> {
    let scan = RefScan { st, parens, b, ctes, depth, driver };
    let mut out = Vec::new();
    let mut clause = Clause::Other;
    for i in parens.level(g, a, b) {
        let t = &st[i];
        if t.kind == TokenKind::Word {
            if TABLE_INTRO.contains(&t.upper.as_str()) && t.upper != "TABLE" {
                // `INSERT INTO`, `DELETE FROM`, `SELECT … INTO var` (MySQL): only tables matter here.
                clause = Clause::From;
                let join = t.upper == "JOIN" || t.upper == "STRAIGHT_JOIN";
                out.extend(scan.item(i + 1, join, t.upper == "INTO"));
                continue;
            }
            if let Some(c) = clause_of(&t.upper) {
                clause = c;
            }
        } else if is_p(Some(t), ",") && clause == Clause::From {
            out.extend(scan.item(i + 1, false, false));
        }
    }
    out
}

struct Alias<'a> {
    name: String,
    tok: &'a Token,
    end: usize,
}

/// An alias at `k` (`AS x`, or a bare non-keyword identifier).
fn alias(st: &[Token], k: usize, b: usize) -> Option<Alias<'_>> {
    let mut j = k;
    let as_ = is_w(st.get(j), &["AS"]);
    if as_ {
        j += 1;
    }
    let t = st.get(j)?;
    if j >= b {
        return None;
    }
    let ok = t.kind == TokenKind::Quoted
        || (t.kind == TokenKind::Word && !is_clause_word(&t.upper) && (as_ || !is_reserved(&t.text)));
    ok.then(|| Alias { name: ident_name(t), tok: t, end: j })
}

/// The INSERT target (an index into the refs) when `open` is the `(` right after it (`INSERT INTO t (`).
fn insert_target(
    st: &[Token],
    parens: &Parens,
    open: Option<usize>,
    ref_ends: &HashMap<usize, usize>,
) -> Option<usize> {
    let open = open.filter(|&o| is_p(st.get(o), "("))?;
    let reference = *ref_ends.get(&open.checked_sub(1)?)?;
    // The ref must be introduced by INTO.
    for i in (0..open).rev() {
        if is_w(st.get(i), &["INTO"]) {
            return (parens.parent_of(i) == parens.parent_of(open)).then_some(reference);
        }
        if st[i].kind == TokenKind::Word && is_clause_word(&st[i].upper) {
            return None;
        }
    }
    None
}

/// For a SELECT block without FROM: the offset to append `FROM t` at (end of its select list).
fn from_insert_pos(st: &[Token], parens: &Parens, block: isize, pos: usize) -> Option<usize> {
    let end = if block < 0 { st.len() } else { parens.close_of(inside(block) - 1) };
    let level = parens.level(block, inside(block), end);
    if !is_w(level.first().and_then(|&i| st.get(i)), &["SELECT"]) {
        return None;
    }
    for &i in &level {
        let t = &st[i];
        if t.kind == TokenKind::Word
            && ((clause_of(&t.upper).is_some() && t.upper != "SELECT") || is_set_op(t) || t.upper == "INTO")
        {
            return None;
        }
    }
    let last = end.checked_sub(1).and_then(|i| st.get(i)).map_or(pos, |t| t.to);
    Some(pos.max(last))
}

/// Live template contexts at the analysed caret: `sql-statement` where a statement starts,
/// `sql-query` where a query may (subqueries, set operations), `sql-code` anywhere in code.
pub fn template_contexts(a: &Analysis) -> Vec<&'static str> {
    if a.context == SqlContext::None || a.qualifier.is_some() {
        return Vec::new();
    }
    let mut out = vec!["sql-code"];
    if a.context == SqlContext::Statement {
        out.extend(["sql-statement", "sql-query"]);
    } else if a.previous.as_deref().is_some_and(|p| p == "(" || SET_OPS.contains(&p) || p == "ALL") {
        out.push("sql-query");
    }
    out
}

/// The VALUES tuple slot at the caret of an INSERT: its target, explicit column list and value index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuesSlot {
    pub target: TableRef,
    /// The `(a, b)` list after the table; `None` when the INSERT lists none.
    pub columns: Option<Vec<String>>,
    pub index: usize,
}

pub fn values_slot(text: &str, pos: usize, driver: DbDriver) -> Option<ValuesSlot> {
    let a = analyze(text, pos, driver);
    if a.verb != "INSERT" && a.verb != "REPLACE" {
        return None;
    }
    let target = a.refs.iter().find(|r| r.depth == 0 && r.kind == RefKind::Table)?.clone();
    let toks = lex(&a.statement.text, driver).tokens;
    let caret = pos.saturating_sub(a.statement.from);
    let mut depth = 0usize;
    let mut columns: Option<Vec<String>> = None;
    let mut after_values = false;
    let mut index = 0usize;
    let mut in_tuple = false;
    for t in &toks {
        if t.from >= caret {
            break;
        }
        if is_p(Some(t), "(") {
            depth += 1;
            if depth == 1 && after_values {
                in_tuple = true;
                index = 0;
            } else if depth == 1 && !after_values && columns.is_none() && t.from > target.from {
                columns = Some(Vec::new());
            }
            continue;
        }
        if is_p(Some(t), ")") {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                in_tuple = false;
            }
            continue;
        }
        if depth == 0 && is_w(Some(t), &["VALUES"]) {
            after_values = true;
        } else if depth == 1
            && !after_values
            && let Some(columns) = columns.as_mut()
            && is_name(Some(t))
        {
            columns.push(ident_name(t));
        } else if depth == 1 && in_tuple && is_p(Some(t), ",") {
            index += 1;
        }
    }
    in_tuple.then(|| ValuesSlot { target, columns: columns.filter(|c| !c.is_empty()), index })
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::{at, caret};
    use super::*;

    fn ctx(sql: &str) -> SqlContext {
        at(sql, DbDriver::Sqlite).context
    }

    #[test]
    fn knows_what_is_expected_at_the_caret() {
        assert_eq!(ctx("|"), SqlContext::Statement);
        assert_eq!(ctx("select 1; sel|"), SqlContext::Statement);
        assert_eq!(ctx("select * from |"), SqlContext::Table { join: false });
        assert_eq!(ctx("select * from users u join |"), SqlContext::Table { join: true });
        assert!(matches!(ctx("select * from users u, |"), SqlContext::Table { .. }));
        assert_eq!(ctx("select | from users"), SqlContext::Column { clause: Clause::Select });
        assert_eq!(ctx("select * from users where |"), SqlContext::Column { clause: Clause::Where });
        assert!(matches!(ctx("select * from users u |"), SqlContext::AfterTable { .. }));
        assert!(matches!(ctx("select * from orders o join users u on |"), SqlContext::JoinCondition { .. }));
        assert!(matches!(ctx("insert into users (|)"), SqlContext::InsertColumns { .. }));
        assert!(matches!(ctx("insert into users (id, |"), SqlContext::InsertColumns { .. }));
        assert_eq!(ctx("insert into users (id) |"), SqlContext::AfterInsertColumns);
        assert_eq!(ctx("select 'x|'"), SqlContext::None);
        assert_eq!(ctx("select a as |"), SqlContext::None);
        assert!(!matches!(ctx("create table |"), SqlContext::Table { .. }));
    }

    #[test]
    fn resolves_aliases_ctes_derived_tables_and_enclosing_queries() {
        let a = at(
            "with recent as (select id, total as amount from orders) select r.| from recent r join users u on u.id = r.id",
            DbDriver::Sqlite,
        );
        assert_eq!(a.qualifier, Some(vec!["r".to_string()]));
        let r = a.refs.iter().find(|x| x.alias.as_deref() == Some("r")).expect("r is in scope");
        assert_eq!(r.kind, RefKind::Cte);
        assert_eq!(r.columns, Some(vec!["id".to_string(), "amount".to_string()]));

        let d = at("select x.| from (select name n, email from users) x", DbDriver::Sqlite);
        assert_eq!(d.refs[0].kind, RefKind::Derived);
        assert_eq!(d.refs[0].name, "x");
        assert_eq!(d.refs[0].columns, Some(vec!["n".to_string(), "email".to_string()]));

        let corr =
            at("select * from users u where exists (select 1 from orders o where o.user_id = |)", DbDriver::Sqlite);
        let names: Vec<(&str, usize)> = corr.refs.iter().map(|x| (x.name.as_str(), x.depth)).collect();
        assert_eq!(names, [("orders", 0), ("users", 1)]);

        let union = at("select id from users union select | from orders", DbDriver::Sqlite);
        let names: Vec<&str> = union.refs.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["orders"]);
    }

    #[test]
    fn finds_where_from_goes_when_a_select_has_none() {
        assert_eq!(at("select na|", DbDriver::Sqlite).from_insert, Some(9));
        assert_eq!(at("select na| from users", DbDriver::Sqlite).from_insert, None);
    }

    #[test]
    fn follows_the_statement_s_keyword_case() {
        assert!(at("select * from |", DbDriver::Sqlite).lower_keywords);
        assert!(!at("SELECT * FROM |", DbDriver::Sqlite).lower_keywords);
    }

    #[test]
    fn knows_where_statements_queries_and_code_go() {
        let t = |sql: &str| template_contexts(&at(sql, DbDriver::Sqlite));
        assert_eq!(t("sel|"), ["sql-code", "sql-statement", "sql-query"]);
        assert!(t("select 1; sel|").contains(&"sql-statement"));
        assert_eq!(t("select * from (sel|"), ["sql-code", "sql-query"]);
        assert_eq!(t("select 1 union sel|"), ["sql-code", "sql-query"]);
        assert_eq!(t("select * from users where sel|"), ["sql-code"]);
        assert!(t("select 'sel|'").is_empty());
    }

    #[test]
    fn finds_the_column_of_the_value_at_the_caret() {
        let slot = |sql: &str| {
            let (text, pos) = caret(sql);
            values_slot(&text, pos, DbDriver::Sqlite)
        };
        let s = slot("insert into users (email, name) values ('a', |)").expect("a slot");
        assert_eq!(s.columns, Some(vec!["email".to_string(), "name".to_string()]));
        assert_eq!(s.index, 1);
        let s = slot("insert into users values (|)").expect("a slot");
        assert_eq!((s.columns, s.index), (None, 0));
        assert_eq!(slot("insert into users (email) values ('a'), (|").map(|s| s.index), Some(0));
        assert_eq!(slot("insert into users (email|)"), None);
        assert_eq!(slot("select (|)"), None);
    }
}
