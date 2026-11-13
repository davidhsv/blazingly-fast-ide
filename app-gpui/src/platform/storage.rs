//! UI state that outlives a session: tool window layouts, search history, the last used
//! values of dialogs. The webview keeps these in `localStorage` under string keys; here the
//! same keys and the same JSON values live in one file of the configuration directory, so a
//! value written by one of the two apps reads the same in the other.

use gpui_kit::Global;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// File name inside the configuration directory.
pub const FILE_NAME: &str = "ui-storage.json";

/// String values by key, written through to disk on every change.
#[derive(Debug, Default)]
pub struct UiStorage {
    /// `None` keeps the values in memory only (tests, and when the directory is unusable).
    file: Option<PathBuf>,
    values: BTreeMap<String, String>,
}

impl Global for UiStorage {}

impl UiStorage {
    /// Reads `config_dir`'s storage file. A missing file is an empty store; an unreadable or
    /// malformed one is logged and treated as empty, and is replaced by the next write.
    pub fn load(config_dir: &Path) -> Self {
        let file = config_dir.join(FILE_NAME);
        let values = match std::fs::read_to_string(&file) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
                tracing::warn!("{} is not valid: {error}", file.display());
                BTreeMap::new()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => {
                tracing::warn!("cannot read {}: {error}", file.display());
                BTreeMap::new()
            }
        };
        Self { file: Some(file), values }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    /// Every stored entry, in key order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values.iter().map(|(key, value)| (key.as_str(), value.as_str()))
    }

    /// Stores `value` under `key`. A failed write is logged; the value stays for this session.
    pub fn set(&mut self, key: &str, value: String) {
        if self.values.get(key) == Some(&value) {
            return;
        }
        self.values.insert(key.to_owned(), value);
        self.write();
    }

    pub fn remove(&mut self, key: &str) {
        if self.values.remove(key).is_some() {
            self.write();
        }
    }

    /// Writes the whole store next to its file and renames it into place, so a crash mid-write
    /// leaves the previous file intact.
    fn write(&self) {
        let Some(file) = &self.file else { return };
        let written = serde_json::to_string_pretty(&self.values).map_err(std::io::Error::other).and_then(|text| {
            let temporary = file.with_extension("json.tmp");
            std::fs::write(&temporary, text)?;
            std::fs::rename(&temporary, file)
        });
        if let Err(error) = written {
            tracing::warn!("cannot write {}: {error}", file.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_survive_a_restart_and_bad_files_start_empty() {
        let dir = tempfile::tempdir().expect("a temporary folder");
        let mut storage = UiStorage::load(dir.path());
        assert_eq!(storage.get("a"), None);
        storage.set("a", "{\"v\":2}".to_owned());
        storage.set("b", "x".to_owned());
        storage.remove("b");
        storage.remove("never");
        let reloaded = UiStorage::load(dir.path());
        assert_eq!((reloaded.get("a"), reloaded.get("b")), (Some("{\"v\":2}"), None));
        // No temporary file is left behind.
        assert!(!dir.path().join("ui-storage.json.tmp").exists());

        std::fs::write(dir.path().join(FILE_NAME), "not json").expect("writable");
        let mut broken = UiStorage::load(dir.path());
        assert_eq!(broken.get("a"), None);
        broken.set("c", "1".to_owned());
        assert_eq!(UiStorage::load(dir.path()).get("c"), Some("1"));
    }

    #[test]
    fn a_store_without_a_file_keeps_values_in_memory() {
        let mut storage = UiStorage::default();
        storage.set("k", "v".to_owned());
        assert_eq!(storage.get("k"), Some("v"));
    }
}
