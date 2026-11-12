//! The environment variables table of a run configuration: rows, their validation and pasting.

use crate::run::configs::{EnvEntry, parse_env};
use std::collections::BTreeMap;

/// The rows of a configuration's environment.
pub fn env_rows(env: &BTreeMap<String, String>) -> Vec<EnvEntry> {
    env.iter().map(|(key, value)| EnvEntry { key: key.clone(), value: value.clone() }).collect()
}

/// Env map from table rows; blank rows are ignored. `Err` carries what was read up to the bad row and
/// why it is bad.
pub fn env_from_rows(rows: &[EnvEntry]) -> Result<BTreeMap<String, String>, (BTreeMap<String, String>, String)> {
    let mut env = BTreeMap::new();
    for row in rows {
        let key = row.key.trim();
        if key.is_empty() && row.value.is_empty() {
            continue;
        }
        if key.is_empty() {
            return Err((env, "Environment variable names can't be empty".to_string()));
        }
        if env.contains_key(key) {
            return Err((env, format!("Duplicate environment variable: {key}")));
        }
        env.insert(key.to_string(), row.value.clone());
    }
    Ok(env)
}

/// Rows as `NAME=value` lines (what Copy puts on the clipboard).
pub fn rows_text(rows: &[EnvEntry]) -> String {
    rows.iter().map(|row| format!("{}={}", row.key, row.value)).collect::<Vec<_>>().join("\n")
}

/// Pasted variables replace rows with the same name and are appended otherwise.
pub fn paste_rows(rows: &[EnvEntry], text: &str) -> Vec<EnvEntry> {
    let mut out: Vec<EnvEntry> =
        rows.iter().filter(|row| !row.key.is_empty() || !row.value.is_empty()).cloned().collect();
    for pasted in parse_env(text) {
        match out.iter_mut().find(|row| row.key == pasted.key) {
            Some(existing) => existing.value = pasted.value,
            None => out.push(pasted),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, value: &str) -> EnvEntry {
        EnvEntry { key: key.into(), value: value.into() }
    }

    #[test]
    fn validates_env_table_rows() {
        let env = env_from_rows(&[row("A", "1"), row("", "")]);
        assert_eq!(env, Ok(BTreeMap::from([("A".to_string(), "1".to_string())])));
        assert!(env_from_rows(&[row("", "x")]).is_err_and(|(_, error)| error.contains("empty")));
        assert!(env_from_rows(&[row("A", "1"), row("A", "2")]).is_err_and(|(_, error)| error.contains("Duplicate")));
    }

    // Not from a spec: pasted variables replace rows of the same name, and rows copy as lines.
    #[test]
    fn pasting_replaces_rows_of_the_same_name() {
        let rows = [row("A", "1"), row("", ""), row("B", "2")];
        let pasted = paste_rows(&rows, "B=3\nC=4");
        assert_eq!(pasted, [row("A", "1"), row("B", "3"), row("C", "4")]);
        assert_eq!(rows_text(&pasted), "A=1\nB=3\nC=4");
        assert_eq!(env_rows(&BTreeMap::from([("A".to_string(), "1".to_string())])), [row("A", "1")]);
    }
}
