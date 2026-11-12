//! Result-set export formats (data extractors): CSV, TSV, JSON, SQL INSERT statements, a Markdown
//! table and One-Row (all values on one `, ` list). Values arrive as text; numeric columns are
//! written unquoted where the format distinguishes them.

use super::grid::is_numeric_column;
use super::model::{Cell, is_js_finite, js_number, js_number_string, to_csv, to_one_row, to_tsv};
use super::sql::complete::quote_ident;
use ide_db::DbDriver;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExportFormat {
    Csv,
    Tsv,
    Json,
    Sql,
    Markdown,
    OneRow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportFormatInfo {
    pub id: ExportFormat,
    /// English label; the dialog translates it.
    pub label: &'static str,
    pub extension: &'static str,
}

pub const EXPORT_FORMATS: [ExportFormatInfo; 6] = [
    ExportFormatInfo { id: ExportFormat::Csv, label: "CSV", extension: "csv" },
    ExportFormatInfo { id: ExportFormat::Tsv, label: "TSV", extension: "tsv" },
    ExportFormatInfo { id: ExportFormat::Json, label: "JSON", extension: "json" },
    ExportFormatInfo { id: ExportFormat::Sql, label: "SQL Inserts", extension: "sql" },
    ExportFormatInfo { id: ExportFormat::Markdown, label: "Markdown", extension: "md" },
    ExportFormatInfo { id: ExportFormat::OneRow, label: "One-Row", extension: "txt" },
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportColumn {
    pub name: String,
    pub type_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportData {
    pub columns: Vec<ExportColumn>,
    pub rows: Vec<Vec<Cell>>,
    /// Target of SQL inserts (the query's table, when it reads one).
    pub table: String,
    pub driver: DbDriver,
}

/// The Export Data dialog's options (CSV, TSV and Markdown; JSON and SQL keep their shape).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    pub column_header: bool,
    pub row_header: bool,
    pub transpose: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { column_header: true, row_header: false, transpose: false }
    }
}

/// Header and rows of a delimited export after the row header / transpose options.
fn laid_out(d: &ExportData, o: ExportOptions) -> (Option<Vec<String>>, Vec<Vec<Cell>>) {
    let mut head: Vec<String> = d.columns.iter().map(|c| c.name.clone()).collect();
    let mut rows: Vec<Vec<Cell>> = d.rows.clone();
    if o.row_header {
        head.insert(0, "#".into());
        for (i, r) in rows.iter_mut().enumerate() {
            r.insert(0, Some((i + 1).to_string()));
        }
    }
    if !o.transpose {
        return (o.column_header.then_some(head), rows);
    }
    let transposed = head
        .iter()
        .enumerate()
        .map(|(c, h)| {
            let mut line: Vec<Cell> = if o.column_header { vec![Some(h.clone())] } else { Vec::new() };
            line.extend(rows.iter().map(|r| r.get(c).cloned().flatten()));
            line
        })
        .collect();
    (None, transposed)
}

/// A JSON string literal.
fn json_string(s: &str) -> String {
    serde_json::Value::String(s.to_string()).to_string()
}

pub fn export_data(format: ExportFormat, d: &ExportData, options: ExportOptions) -> String {
    let names: Vec<&str> = d.columns.iter().map(|c| c.name.as_str()).collect();
    let numeric: Vec<bool> = d
        .columns
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let sample: Vec<Cell> = d.rows.iter().take(50).map(|r| r.get(i).cloned().flatten()).collect();
            is_numeric_column(&c.type_name, &sample)
        })
        .collect();
    let is_numeric = |i: usize| numeric.get(i).copied().unwrap_or(false);
    match format {
        ExportFormat::Csv => {
            let (head, rows) = laid_out(d, options);
            to_csv(head.as_deref(), &rows)
        }
        ExportFormat::Tsv => {
            let (head, rows) = laid_out(d, options);
            to_tsv(head.as_deref(), &rows)
        }
        ExportFormat::Json => {
            // Objects keep the column order; a repeated name keeps its first place and last value.
            let objects: Vec<String> = d
                .rows
                .iter()
                .map(|r| {
                    let mut fields: Vec<(&str, String)> = Vec::new();
                    for (i, n) in names.iter().enumerate() {
                        let value = match r.get(i).cloned().flatten() {
                            None => "null".to_string(),
                            Some(v) if is_numeric(i) && is_js_finite(&v) => {
                                js_number_string(js_number(&v).unwrap_or_default())
                            }
                            Some(v) => json_string(&v),
                        };
                        match fields.iter_mut().find(|(k, _)| k == n) {
                            Some(slot) => slot.1 = value,
                            None => fields.push((n, value)),
                        }
                    }
                    if fields.is_empty() {
                        return "  {}".to_string();
                    }
                    let body: Vec<String> =
                        fields.iter().map(|(k, v)| format!("    {}: {v}", json_string(k))).collect();
                    format!("  {{\n{}\n  }}", body.join(",\n"))
                })
                .collect();
            if objects.is_empty() { "[]".to_string() } else { format!("[\n{}\n]", objects.join(",\n")) }
        }
        ExportFormat::Sql => {
            let literal = |v: &Cell, i: usize| match v {
                None => "NULL".to_string(),
                Some(v) if is_numeric(i) && is_js_finite(v) => v.clone(),
                Some(v) => {
                    let escaped = if d.driver == DbDriver::Clickhouse { v.replace('\\', "\\\\") } else { v.clone() };
                    format!("'{}'", escaped.replace('\'', "''"))
                }
            };
            let columns: Vec<String> = names.iter().map(|n| quote_ident(n, d.driver)).collect();
            let head = format!("INSERT INTO {} ({}) VALUES", quote_ident(&d.table, d.driver), columns.join(", "));
            d.rows
                .iter()
                .map(|r| {
                    let values: Vec<String> = r.iter().enumerate().map(|(i, v)| literal(v, i)).collect();
                    format!("{head} ({});", values.join(", "))
                })
                .collect::<Vec<_>>()
                .join("\n")
        }
        ExportFormat::Markdown => {
            fn cell(v: Option<&str>) -> String {
                v.unwrap_or("").replace('|', "\\|").replace("\r\n", " ").replace('\n', " ")
            }
            let (head, rows) = laid_out(d, options);
            let width = head.as_ref().map(Vec::len).or_else(|| rows.first().map(Vec::len)).unwrap_or(0);
            // A Markdown table always has a header row: blank when left out.
            let header_shown = head.is_some();
            let head = head.unwrap_or_else(|| vec![String::new(); width]);
            let align: Vec<String> = if options.transpose || !header_shown {
                head.iter().map(|_| "---".to_string()).collect()
            } else {
                let mut a: Vec<String> = if options.row_header { vec!["--:".into()] } else { Vec::new() };
                a.extend(numeric.iter().map(|&n| (if n { "--:" } else { "---" }).to_string()));
                a
            };
            let mut lines: Vec<Vec<String>> = vec![head.iter().map(|h| cell(Some(h))).collect(), align];
            lines.extend(rows.iter().map(|r| r.iter().map(|v| cell(v.as_deref())).collect()));
            lines.iter().map(|r| format!("| {} |", r.join(" | "))).collect::<Vec<_>>().join("\n")
        }
        ExportFormat::OneRow => to_one_row(&d.rows, is_numeric),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn data() -> ExportData {
        ExportData {
            columns: vec![
                ExportColumn { name: "id".into(), type_name: "INTEGER".into() },
                ExportColumn { name: "name".into(), type_name: "TEXT".into() },
            ],
            rows: vec![vec![Some("1".into()), Some("O'Hara".into())], vec![Some("2".into()), None]],
            table: "order items".into(),
            driver: DbDriver::Postgres,
        }
    }

    #[test]
    fn writes_json_with_numbers_and_nulls() {
        let out: serde_json::Value =
            serde_json::from_str(&export_data(ExportFormat::Json, &data(), ExportOptions::default())).expect("JSON");
        assert_eq!(out, json!([{ "id": 1, "name": "O'Hara" }, { "id": 2, "name": null }]));
    }

    #[test]
    fn writes_sql_inserts_with_quoted_names_and_literals() {
        assert_eq!(
            export_data(ExportFormat::Sql, &data(), ExportOptions::default()),
            "INSERT INTO \"order items\" (id, name) VALUES (1, 'O''Hara');\nINSERT INTO \"order items\" (id, name) VALUES (2, NULL);"
        );
    }

    #[test]
    fn preserves_backslashes_in_clickhouse_sql_exports() {
        let d = ExportData {
            driver: DbDriver::Clickhouse,
            rows: vec![vec![Some("1".into()), Some("C:\\new\\file's".into())]],
            ..data()
        };
        assert_eq!(
            export_data(ExportFormat::Sql, &d, ExportOptions::default()),
            "INSERT INTO \"order items\" (id, name) VALUES (1, 'C:\\\\new\\\\file''s');"
        );
    }

    #[test]
    fn writes_a_markdown_table_numbers_right_aligned() {
        let d = ExportData { rows: vec![vec![Some("1".into()), Some("a|b".into())]], ..data() };
        assert_eq!(
            export_data(ExportFormat::Markdown, &d, ExportOptions::default()),
            "| id | name |\n| --: | --- |\n| 1 | a\\|b |"
        );
    }
}
