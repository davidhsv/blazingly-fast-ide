//! Name resolution over console SQL: what an identifier refers to (table, CTE, alias, column) for
//! inspections ("Unable to resolve…"), Go to Declaration and Quick Documentation. Built on the
//! caret analysis, so it is as lexical and forgiving as completion; names it cannot judge (unknown
//! tables' columns, dialect pseudo-columns) are left alone.

use super::analyze::{Clause, RefKind, Span, SqlContext, TableRef, analyze_tokens, is_p};
use super::dialect::{data_types, functions, is_clause_word, is_reserved, names_eq, names_eq_opt};
use super::lexer::{Lexed, Token, TokenKind, ident_name, lex};
use ide_db::{CompletionColumn, CompletionModel, CompletionTable, DbDriver};
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlSymbol<'m> {
    Table(&'m CompletionTable),
    Cte {
        name: String,
        def: Span,
    },
    /// A FROM item's alias (or a derived table); `table` when it aliases a catalog table.
    Alias {
        name: String,
        def: Span,
        table: Option<&'m CompletionTable>,
    },
    Column {
        name: String,
        column: Option<&'m CompletionColumn>,
        table: Option<&'m CompletionTable>,
        owner: String,
    },
    Schema {
        name: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved<'m> {
    pub from: usize,
    pub to: usize,
    pub symbol: Option<SqlSymbol<'m>>,
    /// Set when the name should resolve but doesn't.
    pub problem: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlProblem {
    pub from: usize,
    pub to: usize,
    pub message: String,
}

/// Statements whose names are checked; DDL and dialect commands are left alone.
const CHECKED: [&str; 6] = ["SELECT", "WITH", "INSERT", "UPDATE", "DELETE", "REPLACE"];
/// Qualifiers with a meaning of their own (upserts, triggers).
const PSEUDO_TABLES: [&str; 3] = ["EXCLUDED", "NEW", "OLD"];
/// Bare words that are not column references.
static NOT_COLUMNS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "TRUE FALSE NULL DEFAULT UNKNOWN CURRENT_DATE CURRENT_TIME CURRENT_TIMESTAMP LOCALTIME LOCALTIMESTAMP \
     CURRENT_USER SESSION_USER USER INTERVAL ESCAPE COLLATE NULLS FIRST LAST DO NOTHING CONFLICT RECURSIVE \
     ROWID OID CTID _ROWID_ GLOB REGEXP MATCH ROWS RANGE GROUPS PRECEDING FOLLOWING UNBOUNDED CURRENT ROW OVER \
     PARTITION FILTER WITHIN ONLY TIES NEXT IGNORE ABORT FAIL ROLLBACK DUPLICATE KEY ISNULL NOTNULL SIMILAR \
     TOP PERCENT ANY SOME ALL DISTINCT ZONE YEAR MONTH DAY HOUR MINUTE SECOND EPOCH SEPARATOR PRIOR LEADING \
     TRAILING BOTH NOWAIT SKIP LOCKED SHARE MODE BINARY UNSIGNED SIGNED"
        .split_whitespace()
        .collect()
});

/// The catalog table a (possibly schema-qualified) name refers to: an unqualified name prefers
/// the default schema.
pub fn find_table<'m>(
    model: Option<&'m CompletionModel>,
    schema: Option<&str>,
    name: &str,
    driver: DbDriver,
) -> Option<&'m CompletionTable> {
    let model = model?;
    let eq = |a: &str, b: &str| names_eq(driver, a, b);
    match schema {
        Some(schema) => model.tables.iter().find(|t| eq(&t.schema, schema) && eq(&t.name, name)),
        None => model
            .tables
            .iter()
            .find(|t| eq(&t.schema, &model.default_schema) && eq(&t.name, name))
            .or_else(|| model.tables.iter().find(|t| eq(&t.name, name))),
    }
}

fn is_name(t: Option<&Token>) -> bool {
    t.is_some_and(|t| matches!(t.kind, TokenKind::Word | TokenKind::Quoted))
}

fn is_word(t: Option<&Token>, upper: &str) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Word && t.upper == upper)
}

/// A column of a FROM item: from the catalog, or a name from the query text.
struct Col<'m> {
    name: String,
    column: Option<&'m CompletionColumn>,
}

pub struct SqlResolver<'m> {
    pub text: String,
    pub driver: DbDriver,
    pub model: Option<&'m CompletionModel>,
    pub lexed: Lexed,
    words: HashSet<String>,
    /// Token ranges `[a, b)` of the statements (`;` included), and each token's statement.
    statements: Vec<(usize, usize)>,
    statement_of: Vec<usize>,
}

impl<'m> SqlResolver<'m> {
    pub fn new(text: &str, driver: DbDriver, model: Option<&'m CompletionModel>) -> Self {
        let lexed = lex(text, driver);
        let words = data_types(driver)
            .iter()
            .map(|t| t.to_uppercase())
            .chain(functions(driver).iter().map(|f| f.name.to_uppercase()))
            .collect();
        let mut statements = Vec::new();
        let mut statement_of = Vec::with_capacity(lexed.tokens.len());
        let mut a = 0;
        for (i, t) in lexed.tokens.iter().enumerate() {
            statement_of.push(statements.len());
            if is_p(Some(t), ";") {
                statements.push((a, i + 1));
                a = i + 1;
            }
        }
        statements.push((a, lexed.tokens.len()));
        Self { text: text.to_string(), driver, model, lexed, words, statements, statement_of }
    }

    fn eq(&self, a: &str, b: &str) -> bool {
        names_eq(self.driver, a, b)
    }

    /// The tokens of token `i`'s statement: analysing only those keeps a long console linear.
    fn statement_tokens(&self, i: usize) -> &[Token] {
        let (a, b) = self.statements[self.statement_of[i]];
        &self.lexed.tokens[a..b]
    }

    /// The name token at `pos` (caret inside or at either edge).
    pub fn token_at(&self, pos: usize) -> Option<usize> {
        for (i, t) in self.lexed.tokens.iter().enumerate() {
            if t.from > pos {
                break;
            }
            if pos <= t.to && is_name(Some(t)) {
                return Some(i);
            }
        }
        None
    }

    pub fn resolve_at(&self, pos: usize) -> Option<Resolved<'m>> {
        self.resolve(self.token_at(pos)?)
    }

    fn found(span: Span, symbol: Option<SqlSymbol<'m>>) -> Option<Resolved<'m>> {
        Some(Resolved { from: span.from, to: span.to, symbol, problem: None })
    }

    fn problem(span: Span, message: String) -> Option<Resolved<'m>> {
        Some(Resolved { from: span.from, to: span.to, symbol: None, problem: Some(message) })
    }

    fn cte_symbol(c: &TableRef) -> SqlSymbol<'m> {
        SqlSymbol::Cte { name: c.name.clone(), def: Span { from: c.from, to: c.to } }
    }

    /// Resolves token `i`; `None` when it is no name (keywords, function names…).
    pub fn resolve(&self, i: usize) -> Option<Resolved<'m>> {
        let toks = &self.lexed.tokens;
        let t = toks.get(i)?;
        if !is_name(Some(t)) || (t.kind == TokenKind::Word && is_clause_word(&t.upper)) {
            return None;
        }
        let prev = i.checked_sub(1).and_then(|p| toks.get(p));
        let next = toks.get(i + 1);
        let name = ident_name(t);
        let span = Span { from: t.from, to: t.to };
        let a = analyze_tokens(&self.text, t.from, self.driver, self.statement_tokens(i), &self.lexed.comments);
        let checked = CHECKED.contains(&a.verb.as_str());
        let loaded = self.model.is_some_and(|m| !m.tables.is_empty());
        let qualifier = a.qualifier.as_deref();

        // `x.`: a table reference (or schema) qualifying the next name.
        if is_p(next, ".") && next.is_some_and(|n| n.from == t.to) && is_name(toks.get(i + 2)) {
            if matches!(a.context, SqlContext::Table { .. }) || qualifier.is_some_and(|q| !q.is_empty()) {
                return Some(self.schema(&name, span));
            }
            if let Some(r) = self.find_ref(&a.refs, None, &name) {
                return Self::found(span, Some(self.ref_symbol(r)));
            }
            if let Some(cte) = a.ctes.iter().find(|c| self.eq(&c.name, &name)) {
                return Self::found(span, Some(Self::cte_symbol(cte)));
            }
            if self.model.is_some_and(|m| m.tables.iter().any(|x| self.eq(&x.schema, &name))) {
                return Some(self.schema(&name, span));
            }
            if !checked || PSEUDO_TABLES.contains(&name.to_uppercase().as_str()) || a.context == SqlContext::None {
                return Self::found(span, None);
            }
            return Self::problem(span, format!("Unable to resolve symbol '{name}'"));
        }

        // An alias being defined.
        if let Some(defined) = a.refs.iter().find(|r| r.alias_at.is_some_and(|at| at.from == t.from)) {
            return Self::found(span, Some(self.ref_symbol(defined)));
        }

        let clause = match a.context {
            SqlContext::None
            | SqlContext::Statement
            | SqlContext::AfterTable { .. }
            | SqlContext::AfterInsertColumns => {
                return None;
            }
            SqlContext::Table { .. } => {
                // Table functions (`generate_series(1, 3)`), `DROP TABLE IF EXISTS`.
                if is_p(next, "(") && !is_into_target(toks, i) {
                    return None;
                }
                if t.kind == TokenKind::Word && ["IF", "ONLY", "LATERAL"].contains(&t.upper.as_str()) {
                    return None;
                }
                let schema = qualifier.and_then(|q| q.last()).map(String::as_str);
                if schema.is_none()
                    && let Some(cte) = a.ctes.iter().find(|c| self.eq(&c.name, &name))
                {
                    return Self::found(span, Some(Self::cte_symbol(cte)));
                }
                if let Some(table) = find_table(self.model, schema, &name, self.driver) {
                    return Self::found(span, Some(SqlSymbol::Table(table)));
                }
                if !loaded || (a.verb != "DROP" && a.verb != "ALTER" && !checked) {
                    return Self::found(span, None);
                }
                if let Some(schema) = schema
                    && !self.model.is_some_and(|m| m.tables.iter().any(|x| self.eq(&x.schema, schema)))
                {
                    return Self::found(span, None);
                }
                return Self::problem(span, format!("Unable to resolve table '{name}'"));
            }
            SqlContext::InsertColumns { target } => {
                let target = &a.refs[target];
                let cols = self.columns_of(target);
                let col = cols.as_ref().and_then(|cols| cols.iter().find(|c| self.eq(&c.name, &name)));
                return match (col, &cols) {
                    (Some(col), _) => Self::found(span, Some(self.column_symbol(target, col))),
                    (None, None) => Self::found(span, None),
                    (None, Some(_)) => Self::problem(span, format!("Unable to resolve column '{name}'")),
                };
            }
            SqlContext::Column { clause } => clause,
            SqlContext::JoinCondition { .. } => Clause::On,
        };

        if let Some(qualifier) = qualifier {
            let found = match qualifier {
                [only] => self.find_ref(&a.refs, None, only),
                [.., schema, table] => self.find_ref(&a.refs, Some(schema.as_str()), table),
                [] => None,
            };
            let Some(r) = found else { return Self::found(span, None) };
            let cols = self.columns_of(r);
            if let Some(col) = cols.as_ref().and_then(|cols| cols.iter().find(|c| self.eq(&c.name, &name))) {
                return Self::found(span, Some(self.column_symbol(r, col)));
            }
            if cols.is_none() || !checked {
                return Self::found(span, None);
            }
            return Self::problem(span, format!("Unable to resolve column '{name}'"));
        }

        // A bare word: a column unless it is a function, a type, a keyword or an alias being defined.
        if is_p(next, "(") || is_p(prev, "::") || is_word(prev, "AS") {
            return None;
        }
        if t.kind == TokenKind::Word
            && (is_reserved(&t.text) || NOT_COLUMNS.contains(t.upper.as_str()) || self.words.contains(&t.upper))
        {
            return None;
        }
        // `count(*) n`, `x total`: an alias after an expression.
        if prev.is_some_and(|p| {
            matches!(p.kind, TokenKind::Quoted | TokenKind::Number | TokenKind::Str)
                || is_p(Some(p), ")")
                || (p.kind == TokenKind::Word && !is_clause_word(&p.upper))
        }) {
            return None;
        }
        for r in &a.refs {
            if let Some(col) = self.columns_of(r).and_then(|cols| cols.into_iter().find(|c| self.eq(&c.name, &name))) {
                return Self::found(span, Some(self.column_symbol(r, &col)));
            }
        }
        if let Some(r) = self.find_ref(&a.refs, None, &name) {
            return Self::found(span, Some(self.ref_symbol(r)));
        }
        let alias_key = if self.driver == DbDriver::Clickhouse { name.clone() } else { name.to_lowercase() };
        let judged = checked
            && loaded
            && !a.refs.is_empty()
            && a.refs.iter().all(|r| self.columns_of(r).is_some())
            && !matches!(clause, Clause::Other | Clause::Values | Clause::Limit)
            && !self.output_aliases(i).contains(&alias_key);
        if judged { Self::problem(span, format!("Unable to resolve column '{name}'")) } else { Self::found(span, None) }
    }

    /// Every unresolved name of the text.
    pub fn problems(&self) -> Vec<SqlProblem> {
        if !self.model.is_some_and(|m| !m.tables.is_empty()) {
            return Vec::new();
        }
        (0..self.lexed.tokens.len())
            .filter(|&i| is_name(self.lexed.tokens.get(i)))
            .filter_map(|i| self.resolve(i))
            .filter_map(|r| r.problem.map(|message| SqlProblem { from: r.from, to: r.to, message }))
            .collect()
    }

    fn schema(&self, name: &str, span: Span) -> Resolved<'m> {
        let known = self.model.is_some_and(|m| m.tables.iter().any(|x| self.eq(&x.schema, name)));
        Resolved {
            from: span.from,
            to: span.to,
            symbol: known.then(|| SqlSymbol::Schema { name: name.to_string() }),
            problem: None,
        }
    }

    fn find_ref<'r>(&self, refs: &'r [TableRef], schema: Option<&str>, name: &str) -> Option<&'r TableRef> {
        refs.iter().find(|r| match &r.alias {
            Some(alias) => self.eq(alias, name),
            None => self.eq(&r.name, name) && schema.is_none_or(|s| names_eq_opt(self.driver, r.schema.as_deref(), s)),
        })
    }

    fn table_of(&self, r: &TableRef) -> Option<&'m CompletionTable> {
        if r.kind == RefKind::Table { find_table(self.model, r.schema.as_deref(), &r.name, self.driver) } else { None }
    }

    fn ref_symbol(&self, r: &TableRef) -> SqlSymbol<'m> {
        let table = self.table_of(r);
        if let (Some(alias), Some(def)) = (&r.alias, r.alias_at) {
            return SqlSymbol::Alias { name: alias.clone(), def, table };
        }
        match table {
            Some(table) => SqlSymbol::Table(table),
            None => Self::cte_symbol(r),
        }
    }

    /// Columns of a FROM item; `None` when unknown (not in the model, `select *` subqueries).
    fn columns_of(&self, r: &TableRef) -> Option<Vec<Col<'m>>> {
        if let Some(names) = &r.columns {
            return (!names.is_empty())
                .then(|| names.iter().map(|name| Col { name: name.clone(), column: None }).collect());
        }
        let table = find_table(self.model, r.schema.as_deref(), &r.name, self.driver)?;
        Some(table.columns.iter().map(|c| Col { name: c.name.clone(), column: Some(c) }).collect())
    }

    fn column_symbol(&self, r: &TableRef, col: &Col<'m>) -> SqlSymbol<'m> {
        SqlSymbol::Column {
            name: col.name.clone(),
            column: col.column,
            table: self.table_of(r),
            owner: r.ref_name().to_string(),
        }
    }

    /// Select-list aliases of token `i`'s statement (usable in ORDER BY / GROUP BY / HAVING).
    fn output_aliases(&self, i: usize) -> HashSet<String> {
        let toks = self.statement_tokens(i);
        toks.windows(2)
            .filter(|w| is_name(Some(&w[1])))
            .filter(|w| {
                let prev = &w[0];
                is_word(Some(prev), "AS")
                    || is_p(Some(prev), ")")
                    || matches!(prev.kind, TokenKind::Number | TokenKind::Str)
            })
            .map(|w| {
                let n = ident_name(&w[1]);
                if self.driver == DbDriver::Clickhouse { n } else { n.to_lowercase() }
            })
            .collect()
    }
}

/// Whether token `i` follows INSERT / REPLACE INTO (so a `(` after it lists columns).
fn is_into_target(toks: &[Token], i: usize) -> bool {
    let mut j = i as isize - 1;
    let at = |j: isize| usize::try_from(j).ok().and_then(|j| toks.get(j));
    while is_p(at(j), ".") && is_name(at(j - 1)) {
        j -= 2;
    }
    is_word(at(j), "INTO")
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::model;
    use super::*;

    fn problems(sql: &str, driver: DbDriver) -> Vec<String> {
        let m = model();
        let units = super::super::lexer::utf16(sql);
        SqlResolver::new(sql, driver, Some(&m))
            .problems()
            .into_iter()
            .map(|p| format!("{}: {}", String::from_utf16_lossy(&units[p.from..p.to]), p.message))
            .collect()
    }

    #[test]
    fn accepts_resolvable_statements() {
        for sql in [
            "select u.id, u.email, count(*) cnt from users u join orders o on o.user_id = u.id group by u.id order by cnt desc;",
            "select id, name as n from users where name like 'a%' order by n;",
            "with big as (select user_id, total from orders where total > 10) select b.user_id, total from big b;",
            "select * from (select id, email from users) x where x.email is not null;",
            "insert into users (email, name) values ('a', 'b');",
            "update users set name = upper(name) where id = 1;",
            "delete from orders where status = 'new' and total between 1 and 2;",
            "select cast(total as text), coalesce(status, 'x') from orders where id is null;",
            "select exists (select 1 from orders o where o.user_id = u.id) from users u;",
            "create table t (a int, b text); drop table if exists t;",
            "insert into users (email) values ('x') on conflict (email) do update set email = excluded.email;",
        ] {
            assert!(problems(sql, DbDriver::Sqlite).is_empty(), "{sql}: {:?}", problems(sql, DbDriver::Sqlite));
        }
    }

    #[test]
    fn flags_unresolved_tables_columns_and_qualifiers() {
        let p = |sql: &str| problems(sql, DbDriver::Sqlite);
        assert_eq!(p("select * from usres;"), ["usres: Unable to resolve table 'usres'"]);
        assert_eq!(p("select u.emial from users u;"), ["emial: Unable to resolve column 'emial'"]);
        assert_eq!(p("select x.id from users u;"), ["x: Unable to resolve symbol 'x'"]);
        assert_eq!(p("select nope from users;"), ["nope: Unable to resolve column 'nope'"]);
        assert_eq!(p("insert into users (emial) values (1);"), ["emial: Unable to resolve column 'emial'"]);
        assert_eq!(p("update users set nme = 1;"), ["nme: Unable to resolve column 'nme'"]);
    }

    #[test]
    fn leaves_names_alone_when_it_cannot_judge_them() {
        let p = |sql: &str| problems(sql, DbDriver::Sqlite);
        assert_eq!(p("select foo from unknown_table;"), ["unknown_table: Unable to resolve table 'unknown_table'"]);
        assert!(p("select * from (select * from users) x where x.whatever = 1;").is_empty());
        assert!(p("select rowid, * from users;").is_empty());
    }

    #[test]
    fn resolves_symbols_for_navigation_and_documentation() {
        let sql = "select u.email from users u join orders o on o.user_id = u.id;";
        let m = model();
        let r = SqlResolver::new(sql, DbDriver::Sqlite, Some(&m));
        let at = |needle: &str, nth: usize| {
            let mut i = None;
            for _ in 0..=nth {
                let start = i.map_or(0, |i| i + 1);
                i = sql[start..].find(needle).map(|k| k + start);
            }
            r.resolve_at(i.expect("the needle is in the text") + 1).and_then(|x| x.symbol)
        };
        let u_join = sql.find("u join").expect("alias");
        match at("u.email", 0) {
            Some(SqlSymbol::Alias { name, def, .. }) => assert_eq!((name.as_str(), def.from), ("u", u_join)),
            other => panic!("{other:?}"),
        }
        match at("email", 0) {
            Some(SqlSymbol::Column { name, owner, table, .. }) => {
                assert_eq!((name.as_str(), owner.as_str()), ("email", "u"));
                assert_eq!(table.map(|t| t.name.as_str()), Some("users"));
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(at("users", 0), Some(SqlSymbol::Table(t)) if t.name == "users"));
        assert!(matches!(at("u join", 0), Some(SqlSymbol::Alias { table: Some(t), .. }) if t.name == "users"));
        match at("user_id", 0) {
            Some(SqlSymbol::Column { owner, column, .. }) => {
                assert_eq!(owner, "o");
                assert_eq!(column.map(|c| c.data_type.as_str()), Some("INTEGER"));
            }
            other => panic!("{other:?}"),
        }
    }
}
