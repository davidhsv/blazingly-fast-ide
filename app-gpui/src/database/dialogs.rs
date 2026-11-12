//! State of the Data Source and Export Data dialogs: the fields a new data source starts with (no
//! password is saved unless asked to), the SSH tab's edits, and the export's format, options,
//! bounded preview and output file.

use super::export::{EXPORT_FORMATS, ExportData, ExportFormat, ExportOptions, export_data};
use super::grid::cell_preview;
use ide_db::{DataSource, DbDriver, DbSsh, DbSshAuth, DbSslMode};
use std::path::Path;

/// The fields of New ▸ Data Source: PostgreSQL on localhost, nothing saved.
pub fn blank_source(id: String) -> DataSource {
    DataSource {
        id,
        driver: DbDriver::Postgres,
        host: "localhost".into(),
        ssl: DbSslMode::Prefer,
        save_password: false,
        ..DataSource::default()
    }
}

/// The SSH tab before it is first used: OpenSSH config + agent, nothing saved.
pub fn blank_ssh() -> DbSsh {
    DbSsh { enabled: false, auth: DbSshAuth::OpenSsh, save_secret: false, ..DbSsh::default() }
}

/// An edit of the SSH tab, made on its current settings (or the blank ones).
pub fn set_ssh(source: &mut DataSource, edit: impl FnOnce(&mut DbSsh)) {
    let ssh = source.ssh.get_or_insert_with(blank_ssh);
    edit(ssh);
}

/// Rows the export preview shows.
pub const PREVIEW_ROWS: usize = 100;

/// The formats whose layout options (header, row numbers, transpose) apply.
pub fn is_tabular(format: ExportFormat) -> bool {
    matches!(format, ExportFormat::Csv | ExportFormat::Tsv | ExportFormat::Markdown)
}

fn extension(format: ExportFormat) -> &'static str {
    EXPORT_FORMATS.iter().find(|f| f.id == format).map_or("txt", |f| f.extension)
}

/// The Export Data dialog.
#[derive(Clone, Debug)]
pub struct ExportDialog {
    pub data: ExportData,
    pub format: ExportFormat,
    pub options: ExportOptions,
    /// The output file: `<table>.<ext>`, in the home directory once that is known.
    pub file: String,
}

impl ExportDialog {
    pub fn new(data: ExportData, format: ExportFormat) -> Self {
        let file = format!("{}.{}", data.table, extension(format));
        Self { data, format, options: ExportOptions::default(), file }
    }

    /// The home directory arrived: a bare file name goes there.
    pub fn set_home(&mut self, home: &str) {
        if !self.file.contains(['/', '\\']) {
            self.file = Path::new(home).join(&self.file).to_string_lossy().into_owned();
        }
    }

    /// Another format: the file follows its extension.
    pub fn set_format(&mut self, format: ExportFormat) {
        self.format = format;
        let next = extension(format);
        let name_start = self.file.rfind(['/', '\\']).map_or(0, |i| i + 1);
        match self.file[name_start..].rfind('.') {
            Some(dot) => self.file = format!("{}.{next}", &self.file[..name_start + dot]),
            None => self.file = format!("{}.{next}", self.file),
        }
    }

    fn effective(&self) -> ExportOptions {
        if is_tabular(self.format) { self.options } else { ExportOptions::default() }
    }

    /// The preview: the first rows, large values bounded.
    pub fn preview(&self) -> String {
        let rows = self
            .data
            .rows
            .iter()
            .take(PREVIEW_ROWS)
            .map(|r| r.iter().map(|v| v.as_deref().map(cell_preview)).collect())
            .collect();
        export_data(self.format, &ExportData { rows, ..self.data.clone() }, self.effective())
    }

    /// Rows the preview leaves out ("… N more rows").
    pub fn more_rows(&self) -> usize {
        self.data.rows.len().saturating_sub(PREVIEW_ROWS)
    }

    /// Export to File: the backend's `dbExportFile` with the path and every row in full.
    pub fn export(&self) -> (String, String) {
        (self.file.clone(), export_data(self.format, &self.data, self.effective()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::export::ExportColumn;
    use super::*;

    #[test]
    fn does_not_save_the_database_password_unless_asked_to() {
        assert!(!blank_source("id".into()).save_password);
    }

    #[test]
    fn does_not_save_the_ssh_tunnel_password_unless_asked_to() {
        let mut s = blank_source("id".into());
        set_ssh(&mut s, |ssh| ssh.enabled = true);
        set_ssh(&mut s, |ssh| ssh.auth = DbSshAuth::Password);
        assert!(!s.save_password);
        assert_eq!(s.ssh.map(|ssh| ssh.save_secret), Some(false));
    }

    #[test]
    fn bounds_preview_values_while_exporting_every_row_in_full() {
        let full = serde_json::json!({ "text": "\u{4e2d} text ".repeat(200_000), "tail": "complete" }).to_string();
        let rows = vec![vec![Some(full.clone())], vec![Some("short".to_string())], vec![None]];
        let data = ExportData {
            columns: vec![ExportColumn { name: "payload".into(), type_name: "jsonb".into() }],
            rows,
            table: "big".into(),
            driver: DbDriver::Postgres,
        };
        let mut dialog = ExportDialog::new(data, ExportFormat::Json);
        let preview: serde_json::Value = serde_json::from_str(&dialog.preview()).expect("JSON");
        assert_eq!(
            preview,
            serde_json::json!([{ "payload": cell_preview(&full) }, { "payload": "short" }, { "payload": null }])
        );
        dialog.set_home("/home/test");
        assert_eq!(dialog.file, "/home/test/big.json");
        let (path, contents) = dialog.export();
        assert_eq!(path, "/home/test/big.json");
        let exported: serde_json::Value = serde_json::from_str(&contents).expect("JSON");
        assert_eq!(exported, serde_json::json!([{ "payload": full }, { "payload": "short" }, { "payload": null }]));
    }
}
