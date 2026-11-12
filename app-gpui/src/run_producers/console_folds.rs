//! Folding of console output: runs of consecutive lines that contain one of the configured patterns
//! (stack frames of a test framework, say) collapse onto the line before them.

use crate::run::console::{ChunkKind, ConsoleLine};
use std::collections::HashSet;

/// A run of folded lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fold {
    /// Index of the run's first line.
    pub start: usize,
    pub count: usize,
    /// Id of the run's first line: what expanding it remembers.
    pub id: u64,
    pub expanded: bool,
}

/// A row the console shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayRow {
    /// The line at `index`, with the fold that hangs off it.
    Line { index: usize, fold: Option<Fold> },
    /// A collapsed fold with no line before it.
    Fold(Fold),
}

/// Folds runs of consecutive output lines (one or more) that contain one of `patterns`; runs whose
/// first line id is in `expanded` stay open.
pub fn fold_rows(lines: &[ConsoleLine], patterns: &[String], expanded: &HashSet<u64>) -> Vec<DisplayRow> {
    let matches = |line: &ConsoleLine| {
        line.kind != ChunkKind::System && patterns.iter().any(|pattern| line.text.contains(pattern.as_str()))
    };
    let mut rows: Vec<DisplayRow> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if !matches(&lines[i]) {
            rows.push(DisplayRow::Line { index: i, fold: None });
            i += 1;
            continue;
        }
        let count = lines[i..].iter().take_while(|line| matches(line)).count();
        let fold = Fold { start: i, count, id: lines[i].id, expanded: expanded.contains(&lines[i].id) };
        if let Some(DisplayRow::Line { fold: hung, .. }) = rows.last_mut() {
            *hung = Some(fold);
        } else if !fold.expanded {
            rows.push(DisplayRow::Fold(fold));
        }
        if fold.expanded {
            rows.extend((i..i + count).map(|index| DisplayRow::Line { index, fold: None }));
        }
        i += count;
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::console::{ConsoleModel, ConsoleOptions};

    const JUNIT: &str = "at org.junit.";

    /// The rows as text: a line trimmed, with `+n` for the fold hanging off it.
    fn show(model: &ConsoleModel, rows: &[DisplayRow]) -> Vec<String> {
        let line = |index: &usize| model.lines()[*index].text.trim().to_string();
        let text = |row: &DisplayRow| match row {
            DisplayRow::Fold(fold) => format!("fold{}", fold.count),
            DisplayRow::Line { index, fold: Some(fold) } => format!("{} +{}", line(index), fold.count),
            DisplayRow::Line { index, fold: None } => line(index),
        };
        rows.iter().map(text).collect()
    }

    #[test]
    fn keeps_the_cycle_buffer_under_the_character_limit() {
        let mut model = ConsoleModel::new(ConsoleOptions { max_chars: Some(100), ..ConsoleOptions::default() });
        for i in 0..50 {
            model.append(ChunkKind::Stdout, &format!("line {i}\n"));
        }
        let chars: usize = model.lines().iter().map(|line| line.text.len() + 1).sum();
        assert!(chars <= 100, "{chars}");
        assert_eq!(model.lines().last().map(|line| line.text.as_str()), Some("line 49"));
        assert!(model.text().contains("line 49"));
        assert!(model.dropped() > 0);
    }

    #[test]
    fn folds_runs_of_matching_lines_onto_the_previous_line_single_lines_too() {
        let mut model = ConsoleModel::new(ConsoleOptions::default());
        let trace = "Error\n  at org.junit.A\n  at org.junit.B\n  at app.Main\n  at org.junit.C\n";
        model.append(ChunkKind::Stderr, trace);
        let patterns = [JUNIT.to_string()];
        let rows = fold_rows(model.lines(), &patterns, &HashSet::new());
        assert_eq!(show(&model, &rows), ["Error +2", "at app.Main +1"]);
        let Some(DisplayRow::Line { fold: Some(first), .. }) = rows.first() else { panic!("a line with a fold") };
        let open = fold_rows(model.lines(), &patterns, &HashSet::from([first.id]));
        assert_eq!(show(&model, &open), ["Error +2", "at org.junit.A", "at org.junit.B", "at app.Main +1"]);
        assert!(matches!(open.first(), Some(DisplayRow::Line { fold: Some(fold), .. }) if fold.expanded));
    }

    #[test]
    fn keeps_a_fold_with_no_line_before_it_as_a_row_of_its_own() {
        let mut model = ConsoleModel::new(ConsoleOptions::default());
        model.append(ChunkKind::Stdout, "  at org.junit.A\nok\n");
        let rows = fold_rows(model.lines(), &[JUNIT.to_string()], &HashSet::new());
        assert!(matches!(rows[..], [DisplayRow::Fold(_), DisplayRow::Line { .. }]), "{rows:?}");
    }

    #[test]
    fn exposes_the_open_prompt_line() {
        let mut model = ConsoleModel::new(ConsoleOptions::default());
        model.append(ChunkKind::Stdout, "done\nName: ");
        assert_eq!(model.open_line().map(|line| line.text.as_str()), Some("Name: "));
        model.append(ChunkKind::Stdout, "x\n");
        assert!(model.open_line().is_none());
    }
}
