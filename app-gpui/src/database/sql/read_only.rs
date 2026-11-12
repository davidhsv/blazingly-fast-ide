//! Whether a statement only reads: paging a result re-runs its query, which must not write again.

use super::lexer::{TokenKind, lex};
use ide_db::DbDriver;

/// Statements that only read.
const READ_VERBS: [&str; 9] = ["SELECT", "WITH", "VALUES", "TABLE", "SHOW", "DESCRIBE", "DESC", "EXPLAIN", "PRAGMA"];
/// Words that make a reading statement write (data-modifying CTEs, SELECT INTO, row locks, sequences).
const WRITES: [&str; 10] =
    ["INSERT", "UPDATE", "DELETE", "MERGE", "INTO", "NEXTVAL", "SETVAL", "CALL", "EXEC", "EXECUTE"];

/// Whether running `text` again only reads.
pub fn is_read_only_query(text: &str, driver: DbDriver) -> bool {
    let tokens = lex(text, driver).tokens;
    let Some(first) = tokens.iter().find(|t| t.text != "(") else { return false };
    if first.kind != TokenKind::Word || !READ_VERBS.contains(&first.upper.as_str()) {
        return false;
    }
    !tokens.iter().any(|t| t.kind == TokenKind::Word && WRITES.contains(&t.upper.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_queries_that_only_read() {
        for text in [
            "SELECT * FROM t",
            "with x as (select 1) select * from x",
            "VALUES (1), (2)",
            "show tables",
            "(select 1) union (select 2)",
        ] {
            assert!(is_read_only_query(text, DbDriver::Postgres), "{text}");
        }
    }

    #[test]
    fn rejects_statements_that_change_data() {
        for text in [
            "DELETE FROM t RETURNING *",
            "UPDATE t SET a = 1 RETURNING a",
            "INSERT INTO t VALUES (1) RETURNING id",
            "with d as (delete from t returning *) select * from d",
            "select * into t2 from t",
            "select * from t for update",
            "select nextval('s')",
        ] {
            assert!(!is_read_only_query(text, DbDriver::Postgres), "{text}");
        }
    }
}
