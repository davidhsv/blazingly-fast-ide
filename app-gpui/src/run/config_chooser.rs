//! The rows of the Run/Debug configuration chooser popup (⌃⌥R).

use crate::run::config_types::RunConfigTypes;
use crate::run::configs::group_configs;
use crate::ui::matching::fuzzy_match;
use ide_run::RunConfig;

const CURRENT_FILE_LABEL: &str = "Current File";

/// A row of the chooser. `indices`: the characters of the label the search matched.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// "Save '<name>' Configuration", while a temporary configuration is selected.
    Save {
        config: Box<RunConfig>,
    },
    /// The permanent "Current File" entry: runs the active file's context.
    Current {
        indices: Option<Vec<usize>>,
    },
    Section {
        key: String,
        label: String,
    },
    Config {
        config: Box<RunConfig>,
        indices: Option<Vec<usize>>,
    },
    /// "Edit Configurations…".
    Edit,
}

impl Row {
    /// The row's identity in the list.
    pub fn key(&self) -> String {
        match self {
            Self::Save { .. } => "save".to_string(),
            Self::Current { .. } => "current".to_string(),
            Self::Section { key, .. } => key.clone(),
            Self::Config { config, .. } => format!("c:{}", config.id),
            Self::Edit => "edit".to_string(),
        }
    }
}

/// What the chooser lists, top to bottom.
#[derive(Clone, Copy, Debug)]
pub struct ChooserState<'a> {
    pub configs: &'a [RunConfig],
    pub selected: Option<&'a RunConfig>,
    pub temporary: &'a [String],
    /// Pinned configurations come first, in the order they were pinned.
    pub pinned: &'a [String],
    /// Then the ones started last, newest first.
    pub recent: &'a [String],
}

pub fn build_rows(state: ChooserState, query: &str, types: &RunConfigTypes) -> Vec<Row> {
    let mut rows = Vec::new();
    // IDEA: "Save '<name>' Configuration" first while a temporary configuration is selected.
    let temporary = |config: &&RunConfig| state.temporary.contains(&config.id);
    if let Some(selected) = state.selected.filter(|selected| query.is_empty() && temporary(selected)) {
        rows.push(Row::Save { config: Box::new(selected.clone()) });
    }
    let matched = |label: &str| if query.is_empty() { Some(None) } else { fuzzy_match(label, query).map(Some) };
    if let Some(indices) = matched(CURRENT_FILE_LABEL) {
        rows.push(Row::Current { indices });
    }
    let section = |rows: &mut Vec<Row>, key: String, label: String, configs: &[&RunConfig]| {
        let row = |config: &RunConfig| {
            Some(Row::Config { indices: matched(&config.name)?, config: Box::new(config.clone()) })
        };
        let found: Vec<Row> = configs.iter().filter_map(|config| row(config)).collect();
        if !found.is_empty() {
            rows.push(Row::Section { key, label });
            rows.extend(found);
        }
    };
    let by_id = |id: &String| state.configs.iter().find(|config| config.id == *id);
    let pinned: Vec<&RunConfig> = state.pinned.iter().filter_map(by_id).collect();
    section(&mut rows, "s:pinned".to_string(), "Pinned".to_string(), &pinned);
    let recent: Vec<&RunConfig> =
        state.recent.iter().filter(|id| !state.pinned.contains(id)).filter_map(by_id).collect();
    section(&mut rows, "s:recent".to_string(), "Recent".to_string(), &recent);
    let listed = |config: &RunConfig| pinned.iter().chain(&recent).any(|shown| shown.id == config.id);
    let rest: Vec<RunConfig> = state.configs.iter().filter(|config| !listed(config)).cloned().collect();
    for group in group_configs(&rest, types) {
        let configs: Vec<&RunConfig> = group.configs.iter().collect();
        section(&mut rows, format!("s:{}", group.kind), group.label.clone(), &configs);
    }
    rows.push(Row::Edit);
    rows
}

/// Numeric mnemonics (IDEA: 1–9 run the Nth configuration while the search is empty), by row key.
pub fn mnemonics(rows: &[Row]) -> Vec<(String, u8)> {
    let configs = rows.iter().filter(|row| matches!(row, Row::Config { .. }));
    configs.take(9).zip(1u8..).map(|(row, number)| (row.key(), number)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::application::{JvmContext, application_config};
    use crate::run::configs::blank_config;

    fn cfg(id: &str, kind: &str) -> RunConfig {
        RunConfig { command: "echo".into(), detected: true, ..blank_config(id, id, kind) }
    }

    #[test]
    fn offers_save_in_the_chooser_while_a_temporary_configuration_is_selected() {
        let app = application_config("a.Main", false, "src/main/java/a/Main.java", JvmContext::default());
        let configs = [cfg("maven:spring-boot:run", "maven"), app.clone()];
        let temporary = [app.id.clone()];
        let state =
            ChooserState { configs: &configs, selected: Some(&app), temporary: &temporary, pinned: &[], recent: &[] };
        let types = RunConfigTypes::new();
        let rows = build_rows(state, "", &types);
        assert!(matches!(&rows[0], Row::Save { config } if config.id == app.id));
        assert!(!matches!(build_rows(state, "x", &types).first(), Some(Row::Save { .. })));
    }

    // Not from a spec: pinned configurations lead, then the recent ones, then the rest by kind.
    #[test]
    fn lists_pinned_then_recent_then_groups_by_kind() {
        let configs = [cfg("a", "npm"), cfg("b", "cargo"), cfg("c", "npm"), cfg("d", "cargo")];
        let (pinned, recent) = (["c".to_string()], ["d".to_string(), "c".to_string()]);
        let state =
            ChooserState { configs: &configs, selected: None, temporary: &[], pinned: &pinned, recent: &recent };
        let rows = build_rows(state, "", &RunConfigTypes::new());
        let keys: Vec<String> = rows.iter().map(Row::key).collect();
        let expected = ["current", "s:pinned", "c:c", "s:recent", "c:d", "s:npm", "c:a", "s:cargo", "c:b", "edit"];
        assert_eq!(keys, expected);
        assert_eq!(mnemonics(&rows)[..2], [("c:c".to_string(), 1), ("c:d".to_string(), 2)]);
        let searched = build_rows(state, "b", &RunConfigTypes::new());
        assert_eq!(searched.iter().map(Row::key).collect::<Vec<_>>(), ["s:cargo", "c:b", "edit"]);
    }
}
