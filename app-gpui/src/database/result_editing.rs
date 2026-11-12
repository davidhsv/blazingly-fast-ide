//! Whether a console result can be edited in place: only when the query is a plain select of one
//! base table's rows — no joins, derived tables, CTEs, `DISTINCT` / `GROUP BY` / `HAVING` / window
//! functions or set operations — and its select list maps to that table's columns (plain column
//! references and `*`; aliased columns and expressions stay read-only). Purely lexical over the
//! statement's tokens, like the completion analysis. Also the statements an edit runs, for the
//! console's Output log.

use super::sql::complete::quote_ident;
use super::sql::dialect::{functions, is_clause_word};
use super::sql::lexer::{Token, TokenKind, ident_name, lex, slice16, utf16};
use ide_db::{CellValue, CompletionModel, DbDriver, DbRowEdit};
use std::collections::HashSet;

/// One select list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectItem {
    /// `*` or `t.*`: every column of the table.
    Star { qualifier: Option<String> },
    /// A plain column reference (`col`, `t.col`…); `qualifier` is what it is qualified with, when written.
    Column { qualifier: Option<String>, name: String },
    /// Anything else — an aliased column, a literal, a function call…: the column stays read-only.
    Expression,
}

/// Why a result cannot be edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditBlockReason {
    /// The query joins several tables.
    Joined,
    /// `DISTINCT`, `GROUP BY` / `HAVING`, a window function or an aggregate.
    Aggregated,
    /// `UNION` / `INTERSECT` / `EXCEPT`.
    SetOp,
    /// Not a plain select of one base table (another verb, a CTE, a derived table, no `FROM`…).
    Plain,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditableResult {
    /// The table's schema as written (`None`: unqualified).
    pub schema: Option<String>,
    pub table: String,
    /// The FROM item's alias, when written (`users u`, `users AS u`).
    pub alias: Option<String>,
    /// The select list, positionally aligned with the result's columns.
    pub items: Vec<SelectItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultEditability {
    Editable(EditableResult),
    ReadOnly(EditBlockReason),
}

const SET_OPS: [&str; 4] = ["UNION", "INTERSECT", "EXCEPT", "MINUS"];
const JOIN_WORDS: [&str; 10] =
    ["JOIN", "INNER", "LEFT", "RIGHT", "FULL", "CROSS", "NATURAL", "OUTER", "STRAIGHT_JOIN", "LATERAL"];
/// Words that end the FROM items (the clauses that may follow them).
const AFTER_FROM: [&str; 6] = ["WHERE", "ORDER", "LIMIT", "OFFSET", "FETCH", "FOR"];

/// A name as the server stores it: quoted as written, unquoted folded (PostgreSQL lower case,
/// Oracle upper case).
fn sql_name(token: &Token, driver: DbDriver) -> String {
    let name = ident_name(token);
    if token.kind == TokenKind::Quoted {
        return name;
    }
    match driver {
        DbDriver::Postgres => name.to_lowercase(),
        DbDriver::Oracle => name.to_uppercase(),
        _ => name,
    }
}

fn is_ident(t: Option<&Token>) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Quoted || (t.kind == TokenKind::Word && !is_clause_word(&t.upper)))
}

fn is_punct(t: Option<&Token>, p: &str) -> bool {
    t.is_some_and(|t| t.kind == TokenKind::Punct && t.text == p)
}

fn readonly(reason: EditBlockReason) -> ResultEditability {
    ResultEditability::ReadOnly(reason)
}

/// Length of the identifier chain (`a`, `a.b`, `a.b.c`…, dots included) the tokens start with.
fn chain_length(tokens: &[Token]) -> usize {
    if !is_ident(tokens.first()) {
        return 0;
    }
    let mut n = 1;
    while is_punct(tokens.get(n), ".") && is_ident(tokens.get(n + 1)) {
        n += 2;
    }
    n
}

/// One select list item: an identifier chain, plain or ending in `.*`, or an expression.
fn select_item_of(item: &[Token], driver: DbDriver) -> SelectItem {
    if item.len() == 1 && is_punct(item.first(), "*") {
        return SelectItem::Star { qualifier: None };
    }
    if item.len() >= 3 && is_punct(item.last(), "*") && is_punct(item.get(item.len() - 2), ".") {
        let chain = &item[..item.len() - 2];
        let n = chain_length(chain);
        return if n == chain.len() && n > 0 {
            SelectItem::Star { qualifier: Some(sql_name(&chain[n - 1], driver)) }
        } else {
            SelectItem::Expression
        };
    }
    let n = chain_length(item);
    if n != item.len() || n == 0 {
        return SelectItem::Expression;
    }
    if n == 1 {
        SelectItem::Column { qualifier: None, name: sql_name(&item[0], driver) }
    } else {
        // `a.b` / `a.b.c`: the qualifier is the ident before the last dot.
        SelectItem::Column { qualifier: Some(sql_name(&item[n - 3], driver)), name: sql_name(&item[n - 1], driver) }
    }
}

/// What a console statement's result grid can do: editable with the base table and the select
/// list, or read-only with the reason.
pub fn result_editability(text: &str, driver: DbDriver) -> ResultEditability {
    let units = utf16(text);
    let lexed = lex(text, driver);
    if lexed.comments.iter().any(|c| slice16(&units, c.from + 2, c.to).contains("/*")) {
        return readonly(EditBlockReason::Plain);
    }
    if driver == DbDriver::Mysql
        && lexed.comments.iter().any(|c| {
            let comment = slice16(&units, c.from, c.to);
            comment.starts_with("/*!") || comment.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("/*m!"))
        })
    {
        return readonly(EditBlockReason::Plain);
    }
    let mut all = lexed.tokens;
    if driver == DbDriver::Postgres && all.iter().any(|t| t.kind == TokenKind::Str && t.text.contains('\\')) {
        return readonly(EditBlockReason::Plain);
    }
    if all.iter().any(|t| t.kind == TokenKind::Word && SET_OPS.contains(&t.upper.as_str())) {
        return readonly(EditBlockReason::SetOp);
    }
    if all.last().is_some_and(|t| t.text == ";") {
        all.pop();
    }
    if all.iter().enumerate().any(|(i, t)| {
        (t.kind == TokenKind::Word && (t.upper == "SELECT" || t.upper == "WITH") && i > 0) || is_punct(Some(t), ";")
    }) {
        return readonly(EditBlockReason::Plain);
    }
    if !all.first().is_some_and(|t| t.kind == TokenKind::Word && t.upper == "SELECT") {
        return readonly(EditBlockReason::Plain);
    }
    let aggregates: HashSet<String> =
        functions(driver).iter().filter(|f| f.aggregate).map(|f| f.name.to_uppercase()).collect();

    // Top-level landmarks over the whole statement (the lexer dropped comments).
    let mut depth = 0isize;
    let mut from = None;
    for (j, tok) in all.iter().enumerate() {
        if is_punct(Some(tok), "(") {
            depth += 1;
        } else if is_punct(Some(tok), ")") {
            depth -= 1;
        }
        if tok.kind != TokenKind::Word {
            continue;
        }
        let w = tok.upper.as_str();
        if w == "OVER" || w == "WINDOW" {
            return readonly(EditBlockReason::Aggregated);
        }
        if aggregates.contains(w) && all.get(j + 1).is_some_and(|n| n.text == "(") {
            return readonly(EditBlockReason::Aggregated);
        }
        if depth != 0 {
            continue;
        }
        if j == 1 && w == "DISTINCT" {
            return readonly(EditBlockReason::Aggregated);
        }
        if w == "INTO" {
            return readonly(EditBlockReason::Plain);
        }
        if w == "GROUP" || w == "HAVING" {
            return readonly(EditBlockReason::Aggregated);
        }
        if w == "FROM" && from.is_none() {
            from = Some(j);
        }
    }
    let Some(from) = from else { return readonly(EditBlockReason::Plain) };

    let select_list = &all[1..from];
    if select_list.is_empty() {
        return readonly(EditBlockReason::Plain);
    }

    // Select items: top-level commas separate them.
    let mut items = Vec::new();
    let mut item_start = 0;
    let mut item_depth = 0isize;
    for j in 0..=select_list.len() {
        let tok = select_list.get(j);
        if is_punct(tok, "(") {
            item_depth += 1;
        } else if is_punct(tok, ")") {
            item_depth -= 1;
        }
        let ends = j == select_list.len();
        if !ends && !(item_depth == 0 && is_punct(tok, ",")) {
            continue;
        }
        let item = &select_list[item_start..j];
        item_start = j + 1;
        if !item.is_empty() {
            items.push(select_item_of(item, driver));
        }
    }
    if items.is_empty() {
        return readonly(EditBlockReason::Plain);
    }

    // The FROM items: one `[schema.]table` (with an optional alias) and nothing else.
    let rest = &all[from + 1..];
    let mut end = rest.len();
    let mut depth = 0isize;
    for (j, tok) in rest.iter().enumerate() {
        if is_punct(Some(tok), "(") {
            depth += 1;
        } else if is_punct(Some(tok), ")") {
            depth -= 1;
        }
        if depth == 0 && tok.kind == TokenKind::Word && AFTER_FROM.contains(&tok.upper.as_str()) {
            end = j;
            break;
        }
    }
    let mut item = &rest[..end];
    if item.first().is_some_and(|t| t.kind == TokenKind::Word && t.upper == "ONLY") {
        return readonly(EditBlockReason::Plain);
    }
    // A statement's trailing `;` is not part of the FROM item.
    while is_punct(item.last(), ";") {
        item = &item[..item.len() - 1];
    }
    // A derived table or a parenthesized join.
    if item.first().is_none_or(|t| t.kind == TokenKind::Punct) {
        return readonly(EditBlockReason::Plain);
    }
    let n = chain_length(item);
    if n == 0 || n > 3 {
        return readonly(EditBlockReason::Plain);
    }
    let mut at = n;
    let mut alias = None;
    if item.get(at).is_some_and(|t| t.kind == TokenKind::Word && t.upper == "AS") {
        at += 1;
        if !is_ident(item.get(at)) {
            return readonly(EditBlockReason::Plain);
        }
        alias = Some(sql_name(&item[at], driver));
        at += 1;
    } else if is_ident(item.get(at)) {
        alias = Some(sql_name(&item[at], driver));
        at += 1;
    }
    if let Some(tok) = item.get(at) {
        let joined =
            is_punct(Some(tok), ",") || (tok.kind == TokenKind::Word && JOIN_WORDS.contains(&tok.upper.as_str()));
        return readonly(if joined { EditBlockReason::Joined } else { EditBlockReason::Plain });
    }
    ResultEditability::Editable(EditableResult {
        // The ident before the last one (`public.users`, `db.schema.table`; dots are part of `n`).
        schema: (n >= 3).then(|| sql_name(&item[n - 3], driver)),
        table: sql_name(&item[n - 1], driver),
        alias,
        items,
    })
}

/// The table an edit writes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditTarget {
    pub schema: Option<String>,
    pub table: String,
    pub alias: Option<String>,
}

/// Resolves unqualified names in the schema the result was executed in (`schema`; `Some("")`
/// when that is unknown). `None` when the target is ambiguous.
pub fn resolve_target(
    e: &EditableResult,
    model: Option<&CompletionModel>,
    schema: Option<&str>,
    driver: DbDriver,
) -> Option<EditTarget> {
    let default_schema = model.map(|m| m.default_schema.as_str());
    let picked = schema.filter(|s| !s.is_empty());
    // SQL Server switches databases, but table details and edits take a table schema.
    if driver == DbDriver::SqlServer && (e.schema.is_none() || picked.is_some_and(|s| Some(s) != default_schema)) {
        return None;
    }
    if e.schema.is_none() && schema == Some("") {
        return None;
    }
    let schema = e.schema.clone().or_else(|| picked.or(default_schema.filter(|d| !d.is_empty())).map(str::to_string));
    Some(EditTarget { schema, table: e.table.clone(), alias: e.alias.clone() })
}

/// The table column behind each result column: the select items aligned positionally with the
/// result's columns. `None` for expressions and aliased columns, and for `*` columns the table
/// doesn't have. `table_columns` may be `None` while the table's details are loading (the names
/// are then trusted).
pub fn result_column_sources(
    items: &[SelectItem],
    result_names: &[String],
    table_columns: Option<&[String]>,
    eq: impl Fn(&str, &str) -> bool,
) -> Vec<Option<String>> {
    let mut sources: Vec<Option<String>> = vec![None; result_names.len()];
    let mut r = 0;
    for (i, item) in items.iter().enumerate() {
        let pending = items.len() - i - 1;
        if r >= result_names.len() {
            break;
        }
        match item {
            SelectItem::Column { name, .. } => {
                let found = match table_columns {
                    Some(cols) => cols.iter().find(|c| eq(c, name)).cloned(),
                    None => Some(name.clone()),
                };
                sources[r] = found.filter(|n| eq(&result_names[r], n));
                r += 1;
            }
            SelectItem::Expression => r += 1,
            SelectItem::Star { .. } => {
                // `*`: every result column left for it (the items after it need theirs).
                let take = result_names.len().saturating_sub(r + pending);
                for _ in 0..take {
                    if r >= result_names.len() {
                        break;
                    }
                    let name = &result_names[r];
                    if table_columns.is_none_or(|cols| cols.iter().any(|c| eq(c, name))) {
                        sources[r] = Some(name.clone());
                    }
                    r += 1;
                }
            }
        }
    }
    if r == result_names.len() { sources } else { vec![None; result_names.len()] }
}

/// The result columns that carry the table's primary key (their names go into the generated
/// `UPDATE` / `DELETE`), or `None` when the result doesn't include every key column or repeats a
/// column label.
pub fn pk_result_columns(
    table_pk: &[String],
    sources: &[Option<String>],
    result_names: &[String],
    eq: impl Fn(&str, &str) -> bool,
) -> Option<Vec<String>> {
    let duplicated = result_names.iter().enumerate().any(|(i, name)| result_names[..i].iter().any(|o| eq(name, o)));
    if table_pk.is_empty() || duplicated {
        return None;
    }
    table_pk
        .iter()
        .map(|key| {
            let at = sources.iter().position(|s| s.as_deref().is_some_and(|s| eq(s, key)))?;
            result_names.get(at).cloned()
        })
        .collect()
}

fn literal_of(driver: DbDriver, v: Option<&str>) -> String {
    let Some(v) = v else { return "NULL".into() };
    let mut s = v.replace('\'', "''");
    if driver == DbDriver::Mysql || driver == DbDriver::Clickhouse {
        s = s.replace('\\', "\\\\");
    }
    format!("'{s}'")
}

fn condition_of(driver: DbDriver, key: &[CellValue]) -> String {
    key.iter()
        .map(|c| match &c.value {
            None => format!("{} IS NULL", quote_ident(&c.column, driver)),
            Some(v) => format!("{} = {}", quote_ident(&c.column, driver), literal_of(driver, Some(v))),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

/// The statements applying `edits` runs, for the console's Output log: the same `UPDATE` /
/// `INSERT` / `DELETE` the backend generates.
pub fn edit_statements(driver: DbDriver, schema: &str, table: &str, edits: &[DbRowEdit]) -> Vec<String> {
    let quoted = |name: &str| quote_ident(name, driver);
    let target = if schema.is_empty() { quoted(table) } else { format!("{}.{}", quoted(schema), quoted(table)) };
    edits
        .iter()
        .map(|e| match e {
            DbRowEdit::Update { key, values } => {
                let set: Vec<String> = values
                    .iter()
                    .map(|c| format!("{} = {}", quoted(&c.column), literal_of(driver, c.value.as_deref())))
                    .collect();
                format!("UPDATE {target} SET {} WHERE {}", set.join(", "), condition_of(driver, key))
            }
            DbRowEdit::Delete { key } => format!("DELETE FROM {target} WHERE {}", condition_of(driver, key)),
            DbRowEdit::Insert { values } if values.is_empty() => {
                if driver == DbDriver::Mysql {
                    format!("INSERT INTO {target} () VALUES ()")
                } else {
                    format!("INSERT INTO {target} DEFAULT VALUES")
                }
            }
            DbRowEdit::Insert { values } => {
                let columns: Vec<String> = values.iter().map(|c| quoted(&c.column)).collect();
                let literals: Vec<String> = values.iter().map(|c| literal_of(driver, c.value.as_deref())).collect();
                format!("INSERT INTO {target} ({}) VALUES ({})", columns.join(", "), literals.join(", "))
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ide_db::{CompletionTable, DbTableKind};

    fn editable_in(text: &str, driver: DbDriver) -> EditableResult {
        match result_editability(text, driver) {
            ResultEditability::Editable(e) => e,
            other => panic!("{text}: {other:?}"),
        }
    }

    fn editable(text: &str) -> EditableResult {
        editable_in(text, DbDriver::Postgres)
    }

    fn readonly_in(text: &str, driver: DbDriver) -> EditBlockReason {
        match result_editability(text, driver) {
            ResultEditability::ReadOnly(r) => r,
            other => panic!("{text}: {other:?}"),
        }
    }

    fn readonly(text: &str) -> EditBlockReason {
        readonly_in(text, DbDriver::Postgres)
    }

    fn eq(a: &str, b: &str) -> bool {
        !a.is_empty() && !b.is_empty() && a.to_lowercase() == b.to_lowercase()
    }

    fn column(qualifier: Option<&str>, name: &str) -> SelectItem {
        SelectItem::Column { qualifier: qualifier.map(str::to_string), name: name.into() }
    }

    fn star(qualifier: Option<&str>) -> SelectItem {
        SelectItem::Star { qualifier: qualifier.map(str::to_string) }
    }

    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn some(v: &[Option<&str>]) -> Vec<Option<String>> {
        v.iter().map(|s| s.map(str::to_string)).collect()
    }

    use EditBlockReason::{Aggregated, Joined, Plain, SetOp};

    #[test]
    fn accepts_a_plain_select_of_one_table() {
        let e = editable("SELECT * FROM users WHERE id < 10 ORDER BY id LIMIT 5");
        assert_eq!(e.schema, None);
        assert_eq!(e.table, "users");
        assert_eq!(e.alias, None);
        assert_eq!(e.items, [star(None)]);
    }

    #[test]
    fn accepts_a_schema_qualified_table_aliases_and_plain_column_references() {
        let e = editable("select u.id, u.name\nfrom public.users as u\nwhere u.id > 1\norder by u.id");
        assert_eq!(e.schema.as_deref(), Some("public"));
        assert_eq!(e.table, "users");
        assert_eq!(e.alias.as_deref(), Some("u"));
        assert_eq!(e.items, [column(Some("u"), "id"), column(Some("u"), "name")]);
    }

    #[test]
    fn accepts_qualified_stars_and_for_update() {
        assert_eq!(editable("SELECT u.* FROM users u FOR UPDATE").items, [star(Some("u"))]);
        assert_eq!(editable("SELECT id FROM users").items, [column(None, "id")]);
    }

    #[test]
    fn separates_an_implicit_alias_from_a_schema_qualified_table() {
        let e = editable("SELECT u.id, u.name FROM public.users u");
        assert_eq!((e.schema.as_deref(), e.table.as_str(), e.alias.as_deref()), (Some("public"), "users", Some("u")));
        assert_eq!(readonly("SELECT a.id FROM s1.users a JOIN s2.users b USING (id)"), Joined);
        assert_eq!(readonly("SELECT a.id FROM s1.users a, s2.users b"), Joined);
    }

    #[test]
    fn keeps_a_quoted_identifier_a_column_reference() {
        assert_eq!(
            editable_in("SELECT \"name\", \"user id\" FROM \"my table\"", DbDriver::Sqlite).items,
            [column(None, "name"), column(None, "user id")]
        );
    }

    #[test]
    fn rejects_joins_comma_lists_and_derived_tables_as_joined_plain() {
        assert_eq!(readonly("SELECT * FROM a JOIN b ON a.id = b.id"), Joined);
        assert_eq!(readonly("SELECT * FROM a LEFT OUTER JOIN b ON a.id = b.id"), Joined);
        assert_eq!(readonly("SELECT * FROM a, b WHERE a.id = b.id"), Joined);
        assert_eq!(readonly("SELECT * FROM a CROSS JOIN b"), Joined);
        assert_eq!(readonly("SELECT * FROM (SELECT id FROM a) x"), Plain);
    }

    #[test]
    fn rejects_aggregates_distinct_group_by_having_and_windows_as_aggregated() {
        assert_eq!(readonly("SELECT count(*) FROM users"), Aggregated);
        assert_eq!(readonly("SELECT max(id) + 1 FROM users"), Aggregated);
        assert_eq!(readonly("SELECT DISTINCT name FROM users"), Aggregated);
        assert_eq!(readonly("SELECT name FROM users GROUP BY name"), Aggregated);
        assert_eq!(readonly("SELECT name FROM users GROUP BY name HAVING count(*) > 1"), Aggregated);
        assert_eq!(readonly("SELECT name, row_number() OVER () FROM users"), Aggregated);
        // A column called `min` (no call parens) is not an aggregate.
        assert_eq!(editable("SELECT min FROM mins").items, [column(None, "min")]);
    }

    #[test]
    fn rejects_set_operations() {
        assert_eq!(readonly("SELECT id FROM a UNION SELECT id FROM b"), SetOp);
        assert_eq!(readonly("SELECT id FROM a INTERSECT SELECT id FROM b"), SetOp);
        assert_eq!(readonly("SELECT id FROM a EXCEPT SELECT id FROM b"), SetOp);
    }

    #[test]
    fn rejects_anything_that_is_not_a_plain_single_table_select() {
        for sql in [
            "WITH t AS (SELECT 1) SELECT * FROM t",
            "SELECT 1",
            "SELECT *",
            "SELECT * INTO copy FROM users",
            "INSERT INTO users VALUES (1)",
            "UPDATE users SET id = 1",
            "DELETE FROM users",
            "TABLE users",
        ] {
            assert_eq!(readonly(sql), Plain, "{sql}");
        }
    }

    #[test]
    fn refuses_ambiguous_provenance() {
        for sql in [
            "SELECT id, (SELECT name FROM logs LIMIT 1) FROM users",
            "SELECT * FROM users WHERE id IN (SELECT id FROM logs)",
            "SELECT id, coalesce(sum(id), 0) FROM users",
            "SELECT id, (row_number() OVER ()) FROM users",
            "SELECT * FROM db.public.users",
            "SELECT * FROM users WHERE id > 0; SELECT * FROM logs",
            "SELECT id FROM ONLY users",
            "SELECT id /* outer /* inner */ FROM wrong WHERE 1=1 */ FROM users",
            r"SELECT id, E'ignored\' FROM wrong WHERE 1=1 --' FROM users",
        ] {
            assert!(matches!(result_editability(sql, DbDriver::Postgres), ResultEditability::ReadOnly(_)), "{sql}");
        }
    }

    #[test]
    fn rejects_executable_comments() {
        for sql in [
            "SELECT id, name FROM users /*!50000 UNION SELECT id, name FROM admins */",
            "SELECT id, name FROM users /*M! UNION SELECT id, name FROM admins */",
        ] {
            assert!(matches!(result_editability(sql, DbDriver::Mysql), ResultEditability::ReadOnly(_)), "{sql}");
        }
    }

    #[test]
    fn folds_unquoted_identifiers_per_dialect_and_preserves_quoted_case() {
        assert_eq!(editable("SELECT ID FROM PUBLIC.USERS").table, "users");
        assert_eq!(editable("SELECT \"ID\" FROM \"Users\"").table, "Users");
        assert_eq!(editable_in("SELECT id FROM users", DbDriver::Oracle).table, "USERS");
    }

    #[test]
    fn keeps_aliased_columns_and_expressions_read_only_per_column() {
        use SelectItem::Expression;
        assert_eq!(editable("SELECT id, name AS n FROM users").items, [column(None, "id"), Expression]);
        assert_eq!(editable("SELECT id, name n FROM users").items, [column(None, "id"), Expression]);
        assert_eq!(
            editable("SELECT id || '-x', id::text, 'k', 5 FROM users").items,
            [Expression, Expression, Expression, Expression]
        );
    }

    #[test]
    fn reads_the_table_of_a_statement_with_trailing_semicolon_and_comments() {
        assert_eq!(editable("SELECT id FROM users; -- latest").table, "users");
    }

    fn model() -> CompletionModel {
        let table = |schema: &str, name: &str| CompletionTable {
            schema: schema.into(),
            name: name.into(),
            kind: DbTableKind::Table,
            columns: Vec::new(),
        };
        CompletionModel {
            default_schema: "public".into(),
            tables: vec![table("public", "users"), table("other", "public")],
            foreign_keys: Vec::new(),
        }
    }

    fn target(schema: Option<&str>, table: &str, alias: Option<&str>) -> Option<EditTarget> {
        Some(EditTarget { schema: schema.map(str::to_string), table: table.into(), alias: alias.map(str::to_string) })
    }

    #[test]
    fn keeps_a_known_schema_table() {
        let e = editable("SELECT * FROM public.users");
        assert_eq!(resolve_target(&e, Some(&model()), None, DbDriver::Postgres), target(Some("public"), "users", None));
    }

    #[test]
    fn preserves_explicit_qualification_even_with_incomplete_completion_metadata() {
        let e = editable("SELECT * FROM public.users");
        let mut m = model();
        m.tables.remove(0);
        assert_eq!(resolve_target(&e, Some(&m), None, DbDriver::Postgres), target(Some("public"), "users", None));
    }

    #[test]
    fn uses_the_execution_schema_for() {
        for driver in [DbDriver::Postgres, DbDriver::Mysql, DbDriver::Oracle] {
            let e = editable_in("SELECT * FROM users u", driver);
            let m = model();
            assert_eq!(resolve_target(&e, Some(&m), Some("s2"), driver).and_then(|t| t.schema).as_deref(), Some("s2"));
            assert_eq!(resolve_target(&e, Some(&m), None, driver).and_then(|t| t.schema).as_deref(), Some("public"));
        }
    }

    #[test]
    fn passes_an_unknown_schema_table_through() {
        let e = editable("SELECT * FROM nope.missing");
        assert_eq!(resolve_target(&e, Some(&model()), None, DbDriver::Postgres), target(Some("nope"), "missing", None));
    }

    #[test]
    fn refuses_unresolved_defaults_and_sql_server_database_schema_ambiguity() {
        let m = model();
        assert_eq!(resolve_target(&editable("SELECT * FROM users"), Some(&m), Some(""), DbDriver::Postgres), None);
        assert_eq!(
            resolve_target(&editable("SELECT * FROM users"), Some(&m), Some("public"), DbDriver::SqlServer),
            None
        );
        let explicit = editable("SELECT * FROM dbo.users");
        assert_eq!(resolve_target(&explicit, Some(&m), Some("other_database"), DbDriver::SqlServer), None);
        assert_eq!(
            resolve_target(&explicit, Some(&m), Some("public"), DbDriver::SqlServer),
            target(Some("dbo"), "users", None)
        );
    }

    #[test]
    fn does_not_touch_unqualified_or_explicitly_aliased_names() {
        let e = editable("SELECT * FROM users u");
        assert_eq!(resolve_target(&e, None, None, DbDriver::Postgres), target(None, "users", Some("u")));
    }

    #[test]
    fn maps_plain_columns_positionally_and_stars_against_the_table_s_columns() {
        let table = names(&["id", "name"]);
        assert_eq!(
            result_column_sources(&[column(None, "id")], &names(&["id"]), Some(&table), eq),
            some(&[Some("id")])
        );
        assert_eq!(
            result_column_sources(&[star(None)], &names(&["id", "name"]), Some(&table), eq),
            some(&[Some("id"), Some("name")])
        );
        // A `*` column the table doesn't have (a computed column of a view): read-only.
        assert_eq!(
            result_column_sources(&[star(None)], &names(&["id", "extra"]), Some(&names(&["id"])), eq),
            some(&[Some("id"), None])
        );
        // While the details are loading, the names are trusted.
        assert_eq!(
            result_column_sources(&[star(None)], &names(&["id", "extra"]), None, eq),
            some(&[Some("id"), Some("extra")])
        );
    }

    #[test]
    fn aligns_mixed_items_positionally() {
        let items = [SelectItem::Expression, star(None)];
        assert_eq!(
            result_column_sources(&items, &names(&["count", "id", "name"]), Some(&names(&["id", "name"])), eq),
            some(&[None, Some("id"), Some("name")])
        );
    }

    #[test]
    fn honours_the_star_qualifier_only_through_the_caller_s_qualifier_check() {
        // The qualifier match against the alias / table name happens before mapping; here both pass.
        let items = [star(Some("u")), column(Some("u"), "id")];
        assert_eq!(
            result_column_sources(&items, &names(&["name", "id"]), Some(&names(&["id", "name"])), eq),
            some(&[Some("name"), Some("id")])
        );
    }

    #[test]
    fn maps_nothing_when_the_items_cannot_line_up_with_the_result() {
        assert_eq!(result_column_sources(&[column(None, "a")], &names(&["a", "b"]), None, eq), some(&[None, None]));
        assert!(result_column_sources(&[star(None)], &[], None, eq).is_empty());
    }

    #[test]
    fn refuses_duplicate_result_labels_that_could_shadow_a_primary_key() {
        let pk = names(&["id"]);
        assert_eq!(
            pk_result_columns(&pk, &some(&[None, Some("id"), Some("name")]), &names(&["id", "id", "name"]), eq),
            None
        );
        assert_eq!(pk_result_columns(&pk, &some(&[Some("id"), Some("name")]), &names(&["id", "ID"]), eq), None);
    }

    #[test]
    fn collects_the_result_columns_of_every_key_column_or_null_when_one_is_missing() {
        let both = names(&["id", "name"]);
        assert_eq!(
            pk_result_columns(&names(&["id"]), &some(&[Some("id"), Some("name")]), &both, eq),
            Some(names(&["id"]))
        );
        let three = names(&["tenant", "name", "id"]);
        assert_eq!(
            pk_result_columns(
                &names(&["id", "tenant"]),
                &some(&[Some("tenant"), Some("name"), Some("id")]),
                &three,
                eq
            ),
            Some(names(&["id", "tenant"]))
        );
        assert_eq!(pk_result_columns(&names(&["id"]), &some(&[None, Some("name")]), &both, eq), None);
        assert_eq!(pk_result_columns(&[], &some(&[Some("id")]), &names(&["id"]), eq), None);
    }

    #[test]
    fn renders_the_update_insert_delete_the_backend_applies() {
        let cell =
            |column: &str, value: Option<&str>| CellValue { column: column.into(), value: value.map(str::to_string) };
        let edits = [
            DbRowEdit::Update { key: vec![cell("id", Some("1"))], values: vec![cell("name", Some("O'Brien"))] },
            DbRowEdit::Delete { key: vec![cell("id", Some("2"))] },
            DbRowEdit::Insert { values: vec![cell("id", None), cell("name", Some("x"))] },
        ];
        assert_eq!(
            edit_statements(DbDriver::Postgres, "public", "users", &edits),
            [
                "UPDATE public.users SET name = 'O''Brien' WHERE id = '1'",
                "DELETE FROM public.users WHERE id = '2'",
                "INSERT INTO public.users (id, name) VALUES (NULL, 'x')",
            ]
        );
        let empty = [DbRowEdit::Insert { values: Vec::new() }];
        assert_eq!(edit_statements(DbDriver::Mysql, "", "users", &empty), ["INSERT INTO users () VALUES ()"]);
        assert_eq!(edit_statements(DbDriver::Postgres, "", "users", &empty), ["INSERT INTO users DEFAULT VALUES"]);
        let null_key = [DbRowEdit::Delete { key: vec![cell("id", None)] }];
        assert_eq!(edit_statements(DbDriver::Postgres, "", "users", &null_key), ["DELETE FROM users WHERE id IS NULL"]);
    }
}
