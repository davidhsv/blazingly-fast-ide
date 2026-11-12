//! SQL language support of the query consoles: tokens, caret analysis, completion, name
//! resolution, query parameters, formatting and the read-only check. All offsets are UTF-16 code
//! units of the console text.

pub mod analyze;
pub mod complete;
pub mod dialect;
pub mod format;
pub mod lexer;
pub mod params;
pub mod read_only;
pub mod resolve;

#[cfg(test)]
pub(crate) mod fixtures {
    use super::analyze::{Analysis, analyze};
    use super::lexer::len16;
    use ide_db::{CompletionColumn, CompletionModel, CompletionTable, DbDriver, DbTableKind, ForeignKey};

    /// `sql` without its `|`, and the UTF-16 offset of the `|`.
    pub(crate) fn caret(sql: &str) -> (String, usize) {
        let byte = sql.find('|').expect("the text marks the caret");
        (sql.replacen('|', "", 1), len16(&sql[..byte]))
    }

    /// Analysis at the `|` of `sql`.
    pub(crate) fn at(sql: &str, driver: DbDriver) -> Analysis {
        let (text, pos) = caret(sql);
        analyze(&text, pos, driver)
    }

    pub(crate) fn column(name: &str, data_type: &str, primary_key: bool, has_default: bool) -> CompletionColumn {
        CompletionColumn {
            name: name.into(),
            data_type: data_type.into(),
            nullable: !primary_key,
            primary_key,
            has_default,
        }
    }

    pub(crate) fn col(name: &str) -> CompletionColumn {
        column(name, "INTEGER", false, false)
    }

    fn table(name: &str, kind: DbTableKind, columns: Vec<CompletionColumn>) -> CompletionTable {
        CompletionTable { schema: "main".into(), name: name.into(), kind, columns }
    }

    fn fk(table: &str, column: &str, ref_table: &str) -> ForeignKey {
        ForeignKey {
            schema: "main".into(),
            table: table.into(),
            columns: vec![column.into()],
            ref_schema: "main".into(),
            ref_table: ref_table.into(),
            ref_columns: vec!["id".into()],
        }
    }

    /// The shop schema the SQL specs complete and resolve against.
    pub(crate) fn model() -> CompletionModel {
        CompletionModel {
            default_schema: "main".into(),
            tables: vec![
                table(
                    "users",
                    DbTableKind::Table,
                    vec![
                        column("id", "INTEGER", true, false),
                        column("email", "TEXT", false, false),
                        column("name", "TEXT", false, false),
                        column("created_at", "TEXT", false, true),
                    ],
                ),
                table(
                    "orders",
                    DbTableKind::Table,
                    vec![
                        column("id", "INTEGER", true, false),
                        col("user_id"),
                        column("status", "TEXT", false, false),
                        column("total", "NUMERIC", false, false),
                    ],
                ),
                table(
                    "order_items",
                    DbTableKind::Table,
                    vec![
                        column("order_id", "INTEGER", true, false),
                        column("product_id", "INTEGER", true, false),
                        col("qty"),
                    ],
                ),
                table(
                    "products",
                    DbTableKind::Table,
                    vec![column("id", "INTEGER", true, false), column("title", "TEXT", false, false)],
                ),
                table(
                    "big_orders",
                    DbTableKind::View,
                    vec![col("id"), col("user_id"), column("total", "NUMERIC", false, false)],
                ),
            ],
            foreign_keys: vec![
                fk("orders", "user_id", "users"),
                fk("order_items", "order_id", "orders"),
                fk("order_items", "product_id", "products"),
            ],
        }
    }
}
