//! Query parameters of console statements (`:id`, `?`, `?1`, `@name`, `$1`, ClickHouse
//! `{name:Type}`), filled in before execution: each value is inserted as SQL text, so strings carry
//! their quotes. Named parameters share one value; every bare `?` is its own.

use super::lexer::{TokenKind, lex, slice16, utf16};
use ide_db::DbDriver;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlParam {
    /// Shown in the dialog and the key of its value (`:id`, `?2`).
    pub name: String,
    pub from: usize,
    pub to: usize,
    /// A bare `?`, numbered in order.
    pub positional: bool,
}

static TYPED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\{[A-Za-z_][A-Za-z0-9_]*\s*:\s*[^{}]+\}$").expect("the pattern is valid"));

/// Parameters of `text`; bare `?` are numbered from `numbered + 1` (earlier statements' count).
pub fn find_params(text: &str, driver: DbDriver, numbered: usize) -> Vec<SqlParam> {
    let units = utf16(text);
    let toks = lex(text, driver).tokens;
    let mut out = Vec::new();
    let mut positional = numbered;
    let mut typed_end = 0;
    for (i, t) in toks.iter().enumerate() {
        if t.from < typed_end {
            continue;
        }
        if driver == DbDriver::Clickhouse && t.kind == TokenKind::Punct && t.text == "{" {
            let end = toks[i + 1..].iter().find(|c| c.kind == TokenKind::Punct && c.text == "}");
            if let Some(end) = end {
                let name = slice16(&units, t.from, end.to);
                if TYPED.is_match(&name) {
                    out.push(SqlParam { name, from: t.from, to: end.to, positional: false });
                    typed_end = end.to;
                    continue;
                }
            }
        }
        let prev = i.checked_sub(1).map(|p| &toks[p]);
        if t.kind == TokenKind::Param {
            // MySQL `@var` / `@@var` are session variables.
            if (driver == DbDriver::Mysql && t.text.starts_with('@')) || prev.is_some_and(|p| p.text == "@") {
                continue;
            }
            out.push(SqlParam { name: t.text.clone(), from: t.from, to: t.to, positional: false });
        } else if t.kind == TokenKind::Punct && t.text == "?" && driver != DbDriver::Postgres {
            // PostgreSQL's `?` is a jsonb operator; its parameters are `$1`.
            match toks.get(i + 1) {
                Some(n) if n.kind == TokenKind::Number && n.from == t.to => {
                    out.push(SqlParam { name: format!("?{}", n.text), from: t.from, to: n.to, positional: false });
                }
                _ => {
                    positional += 1;
                    out.push(SqlParam { name: format!("?{positional}"), from: t.from, to: t.to, positional: true });
                }
            }
        }
    }
    out
}

/// `text` with each parameter replaced by its value (empty: `NULL`).
pub fn bind_params(text: &str, params: &[SqlParam], values: &HashMap<String, String>) -> String {
    let units = utf16(text);
    let mut out = String::new();
    let mut at = 0;
    for p in params {
        out.push_str(&slice16(&units, at, p.from));
        match values.get(&p.name).map(|v| v.trim()).filter(|v| !v.is_empty()) {
            Some(v) => out.push_str(v),
            None => out.push_str("NULL"),
        }
        at = p.to;
    }
    out.push_str(&slice16(&units, at, units.len()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|&(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn binds_whole_clickhouse_typed_parameters_without_treating_their_types_as_names() {
        let sql = "select {value:UInt64}, {label: Nullable(String)}, '{skip:String}'";
        let params = find_params(sql, DbDriver::Clickhouse, 0);
        let names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["{value:UInt64}", "{label: Nullable(String)}"]);
        let bound =
            bind_params(sql, &params, &values(&[("{value:UInt64}", "12"), ("{label: Nullable(String)}", "'test'")]));
        assert_eq!(bound, "select 12, 'test', '{skip:String}'");
    }

    #[test]
    fn finds_parameters_per_dialect_and_binds_values() {
        let names = |sql: &str, driver, n| find_params(sql, driver, n).into_iter().map(|p| p.name).collect::<Vec<_>>();
        assert_eq!(
            names(
                "select * from t where a = :id and b = ? and c = ?3 and d = @x and e = '?' -- ?",
                DbDriver::Sqlite,
                0
            ),
            [":id", "?1", "?3", "@x"]
        );
        assert_eq!(names("select * from t where a = ? and b = ?", DbDriver::Sqlite, 2), ["?3", "?4"]);
        assert_eq!(names("select @x := 1, @@version, ? from t", DbDriver::Mysql, 0), ["?1"]);
        assert_eq!(names("select j ? 'k', $1, a::int from t where b = :name", DbDriver::Postgres, 0), ["$1", ":name"]);
        let sql = "update t set a = :v where id = :id and b = :v";
        let bound = bind_params(sql, &find_params(sql, DbDriver::Sqlite, 0), &values(&[(":v", "'x'"), (":id", " 3 ")]));
        assert_eq!(bound, "update t set a = 'x' where id = 3 and b = 'x'");
        assert_eq!(
            bind_params("select ?", &find_params("select ?", DbDriver::Sqlite, 0), &HashMap::new()),
            "select NULL"
        );
    }
}
