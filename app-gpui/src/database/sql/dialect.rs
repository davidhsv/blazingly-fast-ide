//! Per-dialect SQL vocabulary: reserved words, statement starters, data types and built-in functions.

use ide_db::DbDriver;
use std::collections::HashSet;
use std::sync::LazyLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SqlFunction {
    pub name: &'static str,
    /// Parameter list shown in the popup, without parentheses.
    pub params: &'static str,
    pub returns: Option<&'static str>,
    pub aggregate: bool,
}

/// Words that must be quoted when used as identifiers (the common core of the dialects).
static RESERVED: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "ALL AND ANY AS ASC BETWEEN BY CASE CAST CHECK COLLATE COLUMN CONSTRAINT CREATE CROSS CURRENT_DATE \
     CURRENT_TIME CURRENT_TIMESTAMP DEFAULT DELETE DESC DISTINCT DROP ELSE END EXCEPT EXISTS FALSE FETCH FOR \
     FOREIGN FROM FULL GRANT GROUP HAVING IN INNER INSERT INTERSECT INTO IS JOIN LEFT LIKE LIMIT NATURAL NOT \
     NULL OFFSET ON OR ORDER OUTER PRIMARY REFERENCES RIGHT SELECT SET TABLE THEN TO TRUE UNION UNIQUE UPDATE \
     USING VALUES WHEN WHERE WINDOW WITH"
        .split_whitespace()
        .collect()
});

pub fn is_reserved(word: &str) -> bool {
    RESERVED.contains(word.to_uppercase().as_str())
}

/// Whether two names are the same object name: exact for ClickHouse, case-insensitive elsewhere.
/// An empty name matches nothing.
pub fn names_eq(driver: DbDriver, a: &str, b: &str) -> bool {
    !a.is_empty()
        && !b.is_empty()
        && if driver == DbDriver::Clickhouse { a == b } else { a.to_lowercase() == b.to_lowercase() }
}

/// [`names_eq`] for an optional name (an absent one matches nothing).
pub fn names_eq_opt(driver: DbDriver, a: Option<&str>, b: &str) -> bool {
    a.is_some_and(|a| names_eq(driver, a, b))
}

/// Clause keywords: a word in this set ends a table alias or expression.
pub static CLAUSE_WORDS: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    "SELECT FROM WHERE GROUP ORDER HAVING LIMIT OFFSET UNION INTERSECT EXCEPT JOIN INNER LEFT RIGHT FULL CROSS \
     NATURAL OUTER ON USING SET VALUES RETURNING INTO WINDOW FETCH FOR LATERAL AS WITH UPDATE DELETE INSERT \
     AND OR NOT WHEN THEN ELSE END CASE BY ASC DESC IS IN LIKE ILIKE BETWEEN EXISTS NULL STRAIGHT_JOIN"
        .split_whitespace()
        .collect()
});

pub fn is_clause_word(upper: &str) -> bool {
    CLAUSE_WORDS.contains(upper)
}

const COMMON_STARTERS: [&str; 16] = [
    "SELECT",
    "INSERT INTO",
    "UPDATE",
    "DELETE FROM",
    "WITH",
    "CREATE TABLE",
    "CREATE INDEX",
    "CREATE VIEW",
    "ALTER TABLE",
    "DROP TABLE",
    "DROP VIEW",
    "DROP INDEX",
    "EXPLAIN",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
];

pub fn statement_starters(driver: DbDriver) -> Vec<&'static str> {
    let extra: &[&str] = match driver {
        DbDriver::Clickhouse => {
            return vec![
                "SELECT",
                "INSERT INTO",
                "WITH",
                "CREATE TABLE",
                "CREATE VIEW",
                "CREATE MATERIALIZED VIEW",
                "CREATE DATABASE",
                "ALTER TABLE",
                "DROP TABLE",
                "DROP VIEW",
                "DROP DATABASE",
                "RENAME TABLE",
                "TRUNCATE TABLE",
                "EXPLAIN",
                "SHOW DATABASES",
                "SHOW TABLES",
                "SHOW CREATE TABLE",
                "DESCRIBE TABLE",
                "USE",
                "SET",
                "OPTIMIZE TABLE",
            ];
        }
        DbDriver::Sqlite => &["PRAGMA", "VACUUM", "ATTACH DATABASE", "REPLACE INTO", "CREATE TRIGGER"],
        DbDriver::Mysql => &[
            "SHOW TABLES",
            "SHOW CREATE TABLE",
            "DESCRIBE",
            "TRUNCATE TABLE",
            "REPLACE INTO",
            "USE",
            "CREATE DATABASE",
            "START TRANSACTION",
        ],
        DbDriver::Oracle => &[
            "TRUNCATE TABLE",
            "MERGE INTO",
            "CREATE SEQUENCE",
            "CREATE OR REPLACE PROCEDURE",
            "CREATE OR REPLACE FUNCTION",
            "CREATE OR REPLACE TRIGGER",
            "CREATE OR REPLACE VIEW",
            "ALTER SESSION SET",
            "GRANT",
            "REVOKE",
        ],
        DbDriver::Postgres | DbDriver::SqlServer => &[
            "TRUNCATE",
            "CREATE SCHEMA",
            "CREATE FUNCTION",
            "CREATE MATERIALIZED VIEW",
            "SET search_path TO",
            "COPY",
            "ANALYZE",
            "VACUUM",
        ],
    };
    COMMON_STARTERS.iter().chain(extra).copied().collect()
}

pub fn join_keywords(driver: DbDriver) -> Vec<&'static str> {
    let mut j = vec!["JOIN", "LEFT JOIN", "INNER JOIN", "CROSS JOIN", "RIGHT JOIN"];
    if driver != DbDriver::Mysql {
        j.push("FULL JOIN");
    }
    if driver == DbDriver::Postgres {
        j.extend(["JOIN LATERAL", "LEFT JOIN LATERAL"]);
    }
    if driver == DbDriver::Oracle {
        j.extend(["JOIN", "(+)"]);
    }
    j
}

/// Comparison / predicate keywords of WHERE, ON and HAVING.
pub fn predicate_keywords(driver: DbDriver) -> Vec<&'static str> {
    let mut k = vec![
        "AND",
        "OR",
        "NOT",
        "IN",
        "IS NULL",
        "IS NOT NULL",
        "LIKE",
        "BETWEEN",
        "EXISTS",
        "NULL",
        "TRUE",
        "FALSE",
        "CASE",
    ];
    match driver {
        DbDriver::Postgres => k.extend(["ILIKE", "IS DISTINCT FROM", "ANY", "SIMILAR TO"]),
        DbDriver::Mysql => k.push("REGEXP"),
        DbDriver::Sqlite => k.extend(["GLOB", "REGEXP"]),
        DbDriver::Oracle => k.extend(["CONNECT BY PRIOR", "START WITH", "NOT IN", "IS NOT NULL"]),
        DbDriver::SqlServer | DbDriver::Clickhouse => {}
    }
    k
}

pub fn data_types(driver: DbDriver) -> &'static [&'static str] {
    match driver {
        DbDriver::Sqlite => &["INTEGER", "TEXT", "REAL", "BLOB", "NUMERIC", "BOOLEAN", "DATETIME"],
        DbDriver::Postgres => &[
            "integer",
            "bigint",
            "smallint",
            "serial",
            "bigserial",
            "text",
            "varchar",
            "char",
            "boolean",
            "numeric",
            "real",
            "double precision",
            "date",
            "timestamp",
            "timestamptz",
            "time",
            "interval",
            "uuid",
            "json",
            "jsonb",
            "bytea",
        ],
        DbDriver::Mysql => &[
            "INT",
            "BIGINT",
            "SMALLINT",
            "TINYINT",
            "VARCHAR",
            "CHAR",
            "TEXT",
            "LONGTEXT",
            "DECIMAL",
            "DOUBLE",
            "FLOAT",
            "DATE",
            "DATETIME",
            "TIMESTAMP",
            "TIME",
            "JSON",
            "BLOB",
            "BOOLEAN",
            "ENUM",
        ],
        DbDriver::SqlServer => &[
            "INT",
            "BIGINT",
            "SMALLINT",
            "TINYINT",
            "VARCHAR",
            "NVARCHAR",
            "CHAR",
            "NCHAR",
            "TEXT",
            "NTEXT",
            "DECIMAL",
            "FLOAT",
            "REAL",
            "DATE",
            "DATETIME",
            "DATETIME2",
            "TIME",
            "UNIQUEIDENTIFIER",
            "BIT",
            "VARBINARY",
            "XML",
        ],
        DbDriver::Clickhouse => &[
            "UInt64",
            "UInt32",
            "UInt16",
            "UInt8",
            "Int64",
            "Int32",
            "Float64",
            "Float32",
            "Decimal",
            "String",
            "FixedString",
            "Bool",
            "Date",
            "Date32",
            "DateTime",
            "DateTime64",
            "UUID",
            "IPv4",
            "IPv6",
            "Array",
            "Tuple",
            "Map",
            "Nullable",
            "LowCardinality",
            "JSON",
        ],
        DbDriver::Oracle => &[
            "NUMBER",
            "NUMBER(10)",
            "NUMBER(19, 4)",
            "INTEGER",
            "BINARY_FLOAT",
            "BINARY_DOUBLE",
            "VARCHAR2(255)",
            "VARCHAR2(4000)",
            "NVARCHAR2(255)",
            "CHAR(1)",
            "CLOB",
            "NCLOB",
            "BLOB",
            "RAW(16)",
            "DATE",
            "TIMESTAMP",
            "TIMESTAMP WITH TIME ZONE",
            "TIMESTAMP WITH LOCAL TIME ZONE",
            "INTERVAL DAY TO SECOND",
            "INTERVAL YEAR TO MONTH",
            "ROWID",
            "XMLTYPE",
            "BOOLEAN",
        ],
    }
}

const fn f(name: &'static str, params: &'static str) -> SqlFunction {
    SqlFunction { name, params, returns: None, aggregate: false }
}

const fn fr(name: &'static str, params: &'static str, returns: &'static str) -> SqlFunction {
    SqlFunction { name, params, returns: Some(returns), aggregate: false }
}

const fn agg(name: &'static str, params: &'static str, returns: Option<&'static str>) -> SqlFunction {
    SqlFunction { name, params, returns, aggregate: true }
}

const COMMON: [SqlFunction; 16] = [
    agg("count", "*", Some("bigint")),
    agg("sum", "expr", None),
    agg("avg", "expr", None),
    agg("min", "expr", None),
    agg("max", "expr", None),
    f("coalesce", "value, ..."),
    f("nullif", "a, b"),
    fr("lower", "text", "text"),
    fr("upper", "text", "text"),
    fr("length", "text", "integer"),
    fr("trim", "text", "text"),
    fr("replace", "text, from, to", "text"),
    fr("substr", "text, start, length", "text"),
    f("abs", "number"),
    f("round", "number, digits"),
    f("cast", "expr AS type"),
];

fn with_common(extra: &[SqlFunction]) -> Vec<SqlFunction> {
    COMMON.iter().chain(extra).copied().collect()
}

static SQLITE: LazyLock<Vec<SqlFunction>> = LazyLock::new(|| {
    with_common(&[
        agg("group_concat", "expr, separator", Some("text")),
        f("ifnull", "a, b"),
        f("iif", "cond, then, else"),
        fr("instr", "text, search", "integer"),
        fr("printf", "format, ...", "text"),
        fr("date", "time, modifier...", "text"),
        fr("datetime", "time, modifier...", "text"),
        fr("strftime", "format, time, modifier...", "text"),
        fr("julianday", "time", "real"),
        f("json_extract", "json, path"),
        fr("json_object", "key, value, ...", "text"),
        fr("random", "", "integer"),
        fr("typeof", "expr", "text"),
        fr("last_insert_rowid", "", "integer"),
        agg("total", "expr", Some("real")),
    ])
});

static POSTGRES: LazyLock<Vec<SqlFunction>> = LazyLock::new(|| {
    with_common(&[
        agg("string_agg", "expr, delimiter", Some("text")),
        agg("array_agg", "expr", Some("anyarray")),
        agg("json_agg", "expr", Some("json")),
        agg("jsonb_agg", "expr", Some("jsonb")),
        agg("bool_and", "expr", Some("boolean")),
        agg("bool_or", "expr", Some("boolean")),
        fr("now", "", "timestamptz"),
        fr("current_date", "", "date"),
        fr("date_trunc", "field, source", "timestamp"),
        fr("extract", "field FROM source", "numeric"),
        fr("age", "timestamp, timestamp", "interval"),
        fr("to_char", "value, format", "text"),
        fr("to_timestamp", "text, format", "timestamptz"),
        fr("concat", "value, ...", "text"),
        fr("concat_ws", "separator, value, ...", "text"),
        fr("split_part", "text, delimiter, n", "text"),
        fr("regexp_replace", "text, pattern, replacement", "text"),
        fr("position", "substring IN text", "integer"),
        f("greatest", "value, ..."),
        f("least", "value, ..."),
        fr("gen_random_uuid", "", "uuid"),
        fr("jsonb_build_object", "key, value, ...", "jsonb"),
        fr("json_build_object", "key, value, ...", "json"),
        fr("unnest", "anyarray", "setof"),
        fr("generate_series", "start, stop, step", "setof"),
        fr("row_number", "", "bigint"),
        fr("rank", "", "bigint"),
        fr("dense_rank", "", "bigint"),
        f("lag", "value, offset, default"),
        f("lead", "value, offset, default"),
        fr("pg_size_pretty", "bigint", "text"),
    ])
});

static MYSQL: LazyLock<Vec<SqlFunction>> = LazyLock::new(|| {
    with_common(&[
        agg("group_concat", "expr SEPARATOR ','", Some("text")),
        f("ifnull", "a, b"),
        f("if", "cond, then, else"),
        fr("concat", "value, ...", "text"),
        fr("concat_ws", "separator, value, ...", "text"),
        fr("now", "", "datetime"),
        fr("curdate", "", "date"),
        fr("date_format", "date, format", "text"),
        fr("date_add", "date, INTERVAL n unit", "datetime"),
        fr("date_sub", "date, INTERVAL n unit", "datetime"),
        fr("datediff", "a, b", "int"),
        fr("unix_timestamp", "date", "bigint"),
        fr("from_unixtime", "seconds", "datetime"),
        f("json_extract", "json, path"),
        fr("json_object", "key, value, ...", "json"),
        agg("json_arrayagg", "expr", Some("json")),
        f("greatest", "value, ..."),
        f("least", "value, ..."),
        fr("uuid", "", "varchar"),
        fr("last_insert_id", "", "bigint"),
        fr("row_number", "", "bigint"),
        fr("rank", "", "bigint"),
    ])
});

static SQL_SERVER: LazyLock<Vec<SqlFunction>> = LazyLock::new(|| {
    with_common(&[
        fr("getdate", "", "datetime"),
        fr("getutcdate", "", "datetime"),
        fr("dateadd", "part, n, date", "datetime"),
        fr("datediff", "part, a, b", "int"),
        f("isnull", "a, b"),
        f("iif", "cond, then, else"),
        fr("concat", "value, ...", "text"),
        fr("format", "value, format", "text"),
        fr("newid", "", "uniqueidentifier"),
        fr("row_number", "", "bigint"),
        fr("rank", "", "bigint"),
    ])
});

const CLICKHOUSE: [SqlFunction; 30] = [
    agg("count", "*", Some("UInt64")),
    agg("sum", "expr", None),
    agg("avg", "expr", Some("Float64")),
    agg("min", "expr", None),
    agg("max", "expr", None),
    agg("uniq", "expr", Some("UInt64")),
    agg("uniqExact", "expr", Some("UInt64")),
    agg("groupArray", "expr", Some("Array")),
    agg("argMax", "arg, value", None),
    f("coalesce", "value, ..."),
    f("ifNull", "a, b"),
    f("nullIf", "a, b"),
    fr("lower", "text", "String"),
    fr("upper", "text", "String"),
    fr("length", "value", "UInt64"),
    fr("concat", "value, ...", "String"),
    fr("substring", "text, start, length", "String"),
    fr("replaceAll", "text, pattern, replacement", "String"),
    f("abs", "number"),
    f("round", "number, digits"),
    fr("toString", "expr", "String"),
    fr("toUInt64", "expr", "UInt64"),
    fr("toDate", "expr", "Date"),
    fr("toDateTime", "expr", "DateTime"),
    fr("toStartOfDay", "datetime", "DateTime"),
    fr("now", "", "DateTime"),
    fr("today", "", "Date"),
    f("arrayJoin", "array"),
    fr("tuple", "value, ...", "Tuple"),
    f("cast", "expr AS type"),
];

static ORACLE: LazyLock<Vec<SqlFunction>> = LazyLock::new(|| {
    with_common(&[
        f("nvl", "expr, replacement"),
        f("nvl2", "expr, value_if_not_null, value_if_null"),
        f("decode", "expr, search, result, ..., default"),
        f("greatest", "value, ..."),
        f("least", "value, ..."),
        fr("instr", "text, substring, position, occurrence", "number"),
        f("translate", "text, from, to"),
        f("lpad", "text, length, pad"),
        f("rpad", "text, length, pad"),
        f("regexp_replace", "text, pattern, replacement, position, occurrence"),
        fr("regexp_like", "text, pattern", "boolean"),
        fr("to_char", "value, format", "varchar2"),
        fr("to_date", "text, format", "date"),
        fr("to_number", "text, format", "number"),
        f("trunc", "date_or_number, format"),
        fr("add_months", "date, months", "date"),
        fr("months_between", "date, date", "number"),
        fr("last_day", "date", "date"),
        fr("next_day", "date, weekday", "date"),
        fr("sysdate", "", "date"),
        fr("systimestamp", "", "timestamp with time zone"),
        fr("extract", "field FROM source", "number"),
        agg("listagg", "expr, delimiter ON OVERFLOW TRUNCATE", Some("varchar2")),
        fr("row_number", "", "number"),
        fr("rank", "", "number"),
        fr("dense_rank", "", "number"),
        f("lag", "value, offset, default"),
        f("lead", "value, offset, default"),
        f("raise_application_error", "code, message"),
        fr("sys_context", "namespace, parameter", "varchar2"),
    ])
});

pub fn functions(driver: DbDriver) -> &'static [SqlFunction] {
    match driver {
        DbDriver::Sqlite => &SQLITE,
        DbDriver::Postgres => &POSTGRES,
        DbDriver::Mysql => &MYSQL,
        DbDriver::SqlServer => &SQL_SERVER,
        DbDriver::Clickhouse => &CLICKHOUSE,
        DbDriver::Oracle => &ORACLE,
    }
}
