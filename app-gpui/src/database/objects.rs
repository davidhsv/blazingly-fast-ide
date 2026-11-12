//! SQL that the Database tree's object actions run or paste (Rename, Drop, Truncate, SQL Scripts ›
//! …, Modify Table, Copy Reference), per dialect.

use ide_db::{ColumnInfo, DbDriver};
use std::collections::HashSet;
use std::sync::LazyLock;

/// `name` always quoted, in the dialect's quotes.
pub fn quote_ident(driver: DbDriver, name: &str) -> String {
    match driver {
        DbDriver::Clickhouse => format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\"")),
        DbDriver::Mysql => format!("`{}`", name.replace('`', "``")),
        DbDriver::SqlServer => format!("[{}]", name.replace(']', "]]")),
        DbDriver::Sqlite | DbDriver::Postgres | DbDriver::Oracle => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

static RESERVED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "ALL AND AS ASC BETWEEN BY CASE CHECK COLUMN CONSTRAINT CREATE CROSS DEFAULT DELETE DESC DISTINCT DROP \
     ELSE END EXISTS FOREIGN FROM FULL GROUP HAVING IN INDEX INNER INSERT INTO IS JOIN KEY LEFT LIKE LIMIT NOT \
     NULL OFFSET ON OR ORDER OUTER PRIMARY REFERENCES RIGHT SELECT SET TABLE THEN TO UNION UNIQUE UPDATE USER \
     USING VALUES VIEW WHEN WHERE WITH"
        .split_whitespace()
        .collect()
});

fn plain(driver: DbDriver, name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    match driver {
        DbDriver::Postgres => {
            (first == '_' || first.is_ascii_lowercase())
                && chars.all(|c| c == '_' || c == '$' || c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        // Oracle folds unquoted names to upper case.
        DbDriver::Oracle => {
            (first == '_' || first.is_ascii_uppercase())
                && chars.all(|c| c == '_' || c == '$' || c.is_ascii_uppercase() || c.is_ascii_digit())
        }
        DbDriver::Clickhouse => {
            (first == '_' || first.is_ascii_alphabetic()) && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
        }
        DbDriver::Sqlite | DbDriver::Mysql | DbDriver::SqlServer => {
            (first == '_' || first.is_ascii_alphabetic())
                && chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
        }
    }
}

/// Quotes only when needed: lower-case (PostgreSQL) / UPPER-case (Oracle) / plain words stay bare.
pub fn ident(driver: DbDriver, name: &str) -> String {
    if plain(driver, name) && !RESERVED.contains(name.to_uppercase().as_str()) {
        name.to_string()
    } else {
        quote_ident(driver, name)
    }
}

/// `schema.name`; SQLite's `main` and an empty schema are left out.
pub fn qualified_name(driver: DbDriver, schema: &str, name: &str) -> String {
    if schema.is_empty() || (driver == DbDriver::Sqlite && schema == "main") {
        ident(driver, name)
    } else {
        format!("{}.{}", ident(driver, schema), ident(driver, name))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Table,
    View,
    MaterializedView,
    Column,
    Index,
    Schema,
    Function,
    Procedure,
    Trigger,
    Sequence,
}

impl ObjectKind {
    /// The kind as SQL names it (`drop materialized view`).
    fn sql(self) -> &'static str {
        match self {
            ObjectKind::Table => "table",
            ObjectKind::View => "view",
            ObjectKind::MaterializedView => "materialized view",
            ObjectKind::Column => "column",
            ObjectKind::Index => "index",
            ObjectKind::Schema => "schema",
            ObjectKind::Function => "function",
            ObjectKind::Procedure => "procedure",
            ObjectKind::Trigger => "trigger",
            ObjectKind::Sequence => "sequence",
        }
    }
}

/// A schema object of a data source, as the tree actions see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRef {
    pub kind: ObjectKind,
    pub schema: String,
    /// Table / view / index / schema / routine / trigger / sequence name; the column's table for a column.
    pub name: String,
    /// Column name (kind `Column`).
    pub column: Option<String>,
    /// Table of an index or trigger (MySQL drops an index `ON` its table, PostgreSQL a trigger).
    pub table: Option<String>,
    /// Argument types of a PostgreSQL routine (tells overloads apart).
    pub arguments: Option<String>,
}

impl ObjectRef {
    pub fn new(kind: ObjectKind, schema: &str, name: &str) -> Self {
        Self { kind, schema: schema.into(), name: name.into(), column: None, table: None, arguments: None }
    }
}

/// Copy Reference: the object's qualified name.
pub fn reference(driver: DbDriver, o: &ObjectRef) -> String {
    if o.kind == ObjectKind::Schema {
        return ident(driver, &o.name);
    }
    let q = qualified_name(driver, &o.schema, &o.name);
    match &o.column {
        Some(column) if o.kind == ObjectKind::Column && !column.is_empty() => format!("{q}.{}", ident(driver, column)),
        _ => q,
    }
}

pub fn can_rename(driver: DbDriver, o: &ObjectRef) -> bool {
    match o.kind {
        ObjectKind::Table | ObjectKind::Column => true,
        ObjectKind::View => driver != DbDriver::Sqlite,
        ObjectKind::Index => driver == DbDriver::Postgres,
        _ => false,
    }
}

/// The statement renaming `o` to `to`.
pub fn rename_sql(driver: DbDriver, o: &ObjectRef, to: &str) -> String {
    let q = qualified_name(driver, &o.schema, &o.name);
    let to_ident = ident(driver, to);
    match o.kind {
        ObjectKind::Column => {
            format!("alter table {q} rename column {} to {to_ident};", ident(driver, o.column.as_deref().unwrap_or("")))
        }
        ObjectKind::Index => format!("alter index {q} rename to {to_ident};"),
        ObjectKind::View if driver == DbDriver::Postgres => format!("alter view {q} rename to {to_ident};"),
        ObjectKind::View if driver == DbDriver::Oracle => format!("rename {q} to {to_ident};"),
        ObjectKind::View => format!("rename table {q} to {};", qualified_name(driver, &o.schema, to)),
        _ if driver == DbDriver::Mysql || driver == DbDriver::Clickhouse => {
            format!("rename table {q} to {};", qualified_name(driver, &o.schema, to))
        }
        _ => format!("alter table {q} rename to {to_ident};"),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropOptions {
    pub if_exists: bool,
    /// PostgreSQL `CASCADE`.
    pub cascade: bool,
}

pub fn can_drop(driver: DbDriver, o: &ObjectRef) -> bool {
    if driver == DbDriver::Clickhouse {
        return matches!(
            o.kind,
            ObjectKind::Table
                | ObjectKind::View
                | ObjectKind::MaterializedView
                | ObjectKind::Column
                | ObjectKind::Schema
        );
    }
    !(o.kind == ObjectKind::Schema && driver == DbDriver::Sqlite)
}

pub fn can_modify_table(driver: DbDriver) -> bool {
    driver != DbDriver::Clickhouse
}

/// The Confirm Drop preview.
pub fn drop_sql(driver: DbDriver, o: &ObjectRef, opts: DropOptions) -> String {
    let if_exists = if opts.if_exists { "if exists " } else { "" };
    let cascade = if opts.cascade && driver == DbDriver::Postgres { " cascade" } else { "" };
    let q = qualified_name(driver, &o.schema, &o.name);
    let table = o.table.as_deref().unwrap_or("");
    match o.kind {
        ObjectKind::Column => {
            let guard = if driver == DbDriver::Postgres { if_exists } else { "" };
            let column = ident(driver, o.column.as_deref().unwrap_or(""));
            format!("alter table {q} drop column {guard}{column}{cascade};")
        }
        ObjectKind::Index if driver == DbDriver::Mysql => {
            format!("drop index {} on {};", ident(driver, &o.name), qualified_name(driver, &o.schema, table))
        }
        // Oracle's DROP INDEX takes no IF EXISTS.
        ObjectKind::Index if driver == DbDriver::Oracle => format!("drop index {q};"),
        ObjectKind::Index => format!("drop index {if_exists}{q}{cascade};"),
        ObjectKind::Schema if driver == DbDriver::Oracle => format!("drop user {} cascade;", ident(driver, &o.name)),
        ObjectKind::Schema => {
            let word = if driver == DbDriver::Mysql || driver == DbDriver::Clickhouse { "database" } else { "schema" };
            format!("drop {word} {if_exists}{}{cascade};", ident(driver, &o.name))
        }
        ObjectKind::MaterializedView if driver == DbDriver::Clickhouse => format!("drop table {if_exists}{q};"),
        ObjectKind::Function | ObjectKind::Procedure => {
            let args = if driver == DbDriver::Postgres {
                format!("({})", o.arguments.as_deref().unwrap_or(""))
            } else {
                String::new()
            };
            format!("drop {} {if_exists}{q}{args}{cascade};", o.kind.sql())
        }
        ObjectKind::Trigger if driver == DbDriver::Postgres => format!(
            "drop trigger {if_exists}{} on {}{cascade};",
            ident(driver, &o.name),
            qualified_name(driver, &o.schema, table)
        ),
        ObjectKind::Trigger => format!("drop trigger {if_exists}{q};"),
        _ => format!("drop {} {if_exists}{q}{cascade};", o.kind.sql()),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TruncateOptions {
    pub restart_identity: bool,
    pub cascade: bool,
}

/// Object Actions › Truncate (SQLite has no TRUNCATE: `delete from`).
pub fn truncate_sql(driver: DbDriver, schema: &str, table: &str, opts: TruncateOptions) -> String {
    let q = qualified_name(driver, schema, table);
    match driver {
        DbDriver::Sqlite => format!("delete from {q};"),
        DbDriver::Mysql | DbDriver::SqlServer | DbDriver::Clickhouse | DbDriver::Oracle => {
            format!("truncate table {q};")
        }
        DbDriver::Postgres => format!(
            "truncate table {q}{}{};",
            if opts.restart_identity { " restart identity" } else { "" },
            if opts.cascade { " cascade" } else { "" }
        ),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptKind {
    SelectAll,
    SelectFirst,
    Insert,
    Update,
    Delete,
}

impl ScriptKind {
    /// The SQL Scripts submenu label (English text key).
    pub fn label(self) -> &'static str {
        match self {
            ScriptKind::SelectAll => "Select all rows from a table",
            ScriptKind::SelectFirst => "Select first N rows from a table",
            ScriptKind::Insert => "Insert rows into a table",
            ScriptKind::Update => "Update values in a table",
            ScriptKind::Delete => "Delete rows from a table",
        }
    }
}

/// Why a script template cannot be written.
pub const CLICKHOUSE_KEY_UPDATE: &str = "ClickHouse sorting key columns cannot be updated.";

/// SQL Scripts › … templates (the `sel`, `selc`, `ins`, `upd`, `del` live templates). A ClickHouse
/// update of a table whose columns are all key columns is refused with [`CLICKHOUSE_KEY_UPDATE`].
pub fn script_sql(
    driver: DbDriver,
    kind: ScriptKind,
    schema: &str,
    table: &str,
    columns: &[ColumnInfo],
) -> Result<String, &'static str> {
    let q = qualified_name(driver, schema, table);
    let names: Vec<String> = columns.iter().map(|c| ident(driver, &c.name)).collect();
    let keys: Vec<String> = columns.iter().filter(|c| c.primary_key).map(|c| ident(driver, &c.name)).collect();
    let by = if keys.is_empty() { names.iter().take(1).cloned().collect() } else { keys };
    let condition = by.iter().map(|k| format!("{k} = ?")).collect::<Vec<_>>().join(" and ");
    let condition = if condition.is_empty() { "1 = 0".to_string() } else { condition };
    Ok(match kind {
        ScriptKind::SelectAll => format!("select *\nfrom {q};\n"),
        ScriptKind::SelectFirst if driver == DbDriver::Oracle => {
            format!("select *\nfrom {q}\nfetch first 10 rows only;\n")
        }
        ScriptKind::SelectFirst => format!("select *\nfrom {q}\nlimit 10;\n"),
        ScriptKind::Insert => {
            format!("insert into {q} ({})\nvalues ({});\n", names.join(", "), vec!["?"; names.len()].join(", "))
        }
        ScriptKind::Update => {
            let set: Vec<String> =
                columns.iter().filter(|c| !c.primary_key).map(|c| format!("{} = ?", ident(driver, &c.name))).collect();
            if driver == DbDriver::Clickhouse {
                if set.is_empty() {
                    return Err(CLICKHOUSE_KEY_UPDATE);
                }
                return Ok(format!("alter table {q}\nupdate {}\nwhere 1 = 0;\n", set.join(",\n       ")));
            }
            let set = if set.is_empty() { names.iter().map(|n| format!("{n} = ?")).collect() } else { set };
            format!("update {q}\nset {}\nwhere {condition};\n", set.join(",\n    "))
        }
        ScriptKind::Delete if driver == DbDriver::Clickhouse => format!("alter table {q}\ndelete\nwhere 1 = 0;\n"),
        ScriptKind::Delete => format!("delete\nfrom {q}\nwhere {condition};\n"),
    })
}

/// A column row of the Modify Table dialog; `original` is the column it started as (`None`: new).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDraft {
    pub key: u64,
    pub original: Option<ColumnInfo>,
    pub name: String,
    pub data_type: String,
    pub not_null: bool,
    /// SQL expression; empty for none.
    pub default: String,
    pub primary_key: bool,
    pub dropped: bool,
}

pub fn draft_of(c: &ColumnInfo, key: u64) -> ColumnDraft {
    ColumnDraft {
        key,
        original: Some(c.clone()),
        name: c.name.clone(),
        data_type: c.data_type.clone(),
        not_null: !c.nullable,
        default: c.default.clone().unwrap_or_default(),
        primary_key: c.primary_key,
        dropped: false,
    }
}

fn column_definition(driver: DbDriver, c: &ColumnDraft) -> String {
    let fallback = match driver {
        DbDriver::Clickhouse => "String",
        DbDriver::Sqlite => "TEXT",
        DbDriver::Oracle => "VARCHAR2(255)",
        DbDriver::Postgres | DbDriver::Mysql | DbDriver::SqlServer => "text",
    };
    let data_type = c.data_type.trim();
    let mut s = format!("{} {}", ident(driver, &c.name), if data_type.is_empty() { fallback } else { data_type });
    if !c.default.trim().is_empty() {
        s.push_str(&format!(" default {}", c.default.trim()));
    }
    if c.not_null && driver != DbDriver::Clickhouse {
        s.push_str(" not null");
    }
    s
}

/// `create table` for New › Table.
pub fn create_table_sql(driver: DbDriver, schema: &str, name: &str, columns: &[ColumnDraft]) -> String {
    let live: Vec<&ColumnDraft> = columns.iter().filter(|c| !c.dropped && !c.name.trim().is_empty()).collect();
    let keys: Vec<String> = live.iter().filter(|c| c.primary_key).map(|c| ident(driver, &c.name)).collect();
    let mut items: Vec<String> = live.iter().map(|c| format!("    {}", column_definition(driver, c))).collect();
    if !keys.is_empty() && driver != DbDriver::Clickhouse {
        items.push(format!("    primary key ({})", keys.join(", ")));
    }
    let engine = if driver == DbDriver::Clickhouse { "\nengine = MergeTree\norder by tuple()" } else { "" };
    format!("create table {}\n(\n{}\n){engine};\n", qualified_name(driver, schema, name), items.join(",\n"))
}

/// Whether [`alter_table_sql`] can express changing column `c` (SQLite only renames, adds and drops).
pub fn can_alter_column(driver: DbDriver, c: &ColumnDraft) -> bool {
    driver != DbDriver::Sqlite || c.original.is_none()
}

/// The statements turning the table `table` into `name` with `columns` (Modify Table): renames,
/// added / dropped columns, and — PostgreSQL / MySQL / Oracle — type, nullability and default changes.
pub fn alter_table_sql(
    driver: DbDriver,
    schema: &str,
    table: &str,
    name: &str,
    columns: &[ColumnDraft],
) -> Vec<String> {
    if !can_modify_table(driver) {
        return Vec::new();
    }
    let mut out = Vec::new();
    let q = qualified_name(driver, schema, table);
    for c in columns {
        let Some(o) = &c.original else {
            if !c.dropped && !c.name.trim().is_empty() {
                // Oracle takes the added column's definition in parentheses.
                out.push(if driver == DbDriver::Oracle {
                    format!("alter table {q} add ({});", column_definition(driver, c))
                } else {
                    format!("alter table {q} add column {};", column_definition(driver, c))
                });
            }
            continue;
        };
        if c.dropped {
            out.push(format!("alter table {q} drop column {};", ident(driver, &o.name)));
            continue;
        }
        let data_type = c.data_type.trim();
        let default = c.default.trim();
        let old_default = o.default.as_deref().unwrap_or("").trim();
        let renamed = !c.name.trim().is_empty() && c.name != o.name;
        let nullability_changed = c.not_null == o.nullable;
        let changed = data_type != o.data_type || nullability_changed || default != old_default;
        if driver == DbDriver::Mysql && (renamed || changed) {
            out.push(format!(
                "alter table {q} change column {} {};",
                ident(driver, &o.name),
                column_definition(driver, c)
            ));
            continue;
        }
        if driver == DbDriver::Oracle {
            let col = ident(driver, &o.name);
            if !data_type.is_empty() && data_type != o.data_type {
                let not_null = if c.not_null { " not null" } else { "" };
                out.push(format!("alter table {q} modify ({col} {data_type}{not_null});"));
            }
            if nullability_changed {
                out.push(format!("alter table {q} modify ({col} {});", if c.not_null { "not null" } else { "null" }));
            }
            if default != old_default {
                let value = if default.is_empty() { "null" } else { default };
                out.push(format!("alter table {q} modify ({col} default {value});"));
            }
            if renamed {
                out.push(format!("alter table {q} rename column {col} to {};", ident(driver, &c.name)));
            }
            continue;
        }
        if renamed {
            out.push(format!(
                "alter table {q} rename column {} to {};",
                ident(driver, &o.name),
                ident(driver, &c.name)
            ));
        }
        if driver != DbDriver::Postgres {
            continue;
        }
        let col = ident(driver, if renamed { &c.name } else { &o.name });
        if !data_type.is_empty() && data_type != o.data_type {
            out.push(format!("alter table {q} alter column {col} type {data_type};"));
        }
        if nullability_changed {
            out.push(format!(
                "alter table {q} alter column {col} {} not null;",
                if c.not_null { "set" } else { "drop" }
            ));
        }
        if default != old_default {
            let action = if default.is_empty() { "drop default".to_string() } else { format!("set default {default}") };
            out.push(format!("alter table {q} alter column {col} {action};"));
        }
    }
    // Renamed last: the column statements above name the table as it was.
    let new_name = name.trim();
    if !new_name.is_empty() && new_name != table {
        out.push(rename_sql(driver, &ObjectRef::new(ObjectKind::Table, schema, table), new_name));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, data_type: &str, primary_key: bool, nullable: bool) -> ColumnInfo {
        ColumnInfo { name: name.into(), data_type: data_type.into(), nullable, default: None, primary_key }
    }

    fn users() -> Vec<ColumnInfo> {
        vec![col("id", "INTEGER", true, false), col("email", "TEXT", false, false), col("name", "TEXT", false, true)]
    }

    fn drafts(columns: &[ColumnInfo]) -> Vec<ColumnDraft> {
        columns.iter().enumerate().map(|(i, c)| draft_of(c, i as u64)).collect()
    }

    fn opts(if_exists: bool, cascade: bool) -> DropOptions {
        DropOptions { if_exists, cascade }
    }

    fn truncate(restart_identity: bool, cascade: bool) -> TruncateOptions {
        TruncateOptions { restart_identity, cascade }
    }

    fn with(kind: ObjectKind, schema: &str, name: &str, f: impl FnOnce(&mut ObjectRef)) -> ObjectRef {
        let mut o = ObjectRef::new(kind, schema, name);
        f(&mut o);
        o
    }

    #[test]
    fn uses_clickhouse_object_syntax_and_escapes_identifiers() {
        let ch = DbDriver::Clickhouse;
        assert_eq!(quote_ident(ch, "a\\b\"c"), "\"a\\\\b\\\"c\"");
        assert_eq!(ident(ch, "price$usd"), "\"price$usd\"");
        assert_eq!(
            rename_sql(ch, &ObjectRef::new(ObjectKind::Table, "default", "users"), "people"),
            "rename table \"default\".users to \"default\".people;"
        );
        assert_eq!(
            drop_sql(ch, &ObjectRef::new(ObjectKind::Schema, "", "analytics"), opts(true, true)),
            "drop database if exists analytics;"
        );
        assert_eq!(
            drop_sql(ch, &ObjectRef::new(ObjectKind::MaterializedView, "default", "totals"), opts(true, true)),
            "drop table if exists \"default\".totals;"
        );
        assert_eq!(truncate_sql(ch, "default", "users", truncate(true, true)), "truncate table \"default\".users;");
        assert!(!can_drop(ch, &ObjectRef::new(ObjectKind::Index, "default", "ix")));
        assert!(!can_modify_table(ch));
        assert!(alter_table_sql(ch, "default", "users", "people", &drafts(&users())).is_empty());
    }

    #[test]
    fn creates_clickhouse_mergetree_tables_without_a_relational_primary_key() {
        let d = drafts(&[col("id", "UInt64", true, false), col("name", "Nullable(String)", false, true)]);
        assert_eq!(
            create_table_sql(DbDriver::Clickhouse, "default", "people", &d),
            "create table \"default\".people\n(\n    id UInt64,\n    name Nullable(String)\n)\nengine = MergeTree\norder by tuple();\n"
        );
    }

    #[test]
    fn writes_guarded_clickhouse_mutation_templates_without_unique_key_assumptions() {
        let ch = DbDriver::Clickhouse;
        assert_eq!(
            script_sql(ch, ScriptKind::Update, "default", "users", &users()),
            Ok("alter table \"default\".users\nupdate email = ?,\n       name = ?\nwhere 1 = 0;\n".to_string())
        );
        assert_eq!(
            script_sql(ch, ScriptKind::Delete, "default", "users", &users()),
            Ok("alter table \"default\".users\ndelete\nwhere 1 = 0;\n".to_string())
        );
        assert_eq!(script_sql(ch, ScriptKind::Update, "default", "users", &users()[..1]), Err(CLICKHOUSE_KEY_UPDATE));
    }

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(ident(DbDriver::Postgres, "users"), "users");
        assert_eq!(ident(DbDriver::Postgres, "Users"), "\"Users\"");
        assert_eq!(ident(DbDriver::Sqlite, "order"), "\"order\"");
        assert_eq!(ident(DbDriver::Mysql, "my table"), "`my table`");
    }

    #[test]
    fn copies_references() {
        assert_eq!(reference(DbDriver::Sqlite, &ObjectRef::new(ObjectKind::Table, "main", "users")), "users");
        let column = with(ObjectKind::Column, "public", "users", |o| o.column = Some("email".into()));
        assert_eq!(reference(DbDriver::Postgres, &column), "public.users.email");
    }

    #[test]
    fn renames_per_dialect() {
        assert_eq!(
            rename_sql(DbDriver::Sqlite, &ObjectRef::new(ObjectKind::Table, "main", "users"), "people"),
            "alter table users rename to people;"
        );
        assert_eq!(
            rename_sql(DbDriver::Mysql, &ObjectRef::new(ObjectKind::Table, "app", "users"), "people"),
            "rename table app.users to app.people;"
        );
        let column = with(ObjectKind::Column, "public", "users", |o| o.column = Some("email".into()));
        assert_eq!(
            rename_sql(DbDriver::Postgres, &column, "mail"),
            "alter table public.users rename column email to mail;"
        );
    }

    #[test]
    fn drops_with_if_exists_cascade() {
        let t = ObjectRef::new(ObjectKind::Table, "public", "users");
        assert_eq!(drop_sql(DbDriver::Postgres, &t, opts(true, true)), "drop table if exists public.users cascade;");
        let main = ObjectRef { schema: "main".into(), ..t };
        assert_eq!(drop_sql(DbDriver::Sqlite, &main, opts(false, true)), "drop table users;");
        let index = with(ObjectKind::Index, "app", "ix", |o| o.table = Some("users".into()));
        assert_eq!(drop_sql(DbDriver::Mysql, &index, opts(false, false)), "drop index ix on app.users;");
        let column = with(ObjectKind::Column, "main", "users", |o| o.column = Some("name".into()));
        assert_eq!(drop_sql(DbDriver::Sqlite, &column, opts(true, false)), "alter table users drop column name;");
    }

    #[test]
    fn truncates_sqlite_deletes() {
        assert_eq!(truncate_sql(DbDriver::Sqlite, "main", "users", truncate(true, true)), "delete from users;");
        assert_eq!(
            truncate_sql(DbDriver::Postgres, "public", "users", truncate(true, false)),
            "truncate table public.users restart identity;"
        );
    }

    #[test]
    fn writes_script_templates_keyed_by_the_primary_key() {
        let s = |kind| script_sql(DbDriver::Sqlite, kind, "main", "users", &users());
        assert_eq!(s(ScriptKind::SelectFirst), Ok("select *\nfrom users\nlimit 10;\n".to_string()));
        assert_eq!(s(ScriptKind::Insert), Ok("insert into users (id, email, name)\nvalues (?, ?, ?);\n".to_string()));
        assert_eq!(
            s(ScriptKind::Update),
            Ok("update users\nset email = ?,\n    name = ?\nwhere id = ?;\n".to_string())
        );
        assert_eq!(s(ScriptKind::Delete), Ok("delete\nfrom users\nwhere id = ?;\n".to_string()));
    }

    #[test]
    fn creates_tables() {
        assert_eq!(
            create_table_sql(DbDriver::Sqlite, "main", "people", &drafts(&users())),
            "create table people\n(\n    id INTEGER not null,\n    email TEXT not null,\n    name TEXT,\n    primary key (id)\n);\n"
        );
    }

    #[test]
    fn alters_tables_sqlite_renames_adds_drops_postgresql_also_changes_columns() {
        let mut d = drafts(&users());
        d[1].name = "mail".into();
        d[2].dropped = true;
        d.push(ColumnDraft {
            key: 9,
            original: None,
            name: "age".into(),
            data_type: "INTEGER".into(),
            not_null: false,
            default: "0".into(),
            primary_key: false,
            dropped: false,
        });
        assert_eq!(
            alter_table_sql(DbDriver::Sqlite, "main", "users", "people", &d),
            [
                "alter table users rename column email to mail;",
                "alter table users drop column name;",
                "alter table users add column age INTEGER default 0;",
                "alter table users rename to people;",
            ]
        );
        let mut p = drafts(&users());
        p[2].data_type = "varchar(40)".into();
        p[2].not_null = true;
        p[2].default = "''".into();
        assert_eq!(
            alter_table_sql(DbDriver::Postgres, "public", "users", "users", &p),
            [
                "alter table public.users alter column name type varchar(40);",
                "alter table public.users alter column name set not null;",
                "alter table public.users alter column name set default '';",
            ]
        );
        assert_eq!(
            alter_table_sql(DbDriver::Mysql, "app", "users", "users", &p),
            ["alter table app.users change column name name varchar(40) default '' not null;"]
        );
    }

    fn emp() -> Vec<ColumnInfo> {
        vec![col("ID", "NUMBER", true, false), col("NAME", "VARCHAR2(40)", false, true)]
    }

    #[test]
    fn keeps_unquoted_names_that_the_server_folds_to_upper_case() {
        assert_eq!(ident(DbDriver::Oracle, "EMPLOYEES"), "EMPLOYEES");
        assert_eq!(ident(DbDriver::Oracle, "employees"), "\"employees\"");
        assert_eq!(reference(DbDriver::Oracle, &ObjectRef::new(ObjectKind::Table, "HR", "EMPLOYEES")), "HR.EMPLOYEES");
    }

    #[test]
    fn generates_oracle_ddl_and_dml() {
        let ora = DbDriver::Oracle;
        assert_eq!(truncate_sql(ora, "HR", "EMPLOYEES", truncate(true, true)), "truncate table HR.EMPLOYEES;");
        let first = script_sql(ora, ScriptKind::SelectFirst, "HR", "EMPLOYEES", &emp()).expect("a script");
        assert!(first.contains("fetch first 10 rows only;"));
        assert!(!first.contains("limit"));
        assert_eq!(
            drop_sql(ora, &ObjectRef::new(ObjectKind::Index, "HR", "E_IDX"), opts(true, false)),
            "drop index HR.E_IDX;"
        );
        assert_eq!(
            drop_sql(ora, &ObjectRef::new(ObjectKind::Schema, "", "APP"), opts(false, false)),
            "drop user APP cascade;"
        );
        assert_eq!(
            rename_sql(ora, &ObjectRef::new(ObjectKind::View, "HR", "V_EMP"), "V_STAFF"),
            "rename HR.V_EMP to V_STAFF;"
        );
        assert!(create_table_sql(ora, "HR", "T", &drafts(&emp())).contains("primary key (ID)"));
    }

    #[test]
    fn alters_columns_with_modify_and_parenthesised_add() {
        let mut p = drafts(&emp());
        p[1].data_type = "VARCHAR2(80)".into();
        p[1].not_null = true;
        p[1].name = "FULL_NAME".into();
        p.push(ColumnDraft {
            key: 99,
            original: None,
            name: "HIRED".into(),
            data_type: "DATE".into(),
            not_null: false,
            default: "SYSDATE".into(),
            primary_key: false,
            dropped: false,
        });
        assert_eq!(
            alter_table_sql(DbDriver::Oracle, "HR", "EMPLOYEES", "EMPLOYEES", &p),
            [
                "alter table HR.EMPLOYEES modify (NAME VARCHAR2(80) not null);",
                "alter table HR.EMPLOYEES modify (NAME not null);",
                "alter table HR.EMPLOYEES rename column NAME to FULL_NAME;",
                "alter table HR.EMPLOYEES add (HIRED DATE default SYSDATE);",
            ]
        );
    }

    #[test]
    fn drops_routines_triggers_sequences_and_materialized_views_per_dialect() {
        let o = opts(true, false);
        let f = with(ObjectKind::Function, "public", "f", |o| o.arguments = Some("a integer".into()));
        assert_eq!(drop_sql(DbDriver::Postgres, &f, o), "drop function if exists public.f(a integer);");
        assert_eq!(
            drop_sql(DbDriver::Mysql, &ObjectRef::new(ObjectKind::Procedure, "db", "p"), o),
            "drop procedure if exists db.p;"
        );
        let trigger = |schema: &str| with(ObjectKind::Trigger, schema, "t", |o| o.table = Some("x".into()));
        assert_eq!(drop_sql(DbDriver::Postgres, &trigger("s"), o), "drop trigger if exists t on s.x;");
        assert_eq!(drop_sql(DbDriver::Sqlite, &trigger("main"), o), "drop trigger if exists t;");
        assert_eq!(
            drop_sql(DbDriver::Postgres, &ObjectRef::new(ObjectKind::Sequence, "s", "q"), o),
            "drop sequence if exists s.q;"
        );
        assert_eq!(
            drop_sql(DbDriver::Postgres, &ObjectRef::new(ObjectKind::MaterializedView, "s", "m"), o),
            "drop materialized view if exists s.m;"
        );
    }
}
